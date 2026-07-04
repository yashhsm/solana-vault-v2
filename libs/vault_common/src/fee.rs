use anchor_lang::prelude::{AnchorDeserialize, AnchorSerialize, InitSpace};

use crate::{constants::MAX_BPS, error::VaultMathError};

/// The fee types:
/// FixedAmount: a fixed fee is applied (ex 0.1 asset)
/// Percentage: the fee is a % of the transfer amount
#[derive(AnchorDeserialize, AnchorSerialize, Clone, InitSpace, Copy)]
pub enum FeeType {
    FixedAmount { amount: u64 },
    Percentage { bps: u16 },
}

impl FeeType {
    /// Ensures percentage fees don't exceed MAX_BPS.
    pub fn validate(self) -> Result<(), VaultMathError> {
        if let FeeType::Percentage { bps } = self {
            if bps > MAX_BPS {
                return Err(VaultMathError::FeeBpsLimitReached);
            }
        }
        Ok(())
    }

    /// Computes the fee from a known total amount (fee-inclusive).
    /// Rounds up for percentage fees.
    pub fn get_fee(self, total_amount: u64) -> Result<u64, VaultMathError> {
        match self {
            FeeType::Percentage { bps } => {
                let fee = u128::from(total_amount)
                    .checked_mul(u128::from(bps))
                    .ok_or(VaultMathError::ArithmeticError)?
                    .checked_add(9_999)
                    .ok_or(VaultMathError::ArithmeticError)?
                    .checked_div(10_000)
                    .ok_or(VaultMathError::ArithmeticError)?;
                u64::try_from(fee).map_err(|_| VaultMathError::ArithmeticError)
            }
            FeeType::FixedAmount { amount } => Ok(amount),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn get_fee_percentage_rounds_up() {
        assert_eq!(
            FeeType::Percentage { bps: 100 }.get_fee(1_000_000).unwrap(),
            10_000
        );
    }

    #[test]
    fn get_fee_percentage_large_amount_does_not_overflow() {
        let fee = FeeType::Percentage { bps: 100 }
            .get_fee(2_000_000_000_000_000_000)
            .unwrap();
        assert_eq!(fee, 20_000_000_000_000_000);
    }

    #[test]
    fn get_fee_percentage_max_amount_full_bps() {
        assert_eq!(
            FeeType::Percentage { bps: MAX_BPS }
                .get_fee(u64::MAX)
                .unwrap(),
            u64::MAX
        );
    }

    #[test]
    fn get_fee_fixed_passes_through() {
        assert_eq!(
            FeeType::FixedAmount { amount: 42 }
                .get_fee(1_000_000)
                .unwrap(),
            42
        );
    }

    proptest! {
        #[test]
        fn percentage_fee_matches_round_up_formula(
            amount in 0u64..=u64::MAX,
            bps in 0u16..=MAX_BPS,
        ) {
            let fee = FeeType::Percentage { bps }.get_fee(amount).unwrap();
            let expected = u128::from(amount)
                .checked_mul(u128::from(bps))
                .unwrap()
                .checked_add(9_999)
                .unwrap()
                .checked_div(10_000)
                .unwrap();
            prop_assert_eq!(u128::from(fee), expected);
            prop_assert!(fee <= amount);
            if bps == 0 {
                prop_assert_eq!(fee, 0);
            }
            if bps == MAX_BPS {
                prop_assert_eq!(fee, amount);
            }
        }

        #[test]
        fn fixed_fee_passes_through_for_any_amount(
            fixed_amount in 0u64..=u64::MAX,
            total_amount in 0u64..=u64::MAX,
        ) {
            prop_assert_eq!(
                FeeType::FixedAmount {
                    amount: fixed_amount,
                }
                .get_fee(total_amount)
                .unwrap(),
                fixed_amount
            );
        }
    }
}
