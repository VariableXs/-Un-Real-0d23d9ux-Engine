//! UNX-F49065 中断风暴延迟归因报告（M8 注入 ×10 分段定位成文）· 0x73300
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B14 族第三槽）：
//!   UNX-F49065 中断风暴延迟归因报告（M8 注入 ×10 分段定位成文）:
//!          ① 纯功能代码 ≥320 行；
//!          ② 判据原文：UNX-F49066-J1 长尾 ×10^2 回放一致；
//!             判据锚定 体验十三。
//!   UNX-F49066 延迟毛刺取证（P99.9 长尾样本入库可回放运营化）:
//!          ① 纯功能代码 ≥340 行；
//!          ② 判据原文：UNX-F49067-J1 导出 ×10 终验一致；
//!             判据锚定 §AI-62 §8 O1 联签。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（风暴归因）：定级 → 落段 → 归因 → 成文。
//! 定级先于落段：M8 注入必须先有等级刻度（L01..L10），否则"风暴多大"
//!   没有分母，落段统计只是噪声的另一种写法；
//! 落段先于归因：每一次注入必须先在四段里找到落点，才谈得上"谁扛了延迟"；
//!   落点按固定权重抽取（中断 6 : 分发 3 : 合成 2 : 呈现 1），
//!   权重是**先验**，不是实测后倒推的结论——倒推等于让结果选择证据；
//! 归因先于成文：主导段必须唯一且份额过半线以上，报告才允许落行；
//!   若两段并列，报告宁可拒绝（ReportBad）也不许挑一个写成事实。
//! 闸门次序（长尾取证）：切点 → 入库 → 回放 → 导出。
//! 切点先于入库：先在全量流上定出 P99.9 切点，再谈"哪些算长尾"；
//!   先收后定会让容量淘汰反过来改写切点，取证就成了自我实现的预言；
//! 入库先于回放：入库摘要按窗口内样本链式计算，回放只做同一件事；
//!   若回放另走一条路径，"一致"就只是两条代码碰巧写了同样的错；
//! 回放先于导出：百次回放同摘要之后，导出十轮才有"终验"的资格；
//!   导出格式对 O1 联签负责，内容对回放负责，两责不得互换。
//! 关键差异：F49065 面向**原因**（注入等级 → 落点 → 主导段），
//! F49066 面向**证据**（切点 → 样本窗口 → 回放 → 导出）。
//! 两本账摘要域独立：风暴摘要折等级/波次/三段计数，长尾摘要折窗口样本链。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x73300，B14 族第三槽）。
pub const SEG_BASE: u32 = 0x73300;

/// 延迟链四段。
pub const SEGMENTS: usize = 4;
/// M8 注入等级数（L01..L10）。
pub const LEVELS: u32 = 10;
/// 每级注入的突发波次。
pub const BURSTS: u32 = 16;
/// 每波次的标准中断数。
pub const PER_BURST: u32 = 64;
/// 四段基准延迟（纳秒）：中断 / 分发 / 合成 / 呈现。
pub const STORM_BASE_NS: [u32; SEGMENTS] = [700, 1_100, 2_600, 4_400];
/// 四段预算（纳秒）：越预算的落点计入毛刺。
pub const SEG_BUDGET_NS: [u32; SEGMENTS] = [1_200, 1_800, 4_800, 7_000];
/// 落点权重（先验）：中断 6 : 分发 3 : 合成 2 : 呈现 1。
pub const LOCALITY: [u32; SEGMENTS] = [6, 3, 2, 1];
/// 每级每单位权重的注入延迟（纳秒）。
pub const LEVEL_STEP_NS: u32 = 30;
/// 计时值上限（1 秒，纳秒）。
pub const NS_LIMIT: u32 = 1_000_000_000;
/// 主导段份额下限（千分比，40%）。
pub const DOMINANT_SHARE_PERMILLE: u32 = 400;
/// 千分比刻度。
pub const PERMILLE: u32 = 1_000;
/// P99.9 切点刻度。
pub const TAIL_CUT_PERMILLE: u32 = 999;
/// 长尾热窗默认容量：P99.9 在 12 000 样本流上只留 12 条，
/// 热窗取 8 条（最近的证据优先），余者按淘汰记账。
pub const STORE_CAP: usize = 8;
/// 长尾窗口容量上限。
pub const CAP_LIMIT: usize = 4_096;
/// 回放次数（判据口径 ×10^2）。
pub const REPLAYS: u32 = 100;
/// 合成流每段样本数。
pub const STREAM_PER_SEG: u32 = 3_000;
/// 尾部样本概率分母（1/64 的样本进入长尾）。
pub const TAIL_ONE_IN: u32 = 64;
/// 尾部放大倍数。
pub const TAIL_FACTOR: u32 = 8;
/// 日号空间（十天）。
pub const DAYS: u32 = 10;
/// 账行字段数。
pub const ROW_FIELDS: usize = 6;
/// 风暴账行标签。
pub const STORM_TAG: &str = "STRM";
/// 长尾导出标签。
pub const TAIL_TAG: &str = "TAILEX";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B14 风暴/长尾诊断码";
/// 段名（下标即段号）。
pub const SEG_NAMES: [&str; SEGMENTS] = ["interrupt", "dispatch", "compose", "present"];
/// 风暴行数字键（固定列序）。
pub const ROW_KEYS: [&str; 4] = ["level", "seg", "excess", "hits"];
/// 长尾行数字键（固定列序）。
pub const TAIL_KEYS: [&str; 4] = ["seq", "seg", "ns", "day"];

// ── 诊断码 ───────────────────────────────────────────────────────
/// 风暴/长尾诊断码（14 条，互不共用说明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StormCode {
    /// 注入等级越出 1..10
    LevelBad,
    /// 段号越出 0..3
    SegBad,
    /// 样本计时非法（0 或超过 1 秒）
    SampleBad,
    /// 空窗口/空流，分位与切点无定义域
    EmptyStore,
    /// 窗口容量为 0 或超过上限
    CapacityBad,
    /// 样本序号未严格递增
    SeqBad,
    /// 波次配置为 0 或超过标准波次
    BurstBad,
    /// 落点权重表退化（合计为 0）
    WeightBad,
    /// 归因报告字段与实测不符或主导段并列
    ReportBad,
    /// 回放摘要与入库摘要不一致
    ReplayBad,
    /// 导出十轮不一致或导出行不可回读
    ExportBad,
    /// 账行字段缺失、键序错位或往返不等
    RenderBad,
    /// 淘汰计数与入库窗口不守恒
    EvictBad,
    /// 计数累计溢出
    Overflow,
}

impl StormCode {
    /// 诊断码值：段基址 + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码中文说明（不得共用同一句）。
    pub fn detail(self) -> &'static str {
        match self {
            StormCode::LevelBad => "注入等级不在 L01..L10，风暴无刻度",
            StormCode::SegBad => "段号不在 0..3，延迟无法归属延迟链",
            StormCode::SampleBad => "样本计时为 0 或超过 1 秒上限",
            StormCode::EmptyStore => "长尾窗口为空，切点与回放无定义域",
            StormCode::CapacityBad => "窗口容量为 0 或超过 4096 上限",
            StormCode::SeqBad => "样本序号未严格递增，取证链断点",
            StormCode::BurstBad => "波次配置为 0 或超过标准波次",
            StormCode::WeightBad => "落点权重合计为 0，抽段无分布",
            StormCode::ReportBad => "归因字段与实测不符或主导段并列",
            StormCode::ReplayBad => "回放摘要与入库摘要不一致",
            StormCode::ExportBad => "导出十轮不一致或导出行不可回读",
            StormCode::RenderBad => "账行键序错位、数字非法或往返不等",
            StormCode::EvictBad => "淘汰计数与入库窗口不守恒",
            StormCode::Overflow => "中断或样本计数累计溢出",
        }
    }

    /// 14 个诊断码全集（顺序即变体顺序）。
    pub const ALL: [StormCode; 14] = [
        StormCode::LevelBad,
        StormCode::SegBad,
        StormCode::SampleBad,
        StormCode::EmptyStore,
        StormCode::CapacityBad,
        StormCode::SeqBad,
        StormCode::BurstBad,
        StormCode::WeightBad,
        StormCode::ReportBad,
        StormCode::ReplayBad,
        StormCode::ExportBad,
        StormCode::RenderBad,
        StormCode::EvictBad,
        StormCode::Overflow,
    ];
}

/// 自由函数版诊断说明，越界返回 [`UNKNOWN`]。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x0000 => UNKNOWN,
        _ => {
            let idx = code.wrapping_sub(SEG_BASE + 1);
            match StormCode::ALL.get(idx as usize) {
                Some(c) => c.detail(),
                None => UNKNOWN,
            }
        }
    }
}

/// 段位守卫：14 码恰好占满 [SEG_BASE+1, SEG_BASE+14]，且互不重复。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0E;
    let mut i = 0usize;
    while i < StormCode::ALL.len() {
        let v = match StormCode::ALL.get(i) {
            Some(c) => c.val(),
            None => return false,
        };
        if v < lo || v > hi {
            return false;
        }
        let mut j = i + 1;
        while j < StormCode::ALL.len() {
            if let Some(other) = StormCode::ALL.get(j) {
                if other.val() == v {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 段与权重 ─────────────────────────────────────────────────────
/// 段名；越界返回 `"?"`。
pub fn seg_name(i: u8) -> &'static str {
    match SEG_NAMES.get(i as usize) {
        Some(v) => v,
        None => "?",
    }
}

/// 段基准延迟；越界返回 SegBad。
pub fn seg_base_ns(i: u8) -> Result<u32, u32> {
    match STORM_BASE_NS.get(i as usize) {
        Some(v) => Ok(*v),
        None => Err(StormCode::SegBad.val()),
    }
}

/// 段预算；越界返回 SegBad。
pub fn seg_budget_ns(i: u8) -> Result<u32, u32> {
    match SEG_BUDGET_NS.get(i as usize) {
        Some(v) => Ok(*v),
        None => Err(StormCode::SegBad.val()),
    }
}

/// 段落点权重；越界返回 SegBad。
pub fn seg_weight(i: u8) -> Result<u32, u32> {
    match LOCALITY.get(i as usize) {
        Some(v) => Ok(*v),
        None => Err(StormCode::SegBad.val()),
    }
}

/// 权重合计（先验分布的归一化分母）。
pub fn locality_sum() -> u32 {
    let mut s = 0u32;
    let mut i = 0usize;
    while i < LOCALITY.len() {
        if let Some(v) = LOCALITY.get(i) {
            s = s.saturating_add(*v);
        }
        i += 1;
    }
    s
}

/// 按累计权重把 `r ∈ [0, locality_sum)` 映射到段号。
pub fn pick_segment(r: u32) -> Result<u8, u32> {
    let total = locality_sum();
    if total == 0 {
        return Err(StormCode::WeightBad.val());
    }
    if r >= total {
        return Err(StormCode::WeightBad.val());
    }
    let mut acc = 0u32;
    let mut i = 0usize;
    while i < LOCALITY.len() {
        let w = match LOCALITY.get(i) {
            Some(v) => *v,
            None => return Err(StormCode::SegBad.val()),
        };
        acc = acc.saturating_add(w);
        if r < acc {
            return Ok(i as u8);
        }
        i += 1;
    }
    Err(StormCode::WeightBad.val())
}

// ── 注入计划 ─────────────────────────────────────────────────────
/// 单级注入计划：等级 + 波次 + 每波中断数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StormPlan {
    /// 注入等级（1..=10）。
    pub level: u32,
    /// 波次数。
    pub bursts: u32,
    /// 每波中断数。
    pub per_burst: u32,
}

impl StormPlan {
    /// 按等级取标准计划。
    pub fn for_level(level: u32) -> Result<StormPlan, u32> {
        if level == 0 || level > LEVELS {
            return Err(StormCode::LevelBad.val());
        }
        Ok(StormPlan {
            level,
            bursts: BURSTS,
            per_burst: PER_BURST,
        })
    }

    /// 标准总中断数（波次 × 每波）。
    pub fn load(&self) -> Result<u32, u32> {
        if self.bursts == 0 || self.bursts > BURSTS {
            return Err(StormCode::BurstBad.val());
        }
        if self.per_burst == 0 || self.per_burst > PER_BURST {
            return Err(StormCode::BurstBad.val());
        }
        self.bursts
            .checked_mul(self.per_burst)
            .ok_or(StormCode::Overflow.val())
    }

    /// 按等级缩放后的实际注入中断数。
    pub fn scaled_load(&self) -> Result<u32, u32> {
        let load = self.load()?;
        let scaled = load
            .checked_mul(self.level)
            .ok_or(StormCode::Overflow.val())?;
        scaled
            .checked_div(LEVELS)
            .ok_or(StormCode::LevelBad.val())
    }
}

// ── 风暴仿真 ─────────────────────────────────────────────────────
/// 单级风暴结果：命中次数 / 累计注入延迟 / 越预算次数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StormRun {
    /// 注入等级。
    pub level: u32,
    /// 波次数。
    pub bursts: u32,
    /// 每段落点次数。
    pub hits: [u32; SEGMENTS],
    /// 每段累计注入延迟（纳秒）。
    pub excess: [u32; SEGMENTS],
    /// 每段越预算次数（毛刺）。
    pub over: [u32; SEGMENTS],
    /// 结果摘要。
    pub digest: u64,
}

impl StormRun {
    /// 累计注入延迟。
    pub fn total_excess(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < SEGMENTS {
            if let Some(v) = self.excess.get(i) {
                s = s.saturating_add(*v);
            }
            i += 1;
        }
        s
    }

    /// 累计命中次数。
    pub fn total_hits(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < SEGMENTS {
            if let Some(v) = self.hits.get(i) {
                s = s.saturating_add(*v);
            }
            i += 1;
        }
        s
    }

    /// 累计毛刺次数。
    pub fn total_over(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < SEGMENTS {
            if let Some(v) = self.over.get(i) {
                s = s.saturating_add(*v);
            }
            i += 1;
        }
        s
    }

    /// 主导段：唯一最大注入段；并列或无注入一律 ReportBad。
    pub fn dominant(&self) -> Result<u8, u32> {
        let mut best = 0usize;
        let mut best_v = 0u32;
        let mut ties = 0u32;
        let mut i = 0usize;
        while i < SEGMENTS {
            let v = match self.excess.get(i) {
                Some(v) => *v,
                None => return Err(StormCode::SegBad.val()),
            };
            if v > best_v {
                best_v = v;
                best = i;
                ties = 0;
            } else if v == best_v && v != 0 {
                ties = ties.saturating_add(1);
            }
            i += 1;
        }
        if best_v == 0 {
            return Err(StormCode::ReportBad.val());
        }
        if ties > 0 {
            return Err(StormCode::ReportBad.val());
        }
        Ok(best as u8)
    }

    /// 段份额（千分比，向下取整）。
    pub fn share_permille(&self, seg: u8) -> u32 {
        let total = self.total_excess();
        if total == 0 {
            return 0;
        }
        let part = match self.excess.get(seg as usize) {
            Some(v) => *v,
            None => return 0,
        };
        let bp = (part as u64)
            .saturating_mul(PERMILLE as u64)
            .checked_div(total as u64)
            .unwrap_or(0);
        if bp > u32::MAX as u64 {
            u32::MAX
        } else {
            bp as u32
        }
    }

    /// 定位判据：主导段为中断段且份额 ≥ 400‰。
    pub fn localized(&self) -> bool {
        match self.dominant() {
            Ok(d) => d == 0 && self.share_permille(0) >= DOMINANT_SHARE_PERMILLE,
            Err(_) => false,
        }
    }

    /// 摘要自洽：存储摘要等于按字段重算值。
    pub fn verify(&self) -> bool {
        storm_digest(
            self.level,
            self.bursts,
            &self.hits,
            &self.excess,
            &self.over,
        ) == self.digest
    }
}

/// 风暴摘要：折等级、波次与四段的命中/注入/越预算三联。
pub fn storm_digest(
    level: u32,
    bursts: u32,
    hits: &[u32; SEGMENTS],
    excess: &[u32; SEGMENTS],
    over: &[u32; SEGMENTS],
) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, level);
    h = p_core::fold_u32(h, bursts);
    let mut i = 0usize;
    while i < SEGMENTS {
        if let (Some(a), Some(b), Some(c)) = (hits.get(i), excess.get(i), over.get(i)) {
            h = p_core::fold_byte(h, i as u8);
            h = p_core::fold_u32(h, *a);
            h = p_core::fold_u32(h, *b);
            h = p_core::fold_u32(h, *c);
        }
        i += 1;
    }
    h
}

/// 跑一级风暴注入。
///
/// 抽段与抖动共用一条 LCG，但两者取不同高位：抽段用 `>> 32`，
/// 抖动用 `>> 40`。同一条流里若两个用途都取低位，LCG 的低位周期短，
/// 会出现"抽到中断段的同时抖动也恒小"的伪相关，主导份额被系统性抬高。
pub fn storm_run(seed: u64, level: u32) -> Result<StormRun, u32> {
    let plan = StormPlan::for_level(level)?;
    let total = plan.scaled_load()?;
    if total == 0 {
        return Err(StormCode::BurstBad.val());
    }
    let mix = ((level as u64) << 32) ^ 0x5354_4F52_4D31;
    let mut rng = p_core::Lcg::new(seed ^ mix);
    let mut hits = [0u32; SEGMENTS];
    let mut excess = [0u32; SEGMENTS];
    let mut over = [0u32; SEGMENTS];
    let mut n = 0u32;
    while n < total {
        let r = ((rng.next() >> 32) as u32) % locality_sum();
        let seg = pick_segment(r)? as usize;
        let w = match LOCALITY.get(seg) {
            Some(v) => *v,
            None => return Err(StormCode::SegBad.val()),
        };
        let extra = level
            .checked_mul(w)
            .and_then(|v| v.checked_mul(LEVEL_STEP_NS))
            .ok_or(StormCode::Overflow.val())?;
        let base = match STORM_BASE_NS.get(seg) {
            Some(v) => *v,
            None => return Err(StormCode::SegBad.val()),
        };
        let jitter = ((rng.next() >> 40) as u32) % (base / 8 + 1);
        let cost = base.saturating_add(extra).saturating_add(jitter);
        match excess.get_mut(seg) {
            Some(e) => *e = e.saturating_add(extra),
            None => return Err(StormCode::SegBad.val()),
        }
        match hits.get_mut(seg) {
            Some(h) => *h = h.saturating_add(1),
            None => return Err(StormCode::SegBad.val()),
        }
        let budget = match SEG_BUDGET_NS.get(seg) {
            Some(v) => *v,
            None => return Err(StormCode::SegBad.val()),
        };
        if cost > budget {
            match over.get_mut(seg) {
                Some(o) => *o = o.saturating_add(1),
                None => return Err(StormCode::SegBad.val()),
            }
        }
        n = n.saturating_add(1);
    }
    let digest = storm_digest(level, plan.bursts, &hits, &excess, &over);
    Ok(StormRun {
        level,
        bursts: plan.bursts,
        hits,
        excess,
        over,
        digest,
    })
}

/// 十级风暴全跑（顺序即等级序）。
pub fn storm_report(seed: u64) -> Result<Vec<StormRun>, u32> {
    let mut out: Vec<StormRun> = Vec::with_capacity(LEVELS as usize);
    let mut lv = 1u32;
    while lv <= LEVELS {
        out.push(storm_run(seed, lv)?);
        lv += 1;
    }
    Ok(out)
}

/// 十级复跑零漂移：每级重跑 `times` 次，摘要与总注入必须逐级恒定。
pub fn storm_replay(seed: u64, times: u32) -> Result<bool, u32> {
    if times == 0 {
        return Err(StormCode::ReplayBad.val());
    }
    let base = storm_report(seed)?;
    let mut t = 0u32;
    while t < times {
        let again = storm_report(seed)?;
        if again.len() != base.len() {
            return Ok(false);
        }
        let mut i = 0usize;
        while i < base.len() {
            match (base.get(i), again.get(i)) {
                (Some(a), Some(b)) => {
                    if a.digest != b.digest
                        || a.total_excess() != b.total_excess()
                        || a.level != b.level
                    {
                        return Ok(false);
                    }
                }
                _ => return Ok(false),
            }
            i += 1;
        }
        t += 1;
    }
    Ok(true)
}

// ── 账行格式（六字段） ───────────────────────────────────────────
/// 风暴行摘要：折等级、段号、总注入与总命中。
pub fn row_digest(level: u32, seg: u32, excess: u32, hits: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, level);
    h = p_core::fold_u32(h, seg);
    h = p_core::fold_u32(h, excess);
    h = p_core::fold_u32(h, hits);
    h
}

/// 渲染六字段风暴行：`STRM|level=|seg=|excess=|hits=|digest=`。
pub fn render_row(
    tag: &str,
    level: u32,
    seg: u32,
    excess: u32,
    hits: u32,
    digest: u64,
) -> String {
    format!(
        "{}|level={}|seg={}|excess={}|hits={}|digest={:016X}",
        tag, level, seg, excess, hits, digest
    )
}

/// 十进制 u32 解析（拒绝空串/前导非数字/溢出）。
pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

/// 16 位十六进制解析（大小写皆可）。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

/// 拆 `key=value`，键值皆非空且不得含第二个 `=`。
fn split_kv(p: &str) -> Option<(&str, &str)> {
    let mut it = p.split('=');
    let k = it.next()?;
    let v = it.next()?;
    if it.next().is_some() {
        return None;
    }
    if k.is_empty() || v.is_empty() {
        return None;
    }
    Some((k, v))
}

/// 按固定键序解析六字段行：(tag, [四数字], digest)。
pub fn fields_with_keys<'a>(
    row: &'a str,
    keys: &[&str; 4],
) -> Option<(&'a str, [u32; 4], u64)> {
    let mut parts: [Option<&str>; ROW_FIELDS] = [None; ROW_FIELDS];
    let mut n = 0usize;
    for p in row.split('|') {
        if n >= ROW_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != ROW_FIELDS {
        return None;
    }
    let tag = match parts.first().and_then(|x| *x) {
        Some(v) if !v.is_empty() => v,
        _ => return None,
    };
    let mut vals = [0u32; 4];
    let mut i = 0usize;
    while i < 4 {
        let p = match parts.get(i + 1).and_then(|x| *x) {
            Some(v) => v,
            None => return None,
        };
        let (k, v) = split_kv(p)?;
        if k != *keys.get(i)? {
            return None;
        }
        let num = parse_u32(v)?;
        if let Some(slot) = vals.get_mut(i) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5).and_then(|x| *x) {
        Some(v) => v,
        None => return None,
    };
    let (k, v) = split_kv(dpart)?;
    if k != "digest" {
        return None;
    }
    let digest = parse_hex16(v)?;
    Some((tag, vals, digest))
}

/// 风暴行解析。
pub fn row_fields<'a>(row: &'a str) -> Option<(&'a str, [u32; 4], u64)> {
    fields_with_keys(row, &ROW_KEYS)
}

/// 长尾导出行的数字字段解析。
pub fn tail_fields<'a>(row: &'a str) -> Option<(&'a str, [u32; 4], u64)> {
    fields_with_keys(row, &TAIL_KEYS)
}

/// 风暴行审计：键序 + 等级/段界 + 摘要重算 + 往返恒等，四关齐过才算数。
pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, v, digest)) => {
            let level = v[0];
            let seg = v[1];
            level >= 1
                && level <= LEVELS
                && seg < SEGMENTS as u32
                && digest == row_digest(level, seg, v[2], v[3])
                && render_row(tag, level, seg, v[2], v[3], digest) == row
        }
        None => false,
    }
}

/// 由一轮结果渲染主导行；主导段并列时返回 ReportBad。
pub fn report_row(run: &StormRun) -> Result<String, u32> {
    if !run.verify() {
        return Err(StormCode::ReportBad.val());
    }
    let d = run.dominant()?;
    let level = run.level;
    let total = run.total_excess();
    let hits = run.total_hits();
    let digest = row_digest(level, d as u32, total, hits);
    Ok(render_row(STORM_TAG, level, d as u32, total, hits, digest))
}

/// 十级报告行集合。
pub fn report_rows(runs: &[StormRun]) -> Result<Vec<String>, u32> {
    if runs.len() != LEVELS as usize {
        return Err(StormCode::ReportBad.val());
    }
    let mut out: Vec<String> = Vec::with_capacity(runs.len());
    let mut i = 0usize;
    while i < runs.len() {
        let r = match runs.get(i) {
            Some(v) => v,
            None => return Err(StormCode::ReportBad.val()),
        };
        out.push(report_row(r)?);
        i += 1;
    }
    Ok(out)
}

// ── 长尾样本 ─────────────────────────────────────────────────────
/// 长尾取证样本：全局序号 + 段 + 日 + 计时。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TailSample {
    /// 全局递增序号。
    pub seq: u32,
    /// 段号。
    pub seg: u8,
    /// 日号（0..9）。
    pub day: u8,
    /// 计时（纳秒）。
    pub ns: u32,
}

impl TailSample {
    /// 校验并构造。
    pub fn new(seq: u32, seg: u8, day: u8, ns: u32) -> Result<TailSample, u32> {
        if (seg as usize) >= SEGMENTS {
            return Err(StormCode::SegBad.val());
        }
        if (day as u32) >= DAYS {
            return Err(StormCode::SampleBad.val());
        }
        if ns == 0 || ns > NS_LIMIT {
            return Err(StormCode::SampleBad.val());
        }
        Ok(TailSample { seq, seg, day, ns })
    }

    /// 样本摘要。
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.seq);
        h = p_core::fold_byte(h, self.seg);
        h = p_core::fold_byte(h, self.day);
        h = p_core::fold_u32(h, self.ns);
        h
    }
}

/// 把 64 位摘要折入 64 位累加器（高低两半各折一次）。
fn fold_digest(h: u64, v: u64) -> u64 {
    p_core::fold_u32(p_core::fold_u32(h, (v >> 32) as u32), v as u32)
}

/// 复制排序（不改动输入）。
pub fn sort_copy(v: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::with_capacity(v.len());
    out.extend_from_slice(v);
    out.sort_unstable();
    out
}

/// P99.9 切点：最近秩法，rank = ceil(999×N/1000)，索引从 1 起。
pub fn tail_cut(sorted: &[u32]) -> Result<u32, u32> {
    if sorted.is_empty() {
        return Err(StormCode::EmptyStore.val());
    }
    let n = sorted.len() as u64;
    let num = (TAIL_CUT_PERMILLE as u64)
        .checked_mul(n)
        .ok_or(StormCode::Overflow.val())?;
    let den = PERMILLE as u64;
    let rank = num.checked_add(den - 1).ok_or(StormCode::Overflow.val())? / den;
    let idx = if rank == 0 { 0 } else { rank - 1 };
    let idx = idx.min(sorted.len() as u64 - 1);
    match sorted.get(idx as usize) {
        Some(v) => Ok(*v),
        None => Err(StormCode::EmptyStore.val()),
    }
}

/// 窗口快照：行数 + 摘要 + 首末序号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Replay {
    /// 窗口内样本数。
    pub rows: u32,
    /// 窗口链摘要。
    pub digest: u64,
    /// 首个样本序号。
    pub first_seq: u32,
    /// 末个样本序号。
    pub last_seq: u32,
}

/// 长尾取证窗口：容量受限的按序样本窗口 + 全量提交计数。
#[derive(Clone, Debug)]
pub struct TailStore {
    cap: usize,
    entries: Vec<TailSample>,
    offered: u32,
    captured: u32,
    dropped: u32,
    evicted: u32,
    last_seq: u32,
    has_last: bool,
    digest: u64,
}

impl TailStore {
    /// 建空窗口。
    pub fn new(cap: usize) -> Result<TailStore, u32> {
        if cap == 0 || cap > CAP_LIMIT {
            return Err(StormCode::CapacityBad.val());
        }
        Ok(TailStore {
            cap,
            entries: Vec::new(),
            offered: 0,
            captured: 0,
            dropped: 0,
            evicted: 0,
            last_seq: 0,
            has_last: false,
            digest: p_core::FNV_OFF,
        })
    }

    /// 容量。
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 窗口内样本数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 提交总数（合法提交，含被切点挡下的）。
    pub fn offered(&self) -> u32 {
        self.offered
    }

    /// 入册数。
    pub fn captured(&self) -> u32 {
        self.captured
    }

    /// 切点挡下数。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 淘汰数。
    pub fn evicted(&self) -> u32 {
        self.evicted
    }

    /// 窗口链摘要。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 窗口样本只读。
    pub fn entry(&self, i: usize) -> Option<&TailSample> {
        self.entries.get(i)
    }

    /// 窗口链摘要重算（纯函数，供回放与自检共用）。
    pub fn recompute(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(s) = self.entries.get(i) {
                h = fold_digest(h, s.digest());
            }
            i += 1;
        }
        h
    }

    /// 守恒自检：库存 + 淘汰 = 入册；入册 + 挡下 = 提交；摘要 = 重算。
    pub fn verify(&self) -> bool {
        if self.cap == 0 || self.cap > CAP_LIMIT {
            return false;
        }
        if self.entries.len() > self.cap {
            return false;
        }
        let stock = match self.captured.checked_sub(self.evicted) {
            Some(v) => v,
            None => return false,
        };
        if stock as usize != self.entries.len() {
            return false;
        }
        match self.captured.checked_add(self.dropped) {
            Some(v) if v == self.offered => {}
            _ => return false,
        }
        self.digest == self.recompute()
    }

    /// 提交一个样本；低于切点返回 `Ok(false)` 并计挡下。
    pub fn push(&mut self, s: TailSample, cut_ns: u32) -> Result<bool, u32> {
        if cut_ns == 0 || cut_ns > NS_LIMIT {
            return Err(StormCode::SampleBad.val());
        }
        let checked = TailSample::new(s.seq, s.seg, s.day, s.ns)?;
        if self.has_last && checked.seq <= self.last_seq {
            return Err(StormCode::SeqBad.val());
        }
        self.offered = self
            .offered
            .checked_add(1)
            .ok_or(StormCode::Overflow.val())?;
        self.last_seq = checked.seq;
        self.has_last = true;
        if checked.ns < cut_ns {
            self.dropped = self
                .dropped
                .checked_add(1)
                .ok_or(StormCode::Overflow.val())?;
            return Ok(false);
        }
        while self.entries.len() >= self.cap {
            self.entries.remove(0);
            self.evicted = self
                .evicted
                .checked_add(1)
                .ok_or(StormCode::Overflow.val())?;
        }
        self.entries.push(checked);
        self.captured = self
            .captured
            .checked_add(1)
            .ok_or(StormCode::Overflow.val())?;
        self.digest = self.recompute();
        Ok(true)
    }

    /// 回放：重算窗口摘要并核对序号严格递增。
    pub fn replay(&self) -> Result<Replay, u32> {
        if self.entries.is_empty() {
            return Err(StormCode::EmptyStore.val());
        }
        let first = match self.entries.first() {
            Some(v) => *v,
            None => return Err(StormCode::EmptyStore.val()),
        };
        let last = match self.entries.last() {
            Some(v) => *v,
            None => return Err(StormCode::EmptyStore.val()),
        };
        let mut prev = first.seq;
        let mut i = 1usize;
        while i < self.entries.len() {
            let s = match self.entries.get(i) {
                Some(v) => v,
                None => return Err(StormCode::ReplayBad.val()),
            };
            if s.seq <= prev {
                return Err(StormCode::SeqBad.val());
            }
            prev = s.seq;
            i += 1;
        }
        let digest = self.recompute();
        if digest != self.digest {
            return Err(StormCode::ReplayBad.val());
        }
        Ok(Replay {
            rows: self.entries.len() as u32,
            digest,
            first_seq: first.seq,
            last_seq: last.seq,
        })
    }

    /// 百次回放一致：每轮摘要与行数都必须与首轮相同。
    pub fn replay_consistent(&self, times: u32) -> bool {
        if times == 0 {
            return false;
        }
        let base = match self.replay() {
            Ok(v) => v,
            Err(_) => return false,
        };
        let mut t = 0u32;
        while t < times {
            match self.replay() {
                Ok(v) => {
                    if v != base {
                        return false;
                    }
                }
                Err(_) => return false,
            }
            t += 1;
        }
        true
    }

    /// 序号严格递增（窗口内）。
    pub fn ascending(&self) -> bool {
        let mut i = 1usize;
        while i < self.entries.len() {
            match (self.entries.get(i - 1), self.entries.get(i)) {
                (Some(a), Some(b)) => {
                    if b.seq <= a.seq {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 窗口内所有样本计时 ≥ 切点。
    pub fn all_at_least(&self, cut_ns: u32) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(s) => {
                    if s.ns < cut_ns {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

// ── 长尾导出（O1 联签列） ────────────────────────────────────────
/// 长尾行摘要：折序号、段、计时与日。
pub fn tail_row_digest(seq: u32, seg: u32, ns: u32, day: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, seq);
    h = p_core::fold_u32(h, seg);
    h = p_core::fold_u32(h, ns);
    h = p_core::fold_u32(h, day);
    h
}

/// 渲染长尾导出行：`TAILEX|seq=|seg=|ns=|day=|digest=`。
pub fn render_tail_row(seq: u32, seg: u32, ns: u32, day: u32, digest: u64) -> String {
    format!(
        "{}|seq={}|seg={}|ns={}|day={}|digest={:016X}",
        TAIL_TAG, seq, seg, ns, day, digest
    )
}

/// 由样本渲染导出行。
pub fn export_row(s: &TailSample) -> String {
    let digest = tail_row_digest(s.seq, s.seg as u32, s.ns, s.day as u32);
    render_tail_row(s.seq, s.seg as u32, s.ns, s.day as u32, digest)
}

/// 长尾导出行审计：标签 + 键序 + 段/日界 + 摘要重算 + 往返恒等。
pub fn audit_tail_row(row: &str) -> bool {
    match tail_fields(row) {
        Some((tag, v, digest)) => {
            let seq = v[0];
            let seg = v[1];
            let ns = v[2];
            let day = v[3];
            tag == TAIL_TAG
                && seg < SEGMENTS as u32
                && day < DAYS
                && ns > 0
                && ns <= NS_LIMIT
                && digest == tail_row_digest(seq, seg, ns, day)
                && render_tail_row(seq, seg, ns, day, digest) == row
        }
        None => false,
    }
}

/// 窗口导出行集合（按窗口顺序）。
pub fn export_rows(store: &TailStore) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(store.len());
    let mut i = 0usize;
    while i < store.len() {
        if let Some(s) = store.entry(i) {
            out.push(export_row(s));
        }
        i += 1;
    }
    out
}

/// 窗口导出摘要：对导出行序贯折行摘要。
pub fn export_digest(store: &TailStore) -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < store.len() {
        if let Some(s) = store.entry(i) {
            h = fold_digest(h, tail_row_digest(s.seq, s.seg as u32, s.ns, s.day as u32));
        }
        i += 1;
    }
    h
}

/// 导出十轮终验一致：逐轮重算摘要与行数。
pub fn export_stable(store: &TailStore, times: u32) -> bool {
    if times == 0 || store.is_empty() {
        return false;
    }
    let base = export_digest(store);
    let rows = store.len();
    let mut t = 0u32;
    while t < times {
        if export_digest(store) != base {
            return false;
        }
        let again = export_rows(store);
        if again.len() != rows {
            return false;
        }
        let mut i = 0usize;
        while i < again.len() {
            match again.get(i) {
                Some(r) => {
                    if !audit_tail_row(r) {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        t += 1;
    }
    base != 0
}

// ── 合成流与取证 ─────────────────────────────────────────────────
/// 合成全量流：四段 × 每段样本数，1/64 样本进入 8 倍长尾。
pub fn synth_stream(seed: u64, per_seg: u32) -> Result<Vec<TailSample>, u32> {
    if per_seg == 0 || per_seg > STREAM_PER_SEG {
        return Err(StormCode::SampleBad.val());
    }
    let total = (SEGMENTS as u32)
        .checked_mul(per_seg)
        .ok_or(StormCode::Overflow.val())?;
    let mut rng = p_core::Lcg::new(seed ^ 0x5441_494C_5354_5231);
    let mut out: Vec<TailSample> = Vec::with_capacity(total as usize);
    let mut seg = 0u8;
    while (seg as usize) < SEGMENTS {
        let base = match STORM_BASE_NS.get(seg as usize) {
            Some(v) => *v,
            None => return Err(StormCode::SegBad.val()),
        };
        let mut k = 0u32;
        while k < per_seg {
            let seq = (seg as u32)
                .checked_mul(per_seg)
                .and_then(|v| v.checked_add(k))
                .ok_or(StormCode::Overflow.val())?;
            let jitter = ((rng.next() >> 40) as u32) % (base / 4 + 1);
            let tail = ((rng.next() >> 36) as u32) % TAIL_ONE_IN == 0;
            let ns = if tail {
                base.saturating_mul(TAIL_FACTOR).saturating_add(jitter)
            } else {
                base.saturating_add(jitter)
            };
            let day = (k % DAYS) as u8;
            out.push(TailSample::new(seq, seg, day, ns)?);
            k = k.saturating_add(1);
        }
        seg = seg.saturating_add(1);
    }
    Ok(out)
}

/// 取证结果：切点 + 窗口。
#[derive(Clone, Debug)]
pub struct Capture {
    /// P99.9 切点（纳秒）。
    pub cut_ns: u32,
    /// 受限窗口。
    pub store: TailStore,
}

/// 全量流取证：先定 P99.9 切点，再按序入库。
pub fn capture_stream(stream: &[TailSample], cap: usize) -> Result<Capture, u32> {
    if stream.is_empty() {
        return Err(StormCode::EmptyStore.val());
    }
    let mut ns: Vec<u32> = Vec::with_capacity(stream.len());
    let mut i = 0usize;
    while i < stream.len() {
        match stream.get(i) {
            Some(s) => ns.push(s.ns),
            None => return Err(StormCode::EmptyStore.val()),
        }
        i += 1;
    }
    ns.sort_unstable();
    let cut_ns = tail_cut(&ns)?;
    let mut store = TailStore::new(cap)?;
    let mut j = 0usize;
    while j < stream.len() {
        match stream.get(j) {
            Some(s) => {
                store.push(*s, cut_ns)?;
            }
            None => return Err(StormCode::EmptyStore.val()),
        }
        j += 1;
    }
    Ok(Capture { cut_ns, store })
}

/// 便捷入口：合成 + 取证一次完成。
pub fn capture_synth(seed: u64, per_seg: u32, cap: usize) -> Result<Capture, u32> {
    let stream = synth_stream(seed, per_seg)?;
    capture_stream(&stream, cap)
}

/// 取证复跑一致：同种子同容量两次取证的切点/摘要/计数必须全等。
pub fn capture_replay(seed: u64, per_seg: u32, cap: usize, times: u32) -> Result<bool, u32> {
    if times == 0 {
        return Err(StormCode::ReplayBad.val());
    }
    let base = capture_synth(seed, per_seg, cap)?;
    let mut t = 0u32;
    while t < times {
        let again = capture_synth(seed, per_seg, cap)?;
        if again.cut_ns != base.cut_ns
            || again.store.digest() != base.store.digest()
            || again.store.captured() != base.store.captured()
            || again.store.evicted() != base.store.evicted()
            || again.store.dropped() != base.store.dropped()
            || again.store.len() != base.store.len()
        {
            return Ok(false);
        }
        t += 1;
    }
    Ok(true)
}

// ── 判据八条 ─────────────────────────────────────────────────────
/// 八条判据（F49065 四条 + F49066 四条），全部为真才允许写回。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::with_capacity(8);

    // 1) 注入十级定级：十级全跑、总注入严格递增、每级摘要互异、越界拒绝。
    let mut ok = true;
    let mut last = 0u32;
    let mut digests: Vec<u64> = Vec::with_capacity(LEVELS as usize);
    let mut lv = 1u32;
    while lv <= LEVELS {
        match storm_run(0x6501, lv) {
            Ok(run) => {
                if run.level != lv || !run.verify() {
                    ok = false;
                }
                let total = run.total_excess();
                if total <= last {
                    ok = false;
                }
                last = total;
                if digests.contains(&run.digest) {
                    ok = false;
                }
                digests.push(run.digest);
            }
            Err(_) => ok = false,
        }
        lv += 1;
    }
    let rejects = StormPlan::for_level(0).is_err()
        && StormPlan::for_level(LEVELS + 1).is_err()
        && storm_run(0x6501, 0).is_err()
        && storm_run(0x6501, LEVELS + 1).is_err();
    ok = ok && rejects && digests.len() == LEVELS as usize;
    out.push((
        "F49065/注入十级定级",
        ok,
        format!("levels={} total={} reject={}", digests.len(), last, rejects),
    ));

    // 2) 分段定位：每级主导段唯一且为中断段，份额 ≥ 400‰，三段和守恒。
    let mut ok = true;
    let mut min_share = PERMILLE;
    let mut lv = 1u32;
    while lv <= LEVELS {
        match storm_run(0x6502, lv) {
            Ok(run) => {
                match run.dominant() {
                    Ok(d) if d == 0 => {}
                    _ => ok = false,
                }
                let share = run.share_permille(0);
                if share < DOMINANT_SHARE_PERMILLE {
                    ok = false;
                }
                if share < min_share {
                    min_share = share;
                }
                if run.total_hits() == 0 {
                    ok = false;
                }
                let mut sum = 0u32;
                let mut i = 0usize;
                while i < SEGMENTS {
                    if let Some(v) = run.excess.get(i) {
                        sum = sum.saturating_add(*v);
                    }
                    i += 1;
                }
                if sum != run.total_excess() {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        lv += 1;
    }
    out.push((
        "F49065/分段定位",
        ok,
        format!("min_share={}‰ dom=0", min_share),
    ));

    // 3) 报告成文：十行可渲染/可解析/可审计；改一位数字立即判红。
    let mut ok = true;
    let mut rows = 0u32;
    let mut tamper_caught = 0u32;
    match storm_report(0x6503).and_then(|runs| report_rows(&runs)) {
        Ok(list) => {
            let mut i = 0usize;
            while i < list.len() {
                match list.get(i) {
                    Some(row) => {
                        rows = rows.saturating_add(1);
                        if !audit_render(row) {
                            ok = false;
                        }
                        match row_fields(row) {
                            Some((_tag, v, d)) => {
                                let broken = render_row(
                                    STORM_TAG,
                                    v[0],
                                    v[1],
                                    v[2].saturating_add(1),
                                    v[3],
                                    d,
                                );
                                if audit_render(&broken) {
                                    ok = false;
                                } else {
                                    tamper_caught = tamper_caught.saturating_add(1);
                                }
                            }
                            None => ok = false,
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
        }
        Err(_) => ok = false,
    }
    let bad = [
        "",
        "STRM|level=1|seg=0|excess=1|hits=2",
        "STRM|level=1|seg=0|excess=1|hits=2|digest=0000000000000000",
        "STRM|seg=0|level=1|excess=1|hits=2|digest=0000000000000000",
        "STRM|level=0|seg=0|excess=1|hits=2|digest=0000000000000000",
        "STRM|level=1|seg=4|excess=1|hits=2|digest=0000000000000000",
        "STRM|level=1|seg=0|excess=x|hits=2|digest=0000000000000000",
        "|level=1|seg=0|excess=1|hits=2|digest=0000000000000000",
    ];
    let mut refused = 0u32;
    let mut k = 0usize;
    while k < bad.len() {
        if let Some(row) = bad.get(k) {
            if !audit_render(row) {
                refused = refused.saturating_add(1);
            }
        }
        k += 1;
    }
    ok = ok && rows == LEVELS && tamper_caught == LEVELS && refused == bad.len() as u32;
    out.push((
        "F49065/报告成文",
        ok,
        format!("rows={} tamper={} refuse={}", rows, tamper_caught, refused),
    ));

    // 4) 复跑零漂移：五次复跑十级全同；换种子后至少一级摘要改变。
    let mut ok = storm_replay(0x6505, 5).unwrap_or(false);
    let a = storm_run(0x6505, 5);
    let b = storm_run(0x6506, 5);
    let mut seed_moves = false;
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x.digest != y.digest {
                seed_moves = true;
            }
        }
        _ => ok = false,
    }
    ok = ok && seed_moves;
    out.push((
        "F49065/复跑零漂移",
        ok,
        format!("replay5={} seed_moves={}", ok, seed_moves),
    ));

    // 5) 长尾入库：切点先定、窗口内计时全 ≥ 切点、三账守恒、摘要自洽。
    let ok;
    let mut info = String::from("capture failed");
    match capture_synth(0x6601, STREAM_PER_SEG, STORE_CAP) {
        Ok(c) => {
            let cut = c.cut_ns;
            let st = &c.store;
            let stock = st.captured().saturating_sub(st.evicted());
            ok = st.verify()
                && cut > 0
                && st.all_at_least(cut)
                && st.ascending()
                && stock == st.len() as u32
                && st.offered() == st.captured().saturating_add(st.dropped())
                && st.evicted() > 0
                && st.captured() > 0;
            info = format!(
                "cut={} cap={} evict={} drop={}",
                cut,
                st.len(),
                st.evicted(),
                st.dropped()
            );
        }
        Err(_) => ok = false,
    }
    out.push(("F49066/长尾入库", ok, info));

    // 6) 回放百次：100 轮摘要/行数/首末序号全等；两次取证同摘要。
    let mut ok = true;
    let mut rows = 0u32;
    match capture_synth(0x6602, STREAM_PER_SEG, STORE_CAP) {
        Ok(c) => {
            match c.store.replay() {
                Ok(r) => {
                    rows = r.rows;
                    if r.digest != c.store.digest() || r.rows as usize != c.store.len() {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            if !c.store.replay_consistent(REPLAYS) {
                ok = false;
            }
        }
        Err(_) => ok = false,
    }
    if !capture_replay(0x6602, STREAM_PER_SEG, STORE_CAP, 3).unwrap_or(false) {
        ok = false;
    }
    out.push((
        "F49066/回放百次",
        ok,
        format!("replays={} rows={}", REPLAYS, rows),
    ));

    // 7) 导出十轮：十轮摘要一致、逐行可审计、摘要非零、行数与窗口等长。
    let mut ok = true;
    let mut info = String::from("capture failed");
    match capture_synth(0x6603, STREAM_PER_SEG, STORE_CAP) {
        Ok(c) => {
            let st = &c.store;
            let rows = st.len() as u32;
            let list = export_rows(st);
            if list.len() != st.len() {
                ok = false;
            }
            let mut i = 0usize;
            while i < list.len() {
                if let Some(row) = list.get(i) {
                    if !audit_tail_row(row) {
                        ok = false;
                    }
                }
                i += 1;
            }
            if !export_stable(st, 10) || export_digest(st) == 0 {
                ok = false;
            }
            info = format!("rows={} ten={}", rows, export_stable(st, 10));
        }
        Err(_) => ok = false,
    }
    out.push(("F49066/导出十轮", ok, info));

    // 8) 异常码守卫：14 码满窗、说明不越界、坏输入逐码命中、坏行全拒。
    let mut ok = segment_guard();
    let mut bits = String::new();
    if !ok {
        bits.push_str("guard red; ");
    }
    if detail(SEG_BASE) != UNKNOWN || detail(SEG_BASE + 15) != UNKNOWN {
        ok = false;
        bits.push_str("detail leak; ");
    }
    let codes_ok = StormPlan::for_level(0).err() == Some(StormCode::LevelBad.val())
        && StormPlan::for_level(LEVELS + 1).err() == Some(StormCode::LevelBad.val())
        && seg_budget_ns(4).err() == Some(StormCode::SegBad.val())
        && TailSample::new(0, 4, 0, 100).err() == Some(StormCode::SegBad.val())
        && TailSample::new(0, 0, 10, 100).err() == Some(StormCode::SampleBad.val())
        && TailSample::new(0, 0, 0, 0).err() == Some(StormCode::SampleBad.val())
        && TailSample::new(0, 0, 0, NS_LIMIT + 1).err() == Some(StormCode::SampleBad.val())
        && TailStore::new(0).is_err()
        && TailStore::new(CAP_LIMIT + 1).is_err()
        && tail_cut(&[]).err() == Some(StormCode::EmptyStore.val())
        && locality_sum() == 12;
    if !codes_ok {
        ok = false;
        bits.push_str("code miss; ");
    }
    let seq_guard = match TailStore::new(4) {
        Ok(mut st) => {
            let s1 = TailSample::new(10, 0, 0, 5_000);
            let s2 = TailSample::new(10, 0, 0, 5_001);
            let first = s1.map(|s| st.push(s, 1_000).unwrap_or(false)).unwrap_or(false);
            let dup = s2.map(|s| st.push(s, 1_000)).unwrap_or(Ok(false));
            first && dup.err() == Some(StormCode::SeqBad.val()) && st.verify()
        }
        Err(_) => false,
    };
    if !seq_guard {
        ok = false;
        bits.push_str("seq guard; ");
    }
    if ok && bits.is_empty() {
        bits = String::from("14 codes/guard/seq ok");
    }
    out.push(("F49066/异常码守卫", ok, bits));

    out
}

/// 模块入口：八条检查折叠为一个 CheckSet（域标签 `unxstar`）。
pub fn run_unx49065_checks() -> CheckSet {
    let mut set = CheckSet::new("unxstar");
    for (name, passed, detail_text) in extra_checks() {
        set.add_dyn(name, passed, &detail_text);
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn run(seed: u64, level: u32) -> StormRun {
        match storm_run(seed, level) {
            Ok(v) => v,
            Err(_) => StormRun {
                level,
                bursts: 0,
                hits: [0; SEGMENTS],
                excess: [0; SEGMENTS],
                over: [0; SEGMENTS],
                digest: 0,
            },
        }
    }

    fn sample(seq: u32, seg: u8, day: u8, ns: u32) -> TailSample {
        match TailSample::new(seq, seg, day, ns) {
            Ok(v) => v,
            Err(_) => TailSample {
                seq: 0,
                seg: 0,
                day: 0,
                ns: 1,
            },
        }
    }

    #[test]
    fn code_values_are_dense_window() {
        let mut seen = [false; 14];
        for c in StormCode::ALL {
            let off = c.val() - SEG_BASE;
            assert!(off >= 1 && off <= 14);
            let idx = (off - 1) as usize;
            assert!(!seen[idx]);
            seen[idx] = true;
        }
        assert!(seen.iter().all(|x| *x));
    }

    #[test]
    fn code_detail_nonempty_and_distinct() {
        let mut texts: Vec<&'static str> = Vec::new();
        for c in StormCode::ALL {
            let t = c.detail();
            assert!(!t.is_empty());
            assert!(!texts.contains(&t));
            texts.push(t);
        }
        assert_eq!(texts.len(), 14);
    }

    #[test]
    fn free_detail_bounds() {
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 15), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 1), StormCode::LevelBad.detail());
        assert_eq!(detail(SEG_BASE + 14), StormCode::Overflow.detail());
    }

    #[test]
    fn segment_guard_green() {
        assert!(segment_guard());
    }

    #[test]
    fn seg_names_table() {
        assert_eq!(seg_name(0), "interrupt");
        assert_eq!(seg_name(3), "present");
        assert_eq!(seg_name(4), "?");
        assert_eq!(seg_name(255), "?");
    }

    #[test]
    fn seg_tables_consistent() {
        for i in 0..SEGMENTS {
            assert!(seg_base_ns(i as u8).unwrap() <= seg_budget_ns(i as u8).unwrap());
            assert!(seg_weight(i as u8).unwrap() > 0);
        }
        assert_eq!(seg_base_ns(4), Err(StormCode::SegBad.val()));
        assert_eq!(seg_budget_ns(4), Err(StormCode::SegBad.val()));
        assert_eq!(seg_weight(4), Err(StormCode::SegBad.val()));
    }

    #[test]
    fn locality_sum_is_prior_total() {
        assert_eq!(locality_sum(), 12);
    }

    #[test]
    fn pick_segment_covers_weight_window() {
        let mut counts = [0u32; SEGMENTS];
        let mut r = 0u32;
        while r < locality_sum() {
            let s = pick_segment(r).expect("seg");
            counts[s as usize] += 1;
            r += 1;
        }
        for i in 0..SEGMENTS {
            assert_eq!(counts[i], LOCALITY[i]);
        }
    }

    #[test]
    fn pick_segment_boundaries() {
        assert_eq!(pick_segment(0), Ok(0));
        assert_eq!(pick_segment(5), Ok(0));
        assert_eq!(pick_segment(6), Ok(1));
        assert_eq!(pick_segment(8), Ok(1));
        assert_eq!(pick_segment(9), Ok(2));
        assert_eq!(pick_segment(10), Ok(2));
        assert_eq!(pick_segment(11), Ok(3));
        assert_eq!(pick_segment(12), Err(StormCode::WeightBad.val()));
        assert_eq!(pick_segment(u32::MAX), Err(StormCode::WeightBad.val()));
    }

    #[test]
    fn plan_for_level_bounds() {
        assert!(StormPlan::for_level(1).is_ok());
        assert!(StormPlan::for_level(LEVELS).is_ok());
        assert_eq!(StormPlan::for_level(0), Err(StormCode::LevelBad.val()));
        assert_eq!(
            StormPlan::for_level(LEVELS + 1),
            Err(StormCode::LevelBad.val())
        );
    }

    #[test]
    fn plan_load_and_scaled() {
        let p = StormPlan {
            level: 5,
            bursts: 4,
            per_burst: 8,
        };
        assert_eq!(p.load(), Ok(32));
        assert_eq!(p.scaled_load(), Ok(16));
    }

    #[test]
    fn plan_rejects_bad_bursts() {
        let a = StormPlan {
            level: 1,
            bursts: 0,
            per_burst: 8,
        };
        let b = StormPlan {
            level: 1,
            bursts: BURSTS + 1,
            per_burst: 8,
        };
        let c = StormPlan {
            level: 1,
            bursts: 1,
            per_burst: 0,
        };
        assert_eq!(a.load(), Err(StormCode::BurstBad.val()));
        assert_eq!(b.load(), Err(StormCode::BurstBad.val()));
        assert_eq!(c.load(), Err(StormCode::BurstBad.val()));
    }

    #[test]
    fn storm_run_rejects_bad_levels() {
        assert_eq!(storm_run(1, 0), Err(StormCode::LevelBad.val()));
        assert_eq!(storm_run(1, LEVELS + 1), Err(StormCode::LevelBad.val()));
    }

    #[test]
    fn storm_run_deterministic() {
        let a = run(0x7777, 4);
        let b = run(0x7777, 4);
        assert_eq!(a, b);
        assert!(a.verify());
    }

    #[test]
    fn storm_levels_have_distinct_digests() {
        let mut ds: Vec<u64> = Vec::new();
        let mut lv = 1u32;
        while lv <= LEVELS {
            ds.push(run(0x7778, lv).digest);
            lv += 1;
        }
        for i in 0..ds.len() {
            for j in (i + 1)..ds.len() {
                assert_ne!(ds[i], ds[j]);
            }
        }
    }

    #[test]
    fn storm_total_monotone_in_level() {
        let mut last = 0u32;
        let mut lv = 1u32;
        while lv <= LEVELS {
            let t = run(0x7779, lv).total_excess();
            assert!(t > last);
            last = t;
            lv += 1;
        }
    }

    #[test]
    fn storm_dominant_is_interrupt() {
        let mut lv = 1u32;
        while lv <= LEVELS {
            let r = run(0x777A, lv);
            assert_eq!(r.dominant(), Ok(0));
            assert!(r.localized());
            lv += 1;
        }
    }

    #[test]
    fn storm_share_above_floor() {
        let mut lv = 1u32;
        while lv <= LEVELS {
            let r = run(0x777B, lv);
            assert!(r.share_permille(0) >= DOMINANT_SHARE_PERMILLE);
            lv += 1;
        }
    }

    #[test]
    fn storm_hits_sum_matches_total() {
        let r = run(0x777C, 7);
        let mut s = 0u32;
        for i in 0..SEGMENTS {
            s += r.hits[i];
        }
        assert_eq!(s, r.total_hits());
        assert!(r.total_hits() > 0);
    }

    #[test]
    fn storm_excess_sum_matches_total() {
        let r = run(0x777D, 7);
        let mut s = 0u32;
        for i in 0..SEGMENTS {
            s = s.saturating_add(r.excess[i]);
        }
        assert_eq!(s, r.total_excess());
    }

    #[test]
    fn storm_over_counts_within_hits() {
        let r = run(0x777E, LEVELS);
        for i in 0..SEGMENTS {
            assert!(r.over[i] <= r.hits[i]);
        }
        assert!(r.total_over() > 0);
    }

    #[test]
    fn storm_verify_detects_tamper() {
        let mut r = run(0x777F, 3);
        assert!(r.verify());
        r.excess[0] = r.excess[0].saturating_add(1);
        assert!(!r.verify());
    }

    #[test]
    fn dominant_rejects_empty_and_ties() {
        let r = StormRun {
            level: 1,
            bursts: 1,
            hits: [0; SEGMENTS],
            excess: [0; SEGMENTS],
            over: [0; SEGMENTS],
            digest: 0,
        };
        assert_eq!(r.dominant(), Err(StormCode::ReportBad.val()));
        let tie = StormRun {
            level: 1,
            bursts: 1,
            hits: [1; SEGMENTS],
            excess: [5; SEGMENTS],
            over: [0; SEGMENTS],
            digest: 0,
        };
        assert_eq!(tie.dominant(), Err(StormCode::ReportBad.val()));
    }

    #[test]
    fn share_permille_bounds() {
        let r = StormRun {
            level: 1,
            bursts: 1,
            hits: [1; SEGMENTS],
            excess: [10, 10, 10, 10],
            over: [0; SEGMENTS],
            digest: 0,
        };
        assert_eq!(r.share_permille(0), 250);
        assert_eq!(r.share_permille(9), 0);
        let zero = StormRun {
            level: 1,
            bursts: 1,
            hits: [0; SEGMENTS],
            excess: [0; SEGMENTS],
            over: [0; SEGMENTS],
            digest: 0,
        };
        assert_eq!(zero.share_permille(0), 0);
    }

    #[test]
    fn report_row_roundtrip() {
        let r = run(0x7801, 2);
        let row = report_row(&r).expect("row");
        assert!(audit_render(&row));
        let (tag, v, d) = row_fields(&row).expect("fields");
        assert_eq!(tag, STORM_TAG);
        assert_eq!(v[0], 2);
        assert_eq!(v[1], 0);
        assert_eq!(v[2], r.total_excess());
        assert_eq!(v[3], r.total_hits());
        assert_eq!(d, row_digest(v[0], v[1], v[2], v[3]));
    }

    #[test]
    fn report_row_rejects_broken_run() {
        let mut r = run(0x7802, 2);
        r.digest = r.digest.wrapping_add(1);
        assert_eq!(report_row(&r), Err(StormCode::ReportBad.val()));
    }

    #[test]
    fn report_rows_requires_ten() {
        let runs = storm_report(0x7803).expect("report");
        assert_eq!(runs.len(), LEVELS as usize);
        let rows = report_rows(&runs).expect("rows");
        assert_eq!(rows.len(), LEVELS as usize);
        let short = report_rows(&runs[..3]);
        assert_eq!(short, Err(StormCode::ReportBad.val()));
    }

    #[test]
    fn audit_render_rejects_bad_rows() {
        let bad = [
            "",
            "STRM",
            "STRM|level=1|seg=0|excess=1|hits=2",
            "STRM|level=1|seg=0|excess=1|hits=2|digest=0",
            "STRM|level=1|seg=0|excess=1|hits=2|digest=0000000000000000|x=1",
            "STRM|level=1|seg=0|excess=1|hits=2|hash=0000000000000000",
            "STRM|level=0|seg=0|excess=1|hits=2|digest=0000000000000000",
            "STRM|level=11|seg=0|excess=1|hits=2|digest=0000000000000000",
            "STRM|level=1|seg=4|excess=1|hits=2|digest=0000000000000000",
            "STRM|level=1|seg=0|excess=1|hits=2|digest=00000000000000ZZ",
        ];
        for row in bad {
            assert!(!audit_render(row), "accepted {}", row);
        }
    }

    #[test]
    fn audit_render_rejects_value_tamper() {
        let row = report_row(&run(0x7804, 3)).expect("row");
        let (tag, v, d) = row_fields(&row).expect("f");
        let tampered = render_row(tag, v[0], v[1], v[2] + 1, v[3], d);
        assert!(!audit_render(&tampered));
        let fixed_digest = render_row(
            tag,
            v[0],
            v[1],
            v[2] + 1,
            v[3],
            row_digest(v[0], v[1], v[2] + 1, v[3]),
        );
        assert!(audit_render(&fixed_digest));
    }

    #[test]
    fn fields_with_keys_rejects_wrong_key_order() {
        let row = "STRM|seg=0|level=1|excess=1|hits=2|digest=0000000000000000";
        assert!(row_fields(row).is_none());
    }

    #[test]
    fn parse_u32_edges() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("+1"), None);
        assert_eq!(parse_u32(" 1"), None);
    }

    #[test]
    fn parse_hex16_edges() {
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFFF"), Some(u64::MAX));
        assert_eq!(parse_hex16("ffffffffffffffff"), Some(u64::MAX));
        assert_eq!(parse_hex16("000000000000000g"), None);
        assert_eq!(parse_hex16("0"), None);
    }

    #[test]
    fn storm_replay_stable_and_seed_sensitive() {
        assert!(storm_replay(0x7805, 3).unwrap());
        let a = run(0x7805, 6);
        let b = run(0x7806, 6);
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn storm_replay_rejects_zero_times() {
        assert_eq!(storm_replay(1, 0), Err(StormCode::ReplayBad.val()));
    }

    #[test]
    fn tail_sample_validation() {
        assert!(TailSample::new(0, 0, 0, 1).is_ok());
        assert!(TailSample::new(u32::MAX, 3, 9, NS_LIMIT).is_ok());
        assert_eq!(
            TailSample::new(0, 4, 0, 1),
            Err(StormCode::SegBad.val())
        );
        assert_eq!(
            TailSample::new(0, 0, 10, 1),
            Err(StormCode::SampleBad.val())
        );
        assert_eq!(
            TailSample::new(0, 0, 0, 0),
            Err(StormCode::SampleBad.val())
        );
        assert_eq!(
            TailSample::new(0, 0, 0, NS_LIMIT + 1),
            Err(StormCode::SampleBad.val())
        );
    }

    #[test]
    fn tail_sample_digest_field_sensitive() {
        let a = sample(1, 0, 0, 100);
        let b = sample(2, 0, 0, 100);
        let c = sample(1, 1, 0, 100);
        let d = sample(1, 0, 1, 100);
        let e = sample(1, 0, 0, 101);
        let ds = [a.digest(), b.digest(), c.digest(), d.digest(), e.digest()];
        for i in 0..ds.len() {
            for j in (i + 1)..ds.len() {
                assert_ne!(ds[i], ds[j]);
            }
        }
    }

    #[test]
    fn tail_cut_empty() {
        assert_eq!(tail_cut(&[]), Err(StormCode::EmptyStore.val()));
    }

    #[test]
    fn tail_cut_single() {
        assert_eq!(tail_cut(&[7]), Ok(7));
    }

    #[test]
    fn tail_cut_nearest_rank() {
        let mut v: Vec<u32> = Vec::new();
        let mut i = 1u32;
        while i <= 1_000 {
            v.push(i);
            i += 1;
        }
        assert_eq!(tail_cut(&v), Ok(999));
        assert_eq!(tail_cut(&v[..10]), Ok(10));
    }

    #[test]
    fn tail_cut_clamped_to_last() {
        let v = [1u32, 2, 3];
        assert_eq!(tail_cut(&v), Ok(3));
    }

    #[test]
    fn sort_copy_does_not_mutate() {
        let v = [5u32, 1, 3];
        let s = sort_copy(&v);
        assert_eq!(v, [5, 1, 3]);
        assert_eq!(s, vec![1, 3, 5]);
    }

    #[test]
    fn store_capacity_bounds() {
        assert!(TailStore::new(1).is_ok());
        assert!(TailStore::new(CAP_LIMIT).is_ok());
        assert_eq!(TailStore::new(0).err(), Some(StormCode::CapacityBad.val()));
        assert_eq!(
            TailStore::new(CAP_LIMIT + 1).err(),
            Some(StormCode::CapacityBad.val())
        );
    }

    #[test]
    fn store_push_counts_drop_and_capture() {
        let mut st = TailStore::new(8).expect("store");
        assert_eq!(st.push(sample(0, 0, 0, 999), 1_000), Ok(false));
        assert_eq!(st.push(sample(1, 0, 0, 1_000), 1_000), Ok(true));
        assert_eq!(st.push(sample(2, 0, 0, 1_500), 1_000), Ok(true));
        assert_eq!(st.dropped(), 1);
        assert_eq!(st.captured(), 2);
        assert_eq!(st.offered(), 3);
        assert_eq!(st.len(), 2);
        assert!(st.verify());
        assert!(st.ascending());
        assert!(st.all_at_least(1_000));
    }

    #[test]
    fn store_evicts_oldest_at_capacity() {
        let mut st = TailStore::new(2).expect("store");
        for i in 0..5u32 {
            let s = sample(i, 0, 0, 2_000 + i);
            assert_eq!(st.push(s, 1_000), Ok(true));
        }
        assert_eq!(st.len(), 2);
        assert_eq!(st.captured(), 5);
        assert_eq!(st.evicted(), 3);
        assert_eq!(st.offered(), 5);
        assert!(st.verify());
        assert_eq!(st.entry(0).map(|s| s.seq), Some(3));
        assert_eq!(st.entry(1).map(|s| s.seq), Some(4));
    }

    #[test]
    fn store_seq_guard_rejects_duplicate() {
        let mut st = TailStore::new(8).expect("store");
        assert_eq!(st.push(sample(5, 0, 0, 2_000), 1_000), Ok(true));
        assert_eq!(
            st.push(sample(5, 0, 0, 2_001), 1_000),
            Err(StormCode::SeqBad.val())
        );
        assert_eq!(
            st.push(sample(4, 0, 0, 2_001), 1_000),
            Err(StormCode::SeqBad.val())
        );
        assert_eq!(st.offered(), 1);
        assert!(st.verify());
    }

    #[test]
    fn store_seq_guard_survives_eviction() {
        let mut st = TailStore::new(1).expect("store");
        assert_eq!(st.push(sample(9, 0, 0, 2_000), 1_000), Ok(true));
        assert_eq!(
            st.push(sample(9, 0, 0, 2_000), 1_000),
            Err(StormCode::SeqBad.val())
        );
        assert_eq!(st.push(sample(10, 0, 0, 2_000), 1_000), Ok(true));
        assert_eq!(st.evicted(), 1);
        assert!(st.verify());
    }

    #[test]
    fn store_rejects_bad_cut_and_sample() {
        let mut st = TailStore::new(4).expect("store");
        assert_eq!(
            st.push(sample(0, 0, 0, 1_000), 0),
            Err(StormCode::SampleBad.val())
        );
        let bad = TailSample {
            seq: 0,
            seg: 9,
            day: 0,
            ns: 1_000,
        };
        assert_eq!(st.push(bad, 1_000), Err(StormCode::SegBad.val()));
        assert_eq!(st.offered(), 0);
    }

    #[test]
    fn store_digest_moves_with_content() {
        let mut a = TailStore::new(4).expect("a");
        let mut b = TailStore::new(4).expect("b");
        let _ = a.push(sample(0, 0, 0, 2_000), 1_000);
        let _ = b.push(sample(0, 0, 0, 2_001), 1_000);
        assert_ne!(a.digest(), b.digest());
        assert!(a.verify() && b.verify());
    }

    #[test]
    fn store_verify_detects_digest_drift() {
        let mut st = TailStore::new(4).expect("store");
        let _ = st.push(sample(0, 0, 0, 2_000), 1_000);
        let good = st.digest();
        st.digest = good.wrapping_add(1);
        assert!(!st.verify());
        st.digest = good;
        assert!(st.verify());
    }

    #[test]
    fn store_empty_replay_rejected() {
        let st = TailStore::new(4).expect("store");
        assert_eq!(st.replay(), Err(StormCode::EmptyStore.val()));
        assert!(!st.replay_consistent(10));
    }

    #[test]
    fn store_replay_snapshot() {
        let mut st = TailStore::new(8).expect("store");
        let _ = st.push(sample(1, 0, 0, 2_000), 1_000);
        let _ = st.push(sample(2, 1, 1, 2_100), 1_000);
        let rep = st.replay().expect("replay");
        assert_eq!(rep.rows, 2);
        assert_eq!(rep.first_seq, 1);
        assert_eq!(rep.last_seq, 2);
        assert_eq!(rep.digest, st.digest());
        assert!(st.replay_consistent(REPLAYS));
    }

    #[test]
    fn synth_stream_shape() {
        let s = synth_stream(0x9001, 64).expect("stream");
        assert_eq!(s.len(), SEGMENTS * 64);
        let mut i = 1usize;
        while i < s.len() {
            assert!(s[i].seq > s[i - 1].seq);
            i += 1;
        }
    }

    #[test]
    fn synth_stream_has_tail() {
        let s = synth_stream(0x9002, 200).expect("stream");
        let mut max = 0u32;
        for x in &s {
            if x.ns > max {
                max = x.ns;
            }
        }
        assert!(max > STORM_BASE_NS[3] * TAIL_FACTOR / 2);
    }

    #[test]
    fn synth_stream_rejects_bad_per_seg() {
        assert_eq!(synth_stream(1, 0), Err(StormCode::SampleBad.val()));
        assert_eq!(
            synth_stream(1, STREAM_PER_SEG + 1),
            Err(StormCode::SampleBad.val())
        );
    }

    #[test]
    fn capture_stream_cut_and_window() {
        let stream = synth_stream(0x9003, 256).expect("stream");
        let c = capture_stream(&stream, 32).expect("capture");
        assert!(c.cut_ns > 0);
        assert!(c.store.verify());
        assert!(c.store.all_at_least(c.cut_ns));
        assert!(c.store.captured() > 0);
        assert_eq!(
            c.store.offered(),
            c.store.captured() + c.store.dropped()
        );
    }

    #[test]
    fn capture_stream_empty_rejected() {
        assert!(capture_stream(&[], 8).is_err());
    }

    #[test]
    fn capture_synth_evicts() {
        let c = capture_synth(0x9004, STREAM_PER_SEG, STORE_CAP).expect("cap");
        assert!(c.store.evicted() > 0);
        assert_eq!(c.store.len(), STORE_CAP);
        assert!(c.store.verify());
    }

    #[test]
    fn capture_replay_stable() {
        assert!(capture_replay(0x9005, 512, 32, 3).unwrap());
        assert_eq!(
            capture_replay(0x9005, 512, 32, 0),
            Err(StormCode::ReplayBad.val())
        );
    }

    #[test]
    fn export_row_roundtrip() {
        let s = sample(42, 2, 3, 12_345);
        let row = export_row(&s);
        assert!(audit_tail_row(&row));
        let (_tag, v, d) = tail_fields(&row).expect("fields");
        assert_eq!(v[0], 42);
        assert_eq!(v[1], 2);
        assert_eq!(v[2], 12_345);
        assert_eq!(v[3], 3);
        assert_eq!(d, tail_row_digest(42, 2, 12_345, 3));
    }

    #[test]
    fn export_row_tamper_rejected() {
        let s = sample(42, 2, 3, 12_345);
        let good = export_row(&s);
        assert!(audit_tail_row(&good));
        let d = tail_row_digest(42, 2, 12_345, 3);
        let tampered = render_tail_row(42, 2, 12_346, 3, d);
        assert!(!audit_tail_row(&tampered));
        assert!(audit_tail_row(&render_tail_row(
            42,
            2,
            12_346,
            3,
            tail_row_digest(42, 2, 12_346, 3)
        )));
    }

    #[test]
    fn export_rows_order_and_count() {
        let mut st = TailStore::new(8).expect("store");
        let _ = st.push(sample(1, 0, 0, 5_000), 1_000);
        let _ = st.push(sample(2, 1, 1, 5_100), 1_000);
        let rows = export_rows(&st);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| audit_tail_row(r)));
        assert!(rows[0].contains("seq=1"));
        assert!(rows[1].contains("seq=2"));
    }

    #[test]
    fn export_digest_stable_and_nonzero() {
        let mut st = TailStore::new(8).expect("store");
        let _ = st.push(sample(1, 0, 0, 5_000), 1_000);
        assert_ne!(export_digest(&st), 0);
        assert!(export_stable(&st, 10));
    }

    #[test]
    fn export_stable_rejects_empty() {
        let st = TailStore::new(8).expect("store");
        assert!(!export_stable(&st, 10));
        assert!(!export_stable(&st, 0));
    }

    #[test]
    fn export_digest_is_fold_of_rows() {
        let mut st = TailStore::new(8).expect("store");
        let _ = st.push(sample(1, 0, 0, 5_000), 1_000);
        let _ = st.push(sample(2, 1, 1, 5_100), 1_000);
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < st.len() {
            if let Some(s) = st.entry(i) {
                h = fold_digest(h, tail_row_digest(s.seq, s.seg as u32, s.ns, s.day as u32));
            }
            i += 1;
        }
        assert_eq!(h, export_digest(&st));
    }

    #[test]
    fn checks_all_green() {
        let set = run_unx49065_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn checks_not_truncated_and_len_8() {
        let set = run_unx49065_checks();
        assert_eq!(set.len(), 8);
        assert!(!set.truncated());
        assert_eq!(set.passed_count(), 8);
        assert_eq!(set.failed_count(), 0);
    }

    #[test]
    fn checks_names_distinct() {
        let set = run_unx49065_checks();
        let mut names: Vec<&'static str> = Vec::new();
        for c in set.iter() {
            assert!(!names.contains(&c.name));
            names.push(c.name);
        }
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn checks_detail_nonempty() {
        for (name, passed, det) in extra_checks() {
            assert!(passed, "{} red: {}", name, det);
            assert!(!det.is_empty());
        }
    }

    #[test]
    fn storm_report_error_free_for_all_levels() {
        let runs = storm_report(0xABCD).expect("report");
        for r in &runs {
            assert!(r.verify());
            assert!(r.localized());
        }
    }

    #[test]
    fn capture_small_cap_no_eviction() {
        let c = capture_synth(0x9006, 256, CAP_LIMIT).expect("cap");
        assert_eq!(c.store.evicted(), 0);
        assert!(c.store.verify());
    }
}
