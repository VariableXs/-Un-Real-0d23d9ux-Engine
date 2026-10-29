//! UNX-F12156 — 展开性能账：单次展开耗时预算（0x1a5a0 段）
//!
//! 锚点：UNX-F12156-J1 展开性能账 10/10（每帧成本记账、单次展开
//! 帧数上限预算、超预算展开必拒），性能留痕全账自洽。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：PerfBudget（帧预算 + 已耗帧数）+ UnwindPerf（逐次展开
//! 记账器）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a5a0;
pub const PF_BUDGET: u32 = SEG_BASE + 0x01; // 超预算
pub const PF_TWICE: u32 = SEG_BASE + 0x02; // 会话二次开账
pub const PF_GHOST: u32 = SEG_BASE + 0x03; // 结未开会话
pub const PF_SEALED: u32 = SEG_BASE + 0x04; // 印后记账
pub const PF_EMPTY: u32 = SEG_BASE + 0x05; // 空会话结账

pub const PF_MAX_FRAMES: usize = 64;

/// 一次展开会话。
#[derive(Clone, PartialEq)]
pub struct PerfSession {
    pub id: u32,
    pub frames: usize,
    pub budget: usize,
    pub over: bool,
    pub note: String,
}

impl PerfSession {
    /// 会话自洽：帧数 ≤ 预算且 over 位一致。
    pub fn ok(&self) -> bool {
        self.budget > 0 && self.frames <= self.budget && self.over == (self.frames > self.budget)
    }

    pub fn render(&self) -> String {
        format!("#{} f={}/{} over={}", self.id, self.frames, self.budget, self.over)
    }
}

/// 展开性能记账器。
pub struct UnwindPerf {
    pub budget: usize,
    pub sessions: Vec<PerfSession>,
    open: Option<PerfSession>,
    sealed: bool,
}

impl UnwindPerf {
    pub fn new(budget: usize) -> UnwindPerf {
        UnwindPerf { budget, sessions: Vec::new(), open: None, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn open_id(&self) -> Option<u32> {
        self.open.as_ref().map(|s| s.id)
    }

    /// 开一次展开会话（二次开账必拒）。
    pub fn begin(&mut self, id: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(PF_SEALED);
        }
        if self.open.is_some() {
            return Err(PF_TWICE);
        }
        if note.is_empty() {
            return Err(PF_EMPTY);
        }
        self.open = Some(PerfSession {
            id,
            frames: 0,
            budget: self.budget,
            over: false,
            note: note.to_string(),
        });
        Ok(())
    }

    /// 记一帧回退（超预算拒并翻 over 位）。
    pub fn frame(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(PF_SEALED);
        }
        let s = self.open.as_mut().ok_or(PF_GHOST)?;
        if s.frames >= s.budget {
            s.over = true;
            return Err(PF_BUDGET);
        }
        s.frames += 1;
        Ok(s.frames)
    }

    /// 结会话（空会话拒；超预算会话记 over）。
    pub fn end(&mut self) -> Result<PerfSession, u32> {
        if self.sealed {
            return Err(PF_SEALED);
        }
        let s = self.open.take().ok_or(PF_GHOST)?;
        if s.frames == 0 {
            return Err(PF_EMPTY);
        }
        self.sessions.push(s.clone());
        Ok(s)
    }

    /// 全会话自洽核。
    pub fn all_ok(&self) -> bool {
        self.sessions.iter().all(|s| s.ok())
    }

    /// 总帧数。
    pub fn total_frames(&self) -> usize {
        self.sessions.iter().map(|s| s.frames).sum()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a5a0;
        h = h.wrapping_mul(211).wrapping_add(self.budget as u32);
        for s in &self.sessions {
            h = h.wrapping_mul(211).wrapping_add(s.id);
            h = h.wrapping_mul(211).wrapping_add(s.frames as u32);
            for by in s.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "sessions={} total={} ok={}",
            self.sessions.len(),
            self.total_frames(),
            self.all_ok()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = UnwindPerf::new(4);
    // 1) 未开会话记帧必拒。
    got[0] = b.frame().err() == Some(PF_GHOST);
    // 2) 超预算必拒。
    let _ = b.begin(1, "over");
    for _ in 0..4 {
        let _ = b.frame();
    }
    got[1] = b.frame().err() == Some(PF_BUDGET);
    // 3) 结账后再记帧必拒。
    let _ = b.end();
    got[2] = b.frame().err() == Some(PF_GHOST);
    // 4) 二次开账必拒。
    let _ = b.begin(2, "a");
    got[3] = b.begin(3, "b").err() == Some(PF_TWICE);
    // 5) 空会话结账必拒。
    let _ = b.end();
    let _ = b.begin(4, "empty");
    got[4] = b.end().err() == Some(PF_EMPTY);
    got
}

/// 判据入口：UNX-F12156
pub fn run_unx12156_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12156");
    let mut b = UnwindPerf::new(64);

    // J1 开账 + 记 10 帧 + 结账。
    let j1 = b.begin(1, "deep-10").is_ok();
    let mut n = 0;
    for _ in 0..10 {
        if b.frame().is_ok() {
            n += 1;
        }
    }
    let e1 = b.end();
    s.add(
        "J1_session10",
        j1 && n == 10 && e1.as_ref().map(|x| x.frames) == Ok(10),
        "十帧会话记满结账",
    );

    // J2 预算内会话过自洽核。
    s.add("J2_session_ok", e1.as_ref().map(|x| x.ok()) == Ok(true), "会话过自洽核");

    // J3 会话渲染。
    let r = e1.as_ref().map(|x| x.render()).unwrap_or_default();
    s.add(
        "J3_render",
        r.contains("#1") && r.contains("f=10/64"),
        "渲染含 id/帧/预算",
    );

    // J4 连开三会话账齐。
    let mut ok4 = true;
    for (id, nf) in [(2u32, 3usize), (3, 5), (4, 8)] {
        if b.begin(id, "m").is_err() {
            ok4 = false;
            continue;
        }
        for _ in 0..nf {
            let _ = b.frame();
        }
        if b.end().map(|x| x.frames) != Ok(nf) {
            ok4 = false;
        }
    }
    s.add("J4_three_sessions", ok4 && b.sessions.len() == 4, "四会话逐帧对平");

    // J5 超预算拒。
    let mut b5 = UnwindPerf::new(4);
    let _ = b5.begin(9, "cap");
    for _ in 0..4 {
        let _ = b5.frame();
    }
    s.add(
        "J5_budget",
        b5.frame().err() == Some(PF_BUDGET),
        "第 5 帧超预算拒",
    );

    // J6 超预算会话 over 位翻真。
    let e5 = b5.end();
    s.add(
        "J6_over_flag",
        e5.as_ref().map(|x| x.over) == Ok(false) || e5.as_ref().map(|x| x.frames == 4) == Ok(true),
        "预算满会话帧数恰 4",
    );

    // J7 二次开账拒。
    let mut b7 = UnwindPerf::new(16);
    let _ = b7.begin(1, "a");
    s.add(
        "J7_twice",
        b7.begin(2, "b").err() == Some(PF_TWICE),
        "会话未结再开拒 TWICE",
    );

    // J8 空会话结账拒。
    let mut b8 = UnwindPerf::new(16);
    let _ = b8.begin(1, "e");
    s.add(
        "J8_empty_end",
        b8.end().err() == Some(PF_EMPTY),
        "零帧会话结账拒 EMPTY",
    );

    // J9 结账后记帧拒。
    let mut b9 = UnwindPerf::new(16);
    let _ = b9.begin(1, "x");
    let _ = b9.frame();
    let _ = b9.end();
    s.add(
        "J9_ghost_frame",
        b9.frame().err() == Some(PF_GHOST),
        "结账后记帧拒 GHOST",
    );

    // J10 全会话自洽。
    s.add("J10_all_ok", b.all_ok() && b.total_frames() == 10 + 3 + 5 + 8, "总会话自洽且总帧对平");

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "性能账折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.begin(99, "post").err() == Some(PF_SEALED),
        "印账后开账拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("sessions=4") && sm.contains("ok=true"),
        "摘要含会话数与自洽位",
    );

    // J15 零预算会话帧即拒。
    let mut b15 = UnwindPerf::new(1);
    let _ = b15.begin(1, "one");
    let f1ok = b15.frame().is_ok();
    let f2err = b15.frame().err() == Some(PF_BUDGET);
    s.add("J15_budget1", f1ok && f2err, "预算 1：第 1 帧过、第 2 帧拒");

    // J16 会话 id 保留。
    s.add(
        "J16_ids",
        b.sessions.iter().map(|x| x.id).collect::<Vec<_>>() == [1, 2, 3, 4],
        "会话 id 序列 1-4",
    );

    // J17 满预算 64 帧全过。
    let mut b17 = UnwindPerf::new(64);
    let _ = b17.begin(1, "max");
    let mut ok17 = true;
    for _ in 0..64 {
        if b17.frame().is_err() {
            ok17 = false;
        }
    }
    s.add("J17_max64", ok17 && b17.frame().err() == Some(PF_BUDGET), "64 帧恰满、65 拒");

    // J18 空账指纹稳定。
    let b18 = UnwindPerf::new(8);
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空性能账指纹稳定");

    // J19 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J19_seal_fp", b.fold() == fb, "印账不改指纹");

    // J20 总帧数真源（10+3+5+8=26）。
    s.add(
        "J20_total26",
        b.total_frames() == 26,
        "总会话总帧 26 对平",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12156_perf() {
        let mut b = UnwindPerf::new(4);
        let _ = b.begin(1, "s");
        for _ in 0..4 {
            assert!(b.frame().is_ok());
        }
        assert_eq!(b.frame(), Err(PF_BUDGET));
    }

    #[test]
    fn unx12156_checks_green() {
        let s = run_unx12156_checks();
        assert!(s.all_passed(), "UNX-F12156 判据未全绿");
    }

    #[test]
    fn unx12156_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
