//! Both trigger-selection forms accepted by `metadata!`, including optional
//! fields, raw/cooked strings, Unicode HookName counting, and acronym-heavy
//! transaction variants whose JSON names differ from their Rust variants.

mod legacy {
    use hooks_lib::metadata;

    metadata! {
        name: "legacy hook",
        description: r#"A "quoted" description."#,
        HookOn: [Payment, HookSet, RegularKeySet, AmmCreate],
        HookCanEmit: [NftokenMint, XchainCreateClaimId],
        HookName: "支払",
    }
}

mod directional {
    use hooks_lib::metadata;

    metadata! {
        name: "directional hook",
        IncomingHookOn: [Payment, Invoke],
        OutgoingHookOn: [Payment],
    }
}

fn main() {}
