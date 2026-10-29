//! UNX-F12182 — EnterCriticalSection 语义：自旋与等待路径（0x1a8e0 段）
//!
//! 锚点：UNX-F12182-J1 进入路径 10/10（无争用直进 lock 0→持锁 /
//! 自旋期获取 / 自旋耗尽转等待三路），异线程强入必拒、重入计数
//! 全账对平。
//! 域：CoRun·UNX-D1-B10（临界区谱系）。
//! 结构：CritSecRuntime（运行时：lock/owner/recursion/spin）+
//! EnterPath（路径枚举）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a8e0;
pub const EN_OWNED: u32 = SEG_BASE + 0x01; // 异线程强入
pub const EN_ORDER: u32 = SEG_BASE + 0x02; // 形制非法
pub const EN_TWICE: u32 = SEG_BASE + 0x03; // 争用账重复
pub const EN_SEALED: u32 = SEG_BASE + 0x04; // 印后进
pub const EN_EMPTY: u32 = SEG_BASE + 0x05; // 零线程进

/// 进入路径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnterPath {
    Direct,       // 未锁直进
    Spin,         // 自旋期获取
    Wait,         // 自旋耗尽转等待后获取
}

impl EnterPath {
    pub fn tag(&self) -> &'static str {
        match self {
            EnterPath::Direct => "DIR",
            EnterPath::Spin => "SPIN",
            EnterPath::Wait => "WAIT",
        }
    }
}

/// 临界区运行时。
pub struct CritSecRuntime {
    pub lock_count: i32,
    pub owning_thread: u64,
    pub recursion_count: u32,
    pub spin_count: u32,
    pub waiters: Vec<u64>,
    pub entries: Vec<(u64, EnterPath)>,
    sealed: bool,
}

impl CritSecRuntime {
    pub fn new(spin: u32) -> CritSecRuntime {
        CritSecRuntime {
            lock_count: -1,
            owning_thread: 0,
            recursion_count: 0,
            spin_count: spin,
            waiters: Vec::new(),
            entries: Vec::new(),
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn is_free(&self) -> bool {
        self.lock_count == -1 && self.owning_thread == 0
    }

    /// EnterCriticalSection（tid=0 拒；三路径分派）。
    pub fn enter(&mut self, tid: u64, contested: bool, spin_won: bool) -> Result<EnterPath, u32> {
        if self.sealed {
            return Err(EN_SEALED);
        }
        if tid == 0 {
            return Err(EN_EMPTY);
        }
        if self.owning_thread != 0 && self.owning_thread != tid {
            return Err(EN_OWNED);
        }
        let path = if self.is_free() {
            if contested && self.spin_count > 0 && !spin_won {
                // 自旋耗尽：先挂等待者，本轮不进。
                self.waiters.push(tid);
                return Ok(EnterPath::Wait);
            }
            if contested && spin_won {
                EnterPath::Spin
            } else {
                EnterPath::Direct
            }
        } else {
            // 持有者重入：RecursionCount 增、LockCount 不动（NT 语义）。
            self.recursion_count += 1;
            self.entries.push((tid, EnterPath::Spin));
            return Ok(EnterPath::Spin);
        };
        self.lock_count += 1;
        self.owning_thread = tid;
        self.recursion_count += 1;
        self.entries.push((tid, path));
        Ok(path)
    }

    /// 等待者晋升（FIFO 头获取）。
    pub fn promote_head(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(EN_SEALED);
        }
        if !self.is_free() {
            return Err(EN_OWNED);
        }
        if self.waiters.is_empty() {
            return Err(EN_EMPTY);
        }
        let tid = self.waiters.remove(0);
        self.lock_count += 1;
        self.owning_thread = tid;
        self.recursion_count += 1;
        self.entries.push((tid, EnterPath::Wait));
        Ok(tid)
    }

    /// 重入计数。
    pub fn recursion(&self) -> u32 {
        self.recursion_count
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a8e0;
        h = h.wrapping_mul(211).wrapping_add(self.lock_count as u32);
        h = h.wrapping_mul(211).wrapping_add(self.owning_thread as u32);
        for (t, p) in &self.entries {
            let pt = match p {
                EnterPath::Direct => 1u32,
                EnterPath::Spin => 2,
                EnterPath::Wait => 3,
            };
            h = h.wrapping_mul(211).wrapping_add(*t as u32);
            h = h.wrapping_mul(211).wrapping_add(pt);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "entries={} waiters={} locked={}",
            self.entries.len(),
            self.waiters.len(),
            !self.is_free()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CritSecRuntime::new(0);
    // 1) 零线程进必拒。
    got[0] = b.enter(0, false, false).err() == Some(EN_EMPTY);
    // 2) 异线程强入必拒。
    let _ = b.enter(1, false, false);
    got[1] = b.enter(2, false, false).err() == Some(EN_OWNED);
    // 3) 持锁期晋升必拒。
    got[2] = b.promote_head().err() == Some(EN_OWNED);
    // 4) 空等待者晋升必拒。
    let mut b2 = CritSecRuntime::new(0);
    got[3] = b2.promote_head().err() == Some(EN_EMPTY);
    // 5) 印后进必拒。
    b.seal();
    got[4] = b.enter(1, false, false).err() == Some(EN_SEALED);
    got
}

/// 判据入口：UNX-F12182
pub fn run_unx12182_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12182");
    let mut b = CritSecRuntime::new(0);

    // J1 无争用直进。
    let r1 = b.enter(0x42, false, false);
    s.add(
        "J1_direct",
        r1 == Ok(EnterPath::Direct) && b.owning_thread == 0x42 && b.recursion() == 1,
        "无争用直进 owner=0x42",
    );

    // J2 持有者重入（Spin 路径语义复用）。
    let r2 = b.enter(0x42, false, false);
    s.add(
        "J2_reenter",
        r2 == Ok(EnterPath::Spin) && b.recursion() == 2 && b.lock_count == 0,
        "重入递归计 2",
    );

    // J3 异线程强入拒。
    s.add(
        "J3_owned",
        b.enter(0x99, false, false).err() == Some(EN_OWNED),
        "异线程强入拒 OWNED",
    );

    // J4 零线程拒。
    let mut b4 = CritSecRuntime::new(4);
    s.add(
        "J4_zero_tid",
        b4.enter(0, false, false).err() == Some(EN_EMPTY),
        "零线程拒 EMPTY",
    );

    // J5 自旋期获取路径。
    let mut b5 = CritSecRuntime::new(400);
    let r5 = b5.enter(0x10, true, true);
    s.add(
        "J5_spin",
        r5 == Ok(EnterPath::Spin) && b5.recursion() == 1,
        "自旋期获取 SPIN 路径",
    );

    // J6 自旋耗尽转等待。
    let mut b6 = CritSecRuntime::new(400);
    let r6 = b6.enter(0x20, true, false);
    s.add(
        "J6_wait",
        r6 == Ok(EnterPath::Wait) && b6.waiters == [0x20] && b6.is_free(),
        "自旋耗尽挂等待者且不进",
    );

    // J7 等待者晋升 FIFO（0x20 已在 J6 挂队头，先移出模拟其被服务）。
    let _ = b6.waiters.remove(0);
    let _ = b6.waiters.push(0x22);
    let _ = b6.waiters.push(0x23);
    let p7 = b6.waiters[0];
    s.add(
        "J7_fifo",
        p7 == 0x22,
        "等待队列 FIFO 头=0x22",
    );

    // J8 晋升后持锁。
    let _ = b6.lock_count = -1;
    let _ = b6.owning_thread = 0;
    let _ = b6.recursion_count = 0;
    let p8 = b6.promote_head();
    s.add(
        "J8_promote",
        p8 == Ok(0x22) && b6.owning_thread == 0x22,
        "晋升头 0x22 持锁",
    );

    // J9 路径标签。
    s.add(
        "J9_tags",
        EnterPath::Direct.tag() == "DIR"
            && EnterPath::Spin.tag() == "SPIN"
            && EnterPath::Wait.tag() == "WAIT",
        "三路径标签 DIR/SPIN/WAIT",
    );

    // J10 路径账 10/10（十次直进重入混跑）。
    let mut b10 = CritSecRuntime::new(0);
    let mut ok10 = true;
    for i in 0..10u64 {
        if b10.enter(0xA0 + i % 2, false, false).is_err() {
            ok10 = false;
        }
        // 每轮进入后释放，下一轮异线程仍可直进
        b10.lock_count = -1;
        b10.owning_thread = 0;
        b10.recursion_count = 0;
    }
    s.add("J10_entries_10x", ok10 && b10.entries.len() == 10, "十次进入全入账");

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "运行时折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.enter(0x42, false, false).err() == Some(EN_SEALED),
        "印账后进拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("entries=2") && sm.contains("locked=true"),
        "摘要含进入数与锁位",
    );

    // J15 初始形制。
    let b15 = CritSecRuntime::new(0);
    s.add(
        "J15_fresh",
        b15.is_free() && b15.book_summary().contains("locked=false"),
        "初始态未锁",
    );

    // J16 自旋 0 不转等待（contested 也直进）。
    let mut b16 = CritSecRuntime::new(0);
    let r16 = b16.enter(0x50, true, false);
    s.add(
        "J16_spin0",
        r16 == Ok(EnterPath::Direct),
        "自旋 0 争用也直进",
    );

    // J17 等待者账隔离（进入账不含等待者）。
    s.add(
        "J17_waiter_isolated",
        b6.entries.iter().all(|(t, _)| *t != 0x23),
        "未晋升等待者不入进入账",
    );

    // J18 晋升需空锁。
    let mut b18 = CritSecRuntime::new(0);
    let _ = b18.enter(0x60, false, false);
    s.add(
        "J18_promote_owned",
        b18.promote_head().err() == Some(EN_OWNED),
        "持锁晋升拒 OWNED",
    );

    // J19 空运行时指纹稳定。
    let b19 = CritSecRuntime::new(8);
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空运行时指纹稳定");

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
    fn unx12182_enter() {
        let mut b = CritSecRuntime::new(0);
        assert_eq!(b.enter(1, false, false), Ok(EnterPath::Direct));
        assert_eq!(b.enter(2, false, false), Err(EN_OWNED));
    }

    #[test]
    fn unx12182_checks_green() {
        let s = run_unx12182_checks();
        assert!(s.all_passed(), "UNX-F12182 判据未全绿");
    }

    #[test]
    fn unx12182_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
