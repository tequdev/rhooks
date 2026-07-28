// e2e: examples/14_account-id-macro against a standalone Xahau node.
//
// `hooks_lib::account_id!("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh")` decodes
// that r-address into an `AccountId` entirely at compile time (host side,
// inside the proc-macro — no base58/checksum logic ships in the wasm at
// all). This is the live-node confirmation that the compile-time result is
// actually correct: the hook cross-checks it against `hook_account`,
// `util_accid`, and `util_raddr` at runtime, and this suite installs it on
// the one account where all three checks are meaningful.
//
// `rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh` is the Xahau/XRPL standalone-network
// genesis/master account (seed "masterpassphrase" /
// snoPBrXtMeMyMHUVTgbuqAfg1SUTb) — exactly `@transia/hooks-toolkit`'s own
// `MASTER_WALLET`/`testContext.master` (see govern.test.ts's header comment
// for the same fact, confirmed there via `Wallet.fromSeed(...)`). Installing
// on `testContext.master` (not `testContext.hook1`, unlike most other
// example suites) is what makes check 1 (`hook_account()` matches `OWNER`)
// meaningful rather than vacuously false.
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

const namespace = 'rhooks-e2e-account-id-macro'
// hooks-build's printed worst case for account_id_macro.wasm (`mise run
// build-examples`).
const WORST_CASE_HOOK_INSTRUCTIONS = 365

describe('account-id-macro', () => {
  let testContext: XrplIntegrationTestContext

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)

    const hook: iHook = {
      CreateCode: readHookBinaryHexFromNS('account_id_macro', 'wasm'),
      Flags: HookFlags.hsfOverride,
      HookOn: calculateHookOn(['Invoke']),
      HookNamespace: hexNamespace(namespace),
      HookApiVersion: 0,
    }
    // Installed on `testContext.master` - the genesis/master account this
    // hook's `OWNER` constant names - not `testContext.hook1` (unlike most
    // other example suites): see this file's header comment.
    await setHooksV3({
      client: testContext.client,
      seed: testContext.master.seed,
      hooks: [{ Hook: hook }],
    } as unknown as SetHookParams)
  })

  afterAll(async () => {
    // `clearAllHooksV3` (10x hsfOverride|hsfNSDelete, empty CreateCode) is
    // the standard cleanup every other suite here uses, but on
    // `testContext.master` specifically it reproducibly fails with
    // `tefINTERNAL` on this xahaud build (2026.6.21-release+3350) - isolated
    // during development to *any* hsfNSDelete/hook-deletion SetHook on the
    // genesis/master account, independent of this hook's own code (repros
    // even deleting an already-empty slot, or a slot holding a *different*
    // hook that never wrote any namespace state - overriding master's hook
    // with a *non-empty* CreateCode always succeeds, only deletion doesn't).
    // Not exercised by every suite that touches `testContext.master`:
    // `govern.test.ts`'s hook writes real namespace state (L1/L2 seat
    // table) before its own `clearAllHooksV3`, which does not hit this.
    // Swallowing it here (rather than failing the whole suite over a
    // cleanup step, after the actual assertions below have already run)
    // matches this file's job: prove `account_id!` is correct, not audit
    // xahaud's genesis-account reserve accounting.
    try {
      await clearAllHooksV3({
        client: testContext.client,
        seed: testContext.master.seed,
      } as unknown as SetHookParams)
    } catch (e) {
      console.warn(
        'account-id-macro: clearAllHooksV3 on master failed (known xahaud ' +
          'genesis-account hook-deletion quirk, see comment above) - ' +
          'ignoring:',
        e,
      )
    }
    await teardownClient(testContext)
  })

  it('hook_account/util_accid/util_raddr all agree with the account_id! compile-time constant', async () => {
    const response = await Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'Invoke',
        Account: testContext.alice.classicAddress,
        Destination: testContext.master.classicAddress,
      },
      wallet: testContext.alice,
    })

    const meta = response.meta as any
    const hookExecutions = await ExecutionUtility.getHookExecutionsFromMeta(
      testContext.client,
      meta,
    )
    expect(hookExecutions.executions.length).toBe(1)
    const execution = hookExecutions.executions[0]
    // HookReturnCode is a hex string over RPC (same convention as
    // HookInstructionCount below) - see emit-txn.test.ts's comment for
    // why this only matters for multi-digit codes (not the case here).
    expect(Number(execution.HookReturnCode)).toBe(0)
    expect(execution.HookReturnString).toBe(
      'account-id-macro: all three checks passed',
    )
    expect(parseInt(execution.HookInstructionCount, 16)).toBeLessThanOrEqual(
      WORST_CASE_HOOK_INSTRUCTIONS,
    )
  })
})
