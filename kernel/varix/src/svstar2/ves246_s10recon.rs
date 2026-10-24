//! VE-F3992 · S 域清账确认
//!
//! 确认：S 域清账——终审项逐条对账（F3981-F3991 终审回执齐备核验 +
//! 遗留项清单处置（销项 / 转 T 域 / 登记缓办三路））+ 账实相符声明。
//! 数据结构：清账册（回执+遗留）；三路处置表。
//! 错误路径：回执缺→催办；遗留未处置→禁收官；账实不符→重盘。
//! 性能：对账 O(项)；处置 O(遗留)；催办 O(1)。
//! 跨批对接：F3981-F3991 全前置；F3993 评分前提；F3999 归档联动。
//! 无障碍与隐私：遗留清单含无障碍专项路（转 T 域国际化无障碍适配）；
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x45C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x45C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T12CCode(pub u16);

impl T12CCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const RECEIPT_LATE: Self = Self(SEG_BASE + 1);
    pub const LEFTOVER_OPEN: Self = Self(SEG_BASE + 2);
    pub const BOOK_DRIFT: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::RECEIPT_LATE => "回执缺→催办",
            Self::LEFTOVER_OPEN => "遗留未处置→禁收官",
            Self::BOOK_DRIFT => "账实不符→重盘",
            _ => "未知",
        }
    }
}

// ── 终审回执对账（F3981-F3991 共 11 项）─────────────────────────────

pub const FINAL_ITEMS: [&str; 11] = [
    "F3981-接口总账", "F3982-词典", "F3983-联测", "F3984-性能", "F3985-安全",
    "F3986-无障碍", "F3987-观测", "F3988-fuzz", "F3989-文档schema", "F3990-冻结",
    "F3991-扩展",
];

/// 清账册：回执齐备核验。
#[derive(Debug, Clone)]
pub struct ReconBook {
    /// 已收回执。
    pub receipts: Vec<&'static str>,
}

impl ReconBook {
    pub fn new() -> Self {
        Self { receipts: Vec::new() }
    }

    pub fn receive(&mut self, item: &'static str) -> bool {
        if !FINAL_ITEMS.contains(&item) || self.receipts.contains(&item) {
            return false;
        }
        self.receipts.push(item);
        true
    }

    /// 催办清单（未回执项）。
    pub fn urgent_list(&self) -> Vec<&'static str> {
        FINAL_ITEMS.iter().filter(|i| !self.receipts.contains(i)).map(|i| *i).collect()
    }

    /// 回执齐备。
    pub fn all_received(&self) -> bool {
        self.urgent_list().is_empty()
    }
}

// ── 遗留项三路处置 ───────────────────────────────────────────────────

/// 处置三路。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// 销项（本域内关闭）。
    Close,
    /// 转 T 域（含无障碍专项路——国际化无障碍适配）。
    ToTDomain,
    /// 登记缓办。
    Defer,
}

/// 遗留项。
#[derive(Debug, Clone)]
pub struct Leftover {
    /// 项名。
    pub name: String,
    /// 处置（None = 未处置）。
    pub disp: Option<Disposition>,
    /// 无障碍专项标记（转 T 域时必须）。
    pub a11y_special: bool,
}

/// 三路处置表。
#[derive(Debug, Clone)]
pub struct DispositionBook {
    pub items: Vec<Leftover>,
}

impl DispositionBook {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn put(&mut self, name: &str, a11y_special: bool) {
        self.items.push(Leftover {
            name: String::from(name),
            disp: None,
            a11y_special,
        });
    }

    /// 处置登记：无障碍专项项只允许转 T 域路。
    pub fn dispose(&mut self, name: &str, disp: Disposition) -> bool {
        let row = match self.items.iter_mut().find(|i| i.name == name) {
            Some(r) => r,
            None => return false,
        };
        if row.a11y_special && disp != Disposition::ToTDomain {
            return false;
        }
        row.disp = Some(disp);
        true
    }

    /// 未处置清单（禁收官依据）。
    pub fn open_items(&self) -> Vec<String> {
        self.items.iter().filter(|i| i.disp.is_none()).map(|i| i.name.clone()).collect()
    }

    /// 三路齐活：零未处置。
    pub fn all_disposed(&self) -> bool {
        self.open_items().is_empty()
    }

    /// 转 T 域清单（交接面）。
    pub fn to_t_domain(&self) -> Vec<String> {
        self.items.iter()
            .filter(|i| i.disp == Some(Disposition::ToTDomain))
            .map(|i| i.name.clone())
            .collect()
    }
}

// ── 账实相符声明 ─────────────────────────────────────────────────────

/// 账面 vs 实盘：两侧清单比对。
pub fn book_matches_reality(book: &[&str], reality: &[&str]) -> bool {
    if book.len() != reality.len() {
        return false;
    }
    book.iter().all(|b| reality.contains(b))
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves246_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves246_s10recon");

    // C3992-01 诊断码段不越窗。
    chk(&mut set, "C3992-01",
        T12CCode::BOOK_DRIFT.0 < SEG_BASE + 16 && SEG_BASE == 0x45C0, "err");

    // C3992-02 终审项 11 条齐备。
    chk(&mut set, "C3992-02",
        FINAL_ITEMS.len() == 11
            && FINAL_ITEMS[0] == "F3981-接口总账"
            && FINAL_ITEMS[10] == "F3991-扩展", "err");

    // C3992-03 回执催办清单。
    let mut r = ReconBook::new();
    for i in FINAL_ITEMS.iter().take(8) {
        r.receive(i);
    }
    chk(&mut set, "C3992-03", r.urgent_list().len() == 3 && !r.all_received(), "err");

    // C3992-04 补齐后回执齐。
    for i in r.urgent_list().clone() {
        r.receive(i);
    }
    chk(&mut set, "C3992-04", r.all_received(), "err");

    // C3992-05 越名回执拒绝 + 重复拒绝。
    let mut r2 = ReconBook::new();
    r2.receive("F3981-接口总账");
    chk(&mut set, "C3992-05",
        !r2.receive("伪项") && !r2.receive("F3981-接口总账"), "err");

    // C3992-06 遗留未处置 → 禁收官。
    let mut d = DispositionBook::new();
    d.put("高对比残项", false);
    d.put("朗读语料多语言", true);
    chk(&mut set, "C3992-06",
        d.open_items().len() == 2 && !d.all_disposed(), "err");

    // C3992-07 无障碍专项项禁销项/缓办（只许转 T 域）。
    chk(&mut set, "C3992-07",
        !d.dispose("朗读语料多语言", Disposition::Close)
            && !d.dispose("朗读语料多语言", Disposition::Defer), "err");

    // C3992-08 专项项转 T 域放行。
    chk(&mut set, "C3992-08",
        d.dispose("朗读语料多语言", Disposition::ToTDomain)
            && d.to_t_domain() == vec![String::from("朗读语料多语言")], "err");

    // C3992-09 普通项三路任选。
    chk(&mut set, "C3992-09",
        d.dispose("高对比残项", Disposition::Close) && d.all_disposed(), "err");

    // C3992-10 越名处置拒绝。
    let mut d2 = DispositionBook::new();
    d2.put("甲", false);
    chk(&mut set, "C3992-10", !d2.dispose("乙", Disposition::Close), "err");

    // C3992-11 账实相符：一致过。
    chk(&mut set, "C3992-11",
        book_matches_reality(&["a", "b", "c"], &["c", "a", "b"]), "err");

    // C3992-12 账实不符：缺项 / 多项。
    chk(&mut set, "C3992-12",
        !book_matches_reality(&["a", "b"], &["a", "b", "c"])
            && !book_matches_reality(&["a", "x"], &["a", "b"]), "err");

    // C3992-13 三路枚举互异。
    chk(&mut set, "C3992-13",
        Disposition::Close != Disposition::ToTDomain
            && Disposition::ToTDomain != Disposition::Defer
            && Disposition::Close != Disposition::Defer, "err");

    // C3992-14 全清账后转 T 域交接面非空（专项路断言）。
    let mut d3 = DispositionBook::new();
    d3.put("色标文化色", true);
    d3.put("布局镜像前置", true);
    for name in ["色标文化色", "布局镜像前置"] {
        d3.dispose(name, Disposition::ToTDomain);
    }
    chk(&mut set, "C3992-14",
        d3.all_disposed() && d3.to_t_domain().len() == 2, "err");

    // C3992-15 缓办路可登记。
    let mut d4 = DispositionBook::new();
    d4.put("文档润色", false);
    d4.dispose("文档润色", Disposition::Defer);
    chk(&mut set, "C3992-15", d4.all_disposed() && d4.to_t_domain().is_empty(), "err");

    // C3992-16 催办语义。
    chk(&mut set, "C3992-16", T12CCode::RECEIPT_LATE.explain().contains("催办"), "err");

    // C3992-17 回执表幂等收齐。
    let r3_count = FINAL_ITEMS.len();
    let mut r3 = ReconBook::new();
    for i in FINAL_ITEMS.iter() {
        r3.receive(i);
    }
    for i in FINAL_ITEMS.iter() {
        r3.receive(i);
    }
    chk(&mut set, "C3992-17", r3.receipts.len() == r3_count && r3.all_received(), "err");

    // C3992-18 遗留表可空（无遗留直接收官）。
    chk(&mut set, "C3992-18", DispositionBook::new().all_disposed(), "err");

    // C3992-19 禁收官码语义。
    chk(&mut set, "C3992-19", T12CCode::LEFTOVER_OPEN.explain().contains("禁收官"), "err");

    // C3992-20 错误码互异。
    let codes = [T12CCode::OK.0, T12CCode::RECEIPT_LATE.0, T12CCode::LEFTOVER_OPEN.0,
        T12CCode::BOOK_DRIFT.0];
    chk(&mut set, "C3992-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipts_gate() {
        let mut r = ReconBook::new();
        assert!(!r.all_received());
        for i in FINAL_ITEMS.iter() {
            r.receive(i);
        }
        assert!(r.all_received());
    }

    #[test]
    fn a11y_special_only_t_domain() {
        let mut d = DispositionBook::new();
        d.put("专项", true);
        assert!(!d.dispose("专项", Disposition::Close));
        assert!(d.dispose("专项", Disposition::ToTDomain));
    }

    #[test]
    fn book_reality_match() {
        assert!(book_matches_reality(&["x"], &["x"]));
        assert!(!book_matches_reality(&["x"], &["y"]));
    }
}
