__portman_sync_env() {
    eval "$(command portman env bash)"
}

portman() {
    command portman "$@"
    portman_status=$?
    __portman_sync_env
    return $portman_status
}

if [[ -z "$PROMPT_COMMAND" ]]; then
    PROMPT_COMMAND="__portman_sync_env"
elif [[ "$PROMPT_COMMAND" != *"__portman_sync_env"* ]]; then
    PROMPT_COMMAND="__portman_sync_env;$PROMPT_COMMAND"
fi
