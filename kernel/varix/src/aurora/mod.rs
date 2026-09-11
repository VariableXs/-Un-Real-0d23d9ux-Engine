//! AURORA-1000（极光）—— 让 Varix 内核成为有界面的完整系统。
//!
//! 本模块收纳 AI-01~AI-15（A001~A375，W1/W2）十五个界面域：
//! 每域一文件、每域 25 项功能（纯逻辑 + 固定容量数组，no_std 无分配），
//! 并各自导出 `run_<域>_checks() -> CheckSet` 参与内核自检闭环。
//!
//! 命名空间说明：顶层已有 VARIX 的 `display.rs` / `input.rs` / `audio.rs`
//! 等既有模块（不得覆盖），故 AURORA 各域统一收口在 `aurora::` 之下。

pub mod display;
pub mod render2d;
pub mod typography;
pub mod gpu;
pub mod compositor;
pub mod image;
pub mod input;
pub mod audio;
pub mod window;
pub mod motion;
pub mod desktop;
pub mod appfw;
pub mod widgets;
pub mod clipboard;
pub mod session;
/// W1 跨域联调集成域（步骤 0257 域收口）。
pub mod integration;
/// W2 联调集成域（步骤 0490~0499）：八条跨域场景 + CheckSet 汇总。
pub mod w2_integration;
/// W4 联调集成域（步骤 0969~0980）：八条跨域场景 + CheckSet 汇总。
pub mod w4_integration;
