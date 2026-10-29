//! UNX-F12165 — RtlRestoreContext 与恢复流程对接（0x1a6c0 段）
//!
//! 锚点：UNX-F12165-J1 上下文恢复 10/10（恢复点三锚齐、恢复目标
//! 必须曾在采集账、恢复留痕全账回放一致），恢复未采集点必拒。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：RestorePoint（恢复点）+ RestoreBook（采集→恢复对接账）+
//! 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a6c0;
pub const RC_GHOST: u32 = SEG_BASE + 0x01; // 恢复未采集点
pub const RC_ORDER: u32 = SEG_BASE + 0x02; // 恢复点锚非法
pub const RC_TWICE: u32 = SEG_BASE + 0x03; // 重复恢复
pub const RC_SEALED: u32 = SEG_BASE + 0x04; // 印后恢复
pub const RC_EMPTY: u32 = SEG_BASE + 0x05; // 空留痕恢复

/// 恢复点。
#[derive(Clone, PartialEq)]
pub struct RestorePoint {
    pub rip: u64,
    pub rsp: u64,
    pub seq: u32,
    pub note: String,
}

impl RestorePoint {
    pub fn ok(&self) -> bool {
        self.rip != 0 && self.rsp % 8 == 0 && self.rsp != 0 && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!("#{} rip={:#x} rsp={:#x}", self.seq, self.rip, self.rsp)
    }
}

/// 采集→恢复对接账。
pub struct RestoreBook {
    pub captured: Vec<(u64, u64)>, // (rip, rsp)
    pub restored: Vec<RestorePoint>,
    next_seq: u32,
    sealed: bool,
}

impl RestoreBook {
    pub fn new() -> RestoreBook {
        RestoreBook { captured: Vec::new(), restored: Vec::new(), next_seq: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 采集（RSP 8 对齐、RIP 非零、重复必拒）。
    pub fn capture(&mut self, rip: u64, rsp: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(RC_SEALED);
        }
        if rip == 0 || rsp == 0 || rsp % 8 != 0 {
            return Err(RC_ORDER);
        }
        if self.captured.iter().any(|&(r, s)| r == rip && s == rsp) {
            return Err(RC_TWICE);
        }
        self.captured.push((rip, rsp));
        Ok(())
    }

    /// 恢复（必须命中采集账；重复恢复同点拒）。
    pub fn restore(&mut self, rip: u64, rsp: u64, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RC_SEALED);
        }
        if note.is_empty() {
            return Err(RC_EMPTY);
        }
        if !self.captured.iter().any(|&(r, s)| r == rip && s == rsp) {
            return Err(RC_GHOST);
        }
        if self.restored.iter().any(|p| p.rip == rip && p.rsp == rsp) {
            return Err(RC_TWICE);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.restored.push(RestorePoint { rip, rsp, seq, note: note.to_string() });
        Ok(seq)
    }

    /// 恢复留痕全可回放：每条恢复点都在采集账。
    pub fn restores_anchored(&self) -> bool {
        self.restored
            .iter()
            .all(|p| self.captured.iter().any(|&(r, s)| r == p.rip && s == p.rsp))
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a6c0;
        for (r, s) in &self.captured {
            h = h.wrapping_mul(211).wrapping_add(*r as u32);
            h = h.wrapping_mul(211).wrapping_add(*s as u32);
        }
        for p in &self.restored {
            h = h.wrapping_mul(211).wrapping_add(p.seq);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "cap={} res={} anchored={}",
            self.captured.len(),
            self.restored.len(),
            self.restores_anchored()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = RestoreBook::new();
    // 1) 恢复未采集点必拒。
    got[0] = b.restore(0x1000, 0x2000, "wild").err() == Some(RC_GHOST);
    // 2) 非对齐采集必拒。
    got[1] = b.capture(0x1000, 0x2004).err() == Some(RC_ORDER);
    // 3) 零 RIP 采集必拒。
    got[2] = b.capture(0, 0x2000).err() == Some(RC_ORDER);
    // 4) 空留痕恢复必拒。
    let _ = b.capture(0x1000, 0x2000);
    got[3] = b.restore(0x1000, 0x2000, "").err() == Some(RC_EMPTY);
    // 5) 印后恢复必拒。
    b.seal();
    got[4] = b.restore(0x1000, 0x2000, "post").err() == Some(RC_SEALED);
    got
}

/// 判据入口：UNX-F12165
pub fn run_unx12165_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12165");
    let mut b = RestoreBook::new();

    // J1 采集十点。
    let mut ok1 = true;
    for i in 0..10u64 {
        if b.capture(0x1000 + i * 0x100, 0x8000 - i * 0x80).is_err() {
            ok1 = false;
        }
    }
    s.add("J1_cap10", ok1 && b.captured.len() == 10, "十点采集全过");

    // J2 恢复三点全对接。
    let r1 = b.restore(0x1000, 0x8000, "back-to-entry");
    let r2 = b.restore(0x1200, 0x7F00, "back-mid");
    let r3 = b.restore(0x1900, 0x7B80, "back-tail");
    s.add(
        "J2_res3",
        r1 == Ok(1) && r2 == Ok(2) && r3 == Ok(3),
        "三点恢复序号 1/2/3",
    );

    // J3 恢复点全在采集账。
    s.add("J3_anchored", b.restores_anchored(), "恢复点全锚定");

    // J4 恢复未采集点拒。
    s.add(
        "J4_wild",
        b.restore(0xDEAD, 0xBEEF, "w").err() == Some(RC_GHOST),
        "未采集恢复拒 GHOST",
    );

    // J5 重复恢复拒。
    s.add(
        "J5_twice",
        b.restore(0x1000, 0x8000, "again").err() == Some(RC_TWICE),
        "同点重复恢复拒 TWICE",
    );

    // J6 恢复点渲染。
    let r = b.restored[0].render();
    s.add(
        "J6_render",
        r.contains("#1") && r.contains("rip=0x1000"),
        "恢复点渲染含 seq/rip",
    );

    // J7 恢复点自洽核。
    s.add(
        "J7_points_ok",
        b.restored.iter().all(|p| p.ok()),
        "全恢复点过自洽核",
    );

    // J8 采集重复拒。
    s.add(
        "J8_cap_dup",
        b.capture(0x1000, 0x8000).err() == Some(RC_TWICE),
        "重复采集拒 TWICE",
    );

    // J9 非对齐采集拒。
    s.add(
        "J9_misalign",
        b.capture(0x2000, 0x8004).err() == Some(RC_ORDER),
        "RSP 非对齐拒 ORDER",
    );

    // J10 零 RIP 采集拒。
    s.add(
        "J10_zero_rip",
        b.capture(0, 0x8000).err() == Some(RC_ORDER),
        "零 RIP 拒 ORDER",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "对接账折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.capture(0x3000, 0x6000).err() == Some(RC_SEALED)
            && b.restore(0x1100, 0x7F80, "post").err() == Some(RC_SEALED),
        "印账后采/恢皆拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("cap=10") && sm.contains("res=3"),
        "摘要含采/恢计数",
    );

    // J15 恢复序号连续。
    let seqs: Vec<u32> = b.restored.iter().map(|p| p.seq).collect();
    s.add(
        "J15_seq_mono",
        seqs == [1, 2, 3],
        "恢复序号 1/2/3 连续",
    );

    // J16 空留痕恢复拒。
    let mut b16 = RestoreBook::new();
    let _ = b16.capture(0x1000, 0x2000);
    s.add(
        "J16_empty_note",
        b16.restore(0x1000, 0x2000, "").err() == Some(RC_EMPTY),
        "留痕空拒 EMPTY",
    );

    // J17 采集对可恢复集合真源（未采必拒、采后可恢）。
    let mut b17 = RestoreBook::new();
    let before = b17.restore(0x42, 0x80, "pre");
    let _ = b17.capture(0x42, 0x80);
    let after = b17.restore(0x42, 0x80, "post");
    s.add(
        "J17_cap_gate",
        before.err() == Some(RC_GHOST) && after.is_ok(),
        "采集门：前拒后开",
    );

    // J18 空账指纹稳定。
    let b18 = RestoreBook::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空对接账指纹稳定");

    // J19 十点采集全恢复（采样率 10/10）。
    let mut b19 = RestoreBook::new();
    let mut ok19 = true;
    for i in 0..10u64 {
        let rip = 0x2000 + i * 0x40;
        let rsp = 0x9000 + i * 0x40;
        if b19.capture(rip, rsp).is_err() || b19.restore(rip, rsp, "r").is_err() {
            ok19 = false;
        }
    }
    s.add(
        "J19_full10",
        ok19 && b19.restored.len() == 10 && b19.restores_anchored(),
        "十点采→恢全对接",
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
    fn unx12165_restore() {
        let mut b = RestoreBook::new();
        let _ = b.capture(0x1000, 0x2000);
        assert_eq!(b.restore(0x1000, 0x2000, "r"), Ok(1));
        assert_eq!(b.restore(0xDEAD, 0xBEEF, "w"), Err(RC_GHOST));
    }

    #[test]
    fn unx12165_checks_green() {
        let s = run_unx12165_checks();
        assert!(s.all_passed(), "UNX-F12165 判据未全绿");
    }

    #[test]
    fn unx12165_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
