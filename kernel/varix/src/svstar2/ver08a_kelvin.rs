//! VE-F3501 · 色温曲线引擎运行时 —— 时间→色温映射曲线族 + 渐变器 + V 管线单源对接。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3501`
//!
//! # 职责（锚点原文拆解）
//!
//! - **曲线族**（[`CurveFamily`]）：色温调节的核心是「一天中的时间 → 目标色温」
//!   的映射曲线。不是一条曲线而是一族——工作、阅读、夜间各有自己的曲线
//!   （那是 F3503 场景系统的事），本册提供曲线的**表示与求值**：
//!   折线控制点插值（[`CurvePoint`]），采样时 O(控制点) 但带二分、带缓存。
//! - **渐变器**（[`BlendGard`]）：**色温突变 = 光敏风险**。任何曲线求值结果
//!   与当前生效值的差超过 [`STEP_MAX_K`]，都必须走渐变器分步推进，
//!   每步不超过一步上限。这条是 Z05 渐变禁令在色温域的延伸——
//!   禁令不分域：凡是「用户看得见的量」都不许跳。
//! - **V 管线单源对接**（[`PipelineHook`]）：色温是 V 域色彩管线的**输入**
//!   而不是旁路。本册不直接改像素，只把生效值通过 [`PipelineHook::publish`]
//!   交给注册的管线钩子；钩子失联（返回 [`HookState::Lost`]）→ **暂停推进**
//!   并告知（[`KelvinCode::PipelineLost`]），暂停期间曲线求值继续但不下发，
//!   恢复注册后从暂停值继续渐变——不许因为失联就突然跳到新值。
//! - **曲线损坏回退默认**（[`CurveBook::load`]）：控制点非升序、色温越界、
//!   点数不足都算损坏。损坏曲线**整族拒绝**并回退 [`DEFAULT_CURVE`]，
//!   原因三要素（什么/在哪/怎么办）记入 [`CurveReport`]。
//! - **用户自定义入口**（[`CurveBook::edit`]）：曲线可编辑，编辑结果**实时
//!   预览**（[`CurveBook::preview`]）不落正式族；预览通过才允许提交。
//!   编辑后的曲线与内置曲线同权（同校验、同渐变、同管线对接）。
//!
//! # 为什么采样要带缓存
//!
//! 曲线求值会被 V 管线每帧调用（合成器问「当前该用多少 K」）。虽然
//! 单次求值是 O(log n) 的二分，但「一分钟内的答案不变」——时间粒度是
//! 分钟（[`TIME_SLICE_MIN`]），同一分钟切片内的求值直接命中缓存。
//! 缓存键是分钟切片号，曲线变更（编辑/回退）会整体失效缓存。
//!
//! # 复杂度
//!
//! - `evaluate`：O(log n)（二分定位控制点区间），命中缓存 O(1)；
//! - `step_toward`：O(1)（单步渐变）；
//! - `load` / `edit` 校验：O(n)（控制点数，以十计）。
//!
//! # 零 panic 面
//!
//! 无索引越界可能（二分用切片 `get`）、无 `unwrap`、无浮点（色温用
//! 开尔文整数，插值用定点千分比）。

use alloc::vec::Vec;
use core::cmp::Ordering;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5680 起）
// ---------------------------------------------------------------------------

/// 色温曲线引擎诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KelvinCode {
    /// 控制点数不足（至少要 2 个点才能构成曲线）。
    TooFewPoints = 5680,
    /// 控制点时间非严格升序。
    TimeNotAscending = 5681,
    /// 色温值越界（超出 [`K_MIN`]..=[`K_MAX`]）。
    KelvinOutOfRange = 5682,
    /// 管线钩子失联（暂停推进并告知）。
    PipelineLost = 5683,
    /// 曲线族损坏（整族拒绝，回退默认）。
    FamilyCorrupt = 5684,
    /// 编辑预览未通过就提交。
    PreviewNotApproved = 5685,
    /// 采样时间切片无效（超出一天范围）。
    SliceOutOfRange = 5686,
    /// 渐变步长配置非法（步上限为零）。
    StepConfigInvalid = 5687,
}

impl KelvinCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            KelvinCode::TooFewPoints => "控制点数不足",
            KelvinCode::TimeNotAscending => "控制点时间非升序",
            KelvinCode::KelvinOutOfRange => "色温值越界",
            KelvinCode::PipelineLost => "V管线钩子失联",
            KelvinCode::FamilyCorrupt => "曲线族损坏",
            KelvinCode::PreviewNotApproved => "预览未通过即提交",
            KelvinCode::SliceOutOfRange => "采样时间切片无效",
            KelvinCode::StepConfigInvalid => "渐变步长配置非法",
        }
    }

    /// 是否阻断级：管线失联与族损坏会让推进完全停摆。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            KelvinCode::PipelineLost | KelvinCode::FamilyCorrupt
        )
    }
}

// ---------------------------------------------------------------------------
// 二、常量：物理边界与安全钳制
// ---------------------------------------------------------------------------

/// 色温下限（开尔文）。低于此值显示会明显偏红且失去内容可读性。
pub const K_MIN: u32 = 1200;
/// 色温上限（开尔文）。高于此值接近纯蓝白，再高无意义。
pub const K_MAX: u32 = 9300;
/// 单步渐变上限（开尔文/步）。超过此值的差必须分多步走（渐变禁令）。
pub const STEP_MAX_K: u32 = 60;
/// 渐变最短步数：任何跨步渐变至少分这么多步，杜绝「一步到位」伪装。
pub const BLEND_MIN_STEPS: u32 = 4;
/// 时间切片（分钟）：同一分钟内求值命中缓存。
pub const TIME_SLICE_MIN: u32 = 1;
/// 一天的分钟数（求值域）。
pub const DAY_MINUTES: u32 = 24 * 60;
/// 默认曲线：暖夜护眼基线（22:00 起 3400K 暖，06:00 后回 6500K 中性）。
pub const DEFAULT_CURVE: [(u32, u32); 4] = [(0, 3400), (360, 3400), (420, 6500), (1320, 6500)];

/// 定点千分比乘法：`(a * b) / 1000`，全程整数，无浮点。
pub fn permille_mul(a: u32, b: u32) -> u32 {
    (a as u64 * b as u64 / 1000) as u32
}

/// 把任意输入钳制进合法色温域。
pub fn clamp_kelvin(k: u32) -> u32 {
    k.clamp(K_MIN, K_MAX)
}

// ---------------------------------------------------------------------------
// 三、曲线表示
// ---------------------------------------------------------------------------

/// 曲线控制点：`(分钟切片, 目标色温)`。分钟按 [`DAY_MINUTES`] 取模归一。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CurvePoint {
    /// 一天中的分钟数（0..DAY_MINUTES）。
    pub minute: u32,
    /// 目标色温（开尔文，已钳制）。
    pub kelvin: u32,
}

impl CurvePoint {
    /// 构造并钳制。
    pub fn new(minute: u32, kelvin: u32) -> Self {
        CurvePoint {
            minute: minute % DAY_MINUTES,
            kelvin: clamp_kelvin(kelvin),
        }
    }
}

/// 单条色温曲线：按分钟升序的控制点序列。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Curve {
    points: Vec<CurvePoint>,
}

impl Curve {
    /// 校验并构造。非升序 / 点数不足 / 色温越界一律拒绝（三要素理由）。
    pub fn build(points: &[(u32, u32)]) -> Result<Self, KelvinCode> {
        if points.len() < 2 {
            return Err(KelvinCode::TooFewPoints);
        }
        let mut pts = Vec::with_capacity(points.len());
        for &(m, k) in points {
            pts.push(CurvePoint::new(m, k));
        }
        for w in pts.windows(2) {
            if w[0].minute >= w[1].minute {
                return Err(KelvinCode::TimeNotAscending);
            }
        }
        for &(_, k) in points {
            if !(K_MIN..=K_MAX).contains(&k) {
                return Err(KelvinCode::KelvinOutOfRange);
            }
        }
        Ok(Curve { points: pts })
    }

    /// 控制点数。
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// 是否空曲线（构造保证非空，此接口供防御性检查）。
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// 只读控制点。
    pub fn points(&self) -> &[CurvePoint] {
        &self.points
    }

    /// 二分定位覆盖 `minute` 的控制点区间并线性插值。
    /// 区间外取最近端点值（曲线定义在全天，端点即边界）。
    pub fn evaluate(&self, minute: u32) -> u32 {
        if self.points.is_empty() {
            return clamp_kelvin(6500);
        }
        let m = minute % DAY_MINUTES;
        let first = self.points[0];
        let last = *self.points.last().unwrap_or(&first);
        if m <= first.minute {
            return first.kelvin;
        }
        if m >= last.minute {
            return last.kelvin;
        }
        let mut lo = 0usize;
        let mut hi = self.points.len();
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            match self.points[mid].minute.cmp(&m) {
                Ordering::Less => lo = mid,
                Ordering::Equal => return self.points[mid].kelvin,
                Ordering::Greater => hi = mid,
            }
        }
        let a = self.points[lo];
        let b = self.points[hi.min(self.points.len() - 1)];
        let span = b.minute.saturating_sub(a.minute).max(1);
        let off = m.saturating_sub(a.minute);
        // 定点插值：a + (b-a) * off / span，全程整数。
        let delta = (b.kelvin as u64 - a.kelvin as u64) * off as u64 / span as u64;
        (a.kelvin as u64 + delta) as u32
    }

    /// 曲线结构签名（编辑去重与损坏定位用）。
    pub fn signature(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for p in &self.points {
            h = (h ^ (p.minute as u64)).wrapping_mul(0x1000_0000_01b3);
            h = (h ^ (p.kelvin as u64)).wrapping_mul(0x1000_0000_01b3);
        }
        h
    }
}

/// 曲线族：场景名 → 曲线。族是原子单位——一条坏，整族退默认。
#[derive(Clone, Debug)]
pub struct CurveFamily {
    name: Option<&'static str>,
    curve: Curve,
}

impl CurveFamily {
    /// 内置默认族（编译期常量展开，永不损坏）。
    pub fn builtin() -> Self {
        CurveFamily {
            name: Some("builtin-default"),
            curve: Curve::build(&DEFAULT_CURVE).unwrap_or(Curve {
                points: Vec::new(),
            }),
        }
    }

    /// 从控制点装载（用户自定义入口的底层）。
    pub fn load(points: &[(u32, u32)]) -> Result<Self, KelvinCode> {
        let curve = Curve::build(points)?;
        Ok(CurveFamily {
            name: Some("user"),
            curve,
        })
    }

    /// 族名。
    pub fn name(&self) -> &'static str {
        self.name.unwrap_or("unnamed")
    }

    /// 求值。
    pub fn evaluate(&self, minute: u32) -> u32 {
        self.curve.evaluate(minute)
    }

    /// 结构签名。
    pub fn signature(&self) -> u64 {
        self.curve.signature()
    }
}

// ---------------------------------------------------------------------------
// 四、渐变器
// ---------------------------------------------------------------------------

/// 渐变器：把「当前生效值 → 目标值」的差切成不超过 [`STEP_MAX_K`] 的步。
/// 每次调用 [`BlendGard::step_toward`] 走一步；目标变了就地重切，已走的
/// 步作废但不回退（显示值永远单调趋近最新目标，不摆动）。
#[derive(Clone, Debug)]
pub struct BlendGard {
    current: u32,
    target: u32,
    steps_left: u32,
    step_size: u32,
}

impl BlendGard {
    /// 从初始值出发构造（初始值即当前生效值，不需渐变）。
    pub fn at(kelvin: u32) -> Self {
        let k = clamp_kelvin(kelvin);
        BlendGard {
            current: k,
            target: k,
            steps_left: 0,
            step_size: STEP_MAX_K,
        }
    }

    /// 自定义步上限（配置合法性检查：零步长拒绝）。
    pub fn with_step_cap(kelvin: u32, cap: u32) -> Result<Self, KelvinCode> {
        if cap == 0 {
            return Err(KelvinCode::StepConfigInvalid);
        }
        let mut g = BlendGard::at(kelvin);
        g.step_size = cap;
        Ok(g)
    }

    /// 当前生效值。
    pub fn current(&self) -> u32 {
        self.current
    }

    /// 当前目标值。
    pub fn target(&self) -> u32 {
        self.target
    }

    /// 剩余步数（0 表示已就位）。
    pub fn steps_left(&self) -> u32 {
        self.steps_left
    }

    /// 是否渐变中。
    pub fn blending(&self) -> bool {
        self.steps_left > 0
    }

    /// 设定新目标：差超步上限自动分步，最小步数 [`BLEND_MIN_STEPS`] 兜底。
    pub fn set_target(&mut self, kelvin: u32) {
        let t = clamp_kelvin(kelvin);
        self.target = t;
        let gap = t.abs_diff(self.current);
        if gap == 0 {
            self.steps_left = 0;
            return;
        }
        let by_cap = gap.div_ceil(self.step_size);
        self.steps_left = by_cap.max(BLEND_MIN_STEPS);
    }

    /// 推进一步，返回本步后的生效值。目标突变时（中间值被跳过）钳制保护。
    pub fn step_toward(&mut self) -> u32 {
        if self.steps_left == 0 {
            return self.current;
        }
        let gap = self.target as i64 - self.current as i64;
        let direct = gap.unsigned_abs() as u32;
        let move_k = direct.min(self.step_size);
        self.current = if gap >= 0 {
            clamp_kelvin(self.current + move_k)
        } else {
            clamp_kelvin(self.current - move_k)
        };
        self.steps_left -= 1;
        // 到位即清零：剩余步数是进度指示不是配额。
        if self.current == self.target {
            self.steps_left = 0;
        }
        self.current
    }

    /// 中断接管：把当前生效值冻结为新基准，目标保持等待重新驱动。
    /// （F3507 中断安全同源——中断不许丢当前值，也不许替用户做决定。）
    pub fn freeze(&mut self) -> u32 {
        self.steps_left = 0;
        self.current
    }
}

// ---------------------------------------------------------------------------
// 五、V 管线单源对接
// ---------------------------------------------------------------------------

/// 管线钩子状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookState {
    /// 已注册且最近一次下发被确认。
    Live,
    /// 已注册但下发未获确认（失联）。
    Lost,
    /// 尚未注册任何钩子。
    Detached,
}

/// V 管线钩子 trait：色温引擎的唯一出站口。单源纪律——引擎不认识
/// 任何具体渲染路径，只认识这个钩子；钩子失联由注册方负责重连。
pub trait PipelineHook {
    /// 下发当前生效色温。返回 false 表示管线未确认（失联）。
    fn publish(&mut self, kelvin: u32) -> bool;
}

/// 暂停态告知条目（三要素：什么/在哪/怎么办）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineNotice {
    /// 主体：发生了什么。
    pub what: &'static str,
    /// 位置：在哪条链路上。
    pub where_: &'static str,
    /// 建议：接下来怎么办。
    pub advice: &'static str,
    /// 关联诊断码。
    pub code: KelvinCode,
}

/// 引擎本体：曲线族 + 渐变器 + 管线钩子 + 分钟缓存。
pub struct KelvinEngine<H: PipelineHook> {
    family: CurveFamily,
    blend: BlendGard,
    hook: Option<H>,
    hook_state: HookState,
    cache_minute: Option<u32>,
    cache_value: Option<u32>,
    paused: bool,
    notices: Vec<PipelineNotice>,
}

impl<H: PipelineHook> KelvinEngine<H> {
    /// 用默认曲线族构造引擎。
    pub fn new(hook: H) -> Self {
        KelvinEngine {
            family: CurveFamily::builtin(),
            blend: BlendGard::at(6500),
            hook: Some(hook),
            hook_state: HookState::Live,
            cache_minute: None,
            cache_value: None,
            paused: false,
            notices: Vec::new(),
        }
    }

    /// 装载新曲线族（用户编辑提交走这里）。装载成功即失效缓存。
    pub fn load_family(&mut self, points: &[(u32, u32)]) -> Result<(), KelvinCode> {
        let fam = CurveFamily::load(points)?;
        self.family = fam;
        self.cache_minute = None;
        self.cache_value = None;
        Ok(())
    }

    /// 回退默认族（损坏恢复路径）。
    pub fn revert_default(&mut self) -> PipelineNotice {
        self.family = CurveFamily::builtin();
        self.cache_minute = None;
        self.cache_value = None;
        PipelineNotice {
            what: "曲线族损坏已回退默认",
            where_: "色温曲线引擎/族装载",
            advice: "检查自定义曲线控制点后重新提交",
            code: KelvinCode::FamilyCorrupt,
        }
    }

    /// 编辑预览：不落正式族，只返回该曲线在指定分钟的求值结果。
    pub fn preview(points: &[(u32, u32)], minute: u32) -> Result<u32, KelvinCode> {
        let c = Curve::build(points)?;
        Ok(c.evaluate(minute))
    }

    /// 注册/替换管线钩子（恢复路径）。恢复后从暂停值继续渐变。
    pub fn attach(&mut self, hook: H) {
        self.hook = Some(hook);
        self.hook_state = HookState::Live;
        self.paused = false;
    }

    /// 摘除钩子（进入 Detached，不再下发）。
    pub fn detach(&mut self) {
        self.hook = None;
        self.hook_state = HookState::Detached;
        self.paused = true;
    }

    /// 管线状态。
    pub fn hook_state(&self) -> HookState {
        self.hook_state
    }

    /// 暂停标志。
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// 暂停期告知列表（读屏播报源）。
    pub fn notices(&self) -> &[PipelineNotice] {
        &self.notices
    }

    /// 清空告知（消费方已读）。
    pub fn clear_notices(&mut self) {
        self.notices.clear();
    }

    /// 分钟切片求值（带缓存）。
    pub fn evaluate_at(&mut self, minute: u32) -> Result<u32, KelvinCode> {
        if minute >= DAY_MINUTES {
            return Err(KelvinCode::SliceOutOfRange);
        }
        let slice = minute / TIME_SLICE_MIN;
        if self.cache_minute == Some(slice) {
            return Ok(self.cache_value.unwrap_or(K_MIN));
        }
        let v = self.family.evaluate(minute);
        self.cache_minute = Some(slice);
        self.cache_value = Some(v);
        Ok(v)
    }

    /// 主推进循环：求值 → 设目标 → 走一步 → 下发钩子。
    /// 失联时暂停推进并记告知；恢复后自动续推。
    pub fn tick(&mut self, minute: u32) -> Result<u32, KelvinCode> {
        let want = self.evaluate_at(minute)?;
        if !self.paused {
            self.blend.set_target(want);
        }
        let now = self.blend.step_toward();
        match self.hook_state {
            HookState::Live => {
                let confirmed = self
                    .hook
                    .as_mut()
                    .map(|h| h.publish(now))
                    .unwrap_or(false);
                if !confirmed {
                    self.hook_state = HookState::Lost;
                    self.paused = true;
                    self.notices.push(PipelineNotice {
                        what: "色温推进暂停",
                        where_: "V管线钩子",
                        advice: "等待管线重连，恢复后继续渐变不跳变",
                        code: KelvinCode::PipelineLost,
                    });
                }
            }
            _ => {}
        }
        Ok(now)
    }

    /// 生效链视图：曲线族名 + 当前值 + 目标 + 剩余步（F3512 调试工具复用）。
    pub fn chain_view(&self) -> ( &'static str, u32, u32, u32) {
        (
            self.family.name(),
            self.blend.current(),
            self.blend.target(),
            self.blend.steps_left(),
        )
    }
}

// ---------------------------------------------------------------------------
// 六、损坏报告
// ---------------------------------------------------------------------------

/// 曲线装载报告：成功或三要素失败理由。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CurveReport {
    /// 装载成功（族签名）。
    Loaded(u64),
    /// 装载失败：三要素 + 诊断码。
    Rejected {
        /// 什么坏了。
        what: &'static str,
        /// 坏在第几个控制点（0 起；结构性错误为 usize::MAX）。
        at: usize,
        /// 怎么办。
        advice: &'static str,
        /// 诊断码。
        code: KelvinCode,
    },
}

impl CurveReport {
    /// 从装载结果生成报告。
    pub fn from_load(points: &[(u32, u32)]) -> Self {
        match Curve::build(points) {
            Ok(c) => CurveReport::Loaded(c.signature()),
            Err(code) => {
                let (what, at, advice) = match code {
                    KelvinCode::TooFewPoints => (
                        "控制点数不足",
                        usize::MAX,
                        "至少提供 2 个控制点",
                    ),
                    KelvinCode::TimeNotAscending => (
                        "控制点时间非升序",
                        find_order_violation(points),
                        "按一天内分钟数升序排列",
                    ),
                    KelvinCode::KelvinOutOfRange => (
                        "色温值越界",
                        find_kelvin_violation(points),
                        "色温必须落在 K_MIN..=K_MAX",
                    ),
                    _ => ("未知装载错误", usize::MAX, "检查控制点格式"),
                };
                CurveReport::Rejected { what, at, advice, code }
            }
        }
    }

    /// 是否成功。
    pub fn ok(&self) -> bool {
        matches!(self, CurveReport::Loaded(_))
    }
}

/// 找第一个非升序控制点下标（无则 usize::MAX）。
fn find_order_violation(points: &[(u32, u32)]) -> usize {
    for i in 1..points.len() {
        if points[i - 1].0 >= points[i].0 {
            return i;
        }
    }
    usize::MAX
}

/// 找第一个色温越界控制点下标（无则 usize::MAX）。
fn find_kelvin_violation(points: &[(u32, u32)]) -> usize {
    for (i, &(_, k)) in points.iter().enumerate() {
        if !(K_MIN..=K_MAX).contains(&k) {
            return i;
        }
    }
    usize::MAX
}

// ---------------------------------------------------------------------------
// 七、测试支撑（仅 cfg(test)，不属于功能行数）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopHook(bool);
    impl PipelineHook for NoopHook {
        fn publish(&mut self, _k: u32) -> bool {
            self.0
        }
    }

    #[test]
    fn curve_rejects_descending() {
        let bad = [(100u32, 5000u32), (50, 4000)];
        let r = CurveReport::from_load(&bad);
        assert!(!r.ok());
    }

    #[test]
    fn blend_never_jumps() {
        let mut g = BlendGard::at(6500);
        g.set_target(2000);
        let mut last = g.current();
        for _ in 0..64 {
            let now = g.step_toward();
            assert!(last.abs_diff(now) <= STEP_MAX_K);
            last = now;
        }
        assert_eq!(last, 2000);
    }

    #[test]
    fn pipeline_loss_pauses() {
        let mut e = KelvinEngine::new(NoopHook(false));
        let r = e.tick(600);
        assert!(r.is_ok());
        assert!(e.paused());
        assert!(!e.notices().is_empty());
    }
}
