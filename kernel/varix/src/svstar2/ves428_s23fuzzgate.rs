//! ves428_s23fuzzgate — VE-F4188 · T 域 fuzz 总闸确认
//! 确认：T 域 fuzz 总闸——总面册终版确认（T04/T05/T06/T07/T08 五源+新面归集——13+ 面）；
//! 三零线（崩溃 0/挂起 0/越权写 0）确认；最小复现库统一核验；
//! 线未达→阻断+复测；新崩溃→立案+最小复现；面缺→补面；
//! fuzz 语料合成（红线）；越权写为零线（复述）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x11C0;
pub const FACE_FLOOR: usize = 13;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FzCode {
    Ok = 0,
    LineNotMet = SEG_BASE + 0x01,   // 0x11C1
    FaceShort = SEG_BASE + 0x02,    // 0x11C2
    Crash = SEG_BASE + 0x03,        // 0x11C3
    ReproMissing = SEG_BASE + 0x04, // 0x11C4
}

/// fuzz 五源（T04~T08）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzSource { S04 = 0, S05 = 1, S06 = 2, S07 = 3, S08 = 4 }

impl FuzzSource {
    pub const ALL: [FuzzSource; 5] = [
        FuzzSource::S04, FuzzSource::S05, FuzzSource::S06, FuzzSource::S07, FuzzSource::S08];
}

/// 三零线记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ZeroLines { pub crashes: u32, pub hangs: u32, pub unauthorized_writes: u32 }

impl ZeroLines {
    pub fn all_zero(self) -> bool { self.crashes == 0 && self.hangs == 0 && self.unauthorized_writes == 0 }
}

/// fuzz 总闸册。
pub struct FuzzGateBook {
    pub faces: Vec<(&'static str, FuzzSource)>, // 总面册（面名, 归属源）
    pub lines: ZeroLines,
    pub repros: Vec<(String, bool)>, // 最小复现库（崩溃指纹, 已最小化）
    pub new_crash_filed: bool,
}

impl FuzzGateBook {
    pub fn new() -> Self {
        FuzzGateBook { faces: Vec::new(), lines: ZeroLines { crashes: 0, hangs: 0, unauthorized_writes: 0 },
            repros: Vec::new(), new_crash_filed: false }
    }
    pub fn add_face(&mut self, name: &'static str, src: FuzzSource) {
        if !self.faces.iter().any(|(n, _)| *n == name) { self.faces.push((name, src)); }
    }
    /// 总面册终版核验：面数 ≥ 13 且五源各至少一面。
    pub fn faces_final(&self) -> Result<(), FzCode> {
        if self.faces.len() < FACE_FLOOR { return Err(FzCode::FaceShort); }
        for s in FuzzSource::ALL {
            if !self.faces.iter().any(|(_, src)| *src == s) { return Err(FzCode::FaceShort); }
        }
        Ok(())
    }
    /// 新崩溃立案+登记最小复现。
    pub fn file_crash(&mut self, fingerprint: &str, minimized: bool) {
        self.new_crash_filed = true;
        self.repros.push((String::from(fingerprint), minimized));
    }
    /// 总闸裁决。
    pub fn gate(&self) -> Result<(), FzCode> {
        self.faces_final()?;
        if !self.lines.all_zero() { return Err(FzCode::LineNotMet); }
        if self.new_crash_filed { return Err(FzCode::Crash); }
        if self.repros.iter().any(|(_, m)| !m) { return Err(FzCode::ReproMissing); }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves428_checks() -> CheckSet {
    let mut set = CheckSet::new("ves428");
    let mut bk = FuzzGateBook::new();

    // C428-01 面数不足 13 → 补面码。
    for i in 0..10 { bk.add_face("face-x", FuzzSource::ALL[i % 5]); }
    chk(&mut set, "C428-01", bk.faces_final() == Err(FzCode::FaceShort), "面数不足");

    // C428-02 五源缺源拒（补面路径）。
    for i in 10..13 { bk.add_face("face-x", FuzzSource::S04); }
    chk(&mut set, "C428-02", bk.faces_final() == Err(FzCode::FaceShort), "缺源拒");

    // C428-03 五源齐 + 13 面通过终版核验。
    let srcs = [FuzzSource::S05, FuzzSource::S06, FuzzSource::S07];
    for (i, s) in srcs.iter().enumerate() { bk.faces[i + 1].1 = *s; }
    chk(&mut set, "C428-03", bk.faces_final().is_ok() && bk.faces.len() == 13, "13 面五源齐");

    // C428-04 三零线全零→闸绿。
    chk(&mut set, "C428-04", bk.lines.all_zero() && bk.gate().is_ok(), "零线闸绿");

    // C428-05 挂起破线→阻断。
    bk.lines.hangs = 1;
    chk(&mut set, "C428-05", bk.gate() == Err(FzCode::LineNotMet), "挂起破线阻断");

    // C428-06 越权写破线→阻断（红线复述）。
    bk.lines.hangs = 0;
    bk.lines.unauthorized_writes = 2;
    chk(&mut set, "C428-06", bk.gate() == Err(FzCode::LineNotMet), "越权写破线");

    // C428-07 复测归零复绿。
    bk.lines.unauthorized_writes = 0;
    chk(&mut set, "C428-07", bk.gate().is_ok(), "复测归零复绿");

    // C428-08 新崩溃→立案+Crash 码。
    bk.file_crash("fp-a1", true);
    chk(&mut set, "C428-08", bk.gate() == Err(FzCode::Crash) && bk.new_crash_filed, "新崩溃立案");

    // C428-09 复现未最小化→ReproMissing（先于 Crash 不触发——清立案位后验证）。
    bk.new_crash_filed = false;
    bk.repros.push((String::from("fp-b2"), false));
    chk(&mut set, "C428-09", bk.gate() == Err(FzCode::ReproMissing), "复现未最小化");

    // C428-10 复现库统一核验：全最小化后过。
    for r in bk.repros.iter_mut() { r.1 = true; }
    chk(&mut set, "C428-10", bk.gate().is_ok() && bk.repros.len() == 2, "复现库全最小化");

    // C428-11 面册幂等（重名不重入）。
    bk.add_face("face-x", FuzzSource::S08);
    chk(&mut set, "C428-11", bk.faces.len() == 13, "面册幂等");

    // C428-12 零线单源断言。
    chk(&mut set, "C428-12", ZeroLines { crashes: 0, hangs: 0, unauthorized_writes: 0 }.all_zero()
        && !ZeroLines { crashes: 1, hangs: 0, unauthorized_writes: 0 }.all_zero(), "零线单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves428_smoke() {
        let set = run_ves428_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves428 red: {:?}", red);
    }
}
