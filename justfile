# pgr — justfile

# --- Quality checks ---

# Format check + clippy (default features)
check:
    cargo fmt --check && cargo clippy --workspace -- -D warnings

# Format check + clippy for both feature profiles
check-all:
    cargo fmt --check
    cargo clippy --workspace -- -D warnings
    cargo clippy --workspace --no-default-features -- -D warnings

# --- Test suites ---

# Fast test suite (unit + doc tests, <10s)
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

# Visual correctness tests (content modes, rendering, new features)
visual:
    cargo test -p pgr-cli --test visual -- --include-ignored

# Test both feature profiles (thin + full)
test-profiles:
    cargo test --workspace --lib --bins
    cargo test --workspace --lib --bins --no-default-features

# --- Builds ---

# Debug build
build:
    cargo build --workspace

# Release build
release:
    cargo build --workspace --release

# Release build (thin — no syntax highlighting)
release-thin:
    cargo build --workspace --release --no-default-features

# Check binary size and report
size: release
    @ls -lh target/release/pgr-cli | awk '{print "full: " $5}'
    @just release-thin 2>/dev/null && ls -lh target/release/pgr-cli | awk '{print "thin: " $5}'

# --- CI ---

# Full CI pipeline
ci:
    just check-all && just test-all && just test-profiles && cargo doc --workspace --no-deps

# Coverage (placeholder)
coverage:
    @echo "Coverage tooling TBD"

# --- Dev ---

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
