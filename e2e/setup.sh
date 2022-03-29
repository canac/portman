#!/usr/bin/env bash

brew install caddy fish

# Create an empty Caddyfile because caddy will complain if it is nonexistent or empty
echo -e "# Empty" > $HOMEBREW_PREFIX/etc/Caddyfile
# Emulate `brew services start caddy`
sudo $(which caddy) start --config $HOMEBREW_PREFIX/etc/Caddyfile

echo "127.0.0.1 portman.localhost" | sudo tee -a /etc/hosts
portman allocate
