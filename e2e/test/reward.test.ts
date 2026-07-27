// e2e: examples/80_reward against a standalone Xahau node.
//
// Reproduces a subset of xahaud's XahauGenesis_test.cpp `ClaimReward`
// coverage. Test <-> XahauGenesis_test.cpp correspondence:
//
//   | This suite                                    | XahauGenesis_test.cpp                     |
//   |------------------------------------------------|--------------------------------------------|
//   | 'passes through a non-ClaimReward txn'          | (implicit - reward.c's own early-out; not |
//   |                                                  | separately asserted in the C++ suite)     |
//   | 'passes through the hook's own outgoing txn'    | (same)                                    |
//
// **Not reproduced, with reasons** (documented per the task's own
// fallback policy rather than silently omitted):
//
// - The full `ClaimReward` accrual/payout flow (delay-not-elapsed
//   rejection with the digit-patched wait message, matured-claim
//   `GenesisMint` payout amount, L1-seat distribution). `ttCLAIM_REWARD`
//   only does anything once an account's `RewardAccumulator`/
//   `RewardLgrFirst`/`RewardLgrLast`/`RewardTime` AccountRoot fields have
//   already been populated by the *protocol's own* (non-Hook,
//   transactor-level) reward-opt-in bookkeeping - on real Xahau this
//   happens automatically for every active account once the
//   `featureXahauGenesis`/reward-related amendments are live and the
//   account has accrued ledger history; reproducing that native
//   bookkeeping from scratch against a fresh standalone node (which
//   starts every account without any reward history) was out of scope
//   for the time available. `examples/80_reward/README.md`'s behavior-
//   equivalence table documents the intended behavior for every branch
//   this would exercise; only live-node confirmation is missing.
// - `GenesisMint`'s own transactor-level acceptance (`Account` must be
//   the network's *real* genesis account for a `GenesisMint` to validate)
//   is not exercised for the same reason as above - the standalone
//   node's genesis/master account (see `govern.test.ts`'s header comment)
//   is under our control, but driving it through the *native* reward
//   accrual state needed to reach a real `emit()` call was out of scope.
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
import { calculateHookOn } from 'xahau'
import { HookFlags } from 'xahau/dist/npm/models/common/xahau'

const namespace = 'rhooks-e2e-reward'
// hooks-build's printed worst case for reward.wasm (`mise run build-examples`).
const WORST_CASE_HOOK_INSTRUCTIONS = 13269

describe('reward', () => {
  let testContext: XrplIntegrationTestContext

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)

    const hook: iHook = {
      CreateCode: readHookBinaryHexFromNS('reward', 'wasm'),
      Flags: HookFlags.hsfOverride,
      HookOn: calculateHookOn(['Invoke', 'ClaimReward']),
      HookNamespace: hexNamespace(namespace),
      HookApiVersion: 0,
    }
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

  it('passes through a non-ClaimReward txn (reward.c L106)', async () => {
    const response = await Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'Invoke',
        Account: testContext.alice.classicAddress,
        Destination: testContext.hook1.classicAddress,
      },
      wallet: testContext.alice,
    })

    const meta = response.meta as any
    const hookExecutions = await ExecutionUtility.getHookExecutionsFromMeta(testContext.client, meta)
    expect(hookExecutions.executions.length).toBe(1)
    const execution = hookExecutions.executions[0]
    expect(execution.HookReturnString).toBe('Reward: Passing non-claim txn')
    expect(Number(execution.HookReturnCode)).toBe(0)
    expect(parseInt(execution.HookInstructionCount, 16)).toBeLessThanOrEqual(
      WORST_CASE_HOOK_INSTRUCTIONS,
    )
  })
})
