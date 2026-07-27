// e2e: examples/03_hook-params against a standalone Xahau node.
//
// `hook()` reads the originating Payment's native (XRP/XAH) `Amount`,
// compares it against a minimum threshold read from the `MIN`
// HookParameter (8 raw bytes, a big-endian u64 drops value - see
// `min_drops`/`hook_param_exact` in examples/03_hook-params/src/lib.rs),
// and rolls back with `HookParamsError::BelowMinimum` (code 2) if the
// Amount falls short; accepts otherwise. HookOn is Payment.
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
import { calculateHookOn, convertStringToHex, type TransactionMetadata } from 'xahau'
// HookFlags isn't re-exported from the package root in xahau@4.x - only
// reachable via this deep import (same path hooks-toolkit's own source
// uses internally for the same enum).
import { HookFlags } from 'xahau/dist/npm/models/common/xahau'

const namespace = 'rhooks-e2e-hook-params'
// hooks-build's printed worst case for hook_params.wasm (`mise run build-examples`).
const WORST_CASE_INSTRUCTIONS = 165
// MIN HookParameter: 5,000,000 drops (5 XAH), matching the worked example
// in examples/03_hook-params/README.md's hex-encoding section.
const MIN_DROPS = 5_000_000n

function u64BEHex(value: bigint): string {
  const buf = Buffer.alloc(8)
  buf.writeBigUInt64BE(value)
  return buf.toString('hex').toUpperCase()
}

describe('hook-params', () => {
  let testContext: XrplIntegrationTestContext

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)

    const hook: iHook = {
      CreateCode: readHookBinaryHexFromNS('hook_params', 'wasm'),
      Flags: HookFlags.hsfOverride,
      HookOn: calculateHookOn(['Payment']),
      HookNamespace: hexNamespace(namespace),
      HookApiVersion: 0,
      HookParameters: [
        {
          HookParameter: {
            HookParameterName: convertStringToHex('MIN'),
            HookParameterValue: u64BEHex(MIN_DROPS),
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

  it('rejects a Payment below the configured MIN threshold', async () => {
    const response = Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'Payment',
        Account: testContext.alice.classicAddress,
        Destination: testContext.hook1.classicAddress,
        Amount: '1000000', // 1 XAH, below the configured 5 XAH minimum
      },
      wallet: testContext.alice,
    })
    await expect(response).rejects.toThrow(
      'hook-params: amount below configured minimum',
    )
  })

  it('accepts a Payment at or above the configured MIN threshold', async () => {
    const response = await Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'Payment',
        Account: testContext.alice.classicAddress,
        Destination: testContext.hook1.classicAddress,
        Amount: '6000000', // 6 XAH, above the configured 5 XAH minimum
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
