#!/bin/bash
# Scripted demo: SQL table mode in pgr
# Run: asciinema rec --cols 120 --rows 30 --overwrite -c "./demos/record-table.sh" demos/table-view.cast

export TERM=xterm-256color
PGR="$(pwd)/target/release/pgr-cli"
FILE="/tmp/pgr_demo_table.txt"
SOCK="pgr-demo-$$"

# Generate a realistic SQL query result
cat > "$FILE" << 'TABLE'
 id | username       | email                        | role      | department       | status   | last_login          | created_at          | country        | salary
----+----------------+------------------------------+-----------+------------------+----------+---------------------+---------------------+----------------+---------
  1 | alice.johnson  | alice.johnson@example.com    | admin     | Engineering      | active   | 2026-04-03 14:22:01 | 2024-01-15 09:00:00 | United States  |  145000
  2 | bob.smith      | bob.smith@example.com        | developer | Engineering      | active   | 2026-04-03 11:45:33 | 2024-02-20 10:30:00 | Canada         |  128000
  3 | carol.white    | carol.white@example.com      | designer  | Product          | active   | 2026-04-02 16:10:22 | 2024-03-01 08:15:00 | United Kingdom |  115000
  4 | dave.brown     | dave.brown@example.com       | developer | Engineering      | inactive | 2026-03-15 09:30:00 | 2024-03-10 11:00:00 | Germany        |  132000
  5 | eve.davis      | eve.davis@example.com        | manager   | Product          | active   | 2026-04-03 15:00:45 | 2024-04-05 14:20:00 | France         |  155000
  6 | frank.miller   | frank.miller@example.com     | developer | Infrastructure   | active   | 2026-04-03 10:12:18 | 2024-04-15 09:45:00 | United States  |  138000
  7 | grace.wilson   | grace.wilson@example.com     | analyst   | Data Science     | active   | 2026-04-03 13:55:40 | 2024-05-01 10:00:00 | Australia      |  121000
  8 | hank.moore     | hank.moore@example.com       | developer | Engineering      | active   | 2026-04-02 17:30:22 | 2024-05-20 08:30:00 | Canada         |  130000
  9 | iris.taylor    | iris.taylor@example.com      | designer  | Product          | active   | 2026-04-03 09:20:15 | 2024-06-10 11:15:00 | Japan          |  118000
 10 | jack.anderson  | jack.anderson@example.com    | devops    | Infrastructure   | active   | 2026-04-03 14:45:00 | 2024-06-25 13:00:00 | United States  |  142000
 11 | karen.thomas   | karen.thomas@example.com     | manager   | Engineering      | active   | 2026-04-03 16:10:33 | 2024-07-01 09:30:00 | Ireland        |  160000
 12 | leo.jackson    | leo.jackson@example.com      | developer | Data Science     | active   | 2026-04-02 15:22:10 | 2024-07-15 10:45:00 | Netherlands    |  135000
 13 | mia.harris     | mia.harris@example.com       | analyst   | Data Science     | inactive | 2026-03-20 11:00:00 | 2024-08-01 08:00:00 | Sweden         |  119000
 14 | nick.clark     | nick.clark@example.com       | developer | Engineering      | active   | 2026-04-03 12:35:48 | 2024-08-20 14:30:00 | United States  |  131000
 15 | olivia.lewis   | olivia.lewis@example.com     | designer  | Product          | active   | 2026-04-03 11:10:25 | 2024-09-05 09:15:00 | Spain          |  116000
 16 | peter.walker   | peter.walker@example.com     | devops    | Infrastructure   | active   | 2026-04-02 18:45:30 | 2024-09-15 10:00:00 | Germany        |  140000
 17 | quinn.hall     | quinn.hall@example.com       | developer | Engineering      | active   | 2026-04-03 14:00:12 | 2024-10-01 11:30:00 | Canada         |  129000
 18 | rachel.young   | rachel.young@example.com     | manager   | Data Science     | active   | 2026-04-03 15:30:00 | 2024-10-20 08:45:00 | United States  |  158000
 19 | sam.king       | sam.king@example.com         | analyst   | Product          | active   | 2026-04-03 10:50:33 | 2024-11-01 13:00:00 | Australia      |  117000
 20 | tina.wright    | tina.wright@example.com      | developer | Infrastructure   | inactive | 2026-03-28 09:15:00 | 2024-11-15 09:30:00 | Japan          |  133000
(20 rows)
TABLE

# Use a SEPARATE tmux server socket
unset TMUX
tmux -L "$SOCK" kill-server 2>/dev/null
tmux -L "$SOCK" new-session -d -s demo -x 120 -y 30 "$PGR $FILE"
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

    # Scroll down through rows
    show_key "j  (scroll down)"
    for i in $(seq 1 10); do
        tmux -L "$SOCK" send-keys -t demo j; sleep 0.3
    done
    sleep 1.5
    clear_key

    # Back to top — sticky header stays visible
    show_key "g  (top)"
    tmux -L "$SOCK" send-keys -t demo g
    sleep 2
    clear_key

    # Scroll right to see more columns
    show_key "→  (scroll right)"
    for i in $(seq 1 8); do
        tmux -L "$SOCK" send-keys -t demo Right; sleep 0.5
    done
    sleep 2
    clear_key

    # Scroll back left
    show_key "←  (scroll left)"
    for i in $(seq 1 8); do
        tmux -L "$SOCK" send-keys -t demo Left; sleep 0.5
    done
    sleep 2
    clear_key

    # Scroll down while scrolled — header stays
    show_key "j  (scroll — header stays)"
    for i in $(seq 1 8); do
        tmux -L "$SOCK" send-keys -t demo j; sleep 0.3
    done
    sleep 2
    clear_key

    # Quit
    show_key "q  (quit)"
    tmux -L "$SOCK" send-keys -t demo q
} &

tmux -L "$SOCK" attach -t demo
wait
tmux -L "$SOCK" kill-server 2>/dev/null
