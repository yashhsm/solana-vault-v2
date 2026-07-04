use anchor_lang::prelude::*;

use crate::{error::AsyncVaultError, state::Vault};

#[derive(Accounts)]
pub struct AcceptAuthorityInvitation<'info> {
    pub new_authority: Signer<'info>,

    #[account(mut)]
    pub vault: Account<'info, Vault>,
}

pub fn handler(ctx: Context<AcceptAuthorityInvitation>) -> Result<()> {
    let vault = &mut ctx.accounts.vault;

    let pending = vault
        .pending_authority
        .ok_or(AsyncVaultError::NoPendingAuthority)?;

    require_keys_eq!(
        ctx.accounts.new_authority.key(),
        pending,
        AsyncVaultError::UnauthorizedSigner
    );

    let previous_authority = vault.authority;
    vault.authority = pending;
    vault.curator = pending;
    if vault.manager == previous_authority {
        vault.manager = pending;
    }
    if vault.hot_manager == previous_authority {
        vault.hot_manager = pending;
    }
    if vault.fulfiller == previous_authority {
        vault.fulfiller = pending;
    }
    if vault.breaker == previous_authority {
        vault.breaker = pending;
    }
    vault.pending_authority = None;
    Ok(())
}
