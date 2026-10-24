#![allow(dead_code)]
//! VE-F3957 · 无障碍资产汇总
//!
//! 全域无障碍资产总册（复用资产册模式全域化）：全域五册（全域 WCAG 矩阵/
//! 全域豁免台账/全域测试资产/全域走查汇总/全域断言册）。全域归一断言：
//! 全域资产双处 = 分叉（全域分叉红线实测）；全域缺口四态；全域签核；全域
//! 自举。
//! 错误路径：全域分叉→归一；虚登全域→伪证；缺口无态全域→立案；全域无
//! 签核→无效。
//! 性能：五册 O(归一全域)；矩阵 O(全域条款)；四态 O(全域缺口)；签核
//! O(全域)。
//! 跨批对接：F3157/F3756/F3957 模式全域复用声明；全域资产对端；收官全域
//! 输入。无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x2AC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x2AC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8QCode(pub u16);

impl S8QCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const FORK: Self = Self(SEG_BASE + 1);
    pub const FAKE_REG: Self = Self(SEG_BASE + 2);
    pub const GAP_NO_STATE: Self = Self(SEG_BASE + 3);
    pub const UNSIGNED: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::FORK => "全域分叉→归一",
            Self::FAKE_REG => "虚登全域→伪证",
            Self::GAP_NO_STATE => "缺口无态全域→立案",
            Self::UNSIGNED => "全域无签核→无效",
            _ => "未知",
        }
    }
}

// ── 全域五册 ─────────────────────────────────────────────────────────

/// 五册种别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetBook {
    /// 全域 WCAG 矩阵。
    WcagMatrix,
    /// 全域豁免台账。
    WaiverLedger,
    /// 全域测试资产。
    TestAsset,
    /// 全域走查汇总。
    WalkSummary,
    /// 全域断言册。
    AssertBook,
}

pub const FIVE_BOOKS: [AssetBook; 5] = [
    AssetBook::WcagMatrix,
    AssetBook::WaiverLedger,
    AssetBook::TestAsset,
    AssetBook::WalkSummary,
    AssetBook::AssertBook,
];

/// 资产条目：册种别 + 唯一键 + 指纹。
#[derive(Debug, Clone)]
pub struct AssetItem {
    pub book: AssetBook,
    pub key: &'static str,
    pub fp: u64,
}

/// 全域资产总册：单源——同键双处 = 分叉。
#[derive(Debug, Clone)]
pub struct AssetMaster {
    pub items: Vec<AssetItem>,
}

impl AssetMaster {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// 登记：同键已存在且指纹一致 = 幂等；指纹不同 = 分叉拒收。
    pub fn put(&mut self, it: AssetItem) -> S8QCode {
        if let Some(x) = self.items.iter().find(|x| x.book == it.book && x.key == it.key) {
            return if x.fp == it.fp { S8QCode::OK } else { S8QCode::FORK };
        }
        self.items.push(it);
        S8QCode::OK
    }

    /// 分叉对账：外部指纹表 vs 总册。
    pub fn forks(&self, external: &[AssetItem]) -> Vec<&'static str> {
        external
            .iter()
            .filter(|e| {
                match self.items.iter().find(|x| x.book == e.book && x.key == e.key) {
                    Some(x) => x.fp != e.fp,
                    None => false,
                }
            })
            .map(|e| e.key)
            .collect()
    }

    /// 归一：以总册为准改写外部表。
    pub fn normalize(&self, external: &mut Vec<AssetItem>) -> usize {
        let mut n = 0;
        for e in external.iter_mut() {
            if let Some(x) = self.items.iter().find(|x| x.book == e.book && x.key == e.key) {
                if e.fp != x.fp {
                    e.fp = x.fp;
                    n += 1;
                }
            }
        }
        n
    }

    /// 五册覆盖：每册至少一资产。
    pub fn book_gaps(&self) -> Vec<AssetBook> {
        FIVE_BOOKS
            .iter()
            .filter(|b| !self.items.iter().any(|i| i.book == **b))
            .map(|b| *b)
            .collect()
    }

    /// 虚登检出：key 为空或指纹为 0 = 虚登。
    pub fn fake_regs(&self) -> Vec<&'static str> {
        self.items.iter().filter(|i| i.key.is_empty() || i.fp == 0).map(|i| i.key).collect()
    }
}

impl Default for AssetMaster {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全域缺口四态 ─────────────────────────────────────────────────────

/// 缺口四态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapState {
    /// 未登记态（缺口无态 = 立案）。
    None,
    /// 已登记。
    Logged,
    /// 处置中。
    Ongoing,
    /// 已闭环。
    Closed,
}

/// 缺口条目。
#[derive(Debug, Clone)]
pub struct GapItem {
    pub name: &'static str,
    pub state: GapState,
}

/// 缺口台账：无态缺口立案。
#[derive(Debug, Clone)]
pub struct GapBook {
    pub items: Vec<GapItem>,
}

impl GapBook {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add(&mut self, name: &'static str) {
        self.items.push(GapItem { name, state: GapState::None });
    }

    /// 立案名单：无态缺口。
    pub fn case_needed(&self) -> Vec<&'static str> {
        self.items.iter().filter(|g| g.state == GapState::None).map(|g| g.name).collect()
    }

    /// 状态推进。
    pub fn advance(&mut self, name: &str) -> S8QCode {
        match self.items.iter_mut().find(|g| g.name == name) {
            Some(g) => {
                g.state = match g.state {
                    GapState::None => GapState::Logged,
                    GapState::Logged => GapState::Ongoing,
                    GapState::Ongoing => GapState::Closed,
                    GapState::Closed => GapState::Closed,
                };
                S8QCode::OK
            }
            None => S8QCode::GAP_NO_STATE,
        }
    }

    pub fn all_closed(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|g| g.state == GapState::Closed)
    }
}

impl Default for GapBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全域签核 ─────────────────────────────────────────────────────────

/// 签核结构：五册各一签。
#[derive(Debug, Clone)]
pub struct SignOff {
    pub signed: [bool; 5],
    pub signer: &'static str,
}

impl SignOff {
    pub fn new(signer: &'static str) -> Self {
        Self { signed: [false; 5], signer }
    }

    pub fn sign(&mut self, book: AssetBook) {
        self.signed[book as usize] = true;
    }

    /// 全域无签核→无效：五签齐备才有效。
    pub fn valid(&self) -> bool {
        !self.signer.is_empty() && self.signed.iter().all(|s| *s)
    }

    pub fn missing(&self) -> Vec<AssetBook> {
        FIVE_BOOKS.iter().filter(|b| !self.signed[**b as usize]).map(|b| *b).collect()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves211_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves211_s08asset");

    fn item(book: AssetBook, key: &'static str, fp: u64) -> AssetItem {
        AssetItem { book, key, fp }
    }

    // 01 五册清单守恒
    {
        chk(&mut set, "C3957-01", FIVE_BOOKS.len() == 5 && AssetBook::WcagMatrix as usize == 0 && AssetBook::AssertBook as usize == 4, "err");
    }
    // 02 五册覆盖：全部登记后零缺口
    {
        let mut m = AssetMaster::new();
        for (i, b) in FIVE_BOOKS.iter().enumerate() {
            m.put(item(*b, "key", 100 + i as u64));
        }
        chk(&mut set, "C3957-02", m.book_gaps().is_empty(), "err");
    }
    // 03 全域分叉红线：同键异指纹拒收
    {
        let mut m = AssetMaster::new();
        m.put(item(AssetBook::WcagMatrix, "对比度", 111));
        chk(&mut set, "C3957-03", m.put(item(AssetBook::WcagMatrix, "对比度", 222)) == S8QCode::FORK, "err");
    }
    // 04 同键同指纹幂等
    {
        let mut m = AssetMaster::new();
        m.put(item(AssetBook::TestAsset, "语料", 7));
        chk(&mut set, "C3957-04", m.put(item(AssetBook::TestAsset, "语料", 7)) == S8QCode::OK && m.items.len() == 1, "err");
    }
    // 05 分叉检出与归一
    {
        let mut m = AssetMaster::new();
        m.put(item(AssetBook::WalkSummary, "走查", 55));
        let mut ext = vec![item(AssetBook::WalkSummary, "走查", 99)];
        chk(&mut set, "C3957-05", m.forks(&ext) == vec!["走查"] && m.normalize(&mut ext) == 1 && m.forks(&ext).is_empty() && ext[0].fp == 55, "err");
    }
    // 06 虚登全域→伪证检出
    {
        let mut m = AssetMaster::new();
        m.put(item(AssetBook::WaiverLedger, "", 5));
        m.put(item(AssetBook::WaiverLedger, "空指纹", 0));
        m.put(item(AssetBook::WaiverLedger, "正常", 9));
        chk(&mut set, "C3957-06", m.fake_regs().len() == 2, "err");
    }
    // 07 缺口无态→立案
    {
        let mut g = GapBook::new();
        g.add("豁免超期");
        g.add("矩阵缺格");
        chk(&mut set, "C3957-07", g.case_needed().len() == 2 && !g.all_closed(), "err");
    }
    // 08 缺口四态推进闭环
    {
        let mut g = GapBook::new();
        g.add("缺口甲");
        for _ in 0..3 {
            g.advance("缺口甲");
        }
        chk(&mut set, "C3957-08", g.items[0].state == GapState::Closed && g.all_closed(), "err");
    }
    // 09 缺口推进越界拒收
    {
        let mut g = GapBook::new();
        chk(&mut set, "C3957-09", g.advance("幽灵") == S8QCode::GAP_NO_STATE, "err");
    }
    // 10 全域签核：五签齐备有效
    {
        let mut s = SignOff::new("签核人甲");
        for b in FIVE_BOOKS.iter() {
            s.sign(*b);
        }
        chk(&mut set, "C3957-10", s.valid() && s.missing().is_empty(), "err");
    }
    // 11 缺签→无效 + 缺单
    {
        let mut s = SignOff::new("签核人乙");
        s.sign(AssetBook::WcagMatrix);
        s.sign(AssetBook::TestAsset);
        let miss = s.missing();
        chk(&mut set, "C3957-11", !s.valid() && miss == vec![AssetBook::WaiverLedger, AssetBook::WalkSummary, AssetBook::AssertBook], "err");
    }
    // 12 空签核人无效
    {
        let mut s = SignOff::new("");
        for b in FIVE_BOOKS.iter() {
            s.sign(*b);
        }
        chk(&mut set, "C3957-12", !s.valid(), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3957-13", S8QCode::UNSIGNED.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8QCode::OK, S8QCode::FORK, S8QCode::FAKE_REG, S8QCode::GAP_NO_STATE, S8QCode::UNSIGNED];
        chk(&mut set, "C3957-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_asset_all_green() {
        let s = run_ves211_checks();
        assert!(s.all_passed(), "ves211 red: {}", s.red_items_text());
    }

    #[test]
    fn test_fork_normalize_roundtrip() {
        let mut m = AssetMaster::new();
        m.put(AssetItem { book: AssetBook::AssertBook, key: "断言", fp: 3 });
        let mut ext = vec![AssetItem { book: AssetBook::AssertBook, key: "断言", fp: 4 }];
        assert_eq!(m.forks(&ext), vec!["断言"]);
        m.normalize(&mut ext);
        assert!(m.forks(&ext).is_empty());
    }

    #[test]
    fn test_gap_four_states() {
        let mut g = GapBook::new();
        g.add("x");
        assert_eq!(g.items[0].state, GapState::None);
        g.advance("x");
        assert_eq!(g.items[0].state, GapState::Logged);
        g.advance("x");
        assert_eq!(g.items[0].state, GapState::Ongoing);
        g.advance("x");
        assert!(g.all_closed());
    }
}
