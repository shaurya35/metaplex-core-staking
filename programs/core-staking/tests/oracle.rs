//! Task 2: time-gated transfers through an MPL Core Oracle external plugin, a
//! permissionless crank that keeps the oracle in sync, and the crank reward vault.

mod common;

use common::*;
use core_staking::{ExternalValidationResult, OracleValidation};
use mpl_core::types::{ExternalValidationResult as CoreResult, OracleValidation as CoreValidation};
use solana_signer::Signer;

fn transfer_state(env: &Env) -> ExternalValidationResult {
    env.oracle_state().validation.transfer().unwrap()
}

#[test]
fn oracle_account_layout_matches_mpl_core() {
    // MPL Core reads `OracleValidation` at offset 8 (ValidationResultsOffset::Anchor).
    let env = Env::new();
    let data = env.svm.get_account(&env.oracle).unwrap().data;
    let core: CoreValidation = borsh::BorshDeserialize::deserialize(&mut &data[8..]).unwrap();
    assert_eq!(
        core,
        CoreValidation::V1 {
            create: CoreResult::Pass,
            transfer: CoreResult::Pass,
            burn: CoreResult::Pass,
            update: CoreResult::Pass,
        }
    );
}

#[test]
fn initialize_seeds_oracle_from_clock() {
    let open = Env::new_at(DAY0 + 10 * HOUR);
    assert_eq!(transfer_state(&open), ExternalValidationResult::Pass);

    let closed = Env::new_at(DAY0 + 20 * HOUR);
    assert_eq!(transfer_state(&closed), ExternalValidationResult::Rejected);
    assert_eq!(
        closed.oracle_state().validation,
        OracleValidation::for_transfer(ExternalValidationResult::Rejected)
    );
}

#[test]
fn transfer_allowed_during_trading_hours() {
    let mut env = Env::new_at(DAY0 + 10 * HOUR);
    let alice = env.user();
    let bob = env.user();
    let asset = env.mint_nft(&alice.pubkey());

    env.transfer(&alice, &asset, &bob.pubkey())
        .expect("10:00 UTC transfer");
    assert_eq!(env.asset(&asset).base.owner, bob.pubkey());
}

#[test]
fn transfer_rejected_outside_trading_hours() {
    let mut env = Env::new_at(DAY0 + 10 * HOUR);
    let alice = env.user();
    let bob = env.user();
    let cranker = env.user();
    let asset = env.mint_nft(&alice.pubkey());

    // 17:00 -> crank closes the window
    env.warp_to(DAY0 + 17 * HOUR);
    env.crank(&cranker).expect("close crank");
    assert_eq!(transfer_state(&env), ExternalValidationResult::Rejected);

    assert_fails_with(env.transfer(&alice, &asset, &bob.pubkey()), CORE_REJECTED);
    assert_eq!(env.asset(&asset).base.owner, alice.pubkey());

    // next morning 09:00 -> crank opens it again
    env.warp_to(DAY0 + DAY + 9 * HOUR);
    env.crank(&cranker).expect("open crank");
    assert_eq!(transfer_state(&env), ExternalValidationResult::Pass);
    env.transfer(&alice, &asset, &bob.pubkey())
        .expect("transfer after reopen");
    assert_eq!(env.asset(&asset).base.owner, bob.pubkey());
}

#[test]
fn oracle_is_enforced_by_core_not_just_our_program() {
    // A direct MPL Core TransferV1 (bypassing transfer_nft) must also be rejected,
    // proving the Oracle plugin itself gates the collection.
    let mut env = Env::new_at(DAY0 + 20 * HOUR);
    let alice = env.user();
    let bob = env.user();
    let asset = env.mint_nft(&alice.pubkey());

    let ix = mpl_core::instructions::TransferV1Builder::new()
        .asset(asset)
        .collection(Some(env.collection))
        .payer(alice.pubkey())
        .new_owner(bob.pubkey())
        .add_remaining_account(anchor_lang::prelude::AccountMeta::new_readonly(
            env.oracle, false,
        ))
        .instruction();
    assert_fails_with(env.send(&[ix], &alice, &[]), "custom program error: 0x9");
}

#[test]
fn crank_is_permissionless_but_only_on_state_change() {
    let mut env = Env::new_at(DAY0 + 10 * HOUR);
    let rando = env.user();

    // Already Pass at 10:00 -> nothing to do
    assert_fails_with(env.crank(&rando), "OracleAlreadyUpToDate");

    env.warp_to(DAY0 + 18 * HOUR);
    env.crank(&rando).expect("any signer can crank");
    assert_eq!(transfer_state(&env), ExternalValidationResult::Rejected);

    // Repeat in the same closed period -> rejected (no reward farming)
    env.warp_to(DAY0 + 18 * HOUR + 60);
    assert_fails_with(env.crank(&rando), "OracleAlreadyUpToDate");
}

#[test]
fn crank_rewarded_near_boundary() {
    let mut env = Env::new_at(DAY0 + 10 * HOUR);
    let cranker = env.user();

    // 2 minutes after close -> within the 5 min window -> paid
    env.warp_to(DAY0 + 17 * HOUR + 120);
    let vault_before = env.lamports(&env.vault);
    let before = env.lamports(&cranker.pubkey());
    env.crank(&cranker).unwrap();
    let fee = 5_000;
    assert_eq!(env.lamports(&cranker.pubkey()), before + CRANK_REWARD - fee);
    assert_eq!(env.lamports(&env.vault), vault_before - CRANK_REWARD);

    // Exactly at open boundary next day -> paid again
    env.warp_to(DAY0 + DAY + 9 * HOUR);
    let before = env.lamports(&cranker.pubkey());
    env.crank(&cranker).unwrap();
    assert_eq!(env.lamports(&cranker.pubkey()), before + CRANK_REWARD - fee);
}

#[test]
fn late_crank_updates_state_without_reward() {
    let mut env = Env::new_at(DAY0 + 10 * HOUR);
    let cranker = env.user();

    // Nobody cranked at 17:00; at 21:00 the state is stale. A crank still fixes it,
    // but it is far from a boundary so it is not paid.
    env.warp_to(DAY0 + 21 * HOUR);
    let vault_before = env.lamports(&env.vault);
    let before = env.lamports(&cranker.pubkey());
    env.crank(&cranker).unwrap();
    assert_eq!(transfer_state(&env), ExternalValidationResult::Rejected);
    assert_eq!(
        env.lamports(&cranker.pubkey()),
        before - 5_000,
        "only paid the tx fee"
    );
    assert_eq!(env.lamports(&env.vault), vault_before);
}

#[test]
fn crank_reward_skipped_when_vault_empty() {
    // Build an env whose vault holds only rent: drain by cranking many boundaries is
    // slow, so instead overwrite the vault balance directly.
    let mut env = Env::new_at(DAY0 + 10 * HOUR);
    let cranker = env.user();
    let mut vault = env.svm.get_account(&env.vault).unwrap();
    vault.lamports = 890_880; // rent-exempt minimum for 0 bytes
    env.svm.set_account(env.vault, vault).unwrap();

    env.warp_to(DAY0 + 17 * HOUR);
    let before = env.lamports(&cranker.pubkey());
    env.crank(&cranker).expect("crank succeeds without paying");
    assert_eq!(env.lamports(&cranker.pubkey()), before - 5_000);
    assert_eq!(
        env.lamports(&env.vault),
        890_880,
        "vault never drops below rent"
    );

    // Top up and the next boundary pays again
    let funder = env.user();
    env.fund_vault(&funder, CRANK_REWARD * 10).unwrap();
    env.warp_to(DAY0 + DAY + 9 * HOUR + 30);
    let before = env.lamports(&cranker.pubkey());
    env.crank(&cranker).unwrap();
    assert_eq!(
        env.lamports(&cranker.pubkey()),
        before + CRANK_REWARD - 5_000
    );
}

#[test]
fn staked_nft_blocked_even_in_trading_hours() {
    let mut env = Env::new_at(DAY0 + 10 * HOUR);
    let alice = env.user();
    let bob = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    env.stake(&alice, &asset).unwrap();
    assert_eq!(transfer_state(&env), ExternalValidationResult::Pass);
    assert_fails_with(env.transfer(&alice, &asset, &bob.pubkey()), CORE_REJECTED);
}

#[test]
fn staking_flows_unaffected_by_closed_oracle() {
    // The oracle only checks Transfer, so stake / claim / unstake / burn work at night.
    let mut env = Env::new_at(DAY0 + 22 * HOUR);
    assert_eq!(transfer_state(&env), ExternalValidationResult::Rejected);
    let alice = env.user();
    let a1 = env.mint_nft(&alice.pubkey());
    let a2 = env.mint_nft(&alice.pubkey());
    env.stake(&alice, &a1).unwrap();
    env.stake(&alice, &a2).unwrap();
    env.warp_to(env.now() + FREEZE_PERIOD);
    env.crank(&alice).unwrap_err(); // still night: up to date
    env.claim(&alice, &a1).unwrap();
    env.unstake(&alice, &a1).unwrap();
    env.burn(&alice, &a2).unwrap();
    assert_eq!(env.total_staked(), 0);
}
