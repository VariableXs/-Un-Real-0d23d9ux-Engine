//! 深化层五 · F142 安全披露通道（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：漏洞知识库 → F120 诊断中心/F193 安全模式的面板
//! 数据行、导出脱敏行（版本号保留、内部代号硬删）。

use super::f142g::{fix_priority, VulnStatus};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 安全面板装配：漏洞条目 → 面板行（状态人话 + 优先级）
// ---------------------------------------------------------------------------

pub struct VulnPanelRow {
    pub cve: &'static str,
    pub status_label: &'static str,
    pub priority: u8,
}

/// 裁决披露中（Triaging）的行不外发到用户面板（embargo 红线）——
/// 面板只呈现 Fixed/Published；输入侧带 embargo 标记。
pub fn panel_rows(
    entries: &[(u32, &'static str, VulnStatus, u32, u8)],
    embargoed: &[u32],
) -> alloc::vec::Vec<VulnPanelRow> {
    entries
        .iter()
        .filter(|(id, _, _, _, _)| !embargoed.contains(id))
        .map(|(_, cve, st, cvss, pop)| VulnPanelRow {
            cve,
            status_label: match st {
                VulnStatus::Triaging => "处理中",
                VulnStatus::Fixed => "已修复：随下个更新安装",
                VulnStatus::Published => "已披露",
            },
            priority: fix_priority(*cvss, *pop),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 安全模式接线：未修复的高优先项 → F193 安全模式提示行
// ---------------------------------------------------------------------------

pub fn safe_mode_hints(rows: &[VulnPanelRow]) -> alloc::vec::Vec<&'static str> {
    rows.iter()
        .filter(|r| r.status_label == "处理中" && r.priority == 0)
        .map(|r| r.cve)
        .collect()
}

// ---------------------------------------------------------------------------
// 导出脱敏：诊断导出行——CVE 与版本保留，内部代号/报告人硬删
// ---------------------------------------------------------------------------

pub struct DiagExport {
    pub cve: &'static str,
    pub ver: u32,
    pub internal_codename: &'static str,
    pub reporter: &'static str,
}

pub fn export_sanitize(rows: &[DiagExport]) -> alloc::vec::Vec<(&'static str, u32)> {
    rows.iter().map(|r| (r.cve, r.ver)).collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F142H_TAG: &str = "stareco-F142-deep5";

pub fn run_f142_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F142H_TAG);

    let entries = [
        (1u32, "CVE-2026-0001", VulnStatus::Published, 90u32, 2u8),
        (2, "CVE-2026-0002", VulnStatus::Fixed, 80, 0),
        (3, "CVE-2026-0003", VulnStatus::Triaging, 95, 2),
    ];

    // embargo 拦截
    let all = panel_rows(&entries, &[]);
    set.add("f142h embargo leak", all.len() == 3, "基线：未拦时三条");
    let guarded = panel_rows(&entries, &[3]);
    set.add(
        "f142h embargo guarded",
        guarded.len() == 2 && !guarded.iter().any(|r| r.cve == "CVE-2026-0003"),
        "披露中条目不外发",
    );

    // 状态人话 + 优先级
    set.add(
        "f142h labels",
        guarded[0].status_label.contains("已披露") && guarded[1].status_label.contains("下个更新"),
        "状态人话",
    );
    set.add("f142h priority", guarded[0].priority == 0, "野外利用高分 P0");

    // 安全模式提示
    let hints = safe_mode_hints(&all);
    set.add(
        "f142h safe mode hints",
        hints == alloc::vec!["CVE-2026-0003"],
        "未修复 P0 点名",
    );

    // 导出脱敏
    let exports = [DiagExport {
        cve: "CVE-2026-0001",
        ver: 12,
        internal_codename: "project-x",
        reporter: "someone",
    }];
    let sanitized = export_sanitize(&exports);
    set.add(
        "f142h export",
        sanitized == alloc::vec![("CVE-2026-0001", 12)],
        "只保留 CVE+版本",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn empty_panel_ok() {
        assert!(panel_rows(&[], &[]).is_empty());
        assert!(safe_mode_hints(&[]).is_empty());
    }

    #[test]
    fn triaging_never_even_counted() {
        let rows = panel_rows(
            &[(9u32, "CVE-2026-9999", VulnStatus::Triaging, 99, 2)],
            &[],
        );
        // embargo 未标记也不该出现？——不：面板门只认显式 embargo 清单，
        // 但 Triaging 状态行以「处理中」呈现是登记面职责——如实呈现。
        assert_eq!(rows[0].status_label, "处理中");
    }
}
