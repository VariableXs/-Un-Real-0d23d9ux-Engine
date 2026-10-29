//! UNX-F48734 C·并发全域复跑（并发用例集汇总）
//! 判据：UNX-F48734-J1 复跑全绿；判据锚定 并发用例集。
//! 纪律：交错调度确定性重演——固定种子展开交错序、终态快照对拍恒等、竞态注入零逃逸。

use crate::alloc::vec::Vec;
use crate::alloc::vec;

pub const SEG_BASE: u16 = 0x1800;

const E_WORKER_BAD: u16 = SEG_BASE + 1; // 工作者号越界
const E_ORDER_BREAK: u16 = SEG_BASE + 2; // 交错序破
const E_SNAP_DIFF: u16 = SEG_BASE + 3; // 复跑终态不一致
const E_COUNT_BREAK: u16 = SEG_BASE + 4; // 工作量守恒破
const E_SCHED_EMPTY: u16 = SEG_BASE + 5; // 空调度拒
const E_LOST_OP: u16 = SEG_BASE + 6; // 操作丢失
const E_DUP_OP: u16 = SEG_BASE + 7; // 操作重复
const E_WORKER_MISS: u16 = SEG_BASE + 8; // 工作者缺漏

/// 并发操作。
#[derive(Clone, Copy, PartialEq)]
pub struct COp {
    /// 工作者号。
    pub worker: u32,
    /// 操作序（工作者内）。
    pub op_seq: u32,
    /// 写入值。
    pub val: u32,
}

/// 交错调度器：把多工作者操作按确定性序展开。
pub struct Interleaver {
    /// 展开序。
    order: Vec<COp>,
    /// 工作者数。
    workers: u32,
}

impl Interleaver {
    pub fn new(workers: u32) -> Interleaver {
        Interleaver {
            order: Vec::new(),
            workers,
        }
    }

    /// 登记一个操作（工作者号合法、工作者内序连续）。
    pub fn submit(&mut self, worker: u32, op_seq: u32, val: u32) -> Result<bool, u16> {
        if worker == 0 || worker > self.workers {
            return Err(E_WORKER_BAD);
        }
        // 工作者内序不得重复
        for o in self.order.iter() {
            if o.worker == worker && o.op_seq == op_seq {
                return Err(E_DUP_OP);
            }
        }
        self.order.push(COp { worker, op_seq, val });
        Ok(true)
    }

    /// 展开：按轮转（round-robin）序输出确定性交错。
    pub fn expand(&self) -> Result<Vec<COp>, u16> {
        if self.order.is_empty() {
            return Err(E_SCHED_EMPTY);
        }
        // 每工作者取操作列表
        let mut per: Vec<Vec<COp>> = Vec::new();
        let mut w = 1u32;
        while w <= self.workers {
            let mut lst: Vec<COp> = Vec::new();
            for o in self.order.iter() {
                if o.worker == w {
                    lst.push(*o);
                }
            }
            lst.sort_by(|a, b| a.op_seq.cmp(&b.op_seq));
            per.push(lst);
            w += 1;
        }
        // 轮转展开
        let mut out: Vec<COp> = Vec::new();
        let mut idx: Vec<usize> = vec![0; per.len()];
        let mut progressed = true;
        while progressed {
            progressed = false;
            let mut wi = 0usize;
            while wi < per.len() {
                if idx[wi] < per[wi].len() {
                    out.push(per[wi][idx[wi]]);
                    idx[wi] += 1;
                    progressed = true;
                }
                wi += 1;
            }
        }
        if out.len() != self.order.len() {
            return Err(E_COUNT_BREAK);
        }
        Ok(out)
    }

    /// 复跑终态快照：按展开序求和与各工作者贡献。
    pub fn run(&self, order: &[COp]) -> Result<(u64, Vec<u64>), u16> {
        if order.is_empty() {
            return Err(E_SCHED_EMPTY);
        }
        let mut total = 0u64;
        let mut per_sum: Vec<u64> = vec![0; self.workers as usize];
        let mut want_seq: Vec<u32> = vec![1; self.workers as usize];
        for o in order.iter() {
            let wi = (o.worker - 1) as usize;
            if o.op_seq != want_seq[wi] {
                return Err(E_ORDER_BREAK);
            }
            want_seq[wi] += 1;
            per_sum[wi] += o.val as u64;
            total += o.val as u64;
        }
        // 工作者内序必须耗尽
        let mut wi2 = 0usize;
        while wi2 < self.workers as usize {
            let mut submitted = 0u32;
            for o in self.order.iter() {
                if o.worker == (wi2 + 1) as u32 {
                    submitted += 1;
                }
            }
            if want_seq[wi2] != submitted + 1 {
                return Err(E_LOST_OP);
            }
            wi2 += 1;
        }
        Ok((total, per_sum))
    }

    /// 两轮终态对拍：总数与各工作者贡献必须全等。
    pub fn assert_eq_run(
        &self,
        a: &(u64, Vec<u64>),
        b: &(u64, Vec<u64>),
    ) -> Result<(), u16> {
        if a.0 != b.0 {
            return Err(E_SNAP_DIFF);
        }
        if a.1.len() != b.1.len() {
            return Err(E_SNAP_DIFF);
        }
        let mut i = 0usize;
        while i < a.1.len() {
            if a.1[i] != b.1[i] {
                return Err(E_SNAP_DIFF);
            }
            i += 1;
        }
        Ok(())
    }

    /// 工作者覆盖守恒。
    pub fn assert_cover(&self) -> Result<(), u16> {
        let mut w = 1u32;
        while w <= self.workers {
            let mut hit = false;
            for o in self.order.iter() {
                if o.worker == w {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_WORKER_MISS);
            }
            w += 1;
        }
        Ok(())
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n.max(1)
    }
}

/// 并发全域复跑主判据：4 工作者交错展开 ×10 轮复跑终态恒等。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 构造 4 工作者 × 每工作者 16 操作（LCG 值）
    let mut iv = Interleaver::new(4);
    let mut rng = Lcg::new(0x48734);
    let mut w = 1u32;
    while w <= 4 {
        let mut s = 1u32;
        while s <= 16 {
            let v = (rng.below(1000) + 1) as u32;
            iv.submit(w, s, v)?;
            s += 1;
        }
        w += 1;
    }
    iv.assert_cover()?;
    hits += 1;

    // R2 展开序确定性：两次展开逐项全等
    let e1 = iv.expand()?;
    let e2 = iv.expand()?;
    if e1.len() != e2.len() {
        return Err(E_COUNT_BREAK);
    }
    let mut i = 0usize;
    while i < e1.len() {
        if e1[i] != e2[i] {
            return Err(E_ORDER_BREAK);
        }
        i += 1;
    }
    hits += 1;

    // R3 复跑 ×10 轮终态恒等
    let base = iv.run(&e1)?;
    let mut r = 0u32;
    while r < 10 {
        let again = iv.run(&e1)?;
        iv.assert_eq_run(&base, &again)?;
        r += 1;
    }
    hits += 1;

    // R4 工作者号越界必拒
    if iv.submit(5, 1, 1).err() == Some(E_WORKER_BAD) {
        hits += 1;
    } else {
        return Err(E_WORKER_BAD);
    }

    // R5 工作者内重复序必拒
    if iv.submit(1, 1, 9).err() == Some(E_DUP_OP) {
        hits += 1;
    } else {
        return Err(E_DUP_OP);
    }

    // R6 交错序破必抓：手工构造乱序展开流，run 应拒
    let mut bad: Vec<COp> = e1.clone();
    if bad.len() >= 3 {
        bad.swap(0, 2);
        if iv.run(&bad).err() == Some(E_ORDER_BREAK) {
            hits += 1;
        } else {
            return Err(E_ORDER_BREAK);
        }
    } else {
        return Err(E_SCHED_EMPTY);
    }

    // R7 空调度必拒
    let iv2 = Interleaver::new(2);
    if iv2.expand().err() == Some(E_SCHED_EMPTY) {
        hits += 1;
    } else {
        return Err(E_SCHED_EMPTY);
    }

    // R8 工作者缺漏必抓
    let mut iv3 = Interleaver::new(3);
    iv3.submit(1, 1, 5)?;
    iv3.submit(2, 1, 6)?;
    if iv3.assert_cover().err() == Some(E_WORKER_MISS) {
        hits += 1;
    } else {
        return Err(E_WORKER_MISS);
    }

    // R9 操作丢失必抓：展开后抹掉一条，run 应报 LOST
    let mut e3 = e1.clone();
    let last = e3.pop().ok_or(E_SCHED_EMPTY)?;
    if iv.run(&e3).err() == Some(E_LOST_OP) {
        hits += 1;
    } else {
        return Err(E_LOST_OP);
    }
    e3.push(last);
    iv.run(&e3)?;

    // R10 LCG 大流 ×256 操作复跑恒等 + 守恒
    let mut iv4 = Interleaver::new(8);
    let mut rng2 = Lcg::new(0x48734_0002);
    let mut w2 = 1u32;
    while w2 <= 8 {
        let mut s2 = 1u32;
        while s2 <= 32 {
            iv4.submit(w2, s2, (rng2.below(500) + 1) as u32)?;
            s2 += 1;
        }
        w2 += 1;
    }
    let e4 = iv4.expand()?;
    if e4.len() != 256 {
        return Err(E_COUNT_BREAK);
    }
    let r4a = iv4.run(&e4)?;
    let r4b = iv4.run(&e4)?;
    iv4.assert_eq_run(&r4a, &r4b)?;
    hits += 1;

    Ok(hits)
}
