# pgr — justfile

# Format check + clippy
check:
    cargo fmt --check && cargo clippy --workspace -- -D warnings

# Fast test suite
test:
    cargo test --workspace --lib --bins && cargo test --workspace --doc

# Full test suite (includes slow PTY/integration tests)
test-all:
    cargo test --workspace -- --include-ignored

# Conformance tests only (PTY comparison against GNU less)
# Set LESS_BIN to point to a specific less binary, e.g.:
#   LESS_BIN=/opt/homebrew/Cellar/less/692/bin/less just conformance
conformance:
    LESS_BIN="${LESS_BIN:-less}" cargo test -p pgr-cli --test conformance -- --include-ignored

# Debug build
build:
    cargo build --workspace

# Release build
release:
    cargo build --workspace --release

# Coverage (placeholder)
coverage:
    @echo "Coverage tooling TBD"

# Full CI pipeline
ci:
    just check && just test-all && cargo doc --workspace --no-deps

# Build and open docs
doc:
    cargo doc --workspace --no-deps --open

# Run pgr with pass-through args
start *ARGS:
    cargo run --package pgr-cli -- {{ARGS}}

# Clean build artifacts
clean:
    cargo clean

# Set up git hooks
setup:
    git config core.hooksPath .githooks
