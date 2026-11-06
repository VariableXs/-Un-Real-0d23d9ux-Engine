//! UNX-F48967 加固记录与基线曲线五档 · 0x714C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B40 族第四槽）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F48967-J1 加固记录可查（每次旋钮变更留下前值/后值/结论）；
//!   ③ UNX-F48968-J1 基线曲线 ×5 档：档位锚定、帧数恒等、时延单调。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：旋钮号 → 单值域 → 跨域前置 → 变更留痕 → 记录审计 →
//!           档位锚定 → 帧数恒等 → 时延单调 → 曲线汇总。
//! 先校旋钮号再校值域：号错了的变更连「比什么」都不知道，若先比值，
//! 错误会被当成越界值报出来，真正的问题（号映射错位）就被掩盖了。
//! 跨域前置单独成闸：队列深度必须容得下一整批。单值都在域内但组合
//! 不成立的配置是最常见的加固逃逸——逐字段校验永远发现不了它。
//! 留痕在应用之前完成：先写后改才能保证「记录 = 实际发生」。反过来
//! 一次掉电就会留下改了值却没有记录的空档，加固记录立即失效。
//! 记录审计用「尝试 = 生效 + 拒绝」：只数生效次数会漏掉被拒的尝试，
//! 而拒绝正是加固强度的证据；只数尝试又无法证明每次尝试都有结论。
//! 档位锚定先于帧数：5 档速率是曲线的自变量，档位错位时帧数怎么算
//! 都能凑出一个「像样」的曲线——先在锚点上定死，曲线才有可比性。
//! 时延单调放在最后：它是曲线的定性结论（报点率越高、成帧越快）。
//! 只有当档位、帧数、恒等全部成立后，单调才有意义；否则单调只是
//! 一条被构造出来的漂亮折线。
//! 关键差异：本模块只回答「加固是否逐次留痕、五档基线是否自洽」，
//!           不做风暴恢复（B40-STORM）、不做水位背压（B40-BACKPRESSURE）、
//!           不做帧折叠等价（B40-CORE）。曲线是参照，不是实现。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x714C0，B40 族第四槽）。
pub const SEG_BASE: u32 = 0x714C0;

/// 加固旋钮个数。
pub const KNOB_COUNT: usize = 4;
/// 旋钮 0：报点率（Hz）。
pub const KNOB_HZ: usize = 0;
/// 旋钮 1：帧合并比（事件 : 帧）。
pub const KNOB_MERGE: usize = 1;
/// 旋钮 2：批大小（事件/批）。
pub const KNOB_BATCH: usize = 2;
/// 旋钮 3：队列深度（事件）。
pub const KNOB_QUEUE: usize = 3;
/// 合并比的下限：小于 2 就不叫合并。
pub const MERGE_MIN: u32 = 2;
/// 合并比的上限：8:1 是 B40 的标称口径。
pub const MERGE_RATIO: u32 = 8;
/// 批大小下限。
pub const BATCH_MIN: u32 = 8;
/// 批大小上限。
pub const BATCH_MAX: u32 = 256;
/// 队列深度下限。
pub const QUEUE_MIN: u32 = 32;
/// 队列深度上限。
pub const QUEUE_MAX: u32 = 2_048;
/// 档位个数（卡面判据的「×5 档」）。
pub const TIER_COUNT: usize = 5;
/// 基线档位速率（Hz）：125 → 2,000，逐档翻倍。
pub const TIER_HZ: [u32; TIER_COUNT] = [125, 250, 500, 1_000, 2_000];
/// 每档采样时长（秒）。
pub const SAMPLE_SECS: u32 = 10;
/// 时延常数：8 个样本凑一帧，故一帧的时延为 8×10^6 / 速率（微秒）。
pub const LAT_BASE_US: u32 = 8_000_000;
/// 加固轮次（卡面判据的 ×10 轮）。
pub const HARDEN_ROUNDS: u32 = 10;
/// 加固轮次上限。
pub const HARDEN_MAX: u32 = 10_000;
/// 曲线账字段数。
pub const TUNE_FIELDS: usize = 6;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0C） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TnCode {
    /// 旋钮号非法（未知旋钮）
    KnobBad,
    /// 值域非法（越出该旋钮的上下限）
    RangeBad,
    /// 无变更（与当前值相同，不得记入加固记录）
    StableBad,
    /// 前置不满足（跨域约束被破坏，如队列装不下一批）
    NeedBad,
    /// 次序非法（轮次越界或记录次序断裂）
    OrderBad,
    /// 账不平（尝试 ≠ 生效 + 拒绝）
    LedgerBad,
    /// 曲线非法（档位错位、帧数不恒等或时延不单调）
    CurveBad,
    /// 渲染非法（字段缺失或回读失败）
    RenderBad,
    /// 复算不一致（同一流重放不同）
    ReplayBad,
    /// 覆盖不足（轮次或档位不足）
    CoverageBad,
    /// 段位越界（诊断码逃出 0x714C0 窗口）
    SegmentBad,
    /// 记录非法（留痕缺失或前后值不自洽）
    TraceBad,
}

impl TnCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            TnCode::KnobBad => "旋钮号非法：未知旋钮",
            TnCode::RangeBad => "值域非法：越出该旋钮的上下限",
            TnCode::StableBad => "无变更：与当前值相同，不计入加固记录",
            TnCode::NeedBad => "前置不满足：跨域约束被破坏（如队列装不下一批）",
            TnCode::OrderBad => "次序非法：轮次越界或记录次序断裂",
            TnCode::LedgerBad => "账不平：尝试 ≠ 生效 + 拒绝",
            TnCode::CurveBad => "曲线非法：档位错位、帧数不恒等或时延不单调",
            TnCode::RenderBad => "渲染非法：字段缺失或回读失败",
            TnCode::ReplayBad => "复算不一致：同一流重放结果不同",
            TnCode::CoverageBad => "覆盖不足：轮次或档位数不足",
            TnCode::SegmentBad => "段位越界：诊断码逃出 0x714C0 窗口",
            TnCode::TraceBad => "记录非法：留痕缺失或前后值不自洽",
        }
    }

    pub const ALL: [TnCode; 12] = [
        TnCode::KnobBad,
        TnCode::RangeBad,
        TnCode::StableBad,
        TnCode::NeedBad,
        TnCode::OrderBad,
        TnCode::LedgerBad,
        TnCode::CurveBad,
        TnCode::RenderBad,
        TnCode::ReplayBad,
        TnCode::CoverageBad,
        TnCode::SegmentBad,
        TnCode::TraceBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < TnCode::ALL.len() {
        if TnCode::ALL[i].val() == code {
            return TnCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 旋钮表（加固面） ──────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Knob {
    /// 旋钮名（诊断用，固定字面量）
    pub name: &'static str,
    /// 下限（含）
    pub lo: u32,
    /// 上限（含）
    pub hi: u32,
}

/// 四个旋钮的冻结表：名称、下限、上限。
pub const KNOBS: [Knob; KNOB_COUNT] = [
    Knob {
        name: "polling_hz",
        lo: 125,
        hi: 1_000,
    },
    Knob {
        name: "merge_ratio",
        lo: MERGE_MIN,
        hi: MERGE_RATIO,
    },
    Knob {
        name: "batch_size",
        lo: BATCH_MIN,
        hi: BATCH_MAX,
    },
    Knob {
        name: "queue_depth",
        lo: QUEUE_MIN,
        hi: QUEUE_MAX,
    },
];

/// 取旋钮定义；越界索引返回 KnobBad。
pub fn knob_at(index: u32) -> Result<Knob, u32> {
    match KNOBS.get(index as usize) {
        Some(k) => Ok(*k),
        None => Err(TnCode::KnobBad.val()),
    }
}

/// 单值域校验。
pub fn in_range(index: u32, value: u32) -> Result<(), u32> {
    let k = knob_at(index)?;
    if value < k.lo || value > k.hi {
        return Err(TnCode::RangeBad.val());
    }
    Ok(())
}

// ── 加固档案（逐次留痕） ──────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TuningProfile {
    /// [报点率, 合并比, 批大小, 队列深度]
    pub values: [u32; KNOB_COUNT],
}

impl TuningProfile {
    /// 生产默认档：1,000Hz / 8:1 / 64 / 512。
    pub fn new() -> TuningProfile {
        TuningProfile {
            values: [1_000, MERGE_RATIO, 64, 512],
        }
    }

    /// 单值域全部成立。
    pub fn within_bounds(&self) -> bool {
        let mut i = 0usize;
        while i < KNOB_COUNT {
            let v = match self.values.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if in_range(i as u32, v).is_err() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 跨域前置：队列深度必须容得下一整批。
    ///
    /// 这是「逐字段都在域内、组合不可用」的典型：batch=256、queue=64
    /// 各自合法，但一次入队就会撕裂整批。加固必须把这类组合挡在门外。
    pub fn needs_ok(&self) -> bool {
        let batch = match self.values.get(KNOB_BATCH) {
            Some(v) => *v,
            None => return false,
        };
        let queue = match self.values.get(KNOB_QUEUE) {
            Some(v) => *v,
            None => return false,
        };
        queue >= batch
    }

    pub fn get(&self, index: usize) -> Result<u32, u32> {
        match self.values.get(index) {
            Some(v) => Ok(*v),
            None => Err(TnCode::KnobBad.val()),
        }
    }

    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < KNOB_COUNT {
            if let Some(v) = self.values.get(i) {
                h = p_core::fold_u32(h, *v);
            }
            i += 1;
        }
        h
    }
}

/// 一条加固记录（前值/后值/结论）——「可查」的最小凭证。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HardenEntry {
    pub seq: u32,
    pub knob: u32,
    pub before: u32,
    pub after: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HardeningBook {
    pub profile: TuningProfile,
    /// 生效的变更数
    pub entries: u32,
    /// 被拒的尝试数
    pub rejected: u32,
    /// 尝试总数 = 生效 + 拒绝
    pub attempts: u32,
    /// 最后一条留痕（供「可查」直接读取）
    pub last: HardenEntry,
    pub digest: u64,
}

impl HardeningBook {
    pub fn new() -> HardeningBook {
        HardeningBook {
            profile: TuningProfile::new(),
            entries: 0,
            rejected: 0,
            attempts: 0,
            last: HardenEntry {
                seq: 0,
                knob: 0,
                before: 0,
                after: 0,
            },
            digest: p_core::FNV_OFF,
        }
    }

    /// 施加一次旋钮变更：先校验、再留痕、最后生效。
    pub fn apply(&mut self, knob: u32, value: u32) -> Result<(), u32> {
        self.attempts = self.attempts.saturating_add(1);
        // 未知旋钮也是「一次有结论的尝试」：只记 attempts 不记 rejected，
        // 尝试分解（尝试 = 生效 + 拒绝）就会漏数，账立即不平。
        let def = match knob_at(knob) {
            Ok(k) => k,
            Err(e) => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(e);
            }
        };
        if value < def.lo || value > def.hi {
            self.rejected = self.rejected.saturating_add(1);
            return Err(TnCode::RangeBad.val());
        }
        let before = match self.profile.values.get(knob as usize) {
            Some(v) => *v,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(TnCode::KnobBad.val());
            }
        };
        if before == value {
            self.rejected = self.rejected.saturating_add(1);
            return Err(TnCode::StableBad.val());
        }
        let mut candidate = self.profile;
        if let Some(slot) = candidate.values.get_mut(knob as usize) {
            *slot = value;
        }
        if !candidate.needs_ok() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(TnCode::NeedBad.val());
        }
        // 留痕先于生效：先写后改，掉电也不会出现「改了没记」。
        self.last = HardenEntry {
            seq: self.entries,
            knob,
            before,
            after: value,
        };
        self.digest = p_core::fold_u32(self.digest, knob);
        self.digest = p_core::fold_u32(self.digest, before);
        self.digest = p_core::fold_u32(self.digest, value);
        self.profile = candidate;
        self.entries = self.entries.saturating_add(1);
        Ok(())
    }

    /// 记录审计：尝试分解、值域、跨域、留痕自洽四项同时成立。
    pub fn audit(&self) -> Result<(), u32> {
        if self.attempts != self.entries.saturating_add(self.rejected) {
            return Err(TnCode::LedgerBad.val());
        }
        if !self.profile.within_bounds() || !self.profile.needs_ok() {
            return Err(TnCode::NeedBad.val());
        }
        if self.entries > 0 {
            if self.last.seq.saturating_add(1) != self.entries {
                return Err(TnCode::TraceBad.val());
            }
            if self.last.before == self.last.after {
                return Err(TnCode::TraceBad.val());
            }
        }
        Ok(())
    }
}

/// 加固 ×N 轮：确定性乱参数，越界/跨域/无变更都会被拒并计数。
pub fn harden_rounds(rounds: u32) -> Result<HardeningBook, u32> {
    if rounds == 0 || rounds > HARDEN_MAX {
        return Err(TnCode::CoverageBad.val());
    }
    let mut book = HardeningBook::new();
    let mut rng = p_core::Lcg::new(0x5439_0033);
    let mut r = 0u32;
    while r < rounds {
        // 高位路由：低位在 2^k 模下会锁死，乱参数退化成同一条流。
        let a = rng.next();
        let knob = ((a >> 33) % (KNOB_COUNT as u64 + 1)) as u32;
        let value = ((a >> 17) % 4_096) as u32;
        let _ = book.apply(knob, value);
        book.audit()?;
        r = r.saturating_add(1);
    }
    if book.entries == 0 || book.rejected == 0 {
        return Err(TnCode::CoverageBad.val());
    }
    Ok(book)
}

// ── 基线曲线五档 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TierRow {
    /// 档位速率（Hz）
    pub hz: u32,
    /// 采样窗口内事件数
    pub events: u32,
    /// 合成帧数
    pub frames: u32,
    /// 整数除法的帧余量
    pub residue: u32,
    /// 单帧时延（微秒）
    pub latency_us: u32,
}

/// 单档换算：事件 = 速率 × 秒；帧 = 事件 / 合并比；时延 = 8×10^6 / 速率。
pub fn tier_row(hz: u32) -> Result<TierRow, u32> {
    if !TIER_HZ.contains(&hz) {
        return Err(TnCode::CurveBad.val());
    }
    let events = hz.checked_mul(SAMPLE_SECS).ok_or(TnCode::CurveBad.val())?;
    let frames = events / MERGE_RATIO;
    let residue = events % MERGE_RATIO;
    let latency_us = LAT_BASE_US / hz;
    if frames == 0 || latency_us == 0 {
        return Err(TnCode::CurveBad.val());
    }
    // 恒等：事件 = 帧 × 合并比 + 余量。
    if frames.saturating_mul(MERGE_RATIO).saturating_add(residue) != events {
        return Err(TnCode::CurveBad.val());
    }
    Ok(TierRow {
        hz,
        events,
        frames,
        residue,
        latency_us,
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CurveLedger {
    pub tiers: u32,
    pub events: u64,
    pub frames: u64,
    pub latency_max: u32,
    pub latency_min: u32,
    pub digest: u64,
}

/// 曲线汇总：档位锚定 → 逐档恒等 → 帧数单调（不降）→ 时延严格单调（降）。
pub fn baseline_curve() -> Result<CurveLedger, u32> {
    let mut events = 0u64;
    let mut frames = 0u64;
    let mut latency_max = 0u32;
    let mut latency_min = u32::MAX;
    let mut prev_frames = 0u32;
    let mut prev_latency = u32::MAX;
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < TIER_COUNT {
        let hz = match TIER_HZ.get(i) {
            Some(v) => *v,
            None => return Err(TnCode::CurveBad.val()),
        };
        let row = tier_row(hz)?;
        if row.frames < prev_frames {
            return Err(TnCode::CurveBad.val());
        }
        if row.latency_us >= prev_latency {
            // 报点率上升而时延不降：曲线定性结论被破坏。
            return Err(TnCode::CurveBad.val());
        }
        events = events.saturating_add(row.events as u64);
        frames = frames.saturating_add(row.frames as u64);
        if row.latency_us > latency_max {
            latency_max = row.latency_us;
        }
        if row.latency_us < latency_min {
            latency_min = row.latency_us;
        }
        prev_frames = row.frames;
        prev_latency = row.latency_us;
        h = p_core::fold_u32(h, row.hz);
        h = p_core::fold_u32(h, row.events);
        h = p_core::fold_u32(h, row.frames);
        h = p_core::fold_u32(h, row.latency_us);
        i += 1;
    }
    if latency_min == u32::MAX || latency_max == 0 {
        return Err(TnCode::CurveBad.val());
    }
    Ok(CurveLedger {
        tiers: TIER_COUNT as u32,
        events,
        frames,
        latency_max,
        latency_min,
        digest: h,
    })
}

/// 曲线总事件（编译期常量，供渲染回读对账）。
const fn curve_events() -> u32 {
    let mut i = 0usize;
    let mut s = 0u32;
    while i < TIER_COUNT {
        s += TIER_HZ[i] * SAMPLE_SECS;
        i += 1;
    }
    s
}

/// 曲线总帧数（编译期常量）。
const fn curve_frames() -> u32 {
    let mut i = 0usize;
    let mut s = 0u32;
    while i < TIER_COUNT {
        s += TIER_HZ[i] * SAMPLE_SECS / MERGE_RATIO;
        i += 1;
    }
    s
}

/// 曲线总事件。
pub const CURVE_EVENTS: u32 = curve_events();
/// 曲线总帧数。
pub const CURVE_FRAMES: u32 = curve_frames();
/// 曲线最大时延（最低档 125Hz 处取得）。
pub const CURVE_LATENCY_MAX: u32 = LAT_BASE_US / TIER_HZ[0];

// ── 汇总账（加固 + 曲线） ──────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TuneLedger {
    pub tiers: u32,
    pub events: u64,
    pub frames: u64,
    pub latency_max: u32,
    pub entries: u32,
    pub digest: u64,
}

/// 汇总：先加固、再测曲线，两者都过审才出账。
pub fn tune_ledger() -> Result<TuneLedger, u32> {
    let book = harden_rounds(HARDEN_ROUNDS)?;
    book.audit()?;
    let curve = baseline_curve()?;
    let mut digest = p_core::fold_u32(p_core::FNV_OFF, book.digest as u32);
    digest = p_core::fold_u32(digest, (book.digest >> 32) as u32);
    digest = p_core::fold_u32(digest, book.entries);
    digest = p_core::fold_u32(digest, curve.digest as u32);
    digest = p_core::fold_u32(digest, (curve.digest >> 32) as u32);
    Ok(TuneLedger {
        tiers: curve.tiers,
        events: curve.events,
        frames: curve.frames,
        latency_max: curve.latency_max,
        entries: book.entries,
        digest,
    })
}

// ── fuzz 面 ───────────────────────────────────────────────────────
/// 旋钮 fuzz：乱号/乱值下，记录审计恒成立且必有两路结论。
pub fn fuzz_knob_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 || rounds > HARDEN_MAX {
        return Err(TnCode::CoverageBad.val());
    }
    let mut book = HardeningBook::new();
    let mut rng = p_core::Lcg::new(0x4B4E_0044);
    let mut r = 0u32;
    while r < rounds {
        let a = rng.next();
        let knob = ((a >> 33) % (KNOB_COUNT as u64 + 3)) as u32;
        let value = ((a >> 19) % 3_000) as u32;
        let _ = book.apply(knob, value);
        book.audit()?;
        r = r.saturating_add(1);
    }
    if book.entries == 0 {
        return Err(TnCode::CoverageBad.val());
    }
    Ok(book.digest)
}

/// 曲线 fuzz：逐档恒等与单调必须恒成立（档位集合本身是闭集）。
pub fn fuzz_curve_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 || rounds > HARDEN_MAX {
        return Err(TnCode::CoverageBad.val());
    }
    let mut h = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        let c = baseline_curve()?;
        if c.events != CURVE_EVENTS as u64 || c.frames != CURVE_FRAMES as u64 {
            return Err(TnCode::CurveBad.val());
        }
        h = p_core::fold_u32(h, r);
        h = p_core::fold_u32(h, c.digest as u32);
        r = r.saturating_add(1);
    }
    Ok(h)
}

// ── 渲染与回读 ────────────────────────────────────────────────────
pub fn render_tune(l: &TuneLedger) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        l.tiers, l.events, l.frames, l.latency_max, l.entries, l.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(TnCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(TnCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(TnCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(TnCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(TnCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(TnCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(TnCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(TnCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 渲染复读：档位、事件、帧数、时延、记录、摘要六项。
pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != TUNE_FIELDS {
        return Err(TnCode::RenderBad.val());
    }
    let tiers = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(TnCode::RenderBad.val()),
    })?;
    let events = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(TnCode::RenderBad.val()),
    })?;
    let frames = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(TnCode::RenderBad.val()),
    })?;
    let latency_max = parse_u32(match parts.get(3) {
        Some(v) => v,
        None => return Err(TnCode::RenderBad.val()),
    })?;
    let entries = parse_u32(match parts.get(4) {
        Some(v) => v,
        None => return Err(TnCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(5) {
        Some(v) => v,
        None => return Err(TnCode::RenderBad.val()),
    })?;
    if tiers != TIER_COUNT as u32 {
        return Err(TnCode::CurveBad.val());
    }
    if events != CURVE_EVENTS || frames != CURVE_FRAMES {
        return Err(TnCode::CurveBad.val());
    }
    if latency_max != CURVE_LATENCY_MAX {
        return Err(TnCode::CurveBad.val());
    }
    if entries == 0 {
        return Err(TnCode::TraceBad.val());
    }
    if digest == 0 {
        return Err(TnCode::RenderBad.val());
    }
    Ok(tiers)
}

pub fn golden_run() -> Result<(u32, u64, u64, u32, u32, u64), u32> {
    let l = tune_ledger()?;
    let text = render_tune(&l);
    if audit_render(&text)? != l.tiers {
        return Err(TnCode::ReplayBad.val());
    }
    if l.entries == 0 || l.latency_max != CURVE_LATENCY_MAX {
        return Err(TnCode::TraceBad.val());
    }
    Ok((
        l.tiers,
        l.events,
        l.frames,
        l.latency_max,
        l.entries,
        l.digest,
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
        return Err(TnCode::CoverageBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut r = 0u32;
    while r < rounds {
        // ① 未知旋钮号。
        log.tries[0] = log.tries[0].saturating_add(1);
        let mut b = HardeningBook::new();
        if b.apply(KNOB_COUNT as u32, 128) == Err(TnCode::KnobBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }

        // ② 越界值。
        log.tries[1] = log.tries[1].saturating_add(1);
        let mut c = HardeningBook::new();
        if c.apply(KNOB_HZ as u32, KNOBS[KNOB_HZ].hi.saturating_add(1))
            == Err(TnCode::RangeBad.val())
        {
            log.caught[1] = log.caught[1].saturating_add(1);
        }

        // ③ 跨域前置：队列 32 装不下 64 的批。
        log.tries[2] = log.tries[2].saturating_add(1);
        let mut d = HardeningBook::new();
        if d.apply(KNOB_QUEUE as u32, QUEUE_MIN) == Err(TnCode::NeedBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }

        // ④ 无变更：重复设置当前值不入记录。
        log.tries[3] = log.tries[3].saturating_add(1);
        let mut e = HardeningBook::new();
        let now = match e.profile.get(KNOB_BATCH) {
            Ok(v) => v,
            Err(_) => 0,
        };
        if e.apply(KNOB_BATCH as u32, now) == Err(TnCode::StableBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }

        r = r.saturating_add(1);
    }
    Ok(log)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if TIER_COUNT != 5 || TIER_HZ[0] != 125 || TIER_HZ[4] != 2_000 {
        return Err(TnCode::CurveBad.val());
    }
    let book = harden_rounds(HARDEN_ROUNDS)?;
    book.audit()?;
    if book.entries == 0 || book.rejected == 0 {
        return Err(TnCode::TraceBad.val());
    }
    if !book.profile.within_bounds() || !book.profile.needs_ok() {
        return Err(TnCode::NeedBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let c = baseline_curve()?;
    if c.tiers != TIER_COUNT as u32 {
        return Err(TnCode::CurveBad.val());
    }
    if c.events != CURVE_EVENTS as u64 || c.frames != CURVE_FRAMES as u64 {
        return Err(TnCode::CurveBad.val());
    }
    if c.latency_max != CURVE_LATENCY_MAX || c.latency_min != LAT_BASE_US / TIER_HZ[4] {
        return Err(TnCode::CurveBad.val());
    }
    // 125Hz：1,250 事件 → 156 帧余 2；2,000Hz：20,000 事件 → 2,500 帧。
    let lo = tier_row(TIER_HZ[0])?;
    if lo.frames != 156 || lo.residue != 2 || lo.latency_us != 64_000 {
        return Err(TnCode::CurveBad.val());
    }
    let hi = tier_row(TIER_HZ[4])?;
    if hi.frames != 2_500 || hi.residue != 0 || hi.latency_us != 4_000 {
        return Err(TnCode::CurveBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let mut b = HardeningBook::new();
    if b.apply(KNOB_MERGE as u32, MERGE_RATIO + 1) != Err(TnCode::RangeBad.val()) {
        return Err(TnCode::RangeBad.val());
    }
    if b.apply(KNOB_QUEUE as u32, QUEUE_MIN) != Err(TnCode::NeedBad.val()) {
        return Err(TnCode::NeedBad.val());
    }
    if b.apply(KNOB_BATCH as u32, 64) != Err(TnCode::StableBad.val()) {
        return Err(TnCode::StableBad.val());
    }
    if b.apply(KNOB_COUNT as u32, 128) != Err(TnCode::KnobBad.val()) {
        return Err(TnCode::KnobBad.val());
    }
    // 合法变更：批 64 → 32 与队列 512 相容。
    b.apply(KNOB_BATCH as u32, 32)?;
    b.audit()?;
    if b.entries != 1 || b.rejected != 4 || b.attempts != 5 {
        return Err(TnCode::LedgerBad.val());
    }
    if b.last.knob != KNOB_BATCH as u32 || b.last.before != 64 || b.last.after != 32 {
        return Err(TnCode::TraceBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = harden_rounds(HARDEN_ROUNDS)?;
    let b = harden_rounds(HARDEN_ROUNDS)?;
    if a.digest != b.digest || a.entries != b.entries || a.rejected != b.rejected {
        return Err(TnCode::ReplayBad.val());
    }
    if baseline_curve()?.digest != baseline_curve()?.digest {
        return Err(TnCode::ReplayBad.val());
    }
    if fuzz_knob_rounds(300)? != fuzz_knob_rounds(300)? {
        return Err(TnCode::ReplayBad.val());
    }
    if fuzz_curve_rounds(50)? != fuzz_curve_rounds(50)? {
        return Err(TnCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < TnCode::ALL.len() {
        let v = TnCode::ALL[i].val();
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
        "F48967 旋钮表锚定",
        KNOBS[KNOB_HZ].lo == 125
            && KNOBS[KNOB_HZ].hi == 1_000
            && KNOBS[KNOB_MERGE].lo == MERGE_MIN
            && KNOBS[KNOB_MERGE].hi == MERGE_RATIO
            && KNOBS[KNOB_BATCH].hi == BATCH_MAX
            && KNOBS[KNOB_QUEUE].hi == QUEUE_MAX,
        format!(
            "hz=[{},{}] merge=[{},{}] batch=[{},{}] queue=[{},{}]",
            KNOBS[KNOB_HZ].lo,
            KNOBS[KNOB_HZ].hi,
            KNOBS[KNOB_MERGE].lo,
            KNOBS[KNOB_MERGE].hi,
            KNOBS[KNOB_BATCH].lo,
            KNOBS[KNOB_BATCH].hi,
            KNOBS[KNOB_QUEUE].lo,
            KNOBS[KNOB_QUEUE].hi
        ),
    ));

    let book = harden_rounds(HARDEN_ROUNDS);
    out.push((
        "F48967 加固记录×10可查",
        matches!(&book, Ok(b) if b.entries > 0
            && b.rejected > 0
            && b.attempts == b.entries + b.rejected
            && b.profile.within_bounds()
            && b.profile.needs_ok()),
        match &book {
            Ok(b) => format!(
                "entries={} rejected={} attempts={} last={}→{}",
                b.entries, b.rejected, b.attempts, b.last.before, b.last.after
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut reject_ok = false;
    let mut probe = HardeningBook::new();
    let r1 = probe.apply(KNOB_COUNT as u32, 128) == Err(TnCode::KnobBad.val());
    let r2 = probe.apply(KNOB_HZ as u32, 0) == Err(TnCode::RangeBad.val());
    let r3 = probe.apply(KNOB_QUEUE as u32, QUEUE_MIN) == Err(TnCode::NeedBad.val());
    let r4 = probe.apply(KNOB_BATCH as u32, 64) == Err(TnCode::StableBad.val());
    if r1 && r2 && r3 && r4 {
        reject_ok = probe.attempts == 4 && probe.rejected == 4 && probe.entries == 0;
    }
    out.push((
        "F48967 越界与跨域拒绝",
        reject_ok,
        format!(
            "knob={} range={} need={} stable={} attempts={}",
            r1, r2, r3, r4, probe.attempts
        ),
    ));

    let curve = baseline_curve();
    out.push((
        "F48968 基线曲线×5档",
        matches!(&curve, Ok(c) if c.tiers == TIER_COUNT as u32
            && c.events == CURVE_EVENTS as u64
            && c.frames == CURVE_FRAMES as u64),
        match &curve {
            Ok(c) => format!(
                "tiers={} events={} frames={}",
                c.tiers, c.events, c.frames
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut mono_ok = true;
    let mut lat_seen = [0u32; TIER_COUNT];
    let mut i = 0usize;
    while i < TIER_COUNT {
        match tier_row(TIER_HZ[i]) {
            Ok(row) => lat_seen[i] = row.latency_us,
            Err(_) => mono_ok = false,
        }
        if i > 0 && lat_seen[i] >= lat_seen[i - 1] {
            mono_ok = false;
        }
        i += 1;
    }
    let golden = golden_run();
    out.push((
        "F48968 时延单调与账本可解析",
        mono_ok
            && matches!(&golden, Ok((t, _, _, lat, e, _)) if *t == TIER_COUNT as u32
                && *lat == CURVE_LATENCY_MAX
                && *e > 0),
        match &golden {
            Ok((t, ev, f, lat, e, d)) => format!(
                "tiers={} events={} frames={} lat={} entries={} d=0x{:X}",
                t, ev, f, lat, e, d
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let fuzz_a = fuzz_knob_rounds(10_000);
    let fuzz_b = fuzz_curve_rounds(1_000);
    out.push((
        "F48967 fuzz面不崩",
        fuzz_a.is_ok() && fuzz_b.is_ok(),
        format!(
            "knob={} curve={}",
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
        "F48967 四路注入捕获",
        matches!(&inject, Ok(l) if l.all_caught()),
        match &inject {
            Ok(l) => format!("caught={:?}", l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48967 段位闭集",
        segment_guard(),
        format!("codes={} seg=0x{:04X}", TnCode::ALL.len(), SEG_BASE),
    ));

    out
}

pub fn run_unx48967_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48967 加固记录与基线曲线");
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
        assert_eq!(SEG_BASE, 0x714C0);
        assert_eq!(TIER_COUNT, 5);
        assert_eq!(TIER_HZ[2], 500);
        assert_eq!(TUNE_FIELDS, 6);
        assert_eq!(CURVE_EVENTS, 38_750);
        assert_eq!(CURVE_FRAMES, 4_843);
        assert_eq!(CURVE_LATENCY_MAX, 64_000);
    }

    #[test]
    fn profile_defaults_within_bounds() {
        let p = TuningProfile::new();
        assert_eq!(p.get(KNOB_HZ), Ok(1_000));
        assert_eq!(p.get(KNOB_MERGE), Ok(MERGE_RATIO));
        assert!(p.within_bounds());
        assert!(p.needs_ok());
    }

    #[test]
    fn apply_records_before_and_after() {
        let mut b = HardeningBook::new();
        assert!(b.apply(KNOB_BATCH as u32, 32).is_ok());
        assert_eq!(b.entries, 1);
        assert_eq!(b.attempts, 1);
        assert_eq!(b.last.seq, 0);
        assert_eq!(b.last.before, 64);
        assert_eq!(b.last.after, 32);
        assert_eq!(b.profile.get(KNOB_BATCH), Ok(32));
        assert!(b.audit().is_ok());
    }

    #[test]
    fn apply_rejects_unknown_knob() {
        let mut b = HardeningBook::new();
        assert_eq!(
            b.apply(KNOB_COUNT as u32, 128),
            Err(TnCode::KnobBad.val())
        );
        assert_eq!(b.rejected, 1);
        assert_eq!(b.entries, 0);
        assert!(b.audit().is_ok());
    }

    #[test]
    fn apply_rejects_out_of_range() {
        let mut b = HardeningBook::new();
        assert_eq!(
            b.apply(KNOB_HZ as u32, KNOBS[KNOB_HZ].hi + 1),
            Err(TnCode::RangeBad.val())
        );
        assert_eq!(b.apply(KNOB_MERGE as u32, MERGE_MIN - 1), Err(TnCode::RangeBad.val()));
        assert_eq!(b.rejected, 2);
        assert!(b.audit().is_ok());
    }

    #[test]
    fn apply_rejects_cross_field() {
        let mut b = HardeningBook::new();
        assert_eq!(
            b.apply(KNOB_QUEUE as u32, QUEUE_MIN),
            Err(TnCode::NeedBad.val())
        );
        assert_eq!(b.entries, 0);
        assert_eq!(b.profile.get(KNOB_QUEUE), Ok(512));
    }

    #[test]
    fn apply_rejects_noop() {
        let mut b = HardeningBook::new();
        assert_eq!(
            b.apply(KNOB_HZ as u32, 1_000),
            Err(TnCode::StableBad.val())
        );
        assert_eq!(b.rejected, 1);
    }

    #[test]
    fn audit_catches_identity_tamper() {
        let mut b = HardeningBook::new();
        assert!(b.apply(KNOB_BATCH as u32, 32).is_ok());
        b.attempts = b.attempts.saturating_add(1);
        assert_eq!(b.audit(), Err(TnCode::LedgerBad.val()));
    }

    #[test]
    fn audit_catches_trace_tamper() {
        let mut b = HardeningBook::new();
        assert!(b.apply(KNOB_BATCH as u32, 32).is_ok());
        b.last.after = b.last.before;
        assert_eq!(b.audit(), Err(TnCode::TraceBad.val()));
    }

    #[test]
    fn harden_ten_rounds_is_sound() {
        let b = harden_rounds(HARDEN_ROUNDS);
        assert!(b.is_ok());
        if let Ok(v) = b {
            assert!(v.entries > 0);
            assert!(v.rejected > 0);
            assert_eq!(v.attempts, v.entries + v.rejected);
            assert!(v.profile.within_bounds());
            assert!(v.profile.needs_ok());
        }
    }

    #[test]
    fn harden_rejects_bad_rounds() {
        assert_eq!(harden_rounds(0), Err(TnCode::CoverageBad.val()));
        assert_eq!(harden_rounds(HARDEN_MAX + 1), Err(TnCode::CoverageBad.val()));
    }

    #[test]
    fn tier_rows_are_exact() {
        let lo = match tier_row(125) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(lo.events, 1_250);
        assert_eq!(lo.frames, 156);
        assert_eq!(lo.residue, 2);
        assert_eq!(lo.latency_us, 64_000);
        let hi = match tier_row(2_000) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(hi.events, 20_000);
        assert_eq!(hi.frames, 2_500);
        assert_eq!(hi.residue, 0);
        assert_eq!(hi.latency_us, 4_000);
    }

    #[test]
    fn tier_rejects_unknown_hz() {
        assert_eq!(tier_row(126), Err(TnCode::CurveBad.val()));
        assert_eq!(tier_row(0), Err(TnCode::CurveBad.val()));
    }

    #[test]
    fn curve_is_monotone() {
        let c = match baseline_curve() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(c.tiers, TIER_COUNT as u32);
        assert_eq!(c.events, CURVE_EVENTS as u64);
        assert_eq!(c.frames, CURVE_FRAMES as u64);
        assert_eq!(c.latency_max, 64_000);
        assert_eq!(c.latency_min, 4_000);
        let mut last_lat = u32::MAX;
        let mut last_frames = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            let row = match tier_row(TIER_HZ[i]) {
                Ok(v) => v,
                Err(_) => return assert!(false),
            };
            assert!(row.latency_us < last_lat);
            assert!(row.frames >= last_frames);
            last_lat = row.latency_us;
            last_frames = row.frames;
            i += 1;
        }
    }

    #[test]
    fn tune_ledger_combines_faces() {
        let l = match tune_ledger() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(l.tiers, TIER_COUNT as u32);
        assert_eq!(l.events, CURVE_EVENTS as u64);
        assert_eq!(l.frames, CURVE_FRAMES as u64);
        assert!(l.entries > 0);
        assert!(l.digest != 0);
    }

    #[test]
    fn fuzz_faces_are_green() {
        assert!(fuzz_knob_rounds(2_000).is_ok());
        assert!(fuzz_curve_rounds(200).is_ok());
        assert_eq!(fuzz_knob_rounds(0), Err(TnCode::CoverageBad.val()));
        assert_eq!(fuzz_curve_rounds(0), Err(TnCode::CoverageBad.val()));
    }

    #[test]
    fn render_roundtrip() {
        let l = match tune_ledger() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let text = render_tune(&l);
        assert_eq!(audit_render(&text), Ok(TIER_COUNT as u32));
    }

    #[test]
    fn render_tamper_is_rejected() {
        let l = match tune_ledger() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let text = render_tune(&l);
        assert!(audit_render(&text.replacen("0x", "0y", 1)).is_err());
        // 帧数被改：与 5 档常量不再相等。
        assert!(audit_render(&text.replacen("|4843|", "|4844|", 1)).is_err());
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
        while i < TnCode::ALL.len() {
            let c = TnCode::ALL[i];
            assert_eq!(detail(c.val()), c.detail());
            let mut k = i + 1;
            while k < TnCode::ALL.len() {
                assert!(c.val() != TnCode::ALL[k].val());
                k += 1;
            }
            i += 1;
        }
        assert!(segment_guard());
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48967_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
