// Return the fish shell initialization command
pub fn init_fish() -> &'static str {
    "function __portman_sync_port --on-variable PWD
    if set port (portman get 2> /dev/null)
        set -gx PORT $port
    else
        set --erase PORT
    end
end
# Manually call __portman_sync_port because it won't be called for the initial setting of PWD
__portman_sync_port"
}
