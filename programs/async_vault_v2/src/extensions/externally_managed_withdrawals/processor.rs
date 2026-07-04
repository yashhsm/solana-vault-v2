use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    extensions::{read_vault_extension, ExtensionType},
};

/// Vault extension: opt-in gate for free-form externally managed withdrawals.
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
#[repr(C)]
pub struct ExternallyManagedWithdrawals {
    /// 0 = disabled, 1 = enabled. Current initializer always writes enabled.
    pub enabled: u8,
}

impl crate::extensions::VaultExtension for ExternallyManagedWithdrawals {
    const EXTENSION_TYPE: ExtensionType = ExtensionType::ExternallyManagedWithdrawals;
}

pub fn assert_externally_managed_withdrawals_enabled(vault_info: &AccountInfo) -> Result<()> {
    let data = vault_info
        .data
        .try_borrow()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    let Some(ext) = read_vault_extension::<ExternallyManagedWithdrawals>(&data)? else {
        return err!(AsyncVaultError::ExternallyManagedWithdrawalsDisabled);
    };
    require!(
        ext.enabled == 1,
        AsyncVaultError::ExternallyManagedWithdrawalsDisabled
    );
    Ok(())
}
