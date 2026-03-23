# pgr — Specification & Design Document

**Version:** 0.1.0-draft
**Date:** March 22, 2026
**Author:** Justin (with Claude as thinking partner)
**License:** Apache 2.0

---

## 1. What pgr Is

pgr is a drop-in replacement for the `less` pager, written in Rust, targeting 100% behavioral conformance with GNU less while introducing a structured agent interface that makes paged content programmatically queryable.

pgr is three things in one binary:

1. **A conformant pager.** Every flag, keybinding, environment variable, and behavior of GNU less works identically. Anyone who sets `PAGER=pgr` or `MANPAGER=pgr` notices nothing different — until they need more.

2. **A modern pager.** Native UTF-8 with correct East Asian width handling, true color support, syntax-aware highlighting via tree-sitter, transparent decompression (gzip, bzip2, xz, zstd), and a regex engine that doesn't crash on adversarial input.

3. **An agent-readable viewport.** A structured IPC interface — the Pgr Protocol — that allows programs and AI agents to query, search, navigate, and extract content from the pager programmatically. This transforms a pager from a passive display tool into an active intelligence-gathering interface.

---

## 2. Why pgr Should Exist

### 2.1 The Pager Gap

The "rewrite it in Rust" wave has produced excellent replacements for grep (ripgrep), find (fd), ls (eza), cat (bat), du (dust), and others. The pager — arguably the most frequently invoked tool in any terminal workflow — has been left behind. There is no serious Rust pager. `bat` handles file viewing with syntax highlighting but is not a general-purpose pager; it cannot replace `less` as `PAGER` for arbitrary piped input, man pages, git diffs, or log tailing.

### 2.2 The Security Case

less is written in C, has been in continuous development since 1984, and parses untrusted input (piped data, user-supplied files, preprocessor output via LESSOPEN). It runs in contexts where it processes content from the network (curl piped through less, git log, docker logs). A memory-safe implementation eliminates an entire class of potential vulnerabilities.

### 2.3 The Agent Case

No pager is designed for programmatic consumption. In the emerging world of AI agents, code assistants, and autonomous development environments, agents constantly need to:

- Read command output that exceeds terminal buffer size
- Search through log files for specific patterns and extract context
- Navigate large files to specific locations and understand what's visible
- Monitor growing files (tail -f equivalent) and react to new content
- Extract structured data from what's currently displayed

Today, agents work around the pager by capturing raw stdout, buffering entire files into memory, and doing their own text processing. This is wasteful and loses the pager's ability to efficiently handle files larger than available memory. pgr makes the pager itself an agent-accessible resource.

### 2.4 The Firebox Case

pgr is designed as a native primitive for the Firebox WASM agent runtime. With WASM as a first-class compilation target from day one, every Firebox environment gets a working pager with zero external dependencies. A fully functional interactive pager running inside WASM demonstrates that Firebox can handle real terminal applications, not just batch scripts. The Pgr Protocol gives agents inside Firebox a structured way to interrogate content without buffering everything into memory — critical in a resource-constrained sandbox.

---

## 3. Conformance Target

### 3.1 Primary Target: GNU less 668+

The canonical reference is the GNU less man page (less.1) from the official repository at https://github.com/gwsw/less. The current release line is 668.x. pgr targets full behavioral compatibility with this version.

"Full behavioral compatibility" means:

- Every command-line flag produces identical behavior
- Every interactive command (keybinding) produces identical behavior
- Every environment variable is recognized and honored
- Every prompt string escape sequence is interpreted identically
- The LESSOPEN/LESSCLOSE input preprocessor protocol works identically
- The lesskey/lesskey source file format is parsed identically
- Exit codes match under identical conditions
- Output to the terminal is visually identical for the same input and terminal configuration

### 3.2 Secondary Target: POSIX more(1) Compatibility

When invoked as `more` (via symlink or filename detection) or when `LESS_IS_MORE=1` is set, less enters a POSIX more compatibility mode with altered behaviors. pgr implements this mode identically, per POSIX.1-2017 (IEEE Std 1003.1-2017), Shell and Utilities volume, more(1) specification.

### 3.3 What Conformance Does NOT Mean

- pgr does not aim to reproduce bugs in less. Behavior that is clearly a bug in less (and documented as such in the less issue tracker) may be fixed in pgr, with the divergence documented.
- pgr does not aim to reproduce less's behavior on platforms it does not target (OS/2, MS-DOS). Initial targets are Linux, macOS, and WASM. Windows support is a future goal.
- pgr does not aim to reproduce less's internal implementation. The C source is not consulted for implementation strategy; only the documented external behavior (man page, help output, FAQ, changelog) defines the contract.

---

## 4. Conformance Verification Strategy

### 4.1 No Existing Test Suite

GNU less does not ship with a conformance test suite. There is no external test corpus equivalent to the gawk test suite or the GNU coreutils test suite. This means pgr must build its own comprehensive test suite from scratch, derived from the man page specification.

### 4.2 Test Suite Architecture

The test suite is organized into the following categories:

**4.2.1 Command-Line Option Tests**
One or more tests for every flag documented in the man page. Each test verifies:
- The flag is accepted without error
- The flag produces the documented behavior
- The long-form name is equivalent to the short form
- Flag interactions (e.g., -i vs -I, -e vs -E) behave correctly

**4.2.2 Interactive Command Tests**
Tests that simulate terminal input (via pseudo-terminal) and verify screen output. Every interactive command documented in the COMMANDS section of the man page is tested:
- Navigation: SPACE, b, d, u, g, G, j, k, J, K, ESC-SPACE, ESC-b, etc.
- Search: /, ?, n, N, ESC-n, ESC-N, &, and all search modifiers (^N, ^E, ^F, ^K, ^R, ^S, ^W, ^L)
- File management: :e, :n, :p, :x, :d
- Marks: m, M, ', ESC-m
- Bracket matching: {, }, (, ), [, ]
- Option toggling: -, --, -+, --+, -!, --!, _, __
- Miscellaneous: F, ESC-F, R, v, !, #, |, s, =, V, q, h
- OSC 8 hyperlink commands: ^O^N, ^O^P, ^O^L, ^O^O

**4.2.3 Environment Variable Tests**
Every environment variable documented in the ENVIRONMENT VARIABLES section:
- LESS, LESSANSIENDCHARS, LESSANSIMIDCHARS, LESSCHARDEF, LESSCHARSET
- LESSCLOSE, LESSECHO, LESSEDIT, LESSGLOBALTAGS, LESSHISTFILE, LESSHISTSIZE
- LESSKEYIN, LESSKEYIN_SYSTEM, LESSKEY, LESSKEY_SYSTEM
- LESSMETACHARS, LESSMETAESCAPE, LESS_IS_MORE, LESSOPEN
- LESS_OSC8_xxx, LESS_OSC8_ANY
- LESSSECURE, LESSSEPARATOR, LESS_SHELL_LINES
- LESSUTFBINFMT, LESSWINDOW
- COLUMNS, LINES, HOME, LANG, LC_CTYPE, SHELL, TERM, VISUAL, EDITOR, PATH

**4.2.4 Prompt String Tests**
The prompt string mini-language (%b, %B, %c, %d, %D, %E, %f, %F, %i, %l, %L, %m, %M, %o, %p, %P, %s, %S, %t, %T, %x, and conditional expressions with ?x....) is tested via the -P flag.

**4.2.5 Input Preprocessor Tests**
LESSOPEN and LESSCLOSE pipeline behavior, including:
- Standard preprocessor (LESSOPEN="command %s")
- Pipe preprocessor (LESSOPEN="|command %s")
- Two-pipe preprocessor (LESSOPEN="||command %s")
- LESSCLOSE cleanup behavior

**4.2.6 lesskey Source File Tests**
Parsing of lesskey source files, including:
- #command section (key bindings)
- #line-edit section (line editing keys)
- #env section (environment variables)
- Key naming syntax (\\e, \\k, ^X, etc.)

**4.2.7 Regression Tests Against less**
A harness that runs the same input through both `less` and `pgr`, captures terminal output via a pseudo-terminal, and diffs the results. This catches behavioral divergences that unit tests miss.

**4.2.8 Fuzz Testing**
Property-based testing (via `proptest` or `arbitrary` crates) and AFL/libfuzzer integration for:
- Input parsing (arbitrary byte sequences)
- Regex compilation (adversarial patterns)
- lesskey file parsing
- Prompt string evaluation
- ANSI escape sequence handling

### 4.3 Test Tooling

Tests that require terminal interaction use a pseudo-terminal (PTY) harness. The harness:
- Spawns pgr in a PTY with controlled dimensions (e.g., 80x24)
- Sends keystrokes with configurable timing
- Captures screen state after each interaction
- Compares screen state against expected output
- Supports both snapshot testing and assertion-based testing

---

## 5. Full Feature Inventory (Conformance Layer)

The following is a complete inventory of less features that pgr must implement for conformance. Each item maps to a section of the less(1) man page.

### 5.1 Navigation Commands

| Command | Behavior |
|---------|----------|
| SPACE, ^V, f, ^F | Scroll forward N lines (default: one window) |
| z | Like SPACE; if N given, sets new window size |
| ENTER, RETURN, ^N, e, ^E, j, ^J | Scroll forward N lines (default: 1) |
| d, ^D | Scroll forward N lines (default: half screen); N becomes sticky |
| b, ^B, ESC-v | Scroll backward N lines (default: one window) |
| w | Like ESC-v; if N given, sets new window size |
| y, ^Y, ^P, k, ^K | Scroll backward N lines (default: 1) |
| u, ^U | Scroll backward N lines (default: half screen); N becomes sticky |
| J | Like j, but scrolls beyond end of file |
| K or Y | Like k, but scrolls beyond beginning of file |
| ESC-SPACE | Scroll forward full screen, even at end of file |
| ESC-b | Scroll backward full screen, even at beginning |
| ESC-j | Scroll forward N file lines (default: 1) |
| ESC-k | Scroll backward N file lines (default: 1) |
| ESC-) or RIGHTARROW | Scroll horizontally right N chars (default: half screen width) |
| ESC-( or LEFTARROW | Scroll horizontally left N chars |
| ESC-} or ^RIGHTARROW | Scroll right to end of longest displayed line |
| ESC-{ or ^LEFTARROW | Scroll left to first column |
| r, ^R, ^L | Repaint screen |
| R | Repaint, discarding buffered input (reload file) |
| F | Follow mode (tail -f equivalent) |
| ESC-F | Follow until search pattern matches |
| g, <, ESC-< | Go to line N (default: 1 / beginning) |
| G, >, ESC-> | Go to line N (default: end of file) |
| ESC-G | Like G, but for stdin goes to last buffered line |
| p, % | Go to position N percent |
| P | Go to byte offset N |

### 5.2 Search Commands

| Command | Behavior |
|---------|----------|
| /pattern | Search forward for N-th match |
| ?pattern | Search backward for N-th match |
| ESC-/pattern | Same as /* |
| ESC-?pattern | Same as ?* |
| n | Repeat previous search |
| N | Repeat previous search, reverse direction |
| ESC-n | Repeat search, crossing file boundaries |
| ESC-N | Repeat search, reverse, crossing file boundaries |
| ESC-u | Toggle search highlighting |
| ESC-U | Clear search highlighting and saved pattern |
| &pattern | Filter: display only matching lines |

**Search modifiers (at start of pattern):**
^N/! (NOT match), ^E/* (multi-file), ^F/@ (from first file), ^K (keep position), ^R (literal), ^S (sub-pattern), ^W (wrap), ^L (literal next char)

### 5.3 File Management Commands

| Command | Behavior |
|---------|----------|
| :e [filename] | Examine new file (% = current, # = previous) |
| ^X^V or E | Same as :e |
| :n | Next file |
| :p | Previous file |
| :x | First file (or N-th file) |
| :d | Remove current file from list |

### 5.4 Marks

| Command | Behavior |
|---------|----------|
| m (letter) | Mark first displayed line with letter |
| M (letter) | Mark last displayed line with letter |
| ' (letter) | Return to marked position |
| '' | Return to position of last large movement |
| '^ | Jump to beginning of file |
| '$ | Jump to end of file |
| ^X^X | Same as ' |
| ESC-m (letter) | Clear mark |

### 5.5 Bracket Matching

{, }, (, ), [, ] and ESC-^F / ESC-^B with custom bracket characters.

### 5.6 Option Toggling (Runtime)

-, --, -+, --+, -!, --!, _ (underscore), __ (double underscore) — for querying and changing options while running.

### 5.7 Miscellaneous Commands

| Command | Behavior |
|---------|----------|
| = or ^G or :f | Print file info |
| v | Invoke editor ($VISUAL or $EDITOR) |
| ! command | Shell command |
| # command | Shell command with prompt-style expansion |
| \| mark command | Pipe section to shell command |
| s filename | Save piped input to file |
| t | Next tag |
| T | Previous tag |
| ^O^N / ^O^P | Navigate OSC 8 hyperlinks |
| ^O^L | Jump to selected hyperlink |
| ^O^O | Open hyperlink URI |
| V | Print version |
| q, Q, :q, :Q, ZZ | Quit |
| h, H | Help |

### 5.8 Command-Line Options (Complete List)

The following flags must all be supported with identical semantics to GNU less:

```
-? --help
-a --search-skip-screen
-A --SEARCH-SKIP-SCREEN
-bn --buffers=n
-B --auto-buffers
-c --clear-screen
-C --CLEAR-SCREEN
-d --dumb
-Dxcolor --color=xcolor
-e --quit-at-eof
-E --QUIT-AT-EOF
-f --force
-F --quit-if-one-screen
-g --hilite-search
-G --HILITE-SEARCH
-hn --max-back-scroll=n
-i --ignore-case
-I --IGNORE-CASE
-jn --jump-target=n
-J --status-column
-kfilename --lesskey-file=filename
   --lesskey-src=filename
   --lesskey-content=text
-K --quit-on-intr
-L --no-lessopen
-m --long-prompt
-M --LONG-PROMPT
-n --line-numbers
-N --LINE-NUMBERS
-ofilename --log-file=filename
-Ofilename --LOG-FILE=filename
-ppattern --pattern=pattern
-Pprompt --prompt=prompt
-q --quiet --silent
-Q --QUIET --SILENT
-r --raw-control-chars
-R --RAW-CONTROL-CHARS
-s --squeeze-blank-lines
-S --chop-long-lines
-ttag --tag=tag
-Ttagsfile --tag-file=tagsfile
-u --underline-special
-U --UNDERLINE-SPECIAL
-V --version
-w --hilite-unread
-W --HILITE-UNREAD
-xn,... --tabs=n,...
-X --no-init
-yn --max-forw-scroll=n
-zn --window=n
-"cc --quotes=cc
-~  --tilde
-#n --shift=n
   --exit-follow-on-close
   --file-size
   --follow-name
   --header=n[,m[,n[,m]]]
   --intr=c
   --line-num-width=n
   --modelines=n
   --mouse
   --no-keypad
   --no-histdups
   --no-number-headers
   --no-search-headers
   --no-vbell
   --proc-backspace
   --PROC-BACKSPACE
   --proc-return
   --PROC-RETURN
   --proc-tab
   --PROC-TAB
   --redraw-on-quit
   --rscroll=c
   --save-marks
   --search-options=...
   --show-preproc-errors
   --status-col-width=n
   --status-line
   --use-backslash
   --use-color
   --wheel-lines=n
   --wordwrap
```

### 5.9 Prompt String Escapes

All `%` escape sequences for -P prompt customization, including conditional expressions (?x true-text .), must be implemented per the PROMPTS section of the man page.

### 5.10 Color System

The -D / --color option with all selector characters (B, C, E, H, M, N, P, R, S, W, 1-5, d, k, s, u) and both 4-bit and 8-bit color specifications, plus text attribute modifiers (s/~, u/_, d/*, l/&).

### 5.11 Security Mode

When LESSSECURE=1 is set, the following must be disabled: !, #, |, :e, v, s, -o, -O, -k, --lesskey-src, --lesskey-content, -t, -T, LESSOPEN, LESSCLOSE, LESSHISTFILE, LESSEDIT, VISUAL, EDITOR.

### 5.12 Character Set Handling

LESSCHARSET, LESSCHARDEF, and the automatic UTF-8 detection via locale variables (LANG, LC_ALL, LC_CTYPE). Binary character display via LESSUTFBINFMT.

---

## 6. Modern Pager Improvements (Enhancement Layer)

These features are additive and do not alter conformance behavior. They are activated via pgr-specific flags or configuration. When not activated, pgr behaves identically to less.

### 6.1 Syntax Highlighting

- Tree-sitter integration for language-aware highlighting
- Activated via `--syntax` flag or `PGR_SYNTAX=1` environment variable
- File type detection via extension, shebang, and content sniffing
- Customizable themes via TOML configuration
- Disabled by default to maintain less compatibility
- Graceful degradation: if a tree-sitter grammar is not available, falls back to no highlighting

### 6.2 Transparent Decompression (Native)

- Built-in support for gzip, bzip2, xz, zstd, lz4
- Activated automatically when a compressed file is detected (by magic bytes)
- Does not interfere with LESSOPEN; if LESSOPEN is set, it takes precedence
- Available via `--decompress` flag or `PGR_DECOMPRESS=1`

### 6.3 Improved Regex Engine

- Uses the Rust `regex` crate by default, which provides:
  - Linear-time guarantees (no catastrophic backtracking)
  - Full Unicode support
  - Named capture groups
- Optional PCRE2 support via feature flag for compatibility with less's PCRE2 mode
- Pattern syntax is compatible with less's default POSIX regex mode

### 6.4 Enhanced Unicode Handling

- Correct East Asian Width (EAW) computation for CJK characters
- Correct handling of combining characters and zero-width joiners
- Emoji width handling (including multi-codepoint emoji sequences)
- Grapheme cluster-aware line wrapping (via `unicode-segmentation` crate)

### 6.5 True Color Support

- 24-bit color passthrough when terminal supports it
- Automatic detection via COLORTERM=truecolor
- Enhancement of the -D color system to accept 24-bit hex values (e.g., -DN#2E75B6)

### 6.6 Clipboard Integration

- `Y` (in pgr mode) yanks the current line or visual selection to system clipboard
- Uses OS-native clipboard APIs (xclip/xsel on Linux, pbcopy on macOS)
- Disabled in LESSSECURE mode

---

## 7. Agent Interface (Pgr Protocol)

This is the core differentiator. The Pgr Protocol is a structured IPC mechanism that allows external programs — including AI agents, scripts, and development tools — to query and control pgr programmatically.

### 7.1 Protocol Overview

The Pgr Protocol is exposed via a Unix domain socket. When pgr starts with the `--agent` flag (or `PGR_AGENT=1`), it creates a socket at a well-known path (defaulting to `$XDG_RUNTIME_DIR/pgr/$PID.sock` or `/tmp/pgr-$PID.sock`). The socket path is also printed to stderr and set in the `PGR_SOCKET` environment variable for child processes.

The protocol uses newline-delimited JSON (NDJSON). Each request is a single JSON object on one line; each response is a single JSON object on one line.

**Request format:**
```json
{"id": "req-001", "method": "viewport.get", "params": {}}
```

**Response format:**
```json
{"id": "req-001", "result": { ... }}
```

**Error format:**
```json
{"id": "req-001", "error": {"code": -1, "message": "No file loaded"}}
```

**Event format (server-initiated, no id):**
```json
{"event": "content.changed", "data": {"line_count": 1523}}
```

### 7.2 Viewport Methods

These methods query and manipulate what is currently visible on screen.

#### `viewport.get`
Returns the current viewport state.

**Request:**
```json
{"id": "1", "method": "viewport.get", "params": {}}
```

**Response:**
```json
{
  "id": "1",
  "result": {
    "top_line": 42,
    "bottom_line": 65,
    "total_lines": 1523,
    "total_bytes": 98304,
    "byte_offset": 2048,
    "percent": 2.7,
    "columns": 80,
    "rows": 24,
    "filename": "/var/log/syslog",
    "file_index": 0,
    "file_count": 3,
    "horizontal_shift": 0,
    "following": false,
    "filtering": false,
    "filter_pattern": null
  }
}
```

#### `viewport.lines`
Returns the actual text content of the current viewport (or a specified range).

**Request:**
```json
{"id": "2", "method": "viewport.lines", "params": {"from": 42, "to": 65}}
```

**Response:**
```json
{
  "id": "2",
  "result": {
    "lines": [
      {"number": 42, "text": "Mar 22 10:15:03 host sshd[1234]: Accepted publickey for user", "matched": false, "marked": null},
      {"number": 43, "text": "Mar 22 10:15:03 host sshd[1234]: pam_unix(sshd:session): session opened", "matched": true, "marked": "a"}
    ]
  }
}
```

#### `viewport.scroll`
Scroll the viewport.

**Request:**
```json
{"id": "3", "method": "viewport.scroll", "params": {"direction": "forward", "amount": 10, "unit": "lines"}}
```

Units: `lines`, `pages`, `half_pages`, `percent`, `bytes`.

#### `viewport.goto`
Navigate to a specific position.

**Request:**
```json
{"id": "4", "method": "viewport.goto", "params": {"line": 500}}
```

Or: `{"percent": 50}`, `{"byte_offset": 4096}`, `{"mark": "a"}`, `{"position": "start"}`, `{"position": "end"}`.

### 7.3 Search Methods

#### `search.find`
Execute a search and return results without moving the viewport.

**Request:**
```json
{
  "id": "5",
  "method": "search.find",
  "params": {
    "pattern": "error|warning",
    "direction": "forward",
    "from_line": 1,
    "max_results": 50,
    "context_lines": 3,
    "regex": true,
    "case_sensitive": false
  }
}
```

**Response:**
```json
{
  "id": "5",
  "result": {
    "matches": [
      {
        "line": 87,
        "column": 15,
        "length": 5,
        "text": "Mar 22 10:15:03 error: connection refused",
        "context_before": ["line 84", "line 85", "line 86"],
        "context_after": ["line 88", "line 89", "line 90"]
      }
    ],
    "total_matches": 142,
    "truncated": true
  }
}
```

#### `search.navigate`
Move the viewport to the N-th match of the current (or specified) search pattern. Equivalent to the interactive / and n commands.

**Request:**
```json
{"id": "6", "method": "search.navigate", "params": {"pattern": "FATAL", "occurrence": 1, "direction": "forward"}}
```

#### `search.count`
Count total matches for a pattern without returning them.

**Request:**
```json
{"id": "7", "method": "search.count", "params": {"pattern": "TODO", "regex": false}}
```

**Response:**
```json
{"id": "7", "result": {"count": 23}}
```

#### `search.filter`
Apply a filter (equivalent to the & command). Only matching lines are displayed.

**Request:**
```json
{"id": "8", "method": "search.filter", "params": {"pattern": "level=error", "invert": false}}
```

To clear: `{"id": "9", "method": "search.filter", "params": {"pattern": null}}`

### 7.4 Content Methods

#### `content.extract`
Extract a range of lines as plain text or structured data.

**Request:**
```json
{
  "id": "10",
  "method": "content.extract",
  "params": {
    "from": 100,
    "to": 200,
    "format": "text"
  }
}
```

Formats: `text` (plain text, newline-separated), `json` (array of line objects with number and text fields), `raw` (bytes, base64-encoded).

#### `content.pipe`
Pipe a range of content through an external command and return the result. Equivalent to the | command.

**Request:**
```json
{
  "id": "11",
  "method": "content.pipe",
  "params": {
    "from_mark": "a",
    "to": "screen",
    "command": "grep -c error"
  }
}
```

#### `content.stats`
Return statistical information about the loaded content.

**Request:**
```json
{"id": "12", "method": "content.stats", "params": {}}
```

**Response:**
```json
{
  "id": "12",
  "result": {
    "filename": "/var/log/syslog",
    "total_lines": 15230,
    "total_bytes": 1048576,
    "longest_line": 512,
    "average_line_length": 68,
    "is_pipe": false,
    "is_growing": false,
    "charset": "utf-8",
    "binary": false,
    "compressed": false,
    "compression_format": null
  }
}
```

### 7.5 File Methods

#### `file.list`
List all files in the current file list.

**Response:**
```json
{
  "id": "13",
  "result": {
    "files": [
      {"index": 0, "name": "app.log", "current": true},
      {"index": 1, "name": "error.log", "current": false}
    ]
  }
}
```

#### `file.open`
Open a new file (equivalent to :e).

#### `file.next` / `file.prev`
Navigate to next/previous file.

### 7.6 Mark Methods

#### `marks.list`
List all current marks.

#### `marks.set`
Set a mark at a specified line.

#### `marks.clear`
Clear a mark.

### 7.7 Follow Mode Methods

#### `follow.start`
Enter follow mode (equivalent to F command).

#### `follow.stop`
Exit follow mode.

#### `follow.status`
Query whether follow mode is active and whether new content has arrived.

### 7.8 Event Subscriptions

Clients can subscribe to events to receive real-time notifications.

#### `events.subscribe`

**Request:**
```json
{"id": "20", "method": "events.subscribe", "params": {"events": ["content.changed", "viewport.changed", "search.match", "follow.new_data"]}}
```

**Available events:**

| Event | Data | Description |
|-------|------|-------------|
| `viewport.changed` | `{top_line, bottom_line}` | Viewport was scrolled or resized |
| `content.changed` | `{line_count, byte_count}` | File content changed (follow mode, reload) |
| `search.match` | `{pattern, line, column}` | New match found during ESC-F follow-search |
| `follow.new_data` | `{new_lines, new_bytes}` | New data arrived in follow mode |
| `file.changed` | `{index, name}` | Active file changed |
| `filter.changed` | `{pattern, visible_lines}` | Filter applied or cleared |

### 7.9 Batch Mode

For agents that don't need interactive paging, pgr supports a batch mode that processes commands from stdin and writes results to stdout without terminal interaction.

```bash
echo '{"method":"search.find","params":{"pattern":"error"}}' | pgr --batch logfile.txt
```

### 7.10 WASM Interface

When compiled to WASM for Firebox, the Unix socket is replaced with a message-passing interface exposed via WASM imports/exports. The protocol is identical (NDJSON over a byte stream), but the transport is Firebox's inter-module communication channel rather than a Unix socket.

```rust
// WASM host interface (Firebox side)
#[link(wasm_import_module = "pgr")]
extern "C" {
    fn pgr_send(ptr: *const u8, len: u32);
    fn pgr_recv(ptr: *mut u8, max_len: u32) -> u32;
}
```

---

## 8. Crate Structure

```
pgr/
├── Cargo.toml                 # Workspace root
├── crates/
│   ├── pgr-core/             # Core data structures, buffer management, line indexing
│   │   └── src/
│   │       ├── buffer.rs      # Input buffering (file, pipe, growing file)
│   │       ├── line_index.rs  # Line offset index, byte-to-line mapping
│   │       ├── charset.rs     # Character set detection, UTF-8 handling
│   │       ├── filter.rs      # Line filtering (& command)
│   │       └── marks.rs       # Mark storage and retrieval
│   │
│   ├── pgr-input/            # Input handling: file, pipe, preprocessor
│   │   └── src/
│   │       ├── file.rs        # Regular file reading with mmap support
│   │       ├── pipe.rs        # Pipe/stdin reading with dynamic buffering
│   │       ├── preproc.rs     # LESSOPEN/LESSCLOSE pipeline
│   │       ├── decompress.rs  # Native decompression (gzip, xz, zstd, etc.)
│   │       └── follow.rs      # Follow mode (inotify/kqueue + poll)
│   │
│   ├── pgr-search/           # Search engine
│   │   └── src/
│   │       ├── regex.rs       # Regex compilation and matching
│   │       ├── literal.rs     # Literal (non-regex) search
│   │       ├── highlight.rs   # Match highlighting state
│   │       └── filter.rs      # Filter mode (& command)
│   │
│   ├── pgr-display/          # Terminal rendering
│   │   └── src/
│   │       ├── screen.rs      # Screen state, dirty tracking
│   │       ├── render.rs      # Line rendering (ANSI, overstrike, wrapping)
│   │       ├── prompt.rs      # Prompt string evaluation (%escapes)
│   │       ├── status.rs      # Status column rendering (-J)
│   │       ├── color.rs       # Color system (-D / --color)
│   │       ├── ansi.rs        # ANSI escape sequence parsing (-r/-R)
│   │       └── unicode.rs     # Display width calculation (EAW, emoji)
│   │
│   ├── pgr-keys/             # Input handling and key binding
│   │   └── src/
│   │       ├── terminal.rs    # Raw terminal mode, key reading
│   │       ├── keymap.rs      # Default key bindings
│   │       ├── lesskey.rs     # lesskey source file parser
│   │       ├── command.rs     # Command dispatch
│   │       └── line_edit.rs   # Command-line editing (search, :e, etc.)
│   │
│   ├── pgr-syntax/           # Syntax highlighting (optional feature)
│   │   └── src/
│   │       ├── detect.rs      # File type detection
│   │       ├── highlight.rs   # Tree-sitter integration
│   │       └── theme.rs       # Theme loading
│   │
│   ├── pgr-agent/            # Agent interface (Pgr Protocol)
│   │   └── src/
│   │       ├── server.rs      # Unix socket server
│   │       ├── protocol.rs    # NDJSON request/response parsing
│   │       ├── handlers.rs    # Method dispatch
│   │       ├── events.rs      # Event subscription management
│   │       └── batch.rs       # Batch mode (stdin/stdout)
│   │
│   └── pgr-cli/              # Binary entry point
│       └── src/
│           ├── main.rs        # Argument parsing, initialization
│           ├── options.rs     # All command-line options
│           ├── env.rs         # Environment variable handling
│           └── security.rs    # LESSSECURE enforcement
│
├── tests/                     # Integration tests
│   ├── conformance/           # less conformance tests
│   │   ├── options/           # Per-flag tests
│   │   ├── commands/          # Interactive command tests
│   │   ├── search/            # Search behavior tests
│   │   ├── prompt/            # Prompt string tests
│   │   ├── lesskey/           # lesskey parsing tests
│   │   └── compat/           # POSIX more compatibility tests
│   │
│   ├── agent/                 # Pgr Protocol tests
│   │   ├── viewport/
│   │   ├── search/
│   │   ├── content/
│   │   ├── events/
│   │   └── batch/
│   │
│   ├── regression/            # Regression tests against GNU less
│   └── fuzz/                  # Fuzz testing targets
│
└── fixtures/                  # Test input files
    ├── basic.txt
    ├── utf8.txt
    ├── ansi.txt
    ├── binary.bin
    ├── large.txt              # Generated: multi-MB file
    ├── growing.sh             # Script that appends to a file
    ├── compressed/
    └── lesskey/
```

---

## 9. Build Targets

| Target | Platform | Notes |
|--------|----------|-------|
| x86_64-unknown-linux-gnu | Linux (primary) | Full feature set |
| aarch64-unknown-linux-gnu | Linux ARM | Full feature set |
| x86_64-apple-darwin | macOS Intel | Full feature set |
| aarch64-apple-darwin | macOS Apple Silicon | Full feature set |
| wasm32-wasip1 | WASM (Firebox) | No Unix socket; uses WASM message passing. No syntax highlighting in initial build. |
| wasm32-unknown-unknown | WASM (Browser) | Reduced feature set; rendering to virtual terminal |

---

## 10. Non-Goals (Explicit Exclusions)

- **Not a file viewer.** pgr is not bat. It does not default to syntax highlighting or line numbers. It is a pager first; enhancements are opt-in.
- **Not a terminal multiplexer.** pgr does not manage splits, panes, or sessions.
- **Not an editor.** The `v` command launches an external editor per less behavior, but pgr itself has no editing capability.
- **Not a log analysis tool.** The agent interface enables agents to build log analysis on top of pgr, but pgr itself does not parse log formats, aggregate data, or produce dashboards.
- **No GUI.** pgr is a terminal application. A hypothetical GUI wrapper using the Pgr Protocol would be a separate project.

---

## 11. Development Methodology

pgr is developed using an agent swarm methodology where the architect (Justin) maintains architectural vision, trait definitions, and test specifications while agent contributors handle implementation within strict quality guardrails:

- **Comprehensive test coverage:** Every public function and every conformance behavior has associated tests before implementation begins (test-first).
- **Clippy pedantic:** `#![warn(clippy::pedantic)]` on all crates.
- **Review agent:** A dedicated review agent checks all contributions for alignment with this specification, architectural consistency, and test coverage.
- **CI gates:** All PRs must pass: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `cargo doc --no-deps`, and the conformance regression suite.

---

## 12. Phasing

### Phase 0: Foundation (Target: Working pager for simple files)
- pgr-core: buffer, line index
- pgr-input: file reading (no pipe, no preprocessor, no follow)
- pgr-display: basic screen rendering, basic prompt
- pgr-keys: raw terminal, default keymap, basic navigation commands
- pgr-cli: argument parsing for core flags (-e, -F, -N, -S, -R, -i, -q)
- Tests: Navigation, basic display, quit

### Phase 1: Core Conformance (Target: Passes 80% of conformance tests)
- pgr-input: pipe reading, dynamic buffering
- pgr-search: forward/backward search, highlighting, n/N
- pgr-display: full prompt string evaluation, ANSI handling, color system
- pgr-keys: all navigation commands, search commands, file commands
- All remaining command-line flags
- Tests: Search, all commands, all flags

### Phase 2: Full Conformance (Target: 100% conformance)
- pgr-input: LESSOPEN/LESSCLOSE, follow mode
- pgr-keys: lesskey source file parsing, full key binding system
- pgr-display: status column, OSC 8 hyperlinks, bracket matching
- Security mode (LESSSECURE)
- POSIX more compatibility mode
- Tests: Preprocessor, lesskey, security, more compat, regression against less

### Phase 3: Agent Interface (Target: Pgr Protocol v1)
- pgr-agent: Unix socket server, NDJSON protocol
- All viewport, search, content, file, mark, follow, event methods
- Batch mode
- Tests: Full protocol test suite

### Phase 4: Enhancements (Target: Modern pager features)
- pgr-syntax: tree-sitter integration
- pgr-input: native decompression
- Enhanced Unicode (EAW, emoji)
- True color
- Clipboard integration
- WASM compilation target

### Phase 5: Firebox Integration
- WASM message-passing transport for Pgr Protocol
- Browser virtual terminal rendering
- Firebox runtime registration as built-in pager

---

## 13. Success Criteria

1. **Drop-in replacement.** `alias less=pgr` causes zero breakage in a user's existing workflow, including git, man, systemctl, journalctl, and any tool that uses $PAGER.
2. **Performance parity or better.** Startup time, scrolling latency, and search speed are at least as fast as less on the same hardware. Large file handling (multi-GB) does not degrade.
3. **Agent utility proven.** At least one AI agent workflow (e.g., a Claude Code task, a Firebox agent script) demonstrably uses the Pgr Protocol to inspect and navigate content that would otherwise require ad-hoc stdout buffering.
4. **Community adoption signal.** Packaged for at least two Linux distributions (AUR, Homebrew) within six months of v1.0 release.

---

## 14. Open Questions

- **Name collision check:** Is "pgr" taken on crates.io? Need to verify before committing to the name. Fallback: `lns`, `pore`, `reed`.
- **lesskey binary format:** Should pgr support the legacy lesskey binary format, or only the source format? Modern less reads source directly; the binary format is deprecated.
- **PCRE2 support:** Should PCRE2 be a compile-time feature flag, or should pgr always use the Rust regex crate with a PCRE2-compatible syntax subset?
- **Tree-sitter grammar bundling:** Should grammars be compiled in, loaded from disk, or fetched on first use?
- **License interaction:** less is dual-licensed GPL-3.0/Less License. pgr is Apache 2.0, a clean-room implementation from the man page specification. No code is derived from less source. Verify this is clean.
