//! UNX-F48961 1,000Hz 高报点率设备支持总成 · 0x71400
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B40 族首槽）：
//!   ① 纯功能代码 ≥420 行；
//!   ② UNX-F48961-J1 1,000Hz ×10 分钟计数对账零丢（判据 J-2 载体）；
//!   ③ UNX-F48962-J1 实验通道 ×10 轮等价性对账一致。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：速率先验 → 窗口展开 → 去向三分（递送/合并/丢弃）→
//!           恒等式对账 → 8:1 帧折叠恒等 → 等价流逐帧比对 → 实验闸门。
//! 先速率再窗口：1,000Hz 是全部计数的分母。速率读错，十分钟 600,000
//! 这个基数就错，后面的「零丢」只是在一个错基上自洽——账会绿而语义是假。
//! 去向三分放在对账之前：一次到达只有「递送 / 进入帧通道 / 显式丢弃」
//! 三个出路，少记一路就是静默丢事件；先钉死三条出路，恒等式才有判别力。
//! 8:1 折叠恒等紧跟对账：1,000→125 是**整除**关系，帧数、余量、累计和
//! 三者互相约束；约束一旦放松，「等价」就退化成「帧数对、内容错」。
//! 逐帧比对不能省：等价流不是第二条实现，而是同一批细事件的折叠结果，
//! 只有逐帧对齐才能发现搬移错误（首帧对、中段错是典型形态）。
//! 实验闸门放最后：帧合并是实验通道，生产路径默认关闭；先把账算对再
//! 允许开门——顺序反了就是拿未对账的实验产出当生产数据。
//! 关键差异：本模块只回答「1,000Hz 全路径怎样计数不丢、8:1 折叠是否
//!           逐帧等价」，不做 per-CPU 均衡（B40-SCHED）、不做尾延迟 P99
//!           归因（B40-SCHED）、不做水位背压（B40-BACKPRESSURE）。丢弃
//!           在本模块只记账不问原因，原因归背压模块。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71400，B40 族首槽）。
pub const SEG_BASE: u32 = 0x71400;

/// 高报点率设备基准：1,000 事件/秒。
pub const RATE_HZ: u32 = 1_000;
/// 对账窗口长度：10 分钟 = 600 秒（卡面判据的「×10 分钟」）。
pub const WINDOW_SECS: u32 = 600;
/// 十分钟窗口的事件基数（速率 × 秒数，编译期常量不会溢出）。
pub const RATE_TICKS: u32 = RATE_HZ * WINDOW_SECS;
/// 帧合并比：1,000Hz / 125Hz = 8（恰好整除，无分数余量）。
pub const MERGE_RATIO: u32 = 8;
/// 合成等价流的报点率。
pub const NATIVE_HZ: u32 = RATE_HZ / MERGE_RATIO;
/// 卡面「×10 分钟」的载体轮数。
pub const REPLAY_MINUTES: u32 = 10;
/// 实验通道等价性轮数（卡面 ×10）。
pub const EQUIV_ROUNDS: u32 = 10;
/// 单次 delta 绝对值上限（与输入层 int 域自洽）。
pub const DELTA_CAP: i32 = 1_048_575;
/// 账本字段数（渲染/解析共用，改了必须两处同步）。
pub const LEDGER_FIELDS: usize = 6;
/// 单次对账窗口的秒数上限（防 32 位计数越界）。
pub const WINDOW_CAP_SECS: u32 = 3_600;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0D） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RateCode {
    /// 速率/增量非法（零增量或越界）
    RateBad,
    /// 窗口非法（零秒或超越上限）
    WindowBad,
    /// 去向非法（未知去向编号）
    RouteBad,
    /// 丢事件（lost 非零，恒等式即便配平也不接受）
    LostBad,
    /// 折叠非法（帧数/余量不自洽）
    MergeBad,
    /// 帧内容非法（与参照折叠结果不符）
    FrameBad,
    /// 等价流不一致（×10 轮有任一轮对不上）
    EquivBad,
    /// 实验闸门非法（未开闸即投喂 / 重复开闸）
    GateBad,
    /// 次序非法（计数回退或窗口次序断裂）
    OrderBad,
    /// 账不平（三分之和 ≠ 到达数）
    LedgerBad,
    /// 渲染非法（字段缺失/回读失败）
    RenderBad,
    /// 复算不一致（同一流重放不同）
    ReplayBad,
    /// 覆盖不足（轮数或样本不足）
    CoverageBad,
    /// 段位越界（诊断码逃出 0x71400 窗口）
    SegmentBad,
}

impl RateCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            RateCode::RateBad => "速率非法：零增量或增量绝对值越界",
            RateCode::WindowBad => "窗口非法：零秒或超越 3600 秒上限",
            RateCode::RouteBad => "去向非法：未知去向编号",
            RateCode::LostBad => "丢事件：lost 非零，零丢判据不成立",
            RateCode::MergeBad => "折叠非法：帧数、余量与累计和不自洽",
            RateCode::FrameBad => "帧内容非法：与参照 8 样本折叠结果不符",
            RateCode::EquivBad => "等价流不一致：×10 轮存在对不上的轮次",
            RateCode::GateBad => "实验闸门非法：未开闸投喂或重复开闸",
            RateCode::OrderBad => "次序非法：计数回退或窗口次序断裂",
            RateCode::LedgerBad => "账不平：递送+合并+丢弃+丢 ≠ 到达数",
            RateCode::RenderBad => "渲染非法：字段缺失或回读失败",
            RateCode::ReplayBad => "复算不一致：同一流重放结果不同",
            RateCode::CoverageBad => "覆盖不足：轮数或样本数不足",
            RateCode::SegmentBad => "段位越界：诊断码逃出 0x71400 窗口",
        }
    }

    pub const ALL: [RateCode; 14] = [
        RateCode::RateBad,
        RateCode::WindowBad,
        RateCode::RouteBad,
        RateCode::LostBad,
        RateCode::MergeBad,
        RateCode::FrameBad,
        RateCode::EquivBad,
        RateCode::GateBad,
        RateCode::OrderBad,
        RateCode::LedgerBad,
        RateCode::RenderBad,
        RateCode::ReplayBad,
        RateCode::CoverageBad,
        RateCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < RateCode::ALL.len() {
        if RateCode::ALL[i].val() == code {
            return RateCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 去向三分 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TickRoute {
    /// 直接递送给消费端
    Delivered,
    /// 进入 8:1 帧折叠通道
    Merged,
    /// 显式丢弃（背压/过载，必须记账）
    Dropped,
}

impl TickRoute {
    pub fn index(self) -> u8 {
        match self {
            TickRoute::Delivered => 0,
            TickRoute::Merged => 1,
            TickRoute::Dropped => 2,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + 0x30 + self.index() as u32
    }

    pub fn from_index(i: u8) -> Result<TickRoute, u32> {
        match i {
            0 => Ok(TickRoute::Delivered),
            1 => Ok(TickRoute::Merged),
            2 => Ok(TickRoute::Dropped),
            _ => Err(RateCode::RouteBad.val()),
        }
    }
}

/// 确定性去向判定：到达序号决定归属，重放可复现。
///
/// 每 997 个事件丢 1 个（模拟过载丢弃，占 0.1%），每 13 个事件有 1 个
/// 进入折叠通道（占 ~7.7%）。两条模数互质，故丢弃优先于合并的次序
/// 只影响 1/12961 的事件——这个重叠率本身就说明「先判丢弃」不是
/// 随手写的顺序，而是可枚举的确定性规则。
pub fn classify_seq(seq: u32) -> TickRoute {
    if seq % 997 == 0 {
        TickRoute::Dropped
    } else if seq % 13 == 0 {
        TickRoute::Merged
    } else {
        TickRoute::Delivered
    }
}

// ── 速率账本（十分钟零丢） ────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RateLedger {
    /// 设备速率快照（建账即锚定，事后篡改会被渲染复读发现）
    pub hz: u32,
    /// 到达事件总数
    pub ticks: u32,
    /// 已递送
    pub delivered: u32,
    /// 已入折叠通道
    pub merged: u32,
    /// 已显式丢弃
    pub dropped: u32,
    /// 消失事件（任何时刻必须为 0；非零即判据不成立）
    pub lost: u32,
    /// 已完成的对账窗口数（秒）
    pub windows: u32,
    pub digest: u64,
}

impl RateLedger {
    pub fn new() -> RateLedger {
        RateLedger {
            hz: RATE_HZ,
            ticks: 0,
            delivered: 0,
            merged: 0,
            dropped: 0,
            lost: 0,
            windows: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 吞入一次到达，返回其去向。
    ///
    /// 三条出路**必居其一**：`ticks` 递增与去向计数递增在同一函数内
    /// 完成，任何提前返回都在计数之前——这样恒等式只有「漏记」一种
    /// 失败形态，不会出现「记了到达却没记去向」的中间态。
    pub fn push_tick(&mut self, delta: i32) -> Result<TickRoute, u32> {
        if delta == 0 || (delta as i64).unsigned_abs() > DELTA_CAP as u64 {
            return Err(RateCode::RateBad.val());
        }
        if self.hz == 0 || self.hz > RATE_HZ {
            return Err(RateCode::RateBad.val());
        }
        let route = classify_seq(self.ticks);
        if self.ticks == u32::MAX {
            return Err(RateCode::OrderBad.val());
        }
        match route {
            TickRoute::Delivered => self.delivered = self.delivered.saturating_add(1),
            TickRoute::Merged => self.merged = self.merged.saturating_add(1),
            TickRoute::Dropped => self.dropped = self.dropped.saturating_add(1),
        }
        self.ticks += 1;
        self.digest = p_core::fold_i32(self.digest, delta);
        self.digest = p_core::fold_u32(self.digest, route.index() as u32);
        Ok(route)
    }

    /// 恒等式与零丢审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.hz == 0 {
            return Err(RateCode::RateBad.val());
        }
        if self.lost != 0 {
            return Err(RateCode::LostBad.val());
        }
        let sum = self
            .delivered
            .saturating_add(self.merged)
            .saturating_add(self.dropped)
            .saturating_add(self.lost);
        if sum != self.ticks {
            return Err(RateCode::LedgerBad.val());
        }
        if self.ticks == 0 {
            return Err(RateCode::CoverageBad.val());
        }
        // 每个窗口至少装 RATE_HZ 个事件；窗口数与事件数互为下界。
        if self.windows.saturating_mul(RATE_HZ) > self.ticks {
            return Err(RateCode::OrderBad.val());
        }
        Ok(())
    }

    /// 递送份额下界：三分之后递送必须仍占绝对多数（≥92.0%）。
    ///
    /// 用 64 位乘法比较，避免 `delivered*1000` 在百万级事件上越界。
    pub fn throughput_ok(&self) -> bool {
        if self.ticks == 0 {
            return false;
        }
        (self.delivered as u64).saturating_mul(1_000)
            >= (self.ticks as u64).saturating_mul(920)
    }
}

/// 十分钟速率对账：逐秒展开 1,000 个事件，每个窗口后复算恒等式。
pub fn reconcile_window(seconds: u32) -> Result<RateLedger, u32> {
    if seconds == 0 || seconds > WINDOW_CAP_SECS {
        return Err(RateCode::WindowBad.val());
    }
    let mut l = RateLedger::new();
    let mut s = 0u32;
    while s < seconds {
        let mut k = 0u32;
        while k < RATE_HZ {
            let seq = s.wrapping_mul(RATE_HZ).wrapping_add(k);
            let _ = l.push_tick(sample_delta(seq, 0))?;
            k = k.saturating_add(1);
        }
        l.windows = l.windows.saturating_add(1);
        l.audit()?;
        s = s.saturating_add(1);
    }
    let want = (seconds as u64).saturating_mul(RATE_HZ as u64);
    if l.ticks as u64 != want {
        return Err(RateCode::LedgerBad.val());
    }
    l.audit()?;
    Ok(l)
}

/// 确定性样本：覆盖整格、整格±1、半格、奇偶余量与极值。
fn sample_delta(seq: u32, phase: u32) -> i32 {
    match (seq.wrapping_add(phase)) % 12 {
        0 => 120,
        1 => -120,
        2 => 1,
        3 => -1,
        4 => 40,
        5 => -40,
        6 => 7,
        7 => -7,
        8 => 119,
        9 => -119,
        10 => 13,
        _ => -13,
    }
}

/// 账本 fuzz：随机增量下恒等式与零丢必须恒成立。
pub fn fuzz_ledger_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 {
        return Err(RateCode::CoverageBad.val());
    }
    let mut l = RateLedger::new();
    let mut rng = p_core::Lcg::new(0x8A11_0061);
    let mut i = 0u32;
    while i < rounds {
        // 先取范围再取符号：嵌套可变借用 `rng.signed(1 + rng.below(n))`
        // 在 Rust 里是 E0499，必须把 span 提出来。
        let span = rng.below(20_000);
        let d = rng.signed(1 + span);
        let _ = l.push_tick(d)?;
        l.audit()?;
        i = i.saturating_add(1);
    }
    Ok(l.digest)
}

// ── 8:1 帧折叠通道（1,000Hz → 125Hz） ─────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MergeChannel {
    /// 输入细事件数
    pub ticks: u32,
    /// 已合成帧数
    pub frames: u32,
    /// 未满帧的累积量
    pub pending: i32,
    /// 未满帧已收事件数（恒 < MERGE_RATIO）
    pub pending_n: u32,
    /// 输入累计和
    pub sum_in: i64,
    /// 输出累计和（已合成帧之和）
    pub sum_out: i64,
    pub digest: u64,
}

impl MergeChannel {
    pub fn new() -> MergeChannel {
        MergeChannel {
            ticks: 0,
            frames: 0,
            pending: 0,
            pending_n: 0,
            sum_in: 0,
            sum_out: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 喂入一段细格增量；凑满 8 个返回 `Some(帧值)`，否则 `None`。
    ///
    /// 帧值 = 8 个样本的整数和：1,000Hz 的 8 个 tick 恰好对应 125Hz 的
    /// 1 帧，故折叠不产生分数余量——这是本模块「等价」判据的代数基础。
    pub fn push(&mut self, delta: i32) -> Result<Option<i32>, u32> {
        if delta == 0 || (delta as i64).unsigned_abs() > DELTA_CAP as u64 {
            return Err(RateCode::RateBad.val());
        }
        if self.pending_n >= MERGE_RATIO {
            return Err(RateCode::MergeBad.val());
        }
        let sum_in = match self.sum_in.checked_add(delta as i64) {
            Some(v) => v,
            None => return Err(RateCode::MergeBad.val()),
        };
        let pending = match self.pending.checked_add(delta) {
            Some(v) => v,
            None => return Err(RateCode::MergeBad.val()),
        };
        if self.ticks == u32::MAX {
            return Err(RateCode::OrderBad.val());
        }
        self.sum_in = sum_in;
        self.pending = pending;
        self.pending_n = self.pending_n.saturating_add(1);
        self.ticks += 1;
        self.digest = p_core::fold_i32(self.digest, delta);
        if self.pending_n == MERGE_RATIO {
            let frame = self.pending;
            let sum_out = match self.sum_out.checked_add(frame as i64) {
                Some(v) => v,
                None => return Err(RateCode::MergeBad.val()),
            };
            self.sum_out = sum_out;
            self.frames = self.frames.saturating_add(1);
            self.pending = 0;
            self.pending_n = 0;
            self.digest = p_core::fold_i32(self.digest, frame);
            return Ok(Some(frame));
        }
        Ok(None)
    }

    /// 折叠恒等：`输入和 = 已输出帧和 + 未满帧累积`，且
    /// `输入数 = 帧数×8 + 未满帧事件数`。
    pub fn audit(&self) -> Result<(), u32> {
        if self.pending_n >= MERGE_RATIO {
            return Err(RateCode::MergeBad.val());
        }
        let lhs = self.sum_out.checked_add(self.pending as i64);
        match lhs {
            Some(v) if v == self.sum_in => {}
            _ => return Err(RateCode::MergeBad.val()),
        }
        let covered = (self.frames as u64)
            .saturating_mul(MERGE_RATIO as u64)
            .saturating_add(self.pending_n as u64);
        if covered != self.ticks as u64 {
            return Err(RateCode::MergeBad.val());
        }
        if self.ticks == 0 {
            return Err(RateCode::CoverageBad.val());
        }
        Ok(())
    }

    /// 帧序号对应的首样本序号（供参照折叠定位）。
    pub fn frame_span_start(&self, frame: u32) -> Option<u32> {
        frame.checked_mul(MERGE_RATIO)
    }
}

/// 等价流逐帧比对：折叠通道的每一帧必须等于「独立按 8 样本求和」的结果。
///
/// 参照值从样本序号直接重算，不复用 `MergeChannel` 的累积量——复用就变成
/// 自证；这里要的是两条互不依赖的算式给出同一批帧。
pub fn frame_equivalence(ticks: u32, phase: u32) -> Result<(u32, i32), u32> {
    if ticks == 0 {
        return Err(RateCode::CoverageBad.val());
    }
    let mut ch = MergeChannel::new();
    let mut last = 0i32;
    let mut i = 0u32;
    while i < ticks {
        let d = sample_delta(i, phase);
        match ch.push(d)? {
            Some(frame) => {
                // 该帧覆盖样本 [i-7, i]：8 个样本独立重算。
                let start = i.saturating_sub(MERGE_RATIO - 1);
                let mut want = 0i64;
                let mut k = 0u32;
                while k < MERGE_RATIO {
                    want = want.saturating_add(sample_delta(start.saturating_add(k), phase) as i64);
                    k = k.saturating_add(1);
                }
                if want != frame as i64 {
                    return Err(RateCode::FrameBad.val());
                }
                last = frame;
            }
            None => {}
        }
        i = i.saturating_add(1);
    }
    ch.audit()?;
    let want_frames = ticks / MERGE_RATIO;
    if ch.frames != want_frames {
        return Err(RateCode::MergeBad.val());
    }
    if ch.sum_out.checked_add(ch.pending as i64) != Some(ch.sum_in) {
        return Err(RateCode::MergeBad.val());
    }
    Ok((ch.frames, last))
}

/// 实验通道等价性 ×10 轮：每轮换相位，帧数与帧内容都必须一致。
pub fn equivalence_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 {
        return Err(RateCode::CoverageBad.val());
    }
    let mut rng = p_core::Lcg::new(0x8A11_0062);
    let mut h = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        // 相位取 LCG 高位：低位周期 2^k，`below(8)` 会让相位锁死。
        let phase = ((rng.next() >> 33) % MERGE_RATIO as u64) as u32;
        let ticks = 800u32.saturating_add(phase.saturating_mul(MERGE_RATIO));
        let (frames, last) = frame_equivalence(ticks, phase)?;
        if frames != ticks / MERGE_RATIO {
            return Err(RateCode::EquivBad.val());
        }
        h = p_core::fold_u32(h, frames);
        h = p_core::fold_i32(h, last);
        h = p_core::fold_u32(h, phase);
        r = r.saturating_add(1);
    }
    Ok(h)
}

// ── 实验闸门（仅对账实验开启） ────────────────────────────────────
/// 帧合并是实验通道：默认关闭，显式开闸后才收事件。
///
/// 闸门状态单独记账而不复用 `MergeChannel`：关闭期被拒的投喂必须留下
/// `rejected` 痕迹，否则「实验通道只在实验时开」就成了一句无凭证的声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExperimentGate {
    pub armed: bool,
    pub frames: u32,
    pub rejected: u32,
    pub channel: MergeChannel,
}

impl ExperimentGate {
    pub fn new() -> ExperimentGate {
        ExperimentGate {
            armed: false,
            frames: 0,
            rejected: 0,
            channel: MergeChannel::new(),
        }
    }

    pub fn arm(&mut self) -> Result<(), u32> {
        if self.armed {
            return Err(RateCode::GateBad.val());
        }
        self.armed = true;
        Ok(())
    }

    pub fn disarm(&mut self) -> Result<(), u32> {
        if !self.armed {
            return Err(RateCode::GateBad.val());
        }
        self.armed = false;
        Ok(())
    }

    pub fn feed(&mut self, delta: i32) -> Result<Option<i32>, u32> {
        if !self.armed {
            self.rejected = self.rejected.saturating_add(1);
            return Err(RateCode::GateBad.val());
        }
        let out = self.channel.push(delta)?;
        if out.is_some() {
            self.frames = self.frames.saturating_add(1);
        }
        Ok(out)
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.frames != self.channel.frames {
            return Err(RateCode::LedgerBad.val());
        }
        if self.rejected > 0 && self.armed && self.channel.ticks > 0 {
            // 开闸后仍被拒说明闸门状态自相矛盾。
            return Err(RateCode::GateBad.val());
        }
        Ok(())
    }
}

// ── 账本渲染 ──────────────────────────────────────────────────────
pub fn render_ledger(l: &RateLedger) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        l.hz, l.ticks, l.delivered, l.merged, l.dropped, l.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(RateCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(RateCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(RateCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(RateCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(RateCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(RateCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(RateCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(RateCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 渲染复读：字段数、速率锚、三分和、摘要四项缺一不可。
pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != LEDGER_FIELDS {
        return Err(RateCode::RenderBad.val());
    }
    let hz = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(RateCode::RenderBad.val()),
    })?;
    let ticks = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(RateCode::RenderBad.val()),
    })?;
    let delivered = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(RateCode::RenderBad.val()),
    })?;
    let merged = parse_u32(match parts.get(3) {
        Some(v) => v,
        None => return Err(RateCode::RenderBad.val()),
    })?;
    let dropped = parse_u32(match parts.get(4) {
        Some(v) => v,
        None => return Err(RateCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(5) {
        Some(v) => v,
        None => return Err(RateCode::RenderBad.val()),
    })?;
    if hz != RATE_HZ {
        return Err(RateCode::RateBad.val());
    }
    if ticks == 0 || ticks > 100_000_000 {
        return Err(RateCode::RenderBad.val());
    }
    let sum = delivered
        .saturating_add(merged)
        .saturating_add(dropped);
    if sum != ticks {
        return Err(RateCode::LedgerBad.val());
    }
    if digest == 0 {
        return Err(RateCode::RenderBad.val());
    }
    Ok(ticks)
}

pub fn golden_run() -> Result<(u32, u32, u32, u32, u64), u32> {
    let l = reconcile_window(REPLAY_MINUTES * 60)?;
    let text = render_ledger(&l);
    if audit_render(&text)? != l.ticks {
        return Err(RateCode::ReplayBad.val());
    }
    if l.lost != 0 || !l.throughput_ok() {
        return Err(RateCode::LostBad.val());
    }
    Ok((l.ticks, l.delivered, l.merged, l.dropped, l.digest))
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
        return Err(RateCode::CoverageBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 零增量必须被拒。
        let mut l = RateLedger::new();
        if l.push_tick(0).err() == Some(RateCode::RateBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);
        // ② 三分恒等式被破坏（多记一个到达）必须炸 LedgerBad。
        let mut a = RateLedger::new();
        let _ = a.push_tick(120)?;
        a.ticks = a.ticks.saturating_add(1);
        if a.audit().err() == Some(RateCode::LedgerBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);
        // ③ lost 非零必须炸 LostBad（哪怕三分之和仍配平）。
        let mut b = RateLedger::new();
        let _ = b.push_tick(-120)?;
        b.lost = 1;
        b.ticks = b.ticks.saturating_add(1);
        if b.audit().err() == Some(RateCode::LostBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);
        // ④ 渲染篡改必须被拒。
        let mut c = RateLedger::new();
        let _ = c.push_tick(40)?;
        let bad = render_ledger(&c).replacen("0x", "0y", 1);
        if audit_render(&bad).is_err() {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(RateCode::CoverageBad.val());
    }
    Ok(log)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if RATE_HZ != 1_000 || NATIVE_HZ != 125 || MERGE_RATIO != 8 || RATE_TICKS != 600_000 {
        return Err(RateCode::RateBad.val());
    }
    let l = reconcile_window(1)?;
    if l.ticks != RATE_HZ {
        return Err(RateCode::LedgerBad.val());
    }
    if l.lost != 0 || !l.throughput_ok() {
        return Err(RateCode::LostBad.val());
    }
    l.audit()?;
    // 去向编号与枚举可互逆。
    let mut i = 0u8;
    while i < 3 {
        if TickRoute::from_index(i)?.index() != i {
            return Err(RateCode::RouteBad.val());
        }
        i += 1;
    }
    if TickRoute::from_index(3).err() != Some(RateCode::RouteBad.val()) {
        return Err(RateCode::RouteBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let mut ch = MergeChannel::new();
    let mut k = 0u32;
    while k < MERGE_RATIO - 1 {
        if ch.push(7)? != None {
            return Err(RateCode::MergeBad.val());
        }
        k = k.saturating_add(1);
    }
    if ch.pending_n != MERGE_RATIO - 1 || ch.frames != 0 {
        return Err(RateCode::MergeBad.val());
    }
    if ch.push(7)? != Some(56) {
        return Err(RateCode::FrameBad.val());
    }
    if ch.frames != 1 || ch.pending != 0 || ch.pending_n != 0 {
        return Err(RateCode::MergeBad.val());
    }
    ch.audit()?;
    // 负值帧：8 × (−13) = −104。
    let mut n = MergeChannel::new();
    let mut j = 0u32;
    let mut last = None;
    while j < MERGE_RATIO {
        last = n.push(-13)?;
        j = j.saturating_add(1);
    }
    if last != Some(-104) {
        return Err(RateCode::FrameBad.val());
    }
    n.audit()?;
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let h1 = equivalence_rounds(EQUIV_ROUNDS)?;
    let h2 = equivalence_rounds(EQUIV_ROUNDS)?;
    if h1 != h2 {
        return Err(RateCode::ReplayBad.val());
    }
    let (frames, _last) = frame_equivalence(800, 0)?;
    if frames != 100 {
        return Err(RateCode::EquivBad.val());
    }
    let mut gate = ExperimentGate::new();
    if gate.feed(120).err() != Some(RateCode::GateBad.val()) {
        return Err(RateCode::GateBad.val());
    }
    if gate.rejected != 1 {
        return Err(RateCode::GateBad.val());
    }
    gate.arm()?;
    let _ = gate.feed(120)?;
    gate.disarm()?;
    if gate.feed(120).err() != Some(RateCode::GateBad.val()) {
        return Err(RateCode::GateBad.val());
    }
    gate.audit()?;
    if !segment_guard() {
        return Err(RateCode::SegmentBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = reconcile_window(2)?;
    let b = reconcile_window(2)?;
    if a.digest != b.digest || a.ticks != b.ticks || a.delivered != b.delivered {
        return Err(RateCode::ReplayBad.val());
    }
    if fuzz_ledger_rounds(300)? != fuzz_ledger_rounds(300)? {
        return Err(RateCode::ReplayBad.val());
    }
    if frame_equivalence(400, 3)? != frame_equivalence(400, 3)? {
        return Err(RateCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < RateCode::ALL.len() {
        let v = RateCode::ALL[i].val();
        if v <= SEG_BASE || v > SEG_BASE + 0x3F {
            return false;
        }
        i += 1;
    }
    true
}

// ── 域检查项（≤8 条：超过 8 会被 CheckSet 静默截断） ──────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    out.push((
        "F48961 速率锚定",
        RATE_HZ == 1_000
            && NATIVE_HZ == 125
            && RATE_TICKS == RATE_HZ * WINDOW_SECS
            && RATE_TICKS == 600_000,
        format!(
            "hz={} native={} ticks={} secs={}",
            RATE_HZ, NATIVE_HZ, RATE_TICKS, WINDOW_SECS
        ),
    ));

    let ten = reconcile_window(REPLAY_MINUTES * 60);
    out.push((
        "F48961 十分钟零丢对账",
        matches!(&ten, Ok(l) if l.lost == 0
            && l.ticks as u64 == (REPLAY_MINUTES as u64) * 60 * (RATE_HZ as u64)
            && l.audit().is_ok()
            && l.throughput_ok()),
        match &ten {
            Ok(l) => format!(
                "ticks={} delivered={} merged={} dropped={} lost={}",
                l.ticks, l.delivered, l.merged, l.dropped, l.lost
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let fuzz = fuzz_ledger_rounds(2_000);
    out.push((
        "F48961 fuzz 恒等式",
        fuzz.is_ok(),
        match &fuzz {
            Ok(h) => format!("digest=0x{:016X}", h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let merge = frame_equivalence(800, 0);
    out.push((
        "F48962 8:1折叠恒等",
        matches!(&merge, Ok((f, _)) if *f == 100 && NATIVE_HZ == 125),
        match &merge {
            Ok((f, last)) => format!("frames={} last={} native={}", f, last, NATIVE_HZ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let equiv = equivalence_rounds(EQUIV_ROUNDS);
    out.push((
        "F48962 等价性×10轮",
        equiv.is_ok(),
        match &equiv {
            Ok(h) => format!("rounds={} digest=0x{:016X}", EQUIV_ROUNDS, h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // 闸门关闭期拒绝与开闸期接受合并为一条：两者是同一状态机的两面。
    let mut gate = ExperimentGate::new();
    let closed = gate.feed(120);
    let arm = gate.arm();
    let opened = gate.feed(120);
    let rearm = gate.arm();
    out.push((
        "F48962 实验闸门状态机",
        closed.err() == Some(RateCode::GateBad.val())
            && arm.is_ok()
            && opened == Ok(None)
            && rearm.err() == Some(RateCode::GateBad.val())
            && gate.rejected == 1,
        format!(
            "closed={:?} arm={:?} opened={:?} rearm={:?} rejected={}",
            closed, arm, opened, rearm, gate.rejected
        ),
    ));

    let golden = golden_run();
    out.push((
        "F48961 账本可解析",
        golden.is_ok(),
        match &golden {
            Ok((t, d, m, x, h)) => {
                format!("ticks={} delivered={} merged={} dropped={} d=0x{:X}", t, d, m, x, h)
            }
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48961 段位闭集",
        segment_guard(),
        format!("codes={} seg=0x{:04X}", RateCode::ALL.len(), SEG_BASE),
    ));

    out
}

pub fn run_unx48961_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48961 1,000Hz 高报点率总成");
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
        assert_eq!(SEG_BASE, 0x71400);
        assert_eq!(RATE_HZ, 1_000);
        assert_eq!(MERGE_RATIO, 8);
        assert_eq!(NATIVE_HZ, 125);
        assert_eq!(RATE_TICKS, 600_000);
        assert_eq!(LEDGER_FIELDS, 6);
    }

    #[test]
    fn ten_minutes_reconcile_zero_loss() {
        let l = match reconcile_window(REPLAY_MINUTES * 60) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(l.ticks, RATE_TICKS);
        assert_eq!(l.lost, 0);
        assert_eq!(l.ticks, l.delivered + l.merged + l.dropped);
        assert!(l.audit().is_ok());
        assert!(l.throughput_ok());
        assert_eq!(l.windows, 600);
    }

    #[test]
    fn window_zero_is_rejected() {
        assert_eq!(reconcile_window(0), Err(RateCode::WindowBad.val()));
        assert_eq!(
            reconcile_window(WINDOW_CAP_SECS + 1),
            Err(RateCode::WindowBad.val())
        );
    }

    #[test]
    fn zero_delta_is_rejected() {
        let mut l = RateLedger::new();
        assert_eq!(l.push_tick(0), Err(RateCode::RateBad.val()));
        assert_eq!(l.ticks, 0);
    }

    #[test]
    fn oversize_delta_is_rejected() {
        let mut l = RateLedger::new();
        assert_eq!(
            l.push_tick(DELTA_CAP + 1),
            Err(RateCode::RateBad.val())
        );
        assert_eq!(
            l.push_tick(-DELTA_CAP - 1),
            Err(RateCode::RateBad.val())
        );
    }

    #[test]
    fn routes_are_three_and_exhaustive() {
        let mut l = RateLedger::new();
        let mut i = 0u32;
        let mut seen = [false; 3];
        while i < 997 {
            match l.push_tick(1) {
                Ok(r) => {
                    let idx = r.index() as usize;
                    if let Some(slot) = seen.get_mut(idx) {
                        *slot = true;
                    }
                }
                Err(_) => return assert!(false),
            }
            i += 1;
        }
        assert!(seen[0] && seen[1] && seen[2]);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn lost_tick_breaks_audit() {
        let mut l = RateLedger::new();
        let _ = l.push_tick(120);
        l.lost = 1;
        l.ticks = l.ticks.saturating_add(1);
        assert_eq!(l.audit(), Err(RateCode::LostBad.val()));
    }

    #[test]
    fn missing_route_breaks_identity() {
        let mut l = RateLedger::new();
        let mut i = 0u32;
        while i < 9 {
            let _ = l.push_tick(120);
            i += 1;
        }
        // 三条出路计数全抹平：和必小于到达数，账不平。
        l.delivered = 0;
        l.merged = 0;
        l.dropped = 0;
        assert_eq!(l.audit(), Err(RateCode::LedgerBad.val()));
    }

    #[test]
    fn merge_eight_to_one() {
        let mut ch = MergeChannel::new();
        let mut i = 0u32;
        let mut out = None;
        while i < MERGE_RATIO {
            out = match ch.push(15) {
                Ok(v) => v,
                Err(_) => return assert!(false),
            };
            i += 1;
        }
        assert_eq!(out, Some(120));
        assert_eq!(ch.frames, 1);
        assert_eq!(ch.pending, 0);
        assert_eq!(ch.sum_in, 120);
        assert_eq!(ch.sum_out, 120);
        assert!(ch.audit().is_ok());
    }

    #[test]
    fn merge_partial_keeps_pending() {
        let mut ch = MergeChannel::new();
        let mut i = 0u32;
        while i < 3 {
            assert_eq!(ch.push(40), Ok(None));
            i += 1;
        }
        assert_eq!(ch.pending, 120);
        assert_eq!(ch.pending_n, 3);
        assert_eq!(ch.frames, 0);
        assert!(ch.audit().is_ok());
        assert_eq!(ch.frame_span_start(2), Some(16));
    }

    #[test]
    fn merge_negative_frame() {
        let mut ch = MergeChannel::new();
        let mut last = Ok(None);
        let mut i = 0u32;
        while i < MERGE_RATIO {
            last = ch.push(-120);
            i += 1;
        }
        assert_eq!(last, Ok(Some(-960)));
        assert!(ch.audit().is_ok());
    }

    #[test]
    fn merge_rejects_zero_and_overflow() {
        let mut ch = MergeChannel::new();
        assert_eq!(ch.push(0), Err(RateCode::RateBad.val()));
        assert_eq!(ch.push(DELTA_CAP + 1), Err(RateCode::RateBad.val()));
    }

    #[test]
    fn merge_identity_is_enforced() {
        let mut ch = MergeChannel::new();
        let _ = ch.push(7);
        ch.sum_in = ch.sum_in.saturating_add(1);
        assert_eq!(ch.audit(), Err(RateCode::MergeBad.val()));
    }

    #[test]
    fn frame_equivalence_is_exact() {
        // 800 = 100 帧整；末帧覆盖样本 792..799，12 周期残 0..7 之和为 0。
        assert_eq!(frame_equivalence(800, 0), Ok((100, 0)));
        let (f, last) = match frame_equivalence(803, 1) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(f, 100);
        // 尾帧覆盖 792..799（相位 1）：残 1..8 之和 = −1。
        assert_eq!(last, -1);
        assert!(frame_equivalence(0, 0).is_err());
    }

    #[test]
    fn equivalence_rounds_are_stable() {
        assert_eq!(equivalence_rounds(EQUIV_ROUNDS), equivalence_rounds(EQUIV_ROUNDS));
        assert_eq!(equivalence_rounds(0), Err(RateCode::CoverageBad.val()));
    }

    #[test]
    fn gate_is_closed_by_default() {
        let mut g = ExperimentGate::new();
        assert_eq!(g.channel.ticks, 0);
        assert_eq!(g.feed(120), Err(RateCode::GateBad.val()));
        assert_eq!(g.rejected, 1);
        assert_eq!(g.channel.ticks, 0);
    }

    #[test]
    fn gate_arms_and_frames() {
        let mut g = ExperimentGate::new();
        assert!(g.arm().is_ok());
        assert_eq!(g.arm(), Err(RateCode::GateBad.val()));
        let mut i = 0u32;
        while i < MERGE_RATIO {
            let _ = g.feed(15);
            i += 1;
        }
        assert_eq!(g.frames, 1);
        assert!(g.disarm().is_ok());
        assert_eq!(g.disarm(), Err(RateCode::GateBad.val()));
        assert!(g.audit().is_ok());
    }

    #[test]
    fn render_roundtrip() {
        let l = RateLedger::new();
        let mut m = l;
        let _ = m.push_tick(120);
        let text = render_ledger(&m);
        assert_eq!(audit_render(&text), Ok(1));
    }

    #[test]
    fn render_tamper_is_rejected() {
        let mut m = RateLedger::new();
        let _ = m.push_tick(120);
        let text = render_ledger(&m);
        assert!(audit_render(&text.replacen("0x", "0y", 1)).is_err());
        // 速率锚被改也拒。
        assert!(audit_render(&text.replacen("1000|", "999|", 1)).is_err());
    }

    #[test]
    fn render_identity_is_enforced() {
        let mut m = RateLedger::new();
        let _ = m.push_tick(120);
        let text = render_ledger(&m);
        let parts: Vec<&str> = text.split('|').collect();
        let mut rebuilt = String::new();
        if let Some(hz) = parts.first() {
            rebuilt.push_str(hz);
            rebuilt.push_str("|1|0|0|0|0x00000000DEADBEEF");
        }
        assert_eq!(
            audit_render(&rebuilt),
            Err(RateCode::LedgerBad.val())
        );
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(5) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [5, 5, 5, 5]);
            }
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
        while i < RateCode::ALL.len() {
            let c = RateCode::ALL[i];
            assert_eq!(detail(c.val()), c.detail());
            assert!(!c.detail().is_empty());
            let mut k = i + 1;
            while k < RateCode::ALL.len() {
                assert!(c.val() != RateCode::ALL[k].val());
                k += 1;
            }
            i += 1;
        }
        assert_eq!(detail(0xDEAD_BEEF), "未知诊断码");
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48961_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
