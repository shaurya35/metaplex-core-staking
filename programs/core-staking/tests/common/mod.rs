#![allow(dead_code, clippy::result_large_err)]

use anchor_lang::{
    prelude::{Clock, Pubkey},
    solana_program::{instruction::Instruction, system_program},
    AccountDeserialize, InstructionData, ToAccountMetas,
};
use anchor_spl::associated_token::{self, get_associated_token_address};
use core_staking::{
    accounts as acc, instruction as ix, InitializeArgs, Oracle, StakeRecord, CONFIG_SEED,
    ORACLE_SEED, REWARDS_MINT_SEED, STAKE_SEED, VAULT_SEED,
};
use litesvm::{
    types::{FailedTransactionMetadata, TransactionMetadata},
    LiteSVM,
};
use mpl_core::{Asset, Collection};
use solana_keypair::Keypair;
use solana_message::{Message, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;

pub const HOUR: i64 = 3_600;
pub const DAY: i64 = 86_400;
/// 2026-10-06 00:00:00 UTC
pub const DAY0: i64 = 1_791_244_800;

pub const TOKEN: u64 = 1_000_000; // 6 decimals
pub const REWARDS_PER_DAY: u64 = 100 * TOKEN;
pub const FREEZE_PERIOD: i64 = 3 * DAY;
pub const BURN_BONUS: u64 = 1_000 * TOKEN;
pub const CRANK_REWARD: u64 = 10_000_000; // 0.01 SOL
pub const CRANK_WINDOW: i64 = 300; // 5 minutes
pub const VAULT_FUNDING: u64 = 1_000_000_000; // 1 SOL

/// Log line MPL Core emits when a lifecycle check rejects (InvalidAuthority).
pub const CORE_REJECTED: &str =
    "CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d failed: custom program error: 0x9";

pub type TxResult = Result<TransactionMetadata, FailedTransactionMetadata>;

pub struct Env {
    pub svm: LiteSVM,
    pub admin: Keypair,
    pub collection: Pubkey,
    pub config: Pubkey,
    pub rewards_mint: Pubkey,
    pub oracle: Pubkey,
    pub vault: Pubkey,
}

pub fn pda(seeds: &[&[u8]]) -> Pubkey {
    Pubkey::find_program_address(seeds, &core_staking::ID).0
}

pub fn stake_record_pda(asset: &Pubkey) -> Pubkey {
    pda(&[STAKE_SEED, asset.as_ref()])
}

/// Asserts the transaction failed and its logs contain `needle`.
pub fn assert_fails_with(res: TxResult, needle: &str) {
    match res {
        Ok(meta) => panic!(
            "expected failure containing {needle:?}, got success: {:#?}",
            meta.logs
        ),
        Err(e) => assert!(
            e.meta.logs.iter().any(|l| l.contains(needle)),
            "expected logs to contain {needle:?}, got err {:?}\n{:#?}",
            e.err,
            e.meta.logs
        ),
    }
}

impl Env {
    /// Fresh SVM with MPL Core + this program loaded, clock at 10:00 UTC (transfers open),
    /// and an initialized staking collection.
    pub fn new() -> Self {
        Self::new_at(DAY0 + 10 * HOUR)
    }

    pub fn new_at(ts: i64) -> Self {
        let mut svm = LiteSVM::new();
        let root = env!("CARGO_MANIFEST_DIR");
        svm.add_program(
            mpl_core::ID,
            &std::fs::read(format!("{root}/tests/fixtures/mpl_core.so")).unwrap(),
        )
        .unwrap();
        svm.add_program(
            core_staking::ID,
            &std::fs::read(format!("{root}/../../target/deploy/core_staking.so"))
                .expect("run `anchor build` first"),
        )
        .unwrap();

        let admin = Keypair::new();
        svm.airdrop(&admin.pubkey(), 100_000_000_000).unwrap();

        let collection = Keypair::new();
        let config = pda(&[CONFIG_SEED, collection.pubkey().as_ref()]);
        let mut env = Env {
            svm,
            admin,
            collection: collection.pubkey(),
            config,
            rewards_mint: pda(&[REWARDS_MINT_SEED, config.as_ref()]),
            oracle: pda(&[ORACLE_SEED, collection.pubkey().as_ref()]),
            vault: pda(&[VAULT_SEED, collection.pubkey().as_ref()]),
        };
        env.warp_to(ts);

        let init = Instruction::new_with_bytes(
            core_staking::ID,
            &ix::Initialize {
                args: InitializeArgs {
                    name: "Staking Collection".into(),
                    uri: "https://example.com/collection.json".into(),
                    rewards_per_day: REWARDS_PER_DAY,
                    freeze_period: FREEZE_PERIOD,
                    burn_bonus: BURN_BONUS,
                    crank_reward: CRANK_REWARD,
                    crank_window: CRANK_WINDOW,
                    open_hour: 9,
                    close_hour: 17,
                    vault_funding: VAULT_FUNDING,
                },
            }
            .data(),
            acc::Initialize {
                admin: env.admin.pubkey(),
                collection: env.collection,
                config: env.config,
                rewards_mint: env.rewards_mint,
                oracle: env.oracle,
                vault: env.vault,
                mpl_core_program: mpl_core::ID,
                token_program: anchor_spl::token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        let admin = env.admin.insecure_clone();
        env.send(&[init], &admin, &[&collection])
            .expect("initialize");
        env
    }

    // ----- runtime helpers -------------------------------------------------

    pub fn send(&mut self, ixs: &[Instruction], payer: &Keypair, extra: &[&Keypair]) -> TxResult {
        self.svm.expire_blockhash();
        let blockhash = self.svm.latest_blockhash();
        let msg = Message::new_with_blockhash(ixs, Some(&payer.pubkey()), &blockhash);
        let mut signers: Vec<&Keypair> = vec![payer];
        signers.extend_from_slice(extra);
        let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &signers).unwrap();
        self.svm.send_transaction(tx)
    }

    pub fn warp_to(&mut self, unix_ts: i64) {
        let mut clock: Clock = self.svm.get_sysvar();
        clock.unix_timestamp = unix_ts;
        clock.slot += 1;
        self.svm.set_sysvar(&clock);
    }

    pub fn now(&self) -> i64 {
        self.svm.get_sysvar::<Clock>().unix_timestamp
    }

    pub fn user(&mut self) -> Keypair {
        let kp = Keypair::new();
        self.svm.airdrop(&kp.pubkey(), 10_000_000_000).unwrap();
        kp
    }

    // ----- state readers ---------------------------------------------------

    pub fn asset(&self, asset: &Pubkey) -> Box<Asset> {
        let acct = self.svm.get_account(asset).expect("asset account");
        Asset::from_bytes(&acct.data).expect("deserialize asset")
    }

    pub fn collection_state(&self) -> Box<Collection> {
        let acct = self.svm.get_account(&self.collection).unwrap();
        Collection::from_bytes(&acct.data).unwrap()
    }

    pub fn total_staked(&self) -> u64 {
        self.collection_state()
            .plugin_list
            .attributes
            .expect("attributes plugin")
            .attributes
            .attribute_list
            .iter()
            .find(|a| a.key == "total_staked")
            .expect("total_staked attribute")
            .value
            .parse()
            .unwrap()
    }

    pub fn stake_record(&self, asset: &Pubkey) -> Option<StakeRecord> {
        let acct = self.svm.get_account(&stake_record_pda(asset))?;
        if acct.data.is_empty() {
            return None;
        }
        Some(StakeRecord::try_deserialize(&mut acct.data.as_slice()).unwrap())
    }

    pub fn oracle_state(&self) -> Oracle {
        let acct = self.svm.get_account(&self.oracle).unwrap();
        Oracle::try_deserialize(&mut acct.data.as_slice()).unwrap()
    }

    pub fn ata(&self, owner: &Pubkey) -> Pubkey {
        get_associated_token_address(owner, &self.rewards_mint)
    }

    /// SPL token balance of the owner's reward ATA (0 if it does not exist).
    pub fn reward_balance(&self, owner: &Pubkey) -> u64 {
        match self.svm.get_account(&self.ata(owner)) {
            Some(a) if a.data.len() >= 72 => u64::from_le_bytes(a.data[64..72].try_into().unwrap()),
            _ => 0,
        }
    }

    pub fn lamports(&self, key: &Pubkey) -> u64 {
        self.svm.get_account(key).map(|a| a.lamports).unwrap_or(0)
    }

    pub fn is_frozen(&self, asset: &Pubkey) -> bool {
        self.asset(asset)
            .plugin_list
            .freeze_delegate
            .map(|f| f.freeze_delegate.frozen)
            .unwrap_or(false)
    }

    // ----- instruction wrappers --------------------------------------------

    pub fn mint_nft(&mut self, owner: &Pubkey) -> Pubkey {
        let asset = Keypair::new();
        let res = self.try_mint_nft(&self.admin.insecure_clone(), owner, &asset);
        res.expect("mint_nft");
        asset.pubkey()
    }

    pub fn try_mint_nft(&mut self, admin: &Keypair, owner: &Pubkey, asset: &Keypair) -> TxResult {
        let ix = Instruction::new_with_bytes(
            core_staking::ID,
            &ix::MintNft {
                name: "Staker #1".into(),
                uri: "https://example.com/1.json".into(),
            }
            .data(),
            acc::MintNft {
                admin: admin.pubkey(),
                owner: *owner,
                asset: asset.pubkey(),
                collection: self.collection,
                config: self.config,
                mpl_core_program: mpl_core::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        self.send(&[ix], admin, &[asset])
    }

    pub fn stake(&mut self, owner: &Keypair, asset: &Pubkey) -> TxResult {
        let ix = Instruction::new_with_bytes(
            core_staking::ID,
            &ix::Stake {}.data(),
            acc::Stake {
                owner: owner.pubkey(),
                config: self.config,
                asset: *asset,
                collection: self.collection,
                stake_record: stake_record_pda(asset),
                mpl_core_program: mpl_core::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        self.send(&[ix], owner, &[])
    }

    pub fn claim(&mut self, owner: &Keypair, asset: &Pubkey) -> TxResult {
        let ix = Instruction::new_with_bytes(
            core_staking::ID,
            &ix::ClaimRewards {}.data(),
            acc::ClaimRewards {
                owner: owner.pubkey(),
                config: self.config,
                asset: *asset,
                collection: self.collection,
                stake_record: stake_record_pda(asset),
                rewards_mint: self.rewards_mint,
                owner_ata: self.ata(&owner.pubkey()),
                token_program: anchor_spl::token::ID,
                associated_token_program: associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        self.send(&[ix], owner, &[])
    }

    pub fn unstake(&mut self, owner: &Keypair, asset: &Pubkey) -> TxResult {
        let ix = Instruction::new_with_bytes(
            core_staking::ID,
            &ix::Unstake {}.data(),
            acc::Unstake {
                owner: owner.pubkey(),
                config: self.config,
                asset: *asset,
                collection: self.collection,
                stake_record: stake_record_pda(asset),
                rewards_mint: self.rewards_mint,
                owner_ata: self.ata(&owner.pubkey()),
                mpl_core_program: mpl_core::ID,
                token_program: anchor_spl::token::ID,
                associated_token_program: associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        self.send(&[ix], owner, &[])
    }

    pub fn burn(&mut self, owner: &Keypair, asset: &Pubkey) -> TxResult {
        let ix = Instruction::new_with_bytes(
            core_staking::ID,
            &ix::BurnStakedNft {}.data(),
            acc::BurnStakedNft {
                owner: owner.pubkey(),
                config: self.config,
                asset: *asset,
                collection: self.collection,
                stake_record: stake_record_pda(asset),
                rewards_mint: self.rewards_mint,
                owner_ata: self.ata(&owner.pubkey()),
                mpl_core_program: mpl_core::ID,
                token_program: anchor_spl::token::ID,
                associated_token_program: associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        self.send(&[ix], owner, &[])
    }

    pub fn crank(&mut self, cranker: &Keypair) -> TxResult {
        let ix = Instruction::new_with_bytes(
            core_staking::ID,
            &ix::UpdateOracle {}.data(),
            acc::UpdateOracle {
                cranker: cranker.pubkey(),
                config: self.config,
                oracle: self.oracle,
                vault: self.vault,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        self.send(&[ix], cranker, &[])
    }

    pub fn fund_vault(&mut self, funder: &Keypair, amount: u64) -> TxResult {
        let ix = Instruction::new_with_bytes(
            core_staking::ID,
            &ix::FundVault { amount }.data(),
            acc::FundVault {
                funder: funder.pubkey(),
                config: self.config,
                vault: self.vault,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        self.send(&[ix], funder, &[])
    }

    pub fn transfer(&mut self, owner: &Keypair, asset: &Pubkey, new_owner: &Pubkey) -> TxResult {
        let ix = Instruction::new_with_bytes(
            core_staking::ID,
            &ix::TransferNft {}.data(),
            acc::TransferNft {
                owner: owner.pubkey(),
                new_owner: *new_owner,
                config: self.config,
                asset: *asset,
                collection: self.collection,
                oracle: self.oracle,
                mpl_core_program: mpl_core::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        self.send(&[ix], owner, &[])
    }
}
