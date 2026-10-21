set shell := ["nu", "-c"]

# Enter in a nix-shell to dev
[group: 'dev']
env:
    nix develop -c nu

# Watch
[group: 'dev']
w:
    bacon

[group: 'dev']
nix-w:
    nix develop -c "bacon"

# Run
[group: 'dev']
run:
    cargo run

[group: 'dev']
nix-run:
    nix develop -c "cargo" "run"

# Check
[group: 'check']
check:
    cargo check
    cargo fmt --check
    try { cargo nextest run }
    cargo clippy
    taplo fmt --check


[group: 'check']
fix: fmt
    cargo clippy --fix --allow-staged

[group: 'check']
fmt:
    cargo fmt
    taplo fmt
    nix fmt flake.nix

# CI

[group: 'ci']
ci-fmt:
    cargo fmt --check
    taplo fmt --check

[group: 'ci']
ci-machete:
    cargo machete

[group: 'ci']
ci-deny:
    cargo deny check advisories bans licenses sources

[group: 'ci']
ci-check:
    cargo check --locked --all-targets

[group: 'ci']
ci-clippy:
    cargo clippy --locked --all-targets -- -D warnings

[group: 'ci']
ci-test:
    cargo nextest run --locked

[group: 'ci']
ci-build:
    cargo build --locked --release

[group: 'ci']
ci: ci-fmt ci-machete ci-deny ci-check ci-clippy ci-test ci-build

[group: 'release']
release version:
    nu ./scripts/release.nu {{version}}
