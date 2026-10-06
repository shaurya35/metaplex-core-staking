use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};
use anchor_spl::token::{Mint, Token};
use mpl_core::{
    instructions::CreateCollectionV2CpiBuilder,
    types::{
        Attribute, Attributes, ExternalCheckResult, ExternalPluginAdapterInitInfo,
        HookableLifecycleEvent, OracleInitInfo, Plugin, PluginAuthority, PluginAuthorityPair,
        ValidationResultsOffset,
    },
};

use crate::{
    constants::*,
    error::StakingError,
    state::{Config, ExternalValidationResult, Oracle, OracleValidation},
    utils::{assert_vault_deposit_ok, is_transfer_window_open},
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct InitializeArgs {
    pub name: String,
    pub uri: String,
    pub rewards_per_day: u64,
    pub freeze_period: i64,
    pub burn_bonus: u64,
    pub crank_reward: u64,
    pub crank_window: i64,
    pub open_hour: u8,
    pub close_hour: u8,
    /// Lamports moved into the crank-reward vault right away (0 to skip).
    pub vault_funding: u64,
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    /// New MPL Core collection. Created by CPI in this instruction.
    #[account(mut)]
    pub collection: Signer<'info>,

    #[account(
        init,
        payer = admin,
        space = 8 + Config::INIT_SPACE,
        seeds = [CONFIG_SEED, collection.key().as_ref()],
        bump,
    )]
    pub config: Box<Account<'info, Config>>,

    #[account(
        init,
        payer = admin,
        seeds = [REWARDS_MINT_SEED, config.key().as_ref()],
        bump,
        mint::decimals = REWARD_DECIMALS,
        mint::authority = config,
        mint::token_program = token_program,
    )]
    pub rewards_mint: Box<Account<'info, Mint>>,

    #[account(
        init,
        payer = admin,
        space = 8 + Oracle::INIT_SPACE,
        seeds = [ORACLE_SEED, collection.key().as_ref()],
        bump,
    )]
    pub oracle: Box<Account<'info, Oracle>>,

    #[account(
        mut,
        seeds = [VAULT_SEED, collection.key().as_ref()],
        bump,
    )]
    pub vault: SystemAccount<'info>,

    /// CHECK: address constrained to the MPL Core program
    #[account(address = mpl_core::ID)]
    pub mpl_core_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<Initialize>, args: InitializeArgs) -> Result<()> {
    require!(
        args.open_hour < args.close_hour && args.close_hour <= 24,
        StakingError::InvalidHours
    );
    require!(
        args.freeze_period >= 0 && args.crank_window >= 0,
        StakingError::InvalidParam
    );

    let now = Clock::get()?.unix_timestamp;
    let accounts = ctx.accounts;

    accounts.config.set_inner(Config {
        admin: accounts.admin.key(),
        collection: accounts.collection.key(),
        rewards_mint: accounts.rewards_mint.key(),
        rewards_per_day: args.rewards_per_day,
        freeze_period: args.freeze_period,
        burn_bonus: args.burn_bonus,
        crank_reward: args.crank_reward,
        crank_window: args.crank_window,
        open_hour: args.open_hour,
        close_hour: args.close_hour,
        bump: ctx.bumps.config,
        rewards_mint_bump: ctx.bumps.rewards_mint,
        oracle_bump: ctx.bumps.oracle,
        vault_bump: ctx.bumps.vault,
    });

    // Seed the oracle with the correct state for "now" so transfers are gated from day one.
    let transfer_state = if is_transfer_window_open(now, args.open_hour, args.close_hour) {
        ExternalValidationResult::Pass
    } else {
        ExternalValidationResult::Rejected
    };
    accounts.oracle.set_inner(Oracle {
        validation: OracleValidation::for_transfer(transfer_state),
        bump: ctx.bumps.oracle,
    });

    if args.vault_funding > 0 {
        assert_vault_deposit_ok(accounts.vault.lamports(), args.vault_funding)?;
        transfer(
            CpiContext::new(
                accounts.system_program.key(),
                Transfer {
                    from: accounts.admin.to_account_info(),
                    to: accounts.vault.to_account_info(),
                },
            ),
            args.vault_funding,
        )?;
    }

    // Collection: config PDA is update authority, carries the `total_staked` attribute
    // and an Oracle adapter that may reject Transfer.
    CreateCollectionV2CpiBuilder::new(&accounts.mpl_core_program.to_account_info())
        .collection(&accounts.collection.to_account_info())
        .update_authority(Some(&accounts.config.to_account_info()))
        .payer(&accounts.admin.to_account_info())
        .system_program(&accounts.system_program.to_account_info())
        .name(args.name)
        .uri(args.uri)
        .plugins(vec![PluginAuthorityPair {
            plugin: Plugin::Attributes(Attributes {
                attribute_list: vec![Attribute {
                    key: TOTAL_STAKED_KEY.to_string(),
                    value: "0".to_string(),
                }],
            }),
            authority: Some(PluginAuthority::UpdateAuthority),
        }])
        .external_plugin_adapters(vec![ExternalPluginAdapterInitInfo::Oracle(
            OracleInitInfo {
                base_address: accounts.oracle.key(),
                init_plugin_authority: Some(PluginAuthority::UpdateAuthority),
                lifecycle_checks: vec![(
                    HookableLifecycleEvent::Transfer,
                    ExternalCheckResult { flags: CAN_REJECT },
                )],
                base_address_config: None,
                results_offset: Some(ValidationResultsOffset::Anchor),
            },
        )])
        .invoke()?;

    Ok(())
}
