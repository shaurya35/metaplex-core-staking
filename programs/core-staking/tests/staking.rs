//! Task 1: stake / claim_rewards / unstake / burn_staked_nft and the collection-level
//! `total_staked` attribute, run against the real MPL Core program in LiteSVM.

mod common;

use common::*;
use mpl_core::{
    types::{Key, UpdateAuthority},
    AuthorityType,
};
use solana_keypair::Keypair;
use solana_signer::Signer;

#[test]
fn initialize_creates_collection_with_stats_and_oracle() {
    let env = Env::new();
    let col = env.collection_state();

    assert_eq!(
        col.base.update_authority, env.config,
        "config PDA is update authority"
    );
    assert_eq!(env.total_staked(), 0);

    let oracles = &col.external_plugin_adapter_list.oracles;
    assert_eq!(oracles.len(), 1, "one Oracle adapter on the collection");
    assert_eq!(oracles[0].base_address, env.oracle);

    // Config + vault funded
    assert!(env.lamports(&env.vault) >= VAULT_FUNDING);
    let cfg = env.svm.get_account(&env.config).unwrap();
    assert_eq!(cfg.owner, core_staking::ID);
}

#[test]
fn admin_mints_into_collection_and_non_admin_cannot() {
    let mut env = Env::new();
    let alice = env.user();
    let asset = env.mint_nft(&alice.pubkey());

    let a = env.asset(&asset);
    assert_eq!(a.base.owner, alice.pubkey());
    assert_eq!(
        a.base.update_authority,
        UpdateAuthority::Collection(env.collection)
    );

    let mallory = env.user();
    let res = env.try_mint_nft(&mallory, &mallory.pubkey(), &Keypair::new());
    assert_fails_with(res, "ConstraintHasOne");
}

#[test]
fn stake_freezes_asset_and_increments_total_staked() {
    let mut env = Env::new();
    let alice = env.user();
    let asset = env.mint_nft(&alice.pubkey());

    env.stake(&alice, &asset).expect("stake");

    let a = env.asset(&asset);
    let freeze = a.plugin_list.freeze_delegate.expect("FreezeDelegate added");
    assert!(freeze.freeze_delegate.frozen);
    assert_eq!(freeze.base.authority.authority_type, AuthorityType::Address);
    assert_eq!(freeze.base.authority.address, Some(env.config));

    let burn = a.plugin_list.burn_delegate.expect("BurnDelegate added");
    assert_eq!(burn.base.authority.address, Some(env.config));

    assert_eq!(
        a.base.owner,
        alice.pubkey(),
        "asset stays in the owner's wallet"
    );
    assert_eq!(env.total_staked(), 1);

    let rec = env.stake_record(&asset).unwrap();
    assert_eq!(rec.owner, alice.pubkey());
    assert_eq!(rec.staked_at, env.now());

    // second stake of the same asset fails (stake record already exists)
    assert!(env.stake(&alice, &asset).is_err());
    assert_eq!(env.total_staked(), 1);
}

#[test]
fn only_owner_can_stake() {
    let mut env = Env::new();
    let alice = env.user();
    let mallory = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    assert_fails_with(env.stake(&mallory, &asset), "NotOwner");
}

#[test]
fn asset_from_another_collection_is_rejected() {
    let mut env = Env::new();
    let mut other = Env::new();
    let alice = env.user();
    // Mint in `other`, then copy the asset account into `env`'s SVM.
    let foreign = other.mint_nft(&alice.pubkey());
    let acct = other.svm.get_account(&foreign).unwrap();
    env.svm.set_account(foreign, acct).unwrap();

    assert_fails_with(env.stake(&alice, &foreign), "WrongCollection");
}

#[test]
fn total_staked_tracks_multiple_assets() {
    let mut env = Env::new();
    let alice = env.user();
    let bob = env.user();
    let a1 = env.mint_nft(&alice.pubkey());
    let a2 = env.mint_nft(&alice.pubkey());
    let b1 = env.mint_nft(&bob.pubkey());

    env.stake(&alice, &a1).unwrap();
    env.stake(&alice, &a2).unwrap();
    env.stake(&bob, &b1).unwrap();
    assert_eq!(env.total_staked(), 3);

    env.warp_to(env.now() + FREEZE_PERIOD);
    env.unstake(&alice, &a1).unwrap();
    assert_eq!(env.total_staked(), 2);
    env.burn(&bob, &b1).unwrap();
    assert_eq!(env.total_staked(), 1);
}

// ---------------------------------------------------------------------------
// claim_rewards
// ---------------------------------------------------------------------------

#[test]
fn claim_rewards_keeps_nft_staked_and_frozen() {
    let mut env = Env::new();
    let alice = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    env.stake(&alice, &asset).unwrap();

    env.warp_to(env.now() + DAY);
    env.claim(&alice, &asset).expect("claim");

    assert_eq!(env.reward_balance(&alice.pubkey()), REWARDS_PER_DAY);
    assert!(env.is_frozen(&asset), "still frozen after claim");
    assert_eq!(env.asset(&asset).base.owner, alice.pubkey());
    assert_eq!(env.total_staked(), 1, "claim does not unstake");
    let rec = env.stake_record(&asset).expect("stake record still open");
    assert_eq!(rec.last_claimed_at, env.now());

    // Frozen asset still can't move (MPL Core InvalidAuthority = 0x9), even in open hours.
    let bob = env.user();
    assert_fails_with(env.transfer(&alice, &asset, &bob.pubkey()), CORE_REJECTED);
}

#[test]
fn claim_twice_does_not_double_count() {
    let mut env = Env::new();
    let alice = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    env.stake(&alice, &asset).unwrap();

    env.warp_to(env.now() + DAY / 2);
    env.claim(&alice, &asset).unwrap();
    assert_eq!(env.reward_balance(&alice.pubkey()), REWARDS_PER_DAY / 2);

    // Nothing accrued in the same second
    assert_fails_with(env.claim(&alice, &asset), "NothingToClaim");

    env.warp_to(env.now() + DAY / 2);
    env.claim(&alice, &asset).unwrap();
    assert_eq!(env.reward_balance(&alice.pubkey()), REWARDS_PER_DAY);
}

#[test]
fn non_owner_cannot_claim() {
    let mut env = Env::new();
    let alice = env.user();
    let mallory = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    env.stake(&alice, &asset).unwrap();
    env.warp_to(env.now() + DAY);

    assert_fails_with(env.claim(&mallory, &asset), "ConstraintHasOne");
    assert_eq!(env.reward_balance(&mallory.pubkey()), 0);
}

#[test]
fn claim_without_stake_fails() {
    let mut env = Env::new();
    let alice = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    assert_fails_with(env.claim(&alice, &asset), "AccountNotInitialized");
}

// ---------------------------------------------------------------------------
// unstake
// ---------------------------------------------------------------------------

#[test]
fn unstake_respects_freeze_period() {
    let mut env = Env::new();
    let alice = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    env.stake(&alice, &asset).unwrap();

    env.warp_to(env.now() + FREEZE_PERIOD - 1);
    assert_fails_with(env.unstake(&alice, &asset), "FreezePeriodNotOver");
    assert!(env.is_frozen(&asset));
}

#[test]
fn unstake_pays_rewards_thaws_and_cleans_up() {
    let mut env = Env::new();
    let alice = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    env.stake(&alice, &asset).unwrap();

    // claim one day mid-way, then unstake at the end of the freeze period
    env.warp_to(env.now() + DAY);
    env.claim(&alice, &asset).unwrap();
    env.warp_to(env.now() + FREEZE_PERIOD - DAY);
    env.unstake(&alice, &asset).expect("unstake");

    assert_eq!(
        env.reward_balance(&alice.pubkey()),
        REWARDS_PER_DAY * (FREEZE_PERIOD / DAY) as u64,
        "claimed + unstake payout equals full period"
    );
    let a = env.asset(&asset);
    assert!(
        a.plugin_list.freeze_delegate.is_none(),
        "FreezeDelegate removed"
    );
    assert!(
        a.plugin_list.burn_delegate.is_none(),
        "BurnDelegate removed"
    );
    assert_eq!(a.base.owner, alice.pubkey());
    assert_eq!(env.total_staked(), 0);
    assert!(env.stake_record(&asset).is_none(), "stake record closed");

    // The asset is free again: it can be transferred (10:00 UTC + 3 days = open hours)
    let bob = env.user();
    env.transfer(&alice, &asset, &bob.pubkey())
        .expect("transfer after unstake");
    assert_eq!(env.asset(&asset).base.owner, bob.pubkey());
}

#[test]
fn restake_after_unstake() {
    let mut env = Env::new();
    let alice = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    env.stake(&alice, &asset).unwrap();
    env.warp_to(env.now() + FREEZE_PERIOD);
    env.unstake(&alice, &asset).unwrap();

    env.stake(&alice, &asset).expect("restake");
    assert!(env.is_frozen(&asset));
    assert_eq!(env.total_staked(), 1);
}

#[test]
fn non_owner_cannot_unstake() {
    let mut env = Env::new();
    let alice = env.user();
    let mallory = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    env.stake(&alice, &asset).unwrap();
    env.warp_to(env.now() + FREEZE_PERIOD);
    assert_fails_with(env.unstake(&mallory, &asset), "ConstraintHasOne");
    assert!(env.is_frozen(&asset));
}

// ---------------------------------------------------------------------------
// burn_staked_nft
// ---------------------------------------------------------------------------

#[test]
fn burn_staked_nft_pays_bonus_and_burns() {
    let mut env = Env::new();
    let alice = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    env.stake(&alice, &asset).unwrap();
    assert_eq!(env.total_staked(), 1);

    env.warp_to(env.now() + DAY);
    env.burn(&alice, &asset).expect("burn");

    assert_eq!(
        env.reward_balance(&alice.pubkey()),
        REWARDS_PER_DAY + BURN_BONUS,
        "pending rewards + burn bonus"
    );
    // MPL Core leaves a 1-byte tombstone with Key::Uninitialized after burn
    let acct = env.svm.get_account(&asset);
    if let Some(acct) = acct {
        assert!(
            acct.data.is_empty() || acct.data[0] == Key::Uninitialized as u8,
            "asset burned"
        );
    }
    assert_eq!(env.total_staked(), 0);
    assert!(env.stake_record(&asset).is_none());
    assert_eq!(env.collection_state().base.current_size, 0);
}

#[test]
fn burn_works_inside_freeze_period() {
    let mut env = Env::new();
    let alice = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    env.stake(&alice, &asset).unwrap();
    env.burn(&alice, &asset).expect("burn immediately");
    assert_eq!(env.reward_balance(&alice.pubkey()), BURN_BONUS);
}

#[test]
fn non_owner_cannot_burn() {
    let mut env = Env::new();
    let alice = env.user();
    let mallory = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    env.stake(&alice, &asset).unwrap();
    assert_fails_with(env.burn(&mallory, &asset), "ConstraintHasOne");
    assert!(env.is_frozen(&asset));
    assert_eq!(env.total_staked(), 1);
}

#[test]
fn unstaked_nft_cannot_be_burned_for_bonus() {
    let mut env = Env::new();
    let alice = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    assert_fails_with(env.burn(&alice, &asset), "AccountNotInitialized");
}
