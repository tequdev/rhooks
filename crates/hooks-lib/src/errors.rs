//! User-defined Hook error enums that map to `i64` rollback/accept codes.
//!
//! [`hook_errors!`] expands a C-like enum declaration into a `#[repr(i64)]`
//! enum plus the small amount of boilerplate needed to use it as an exit
//! code: `impl From<Enum> for i64` and an inherent `code(self) -> i64`
//! method. Paired with [`exit_on_err!`] and the `i64::from`-based
//! `code`/`msg` argument of [`rollback!`](crate::rollback) /
//! [`accept!`](crate::accept), this gives a small `Result`-based idiom for
//! hook logic — write ordinary helper functions returning
//! `Result<T, YourErrorEnum>`, and convert to `rollback` only at the point a
//! hook actually needs to exit — without introducing a full error-type
//! hierarchy on top of [`crate::error::HookError`] (which remains the type
//! for *Hook API* failures; user-defined error enums are a separate,
//! unrelated concept that happens to also bottom out in an `i64`).

/// Define an enum of user error codes for use as `rollback!`/`accept!` exit
/// codes.
///
/// Each variant requires an explicit `i64`-valued discriminant — the
/// macro's grammar itself enforces this, there is no separate check — and
/// the macro expands to:
///
/// - a `#[repr(i64)]`, `Debug + Clone + Copy + PartialEq + Eq` enum with the
///   given variants and discriminants (doc comments on the enum and on each
///   variant are passed through verbatim, so `missing_docs` is satisfied
///   the same way it would be for a hand-written enum);
/// - `impl From<EnumName> for i64`;
/// - an inherent `fn code(self) -> i64` — the same conversion, as a method,
///   for call sites that prefer `err.code()` over `i64::from(err)`.
///
/// # Examples
///
/// ```
/// use hooks_lib::hook_errors;
///
/// hook_errors! {
///     /// Firewall error codes.
///     pub enum FirewallError {
///         /// The sender is on the blacklist.
///         BlockedAccount = 1,
///         /// A required Hook parameter was missing.
///         MissingParam = 2,
///     }
/// }
///
/// assert_eq!(FirewallError::BlockedAccount.code(), 1);
/// assert_eq!(i64::from(FirewallError::MissingParam), 2);
/// ```
///
/// Negative discriminants work the same way (the expression after `=` is
/// not restricted to positive integer literals):
///
/// ```
/// use hooks_lib::hook_errors;
///
/// hook_errors! {
///     enum Negative {
///         First = -1,
///         Second = -2,
///     }
/// }
///
/// assert_eq!(Negative::First.code(), -1);
/// assert_eq!(i64::from(Negative::Second), -2);
/// ```
#[macro_export]
macro_rules! hook_errors {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $code:expr
            ),+ $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        #[repr(i64)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant = $code,
            )+
        }

        impl $name {
            /// The `i64` rollback/accept code for this variant.
            #[must_use]
            $vis fn code(self) -> i64 {
                self as i64
            }
        }

        impl ::core::convert::From<$name> for i64 {
            fn from(value: $name) -> i64 {
                value as i64
            }
        }
    };
}

/// Evaluate a `Result<T, E>` (typically returned by a plain helper
/// function), returning `T` on `Ok`, or rolling the hook back on `Err`.
///
/// `E` must implement `Into<i64>` — any enum defined with [`hook_errors!`]
/// does, via the `From<EnumName> for i64` impl it generates; a plain `i64`
/// error works too, since `i64: From<i64>`. Expands to a `match` that calls
/// [`rollback!`](crate::rollback) with `msg` and the error on the `Err` arm
/// (which — like `rollback!` itself — never returns on the real wasm host);
/// this is the same "convert at the boundary" pattern the crate already
/// uses for [`accept!`](crate::accept)/[`rollback!`](crate::rollback)'s own
/// `i64::from($code)` handling of `Into<i64>` codes.
///
/// # Examples
///
/// The `Ok` path just returns the wrapped value, so it is safe to run as an
/// ordinary doctest:
///
/// ```
/// use hooks_lib::{exit_on_err, hook_errors};
///
/// hook_errors! {
///     /// Firewall error codes.
///     pub enum FirewallError {
///         /// The sender is on the blacklist.
///         BlockedAccount = 1,
///     }
/// }
///
/// fn check(blocked: bool) -> Result<u32, FirewallError> {
///     if blocked {
///         Err(FirewallError::BlockedAccount)
///     } else {
///         Ok(42)
///     }
/// }
///
/// let value = exit_on_err!(b"firewall: blocked", check(false));
/// assert_eq!(value, 42);
/// ```
///
/// The `Err` path rolls back, which on a host build never returns (see
/// [`crate::api::control::rollback`]'s own doc comment for why) — so this
/// variant is `no_run`; on the real wasm host it unwinds hook execution
/// instead of looping:
///
/// ```no_run
/// use hooks_lib::{exit_on_err, hook_errors};
///
/// hook_errors! {
///     /// Firewall error codes.
///     pub enum FirewallError {
///         /// The sender is on the blacklist.
///         BlockedAccount = 1,
///     }
/// }
///
/// fn check(blocked: bool) -> Result<u32, FirewallError> {
///     if blocked {
///         Err(FirewallError::BlockedAccount)
///     } else {
///         Ok(42)
///     }
/// }
///
/// // Never returns: rolls back with b"firewall: blocked" and code 1.
/// let _value = exit_on_err!(b"firewall: blocked", check(true));
/// ```
#[macro_export]
macro_rules! exit_on_err {
    ($msg:expr, $result:expr) => {
        match $result {
            ::core::result::Result::Ok(value) => value,
            ::core::result::Result::Err(err) => $crate::rollback!($msg, err),
        }
    };
}

#[cfg(test)]
mod tests {
    hook_errors! {
        /// Test error enum exercising positive discriminants.
        pub enum SampleError {
            /// First variant.
            First = 1,
            /// Second variant.
            Second = 2,
        }
    }

    hook_errors! {
        /// Test error enum exercising negative discriminants.
        enum NegativeError {
            /// First variant.
            First = -1,
            /// Second variant.
            Second = -2,
        }
    }

    #[test]
    fn code_matches_discriminant() {
        assert_eq!(SampleError::First.code(), 1);
        assert_eq!(SampleError::Second.code(), 2);
    }

    #[test]
    fn into_i64_matches_code() {
        assert_eq!(i64::from(SampleError::First), SampleError::First.code());
        assert_eq!(i64::from(SampleError::Second), SampleError::Second.code());
    }

    #[test]
    fn negative_discriminants_round_trip() {
        assert_eq!(NegativeError::First.code(), -1);
        assert_eq!(i64::from(NegativeError::Second), -2);
    }

    #[test]
    fn derives_are_usable() {
        extern crate std;
        use std::format;

        // Debug + Clone + Copy + PartialEq + Eq, as documented.
        let a = SampleError::First;
        let b = a;
        assert_eq!(a, b);
        assert_eq!(format!("{a:?}"), "First");
    }

    #[test]
    fn exit_on_err_returns_ok_value() {
        let result: Result<u32, SampleError> = Ok(7);
        let value = exit_on_err!(b"unused", result);
        assert_eq!(value, 7);
    }
}
