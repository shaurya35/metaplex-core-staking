use anchor_lang::prelude::*;

#[error_code]
pub enum StakingError {
    #[msg("Signer is not the owner of this asset")]
    NotOwner,
    #[msg("Asset does not belong to this collection")]
    WrongCollection,
    #[msg("Account is not a valid MPL Core account")]
    InvalidCoreAccount,
    #[msg("The asset is still inside its freeze period")]
    FreezePeriodNotOver,
    #[msg("There are no rewards to claim yet")]
    NothingToClaim,
    #[msg("Arithmetic overflow")]
    Overflow,
    #[msg("Collection is missing the total_staked attribute")]
    MissingStakedAttribute,
    #[msg("Collection staking counter would underflow")]
    CounterUnderflow,
    #[msg("Oracle already reflects the current transfer window")]
    OracleAlreadyUpToDate,
    #[msg("Invalid trading hours: need open < close <= 24")]
    InvalidHours,
    #[msg("Invalid config parameter")]
    InvalidParam,
}
