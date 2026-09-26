//! compatstar — Varix STAR I start · A 应用兼容域深化（F001~F020 · AI-C1 分工包）。
//!
//! 本目录是《Varix STAR I start.md》主册 A-5 深化设计报告（G-A-01 ~ G-A-20）
//! 的判据实装层。二十项各占一个子模块，一项一事实：
//!
//! | 项 | 判据锚 | 子模块 |
//! | --- | --- | --- |
//! | F001 无感双击运行 | G-A-01 | [`dblrun`] |
//! | F002 静态 PE 全量支持 | G-A-02 | [`peblend`] |
//! | F003 导入绑定加速 | G-A-03 | [`pebind`] |
//! | F004 Wow64 门 | G-A-04 | [`wow64`] |
//! | F005 Win32 窗口管理兼容层 | G-A-05 | [`winmgr`] |
//! | F006 GDI 绘图面 | G-A-06 | [`gdiface`] |
//! | F007 GDI+ 与双缓冲 | G-A-07 | [`gdiplus`] |
//! | F008 通用对话框族 | G-A-08 | [`comdlg`] |
//! | F009 注册表虚拟化 | G-A-09 | [`reghive`] |
//! | F010 文件系统重定向 | G-A-10 | [`fsredir`] |
//! | F011 环境变量会话面 | G-A-11 | [`envsess`] |
//! | F012 控制台子系统 | G-A-12 | [`condrv`] |
//! | F013 .lnk 与快捷方式 | G-A-13 | [`lnkfile`] |
//! | F014 PE 资源全解析 | G-A-14 | [`persrc`] |
//! | F015 多语言 .exe 资源 | G-A-15 | [`mlangres`] |
//! | F016 字体链兼容 | G-A-16 | [`fontchain`] |
//! | F017 剪贴板格式族 | G-A-17 | [`clipfmt`] |
//! | F018 拖放协议互通 | G-A-18 | [`dragdrop`] |
//! | F019 COM 本地接口最小集 | G-A-19 | [`comloc`] |
//! | F020 异常与调试面 | G-A-20 | [`excface`] |
//!
//! 共同纪律（与主册铁律对齐，沿用 perfstar/star 两域惯例）：
//! - **零堆热路径**：所有内核路径定长结构，无 Vec/String/Box/format!。
//! - **一处一事实**：每条常量在注释里写明主册依据（G-A-NN 段 + 行为句）。
//! - **先底盘后界面**：F002/F004/F009/F010 是 F001/F005/F008 的依赖底盘，
//!   模块间依赖方向与主册【依赖锚点】一致，不反向借力。
//! - **判据唯一源**：验收标准第一句摘自主册判据，通用十二查叠加执行。
//! - **诚实边界**：拒绝/降级/差异全部显性化——差异表登记、不静默吞错
//!   （十三·补 异常零静默纪律在本域的落点）。

pub mod clipfmt;
pub mod comdlg;
pub mod comloc;
pub mod condrv;
pub mod dblrun;
pub mod dragdrop;
pub mod envsess;
pub mod excface;
pub mod fontchain;
pub mod fsredir;
pub mod gdiface;
pub mod gdiplus;
pub mod lnkfile;
pub mod mlangres;
pub mod pebind;
pub mod peblend;
pub mod persrc;
pub mod reghive;
pub mod winmgr;
pub mod wow64;

/// 全域自检登记名（robust.rs KernelCheckup 用，每项一个独立域集）。
pub const DOMAIN_NAMES: [&str; 20] = [
    "F001-dblrun",
    "F002-peblend",
    "F003-pebind",
    "F004-wow64",
    "F005-winmgr",
    "F006-gdiface",
    "F007-gdiplus",
    "F008-comdlg",
    "F009-reghive",
    "F010-fsredir",
    "F011-envsess",
    "F012-condrv",
    "F013-lnkfile",
    "F014-persrc",
    "F015-mlangres",
    "F016-fontchain",
    "F017-clipfmt",
    "F018-dragdrop",
    "F019-comloc",
    "F020-excface",
];
