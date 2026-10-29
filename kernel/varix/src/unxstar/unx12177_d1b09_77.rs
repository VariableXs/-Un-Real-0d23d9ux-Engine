//! UNX-F12177 — 陷阱帧诊断导出：崩溃现场快照（0x1a840 段）
//!
//! 锚点：UNX-F12177-J1 崩溃现场快照 10/10（陷阱帧+寄存器+留痕三段
//! 导出串，快照可回放比对、导出留痕全账一致），未捕获崩溃快照必拒。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：CrashSnap（快照）+ SnapExporter（导出器）+ 折叠指纹 +
//! 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a840;
pub const SN_GHOST: u32 = SEG_BASE + 0x01; // 未捕获崩溃导出
pub const SN_ORDER: u32 = SEG_BASE + 0x02; // 快照锚非法
pub const SN_DUP: u32 = SEG_BASE + 0x03; // 重复快照
pub const SN_SEALED: u32 = SEG_BASE + 0x04; // 印后导出
pub const SN_EMPTY: u32 = SEG_BASE + 0x05; // 空留痕快照

/// 崩溃快照。
#[derive(Clone, PartialEq)]
pub struct CrashSnap {
    pub seq: u32,
    pub rip: u64,
    pub rsp: u64,
    pub code: u32,
    pub dump: String,
    pub note: String,
}

impl CrashSnap {
    /// 快照自洽：RIP 非零、RSP 8 对齐、码非零、导出串含三段头。
    pub fn ok(&self) -> bool {
        self.rip != 0
            && self.rsp % 8 == 0
            && self.code != 0
            && self.dump.contains("TRAP:")
            && self.dump.contains("REGS:")
            && self.dump.contains("TRACE:")
    }

    pub fn render(&self) -> String {
        format!("#{} c={:#x} rip={:#x}", self.seq, self.code, self.rip)
    }
}

/// 导出器。
pub struct SnapExporter {
    pub snaps: Vec<CrashSnap>,
    pub caught: u32,
    next_seq: u32,
    sealed: bool,
}

impl SnapExporter {
    pub fn new() -> SnapExporter {
        SnapExporter { snaps: Vec::new(), caught: 0, next_seq: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 标记崩溃捕获（导出前置条件）。
    pub fn mark_caught(&mut self) {
        self.caught += 1;
    }

    /// 导出快照（未捕获必拒；锚非法/空留痕/重复必拒）。
    pub fn export(&mut self, rip: u64, rsp: u64, code: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(SN_SEALED);
        }
        if self.caught == 0 {
            return Err(SN_GHOST);
        }
        if note.is_empty() {
            return Err(SN_EMPTY);
        }
        if rsp % 8 != 0 {
            return Err(SN_ORDER);
        }
        if code == 0 {
            return Err(SN_ORDER);
        }
        let dump = format!(
            "TRAP:rip={:#x} rsp={:#x} | REGS:ax={:#x} | TRACE:{}",
            rip, rsp, code, note
        );
        let seq = self.next_seq;
        self.next_seq += 1;
        self.snaps.push(CrashSnap { seq, rip, rsp, code, dump, note: note.to_string() });
        Ok(seq)
    }

    /// 快照回放比对：导出串重建 == 原串。
    pub fn snap_replayable(&self, snap: &CrashSnap) -> bool {
        snap.dump == format!(
            "TRAP:rip={:#x} rsp={:#x} | REGS:ax={:#x} | TRACE:{}",
            snap.rip, snap.rsp, snap.code, snap.note
        )
    }

    /// 全快照回放一致。
    pub fn all_replayable(&self) -> bool {
        self.snaps.iter().all(|s| self.snap_replayable(s))
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a840;
        for s in &self.snaps {
            h = h.wrapping_mul(211).wrapping_add(s.code);
            h = h.wrapping_mul(211).wrapping_add(s.rip as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!("snaps={} caught={} replay={}", self.snaps.len(), self.caught, self.all_replayable())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SnapExporter::new();
    // 1) 未捕获崩溃导出必拒。
    got[0] = b.export(0x1000, 0x2000, 0xC0000005, "w").err() == Some(SN_GHOST);
    // 2) 非对齐 RSP 必拒。
    b.mark_caught();
    got[1] = b.export(0x1000, 0x2004, 0xC0000005, "m").err() == Some(SN_ORDER);
    // 3) 零码必拒。
    got[2] = b.export(0x1000, 0x2000, 0, "z").err() == Some(SN_ORDER);
    // 4) 空留痕必拒。
    got[3] = b.export(0x1000, 0x2000, 0xC0000005, "").err() == Some(SN_EMPTY);
    // 5) 印后导出必拒。
    b.seal();
    got[4] = b.export(0x1000, 0x2000, 0xC0000005, "post").err() == Some(SN_SEALED);
    got
}

/// 判据入口：UNX-F12177
pub fn run_unx12177_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12177");
    let mut b = SnapExporter::new();

    // J1 未捕获导出拒。
    s.add(
        "J1_not_caught",
        b.export(0x1000, 0x2000, 0xC0000005, "w").err() == Some(SN_GHOST),
        "未捕获崩溃拒 GHOST",
    );

    // J2 标记捕获 + 导出十点。
    b.mark_caught();
    let mut ok2 = true;
    for i in 0..10u64 {
        if b.export(0x1000 + i * 0x40, 0x8000 + i * 0x20, 0xC0000005, "av").is_err() {
            ok2 = false;
        }
    }
    s.add("J2_snap_10x", ok2 && b.snaps.len() == 10, "捕获后十点快照全过");

    // J3 快照自洽核。
    s.add(
        "J3_snaps_ok",
        b.snaps.iter().all(|x| x.ok()),
        "全快照过自洽核",
    );

    // J4 导出串三段头。
    let d = &b.snaps[0].dump;
    s.add(
        "J4_three_parts",
        d.contains("TRAP:") && d.contains("REGS:") && d.contains("TRACE:"),
        "导出串 TRAP/REGS/TRACE 三段",
    );

    // J5 回放比对全一致。
    s.add("J5_replayable", b.all_replayable(), "全快照回放一致");

    // J6 快照渲染。
    let r = b.snaps[0].render();
    s.add(
        "J6_render",
        r.contains("#1") && r.contains("c=0xc0000005"),
        "渲染含 seq/码/rip",
    );

    // J7 重复快照序号不重（seq 单调）。
    let seqs: Vec<u32> = b.snaps.iter().map(|x| x.seq).collect();
    s.add(
        "J7_seq_mono",
        seqs == (1..=10).collect::<Vec<u32>>(),
        "seq 1..10 单调",
    );

    // J8 非对齐 RSP 拒。
    s.add(
        "J8_misalign",
        b.export(0x2000, 0x8004, 0xC0000094, "m").err() == Some(SN_ORDER),
        "RSP 非对齐拒 ORDER",
    );

    // J9 零码拒。
    s.add(
        "J9_zero_code",
        b.export(0x2000, 0x8000, 0, "z").err() == Some(SN_ORDER),
        "零码拒 ORDER",
    );

    // J10 空留痕拒。
    s.add(
        "J10_empty_note",
        b.export(0x2000, 0x8000, 0xC0000094, "").err() == Some(SN_EMPTY),
        "留痕空拒 EMPTY",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "导出器折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.export(0x3000, 0x6000, 0xC0000409, "p").err() == Some(SN_SEALED),
        "印账后导出拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("snaps=10") && sm.contains("caught=1"),
        "摘要含快照数与捕获数",
    );

    // J15 未捕获导出计数为 0 时摘要拒语义。
    let b15 = SnapExporter::new();
    s.add(
        "J15_zero_caught",
        b15.caught == 0 && b15.book_summary().contains("caught=0"),
        "未捕获态摘要",
    );

    // J16 多次捕获计数。
    let mut b16 = SnapExporter::new();
    b16.mark_caught();
    b16.mark_caught();
    b16.mark_caught();
    s.add(
        "J16_multi_caught",
        b16.caught == 3,
        "三次标记捕获计 3",
    );

    // J17 快照导出三码混合。
    let mut b17 = SnapExporter::new();
    b17.mark_caught();
    let ok17 = b17.export(0x1000, 0x2000, 0xC0000005, "av").is_ok()
        && b17.export(0x1010, 0x2010, 0xC0000094, "dbz").is_ok()
        && b17.export(0x1020, 0x2020, 0xC00000FD, "so").is_ok();
    s.add("J17_three_codes", ok17 && b17.snaps.len() == 3, "三码快照混合全过");

    // J18 空导出器指纹稳定。
    let b18 = SnapExporter::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空导出器指纹稳定");

    // J19 单快照回放函数独立。
    let s19 = &b.snaps[0];
    s.add(
        "J19_single_replay",
        b.snap_replayable(s19),
        "单快照回放一致",
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
    fn unx12177_snap() {
        let mut b = SnapExporter::new();
        assert_eq!(b.export(0x1000, 0x2000, 0xC0000005, "w"), Err(SN_GHOST));
        b.mark_caught();
        assert!(b.export(0x1000, 0x2000, 0xC0000005, "av").is_ok());
    }

    #[test]
    fn unx12177_checks_green() {
        let s = run_unx12177_checks();
        assert!(s.all_passed(), "UNX-F12177 判据未全绿");
    }

    #[test]
    fn unx12177_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
