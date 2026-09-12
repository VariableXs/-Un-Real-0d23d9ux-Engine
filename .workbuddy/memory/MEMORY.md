# 项目长期约定（-Un-Real-0d23d9ux-Engine）

## 仓库结构
- `kernel/`：独立 Cargo workspace（varix 内核，`x86_64-unknown-none`）。
  别名 `cargo kbuild` / `ktest`（用 `--lib`）/`kcheck`；始终带
  `RUSTUP_TOOLCHAIN=1.97.1-x86_64-pc-windows-msvc` 以绕过 rust-toolchain resync。
- `code-analysis/`：独立 workspace（core + app + ui），纯 std 零依赖，与内核无关。
  命令 `cargo test -p ca-core --lib`（当前 261 项全绿）。
- 不要碰 `src/`、`src-tauri/`（launcher/桌面端）。

## 域模块约定（各 AI 共用）
- 一域一文件 + `pub fn run_<domain>_checks() -> CheckSet`（`CheckSet::new("domain")`
  + `set.add("Fxxx …", cond, "detail")`），单测写在模块内 `#[cfg(test)] mod tests`。
- **禁止「末态读取」**：`set.add(name, a && obj.field == X)` 里字段读发生在表达式末尾
  = 末态。凡「操作→读状态→再操作→断言」，中间读数必须先存进独立 `let`。
- 登记类接口（register/record/add）必须自带去重，否则计数断言被静默改变。
- 调试：临时加 `#[cfg(test)] mod tmp_dbg` 拆子条件，**验完立刻删**。
- 自检失败时让 panic 输出 `CheckSet::render()`，否则定位不到具体项。

## Code Analysis（code-analysis/）
- AI-06 落点 #411~#460：`cmd` / `manual` / `undo` / `multisrc` / `marquee` / `link`，
  聚合 `run_ai06_checks()`（50 项），已纳入 `run_all_checks()`。
- 任务文档有**四处副本**（仓库根 `docs/`、`code-analysis/docs/`、C² 根、
  C² `code-analysis/docs/`）：分工完成图 / 功能全景图 / 实施总步骤图 / Code Analysis2 /
  Code Analysis3。改完必须四处同步并用 md5（字节级）校验一致。
- C² `code-analysis/` 是仓库 `code-analysis/` 的完整项目副本（含 core/app/ui/docs，
  排除 target/.git）；仓库为源，用 mtime 增量同步过去。
- `lib.rs` 的 `pub mod` 列表与 `run_*_checks` 汇总极易被并行会话覆盖；提交前复查。

## Git 并发纪律（血泪）
- **绝对禁止 `git pull --rebase --autostash`**（曾删光 `.git/objects/pack` + refs）。
  远端领先时：`git fetch origin main` + `git merge origin/main`。
- 并行会话常造出「内容等价、SHA 不同」双份提交，merge 冲突多落在共享 md；
  用 `git checkout --theirs` + 脚本重放自己的改动（保留原文件行尾）。
- 提交前 `git log --oneline -1` + `git status --porcelain`，只挑自己域路径
  （别人的 `git add .` 会卷走你的未提交文件）。
- 推送：`git -c credential.helper= -c credential.helper=wincred push origin main`
  （`~/.gitconfig` 里空 helper 清空了助手链；凭据在 wincred / VariableXs）。
  本地代理对 github.com:443 间歇 502/连接重置，**循环重试 4~8 次**；
  api.github.com 始终通（别被它误导），必要时走 Git Data API 推送。
- 仓库有 `.gitattributes`（`* text=auto eol=lf`），md 存 LF、工作区可 CRLF；
  只差行尾时 `git add` 刷新索引即可（staged diff 为空）。

## 环境 / 工具链
- QEMU 11.1.0 已装：构建 → `scripts/make-iso.sh`（**必须 xorriso**）→
  `qemu-system-x86_64 -cdrom varix.iso -serial file:X.log -monitor tcp:127.0.0.1:55xx,server,nowait
  -no-reboot -no-shutdown -m 512M -M q35 -display none`，**必须 run_in_background**。
  抓状态 `python tools/qmon.py <port> …`；异常定位靠串口末行 + RIP/CR2 + ELF 符号表。
- rustup 工具链曾整链损坏：uninstall + minimal 重装 rustc/std/rust-src，cargo 与
  none-std 需从 static.rust-lang.org 手工解包 + 手写 rustlib manifest。
- Limine：`limine-<ver>.tar.gz` 是源码包，二进制要 `limine-binary.tar.gz`，
  且解包**不加 `--strip-components=1`**（首层目录就是 `limine-binary/`）。

## CI
- `.github/workflows/ci.yml`：frontend / backend / kernel / boot（含云端 QEMU 冒烟）。
- YAML step name 含裸冒号会让整条 workflow startup_failure；name 一律加引号。
- 根目录 html 是 vite MPA 入口，清理前对照 vite.config。

## Rust 实现坑
- `const fn` 不能用 `Ord::min`（E0658）→ 手写 `if x > n { n } else { x }`。
- 非 Copy 数组重复初始化用 `[const { None }; N]`（inline const block）。
- `Result<&mut T, E>` 无 `PartialEq`/`Debug` → 用 `is_err()` 或先拷成值。
- fuzz 必须偏置到有效号段（纯随机几乎永远走 ENOSYS，"零 panic" 是空证明）。
- 删"未用导入"要连带查 `#[cfg(test)] mod tests`，否则 E0425。
- 内层 crate 位于别的工作区目录树内时要自带空 `[workspace]` 表。
