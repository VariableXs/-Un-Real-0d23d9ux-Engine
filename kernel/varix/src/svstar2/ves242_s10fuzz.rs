//! VE-F3988 · S 域 fuzz 总闸确认
//!
//! 确认：S 域 fuzz 总闸——八组 fuzz 面归集（语义树变异 / 色标配置变异 /
//! 偏好序列变异 / 输入序列变异四主面）+ 总闸通过线（崩溃 0 / 挂起 0 /
//! 越权写 0 三零线）确认。
//! 数据结构：fuzz 面册（四主面）；三零线记录。
//! 错误路径：线未达→阻断+复测；新崩溃→立案+最小复现；面缺→补面。
//! 性能：归集 O(面)；复测 O(样本)；确认 O(1)。
//! 跨批对接：八组 fuzz 条目；F3985 安全终审联动；F3992 清账前提。
//! 无障碍与隐私：fuzz 含偏好序列面（防越权改无障碍配置——域本色）；
//! 语料合成。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x42C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x42C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T8FCode(pub u16);

impl T8FCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const LINE_FAIL: Self = Self(SEG_BASE + 1);
    pub const NEW_CRASH: Self = Self(SEG_BASE + 2);
    pub const FACE_GAP: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::LINE_FAIL => "线未达→阻断+复测",
            Self::NEW_CRASH => "新崩溃→立案+最小复现",
            Self::FACE_GAP => "面缺→补面",
            _ => "未知",
        }
    }
}

// ── 四主面 ───────────────────────────────────────────────────────────

/// fuzz 四主面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuzzFace {
    /// 语义树变异。
    TreeMut,
    /// 色标配置变异。
    ColorMut,
    /// 偏好序列变异（防越权改无障碍配置——域本色）。
    PrefSeqMut,
    /// 输入序列变异。
    InputSeqMut,
}

pub const FUZZ_FACES: [FuzzFace; 4] = [
    FuzzFace::TreeMut, FuzzFace::ColorMut, FuzzFace::PrefSeqMut, FuzzFace::InputSeqMut,
];

impl FuzzFace {
    pub fn name(self) -> &'static str {
        match self {
            FuzzFace::TreeMut => "语义树变异",
            FuzzFace::ColorMut => "色标配置变异",
            FuzzFace::PrefSeqMut => "偏好序列变异",
            FuzzFace::InputSeqMut => "输入序列变异",
        }
    }

    pub fn slot(self) -> usize {
        match self {
            FuzzFace::TreeMut => 0,
            FuzzFace::ColorMut => 1,
            FuzzFace::PrefSeqMut => 2,
            FuzzFace::InputSeqMut => 3,
        }
    }
}

/// 三零线：崩溃 / 挂起 / 越权写。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZeroLines {
    pub crashes: u32,
    pub hangs: u32,
    pub over_writes: u32,
}

impl ZeroLines {
    pub fn all_zero(self) -> bool {
        self.crashes == 0 && self.hangs == 0 && self.over_writes == 0
    }

    /// 线差清单。
    pub fn failed_lines(self) -> [&'static str; 3] {
        let mut out = ["", "", ""];
        if self.crashes > 0 { out[0] = "崩溃线"; }
        if self.hangs > 0 { out[1] = "挂起线"; }
        if self.over_writes > 0 { out[2] = "越权写线"; }
        out
    }
}

/// 单面记录：样本数 + 该面三零线。
#[derive(Debug, Clone)]
pub struct FaceRecord {
    /// 样本数。
    pub samples: u32,
    /// 三零线。
    pub lines: ZeroLines,
}

/// fuzz 面册。
#[derive(Debug, Clone)]
pub struct FuzzBook {
    pub faces: [Option<FaceRecord>; 4],
    /// 最小复现登记（新崩溃时）。
    pub repros: Vec<(u32, String)>,
}

impl FuzzBook {
    pub fn new() -> Self {
        Self { faces: [None, None, None, None], repros: Vec::new() }
    }

    /// 面登记。
    pub fn put(&mut self, face: FuzzFace, samples: u32, lines: ZeroLines) {
        self.faces[face.slot()] = Some(FaceRecord { samples, lines });
    }

    /// 面缺检出。
    pub fn missing_faces(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for f in FUZZ_FACES.iter() {
            if self.faces[f.slot()].is_none() {
                out.push(f.name());
            }
        }
        out
    }

    /// 总闸确认：四主面齐备 + 各面三零线 + 全局三零线。
    pub fn confirm(&self) -> T8FCode {
        let missing = self.missing_faces();
        if !missing.is_empty() {
            return T8FCode::FACE_GAP;
        }
        for r in self.faces.iter().flatten() {
            if r.samples == 0 {
                return T8FCode::FACE_GAP;
            }
            if !r.lines.all_zero() {
                return T8FCode::LINE_FAIL;
            }
        }
        T8FCode::OK
    }

    /// 新崩溃立案 + 最小复现登记。
    pub fn file_crash(&mut self, face: FuzzFace, repro: &str) -> T8FCode {
        self.repros.push((face.slot() as u32, String::from(repro)));
        if let Some(r) = self.faces[face.slot()].as_mut() {
            r.lines.crashes += 1;
        }
        T8FCode::NEW_CRASH
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves242_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves242_s10fuzz");

    // C3988-01 诊断码段不越窗。
    chk(&mut set, "C3988-01",
        T8FCode::FACE_GAP.0 < SEG_BASE + 16 && SEG_BASE == 0x42C0, "err");

    // C3988-02 四主面齐备（含偏好序列面——域本色）。
    chk(&mut set, "C3988-02",
        FUZZ_FACES.len() == 4
            && FUZZ_FACES[2].name() == "偏好序列变异", "err");

    // C3988-03 三零线全零。
    let z = ZeroLines { crashes: 0, hangs: 0, over_writes: 0 };
    chk(&mut set, "C3988-03", z.all_zero() && z.failed_lines() == ["", "", ""], "err");

    // C3988-04 三零线破线检出。
    let z1 = ZeroLines { crashes: 1, hangs: 0, over_writes: 0 };
    let z2 = ZeroLines { crashes: 0, hangs: 2, over_writes: 0 };
    let z3 = ZeroLines { crashes: 0, hangs: 0, over_writes: 3 };
    chk(&mut set, "C3988-04",
        !z1.all_zero() && !z2.all_zero() && !z3.all_zero()
            && z1.failed_lines()[0] == "崩溃线"
            && z2.failed_lines()[1] == "挂起线"
            && z3.failed_lines()[2] == "越权写线", "err");

    // C3988-05 面缺检出。
    let mut b = FuzzBook::new();
    b.put(FuzzFace::TreeMut, 500, z);
    b.put(FuzzFace::ColorMut, 500, z);
    let miss = b.missing_faces();
    chk(&mut set, "C3988-05",
        miss == vec!["偏好序列变异", "输入序列变异"] && b.confirm() == T8FCode::FACE_GAP, "err");

    // C3988-06 补面 + 三零 → 总闸确认过。
    b.put(FuzzFace::PrefSeqMut, 800, z);
    b.put(FuzzFace::InputSeqMut, 800, z);
    chk(&mut set, "C3988-06", b.confirm() == T8FCode::OK, "err");

    // C3988-07 零样本面 = 面缺。
    let mut b2 = FuzzBook::new();
    for f in FUZZ_FACES.iter() {
        b2.put(*f, 0, z);
    }
    chk(&mut set, "C3988-07", b2.confirm() == T8FCode::FACE_GAP, "err");

    // C3988-08 一面破线 → 阻断。
    let mut b3 = FuzzBook::new();
    for f in FUZZ_FACES.iter() {
        b3.put(*f, 100, z);
    }
    b3.put(FuzzFace::InputSeqMut, 100, z1);
    chk(&mut set, "C3988-08", b3.confirm() == T8FCode::LINE_FAIL, "err");

    // C3988-09 新崩溃立案 + 最小复现登记。
    let mut b4 = FuzzBook::new();
    for f in FUZZ_FACES.iter() {
        b4.put(*f, 100, z);
    }
    let code = b4.file_crash(FuzzFace::TreeMut, "min-repro: 深度 64 层语义树");
    chk(&mut set, "C3988-09",
        code == T8FCode::NEW_CRASH && b4.repros.len() == 1
            && b4.faces[0].as_ref().map(|r| r.lines.crashes).unwrap_or(0) == 1
            && b4.confirm() == T8FCode::LINE_FAIL, "err");

    // C3988-10 偏好序列面越权写线（域本色）。
    let zw = ZeroLines { crashes: 0, hangs: 0, over_writes: 2 };
    let mut b5 = FuzzBook::new();
    for f in FUZZ_FACES.iter() {
        b5.put(*f, 100, z);
    }
    b5.put(FuzzFace::PrefSeqMut, 100, zw);
    chk(&mut set, "C3988-10", b5.confirm() == T8FCode::LINE_FAIL, "err");

    // C3988-11 面槽双射。
    let slots: Vec<usize> = FUZZ_FACES.iter().map(|f| f.slot()).collect();
    let mut uniq = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if slots[i] == slots[j] { uniq = false; }
        }
    }
    chk(&mut set, "C3988-11", uniq, "err");

    // C3988-12 复现登记不影响其他面。
    let before = b4.faces[1].as_ref().map(|r| r.lines.crashes).unwrap_or(0);
    let _ = b4.file_crash(FuzzFace::TreeMut, "repro2");
    chk(&mut set, "C3988-12", b4.faces[1].as_ref().map(|r| r.lines.crashes).unwrap_or(0) == before, "err");

    // C3988-13 空面册确认 = 面缺。
    chk(&mut set, "C3988-13", FuzzBook::new().confirm() == T8FCode::FACE_GAP, "err");

    // C3988-14 三零线清单双写位不串。
    let z12 = ZeroLines { crashes: 1, hangs: 2, over_writes: 0 };
    let fl = z12.failed_lines();
    chk(&mut set, "C3988-14", fl[0] == "崩溃线" && fl[1] == "挂起线" && fl[2] == "", "err");

    // C3988-15 立案后复测清零（修复路径）。
    let mut b6 = FuzzBook::new();
    for f in FUZZ_FACES.iter() {
        b6.put(*f, 100, z);
    }
    let _ = b6.file_crash(FuzzFace::ColorMut, "r");
    // 复测通过：重录三零。
    b6.put(FuzzFace::ColorMut, 120, z);
    chk(&mut set, "C3988-15", b6.confirm() == T8FCode::OK, "err");

    // C3988-16 错误码互异。
    let codes = [T8FCode::OK.0, T8FCode::LINE_FAIL.0, T8FCode::NEW_CRASH.0, T8FCode::FACE_GAP.0];
    chk(&mut set, "C3988-16",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    // C3988-17 复现簿与面册解耦。
    chk(&mut set, "C3988-17", b6.repros.len() == 1 && b6.confirm() == T8FCode::OK, "err");

    // C3988-18 四面名互异。
    let mut uniq2 = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if FUZZ_FACES[i].name() == FUZZ_FACES[j].name() { uniq2 = false; }
        }
    }
    chk(&mut set, "C3988-18", uniq2, "err");

    // C3988-19 样本量与三零并存（正常量产面）。
    let big = FaceRecord { samples: 10_000, lines: z };
    chk(&mut set, "C3988-19", big.samples == 10_000 && big.lines.all_zero(), "err");

    // C3988-20 总闸幂等确认。
    chk(&mut set, "C3988-20",
        b6.confirm() == T8FCode::OK && b6.confirm() == T8FCode::OK, "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_blocks_missing_face() {
        let b = FuzzBook::new();
        assert_eq!(b.confirm(), T8FCode::FACE_GAP);
    }

    #[test]
    fn gate_blocks_crash_line() {
        let mut b = FuzzBook::new();
        let z = ZeroLines { crashes: 0, hangs: 0, over_writes: 0 };
        let c1 = ZeroLines { crashes: 1, hangs: 0, over_writes: 0 };
        for f in FUZZ_FACES.iter() {
            b.put(*f, 100, z);
        }
        b.put(FuzzFace::TreeMut, 100, c1);
        assert_eq!(b.confirm(), T8FCode::LINE_FAIL);
    }

    #[test]
    fn crash_filing() {
        let mut b = FuzzBook::new();
        let z = ZeroLines { crashes: 0, hangs: 0, over_writes: 0 };
        for f in FUZZ_FACES.iter() {
            b.put(*f, 10, z);
        }
        assert_eq!(b.file_crash(FuzzFace::InputSeqMut, "r"), T8FCode::NEW_CRASH);
        assert_eq!(b.confirm(), T8FCode::LINE_FAIL);
    }
}
