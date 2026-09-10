//! 内核桌面 Shell 的模块聚合（AI-10 桌面 / AI-11 任务栏）。
//!
//! `taskbar` 与 `startmenu` 由 AI-11 交付，为避免多会话并行改写 `mod.rs`
//! 时互相抹掉声明，这两个模块同时在 `lib.rs` 里用 `#[path]` 直接声明；
//! 这里只做再导出，保证 `shell::taskbar` 与 `crate::taskbar` 指向同一份代码。

pub use crate::startmenu;
pub use crate::taskbar;

pub mod desktop;

pub use desktop::run_shell_checks;
