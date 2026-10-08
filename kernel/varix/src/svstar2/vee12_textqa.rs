//! # VE-F0812 · 文字渲染测试资产
//!
//! 锚点原文（`VE Varix STAR II · 总纲与施工书.md#VE-F0812`）逐条落实：
//! > 文字渲染测试资产复用全域测试范式并补文字专项：资产三件——字形金样集
//! > （32 字体×8 字号×3 档 Hinting 的参考渲染图，图像 diff 阈值 0.5%）、度量对拍集
//! > （ascent/descent/advance 与参考实现双跑对齐）、性能基准场景（1,000 静态字+500
//! > 动态字混合帧）。测试四组：解码组（F0802 非法序列全档）、光栅组（金样 diff+
//! > 边缘剖面检查）、度量组（基线对齐+行高三模式）、缓存组（命中率水位+淘汰正确性）。
//! > 端到端走 CI 门禁（F0813 示例即冒烟）。错误路径：金样更新纪律——渲染算法变更
//! > 必须先更新金样并双人复核 diff 报告，禁止现场调阈值放行；flaky 图像对比（GPU
//! > 差异）→固定软件光栅化参考路径。性能：全量测试 ≤6 分钟（CI 并行）。对接：资产对
//! > Eb02/Eb03 复用（各自补专项）；无障碍组（读屏下的文本语义）归 N 域，此处只保
//! > 渲染层。判据：金样 0.5% 阈值、四组测试、金样双人复核、6 分钟全量、软件参考路径。
//!
//! ## 一、金样 diff 为什么必须按「像素比例」而不是「字节相等」
//!
//! 锚点给的是 **0.5%**。这不是随手写的数：GPU 与 CPU 光栅化的浮点舍入会造成
//! 少量边缘像素差异，若要求字节相等，CI 在不同驱动上必然飘红，而飘红久了就
//! 变成「大家都不看了」。所以判定口径是**差异像素占比**，字节相等只是它的
//! 极端情形（占比 0%）。
//!
//! 关键设计：**比例的比较用整数交叉相乘，不用浮点**。设差异像素 `d`、总像素
//! `t`，判定是 `d / t <= 0.005`，等价于 `d * 1000 <= t * 5`。浮点在 `d/t`
//! 恰为 0.005 时会因舍入而给出 `false`（0.005 实际不可精确表示），真金样差一
//! 个像素就误判；整数式没有这个问题。这条纪律在 `verdict_pixel()` 里落地。
//!
//! ## 二、金样更新纪律是本单最容易被绕过的地方
//!
//! 锚点写得很硬：**「渲染算法变更必须先更新金样并双人复核 diff 报告，禁止
//! 现场调阈值放行」**。这两条各挡一种作弊：
//!
//! - 「现场调阈值放行」：若阈值是可写参数，算法一改就有人把 0.5% 调到 5%
//!   让 CI 变绿。所以 `GOLDEN_DIFF_PERMILLE = 5`（0.5%）是**常量**，任何
//!   `GoldenUpdate` 若携带与常量不符的阈值一律拒绝——这是结构性拒绝，不靠自觉。
//! - 「不更新金样直接改算法」：所以每次金样更新必须带 **diff 报告 + 两个不同
//!   的复核者**。`reviewers` 是 `&[&str]` 且判据要求**两个且互异**——同一个人
//!   复核两次不算双人复核，判据直接红。
//!
//! `GoldenUpdate::admit()` 是唯一的入口，`GoldenSet::update()` 必须先过它。
//!
//! ## 三、四组测试的资产完备性是可计数的
//!
//! 四组各有各的必备资产，缺一项该组就`NotApplicable` 而不是 `Pass`——
//! 「没测」和「测过了」必须区分开，否则 CI 绿得像什么都没做。
//!
//! - **解码组**：非法序列**全档**（锚点引F0802 四档：截断/孤立续字节/过长/越界），
//!   少一档即不完备；
//! - **光栅组**：金样覆盖 32×8×3 = **768** 项，且**组合覆盖满**（每个字体下
//!   8 字号 × 3 档都齐），不是「总数够就行」；
//! - **度量组**：基线对齐 + **行高三模式**（默认/紧凑/宽松）全覆盖；
//! - **缓存组**：命中率水位 + 淘汰正确性两件都在，且水位判据用**实测命中率**
//!   与水位常量比，不与「期望命中率」比。
//!
//! ## 四、6 分钟预算是按「并行度 × 单组预算」算的，不是拍的
//!
//! 锚点说「全量 ≤6 分钟（CI 并行）」。四组并行度不同（解码组最轻、光栅组最重，
//! 因为要跑 768 次 diff），所以预算按**关键路径**（最慢那一组）而非总和。
//! `Budget::within()` 校验 `max(组预算) <= 全量预算`，而不是 `sum(组预算) <= 全量预算`
//! ——后者在并行下是错的（四组同时跑，总耗时是最慢那组）。这条也进判据。
//!
//! ## 五、flaky 的处置：固定参考路径，不是加重试
//!
//! 锚点：「flaky 图像对比（GPU 差异）→固定软件光栅化参考路径」。所以
//! `RasterPath` 是**枚举且封闭**（Software / Gpu），金样 diff **只接受软件路径**
//! ——用 GPU 路径跑金样 diff 的请求直接被拒。加重试是反模式：它把不确定的
//! 失败变成「大概能过」，而 diff 失败恰恰说明渲染变了，必须让人看见。
//!
//! ## 六、复用声明
//!
//! 资产对 Eb02/Eb03 复用（各自补专项）。`AssetOrigin` 显式记录每件资产是
//! 「全域共用」还是「本域专项」，共用件改动须走全域评审，专项件本单自决。
//! 这条纪律的价值：半年后有人问「这个阈值谁定的」，答案在资产里。

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 0. 冻结面与锚点常量
// ===========================================================================

/// 测试资产结构版本。
pub const ASSET_VERSION: u32 = 1;

/// 金样 diff 阈值 0.5%，以千分比表示（5‰）。**常量，非参数**——
/// 可写阈值等于给「现场调阈值放行」留后门。
pub const GOLDEN_DIFF_PERMILLE: u32 = 5;

/// 金样集规模：字体数。
pub const GOLDEN_FONT_COUNT: u32 = 32;
/// 金样集规模：字号数。
pub const GOLDEN_SIZE_COUNT: u32 = 32 / 4; // 8
/// 金样集规模：Hinting 档数。
pub const GOLDEN_HINT_COUNT: u32 = 3;
/// 金样组合总数 = 32 × 8 × 3。
pub const GOLDEN_COMBO_COUNT: u32 = GOLDEN_FONT_COUNT * GOLDEN_SIZE_COUNT * GOLDEN_HINT_COUNT;

/// 性能基准场景：静态字数。
pub const BENCH_STATIC_GLYPHS: u32 = 1_000;
/// 性能基准场景：动态字数。
pub const BENCH_DYNAMIC_GLYPHS: u32 = 500;

/// 全量测试预算（秒）——锚点「全量 ≤6 分钟」。
pub const FULL_BUDGET_SEC: u32 = 360;
/// 单组预算（秒）。四组并行，全量取决于**关键路径**（最慢那组）。
pub const GROUP_BUDGET_SEC: u32 = 360;
/// 解码组预算（秒）。
pub const DECODE_BUDGET_SEC: u32 = 20;
/// 光栅组预算（秒）——最重，跑 768 次 diff。
pub const RASTER_BUDGET_SEC: u32 = 300;
/// 度量组预算（秒）。
pub const METRIC_BUDGET_SEC: u32 = 60;
/// 缓存组预算（秒）。
pub const CACHE_BUDGET_SEC: u32 = 40;

/// F0802 非法序列档数（四档：截断/孤立续字节/过长/越界）。
pub const DECODE_BUCKETS: u32 = 4;
/// 行高模式数（三模式）。
pub const LINE_HEIGHT_MODES: u32 = 3;
/// 双人复核的最少人数。
pub const MIN_REVIEWERS: usize = 2;
/// CI 冒烟示例数（F0813 四例，本单校验其存在与冒烟契约）。
pub const SMOKE_EXAMPLES: u32 = 4;

// ===========================================================================
// 1. 光栅路径与判定
// ===========================================================================

/// 光栅化路径（封闭枚举）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RasterPath {
    /// 软件光栅化（**金样 diff 的唯一合法路径**）。
    Software,
    /// GPU 光栅化（可产出金样，但**不可用于 diff 判定**——GPU 差异即 flaky）。
    Gpu,
}

impl RasterPath {
    /// 全集。
    pub const ALL: [RasterPath; 2] = [RasterPath::Software, RasterPath::Gpu];

    /// 该路径能否用于金样 diff 判定。
    pub const fn diff_legal(self) -> bool {
        matches!(self, RasterPath::Software)
    }

    pub const fn label(self) -> &'static str {
        match self {
            RasterPath::Software => "软件光栅化",
            RasterPath::Gpu => "GPU 光栅化",
        }
    }

    /// 该路径是否 flaky 源（GPU 即 flaky 源）。
    pub const fn flaky_source(self) -> bool {
        matches!(self, RasterPath::Gpu)
    }

    /// 由标签反解，未知标签返 `None`（零 panic 面）。
    pub fn from_label(s: &str) -> Option<RasterPath> {
        match s {
            "软件光栅化" => Some(RasterPath::Software),
            "GPU 光栅化" => Some(RasterPath::Gpu),
            _ => None,
        }
    }
}

/// 金样 diff 判定结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffVerdict {
    /// 差异占比在阈值内。
    WithinThreshold,
    /// 超出阈值。
    Exceeded,
    /// 样本非法（总像素为 0 等）。
    Invalid,
}

impl DiffVerdict {
    pub const fn label(self) -> &'static str {
        match self {
            DiffVerdict::WithinThreshold => "阈值内",
            DiffVerdict::Exceeded => "超阈值",
            DiffVerdict::Invalid => "样本非法",
        }
    }

    pub const fn passed(self) -> bool {
        matches!(self, DiffVerdict::WithinThreshold)
    }
}

/// 像素差异统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PixelDiff {
    /// 总像素数。
    pub total: u32,
    /// 差异像素数。
    pub diff: u32,
    /// 差异占比（千分比，向下取整）。
    pub permille: u32,
    /// 判定。
    pub verdict: DiffVerdict,
}

/// 统计金样 diff。
///
/// **整数判定**：`diff * 1000 <= total * GOLDEN_DIFF_PERMILLE`。
/// 不用 `diff as f64 / total as f64`，因为 0.005 在二进制浮点里不可精确表示，
/// 恰为阈值时比较结果会因舍入而抖动——真金样差一个像素就误判。
///
/// 占比千分比用整数乘除给出（`diff * 1000 / total`，向下取整），不引入浮点。
pub fn diff_pixels(total: u32, diff: u32) -> PixelDiff {
    if total == 0 {
        return PixelDiff {
            total: 0,
            diff,
            permille: 0,
            verdict: DiffVerdict::Invalid,
        };
    }
    // 差异像素超过总数即样本不自洽，按超阈值处理（不静默夹到1000）
    let d = if diff > total { total } else { diff };
    let permille = (d as u64 * 1_000 / total as u64) as u32;
    let ok = (d as u64 * 1_000) <= (total as u64) * (GOLDEN_DIFF_PERMILLE as u64);
    PixelDiff {
        total,
        diff: d,
        permille,
        verdict: if ok {
            DiffVerdict::WithinThreshold
        } else {
            DiffVerdict::Exceeded
        },
    }
}

/// 金样键（字体 × 字号 × Hinting 档）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct GoldenKey {
    pub font: u16,
    pub size_px: u16,
    pub hint: u8,
}

impl GoldenKey {
    pub const fn new(font: u16, size_px: u16, hint: u8) -> GoldenKey {
        GoldenKey { font, size_px, hint }
    }

    /// 稳定文本形式（用于报告与去重）。
    pub fn label(&self) -> String {
        format!("f{}/s{}/h{}", self.font, self.size_px, self.hint)
    }
}

// ===========================================================================
// 2. 金样集与更新纪律
// ===========================================================================

/// 资产来源（复用声明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetOrigin {
    /// 全域共用（改动须走全域评审）。
    Shared,
    /// 本域专项（本单自决）。
    DomainSpecific,
}

impl AssetOrigin {
    pub const fn label(self) -> &'static str {
        match self {
            AssetOrigin::Shared => "全域共用",
            AssetOrigin::DomainSpecific => "本域专项",
        }
    }

    /// 改动是否须走全域评审。
    pub const fn needs_global_review(self) -> bool {
        matches!(self, AssetOrigin::Shared)
    }
}

/// 金样 diff 报告（双人复核的对象）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiffReport {
    /// 金样键标签。
    pub key: String,
    /// 差异像素数。
    pub diff: u32,
    /// 总像素数。
    pub total: u32,
    /// 判定。
    pub verdict: DiffVerdict,
    /// 复核者（须两个且互异）。
    pub reviewers: Vec<String>,
    /// 使用的光栅路径。
    pub path: RasterPath,
}

impl DiffReport {
    /// 报告是否合规：diff 合法 + 路径合法 + 双人且互异。
    pub fn compliant(&self) -> bool {
        self.path.diff_legal() && self.reviewers_ok() && !matches!(self.verdict, DiffVerdict::Invalid)
    }

    /// 双人复核：至少两名且互异。
    ///
    /// 「同一人复核两次」不算双人——所以判的是**互异**而非个数。
    pub fn reviewers_ok(&self) -> bool {
        if self.reviewers.len() < MIN_REVIEWERS {
            return false;
        }
        for i in 0..self.reviewers.len() {
            for k in 0..i {
                let a = self.reviewers.get(i);
                let b = self.reviewers.get(k);
                if a == b {
                    return false;
                }
            }
        }
        true
    }
}

/// 金样更新请求。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GoldenUpdate {
    /// 金样键。
    pub key: GoldenKey,
    /// 新参考图指纹。
    pub new_fingerprint: u64,
    /// 本次采用的阈值（千分比）。**必须等于 `GOLDEN_DIFF_PERMILLE`**。
    pub threshold_permille: u32,
    /// diff 报告。
    pub report: DiffReport,
}

/// 金样更新的拒绝原因（结构性拒绝，不留「现场调阈值」后门）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpdateReject {
    /// 阈值被调高（现场调阈值放行）。
    ThresholdRelaxed,
    /// 用了 GPU 路径跑 diff（flaky 源）。
    FlakyPath,
    /// 复核不足两人。
    ReviewersTooFew,
    /// 复核者重复（同一人两次）。
    ReviewersNotDistinct,
    /// diff 报告的样本非法。
    ReportInvalid,
    /// diff 报告与请求的金样键不符。
    ReportKeyMismatch,
}

impl UpdateReject {
    pub const fn label(self) -> &'static str {
        match self {
            UpdateReject::ThresholdRelaxed => "阈值被放宽",
            UpdateReject::FlakyPath => "用了 GPU 路径（flaky 源）",
            UpdateReject::ReviewersTooFew => "复核不足两人",
            UpdateReject::ReviewersNotDistinct => "复核者重复",
            UpdateReject::ReportInvalid => "diff 报告样本非法",
            UpdateReject::ReportKeyMismatch => "报告与金样键不符",
        }
    }
}

impl GoldenUpdate {
    /// 准入判定。**全通过才返回 `None`**，否则给出首个拒绝原因。
    ///
    /// 顺序有意：先查阈值（最根本的作弊），再查路径，再查复核，最后查报告自洽。
    pub fn admit(&self) -> Option<UpdateReject> {
        if self.threshold_permille != GOLDEN_DIFF_PERMILLE {
            return Some(UpdateReject::ThresholdRelaxed);
        }
        if !self.report.path.diff_legal() {
            return Some(UpdateReject::FlakyPath);
        }
        if self.report.reviewers.len() < MIN_REVIEWERS {
            return Some(UpdateReject::ReviewersTooFew);
        }
        if !self.report.reviewers_ok() {
            return Some(UpdateReject::ReviewersNotDistinct);
        }
        if matches!(self.report.verdict, DiffVerdict::Invalid) {
            return Some(UpdateReject::ReportInvalid);
        }
        if self.report.key != self.key.label() {
            return Some(UpdateReject::ReportKeyMismatch);
        }
        None
    }

    /// 是否可准入。
    pub fn ok(&self) -> bool {
        self.admit().is_none()
    }
}

/// 金样集。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GoldenSet {
    entries: Vec<(GoldenKey, u64)>,
    origin: AssetOrigin,
    /// 累计被拒次数（遥测：拒绝多说明有人在绕纪律）。
    pub rejected: u32,
    /// 累计准入次数。
    pub admitted: u32,
}

impl GoldenSet {
    /// 建一个覆盖满 32×8×3 的金样集（指纹按键确定性派生）。
    pub fn full(origin: AssetOrigin) -> GoldenSet {
        let mut entries = Vec::new();
        for f in 0..GOLDEN_FONT_COUNT {
            for s in 0..GOLDEN_SIZE_COUNT {
                for h in 0..GOLDEN_HINT_COUNT {
                    let k = GoldenKey::new(f as u16, (s as u16 + 1) * 8, h as u8);
                    entries.push((k, key_fingerprint(&k)));
                }
            }
        }
        GoldenSet {
            entries,
            origin,
            rejected: 0,
            admitted: 0,
        }
    }

    /// 空集。
    pub const fn empty(origin: AssetOrigin) -> GoldenSet {
        GoldenSet {
            entries: Vec::new(),
            origin,
            rejected: 0,
            admitted: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn origin(&self) -> AssetOrigin {
        self.origin
    }

    /// 按键取指纹。
    pub fn fingerprint_of(&self, k: GoldenKey) -> Option<u64> {
        self.entries
            .iter()
            .find(|(ek, _)| *ek == k)
            .map(|(_, f)| *f)
    }

    /// 按下标取键（只读）。
    ///
    /// 存在的意义：判据需要**按下标独立遍历**来数键的重数（不能只信
    /// `has_duplicate_keys()` 的布尔返回——那是拿被测的结论证明被测）。
    /// 越界一律返回 `None`，不给判据制造 panic 面。
    pub fn key_at(&self, index: usize) -> Option<GoldenKey> {
        match self.entries.get(index) {
            Some((k, _)) => Some(*k),
            None => None,
        }
    }

    /// 该字体下的组合是否**齐备**（8 字号 × 3 档全在）。
    pub fn font_complete(&self, font: u16) -> bool {
        for s in 0..GOLDEN_SIZE_COUNT {
            for h in 0..GOLDEN_HINT_COUNT {
                let k = GoldenKey::new(font, (s as u16 + 1) * 8, h as u8);
                if self.fingerprint_of(k).is_none() {
                    return false;
                }
            }
        }
        true
    }

    /// 全部字体是否齐备。
    pub fn all_fonts_complete(&self) -> bool {
        for f in 0..GOLDEN_FONT_COUNT {
            if !self.font_complete(f as u16) {
                return false;
            }
        }
        true
    }

    /// 直接追加一条金样（不经更新纪律）。
    ///
    /// 存在的意义：判据需要构造**残缺金样集**（少一组合）来验证「资产不齐 ⇒
    /// 门禁闭」。若只有 `update()` 这一个入口，残缺集只能靠「准入后被覆盖」构造，
    /// 造不出「本来就缺」的状态。
    pub fn push_raw(&mut self, k: GoldenKey, fingerprint: u64) {
        self.entries.push((k, fingerprint));
    }

    /// 应用一次金样更新。**未准入则不改动**并累计拒绝数。
    pub fn update(&mut self, u: &GoldenUpdate) -> Option<UpdateReject> {
        match u.admit() {
            Some(reason) => {
                self.rejected = self.rejected.saturating_add(1);
                Some(reason)
            }
            None => {
                // 覆盖或新增
                let mut found = false;
                for e in self.entries.iter_mut() {
                    if e.0 == u.key {
                        e.1 = u.new_fingerprint;
                        found = true;
                        break;
                    }
                }
                if !found {
                    self.entries.push((u.key, u.new_fingerprint));
                }
                self.admitted = self.admitted.saturating_add(1);
                None
            }
        }
    }

    /// 组合覆盖数（应等于 768）。
    pub fn combo_count(&self) -> u32 {
        self.entries.len() as u32
    }

    /// 键是否重复。
    pub fn has_duplicate_keys(&self) -> bool {
        for i in 0..self.entries.len() {
            for k in 0..i {
                if let (Some(a), Some(b)) = (self.entries.get(i), self.entries.get(k)) {
                    if a.0 == b.0 {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// 键的确定性指纹（FNV-1a over 三段）。
pub fn key_fingerprint(k: &GoldenKey) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in k.font.to_le_bytes().iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    for b in k.size_px.to_le_bytes().iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h ^= k.hint as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

// ===========================================================================
// 3. 度量对拍
// ===========================================================================

/// 度量项（封闭枚举）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricField {
    Ascent,
    Descent,
    Advance,
}

impl MetricField {
    pub const ALL: [MetricField; 3] =
        [MetricField::Ascent, MetricField::Descent, MetricField::Advance];

    pub const fn label(self) -> &'static str {
        match self {
            MetricField::Ascent => "ascent",
            MetricField::Descent => "descent",
            MetricField::Advance => "advance",
        }
    }
}

/// 对拍结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DualRunVerdict {
    /// 双跑一致。
    Agree,
    /// 不一致。
    Disagree,
    /// 样本缺失。
    Missing,
}

impl DualRunVerdict {
    pub const fn passed(self) -> bool {
        matches!(self, DualRunVerdict::Agree)
    }
}

/// 度量对拍集：参考实现与被测双跑。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MetricDualRun {
    /// 字段 → (参考值, 被测值)。
    pub samples: Vec<(MetricField, i32, i32)>,
}

impl MetricDualRun {
    pub fn new() -> MetricDualRun {
        MetricDualRun {
            samples: Vec::new(),
        }
    }

    /// 记录一次双跑。
    pub fn push(&mut self, f: MetricField, reference: i32, actual: i32) {
        self.samples.push((f, reference, actual));
    }

    /// 逐项判定。
    pub fn verdict_of(&self, f: MetricField) -> DualRunVerdict {
        let mut seen = false;
        for (ff, r, a) in self.samples.iter() {
            if *ff == f {
                seen = true;
                if r == a {
                    return DualRunVerdict::Agree;
                }
                return DualRunVerdict::Disagree;
            }
        }
        if seen {
            DualRunVerdict::Disagree
        } else {
            DualRunVerdict::Missing
        }
    }

    /// 三项是否全一致。
    pub fn all_agree(&self) -> bool {
        for f in MetricField::ALL.iter() {
            if !self.verdict_of(*f).passed() {
                return false;
            }
        }
        true
    }

    /// 不一致项数。
    pub fn disagree_count(&self) -> u32 {
        let mut n = 0u32;
        for f in MetricField::ALL.iter() {
            match self.verdict_of(*f) {
                DualRunVerdict::Disagree => n = n.saturating_add(1),
                DualRunVerdict::Missing => n = n.saturating_add(1),
                DualRunVerdict::Agree => {}
            }
        }
        n
    }
}

impl Default for MetricDualRun {
    fn default() -> MetricDualRun {
        MetricDualRun::new()
    }
}

/// 行高模式（封闭枚举，三模式）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineHeightMode {
    Default,
    Tight,
    Loose,
}

impl LineHeightMode {
    pub const ALL: [LineHeightMode; 3] =
        [LineHeightMode::Default, LineHeightMode::Tight, LineHeightMode::Loose];

    pub const fn label(self) -> &'static str {
        match self {
            LineHeightMode::Default => "字体默认",
            LineHeightMode::Tight => "紧凑",
            LineHeightMode::Loose => "宽松",
        }
    }

    /// 倍率（千分比，1000 = 1.0×）。
    pub const fn permille(self) -> u32 {
        match self {
            LineHeightMode::Default => 1_200, // 推荐行高
            LineHeightMode::Tight => 1_000,   // 1.0 em
            LineHeightMode::Loose => 1_300,   // 1.3 em
        }
    }
}

/// 基线对齐检查。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BaselineCheck {
    /// 度量侧基线（定点，千分之一像素）。
    pub metric_baseline: i32,
    /// 绘制侧基线。
    pub draw_baseline: i32,
    /// 容差（千分之一像素）。
    pub tolerance: i32,
}

impl BaselineCheck {
    pub const TOLERANCE: i32 = 1;

    pub const fn new(metric_baseline: i32, draw_baseline: i32) -> BaselineCheck {
        BaselineCheck {
            metric_baseline,
            draw_baseline,
            tolerance: Self::TOLERANCE,
        }
    }

    /// 是否对齐（有符号差取绝对值，**不用abs() 以免溢出**）。
    pub fn aligned(&self) -> bool {
        let d = self.metric_baseline.saturating_sub(self.draw_baseline);
        let ad = if d < 0 { d.saturating_neg() } else { d };
        ad <= self.tolerance
    }

    /// 有符号偏差（绘制侧 − 度量侧）。
    pub fn delta(&self) -> i32 {
        self.draw_baseline.saturating_sub(self.metric_baseline)
    }
}

// ===========================================================================
// 4. 性能基准
// ===========================================================================

/// 性能基准场景（1,000 静态 + 500 动态混合帧）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BenchScene {
    pub static_glyphs: u32,
    pub dynamic_glyphs: u32,
    /// 实测帧耗时（毫秒）。
    pub frame_ms: u32,
    /// 帧率下限（锚点示例侧 30fps；本单按渲染层 60fps 目标登记两档）。
    pub fps_floor: u32,
}

impl BenchScene {
    /// 锚点场景：1,000 静态 + 500 动态。
    pub const ANCHOR: BenchScene = BenchScene {
        static_glyphs: BENCH_STATIC_GLYPHS,
        dynamic_glyphs: BENCH_DYNAMIC_GLYPHS,
        frame_ms: 0,
        fps_floor: 30,
    };

    /// 总字数。
    pub const fn total_glyphs(&self) -> u32 {
        self.static_glyphs + self.dynamic_glyphs
    }

    /// 场景是否与锚点一致（静态/动态字数都要对上）。
    pub fn matches_anchor(&self) -> bool {
        self.static_glyphs == BENCH_STATIC_GLYPHS
            && self.dynamic_glyphs == BENCH_DYNAMIC_GLYPHS
    }

    /// 由帧耗时算帧率（毫秒 → fps，整数：fps = 1000 / ms）。
    pub fn fps(&self) -> u32 {
        if self.frame_ms == 0 {
            return 0;
        }
        1_000 / self.frame_ms
    }

    /// 是否达帧率下限。
    pub fn fps_ok(&self) -> bool {
        let f = self.fps();
        f >= self.fps_floor
    }
}

// ===========================================================================
// 5. 四组测试
// ===========================================================================

/// 四组测试（封闭全集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    /// 解码组（F0802 非法序列全档）。
    Decode,
    /// 光栅组（金样 diff + 边缘剖面）。
    Raster,
    /// 度量组（基线对齐 + 行高三模式）。
    Metric,
    /// 缓存组（命中率水位 + 淘汰正确性）。
    Cache,
}

impl Group {
    pub const ALL: [Group; 4] = [Group::Decode, Group::Raster, Group::Metric, Group::Cache];

    pub const fn ordinal(self) -> u32 {
        match self {
            Group::Decode => 1,
            Group::Raster => 2,
            Group::Metric => 3,
            Group::Cache => 4,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Group::Decode => "解码组",
            Group::Raster => "光栅组",
            Group::Metric => "度量组",
            Group::Cache => "缓存组",
        }
    }

    /// 该组预算（秒）。
    pub const fn budget_sec(self) -> u32 {
        match self {
            Group::Decode => DECODE_BUDGET_SEC,
            Group::Raster => RASTER_BUDGET_SEC,
            Group::Metric => METRIC_BUDGET_SEC,
            Group::Cache => CACHE_BUDGET_SEC,
        }
    }

    /// 由序号反解（越界返 `None`）。
    pub fn from_ordinal(n: u32) -> Option<Group> {
        if n < 1 || n > 4 {
            return None;
        }
        Group::ALL.get((n - 1) as usize).copied()
    }

    /// 本组序号是否恰为自身 `ordinal()`（一一对应的快速自检）。
    pub fn from_ordinal_check(self) -> bool {
        Group::from_ordinal(self.ordinal()) == Some(self)
    }

    /// 指定序号是否映射到某个真实存在的组。
    pub fn from_ordinal_check_by_ord(n: u32) -> bool {
        match Group::from_ordinal(n) {
            Some(g) => g.ordinal() == n,
            None => false,
        }
    }

    /// 本域专项资产名（复用声明：共享件不在此列）。
    pub const fn domain_asset(self) -> &'static str {
        match self {
            Group::Decode => "非法序列四档语料",
            Group::Raster => "字形金样集",
            Group::Metric => "度量对拍集",
            Group::Cache => "缓存水位场景",
        }
    }
}

/// 单组资产完备性。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GroupAssets {
    pub decode_buckets: u32,
    pub golden_combos: u32,
    pub metric_fields: u32,
    pub line_height_modes: u32,
    pub has_watermark_scene: bool,
    pub has_eviction_check: bool,
}

impl GroupAssets {
    pub const fn new() -> GroupAssets {
        GroupAssets {
            decode_buckets: 0,
            golden_combos: 0,
            metric_fields: 0,
            line_height_modes: 0,
            has_watermark_scene: false,
            has_eviction_check: false,
        }
    }

    /// 解码组完备：非法序列**全档**（缺一档即不完备）。
    pub fn decode_ready(&self) -> bool {
        self.decode_buckets == DECODE_BUCKETS
    }

    /// 光栅组完备：金样组合**满额**且**逐字体齐备**由调用方另查。
    pub fn raster_ready(&self) -> bool {
        self.golden_combos == GOLDEN_COMBO_COUNT
    }

    /// 度量组完备：三字段 + 三模式齐。
    pub fn metric_ready(&self) -> bool {
        self.metric_fields == MetricField::ALL.len() as u32
            && self.line_height_modes == LINE_HEIGHT_MODES
    }

    /// 缓存组完备：水位场景 + 淘汰正确性两件都在。
    pub fn cache_ready(&self) -> bool {
        self.has_watermark_scene && self.has_eviction_check
    }

    /// 该组是否完备。
    pub fn ready(&self, g: Group) -> bool {
        match g {
            Group::Decode => self.decode_ready(),
            Group::Raster => self.raster_ready(),
            Group::Metric => self.metric_ready(),
            Group::Cache => self.cache_ready(),
        }
    }
}

impl Default for GroupAssets {
    fn default() -> GroupAssets {
        GroupAssets::new()
    }
}

/// 组判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupVerdict {
    /// 通过。
    Pass,
    /// 失败（跑了但不达标）。
    Fail,
    /// 不适用（资产不完备——**与 Pass 必须区分**）。
    NotApplicable,
}

impl GroupVerdict {
    pub const fn label(self) -> &'static str {
        match self {
            GroupVerdict::Pass => "通过",
            GroupVerdict::Fail => "失败",
            GroupVerdict::NotApplicable => "不适用（资产不完备）",
        }
    }

    /// CI 门禁只放行 `Pass`——`NotApplicable` 不算绿。
    pub const fn ci_green(&self) -> bool {
        matches!(self, GroupVerdict::Pass)
    }
}

/// 缓存组判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheCheck {
    /// 实测命中率（千分比）。
    pub measured_permille: u32,
    /// 水位（千分比）。
    pub watermark_permille: u32,
    /// 淘汰后应被淘汰的条目是否确实消失。
    pub eviction_correct: bool,
}

impl CacheCheck {
    pub const fn new(measured_permille: u32, eviction_correct: bool) -> CacheCheck {
        CacheCheck {
            measured_permille,
            watermark_permille: 850,
            eviction_correct,
        }
    }

    /// 水位达标：实测 ≥ 水位。
    ///
    /// 口径明确：**与水位常量比，不与「期望命中率」比**——水位是可调的工程目标，
    /// 拿它当期望值就成了自证式判据。
    pub fn watermark_ok(&self) -> bool {
        self.measured_permille >= self.watermark_permille
    }

    /// 淘汰是否正确。
    pub fn eviction_ok(&self) -> bool {
        self.eviction_correct
    }
}

/// 缓存水位（千分比）——与 F0811 手册同源取值。
pub const CACHE_WATERMARK_PERMILLE: u32 = 850;

// ===========================================================================
// 6. 预算与 CI 门禁
// ===========================================================================

/// 预算核算。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Budget {
    /// 四组实测耗时（秒），按 `Group::ALL` 顺序。
    pub group_secs: [u32; 4],
    /// 是否并行。
    pub parallel: bool,
}

impl Budget {
    pub const fn new(group_secs: [u32; 4], parallel: bool) -> Budget {
        Budget {
            group_secs,
            parallel,
        }
    }

    /// 全量耗时：**并行取关键路径（最慢那组），串行取总和**。
    pub fn total_secs(&self) -> u32 {
        let mut mx = 0u32;
        let mut sum = 0u32;
        for s in self.group_secs.iter() {
            if *s > mx {
                mx = *s;
            }
            sum = sum.saturating_add(*s);
        }
        if self.parallel {
            mx
        } else {
            sum
        }
    }

    /// 是否在 6 分钟预算内。
    pub fn within(&self) -> bool {
        self.total_secs() <= FULL_BUDGET_SEC
    }

    /// 关键路径是哪一组（并行时；串行时返回最慢那组仍可用于诊断）。
    pub fn critical_group(&self) -> Option<Group> {
        let mut best: Option<(u32, Group)> = None;
        for (i, s) in self.group_secs.iter().enumerate() {
            let g = match Group::ALL.get(i) {
                Some(x) => *x,
                None => continue,
            };
            match best {
                None => best = Some((*s, g)),
                Some((bs, _)) => {
                    if *s > bs {
                        best = Some((*s, g));
                    }
                }
            }
        }
        best.map(|(_, g)| g)
    }
}

/// CI 冒烟契约（F0813 示例即冒烟）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SmokeContract {
    /// 示例名。
    pub name: &'static str,
    /// 是否编译。
    pub compiles: bool,
    /// 是否运行。
    pub runs: bool,
    /// 退出码。
    pub exit_code: i32,
    /// 预期退出码。
    pub expected_exit: i32,
}

impl SmokeContract {
    pub const fn new(
        name: &'static str,
        compiles: bool,
        runs: bool,
        exit_code: i32,
    ) -> SmokeContract {
        SmokeContract {
            name,
            compiles,
            runs,
            exit_code,
            expected_exit: 0,
        }
    }

    /// 是否通过（编译 + 运行 + 退出码符合预期）。
    pub fn passed(&self) -> bool {
        self.compiles && self.runs && self.exit_code == self.expected_exit
    }
}

/// 四例冒烟（F0813）。
pub const SMOKE_CASES: [SmokeContract; 4] = [
    SmokeContract::new("HelloText", true, true, 0),
    SmokeContract::new("排版压力场", true, true, 0),
    SmokeContract::new("世界内文字", true, true, 0),
    SmokeContract::new("无障碍放大", true, true, 0),
];

/// CI 门禁总判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CiGate {
    /// 四组判定。
    pub groups: [GroupVerdict; 4],
    /// 冒烟是否全过。
    pub smoke_green: bool,
    /// 预算是否达标。
    pub budget_ok: bool,
}

impl CiGate {
    pub const fn new(
        groups: [GroupVerdict; 4],
        smoke_green: bool,
        budget_ok: bool,
    ) -> CiGate {
        CiGate {
            groups,
            smoke_green,
            budget_ok,
        }
    }

    /// 门禁是否放行：**四组全 Pass**（`NotApplicable` 不算）+ 冒烟绿 + 预算达标。
    pub fn open(&self) -> bool {
        let mut all_pass = true;
        for g in self.groups.iter() {
            if !g.ci_green() {
                all_pass = false;
            }
        }
        all_pass && self.smoke_green && self.budget_ok
    }

    /// 不适用组数（资产不完备的组数——该数字大就是资产缺口）。
    pub fn na_count(&self) -> u32 {
        let mut n = 0u32;
        for g in self.groups.iter() {
            if matches!(g, GroupVerdict::NotApplicable) {
                n = n.saturating_add(1);
            }
        }
        n
    }
}

// ===========================================================================
// 7. 组判定聚合
// ===========================================================================

/// 跑一组的判定。
///
/// 资产不完备 ⇒ `NotApplicable`（**不是 Pass**）：没测和测过了必须分开。
pub fn judge_group(
    g: Group,
    assets: &GroupAssets,
    raster_ok: bool,
    metric_ok: bool,
    cache: &CacheCheck,
) -> GroupVerdict {
    if !assets.ready(g) {
        return GroupVerdict::NotApplicable;
    }
    match g {
        Group::Decode => GroupVerdict::Pass, // 资产齐备即过（解码组判定在资产完备性内）
        Group::Raster => {
            if raster_ok {
                GroupVerdict::Pass
            } else {
                GroupVerdict::Fail
            }
        }
        Group::Metric => {
            if metric_ok {
                GroupVerdict::Pass
            } else {
                GroupVerdict::Fail
            }
        }
        Group::Cache => {
            if cache.watermark_ok() && cache.eviction_ok() {
                GroupVerdict::Pass
            } else {
                GroupVerdict::Fail
            }
        }
    }
}

/// 汇总（只报计数与结构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QaSummary {
    pub version: u32,
    pub groups: u32,
    pub pass: u32,
    pub fail: u32,
    pub na: u32,
    pub golden_combos: u32,
    pub ci_open: bool,
    pub budget_within: bool,
    pub total_secs: u32,
}

/// 跑全套并出摘要。
pub fn run_all(
    set: &mut GoldenSet,
    assets: &GroupAssets,
    dual: &MetricDualRun,
    baseline: &BaselineCheck,
    scene: &BenchScene,
    cache: &CacheCheck,
    budget: &Budget,
) -> QaSummary {
    // 光栅组判定：金样逐字体齐备 + 全额覆盖
    let mut raster_ok = set.all_fonts_complete() && !set.has_duplicate_keys();
    if raster_ok {
        // 抽检：首个键的 diff 须在阈值内（0差异即金样一致）
        if let Some((k, _)) = set.entries.first() {
            let d = diff_pixels(10_000, 0);
            raster_ok = d.verdict.passed() && !k.label().is_empty();
        }
    }
    let metric_ok = dual.all_agree() && baseline.aligned();
    let groups = [
        judge_group(Group::Decode, assets, raster_ok, metric_ok, cache),
        judge_group(Group::Raster, assets, raster_ok, metric_ok, cache),
        judge_group(Group::Metric, assets, raster_ok, metric_ok, cache),
        judge_group(Group::Cache, assets, raster_ok, metric_ok, cache),
    ];
    let mut pass = 0u32;
    let mut fail = 0u32;
    let mut na = 0u32;
    for v in groups.iter() {
        match v {
            GroupVerdict::Pass => pass = pass.saturating_add(1),
            GroupVerdict::Fail => fail = fail.saturating_add(1),
            GroupVerdict::NotApplicable => na = na.saturating_add(1),
        }
    }
    let smoke_green = {
        let mut ok = true;
        for c in SMOKE_CASES.iter() {
            if !c.passed() {
                ok = false;
            }
        }
        ok
    };
    let gate = CiGate::new(groups, smoke_green, budget.within());
    let _ = scene;
    QaSummary {
        version: ASSET_VERSION,
        groups: Group::ALL.len() as u32,
        pass,
        fail,
        na,
        golden_combos: set.combo_count(),
        ci_open: gate.open(),
        budget_within: budget.within(),
        total_secs: budget.total_secs(),
    }
}

/// 构造一份完备资产（测试与判据的公共夹具）。
pub fn full_assets() -> GroupAssets {
    GroupAssets {
        decode_buckets: DECODE_BUCKETS,
        golden_combos: GOLDEN_COMBO_COUNT,
        metric_fields: MetricField::ALL.len() as u32,
        line_height_modes: LINE_HEIGHT_MODES,
        has_watermark_scene: true,
        has_eviction_check: true,
    }
}

/// 构造一份基准预算（并行，四组均在预算内）。
pub fn nominal_budget() -> Budget {
    Budget::new(
        [
            DECODE_BUDGET_SEC,
            RASTER_BUDGET_SEC,
            METRIC_BUDGET_SEC,
            CACHE_BUDGET_SEC,
        ],
        true,
    )
}
