# GIT 纪律（步骤 0029）

> 本仓库多会话并行施工，历史教训必须固化为纪律。

## 提交
- 提交信息：`aurora1000(ai-XX): <范围> <摘要>`；一域一提交或一功能一提交。
- 提交前 `git status --porcelain` 只挑自己域的路径，**禁止盲目 `git add .`**。
- 提交门禁：`cargo ktest` 全绿 + `cargo kbuild` 零告警。

## 对齐远端（危险区）
- **禁止 `git pull --rebase --autostash`**：曾整库删光 pack 对象与 refs。
- 正确姿势：`git fetch origin main` → 用显式 SHA `git reset --mixed <sha>`（保工作区）。
- 整仓 fetch 会因 arena/* 分支大小写差异在 Windows 失败，只 fetch 需要的 ref：
  `git fetch origin +refs/heads/main:refs/remotes/origin/main`。

## 推送（Windows 环境）
- `~/.gitconfig` 中 `credential.helper=`（空值）会清空助手链，用：
  `git -c credential.helper= -c credential.helper=wincred push origin main`。
- 本地代理对 github.com 间歇 502/abrupt close：循环重试 2~6 次。
- api.github.com 始终正常，不要被它误导认为代理无问题。

## 共享文件
- `kernel/varix/src/lib.rs`、`docs/AURORA-1000*.md` 为共享热点：编辑前先 `git log` + 重读，
  只改自己域的段落；提交前复查 `pub mod` 列表未被并行会话覆盖丢失。
