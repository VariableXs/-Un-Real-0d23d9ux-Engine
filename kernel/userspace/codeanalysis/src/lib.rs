//! AI-09 · 壳C VARIX 用户态骨架（C13~C15）。
//!
//! 一个 core，三个壳——本 crate 是「壳C：VARIX 内核用户态」的落点：
//!
//! - **C13**（[`manifest`]）：向内核 appfw/appmgr 登记的应用描述符。
//!   能力位声明 `fs.read` / `fs.write`，折算成内核 `PERM_FS`（与
//!   `varix/src/aurora/appfw.rs` 的权限位镜像对齐，漂移即注册被拒）。
//! - **C14**（[`vfsbridge`]）：写回通道的内核实现——壳A/壳B 走磁盘直写，
//!   壳C 把同一份相对路径写请求翻译成 VFS `open/read/write/close`。
//!   路径防逃逸规则与 `ca-core::shell::WriteChannel::safe_rel` 同源。
//! - **C15**（[`eventloop`]）：内核输入子系统键位事件消费循环。
//!   同一份键位配置（ca-core keymap），注册目标从「系统级/vwm 转发」
//!   换成「内核事件端口订阅」，处理逻辑三壳等价。
//!
//! ## 宿主可测
//!
//! 全部纯逻辑（能力折算、路径防逃逸、事件分发）在宿主 `cargo test` 直接跑；
//! 触 syscall 的实现收在 `target_os = "none"` 门后，宿主用注入桩等价驱动。
//!
//! ## 挂账
//!
//! `codeanalysis-entry`（entry.elf）的内核联调待 VARIX 运行时就绪，
//! 对应 C24 等价清单中的 🔶 行。

pub mod eventloop;
pub mod manifest;
pub mod vfsbridge;
