//! UNX-F56169 归因结论-缺陷账转单器（syscall 域）（0x61520 段）
//!
//! 锚点：UNX-F56169-J1 转单账（归因结论 → 缺陷单逐条转单：结论键 ×
//! 缺陷等级 × 转单态 × 单指纹四元组）；J2 转单闸（等级闭集 P0~P3 +
//! 重复结论拒转 + 空结论拒转三闸）；J3 转单印（全部结论已转可印，印
//! 后只读）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·缺陷转单面）。
//!
//! 结构：缺陷等级闭集（P0 致命/P1 严重/P2 一般/P3 提示）+ 转单器（结
//! 论键唯一索引）+ 转单账（逐单落账）+ 转单印 + 注入核（重复转单/空
//! 结论/未知等级/印后转/未转完印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61520 独占 0x20）。
pub const SEG_BASE: u32 = 0x61520;
pub const TC_DUP_KEY: u32 = SEG_BASE + 0x01; // 重复结论转单
pub const TC_EMPTY_KEY: u32 = SEG_BASE + 0x02; // 空结论
pub const TC_UNK_LEVEL: u32 = SEG_BASE + 0x03; // 未知等级
pub const TC_SEALED: u32 = SEG_BASE + 0x04; // 印后转
pub const TC_OPEN_SEAL: u32 = SEG_BASE + 0x05; // 未转完印

/// 缺陷等级闭集。
pub const L_P0: usize = 0; // 致命
pub const L_P1: usize = 1; // 严重
pub const L_P2: usize = 2; // 一般
pub const L_P3: usize = 3; // 提示
pub const LEVEL_COUNT: usize = 4;

/// 等级名渲染。
pub const LEVEL_NAMES: [&str; LEVEL_COUNT] = ["P0", "P1", "P2", "P3"];

/// 单指纹：FNV（结论键 × 等级）。
pub fn ticket_fp(key: &str, level: usize) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in key.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h = h.wrapping_mul(0x0100_0193) ^ level as u32;
    h
}

/// 单条转单账。
#[derive(Clone)]
pub struct TicketEntry {
    pub key: String,
    pub level: usize,
    pub fp: u32,
    pub state: &'static str,
}

impl TicketEntry {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|{:08x}|{}",
            self.key, LEVEL_NAMES[self.level], self.fp, self.state
        )
    }
}

/// 转单器。
pub struct TicketConverter {
    entries: Vec<TicketEntry>,
    pending_conclusions: Vec<String>,
    sealed: bool,
}

impl TicketConverter {
    pub fn new(conclusions: Vec<String>) -> Self {
        TicketConverter {
            entries: Vec::new(),
            pending_conclusions: conclusions,
            sealed: false,
        }
    }

    /// 转一单：空结论拒 + 未知等级拒 + 重复拒 + 未登记结论拒 + 印后拒五闸。
    pub fn convert(&mut self, key: &str, level: usize) -> Result<u32, u32> {
        if self.sealed {
            return Err(TC_SEALED);
        }
        if key.is_empty() {
            return Err(TC_EMPTY_KEY);
        }
        if level >= LEVEL_COUNT {
            return Err(TC_UNK_LEVEL);
        }
        if self.entries.iter().any(|e| e.key == key) {
            return Err(TC_DUP_KEY);
        }
        if !self.pending_conclusions.iter().any(|c| c == key) {
            return Err(TC_EMPTY_KEY);
        }
        let fp = ticket_fp(key, level);
        self.entries.push(TicketEntry {
            key: key.to_string(),
            level,
            fp,
            state: "TICKETED",
        });
        self.pending_conclusions.retain(|c| c != key);
        Ok(fp)
    }

    pub fn entry(&self, key: &str) -> Option<&TicketEntry> {
        self.entries.iter().find(|e| e.key == key)
    }

    pub fn converted_n(&self) -> usize {
        self.entries.len()
    }

    pub fn pending_n(&self) -> usize {
        self.pending_conclusions.len()
    }

    /// 等级分布统计。
    pub fn level_dist(&self) -> [usize; LEVEL_COUNT] {
        let mut d = [0; LEVEL_COUNT];
        for e in &self.entries {
            d[e.level] += 1;
        }
        d
    }

    /// 转单印：待转清零才可印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.pending_conclusions.is_empty() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        out.push_str(&format!("PENDING:{}\n", self.pending_n()));
        if self.sealed {
            out.push_str("TICKET-SEAL\n");
        }
        out
    }
}

/// 标准夹具：四结论全转（P0/P1/P2/P3 各一）。
fn standard_converter() -> TicketConverter {
    let mut c = TicketConverter::new(
        ["entry-handle-bottleneck", "sched-starve-risk", "tail-spike", "doc-drift"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    );
    let _ = c.convert("entry-handle-bottleneck", L_P1);
    let _ = c.convert("sched-starve-risk", L_P0);
    let _ = c.convert("tail-spike", L_P2);
    let _ = c.convert("doc-drift", L_P3);
    c
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：空结论
    let mut c = TicketConverter::new(alloc::vec!["a".to_string()]);
    if c.convert("", L_P1) == Err(TC_EMPTY_KEY) {
        caught += 1;
    }
    // 注 2：未知等级
    let mut c = TicketConverter::new(alloc::vec!["a".to_string()]);
    if c.convert("a", 9) == Err(TC_UNK_LEVEL) {
        caught += 1;
    }
    // 注 3：未登记结论
    let mut c = TicketConverter::new(alloc::vec!["a".to_string()]);
    if c.convert("ghost", L_P1) == Err(TC_EMPTY_KEY) {
        caught += 1;
    }
    // 注 4：重复转单
    let mut c = standard_converter();
    if c.convert("entry-handle-bottleneck", L_P2) == Err(TC_DUP_KEY) {
        caught += 1;
    }
    // 注 5：未转完印
    let mut c = TicketConverter::new(alloc::vec!["a".to_string(), "b".to_string()]);
    let _ = c.convert("a", L_P1);
    if !c.seal() && !c.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 转单账；J2 转单闸；J3 转单印。
pub fn run_unx56169_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56169");
    // J1-1 四单全转
    let c = standard_converter();
    let ok = c.converted_n() == 4 && c.pending_n() == 0;
    s.add("56169-J1-四单全转", ok, if ok { "" } else { "转单账不全" });
    // J1-2 渲染导出齐
    let text = standard_converter().render_book();
    let ok = text.contains("entry-handle-bottleneck|P1|") && text.contains("PENDING:0");
    s.add("56169-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 等级分布（各一）
    let ok = standard_converter().level_dist() == [1, 1, 1, 1];
    s.add("56169-J2-等级分布", ok, if ok { "" } else { "分布漂移" });
    // J2-2 单指纹确定
    let ok = ticket_fp("k", L_P1) == ticket_fp("k", L_P1);
    s.add("56169-J2-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J2-3 等级入签
    let ok = ticket_fp("k", L_P1) != ticket_fp("k", L_P2);
    s.add("56169-J2-等级入签", ok, if ok { "" } else { "等级穿透" });
    // J2-4 键入签
    let ok = ticket_fp("k1", L_P1) != ticket_fp("k2", L_P1);
    s.add("56169-J2-键入签", ok, if ok { "" } else { "键穿透" });
    // J2-5 等级名闭集
    let ok = LEVEL_NAMES[L_P0] == "P0" && LEVEL_NAMES[L_P3] == "P3";
    s.add("56169-J2-等级名闭集", ok, if ok { "" } else { "名漂移" });
    // J3-1 转单印
    let mut c = standard_converter();
    let ok = c.seal() && c.sealed();
    s.add("56169-J3-转单印", ok, if ok { "" } else { "转单印失败" });
    // J3-2 印后转拒
    let ok = c.convert("x", L_P1) == Err(TC_SEALED);
    s.add("56169-J3-印后转拒", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let ok = c.render_book().contains("TICKET-SEAL");
    s.add("56169-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 单态渲染 TICKETED
    let ok = standard_converter().entry("tail-spike").map(|e| e.state == "TICKETED").unwrap_or(false);
    s.add("56169-J3-单态渲染", ok, if ok { "" } else { "态渲染漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56169-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56169_all_checks_green() {
        let set = run_unx56169_checks();
        assert!(set.all_passed(), "UNX-F56169 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56169_fp_stable() {
        assert_eq!(ticket_fp("k", L_P1), ticket_fp("k", L_P1));
    }
}
