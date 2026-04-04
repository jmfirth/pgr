#!/bin/bash
# Scripted demo: syntax highlighting in pgr
# Run: asciinema rec --cols 100 --rows 30 --overwrite -c "./demos/record-syntax.sh" demos/syntax-highlight.cast

export TERM=xterm-256color
PGR="$(pwd)/target/release/pgr-cli"
FILE="/tmp/pgr_demo.rs"
SOCK="pgr-demo-$$"

# Create demo file
cat > "$FILE" << 'RUST'
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A simple key-value store with TTL-based expiration.
///
/// Each entry is stored alongside its creation timestamp. When a value
/// is retrieved, the cache checks whether the entry has expired based
/// on the configured TTL (time-to-live) duration.
pub struct Cache<V> {
    entries: HashMap<String, (V, Instant)>,
    ttl: Duration,
    hits: u64,
    misses: u64,
}

impl<V: Clone> Cache<V> {
    /// Create a new cache with the given TTL in seconds.
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            entries: HashMap::new(),
            ttl: Duration::from_secs(ttl_secs),
            hits: 0,
            misses: 0,
        }
    }

    /// Retrieve a value from the cache.
    ///
    /// Returns `Some(value)` if the key exists and hasn't expired,
    /// `None` otherwise. Expired entries are not removed eagerly —
    /// they remain until overwritten or the cache is pruned.
    pub fn get(&mut self, key: &str) -> Option<V> {
        match self.entries.get(key) {
            Some((val, created)) if created.elapsed() < self.ttl => {
                self.hits += 1;
                Some(val.clone())
            }
            Some(_) => {
                self.misses += 1;
                None // expired
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    /// Insert or update a value in the cache.
    pub fn set(&mut self, key: String, value: V) {
        self.entries.insert(key, (value, Instant::now()));
    }

    /// Remove expired entries from the cache.
    pub fn prune(&mut self) -> usize {
        let before = self.entries.len();
        self.entries.retain(|_, (_, created)| created.elapsed() < self.ttl);
        before - self.entries.len()
    }

    /// Returns cache statistics: (hits, misses, entries, hit_rate).
    pub fn stats(&self) -> (u64, u64, usize, f64) {
        let total = self.hits + self.misses;
        let rate = if total > 0 {
            self.hits as f64 / total as f64
        } else {
            0.0
        };
        (self.hits, self.misses, self.entries.len(), rate)
    }
}

fn main() {
    let mut cache = Cache::new(300);

    // Populate the cache with some test data
    for i in 0..20 {
        cache.set(format!("user:{i}"), format!("User #{i}"));
    }

    // Simulate lookups
    for i in 0..25 {
        match cache.get(&format!("user:{i}")) {
            Some(name) => println!("[HIT]  user:{i} => {name}"),
            None => println!("[MISS] user:{i}"),
        }
    }

    // Print statistics
    let (hits, misses, entries, rate) = cache.stats();
    println!("\nCache stats:");
    println!("  Entries: {entries}");
    println!("  Hits:    {hits}");
    println!("  Misses:  {misses}");
    println!("  Rate:    {rate:.1}%");

    // Prune expired entries
    let pruned = cache.prune();
    println!("  Pruned:  {pruned}");
}
RUST

# Use a SEPARATE tmux server socket — no interference from existing sessions
unset TMUX
tmux -L "$SOCK" kill-server 2>/dev/null
tmux -L "$SOCK" new-session -d -s demo -x 100 -y 30 "$PGR $FILE"
tmux -L "$SOCK" set -g default-terminal "xterm-256color"
tmux -L "$SOCK" set -ga terminal-overrides ",xterm-256color:Tc"

# Configure status bar for keystroke overlay
tmux -L "$SOCK" set -t demo status on
tmux -L "$SOCK" set -t demo status-style "fg=white,bg=#333333"
tmux -L "$SOCK" set -t demo status-left ""
tmux -L "$SOCK" set -t demo status-right ""
tmux -L "$SOCK" set -t demo status-justify centre

show_key() { tmux -L "$SOCK" set -t demo status-left "  $1"; }
clear_key() { tmux -L "$SOCK" set -t demo status-left ""; }

# Background: send scripted keystrokes
{
    sleep 3

    # Scroll down
    show_key "j  (scroll down)"
    for i in $(seq 1 12); do
        tmux -L "$SOCK" send-keys -t demo j; sleep 0.4
    done
    clear_key
    sleep 1.5

    # Jump to top
    show_key "g  (go to top)"
    tmux -L "$SOCK" send-keys -t demo g
    sleep 2
    clear_key

    # Search for "fn"
    show_key "/fn  (search)"
    tmux -L "$SOCK" send-keys -t demo /fn Enter
    sleep 2

    # Next match
    show_key "n  (next match)"
    tmux -L "$SOCK" send-keys -t demo n
    sleep 1.5
    tmux -L "$SOCK" send-keys -t demo n
    sleep 1.5
    tmux -L "$SOCK" send-keys -t demo n
    sleep 2
    clear_key

    # Quit
    show_key "q  (quit)"
    tmux -L "$SOCK" send-keys -t demo q
} &

# Attach �� this is the foreground process asciinema captures
tmux -L "$SOCK" attach -t demo
wait
tmux -L "$SOCK" kill-server 2>/dev/null
