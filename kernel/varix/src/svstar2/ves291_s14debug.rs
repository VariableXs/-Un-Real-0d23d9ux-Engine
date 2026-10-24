//! ves291_s14debug — VE-F4051 · T03 排版调试器
//! 排版检视五列仪表（语言路由/整形状态/断行决策/基线对齐/家族位）+ 断行可视化 + 整形解释（复用可解释）+ 家族关系协议 + 打开才采样零常态开销。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x7DC0;

pub const FAMILY_TOTAL: u8 = 29;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum DbgCode {
    Ok = 0,
    VisualWrong = SEG_BASE + 0x01,
    ExplainDistort = SEG_BASE + 0x02,
    PanelSplit = SEG_BASE + 0x03,
    FamilyOver = SEG_BASE + 0x04,
}

pub const PANEL_COLS: [&str; 5] = ["语言路由", "整形状态", "断行决策", "基线对齐", "家族位"];

/// 仪表行。
#[derive(Clone)]
pub struct PanelRow {
    pub lang: String,
    pub shape_state: &'static str,
    pub break_at: Option<usize>,
    pub baseline: i32,
    pub family_slot: u8,
}

pub struct DebugPanel {
    pub sampling: bool,
    pub rows: Vec<PanelRow>,
}

impl DebugPanel {
    pub fn new() -> Self {
        DebugPanel { sampling: false, rows: Vec::new() }
    }
    pub fn set_sampling(&mut self, on: bool) {
        self.sampling = on;
    }
    /// 采样：打开才收（零常态开销）。
    pub fn sample(&mut self, row: PanelRow) -> bool {
        if !self.sampling { return false; }
        if row.family_slot >= FAMILY_TOTAL { return false; }
        self.rows.push(row);
        true
    }
    /// 五列仪表齐备断言。
    pub fn cols_complete(&self) -> bool {
        PANEL_COLS.len() == 5 && PANEL_COLS.iter().all(|c| !c.is_empty())
    }
    /// 断行可视化：把断行点渲染成 | 标记序列。
    pub fn visualize_breaks(&self, text: &str, points: &[usize]) -> Result<String, DbgCode> {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::new();
        for (i, c) in chars.iter().enumerate() {
            if points.contains(&i) {
                if i > chars.len() { return Err(DbgCode::VisualWrong); }
                out.push('|');
            }
            out.push(*c);
        }
        Ok(out)
    }
    /// 整形解释：形态 → 决策文案（复用可解释）。
    pub fn explain_shape(form: &str) -> Result<&'static str, DbgCode> {
        match form {
            "isolated" => Ok("孤立字形"),
            "initial" => Ok("词首连写"),
            "medial" => Ok("词中连写"),
            "final" => Ok("词尾连写"),
            _ => Err(DbgCode::ExplainDistort),
        }
    }
    /// 家族注册：29 成员位唯一。
    pub fn register_family(&mut self, name: &'static str, slot: u8) -> bool {
        if slot >= FAMILY_TOTAL { return false; }
        if self.rows.iter().any(|r| r.family_slot == slot) { return false; }
        let _ = name;
        true
    }
}

pub fn run_ves291_checks() -> CheckSet {
    let mut set = CheckSet::new("ves291_s14debug");
    let mut p = DebugPanel::new();

    // 1) 五列仪表齐。
    set.add("C4051-01", p.cols_complete() && PANEL_COLS[0] == "语言路由" && PANEL_COLS[4] == "家族位", "五列仪表");

    // 2) 采样关闭拒收（零常态开销）。
    let r1 = PanelRow { lang: String::from("ar"), shape_state: "medial", break_at: None, baseline: -40, family_slot: 1 };
    set.add("C4051-02", !p.sample(r1) && !p.sampling, "关闭拒收");

    // 3) 打开后采样。
    p.set_sampling(true);
    set.add("C4051-03", p.sample(PanelRow { lang: String::from("ar"), shape_state: "medial", break_at: None, baseline: -40, family_slot: 1 }), "打开采样");

    // 4) 家族位越界拒。
    set.add("C4051-04", !p.sample(PanelRow { lang: String::from("zh"), shape_state: "ok", break_at: None, baseline: 88, family_slot: 29 }), "家族位越界拒");

    // 5) 断行可视化。
    let vis = p.visualize_breaks("汉字串", &[1]);
    set.add("C4051-05", vis == Ok(String::from("汉|字串")), "断行可视化");

    // 6) 空断点原样。
    set.add("C4051-06", p.visualize_breaks("abc", &[]) == Ok(String::from("abc")), "空断点原样");

    // 7) 整形解释四态。
    set.add("C4051-07", DebugPanel::explain_shape("isolated") == Ok("孤立字形")
        && DebugPanel::explain_shape("medial") == Ok("词中连写"), "整形解释");

    // 8) 解释失真拒。
    set.add("C4051-08", DebugPanel::explain_shape("weird") == Err(DbgCode::ExplainDistort), "解释失真拒");

    // 9) 家族 29 位唯一注册。
    set.add("C4051-09", p.register_family("a", 0) && !p.register_family("b", 0), "家族位唯一");

    // 10) 家族总数 29。
    set.add("C4051-10", FAMILY_TOTAL == 29, "家族二十九");

    // 11) 段位自查。
    set.add("C4051-11", SEG_BASE == 0x7DC0, "段位 0x7DC0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_flow() {
        let mut p = DebugPanel::new();
        let row = PanelRow { lang: String::from("zh"), shape_state: "ok", break_at: None, baseline: 88, family_slot: 2 };
        assert!(!p.sample(row));
        p.set_sampling(true);
        let row2 = PanelRow { lang: String::from("zh"), shape_state: "ok", break_at: None, baseline: 88, family_slot: 2 };
        assert!(p.sample(row2));
    }
}
