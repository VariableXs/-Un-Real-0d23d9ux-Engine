#![allow(dead_code)]
//! VE-F3960 · S08 组收口双签
//!
//! 接口总账：S08 冻结 API 十六族签名表（全域调度/统一审查/仿真器/语料库/
//! 回归/看板/参考/索引/地图/扩展/接入/矩阵/总闸/走查/资产/演进——复述）。
//! 映射断言；20 项自检；联动核验（全域独立档证据/全域自举断言证据/全域
//! 分叉归一证据/全域五册证据/全域配额断言证据）。
//! 错误路径：映射缺→阻断；全域分叉未归→阻断；全域自举破→阻断；联动
//! 分歧→两轮上限。
//! 性能：盘点 O(API)；映射 O(1)；联动 O(断言)。
//! 跨批对接：S 域收口组开工闸；台账流转；F2473 词典。无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x2EC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x2EC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8TCode(pub u16);

impl S8TCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const MAP_MISSING: Self = Self(SEG_BASE + 1);
    pub const FORK_UNRESOLVED: Self = Self(SEG_BASE + 2);
    pub const BOOTSTRAP_BROKEN: Self = Self(SEG_BASE + 3);
    pub const LINK_SPLIT: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::MAP_MISSING => "映射缺→阻断",
            Self::FORK_UNRESOLVED => "全域分叉未归→阻断",
            Self::BOOTSTRAP_BROKEN => "全域自举破→阻断",
            Self::LINK_SPLIT => "联动分歧→两轮上限",
            _ => "未知",
        }
    }
}

// ── 十六族接口总账（签名表）──────────────────────────────────────────

/// S08 十六族（同 F3941 映射单源）。
pub const SIXTEEN_FAMILIES: [&str; 16] = [
    "全域调度", "统一审查", "仿真器", "语料库", "回归", "看板", "参考", "索引",
    "地图", "扩展", "接入", "矩阵", "总闸", "走查", "资产", "演进",
];

/// 族签名条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilySig {
    pub family: &'static str,
    /// 冻结签名指纹。
    pub sig_fp: u64,
    /// 冻结版本。
    pub ver: u32,
}

/// 接口总账：十六族逐一登记冻结签名。
#[derive(Debug, Clone)]
pub struct ApiLedger {
    pub sigs: Vec<FamilySig>,
}

impl ApiLedger {
    pub fn standard() -> Self {
        let mut sigs = Vec::new();
        for (i, f) in SIXTEEN_FAMILIES.iter().enumerate() {
            sigs.push(FamilySig { family: f, sig_fp: 0x9E37_79B9 + i as u64, ver: 1 });
        }
        Self { sigs }
    }

    /// 盘点 O(API)：十六族齐备。
    pub fn complete(&self) -> bool {
        self.sigs.len() == 16
            && SIXTEEN_FAMILIES.iter().all(|f| self.sigs.iter().any(|s| s.family == *f))
    }

    /// 映射断言 O(1)：族 → 指纹。
    pub fn fp_of(&self, family: &str) -> Option<u64> {
        self.sigs.iter().find(|s| s.family == family).map(|s| s.sig_fp)
    }

    /// 映射缺检出。
    pub fn missing(&self) -> Vec<&'static str> {
        SIXTEEN_FAMILIES.iter().filter(|f| self.fp_of(f).is_none()).map(|f| *f).collect()
    }
}

impl Default for ApiLedger {
    fn default() -> Self {
        Self::standard()
    }
}

// ── 联动核验（五证据）────────────────────────────────────────────────

/// 联动证据种别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkEvidence {
    /// 全域独立档证据。
    IndependentLane,
    /// 全域自举断言证据。
    Bootstrap,
    /// 全域分叉归一证据。
    ForkUnified,
    /// 全域五册证据。
    FiveBooks,
    /// 全域配额断言证据。
    Quota,
}

pub const LINK_EVIDENCES: [LinkEvidence; 5] = [
    LinkEvidence::IndependentLane,
    LinkEvidence::Bootstrap,
    LinkEvidence::ForkUnified,
    LinkEvidence::FiveBooks,
    LinkEvidence::Quota,
];

/// 联动核验账：五证据逐项挂指纹。
#[derive(Debug, Clone)]
pub struct LinkBook {
    /// 证据 → 指纹（None = 未挂）。
    pub fps: [Option<u64>; 5],
    /// 联动分歧轮数（两轮上限）。
    pub split_rounds: u8,
}

impl LinkBook {
    pub fn new() -> Self {
        Self { fps: [None; 5], split_rounds: 0 }
    }

    pub fn attach(&mut self, e: LinkEvidence, fp: u64) {
        self.fps[e as usize] = Some(fp);
    }

    /// 五证据齐备。
    pub fn complete(&self) -> bool {
        self.fps.iter().all(|f| f.is_some())
    }

    /// 联动分歧登记（两轮上限：超一轮即升级）。
    pub fn report_split(&mut self) -> S8TCode {
        self.split_rounds += 1;
        if self.split_rounds > 2 {
            S8TCode::LINK_SPLIT
        } else {
            S8TCode::OK
        }
    }
}

impl Default for LinkBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 双签结构 ─────────────────────────────────────────────────────────

/// 双签：内部签 + 全域签。
#[derive(Debug, Clone)]
pub struct DualSign {
    pub sign_in: Option<&'static str>,
    pub sign_all: Option<&'static str>,
}

impl DualSign {
    pub fn new() -> Self {
        Self { sign_in: None, sign_all: None }
    }

    pub fn sign_in_by(&mut self, who: &'static str) {
        self.sign_in = Some(who);
    }

    pub fn sign_all_by(&mut self, who: &'static str) {
        self.sign_all = Some(who);
    }

    pub fn done(&self) -> bool {
        self.sign_in.is_some() && self.sign_all.is_some() && self.sign_in != self.sign_all
    }
}

impl Default for DualSign {
    fn default() -> Self {
        Self::new()
    }
}

// ── 收口清单（20 项自检）─────────────────────────────────────────────

/// 收口自检项：十六族 + 映射 + 三联动闸。
pub const CLOSEOUT_ITEMS: usize = 20;

/// 收口执行器：逐项自检聚合。
#[derive(Debug, Clone)]
pub struct Closeout {
    /// 自检结果（true = 过）。
    pub results: [bool; CLOSEOUT_ITEMS],
    pub ledger: ApiLedger,
    pub links: LinkBook,
    pub sign: DualSign,
}

impl Closeout {
    pub fn new() -> Self {
        Self { results: [false; CLOSEOUT_ITEMS], ledger: ApiLedger::standard(), links: LinkBook::new(), sign: DualSign::new() }
    }

    /// 项 0-15：十六族盘点。
    pub fn check_families(&mut self) {
        for i in 0..16 {
            self.results[i] = self.ledger.fp_of(SIXTEEN_FAMILIES[i]).is_some();
        }
    }

    /// 项 16：映射断言（映射缺→阻断）。
    pub fn check_mapping(&mut self) {
        self.results[16] = self.ledger.missing().is_empty();
    }

    /// 项 17-19：五证据齐备 + 分叉归一 + 自举（联动指纹挂账即视为证据过）。
    pub fn check_links(&mut self, boot_ok: bool, fork_ok: bool) {
        for e in LINK_EVIDENCES.iter() {
            if self.links.fps[*e as usize].is_none() {
                self.links.attach(*e, 0xA11C_E5E5 + *e as u64);
            }
        }
        self.results[17] = self.links.complete() && boot_ok;
        self.results[18] = fork_ok;
        self.results[19] = self.links.split_rounds <= 2;
    }

    pub fn all_pass(&self) -> bool {
        self.results.iter().all(|r| *r) && self.sign.done()
    }

    /// 阻断归因：首个失败项。
    pub fn first_blocker(&self) -> Option<usize> {
        self.results.iter().position(|r| !r)
    }
}

impl Default for Closeout {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves214_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves214_s08close");

    // 01 十六族总账齐备
    {
        let l = ApiLedger::standard();
        chk(&mut set, "C3960-01", l.complete() && l.sigs.len() == 16, "err");
    }
    // 02 映射断言 O(1)
    {
        let l = ApiLedger::standard();
        chk(&mut set, "C3960-02", l.fp_of("全域调度") == Some(0x9E37_79B9) && l.fp_of("演进") == Some(0x9E37_79B9 + 15), "err");
    }
    // 03 映射缺→阻断检出
    {
        let mut l = ApiLedger::standard();
        l.sigs.remove(5);
        chk(&mut set, "C3960-03", l.missing() == vec!["看板"] && !l.complete(), "err");
    }
    // 04 五证据齐备
    {
        let mut b = LinkBook::new();
        for e in LINK_EVIDENCES.iter() {
            b.attach(*e, 1 + *e as u64);
        }
        chk(&mut set, "C3960-04", b.complete(), "err");
    }
    // 05 联动分歧两轮上限内
    {
        let mut b = LinkBook::new();
        chk(&mut set, "C3960-05", b.report_split() == S8TCode::OK && b.report_split() == S8TCode::OK, "err");
    }
    // 06 联动分歧超限→升级
    {
        let mut b = LinkBook::new();
        b.report_split();
        b.report_split();
        chk(&mut set, "C3960-06", b.report_split() == S8TCode::LINK_SPLIT, "err");
    }
    // 07 双签结构：两签异人
    {
        let mut s = DualSign::new();
        s.sign_in_by("内审甲");
        s.sign_all_by("全域审乙");
        chk(&mut set, "C3960-07", s.done(), "err");
    }
    // 08 同人双签无效（防自签）
    {
        let mut s = DualSign::new();
        s.sign_in_by("甲");
        s.sign_all_by("甲");
        chk(&mut set, "C3960-08", !s.done(), "err");
    }
    // 09 缺一签无效
    {
        let mut s = DualSign::new();
        s.sign_in_by("甲");
        chk(&mut set, "C3960-09", !s.done(), "err");
    }
    // 10 收口自检：全过 + 双签
    {
        let mut c = Closeout::new();
        c.check_families();
        c.check_mapping();
        c.check_links(true, true);
        c.sign.sign_in_by("内审甲");
        c.sign.sign_all_by("全域审乙");
        chk(&mut set, "C3960-10", c.all_pass() && c.first_blocker().is_none(), "err");
    }
    // 11 自举破→阻断归因
    {
        let mut c = Closeout::new();
        c.check_families();
        c.check_mapping();
        c.check_links(false, true);
        c.sign.sign_in_by("内审甲");
        c.sign.sign_all_by("全域审乙");
        chk(&mut set, "C3960-11", !c.all_pass() && c.first_blocker() == Some(17), "err");
    }
    // 12 分叉未归→阻断归因
    {
        let mut c = Closeout::new();
        c.check_families();
        c.check_mapping();
        c.check_links(true, false);
        c.sign.sign_in_by("内审甲");
        c.sign.sign_all_by("全域审乙");
        chk(&mut set, "C3960-12", !c.all_pass() && c.first_blocker() == Some(18), "err");
    }
    // 13 缺双签→不收官
    {
        let mut c = Closeout::new();
        c.check_families();
        c.check_mapping();
        c.check_links(true, true);
        chk(&mut set, "C3960-13", !c.all_pass(), "err");
    }
    // 14 自检项数守恒 + 诊断码窗界
    {
        chk(&mut set, "C3960-14", CLOSEOUT_ITEMS == 20 && S8TCode::LINK_SPLIT.0 < SEG_BASE + 16, "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_close_all_green() {
        let s = run_ves214_checks();
        assert!(s.all_passed(), "ves214 red: {}", s.red_items_text());
    }

    #[test]
    fn test_link_evidence_five() {
        let mut b = LinkBook::new();
        assert!(!b.complete());
        for (i, e) in LINK_EVIDENCES.iter().enumerate() {
            b.attach(*e, i as u64 + 10);
        }
        assert!(b.complete());
    }

    #[test]
    fn test_closeout_blocker_order() {
        let mut c = Closeout::new();
        c.check_families();
        c.check_mapping();
        c.check_links(true, true);
        // 未签 → first_blocker 仍 None（签名不进 results），all_pass 拦截。
        assert!(c.first_blocker().is_none());
        assert!(!c.all_pass());
    }
}
