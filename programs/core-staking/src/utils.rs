use anchor_lang::prelude::*;
use anchor_spl::token::{self, MintTo};
use mpl_core::{
    accounts::{BaseAssetV1, BaseCollectionV1},
    fetch_plugin,
    instructions::UpdateCollectionPluginV1CpiBuilder,
    types::{Attribute, Attributes, Plugin, PluginType, UpdateAuthority},
};

use crate::{constants::*, error::StakingError};

// ---------------------------------------------------------------------------
// Pure helpers (unit tested below)
// ---------------------------------------------------------------------------

/// Rewards accrued between `from` and `to` at `rewards_per_day`, pro-rated per second.
///
/// `remainder` is the sub-unit leftover (numerator over `SECONDS_PER_DAY`) carried from the
/// previous payout. Returns `(amount, new_remainder)`, so many small claims add up to
/// exactly what one large claim would pay.
pub fn pending_rewards(
    rewards_per_day: u64,
    from: i64,
    to: i64,
    remainder: u64,
) -> Result<(u64, u64)> {
    let elapsed = to.saturating_sub(from).max(0) as u128;
    let accrued = elapsed
        .checked_mul(rewards_per_day as u128)
        .and_then(|v| v.checked_add(remainder as u128))
        .ok_or(StakingError::Overflow)?;
    let amount =
        u64::try_from(accrued / SECONDS_PER_DAY as u128).map_err(|_| StakingError::Overflow)?;
    Ok((amount, (accrued % SECONDS_PER_DAY as u128) as u64))
}

/// Seconds since 00:00 UTC for a unix timestamp.
pub fn seconds_of_day(unix_ts: i64) -> i64 {
    unix_ts.rem_euclid(SECONDS_PER_DAY)
}

/// True when `unix_ts` falls in `[open_hour, close_hour)` UTC.
pub fn is_transfer_window_open(unix_ts: i64, open_hour: u8, close_hour: u8) -> bool {
    let hour = seconds_of_day(unix_ts) / SECONDS_PER_HOUR;
    hour >= open_hour as i64 && hour < close_hour as i64
}

/// True when `unix_ts` is within `window` seconds of the open or close boundary
/// (distance is measured around the 24h clock, so 23:59 is 1 minute from 00:00).
pub fn is_near_boundary(unix_ts: i64, open_hour: u8, close_hour: u8, window: i64) -> bool {
    let now = seconds_of_day(unix_ts);
    [open_hour, close_hour].iter().any(|&h| {
        let boundary = (h as i64 * SECONDS_PER_HOUR) % SECONDS_PER_DAY;
        let diff = (now - boundary).abs();
        diff.min(SECONDS_PER_DAY - diff) <= window
    })
}

/// Returns `attributes` with `total_staked` moved by `delta`.
pub fn apply_staked_delta(mut attributes: Vec<Attribute>, delta: i64) -> Result<Vec<Attribute>> {
    let attr = attributes
        .iter_mut()
        .find(|a| a.key == TOTAL_STAKED_KEY)
        .ok_or(StakingError::MissingStakedAttribute)?;
    let current: u64 = attr
        .value
        .parse()
        .map_err(|_| StakingError::MissingStakedAttribute)?;
    let next = if delta >= 0 {
        current
            .checked_add(delta as u64)
            .ok_or(StakingError::Overflow)?
    } else {
        current
            .checked_sub(delta.unsigned_abs())
            .ok_or(StakingError::CounterUnderflow)?
    };
    attr.value = next.to_string();
    Ok(attributes)
}

// ---------------------------------------------------------------------------
// MPL Core account checks
// ---------------------------------------------------------------------------

/// Deserializes an MPL Core asset and checks it is owned by `owner` and belongs to `collection`.
pub fn assert_asset(asset: &AccountInfo, owner: &Pubkey, collection: &Pubkey) -> Result<()> {
    require_keys_eq!(*asset.owner, mpl_core::ID, StakingError::InvalidCoreAccount);
    let base = BaseAssetV1::from_bytes(&asset.data.borrow())
        .map_err(|_| StakingError::InvalidCoreAccount)?;
    require!(
        base.update_authority == UpdateAuthority::Collection(*collection),
        StakingError::WrongCollection
    );
    require_keys_eq!(base.owner, *owner, StakingError::NotOwner);
    Ok(())
}

/// Checks that `collection` is an MPL Core collection owned by the core program.
pub fn assert_collection(collection: &AccountInfo) -> Result<()> {
    require_keys_eq!(
        *collection.owner,
        mpl_core::ID,
        StakingError::InvalidCoreAccount
    );
    BaseCollectionV1::from_bytes(&collection.data.borrow())
        .map_err(|_| StakingError::InvalidCoreAccount)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// CPI helpers
// ---------------------------------------------------------------------------

/// Bumps the `total_staked` Attribute on the Collection by `delta` (config PDA signs as
/// the collection's update authority).
#[allow(clippy::too_many_arguments)]
pub fn update_total_staked<'info>(
    mpl_core_program: &AccountInfo<'info>,
    collection: &AccountInfo<'info>,
    payer: &AccountInfo<'info>,
    config: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    signer_seeds: &[&[&[u8]]],
    delta: i64,
) -> Result<()> {
    let (_, attributes, _) =
        fetch_plugin::<BaseCollectionV1, Attributes>(collection, PluginType::Attributes)
            .map_err(|_| StakingError::MissingStakedAttribute)?;
    let attribute_list = apply_staked_delta(attributes.attribute_list, delta)?;

    UpdateCollectionPluginV1CpiBuilder::new(mpl_core_program)
        .collection(collection)
        .payer(payer)
        .authority(Some(config))
        .system_program(system_program)
        .plugin(Plugin::Attributes(Attributes { attribute_list }))
        .invoke_signed(signer_seeds)?;
    Ok(())
}

/// Mints `amount` reward tokens to `to` with the config PDA as mint authority.
pub fn mint_rewards<'info>(
    token_program: &AccountInfo<'info>,
    mint: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    config: &AccountInfo<'info>,
    signer_seeds: &[&[&[u8]]],
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    token::mint_to(
        CpiContext::new_with_signer(
            token_program.key(),
            MintTo {
                mint: mint.clone(),
                to: to.clone(),
                authority: config.clone(),
            },
            signer_seeds,
        ),
        amount,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = SECONDS_PER_HOUR;
    // 2026-10-06 00:00:00 UTC
    const DAY0: i64 = 1_791_244_800;

    #[test]
    fn rewards_prorate_per_second() {
        assert_eq!(pending_rewards(86_400, 0, 1, 0).unwrap(), (1, 0));
        assert_eq!(
            pending_rewards(1_000_000, 0, SECONDS_PER_DAY, 0).unwrap(),
            (1_000_000, 0)
        );
        assert_eq!(
            pending_rewards(1_000_000, 0, SECONDS_PER_DAY / 2, 0).unwrap(),
            (500_000, 0)
        );
        assert_eq!(pending_rewards(1_000_000, 100, 50, 0).unwrap(), (0, 0));
        assert!(pending_rewards(u64::MAX, 0, i64::MAX, 0).is_err());
    }

    #[test]
    fn rewards_carry_remainder() {
        // 1.5 units/s: two 1s claims pay 1 then 2 (carrying the half), same as one 2s claim.
        let rate = 3 * SECONDS_PER_DAY as u64 / 2;
        let (a1, r1) = pending_rewards(rate, 0, 1, 0).unwrap();
        assert_eq!((a1, r1), (1, 43_200));
        let (a2, r2) = pending_rewards(rate, 1, 2, r1).unwrap();
        assert_eq!((a2, r2), (2, 0));
        assert_eq!(a1 + a2, pending_rewards(rate, 0, 2, 0).unwrap().0);
    }

    #[test]
    fn window_open_between_hours() {
        assert_eq!(seconds_of_day(DAY0), 0);
        assert!(!is_transfer_window_open(DAY0 + 8 * H + 3599, 9, 17));
        assert!(is_transfer_window_open(DAY0 + 9 * H, 9, 17));
        assert!(is_transfer_window_open(DAY0 + 16 * H + 3599, 9, 17));
        assert!(!is_transfer_window_open(DAY0 + 17 * H, 9, 17));
        assert!(!is_transfer_window_open(DAY0 + 23 * H, 9, 17));
    }

    #[test]
    fn boundary_detection() {
        let w = 300;
        assert!(is_near_boundary(DAY0 + 9 * H + 120, 9, 17, w));
        assert!(is_near_boundary(DAY0 + 9 * H - 300, 9, 17, w));
        assert!(is_near_boundary(DAY0 + 17 * H + 299, 9, 17, w));
        assert!(!is_near_boundary(DAY0 + 12 * H, 9, 17, w));
        assert!(!is_near_boundary(DAY0 + 9 * H + 301, 9, 17, w));
        // wrap around midnight when close_hour is 24
        assert!(is_near_boundary(DAY0 + 60, 9, 24, w));
        assert!(is_near_boundary(DAY0 - 60, 9, 24, w));
    }

    #[test]
    fn staked_counter_delta() {
        let attrs = vec![
            Attribute {
                key: "other".into(),
                value: "x".into(),
            },
            Attribute {
                key: TOTAL_STAKED_KEY.into(),
                value: "1".into(),
            },
        ];
        let up = apply_staked_delta(attrs, 1).unwrap();
        assert_eq!(up[1].value, "2");
        assert_eq!(up[0].value, "x");
        let down = apply_staked_delta(up, -2).unwrap();
        assert_eq!(down[1].value, "0");
        assert!(apply_staked_delta(down, -1).is_err());
        assert!(apply_staked_delta(vec![], 1).is_err());
    }
}
