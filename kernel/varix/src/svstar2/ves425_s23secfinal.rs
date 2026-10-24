//! ves425_s23secfinal — VE-F4185 · T 域安全终审
//! 终审：T 域安全三面终审（机翻出站面/语言包供应链面/地区数据路由面——F4169 复审升级为终审）
//! +红线全量复核+用户内容不出站专项断言；
//! 面未清→阻断收口；红线破→最高级立案（复述红线实测）；断言缺→补立；
//! 安全终审与隐私红线并审（出站+驻留双核心断言——域本色）；无新增面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x10A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SfCode {
    Ok = 0,
    FaceUnclear = SEG_BASE + 0x01,   // 0x10A1
    RedlineBreach = SEG_BASE + 0x02, // 0x10A2
    AssertMissing = SEG_BASE + 0x03, // 0x10A3
}

/// 安全三面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecFace { MtOutbound = 0, LpSupplyChain = 1, RegionRouting = 2 }

impl SecFace {
    pub const ALL: [SecFace; 3] = [SecFace::MtOutbound, SecFace::LpSupplyChain, SecFace::RegionRouting];
    pub fn name(self) -> &'static str {
        match self {
            SecFace::MtOutbound => "mt-outbound", SecFace::LpSupplyChain => "lp-supply",
            SecFace::RegionRouting => "region-routing",
        }
    }
}

/// 红线双核心（域本色并审）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedlineCore { Outbound = 0, Residency = 1 }

/// 安全终审册。
pub struct SecFinalBook {
    pub face_clear: Vec<SecFace>,        // 已清面
    pub redlines_ok: Vec<RedlineCore>,   // 复核通过红线
    pub asserts: Vec<&'static str>,      // 专项断言集
    pub filed_critical: bool,            // 最高级立案位
}

impl SecFinalBook {
    pub fn new() -> Self {
        SecFinalBook { face_clear: Vec::new(), redlines_ok: Vec::new(), asserts: Vec::new(), filed_critical: false }
    }
    pub fn clear_face(&mut self, f: SecFace) { if !self.face_clear.contains(&f) { self.face_clear.push(f); } }
    pub fn pass_redline(&mut self, r: RedlineCore) { if !self.redlines_ok.contains(&r) { self.redlines_ok.push(r); } }
    pub fn add_assert(&mut self, name: &'static str) { if !self.asserts.contains(&name) { self.asserts.push(name); } }
    /// 红线破→最高级立案（实测触发位）。
    pub fn breach(&mut self) { self.filed_critical = true; }
    /// 面未清→阻断收口。
    pub fn faces_block(&self) -> Option<SfCode> {
        for f in SecFace::ALL {
            if !self.face_clear.contains(&f) { return Some(SfCode::FaceUnclear); }
        }
        None
    }
    /// 断言缺→补立：专项断言集必须含「不出站」与「驻留延续」两条。
    pub fn asserts_complete(&self) -> bool {
        self.asserts.contains(&"user-content-no-outbound") && self.asserts.contains(&"residency-continuity")
    }
    /// 终审裁决：三面清+双红线过+断言齐+无立案。
    pub fn verdict(&self) -> Result<(), SfCode> {
        if let Some(c) = self.faces_block() { return Err(c); }
        if self.filed_critical { return Err(SfCode::RedlineBreach); }
        if !self.asserts_complete() { return Err(SfCode::AssertMissing); }
        if self.redlines_ok.len() < 2 { return Err(SfCode::RedlineBreach); }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves425_checks() -> CheckSet {
    let mut set = CheckSet::new("ves425");
    let mut bk = SecFinalBook::new();

    // C425-01 三面未清→阻断收口。
    chk(&mut set, "C425-01", bk.verdict() == Err(SfCode::FaceUnclear), "面未清阻断");

    // C425-02 三面清后仍因断言缺被拒（补立路径）。
    for f in SecFace::ALL { bk.clear_face(f); }
    chk(&mut set, "C425-02", bk.verdict() == Err(SfCode::AssertMissing), "断言缺拒");

    // C425-03 断言缺定位。
    chk(&mut set, "C425-03", !bk.asserts_complete(), "缺两条专项断言");

    // C425-04 补立断言后因红线未过被拒。
    bk.add_assert("user-content-no-outbound");
    chk(&mut set, "C425-04", bk.verdict() == Err(SfCode::AssertMissing), "仍缺驻留断言");

    // C425-05 断言齐+双红线过→终审通过。
    bk.add_assert("residency-continuity");
    bk.pass_redline(RedlineCore::Outbound);
    bk.pass_redline(RedlineCore::Residency);
    chk(&mut set, "C425-05", bk.verdict().is_ok(), "终审通过");

    // C425-06 红线破→最高级立案+终审翻转。
    bk.breach();
    chk(&mut set, "C425-06", bk.verdict() == Err(SfCode::RedlineBreach) && bk.filed_critical, "红线破立案");

    // C425-07 三面名单单源。
    chk(&mut set, "C425-07", SecFace::ALL.len() == 3
        && SecFace::ALL[0].name() == "mt-outbound"
        && SecFace::ALL[1].name() == "lp-supply"
        && SecFace::ALL[2].name() == "region-routing", "三面单源");

    // C425-08 面清幂等（重复清不重复入册）。
    let mut bk2 = SecFinalBook::new();
    bk2.clear_face(SecFace::MtOutbound);
    bk2.clear_face(SecFace::MtOutbound);
    chk(&mut set, "C425-08", bk2.face_clear.len() == 1, "面清幂等");

    // C425-09 双红线单源断言。
    chk(&mut set, "C425-09", RedlineCore::Outbound as u8 == 0 && RedlineCore::Residency as u8 == 1, "双核心单源");

    // C425-10 缺单红线终审拒。
    let mut bk3 = SecFinalBook::new();
    for f in SecFace::ALL { bk3.clear_face(f); }
    bk3.add_assert("user-content-no-outbound");
    bk3.add_assert("residency-continuity");
    bk3.pass_redline(RedlineCore::Outbound);
    chk(&mut set, "C425-10", bk3.verdict() == Err(SfCode::RedlineBreach), "缺红线拒");

    // C425-11 断言补立幂等。
    bk.add_assert("user-content-no-outbound");
    chk(&mut set, "C425-11", bk.asserts.len() == 2, "断言幂等");

    // C425-12 无新增面：终审不引入第四面。
    chk(&mut set, "C425-12", SecFace::ALL.iter().all(|f| bk.face_clear.contains(f))
        && bk.face_clear.len() == 3, "无新增面");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves425_smoke() {
        let set = run_ves425_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves425 red: {:?}", red);
    }
}
