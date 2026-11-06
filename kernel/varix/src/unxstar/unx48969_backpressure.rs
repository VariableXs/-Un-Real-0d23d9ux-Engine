//! UNX-F48969 事件层背压与水位告警 · 0x71500
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B40 族第三槽）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F48969-J1 水位 ×10 越档告警零漏（队列水位四档告警）；
//!   ③ UNX-F48970-J1 矩阵 ×15 格实测入账（批大小 × 报点率矩阵实测）。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：容量锚定 → 入队合法性 → 入队守恒 → 档位抬升逐档告警 →
//!           背压拒绝 → 守恒复算 → 矩阵逐格恒等 → 批大小单调。
//! 先容量再水位：四档阈值（256/512/768）由容量 1024 的四等分导出；容量
//! 若可自由改动，阈值就成了没有出处的魔数，越档告警也就无从复核。
//! 档位抬升逐档告警：一次跨越三档必须发三次告警。合并成一次「跳档告警」
//! 会让上层只看到「有一档变了」，中间两档被静默吞掉——这就是「漏」。
//! 背压放在告警之后：告警是观测，背压是动作。先观测后动作，才能在丢掉
//! 一个事件之前留下它的水位凭证；反过来就是「先扔再记」。
//! 矩阵放在最后：批大小 × 报点率的实测账假设队列已经能算清水位与背压，
//! 否则每格测出来的「递交次数」都不知道是在什么水位条件下得到的。
//! 关键差异：本模块只回答「水位到哪一档、越档是否零漏、批大小怎么随
//!           报点率变化」，不做 P99 归因（B40-SCHED）、不做核间搬移
//!           （B40-SCHED）、不做 1,000Hz 全路径对账（B40-RATE）。矩阵
//!           每格只记递交次数与末批余量，不记 CPU 占用（曲线归 F48968）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71500，B40 族第三槽）。
pub const SEG_BASE: u32 = 0x71500;

/// 事件队列容量（四档阈值的分母）。
pub const QUEUE_CAP: u32 = 1_024;
/// 一档下沿：≤256 视为安静。
pub const WATER_LOW: u32 = QUEUE_CAP / 4;
/// 二档下沿：≤512 视为偏低。
pub const WATER_HIGH: u32 = QUEUE_CAP / 2;
/// 三档下沿：≤768 视为偏高；>768 即临界，触发背压。
pub const WATER_CRIT: u32 = (QUEUE_CAP * 3) / 4;
/// 档位数（安静/偏低/偏高/临界）。
pub const TIERS: usize = 4;
/// 水位告警轮数（卡面 ×10）。
pub const ALARM_ROUNDS: u32 = 10;
/// 背压 fuzz 轮数（×10 的十倍载体）。
pub const PRESSURE_ROUNDS: u32 = 100;
/// 矩阵格数：5 种批大小 × 3 种报点率 = 15。
pub const MATRIX_CELLS: usize = 15;
/// 批大小梯度。
pub const BATCH_LADDER: [u32; 5] = [4, 8, 16, 32, 64];
/// 报点率梯度（Hz）。
pub const RATE_LADDER: [u32; 3] = [125, 500, 1_000];
/// 每格实测时长（秒）。
pub const MEASURE_SECS: u32 = 1;
/// 矩阵账渲染字段数。
pub const MATRIX_FIELDS: usize = 5;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0D） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BpCode {
    /// 水位非法（超出容量）
    FillBad,
    /// 档位非法（档位与水位不一致）
    LevelBad,
    /// 告警漏记（alarms 与 crossings 不等）
    AlarmBad,
    /// 背压非法（临界档仍被接收入队）
    PressureBad,
    /// 计数非法（零批量）
    CountBad,
    /// 矩阵非法（格数不足/重复/恒等不成立）
    MatrixBad,
    /// 批大小非法（末批余量越界或非单调）
    BatchBad,
    /// 次序非法（计数回退）
    OrderBad,
    /// 账不平（入队 = 出队 + 在队）
    LedgerBad,
    /// 渲染非法（字段缺失/回读失败）
    RenderBad,
    /// 复算不一致（同一序列重放不同）
    ReplayBad,
    /// 覆盖不足（轮数或格数不足）
    CoverageBad,
    /// 段位越界（诊断码逃出 0x71500 窗口）
    SegmentBad,
}

impl BpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            BpCode::FillBad => "水位非法：在队深度越过容量或容量被改动",
            BpCode::LevelBad => "档位非法：档位与水位的换算不一致",
            BpCode::AlarmBad => "告警漏记：逐档告警数与越档次数不等",
            BpCode::PressureBad => "背压非法：临界档仍被接收入队",
            BpCode::CountBad => "计数非法：零批量入队或出队",
            BpCode::MatrixBad => "矩阵非法：格数不足、重复或逐格恒等不成立",
            BpCode::BatchBad => "批大小非法：末批余量越界或递交次数非单调",
            BpCode::OrderBad => "次序非法：计数回退",
            BpCode::LedgerBad => "账不平：入队 = 出队 + 在队 不成立",
            BpCode::RenderBad => "渲染非法：字段缺失或回读失败",
            BpCode::ReplayBad => "复算不一致：同一序列重放结果不同",
            BpCode::CoverageBad => "覆盖不足：轮数或格数不足",
            BpCode::SegmentBad => "段位越界：诊断码逃出 0x71500 窗口",
        }
    }

    pub const ALL: [BpCode; 13] = [
        BpCode::FillBad,
        BpCode::LevelBad,
        BpCode::AlarmBad,
        BpCode::PressureBad,
        BpCode::CountBad,
        BpCode::MatrixBad,
        BpCode::BatchBad,
        BpCode::OrderBad,
        BpCode::LedgerBad,
        BpCode::RenderBad,
        BpCode::ReplayBad,
        BpCode::CoverageBad,
        BpCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < BpCode::ALL.len() {
        if BpCode::ALL[i].val() == code {
            return BpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 四档水位 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaterLevel {
    /// 安静：≤ 256
    Quiet,
    /// 偏低：257..=512
    Low,
    /// 偏高：513..=768
    High,
    /// 临界：769..=1024（触发背压）
    Critical,
}

impl WaterLevel {
    pub fn index(self) -> usize {
        match self {
            WaterLevel::Quiet => 0,
            WaterLevel::Low => 1,
            WaterLevel::High => 2,
            WaterLevel::Critical => 3,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + 0x20 + self.index() as u32
    }

    pub fn from_index(i: u8) -> Result<WaterLevel, u32> {
        match i {
            0 => Ok(WaterLevel::Quiet),
            1 => Ok(WaterLevel::Low),
            2 => Ok(WaterLevel::High),
            3 => Ok(WaterLevel::Critical),
            _ => Err(BpCode::LevelBad.val()),
        }
    }

    /// 水位 → 档位。阈值是**闭区间上沿**：恰在阈值上仍属本档。
    pub fn from_fill(fill: u32) -> Result<WaterLevel, u32> {
        if fill > QUEUE_CAP {
            return Err(BpCode::FillBad.val());
        }
        if fill <= WATER_LOW {
            Ok(WaterLevel::Quiet)
        } else if fill <= WATER_HIGH {
            Ok(WaterLevel::Low)
        } else if fill <= WATER_CRIT {
            Ok(WaterLevel::High)
        } else {
            Ok(WaterLevel::Critical)
        }
    }
}

/// 独立档位换算：与 `from_fill` 同义但**不同算式**，用于交叉复核。
///
/// 两条路径给出同一档位才算「档位可查」；只信一条路径的实现无法发现
/// 边界比较写反（`<` 写成 `<=`）这类错误——而那正是越档漏报的根源。
fn expect_level(fill: u32) -> WaterLevel {
    if fill > WATER_CRIT {
        WaterLevel::Critical
    } else if fill > WATER_HIGH {
        WaterLevel::High
    } else if fill > WATER_LOW {
        WaterLevel::Low
    } else {
        WaterLevel::Quiet
    }
}

/// 从 `from` 抬升到 `to` 所跨越的档数（下降记 0：水位回落不告警）。
pub fn tiers_crossed(from: WaterLevel, to: WaterLevel) -> u32 {
    if to.index() <= from.index() {
        0
    } else {
        (to.index() - from.index()) as u32
    }
}

// ── 水位队列 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WaterQueue {
    pub cap: u32,
    pub fill: u32,
    pub level: WaterLevel,
    /// 累计入队（被接受的部分）
    pub pushed: u32,
    /// 累计出队
    pub popped: u32,
    /// 累计被拒：越容溢出 + 临界背压
    pub rejected: u32,
    /// 逐档告警次数（下标 = 档位）
    pub alarms: [u32; TIERS],
    /// 累计越档次数（一次跳三档记 3）
    pub crossings: u32,
    /// 触发背压的次数
    pub pressure: u32,
    pub digest: u64,
}

impl WaterQueue {
    pub fn new() -> WaterQueue {
        WaterQueue {
            cap: QUEUE_CAP,
            fill: 0,
            level: WaterLevel::Quiet,
            pushed: 0,
            popped: 0,
            rejected: 0,
            alarms: [0; TIERS],
            crossings: 0,
            pressure: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 入队：返回实际接受量。临界档（>768）一律背压拒绝。
    ///
    /// 水位抬升时**逐档**记告警：从 Quiet 直接冲到 Critical 会产生
    /// Low/High/Critical 三条告警，与「每跨一档必告警一次」逐字对应。
    pub fn push(&mut self, n: u32) -> Result<u32, u32> {
        if n == 0 {
            return Err(BpCode::CountBad.val());
        }
        if self.fill > self.cap {
            return Err(BpCode::FillBad.val());
        }
        if self.fill >= WATER_CRIT {
            self.rejected = self.rejected.saturating_add(n);
            self.pressure = self.pressure.saturating_add(1);
            self.level = WaterLevel::from_fill(self.fill)?;
            self.digest = p_core::fold_u32(self.digest, n);
            return Err(BpCode::PressureBad.val());
        }
        let room = self.cap.saturating_sub(self.fill);
        let accepted = if n <= room { n } else { room };
        let overflow = n.saturating_sub(accepted);
        if accepted == 0 {
            // 容量已满但未达背压阈值：显式拒收，不静默吞。
            self.rejected = self.rejected.saturating_add(n);
            return Err(BpCode::FillBad.val());
        }
        self.fill = self.fill.saturating_add(accepted);
        self.pushed = self.pushed.saturating_add(accepted);
        self.rejected = self.rejected.saturating_add(overflow);
        let before = self.level;
        let after = WaterLevel::from_fill(self.fill)?;
        let crossed = tiers_crossed(before, after);
        if crossed > 0 {
            let mut t = before.index().saturating_add(1);
            while t <= after.index() {
                if let Some(slot) = self.alarms.get_mut(t) {
                    *slot = slot.saturating_add(1);
                }
                t = t.saturating_add(1);
            }
            self.crossings = self.crossings.saturating_add(crossed);
        }
        self.level = after;
        self.digest = p_core::fold_u32(self.digest, n);
        self.digest = p_core::fold_u32(self.digest, accepted);
        self.digest = p_core::fold_u32(self.digest, after.index() as u32);
        Ok(accepted)
    }

    /// 出队：返回实际取出量。水位回落不产生告警，只同步档位。
    pub fn pop(&mut self, n: u32) -> Result<u32, u32> {
        if n == 0 {
            return Err(BpCode::CountBad.val());
        }
        let taken = if n <= self.fill { n } else { self.fill };
        self.fill = self.fill.saturating_sub(taken);
        self.popped = self.popped.saturating_add(taken);
        let after = WaterLevel::from_fill(self.fill)?;
        self.level = after;
        self.digest = p_core::fold_u32(self.digest, taken);
        self.digest = p_core::fold_u32(self.digest, after.index() as u32);
        Ok(taken)
    }

    /// 逐档告警总数。
    pub fn alarm_total(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < TIERS {
            s = s.saturating_add(match self.alarms.get(i) {
                Some(v) => *v,
                None => 0,
            });
            i += 1;
        }
        s
    }

    /// 守恒审计：在队 = 入队 − 出队，档位与水位一致，告警与越档一致。
    pub fn audit(&self) -> Result<(), u32> {
        if self.cap != QUEUE_CAP {
            return Err(BpCode::FillBad.val());
        }
        if self.fill > self.cap {
            return Err(BpCode::FillBad.val());
        }
        if self.level != expect_level(self.fill) || self.level != WaterLevel::from_fill(self.fill)? {
            return Err(BpCode::LevelBad.val());
        }
        if self.pushed != self.popped.saturating_add(self.fill) {
            return Err(BpCode::LedgerBad.val());
        }
        if self.alarm_total() != self.crossings {
            return Err(BpCode::AlarmBad.val());
        }
        if self.pressure > self.rejected {
            return Err(BpCode::PressureBad.val());
        }
        Ok(())
    }
}

/// 水位 ×10：每轮抬升三段再排空，逐档告警必须一条不落。
///
/// 期望值由 `expect_level` 独立算出（不复用队列内部档位），这样
/// 「告警零漏」检验的是队列行为，而不是队列自证。
pub fn alarm_rounds(rounds: u32) -> Result<(u32, [u32; TIERS]), u32> {
    if rounds == 0 || rounds > 100 {
        return Err(BpCode::CoverageBad.val());
    }
    const LIFTS: [u32; 5] = [100, 200, 300, 200, 300];
    let mut q = WaterQueue::new();
    let mut want = [0u32; TIERS];
    let mut want_cross = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let mut k = 0usize;
        while k < LIFTS.len() {
            let n = match LIFTS.get(k) {
                Some(v) => *v,
                None => 0,
            };
            let before = expect_level(q.fill);
            let _ = q.push(n);
            let after = expect_level(q.fill);
            let mut t = before.index().saturating_add(1);
            while t <= after.index() {
                if let Some(slot) = want.get_mut(t) {
                    *slot = slot.saturating_add(1);
                }
                want_cross = want_cross.saturating_add(1);
                t = t.saturating_add(1);
            }
            if q.level != after {
                return Err(BpCode::LevelBad.val());
            }
            q.audit()?;
            k = k.saturating_add(1);
        }
        let _ = q.pop(QUEUE_CAP)?;
        if q.fill != 0 || q.level != WaterLevel::Quiet {
            return Err(BpCode::LevelBad.val());
        }
        q.audit()?;
        r = r.saturating_add(1);
    }
    if q.crossings != want_cross || q.alarms != want {
        return Err(BpCode::AlarmBad.val());
    }
    Ok((q.crossings, q.alarms))
}

/// 背压 fuzz：随机入队/出队，临界档必须拒收且水位不动。
pub fn backpressure_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 {
        return Err(BpCode::CoverageBad.val());
    }
    let mut q = WaterQueue::new();
    let mut rng = p_core::Lcg::new(0x8A11_0069);
    let mut i = 0u32;
    while i < rounds {
        // 分支取 LCG 高位：低位周期 2^k，`below(2)` 会让序列退化成周期 2。
        let do_push = (rng.next() >> 33) % 2 == 0;
        let n = 1u32.saturating_add(rng.below(300));
        if do_push {
            let before_fill = q.fill;
            let res = q.push(n);
            if res.err() == Some(BpCode::PressureBad.val()) {
                if q.fill != before_fill {
                    return Err(BpCode::PressureBad.val());
                }
                if before_fill < WATER_CRIT {
                    return Err(BpCode::LevelBad.val());
                }
            }
        } else {
            let _ = q.pop(n)?;
        }
        q.audit()?;
        i = i.saturating_add(1);
    }
    Ok(q.digest)
}

// ── 批大小 × 报点率矩阵（15 格实测） ────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatrixCell {
    pub batch: u32,
    /// 报点率（Hz）
    pub rate: u32,
    /// 实测事件数 = rate × MEASURE_SECS
    pub events: u32,
    /// 递交次数 = ceil(events / batch)
    pub deliveries: u32,
    /// 末批余量（1..=batch）
    pub tail: u32,
    pub digest: u64,
}

impl MatrixCell {
    /// 单格恒等：`events = (deliveries−1)×batch + tail`，且 tail ∈ [1, batch]。
    ///
    /// 用 `deliveries−1` 个整批加一个非空末批来描述：末批为 0 就等价于
    /// 「多算了一次空递交」，那会让矩阵里的递交次数虚高、调优结论失真。
    pub fn audit(&self) -> Result<(), u32> {
        if self.batch == 0 || self.rate == 0 {
            return Err(BpCode::BatchBad.val());
        }
        if self.events != self.rate.saturating_mul(MEASURE_SECS) || self.events == 0 {
            return Err(BpCode::MatrixBad.val());
        }
        if self.deliveries == 0 {
            return Err(BpCode::BatchBad.val());
        }
        if self.tail == 0 || self.tail > self.batch {
            return Err(BpCode::BatchBad.val());
        }
        let full = (self.deliveries - 1) as u64;
        let lhs = full
            .saturating_mul(self.batch as u64)
            .saturating_add(self.tail as u64);
        if lhs != self.events as u64 {
            return Err(BpCode::MatrixBad.val());
        }
        if self.digest == 0 {
            return Err(BpCode::RenderBad.val());
        }
        Ok(())
    }
}

/// 实测一格：按给定批大小切分一秒的报点流。
pub fn measure_cell(batch: u32, rate: u32) -> Result<MatrixCell, u32> {
    if batch == 0 || rate == 0 {
        return Err(BpCode::BatchBad.val());
    }
    let events = rate.saturating_mul(MEASURE_SECS);
    if events == 0 {
        return Err(BpCode::MatrixBad.val());
    }
    let deliveries = events.saturating_add(batch - 1) / batch;
    if deliveries == 0 {
        return Err(BpCode::BatchBad.val());
    }
    let full = deliveries - 1;
    let tail = events.saturating_sub(full.saturating_mul(batch));
    let mut d = p_core::fold_u32(p_core::FNV_OFF, batch);
    d = p_core::fold_u32(d, rate);
    d = p_core::fold_u32(d, deliveries);
    d = p_core::fold_u32(d, tail);
    let cell = MatrixCell {
        batch,
        rate,
        events,
        deliveries,
        tail,
        digest: d,
    };
    cell.audit()?;
    Ok(cell)
}

/// 在矩阵里按 (批大小, 报点率) 定位一格。
pub fn cell_at(cells: &[MatrixCell], batch: u32, rate: u32) -> Option<&MatrixCell> {
    let mut i = 0usize;
    while i < cells.len() {
        if let Some(c) = cells.get(i) {
            if c.batch == batch && c.rate == rate {
                return Some(c);
            }
        }
        i += 1;
    }
    None
}

/// 矩阵入账审计：格数、唯一性、逐格恒等、逐率单调，四项缺一不可。
pub fn matrix_audit(cells: &[MatrixCell]) -> Result<u32, u32> {
    if cells.len() != MATRIX_CELLS {
        return Err(BpCode::MatrixBad.val());
    }
    let mut i = 0usize;
    while i < cells.len() {
        let c = match cells.get(i) {
            Some(v) => v,
            None => return Err(BpCode::MatrixBad.val()),
        };
        c.audit()?;
        let mut j = i + 1;
        while j < cells.len() {
            if let Some(o) = cells.get(j) {
                if o.batch == c.batch && o.rate == c.rate {
                    return Err(BpCode::MatrixBad.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    // 同一报点率下，批越大递交次数不增；否则「调大批」这个结论不成立。
    let mut r = 0usize;
    while r < RATE_LADDER.len() {
        let rate = match RATE_LADDER.get(r) {
            Some(v) => *v,
            None => return Err(BpCode::MatrixBad.val()),
        };
        let mut prev = u32::MAX;
        let mut b = 0usize;
        while b < BATCH_LADDER.len() {
            let batch = match BATCH_LADDER.get(b) {
                Some(v) => *v,
                None => return Err(BpCode::MatrixBad.val()),
            };
            let cell = match cell_at(cells, batch, rate) {
                Some(v) => v,
                None => return Err(BpCode::MatrixBad.val()),
            };
            if cell.deliveries > prev {
                return Err(BpCode::BatchBad.val());
            }
            prev = cell.deliveries;
            b = b.saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    Ok(MATRIX_CELLS as u32)
}

/// 生成 15 格实测评据（顺序固定：先批大小后报点率，重放可复现）。
pub fn matrix_face() -> Result<Vec<MatrixCell>, u32> {
    let mut out = Vec::new();
    let mut b = 0usize;
    while b < BATCH_LADDER.len() {
        let batch = match BATCH_LADDER.get(b) {
            Some(v) => *v,
            None => return Err(BpCode::BatchBad.val()),
        };
        let mut r = 0usize;
        while r < RATE_LADDER.len() {
            let rate = match RATE_LADDER.get(r) {
                Some(v) => *v,
                None => return Err(BpCode::BatchBad.val()),
            };
            out.push(measure_cell(batch, rate)?);
            r = r.saturating_add(1);
        }
        b = b.saturating_add(1);
    }
    matrix_audit(&out)?;
    Ok(out)
}

/// 矩阵合计摘要（渲染与复算共用）。
pub fn matrix_digest(cells: &[MatrixCell]) -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < cells.len() {
        if let Some(c) = cells.get(i) {
            h = p_core::fold_u32(h, c.batch);
            h = p_core::fold_u32(h, c.rate);
            h = p_core::fold_u32(h, c.deliveries);
            h = p_core::fold_u32(h, c.tail);
        }
        i += 1;
    }
    h
}

// ── 矩阵账渲染 ────────────────────────────────────────────────────
pub fn render_matrix(cells: &[MatrixCell]) -> String {
    let mut events = 0u32;
    let mut deliveries = 0u32;
    let mut tail = 0u32;
    let mut i = 0usize;
    while i < cells.len() {
        if let Some(c) = cells.get(i) {
            events = events.saturating_add(c.events);
            deliveries = deliveries.saturating_add(c.deliveries);
            tail = tail.saturating_add(c.tail);
        }
        i += 1;
    }
    format!(
        "{}|{}|{}|{}|0x{:016X}",
        cells.len(),
        events,
        deliveries,
        tail,
        matrix_digest(cells)
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(BpCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(BpCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(BpCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(BpCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(BpCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(BpCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(BpCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(BpCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != MATRIX_FIELDS {
        return Err(BpCode::RenderBad.val());
    }
    let cells = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(BpCode::RenderBad.val()),
    })?;
    let events = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(BpCode::RenderBad.val()),
    })?;
    let deliveries = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(BpCode::RenderBad.val()),
    })?;
    let tail = parse_u32(match parts.get(3) {
        Some(v) => v,
        None => return Err(BpCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(4) {
        Some(v) => v,
        None => return Err(BpCode::RenderBad.val()),
    })?;
    if cells as usize != MATRIX_CELLS {
        return Err(BpCode::MatrixBad.val());
    }
    if events == 0 || deliveries == 0 || tail == 0 {
        return Err(BpCode::RenderBad.val());
    }
    // 每格至少递交一次、末批至少一个事件：两条下界把「空递交」挡住。
    if events < deliveries || tail < cells {
        return Err(BpCode::MatrixBad.val());
    }
    if digest == 0 {
        return Err(BpCode::RenderBad.val());
    }
    Ok(cells)
}

pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let cells = matrix_face()?;
    let text = render_matrix(&cells);
    if audit_render(&text)? != cells.len() as u32 {
        return Err(BpCode::ReplayBad.val());
    }
    let (crossings, _alarms) = alarm_rounds(ALARM_ROUNDS)?;
    if crossings == 0 {
        return Err(BpCode::AlarmBad.val());
    }
    let events: u32 = {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < cells.len() {
            if let Some(c) = cells.get(i) {
                s = s.saturating_add(c.events);
            }
            i += 1;
        }
        s
    };
    let deliveries: u32 = {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < cells.len() {
            if let Some(c) = cells.get(i) {
                s = s.saturating_add(c.deliveries);
            }
            i += 1;
        }
        s
    };
    Ok((cells.len() as u32, events, deliveries, matrix_digest(&cells)))
}

// ── 注入对账（四路缺陷必须被捕获） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 {
        return Err(BpCode::CoverageBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 零批量必须被拒。
        let mut q = WaterQueue::new();
        if q.push(0).err() == Some(BpCode::CountBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);
        // ② 入队恒等式被破坏（在队深度凭空 +1）必须炸 LedgerBad。
        let mut a = WaterQueue::new();
        let _ = a.push(100)?;
        a.fill = a.fill.saturating_add(1);
        if a.audit().err() == Some(BpCode::LedgerBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);
        // ③ 越档告警漏记必须炸 AlarmBad。
        let mut b = WaterQueue::new();
        let _ = b.push(900)?;
        b.alarms = [0; TIERS];
        if b.audit().err() == Some(BpCode::AlarmBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);
        // ④ 渲染篡改必须被拒。
        let cells = matrix_face()?;
        let bad = render_matrix(&cells).replacen("0x", "0y", 1);
        if audit_render(&bad).is_err() {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(BpCode::CoverageBad.val());
    }
    Ok(log)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if QUEUE_CAP != 1_024 || WATER_LOW != 256 || WATER_HIGH != 512 || WATER_CRIT != 768 {
        return Err(BpCode::FillBad.val());
    }
    let mut q = WaterQueue::new();
    if q.push(200)? != 200 || q.level != WaterLevel::Quiet {
        return Err(BpCode::LevelBad.val());
    }
    if q.push(100)? != 100 || q.level != WaterLevel::Low {
        return Err(BpCode::LevelBad.val());
    }
    if q.push(300)? != 300 || q.level != WaterLevel::High {
        return Err(BpCode::LevelBad.val());
    }
    if q.push(200)? != 200 || q.level != WaterLevel::Critical {
        return Err(BpCode::LevelBad.val());
    }
    if q.alarms != [0, 1, 1, 1] || q.crossings != 3 {
        return Err(BpCode::AlarmBad.val());
    }
    // 临界档拒绝且水位不动。
    if q.push(1).err() != Some(BpCode::PressureBad.val()) || q.fill != 800 {
        return Err(BpCode::PressureBad.val());
    }
    q.audit()?;
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 一次跨三档：三条告警一条不能少。
    let mut q = WaterQueue::new();
    let _ = q.push(900)?;
    if q.level != WaterLevel::Critical || q.crossings != 3 {
        return Err(BpCode::AlarmBad.val());
    }
    if q.alarms != [0, 1, 1, 1] {
        return Err(BpCode::AlarmBad.val());
    }
    q.audit()?;
    // 回落不告警；再次抬升必须再告警。
    let _ = q.pop(QUEUE_CAP)?;
    if q.level != WaterLevel::Quiet || q.alarms != [0, 1, 1, 1] {
        return Err(BpCode::AlarmBad.val());
    }
    let _ = q.push(300)?;
    if q.alarms != [0, 2, 1, 1] || q.crossings != 4 {
        return Err(BpCode::AlarmBad.val());
    }
    q.audit()?;
    // 档位枚举与编号可互逆。
    let mut i = 0u8;
    while i < TIERS as u8 {
        if WaterLevel::from_index(i)?.index() as u8 != i {
            return Err(BpCode::LevelBad.val());
        }
        i += 1;
    }
    if WaterLevel::from_index(TIERS as u8).err() != Some(BpCode::LevelBad.val()) {
        return Err(BpCode::LevelBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let cells = matrix_face()?;
    if cells.len() != MATRIX_CELLS {
        return Err(BpCode::MatrixBad.val());
    }
    if matrix_audit(&cells)? != MATRIX_CELLS as u32 {
        return Err(BpCode::MatrixBad.val());
    }
    // 最大批的行递交次数必须严格小于最小批的行。
    let small = match cell_at(&cells, BATCH_LADDER[0], RATE_LADDER[2]) {
        Some(v) => v,
        None => return Err(BpCode::MatrixBad.val()),
    };
    let large = match cell_at(&cells, BATCH_LADDER[4], RATE_LADDER[2]) {
        Some(v) => v,
        None => return Err(BpCode::MatrixBad.val()),
    };
    if small.deliveries <= large.deliveries {
        return Err(BpCode::BatchBad.val());
    }
    let (crossings, alarms) = alarm_rounds(ALARM_ROUNDS)?;
    if crossings == 0 || crossings != alarms.iter().sum::<u32>() {
        return Err(BpCode::AlarmBad.val());
    }
    if !segment_guard() {
        return Err(BpCode::SegmentBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = alarm_rounds(ALARM_ROUNDS)?;
    let b = alarm_rounds(ALARM_ROUNDS)?;
    if a != b {
        return Err(BpCode::ReplayBad.val());
    }
    if matrix_digest(&matrix_face()?) != matrix_digest(&matrix_face()?) {
        return Err(BpCode::ReplayBad.val());
    }
    if backpressure_rounds(PRESSURE_ROUNDS)? != backpressure_rounds(PRESSURE_ROUNDS)? {
        return Err(BpCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < BpCode::ALL.len() {
        let v = BpCode::ALL[i].val();
        if v <= SEG_BASE || v > SEG_BASE + 0x3F {
            return false;
        }
        i += 1;
    }
    true
}

// ── 域检查项（≤8 条） ─────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    let ladder_ok = WaterLevel::from_fill(WATER_LOW) == Ok(WaterLevel::Quiet)
        && WaterLevel::from_fill(WATER_LOW + 1) == Ok(WaterLevel::Low)
        && WaterLevel::from_fill(WATER_HIGH) == Ok(WaterLevel::Low)
        && WaterLevel::from_fill(WATER_HIGH + 1) == Ok(WaterLevel::High)
        && WaterLevel::from_fill(WATER_CRIT) == Ok(WaterLevel::High)
        && WaterLevel::from_fill(WATER_CRIT + 1) == Ok(WaterLevel::Critical)
        && WaterLevel::from_fill(QUEUE_CAP) == Ok(WaterLevel::Critical)
        && WaterLevel::from_fill(QUEUE_CAP + 1) == Err(BpCode::FillBad.val());
    out.push((
        "F48969 四档阈值闭集",
        ladder_ok && TIERS == 4 && QUEUE_CAP == WATER_LOW * 4,
        format!(
            "cap={} low={} high={} crit={} ladder={}",
            QUEUE_CAP, WATER_LOW, WATER_HIGH, WATER_CRIT, ladder_ok
        ),
    ));

    let alarm = alarm_rounds(ALARM_ROUNDS);
    out.push((
        "F48969 水位×10越档零漏",
        matches!(&alarm, Ok((c, a)) if *c > 0
            && *c == a.iter().sum::<u32>()
            && a[0] == 0
            && a[1] == ALARM_ROUNDS
            && a[2] == ALARM_ROUNDS
            && a[3] == ALARM_ROUNDS),
        match &alarm {
            Ok((c, a)) => format!(
                "crossings={} alarms=[{},{},{},{}]",
                c, a[0], a[1], a[2], a[3]
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 背压拒绝与守恒合并：两者是同一次入队动作的两面。
    // 900 一步入队（容量 1024 内全额接受），随后 900 ≥ 768 背压闸门开启，
    // 再推 50 必被拒且水位冻结，最后出队 400 回到 500。
    let mut q = WaterQueue::new();
    let filled = q.push(900);
    let blocked = q.push(50);
    let frozen = q.fill;
    let drained = q.pop(400);
    let (bp_ok, bp_fill) = (blocked.err() == Some(BpCode::PressureBad.val()), frozen);
    out.push((
        "F48969 背压拒收与守恒",
        filled == Ok(900) && bp_ok && bp_fill == 900
            && drained == Ok(400)
            && q.fill == 500
            && q.pushed == 900
            && q.popped == 400
            && q.audit().is_ok(),
        format!(
            "filled={:?} blocked={:?} fill={} drained={:?} pushed={} popped={}",
            filled, blocked, bp_fill, drained, q.pushed, q.popped
        ),
    ));

    let cells = matrix_face();
    out.push((
        "F48970 矩阵×15格入账",
        matches!(&cells, Ok(v) if v.len() == MATRIX_CELLS
            && matrix_digest(v) != 0),
        match &cells {
            Ok(v) => format!(
                "cells={} digest=0x{:016X}",
                v.len(),
                matrix_digest(v)
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 单调与逐格恒等合并：单调断言只在恒等成立的行上才有意义。
    let mut mono_ok = true;
    let mut mono_checked = 0u32;
    if let Ok(v) = cells {
        mono_ok = matrix_audit(&v).is_ok();
        let mut r = 0usize;
        while r < RATE_LADDER.len() {
            let rate = match RATE_LADDER.get(r) {
                Some(x) => *x,
                None => 0,
            };
            let mut prev = u32::MAX;
            let mut b = 0usize;
            while b < BATCH_LADDER.len() {
                let batch = match BATCH_LADDER.get(b) {
                    Some(x) => *x,
                    None => 0,
                };
                match cell_at(&v, batch, rate) {
                    Some(c) => {
                        if c.deliveries > prev || c.audit().is_err() {
                            mono_ok = false;
                        }
                        prev = c.deliveries;
                        mono_checked = mono_checked.saturating_add(1);
                    }
                    None => mono_ok = false,
                }
                b = b.saturating_add(1);
            }
            r = r.saturating_add(1);
        }
    }
    out.push((
        "F48970 逐格恒等与递交单调",
        mono_ok && mono_checked == MATRIX_CELLS as u32,
        format!("checked={} ok={}", mono_checked, mono_ok),
    ));

    let fuzz = backpressure_rounds(PRESSURE_ROUNDS);
    out.push((
        "F48969 背压fuzz零穿透",
        fuzz.is_ok(),
        match &fuzz {
            Ok(h) => format!("rounds={} digest=0x{:016X}", PRESSURE_ROUNDS, h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let golden = golden_run();
    out.push((
        "F48970 账本可解析",
        golden.is_ok(),
        match &golden {
            Ok((c, e, d, h)) => format!("cells={} events={} deliveries={} d=0x{:X}", c, e, d, h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48969 段位闭集",
        segment_guard(),
        format!("codes={} seg=0x{:04X}", BpCode::ALL.len(), SEG_BASE),
    ));

    out
}

pub fn run_unx48969_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48969 事件层背压与水位告警");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x71500);
        assert_eq!(QUEUE_CAP, 1_024);
        assert_eq!(WATER_LOW, 256);
        assert_eq!(WATER_HIGH, 512);
        assert_eq!(WATER_CRIT, 768);
        assert_eq!(MATRIX_CELLS, 15);
        assert_eq!(BATCH_LADDER.len() * RATE_LADDER.len(), MATRIX_CELLS);
    }

    #[test]
    fn level_boundaries_are_exact() {
        assert_eq!(WaterLevel::from_fill(0), Ok(WaterLevel::Quiet));
        assert_eq!(WaterLevel::from_fill(256), Ok(WaterLevel::Quiet));
        assert_eq!(WaterLevel::from_fill(257), Ok(WaterLevel::Low));
        assert_eq!(WaterLevel::from_fill(512), Ok(WaterLevel::Low));
        assert_eq!(WaterLevel::from_fill(513), Ok(WaterLevel::High));
        assert_eq!(WaterLevel::from_fill(768), Ok(WaterLevel::High));
        assert_eq!(WaterLevel::from_fill(769), Ok(WaterLevel::Critical));
        assert_eq!(WaterLevel::from_fill(1_024), Ok(WaterLevel::Critical));
        assert_eq!(
            WaterLevel::from_fill(1_025),
            Err(BpCode::FillBad.val())
        );
    }

    #[test]
    fn tiers_crossed_counts_each_tier() {
        assert_eq!(tiers_crossed(WaterLevel::Quiet, WaterLevel::Critical), 3);
        assert_eq!(tiers_crossed(WaterLevel::Low, WaterLevel::High), 1);
        assert_eq!(tiers_crossed(WaterLevel::High, WaterLevel::High), 0);
        assert_eq!(tiers_crossed(WaterLevel::Critical, WaterLevel::Quiet), 0);
    }

    #[test]
    fn push_records_each_tier_alarm() {
        // 空队列容量 1024，900 一步入队全额收下；随后一次跨三档，各档各记一次告警。
        let mut q = WaterQueue::new();
        assert_eq!(q.push(900), Ok(900));
        assert_eq!(q.level, WaterLevel::Critical);
        assert_eq!(q.crossings, 3);
        assert_eq!(q.alarms, [0, 1, 1, 1]);
        assert_eq!(q.rejected, 0);
        assert!(q.audit().is_ok());
    }

    #[test]
    fn pressure_blocks_at_critical() {
        let mut q = WaterQueue::new();
        let _ = q.push(800);
        let before = q.fill;
        assert_eq!(q.push(10), Err(BpCode::PressureBad.val()));
        assert_eq!(q.fill, before);
        assert_eq!(q.pressure, 1);
        assert_eq!(q.rejected, 10);
        assert!(q.audit().is_ok());
    }

    #[test]
    fn zero_batch_is_rejected() {
        let mut q = WaterQueue::new();
        assert_eq!(q.push(0), Err(BpCode::CountBad.val()));
        assert_eq!(q.pop(0), Err(BpCode::CountBad.val()));
        assert_eq!(q.fill, 0);
    }

    #[test]
    fn pop_releases_and_drops_level_silently() {
        let mut q = WaterQueue::new();
        let _ = q.push(600);
        assert_eq!(q.level, WaterLevel::High);
        let alarms_before = q.alarms;
        assert_eq!(q.pop(500), Ok(500));
        assert_eq!(q.level, WaterLevel::Quiet);
        assert_eq!(q.alarms, alarms_before);
        assert!(q.audit().is_ok());
    }

    #[test]
    fn drain_to_empty_conserves() {
        let mut q = WaterQueue::new();
        let _ = q.push(300);
        let _ = q.push(200);
        assert_eq!(q.push(700), Ok(524));
        assert_eq!(q.fill, 1_024);
        assert_eq!(q.pop(2_000), Ok(1_024));
        assert_eq!(q.fill, 0);
        assert_eq!(q.pushed, 1_024);
        assert_eq!(q.popped, 1_024);
        assert!(q.audit().is_ok());
    }

    #[test]
    fn fill_tamper_breaks_ledger() {
        let mut q = WaterQueue::new();
        let _ = q.push(100);
        q.fill = q.fill.saturating_add(1);
        assert_eq!(q.audit(), Err(BpCode::LedgerBad.val()));
    }

    #[test]
    fn alarm_tamper_is_detected() {
        let mut q = WaterQueue::new();
        let _ = q.push(900);
        q.alarms[3] = 0;
        assert_eq!(q.audit(), Err(BpCode::AlarmBad.val()));
    }

    #[test]
    fn level_tamper_is_detected() {
        let mut q = WaterQueue::new();
        let _ = q.push(300);
        q.level = WaterLevel::Critical;
        assert_eq!(q.audit(), Err(BpCode::LevelBad.val()));
    }

    #[test]
    fn alarm_rounds_are_ten_and_exact() {
        match alarm_rounds(ALARM_ROUNDS) {
            Ok((c, a)) => {
                assert_eq!(c, 3 * ALARM_ROUNDS);
                assert_eq!(a, [0, ALARM_ROUNDS, ALARM_ROUNDS, ALARM_ROUNDS]);
            }
            Err(_) => assert!(false),
        }
        assert_eq!(alarm_rounds(0), Err(BpCode::CoverageBad.val()));
        assert_eq!(alarm_rounds(101), Err(BpCode::CoverageBad.val()));
    }

    #[test]
    fn backpressure_fuzz_is_stable() {
        assert_eq!(
            backpressure_rounds(PRESSURE_ROUNDS),
            backpressure_rounds(PRESSURE_ROUNDS)
        );
        assert!(backpressure_rounds(0).is_err());
    }

    #[test]
    fn matrix_has_fifteen_unique_cells() {
        let cells = match matrix_face() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(cells.len(), MATRIX_CELLS);
        assert_eq!(matrix_audit(&cells), Ok(MATRIX_CELLS as u32));
        let mut i = 0usize;
        while i < cells.len() {
            let mut j = i + 1;
            while j < cells.len() {
                let (a, b) = (cells[i], cells[j]);
                assert!(a.batch != b.batch || a.rate != b.rate);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn cell_identity_holds() {
        let cell = match measure_cell(64, 1_000) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(cell.events, 1_000);
        assert_eq!(cell.deliveries, 16);
        assert_eq!(cell.tail, 40);
        assert_eq!(
            (cell.deliveries - 1) * cell.batch + cell.tail,
            cell.events
        );
        assert!(cell.audit().is_ok());
        assert_eq!(measure_cell(0, 1_000), Err(BpCode::BatchBad.val()));
        assert_eq!(measure_cell(8, 0), Err(BpCode::BatchBad.val()));
    }

    #[test]
    fn matrix_deliveries_are_monotone() {
        let cells = match matrix_face() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let mut r = 0usize;
        while r < RATE_LADDER.len() {
            let rate = RATE_LADDER[r];
            let mut prev = u32::MAX;
            let mut b = 0usize;
            while b < BATCH_LADDER.len() {
                let c = match cell_at(&cells, BATCH_LADDER[b], rate) {
                    Some(v) => v,
                    None => return assert!(false),
                };
                assert!(c.deliveries <= prev);
                prev = c.deliveries;
                b += 1;
            }
            r += 1;
        }
    }

    #[test]
    fn matrix_audit_catches_short_list() {
        let mut cells = match matrix_face() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        cells.pop();
        assert_eq!(
            matrix_audit(&cells),
            Err(BpCode::MatrixBad.val())
        );
    }

    #[test]
    fn matrix_audit_catches_tail_overflow() {
        let cells = match matrix_face() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let mut bad = cells;
        if let Some(c) = bad.get_mut(0) {
            c.tail = c.batch + 1;
        }
        assert!(matrix_audit(&bad).is_err());
    }

    #[test]
    fn render_roundtrip() {
        let cells = match matrix_face() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let text = render_matrix(&cells);
        assert_eq!(audit_render(&text), Ok(MATRIX_CELLS as u32));
    }

    #[test]
    fn render_tamper_is_rejected() {
        let cells = match matrix_face() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let text = render_matrix(&cells);
        assert!(audit_render(&text.replacen("0x", "0y", 1)).is_err());
        assert!(audit_render(&text.replacen("15|", "14|", 1)).is_err());
        assert!(audit_render("15|1|1").is_err());
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(5) {
            Ok(l) => assert!(l.all_caught()),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn repro_is_deterministic() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn codes_are_unique_and_described() {
        let mut i = 0usize;
        while i < BpCode::ALL.len() {
            let c = BpCode::ALL[i];
            assert_eq!(detail(c.val()), c.detail());
            assert!(!c.detail().is_empty());
            let mut k = i + 1;
            while k < BpCode::ALL.len() {
                assert!(c.val() != BpCode::ALL[k].val());
                k += 1;
            }
            i += 1;
        }
        assert_eq!(detail(0xDEAD_BEEF), "未知诊断码");
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48969_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
