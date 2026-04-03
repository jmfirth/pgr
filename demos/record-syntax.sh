#!/bin/bash
# Scripted demo: syntax highlighting in pgr
# Uses tmux to send keystrokes with real timing.
# Run: asciinema rec --command "./demos/record-syntax.sh" --cols 100 --rows 30 demos/syntax-highlight.cast

PGR="$(pwd)/target/release/pgr-cli"
FILE="/tmp/pgr_demo.rs"
SESSION="pgr-demo-$$"

# Create demo file
cat > "$FILE" << 'RUST'
use std::collections::HashMap;

/// A simple key-value store with expiration.
pub struct Cache<V> {
    entries: HashMap<String, (V, std::time::Instant)>,
    ttl: std::time::Duration,
}

impl<V: Clone> Cache<V> {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            entries: HashMap::new(),
            ttl: std::time::Duration::from_secs(ttl_secs),
        }
    }

    pub fn get(&self, key: &str) -> Option<V> {
        self.entries.get(key).and_then(|(val, created)| {
            if created.elapsed() < self.ttl {
                Some(val.clone())
            } else {
                None
            }
        })
    }

    pub fn set(&mut self, key: String, value: V) {
        self.entries.insert(key, (value, std::time::Instant::now()));
    }
}

fn main() {
    let mut cache = Cache::new(300);
    cache.set("user:42".into(), "Alice".to_string());

    match cache.get("user:42") {
        Some(name) => println!("Found: {name}"),
        None => println!("Cache miss"),
    }
}
RUST

# Kill any leftover session
tmux kill-session -t "$SESSION" 2>/dev/null

# Start pgr in a detached tmux session
tmux new-session -d -s "$SESSION" -x 100 -y 30 "$PGR $FILE"

# Background: send keystrokes with real timing
{
    sleep 1.5

    # Jump to top
    tmux send-keys -t "$SESSION" g
    sleep 1.5

    # Scroll down slowly
    tmux send-keys -t "$SESSION" j; sleep 0.8
    tmux send-keys -t "$SESSION" j; sleep 0.8
    tmux send-keys -t "$SESSION" j; sleep 0.8
    tmux send-keys -t "$SESSION" j; sleep 0.8
    tmux send-keys -t "$SESSION" j; sleep 0.8
    tmux send-keys -t "$SESSION" j; sleep 0.8
    tmux send-keys -t "$SESSION" j; sleep 0.8
    tmux send-keys -t "$SESSION" j; sleep 0.8
    sleep 1.5

    # Jump to top
    tmux send-keys -t "$SESSION" g
    sleep 1.5

    # Search for "fn"
    tmux send-keys -t "$SESSION" /fn Enter
    sleep 1.5

    # Next match
    tmux send-keys -t "$SESSION" n
    sleep 1.5
    tmux send-keys -t "$SESSION" n
    sleep 1.5
    tmux send-keys -t "$SESSION" n
    sleep 1.5

    # Quit
    tmux send-keys -t "$SESSION" q
} &
KEYS_PID=$!

# Attach — this is what asciinema captures
tmux attach -t "$SESSION"

# Cleanup
wait $KEYS_PID 2>/dev/null
tmux kill-session -t "$SESSION" 2>/dev/null
