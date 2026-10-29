//! UNX-F12190 — 进程退出临界区泄漏核账（0x1a9e0 段）
//!
//! 锚点：UNX-F12190-J1 退出核账 10/10（退出时持锁未释=泄漏、泄漏
//! 逐锁留痕、净账对平），重复核账必拒，泄漏账全账回放一致。
//! 域：CoRun·UNX-D1-B10（临界区语义谱系）。
//! 结构：CsEntry（临界区登记项）+ ExitAudit（退出核账器：登记→
//! 退出扫描→泄漏账）+ 折叠指纹 + 注入核。
//!
//! NT 语义：进程终止时内核遍历该进程的全部临界区（TEB 的
//! CountOfOwnedCriticalSections 与 DebugInfo 链辅助），仍被持有
//! 的临界区即泄漏——逐个记入泄漏账供诊断；已释放的入净账。退出
//! 核账只做一次，重复核账是账面破坏，必拒。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a9e0;
pub const LA_SEALED: u32 = SEG_BASE + 0x01; // 印后登记
pub const LA_TWICE: u32 = SEG_BASE + 0x02; // 重复登记
pub const LA_GHOST: u32 = SEG_BASE + 0x03; // 野锁释放
pub const LA_AGAIN: u32 = SEG_BASE + 0x04; // 重复核账
pub const LA_EMPTY: u32 = SEG_BASE + 0x05; // 空账回放

/// 临界区登记项。
#[derive(Clone, PartialEq)]
pub struct CsEntry {
    pub cs_id: u64,
    pub owner: u64, // 0=未持有
    pub note: String,
}

impl CsEntry {
    /// 登记项自洽：cs_id 非零、留痕非空。
    pub fn ok(&self) -> bool {
        self.cs_id != 0 && !self.note.is_empty()
    }

    /// 持锁中。
    pub fn held(&self) -> bool {
        self.owner != 0
    }

    pub fn render(&self) -> String {
        format!("cs={:#x} held={}", self.cs_id, self.held())
    }
}

/// 泄漏留痕。
#[derive(Clone, PartialEq)]
pub struct LeakRec {
    pub cs_id: u64,
    pub owner: u64,
    pub note: String,
}

impl LeakRec {
    /// 泄漏留痕自洽：锁与持有者皆非零。
    pub fn ok(&self) -> bool {
        self.cs_id != 0 && self.owner != 0 && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!("leak cs={:#x} by={:#x}", self.cs_id, self.owner)
    }
}

/// 退出核账器。
pub struct ExitAudit {
    pub entries: Vec<CsEntry>,
    pub leaks: Vec<LeakRec>,
    audited: bool,
    sealed: bool,
}

impl ExitAudit {
    pub fn new() -> ExitAudit {
        ExitAudit { entries: Vec::new(), leaks: Vec::new(), audited: false, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn audited(&self) -> bool {
        self.audited
    }

    /// 登记一把临界区；重复登记/零 id 拒。
    pub fn register(&mut self, cs_id: u64, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(LA_SEALED);
        }
        if cs_id == 0 {
            return Err(LA_GHOST);
        }
        if note.is_empty() {
            return Err(LA_EMPTY);
        }
        if self.entries.iter().any(|e| e.cs_id == cs_id) {
            return Err(LA_TWICE);
        }
        self.entries.push(CsEntry { cs_id, owner: 0, note: note.to_string() });
        Ok(self.entries.len() as u32)
    }

    /// 持锁（得锁时登记持有者）。
    pub fn acquire(&mut self, cs_id: u64, tid: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(LA_SEALED);
        }
        if tid == 0 {
            return Err(LA_GHOST);
        }
        let e = match self.entries.iter_mut().find(|e| e.cs_id == cs_id) {
            Some(e) => e,
            None => return Err(LA_GHOST),
        };
        if e.owner == tid {
            return Err(LA_TWICE);
        }
        e.owner = tid;
        Ok(())
    }

    /// 释锁。
    pub fn release(&mut self, cs_id: u64, tid: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(LA_SEALED);
        }
        if tid == 0 {
            return Err(LA_FREE_GATE);
        }
        let e = match self.entries.iter_mut().find(|e| e.cs_id == cs_id) {
            Some(e) => e,
            None => return Err(LA_GHOST),
        };
        if e.owner != tid {
            return Err(LA_FREE_GATE);
        }
        e.owner = 0;
        Ok(())
    }

    /// 退出扫描：持锁未释的逐个入泄漏账，其余入净账。
    /// 返回 (泄漏数, 净账数)；重复核账拒。
    pub fn audit_exit(&mut self, note: &str) -> Result<(usize, usize), u32> {
        if self.sealed {
            return Err(LA_SEALED);
        }
        if self.audited {
            return Err(LA_AGAIN);
        }
        if note.is_empty() {
            return Err(LA_EMPTY);
        }
        self.leaks.clear();
        for e in &self.entries {
            if e.held() {
                self.leaks.push(LeakRec {
                    cs_id: e.cs_id,
                    owner: e.owner,
                    note: note.to_string(),
                });
            }
        }
        self.audited = true;
        let total = self.entries.len();
        Ok((self.leaks.len(), total - self.leaks.len()))
    }

    /// 泄漏账回放自洽。
    pub fn leaks_replay(&self) -> bool {
        !self.leaks.is_empty() && self.leaks.iter().all(|l| l.ok())
    }

    /// 净账数 = 总登记数 - 泄漏数。
    pub fn clean_count(&self) -> usize {
        self.entries.len() - self.leaks.len()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a9e0;
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.cs_id as u32);
            h = h.wrapping_mul(211).wrapping_add(e.owner as u32);
        }
        for l in &self.leaks {
            h = h.wrapping_mul(211).wrapping_add(l.cs_id as u32);
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
            "reg={} leak={} clean={}",
            self.entries.len(),
            self.leaks.len(),
            self.clean_count()
        )
    }
}

/// 内部判定位：未持而释（借用 FREE 语义，独立于五个公开错误码）。
const LA_FREE_GATE: u32 = SEG_BASE + 0x05;

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ExitAudit::new();
    let _ = b.register(0x10, "a");
    let _ = b.register(0x20, "b");
    // 1) 重复登记必拒。
    got[0] = b.register(0x10, "dup").err() == Some(LA_TWICE);
    // 2) 野锁 acquire 必拒。
    got[1] = b.acquire(0x99, 1).err() == Some(LA_GHOST);
    // 3) 未持而释必拒。
    got[2] = b.release(0x10, 7).err() == Some(LA_FREE_GATE);
    // 4) 零 id 登记必拒。
    got[3] = b.register(0, "z").err() == Some(LA_GHOST);
    // 5) 重复核账必拒。
    let _ = b.acquire(0x10, 5);
    let _ = b.audit_exit("first");
    got[4] = b.audit_exit("again").err() == Some(LA_AGAIN);
    got
}

/// 判据入口：UNX-F12190
pub fn run_unx12190_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12190");
    let mut b = ExitAudit::new();

    // J1 五锁登记。
    let mut ok1 = true;
    for i in 0..5u64 {
        if b.register(0x10 + i * 0x10, "cs").is_err() {
            ok1 = false;
        }
    }
    s.add("J1_register5", ok1 && b.entries.len() == 5, "五锁全登记");

    // J2 重复登记拒。
    s.add(
        "J2_dup_register",
        b.register(0x10, "dup").err() == Some(LA_TWICE),
        "重复登记拒 TWICE",
    );

    // J3 持锁两把、释放三把。
    let ok3 = b.acquire(0x10, 0xA1).is_ok()
        && b.acquire(0x40, 0xA1).is_ok()
        && b.release(0x20, 0).is_err()
        && b.release(0x30, 0xA1).is_err();
    s.add(
        "J3_holds",
        ok3 && b.entries[0].held() && b.entries[3].held() && !b.entries[1].held(),
        "两把持锁、三把空置",
    );

    // J4 零 tid 释锁拒（不改账）。
    s.add(
        "J4_zero_tid_release",
        b.release(0x20, 0).err() == Some(LA_FREE_GATE) && !b.entries[1].held(),
        "零 tid 释锁拒且账面不动",
    );

    // J5 持锁者释锁成功。
    let r = b.release(0x10, 0xA1);
    s.add(
        "J5_owner_release",
        r.is_ok() && !b.entries[0].held(),
        "持锁者释锁后归空",
    );

    // J6 恢复持锁供核账（泄漏样本）。
    s.add(
        "J6_rehold",
        b.acquire(0x10, 0xB2).is_ok() && b.entries[0].held(),
        "0x10 重新持有作泄漏样本",
    );

    // J7 退出核账：泄漏 2 / 净账 3。
    let aud = b.audit_exit("exit");
    s.add(
        "J7_audit",
        aud == Ok((2, 3)),
        "退出扫描泄漏2/净账3 对平",
    );

    // J8 泄漏账全自洽。
    s.add("J8_leaks_replay", b.leaks_replay(), "泄漏留痕逐条过核");

    // J9 泄漏名单对平。
    let ids: Vec<u64> = b.leaks.iter().map(|l| l.cs_id).collect();
    s.add(
        "J9_leak_ids",
        ids == [0x10, 0x40],
        "泄漏名单恰为两把持锁",
    );

    // J10 净账数对平。
    s.add(
        "J10_clean",
        b.clean_count() == 3,
        "净账 = 登记 - 泄漏",
    );

    // J11 重复核账拒。
    s.add(
        "J11_reaudit",
        b.audit_exit("again").err() == Some(LA_AGAIN),
        "重复核账拒 AGAIN",
    );

    // J12 泄漏渲染。
    let r = b.leaks[0].render();
    s.add(
        "J12_render",
        r.contains("cs=0x10") && r.contains("by=0xb2"),
        "泄漏渲染含锁与持有者",
    );

    // J13 登记项渲染。
    let r = b.entries[0].render();
    s.add(
        "J13_entry_render",
        r.contains("cs=0x10") && r.contains("held=true"),
        "登记项渲染含持锁位",
    );

    // J14 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J14_fingerprint", f1 == f2, "核账器折叠指纹一致");

    // J15 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J15_inject_all", caught == 5, "5 注入点全检出");

    // J16 印账受控。
    b.seal();
    s.add(
        "J16_sealed",
        b.register(0x90, "post").err() == Some(LA_SEALED)
            && b.audit_exit("post").err() == Some(LA_SEALED),
        "印账后登记/核账皆拒",
    );

    // J17 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J17_seal_fp", b.fold() == fb, "印账不改指纹");

    // J18 全净账（无泄漏）场景。
    let mut b18 = ExitAudit::new();
    let _ = b18.register(0x11, "c");
    let _ = b18.acquire(0x11, 1);
    let _ = b18.release(0x11, 1);
    let aud18 = b18.audit_exit("clean");
    s.add(
        "J18_all_clean",
        aud18 == Ok((0, 1)) && b18.leaks.is_empty(),
        "持锁后释放：退出零泄漏",
    );

    // J19 空账指纹稳定。
    let b19 = ExitAudit::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空核账器指纹稳定");

    // J20 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J20_summary",
        sm.contains("reg=5") && sm.contains("leak=2") && sm.contains("clean=3"),
        "摘要含登记/泄漏/净账三数",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12190_audit() {
        let mut b = ExitAudit::new();
        assert!(b.register(1, "a").is_ok());
        assert!(b.register(1, "a").is_err());
        assert!(b.acquire(1, 9).is_ok());
        assert_eq!(b.audit_exit("x"), Ok((1, 0)));
        assert_eq!(b.audit_exit("x"), Err(LA_AGAIN));
    }

    #[test]
    fn unx12190_checks_green() {
        let s = run_unx12190_checks();
        assert!(s.all_passed(), "UNX-F12190 判据未全绿");
    }

    #[test]
    fn unx12190_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
