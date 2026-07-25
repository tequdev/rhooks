// e2e: examples/06_guard-patterns against a standalone Xahau node.
//
// `hook()` reads the otxn sender and a 20-byte blocked AccountId from the
// `BL` HookParameter (same idiom as `firewall`), rolling back with
// `GuardPatternsError::BlockedAccount` (code 2) on a match; accepts
// otherwise with `"guard-patterns: accepted"` and an accept code that is
// a non-meaningful sum of two demonstration loops (see the source) - not
// asserted here beyond "present", since its value depends on the test
// wallets' generated AccountIds. HookOn is Invoke (this hook only reads
// `sfAccount`, present on every transaction type).
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
import {
  calculateHookOn,
  convertStringToHex,
  decodeAccountID,
  type TransactionMetadata,
} from 'xahau'
// HookFlags isn't re-exported from the package root in xahau@4.x - only
// reachable via this deep import (same path hooks-toolkit's own source
// uses internally for the same enum).
import { HookFlags } from 'xahau/dist/npm/models/common/xahau'

const namespace = 'rhooks-e2e-guard-patterns'
// hooks-build's printed worst case for guard_patterns.wasm (`mise run build-examples`).
const WORST_CASE_INSTRUCTIONS = 1292

function accountIdHex(classicAddress: string): string {
  return Buffer.from(decodeAccountID(classicAddress)).toString('hex').toUpperCase()
}

describe('guard-patterns', () => {
  let testContext: XrplIntegrationTestContext

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)

    const hook = {
      CreateCode: readHookBinaryHexFromNS('guard_patterns', 'wasm'),
      Flags: HookFlags.hsfOverride,
      HookOn: calculateHookOn(['Invoke']),
      HookNamespace: hexNamespace(namespace),
      HookApiVersion: 0,
      HookParameters: [
        {
          HookParameter: {
            HookParameterName: convertStringToHex('BL'),
            HookParameterValue: accountIdHex(testContext.bob.classicAddress),
          },
        },
      ],
    } as iHook
    await setHooksV3({
      client: testContext.client,
      seed: testContext.hook1.seed,
      hooks: [{ Hook: hook }],
    } as unknown as SetHookParams)
  })

  afterAll(async () => {
    await clearAllHooksV3({
      client: testContext.client,
      seed: testContext.hook1.seed,
    } as unknown as SetHookParams)
    await teardownClient(testContext)
  })

  it('rejects an Invoke from the blocked account', async () => {
    const response = Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'Invoke',
        Account: testContext.bob.classicAddress,
        Destination: testContext.hook1.classicAddress,
      },
      wallet: testContext.bob,
    })
    await expect(response).rejects.toThrow('guard-patterns: blocked account')
  })

  it('accepts an Invoke from a non-blocked account', async () => {
    const response = await Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'Invoke',
        Account: testContext.alice.classicAddress,
        Destination: testContext.hook1.classicAddress,
      },
      wallet: testContext.alice,
    })

    const meta = response.meta as TransactionMetadata
    const hookExecutions = await ExecutionUtility.getHookExecutionsFromMeta(
      testContext.client,
      meta,
    )
    expect(hookExecutions.executions.length).toBe(1)
    const execution = hookExecutions.executions[0]
    expect(execution.HookReturnString).toBe('guard-patterns: accepted')
    // HookInstructionCount is a *hex* string over RPC (confirmed by direct
    // inspection - e.g. "d" = 13).
    expect(parseInt(execution.HookInstructionCount, 16)).toBeLessThanOrEqual(
      WORST_CASE_INSTRUCTIONS,
    )
  })
})
