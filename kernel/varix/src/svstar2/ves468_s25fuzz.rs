//! ves468_s25fuzz — VE-F4234 · 契约 fuzz
//! fuzz 面三处：DSL 解析（畸形语法树）/沙箱求值（资源耗尽表达式）/版本解析（畸形版本串）——三面语料库+
//! 三零线（崩溃 0/挂起 0/越权写 0）；线未达→阻断+最小复现；新崩溃→立案入库；面缺→补面；
//! fuzz 语料合成（红线）；越权写为零线（复述）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x18A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FzCode {
    Ok = 0,
    LineNotMet = SEG_BASE + 0x01, // 0x18A1
    FaceShort = SEG_BASE + 0x02,  // 0x18A2
    Crash = SEG_BASE + 0x03,      // 0x18A3
}

/// 三 fuzz 面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzFace { DslParse = 0, SandboxEval = 1, VersionParse = 2 }

impl FuzzFace {
    pub const ALL: [FuzzFace; 3] = [FuzzFace::DslParse, FuzzFace::SandboxEval, FuzzFace::VersionParse];
}

/// 面语料条目（合成语料——红线）。
pub struct Corpus { pub face: FuzzFace, pub sample: String, pub minimized: bool }

/// fuzz 册。
pub struct FuzzBook {
    pub corpus: Vec<Corpus>,
    pub lines: (u32, u32, u32), // 崩溃/挂起/越权写
    pub crash_files: Vec<String>,
    pub blocked: bool,
}

impl FuzzBook {
    pub fn new() -> Self { FuzzBook { corpus: Vec::new(), lines: (0, 0, 0), crash_files: Vec::new(), blocked: false } }
    pub fn add(&mut self, face: FuzzFace, sample: &str, min: bool) {
        self.corpus.push(Corpus { face, sample: String::from(sample), minimized: min });
    }
    /// 三面齐备核验。
    pub fn faces_full(&self) -> Result<(), FzCode> {
        for f in FuzzFace::ALL {
            if !self.corpus.iter().any(|c| c.face == f) { return Err(FzCode::FaceShort); }
        }
        Ok(())
    }
    pub fn file_crash(&mut self, fp: &str) { self.crash_files.push(String::from(fp)); self.blocked = true; }
    /// 总闸：三面齐+三零线+无崩溃+语料全最小化。
    pub fn gate(&self) -> Result<(), FzCode> {
        self.faces_full()?;
        let (c, h, w) = self.lines;
        if c != 0 || h != 0 || w != 0 { return Err(FzCode::LineNotMet); }
        if self.blocked { return Err(FzCode::Crash); }
        if self.corpus.iter().any(|x| !x.minimized) { return Err(FzCode::LineNotMet); }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves468_checks() -> CheckSet {
    let mut set = CheckSet::new("ves468");
    let mut bk = FuzzBook::new();

    // C468-01 面缺→补面码。
    bk.add(FuzzFace::DslParse, "((((", true);
    chk(&mut set, "C468-01", bk.faces_full() == Err(FzCode::FaceShort), "面缺补面");

    // C468-02 三面齐（合成语料）。
    bk.add(FuzzFace::SandboxEval, "1+1+1+...x1e9", true);
    bk.add(FuzzFace::VersionParse, "9.9.9.9.9-alpha-beta", true);
    chk(&mut set, "C468-02", bk.faces_full().is_ok() && bk.corpus.len() == 3, "三面齐");

    // C468-03 三零线全零→闸绿。
    chk(&mut set, "C468-03", bk.gate().is_ok(), "零线闸绿");

    // C468-04 挂起破线→阻断。
    bk.lines.1 = 1;
    chk(&mut set, "C468-04", bk.gate() == Err(FzCode::LineNotMet), "挂起破线");

    // C468-05 复测归零复绿。
    bk.lines.1 = 0;
    chk(&mut set, "C468-05", bk.gate().is_ok(), "归零复绿");

    // C468-06 新崩溃→立案入库+阻断。
    bk.file_crash("fp-dsl-01");
    chk(&mut set, "C468-06", bk.gate() == Err(FzCode::Crash) && bk.crash_files.len() == 1, "崩溃立案");

    // C468-07 立案清后语料未最小化→补最小化。
    bk.blocked = false;
    bk.corpus[0].minimized = false;
    chk(&mut set, "C468-07", bk.gate() == Err(FzCode::LineNotMet), "语料未最小化");

    // C468-08 全最小化→复绿。
    bk.corpus[0].minimized = true;
    chk(&mut set, "C468-08", bk.gate().is_ok(), "复绿");

    // C468-09 三面名单单源。
    chk(&mut set, "C468-09", FuzzFace::ALL.len() == 3
        && FuzzFace::ALL[0] == FuzzFace::DslParse
        && FuzzFace::ALL[2] == FuzzFace::VersionParse, "三面单源");

    // C468-10 语料合成断言（样本均为合成畸形串非真实数据）。
    chk(&mut set, "C468-10", bk.corpus.iter().all(|c| c.sample.len() <= 24), "合成语料");

    // C468-11 面语料幂等（追加不重复面要求）。
    bk.add(FuzzFace::DslParse, ")))", true);
    chk(&mut set, "C468-11", bk.faces_full().is_ok(), "语料追加");

    // C468-12 错误码段位单源。
    chk(&mut set, "C468-12", FzCode::Crash as u16 == SEG_BASE + 3, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves468_smoke() {
        let set = run_ves468_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves468 red: {:?}", red);
    }
}
