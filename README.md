# pre-req-vault — Turbin3 Prerequisite Challenge

An Anchor SOL vault, extended so that `withdraw` performs a **Cross-Program
Invocation (CPI)** into the Turbin3 registration program to record a GitHub
handle on-chain.

| | |
|---|---|
| **Vault program (deployed by me)** | [`9PrupsAJcRLDZAS5wCKpJttKBREHFLkCBgauqMjWL4rv`](https://explorer.solana.com/address/9PrupsAJcRLDZAS5wCKpJttKBREHFLkCBgauqMjWL4rv?cluster=devnet) |
| **Registration program (Turbin3's)** | [`TRBZyQHB3m68FGeVsqTK39Wm4xejadjVhP5MAZaKWDM`](https://explorer.solana.com/address/TRBZyQHB3m68FGeVsqTK39Wm4xejadjVhP5MAZaKWDM?cluster=devnet) |
| **Registered GitHub handle** | `nisargpatel7042lva` |
| **`ApplicationAccount` PDA** | [`3HzjHaxztQ2qPztzBJXrHhdKB25usg13qZFnRk5yRm4T`](https://explorer.solana.com/address/3HzjHaxztQ2qPztzBJXrHhdKB25usg13qZFnRk5yRm4T?cluster=devnet) |
| **Withdraw tx (contains the CPI)** | [`39y4FNeu…FoCJ85B`](https://explorer.solana.com/tx/39y4FNeuKaMXouLeEtcqEGVvJBfRJ6My6LasDrT8dwqSMZ9w7DmEwxKr51cxG33htUpshuRFxrZF1kra8FoCJ85B?cluster=devnet) |
| **Cluster** | devnet |
| **Architecture diagram** | [docs/excalidraw/architecture.png](docs/excalidraw/architecture.png) &middot; [live page](https://nisargpatel7042lva.github.io/turbin3-prereq-vault/) |

---

## 1. How the vault works

Solana programs are stateless, so every byte of vault state lives in accounts.
This program uses exactly two, both **PDAs** derived from the program itself.

### Accounts

| Account | Seeds | Owner | Holds |
|---|---|---|---|
| `vault_state` | `["state", user]` | vault program | `vault_bump: u8`, `state_bump: u8` (16 bytes + 8 discriminator) |
| `vault` | `["vault", vault_state]` | System Program | nothing but lamports — it *is* the balance |

Two design points worth noticing:

- **The vault is a `SystemAccount`, not a data account.** It stores no state; it
  only holds lamports. That is why the System Program owns it and why moving
  money in and out is a plain System `transfer` rather than manual lamport
  arithmetic.
- **`vault_state` is seeded on the user, `vault` is seeded on `vault_state`.**
  That chain means one vault per wallet, and it makes `vault_state` the single
  thing you need in order to re-derive everything else.

The two bumps are cached in `vault_state` on `initialize`. Later instructions
pass `bump = vault_state.vault_bump` instead of re-deriving, which saves the
compute of `find_program_address` on every call.

### Instructions

| Instruction | What it does | Who signs the lamport movement |
|---|---|---|
| `initialize` | Creates `vault_state`, stores both bumps. The `vault` PDA is only *derived* here, not created — it springs into existence when it first receives lamports. | — |
| `deposit(amount)` | System transfer `user → vault`. | the user |
| `withdraw(amount)` | System transfer `vault → user`, **then CPIs into the registration program**. | the `vault` PDA, via `CpiContext::new_with_signer` |
| `close` | Drains the entire `vault` balance to the user and closes `vault_state` (`close = user` refunds its rent). | the `vault` PDA |

### Why `withdraw` and `close` need signer seeds

A PDA has no private key. To move lamports *out* of the vault, the program
proves ownership by passing the seeds that derive it:

```rust
let seeds = &[b"vault", vault_state.key().as_ref(), &[vault_state.vault_bump]];
let cpi_ctx = CpiContext::new_with_signer(System::id(), cpi_accounts, &[&seeds[..]]);
```

The runtime re-derives the address from those seeds, sees that it matches the
`from` account, and accepts the program's signature on its behalf. `deposit`
needs none of this — the user is a real keypair signing the outer transaction.

### State over time

```
initialize        deposit(1 SOL)      withdraw(0.5 SOL)        close
    │                  │                     │                   │
    ▼                  ▼                     ▼                   ▼
vault_state:      vault_state:          vault_state:        vault_state: closed
  created           unchanged             unchanged           (rent → user)
vault: 0          vault: 1 SOL          vault: 0.5 SOL      vault: 0
                                        + ApplicationAccount
                                          created via CPI
```

---

## 2. The extension: the registration CPI

The stub shipped with `application_account` and `application_program` already
declared on `Withdraw` but unused. The task was to actually make the call.

### How the interface is imported

```rust
declare_program!(registration);
use registration::cpi::{accounts::Initialize, initialize};
```

`declare_program!` reads [`idls/registration.json`](idls/registration.json) at
compile time and generates a typed Rust client — account structs, instruction
builders, the program struct `Q3PreReqsRs` — from the IDL. This is the IDL
acting exactly like an ABI: no source code for the other program is needed.

### The account constraint that does the real work

```rust
#[account(
    mut,
    seeds = [b"prereqs", user.key().as_ref()],
    seeds::program = application_program.key(),
    bump
)]
pub application_account: UncheckedAccount<'info>,
```

`seeds::program` is the important bit. By default Anchor derives PDAs using
*this* program's ID; here the PDA belongs to the registration program, so the
derivation has to be pointed at it. This constraint means a caller cannot pass
in some other account and have it registered — the address is checked before
the instruction body runs.

### The call

```rust
let register_accounts = Initialize {
    user: self.user.to_account_info(),
    account: self.application_account.to_account_info(),
    system_program: self.system_program.to_account_info(),
};

let register_ctx = CpiContext::new(self.application_program.key(), register_accounts);

initialize(register_ctx, GITHUB_USERNAME.to_string())?;
```

Note this is `CpiContext::new`, **not** `new_with_signer`. The registration
program only requires the *user's* signature, and the user already signed the
outer transaction — signatures propagate down through CPI. The vault PDA is not
an authority for registration, so adding its seeds here would be meaningless.

### Why the handle is a constant, not an argument

`GITHUB_USERNAME` lives in [`constants.rs`](programs/pre-req-vault/src/constants.rs).
Two reasons: the provided TypeScript test calls `withdraw(amount)` with a single
argument, so the signature must not change; and baking it in means the on-chain
program itself attests to the handle rather than trusting whatever a caller
passes.

---

## 3. Proof it worked

The `withdraw` transaction logs show the invocation depth, which is what
distinguishes a real CPI from a direct call to the registration program:

```
Program 9PrupsAJcRLDZAS5wCKpJttKBREHFLkCBgauqMjWL4rv invoke [1]   ← my vault program
  Program log: Instruction: Withdraw
  Program 11111111111111111111111111111111 invoke [2]             ← lamport transfer
  Program 11111111111111111111111111111111 success
  Program TRBZyQHB3m68FGeVsqTK39Wm4xejadjVhP5MAZaKWDM invoke [2]  ← the CPI
    Program log: Instruction: Initialize
    Program 11111111111111111111111111111111 invoke [3]           ← rent-exempt alloc
    Program 11111111111111111111111111111111 success
  Program TRBZyQHB3m68FGeVsqTK39Wm4xejadjVhP5MAZaKWDM success
Program 9PrupsAJcRLDZAS5wCKpJttKBREHFLkCBgauqMjWL4rv success
```

`invoke [2]` under my program's `invoke [1]` is the CPI. Total cost: 22,355 CU.

Resulting on-chain account (`node verify-registration.mjs`):

```
ApplicationAccount PDA : 3HzjHaxztQ2qPztzBJXrHhdKB25usg13qZFnRk5yRm4T (bump 254)
owner program          : TRBZyQHB3m68FGeVsqTK39Wm4xejadjVhP5MAZaKWDM
  user                 : 3fTVWVBgm8yYh8XXd7qTBCBuLNP4nMKsCAgesHHCBnA5
  github               : "nisargpatel7042lva"
```

---

## 4. Tests

### TypeScript (devnet) — `anchor test`

```
pre-req-vault
  ✔ Initialize the vault (8496ms)
  ✔  Deposilt 1 Sol in to the vault (7063ms)
  ✔  Withdraw 0.5 Sol from the vault (3054ms)
  ✔  Close the vault and withdraw all the funds (4050ms)

4 passing (23s)
```

### Rust / LiteSVM — `cargo test`

Registration is **one per wallet**, so the devnet run is a single shot. Rather
than burn it on a guess, the whole flow is first proven locally in LiteSVM
against the *real* registration program, dumped from devnet to
`fixtures/registration.so` and loaded into the test VM:

```bash
solana program dump -u d TRBZyQHB3m68FGeVsqTK39Wm4xejadjVhP5MAZaKWDM fixtures/registration.so
```

The test asserts the CPI genuinely created the PDA, that the registration
program owns it, and that the stored `github` string round-trips:

```
initialize ok — signature 4QEUdirzh87m71tqNQgs…
deposit ok — signature 4xHYh88LDqMJLtg8L6Am…
withdraw ok — signature 4nqsWmZSWamfZiCsu7V6…
registration CPI ok — github "nisargpatel7042lva" recorded at C2wx5kbGhJ548kTrte754nA9ChTkd3Qc6tLsyL5UYNfL
close ok — signature 4JNkdV9joMNxrYiGFUDB…
test result: ok. 1 passed; 0 failed
```

---

## 5. Running it yourself

```bash
pnpm install
anchor build
cargo test -- --nocapture              # local LiteSVM, safe to re-run
anchor deploy --provider.cluster devnet
anchor test --skip-build --skip-deploy --provider.cluster devnet
node verify-registration.mjs
```

Change `GITHUB_USERNAME` in `programs/pre-req-vault/src/constants.rs` first, and
note that the devnet `withdraw` will only succeed **once per wallet** — the
second attempt fails because the `ApplicationAccount` PDA already exists.

## Changes from the starting template

- `instructions/withdraw.rs` — implemented the registration CPI.
- `constants.rs` — added `GITHUB_USERNAME`.
- `lib.rs` / `Anchor.toml` — new program ID (`anchor keys sync`) plus a
  `[programs.devnet]` entry and `anchor_version = "1.1.2"` to match `anchor-lang`.
- `tests/test_initialize.rs` — restored from comments and extended to load the
  real registration program and assert on the CPI's result.
- `fixtures/registration.so` — registration program dumped from devnet.
- `docs/` — the architecture diagram, as an Excalidraw source file and a PNG
  export, plus a page that presents it alongside the CPI logs.
- `verify-registration.mjs` — reads the `ApplicationAccount` back off devnet.
- `package.json` / `pnpm-workspace.yaml` — pnpm 11 refuses to run under
  `anchor test` while optional native build scripts are unapproved; this
  silences that gate.

## Toolchain

Rust 1.96 · Solana CLI 3.1.11 (Agave) · Anchor 1.1.2 · anchor-lang 1.1.2 · LiteSVM 0.10 · Node 24
