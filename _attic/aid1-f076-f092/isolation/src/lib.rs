//! AI-D1 隔离舱（#[path] 直挂真实文件——K2 批同款工艺）。
//!
//! 用途：共享 crate（kernel/varix）被其他分队的在途状态挡住编译时
//! （本次：h1base 缺文件、selfheal2::detect 缺方法——均非 D1 任务面），
//! 本舱把 AI-D1 的真实模块文件原样挂进来独立编译 + 独立测试。
//! **零拷贝、零分叉**：所有 #[path] 指回仓库内的唯一事实文件，
//! 舱内任何红绿都不是另一份事实，只是同一份事实的独立验证口。
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

pub mod deskstar {
    #[path = "../../../../../kernel/varix/src/deskstar/dbase.rs"]
    pub mod dbase;
    #[path = "../../../../../kernel/varix/src/deskstar/quickset.rs"]
    pub mod quickset;
    #[path = "../../../../../kernel/varix/src/deskstar/notifctr.rs"]
    pub mod notifctr;
    #[path = "../../../../../kernel/varix/src/deskstar/alttab.rs"]
    pub mod alttab;
    #[path = "../../../../../kernel/varix/src/deskstar/calflyout.rs"]
    pub mod calflyout;
    // v1 批漏挂（v2 回炉补挂——crumbsbar 的 15 项测试当时未进隔离舱）。
    #[path = "../../../../../kernel/varix/src/deskstar/crumbsbar.rs"]
    pub mod crumbsbar;
    #[path = "../../../../../kernel/varix/src/deskstar/copydlg.rs"]
    pub mod copydlg;
    #[path = "../../../../../kernel/varix/src/deskstar/conflict.rs"]
    pub mod conflict;
    #[path = "../../../../../kernel/varix/src/deskstar/deskrefresh.rs"]
    pub mod deskrefresh;
    #[path = "../../../../../kernel/varix/src/deskstar/detailpane.rs"]
    pub mod detailpane;
    #[path = "../../../../../kernel/varix/src/deskstar/icongrid.rs"]
    pub mod icongrid;
    #[path = "../../../../../kernel/varix/src/deskstar/livesearch.rs"]
    pub mod livesearch;
    #[path = "../../../../../kernel/varix/src/deskstar/snapwin.rs"]
    pub mod snapwin;
    #[path = "../../../../../kernel/varix/src/deskstar/sndfx.rs"]
    pub mod sndfx;
    #[path = "../../../../../kernel/varix/src/deskstar/tabexplorer.rs"]
    pub mod tabexplorer;
    #[path = "../../../../../kernel/varix/src/deskstar/taskview.rs"]
    pub mod taskview;
    #[path = "../../../../../kernel/varix/src/deskstar/trashui.rs"]
    pub mod trashui;
    #[path = "../../../../../kernel/varix/src/deskstar/zipkit.rs"]
    pub mod zipkit;
}

