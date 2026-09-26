//! AI-H3 隔离舱（#[path] 直挂真实文件——K2/D1 批同款工艺）。
//!
//! 用途：共享 crate（kernel/varix）被其他分队的在途状态挡住编译时
//! （本次：deskstar/svstar/uni1 等他队模块 201 项在途编译错——均非
//! H3 任务面），本舱把 AI-H3 的真实模块文件原样挂进来独立编译 +
//! 独立测试。**零拷贝、零分叉**：所有 #[path] 指回仓库内的唯一事实
//! 文件，舱内任何红绿都不是另一份事实，只是同一份事实的独立验证口。
//!
//! 垫片面（仅一处，直挂真实文件而非仿写）：
//! - `crate::checks` → 直挂真实 checks.rs（判定口径同源）；
//! - `crate::h3star` → 直挂真实 h3star/mod.rs（兄弟模块按 mod.rs 所在
//!   目录自动解析，五十项 + hbase 全量入舱）。
//!
//! 收口后本舱整体留 `_attic/aih3-f301-f350/` 归档（非功能产物）。

extern crate alloc;

#[path = "../../../../kernel/varix/src/checks.rs"]
pub mod checks;

#[path = "../../../../kernel/varix/src/h3star/mod.rs"]
pub mod h3star;
