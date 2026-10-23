//! VE-F0132 · 输入到渲染延迟通路（VE-A 域 · A06 环境与系统集成组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0132`
//!
//! **职责定位（锚点原文）**：输入事件到渲染响应的专用通路（输入触发的重渲染走
//! 快车道），通路与 AE01 延迟账本对齐（A56 分段的上游），通路延迟 P95 承诺。
//!
//! **铁律**：
//! 1. **输入快车道**——输入触发的重渲染优先于常规帧（队列插队有上限，防饿死）。
//! 2. **账本对齐**——通路延迟记账口径与 AE01 账本同源（口径漂移即报）。
//! 3. **P95 承诺**——通路延迟 P95 有明确数字承诺，超标显性归因。
//! 4. **阻塞告警**——通路阻塞（队列深度超阈）立即告警，不静默排队。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8CA（输入渲染延迟通路）。
pub const SEG_BASE: u16 = 0xE8CA;
/// 通路阻塞（快车道队列超阈）。
pub const XC_PATH_BLOCKED: u16 = SEG_BASE | 0x01;
/// 口径漂移（与 AE01 账本不同源）。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE | 0x02;
/// P95 劣化（超承诺）。
pub const XC_P95_MISS: u16 = SEG_BASE | 0x03;
/// 队列容量超限。
pub const XC_QUEUE_FULL: u16 = SEG_BASE | 0x04;
/// 非法输入。
pub const XC_BAD_INPUT: u16 = SEG_BASE | 0x05;

/// 快车道事件（输入触发的重渲染请求）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputEvent {
    /// 事件序号。
    pub seq: u32,
    /// 入队时刻（微秒）。
    pub enqueue_us: u64,
    /// 事件类别（0=指针 1=键 2=触控 3=滚轮）。
    pub kind: u8,
    /// 是否高优先（指针移动/键按下——延迟最敏感）。
    pub urgent: bool,
}

/// 单次通路延迟样本（账本对齐单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatencySample {
    /// 事件序号（与 InputEvent::seq 对应）。
    pub seq: u32,
    /// 端到端延迟（微秒：入队→呈现完成）。
    pub latency_us: u32,
    /// 记账口径代数（与 AE01 对齐的口径版本）。
    pub ledger_gen: u32,
}

/// P95 承诺（数字即承诺）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct P95Budget {
    /// P95 延迟上限（微秒）。
    pub p95_us: u32,
    /// 样本窗口大小。
    pub window: usize,
}

/// 输入到渲染延迟通路。
///
/// **快车道语义**：urgent 事件插队到队头，但插队次数有上限（`max_jump`），
/// 超限后只能排在已插队事件之后——防常规帧被无限饿死。
pub struct FastPath {
    /// 快车道队列（环形缓冲语义由 Vec+remove(0) 模拟，容量受控）。
    queue: Vec<InputEvent>,
    /// 队列容量。
    cap: usize,
    /// 最大插队深度（防饿死）。
    max_jump: usize,
    /// 延迟样本窗口。
    samples: Vec<LatencySample>,
    /// P95 承诺。
    budget: P95Budget,
    /// 当前记账口径代数（与 AE01 同步）。
    ledger_gen: u32,
    /// 阻塞告警累计。
    block_alerts: u32,
    /// 口径漂移检出累计。
    drift_hits: u32,
    /// P95 超标累计。
    p95_misses: u32,
    /// 最近诊断码。
    last_code: u16,
}

impl FastPath {
    /// 建通路（容量>0，插队上限<=容量，窗口>0，P95 预算>0）。
    pub fn new(cap: usize, max_jump: usize, budget: P95Budget) -> Result<Self, u16> {
        if cap == 0 || budget.window == 0 || budget.p95_us == 0 {
            return Err(XC_BAD_INPUT);
        }
        if max_jump > cap {
            return Err(XC_QUEUE_FULL);
        }
        Ok(FastPath {
            queue: Vec::new(),
            cap,
            max_jump,
            samples: Vec::new(),
            budget,
            ledger_gen: 1,
            block_alerts: 0,
            drift_hits: 0,
            p95_misses: 0,
            last_code: 0,
        })
    }

    /// 最近诊断码。
    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 同步记账口径（与 AE01 对齐入口）。
    pub fn sync_ledger(&mut self, gen: u32) -> Result<(), u16> {
        if gen == 0 {
            self.last_code = XC_BAD_INPUT;
            return Err(XC_BAD_INPUT);
        }
        self.ledger_gen = gen;
        self.last_code = 0;
        Ok(())
    }

    /// 入队（urgent 插队受 max_jump 限制）。
    pub fn enqueue(&mut self, ev: InputEvent) -> Result<(), u16> {
        if self.queue.len() >= self.cap {
            self.last_code = XC_QUEUE_FULL;
            return Err(XC_QUEUE_FULL);
        }
        if ev.urgent {
            // 插队位置：最多跳过 max_jump 个非 urgent 事件。
            let mut pos = self.queue.len();
            let mut jumped = 0;
            while pos > 0 && jumped < self.max_jump {
                if self.queue[pos - 1].urgent {
                    break;
                }
                pos -= 1;
                jumped += 1;
            }
            self.queue.insert(pos, ev);
        } else {
            self.queue.push(ev);
        }
        // 阻塞告警：深度达容量一半即告警（提前量，不是爆满才叫）。
        if self.queue.len() * 2 >= self.cap {
            self.block_alerts += 1;
            self.last_code = XC_PATH_BLOCKED;
        } else {
            self.last_code = 0;
        }
        Ok(())
    }

    /// 出队（FIFO，urgent 已在前部）。
    pub fn dequeue(&mut self) -> Option<InputEvent> {
        if self.queue.is_empty() {
            None
        } else {
            Some(self.queue.remove(0))
        }
    }

    /// 记录端到端延迟样本（含口径对拍）。
    pub fn record_latency(&mut self, s: LatencySample) -> Result<(), u16> {
        // 口径对拍：样本的 ledger_gen 必须与当前通路口径一致。
        if s.ledger_gen != self.ledger_gen {
            self.drift_hits += 1;
            self.last_code = XC_LEDGER_DRIFT;
            return Err(XC_LEDGER_DRIFT);
        }
        if self.samples.len() >= self.budget.window {
            self.samples.remove(0);
        }
        self.samples.push(s);
        // P95 快检：窗口满后按排序取 P95（样本量小，插入排序足够）。
        if self.samples.len() == self.budget.window {
            let mut sorted: Vec<u32> = self.samples.iter().map(|x| x.latency_us).collect();
            sorted.sort();
            let idx = (self.budget.window * 95 / 100).min(self.budget.window - 1);
            if sorted[idx] > self.budget.p95_us {
                self.p95_misses += 1;
                self.last_code = XC_P95_MISS;
                return Err(XC_P95_MISS);
            }
        }
        self.last_code = 0;
        Ok(())
    }

    /// 当前 P95 估计（窗口未满时按现有样本算）。
    pub fn p95_estimate(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut sorted: Vec<u32> = self.samples.iter().map(|x| x.latency_us).collect();
        sorted.sort();
        let n = sorted.len();
        let idx = (n * 95 / 100).min(n - 1);
        sorted[idx]
    }

    /// 队列深度。
    pub fn depth(&self) -> usize {
        self.queue.len()
    }

    /// 阻塞告警累计。
    pub fn block_alerts(&self) -> u32 {
        self.block_alerts
    }

    /// 口径漂移累计。
    pub fn drift_hits(&self) -> u32 {
        self.drift_hits
    }

    /// P95 超标累计。
    pub fn p95_misses(&self) -> u32 {
        self.p95_misses
    }

    /// 当前口径代数。
    pub fn ledger_gen(&self) -> u32 {
        self.ledger_gen
    }

    /// 读屏行。
    pub fn status_line(&self) -> &'static str {
        if self.queue.len() * 2 >= self.cap {
            "path:blocked"
        } else {
            "path:ok"
        }
    }
}

/// 自检：输入快车道、账本对齐、P95 承诺、阻塞告警、插队上限。
pub fn run_vea132_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn ev(seq: u32, urgent: bool) -> InputEvent {
        InputEvent {
            seq,
            enqueue_us: 1000 + seq as u64,
            kind: 0,
            urgent,
        }
    }

    fn budget() -> P95Budget {
        P95Budget {
            p95_us: 1000,
            window: 4,
        }
    }

    let mut set = CheckSet::new("vea132_fastpath");

    // C132-01 合法建
    {
        let p = FastPath::new(8, 2, budget());
        chk(&mut set, "C132-01 合法建", p.is_ok(), "合法参数被拒");
    }

    // C132-02 参数守卫
    {
        let a = FastPath::new(0, 2, budget()).err() == Some(XC_BAD_INPUT);
        let b = FastPath::new(8, 9, budget()).err() == Some(XC_QUEUE_FULL);
        let c = FastPath::new(8, 2, P95Budget { p95_us: 1000, window: 0 }).err() == Some(XC_BAD_INPUT);
        chk(&mut set, "C132-02 参数守卫", a && b && c, "非法参数被放行");
    }

    // C132-03 快车道插队：urgent 排前
    {
        let mut p = FastPath::new(8, 2, budget()).unwrap();
        let _ = p.enqueue(ev(1, false));
        let _ = p.enqueue(ev(2, false));
        let _ = p.enqueue(ev(3, true));
        let first = p.dequeue().map(|e| e.seq);
        let ok = first == Some(3);
        chk(&mut set, "C132-03 快车道插队", ok, "urgent 未插队");
    }

    // C132-04 插队上限防饿死
    {
        let mut p = FastPath::new(8, 1, budget()).unwrap();
        let _ = p.enqueue(ev(1, false));
        let _ = p.enqueue(ev(2, true));
        let _ = p.enqueue(ev(3, true));
        // max_jump=1：第二个 urgent 只能跳 1 个，排在第一个 urgent 之后。
        let a = p.dequeue().map(|e| e.seq) == Some(2);
        let b = p.dequeue().map(|e| e.seq) == Some(3);
        let c = p.dequeue().map(|e| e.seq) == Some(1);
        chk(&mut set, "C132-04 插队上限", a && b && c, "饿死防护失效");
    }

    // C132-05 阻塞告警：半满即告
    {
        let mut p = FastPath::new(4, 1, budget()).unwrap();
        let _ = p.enqueue(ev(1, false));
        let _ = p.enqueue(ev(2, false));
        let ok = p.block_alerts() >= 1;
        chk(&mut set, "C132-05 阻塞告警", ok, "半满未告警");
    }

    // C132-06 队列满显性
    {
        let mut p = FastPath::new(2, 1, budget()).unwrap();
        let _ = p.enqueue(ev(1, false));
        let _ = p.enqueue(ev(2, false));
        let r = p.enqueue(ev(3, false));
        let ok = r.err() == Some(XC_QUEUE_FULL);
        chk(&mut set, "C132-06 队列满", ok, "溢出未报");
    }

    // C132-07 账本对齐：口径漂移检出
    {
        let mut p = FastPath::new(8, 2, budget()).unwrap();
        let _ = p.sync_ledger(5);
        let r = p.record_latency(LatencySample { seq: 1, latency_us: 100, ledger_gen: 4 });
        let ok = r.err() == Some(XC_LEDGER_DRIFT) && p.drift_hits() == 1;
        chk(&mut set, "C132-07 账本对齐", ok, "口径漂移未检出");
    }

    // C132-08 P95 承诺：超标显性
    {
        let mut p = FastPath::new(8, 2, P95Budget { p95_us: 100, window: 4 }).unwrap();
        let _ = p.sync_ledger(1);
        let _ = p.record_latency(LatencySample { seq: 1, latency_us: 50, ledger_gen: 1 });
        let _ = p.record_latency(LatencySample { seq: 2, latency_us: 60, ledger_gen: 1 });
        let _ = p.record_latency(LatencySample { seq: 3, latency_us: 70, ledger_gen: 1 });
        let r = p.record_latency(LatencySample { seq: 4, latency_us: 900, ledger_gen: 1 });
        let ok = r.err() == Some(XC_P95_MISS) && p.p95_misses() == 1;
        chk(&mut set, "C132-08 P95 承诺", ok, "超标未显性");
    }

    // C132-09 P95 达标放行
    {
        let mut p = FastPath::new(8, 2, P95Budget { p95_us: 1000, window: 4 }).unwrap();
        let _ = p.sync_ledger(1);
        let mut ok = true;
        for i in 0..4 {
            let r = p.record_latency(LatencySample { seq: i, latency_us: 100 + i * 10, ledger_gen: 1 });
            ok = ok && r.is_ok();
        }
        ok = ok && p.p95_misses() == 0;
        chk(&mut set, "C132-09 P95 达标", ok, "达标被误报");
    }

    // C132-10 口径同步零值拒绝
    {
        let mut p = FastPath::new(8, 2, budget()).unwrap();
        let r = p.sync_ledger(0);
        let ok = r.err() == Some(XC_BAD_INPUT);
        chk(&mut set, "C132-10 口径守卫", ok, "零口径被放行");
    }

    set
}
