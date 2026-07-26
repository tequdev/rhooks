// e2e: examples/05_firewall against a standalone Xahau node.
//
// `hook()` reads the otxn sender, reads a 20-byte blocked AccountId from the
// `BL` HookParameter (raw bytes, no extra encoding - `hook_param` copies the
// HookParameterValue bytes as-is), and rolls back with
// `rollback!(b"firewall: blocked account", FirewallError::BlockedAccount)`
// (code 2, from the `hook_errors!`-defined `FirewallError` enum) on a
// match; accepts otherwise. HookOn is Payment.
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

const namespace = 'rhooks-e2e-firewall'
// hooks-build's printed worst case for firewall.wasm (`mise run build-examples`).
const WORST_CASE_INSTRUCTIONS = 134

function accountIdHex(classicAddress: string): string {
  return Buffer.from(decodeAccountID(classicAddress)).toString('hex').toUpperCase()
}

describe('firewall', () => {
  let testContext: XrplIntegrationTestContext

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)

    const hook = {
      CreateCode: readHookBinaryHexFromNS('firewall', 'wasm'),
      Flags: HookFlags.hsfOverride,
      HookOn: calculateHookOn(['Payment']),
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

  it('rejects a Payment from the blocked account', async () => {
    const response = Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'Payment',
        Account: testContext.bob.classicAddress,
        Destination: testContext.hook1.classicAddress,
        Amount: '1',
      },
      wallet: testContext.bob,
    })
    await expect(response).rejects.toThrow('firewall: blocked account')
  })

  it('accepts a Payment from a non-blocked account', async () => {
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
