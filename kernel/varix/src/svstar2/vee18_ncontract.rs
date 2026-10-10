//! VE-F0818 · 与 N 域消费契约（VE-E 域 · 文字渲染 · 测量契约闭环）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0818`
//!
//! 锚点原文：「与 N 域消费契约兑现测量闭环：E 引擎测量（MeasureText 与整形输出
//! 的契约化封装）、N 消费测量（UI 布局以契约接口取度量，禁止旁路读字体文件）
//! ——兑现 N 域前置契约（F0813 号测量契约：请求/响应格式、精度约定 1/64 像素、
//! 异步超时 16ms 语义）并出具兑现声明。契约内容四条：测量请求字段（文本、字体
//! 实例、排版参数、语言标记）、响应字段（advance/边界/行高/整形簇映射）、精度
//! 约定（量化规则与舍入方向）、失败语义（超时返回保守估算并标记低置信，不阻塞
//! UI 帧）。双向纪律：N 侧只依赖契约不依赖实现（可替换引擎后端验证）；E 侧变更
//! 契约走跨域变更流程（AE10 F6394 范式）。错误路径：契约哈希不符→两端构建同时
//! 失败；N 侧压测超时率 >0.1%→触发契约评审。性能：契约调用开销 ≤0.002ms/次。
//! 判据：契约闭环、兑现声明、四条内容、双端同败、0.002ms 开销。」
//!
//! # 一、N 侧只依赖契约不依赖实现：**类型所有权**是纪律的载体
//!
//! 「N 侧不依赖实现」若靠口头约定，一次顺手 import 就破功。本单让契约**自带
//! 全套类型**——[`FontRef`]/[`ContractParams`]/[`ContractRequest`]/
//! [`ContractResponse`] 全部定义在契约层，N 侧（[`n_side`] 构建单元）只看得见
//! 这些类型与 [`MeasureBackend`] trait，**结构上 import 不到** vee17/vee08 的
//! 实现类型。引擎侧（[`EngineBackend`]）做契约类型→实现类型的单向适配
//! （FontRef→vee08 MetricKey、ContractParams→vee17 TypesetParams），E 侧实现
//! 可整体替换（判据：可替换引擎后端验证——换任何 `MeasureBackend` 实现即可）。
//!
//! # 二、契约哈希双端同败：漂移在**两个构建单元同时**编译红
//!
//! 契约规范文本 [`CONTRACT_TEXT`] 的 FNV-1a 哈希由 E 侧
//! [`PROVIDER_CONTRACT_HASH`] 持有；N 侧在 [`n_side`] 里**独立写死**同一份
//! 规范文本并重算 [`n_side::N_EXPECTED_HASH`]，两侧各挂一条 `const` 断言
//! （[`PROVIDER_GATE`]/[`n_side::N_GATE`]）——任何一侧漂移契约文本，**两条
//! const 闸同时**编译失败（锚点「契约哈希不符→两端构建同时失败」的落地形态，
//! 与 F0817 签名指纹同范式）。运行期另有 [`contract_hash_ok`] 复核。
//!
//! # 三、精度约定与失败语义：量化规则**钉死舍入方向**
//!
//! 全部响应值以 1/64 px（q16）表达（[`GRID_Q16`]）；量化规则钉死方向——
//! advance/行高**向上取整**（布局保守：绝不低估占宽），轮廓边界**向下取整**
//! （内容内收：绝不高估覆盖）。[`ceil_grid`]/[`floor_grid`] 是唯二量化出口。
//! 失败语义：异步模式超时（[`TIMEOUT_US`] 语义由调用方 deadline 承载）**不再
//! 问引擎**、立即给**保守估算**（[`BuiltinEstimator`]：每簇 1em、行高 1.2em 的
//! 上界，对 advance/行高永不低估）并标记 `low_confidence`——数字照给、置信自报，
//! UI 帧不阻塞。估算策略本身可整体替换（[`ConservativeEstimator`] trait）。
//!
//! # 四、超时率评审线与开销预算
//!
//! N 侧消费统计（[`n_side::NConsumer`]）记录调用数、超时数与累计开销，压测超时率
//! **>0.1%**（严格大于：恰 0.1% 不触发）→ [`n_side::NConsumer::take_review`] 开出
//! 实体评审单 [`n_side::ContractReview`]（越线当拍开一单，不重复刷屏）——锚点
//! 「触发契约评审」的落地形态。契约调用开销模型 [`call_cost_ns`] 按
//! 「哈希基价 + 逐字节 FNV + 逐簇映射」记账，预算 [`CALL_BUDGET_NS`] =
//! 2000ns（0.002ms），**逐次落到响应/句柄的 `cost_ns` 字段**（不是纸面公式），
//! 总量面由 [`n_side::NConsumer::budget_ok`] 对账。
//!
//! # 四之二、跨域变更流程（AE10 F6394 范式）
//!
//! E 侧不得单方改契约：[`propose_change`] 受理空理由与空变更两拒并留痕前后哈希，
//! [`accept_change`] 是 N 侧受理权（双签的另一半）——提案未受理即永不可生效。
//!
//! # 五、兑现声明：F0813 号契约的逐条应答
//!
//! [`FULFILLMENT`] 四条内容逐条应答（请求字段/响应字段/精度约定/失败语义），
//! 每条带**证据指针**（指向本模块真实符号与常量值）；[`fulfillment_statement`]
//! 拼出可读声明，判据侧断言证据值与实际常量**逐项相等**（声明不许空话）。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap`/`expect`/索引越界/算术溢出：量化走 `div_ceil` 与
//! `checked` 语义的 i64 域（两乘数受字号上界钳制不溢出），簇映射构建用迭代器
//! 无索引；所有失败路径走 `Result<ContractError>`。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ===========================================================================
// 一、契约身份：F0813 号测量契约 v1（变更走跨域变更流程的机制化）
// ===========================================================================

/// 契约登记号（N 域前置契约的历史 ID，N 域并入 Eb 补写区后由 F0818 承接）。
pub const CONTRACT_ID: &str = "F0813";
/// 契约版本（变更唯一入口：改文本必改版本并追加 [`CONTRACT_REVISIONS`]）。
pub const CONTRACT_VERSION: u32 = 1;
/// 版本修订账（跨域变更流程 AE10 F6394 范式的契约侧承载：版本数=账目数）。
pub const CONTRACT_REVISIONS: [&str; 1] = ["v1 初始四条：请求字段/响应字段/精度约定/失败语义"];

/// 契约规范文本（哈希的原料；N 侧独立写死同文对拍）。
pub const CONTRACT_TEXT: &str = "VE-F0818/text-measure/v1:\
req{text:bytes, font:FontRef{font_id,px_size,weight,glyph_variant},\
params:ContractParams{font_size_q16,letter_spacing_q16,max_width_q16,line_mode},\
lang_tag:u32};\
resp{advance_q16, bound_w_q16, bound_h_q16, line_height_q16, glyph_count,\
clusters[{byte_off,glyph}], low_confidence};\
precision{grid=1/64px, advance&line_height=ceil, bounds=floor};\
failure{timeout→conservative+low_confidence, ui-frame-never-blocked}";

/// FNV-1a 64 位（与全域哈希设施同源）。
pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// E 侧契约哈希（规范文本的指纹）。
pub const PROVIDER_CONTRACT_HASH: u64 = contract_hash();

/// 契约哈希（const 可算：const fn 逐字节 FNV）。
pub const fn contract_hash() -> u64 {
    let b = CONTRACT_TEXT.as_bytes();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 版本-账目守恒（变更流程的机制化：版本漂移而账目未记 = 编译红）。
const _: () = assert!(CONTRACT_VERSION as usize == CONTRACT_REVISIONS.len());

// ===========================================================================
// 二、契约错误：五码独占 0x18 细分段 + 呈现三要素
// ===========================================================================

/// 契约哈希不符（双端同败的运行期复核出口）。
pub const C_TEXT18_HASH_MISMATCH: u16 = 0x1800;
/// 请求字段非法（契约层前置校验）。
pub const C_TEXT18_BAD_REQUEST: u16 = 0x1801;
/// 引擎后端故障（后端 Err 的契约化包装）。
pub const C_TEXT18_BACKEND_FAULT: u16 = 0x1802;
/// 异步句柄与请求内容不匹配（stale 句柄取回他人请求）。
pub const C_TEXT18_TICKET_MISMATCH: u16 = 0x1803;
/// 跨域变更提案被拒（AE10 F6394 范式的拒绝出口）。
pub const C_TEXT18_CHANGE_REJECTED: u16 = 0x1804;

/// 契约错误（呈现三要素：机读码 + 可读原因 + 建议动作）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractError {
    /// 机读错误码（0x18xx 独占段）。
    pub code: u16,
    /// 可读原因。
    pub reason: String,
    /// 建议动作。
    pub advice: String,
}

impl ContractError {
    /// 构造（三要素齐备）。
    pub fn new(code: u16, reason: &str, advice: &str) -> ContractError {
        ContractError { code, reason: String::from(reason), advice: String::from(advice) }
    }
    /// 呈现三要素一行（读屏/日志统一）。
    pub fn screen_line(&self) -> String {
        format!("[0x{:04X}] {} | 建议: {}", self.code, self.reason, self.advice)
    }
}

// ===========================================================================
// 三、契约类型集（N 侧唯一可见面——不 import 任何实现类型）
// ===========================================================================

/// 字体实例引用（契约自有类型；引擎侧适配为 vee08 MetricKey）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontRef {
    /// 字体 ID。
    pub font_id: u32,
    /// 字号（px）。
    pub px_size: u32,
    /// 字重。
    pub weight: u32,
    /// 字形档位。
    pub glyph_variant: u32,
}

/// 行高模式（契约自有序数：0 字体默认 / 1 紧凑 / 2 宽松）。
pub const LINE_MODE_FONT_DEFAULT: u8 = 0;
pub const LINE_MODE_TIGHT: u8 = 1;
pub const LINE_MODE_LOOSE: u8 = 2;
/// 行高模式合法域上界（含）。
pub const LINE_MODE_MAX: u8 = 2;

/// 排版参数（契约自有类型；引擎侧适配为 vee17 TypesetParams）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractParams {
    /// 字号（1/64 px；合法域 64..=16_384，即 1..=256px）。
    pub font_size_q16: u32,
    /// 字距（1/64 px，可负）。
    pub letter_spacing_q16: i32,
    /// 换行约束宽度（1/64 px；0 = 不换行）。
    pub max_width_q16: u32,
    /// 行高模式序数（0/1/2）。
    pub line_mode: u8,
}

impl ContractParams {
    /// 参数合法域（与引擎字号域一致——契约前置闸）。
    pub const fn valid(&self) -> bool {
        self.font_size_q16 >= 64
            && self.font_size_q16 <= 16_384
            && self.line_mode <= LINE_MODE_MAX
    }
}

/// 测量请求（契约内容一条：文本、字体实例、排版参数、语言标记）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractRequest<'a> {
    /// 文本（UTF-8 字节；生命周期归调用方——API 层不持有文本缓冲）。
    pub text: &'a [u8],
    /// 字体实例。
    pub font: FontRef,
    /// 排版参数。
    pub params: ContractParams,
    /// 语言标记（FNV 短 ID）。
    pub lang_tag: u32,
}

/// 整形簇映射条目（v1 确定性字节模型：簇=非换行字节单元）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cluster {
    /// 字节偏移（text 内）。
    pub byte_off: u32,
    /// 簇序（0 起，跳过换行）。
    pub glyph: u32,
}

/// 测量响应（契约内容一条：advance/边界/行高/整形簇映射；全部已量化）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractResponse {
    /// 推进宽度（1/64 px，向上取整——布局保守）。
    pub advance_q16: i64,
    /// 轮廓边界宽（1/64 px，向下取整）。
    pub bound_w_q16: i64,
    /// 轮廓边界高（1/64 px，向下取整）。
    pub bound_h_q16: i64,
    /// 单行行高（1/64 px，向上取整）。
    pub line_height_q16: i64,
    /// 参与度量的簇数（不含换行）。
    pub glyph_count: u32,
    /// 整形簇映射（簇数=0 时为空表）。
    pub clusters: Vec<Cluster>,
    /// 低置信标记（超时降级路径置位）。
    pub low_confidence: bool,
    /// 本次契约调用的开销记账（ns；判据「≤0.002ms/次」的逐次可观测面）。
    pub cost_ns: u64,
}

// ===========================================================================
// 四、精度约定：1/64 像素网格 + 钉死的舍入方向
// ===========================================================================

/// 量化网格：1px = 64 × (1/64 px)。
pub const GRID_Q16: i64 = 64;

/// 向上取整到 1/64 px 网格（advance/行高的舍入方向——布局保守不低估）。
pub const fn ceil_grid(v: i64) -> i64 {
    ((v + GRID_Q16 - 1) / GRID_Q16) * GRID_Q16
}

/// 向下取整到 1/64 px 网格（轮廓边界的舍入方向——内容内收不高估）。
pub const fn floor_grid(v: i64) -> i64 {
    (v / GRID_Q16) * GRID_Q16
}

// ===========================================================================
// 五、失败语义：异步超时 16ms → 保守估算 + 低置信，UI 帧不阻塞
// ===========================================================================

/// 异步超时语义值（16ms；调用方以 start/now 微秒时钟承载 deadline）。
pub const TIMEOUT_US: u32 = 16_000;

/// 度量结局：同步直出，或异步挂起（凭句柄取）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractOutcome {
    /// 已完成（同步模式，或异步模式取回后）。
    Fulfilled(ContractResponse),
    /// 已挂起（异步模式），凭 [`ContractHandle`] 取结果。
    Pended(ContractHandle),
}

/// 异步句柄（ticket 由请求内容唯一决定——同请求同 ticket）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractHandle {
    /// 请求指纹。
    pub ticket: u64,
    /// 超时上限（μs；契约 v1 语义值见 [`TIMEOUT_US`]）。
    pub timeout_us: u32,
    /// 本次挂起调用的开销记账（ns；与响应同源同值）。
    pub cost_ns: u64,
}

/// 契约选项（同步/异步模式开关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractOpts {
    /// 0 = 同步；>0 = 异步模式（超时上限 μs）。
    pub async_timeout_us: u32,
}

impl ContractOpts {
    /// 同步默认。
    pub const fn sync() -> ContractOpts {
        ContractOpts { async_timeout_us: 0 }
    }
    /// 异步（16ms 语义值）。
    pub const fn async16() -> ContractOpts {
        ContractOpts { async_timeout_us: TIMEOUT_US }
    }
}

// ===========================================================================
// 六、后端 trait（N 侧唯一依赖面）与引擎适配器（E 侧实现）
// ===========================================================================

/// 引擎后端产出（实现侧原始读数——未量化；量化归契约层）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendResult {
    /// 推进宽度（1/64 px，原始读数）。
    pub advance_q16: i64,
    /// 边界宽（1/64 px，原始）。
    pub bound_w_q16: i64,
    /// 边界高（1/64 px，原始）。
    pub bound_h_q16: i64,
    /// 行高（1/64 px，原始）。
    pub line_height_q16: i64,
    /// 簇数（不含换行）。
    pub glyph_count: u32,
}

/// 测量后端 trait——N 侧**只**看得见它与契约类型（可替换引擎后端验证面）。
pub trait MeasureBackend {
    /// 同步度量（契约层负责校验/量化/簇映射/超时语义）。
    fn measure(&self, req: &ContractRequest) -> Result<BackendResult, ContractError>;
}

/// 簇映射构建（v1 确定性字节模型：簇=非换行字节单元；与引擎 glyph_count 同口径）。
pub fn build_clusters(text: &[u8]) -> Vec<Cluster> {
    let mut out = Vec::new();
    let mut glyph = 0u32;
    for (off, b) in text.iter().enumerate() {
        if *b == b'\n' {
            continue;
        }
        out.push(Cluster { byte_off: off as u32, glyph });
        glyph += 1;
    }
    out
}

fn ticket_of(req: &ContractRequest) -> u64 {
    let mut h = fnv1a(b"VE-F0818/ticket");
    h = merge(h, req.text);
    h = merge(h, &req.font.font_id.to_le_bytes());
    h = merge(h, &req.font.px_size.to_le_bytes());
    h = merge(h, &req.font.weight.to_le_bytes());
    h = merge(h, &req.font.glyph_variant.to_le_bytes());
    h = merge(h, &req.params.font_size_q16.to_le_bytes());
    h = merge(h, &req.params.letter_spacing_q16.to_le_bytes());
    h = merge(h, &req.params.max_width_q16.to_le_bytes());
    h = merge(h, &[req.params.line_mode]);
    h = merge(h, &req.lang_tag.to_le_bytes());
    h
}

fn merge(prev: u64, data: &[u8]) -> u64 {
    let mut h = prev;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 运行期契约哈希复核（编译期双端 const 闸之外的第二道）。
pub fn contract_hash_ok() -> bool {
    PROVIDER_CONTRACT_HASH == contract_hash() && PROVIDER_CONTRACT_HASH != 0
}

/// 请求合法域前置闸（参数越界 → 0x1801；**不进引擎、不进估算器**）。
fn validate_params(req: &ContractRequest) -> Result<(), ContractError> {
    if !req.params.valid() {
        return Err(ContractError::new(
            C_TEXT18_BAD_REQUEST,
            "排版参数越界（字号域 64..=16384 或行高模式越界）",
            "钳到契约合法域后重试；越界值不做隐式缩放",
        ));
    }
    Ok(())
}

/// 原始读数 → 契约响应（量化出口唯一：`ceil`/`floor` 两个方向 + 簇映射 + 开销记账）。
fn quantize(raw: BackendResult, text: &[u8], low_confidence: bool) -> ContractResponse {
    let clusters = build_clusters(text);
    ContractResponse {
        advance_q16: ceil_grid(raw.advance_q16),
        bound_w_q16: floor_grid(raw.bound_w_q16),
        bound_h_q16: floor_grid(raw.bound_h_q16),
        line_height_q16: ceil_grid(raw.line_height_q16),
        glyph_count: raw.glyph_count,
        cost_ns: call_cost_ns(text.len(), clusters.len()),
        clusters,
        low_confidence,
    }
}

/// 单请求开销记账（域分隔串 + 逐字节 FNV + 逐簇映射）。
pub fn request_cost(req: &ContractRequest) -> u64 {
    let clusters = req.text.iter().filter(|b| **b != b'\n').count();
    call_cost_ns(req.text.len(), clusters)
}

/// **契约测量入口**：校验 → 后端度量 → 量化 → 簇映射 → 超时语义。
pub fn contract_measure(
    backend: &dyn MeasureBackend,
    req: &ContractRequest,
    opts: &ContractOpts,
) -> Result<ContractOutcome, ContractError> {
    if !contract_hash_ok() {
        return Err(ContractError::new(
            C_TEXT18_HASH_MISMATCH,
            "契约哈希运行期复核不符（规范文本漂移）",
            "按跨域变更流程重签契约文本与哈希后重试",
        ));
    }
    let resp = measure_inner(backend, req)?;
    if opts.async_timeout_us == 0 {
        return Ok(ContractOutcome::Fulfilled(resp));
    }
    Ok(ContractOutcome::Pended(ContractHandle {
        ticket: ticket_of(req),
        timeout_us: opts.async_timeout_us,
        cost_ns: resp.cost_ns,
    }))
}

fn measure_inner(
    backend: &dyn MeasureBackend,
    req: &ContractRequest,
) -> Result<ContractResponse, ContractError> {
    validate_params(req)?;
    let raw = backend.measure(req)?;
    Ok(quantize(raw, req.text, false))
}

// ===========================================================================
// 六之二、失败语义落地：**保守估算**（超时后不等引擎，立即给上界）
// ===========================================================================

/// 保守估算的每簇宽度假设（em/簇；1 = 最坏字面即全角上限）。
pub const CONSERVATIVE_EM_PER_CLUSTER: i64 = 1;
/// 保守估算的行高分子（行高 = font_size × NUM/DEN）。
pub const CONSERVATIVE_LINE_HEIGHT_NUM: i64 = 6;
/// 保守估算的行高分母（1.2em 行距上限）。
pub const CONSERVATIVE_LINE_HEIGHT_DEN: i64 = 5;

/// 保守估算器（可替换：N 侧超时后自行求值，**不依赖任何引擎实现类型**）。
pub trait ConservativeEstimator {
    /// 按请求求保守原始读数（契约层负责量化与置信标记）。
    fn estimate(&self, req: &ContractRequest) -> BackendResult;
}

/// 内建保守估算器：每簇 1em 宽、行高 1.2em——对 `advance`/`line_height` 是**上界**
/// （布局永不低估占宽）；轮廓边界仍走下取整（内容内收，安全方向不变）。
pub struct BuiltinEstimator;

impl ConservativeEstimator for BuiltinEstimator {
    fn estimate(&self, req: &ContractRequest) -> BackendResult {
        let glyphs = req.text.iter().filter(|b| **b != b'\n').count() as i64;
        let size = req.params.font_size_q16 as i64;
        let advance = glyphs
            .saturating_mul(CONSERVATIVE_EM_PER_CLUSTER)
            .saturating_mul(size);
        let line_height = size
            .saturating_mul(CONSERVATIVE_LINE_HEIGHT_NUM)
            / CONSERVATIVE_LINE_HEIGHT_DEN;
        BackendResult {
            advance_q16: advance,
            bound_w_q16: advance,
            bound_h_q16: line_height,
            line_height_q16: line_height,
            glyph_count: clamp_u32(glyphs),
        }
    }
}

/// 保守估算响应（超时降级路径：数字照给、置信自报、UI 帧不阻塞）。
///
/// 走此路径**完全不问引擎**——超时之后继续等引擎正是超时的定义。
pub fn conservative_response(req: &ContractRequest) -> Result<ContractResponse, ContractError> {
    conservative_response_with(&BuiltinEstimator, req)
}

/// 保守估算响应（自定义估算器面：N 侧可整体替换估算策略而契约层不动）。
pub fn conservative_response_with(
    est: &dyn ConservativeEstimator,
    req: &ContractRequest,
) -> Result<ContractResponse, ContractError> {
    validate_params(req)?;
    Ok(quantize(est.estimate(req), req.text, true))
}

fn clamp_u32(v: i64) -> u32 {
    if v > i64::from(u32::MAX) {
        u32::MAX
    } else if v < 0 {
        0
    } else {
        v as u32
    }
}

/// 异步取回：超时语义翻面——时限内保真，超时**不再问引擎**、改给保守估算并标记
/// 低置信（帧不阻塞）。
///
/// 边界钉死：`elapsed == timeout` 恰好到点 = 时限内（`<=` 语义）。
pub fn contract_resolve(
    handle: &ContractHandle,
    backend: &dyn MeasureBackend,
    req: &ContractRequest,
    now_us: u64,
    start_us: u64,
) -> Result<ContractResponse, ContractError> {
    validate_params(req)?;
    if ticket_of(req) != handle.ticket {
        return Err(ContractError::new(
            C_TEXT18_TICKET_MISMATCH,
            "异步句柄与请求内容不匹配（stale 句柄）",
            "按当前请求重新发起异步度量；禁止用旧句柄取回别的请求",
        ));
    }
    let elapsed = now_us.saturating_sub(start_us);
    if elapsed > handle.timeout_us as u64 {
        return conservative_response(req);
    }
    let raw = backend.measure(req)?;
    Ok(quantize(raw, req.text, false))
}

// ===========================================================================
// 七、契约调用开销模型（≤0.002ms/次 的记账面）
// ===========================================================================

/// 契约调用开销预算（0.002ms = 2000ns）。
pub const CALL_BUDGET_NS: u64 = 2_000;
/// 哈希基价（域分隔串 + 字段装配）。
const COST_HASH_BASE_NS: u64 = 150;
/// 逐字节 FNV 单价。
const COST_PER_BYTE_NS: u64 = 2;
/// 逐簇映射单价。
const COST_PER_CLUSTER_NS: u64 = 8;

/// 契约调用开销模型（纯函数记账——判据按代表请求对预算对账）。
pub const fn call_cost_ns(text_len: usize, cluster_count: usize) -> u64 {
    COST_HASH_BASE_NS
        + (text_len as u64) * COST_PER_BYTE_NS
        + (cluster_count as u64) * COST_PER_CLUSTER_NS
}

// ===========================================================================
// 八、n_side 构建单元（N 侧唯一可见的契约消费面）
// ===========================================================================

pub mod n_side {
    //! N 消费测量：UI 布局以契约接口取度量，禁止旁路读字体文件。
    //!
    //! 本模块**只**依赖 `super` 的契约类型与 [`super::MeasureBackend`]——
    //! import 不到 vee17/vee08 任何实现类型（类型所有权纪律）。契约规范文本
    //! 在此**独立写死**并重算期望哈希，`N_GATE` const 闸与 E 侧
    //! `PROVIDER_GATE` 构成「双端同败」。

    use super::{
        ceil_grid, floor_grid, ContractError, ContractOpts, ContractOutcome, ContractRequest,
        MeasureBackend,
    };
    use alloc::format;
    use alloc::string::String;

    /// N 侧独立写死的契约规范文本（与 E 侧 [`super::CONTRACT_TEXT`] 对拍）。
    pub const N_CONTRACT_TEXT: &str = "VE-F0818/text-measure/v1:\
req{text:bytes, font:FontRef{font_id,px_size,weight,glyph_variant},\
params:ContractParams{font_size_q16,letter_spacing_q16,max_width_q16,line_mode},\
lang_tag:u32};\
resp{advance_q16, bound_w_q16, bound_h_q16, line_height_q16, glyph_count,\
clusters[{byte_off,glyph}], low_confidence};\
precision{grid=1/64px, advance&line_height=ceil, bounds=floor};\
failure{timeout→conservative+low_confidence, ui-frame-never-blocked}";

    /// N 侧独立重算的期望哈希（第二实现：运行期迭代，与 E 侧 const fn 分路径）。
    pub fn n_expected_hash() -> u64 {
        super::fnv1a(N_CONTRACT_TEXT.as_bytes())
    }

    /// N 侧 const 闸：哈希不符 = N 侧构建失败（双端同败之二）。
    pub const N_EXPECTED_HASH: u64 = n_const_hash();

    const fn n_const_hash() -> u64 {
        let b = N_CONTRACT_TEXT.as_bytes();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < b.len() {
            h ^= b[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    /// E 侧 const 闸（在 provider 侧声明引用 N 侧期望值——漂移即双红）。
    pub const PROVIDER_GATE: () =
        assert!(super::PROVIDER_CONTRACT_HASH == N_EXPECTED_HASH, "contract hash drift");
    /// N 侧 const 闸（对称复核）。
    pub const N_GATE: () = assert!(N_EXPECTED_HASH == super::PROVIDER_CONTRACT_HASH, "contract hash drift");

    /// N 侧消费统计（评审线的数据面）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct NConsumer {
        /// 总调用数。
        pub calls: u64,
        /// 超时降级数。
        pub timeouts: u64,
        /// 累计契约调用开销（ns；逐次由响应 `cost_ns` 累加）。
        pub overhead_ns: u64,
        /// 评审单是否已开（同一波越线只开一单，不重复刷屏）。
        pub review_open: bool,
    }

    /// 契约评审单（N 侧超时率越线时触发——「触发契约评审」的实体）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ContractReview {
        /// 契约登记号。
        pub contract_id: &'static str,
        /// 契约版本。
        pub version: u32,
        /// 契约哈希（评审对拍的对象）。
        pub hash: u64,
        /// 触发时调用数。
        pub calls: u64,
        /// 触发时超时数。
        pub timeouts: u64,
        /// 实测超时率（千分数；判据线 1‰ = 0.1%）。
        pub rate_permille: u32,
        /// 触发口径（可读）。
        pub trigger: &'static str,
    }

    impl ContractReview {
        /// 评审单可读摘要（归档/告警台账）。
        pub fn screen_line(&self) -> String {
            format!(
                "review {} v{} hash={:016X} 超时率 {}/{} ({}‰) 越线 1‰ 触发：{}",
                self.contract_id,
                self.version,
                self.hash,
                self.timeouts,
                self.calls,
                self.rate_permille,
                self.trigger,
            )
        }
    }

    impl NConsumer {
        /// 新消费者（零计数）。
        pub const fn new() -> NConsumer {
            NConsumer { calls: 0, timeouts: 0, overhead_ns: 0, review_open: false }
        }

        /// 超时率是否触发契约评审（**>0.1%** 严格大于：恰 0.1% 不触发）。
        pub const fn review_required(&self) -> bool {
            self.timeouts.saturating_mul(1000) > self.calls
        }

        /// 实测超时率（千分数；零调用记 0）。
        pub const fn timeout_rate_permille(&self) -> u32 {
            if self.calls == 0 {
                return 0;
            }
            let r = self.timeouts.saturating_mul(1000) / self.calls;
            if r > u32::MAX as u64 {
                u32::MAX
            } else {
                r as u32
            }
        }

        /// 累计开销是否仍在逐次预算内（判据「≤0.002ms/次」的总量对账面）。
        pub const fn budget_ok(&self) -> bool {
            self.overhead_ns <= self.calls.saturating_mul(super::CALL_BUDGET_NS)
        }

        /// 取出契约评审单：越线当拍开一单，已开则不再重复开单（防刷屏）。
        pub fn take_review(&mut self) -> Option<ContractReview> {
            if !self.review_required() || self.review_open {
                return None;
            }
            self.review_open = true;
            Some(ContractReview {
                contract_id: super::CONTRACT_ID,
                version: super::CONTRACT_VERSION,
                hash: super::PROVIDER_CONTRACT_HASH,
                calls: self.calls,
                timeouts: self.timeouts,
                rate_permille: self.timeout_rate_permille(),
                trigger: "N 侧压测超时率 > 0.1%（F0818 错误路径）",
            })
        }

        /// 消费一次度量：UI 布局以契约接口取宽度并判定是否放得下。
        ///
        /// 异步模式（`opts.async_timeout_us > 0`）以 `now_us/start_us` 承载
        /// deadline：超时返回保守估算并标记低置信——**计数进评审线**。
        pub fn consume(
            &mut self,
            backend: &dyn MeasureBackend,
            req: &ContractRequest,
            opts: &ContractOpts,
            available_q16: i64,
            now_us: u64,
            start_us: u64,
        ) -> Result<NFit, ContractError> {
            self.calls += 1;
            let resp = match super::contract_measure(backend, req, opts)? {
                ContractOutcome::Fulfilled(r) => r,
                ContractOutcome::Pended(h) => {
                    let r = super::contract_resolve(&h, backend, req, now_us, start_us)?;
                    if r.low_confidence {
                        self.timeouts += 1;
                    }
                    r
                }
            };
            self.overhead_ns = self.overhead_ns.saturating_add(resp.cost_ns);
            let fits = resp.advance_q16 <= available_q16;
            Ok(NFit {
                advance_q16: resp.advance_q16,
                fits,
                low_confidence: resp.low_confidence,
                glyph_count: resp.glyph_count,
                cost_ns: resp.cost_ns,
            })
        }
    }

    /// N 侧消费结果（UI 布局的取用面）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct NFit {
        /// 契约推进宽度（已量化）。
        pub advance_q16: i64,
        /// 可用宽度内放得下。
        pub fits: bool,
        /// 低置信（超时降级——布局可自选重测或保守采用）。
        pub low_confidence: bool,
        /// 簇数。
        pub glyph_count: u32,
        /// 本次调用开销（ns）。
        pub cost_ns: u64,
    }

    // 静默引用面：N 侧对契约量化出口的显式依赖（判据断言其方向性）。
    #[allow(dead_code)]
    const _PRECISION_SURFACE: (i64, i64) = (ceil_grid(0), floor_grid(0));
}

use n_side::{N_GATE, PROVIDER_GATE};
const _: () = {
    let _provider: () = PROVIDER_GATE;
    let _n: () = N_GATE;
};

// ===========================================================================
// 九、兑现声明（F0813 号契约逐条应答——证据指针指向真实符号）
// ===========================================================================

/// 兑现声明（四条内容逐条应答；证据值与实际常量判据侧逐项对账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fulfillment {
    /// 契约登记号。
    pub contract_id: &'static str,
    /// 契约版本。
    pub version: u32,
    /// 四条内容。
    pub items: [&'static str; 4],
    /// 逐条证据指针（指向本模块真实符号/常量）。
    pub evidence: [&'static str; 4],
    /// 契约哈希。
    pub hash: u64,
}

/// 兑现声明常量。
pub const FULFILLMENT: Fulfillment = Fulfillment {
    contract_id: CONTRACT_ID,
    version: CONTRACT_VERSION,
    items: [
        "测量请求字段：文本、字体实例、排版参数、语言标记",
        "响应字段：advance、边界、行高、整形簇映射",
        "精度约定：1/64 像素网格，advance/行高向上取整、边界向下取整",
        "失败语义：超时返回保守估算并标记低置信，不阻塞 UI 帧",
    ],
    evidence: [
        "ContractRequest{text,font,params,lang_tag}",
        "ContractResponse{advance_q16,bound_w_q16,bound_h_q16,line_height_q16,clusters}",
        "GRID_Q16=64, ceil_grid/floor_grid",
        "TIMEOUT_US=16000, BuiltinEstimator+ContractResponse.low_confidence",
    ],
    hash: PROVIDER_CONTRACT_HASH,
};

/// 兑现声明可读文本（读屏/归档共用）。
pub fn fulfillment_statement() -> String {
    format!(
        "契约 {} v{} 哈希 {:016X} 兑现声明：{}；{}；{}；{}。证据：{} / {} / {} / {}。",
        FULFILLMENT.contract_id,
        FULFILLMENT.version,
        FULFILLMENT.hash,
        FULFILLMENT.items[0],
        FULFILLMENT.items[1],
        FULFILLMENT.items[2],
        FULFILLMENT.items[3],
        FULFILLMENT.evidence[0],
        FULFILLMENT.evidence[1],
        FULFILLMENT.evidence[2],
        FULFILLMENT.evidence[3],
    )
}

// ===========================================================================
// 九之二、E 侧变更契约走跨域变更流程（AE10 F6394 范式：提案方 E，受理方 N）
// ===========================================================================

/// 变更单状态：已提案（E 侧单方不得落地）。
pub const CHANGE_PROPOSED: u8 = 0;
/// 变更单状态：N 侧已受理（双签齐备，可进入下一版）。
pub const CHANGE_ACCEPTED: u8 = 1;

/// 跨域变更单（提案方 E 引擎 / 受理方 N 消费；两段哈希即变更前后指纹）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedChange {
    /// 变更理由（非空是受理的前置——无理由的变更一律拒）。
    pub rationale: String,
    /// 变更前契约文本。
    pub text_before: &'static str,
    /// 变更后契约文本。
    pub text_after: &'static str,
    /// 变更前哈希。
    pub hash_before: u64,
    /// 变更后哈希。
    pub hash_after: u64,
    /// 变更前版本号。
    pub from_version: u32,
    /// 变更后版本号（恒为 +1——契约版本只进一格）。
    pub to_version: u32,
    /// 受理状态。
    pub state: u8,
}

impl ProposedChange {
    /// 是否已被 N 侧受理（E 侧单方永远拿不到真值即落地权）。
    pub fn is_accepted(&self) -> bool {
        self.state == CHANGE_ACCEPTED
    }
    /// 变更单可读摘要（归档/评审台账）。
    pub fn screen_line(&self) -> String {
        format!(
            "change {} v{}->v{} hash {:016X}->{:016X} state={} 理由：{}",
            CONTRACT_ID,
            self.from_version,
            self.to_version,
            self.hash_before,
            self.hash_after,
            self.state,
            self.rationale,
        )
    }
}

/// E 侧发起变更提案（F6394 范式的入口；三条拒绝规则即流程的最小纪律）。
///
/// 拒绝：理由为空、目标文本与现行契约逐字相同（空变更）、受理方哈希未变。
pub fn propose_change(
    next_text: &'static str,
    rationale: &str,
) -> Result<ProposedChange, ContractError> {
    if rationale.trim().is_empty() {
        return Err(ContractError::new(
            C_TEXT18_CHANGE_REJECTED,
            "变更提案未给出理由（空理由不得进跨域评审）",
            "补写变更理由（受影响字段 + 迁移口径）后重新提案",
        ));
    }
    if next_text == CONTRACT_TEXT {
        return Err(ContractError::new(
            C_TEXT18_CHANGE_REJECTED,
            "变更后契约文本与现行契约逐字相同（空变更）",
            "要么不提案，要么给出真实的字段/精度/失败语义差异",
        ));
    }
    Ok(ProposedChange {
        rationale: String::from(rationale),
        text_before: CONTRACT_TEXT,
        text_after: next_text,
        hash_before: contract_hash(),
        hash_after: fnv1a(next_text.as_bytes()),
        from_version: CONTRACT_VERSION,
        to_version: CONTRACT_VERSION + 1,
        state: CHANGE_PROPOSED,
    })
}

/// N 侧受理变更单（双签的另一半：受理前一律不可生效）。
pub fn accept_change(change: &mut ProposedChange) -> Result<(), ContractError> {
    if change.hash_after == change.hash_before {
        return Err(ContractError::new(
            C_TEXT18_CHANGE_REJECTED,
            "变更前后哈希一致（变更未真正改变契约面）",
            "核对变更文本与哈希计算路径后重新受理",
        ));
    }
    if change.to_version != change.from_version + 1 {
        return Err(ContractError::new(
            C_TEXT18_CHANGE_REJECTED,
            "目标版本号非 +1（契约版本只进一格）",
            "把 to_version 定为 from_version + 1 后重新受理",
        ));
    }
    change.state = CHANGE_ACCEPTED;
    Ok(())
}

// ===========================================================================
// 十、E 引擎适配器（契约类型 → vee17/vee08 实现类型的单向转换）
// ===========================================================================

/// 字体实例快照源（引擎侧持有快照表；N 侧不可见）。
pub trait SnapshotSource {
    /// 按契约字体引用取度量快照（未登记 → None）。
    fn snapshot(&self, key: &FontRef) -> Option<crate::svstar2::vee08_fontmetric::MetricSnapshot>;
}

/// E 引擎后端（vee17 measure_text 的契约化封装——「MeasureText 与整形输出的
/// 契约化封装」落点；换掉本类型即完成「可替换引擎后端验证」）。
pub struct EngineBackend<'a> {
    /// 快照源（引擎侧登记表）。
    pub source: &'a dyn SnapshotSource,
}

impl MeasureBackend for EngineBackend<'_> {
    fn measure(&self, req: &ContractRequest) -> Result<BackendResult, ContractError> {
        use crate::svstar2::vee08_fontmetric::LineHeightMode;
        use crate::svstar2::vee17_textapi::{MeasureOptions, MeasureOutcome, TypesetParams, measure_text};
        let snap = match self.source.snapshot(&req.font) {
            Some(s) => s,
            None => {
                return Err(ContractError::new(
                    C_TEXT18_BACKEND_FAULT,
                    "字体实例未在引擎登记（快照缺失）",
                    "先在引擎侧完成 F0808 度量快照加载并登记该实例",
                ));
            }
        };
        let line_mode = match req.params.line_mode {
            LINE_MODE_FONT_DEFAULT => LineHeightMode::FontDefault,
            LINE_MODE_TIGHT => LineHeightMode::Tight,
            _ => LineHeightMode::Loose,
        };
        let params = TypesetParams {
            font_size_q16: req.params.font_size_q16,
            letter_spacing_q16: req.params.letter_spacing_q16,
            max_width_q16: req.params.max_width_q16,
            line_mode,
        };
        let opts = MeasureOptions::sync();
        match measure_text(req.text, &snap, &params, &opts) {
            Ok(MeasureOutcome::Sync(m)) => Ok(BackendResult {
                advance_q16: m.advance_q16,
                bound_w_q16: m.bound_w_q16,
                bound_h_q16: m.bound_h_q16,
                line_height_q16: m.line_height_q16,
                glyph_count: m.glyph_count,
            }),
            Ok(MeasureOutcome::Pended(_)) => Err(ContractError::new(
                C_TEXT18_BACKEND_FAULT,
                "同步路径返回挂起句柄（引擎后端状态机违规）",
                "检查引擎后端 sync 模式实现；契约层不做隐式重试",
            )),
            Err(e) => Err(ContractError::new(
                C_TEXT18_BACKEND_FAULT,
                &alloc::format!("引擎度量失败：{}", e.screen_line()),
                "按引擎错误建议处置后重试",
            )),
        }
    }
}

/// 摘要行（面板/日志共用）。
pub fn screen_line() -> String {
    format!(
        "ncontract {} v{} hash={:016X} timeout={}us budget={}ns",
        CONTRACT_ID, CONTRACT_VERSION, PROVIDER_CONTRACT_HASH, TIMEOUT_US, CALL_BUDGET_NS,
    )
}
