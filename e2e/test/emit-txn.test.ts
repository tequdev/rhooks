// e2e: examples/10_emit-txn against a standalone Xahau node.
//
// `hook()` reserves one emission slot, builds its `txn_template!`-declared
// Payment (1 drop back to the otxn sender) fully in hook memory, and calls
// `emit()` - this is the FIRST live validation of hooks-lib's
// `txn_template!` wire format against a real xahaud: if the node rejects
// the emitted txn, or SetHook/emit itself fails, that is reported
// prominently below rather than patched around.
//
// Emitted transactions carry `FirstLedgerSequence = triggering seq + 1`,
// so they cannot land in the same ledger as the triggering Invoke - at
// least one extra `ledger_accept` is needed after the one that closes the
// Invoke's own ledger.
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
  type HookEmission,
  type TransactionMetadata,
} from 'xahau'
// HookFlags isn't re-exported from the package root in xahau@4.x - only
// reachable via this deep import (same path hooks-toolkit's own source
// uses internally for the same enum).
import { HookFlags } from 'xahau/dist/npm/models/common/xahau'

const namespace = 'rhooks-e2e-emit-txn'
// hooks-build's printed worst case for emit_txn.wasm (`mise run build-examples`),
// derived straight from xahaud's own vendored guard checker (see
// crates/hooks-build/src/guard_native.rs) - not a hooks-build estimate.
const WORST_CASE_HOOK_INSTRUCTIONS = 322
// NOT used as a hard bound below: live-observed cbak HookInstructionCount
// for this trivial `accept!()`-only cbak is 10, exceeding the checker's
// own cbak_cost of 7. Confirmed reproducible (crates/hooks-build/src/
// guard_native.rs's cbak_cost comes from xahaud's *own* vendored
// `validateGuards()`, unmodified - this is a static-vs-live accounting
// gap in xahaud's own upstream checker, not something to patch here).
// See this suite's task report for the full writeup.

describe('emit-txn', () => {
  let testContext: XrplIntegrationTestContext

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)

    const hook: iHook = {
      CreateCode: readHookBinaryHexFromNS('emit_txn', 'wasm'),
      Flags: HookFlags.hsfOverride,
      HookOn: calculateHookOn(['Invoke']),
      HookNamespace: hexNamespace(namespace),
      HookApiVersion: 0,
    }
    // setupClient's fundSystem funds every hookN wallet to 20000 XAH if
    // below 10000 XAH - comfortably above the 1-drop emission + its fee
    // and the emitted-transaction owner reserve this hook needs.
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

  it('emits a 1-drop Payment back to the otxn sender, which settles tesSUCCESS with a cbak execution', async () => {
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
    // HookReturnCode is a 64-bit int field, serialized as a *hex* string
    // over RPC (same convention as HookInstructionCount below) even
    // though the toolkit's `iHookExecution` type declares it as `number`
    // - only matters here because the asserted codes are single-digit,
    // which read identically whether parsed as decimal or hex (see
    // slot-ledger.test.ts for a case where it doesn't).
    expect(Number(execution.HookReturnCode)).toBe(0)
    expect(execution.HookReturnString).toBe('emit-txn: emitted')
    // HookInstructionCount is a *hex* string over RPC (confirmed by direct
    // inspection - e.g. "d" = 13).
    expect(parseInt(execution.HookInstructionCount, 16)).toBeLessThanOrEqual(
      WORST_CASE_HOOK_INSTRUCTIONS,
    )

    expect(meta.HookEmissions).toBeDefined()
    const emissions = meta.HookEmissions as HookEmission[]
    expect(emissions.length).toBe(1)
    const emittedTxnID = emissions[0].HookEmission.EmittedTxnID

    // Poll for the emitted txn to validate: it needs at least one more
    // closed ledger beyond the one containing the triggering Invoke.
    // The client defaults to API v1 (see xahau's Client.apiVersion), whose
    // `tx` response flattens the transaction's fields onto `.result`
    // directly (no `.result.tx_json` nesting).
    let emittedTxn: any
    for (let attempt = 0; attempt < 4 && !emittedTxn; attempt += 1) {
      await testContext.client.request({ command: 'ledger_accept' } as any)
      try {
        const txResponse = await testContext.client.request({
          command: 'tx',
          transaction: emittedTxnID,
        } as any)
        if ((txResponse as any).result.validated) {
          emittedTxn = txResponse
        }
      } catch {
        // Not yet included - keep polling.
      }
    }

    if (!emittedTxn) {
      let debugLog = '(docker logs unavailable)'
      try {
        const { execSync } = await import('node:child_process')
        debugLog = execSync('docker logs --tail 200 xahau', {
          encoding: 'utf8',
        })
      } catch (logError) {
        debugLog = `(failed to capture docker logs: ${String(logError)})`
      }
      throw new Error(
        `emitted txn ${emittedTxnID} never validated after 4 ledger_accept calls.\n` +
          `Triggering tx meta: ${JSON.stringify(meta)}\n` +
          `docker logs (tail 200): ${debugLog}`,
      )
    }

    const emittedMeta = emittedTxn.result.meta as TransactionMetadata
    expect(emittedMeta.TransactionResult).toBe('tesSUCCESS')
    expect(emittedTxn.result.Account).toBe(testContext.hook1.classicAddress)
    expect(emittedTxn.result.Destination).toBe(testContext.alice.classicAddress)
    expect(emittedTxn.result.Amount).toBe('1')

    const cbakExecutions = await ExecutionUtility.getHookExecutionsFromMeta(
      testContext.client,
      emittedMeta,
    )
    expect(cbakExecutions.executions.length).toBe(1)
    const cbakExecution = cbakExecutions.executions[0]
    expect(cbakExecution.HookAccount).toBe(testContext.hook1.classicAddress)
    expect(Number(cbakExecution.HookReturnCode)).toBe(0)
    expect(cbakExecution.HookReturnString).toBe('')
    // Sanity bound only (see the comment above) - deliberately not
    // hooks-build's printed cbak worst case, which live evidence shows
    // this trivial cbak already exceeds.
    expect(parseInt(cbakExecution.HookInstructionCount, 16)).toBeLessThanOrEqual(20)
  })
})
