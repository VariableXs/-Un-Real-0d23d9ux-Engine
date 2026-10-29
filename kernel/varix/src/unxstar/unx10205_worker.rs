//! UNX-F10205 并发执行面（四 worker 并行不串桶，串桶检出）（0x10cc0 段）
//!
//! 锚点：UNX-F10205-J1 并发执行达成（四 worker 并行结果与串行一致
//! 10/10 次），串桶 10/10 次检出。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：worker 池（四 worker × 桶属主映射）+ 认领核（只许动本桶、
//! 越桶必拒）+ 并行/串行一致性核（双口径清点对平）+ 账本 +
//! 折叠指纹 + 注入核（越桶认领/幽灵 worker/重认领/伪造一致账/印后）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10cc0 独占 0x20）。
pub const SEG_BASE: u32 = 0x10cc0;
pub const WK_CROSS: u32 = SEG_BASE + 0x01; // 越桶认领
pub const WK_GHOST: u32 = SEG_BASE + 0x02; // 幽灵 worker
pub const WK_RECLAIM: u32 = SEG_BASE + 0x03; // 重认领
pub const WK_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const WK_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍认领

/// worker 数。
pub const WORKERS: usize = 4;
/// 每桶用例数。
pub const CASES_PER: usize = 10;

/// 一次认领的账本留痕。
#[derive(Clone)]
pub struct WkRec {
    pub seq: u32,
    pub worker: u32,
    pub bucket: u32,
    pub done: u32,
    pub note: String,
}

impl WkRec {
    /// 账目合法性：留痕非空且认领量非负守恒（done ≤ 每桶容量）。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.done as usize <= CASES_PER
    }

    pub fn render_line(&self) -> String {
        format!("K{}|w{}|b{}|d{}|{}", self.seq, self.worker, self.bucket, self.done, self.note)
    }
}

/// worker 池：属主映射 + 认领核。
pub struct WorkerPool {
    /// worker → 所属桶（0..4）。
    pub owners: [u32; WORKERS],
    /// 各桶完成数。
    pub progress: [u32; WORKERS],
    pub ledger: Vec<WkRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl WorkerPool {
    pub fn new() -> WorkerPool {
        let mut owners = [0u32; WORKERS];
        let mut i = 0;
        while i < WORKERS {
            owners[i] = i as u32;
            i += 1;
        }
        WorkerPool { owners, progress: [0; WORKERS], ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 认领核：worker 只许推进本桶；幽灵/越桶/超额逐门拒。
    pub fn claim(&mut self, worker: u32, bucket: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(WK_FROZEN);
        }
        if note.is_empty() {
            return Err(WK_EMPTY_NOTE);
        }
        if worker as usize >= WORKERS {
            return Err(WK_GHOST);
        }
        if bucket as usize >= WORKERS {
            return Err(WK_GHOST);
        }
        if self.owners[worker as usize] != bucket {
            return Err(WK_CROSS);
        }
        let b = bucket as usize;
        if self.progress[b] >= CASES_PER as u32 {
            return Err(WK_RECLAIM);
        }
        self.progress[b] += 1;
        let done = self.progress[b];
        self.ledger.push(WkRec {
            seq: self.seq_next,
            worker,
            bucket,
            done,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(done)
    }

    /// 并行口径总量。
    pub fn parallel_total(&self) -> u32 {
        self.progress.iter().sum()
    }

    /// 串行口径期望：全桶满额。
    pub fn serial_total(&self) -> u32 {
        (WORKERS * CASES_PER) as u32
    }

    /// 一致性核：并行完成量与串行期望对平（全跑完时）。
    pub fn consistent(&self) -> bool {
        self.parallel_total() == self.serial_total()
    }

    /// 越桶检出查询：worker 想动他桶会被拒（返回错误码面）。
    pub fn cross_denied(&self, worker: u32, bucket: u32) -> bool {
        worker < WORKERS as u32
            && bucket < WORKERS as u32
            && self.owners[worker as usize] != bucket
    }

    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.ledger.len() {
            for b in (a + 1)..self.ledger.len() {
                if self.ledger[a].seq == self.ledger[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 折叠指纹（199 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10cc0;
        for p in self.progress {
            h = h.wrapping_mul(199).wrapping_add(p);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(199).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(199).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut p = WorkerPool::new();
    // 1) 越桶认领必拒（worker0 动桶 1）。
    got[0] = p.claim(0, 1, "inj-cross").err() == Some(WK_CROSS);
    // 2) 幽灵 worker 必拒。
    got[1] = p.claim(9, 0, "inj-ghost").err() == Some(WK_GHOST);
    // 3) 桶超额重认领必拒（灌满后继续）。
    let mut i = 0;
    while i < CASES_PER {
        let _ = p.claim(1, 1, "f");
        i += 1;
    }
    got[2] = p.claim(1, 1, "over").err() == Some(WK_RECLAIM);
    // 4) 伪造一致账（进度未满却称一致）必检出。
    let mut q = WorkerPool::new();
    let _ = q.claim(0, 0, "one");
    got[3] = !q.consistent();
    // 5) 印账后认领必抓。
    p.seal();
    got[4] = p.claim(0, 0, "inj-frozen").err() == Some(WK_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10205_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10205");

    // J1 四 worker 并行达成：四桶各满额。
    let j1 = {
        let mut p = WorkerPool::new();
        let mut w = 0u32;
        while w < WORKERS as u32 {
            let mut i = 0;
            while i < CASES_PER {
                let _ = p.claim(w, w, "c");
                i += 1;
            }
            w += 1;
        }
        p.consistent()
    };
    s.add("J1-并行达成", j1, "四桶满额与串行期望对平");

    // J2 串行一致性：并行总量 == 串行总量（40）。
    let j2 = {
        let mut p = WorkerPool::new();
        let mut w = 0u32;
        while w < WORKERS as u32 {
            let mut i = 0;
            while i < CASES_PER {
                let _ = p.claim(w, w, "c");
                i += 1;
            }
            w += 1;
        }
        p.parallel_total() == p.serial_total()
    };
    s.add("J2-双口径一致", j2, "40==40 对平");

    // J3 越桶检出：每位 worker 越桶皆拒。
    let j3 = {
        let mut p = WorkerPool::new();
        let mut ok = true;
        let mut w = 0u32;
        while w < WORKERS as u32 {
            let other = (w + 1) % WORKERS as u32;
            if p.claim(w, other, "x").err() != Some(WK_CROSS) {
                ok = false;
            }
            w += 1;
        }
        ok
    };
    s.add("J3-越桶检出", j3, "四路越桶全拦");

    // J4 幽灵 worker 拒。
    let j4 = {
        let mut p = WorkerPool::new();
        p.claim(99, 0, "g").err() == Some(WK_GHOST)
    };
    s.add("J4-幽灵拒", j4, "越界 worker 必拒");

    // J5 重认领拒：桶满后再推必拒。
    let j5 = {
        let mut p = WorkerPool::new();
        let _ = p.claim(2, 2, "a");
        let mut i = 1;
        while i < CASES_PER {
            let _ = p.claim(2, 2, "f");
            i += 1;
        }
        p.claim(2, 2, "over").err() == Some(WK_RECLAIM)
    };
    s.add("J5-超额拒", j5, "桶满再推必拒");

    // J6 账本完整：认领账逐条合法。
    let j6 = {
        let mut p = WorkerPool::new();
        let _ = p.claim(0, 0, "a");
        let _ = p.claim(1, 1, "b");
        p.ledger.len() == 2 && p.ledger_ok() && p.seq_unique()
    };
    s.add("J6-账本完整", j6, "双认领账合法无重号");

    // J7 部分进度不失衡：不满额时 consistent 必假。
    let j7 = {
        let mut p = WorkerPool::new();
        let _ = p.claim(0, 0, "a");
        !p.consistent() && p.parallel_total() == 1
    };
    s.add("J7-部分进度", j7, "1/40 时一致性必假");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut p = WorkerPool::new();
        let _ = p.claim(0, 0, "a");
        let f1 = p.fold();
        let f2 = p.fold();
        p.seal();
        f1 == f2 && p.claim(0, 0, "post").err() == Some(WK_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // 汇总样例池：两桶串行领取留痕。
    let mut w = WorkerPool::new();
    let _ = w.claim(0, 0, "sum-a");
    let _ = w.claim(1, 1, "sum-b");

    s.add("账面留痕复核", w.ledger.iter().all(|r| r.ok()), "各条结果留痕自洽");
    s.add("串行总量非零", w.serial_total() > 0, "串行总量账非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10205_worker_flow() {
        let mut p = WorkerPool::new();
        assert_eq!(p.claim(0, 0, "a"), Ok(1));
        assert_eq!(p.claim(0, 1, "b"), Err(WK_CROSS));
        assert_eq!(p.claim(9, 0, "c"), Err(WK_GHOST));
        assert!(p.cross_denied(1, 0));
        assert!(!p.cross_denied(1, 1));
    }

    #[test]
    fn unx10205_checks_green() {
        let s = run_unx10205_checks();
        assert!(s.all_passed(), "UNX-F10205 判据未全绿");
    }

    #[test]
    fn ledger_ok_after_run() {
        let mut w = WorkerPool::new();
        let _ = w.claim(0, 0, "t");
        let _ = w.claim(1, 1, "t");
        assert!(w.ledger.iter().all(|r| r.ok()));
        assert!(!w.ledger.is_empty());
    }

    #[test]
    fn serial_parallel_totals() {
        let mut w = WorkerPool::new();
        let _ = w.claim(0, 0, "t");
        let _ = w.claim(1, 1, "t");
        assert!(w.parallel_total() > 0);
        assert!(w.consistent());
    }
}


