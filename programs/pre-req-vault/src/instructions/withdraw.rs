use crate::constants::GITHUB_USERNAME;
use crate::state::VaultState;
use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};

declare_program!(registration);

use registration::cpi::{accounts::Initialize, initialize};

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
    mut,
    seeds = [b"vault", vault_state.key().as_ref()],
    bump = vault_state.vault_bump,
  )]
    pub vault: SystemAccount<'info>,

    #[account(
    seeds = [b"state", user.key().as_ref()],
    bump = vault_state.state_bump
  )]
    pub vault_state: Account<'info, VaultState>,

    /// CHECK: application account will be initialized by the cpi call to the application program
    #[account(
    mut,
    seeds = [b"prereqs", user.key().as_ref()],
    seeds::program = application_program.key(),
    bump
    )]
    pub application_account: UncheckedAccount<'info>,

    pub application_program: Program<'info, registration::program::Q3PreReqsRs>,

    system_program: Program<'info, System>,
}

impl<'info> Withdraw<'info> {
    pub fn withdraw(&mut self, amount: u64) -> Result<()> {
        let cpi_accounts = Transfer {
            from: self.vault.to_account_info(),
            to: self.user.to_account_info(),
        };

        let seeds = &[
            b"vault",
            self.vault_state.to_account_info().key.as_ref(),
            &[self.vault_state.vault_bump],
        ];

        let signer_seeds = &[&seeds[..]];

        let cpi_ctx = CpiContext::new_with_signer(System::id(), cpi_accounts, signer_seeds);

        transfer(cpi_ctx, amount)?;

        // CPI to the registration program to create this user's application account.
        //
        // `user` is already a signer on the outer transaction, and the registration
        // program only requires the user's signature (the `account` it writes to is a
        // PDA it owns and signs for itself). So this is a plain `CpiContext::new` —
        // no `with_signer`, because the vault PDA is not an authority here.
        let register_accounts = Initialize {
            user: self.user.to_account_info(),
            account: self.application_account.to_account_info(),
            system_program: self.system_program.to_account_info(),
        };

        let register_ctx = CpiContext::new(self.application_program.key(), register_accounts);

        initialize(register_ctx, GITHUB_USERNAME.to_string())?;

        Ok(())
    }
}
