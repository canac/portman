// Return the fish shell initialization command
pub fn init_fish() -> &'static str {
    "function __portman_sync_port
    if set port (portman get 2> /dev/null)
        set -gx PORT $port
    else
        set -e PORT
    end
end

function __portman_prompt_hook --on-event fish_prompt
    __portman_sync_port
    function __portman_cd_hook --on-variable PWD
        __portman_sync_port
    end
end

function __portman_preexec_hook --on-event fish_preexec
    # Without clearing the cd hook, the cd hook and prompt hook would both sync the port
    functions -e __portman_cd_hook
end"
}
