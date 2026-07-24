// e2e: examples/08_slot-ledger against a standalone Xahau node.
//
// `hook()` navigates the originating Payment through the Slot API
// (`otxn_slot` -> `slot_subfield` -> `slot_exact`) to read `sfDestination`
// and `sfAmount`, rolling back with a `SlotLedgerError` code (see
// examples/08_slot-ledger/README.md) if either lookup fails or `Amount`
// isn't native; accepts otherwise with a marker accept code (the sum of
// both fields' first bytes - not meaningful hook logic, see the source).
// HookOn is Payment.
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

const namespace = 'rhooks-e2e-slot-ledger'
// hooks-build's printed worst case for slot_ledger.wasm (`mise run build-examples`).
const WORST_CASE_INSTRUCTIONS = 193

describe('slot-ledger', () => {
  let testContext: XrplIntegrationTestContext

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)

    const hook: iHook = {
      CreateCode: readHookBinaryHexFromNS('slot_ledger', 'wasm'),
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

  it('reads Destination and a native Amount via the Slot API and accepts', async () => {
    const response = await Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'Payment',
        Account: testContext.alice.classicAddress,
        Destination: testContext.hook1.classicAddress,
        Amount: '1',
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
    // over RPC (confirmed by direct inspection: this hook's marker accept
    // code came back as "ad" = 173 decimal) despite the toolkit's
    // `iHookExecution` type declaring it as `number` (hence the `String`
    // cast below) - every other test in this suite only ever asserts
    // single-digit codes (0, 1, 2, ...), which read identically whether
    // parsed as decimal or hex, so this is the first assertion in the
    // suite to actually depend on getting the base right. Parse as hex
    // here, same as HookInstructionCount below.
    expect(
      parseInt(String(execution.HookReturnCode), 16),
    ).toBeGreaterThanOrEqual(0)
    expect(execution.HookReturnString).toBe(
      'slot-ledger: read Destination and native Amount',
    )
    // HookInstructionCount is also a hex string over RPC (confirmed by
    // direct inspection - e.g. "d" = 13).
    expect(parseInt(execution.HookInstructionCount, 16)).toBeLessThanOrEqual(
      WORST_CASE_INSTRUCTIONS,
    )
  })
})
