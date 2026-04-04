# pgr

A pager that understands what it's looking at.

`$PAGER` is the most underutilized integration point in the terminal. Dozens of tools pipe through it — `git diff`, `git log`, `man`, `kubectl`, `psql`, `journalctl` — but your pager treats everything as plain text. pgr changes that.

Set `PAGER=pgr` and diffs get background tinting and hunk navigation. Man pages get section jumping. Git blame gets recency coloring. JSON gets syntax highlighting. SQL tables get sticky headers. Compiler errors get clickable links. No configuration, no piping through external tools. One binary, zero dependencies.

**Drop-in compatible.** Every less keybinding works. 212 PTY conformance tests against GNU less 692.

**Faster.** Up to 6.5x throughput on large files. First screen appears before the full file is scanned.

**Competitive size.** 2.3 MB fully static with 75-language highlighting included. GNU less with its dynamic dependencies (ncurses, pcre2, libtinfo) is ~1.35 MB — pgr adds content awareness, diff rendering, and syntax highlighting for less than 1 MB more. A thin build without highlighting is 1.7 MB.

**Tested.** 2,400+ tests across unit, PTY conformance, and visual verification tiers.

pgr replaces less, bat, delta, and diff-so-fancy in a single backward-compatible binary.

## Install

```
cargo install pgr
```

Then add to your shell profile:
```bash
export PAGER=pgr
```

## Features

### Syntax highlighting

Opens a file — highlighted. Opens a diff — code within hunks is highlighted. No configuration, no piping through other tools. 75 languages supported out of the box.

- Toggle on/off: `ESC-S`
- Choose theme: `--theme <name>` or `PGR_THEME=name`
- Disable: `--no-syntax` or `PGR_SYNTAX=0`

### Diff awareness

pgr detects diff content automatically and enhances it:

- **Background tinting** — added lines get subtle green, removed lines get subtle red
- **Per-hunk syntax highlighting** — code inside diffs is highlighted by language
- **Side-by-side view** — `ESC-V` toggles split panel view (old left, new right)
- **Hunk navigation** — `]c` / `[c` jump between hunks
- **File navigation** — `]f` / `[f` jump between files in multi-file diffs

### Content modes

pgr auto-detects what it's looking at and adapts:

| Content | Detection | Features |
|---------|-----------|----------|
| **Diff** | `diff --git`, `@@` hunk headers | Highlighting, side-by-side, hunk/file nav |
| **Man page** | Backspace overprinting | `]s` / `[s` section navigation |
| **Git blame** | Hash prefix on every line | Recency coloring, syntax-highlighted code |
| **Git log** | `commit` headers | `]g` / `[g` commit navigation |
| **JSON** | Starts with `{` or `[` | Syntax highlighting |
| **SQL table** | ASCII box drawing | Sticky header, column-snap scroll, frozen first column |
| **Compiler errors** | `file:line:col` patterns | OSC 8 clickable hyperlinks |

### Search enhancements

- **Match count** — "match 3 of 47" shown in prompt after search
- **Live count** — match count updates during incremental search
- **Multi-pattern** — `&+` adds patterns in different colors, `&-` removes, `&l` lists

### Clipboard

- `y` — yank current line to clipboard
- `Y` — yank all visible lines to clipboard
- Backends: OSC 52 (universal, works over SSH), pbcopy, xclip, xsel, wl-copy

### URL navigation

- `]u` / `[u` — jump between URLs in content
- `o` — open highlighted URL in `$BROWSER`

### Git gutter

When viewing a tracked file, shows `+`/`-`/`~` markers for uncommitted changes.

- Toggle: `ESC-G`
- Disable: `--no-git-gutter` or `PGR_GIT_GUTTER=0`

### Buffer save

- `s` — save entire buffer to file (ANSI-stripped plain text)

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

### Environment variables

| Variable | Default | Effect |
|----------|---------|--------|
| `PAGER` | — | Set to `pgr` for system-wide use |
| `PGR_SYNTAX` | `1` | `0` to disable syntax highlighting |
| `PGR_THEME` | `base16-ocean.dark` | Syntect theme name |
| `PGR_CLIPBOARD` | `auto` | Clipboard backend: `auto`, `osc52`, `pbcopy`, `xclip`, `xsel`, `wl-copy`, `off` |
| `PGR_GIT_GUTTER` | `1` | `0` to disable git gutter |
| `LESS` | — | Flags passed to pgr (less-compatible) |
| `LESSOPEN` | — | Preprocessor command (less-compatible) |

### CLI flags

All GNU less flags are supported. pgr adds:

```
--syntax / --no-syntax     Enable/disable syntax highlighting
--theme <name>             Select syntax theme
--clipboard <backend>      Choose clipboard backend
--git-gutter / --no-git-gutter  Enable/disable git gutter
```

## Architecture

Six Rust crates in a Cargo workspace:

| Crate | Responsibility |
|-------|---------------|
| `pgr-core` | Buffer, line index, marks, content detection, diff parsing |
| `pgr-input` | File/pipe reading, LESSOPEN, follow mode |
| `pgr-search` | Regex search, highlighting, multi-pattern |
| `pgr-display` | Rendering, ANSI, syntax, Unicode, side-by-side |
| `pgr-keys` | Terminal, key binding, command dispatch |
| `pgr-cli` | Entry point, arg parsing, env vars |

## Two builds

| Build | Command | Size | What's included |
|-------|---------|------|-----------------|
| **Full** (default) | `cargo install pgr` | ~2.3 MB | Everything including syntax highlighting |
| **Thin** | `cargo install pgr --no-default-features` | ~1.7 MB | All features except syntax highlighting |

## Performance

pgr is faster than GNU less at every file size, measured on the throughput path (open, dump, exit):

| File size | less | pgr | Speedup |
|-----------|------|-----|---------|
| 100 lines (5 KB) | 3.7 ms | 3.1 ms | **1.2x faster** |
| 10,000 lines (500 KB) | 7.2 ms | 3.4 ms | **2.1x faster** |
| 100,000 lines (5 MB) | 37.9 ms | 5.8 ms | **6.5x faster** |

Benchmarked with [hyperfine](https://github.com/sharkdp/hyperfine) on macOS (Apple Silicon). Syntax highlighting adds negligible overhead (~0.5ms on a 2,300-line Rust file).

Interactive performance (first render, scroll responsiveness) is equivalent to less — pgr uses lazy line indexing so the first screen appears before the full file is scanned.

## Testing

pgr has **2,400+ tests** across three tiers:

| Tier | Tests | What it validates |
|------|-------|-------------------|
| **Unit tests** | 2,151 | Internal API correctness across all 6 crates |
| **Conformance** | 212 | PTY side-by-side comparison against GNU less 692 |
| **Visual** | 37 | PTY end-to-end verification of every Phase 3 feature |

The visual test suite spawns pgr in a real pseudo-terminal, sends keystrokes, and inspects the rendered screen at cell level — verifying exact colors, text content, and cursor positions. Every feature (syntax highlighting, diff coloring, side-by-side, content modes, search, clipboard, URL navigation, git gutter, SQL tables, compiler hyperlinks) is validated this way.

Both build profiles (full + thin) are tested in CI.

```
just test          # 2,151 unit tests (<10s)
just conformance   # 212 PTY tests vs GNU less (~90s)
just visual        # 37 PTY feature tests (~12s)
```

## License

MIT
