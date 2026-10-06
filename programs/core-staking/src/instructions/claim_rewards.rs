use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};

use crate::{
    constants::*,
    error::StakingError,
    state::{Config, StakeRecord},
    utils::{assert_asset, mint_rewards, pending_rewards},
};

#[derive(Accounts)]
pub struct ClaimRewards<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        seeds = [CONFIG_SEED, collection.key().as_ref()],
        bump = config.bump,
        has_one = collection,
        has_one = rewards_mint,
    )]
    pub config: Account<'info, Config>,

    /// CHECK: owner / collection verified in handler
    pub asset: UncheckedAccount<'info>,

    /// CHECK: matched against config.collection
    pub collection: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [STAKE_SEED, asset.key().as_ref()],
        bump = stake_record.bump,
        has_one = owner,
        has_one = asset,
        has_one = collection,
    )]
    pub stake_record: Account<'info, StakeRecord>,

    #[account(
        mut,
        seeds = [REWARDS_MINT_SEED, config.key().as_ref()],
        bump = config.rewards_mint_bump,
    )]
    pub rewards_mint: Account<'info, Mint>,

    #[account(
        init_if_needed,
        payer = owner,
        associated_token::mint = rewards_mint,
        associated_token::authority = owner,
        associated_token::token_program = token_program,
    )]
    pub owner_ata: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// Mints accrued rewards without touching the asset: it stays staked and frozen.
pub fn handler(ctx: Context<ClaimRewards>) -> Result<()> {
    let a = &ctx.accounts;
    assert_asset(&a.asset, &a.owner.key(), &a.collection.key())?;

    let now = Clock::get()?.unix_timestamp;
    let (amount, remainder) = pending_rewards(
        a.config.rewards_per_day,
        a.stake_record.last_claimed_at,
        now,
        a.stake_record.reward_remainder,
    )?;
    require!(amount > 0, StakingError::NothingToClaim);

    let collection_key = a.collection.key();
    let seeds: &[&[u8]] = &[CONFIG_SEED, collection_key.as_ref(), &[a.config.bump]];
    mint_rewards(
        &a.token_program,
        &a.rewards_mint.to_account_info(),
        &a.owner_ata.to_account_info(),
        &a.config.to_account_info(),
        &[seeds],
        amount,
    )?;

    ctx.accounts.stake_record.last_claimed_at = now;
    ctx.accounts.stake_record.reward_remainder = remainder;

    emit!(RewardsClaimed {
        owner: ctx.accounts.owner.key(),
        asset: ctx.accounts.asset.key(),
        amount,
    });
    Ok(())
}

#[event]
pub struct RewardsClaimed {
    pub owner: Pubkey,
    pub asset: Pubkey,
    pub amount: u64,
}
