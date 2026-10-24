//! cga112_worksteal —— CGPU-F0112 工作窃取深度调优。
//! 锚点（CGPU 施工书）：CGPU-F0112 · 工作窃取深度调优 · 目标 320 行
//! 判据摘录：双端队列闭账（push/pop 自端 LIFO、steal 他端
//!   FIFO——三账互斥）；半满触发（本地队列低于半高即放开
//!   窃取——阈值冻结）；窃取批账（每次窃取固定批——1/4
//!   队高向上取整）；重试退避（连续空偷指数退避——上界
//!   封顶）；负载均衡账（最大/最小队列差 ≤ 批——收敛判据）；
//!   停机协议（全局空 + 无在飞——两条件双闸）。
//! 组件：双端工作队列 + 窃取决策核 + 退避控制器 + 均衡账
//!   + 停机闸。
//! 职责定位：线程池调度核心的窃取策略——全部用确定性
//!   账面模型（无真线程），策略参数可判据锁定。
//!
//! 策略契约：
//!   steal_batch = (len + 3) / 4（1/4 向上取整）。
//!   触发条件：len <= cap/2。
//!   退避：delay = min(1 << retries, BACKOFF_CAP)。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xEA50 段 · 本模块独占）=====
pub const XC_EMPTY_DEQ: u32 = 0xEA50; // 空队列弹出
pub const XC_STEAL_SELF: u32 = 0xEA51; // 自窃拒
pub const XC_WORKER_OOB: u32 = 0xEA52; // 工人越界
pub const XC_GATE: u32 = 0xEA53; // 停机闸破

/// 双端工作队列：自端 LIFO（压栈/弹出同端），他端 FIFO 窃取。
pub struct WorkDeque {
    pub buf: Vec<u64>,
    cap: usize,
}

impl WorkDeque {
    pub fn new(cap: usize) -> WorkDeque {
        WorkDeque { buf: Vec::new(), cap }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// 自端压入（溢出拒——容量闭）。
    pub fn push(&mut self, job: u64) -> Result<(), u32> {
        if self.buf.len() >= self.cap {
            return Err(XC_GATE);
        }
        self.buf.push(job);
        Ok(())
    }

    /// 自端弹出（LIFO）。
    pub fn pop(&mut self) -> Result<u64, u32> {
        match self.buf.pop() {
            Some(j) => Ok(j),
            None => Err(XC_EMPTY_DEQ),
        }
    }

    /// 他端窃取（FIFO 取首元素）。
    pub fn steal_one(&mut self) -> Result<u64, u32> {
        if self.buf.is_empty() {
            return Err(XC_EMPTY_DEQ);
        }
        Ok(self.buf.remove(0))
    }

    /// 窃取批：1/4 队高向上取整（至少 1）。
    pub fn steal_batch(&mut self) -> Result<Vec<u64>, u32> {
        if self.buf.is_empty() {
            return Err(XC_EMPTY_DEQ);
        }
        let n = (self.buf.len() + 3) / 4;
        let mut out = Vec::new();
        for _ in 0..n {
            match self.steal_one() {
                Ok(j) => out.push(j),
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    /// 半满触发核：len ≤ cap/2 即放开窃取。
    pub fn steal_allowed(&self) -> bool {
        self.buf.len() * 2 <= self.cap
    }

    /// 容量账冻结。
    pub fn cap(&self) -> usize {
        self.cap
    }
}

/// 窃取批大小模型（纯函数锚——与实例解耦可单测）。
pub fn steal_batch_model(len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (len + 3) / 4
}

// ===== 退避控制器 =====
pub const BACKOFF_CAP: u32 = 32;

pub struct BackoffCtl {
    pub retries: u32,
}

impl BackoffCtl {
    pub fn new() -> BackoffCtl {
        BackoffCtl { retries: 0 }
    }

    /// 空偷一次 → 重试 +1，退避 = min(1<<retries, CAP)。
    pub fn on_empty_steal(&mut self) -> u32 {
        self.retries += 1;
        self.delay()
    }

    /// 偷到活 → 归零。
    pub fn on_progress(&mut self) {
        self.retries = 0;
    }

    pub fn delay(&self) -> u32 {
        let d = 1u32.checked_shl(self.retries.min(31)).unwrap_or(BACKOFF_CAP);
        d.min(BACKOFF_CAP)
    }

    /// 封顶核：retries 再大 delay 也不越 CAP。
    pub fn capped_ok(&self) -> bool {
        self.delay() <= BACKOFF_CAP
    }
}

// ===== 均衡账 =====
/// 工人组：N 条队列的负载账（确定性调度模型）。
pub struct WorkerGroup {
    pub deques: Vec<WorkDeque>,
}

impl WorkerGroup {
    pub fn new(n: usize, cap: usize) -> WorkerGroup {
        let mut deques = Vec::new();
        for _ in 0..n {
            deques.push(WorkDeque::new(cap));
        }
        WorkerGroup { deques }
    }

    /// 工人越界核。
    pub fn get(&mut self, w: usize) -> Result<&mut WorkDeque, u32> {
        if w >= self.deques.len() {
            return Err(XC_WORKER_OOB);
        }
        Ok(&mut self.deques[w])
    }

    /// 窃取核：from 的队列向 to 转一批（自窃拒）。
    pub fn steal(&mut self, from: usize, to: usize) -> Result<usize, u32> {
        if from >= self.deques.len() || to >= self.deques.len() {
            return Err(XC_WORKER_OOB);
        }
        if from == to {
            return Err(XC_STEAL_SELF);
        }
        // 两段借用拆解：先算批大小，再逐个转移。
        let n = steal_batch_model(self.deques[from].len());
        let mut moved = 0usize;
        for _ in 0..n {
            // 逐个借用于 from、推入 to。
            let job = match self.deques[from].steal_one() {
                Ok(j) => j,
                Err(e) => return Err(e),
            };
            let _ = self.deques[to].push(job);
            moved += 1;
        }
        Ok(moved)
    }

    /// 均衡核：max-min 差 ≤ 批模型上界。
    pub fn imbalance(&self) -> usize {
        let mut mx = 0usize;
        let mut mn = usize::MAX;
        for d in self.deques.iter() {
            mx = mx.max(d.len());
            if d.len() < mn {
                mn = d.len();
            }
        }
        if self.deques.is_empty() {
            0
        } else {
            mx.saturating_sub(mn)
        }
    }

    /// 停机闸：全空 + 无在飞（这里账面只有全空一闸，二闸由
    ///   in_flight 参数注入——纯账面双闸）。
    pub fn quiescent(&self, in_flight: usize) -> bool {
        in_flight == 0 && self.deques.iter().all(|d| d.is_empty())
    }
}

// ===== 判据聚合 =====
pub fn run_cga112_checks() -> CheckSet {
    let mut cs = CheckSet::new("cga112_worksteal");
    // 族一：双端队列闭账。
    let mut dq = WorkDeque::new(8);
    cs.add("空弹拒", dq.pop().is_err(), "EMPTY_DEQ");
    let _ = dq.push(10);
    let _ = dq.push(20);
    let _ = dq.push(30);
    cs.add("LIFO 弹出", dq.pop() == Ok(30), "自端后进先出");
    cs.add("FIFO 窃", dq.steal_one() == Ok(10), "他端先进先出");
    cs.add("账闭合", dq.len() == 1, "3−1−1");
    // 容量闭。
    let mut tiny = WorkDeque::new(2);
    let _ = tiny.push(1);
    let _ = tiny.push(2);
    cs.add("容量闭", tiny.push(3).is_err(), "CAP_FULL");
    // 族二：半满触发。
    let half = WorkDeque::new(8);
    cs.add("空队放开", half.steal_allowed(), "0 ≤ 4");
    let mut full = WorkDeque::new(8);
    for i in 0..5u64 {
        let _ = full.push(i);
    }
    cs.add("过半禁窃", !full.steal_allowed(), "5 > 4");
    let mut at_half = WorkDeque::new(8);
    for i in 0..4u64 {
        let _ = at_half.push(i);
    }
    cs.add("半高压线", at_half.steal_allowed(), "4 ≤ 4");
    // 族三：批账模型。
    cs.add("批账 0", steal_batch_model(0) == 0, "空=0");
    cs.add("批账 1", steal_batch_model(1) == 1, "1/4 上整");
    cs.add("批账 5", steal_batch_model(5) == 2, "(5+3)/4");
    cs.add("批账 8", steal_batch_model(8) == 2, "整除");
    cs.add("批账 9", steal_batch_model(9) == 3, "上整核");
    // 批窃取。
    let mut bq = WorkDeque::new(16);
    for i in 0..9u64 {
        let _ = bq.push(i);
    }
    if let Ok(batch) = bq.steal_batch() {
        cs.add("批窃数量", batch.len() == 3, "9→3");
        cs.add("批窃次序", batch == vec![0u64, 1, 2], "FIFO 前缀");
        cs.add("批后余账", bq.len() == 6, "9−3");
    } else {
        cs.add("批窃数量", false, "批窃失败");
    }
    cs.add("空批窃拒", WorkDeque::new(4).steal_batch().is_err(), "EMPTY_DEQ");
    // 族四：退避。
    let mut bk = BackoffCtl::new();
    let _ = bk.on_empty_steal();
    let d1 = bk.delay();
    let _ = bk.on_empty_steal();
    let d2 = bk.delay();
    cs.add("退避倍增", d1 == 2 && d2 == 4, "1<<r");
    bk.on_progress();
    cs.add("进度归零", bk.retries == 0 && bk.delay() == 1, "重置");
    for _ in 0..40u32 {
        let _ = bk.on_empty_steal();
    }
    cs.add("退避封顶", bk.capped_ok() && bk.delay() == BACKOFF_CAP, "≤32");
    // 族五：均衡账。
    let mut g = WorkerGroup::new(4, 16);
    for i in 0..8u64 {
        let _ = g.get(0).map_err(|e| e as u32).map(|d| d.push(i));
    }
    cs.add("失衡账", g.imbalance() == 8, "8-0");
    let mv = g.steal(0, 1).map_err(|e| e as u32);
    cs.add("窃取转移", mv == Ok(2), "8→批2");
    cs.add("失均衡收敛", g.imbalance() == 6, "6-2");
    cs.add("自窃拒", g.steal(2, 2).is_err(), "STEAL_SELF");
    cs.add("工人越界拒", g.steal(0, 9).is_err(), "WORKER_OOB");
    // 反复窃取收敛到差 ≤ 1 批上界。
    for _ in 0..8usize {
        let _ = g.steal(0, 3);
    }
    cs.add("收敛判据", g.imbalance() <= 2, "差 ≤ 批上界");
    // 族六：停机闸。
    let gq = WorkerGroup::new(2, 4);
    cs.add("全空停机", gq.quiescent(0), "双闸全开");
    cs.add("在飞闸破", !gq.quiescent(1), "in_flight≠0");
    // 留一条 job 再验。
    let mut g2 = WorkerGroup::new(2, 4);
    let _ = g2.get(0).map_err(|e| e as u32).map(|d| d.push(1));
    cs.add("残留停机拒", !g2.quiescent(0), "队列非空");
    CheckSet::merge(cs, run_cga112_deep2())
}

// ===== 深化面二：批模型全域与退避序列 =====
pub fn run_cga112_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga112_deep2");
    // 批模型扫：1..33 全等 (len+3)/4。
    cs.add("批模型全域", {
        (1..=32usize).all(|len| steal_batch_model(len) == (len + 3) / 4)
    }, "32 点全对");
    // 退避序列：1,2,4,8,16,32,32,32。
    cs.add("退避序列", {
        let mut bk = BackoffCtl::new();
        let seq = [1u32, 2, 4, 8, 16, 32, 32, 32];
        seq.iter().all(|&e| {
            let _ = bk.on_empty_steal();
            bk.delay() == e
        })
    }, "指数→封顶");
    // 队列 FIFO 前缀批窃两次。
    cs.add("批窃两连", {
        let mut dq = WorkDeque::new(16);
        for i in 0..8u64 {
            let _ = dq.push(i);
        }
        match (dq.steal_batch(), dq.steal_batch()) {
            (Ok(b1), Ok(b2)) => b1 == alloc::vec![0u64, 1] && b2 == alloc::vec![2, 3],
            _ => false,
        }
    }, "前缀序");
    cs.add("停机三态", WorkerGroup::new(1, 4).quiescent(0)
        && !WorkerGroup::new(1, 4).quiescent(3), "空+无在飞");
    cs
}
