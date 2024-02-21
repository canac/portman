// Return the fish shell initialization command
pub fn init_fish() -> &'static str {
    "function __portman_sync_env
    if set lines (command portman get --extended 2> /dev/null)
        set -gx PORT $lines[1]
        set -gx PORTMAN_PROJECT $lines[2]
        if test -n $lines[4]
            set -gx PORTMAN_LINKED_PORT $lines[4]
        else
            set -e PORTMAN_LINKED_PORT
        end
    else
        set -e PORT PORTMAN_PROJECT PORTMAN_LINKED_PORT
    end
end

function portman --wrap portman
    command portman $argv
    set portman_status $status
    __portman_sync_env
    return $portman_status
end

function __portman_prompt_hook --on-event fish_prompt
    __portman_sync_env
    function __portman_cd_hook --on-variable PWD
        __portman_sync_env
    end
end

function __portman_preexec_hook --on-event fish_preexec
    # Without clearing the cd hook, the cd hook and prompt hook would both sync the port
    functions -e __portman_cd_hook
end
"
}
