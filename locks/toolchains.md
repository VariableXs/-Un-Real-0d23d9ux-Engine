# 工具链版本锁（B-21）

| 工具 | 版本 | 来源 |
| --- | --- | --- |
| python | 3.12.7 embeddable amd64 | https://www.python.org/ftp/python/3.12.7/python-3.12.7-embed-amd64.zip |
| go | 1.23.2 windows-amd64 | https://go.dev/dl/go1.23.2.windows-amd64.zip |
| rust | stable via rustup-init（static.rust-lang.org dist x86_64-pc-windows-msvc） | CARGO_HOME/RUSTUP_HOME 全在容器 |

> 升版纪律：改 toolchains.rs 常量 + 本文件同提交；下载经 netconsent 授权口径。
