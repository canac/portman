#!/usr/bin/env bash
set -ex

eval "$(portman init bash)"
$PROMPT_COMMAND

# Test shell integration setting port
cd src # Test inside a subdirectory
test -n "$PORT"
test 4000 = "$PORTMAN_LINKED_PORT"
test portman = "$PORTMAN_PROJECT"
cd .. # Test inside the project directory
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
$PROMPT_COMMAND
test -n "$PORT"
test -z "$PORTMAN_LINKED_PORT"
test portman2 = "$PORTMAN_PROJECT"
MESSAGE=portman2 python3 e2e/echo_server.py &
server2_pid=$!
sleep 1
curl --silent --show-error https://portman2.localhost | grep portman2

# Test shell integration clearing port
cd ..
$PROMPT_COMMAND
test -z "$PORT"
test -z "$PORTMAN_LINKED_PORT"
test -z "$PORTMAN_PROJECT"

kill $server_pid $server2_pid
