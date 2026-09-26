//! F421 输入法指示器点击 · 完整设计（STAR I 主册 G-I-21）。
//!
//! **判据（主册）**：循环顺序=F373 设置序；右键直选；徽标形态（含双拼
//! 角标）；三处同步（复用 F327 判据）；点击响应 <100ms。＋通12。
//!
//! 设计：语言指示核——布局循环按 F373 设置序（注入）；右键直选；徽标
//! 形态枚举（中/EN/双拼角标）；点击响应预算记账 <100ms；三处同步 =
//! 循环后「任务栏/候选窗/设置页」三面读同值（同源 state 的账面对拍）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 点击响应判线（ms）。
pub const CLICK_BUDGET_MS: u64 = 100;

/// 徽标形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Badge {
    /// 中文（「中」字块）。
    Zh,
    /// 英文（EN 块）。
    En,
    /// 双拼方案（中块 + 小角标）。
    ZhShuangpin,
}

/// 语言指示核。
pub struct ImeIndicator {
    /// F373 设置序（注入——唯一循环顺序来源）。
    pub order: Vec<&'static str>,
    pub current: usize,
    /// 双拼方案名（当前布局命中时徽标加角标；None = 无）。
    pub shuangpin_layout: Option<&'static str>,
    pub last_click_ms: Option<u64>,
    pub over_budget: u64,
}

impl ImeIndicator {
    pub fn new(order: Vec<&'static str>, shuangpin_layout: Option<&'static str>) -> ImeIndicator {
        ImeIndicator { order, current: 0, shuangpin_layout, last_click_ms: None, over_budget: 0 }
    }

    /// 点击循环：顺序 = F373 设置序（环回）。
    pub fn click_cycle(&mut self, latency_ms: u64) -> &str {
        self.last_click_ms = Some(latency_ms);
        if latency_ms > CLICK_BUDGET_MS {
            self.over_budget += 1;
        }
        if !self.order.is_empty() {
            self.current = (self.current + 1) % self.order.len();
        }
        self.current_layout()
    }

    /// 右键直选：点击列表项直接切到该布局。
    pub fn right_pick(&mut self, name: &str, latency_ms: u64) -> bool {
        match self.order.iter().position(|l| *l == name) {
            Some(i) => {
                self.current = i;
                self.last_click_ms = Some(latency_ms);
                if latency_ms > CLICK_BUDGET_MS {
                    self.over_budget += 1;
                }
                true
            }
            None => false,
        }
    }

    /// 徽标形态（双拼角标判据）。
    pub fn badge(&self) -> Badge {
        let cur = self.order.get(self.current).copied().unwrap_or("");
        if cur.starts_with("en") {
            Badge::En
        } else if self.shuangpin_layout == Some(cur) {
            Badge::ZhShuangpin
        } else {
            Badge::Zh
        }
    }

    /// 三处同步对拍：任务栏徽标/候选窗/设置页三方读数注入（同一真相
    /// ——三方值必须一致，否则判据红）。
    pub fn three_way_sync(&self, candidate_view: &str, settings_view: &str) -> bool {
        let cur = self.order.get(self.current).copied().unwrap_or("");
        cur == candidate_view && cur == settings_view
    }

    pub fn current_layout(&self) -> &str {
        self.order.get(self.current).copied().unwrap_or("")
    }
}

pub fn run_imeind_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F421");
    let mut i = ImeIndicator::new(
        alloc::vec!["zh-pinyin", "zh-shuangpin", "en-us"],
        Some("zh-shuangpin"),
    );
    // 循环顺序 = 设置序（zh-pinyin → zh-shuangpin → en-us → 回环）。
    set.add("f421-cycle-1", i.click_cycle(60) == "zh-shuangpin", "");
    set.add("f421-cycle-2", i.click_cycle(60) == "en-us", "");
    set.add("f421-cycle-wrap", i.click_cycle(60) == "zh-pinyin", "");
    set.add("f421-under-100ms", i.over_budget == 0, "");
    // 右键直选。
    set.add(
        "f421-right-pick",
        i.right_pick("en-us", 80) && i.current_layout() == "en-us",
        "",
    );
    set.add("f421-right-pick-miss", !i.right_pick("fr", 80), "");
    // 徽标形态三态。
    set.add("f421-badge-en", i.badge() == Badge::En, "");
    let _ = i.right_pick("zh-shuangpin", 60);
    set.add("f421-badge-shuangpin", i.badge() == Badge::ZhShuangpin, "");
    let _ = i.right_pick("zh-pinyin", 60);
    set.add("f421-badge-zh", i.badge() == Badge::Zh, "");
    // 三处同步。
    set.add(
        "f421-three-way-sync",
        i.three_way_sync("zh-pinyin", "zh-pinyin") && !i.three_way_sync("zh-pinyin", "en-us"),
        "",
    );
    // 超预算诚实记账。
    let _ = i.click_cycle(150);
    set.add("f421-over-budget-logged", i.over_budget == 1, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_order_safe() {
        let mut i = ImeIndicator::new(alloc::vec![], None);
        assert_eq!(i.click_cycle(50), "");
        assert_eq!(i.badge(), Badge::Zh, "空序回退中块（不 panic）");
    }

    #[test]
    fn single_layout_cycle_self() {
        let mut i = ImeIndicator::new(alloc::vec!["zh-pinyin"], None);
        assert_eq!(i.click_cycle(50), "zh-pinyin");
        assert_eq!(i.click_cycle(50), "zh-pinyin");
    }
}
