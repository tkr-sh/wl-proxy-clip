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
