use anchor_lang::prelude::*;

use crate::state::Vault;

#[derive(AnchorDeserialize, AnchorSerialize)]
pub struct InviteNewAuthorityArgs {
    pub new_authority: Pubkey,
}

#[derive(Accounts)]
pub struct InviteNewAuthority<'info> {
    pub authority: Signer<'info>,

    #[account(mut)]
    pub vault: Account<'info, Vault>,
}

pub fn handler(ctx: Context<InviteNewAuthority>, args: InviteNewAuthorityArgs) -> Result<()> {
    ctx.accounts
        .vault
        .assert_curator(ctx.accounts.authority.key())?;
    ctx.accounts.vault.pending_authority = Some(args.new_authority);
    Ok(())
}
