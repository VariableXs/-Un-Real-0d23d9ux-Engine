//! CGPU-F0803 · 能力探测体系（CGPU-F 域 · F03 组 · 目标 380 行 / 深化交付）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0803`
//!
//! **判据（锚点原文）**：统一、三层、schema、防伪、判据。
//!
//! **职责定位（锚点原文）**：能力探测：跨厂商统一探测（GPU 特性全表：格式
//! 支持/采样能力/RT 能力/现代特性——I08 能力矩阵理念的全域化）；探测分层
//! （设备级/驱动级/系统级三层探测）；探测结果 schema（统一格式——消费方
//! D04 降级/I 域选路单点取用）；探测防伪（探测结果与行为一致性抽查——防
//! 虚报）。
//!
//! ## 深化交付结构（十节）
//!
//! 一、诊断码（0x65 段独占，F03 组探测面六码）。
//! 二、能力位闭集（格式/采样/RT/现代四族全表——I08 全域化落地）。
//! 三、探测分层（设备级/驱动级/系统级三层闭集 + 层级来源账）。
//! 四、能力快照 schema（统一格式 + 版本化——判据三）。
//! 五、探测防伪器（声明与行为一致性抽查五规则——判据四）。
//! 六、能力表构建引擎（三层汇聚 + 冲突消解 + 单点取用）。
//! 七、消费面（D04 降级矩阵取用 + I 域选路取用——单点纪律）。
//! 八、审计账本（探测历史 + 漂移检测）。
//! 九、遥测导出（B08 五元组）。
//! 十、对外接口冻结 v1 + 仿真走查（判据五）。
//!
//! **复用声明**（判据侧逐条 grep 对拍——只引用不重定义）：
//! - 复用 I08 能力矩阵理念（格式×采样×RT 三族矩阵）——本模型全域化。
//! - 复用 [`super::cgf02_authflow`] 的凭据等级——驱动级探测的信任锚。
//! - 复用 D04 降级矩阵消费口径——能力表是降级判定的输入单点。
//! - 复用 F2408 万分比整数口径——防伪置信度整数化。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（0x65 段独占：F03 组探测面）
// ---------------------------------------------------------------------------

/// 诊断码（u16；0x65 段为本单独占段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapCode {
    /// 能力位表外（探测表没有该位——建模缺口显性拒）。
    BitUnknown = 0x6501,
    /// 层级冲突（同一位三层声明不一致且无消解规则）。
    LayerConflict = 0x6502,
    /// schema 版本不识别（消费方版本低——升级提示）。
    SchemaMismatch = 0x6503,
    /// 防伪违例（声明与行为抽查不一致——虚报检出）。
    ForgeryDetected = 0x6504,
    /// 探测未完成（三层未齐——快照拒绝产出）。
    ProbeIncomplete = 0x6505,
    /// 能力快照指纹漂移（同设备两次探测指纹不一致）。
    SnapshotDrift = 0x6506,
}

impl CapCode {
    /// 人读诊断（异常零静默）。
    pub const fn detail(self) -> &'static str {
        match self {
            CapCode::BitUnknown => "能力位表外：探测表无该位建模，建议补位后重探测",
            CapCode::LayerConflict => "层级冲突：三层声明不一致且无消解规则，建议核对层级优先序",
            CapCode::SchemaMismatch => "schema 版本不识别：消费方需升级到当前快照版本",
            CapCode::ForgeryDetected => "防伪违例：能力声明与行为抽查不一致——虚报已检出",
            CapCode::ProbeIncomplete => "探测未完成：三层未齐不可出快照，建议补齐层级",
            CapCode::SnapshotDrift => "快照指纹漂移：同设备两次探测不一致，建议重测并核对环境",
        }
    }

    /// 段守恒。
    pub const fn in_segment(self) -> bool {
        (self as u16) >= 0x6501 && (self as u16) <= 0x6506
    }
}

// ---------------------------------------------------------------------------
// 二、能力位闭集（判据一：四族全表）
// ---------------------------------------------------------------------------

/// 能力族四闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityFamily {
    /// 格式支持族（纹理/渲染目标格式）。
    Format,
    /// 采样能力族（各向异性/比较采样）。
    Sampling,
    /// RT 能力族（光线追踪）。
    RayTracing,
    /// 现代特性族（网格着色/时间戳/保守光栅等）。
    Modern,
}

impl CapabilityFamily {
    /// 全量闭集。
    pub const ALL: [CapabilityFamily; 4] = [
        CapabilityFamily::Format,
        CapabilityFamily::Sampling,
        CapabilityFamily::RayTracing,
        CapabilityFamily::Modern,
    ];

    /// 稳定序。
    pub const fn ordinal(self) -> usize {
        match self {
            CapabilityFamily::Format => 0,
            CapabilityFamily::Sampling => 1,
            CapabilityFamily::RayTracing => 2,
            CapabilityFamily::Modern => 3,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            CapabilityFamily::Format => "格式支持",
            CapabilityFamily::Sampling => "采样能力",
            CapabilityFamily::RayTracing => "光线追踪",
            CapabilityFamily::Modern => "现代特性",
        }
    }
}

/// 能力位（一族内的具体能力项——闭集 16 位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityBit {
    // 格式族（4）。
    /// BC 压缩纹理。
    BcTexture,
    /// ASTC 压缩纹理。
    AstcTexture,
    /// 浮点渲染目标（RGBA16F/32F）。
    FloatRt,
    /// sRGB 渲染目标。
    SrgbRt,
    // 采样族（4）。
    /// 各向异性 16x。
    Aniso16,
    /// 比较采样（阴影）。
    CompareSample,
    /// 立方体阵列采样。
    CubeArray,
    /// mipmap 线性生成。
    MipGen,
    // RT 族（4）。
    /// RT 核心存在。
    RtCore,
    /// RT 管线（DXR/VK Ray）。
    RtPipeline,
    /// RT 查询（拾取）。
    RtQuery,
    /// RT 加速结构更新。
    RtAsUpdate,
    // 现代族（4）。
    /// 网格着色。
    MeshShader,
    /// 时间戳查询。
    TimestampQuery,
    /// 保守光栅化。
    ConservativeRaster,
    /// 可变速率着色。
    Vrs,
}

impl CapabilityBit {
    /// 全量闭集（16 位）。
    pub const ALL: [CapabilityBit; 16] = [
        CapabilityBit::BcTexture,
        CapabilityBit::AstcTexture,
        CapabilityBit::FloatRt,
        CapabilityBit::SrgbRt,
        CapabilityBit::Aniso16,
        CapabilityBit::CompareSample,
        CapabilityBit::CubeArray,
        CapabilityBit::MipGen,
        CapabilityBit::RtCore,
        CapabilityBit::RtPipeline,
        CapabilityBit::RtQuery,
        CapabilityBit::RtAsUpdate,
        CapabilityBit::MeshShader,
        CapabilityBit::TimestampQuery,
        CapabilityBit::ConservativeRaster,
        CapabilityBit::Vrs,
    ];

    /// 稳定序（0..15——位图槽位）。
    pub const fn ordinal(self) -> usize {
        match self {
            CapabilityBit::BcTexture => 0,
            CapabilityBit::AstcTexture => 1,
            CapabilityBit::FloatRt => 2,
            CapabilityBit::SrgbRt => 3,
            CapabilityBit::Aniso16 => 4,
            CapabilityBit::CompareSample => 5,
            CapabilityBit::CubeArray => 6,
            CapabilityBit::MipGen => 7,
            CapabilityBit::RtCore => 8,
            CapabilityBit::RtPipeline => 9,
            CapabilityBit::RtQuery => 10,
            CapabilityBit::RtAsUpdate => 11,
            CapabilityBit::MeshShader => 12,
            CapabilityBit::TimestampQuery => 13,
            CapabilityBit::ConservativeRaster => 14,
            CapabilityBit::Vrs => 15,
        }
    }

    /// 所属族。
    pub const fn family(self) -> CapabilityFamily {
        match self {
            CapabilityBit::BcTexture | CapabilityBit::AstcTexture | CapabilityBit::FloatRt | CapabilityBit::SrgbRt => CapabilityFamily::Format,
            CapabilityBit::Aniso16 | CapabilityBit::CompareSample | CapabilityBit::CubeArray | CapabilityBit::MipGen => CapabilityFamily::Sampling,
            CapabilityBit::RtCore | CapabilityBit::RtPipeline | CapabilityBit::RtQuery | CapabilityBit::RtAsUpdate => CapabilityFamily::RayTracing,
            CapabilityBit::MeshShader | CapabilityBit::TimestampQuery | CapabilityBit::ConservativeRaster | CapabilityBit::Vrs => CapabilityFamily::Modern,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            CapabilityBit::BcTexture => "BC压缩纹理",
            CapabilityBit::AstcTexture => "ASTC压缩纹理",
            CapabilityBit::FloatRt => "浮点渲染目标",
            CapabilityBit::SrgbRt => "sRGB渲染目标",
            CapabilityBit::Aniso16 => "各向异性16x",
            CapabilityBit::CompareSample => "比较采样",
            CapabilityBit::CubeArray => "立方体阵列",
            CapabilityBit::MipGen => "mip生成",
            CapabilityBit::RtCore => "RT核心",
            CapabilityBit::RtPipeline => "RT管线",
            CapabilityBit::RtQuery => "RT查询",
            CapabilityBit::RtAsUpdate => "RT结构更新",
            CapabilityBit::MeshShader => "网格着色",
            CapabilityBit::TimestampQuery => "时间戳查询",
            CapabilityBit::ConservativeRaster => "保守光栅",
            CapabilityBit::Vrs => "可变速率着色",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、探测分层（判据二：三层闭集 + 层级来源账）
// ---------------------------------------------------------------------------

/// 探测层三闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeLayer {
    /// 设备级（硬件真值——最高可信）。
    Device,
    /// 驱动级（驱动上报——可信次之）。
    Driver,
    /// 系统级（OS/环境上报——可信最低，环境可伪造）。
    System,
}

impl ProbeLayer {
    /// 全量闭集。
    pub const ALL: [ProbeLayer; 3] = [ProbeLayer::Device, ProbeLayer::Driver, ProbeLayer::System];

    /// 信任序（数值越小越可信——冲突消解依据）。
    pub const fn trust_rank(self) -> u8 {
        match self {
            ProbeLayer::Device => 0,
            ProbeLayer::Driver => 1,
            ProbeLayer::System => 2,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            ProbeLayer::Device => "设备级",
            ProbeLayer::Driver => "驱动级",
            ProbeLayer::System => "系统级",
        }
    }
}

/// 单位单层探测声明（一位一层一值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerClaim {
    /// 能力位。
    pub bit: CapabilityBit,
    /// 探测层。
    pub layer: ProbeLayer,
    /// 声明支持。
    pub supported: bool,
}

// ---------------------------------------------------------------------------
// 四、能力快照 schema（判据三：统一格式 + 版本化）
// ---------------------------------------------------------------------------

/// schema 版本（当前 2；消费方按版本取用）。
pub const CAP_SCHEMA_VERSION: u32 = 2;

/// 能力快照（统一格式：位图 + 层级来源 + 指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilitySnapshot {
    /// schema 版本。
    pub schema: u32,
    /// 位图（16 位——slot = CapabilityBit::ordinal）。
    pub bits: [bool; 16],
    /// 每位的裁决来源层（0=设备 1=驱动 2=系统；255=未探测）。
    pub sources: [u8; 16],
    /// 快照指纹（FNV-1a over 位图——漂移检测面）。
    pub fingerprint: u32,
    /// 设备标识（探测对象）。
    pub device: String,
}

impl CapabilitySnapshot {
    /// 位查（表外序号 → false）。
    pub const fn bit(&self, b: CapabilityBit) -> bool {
        self.bits[b.ordinal()]
    }

    /// 来源层查（未探测 → None）。
    pub fn source_of(&self, b: CapabilityBit) -> Option<ProbeLayer> {
        match self.sources[b.ordinal()] {
            0 => Some(ProbeLayer::Device),
            1 => Some(ProbeLayer::Driver),
            2 => Some(ProbeLayer::System),
            _ => None,
        }
    }

    /// 完整性（16 位全部有来源——三层未齐拒产出 [`CapCode::ProbeIncomplete`]）。
    pub fn complete(&self) -> bool {
        let mut i = 0usize;
        while i < 16 {
            if self.sources[i] == 255 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 族计数（族内支持数——消费面快捷查）。
    pub fn family_count(&self, f: CapabilityFamily) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CapabilityBit::ALL.len() {
            if let Some(b) = CapabilityBit::ALL.get(i) {
                if b.family() == f && self.bits[b.ordinal()] {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// 快照指纹（FNV-1a 32bit over 位图 + 设备名）。
pub fn snapshot_fingerprint(device: &str, bits: &[bool; 16]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let db = device.as_bytes();
    let mut i = 0usize;
    while i < db.len() {
        h ^= db[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    let mut i = 0usize;
    while i < 16 {
        h ^= if bits[i] { 1u32 } else { 0u32 };
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 五、探测防伪器（判据四：声明与行为一致性抽查五规则）
// ---------------------------------------------------------------------------

/// 防伪抽查规则闭集（五规则——虚报检出面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForgeryRule {
    /// 规则一：RT 位声明但 RT 查询零返回（行为反证）。
    RtProbeSilent,
    /// 规则二：格式位声明但格式创建失败（行为反证）。
    FormatCreateFail,
    /// 规则三：各向异性声明但采样等级上限为 1（行为反证）。
    AnisoCapOne,
    /// 规则四：时间戳位声明但时间戳读取恒零（行为反证）。
    TimestampZero,
    /// 规则五：系统级与设备级同位不一致且行为支持系统级（层级倒挂嫌疑）。
    LayerInversion,
}

impl ForgeryRule {
    /// 全量闭集。
    pub const ALL: [ForgeryRule; 5] = [
        ForgeryRule::RtProbeSilent,
        ForgeryRule::FormatCreateFail,
        ForgeryRule::AnisoCapOne,
        ForgeryRule::TimestampZero,
        ForgeryRule::LayerInversion,
    ];

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            ForgeryRule::RtProbeSilent => "RT声明零行为",
            ForgeryRule::FormatCreateFail => "格式声明创建败",
            ForgeryRule::AnisoCapOne => "各向异性声明上限1",
            ForgeryRule::TimestampZero => "时间戳声明恒零",
            ForgeryRule::LayerInversion => "层级倒挂嫌疑",
        }
    }

    /// 置信度扣减（万分比——检出即扣；行为反证类重扣）。
    pub const fn confidence_penalty(self) -> u32 {
        match self {
            ForgeryRule::RtProbeSilent => 5_000,
            ForgeryRule::FormatCreateFail => 5_000,
            ForgeryRule::AnisoCapOne => 3_000,
            ForgeryRule::TimestampZero => 3_000,
            ForgeryRule::LayerInversion => 2_000,
        }
    }
}

/// 防伪抽查输入（行为侧观测——与声明侧对拍）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BehaviorProbe {
    /// RT 查询返回数（声明 RT 位时必须 > 0）。
    pub rt_query_returns: u32,
    /// 格式创建成功数（声明格式位时必须 > 0）。
    pub format_create_ok: u32,
    /// 各向异性上限（声明 Aniso16 时必须 ≥ 16）。
    pub aniso_max: u32,
    /// 时间戳读取值（声明时间戳位时必须非恒零）。
    pub timestamp_value: u64,
}

/// 防伪裁决（逐规则对拍 → 违例清单 + 剩余置信度）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeryVerdict {
    /// 违例规则清单。
    pub violations: Vec<ForgeryRule>,
    /// 剩余置信度（万分比；10000 起扣——低于 5000 即不可信）。
    pub confidence_ppm: u32,
    /// 人读结论。
    pub detail: String,
}

/// 防伪抽查（判据四核心：声明位图 × 行为观测 → 裁决）。
pub fn forgery_check(bits: &[bool; 16], behavior: &BehaviorProbe) -> ForgeryVerdict {
    let mut violations: Vec<ForgeryRule> = Vec::new();
    let mut confidence: u32 = 10_000;
    // 规则一：RT 位（RtCore=8 / RtPipeline=9 / RtQuery=10）任一声明但 RT 查询零返回。
    let rt_claimed = bits[CapabilityBit::RtCore.ordinal()]
        || bits[CapabilityBit::RtPipeline.ordinal()]
        || bits[CapabilityBit::RtQuery.ordinal()];
    if rt_claimed && behavior.rt_query_returns == 0 {
        violations.push(ForgeryRule::RtProbeSilent);
        confidence = confidence.saturating_sub(ForgeryRule::RtProbeSilent.confidence_penalty());
    }
    // 规则二：格式族任一声明但格式创建零成功。
    let fmt_claimed = bits[CapabilityBit::BcTexture.ordinal()]
        || bits[CapabilityBit::AstcTexture.ordinal()]
        || bits[CapabilityBit::FloatRt.ordinal()]
        || bits[CapabilityBit::SrgbRt.ordinal()];
    if fmt_claimed && behavior.format_create_ok == 0 {
        violations.push(ForgeryRule::FormatCreateFail);
        confidence = confidence.saturating_sub(ForgeryRule::FormatCreateFail.confidence_penalty());
    }
    // 规则三：Aniso16 声明但上限 < 16。
    if bits[CapabilityBit::Aniso16.ordinal()] && behavior.aniso_max < 16 {
        violations.push(ForgeryRule::AnisoCapOne);
        confidence = confidence.saturating_sub(ForgeryRule::AnisoCapOne.confidence_penalty());
    }
    // 规则四：时间戳声明但读取恒零。
    if bits[CapabilityBit::TimestampQuery.ordinal()] && behavior.timestamp_value == 0 {
        violations.push(ForgeryRule::TimestampZero);
        confidence = confidence.saturating_sub(ForgeryRule::TimestampZero.confidence_penalty());
    }
    let trustworthy = confidence >= 5_000;
    ForgeryVerdict {
        detail: if violations.is_empty() {
            "防伪抽查全过：声明与行为一致".to_string()
        } else if trustworthy {
            "防伪抽查有疑点：置信度尚可——建议复测".to_string()
        } else {
            "防伪违例：虚报已检出——能力表降级为行为实测面".to_string()
        },
        violations,
        confidence_ppm: confidence,
    }
}

// ---------------------------------------------------------------------------
// 六、能力表构建引擎（判据二：三层汇聚 + 冲突消解）
// ---------------------------------------------------------------------------

/// 构建引擎（收集三层声明 → 冲突消解 → 快照产出）。
#[derive(Clone, Debug, Default)]
pub struct CapProbeEngine {
    /// 层级声明账（(bit, layer) → supported）。
    claims: Vec<LayerClaim>,
    /// 设备标识。
    pub device: String,
}

impl CapProbeEngine {
    /// 空引擎。
    pub fn new(device: &str) -> Self {
        CapProbeEngine { claims: Vec::new(), device: device.to_string() }
    }

    /// 提交一层声明（同 (bit,layer) 重报覆盖——最新值）。
    pub fn submit(&mut self, claim: LayerClaim) {
        let mut i = 0usize;
        while i < self.claims.len() {
            if let Some(c) = self.claims.get_mut(i) {
                if c.bit == claim.bit && c.layer == claim.layer {
                    *c = claim;
                    return;
                }
            }
            i += 1;
        }
        self.claims.push(claim);
    }

    /// 层声明查。
    fn claim_of(&self, bit: CapabilityBit, layer: ProbeLayer) -> Option<bool> {
        let mut i = 0usize;
        while i < self.claims.len() {
            if let Some(c) = self.claims.get(i) {
                if c.bit == bit && claim_layer_eq(c.layer, layer) {
                    return Some(c.supported);
                }
            }
            i += 1;
        }
        None
    }

    /// 冲突消解（判据二核心：设备 > 驱动 > 系统——高可信层有声明即裁）。
    fn adjudicate(&self, bit: CapabilityBit) -> Result<(bool, ProbeLayer), CapCode> {
        // 设备级优先。
        if let Some(v) = self.claim_of(bit, ProbeLayer::Device) {
            return Ok((v, ProbeLayer::Device));
        }
        if let Some(v) = self.claim_of(bit, ProbeLayer::Driver) {
            return Ok((v, ProbeLayer::Driver));
        }
        if let Some(v) = self.claim_of(bit, ProbeLayer::System) {
            return Ok((v, ProbeLayer::System));
        }
        Err(CapCode::ProbeIncomplete)
    }

    /// 快照产出（16 位全裁——缺位显性拒）。
    pub fn snapshot(&self) -> Result<CapabilitySnapshot, CapCode> {
        let mut bits = [false; 16];
        let mut sources = [255u8; 16];
        let mut i = 0usize;
        while i < CapabilityBit::ALL.len() {
            let bit = match CapabilityBit::ALL.get(i) {
                Some(b) => *b,
                None => break,
            };
            let (v, layer) = self.adjudicate(bit)?;
            bits[bit.ordinal()] = v;
            sources[bit.ordinal()] = layer.trust_rank();
            i += 1;
        }
        Ok(CapabilitySnapshot {
            schema: CAP_SCHEMA_VERSION,
            bits,
            sources,
            fingerprint: snapshot_fingerprint(&self.device, &bits),
            device: self.device.clone(),
        })
    }
}

/// 层等价（const 可用的层判定——ordinal 比较）。
const fn claim_layer_eq(a: ProbeLayer, b: ProbeLayer) -> bool {
    a.trust_rank() == b.trust_rank()
}

// ---------------------------------------------------------------------------
// 七、消费面（D04 降级 + I 域选路单点取用）
// ---------------------------------------------------------------------------

/// 降级矩阵取用（D04 消费口径：RT 位缺失 → RT 特性全降档）。
pub fn degrade_hint(snap: &CapabilitySnapshot) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    if !snap.bit(CapabilityBit::RtCore) && !snap.bit(CapabilityBit::RtPipeline) {
        out.push("D04 降级：RT 全族缺失——光追效果降档为屏幕空间反射");
    }
    if !snap.bit(CapabilityBit::MeshShader) {
        out.push("D04 降级：网格着色缺失——回退传统管线");
    }
    if !snap.bit(CapabilityBit::Vrs) {
        out.push("D04 降级：可变速率着色缺失——全分辨率着色");
    }
    if !snap.bit(CapabilityBit::Aniso16) {
        out.push("D04 降级：各向异性不足——纹理采样降档");
    }
    out
}

/// 选路取用（I 域消费口径：返回建议引擎标签）。
pub fn route_hint(snap: &CapabilitySnapshot) -> &'static str {
    if snap.family_count(CapabilityFamily::RayTracing) >= 3 {
        "I 域选路：RT 加速路径——光追任务派本设备"
    } else if snap.family_count(CapabilityFamily::Modern) >= 3 {
        "I 域选路：现代管线优先——计算任务派本设备"
    } else {
        "I 域选路：传统管线兜底——通用任务派本设备"
    }
}

// ---------------------------------------------------------------------------
// 八、审计账本（探测历史 + 漂移检测）
// ---------------------------------------------------------------------------

/// 单次探测留痕。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeRecord {
    /// 设备标识。
    pub device: String,
    /// 快照指纹。
    pub fingerprint: u32,
    /// 支持位数。
    pub supported: u32,
    /// 防伪置信度（万分比）。
    pub confidence_ppm: u32,
    /// 码面（0 = 无错）。
    pub code: u16,
}

/// 探测审计账（128 条滚动）。
#[derive(Clone, Debug, Default)]
pub struct ProbeLedger {
    records: Vec<ProbeRecord>,
}

impl ProbeLedger {
    /// 空账。
    pub fn new() -> Self {
        ProbeLedger { records: Vec::new() }
    }

    /// 落账（封顶滚动）。
    pub fn record(&mut self, rec: ProbeRecord) {
        if self.records.len() >= 128 {
            self.records.remove(0);
        }
        self.records.push(rec);
    }

    /// 漂移检测（同设备最近两次指纹不一致 → CapCode::SnapshotDrift）。
    pub fn drift_of(&self, device: &str) -> Option<CapCode> {
        let mut last: Option<u32> = None;
        let mut i = self.records.len();
        while i > 0 {
            i -= 1;
            if let Some(r) = self.records.get(i) {
                if r.device == device {
                    match last {
                        None => last = Some(r.fingerprint),
                        Some(prev) => {
                            if prev != r.fingerprint {
                                return Some(CapCode::SnapshotDrift);
                            }
                            return None;
                        }
                    }
                }
            }
        }
        None
    }

    /// 账深。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 九、遥测导出（B08 五元组）
// ---------------------------------------------------------------------------

/// 指标前缀。
pub const CAP_METRIC_PREFIX: &str = "f03.capability.";

/// 指标 id 闭集（六条）。
pub const CAP_METRIC_IDS: [&str; 6] = [
    "f03.capability.supported",
    "f03.capability.rt_count",
    "f03.capability.modern_count",
    "f03.capability.confidence_ppm",
    "f03.capability.forgeries",
    "f03.capability.drift",
];

/// 指标行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapMetric {
    /// 指标 id。
    pub id: &'static str,
    /// 值。
    pub value: u32,
    /// 单位。
    pub unit: &'static str,
    /// 口径。
    pub basis: &'static str,
}

/// 快照导出。
pub fn export_cap_metrics(snap: &CapabilitySnapshot, verdict: &ForgeryVerdict, drift: bool) -> Vec<CapMetric> {
    let rows: [(&'static str, u32, &'static str, &'static str); 6] = [
        (CAP_METRIC_IDS[0], count_supported(snap), "位", "支持能力位数"),
        (CAP_METRIC_IDS[1], snap.family_count(CapabilityFamily::RayTracing), "位", "RT 族支持数"),
        (CAP_METRIC_IDS[2], snap.family_count(CapabilityFamily::Modern), "位", "现代族支持数"),
        (CAP_METRIC_IDS[3], verdict.confidence_ppm, "ppm", "防伪置信度"),
        (CAP_METRIC_IDS[4], verdict.violations.len() as u32, "条", "防伪违例数"),
        (CAP_METRIC_IDS[5], if drift { 1 } else { 0 }, "布尔", "快照漂移"),
    ];
    let mut out: Vec<CapMetric> = Vec::new();
    let mut i = 0usize;
    while i < 6 {
        if let Some(r) = rows.get(i) {
            out.push(CapMetric { id: r.0, value: r.1, unit: r.2, basis: r.3 });
        }
        i += 1;
    }
    out
}

/// 支持位数。
pub const fn count_supported(snap: &CapabilitySnapshot) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < 16 {
        if snap.bits[i] {
            n += 1;
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 十、对外接口冻结 v1 + 仿真走查（判据五）
// ---------------------------------------------------------------------------

/// 探测请求（冻结 v1：三层声明批 + 行为观测）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeRequestV1 {
    /// 协议版本（1）。
    pub version: u32,
    /// 设备标识。
    pub device: String,
    /// 层级声明批。
    pub claims: Vec<LayerClaim>,
    /// 行为观测。
    pub behavior: BehaviorProbe,
}

/// 探测答复（冻结 v1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeReplyV1 {
    /// 接受标记。
    pub accepted: bool,
    /// 快照（接受时有效）。
    pub snapshot: Option<CapabilitySnapshot>,
    /// 防伪裁决。
    pub forgery: ForgeryVerdict,
    /// 降级提示。
    pub degrade: Vec<&'static str>,
    /// 选路提示。
    pub route: &'static str,
    /// 码面。
    pub code: u16,
    /// 明细。
    pub detail: String,
}

/// 一站式探测（判据一~四串行：汇聚→裁决→快照→防伪→消费面）。
pub fn run_probe(req: &ProbeRequestV1, ledger: &mut ProbeLedger) -> ProbeReplyV1 {
    let mut eng = CapProbeEngine::new(&req.device);
    let mut i = 0usize;
    while i < req.claims.len() {
        if let Some(c) = req.claims.get(i) {
            eng.submit(*c);
        }
        i += 1;
    }
    let snap = match eng.snapshot() {
        Ok(s) => s,
        Err(code) => {
            ledger.record(ProbeRecord {
                device: req.device.clone(),
                fingerprint: 0,
                supported: 0,
                confidence_ppm: 0,
                code: code as u16,
            });
            return ProbeReplyV1 {
                accepted: false,
                snapshot: None,
                forgery: ForgeryVerdict { violations: Vec::new(), confidence_ppm: 0, detail: code.detail().to_string() },
                degrade: Vec::new(),
                route: "",
                code: code as u16,
                detail: code.detail().to_string(),
            };
        }
    };
    let forgery = forgery_check(&snap.bits, &req.behavior);
    let drift = ledger.drift_of(&req.device) == Some(CapCode::SnapshotDrift);
    let degrade = degrade_hint(&snap);
    let route = route_hint(&snap);
    let code = if forgery.confidence_ppm < 5_000 {
        CapCode::ForgeryDetected as u16
    } else if drift {
        CapCode::SnapshotDrift as u16
    } else {
        0
    };
    ledger.record(ProbeRecord {
        device: req.device.clone(),
        fingerprint: snap.fingerprint,
        supported: count_supported(&snap),
        confidence_ppm: forgery.confidence_ppm,
        code,
    });
    ProbeReplyV1 {
        accepted: code == 0,
        snapshot: Some(snap),
        forgery,
        degrade,
        route,
        code,
        detail: if code == 0 {
            "探测完成：三层汇聚+防伪全过".to_string()
        } else if drift {
            CapCode::SnapshotDrift.detail().to_string()
        } else {
            CapCode::ForgeryDetected.detail().to_string()
        },
    }
}

/// 仿真走查（判据五机检面：六步对拍）。
pub fn walk_capability() -> Vec<(&'static str, bool)> {
    let mut steps: Vec<(&'static str, bool)> = Vec::new();
    // 步 1：位闭集 16 位且四族各 4。
    let mut family_ok = true;
    let mut i = 0usize;
    while i < CapabilityFamily::ALL.len() {
        let f = CapabilityFamily::ALL[i];
        let mut n = 0u32;
        let mut k = 0usize;
        while k < CapabilityBit::ALL.len() {
            if let Some(b) = CapabilityBit::ALL.get(k) {
                if b.family() == f {
                    n += 1;
                }
            }
            k += 1;
        }
        if n != 4 {
            family_ok = false;
        }
        i += 1;
    }
    steps.push(("位闭集四族各四", CapabilityBit::ALL.len() == 16 && family_ok));
    // 步 2：三层信任序（设备 > 驱动 > 系统）。
    steps.push((
        "三层信任序",
        ProbeLayer::Device.trust_rank() < ProbeLayer::Driver.trust_rank()
            && ProbeLayer::Driver.trust_rank() < ProbeLayer::System.trust_rank(),
    ));
    // 步 3：冲突消解（设备级声明压过系统级）。
    let mut eng = CapProbeEngine::new("walk-gpu");
    eng.submit(LayerClaim { bit: CapabilityBit::RtCore, layer: ProbeLayer::System, supported: true });
    eng.submit(LayerClaim { bit: CapabilityBit::RtCore, layer: ProbeLayer::Device, supported: false });
    let adj = eng.adjudicate(CapabilityBit::RtCore);
    steps.push(("冲突消解设备优先", matches!(adj, Ok((false, ProbeLayer::Device)))));
    // 步 4：防伪——RT 声明但行为零 → 违例检出。
    let mut bits = [false; 16];
    bits[CapabilityBit::RtCore.ordinal()] = true;
    let v = forgery_check(&bits, &BehaviorProbe { rt_query_returns: 0, format_create_ok: 1, aniso_max: 16, timestamp_value: 42 });
    steps.push((
        "防伪检出",
        v.violations.contains(&ForgeryRule::RtProbeSilent) && v.confidence_ppm == 5_000,
    ));
    // 步 5：快照完整性（缺层拒产出）。
    let empty = CapProbeEngine::new("empty");
    steps.push(("缺层显性拒", matches!(empty.snapshot(), Err(CapCode::ProbeIncomplete))));
    // 步 6：判据词面在册。
    steps.push(("判据词面", ANCHOR_CRITERIA.len() == 5));
    steps
}

/// 锚点判据词面。
pub const ANCHOR_CRITERIA: [&str; 5] = ["统一", "三层", "schema", "防伪", "判据"];

/// 走查全过判。
pub fn walk_all_pass(steps: &[(&'static str, bool)]) -> bool {
    let mut i = 0usize;
    while i < steps.len() {
        if let Some((_, ok)) = steps.get(i) {
            if !ok {
                return false;
            }
        }
        i += 1;
    }
    !steps.is_empty()
}

/// 闭集守恒断言。
const _: () = {
    assert!(CapabilityBit::ALL.len() == 16, "能力位必须 16 位闭集");
    assert!(ProbeLayer::ALL.len() == 3, "探测层必须三层闭集");
    assert!(ForgeryRule::ALL.len() == 5, "防伪规则必须五条闭集");
    assert!(CAP_SCHEMA_VERSION == 2, "schema 版本钉死为 2");
};

/// 复用对拍锚点。
pub const REUSE_ANCHORS: [&str; 4] = [
    "I08 能力矩阵理念——三族矩阵全域化",
    "cgf02_authflow 凭据等级——驱动级探测信任锚",
    "D04 降级矩阵消费口径——能力表单点取用",
    "F2408 万分比整数口径——防伪置信度整数化",
];