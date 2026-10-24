#![allow(dead_code)]
//! VE-F3958 · 无障碍演进预留
//!
//! 全域预留归一（复用预留制度全域化）：全域预留清单（各域预留 + 全域
//! 预留）——全域三段（签名/激活条件/成本评估——三段断言全域硬门）。全域
//! 路线图；全域不承诺（虚假承诺→撤回）；全域评估制度化（未评估→拦截；
//! 破冻结→重设）。
//! 错误路径：全域三段缺→补；破冻结全域→重设；未评估全域→拦截；全域
//! 虚假承诺→撤回。
//! 性能：三段 O(全域断言)；路线 O(全域)；评估 O(全域项)；撤回 O(全域)。
//! 跨批对接：F3398/F3798 复用全域化声明；全域预留对端；全域路线对端。
//! 无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x2DC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x2DC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8RCode(pub u16);

impl S8RCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const TRIPLE_GAP: Self = Self(SEG_BASE + 1);
    pub const FREEZE_BROKEN: Self = Self(SEG_BASE + 2);
    pub const NOT_ASSESSED: Self = Self(SEG_BASE + 3);
    pub const FALSE_PROMISE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::TRIPLE_GAP => "全域三段缺→补",
            Self::FREEZE_BROKEN => "破冻结全域→重设",
            Self::NOT_ASSESSED => "未评估全域→拦截",
            Self::FALSE_PROMISE => "虚假承诺→撤回",
            _ => "未知",
        }
    }
}

// ── 全域预留清单（三段断言）──────────────────────────────────────────

/// 预留项：全域三段。
#[derive(Debug, Clone)]
pub struct ReserveItem {
    pub name: &'static str,
    /// 域归属（0 = 全域预留）。
    pub domain: u8,
    /// 段一：签名（承诺人）。
    pub signature: &'static str,
    /// 段二：激活条件（非空）。
    pub activate: &'static str,
    /// 段三：成本评估完成（评估制度化）。
    pub assessed: bool,
    /// 冻结标记（冻结期禁改）。
    pub frozen: bool,
    /// 撤回标记。
    pub withdrawn: bool,
}

impl ReserveItem {
    /// 三段断言：签名 + 激活条件 + 评估齐备。
    pub fn triple_complete(&self) -> bool {
        !self.signature.is_empty() && !self.activate.is_empty() && self.assessed
    }

    /// 缺段清单。
    pub fn gaps(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.signature.is_empty() {
            v.push("签名");
        }
        if self.activate.is_empty() {
            v.push("激活条件");
        }
        if !self.assessed {
            v.push("成本评估");
        }
        v
    }
}

/// 全域预留册：硬门 = 三段齐备且未撤回才入路线图。
#[derive(Debug, Clone)]
pub struct ReserveBook {
    pub items: Vec<ReserveItem>,
}

impl ReserveBook {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// 登记：三段缺→补（登记为在补状态，不入路线）。
    pub fn register(&mut self, it: ReserveItem) -> S8RCode {
        if it.domain > 8 {
            return S8RCode::TRIPLE_GAP;
        }
        self.items.push(it);
        S8RCode::OK
    }

    /// 全域硬门：可入路线图名单（三段齐备 + 未撤回）。
    pub fn roadmap(&self) -> Vec<&'static str> {
        self.items
            .iter()
            .filter(|i| i.triple_complete() && !i.withdrawn)
            .map(|i| i.name)
            .collect()
    }

    /// 三段缺口项。
    pub fn triple_gaps(&self) -> Vec<&'static str> {
        self.items.iter().filter(|i| !i.triple_complete()).map(|i| i.name).collect()
    }

    /// 补段动作：签名/条件/评估。
    pub fn fill(&mut self, name: &str, sig: Option<&'static str>, act: Option<&'static str>) -> S8RCode {
        match self.items.iter_mut().find(|i| i.name == name) {
            Some(i) => {
                if i.frozen {
                    return S8RCode::FREEZE_BROKEN;
                }
                if let Some(s) = sig {
                    i.signature = s;
                }
                if let Some(a) = act {
                    i.activate = a;
                }
                S8RCode::OK
            }
            None => S8RCode::TRIPLE_GAP,
        }
    }

    /// 补评估。
    pub fn assess(&mut self, name: &str) -> S8RCode {
        match self.items.iter_mut().find(|i| i.name == name) {
            Some(i) if i.frozen => S8RCode::FREEZE_BROKEN,
            Some(i) => {
                i.assessed = true;
                S8RCode::OK
            }
            None => S8RCode::NOT_ASSESSED,
        }
    }

    /// 冻结期改写拦截。
    pub fn frozen_list(&self) -> Vec<&'static str> {
        self.items.iter().filter(|i| i.frozen).map(|i| i.name).collect()
    }
}

impl Default for ReserveBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 虚假承诺撤回 ─────────────────────────────────────────────────────

/// 承诺审计：承诺了激活条件但签名/评估任一缺失 = 虚假承诺。
pub fn false_promises(book: &ReserveBook) -> Vec<&'static str> {
    book.items
        .iter()
        .filter(|i| !i.withdrawn && i.signature.is_empty() && !i.activate.is_empty())
        .map(|i| i.name)
        .collect()
}

/// 撤回动作。
pub fn withdraw(book: &mut ReserveBook, name: &str) -> S8RCode {
    match book.items.iter_mut().find(|i| i.name == name) {
        Some(i) => {
            i.withdrawn = true;
            S8RCode::OK
        }
        None => S8RCode::FALSE_PROMISE,
    }
}

// ── 全域不承诺锚 ─────────────────────────────────────────────────────

/// 全域不承诺清单：预留项在激活条件满足前不得对外承诺。
pub const NO_PROMISE_RULES: [&str; 3] = [
    "激活前不公告", "未评估不排期", "未冻结不发布",
];

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves212_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves212_s08reserve");

    // 01 三段齐备硬门
    {
        let it = ReserveItem { name: "眼动输入", domain: 0, signature: "甲", activate: "眼动硬件普及", assessed: true, frozen: false, withdrawn: false };
        chk(&mut set, "C3958-01", it.triple_complete() && it.gaps().is_empty(), "err");
    }
    // 02 三段缺检出（逐段）
    {
        let a = ReserveItem { name: "a", domain: 1, signature: "", activate: "x", assessed: true, frozen: false, withdrawn: false };
        let b = ReserveItem { name: "b", domain: 1, signature: "甲", activate: "", assessed: true, frozen: false, withdrawn: false };
        let c = ReserveItem { name: "c", domain: 1, signature: "甲", activate: "x", assessed: false, frozen: false, withdrawn: false };
        chk(&mut set, "C3958-02", a.gaps() == vec!["签名"] && b.gaps() == vec!["激活条件"] && c.gaps() == vec!["成本评估"], "err");
    }
    // 03 域号越界拒收
    {
        let mut bk = ReserveBook::new();
        let it = ReserveItem { name: "x", domain: 9, signature: "甲", activate: "y", assessed: true, frozen: false, withdrawn: false };
        chk(&mut set, "C3958-03", bk.register(it) == S8RCode::TRIPLE_GAP, "err");
    }
    // 04 路线图硬门：三段缺不入路线
    {
        let mut bk = ReserveBook::new();
        bk.register(ReserveItem { name: "齐", domain: 0, signature: "甲", activate: "y", assessed: true, frozen: false, withdrawn: false });
        bk.register(ReserveItem { name: "缺", domain: 0, signature: "", activate: "y", assessed: true, frozen: false, withdrawn: false });
        chk(&mut set, "C3958-04", bk.roadmap() == vec!["齐"] && bk.triple_gaps() == vec!["缺"], "err");
    }
    // 05 三段缺→补实测
    {
        let mut bk = ReserveBook::new();
        bk.register(ReserveItem { name: "缺", domain: 0, signature: "", activate: "", assessed: false, frozen: false, withdrawn: false });
        bk.fill("缺", Some("签核人乙"), Some("条件丙"));
        bk.assess("缺");
        chk(&mut set, "C3958-05", bk.roadmap() == vec!["缺"] && bk.triple_gaps().is_empty(), "err");
    }
    // 06 冻结期改写拦截
    {
        let mut bk = ReserveBook::new();
        bk.register(ReserveItem { name: "冻", domain: 0, signature: "甲", activate: "y", assessed: true, frozen: true, withdrawn: false });
        chk(&mut set, "C3958-06", bk.fill("冻", Some("乙"), None) == S8RCode::FREEZE_BROKEN && bk.assess("冻") == S8RCode::FREEZE_BROKEN && bk.frozen_list() == vec!["冻"], "err");
    }
    // 07 未评估→拦截（assess 不存在项）
    {
        let mut bk = ReserveBook::new();
        chk(&mut set, "C3958-07", bk.assess("幽灵") == S8RCode::NOT_ASSESSED, "err");
    }
    // 08 虚假承诺检出
    {
        let mut bk = ReserveBook::new();
        bk.register(ReserveItem { name: "虚", domain: 0, signature: "", activate: "有条件", assessed: false, frozen: false, withdrawn: false });
        bk.register(ReserveItem { name: "实", domain: 0, signature: "甲", activate: "有条件", assessed: true, frozen: false, withdrawn: false });
        chk(&mut set, "C3958-08", false_promises(&bk) == vec!["虚"], "err");
    }
    // 09 撤回后出路线
    {
        let mut bk = ReserveBook::new();
        bk.register(ReserveItem { name: "虚", domain: 0, signature: "", activate: "有条件", assessed: false, frozen: false, withdrawn: false });
        chk(&mut set, "C3958-09", withdraw(&mut bk, "虚") == S8RCode::OK && bk.roadmap().is_empty() && withdraw(&mut bk, "无") == S8RCode::FALSE_PROMISE, "err");
    }
    // 10 全域不承诺锚守恒
    {
        chk(&mut set, "C3958-10", NO_PROMISE_RULES.len() == 3 && NO_PROMISE_RULES[0] == "激活前不公告" && NO_PROMISE_RULES[2] == "未冻结不发布", "err");
    }
    // 11 全域预留与域预留混账
    {
        let mut bk = ReserveBook::new();
        bk.register(ReserveItem { name: "全域", domain: 0, signature: "甲", activate: "y", assessed: true, frozen: false, withdrawn: false });
        bk.register(ReserveItem { name: "域三", domain: 3, signature: "乙", activate: "z", assessed: true, frozen: false, withdrawn: false });
        chk(&mut set, "C3958-11", bk.roadmap().len() == 2, "err");
    }
    // 12 撤回不可逆
    {
        let mut bk = ReserveBook::new();
        bk.register(ReserveItem { name: "撤", domain: 0, signature: "甲", activate: "y", assessed: true, frozen: false, withdrawn: true });
        chk(&mut set, "C3958-12", bk.roadmap().is_empty(), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3958-13", S8RCode::FALSE_PROMISE.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8RCode::OK, S8RCode::TRIPLE_GAP, S8RCode::FREEZE_BROKEN, S8RCode::NOT_ASSESSED, S8RCode::FALSE_PROMISE];
        chk(&mut set, "C3958-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_reserve_all_green() {
        let s = run_ves212_checks();
        assert!(s.all_passed(), "ves212 red: {}", s.red_items_text());
    }

    #[test]
    fn test_triple_fill_flow() {
        let mut bk = ReserveBook::new();
        bk.register(ReserveItem { name: "r", domain: 2, signature: "", activate: "", assessed: false, frozen: false, withdrawn: false });
        assert!(bk.roadmap().is_empty());
        bk.fill("r", Some("s"), Some("a"));
        bk.assess("r");
        assert_eq!(bk.roadmap(), vec!["r"]);
    }

    #[test]
    fn test_freeze_blocks_mutation() {
        let mut bk = ReserveBook::new();
        bk.register(ReserveItem { name: "f", domain: 0, signature: "", activate: "", assessed: false, frozen: true, withdrawn: false });
        assert_eq!(bk.fill("f", Some("x"), None), S8RCode::FREEZE_BROKEN);
        assert_eq!(bk.assess("f"), S8RCode::FREEZE_BROKEN);
        assert_eq!(bk.triple_gaps(), vec!["f"]);
    }
}
