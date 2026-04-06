# pgr

A pager that understands what it's looking at.

`$PAGER` is the most underutilized integration point in the terminal. Dozens of tools pipe through it — `git diff`, `git log`, `man`, `kubectl`, `psql`, `journalctl` — but your pager treats everything as plain text. pgr changes that.

Set `PAGER=pgr` and diffs get syntax-highlighted code with background tinting and hunk navigation. Man pages get section jumping. Git blame gets recency coloring. JSON gets highlighting. SQL tables get sticky headers. Compiler errors get clickable links. No configuration, no piping through external tools.

**Drop-in compatible.** Every less keybinding works. 212 PTY conformance tests against GNU less 692.

**Faster.** Up to 6.5x throughput on large files. First screen appears before the full file is scanned.

**Tested.** 2,400+ tests across unit, PTY conformance, and visual verification tiers.

pgr replaces less, bat, delta, and diff-so-fancy in a single backward-compatible binary.

## Install

```
cargo install pgr
```

```bash
export PAGER=pgr
```

That's it. Every tool that uses `$PAGER` now gets pgr's features automatically.

## Diff awareness

This is the headline feature. pgr detects diff content and transforms how you read it:

![Diff awareness](demos/diff-awareness.gif)

- **Background tinting** — added lines get subtle green, removed lines get subtle red, with syntax-highlighted code inside the hunks
- **Word-level emphasis** — changed words within modified lines are highlighted with intense color in side-by-side view
- **Side-by-side view** — `ESC-V` toggles split panel view (old left, new right)
- **Hunk navigation** — `]c` / `[c` jump between hunks, `]f` / `[f` between files
- **Works transparently** — `git diff`, `git log -p`, `git show`, patch files — all detected automatically

```
$ git diff | pgr          # tinted + highlighted diff
$ git log -p | pgr        # per-commit navigation with ]g/[g
$ git blame file.rs | pgr # recency-colored, syntax-highlighted
```

## Syntax highlighting

75 languages built in via syntect. Opens a `.rs` file — highlighted. Toggle with `ESC-S`. Works in both file and diff modes.

![Syntax highlighting](demos/syntax-highlight.gif)

## SQL table mode

pgr detects SQL table output (psql, mysql, sqlite3) and activates a table viewer:

![SQL table mode](demos/table-view.gif)

- **Sticky header** — column names stay pinned at the top while you scroll
- **Column-snap scroll** — left/right arrow jumps to column boundaries, not characters
- **Frozen first column** — the ID column stays visible during horizontal scroll
- **Visual treatment** — bold headers, dim structure characters, alternating row stripes

## 100% less compatible

pgr isn't "mostly compatible." It's tested against GNU less 692 with 212 PTY-level conformance tests that compare terminal output byte-for-byte. Every flag, every keybinding, every prompt escape, every edge case in search, scroll, and multi-file navigation.

If your muscle memory works in less, it works in pgr. If your scripts pipe through `$PAGER`, they work with pgr. If your `.lesskey` bindings are customized, pgr reads them.

## Content modes

Beyond diffs and tables, pgr auto-detects what it's looking at from the first screenful:

| Content | What you get |
|---------|-------------|
| **Man pages** | `]s` / `[s` section navigation (jump to OPTIONS, SYNOPSIS, etc.) |
| **Git log** | `]g` / `[g` commit-to-commit navigation |
| **JSON** | Syntax highlighting (even from pipes with no filename) |
| **SQL tables** | Sticky header, column-snap scroll, frozen first column |
| **Compiler errors** | `file:line:col` references become clickable OSC 8 hyperlinks |

Detection is automatic. No flags, no configuration.

## More features

**Search enhancements** — "match 3 of 47" in the prompt. Live match count during incremental search. `&+` adds extra highlight patterns in different colors.

**Clipboard** — `y` yanks the current line, `Y` yanks the visible screen. Works over SSH via OSC 52.

**URL navigation** — `]u` / `[u` jump between URLs. `o` opens the highlighted URL in your browser.

**Git gutter** — `+`/`-`/`~` markers in the left margin for uncommitted changes. Toggle with `ESC-G`.

**Buffer save** — `s` saves the entire buffer to a file (ANSI-stripped plain text).

## Key bindings

All GNU less key bindings work. pgr adds:

| Key | Action |
|-----|--------|
| `ESC-S` | Toggle syntax highlighting |
| `ESC-V` | Toggle side-by-side diff |
| `ESC-G` | Toggle git gutter |
| `]c` / `[c` | Next/prev diff hunk |
| `]f` / `[f` | Next/prev diff file |
| `]s` / `[s` | Next/prev man page section |
| `]g` / `[g` | Next/prev git log commit |
| `]u` / `[u` | Next/prev URL |
| `o` | Open highlighted URL |
| `y` / `Y` | Yank line / screen to clipboard |
| `&+` / `&-` / `&l` | Add/remove/list highlight patterns |
| `s` | Save buffer to file |

## Configuration

| Variable | Default | Effect |
|----------|---------|--------|
| `PGR_SYNTAX` | `1` | `0` to disable syntax highlighting |
| `PGR_THEME` | `base16-ocean.dark` | Syntect theme name |
| `PGR_CLIPBOARD` | `auto` | `osc52`, `pbcopy`, `xclip`, `xsel`, `wl-copy`, `off` |
| `PGR_GIT_GUTTER` | `1` | `0` to disable git gutter |
| `LESS` | — | Flags passed to pgr (less-compatible) |
| `LESSOPEN` | — | Preprocessor command (less-compatible) |

CLI flags: `--syntax`/`--no-syntax`, `--theme <name>`, `--clipboard <backend>`, `--git-gutter`/`--no-git-gutter`. All GNU less flags are also supported.

## Performance

| File size | less | pgr | |
|-----------|------|-----|-|
| 100 lines | 3.7 ms | 3.1 ms | **1.2x faster** |
| 10,000 lines | 7.2 ms | 3.4 ms | **2.1x faster** |
| 100,000 lines | 37.9 ms | 5.8 ms | **6.5x faster** |

Benchmarked with [hyperfine](https://github.com/sharkdp/hyperfine) on macOS (Apple Silicon). Binary is 2.3 MB fully static with 75-language highlighting. A thin build without highlighting is 1.7 MB (`cargo install pgr --no-default-features`).

## Testing

| Tier | Tests | What it validates |
|------|-------|-------------------|
| Unit | 2,176 | API correctness across 6 crates |
| Conformance | 212 | PTY comparison against GNU less 692 |
| Visual | 37 | Cell-level PTY verification of every feature |

```
just test          # unit tests (<10s)
just conformance   # PTY tests vs GNU less (~90s)
just visual        # feature verification (~12s)
```

## License

MIT
