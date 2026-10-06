use anchor_lang::prelude::*;

// Local mirrors of `mpl_core::types::{OracleValidation, ExternalValidationResult}`.
// The Borsh layout must match exactly: MPL Core reads the account at offset 8
// (`ValidationResultsOffset::Anchor`) and deserializes an `OracleValidation`.

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum ExternalValidationResult {
    Approved,
    Rejected,
    Pass,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum OracleValidation {
    Uninitialized,
    V1 {
        create: ExternalValidationResult,
        transfer: ExternalValidationResult,
        burn: ExternalValidationResult,
        update: ExternalValidationResult,
    },
}

impl OracleValidation {
    /// Only `transfer` is ever checked (the plugin is registered for Transfer only),
    /// the other events always pass.
    pub fn for_transfer(transfer: ExternalValidationResult) -> Self {
        OracleValidation::V1 {
            create: ExternalValidationResult::Pass,
            transfer,
            burn: ExternalValidationResult::Pass,
            update: ExternalValidationResult::Pass,
        }
    }

    pub fn transfer(&self) -> Option<ExternalValidationResult> {
        match self {
            OracleValidation::V1 { transfer, .. } => Some(*transfer),
            OracleValidation::Uninitialized => None,
        }
    }
}

/// Oracle account read by the MPL Core Oracle plugin adapter on the Collection.
/// `validation` must stay the first field so it sits right after the discriminator.
#[account]
#[derive(InitSpace)]
pub struct Oracle {
    pub validation: OracleValidation,
    pub bump: u8,
}
