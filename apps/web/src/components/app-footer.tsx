export function AppFooter() {
    return (
        <footer className="mx-auto w-full max-w-7xl px-6 py-12 text-center text-xs text-muted-foreground">
            <p>
                Built on the{' '}
                <a
                    href="https://github.com/sendai/solana-vault-v2"
                    className="underline-offset-4 hover:underline"
                    target="_blank"
                    rel="noreferrer"
                >
                    Vault Standard Suite
                </a>{' '}
                from solana-foundation/vault. This demo is unaffiliated and unaudited.
            </p>
        </footer>
    );
}
