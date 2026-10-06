use anchor_lang::prelude::*;

/// Per-collection staking configuration.
///
/// The config PDA is the Collection's update authority and the authority of the
/// FreezeDelegate / BurnDelegate plugins added to staked assets.
#[account]
#[derive(InitSpace)]
pub struct Config {
    pub admin: Pubkey,
    pub collection: Pubkey,
    pub rewards_mint: Pubkey,
    /// Reward tokens (base units) earned per staked asset per day.
    pub rewards_per_day: u64,
    /// Minimum seconds an asset must stay staked before it can be unstaked.
    pub freeze_period: i64,
    /// One-time bonus (base units) paid when a staked asset is burned.
    pub burn_bonus: u64,
    /// Lamports paid to whoever cranks the oracle near an open/close boundary.
    pub crank_reward: u64,
    /// How close (in seconds) to a boundary a crank must be to earn the reward.
    pub crank_window: i64,
    /// UTC hour transfers open (inclusive).
    pub open_hour: u8,
    /// UTC hour transfers close (exclusive).
    pub close_hour: u8,
    pub bump: u8,
    pub rewards_mint_bump: u8,
    pub oracle_bump: u8,
    pub vault_bump: u8,
}
