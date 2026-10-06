use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};

use crate::{
    constants::*,
    error::StakingError,
    state::{Config, ExternalValidationResult, Oracle, OracleValidation},
    utils::{is_near_boundary, is_transfer_window_open},
};

#[derive(Accounts)]
pub struct UpdateOracle<'info> {
    /// Anyone can crank.
    #[account(mut)]
    pub cranker: Signer<'info>,

    #[account(
        seeds = [CONFIG_SEED, config.collection.as_ref()],
        bump = config.bump,
    )]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        seeds = [ORACLE_SEED, config.collection.as_ref()],
        bump = config.oracle_bump,
    )]
    pub oracle: Account<'info, Oracle>,

    #[account(
        mut,
        seeds = [VAULT_SEED, config.collection.as_ref()],
        bump = config.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    pub system_program: Program<'info, System>,
}

/// Permissionless crank: sets the Transfer validation to `Pass` inside trading hours and
/// `Rejected` outside them. Only a real state change is accepted, and only a change made
/// within `crank_window` seconds of the open/close boundary is paid from the vault.
pub fn handler(ctx: Context<UpdateOracle>) -> Result<()> {
    let config = &ctx.accounts.config;
    let now = Clock::get()?.unix_timestamp;

    let open = is_transfer_window_open(now, config.open_hour, config.close_hour);
    let desired = if open {
        ExternalValidationResult::Pass
    } else {
        ExternalValidationResult::Rejected
    };
    require!(
        ctx.accounts.oracle.validation.transfer() != Some(desired),
        StakingError::OracleAlreadyUpToDate
    );
    ctx.accounts.oracle.validation = OracleValidation::for_transfer(desired);

    let mut reward = 0;
    if config.crank_reward > 0
        && is_near_boundary(
            now,
            config.open_hour,
            config.close_hour,
            config.crank_window,
        )
    {
        // Never drain the vault below rent exemption; skip the reward if it is underfunded.
        let rent_floor = Rent::get()?.minimum_balance(0);
        let available = ctx.accounts.vault.lamports().saturating_sub(rent_floor);
        if available >= config.crank_reward {
            let collection = config.collection;
            let seeds: &[&[u8]] = &[VAULT_SEED, collection.as_ref(), &[config.vault_bump]];
            transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.system_program.key(),
                    Transfer {
                        from: ctx.accounts.vault.to_account_info(),
                        to: ctx.accounts.cranker.to_account_info(),
                    },
                    &[seeds],
                ),
                config.crank_reward,
            )?;
            reward = config.crank_reward;
        }
    }

    emit!(OracleUpdated {
        cranker: ctx.accounts.cranker.key(),
        transfers_open: open,
        reward,
        timestamp: now,
    });
    Ok(())
}

#[event]
pub struct OracleUpdated {
    pub cranker: Pubkey,
    pub transfers_open: bool,
    pub reward: u64,
    pub timestamp: i64,
}
