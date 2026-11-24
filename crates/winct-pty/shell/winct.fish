# fish 的 shell 集成：在首次提示符前加载启动目录中的 Python 环境，报告命令块和当前目录。
# 使用 OSC 133 和 OSC 7，加载环境时可在用户提示符前添加环境名称。
# winct 通过 `XDG_DATA_DIRS` 注册 `vendor_conf.d` 脚本，启动前按需生成本文件。

# 恢复原有的 `XDG_DATA_DIRS`，子进程不继承 winct 的脚本目录。
if set -q WINCT_XDG_DATA_DIRS
    set -gx XDG_DATA_DIRS $WINCT_XDG_DATA_DIRS
    set -e WINCT_XDG_DATA_DIRS
else
    set -e XDG_DATA_DIRS
end
# 命令行报告附带的随机数，保存在未导出的变量中，供 OSC 133 验证。
set -g __winct_nonce "$WINCT_NONCE"
set -e WINCT_NONCE
# `WINCT_PYTHON=0` 时不加载启动目录中的 Python 环境：用户没有允许这个目录。
set -g __winct_python "$WINCT_PYTHON"
set -e WINCT_PYTHON

status is-interactive; or exit
set -q __winct_hooked; and exit
set -g __winct_hooked 1
# 记录启动目录，用户配置中的 `cd` 不改变项目环境的选择。
set -g __winct_python_root "$PWD"
set -g __winct_python_script (string replace -r '/fish/fish/vendor_conf.d/winct.fish$' '/python.fish' -- (status filename))

function __winct_prompt --on-event fish_prompt
    if set -q __winct_python_script
        set -l script "$__winct_python_script"
        set -e __winct_python_script
        test "$__winct_python" = 0; or source "$script"
        set -e __winct_python
    end
    printf '\e]7;file://%s%s\a\e]133;A;redraw=1;signed=1\a' $hostname (string escape --style=url -- $PWD)
end

function __winct_preexec --on-event fish_preexec
    printf '\e]133;C;nonce=%s;cmdline_url=%s\a' "$__winct_nonce" (string escape --style=url -- $argv[1])
end

function __winct_postexec --on-event fish_postexec
    # 结束标记附带随机数：命令的输出中伪造的结束标记不会被采用。
    printf '\e]133;D;%s;nonce=%s\a' $status "$__winct_nonce"
end
