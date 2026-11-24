# zsh 的启动环境：记录启动目录，恢复用户的 `ZDOTDIR` 并读取用户的 `.zshenv`。
# 交互式 shell 从 `winct.zsh` 安装命令块钩子，后续配置仍按用户的 `ZDOTDIR` 读取。
# winct 通过 `ZDOTDIR` 指向本文件所在目录，启动前按需生成。

builtin typeset -g _winct_dir=${${(%):-%x}:A:h}
# 记录启动目录，用户配置中的 `cd` 不改变项目环境的选择。
builtin typeset -g _winct_python_root=$PWD
# `WINCT_PYTHON=0` 时不加载启动目录中的 Python 环境：用户没有允许这个目录。
builtin typeset -g _winct_python=${WINCT_PYTHON-1}
builtin unset WINCT_PYTHON
# `WINCT_PROMPT=0` 时保留默认提示符，由 `terminal.prompt` 控制。
builtin typeset -g _winct_prompt=${WINCT_PROMPT-1}
builtin unset WINCT_PROMPT
# 命令行报告附带的随机数，保存在未导出的变量中，供 OSC 133 验证。
builtin typeset -g _winct_nonce=${WINCT_NONCE-}
builtin unset WINCT_NONCE
if [[ -n ${WINCT_ZDOTDIR+x} ]]; then
  builtin export ZDOTDIR=$WINCT_ZDOTDIR
  builtin unset WINCT_ZDOTDIR
else
  builtin unset ZDOTDIR
fi

{
  [[ -f ${ZDOTDIR:-$HOME}/.zshenv ]] && builtin source -- "${ZDOTDIR:-$HOME}/.zshenv"
} always {
  [[ -o interactive ]] && builtin source -- "$_winct_dir/winct.zsh"
  builtin unset _winct_dir
}
