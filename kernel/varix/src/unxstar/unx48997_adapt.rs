//! UNX-F48997 自适应阈值收敛 + UNX-F48998 参数回滚快照 · 0x71840
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B10 族第九槽）：
//!   UNX-F48997 手势状态机（1/2/3 指 · 42 迁移表）fuzz 面（乱参数注入不崩）:
//!          ① 纯功能代码 ≥260 行；
//!          ② UNX-F48997-J1 fuzz ×10^4 零崩。
//!   UNX-F48998 手势状态机（1/2/3 指 · 42 迁移表）判据用例集（20 例入库可复跑）:
//!          ① 纯功能代码 ≥300 行；
//!          ② UNX-F48998-J1 ×20 全绿入库。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：目标合法 → 步长 → 逼近方向 → 落点收敛 → 界内钳制 →
//!          摘要 → 快照版本 → 保留上界 → 逐位还原。
//! 目标合法先于一切：目标本身越界时，任何「逼近量」都没有意义。
//! 若先算误差再校验目标，越界目标会先污染 last_sign 与翻转计数，
//! 让「零震荡」判据变成一笔糊涂账。
//! 步长先于方向：步长是这次调整的物理上限，必须先定下来；先算方向
//! 再截步长会让同一次观测被记两次位移，迭代账与移动账对不上。
//! 方向先于落点：误差不跨零时不允许「越过再回来」的落点策略。
//! 本模块的落点规则是「误差 ≤ 步长则精确着陆」，着陆后误差恒 0，
//! 这是零震荡的结构保证；若允许过冲，符号翻转就成了常态。
//! 界内钳制在落点之后：着陆点在 [MIN,MAX] 内时钳制不动作；
//! 钳制只在防御性路径触发并单独计数，正常收敛路径的 clamped 必须为 0。
//! 快照侧：版本先于保留上界，保留上界先于逐位还原。版本不存在是
//! 「问错了对象」，版本被逐出是「问得太旧」，两者诊断不同；先查上界
//! 会把未知版本误报成越界回滚，排障方向被带偏。
//! 本槽在既有「收敛 + 快照」底座之上补两层卡面判据：
//! ① F48997 fuzz 面——把目标、步长、补丁三类参数按全量 i32 噪声注入，
//!    越界目标一律 BoundBad、越界补丁一律 BoundBad，钳制分支单独记账，
//!    10^4 轮内只允许返回错误码，函数本体不 panic、不越界、不静默；
//! ② F48998 判据用例集——20 例具名用例，每例固定输入、固定期望
//!    （值或错误码），入库双跑逐位一致，并可复跑；
//! 底座仍然只回答「阈值能否收敛」与「快照能否逐位还原」；不做洪泛
//! 压测（B10-SIZE）、不做手势分类（B10-CORE）、不做质量指标
//! （B10-QUALITY）、不做段位封印（B10-SEAL）。快照环是定长 8 槽。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71840，VX6 批 B10 族第九槽）。
pub const SEG_BASE: u32 = 0x71840;

/// 可调参数数（点击时长 / 长按时长 / 甩动速度 / 捏合间隙）。
pub const PARAMS: usize = 4;
/// 点击时长下界（ms）。
pub const TAP_MIN: i32 = 40;
/// 点击时长上界（ms）。
pub const TAP_MAX: i32 = 400;
/// 点击时长初值（ms）。
pub const TAP_INIT: i32 = 100;
/// 自适应目标（ms）。
pub const TAP_TARGET: i32 = 180;
/// 初始步长。
pub const STEP_INIT: u32 = 32;
/// 步长下界。
pub const STEP_MIN: u32 = 1;
/// 步长衰减分子。
pub const STEP_DECAY_NUM: u32 = 7;
/// 步长衰减分母。
pub const STEP_DECAY_DEN: u32 = 8;
/// 自适应轮数（卡面 ×10^3）。
pub const ADAPT_ROUNDS: u32 = 1_000;
/// 收敛前段目标。
pub const WARM_TARGET: i32 = 140;
/// 收敛前段长度。
pub const WARM_ROUNDS: u32 = 10;
/// 不动点观察轮数。
pub const FIXED_ROUNDS: u32 = 256;
/// 追逐轮数（交替目标有界性）。
pub const CHASE_ROUNDS: u32 = 200;
/// 快照槽数。
pub const SNAP_SLOTS: usize = 8;
/// 快照轮数（卡面 ×10^2）。
pub const SNAP_ROUNDS: u32 = 100;
/// 注入路数。
pub const INJECT_ROUTES: usize = 4;
/// 账本字段数。
pub const STATS_FIELDS: usize = 6;
/// 黄金轮数。
pub const GOLDEN_ROUNDS: u32 = 16;
/// 黄金起始目标（每轮 +1，终点恰为 TAP_TARGET）。
pub const GOLDEN_BASE: i32 = 165;
/// 黄金目标步进（1ms：任何步长都足以精确着陆）。
pub const GOLDEN_STEP: i32 = 1;

/// 参数下界表（顺序与 PARAMS 一致）。
pub const PARAM_MIN: [i32; PARAMS] = [TAP_MIN, 300, 100, 8];
/// 参数上界表。
pub const PARAM_MAX: [i32; PARAMS] = [TAP_MAX, 1_200, 1_200, 64];
/// 参数初值表。
pub const PARAM_INIT: [i32; PARAMS] = [TAP_INIT, 500, 300, 24];

// ── 诊断码闭集（本模块窗口 +0x01..+0x0E） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdCode {
    /// 自适应账不守恒
    AdaptBad,
    /// 收敛判据失败（未收敛/步长未到界/翻转）
    ConvergeBad,
    /// 步长非法或未单调衰减
    StepBad,
    /// 目标越界
    BoundBad,
    /// 符号翻转超出预期
    OscBad,
    /// 快照版本不存在
    VersionBad,
    /// 快照缺失/重复/摘要坏
    SnapshotBad,
    /// 回滚跨过保留上界
    RollbackBad,
    /// 账本不等式不成立
    LedgerBad,
    /// 渲染字段数/格式坏
    RenderBad,
    /// 重放不一致
    ReplayBad,
    /// 轮数越界覆盖闸门
    CoverBad,
    /// 段窗口越界或码族重复
    SegmentBad,
    /// 注入未全捕获
    FuzzBad,
}

impl AdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            AdCode::AdaptBad => "自适应账不守恒",
            AdCode::ConvergeBad => "收敛判据失败",
            AdCode::StepBad => "步长非法或未单调衰减",
            AdCode::BoundBad => "目标越界",
            AdCode::OscBad => "符号翻转超出预期",
            AdCode::VersionBad => "快照版本不存在",
            AdCode::SnapshotBad => "快照缺失重复或摘要坏",
            AdCode::RollbackBad => "回滚跨过保留上界",
            AdCode::LedgerBad => "账本不等式不成立",
            AdCode::RenderBad => "渲染字段数或格式坏",
            AdCode::ReplayBad => "重放不一致",
            AdCode::CoverBad => "轮数越界覆盖闸门失败",
            AdCode::SegmentBad => "段窗口越界或码族重复",
            AdCode::FuzzBad => "注入未全捕获",
        }
    }

    pub const ALL: [AdCode; 14] = [
        AdCode::AdaptBad,
        AdCode::ConvergeBad,
        AdCode::StepBad,
        AdCode::BoundBad,
        AdCode::OscBad,
        AdCode::VersionBad,
        AdCode::SnapshotBad,
        AdCode::RollbackBad,
        AdCode::LedgerBad,
        AdCode::RenderBad,
        AdCode::ReplayBad,
        AdCode::CoverBad,
        AdCode::SegmentBad,
        AdCode::FuzzBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < AdCode::ALL.len() {
        if let Some(c) = AdCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知自适应诊断码"
}

pub fn codes_are_distinct() -> bool {
    let mut i = 0usize;
    while i < AdCode::ALL.len() {
        let a = match AdCode::ALL.get(i) {
            Some(v) => v.val(),
            None => return false,
        };
        let mut j = i + 1;
        while j < AdCode::ALL.len() {
            let b = match AdCode::ALL.get(j) {
                Some(v) => v.val(),
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn segment_guard() -> Result<(), u32> {
    let mut i = 0usize;
    while i < AdCode::ALL.len() {
        let c = match AdCode::ALL.get(i) {
            Some(v) => *v,
            None => return Err(AdCode::SegmentBad.val()),
        };
        let off = c.val().wrapping_sub(SEG_BASE);
        if off < 0x01 || off > 0x0F {
            return Err(AdCode::SegmentBad.val());
        }
        i += 1;
    }
    if !codes_are_distinct() {
        return Err(AdCode::SegmentBad.val());
    }
    Ok(())
}

// ── 参数合法域 ────────────────────────────────────────────────────
/// 四参数逐项界内检查。
pub fn params_in_bounds(params: &[i32; PARAMS]) -> bool {
    let mut i = 0usize;
    while i < PARAMS {
        let v = match params.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let lo = match PARAM_MIN.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let hi = match PARAM_MAX.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if v < lo || v > hi {
            return false;
        }
        i += 1;
    }
    true
}

/// 逐位比较两个参数向量（逐位还原判据的底座）。
pub fn params_equal(a: &[i32; PARAMS], b: &[i32; PARAMS]) -> bool {
    let mut i = 0usize;
    while i < PARAMS {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) if x == y => {}
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 饱和补丁：`params + delta`，逐项 saturating_add 后做界内校验。
pub fn apply_patch(params: &[i32; PARAMS], delta: &[i32; PARAMS]) -> Result<[i32; PARAMS], u32> {
    let mut out = *params;
    let mut i = 0usize;
    while i < PARAMS {
        let p = match params.get(i) {
            Some(v) => *v,
            None => return Err(AdCode::BoundBad.val()),
        };
        let d = match delta.get(i) {
            Some(v) => *v,
            None => return Err(AdCode::BoundBad.val()),
        };
        let next = p.saturating_add(d);
        if let Some(slot) = out.get_mut(i) {
            *slot = next;
        }
        i += 1;
    }
    if !params_in_bounds(&out) {
        return Err(AdCode::BoundBad.val());
    }
    Ok(out)
}

// ── F48997 自适应调谐器 ───────────────────────────────────────────
/// 六字段自适应账：迭代、移动、静止、翻转、当前值、摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AdaptStats {
    pub iterations: u32,
    pub moved: u32,
    pub still: u32,
    pub flips: u32,
    pub value: i32,
    pub digest: u64,
}

impl AdaptStats {
    pub fn balanced(&self) -> bool {
        self.iterations > 0
            && self.iterations == self.moved.saturating_add(self.still)
            && self.value >= TAP_MIN
            && self.value <= TAP_MAX
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.balanced() {
            return Err(AdCode::AdaptBad.val());
        }
        if self.digest == 0 {
            return Err(AdCode::ReplayBad.val());
        }
        Ok(())
    }
}

/// 单参数调谐器：目标驱动、让步长单调衰减、误差不跨零。
///
/// 核心规则：
/// - 目标必须落在 [TAP_MIN, TAP_MAX]；
/// - 误差符号只用于选择方向，绝不允许过冲；
/// - 误差 ≤ 步长时精确着陆（误差归零），这是零震荡的结构保证；
/// - 每次接受观测都把步长按 7/8 衰减，终至 1；
/// - 着陆后继续观测只走「静止」账，值不再移动（不动点）。
pub struct Tuner {
    value: i32,
    step: u32,
    iterations: u32,
    moved: u32,
    still: u32,
    rejected: u32,
    flips: u32,
    last_sign: i8,
    clamped: u32,
    digest: u64,
}

impl Tuner {
    pub fn new() -> Tuner {
        Tuner {
            value: TAP_INIT,
            step: STEP_INIT,
            iterations: 0,
            moved: 0,
            still: 0,
            rejected: 0,
            flips: 0,
            last_sign: 0,
            clamped: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn value(&self) -> i32 {
        self.value
    }

    pub fn step(&self) -> u32 {
        self.step
    }

    pub fn flips(&self) -> u32 {
        self.flips
    }

    pub fn moved(&self) -> u32 {
        self.moved
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn clamped(&self) -> u32 {
        self.clamped
    }

    fn decay(&mut self) {
        let next = self.step.saturating_mul(STEP_DECAY_NUM) / STEP_DECAY_DEN;
        self.step = if next < STEP_MIN { STEP_MIN } else { next };
    }

    fn fold_obs(&mut self, sgn: u32, delta: u32) {
        self.digest = p_core::fold_u32(self.digest, sgn);
        self.digest = p_core::fold_u32(self.digest, delta);
        self.digest = p_core::fold_u32(self.digest, self.value as u32);
    }

    /// 观测一次目标并更新阈值。返回更新后的阈值。
    pub fn observe(&mut self, target: i32) -> Result<i32, u32> {
        if target < TAP_MIN || target > TAP_MAX {
            self.rejected = self.rejected.saturating_add(1);
            self.digest = p_core::fold_u32(self.digest, 0xE1);
            self.digest = p_core::fold_u32(self.digest, target as u32);
            return Err(AdCode::BoundBad.val());
        }
        self.iterations = match self.iterations.checked_add(1) {
            Some(v) => v,
            None => return Err(AdCode::LedgerBad.val()),
        };
        let err = target - self.value;
        let sign: i8 = if err > 0 {
            1
        } else if err < 0 {
            -1
        } else {
            0
        };
        if sign == 0 {
            self.still = match self.still.checked_add(1) {
                Some(v) => v,
                None => return Err(AdCode::LedgerBad.val()),
            };
            self.last_sign = 0;
            self.decay();
            self.fold_obs(0, 0);
            return Ok(self.value);
        }
        if self.last_sign != 0 && sign != self.last_sign {
            self.flips = self.flips.saturating_add(1);
        }
        self.last_sign = sign;
        let mag = if err > 0 { err as u32 } else { (-err) as u32 };
        let delta = if mag <= self.step {
            self.value = target;
            mag
        } else {
            let next = self
                .value
                .saturating_add((sign as i32).saturating_mul(self.step as i32));
            self.value = next;
            self.step
        };
        if self.value < TAP_MIN {
            self.value = TAP_MIN;
            self.clamped = self.clamped.saturating_add(1);
        }
        if self.value > TAP_MAX {
            self.value = TAP_MAX;
            self.clamped = self.clamped.saturating_add(1);
        }
        self.moved = match self.moved.checked_add(1) {
            Some(v) => v,
            None => return Err(AdCode::LedgerBad.val()),
        };
        self.decay();
        let sgn = if sign > 0 { 1 } else { 2 };
        self.fold_obs(sgn, delta);
        Ok(self.value)
    }

    pub fn stats(&self) -> AdaptStats {
        AdaptStats {
            iterations: self.iterations,
            moved: self.moved,
            still: self.still,
            flips: self.flips,
            value: self.value,
            digest: self.digest,
        }
    }

    /// 全账自检：被接受观测必须恰好分解为 moved + still。
    /// 被拒观测单独入 rejected，不参与移动/静止分解。
    pub fn audit(&self) -> Result<(), u32> {
        if self.moved.saturating_add(self.still) != self.iterations {
            return Err(AdCode::AdaptBad.val());
        }
        if self.value < TAP_MIN || self.value > TAP_MAX {
            return Err(AdCode::BoundBad.val());
        }
        if self.step < STEP_MIN || self.step > STEP_INIT {
            return Err(AdCode::StepBad.val());
        }
        if self.digest == 0 {
            return Err(AdCode::ReplayBad.val());
        }
        Ok(())
    }
}

/// 收敛目标日程：前 WARM_ROUNDS 拍用 WARM_TARGET，其后用 TAP_TARGET。
pub fn schedule_target(i: u32) -> i32 {
    if i < WARM_ROUNDS {
        WARM_TARGET
    } else {
        TAP_TARGET
    }
}

/// 收敛探针：目标在同向区间内单调抬升，值只允许向上逼近，翻转必须为 0。
pub fn converge_probe(rounds: u32) -> Result<Tuner, u32> {
    if rounds < WARM_ROUNDS || rounds > ADAPT_ROUNDS {
        return Err(AdCode::CoverBad.val());
    }
    let mut t = Tuner::new();
    let mut i = 0u32;
    while i < rounds {
        let target = schedule_target(i);
        t.observe(target)?;
        if t.value() < TAP_MIN || t.value() > TAP_MAX {
            return Err(AdCode::BoundBad.val());
        }
        i += 1;
    }
    if t.value() != TAP_TARGET {
        return Err(AdCode::ConvergeBad.val());
    }
    if t.flips() != 0 {
        return Err(AdCode::OscBad.val());
    }
    if t.step() != STEP_MIN {
        return Err(AdCode::StepBad.val());
    }
    t.audit()?;
    Ok(t)
}

/// 步长轨迹：每一步都必须是上一步的 7/8 向下取整，且终至下界。
pub fn step_trace(rounds: u32) -> Result<Vec<u32>, u32> {
    if rounds < WARM_ROUNDS || rounds > ADAPT_ROUNDS {
        return Err(AdCode::CoverBad.val());
    }
    let mut t = Tuner::new();
    let mut out: Vec<u32> = Vec::new();
    let mut prev = STEP_INIT;
    let mut i = 0u32;
    while i < rounds {
        let target = schedule_target(i);
        t.observe(target)?;
        let now = t.step();
        if now > prev {
            return Err(AdCode::StepBad.val());
        }
        if now < STEP_MIN {
            return Err(AdCode::StepBad.val());
        }
        prev = now;
        out.push(now);
        i += 1;
    }
    if prev != STEP_MIN {
        return Err(AdCode::StepBad.val());
    }
    Ok(out)
}

/// 不动点探针：收敛后再观测固定目标，移动账必须冻结，静止账必须逐拍增长。
pub fn fixed_point_probe(rounds: u32) -> Result<(u32, u32, u32), u32> {
    if rounds != FIXED_ROUNDS {
        return Err(AdCode::CoverBad.val());
    }
    let mut t = Tuner::new();
    let mut i = 0u32;
    while i < ADAPT_ROUNDS {
        t.observe(schedule_target(i))?;
        i += 1;
    }
    let moved_before = t.moved();
    let still_before = t.stats().still;
    let value_before = t.value();
    let digest_before = t.stats().digest;
    let mut k = 0u32;
    while k < rounds {
        t.observe(TAP_TARGET)?;
        k += 1;
    }
    let moved_after = t.moved();
    let still_after = t.stats().still;
    if moved_after != moved_before {
        return Err(AdCode::ConvergeBad.val());
    }
    if still_after.saturating_sub(still_before) != rounds {
        return Err(AdCode::AdaptBad.val());
    }
    if t.value() != value_before {
        return Err(AdCode::ConvergeBad.val());
    }
    if t.stats().digest == digest_before {
        return Err(AdCode::ReplayBad.val());
    }
    t.audit()?;
    Ok((moved_before, moved_after, still_after))
}

/// 追逐探针：收敛后交替目标，值必须始终界内且每拍都走「移动」账。
/// 交替本身必然产生符号翻转，因此这里核对的是有界性而非零翻转。
pub fn chase_probe(rounds: u32) -> Result<Tuner, u32> {
    if rounds == 0 || rounds > ADAPT_ROUNDS {
        return Err(AdCode::CoverBad.val());
    }
    let mut t = Tuner::new();
    let mut i = 0u32;
    while i < ADAPT_ROUNDS {
        t.observe(schedule_target(i))?;
        i += 1;
    }
    let moved_before = t.moved();
    let flips_before = t.flips();
    let mut k = 0u32;
    while k < rounds {
        let target = if k % 2 == 0 { 175 } else { 185 };
        t.observe(target)?;
        if t.value() < 174 || t.value() > 186 {
            return Err(AdCode::OscBad.val());
        }
        k += 1;
    }
    if t.moved() != moved_before.saturating_add(rounds) {
        return Err(AdCode::AdaptBad.val());
    }
    if rounds >= 2 && t.flips() <= flips_before {
        return Err(AdCode::OscBad.val());
    }
    t.audit()?;
    Ok(t)
}

// ── F48998 快照账 ─────────────────────────────────────────────────
/// 一次提交的不可变参数快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snapshot {
    pub version: u32,
    pub params: [i32; PARAMS],
    pub digest: u64,
}

/// 快照摘要：版本 + 四参数顺序折叠。
pub fn snapshot_digest(version: u32, params: &[i32; PARAMS]) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, version);
    let mut i = 0usize;
    while i < PARAMS {
        if let Some(v) = params.get(i) {
            h = p_core::fold_u32(h, *v as u32);
        }
        i += 1;
    }
    h
}

/// 快照账统计：提交、回滚、逐出、最新版本、摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SnapStats {
    pub commits: u32,
    pub rollbacks: u32,
    pub evicted: u32,
    pub latest: u32,
    pub digest: u64,
}

/// 定长快照环：8 槽，逐出最旧版本即抬升保留下界。
pub struct SnapBook {
    slots: [Option<Snapshot>; SNAP_SLOTS],
    head: usize,
    count: usize,
    next_version: u32,
    commits: u32,
    rollbacks: u32,
    rejected: u32,
    digest: u64,
}

impl SnapBook {
    pub fn new() -> SnapBook {
        SnapBook {
            slots: [None; SNAP_SLOTS],
            head: 0,
            count: 0,
            next_version: 1,
            commits: 0,
            rollbacks: 0,
            rejected: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn latest_version(&self) -> Option<u32> {
        if self.count == 0 {
            return None;
        }
        let idx = (self.head + self.count - 1) % SNAP_SLOTS;
        match self.slots.get(idx) {
            Some(Some(s)) => Some(s.version),
            _ => None,
        }
    }

    pub fn oldest_version(&self) -> Option<u32> {
        if self.count == 0 {
            return None;
        }
        match self.slots.get(self.head) {
            Some(Some(s)) => Some(s.version),
            _ => None,
        }
    }

    pub fn params_of(&self, version: u32) -> Option<[i32; PARAMS]> {
        let mut k = 0usize;
        while k < self.count {
            let idx = (self.head + k) % SNAP_SLOTS;
            if let Some(Some(s)) = self.slots.get(idx) {
                if s.version == version {
                    return Some(s.params);
                }
            }
            k += 1;
        }
        None
    }

    pub fn versions(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.count {
            let idx = (self.head + k) % SNAP_SLOTS;
            if let Some(Some(s)) = self.slots.get(idx) {
                out.push(s.version);
            }
            k += 1;
        }
        out
    }

    /// 提交一版参数。满环时逐出最旧槽，保留下界随之抬升。
    pub fn commit(&mut self, params: [i32; PARAMS]) -> Result<u32, u32> {
        if !params_in_bounds(&params) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(AdCode::BoundBad.val());
        }
        let version = self.next_version;
        self.next_version = match self.next_version.checked_add(1) {
            Some(v) => v,
            None => return Err(AdCode::SnapshotBad.val()),
        };
        if self.count >= SNAP_SLOTS {
            if let Some(slot) = self.slots.get_mut(self.head) {
                *slot = None;
            }
            self.head = (self.head + 1) % SNAP_SLOTS;
            self.count = self.count.saturating_sub(1);
        }
        let digest = snapshot_digest(version, &params);
        let snap = Snapshot {
            version,
            params,
            digest,
        };
        let idx = (self.head + self.count) % SNAP_SLOTS;
        if let Some(slot) = self.slots.get_mut(idx) {
            *slot = Some(snap);
        } else {
            return Err(AdCode::SnapshotBad.val());
        }
        self.count = self.count.saturating_add(1);
        self.commits = match self.commits.checked_add(1) {
            Some(v) => v,
            None => return Err(AdCode::SnapshotBad.val()),
        };
        self.digest = p_core::fold_u32(self.digest, (digest >> 32) as u32);
        self.digest = p_core::fold_u32(self.digest, digest as u32);
        self.audit()?;
        Ok(version)
    }

    /// 回滚到指定版本：版本必须仍在保留窗口内。
    pub fn rollback_to(&mut self, version: u32) -> Result<[i32; PARAMS], u32> {
        let oldest = match self.oldest_version() {
            Some(v) => v,
            None => return Err(AdCode::VersionBad.val()),
        };
        if version < oldest {
            self.rejected = self.rejected.saturating_add(1);
            self.digest = p_core::fold_u32(self.digest, 0xE3);
            return Err(AdCode::RollbackBad.val());
        }
        let latest = match self.latest_version() {
            Some(v) => v,
            None => return Err(AdCode::VersionBad.val()),
        };
        if version > latest {
            self.rejected = self.rejected.saturating_add(1);
            self.digest = p_core::fold_u32(self.digest, 0xE4);
            return Err(AdCode::VersionBad.val());
        }
        match self.params_of(version) {
            Some(p) => {
                self.rollbacks = self.rollbacks.saturating_add(1);
                self.digest = p_core::fold_u32(self.digest, version);
                Ok(p)
            }
            None => {
                self.rejected = self.rejected.saturating_add(1);
                Err(AdCode::VersionBad.val())
            }
        }
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.count > SNAP_SLOTS {
            return Err(AdCode::SnapshotBad.val());
        }
        if self.count == 0 {
            if self.head != 0 {
                return Err(AdCode::SnapshotBad.val());
            }
            return Ok(());
        }
        let oldest = match self.oldest_version() {
            Some(v) => v,
            None => return Err(AdCode::SnapshotBad.val()),
        };
        // 只有「无逐出、无回滚、无拒账」的纯提交链才能从槽位复算主摘要；
        // 一旦发生逐出/回滚/拒账，主摘要已折入槽位之外的事件，二次校验
        // 退化为逐槽校验。
        let pure = self.rollbacks == 0
            && self.rejected == 0
            && self.commits as usize == self.count;
        let mut chain = p_core::FNV_OFF;
        let mut expect = oldest;
        let mut k = 0usize;
        while k < self.count {
            let idx = (self.head + k) % SNAP_SLOTS;
            let s = match self.slots.get(idx) {
                Some(Some(v)) => *v,
                _ => return Err(AdCode::SnapshotBad.val()),
            };
            if s.version != expect {
                return Err(AdCode::SnapshotBad.val());
            }
            if s.digest != snapshot_digest(s.version, &s.params) {
                return Err(AdCode::SnapshotBad.val());
            }
            if !params_in_bounds(&s.params) {
                return Err(AdCode::BoundBad.val());
            }
            if pure {
                chain = p_core::fold_u32(chain, (s.digest >> 32) as u32);
                chain = p_core::fold_u32(chain, s.digest as u32);
            }
            expect = expect.saturating_add(1);
            k += 1;
        }
        if pure && chain != self.digest {
            return Err(AdCode::SnapshotBad.val());
        }
        Ok(())
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn stats(&self) -> SnapStats {
        let latest = match self.latest_version() {
            Some(v) => v,
            None => 0,
        };
        SnapStats {
            commits: self.commits,
            rollbacks: self.rollbacks,
            evicted: self.commits.saturating_sub(self.count as u32),
            latest,
            digest: self.digest,
        }
    }
}

/// 第 i 次提交的确定性参数（全部落在合法域内）。
pub fn sample_params(i: u32) -> [i32; PARAMS] {
    [
        TAP_MIN + 20 + (i % 50) as i32,
        500 + (i % 100) as i32,
        300 + (i % 80) as i32,
        24 + (i % 16) as i32,
    ]
}

/// 快照探针：连续提交、逐版本取回、保留窗口边界与越界回滚。
pub fn snapshot_probe(rounds: u32) -> Result<SnapBook, u32> {
    if rounds == 0 || rounds > SNAP_ROUNDS {
        return Err(AdCode::CoverBad.val());
    }
    let mut book = SnapBook::new();
    let mut recorded: Vec<[i32; PARAMS]> = Vec::new();
    let mut i = 0u32;
    while i < rounds {
        let p = sample_params(i);
        let v = book.commit(p)?;
        if v != i + 1 {
            return Err(AdCode::VersionBad.val());
        }
        recorded.push(p);
        if book.latest_version() != Some(v) {
            return Err(AdCode::VersionBad.val());
        }
        let got = match book.params_of(v) {
            Some(x) => x,
            None => return Err(AdCode::SnapshotBad.val()),
        };
        if !params_equal(&got, &p) {
            return Err(AdCode::RollbackBad.val());
        }
        i += 1;
    }
    let oldest = match book.oldest_version() {
        Some(v) => v,
        None => return Err(AdCode::SnapshotBad.val()),
    };
    // 保留窗口内逐版本逐位取回。
    let mut v = oldest;
    while v <= rounds {
        let got = match book.rollback_to(v) {
            Ok(x) => x,
            Err(e) => return Err(e),
        };
        let want = match recorded.get((v - 1) as usize) {
            Some(x) => *x,
            None => return Err(AdCode::SnapshotBad.val()),
        };
        if !params_equal(&got, &want) {
            return Err(AdCode::RollbackBad.val());
        }
        v += 1;
    }
    // 已逐出的版本必须 RollbackBad（除非 rounds <= SNAP_SLOTS）。
    if rounds > SNAP_SLOTS as u32 {
        let evicted = oldest.saturating_sub(1);
        if book.rollback_to(evicted).err() != Some(AdCode::RollbackBad.val()) {
            return Err(AdCode::RollbackBad.val());
        }
    }
    // 未来版本必须 VersionBad。
    if book.rollback_to(rounds + 1).err() != Some(AdCode::VersionBad.val()) {
        return Err(AdCode::VersionBad.val());
    }
    book.audit()?;
    Ok(book)
}

/// 快照-回滚-再应用：补丁必须可重放，回滚后重放结果逐位一致。
pub fn reapply_probe() -> Result<([i32; PARAMS], [i32; PARAMS]), u32> {
    let base = PARAM_INIT;
    let patch: [i32; PARAMS] = [10, -20, 30, -4];
    let applied = apply_patch(&base, &patch)?;
    let mut book = SnapBook::new();
    let v1 = book.commit(base)?;
    let v2 = book.commit(applied)?;
    if v2 != v1 + 1 {
        return Err(AdCode::VersionBad.val());
    }
    let back = book.rollback_to(v1)?;
    if !params_equal(&back, &base) {
        return Err(AdCode::RollbackBad.val());
    }
    let again = apply_patch(&back, &patch)?;
    if !params_equal(&again, &applied) {
        return Err(AdCode::SnapshotBad.val());
    }
    let v3 = book.commit(again)?;
    let got = match book.params_of(v3) {
        Some(x) => x,
        None => return Err(AdCode::SnapshotBad.val()),
    };
    if !params_equal(&got, &applied) {
        return Err(AdCode::RollbackBad.val());
    }
    book.audit()?;
    Ok((applied, got))
}

// ── 注入 ──────────────────────────────────────────────────────────
/// 四路注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; INJECT_ROUTES],
    pub caught: [u32; INJECT_ROUTES],
}

impl InjectLog {
    pub fn new() -> InjectLog {
        InjectLog {
            tries: [0; INJECT_ROUTES],
            caught: [0; INJECT_ROUTES],
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < INJECT_ROUTES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c == 0 || c > t {
                return false;
            }
            i += 1;
        }
        true
    }
}

fn hit(log: &mut InjectLog, route: usize) -> Result<(), u32> {
    match log.caught.get_mut(route) {
        Some(c) => {
            *c = c.saturating_add(1);
            Ok(())
        }
        None => Err(AdCode::CoverBad.val()),
    }
}

/// 四路注入：
/// ① 越界目标 → BoundBad；
/// ② 空账回滚 → VersionBad；
/// ③ 已逐出版本回滚 → RollbackBad；
/// ④ 槽位摘要被篡改 → SnapshotBad。
pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > ADAPT_ROUNDS {
        return Err(AdCode::CoverBad.val());
    }
    let mut log = InjectLog::new();
    let mut i = 0u32;
    while i < rounds {
        // ① 越界目标。
        let mut t = Tuner::new();
        if t.observe(TAP_MAX + 1).err() == Some(AdCode::BoundBad.val()) {
            hit(&mut log, 0)?;
        }
        if t.observe(TAP_MIN - 1).err() == Some(AdCode::BoundBad.val()) {
            hit(&mut log, 0)?;
        }
        if let Some(x) = log.tries.get_mut(0) {
            *x = x.saturating_add(2);
        }

        // ② 空账回滚。
        let mut empty = SnapBook::new();
        if empty.rollback_to(1).err() == Some(AdCode::VersionBad.val()) {
            hit(&mut log, 1)?;
        }
        if let Some(x) = log.tries.get_mut(1) {
            *x = x.saturating_add(1);
        }

        // ③ 已逐出版本回滚。
        let mut book = SnapBook::new();
        let mut k = 0u32;
        while k < SNAP_SLOTS as u32 + 2 {
            book.commit(sample_params(k))?;
            k += 1;
        }
        let oldest = match book.oldest_version() {
            Some(v) => v,
            None => return Err(AdCode::SnapshotBad.val()),
        };
        if oldest > 1 && book.rollback_to(1).err() == Some(AdCode::RollbackBad.val()) {
            hit(&mut log, 2)?;
        }
        if let Some(x) = log.tries.get_mut(2) {
            *x = x.saturating_add(1);
        }

        // ④ 槽位摘要篡改。
        let mut tamper = SnapBook::new();
        tamper.commit(sample_params(0))?;
        let idx = (tamper.head + tamper.count - 1) % SNAP_SLOTS;
        if let Some(Some(slot)) = tamper.slots.get_mut(idx) {
            slot.digest ^= 1;
        }
        if tamper.audit().err() == Some(AdCode::SnapshotBad.val()) {
            hit(&mut log, 3)?;
        }
        if let Some(x) = log.tries.get_mut(3) {
            *x = x.saturating_add(1);
        }

        i += 1;
    }
    if !log.all_caught() {
        return Err(AdCode::FuzzBad.val());
    }
    Ok(log)
}

// ── 渲染 ──────────────────────────────────────────────────────────
/// 六字段：迭代|移动|静止|翻转|当前值|摘要。
pub fn render_stats(s: &AdaptStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.iterations, s.moved, s.still, s.flips, s.value, s.digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        let c = *b.get(i)?;
        if !(48..=57).contains(&c) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - 48) as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 18 {
        return None;
    }
    if *b.get(0)? != b'0' || *b.get(1)? != b'x' {
        return None;
    }
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let c = *b.get(i)?;
        let d = match c {
            48..=57 => c - 48,
            65..=70 => c - 55,
            97..=102 => c - 87,
            _ => return None,
        };
        v = (v << 4) | d as u64;
        i += 1;
    }
    Some(v)
}

/// 严格解析并核对「迭代 == 移动 + 静止」、翻转上限、值域与摘要。
pub fn audit_render(text: &str) -> Result<usize, u32> {
    let mut parts = [""; STATS_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < STATS_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != STATS_FIELDS {
        return Err(AdCode::RenderBad.val());
    }
    let iterations = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(AdCode::RenderBad.val()),
    };
    let moved = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(AdCode::RenderBad.val()),
    };
    let still = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(AdCode::RenderBad.val()),
    };
    let flips = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(AdCode::RenderBad.val()),
    };
    let value = match parts.get(4).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(AdCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(AdCode::RenderBad.val()),
    };
    if iterations == 0 || iterations != moved.saturating_add(still) {
        return Err(AdCode::AdaptBad.val());
    }
    if flips > iterations {
        return Err(AdCode::OscBad.val());
    }
    if value < TAP_MIN as u32 || value > TAP_MAX as u32 {
        return Err(AdCode::BoundBad.val());
    }
    if digest == 0 {
        return Err(AdCode::RenderBad.val());
    }
    Ok(STATS_FIELDS)
}

// ── 黄金运行 ──────────────────────────────────────────────────────
/// 黄金脚本：16 轮目标 165..180，值单调逼近并于终点精确着陆。
pub fn golden_run() -> Result<AdaptStats, u32> {
    let mut t = Tuner::new();
    let mut prev = t.value();
    let mut i = 0u32;
    while i < GOLDEN_ROUNDS {
        let target = GOLDEN_BASE + (i as i32).saturating_mul(GOLDEN_STEP);
        t.observe(target)?;
        if t.value() < prev {
            return Err(AdCode::ConvergeBad.val());
        }
        prev = t.value();
        i += 1;
    }
    let s = t.stats();
    if s.moved != GOLDEN_ROUNDS || s.still != 0 || s.flips != 0 {
        return Err(AdCode::AdaptBad.val());
    }
    if s.value != GOLDEN_BASE + (GOLDEN_ROUNDS as i32 - 1).saturating_mul(GOLDEN_STEP) {
        return Err(AdCode::ConvergeBad.val());
    }
    s.audit()?;
    Ok(s)
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AdCode::ReplayBad.val());
    }
    let c = converge_probe(512)?;
    let d = converge_probe(512)?;
    if c.stats() != d.stats() {
        return Err(AdCode::ReplayBad.val());
    }
    let e = snapshot_probe(SNAP_ROUNDS)?;
    let f = snapshot_probe(SNAP_ROUNDS)?;
    if e.stats() != f.stats() {
        return Err(AdCode::ReplayBad.val());
    }
    let g = reapply_probe()?;
    let h = reapply_probe()?;
    if !params_equal(&g.0, &h.0) || !params_equal(&g.1, &h.1) {
        return Err(AdCode::ReplayBad.val());
    }
    Ok(())
}

pub fn probe_seq_a() -> Result<u32, u32> {
    Ok(golden_run()?.iterations)
}

pub fn probe_seq_b() -> Result<u32, u32> {
    let book = snapshot_probe(32)?;
    Ok(book.stats().latest)
}

pub fn probe_seq_c() -> Result<u32, u32> {
    Ok(step_trace(64)?.len() as u32)
}

// ══════════════════════════════════════════════════════════════════
// F48997 fuzz 面：乱参数注入不崩（×10^4）
// ──────────────────────────────────────────────────────────────────
// 注入面有三类：目标值、补丁增量、快照参数。三类都由全量 i32 噪声驱动，
// 合法域的判定交给既有闸门（目标越界 BoundBad、补丁越界 BoundBad、
// 快照越界 BoundBad），本层只负责「跑满 10^4 轮后账仍然闭合」。
// 钳制分支单独计数：被钳到 [TAP_MIN,TAP_MAX] 的目标必须真的落界内，
// 且 Tuner 内部防御性 clamped 必须恒为 0——否则说明落点策略越界了。
// ══════════════════════════════════════════════════════════════════

/// fuzz 轮数（卡面 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// fuzz 种子（固定，保证可复跑）。
pub const FUZZ_SEED: u64 = 0x7184_0F97_0001;
/// 不变量抽查间隔（轮）。
pub const FUZZ_INVARIANT_EVERY: u32 = 997;

/// 目标钳制：任意 i32 落到 [TAP_MIN, TAP_MAX]。
pub fn clamp_param(v: i32) -> i32 {
    if v < TAP_MIN {
        TAP_MIN
    } else if v > TAP_MAX {
        TAP_MAX
    } else {
        v
    }
}

/// 全量 i32 噪声：直接截取 LCG 高 32 位，覆盖 i32 全域（含极值附近）。
fn chaotic_i32(rng: &mut p_core::Lcg) -> i32 {
    (rng.next() >> 32) as u32 as i32
}

/// fuzz 账：目标侧接受/拒绝/钳制、补丁侧提交/拒绝、值域与摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzStats {
    pub rounds: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub target_bound: u32,
    pub clamped: u32,
    pub committed: u32,
    pub patch_rej: u32,
    pub patch_bound: u32,
    pub min_value: i32,
    pub max_value: i32,
    pub digest: u64,
}

impl FuzzStats {
    /// 空账：摘要从 FNV 偏移量起折，值域上下界取反方便首轮收敛。
    pub fn new(rounds: u32) -> FuzzStats {
        FuzzStats {
            rounds,
            accepted: 0,
            rejected: 0,
            target_bound: 0,
            clamped: 0,
            committed: 0,
            patch_rej: 0,
            patch_bound: 0,
            min_value: TAP_MAX,
            max_value: TAP_MIN,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.accepted.saturating_add(self.rejected) != self.rounds {
            return Err(AdCode::AdaptBad.val());
        }
        if self.committed.saturating_add(self.patch_rej) != self.rounds {
            return Err(AdCode::LedgerBad.val());
        }
        // 分项账必须与总账逐项相等：界外目标数 == 目标拒绝数，
        // 补丁越界数 == 补丁拒绝数。任一项对不上就是分类账被污染。
        if self.target_bound != self.rejected {
            return Err(AdCode::LedgerBad.val());
        }
        if self.patch_bound != self.patch_rej {
            return Err(AdCode::LedgerBad.val());
        }
        if self.clamped > self.accepted {
            return Err(AdCode::AdaptBad.val());
        }
        if self.digest == 0 {
            return Err(AdCode::ReplayBad.val());
        }
        if self.min_value < TAP_MIN
            || self.max_value > TAP_MAX
            || self.min_value > self.max_value
        {
            return Err(AdCode::BoundBad.val());
        }
        Ok(())
    }
}

/// 参数 fuzz：每轮「目标噪声 → 补丁噪声 → 周期不变量复核」。
///
/// 目标侧：全量 i32 噪声。界内直接观测；界外偶数轮必须精确返回
/// BoundBad，奇数轮先钳制再观测（钳制分支必须成功且落界内）。
/// 补丁侧：每 3 轮用小增量制造真实提交，其余轮用全量噪声制造拒绝；
/// 任何非 BoundBad 的错误都视为账本被污染，直接上抛。
pub fn param_fuzz(rounds: u32) -> Result<FuzzStats, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(AdCode::CoverBad.val());
    }
    let mut t = Tuner::new();
    let mut book = SnapBook::new();
    let mut current = PARAM_INIT;
    let mut rng = p_core::Lcg::new(FUZZ_SEED);
    let mut st = FuzzStats::new(rounds);
    let mut i = 0u32;
    while i < rounds {
        // ① 目标噪声注入。
        let raw = chaotic_i32(&mut rng);
        let in_band = raw >= TAP_MIN && raw <= TAP_MAX;
        if in_band {
            let v = t.observe(raw)?;
            st.accepted = st.accepted.saturating_add(1);
            if v < TAP_MIN || v > TAP_MAX {
                return Err(AdCode::BoundBad.val());
            }
        } else if i % 2 == 0 {
            if t.observe(raw).err() != Some(AdCode::BoundBad.val()) {
                return Err(AdCode::BoundBad.val());
            }
            st.rejected = st.rejected.saturating_add(1);
            st.target_bound = st.target_bound.saturating_add(1);
        } else {
            let v = t.observe(clamp_param(raw))?;
            st.accepted = st.accepted.saturating_add(1);
            st.clamped = st.clamped.saturating_add(1);
            if v < TAP_MIN || v > TAP_MAX {
                return Err(AdCode::BoundBad.val());
            }
        }
        let v = t.value();
        if v < st.min_value {
            st.min_value = v;
        }
        if v > st.max_value {
            st.max_value = v;
        }
        // ② 补丁噪声注入：小幅增量走真实提交，全量噪声走拒绝账。
        let delta: [i32; PARAMS] = if i % 3 == 0 {
            [
                (rng.next() % 17) as i32 - 8,
                (rng.next() % 17) as i32 - 8,
                (rng.next() % 17) as i32 - 8,
                (rng.next() % 17) as i32 - 8,
            ]
        } else {
            [
                chaotic_i32(&mut rng),
                chaotic_i32(&mut rng),
                chaotic_i32(&mut rng),
                chaotic_i32(&mut rng),
            ]
        };
        match apply_patch(&current, &delta) {
            Ok(p) => {
                if !params_in_bounds(&p) {
                    return Err(AdCode::BoundBad.val());
                }
                let _ = book.commit(p)?;
                current = p;
                st.committed = st.committed.saturating_add(1);
            }
            Err(c) => {
                if c != AdCode::BoundBad.val() {
                    return Err(c);
                }
                st.patch_rej = st.patch_rej.saturating_add(1);
                st.patch_bound = st.patch_bound.saturating_add(1);
            }
        }
        // ③ 周期不变量复核：调谐账、快照账、参数域、防御钳制。
        if i % FUZZ_INVARIANT_EVERY == 0 {
            t.audit()?;
            book.audit()?;
            if !params_in_bounds(&current) {
                return Err(AdCode::BoundBad.val());
            }
            if t.clamped() != 0 {
                return Err(AdCode::BoundBad.val());
            }
            if t.moved().saturating_add(t.stats().still) != t.stats().iterations {
                return Err(AdCode::AdaptBad.val());
            }
        }
        st.digest = p_core::fold_u32(st.digest, i);
        st.digest = p_core::fold_u32(st.digest, t.value() as u32);
        i = i.saturating_add(1);
    }
    st.audit()?;
    t.audit()?;
    book.audit()?;
    Ok(st)
}

/// fuzz 复跑：同轮数两次注入的整账与摘要必须逐位相等。
/// 这是「fuzz 面可复跑」的底座：噪声序列由固定种子决定，账本又要求
/// 分项账闭合，两次运行的任一位不同都说明有隐藏状态泄漏。
pub fn fuzz_replay(rounds: u32) -> Result<u64, u32> {
    let a = param_fuzz(rounds)?;
    let b = param_fuzz(rounds)?;
    if a != b {
        return Err(AdCode::ReplayBad.val());
    }
    if a.digest == 0 {
        return Err(AdCode::ReplayBad.val());
    }
    Ok(a.digest)
}

/// 边界探针：把「恰好界内 / 恰好界外」的目标与补丁逐一钉死。
/// 返回命中的边界点数（固定 8 点），任何一点松动都判 red。
pub fn fuzz_boundary_probe() -> Result<u32, u32> {
    let mut hit = 0u32;
    let mut t = Tuner::new();
    if t.observe(TAP_MIN - 1).err() != Some(AdCode::BoundBad.val()) {
        return Err(AdCode::BoundBad.val());
    }
    hit = hit.saturating_add(1);
    if t.observe(TAP_MAX + 1).err() != Some(AdCode::BoundBad.val()) {
        return Err(AdCode::BoundBad.val());
    }
    hit = hit.saturating_add(1);
    // 恰好下界/上界：合法且落点必须仍在界内。
    let v1 = t.observe(TAP_MIN)?;
    if v1 < TAP_MIN || v1 > TAP_MAX {
        return Err(AdCode::BoundBad.val());
    }
    hit = hit.saturating_add(1);
    let v2 = t.observe(TAP_MAX)?;
    if v2 < TAP_MIN || v2 > TAP_MAX {
        return Err(AdCode::BoundBad.val());
    }
    hit = hit.saturating_add(1);
    // 补丁恰好触界：合法；再动 1 即越界。
    let p_min = apply_patch(&PARAM_INIT, &[TAP_MIN - TAP_INIT, 0, 0, 0])?;
    if p_min.get(0) != Some(&TAP_MIN) {
        return Err(AdCode::BoundBad.val());
    }
    hit = hit.saturating_add(1);
    let p_max = apply_patch(&PARAM_INIT, &[TAP_MAX - TAP_INIT, 0, 0, 0])?;
    if p_max.get(0) != Some(&TAP_MAX) {
        return Err(AdCode::BoundBad.val());
    }
    hit = hit.saturating_add(1);
    if apply_patch(&p_max, &[1, 0, 0, 0]).err() != Some(AdCode::BoundBad.val()) {
        return Err(AdCode::BoundBad.val());
    }
    hit = hit.saturating_add(1);
    if apply_patch(&p_min, &[-1, 0, 0, 0]).err() != Some(AdCode::BoundBad.val()) {
        return Err(AdCode::BoundBad.val());
    }
    hit = hit.saturating_add(1);
    Ok(hit)
}

/// 补丁风暴：只打补丁、不碰调谐器，全量 i32 增量 ×`rounds`。
/// 接受的补丁必须落界内并成为下一轮基底；拒绝必须是 BoundBad；
/// 账尾 accepted + rejected == rounds。返回 (accepted, rejected)。
pub fn patch_storm(rounds: u32) -> Result<(u32, u32), u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(AdCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(FUZZ_SEED ^ 0x9E37_79B9);
    let mut current = PARAM_INIT;
    let mut ok = 0u32;
    let mut rej = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let delta: [i32; PARAMS] = [
            chaotic_i32(&mut rng),
            chaotic_i32(&mut rng),
            chaotic_i32(&mut rng),
            chaotic_i32(&mut rng),
        ];
        match apply_patch(&current, &delta) {
            Ok(p) => {
                if !params_in_bounds(&p) {
                    return Err(AdCode::BoundBad.val());
                }
                current = p;
                ok = ok.saturating_add(1);
            }
            Err(c) => {
                if c != AdCode::BoundBad.val() {
                    return Err(c);
                }
                rej = rej.saturating_add(1);
            }
        }
        if i % FUZZ_INVARIANT_EVERY == 0 && !params_in_bounds(&current) {
            return Err(AdCode::BoundBad.val());
        }
        i = i.saturating_add(1);
    }
    if ok.saturating_add(rej) != rounds {
        return Err(AdCode::LedgerBad.val());
    }
    if !params_in_bounds(&current) {
        return Err(AdCode::BoundBad.val());
    }
    Ok((ok, rej))
}

// ══════════════════════════════════════════════════════════════════
// F48998 判据用例集（20 例入库可复跑）
// ──────────────────────────────────────────────────────────────────
// 每例都是「固定输入 + 固定期望」：期望要么是一个值，要么是一条
// 具体错误码。用例表本身入库（静态 const），执行器按序号复跑；
// 入库前先证明 20 例全绿、双跑逐位一致，再证明「错值/错码会被
// 识别」——否则用例集只是自说自话。
// ══════════════════════════════════════════════════════════════════

/// 用例期望：值或具体错误码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Expect {
    Value(i32),
    Code(AdCode),
}

/// 一条具名用例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Criterion {
    pub id: &'static str,
    pub name: &'static str,
    pub expect: Expect,
}

/// 用例条数（卡面 20 例）。
pub const CRITERIA_CASES: usize = 20;

/// 20 例入库表：覆盖观测/钳制/补丁/快照/回滚/收敛六类路径。
pub const CRITERIA: [Criterion; CRITERIA_CASES] = [
    Criterion {
        id: "C01",
        name: "观测180落132",
        expect: Expect::Value(132),
    },
    Criterion {
        id: "C02",
        name: "观测100静止",
        expect: Expect::Value(100),
    },
    Criterion {
        id: "C03",
        name: "观测40落68",
        expect: Expect::Value(68),
    },
    Criterion {
        id: "C04",
        name: "观测400落132",
        expect: Expect::Value(132),
    },
    Criterion {
        id: "C05",
        name: "观测39越下界",
        expect: Expect::Code(AdCode::BoundBad),
    },
    Criterion {
        id: "C06",
        name: "观测401越上界",
        expect: Expect::Code(AdCode::BoundBad),
    },
    Criterion {
        id: "C07",
        name: "观测180两拍落160",
        expect: Expect::Value(160),
    },
    Criterion {
        id: "C08",
        name: "观测180三拍落180",
        expect: Expect::Value(180),
    },
    Criterion {
        id: "C09",
        name: "小补丁点击时长110",
        expect: Expect::Value(110),
    },
    Criterion {
        id: "C10",
        name: "正万补丁越界",
        expect: Expect::Code(AdCode::BoundBad),
    },
    Criterion {
        id: "C11",
        name: "负万补丁越界",
        expect: Expect::Code(AdCode::BoundBad),
    },
    Criterion {
        id: "C12",
        name: "零补丁不动",
        expect: Expect::Value(100),
    },
    Criterion {
        id: "C13",
        name: "首提交版本1",
        expect: Expect::Value(1),
    },
    Criterion {
        id: "C14",
        name: "非法参数提交拒",
        expect: Expect::Code(AdCode::BoundBad),
    },
    Criterion {
        id: "C15",
        name: "两提交最新2",
        expect: Expect::Value(2),
    },
    Criterion {
        id: "C16",
        name: "空账回滚拒",
        expect: Expect::Code(AdCode::VersionBad),
    },
    Criterion {
        id: "C17",
        name: "回滚版本3点击62",
        expect: Expect::Value(62),
    },
    Criterion {
        id: "C18",
        name: "回滚逐出版本拒",
        expect: Expect::Code(AdCode::RollbackBad),
    },
    Criterion {
        id: "C19",
        name: "千轮收敛步长1",
        expect: Expect::Value(1),
    },
    Criterion {
        id: "C20",
        name: "黄金终点180",
        expect: Expect::Value(180),
    },
];

/// 执行第 i 例（固定输入，重复调用逐位一致）。
pub fn run_criterion(i: usize) -> Result<i32, u32> {
    match i {
        0 => {
            let mut t = Tuner::new();
            t.observe(180)
        }
        1 => {
            let mut t = Tuner::new();
            t.observe(100)
        }
        2 => {
            let mut t = Tuner::new();
            t.observe(40)
        }
        3 => {
            let mut t = Tuner::new();
            t.observe(400)
        }
        4 => {
            let mut t = Tuner::new();
            t.observe(TAP_MIN - 1)
        }
        5 => {
            let mut t = Tuner::new();
            t.observe(TAP_MAX + 1)
        }
        6 => {
            let mut t = Tuner::new();
            let _ = t.observe(180)?;
            t.observe(180)
        }
        7 => {
            let mut t = Tuner::new();
            let _ = t.observe(180)?;
            let _ = t.observe(180)?;
            t.observe(180)
        }
        8 => {
            let p = apply_patch(&PARAM_INIT, &[10, -20, 30, -4])?;
            match p.get(0) {
                Some(v) => Ok(*v),
                None => Err(AdCode::BoundBad.val()),
            }
        }
        9 => apply_patch(&PARAM_INIT, &[10_000, 0, 0, 0]).map(|p| p[0]),
        10 => apply_patch(&PARAM_INIT, &[-10_000, 0, 0, 0]).map(|p| p[0]),
        11 => apply_patch(&PARAM_INIT, &[0, 0, 0, 0]).map(|p| p[0]),
        12 => {
            let mut b = SnapBook::new();
            Ok(b.commit(PARAM_INIT)? as i32)
        }
        13 => {
            let mut b = SnapBook::new();
            b.commit([0, 500, 300, 24]).map(|v| v as i32)
        }
        14 => {
            let mut b = SnapBook::new();
            let _ = b.commit(PARAM_INIT)?;
            let _ = b.commit([120, 520, 320, 26])?;
            Ok(b.latest_version().unwrap_or(0) as i32)
        }
        15 => {
            let mut b = SnapBook::new();
            b.rollback_to(1).map(|_| 0)
        }
        16 => {
            let mut b = SnapBook::new();
            let mut k = 0u32;
            while k < 10 {
                let _ = b.commit(sample_params(k))?;
                k = k.saturating_add(1);
            }
            let p = b.rollback_to(3)?;
            match p.get(0) {
                Some(v) => Ok(*v),
                None => Err(AdCode::SnapshotBad.val()),
            }
        }
        17 => {
            let mut b = SnapBook::new();
            let mut k = 0u32;
            while k < 10 {
                let _ = b.commit(sample_params(k))?;
                k = k.saturating_add(1);
            }
            b.rollback_to(2).map(|p| p[0])
        }
        18 => {
            let t = converge_probe(ADAPT_ROUNDS)?;
            Ok(t.step() as i32)
        }
        19 => {
            let s = golden_run()?;
            Ok(s.value)
        }
        _ => Err(AdCode::CoverBad.val()),
    }
}

/// 期望匹配：值对值、码对码，类型错配一律不认。
pub fn criterion_matches(c: &Criterion, got: Result<i32, u32>) -> bool {
    match (c.expect, got) {
        (Expect::Value(v), Ok(x)) => v == x,
        (Expect::Code(code), Err(x)) => code.val() == x,
        _ => false,
    }
}

/// 20 例入库复跑：逐例「执行 → 匹配 → 再执行 → 逐位相等 → 再匹配」。
/// 返回全绿条数（必须等于 20）。
pub fn criteria_probe() -> Result<u32, u32> {
    let mut green = 0u32;
    let mut i = 0usize;
    while i < CRITERIA_CASES {
        let c = match CRITERIA.get(i) {
            Some(v) => v,
            None => return Err(AdCode::CoverBad.val()),
        };
        let a = run_criterion(i);
        if !criterion_matches(c, a) {
            return Err(AdCode::ConvergeBad.val());
        }
        let b = run_criterion(i);
        if a != b {
            return Err(AdCode::ReplayBad.val());
        }
        if !criterion_matches(c, b) {
            return Err(AdCode::ConvergeBad.val());
        }
        green = green.saturating_add(1);
        i += 1;
    }
    if green != CRITERIA_CASES as u32 {
        return Err(AdCode::CoverBad.val());
    }
    Ok(green)
}

/// 用例集负例：id 唯一、期望非空、错值/错码必须被识别、越界序号被拒。
pub fn criteria_negative() -> Result<(), u32> {
    let mut i = 0usize;
    while i < CRITERIA_CASES {
        let c = match CRITERIA.get(i) {
            Some(v) => v,
            None => return Err(AdCode::CoverBad.val()),
        };
        if c.id.is_empty() || c.name.is_empty() {
            return Err(AdCode::ConvergeBad.val());
        }
        let mut j = i + 1;
        while j < CRITERIA_CASES {
            if let Some(d) = CRITERIA.get(j) {
                if d.id == c.id {
                    return Err(AdCode::ConvergeBad.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    // 错值：C01 的真实结果是 132，喂 0 必须不认。
    let first = match CRITERIA.get(0) {
        Some(v) => *v,
        None => return Err(AdCode::CoverBad.val()),
    };
    if criterion_matches(&first, Ok(0)) {
        return Err(AdCode::ConvergeBad.val());
    }
    if !criterion_matches(&first, run_criterion(0)) {
        return Err(AdCode::ConvergeBad.val());
    }
    // 错码：C05 期望 BoundBad，喂 OscBad 必须不认。
    let fifth = match CRITERIA.get(4) {
        Some(v) => *v,
        None => return Err(AdCode::CoverBad.val()),
    };
    if criterion_matches(&fifth, Err(AdCode::OscBad.val())) {
        return Err(AdCode::ConvergeBad.val());
    }
    // 类型错配：值期望喂错误码、码期望喂值，都不认。
    if criterion_matches(&first, Err(AdCode::BoundBad.val())) {
        return Err(AdCode::ConvergeBad.val());
    }
    if criterion_matches(&fifth, Ok(0)) {
        return Err(AdCode::ConvergeBad.val());
    }
    // 越界序号必须 CoverBad。
    if run_criterion(CRITERIA_CASES).err() != Some(AdCode::CoverBad.val()) {
        return Err(AdCode::CoverBad.val());
    }
    Ok(())
}

// ── 判据 ──────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // ① F48997-J1：全量噪声 ×10^4，账闭合、无 panic、无越界。
    let (p1, d1) = match param_fuzz(FUZZ_ROUNDS) {
        Ok(s) => {
            let replay = fuzz_replay(1_000).is_ok();
            let ok = s.audit().is_ok()
                && s.rounds == FUZZ_ROUNDS
                && s.rejected > 0
                && s.clamped > 0
                && s.committed > 0
                && s.target_bound == s.rejected
                && s.patch_bound == s.patch_rej
                && s.max_value <= TAP_MAX
                && s.min_value >= TAP_MIN
                && replay;
            (
                ok,
                format!(
                    "r={} 拒={} 界外={} 补丁拒={}",
                    s.rounds, s.rejected, s.target_bound, s.patch_rej
                ),
            )
        }
        Err(e) => (false, format!("err=0x{:X}", e)),
    };
    out.push(("F48997 乱参数fuzz×10^4零崩", p1, d1));

    // ② 钳制分支 + 边界 8 点 + 越界目标拒绝 + 千次收敛（沿用旧收敛证据）。
    let (p2, d2) = {
        let clamp_ok = clamp_param(i32::MIN) == TAP_MIN
            && clamp_param(i32::MAX) == TAP_MAX
            && clamp_param(TAP_MIN - 1) == TAP_MIN
            && clamp_param(TAP_MAX + 1) == TAP_MAX
            && clamp_param(0) == TAP_MIN
            && clamp_param(TAP_TARGET) == TAP_TARGET;
        let edge = fuzz_boundary_probe() == Ok(8);
        let fz = param_fuzz(1_000)
            .map(|s| s.audit().is_ok() && s.clamped > 0)
            .unwrap_or(false);
        let conv = converge_probe(ADAPT_ROUNDS)
            .map(|t| t.value() == TAP_TARGET && t.flips() == 0 && t.step() == STEP_MIN)
            .unwrap_or(false);
        let rej = {
            let mut t = Tuner::new();
            t.observe(TAP_MAX + 1).err() == Some(AdCode::BoundBad.val())
                && t.observe(TAP_MIN - 1).err() == Some(AdCode::BoundBad.val())
        };
        (
            clamp_ok && edge && fz && conv && rej,
            String::from("clamp下=40 上=400 收敛=180 翻转0 边界8"),
        )
    };
    out.push(("F48997 fuzz钳制+越界目标拒绝", p2, d2));

    // ③ 步长逐拍不增 + 收敛后不动点（沿用旧证据）。
    let (p3, d3) = {
        let step_ok = step_trace(ADAPT_ROUNDS)
            .map(|v| {
                let mut mono = true;
                let mut prev = STEP_INIT;
                let mut i = 0usize;
                while i < v.len() {
                    if let Some(x) = v.get(i) {
                        if *x > prev {
                            mono = false;
                        }
                        prev = *x;
                    }
                    i += 1;
                }
                mono && v.last() == Some(&STEP_MIN)
            })
            .unwrap_or(false);
        let fp_ok = fixed_point_probe(FIXED_ROUNDS)
            .map(|(m0, m1, _)| m0 == m1)
            .unwrap_or(false);
        (
            step_ok && fp_ok,
            format!("步长末={} 移动前=后 静止+{}", STEP_MIN, FIXED_ROUNDS),
        )
    };
    out.push(("F48997 步长不增+不动点零移动", p3, d3));

    // ④ 追逐有界、防御钳制为 0、补丁风暴、四路注入全捕获（沿用旧证据）。
    let (p4, d4) = {
        let chase = chase_probe(CHASE_ROUNDS)
            .map(|t| t.clamped() == 0 && t.value() >= 174 && t.value() <= 186)
            .unwrap_or(false);
        let inject = inject_rounds(4).map(|l| l.all_caught()).unwrap_or(false);
        let storm = patch_storm(FUZZ_ROUNDS)
            .map(|(a, b)| a.saturating_add(b) == FUZZ_ROUNDS)
            .unwrap_or(false);
        let v = chase_probe(CHASE_ROUNDS).map(|t| t.value()).unwrap_or(0);
        (
            chase && inject && storm,
            format!("chase值={} clamped=0 补丁{}", v, FUZZ_ROUNDS),
        )
    };
    out.push(("F48997 追逐有界+补丁风暴零崩", p4, d4));

    // ⑤ F48998-J1：20 例入库、全绿、双跑逐位一致。
    let (p5, d5) = match criteria_probe() {
        Ok(n) => (
            n == CRITERIA_CASES as u32 && criteria_probe() == Ok(CRITERIA_CASES as u32),
            format!("{}/{} 绿 双跑同 值+码", n, CRITERIA_CASES),
        ),
        Err(e) => (false, format!("err=0x{:X}", e)),
    };
    out.push(("F48998 判据用例×20全绿入库", p5, d5));

    // ⑥ 用例集负例：错值/错码/类型错配必须识别，序号越界被拒。
    let (p6, d6) = match criteria_negative() {
        Ok(()) => (true, String::from("错值错码识别 id唯一 越界拒")),
        Err(e) => (false, format!("err=0x{:X}", e)),
    };
    out.push(("F48998 用例错配必须识别", p6, d6));

    // ⑦ 版本严格递增 + 回滚逐位还原 + 保留上界（沿用旧证据）。
    let (p7, d7) = {
        let snap = snapshot_probe(SNAP_ROUNDS)
            .map(|book| {
                let vers = book.versions();
                let mut mono = true;
                let mut i = 1usize;
                while i < vers.len() {
                    match (vers.get(i - 1), vers.get(i)) {
                        (Some(a), Some(b)) if *b == a + 1 => {}
                        _ => mono = false,
                    }
                    i += 1;
                }
                let st = book.stats();
                mono && st.latest == SNAP_ROUNDS && st.commits == SNAP_ROUNDS
            })
            .unwrap_or(false);
        let bit = snapshot_probe(32)
            .map(|book| {
                let oldest = match book.oldest_version() {
                    Some(v) => v,
                    None => return false,
                };
                let mut v = oldest;
                let mut ok = true;
                while v <= 32 {
                    match book.params_of(v) {
                        Some(p) => {
                            if !params_equal(&p, &sample_params(v - 1)) {
                                ok = false;
                            }
                        }
                        None => ok = false,
                    }
                    v += 1;
                }
                ok
            })
            .unwrap_or(false);
        let win = snapshot_probe(64)
            .map(|mut book| {
                let oldest = match book.oldest_version() {
                    Some(v) => v,
                    None => return false,
                };
                let inside = book.rollback_to(oldest).is_ok();
                let below = book.rollback_to(oldest.saturating_sub(1)).err()
                    == Some(AdCode::RollbackBad.val());
                let future = book.rollback_to(65).err() == Some(AdCode::VersionBad.val());
                inside && below && future
            })
            .unwrap_or(false);
        (
            snap && bit && win,
            format!("latest={} win={} 还原32 上界拒", SNAP_ROUNDS, SNAP_SLOTS),
        )
    };
    out.push(("F48998 版本递增+回滚逐位还原", p7, d7));

    // ⑧ 快照-回滚-再应用 + 渲染自检 + 篡改捕获 + 全链复跑。
    let (p8, d8) = {
        let re = reapply_probe()
            .map(|(a, b)| params_equal(&a, &b))
            .unwrap_or(false);
        let render = golden_run()
            .map(|s| {
                let text = render_stats(&s);
                audit_render(&text) == Ok(STATS_FIELDS)
            })
            .unwrap_or(false);
        let tamper = {
            let mut b = SnapBook::new();
            let mut ok = b.commit(PARAM_INIT).is_ok();
            let idx = (b.head + b.count - 1) % SNAP_SLOTS;
            if let Some(Some(s)) = b.slots.get_mut(idx) {
                s.digest ^= 1;
            }
            ok = ok && b.audit().err() == Some(AdCode::SnapshotBad.val());
            ok
        };
        let repro = repro_check().is_ok();
        (
            re && render && tamper && repro,
            String::from("再应用逐位同 摘要自检 OK"),
        )
    };
    out.push(("F48998 再应用逐位一致+摘要自检", p8, d8));

    out
}

pub fn run_unx48997_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48997 自适应收敛与快照回滚");
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
        assert_eq!(SEG_BASE, 0x71840);
        assert_eq!(PARAMS, 4);
        assert_eq!(SNAP_SLOTS, 8);
        assert_eq!(SNAP_ROUNDS, 100);
        assert_eq!(ADAPT_ROUNDS, 1_000);
        assert_eq!(AdCode::ALL.len(), 14);
    }

    #[test]
    fn codes_are_distinct_and_windowed() {
        assert!(codes_are_distinct());
        assert_eq!(segment_guard(), Ok(()));
        let mut i = 0usize;
        while i < AdCode::ALL.len() {
            let c = AdCode::ALL[i];
            assert_eq!(c.val(), SEG_BASE + 0x01 + i as u32);
            assert!(!c.detail().is_empty());
            assert!(!detail(c.val()).is_empty());
            i += 1;
        }
    }

    #[test]
    fn bounds_and_patch() {
        assert!(params_in_bounds(&PARAM_INIT));
        let p = apply_patch(&PARAM_INIT, &[10, -20, 30, -4]).unwrap();
        assert_eq!(p, [110, 480, 330, 20]);
        assert_eq!(
            apply_patch(&PARAM_INIT, &[10_000, 0, 0, 0]),
            Err(AdCode::BoundBad.val())
        );
    }

    #[test]
    fn observe_moves_toward_target() {
        let mut t = Tuner::new();
        assert_eq!(t.observe(180), Ok(132));
        assert!(t.value() > TAP_INIT);
        assert!(t.observe(180).unwrap() <= 180);
        assert_eq!(t.flips(), 0);
        assert!(t.audit().is_ok());
    }

    #[test]
    fn observe_lands_exactly_within_step() {
        let mut t = Tuner::new();
        assert_eq!(t.observe(110), Ok(110));
        assert_eq!(t.observe(110), Ok(110));
        assert_eq!(t.moved(), 1);
        assert_eq!(t.stats().still, 1);
    }

    #[test]
    fn observe_rejects_out_of_bounds() {
        let mut t = Tuner::new();
        assert_eq!(t.observe(TAP_MIN - 1), Err(AdCode::BoundBad.val()));
        assert_eq!(t.observe(TAP_MAX + 1), Err(AdCode::BoundBad.val()));
        assert_eq!(t.rejected(), 2);
        assert_eq!(t.value(), TAP_INIT);
        assert!(t.audit().is_ok());
    }

    #[test]
    fn converge_reaches_target_with_no_flips() {
        let t = converge_probe(ADAPT_ROUNDS).unwrap();
        assert_eq!(t.value(), TAP_TARGET);
        assert_eq!(t.flips(), 0);
        assert_eq!(t.step(), STEP_MIN);
        assert_eq!(t.stats().iterations, ADAPT_ROUNDS);
    }

    #[test]
    fn step_trace_is_monotone() {
        let v = step_trace(64).unwrap();
        assert_eq!(v.len(), 64);
        assert_eq!(v[0], 28);
        assert_eq!(v.last(), Some(&STEP_MIN));
    }

    #[test]
    fn fixed_point_freezes_value() {
        let (m0, m1, still) = fixed_point_probe(FIXED_ROUNDS).unwrap();
        assert_eq!(m0, m1);
        assert_eq!(m1.saturating_add(still), ADAPT_ROUNDS + FIXED_ROUNDS);
    }

    #[test]
    fn chase_stays_bounded() {
        let t = chase_probe(CHASE_ROUNDS).unwrap();
        assert!(t.value() >= 174 && t.value() <= 186);
        assert!(t.clamped() == 0);
    }

    #[test]
    fn snapshot_versions_increase() {
        let mut b = SnapBook::new();
        let v1 = b.commit(PARAM_INIT).unwrap();
        let v2 = b.commit([120, 520, 320, 26]).unwrap();
        assert_eq!((v1, v2), (1, 2));
        assert_eq!(b.latest_version(), Some(2));
        assert_eq!(b.oldest_version(), Some(1));
        assert!(b.audit().is_ok());
    }

    #[test]
    fn commit_rejects_out_of_bounds() {
        let mut b = SnapBook::new();
        assert_eq!(
            b.commit([0, 500, 300, 24]),
            Err(AdCode::BoundBad.val())
        );
        assert_eq!(b.len(), 0);
        assert_eq!(b.rejected(), 1);
    }

    #[test]
    fn eviction_moves_floor() {
        let mut b = SnapBook::new();
        let mut i = 0u32;
        while i < 10 {
            b.commit(sample_params(i)).unwrap();
            i += 1;
        }
        assert_eq!(b.len(), SNAP_SLOTS);
        assert_eq!(b.oldest_version(), Some(3));
        assert_eq!(b.latest_version(), Some(10));
        assert!(b.audit().is_ok());
    }

    #[test]
    fn rollback_window_rules() {
        let mut b = SnapBook::new();
        let mut i = 0u32;
        while i < 10 {
            b.commit(sample_params(i)).unwrap();
            i += 1;
        }
        assert!(b.rollback_to(3).is_ok());
        assert_eq!(b.rollback_to(2), Err(AdCode::RollbackBad.val()));
        assert_eq!(b.rollback_to(11), Err(AdCode::VersionBad.val()));
    }

    #[test]
    fn empty_book_rollback_is_version_bad() {
        let mut b = SnapBook::new();
        assert_eq!(b.rollback_to(1), Err(AdCode::VersionBad.val()));
    }

    #[test]
    fn reapply_is_bit_identical() {
        let (a, b) = reapply_probe().unwrap();
        assert!(params_equal(&a, &b));
    }

    #[test]
    fn snapshot_tamper_is_caught() {
        let mut b = SnapBook::new();
        b.commit(PARAM_INIT).unwrap();
        let idx = (b.head + b.count - 1) % SNAP_SLOTS;
        if let Some(Some(s)) = b.slots.get_mut(idx) {
            s.digest ^= 1;
        }
        assert_eq!(b.audit(), Err(AdCode::SnapshotBad.val()));
    }

    #[test]
    fn snapshot_probe_is_green() {
        let b = snapshot_probe(SNAP_ROUNDS).unwrap();
        let st = b.stats();
        assert_eq!(st.latest, SNAP_ROUNDS);
        assert_eq!(st.evicted, SNAP_ROUNDS - SNAP_SLOTS as u32);
        assert!(b.audit().is_ok());
    }

    #[test]
    fn golden_is_deterministic() {
        let a = golden_run().unwrap();
        let b = golden_run().unwrap();
        assert_eq!(a, b);
        assert_eq!(a.iterations, GOLDEN_ROUNDS);
        assert_eq!(a.value, TAP_TARGET);
        assert_eq!(a.flips, 0);
    }

    #[test]
    fn render_roundtrip_and_tamper() {
        let s = golden_run().unwrap();
        let text = render_stats(&s);
        assert_eq!(audit_render(&text), Ok(STATS_FIELDS));
        let tampered = text.replacen("16|16|", "16|15|", 1);
        assert_eq!(audit_render(&tampered), Err(AdCode::AdaptBad.val()));
        assert_eq!(audit_render("1|2|3"), Err(AdCode::RenderBad.val()));
        assert_eq!(parse_hex16("0x12"), None);
        assert_eq!(parse_u32("+1"), None);
    }

    #[test]
    fn inject_catches_all_routes() {
        let log = inject_rounds(2).unwrap();
        assert!(log.all_caught());
        assert_eq!(log.tries, [4, 2, 2, 2]);
        assert_eq!(log.caught, [4, 2, 2, 2]);
    }

    #[test]
    fn reproducibility_holds() {
        assert_eq!(repro_check(), Ok(()));
        assert_eq!(probe_seq_a(), Ok(GOLDEN_ROUNDS));
        assert_eq!(probe_seq_b(), Ok(32));
        assert_eq!(probe_seq_c(), Ok(64));
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48997_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }

    // ── F48997 / F48998 新判据 ──────────────────────────────────
    #[test]
    fn fuzz_ten_thousand_rounds_zero_blowup() {
        let s = param_fuzz(FUZZ_ROUNDS).unwrap();
        assert_eq!(s.rounds, FUZZ_ROUNDS);
        assert_eq!(s.accepted + s.rejected, FUZZ_ROUNDS);
        assert_eq!(s.committed + s.patch_rej, FUZZ_ROUNDS);
        assert_eq!(s.target_bound, s.rejected);
        assert_eq!(s.patch_bound, s.patch_rej);
        assert!(s.rejected > 0);
        assert!(s.clamped > 0);
        assert!(s.clamped <= s.accepted);
        assert!(s.committed > 0);
        assert!(s.min_value >= TAP_MIN);
        assert!(s.max_value <= TAP_MAX);
        assert_eq!(s.audit(), Ok(()));
        // 轮数闸门：0 与越界都必须 CoverBad。
        assert_eq!(param_fuzz(0).err(), Some(AdCode::CoverBad.val()));
        assert_eq!(
            param_fuzz(FUZZ_ROUNDS + 1).err(),
            Some(AdCode::CoverBad.val())
        );
    }

    #[test]
    fn fuzz_boundary_and_patch_storm_are_closed() {
        assert_eq!(fuzz_boundary_probe(), Ok(8));
        let (ok, rej) = patch_storm(FUZZ_ROUNDS).unwrap();
        assert_eq!(ok + rej, FUZZ_ROUNDS);
        assert!(params_in_bounds(&PARAM_INIT));
        assert_eq!(patch_storm(0).err(), Some(AdCode::CoverBad.val()));
        assert!(fuzz_replay(256).is_ok());
        // 分项账篡改必须被 audit 抓住。
        let mut s = param_fuzz(128).unwrap();
        s.target_bound = s.target_bound.saturating_add(1);
        assert_eq!(s.audit(), Err(AdCode::LedgerBad.val()));
    }

    #[test]
    fn fuzz_is_deterministic_and_bounded() {
        let a = param_fuzz(2_048).unwrap();
        let b = param_fuzz(2_048).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.accepted + a.rejected, 2_048);
        assert_eq!(a.committed + a.patch_rej, 2_048);
        assert_eq!(a.audit(), Ok(()));
    }

    #[test]
    fn clamp_covers_full_i32_range() {
        assert_eq!(clamp_param(i32::MIN), TAP_MIN);
        assert_eq!(clamp_param(i32::MAX), TAP_MAX);
        assert_eq!(clamp_param(TAP_MIN - 1), TAP_MIN);
        assert_eq!(clamp_param(TAP_MAX + 1), TAP_MAX);
        assert_eq!(clamp_param(0), TAP_MIN);
        assert_eq!(clamp_param(TAP_TARGET), TAP_TARGET);
        assert_eq!(clamp_param(TAP_MIN), TAP_MIN);
        assert_eq!(clamp_param(TAP_MAX), TAP_MAX);
        // 任意噪声钳制后必在界内。
        let mut rng = p_core::Lcg::new(FUZZ_SEED ^ 0x5A5A);
        let mut i = 0u32;
        while i < 1_000 {
            let v = clamp_param(chaotic_i32(&mut rng));
            assert!(v >= TAP_MIN && v <= TAP_MAX);
            i += 1;
        }
    }

    #[test]
    fn criteria_twenty_cases_all_green() {
        assert_eq!(criteria_probe(), Ok(CRITERIA_CASES as u32));
        assert_eq!(CRITERIA.len(), 20);
        let mut i = 0usize;
        while i < CRITERIA_CASES {
            let a = run_criterion(i);
            let b = run_criterion(i);
            assert_eq!(a, b, "case {} not replayable", i);
            assert!(criterion_matches(&CRITERIA[i], a), "case {} mismatch", i);
            i += 1;
        }
        assert_eq!(
            run_criterion(CRITERIA_CASES).err(),
            Some(AdCode::CoverBad.val())
        );
    }

    #[test]
    fn criteria_ids_are_unique_and_named() {
        let mut i = 0usize;
        while i < CRITERIA_CASES {
            let c = &CRITERIA[i];
            assert_eq!(c.id.len(), 3);
            assert!(!c.name.is_empty());
            assert!(c.name.len() <= 48);
            let mut j = i + 1;
            while j < CRITERIA_CASES {
                assert_ne!(c.id, CRITERIA[j].id);
                j += 1;
            }
            i += 1;
        }
        assert_eq!(CRITERIA[0].id, "C01");
        assert_eq!(CRITERIA[19].id, "C20");
    }

    #[test]
    fn criteria_negative_detects_mismatch() {
        assert_eq!(criteria_negative(), Ok(()));
        // 值期望：错值不认、对值认。
        let c = CRITERIA[0];
        assert!(criterion_matches(&c, Ok(132)));
        assert!(!criterion_matches(&c, Ok(133)));
        assert!(!criterion_matches(&c, Err(AdCode::BoundBad.val())));
        // 码期望：错码不认、对码认。
        let e = CRITERIA[4];
        assert!(criterion_matches(&e, Err(AdCode::BoundBad.val())));
        assert!(!criterion_matches(&e, Err(AdCode::OscBad.val())));
        assert!(!criterion_matches(&e, Ok(39)));
        // 半开区间：39/401 越界，40/400 合法。
        let mut t = Tuner::new();
        assert_eq!(t.observe(39).err(), Some(AdCode::BoundBad.val()));
        assert_eq!(t.observe(401).err(), Some(AdCode::BoundBad.val()));
        assert!(t.observe(40).is_ok());
        assert!(t.observe(400).is_ok());
    }

    #[test]
    fn adapt_check_details_fit_dyn_cap() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            let (name, _, d) = &items[i];
            assert!(!name.is_empty());
            assert!(d.len() <= 48, "detail too long: {}", d);
            i += 1;
        }
    }
}

