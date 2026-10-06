use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};
use mpl_core::{
    instructions::{BurnV1CpiBuilder, UpdatePluginV1CpiBuilder},
    types::{FreezeDelegate, Plugin},
};

use crate::{
    constants::*,
    error::StakingError,
    state::{Config, StakeRecord},
    utils::{assert_asset, mint_rewards, pending_rewards, update_total_staked},
};

#[derive(Accounts)]
pub struct BurnStakedNft<'info> {
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
    #[account(mut)]
    pub asset: UncheckedAccount<'info>,

    /// CHECK: matched against config.collection
    #[account(mut)]
    pub collection: UncheckedAccount<'info>,

    #[account(
        mut,
        close = owner,
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

    /// CHECK: address constrained to the MPL Core program
    #[account(address = mpl_core::ID)]
    pub mpl_core_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// Burn-to-earn: the owner gives up a staked asset for its pending rewards plus a one-time
/// `burn_bonus`. The config PDA burns it through the BurnDelegate approved at stake time.
/// No freeze-period check: burning is a permanent exit, not an early unstake.
pub fn handler(ctx: Context<BurnStakedNft>) -> Result<()> {
    let a = &ctx.accounts;
    assert_asset(&a.asset, &a.owner.key(), &a.collection.key())?;

    let now = Clock::get()?.unix_timestamp;
    let (pending, _) = pending_rewards(
        a.config.rewards_per_day,
        a.stake_record.last_claimed_at,
        now,
        a.stake_record.reward_remainder,
    )?;
    let amount = pending
        .checked_add(a.config.burn_bonus)
        .ok_or(StakingError::Overflow)?;

    let collection_key = a.collection.key();
    let seeds: &[&[u8]] = &[CONFIG_SEED, collection_key.as_ref(), &[a.config.bump]];
    let config_info = a.config.to_account_info();

    mint_rewards(
        &a.token_program,
        &a.rewards_mint.to_account_info(),
        &a.owner_ata.to_account_info(),
        &config_info,
        &[seeds],
        amount,
    )?;

    // A frozen asset cannot be burned, so thaw it first.
    UpdatePluginV1CpiBuilder::new(&a.mpl_core_program)
        .asset(&a.asset)
        .collection(Some(&a.collection))
        .payer(&a.owner)
        .authority(Some(&config_info))
        .system_program(&a.system_program)
        .plugin(Plugin::FreezeDelegate(FreezeDelegate { frozen: false }))
        .invoke_signed(&[seeds])?;

    BurnV1CpiBuilder::new(&a.mpl_core_program)
        .asset(&a.asset)
        .collection(Some(&a.collection))
        .payer(&a.owner)
        .authority(Some(&config_info))
        .system_program(Some(&a.system_program))
        .invoke_signed(&[seeds])?;

    update_total_staked(
        &a.mpl_core_program,
        &a.collection,
        &a.owner,
        &config_info,
        &a.system_program,
        &[seeds],
        -1,
    )?;

    emit!(StakedNftBurned {
        owner: a.owner.key(),
        asset: a.asset.key(),
        rewards: pending,
        bonus: a.config.burn_bonus,
    });
    Ok(())
}

#[event]
pub struct StakedNftBurned {
    pub owner: Pubkey,
    pub asset: Pubkey,
    pub rewards: u64,
    pub bonus: u64,
}
