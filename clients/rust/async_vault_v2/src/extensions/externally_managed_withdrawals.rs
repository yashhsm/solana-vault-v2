use super::{get_extension_bytes, ExtensionType, VAULT_TLV_START};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternallyManagedWithdrawals {
    pub enabled: bool,
}

pub fn get_state(vault_data: &[u8]) -> Option<ExternallyManagedWithdrawals> {
    if vault_data.len() <= VAULT_TLV_START {
        return None;
    }
    let bytes = get_extension_bytes(
        &vault_data[VAULT_TLV_START..],
        ExtensionType::ExternallyManagedWithdrawals,
    )?;
    Some(ExternallyManagedWithdrawals {
        enabled: bytes.first().copied() == Some(1),
    })
}

pub fn is_enabled(vault_data: &[u8]) -> bool {
    get_state(vault_data)
        .map(|state| state.enabled)
        .unwrap_or(false)
}
