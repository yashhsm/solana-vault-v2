use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    extensions::{read_vault_extension, ExtensionType},
};

/// Vault extension: opt-in gate for primary-asset instant deposit/redeem.
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
#[repr(C)]
pub struct InstantSettlement {
    /// Minimum gross asset amount for instant deposits. Zero disables this bound.
    pub min_deposit_amount: u64,
    /// Maximum gross asset amount for instant deposits. Zero disables this bound.
    pub max_deposit_amount: u64,
    /// Minimum share amount for instant redemptions. Zero disables this bound.
    pub min_redeem_shares: u64,
    /// Maximum share amount for instant redemptions. Zero disables this bound.
    pub max_redeem_shares: u64,
    /// Maximum gross instant-deposit assets per user/window. Zero disables this bound.
    pub max_user_deposit_amount: u64,
    /// Maximum instant-redeem shares per user/window. Zero disables this bound.
    pub max_user_redeem_shares: u64,
    /// 0 = disabled, 1 = enabled. Current initializer always writes enabled.
    pub enabled: u8,
    pub _reserved: [u8; 7],
}

impl crate::extensions::VaultExtension for InstantSettlement {
    const EXTENSION_TYPE: ExtensionType = ExtensionType::InstantSettlement;
}

pub fn assert_instant_settlement_enabled(vault_info: &AccountInfo) -> Result<InstantSettlement> {
    let data = vault_info
        .data
        .try_borrow()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    let Some(ext) = read_vault_extension::<InstantSettlement>(&data)? else {
        return err!(AsyncVaultError::InstantSettlementDisabled);
    };
    require!(ext.enabled == 1, AsyncVaultError::InstantSettlementDisabled);
    Ok(ext)
}

pub fn validate_instant_settlement_thresholds(
    min_deposit_amount: u64,
    max_deposit_amount: u64,
    min_redeem_shares: u64,
    max_redeem_shares: u64,
) -> Result<()> {
    require!(
        max_deposit_amount == 0 || min_deposit_amount <= max_deposit_amount,
        AsyncVaultError::InvalidInstantSettlementThresholdConfig
    );
    require!(
        max_redeem_shares == 0 || min_redeem_shares <= max_redeem_shares,
        AsyncVaultError::InvalidInstantSettlementThresholdConfig
    );
    Ok(())
}

impl InstantSettlement {
    pub fn assert_deposit_amount(&self, amount: u64) -> Result<()> {
        require!(
            self.min_deposit_amount == 0 || amount >= self.min_deposit_amount,
            AsyncVaultError::InstantDepositAmountBelowMinimum
        );
        require!(
            self.max_deposit_amount == 0 || amount <= self.max_deposit_amount,
            AsyncVaultError::InstantDepositAmountAboveMaximum
        );
        Ok(())
    }

    pub fn assert_redeem_shares(&self, shares: u64) -> Result<()> {
        require!(
            self.min_redeem_shares == 0 || shares >= self.min_redeem_shares,
            AsyncVaultError::InstantRedeemSharesBelowMinimum
        );
        require!(
            self.max_redeem_shares == 0 || shares <= self.max_redeem_shares,
            AsyncVaultError::InstantRedeemSharesAboveMaximum
        );
        Ok(())
    }
}
