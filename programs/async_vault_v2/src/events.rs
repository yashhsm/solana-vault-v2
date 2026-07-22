use anchor_lang::prelude::*;

#[event]
pub struct ProtocolFeeGovernanceInitialized {
    pub protocol_fee_config: Pubkey,
    pub protocol_fee_governance: Pubkey,
    pub authority: Pubkey,
    pub breaker: Pubkey,
    pub timelock_delay_slots: u64,
}

#[event]
pub struct ProtocolFeeConfigUpdateQueued {
    pub protocol_fee_config: Pubkey,
    pub protocol_fee_governance: Pubkey,
    pub pending_update: Pubkey,
    pub queued_by: Pubkey,
    pub expected_version: u64,
    pub eta_slot: u64,
    pub protocol_fee_recipient: Pubkey,
    pub timelock_delay_slots: Option<u64>,
}

#[event]
pub struct ProtocolFeeConfigUpdated {
    pub protocol_fee_config: Pubkey,
    pub protocol_fee_governance: Pubkey,
    pub authority: Pubkey,
    pub protocol_fee_recipient: Pubkey,
    pub timelock_delay_slots: u64,
    pub version: u64,
}

#[event]
pub struct ProtocolFeeConfigPaused {
    pub protocol_fee_config: Pubkey,
    pub protocol_fee_governance: Pubkey,
    pub authority: Pubkey,
    pub version: u64,
}

#[event]
pub struct ProtocolFeeAuthorityTransferQueued {
    pub protocol_fee_config: Pubkey,
    pub protocol_fee_governance: Pubkey,
    pub pending_transfer: Pubkey,
    pub current_authority: Pubkey,
    pub new_authority: Pubkey,
    pub expected_version: u64,
    pub eta_slot: u64,
}

#[event]
pub struct ProtocolFeeAuthorityTransferred {
    pub protocol_fee_config: Pubkey,
    pub protocol_fee_governance: Pubkey,
    pub previous_authority: Pubkey,
    pub new_authority: Pubkey,
    pub version: u64,
}

#[event]
pub struct StrategyPolicyInitialized {
    pub vault: Pubkey,
    pub strategist: Pubkey,
    pub strategy_policy: Pubkey,
}

#[event]
pub struct StrategyPolicyUpdateQueued {
    pub vault: Pubkey,
    pub strategy_policy: Pubkey,
    pub pending_update: Pubkey,
    pub expected_version: u64,
    pub eta_slot: u64,
    pub merkle_root: [u8; 32],
    pub paused: bool,
}

#[event]
pub struct StrategyPolicyUpdated {
    pub vault: Pubkey,
    pub strategy_policy: Pubkey,
    pub strategist: Pubkey,
    pub old_root: [u8; 32],
    pub new_root: [u8; 32],
    pub version: u64,
    pub paused: bool,
}

#[event]
pub struct StrategyPolicyPaused {
    pub vault: Pubkey,
    pub strategy_policy: Pubkey,
    pub strategist: Pubkey,
    pub authority: Pubkey,
    pub version: u64,
}

#[event]
pub struct StrategyActionExecuted {
    pub vault: Pubkey,
    pub strategy_policy: Pubkey,
    pub strategist: Pubkey,
    pub venue_entry: Pubkey,
    pub target_program: Pubkey,
    pub policy_version: u64,
    pub leaf: [u8; 32],
    pub cpi_account_count: u16,
    pub manager_limit_amount: Option<u64>,
}
