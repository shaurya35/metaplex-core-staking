// Regression: ISSUE-002 — vault deposits below rent exemption were accepted (LiteSVM skips the
// rent check for zero-data accounts; a real cluster fails the tx with a generic rent error)
// Found by /qa on 2026-10-06
// Report: .gstack/qa-reports/qa-report-core-staking-2026-10-06.md

mod common;

use common::*;

/// Rent-exempt minimum for a 0-byte system account.
const RENT_MIN: u64 = 890_880;

#[test]
fn initialize_rejects_vault_funding_below_rent() {
    let mut args = default_args();
    args.vault_funding = 1_000;
    let (_, res) = Env::try_new(DAY0 + 10 * HOUR, args);
    assert_fails_with(res, "VaultBelowRentExempt");
}

#[test]
fn initialize_accepts_zero_or_rent_exempt_funding() {
    for funding in [0, RENT_MIN] {
        let mut args = default_args();
        args.vault_funding = funding;
        let (env, res) = Env::try_new(DAY0 + 10 * HOUR, args);
        res.unwrap_or_else(|e| panic!("funding {funding}: {:?}", e.meta.logs));
        assert_eq!(env.lamports(&env.vault), funding);
    }
}

#[test]
fn fund_vault_rejects_first_deposit_below_rent_but_allows_top_ups() {
    let mut args = default_args();
    args.vault_funding = 0;
    let (mut env, res) = Env::try_new(DAY0 + 10 * HOUR, args);
    res.unwrap();
    let funder = env.user();

    assert_fails_with(env.fund_vault(&funder, 1_000), "VaultBelowRentExempt");
    assert_eq!(env.lamports(&env.vault), 0);

    env.fund_vault(&funder, RENT_MIN)
        .expect("rent-exempt deposit");
    env.fund_vault(&funder, 1)
        .expect("small top-up of a funded vault");
    assert_eq!(env.lamports(&env.vault), RENT_MIN + 1);
}
