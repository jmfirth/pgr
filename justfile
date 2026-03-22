# pgr — justfile

# Format check + clippy
check:
    cargo fmt --check && cargo clippy --workspace -- -D warnings

# Fast test suite
test:
    cargo test --workspace --lib --bins && cargo test --workspace --doc

# Full test suite
test-all:
    cargo test --workspace

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
