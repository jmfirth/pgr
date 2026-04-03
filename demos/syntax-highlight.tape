Output demos/syntax-highlight.gif
Set Shell "bash"
Set FontSize 14
Set Width 100
Set Height 35
Set Padding 10
Set Theme "Catppuccin Mocha"
Set TypingSpeed 50ms

# Open a Rust file with syntax highlighting
Type "target/release/pgr-cli -X /tmp/demo_file.rs"
Enter
Sleep 3s

# Scroll down to show more
Type "j"
Sleep 300ms
Type "j"
Sleep 300ms
Type "j"
Sleep 300ms
Type "j"
Sleep 300ms
Type "j"
Sleep 2s

# Scroll back up
Type "g"
Sleep 2s

# Quit
Type "q"
Sleep 1s
