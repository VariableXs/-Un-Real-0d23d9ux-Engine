//! UNX 册模块登记（CoRun Varix STAR II · Unxreal —— UNX-A1 引导与早年初始化）。
//!
//! 本册与主内核 acpi.rs（F008 RSDP/XSDT/FADT/MADT 基础解析）为分层深化：
//! 本册做 ACPI 表专项（DSDT/SSDT/AML/拓扑/UEFI 态），基础解析不重复实现，
//! 直接复用 `crate::acpi` 的 SdtHeader/checksum_ok/SDT_HEADER_LEN。
//! 后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433。

pub mod unx041_dsdt;
pub mod unx041_dsdt_checks;
// unx041_dsdt_deep — UNX-F0041 DSDT 采集与校验链深化版（四通道定位链 RSDP双验/XSDT优先RSDT回退/FACP三闸/X_DSDT优先32位回退/DSDT四闸 深度校验/OEM ID三态落账全零-空白-可读/诊断三级 Info-Warn-Fatal/BootReport append-only不阻塞容量界/健康度三因子万分制/DsdtSnap 快照FNV+往返/0x2F 段十六码与速写版 0x2E 不重叠）
pub mod unx041_dsdt_deep;

/// UNX 册域标签（F475 闭环登记用）。
pub const UNX_DOMAIN: &str = "unxstar";

/// UNX 册判据聚合（逐子集折叠，新单入账在此数组追加）。
///
/// 原实现是 `CheckSet::merge(单子集)`——`merge` 需二参，`unxstar` 只有
/// 一个子集，这个调用**根本编译不过**。之所以一直没暴露：本模块此前未在
/// `lib.rs` 声明，是孤儿模块，编译单元从未被纳入（AI-CGPU 给CGPU 接线时
/// 顺路补声明才暴露）。折叠范式与 `svstar2::run_svstar2_checks` 一致。
pub fn run_unxstar_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new(UNX_DOMAIN);
    let blocks: [(&'static str, CheckSet); 2] = [
        ("UNX-F0041", unx041_dsdt_checks::run_unx041_checks()),
        ("UNX-F0041-DEEP", unx041_dsdt_deep::run_unx041b_checks()),
    ];
    for (tag, sub) in blocks.iter() {
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            tag,
            passed,
            if passed {
                ""
            } else if sub.truncated() {
                "sub-checks truncated"
            } else {
                "sub-checks red"
            },
        );
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UNX 册域聚合全绿（接线 F475 闭环后域内红项必须在此暴露）。
    #[test]
    fn unxstar_domain_aggregate_all_green() {
        let set = run_unxstar_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "UNX 册自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
        assert!(!set.truncated(), "UNX 域聚合溢出（子集数超 CheckSet 上限）");
    }
}
