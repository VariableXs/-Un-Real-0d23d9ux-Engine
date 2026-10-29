//! UNX-F24200 vblank/HPD 内核事件面与事件队列·子功能20（G9 面收口印）（0x41AE0 段）
//!
//! 锚点：UNX-F24200-J1 印账（19 单锚逐条落账）；J2 收口语义（锚全
//! 对 + 重跑全绿 + 全绿才发印三性质）；J3 印指纹（印后账折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-F5-B15（G9 vblank/HPD 面收口印）。
//!
//! 结构：G9_UNITS 19 锚表 + 内嵌重跑核（rv_24181~rv_24199）+ 印核
//! （夹 FAIL/半覆盖/重印/印后录全拒）+ 注入核（锚错/红项/重印/印
//! 后/尾锚缺必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;

use crate::unxstar::unx24181_vblank_hpd::run_unx24181_checks;
use crate::unxstar::unx24182_vblank_hpd::run_unx24182_checks;
use crate::unxstar::unx24183_vblank_hpd::run_unx24183_checks;
use crate::unxstar::unx24184_vblank_hpd::run_unx24184_checks;
use crate::unxstar::unx24185_vblank_hpd::run_unx24185_checks;
use crate::unxstar::unx24186_vblank_hpd::run_unx24186_checks;
use crate::unxstar::unx24187_vblank_hpd::run_unx24187_checks;
use crate::unxstar::unx24188_vblank_hpd::run_unx24188_checks;
use crate::unxstar::unx24189_vblank_hpd::run_unx24189_checks;
use crate::unxstar::unx24190_vblank_hpd::run_unx24190_checks;
use crate::unxstar::unx24191_vblank_hpd::run_unx24191_checks;
use crate::unxstar::unx24192_vblank_hpd::run_unx24192_checks;
use crate::unxstar::unx24193_vblank_hpd::run_unx24193_checks;
use crate::unxstar::unx24194_vblank_hpd::run_unx24194_checks;
use crate::unxstar::unx24195_vblank_hpd::run_unx24195_checks;
use crate::unxstar::unx24196_vblank_hpd::run_unx24196_checks;
use crate::unxstar::unx24197_vblank_hpd::run_unx24197_checks;
use crate::unxstar::unx24198_vblank_hpd::run_unx24198_checks;
use crate::unxstar::unx24199_vblank_hpd::run_unx24199_checks;

/// 诊断段（0x41AE0 独占 0x20）。
pub const SEG_BASE: u32 = 0x41AE0;
pub const SL_ANCHOR_BAD: u32 = SEG_BASE + 0x01; // 锚不齐
pub const SL_RERUN_RED: u32 = SEG_BASE + 0x02; // 重跑红
pub const SL_ALREADY: u32 = SEG_BASE + 0x03; // 重印
pub const SL_FROZEN: u32 = SEG_BASE + 0x04; // 印后录
pub const SL_TAIL_MISS: u32 = SEG_BASE + 0x05; // 尾锚缺

/// G9 面 19 单锚（号，段址）。
pub const G9_UNITS: [(u16, u32); 19] = [
    (24181, 0x41880),
    (24182, 0x418A0),
    (24183, 0x418C0),
    (24184, 0x418E0),
    (24185, 0x41900),
    (24186, 0x41920),
    (24187, 0x41940),
    (24188, 0x41960),
    (24189, 0x41980),
    (24190, 0x419A0),
    (24191, 0x419C0),
    (24192, 0x419E0),
    (24193, 0x41A00),
    (24194, 0x41A20),
    (24195, 0x41A40),
    (24196, 0x41A60),
    (24197, 0x41A80),
    (24198, 0x41AA0),
    (24199, 0x41AC0),
];

/// 印面。
pub struct SlSeal {
    pub face: &'static str,
    pub fold_val: Option<u32>,
    pub sealed: bool,
}

impl SlSeal {
    pub fn new(face: &'static str) -> SlSeal {
        SlSeal {
            face,
            fold_val: None,
            sealed: false,
        }
    }

    /// 发印：锚齐 + 重跑绿 + 未印才发。
    pub fn issue(&mut self, anchor_ok: bool, rerun_ok: bool, fold: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(SL_ALREADY);
        }
        if !anchor_ok {
            return Err(SL_ANCHOR_BAD);
        }
        if !rerun_ok {
            return Err(SL_RERUN_RED);
        }
        self.sealed = true;
        self.fold_val = Some(fold);
        Ok(fold)
    }

    /// 印后录：只读。
    pub fn record(&self) -> Result<u32, u32> {
        if !self.sealed {
            return Err(SL_FROZEN);
        }
        Ok(self.fold_val.unwrap_or(0))
    }
}

/// 内嵌重跑：G9 全 19 单判据复跑，返回逐单绿标。
pub fn g9_rerun() -> [(u16, bool); 19] {
    [
        (24181, run_unx24181_checks().all_passed()),
        (24182, run_unx24182_checks().all_passed()),
        (24183, run_unx24183_checks().all_passed()),
        (24184, run_unx24184_checks().all_passed()),
        (24185, run_unx24185_checks().all_passed()),
        (24186, run_unx24186_checks().all_passed()),
        (24187, run_unx24187_checks().all_passed()),
        (24188, run_unx24188_checks().all_passed()),
        (24189, run_unx24189_checks().all_passed()),
        (24190, run_unx24190_checks().all_passed()),
        (24191, run_unx24191_checks().all_passed()),
        (24192, run_unx24192_checks().all_passed()),
        (24193, run_unx24193_checks().all_passed()),
        (24194, run_unx24194_checks().all_passed()),
        (24195, run_unx24195_checks().all_passed()),
        (24196, run_unx24196_checks().all_passed()),
        (24197, run_unx24197_checks().all_passed()),
        (24198, run_unx24198_checks().all_passed()),
        (24199, run_unx24199_checks().all_passed()),
    ]
}

/// 锚表自检：19 锚 + 连续段 + 尾锚 24199。
pub fn anchors_ok(units: &[(u16, u32)]) -> bool {
    if units.len() != G9_UNITS.len() {
        return false;
    }
    for (i, (n, seg)) in units.iter().enumerate() {
        let (en, eseg) = G9_UNITS[i];
        if *n != en || *seg != eseg {
            return false;
        }
    }
    // 段连续性
    for w in units.windows(2) {
        if w[1].1 - w[0].1 != 0x20 {
            return false;
        }
    }
    units.last().map(|(n, _)| *n == 24199).unwrap_or(false)
}

/// 标准夹具：G9 全绿印面（真重跑）。
pub fn mk_seal() -> SlSeal {
    let mut s = SlSeal::new("UNX-F5-B15/G9");
    let rr = g9_rerun();
    let all_green = rr.iter().all(|(_, ok)| *ok);
    let fold = 0x41AE0u32
        .wrapping_mul(31)
        .wrapping_add(rr.iter().filter(|(_, ok)| *ok).count() as u32);
    let _ = s.issue(anchors_ok(&G9_UNITS), all_green, fold);
    s
}

/// 注入核：五个必然被拒的坏印样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 锚不齐（错段址）
    let mut bad_anchors = G9_UNITS;
    bad_anchors[0].1 = 0x41800;
    let mut s1 = SlSeal::new("T");
    got[0] = s1.issue(anchors_ok(&bad_anchors), true, 1).err() == Some(SL_ANCHOR_BAD);
    // 2) 重跑红
    let mut s2 = SlSeal::new("T");
    got[1] = s2.issue(true, false, 1).err() == Some(SL_RERUN_RED);
    // 3) 尾锚缺
    let short = &G9_UNITS[..18];
    got[2] = !anchors_ok(short);
    // 4) 重印拒
    let mut s3 = SlSeal::new("T");
    let _ = s3.issue(true, true, 7);
    got[3] = s3.issue(true, true, 7).err() == Some(SL_ALREADY);
    // 5) 印后录（未印拒录 / 已印可录）
    got[4] = SlSeal::new("T").record().err() == Some(SL_FROZEN)
        && mk_seal().record().is_ok();
    got
}

/// 判据主体。
pub fn run_unx24200_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24200");

    // J1 锚数守恒
    let j1 = G9_UNITS.len() == 19;
    s.add("J1-锚数守恒", j1, "19 单锚");

    // J2 锚表自检
    let j2 = anchors_ok(&G9_UNITS);
    s.add("J2-锚表自检", j2, "值+连续+尾锚全对");

    // J3 内嵌重跑 19/19 绿
    let rr = g9_rerun();
    let green = rr.iter().filter(|(_, ok)| *ok).count();
    let j3 = green == 19;
    s.add_dyn("J3-重跑全绿", j3, &format!("green={}/19", green));

    // J4 首锚 24181
    let j4 = G9_UNITS[0] == (24181, 0x41880);
    s.add("J4-首锚", j4, "24181@0x41880");

    // J5 尾锚 24199
    let j5 = G9_UNITS[18] == (24199, 0x41AC0);
    s.add("J5-尾锚", j5, "24199@0x41AC0");

    // J6 段界连续
    let j6 = G9_UNITS.windows(2).all(|w| w[1].1 - w[0].1 == 0x20);
    s.add("J6-段界连续", j6, "+0x20 递进");

    // J7 全绿发印
    let seal = mk_seal();
    let j7 = seal.sealed && seal.fold_val.is_some();
    s.add("J7-全绿发印", j7, "G9 印成");

    // J8 印后录
    let j8 = seal.record().is_ok();
    s.add("J8-印后录", j8, "印发可录");

    // J9 红项拒印
    let j9 = {
        let mut s2 = SlSeal::new("T");
        s2.issue(true, false, 1).err() == Some(SL_RERUN_RED)
    };
    s.add("J9-红项拒印", j9, "重跑红必拒");

    // J10 重印拒
    let j10 = {
        let mut s2 = mk_seal();
        s2.issue(true, true, 9).err() == Some(SL_ALREADY)
    };
    s.add("J10-重印拒", j10, "重印发必拒");

    // J11 印面声明
    let j11 = seal.face == "UNX-F5-B15/G9";
    s.add("J11-印面声明", j11, "B15/G9 面");

    // J12 指纹稳定：印纹 = 折叠基式（段基×31+绿数）且录得一致
    let j12 = {
        let expect = 0x41AE0u32.wrapping_mul(31).wrapping_add(19);
        seal.fold_val == Some(expect) && seal.record() == Ok(expect)
    };
    s.add("J12-指纹稳定", j12, "印纹=折叠值");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24200_seal_flow() {
        assert!(anchors_ok(&G9_UNITS));
        let rr = g9_rerun();
        assert!(rr.iter().all(|(_, ok)| *ok), "G9 重跑有红");
        let mut s = SlSeal::new("T");
        assert_eq!(s.issue(true, false, 1), Err(SL_RERUN_RED));
        assert_eq!(s.issue(false, true, 1), Err(SL_ANCHOR_BAD));
        assert_eq!(s.issue(true, true, 42), Ok(42));
        assert_eq!(s.record(), Ok(42));
        assert_eq!(s.issue(true, true, 42), Err(SL_ALREADY));
    }

    #[test]
    fn unx24200_checks_green() {
        let s = run_unx24200_checks();
        assert!(s.all_passed(), "UNX-F24200 判据未全绿");
    }
}
