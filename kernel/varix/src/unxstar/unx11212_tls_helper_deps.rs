//! UNX-F11212 TLS 辅助路径依赖登记（0x1f812 段）
//!
//! 锚点：UNX-F11212-J1 glibc TLS 初始化对内核的依赖项（set_tid_address/
//! 清零内存假设/FS 基址）逐项登记与 B06 挂点对账。
//! 域：CoRun·UNX-C5-B01——TLS 依赖按种类落条目，每条必须挂到 B06 的具名
//! 挂点；对账比挂点名集合，缺挂与错挂都显性列出。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f812;
pub const TL_NOUN: u32 = SEG_BASE + 0x01; // 依赖种类越界
pub const TL_DUP: u32 = SEG_BASE + 0x02; // 种类重录
pub const TL_NOHOOK: u32 = SEG_BASE + 0x03; // 未挂 B06 挂点
pub const TL_UNPROV: u32 = SEG_BASE + 0x04; // 内核未供给
pub const TL_BADFS: u32 = SEG_BASE + 0x05; // FS 基址零值
pub const TL_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后登记
pub const TL_ALIGN: u32 = SEG_BASE + 0x07; // TCB 未对齐

/// 依赖种类：0 set_tid_address 1 清零假设 2 FS 基址 3 TCB 对齐 4 DTV。
pub const KINDS: usize = 5;
pub const KIND_TID: u32 = 0;
pub const KIND_ZERO: u32 = 1;
pub const KIND_FS: u32 = 2;
pub const KIND_ALIGN: u32 = 3;
pub const KIND_DTV: u32 = 4;
/// TCB 对齐定额（16 字节）。
pub const TCB_ALIGN: u32 = 16;
/// B06 挂点名（冻结集合）。
pub const B06_HOOKS: [&str; 5] = ["tid_addr", "zero_fill", "fs_base", "tcb_align", "dtv_slot"];
pub const TL_TAG: u64 = 0x1f812c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 单条 TLS 依赖。
#[derive(Clone, Debug)]
pub struct TlsNeed {
    pub kind: u32,
    pub name: &'static str,
    pub hook: &'static str,
    pub provided: bool,
}

impl TlsNeed {
    pub fn linked(&self) -> bool {
        !self.hook.is_empty()
    }

    pub fn render(&self) -> String {
        format!("{}:{}:{}", self.name, self.kind, if self.provided { "ok" } else { "gap" })
    }
}

/// TLS 依赖册 + FS 基址与对齐样本。
#[derive(Clone, Debug)]
pub struct TlsLedger {
    pub needs: Vec<TlsNeed>,
    pub fs_base: u32,
    pub tcb_off: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl TlsLedger {
    pub fn new() -> TlsLedger {
        TlsLedger { needs: Vec::new(), fs_base: 0, tcb_off: 0, frozen: false, root: None }
    }

    /// 登记依赖：冻结/种类越界/重录/空名四路拒。
    pub fn enroll(&mut self, kind: u32, name: &'static str, provided: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(TL_FROZEN);
        }
        if kind as usize >= KINDS {
            return Err(TL_NOUN);
        }
        if name.is_empty() || self.needs.iter().any(|n| n.kind == kind) {
            return Err(TL_DUP);
        }
        self.needs.push(TlsNeed { kind, name, hook: "", provided });
        Ok(())
    }

    /// 挂 B06 挂点：挂点名必须在冻结集合内。
    pub fn link(&mut self, kind: u32, hook: &'static str) -> Result<(), u32> {
        if self.frozen {
            return Err(TL_FROZEN);
        }
        if !B06_HOOKS.contains(&hook) {
            return Err(TL_NOHOOK);
        }
        let mut k = 0usize;
        while k < self.needs.len() {
            if self.needs[k].kind == kind {
                self.needs[k].hook = hook;
                return Ok(());
            }
            k += 1;
        }
        Err(TL_NOUN)
    }

    /// 定 FS 基址（零值拒）。
    pub fn set_fs(&mut self, base: u32) -> Result<(), u32> {
        if base == 0 {
            return Err(TL_BADFS);
        }
        self.fs_base = base;
        Ok(())
    }

    /// 定 TCB 偏移（须 16 字节对齐）。
    pub fn set_tcb(&mut self, off: u32) -> Result<(), u32> {
        if off % TCB_ALIGN != 0 {
            return Err(TL_ALIGN);
        }
        self.tcb_off = off;
        Ok(())
    }

    /// 未挂挂点的依赖种类。
    pub fn unlinked(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.needs.len() {
            if !self.needs[k].linked() {
                out.push(self.needs[k].kind);
            }
            k += 1;
        }
        out
    }

    /// 与 B06 挂点集合对账：只比已挂挂点名。
    pub fn missing_hooks(&self, b06: &[&'static str]) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.needs.len() {
            if !b06.contains(&self.needs[k].hook) {
                out.push(self.needs[k].kind);
            }
            k += 1;
        }
        out
    }

    pub fn all_provided(&self) -> bool {
        !self.needs.is_empty() && self.needs.iter().all(|n| n.provided)
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = TL_TAG;
        let mut k = 0usize;
        while k < self.needs.len() {
            h = (h ^ self.needs[k].kind as u64).wrapping_mul(FP_STEP);
            let nb = self.needs[k].name.as_bytes();
            let mut b = 0usize;
            while b < nb.len() {
                h = (h ^ nb[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            let hb = self.needs[k].hook.as_bytes();
            let mut c = 0usize;
            while c < hb.len() {
                h = (h ^ hb[c] as u64).wrapping_mul(FP_STEP);
                c += 1;
            }
            h = (h ^ self.needs[k].provided as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h = (h ^ self.fs_base as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.tcb_off as u64).wrapping_mul(FP_STEP);
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 建册：五类依赖 + 五个 B06 挂点 + FS 基址与 TCB 偏移。
pub fn build() -> (TlsLedger, bool) {
    let mut l = TlsLedger::new();
    let rows: [(u32, &'static str); 5] = [
        (KIND_TID, "set_tid_address"),
        (KIND_ZERO, "tls_zero_fill"),
        (KIND_FS, "fs_base_write"),
        (KIND_ALIGN, "tcb_alignment"),
        (KIND_DTV, "dtv_slot_zero"),
    ];
    let mut ok = true;
    let mut k = 0usize;
    while k < rows.len() {
        if l.enroll(rows[k].0, rows[k].1, true).is_err() {
            ok = false;
        }
        if l.link(rows[k].0, B06_HOOKS[k]).is_err() {
            ok = false;
        }
        k += 1;
    }
    if l.set_fs(0x7fff_0000).is_err() {
        ok = false;
    }
    if l.set_tcb(0x10).is_err() {
        ok = false;
    }
    let good = ok && l.unlinked().is_empty() && l.missing_hooks(&B06_HOOKS).is_empty();
    (l, good)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 依赖种类越界必拒。
    let mut a = TlsLedger::new();
    got[0] = a.enroll(KINDS as u32, "x", true).err() == Some(TL_NOUN);
    // 2) 非 B06 挂点拒。
    let mut b = TlsLedger::new();
    let _ = b.enroll(KIND_TID, "tid", true);
    got[1] = b.link(KIND_TID, "bogus_hook").err() == Some(TL_NOHOOK)
        && b.unlinked() == vec![KIND_TID];
    // 3) FS 基址零值拒、TCB 未对齐拒。
    let mut c = TlsLedger::new();
    got[2] = c.set_fs(0).err() == Some(TL_BADFS) && c.set_tcb(0x18).err() == Some(TL_ALIGN);
    // 4) 错挂（挂到集合外的空串）在对账中列为缺挂。
    let mut d = TlsLedger::new();
    let _ = d.enroll(KIND_FS, "fs", true);
    got[3] = d.missing_hooks(&B06_HOOKS) == vec![KIND_FS];
    got
}

/// 批内判据。
pub fn run_unx11212_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11212");
    let (mut l, built) = build();

    s.add("J1-五类依赖", built && l.needs.len() == KINDS, "TLS 五类内核依赖逐项登记");
    s.add("J2-挂点齐备", l.unlinked().is_empty() && l.missing_hooks(&B06_HOOKS).is_empty(),
        "五类依赖全部挂入 B06 具名挂点");
    s.add("J3-供给齐备", l.all_provided(), "五类依赖内核全部供给");
    s.add("J4-set_tid_address", l.needs[0].name == "set_tid_address" && l.needs[0].hook == "tid_addr",
        "set_tid_address 挂 tid_addr");
    s.add("J5-清零假设", l.needs[1].kind == KIND_ZERO && l.needs[1].hook == "zero_fill",
        "清零内存假设显式挂账");
    s.add("J6-FS 基址", l.fs_base == 0x7fff_0000 && l.needs[2].hook == "fs_base",
        "FS 基址非零且与 fs_base 挂点对应");
    s.add("J7-TCB 对齐", l.tcb_off % TCB_ALIGN == 0 && l.needs[3].hook == "tcb_align",
        "TCB 偏移 16 字节对齐");
    s.add("J8-DTV 挂点", l.needs[4].hook == "dtv_slot", "DTV 槽挂点对应");

    s.add("J9-种类重录拒", {
        let mut a = TlsLedger::new();
        let _ = a.enroll(KIND_TID, "t", true);
        a.enroll(KIND_TID, "t2", true).err() == Some(TL_DUP)
    }, "同种类二次登记必拒");
    s.add("J10-FS 零拒", {
        let mut a = TlsLedger::new();
        a.set_fs(0).err() == Some(TL_BADFS)
    }, "FS 基址零值必拒");
    s.add("J11-对齐拒", {
        let mut a = TlsLedger::new();
        a.set_tcb(0x8).err() == Some(TL_ALIGN)
    }, "TCB 偏移未 16 对齐必拒");

    let inj = inject_four();
    s.add("J12-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "越界/错挂/零基址未对齐/缺挂四路必抓");

    let root = l.freeze();
    s.add("J13-冻结根", l.fold() == root && root != 0, "冻结后依赖册指纹可重算");
    s.add("J14-冻结拒登", l.enroll(KIND_TID, "dup", true).err() == Some(TL_FROZEN),
        "冻结后登记必拒");

    let fp = l.fold();
    s.add_dyn("J15-依赖明细", l.missing_hooks(&B06_HOOKS).is_empty() && fp == root,
        &format!("needs={} fs={:#x}", l.needs.len(), l.fs_base));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11212_tls_flow() {
        let (mut l, built) = build();
        assert!(built);
        assert_eq!(l.needs.len(), 5);
        assert_eq!(l.fs_base, 0x7fff_0000);
        assert_eq!(l.set_tcb(0x20), Ok(()));
        assert_eq!(l.set_tcb(0x21), Err(TL_ALIGN));
        assert_eq!(l.link(KIND_TID, "nope"), Err(TL_NOHOOK));
        let root = l.freeze();
        assert_eq!(l.fold(), root);
        assert_eq!(l.enroll(KIND_TID, "x", true), Err(TL_FROZEN));
        assert!(l.needs[0].render().starts_with("set_tid_address"));
    }

    #[test]
    fn unx11212_checks_green() {
        let s = run_unx11212_checks();
        assert!(s.all_passed(), "UNX-F11212 判据未全绿");
    }
}
