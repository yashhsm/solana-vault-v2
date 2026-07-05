use anchor_lang::prelude::*;
use anchor_spl::{
    token, token_2022,
    token_interface::{self, MintTo},
};

use crate::{
    error::AsyncVaultError,
    state::{TrancheConfig, Vault, TRANCHE_CONFIG_SEED, VAULT_CONFIG_SEED},
    utils::{
        read_mint_supply_and_decimals, resolve_protocol_fee_recipient, split_protocol_fee,
        validate_token_account_mint_and_owner,
    },
};

const SECONDS_PER_YEAR: u128 = 31_536_000;

#[derive(Accounts)]
pub struct UpdateVaultNav<'info> {
    pub authority: Signer<'info>,

    #[account(mut)]
    pub vault: Account<'info, Vault>,
}

#[derive(Clone, Copy)]
struct TrancheMintSnapshot {
    supply: u64,
    decimals: u8,
}

#[derive(Clone, Copy)]
struct TrancheWaterfallState {
    senior_nav: u128,
    junior_nav: u128,
    senior_target_bps: u16,
    last_waterfall_timestamp: i64,
}

#[derive(Clone, Copy)]
struct TrancheWaterfallOutcome {
    updated_total_assets: u128,
    senior_assets: u128,
    junior_assets: u128,
    senior_nav: u128,
    junior_nav: u128,
}

pub fn handler<'info>(ctx: Context<'info, UpdateVaultNav<'info>>, updated_nav: u128) -> Result<()> {
    let clock = Clock::get()?;
    let vault = &mut ctx.accounts.vault;
    vault.assert_curator_or_fulfiller(ctx.accounts.authority.key())?;
    vault.validate_nav_update(updated_nav, clock.unix_timestamp)?;

    let fee_accounts_used = maybe_crystallize_performance_fee(
        vault,
        updated_nav,
        clock.unix_timestamp,
        ctx.remaining_accounts,
    )?;
    maybe_apply_tranche_waterfall(
        vault,
        updated_nav,
        clock.slot,
        clock.unix_timestamp,
        &ctx.remaining_accounts[fee_accounts_used..],
    )?;

    vault.nav = updated_nav;
    vault.nav_version = vault
        .nav_version
        .checked_add(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    vault.last_nav_update_slot = clock.slot;
    vault.last_nav_update_timestamp = clock.unix_timestamp;
    Ok(())
}

fn maybe_crystallize_performance_fee<'info>(
    vault: &mut Account<'info, Vault>,
    updated_nav: u128,
    timestamp: i64,
    remaining_accounts: &'info [AccountInfo<'info>],
) -> Result<usize> {
    if updated_nav <= vault.high_water_mark {
        return Ok(0);
    }

    let fee_bps = vault.performance_fee_bps;
    let previous_high_water_mark = vault.high_water_mark;
    if fee_bps > 0 && previous_high_water_mark > 0 {
        let interval_seconds = vault.performance_fee_crystallization_interval_seconds;
        if interval_seconds > 0 {
            let elapsed_seconds = timestamp
                .checked_sub(vault.last_fee_crystallization_timestamp)
                .ok_or(AsyncVaultError::ArithmeticError)?;
            if u64::try_from(elapsed_seconds).unwrap_or(0) < interval_seconds {
                return Ok(0);
            }
        }
    }

    vault.high_water_mark = updated_nav;
    vault.last_fee_crystallization_timestamp = timestamp;

    if fee_bps == 0 || previous_high_water_mark == 0 {
        return Ok(0);
    }

    let share_mint_info = remaining_accounts
        .first()
        .ok_or(AsyncVaultError::MissingRequiredAccount)?;
    let fee_recipient_share_account_info = remaining_accounts
        .get(1)
        .ok_or(AsyncVaultError::MissingRequiredAccount)?;
    let protocol_fee_enabled = vault.protocol_fee_bps > 0;
    let mut next_account_index = 2usize;
    let protocol_fee_recipient = if protocol_fee_enabled {
        let (recipient, consumed_protocol_fee_config) =
            resolve_protocol_fee_recipient(vault, remaining_accounts.get(next_account_index))?;
        if consumed_protocol_fee_config {
            next_account_index = next_account_index
                .checked_add(1)
                .ok_or(AsyncVaultError::ArithmeticError)?;
        }
        Some(recipient)
    } else {
        None
    };
    let protocol_fee_share_account_info = if protocol_fee_enabled {
        let account = remaining_accounts
            .get(next_account_index)
            .ok_or(AsyncVaultError::MissingRequiredAccount)?;
        next_account_index = next_account_index
            .checked_add(1)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        Some(account)
    } else {
        None
    };
    let share_token_program_index = next_account_index;
    let share_token_program_info = remaining_accounts
        .get(share_token_program_index)
        .ok_or(AsyncVaultError::MissingRequiredAccount)?;

    require!(
        *share_token_program_info.key == token::ID
            || *share_token_program_info.key == token_2022::ID,
        AsyncVaultError::InvalidShareMint
    );
    require_keys_eq!(
        *share_mint_info.key,
        vault.share_mint,
        AsyncVaultError::InvalidShareMint
    );
    require_keys_eq!(
        *share_mint_info.owner,
        *share_token_program_info.key,
        AsyncVaultError::InvalidShareMint
    );
    validate_token_account_mint_and_owner(
        fee_recipient_share_account_info,
        &vault.share_mint,
        &vault.fee_recipient,
    )?;

    let (share_supply, decimals) = read_mint_supply_and_decimals(share_mint_info)?;
    let fee_shares = calculate_performance_fee_shares(
        updated_nav,
        previous_high_water_mark,
        fee_bps,
        share_supply,
        decimals,
    )?;
    if fee_shares == 0 {
        return Ok(share_token_program_index + 1);
    }
    let (protocol_fee_shares, fee_recipient_shares) =
        split_protocol_fee(fee_shares, vault.protocol_fee_bps)?;
    if let Some(protocol_fee_share_account_info) = protocol_fee_share_account_info {
        let protocol_fee_recipient =
            protocol_fee_recipient.ok_or(AsyncVaultError::MissingRequiredAccount)?;
        validate_token_account_mint_and_owner(
            protocol_fee_share_account_info,
            &vault.share_mint,
            &protocol_fee_recipient,
        )?;
    }

    let share_mint_key = vault.share_mint;
    let vault_bump = vault.bump;
    let seeds: &[&[&[u8]]] = &[&[VAULT_CONFIG_SEED, share_mint_key.as_ref(), &[vault_bump]]];
    if fee_recipient_shares > 0 {
        token_interface::mint_to(
            CpiContext::new_with_signer(
                *share_token_program_info.key,
                MintTo {
                    mint: share_mint_info.to_account_info(),
                    to: fee_recipient_share_account_info.to_account_info(),
                    authority: vault.to_account_info(),
                },
                seeds,
            ),
            fee_recipient_shares,
        )?;
    }
    if protocol_fee_shares > 0 {
        let protocol_fee_share_account_info =
            protocol_fee_share_account_info.ok_or(AsyncVaultError::MissingRequiredAccount)?;
        token_interface::mint_to(
            CpiContext::new_with_signer(
                *share_token_program_info.key,
                MintTo {
                    mint: share_mint_info.to_account_info(),
                    to: protocol_fee_share_account_info.to_account_info(),
                    authority: vault.to_account_info(),
                },
                seeds,
            ),
            protocol_fee_shares,
        )?;
    }
    Ok(share_token_program_index + 1)
}

fn maybe_apply_tranche_waterfall<'info>(
    vault: &mut Account<'info, Vault>,
    updated_nav: u128,
    slot: u64,
    timestamp: i64,
    remaining_accounts: &'info [AccountInfo<'info>],
) -> Result<()> {
    let Some(expected_tranche_config) = vault.tranche_config else {
        return Ok(());
    };

    let tranche_config_info = remaining_accounts
        .first()
        .ok_or(AsyncVaultError::MissingRequiredAccount)?;
    let senior_share_mint_info = remaining_accounts
        .get(1)
        .ok_or(AsyncVaultError::MissingRequiredAccount)?;
    let junior_share_mint_info = remaining_accounts
        .get(2)
        .ok_or(AsyncVaultError::MissingRequiredAccount)?;

    require!(
        tranche_config_info.is_writable,
        AsyncVaultError::MissingRequiredAccount
    );
    require_keys_eq!(
        *tranche_config_info.key,
        expected_tranche_config,
        AsyncVaultError::InvalidVault
    );
    let vault_key = vault.key();
    let (expected_pda, _) =
        Pubkey::find_program_address(&[TRANCHE_CONFIG_SEED, vault_key.as_ref()], &crate::ID);
    require_keys_eq!(
        expected_tranche_config,
        expected_pda,
        AsyncVaultError::InvalidVault
    );

    let mut tranche_config: Account<TrancheConfig> = Account::try_from(tranche_config_info)?;
    require_keys_eq!(
        tranche_config.vault,
        vault_key,
        AsyncVaultError::InvalidVault
    );
    require_keys_eq!(
        tranche_config.senior_share_mint,
        *senior_share_mint_info.key,
        AsyncVaultError::InvalidShareMint
    );
    require_keys_eq!(
        tranche_config.junior_share_mint,
        *junior_share_mint_info.key,
        AsyncVaultError::InvalidShareMint
    );
    require!(
        *senior_share_mint_info.owner == token::ID
            || *senior_share_mint_info.owner == token_2022::ID,
        AsyncVaultError::InvalidShareMint
    );
    require!(
        *junior_share_mint_info.owner == token::ID
            || *junior_share_mint_info.owner == token_2022::ID,
        AsyncVaultError::InvalidShareMint
    );

    let (senior_supply, senior_decimals) = read_mint_supply_and_decimals(senior_share_mint_info)?;
    let (junior_supply, junior_decimals) = read_mint_supply_and_decimals(junior_share_mint_info)?;
    let senior = TrancheMintSnapshot {
        supply: senior_supply.max(tranche_config.senior_supply),
        decimals: senior_decimals,
    };
    let junior = TrancheMintSnapshot {
        supply: junior_supply.max(tranche_config.junior_supply),
        decimals: junior_decimals,
    };

    apply_tranche_waterfall(
        &mut tranche_config,
        updated_nav,
        slot,
        timestamp,
        senior,
        junior,
    )?;
    tranche_config.exit(&crate::ID)?;
    Ok(())
}

fn apply_tranche_waterfall(
    tranche_config: &mut Account<TrancheConfig>,
    updated_nav: u128,
    slot: u64,
    timestamp: i64,
    senior: TrancheMintSnapshot,
    junior: TrancheMintSnapshot,
) -> Result<()> {
    let first_waterfall = tranche_config.last_waterfall_slot == 0
        && tranche_config.senior_nav == 0
        && tranche_config.junior_nav == 0;

    if first_waterfall {
        tranche_config.senior_nav = if senior.supply == 0 { 0 } else { updated_nav };
        tranche_config.junior_nav = if junior.supply == 0 { 0 } else { updated_nav };
        tranche_config.senior_supply = senior.supply;
        tranche_config.junior_supply = junior.supply;
        tranche_config.last_waterfall_slot = slot;
        tranche_config.last_waterfall_timestamp = timestamp;
        return Ok(());
    }

    let outcome = compute_tranche_waterfall(
        TrancheWaterfallState {
            senior_nav: tranche_config.senior_nav,
            junior_nav: tranche_config.junior_nav,
            senior_target_bps: tranche_config.senior_target_bps,
            last_waterfall_timestamp: tranche_config.last_waterfall_timestamp,
        },
        updated_nav,
        timestamp,
        senior,
        junior,
    )?;
    let conserved_assets = outcome
        .senior_assets
        .checked_add(outcome.junior_assets)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    require!(
        conserved_assets == outcome.updated_total_assets,
        AsyncVaultError::ArithmeticError
    );

    tranche_config.senior_nav = outcome.senior_nav;
    tranche_config.junior_nav = outcome.junior_nav;
    tranche_config.senior_supply = senior.supply;
    tranche_config.junior_supply = junior.supply;
    tranche_config.last_waterfall_slot = slot;
    tranche_config.last_waterfall_timestamp = timestamp;
    Ok(())
}

fn compute_tranche_waterfall(
    state: TrancheWaterfallState,
    updated_nav: u128,
    timestamp: i64,
    senior: TrancheMintSnapshot,
    junior: TrancheMintSnapshot,
) -> Result<TrancheWaterfallOutcome> {
    let previous_senior_assets = assets_from_nav(senior.supply, state.senior_nav, senior.decimals)?;
    let previous_junior_assets = assets_from_nav(junior.supply, state.junior_nav, junior.decimals)?;
    let previous_total_assets = previous_senior_assets
        .checked_add(previous_junior_assets)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    let updated_total_assets = assets_from_nav(senior.supply, updated_nav, senior.decimals)?
        .checked_add(assets_from_nav(
            junior.supply,
            updated_nav,
            junior.decimals,
        )?)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    let (next_senior_assets, next_junior_assets) = if updated_total_assets >= previous_total_assets
    {
        let gain = updated_total_assets
            .checked_sub(previous_total_assets)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        let elapsed_seconds = timestamp
            .checked_sub(state.last_waterfall_timestamp)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        let target_accrual = if elapsed_seconds <= 0 || state.senior_target_bps == 0 {
            0
        } else {
            previous_senior_assets
                .checked_mul(u128::from(state.senior_target_bps))
                .ok_or(AsyncVaultError::ArithmeticError)?
                .checked_mul(elapsed_seconds as u128)
                .ok_or(AsyncVaultError::ArithmeticError)?
                .checked_div(u128::from(vault_common::MAX_BPS))
                .ok_or(AsyncVaultError::ArithmeticError)?
                .checked_div(SECONDS_PER_YEAR)
                .ok_or(AsyncVaultError::ArithmeticError)?
        };
        let senior_credit = gain.min(target_accrual);
        let senior_assets = previous_senior_assets
            .checked_add(senior_credit)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        let junior_assets = updated_total_assets
            .checked_sub(senior_assets)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        (senior_assets, junior_assets)
    } else {
        let loss = previous_total_assets
            .checked_sub(updated_total_assets)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        let junior_loss = loss.min(previous_junior_assets);
        let junior_assets = previous_junior_assets
            .checked_sub(junior_loss)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        let senior_loss = loss
            .checked_sub(junior_loss)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        let senior_assets = previous_senior_assets
            .checked_sub(senior_loss)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        (senior_assets, junior_assets)
    };

    Ok(TrancheWaterfallOutcome {
        updated_total_assets,
        senior_assets: next_senior_assets,
        junior_assets: next_junior_assets,
        senior_nav: nav_from_assets(next_senior_assets, senior.supply, senior.decimals)?,
        junior_nav: nav_from_assets(next_junior_assets, junior.supply, junior.decimals)?,
    })
}

fn assets_from_nav(supply: u64, nav: u128, decimals: u8) -> Result<u128> {
    if supply == 0 || nav == 0 {
        return Ok(0);
    }
    let precision = 10u128
        .checked_pow(decimals as u32)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    u128::from(supply)
        .checked_mul(nav)
        .ok_or(AsyncVaultError::ArithmeticError)?
        .checked_div(precision)
        .ok_or(AsyncVaultError::ArithmeticError.into())
}

fn nav_from_assets(assets: u128, supply: u64, decimals: u8) -> Result<u128> {
    if supply == 0 {
        return Ok(0);
    }
    let precision = 10u128
        .checked_pow(decimals as u32)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    assets
        .checked_mul(precision)
        .ok_or(AsyncVaultError::ArithmeticError)?
        .checked_div(u128::from(supply))
        .ok_or(AsyncVaultError::ArithmeticError.into())
}

fn calculate_performance_fee_shares(
    updated_nav: u128,
    high_water_mark: u128,
    fee_bps: u16,
    share_supply: u64,
    decimals: u8,
) -> Result<u64> {
    if fee_bps == 0 || share_supply == 0 || updated_nav <= high_water_mark {
        return Ok(0);
    }

    let precision = 10u128
        .checked_pow(decimals as u32)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    let supply = u128::from(share_supply);
    let profit_per_share = updated_nav
        .checked_sub(high_water_mark)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    let gross_profit_assets = supply
        .checked_mul(profit_per_share)
        .ok_or(AsyncVaultError::ArithmeticError)?
        .checked_div(precision)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    let fee_assets = gross_profit_assets
        .checked_mul(u128::from(fee_bps))
        .ok_or(AsyncVaultError::ArithmeticError)?
        .checked_div(u128::from(vault_common::MAX_BPS))
        .ok_or(AsyncVaultError::ArithmeticError)?;
    if fee_assets == 0 {
        return Ok(0);
    }

    let total_assets = supply
        .checked_mul(updated_nav)
        .ok_or(AsyncVaultError::ArithmeticError)?
        .checked_div(precision)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    let denominator = total_assets
        .checked_sub(fee_assets)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    require!(denominator > 0, AsyncVaultError::ArithmeticError);
    let fee_shares = fee_assets
        .checked_mul(supply)
        .ok_or(AsyncVaultError::ArithmeticError)?
        .checked_div(denominator)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    Ok(u64::try_from(fee_shares).map_err(|_| AsyncVaultError::ArithmeticError)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const MAX_TEST_SUPPLY: u64 = 1_000_000_000;
    const MAX_TEST_NAV: u128 = 1_000_000_000_000;

    fn ceil_div(numerator: u128, denominator: u128) -> u128 {
        numerator
            .checked_add(denominator - 1)
            .expect("bounded test numerator should not overflow")
            / denominator
    }

    fn one_nav_unit_asset_bound(supply: u64, decimals: u8) -> u128 {
        let precision = 10u128
            .checked_pow(u32::from(decimals))
            .expect("bounded test decimals should not overflow");
        ceil_div(u128::from(supply), precision).saturating_add(1)
    }

    fn loss_case_strategy() -> impl Strategy<Value = (u64, u64, u8, u128, u128, u16)> {
        (
            1u64..=MAX_TEST_SUPPLY,
            1u64..=MAX_TEST_SUPPLY,
            0u8..=9,
            2u128..=MAX_TEST_NAV,
            0u16..=vault_common::MAX_BPS,
        )
            .prop_flat_map(
                |(senior_supply, junior_supply, decimals, previous_nav, senior_target_bps)| {
                    (
                        Just(senior_supply),
                        Just(junior_supply),
                        Just(decimals),
                        Just(previous_nav),
                        1u128..previous_nav,
                        Just(senior_target_bps),
                    )
                },
            )
    }

    proptest! {
        #[test]
        fn tranche_waterfall_conserves_allocated_assets_and_rounding_favors_pool(
            senior_supply in 1u64..=MAX_TEST_SUPPLY,
            junior_supply in 1u64..=MAX_TEST_SUPPLY,
            decimals in 0u8..=9,
            senior_nav in 1u128..=MAX_TEST_NAV,
            junior_nav in 1u128..=MAX_TEST_NAV,
            updated_nav in 1u128..=MAX_TEST_NAV,
            senior_target_bps in 0u16..=vault_common::MAX_BPS,
            elapsed_seconds in 0i64..=SECONDS_PER_YEAR as i64,
        ) {
            let senior = TrancheMintSnapshot {
                supply: senior_supply,
                decimals,
            };
            let junior = TrancheMintSnapshot {
                supply: junior_supply,
                decimals,
            };
            let outcome = compute_tranche_waterfall(
                TrancheWaterfallState {
                    senior_nav,
                    junior_nav,
                    senior_target_bps,
                    last_waterfall_timestamp: 0,
                },
                updated_nav,
                elapsed_seconds,
                senior,
                junior,
            )
            .expect("bounded property inputs should not overflow");

            prop_assert_eq!(
                outcome.senior_assets
                    .checked_add(outcome.junior_assets)
                    .expect("bounded allocation should not overflow"),
                outcome.updated_total_assets
            );

            let recorded_senior_assets =
                assets_from_nav(senior_supply, outcome.senior_nav, decimals)
                    .expect("bounded senior NAV should convert");
            let recorded_junior_assets =
                assets_from_nav(junior_supply, outcome.junior_nav, decimals)
                    .expect("bounded junior NAV should convert");
            let recorded_total_assets = recorded_senior_assets
                .checked_add(recorded_junior_assets)
                .expect("bounded recorded total should not overflow");

            prop_assert!(recorded_total_assets <= outcome.updated_total_assets);
            let rounding_dust = outcome
                .updated_total_assets
                .checked_sub(recorded_total_assets)
                .expect("recorded assets should not exceed allocated assets");
            let dust_bound = one_nav_unit_asset_bound(senior_supply, decimals)
                .checked_add(one_nav_unit_asset_bound(junior_supply, decimals))
                .expect("bounded dust bound should not overflow");
            prop_assert!(rounding_dust <= dust_bound);
        }

        #[test]
        fn tranche_waterfall_losses_hit_junior_before_senior(
            (senior_supply, junior_supply, decimals, previous_nav, updated_nav, senior_target_bps)
                in loss_case_strategy(),
        ) {
            let senior = TrancheMintSnapshot {
                supply: senior_supply,
                decimals,
            };
            let junior = TrancheMintSnapshot {
                supply: junior_supply,
                decimals,
            };
            let previous_senior_assets =
                assets_from_nav(senior_supply, previous_nav, decimals)
                    .expect("bounded senior previous assets should convert");
            let previous_junior_assets =
                assets_from_nav(junior_supply, previous_nav, decimals)
                    .expect("bounded junior previous assets should convert");
            let previous_total_assets = previous_senior_assets
                .checked_add(previous_junior_assets)
                .expect("bounded previous total should not overflow");

            let outcome = compute_tranche_waterfall(
                TrancheWaterfallState {
                    senior_nav: previous_nav,
                    junior_nav: previous_nav,
                    senior_target_bps,
                    last_waterfall_timestamp: 0,
                },
                updated_nav,
                1,
                senior,
                junior,
            )
            .expect("bounded property inputs should not overflow");

            if outcome.updated_total_assets < previous_total_assets {
                let loss = previous_total_assets
                    .checked_sub(outcome.updated_total_assets)
                    .expect("loss branch should subtract");
                if loss <= previous_junior_assets {
                    prop_assert_eq!(outcome.senior_assets, previous_senior_assets);
                    prop_assert_eq!(
                        outcome.junior_assets,
                        previous_junior_assets
                            .checked_sub(loss)
                            .expect("junior absorbs bounded loss")
                    );
                } else {
                    prop_assert_eq!(outcome.junior_assets, 0);
                    prop_assert_eq!(
                        outcome.senior_assets,
                        previous_senior_assets
                            .checked_sub(
                                loss.checked_sub(previous_junior_assets)
                                    .expect("residual loss should be nonnegative"),
                            )
                            .expect("senior absorbs residual bounded loss")
                    );
                }
            }
        }
    }
}
