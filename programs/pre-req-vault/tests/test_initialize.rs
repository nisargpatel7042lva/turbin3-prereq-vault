//! End-to-end LiteSVM test for the vault: initialize -> deposit -> withdraw -> close.
//!
//! The `withdraw` instruction performs a CPI into the Turbin3 registration
//! program, so this test loads the *real* registration program (dumped from
//! devnet into `fixtures/registration.so`) and asserts that the CPI actually
//! created the `ApplicationAccount` PDA with the expected GitHub handle.
//!
//! Running this locally matters: registration is one-per-wallet on devnet, so
//! the whole flow is proven here against a throwaway keypair before the real
//! devnet run is spent.

use {
    anchor_lang::{
        solana_program::instruction::Instruction, system_program::ID as SYSTEM_PROGRAM_ID,
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    litesvm::LiteSVM,
    solana_keypair::Keypair,
    solana_message::Message,
    solana_pubkey::{pubkey, Pubkey},
    solana_signer::Signer,
    solana_transaction::Transaction,
};

/// The Turbin3 registration ("prereqs") program on devnet.
const REGISTRATION_PROGRAM_ID: Pubkey = pubkey!("TRBZyQHB3m68FGeVsqTK39Wm4xejadjVhP5MAZaKWDM");

/// Must match `pre_req_vault::constants::GITHUB_USERNAME`.
const EXPECTED_GITHUB: &str = "nisargpatel7042lva";

/// Minimal borsh reader for the registration program's `ApplicationAccount`.
///
/// Layout: 8-byte discriminator | user: Pubkey | bump: u8 | pre_req_ts: bool
///         | pre_req_rs: bool | github: String
struct ApplicationAccount {
    user: Pubkey,
    pre_req_ts: bool,
    pre_req_rs: bool,
    github: String,
}

impl ApplicationAccount {
    fn unpack(data: &[u8]) -> Self {
        let user = Pubkey::try_from(&data[8..40]).unwrap();
        let pre_req_ts = data[41] != 0;
        let pre_req_rs = data[42] != 0;
        let len = u32::from_le_bytes(data[43..47].try_into().unwrap()) as usize;
        let github = String::from_utf8(data[47..47 + len].to_vec()).unwrap();

        Self {
            user,
            pre_req_ts,
            pre_req_rs,
            github,
        }
    }
}

fn setup() -> (LiteSVM, Keypair) {
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();

    svm.add_program(
        pre_req_vault::id(),
        include_bytes!("../../../target/deploy/pre_req_vault.so"),
    )
    .unwrap();

    // The real registration program, so the CPI is exercised for real.
    svm.add_program(
        REGISTRATION_PROGRAM_ID,
        include_bytes!("../../../fixtures/registration.so"),
    )
    .unwrap();

    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();

    (svm, payer)
}

#[test]
fn test_initialize_deposit_withdraw_close() {
    let (mut svm, payer) = setup();
    let user = payer.pubkey();

    let (vault_state_pda, state_bump) =
        Pubkey::find_program_address(&[b"state", user.as_ref()], &pre_req_vault::id());

    let (vault_pda, vault_bump) =
        Pubkey::find_program_address(&[b"vault", vault_state_pda.as_ref()], &pre_req_vault::id());

    let (application_account, _) =
        Pubkey::find_program_address(&[b"prereqs", user.as_ref()], &REGISTRATION_PROGRAM_ID);

    let send = |svm: &mut LiteSVM, ix: Instruction, label: &str| {
        let message = Message::new(&[ix], Some(&user));
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new(&[&payer], message, blockhash);
        let res = svm
            .send_transaction(tx)
            .unwrap_or_else(|e| panic!("{label} failed: {e:?}"));
        println!("{label} ok — signature {}", res.signature);
    };

    // ---- initialize ----------------------------------------------------
    send(
        &mut svm,
        Instruction {
            program_id: pre_req_vault::id(),
            accounts: pre_req_vault::accounts::Initialize {
                user,
                vault_state: vault_state_pda,
                vault: vault_pda,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: pre_req_vault::instruction::Initialize {}.data(),
        },
        "initialize",
    );

    let vault_state_account = svm.get_account(&vault_state_pda).unwrap();
    let vault_state =
        pre_req_vault::state::VaultState::try_deserialize(&mut vault_state_account.data.as_ref())
            .unwrap();

    assert_eq!(vault_state.vault_bump, vault_bump);
    assert_eq!(vault_state.state_bump, state_bump);

    // ---- deposit 1 SOL -------------------------------------------------
    let deposit_amount: u64 = 1_000_000_000;

    send(
        &mut svm,
        Instruction {
            program_id: pre_req_vault::id(),
            accounts: pre_req_vault::accounts::Deposit {
                user,
                vault_state: vault_state_pda,
                vault: vault_pda,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: pre_req_vault::instruction::Deposit {
                amount: deposit_amount,
            }
            .data(),
        },
        "deposit",
    );

    assert_eq!(svm.get_balance(&vault_pda).unwrap(), deposit_amount);

    // ---- withdraw 0.5 SOL (+ registration CPI) -------------------------
    assert!(
        svm.get_account(&application_account)
            .is_none_or(|a| a.data.is_empty()),
        "application account must not exist before withdraw"
    );

    let withdraw_amount: u64 = 500_000_000;

    send(
        &mut svm,
        Instruction {
            program_id: pre_req_vault::id(),
            accounts: pre_req_vault::accounts::Withdraw {
                user,
                vault_state: vault_state_pda,
                vault: vault_pda,
                application_account,
                application_program: REGISTRATION_PROGRAM_ID,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: pre_req_vault::instruction::Withdraw {
                amount: withdraw_amount,
            }
            .data(),
        },
        "withdraw",
    );

    assert_eq!(
        svm.get_balance(&vault_pda).unwrap(),
        deposit_amount - withdraw_amount
    );

    // The CPI must have created and populated the registration PDA.
    let registered = svm
        .get_account(&application_account)
        .expect("registration CPI did not create the application account");

    assert_eq!(
        registered.owner, REGISTRATION_PROGRAM_ID,
        "application account should be owned by the registration program"
    );

    let application = ApplicationAccount::unpack(&registered.data);
    assert_eq!(application.user, user);
    assert_eq!(application.github, EXPECTED_GITHUB);
    assert!(!application.pre_req_ts);
    assert!(!application.pre_req_rs);

    println!(
        "registration CPI ok — github \"{}\" recorded at {}",
        application.github, application_account
    );

    // ---- close ---------------------------------------------------------
    let user_balance_before_close = svm.get_balance(&user).unwrap();

    send(
        &mut svm,
        Instruction {
            program_id: pre_req_vault::id(),
            accounts: pre_req_vault::accounts::Close {
                user,
                vault_state: vault_state_pda,
                vault: vault_pda,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: pre_req_vault::instruction::Close {}.data(),
        },
        "close",
    );

    assert_eq!(svm.get_balance(&vault_pda).unwrap_or(0), 0);
    assert!(svm
        .get_account(&vault_state_pda)
        .is_none_or(|a| a.data.is_empty()));
    assert!(svm.get_balance(&user).unwrap() > user_balance_before_close);
}
