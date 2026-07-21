use anchor_lang::{
    prelude::*,
    solana_program::{entrypoint::ProgramResult, program::invoke_signed, system_instruction},
};
use anchor_spl::token_2022::spl_token_2022::{
    self,
    extension::{BaseStateWithExtensions, StateWithExtensions},
};
use vault_common::FeeType;

use crate::{
    error::AsyncVaultError,
    extensions::fifo_queue::{read_queue_request_id, QueueRequest},
    state::{
        InstantSettlementUser, ProtocolFeeConfig, RequestType, TrancheConfig, Vault,
        INSTANT_USER_LIMIT_SEED, PROTOCOL_FEE_CONFIG_SEED, TRANCHE_CONFIG_SEED,
    },
};

pub mod merkle;

#[derive(Clone, Copy)]
pub struct TrancheRequestInfo {
    pub senior_share_mint: Pubkey,
    pub junior_share_mint: Pubkey,
    pub senior_nav: u128,
    pub junior_nav: u128,
    pub senior_supply: u64,
    pub junior_supply: u64,
    pub min_junior_ratio_bps: u16,
    pub min_request_amounts: [u64; crate::state::TRANCHE_REQUEST_LIMIT_COUNT],
    pub max_request_amounts: [u64; crate::state::TRANCHE_REQUEST_LIMIT_COUNT],
}

pub struct RequestShareMintContext {
    pub consumed_accounts: usize,
    pub tranche_nav: Option<u128>,
    pub tranche_info: Option<TrancheRequestInfo>,
}

pub fn split_protocol_fee(total_fee: u64, protocol_fee_bps: u16) -> Result<(u64, u64)> {
    if total_fee == 0 || protocol_fee_bps == 0 {
        return Ok((0, total_fee));
    }

    let protocol_fee = FeeType::Percentage {
        bps: protocol_fee_bps,
    }
    .get_fee(total_fee)
    .map_err(AsyncVaultError::from)?;
    let fee_recipient_fee = total_fee
        .checked_sub(protocol_fee)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    Ok((protocol_fee, fee_recipient_fee))
}

pub fn is_protocol_fee_config_account(info: &AccountInfo) -> bool {
    let (expected, _) = Pubkey::find_program_address(&[PROTOCOL_FEE_CONFIG_SEED], &crate::ID);
    *info.key == expected && *info.owner == crate::ID
}

pub fn protocol_fee_recipient_from_config<'info>(
    info: &'info AccountInfo<'info>,
) -> Result<Pubkey> {
    let (expected, expected_bump) =
        Pubkey::find_program_address(&[PROTOCOL_FEE_CONFIG_SEED], &crate::ID);
    require_keys_eq!(*info.key, expected, AsyncVaultError::InvalidVault);
    let config: Account<ProtocolFeeConfig> = Account::try_from(info)?;
    require!(config.bump == expected_bump, AsyncVaultError::InvalidVault);
    require!(
        config.protocol_fee_recipient != Pubkey::default(),
        AsyncVaultError::InvalidFeeRecipient
    );
    Ok(config.protocol_fee_recipient)
}

pub fn resolve_protocol_fee_recipient<'info>(
    vault: &Vault,
    maybe_config_info: Option<&'info AccountInfo<'info>>,
) -> Result<(Pubkey, bool)> {
    if let Some(config_info) = maybe_config_info {
        if is_protocol_fee_config_account(config_info) {
            return Ok((protocol_fee_recipient_from_config(config_info)?, true));
        }
    }

    Ok((vault.protocol_fee_recipient, false))
}

pub fn validate_request_share_mint<'info>(
    vault: &Vault,
    vault_key: Pubkey,
    selected_share_mint: Pubkey,
    remaining_accounts: &'info [AccountInfo<'info>],
) -> Result<RequestShareMintContext> {
    let Some(expected_tranche_config) = vault.tranche_config else {
        require_keys_eq!(
            selected_share_mint,
            vault.share_mint,
            AsyncVaultError::InvalidShareMint
        );
        return Ok(RequestShareMintContext {
            consumed_accounts: 0,
            tranche_nav: None,
            tranche_info: None,
        });
    };

    let tranche_config_info = remaining_accounts
        .first()
        .ok_or(AsyncVaultError::MissingRequiredAccount)?;
    require_keys_eq!(
        *tranche_config_info.key,
        expected_tranche_config,
        AsyncVaultError::InvalidVault
    );

    let (expected_pda, _) =
        Pubkey::find_program_address(&[TRANCHE_CONFIG_SEED, vault_key.as_ref()], &crate::ID);
    require_keys_eq!(
        expected_tranche_config,
        expected_pda,
        AsyncVaultError::InvalidVault
    );

    let tranche_config: Account<TrancheConfig> = Account::try_from(tranche_config_info)?;
    require_keys_eq!(
        tranche_config.vault,
        vault_key,
        AsyncVaultError::InvalidVault
    );

    let tranche_nav = if selected_share_mint == tranche_config.senior_share_mint {
        tranche_config.senior_nav
    } else if selected_share_mint == tranche_config.junior_share_mint {
        tranche_config.junior_nav
    } else {
        return Err(AsyncVaultError::InvalidShareMint.into());
    };

    Ok(RequestShareMintContext {
        consumed_accounts: 1,
        tranche_nav: Some(tranche_nav),
        tranche_info: Some(TrancheRequestInfo {
            senior_share_mint: tranche_config.senior_share_mint,
            junior_share_mint: tranche_config.junior_share_mint,
            senior_nav: tranche_config.senior_nav,
            junior_nav: tranche_config.junior_nav,
            senior_supply: tranche_config.senior_supply,
            junior_supply: tranche_config.junior_supply,
            min_junior_ratio_bps: tranche_config.min_junior_ratio_bps,
            min_request_amounts: tranche_config.min_request_amounts,
            max_request_amounts: tranche_config.max_request_amounts,
        }),
    })
}

pub fn enforce_tranche_request_limits(
    context: &RequestShareMintContext,
    selected_share_mint: Pubkey,
    request_type: RequestType,
    amount: u64,
) -> Result<()> {
    let Some(tranche_info) = context.tranche_info else {
        return Ok(());
    };
    let limit_index = if selected_share_mint == tranche_info.senior_share_mint {
        if matches!(request_type, RequestType::Deposit) {
            0
        } else {
            1
        }
    } else if selected_share_mint == tranche_info.junior_share_mint {
        if matches!(request_type, RequestType::Deposit) {
            2
        } else {
            3
        }
    } else {
        return Err(AsyncVaultError::InvalidShareMint.into());
    };

    let min_amount = tranche_info.min_request_amounts[limit_index];
    require!(
        min_amount == 0 || amount >= min_amount,
        AsyncVaultError::TrancheRequestAmountBelowMinimum
    );
    let max_amount = tranche_info.max_request_amounts[limit_index];
    require!(
        max_amount == 0 || amount <= max_amount,
        AsyncVaultError::TrancheRequestAmountAboveMaximum
    );

    Ok(())
}

pub fn load_or_init_instant_settlement_user<'info>(
    instant_user_info: &'info AccountInfo<'info>,
    payer_info: AccountInfo<'info>,
    system_program_info: AccountInfo<'info>,
    vault: Pubkey,
    user: Pubkey,
    bump: u8,
) -> Result<Account<'info, InstantSettlementUser>> {
    require!(
        instant_user_info.is_writable,
        AsyncVaultError::MissingRequiredAccount
    );

    let (expected, expected_bump) = Pubkey::find_program_address(
        &[INSTANT_USER_LIMIT_SEED, vault.as_ref(), user.as_ref()],
        &crate::ID,
    );
    require_keys_eq!(
        *instant_user_info.key,
        expected,
        AsyncVaultError::InvalidVault
    );
    require!(bump == expected_bump, AsyncVaultError::InvalidVault);

    let space = 8 + InstantSettlementUser::INIT_SPACE;
    if instant_user_info.data_is_empty() {
        require_keys_eq!(
            *instant_user_info.owner,
            System::id(),
            AsyncVaultError::InvalidVault
        );
        let lamports = Rent::get()?.minimum_balance(space);
        let vault_key = vault;
        let user_key = user;
        let signer_seeds: &[&[u8]] = &[
            INSTANT_USER_LIMIT_SEED,
            vault_key.as_ref(),
            user_key.as_ref(),
            &[bump],
        ];
        invoke_signed(
            &system_instruction::create_account(
                payer_info.key,
                instant_user_info.key,
                lamports,
                space as u64,
                &crate::ID,
            ),
            &[payer_info, instant_user_info.clone(), system_program_info],
            &[signer_seeds],
        )?;
        let mut account = Account::<InstantSettlementUser>::try_from_unchecked(instant_user_info)?;
        account.ensure_initialized(vault, user, bump)?;
        return Ok(account);
    }

    let mut account = Account::<InstantSettlementUser>::try_from(instant_user_info)?;
    account.ensure_initialized(vault, user, bump)?;
    Ok(account)
}

pub fn next_tranche_queue_request_id<'remaining>(
    context: &RequestShareMintContext,
    selected_share_mint: Pubkey,
    request_type: RequestType,
    remaining_accounts: &'remaining [AccountInfo<'remaining>],
) -> Result<Option<u64>> {
    if context.tranche_info.is_none() {
        return Ok(None);
    }

    let tranche_config_info = remaining_accounts
        .first()
        .ok_or(AsyncVaultError::MissingRequiredAccount)?;
    require!(
        tranche_config_info.is_writable,
        AsyncVaultError::MissingRequiredAccount
    );
    let mut tranche_config: Account<TrancheConfig> = Account::try_from(tranche_config_info)?;
    let id = tranche_config.next_queue_request_id(selected_share_mint, request_type)?;
    tranche_config.exit(&crate::ID)?;
    Ok(Some(id))
}

pub fn check_and_advance_tranche_queue<'request, 'remaining, R: QueueRequest>(
    context: &RequestShareMintContext,
    selected_share_mint: Pubkey,
    request_type: RequestType,
    request_info: &AccountInfo<'request>,
    remaining_accounts: &'remaining [AccountInfo<'remaining>],
) -> Result<bool> {
    if context.tranche_info.is_none() {
        return Ok(false);
    }

    let tranche_config_info = remaining_accounts
        .first()
        .ok_or(AsyncVaultError::MissingRequiredAccount)?;
    require!(
        tranche_config_info.is_writable,
        AsyncVaultError::MissingRequiredAccount
    );
    let request_id = read_queue_request_id::<R>(request_info)?;
    let mut tranche_config: Account<TrancheConfig> = Account::try_from(tranche_config_info)?;
    tranche_config.check_and_advance_queue(selected_share_mint, request_type, request_id)?;
    tranche_config.exit(&crate::ID)?;
    Ok(true)
}

/// Validates the extensions on the asset mint to ensure compatibility with the
/// Async Vault program. This is checked during Vault Creation and Deposit/DepositRequest.
/// - TransferFeeConfig can be enabled, but must be 0. Many stablecoins have TransferFeeConfig
///   enabled with a 0 fee, so this is important to support.
pub fn validate_asset_mint_extensions_from_acct_info(mint_acct: &AccountInfo) -> Result<()> {
    if mint_acct.owner != &spl_token_2022::ID {
        return Ok(());
    }
    let mint_data = mint_acct.try_borrow_data()?;
    let mint = StateWithExtensions::<spl_token_2022::state::Mint>::unpack(&mint_data)?;

    // Validate: Mint has 0 transfer fees
    if let Ok(transfer_fee_config) =
        mint.get_extension::<spl_token_2022::extension::transfer_fee::TransferFeeConfig>()
    {
        let clock = Clock::get()?;
        let transfer_fee_bps = u16::from_le_bytes(
            transfer_fee_config
                .get_epoch_fee(clock.epoch)
                .transfer_fee_basis_points
                .0,
        );
        if transfer_fee_bps != 0 {
            return Err(AsyncVaultError::InvalidAssetMintExtensions.into());
        }
    }

    Ok(())
}

/// Validates the share mint's extensions against the vault's public mint/burn share lifecycle.
/// Rejects `ConfidentialMintBurn`, under which public `mint_to`/`burn` fail and would brick
/// claims and redemptions.
pub fn validate_share_mint_extensions_from_acct_info(mint_acct: &AccountInfo) -> Result<()> {
    if mint_acct.owner != &spl_token_2022::ID {
        return Ok(());
    }
    let mint_data = mint_acct.try_borrow_data()?;
    let mint = StateWithExtensions::<spl_token_2022::state::Mint>::unpack(&mint_data)?;

    if mint
        .get_extension::<spl_token_2022::extension::confidential_mint_burn::ConfidentialMintBurn>()
        .is_ok()
    {
        return Err(AsyncVaultError::InvalidShareMintExtensions.into());
    }

    Ok(())
}

/// Read the `owner` Pubkey of a TokenAccount without deserializing the whole account
/// and validate against an expected owner.
pub fn validate_token_account_owner(info: &AccountInfo, expected_owner: &Pubkey) -> ProgramResult {
    let data = info.try_borrow_data()?;
    if data.len() < 64 {
        return Err(ProgramError::InvalidAccountData);
    }
    let owner = Pubkey::new_from_array(
        data[32..64]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?,
    );
    if owner.ne(expected_owner) {
        return Err(ProgramError::InvalidAccountOwner);
    }
    Ok(())
}

/// Read the `mint` and `owner` Pubkeys of a TokenAccount without deserializing
/// the whole account and validate against expected values.
pub fn validate_token_account_mint_and_owner(
    info: &AccountInfo,
    expected_mint: &Pubkey,
    expected_owner: &Pubkey,
) -> ProgramResult {
    let data = info.try_borrow_data()?;
    if data.len() < 64 {
        return Err(ProgramError::InvalidAccountData);
    }
    let mint = Pubkey::new_from_array(
        data[0..32]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?,
    );
    let owner = Pubkey::new_from_array(
        data[32..64]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?,
    );
    if mint.ne(expected_mint) {
        return Err(ProgramError::InvalidAccountData);
    }
    if owner.ne(expected_owner) {
        return Err(ProgramError::InvalidAccountOwner);
    }
    Ok(())
}

/// Read SPL Token or Token-2022 mint supply and decimals from the stable base
/// mint layout without requiring a typed account in the instruction.
pub fn read_mint_supply_and_decimals(info: &AccountInfo) -> Result<(u64, u8)> {
    let data = info.try_borrow_data()?;
    if data.len() < 45 {
        return Err(ProgramError::InvalidAccountData.into());
    }
    let supply = u64::from_le_bytes(
        data[36..44]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?,
    );
    let decimals = data[44];
    Ok((supply, decimals))
}

/// Converts an asset amount into shares using the supplied NAV.
/// Rounding: floored
/// `shares = net_amount * 10^decimals / nav`
pub fn calculate_shares(nav: u128, decimals: u8, net_amount: u64) -> Result<u64> {
    let precision = 10u128
        .checked_pow(decimals as u32)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    let shares = u128::from(net_amount)
        .checked_mul(precision)
        .ok_or(AsyncVaultError::ArithmeticError)?
        .checked_div(nav)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    Ok(u64::try_from(shares).map_err(|_| AsyncVaultError::ArithmeticError)?)
}

/// Converts a share amount into assets using the supplied NAV.
/// Rounding: floored
/// `assets = share_amount * nav / 10^decimals`
pub fn calculate_assets(nav: u128, decimals: u8, share_amount: u64) -> Result<u64> {
    let precision = 10u128
        .checked_pow(decimals as u32)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    let assets = u128::from(share_amount)
        .checked_mul(nav)
        .ok_or(AsyncVaultError::ArithmeticError)?
        .checked_div(precision)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    if assets.eq(&0u128) {
        return Err(AsyncVaultError::ArithmeticError.into());
    }
    Ok(u64::try_from(assets).map_err(|_| AsyncVaultError::ArithmeticError)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use test_case::test_case;

    #[test_case(1_000_000u128, 6u8, 1_000_000u64 => 1_000_000u64; "one_to_one")]
    #[test_case(2_000_000u128, 6u8, 2_000_000u64 => 1_000_000u64; "nav_above_one")]
    #[test_case(3_000_000u128, 6u8, 1_000_000u64 => 333_333u64; "fractional_truncates")]
    #[test_case(1_000_000u128, 6u8, 0u64 => 0u64; "zero_amount")]
    #[test_case(100_000_000u128, 8u8, 100_000_000u64 => 100_000_000u64; "different_decimals")]
    #[test_case(1_000_000u128, 6u8, u64::MAX => u64::MAX; "large_amount_no_overflow")]
    fn calculate_shares_success(nav: u128, decimals: u8, amount: u64) -> u64 {
        calculate_shares(nav, decimals, amount).unwrap()
    }

    #[test]
    fn calculate_shares_zero_nav_errors() {
        assert!(calculate_shares(0, 6, 1_000_000).is_err());
    }

    #[test_case(1_000_000u128, 6u8, 1_000_000u64 => 1_000_000u64; "one_to_one")]
    #[test_case(2_000_000u128, 6u8, 1_000_000u64 => 2_000_000u64; "nav_above_one")]
    #[test_case(3_000_000u128, 6u8, 333_333u64 => 999_999u64; "fractional_truncates")]
    #[test_case(100_000_000u128, 8u8, 100_000_000u64 => 100_000_000u64; "different_decimals")]
    fn calculate_assets_success(nav: u128, decimals: u8, shares: u64) -> u64 {
        calculate_assets(nav, decimals, shares).unwrap()
    }

    #[test]
    fn calculate_assets_zero_nav_errors() {
        assert!(calculate_assets(0, 6, 1_000_000).is_err());
    }

    #[test]
    fn calculate_assets_zero_shares_errors() {
        assert!(calculate_assets(1_000_000, 6, 0).is_err());
    }

    proptest! {
        #[test]
        fn asset_share_round_trip_never_mints_value(
            decimals in 0u8..=9,
            nav_multiplier in 1u128..=1_000_000,
            amount in 0u64..=1_000_000_000_000,
        ) {
            let precision = 10u128.pow(u32::from(decimals));
            let nav = precision
                .checked_mul(nav_multiplier)
                .expect("bounded nav does not overflow");
            let shares = calculate_shares(nav, decimals, amount).unwrap();
            let max_single_share_assets = u64::try_from(nav_multiplier).unwrap();

            if shares == 0 {
                prop_assert!(amount <= max_single_share_assets);
                return Ok(());
            }

            let assets_back = calculate_assets(nav, decimals, shares).unwrap();
            prop_assert!(assets_back <= amount);
            prop_assert!(amount - assets_back <= max_single_share_assets);
        }
    }
}
