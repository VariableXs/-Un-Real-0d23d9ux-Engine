//! cgb165_prioemit.rs —— CGPU-F0165 · 优先级发射与抢占点
//!
//! 任务的优先级发射：三档（实时帧/交互/后台）与 F0002 线程池
//! 的优先级队列对接；发射顺序（高优先级的就绪任务先发射——
//! 同优先级 FIFO）；抢占点标记（协作式让位点——长任务每 2ms
//! 检查抢占请求，A 域 F0063 联动）；让位延迟 P95；饥饿防护
//! （F0090 老化复用）；遥测。
//! 诊断码段 0x0440（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0440;

/// 抢占检查周期（毫秒，判据「每 2ms 检查」）。
pub const PREEMPT_PERIOD_MS: u32 = 2;

/// 优先级三档（判据「三档」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum PrioClass {
    /// 后台（最低）。
    Background,
    /// 交互。
    Interactive,
    /// 实时帧（最高）。
    Realtime,
}

impl PrioClass {
    /// 全档序（低 → 高）。
    pub const ALL: [PrioClass; 3] = [
        PrioClass::Background,
        PrioClass::Interactive,
        PrioClass::Realtime,
    ];

    /// 档号。
    pub fn rank(self) -> u8 {
        match self {
            PrioClass::Background => 0,
            PrioClass::Interactive => 1,
            PrioClass::Realtime => 2,
        }
    }
}

/// 就绪任务。
#[derive(Clone, Copy, Debug)]
pub struct ReadyTask {
    /// 任务号。
    pub task: u32,
    /// 优先级档。
    pub class: PrioClass,
    /// 就绪序号（同档 FIFO 依据）。
    pub seq: u64,
}

impl ReadyTask {
    /// 自检。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// 发射队列（判据「发射顺序」）：三档各一 FIFO，发射时高档
/// 先清空再低档。
pub struct EmitQueue {
    /// 三档就绪队列（档号序 = 数组序，高档在前发射）。
    lanes: [Vec<ReadyTask>; 3],
    /// 已发射序列（遥测）。
    emitted: Vec<(u32, PrioClass, u64)>,
    /// 老化计数：任务等待超阈值次数（判据「饥饿防护」）。
    aged: u32,
    /// 老化阈值（就绪序差）。
    age_threshold: u64,
}

impl EmitQueue {
    /// 建队：老化阈值必填（0 = 关闭饥饿防护，违规）。
    pub fn new(age_threshold: u64) -> Result<Self, u16> {
        if age_threshold == 0 {
            return Err(SEG_BASE + 2);
        }
        Ok(EmitQueue {
            lanes: [Vec::new(), Vec::new(), Vec::new()],
            emitted: Vec::new(),
            aged: 0,
            age_threshold,
        })
    }

    /// 队列总长。
    pub fn len(&self) -> usize {
        self.lanes.iter().map(|l| l.len()).sum()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 已发射数。
    pub fn emitted_count(&self) -> usize {
        self.emitted.len()
    }

    /// 老化计数。
    pub fn aged_count(&self) -> u32 {
        self.aged
    }

    /// 入队（判据「三档发射序」基础）：任务号全局不重复。
    pub fn push(&mut self, t: ReadyTask) -> Result<(), u16> {
        t.validate()?;
        if self
            .lanes
            .iter()
            .any(|l| l.iter().any(|e| e.task == t.task))
        {
            return Err(SEG_BASE + 3);
        }
        if self.emitted.iter().any(|(task, _, _)| *task == t.task) {
            return Err(SEG_BASE + 4);
        }
        self.lanes[t.class.rank() as usize].push(t);
        Ok(())
    }

    /// 发射一枚（判据「发射顺序」）：取最高非空档的队头；
    /// 老化复用（判据「饥饿防护」）：若低档队头等待跨度超过
    /// 阈值（seq 差），越级发射之（F0090 老化）。
    pub fn emit_one(&mut self, cur_seq: u64) -> Result<ReadyTask, u16> {
        if self.is_empty() {
            return Err(SEG_BASE + 5);
        }
        // 老化扫描：最低档先看（最容易饿死）。
        for r in (0..3).rev() {
            if let Some(head) = self.lanes[r].first() {
                if cur_seq.saturating_sub(head.seq) >= self.age_threshold {
                    let t = self.lanes[r].remove(0);
                    self.aged = self.aged.saturating_add(1);
                    self.emitted.push((t.task, t.class, t.seq));
                    return Ok(t);
                }
            }
        }
        // 正常发射：高档优先。
        for r in (0..3).rev() {
            if !self.lanes[r].is_empty() {
                let t = self.lanes[r].remove(0);
                self.emitted.push((t.task, t.class, t.seq));
                return Ok(t);
            }
        }
        Err(SEG_BASE + 5)
    }

    /// 发射序合规审计（判据「发射顺序」验证面）：已发射序列中
    /// 不得出现「高档任务就绪序 < 已发射低档任务就绪序」的倒挂
    /// （老化越级除外——但老化已计数）。
    pub fn emit_order_audit(&self) -> Result<(), u16> {
        for i in 0..self.emitted.len() {
            for j in (i + 1)..self.emitted.len() {
                let (ta, ca, sa) = self.emitted[i];
                let (tb, cb, sb) = self.emitted[j];
                if cb.rank() > ca.rank() && sb < sa {
                    // 后发射的是更高档且就绪更早——说明它被低档
                    // 压住了：除非发生过老化越级，否则违规。
                    if self.aged == 0 {
                        return Err(SEG_BASE + 6);
                    }
                }
                let _ = (ta, tb);
            }
        }
        Ok(())
    }

    /// 档位普查。
    pub fn lane_census(&self) -> [usize; 3] {
        [
            self.lanes[0].len(),
            self.lanes[1].len(),
            self.lanes[2].len(),
        ]
    }

    /// 发射档位普查（遥测投影）。
    pub fn emit_class_census(&self) -> [usize; 3] {
        let mut out = [0usize; 3];
        for (_, c, _) in &self.emitted {
            out[c.rank() as usize] += 1;
        }
        out
    }
}

/// 抢占点标记（判据「抢占点标记」）：长任务内的协作让位点册。
pub struct PreemptMarks {
    /// 让位点：任务号 → 检查时刻列表（ms）。
    marks: Vec<(u32, Vec<u32>)>,
    /// 让位延迟采样（ms）：请求 → 实际让位时刻差。
    yield_lats: Vec<u32>,
    /// 抢占请求册：任务号 → 请求时刻。
    requests: Vec<(u32, u32)>,
}

impl PreemptMarks {
    /// 空册。
    pub fn new() -> Self {
        PreemptMarks {
            marks: Vec::new(),
            yield_lats: Vec::new(),
            requests: Vec::new(),
        }
    }

    /// 标记让位点（判据「每 2ms」）：同一任务相邻让位点间隔
    /// 必须 ≥ 周期（间隔小于周期 = 标记过密，抢占风暴）。
    pub fn mark(&mut self, task: u32, at_ms: u32) -> Result<(), u16> {
        if task == 0 {
            return Err(SEG_BASE + 7);
        }
        let row = match self.marks.iter_mut().find(|(t, _)| *t == task) {
            Some(r) => r,
            None => {
                self.marks.push((task, Vec::new()));
                let n = self.marks.len();
                &mut self.marks[n - 1]
            }
        };
        if let Some(last) = row.1.last() {
            if at_ms.saturating_sub(*last) < PREEMPT_PERIOD_MS {
                return Err(SEG_BASE + 8);
            }
        }
        row.1.push(at_ms);
        Ok(())
    }

    /// 抢占请求。
    pub fn request(&mut self, task: u32, at_ms: u32) -> Result<(), u16> {
        if task == 0 {
            return Err(SEG_BASE + 7);
        }
        if self.requests.iter().any(|(t, _)| *t == task) {
            return Err(SEG_BASE + 9);
        }
        self.requests.push((task, at_ms));
        Ok(())
    }

    /// 让位落账（判据「让位延迟」）：让位时刻 - 请求时刻 = 延迟。
    pub fn yield_at(&mut self, task: u32, at_ms: u32) -> Result<u32, u16> {
        let pos = self
            .requests
            .iter()
            .position(|(t, _)| *t == task)
            .ok_or(SEG_BASE + 10)?;
        let (_, req_at) = self.requests.remove(pos);
        let lat = at_ms.saturating_sub(req_at);
        self.yield_lats.push(lat);
        Ok(lat)
    }

    /// 让位延迟 P95（判据「让位延迟 P95」）：样本 ≥20 才有效。
    pub fn yield_p95_ms(&self) -> Option<u32> {
        if self.yield_lats.len() < 20 {
            return None;
        }
        let mut s = self.yield_lats.clone();
        s.sort_unstable();
        Some(s[(s.len() * 95) / 100])
    }

    /// 让位点总数。
    pub fn mark_total(&self) -> usize {
        self.marks.iter().map(|(_, l)| l.len()).sum()
    }

    /// 请求残留（未让位的请求 = 悬空抢占，审计红项）。
    pub fn dangling_requests(&self) -> usize {
        self.requests.len()
    }
}

impl Default for PreemptMarks {
    fn default() -> Self {
        Self::new()
    }
}

/// 发射遥测汇总（判据「遥测」）：队列档案一次投影。
pub struct EmitTelemetry {
    /// 发射总数。
    pub emitted_total: u32,
    /// 老化越级总数。
    pub aged_total: u32,
    /// 让位 P95。
    pub yield_p95: Option<u32>,
    /// 悬空请求。
    pub dangling: u32,
}

impl EmitTelemetry {
    /// 自检：老化越级不得超过发射总数 20%（越级常态化 = 三档
    /// 形同虚设）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.emitted_total == 0 {
            return Err(SEG_BASE + 11);
        }
        if self.aged_total * 5 > self.emitted_total {
            return Err(SEG_BASE + 12);
        }
        Ok(())
    }
}

/// 遥测合成（判据「遥测」收口）。
pub fn build_telemetry(q: &EmitQueue, p: &PreemptMarks) -> Result<EmitTelemetry, u16> {
    let t = EmitTelemetry {
        emitted_total: q.emitted_count() as u32,
        aged_total: q.aged_count(),
        yield_p95: p.yield_p95_ms(),
        dangling: p.dangling_requests() as u32,
    };
    t.validate()?;
    Ok(t)
}

impl EmitQueue {
    /// 发射延迟遥测（判据「遥测」）：每枚任务的等待跨度
    /// （cur_seq - 就绪 seq），供 P95 计算的原料。
    pub fn wait_spans(&self, cur_seq: u64) -> Vec<u64> {
        self.emitted
            .iter()
            .map(|(_, _, s)| cur_seq.saturating_sub(*s))
            .collect()
    }

    /// 老化越级率（千分比）：越级数 / 发射数。
    pub fn aged_permille(&self) -> Option<u32> {
        if self.emitted.is_empty() {
            return None;
        }
        Some((self.aged * 1000) / self.emitted.len() as u32)
    }
}

impl PreemptMarks {
    /// 让位点间隔 P95（判据「抢占点检查」节奏面）：间隔过密
    /// 已被 mark 拦，此处量化实际间隔分布。
    pub fn mark_interval_p95(&self) -> Option<u32> {
        let mut all: Vec<u32> = Vec::new();
        for (_, l) in &self.marks {
            for w in l.windows(2) {
                all.push(w[1] - w[0]);
            }
        }
        if all.len() < 20 {
            return None;
        }
        all.sort_unstable();
        Some(all[(all.len() * 95) / 100])
    }
}
