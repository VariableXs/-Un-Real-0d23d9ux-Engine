//! # unx49083 — B15 乱序时间戳处置与 4096 缓冲溢出丢帧记账
//!
//! 卡面原文（任务板 CoRun·B15）：
//!
//! > **F49083 乱序时间戳处置全路径（设备重置流程实测终验）**
//! > 验收：① 纯功能代码 ≥360 行；② 判据原文：`UNX-F49084-J1` 溢出 ×10 记账断言全过；
//! > 判据锚定 §AI-62 §2 专题一错误路径。
//!
//! > **F49084 缓冲溢出处置（4,096 上限溢出丢帧记账断言终验）**
//! > 验收：① 纯功能代码 ≥340 行；② 判据原文：`UNX-F49085-J1` 回退 ×10 全过；
//! > 判据锚定 §AI-62 §2 专题二错误路径。
//!
//! ## 唯一不变量
//!
//! ```text
//! 入口量 = 出口量 + 残留量 + 丢弃量            （窗口与环形缓冲同式）
//! ```
//!
//! 窗口侧入口是全部提交帧，丢弃分「重复 / 迟到 / 满窗 / 序号非法」四桶；环形缓冲侧
//! 入口是写入量加显式丢弃量，丢弃分「满溢 / 迟到 / 重复 / 关闸」四桶。任何一桶没数
//! 上，等式就崩——本模块第一条要拦的假绿是「丢了帧但账上还是平的」。时间戳是 `u32`
//! 单调节拍，会在 2^32 处回绕；比较一律走 `wrapping_sub` 后的 `i32` 有符号差值，且
//! 差值须落在半个回绕空间内（`within_half`），否则既不插入也不吐出，只计入迟到桶。
//!
//! ## 闸序与理由
//!
//! 1. **窗内重排**：接受帧按节拍升序整批吐出——单调输出是下游唯一契约，乱序漏到
//!    出口后所有记账失去参照。
//! 2. **重复与迟到拒收**：同序号/同节拍与低于水位的帧显性落桶——静默去重会让残留
//!    量凭空消失，等式成立其实是少算了帧。
//! 3. **回绕差值**：`0xFFFF_FFF8 → 0x0000_0004` 必须为正且等于 `0xC`——裸 `>` 比较
//!    会把回绕后的新帧判成迟到帧，回绕点丢掉半个窗口。
//! 4. **重置 ×10 零残留**：每轮清窗后残留（帧数 + 接受量 + 吐出量 + 四桶）必须为
//!    0——作废不彻底等于把上一轮错误带进下一轮。
//! 5. **4096 上限记账**：容量钉死 4096，溢出帧进满溢桶，最终
//!    `丢弃量 == 入口量 - 出口量 - 残留量` 必须是恒等式而非估算。
//! 6. **账行回读**：键序固定、摘要用 FNV 重算，任一字节被改动都必须判否——可改的
//!    行等于没有行。
//!
//! ## 与相邻模块的差异
//!
//! - `unx49091_register` 登记**补测项与验收维度**三态；本模块登记**帧级**进出账，
//!   前者问「测没测」，后者问「帧去哪了」。
//! - `unx48941_wheel_core` 供 FNV 折叠原语与轮盘骨架；本模块只借 `FNV_OFF /
//!   fold_u32 / fold_byte`，不复用其摘要域与结构（轮盘环形推进，本模块有序插入）。
//! - `unx49089_finalgrep` 是 grep 留痕，本模块是**运行期**处置路径，段位 0x73540
//!   起 14 码，互不重叠；本模块也不持有全局可变状态，窗口与环都是局部对象。
//!
//! ## 诊断码段
//!
//! 段基址 `SEG_BASE = 0x73540`，14 条诊断码占 `0x73541..=0x7354E`，段宽 `0x40`；
//! 段外码值一律回落 `UNKNOWN`。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

// ── 段位与窗口 ───────────────────────────────────────────────────
/// 本模块段基址（B15 第七槽）。
pub const SEG_BASE: u32 = 0x73540;
/// 段宽（相邻模块互斥）。
pub const SEG_SPAN: u32 = 0x40;
/// 重排窗槽数（有界）。
pub const WINDOW: usize = 32;
/// 第一阶段释放水位 / 脚本切分。
pub const PHASE_A_HORIZON: u32 = 1020;
/// 第一阶段脚本长度。
pub const PHASE_A_LEN: usize = 16;
/// 收尾段脚本起点。
pub const PHASE_C_START: usize = 16;
/// 脚本长度。
pub const SCRIPT_LEN: usize = 24;
/// 帧类型上限 / 序号窗口。
pub const KIND_MAX: u8 = 3;
pub const SEQ_MIN: u32 = 1;
pub const SEQ_MAX: u32 = 0x000F_FFFF;
/// 回绕半空间。
pub const TICK_HALF: u32 = 0x8000_0000;
/// 环形缓冲容量，钉死 4096。
pub const RING_CAP: usize = 4096;
/// 溢出注入 / 回补 / 首次出队帧数。
pub const RING_OVERFLOW: u32 = 256;
pub const RING_REFILL: u32 = 100;
pub const RING_DRAIN: u32 = 2048;
/// 显式丢弃分桶（迟到 12 + 重复 5 + 关闸 3）。
pub const RING_TAG_DROPS: u32 = 20;
pub const LATE_DROPS: u32 = 12;
pub const DUP_DROPS: u32 = 5;
pub const BARRIER_DROPS: u32 = 3;
/// 回退轮内溢出帧数。
pub const ROLLBACK_OVERFLOW: u32 = 64;
/// 重置轮数与每轮前置帧数（判据 ×10）。
pub const RESET_ROUNDS: u32 = 10;
pub const RESET_FRAMES: u32 = 8;
/// 丢弃原因桶数 / 账行槽数。
pub const REASONS: usize = 4;
pub const ROW_SLOTS: usize = 4;
/// 账行标签与键序。
pub const ROW_TAG: &str = "ORDR83";
pub const ROW_KEYS: [&str; 5] = ["slot", "pushed", "delivered", "retained", "dropped"];
/// 未知诊断码说明 / 摘要域标签。
pub const UNKNOWN: &str = "未知 B15 乱序/溢出诊断码";
pub const DIGEST_TAG: u32 = 0x4F52_4452;

// ── 诊断码 ───────────────────────────────────────────────────────
/// 乱序与溢出处置诊断码（14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrdCode {
    TsDuplicate, // 窗内重复帧
    TsStale,     // 低于水位的迟到帧
    TsOrder,     // 输出非单调
    WindowFull,  // 重排窗已满
    WindowClear, // 重置后仍有残留
    WrapDelta,   // 回绕差值错
    SeqBad,      // 序号或帧类型非法
    RingBound,   // 环形缓冲越过上限
    DropAccount, // 丢帧记账不平衡
    ReasonBad,   // 丢弃原因桶非法
    DigestBad,   // 摘要不符
    RenderBad,   // 账行渲染损坏
    FieldBad,    // 字段数或键序错位
    Incomplete,  // 处置链不完整
}

impl OrdCode {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            OrdCode::TsDuplicate => "窗内同序号或同时间戳帧被拒收",
            OrdCode::TsStale => "时间戳早于已发水位，判为迟到帧",
            OrdCode::TsOrder => "吐出序列出现非单调步进",
            OrdCode::WindowFull => "重排窗 32 槽已满，拒绝继续插入",
            OrdCode::WindowClear => "重置后窗口仍有残留或桶未清零",
            OrdCode::WrapDelta => "u32 节拍回绕差值符号或幅值错",
            OrdCode::SeqBad => "帧序号为 0 或越出 1..=0x000F_FFFF",
            OrdCode::RingBound => "环形缓冲保留量越过 4096 上限",
            OrdCode::DropAccount => "入口量不等于出口量加残留加丢弃",
            OrdCode::ReasonBad => "丢弃原因桶缺项或桶和与丢弃量不符",
            OrdCode::DigestBad => "账行摘要与重算不一致",
            OrdCode::RenderBad => "账行渲染损坏或键序位移",
            OrdCode::FieldBad => "账行字段数或缺键",
            OrdCode::Incomplete => "乱序或溢出处置链未跑满全程",
        }
    }

    /// 面向运营的短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            OrdCode::TsDuplicate => "重复帧：请核对序号与节拍",
            OrdCode::TsStale => "迟到帧：该节拍已过水位",
            OrdCode::TsOrder => "顺序崩：请重跑重排窗",
            OrdCode::WindowFull => "窗口满：请先释放水位",
            OrdCode::WindowClear => "重置脏：窗口未清空",
            OrdCode::WrapDelta => "回绕错：请用有符号差值",
            OrdCode::SeqBad => "序号坏：请回到 1..=0xFFFFF",
            OrdCode::RingBound => "环越界：请先出队再入队",
            OrdCode::DropAccount => "账不平：请补丢帧桶",
            OrdCode::ReasonBad => "原因缺：请补四桶明细",
            OrdCode::DigestBad => "摘要失配：请重算账行",
            OrdCode::RenderBad => "账行坏：请重渲染",
            OrdCode::FieldBad => "字段错位：请核对键序",
            OrdCode::Incomplete => "链路不全：请跑满十轮",
        }
    }
}

/// 全部诊断码（顺序即值序）。
pub const ALL: [OrdCode; 14] = [
    OrdCode::TsDuplicate, OrdCode::TsStale,
    OrdCode::TsOrder, OrdCode::WindowFull,
    OrdCode::WindowClear, OrdCode::WrapDelta,
    OrdCode::SeqBad, OrdCode::RingBound,
    OrdCode::DropAccount, OrdCode::ReasonBad,
    OrdCode::DigestBad, OrdCode::RenderBad,
    OrdCode::FieldBad, OrdCode::Incomplete,
];

/// 自由函数：码值 → 说明（段外一律 `UNKNOWN`）。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => OrdCode::TsDuplicate.detail(),
        2 => OrdCode::TsStale.detail(),
        3 => OrdCode::TsOrder.detail(),
        4 => OrdCode::WindowFull.detail(),
        5 => OrdCode::WindowClear.detail(),
        6 => OrdCode::WrapDelta.detail(),
        7 => OrdCode::SeqBad.detail(),
        8 => OrdCode::RingBound.detail(),
        9 => OrdCode::DropAccount.detail(),
        10 => OrdCode::ReasonBad.detail(),
        11 => OrdCode::DigestBad.detail(),
        12 => OrdCode::RenderBad.detail(),
        13 => OrdCode::FieldBad.detail(),
        14 => OrdCode::Incomplete.detail(),
        _ => UNKNOWN,
    }
}

/// 段位自检：14 码值两两不同且全部落在本段窗口内。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ALL.len() {
        let a = ALL[i].val();
        if a <= SEG_BASE || a > SEG_BASE + 0x0F {
            return false;
        }
        let mut j = i + 1;
        while j < ALL.len() {
            if ALL[j].val() == a {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 节拍语义（回绕安全） ─────────────────────────────────────────
/// 有符号差值 `a - b`（跨回绕正确，前提是相距小于半空间）。
pub fn tick_delta(a: u32, b: u32) -> i32 {
    a.wrapping_sub(b) as i32
}

/// 三向比较：`a` 相对 `b` 的新旧（回绕安全）。
pub fn tick_cmp(a: u32, b: u32) -> i32 {
    tick_delta(a, b).signum()
}

/// `a` 严格晚于 `b`。
pub fn is_after(a: u32, b: u32) -> bool {
    tick_cmp(a, b) > 0
}
/// `a` 严格早于 `b`。
pub fn is_before(a: u32, b: u32) -> bool {
    tick_cmp(a, b) < 0
}
/// 两节拍同刻。
pub fn is_same_tick(a: u32, b: u32) -> bool {
    tick_cmp(a, b) == 0
}

/// 跨度是否落在半个回绕空间内（否则差值不可解释）。
pub fn within_half(a: u32, b: u32) -> bool {
    a.wrapping_sub(b) < TICK_HALF
}

/// 节拍跨度：`ts` 向前推进 `step`（回绕正确）。
pub fn advance(ts: u32, step: u32) -> u32 {
    ts.wrapping_add(step)
}

/// 列内节拍是否严格递增（回绕安全）。
pub fn strictly_monotonic(seq: &[u32]) -> bool {
    let mut i = 1usize;
    while let (Some(prev), Some(cur)) = (seq.get(i - 1), seq.get(i)) {
        if !is_after(*cur, *prev) {
            return false;
        }
        i += 1;
    }
    true
}

// ── 帧与重排窗 ───────────────────────────────────────────────────
/// 一帧：节拍 + 序号 + 类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Frame {
    pub ts: u32,
    pub seq: u32,
    pub kind: u8,
}

impl Frame {
    /// 构造并校验；非法返回 `None`。
    pub fn new(ts: u32, seq: u32, kind: u8) -> Option<Frame> {
        if kind > KIND_MAX || seq < SEQ_MIN || seq > SEQ_MAX {
            return None;
        }
        Some(Frame { ts, seq, kind })
    }

}

/// 插入结果（拒收必须显性落桶）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PushOutcome {
    Accepted,  // 已插入
    Duplicate, // 窗内重复
    Stale,     // 低于水位
    Full,      // 窗满
    SeqBad,    // 序号或类型非法
}

/// 有界重排窗：有序插入、按水位整批吐出、可整轮重置。
pub struct ReorderWindow {
    slots: Vec<Frame>,
    watermark: u32,
    have_watermark: bool,
    /// 接受量。
    pub accepted: u32,
    /// 吐出量。
    pub emitted: u32,
    /// 重复拒收桶。
    pub duplicates: u32,
    /// 迟到拒收桶。
    pub stale: u32,
    /// 满窗拒收桶。
    pub full_rejects: u32,
    /// 序号非法桶。
    pub bad_seq: u32,
    /// 重置轮数。
    pub resets: u32,
}

impl ReorderWindow {
    /// 新建空窗（容量 32）。
    pub fn new() -> ReorderWindow {
        ReorderWindow {
            slots: Vec::with_capacity(WINDOW),
            watermark: 0,
            have_watermark: false,
            accepted: 0,
            emitted: 0,
            duplicates: 0,
            stale: 0,
            full_rejects: 0,
            bad_seq: 0,
            resets: 0,
        }
    }

    /// 窗内帧数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 是否空窗。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 当前水位（未吐出过任何帧为 `None`）。
    pub fn watermark(&self) -> Option<u32> {
        if self.have_watermark {
            Some(self.watermark)
        } else {
            None
        }
    }

    /// 入口量：全部提交帧。
    pub fn offered(&self) -> u32 {
        self.accepted
            .saturating_add(self.duplicates)
            .saturating_add(self.stale)
            .saturating_add(self.full_rejects)
            .saturating_add(self.bad_seq)
    }

    /// 丢弃量：四桶之和。
    pub fn dropped(&self) -> u32 {
        self.duplicates
            .saturating_add(self.stale)
            .saturating_add(self.full_rejects)
            .saturating_add(self.bad_seq)
    }

    /// 记账恒等式：入口 = 吐出 + 残留 + 丢弃。
    pub fn balanced(&self) -> bool {
        (self.offered() as i64) == self.emitted as i64 + self.len() as i64 + self.dropped() as i64
            && self.len() <= WINDOW
    }

    /// 残留量：重置后必须为 0。
    pub fn residue(&self) -> u32 {
        (self.len() as u32)
            .saturating_add(self.emitted)
            .saturating_add(self.dropped())
    }

    /// 逐帧插入：序号 → 重复 → 迟到 → 满窗，四道闸顺序不可换。
    pub fn push(&mut self, f: Frame) -> PushOutcome {
        if f.kind > KIND_MAX || f.seq < SEQ_MIN || f.seq > SEQ_MAX {
            self.bad_seq = self.bad_seq.saturating_add(1);
            return PushOutcome::SeqBad;
        }
        let mut i = 0usize;
        while let Some(slot) = self.slots.get(i) {
            if slot.seq == f.seq || is_same_tick(slot.ts, f.ts) {
                self.duplicates = self.duplicates.saturating_add(1);
                return PushOutcome::Duplicate;
            }
            i += 1;
        }
        if self.have_watermark && !is_after(f.ts, self.watermark) {
            self.stale = self.stale.saturating_add(1);
            return PushOutcome::Stale;
        }
        if self.slots.len() >= WINDOW {
            self.full_rejects = self.full_rejects.saturating_add(1);
            return PushOutcome::Full;
        }
        self.slots.push(f);
        self.accepted = self.accepted.saturating_add(1);
        PushOutcome::Accepted
    }

    /// 窗内最小帧下标（按回绕安全的节拍序）。
    fn min_index(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        let mut i = 0usize;
        while let Some(f) = self.slots.get(i) {
            let take = match best {
                Some(b) => match self.slots.get(b) {
                    Some(cur) => is_before(f.ts, cur.ts),
                    None => true,
                },
                None => true,
            };
            if take {
                best = Some(i);
            }
            i += 1;
        }
        best
    }

    /// 取走指定下标的帧（越界返回 `None`，安全取出）。
    fn take_at(&mut self, idx: usize) -> Option<Frame> {
        if idx >= self.slots.len() {
            return None;
        }
        let f = *self.slots.get(idx)?;
        self.slots.remove(idx);
        Some(f)
    }

    /// 吐出最小帧并把水位推到该帧。
    fn emit_min(&mut self) -> Option<Frame> {
        let idx = self.min_index()?;
        let f = self.take_at(idx)?;
        self.watermark = f.ts;
        self.have_watermark = true;
        self.emitted = self.emitted.saturating_add(1);
        Some(f)
    }

    /// 释放水位：吐出所有不晚于 `horizon` 的帧。
    pub fn drain_ready(&mut self, horizon: u32) -> Vec<Frame> {
        let mut out = Vec::new();
        loop {
            let head = match self.min_index().and_then(|i| self.slots.get(i)) {
                Some(f) => *f,
                None => break,
            };
            if is_after(head.ts, horizon) {
                break;
            }
            match self.emit_min() {
                Some(f) => out.push(f),
                None => break,
            }
        }
        out
    }

    /// 全部吐出（重置前的收尾）。
    pub fn drain_all(&mut self) -> Vec<Frame> {
        let mut out = Vec::new();
        while let Some(f) = self.emit_min() {
            out.push(f);
        }
        out
    }

    /// 强制腾位：吐出最小帧（窗满时先腾位再重试插入）。
    pub fn evict_min(&mut self) -> Option<Frame> {
        self.emit_min()
    }

    /// 整轮清窗：残留、四桶、水位一起归零，只累加重置轮数。
    pub fn reset(&mut self) {
        self.slots.clear();
        self.watermark = 0;
        self.have_watermark = false;
        self.accepted = 0;
        self.emitted = 0;
        self.duplicates = 0;
        self.stale = 0;
        self.full_rejects = 0;
        self.bad_seq = 0;
        self.resets = self.resets.saturating_add(1);
    }

    /// 抽取帧列里的节拍序列。
    pub fn emitted_ticks(frames: &[Frame]) -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while let Some(f) = frames.get(i) {
            out.push(f.ts);
            i += 1;
        }
        out
    }
}

// ── 环形缓冲与丢弃桶 ─────────────────────────────────────────────
/// 丢弃原因四桶。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropReason {
    Full,      // 满溢（4096 上限）
    Late,      // 迟到（低于水位）
    Duplicate, // 重复
    Barrier,   // 关闸（重置/停机）
}

impl DropReason {
    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            DropReason::Full => "full",
            DropReason::Late => "late",
            DropReason::Duplicate => "dup",
            DropReason::Barrier => "barrier",
        }
    }

    /// 桶下标。
    pub fn index(self) -> usize {
        self as usize
    }

    /// 反查桶。
    pub fn from_tag(tag: &str) -> Option<DropReason> {
        match tag {
            "full" => Some(DropReason::Full),
            "late" => Some(DropReason::Late),
            "dup" => Some(DropReason::Duplicate),
            "barrier" => Some(DropReason::Barrier),
            _ => None,
        }
    }
}

/// 全部丢弃原因（顺序即桶序）。
pub const DROP_REASONS: [DropReason; REASONS] = [
    DropReason::Full,
    DropReason::Late,
    DropReason::Duplicate,
    DropReason::Barrier,
];

/// 有界环形缓冲：容量 4096，溢出显性计数，不静默丢帧。
pub struct BoundedRing {
    buf: Vec<u32>,
    head: usize,
    /// 入口量。
    pub pushed: u32,
    /// 出口量。
    pub delivered: u32,
    drops: [u32; REASONS],
}

impl BoundedRing {
    /// 新建空环，容量按 `RING_CAP` 预留。
    pub fn new() -> BoundedRing {
        BoundedRing {
            buf: Vec::with_capacity(RING_CAP),
            head: 0,
            pushed: 0,
            delivered: 0,
            drops: [0u32; REASONS],
        }
    }

    /// 保留量（未出队帧数），恒 ≤ 4096。
    pub fn retained(&self) -> u32 {
        self.buf.len().saturating_sub(self.head) as u32
    }

    /// 是否空环。
    pub fn is_empty(&self) -> bool {
        self.retained() == 0
    }

    /// 指定桶的丢弃量。
    pub fn drop_for(&self, r: DropReason) -> u32 {
        match self.drops.get(r.index()) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 丢弃总量。
    pub fn dropped(&self) -> u32 {
        bucket_sum(&self.drops)
    }

    /// 记账恒等式：入口 = 出口 + 保留 + 丢弃。
    pub fn balanced(&self) -> bool {
        (self.pushed as i64)
            == self.delivered as i64 + self.retained() as i64 + self.dropped() as i64
            && self.retained() <= RING_CAP as u32
    }

    /// 入队：满则落满溢桶并返回 `false`。
    pub fn push(&mut self, ts: u32) -> bool {
        self.pushed = self.pushed.saturating_add(1);
        if self.retained() as usize >= RING_CAP {
            self.bump(DropReason::Full);
            return false;
        }
        self.buf.push(ts);
        true
    }

    /// 显式丢弃：帧未进环也要记入口量，否则等式会把丢弃量藏起来。
    pub fn drop_explicit(&mut self, r: DropReason) -> bool {
        if DropReason::from_tag(r.tag()).is_none() || r.index() >= REASONS {
            return false;
        }
        self.pushed = self.pushed.saturating_add(1);
        self.bump(r);
        true
    }

    fn bump(&mut self, r: DropReason) {
        if let Some(slot) = self.drops.get_mut(r.index()) {
            *slot = slot.saturating_add(1);
        }
    }

    /// 出队一帧；空环返回 `None`。
    pub fn pop(&mut self) -> Option<u32> {
        let v = match self.buf.get(self.head) {
            Some(x) => *x,
            None => return None,
        };
        self.head = self.head.saturating_add(1);
        self.delivered = self.delivered.saturating_add(1);
        if self.head >= RING_CAP {
            self.compact();
        }
        Some(v)
    }

    /// 压缩已出队前缀（每 4096 次出队一次）。
    fn compact(&mut self) {
        if self.head == 0 {
            return;
        }
        self.buf.drain(..self.head);
        self.head = 0;
    }

    /// 回退：全部出队，返回出队帧数。
    pub fn drain_all(&mut self) -> u32 {
        let mut n = 0u32;
        while self.pop().is_some() {
            n = n.saturating_add(1);
        }
        n
    }

    /// 整轮清空：保留帧全部落关闸桶，随后计数器归零。
    pub fn reset(&mut self) {
        let live = self.retained();
        let mut i = 0u32;
        while i < live {
            self.bump(DropReason::Barrier);
            i = i.saturating_add(1);
        }
        self.buf.clear();
        self.head = 0;
        self.pushed = 0;
        self.delivered = 0;
        self.drops = [0u32; REASONS];
    }
}

/// 收桶总和。
pub fn bucket_sum(buckets: &[u32; REASONS]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while let Some(v) = buckets.get(i) {
        n = n.saturating_add(*v);
        i += 1;
    }
    n
}

/// 单桶取值（越界按 0，安全）。
pub fn bucket(buckets: &[u32; REASONS], idx: usize) -> u32 {
    match buckets.get(idx) {
        Some(v) => *v,
        None => 0,
    }
}

// ── 账行模型 ─────────────────────────────────────────────────────
/// 一条记账记录（槽位 + 四项计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Record {
    pub slot: u8, pub pushed: u32, pub delivered: u32,
    pub retained: u32, pub dropped: u32,
}

impl Record {
    /// 构造并做等式校验；不平的记录返回 `None`。
    pub fn new(slot: u8, pushed: u32, delivered: u32, retained: u32, dropped: u32) -> Option<Record> {
        if slot < 1 || slot > ROW_SLOTS as u8 {
            return None;
        }
        let rec = Record { slot, pushed, delivered, retained, dropped };
        if !rec.balanced() {
            return None;
        }
        Some(rec)
    }

    /// 等式自检。
    pub fn balanced(&self) -> bool {
        (self.delivered as i64 + self.retained as i64 + self.dropped as i64) == self.pushed as i64
    }
}

/// 账行摘要：槽位 + 四项计数（纯 FNV 折叠，无浮点、无分配）。
pub fn record_digest(slot: u8, pushed: u32, delivered: u32, retained: u32, dropped: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, DIGEST_TAG);
    h = p_core::fold_byte(h, slot);
    h = p_core::fold_u32(h, pushed);
    h = p_core::fold_u32(h, delivered);
    h = p_core::fold_u32(h, retained);
    h = p_core::fold_u32(h, dropped);
    h
}

/// 解析十进制（拒绝空串、非数字、溢出）。
pub fn parse_u32(s: &str) -> Option<u32> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut acc = 0u32;
    let mut i = 0usize;
    while let Some(b) = bytes.get(i) {
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?;
        acc = acc.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

/// 解析 16 位十六进制摘要（长度必须恰为 16）。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let bytes = s.as_bytes();
    let mut acc = 0u64;
    let mut i = 0usize;
    while let Some(b) = bytes.get(i) {
        let v = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        acc = acc.checked_mul(16)?;
        acc = acc.checked_add(v)?;
        i += 1;
    }
    Some(acc)
}

/// 按固定标签与键序拆行；返回各键值与摘要值。
pub fn row_fields<'a>(row: &'a str, tag: &str, keys: &[&str]) -> Option<Vec<&'a str>> {
    let parts: Vec<&str> = row.split('|').collect();
    if parts.len() != keys.len() + 2 || parts.first() != Some(&tag) {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < keys.len() {
        let piece = parts.get(i + 1)?;
        let key = keys.get(i)?;
        let value = piece.strip_prefix(key)?.strip_prefix('=')?;
        if value.is_empty() {
            return None;
        }
        out.push(value);
        i += 1;
    }
    let d = parts.get(keys.len() + 1)?.strip_prefix("digest=")?;
    if d.len() != 16 {
        return None;
    }
    out.push(d);
    Some(out)
}

/// 渲染账行：`ORDR83|slot=|pushed=|delivered=|retained=|dropped=|digest=`。
pub fn render_row(r: &Record) -> String {
    format!(
        "{}|slot={}|pushed={}|delivered={}|retained={}|dropped={}|digest={:016x}",
        ROW_TAG,
        r.slot,
        r.pushed,
        r.delivered,
        r.retained,
        r.dropped,
        record_digest(r.slot, r.pushed, r.delivered, r.retained, r.dropped)
    )
}

/// 账行回读：键序、等式、摘要三重校验。
pub fn audit_row(row: &str) -> bool {
    let f = match row_fields(row, ROW_TAG, &ROW_KEYS) {
        Some(v) if v.len() == ROW_KEYS.len() + 1 => v,
        _ => return false,
    };
    let slot = match f.first().copied().and_then(parse_u32) {
        Some(v) if v >= 1 && v <= ROW_SLOTS as u32 => v as u8,
        _ => return false,
    };
    let mut nums = [0u32; 4];
    let mut i = 0usize;
    while i < nums.len() {
        let parsed = match f.get(i + 1).copied().and_then(parse_u32) {
            Some(v) => v,
            None => return false,
        };
        if let Some(cell) = nums.get_mut(i) {
            *cell = parsed;
        }
        i += 1;
    }
    let (pushed, delivered, retained, dropped) =
        match (nums.first(), nums.get(1), nums.get(2), nums.get(3)) {
            (Some(p), Some(d), Some(r), Some(x)) => (*p, *d, *r, *x),
            _ => return false,
        };
    if retained > RING_CAP as u32 {
        return false;
    }
    if delivered as i64 + retained as i64 + dropped as i64 != pushed as i64 {
        return false;
    }
    match f.get(5).copied().and_then(parse_hex16) {
        Some(want) => record_digest(slot, pushed, delivered, retained, dropped) == want,
        None => false,
    }
}

// ── 场景：重排窗 ─────────────────────────────────────────────────
/// 乱序脚本（节拍, 序号, 类型）；第 5 条与第 2 条同节拍，用于重复拒收。
pub const SCRIPT: [(u32, u32, u8); SCRIPT_LEN] = [
    (1004, 1, 0), (1000, 2, 0),
    (1016, 3, 1), (1008, 4, 0),
    (1000, 5, 0), (1002, 6, 0),
    (1012, 7, 0), (1020, 8, 1),
    (1014, 9, 0), (1006, 10, 0),
    (1024, 11, 1), (1010, 12, 0),
    (1028, 13, 1), (1018, 14, 0),
    (1022, 15, 0), (1030, 16, 1),
    (1026, 17, 0), (1032, 18, 1),
    (1034, 19, 0), (1036, 20, 1),
    (1038, 21, 0), (1040, 22, 1),
    (1042, 23, 0), (1044, 24, 1),
];

/// 重排窗场景报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WindowReport {
    pub offered: u32, pub accepted: u32, pub emitted: u32,
    pub duplicates: u32, pub stale: u32, pub full: u32,
    pub retained: u32, pub monotonic: bool,
    pub first_tick: u32, pub last_tick: u32, pub balanced: bool,
}

/// 按脚本下标喂一帧；返回插入结果。
fn feed_script(w: &mut ReorderWindow, idx: usize) -> Option<PushOutcome> {
    let t = SCRIPT.get(idx)?;
    let f = Frame::new(t.0, t.1, t.2)?;
    Some(w.push(f))
}

/// 跑一遍完整脚本：乱序插入 → 水位释放 → 迟到/重复探针 → 收尾全吐。
pub fn window_scenario() -> WindowReport {
    let mut w = ReorderWindow::new();
    let mut i = 0usize;
    while i < PHASE_A_LEN {
        let _ = feed_script(&mut w, i);
        i += 1;
    }
    let mut emitted = w.drain_ready(PHASE_A_HORIZON);
    // 两帧迟到（早于水位 1020）+ 两帧窗内重复（同节拍 1024 / 同序号 13）。
    for (ts, seq) in [(1004u32, 60u32), (1008, 61), (1024, 62), (1031, 13)] {
        if let Some(f) = Frame::new(ts, seq, 0) {
            let _ = w.push(f);
        }
    }
    let mut j = PHASE_C_START;
    while j < SCRIPT_LEN {
        let _ = feed_script(&mut w, j);
        j += 1;
    }
    emitted.extend(w.drain_all());
    let ticks = ReorderWindow::emitted_ticks(&emitted);
    WindowReport {
        offered: w.offered(),
        accepted: w.accepted,
        emitted: w.emitted,
        duplicates: w.duplicates,
        stale: w.stale,
        full: w.full_rejects,
        retained: w.len() as u32,
        monotonic: strictly_monotonic(&ticks),
        first_tick: match ticks.first() { Some(v) => *v, None => 0 },
        last_tick: match ticks.last() { Some(v) => *v, None => 0 },
        balanced: w.balanced(),
    }
}

/// 拒收探针：重复（同序号/同刻）、迟到（低于水位/等于水位）、序号非法。
pub fn rejection_probe() -> bool {
    let mut w = ReorderWindow::new();
    let Some(f1) = Frame::new(100, 1, 0) else { return false };
    let Some(f2) = Frame::new(100, 2, 0) else { return false };
    let Some(f3) = Frame::new(200, 3, 0) else { return false };
    let Some(late) = Frame::new(150, 4, 0) else { return false };
    let Some(same) = Frame::new(200, 5, 0) else { return false };
    if w.push(f1) != PushOutcome::Accepted || w.push(f1) != PushOutcome::Duplicate {
        return false;
    }
    if w.push(f2) != PushOutcome::Duplicate || w.push(f3) != PushOutcome::Accepted {
        return false;
    }
    if w.drain_ready(200).len() != 2 {
        return false;
    }
    if w.push(late) != PushOutcome::Stale || w.push(same) != PushOutcome::Stale {
        return false;
    }
    let bad_seq = Frame { ts: 300, seq: 0, kind: 0 };
    let bad_kind = Frame { ts: 301, seq: 6, kind: 9 };
    if w.push(bad_seq) != PushOutcome::SeqBad || w.push(bad_kind) != PushOutcome::SeqBad {
        return false;
    }
    w.duplicates == 2 && w.stale == 2 && w.bad_seq == 2 && w.balanced()
}

/// 重置总报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResetReport {
    pub rounds: u32, pub emitted: u32, pub residue_total: u32,
    pub resets: u32, pub all_clear: bool, pub digest: u64,
}

/// 重置 ×10：每轮装填 → 全吐 → 清窗，残留必须为 0。
pub fn reset_cycles() -> ResetReport {
    let offsets: [u32; 8] = [6, 0, 4, 2, 7, 1, 5, 3];
    let mut w = ReorderWindow::new();
    let mut rounds = 0u32;
    let mut emitted_total = 0u32;
    let mut residue_total = 0u32;
    let mut all_clear = true;
    let mut digest = p_core::FNV_OFF;
    while rounds < RESET_ROUNDS {
        let base = 500u32.saturating_add(rounds.saturating_mul(100));
        let mut j = 0u32;
        while j < RESET_FRAMES {
            let off = match offsets.get(j as usize) {
                Some(v) => *v,
                None => 0,
            };
            let ts = base.saturating_add(off.saturating_mul(4));
            if let Some(f) = Frame::new(ts, j.saturating_add(1), (j % 4) as u8) {
                let _ = w.push(f);
            }
            j = j.saturating_add(1);
        }
        let emitted = w.drain_all().len() as u32;
        let residue_before = w.residue();
        w.reset();
        let cleared = w.is_empty() && w.watermark().is_none();
        let residue = w.residue();
        if residue != 0 || residue_before != emitted || !cleared {
            all_clear = false;
        }
        emitted_total = emitted_total.saturating_add(emitted);
        residue_total = residue_total.saturating_add(residue);
        digest = p_core::fold_u32(digest, emitted);
        digest = p_core::fold_u32(digest, residue);
        rounds = rounds.saturating_add(1);
    }
    digest = p_core::fold_u32(digest, w.resets);
    ResetReport {
        rounds,
        emitted: emitted_total,
        residue_total,
        resets: w.resets,
        all_clear,
        digest,
    }
}

// ── 场景：环形缓冲 ───────────────────────────────────────────────
/// 环形溢出报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingReport {
    pub pushed: u32, pub delivered: u32, pub retained: u32, pub dropped: u32,
    pub by_reason: [u32; REASONS],
    pub balanced: bool, pub over_cap: bool,
}

/// 4096 溢出场景：写满 → 溢出 → 出队 → 回补 → 显式丢弃 → 全吐。
pub fn ring_overflow_scenario() -> RingReport {
    let mut ring = BoundedRing::new();
    let mut over_cap = false;
    let mut i = 0u32;
    while i < RING_CAP as u32 {
        over_cap = over_cap || !ring.push(100u32.saturating_add(i)) || ring.retained() > RING_CAP as u32;
        i = i.saturating_add(1);
    }
    let mut k = 0u32;
    while k < RING_OVERFLOW {
        over_cap = over_cap || ring.push(9000u32.saturating_add(k));
        k = k.saturating_add(1);
    }
    let mut drained = 0u32;
    while drained < RING_DRAIN && ring.pop().is_some() {
        drained = drained.saturating_add(1);
    }
    let mut r = 0u32;
    while r < RING_REFILL {
        over_cap = over_cap || !ring.push(20_000u32.saturating_add(r));
        r = r.saturating_add(1);
    }
    let tagged: [(DropReason, u32); 3] = [
        (DropReason::Late, LATE_DROPS),
        (DropReason::Duplicate, DUP_DROPS),
        (DropReason::Barrier, BARRIER_DROPS),
    ];
    for (reason, n) in tagged {
        let mut t = 0u32;
        while t < n {
            let _ = ring.drop_explicit(reason);
            t = t.saturating_add(1);
        }
    }
    ring.drain_all();
    let mut by_reason = [0u32; REASONS];
    let mut idx = 0usize;
    while idx < REASONS {
        if let (Some(reason), Some(slot)) = (DROP_REASONS.get(idx), by_reason.get_mut(idx)) {
            *slot = ring.drop_for(*reason);
        }
        idx += 1;
    }
    RingReport {
        pushed: ring.pushed,
        delivered: ring.delivered,
        retained: ring.retained(),
        dropped: ring.dropped(),
        by_reason,
        balanced: ring.balanced() && ring.is_empty(),
        over_cap,
    }
}

/// 复跑报告（×10 记账 / ×10 回退共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReplayReport {
    pub rounds: u32, pub pushed: u32, pub delivered: u32, pub dropped: u32,
    pub all_balanced: bool, pub all_empty: bool, pub digest: u64,
}

impl ReplayReport {
    /// 逐轮等式与总量双检。
    pub fn ok(&self) -> bool {
        self.rounds == RESET_ROUNDS && self.all_balanced && self.all_empty
    }
}

/// 溢出记账 ×10（`UNX-F49084-J1`）：每轮跑满 4096 上限，等式逐轮成立。
pub fn replay_cycles() -> ReplayReport {
    let mut rounds = 0u32;
    let mut pushed = 0u32;
    let mut delivered = 0u32;
    let mut dropped = 0u32;
    let mut all_balanced = true;
    let mut digest = p_core::FNV_OFF;
    while rounds < RESET_ROUNDS {
        let r = ring_overflow_scenario();
        if !r.balanced || r.retained != 0 || bucket_sum(&r.by_reason) != r.dropped {
            all_balanced = false;
        }
        pushed = pushed.saturating_add(r.pushed);
        delivered = delivered.saturating_add(r.delivered);
        dropped = dropped.saturating_add(r.dropped);
        digest = p_core::fold_u32(digest, r.pushed);
        digest = p_core::fold_u32(digest, r.delivered);
        digest = p_core::fold_u32(digest, r.dropped);
        rounds = rounds.saturating_add(1);
    }
    let want_pushed = (RING_CAP as u32)
        .saturating_add(RING_OVERFLOW)
        .saturating_add(RING_REFILL)
        .saturating_add(RING_TAG_DROPS)
        .saturating_mul(RESET_ROUNDS);
    let want_delivered = (RING_CAP as u32)
        .saturating_add(RING_REFILL)
        .saturating_mul(RESET_ROUNDS);
    let ok = pushed == want_pushed && delivered == want_delivered;
    ReplayReport {
        rounds,
        pushed,
        delivered,
        dropped,
        all_balanced: all_balanced && ok,
        all_empty: true,
        digest,
    }
}

/// 回退 ×10（`UNX-F49085-J1`）：填满 → 溢出 → 全吐回空环，逐轮核对。
pub fn rollback_cycles() -> ReplayReport {
    let mut rounds = 0u32;
    let mut pushed = 0u32;
    let mut delivered = 0u32;
    let mut dropped = 0u32;
    let mut all_balanced = true;
    let mut all_empty = true;
    let mut digest = p_core::FNV_OFF;
    while rounds < RESET_ROUNDS {
        let mut ring = BoundedRing::new();
        let mut i = 0u32;
        while i < RING_CAP as u32 {
            let _ = ring.push(1000u32.saturating_add(i));
            i = i.saturating_add(1);
        }
        let mut k = 0u32;
        while k < ROLLBACK_OVERFLOW {
            let _ = ring.push(50_000u32.saturating_add(k));
            k = k.saturating_add(1);
        }
        let out = ring.drain_all();
        let empty = ring.is_empty() && ring.retained() == 0;
        if !ring.balanced() || !empty || out != RING_CAP as u32 {
            all_balanced = false;
        }
        if !empty {
            all_empty = false;
        }
        pushed = pushed.saturating_add(ring.pushed);
        delivered = delivered.saturating_add(ring.delivered);
        dropped = dropped.saturating_add(ring.dropped());
        digest = p_core::fold_u32(digest, out);
        digest = p_core::fold_u32(digest, ring.dropped());
        rounds = rounds.saturating_add(1);
    }
    let cap = RING_CAP as u32;
    let ok = pushed == cap.saturating_add(ROLLBACK_OVERFLOW).saturating_mul(RESET_ROUNDS)
        && delivered == cap.saturating_mul(RESET_ROUNDS)
        && dropped == ROLLBACK_OVERFLOW.saturating_mul(RESET_ROUNDS);
    ReplayReport {
        rounds,
        pushed,
        delivered,
        dropped,
        all_balanced: all_balanced && ok,
        all_empty,
        digest,
    }
}

// ── 账行导出 ─────────────────────────────────────────────────────
/// 四槽记录：重排窗 / 重置链 / 4096 溢出 / 回退链。
pub fn records() -> Vec<Record> {
    let mut out = Vec::new();
    let w = window_scenario();
    let win_dropped = w.offered.saturating_sub(w.emitted).saturating_sub(w.retained);
    if let Some(r) = Record::new(1, w.offered, w.emitted, w.retained, win_dropped) {
        out.push(r);
    }
    let reset = reset_cycles();
    if let Some(r) = Record::new(2, reset.emitted, reset.emitted, reset.residue_total, 0) {
        out.push(r);
    }
    let ring = ring_overflow_scenario();
    if let Some(r) = Record::new(3, ring.pushed, ring.delivered, ring.retained, ring.dropped) {
        out.push(r);
    }
    let roll = rollback_cycles();
    if let Some(r) = Record::new(4, roll.pushed, roll.delivered, 0, roll.dropped) {
        out.push(r);
    }
    out
}

/// 导出账行。
pub fn export_rows() -> Vec<String> {
    let recs = records();
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(r) = recs.get(i) {
        out.push(render_row(r));
        i += 1;
    }
    out
}

/// 账行稳定性：两次导出逐行相同、槽位互异、且每行可回读。
pub fn rows_stable() -> bool {
    let a = export_rows();
    let b = export_rows();
    if a.len() != ROW_SLOTS || b.len() != ROW_SLOTS {
        return false;
    }
    let mut i = 0usize;
    while let Some(x) = a.get(i) {
        match b.get(i) {
            Some(y) if x == y => {}
            _ => return false,
        }
        if !audit_row(x) {
            return false;
        }
        let mut j = i + 1;
        while let Some(z) = a.get(j) {
            if z == x {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 判据 ─────────────────────────────────────────────────────────
/// 篡改探针：把账行最后一个字符改掉（摘要末位必变）。
pub fn tamper_tail(row: &str) -> String {
    let mut out = String::new();
    let bytes = row.as_bytes();
    let mut i = 0usize;
    while let Some(b) = bytes.get(i) {
        if i + 1 == bytes.len() {
            out.push(if *b == b'0' { '1' } else { '0' });
        } else {
            out.push(*b as char);
        }
        i += 1;
    }
    out
}

/// 八项判据（4 × F49083 + 4 × F49084），每条都跑真实场景。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();
    // 1) 窗内重排：接受量 = 吐出量，吐出严格单调，等式成立。
    let w = window_scenario();
    let ok = w.balanced && w.retained == 0 && w.emitted == w.accepted && w.monotonic
        && w.first_tick == 1000 && w.last_tick == 1044 && w.full == 0;
    out.push((
        "F49083/窗内重排单调",
        ok,
        format!("of={} acc={} emit={} mono={}", w.offered, w.accepted, w.emitted, w.monotonic),
    ));

    // 2) 重复与迟到拒收：桶数对得上，探针全绿。
    let p = rejection_probe();
    let ok = p && w.duplicates == 3 && w.stale == 2
        && w.offered == w.accepted + w.duplicates + w.stale;
    out.push((
        "F49083/重复与迟到拒收",
        ok,
        format!("dup={} stale={} probe={} of={} acc={}", w.duplicates, w.stale, p, w.offered, w.accepted),
    ));

    // 3) 回绕差值：跨 2^32 的差值符号与幅值都对，窗口跨回绕仍单调。
    let fwd = tick_delta(0x0000_0004, 0xFFFF_FFF8);
    let back = tick_delta(0xFFFF_FFF8, 0x0000_0004);
    let span_ok = within_half(0x0000_0020, 0xFFFF_FFF0) && !within_half(0xFFFF_FFFF, 0x0000_0001);
    let cmp_ok = is_after(0x0000_0020, 0xFFFF_FFF0) && is_before(0xFFFF_FFF0, 0x0000_0020);
    let mut wrap = ReorderWindow::new();
    let mut n = 1u32;
    for t in [0xFFFF_FFF8u32, 0x0000_0004, 0xFFFF_FFFC, 0x0000_0000] {
        if let Some(f) = Frame::new(t, n, 0) {
            let _ = wrap.push(f);
        }
        n += 1;
    }
    let ticks = ReorderWindow::emitted_ticks(&wrap.drain_all());
    let ok = fwd == 0xC && back == -0xC && span_ok && cmp_ok && wrap.balanced()
        && strictly_monotonic(&ticks) && ticks.len() == 4
        && advance(0xFFFF_FFFF, 1) == 0 && advance(0xFFFF_FFF8, 0xC) == 0x0000_0004;
    out.push((
        "F49083/回绕差值有符号",
        ok,
        format!("fwd={} back={} half={} mono={} n={}", fwd, back, span_ok, strictly_monotonic(&ticks), ticks.len()),
    ));

    // 4) 重置 ×10：每轮残留 0，摘要可复算。
    let r = reset_cycles();
    let r2 = reset_cycles();
    let ok = r.rounds == RESET_ROUNDS && r.residue_total == 0 && r.all_clear
        && r.resets == RESET_ROUNDS
        && r.emitted == RESET_ROUNDS.saturating_mul(RESET_FRAMES)
        && r.digest == r2.digest;
    out.push((
        "F49083/重置×10零残留",
        ok,
        format!("r={} res={} emit={} clr={} dg={:016x}", r.rounds, r.residue_total, r.emitted, r.all_clear, r.digest),
    ));

    // 5) 4096 溢出记账：上限不破，等式成立，丢弃量有账。
    let g = ring_overflow_scenario();
    let want_pushed = (RING_CAP as u32).saturating_add(RING_OVERFLOW)
        .saturating_add(RING_REFILL).saturating_add(RING_TAG_DROPS);
    let ok = g.balanced && !g.over_cap && g.retained == 0 && g.pushed == want_pushed
        && g.dropped == RING_OVERFLOW.saturating_add(RING_TAG_DROPS)
        && g.delivered == (RING_CAP as u32).saturating_add(RING_REFILL);
    out.push((
        "F49084/4096溢出记账",
        ok,
        format!("push={} del={} ret={} drop={} cap={}", g.pushed, g.delivered, g.retained, g.dropped, RING_CAP),
    ));

    // 6) 丢弃原因分桶：四桶齐全、桶和等于丢弃量、标签可反查。
    let mut tags_ok = true;
    let mut idx = 0usize;
    while let Some(reason) = DROP_REASONS.get(idx) {
        if DropReason::from_tag(reason.tag()) != Some(*reason) || reason.index() != idx {
            tags_ok = false;
        }
        if bucket(&g.by_reason, idx) == 0 {
            tags_ok = false;
        }
        idx += 1;
    }
    let ok = tags_ok && bucket_sum(&g.by_reason) == g.dropped
        && bucket(&g.by_reason, 0) == RING_OVERFLOW && bucket(&g.by_reason, 1) == LATE_DROPS
        && bucket(&g.by_reason, 2) == DUP_DROPS && bucket(&g.by_reason, 3) == BARRIER_DROPS;
    out.push((
        "F49084/丢弃原因分桶",
        ok,
        format!(
            "full={} late={} dup={} bar={} sum={}",
            bucket(&g.by_reason, 0),
            bucket(&g.by_reason, 1),
            bucket(&g.by_reason, 2),
            bucket(&g.by_reason, 3),
            bucket_sum(&g.by_reason)
        ),
    ));

    // 7) 账行回读：四槽、稳定、逐行可审计、篡改必判否。
    let rows = export_rows();
    let mut tamper_ok = true;
    let mut i = 0usize;
    while let Some(row) = rows.get(i) {
        if audit_row(&format!("{}0", row)) || audit_row(&tamper_tail(row)) {
            tamper_ok = false;
        }
        i += 1;
    }
    let first_ok = match rows.first() {
        Some(first) => audit_row(first),
        None => false,
    };
    let ok = rows.len() == ROW_SLOTS && rows_stable() && tamper_ok && first_ok;
    out.push((
        "F49084/账行回读防篡改",
        ok,
        format!("rows={} stable={} tamper={}", rows.len(), rows_stable(), tamper_ok),
    ));

    // 8) 回退 ×10：填满 → 溢出 → 全吐回空，逐轮等式成立。
    let rb = rollback_cycles();
    let rp = replay_cycles();
    let ok = rb.ok() && rp.ok()
        && rb.dropped == ROLLBACK_OVERFLOW.saturating_mul(RESET_ROUNDS)
        && rp.pushed == want_pushed.saturating_mul(RESET_ROUNDS);
    out.push((
        "F49084/回退×10全过",
        ok,
        format!(
            "rounds={} bal={} empty={} rb_drop={} rp_push={}",
            rb.rounds, rb.all_balanced, rb.all_empty, rb.dropped, rp.pushed
        ),
    ));

    out
}

/// 入口：八项判据写入 CheckSet。
pub fn run_unx49083_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49083 乱序与溢出处置");
    let checks = extra_checks();
    let mut i = 0usize;
    while i < checks.len() {
        s.add_dyn(checks[i].0, checks[i].1, &checks[i].2);
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t01_guard() { assert!(segment_guard()); assert_eq!((SEG_BASE, ALL.len(), ALL[13].val()), (0x73540, 14, 0x7354E)); assert!(ALL[13].val() < SEG_BASE + SEG_SPAN); }
    #[test]
    fn t02_values() { for i in 0..ALL.len() { assert_eq!(ALL[i].val(), SEG_BASE + 1 + i as u32); } }
    #[test]
    fn t03_texts() {
        for i in 0..ALL.len() { for j in 0..ALL.len() {
            assert!(!ALL[i].detail().is_empty() && !ALL[i].message().is_empty());
            assert_ne!(ALL[i].message(), ALL[i].detail());
            if i != j { assert_ne!(ALL[i].detail(), ALL[j].detail()); assert_ne!(ALL[i].message(), ALL[j].message()); }
        }}
        assert!(!UNKNOWN.is_empty() && UNKNOWN.starts_with("未知"));
    }
    #[test]
    fn t04_free_detail() {
        for c in ALL.iter() { assert_eq!(detail(c.val()), c.detail()); }
        assert_eq!((detail(0), detail(SEG_BASE), detail(SEG_BASE + 15), detail(u32::MAX)), (UNKNOWN, UNKNOWN, UNKNOWN, UNKNOWN));
    }
    #[test]
    fn t05_tick_delta() {
        assert_eq!((tick_delta(4, 0xFFFF_FFF8), tick_delta(0xFFFF_FFF8, 4), tick_delta(7, 7)), (12, -12, 0));
        assert_eq!((tick_cmp(4, 0xFFFF_FFF8), tick_cmp(0xFFFF_FFF8, 4), tick_cmp(9, 9)), (1, -1, 0));
    }
    #[test]
    fn t06_half_advance() {
        assert!(within_half(0x20, 0xFFFF_FFF0) && !within_half(0xFFFF_FFFF, 1) && within_half(0, 0xFFFF_FFFE) && !within_half(0, 0x8000_0000));
        assert_eq!((advance(0xFFFF_FFFF, 1), advance(0xFFFF_FFF8, 0xC), advance(10, 0)), (0, 4, 10));
    }
    #[test]
    fn t07_predicates() { assert!(is_after(5, 3) && is_before(3, 5) && is_same_tick(7, 7)); assert!(is_after(2, 0xFFFF_FFF0) && is_before(0xFFFF_FFF0, 2)); }
    #[test]
    fn t08_monotonic() {
        assert!(strictly_monotonic(&[]) && strictly_monotonic(&[5]) && strictly_monotonic(&[1, 2, 3]));
        assert!(!strictly_monotonic(&[1, 3, 2]) && !strictly_monotonic(&[2, 2]) && strictly_monotonic(&[0xFFFF_FFF0, 4]));
    }
    #[test]
    fn t09_frame() {
        assert!(Frame::new(0, 1, KIND_MAX).is_some() && Frame::new(0, SEQ_MAX, 0).is_some() && Frame::new(0, 0, 0).is_none());
        assert!(Frame::new(0, SEQ_MAX + 1, 0).is_none() && Frame::new(0, 1, KIND_MAX + 1).is_none());
    }
    #[test]
    fn t10_window_order() {
        let mut w = ReorderWindow::new();
        for (ts, seq) in [(30u32, 1u32), (10, 2), (20, 3)] { assert_eq!(w.push(Frame::new(ts, seq, 0).expect("f")), PushOutcome::Accepted); }
        let ticks = ReorderWindow::emitted_ticks(&w.drain_all());
        assert_eq!((ticks.get(0), ticks.get(1), ticks.get(2)), (Some(&10), Some(&20), Some(&30))); assert!(w.balanced() && w.is_empty());
    }
    #[test]
    fn t11_dup() {
        let mut w = ReorderWindow::new();
        assert_eq!(w.push(Frame::new(10, 1, 0).expect("a")), PushOutcome::Accepted);
        assert_eq!((w.push(Frame::new(20, 1, 0).expect("b")), w.push(Frame::new(10, 2, 0).expect("c"))), (PushOutcome::Duplicate, PushOutcome::Duplicate));
        assert_eq!(w.duplicates, 2); assert!(w.balanced());
    }
    #[test]
    fn t12_stale() {
        let mut w = ReorderWindow::new();
        assert_eq!(w.push(Frame::new(100, 1, 0).expect("a")), PushOutcome::Accepted);
        assert_eq!(w.drain_ready(100).len(), 1); assert_eq!(w.watermark(), Some(100));
        assert_eq!((w.push(Frame::new(100, 2, 0).expect("b")), w.push(Frame::new(99, 3, 0).expect("c"))), (PushOutcome::Stale, PushOutcome::Stale));
        assert_eq!(w.stale, 2); assert!(w.balanced());
    }
    #[test]
    fn t13_full_evict() {
        let mut w = ReorderWindow::new();
        for i in 0..WINDOW as u32 { assert_eq!(w.push(Frame::new(100 + i, i + 1, 0).expect("f")), PushOutcome::Accepted); }
        assert_eq!(w.push(Frame::new(500, 100, 0).expect("x")), PushOutcome::Full);
        assert_eq!(w.full_rejects, 1); assert!(w.balanced());
        let mut v = ReorderWindow::new();
        for i in 0..WINDOW as u32 { let _ = v.push(Frame::new(132 - i, i + 1, 0).expect("g")); }
        assert_eq!(v.evict_min().expect("min").ts, 101);
        assert_eq!(v.push(Frame::new(999, 77, 0).expect("n")), PushOutcome::Accepted); assert!(v.balanced());
    }
    #[test]
    fn t14_reset_window() {
        let mut w = ReorderWindow::new();
        for i in 1..=5u32 { let _ = w.push(Frame::new(i * 10, i, 0).expect("f")); }
        w.reset();
        assert!(w.is_empty() && w.balanced() && w.watermark().is_none()); assert_eq!((w.accepted, w.residue(), w.resets), (0, 0, 1));
    }
    #[test]
    fn t15_empty_window() {
        let mut w = ReorderWindow::new();
        assert!(w.is_empty() && w.balanced() && w.watermark().is_none());
        assert!(w.evict_min().is_none() && w.drain_all().is_empty() && w.drain_ready(10).is_empty()); assert_eq!((w.offered(), w.residue(), w.len()), (0, 0, 0));
    }
    #[test]
    fn t16_scenario() {
        let w = window_scenario();
        assert!(w.monotonic && w.balanced);
        assert_eq!((w.offered, w.accepted, w.emitted, w.retained), (28, 23, 23, 0)); assert_eq!((w.duplicates, w.stale, w.full), (3, 2, 0));
        assert_eq!((w.first_tick, w.last_tick), (1000, 1044)); assert_eq!(w.offered, w.accepted + w.duplicates + w.stale);
    }
    #[test]
    fn t17_rejection_probe() { assert!(rejection_probe()); }
    #[test]
    fn t18_emitted_ticks() {
        let frames = [Frame { ts: 5, seq: 1, kind: 0 }, Frame { ts: 9, seq: 2, kind: 1 }];
        let ticks = ReorderWindow::emitted_ticks(&frames);
        assert_eq!((ticks.len(), ticks.get(0), ticks.get(1)), (2, Some(&5), Some(&9)));
    }
    #[test]
    fn t19_reset_cycles() {
        let r = reset_cycles();
        assert_eq!((r.rounds, r.resets, r.emitted, r.residue_total), (10, 10, 80, 0)); assert!(r.all_clear); assert_eq!(r.digest, reset_cycles().digest);
    }
    #[test]
    fn t20_reset_residue() {
        let mut w = ReorderWindow::new();
        for round in 0..10u32 {
            for k in 1..=4u32 {
                if let Some(f) = Frame::new(round.saturating_mul(100).saturating_add(k), k, 0) { let _ = w.push(f); }
            }
            let _ = w.drain_all();
            assert_eq!((w.residue(), w.emitted), (4, 4)); w.reset();
            assert_eq!(w.residue(), 0); assert!(w.is_empty() && w.balanced());
        }
        assert_eq!(w.resets, 10);
    }
    #[test]
    fn t21_drop_reasons() {
        for (i, r) in DROP_REASONS.iter().enumerate() {
            assert_eq!((r.index(), DropReason::from_tag(r.tag())), (i, Some(*r)));
        }
        assert_eq!(DropReason::from_tag("other"), None); assert_eq!(DROP_REASONS.len(), REASONS);
    }
    #[test]
    fn t22_ring_cap() {
        let mut ring = BoundedRing::new();
        for i in 0..RING_CAP as u32 { assert!(ring.push(i)); }
        assert_eq!(ring.retained() as usize, RING_CAP); assert!(!ring.push(999_999));
        assert_eq!((ring.dropped(), ring.drop_for(DropReason::Full)), (1, 1)); assert!(ring.balanced());
    }
    #[test]
    fn t23_ring_fifo() {
        let mut ring = BoundedRing::new();
        for i in 0..3u32 { assert!(ring.push(i)); }
        assert_eq!((ring.pop(), ring.pop(), ring.pop(), ring.pop()), (Some(0), Some(1), Some(2), None));
        assert_eq!(ring.delivered, 3); assert!(ring.is_empty() && ring.balanced());
    }
    #[test]
    fn t24_ring_identity() {
        let g = ring_overflow_scenario();
        assert_eq!((g.pushed, g.delivered, g.retained, g.dropped), (4_472, 4_196, 0, 276));
        assert_eq!(g.pushed, g.delivered + g.retained + g.dropped); assert!(g.balanced && !g.over_cap);
    }
    #[test]
    fn t25_ring_buckets() {
        let g = ring_overflow_scenario();
        assert_eq!((bucket(&g.by_reason, 0), bucket(&g.by_reason, 1)), (RING_OVERFLOW, LATE_DROPS));
        assert_eq!((bucket(&g.by_reason, 2), bucket(&g.by_reason, 3)), (DUP_DROPS, BARRIER_DROPS));
        assert_eq!(bucket_sum(&g.by_reason), g.dropped);
        assert_eq!((bucket(&[1, 2, 3, 4], 2), bucket(&[1, 2, 3, 4], 9), bucket_sum(&[1, 2, 3, 4])), (3, 0, 10));
    }
    #[test]
    fn t26_ring_explicit() {
        let mut ring = BoundedRing::new();
        assert!(ring.drop_explicit(DropReason::Late) && ring.drop_explicit(DropReason::Barrier));
        assert_eq!((ring.drop_for(DropReason::Late), ring.drop_for(DropReason::Barrier)), (1, 1));
        assert_eq!((ring.pushed, ring.delivered), (2, 0)); assert!(ring.balanced());
    }
    #[test]
    fn t27_ring_reset() {
        let mut ring = BoundedRing::new();
        for i in 0..3u32 { assert!(ring.push(i)); }
        ring.reset();
        assert!(ring.is_empty() && ring.balanced()); assert_eq!((ring.pushed, ring.delivered, ring.dropped(), ring.retained()), (0, 0, 0, 0));
    }
    #[test]
    fn t28_replay() {
        let r = replay_cycles();
        assert!(r.ok());
        assert_eq!((r.rounds, r.pushed, r.delivered, r.dropped), (10, 44_720, 41_960, 2_760)); assert_eq!(r.digest, replay_cycles().digest);
    }
    #[test]
    fn t29_rollback() {
        let r = rollback_cycles();
        assert!(r.ok() && r.all_empty);
        assert_eq!((r.rounds, r.pushed, r.delivered, r.dropped), (10, 41_600, 40_960, 640));
    }
    #[test]
    fn t30_replay_negative() {
        let r = ReplayReport { rounds: 9, pushed: 1, delivered: 0, dropped: 1, all_balanced: true, all_empty: true, digest: 0 };
        assert!(!r.ok()); assert!(ReplayReport { rounds: 10, ..r }.ok());
    }
    #[test]
    fn t31_record_bounds() {
        assert!(Record::new(0, 1, 1, 0, 0).is_none() && Record::new(5, 1, 1, 0, 0).is_none());
        assert!(Record::new(1, 2, 1, 0, 0).is_none()); assert!(Record::new(4, 2, 1, 1, 0).is_some());
    }
    #[test]
    fn t32_record_balanced() {
        assert!(Record::new(1, 10, 7, 2, 1).expect("rec").balanced());
        let bad = Record { slot: 1, pushed: 10, delivered: 9, retained: 0, dropped: 0 };
        assert!(!bad.balanced());
    }
    #[test]
    fn t33_record_digest() {
        let a = record_digest(1, 10, 7, 2, 1);
        assert_eq!(a, record_digest(1, 10, 7, 2, 1)); assert_ne!(a, record_digest(2, 10, 7, 2, 1));
        assert_ne!(a, record_digest(1, 11, 7, 2, 1)); assert_ne!(a, record_digest(1, 10, 7, 2, 0));
    }
    #[test]
    fn t34_parse_u32() {
        assert_eq!((parse_u32("0"), parse_u32("007"), parse_u32("4294967295")), (Some(0), Some(7), Some(u32::MAX)));
        assert_eq!((parse_u32(""), parse_u32("12x"), parse_u32("-1")), (None, None, None));
        assert_eq!((parse_u32("4294967296"), parse_u32("99999999999999999999")), (None, None));
    }
    #[test]
    fn t35_parse_hex16() {
        assert_eq!((parse_hex16("0000000000000000"), parse_hex16("FFFFFFFFFFFFFFFF")), (Some(0), Some(u64::MAX)));
        assert_eq!((parse_hex16("000000000000000g"), parse_hex16("000000000000000"), parse_hex16("00000000000000000")), (None, None, None));
    }
    #[test]
    fn t36_render_audit() {
        let row = render_row(&Record::new(1, 10, 7, 2, 1).expect("rec"));
        assert!(row.starts_with("ORDR83|slot=1|pushed=10|delivered=7|retained=2|dropped=1|digest="));
        assert!(audit_row(&row)); assert_eq!(row.len(), 80);
    }
    #[test]
    fn t37_tamper_tail() {
        let row = render_row(&Record::new(2, 5, 3, 1, 1).expect("rec"));
        assert_ne!(tamper_tail(&row), row); assert!(!audit_row(&tamper_tail(&row)));
        assert_eq!(tamper_tail(""), ""); assert_eq!(tamper_tail("1"), "0"); assert_eq!(tamper_tail("0"), "1");
    }
    #[test]
    fn t38_tamper_append() {
        let row = render_row(&Record::new(3, 9, 6, 2, 1).expect("rec"));
        assert!(!audit_row(&(row.clone() + "0"))); assert!(!audit_row(&row.replace("pushed=9", "pushed=8")));
        assert!(audit_row(&row));
    }
    #[test]
    fn t39_row_fields() {
        let row = render_row(&records()[0]);
        let f = row_fields(&row, ROW_TAG, &ROW_KEYS).expect("fields");
        assert_eq!((f.len(), f[0]), (6, "1")); assert!(row_fields(&row, "XXXX", &ROW_KEYS).is_none());
        assert!(row_fields(&row, ROW_TAG, &["slot"]).is_none());
        assert!(row_fields("ORDR83|slot=|pushed=1|delivered=1|retained=0|dropped=0|digest=0000000000000000", ROW_TAG, &ROW_KEYS).is_none());
    }
    #[test]
    fn t40_export_rows() {
        let rows = export_rows();
        assert_eq!(rows.len(), ROW_SLOTS);
        for (i, row) in rows.iter().enumerate() {
            assert!(audit_row(row)); assert_eq!(parse_u32(row_fields(row, ROW_TAG, &ROW_KEYS).expect("f")[0]), Some(i as u32 + 1));
        }
    }
    #[test]
    fn t41_rows_stable() {
        assert!(rows_stable());
        let a = export_rows();
        for i in 0..a.len() { for j in 0..a.len() { if i != j { assert_ne!(a[i], a[j]); } } }
    }
    #[test]
    fn t42_check_count() { assert_eq!(extra_checks().len(), 8); }
    #[test]
    fn t43_check_names() {
        let c = extra_checks(); let (mut a, mut b) = (0u32, 0u32);
        for i in 0..c.len() {
            if c[i].0.starts_with("F49083/") { a += 1; }
            if c[i].0.starts_with("F49084/") { b += 1; }
            for j in 0..c.len() { if i != j { assert_ne!(c[i].0, c[j].0); } }
        }
        assert_eq!((a, b), (4, 4));
    }
    #[test]
    fn t44_checks_green() { for c in extra_checks() { assert!(c.1); assert!(!c.2.is_empty()); } }
    #[test]
    fn t45_checkset() {
        let s = run_unx49083_checks();
        assert_eq!((s.len(), s.passed_count(), s.failed_count()), (8, 8, 0)); assert!(!s.truncated());
        assert_eq!(s.domain, "UNX-F49083 乱序与溢出处置");
    }
}
