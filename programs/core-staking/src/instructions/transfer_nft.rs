use anchor_lang::prelude::*;
use mpl_core::instructions::TransferV1CpiBuilder;

use crate::{
    constants::*,
    state::{Config, Oracle},
    utils::assert_asset,
};

#[derive(Accounts)]
pub struct TransferNft<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    /// CHECK: any wallet can receive the NFT
    pub new_owner: UncheckedAccount<'info>,

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
        seeds = [ORACLE_SEED, collection.key().as_ref()],
        bump = config.oracle_bump,
    )]
    pub oracle: Account<'info, Oracle>,

    /// CHECK: address constrained to the MPL Core program
    #[account(address = mpl_core::ID)]
    pub mpl_core_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

/// Transfers the asset. MPL Core runs the collection's Oracle adapter, which reads the
/// oracle PDA (passed as a remaining account) and rejects the transfer outside trading hours.
/// A staked asset is also rejected because its FreezeDelegate is frozen.
pub fn handler(ctx: Context<TransferNft>) -> Result<()> {
    let a = &ctx.accounts;
    assert_asset(&a.asset, &a.owner.key(), &a.collection.key())?;

    let oracle_info = a.oracle.to_account_info();
    TransferV1CpiBuilder::new(&a.mpl_core_program)
        .asset(&a.asset)
        .collection(Some(&a.collection))
        .payer(&a.owner)
        .authority(Some(&a.owner))
        .new_owner(&a.new_owner)
        .system_program(Some(&a.system_program))
        .add_remaining_account(&oracle_info, false, false)
        .invoke()?;
    Ok(())
}
