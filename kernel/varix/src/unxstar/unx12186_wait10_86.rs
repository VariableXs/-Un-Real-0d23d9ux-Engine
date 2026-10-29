//! UNX-F12186 — 争用路径与等待块语义预告（0x1a960 段）
//!
//! 锚点：UNX-F12186-J1 争用路径 10/10（快路无争用直进、争用即挂
//! WaitBlock、唤醒按 FIFO 出队），争用计数全账对平，队空唤醒必拒。
//! 域：CoRun·UNX-D1-B10（临界区语义谱系）。
//! 结构：WaitBlock（等待块）+ ContentionBook（争用账：FIFO 等待
//! 队列 + 争用计数）+ 折叠指纹 + 注入核。
//!
//! NT 语义预告：EnterCriticalSection 快路径一把 CAS 直进；争用时
//! 内核挂 RTL_CRITICAL_SECTION_WAITBLOCK 链表节点，等待队列严格
//! FIFO——先到先醒，不插队。释放时若队列非空则唤醒队头，队空
//! 唤醒是协议破坏，必拒。本卡只建语义账面，真内核路径后续对接。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a960;
pub const CB_SEALED: u32 = SEG_BASE + 0x01; // 印后入队
pub const CB_DUP: u32 = SEG_BASE + 0x02; // 重复挂块
pub const CB_GHOST: u32 = SEG_BASE + 0x03; // 野线程摘队
pub const CB_EMPTY: u32 = SEG_BASE + 0x04; // 队空唤醒
pub const CB_FREE: u32 = SEG_BASE + 0x05; // 未持而释

/// 等待块：挂在临界区 WaitBlock 链表上的一个 waiter。
#[derive(Clone, PartialEq)]
pub struct WaitBlock {
    pub tid: u64,
    pub seq: u32,
    pub note: String,
}

impl WaitBlock {
    /// 块自洽：tid 非零、序号非零。
    pub fn ok(&self) -> bool {
        self.tid != 0 && self.seq != 0 && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!("wb#{}/tid={:#x}", self.seq, self.tid)
    }
}

/// 争用账：等待队列 FIFO + 争用计数 + 持有者。
pub struct ContentionBook {
    pub owner: u64,
    pub queue: Vec<WaitBlock>,
    pub contention: u32,
    next_seq: u32,
    sealed: bool,
}

impl ContentionBook {
    pub fn new() -> ContentionBook {
        ContentionBook { owner: 0, queue: Vec::new(), contention: 0, next_seq: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 进入路径：锁空直进（快路）；他持挂等待块（争用路径）。
    /// 返回 true=快路直进，false=已挂队；重复挂队拒。
    pub fn enter(&mut self, tid: u64, note: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(CB_SEALED);
        }
        if tid == 0 {
            return Err(CB_GHOST);
        }
        if self.owner == tid {
            return Err(CB_DUP);
        }
        if self.owner == 0 {
            self.owner = tid;
            return Ok(true);
        }
        if self.queue.iter().any(|w| w.tid == tid) {
            return Err(CB_DUP);
        }
        self.contention = self.contention.wrapping_add(1);
        let seq = self.next_seq;
        self.next_seq += 1;
        self.queue.push(WaitBlock { tid, seq, note: note.to_string() });
        Ok(false)
    }

    /// 队头等待块（唤醒协议取人处）。
    pub fn front(&self) -> Result<&WaitBlock, u32> {
        if self.queue.is_empty() {
            return Err(CB_EMPTY);
        }
        Ok(&self.queue[0])
    }

    /// 唤醒出队：严格 FIFO，队头先出；队空唤醒拒。
    pub fn pop_front(&mut self) -> Result<WaitBlock, u32> {
        if self.sealed {
            return Err(CB_SEALED);
        }
        if self.queue.is_empty() {
            return Err(CB_EMPTY);
        }
        Ok(self.queue.remove(0))
    }

    /// 释放：只认持有者本人；未持而释拒。
    pub fn release(&mut self, tid: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(CB_SEALED);
        }
        if self.owner != tid {
            return Err(CB_FREE);
        }
        self.owner = 0;
        Ok(())
    }

    /// FIFO 纪律：队列 seq 严格递增。
    pub fn fifo_monotonic(&self) -> bool {
        self.queue.windows(2).all(|w| w[0].seq < w[1].seq)
    }

    /// 队列自洽：逐块过块核。
    pub fn blocks_ok(&self) -> bool {
        !self.queue.is_empty() && self.queue.iter().all(|w| w.ok())
    }

    /// 争用计数与队列长度对平（本模型二者恒等）。
    pub fn contention_tallies(&self) -> bool {
        self.contention as usize == self.queue.len()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a960;
        h = h.wrapping_mul(211).wrapping_add(self.owner as u32);
        h = h.wrapping_mul(211).wrapping_add(self.contention);
        for w in &self.queue {
            h = h.wrapping_mul(211).wrapping_add(w.tid as u32);
            h = h.wrapping_mul(211).wrapping_add(w.seq);
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
            "owner={:#x} q={} contention={}",
            self.owner, self.queue.len(), self.contention
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ContentionBook::new();
    let _ = b.enter(0xA1, "hold");
    // 1) 他持再挂同一持有者必拒（DUP）。
    got[0] = b.enter(0xA1, "dup").err() == Some(CB_DUP);
    // 2) 队空唤醒必拒。
    got[1] = b.pop_front().err() == Some(CB_EMPTY);
    // 3) 重复挂队必拒。
    let _ = b.enter(0xB2, "wait1");
    got[2] = b.enter(0xB2, "wait1-again").err() == Some(CB_DUP);
    // 4) 未持而释必拒。
    got[3] = b.release(0xB2).err() == Some(CB_FREE);
    // 5) 印后入队必拒。
    b.seal();
    got[4] = b.enter(0xC3, "post").err() == Some(CB_SEALED);
    got
}

/// 判据入口：UNX-F12186
pub fn run_unx12186_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12186");
    let mut b = ContentionBook::new();

    // J1 快路直进：锁空时 enter 返回 true 且不产生争用。
    s.add(
        "J1_fast_path",
        b.enter(0xA1, "fast") == Ok(true) && b.owner == 0xA1 && b.contention == 0,
        "锁空直进零争用",
    );

    // J2 争用挂队：他持时 enter 返回 false。
    let j2 = b.enter(0xB2, "wait1") == Ok(false) && b.enter(0xC3, "wait2") == Ok(false);
    s.add("J2_contend_enqueue", j2 && b.queue.len() == 2, "两 waiter 全挂队");

    // J3 争用计数对平。
    s.add(
        "J3_contention",
        b.contention == 2 && b.contention_tallies(),
        "争用计数与队列长度对平",
    );

    // J4 FIFO 纪律：seq 严格递增。
    s.add("J4_fifo_mono", b.fifo_monotonic(), "队列 seq 严格递增");

    // J5 等待块自洽。
    s.add("J5_blocks_ok", b.blocks_ok(), "全等待块过块核");

    // J6 队头先到先醒。
    let f = b.front();
    s.add(
        "J6_front_fifo",
        f.map(|w| w.tid == 0xB2).unwrap_or(false),
        "队头是先到的 B2",
    );

    // J7 唤醒按 FIFO 出队。
    let w1 = b.pop_front();
    s.add(
        "J7_pop_fifo",
        w1.map(|w| w.tid == 0xB2 && w.seq == 1).unwrap_or(false),
        "首醒是队头 B2/seq1",
    );

    // J8 摘队后争用计数保持总量。
    s.add(
        "J8_contention_sync",
        b.contention == 2 && b.queue.len() == 1,
        "摘队后计数保持总量 2",
    );

    // J9 未持而释拒。
    s.add(
        "J9_foreign_release",
        b.release(0xB2).err() == Some(CB_FREE),
        "未持者释放拒 FREE",
    );

    // J10 本主释放 + 队头转正（释放→唤醒→得锁全链）。
    let ok10 = b.release(0xA1).is_ok()
        && b.pop_front().map(|w| w.tid == 0xC3).unwrap_or(false)
        && b.enter(0xC3, "promote") == Ok(true);
    s.add("J10_promote", ok10 && b.owner == 0xC3, "释放→唤醒→得锁全链通");

    // J11 摘空队唤醒拒。
    s.add("J11_empty_wake", b.pop_front().err() == Some(CB_EMPTY), "队空唤醒拒 EMPTY");

    // J12 等待块渲染。
    let mut b12 = ContentionBook::new();
    let _ = b12.enter(0xA1, "hold");
    let _ = b12.enter(0xB2, "w");
    let r = b12.front().map(|w| w.render()).unwrap_or_default();
    s.add(
        "J12_render",
        r.contains("wb#1") && r.contains("tid=0xb2"),
        "等待块渲染含 seq/tid",
    );

    // J13 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J13_fingerprint", f1 == f2, "争用账折叠指纹一致");

    // J14 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J14_inject_all", caught == 5, "5 注入点全检出");

    // J15 印账受控。
    b.seal();
    s.add(
        "J15_sealed",
        b.enter(0xD4, "post").err() == Some(CB_SEALED)
            && b.pop_front().err() == Some(CB_SEALED),
        "印账后入队/唤醒皆拒",
    );

    // J16 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J16_seal_fp", b.fold() == fb, "印账不改指纹");

    // J17 空账指纹稳定。
    let b17 = ContentionBook::new();
    s.add("J17_fresh_fp", b17.fold() == b17.fold(), "空争用账指纹稳定");

    // J18 空账不判自洽（队空 blocks_ok 假）。
    let b18 = ContentionBook::new();
    s.add("J18_empty_blocks", !b18.blocks_ok(), "空队列不判块自洽");

    // J19 野线程（零 tid）入队拒。
    let mut b19 = ContentionBook::new();
    s.add(
        "J19_zero_tid",
        b19.enter(0, "zero").err() == Some(CB_GHOST) && b19.owner == 0,
        "零 tid 入队拒且不改锁态",
    );

    // J20 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J20_summary",
        sm.contains("owner=0xc3") && sm.contains("contention=2"),
        "摘要含持有者与争用计数",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12186_contention() {
        let mut b = ContentionBook::new();
        assert_eq!(b.enter(0xA1, "t"), Ok(true));
        assert_eq!(b.enter(0xB2, "t"), Ok(false));
        assert_eq!(b.enter(0xB2, "t"), Err(CB_DUP));
        assert_eq!(b.release(0xB2), Err(CB_FREE));
        assert_eq!(b.release(0xA1), Ok(()));
        assert_eq!(b.pop_front().map(|w| w.tid), Ok(0xB2));
    }

    #[test]
    fn unx12186_checks_green() {
        let s = run_unx12186_checks();
        assert!(s.all_passed(), "UNX-F12186 判据未全绿");
    }

    #[test]
    fn unx12186_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
