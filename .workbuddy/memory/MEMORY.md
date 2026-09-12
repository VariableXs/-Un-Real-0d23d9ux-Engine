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

### CheckSet 断言铁律：禁止"末态读取"（2026-09-12 反复踩坑）
- `set.add("…", cond1 && obj.field == X && …)` 里对**可变对象**的字段读取，
  全部发生在**表达式求值末尾**，即读到的是**末态**——中间过程的读数会被末态覆盖。
- 凡"操作 → 读状态 → 再操作 → 断言"的写法，必须把每个中间读数先存进独立的
  `let` 变量，再用变量参与布尔表达式。F158/F159/F164/F165/F183/F185/F186/F190/F191
  都因这条挂过。
- 调试手法：临时加 `#[cfg(test)] mod tmp_dbg`，把复合断言拆成逐条
  `assert!(…, "tag")`，一次定位到具体子条件；**验证完立刻删掉该模块**。
- 登记类接口（`register`/`record`/`add`）必须自带**去重**，否则重复登记会静默
  改变计数断言。

### 隔离验证法：绕开并行会话的编译错误
- 当并行会话把别处源码改坏（crate 整体编不过）导致无法跑自己的模块时：
  把 `checks.rs` + 待验模块复制到临时 scratch crate（`/tmp/bcverify`，
  `Cargo.toml` 里 `[profile.test] panic = "unwind"`，自带一个 `gfxsrv::rgb` shim），
  在那里单独 `cargo test --lib`。可完全绕开他人的编译错误。

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
  （2026-09-12 已装 QEMU 11.1.0，见下方「QEMU 真机验收」小节。）

## QEMU 真机验收（2026-09-12 打通，内核可完整引导至 boot complete）
- 命令链：
  - 构建：`cd kernel && RUSTUP_TOOLCHAIN=1.97.1-x86_64-pc-windows-msvc cargo kbuild`
  - 打 ISO：`PATH="/tmp/xorr/root/usr/bin:$PATH" bash scripts/make-iso.sh`
    （**必须 xorriso**；tools/build-iso.py 的 pycdlib 产物 Limine 读不了大文件。
    xorriso 便携版在 /tmp/xorr，重启会话可能丢失，需重新解包 msys2 包。）
  - 运行：`qemu-system-x86_64 -cdrom varix.iso -serial file:serialX.log
    -monitor tcp:127.0.0.1:55xx,server,nowait -no-reboot -no-shutdown -m 512M -M q35 -display none`
    **必须用 run_in_background=true**，否则 Bash 工具调用结束会杀掉 QEMU。
- 抓状态：`python tools/qmon.py <port> "info registers" "xp /96xg 0x…"`
  （自带剥 readline 回显 + ANSI 转义的逻辑；monitor 的 `xp` 只能看已映射地址）。
- 定位流程：串口最后一行 → `info status` → RIP/CR2 → 用 ELF 符号表把 RIP 映射回函数
  （Python 解析 ELF section/symtab，见当日日志）→ 反汇编确认。
- 致命异常现在会打印 `err/rip/cr2`（`cpu/idt.rs::isr_dispatch`），不要再只依赖
  "fatal exception 14" 这种无地址信息。
- 中断入口段约束：stub 表必须放 `linker.ld` 的页对齐 `.stubs` 段（Limine 把 `.data`
  映射为 NX），生成完代码后用 `harden_stub_mapping()`（CR3+HHDM 走页表）清 NX/清写位。
  任何"运行时生成代码"的新需求都要走这条路径，不能塞进 `.data`。

## CI
- `.github/workflows/ci.yml` 三个 job：frontend / backend / kernel（kcheck、ktest、
  fuzz、kbuild + ELF 产物）。
- **当前 GitHub 不为该仓库调度 Actions job**（run #100 起全是 0 job / 0 秒 / failure，
  workflow 状态却是 active），属账号层面（额度/账单/被禁用），与代码无关。
  `gh` CLI 未登录，只能靠未认证 REST API 查 run 元信息。
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

## Rust 实现坑（VARIABLE-200 实测，2026-09-12）
- **`const fn` 里不能用 `Ord::min`**：`1u64 << kind.min(63)` 报 E0658「`Ord` is not yet
  stable as a const trait」。手写 `if kind > 63 { 63 } else { kind }`。
- **非 `Copy` 类型的数组重复初始化**：`[None; N]` 要求 `Option<T>: Copy`；`T` 带
  大缓冲/非 Copy 时必须写 `[const { None }; N]`（inline const block）。
- **`Result<&mut T, E>` 不能 `assert_eq!(…, Err(e))`**：`&mut T` 无 `PartialEq`/`Debug`；
  连带 `Option<Result<(), E>>` 的 `==` 也不成立。用 `is_err()`，或先把字段拷成值再比。
- **自检探针的"末态读取"**：`set.add(name, a && obj.field == X, …)` 里对字段的读取发生在
  表达式末尾 = 读到末态。任何"操作→读状态→再操作"都要先把中间读数存进独立 `let`。
  同理：会被后续操作覆写的缓冲（如 partial copy 的落点）不能与"待断言缓冲"共用。
- **fuzz 必须偏置到有效号段**：纯 `u64` 随机数落到内核号段（如 `0x4000-0x40FF`）的概率
  约 `256/2^64 ≈ 0`，于是每轮都进 `ENOSYS`、处理体一次都没跑到，"零 panic"是空证明。
  正确配比：1/4 完全野生 + 1/4 原生带 + 1/4 兼容带 + 1/4 低位全谱。
- **清"未用导入"要连带查 `#[cfg(test)] mod tests`**：从模块顶部删掉常量后测试里还在用
  → E0425。正确做法是把该导入下沉进 `mod tests`。
- **内层 crate 要自带空 `[workspace]`**：`kernel/userspace/<crate>` 位于 `kernel/` 的
  workspace 目录树内但不是成员 → 「believes it's in a workspace when it's not」。
  加一个空 `[workspace]` 表自成一棵，并在 `.gitignore` 加 `kernel/userspace/*/target/`。
- **提交前先判断改动是否已在 HEAD**：并行会话的 `git add .` 会把你的源文件卷进它的提交
  （2026-09-12 的 61a135c 就卷走了 AI-01/02/03 全部源文件）。先 `git log --oneline -1`
  + `git status --porcelain`，再决定提交哪些路径，避免重复提交或漏提交。

## Git 并发纪律（血泪）

- **绝对禁止 `git pull --rebase --autostash`**：2026-09-11 多会话并行时它把
  `.git/objects/pack/*.pack` 全删（只剩 .idx）并弄丢 `refs/heads/main`，
  本地对象库整体报废。改用：`git fetch origin main` → 用显式 SHA 做
  `git update-ref` / `git reset --mixed`；需要暂存就先 `git stash`。
- 多会话会把别人的未提交文件一起 `git add .` 提交；提交前务必
  `git status --porcelain | grep -E '^(A|M) '` 只挑自己域的路径。
- 恢复对象库只能靠 `git fetch`（代理 127.0.0.1:55799 间歇 502，需重试 5~10 次）。

- **rustup 工具链易损坏（2026-09-12）**：stable/1.97.1 曾整链 Missing manifest。修法：uninstall 后 minimal profile 重装 rustc/std/rust-src；cargo 与 none-std 用 Python 从 static.rust-lang.org dist 手工解包（归档内层还有同名目录要拍平）+ 手写 rustlib manifest/components。跑内核命令始终带 RUSTUP_TOOLCHAIN=1.97.1-x86_64-pc-windows-msvc 绕过 rust-toolchain.toml 的 resync（resync 会回滚删光工具链）。ktest 全量会因 bin varix（msvc target）unwinding panic 失败，用 cargo ktest --lib。

## Limine 发布包选取（CI/ISO 打包必读）

- **`limine-<ver>.tar.gz` 是源码包，不是二进制包**：v12.1.0 包内只有
  `limine-12.1.0/{configure,host/limine.c,...}`，**没有任何预编译 `.bin`**。
  要 `limine-bios-cd.bin` / `limine-uefi-cd.bin` / `limine-bios.sys` 必须下
  `limine-binary.tar.gz`（同名 tag 下有，v12.1.0 与 v12.9.0 都 200）。
- **`limine-binary.tar.gz` 的首层目录就是 `limine-binary/`**（本地 `tools/limine/limine-binary.zip` 同构）。
  因此解包时 **不要加 `--strip-components=1`**，否则文件会落到 `tools/limine/*.bin`，
  而 `scripts/make-iso.sh` 读的是 `tools/limine/limine-binary/*.bin` → `set -e` 下 B5 直接失败。
  正确组合：URL 用 `limine-binary.tar.gz` + `tar -xzf … -C tools/limine`（不 strip）。
- **CI 的 `curl A || curl B` 只在 A 失败时才走 B**：A 是源码包但 HTTP 200，
  所以回退永远不触发 → 失败被拖到 B5 才暴露。下载后必须在 B1 加
  `test -f tools/limine/limine-binary/limine-bios-cd.bin || exit 1` 硬门禁。
- `tools/limine/` 在 `.gitignore` 里，CI 干净检出时不存在，全靠这一步下载。

## CI 已真实启用（2026-09-12）
- run 全绿常态：frontend/backend/kernel/boot 四 job，含云端 QEMU 冒烟引导（boot complete 门禁）。
- 坑：YAML step name 含裸冒号会让整条 workflow startup_failure（0 jobs）；name 一律加引号。
- 推送通道照旧走 api.github.com（Git Data API），脚本必须 core.quotepath=false。
- 根目录 html 是 vite MPA 入口，清理前必须对照 vite.config。
