#!/usr/bin/env zsh
set -ex

eval "$(portman init zsh)"
# Test shell integration setting port
test -n "$PORT"
test 4000 = "$PORTMAN_LINKED_PORT"
test portman = "$PORTMAN_PROJECT"
MESSAGE=portman python3 e2e/echo_server.py &
server_pid=$!
sleep 1
# Test reverse proxy
curl --silent --show-error https://portman.localhost | grep portman
# Test linked port proxy
curl --silent --show-error http://localhost:4000 | grep portman

# Test shell integration changing port
cd ../portman2
test -n "$PORT"
test -z "$PORTMAN_LINKED_PORT"
test portman2 = "$PORTMAN_PROJECT"
MESSAGE=portman2 python3 e2e/echo_server.py &
server2_pid=$!
sleep 1
curl --silent --show-error https://portman2.localhost | grep portman2

# Test shell integration clearing port
cd ..
test -z "$PORT"
test -z "$PORTMAN_LINKED_PORT"
test -z "$PORTMAN_PROJECT"

kill $server_pid $server2_pid
