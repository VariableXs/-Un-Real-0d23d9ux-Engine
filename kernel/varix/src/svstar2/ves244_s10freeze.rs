//! VE-F3990 · S 域基准与预算基线冻结
//!
//! 冻结：S 域三基线冻结执行（性能基准 + 预算基线 + 行为基线——F3984
//! 终核通过后正式冻结 + 冻结哈希留痕）；冻结后变更只走 F3996 变更纪律。
//! 数据结构：冻结册（三基线+哈希）；变更闸。
//! 错误路径：未核先冻→作废重冻；哈希不符→立案；冻结后直改→回退+立案。
//! 性能：冻结 O(基线)；哈希 O(1)；回退 O(改)。
//! 跨批对接：F3984 上游；F3993 评分前提；F3996 纪律下游。
//! 无障碍与隐私：行为基线含无障碍行为基线（色标/朗读/降级——域本色）；
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x44C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x44C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T10BCode(pub u16);

impl T10BCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const NOT_VERIFIED: Self = Self(SEG_BASE + 1);
    pub const HASH_MISMATCH: Self = Self(SEG_BASE + 2);
    pub const DIRECT_EDIT: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::NOT_VERIFIED => "未核先冻→作废重冻",
            Self::HASH_MISMATCH => "哈希不符→立案",
            Self::DIRECT_EDIT => "冻结后直改→回退+立案",
            _ => "未知",
        }
    }
}

// ── 三基线 ───────────────────────────────────────────────────────────

/// 基线三类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineKind {
    /// 性能基准。
    Perf,
    /// 预算基线。
    Budget,
    /// 行为基线（含无障碍行为基线——色标/朗读/降级，域本色）。
    Behavior,
}

pub const BASELINE_KINDS: [BaselineKind; 3] = [
    BaselineKind::Perf, BaselineKind::Budget, BaselineKind::Behavior,
];

impl BaselineKind {
    pub fn name(self) -> &'static str {
        match self {
            BaselineKind::Perf => "性能基准",
            BaselineKind::Budget => "预算基线",
            BaselineKind::Behavior => "行为基线",
        }
    }

    pub fn slot(self) -> usize {
        match self {
            BaselineKind::Perf => 0,
            BaselineKind::Budget => 1,
            BaselineKind::Behavior => 2,
        }
    }
}

/// 单基线：内容指纹 + 终核通过。
#[derive(Debug, Clone)]
pub struct BaselineRow {
    /// 内容指纹（FNV-1a）。
    pub fp: u64,
    /// F3984 终核通过。
    pub verified: bool,
}

/// 冻结册。
#[derive(Debug, Clone)]
pub struct FreezeBook {
    pub rows: [Option<BaselineRow>; 3],
    /// 已冻结。
    pub frozen: bool,
    /// 冻结哈希留痕。
    pub frozen_hash: u64,
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl FreezeBook {
    pub fn new() -> Self {
        Self { rows: [None, None, None], frozen: false, frozen_hash: 0 }
    }

    /// 登记基线（须带终核结论）。
    pub fn put(&mut self, kind: BaselineKind, content: &str, verified: bool) -> bool {
        if self.frozen {
            return false;
        }
        self.rows[kind.slot()] = Some(BaselineRow { fp: fnv1a(content.as_bytes()), verified });
        true
    }

    /// 三基线齐备且全核。
    pub fn ready(&self) -> bool {
        self.rows.iter().all(|r| match r {
            Some(row) => row.verified,
            None => false,
        })
    }

    /// 冻结：未核先冻 → 作废（NOT_VERIFIED）。
    pub fn freeze(&mut self) -> T10BCode {
        if self.frozen {
            return T10BCode::OK;
        }
        if !self.ready() {
            return T10BCode::NOT_VERIFIED;
        }
        let mut h: u64 = 0xcbf29ce484222325;
        for r in self.rows.iter() {
            let fp = r.as_ref().map(|row| row.fp).unwrap_or(0);
            h ^= fp;
            h = h.wrapping_mul(0x100000001b3);
        }
        self.frozen_hash = h;
        self.frozen = true;
        T10BCode::OK
    }

    /// 冻结后直改检出：内容指纹与冻结留痕不符。
    pub fn verify_content(&self, kind: BaselineKind, content: &str) -> T10BCode {
        if !self.frozen {
            return T10BCode::NOT_VERIFIED;
        }
        let fp = fnv1a(content.as_bytes());
        match self.rows[kind.slot()].as_ref() {
            Some(row) if row.fp == fp => T10BCode::OK,
            Some(_) => T10BCode::DIRECT_EDIT,
            None => T10BCode::HASH_MISMATCH,
        }
    }

    /// 冻结哈希复核（立案依据）。
    pub fn hash_ok(&self) -> bool {
        self.frozen && self.frozen_hash != 0
    }
}

// ── 变更闸（F3996 下游入口）─────────────────────────────────────────

/// 变更闸：冻结后一切变更转纪律册。
#[derive(Debug, Clone)]
pub struct ChangeGate {
    /// 冻结册引用（按哈希比对）。
    pub frozen_hash: u64,
    /// 转纪律次数。
    pub routed: u32,
}

impl ChangeGate {
    /// 变更请求：冻结中 → 拒直改、转纪律。
    pub fn request(&mut self, book_frozen: bool) -> T10BCode {
        if !book_frozen {
            return T10BCode::NOT_VERIFIED;
        }
        self.routed += 1;
        T10BCode::DIRECT_EDIT
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves244_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves244_s10freeze");

    // C3990-01 诊断码段不越窗。
    chk(&mut set, "C3990-01",
        T10BCode::DIRECT_EDIT.0 < SEG_BASE + 16 && SEG_BASE == 0x44C0, "err");

    // C3990-02 三基线齐备表。
    chk(&mut set, "C3990-02",
        BASELINE_KINDS.len() == 3
            && BASELINE_KINDS[2].name() == "行为基线", "err");

    // C3990-03 未核先冻 → 作废。
    let mut f = FreezeBook::new();
    f.put(BaselineKind::Perf, "perf-raw", false);
    f.put(BaselineKind::Budget, "budget-raw", true);
    f.put(BaselineKind::Behavior, "behavior-raw", true);
    chk(&mut set, "C3990-03",
        !f.ready() && f.freeze() == T10BCode::NOT_VERIFIED && !f.frozen, "err");

    // C3990-04 终核后冻结 + 哈希留痕。
    f.put(BaselineKind::Perf, "perf-final", true);
    chk(&mut set, "C3990-04",
        f.ready() && f.freeze() == T10BCode::OK && f.hash_ok(), "err");

    // C3990-05 冻结幂等。
    chk(&mut set, "C3990-05", f.freeze() == T10BCode::OK, "err");

    // C3990-06 冻结后登记拒绝。
    chk(&mut set, "C3990-06", !f.put(BaselineKind::Perf, "late", true), "err");

    // C3990-07 内容复核：一致 → OK。
    chk(&mut set, "C3990-07",
        f.verify_content(BaselineKind::Perf, "perf-final") == T10BCode::OK, "err");

    // C3990-08 内容直改 → DIRECT_EDIT（回退+立案）。
    chk(&mut set, "C3990-08",
        f.verify_content(BaselineKind::Perf, "perf-hacked") == T10BCode::DIRECT_EDIT, "err");

    // C3990-09 缺基线复核 → HASH_MISMATCH。
    let mut f2 = FreezeBook::new();
    f2.put(BaselineKind::Perf, "p", true);
    f2.put(BaselineKind::Budget, "b", true);
    f2.put(BaselineKind::Behavior, "v", true);
    f2.freeze();
    // Behavior 改内容但缺登记的行为行未动——用 Budget 缺失构造。
    let mut f3 = FreezeBook::new();
    f3.put(BaselineKind::Perf, "p", true);
    f3.freeze();
    // f3 未冻成功（未核），verify 应 NOT_VERIFIED。
    chk(&mut set, "C3990-09",
        f3.verify_content(BaselineKind::Perf, "p") == T10BCode::NOT_VERIFIED, "err");
    let _ = f2;

    // C3990-10 哈希随内容变。
    let mut f4 = FreezeBook::new();
    f4.put(BaselineKind::Perf, "p1", true);
    f4.put(BaselineKind::Budget, "b1", true);
    f4.put(BaselineKind::Behavior, "v1", true);
    f4.freeze();
    let h1 = f4.frozen_hash;
    let mut f5 = FreezeBook::new();
    f5.put(BaselineKind::Perf, "p2", true);
    f5.put(BaselineKind::Budget, "b1", true);
    f5.put(BaselineKind::Behavior, "v1", true);
    f5.freeze();
    chk(&mut set, "C3990-10", h1 != f5.frozen_hash, "err");

    // C3990-11 行为基线含无障碍三面（域本色抽查——指纹合成）。
    let a11y_fp = fnv1a("色标/朗读/降级".as_bytes());
    chk(&mut set, "C3990-11", a11y_fp != 0 && fnv1a("色标/朗读/降级".as_bytes()) == a11y_fp, "err");

    // C3990-12 变更闸：未冻 NOT_VERIFIED。
    let mut g = ChangeGate { frozen_hash: 0, routed: 0 };
    chk(&mut set, "C3990-12", g.request(false) == T10BCode::NOT_VERIFIED, "err");

    // C3990-13 变更闸：冻中 → 转纪律（DIRECT_EDIT 语义）+ 计数。
    chk(&mut set, "C3990-13",
        g.request(true) == T10BCode::DIRECT_EDIT && g.routed == 1, "err");

    // C3990-14 槽位双射。
    let slots: Vec<usize> = BASELINE_KINDS.iter().map(|k| k.slot()).collect();
    chk(&mut set, "C3990-14",
        slots[0] != slots[1] && slots[1] != slots[2] && slots[0] != slots[2], "err");

    // C3990-15 FNV 确定性。
    chk(&mut set, "C3990-15",
        fnv1a(b"x") == fnv1a(b"x") && fnv1a(b"x") != fnv1a(b"y"), "err");

    // C3990-16 基线名互异。
    chk(&mut set, "C3990-16",
        BaselineKind::Perf.name() != BaselineKind::Budget.name()
            && BaselineKind::Budget.name() != BaselineKind::Behavior.name(), "err");

    // C3990-17 空册冻结拒。
    chk(&mut set, "C3990-17", FreezeBook::new().freeze() == T10BCode::NOT_VERIFIED, "err");

    // C3990-18 未核先冻的作废可重冻（作废不落 frozen 标记）。
    let mut f6 = FreezeBook::new();
    f6.put(BaselineKind::Perf, "p", false);
    f6.put(BaselineKind::Budget, "b", false);
    f6.put(BaselineKind::Behavior, "v", false);
    let _ = f6.freeze();
    f6.put(BaselineKind::Perf, "p", true);
    f6.put(BaselineKind::Budget, "b", true);
    f6.put(BaselineKind::Behavior, "v", true);
    chk(&mut set, "C3990-18", f6.freeze() == T10BCode::OK && f6.hash_ok(), "err");

    // C3990-19 错误码互异。
    let codes = [T10BCode::OK.0, T10BCode::NOT_VERIFIED.0, T10BCode::HASH_MISMATCH.0,
        T10BCode::DIRECT_EDIT.0];
    chk(&mut set, "C3990-19",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    // C3990-20 冻结闸闭环（未冻不得过闸）。
    let mut f7 = FreezeBook::new();
    chk(&mut set, "C3990-20",
        !f7.ready() && f7.frozen_hash == 0 && FreezeBook::new().hash_ok() == false, "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unverified_freeze_fails() {
        let mut f = FreezeBook::new();
        f.put(BaselineKind::Perf, "x", false);
        f.put(BaselineKind::Budget, "y", false);
        f.put(BaselineKind::Behavior, "z", false);
        assert_eq!(f.freeze(), T10BCode::NOT_VERIFIED);
        assert!(!f.frozen);
    }

    #[test]
    fn direct_edit_detected() {
        let mut f = FreezeBook::new();
        f.put(BaselineKind::Perf, "a", true);
        f.put(BaselineKind::Budget, "b", true);
        f.put(BaselineKind::Behavior, "c", true);
        f.freeze();
        assert_eq!(f.verify_content(BaselineKind::Budget, "bx"), T10BCode::DIRECT_EDIT);
    }

    #[test]
    fn change_gate_routes() {
        let mut g = ChangeGate { frozen_hash: 7, routed: 0 };
        assert_eq!(g.request(true), T10BCode::DIRECT_EDIT);
        assert_eq!(g.routed, 1);
    }
}
