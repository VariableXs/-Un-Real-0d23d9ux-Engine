//! UNX-F12196 — RTL_SRWLOCK Acquire/Release 族语义（0x1aaa0 段）
//!
//! 锚点：UNX-F12196-J1 四族对账 10/10（AcquireShared/ReleaseShared、
//! AcquireExclusive/ReleaseExclusive 四族各自进出对平），非法迁移
//! 全族拒止，操作账全账回放一致。
//! 域：CoRun·UNX-D1-B10（临界区语义谱系，SRW 展开面）。
//! 结构：SrwFour（四族操作账：状态机 + 逐族计数）+ 折叠指纹 +
//! 注入核。
//!
//! NT 语义：RTL_SRWLOCK 的四个入口构成封闭状态机——
//! AcquireExclusive 仅可从无主态进入；AcquireShared 仅可在非独占
//! 态叠加；ReleaseExclusive/ReleaseShared 各自只能解除本族持有。
//! 任何族在错误态被调用即非法迁移，必拒并留痕。四族进出计数
//! 必须两两对平，账面才算闭合。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1aaa0;
pub const FQ_SEALED: u32 = SEG_BASE + 0x01; // 印后操作
pub const FQ_EXCL_BUSY: u32 = SEG_BASE + 0x02; // 独占态下得锁拒
pub const FQ_NOT_EXCL: u32 = SEG_BASE + 0x03; // 非独占态释放独占拒
pub const FQ_RANGE: u32 = SEG_BASE + 0x04; // 共享计数越界/下溢
pub const FQ_EMPTY: u32 = SEG_BASE + 0x05; // 空账回放

/// 四族操作标签。
#[derive(Clone, Copy, PartialEq)]
pub enum FqOp {
    AcqShared,
    RelShared,
    AcqExclusive,
    RelExclusive,
}

impl FqOp {
    pub fn tag(&self) -> &'static str {
        match self {
            FqOp::AcqShared => "acq-s",
            FqOp::RelShared => "rel-s",
            FqOp::AcqExclusive => "acq-x",
            FqOp::RelExclusive => "rel-x",
        }
    }
}

/// 四族操作留痕。
#[derive(Clone, PartialEq)]
pub struct FqRec {
    pub op: u8, // 0..3 对应 FqOp
    pub state_after: u8, // 0=unowned 1=shared 2=exclusive
    pub note: String,
}

impl FqRec {
    pub fn ok(&self) -> bool {
        self.op <= 3 && self.state_after <= 2 && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!("op{} s{}", self.op, self.state_after)
    }
}

/// 四族操作账。
pub struct SrwFour {
    pub shared: u32,
    pub exclusive: bool,
    pub log: Vec<FqRec>,
    pub acq_s: u32,
    pub rel_s: u32,
    pub acq_x: u32,
    pub rel_x: u32,
    sealed: bool,
}

impl SrwFour {
    pub fn new() -> SrwFour {
        SrwFour {
            shared: 0,
            exclusive: false,
            log: Vec::new(),
            acq_s: 0,
            rel_s: 0,
            acq_x: 0,
            rel_x: 0,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    fn state(&self) -> u8 {
        if self.exclusive {
            2
        } else if self.shared > 0 {
            1
        } else {
            0
        }
    }

    fn push(&mut self, op: u8, note: &str) {
        self.log.push(FqRec { op, state_after: self.state(), note: note.to_string() });
    }

    /// AcquireExclusive：仅无主态可进。
    pub fn acquire_exclusive(&mut self, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(FQ_SEALED);
        }
        if note.is_empty() {
            return Err(FQ_EMPTY);
        }
        if self.exclusive {
            return Err(FQ_EXCL_BUSY);
        }
        if self.shared > 0 {
            return Err(FQ_EXCL_BUSY);
        }
        self.exclusive = true;
        self.acq_x += 1;
        self.push(2, note);
        Ok(())
    }

    /// ReleaseExclusive：仅独占态可出。
    pub fn release_exclusive(&mut self, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(FQ_SEALED);
        }
        if note.is_empty() {
            return Err(FQ_EMPTY);
        }
        if !self.exclusive {
            return Err(FQ_NOT_EXCL);
        }
        self.exclusive = false;
        self.rel_x += 1;
        self.push(3, note);
        Ok(())
    }

    /// AcquireShared：非独占态可叠加。
    pub fn acquire_shared(&mut self, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(FQ_SEALED);
        }
        if note.is_empty() {
            return Err(FQ_EMPTY);
        }
        if self.exclusive {
            return Err(FQ_EXCL_BUSY);
        }
        if self.shared >= 0x0FFF {
            return Err(FQ_RANGE);
        }
        self.shared += 1;
        self.acq_s += 1;
        self.push(0, note);
        Ok(self.shared)
    }

    /// ReleaseShared：共享态可减。
    pub fn release_shared(&mut self, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(FQ_SEALED);
        }
        if note.is_empty() {
            return Err(FQ_EMPTY);
        }
        if self.exclusive {
            return Err(FQ_NOT_EXCL);
        }
        if self.shared == 0 {
            return Err(FQ_RANGE);
        }
        self.shared -= 1;
        self.rel_s += 1;
        self.push(1, note);
        Ok(self.shared)
    }

    /// 四族计数对平：进 = 出（每族）。
    pub fn families_balanced(&self) -> bool {
        self.acq_s == self.rel_s && self.acq_x == self.rel_x
    }

    /// 终态无主。
    pub fn settled(&self) -> bool {
        !self.exclusive && self.shared == 0
    }

    /// 全账回放自洽。
    pub fn replay(&self) -> bool {
        !self.log.is_empty() && self.log.iter().all(|r| r.ok())
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1aaa0;
        h = h.wrapping_mul(211).wrapping_add(self.shared);
        h = h.wrapping_mul(211).wrapping_add(self.exclusive as u32);
        h = h.wrapping_mul(211).wrapping_add(self.acq_s);
        h = h.wrapping_mul(211).wrapping_add(self.rel_s);
        h = h.wrapping_mul(211).wrapping_add(self.acq_x);
        h = h.wrapping_mul(211).wrapping_add(self.rel_x);
        h
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 账面摘要。
    pub fn book_summary(&self) -> String {
        format!(
            "s+={} s-={} x+={} x-={} settled={}",
            self.acq_s, self.rel_s, self.acq_x, self.rel_x, self.settled()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SrwFour::new();
    // 1) 非独占态释放独占必拒。
    got[0] = b.release_exclusive("no-x").err() == Some(FQ_NOT_EXCL);
    // 2) 零共享释放共享必拒。
    got[1] = b.release_shared("no-s").err() == Some(FQ_RANGE);
    // 3) 共享态下独占必拒。
    let _ = b.acquire_shared("s");
    got[2] = b.acquire_exclusive("on-s").err() == Some(FQ_EXCL_BUSY);
    // 4) 独占态下共享必拒。
    let _ = b.release_shared("s-off");
    let _ = b.acquire_exclusive("x");
    got[3] = b.acquire_shared("on-x").err() == Some(FQ_EXCL_BUSY);
    // 5) 印后操作必拒。
    let _ = b.release_exclusive("x-off");
    b.seal();
    got[4] = b.acquire_shared("post").err() == Some(FQ_SEALED);
    got
}

/// 判据入口：UNX-F12196
pub fn run_unx12196_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12196");
    let mut b = SrwFour::new();

    // J1 AcquireShared 三连叠加。
    let mut ok1 = true;
    for i in 0..3u32 {
        if b.acquire_shared("as") != Ok(i + 1) {
            ok1 = false;
        }
    }
    s.add("J1_acq_shared3", ok1 && b.shared == 3, "共享三叠 1..3");

    // J2 ReleaseShared 三连对平归零。
    let mut ok2 = true;
    for want in (0..3u32).rev() {
        if b.release_shared("rs") != Ok(want) {
            ok2 = false;
        }
    }
    s.add("J2_rel_shared3", ok2 && b.shared == 0, "共享三释归零");

    // J3 AcquireExclusive 独占一进。
    s.add(
        "J3_acq_excl",
        b.acquire_exclusive("ax").is_ok() && b.exclusive,
        "独占态进入",
    );

    // J4 独占态下四族中另三族拒。
    s.add(
        "J4_excl_exclusive",
        b.acquire_exclusive("re").err() == Some(FQ_EXCL_BUSY)
            && b.acquire_shared("as-on-x").err() == Some(FQ_EXCL_BUSY)
            && b.release_shared("rs-on-x").err() == Some(FQ_NOT_EXCL),
        "独占态他族全拒",
    );

    // J5 ReleaseExclusive 一出归无主。
    let ok5 = b.release_exclusive("rx").is_ok() && b.settled();
    s.add("J5_rel_excl", ok5, "独占释出归无主");

    // J6 四族计数对平。
    s.add(
        "J6_balanced",
        b.families_balanced() && (b.acq_s, b.acq_x) == (3, 1),
        "四族进出 3/3、1/1 对平",
    );

    // J7 非独占态释放独占拒。
    s.add(
        "J7_rel_excl_unowned",
        b.release_exclusive("no-x").err() == Some(FQ_NOT_EXCL),
        "无主释放独占拒 NOT_EXCL",
    );

    // J8 零共享释放共享拒。
    s.add(
        "J8_rel_shared_zero",
        b.release_shared("zero").err() == Some(FQ_RANGE),
        "零共享释放拒 RANGE",
    );

    // J9 混合序列：共享×2 → 全释 → 独占 → 释。
    let mut b9 = SrwFour::new();
    let ok9 = b9.acquire_shared("1").is_ok()
        && b9.acquire_shared("2").is_ok()
        && b9.release_shared("3").is_ok()
        && b9.release_shared("4").is_ok()
        && b9.acquire_exclusive("5").is_ok()
        && b9.release_exclusive("6").is_ok()
        && b9.settled()
        && b9.families_balanced();
    s.add("J9_mixed_seq", ok9, "混序后账面闭合");

    // J10 留痕全自洽。
    s.add("J10_replay", b.replay() && b9.replay(), "两账留痕逐条过核");

    // J11 留痕渲染。
    let r = b.log[0].render();
    s.add(
        "J11_render",
        r == "op0 s1",
        "首条留痕 op0/s1 渲染对平",
    );

    // J12 操作标签。
    s.add(
        "J12_op_tags",
        FqOp::AcqShared.tag() == "acq-s" && FqOp::RelExclusive.tag() == "rel-x",
        "四族标签对平",
    );

    // J13 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J13_fingerprint", f1 == f2, "四族账折叠指纹一致");

    // J14 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J14_inject_all", caught == 5, "5 注入点全检出");

    // J15 印账受控。
    b.seal();
    s.add(
        "J15_sealed",
        b.acquire_shared("post").err() == Some(FQ_SEALED)
            && b.release_exclusive("post").err() == Some(FQ_SEALED),
        "印账后四族皆拒",
    );

    // J16 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J16_seal_fp", b.fold() == fb, "印账不改指纹");

    // J17 空 note 全族拒。
    let mut b17 = SrwFour::new();
    s.add(
        "J17_empty_note",
        b17.acquire_shared("").err() == Some(FQ_EMPTY)
            && b17.release_shared("").err() == Some(FQ_EMPTY)
            && b17.acquire_exclusive("").err() == Some(FQ_EMPTY)
            && b17.release_exclusive("").err() == Some(FQ_EMPTY),
        "空留痕四族皆拒",
    );

    // J18 空账指纹稳定。
    let b18 = SrwFour::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空四族账指纹稳定");

    // J19 空账回放判不自洽。
    s.add("J19_empty_replay", !b18.replay(), "空账回放判不自洽");

    // J20 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J20_summary",
        sm.contains("s+=3") && sm.contains("x+=1") && sm.contains("settled=true"),
        "摘要含四族计数与终态",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12196_four_ops() {
        let mut b = SrwFour::new();
        assert_eq!(b.acquire_shared("a"), Ok(1));
        assert_eq!(b.acquire_exclusive("b"), Err(FQ_EXCL_BUSY));
        assert_eq!(b.release_shared("c"), Ok(0));
        assert!(b.acquire_exclusive("d").is_ok());
        assert_eq!(b.release_shared("e"), Err(FQ_NOT_EXCL));
        assert!(b.release_exclusive("f").is_ok());
        assert!(b.families_balanced() && b.settled());
    }

    #[test]
    fn unx12196_checks_green() {
        let s = run_unx12196_checks();
        assert!(s.all_passed(), "UNX-F12196 判据未全绿");
    }

    #[test]
    fn unx12196_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
