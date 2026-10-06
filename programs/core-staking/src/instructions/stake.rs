use anchor_lang::prelude::*;
use mpl_core::{
    instructions::AddPluginV1CpiBuilder,
    types::{BurnDelegate, FreezeDelegate, Plugin, PluginAuthority},
};

use crate::{
    constants::*,
    state::{Config, StakeRecord},
    utils::{assert_asset, update_total_staked},
};

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        seeds = [CONFIG_SEED, collection.key().as_ref()],
        bump = config.bump,
        has_one = collection,
    )]
    pub config: Account<'info, Config>,

    /// CHECK: owner / collection verified in handler
    #[account(mut)]
    pub asset: UncheckedAccount<'info>,

    /// CHECK: matched against config.collection
    #[account(mut)]
    pub collection: UncheckedAccount<'info>,

    #[account(
        init,
        payer = owner,
        space = 8 + StakeRecord::INIT_SPACE,
        seeds = [STAKE_SEED, asset.key().as_ref()],
        bump,
    )]
    pub stake_record: Account<'info, StakeRecord>,

    /// CHECK: address constrained to the MPL Core program
    #[account(address = mpl_core::ID)]
    pub mpl_core_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

/// Freezes the asset in the owner's wallet (FreezeDelegate) and pre-approves the config PDA
/// as BurnDelegate so the owner can later burn it for a bonus. Both plugins are owner-managed,
/// so the owner signs to add them; their authority is the config PDA.
pub fn handler(ctx: Context<Stake>) -> Result<()> {
    let a = &ctx.accounts;
    assert_asset(&a.asset, &a.owner.key(), &a.collection.key())?;

    let config_authority = PluginAuthority::Address {
        address: a.config.key(),
    };

    AddPluginV1CpiBuilder::new(&a.mpl_core_program)
        .asset(&a.asset)
        .collection(Some(&a.collection))
        .payer(&a.owner)
        .authority(Some(&a.owner))
        .system_program(&a.system_program)
        .plugin(Plugin::FreezeDelegate(FreezeDelegate { frozen: true }))
        .init_authority(config_authority.clone())
        .invoke()?;

    AddPluginV1CpiBuilder::new(&a.mpl_core_program)
        .asset(&a.asset)
        .collection(Some(&a.collection))
        .payer(&a.owner)
        .authority(Some(&a.owner))
        .system_program(&a.system_program)
        .plugin(Plugin::BurnDelegate(BurnDelegate {}))
        .init_authority(config_authority)
        .invoke()?;

    let collection_key = a.collection.key();
    let seeds: &[&[u8]] = &[CONFIG_SEED, collection_key.as_ref(), &[a.config.bump]];
    update_total_staked(
        &a.mpl_core_program,
        &a.collection,
        &a.owner,
        &a.config.to_account_info(),
        &a.system_program,
        &[seeds],
        1,
    )?;

    let now = Clock::get()?.unix_timestamp;
    ctx.accounts.stake_record.set_inner(StakeRecord {
        owner: ctx.accounts.owner.key(),
        asset: ctx.accounts.asset.key(),
        collection: collection_key,
        staked_at: now,
        last_claimed_at: now,
        reward_remainder: 0,
        bump: ctx.bumps.stake_record,
    });

    emit!(Staked {
        owner: ctx.accounts.owner.key(),
        asset: ctx.accounts.asset.key(),
        staked_at: now,
    });
    Ok(())
}

#[event]
pub struct Staked {
    pub owner: Pubkey,
    pub asset: Pubkey,
    pub staked_at: i64,
}
