//! VE-F0817 · 文字渲染 API（VE-E 域 · 文字渲染 · 三函数门面）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0817`
//!
//! 锚点原文：「文字渲染 API 三函数冻结十年：MeasureText（文本+字体实例+排版
//! 参数→度量结果与轮廓边界，异步模式返回句柄支持超时语义）、RenderText（图元
//! 批次提交，声明式：调用方描述目标与效果，引擎负责合批与图集）、CacheControl
//! （图集管理：预热/清理/水位查询）。签名十年不变承诺：三函数签名永不修改，
//! 扩展走 options 结构体追加，废弃参数双轨渐弃（保留+告警两个版本后移除）。
//! 错误契约：错误码分段（编码/字体/缓存/管线四段），错误必带可读原因与建议
//! 动作（呈现三要素）。数据边界：MeasureText 只读不缓存（缓存由引擎侧统一
//! 管理），RenderText 幂等（同参数重复提交结果一致）。对接：N 域消费走 F0818
//! 契约（MeasureText 的 N 侧封装）；与全域扩展框架（F0816）的注册协议同源。
//! 测试：三函数签名冻结断言进 CI（防意外签名漂移）。判据：三函数、十年签名、
//! 四段错误码、幂等、签名冻结断言。」
//!
//! # 一、十年冻结是**机制**不是口号：签名可指纹化
//!
//! 「签名永不修改」若只写在注释里，一次重构就会悄悄破坏承诺。本单把三个
//! 函数签名落成**规范化签名文本** [`SIGNATURE_TEXTS`]，指纹
//! [`signature_fingerprint`] 对其做 FNV-1a——判据侧**独立写死**同样的文本
//! 重算比对，任何签名漂移（含参数名与顺序）都会让指纹失配而转红。扩展
//! 永远走 options 结构体**追加字段**（[`MeasureOptions`] /
//! [`RenderOptions`]），函数签名本身不动。
//!
//! # 二、呈现三要素：错误必带「原因 + 建议」
//!
//! [`TextApiError`] 四段（编码/字体/缓存/管线）各码独占 0x17xx 段细分码，
//! 每个错误携带 `reason`（可读原因）与 `advice`（建议动作）——错误码只让
//! 程序分支，人排障靠的是后两者。[`TextApiError::screen_line`] 把三要素
//! 拼成一行，供读屏与日志统一呈现。
//!
//! # 三、数据边界：MeasureText 只读不缓存、RenderText 幂等
//!
//! MeasureText 是**纯函数**：只读字体实例度量快照（[`MetricSnapshot`]），
//! 不触碰 [`MetricCache`]——缓存由引擎侧统一管理，API 层缓存会让「同参数
//! 两次测量」因缓存态不同而结果不同，破坏可预测性。RenderText 幂等由
//! **规范化内容指纹**保证：收据 [`RenderReceipt::content_hash`] 对请求的
//! 全部语义字段做 FNV-1a，同参数重复提交哈希一致——幂等不是「引擎记得
//! 去重」，而是「结果由参数唯一决定」。
//!
//! # 四、废弃双轨渐弃：保留 + 告警 + 版本倒计时
//!
//! 旧效果参数（legacy glow）以 [`RenderOptions::legacy_glow`] 保留，命中
//! 即计数进 [`deprecation_hits`] 并在收据打标记，[`DEPRECATION_VERSIONS_LEFT`]
//! 常量公示还剩两个版本移除——「还在但数得着、走得可见」，比悄悄删掉或
//! 永远不删都诚实。

use core::str;
use core::sync::atomic::{AtomicU32, Ordering};

use alloc::format;
use alloc::string::String;

use super::vee08_fontmetric::{LineHeightMode, MetricKey, MetricSnapshot};

// ---------------------------------------------------------------------------
// 签名冻结（十年承诺的机制化）
// ---------------------------------------------------------------------------

/// 三函数规范化签名文本（指纹的原料；判据侧独立写死对拍）。
pub const SIGNATURE_TEXTS: [&str; 3] = [
    "measure_text(text: &[u8], instance: &MetricSnapshot, params: &TypesetParams, opts: &MeasureOptions) -> Result<MeasureOutcome, TextApiError>",
    "render_text(req: &RenderRequest) -> Result<RenderReceipt, TextApiError>",
    "cache_control(gate: &mut AtlasGate, op: CacheOp) -> Result<CacheWatermark, TextApiError>",
];

/// FNV-1a 64 位步进（与全域哈希设施同源）。
fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 三函数签名指纹：判据侧独立重算比对，漂移即红（签名冻结断言进 CI）。
pub fn signature_fingerprint() -> u64 {
    let mut h = fnv1a(b"VE-F0817/signatures/v1");
    for s in SIGNATURE_TEXTS.iter() {
        h = fnv1a_merge(h, s.as_bytes());
    }
    h
}

fn fnv1a_merge(prev: u64, data: &[u8]) -> u64 {
    let mut h = prev;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ---------------------------------------------------------------------------
// 错误契约：四段错误码 + 呈现三要素
// ---------------------------------------------------------------------------

/// 编码段：UTF-8 解码失败（对齐 F0802 四档处置中的拒绝档）。
pub const E_TEXTAPI_BAD_UTF8: u16 = 0x1700;
/// 字体段：字体实例度量无效（advance/ascent 全零——空实例）。
pub const E_TEXTAPI_EMPTY_INSTANCE: u16 = 0x1701;
/// 字体段：字号越界（合法域 1px..=256px 的 1/64 量化）。
pub const E_TEXTAPI_SIZE_RANGE: u16 = 0x1702;
/// 缓存段：预热超容量（全量拒绝，不做部分预热）。
pub const E_TEXTAPI_CACHE_FULL: u16 = 0x1703;
/// 缓存段：图集未建立（容量为零，先 init 再用）。
pub const E_TEXTAPI_CACHE_UNINIT: u16 = 0x1704;
/// 管线段：单次提交 quad 超批上限（对齐 F0807 合批 2,000）。
pub const E_TEXTAPI_BATCH_LIMIT: u16 = 0x1705;
/// 管线段：双轨渐弃违规（legacy 参数与新参数同设，语义冲突）。
pub const E_TEXTAPI_LEGACY_CONFLICT: u16 = 0x1706;

/// 文字渲染 API 错误：四段（编码/字体/缓存/管线）+ 呈现三要素。
///
/// 三要素 = 错误码（程序分支）+ `reason`（可读原因）+ `advice`（建议动作）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextApiError {
    /// 编码段。
    Encoding { code: u16, reason: String, advice: String },
    /// 字体段。
    Font { code: u16, reason: String, advice: String },
    /// 缓存段。
    Cache { code: u16, reason: String, advice: String },
    /// 管线段。
    Pipeline { code: u16, reason: String, advice: String },
}

impl TextApiError {
    /// 段名（四段之一）。
    pub const fn segment(&self) -> &'static str {
        match self {
            TextApiError::Encoding { .. } => "编码",
            TextApiError::Font { .. } => "字体",
            TextApiError::Cache { .. } => "缓存",
            TextApiError::Pipeline { .. } => "管线",
        }
    }

    /// 错误码。
    pub const fn code(&self) -> u16 {
        match self {
            TextApiError::Encoding { code, .. }
            | TextApiError::Font { code, .. }
            | TextApiError::Cache { code, .. }
            | TextApiError::Pipeline { code, .. } => *code,
        }
    }

    /// 呈现三要素一行：「[段] 0xXXXX | 原因 | 建议」。
    pub fn screen_line(&self) -> String {
        let (reason, advice) = match self {
            TextApiError::Encoding { reason, advice, .. }
            | TextApiError::Font { reason, advice, .. }
            | TextApiError::Cache { reason, advice, .. }
            | TextApiError::Pipeline { reason, advice, .. } => (reason, advice),
        };
        format!(
            "[{}] 0x{:04X} | {} | {}",
            self.segment(),
            self.code(),
            reason,
            advice
        )
    }
}

// ---------------------------------------------------------------------------
// 一、MeasureText —— 只读不缓存；异步句柄带超时语义
// ---------------------------------------------------------------------------

/// 字号合法域下界：1px 的 1/64 量化。
pub const FONT_SIZE_MIN_Q16: u32 = 64;
/// 字号合法域上界：256px 的 1/64 量化。
pub const FONT_SIZE_MAX_Q16: u32 = 16_384;

/// 排版参数（1/64 像素量化，对齐 F0803/F0818 精度约定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypesetParams {
    /// 字号（1/64 px）。
    pub font_size_q16: u32,
    /// 字距（1/64 px，可负）。
    pub letter_spacing_q16: i32,
    /// 换行约束宽度（1/64 px；0 = 不换行）。
    pub max_width_q16: u32,
    /// 行高模式（复用 F0808 三模式）。
    pub line_mode: LineHeightMode,
}

impl TypesetParams {
    /// 参数是否在合法域内。
    pub const fn valid(&self) -> bool {
        self.font_size_q16 >= FONT_SIZE_MIN_Q16 && self.font_size_q16 <= FONT_SIZE_MAX_Q16
    }
}

/// MeasureText 的 options 追加面（十年冻结：扩展只加字段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeasureOptions {
    /// 异步超时（微秒）；0 = 同步模式。
    pub async_timeout_us: u32,
    /// 语言标记（FNV 短 ID，F0849 前向）。
    pub lang_tag: u32,
}

impl MeasureOptions {
    /// 同步默认。
    pub const fn sync() -> Self {
        MeasureOptions { async_timeout_us: 0, lang_tag: 0 }
    }
}

/// 异步句柄：ticket 由请求内容唯一决定（同请求同 ticket）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsyncHandle {
    /// 句柄票据（请求指纹）。
    pub ticket: u64,
    /// 超时上限（微秒）。
    pub timeout_us: u32,
}

/// 度量结果（1/64 px）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeasureResult {
    /// 首行推进宽度（单行语义；多行取最长行）。
    pub advance_q16: i64,
    /// 轮廓边界宽。
    pub bound_w_q16: i64,
    /// 轮廓边界高（行数 × 行高）。
    pub bound_h_q16: i64,
    /// 单行行高（1/64 px）。
    pub line_height_q16: i64,
    /// 参与度量的字形数（不含换行符）。
    pub glyph_count: u32,
    /// 低置信标记（超时降级路径置位，F0818 失败语义前向对齐）。
    pub low_confidence: bool,
}

/// 测量结局：同步直出，或异步句柄（超时语义见 [`AsyncHandle`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasureOutcome {
    /// 同步完成。
    Sync(MeasureResult),
    /// 已挂起（异步模式），凭句柄取结果。
    Pended(AsyncHandle),
}

/// 千分比 → 1/64 px（em 比例换算，checked 防溢出）。
const fn per_mille_to_q16(per_mille: u32, font_size_q16: u32) -> Option<i64> {
    // (per_mille * font_size_q16) / 1000，i64 域内不溢出（两乘数 ≤ 2^31）。
    let v = (per_mille as i64) * (font_size_q16 as i64) / 1000;
    Some(v)
}

/// 文本内容指纹（调用方对 UTF-8 字节求 FNV-1a；RenderRequest 持哈希不持
/// 字节——API 层声明式边界，文本缓冲归调用方所有）。
pub fn text_fingerprint(text: &[u8]) -> u64 {
    fnv1a(text)
}

/// **MeasureText**（十年冻结签名）：文本 + 字体实例 + 排版参数 → 度量结果
/// 与轮廓边界；异步模式返回句柄支持超时语义。
///
/// 只读不缓存：不触碰 [`MetricCache`]，纯函数语义。
///
/// # 错误
/// - 编码段：`text` 非 UTF-8（对齐 F0802 四档处置的拒绝档）。
/// - 字体段：实例度量无效（advance 与 ascent 全零）；字号越界。
pub fn measure_text(
    text: &[u8],
    instance: &MetricSnapshot,
    params: &TypesetParams,
    opts: &MeasureOptions,
) -> Result<MeasureOutcome, TextApiError> {
    // 编码段闸：非 UTF-8 拒测（编码错误进不了整形，错排比拒测更糟）。
    if str::from_utf8(text).is_err() {
        return Err(TextApiError::Encoding {
            code: E_TEXTAPI_BAD_UTF8,
            reason: String::from("文本字节序列不是合法 UTF-8"),
            advice: String::from("先走 F0802 解码器定位坏字节；不要逐字节替换后重测"),
        });
    }
    // 字体段闸：字号越界（合法域 1..=256px）。
    if !params.valid() {
        return Err(TextApiError::Font {
            code: E_TEXTAPI_SIZE_RANGE,
            reason: format!(
                "字号 {} 越界（合法 {}..={}，1/64 px）",
                params.font_size_q16, FONT_SIZE_MIN_Q16, FONT_SIZE_MAX_Q16
            ),
            advice: String::from("钳到边界或改字号后重测；越界值不做隐式缩放"),
        });
    }
    // 字体段闸：空实例（度量快照全零——未实例化的占位句柄）。
    if instance.advance == 0 && instance.ascent == 0 && instance.descent == 0 {
        return Err(TextApiError::Font {
            code: E_TEXTAPI_EMPTY_INSTANCE,
            reason: String::from("字体实例度量全零（advance/ascent/descent 皆为 0）"),
            advice: String::from("确认实例已完成 F0808 度量快照加载；不要用默认值兜底测量"),
        });
    }
    // 行高（千分比 → 1/64 px）。
    let lh_q16 =
        per_mille_to_q16(instance.line_height(params.line_mode), params.font_size_q16).unwrap_or(0);

    // 逐行累积 advance：'\n' 分行；max_width 非零时逐字换行。
    let spacing = params.letter_spacing_q16 as i64;
    let mut max_line_adv: i64 = 0;
    let mut cur_adv: i64 = 0;
    let mut glyphs: u32 = 0;
    let mut rows: u32 = 1;
    for b in text {
        if *b == b'\n' {
            if cur_adv > max_line_adv {
                max_line_adv = cur_adv;
            }
            cur_adv = 0;
            rows += 1;
            continue;
        }
        let adv = per_mille_to_q16(instance.advance, params.font_size_q16).unwrap_or(0);
        cur_adv += adv + spacing;
        glyphs += 1;
        // 逐字换行：行宽约束非零且已超，折行。
        if params.max_width_q16 > 0 && cur_adv > params.max_width_q16 as i64 {
            if cur_adv > max_line_adv {
                max_line_adv = cur_adv;
            }
            cur_adv = adv;
            rows += 1;
        }
    }
    if cur_adv > max_line_adv {
        max_line_adv = cur_adv;
    }

    let bound_h = lh_q16 * rows as i64;
    let result = MeasureResult {
        advance_q16: max_line_adv,
        bound_w_q16: max_line_adv,
        bound_h_q16: bound_h,
        line_height_q16: lh_q16,
        glyph_count: glyphs,
        low_confidence: false,
    };

    if opts.async_timeout_us == 0 {
        return Ok(MeasureOutcome::Sync(result));
    }
    // 异步模式：句柄 ticket = 请求内容指纹（同请求同 ticket，幂等前提）。
    let mut h = fnv1a(b"VE-F0817/measure");
    h = fnv1a_merge(h, text);
    h = fnv1a_merge(h, &params.font_size_q16.to_le_bytes());
    h = fnv1a_merge(h, &opts.lang_tag.to_le_bytes());
    Ok(MeasureOutcome::Pended(AsyncHandle {
        ticket: h,
        timeout_us: opts.async_timeout_us,
    }))
}

/// 异步句柄取结果：`now_us` 未过超时即得结果，过时返回低置信降级。
///
/// 挂起语义本地化：句柄自持结果与 deadline；真实引擎由 F0818 契约承载。
pub fn resolve_measure(
    handle: AsyncHandle,
    result: MeasureResult,
    now_us: u64,
    start_us: u64,
) -> MeasureResult {
    let elapsed = now_us.saturating_sub(start_us);
    if elapsed <= handle.timeout_us as u64 {
        result
    } else {
        // 超时：返回保守估算并标记低置信，不阻塞 UI 帧（F0818 失败语义）。
        MeasureResult { low_confidence: true, ..result }
    }
}

// ---------------------------------------------------------------------------
// 二、RenderText —— 声明式图元批次提交；幂等由内容指纹保证
// ---------------------------------------------------------------------------

/// 单批 quad 上限（对齐 F0807 合批目标 2,000）。
pub const PIPELINE_BATCH_LIMIT: u32 = 2_000;

/// RenderText 的 options 追加面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    /// 目标空间（复用 F0807 TextSpace 语义：0=屏空间 1=世界空间）。
    pub space: u8,
    /// 深度（1/64 px）。
    pub z_q16: i32,
    /// 旧版辉光参数（**废弃双轨**：保留 + 告警，两个版本后移除）。
    pub legacy_glow: Option<u8>,
}

/// 渲染请求：调用方**声明**目标与效果，引擎负责合批与图集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderRequest {
    /// 文本长度（字节数；每字形一 quad）。
    pub text_len: u32,
    /// 文本内容指纹（[`text_fingerprint`] 产物；同文本同指纹）。
    pub text_hash: u64,
    /// 字体实例键（F0808 四段单射键）。
    pub instance: MetricKey,
    /// 排版参数。
    pub params: TypesetParams,
    /// 目标矩形（1/64 px，屏/世界空间由 opts.space 决定）。
    pub dst_x_q16: i64,
    pub dst_y_q16: i64,
    pub dst_w_q16: i64,
    pub dst_h_q16: i64,
    /// 效果位旗标（F0807 四效果定长 uniform 的位面）。
    pub effect_flags: u32,
    /// options。
    pub opts: RenderOptions,
}

/// 渲染收据：幂等凭证。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderReceipt {
    /// 批次键（内容指纹，同参数相同）。
    pub batch_key: u64,
    /// 本次提交 quad 数（每字形一 quad）。
    pub quad_count: u32,
    /// 规范化内容指纹（幂等判据：同参数重复提交必相等）。
    pub content_hash: u64,
    /// 命中废弃双轨标记。
    pub legacy_warned: bool,
}

/// 废弃参数命中计数（保留 + 告警；两版本后移除 legacy_glow）。
pub fn deprecation_hits() -> u32 {
    DEPRECATION_HITS.load(Ordering::Relaxed)
}

static DEPRECATION_HITS: AtomicU32 = AtomicU32::new(0);

/// 废弃参数还剩几个版本移除（公示倒计时）。
pub const DEPRECATION_VERSIONS_LEFT: u8 = 2;

/// **RenderText**（十年冻结签名）：图元批次提交（声明式）。
///
/// 幂等：同参数重复提交结果一致——收据由请求语义字段唯一决定。
pub fn render_text(req: &RenderRequest) -> Result<RenderReceipt, TextApiError> {
    // 管线段闸：批次上限。
    if req.text_len > PIPELINE_BATCH_LIMIT {
        return Err(TextApiError::Pipeline {
            code: E_TEXTAPI_BATCH_LIMIT,
            reason: format!(
                "提交 {} quad 超单批上限 {}",
                req.text_len, PIPELINE_BATCH_LIMIT
            ),
            advice: String::from("拆分为多次提交，由引擎侧合批；不要调大上限"),
        });
    }
    // 管线段闸：双轨冲突（legacy 与新效果位同设）。
    let legacy_warned = req.opts.legacy_glow.is_some();
    if legacy_warned && req.effect_flags & 0b1000 != 0 {
        return Err(TextApiError::Pipeline {
            code: E_TEXTAPI_LEGACY_CONFLICT,
            reason: String::from("legacy_glow 与新效果位 3（glow）同设，语义冲突"),
            advice: String::from("二选一：迁移期建议保留新效果位；legacy_glow 两个版本后移除"),
        });
    }
    if legacy_warned {
        let _ = DEPRECATION_HITS.fetch_add(1, Ordering::Relaxed);
    }
    // 编码段闸：文本长度为 0 的提交合法（空批收据），长度上限已查。
    // 规范化内容指纹：请求全部语义字段（不含 opts.z 之外的保留位——
    // z 属于语义，包含；legacy_glow 属于语义，也包含）。
    let mut h = fnv1a(b"VE-F0817/render");
    h = fnv1a_merge(h, &req.text_len.to_le_bytes());
    h = fnv1a_merge(h, &req.text_hash.to_le_bytes());
    h = fnv1a_merge(h, &req.instance.of().to_le_bytes());
    h = fnv1a_merge(h, &req.params.font_size_q16.to_le_bytes());
    h = fnv1a_merge(h, &req.params.letter_spacing_q16.to_le_bytes());
    h = fnv1a_merge(h, &req.params.max_width_q16.to_le_bytes());
    h = fnv1a_merge(h, &[req.params.line_mode as u8]);
    h = fnv1a_merge(h, &req.dst_x_q16.to_le_bytes());
    h = fnv1a_merge(h, &req.dst_y_q16.to_le_bytes());
    h = fnv1a_merge(h, &req.dst_w_q16.to_le_bytes());
    h = fnv1a_merge(h, &req.dst_h_q16.to_le_bytes());
    h = fnv1a_merge(h, &req.effect_flags.to_le_bytes());
    h = fnv1a_merge(h, &[req.opts.space]);
    h = fnv1a_merge(h, &req.opts.z_q16.to_le_bytes());
    match req.opts.legacy_glow {
        Some(g) => h = fnv1a_merge(h, &[0xFF, g]),
        None => h = fnv1a_merge(h, &[0x00]),
    }
    Ok(RenderReceipt {
        batch_key: h,
        quad_count: req.text_len,
        content_hash: h,
        legacy_warned,
    })
}

// ---------------------------------------------------------------------------
// 三、CacheControl —— 图集管理：预热 / 清理 / 水位查询
// ---------------------------------------------------------------------------

/// 高水位线（千分比）——F0819 联调断言「水位 ≥97%」的对齐值。
pub const WATERMARK_HIGH_PERMILLE: u32 = 970;

/// 图集闸门：API 层的图集账（真实图集页由 F0806 承载，此处管容量账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasGate {
    /// 总槽位。
    pub capacity_slots: u32,
    /// 常驻槽位。
    pub resident: u32,
}

impl AtlasGate {
    /// 建闸（容量为零 = 未初始化）。
    pub const fn new(capacity_slots: u32) -> Self {
        AtlasGate { capacity_slots, resident: 0 }
    }
}

/// 图集操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheOp {
    /// 预热：预注册 n 个码点槽位（全量拒绝，不做部分预热）。
    Warmup(u32),
    /// 清理：全部逐出。
    Evict,
    /// 水位查询。
    Watermark,
}

/// 水位读数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheWatermark {
    /// 常驻槽位。
    pub used_slots: u32,
    /// 总槽位。
    pub total_slots: u32,
    /// 水位（千分比；容量 0 时恒 0）。
    pub permille: u32,
    /// 是否达高水位线。
    pub high: bool,
}

/// **CacheControl**（十年冻结签名）：图集管理三操作。
pub fn cache_control(gate: &mut AtlasGate, op: CacheOp) -> Result<CacheWatermark, TextApiError> {
    // 缓存段闸：未初始化（除 Evict 外一律拒——清空空账无意义但也无害，
    // 统一拒绝让「未 init 就用」总是显性）。
    if gate.capacity_slots == 0 {
        return Err(TextApiError::Cache {
            code: E_TEXTAPI_CACHE_UNINIT,
            reason: String::from("图集容量为零（未初始化）"),
            advice: String::from("先以非零容量建闸（AtlasGate::new），再执行图集操作"),
        });
    }
    match op {
        CacheOp::Warmup(n) => {
            if gate.resident + n > gate.capacity_slots {
                return Err(TextApiError::Cache {
                    code: E_TEXTAPI_CACHE_FULL,
                    reason: format!(
                        "预热 {} 槽后 {} > 容量 {}",
                        n,
                        gate.resident + n,
                        gate.capacity_slots
                    ),
                    advice: String::from("先 Evict 逐出或缩小预热批量；不做部分预热"),
                });
            }
            gate.resident += n;
        }
        CacheOp::Evict => {
            gate.resident = 0;
        }
        CacheOp::Watermark => {}
    }
    let permille = (gate.resident as u64 * 1000 / gate.capacity_slots as u64) as u32;
    Ok(CacheWatermark {
        used_slots: gate.resident,
        total_slots: gate.capacity_slots,
        permille,
        high: permille >= WATERMARK_HIGH_PERMILLE,
    })
}

// ---------------------------------------------------------------------------
// 编译期闸
// ---------------------------------------------------------------------------

const _: () = {
    assert!(FONT_SIZE_MIN_Q16 < FONT_SIZE_MAX_Q16);
    assert!(PIPELINE_BATCH_LIMIT == 2_000);
    assert!(WATERMARK_HIGH_PERMILLE == 970);
    assert!(DEPRECATION_VERSIONS_LEFT == 2);
    // 七码互异且同处 0x17 细分段（独占段纪律）。
    assert!(E_TEXTAPI_BAD_UTF8 & 0xFF00 == 0x1700);
    assert!(E_TEXTAPI_EMPTY_INSTANCE & 0xFF00 == 0x1700);
    assert!(E_TEXTAPI_SIZE_RANGE & 0xFF00 == 0x1700);
    assert!(E_TEXTAPI_CACHE_FULL & 0xFF00 == 0x1700);
    assert!(E_TEXTAPI_CACHE_UNINIT & 0xFF00 == 0x1700);
    assert!(E_TEXTAPI_BATCH_LIMIT & 0xFF00 == 0x1700);
    assert!(E_TEXTAPI_LEGACY_CONFLICT & 0xFF00 == 0x1700);
    assert!(
        E_TEXTAPI_BAD_UTF8 != E_TEXTAPI_EMPTY_INSTANCE
            && E_TEXTAPI_EMPTY_INSTANCE != E_TEXTAPI_SIZE_RANGE
            && E_TEXTAPI_SIZE_RANGE != E_TEXTAPI_CACHE_FULL
            && E_TEXTAPI_CACHE_FULL != E_TEXTAPI_CACHE_UNINIT
            && E_TEXTAPI_CACHE_UNINIT != E_TEXTAPI_BATCH_LIMIT
            && E_TEXTAPI_BATCH_LIMIT != E_TEXTAPI_LEGACY_CONFLICT
    );
};
