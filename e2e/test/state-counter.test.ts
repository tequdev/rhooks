// e2e: examples/state-counter against a standalone Xahau node.
//
// `hook()` reads an 8-byte LE u64 counter from state key
// `pad!(b"counter")` (crates/hooks-lib STATE_KEY_LEN = 32: the 7 ASCII
// bytes of "counter" left-aligned, zero-padded to 32 bytes - see
// `padded_bytes` in crates/hooks-lib/src/macros.rs), defaulting to 0,
// increments it, writes it back, and calls
// `accept!(b"state-counter: incremented", next as i64)` - the new count
// becomes the HookReturnCode. HookOn is Invoke.
import {
  ExecutionUtility,
  StateUtility,
  Xrpld,
  clearAllHooksV3,
  clearHookStateV3,
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

const namespace = 'rhooks-e2e-state-counter'
// hooks-build's printed worst case for state_counter.wasm (`mise run build-examples`).
const WORST_CASE_INSTRUCTIONS = 58

// STATE_KEY = pad!(b"counter"): "counter" (7 bytes) left-aligned in a
// 32-byte array, zero-padded on the right.
const COUNTER_KEY =
  Buffer.from('counter', 'ascii').toString('hex').toUpperCase().padEnd(64, '0')

describe('state-counter', () => {
  let testContext: XrplIntegrationTestContext
  const hookNamespace = hexNamespace(namespace)

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)

    const hook: iHook = {
      CreateCode: readHookBinaryHexFromNS('state_counter', 'wasm'),
      Flags: HookFlags.hsfOverride,
      HookOn: calculateHookOn(['Invoke']),
      HookNamespace: hookNamespace,
      HookApiVersion: 0,
    }
    await setHooksV3({
      client: testContext.client,
      seed: testContext.hook1.seed,
      hooks: [{ Hook: hook }],
    } as unknown as SetHookParams)
  })

  afterAll(async () => {
    // hsfNSDelete: clear the namespace's state before removing the hook,
    // so a re-run of this suite starts from a fresh counter.
    const clearStateHook: iHook = {
      Flags: HookFlags.hsfNSDelete,
      HookNamespace: hookNamespace,
    }
    await clearHookStateV3({
      client: testContext.client,
      seed: testContext.hook1.seed,
      hooks: [{ Hook: clearStateHook }],
    } as unknown as SetHookParams)
    await clearAllHooksV3({
      client: testContext.client,
      seed: testContext.hook1.seed,
    } as unknown as SetHookParams)
    await teardownClient(testContext)
  })

  const invoke = async () => {
    const response = await Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'Invoke',
        Account: testContext.alice.classicAddress,
        Destination: testContext.hook1.classicAddress,
      },
      wallet: testContext.alice,
    })
    const meta = response.meta as TransactionMetadata
    return ExecutionUtility.getHookExecutionsFromMeta(testContext.client, meta)
  }

  it('increments the counter on the first Invoke (count = 1)', async () => {
    const hookExecutions = await invoke()
    expect(hookExecutions.executions.length).toBe(1)
    const execution = hookExecutions.executions[0]
    // HookReturnCode is a 64-bit int field, serialized as a decimal string
    // over RPC (like other int64/uint64 ledger fields) even though the
    // toolkit's `iHookExecution` type declares it as `number`.
    expect(Number(execution.HookReturnCode)).toBe(1)
    expect(execution.HookReturnString).toBe('state-counter: incremented')
    // HookInstructionCount is a *hex* string over RPC (confirmed by direct
    // inspection - e.g. "d" = 13), unlike HookReturnCode's decimal string.
    expect(parseInt(execution.HookInstructionCount, 16)).toBeLessThanOrEqual(
      WORST_CASE_INSTRUCTIONS,
    )
  })

  it('increments the counter on the second Invoke (count = 2)', async () => {
    const hookExecutions = await invoke()
    expect(hookExecutions.executions.length).toBe(1)
    expect(Number(hookExecutions.executions[0].HookReturnCode)).toBe(2)
  })

  it('persists the counter as an 8-byte LE u64 in hook state', async () => {
    const entry = await StateUtility.getHookState(
      testContext.client,
      testContext.hook1.classicAddress,
      COUNTER_KEY,
      hookNamespace,
    )
    const raw = Buffer.from(entry.HookStateData, 'hex')
    expect(raw.length).toBe(8)
    expect(raw.readBigUInt64LE(0)).toBe(2n)
  })
})
