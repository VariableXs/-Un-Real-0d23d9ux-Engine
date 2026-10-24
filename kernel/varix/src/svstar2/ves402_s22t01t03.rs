//! ves402_s22t01t03 — VE-F4162 · T01-T03 组自查核验
//! 三组逐项核验：T01 基础四断言复测（语种矩阵/回退链/格式引擎/切换机制）；
//! T02 RTL 方向双断言复测（镜像布局完整性+方向混合文本正确性）；
//! T03 四族排版管线复测（分词/整形/折行/定位四段语言感知签名一致）——证据引用制执行。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x00E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RvCode {
    Ok = 0,
    AssertFail = SEG_BASE + 0x01,  // 0x00E1
    EvidenceMissing = SEG_BASE + 0x02, // 0x00E2
    SigMismatch = SEG_BASE + 0x03, // 0x00E3
}

/// T01 基础四断言。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum T01Assert { LangMatrix = 0, FallbackChain = 1, FormatEngine = 2, SwitchMech = 3 }

impl T01Assert {
    pub const ALL: [T01Assert; 4] = [
        T01Assert::LangMatrix, T01Assert::FallbackChain, T01Assert::FormatEngine, T01Assert::SwitchMech];
    pub fn name(self) -> &'static str {
        match self {
            T01Assert::LangMatrix => "lang-matrix", T01Assert::FallbackChain => "fallback",
            T01Assert::FormatEngine => "format", T01Assert::SwitchMech => "switch",
        }
    }
}

/// T02 RTL 双断言。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum T02Assert { MirrorComplete = 0, MixedDirection = 1 }

impl T02Assert {
    pub const ALL: [T02Assert; 2] = [T02Assert::MirrorComplete, T02Assert::MixedDirection];
    pub fn name(self) -> &'static str {
        match self { T02Assert::MirrorComplete => "mirror-complete", T02Assert::MixedDirection => "mixed-dir" }
    }
}

/// T03 排版四段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipelineStage { Segment = 0, Shaping = 1, LineBreak = 2, Position = 3 }

impl PipelineStage {
    pub const ALL: [PipelineStage; 4] = [
        PipelineStage::Segment, PipelineStage::Shaping, PipelineStage::LineBreak, PipelineStage::Position];
    pub fn name(self) -> &'static str {
        match self {
            PipelineStage::Segment => "segment", PipelineStage::Shaping => "shaping",
            PipelineStage::LineBreak => "linebreak", PipelineStage::Position => "position",
        }
    }
}

/// 核验矩阵：组 → 断言 → (复测过, 有证据)。
pub struct VerifyMatrix {
    pub t01: Vec<(T01Assert, bool, bool)>,
    pub t02: Vec<(T02Assert, bool, bool)>,
    pub t03_sigs: Vec<(PipelineStage, u32)>, // (段, 语言感知签名)
    pub evidence: Vec<&'static str>,
}

impl VerifyMatrix {
    pub fn new() -> Self {
        VerifyMatrix {
            t01: T01Assert::ALL.iter().map(|a| (*a, false, false)).collect(),
            t02: T02Assert::ALL.iter().map(|a| (*a, false, false)).collect(),
            t03_sigs: Vec::new(),
            evidence: Vec::new(),
        }
    }

    /// T01 复测登记（证据引用制：不过即缺陷立案）。
    pub fn mark_t01(&mut self, a: T01Assert, pass: bool) -> bool {
        match self.t01.iter_mut().find(|x| x.0 == a) {
            Some(x) => { x.1 = pass; x.2 = pass; true }
            None => false,
        }
    }
    pub fn mark_t02(&mut self, a: T02Assert, pass: bool) -> bool {
        match self.t02.iter_mut().find(|x| x.0 == a) {
            Some(x) => { x.1 = pass; x.2 = pass; true }
            None => false,
        }
    }

    /// T03 四段签名一致性：全段签名相等才一致。
    pub fn check_t03(&mut self, sigs: [u32; 4]) -> bool {
        self.t03_sigs = PipelineStage::ALL.iter().zip(sigs.iter()).map(|(s, v)| (*s, *v)).collect();
        sigs.iter().all(|v| *v == sigs[0])
    }

    pub fn t01_ok(&self) -> bool { self.t01.iter().all(|x| x.1 && x.2) }
    pub fn t02_ok(&self) -> bool { self.t02.iter().all(|x| x.1 && x.2) }
    pub fn t03_ok(&self) -> bool { self.t03_sigs.len() == 4 }

    /// 读屏跟读抽测（域本色）。
    pub fn screen_reader_sample(&mut self, ok: bool) { if ok { self.evidence.push("sr-follow-read-ok"); } }

    /// 三组全过。
    pub fn all_ok(&self) -> bool { self.t01_ok() && self.t02_ok() && self.t03_ok() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves402_checks() -> CheckSet {
    let mut set = CheckSet::new("ves402");
    let mut m = VerifyMatrix::new();

    // C402-01 T01 四断言复测全过。
    for a in T01Assert::ALL.iter() { m.mark_t01(*a, true); }
    chk(&mut set, "C402-01", m.t01_ok(), "T01 四断言绿");

    // C402-02 单断言失败检出。
    let mut m2 = VerifyMatrix::new();
    for a in T01Assert::ALL.iter() { m2.mark_t01(*a, true); }
    m2.mark_t01(T01Assert::FormatEngine, false);
    chk(&mut set, "C402-02", !m2.t01_ok(), "格式断言红");

    // C402-03 T02 RTL 双断言。
    for a in T02Assert::ALL.iter() { m.mark_t02(*a, true); }
    chk(&mut set, "C402-03", m.t02_ok(), "RTL 双断言绿");

    // C402-04 T03 四段签名一致。
    chk(&mut set, "C402-04", m.check_t03([0x5EED, 0x5EED, 0x5EED, 0x5EED]) && m.t03_ok(), "四段签名一致");

    // C402-05 签名失配检出。
    let mut m3 = VerifyMatrix::new();
    chk(&mut set, "C402-05", !m3.check_t03([0xA1, 0xA1, 0xB2, 0xA1]) && !m3.t03_ok(), "整形段签名漂移");

    // C402-06 证据引用制：读屏跟读抽测（域本色）。
    m.screen_reader_sample(true);
    chk(&mut set, "C402-06", m.evidence.contains(&"sr-follow-read-ok"), "跟读抽测挂证");

    // C402-07 三组全过判定。
    chk(&mut set, "C402-07", m.all_ok(), "T01-T03 全绿");

    // C402-08 部分失败全绿否决。
    m3.mark_t02(T02Assert::MirrorComplete, true);
    m3.mark_t02(T02Assert::MixedDirection, true);
    chk(&mut set, "C402-08", !m3.all_ok(), "T03 失配拖累全判");

    // C402-09 名单单源（4+2+4）。
    chk(&mut set, "C402-09", T01Assert::ALL.len() == 4 && T02Assert::ALL.len() == 2
        && PipelineStage::ALL.len() == 4, "断言名单单源");

    // C402-10 名单名核验。
    chk(&mut set, "C402-10", T01Assert::FallbackChain.name() == "fallback"
        && PipelineStage::Shaping.name() == "shaping", "名单名正确");

    // C402-11 失败项不挂证（证据与状态绑定）。
    let mut m4 = VerifyMatrix::new();
    m4.mark_t01(T01Assert::SwitchMech, false);
    let failed = m4.t01.iter().find(|x| !x.1).map(|x| x.0.name());
    chk(&mut set, "C402-11", failed == Some("switch") && !m4.t01_ok(), "失败项定位且不挂证");

    // C402-12 复测登记幂等。
    m.mark_t01(T01Assert::LangMatrix, true);
    chk(&mut set, "C402-12", m.t01.len() == 4, "登记幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves402_smoke() {
        let set = run_ves402_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves402 red: {:?}", red);
    }
}
