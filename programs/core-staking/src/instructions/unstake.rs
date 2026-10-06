use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};
use mpl_core::{
    instructions::{RemovePluginV1CpiBuilder, UpdatePluginV1CpiBuilder},
    types::{FreezeDelegate, Plugin, PluginType},
};

use crate::{
    constants::*,
    error::StakingError,
    state::{Config, StakeRecord},
    utils::{assert_asset, mint_rewards, pending_rewards, update_total_staked},
};

#[derive(Accounts)]
pub struct Unstake<'info> {
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

/// After the freeze period: pays out pending rewards, thaws the asset, removes the
/// staking plugins and decrements the collection's `total_staked`.
pub fn handler(ctx: Context<Unstake>) -> Result<()> {
    let a = &ctx.accounts;
    assert_asset(&a.asset, &a.owner.key(), &a.collection.key())?;

    let now = Clock::get()?.unix_timestamp;
    let unlock_at = a
        .stake_record
        .staked_at
        .checked_add(a.config.freeze_period)
        .ok_or(StakingError::Overflow)?;
    require!(now >= unlock_at, StakingError::FreezePeriodNotOver);

    let collection_key = a.collection.key();
    let seeds: &[&[u8]] = &[CONFIG_SEED, collection_key.as_ref(), &[a.config.bump]];
    let config_info = a.config.to_account_info();

    let amount = pending_rewards(
        a.config.rewards_per_day,
        a.stake_record.last_claimed_at,
        now,
    )?;
    mint_rewards(
        &a.token_program,
        &a.rewards_mint.to_account_info(),
        &a.owner_ata.to_account_info(),
        &config_info,
        &[seeds],
        amount,
    )?;

    // Config PDA is the FreezeDelegate authority: thaw first, otherwise removal is rejected.
    UpdatePluginV1CpiBuilder::new(&a.mpl_core_program)
        .asset(&a.asset)
        .collection(Some(&a.collection))
        .payer(&a.owner)
        .authority(Some(&config_info))
        .system_program(&a.system_program)
        .plugin(Plugin::FreezeDelegate(FreezeDelegate { frozen: false }))
        .invoke_signed(&[seeds])?;

    for plugin_type in [PluginType::FreezeDelegate, PluginType::BurnDelegate] {
        RemovePluginV1CpiBuilder::new(&a.mpl_core_program)
            .asset(&a.asset)
            .collection(Some(&a.collection))
            .payer(&a.owner)
            .authority(Some(&a.owner))
            .system_program(&a.system_program)
            .plugin_type(plugin_type)
            .invoke()?;
    }

    update_total_staked(
        &a.mpl_core_program,
        &a.collection,
        &a.owner,
        &config_info,
        &a.system_program,
        &[seeds],
        -1,
    )?;

    emit!(Unstaked {
        owner: a.owner.key(),
        asset: a.asset.key(),
        rewards: amount,
    });
    Ok(())
}

#[event]
pub struct Unstaked {
    pub owner: Pubkey,
    pub asset: Pubkey,
    pub rewards: u64,
}
