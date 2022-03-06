// Return the fish shell initialization command
pub fn init_fish() -> &'static str {
    "function __sync_port --on-variable PWD
    if set project (git config --get remote.origin.url | rg '^https://github.com/(.+)/(.+)\\.git$' --only-matching -r '$2')
        set -gx PORT (portman $project)
    else
        set --erase PORT
    end
end
# Manually call __sync_port because it won't be called for the initial setting of PWD
__sync_port"
}
