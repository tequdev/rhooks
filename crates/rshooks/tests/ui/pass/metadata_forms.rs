//! Both trigger-selection forms accepted by `metadata!`, including optional
//! fields, raw/cooked strings, Unicode HookName counting, and acronym-heavy
//! transaction variants whose JSON names differ from their Rust variants.

mod legacy {
    use rshooks::metadata;

    metadata! {
        name: "legacy hook",
        description: r#"A "quoted" description."#,
        HookOn: [Payment, SetHook, SetRegularKey, AMMCreate],
        HookCanEmit: [NFTokenMint, XChainCreateClaimID],
        HookName: "支払",
    }
}

mod directional {
    use rshooks::metadata;

    metadata! {
        name: "directional hook",
        IncomingHookOn: [Payment, Invoke],
        OutgoingHookOn: [Payment],
    }
}

mod no_trigger_selection {
    use rshooks::metadata;

    metadata! {
        name: "no trigger selection",
    }
}

fn main() {}
