use anchor_lang::prelude::*;

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
