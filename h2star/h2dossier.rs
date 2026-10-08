//! H2 域功能档案生成器 · 深化批次四（检查项对账的域内常设设施——
//! 模块注册表 → 逐项档案 → 域健康汇总 → 对账表数据）。
//!
//! **承接判据**（主册 H 域正文 + 分工书铁律七）：
//! - **对账纪律**：每分队完成一项即记「实际行数/目标行数」——本
//!   生成器把对账从「每次手写脚本」升级为「域内常设注册表 + 机查
//!   出表」：任何时刻调 `run_h2dossier_checks()` 即得 50/50 覆盖
//!   对账与检查密度健康线；
//! - **F375 车道（H 域总判据，经域内预检落位）**：孤例=0（零检查
//!   的功能即孤例——发现即红）、密度线（每项 ≥4 条判据锚断言——
//!   首批既达到的密度成为下限）；
//! - **十二章**：档案页数据可导出（开放格式——对账表不再锁在本
//!   队文档里，F135 文档站可机取）。
//!
//! 注册表：模块名 → (功能锚, 目标行数上限)。行数实测由构建侧
//! wc 供给（内核内不计行——本层持注册与判定，数字由调用方注入）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 注册表
// ---------------------------------------------------------------------------

/// 一条功能注册：模块 → 锚 → 目标行数（分工书上限口径）。
#[derive(Clone, Copy, Debug)]
pub struct FeatureReg {
    pub module: &'static str,
    pub anchor: u32, // F 编号数字（251-300）
    pub target_lines: u32,
}

/// 五十项注册表（目标行数 = 分工书 AI-H2 表上限，一处一事实）。
pub const FEATURES: [FeatureReg; 50] = [
    FeatureReg { module: "mediarbit", anchor: 251, target_lines: 2795 },
    FeatureReg { module: "tbgroup", anchor: 252, target_lines: 4875 },
    FeatureReg { module: "quickpin", anchor: 253, target_lines: 585 },
    FeatureReg { module: "translucent", anchor: 254, target_lines: 130 },
    FeatureReg { module: "textdrop", anchor: 255, target_lines: 1105 },
    FeatureReg { module: "hscroll", anchor: 256, target_lines: 975 },
    FeatureReg { module: "openwith", anchor: 257, target_lines: 1040 },
    FeatureReg { module: "deskmenu", anchor: 258, target_lines: 715 },
    FeatureReg { module: "newmenu", anchor: 259, target_lines: 1170 },
    FeatureReg { module: "rename", anchor: 260, target_lines: 1040 },
    FeatureReg { module: "delkeys", anchor: 261, target_lines: 715 },
    FeatureReg { module: "dragsense", anchor: 262, target_lines: 585 },
    FeatureReg { module: "sendto", anchor: 263, target_lines: 520 },
    FeatureReg { module: "propdlg", anchor: 264, target_lines: 1170 },
    FeatureReg { module: "addredit", anchor: 265, target_lines: 780 },
    FeatureReg { module: "navstack", anchor: 266, target_lines: 1170 },
    FeatureReg { module: "storagesense", anchor: 267, target_lines: 520 },
    FeatureReg { module: "diskwarn", anchor: 268, target_lines: 520 },
    FeatureReg { module: "copyresume", anchor: 269, target_lines: 650 },
    FeatureReg { module: "opretry", anchor: 270, target_lines: 845 },
    FeatureReg { module: "extabs", anchor: 271, target_lines: 1105 },
    FeatureReg { module: "textmenu", anchor: 272, target_lines: 1105 },
    FeatureReg { module: "docpos", anchor: 273, target_lines: 845 },
    FeatureReg { module: "allapps", anchor: 274, target_lines: 975 },
    FeatureReg { module: "powermenu", anchor: 275, target_lines: 2990 },
    FeatureReg { module: "snapgroup", anchor: 276, target_lines: 975 },
    FeatureReg { module: "multitb", anchor: 277, target_lines: 910 },
    FeatureReg { module: "projmode", anchor: 278, target_lines: 1170 },
    FeatureReg { module: "padgest", anchor: 279, target_lines: 1170 },
    FeatureReg { module: "longpress", anchor: 280, target_lines: 1105 },
    FeatureReg { module: "notifrule", anchor: 281, target_lines: 975 },
    FeatureReg { module: "singleton", anchor: 282, target_lines: 520 },
    FeatureReg { module: "bootskel", anchor: 283, target_lines: 585 },
    FeatureReg { module: "notresp", anchor: 284, target_lines: 975 },
    FeatureReg { module: "iconcache", anchor: 285, target_lines: 520 },
    FeatureReg { module: "wallmulti", anchor: 286, target_lines: 585 },
    FeatureReg { module: "extraclk", anchor: 287, target_lines: 845 },
    FeatureReg { module: "fontmgr", anchor: 288, target_lines: 520 },
    FeatureReg { module: "printq", anchor: 289, target_lines: 1950 },
    FeatureReg { module: "devpage", anchor: 290, target_lines: 520 },
    FeatureReg { module: "powerank", anchor: 291, target_lines: 1040 },
    FeatureReg { module: "lnkhealth", anchor: 292, target_lines: 650 },
    FeatureReg { module: "removask", anchor: 293, target_lines: 520 },
    FeatureReg { module: "safeeject", anchor: 294, target_lines: 1040 },
    FeatureReg { module: "timesync", anchor: 295, target_lines: 1430 },
    FeatureReg { module: "regionfmt", anchor: 296, target_lines: 715 },
    FeatureReg { module: "walldim", anchor: 297, target_lines: 975 },
    FeatureReg { module: "tileedit", anchor: 298, target_lines: 845 },
    FeatureReg { module: "recommends", anchor: 299, target_lines: 845 },
    FeatureReg { module: "iconlang", anchor: 300, target_lines: 195 },
];

/// 判据密度下限（每项检查数 ≥ 4——首批既达到的密度成为门槛）。
pub const MIN_CHECKS_PER_FEATURE: u32 = 4;

// ---------------------------------------------------------------------------
// 对账引擎
// ---------------------------------------------------------------------------

/// 逐项对账输入：功能锚 → 实测检查数（由聚合侧脚本/构建注入）。
pub type CheckCounts = [u32; 50];

/// 域健康汇总。
#[derive(Debug, PartialEq, Eq)]
pub struct DomainHealth {
    /// 覆盖对账（有检查的项数 / 50）。
    pub covered: usize,
    /// 孤例（零检查项——发现即红，F375 车道「孤例=0」）。
    pub orphans: Vec<u32>,
    /// 密度不足项（检查数 >0 但 < 下限）。
    pub thin: Vec<u32>,
    /// 检查总计。
    pub total_checks: u32,
    /// 注册表目标行数合计（对账分母——50,505 的机算来源）。
    pub target_total: u32,
}

/// 域健康机查：注册表 × 实测检查数 → 汇总。
pub fn domain_health(counts: &CheckCounts) -> DomainHealth {
    let mut orphans = Vec::new();
    let mut thin = Vec::new();
    let mut covered = 0usize;
    let mut total = 0u32;
    for (i, reg) in FEATURES.iter().enumerate() {
        let c = counts[i];
        total += c;
        if c == 0 {
            orphans.push(reg.anchor);
        } else {
            covered += 1;
            if c < MIN_CHECKS_PER_FEATURE {
                thin.push(reg.anchor);
            }
        }
    }
    DomainHealth {
        covered,
        orphans,
        thin,
        total_checks: total,
        target_total: FEATURES.iter().map(|r| r.target_lines).sum(),
    }
}

impl DomainHealth {
    /// 全绿线：全覆盖 + 零孤例 + 零密度不足 + 目标合计 = 50,505。
    pub fn all_green(&self) -> bool {
        self.orphans.is_empty()
            && self.thin.is_empty()
            && self.covered == FEATURES.len()
            && self.target_total == 50_505
    }

    /// 对账表（开放格式数据行——module,anchor,checks,target）。
    pub fn rows(&self, counts: &CheckCounts) -> Vec<String> {
        FEATURES
            .iter()
            .enumerate()
            .map(|(i, r)| {
                alloc::format!("{},{},{},{}", r.module, r.anchor, counts[i], r.target_lines)
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2dossier_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2dossier");
    // 注册表完整性：锚连续 251..=300 无缺号无重复；模块名唯一。
    let anchors: Vec<u32> = FEATURES.iter().map(|r| r.anchor).collect();
    let contiguous = anchors.iter().enumerate().all(|(i, a)| *a == 251 + i as u32);
    let mut mods = FEATURES.iter().map(|r| r.module).collect::<Vec<_>>();
    mods.sort_unstable();
    let unique = mods.windows(2).all(|w| w[0] != w[1]);
    set.add(
        "h2dossier registry complete",
        contiguous && unique && FEATURES.len() == 50,
        "251-300 no gaps no dups",
    );
    // 目标合计 = 50,505（分工书口径的机算钉死）。
    let sum: u32 = FEATURES.iter().map(|r| r.target_lines).sum();
    set.add(
        "h2dossier target sum",
        sum == 50_505,
        "50,505 exact",
    );
    // 健康：全量注册（每项 4 条）→ 全绿；分母机算一致。
    let full: CheckCounts = [4; 50];
    let h1 = domain_health(&full);
    set.add(
        "h2dossier health green",
        h1.all_green() && h1.total_checks == 200 && h1.covered == 50,
        "min density baseline",
    );
    // 孤例：零检查项点名（F375 车道「孤例=0」的对账面）。
    let mut partial = full;
    partial[7] = 0; // F258 deskmenu 缺检
    let h2 = domain_health(&partial);
    set.add(
        "h2dossier orphan named",
        h2.orphans == vec![258] && !h2.all_green() && h2.covered == 49,
        "zero-check = orphan",
    );
    // 密度不足：3 条 < 下限 4。
    let mut thin = full;
    thin[29] = 3; // F280 longpress 偏瘦
    let h3 = domain_health(&thin);
    set.add(
        "h2dossier thin flagged",
        h3.thin == vec![280] && h3.orphans.is_empty() && !h3.all_green(),
        "below density floor",
    );
    // 开放导出：50 行 CSV 形数据，行字段序固定（module,anchor,checks,target）。
    let rows = h1.rows(&full);
    set.add(
        "h2dossier export format",
        rows.len() == 50
            && rows[0] == "mediarbit,251,4,2795"
            && rows[49] == "iconlang,300,4,195",
        "open format rows",
    );
    set.add(
        "h2dossier density const",
        MIN_CHECKS_PER_FEATURE == 4,
        "floor = first-batch density",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2dossier_all_green() {
        let set = run_h2dossier_checks();
        assert!(set.all_passed(), "h2dossier 自检有红项");
        assert!(!set.truncated(), "h2dossier 自检溢出");
    }

    #[test]
    fn orphan_never_silent_at_scale() {
        // 随机挖掉 10 项检查：孤例名单精确点名（一个不漏、一个不冤）。
        let mut counts = [4u32; 50];
        let mut holes = Vec::new();
        for i in (0..50).step_by(5) {
            counts[i] = 0;
            holes.push(251 + i as u32);
        }
        let h = domain_health(&counts);
        assert_eq!(h.orphans, holes);
        assert_eq!(h.covered, 40);
    }
}
