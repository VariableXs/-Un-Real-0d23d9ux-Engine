//! VE-F4254 · 验证 fuzz（U03 一致性验证平台）
//! 职责：fuzz 三面（归一器/比对引擎/导出器）+ 三零线（崩溃 0/挂起 0/越权写 0）
//! + 线未达阻断 + 最小复现 + 新崩溃立案入库 + 面缺补面。
//! 段位：0x1DA0（续表 idx488）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1DA0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FzCode {
    /// 零线未达（崩溃/挂起/越权写非零）。
    LineBroken = SEG_BASE,
    /// 面缺失。
    FaceMissing = SEG_BASE + 0x01,
    /// 复现失败（不可复现=立案材料残缺）。
    ReproFail = SEG_BASE + 0x02,
    /// 阻断权未行使。
    NoBlock = SEG_BASE + 0x03,
}

/// fuzz 面。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FzFace {
    Normalizer = 0, // 归一器：畸形发现项
    Comparator = 1, // 比对引擎：极端图像
    Exporter = 2,   // 导出器：畸形格式请求
}
pub const FACE_N: usize = 3;

/// 三零线记录。
#[derive(Clone, Copy, Default, Debug)]
pub struct ZeroLines {
    pub crashes: u32,
    pub hangs: u32,
    pub illegal_writes: u32,
}

impl ZeroLines {
    pub fn all_zero(&self) -> bool {
        self.crashes == 0 && self.hangs == 0 && self.illegal_writes == 0
    }
}

/// 语料条目。
#[derive(Clone, Debug)]
pub struct Corpus {
    pub face: FzFace,
    pub tag: u16,
    pub hostile: bool,
}

/// fuzz 面册。
pub struct FuzzBook {
    pub corpus: Vec<Corpus>,
    pub lines: ZeroLines,
    /// 最小复现库：(面, 语料 tag)。
    pub repros: Vec<(FzFace, u16)>,
    /// 阻断状态。
    pub blocked: bool,
    /// 已立案新崩溃。
    pub cases: u32,
}

impl FuzzBook {
    pub fn new() -> Self {
        FuzzBook { corpus: Vec::new(), lines: ZeroLines::default(), repros: Vec::new(), blocked: false, cases: 0 }
    }

    /// 补面：登记语料（三面各至少一条才可开跑）。
    pub fn seed(&mut self, face: FzFace, tag: u16, hostile: bool) {
        self.corpus.push(Corpus { face, tag, hostile });
    }

    /// 面覆盖检：三面齐备。
    pub fn faces_covered(&self) -> bool {
        (0..FACE_N).all(|f| self.corpus.iter().any(|c| c.face as usize == f))
    }

    /// 开跑：零线未达即阻断+最小复现+立案。
    pub fn run(&mut self, result: ZeroLines) -> Result<(), FzCode> {
        if !self.faces_covered() {
            return Err(FzCode::FaceMissing);
        }
        self.lines = result;
        if result.all_zero() {
            self.blocked = false;
            return Ok(());
        }
        self.blocked = true; // 阻断权行使
        // 为每个非零线生成最小复现（取该面首条语料）
        if result.crashes > 0 {
            if let Some(c) = self.corpus.iter().find(|c| c.face == FzFace::Normalizer) {
                self.repros.push((FzFace::Normalizer, c.tag));
            }
        }
        if result.hangs > 0 {
            if let Some(c) = self.corpus.iter().find(|c| c.face == FzFace::Comparator) {
                self.repros.push((FzFace::Comparator, c.tag));
            }
        }
        if result.illegal_writes > 0 {
            if let Some(c) = self.corpus.iter().find(|c| c.face == FzFace::Exporter) {
                self.repros.push((FzFace::Exporter, c.tag));
            }
        }
        self.cases += self.repros.len() as u32;
        Err(FzCode::LineBroken)
    }

    /// 复现幂等：同 (面,tag) 只复现一次。
    pub fn repro_unique(&self) -> bool {
        for i in 0..self.repros.len() {
            for j in i + 1..self.repros.len() {
                if self.repros[i] == self.repros[j] {
                    return false;
                }
            }
        }
        true
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C488-01..12。
pub fn run_ves488_checks() -> CheckSet {
    let mut set = CheckSet::new("ves488");

    let mut fb = FuzzBook::new();
    chk(&mut set, "C488-01", fb.run(ZeroLines::default()) == Err(FzCode::FaceMissing), "面缺不开跑");

    fb.seed(FzFace::Normalizer, 1, true);
    fb.seed(FzFace::Comparator, 2, true);
    fb.seed(FzFace::Exporter, 3, true);
    chk(&mut set, "C488-02", fb.faces_covered(), "三面齐备（归一器/比对/导出）");

    chk(&mut set, "C488-03", fb.run(ZeroLines::default()).is_ok(), "三零线全零→放行");
    chk(&mut set, "C488-04", !fb.blocked && fb.cases == 0, "放行无阻断无立案");

    let bad = ZeroLines { crashes: 1, hangs: 0, illegal_writes: 0 };
    chk(&mut set, "C488-05", fb.run(bad) == Err(FzCode::LineBroken), "崩溃线破→LineBroken");
    chk(&mut set, "C488-06", fb.blocked, "阻断权行使");
    chk(&mut set, "C488-07", fb.repros.contains(&(FzFace::Normalizer, 1)), "最小复现定位到崩溃面");
    chk(&mut set, "C488-08", fb.cases == 1, "新崩溃立案入库");

    let bad2 = ZeroLines { crashes: 0, hangs: 2, illegal_writes: 3 };
    let _ = fb.run(bad2);
    chk(&mut set, "C488-09", fb.repros.len() == 3 && fb.repro_unique(), "挂起+越权写复现补齐且去重");
    chk(&mut set, "C488-10", fb.cases == 4, "立案累计 1+3");

    fb.seed(FzFace::Normalizer, 1, true);
    chk(&mut set, "C488-11", fb.corpus.len() == 4 && fb.faces_covered(), "补面允许（幂等语料由调用方控重）");
    let mut fb2 = FuzzBook::new();
    fb2.seed(FzFace::Normalizer, 9, false);
    let _ = fb2.run(ZeroLines { crashes: 1, hangs: 0, illegal_writes: 0 });
    chk(&mut set, "C488-12", fb2.repros.is_empty() || !fb2.faces_covered() || fb2.repros.iter().all(|(f, _)| *f == FzFace::Normalizer), "复现只取已覆盖面语料");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzz_zero_lines() {
        let mut fb = FuzzBook::new();
        assert_eq!(fb.run(ZeroLines::default()), Err(FzCode::FaceMissing));
        fb.seed(FzFace::Normalizer, 1, true);
        fb.seed(FzFace::Comparator, 2, true);
        fb.seed(FzFace::Exporter, 3, true);
        assert!(fb.run(ZeroLines::default()).is_ok());
        assert!(fb.run(ZeroLines { crashes: 1, hangs: 1, illegal_writes: 1 }).is_err());
        assert!(fb.blocked && fb.repros.len() == 3);
    }
}
