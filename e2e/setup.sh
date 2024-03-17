#!/usr/bin/env bash
set -e

brew install caddy fish zsh

# Create an empty Caddyfile because caddy will complain if it is nonexistent or empty
echo "# Empty" > $HOMEBREW_PREFIX/etc/Caddyfile
# Emulate `brew services start caddy`
sudo $(which caddy) start --config $HOMEBREW_PREFIX/etc/Caddyfile

echo "127.0.0.1 portman.localhost" | sudo tee -a /etc/hosts

# Test git associating repos and ports
git worktree add ../portman2 --detach
cd ../portman2
portman create
portman link 4000

cd ../portman
portman create
