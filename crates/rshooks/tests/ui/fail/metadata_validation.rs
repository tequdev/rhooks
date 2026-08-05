//! Macro-owned validation failures: required fields, duplicate/unknown
//! fields and entries, trigger-mode XOR, directional-set equality, and
//! Unicode-scalar HookName bounds.

use rshooks::metadata;

metadata! {
    HookOn: [Payment],
}

metadata! {
    name: "",
    HookOn: [Payment],
}

metadata! {
    name: "duplicate",
    name: "again",
    HookOn: [Payment],
}

metadata! {
    name: "unknown field",
    HookOn: [Payment],
    Namespace: "nope",
}

metadata! {
    name: "missing trigger",
}

metadata! {
    name: "mixed trigger forms",
    HookOn: [Payment],
    IncomingHookOn: [Payment],
    OutgoingHookOn: [Invoke],
}

metadata! {
    name: "half directional",
    IncomingHookOn: [Payment],
}

metadata! {
    name: "duplicate transaction",
    HookOn: [Payment, Payment],
}

metadata! {
    name: "same directional sets",
    IncomingHookOn: [Payment, Invoke],
    OutgoingHookOn: [Invoke, Payment],
}

metadata! {
    name: "short hook name",
    HookOn: [Payment],
    HookName: "一",
}

metadata! {
    name: "long hook name",
    HookOn: [Payment],
    HookName: "一二三四五六七八九",
}

metadata! {
    name: 42,
    HookOn: [Payment],
}

metadata! {
    name: "qualified variant",
    HookOn: [TxType::Payment],
}

fn main() {}
