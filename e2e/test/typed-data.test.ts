// e2e: examples/12_typed-data against a standalone Xahau node.
//
// `hook()` maintains a per-account deposit ledger keyed by a
// `#[derive(HookKey)]` struct and valued by a `#[derive(HookData)]` struct
// (`DepositKey`/`DepositValue` - see examples/12_typed-data/src/lib.rs and
// its README). Every Invoke carries its own `Instruction` (action +
// amount, `#[derive(ParamValue)]`) as an `INS` Hook parameter attached to
// the transaction itself (read via `otxn_param`), separate from the
// hook's installed `Config` (`CFG`, read via `hook_param`). An operator
// can also pause new deposits via a **composite**, struct-shaped
// `#[derive(ParamName)]` parameter name (`AdminName`) - see the last
// nested `describe` block below.
//
// Every `#[derive(HookData)]`/`#[derive(ParamValue)]` struct's wire layout
// is "every field, in declaration order, little-endian, back-to-back" -
// see the README's "Hook parameter hex encoding" section for the worked
// byte layouts this suite's `cfgHex`/`insHex` helpers reproduce.
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

const namespace = 'rhooks-e2e-typed-data'
// hooks-build's printed worst case for typed_data.wasm (`mise run build-examples`),
// at this workspace's opt-level = 3 default (docs/DESIGN.md's §2 C6) - includes
// the composite `AdminName`/`PauseSwitch` pause-switch path (see the README's
// "Measured cost of a composite name" section for the with/without comparison).
const WORST_CASE_INSTRUCTIONS = 489

const ACTION_DEPOSIT = 1
const ACTION_WITHDRAW = 2

// `Config { min_amount: u64, lock_ledgers: u32 }` - 12 bytes, LE.
const MIN_DROPS = 5_000_000n // 5 XAH
// Deliberately large: the standalone node auto-advances the ledger by more
// than one between successive submitted transactions (observed: ~2 per
// `Xrpld.submit` round trip), so a small `lock_ledgers` would already have
// elapsed by the very next Invoke. Comfortably larger than that keeps
// "rejects a withdraw before the lock window elapses" (below) genuinely
// still-locked, and "accepts a withdraw once the lock window has elapsed"
// explicitly fast-forwards past it with repeated `ledger_accept` calls.
const LOCK_LEDGERS = 30

function u64LEHex(value: bigint): string {
  const buf = Buffer.alloc(8)
  buf.writeBigUInt64LE(value)
  return buf.toString('hex').toUpperCase()
}

function u32LEHex(value: number): string {
  const buf = Buffer.alloc(4)
  buf.writeUInt32LE(value)
  return buf.toString('hex').toUpperCase()
}

function cfgHex(minAmount: bigint, lockLedgers: number): string {
  return u64LEHex(minAmount) + u32LEHex(lockLedgers)
}

// `Instruction { action: u8, amount: u64 }` - 9 bytes, LE.
function insHex(action: number, amount: bigint): string {
  const actionByte = Buffer.from([action]).toString('hex').toUpperCase()
  return actionByte + u64LEHex(amount)
}

function hookParam(name: string, valueHex: string) {
  return {
    HookParameter: {
      HookParameterName: convertStringToHex(name),
      HookParameterValue: valueHex,
    },
  }
}

async function invoke(
  testContext: XrplIntegrationTestContext,
  sender: XrplIntegrationTestContext['alice'],
  insValueHex: string,
) {
  return Xrpld.submit(testContext.client, {
    tx: {
      TransactionType: 'Invoke',
      Account: sender.classicAddress,
      Destination: testContext.hook1.classicAddress,
      HookParameters: [hookParam('INS', insValueHex)],
    } as any,
    wallet: sender,
  })
}

describe('typed-data', () => {
  let testContext: XrplIntegrationTestContext

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)

    const hook: iHook = {
      CreateCode: readHookBinaryHexFromNS('typed_data', 'wasm'),
      Flags: HookFlags.hsfOverride,
      HookOn: calculateHookOn(['Invoke']),
      HookNamespace: hexNamespace(namespace),
      HookApiVersion: 0,
      HookParameters: [hookParam('CFG', cfgHex(MIN_DROPS, LOCK_LEDGERS))],
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

  it('rejects an Invoke with no INS parameter', async () => {
    const response = Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'Invoke',
        Account: testContext.alice.classicAddress,
        Destination: testContext.hook1.classicAddress,
      },
      wallet: testContext.alice,
    })
    await expect(response).rejects.toThrow(
      'typed-data: INS parameter missing or malformed',
    )
  })

  it('rejects a withdraw when the account has no outstanding deposit', async () => {
    // bob has never deposited, so `DepositKey { tag: 1, owner: bob }` has no
    // state entry - `state_get` returns `None`, decoded as `EMPTY_DEPOSIT`
    // (`flags == 0`).
    const response = invoke(
      testContext,
      testContext.bob,
      insHex(ACTION_WITHDRAW, 0n),
    )
    await expect(response).rejects.toThrow('typed-data: nothing to withdraw')
  })

  it('rejects a deposit below the configured minimum', async () => {
    const response = invoke(
      testContext,
      testContext.alice,
      insHex(ACTION_DEPOSIT, MIN_DROPS - 1n),
    )
    await expect(response).rejects.toThrow(
      'typed-data: deposit below configured minimum',
    )
  })

  it('accepts a deposit at the configured minimum', async () => {
    const response = await invoke(
      testContext,
      testContext.alice,
      insHex(ACTION_DEPOSIT, MIN_DROPS),
    )

    const meta = response.meta as TransactionMetadata
    const hookExecutions = await ExecutionUtility.getHookExecutionsFromMeta(
      testContext.client,
      meta,
    )
    expect(hookExecutions.executions.length).toBe(1)
    const execution = hookExecutions.executions[0]
    // `accept!(b"typed-data: ok", next.amount as i64)` - the resulting
    // balance (here, exactly `MIN_DROPS`, alice's first deposit) becomes
    // the `HookReturnCode`, a hex string over RPC (see hook-params.test.ts
    // for the same convention).
    expect(BigInt(`0x${execution.HookReturnCode}`)).toBe(MIN_DROPS)
    expect(execution.HookReturnString).toBe('typed-data: ok')
    expect(parseInt(execution.HookInstructionCount, 16)).toBeLessThanOrEqual(
      WORST_CASE_INSTRUCTIONS,
    )
  })

  it('rejects a withdraw before the lock window elapses', async () => {
    const response = invoke(
      testContext,
      testContext.alice,
      insHex(ACTION_WITHDRAW, 0n),
    )
    await expect(response).rejects.toThrow('typed-data: deposit still locked')
  })

  it('accepts a withdraw once the lock window has elapsed', async () => {
    // `LOCK_LEDGERS = 30`: force-advance the ledger well past the
    // deposit's `deadline = deposit_ledger_seq + 30` via the standalone
    // node's `ledger_accept` admin RPC - the same mechanism
    // emit-txn.test.ts uses to satisfy an emitted transaction's
    // `FirstLedgerSequence`.
    for (let i = 0; i < 35; i += 1) {
      await testContext.client.request({ command: 'ledger_accept' } as any)
    }

    const response = await invoke(
      testContext,
      testContext.alice,
      insHex(ACTION_WITHDRAW, 0n),
    )

    const meta = response.meta as TransactionMetadata
    const hookExecutions = await ExecutionUtility.getHookExecutionsFromMeta(
      testContext.client,
      meta,
    )
    expect(hookExecutions.executions.length).toBe(1)
    const execution = hookExecutions.executions[0]
    // A withdrawal always zeroes the balance out.
    expect(Number(execution.HookReturnCode)).toBe(0)
    expect(execution.HookReturnString).toBe('typed-data: ok')
    expect(parseInt(execution.HookInstructionCount, 16)).toBeLessThanOrEqual(
      WORST_CASE_INSTRUCTIONS,
    )
  })

  it('rejects a second withdraw now that the deposit is gone', async () => {
    const response = invoke(
      testContext,
      testContext.alice,
      insHex(ACTION_WITHDRAW, 0n),
    )
    await expect(response).rejects.toThrow('typed-data: nothing to withdraw')
  })

  it('rejects an unknown INS action', async () => {
    const response = invoke(
      testContext,
      testContext.bob,
      insHex(99, 0n),
    )
    await expect(response).rejects.toThrow('typed-data: unknown INS action')
  })

  describe('deposit pause switch (composite AdminName parameter)', () => {
    // `AdminName { section: 0, field: 0 }` - 2 bytes, back-to-back, no
    // padding (README's "Hex encoding" section under "Composite
    // (struct-shaped) parameter names").
    const ADMIN_NAME_HEX = '0000'

    beforeAll(async () => {
      // Re-install the same hook with an additional composite `AdminName`
      // Hook parameter, `PauseSwitch { paused: 1 }` (hex `01`) - pauses new
      // deposits without touching any existing `DepositValue` state (the
      // HookNamespace, and so every account's state entry, is unchanged by
      // a SetHook that only replaces the hook definition/parameters).
      const hook: iHook = {
        CreateCode: readHookBinaryHexFromNS('typed_data', 'wasm'),
        Flags: HookFlags.hsfOverride,
        HookOn: calculateHookOn(['Invoke']),
        HookNamespace: hexNamespace(namespace),
        HookApiVersion: 0,
        HookParameters: [
          hookParam('CFG', cfgHex(MIN_DROPS, LOCK_LEDGERS)),
          {
            HookParameter: {
              HookParameterName: ADMIN_NAME_HEX,
              HookParameterValue: '01',
            },
          },
        ],
      }
      await setHooksV3({
        client: testContext.client,
        seed: testContext.hook1.seed,
        hooks: [{ Hook: hook }],
      } as unknown as SetHookParams)
    })

    afterAll(async () => {
      // Restore the unpaused hook (no `AdminName` parameter at all - absent
      // is treated the same as `paused: 0`, per `deposits_paused`'s doc
      // comment) so nothing after this block observes deposits paused.
      const hook: iHook = {
        CreateCode: readHookBinaryHexFromNS('typed_data', 'wasm'),
        Flags: HookFlags.hsfOverride,
        HookOn: calculateHookOn(['Invoke']),
        HookNamespace: hexNamespace(namespace),
        HookApiVersion: 0,
        HookParameters: [hookParam('CFG', cfgHex(MIN_DROPS, LOCK_LEDGERS))],
      }
      await setHooksV3({
        client: testContext.client,
        seed: testContext.hook1.seed,
        hooks: [{ Hook: hook }],
      } as unknown as SetHookParams)
    })

    it('rejects a deposit while the AdminName pause switch is set', async () => {
      // bob has no outstanding deposit at this point in the suite (his two
      // earlier invokes both rolled back, so no state was ever written) -
      // a deposit here exercises the pause check regardless.
      const response = invoke(
        testContext,
        testContext.bob,
        insHex(ACTION_DEPOSIT, MIN_DROPS),
      )
      await expect(response).rejects.toThrow(
        'typed-data: deposits are currently paused',
      )
    })

    it('still allows a withdraw while paused (withdrawals are never paused)', async () => {
      // bob still has no outstanding deposit, so this hits the *other*
      // rollback path (`NothingToWithdraw`) rather than succeeding - but
      // reaching that check at all (instead of `DepositsPaused`) proves
      // `deposits_paused()` is only ever consulted on the deposit branch,
      // exactly as documented.
      const response = invoke(
        testContext,
        testContext.bob,
        insHex(ACTION_WITHDRAW, 0n),
      )
      await expect(response).rejects.toThrow('typed-data: nothing to withdraw')
    })
  })
})
