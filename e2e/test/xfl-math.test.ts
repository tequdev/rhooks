// e2e: examples/07_xfl-math against a standalone Xahau node.
//
// `hook()` reads the originating Payment's `Amount` as an XFL (via
// `otxn_slot`/`slot_subfield`/`XFL::from_slot` - native amounts decode as
// their XAH value, not raw drops), computes 1% of it with `mulratio`, and
// rolls back with `XflMathError::BelowMinimum` (code 7) if that share is
// below the fixed `0.000001` (1e-6) XAH minimum - i.e. if the Amount is
// below 100 drops (1% of 100 drops = 1 drop = 0.000001 XAH exactly);
// accepts otherwise. HookOn is Payment.
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
import { calculateHookOn, type TransactionMetadata } from 'xahau'
// HookFlags isn't re-exported from the package root in xahau@4.x - only
// reachable via this deep import (same path hooks-toolkit's own source
// uses internally for the same enum).
import { HookFlags } from 'xahau/dist/npm/models/common/xahau'

const namespace = 'rhooks-e2e-xfl-math'
// hooks-build's printed worst case for xfl_math.wasm (`mise run build-examples`).
const WORST_CASE_INSTRUCTIONS = 155

describe('xfl-math', () => {
  let testContext: XrplIntegrationTestContext

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)

    const hook: iHook = {
      CreateCode: readHookBinaryHexFromNS('xfl_math', 'wasm'),
      Flags: HookFlags.hsfOverride,
      HookOn: calculateHookOn(['Payment']),
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

  it('rejects a Payment whose computed 1% share falls below the minimum', async () => {
    const response = Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'Payment',
        Account: testContext.alice.classicAddress,
        Destination: testContext.hook1.classicAddress,
        Amount: '50', // 1% of 50 drops < 0.000001 XAH
      },
      wallet: testContext.alice,
    })
    await expect(response).rejects.toThrow('xfl-math: computed share below minimum')
  })

  it('accepts a Payment whose computed 1% share meets the minimum', async () => {
    const response = await Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'Payment',
        Account: testContext.alice.classicAddress,
        Destination: testContext.hook1.classicAddress,
        Amount: '1000000', // 1 XAH; 1% share is 0.01 XAH, well above the minimum
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
    // HookReturnCode is a 64-bit int field, serialized as a *hex* string
    // over RPC (same convention as HookInstructionCount below) even
    // though the toolkit's `iHookExecution` type declares it as `number`
    // - only matters here because the asserted codes are single-digit,
    // which read identically whether parsed as decimal or hex (see
    // slot-ledger.test.ts for a case where it doesn't).
    expect(Number(execution.HookReturnCode)).toBe(0)
    expect(execution.HookReturnString).toBe('')
    // HookInstructionCount is a *hex* string over RPC (confirmed by direct
    // inspection - e.g. "d" = 13).
    expect(parseInt(execution.HookInstructionCount, 16)).toBeLessThanOrEqual(
      WORST_CASE_INSTRUCTIONS,
    )
  })
})
