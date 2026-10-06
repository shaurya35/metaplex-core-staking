# Architecture

## Accounts

All program PDAs are derived from the collection address, so one deployment can run any number of independent staking collections.

| Account | Seeds | Owner | Purpose |
|---|---|---|---|
| `Config` | `["config", collection]` | this program | Settings. **Update authority of the Collection** and authority of every delegate plugin on staked assets. Mint authority of the reward token. |
| Reward mint | `["rewards", config]` | SPL Token | 6-decimal reward token. |
| `StakeRecord` | `["stake", asset]` | this program | `owner`, `asset`, `collection`, `staked_at`, `last_claimed_at`, `reward_remainder` (sub-unit carry, < 86400). Created on stake, closed on unstake / burn. |
| `Oracle` | `["oracle", collection]` | this program | `OracleValidation::V1 { create, transfer, burn, update }`, which MPL Core reads at offset 8. |
| Vault | `["vault", collection]` | System | Holds lamports that pay oracle crankers. |
| Collection | keypair | MPL Core | `Attributes { total_staked }` plugin and an `Oracle` external plugin adapter. |
| Asset | keypair | MPL Core | Stays in the owner's wallet. Gets `FreezeDelegate` and `BurnDelegate` while staked. |

## Plugin matrix

| Plugin | Lives on | Authority | Added | Changed / removed |
|---|---|---|---|---|
| `Attributes { total_staked }` | Collection | UpdateAuthority (config) | `initialize` | +1 on `stake`, −1 on `unstake` and `burn_staked_nft` |
| `Oracle` adapter (Transfer, CanReject, `ValidationResultsOffset::Anchor`) | Collection | UpdateAuthority (config) | `initialize` | `update_oracle` rewrites the oracle *account*, not the plugin |
| `FreezeDelegate { frozen: true }` | Asset | `Address(config)` | `stake` (owner signs) | thawed by config, then removed by owner on `unstake`; thawed before burn |
| `BurnDelegate` | Asset | `Address(config)` | `stake` (owner signs) | removed on `unstake`; used by config to burn on `burn_staked_nft` |

## Flows

### Stake → claim → unstake

```
owner ──stake──────────▶ program
                           ├─ CPI AddPluginV1 FreezeDelegate{frozen}  (authority = config)
                           ├─ CPI AddPluginV1 BurnDelegate            (authority = config)
                           ├─ CPI UpdateCollectionPluginV1 Attributes total_staked += 1   [config signs]
                           └─ init StakeRecord(staked_at = last_claimed_at = now)

owner ──claim_rewards──▶ program
                           ├─ accrued = (now - last_claimed_at) * rewards_per_day + reward_remainder
                           ├─ pending = accrued / 86400, reward_remainder = accrued % 86400
                           ├─ mint_to(owner ATA, pending)                                  [config signs]
                           └─ last_claimed_at = now        (asset untouched: still frozen)

owner ──unstake────────▶ program   (requires now ≥ staked_at + freeze_period)
                           ├─ mint_to(owner ATA, pending)
                           ├─ CPI UpdatePluginV1 FreezeDelegate{frozen:false}              [config signs]
                           ├─ CPI RemovePluginV1 FreezeDelegate, BurnDelegate              [owner signs]
                           ├─ CPI UpdateCollectionPluginV1 total_staked -= 1               [config signs]
                           └─ close StakeRecord → owner
```

### Burn-to-earn

```
owner ──burn_staked_nft▶ program   (no freeze-period requirement)
                           ├─ mint_to(owner ATA, pending + burn_bonus)
                           ├─ CPI UpdatePluginV1 FreezeDelegate{frozen:false}   (frozen assets can't burn)
                           ├─ CPI BurnV1 authority = config  ← allowed by BurnDelegate
                           ├─ CPI UpdateCollectionPluginV1 total_staked -= 1
                           └─ close StakeRecord → owner
```

### Time-gated transfers (Oracle)

```
            09:00 UTC                         17:00 UTC
  ─────────────┼─────────────────────────────────┼───────────────▶ time
   Rejected    │             Pass                │   Rejected
            ▲ ±crank_window                    ▲ ±crank_window
            paid crank                         paid crank

anyone ──update_oracle──▶ program
                            ├─ desired = Pass if open_hour ≤ hour < close_hour else Rejected
                            ├─ require(oracle.transfer != desired)   ← OracleAlreadyUpToDate
                            ├─ oracle.transfer = desired
                            └─ if |now − boundary| ≤ crank_window and vault has funds above rent:
                                   vault ──crank_reward──▶ cranker   [vault seeds sign]

owner ──transfer_nft──▶ program ──TransferV1 + remaining_accounts[oracle]──▶ MPL Core
                                                          └─ Oracle adapter reads oracle PDA @ offset 8
                                                             Rejected → tx fails (InvalidAuthority 0x9)
```

The plugin lives on the Collection, so MPL Core enforces it for every transfer of every asset in that collection. That includes transfers that skip this program and call MPL Core directly; `oracle_is_enforced_by_core_not_just_our_program` tests this case.

## Design decisions

- **Rewards are tracked in a PDA, not in asset attributes.** A program-owned `StakeRecord` keeps staking time safe from anyone who can update attributes, and it makes `has_one = owner` checks trivial. The collection-level stat is still stored as a Core Attribute, which is what the task asks for.
- **The config PDA is the plugin authority.** The owner signs to add the owner-managed delegate plugins, but their authority is the config PDA. The owner therefore cannot thaw or burn on their own and must go through `unstake` or `burn_staked_nft`.
- **Thaw before remove or burn.** MPL Core rejects removal of a frozen FreezeDelegate, and it rejects burns of frozen assets, so the config PDA first sets `frozen: false`.
- **Minting is admin-gated.** If anyone could mint, they could mint unlimited NFTs and farm rewards.
- **The crank only pays for real state changes.** `update_oracle` fails with `OracleAlreadyUpToDate` when nothing changes, so a cranker can be paid at most once per boundary. A late crank still fixes stale state, but it is not paid. The vault never drops below rent exemption, and an underfunded vault skips the reward rather than failing.
- **No reward lost to rounding.** Each claim rounds down to whole base units, and the remainder (`accrued % 86400`) is stored on the `StakeRecord`. Many small claims therefore add up to exactly one large claim (QA ISSUE-001).
- **The vault stays rent exempt.** `initialize` and `fund_vault` reject a deposit that would leave the 0-byte vault below the rent-exempt minimum. LiteSVM doesn't check rent on zero-data accounts, but a real cluster does (QA ISSUE-002).
- **Transfer is `Pass` when open, not `Approved`.** The adapter only has the `CanReject` capability, so "no objection" is the right signal and it does not override other plugins.
- **Mirrored oracle types.** `state/oracle.rs` copies `OracleValidation` and `ExternalValidationResult` so the account can derive Anchor traits. `oracle_account_layout_matches_mpl_core` deserializes the raw bytes with the real `mpl_core` types to prove the layout matches.

## Errors

| Code | Name | When |
|---|---|---|
| 6000 | `NotOwner` | signer is not the asset owner |
| 6001 | `WrongCollection` | asset's update authority is not this collection |
| 6002 | `InvalidCoreAccount` | account isn't owned by MPL Core or doesn't deserialize |
| 6003 | `FreezePeriodNotOver` | unstake before `staked_at + freeze_period` |
| 6004 | `NothingToClaim` | zero rewards accrued |
| 6005 | `Overflow` | checked math overflow |
| 6006 | `MissingStakedAttribute` | collection lacks `total_staked` |
| 6007 | `CounterUnderflow` | `total_staked` would go below 0 |
| 6008 | `OracleAlreadyUpToDate` | crank with no state change |
| 6009 | `InvalidHours` | `open_hour >= close_hour` or `close_hour > 24` |
| 6010 | `InvalidParam` | negative freeze period / crank window |
| 6011 | `VaultBelowRentExempt` | a vault deposit would leave the vault below the rent-exempt minimum |
