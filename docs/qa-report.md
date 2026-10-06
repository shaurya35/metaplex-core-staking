# Functional QA Report: core-staking (Anchor program)

> Copied from `.gstack/qa-reports/` (local QA evidence, not committed). Checkpoint links refer to that local folder.

| Field | Value |
|---|---|
| Date / branch / revision | 2026-10-06 / main / baseline 21b196f → fixes 0a30f21, c67e299 |
| Caller / authority / depth | /qa, Standard tier (critical+high+medium fixed), Full functional mode; writes: program source, new regression tests, reports |
| Surfaces / scope | Functional only: Solana program instructions (initialize, fund_vault, mint_nft, stake, claim_rewards, unstake, burn_staked_nft, update_oracle, transfer_nft) and direct MPL Core calls against staked assets |
| Runtime / native tools | anchor-cli 1.1.2, rustc 1.96.1, LiteSVM 0.10, real `mpl_core.so` fixture; `anchor build`, `cargo test -p core-staking` |
| Fixture ownership / destinations | Fresh in-memory LiteSVM per test, synthetic keypairs; no RPC, devnet or mainnet |
| Probe budget / guarded command time / stop reason | Unbounded functional mode, per-command `--timeout-ms 600000`; 15 captures, each 0.4–5 s; stopped when all charters (C1–C6) were tested |

**Evidence verdict (from `gstack-qa-evidence materialize`): inconclusive.** The open items are captures 008 and 009, the red runs before repair. The same exact commands were not re-run on the final inputs. Both regression files *are* green inside the final full-suite capture 015 (`cargo test -p core-staking`, 41/41), but the tool only closes a capture with an identical-command rerun.

## Contract outcomes

| Contract and source | Exact probe / evidence | Expected → observed | Outcome |
|---|---|---|---|
| C1 Documented suite passes (README) | `cargo test -p core-staking --test staking --test oracle --lib` · .qa-evidence/001 | all pass → 34/34 | pass |
| C2a Owner can't unfreeze a staked asset (design: config PDA owns FreezeDelegate) | qa_probe `owner_unfreezes_directly` · 002 | rejected → Core 0x1a, still frozen | pass |
| C2b Owner can't revoke FreezeDelegate authority | `owner_revokes_freeze_authority` · 003 | rejected → Core 0x9, still frozen | pass |
| C2c Owner can't remove FreezeDelegate | `owner_removes_freeze_plugin` · 004 | rejected → Core 0x9, still frozen | pass |
| C3 Stripping BurnDelegate doesn't break unstake / `total_staked` | `owner_strips_burn_delegate` · 005 | unstake ok, counter 0 → as expected; **revoke of BurnDelegate succeeds** (ISSUE-003) | pass (low finding deferred) |
| C4 Vault deposit below rent exemption is rejected with a named error | `initialize_with_tiny_vault_funding` · 006 → replay 013 | named error → accepted (006, ISSUE-002); after fix `VaultBelowRentExempt` (013) | fail → fixed |
| C5 Frequent claims pay the same as one claim | `reward_truncation_drift` · 007 → replay 011 | lost 0 → lost 244/694,444 (007, ISSUE-001); after fix lost 0 (011) | fail → fixed |
| C6 Invalid hours rejected | `invalid_hours_rejected` · 014 | InvalidHours → 17–9, 9–9, 9–25 all InvalidHours | pass |
| Final: full suite on final inputs | `cargo test -p core-staking` · 015 | all pass → 41/41 (6 unit + 18 + 11 + 2 + 3 + 1 CU) | pass |

## Findings

### ISSUE-001: Frequent claims lose the fractional reward on every claim — Medium, **fixed (verified)**

- Classification / severity: product defect. Medium: stakers who claim often get underpaid, by about 50% at low rates.
- Intended contract: "pending = elapsed × rewards_per_day / 86400"; claiming in pieces should sum to the same total (README / docs/architecture.md).
- Reproduction: stake, then call `claim_rewards` every second for N seconds at `rewards_per_day = 172_799`.
- Observed: 100 claims paid 100 units, against the 199 that one claim would pay. At the default rate, 600 claims lost 244 units.
- Root cause: each claim rounded down `elapsed*rate/86400` and set `last_claimed_at = now`, so the sub-unit remainder was thrown away.
- Fix (0a30f21): `StakeRecord.reward_remainder` carries `accrued % 86400`, and `pending_rewards(rate, from, to, remainder)` returns `(amount, new_remainder)`.
- Regression: `programs/core-staking/tests/rewards_regression_1.rs` (red in 008, green in 010/015) and unit test `utils::tests::rewards_carry_remainder`.
- Value: protects=sum of per-second claims equals a single claim; fails_when=remainder dropped or last_claimed_at reset without carry; why_new=staking.rs only tested half-day claims at a rate that divides evenly; seam=none

### ISSUE-002: Vault deposits below rent exemption accepted — Medium, **fixed (verified)**

- Classification / severity: product defect plus a gap between the test harness and a real cluster. Medium: on a real cluster `initialize` / `fund_vault` fail with the runtime's generic "insufficient funds for rent" instead of a program error, and LiteSVM hides this because it skips the rent check for zero-data accounts (`litesvm-0.10.0/src/lib.rs:1236`).
- Reproduction: `initialize` with `vault_funding = 1_000`, or a first `fund_vault(1_000)` into an empty vault.
- Observed: the deposit is accepted in LiteSVM (006). Agave rejects the Uninitialized→RentPaying transition.
- Fix (c67e299): `assert_vault_deposit_ok` requires the balance after the deposit to be ≥ `Rent::minimum_balance(0)`. It is used in `initialize` and `fund_vault` and fails with the new error `VaultBelowRentExempt` (6011).
- Regression: `programs/core-staking/tests/vault_regression_1.rs` (red in 009, green in 012/015).
- Value: protects=vault never enters a rent-paying state via our ixs; fails_when=guard removed from initialize or fund_vault; why_new=LiteSVM cannot catch this without an explicit program check; seam=none

### ISSUE-003: Owner can revoke the BurnDelegate on a staked asset — Low, **deferred**

- Observed (005): `RevokePluginAuthorityV1(BurnDelegate)` signed by the owner succeeds while the asset is frozen. After that, `burn_staked_nft` for that asset fails with MPL Core 0x9 instead of a named error.
- Impact: only the owner is affected, since they lose the burn-to-earn option for that NFT. `unstake` still works and `total_staked` stays correct, so there is no fund or accounting risk.
- Possible follow-up: check the BurnDelegate authority in `burn_staked_nft` and return a named error, or re-approve it.

## Discoveries and permanent tests

- [checkpoint 002](../.gstack/qa-reports/exploration-002.json): baseline green, so I tested whether the owner can bypass the program and unfreeze directly.
- [checkpoint 003](../.gstack/qa-reports/exploration-003.json): direct unfreeze was rejected, so next was revoking the delegate.
- [checkpoint 004](../.gstack/qa-reports/exploration-004.json): revoke was rejected, so next was removing the plugin outright.
- [checkpoint 005](../.gstack/qa-reports/exploration-005.json): the freeze cannot be escaped, so next was whether stripping the BurnDelegate breaks cleanup.
- [checkpoint 006](../.gstack/qa-reports/exploration-006.json): BurnDelegate revoke works but only hurts the owner, so next was rent behaviour on vault deposits.
- [checkpoint 007](../.gstack/qa-reports/exploration-007.json): a below-rent deposit was accepted (LiteSVM skips the check), so next was claim rounding.
- [checkpoint 008](../.gstack/qa-reports/exploration-008.json): a leak was confirmed, so I wrote regressions and confirmed them red (superseded).
- [checkpoint 009](../.gstack/qa-reports/exploration-009.json): ISSUE-002 regression red (superseded).
- [checkpoint 010](../.gstack/qa-reports/exploration-010.json): ISSUE-001 fix green, adjacent suites green.
- [checkpoint 011](../.gstack/qa-reports/exploration-011.json): the original truncation probe now shows lost=0.
- [checkpoint 012](../.gstack/qa-reports/exploration-012.json): ISSUE-002 fix green, oracle and vault suite green.
- [checkpoint 013](../.gstack/qa-reports/exploration-013.json): the original vault probe now returns VaultBelowRentExempt.
- [checkpoint 014](../.gstack/qa-reports/exploration-014.json): C6 invalid hours all rejected.
- [checkpoint 015](../.gstack/qa-reports/exploration-015.json): final full suite 41/41.

| Hypothesis / discovery | Native test | Red before repair | Green + original + adjacent | Disposition |
|---|---|---|---|---|
| Per-claim rounding leaks rewards | integration `tests/rewards_regression_1.rs` and unit `rewards_carry_remainder` | 008: 100≠199, 11≠21 | 010 green; 011 probe lost=0; 015 full suite | fixed, 0a30f21 |
| Below-rent vault deposit is not guarded | integration `tests/vault_regression_1.rs` | 009: 2 red | 012 green; 013 probe named error; 015 full suite | fixed, c67e299 |
| Owner can revoke BurnDelegate | none | n/a | n/a | deferred (Low) |

## Coverage limits and cleanup

- The scratch file `tests/qa_probe.rs` (probes for 002–007, 011, 013, 014) was deleted after final QA and never committed. To replay those probes it must be recreated; its captured outputs are kept in `.qa-evidence/`.
- Real-cluster rent behaviour (ISSUE-002) is inferred from Agave's rent-state rules, not run on a cluster. LiteSVM can't reproduce it.
- Not run: concurrency/ordering (Solana serializes writes to the same account), timeouts/retries (no async workers in this program), and devnet deployment (not required by the assignment).
- Cleanup: all state was in-memory LiteSVM and nothing is left behind except build artifacts in `target/`.

**PR summary:** QA found 3 issues and fixed 2 (both Medium, verified with regression tests); 1 Low is deferred. Functional contracts: 8 pass, 2 fail→fixed, 0 blocked. Evidence verdict: inconclusive (captures 008/009 have no identical-command rerun; their tests pass in the final full run).
