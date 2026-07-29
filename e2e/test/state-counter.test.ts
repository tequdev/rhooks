// e2e: examples/02_state-counter against a standalone Xahau node.
//
// `hook()` reads a `u64` counter through hooks-lib's typed storage layer
// (`state_get_typed`/`state_set_typed`), keyed by `CounterKey { name: [u8;
// 7] }` (a `#[derive(HookKey)]` struct wrapping the literal `*b"counter"`)
// paired with `u64` via `hook_state!`. `HookKey` sends a struct at its own
// real encoded length - 7 bytes here, no local padding - so this lands on
// the exact same on-ledger slot a bare `*b"counter"` array key would (see
// `hooks_lib::state`'s module doc comment, "Key length and padding", and
// `examples/02_state-counter/README.md`'s "Same slot as before" section).
// The host itself left-pads that 7-byte key to its fixed 32-byte storage
// width, so the real on-ledger HookState key is "counter"'s 7 ASCII bytes
// right-aligned in 32 bytes, zero-padded on the *left*. Defaults to 0,
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
// hooks-build's printed worst case for state_counter.wasm (`mise run
// build-examples`) - 254, up from 58 for the previous, hand-rolled-buffer
// version of this hook (see the README's "Cost of the typed layer, here"
// section): `state_get_typed`/`state_set_typed` go through `crate::state`'s
// generic 32-byte-scratch-buffer machinery instead of a plain 8-byte
// buffer over the raw `state`/`state_set` calls.
const WORST_CASE_INSTRUCTIONS = 254

// CounterKey { name: *b"counter" } (7 bytes), sent to the host at its own
// real length (HookKey's derive; see the README's "Same slot as before"
// section); the host left-pads a short key to its fixed 32-byte storage
// width - so the real on-ledger key is "counter" right-aligned in 32
// bytes, zero-padded on the left.
const COUNTER_KEY = Buffer.from('counter', 'ascii')
  .toString('hex')
  .toUpperCase()
  .padStart(64, '0')

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
    // HookReturnCode is a 64-bit int field, serialized as a *hex* string
    // over RPC (same convention as HookInstructionCount below) even
    // though the toolkit's `iHookExecution` type declares it as `number`
    // - only matters here because the asserted codes are single-digit,
    // which read identically whether parsed as decimal or hex (see
    // slot-ledger.test.ts for a case where it doesn't).
    expect(Number(execution.HookReturnCode)).toBe(1)
    expect(execution.HookReturnString).toBe('state-counter: incremented')
    // HookInstructionCount is a *hex* string over RPC (confirmed by direct
    // inspection - e.g. "d" = 13).
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
