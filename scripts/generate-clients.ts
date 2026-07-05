/**
 * Generates Rust and TypeScript clients from the Anchor IDL.
 */

import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { AnchorIdl } from '@codama/nodes-from-anchor';
import { rootNodeFromAnchor } from '@codama/nodes-from-anchor';
import { renderVisitor as renderJavaScriptVisitor } from '@codama/renderers-js';
import { renderVisitor as renderRustVisitor } from '@codama/renderers-rust';
import { createFromRoot, deduplicateIdenticalDefinedTypesVisitor, updateDefinedTypesVisitor } from 'codama';

const projectRoot = join(dirname(fileURLToPath(import.meta.url)), '..');

const idl = JSON.parse(readFileSync(join(projectRoot, 'idl/async_vault_v2.json'), 'utf-8')) as AnchorIdl;
const codama = createFromRoot(rootNodeFromAnchor(idl));
codama.update(deduplicateIdenticalDefinedTypesVisitor());

const rustCrateFolder = join(projectRoot, 'clients/rust/async_vault_v2');
codama.accept(
    renderRustVisitor(rustCrateFolder, {
        formatCode: false,
        syncCargoToml: false,
    }),
);
patchRustUpdateVaultBuilder(rustCrateFolder);
execFileSync('cargo', ['+nightly', 'fmt', '-p', 'async-vault-v2-client'], { cwd: projectRoot, stdio: 'inherit' });
console.log('Rust client generated at:', join(rustCrateFolder, 'src/generated'));

codama.update(updateDefinedTypesVisitor({ RequestArgs: { name: 'CreateRequestArgs' } }));

const tsPackageFolder = join(projectRoot, 'clients/typescript');
void codama.accept(
    renderJavaScriptVisitor(tsPackageFolder, {
        kitImportStrategy: 'rootOnly',
        syncPackageJson: false,
    }),
);
console.log('TypeScript client generated at:', join(tsPackageFolder, 'src/generated'));

function patchRustUpdateVaultBuilder(rustCrateFolder: string): void {
    const path = join(rustCrateFolder, 'src/generated/instructions/update_vault.rs');
    let source = readFileSync(path, 'utf-8');
    if (source.includes('pub fn paused(&mut self, paused: bool) -> &mut Self')) {
        return;
    }

    const helper = `
fn empty_update_vault_args() -> UpdateVaultArgs {
    UpdateVaultArgs {
        paused: None,
        fee_recipient: None,
        manager: None,
        hot_manager: None,
        fulfiller: None,
        breaker: None,
        nav_mode: None,
        require_fresh_nav: None,
        max_nav_delta_bps: None,
        max_implied_apy_bps: None,
        max_nav_staleness_slots: None,
        deposit_cap: None,
        rolling_limit_window_slots: None,
        manager_rolling_limit: None,
        external_withdraw_rolling_limit: None,
        redemption_rolling_limit: None,
        timelock_delay_slots: None,
        performance_fee_bps: None,
        performance_fee_crystallization_interval_seconds: None,
        protocol_fee_bps: None,
        protocol_fee_recipient: None,
        instant_redemption_fee_bps: None,
    }
}
`;

    const setters = `
    #[inline(always)]
    pub fn paused(&mut self, paused: bool) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).paused = Some(paused);
        self
    }

    #[inline(always)]
    pub fn fee_recipient(&mut self, fee_recipient: solana_address::Address) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).fee_recipient = Some(fee_recipient);
        self
    }

    #[inline(always)]
    pub fn manager(&mut self, manager: solana_address::Address) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).manager = Some(manager);
        self
    }

    #[inline(always)]
    pub fn hot_manager(&mut self, hot_manager: solana_address::Address) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).hot_manager = Some(hot_manager);
        self
    }

    #[inline(always)]
    pub fn fulfiller(&mut self, fulfiller: solana_address::Address) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).fulfiller = Some(fulfiller);
        self
    }

    #[inline(always)]
    pub fn breaker(&mut self, breaker: solana_address::Address) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).breaker = Some(breaker);
        self
    }

    #[inline(always)]
    pub fn nav_mode(&mut self, nav_mode: crate::generated::types::NavMode) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).nav_mode = Some(nav_mode);
        self
    }

    #[inline(always)]
    pub fn require_fresh_nav(&mut self, require_fresh_nav: bool) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).require_fresh_nav = Some(require_fresh_nav);
        self
    }

    #[inline(always)]
    pub fn max_nav_delta_bps(&mut self, max_nav_delta_bps: u16) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).max_nav_delta_bps = Some(max_nav_delta_bps);
        self
    }

    #[inline(always)]
    pub fn max_implied_apy_bps(&mut self, max_implied_apy_bps: u32) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).max_implied_apy_bps = Some(max_implied_apy_bps);
        self
    }

    #[inline(always)]
    pub fn max_nav_staleness_slots(&mut self, max_nav_staleness_slots: u64) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).max_nav_staleness_slots = Some(max_nav_staleness_slots);
        self
    }

    #[inline(always)]
    pub fn deposit_cap(&mut self, deposit_cap: u64) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).deposit_cap = Some(deposit_cap);
        self
    }

    #[inline(always)]
    pub fn rolling_limit_window_slots(&mut self, rolling_limit_window_slots: u64) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).rolling_limit_window_slots = Some(rolling_limit_window_slots);
        self
    }

    #[inline(always)]
    pub fn manager_rolling_limit(&mut self, manager_rolling_limit: u64) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).manager_rolling_limit = Some(manager_rolling_limit);
        self
    }

    #[inline(always)]
    pub fn external_withdraw_rolling_limit(&mut self, external_withdraw_rolling_limit: u64) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).external_withdraw_rolling_limit = Some(external_withdraw_rolling_limit);
        self
    }

    #[inline(always)]
    pub fn redemption_rolling_limit(&mut self, redemption_rolling_limit: u64) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).redemption_rolling_limit = Some(redemption_rolling_limit);
        self
    }

    #[inline(always)]
    pub fn timelock_delay_slots(&mut self, timelock_delay_slots: u64) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).timelock_delay_slots = Some(timelock_delay_slots);
        self
    }

    #[inline(always)]
    pub fn performance_fee_bps(&mut self, performance_fee_bps: u16) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).performance_fee_bps = Some(performance_fee_bps);
        self
    }

    #[inline(always)]
    pub fn performance_fee_crystallization_interval_seconds(
        &mut self,
        performance_fee_crystallization_interval_seconds: u64,
    ) -> &mut Self {
        self.args
            .get_or_insert_with(empty_update_vault_args)
            .performance_fee_crystallization_interval_seconds =
            Some(performance_fee_crystallization_interval_seconds);
        self
    }

    #[inline(always)]
    pub fn protocol_fee_bps(&mut self, protocol_fee_bps: u16) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).protocol_fee_bps = Some(protocol_fee_bps);
        self
    }

    #[inline(always)]
    pub fn protocol_fee_recipient(&mut self, protocol_fee_recipient: solana_address::Address) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).protocol_fee_recipient = Some(protocol_fee_recipient);
        self
    }

    #[inline(always)]
    pub fn instant_redemption_fee_bps(&mut self, instant_redemption_fee_bps: u16) -> &mut Self {
        self.args.get_or_insert_with(empty_update_vault_args).instant_redemption_fee_bps = Some(instant_redemption_fee_bps);
        self
    }

`;

    source = source.replace(
        '\n/// Instruction builder for `UpdateVault`.',
        `${helper}\n/// Instruction builder for \`UpdateVault\`.`,
    );
    source = source.replace(
        '    /// Add an additional account to the instruction.\n',
        `${setters}    /// Add an additional account to the instruction.\n`,
    );
    source = source.replace(
        'args: self.args.clone().expect("args is not set"),',
        'args: self.args.clone().unwrap_or_else(empty_update_vault_args),',
    );
    writeFileSync(path, source);
}
