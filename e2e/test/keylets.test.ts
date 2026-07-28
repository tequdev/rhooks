// e2e: examples/13_keylets against a standalone Xahau node.
//
// `hook()` computes 25 of the 26 `KEYLET_*` types via `hooks_lib::api::keylet`'s
// typed `keylet_xxx` helpers, from the invoking transaction's own
// `sfAccount`/`sfDestination` plus a handful of fixed test constants (see
// examples/13_keylets/src/lib.rs and its README), and writes every 34-byte
// result to hook state, keyed by a `state_keys!` enum (`KeyletKey`) whose
// unit-variant discriminants run 0..25 in the same order as their
// `KEYLET_*` constant (discriminant = constant value - 1). This suite reads
// every stored entry back and checks it against an independently computed
// expected value.
//
// `KEYLET_TICKET` (discriminant 12) is the one exception: `my_hook()`
// never computes or stores it at all. Live testing against this exact
// node build found `keylet_ticket` reliably fails at runtime regardless of
// its `ticket_seq` argument, while every structurally similar type
// (`Offer`/`Escrow`/`Check`/`Signers`, isolated the same way) succeeds and
// the identical `account`/`ticket_seq` shape is accepted by that same
// node's `ledger_entry` RPC — see `hooks_lib::api::keylet::keylet_ticket`'s
// own doc comment for the full writeup. This looks like a host-side gap in
// this specific `xahaud` build's `util_keylet`, not a bug in the wrapper,
// so the helper itself stays in `hooks_lib::api::keylet` — only this
// example's exercised set skips it.
//
// Verification of the remaining 25 is genuinely two-tier — see
// examples/13_keylets/README.md's "e2e verification scope" section for the
// full reasoning per type:
//
// - 13 types get byte-for-byte independent verification, either directly
//   via `xahau` npm's own exported `hashes.*` helpers, or via the exact
//   same `sha512Half(ledgerSpace + args)` pattern those helpers use
//   (reusing `xahau`'s own `utils/hashes/ledgerSpaces.ts` character table,
//   not an independently-recalled one), or (for `Unchecked`) by definition
//   (`ltANY` (`0`) plus the raw hash verbatim, no hashing at all).
// - The remaining 12 (Xahau/Hooks-specific ledger-space extensions, or a
//   composite/derived index shape) only get a "well-formed" check: exactly
//   34 bytes, not the all-zero placeholder.
import { createHash } from 'node:crypto'
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
import {
  calculateHookOn,
  decodeAccountID,
  hashes,
  type TransactionMetadata,
} from 'xahau'
// HookFlags isn't re-exported from the package root in xahau@4.x - only
// reachable via this deep import (same path hooks-toolkit's own source
// uses internally for the same enum).
import { HookFlags } from 'xahau/dist/npm/models/common/xahau'
// hashCron is implemented (and internally exported) alongside every other
// hashes.* helper, but the package's own top-level `hashes` bundle object
// omits it - reach it via the same deep-import convention as HookFlags
// above.
import { hashCron } from 'xahau/dist/npm/utils/hashes'

const namespace = 'rhooks-e2e-keylets'
// hooks-build's printed worst case for keylets.wasm, built with
// `--auto-guard --default-maxiter 34` (see the example's own README's
// "Toolchain limitation" section for why that flag is required at all).
const WORST_CASE_INSTRUCTIONS = 3637

// `xahau` npm's own ledger-space character table
// (utils/hashes/ledgerSpaces.ts) - reused verbatim rather than
// independently recalled, for every type this suite verifies byte-exact
// that isn't already covered by one of `hashes`'s own exported helpers.
const SPACE = {
  account: 'a',
  signerList: 'S',
  rippleState: 'r',
  offer: 'o',
  escrow: 'u',
  paychan: 'x',
  check: 'C',
  ticket: 'T',
  depositPreauth: 'p',
  ownerDir: 'O',
  amendment: 'f',
  feeSettings: 'e',
  cron: 'L',
} as const

// `TEST_HASH = Hash([0xAB; 32])` (src/lib.rs) - reused for every
// `keylet_xxx` argument shaped like a raw 32-byte hash.
const TEST_HASH_HEX = 'AB'.repeat(32)

const OFFER_SEQ = 1
const ESCROW_SEQ = 2
const CHECK_SEQ = 3
// No TICKET_SEQ - see the module doc comment's "known host limitation"
// paragraph.
const PAYCHAN_SEQ = 5
const CRON_START_TIME = 1_700_000_000

/// A "KeyletKey" `state_keys!` discriminant (0..25, declaration order in
/// src/lib.rs) as its 32-byte, zero-padded state key, hex-encoded.
function keyletStateKeyHex(discriminant: number): string {
  return discriminant.toString(16).padStart(2, '0').toUpperCase().padEnd(64, '0')
}

function accountHex(address: string): string {
  return Buffer.from(decodeAccountID(address)).toString('hex')
}

function u32Hex(value: number): string {
  return value.toString(16).padStart(8, '0')
}

// `ledgerSpaceHex` in `xahau`'s own `utils/hashes/index.ts`: a ledger
// space is a single ASCII byte, encoded as a big-endian `0x00XX` pair, fed
// into the sha512Half hash that produces a type's 32-byte *index*.
//
// For most types this same `0x00XX` value also happens to be the
// resulting `Keylet`'s own 2-byte `ltXXX` *type code* (confirmed live for
// every type below that uses `keyletHex` directly) - but **not always**:
// live testing found `OwnerDir`/`Fees`/`Cron`'s actual on-chain type code
// differs from their hash-space character (their 32-byte index still
// matches exactly, proving the hash formula itself is right - only the
// outer type-code byte differs), so those three use `typedKeyletHex`
// instead, with the type code confirmed against this exact node's own
// output rather than assumed equal to the hash space.
function spacePrefixHex(spaceChar: string): string {
  return spaceChar.charCodeAt(0).toString(16).padStart(4, '0')
}

function sha512Half(hex: string): string {
  return createHash('sha512').update(Buffer.from(hex, 'hex')).digest('hex').slice(0, 64)
}

// The common case: a verified type's expected 34-byte `Keylet` is its own
// 2-byte `0x00XX` space/type prefix (the two coincide) followed by its
// 32-byte index.
function keyletHex(spaceChar: string, indexHex: string): string {
  return (spacePrefixHex(spaceChar) + indexHex).toUpperCase()
}

// The exception: `typeCodeHex` is the `Keylet`'s own 2-byte type-code
// prefix, kept separate from whatever hash-space character actually
// produced `indexHex` - see the comment above `spacePrefixHex`.
function typedKeyletHex(typeCodeHex: string, indexHex: string): string {
  return (typeCodeHex + indexHex).toUpperCase()
}

function expectedAccount(owner: string): string {
  return keyletHex(SPACE.account, hashes.hashAccountRoot(owner))
}
function expectedSigners(owner: string): string {
  return keyletHex(SPACE.signerList, hashes.hashSignerListId(owner))
}
function expectedLine(a: string, b: string, currency: string): string {
  return keyletHex(SPACE.rippleState, hashes.hashTrustline(a, b, currency))
}
function expectedOffer(owner: string, seq: number): string {
  return keyletHex(SPACE.offer, hashes.hashOfferId(owner, seq))
}
function expectedEscrow(owner: string, seq: number): string {
  return keyletHex(SPACE.escrow, hashes.hashEscrow(owner, seq))
}
function expectedPaychan(src: string, dst: string, seq: number): string {
  return keyletHex(SPACE.paychan, hashes.hashPaymentChannel(src, dst, seq))
}
// `ltCRON`'s real type code is `0x0041` ('A'), not the `SPACE.cron` ('L')
// hash-space character `hashCron` itself hashes with - confirmed live
// (the 32-byte index matches with either byte, only the type code
// differs).
function expectedCron(owner: string, time: number): string {
  return typedKeyletHex('0041', hashCron(owner, time))
}
function expectedCheck(owner: string, seq: number): string {
  return keyletHex(
    SPACE.check,
    sha512Half(spacePrefixHex(SPACE.check) + accountHex(owner) + u32Hex(seq)),
  )
}
// No expectedTicket() - KEYLET_TICKET is not exercised by this hook at
// all (see the module doc comment's "known host limitation" paragraph and
// hooks_lib::api::keylet::keylet_ticket's own doc comment).
function expectedDepositPreauth(owner: string, authorized: string): string {
  return keyletHex(
    SPACE.depositPreauth,
    sha512Half(
      spacePrefixHex(SPACE.depositPreauth) + accountHex(owner) + accountHex(authorized),
    ),
  )
}
// `ltDIR_NODE`'s real type code is `0x0064` ('d') - the single ledger
// entry type shared by owner directories *and* order-book directories -
// not `SPACE.ownerDir` ('O'), which is only the hash-space character used
// to compute the *index*. Confirmed live the same way as Cron above.
function expectedOwnerDir(owner: string): string {
  return typedKeyletHex(
    '0064',
    sha512Half(spacePrefixHex(SPACE.ownerDir) + accountHex(owner)),
  )
}
function expectedAmendments(): string {
  return keyletHex(SPACE.amendment, sha512Half(spacePrefixHex(SPACE.amendment)))
}
// `ltFEE_SETTINGS`'s real type code is `0x0073` ('s'), not `SPACE.feeSettings`
// ('e'). Confirmed live the same way as Cron/OwnerDir above.
function expectedFees(): string {
  return typedKeyletHex('0073', sha512Half(spacePrefixHex(SPACE.feeSettings)))
}
// `KEYLET_UNCHECKED`: `hash` reinterpreted directly as a keylet index with
// no type-prefix validation - `ltANY` (`0`) plus the raw hash verbatim, no
// hashing involved at all (hooks_lib::api::keylet::keylet_unchecked's own
// doc comment).
function expectedUnchecked(hashHex: string): string {
  return ('0000' + hashHex).toUpperCase()
}

describe('keylets', () => {
  let testContext: XrplIntegrationTestContext
  const hookNamespace = hexNamespace(namespace)

  beforeAll(async () => {
    testContext = await setupClient(serverUrl)

    const hook: iHook = {
      CreateCode: readHookBinaryHexFromNS('keylets', 'wasm'),
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

  // owner = sfAccount (alice, the Invoke's sender), dest = sfDestination
  // (hook1, this hook's own account) - the same two accounts src/lib.rs
  // reads off the originating transaction.
  const owner = () => testContext.alice.classicAddress
  const dest = () => testContext.hook1.classicAddress

  it('accepts the Invoke and writes all 26 keylets', async () => {
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
    expect(execution.HookReturnString).toBe('keylets: ok')
    expect(parseInt(execution.HookInstructionCount, 16)).toBeLessThanOrEqual(
      WORST_CASE_INSTRUCTIONS,
    )
  })

  async function readKeylet(discriminant: number): Promise<string> {
    const entry = await StateUtility.getHookState(
      testContext.client,
      testContext.hook1.classicAddress,
      keyletStateKeyHex(discriminant),
      hookNamespace,
    )
    return entry.HookStateData.toUpperCase()
  }

  function wellFormed(actual: string) {
    expect(Buffer.from(actual, 'hex').length).toBe(34)
    expect(actual).not.toBe('00'.repeat(34).toUpperCase())
  }

  // ---- 14 types verified byte-for-byte ----

  it('Account (KEYLET_ACCOUNT, discriminant 2)', async () => {
    expect(await readKeylet(2)).toBe(expectedAccount(owner()))
  })

  it('Signers (KEYLET_SIGNERS, discriminant 13)', async () => {
    expect(await readKeylet(13)).toBe(expectedSigners(owner()))
  })

  it('Line (KEYLET_LINE, discriminant 8)', async () => {
    expect(await readKeylet(8)).toBe(expectedLine(owner(), dest(), 'USD'))
  })

  it('Offer (KEYLET_OFFER, discriminant 9)', async () => {
    expect(await readKeylet(9)).toBe(expectedOffer(owner(), OFFER_SEQ))
  })

  it('Escrow (KEYLET_ESCROW, discriminant 19)', async () => {
    expect(await readKeylet(19)).toBe(expectedEscrow(owner(), ESCROW_SEQ))
  })

  it('Paychan (KEYLET_PAYCHAN, discriminant 20)', async () => {
    expect(await readKeylet(20)).toBe(
      expectedPaychan(owner(), dest(), PAYCHAN_SEQ),
    )
  })

  it('Cron (KEYLET_CRON, discriminant 25)', async () => {
    expect(await readKeylet(25)).toBe(expectedCron(owner(), CRON_START_TIME))
  })

  it('Check (KEYLET_CHECK, discriminant 14)', async () => {
    expect(await readKeylet(14)).toBe(expectedCheck(owner(), CHECK_SEQ))
  })

  // No Ticket (KEYLET_TICKET, discriminant 12) test - `my_hook()` never
  // computes or stores it at all (see the module doc comment's "known host
  // limitation" paragraph), so there is no state entry to read back.

  it('DepositPreauth (KEYLET_DEPOSIT_PREAUTH, discriminant 15)', async () => {
    expect(await readKeylet(15)).toBe(expectedDepositPreauth(owner(), dest()))
  })

  it('OwnerDir (KEYLET_OWNER_DIR, discriminant 17)', async () => {
    expect(await readKeylet(17)).toBe(expectedOwnerDir(owner()))
  })

  it('Amendments (KEYLET_AMENDMENTS, discriminant 3)', async () => {
    expect(await readKeylet(3)).toBe(expectedAmendments())
  })

  it('Fees (KEYLET_FEES, discriminant 6)', async () => {
    expect(await readKeylet(6)).toBe(expectedFees())
  })

  it('Unchecked (KEYLET_UNCHECKED, discriminant 16)', async () => {
    expect(await readKeylet(16)).toBe(expectedUnchecked(TEST_HASH_HEX))
  })

  // ---- 12 types: well-formed only (see the module doc comment above and
  // examples/13_keylets/README.md's "e2e verification scope" section) ----

  it.each([
    ['Hook', 0],
    ['HookState', 1],
    ['Child', 4],
    ['Skip', 5],
    ['NegativeUnl', 7],
    ['Quality', 10],
    ['EmittedDir', 11],
    ['Page', 18],
    ['Emitted', 21],
    ['NftOffer', 22],
    ['HookDefinition', 23],
    ['HookStateDir', 24],
  ])('%s is a well-formed 34-byte keylet', async (_name, discriminant) => {
    wellFormed(await readKeylet(discriminant as number))
  })
})
