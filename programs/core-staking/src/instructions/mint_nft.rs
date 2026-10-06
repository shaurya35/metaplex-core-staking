use anchor_lang::prelude::*;
use mpl_core::instructions::CreateV2CpiBuilder;

use crate::{constants::*, state::Config, utils::assert_collection};

#[derive(Accounts)]
pub struct MintNft<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    /// CHECK: any wallet can receive the NFT
    pub owner: UncheckedAccount<'info>,

    /// New MPL Core asset.
    #[account(mut)]
    pub asset: Signer<'info>,

    /// CHECK: matched against config.collection, owner checked in handler
    #[account(mut)]
    pub collection: UncheckedAccount<'info>,

    #[account(
        seeds = [CONFIG_SEED, collection.key().as_ref()],
        bump = config.bump,
        has_one = admin,
        has_one = collection,
    )]
    pub config: Account<'info, Config>,

    /// CHECK: address constrained to the MPL Core program
    #[account(address = mpl_core::ID)]
    pub mpl_core_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

/// Admin mints a new asset into the collection for `owner`. The config PDA signs as the
/// collection's update authority.
pub fn handler(ctx: Context<MintNft>, name: String, uri: String) -> Result<()> {
    let a = &ctx.accounts;
    assert_collection(&a.collection)?;

    let collection_key = a.collection.key();
    let seeds: &[&[u8]] = &[CONFIG_SEED, collection_key.as_ref(), &[a.config.bump]];

    CreateV2CpiBuilder::new(&a.mpl_core_program)
        .asset(&a.asset)
        .collection(Some(&a.collection))
        .authority(Some(&a.config.to_account_info()))
        .payer(&a.admin)
        .owner(Some(&a.owner))
        .system_program(&a.system_program)
        .name(name)
        .uri(uri)
        .invoke_signed(&[seeds])?;
    Ok(())
}
