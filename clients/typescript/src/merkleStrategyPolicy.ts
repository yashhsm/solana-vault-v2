import { getAddressEncoder, type Address, type ReadonlyUint8Array } from '@solana/kit';

import {
    ASYNC_VAULT_V2_PROGRAM_ADDRESS,
    type PolicyOperatorArgs,
    TokenBalanceAdapterAction,
    type TokenBalanceAdapterActionArgs,
} from './generated';

const LEAF_DOMAIN = new TextEncoder().encode('async-vault-v2/strategy-policy-leaf/v1');
const NODE_DOMAIN = new TextEncoder().encode('async-vault-v2/strategy-policy-node/v1');
const TOKEN_BALANCE_ADAPTER_LEAF_DOMAIN = new TextEncoder().encode('async-vault-v2/token-balance-adapter-leaf/v1');
const MAX_CPI_ACCOUNTS = 64;
const MAX_INSTRUCTION_DATA_LENGTH = 1024;
const MAX_OPERATORS = 32;
const MAX_INGESTED_INSTRUCTION_BYTES = 2048;
const MAX_POLICY_LEAVES = 1 << 12;
const MAX_U64 = (1n << 64n) - 1n;
const addressEncoder = getAddressEncoder();

export type StrategyPolicyAccountMeta = {
    address: Address;
    isSigner: boolean;
    isWritable: boolean;
};

export type StrategyPolicyLeafInput = {
    vault: Address;
    strategist: Address;
    policyVersion: number | bigint;
    targetProgram: Address;
    instructionData: ReadonlyUint8Array;
    accounts: readonly StrategyPolicyAccountMeta[];
    operators: readonly PolicyOperatorArgs[];
    programAddress?: Address;
};

export type TokenBalanceAdapterLeafInput = {
    vault: Address;
    strategist: Address;
    policyVersion: number | bigint;
    venueEntry: Address;
    vaultVenue: Address;
    targetProgram: Address;
    action: TokenBalanceAdapterActionArgs;
    assetMint: Address;
    vaultTokenAccount: Address;
    position: Address;
    positionTokenAccount: Address;
    policyMaxAmount: number | bigint;
    programAddress?: Address;
};

export type StrategyPolicyMerkleTree = {
    /** Lexicographically sorted leaves. Proof indexes refer to this order. */
    leaves: readonly ReadonlyUint8Array[];
    /** Level zero contains `leaves`; the last level contains exactly the root. */
    levels: readonly (readonly ReadonlyUint8Array[])[];
    root: ReadonlyUint8Array;
};

/**
 * Normalizes outer-transaction privileges to the privileges the venue CPI receives.
 * The vault PDA becomes a read-only signer through `invoke_signed`.
 */
export function normalizeStrategyPolicyAccounts(
    vault: Address,
    accounts: readonly StrategyPolicyAccountMeta[],
): StrategyPolicyAccountMeta[] {
    return accounts.map(account => ({
        ...account,
        isSigner: account.isSigner || account.address === vault,
        isWritable: account.address === vault ? false : account.isWritable,
    }));
}

/** Reconstructs the exact SHA-256 leaf verified by the on-chain program. */
export async function hashStrategyPolicyLeaf(input: StrategyPolicyLeafInput): Promise<ReadonlyUint8Array> {
    const instructionData = new Uint8Array(input.instructionData);
    assertRange(
        instructionData.length >= 8 && instructionData.length <= MAX_INSTRUCTION_DATA_LENGTH,
        'instructionData must contain at least an 8-byte discriminator and at most 1024 bytes',
    );
    assertRange(input.accounts.length <= MAX_CPI_ACCOUNTS, 'managed CPI has too many accounts');
    assertRange(input.operators.length <= MAX_OPERATORS, 'policy has too many operators');

    if (typeof input.policyVersion === 'number') {
        assertRange(
            Number.isSafeInteger(input.policyVersion),
            'numeric policyVersion must be a safe integer; use bigint for larger values',
        );
    }
    const version = BigInt(input.policyVersion);
    assertRange(version >= 0n && version <= MAX_U64, 'policyVersion must fit in a u64');
    const bytes: ReadonlyUint8Array[] = [
        LEAF_DOMAIN,
        addressEncoder.encode(input.programAddress ?? ASYNC_VAULT_V2_PROGRAM_ADDRESS),
        addressEncoder.encode(input.vault),
        addressEncoder.encode(input.strategist),
        encodeU64(version),
        addressEncoder.encode(input.targetProgram),
        instructionData.slice(0, 8),
        encodeU32(instructionData.length),
        encodeU16(input.accounts.length),
        Uint8Array.of(input.operators.length),
    ];

    let hasManagerLimitAmount = false;
    let ingestedInstructionBytes = 0;
    for (const operator of input.operators) {
        switch (operator.__kind) {
            case 'IngestInstruction': {
                assertRange(operator.length > 0, 'instruction ingestion length must be nonzero');
                const end = operator.offset + operator.length;
                assertRange(
                    Number.isInteger(operator.offset) &&
                        Number.isInteger(operator.length) &&
                        operator.offset >= 0 &&
                        operator.offset <= 0xffff &&
                        operator.length <= 0xffff &&
                        end <= instructionData.length,
                    'instruction ingestion range is out of bounds',
                );
                ingestedInstructionBytes += operator.length;
                assertRange(
                    ingestedInstructionBytes <= MAX_INGESTED_INSTRUCTION_BYTES,
                    'policy instruction ingestion exceeds the 2048-byte limit',
                );
                bytes.push(
                    Uint8Array.of(0),
                    encodeU16(operator.offset),
                    encodeU16(operator.length),
                    instructionData.slice(operator.offset, end),
                );
                break;
            }
            case 'IngestAccount': {
                assertRange(
                    Number.isInteger(operator.index) &&
                        operator.index >= 0 &&
                        operator.index <= 0xff &&
                        operator.index < input.accounts.length,
                    'account ingestion index is out of bounds',
                );
                const account = input.accounts[operator.index];
                bytes.push(
                    Uint8Array.of(1, operator.index),
                    addressEncoder.encode(account.address),
                    Uint8Array.of(Number(account.isSigner), Number(account.isWritable)),
                );
                break;
            }
            case 'ManagerLimitAmount': {
                assertRange(!hasManagerLimitAmount, 'only one manager-limit amount is allowed');
                assertRange(
                    Number.isInteger(operator.offset) &&
                        operator.offset >= 0 &&
                        operator.offset <= 0xffff &&
                        operator.offset + 8 <= instructionData.length,
                    'manager-limit amount range is out of bounds',
                );
                hasManagerLimitAmount = true;
                bytes.push(Uint8Array.of(2), encodeU16(operator.offset));
                break;
            }
        }
    }

    return sha256(concatBytes(bytes));
}

/** Reconstructs the exact leaf for the balance-checked SPL token position adapter. */
export async function hashTokenBalanceAdapterLeaf(input: TokenBalanceAdapterLeafInput): Promise<ReadonlyUint8Array> {
    const version = checkedU64(input.policyVersion, 'policyVersion');
    const policyMaxAmount = checkedU64(input.policyMaxAmount, 'policyMaxAmount');
    assertRange(policyMaxAmount > 0n, 'policyMaxAmount must be nonzero');
    const action = (() => {
        switch (input.action) {
            case TokenBalanceAdapterAction.Deploy:
                return 0;
            case TokenBalanceAdapterAction.Pull:
                return 1;
            default:
                throw new RangeError('unsupported token-balance adapter action');
        }
    })();

    return sha256(
        concatBytes([
            TOKEN_BALANCE_ADAPTER_LEAF_DOMAIN,
            addressEncoder.encode(input.programAddress ?? ASYNC_VAULT_V2_PROGRAM_ADDRESS),
            addressEncoder.encode(input.vault),
            addressEncoder.encode(input.strategist),
            encodeU64(version),
            addressEncoder.encode(input.venueEntry),
            addressEncoder.encode(input.vaultVenue),
            addressEncoder.encode(input.targetProgram),
            Uint8Array.of(action),
            addressEncoder.encode(input.assetMint),
            addressEncoder.encode(input.vaultTokenAccount),
            addressEncoder.encode(input.position),
            addressEncoder.encode(input.positionTokenAccount),
            encodeU64(policyMaxAmount),
        ]),
    );
}

/** Builds the same sorted-pair Merkle tree accepted by the on-chain verifier. */
export async function buildStrategyPolicyMerkleTree(
    leafHashes: readonly ReadonlyUint8Array[],
): Promise<StrategyPolicyMerkleTree> {
    assertRange(leafHashes.length > 0, 'at least one policy leaf is required');
    assertRange(leafHashes.length <= MAX_POLICY_LEAVES, 'policy tree exceeds the 4096-leaf proof-depth limit');
    let current: ReadonlyUint8Array[] = leafHashes.map(assertHash).sort(compareBytes);
    const levels: ReadonlyUint8Array[][] = [current];

    while (current.length > 1) {
        const next: ReadonlyUint8Array[] = [];
        for (let index = 0; index < current.length; index += 2) {
            const left = current[index];
            const right = current[index + 1] ?? left;
            next.push(await hashStrategyPolicyNode(left, right));
        }
        current = next;
        levels.push(current);
    }

    return { leaves: levels[0], levels, root: current[0] };
}

/** Returns a proof for a leaf's index in `tree.leaves` (the sorted leaf order). */
export function getStrategyPolicyProof(tree: StrategyPolicyMerkleTree, sortedLeafIndex: number): ReadonlyUint8Array[] {
    assertRange(
        Number.isInteger(sortedLeafIndex) && sortedLeafIndex >= 0 && sortedLeafIndex < tree.leaves.length,
        'sorted leaf index is out of bounds',
    );
    const proof: ReadonlyUint8Array[] = [];
    let index = sortedLeafIndex;
    for (let levelIndex = 0; levelIndex < tree.levels.length - 1; levelIndex += 1) {
        const level = tree.levels[levelIndex];
        const siblingIndex = index % 2 === 0 ? index + 1 : index - 1;
        proof.push(level[siblingIndex] ?? level[index]);
        index = Math.floor(index / 2);
    }
    return proof;
}

export async function hashStrategyPolicyNode(
    first: ReadonlyUint8Array,
    second: ReadonlyUint8Array,
): Promise<ReadonlyUint8Array> {
    const left = assertHash(first);
    const right = assertHash(second);
    const [lower, upper] = compareBytes(left, right) <= 0 ? [left, right] : [right, left];
    return sha256(concatBytes([NODE_DOMAIN, lower, upper]));
}

function assertHash(value: ReadonlyUint8Array): Uint8Array {
    assertRange(value.length === 32, 'Merkle hashes must contain exactly 32 bytes');
    return new Uint8Array(value);
}

function assertRange(condition: boolean, message: string): asserts condition {
    if (!condition) throw new RangeError(message);
}

function concatBytes(values: readonly ReadonlyUint8Array[]): Uint8Array {
    const length = values.reduce((total, value) => total + value.length, 0);
    const result = new Uint8Array(length);
    let offset = 0;
    for (const value of values) {
        result.set(value, offset);
        offset += value.length;
    }
    return result;
}

function compareBytes(left: ReadonlyUint8Array, right: ReadonlyUint8Array): number {
    for (let index = 0; index < Math.min(left.length, right.length); index += 1) {
        if (left[index] !== right[index]) return left[index] - right[index];
    }
    return left.length - right.length;
}

function encodeU16(value: number): Uint8Array {
    const bytes = new Uint8Array(2);
    new DataView(bytes.buffer).setUint16(0, value, true);
    return bytes;
}

function encodeU32(value: number): Uint8Array {
    const bytes = new Uint8Array(4);
    new DataView(bytes.buffer).setUint32(0, value, true);
    return bytes;
}

function encodeU64(value: bigint): Uint8Array {
    const bytes = new Uint8Array(8);
    new DataView(bytes.buffer).setBigUint64(0, value, true);
    return bytes;
}

function checkedU64(value: number | bigint, name: string): bigint {
    if (typeof value === 'number') {
        assertRange(
            Number.isSafeInteger(value),
            `numeric ${name} must be a safe integer; use bigint for larger values`,
        );
    }
    const result = BigInt(value);
    assertRange(result >= 0n && result <= MAX_U64, `${name} must fit in a u64`);
    return result;
}

async function sha256(bytes: Uint8Array): Promise<ReadonlyUint8Array> {
    const owned = bytes.slice().buffer as ArrayBuffer;
    return new Uint8Array(await globalThis.crypto.subtle.digest('SHA-256', owned));
}
