//! UNX-F48971 热拔零残留与高频打点 ×10^3 · 0x71540
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B40 族第五槽）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F48971-J1 热拔零残留：移除必清空全部通道，到达 = 递送 +
//!      显式丢弃，残留恒为零；
//!   ③ UNX-F48972-J1 高频打点 ×10^3：连号、单调、环窗自洽，乱序/重复/
//!      溢出一律显式拒绝。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：通道先验 → 状态先验 → 容量先验 → 入队 → 起移除 →
//!           停止收件 → 逐通道清空 → 零残留判决 → 收据封账 →
//!           打点连号 → 打点单调 → 环窗复核 → 溢出拒绝。
//! 先验状态再验容量：设备一旦进入移除，任何投喂都必须以「设备已走」
//! 拒绝，而不是以「队列满」拒绝。错误码分不清这两者，运维就会去调
//! 容量，而真正的问题是设备生命周期。
//! 起移除先于清空：移除是一个显式相位，不是「把队列清零」这个动作。
//! 没有相位，清空与投喂可以交错，残留判决就会在竞态里失真。
//! 清空必须逐通道记账：递送与丢弃都要落数，否则「到达 = 递送 + 丢弃」
//! 只能靠总数凑——某一通道悄悄丢光、另一通道重复递送，总数照样平。
//! 零残留是判决而不是统计：收据上的 residue 必须是 0，不是「很小」。
//! 允许非零残留等于允许事件在设备消失后仍滞留，这就是最隐蔽的泄漏。
//! 收据封账后才能打点：打点是「事后凭证」，先有移除结论，打点才有
//! 可锚定的时间原点；顺序反了，打点只是无条件流水，证不了任何事。
//! 打点先连号再单调：连号是同一来源的凭证（缺号 = 丢点），单调是
//! 时间前进的凭证（回拨 = 时源异常）。两者都过，环窗才有复核意义。
//! 关键差异：本模块只回答「热拔是否零残留、打点是否连号单调」，
//!           不做风暴恢复（B40-STORM）、不做水位背压（B40-BACKPRESSURE）、
//!           不做曲线基线（B40-TUNING）。打点不是遥测，它是本地凭证。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71540，B40 族第五槽）。
pub const SEG_BASE: u32 = 0x71540;

/// 通道数（每设备 4 条并行通道）。
pub const LANES: usize = 4;
/// 单通道容量上限。
pub const LANE_CAP: u32 = 256;
/// 热拔轮次（卡面判据的 ×10 轮）。
pub const HOT_ROUNDS: u32 = 10;
/// 热拔轮次上限。
pub const HOT_MAX: u32 = 100;
/// 打点轮次（卡面判据的 ×10^3）。
pub const STAMP_ROUNDS: u32 = 1_000;
/// 打点环窗容量。
pub const STAMP_RING: usize = 32;
/// 打点总数上限。
pub const STAMP_CAP: u32 = 10_000;
/// 收据账字段数。
pub const HOT_FIELDS: usize = 6;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0D） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HrCode {
    /// 通道非法（未知通道或零事件）
    LaneBad,
    /// 状态非法（相位不允许该操作）
    StateBad,
    /// 设备已走（移除后仍被投喂）
    GoneBad,
    /// 残留非零（移除时队列未清空）
    ResidueBad,
    /// 双释放（重复封账）
    DoubleBad,
    /// 打点非法（时间不前进）
    StampBad,
    /// 次序非法（打点不连号）
    OrderBad,
    /// 容量非法（通道或打点环越容）
    CapacityBad,
    /// 账不平（到达 ≠ 递送 + 丢弃）
    LedgerBad,
    /// 渲染非法（字段缺失或回读失败）
    RenderBad,
    /// 复算不一致（同一流重放不同）
    ReplayBad,
    /// 覆盖不足（轮次或打点数不足）
    CoverageBad,
    /// 段位越界（诊断码逃出 0x71540 窗口）
    SegmentBad,
}

impl HrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            HrCode::LaneBad => "通道非法：未知通道或零事件",
            HrCode::StateBad => "状态非法：设备相位不允许该操作",
            HrCode::GoneBad => "设备已走：移除后仍被投喂",
            HrCode::ResidueBad => "残留非零：移除时队列未清空",
            HrCode::DoubleBad => "双释放：重复封账",
            HrCode::StampBad => "打点非法：时间不前进",
            HrCode::OrderBad => "次序非法：打点不连号",
            HrCode::CapacityBad => "容量非法：通道或打点环越容",
            HrCode::LedgerBad => "账不平：到达 ≠ 递送 + 丢弃",
            HrCode::RenderBad => "渲染非法：字段缺失或回读失败",
            HrCode::ReplayBad => "复算不一致：同一流重放结果不同",
            HrCode::CoverageBad => "覆盖不足：轮次或打点数不足",
            HrCode::SegmentBad => "段位越界：诊断码逃出 0x71540 窗口",
        }
    }

    pub const ALL: [HrCode; 13] = [
        HrCode::LaneBad,
        HrCode::StateBad,
        HrCode::GoneBad,
        HrCode::ResidueBad,
        HrCode::DoubleBad,
        HrCode::StampBad,
        HrCode::OrderBad,
        HrCode::CapacityBad,
        HrCode::LedgerBad,
        HrCode::RenderBad,
        HrCode::ReplayBad,
        HrCode::CoverageBad,
        HrCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < HrCode::ALL.len() {
        if HrCode::ALL[i].val() == code {
            return HrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 设备相位 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DevState {
    /// 在线：可收件
    Attached,
    /// 移除中：只允许清空
    Removing,
    /// 已移除：只允许回读收据
    Removed,
}

/// 热拔设备：4 条通道、累计到达、递送、丢弃三本账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HotDevice {
    pub id: u32,
    pub state: DevState,
    /// 每通道在队事件数
    pub queues: [u32; LANES],
    /// 累计到达（入队成功）
    pub queued: u32,
    /// 累计递送
    pub delivered: u32,
    /// 累计显式丢弃
    pub dropped: u32,
    /// 移除后仍被投喂的次数（每次都必须被拒）
    pub gone_hits: u32,
    /// 重复封账次数（每次都必须被拒）
    pub double_hits: u32,
    pub digest: u64,
}

impl HotDevice {
    pub fn new(id: u32) -> HotDevice {
        HotDevice {
            id,
            state: DevState::Attached,
            queues: [0; LANES],
            queued: 0,
            delivered: 0,
            dropped: 0,
            gone_hits: 0,
            double_hits: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 投喂：状态 → 通道 → 零事件 → 容量，四道闸依次校验。
    pub fn submit(&mut self, lane: usize, count: u32) -> Result<(), u32> {
        if self.state != DevState::Attached {
            self.gone_hits = self.gone_hits.saturating_add(1);
            return Err(HrCode::GoneBad.val());
        }
        if lane >= LANES || count == 0 {
            return Err(HrCode::LaneBad.val());
        }
        let cur = match self.queues.get(lane) {
            Some(v) => *v,
            None => return Err(HrCode::LaneBad.val()),
        };
        if cur.saturating_add(count) > LANE_CAP {
            return Err(HrCode::CapacityBad.val());
        }
        if let Some(slot) = self.queues.get_mut(lane) {
            *slot = cur.saturating_add(count);
        }
        self.queued = self.queued.saturating_add(count);
        self.digest = p_core::fold_u32(self.digest, lane as u32);
        self.digest = p_core::fold_u32(self.digest, count);
        Ok(())
    }

    /// 进入移除相位：只允许从在线进入一次。
    pub fn begin_remove(&mut self) -> Result<(), u32> {
        if self.state != DevState::Attached {
            return Err(HrCode::StateBad.val());
        }
        self.state = DevState::Removing;
        Ok(())
    }

    /// 逐通道清空：`deliver_mask` 的第 i 位置 1 表示通道 i 递送、否则丢弃。
    pub fn drain_lanes(&mut self, deliver_mask: u32) -> Result<u32, u32> {
        if self.state != DevState::Removing {
            return Err(HrCode::StateBad.val());
        }
        let mut moved = 0u32;
        let mut i = 0usize;
        while i < LANES {
            let q = match self.queues.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if q > 0 {
                if (deliver_mask >> i) & 1 == 1 {
                    self.delivered = self.delivered.saturating_add(q);
                } else {
                    self.dropped = self.dropped.saturating_add(q);
                }
                if let Some(slot) = self.queues.get_mut(i) {
                    *slot = 0;
                }
                moved = moved.saturating_add(q);
            }
            i = i.saturating_add(1);
        }
        Ok(moved)
    }

    /// 封账：残留必须为零，到达必须等于递送 + 丢弃。
    pub fn finish(&mut self) -> Result<RemovalReceipt, u32> {
        if self.state != DevState::Removing {
            self.double_hits = self.double_hits.saturating_add(1);
            return Err(HrCode::StateBad.val());
        }
        let mut residue = 0u32;
        let mut i = 0usize;
        while i < LANES {
            residue = residue.saturating_add(match self.queues.get(i) {
                Some(v) => *v,
                None => 0,
            });
            i = i.saturating_add(1);
        }
        if residue != 0 {
            // 残留非零不算「差一点成功」：封账直接被拒，相位停在移除中。
            return Err(HrCode::ResidueBad.val());
        }
        if self.queued != self.delivered.saturating_add(self.dropped) {
            return Err(HrCode::LedgerBad.val());
        }
        self.state = DevState::Removed;
        Ok(RemovalReceipt {
            lanes: LANES as u32,
            queued: self.queued,
            delivered: self.delivered,
            dropped: self.dropped,
            residue: 0,
            rejected: self.gone_hits,
            digest: self.digest,
        })
    }

    /// 当前残留（仅供审计读取，不参与决策）。
    pub fn residue(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < LANES {
            s = s.saturating_add(match self.queues.get(i) {
                Some(v) => *v,
                None => 0,
            });
            i = i.saturating_add(1);
        }
        s
    }
}

/// 移除收据：零残留判决的凭证。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RemovalReceipt {
    pub lanes: u32,
    pub queued: u32,
    pub delivered: u32,
    pub dropped: u32,
    /// 恒为 0（非零不得封账）
    pub residue: u32,
    /// 移除后仍被投喂且被拒的次数
    pub rejected: u32,
    pub digest: u64,
}

// ── 热拔轮次与汇总账 ──────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HotRound {
    pub round: u32,
    pub queued: u32,
    pub delivered: u32,
    pub dropped: u32,
    /// 本轮被拒的接收（移除后投喂）
    pub rejected: u32,
    /// 本轮检测到的重复封账
    pub doubles: u32,
    pub digest: u64,
}

/// 单轮热拔：填 → 起移除 → 停止收件 → 逐通道清空 → 封账 → 再试。
pub fn hot_round(round: u32) -> Result<HotRound, u32> {
    if round >= HOT_MAX {
        return Err(HrCode::CoverageBad.val());
    }
    let mut dev = HotDevice::new(round);
    let n = 40u32.saturating_add((round % 5).saturating_mul(8));
    let mut i = 0u32;
    while i < n {
        let lane = ((i as usize).saturating_add(round as usize)) % LANES;
        dev.submit(lane, 1)?;
        i = i.saturating_add(1);
    }
    dev.begin_remove()?;
    // 移除相位开始后仍有两路投喂到达：必须全部以「设备已走」拒绝。
    let _ = dev.submit(0, 1);
    let _ = dev.submit(1, 1);
    // 逐通道清空：偶数通道递送、奇数通道显式丢弃。
    let moved = dev.drain_lanes(0b0101)?;
    if moved != n {
        return Err(HrCode::LedgerBad.val());
    }
    let receipt = dev.finish()?;
    if receipt.residue != 0 || receipt.lanes != LANES as u32 {
        return Err(HrCode::ResidueBad.val());
    }
    // 双释放：再次封账必须被拒。
    let again = dev.finish();
    if again != Err(HrCode::StateBad.val()) {
        return Err(HrCode::DoubleBad.val());
    }
    // 移除完成后仍被投喂：必须被拒。
    if dev.submit(2, 1) != Err(HrCode::GoneBad.val()) {
        return Err(HrCode::GoneBad.val());
    }
    if receipt.queued != receipt.delivered.saturating_add(receipt.dropped) {
        return Err(HrCode::LedgerBad.val());
    }
    Ok(HotRound {
        round,
        queued: receipt.queued,
        delivered: receipt.delivered,
        dropped: receipt.dropped,
        rejected: dev.gone_hits,
        doubles: dev.double_hits,
        digest: dev.digest,
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HotLedger {
    pub rounds: u32,
    pub queued: u64,
    pub delivered: u64,
    pub dropped: u64,
    pub rejected: u32,
    pub doubles: u32,
    pub digest: u64,
}

/// 热拔 ×N 轮：逐轮核对零残留与恒等式。
pub fn hot_rounds(rounds: u32) -> Result<HotLedger, u32> {
    if rounds == 0 || rounds > HOT_MAX {
        return Err(HrCode::CoverageBad.val());
    }
    let mut queued = 0u64;
    let mut delivered = 0u64;
    let mut dropped = 0u64;
    let mut rejected = 0u32;
    let mut doubles = 0u32;
    let mut h = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        let rec = hot_round(r)?;
        if rec.queued != rec.delivered.saturating_add(rec.dropped) {
            return Err(HrCode::LedgerBad.val());
        }
        if rec.delivered == 0 || rec.dropped == 0 {
            return Err(HrCode::LedgerBad.val());
        }
        queued = queued.saturating_add(rec.queued as u64);
        delivered = delivered.saturating_add(rec.delivered as u64);
        dropped = dropped.saturating_add(rec.dropped as u64);
        rejected = rejected.saturating_add(rec.rejected);
        doubles = doubles.saturating_add(rec.doubles);
        h = p_core::fold_u32(h, rec.round);
        h = p_core::fold_u32(h, rec.queued);
        h = p_core::fold_u32(h, rec.dropped);
        h = p_core::fold_u32(h, rec.rejected);
        r = r.saturating_add(1);
    }
    if queued != delivered.saturating_add(dropped) {
        return Err(HrCode::LedgerBad.val());
    }
    Ok(HotLedger {
        rounds,
        queued,
        delivered,
        dropped,
        rejected,
        doubles,
        digest: h,
    })
}

// ── 高频打点（×10^3） ─────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StampBook {
    /// 已收打点数
    pub count: u32,
    /// 下一个期望序号（从 0 起严格连号）
    pub next_seq: u32,
    /// 上一个时间戳（必须严格递增）
    pub last_tick: u64,
    /// 环形窗口（槽位 = 序号 % STAMP_RING，值为序号）
    pub ring: [u32; STAMP_RING],
    /// 被拒次数（乱序/重复/回拨/溢出）
    pub errors: u32,
    pub digest: u64,
}

impl StampBook {
    pub fn new() -> StampBook {
        StampBook {
            count: 0,
            next_seq: 0,
            last_tick: 0,
            ring: [0; STAMP_RING],
            errors: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 打一点：容量 → 连号 → 时间前进，三道闸依次校验。
    pub fn push(&mut self, seq: u32, tick: u64) -> Result<(), u32> {
        if self.count >= STAMP_CAP {
            self.errors = self.errors.saturating_add(1);
            return Err(HrCode::CapacityBad.val());
        }
        if seq != self.next_seq {
            // 缺号、重号、跳号都是「同一来源不连号」：一律显式拒绝。
            self.errors = self.errors.saturating_add(1);
            return Err(HrCode::OrderBad.val());
        }
        if self.count > 0 && tick <= self.last_tick {
            self.errors = self.errors.saturating_add(1);
            return Err(HrCode::StampBad.val());
        }
        let slot = (self.count as usize) % STAMP_RING;
        if let Some(cell) = self.ring.get_mut(slot) {
            *cell = seq;
        }
        self.next_seq = seq.saturating_add(1);
        self.last_tick = tick;
        self.count = self.count.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, seq);
        self.digest = p_core::fold_u32(self.digest, (tick & 0xFFFF_FFFF) as u32);
        Ok(())
    }

    /// 环窗审计：连号、单调、每个槽位的值必须等于「最近一次落入该槽的序号」。
    pub fn audit(&self) -> Result<(), u32> {
        if self.count == 0 {
            return Err(HrCode::CoverageBad.val());
        }
        if self.next_seq != self.count {
            return Err(HrCode::OrderBad.val());
        }
        if self.count > 1 && self.last_tick == 0 {
            return Err(HrCode::StampBad.val());
        }
        let n = self.count as usize;
        let mut s = 0usize;
        while s < STAMP_RING {
            let want = if n >= STAMP_RING {
                let last = (n - 1) as u32;
                let s32 = s as u32;
                let back = last.saturating_sub(s32) % (STAMP_RING as u32);
                Some(last.saturating_sub(back))
            } else if s < n {
                Some(s as u32)
            } else {
                None
            };
            if let Some(want_v) = want {
                if self.ring.get(s) != Some(&want_v) {
                    return Err(HrCode::StampBad.val());
                }
            }
            s = s.saturating_add(1);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StampLedger {
    pub count: u32,
    pub last_seq: u32,
    pub last_tick: u64,
    /// 被拒次数（乱序注入等）
    pub errors: u32,
    pub digest: u64,
}

/// 打点 ×N：序号从 0 连号，时间戳每步递增 3。
pub fn stamp_rounds(stamps: u32) -> Result<StampLedger, u32> {
    if stamps == 0 || stamps > STAMP_CAP {
        return Err(HrCode::CoverageBad.val());
    }
    let mut b = StampBook::new();
    let mut i = 0u32;
    while i < stamps {
        let tick = 1_000u64.saturating_add((i as u64).saturating_mul(3));
        b.push(i, tick)?;
        i = i.saturating_add(1);
    }
    b.audit()?;
    Ok(StampLedger {
        count: b.count,
        last_seq: b.next_seq.saturating_sub(1),
        last_tick: b.last_tick,
        errors: b.errors,
        digest: b.digest,
    })
}

// ── fuzz 面 ───────────────────────────────────────────────────────
/// 热拔 fuzz：每轮收据恒等与零残留必须成立。
pub fn fuzz_hot_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 || rounds > 10_000 {
        return Err(HrCode::CoverageBad.val());
    }
    let mut h = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        let rec = hot_round(r % HOT_MAX)?;
        if rec.queued != rec.delivered.saturating_add(rec.dropped) {
            return Err(HrCode::LedgerBad.val());
        }
        h = p_core::fold_u32(h, rec.queued);
        h = p_core::fold_u32(h, rec.rejected);
        r = r.saturating_add(1);
    }
    Ok(h)
}

/// 打点 fuzz：乱序/重复/回拨必须被拒，且账体不被污染。
pub fn fuzz_stamp_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 || rounds > STAMP_CAP {
        return Err(HrCode::CoverageBad.val());
    }
    let mut rng = p_core::Lcg::new(0x5354_0045);
    let mut b = StampBook::new();
    let mut h = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        let a = rng.next();
        let pick = (a >> 33) % 10;
        let seq = b.next_seq;
        let tick = b.last_tick.saturating_add(1);
        if pick < 7 {
            b.push(seq, tick)?;
        } else if pick < 8 {
            // 重号：把上一号再打一次。
            let dup = seq.saturating_sub(1);
            let _ = b.push(dup, tick);
        } else if pick < 9 {
            // 回拨：时间不前进。
            let back = b.last_tick;
            let _ = b.push(seq, back);
        } else {
            // 跳号：跳过下一号。
            let _ = b.push(seq.saturating_add(2), tick);
        }
        b.audit()?;
        h = p_core::fold_u32(h, b.count);
        r = r.saturating_add(1);
    }
    if b.count == 0 || b.errors == 0 {
        return Err(HrCode::CoverageBad.val());
    }
    Ok(h)
}

// ── 渲染与回读 ────────────────────────────────────────────────────
pub fn render_hot(l: &HotLedger, s: &StampLedger) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        l.rounds,
        l.queued,
        l.delivered,
        l.dropped,
        s.count,
        p_core::fold_u32(p_core::fold_u32(l.digest, s.digest as u32), (s.digest >> 32) as u32)
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(HrCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(HrCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(HrCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(HrCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(HrCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(HrCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(HrCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(HrCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 渲染复读：轮次、到达/递送/丢弃恒等、打点数、摘要五项。
pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != HOT_FIELDS {
        return Err(HrCode::RenderBad.val());
    }
    let rounds = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(HrCode::RenderBad.val()),
    })?;
    let queued = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(HrCode::RenderBad.val()),
    })?;
    let delivered = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(HrCode::RenderBad.val()),
    })?;
    let dropped = parse_u32(match parts.get(3) {
        Some(v) => v,
        None => return Err(HrCode::RenderBad.val()),
    })?;
    let stamps = parse_u32(match parts.get(4) {
        Some(v) => v,
        None => return Err(HrCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(5) {
        Some(v) => v,
        None => return Err(HrCode::RenderBad.val()),
    })?;
    if rounds == 0 || rounds > HOT_MAX {
        return Err(HrCode::RenderBad.val());
    }
    if queued != delivered.saturating_add(dropped) {
        return Err(HrCode::LedgerBad.val());
    }
    if delivered == 0 || dropped == 0 {
        return Err(HrCode::ResidueBad.val());
    }
    if stamps == 0 || stamps > STAMP_CAP {
        return Err(HrCode::CapacityBad.val());
    }
    if digest == 0 {
        return Err(HrCode::RenderBad.val());
    }
    Ok(rounds)
}

pub fn golden_run() -> Result<(u32, u64, u64, u64, u32, u64), u32> {
    let hot = hot_rounds(HOT_ROUNDS)?;
    let stamps = stamp_rounds(STAMP_ROUNDS)?;
    if stamps.count != STAMP_ROUNDS {
        return Err(HrCode::OrderBad.val());
    }
    let text = render_hot(&hot, &stamps);
    if audit_render(&text)? != hot.rounds {
        return Err(HrCode::ReplayBad.val());
    }
    if hot.queued != hot.delivered.saturating_add(hot.dropped) {
        return Err(HrCode::ResidueBad.val());
    }
    Ok((
        hot.rounds,
        hot.queued,
        hot.delivered,
        hot.dropped,
        stamps.count,
        p_core::fold_u32(hot.digest, stamps.digest as u32),
    ))
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
    if rounds == 0 || rounds > 1_000 {
        return Err(HrCode::CoverageBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut r = 0u32;
    while r < rounds {
        // ① 通道越界。
        log.tries[0] = log.tries[0].saturating_add(1);
        let mut a = HotDevice::new(0);
        if a.submit(LANES, 1) == Err(HrCode::LaneBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }

        // ② 移除后投喂。
        log.tries[1] = log.tries[1].saturating_add(1);
        let mut b = HotDevice::new(0);
        let _ = b.submit(0, 4);
        let _ = b.begin_remove();
        if b.submit(0, 1) == Err(HrCode::GoneBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }

        // ③ 未清空就封账：残留必须被拒。
        log.tries[2] = log.tries[2].saturating_add(1);
        let mut c = HotDevice::new(0);
        let _ = c.submit(1, 8);
        let _ = c.begin_remove();
        if c.finish() == Err(HrCode::ResidueBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }

        // ④ 打点重号：同一序号再打一次必须被拒。
        log.tries[3] = log.tries[3].saturating_add(1);
        let mut d = StampBook::new();
        let _ = d.push(0, 100);
        if d.push(0, 101) == Err(HrCode::OrderBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }

        r = r.saturating_add(1);
    }
    Ok(log)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if LANES != 4 || LANE_CAP != 256 || STAMP_RING != 32 {
        return Err(HrCode::CapacityBad.val());
    }
    let l = hot_rounds(HOT_ROUNDS)?;
    // 十轮到达 = 10×40 + 8×(0+1+2+3+4)×2 = 560；递送/丢弃各半。
    if l.queued != 560 || l.delivered != 280 || l.dropped != 280 {
        return Err(HrCode::LedgerBad.val());
    }
    if l.rejected != 30 || l.doubles != HOT_ROUNDS {
        return Err(HrCode::DoubleBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let s = stamp_rounds(STAMP_ROUNDS)?;
    if s.count != STAMP_ROUNDS || s.last_seq != STAMP_ROUNDS - 1 {
        return Err(HrCode::OrderBad.val());
    }
    if s.last_tick != 1_000u64.saturating_add((STAMP_ROUNDS as u64 - 1).saturating_mul(3)) {
        return Err(HrCode::StampBad.val());
    }
    if s.errors != 0 {
        return Err(HrCode::StampBad.val());
    }
    // 环窗自洽：重放后审计仍绿。
    let mut b = StampBook::new();
    let mut i = 0u32;
    while i < STAMP_CAP.min(STAMP_RING as u32 + 5) {
        b.push(i, 10u64.saturating_add(i as u64))?;
        i = i.saturating_add(1);
    }
    b.audit()?;
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 容量：单通道塞满之后再来 1 个必须被拒。
    let mut d = HotDevice::new(7);
    d.submit(0, LANE_CAP)?;
    if d.submit(0, 1) != Err(HrCode::CapacityBad.val()) {
        return Err(HrCode::CapacityBad.val());
    }
    // 残留未清零就封账：被拒且相位停在移除中。
    let _ = d.begin_remove();
    if d.finish() != Err(HrCode::ResidueBad.val()) {
        return Err(HrCode::ResidueBad.val());
    }
    if d.state != DevState::Removing {
        return Err(HrCode::StateBad.val());
    }
    // 清空后封账成功，再次封账被拒。
    let moved = d.drain_lanes(0xFFFF)?;
    if moved != LANE_CAP {
        return Err(HrCode::LedgerBad.val());
    }
    let rec = d.finish()?;
    if rec.residue != 0 || rec.queued != rec.delivered.saturating_add(rec.dropped) {
        return Err(HrCode::ResidueBad.val());
    }
    if d.finish() != Err(HrCode::StateBad.val()) {
        return Err(HrCode::DoubleBad.val());
    }
    // 打点回拨：时间不前进必须被拒。
    let mut s = StampBook::new();
    s.push(0, 50)?;
    if s.push(1, 50) != Err(HrCode::StampBad.val()) {
        return Err(HrCode::StampBad.val());
    }
    if s.push(3, 51) != Err(HrCode::OrderBad.val()) {
        return Err(HrCode::OrderBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = hot_rounds(HOT_ROUNDS)?;
    let b = hot_rounds(HOT_ROUNDS)?;
    if a.digest != b.digest || a.queued != b.queued || a.dropped != b.dropped {
        return Err(HrCode::ReplayBad.val());
    }
    if stamp_rounds(STAMP_ROUNDS)?.digest != stamp_rounds(STAMP_ROUNDS)?.digest {
        return Err(HrCode::ReplayBad.val());
    }
    if fuzz_hot_rounds(300)? != fuzz_hot_rounds(300)? {
        return Err(HrCode::ReplayBad.val());
    }
    if fuzz_stamp_rounds(300)? != fuzz_stamp_rounds(300)? {
        return Err(HrCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < HrCode::ALL.len() {
        let v = HrCode::ALL[i].val();
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

    out.push((
        "F48971 热拔锚定",
        LANES == 4 && LANE_CAP == 256 && HOT_ROUNDS == 10 && STAMP_RING == 32,
        format!(
            "lanes={} lane_cap={} rounds={} ring={}",
            LANES, LANE_CAP, HOT_ROUNDS, STAMP_RING
        ),
    ));

    let hot = hot_rounds(HOT_ROUNDS);
    out.push((
        "F48971 移除账×10零残留",
        matches!(&hot, Ok(l) if l.rounds == HOT_ROUNDS
            && l.queued == l.delivered + l.dropped
            && l.queued == 560
            && l.delivered == 280
            && l.dropped == 280),
        match &hot {
            Ok(l) => format!(
                "rounds={} queued={} delivered={} dropped={} residue=0",
                l.rounds, l.queued, l.delivered, l.dropped
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48971 移除后拒绝与双释放",
        matches!(&hot, Ok(l) if l.rejected == 30 && l.doubles == HOT_ROUNDS),
        match &hot {
            Ok(l) => format!("rejected={} doubles={}", l.rejected, l.doubles),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let stamps = stamp_rounds(STAMP_ROUNDS);
    out.push((
        "F48972 打点×10^3连号单调",
        matches!(&stamps, Ok(s) if s.count == STAMP_ROUNDS
            && s.last_seq == STAMP_ROUNDS - 1
            && s.errors == 0
            && s.last_tick == 3_997),
        match &stamps {
            Ok(s) => format!(
                "count={} last_seq={} last_tick={} errors={}",
                s.count, s.last_seq, s.last_tick, s.errors
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut guard_ok = false;
    let mut guard_note = 0u32;
    let mut b = StampBook::new();
    let ok_a = b.push(0, 10).is_ok();
    let rej_dup = b.push(0, 11) == Err(HrCode::OrderBad.val());
    let rej_back = b.push(1, 10) == Err(HrCode::StampBad.val());
    let ok_b = b.push(1, 11).is_ok();
    let rej_jump = b.push(3, 12) == Err(HrCode::OrderBad.val());
    if ok_a && rej_dup && rej_back && ok_b && rej_jump {
        guard_ok = b.audit().is_ok();
        guard_note = b.errors;
    }
    out.push((
        "F48972 乱序重复回拨拒绝",
        guard_ok && guard_note == 3,
        format!("ok={} errors={}", guard_ok, guard_note),
    ));

    let fuzz_a = fuzz_hot_rounds(10_000);
    let fuzz_b = fuzz_stamp_rounds(2_000);
    out.push((
        "F48971 fuzz×10^4不崩",
        fuzz_a.is_ok() && fuzz_b.is_ok(),
        format!(
            "hot={} stamp={}",
            match &fuzz_a {
                Ok(h) => format!("0x{:016X}", h),
                Err(e) => format!("err=0x{:X}", e),
            },
            match &fuzz_b {
                Ok(h) => format!("0x{:016X}", h),
                Err(e) => format!("err=0x{:X}", e),
            }
        ),
    ));

    let inject = inject_rounds(5);
    out.push((
        "F48971 四路注入捕获",
        matches!(&inject, Ok(l) if l.all_caught()),
        match &inject {
            Ok(l) => format!("caught={:?}", l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let golden = golden_run();
    out.push((
        "F48971 收据可解析",
        golden.is_ok(),
        match &golden {
            Ok((r, q, d, x, s, h)) => format!(
                "rounds={} queued={} delivered={} dropped={} stamps={} d=0x{:X}",
                r, q, d, x, s, h
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out
}

pub fn run_unx48971_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48971 热拔零残留与高频打点");
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
        assert_eq!(SEG_BASE, 0x71540);
        assert_eq!(LANES, 4);
        assert_eq!(STAMP_ROUNDS, 1_000);
        assert_eq!(HOT_FIELDS, 6);
    }

    #[test]
    fn submit_fills_lanes() {
        let mut d = HotDevice::new(1);
        assert!(d.submit(0, 10).is_ok());
        assert!(d.submit(3, 5).is_ok());
        assert_eq!(d.queued, 15);
        assert_eq!(d.residue(), 15);
        assert_eq!(d.queues[0], 10);
        assert_eq!(d.queues[3], 5);
    }

    #[test]
    fn submit_rejects_bad_inputs() {
        let mut d = HotDevice::new(1);
        assert_eq!(d.submit(LANES, 1), Err(HrCode::LaneBad.val()));
        assert_eq!(d.submit(0, 0), Err(HrCode::LaneBad.val()));
        assert_eq!(d.submit(0, LANE_CAP + 1), Err(HrCode::CapacityBad.val()));
        assert_eq!(d.queued, 0);
    }

    #[test]
    fn submit_after_remove_is_rejected() {
        let mut d = HotDevice::new(1);
        let _ = d.submit(0, 4);
        let _ = d.begin_remove();
        assert_eq!(d.submit(0, 1), Err(HrCode::GoneBad.val()));
        assert_eq!(d.gone_hits, 1);
    }

    #[test]
    fn finish_requires_zero_residue() {
        let mut d = HotDevice::new(1);
        let _ = d.submit(2, 9);
        let _ = d.begin_remove();
        assert_eq!(d.finish(), Err(HrCode::ResidueBad.val()));
        assert_eq!(d.state, DevState::Removing);
        let moved = d.drain_lanes(0b0001);
        assert_eq!(moved, Ok(9));
        let rec = d.finish();
        assert!(rec.is_ok());
        if let Ok(r) = rec {
            assert_eq!(r.residue, 0);
            assert_eq!(r.queued, r.delivered + r.dropped);
            assert_eq!(r.lanes, LANES as u32);
        }
    }

    #[test]
    fn drain_splits_deliver_and_drop() {
        let mut d = HotDevice::new(2);
        let _ = d.submit(0, 10);
        let _ = d.submit(1, 20);
        let _ = d.begin_remove();
        assert_eq!(d.drain_lanes(0b0001), Ok(30));
        assert_eq!(d.delivered, 10);
        assert_eq!(d.dropped, 20);
        assert_eq!(d.residue(), 0);
    }

    #[test]
    fn double_finish_is_rejected() {
        let mut d = HotDevice::new(3);
        let _ = d.submit(0, 1);
        let _ = d.begin_remove();
        let _ = d.drain_lanes(0b0001);
        assert!(d.finish().is_ok());
        assert_eq!(d.finish(), Err(HrCode::StateBad.val()));
        assert_eq!(d.double_hits, 1);
    }

    #[test]
    fn begin_remove_only_once() {
        let mut d = HotDevice::new(4);
        assert!(d.begin_remove().is_ok());
        assert_eq!(d.begin_remove(), Err(HrCode::StateBad.val()));
    }

    #[test]
    fn hot_round_receipt_identity() {
        let r = hot_round(0);
        assert!(r.is_ok());
        if let Ok(v) = r {
            assert_eq!(v.queued, 40);
            assert_eq!(v.delivered + v.dropped, 40);
            assert_eq!(v.rejected, 3);
            assert_eq!(v.doubles, 1);
        }
    }

    #[test]
    fn hot_rounds_ten_totals() {
        let l = hot_rounds(HOT_ROUNDS);
        assert!(l.is_ok());
        if let Ok(v) = l {
            assert_eq!(v.queued, 560);
            assert_eq!(v.delivered, 280);
            assert_eq!(v.dropped, 280);
            assert_eq!(v.rejected, 30);
            assert_eq!(v.doubles, HOT_ROUNDS);
        }
    }

    #[test]
    fn hot_rounds_rejects_bad_rounds() {
        assert_eq!(hot_rounds(0), Err(HrCode::CoverageBad.val()));
        assert_eq!(hot_rounds(HOT_MAX + 1), Err(HrCode::CoverageBad.val()));
    }

    #[test]
    fn stamps_are_consecutive() {
        let s = match stamp_rounds(100) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(s.count, 100);
        assert_eq!(s.last_seq, 99);
        assert_eq!(s.last_tick, 1_000 + 99 * 3);
        assert_eq!(s.errors, 0);
    }

    #[test]
    fn stamps_reject_duplicate_and_backward() {
        let mut b = StampBook::new();
        assert!(b.push(0, 10).is_ok());
        assert_eq!(b.push(0, 11), Err(HrCode::OrderBad.val()));
        assert_eq!(b.push(1, 10), Err(HrCode::StampBad.val()));
        assert!(b.push(1, 11).is_ok());
        assert_eq!(b.errors, 2);
        assert!(b.audit().is_ok());
    }

    #[test]
    fn stamps_capacity_is_enforced() {
        let mut b = StampBook::new();
        let mut i = 0u32;
        while i < STAMP_CAP {
            assert!(b.push(i, 1u64 + i as u64).is_ok());
            i = i.saturating_add(1);
        }
        assert_eq!(
            b.push(STAMP_CAP, 1u64 + STAMP_CAP as u64),
            Err(HrCode::CapacityBad.val())
        );
        assert!(b.audit().is_ok());
    }

    #[test]
    fn ring_wraps_and_audits() {
        let mut b = StampBook::new();
        let n = STAMP_RING as u32 + 5;
        let mut i = 0u32;
        while i < n {
            assert!(b.push(i, 1u64 + i as u64).is_ok());
            i = i.saturating_add(1);
        }
        assert!(b.audit().is_ok());
        // 篡改一个槽位必须被环窗审计抓到。
        b.ring[0] = 0;
        assert_eq!(b.audit(), Err(HrCode::StampBad.val()));
    }

    #[test]
    fn stamp_rounds_reject_bad_counts() {
        assert_eq!(stamp_rounds(0), Err(HrCode::CoverageBad.val()));
        assert_eq!(
            stamp_rounds(STAMP_CAP + 1),
            Err(HrCode::CoverageBad.val())
        );
    }

    #[test]
    fn fuzz_faces_are_green() {
        assert!(fuzz_hot_rounds(2_000).is_ok());
        assert!(fuzz_stamp_rounds(2_000).is_ok());
        assert_eq!(fuzz_hot_rounds(0), Err(HrCode::CoverageBad.val()));
        assert_eq!(fuzz_stamp_rounds(0), Err(HrCode::CoverageBad.val()));
    }

    #[test]
    fn render_roundtrip() {
        let hot = match hot_rounds(HOT_ROUNDS) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let stamps = match stamp_rounds(STAMP_ROUNDS) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let text = render_hot(&hot, &stamps);
        assert_eq!(audit_render(&text), Ok(HOT_ROUNDS));
    }

    #[test]
    fn render_tamper_is_rejected() {
        let hot = match hot_rounds(1) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let stamps = match stamp_rounds(10) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let text = render_hot(&hot, &stamps);
        assert!(audit_render(&text.replacen("0x", "0y", 1)).is_err());
        // 递送数被改：到达 ≠ 递送 + 丢弃。
        assert!(audit_render(&text.replacen("|20|", "|21|", 1)).is_err());
        assert!(audit_render("1|2|3").is_err());
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
        while i < HrCode::ALL.len() {
            let c = HrCode::ALL[i];
            assert_eq!(detail(c.val()), c.detail());
            let mut k = i + 1;
            while k < HrCode::ALL.len() {
                assert!(c.val() != HrCode::ALL[k].val());
                k += 1;
            }
            i += 1;
        }
        assert!(segment_guard());
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48971_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
