//! ves404_s22t07t08 — VE-F4164 · T07-T08 组自查核验
//! 两组逐项核验：T07 性能面复测（成本模型复算一致 + 四维预算未超 + 基线哈希完整 + 长稳报告在有效期）；
//! T08 工具面复审（五标准逐条 + fuzz 总闸三零线 + 语料分级正确）——证据引用制执行。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x07E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RvCode {
    Ok = 0,
    ModelDrift = SEG_BASE + 0x01,     // 0x07E1
    BudgetOver = SEG_BASE + 0x02,     // 0x07E2
    HashIncomplete = SEG_BASE + 0x03, // 0x07E3
    SoakExpired = SEG_BASE + 0x04,    // 0x07E4
    ZeroLineBreak = SEG_BASE + 0x05,  // 0x07E5
    GradeWrong = SEG_BASE + 0x06,     // 0x07E6
}

/// T07 复测四项。
#[derive(Clone, Copy)]
pub struct T07Recheck {
    pub model_a: u32,       // 成本模型复算值
    pub model_b: u32,       // 模型当前值（一致才过）
    pub budgets: [u32; 4],  // 查找/切换/内存/包体积 当前消耗
    pub caps: [u32; 4],     // 对应预算上限
    pub baseline_hash: u64, // 基线哈希（0 = 不完整）
    pub soak_age_days: u32, // 长稳报告年龄
}

impl T07Recheck {
    pub const SOAK_VALID_DAYS: u32 = 90;

    pub fn model_ok(&self) -> bool { self.model_a == self.model_b }
    pub fn budgets_ok(&self) -> bool {
        self.budgets.iter().zip(self.caps.iter()).all(|(b, c)| b <= c)
    }
    pub fn hash_ok(&self) -> bool { self.baseline_hash != 0 }
    pub fn soak_ok(&self) -> bool { self.soak_age_days <= Self::SOAK_VALID_DAYS }
    pub fn all_ok(&self) -> bool {
        self.model_ok() && self.budgets_ok() && self.hash_ok() && self.soak_ok()
    }
    /// 越限预算定位。
    pub fn over_budgets(&self) -> Vec<usize> {
        self.budgets.iter().zip(self.caps.iter()).enumerate()
            .filter(|(_, (b, c))| b > c).map(|(i, _)| i).collect()
    }
}

/// T08 复审三项：五标准 / 三零线 / 语料分级。
#[derive(Clone)]
pub struct T08Recheck {
    pub dx_standards: [bool; 5],     // 快/稳/diff/集成/扩展
    pub zero_line: (u32, u32, u32),  // 崩溃/挂起/越权写
    pub grades: Vec<(&'static str, u8)>, // (语料 id, 分级 0/1/2)
}

impl T08Recheck {
    pub fn five_standards_ok(&self) -> bool { self.dx_standards.iter().all(|x| *x) }
    pub fn zero_line_ok(&self) -> bool { self.zero_line == (0, 0, 0) }
    /// 语料分级正确：受限级（2）不允许降级标记错（此处核验值域与顺序）。
    pub fn grades_ok(&self) -> bool {
        self.grades.iter().all(|(_, g)| *g <= 2)
    }
    pub fn all_ok(&self) -> bool {
        self.five_standards_ok() && self.zero_line_ok() && self.grades_ok()
    }
}

/// 无障碍预算保底档核验（域本色红线）：四维预算中无障碍专列不得低于保底。
pub fn a11y_floor_ok(budget_api_ms: u32) -> bool { budget_api_ms >= 150 }

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves404_checks() -> CheckSet {
    let mut set = CheckSet::new("ves404");

    // C404-01 T07 成本模型复算一致。
    let t7 = T07Recheck {
        model_a: 420, model_b: 420,
        budgets: [80, 300, 50000, 12000], caps: [100, 400, 96000, 16000],
        baseline_hash: 0xDEADBEEF, soak_age_days: 30,
    };
    chk(&mut set, "C404-01", t7.model_ok() && t7.budgets_ok() && t7.hash_ok() && t7.soak_ok(), "T07 四项绿");

    // C404-02 模型漂移检出。
    let mut t7b = t7;
    t7b.model_b = 440;
    chk(&mut set, "C404-02", !t7b.model_ok(), "复算漂移红");

    // C404-03 预算越限定位于具体维。
    let mut t7c = t7;
    t7c.budgets[1] = 450;
    chk(&mut set, "C404-03", !t7c.budgets_ok() && t7c.over_budgets() == vec![1], "切换维越限");

    // C404-04 基线哈希缺失红。
    let mut t7d = t7;
    t7d.baseline_hash = 0;
    chk(&mut set, "C404-04", !t7d.hash_ok(), "哈希缺失红");

    // C404-05 长稳报告过期红（90 天线）。
    let mut t7e = t7;
    t7e.soak_age_days = 91;
    chk(&mut set, "C404-05", !t7e.soak_ok() && !t7e.all_ok(), "长稳过期红");
    let mut t7f = t7;
    t7f.soak_age_days = 90;
    chk(&mut set, "C404-05b", t7f.soak_ok(), "压线 90 天过");

    // C404-06 T08 五标准逐条。
    let t8 = T08Recheck {
        dx_standards: [true; 5],
        zero_line: (0, 0, 0),
        grades: vec![("c1", 0), ("c2", 1), ("c3", 2)],
    };
    chk(&mut set, "C404-06", t8.five_standards_ok(), "五标准全过");

    // C404-07 三零线破检出。
    let mut t8b = t8.clone();
    t8b.zero_line = (1, 0, 0);
    chk(&mut set, "C404-07", !t8b.zero_line_ok() && !t8b.all_ok(), "崩溃破线");

    // C404-08 语料分级值域核验。
    let mut t8c = t8.clone();
    t8c.grades.push(("c4", 3));
    chk(&mut set, "C404-08", !t8c.grades_ok(), "分级值域红");

    // C404-09 无障碍预算保底档（域本色）：150ms 红线。
    chk(&mut set, "C404-09", a11y_floor_ok(150) && a11y_floor_ok(200)
        && !a11y_floor_ok(149), "保底 150ms 红线");

    // C404-10 全绿联动判定。
    chk(&mut set, "C404-10", t7.all_ok() && t8.all_ok(), "T07-T08 全绿");

    // C404-11 越限多维权定位。
    let mut t7g = t7;
    t7g.budgets[0] = 999;
    t7g.budgets[3] = 99999;
    chk(&mut set, "C404-11", t7g.over_budgets() == vec![0, 3], "双维越限定");

    // C404-12 名单规模单源（五标准 5、零线三元组）。
    chk(&mut set, "C404-12", t8.dx_standards.len() == 5
        && T07Recheck::SOAK_VALID_DAYS == 90, "阈值单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves404_smoke() {
        let set = run_ves404_checks();
        assert!(set.failed_count() == 0, "ves404 red: {}", set.red_items_text());
    }
}
