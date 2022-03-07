// Return the fish shell initialization command
pub fn init_fish() -> &'static str {
    "function __portman_sync_port --on-variable PWD
    if portman sync check > /dev/null && set project (git config --get remote.origin.url | rg '^https://github.com/(.+)/(.+)\\.git$' --only-matching -r '$2')
        set -gx PORT (portman get $project)
    else
        set --erase PORT
    end
end
# Manually call __portman_sync_port because it won't be called for the initial setting of PWD
__portman_sync_port"
}
