#!/usr/bin/env fish

portman init fish | source
set --erase PORT # Make sure that $PORT is not being set by the init script
cd .
python3 e2e/hello_world_server.py &
set server_pid $last_pid
curl --silent --show-error https://portman.localhost | grep "Hello, world"
kill $server_pid
