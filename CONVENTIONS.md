# pgr Coding Conventions & Best Practices

This document defines the rules all contributors (human and agent) follow. When in doubt, this document wins.

---

## 1. Rust Style

### Formatting
- `rustfmt` with default settings. No overrides in `rustfmt.toml`.
- Run `cargo fmt` before every commit. The pre-commit hook enforces this.

### Linting
- `#![warn(clippy::pedantic)]` in every crate's `lib.rs` or `main.rs`.
- Zero clippy warnings. `cargo clippy --workspace -- -D warnings` must pass.
- `#[allow(clippy::...)]` is permitted ONLY with a comment explaining why. Example:
  ```rust
  #[allow(clippy::cast_possible_truncation)] // Terminal dimensions are always < u16::MAX
  ```

### Naming
- Types: `PascalCase`
- Functions, methods, variables: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
- Crate names: `pgr-xxx` (kebab-case in Cargo.toml, `pgr_xxx` as Rust identifiers)
- Module files: `snake_case.rs`
- Test functions: `test_<what>_<condition>_<expected>`, e.g., `test_line_index_empty_buffer_returns_zero`

### Documentation
- All `pub` items get doc comments (`///`).
- Doc comments describe **what** and **why**, not **how** (the code shows how).
- Crate-level doc comments (`//!`) in every `lib.rs` with a one-line summary.
- No doc comments on private items unless the logic is genuinely non-obvious.

---

## 2. Error Handling

### Library crates (`pgr-core`, `pgr-input`, etc.)
- Use `thiserror` derive for all error enums.
- Each crate has its own error type in `error.rs`, re-exported from `lib.rs`.
- Each crate has a `pub type Result<T> = std::result::Result<T, XxxError>;`
- No `unwrap()` or `expect()` in library code. Ever.
- Use `?` for propagation. Use `map_err` when crossing crate boundaries.
- Error messages are lowercase, no trailing punctuation (Rust convention).

### Binary crate (`pgr-cli`)
- Use `anyhow` for top-level error handling.
- `main()` returns `anyhow::Result<()>`.
- `unwrap()` is acceptable only for things that are truly infallible (e.g., regex compilation of a literal pattern known at compile time). Prefer `expect("reason")` with a clear message if you must.

### Panics
- `panic!()`, `todo!()`, `unimplemented!()`: never in merged library code.
- `unreachable!()`: acceptable when the code path is provably unreachable and the compiler can't see it.
- In tests: `unwrap()`, `expect()`, and `panic!()` are fine.

---

## 3. Dependencies

### Principles
- **Minimize dependency count.** Every dependency is a security and maintenance surface.
- **Prefer well-maintained, widely-used crates.** Check download counts and last publish date.
- **No feature bloat.** Enable only the features we use. Disable default features when appropriate.
- **Pin major versions** in Cargo.toml (e.g., `"1"` not `"*"`).

### Approved dependencies

| Crate | Purpose | Used in |
|-------|---------|---------|
| `thiserror` | Error derive macros | All library crates |
| `anyhow` | Top-level error handling | `pgr-cli` |
| `clap` (derive) | Argument parsing | `pgr-cli` |
| `memmap2` | Memory-mapped file access | `pgr-core` |
| `unicode-width` | Character display width | `pgr-display` |
| `libc` | Terminal ioctl, raw mode | `pgr-keys` |
| `regex` | Search engine | `pgr-search` |
| `tempfile` | Test temp files | dev-dependency |

Adding a new dependency requires justification. Don't pull in a crate for something the standard library can do.

### Forbidden patterns
- No `tokio` or `async-std` — pgr is synchronous. The event loop is a simple blocking read.
- No `unsafe` without a `// SAFETY:` comment explaining the invariants.
- No `build.rs` scripts unless absolutely necessary (and documented why).
- No proc macros beyond `thiserror`, `clap`, and `serde` (when needed for agent protocol).

---

## 4. Architecture

### Crate boundaries
- Crate boundaries are authoritative. See CLAUDE.md for the architecture table.
- No circular dependencies. The dependency graph is a DAG.
- Cross-crate communication uses traits defined in `pgr-core` or in the consuming crate.
- Private implementation details stay private. Only the designed public API is `pub`.

### Ownership and borrowing
- Prefer borrowing over cloning. Clone only when ownership transfer is needed.
- Use `&str` in function parameters, `String` in struct fields that own data.
- Use `Cow<'_, str>` when a function might or might not need to allocate.
- Avoid `Arc`/`Mutex` unless genuinely needed for shared state. pgr is single-threaded in its core loop.

### Type design
- Use newtypes to distinguish semantically different values of the same primitive type. E.g., `LineNumber(usize)` vs `ByteOffset(u64)` — but only introduce these if confusion is a real risk. Don't over-newtype.
- Enums over booleans when a function has more than one boolean parameter.
- Prefer `Option` over sentinel values (no `-1` meaning "not found").

### Module organization
- One primary type per file. `buffer.rs` contains `Buffer`, `file_buffer.rs` contains `FileBuffer`.
- `mod.rs` is forbidden. Use `module_name.rs` with `mod module_name;` in the parent.
- Test modules: `#[cfg(test)] mod tests { ... }` at the bottom of the file they test.

---

## 5. Testing

### Philosophy
- **Test-first.** Write the test, see it fail, then implement.
- **Test behavior, not implementation.** Tests assert on observable outputs, not internal state.
- **100% coverage of public API.** Every `pub fn` has at least one test. Untested public API is a bug.

### Test organization
- Unit tests: `#[cfg(test)] mod tests` in each source file
- Integration tests: `tests/` directory at workspace root
- PTY-based tests: `#[ignore]` attribute (slow, run with `just test-all`)
- Test helpers: `#[cfg(test)]` gated modules (e.g., `test_helpers.rs`)

### Test naming
```
test_<unit>_<scenario>_<expected_behavior>
```
Examples:
- `test_line_index_empty_buffer_returns_zero_lines`
- `test_file_buffer_read_at_beyond_eof_returns_zero`
- `test_keymap_space_maps_to_page_forward`

### Test quality
- No `#[should_panic]` — test error returns instead.
- No `sleep()` in tests. Use deterministic synchronization.
- No filesystem side effects outside `tempfile` directories.
- Each test is independent. No shared mutable state between tests.
- Test both the happy path and edge cases (empty input, boundary values, error conditions).

### Coverage
- Target: 100% of public API surface area.
- Use `cargo-tarpaulin` or `cargo-llvm-cov` for measurement (configured via `just coverage`).
- Coverage of private functions is nice but not required — good public API coverage exercises most private code.

---

## 6. Performance

### Principles
- **Correctness first, then performance.** Don't optimize before profiling.
- **No premature allocation.** Use iterators and lazy evaluation where natural.
- **Large file awareness.** pgr must handle multi-GB files without loading them into memory. The mmap and lazy-indexing strategies exist for this reason.

### Specific guidelines
- Avoid `collect()` into a Vec when you can iterate directly.
- Use `&str` slicing instead of `String::clone()` where lifetime allows.
- Buffer I/O with `BufReader`/`BufWriter` for all file and terminal I/O.
- The scan chunk size (64 KiB) and mmap threshold (8 MiB) are consts — tune with benchmarks, not guesses.

---

## 7. Safety and Security

- No `unsafe` without a `// SAFETY:` comment. Every unsafe block documents why it's sound.
- The two expected `unsafe` sites: terminal ioctl (libc calls) and mmap. Both are well-understood.
- LESSSECURE mode (Phase 2) disables shell execution, file writing, and external process spawning. When implementing features that touch these, always check the secure mode flag.
- Never trust input sizes. Validate before allocating. A malicious file claiming 2^64 lines must not cause OOM.
- Regex compilation must use the `regex` crate (linear-time guarantee). Never use a backtracking regex engine on untrusted patterns.

---

## 8. Git and Commits

- Branch naming: `task/NNN-short-name`
- Commit messages: imperative mood, one-line summary, optional blank line + body
- One logical change per commit
- No merge commits in feature branches (rebase workflow)
- `main` always builds and passes `just test`
- Commit hooks enforce `just check && just test` before commit

---

## 9. What NOT to Do

- Don't add features beyond what the current task specifies.
- Don't refactor code outside your task's scope.
- Don't add comments explaining obvious code.
- Don't add `// TODO` without a tracked issue.
- Don't use `String` where `&str` suffices.
- Don't use `Box<dyn Trait>` where a generic `<T: Trait>` works (unless you need type erasure).
- Don't add optional dependencies or feature flags without architectural justification.
- Don't suppress warnings with `#[allow]` — fix the warning.
- Don't write "defensive" code against impossible states. If a state is impossible, `unreachable!()` is appropriate. If it's merely unlikely, handle it properly.
