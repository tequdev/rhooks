// e2e: examples/81_govern against a standalone Xahau node.
//
// Reproduces a subset of xahaud's XahauGenesis_test.cpp governance
// coverage. Test <-> XahauGenesis_test.cpp correspondence:
//
//   | This suite                                   | XahauGenesis_test.cpp                        |
//   |-----------------------------------------------|-----------------------------------------------|
//   | 'setup: first Invoke populates the seat table' | XahauGenesis (initial member/seat setup via   |
//   |                                                 | the SetHook-time HookParameters, IMC/IS*)     |
//   | 'seat vote: reaches threshold and actions'     | testVotableValue / seat-change voting cases    |
//   | 'seat vote: below threshold just records'      | same, pre-threshold assertions                 |
//   | 'reward vote: L1 table actions RR directly'    | RR/RD voting cases (genesis account only)      |
//
// Two deliberate scope reductions from XahauGenesis_test.cpp, both
// documented here rather than silently skipped:
//
// 1. XahauGenesis_test.cpp drives the *real* genesis bootstrap (the
//    `featureXahauGenesis` amendment installing govern.c/reward.c on the
//    protocol's hardcoded genesis account and blackholing it) via JTX,
//    which a standalone xahaud node run through `xrpld-netgen` does not
//    replicate. What a standalone node's own bootstrap *does* replicate
//    is ordinary: ledger 1's master/genesis account, funded with the
//    full native-currency supply, derived from the well-known
//    `masterpassphrase` seed (`snoPBrXtMeMyMHUVTgbuqAfg1SUTb`,
//    secp256k1) - which is *exactly* `GENESIS_ACCOUNT` in both this
//    crate and `examples/80_reward` (verified: `Wallet.fromSeed(...)`'s
//    resulting `AccountID` matches the hardcoded bytes byte-for-byte -
//    see `examples/81_govern/README.md`'s Testing section). This is also
//    exactly `@transia/hooks-toolkit`'s own `MASTER_WALLET`/
//    `testContext.master`, and the toolkit's own `initGovernTable`/
//    `setGovernTable` helpers (`fundSystem.js`) already use this account
//    for governance testing the same way. So: this suite *can* and does
//    install `govern`/`reward` on the real genesis account and exercise
//    true L1-table behavior (unlike a from-scratch reimplementation of
//    the amendment's ledger manipulation, which is out of scope) - but
//    the account starts as an ordinary (non-blackholed, master-key-
//    active) funded account, not mid-amendment-activation, so anything
//    that depends on the *amendment's own* one-time ledger effects
//    (`GenesisMints`/`NonGovernanceDistribution` initial balances, the
//    master-key-disable/blackhole, `TestL1Membership`) is not reproduced
//    - only govern.c/reward.c's own hook logic is under test.
// 2. Most seat/hook-vote scenarios below use ordinary funded accounts as
//    an L2 table instead, which needs no genesis control at all and
//    exercises the same voting/threshold/actioning state machine.
import {
  ExecutionUtility,
  Xrpld,
  clearAllHooksV3,
  hexNamespace,
  readHookBinaryHexFromNS,
  serverUrl,
  setHooksV3,
  setupClient,
  teardownClient,
  type SetHookParams,
  type XrplIntegrationTestContext,
  type iHook,
} from '@transia/hooks-toolkit'
import { calculateHookOn, convertStringToHex, decodeAccountID } from 'xahau'
import { HookFlags } from 'xahau/dist/npm/models/common/xahau'

// `@transia/hooks-toolkit` depends on `@transia/xrpl` (a separate fork
// from the `xahau` package used elsewhere in this suite) but doesn't
// re-export its `Wallet` type; derive it from `XrplIntegrationTestContext`
// itself instead of adding a direct dependency on `@transia/xrpl`.
type Wallet = XrplIntegrationTestContext['alice']

const namespace = 'rhooks-e2e-govern'
// hooks-build's printed worst case for govern.wasm (`mise run build-examples`).
const WORST_CASE_HOOK_INSTRUCTIONS = 41510

function accountIdHex(classicAddress: string): string {
  return Buffer.from(decodeAccountID(classicAddress)).toString('hex').toUpperCase()
}

function hookParam(name: string, valueHex: string) {
  return {
    HookParameter: {
      HookParameterName: convertStringToHex(name),
      HookParameterValue: valueHex,
    },
  }
}

/** `{'I','S',seat}` -> the member's 20-byte AccountId, hex. */
function isParam(seat: number, wallet: Wallet) {
  return {
    HookParameter: {
      HookParameterName: Buffer.from([0x49, 0x53, seat]).toString('hex').toUpperCase(),
      HookParameterValue: accountIdHex(wallet.classicAddress),
    },
  }
}

async function installGovern(
  testContext: XrplIntegrationTestContext,
  table: Wallet,
  members: Wallet[],
  extra: ReturnType<typeof hookParam>[] = [],
) {
  const hook: iHook = {
    CreateCode: readHookBinaryHexFromNS('govern', 'wasm'),
    Flags: HookFlags.hsfOverride,
    HookOn: calculateHookOn(['Invoke']),
    HookNamespace: hexNamespace(namespace),
    HookApiVersion: 0,
    HookParameters: [
      hookParam('IMC', members.length.toString(16).padStart(2, '0')),
      ...members.map((m, i) => isParam(i, m)),
      ...extra,
    ],
  } as iHook
  await setHooksV3({
    client: testContext.client,
    seed: table.seed,
    hooks: [{ Hook: hook }],
  } as unknown as SetHookParams)
}

async function invoke(
  testContext: XrplIntegrationTestContext,
  from: Wallet,
  table: Wallet,
  params: ReturnType<typeof hookParam>[] = [],
) {
  return Xrpld.submit(testContext.client, {
    tx: {
      TransactionType: 'Invoke',
      Account: from.classicAddress,
      Destination: table.classicAddress,
      HookParameters: params,
    } as any,
    wallet: from,
  })
}

function topicParam(topicType: string, topicId: number) {
  return hookParam('T', Buffer.from([topicType.charCodeAt(0), topicId]).toString('hex').toUpperCase())
}

function voteParam(valueHex: string) {
  return hookParam('V', valueHex)
}

function layerParam(layer: number) {
  return hookParam('L', Buffer.from([layer]).toString('hex').toUpperCase())
}

describe('govern: L2 table setup', () => {
  let testContext: XrplIntegrationTestContext

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)
  })

  afterAll(async () => {
    await clearAllHooksV3({
      client: testContext.client,
      seed: testContext.hook1.seed,
    } as unknown as SetHookParams)
    await teardownClient(testContext)
  })

  it('first Invoke on a fresh table populates the seat table and accepts', async () => {
    await installGovern(testContext, testContext.hook1, [
      testContext.alice,
      testContext.bob,
      testContext.carol,
    ])

    const response = await invoke(testContext, testContext.alice, testContext.hook1)
    const meta = response.meta as any
    const hookExecutions = await ExecutionUtility.getHookExecutionsFromMeta(testContext.client, meta)
    expect(hookExecutions.executions.length).toBe(1)
    const execution = hookExecutions.executions[0]
    expect(execution.HookReturnString).toBe('Governance: Setup completed successfully.')
    expect(parseInt(execution.HookInstructionCount, 16)).toBeLessThanOrEqual(
      WORST_CASE_HOOK_INSTRUCTIONS,
    )
  })
})

describe('govern: L2 table seat voting', () => {
  let testContext: XrplIntegrationTestContext

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)
    await installGovern(testContext, testContext.hook1, [
      testContext.alice,
      testContext.bob,
      testContext.carol,
    ])
    await invoke(testContext, testContext.alice, testContext.hook1)
  })

  afterAll(async () => {
    await clearAllHooksV3({
      client: testContext.client,
      seed: testContext.hook1.seed,
    } as unknown as SetHookParams)
    await teardownClient(testContext)
  })

  it('a single vote below the 80% seat threshold (2 of 3) just records', async () => {
    // Seat 2 (carol) -> dave, layer 2 (an L2 table's own seat topic).
    const response = await invoke(testContext, testContext.alice, testContext.hook1, [
      topicParam('S', 2),
      voteParam(accountIdHex(testContext.dave.classicAddress)),
      layerParam(2),
    ])
    const meta = response.meta as any
    const hookExecutions = await ExecutionUtility.getHookExecutionsFromMeta(testContext.client, meta)
    expect(hookExecutions.executions[0].HookReturnString).toBe(
      'Governance: Vote record. Not yet enough votes to action.',
    )
  })

  it('a second vote reaches the threshold and actions the seat change', async () => {
    const response = await invoke(testContext, testContext.bob, testContext.hook1, [
      topicParam('S', 2),
      voteParam(accountIdHex(testContext.dave.classicAddress)),
      layerParam(2),
    ])
    const meta = response.meta as any
    const hookExecutions = await ExecutionUtility.getHookExecutionsFromMeta(testContext.client, meta)
    expect(hookExecutions.executions[0].HookReturnString).toBe('Governance: Action member change.')
  })

  it('casting the identical vote again is a no-op accept', async () => {
    const response = await invoke(testContext, testContext.bob, testContext.hook1, [
      topicParam('S', 2),
      voteParam(accountIdHex(testContext.dave.classicAddress)),
      layerParam(2),
    ])
    const meta = response.meta as any
    const hookExecutions = await ExecutionUtility.getHookExecutionsFromMeta(testContext.client, meta)
    // dave now holds seat 2 (from the previous test), so bob voting for
    // dave again on the same topic is an identical-vote no-op - but bob
    // himself never re-votes in this suite otherwise, so this exercises
    // the "already cast this way" path deterministically.
    expect(hookExecutions.executions[0].HookReturnString).toBe(
      'Governance: Your vote is already cast this way for this topic.',
    )
  })
})

describe('govern: L1 table (real genesis account) reward-rate vote', () => {
  let testContext: XrplIntegrationTestContext

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)
  })

  afterAll(async () => {
    await clearAllHooksV3({
      client: testContext.client,
      seed: testContext.master.seed,
    } as unknown as SetHookParams)
    await teardownClient(testContext)
  })

  it('installs on the real genesis account and completes L1 setup', async () => {
    // `testContext.master` (`@transia/hooks-toolkit`'s `MASTER_WALLET`,
    // seed `snoPBrXtMeMyMHUVTgbuqAfg1SUTb`) is `GENESIS_ACCOUNT` - see
    // this file's header comment.
    await installGovern(
      testContext,
      testContext.master,
      [testContext.alice, testContext.bob, testContext.carol],
      [
        hookParam('IRR', '0000000000000000'),
        hookParam('IRD', '0100000000000000'),
      ],
    )

    const response = await invoke(testContext, testContext.alice, testContext.master)
    const meta = response.meta as any
    const hookExecutions = await ExecutionUtility.getHookExecutionsFromMeta(testContext.client, meta)
    expect(hookExecutions.executions[0].HookReturnString).toBe(
      'Governance: Setup completed successfully.',
    )
  })

  it('a unanimous RR vote (3 of 3, 100% required at L1) actions the reward rate', async () => {
    // A recognizable nonzero XFL bit pattern - this test only checks
    // that the vote is *actioned*, not the specific rate; see
    // examples/80_reward's own state-layout table for how "RR" is
    // interpreted.
    const rrValue = '0100000000000000'
    await invoke(testContext, testContext.alice, testContext.master, [
      topicParam('R', 'R'.charCodeAt(0)),
      voteParam(rrValue),
    ])
    await invoke(testContext, testContext.bob, testContext.master, [
      topicParam('R', 'R'.charCodeAt(0)),
      voteParam(rrValue),
    ])
    const response = await invoke(testContext, testContext.carol, testContext.master, [
      topicParam('R', 'R'.charCodeAt(0)),
      voteParam(rrValue),
    ])
    const meta = response.meta as any
    const hookExecutions = await ExecutionUtility.getHookExecutionsFromMeta(testContext.client, meta)
    expect(hookExecutions.executions[0].HookReturnString).toBe(
      'Governance: Reward rate change actioned!',
    )
  })
})
