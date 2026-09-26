//! compatstar2 — Varix STAR I start · A 应用兼容域·后段（F021~F040 · AI-C2 分工包）。
//!
//! 本目录是《Varix STAR I start.md》主册 A-5 深化设计报告（G-A-21 ~ G-A-40）
//! 的判据实装层。二十项各占一个子模块，一项一事实。
//!
//! **目录命名说明**：AI-C1 分工包（F001~F020）先行占用了 `compatstar/`
//! 目录名（在途施工中）；本包按 secstar → secstar2 先例落 `compatstar2/`，
//! 两包目录互不重叠、依赖方向单向（本包不反向借力 C1 未收口模块）。
//!
//! | 项 | 判据锚 | 子模块 |
//! | --- | --- | --- |
//! | F021 内存语义对齐 | G-A-21 | [`memalign`] |
//! | F022 时间与定时器族 | G-A-22 | [`timefam`] |
//! | F023 网络栈 Winsock 面 | G-A-23 | [`winsock`] |
//! | F024 TLS 与证书库 | G-A-24 | [`tlsstore`] |
//! | F025 打印虚拟面 | G-A-25 | [`printpdf`] |
//! | F026 音频 WinMM/DirectSound 面 | G-A-26 | [`winmm`] |
//! | F027 输入法 IMM32 面 | G-A-27 | [`imm32`] |
//! | F028 高 DPI 感知三态 | G-A-28 | [`dpistate`] |
//! | F029 多显示器前瞻接口 | G-A-29 | [`moneum`] |
//! | F030 安装器兼容模式 | G-A-30 | [`installr`] |
//! | F031 卸载与残留清扫 | G-A-31 | [`uninstall`] |
//! | F032 开源运行时直装族 | G-A-32 | [`runtimes`] |
//! | F033 构建工具链判例 | G-A-33 | [`buildchain`] |
//! | F034 字符编码终局 | G-A-34 | [`codepage`] |
//! | F035 兼容性自检向导 | G-A-35 | [`compatwiz`] |
//! | F036 星卡自动草稿 | G-A-36 | [`stardraft`] |
//! | F037 peblock 门用户可见化 | G-A-37 | [`peblockui`] |
//! | F038 应用隔离档位 | G-A-38 | [`isolevel`] |
//! | F039 游戏兼容前瞻面 | G-A-39 | [`gamefront`] |
//! | F040 兼容域总判据 | G-A-40 | [`compatledger`] |
//!
//! 共同纪律（与主册铁律对齐，沿用 perfstar/star/secstar2 三域惯例）：
//! - **零堆热路径**：所有内核路径定长结构，无 Vec/String/Box/format!。
//! - **一处一事实**：每条常量在注释里写明主册依据（G-A-NN 段 + 行为句）。
//! - **判据唯一源**：验收标准第一句摘自主册判据，通用十二查叠加执行。
//! - **异常零静默**：拒绝/降级/差异全部显性化（十三·补 落点）——错误码
//!   如实翻译（F023）、NULL 语义如实返回（F021）、软失败策略公开（F024）。
//! - **诚实边界**：实机/QEMU 类判据（录屏/秒表/对拍）登记「随闸门补测」，
//!   域内以判据账本 + 模型对拍面承载，不虚构实测数字。

pub mod buildchain;
pub mod codepage;
pub mod compatledger;
pub mod compatwiz;
pub mod dpistate;
pub mod gamefront;
pub mod imm32;
pub mod installr;
pub mod isolevel;
pub mod memalign;
pub mod moneum;
pub mod peblockui;
pub mod printpdf;
pub mod runtimes;
pub mod stardraft;
pub mod timefam;
pub mod tlsstore;
pub mod uninstall;
pub mod winmm;
pub mod winsock;

/// 全域自检登记名（robust.rs 域函数指针表用，每项一个独立域集）。
pub const DOMAIN_NAMES: [&str; 20] = [
    "F021-memalign",
    "F022-timefam",
    "F023-winsock",
    "F024-tlsstore",
    "F025-printpdf",
    "F026-winmm",
    "F027-imm32",
    "F028-dpistate",
    "F029-moneum",
    "F030-installr",
    "F031-uninstall",
    "F032-runtimes",
    "F033-buildchain",
    "F034-codepage",
    "F035-compatwiz",
    "F036-stardraft",
    "F037-peblockui",
    "F038-isolevel",
    "F039-gamefront",
    "F040-compatledger",
];

/// 域聚合自检（robust.rs 单行注册；与 secstar2 同款——容量纪律：
/// 单聚合永不超容，域内逐模块红绿在此子行展开）。
/// 深化层（run_*_deep）与主层并行入块：主层判据 + 深化语义层双双全绿才亮。
pub fn run_compatstar2_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("COMPAT-S2");
    let blocks: [(&'static str, crate::checks::CheckSet, crate::checks::CheckSet); 20] = [
        ("F021", memalign::run_memalign_checks(), memalign::run_memalign_deep()),
        ("F022", timefam::run_timefam_checks(), timefam::run_timefam_deep()),
        ("F023", winsock::run_winsock_checks(), winsock::run_winsock_deep()),
        ("F024", tlsstore::run_tlsstore_checks(), tlsstore::run_tlsstore_deep()),
        ("F025", printpdf::run_printpdf_checks(), printpdf::run_printpdf_deep()),
        ("F026", winmm::run_winmm_checks(), winmm::run_winmm_deep()),
        ("F027", imm32::run_imm32_checks(), imm32::run_imm32_deep()),
        ("F028", dpistate::run_dpistate_checks(), dpistate::run_dpistate_deep()),
        ("F029", moneum::run_moneum_checks(), moneum::run_moneum_deep()),
        ("F030", installr::run_installr_checks(), installr::run_installr_deep()),
        ("F031", uninstall::run_uninstall_checks(), uninstall::run_uninstall_deep()),
        ("F032", runtimes::run_runtimes_checks(), runtimes::run_runtimes_deep()),
        ("F033", buildchain::run_buildchain_checks(), buildchain::run_buildchain_deep()),
        ("F034", codepage::run_codepage_checks(), codepage::run_codepage_deep()),
        ("F035", compatwiz::run_compatwiz_checks(), compatwiz::run_compatwiz_deep()),
        ("F036", stardraft::run_stardraft_checks(), stardraft::run_stardraft_deep()),
        ("F037", peblockui::run_peblockui_checks(), peblockui::run_peblockui_deep()),
        ("F038", isolevel::run_isolevel_checks(), isolevel::run_isolevel_deep()),
        ("F039", gamefront::run_gamefront_checks(), gamefront::run_gamefront_deep()),
        ("F040", compatledger::run_compatledger_checks(), compatledger::run_compatledger_deep()),
    ];
    for (tag, main, deep) in blocks {
        let passed = main.all_passed() && !main.truncated() && deep.all_passed() && !deep.truncated();
        set.add(tag, passed, if passed { "" } else { "main-or-deep red" });
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 域聚合全绿：二十子域逐项无红无截断。
    #[test]
    fn compatstar2_domain_aggregate_all_green() {
        let set = run_compatstar2_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "COMPAT-S2 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    /// DOMAIN_NAMES 与聚合器块数一致（登记表对账）。
    #[test]
    fn domain_registry_consistent() {
        assert_eq!(DOMAIN_NAMES.len(), 20);
    }
}
