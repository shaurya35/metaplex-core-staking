// Regression: ISSUE-001 — frequent claims lost the fractional reward on every claim
// Found by /qa on 2026-10-06
// Report: .gstack/qa-reports/qa-report-core-staking-2026-10-06.md

mod common;

use common::*;
use solana_signer::Signer;

/// ~2 base units per second (172_799 / 86_400 = 1.99999). Before the fix each
/// one-second claim paid floor(1.99999) = 1 and reset the clock, losing ~50%.
const RATE: u64 = 172_799;

fn claim_every_second(seconds: i64) -> (u64, u64) {
    let mut args = default_args();
    args.rewards_per_day = RATE;
    let (mut env, res) = Env::try_new(DAY0 + 10 * HOUR, args);
    res.expect("initialize");
    let alice = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    env.stake(&alice, &asset).unwrap();
    let start = env.now();
    for i in 1..=seconds {
        env.warp_to(start + i);
        env.claim(&alice, &asset).expect("claim");
    }
    let expected = (seconds as u128 * RATE as u128 / 86_400) as u64;
    (env.reward_balance(&alice.pubkey()), expected)
}

#[test]
fn frequent_claims_match_a_single_claim() {
    let (paid, expected) = claim_every_second(100);
    assert_eq!(
        paid, expected,
        "100 one-second claims must pay the same as one 100s claim"
    );
}

#[test]
fn fractional_remainder_is_carried_to_unstake() {
    let mut args = default_args();
    args.rewards_per_day = RATE;
    args.freeze_period = 0;
    let (mut env, res) = Env::try_new(DAY0 + 10 * HOUR, args);
    res.expect("initialize");
    let alice = env.user();
    let asset = env.mint_nft(&alice.pubkey());
    env.stake(&alice, &asset).unwrap();
    let start = env.now();
    for i in 1..=10 {
        env.warp_to(start + i);
        env.claim(&alice, &asset).unwrap();
    }
    env.warp_to(start + 11);
    env.unstake(&alice, &asset).unwrap();
    assert_eq!(
        env.reward_balance(&alice.pubkey()),
        (11 * RATE as u128 / 86_400) as u64
    );
}
