//! ves406_s22apiinv — VE-F4166 · T 域 API 面终版清点
//! T 域 API 面清点：八组对外接口逐组清点 + 与 T 域架构声明 API 面终版对账；
//! 清点规则（每 API 挂组/挂条/挂语种矩阵三指针 + 版本号 + 稳定性标注）；冗余与缺口双检；
//! 声明与清点不符→修正留痕；缺口→立案补齐；冗余→归并评审；无障碍文本接口专章（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0820;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiCode {
    Ok = 0,
    DeclareDrift = SEG_BASE + 0x01, // 0x0821
    GapFound = SEG_BASE + 0x02,     // 0x0822
    Redundancy = SEG_BASE + 0x03,   // 0x0823
}

/// 稳定性标注。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stability { Stable = 0, Evolving = 1, Deprecated = 2 }

impl Stability {
    pub fn name(self) -> &'static str {
        match self { Stability::Stable => "stable", Stability::Evolving => "evolving", Stability::Deprecated => "deprecated" }
    }
}

/// API 条目：三指针（组/条/语种矩阵）+ 版本 + 稳定性 + a11y 专章标记。
#[derive(Clone, Copy)]
pub struct ApiItem {
    pub name: &'static str,
    pub group: u8,        // 挂组（0-7 → T01-T08）
    pub card: u32,        // 挂条（卡号锚）
    pub lang_matrix: u32, // 挂语种矩阵锚
    pub version: u32,
    pub stability: Stability,
    pub a11y_chapter: bool, // 无障碍文本接口专章（域本色）
}

impl ApiItem {
    pub fn three_pointers(&self) -> bool { self.card != 0 && self.lang_matrix != 0 && self.group < 8 }
}

/// 清点册。
pub struct ApiInventory {
    pub items: Vec<ApiItem>,
    pub fixes: Vec<&'static str>,   // 修正留痕
    pub cases: Vec<&'static str>,   // 立案
    pub reviews: Vec<&'static str>, // 归并评审
}

impl ApiInventory {
    pub fn new() -> Self { ApiInventory { items: Vec::new(), fixes: Vec::new(), cases: Vec::new(), reviews: Vec::new() } }

    pub fn put(&mut self, it: ApiItem) {
        match self.items.iter_mut().find(|x| x.name == it.name) {
            Some(x) => *x = it,
            None => self.items.push(it),
        }
    }
    pub fn get(&self, name: &str) -> Option<&ApiItem> { self.items.iter().find(|x| x.name == name) }

    /// 三指针全齐核验。
    pub fn pointers_ok(&self) -> bool { self.items.iter().all(|x| x.three_pointers()) }

    /// 与架构声明对账：声明名单 vs 清点册面，双向漂移计数。
    pub fn reconcile(&mut self, declared: &[&'static str]) -> usize {
        let mut drifts = 0;
        for d in declared {
            if self.get(d).is_none() { self.fixes.push("declared-missing"); drifts += 1; }
        }
        for it in self.items.iter() {
            if !declared.contains(&it.name) { self.fixes.push("code-extra"); drifts += 1; }
        }
        drifts
    }

    /// 稳定性标注完备：每 API 均有标注。
    pub fn stability_ok(&self) -> bool { !self.items.iter().any(|x| x.version == 0) }

    /// 缺口立案。
    pub fn file_gap(&mut self, name: &'static str) { if !self.cases.contains(&name) { self.cases.push(name); } }

    /// 冗余归并评审。
    pub fn review_redundancy(&mut self, name: &'static str) {
        if !self.reviews.contains(&name) { self.reviews.push(name); }
    }

    /// 域本色：无障碍专章存在。
    pub fn a11y_chapter_present(&self) -> bool { self.items.iter().any(|x| x.a11y_chapter) }

    /// 组覆盖：八组全有 API。
    pub fn groups_covered(&self) -> bool {
        (0..8).all(|g| self.items.iter().any(|x| x.group == g))
    }
}

fn mk(name: &'static str, group: u8, card: u32, lm: u32, v: u32, st: Stability, a11y: bool) -> ApiItem {
    ApiItem { name, group, card, lang_matrix: lm, version: v, stability: st, a11y_chapter: a11y }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves406_checks() -> CheckSet {
    let mut set = CheckSet::new("ves406");
    let mut inv = ApiInventory::new();

    // C406-01 八组 API 逐条清点（三指针 + 版本 + 稳定性）。
    inv.put(mk("fmt.date", 0, 4021, 9001, 3, Stability::Stable, false));
    inv.put(mk("rtl.mirror", 1, 4022, 9001, 2, Stability::Stable, false));
    inv.put(mk("layout.segment", 2, 4023, 9002, 4, Stability::Evolving, false));
    inv.put(mk("tr.extract", 3, 4071, 9003, 2, Stability::Stable, false));
    inv.put(mk("pack.verify", 4, 4089, 9004, 1, Stability::Stable, false));
    inv.put(mk("locale.route", 5, 4103, 9005, 2, Stability::Stable, false));
    inv.put(mk("perf.budget", 6, 4123, 9006, 1, Stability::Evolving, false));
    inv.put(mk("linter.scan", 7, 4143, 9007, 1, Stability::Evolving, false));
    chk(&mut set, "C406-01", inv.items.len() == 8 && inv.pointers_ok() && inv.stability_ok(), "八组八 API 齐");

    // C406-02 组覆盖核验。
    chk(&mut set, "C406-02", inv.groups_covered(), "八组全覆盖");

    // C406-03 对账归零（声明 = 册面）。
    let declared = ["fmt.date", "rtl.mirror", "layout.segment", "tr.extract",
        "pack.verify", "locale.route", "perf.budget", "linter.scan"];
    chk(&mut set, "C406-03", inv.reconcile(&declared) == 0, "对账归零");

    // C406-04 声明漂移检出（声明多出 ghost.api）。
    let mut decl2 = declared;
    decl2[7] = "ghost.api";
    chk(&mut set, "C406-04", inv.reconcile(&decl2) == 2, "双向漂移二");

    // C406-05 修正留痕。
    chk(&mut set, "C406-05", inv.fixes.contains(&"declared-missing")
        && inv.fixes.contains(&"code-extra"), "双向留痕");

    // C406-06 缺口立案。
    inv.file_gap("ghost.api");
    chk(&mut set, "C406-06", inv.cases.contains(&"ghost.api"), "缺口立案");

    // C406-07 冗余归并评审。
    inv.review_redundancy("linter.scan");
    chk(&mut set, "C406-07", inv.reviews.contains(&"linter.scan"), "冗余评审");

    // C406-08 域本色：无障碍文本接口专章。
    inv.put(mk("a11y.text-read", 7, 4149, 9008, 1, Stability::Stable, true));
    chk(&mut set, "C406-08", inv.a11y_chapter_present(), "a11y 专章在册");

    // C406-09 三指针缺检出。
    inv.put(mk("bad.api", 0, 0, 0, 1, Stability::Stable, false));
    chk(&mut set, "C406-09", !inv.pointers_ok(), "三指针缺检出");

    // C406-10 修复三指针。
    inv.put(mk("bad.api", 0, 4099, 9009, 1, Stability::Stable, false));
    chk(&mut set, "C406-10", inv.pointers_ok(), "修复复绿");

    // C406-11 稳定性名单单源。
    chk(&mut set, "C406-11", Stability::Stable.name() == "stable"
        && Stability::Deprecated.name() == "deprecated", "稳定性单源");

    // C406-12 覆盖登记幂等。
    let before = inv.items.len();
    inv.put(mk("fmt.date", 0, 4021, 9001, 4, Stability::Stable, false));
    chk(&mut set, "C406-12", inv.items.len() == before && inv.get("fmt.date").unwrap().version == 4, "覆盖升版不增行");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves406_smoke() {
        let set = run_ves406_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves406 red: {:?}", red);
    }
}
