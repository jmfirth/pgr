# pgr

A drop-in replacement for GNU `less`, written in Rust. Full behavioral conformance with less 692, plus content-aware rendering (diffs, man pages, git blame, JSON) and optional syntax highlighting via syntect.

See `CONVENTIONS.md` for coding conventions, style rules, and best practices — **read it before writing any code**.

---

## Code Standards

- **Clippy pedantic**: `#![warn(clippy::pedantic)]` on all crates. Zero `#[allow(...)]` without an explanatory comment.
- **Formatting**: `cargo fmt` enforced. No exceptions.
- **No panics in library code**: no `unwrap()` or `expect()` in `pgr-*` crate library code. Use proper error types. Tests and `main.rs` may panic.
- **Doc comments**: all public types, traits, and functions.
- **Error handling**: use `thiserror` for library error types, `anyhow` in the binary crate only.

## Architecture

Six crates in a Cargo workspace:

| Crate | Responsibility |
|-------|---------------|
| `pgr-core` | Buffer management, line indexing, marks, filtering |
| `pgr-input` | File/pipe reading, LESSOPEN/LESSCLOSE, follow mode, decompression |
| `pgr-search` | Regex/literal search, highlighting, filter mode |
| `pgr-display` | Terminal rendering, prompt evaluation, ANSI handling, color, Unicode width |
| `pgr-keys` | Raw terminal, key binding, lesskey parsing, command dispatch |
| `pgr-cli` | Binary entry point, arg parsing, env vars, security mode |

**Dependency direction**: `pgr-cli` depends on everything. `pgr-core` depends on nothing internal. Other crates may depend on `pgr-core` but not on each other without explicit architectural justification.

## Build Profiles

Two build profiles via feature flags:
- **Full** (default): `cargo build --release` → ~2.3 MB. Includes syntax highlighting via syntect.
- **Thin**: `cargo build --release --no-default-features` → ~1.7 MB. All content-aware features, no syntax highlighting.

Both profiles must compile and pass tests. Use `just test-profiles` to verify.

## Testing

- **Test-first**: write failing tests before implementation.
- **100% public API coverage**: every public function has at least one test.
- **Three test tiers**:
  - Fast suite (`just test`): unit + doc tests, <10 seconds, runs on commit hook.
  - Full suite (`just test-all`): includes integration, PTY, conformance, visual, slow tests.
  - Profile suite (`just test-profiles`): fast suite for both thin and full builds.
- **Conformance tests** (`just conformance`): PTY comparison against GNU less 692.
- **Visual tests** (`just visual`): expected-output tests for Phase 3+ features (content modes, highlighting).
- Tests must be deterministic. No flaky tests.

## Commands

| Command | What it does |
|---------|-------------|
| `just check` | Format check + clippy |
| `just check-all` | Format check + clippy for both feature profiles |
| `just test` | Fast test suite |
| `just test-all` | Full test suite |
| `just conformance` | Conformance tests only (PTY vs GNU less) |
| `just visual` | Visual correctness tests (content modes, new features) |
| `just test-profiles` | Fast suite for both thin and full builds |
| `just build` | Debug build |
| `just release` | Release build (full) |
| `just release-thin` | Release build (thin, no syntax) |
| `just size` | Report binary size for both profiles |
| `just ci` | Full CI pipeline (both profiles) |
| `just start` | Run pgr (pass-through args) |
| `just doc` | Build and open docs |

## Code Intelligence (cq MCP tools)

Tree-sitter and LSP-powered code intelligence is available via `cq-mcp`. These tools are available to both the orchestrator and subagents. **Prefer these over grep/read for code navigation.**

| Tool | Use when |
|------|----------|
| `cq_body` | Need a function/struct body — replaces read-file-find-offset |
| `cq_outline` | Need to see all symbols in a file at a glance |
| `cq_callers` | Before modifying a function — who calls it? |
| `cq_callchain` | Need full call graph (depth N) — blast radius analysis |
| `cq_refs` | Find all references to a symbol across the project |
| `cq_dead` | After refactoring — find orphaned/unreferenced code |
| `cq_context` | Debugging: "what function contains this line?" |
| `cq_hover` | Quick type info and signature at a location |
| `cq_search` | Structural search via tree-sitter queries (e.g., find all `unwrap()` calls) |
| `cq_rename` | LSP-driven rename across the project — semantic, not text |
| `cq_diagnostics` | Check for syntax errors and LSP warnings |
| `cq_deps` | See crate/module dependency graph |

**Workflow guidance:**
- **Before modifying a function**: `cq_callers` or `cq_callchain` to understand impact
- **After refactoring**: `cq_dead` to catch orphaned code
- **Reading code**: `cq_body` for specific symbols, `cq_outline` for file overview
- **Debugging**: `cq_context` with file:line from error messages
- **Renaming**: `cq_rename` with `dry_run: true` first, then `apply: true`
- **Scoping**: Use `scope` parameter to limit searches to a crate (e.g., `scope: "pgr-display/src"`)
- **Verifying conventions**: `cq_search` with tree-sitter queries (e.g., find all `unwrap()` in library code)

## Conventions

- Branch naming: `task/[id]-[short-name]`
- No `todo!()`, `unimplemented!()`, or `// TODO` in merged code unless tracked
- Stubs only for interface contracts with identified downstream tasks
- One logical change per commit
- Main always builds and passes fast suite
