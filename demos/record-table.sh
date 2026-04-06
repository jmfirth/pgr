#!/bin/bash
# Scripted demo: SQL table mode in pgr
# Run: asciinema rec --cols 120 --rows 30 --overwrite -c "./demos/record-table.sh" demos/table-view.cast

export TERM=xterm-256color
PGR="$(pwd)/target/release/pgr-cli"
FILE="/tmp/pgr_demo_table.txt"
SOCK="pgr-demo-$$"

# Generate a realistic SQL query result
cat > "$FILE" << 'TABLE'
 id | username        | email                         | role      | department       | status   | last_login          | created_at          | country        | salary
----+-----------------+-------------------------------+-----------+------------------+----------+---------------------+---------------------+----------------+---------
  1 | alice.johnson   | alice.johnson@example.com     | admin     | Engineering      | active   | 2026-04-03 14:22:01 | 2024-01-15 09:00:00 | United States  |  145000
  2 | bob.smith       | bob.smith@example.com         | developer | Engineering      | active   | 2026-04-03 11:45:33 | 2024-02-20 10:30:00 | Canada         |  128000
  3 | carol.white     | carol.white@example.com       | designer  | Product          | active   | 2026-04-02 16:10:22 | 2024-03-01 08:15:00 | United Kingdom |  115000
  4 | dave.brown      | dave.brown@example.com        | developer | Engineering      | inactive | 2026-03-15 09:30:00 | 2024-03-10 11:00:00 | Germany        |  132000
  5 | eve.davis       | eve.davis@example.com         | manager   | Product          | active   | 2026-04-03 15:00:45 | 2024-04-05 14:20:00 | France         |  155000
  6 | frank.miller    | frank.miller@example.com      | developer | Infrastructure   | active   | 2026-04-03 10:12:18 | 2024-04-15 09:45:00 | United States  |  138000
  7 | grace.wilson    | grace.wilson@example.com      | analyst   | Data Science     | active   | 2026-04-03 13:55:40 | 2024-05-01 10:00:00 | Australia      |  121000
  8 | hank.moore      | hank.moore@example.com        | developer | Engineering      | active   | 2026-04-02 17:30:22 | 2024-05-20 08:30:00 | Canada         |  130000
  9 | iris.taylor     | iris.taylor@example.com       | designer  | Product          | active   | 2026-04-03 09:20:15 | 2024-06-10 11:15:00 | Japan          |  118000
 10 | jack.anderson   | jack.anderson@example.com     | devops    | Infrastructure   | active   | 2026-04-03 14:45:00 | 2024-06-25 13:00:00 | United States  |  142000
 11 | karen.thomas    | karen.thomas@example.com      | manager   | Engineering      | active   | 2026-04-03 16:10:33 | 2024-07-01 09:30:00 | Ireland        |  160000
 12 | leo.jackson     | leo.jackson@example.com       | developer | Data Science     | active   | 2026-04-02 15:22:10 | 2024-07-15 10:45:00 | Netherlands    |  135000
 13 | mia.harris      | mia.harris@example.com        | analyst   | Data Science     | inactive | 2026-03-20 11:00:00 | 2024-08-01 08:00:00 | Sweden         |  119000
 14 | nick.clark      | nick.clark@example.com        | developer | Engineering      | active   | 2026-04-03 12:35:48 | 2024-08-20 14:30:00 | United States  |  131000
 15 | olivia.lewis    | olivia.lewis@example.com      | designer  | Product          | active   | 2026-04-03 11:10:25 | 2024-09-05 09:15:00 | Spain          |  116000
 16 | peter.walker    | peter.walker@example.com      | devops    | Infrastructure   | active   | 2026-04-02 18:45:30 | 2024-09-15 10:00:00 | Germany        |  140000
 17 | quinn.hall      | quinn.hall@example.com        | developer | Engineering      | active   | 2026-04-03 14:00:12 | 2024-10-01 11:30:00 | Canada         |  129000
 18 | rachel.young    | rachel.young@example.com      | manager   | Data Science     | active   | 2026-04-03 15:30:00 | 2024-10-20 08:45:00 | United States  |  158000
 19 | sam.king        | sam.king@example.com          | analyst   | Product          | active   | 2026-04-03 10:50:33 | 2024-11-01 13:00:00 | Australia      |  117000
 20 | tina.wright     | tina.wright@example.com       | developer | Infrastructure   | inactive | 2026-03-28 09:15:00 | 2024-11-15 09:30:00 | Japan          |  133000
 21 | uma.patel       | uma.patel@example.com         | developer | Engineering      | active   | 2026-04-03 09:10:00 | 2024-12-01 10:00:00 | India          |  126000
 22 | victor.chen     | victor.chen@example.com       | architect | Engineering      | active   | 2026-04-03 15:20:44 | 2024-12-15 08:30:00 | Singapore      |  165000
 23 | wendy.kim       | wendy.kim@example.com         | designer  | Product          | active   | 2026-04-03 11:30:15 | 2025-01-05 09:00:00 | South Korea    |  122000
 24 | xavier.lopez    | xavier.lopez@example.com      | developer | Infrastructure   | active   | 2026-04-02 14:55:30 | 2025-01-20 10:15:00 | Mexico         |  127000
 25 | yuki.tanaka     | yuki.tanaka@example.com       | analyst   | Data Science     | active   | 2026-04-03 16:40:22 | 2025-02-01 11:00:00 | Japan          |  120000
 26 | zara.ahmed      | zara.ahmed@example.com        | manager   | Product          | active   | 2026-04-03 13:15:10 | 2025-02-15 08:45:00 | Pakistan       |  152000
 27 | adam.fischer    | adam.fischer@example.com      | devops    | Infrastructure   | active   | 2026-04-03 10:05:33 | 2025-03-01 09:30:00 | Austria        |  139000
 28 | beth.murphy     | beth.murphy@example.com       | developer | Engineering      | active   | 2026-04-02 17:10:45 | 2025-03-10 10:00:00 | Ireland        |  131000
 29 | chris.wong      | chris.wong@example.com        | analyst   | Data Science     | inactive | 2026-03-25 09:00:00 | 2025-03-20 11:30:00 | Hong Kong      |  124000
 30 | diana.costa     | diana.costa@example.com       | designer  | Product          | active   | 2026-04-03 14:30:18 | 2025-04-01 08:00:00 | Brazil         |  118000
 31 | erik.svensson   | erik.svensson@example.com     | developer | Engineering      | active   | 2026-04-03 11:55:40 | 2025-04-15 09:15:00 | Sweden         |  134000
 32 | fiona.o_brien   | fiona.obrien@example.com      | manager   | Engineering      | active   | 2026-04-03 15:45:00 | 2025-05-01 10:30:00 | Ireland        |  157000
 33 | george.muller   | george.muller@example.com     | developer | Infrastructure   | active   | 2026-04-02 16:20:33 | 2025-05-10 08:45:00 | Switzerland    |  148000
 34 | hannah.lee      | hannah.lee@example.com        | analyst   | Product          | active   | 2026-04-03 09:35:22 | 2025-05-20 11:00:00 | South Korea    |  119000
 35 | ivan.petrov     | ivan.petrov@example.com       | devops    | Infrastructure   | inactive | 2026-03-18 10:00:00 | 2025-06-01 09:00:00 | Bulgaria       |  136000
 36 | julia.santos    | julia.santos@example.com      | developer | Data Science     | active   | 2026-04-03 12:10:15 | 2025-06-15 10:30:00 | Brazil         |  129000
 37 | karl.weber      | karl.weber@example.com        | architect | Engineering      | active   | 2026-04-03 14:50:44 | 2025-07-01 08:00:00 | Germany        |  162000
 38 | lisa.nguyen     | lisa.nguyen@example.com       | designer  | Product          | active   | 2026-04-03 10:25:30 | 2025-07-10 09:30:00 | Vietnam        |  117000
 39 | marco.rossi     | marco.rossi@example.com       | developer | Engineering      | active   | 2026-04-02 15:40:18 | 2025-07-20 10:00:00 | Italy          |  133000
 40 | nina.berg       | nina.berg@example.com         | manager   | Data Science     | active   | 2026-04-03 16:05:55 | 2025-08-01 11:15:00 | Norway         |  156000
 41 | omar.hassan     | omar.hassan@example.com       | developer | Infrastructure   | active   | 2026-04-03 11:15:42 | 2025-08-15 08:30:00 | Egypt          |  125000
 42 | priya.sharma    | priya.sharma@example.com      | analyst   | Data Science     | active   | 2026-04-03 13:40:10 | 2025-09-01 09:45:00 | India          |  121000
 43 | rafael.silva    | rafael.silva@example.com      | devops    | Infrastructure   | active   | 2026-04-02 18:00:33 | 2025-09-10 10:00:00 | Portugal       |  137000
 44 | sarah.campbell  | sarah.campbell@example.com    | developer | Engineering      | active   | 2026-04-03 09:50:25 | 2025-09-20 08:15:00 | Scotland       |  130000
 45 | tom.eriksson    | tom.eriksson@example.com      | designer  | Product          | inactive | 2026-03-22 14:00:00 | 2025-10-01 09:00:00 | Sweden         |  116000
 46 | ursula.braun    | ursula.braun@example.com      | manager   | Product          | active   | 2026-04-03 15:10:18 | 2025-10-15 10:30:00 | Germany        |  153000
 47 | vincent.dubois  | vincent.dubois@example.com    | developer | Data Science     | active   | 2026-04-03 12:45:40 | 2025-11-01 11:00:00 | France         |  132000
 48 | wendy.park      | wendy.park@example.com        | analyst   | Product          | active   | 2026-04-03 10:30:55 | 2025-11-10 08:45:00 | South Korea    |  118000
 49 | xander.grey     | xander.grey@example.com       | architect | Infrastructure   | active   | 2026-04-03 14:20:12 | 2025-11-20 09:30:00 | Australia      |  160000
 50 | yara.el_amin    | yara.elamin@example.com       | developer | Engineering      | active   | 2026-04-02 16:55:30 | 2025-12-01 10:00:00 | Lebanon        |  128000
 51 | zach.hoffman    | zach.hoffman@example.com      | devops    | Infrastructure   | active   | 2026-04-03 11:40:22 | 2025-12-15 08:00:00 | United States  |  141000
 52 | anna.kowalski   | anna.kowalski@example.com     | developer | Engineering      | active   | 2026-04-03 13:25:45 | 2026-01-05 09:15:00 | Poland         |  127000
 53 | ben.taylor      | ben.taylor@example.com        | manager   | Engineering      | active   | 2026-04-03 15:55:10 | 2026-01-15 10:30:00 | United Kingdom |  159000
 54 | clara.schmidt   | clara.schmidt@example.com     | analyst   | Data Science     | active   | 2026-04-03 09:05:33 | 2026-01-25 11:00:00 | Germany        |  122000
 55 | derek.o_neal    | derek.oneal@example.com       | developer | Infrastructure   | inactive | 2026-03-30 10:30:00 | 2026-02-01 08:30:00 | United States  |  135000
 56 | elena.volkov    | elena.volkov@example.com      | designer  | Product          | active   | 2026-04-03 14:10:22 | 2026-02-10 09:00:00 | Russia         |  119000
 57 | felix.roth      | felix.roth@example.com        | developer | Data Science     | active   | 2026-04-03 12:00:44 | 2026-02-20 10:15:00 | Switzerland    |  146000
 58 | greta.lindberg  | greta.lindberg@example.com    | devops    | Infrastructure   | active   | 2026-04-02 17:45:15 | 2026-03-01 08:45:00 | Sweden         |  138000
 59 | hugo.martinez   | hugo.martinez@example.com     | analyst   | Product          | active   | 2026-04-03 10:55:30 | 2026-03-10 11:30:00 | Spain          |  120000
 60 | isla.fraser     | isla.fraser@example.com       | developer | Engineering      | active   | 2026-04-03 16:30:18 | 2026-03-20 09:00:00 | Scotland       |  131000
(60 rows)
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

    # Scroll down through rows — header stays pinned
    show_key "j  (scroll down)"
    for i in $(seq 1 20); do
        tmux -L "$SOCK" send-keys -t demo j; sleep 0.25
    done
    sleep 1.5
    clear_key

    # Back to top
    show_key "g  (top)"
    tmux -L "$SOCK" send-keys -t demo g
    sleep 2
    clear_key

    # Scroll right — column-snap with frozen first column
    show_key "→  (scroll right)"
    for i in $(seq 1 4); do
        tmux -L "$SOCK" send-keys -t demo Right; sleep 0.6
    done
    sleep 2
    clear_key

    # Scroll down while scrolled right — header + frozen column stay
    show_key "j  (scroll — header stays)"
    for i in $(seq 1 15); do
        tmux -L "$SOCK" send-keys -t demo j; sleep 0.25
    done
    sleep 2
    clear_key

    # Scroll back left
    show_key "←  (scroll left)"
    for i in $(seq 1 4); do
        tmux -L "$SOCK" send-keys -t demo Left; sleep 0.6
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
