// e2e: examples/09_state-foreign against a standalone Xahau node.
//
// `hook()` reads a target `AccountId` from the `ACCT` HookParameter, then
// reads that target's `enabled` state entry in *this hook's own*
// namespace via `state_foreign`, rolling back with a `StateForeignError`
// code (see examples/09_state-foreign/README.md) if `ACCT` isn't
// configured or the target has no such entry; accepts only if the entry
// exists with a nonzero first byte. HookOn is Invoke (this hook never
// reads any otxn field).
//
// This suite covers the `ACCT` unconfigured path, and a live-observed
// `state_foreign` failure path: a target account that has *never* had any
// Hook state of any kind (no namespace directory to look up) fails with
// something other than `DOESNT_EXIST` (confirmed against a live node -
// this hook's own `Err(_) => ReadFailed` catch-all is exactly what that
// case exercises, not `NotConfiguredOnTarget`). The "entry present"
// paths (`NotConfiguredOnTarget`/`FlagOff`/accept) would need a *second*
// hook deployed on the target account purely to seed its raw `enabled`
// state (no existing example hook writes that key), which is out of
// scope for this suite.
import {
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
import { calculateHookOn, convertStringToHex, decodeAccountID } from 'xahau'
// HookFlags isn't re-exported from the package root in xahau@4.x - only
// reachable via this deep import (same path hooks-toolkit's own source
// uses internally for the same enum).
import { HookFlags } from 'xahau/dist/npm/models/common/xahau'

const namespace = 'rhooks-e2e-state-foreign'

function accountIdHex(classicAddress: string): string {
  return Buffer.from(decodeAccountID(classicAddress)).toString('hex').toUpperCase()
}

async function invoke(testContext: XrplIntegrationTestContext) {
  return Xrpld.submit(testContext.client, {
    tx: {
      TransactionType: 'Invoke',
      Account: testContext.alice.classicAddress,
      Destination: testContext.hook1.classicAddress,
    },
    wallet: testContext.alice,
  })
}

describe('state-foreign: ACCT not configured', () => {
  let testContext: XrplIntegrationTestContext

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)

    const hook: iHook = {
      CreateCode: readHookBinaryHexFromNS('state_foreign', 'wasm'),
      Flags: HookFlags.hsfOverride,
      HookOn: calculateHookOn(['Invoke']),
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

  it('rejects when ACCT is not configured', async () => {
    await expect(invoke(testContext)).rejects.toThrow(
      'state-foreign: ACCT parameter not configured',
    )
  })
})

describe('state-foreign: ACCT configured, target has no Hook state at all', () => {
  let testContext: XrplIntegrationTestContext

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)

    const hook: iHook = {
      CreateCode: readHookBinaryHexFromNS('state_foreign', 'wasm'),
      Flags: HookFlags.hsfOverride,
      HookOn: calculateHookOn(['Invoke']),
      HookNamespace: hexNamespace(namespace),
      HookApiVersion: 0,
      HookParameters: [
        {
          HookParameter: {
            HookParameterName: convertStringToHex('ACCT'),
            // bob has never had a hook installed, so it has no Hook
            // state namespace directory at all - see the header comment.
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

  it('rejects with the ReadFailed catch-all, not NotConfiguredOnTarget', async () => {
    // Live-observed: `state_foreign` against an account with no Hook
    // state of any kind fails with something other than
    // `HookError::DoesntExist`, so this hits `Err(_) =>
    // StateForeignError::ReadFailed` rather than the `DoesntExist` arm.
    await expect(invoke(testContext)).rejects.toThrow(
      'state-foreign: state_foreign read failed',
    )
  })
})
