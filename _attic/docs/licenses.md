# 开源依赖许可清单（B-35 归档，全部零运行时遥测）

| 依赖 | 版本 | 许可 | 用途 |
| --- | --- | --- | --- |
| blake3 | 见 Cargo.lock | Apache-2.0/MIT | 容器内容寻址去重、样本指纹 |
| lz4_flex | 见 Cargo.lock | MIT | chunk 压缩（热） |
| zstd | 1.5.x (sys) | BSD-3 | chunk 压缩（冷） |
| chacha20poly1305 | 0.10.1 | Apache-2.0/MIT | 容器加密 |
| argon2 | 0.5.3 | Apache-2.0/MIT | 口令 KDF |
| git2 (libgit2) | 0.19 | Apache-2.0/MIT | Git 面板只读层 |
| iced-x86 | 1.21 | MIT | 反汇编（只读） |
| ssh-key | 0.6.7 | Apache-2.0/MIT | SSH 金库 |
| tauri / vite / react 等前端底座 | 见 package.json | MIT/Apache-2.0 | 框架 |

> 权威来源：`src-tauri/Cargo.lock` + `package-lock.json`；本表为归档摘要。
> 承诺：不引入任何带运行时遥测/广告的依赖（MASTER-PLAN 第 10 节纪律）。
