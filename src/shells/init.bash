__portman_sync_env() {
    unset PORT PORTMAN_PROJECT PORTMAN_LINKED_PORT
    lines=$(command portman get --extended 2> /dev/null || true)
    if [[ -n "$lines" ]]; then
        PORT=$(echo "$lines" | awk 'NR==1')
        PORTMAN_PROJECT=$(echo "$lines" | awk 'NR==2')
        PORTMAN_LINKED_PORT=$(echo "$lines" | awk 'NR==4')
        export PORT PORTMAN_PROJECT
        if [[ -n "$PORTMAN_LINKED_PORT" ]]; then
            export PORTMAN_LINKED_PORT
        fi
    fi
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

__portman_sync_env
