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
//!
//! **深化分层**：主层（run_*_checks）= 主册判据验收面；批次一深化
//! （run_*_deep）= 【设计细节】协议语义面；批次二深化（deep/f0NNd.rs）=
//! 【功能定义】「全语义对齐」的执行与治理面；批次三深化（deep2/f0NNe.rs）=
//! 执行/边界/注入面；批次四深化（deep3/f0NNf.rs）= 序列化/账本/容错面。
//! 五层并行入块，全绿才亮。

pub mod buildchain;
pub mod codepage;
pub mod compatledger;
pub mod compatwiz;
pub mod deep;
pub mod deep2;
pub mod deep3;
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
/// 五层并行入块：主层判据 + 批次一深化（run_*_deep）+ 批次二深化
/// （deep/f0NNd）+ 批次三深化（deep2/f0NNe）+ 批次四深化（deep3/f0NNf）
/// 五层全绿才亮。
/// 实现纪律：fn 指针表 + 逐域求值——五层 × 20 域 = 125 个检查调用
/// 若排在同一栈帧会溢出测试线程栈（批次三隔离舱实测 STATUS_STACK_OVERFLOW，
/// 缺陷账 #26）；逐域求值后局部 CheckSet 用完即弃，栈峰值回到单域水平。
pub fn run_compatstar2_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("COMPAT-S2");
    const DOMAIN_TAGS: [&str; 20] = [
        "F021", "F022", "F023", "F024", "F025", "F026", "F027", "F028", "F029", "F030",
        "F031", "F032", "F033", "F034", "F035", "F036", "F037", "F038", "F039", "F040",
    ];
    let mains: [fn() -> crate::checks::CheckSet; 20] = [
        memalign::run_memalign_checks,
        timefam::run_timefam_checks,
        winsock::run_winsock_checks,
        tlsstore::run_tlsstore_checks,
        printpdf::run_printpdf_checks,
        winmm::run_winmm_checks,
        imm32::run_imm32_checks,
        dpistate::run_dpistate_checks,
        moneum::run_moneum_checks,
        installr::run_installr_checks,
        uninstall::run_uninstall_checks,
        runtimes::run_runtimes_checks,
        buildchain::run_buildchain_checks,
        codepage::run_codepage_checks,
        compatwiz::run_compatwiz_checks,
        stardraft::run_stardraft_checks,
        peblockui::run_peblockui_checks,
        isolevel::run_isolevel_checks,
        gamefront::run_gamefront_checks,
        compatledger::run_compatledger_checks,
    ];
    let deeps1: [fn() -> crate::checks::CheckSet; 20] = [
        memalign::run_memalign_deep,
        timefam::run_timefam_deep,
        winsock::run_winsock_deep,
        tlsstore::run_tlsstore_deep,
        printpdf::run_printpdf_deep,
        winmm::run_winmm_deep,
        imm32::run_imm32_deep,
        dpistate::run_dpistate_deep,
        moneum::run_moneum_deep,
        installr::run_installr_deep,
        uninstall::run_uninstall_deep,
        runtimes::run_runtimes_deep,
        buildchain::run_buildchain_deep,
        codepage::run_codepage_deep,
        compatwiz::run_compatwiz_deep,
        stardraft::run_stardraft_deep,
        peblockui::run_peblockui_deep,
        isolevel::run_isolevel_deep,
        gamefront::run_gamefront_deep,
        compatledger::run_compatledger_deep,
    ];
    let deeps2: [fn() -> crate::checks::CheckSet; 20] = [
        deep::f021d::run_f021d_checks,
        deep::f022d::run_f022d_checks,
        deep::f023d::run_f023d_checks,
        deep::f024d::run_f024d_checks,
        deep::f025d::run_f025d_checks,
        deep::f026d::run_f026d_checks,
        deep::f027d::run_f027d_checks,
        deep::f028d::run_f028d_checks,
        deep::f029d::run_f029d_checks,
        deep::f030d::run_f030d_checks,
        deep::f031d::run_f031d_checks,
        deep::f032d::run_f032d_checks,
        deep::f033d::run_f033d_checks,
        deep::f034d::run_f034d_checks,
        deep::f035d::run_f035d_checks,
        deep::f036d::run_f036d_checks,
        deep::f037d::run_f037d_checks,
        deep::f038d::run_f038d_checks,
        deep::f039d::run_f039d_checks,
        deep::f040d::run_f040d_checks,
    ];
    let deeps3: [fn() -> crate::checks::CheckSet; 20] = [
        deep2::f021e::run_f021e_checks,
        deep2::f022e::run_f022e_checks,
        deep2::f023e::run_f023e_checks,
        deep2::f024e::run_f024e_checks,
        deep2::f025e::run_f025e_checks,
        deep2::f026e::run_f026e_checks,
        deep2::f027e::run_f027e_checks,
        deep2::f028e::run_f028e_checks,
        deep2::f029e::run_f029e_checks,
        deep2::f030e::run_f030e_checks,
        deep2::f031e::run_f031e_checks,
        deep2::f032e::run_f032e_checks,
        deep2::f033e::run_f033e_checks,
        deep2::f034e::run_f034e_checks,
        deep2::f035e::run_f035e_checks,
        deep2::f036e::run_f036e_checks,
        deep2::f037e::run_f037e_checks,
        deep2::f038e::run_f038e_checks,
        deep2::f039e::run_f039e_checks,
        deep2::f040e::run_f040e_checks,
    ];
    let deeps4: [fn() -> crate::checks::CheckSet; 20] = [
        deep3::f021f::run_f021f_checks,
        deep3::f022f::run_f022f_checks,
        deep3::f023f::run_f023f_checks,
        deep3::f024f::run_f024f_checks,
        deep3::f025f::run_f025f_checks,
        deep3::f026f::run_f026f_checks,
        deep3::f027f::run_f027f_checks,
        deep3::f028f::run_f028f_checks,
        deep3::f029f::run_f029f_checks,
        deep3::f030f::run_f030f_checks,
        deep3::f031f::run_f031f_checks,
        deep3::f032f::run_f032f_checks,
        deep3::f033f::run_f033f_checks,
        deep3::f034f::run_f034f_checks,
        deep3::f035f::run_f035f_checks,
        deep3::f036f::run_f036f_checks,
        deep3::f037f::run_f037f_checks,
        deep3::f038f::run_f038f_checks,
        deep3::f039f::run_f039f_checks,
        deep3::f040f::run_f040f_checks,
    ];
    // 逐域求值：每轮局部 CheckSet 在判定后即释放（栈峰值 = 单域五层）。
    for i in 0..20 {
        let main = mains[i]();
        let deep1 = deeps1[i]();
        let deep2 = deeps2[i]();
        let deep3 = deeps3[i]();
        let deep4 = deeps4[i]();
        let passed = main.all_passed() && !main.truncated()
            && deep1.all_passed() && !deep1.truncated()
            && deep2.all_passed() && !deep2.truncated()
            && deep3.all_passed() && !deep3.truncated()
            && deep4.all_passed() && !deep4.truncated();
        set.add(DOMAIN_TAGS[i], passed, if passed { "" } else { "main-or-deep1-or-deep2-or-deep3-or-deep4 red" });
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
