import assert from 'node:assert/strict';

import { address, type ReadonlyUint8Array } from '@solana/kit';

import {
    buildStrategyPolicyMerkleTree,
    getStrategyPolicyProof,
    hashStrategyPolicyLeaf,
    hashTokenBalanceAdapterLeaf,
    normalizeStrategyPolicyAccounts,
    type PolicyOperatorArgs,
    TokenBalanceAdapterAction,
} from '../clients/typescript/src/index.js';

async function main(): Promise<void> {
    const vault = address('11111111111111111111111111111111');
    const strategist = address('SysvarC1ock11111111111111111111111111111111');
    const targetProgram = address('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA');
    const accounts = normalizeStrategyPolicyAccounts(vault, [
        { address: vault, isSigner: false, isWritable: true },
        { address: targetProgram, isSigner: false, isWritable: false },
    ]);
    const instructionData = Uint8Array.from({ length: 16 }, (_, index) => index + 1);
    const operators: PolicyOperatorArgs[] = [
        { __kind: 'IngestInstruction', length: 4, offset: 8 },
        { __kind: 'IngestAccount', index: 0 },
        { __kind: 'IngestAccount', index: 1 },
        { __kind: 'ManagerLimitAmount', offset: 8 },
    ];

    const leaf = await hashStrategyPolicyLeaf({
        accounts,
        instructionData,
        operators,
        policyVersion: 7,
        strategist,
        targetProgram,
        vault,
    });
    const tree = await buildStrategyPolicyMerkleTree([leaf, new Uint8Array(32).fill(7), new Uint8Array(32).fill(9)]);
    const leafIndex = tree.leaves.findIndex(candidate => hex(candidate) === hex(leaf));
    const proof = getStrategyPolicyProof(tree, leafIndex);

    assert.equal(hex(leaf), 'd925c25360e7d800278163511eb9742bedbb5fdf9640323afa9814c3840d77f0');
    assert.equal(hex(tree.root), '8b27c6b7da89dfc5deb2a320214be3e77371e19ea5abae2bd8d585d1d09fe12a');
    assert.deepEqual(proof.map(hex), [
        'd925c25360e7d800278163511eb9742bedbb5fdf9640323afa9814c3840d77f0',
        '8b88aadfa6a0dc942d584d37d30895e891fb6e34900b5d30ebd679b33ef282e5',
    ]);

    const adapterLeaf = await hashTokenBalanceAdapterLeaf({
        action: TokenBalanceAdapterAction.Deploy,
        assetMint: address('Vote111111111111111111111111111111111111111'),
        policyMaxAmount: 250,
        policyVersion: 7,
        position: address('ComputeBudget111111111111111111111111111111'),
        positionTokenAccount: strategist,
        strategist,
        targetProgram,
        vault,
        vaultTokenAccount: address('Stake11111111111111111111111111111111111111'),
        vaultVenue: address('BPFLoaderUpgradeab1e11111111111111111111111'),
        venueEntry: address('ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL'),
    });
    assert.equal(hex(adapterLeaf), '4c0920146de6af2cfc03030a24a4cf10d835a76bdafccc86394602928c4f066e');
}

function hex(value: ReadonlyUint8Array): string {
    return Array.from(value, byte => byte.toString(16).padStart(2, '0')).join('');
}

void main();
