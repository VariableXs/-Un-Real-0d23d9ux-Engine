//! VE-F2408 · 动画调试数据（VE-M 域 · 动画系统 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2408`
//!
//! **判据（锚点原文）**：三负载、按需拾取、协议家族、M 段注册、判据。
//!
//! 本条是 M 域的**可视化面**，对上（编辑器）提供四类调试负载，对下（内核）
//! 只读消费 F2407 的求值计数器。**它不做任何求值**——求值是 F2407 的职责，
//! 本条只负责「把已经算出来的东西，按需、可视、口径唯一地端出去」。
//!
//! 1. **三负载**（判据一）。锚点点名对接 VE-Y 的三负载，本条逐个落地为
//!    独立数据结构，**三者的采样纪律各不相同**（这是最容易写混的地方）：
//!    - **曲线可视** `CurvePick`：关键帧序列 + 插值器标记 → 绘制数据。
//!      **按需拉取**（F2013 拾取家族）：调用方给「轨道 + 时间窗」，本条只
//!      返回窗内那段，不返回整条轨——这是「零常驻」的实现方式。
//!    - **当前值流** `ValueStream`：活跃轨道当前值的**定容环形缓冲**。
//!      环形而非增长队列：调试流的价值在「最近」，不在「全量」。
//!    - **权重可视** `WeightCell`：多轨权重 → 热力标记，**三冗余**。
//!
//! 2. **按需拾取**（判据二）。曲线洪水（万轨全画）的防线是两级：
//!    - **LOD 抽稀**：窗内关键帧数超预算时抽稀，但**两端点必须钉死**
//!      （`pinned endpoints`）。这是抽稀唯一容易写错的地方——若端点也
//!      按步长抽，曲线画出来的时间范围就是假的，端点外插值全错。
//!      抽稀后 `total_keys`（抽稀前真实关键帧数）与 `points.len()`（实际
//!      交付点数）**分别如实记账**，绘制端才知道自己拿到的是抽稀视图。
//!    - **非有限值钳制**：NaN/Inf **绝不进绘制缓冲**——一个 NaN 会顺着
//!      画线算法污染整段曲线（这是曲线编辑器最经典的整屏消失故障）。
//!      钳制 + 计数 + 告警三件齐做，不静默。
//!
//! 3. **协议家族**（判据三）。F1946 协议家族复用：曲线编辑指令 → 生效 ACK。
//!    锚点要求「曲线实时调优 <1 帧」。**关于「帧」的诚实标注**：内核 no_std
//!    无墙钟，本条以**真实工作单元计数**（`work_units`，由拾取/写回的实际
//!    操作数累加，**不是自证式常数**）作为「<1 帧」的机检口径；墙钟口径由
//!    **F2412 动画基准**在目标机器定标。这与 F2407 对「SIMD 收益 4-6×」的
//!    处理同一纪律：可机检的先机检，不可在本条定标的如实移交。
//!
//!    ACK 有**两态**（Applied / Rejected）。二者对序号水位线的处理
//!    **刻意不同**：生效推进 `last_seq`；**因重放/序号回退而拒的不推进**
//!    ——被拒的指令没有生效过，推进水位线会让同一指令二次被拒于「已见过」，
//!    掩盖真实原因。但**因参数非法（下标越界/破坏单调）而拒的必须推进**：
//!    那条指令已被消费方读懂并作废，不推进会让它被无限重投。
//!
//! 4. **M 段注册**（判据四）。F1764 负载族 M 段四类型注册（曲线/值流/
//!    权重/统计）。**信封漂移 → 对账拦截**：`Envelope::checksum()` 是
//!    对（kind, schema, byte_len, seq）的 FNV-1a 摘要，注册时把摘要存进
//!    `declared`；`reconcile()` 重算并比对，不一致即判漂移。
//!    **漂移的后果不是「记一笔」而是「拦截」**：`EnvelopeRegistry` 置
//!    `intercepted` 后，`take()` 对所有类型一律返回 `None`——漂移的信封
//!    不得再被消费端取用。只加计数器不拦截，等于把红线降级成日志。
//!
//! 5. **发行版剔除零成本**（判据五）。`StripGuard` 的零成本是**可证伪的**：
//!    `payload_builds` 在 `Release` 下**恒为 0——包括被强行尝试的那次**
//!    （强行构建只会记 `forced_attempts` 与 P1，不递增 `payload_builds`）。
//!    光断「Release 下为 0」是恒真弱门禁：删掉整个字段照样通过。
//!    判据必须**双向验证**：`Debug` 档下 `payload_builds` 必须**真的递增**
//!    （对照组证明计数器不是死的），`Release` 档下连尝试路径也不递增。
//!
//! 跨批对接：F2407 求值计数器（缓存命中率/求值耗时取数单源）；F2403 插值器
//!   枚举（曲线标记复用，**不另造插值器**）；F2013 拾取家族；F1946/F2102
//!   调优协议家族；F1764 负载族 M 段；F2418 遥测（统计口径联动）。
//!
//! 零 panic 面、零 IO、无全局可变状态（所有状态由调用方持有并显式传入）。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::vem03_interp::Interp;
use super::vem07_perf::{bisect, Telemetry, MAX_CHANNELS, SoaTrack};

// ---------------------------------------------------------------------------
// 一、诊断家族（自有码段 0x2Bxx；0x2Axx 归 F2407，6001 段归 F2406）
// ---------------------------------------------------------------------------

/// 诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagCode(pub u16);

impl DiagCode {
    /// 拾取窗口内无关键帧。
    pub const PICK_WINDOW_EMPTY: DiagCode = DiagCode(0x2B01);
    /// 拾取预算耗尽，已按 LOD 抽稀交付。
    pub const PICK_BUDGET_EXHAUSTED: DiagCode = DiagCode(0x2B02);
    /// 轨道下标越界。
    pub const PICK_TRACK_OUT_OF_RANGE: DiagCode = DiagCode(0x2B03);
    /// 关键帧值非有限（NaN/Inf），已钳制后才进绘制缓冲。
    pub const CURVE_KEY_NON_FINITE: DiagCode = DiagCode(0x2B04);
    /// 值流环形缓冲已覆盖最旧样本。
    pub const STREAM_OVERWRITTEN: DiagCode = DiagCode(0x2B05);
    /// 比率类统计量分母为零，口径未定义（**不等于 0**）。
    pub const STATS_RATIO_UNDEFINED: DiagCode = DiagCode(0x2B06);
    /// 统计洪水，已降档（分辨率减半）。
    pub const STATS_DOWNGRADE: DiagCode = DiagCode(0x2B07);
    /// 权重越界（<0 或 >1），已钳制。
    pub const WEIGHT_OUT_OF_RANGE: DiagCode = DiagCode(0x2B08);
    /// 三冗余不自洽：色/大小/数值标签与权重对不上。
    pub const REDUNDANCY_DRIFT: DiagCode = DiagCode(0x2B09);
    /// 调优指令序号回退（重放或乱序），已拒。
    pub const TUNE_SEQ_REGRESSED: DiagCode = DiagCode(0x2B0A);
    /// 调优生效工作单元超一帧预算。
    pub const TUNE_WORK_OVER_BUDGET: DiagCode = DiagCode(0x2B0B);
    /// 信封漂移：重算摘要与注册时声明不符。
    pub const ENVELOPE_DRIFT: DiagCode = DiagCode(0x2B0C);
    /// 信封类型未注册。
    pub const ENVELOPE_UNKNOWN_PAYLOAD: DiagCode = DiagCode(0x2B0D);
    /// 发行版构建下强行请求调试负载（剔除失败）。
    pub const STRIP_FAILED: DiagCode = DiagCode(0x2B0E);
    /// 拾取时间窗端点倒置（`t1 < t0`），已归一。
    pub const PICK_WINDOW_INVERTED: DiagCode = DiagCode(0x2B0F);
    /// 关键帧时刻改写会破坏时间轴非递减（二分前提），已拒。
    pub const TIMES_NOT_MONOTONIC: DiagCode = DiagCode(0x2B10);
    /// 请求窗整体落在时间轴范围外，已钳制到最近端点。
    ///
    /// **为什么单独给码**：早先这里直接返回端点帧而**不记任何诊断**——
    /// 绘制端看到的是「窗在轴外，却拿到了数据」，无从判断这是钳制结果
    /// 还是真有数据。F2407 对同类钳制给 `TIME_CLAMPED`，本条沿同纪律。
    pub const PICK_WINDOW_CLAMPED: DiagCode = DiagCode(0x2B11);

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            DiagCode::PICK_WINDOW_EMPTY => "拾取窗口内无关键帧",
            DiagCode::PICK_BUDGET_EXHAUSTED => "拾取预算耗尽，按 LOD 抽稀",
            DiagCode::PICK_TRACK_OUT_OF_RANGE => "轨道下标越界",
            DiagCode::CURVE_KEY_NON_FINITE => "关键帧值非有限，已钳制",
            DiagCode::STREAM_OVERWRITTEN => "值流已覆盖最旧样本",
            DiagCode::STATS_RATIO_UNDEFINED => "统计比率分母为零，口径未定义",
            DiagCode::STATS_DOWNGRADE => "统计洪水，已降档",
            DiagCode::WEIGHT_OUT_OF_RANGE => "权重越界，已钳制",
            DiagCode::REDUNDANCY_DRIFT => "三冗余不自洽",
            DiagCode::TUNE_SEQ_REGRESSED => "调优指令序号回退，已拒",
            DiagCode::TUNE_WORK_OVER_BUDGET => "调优工作单元超一帧预算",
            DiagCode::ENVELOPE_DRIFT => "信封漂移",
            DiagCode::ENVELOPE_UNKNOWN_PAYLOAD => "信封类型未注册",
            DiagCode::STRIP_FAILED => "发行版强行请求调试负载",
            DiagCode::PICK_WINDOW_INVERTED => "拾取窗口端点倒置，已归一",
            DiagCode::TIMES_NOT_MONOTONIC => "时刻改写破坏时间轴单调，已拒",
            DiagCode::PICK_WINDOW_CLAMPED => "拾取窗在轴外，已钳制到最近端点",
            other => {
                let _ = other;
                "未登记诊断码"
            }
        }
    }

    /// 全部码（供家族完整性判据）。
    pub const ALL: [DiagCode; 17] = [
        DiagCode::PICK_WINDOW_EMPTY,
        DiagCode::PICK_BUDGET_EXHAUSTED,
        DiagCode::PICK_TRACK_OUT_OF_RANGE,
        DiagCode::CURVE_KEY_NON_FINITE,
        DiagCode::STREAM_OVERWRITTEN,
        DiagCode::STATS_RATIO_UNDEFINED,
        DiagCode::STATS_DOWNGRADE,
        DiagCode::WEIGHT_OUT_OF_RANGE,
        DiagCode::REDUNDANCY_DRIFT,
        DiagCode::TUNE_SEQ_REGRESSED,
        DiagCode::TUNE_WORK_OVER_BUDGET,
        DiagCode::ENVELOPE_DRIFT,
        DiagCode::ENVELOPE_UNKNOWN_PAYLOAD,
        DiagCode::STRIP_FAILED,
        DiagCode::PICK_WINDOW_INVERTED,
        DiagCode::TIMES_NOT_MONOTONIC,
        DiagCode::PICK_WINDOW_CLAMPED,
    ];
}

/// 诊断严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 记账，不阻断。
    Minor,
    /// 显性告警：条件成立即记录，无需人工介入。
    Major,
    /// 立案：需要人看一眼。
    P1,
}

/// 一条诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 诊断码。
    pub code: DiagCode,
    /// 严重度。
    pub severity: Severity,
}

/// 诊断袋（**P1 是可查询的一等公民**，不另立类型——避免同一事实两处记账）。
#[derive(Clone, Debug, Default)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    /// 新建空袋。
    #[allow(clippy::new_without_default)]
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new() }
    }

    /// 记一条（记账级）。
    pub fn push(&mut self, code: DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::Minor });
    }

    /// 记一条显性告警。
    pub fn push_major(&mut self, code: DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::Major });
    }

    /// 记一条立案。
    pub fn push_p1(&mut self, code: DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::P1 });
    }

    /// 全部诊断。
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 某码出现次数。
    pub fn count(&self, code: DiagCode) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].code == code {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 某严重度条数。
    pub fn count_severity(&self, sev: Severity) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].severity == sev {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 是否出现过某码。
    pub fn has(&self, code: DiagCode) -> bool {
        self.count(code) > 0
    }

    /// P1 条数（立案数——剔除失败/口径事故/漂移都必须进这里）。
    pub fn p1_count(&self) -> usize {
        self.count_severity(Severity::P1)
    }

    /// 人话渲染（非空即「诊断面不是哑巴」的可机检证据）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.items.len() {
            let d = self.items[i];
            let _ = s.push_str(&format!("[{:?}/{}] {}\n", d.severity, d.code.0, d.code.label()));
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 二、曲线可视负载（按需拾取 + LOD 抽稀 + 端点钉死）
// ---------------------------------------------------------------------------

/// 单次拾取交付点数上限（绘制端的一次性缓冲尺度）。
pub const MAX_CURVE_POINTS: usize = 64;

/// 一个绘制点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvePoint {
    /// 关键帧时刻。
    pub t_ms: u32,
    /// 通道值（**恒为有限值**——NaN/Inf 在此之前已被钳制）。
    pub values: [f32; MAX_CHANNELS],
    /// 有效通道数。
    pub lanes: u8,
    /// 插值器标记（**复用 F2403 `Interp`，不另造插值器**——单源纪律）。
    pub interp: Interp,
}

/// 一次拾取的结果。
#[derive(Clone, Debug, PartialEq)]
pub struct CurvePick {
    /// 轨道下标。
    pub track: usize,
    /// 交付绘制点（**已抽稀**，长度 ≤ `budget`）。
    pub points: Vec<CurvePoint>,
    /// 窗内真实关键帧数（**抽稀前**）。
    pub total_keys: u32,
    /// 本次实际考察的关键帧数（**真实工作单元**，非自证式算术）。
    pub scanned: u32,
    /// 是否发生了 LOD 抽稀。
    pub decimated: bool,
    /// 非有限值被钳制的个数。
    pub clamped: u32,
}

impl CurvePick {
    /// 交付点数。
    pub fn delivered(&self) -> usize {
        self.points.len()
    }

    /// 被抽稀掉的关键帧数（`total - delivered`；未抽稀时为 0）。
    pub fn dropped(&self) -> u32 {
        self.total_keys.saturating_sub(self.points.len() as u32)
    }

    /// 抽稀比（整数口径 `(分子, 分母)`，避免浮点——判据要精确对账）。
    pub fn decimation_ratio(&self) -> (u32, u32) {
        (self.total_keys, self.points.len() as u32)
    }

    /// 空拾取（各拒绝路径共用一个构造，避免五处各写一遍）。
    fn empty(track_idx: usize) -> CurvePick {
        CurvePick {
            track: track_idx,
            points: Vec::new(),
            total_keys: 0,
            scanned: 0,
            decimated: false,
            clamped: 0,
        }
    }
}

/// 非有限值钳制：NaN/Inf → `fallback`，并如实标记。
fn finite_or(value: f32, fallback: f32) -> (f32, bool) {
    if value.is_finite() {
        (value, false)
    } else {
        (fallback, true)
    }
}

/// **按需拾取**：只返回 `t0..=t1` 时间窗内的关键帧，超预算按 LOD 抽稀。
///
/// **端点钉死纪律**：抽稀时 `lo` 与 `hi` 两个端点**必进**输出。
/// 端点若也按步长抽，绘制端拿到的时间跨度就是假的——首尾外插值会连到
/// 错误的帧上。中间点按整数步长均匀取，取点公式 `lo+1+(j*span)/count`
/// 在 `span ≥ count` 时**步长恒 ≥1**（严格递增由构造保证，不靠事后断言）。
///
/// `budget == 0` 时返回空并记 `PICK_BUDGET_EXHAUSTED`——**不静默给全量**
/// （那正是洪水场景要防的事）。
pub fn pick_curve(
    track: &SoaTrack,
    track_idx: usize,
    t0: u32,
    t1: u32,
    budget: usize,
    interp: Interp,
    bag: &mut DiagBag,
) -> CurvePick {
    // 窗口端点倒置：归一 + 显性记录（不静默交换——调用方可能传反了）。
    let (lo_t, hi_t) = if t1 < t0 {
        bag.push_major(DiagCode::PICK_WINDOW_INVERTED);
        (t1, t0)
    } else {
        (t0, t1)
    };

    let times = &track.times;
    if times.is_empty() {
        bag.push(DiagCode::PICK_WINDOW_EMPTY);
        return CurvePick::empty(track_idx);
    }
    if budget == 0 {
        bag.push_major(DiagCode::PICK_BUDGET_EXHAUSTED);
        return CurvePick::empty(track_idx);
    }

    // 首个 `times[i] >= lo_t`：`bisect` 返回最后一个 `<= t0` 的下标，
    // 故「等于则原地、否则后移一格」；`t0 > times[n-1]` 时自然落到 n-1。
    let mut lo = bisect(times, lo_t).index;
    if lo < times.len() && times[lo] < lo_t {
        lo += 1;
    }
    // 末个 `times[i] <= hi_t`：`bisect` 已给候选，再向后吃掉等值/更小者。
    let mut hi = bisect(times, hi_t).index;
    while hi + 1 < times.len() && times[hi + 1] <= hi_t {
        hi += 1;
    }
    // 整条时间轴都在窗右（`hi_t < times[0]`）：`bisect` 返回 0 但 `times[0] > hi_t`。
    if times[hi] > hi_t || lo > hi {
        bag.push(DiagCode::PICK_WINDOW_EMPTY);
        return CurvePick::empty(track_idx);
    }

    // **轴外钳制必须显性**：整窗落在时间轴右端之外时，`lo`/`hi` 都落到
    // 末帧，交付的是「钳制到端点」的结果而不是窗内真实数据。绘制端若
    // 不知道这件事，会把端点当真实帧画出去。所以此处记码而非静默返回。
    if lo_t > times[times.len() - 1] {
        bag.push_major(DiagCode::PICK_WINDOW_CLAMPED);
    }

    let total = hi - lo + 1;
    let total_keys = total as u32;
    // 拾取本身要考察的关键帧数：与交付量无关（抽稀也在看每一个）——
    // 这是「调优工作单元」的真实来源。
    let scanned = total_keys;

    let decimated = total > budget;
    let mut points: Vec<CurvePoint> = Vec::new();
    let mut clamped_total = 0u32;

    // 采集一个关键帧：值先过非有限钳制，绝不把 NaN 交给绘制端。
    let push_key = |idx: usize, points: &mut Vec<CurvePoint>, clamped: &mut u32| {
        let mut buf = [0.0f32; MAX_CHANNELS];
        let lanes = track.key_slice(idx, &mut buf);
        let mut c = 0usize;
        while c < lanes {
            let (v, bad) = finite_or(buf[c], 0.0);
            buf[c] = v;
            if bad {
                *clamped += 1;
            }
            c += 1;
        }
        points.push(CurvePoint { t_ms: track.times[idx], values: buf, lanes: lanes as u8, interp });
    };

    if !decimated {
        let mut i = lo;
        while i <= hi {
            push_key(i, &mut points, &mut clamped_total);
            i += 1;
        }
    } else {
        bag.push(DiagCode::PICK_BUDGET_EXHAUSTED);
        if budget == 1 {
            // 预算只够一个点：取窗首（**首点优先**，绘制端至少有个锚）。
            push_key(lo, &mut points, &mut clamped_total);
        } else {
            // 端点钉死 + 中间均匀抽稀。
            //
            // **输出必须按时刻单调递增**：首点 → 中间点 → 末点，三段按序
            // 追加。早先把末点排在中间点之前，产出 `[100, 600, 200]` 这种
            // 乱序序列——绘制端按序连线会画出回折线，且「严格递增」一旦
            // 成为契约，后续任何抽稀改动都必须守住它。**顺序由构造保证**，
            // 不靠事后排序（排序等于把问题藏起来）。
            push_key(lo, &mut points, &mut clamped_total);
            let interior = budget - 2;
            let span = total - 2;
            let mut j = 0usize;
            while j < interior {
                let off = (j as u64 * span as u64 / interior as u64) as usize;
                push_key(lo + 1 + off, &mut points, &mut clamped_total);
                j += 1;
            }
            push_key(hi, &mut points, &mut clamped_total);
        }
    }

    if clamped_total > 0 {
        bag.push_major(DiagCode::CURVE_KEY_NON_FINITE);
    }

    CurvePick { track: track_idx, points, total_keys, scanned, decimated, clamped: clamped_total }
}

/// 轨道下标越界时的空拾取（显性拒绝，不 panic）。
pub fn pick_curve_by_index(
    tracks: &[SoaTrack],
    track_idx: usize,
    t0: u32,
    t1: u32,
    budget: usize,
    bag: &mut DiagBag,
) -> CurvePick {
    match tracks.get(track_idx) {
        Some(t) => pick_curve(t, track_idx, t0, t1, budget, Interp::Linear, bag),
        None => {
            bag.push_major(DiagCode::PICK_TRACK_OUT_OF_RANGE);
            CurvePick::empty(track_idx)
        }
    }
}

// ---------------------------------------------------------------------------
// 三、当前值流（定容环形缓冲 · 覆盖即记账）
// ---------------------------------------------------------------------------

/// 值流默认容量。
pub const VALUE_STREAM_CAP: usize = 256;

/// 一个值流样本。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ValueSample {
    /// 采样时刻。
    pub t_ms: u32,
    /// 轨道下标。
    pub track: u32,
    /// 通道值。
    pub values: [f32; MAX_CHANNELS],
    /// 有效通道数。
    pub lanes: u8,
}

/// 当前值流：定容环，**只保最近**。
///
/// 为什么环形而不是增长队列：调试流的消费端是「当前帧附近的曲线」，
/// 旧样本没有价值；环形把常驻内存钉死在 `cap × sizeof(ValueSample)`，
/// 与帧数、运行时长**无关**——这是「零常驻」的第二道保障。
#[derive(Clone, Debug)]
pub struct ValueStream {
    buf: Vec<ValueSample>,
    head: usize,
    filled: usize,
    overwritten: u32,
    non_finite: u32,
}

impl ValueStream {
    /// 新建（容量一次分配，此后不再增长）。
    #[allow(clippy::new_without_default)]
    pub fn new() -> ValueStream {
        ValueStream::with_capacity(VALUE_STREAM_CAP)
    }

    /// 指定容量。
    pub fn with_capacity(cap: usize) -> ValueStream {
        let cap = cap.max(1);
        let mut buf: Vec<ValueSample> = Vec::new();
        let mut i = 0usize;
        while i < cap {
            buf.push(ValueSample { t_ms: 0, track: 0, values: [0.0; MAX_CHANNELS], lanes: 0 });
            i += 1;
        }
        ValueStream { buf, head: 0, filled: 0, overwritten: 0, non_finite: 0 }
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// 有效样本数。
    pub fn len(&self) -> usize {
        self.filled
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// 被覆盖的样本累计数。
    pub fn overwritten(&self) -> u32 {
        self.overwritten
    }

    /// 非有限值被钳制的累计数。
    pub fn non_finite(&self) -> u32 {
        self.non_finite
    }

    /// 推入一个样本。返回 `false` 表示**覆盖了最旧样本**（已记账）。
    pub fn push(&mut self, sample: ValueSample, bag: &mut DiagBag) -> bool {
        let cap = self.buf.len();
        let mut s = sample;
        let mut c = 0usize;
        while c < s.lanes as usize && c < MAX_CHANNELS {
            let (v, bad) = finite_or(s.values[c], 0.0);
            s.values[c] = v;
            if bad {
                self.non_finite += 1;
            }
            c += 1;
        }
        let mut idx = self.head;
        if idx >= cap {
            idx = 0;
        }
        self.buf[idx] = s;
        self.head = idx + 1;
        if self.head >= cap {
            self.head = 0;
        }
        if self.filled < cap {
            self.filled += 1;
            true
        } else {
            self.overwritten += 1;
            bag.push(DiagCode::STREAM_OVERWRITTEN);
            false
        }
    }

    /// 最新样本（`None` 当且仅当流为空）。
    pub fn latest(&self) -> Option<&ValueSample> {
        if self.filled == 0 {
            return None;
        }
        let cap = self.buf.len();
        let mut idx = if self.head == 0 { cap - 1 } else { self.head - 1 };
        if idx >= cap {
            idx = cap - 1;
        }
        self.buf.get(idx)
    }

    /// 窗口内样本数（`(now - window_ms, now]` 半开区间）。
    pub fn window_count(&self, now: u32, window_ms: u32) -> usize {
        let cap = self.buf.len().max(1);
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.filled {
            let idx = (self.head + cap - 1 - i) % cap;
            if let Some(s) = self.buf.get(idx) {
                let in_window = now >= window_ms && s.t_ms > now - window_ms && s.t_ms <= now;
                if in_window {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 四、权重可视（三冗余：色 + 大小 + 数值标签）
// ---------------------------------------------------------------------------

/// 热力色档（冗余一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeatColor {
    /// 冷（低权重）。
    Cool,
    /// 温。
    Warm,
    /// 热。
    Hot,
    /// 峰值（满权重）。
    Critical,
}

/// 热力尺寸档（冗余二）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeatSize {
    /// 小。
    Small,
    /// 中。
    Medium,
    /// 大。
    Large,
    /// 特大。
    XLarge,
}

/// 一个权重热力单元。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeightCell {
    /// 轨道下标。
    pub track: u32,
    /// 权重（**恒在 `[0,1]`**）。
    pub weight: f32,
    /// 色档。
    pub color: HeatColor,
    /// 尺寸档。
    pub size: HeatSize,
    /// 数值标签（百分数整数 0..=100）。
    pub label_pct: u8,
}

impl WeightCell {
    /// 由权重造单元（**先钳制再分档**）。
    pub fn make(track: u32, weight: f32, bag: &mut DiagBag) -> WeightCell {
        let w = clamp_weight(weight, bag);
        WeightCell { track, weight: w, color: color_band(w), size: size_band(w), label_pct: label_of(w) }
    }

    /// **三冗余是否自洽**：三通道各自独立重算并与存储值比对。
    ///
    /// 这是「冗余存在」的**正向**门禁。注意它**不是**恒真门禁：
    /// 若有人把 `color` 直接从 `label_pct` 派生（两份同源），
    /// 只要有人改动其一，本判据即转红。
    pub fn redundant_consistent(&self, bag: &mut DiagBag) -> bool {
        let w = clamp_weight(self.weight, bag);
        let ok =
            self.color == color_band(w) && self.size == size_band(w) && self.label_pct == label_of(w);
        if !ok {
            bag.push_p1(DiagCode::REDUNDANCY_DRIFT);
        }
        ok
    }

    /// 与 `other` 是否构成「同色不同尺寸」。
    ///
    /// 若两套分档边界相同，「同色必同尺寸」恒成立，冗余退化成一份数据。
    /// **判据侧必须自己举出这样一对权重**，不能只断言函数返回 `true`
    /// （同源驱动恒真弱门禁）。
    pub fn channels_independent(&self, other: &WeightCell) -> bool {
        self.color == other.color && self.size != other.size
    }
}

/// 权重钳制到 `[0,1]`；NaN 归零并记账（NaN 权重会让三冗余全部失去意义）。
pub fn clamp_weight(weight: f32, bag: &mut DiagBag) -> f32 {
    if !weight.is_finite() {
        bag.push_major(DiagCode::WEIGHT_OUT_OF_RANGE);
        return 0.0;
    }
    if weight < 0.0 || weight > 1.0 {
        bag.push(DiagCode::WEIGHT_OUT_OF_RANGE);
        if weight < 0.0 {
            0.0
        } else {
            1.0
        }
    } else {
        weight
    }
}

/// 色档：四档，边界 1/4、1/2、3/4。
pub const fn color_band(w: f32) -> HeatColor {
    if w < 0.25 {
        HeatColor::Cool
    } else if w < 0.5 {
        HeatColor::Warm
    } else if w < 0.75 {
        HeatColor::Hot
    } else {
        HeatColor::Critical
    }
}

/// 尺寸档：四档，边界 **1/8、3/8、5/8**（与色档边界**故意错开**）。
///
/// 错开是刻意的：边界相同时「同色必同尺寸」，冗余退化成一份数据，
/// 色觉障碍用户与正常用户读到的东西完全一样——那不叫三冗余。
pub const fn size_band(w: f32) -> HeatSize {
    if w < 0.125 {
        HeatSize::Small
    } else if w < 0.375 {
        HeatSize::Medium
    } else if w < 0.625 {
        HeatSize::Large
    } else {
        HeatSize::XLarge
    }
}

/// 数值标签：百分数整数（四舍五入，不依赖 `f32::round` 以保 no_std 稳定）。
pub const fn label_of(w: f32) -> u8 {
    let pct = w * 100.0 + 0.5;
    if pct <= 0.0 {
        0u8
    } else if pct >= 100.0 {
        100u8
    } else {
        pct as u8
    }
}

/// 造一批权重热力单元。
pub fn make_weight_heatmap(weights: &[f32], bag: &mut DiagBag) -> Vec<WeightCell> {
    let mut out: Vec<WeightCell> = Vec::new();
    let mut i = 0usize;
    while i < weights.len() {
        out.push(WeightCell::make(i as u32, weights[i], bag));
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 五、求值统计（三统计量 · 口径唯一 · 洪水降档）
// ---------------------------------------------------------------------------

/// 统计窗样本上限（超此即降档）。
pub const STAT_WINDOW_MAX_SAMPLES: u32 = 256;

/// 百万分比（比率类统计量的整数口径——避免浮点除法的舍入争议）。
pub const PPM: u32 = 1_000_000;

/// 一次统计观测（单帧增量）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatSample {
    /// 活跃轨道数。
    pub active_tracks: u32,
    /// 本帧动画求值耗时（微秒）。
    pub eval_us: u32,
    /// 本帧总耗时（微秒；**占比的分母，口径由 F2418 钉死**）。
    pub frame_us: u32,
}

/// 统计窗（秒级聚合的累加体）。
#[derive(Clone, Copy, Debug, Default)]
pub struct StatWindow {
    active_tracks: u32,
    peak_active_tracks: u32,
    eval_us: u64,
    frame_us: u64,
    cache_hits: u64,
    cache_recompute: u64,
    samples: u32,
    downgrades: u32,
    /// 降档步长：分辨率已折半的次数。
    stride: u32,
}

/// 统计摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatSummary {
    /// 当前活跃轨道数。
    pub active_tracks: u32,
    /// 窗内峰值活跃轨道数。
    pub peak_active_tracks: u32,
    /// 求值耗时占比（百万分比；`None` = 口径未定义，**不是 0**）。
    pub eval_share_ppm: Option<u32>,
    /// 缓存命中率（百万分比；`None` = 口径未定义）。
    pub cache_hit_ppm: Option<u32>,
    /// 求值是否超帧耗时（占比 > 100%）。
    pub eval_over_budget: bool,
    /// 参与聚合的样本数。
    pub samples: u32,
    /// 降档次数。
    pub downgrades: u32,
    /// 降档步长（1 = 未降档）。
    pub stride: u32,
}

impl StatWindow {
    /// 新建空窗。
    pub fn new() -> StatWindow {
        StatWindow { stride: 1, ..Default::default() }
    }

    /// 观测一帧。返回 `true` 表示**本次触发了降档**。
    ///
    /// **降档语义**：样本数触顶时**折叠**累加体（各累加量折半、样本数折半、
    /// 步长翻倍），而不是「新旧分辨率混在一窗里」。混档会让「这一窗代表
    /// 多长时间」变得不可回答——那正是统计口径唯一性要防的事。
    pub fn observe(&mut self, sample: StatSample, tele: &Telemetry, bag: &mut DiagBag) -> bool {
        self.active_tracks = sample.active_tracks;
        if sample.active_tracks > self.peak_active_tracks {
            self.peak_active_tracks = sample.active_tracks;
        }
        self.eval_us = self.eval_us.saturating_add(sample.eval_us as u64);
        self.frame_us = self.frame_us.saturating_add(sample.frame_us as u64);
        self.cache_hits = self.cache_hits.saturating_add(tele.cache_hits as u64);
        self.cache_recompute = self.cache_recompute.saturating_add(tele.cache_recompute as u64);
        self.samples = self.samples.saturating_add(1);
        if self.samples >= STAT_WINDOW_MAX_SAMPLES {
            self.fold();
            bag.push(DiagCode::STATS_DOWNGRADE);
            return true;
        }
        false
    }

    /// 折叠降档：分辨率减半，覆盖时长加倍。
    fn fold(&mut self) {
        self.eval_us /= 2;
        self.frame_us /= 2;
        self.cache_hits /= 2;
        self.cache_recompute /= 2;
        self.samples /= 2;
        // 降档后峰值口径改为「当前窗内峰值」——历史峰值已被折半语义覆盖，
        // 继续挂着旧峰值会与新分辨率不同量纲（口径不得漂移）。
        self.peak_active_tracks = self.active_tracks;
        self.downgrades = self.downgrades.saturating_add(1);
        self.stride = self.stride.saturating_mul(2);
    }

    /// 窗内有效样本数。
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// 降档次数。
    pub fn downgrades(&self) -> u32 {
        self.downgrades
    }

    /// 降档步长。
    pub fn stride(&self) -> u32 {
        self.stride
    }

    /// 摘要（口径未定义的比率返回 `None` 并记账，**绝不返回 0 冒充**）。
    pub fn summary(&self, bag: &mut DiagBag) -> StatSummary {
        let eval_share = if self.frame_us == 0 {
            bag.push_major(DiagCode::STATS_RATIO_UNDEFINED);
            None
        } else {
            let v = (self.eval_us as u128 * PPM as u128) / (self.frame_us as u128);
            Some(if v > u32::MAX as u128 { u32::MAX } else { v as u32 })
        };
        let denom = self.cache_hits + self.cache_recompute;
        let cache_hit = if denom == 0 {
            bag.push_major(DiagCode::STATS_RATIO_UNDEFINED);
            None
        } else {
            let v = (self.cache_hits as u128 * PPM as u128) / (denom as u128);
            Some(if v > u32::MAX as u128 { u32::MAX } else { v as u32 })
        };
        StatSummary {
            active_tracks: self.active_tracks,
            peak_active_tracks: self.peak_active_tracks,
            eval_share_ppm: eval_share,
            cache_hit_ppm: cache_hit,
            eval_over_budget: self.eval_us > self.frame_us,
            samples: self.samples,
            downgrades: self.downgrades,
            stride: self.stride,
        }
    }
}

// ---------------------------------------------------------------------------
// 六、调参协议（F1946 家族：曲线编辑指令 → 生效 ACK）
// ---------------------------------------------------------------------------

/// 一帧工作单元预算（60Hz 口径：16_667µs 的机检替身）。
///
/// **诚实标注**：本条以工作单元代替墙钟，**不是**「已经测得 <1 帧」。
/// 墙钟定标由 F2412 在目标机器完成（与 F2407 对 SIMD 收益的处理同纪律）。
pub const TUNE_WORK_BUDGET: u32 = 64;

/// 调优指令。
///
/// **`track` 与 `key` 是两个独立下标**：轨道下标 + 该轨内的关键帧下标。
/// 早先只留一个 `key` 并同时当轨道下标与帧下标用，结果是**只有 0 号轨的
/// 0 号帧可改**（`k` 既是 `tracks[k]` 又是 `times[k]`）——多轨场景下编辑器
/// 改第二轨的第三帧会静默落到别的目标上。两下标分开后语义唯一。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TuneOp {
    /// 改关键帧某通道值。
    SetKeyValue {
        /// 轨道下标。
        track: u32,
        /// 该轨内的关键帧下标。
        key: u32,
        /// 通道号。
        channel: u8,
        /// 新值。
        value: f32,
    },
    /// 改关键帧时刻。
    SetKeyTime {
        /// 轨道下标。
        track: u32,
        /// 该轨内的关键帧下标。
        key: u32,
        /// 新时刻。
        t_ms: u32,
    },
    /// 改轨道权重。
    SetWeight {
        /// 轨道下标。
        track: u32,
        /// 新权重。
        weight: f32,
    },
}

/// 调优指令信封（带序号，ACK 按序号对账）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TuneCommand {
    /// 指令序号（**必须严格递增**）。
    pub seq: u64,
    /// 指令体。
    pub op: TuneOp,
}

/// 生效回执（**两态**：生效 / 拒绝）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TuneAck {
    /// 已生效，附**真实工作单元数**与是否超预算。
    Applied {
        /// 对应指令序号。
        seq: u64,
        /// 工作单元数。
        work_units: u32,
        /// 是否超一帧预算。
        over_budget: bool,
    },
    /// 已拒绝，附拒绝码。
    Rejected {
        /// 对应指令序号。
        seq: u64,
        /// 拒绝码。
        code: DiagCode,
    },
}

impl TuneAck {
    /// 是否生效。
    pub const fn is_applied(&self) -> bool {
        matches!(self, TuneAck::Applied { .. })
    }

    /// 是否**因序号回退而拒**（与「参数非法而拒」是两种语义，判据分开钉）。
    pub const fn is_seq_reject(&self) -> bool {
        matches!(self, TuneAck::Rejected { code: DiagCode::TUNE_SEQ_REGRESSED, .. })
    }

    /// 序号。
    pub const fn seq(&self) -> u64 {
        match self {
            TuneAck::Applied { seq, .. } => *seq,
            TuneAck::Rejected { seq, .. } => *seq,
        }
    }
}

/// 调优镜像：编辑器侧被调的轨道状态（**由调用方持有，无全局态**）。
#[derive(Clone, Debug, Default)]
pub struct TrackMirror {
    /// 轨道。
    pub tracks: Vec<SoaTrack>,
    /// 权重。
    pub weights: Vec<f32>,
    /// 曲线可视脏标记（调优后须重绘）。
    pub curve_dirty: Vec<bool>,
    /// 累计交付工作单元（**实测**，供消费端核对）。
    pub total_delivered: u32,
}

/// 调优通道。
#[derive(Clone, Copy, Debug, Default)]
pub struct TuneChannel {
    /// 已接受的最高序号（0 = 尚无）。
    pub last_seq: u64,
    /// 生效计数。
    pub applied: u32,
    /// 拒绝计数。
    pub rejected: u32,
    /// 超预算计数。
    pub over_budget: u32,
    /// 累计工作单元。
    pub total_work: u64,
}

/// 单次调优的可见效果窗口（调参要「立即看见」，故取全轨可视窗）。
pub const TUNE_PREVIEW_WINDOW_MS: u32 = u32::MAX;

/// 应用一条调优指令。
///
/// **工作单元是实测的**：`SetKey*` 会触发该轨曲线的重新拾取，其
/// `scanned`（窗内真实关键帧数）就是本条指令的成本——不是常数、不是估值。
/// 改一个关键帧之所以可能超帧，正是因为它要重算整段可视曲线。
///
/// **序号水位线的两种语义**（刻意不同，判据分开钉）：
/// - 生效 → 推进 `last_seq`；
/// - **重放/序号回退而拒 → 不推进**（未生效，不该推进，否则二次被拒于
///   「已见过」，掩盖真实原因）；
/// - **参数非法（下标越界/破坏单调）而拒 → 推进**（该指令已被消费方作废，
///   不推进会被无限重投）。
pub fn apply_tune(
    ch: &mut TuneChannel,
    mirror: &mut TrackMirror,
    cmd: TuneCommand,
    bag: &mut DiagBag,
) -> TuneAck {
    if cmd.seq <= ch.last_seq {
        ch.rejected = ch.rejected.saturating_add(1);
        bag.push_major(DiagCode::TUNE_SEQ_REGRESSED);
        return TuneAck::Rejected { seq: cmd.seq, code: DiagCode::TUNE_SEQ_REGRESSED };
    }

    let mut work: u32 = 1;
    match cmd.op {
        TuneOp::SetKeyValue { track, key, channel, value } => {
            let t = track as usize;
            let k = key as usize;
            let mut reject = DiagCode::PICK_TRACK_OUT_OF_RANGE;
            let mut ok = false;
            if let Some(tr) = mirror.tracks.get_mut(t) {
                let lanes = tr.kind.lanes();
                let c = channel as usize;
                let slot = if c < lanes && k < tr.times.len() {
                    tr.channels.get_mut(k * lanes + c)
                } else {
                    None
                };
                match slot {
                    Some(dst) => {
                        let (v, bad) = finite_or(value, 0.0);
                        if bad {
                            bag.push_major(DiagCode::CURVE_KEY_NON_FINITE);
                        }
                        *dst = v;
                        tr.edit_rev = tr.edit_rev.wrapping_add(1);
                        ok = true;
                    }
                    None => reject = DiagCode::CURVE_KEY_NON_FINITE,
                }
            }
            if !ok {
                ch.last_seq = cmd.seq;
                ch.rejected = ch.rejected.saturating_add(1);
                bag.push_major(reject);
                return TuneAck::Rejected { seq: cmd.seq, code: reject };
            }
            // 关键帧被改 → 该轨曲线标脏 → 绘制端要重取（真实成本）。
            if let Some(flag) = mirror.curve_dirty.get_mut(t) {
                *flag = true;
            }
            work = work.saturating_add(repick_cost(mirror.tracks.get(t)));
        }
        TuneOp::SetKeyTime { track, key, t_ms } => {
            let t = track as usize;
            let k = key as usize;
            let mut reject = DiagCode::PICK_TRACK_OUT_OF_RANGE;
            let mut ok = false;
            if let Some(tr) = mirror.tracks.get_mut(t) {
                // **单调性守卫**：关键帧时间轴必须非递减——它是 `bisect`
                // 的前提（见 F2407 `TIMES_NOT_MONOTONIC`）。调参面板能
                // 拖动任意关键帧，不设守卫的话一次拖动就把整条轨的求值
                // 前提破坏掉，且**静默**：二分在乱序轴上不 panic，只是
                // 返回错区间，曲线画出来是错的。
                // 守卫口径：前驱 ≤ 新值 ≤ 后继（等宽单帧时间允许重复，
                // 与 F2407 的非递减语义一致）。
                let prev_ok = k == 0 || tr.times[k - 1] <= t_ms;
                let next_ok = k + 1 >= tr.times.len() || t_ms <= tr.times[k + 1];
                if prev_ok && next_ok {
                    if let Some(dst) = tr.times.get_mut(k) {
                        *dst = t_ms;
                        tr.edit_rev = tr.edit_rev.wrapping_add(1);
                        ok = true;
                    }
                } else {
                    reject = DiagCode::TIMES_NOT_MONOTONIC;
                }
            }
            if !ok {
                ch.last_seq = cmd.seq;
                ch.rejected = ch.rejected.saturating_add(1);
                bag.push_major(reject);
                return TuneAck::Rejected { seq: cmd.seq, code: reject };
            }
            if let Some(flag) = mirror.curve_dirty.get_mut(t) {
                *flag = true;
            }
            work = work.saturating_add(repick_cost(mirror.tracks.get(t)));
        }
        TuneOp::SetWeight { track, weight } => {
            let t = track as usize;
            let mut ok = false;
            if let Some(dst) = mirror.weights.get_mut(t) {
                *dst = clamp_weight(weight, bag);
                ok = true;
            }
            if !ok {
                ch.last_seq = cmd.seq;
                ch.rejected = ch.rejected.saturating_add(1);
                bag.push_major(DiagCode::PICK_TRACK_OUT_OF_RANGE);
                return TuneAck::Rejected { seq: cmd.seq, code: DiagCode::PICK_TRACK_OUT_OF_RANGE };
            }
            work = 1;
        }
    }

    mirror.total_delivered = mirror.total_delivered.saturating_add(work);
    ch.last_seq = cmd.seq;
    ch.applied = ch.applied.saturating_add(1);
    ch.total_work = ch.total_work.saturating_add(work as u64);
    let over = work > TUNE_WORK_BUDGET;
    if over {
        ch.over_budget = ch.over_budget.saturating_add(1);
        bag.push_p1(DiagCode::TUNE_WORK_OVER_BUDGET);
    }
    TuneAck::Applied { seq: cmd.seq, work_units: work, over_budget: over }
}

/// 重新拾取的成本（= 窗内真实关键帧数；轨道缺失则 0）。
fn repick_cost(track: Option<&SoaTrack>) -> u32 {
    match track {
        Some(t) => {
            let mut probe = DiagBag::new();
            pick_curve(t, 0, 0, TUNE_PREVIEW_WINDOW_MS, MAX_CURVE_POINTS, Interp::Linear, &mut probe)
                .scanned
        }
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// 七、信封 M 段注册（F1764 负载族 · 四类型 + 漂移对账拦截）
// ---------------------------------------------------------------------------

/// M 段负载类型（四类型：曲线/值流/权重/统计）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadKind {
    /// 曲线可视负载。
    Curve,
    /// 当前值流负载。
    ValueStream,
    /// 权重热力负载。
    Weight,
    /// 统计负载。
    Stats,
}

impl PayloadKind {
    /// 全部类型（M 段取值域）。
    pub const ALL: [PayloadKind; 4] =
        [PayloadKind::Curve, PayloadKind::ValueStream, PayloadKind::Weight, PayloadKind::Stats];

    /// 线上编码（**显式映射**，不用 `as u16`——枚举判别值不是线上值）。
    pub const fn wire(self) -> u16 {
        match self {
            PayloadKind::Curve => 0x01,
            PayloadKind::ValueStream => 0x02,
            PayloadKind::Weight => 0x03,
            PayloadKind::Stats => 0x04,
        }
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            PayloadKind::Curve => "曲线可视",
            PayloadKind::ValueStream => "当前值流",
            PayloadKind::Weight => "权重热力",
            PayloadKind::Stats => "求值统计",
        }
    }

    /// 按线上编码反查。
    pub fn from_wire(w: u16) -> Option<PayloadKind> {
        PayloadKind::ALL.iter().copied().find(|k| k.wire() == w)
    }
}

/// 信封 schema 版本（M 段 v1）。
pub const ENVELOPE_SCHEMA: u16 = 1;

/// M 段注册表容量（四类型）。
pub const M_SEGMENT_SLOTS: usize = 4;

/// 一个负载信封。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Envelope {
    /// 负载类型。
    pub kind: PayloadKind,
    /// schema 版本。
    pub schema: u16,
    /// 载荷字节数。
    pub byte_len: u32,
    /// 序号（重注册即递增，用于对账）。
    pub seq: u32,
    /// 注册时声明的摘要（**对账基准**）。
    pub declared: u32,
}

/// FNV-1a 单步。
const fn fnv_step(h: u32, byte: u32) -> u32 {
    (h ^ byte).wrapping_mul(0x0100_0193)
}

impl Envelope {
    /// 造信封并写入自摘要（注册面唯一构造口——`declared` 不许外部乱填）。
    pub fn new(kind: PayloadKind, byte_len: u32, seq: u32) -> Envelope {
        let mut e = Envelope { kind, schema: ENVELOPE_SCHEMA, byte_len, seq, declared: 0 };
        e.declared = e.checksum();
        e
    }

    /// 重算摘要（对四字段 + 标签长度的 FNV-1a）。
    pub fn checksum(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        h = fnv_step(h, self.kind.wire() as u32);
        h = fnv_step(h, self.schema as u32);
        h = fnv_step(h, self.byte_len);
        h = fnv_step(h, self.seq);
        h = fnv_step(h, self.kind.label().len() as u32);
        h
    }

    /// 是否漂移（重算摘要 ≠ 声明摘要）。
    pub fn drifted(&self) -> bool {
        self.checksum() != self.declared
    }
}

/// M 段信封注册表。
#[derive(Clone, Debug)]
pub struct EnvelopeRegistry {
    slots: Vec<Option<Envelope>>,
    epoch: u32,
    drift_count: u32,
    intercepted: bool,
}

impl EnvelopeRegistry {
    /// 新建（四槽空表）。
    #[allow(clippy::new_without_default)]
    pub fn new() -> EnvelopeRegistry {
        let mut slots: Vec<Option<Envelope>> = Vec::new();
        let mut i = 0usize;
        while i < M_SEGMENT_SLOTS {
            slots.push(None);
            i += 1;
        }
        EnvelopeRegistry { slots, epoch: 0, drift_count: 0, intercepted: false }
    }

    /// 注册一个信封（**重注册即替换**）。返回是否成功。
    pub fn register(&mut self, env: Envelope) -> bool {
        let wire = env.kind.wire();
        if wire == 0 || wire as usize > M_SEGMENT_SLOTS {
            return false;
        }
        self.epoch = self.epoch.wrapping_add(1);
        match self.slots.get_mut((wire - 1) as usize) {
            Some(cell) => {
                *cell = Some(env);
                true
            }
            None => false,
        }
    }

    /// 只读查表（**不受拦截影响**——对账自身要能看到信封）。
    pub fn peek(&self, kind: PayloadKind) -> Option<&Envelope> {
        let wire = kind.wire();
        if wire == 0 || wire as usize > M_SEGMENT_SLOTS {
            return None;
        }
        self.slots.get((wire - 1) as usize).and_then(|c| c.as_ref())
    }

    /// 消费端取用（**拦截态下一律 `None`**——漂移信封不得被消费）。
    pub fn take(&self, kind: PayloadKind) -> Option<&Envelope> {
        if self.intercepted {
            return None;
        }
        self.peek(kind)
    }

    /// 已注册类型数。
    pub fn registered(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 注册代数。
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// 累计漂移次数。
    pub fn drift_count(&self) -> u32 {
        self.drift_count
    }

    /// 是否处于拦截态。
    pub fn intercepted(&self) -> bool {
        self.intercepted
    }

    /// 显式解除拦截（**必须由人确认后调用**，不随注册自动清除）。
    pub fn clear_intercept(&mut self) {
        self.intercepted = false;
    }

    /// 对账：重算全部信封摘要。返回漂移数；**有漂移即置拦截**。
    pub fn reconcile(&mut self, bag: &mut DiagBag) -> u32 {
        let mut drifted = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            let bad = match self.slots.get(i).and_then(|c| c.as_ref()) {
                Some(e) => e.drifted(),
                None => false,
            };
            if bad {
                drifted += 1;
            }
            i += 1;
        }
        if drifted > 0 {
            self.drift_count = self.drift_count.saturating_add(drifted);
            self.intercepted = true;
            bag.push_p1(DiagCode::ENVELOPE_DRIFT);
        }
        drifted
    }

    /// 注册表是否齐备（四类型全注册）。
    pub fn complete(&self) -> bool {
        self.registered() == M_SEGMENT_SLOTS
    }
}

// ---------------------------------------------------------------------------
// 八、发行版剔除（零成本，可证伪）
// ---------------------------------------------------------------------------

/// 构建档位。
///
/// `Default` 取 `Debug`：**调试是安全默认**——想要「零成本剔除」必须
/// 显式写 `BuildProfile::Release`。反过来的默认（发行默认）会让忘配的
/// 发行构建悄悄带上全部调试负载。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BuildProfile {
    /// 开发形态：调试负载可用。
    #[default]
    Debug,
    /// 发行形态：调试负载**零成本剔除**。
    Release,
}

/// 剔除守卫。
#[derive(Clone, Copy, Debug, Default)]
pub struct StripGuard {
    /// 构建档位。
    pub profile: BuildProfile,
    /// 已构建的调试负载数（**Release 档下恒为 0，含强行尝试路径**）。
    pub payload_builds: u64,
    /// 被强行请求的次数（Release 档下只增此项）。
    pub forced_attempts: u64,
}

impl StripGuard {
    /// 构造。
    pub const fn new(profile: BuildProfile) -> StripGuard {
        StripGuard { profile, payload_builds: 0, forced_attempts: 0 }
    }

    /// 当前档位是否允许调试负载。
    pub const fn payloads_enabled(&self) -> bool {
        matches!(self.profile, BuildProfile::Debug)
    }

    /// 请求构建一个调试负载。返回是否真的构建。
    ///
    /// **零成本的物质保证**：Release 档下 `payload_builds` **不递增**——
    /// 不是「构建完再抹掉」，而是**根本不进入构建**。强行请求只记
    /// `forced_attempts` 与一条 P1。
    pub fn try_build(&mut self, bag: &mut DiagBag) -> bool {
        if self.payloads_enabled() {
            self.payload_builds += 1;
            true
        } else {
            self.forced_attempts += 1;
            bag.push_p1(DiagCode::STRIP_FAILED);
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 九、族声明与冒烟
// ---------------------------------------------------------------------------

/// 家族声明一致性（判据用）。
pub fn family_is_consistent() -> bool {
    PayloadKind::ALL.len() == M_SEGMENT_SLOTS && DiagCode::ALL.len() == 17
}

/// 四类型码互异（`wire()` 显式映射的机检——防两类型同码）。
pub fn wires_unique() -> bool {
    let mut seen: Vec<u16> = Vec::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < PayloadKind::ALL.len() {
        let w = PayloadKind::ALL[i].wire();
        if seen.contains(&w) {
            ok = false;
        } else {
            seen.push(w);
        }
        i += 1;
    }
    ok
}

/// 码标签互异（防两码共用一句人话——那是最难查的一类缺陷）。
pub fn labels_unique() -> bool {
    let mut i = 0usize;
    while i < DiagCode::ALL.len() {
        let mut j = i + 1;
        while j < DiagCode::ALL.len() {
            if DiagCode::ALL[i].label() == DiagCode::ALL[j].label() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

fn demo_track(times: Vec<u32>, vals: Vec<f32>) -> SoaTrack {
    use super::vem07_perf::{TrackClass, TrackValueKind};
    SoaTrack::new("demo", TrackClass::Continuous, TrackValueKind::Scalar, false, times, vals)
}

/// 描述。
pub fn describe() -> String {
    let mut s = String::new();
    let _ = s.push_str("动画调试数据：\n");
    let _ = s.push_str("· 三负载：曲线可视（按需拾取 + LOD 抽稀，端点钉死）、当前值流（定容环形）、权重热力（色/大小/数值三冗余）。\n");
    let _ = s.push_str("· 曲线洪水两级防线：窗内抽稀（端点必进，否则时间跨度是假的）+ 非有限值钳制（NaN 不进绘制缓冲）。\n");
    let _ = s.push_str("· 求值统计三量口径唯一：占比分母 = 帧耗时（F2418 钉死）；分母为零返回「未定义」而非 0。\n");
    let _ = s.push_str("· 调优协议走 F1946 家族：指令 → 生效 ACK，工作单元为实测值；重放不改序号水位线，参数拒则改。\n");
    let _ = s.push_str("· 信封 M 段四类型注册：重算摘要对账，漂移即拦截（消费端一律取不到），不只记一笔。\n");
    let _ = s.push_str("· 发行版剔除零成本：Release 档连强行尝试都不递增构建计数。\n");
    s
}

/// 冒烟：拾一段曲线 + 一段统计。
pub fn smoke() -> String {
    let times = vec![0u32, 100, 200, 300, 400, 500];
    let vals = vec![0.0f32, 1.0, 2.0, 3.0, 4.0, 5.0];
    let track = demo_track(times, vals);
    let mut bag = DiagBag::new();
    let pick = pick_curve(&track, 0, 100, 400, 3, Interp::Linear, &mut bag);
    let mut win = StatWindow::new();
    let tele = Telemetry { cache_hits: 7, cache_recompute: 3, ..Telemetry::default() };
    let _ = win.observe(StatSample { active_tracks: 5, eval_us: 400, frame_us: 1000 }, &tele, &mut bag);
    let sum = win.summary(&mut bag);
    let mut s = String::new();
    let _ = s.push_str(&format!(
        "拾取 交付={}/{} 抽稀={} 诊断={}\n",
        pick.delivered(),
        pick.total_keys,
        pick.decimated,
        bag.len()
    ));
    let _ = s.push_str(&format!(
        "统计 活跃={} 耗时占比={:?} 缓存命中={:?} 样本={}\n",
        sum.active_tracks, sum.eval_share_ppm, sum.cache_hit_ppm, sum.samples
    ));
    s
}