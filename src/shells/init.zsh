__portman_sync_env() {
    eval "$(command portman env zsh)"
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
