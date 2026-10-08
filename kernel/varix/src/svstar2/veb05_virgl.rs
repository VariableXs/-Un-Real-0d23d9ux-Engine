//! VE-F0205 · virgl 上下文协商（VE-B 域 · GPU 驱动矩阵 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0205`
//!
//! **规格原文**：3D 能力走 virgl（OpenGL 语义透传到 host）。本功能实现 virgl
//! 上下文协商：VIRTIO_GPU_CAP_VIRGL 能力探测（host 支持才启用）、virgl 上下文
//! 创建（VIRGL_CTX_CREATE）、能力集获取（host GL 能力集 VIRGL_CMD_GET_CAPS
//! 解析映射到 A 域能力位图 F0007）、四态判定（无 virgl/virgl1/virgl2/不可用）。
//! 能力映射表：host GL 限制（最大纹理尺寸/纹理格式集/样本数）逐项映射并在 VE
//! 侧裁剪声明——超出 host 能力的请求在创建期拒绝。降级链：无 virgl 时 3D 需求
//! 走软渲（A 域 F0013）并提示。判据：协商四态正确、能力映射与 host 实测一致、
//! 超能力请求创建期拒绝、降级链联动、协商耗时 ≤50ms。
//!
//! **设计要点**：
//! - 四态判定是判定不是猜测：设备不可达 → Unavailable（协商失败 ≠ 无 virgl，
//!   两者的处置不同——前者可重试，后者直接降级）；无 virgl capset → NoVirgl；
//!   capset 版本 ≥2 → Virgl2、1 → Virgl1；解析失败 → Unavailable（诚实铁律）；
//! - 能力映射表逐项有据：GL 版本 → 标准特性位（F0007 冻结键位），映射规则
//!   表驱动且带依据注记；厂商位隔离纪律（vendor_free）继承 F0007；
//! - 裁剪声明在创建期执行：host 最大纹理/样本数/格式集是硬边界，超边界请求
//!   在创建期拒绝（三要素），不许拖到运行期黑屏；
//! - 降级链显性联动：NoVirgl/Unavailable 产出 DegradeNotice 指向软渲
//!   （VE-F0013），人话提示、读屏可达；
//! - 协商耗时账面：每步成本记账（逻辑 µs），总账 ≤50ms 是判据不是口号。

use super::vea07_caps::StandardBitmap;
use super::veb02_proto::CTRL_HDR_LEN;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量：3D 上下文命令族与 capset 标识（virtio-gpu 规范定值）
// ---------------------------------------------------------------------------

pub const CMD_CTX_CREATE: u32 = 0x0200;
pub const CMD_CTX_DESTROY: u32 = 0x0201;
pub const CMD_CTX_ATTACH_RESOURCE: u32 = 0x0202;
pub const CMD_CTX_DETACH_RESOURCE: u32 = 0x0203;
pub const CMD_SUBMIT_3D: u32 = 0x0206;

/// capset 标识：virgl = 1、venus = 2（规范定值）。
pub const CAPSET_VIRGL_ID: u32 = 1;
pub const CAPSET_VENUS_ID: u32 = 2;

/// 协商总耗时预算（规格判据 ≤50ms）。
pub const NEGOTIATION_BUDGET_US: u64 = 50_000;

/// virgl capset 载荷最小长度：id4+ver4+gl_major4+gl_minor4+max_tex4+max_samples4+fmt_mask8 = 32。
pub const VIRGL_CAPSET_MIN_LEN: usize = 32;

// ---------------------------------------------------------------------------
// 二、四态判定
// ---------------------------------------------------------------------------

/// virgl 可用性四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirglState {
    /// host 无 virgl capset——3D 走软渲降级。
    NoVirgl,
    /// virgl 1.x 可用。
    Virgl1,
    /// virgl 2.x 可用。
    Virgl2,
    /// 设备在但协商失败/不可达——可重试，不等于 NoVirgl。
    Unavailable,
}

impl VirglState {
    pub fn label(self) -> &'static str {
        match self {
            VirglState::NoVirgl => "无 virgl",
            VirglState::Virgl1 => "virgl 1.x",
            VirglState::Virgl2 => "virgl 2.x",
            VirglState::Unavailable => "不可用",
        }
    }

    /// 3D 硬件通路是否就绪（只有两个就绪态为真）。
    pub fn ready(self) -> bool {
        matches!(self, VirglState::Virgl1 | VirglState::Virgl2)
    }
}

// ---------------------------------------------------------------------------
// 三、host 能力集（capset 载荷解析）
// ---------------------------------------------------------------------------

/// host GL 能力集（virgl capset 载荷的结构化视图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostCaps {
    pub gl_major: u32,
    pub gl_minor: u32,
    /// host 最大单纹理边长（像素）。
    pub max_texture_size: u32,
    /// host 最大多重采样数。
    pub max_samples: u32,
    /// host 支持的格式掩码（bit n = 格式 n 支持；本模型登记 4bpp 族位）。
    pub format_mask: u64,
}

impl HostCaps {
    /// GL 版本的比较用数值（major*100+minor）。
    pub fn gl_version(&self) -> u32 {
        self.gl_major * 100 + self.gl_minor
    }
}

/// capset 载荷解析失败（三要素）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NegotiateError {
    pub code: &'static str,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl NegotiateError {
    fn new(code: &'static str, what: String, why: String, next: String) -> NegotiateError {
        NegotiateError { code, what, why, next }
    }
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

/// 解析 virgl capset 载荷（wire：小端，VIRGL_CAPSET_MIN_LEN 起）。
///
/// 短包显性拒绝——解析不是猜字段。
pub fn parse_host_caps(payload: &[u8]) -> Result<HostCaps, NegotiateError> {
    if payload.len() < VIRGL_CAPSET_MIN_LEN {
        return Err(NegotiateError::new(
            "E_CAPSET_SHORT",
            format!("capset 载荷 {} 字节不足最小 {} 字节", payload.len(), VIRGL_CAPSET_MIN_LEN),
            "载荷不完整时逐字段硬解就是编造 host 能力——能力虚报比没有能力更危险".to_string(),
            "重取 capset；持续短包按协商失败（Unavailable）处置".to_string(),
        ));
    }
    let rd = |i: usize| -> u32 {
        u32::from_le_bytes([payload[i], payload[i + 1], payload[i + 2], payload[i + 3]])
    };
    let _capset_id = rd(0);
    let version = rd(4);
    let _ = version; // 版本在四态判定层消费，载荷解析只管结构
    let gl_major = rd(8);
    let gl_minor = rd(12);
    let max_texture_size = rd(16);
    let max_samples = rd(20);
    let mut fmt = [0u8; 8];
    fmt.copy_from_slice(&payload[24..32]);
    Ok(HostCaps {
        gl_major,
        gl_minor,
        max_texture_size,
        max_samples,
        format_mask: u64::from_le_bytes(fmt),
    })
}

// ---------------------------------------------------------------------------
// 四、能力映射表（host GL → F0007 标准特性位，逐项带依据）
// ---------------------------------------------------------------------------

/// 单条映射规则（表驱动：规则、依据、落到哪个标准位）。
pub struct MappingRule {
    pub key: &'static str,
    pub basis: &'static str,
    /// 依据函数：host 能力 → 是否置位。
    pub holds: fn(&HostCaps) -> bool,
}

/// 冻结映射表。加规则 = 追加（位序契约纪律）。
pub const MAPPING_RULES: [MappingRule; 4] = [
    MappingRule {
        key: "raster3d",
        basis: "GL ≥ 2.0 具备可编程 3D 光栅管线",
        holds: |c| c.gl_version() >= 200,
    },
    MappingRule {
        key: "tessellation",
        basis: "GL ≥ 3.2 提供曲面细分着色器",
        holds: |c| c.gl_version() >= 302,
    },
    MappingRule {
        key: "compute",
        basis: "GL ≥ 4.3 提供计算着色器",
        holds: |c| c.gl_version() >= 403,
    },
    MappingRule {
        key: "color_mgmt",
        basis: "GL ≥ 3.0 提供 sRGB 帧缓冲语义",
        holds: |c| c.gl_version() >= 300,
    },
];

/// host 能力 → F0007 标准特性位图。厂商位恒零（vendor_free 纪律）。
pub fn map_to_bitmap(caps: &HostCaps) -> StandardBitmap {
    let mut b = StandardBitmap::default();
    for rule in MAPPING_RULES.iter() {
        if (rule.holds)(caps) {
            b.set(rule.key, true);
        }
    }
    b
}

// ---------------------------------------------------------------------------
// 五、裁剪声明：超 host 能力的请求在创建期拒绝
// ---------------------------------------------------------------------------

/// 3D 资源创建请求的关键参数（本层校验所需子集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Create3dRequest {
    pub width: u32,
    pub height: u32,
    pub nr_samples: u32,
    /// 格式掩码位（与 HostCaps.format_mask 同一套位）。
    pub format_bit: u32,
}

impl Create3dRequest {
    /// 对照 host 限制的创建期校验。通过 = 请求在 host 能力域内。
    pub fn admit(&self, caps: &HostCaps) -> Result<(), NegotiateError> {
        let max_dim = self.width.max(self.height) as u64;
        if max_dim > caps.max_texture_size as u64 {
            return Err(NegotiateError::new(
                "E_EXCEED_MAX_TEX",
                format!(
                    "请求边长 {} 超过 host 最大纹理 {}",
                    max_dim, caps.max_texture_size
                ),
                "host 硬边界不是软建议——放行会在运行期得到截断或黑屏".to_string(),
                "降分辨率至 host 上限内，或走无 virgl 的软渲路径".to_string(),
            ));
        }
        if self.nr_samples > caps.max_samples {
            return Err(NegotiateError::new(
                "E_EXCEED_SAMPLES",
                format!("请求采样数 {} 超过 host 最大 {}", self.nr_samples, caps.max_samples),
                "多重采样上限由 host 分配能力决定，超限请求创建即失败".to_string(),
                format!("降采样数至 {} 或以下", caps.max_samples),
            ));
        }
        if self.format_bit >= 64 || caps.format_mask & (1u64 << self.format_bit) == 0 {
            return Err(NegotiateError::new(
                "E_FORMAT_UNSUPPORTED",
                format!("格式位 {} 不在 host 支持集内", self.format_bit),
                "格式掩码是 host 实测结果，不在集内硬转就是能力虚报".to_string(),
                "改用掩码内格式，或在 VE 侧先做格式转换再上传".to_string(),
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 六、降级链联动（无 virgl → 软渲，VE-F0013）
// ---------------------------------------------------------------------------

/// 降级通知（三要素 + 读屏可达文本）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradeNotice {
    pub state: VirglState,
    pub target: &'static str,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl DegradeNotice {
    /// 四态 → 降级通知。就绪态返回 None（无需降级）。
    pub fn for_state(state: VirglState) -> Option<DegradeNotice> {
        match state {
            VirglState::NoVirgl => Some(DegradeNotice {
                state,
                target: "软渲（VE-F0013）",
                what: "host 未提供 virgl 3D 透传，3D 需求转由 CPU 软渲染承担".to_string(),
                why: "virgl 通路依赖 host capset 支持，无 capset 即无硬件 3D 语义透传"
                    .to_string(),
                next: "3D 场景按软渲性能预期运行；需要硬件 3D 请启用 host 的 virgl 支持"
                    .to_string(),
            }),
            VirglState::Unavailable => Some(DegradeNotice {
                state,
                target: "软渲（VE-F0013）",
                what: "virgl 协商失败（设备不可达或 capset 损坏），3D 需求临时转软渲".to_string(),
                why: "协商失败不等于无 virgl——可重试；但重试成功前 3D 不能空等".to_string(),
                next: "后台按退避节奏重试协商；成功后 3D 自动迁回硬件通路".to_string(),
            }),
            VirglState::Virgl1 | VirglState::Virgl2 => None,
        }
    }

    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }

    /// 读屏播报文本。
    pub fn a11y_text(&self) -> String {
        format!(
            "3D 渲染降级：{}。原因：{}。建议：{}",
            self.what, self.why, self.next
        )
    }
}

// ---------------------------------------------------------------------------
// 七、协商器（分步耗时账面 + 总账 ≤50ms）
// ---------------------------------------------------------------------------

/// 协商输入（探测阶段产物；本模型确定性注入）。
#[derive(Clone, Debug, Default)]
pub struct HostProbe {
    /// 设备是否可达（GET_CAPSET_INFO 是否得到应答）。
    pub device_alive: bool,
    /// capset 登记：(capset_id, version, max_size)。None = 查询失败。
    pub capsets: Option<Vec<(u32, u32, u32)>>,
    /// virgl capset 载荷（capset_id == CAPSET_VIRGL_ID 对应条目）。
    pub payload: Option<Vec<u8>>,
}

impl HostProbe {
    /// 有 virgl capset 登记吗？
    pub fn has_virgl_capset(&self) -> bool {
        self.capsets
            .as_ref()
            .map(|cs| cs.iter().any(|(id, _, _)| *id == CAPSET_VIRGL_ID))
            .unwrap_or(false)
    }

    /// virgl capset 的版本号（0 = 未见）。
    pub fn virgl_version(&self) -> u32 {
        self.capsets
            .as_ref()
            .and_then(|cs| cs.iter().find(|(id, _, _)| *id == CAPSET_VIRGL_ID))
            .map(|(_, v, _)| *v)
            .unwrap_or(0)
    }
}

/// 协商步骤账目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NegotiateStep {
    pub label: &'static str,
    pub cost_us: u64,
}

/// 协商结果。
#[derive(Clone, Debug)]
pub struct Negotiation {
    pub state: VirglState,
    pub caps: Option<HostCaps>,
    pub bitmap: StandardBitmap,
    pub notice: Option<DegradeNotice>,
    pub steps: Vec<NegotiateStep>,
    pub total_us: u64,
    /// 总账是否在 50ms 预算内（判据：协商耗时 ≤50ms）。
    pub within_budget: bool,
    /// 协商成功后创建的 3D 上下文号（0 = 未创建）。
    pub ctx_id: u32,
}

/// virgl 上下文协商器。
pub struct VirglNegotiator {
    next_ctx_id: u32,
}

impl VirglNegotiator {
    pub fn new() -> VirglNegotiator {
        VirglNegotiator { next_ctx_id: 1 }
    }

    /// 全流程协商：探测 → 取载荷 → 解析 → 映射 → （就绪时）创建上下文。
    ///
    /// 每步记账；总账超 50ms 也如实标注（within_budget=false），
    /// 但判定本身不被耗时影响——耗时是性能账，不是功能开关。
    pub fn negotiate(&mut self, probe: &HostProbe) -> Negotiation {
        let mut steps: Vec<NegotiateStep> = Vec::new();
        let mut total: u64 = 0;
        let mut step = |steps: &mut Vec<NegotiateStep>, total: &mut u64, label: &'static str, cost: u64| {
            steps.push(NegotiateStep { label, cost_us: cost });
            *total += cost;
        };

        // 1) 设备探测：capset 信息查询。
        step(&mut steps, &mut total, "capset_info 查询", 200);
        if !probe.device_alive {
            let state = VirglState::Unavailable;
            return self.finish(state, None, steps, total, 0);
        }

        // 2) virgl capset 存在性判定。
        step(&mut steps, &mut total, "virgl capset 存在性", 150);
        if !probe.has_virgl_capset() {
            let state = VirglState::NoVirgl;
            return self.finish(state, None, steps, total, 0);
        }
        let version = probe.virgl_version();

        // 3) 载荷获取与解析。
        step(&mut steps, &mut total, "capset 载荷获取", 150);
        let payload = match probe.payload.as_ref() {
            Some(p) => p.clone(),
            None => {
                let state = VirglState::Unavailable;
                return self.finish(state, None, steps, total, 0);
            }
        };
        step(&mut steps, &mut total, "载荷解析", 50);
        let caps = match parse_host_caps(&payload) {
            Ok(c) => c,
            Err(_) => {
                let state = VirglState::Unavailable;
                return self.finish(state, None, steps, total, 0);
            }
        };

        // 4) 能力映射到 F0007 位图。
        step(&mut steps, &mut total, "能力映射（F0007）", 100);
        let bitmap = map_to_bitmap(&caps);

        // 5) 四态判定（capset 版本 ≥2 = virgl2）。
        let state = if version >= 2 { VirglState::Virgl2 } else { VirglState::Virgl1 };

        // 6) 3D 上下文创建（就绪态才建）。
        let ctx_id = if state.ready() {
            step(&mut steps, &mut total, "VIRGL_CTX_CREATE", 300);
            let id = self.next_ctx_id;
            self.next_ctx_id += 1;
            id
        } else {
            0
        };

        self.finish(state, Some(caps), steps, total, ctx_id)
            .with_bitmap(bitmap)
    }

    fn finish(
        &self,
        state: VirglState,
        caps: Option<HostCaps>,
        steps: Vec<NegotiateStep>,
        total: u64,
        ctx_id: u32,
    ) -> Negotiation {
        Negotiation {
            state,
            caps,
            bitmap: StandardBitmap::default(),
            notice: DegradeNotice::for_state(state),
            steps,
            total_us: total,
            within_budget: total <= NEGOTIATION_BUDGET_US,
            ctx_id,
        }
    }
}

impl Negotiation {
    fn with_bitmap(mut self, bitmap: StandardBitmap) -> Negotiation {
        self.bitmap = bitmap;
        self
    }
}

// ---------------------------------------------------------------------------
// 八、3D 上下文命令编码（wire 对齐规范：24 字节头 + 体）
// ---------------------------------------------------------------------------

/// CTX_CREATE：hdr + ctx_id + nlen + pad4 + name。
pub fn encode_ctx_create(ctx_id: u32, name: &str) -> Vec<u8> {
    let mut b = Vec::with_capacity(CTRL_HDR_LEN + 12 + name.len());
    push_hdr(&mut b, CMD_CTX_CREATE);
    b.extend_from_slice(&ctx_id.to_le_bytes());
    b.extend_from_slice(&(name.len() as u32).to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(name.as_bytes());
    b
}

/// CTX_DESTROY：hdr + ctx_id + pad。
pub fn encode_ctx_destroy(ctx_id: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(CTRL_HDR_LEN + 8);
    push_hdr(&mut b, CMD_CTX_DESTROY);
    b.extend_from_slice(&ctx_id.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b
}

/// CTX_ATTACH/DETACH_RESOURCE：hdr + resource_id + ctx_id + pad。
pub fn encode_ctx_resource(kind: u32, ctx_id: u32, resource_id: u32) -> Result<Vec<u8>, NegotiateError> {
    if kind != CMD_CTX_ATTACH_RESOURCE && kind != CMD_CTX_DETACH_RESOURCE {
        return Err(NegotiateError::new(
            "E_CTX_CMD_UNKNOWN",
            format!("命令 0x{:04X} 不在上下文资源命令族", kind),
            "上下文资源操作只有 ATTACH/DETACH 两种，硬编码未知命令就是越权".to_string(),
            "使用 CMD_CTX_ATTACH_RESOURCE / CMD_CTX_DETACH_RESOURCE".to_string(),
        ));
    }
    let mut b = Vec::with_capacity(CTRL_HDR_LEN + 12);
    push_hdr(&mut b, kind);
    b.extend_from_slice(&resource_id.to_le_bytes());
    b.extend_from_slice(&ctx_id.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    Ok(b)
}

fn push_hdr(b: &mut Vec<u8>, cmd: u32) {
    b.extend_from_slice(&cmd.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes()); // flags
    b.extend_from_slice(&0u64.to_le_bytes()); // fence（协商期占位，提交层覆盖）
    b.extend_from_slice(&0u32.to_le_bytes()); // ctx_id 占位
    b.push(0); // ring
    b.extend_from_slice(&[0u8; 3]); // 规范 padding
}

/// 上下文命令 wire 长度对拍（编码零字节冗余的同源纪律）。
pub fn ctx_wire_len(cmd: u32, body: usize) -> usize {
    CTRL_HDR_LEN + body
}

// ---------------------------------------------------------------------------
// 九、读屏摘要
// ---------------------------------------------------------------------------

/// 协商结果的人话摘要（读屏可达）。
pub fn negotiation_summary(n: &Negotiation) -> String {
    match n.state.ready() {
        true => format!(
            "virgl 协商成功：{}、上下文 {}、特性 {}、耗时 {}µs（预算 {}µs）",
            n.state.label(),
            n.ctx_id,
            n.bitmap.describe(),
            n.total_us,
            NEGOTIATION_BUDGET_US
        ),
        false => format!(
            "virgl 协商未就绪：{}、降级 {}、耗时 {}µs（预算 {}µs）",
            n.state.label(),
            n.notice.as_ref().map(|x| x.target).unwrap_or("无"),
            n.total_us,
            NEGOTIATION_BUDGET_US
        ),
    }
}
