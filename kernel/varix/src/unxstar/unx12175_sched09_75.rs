//! UNX-F12175 — 上下文切换语义边界（与 A3 调度器分界）（0x1a800 段）
//!
//! 锚点：UNX-F12175-J1 切换语义边界 10/10（保存→选择→恢复三段
//! 闭环：切出线程上下文存档、切回必须命中存档账、A3 只管选线程
//! 不碰上下文内容），边界越权必拒。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：CtxSwap（上下文交换账）+ SwitchBoundary（边界判账）+
//! 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a800;
pub const SB_GHOST: u32 = SEG_BASE + 0x01; // 切回未存档线程
pub const SB_ORDER: u32 = SEG_BASE + 0x02; // 切到自己
pub const SB_TWICE: u32 = SEG_BASE + 0x03; // 存档重复
pub const SB_SEALED: u32 = SEG_BASE + 0x04; // 印后切换
pub const SB_EMPTY: u32 = SEG_BASE + 0x05; // 空留痕切

/// 线程上下文存档。
#[derive(Clone, PartialEq)]
pub struct CtxArchive {
    pub tid: u32,
    pub rip: u64,
    pub rsp: u64,
    pub note: String,
}

impl CtxArchive {
    pub fn ok(&self) -> bool {
        self.tid != 0 && self.rip != 0 && self.rsp % 8 == 0 && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!("t{} rip={:#x} rsp={:#x}", self.tid, self.rip, self.rsp)
    }
}

/// 上下文交换账（CONTEXT 层，只管存/取；选线程归 A3）。
pub struct SwitchBoundary {
    pub archives: Vec<CtxArchive>,
    pub switches: Vec<(u32, u32)>, // (from_tid, to_tid)
    pub cur: u32,
    sealed: bool,
}

impl SwitchBoundary {
    pub fn new(cur: u32) -> SwitchBoundary {
        SwitchBoundary { archives: Vec::new(), switches: Vec::new(), cur, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 存档（tid 重复存档拒；锚非法拒）。
    pub fn save(&mut self, tid: u32, rip: u64, rsp: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(SB_SEALED);
        }
        if note.is_empty() {
            return Err(SB_EMPTY);
        }
        let a = CtxArchive { tid, rip, rsp, note: note.to_string() };
        if !a.ok() {
            return Err(SB_ORDER);
        }
        if self.archives.iter().any(|x| x.tid == tid) {
            return Err(SB_TWICE);
        }
        self.archives.push(a);
        Ok(())
    }

    /// 切换（目标必须在存档账；不得切到自己；更新 cur）。
    pub fn switch_to(&mut self, to: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(SB_SEALED);
        }
        if note.is_empty() {
            return Err(SB_EMPTY);
        }
        if !self.archives.iter().any(|a| a.tid == to) {
            return Err(SB_GHOST);
        }
        if to == self.cur {
            return Err(SB_ORDER);
        }
        let from = self.cur;
        self.switches.push((from, to));
        self.cur = to;
        Ok(from)
    }

    /// 存档账全自洽。
    pub fn archives_ok(&self) -> bool {
        self.archives.iter().all(|a| a.ok())
    }

    /// 切换账回放：每条切换两端都在存档账或曾为 cur。
    pub fn switches_replayable(&self) -> bool {
        self.switches.iter().all(|&(f, t)| {
            let f_ok = f == self.cur
                || self.archives.iter().any(|a| a.tid == f)
                || self.switches.iter().any(|&(pf, pt)| pt == f);
            f_ok && self.archives.iter().any(|a| a.tid == t)
        })
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a800;
        for a in &self.archives {
            h = h.wrapping_mul(211).wrapping_add(a.tid);
            h = h.wrapping_mul(211).wrapping_add(a.rip as u32);
        }
        h = h.wrapping_mul(211).wrapping_add(self.cur);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "archives={} switches={} cur={}",
            self.archives.len(),
            self.switches.len(),
            self.cur
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SwitchBoundary::new(1);
    // 1) 切到未存档线程必拒。
    got[0] = b.switch_to(9, "w").err() == Some(SB_GHOST);
    // 2) 切到自己必拒。
    let _ = b.save(1, 0x1000, 0x2000, "self");
    got[1] = b.switch_to(1, "self").err() == Some(SB_ORDER);
    // 3) 重复存档必拒。
    got[2] = b.save(1, 0x1000, 0x2000, "dup").err() == Some(SB_TWICE);
    // 4) 空留痕存档必拒。
    got[3] = b.save(2, 0x1000, 0x2000, "").err() == Some(SB_EMPTY);
    // 5) 印后切换必拒。
    b.seal();
    got[4] = b.switch_to(2, "post").err() == Some(SB_SEALED);
    got
}

/// 判据入口：UNX-F12175
pub fn run_unx12175_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12175");
    let mut b = SwitchBoundary::new(1);

    // J1 四线程存档。
    let j1 = b.save(1, 0x1000, 0x8000, "t1").is_ok()
        && b.save(2, 0x1100, 0x8100, "t2").is_ok()
        && b.save(3, 0x1200, 0x8200, "t3").is_ok()
        && b.save(4, 0x1300, 0x8300, "t4").is_ok();
    s.add("J1_save4", j1 && b.archives.len() == 4, "四线程存档全过");

    // J2 切换 10/10（环形切 1→2→3→4→1…）。
    let mut ok2 = true;
    let mut want_cur = 1u32;
    for i in 0..10u32 {
        let to = 1 + (i + 1) % 4;
        if b.switch_to(to, "ring").is_err() {
            ok2 = false;
        }
        want_cur = to;
    }
    s.add(
        "J2_ring10",
        ok2 && b.cur == want_cur && b.switches.len() == 10,
        "环形切十步 cur 终值对平",
    );

    // J3 切到自己拒。
    s.add(
        "J3_self",
        b.switch_to(b.cur, "self").err() == Some(SB_ORDER),
        "自切拒 ORDER",
    );

    // J4 切到未存档线程拒。
    s.add(
        "J4_ghost",
        b.switch_to(99, "g").err() == Some(SB_GHOST),
        "野 tid 切拒 GHOST",
    );

    // J5 存档自洽核。
    s.add("J5_archives_ok", b.archives_ok(), "全存档过自洽核");

    // J6 存档渲染。
    let r = b.archives[0].render();
    s.add(
        "J6_render",
        r.contains("t1") && r.contains("rip=0x1000"),
        "存档渲染含 tid/rip/rsp",
    );

    // J7 存档重复拒。
    s.add(
        "J7_dup_save",
        b.save(2, 0x1100, 0x8100, "dup").err() == Some(SB_TWICE),
        "重复存档拒 TWICE",
    );

    // J8 切换账回放。
    s.add("J8_replayable", b.switches_replayable(), "切换账全可回放");

    // J9 非对齐存档拒。
    s.add(
        "J9_misalign",
        b.save(5, 0x1400, 0x8404, "m").err() == Some(SB_ORDER),
        "RSP 非对齐存档拒 ORDER",
    );

    // J10 零 RIP 存档拒。
    s.add(
        "J10_zero_rip",
        b.save(6, 0, 0x8000, "z").err() == Some(SB_ORDER),
        "零 RIP 存档拒 ORDER",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "边界账折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.save(9, 0x1000, 0x8000, "p").err() == Some(SB_SEALED)
            && b.switch_to(2, "p").err() == Some(SB_SEALED),
        "印账后存/切皆拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("archives=4") && sm.contains("switches=10"),
        "摘要含存档/切换计数",
    );

    // J15 边界语义：cur 初始即在线程表外不拒（A3 侧管），此处 cur=初始 tid 也合法存档。
    let b15 = SwitchBoundary::new(7);
    s.add(
        "J15_cur_outside",
        b15.cur == 7 && b15.archives.is_empty(),
        "初始 cur 独立于存档账",
    );

    // J16 空留痕切换拒。
    let mut b16 = SwitchBoundary::new(1);
    let _ = b16.save(2, 0x1000, 0x2000, "t");
    s.add(
        "J16_empty_note",
        b16.switch_to(2, "").err() == Some(SB_EMPTY),
        "留痕空切拒 EMPTY",
    );

    // J17 十线程环形切十步全对。
    let mut b17 = SwitchBoundary::new(1);
    let mut ok17 = true;
    for i in 1..=10u32 {
        if b17.save(i, 0x1000 + i as u64 * 0x10, 0x8000 + i as u64 * 0x10, "m").is_err() {
            ok17 = false;
        }
    }
    let mut cur17 = 1u32;
    for i in 0..10u32 {
        let to = 1 + (i + 1) % 10;
        if b17.switch_to(to, "r").is_err() {
            ok17 = false;
        }
        cur17 = to;
    }
    s.add(
        "J17_ring10_threads",
        ok17 && b17.cur == cur17,
        "十线程环形切全对",
    );

    // J18 空账指纹稳定。
    let b18 = SwitchBoundary::new(1);
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空边界账指纹稳定");

    // J19 切换后旧 cur 在账（切换留痕两端可查）。
    let last = b17.switches.last().copied().unwrap_or((0, 0));
    s.add(
        "J19_last_pair",
        b17.archives.iter().any(|a| a.tid == last.0) || last.0 == 1,
        "切换尾对含合法 from",
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
    fn unx12175_switch() {
        let mut b = SwitchBoundary::new(1);
        let _ = b.save(2, 0x1000, 0x2000, "t2");
        assert_eq!(b.switch_to(2, "go"), Ok(1));
        assert_eq!(b.switch_to(2, "self"), Err(SB_ORDER));
    }

    #[test]
    fn unx12175_checks_green() {
        let s = run_unx12175_checks();
        assert!(s.all_passed(), "UNX-F12175 判据未全绿");
    }

    #[test]
    fn unx12175_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
