# Install dependencies
install:
    pnpm install

# Build the program and refresh the committed IDL (idl/async_vault_v2.json)
generate-idl:
    anchor build --ignore-keys
    cp target/idl/async_vault_v2.json idl/async_vault_v2.json

# Generate Rust + TypeScript clients from the committed IDL
generate-clients:
    pnpm run generate-clients

# Full build: IDL + clients
build: generate-idl generate-clients

# Format and lint everything
fmt:
    cargo +nightly fmt -p async_vault_v2 -p vault_common -p integration-tests
    cargo clippy -p async_vault_v2 -p vault_common -p integration-tests
    pnpm format
    pnpm lint:fix

# Verify formatting, lint, and types without modifying files
check:
    cargo +nightly fmt -p async_vault_v2 -p vault_common -p integration-tests -- --check
    cargo clippy -p async_vault_v2 -p vault_common -p integration-tests
    pnpm run format:check
    pnpm lint
    just typecheck

# TypeScript type checking
typecheck:
    pnpm --filter @sendai/solana-vault-v2 typecheck

# Run unit tests
unit-test:
    cargo test -p async_vault_v2 -p vault_common

# Run integration tests (LiteSVM)
integration-test *args:
    cargo test -p integration-tests {{ args }}

# Run all tests
test *args: build unit-test (integration-test args)
