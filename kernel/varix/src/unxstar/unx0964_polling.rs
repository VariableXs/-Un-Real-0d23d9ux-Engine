//! unx0964_polling — UNX-F0964 Polling 态域内语义细化（纯轮询档行为契约）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0964 Polling 态域内语义细化（纯轮询档行为契约）
//! 判据原文摘录（卡面验收行）：纯轮询档行为契约逐条落账（轮询节拍/
//! 事件零丢/无中断寄望），档内违例 10/10 次必拦（开中断寄望/跳拍/
//! 事件旁路），档账回放零漂移。
//! 诊断段 0x7E80（本单独占，XcCode 闭集见下）。
//! 设计纪律：纯轮询档铁律——只靠 poll 取事件、不使能中断、节拍
//! 恒定不可跳；事件入账零丢失（收账恒等式：轮询次数×每拍量 ≥ 事件
//! 数）；全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7E80;

// ── 诊断段独占 · 纯轮询档错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_BEAT: u16 = SEG_ERR + 1;   // 跳拍
const E_INTR: u16 = SEG_ERR + 2;   // 档内使能中断（寄望中断）
const E_LOST: u16 = SEG_ERR + 3;   // 事件丢失
const E_BYPASS: u16 = SEG_ERR + 4; // 事件旁路（不走 poll 入账）
const E_STATE: u16 = SEG_ERR + 5;  // 档状态非法

/// 轮询节拍账：第几拍取到几个事件。
#[derive(Clone, Copy, Debug)]
pub struct BeatRec {
    pub beat: u32,
    pub got: u32,
    pub t_us: u64,
}

/// 纯轮询档执行器。
pub struct PollingMode {
    beat_us: u64,       // 节拍周期
    cur_beat: u32,
    last_t: u64,
    pub recs: Vec<BeatRec>,
    pub irq_enabled: bool,
    pub collected: u32,
}

impl PollingMode {
    pub fn new(beat_us: u64) -> Self {
        PollingMode { beat_us, cur_beat: 0, last_t: 0, recs: Vec::new(), irq_enabled: false, collected: 0 }
    }

    /// 恒等式：拍号连续 + 节拍等距 + 中断禁使能。
    pub fn invariant_ok(&self) -> bool {
        if self.irq_enabled { return false; }
        for (i, r) in self.recs.iter().enumerate() {
            if r.beat != i as u32 + 1 { return false; }
            if i > 0 && r.t_us - self.recs[i - 1].t_us != self.beat_us { return false; }
        }
        true
    }

    /// 跑一拍：按节拍时刻取事件；跳拍（时刻不连续）必拦。
    pub fn beat(&mut self, t_us: u64, got: u32) -> Result<(), u16> {
        if self.irq_enabled { return Err(E_INTR); }
        let expect = self.last_t + self.beat_us;
        if self.cur_beat > 0 && t_us != expect { return Err(E_BEAT); }
        if self.cur_beat == 0 && t_us < self.last_t { return Err(E_BEAT); }
        self.cur_beat += 1;
        self.recs.push(BeatRec { beat: self.cur_beat, got, t_us });
        self.last_t = t_us;
        self.collected += got;
        Ok(())
    }

    /// 事件入账：只有 beat 通道可加 collected；旁路注入器单独核。
    pub fn collected(&self) -> u32 { self.collected }

    /// 收账核验：拍数 × 单拍容量上限 ≥ collected（零丢失下界）。
    pub fn no_loss_audit(&self, cap_per_beat: u32, expect_events: u32) -> Result<(), u16> {
        let cap = self.recs.len() as u32 * cap_per_beat;
        if self.collected > cap { return Err(E_LOST); }
        if self.collected != expect_events { return Err(E_BYPASS); }
        Ok(())
    }

    /// 档内使能中断注入器：必拦。
    pub fn inject_irq_enable(&mut self) -> bool {
        self.irq_enabled = true;
        let caught = self.beat(self.last_t + self.beat_us, 1) == Err(E_INTR);
        self.irq_enabled = false;
        caught
    }

    /// 跳拍注入器：时刻乱序必拦。
    pub fn inject_beat_skip(&mut self) -> bool {
        let bad_t = self.last_t + self.beat_us * 3; // 跳三拍
        self.beat(bad_t, 1) == Err(E_BEAT)
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        alloc::format!("polling beats={} collected={} beat_us={}", self.recs.len(), self.collected, self.beat_us)
    }
}

/// 纯轮询档会话：完整跑 N 拍收齐事件。
pub fn polling_session(pm: &mut PollingMode, beats: u32, per_beat: u32, t0: u64) -> Result<u32, u16> {
    for i in 0..beats {
        let t = if i == 0 { t0 } else { t0 + i as u64 * pm.beat_us };
        pm.beat(t, per_beat)?;
    }
    Ok(pm.collected)
}

/// 自例程：节拍守恒、跳拍/使能中断/旁路 10/10 必拦返回 0。
pub fn drill() -> Result<u32, u16> {
    // 场景一：标准会话 100 拍 × 每拍 4 事件。
    let mut pm = PollingMode::new(10);
    if polling_session(&mut pm, 100, 4, 0)? != 400 { return Err(E_LOST); }
    if !pm.invariant_ok() { return Err(E_BEAT); }
    pm.no_loss_audit(4, 400)?;
    // 场景二：跳拍注入 10/10 必拦。
    for _ in 0..10 {
        let mut p2 = PollingMode::new(10);
        p2.beat(0, 1)?;
        if !p2.inject_beat_skip() { return Err(E_BEAT); }
    }
    // 场景三：使能中断注入 10/10 必拦。
    for _ in 0..10 {
        let mut p3 = PollingMode::new(10);
        if !p3.inject_irq_enable() { return Err(E_INTR); }
    }
    // 场景四：事件旁路注入（collected 与期望不符）必拦。
    let mut p4 = PollingMode::new(10);
    p4.beat(0, 5)?;
    if p4.no_loss_audit(4, 5) != Err(E_LOST) { return Err(E_LOST); } // 单拍超容量
    if p4.no_loss_audit(5, 4) != Err(E_BYPASS) { return Err(E_BYPASS); }
    // 场景五：拍号连续性。
    for (i, r) in p4.recs.iter().enumerate() {
        if r.beat != i as u32 + 1 { return Err(E_BEAT); }
    }
    if pm.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}
