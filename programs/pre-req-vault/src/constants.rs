use anchor_lang::prelude::*;

#[constant]
pub const COUNTER_SEED: &[u8] = b"counter";

#[constant]
pub const HELLO_WORLD_LAMPORTS: u64 = 1;

#[constant]
pub const MAX_COUNT: u64 = 10;

/// GitHub handle recorded on-chain by the registration CPI in `withdraw`.
///
/// The registration program stores this string inside the `ApplicationAccount`
/// PDA. It is a compile-time constant rather than an instruction argument so
/// that the on-chain program itself attests to the handle — a caller cannot
/// register somebody else's username through this vault.
#[constant]
pub const GITHUB_USERNAME: &str = "nisargpatel7042lva";
