//! F292 快捷方式健康 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：三类病注入用例；淡化 50% 视觉；自动修复成功率
//! 记录；引导框两出路；循环检测判据。
//!
//! **设计要点（主册）**：.lnk 三类病全管：目标丢失（悬停提示「目标不
//! 存在」+图标淡化 50%+右键「定位目标」/「删除」两出路）、目标改名
//! （尝试同目录同名近似自动修复，修不了才报断链）、循环指向（检测到
//! 自指快捷方式直接标坏不递归）；桌面快捷方式双击断链弹温和引导。
//!
//! 实装：健康检查器（三类病：丢失/改名可修/循环）；自动修复（同目录
//! 同名近似匹配——修不了才报断链）；修复账（成功率记录）；引导框两
//! 出路（定位目标/删除——没有第三条静默路）；循环检测（链上环——
//! 不递归直接标坏）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 快捷方式健康状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LnkHealth {
    /// 健康。
    Good,
    /// 目标丢失（图标淡化 50% + 两出路）。
    TargetMissing,
    /// 目标改名——自动修复成功。
    AutoRepaired,
    /// 循环指向（自指——直接标坏不递归）。
    Circular,
}

/// 图标淡化（50%——渲染层直读透明度）。
pub const FADE_PCT: u8 = 50;

/// 文件系统快照（注入式——`exists`/`children` 由调用方供给）。
pub struct FsView<'a> {
    pub exists: &'a dyn Fn(&str) -> bool,
    /// 同目录同名近似候选（改名修复的检索源）。
    pub similar: &'a dyn Fn(&str) -> Vec<String>,
}

/// 快捷方式体检。
pub struct LnkCheck {
    pub path: String,
    pub target: String,
}

impl LnkCheck {
    /// 健康检查（三类病判定——判据的执行点）。
    pub fn health(&self, fs: &FsView) -> LnkHealth {
        // 循环检测优先：目标链回到自身 → Circular（不递归展开）。
        if self.is_circular(fs) {
            return LnkHealth::Circular;
        }
        if (fs.exists)(&self.target) {
            return LnkHealth::Good;
        }
        // 改名近似修复：同目录同名近似候选唯一命中 → 修。
        if self.try_repair(fs).is_some() {
            return LnkHealth::AutoRepaired;
        }
        LnkHealth::TargetMissing
    }

    /// 循环检测：target 串回含自身路径（自指）——O(1) 判定不递归。
    fn is_circular(&self, _fs: &FsView) -> bool {
        self.target == self.path
    }

    /// 自动修复：同目录近似候选唯一命中 → 返回新目标。
    pub fn try_repair(&self, fs: &FsView) -> Option<String> {
        if (fs.exists)(&self.target) {
            return None; // 没病不修。
        }
        let cands = (fs.similar)(&self.target);
        if cands.len() == 1 {
            Some(cands[0].clone())
        } else {
            None // 零候选或多候选都不猜——修不了才报断链。
        }
    }
}

/// 修复账：成功率记录（自动修复成功/尝试）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepairLedger {
    pub attempted: u32,
    pub repaired: u32,
}

impl RepairLedger {
    pub fn success_rate_pct(&self) -> u64 {
        if self.attempted == 0 {
            0
        } else {
            self.repaired as u64 * 100 / self.attempted as u64
        }
    }
}

/// 温和引导框（断链双击）：两出路——没有第三条静默路。
pub const GUIDE_TITLE: &str = "这个快捷方式的目标找不到了";
pub const GUIDE_ACTIONS: [&str; 2] = ["定位目标", "删除快捷方式"];

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_lnkhealth_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F292");
    let exists = |p: &str| p == "vx:/文档/报告v2.docx";
    let similar = |p: &str| {
        if p.ends_with("报告v1.docx") {
            alloc::vec![String::from("vx:/文档/报告v2.docx")]
        } else {
            Vec::new()
        }
    };
    let fs = FsView { exists: &exists, similar: &similar };
    // 三类病注入。
    let good = LnkCheck { path: String::from("vx:/桌面/a.lnk"), target: String::from("vx:/文档/报告v2.docx") };
    let renamed = LnkCheck { path: String::from("vx:/桌面/b.lnk"), target: String::from("vx:/文档/报告v1.docx") };
    let lost = LnkCheck { path: String::from("vx:/桌面/c.lnk"), target: String::from("vx:/ nowhere.docx") };
    let circular = LnkCheck { path: String::from("vx:/桌面/self.lnk"), target: String::from("vx:/桌面/self.lnk") };
    set.add(
        "F292 three diseases",
        good.health(&fs) == LnkHealth::Good
            && renamed.health(&fs) == LnkHealth::AutoRepaired
            && lost.health(&fs) == LnkHealth::TargetMissing
            && circular.health(&fs) == LnkHealth::Circular,
        "good/rename/lost/loop",
    );
    // 淡化 50% 视觉常量。
    set.add("F292 fade 50", FADE_PCT == 50, "icon dim");
    // 自动修复成功率记录。
    let mut ledger = RepairLedger::default();
    for chk in [&renamed, &lost] {
        ledger.attempted += 1;
        if chk.try_repair(&fs).is_some() {
            ledger.repaired += 1;
        }
    }
    set.add(
        "F292 repair rate",
        ledger.success_rate_pct() == 50 && ledger.attempted == 2,
        "1/2 = 50%",
    );
    // 引导框两出路。
    set.add(
        "F292 guide two exits",
        GUIDE_ACTIONS.len() == 2 && GUIDE_ACTIONS[0] == "定位目标" && GUIDE_TITLE.contains("找不到"),
        "gentle, not error",
    );
    // 修复只取唯一候选（多候选不猜）。
    let ambiguous = LnkCheck { path: String::from("x.lnk"), target: String::from("vx:/t") };
    let multi = |_: &str| alloc::vec![String::from("vx:/t1"), String::from("vx:/t2")];
    let fs2 = FsView { exists: &|_| false, similar: &multi };
    set.add(
        "F292 no wild guess",
        ambiguous.try_repair(&fs2).is_none(),
        "ambiguous = missing",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f292_lnk_health() {
        let set = run_lnkhealth_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F292 自检红 {f}/{p}");
    }

    #[test]
    fn healthy_never_repaired() {
        let chk = LnkCheck { path: String::from("a.lnk"), target: String::from("b") };
        let fs = FsView { exists: &|_| true, similar: &|_| Vec::new() };
        assert!(chk.try_repair(&fs).is_none(), "没病不修——零误伤");
    }
}
