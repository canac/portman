#!/usr/bin/env fish

portman init fish | source
# Test shell integration setting port
test -n $PORT || return 1
test 4000 = $PORTMAN_LINKED_PORT || return 1
test portman = $PORTMAN_PROJECT || return 1
MESSAGE=portman python3 e2e/echo_server.py &
set server_pid $last_pid
sleep 1
# Test reverse proxy
curl --silent --show-error https://portman.localhost | grep portman || return 1
# Test linked port proxy
curl --silent --show-error http://localhost:4000 | grep portman || return 1

# Test shell integration changing port
cd ../portman2
test -n $PORT || return 1
test -z $PORTMAN_LINKED_PORT || return 1
test portman2 = $PORTMAN_PROJECT || return 1
MESSAGE=portman2 python3 e2e/echo_server.py &
set server2_pid $last_pid
sleep 1
curl --silent --show-error https://portman2.localhost | grep portman2 || return 1

# Test shell integration clearing port
cd ..
test -z $PORT || return 1
test -z $PORTMAN_LINKED_PORT || return 1
test -z $PORTMAN_PROJECT || return 1

kill $server_pid $server2_pid
