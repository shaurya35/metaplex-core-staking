#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;

pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;
pub mod utils;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("ATwkeErS3hJ6rfHNN4ZZax6UtWExuYTE2TcJdkMvnfnt");

#[program]
pub mod core_staking {
    use super::*;

    /// Creates the MPL Core collection (with `total_staked` attribute + Oracle adapter),
    /// the config PDA, reward mint, oracle account and crank vault.
    pub fn initialize(ctx: Context<Initialize>, args: InitializeArgs) -> Result<()> {
        initialize::handler(ctx, args)
    }

    /// Tops up the lamport vault that pays oracle crankers.
    pub fn fund_vault(ctx: Context<FundVault>, amount: u64) -> Result<()> {
        fund_vault::handler(ctx, amount)
    }

    /// Admin mints an asset into the collection.
    pub fn mint_nft(ctx: Context<MintNft>, name: String, uri: String) -> Result<()> {
        mint_nft::handler(ctx, name, uri)
    }

    /// Freezes the asset in place and starts accruing rewards.
    pub fn stake(ctx: Context<Stake>) -> Result<()> {
        stake::handler(ctx)
    }

    /// Mints accrued rewards; the asset stays staked and frozen.
    pub fn claim_rewards(ctx: Context<ClaimRewards>) -> Result<()> {
        claim_rewards::handler(ctx)
    }

    /// Pays pending rewards, thaws the asset and removes the staking plugins.
    pub fn unstake(ctx: Context<Unstake>) -> Result<()> {
        unstake::handler(ctx)
    }

    /// Burns a staked asset through its BurnDelegate for pending rewards + a bonus.
    pub fn burn_staked_nft(ctx: Context<BurnStakedNft>) -> Result<()> {
        burn_staked_nft::handler(ctx)
    }

    /// Permissionless crank that syncs the Oracle's Transfer validation with the clock.
    pub fn update_oracle(ctx: Context<UpdateOracle>) -> Result<()> {
        update_oracle::handler(ctx)
    }

    /// Transfers an asset, passing the oracle PDA so the Oracle plugin can validate it.
    pub fn transfer_nft(ctx: Context<TransferNft>) -> Result<()> {
        transfer_nft::handler(ctx)
    }
}
