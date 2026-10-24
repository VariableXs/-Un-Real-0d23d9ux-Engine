//! VE-F3999 · S 域归档与证据三件套
//!
//! 归档：S 域收官证据三件套（验收证据包（评分+走查+联测）/ 冻结证据
//! 包（三基线哈希）/ 纪律证据包（变更+承诺）封装 + 哈希 + 公示位登记）。
//! 数据结构：三件套（三包+哈希）；公示登记。
//! 错误路径：包缺→补封；哈希错→重算+立案；公示缺→补登。
//! 性能：封装 O(包)；哈希 O(1)；登记 O(1)。
//! 跨批对接：F3992/F3993/F3996 证据源；F4000 宣告引用。
//! 无障碍与隐私：证据包含无障碍终审证据专卷（域本色）；包内录证脱敏
//! 终版。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x4BC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x4BC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T19ECode(pub u16);

impl T19ECode {
    pub const OK: Self = Self(SEG_BASE);
    pub const PACK_GAP: Self = Self(SEG_BASE + 1);
    pub const HASH_WRONG: Self = Self(SEG_BASE + 2);
    pub const PUBLIC_MISS: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::PACK_GAP => "包缺→补封",
            Self::HASH_WRONG => "哈希错→重算+立案",
            Self::PUBLIC_MISS => "公示缺→补登",
            _ => "未知",
        }
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ── 三证据包 ─────────────────────────────────────────────────────────

/// 证据包三类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidencePack {
    /// 验收证据包（评分+走查+联测）。
    Acceptance,
    /// 冻结证据包（三基线哈希）。
    Freeze,
    /// 纪律证据包（变更+承诺）。
    Discipline,
}

pub const EVIDENCE_PACKS: [EvidencePack; 3] = [
    EvidencePack::Acceptance, EvidencePack::Freeze, EvidencePack::Discipline,
];

impl EvidencePack {
    pub fn name(self) -> &'static str {
        match self {
            EvidencePack::Acceptance => "验收证据包",
            EvidencePack::Freeze => "冻结证据包",
            EvidencePack::Discipline => "纪律证据包",
        }
    }

    pub fn slot(self) -> usize {
        match self {
            EvidencePack::Acceptance => 0,
            EvidencePack::Freeze => 1,
            EvidencePack::Discipline => 2,
        }
    }
}

/// 单包内容清单（封装要素）。
#[derive(Debug, Clone)]
pub struct PackContent {
    /// 必含要素。
    pub requires: [&'static str; 3],
    /// 已封入要素。
    pub sealed: Vec<&'static str>,
    /// 包哈希。
    pub hash: u64,
    /// 无障碍终审证据专卷（验收包域本色专卷）。
    pub a11y_volume: bool,
}

/// 三件套总册。
#[derive(Debug, Clone)]
pub struct EvidenceTrio {
    pub packs: [Option<PackContent>; 3],
    /// 公示位登记：包 → 已公示。
    pub published: [bool; 3],
}

impl EvidenceTrio {
    pub fn new() -> Self {
        Self {
            packs: [None, None, None],
            published: [false; 3],
        }
    }

    /// 建包（要素清单按类固定）。
    pub fn create(&mut self, kind: EvidencePack) -> bool {
        if self.packs[kind.slot()].is_some() {
            return false;
        }
        let requires = match kind {
            EvidencePack::Acceptance => ["评分", "走查", "联测"],
            EvidencePack::Freeze => ["性能哈希", "预算哈希", "行为哈希"],
            EvidencePack::Discipline => ["变更单", "承诺台账", "处置记录"],
        };
        self.packs[kind.slot()] = Some(PackContent {
            requires,
            sealed: Vec::new(),
            hash: 0,
            a11y_volume: kind == EvidencePack::Acceptance,
        });
        true
    }

    /// 封入要素。
    pub fn seal_item(&mut self, kind: EvidencePack, item: &'static str) -> bool {
        match self.packs[kind.slot()].as_mut() {
            Some(p) => {
                if !p.requires.contains(&item) || p.sealed.contains(&item) {
                    return false;
                }
                p.sealed.push(item);
                true
            }
            None => false,
        }
    }

    /// 缺要素检出。
    pub fn pack_gaps(&self, kind: EvidencePack) -> Vec<&'static str> {
        match self.packs[kind.slot()].as_ref() {
            Some(p) => p.requires.iter().filter(|r| !p.sealed.contains(r)).map(|r| *r).collect(),
            None => vec!["包缺"],
        }
    }

    /// 封装哈希（三要素齐 + 验收包须专卷）。
    pub fn seal(&mut self, kind: EvidencePack) -> T19ECode {
        let slot = kind.slot();
        if !self.pack_gaps(kind).is_empty() {
            return T19ECode::PACK_GAP;
        }
        let p = self.packs[slot].as_mut().unwrap();
        if kind == EvidencePack::Acceptance && !p.a11y_volume {
            return T19ECode::PACK_GAP;
        }
        let mut src = String::new();
        for r in p.requires.iter() {
            src.push_str(r);
        }
        p.hash = fnv1a(src.as_bytes());
        T19ECode::OK
    }

    /// 哈希复核（重算比对）。
    pub fn verify(&self, kind: EvidencePack) -> T19ECode {
        let slot = kind.slot();
        match self.packs[slot].as_ref() {
            Some(p) => {
                let mut src = String::new();
                for r in p.requires.iter() {
                    src.push_str(r);
                }
                if fnv1a(src.as_bytes()) == p.hash && p.hash != 0 {
                    T19ECode::OK
                } else {
                    T19ECode::HASH_WRONG
                }
            }
            None => T19ECode::PACK_GAP,
        }
    }

    /// 公示登记（哈希核过才能登）。
    pub fn publish(&mut self, kind: EvidencePack) -> T19ECode {
        if self.verify(kind) != T19ECode::OK {
            return T19ECode::HASH_WRONG;
        }
        self.published[kind.slot()] = true;
        T19ECode::OK
    }

    /// 公示缺检出。
    pub fn unpublisheds(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for k in EVIDENCE_PACKS.iter() {
            if !self.published[k.slot()] {
                out.push(k.name());
            }
        }
        out
    }

    /// 三件套齐活：三包齐 + 三哈希过 + 三公示。
    pub fn complete(&self) -> bool {
        self.unpublisheds().is_empty()
            && EVIDENCE_PACKS.iter().all(|k| self.verify(*k) == T19ECode::OK)
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves253_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves253_s10evidence");

    // C3999-01 诊断码段不越窗。
    chk(&mut set, "C3999-01",
        T19ECode::PUBLIC_MISS.0 < SEG_BASE + 16 && SEG_BASE == 0x4BC0, "err");

    // C3999-02 三件套齐备表。
    chk(&mut set, "C3999-02",
        EVIDENCE_PACKS.len() == 3
            && EVIDENCE_PACKS[0].name() == "验收证据包"
            && EVIDENCE_PACKS[2].name() == "纪律证据包", "err");

    // C3999-03 包缺检出（未建包）。
    let mut t = EvidenceTrio::new();
    chk(&mut set, "C3999-03", t.pack_gaps(EvidencePack::Freeze) == vec!["包缺"], "err");

    // C3999-04 建包 + 缺要素。
    t.create(EvidencePack::Acceptance);
    chk(&mut set, "C3999-04",
        t.pack_gaps(EvidencePack::Acceptance) == vec!["评分", "走查", "联测"], "err");

    // C3999-05 封要素 + 重复封拒 + 越名封拒。
    t.seal_item(EvidencePack::Acceptance, "评分");
    chk(&mut set, "C3999-05",
        !t.seal_item(EvidencePack::Acceptance, "评分")
            && !t.seal_item(EvidencePack::Acceptance, "伪要素"), "err");

    // C3999-06 要素齐 → 封装过（专卷默认带）。
    t.seal_item(EvidencePack::Acceptance, "走查");
    t.seal_item(EvidencePack::Acceptance, "联测");
    chk(&mut set, "C3999-06", t.seal(EvidencePack::Acceptance) == T19ECode::OK, "err");

    // C3999-07 验收包去专卷 → 封装拒（域本色硬门）。
    let mut t2 = EvidenceTrio::new();
    t2.create(EvidencePack::Acceptance);
    for i in ["评分", "走查", "联测"] {
        t2.seal_item(EvidencePack::Acceptance, i);
    }
    if let Some(p) = t2.packs[0].as_mut() {
        p.a11y_volume = false;
    }
    chk(&mut set, "C3999-07", t2.seal(EvidencePack::Acceptance) == T19ECode::PACK_GAP, "err");

    // C3999-08 冻结包三哈希要素。
    t.create(EvidencePack::Freeze);
    for i in ["性能哈希", "预算哈希", "行为哈希"] {
        t.seal_item(EvidencePack::Freeze, i);
    }
    chk(&mut set, "C3999-08", t.seal(EvidencePack::Freeze) == T19ECode::OK, "err");

    // C3999-09 纪律包要素。
    t.create(EvidencePack::Discipline);
    for i in ["变更单", "承诺台账", "处置记录"] {
        t.seal_item(EvidencePack::Discipline, i);
    }
    chk(&mut set, "C3999-09", t.seal(EvidencePack::Discipline) == T19ECode::OK, "err");

    // C3999-10 公示缺检出。
    chk(&mut set, "C3999-10", t.unpublisheds().len() == 3 && !t.complete(), "err");

    // C3999-11 逐包公示。
    for k in EVIDENCE_PACKS.iter() {
        t.publish(*k);
    }
    chk(&mut set, "C3999-11", t.unpublisheds().is_empty() && t.complete(), "err");

    // C3999-12 哈希篡改 → 重算前公示拒。
    let mut t3 = EvidenceTrio::new();
    t3.create(EvidencePack::Freeze);
    for i in ["性能哈希", "预算哈希", "行为哈希"] {
        t3.seal_item(EvidencePack::Freeze, i);
    }
    t3.seal(EvidencePack::Freeze);
    if let Some(p) = t3.packs[1].as_mut() {
        p.hash ^= 0xFF;
    }
    chk(&mut set, "C3999-12",
        t3.verify(EvidencePack::Freeze) == T19ECode::HASH_WRONG
            && t3.publish(EvidencePack::Freeze) == T19ECode::HASH_WRONG, "err");

    // C3999-13 重算恢复。
    t3.seal(EvidencePack::Freeze);
    chk(&mut set, "C3999-13", t3.verify(EvidencePack::Freeze) == T19ECode::OK, "err");

    // C3999-14 未核先公示拒（空包）。
    let mut t4 = EvidenceTrio::new();
    chk(&mut set, "C3999-14", t4.publish(EvidencePack::Acceptance) == T19ECode::HASH_WRONG, "err");

    // C3999-15 重复建包拒。
    chk(&mut set, "C3999-15", !t.create(EvidencePack::Acceptance), "err");

    // C3999-16 包槽双射。
    let slots: Vec<usize> = EVIDENCE_PACKS.iter().map(|k| k.slot()).collect();
    chk(&mut set, "C3999-16",
        slots[0] != slots[1] && slots[1] != slots[2] && slots[0] != slots[2], "err");

    // C3999-17 专卷标记仅验收包。
    let mut t5 = EvidenceTrio::new();
    for k in EVIDENCE_PACKS.iter() {
        t5.create(*k);
    }
    chk(&mut set, "C3999-17",
        t5.packs[0].as_ref().map(|p| p.a11y_volume).unwrap_or(false)
            && t5.packs[1].as_ref().map(|p| !p.a11y_volume).unwrap_or(false), "err");

    // C3999-18 封装哈希确定性。
    let mut t6 = EvidenceTrio::new();
    t6.create(EvidencePack::Freeze);
    for i in ["性能哈希", "预算哈希", "行为哈希"] {
        t6.seal_item(EvidencePack::Freeze, i);
    }
    t6.seal(EvidencePack::Freeze);
    chk(&mut set, "C3999-18",
        t6.packs[1].as_ref().map(|p| p.hash).unwrap_or(0)
            == t.packs[1].as_ref().map(|p| p.hash).unwrap_or(1), "err");

    // C3999-19 公示缺码语义。
    chk(&mut set, "C3999-19", T19ECode::PUBLIC_MISS.explain().contains("补登"), "err");

    // C3999-20 错误码互异。
    let codes = [T19ECode::OK.0, T19ECode::PACK_GAP.0, T19ECode::HASH_WRONG.0, T19ECode::PUBLIC_MISS.0];
    chk(&mut set, "C3999-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trio_flow() {
        let mut t = EvidenceTrio::new();
        for k in EVIDENCE_PACKS.iter() {
            t.create(*k);
        }
        for (k, items) in [
            (EvidencePack::Acceptance, ["评分", "走查", "联测"]),
            (EvidencePack::Freeze, ["性能哈希", "预算哈希", "行为哈希"]),
            (EvidencePack::Discipline, ["变更单", "承诺台账", "处置记录"]),
        ] {
            for i in items {
                t.seal_item(k, i);
            }
            assert_eq!(t.seal(k), T19ECode::OK);
            assert_eq!(t.publish(k), T19ECode::OK);
        }
        assert!(t.complete());
    }

    #[test]
    fn pack_gap_detected() {
        let mut t = EvidenceTrio::new();
        t.create(EvidencePack::Discipline);
        t.seal_item(EvidencePack::Discipline, "变更单");
        assert_eq!(t.pack_gaps(EvidencePack::Discipline).len(), 2);
        assert_eq!(t.seal(EvidencePack::Discipline), T19ECode::PACK_GAP);
    }
}
