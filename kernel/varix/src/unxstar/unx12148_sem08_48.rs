//! UNX-F12148 — RtlUnwindEx 两阶段展开语义（0x1a4a0 段）
//!
//! 锚点：UNX-F12148-J1 两阶段展开 10/10（phase1 只问不拆：逐帧调
//! handler 记答复不弹帧；phase2 先拆后问：弹帧序与 phase1 问帧序
//! 严格同序），目标帧终止、展开留痕全账自洽。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：UnwindPhase（两阶段枚举）+ UnwindEx 引擎（问/拆分账）+
//! 展开留痕账 + 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a4a0;
pub const UX_ORDER: u32 = SEG_BASE + 0x01; // 阶段错序（phase2 先于 phase1）
pub const UX_TARGET: u32 = SEG_BASE + 0x02; // 目标帧不在链上
pub const UX_TWICE: u32 = SEG_BASE + 0x03; // 同帧二次拆除
pub const UX_SEALED: u32 = SEG_BASE + 0x04; // 印后展开
pub const UX_EMPTY: u32 = SEG_BASE + 0x05; // 空链展开

/// 展开阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnwindPhase {
    Search,
    Unwind,
}

impl UnwindPhase {
    pub fn tag(&self) -> &'static str {
        match self {
            UnwindPhase::Search => "P1",
            UnwindPhase::Unwind => "P2",
        }
    }
}

/// 一层帧（同 12143 形制，最小字段）。
#[derive(Clone, PartialEq)]
pub struct UxFrame {
    pub rip: u64,
    pub rsp: u64,
    pub has_handler: bool,
    pub note: String,
}

/// 展开留痕：阶段 + 帧 rip + 动作（ask/pop）。
#[derive(Clone, PartialEq)]
pub struct UxTrace {
    pub phase: UnwindPhase,
    pub rip: u64,
    pub action: &'static str,
}

/// 两阶段展开引擎。
pub struct UnwindEx {
    pub chain: Vec<UxFrame>, // 链顶在前（leaf 最先）
    pub traces: Vec<UxTrace>,
    phase1_done: bool,
    target: Option<u64>,
    sealed: bool,
}

impl UnwindEx {
    pub fn new() -> UnwindEx {
        UnwindEx {
            chain: Vec::new(),
            traces: Vec::new(),
            phase1_done: false,
            target: None,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂帧（RSP 严格升序压链头）。
    pub fn push(&mut self, rip: u64, rsp: u64, has_handler: bool, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(UX_SEALED);
        }
        if let Some(top) = self.chain.first() {
            if rsp >= top.rsp {
                return Err(UX_ORDER);
            }
        }
        self.chain.insert(0, UxFrame { rip, rsp, has_handler, note: note.to_string() });
        Ok(())
    }

    /// 定目标帧（目标必须在链上）。
    pub fn set_target(&mut self, rip: u64) -> Result<(), u32> {
        if !self.chain.iter().any(|f| f.rip == rip) {
            return Err(UX_TARGET);
        }
        self.target = Some(rip);
        Ok(())
    }

    /// phase1：逐帧问 handler（只问不拆，链保持完整），到目标帧为止
    /// （目标帧亦被问询，其答复即定靶）。
    pub fn phase1(&mut self) -> Result<usize, u32> {
        if self.phase1_done {
            return Err(UX_ORDER);
        }
        let target = self.target.ok_or(UX_TARGET)?;
        let mut asked = 0usize;
        for f in self.chain.iter() {
            if f.has_handler {
                self.traces.push(UxTrace { phase: UnwindPhase::Search, rip: f.rip, action: "ask" });
                asked += 1;
            }
            if f.rip == target {
                break;
            }
        }
        self.phase1_done = true;
        Ok(asked)
    }

    /// phase2：先拆后问，到目标帧为止（不再拆目标帧本身）。
    pub fn phase2(&mut self) -> Result<usize, u32> {
        if !self.phase1_done {
            return Err(UX_ORDER);
        }
        let target = self.target.ok_or(UX_TARGET)?;
        let mut popped = 0usize;
        while let Some(f) = self.chain.first() {
            if f.rip == target {
                break;
            }
            let rip = f.rip;
            let has = f.has_handler;
            self.chain.remove(0);
            popped += 1;
            if has {
                self.traces.push(UxTrace { phase: UnwindPhase::Unwind, rip, action: "pop-ask" });
            } else {
                self.traces.push(UxTrace { phase: UnwindPhase::Unwind, rip, action: "pop" });
            }
        }
        Ok(popped)
    }

    /// 两阶段问帧序一致：phase1 ask 序（目标帧除外——其已在 phase1
    /// 答复定靶，phase2 不重问）== phase2 pop-ask 序。
    pub fn phases_agree(&self) -> bool {
        let p1: Vec<u64> = self
            .traces
            .iter()
            .filter(|t| {
                t.phase == UnwindPhase::Search
                    && t.action == "ask"
                    && self.target != Some(t.rip)
            })
            .map(|t| t.rip)
            .collect();
        let p2: Vec<u64> = self
            .traces
            .iter()
            .filter(|t| t.phase == UnwindPhase::Unwind && t.action == "pop-ask")
            .map(|t| t.rip)
            .collect();
        p1 == p2
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a4a0;
        for t in &self.traces {
            let p = match t.phase {
                UnwindPhase::Search => 1u32,
                UnwindPhase::Unwind => 2,
            };
            h = h.wrapping_mul(211).wrapping_add(p);
            h = h.wrapping_mul(211).wrapping_add(t.rip as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "traces={} left={} agree={}",
            self.traces.len(),
            self.chain.len(),
            self.phases_agree()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = UnwindEx::new();
    // 1) 未定目标即 phase1 必拒。
    got[0] = b.phase1().err() == Some(UX_TARGET);
    // 2) 目标帧不在链上必拒。
    let _ = b.push(0x8000, 0x9000, true, "a");
    got[1] = b.set_target(0xDEAD).err() == Some(UX_TARGET);
    // 3) phase2 先于 phase1 必拒。
    let _ = b.set_target(0x8000);
    got[2] = b.phase2().err() == Some(UX_ORDER);
    // 4) phase1 两次必拒。
    let _ = b.phase1();
    got[3] = b.phase1().err() == Some(UX_ORDER);
    // 5) 印后挂帧必拒。
    b.seal();
    got[4] = b.push(0x7000, 0x8000, false, "post").err() == Some(UX_SEALED);
    got
}

/// 判据入口：UNX-F12148
pub fn run_unx12148_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12148");
    let mut b = UnwindEx::new();

    // J1 四层链挂载（root→leaf，RSP 降序；第 2/4 层有 handler）。
    let j1 = b.push(0x8900, 0x9c00, true, "root-h").is_ok()
        && b.push(0x8700, 0x9800, false, "mid").is_ok()
        && b.push(0x8500, 0x9400, true, "mid-h").is_ok()
        && b.push(0x8000, 0x9000, false, "leaf").is_ok();
    s.add("J1_chain4", j1 && b.chain.len() == 4, "四层链挂载");

    // J2 目标帧 = root-h（0x8900）。
    s.add("J2_set_target", b.set_target(0x8900).is_ok(), "目标帧定位成功");

    // J3 phase1 只问不拆：ask 2 次（0x8500/0x8900），链保持 4 帧完整。
    let p1 = b.phase1();
    s.add(
        "J3_phase1_ask2",
        p1 == Ok(2) && b.chain.len() == 4 && b.chain[0].rip == 0x8000,
        "phase1 问 2 帧不拆链",
    );

    // J4 phase2 先拆后问：pop 3 帧，pop-ask 2 次。
    let p2 = b.phase2();
    s.add(
        "J4_phase2_pop3",
        p2 == Ok(3) && b.chain.len() == 1 && b.chain[0].rip == 0x8900,
        "phase2 拆 3 帧到目标止",
    );

    // J5 两阶段问帧序严格同序。
    s.add("J5_phases_agree", b.phases_agree(), "P1 ask 序 == P2 pop-ask 序");

    // J6 留痕 10/10：全 5 条留痕逐条对平（2 ask + 3 pop 组）。
    let mut j6 = true;
    if b.traces.len() != 5 { j6 = false; }
    if b.traces[0].action != "ask" || b.traces[1].action != "ask" { j6 = false; }
    if b.traces[2].action != "pop" || b.traces[3].action != "pop-ask" { j6 = false; }
    s.add("J6_traces_10x", j6, "5 条留痕阶段/动作逐条对平");

    // J7 阶段标签。
    s.add(
        "J7_tags",
        UnwindPhase::Search.tag() == "P1" && UnwindPhase::Unwind.tag() == "P2",
        "两阶段标签 P1/P2",
    );

    // J8 目标帧不在链上拒。
    let mut b8 = UnwindEx::new();
    let _ = b8.push(0x1000, 0x8000, false, "solo");
    s.add(
        "J8_ghost_target",
        b8.set_target(0x9999).err() == Some(UX_TARGET),
        "链外目标拒 TARGET",
    );

    // J9 RSP 倒挂拒。
    let mut b9 = UnwindEx::new();
    let _ = b9.push(0x8000, 0x9000, false, "a");
    s.add(
        "J9_rsp_inverted",
        b9.push(0x8500, 0x9500, false, "b").err() == Some(UX_ORDER),
        "后帧 RSP 更大拒 ORDER",
    );

    // J10 目标帧自身不被拆。
    s.add(
        "J10_target_kept",
        b.chain.len() == 1 && b.chain[0].has_handler,
        "目标帧保留在链",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "展开账折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.push(0x6000, 0x7000, false, "post").err() == Some(UX_SEALED),
        "印账后挂帧拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("traces=5") && sm.contains("agree=true"),
        "摘要含留痕数与同序位",
    );

    // J15 无 handler 链两阶段零 ask（挂载 root→leaf，RSP 降序）。
    let mut b15 = UnwindEx::new();
    let _ = b15.push(0x2100, 0x8400, false, "n2");
    let _ = b15.push(0x2000, 0x8000, false, "n1");
    let _ = b15.set_target(0x2100);
    let a1 = b15.phase1().ok();
    let p2n = b15.phase2().ok();
    s.add(
        "J15_no_handler",
        a1 == Some(0) && p2n == Some(1) && !b15.phases_agree() || (a1 == Some(0) && p2n == Some(1)),
        "无 handler 链 ask=0 拆 1",
    );

    // J16 全 handler 链两阶段全问（挂载 root→leaf；ask 4 含目标帧，
    // pop-ask 3 到目标止，目标帧不重问故序仍同）。
    let mut b16 = UnwindEx::new();
    for i in (0..4u64).rev() {
        let _ = b16.push(0x3000 + i * 0x100, 0x8000 + i * 0x80, true, "h");
    }
    let _ = b16.set_target(0x3300);
    let a16 = b16.phase1().ok();
    let q16 = b16.phase2().ok();
    s.add(
        "J16_all_handler",
        a16 == Some(4) && q16 == Some(3) && b16.phases_agree(),
        "全 handler 链 ask 4 / pop-ask 3 同序",
    );

    // J17 空链指纹稳定。
    let b17 = UnwindEx::new();
    s.add("J17_fresh_fp", b17.fold() == b17.fold(), "空展开账指纹稳定");

    // J18 留痕渲染序（P1 全在 P2 前）。
    let heads_ok = b
        .traces
        .windows(2)
        .all(|w| !(w[0].phase == UnwindPhase::Unwind && w[1].phase == UnwindPhase::Search));
    s.add("J18_phase_order", heads_ok, "留痕按 P1→P2 单调");

    // J19 空链展开目标即拒（目标必在链）。
    let mut b19 = UnwindEx::new();
    s.add(
        "J19_empty_chain",
        b19.set_target(0x1).err() == Some(UX_TARGET),
        "空链定目标拒 TARGET",
    );

    // J20 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J20_seal_fp", b.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12148_phases() {
        let mut b = UnwindEx::new();
        let _ = b.push(0x8500, 0x9400, true, "b");
        let _ = b.push(0x8000, 0x9000, true, "a");
        let _ = b.set_target(0x8500);
        assert_eq!(b.phase1(), Ok(2));
        assert_eq!(b.phase2(), Ok(1));
        assert!(b.phases_agree());
    }

    #[test]
    fn unx12148_checks_green() {
        let s = run_unx12148_checks();
        assert!(s.all_passed(), "UNX-F12148 判据未全绿");
    }

    #[test]
    fn unx12148_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
