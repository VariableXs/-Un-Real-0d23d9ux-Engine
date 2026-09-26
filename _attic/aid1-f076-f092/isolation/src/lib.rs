//! AI-D1 隔离舱（#[path] 直挂真实文件——K2 批同款工艺）。
//!
//! 用途：共享 crate（kernel/varix）被其他分队的在途状态挡住编译时，
//! 本舱把 AI-D1 的真实模块树原样挂进来独立编译 + 独立测试。
//! **零拷贝、零分叉**：所有 #[path] 指回仓库内的唯一事实文件，
//! 舱内任何红绿都不是另一份事实，只是同一份事实的独立验证口。
//!
//! v2 收口结构：单点直挂 `deskstar/mod.rs`（域聚合器随舱跑——38 块
//! CheckSet 全量入舱），替代 v1 的平铺模块清单（该清单曾漏挂
//! crumbsbar，酿成 v1 验证覆盖缺口——见账本 #v2-04）。
//!
//! 垫片面（仅两处，均为直挂真实文件而非仿写）：
//! - `crate::checks` → 直挂真实 checks.rs（判定口径同源）；
//! - `crate::star::recenteng` → 直挂真实 recenteng.rs（F077 相对时间
//!   「一处一事实」依赖在舱内同样指向唯一事实文件）。
//!
//! 收口后本舱整体留 `_attic/aid1-f076-f092/` 归档（非功能产物）。

extern crate alloc;

#[path = "../../../../kernel/varix/src/checks.rs"]
pub mod checks;

pub mod star {
    // 内联模块的 path 锚点是 src/star/（多一层），故向上 5 级到仓库根。
    #[path = "../../../../../kernel/varix/src/star/recenteng.rs"]
    pub mod recenteng;
}

// 单点直挂域树：mod.rs 的模块声明相对其自身目录解析——十七项 +
// dbase 全量进舱，域聚合器（含 deep 块）一并可跑。
#[path = "../../../../kernel/varix/src/deskstar/mod.rs"]
pub mod deskstar;
