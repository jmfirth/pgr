#!/bin/bash
# Scripted demo: syntax highlighting in pgr
# Run via: asciinema rec --command ./demos/record-syntax.sh demos/syntax-highlight.cast

export TERM=xterm-256color
export COLUMNS=100
export LINES=30

PGR="$(pwd)/target/release/pgr-cli"
FILE="/tmp/pgr_demo.rs"

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

# Feed keystrokes with delays
{
    sleep 2
    # Scroll down slowly
    for i in 1 2 3 4 5 6 7 8; do
        printf 'j'
        sleep 0.3
    done
    sleep 1.5
    # Jump to top
    printf 'g'
    sleep 1.5
    # Search for "fn"
    printf '/fn\n'
    sleep 1.5
    # Next match
    printf 'n'
    sleep 1
    printf 'n'
    sleep 1.5
    # Quit
    printf 'q'
} | "$PGR" "$FILE"
