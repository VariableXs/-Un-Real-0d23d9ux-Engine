//! ves289_s13quality — VE-F4049 · T03 排版质量对拍
//! 参考基准集（四语言×典型文档×混合文档=80 场景，复用 SSIM 单源）+ 对拍指标（像素 SSIM+断行一致断言）+ 双指标门（SSIM≥0.98 且断行一致率≥99%）+ 环境指纹不可比红线。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x7BC0;

pub const SCENE_TARGET: u32 = 80;
pub const SSIM_FLOOR_PPM: u32 = 980_000; // SSIM ≥ 0.98
pub const LINEBREAK_FLOOR_PPM: u32 = 990_000; // 一致率 ≥ 99%

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum QcCode {
    Ok = 0,
    SceneShort = SEG_BASE + 0x01,
    GateBlocked = SEG_BASE + 0x02,
    ToleranceTamper = SEG_BASE + 0x03,
    FingerprintMissing = SEG_BASE + 0x04,
    SkipSilent = SEG_BASE + 0x05,
}

/// 场景：四语言 × 文档类。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SceneLang { Latin = 0, Cjk = 1, Arabic = 2, Mixed = 3 }

impl SceneLang {
    pub const ALL: [SceneLang; 4] = [SceneLang::Latin, SceneLang::Cjk, SceneLang::Arabic, SceneLang::Mixed];
}

#[derive(Clone)]
pub struct Scene {
    pub id: u32,
    pub lang: SceneLang,
    pub doc_class: String, // 典型 / 混合
    pub ssim_ppm: u32,
    pub linebreak_match: bool,
    pub skipped: bool,
}

pub struct QcBook {
    pub scenes: Vec<Scene>,
    pub fingerprint: Option<u64>,
}

impl QcBook {
    pub fn new() -> Self { QcBook { scenes: Vec::new(), fingerprint: None } }

    pub fn add(&mut self, s: Scene) -> Result<(), QcCode> {
        if self.scenes.len() >= SCENE_TARGET as usize { return Err(QcCode::SceneShort); }
        for e in self.scenes.iter() {
            if e.id == s.id { return Err(QcCode::SceneShort); }
        }
        self.scenes.push(s);
        Ok(())
    }

    /// 数量达标且四语言均衡。
    pub fn balanced(&self) -> bool {
        if (self.scenes.len() as u32) != SCENE_TARGET { return false; }
        for l in SceneLang::ALL.iter() {
            if !self.scenes.iter().any(|s| s.lang == *l) { return false; }
        }
        true
    }

    /// 双指标门：SSIM 与断行一致率（跳过场景不计入但显性）。
    pub fn gate(&self) -> Result<(u32, u32), QcCode> {
        let mut total = 0u32;
        let mut ssim_pass = 0u32;
        let mut lb_pass = 0u32;
        let mut skipped = 0u32;
        for s in self.scenes.iter() {
            if s.skipped { skipped += 1; continue; }
            total += 1;
            if s.ssim_ppm >= SSIM_FLOOR_PPM { ssim_pass += 1; }
            if s.linebreak_match { lb_pass += 1; }
        }
        if total == 0 { return Err(QcCode::SkipSilent); }
        let ssim_rate = ssim_pass * 1_000_000 / total;
        let lb_rate = lb_pass * 1_000_000 / total;
        if skipped > 0 && skipped == self.scenes.len() as u32 {
            return Err(QcCode::SkipSilent);
        }
        if ssim_rate < SSIM_FLOOR_PPM || lb_rate < LINEBREAK_FLOOR_PPM {
            return Err(QcCode::GateBlocked);
        }
        Ok((ssim_rate, lb_rate))
    }

    pub fn set_fingerprint(&mut self, fp: u64) {
        self.fingerprint = Some(fp);
    }
    /// 不可比红线：无环境指纹 → 拒绝对拍结论。
    pub fn comparable(&self) -> bool {
        self.fingerprint.is_some()
    }
    /// 容差 ADR：门限与 ADR 登记值一致（不得私改）。
    pub fn tolerance_matches(&self, adr_ssim: u32, adr_lb: u32) -> bool {
        SSIM_FLOOR_PPM == adr_ssim && LINEBREAK_FLOOR_PPM == adr_lb
    }
}

pub fn run_ves289_checks() -> CheckSet {
    let mut set = CheckSet::new("ves289_s13quality");
    let mut qb = QcBook::new();

    // 1) 构建 80 场景（四语言各 20）。
    let mut id = 1u32;
    'outer: for l in SceneLang::ALL.iter() {
        for d in ["典型", "混合"].iter() {
            for _ in 0..10 {
                let r = qb.add(Scene {
                    id,
                    lang: *l,
                    doc_class: String::from(*d),
                    ssim_ppm: 999_000,
                    linebreak_match: true,
                    skipped: false,
                });
                if r.is_err() { break 'outer; }
                id += 1;
            }
        }
    }
    set.add("C4049-01", qb.scenes.len() == 80, "80 场景");
    set.add("C4049-02", qb.balanced(), "四语言均衡");

    // 3) 超量拒。
    set.add("C4049-03", qb.add(Scene { id: 999, lang: SceneLang::Mixed, doc_class: String::from("典型"), ssim_ppm: 999_000, linebreak_match: true, skipped: false }) == Err(QcCode::SceneShort), "超量拒");

    // 4) 双门通过。
    set.add("C4049-04", qb.gate() == Ok((1_000_000, 1_000_000)), "双门通过");

    // 5) SSIM 不达阻断。
    qb.scenes[0].ssim_ppm = 900_000;
    set.add("C4049-05", qb.gate() == Err(QcCode::GateBlocked), "SSIM 不达阻断");
    qb.scenes[0].ssim_ppm = 999_000;

    // 6) 断行不一致阻断。
    qb.scenes[1].linebreak_match = false;
    set.add("C4049-06", qb.gate() == Err(QcCode::GateBlocked), "断行不达阻断");
    qb.scenes[1].linebreak_match = true;

    // 7) SKIP 显性：全跳 → SkipSilent。
    let mut qb2 = QcBook::new();
    let _ = qb2.add(Scene { id: 1, lang: SceneLang::Latin, doc_class: String::from("典型"), ssim_ppm: 999_000, linebreak_match: true, skipped: true });
    set.add("C4049-07", qb2.gate() == Err(QcCode::SkipSilent), "全跳显性");

    // 8) 容差 ADR 一致。
    set.add("C4049-08", qb.tolerance_matches(980_000, 990_000), "容差 ADR 一致");

    // 9) 容差私改检出。
    set.add("C4049-09", !qb.tolerance_matches(950_000, 990_000), "容差私改检出");

    // 10) 指纹缺失不可比。
    set.add("C4049-10", !qb.comparable(), "指纹缺失不可比");

    // 11) 指纹在册可比。
    qb.set_fingerprint(0xABCD_1234);
    set.add("C4049-11", qb.comparable(), "指纹可比");

    // 12) 常量红线。
    set.add("C4049-12", SSIM_FLOOR_PPM == 980_000 && LINEBREAK_FLOOR_PPM == 990_000 && SCENE_TARGET == 80, "常量红线");

    // 13) 段位自查。
    set.add("C4049-13", SEG_BASE == 0x7BC0, "段位 0x7BC0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_flow() {
        let mut qb = QcBook::new();
        let _ = qb.add(Scene { id: 1, lang: SceneLang::Latin, doc_class: String::from("典型"), ssim_ppm: 999_000, linebreak_match: true, skipped: false });
        let _ = qb.add(Scene { id: 2, lang: SceneLang::Cjk, doc_class: String::from("典型"), ssim_ppm: 999_000, linebreak_match: true, skipped: false });
        assert!(qb.gate().is_ok());
        qb.scenes[0].skipped = true;
        assert!(qb.gate().is_ok()); // 部分跳过仍可比
    }
}
