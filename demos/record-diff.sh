#!/bin/bash
# Scripted demo: diff awareness in pgr
# Run: asciinema rec --cols 140 --rows 30 --overwrite -c "./demos/record-diff.sh" demos/diff-awareness.cast

export TERM=xterm-256color
PGR="$(pwd)/target/release/pgr-cli"
DIFF_FILE="/tmp/pgr_demo_diff.txt"
SOCK="pgr-demo-$$"

# Generate a realistic diff
cat > "$DIFF_FILE" << 'DIFF'
diff --git a/src/cache.rs b/src/cache.rs
index abc1234..def5678 100644
--- a/src/cache.rs
+++ b/src/cache.rs
@@ -10,8 +10,12 @@ pub struct Cache<V> {
     entries: HashMap<String, (V, Instant)>,
     ttl: Duration,
+    hits: u64,
+    misses: u64,
 }

 impl<V: Clone> Cache<V> {
-    pub fn new(ttl_secs: u64) -> Self {
+    /// Create a new cache with the given TTL in seconds.
+    pub fn new(ttl_secs: u64, max_entries: usize) -> Self {
         Self {
             entries: HashMap::new(),
             ttl: Duration::from_secs(ttl_secs),
+            hits: 0,
+            misses: 0,
         }
     }

-    pub fn get(&self, key: &str) -> Option<V> {
-        self.entries.get(key).and_then(|(val, created)| {
+    pub fn get(&mut self, key: &str) -> Option<V> {
+        match self.entries.get(key) {
+            Some((val, created)) if created.elapsed() < self.ttl => {
+                self.hits += 1;
+                Some(val.clone())
+            }
+            Some(_) => {
+                self.misses += 1;
+                None // expired
+            }
+            None => {
+                self.misses += 1;
+                None
+            }
+        }
+    }
+
+    /// Remove expired entries from the cache.
+    pub fn prune(&mut self) -> usize {
+        let before = self.entries.len();
+        self.entries.retain(|_, (_, created)| created.elapsed() < self.ttl);
+        before - self.entries.len()
     }
 }
DIFF

# Use a SEPARATE tmux server socket — no interference from existing sessions
unset TMUX
tmux -L "$SOCK" kill-server 2>/dev/null
tmux -L "$SOCK" new-session -d -s demo -x 140 -y 30 "$PGR $DIFF_FILE"
tmux -L "$SOCK" set -g default-terminal "xterm-256color"
tmux -L "$SOCK" set -ga terminal-overrides ",xterm-256color:Tc"

tmux -L "$SOCK" set -t demo status on
tmux -L "$SOCK" set -t demo status-style "fg=white,bg=#333333"
tmux -L "$SOCK" set -t demo status-left ""
tmux -L "$SOCK" set -t demo status-right ""
tmux -L "$SOCK" set -t demo status-justify centre

show_key() { tmux -L "$SOCK" set -t demo status-left "  $1"; }
clear_key() { tmux -L "$SOCK" set -t demo status-left ""; }

{
    sleep 3

    # Scroll through the diff
    show_key "j  (scroll)"
    for i in $(seq 1 12); do
        tmux -L "$SOCK" send-keys -t demo j; sleep 0.3
    done
    sleep 2
    clear_key

    # Jump to top
    show_key "g  (top)"
    tmux -L "$SOCK" send-keys -t demo g
    sleep 2
    clear_key

    # Hunk navigation
    show_key "]c  (next hunk)"
    tmux -L "$SOCK" send-keys -t demo ']'
    sleep 0.1
    tmux -L "$SOCK" send-keys -t demo c
    sleep 2

    tmux -L "$SOCK" send-keys -t demo ']'
    sleep 0.1
    tmux -L "$SOCK" send-keys -t demo c
    sleep 2
    clear_key

    # Side-by-side
    show_key "ESC-V  (side-by-side)"
    tmux -L "$SOCK" send-keys -t demo Escape
    sleep 0.1
    tmux -L "$SOCK" send-keys -t demo V
    sleep 4
    clear_key

    # Back to unified
    show_key "ESC-V  (unified)"
    tmux -L "$SOCK" send-keys -t demo Escape
    sleep 0.1
    tmux -L "$SOCK" send-keys -t demo V
    sleep 2
    clear_key

    # Quit
    show_key "q  (quit)"
    tmux -L "$SOCK" send-keys -t demo q
} &

tmux -L "$SOCK" attach -t demo
wait
tmux -L "$SOCK" kill-server 2>/dev/null
