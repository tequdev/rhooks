//! Transaction flags (`tfXxx`) and account flags (`asfXxx`).
//!
//! Upstream: `Xahau/xahaud`, branch `release`, `hook/tx_flags.h`.
//!
//! The header groups these into several C enums, one per transaction type
//! (or `AccountFlags` for the `asfXxx` `AccountSet` sub-flags). This
//! translation flattens every enum into a single list of `pub const`s, in
//! header order; no name collides across enums. A few `MPTokenIssuanceCreateFlags`
//! members alias `ls_flags.h` values in the C header (`tfMPTCanLock = lsfMPTCanLock`,
//! etc.) — those are kept as references to the [`crate::ls_flags`] consts
//! rather than re-typed literals, so the two stay in sync by construction.

use crate::ls_flags;

// enum UniversalFlags
/// C: `tfFullyCanonicalSig` (tx_flags.h, `enum UniversalFlags`)
pub const tfFullyCanonicalSig: u32 = 0x8000_0000;

// enum AccountSetFlags
/// C: `tfRequireDestTag` (tx_flags.h, `enum AccountSetFlags`)
pub const tfRequireDestTag: u32 = 0x0001_0000;
/// C: `tfOptionalDestTag` (tx_flags.h, `enum AccountSetFlags`)
pub const tfOptionalDestTag: u32 = 0x0002_0000;
/// C: `tfRequireAuth` (tx_flags.h, `enum AccountSetFlags`)
pub const tfRequireAuth: u32 = 0x0004_0000;
/// C: `tfOptionalAuth` (tx_flags.h, `enum AccountSetFlags`)
pub const tfOptionalAuth: u32 = 0x0008_0000;
/// C: `tfDisallowXRP` (tx_flags.h, `enum AccountSetFlags`)
pub const tfDisallowXRP: u32 = 0x0010_0000;
/// C: `tfAllowXRP` (tx_flags.h, `enum AccountSetFlags`)
pub const tfAllowXRP: u32 = 0x0020_0000;

// enum AccountFlags (asfXxx: AccountSet SetFlag/ClearFlag sub-flags)
/// C: `asfRequireDest` (tx_flags.h, `enum AccountFlags`)
pub const asfRequireDest: u32 = 1;
/// C: `asfRequireAuth` (tx_flags.h, `enum AccountFlags`)
pub const asfRequireAuth: u32 = 2;
/// C: `asfDisallowXRP` (tx_flags.h, `enum AccountFlags`)
pub const asfDisallowXRP: u32 = 3;
/// C: `asfDisableMaster` (tx_flags.h, `enum AccountFlags`)
pub const asfDisableMaster: u32 = 4;
/// C: `asfAccountTxnID` (tx_flags.h, `enum AccountFlags`)
pub const asfAccountTxnID: u32 = 5;
/// C: `asfNoFreeze` (tx_flags.h, `enum AccountFlags`)
pub const asfNoFreeze: u32 = 6;
/// C: `asfGlobalFreeze` (tx_flags.h, `enum AccountFlags`)
pub const asfGlobalFreeze: u32 = 7;
/// C: `asfDefaultRipple` (tx_flags.h, `enum AccountFlags`)
pub const asfDefaultRipple: u32 = 8;
/// C: `asfDepositAuth` (tx_flags.h, `enum AccountFlags`)
pub const asfDepositAuth: u32 = 9;
/// C: `asfAuthorizedNFTokenMinter` (tx_flags.h, `enum AccountFlags`)
pub const asfAuthorizedNFTokenMinter: u32 = 10;
/// C: `asfTshCollect` (tx_flags.h, `enum AccountFlags`)
pub const asfTshCollect: u32 = 11;
/// C: `asfDisallowIncomingNFTokenOffer` (tx_flags.h, `enum AccountFlags`)
pub const asfDisallowIncomingNFTokenOffer: u32 = 12;
/// C: `asfDisallowIncomingCheck` (tx_flags.h, `enum AccountFlags`)
pub const asfDisallowIncomingCheck: u32 = 13;
/// C: `asfDisallowIncomingPayChan` (tx_flags.h, `enum AccountFlags`)
pub const asfDisallowIncomingPayChan: u32 = 14;
/// C: `asfDisallowIncomingTrustline` (tx_flags.h, `enum AccountFlags`)
pub const asfDisallowIncomingTrustline: u32 = 15;
/// C: `asfDisallowIncomingRemit` (tx_flags.h, `enum AccountFlags`)
pub const asfDisallowIncomingRemit: u32 = 16;
/// C: `asfAllowTrustLineClawback` (tx_flags.h, `enum AccountFlags`)
pub const asfAllowTrustLineClawback: u32 = 17;

// enum OfferCreateFlags
/// C: `tfPassive` (tx_flags.h, `enum OfferCreateFlags`)
pub const tfPassive: u32 = 0x0001_0000;
/// C: `tfImmediateOrCancel` (tx_flags.h, `enum OfferCreateFlags`)
pub const tfImmediateOrCancel: u32 = 0x0002_0000;
/// C: `tfFillOrKill` (tx_flags.h, `enum OfferCreateFlags`)
pub const tfFillOrKill: u32 = 0x0004_0000;
/// C: `tfSell` (tx_flags.h, `enum OfferCreateFlags`)
pub const tfSell: u32 = 0x0008_0000;

// enum PaymentFlags
/// C: `tfNoRippleDirect` (tx_flags.h, `enum PaymentFlags`)
pub const tfNoRippleDirect: u32 = 0x0001_0000;
/// C: `tfPartialPayment` (tx_flags.h, `enum PaymentFlags`)
pub const tfPartialPayment: u32 = 0x0002_0000;
/// C: `tfLimitQuality` (tx_flags.h, `enum PaymentFlags`)
pub const tfLimitQuality: u32 = 0x0004_0000;

// enum TrustSetFlags
/// C: `tfSetfAuth` (tx_flags.h, `enum TrustSetFlags`)
pub const tfSetfAuth: u32 = 0x0001_0000;
/// C: `tfSetNoRipple` (tx_flags.h, `enum TrustSetFlags`)
pub const tfSetNoRipple: u32 = 0x0002_0000;
/// C: `tfClearNoRipple` (tx_flags.h, `enum TrustSetFlags`)
pub const tfClearNoRipple: u32 = 0x0004_0000;
/// C: `tfSetFreeze` (tx_flags.h, `enum TrustSetFlags`)
pub const tfSetFreeze: u32 = 0x0010_0000;
/// C: `tfClearFreeze` (tx_flags.h, `enum TrustSetFlags`)
pub const tfClearFreeze: u32 = 0x0020_0000;
/// C: `tfSetDeepFreeze` (tx_flags.h, `enum TrustSetFlags`)
pub const tfSetDeepFreeze: u32 = 0x0040_0000;
/// C: `tfClearDeepFreeze` (tx_flags.h, `enum TrustSetFlags`)
pub const tfClearDeepFreeze: u32 = 0x0080_0000;

// enum EnableAmendmentFlags
/// C: `tfGotMajority` (tx_flags.h, `enum EnableAmendmentFlags`)
pub const tfGotMajority: u32 = 0x0001_0000;
/// C: `tfLostMajority` (tx_flags.h, `enum EnableAmendmentFlags`)
pub const tfLostMajority: u32 = 0x0002_0000;
/// C: `tfTestSuite` (tx_flags.h, `enum EnableAmendmentFlags`)
pub const tfTestSuite: u32 = 0x8000_0000;

// enum PaymentChannelClaimFlags
/// C: `tfRenew` (tx_flags.h, `enum PaymentChannelClaimFlags`)
pub const tfRenew: u32 = 0x0001_0000;
/// C: `tfClose` (tx_flags.h, `enum PaymentChannelClaimFlags`)
pub const tfClose: u32 = 0x0002_0000;

// enum NFTokenMintFlags
/// C: `tfBurnable` (tx_flags.h, `enum NFTokenMintFlags`)
pub const tfBurnable: u32 = 0x0000_0001;
/// C: `tfOnlyXRP` (tx_flags.h, `enum NFTokenMintFlags`)
pub const tfOnlyXRP: u32 = 0x0000_0002;
/// C: `tfTrustLine` (tx_flags.h, `enum NFTokenMintFlags`)
pub const tfTrustLine: u32 = 0x0000_0004;
/// C: `tfTransferable` (tx_flags.h, `enum NFTokenMintFlags`)
pub const tfTransferable: u32 = 0x0000_0008;
/// C: `tfMutable` (tx_flags.h, `enum NFTokenMintFlags`)
pub const tfMutable: u32 = 0x0000_0010;
/// C: `tfStrongTSH` (tx_flags.h, `enum NFTokenMintFlags`)
pub const tfStrongTSH: u32 = 0x0000_8000;

// enum MPTokenIssuanceCreateFlags (aliases of ls_flags.h values in the C header)
/// C: `tfMPTCanLock` (tx_flags.h, `enum MPTokenIssuanceCreateFlags`) = `lsfMPTCanLock`
pub const tfMPTCanLock: u32 = ls_flags::lsfMPTCanLock;
/// C: `tfMPTRequireAuth` (tx_flags.h, `enum MPTokenIssuanceCreateFlags`) = `lsfMPTRequireAuth`
pub const tfMPTRequireAuth: u32 = ls_flags::lsfMPTRequireAuth;
/// C: `tfMPTCanEscrow` (tx_flags.h, `enum MPTokenIssuanceCreateFlags`) = `lsfMPTCanEscrow`
pub const tfMPTCanEscrow: u32 = ls_flags::lsfMPTCanEscrow;
/// C: `tfMPTCanTrade` (tx_flags.h, `enum MPTokenIssuanceCreateFlags`) = `lsfMPTCanTrade`
pub const tfMPTCanTrade: u32 = ls_flags::lsfMPTCanTrade;
/// C: `tfMPTCanTransfer` (tx_flags.h, `enum MPTokenIssuanceCreateFlags`) = `lsfMPTCanTransfer`
pub const tfMPTCanTransfer: u32 = ls_flags::lsfMPTCanTransfer;
/// C: `tfMPTCanClawback` (tx_flags.h, `enum MPTokenIssuanceCreateFlags`) = `lsfMPTCanClawback`
pub const tfMPTCanClawback: u32 = ls_flags::lsfMPTCanClawback;

// enum MPTokenAuthorizeFlags
/// C: `tfMPTUnauthorize` (tx_flags.h, `enum MPTokenAuthorizeFlags`)
pub const tfMPTUnauthorize: u32 = 0x0000_0001;

// enum MPTokenIssuanceSetFlags
/// C: `tfMPTLock` (tx_flags.h, `enum MPTokenIssuanceSetFlags`)
pub const tfMPTLock: u32 = 0x0000_0001;
/// C: `tfMPTUnlock` (tx_flags.h, `enum MPTokenIssuanceSetFlags`)
pub const tfMPTUnlock: u32 = 0x0000_0002;

// enum NFTokenCreateOfferFlags
/// C: `tfSellNFToken` (tx_flags.h, `enum NFTokenCreateOfferFlags`)
pub const tfSellNFToken: u32 = 0x0000_0001;

// enum ClaimRewardFlags
/// C: `tfOptOut` (tx_flags.h, `enum ClaimRewardFlags`)
pub const tfOptOut: u32 = 0x0000_0001;

// enum CronSetFlags
/// C: `tfCronUnset` (tx_flags.h, `enum CronSetFlags`)
pub const tfCronUnset: u32 = 0x0000_0001;

// enum AMMClawbackFlags
/// C: `tfClawTwoAssets` (tx_flags.h, `enum AMMClawbackFlags`)
pub const tfClawTwoAssets: u32 = 0x0000_0001;

// enum BridgeModifyFlags
/// C: `tfClearAccountCreateAmount` (tx_flags.h, `enum BridgeModifyFlags`)
pub const tfClearAccountCreateAmount: u32 = 0x0001_0000;
