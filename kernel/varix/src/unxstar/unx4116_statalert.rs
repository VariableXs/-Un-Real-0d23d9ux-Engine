//! unx4116_statalert — UNX-F4116 统计快照与阈值告警（诊断段 0x9080）。
//!
//! 判据原文（提炼）：
//!   J1 命中率跌破阈值产生告警事件一条：跌破沿产生且仅产生 1 条
//!      （事件账可查，破位持续期不重复告警）。
//!   J2 阈值可配置：set_threshold 改参后行为随动（同水位不同阈值
//       产生不同告警结论）。
//!   J3 正常水位零告警（零误报断言）：水位始终高于阈值时事件账为空。
//!   J4 恢复零告警：回升越阈不产生"恢复事件"重复告警（单沿语义），
//!      恢复后再次跌破才允许第二条。
//!   J5 快照可查：任意时刻 snapshot 返回（请求数/命中数/千分比）
//!      三元组，与累计账复算一致。
//! 设计纪律：
//!   1. 告警单沿状态机：Normal →（跌破）Breached →（回升）Normal，
//!      告警事件只在 Normal→Breached 沿产生，一沿一条。
//!   2. 阈值千分比整数化：PERMILLE 域 [0,1000]，命中率 =
//!      hits × 1000 / lookups（整数除法，lookups=0 不评估）。
//!   3. 评估粒度 = 请求：每笔请求后评估一次水位，跌破判定即时。
//!   4. 事件账只增不删：告警事件含（序号/触发快照千分比/阈值），
//!      可审计不可篡改。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9080 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9080;

/// 默认告警阈值（千分比）：命中率低于 800‰ 告警（批内对照面）。
pub const DEFAULT_THRESHOLD: u32 = 800;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_THRESH_RANGE: u16 = SEG_BASE + 1; // 阈值超 [0,1000] 域
const E_ZERO_LOOKUPS: u16 = SEG_BASE + 2; // 零请求评估（无水位可判）
const E_BAL: u16 = SEG_BASE + 3;          // 快照与累计账复算不一致
const E_FALSE_ALARM: u16 = SEG_BASE + 4;  // 正常水位产生告警（误报）
const E_DUP_ALERT: u16 = SEG_BASE + 5;    // 破位持续期重复告警（判据 J1）
const E_COUNT_BAD: u16 = SEG_BASE + 6;    // 命中数超请求数（账坏）

/// 告警状态机：Normal ↔ Breached 单沿告警。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlertState {
    Normal,   // 水位正常（或无数据）
    Breached, // 已跌破，持续告警期（不再重复出事件）
}

/// 告警事件留痕。
pub struct AlertEvent {
    pub seq: u32,       // 事件序号（从 1 起）
    pub ratio: u32,     // 触发时命中率（‰）
    pub threshold: u32, // 触发时阈值（‰）
}

/// 统计快照（判据 J5 三元组）。
pub struct StatSnapshot {
    pub lookups: u32,       // 累计请求数
    pub hits: u32,          // 累计命中数（正+负命中）
    pub ratio_permille: u32,// 当前命中率（‰）
}

/// 告警引擎：水位账 + 阈值 + 单沿状态机 + 事件账。
pub struct AlertEngine {
    pub lookups: u32,          // 累计请求
    pub hits: u32,             // 累计命中
    pub threshold: u32,        // 当前阈值（‰，可配置）
    pub threshold_changes: u32,// 阈值变更次数（判据 J2 审计）
    pub state: AlertState,     // 单沿状态机
    pub events: Vec<AlertEvent>, // 事件账（只增不删）
    pub evaluated: u32,        // 评估次数
}

impl AlertEngine {
    pub fn new() -> AlertEngine {
        AlertEngine {
            lookups: 0,
            hits: 0,
            threshold: DEFAULT_THRESHOLD,
            threshold_changes: 0,
            state: AlertState::Normal,
            events: Vec::new(),
            evaluated: 0,
        }
    }

    /// 改阈值（判据 J2）：域校验后随动。
    pub fn set_threshold(&mut self, t: u32) -> Result<(), u16> {
        if t > 1000 {
            return Err(E_THRESH_RANGE);
        }
        self.threshold = t;
        self.threshold_changes += 1;
        Ok(())
    }

    /// 记一笔请求（命中与否），随后立即评估水位（判据 J1/J3）。
    pub fn record(&mut self, hit: bool) -> Result<(), u16> {
        self.lookups += 1;
        if hit {
            self.hits += 1;
        }
        if self.hits > self.lookups {
            return Err(E_COUNT_BAD); // 账坏防线
        }
        self.evaluate()
    }

    /// 水位评估（单沿语义）：Normal 跌破→出一条事件转 Breached；
    /// Breached 回升→静默转 Normal（判据 J4）。
    fn evaluate(&mut self) -> Result<(), u16> {
        if self.lookups == 0 {
            return Err(E_ZERO_LOOKUPS);
        }
        self.evaluated += 1;
        let ratio = self.hits * 1000 / self.lookups;
        match self.state {
            AlertState::Normal => {
                if ratio < self.threshold {
                    // 跌破沿：出且仅出一条事件（判据 J1）
                    let seq = self.events.len() as u32 + 1;
                    self.events.push(AlertEvent {
                        seq,
                        ratio,
                        threshold: self.threshold,
                    });
                    self.state = AlertState::Breached;
                }
                Ok(())
            }
            AlertState::Breached => {
                if ratio >= self.threshold {
                    self.state = AlertState::Normal; // 回升沿静默（判据 J4）
                }
                Ok(())
            }
        }
    }

    /// 评估探针（审计面）：供外部触发一次裸评估（零请求应拒）。
    pub fn evaluate_probe(&mut self) -> Result<(), u16> {
        self.evaluate()
    }

    /// 快照（判据 J5）：三元组与累计账复算一致。
    pub fn snapshot(&self) -> Result<StatSnapshot, u16> {
        if self.hits > self.lookups {
            return Err(E_COUNT_BAD);
        }
        let ratio = if self.lookups == 0 {
            0
        } else {
            self.hits * 1000 / self.lookups
        };
        Ok(StatSnapshot {
            lookups: self.lookups,
            hits: self.hits,
            ratio_permille: ratio,
        })
    }

    /// 快照复算核验（判据 J5）：快照比值与账面重算一致。
    pub fn snapshot_consistent(&self, s: &StatSnapshot) -> bool {
        if s.lookups != self.lookups || s.hits != self.hits {
            return false;
        }
        let expect = if self.lookups == 0 {
            0
        } else {
            self.hits * 1000 / self.lookups
        };
        s.ratio_permille == expect
    }

    /// 零误报断言（判据 J3）：事件账空且状态 Normal 才绿。
    pub fn no_false_alarm(&self) -> bool {
        self.events.is_empty() && self.state == AlertState::Normal
    }

    /// 破位持续期无重复告警核验（判据 J1 推论）：
    /// 事件数 == 历史跌破沿数（由单沿状态机保证，此处核事件序号连续）。
    pub fn events_sane(&self) -> bool {
        let mut i = 0usize;
        while i < self.events.len() {
            if self.events[i].seq != i as u32 + 1 {
                return false;
            }
            if self.events[i].ratio >= self.events[i].threshold {
                return false; // 高于阈值不得出事件
            }
            i += 1;
        }
        true
    }

    /// 事件账条数（审计面）。
    pub fn event_count(&self) -> u32 {
        self.events.len() as u32
    }

    /// 点名事件读取（判据 J1 事件账可查面）：序号越界拒收。
    /// 返回（触发千分比，触发时阈值）。
    pub fn event_at(&self, seq: u32) -> Result<(u32, u32), u16> {
        if seq == 0 || seq > self.events.len() as u32 {
            return Err(E_BAL); // 序号从 1 起，越界即账错误
        }
        let e = &self.events[(seq - 1) as usize];
        Ok((e.ratio, e.threshold))
    }

    /// 统计清零（复演面）：请求/命中/状态/事件全清，阈值保留
    /// （配置不属于统计，判据 J2 改参后复演需要保留）。
    pub fn reset_stats(&mut self) {
        self.lookups = 0;
        self.hits = 0;
        self.state = AlertState::Normal;
        self.events.clear();
        self.evaluated = 0;
    }

    /// 评估次数读取（审计面）。
    pub fn evaluations(&self) -> u32 {
        self.evaluated
    }
}

/// 自例程：R1 正常水位零告警→R2 跌破单沿一条→R3 阈值随动→R4 快照复算。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 正常水位（全命中，1000‰ ≥ 800‰）：零告警（判据 J3）。
    let mut e = AlertEngine::new();
    let mut i = 0u32;
    while i < 20 {
        e.record(true)?;
        i += 1;
    }
    if !e.no_false_alarm() {
        return Err(E_FALSE_ALARM);
    }
    if e.snapshot()?.ratio_permille != 1000 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 跌破：注 8 笔未中把水位拉到 (20/28)=714‰ < 800‰，
    // 单沿出 1 条事件；持续未中不再重复（判据 J1）。
    let mut j = 0u32;
    while j < 8 {
        e.record(false)?;
        j += 1;
    }
    if e.events.len() != 1 {
        return Err(E_DUP_ALERT); // 必须恰好一条
    }
    let mut k = 0u32;
    while k < 10 {
        e.record(false)?; // 持续破位期
        k += 1;
    }
    if e.events.len() != 1 {
        return Err(E_DUP_ALERT); // 破位期不得重复告警
    }
    if e.state != AlertState::Breached {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 阈值随动（判据 J2/J4）：降阈到 500‰ 后水位 (20/38)=526‰ ≥ 500
    // → 回升沿静默转 Normal；再升阈到 900‰ 后下一笔未中即第二条事件。
    e.set_threshold(500)?;
    e.record(false)?; // 20/39=512‰ ≥ 500 → 恢复
    if e.state != AlertState::Normal || e.events.len() != 1 {
        return Err(E_BAL); // 恢复零告警（判据 J4）
    }
    e.set_threshold(900)?;
    e.record(false)?; // 20/40=500‰ < 900 → 第二条
    if e.events.len() != 2 {
        return Err(E_BAL);
    }
    if e.set_threshold(1001).is_ok() {
        return Err(E_THRESH_RANGE); // 超域应拒
    }
    if e.threshold_changes != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 快照复算与事件账终审（判据 J5/J1）。
    let s = e.snapshot()?;
    if !e.snapshot_consistent(&s) {
        return Err(E_BAL);
    }
    if !e.events_sane() {
        return Err(E_BAL);
    }
    if e.events[0].threshold != DEFAULT_THRESHOLD {
        return Err(E_BAL); // 第一条按默认阈值触发
    }
    if e.events[1].threshold != 900 {
        return Err(E_BAL); // 第二条按改后阈值触发
    }
    // 零请求评估防线：新引擎直接评估应拒。
    let mut z = AlertEngine::new();
    if z.evaluate_probe().is_ok() {
        return Err(E_ZERO_LOOKUPS);
    }
    hits += 1;

    // R5 审计面：点名事件读取、越界拒收、清零复演。
    let (r0, t0) = e.event_at(1)?;
    if r0 >= t0 {
        return Err(E_BAL); // 事件触发比必须低于阈值
    }
    if r0 != 714 {
        return Err(E_BAL); // 首事件触发比 20/28 = 714‰
    }
    if e.event_at(0).is_ok() || e.event_at(3).is_ok() {
        return Err(E_BAL); // 序号从 1 起，越界应拒
    }
    if e.event_count() != 2 {
        return Err(E_BAL);
    }
    if e.evaluations() < 40 {
        return Err(E_BAL); // 每笔请求一评估（28+10+1+1 笔以上）
    }
    // 清零复演：统计清零阈值保留（900‰），复演第二破位即刻重现。
    e.reset_stats();
    if !e.no_false_alarm() {
        return Err(E_FALSE_ALARM); // 清零后必须回零告警态
    }
    if e.threshold != 900 {
        return Err(E_BAL); // 阈值配置不属于统计，必须保留
    }
    e.record(true)?;
    e.record(false)?; // 1/2 = 500‰ < 900 → 即刻告警
    if e.event_count() != 1 {
        return Err(E_BAL); // 复演确定性
    }
    hits += 1;

    Ok(hits)
}
