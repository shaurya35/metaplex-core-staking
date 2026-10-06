use anchor_lang::prelude::*;

/// Created on stake, closed on unstake / burn.
#[account]
#[derive(InitSpace)]
pub struct StakeRecord {
    pub owner: Pubkey,
    pub asset: Pubkey,
    pub collection: Pubkey,
    pub staked_at: i64,
    pub last_claimed_at: i64,
    pub bump: u8,
}
