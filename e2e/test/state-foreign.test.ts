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
    await expect(invoke(testContext)).rejects.toThrow(
      'state-foreign: state_foreign read failed',
    )
  })
})
