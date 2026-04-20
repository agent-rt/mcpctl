set shell := ["bash", "-euo", "pipefail", "-c"]

# Default: show available recipes.
default:
    @just --list

# Install cmcp to ~/.cargo/bin (globally available on PATH).
install:
    cargo install --path . --locked --force

# Uninstall the globally installed cmcp binary.
uninstall:
    cargo uninstall cmcp

# Fast compile check.
check:
    cargo check --all-targets

# Run default (hermetic) tests.
test:
    cargo test

# Run end-to-end tests against live MCP servers (requires npx + network).
test-e2e:
    cargo test --test e2e_stdio -- --ignored --nocapture

# Lint with clippy (deny warnings).
lint:
    cargo clippy --all-targets -- -D warnings

# Format sources.
fmt:
    cargo fmt --all

# Check formatting (CI-friendly).
fmt-check:
    cargo fmt --all -- --check

# Release build.
build:
    cargo build --release

# Run cmcp with arbitrary args, e.g. `just run server list`.
run *ARGS:
    cargo run --quiet -- {{ARGS}}

# Full pre-commit gate.
ci: fmt-check lint test

# Cut a release: bumps version, commits, tags, pushes. Requires cargo-release.
# Usage: just release 0.1.1
release VERSION:
    @command -v cargo-release >/dev/null 2>&1 || { echo "install cargo-release first: cargo install cargo-release"; exit 1; }
    cargo release {{VERSION}} --execute
