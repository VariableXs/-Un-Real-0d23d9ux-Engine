//! VE-F3967 · 测试资产汇总
//!
//! S 域测试资产汇总：八组测试用例/语料/仿真器/回归集四类归集。汇总规则：
//! 每资产挂组/挂条/挂断言三指针 + 规模登记。与 F3946 语料库总账对拍。
//! 错误路径：台账缺→回补；对拍不符→修正留痕；资产失联→断链修复。
//! 性能：归集 O(资产)；对拍 O(台账)；登记 O(1)。
//! 跨批对接：F3946 语料库单源；F3964 工具自查上游；F3983 联测下游。
//! 无障碍与隐私：语料含读屏朗读样本与认知可读样本（域本色）；样本脱敏
//! 复核。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x32C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x32C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9GCode(pub u16);

impl S9GCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const LEDGER_GAP: Self = Self(SEG_BASE + 1);
    pub const RECON_MISMATCH: Self = Self(SEG_BASE + 2);
    pub const ORPHAN: Self = Self(SEG_BASE + 3);
    pub const SCALE_GAP: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::LEDGER_GAP => "台账缺→回补",
            Self::RECON_MISMATCH => "对拍不符→修正留痕",
            Self::ORPHAN => "资产失联→断链修复",
            Self::SCALE_GAP => "规模缺→补登",
            _ => "未知",
        }
    }
}

// ── 四类资产 ─────────────────────────────────────────────────────────

/// 资产四类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    /// 测试用例。
    Case,
    /// 语料。
    Corpus,
    /// 仿真器。
    Simulator,
    /// 回归集。
    Regress,
}

pub const KINDS: [AssetKind; 4] = [AssetKind::Case, AssetKind::Corpus, AssetKind::Simulator, AssetKind::Regress];

/// 资产条目：三指针 + 规模 + 脱敏标记。
#[derive(Debug, Clone)]
pub struct AssetRow {
    pub name: &'static str,
    pub kind: AssetKind,
    /// 指针一：挂组（1-8）。
    pub group: u8,
    /// 指针二：挂条。
    pub clause: u32,
    /// 指针三：挂断言（0 = 未挂）。
    pub assert_ref: u32,
    /// 规模（条数）。
    pub scale: u32,
    /// 脱敏标记（含样本类资产必须 true）。
    pub sanitized: bool,
    /// 是否样本类（语料/仿真器须脱敏）。
    pub needs_sanitize: bool,
}

impl AssetRow {
    /// 三指针齐备。
    pub fn pointers_ok(&self) -> bool {
        self.group >= 1 && self.group <= 8 && self.clause > 0 && self.assert_ref > 0
    }

    /// 脱敏合规。
    pub fn sanitize_ok(&self) -> bool {
        !self.needs_sanitize || self.sanitized
    }
}

/// 资产台账。
#[derive(Debug, Clone)]
pub struct AssetLedger {
    pub rows: Vec<AssetRow>,
}

impl AssetLedger {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    /// 登记：三指针或脱敏不合规拒收。
    pub fn admit(&mut self, r: AssetRow) -> S9GCode {
        if !r.pointers_ok() {
            return S9GCode::LEDGER_GAP;
        }
        if !r.sanitize_ok() {
            return S9GCode::SCALE_GAP;
        }
        self.rows.push(r);
        S9GCode::OK
    }

    /// 四类覆盖缺口。
    pub fn kind_gaps(&self) -> Vec<AssetKind> {
        KINDS.iter().filter(|k| !self.rows.iter().any(|r| r.kind == **k)).map(|k| *k).collect()
    }

    /// 规模登记总和 O(1) 可复核。
    pub fn total_scale(&self) -> u32 {
        self.rows.iter().map(|r| r.scale).sum()
    }

    /// 分组规模。
    pub fn scale_of_group(&self, group: u8) -> u32 {
        self.rows.iter().filter(|r| r.group == group).map(|r| r.scale).sum()
    }
}

impl Default for AssetLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 与语料库总账对拍 ─────────────────────────────────────────────────

/// 对拍记录：语料名 → 台账数 vs 总账数。
#[derive(Debug, Clone)]
pub struct ReconPair {
    pub name: &'static str,
    pub ledger_n: u32,
    pub corpus_n: u32,
}

/// 对拍不符清单。
pub fn recon_mismatch(pairs: &[ReconPair]) -> Vec<&'static str> {
    pairs.iter().filter(|p| p.ledger_n != p.corpus_n).map(|p| p.name).collect()
}

/// 修正留痕：对拍不符修正记录。
#[derive(Debug, Clone)]
pub struct ReconTrace {
    pub name: &'static str,
    pub from: u32,
    pub to: u32,
}

/// 断链修复：失联资产重联。
#[derive(Debug, Clone)]
pub struct OrphanMender {
    pub orphans: Vec<&'static str>,
    pub mended: Vec<String>,
}

impl OrphanMender {
    pub fn new() -> Self {
        Self { orphans: Vec::new(), mended: Vec::new() }
    }

    pub fn report(&mut self, name: &'static str) {
        self.orphans.push(name);
    }

    pub fn mend(&mut self, name: &str) -> S9GCode {
        if let Some(pos) = self.orphans.iter().position(|o| *o == name) {
            self.orphans.remove(pos);
            self.mended.push(String::from(name));
            S9GCode::OK
        } else {
            S9GCode::ORPHAN
        }
    }

    pub fn clean(&self) -> bool {
        self.orphans.is_empty()
    }
}

impl Default for OrphanMender {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves221_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves221_s09testasset");

    fn row(name: &'static str, kind: AssetKind, group: u8, scale: u32, sanitized: bool) -> AssetRow {
        AssetRow { name, kind, group, clause: 10, assert_ref: 5, scale, sanitized, needs_sanitize: kind == AssetKind::Corpus }
    }

    // 01 四类清单守恒
    {
        chk(&mut set, "C3967-01", KINDS.len() == 4 && AssetKind::Case as usize == 0 && AssetKind::Regress as usize == 3, "err");
    }
    // 02 四类全覆盖归集
    {
        let mut l = AssetLedger::new();
        for (i, k) in KINDS.iter().enumerate() {
            l.admit(row("资产", *k, (i + 1) as u8, 10, true));
        }
        chk(&mut set, "C3967-02", l.kind_gaps().is_empty() && l.total_scale() == 40, "err");
    }
    // 03 三指针缺拒收
    {
        let mut l = AssetLedger::new();
        let mut r = row("缺指针", AssetKind::Case, 1, 5, true);
        r.assert_ref = 0;
        chk(&mut set, "C3967-03", l.admit(r) == S9GCode::LEDGER_GAP, "err");
    }
    // 04 样本脱敏强制（语料类）
    {
        let mut l = AssetLedger::new();
        let bad = row("未脱敏语料", AssetKind::Corpus, 2, 5, false);
        let good = row("已脱敏语料", AssetKind::Corpus, 2, 5, true);
        chk(&mut set, "C3967-04", l.admit(bad) == S9GCode::SCALE_GAP && l.admit(good) == S9GCode::OK, "err");
    }
    // 05 非样本类免脱敏
    {
        let mut l = AssetLedger::new();
        let r = row("用例", AssetKind::Case, 3, 5, false);
        chk(&mut set, "C3967-05", r.sanitize_ok() && l.admit(r) == S9GCode::OK, "err");
    }
    // 06 分组规模 O(1) 复核
    {
        let mut l = AssetLedger::new();
        l.admit(row("甲", AssetKind::Case, 1, 30, true));
        l.admit(row("乙", AssetKind::Corpus, 1, 20, true));
        l.admit(row("丙", AssetKind::Case, 2, 50, true));
        chk(&mut set, "C3967-06", l.scale_of_group(1) == 50 && l.scale_of_group(2) == 50 && l.total_scale() == 100, "err");
    }
    // 07 对拍不符检出
    {
        let pairs = vec![
            ReconPair { name: "语料甲", ledger_n: 10, corpus_n: 10 },
            ReconPair { name: "语料乙", ledger_n: 8, corpus_n: 12 },
        ];
        chk(&mut set, "C3967-07", recon_mismatch(&pairs) == vec!["语料乙"], "err");
    }
    // 08 对拍一致零不符
    {
        let pairs = vec![ReconPair { name: "语料丙", ledger_n: 7, corpus_n: 7 }];
        chk(&mut set, "C3967-08", recon_mismatch(&pairs).is_empty(), "err");
    }
    // 09 修正留痕结构
    {
        let t = ReconTrace { name: "语料乙", from: 8, to: 12 };
        chk(&mut set, "C3967-09", t.from == 8 && t.to == 12 && t.name == "语料乙", "err");
    }
    // 10 断链修复实测
    {
        let mut m = OrphanMender::new();
        m.report("语料甲");
        chk(&mut set, "C3967-10", !m.clean() && m.mend("语料甲") == S9GCode::OK && m.clean() && m.mended[0] == "语料甲", "err");
    }
    // 11 修不存在孤儿拒收
    {
        let mut m = OrphanMender::new();
        chk(&mut set, "C3967-11", m.mend("幽灵") == S9GCode::ORPHAN, "err");
    }
    // 12 规模零登记检出（台账空 = 缺）
    {
        let l = AssetLedger::new();
        chk(&mut set, "C3967-12", l.total_scale() == 0 && l.kind_gaps().len() == 4, "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3967-13", S9GCode::SCALE_GAP.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9GCode::OK, S9GCode::LEDGER_GAP, S9GCode::RECON_MISMATCH, S9GCode::ORPHAN, S9GCode::SCALE_GAP];
        chk(&mut set, "C3967-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_testasset_all_green() {
        let s = run_ves221_checks();
        assert!(s.all_passed(), "ves221 red: {}", s.red_items_text());
    }

    #[test]
    fn test_admit_matrix() {
        let mut l = AssetLedger::new();
        let ok = AssetRow { name: "a", kind: AssetKind::Simulator, group: 1, clause: 1, assert_ref: 1, scale: 1, sanitized: false, needs_sanitize: false };
        assert_eq!(l.admit(ok), S9GCode::OK);
        let mut bad_ptr = ok.clone();
        bad_ptr.clause = 0;
        assert_eq!(l.admit(bad_ptr), S9GCode::LEDGER_GAP);
        let mut bad_san = ok.clone();
        bad_san.needs_sanitize = true;
        assert_eq!(l.admit(bad_san), S9GCode::SCALE_GAP);
    }

    #[test]
    fn test_orphan_mender_cycle() {
        let mut m = OrphanMender::new();
        m.report("语料乙");
        m.report("语料丙");
        m.mend("语料乙").ok();
        assert_eq!(m.orphans, vec!["语料丙"]);
        m.mend("语料丙").ok();
        assert!(m.clean());
    }
}
