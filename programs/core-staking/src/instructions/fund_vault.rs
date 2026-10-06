use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};

use crate::{constants::*, state::Config, utils::assert_vault_deposit_ok};

#[derive(Accounts)]
pub struct FundVault<'info> {
    #[account(mut)]
    pub funder: Signer<'info>,

    #[account(
        seeds = [CONFIG_SEED, config.collection.as_ref()],
        bump = config.bump,
    )]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        seeds = [VAULT_SEED, config.collection.as_ref()],
        bump = config.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    pub system_program: Program<'info, System>,
}

/// Anyone can top up the lamport vault that pays oracle crankers.
pub fn handler(ctx: Context<FundVault>, amount: u64) -> Result<()> {
    assert_vault_deposit_ok(ctx.accounts.vault.lamports(), amount)?;
    transfer(
        CpiContext::new(
            ctx.accounts.system_program.key(),
            Transfer {
                from: ctx.accounts.funder.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
            },
        ),
        amount,
    )
}
