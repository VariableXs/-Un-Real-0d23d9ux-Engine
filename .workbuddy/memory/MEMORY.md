# 项目长期约定（VARIX-500 / Varix 内核）

## 仓库结构
- 内核独立 Cargo workspace：`kernel/`（成员 `varix`），target `x86_64-unknown-none`。
- 别名：`cargo kbuild`（内核 ELF release）、`cargo ktest`（主机单测，std）、`cargo kcheck`。
- 不要碰 `src/`、`src-tauri/`（那是 launcher/桌面端，不是内核）。

## 域模块实现约定（AI-01~AI-20 共用）
- 一域一文件：`kernel/varix/src/<domain>.rs`（如 `power.rs`、`robust.rs`）。
- 每域 25 项功能，按 `// Fxxx 名称` 注释分区，纯逻辑 + 固定容量数组（无 Vec/String/Box）。
- 每个模块必须导出 `pub fn run_<domain>_checks() -> CheckSet`（checks.rs 里的自检框架），
  用 `CheckSet::new("domain")` + `set.add("Fxxx ...", cond, "detail")` 逐项断言。
- 单测写在模块内 `#[cfg(test)] mod tests`，命名 `f<编号>_<要点>`；自检项 ≥25。
- 内核自检闭环：`robust::run_kernel_checkup()` 汇总各域 CheckSet
  （robust 内部聚合时排除自身，避免递归）。

## 共享文件与并行纪律
- 多个会话并行改同一仓库：编辑 `lib.rs`、`docs/VARIX-500-*.md` 前先 `git log` / 重读文件，
  只改自己域的段落（`## AI-xx ·` 到下一个 `## AI-x` 之间）。
- 提交信息格式：`varix500(ai-XX): Fxxx <功能名>`，一功能一提交或一域一提交。
- `lib.rs` 的 `pub mod` 列表被多方追加；提交时若只提交自己的模块会导致 HEAD 缺少
  他域的模块文件（构建失败），必要时连同工作区里他域新增的 `src/*.rs` 一起纳入。

## 模块注册坑（TRINITY-500 多域同目录）
- `ui.rs` 是单文件模块（AI-16，F376~F400）；同名的 `ui/` 子目录（AI-07 组件库、
  AI-08 动效）必须在 `ui.rs` 顶部显式 `pub mod widgets/tokens/motion;` 才会被编译。
- `shell/mod.rs` 由 AI-11 维护（只再导出 taskbar/startmenu），AI-10 的 `desktop`
  需另行 `pub mod desktop;`；`lib.rs` 的 `pub mod shell;` / `pub mod vwm;`
  极易被并行会话覆盖丢失，提交前必须复查。
- 域自检失败时让自测 panic 出 `CheckSet::render` 的内容，否则只能看到
  "domain self-test must pass"，定位不到具体 F 项。

## 环境坑
- **无 QEMU**：真机/模拟验收一律标注"待环境具备"。
- **禁止 `git pull --rebase --autostash`**：曾把 `.git/objects/pack/*.pack` 与
  refs 整体删掉，本地历史全丢（工作区文件无事）。要对齐远端用
  `git fetch origin +refs/heads/main:refs/remotes/origin/main` +
  `git reset --mixed <remote sha>`（保工作区）。
  注：整仓 `git fetch` 会因 arena/* 分支仅有大小写差异而在 Windows 上失败。
- **GitHub 推送（2026-09-11 已打通）**：凭据在 Windows 凭据管理器（wincred，
  用户 VariableXs），但 `~/.gitconfig` 里 `credential.helper=`（空值）把助手链清空了，
  普通 `git push` 拿不到凭据。可用命令（不改全局配置）：
  `git -c credential.helper= -c credential.helper=wincred push origin main`。
  本地代理（127.0.0.1）对 github.com 的 CONNECT 间歇性 502 / abrupt close，
  需循环重试 2~6 次（api.github.com 始终正常，别被它误导）。
- 运行 canonical 命令后确认：`cargo ktest`（应全绿）、`cargo kbuild`（应零告警）。

## Git 并发纪律（血泪）

- **绝对禁止 `git pull --rebase --autostash`**：2026-09-11 多会话并行时它把
  `.git/objects/pack/*.pack` 全删（只剩 .idx）并弄丢 `refs/heads/main`，
  本地对象库整体报废。改用：`git fetch origin main` → 用显式 SHA 做
  `git update-ref` / `git reset --mixed`；需要暂存就先 `git stash`。
- 多会话会把别人的未提交文件一起 `git add .` 提交；提交前务必
  `git status --porcelain | grep -E '^(A|M) '` 只挑自己域的路径。
- 恢复对象库只能靠 `git fetch`（代理 127.0.0.1:55799 间歇 502，需重试 5~10 次）。

- **rustup 工具链易损坏（2026-09-12）**：stable/1.97.1 曾整链 Missing manifest。修法：uninstall 后 minimal profile 重装 rustc/std/rust-src；cargo 与 none-std 用 Python 从 static.rust-lang.org dist 手工解包（归档内层还有同名目录要拍平）+ 手写 rustlib manifest/components。跑内核命令始终带 RUSTUP_TOOLCHAIN=1.97.1-x86_64-pc-windows-msvc 绕过 rust-toolchain.toml 的 resync（resync 会回滚删光工具链）。ktest 全量会因 bin varix（msvc target）unwinding panic 失败，用 cargo ktest --lib。
