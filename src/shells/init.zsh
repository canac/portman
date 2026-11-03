__portman_sync_env() {
    eval "$(command portman env zsh)"
}

portman() {
    command portman "$@"
    portman_status=$?
    __portman_sync_env
    return $portman_status
}

__portman_chpwd() {
    __portman_synced=1
    __portman_sync_env
}

__portman_precmd() {
    if [[ -n $__portman_synced ]]; then
        unset __portman_synced
    else
        __portman_sync_env
    fi
}

__portman_preexec() {
    unset __portman_synced
}

autoload -Uz add-zsh-hook
add-zsh-hook chpwd __portman_chpwd
add-zsh-hook precmd __portman_precmd
add-zsh-hook preexec __portman_preexec
