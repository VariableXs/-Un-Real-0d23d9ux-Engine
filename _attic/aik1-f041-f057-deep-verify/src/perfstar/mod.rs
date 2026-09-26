//! perfstar — Varix STAR I start · B 性能域深化（F041~F057 · AI-K1 分工包）。
//!
//! 本目录是《Varix STAR I start.md》主册 B-3 深化设计报告（G-B-01 ~ G-B-17）
//! 的判据实装层。十七项各占一个子模块，一项一事实：
//!
//! | 项 | 判据锚 | 子模块 |
//! | --- | --- | --- |
//! | F041 帧率账本 | G-B-01 | [`frameledger`] |
//! | F042 帧率归因器 | G-B-02 | [`frameattr`] |
//! | F043 冷启动画像 | G-B-03 | [`startprof`] |
//! | F044 预取指纹 v2 | G-B-04 | [`prefetch2`] |
//! | F045 页缓存水位策略 | G-B-05 | [`pagewater`] |
//! | F046 写合并窗口自适应 | G-B-06 | [`wcoalesce`] |
//! | F047 调度器延迟预算深化 | G-B-07 | [`latbudget`] |
//! | F048 CPU 频率联动 | G-B-08 | [`cpufreq`] |
//! | F049 空转清零工程 | G-B-09 | [`idlezero`] |
//! | F050 中断合并 | G-B-10 | [`intrcoal`] |
//! | F051 大页策略 | G-B-11 | [`bigpage`] |
//! | F052 堆碎片治理 | G-B-12 | [`heapfrag`] |
//! | F053 启动并行度 | G-B-13 | [`bootpar`] |
//! | F054 图像解码 SIMD | G-B-14 | [`imgsimd`] |
//! | F055 字形光栅缓存 | G-B-15 | [`glyphcache`] |
//! | F056 合成器脏区深化 | G-B-16 | [`dirtyrect`] |
//! | F057 IO 调度分级 | G-B-17 | [`iotier`] |
//!
//! 共同纪律（与主册铁律对齐）：
//! - **零堆热路径**：所有内核路径定长结构，无 Vec/String/Box/format!。
//! - **一处一事实**：每条常量在注释里写明主册依据与推导。
//! - **先测量后调参**：账本（F041）是全域共同前提，一切数字与监视器同源。
//! - **判据唯一源**：验收标准第一句摘自主册判据，十二查叠加执行。

pub mod bigpage;
pub mod bootpar;
pub mod cpufreq;
pub mod dirtyrect;
pub mod frameattr;
pub mod frameledger;
pub mod glyphcache;
pub mod heapfrag;
pub mod idlezero;
pub mod imgsimd;
pub mod intrcoal;
pub mod iotier;
pub mod latbudget;
pub mod pagewater;
pub mod prefetch2;
pub mod startprof;
pub mod wcoalesce;

/// 全域自检登记名（robust.rs KernelCheckup 用，每项一个独立域集）。
pub const DOMAIN_NAMES: [&str; 17] = [
    "F041-frameledger",
    "F042-frameattr",
    "F043-startprof",
    "F044-prefetch2",
    "F045-pagewater",
    "F046-wcoalesce",
    "F047-latbudget",
    "F048-cpufreq",
    "F049-idlezero",
    "F050-intrcoal",
    "F051-bigpage",
    "F052-heapfrag",
    "F053-bootpar",
    "F054-imgsimd",
    "F055-glyphcache",
    "F056-dirtyrect",
    "F057-iotier",
];
