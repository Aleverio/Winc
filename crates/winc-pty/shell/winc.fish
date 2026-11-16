# winc 的 shell 集成（docs/TERMINAL.md §6.2）。winc 把这个文件所在的目录加到 XDG_DATA_DIRS 的最前面，
# fish 启动时读 vendor_conf.d 里的它。只发 OSC 133（提示符、输出、结束）和 OSC 7（当前目录），
# 不改提示符的样子，不改 shell 的行为。这个文件由 winc 生成，改了会被覆盖。

# 先把 XDG_DATA_DIRS 恢复成原来的，fish 启动的程序看不到 winc 的目录。
if set -q WINC_XDG_DATA_DIRS
    set -gx XDG_DATA_DIRS $WINC_XDG_DATA_DIRS
    set -e WINC_XDG_DATA_DIRS
else
    set -e XDG_DATA_DIRS
end
# 报命令行时带上的随机数（docs/TERMINAL.md §6.3）：放在不导出的变量里，命令看不到。
set -g __winc_nonce "$WINC_NONCE"
set -e WINC_NONCE

status is-interactive; or exit
set -q __winc_hooked; and exit
set -g __winc_hooked 1
# 记录启动目录；用户配置中的 cd 不改变项目环境的选择。
set -g __winc_python_root "$PWD"
set -g __winc_python_script (string replace -r '/fish/fish/vendor_conf.d/winc.fish$' '/python.fish' -- (status filename))

function __winc_prompt --on-event fish_prompt
    if set -q __winc_python_script
        set -l script "$__winc_python_script"
        set -e __winc_python_script
        source "$script"
    end
    printf '\e]7;file://%s%s\a\e]133;A;redraw=1\a' $hostname (string escape --style=url -- $PWD)
end

function __winc_preexec --on-event fish_preexec
    printf '\e]133;C;nonce=%s;cmdline_url=%s\a' "$__winc_nonce" (string escape --style=url -- $argv[1])
end

function __winc_postexec --on-event fish_postexec
    printf '\e]133;D;%s\a' $status
end
