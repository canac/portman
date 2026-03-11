__portman_sync_env() {
    unset PORT PORTMAN_PROJECT PORTMAN_ORIGIN PORTMAN_LINKED_PORT
    lines=$(command portman get --extended 2> /dev/null || true)
    if [[ -n "$lines" ]]; then
        PORT=$(echo "$lines" | awk 'NR==1')
        PORTMAN_PROJECT=$(echo "$lines" | awk 'NR==2')
        PORTMAN_ORIGIN="https://$PORTMAN_PROJECT.localhost"
        PORTMAN_LINKED_PORT=$(echo "$lines" | awk 'NR==4')
        export PORT PORTMAN_PROJECT PORTMAN_ORIGIN
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

autoload -Uz add-zsh-hook
add-zsh-hook chpwd __portman_sync_env
add-zsh-hook precmd __portman_sync_env
