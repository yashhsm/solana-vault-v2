use super::{get_extension_bytes, ExtensionType, VAULT_TLV_START};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstantSettlement {
    pub min_deposit_amount: u64,
    pub max_deposit_amount: u64,
    pub min_redeem_shares: u64,
    pub max_redeem_shares: u64,
    pub max_user_deposit_amount: u64,
    pub max_user_redeem_shares: u64,
    pub enabled: bool,
}

pub fn get_state(vault_data: &[u8]) -> Option<InstantSettlement> {
    if vault_data.len() <= VAULT_TLV_START {
        return None;
    }
    let bytes = get_extension_bytes(
        &vault_data[VAULT_TLV_START..],
        ExtensionType::InstantSettlement,
    )?;
    if bytes.len() < 56 {
        return None;
    }
    Some(InstantSettlement {
        min_deposit_amount: u64::from_le_bytes(bytes[0..8].try_into().ok()?),
        max_deposit_amount: u64::from_le_bytes(bytes[8..16].try_into().ok()?),
        min_redeem_shares: u64::from_le_bytes(bytes[16..24].try_into().ok()?),
        max_redeem_shares: u64::from_le_bytes(bytes[24..32].try_into().ok()?),
        max_user_deposit_amount: u64::from_le_bytes(bytes[32..40].try_into().ok()?),
        max_user_redeem_shares: u64::from_le_bytes(bytes[40..48].try_into().ok()?),
        enabled: bytes.get(48).copied() == Some(1),
    })
}

pub fn is_enabled(vault_data: &[u8]) -> bool {
    get_state(vault_data)
        .map(|state| state.enabled)
        .unwrap_or(false)
}
