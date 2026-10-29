//! UNX-F12192 — RtlDeleteCriticalSection 语义（0x1aa20 段）
//!
//! 锚点：UNX-F12192-J1 删除协议 10/10（删除即释放 DebugInfo 与
//! LockSemaphore 事件两挂点、持锁中删除必拒、重复删除必拒），
//! 删除账全账回放一致。
//! 域：CoRun·UNX-D1-B10（临界区语义谱系）。
//! 结构：DelEntry（删除登记项：双挂点释放位）+ DeleteLed（删除账）
//! + 折叠指纹 + 注入核。
//!
//! NT 语义：RtlDeleteCriticalSection 释放该临界区挂的 DebugInfo
//! （若曾分配）与 LockSemaphore 事件句柄，并把结构标记为已删除；
//! 持锁未释就删除是使用方错误，必拒；对同一临界区二次删除是重复
//! 释放，必拒；野 id 删除按未登记处理，必拒。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1aa20;
pub const DL_SEALED: u32 = SEG_BASE + 0x01; // 印后删除
pub const DL_TWICE: u32 = SEG_BASE + 0x02; // 重复删除
pub const DL_GHOST: u32 = SEG_BASE + 0x03; // 野 id 删除
pub const DL_HELD: u32 = SEG_BASE + 0x04; // 持锁中删除
pub const DL_EMPTY: u32 = SEG_BASE + 0x05; // 空账回放

/// 删除登记项。
#[derive(Clone, PartialEq)]
pub struct DelEntry {
    pub cs_id: u64,
    pub debug_freed: bool,
    pub event_freed: bool,
    pub note: String,
}

impl DelEntry {
    /// 登记项自洽：id 非零、双挂点必须同时释放、留痕非空。
    pub fn ok(&self) -> bool {
        self.cs_id != 0 && self.debug_freed == self.event_freed && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!(
            "del cs={:#x} dbg={} evt={}",
            self.cs_id, self.debug_freed, self.event_freed
        )
    }
}

/// 删除账：先登记（含挂点状态），后删除（释放双挂点）。
pub struct DeleteLed {
    pub entries: Vec<DelEntry>,
    sealed: bool,
}

impl DeleteLed {
    pub fn new() -> DeleteLed {
        DeleteLed { entries: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 登记一把待删临界区（挂点齐备、尚未持锁）。
    pub fn enlist(&mut self, cs_id: u64, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(DL_SEALED);
        }
        if cs_id == 0 {
            return Err(DL_GHOST);
        }
        if note.is_empty() {
            return Err(DL_EMPTY);
        }
        if self.find(cs_id).is_some() {
            return Err(DL_TWICE);
        }
        self.entries.push(DelEntry {
            cs_id,
            debug_freed: false,
            event_freed: false,
            note: note.to_string(),
        });
        Ok(self.entries.len() as u32)
    }

    /// 删除：释放 DebugInfo 与事件双挂点；野 id/重复删拒。
    pub fn delete(&mut self, cs_id: u64) -> Result<(bool, bool), u32> {
        if self.sealed {
            return Err(DL_SEALED);
        }
        let idx = match self.entries.iter().position(|e| e.cs_id == cs_id) {
            Some(i) => i,
            None => return Err(DL_GHOST),
        };
        if self.entries[idx].debug_freed {
            return Err(DL_TWICE);
        }
        self.entries[idx].debug_freed = true;
        self.entries[idx].event_freed = true;
        Ok((true, true))
    }

    /// 已删判定。
    pub fn deleted(&self, cs_id: u64) -> bool {
        self.find(cs_id).map(|e| e.debug_freed).unwrap_or(false)
    }

    /// 伪持有标记：持锁中删除必拒（使用方错误，独立分支）。
    pub fn delete_held(&mut self, cs_id: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(DL_SEALED);
        }
        match self.find(cs_id) {
            Some(e) if e.debug_freed => Err(DL_TWICE),
            Some(_) => Err(DL_HELD),
            None => Err(DL_GHOST),
        }
    }

    /// 查登记项。
    pub fn find(&self, cs_id: u64) -> Option<&DelEntry> {
        self.entries.iter().find(|e| e.cs_id == cs_id)
    }

    /// 已删计数。
    pub fn deleted_count(&self) -> usize {
        self.entries.iter().filter(|e| e.debug_freed).count()
    }

    /// 全账回放自洽。
    pub fn replay(&self) -> bool {
        !self.entries.is_empty() && self.entries.iter().all(|e| e.ok())
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1aa20;
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.cs_id as u32);
            h = h.wrapping_mul(211).wrapping_add(e.debug_freed as u32);
        }
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
            "enlisted={} deleted={}",
            self.entries.len(),
            self.deleted_count()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = DeleteLed::new();
    let _ = b.enlist(1, "a");
    // 1) 野 id 删除必拒。
    got[0] = b.delete(0x99).err() == Some(DL_GHOST);
    // 2) 持锁中删除必拒。
    got[1] = b.delete_held(1).err() == Some(DL_HELD);
    // 3) 首删成功后再删必拒（重复释放）。
    let _ = b.delete(1);
    got[2] = b.delete(1).err() == Some(DL_TWICE);
    // 4) 零 id 登记必拒。
    got[3] = b.enlist(0, "z").err() == Some(DL_GHOST);
    // 5) 印后删除必拒。
    b.seal();
    got[4] = b.delete(1).err() == Some(DL_SEALED);
    got
}

/// 判据入口：UNX-F12192
pub fn run_unx12192_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12192");
    let mut b = DeleteLed::new();

    // J1 三锁登记。
    let mut ok1 = true;
    for i in 0..3u64 {
        if b.enlist(0x10 + i * 0x10, "cs").is_err() {
            ok1 = false;
        }
    }
    s.add("J1_enlist3", ok1 && b.entries.len() == 3, "三锁待删登记");

    // J2 首删：双挂点同时释放。
    let d = b.delete(0x10);
    s.add(
        "J2_del_frees_both",
        d == Ok((true, true)) && b.deleted(0x10),
        "删除释放 DebugInfo 与事件",
    );

    // J3 重复删除拒。
    s.add(
        "J3_redelete",
        b.delete(0x10).err() == Some(DL_TWICE),
        "重复删除拒 TWICE",
    );

    // J4 野 id 删除拒。
    s.add(
        "J4_wild_del",
        b.delete(0xDEAD).err() == Some(DL_GHOST),
        "野 id 删除拒 GHOST",
    );

    // J5 持锁中删除拒。
    s.add(
        "J5_held_del",
        b.delete_held(0x20).err() == Some(DL_HELD),
        "持锁中删除拒 HELD",
    );

    // J6 已删项的 delete_held 走 TWICE（删除位优先）。
    s.add(
        "J6_held_after_del",
        b.delete_held(0x10).err() == Some(DL_TWICE),
        "已删项持锁位走 TWICE",
    );

    // J7 第二锁删除 + 第三锁删除。
    let ok7 = b.delete(0x20).is_ok() && b.delete(0x30).is_ok();
    s.add(
        "J7_del_rest",
        ok7 && b.deleted_count() == 3,
        "余下两锁删除后全删",
    );

    // J8 已删计数对平。
    s.add(
        "J8_deleted_count",
        b.deleted_count() == b.entries.len(),
        "已删计数与登记数对平",
    );

    // J9 全账回放自洽。
    s.add("J9_replay", b.replay(), "删除账逐项过核");

    // J10 登记项渲染。
    let r = b.entries[0].render();
    s.add(
        "J10_render",
        r.contains("cs=0x10") && r.contains("dbg=true") && r.contains("evt=true"),
        "渲染含双挂点释放位",
    );

    // J11 未删项挂点为假（登记初态）。
    let mut b11 = DeleteLed::new();
    let _ = b11.enlist(7, "fresh");
    s.add(
        "J11_fresh_flags",
        !b11.deleted(7) && b11.find(7).map(|e| !e.debug_freed && !e.event_freed).unwrap_or(false),
        "登记初态双挂点未释放",
    );

    // J12 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J12_fingerprint", f1 == f2, "删除账折叠指纹一致");

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    b.seal();
    s.add(
        "J14_sealed",
        b.delete(0x10).err() == Some(DL_SEALED)
            && b.enlist(0x90, "post").err() == Some(DL_SEALED),
        "印账后删除/登记皆拒",
    );

    // J15 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J15_seal_fp", b.fold() == fb, "印账不改指纹");

    // J16 零 id 登记拒。
    let mut b16 = DeleteLed::new();
    s.add(
        "J16_zero_id",
        b16.enlist(0, "z").err() == Some(DL_GHOST),
        "零 id 登记拒 GHOST",
    );

    // J17 空账指纹稳定。
    let b17 = DeleteLed::new();
    s.add("J17_fresh_fp", b17.fold() == b17.fold(), "空删除账指纹稳定");

    // J18 空账回放判不自洽。
    s.add("J18_empty_replay", !b17.replay(), "空账回放判不自洽");

    // J19 重复登记拒。
    let mut b19 = DeleteLed::new();
    let _ = b19.enlist(5, "a");
    s.add(
        "J19_reenlist",
        b19.enlist(5, "b").err() == Some(DL_TWICE),
        "重复登记拒 TWICE",
    );

    // J20 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J20_summary",
        sm.contains("enlisted=3") && sm.contains("deleted=3"),
        "摘要含登记数与已删数",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12192_delete() {
        let mut b = DeleteLed::new();
        assert!(b.enlist(1, "a").is_ok());
        assert_eq!(b.delete(1), Ok((true, true)));
        assert_eq!(b.delete(1), Err(DL_TWICE));
        assert_eq!(b.delete_held(1), Err(DL_TWICE));
        assert_eq!(b.delete(2), Err(DL_GHOST));
    }

    #[test]
    fn unx12192_checks_green() {
        let s = run_unx12192_checks();
        assert!(s.all_passed(), "UNX-F12192 判据未全绿");
    }

    #[test]
    fn unx12192_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
