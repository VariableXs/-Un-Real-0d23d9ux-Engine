//! F421 输入法指示器点击 · 完整设计（STAR I 主册 G-I-21）。
//!
//! **判据（主册）**：循环顺序=F373 设置序；右键直选；徽标形态（含双拼
//! 角标）；三处同步（复用 F327 判据）；点击响应 <100ms。＋通12。
//!
//! 设计：语言指示核——布局循环按 F373 设置序（注入）；右键直选；徽标
//! 形态枚举（中/EN/双拼角标）；点击响应预算记账 <100ms；三处同步 =
//! 循环后「任务栏/候选窗/设置页」三面读同值（同源 state 的账面对拍）。
//!
//! v5 纵深：F373 设置序热更新（当前布局保持选中）；禁用布局循环跳过
//! （全禁原地不动——不假装切换）；连点防抖（300ms 内第二击不算循环）；
//! IME 组合期点击挂起（不打断输入）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 点击响应判线（ms）。
pub const CLICK_BUDGET_MS: u64 = 100;

/// 连点防抖窗（ms）——窗内第二击不算循环。
pub const CLICK_DEBOUNCE_MS: u64 = 300;

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
    /// 禁用布局（循环跳过；全禁 → 原地不动，不假装切换）。
    pub disabled: Vec<&'static str>,
    /// 连点防抖账。
    pub debounced_clicks: u64,
    last_click_at_ms: Option<u64>,
    /// IME 组合期（点击挂起——组合中不打断输入）。
    pub composing: bool,
    pub held_while_composing: u64,
}

impl ImeIndicator {
    pub fn new(order: Vec<&'static str>, shuangpin_layout: Option<&'static str>) -> ImeIndicator {
        ImeIndicator {
            order,
            current: 0,
            shuangpin_layout,
            last_click_ms: None,
            over_budget: 0,
            disabled: Vec::new(),
            debounced_clicks: 0,
            last_click_at_ms: None,
            composing: false,
            held_while_composing: 0,
        }
    }

    fn is_disabled(&self, name: &str) -> bool {
        self.disabled.iter().any(|d| *d == name)
    }

    fn enabled_count(&self) -> usize {
        self.order.iter().filter(|l| !self.is_disabled(l)).count()
    }

    /// 点击循环：顺序 = F373 设置序（环回；跳过禁用项；组合期挂起）。
    pub fn click_cycle(&mut self, latency_ms: u64) -> &str {
        self.last_click_ms = Some(latency_ms);
        if latency_ms > CLICK_BUDGET_MS {
            self.over_budget += 1;
        }
        if self.composing {
            self.held_while_composing += 1;
            return self.current_layout(); // 组合期挂起——不打断输入
        }
        if !self.order.is_empty() && self.enabled_count() > 0 {
            loop {
                self.current = (self.current + 1) % self.order.len();
                let cur = self.current_layout();
                if !self.is_disabled(cur) {
                    break;
                }
            }
        }
        self.current_layout()
    }

    /// 连点防抖循环：CLICK_DEBOUNCE_MS 内的重复点击不算循环切换。
    pub fn click_debounced(&mut self, latency_ms: u64, now_ms: u64) -> bool {
        if let Some(t) = self.last_click_at_ms {
            if now_ms.saturating_sub(t) < CLICK_DEBOUNCE_MS {
                self.debounced_clicks += 1;
                return false;
            }
        }
        self.last_click_at_ms = Some(now_ms);
        let _ = self.click_cycle(latency_ms);
        true
    }

    /// F373 设置序热更新：当前布局在新序中保持选中（换名单不断人；
    /// 当前布局被移除 → 落新序首项）。
    pub fn set_order(&mut self, order: Vec<&'static str>) {
        let cur = self.order.get(self.current).copied().unwrap_or("");
        self.order = order;
        self.current = self.order.iter().position(|l| *l == cur).unwrap_or(0);
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
    // v5：设置序热更新——当前布局保持选中。
    let mut j = ImeIndicator::new(alloc::vec!["zh-pinyin", "en-us"], None);
    let _ = j.right_pick("en-us", 50);
    j.set_order(alloc::vec!["en-us", "zh-pinyin", "zh-shuangpin"]);
    set.add("f421-order-hotswap-keeps", j.current_layout() == "en-us", "");
    // v5：禁用布局循环跳过；全禁原地不动（诚实——不假装切走了）。
    let mut d = ImeIndicator::new(alloc::vec!["zh-pinyin", "zh-shuangpin", "en-us"], None);
    d.disabled = alloc::vec!["zh-shuangpin"];
    set.add("f421-cycle-skips-disabled", d.click_cycle(50) == "en-us", "");
    d.disabled = alloc::vec!["zh-pinyin", "zh-shuangpin", "en-us"];
    set.add(
        "f421-all-disabled-holds",
        d.click_cycle(50) == "en-us" && d.current_layout() == "en-us",
        "",
    );
    // v5：连点防抖——300ms 内第二击不算循环。
    let mut b = ImeIndicator::new(alloc::vec!["zh-pinyin", "en-us"], None);
    set.add("f421-debounce-first", b.click_debounced(40, 0), "");
    set.add(
        "f421-debounce-hold",
        !b.click_debounced(40, 100)
            && b.debounced_clicks == 1
            && b.current_layout() == "en-us",
        "",
    );
    set.add(
        "f421-debounce-release",
        b.click_debounced(40, 400) && b.current_layout() == "zh-pinyin",
        "",
    );
    // v5：组合期挂起——点击不切（输入优先）。
    b.composing = true;
    set.add(
        "f421-composing-holds",
        b.click_cycle(40) == "zh-pinyin" && b.held_while_composing == 1,
        "",
    );
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

    #[test]
    fn order_hotswap_removed_layout_falls_to_first() {
        let mut i = ImeIndicator::new(alloc::vec!["zh-pinyin", "en-us"], None);
        let _ = i.right_pick("en-us", 50);
        i.set_order(alloc::vec!["zh-shuangpin", "zh-pinyin"]);
        assert_eq!(i.current_layout(), "zh-shuangpin", "当前布局被移除 → 落首项");
    }

    #[test]
    fn debounce_boundary_exclusive() {
        let mut i = ImeIndicator::new(alloc::vec!["a", "b"], None);
        assert!(i.click_debounced(10, 0));
        assert!(!i.click_debounced(10, CLICK_DEBOUNCE_MS - 1), "窗沿内 → 吞");
        assert!(i.click_debounced(10, CLICK_DEBOUNCE_MS), "恰在窗沿 → 放行");
        assert_eq!(i.debounced_clicks, 1);
    }
}
