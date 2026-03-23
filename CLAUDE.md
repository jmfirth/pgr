# pgr

A drop-in replacement for GNU `less`, written in Rust. Full behavioral conformance with less 668+, plus a structured agent interface (Pgr Protocol) and modern pager enhancements.

See `SPECIFICATION.md` for the full design document. See `PROCESS.md` for the development workflow. See `PLAN.md` for the current project plan and task status. See `CONVENTIONS.md` for coding conventions, style rules, and best practices — **read it before writing any code**. See `agents/` for agent role definitions (developer, reviewer, plan-reviewer).

---

## Code Standards

- **Clippy pedantic**: `#![warn(clippy::pedantic)]` on all crates. Zero `#[allow(...)]` without an explanatory comment.
- **Formatting**: `cargo fmt` enforced. No exceptions.
- **No panics in library code**: no `unwrap()` or `expect()` in `pgr-*` crate library code. Use proper error types. Tests and `main.rs` may panic.
- **Doc comments**: all public types, traits, and functions.
- **Error handling**: use `thiserror` for library error types, `anyhow` in the binary crate only.

## Architecture

Eight crates in a Cargo workspace (see SPECIFICATION.md §8):

| Crate | Responsibility |
|-------|---------------|
| `pgr-core` | Buffer management, line indexing, marks, filtering |
| `pgr-input` | File/pipe reading, LESSOPEN/LESSCLOSE, follow mode, decompression |
| `pgr-search` | Regex/literal search, highlighting, filter mode |
| `pgr-display` | Terminal rendering, prompt evaluation, ANSI handling, color, Unicode width |
| `pgr-keys` | Raw terminal, key binding, lesskey parsing, command dispatch |
| `pgr-syntax` | Tree-sitter syntax highlighting (optional feature) |
| `pgr-agent` | Pgr Protocol server, NDJSON, event subscriptions, batch mode |
| `pgr-cli` | Binary entry point, arg parsing, env vars, security mode |

**Dependency direction**: `pgr-cli` depends on everything. `pgr-core` depends on nothing internal. Other crates may depend on `pgr-core` but not on each other without explicit architectural justification.

## Testing

- **Test-first**: write failing tests before implementation.
- **100% public API coverage**: every public function has at least one test.
- **Two test tiers**:
  - Fast suite (`just test`): unit + doc tests, <10 seconds, runs on commit hook.
  - Full suite (`just test-all`): includes integration, PTY, conformance, slow tests.
- **Conformance tests** reference SPECIFICATION.md section numbers in their doc comments.
- Tests must be deterministic. No flaky tests.

## Commands

| Command | What it does |
|---------|-------------|
| `just check` | Format check + clippy |
| `just test` | Fast test suite |
| `just test-all` | Full test suite |
| `just start` | Run pgr (pass-through args) |
| `just build` | Debug build |
| `just release` | Release build |
| `just ci` | Full CI pipeline |
| `just doc` | Build and open docs |

## Conventions

- Branch naming: `task/[id]-[short-name]`
- No `todo!()`, `unimplemented!()`, or `// TODO` in merged code unless tracked in PLAN.md
- Stubs only for interface contracts with identified downstream tasks
- One logical change per commit
- Main always builds and passes fast suite
