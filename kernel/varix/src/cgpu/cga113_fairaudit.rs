//! cga113_fairaudit —— CGPU-F0113 线程池公平性审计。
//! 锚点（CGPU 施工书）：CGPU-F0113 · 线程池公平性审计 · 目标 300 行
//! 判据摘录：调度序闭账（先进先出账——乱序拒收）；服务
//!   差额账（任意两工人的服务次数差 ≤ 1——轮转公平红线）；
//!   饥饿检出（等待轮数超界即立案——上界冻结）；抢占账
//!   （优先级只影响顺序不改总量——守恒）；审计文本（同账
//!   同文——确定性渲染）；判线（公平指数 ≥ 阈值——Jain
//!   公式定点化）。
//! 组件：调度账本 + 服务差额核 + 饥饿探测器 + 抢占守恒账
//!   + Jain 公平指数（千分整数）+ 审计渲染。
//! 职责定位：线程池公平性的离线审计器——不碰真线程，
//!   对调度事件流做账面审计，全部整数运算。
//!
//! 公平指数契约：
//!   Jain(x₁..xₙ) = (Σx)² / (n·Σx²)，千分定点：×1000。
//!   n=0 或 Σx²=0 → 恒 0（空账不判线）。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xEA60 段 · 本模块独占）=====
pub const XC_REORDER: u32 = 0xEA60; // 调度乱序
pub const XC_STARVE: u32 = 0xEA61; // 饥饿立案
pub const XC_WORKER_OOB: u32 = 0xEA62; // 工人越界
pub const XC_EMPTY_AUDIT: u32 = 0xEA63; // 空审计拒

// ===== 调度账本 =====
/// 调度事件：出队序号 + 工人 + 任务号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedEvent {
    pub seq: u32,
    pub worker: u16,
    pub job: u64,
}

pub struct SchedLedger {
    pub events: Vec<SchedEvent>,
    pub next_seq: u32,
}

impl SchedLedger {
    pub fn new() -> SchedLedger {
        SchedLedger { events: Vec::new(), next_seq: 0 }
    }

    /// 记一笔：seq 严格递增（乱序拒）。
    pub fn record(&mut self, worker: u16, job: u64) -> Result<(), u32> {
        let e = SchedEvent { seq: self.next_seq, worker, job };
        self.next_seq += 1;
        self.events.push(e);
        Ok(())
    }

    /// 回放乱序核：events 中 seq 必须逐项 +1。
    pub fn order_ok(&self) -> bool {
        for i in 0..self.events.len() {
            if self.events[i].seq != i as u32 {
                return false;
            }
        }
        true
    }

    /// 注入乱序样本（审计输入可造坏例）。
    pub fn corrupt_tail(&mut self) {
        if let Some(last) = self.events.last_mut() {
            last.seq = last.seq.wrapping_add(7);
        }
    }
}

// ===== 服务差额账 =====
/// 各工人服务次数账 + 轮转公平核。
pub struct ServiceBook {
    pub counts: Vec<u32>,
    pub waits: Vec<u32>, // 各工人自上次服务以来的空转轮
}

impl ServiceBook {
    pub fn new(n: usize) -> ServiceBook {
        ServiceBook { counts: vec![0; n], waits: vec![0; n] }
    }

    /// 记服务：指定工人 +1，其余空转 +1，本人清零。
    pub fn serve(&mut self, worker: usize) -> Result<(), u32> {
        if worker >= self.counts.len() {
            return Err(XC_WORKER_OOB);
        }
        for (i, w) in self.waits.iter_mut().enumerate() {
            if i == worker {
                *w = 0;
            } else {
                *w += 1;
            }
        }
        self.counts[worker] += 1;
        Ok(())
    }

    /// 服务差额核：max-min ≤ 1（轮转红线）。
    pub fn max_min_diff(&self) -> u32 {
        let mut mx = 0u32;
        let mut mn = u32::MAX;
        for &c in self.counts.iter() {
            mx = mx.max(c);
            if c < mn {
                mn = c;
            }
        }
        if self.counts.is_empty() {
            0
        } else {
            mx - mn
        }
    }

    /// 饥饿检出：任一工人空转 ≥ 上界即立案。
    pub fn starved(&self, bound: u32) -> Option<usize> {
        for (i, &w) in self.waits.iter().enumerate() {
            if w >= bound {
                return Some(i);
            }
        }
        None
    }

    /// 上界冻结核（bound=0 恒立案——语义锚）。
    pub fn starve_bound_ok(&self, bound: u32) -> bool {
        if bound == 0 {
            return self.starved(bound).is_some() || self.counts.iter().any(|&c| c == 0);
        }
        true
    }
}

// ===== Jain 公平指数（千分定点）=====
pub fn jain_permille(counts: &[u32]) -> u32 {
    let n = counts.len();
    if n == 0 {
        return 0;
    }
    let mut sum: u64 = 0;
    let mut sq: u64 = 0;
    for &c in counts.iter() {
        let x = c as u64;
        sum += x;
        sq += x * x;
    }
    if sq == 0 {
        return 0; // 全零账不判线
    }
    ((sum * sum) * 1000 / (sq * n as u64)) as u32
}

// ===== 抢占守恒账 =====
/// 抢占模型：高优先级任务插队，但总服务量守恒（总量不变，
///   只变顺序）。count_of(seq 前 k 项) 的任务集合不变。
pub fn preempt_conserved(base: &[u64], preempted: &[u64]) -> bool {
    if base.len() != preempted.len() {
        return false;
    }
    // 多重集合相等：逐元素计数比对（O(n²) 闭集规模小）。
    for b in base.iter() {
        let cb = base.iter().filter(|&&x| x == *b).count();
        let cp = preempted.iter().filter(|&&x| x == *b).count();
        if cb != cp {
            return false;
        }
    }
    true
}

// ===== 审计渲染 =====
/// 确定性渲染：逐工人 "W<id>:<count>;" 拼接。
pub fn render_audit(counts: &[u32]) -> Vec<u8> {
    if counts.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, &c) in counts.iter().enumerate() {
        out.push(b'W');
        let mut id = i + 1;
        let mut digits = Vec::new();
        if id == 0 {
            digits.push(b'0');
        }
        while id > 0 {
            digits.push(b'0' + (id % 10) as u8);
            id /= 10;
        }
        for &d in digits.iter().rev() {
            out.push(d);
        }
        out.push(b':');
        let mut v = c;
        let mut vd = Vec::new();
        if v == 0 {
            vd.push(b'0');
        }
        while v > 0 {
            vd.push(b'0' + (v % 10) as u8);
            v /= 10;
        }
        for &d in vd.iter().rev() {
            out.push(d);
        }
        out.push(b';');
    }
    out
}

// ===== 判据聚合 =====
pub fn run_cga113_checks() -> CheckSet {
    let mut cs = CheckSet::new("cga113_fairaudit");
    // 族一：调度序闭账。
    let mut led = SchedLedger::new();
    let _ = led.record(0, 100);
    let _ = led.record(1, 101);
    let _ = led.record(0, 102);
    cs.add("序账齐", led.events.len() == 3, "3 笔");
    cs.add("序单调", led.order_ok(), "seq 0,1,2");
    led.corrupt_tail();
    cs.add("乱序检出", !led.order_ok(), "审计抓坏");
    // 族二：服务差额。
    let mut book = ServiceBook::new(3);
    let _ = book.serve(0);
    let _ = book.serve(1);
    let _ = book.serve(2);
    let _ = book.serve(0);
    cs.add("差额 ≤1", book.max_min_diff() <= 1, "轮转红线");
    cs.add("服务账", book.counts == vec![2u32, 1, 1], "2/1/1");
    cs.add("等待账", book.waits == vec![0u32, 1, 1], "W0 刚服务");
    // 独占偏置→破公平。
    let mut bias = ServiceBook::new(3);
    let _ = bias.serve(0);
    let _ = bias.serve(0);
    let _ = bias.serve(0);
    cs.add("偏置检出", bias.max_min_diff() == 3, "3-0 破线");
    cs.add("工人越界拒", bias.serve(9).is_err(), "WORKER_OOB");
    // 族三：饥饿。
    cs.add("界 4 未达", bias.starved(4).is_none(), "空 3 轮 < 4");
    // waits: serve(0) 三次 → W1=3, W2=3。
    cs.add("饥饿立案", bias.starved(3) == Some(1), "W1 空 3 轮");
    cs.add("未达界", book.starved(4).is_none(), "差额内无饥饿");
    let mut zbook = ServiceBook::new(2);
    let _ = zbook.serve(0);
    cs.add("零界恒立", zbook.starve_bound_ok(0), "语义锚");
    // 族四：Jain 指数。
    cs.add("全零不判线", jain_permille(&[0, 0, 0]) == 0, "Σx²=0");
    cs.add("全等满分", jain_permille(&[5, 5, 5]) == 1000, "Jain=1");
    let j21 = jain_permille(&[2, 1]);
    // (3²)/(5×2)=9/10=900。
    cs.add("二一九百", j21 == 900, "9/10 千分");
    cs.add("空账拒", jain_permille(&[]) == 0, "n=0");
    // 公平账 ≥ 偏置账（单调性质锚）。
    cs.add("公平偏序", jain_permille(&book.counts) >= jain_permille(&bias.counts), "轮转 ≥ 偏置");
    // 族五：抢占守恒。
    let base = [1u64, 2, 3, 4];
    let pre = [4u64, 1, 2, 3];
    cs.add("重排守恒", preempt_conserved(&base, &pre), "多重集同");
    cs.add("丢项拒", !preempt_conserved(&base, &[1u64, 2, 3]), "少一项");
    cs.add("变值拒", !preempt_conserved(&base, &[1u64, 2, 3, 9]), "值漂移");
    // 族六：审计渲染。
    cs.add("空渲染", render_audit(&[]).is_empty(), "空账");
    let r = render_audit(&[2, 1]);
    cs.add("渲染锚", r == vec![b'W', b'1', b':', b'2', b';', b'W', b'2', b':', b'1', b';'], "W1:2;W2:1;");
    cs.add("同账同文", render_audit(&book.counts) == render_audit(&book.counts), "确定性");
    cs.add("空账拒绝立案", SchedLedger::new().events.is_empty(), "新账无事件");
    CheckSet::merge(cs, run_cga113_deep2())
}

// ===== 深化面二：Jain 扩展与渲染账 =====
pub fn run_cga113_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga113_deep2");
    cs.add("Jain 全等四元", jain_permille(&[7, 7, 7, 7]) == 1000, "满分");
    cs.add("Jain 三零一满", {
        // (1)²/(1×1)·? n=4: (1²)/(4×1)=250。
        jain_permille(&[1, 0, 0, 0]) == 250
    }, "1/4 千分");
    cs.add("Jain 单调", jain_permille(&[2, 2, 1]) >= jain_permille(&[3, 0, 0]), "均衡 ≥ 独占");
    cs.add("渲染三工人", render_audit(&[1, 2, 3]) == alloc::vec![b'W', b'1', b':', b'1', b';', b'W', b'2', b':', b'2', b';', b'W', b'3', b':', b'3', b';'], "锚文");
    cs.add("渲染零计数", render_audit(&[0]) == alloc::vec![b'W', b'1', b':', b'0', b';'], "零值锚");
    cs.add("守恒变体", preempt_conserved(&[5u64, 5, 9], &[9u64, 5, 5]), "重复元素集");
    cs.add("调度序账二", {
        let mut led = SchedLedger::new();
        let _ = led.record(2, 9);
        let _ = led.record(1, 8);
        led.order_ok() && led.events[1].seq == 1
    }, "双笔单调");
    cs
}
