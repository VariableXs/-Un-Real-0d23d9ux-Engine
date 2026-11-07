"""VX11 云端通道助手（paramiko 封装）。

密码经环境变量 VX11_SSH_PASS 提供，不落盘。
用法：
  python vx11_ssh.py run "<command>"
  python vx11_ssh.py sync <dir_with_rs_files>
  python vx11_ssh.py synclist <list_file>  # 每行一个本地 .rs 路径，上传后跑全流程
"""
import os
import sys
import paramiko

HOST = "connect.cqa1.seetacloud.com"
PORT = 27347
USER = "root"
PROBE = "/root/varix-work/probe"
MODS_DIR = PROBE + "/src/mods"
CARGO = "source $HOME/.cargo/env && "

REGEN_LINES = [
    "set -e",
    "SRC=/root/varix-work/probe/src",
    'cd "$SRC"',
    "if [ ! -f main_body.rs ] && [ -f main.rs ]; then",
    "  awk 'f{print} /^];$/{f=1}' main.rs > main_body.rs",
    "  echo main_body.rs derived",
    "fi",
    "MODS=$(ls mods/*.rs | sed 's|mods/||; s|\\.rs$||' | sort)",
    "{",
    "  echo '#![allow(unused)]'",
    "  echo 'extern crate alloc;'",
    "  echo ''",
    "  echo '#[path = \"checks.rs\"]'",
    "  echo 'pub mod checks;'",
    "  echo ''",
    "  for f in $MODS; do",
    "    fn=$(grep -oE 'pub fn run_unx9[0-9]+_checks' mods/$f.rs | head -1 | sed 's/^pub fn //')",
    '    echo "#[path = \"mods/$f.rs\"]"',
    '    echo "pub mod $f;"',
    "  done",
    "  echo ''",
    "  echo 'const ALL: &[(&str, fn() -> checks::CheckSet)] = &['",
    "  for f in $MODS; do",
    "    fn=$(grep -oE 'pub fn run_unx9[0-9]+_checks' mods/$f.rs | head -1 | sed 's/^pub fn //')",
    '    echo "    (\\"$f\\", $f::$fn),"',
    "  done",
    "  echo '];'",
    "  echo ''",
    "  cat main_body.rs",
    "} > main.rs",
    'echo "regen ok: $(ls mods/*.rs | wc -l) modules"',
]
