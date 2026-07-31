// e2e: examples/15_slot-objects against a standalone Xahau node.
//
// This is the typed slot layer's live acceptance harness. Every assertion
// here exists because a host build *cannot* make it: `hooks-core`'s stubs
// return NOT_IMPLEMENTED for every slot call, so the Rust-side integration
// tests can prove typing and reachability but nothing about real slot
// behavior.
//
// The hook runs five checks and reports which passed as a bitmask in its
// accept code, so a partial failure names the broken invariant instead of
// just failing. See examples/15_slot-objects/README.md for the table.
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

const namespace = 'rhooks-e2e-slot-objects'
// hooks-build's printed worst case for slot_objects.wasm (`mise run
// build-examples`). Large by design: the recycling loops each run 260
// iterations of real host calls, and the guard checker sums every loop in
// the module regardless of which one a given execution takes.
const WORST_CASE_INSTRUCTIONS = 61658

// One bit per check, matching the `BIT_*` constants in the hook's source.
const BIT_ACCOUNT_WALK = 1
const BIT_DROPS_ROUNDTRIP = 2
const BIT_PARENT_CLEAR = 4
const BIT_TAKE_LOOP = 8
const BIT_MIDHOP_LOOP = 16
const BIT_DEEP_LOOP = 32
const BIT_TAKE_FAILURE = 64
const BIT_CAST_CLEANUP = 128
const BIT_ROOT_CAST = 256
const BIT_U64_WIRE = 512
const BIT_IOU_XFL = 1024
const ALL_CHECKS =
  BIT_ACCOUNT_WALK |
  BIT_DROPS_ROUNDTRIP |
  BIT_PARENT_CLEAR |
  BIT_TAKE_LOOP |
  BIT_MIDHOP_LOOP |
  BIT_DEEP_LOOP |
  BIT_TAKE_FAILURE |
  BIT_CAST_CLEANUP |
  BIT_ROOT_CAST |
  BIT_U64_WIRE |
  BIT_IOU_XFL

// Check groups, matching the hook's `CHK_*` constants. Each recycling loop
// runs >255 iterations, and one execution running all of them would need
// ~130k instructions against the Hook API's 65,535 ceiling — so the hook
// takes a `CHK` parameter naming one group and we OR the accept codes.
const CHK_CHEAP = 0
const CHK_DEEP = 1
const CHK_TAKE_FAILURE = 2
const CHK_CAST = 3
const CHK_MIDHOP = 4
const CHK_IOU = 5

// The IOU the issuer pays the sender before the run, and what the hook
// expects that trust line's balance to round-trip to through `as_xfl()`.
const IOU_CURRENCY = 'USD'
const IOU_AMOUNT = '100'

describe('slot-objects (typed slot layer)', () => {
  let testContext: XrplIntegrationTestContext
  let checks = 0

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)

    // A SignerList on the sender, so the `slot_path!` walks have real hops
    // to take: `signers[sfSignerEntries][0]` must materialize two *owned*
    // intermediates before the deliberate third-hop failure, or the cleanup
    // test proves nothing. (An earlier version walked a field absent from
    // the account root; the first, borrowed-root hop failed and no
    // intermediate was ever allocated.)
    await Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'SignerListSet',
        Account: testContext.alice.classicAddress,
        SignerQuorum: 1,
        SignerEntries: [
          {
            SignerEntry: {
              Account: testContext.bob.classicAddress,
              SignerWeight: 1,
            },
          },
        ],
      } as never,
      wallet: testContext.alice,
    })

    // A trust line from the sender to an issuer, funded with a known amount:
    // the account root's own balance is always native, so an IOU `as_xfl()`
    // needs a `RippleState` object to read. `testContext.carol` plays the
    // issuer.
    await Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'TrustSet',
        Account: testContext.alice.classicAddress,
        LimitAmount: {
          currency: IOU_CURRENCY,
          issuer: testContext.carol.classicAddress,
          value: '1000',
        },
      } as never,
      wallet: testContext.alice,
    })
    await Xrpld.submit(testContext.client, {
      tx: {
        TransactionType: 'Payment',
        Account: testContext.carol.classicAddress,
        Destination: testContext.alice.classicAddress,
        Amount: {
          currency: IOU_CURRENCY,
          issuer: testContext.carol.classicAddress,
          value: IOU_AMOUNT,
        },
      } as never,
      wallet: testContext.carol,
    })

    const hook: iHook = {
      CreateCode: readHookBinaryHexFromNS('slot_objects', 'wasm'),
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

    // One Invoke per check group; the accept codes OR together into the
    // full bitmask each `it` below reads its own bit out of.
    for (const group of [
      CHK_CHEAP,
      CHK_DEEP,
      CHK_TAKE_FAILURE,
      CHK_CAST,
      CHK_MIDHOP,
      CHK_IOU,
    ]) {
      // The IOU check needs the issuer's account ID: the wallets are
      // generated per run, so the hook cannot have it baked in.
      const params = [
        {
          HookParameter: {
            HookParameterName: convertStringToHex('CHK'),
            HookParameterValue: group.toString(16).padStart(2, '0'),
          },
        },
      ]
      if (group === CHK_IOU) {
        params.push({
          HookParameter: {
            HookParameterName: convertStringToHex('ISS'),
            HookParameterValue: Buffer.from(
              decodeAccountID(testContext.carol.classicAddress),
            )
              .toString('hex')
              .toUpperCase(),
          },
        })
      }

      const response = await Xrpld.submit(testContext.client, {
        tx: {
          TransactionType: 'Invoke',
          Account: testContext.alice.classicAddress,
          Destination: testContext.hook1.classicAddress,
          HookParameters: params,
        } as never,
        wallet: testContext.alice,
      })

      const meta = response.meta as TransactionMetadata
      const hookExecutions = await ExecutionUtility.getHookExecutionsFromMeta(
        testContext.client,
        meta,
      )
      expect(hookExecutions.executions.length).toBe(1)
      const execution = hookExecutions.executions[0]
      expect(execution.HookReturnString).toBe(
        'slot-objects: typed slot layer checks',
      )
      // HookReturnCode is a 64-bit int field, serialized as a *hex* string.
      checks |= parseInt(String(execution.HookReturnCode), 16)

      expect(parseInt(execution.HookInstructionCount, 16)).toBeLessThanOrEqual(
        WORST_CASE_INSTRUCTIONS,
      )
    }
  })

  afterAll(async () => {
    await clearAllHooksV3({
      client: testContext.client,
      seed: testContext.hook1.seed,
    } as unknown as SetHookParams)
    await teardownClient(testContext)
  })

  it('walks the account root with typed reads', () => {
    // `SlotObject::from_keylet` on the sender's account keylet, then
    // `.get(sfSequence)/.get(sfAccount)/.get(sfBalance)` — the account read
    // back must be the sender, and the sequence must be a real value.
    expect(checks & BIT_ACCOUNT_WALK).toBe(BIT_ACCOUNT_WALK)
  })

  it('round-trips a native amount through as_xfl and back to drops', () => {
    // `as_xfl()` on a *native* amount yields XAH units, not drops: the host
    // builds it with the drop count as the mantissa and an exponent of -6,
    // then normalizes. `to_int(xfl, 6, false)` must recover the drops the
    // raw wire bytes carry. The design review corrected this fact by a
    // factor of 1e6; this is the assertion that pins it.
    expect(checks & BIT_DROPS_ROUNDTRIP).toBe(BIT_DROPS_ROUNDTRIP)
  })

  it('reads a u64 identically through value() and raw bytes', () => {
    // `u64::value()` decodes the eight wire bytes big-endian rather than
    // using the host's as-int64 mode, which rejects a value with bit 63 set
    // (`sfExchangeRate` legitimately sets it). An account root carries no
    // such field - a native amount's top bits are 0b01 - so this pins the
    // two paths agreeing on a real serialized value; the bit-63 typing is
    // pinned in the trybuild fixtures.
    expect(checks & BIT_U64_WIRE).toBe(BIT_U64_WIRE)
  })

  it('keeps a child slot readable after its parent is cleared', () => {
    // The soundness assumption `slot_path!` is built on: it clears each
    // intermediate as soon as the child exists, which only works because the
    // host *copies* the parent's storage into the child slot. If it aliased,
    // this bit would be clear.
    expect(checks & BIT_PARENT_CLEAR).toBe(BIT_PARENT_CLEAR)
  })

  it('accepts a root slot in try_cast::<STObject>', () => {
    // A root slot reports a high-level object code (serialized type ID
    // 10001-10004), not the ordinary 14 an object *field* reports, so the
    // cast predicate has to accept those too - and still reject a wrong
    // target on the same slot.
    expect(checks & BIT_ROOT_CAST).toBe(BIT_ROOT_CAST)
  })

  it('survives 260 successful three-hop walks without exhausting the slots', () => {
    // Each iteration derives three slots - two intermediates the ladder
    // clears, plus a leaf `take_value()` releases - so 260 iterations move
    // 780 slots through a 255-slot budget. Failing to recycle in either
    // mechanism exhausts it well before the end.
    expect(checks & BIT_DEEP_LOOP).toBe(BIT_DEEP_LOOP)
    expect(checks & BIT_TAKE_LOOP).toBe(BIT_TAKE_LOOP)
  })

  it('leaks nothing when a slot_path! hop fails after two real hops', () => {
    // Hops 1 and 2 succeed against the SignerList installed above, so two
    // owned intermediates exist before hop 3 fails. The ladder clears each
    // unconditionally, before looking at the result.
    expect(checks & BIT_MIDHOP_LOOP).toBe(BIT_MIDHOP_LOOP)
  })

  it('clears the slot when a take_* read fails', () => {
    // The other half of the `take_*` contract: it clears on the failure path
    // too, so 260 *failing* reads must not exhaust the budget either.
    expect(checks & BIT_TAKE_FAILURE).toBe(BIT_TAKE_FAILURE)
  })

  it('clears the slot when a try_cast fails', () => {
    // Any `try_cast` failure consumes the handle and best-effort clears the
    // slot; repeating past the budget is what proves the clear happened.
    expect(checks & BIT_CAST_CLEANUP).toBe(BIT_CAST_CLEANUP)
  })

  it('reads an IOU amount through as_xfl and reports it non-native', () => {
    // The account root's balance is always native, so this reads the trust
    // line funded above - a `RippleState` object whose `sfBalance` is an IOU
    // amount. Together with the drops round-trip above, both branches of
    // `slot_float` are now exercised live.
    //
    // The hook compares magnitudes (`to_int(0, true)`): a RippleState
    // balance is signed relative to whichever account sorts low, which is
    // not a fact worth depending on.
    expect(checks & BIT_IOU_XFL).toBe(BIT_IOU_XFL)
  })

  it('passes every check', () => {
    // Redundant with the ones above, but reports the whole bitmask in one
    // place when something breaks.
    expect(checks).toBe(ALL_CHECKS)
  })
})
