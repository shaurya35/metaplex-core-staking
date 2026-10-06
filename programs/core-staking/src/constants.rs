use anchor_lang::prelude::*;

#[constant]
pub const CONFIG_SEED: &[u8] = b"config";
#[constant]
pub const REWARDS_MINT_SEED: &[u8] = b"rewards";
#[constant]
pub const STAKE_SEED: &[u8] = b"stake";
#[constant]
pub const ORACLE_SEED: &[u8] = b"oracle";
#[constant]
pub const VAULT_SEED: &[u8] = b"vault";

/// Decimals of the SPL reward token.
pub const REWARD_DECIMALS: u8 = 6;

pub const SECONDS_PER_DAY: i64 = 86_400;
pub const SECONDS_PER_HOUR: i64 = 3_600;

/// Key of the Attribute stored on the Collection that tracks how many assets are staked.
pub const TOTAL_STAKED_KEY: &str = "total_staked";

/// `ExternalCheckResult` flag that lets an Oracle reject a lifecycle event.
pub const CAN_REJECT: u32 = 0b100;
