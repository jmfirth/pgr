# pgr

A drop-in replacement for GNU `less` with syntax highlighting, diff awareness, and content-sensitive rendering. Set `PAGER=pgr` and everything gets better.

## Why pgr

`$PAGER` is invoked transparently by dozens of tools — `git diff`, `git log`, `man`, `kubectl`, `psql`, `journalctl`. The pager is the only place in the pipeline where highlighting, navigation, and rendering can improve. pgr makes that happen automatically.

- **100% less-compatible** — 212 PTY conformance tests against GNU less 692
- **Syntax highlighting** — 75 languages via syntect, built in. No external tools needed.
- **Diff awareness** — background tinting, per-hunk syntax highlighting, side-by-side view, hunk/file navigation
- **Content detection** — auto-detects diff, man page, git blame, git log, JSON, SQL tables, compiler errors
- **Modern features** — match count, multi-pattern highlighting, clipboard yank, URL navigation, git gutter

## Install

```
cargo install pgr
```

Then add to your shell profile:
```bash
export PAGER=pgr
```

## Two builds

| Build | Command | Size | What's included |
|-------|---------|------|-----------------|
| **Full** (default) | `cargo install pgr` | ~2.3 MB | Everything including syntax highlighting |
| **Thin** | `cargo install pgr --no-default-features` | ~1.7 MB | All features except syntax highlighting |

## Features

### Syntax highlighting

Opens a file → highlighted. Opens a diff → code within hunks is highlighted. No configuration, no piping through other tools. 75 languages supported out of the box.

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

## Testing

- **2,125+ unit tests** across all crates
- **212 conformance tests** — PTY comparison against GNU less 692
- **11 visual smoke tests** — PTY verification of Phase 3 features
- Both build profiles (full + thin) tested in CI

## License

MIT
