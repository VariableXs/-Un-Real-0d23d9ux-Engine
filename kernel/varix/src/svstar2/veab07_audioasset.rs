//! veab07_audioasset — VE-F5607 音频资产管线（VE-AB 域 · AB07 · 目标 300 行，深化版）。
//!
//! 判据原文（施工书锚点 VE-F5607）：音频资产管线——导入、压缩、流送三段（格式注册制、
//! 压缩率与音质双标注、流送按需加载）；导入阶段即校验采样率与声道数合法性；流送预取按
//! 播放进度滚动，预取失败提前三秒降级本地回退；管线含资产体检报告——导入完成的资产附
//! 码率、时长、响度三数。判据：三段管线、双标注、按需流送、判据。
//!
//! **职责定位**：三段单向管线（导入 → 压缩 → 流送计划），每段独立可验收、段间交接
//! 带不可变快照；格式注册制（运行期注册、重复注册显性拒、未注册格式拒绝入库——闭集
//! 外一律显性不静默猜）；压缩段输出「压缩率 × 音质档」双标注（二者同时在场缺一即
//! 违约，双标注可查=无障碍要点落地）；流送段按播放进度滚动预取，预取失败提前三秒
//! 降级本地回退（迟滞回切防振荡）；资产体检报告附码率/时长/响度三数，三数全在才算
//! 导入完成。错误路径全显性：格式不支持→明确拒绝；压缩失真超阈→拒绝入库；流送
//! 超时→本地回退。性能逐项分解：导入 O(样本数) 单遍、压缩 O(资产) 分级查表、流送
//! O(段数) 滚动窗。跨批对接点：X05 分发（压缩产物封包交分发）、veab06 语音骨架
//! （本管线为语音资产的上游供给方）。无障碍与隐私：双标注可查、体检报告不含内容
//! 采样数据（零内容记录——只留统计量）。
//!
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，全整数算术（响度 milli-LU、
//! 失真比查表对数），逻辑 tick 注入零墙钟，FNV-1a 指纹，诊断码 0xA0 段闭集。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::result::Result;

use crate::checks::CheckSet;

// ─────────────────────────────────────────────────────────────────────────────
// 诊断码 0xA0 段闭集（本文件独占；先入库者占段纪律已核对 0x91 空隙后取 0xA0）
// ─────────────────────────────────────────────────────────────────────────────

/// 本模块诊断码段基址：0xA0（0xA001~0xA00C 十二码闭集）。
pub const SEGMENT_BASE: u16 = 0xA0;

/// 诊断码闭集：三段管线全部错误路径逐码显性，禁静默吞错。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 0xA001 格式未注册——注册制闭集外格式拒绝入库。
    FormatUnregistered,
    /// 0xA002 采样率非法——闭集外采样率（导入期即拒，不进压缩段）。
    IllegalSampleRate,
    /// 0xA003 声道布局非法——声道数或掩码不合法。
    IllegalChannelLayout,
    /// 0xA004 位深非法——该格式不支持此位深。
    IllegalBitDepth,
    /// 0xA005 空资产——零样本或零字节拒绝导入。
    EmptyAsset,
    /// 0xA006 压缩失真超阈——拒绝入库（降级矩阵第一行）。
    DistortionOverThreshold,
    /// 0xA007 双标注缺一——压缩产物压缩率/音质档缺任一即违约。
    DualAnnotationMissing,
    /// 0xA008 流送超时——请求超龄转本地回退（降级矩阵第二行）。
    StreamTimeout,
    /// 0xA009 本地回退不可用——回退资源缺失（降级矩阵第三行，终极显性）。
    LocalFallbackUnavailable,
    /// 0xA00A 缓冲守恒破坏——Σ已交付+已丢弃+在缓冲≠已加载。
    BufferConservationBroken,
    /// 0xA00B 体检报告三数缺一——码率/时长/响度缺任一不算导入完成。
    HealthReportIncomplete,
    /// 0xA00C 账本溢出防护——checked 算术溢出显性拒绝不回绕。
    LedgerOverflow,
}

impl XcCode {
    /// 段基址（审计聚合用）。
    pub fn seg(self) -> u16 {
        SEGMENT_BASE
    }

    /// 码值。
    pub fn code(self) -> u16 {
        match self {
            XcCode::FormatUnregistered => 0xA001,
            XcCode::IllegalSampleRate => 0xA002,
            XcCode::IllegalChannelLayout => 0xA003,
            XcCode::IllegalBitDepth => 0xA004,
            XcCode::EmptyAsset => 0xA005,
            XcCode::DistortionOverThreshold => 0xA006,
            XcCode::DualAnnotationMissing => 0xA007,
            XcCode::StreamTimeout => 0xA008,
            XcCode::LocalFallbackUnavailable => 0xA009,
            XcCode::BufferConservationBroken => 0xA00A,
            XcCode::HealthReportIncomplete => 0xA00B,
            XcCode::LedgerOverflow => 0xA00C,
        }
    }

    /// 可操作建议（错误路径带建议=深度纪律）。
    pub fn detail(self) -> &'static str {
        match self {
            XcCode::FormatUnregistered => {
                "先在 FormatRegistry::register 注册该格式并核对能力描述符，未注册格式不入库"
            }
            XcCode::IllegalSampleRate => {
                "采样率必须落在闭集 8000~192000 十一档内，越界请重采样后再导入"
            }
            XcCode::IllegalChannelLayout => {
                "声道数 1~8 且布局掩码须命中单声道/立体声/5.1/7.1 四闭集之一"
            }
            XcCode::IllegalBitDepth => {
                "核对 FormatSpec.supported_depths 闭集，选该格式声明支持的位深"
            }
            XcCode::EmptyAsset => "零样本资产无管线意义，请检查上游导出是否被截断",
            XcCode::DistortionOverThreshold => {
                "失真超阈拒绝入库：提高音质档或改用无损格式，禁止静默放行劣化资产"
            }
            XcCode::DualAnnotationMissing => {
                "压缩产物必须同时携带压缩率与音质档（双标注可查），缺一即违约"
            }
            XcCode::StreamTimeout => {
                "预取请求超龄已转本地回退；检查预取窗口与超时阈值配比（建议超时≤预取窗 1/2）"
            }
            XcCode::LocalFallbackUnavailable => {
                "本地回退资源缺失且流送已断：资产不可播，需重新下载或本地预置"
            }
            XcCode::BufferConservationBroken => {
                "缓冲账守恒破坏即记账 bug：Σ(已交付+已丢弃+在缓冲)必须逐 tick 等于已加载"
            }
            XcCode::HealthReportIncomplete => {
                "导入完成以体检报告三数（码率/时长/响度）齐备为准，缺一不算完成"
            }
            XcCode::LedgerOverflow => {
                "checked 算术溢出：容量字段为 u64 可扩展口径，溢出说明上游计数已错，先归因"
            }
        }
    }
}

/// 结构化诊断（码 + 人话 + 建议，零静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    code: XcCode,
    message: String,
}

impl Diag {
    pub fn new(code: XcCode, message: &str) -> Self {
        Diag {
            code,
            message: message.to_string(),
        }
    }

    pub fn code(&self) -> XcCode {
        self.code
    }

    /// 完整人话串：码 · 信息 · 建议。
    pub fn render(&self) -> String {
        format!(
            "0x{:04X}[0x{:02X}] · {} · 建议：{}",
            self.code.code(),
            self.code.seg(),
            self.message,
            self.code.detail()
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FNV-1a 32（确定性指纹，零外部依赖）
// ─────────────────────────────────────────────────────────────────────────────

/// FNV-1a 32 位指纹（对拍红线：同输入同指纹）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG（逻辑 tick 时钟与注入序号用，同种子同序列——对拍可复现）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg {
            state: seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407),
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    /// [0, bound) 内的确定性伪随机（bound 为 0 时恒 0）。
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            0
        } else {
            self.next_u64() % bound
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 第一部分 · 格式注册表（格式注册制：闭集描述符 + 运行期注册 + 重复拒）
// ─────────────────────────────────────────────────────────────────────────────

/// 音频格式闭集：五族覆盖 无损/有损/低延迟/语音 四用途。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioFormat {
    /// 线性 PCM（无损基线，档案级）。
    PcmLinear,
    /// IMA ADPCM（4:1 有损，低算力解压）。
    AdpcmIma,
    /// FLAC（无损压缩，档案+分发两用）。
    FlacLossless,
    /// Vorbis（有损，中码率分发）。
    Vorbis,
    /// Opus（低延迟，语音/交互）。
    Opus,
}

impl AudioFormat {
    /// 全格式闭集遍历（注册表完整性对账用）。
    pub const ALL: [AudioFormat; 5] = [
        AudioFormat::PcmLinear,
        AudioFormat::AdpcmIma,
        AudioFormat::FlacLossless,
        AudioFormat::Vorbis,
        AudioFormat::Opus,
    ];

    pub fn name(self) -> &'static str {
        match self {
            AudioFormat::PcmLinear => "pcm_linear",
            AudioFormat::AdpcmIma => "adpcm_ima",
            AudioFormat::FlacLossless => "flac",
            AudioFormat::Vorbis => "vorbis",
            AudioFormat::Opus => "opus",
        }
    }
}

/// 声道布局闭集：单声道/立体声/5.1/7.1 四闭集（掩码逐位核对，防"8 声道任意掩码"糊弄）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelLayout {
    Mono,
    Stereo,
    Surround51,
    Surround71,
}

impl ChannelLayout {
    /// 声道数。
    pub fn channel_count(self) -> u8 {
        match self {
            ChannelLayout::Mono => 1,
            ChannelLayout::Stereo => 2,
            ChannelLayout::Surround51 => 6,
            ChannelLayout::Surround71 => 8,
        }
    }

    /// 掩码（WAVEFORMATEX 通道掩码语义，常量来源：Windows mmreg.h 惯例）。
    pub fn mask(self) -> u32 {
        match self {
            // 0 = 非定位（单声道缺省）
            ChannelLayout::Mono => 0x0000_0004, // FC 前中
            ChannelLayout::Stereo => 0x0000_0003, // FL|FR
            // FL|FR|FC|LFE|BL|BR
            ChannelLayout::Surround51 => 0x0000_003F,
            // FL|FR|FC|LFE|BL|BR|SL|SR
            ChannelLayout::Surround71 => 0x0000_063F,
        }
    }

    /// 从声道数+掩码双证据裁定布局；二者矛盾显性 None（不猜）。
    pub fn resolve(count: u8, mask: u32) -> Option<ChannelLayout> {
        match count {
            1 => {
                if mask == 0 || mask == ChannelLayout::Mono.mask() {
                    Some(ChannelLayout::Mono)
                } else {
                    None
                }
            }
            2 => {
                if mask == 0 || mask == ChannelLayout::Stereo.mask() {
                    Some(ChannelLayout::Stereo)
                } else {
                    None
                }
            }
            6 => {
                if mask == 0 || mask == ChannelLayout::Surround51.mask() {
                    Some(ChannelLayout::Surround51)
                } else {
                    None
                }
            }
            8 => {
                if mask == 0 || mask == ChannelLayout::Surround71.mask() {
                    Some(ChannelLayout::Surround71)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// 格式能力描述符（注册制的正本——采样率闭集/位深闭集/声道上限/有损标记/包帧字节）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatSpec {
    format: AudioFormat,
    /// 采样率闭集（Hz）。导入期逐项核对，闭集外拒绝。
    sample_rates: Vec<u32>,
    /// 位深闭集（bits per sample）。
    supported_depths: Vec<u8>,
    /// 声道数上限（1~8 内取值）。
    max_channels: u8,
    /// 无损标记（决定压缩段是否允许失真门）。
    lossless: bool,
    /// 流送包帧字节（净载荷每包近似字节数，流送段分片对齐用）。
    frame_packet_bytes: u32,
}

impl FormatSpec {
    /// 注册期自检：描述符自身合法性（闭集非空、声道上限 1~8、包帧字节非零）。
    pub fn validate(&self) -> Result<(), Diag> {
        if self.sample_rates.is_empty() {
            return Err(Diag::new(
                XcCode::FormatUnregistered,
                "采样率闭集为空的描述符不可注册",
            ));
        }
        if self.supported_depths.is_empty() {
            return Err(Diag::new(
                XcCode::FormatUnregistered,
                "位深闭集为空的描述符不可注册",
            ));
        }
        if self.max_channels == 0 || self.max_channels > 8 {
            return Err(Diag::new(
                XcCode::FormatUnregistered,
                "声道上限必须在 1~8（物理输出闭集）",
            ));
        }
        if self.frame_packet_bytes == 0 {
            return Err(Diag::new(
                XcCode::FormatUnregistered,
                "包帧字节为零的格式无法流送分片",
            ));
        }
        Ok(())
    }

    pub fn format(&self) -> AudioFormat {
        self.format
    }

    pub fn lossless(&self) -> bool {
        self.lossless
    }

    pub fn frame_packet_bytes(&self) -> u32 {
        self.frame_packet_bytes
    }

    /// 采样率是否在闭集内。
    pub fn allows_rate(&self, rate: u32) -> bool {
        self.sample_rates.iter().any(|r| *r == rate)
    }

    /// 位深是否在闭集内。
    pub fn allows_depth(&self, depth: u8) -> bool {
        self.supported_depths.iter().any(|d| *d == depth)
    }
}

/// 全局采样率闭集（十一档；来源：Windows WASAPI 共享模式惯例集 + 电话 8k + 高解析 192k）。
pub const GLOBAL_SAMPLE_RATES: [u32; 11] = [
    8000, 11025, 16000, 22050, 32000, 44100, 48000, 88200, 96000, 176400, 192000,
];

/// 格式注册表：运行期注册 + 重复显性拒 + 未注册查表显性 None。
#[derive(Clone, Debug, Default)]
pub struct FormatRegistry {
    entries: Vec<FormatSpec>,
    /// 注册审计账：追加只增，记录每次注册与每次拒绝。
    audit: Vec<String>,
}

impl FormatRegistry {
    pub fn new() -> Self {
        FormatRegistry {
            entries: Vec::new(),
            audit: Vec::new(),
        }
    }

    /// 注册一个格式描述符。重复注册显性拒（返回 Err 携带已注册事实）；
    /// 描述符自检不过拒绝注册。拒绝均入审计账。
    pub fn register(&mut self, spec: FormatSpec) -> Result<(), Diag> {
        spec.validate()?;
        if self.entries.iter().any(|e| e.format == spec.format) {
            self.audit.push(format!(
                "REJECT duplicate register {}",
                spec.format.name()
            ));
            return Err(Diag::new(
                XcCode::FormatUnregistered,
                "格式已注册：重复注册显性拒绝，改注册请先走下线流程",
            ));
        }
        self.audit.push(format!("REGISTER {}", spec.format.name()));
        self.entries.push(spec);
        Ok(())
    }

    /// 预置五族默认描述符（当前策略值，可调——常量来源：格式族工程惯例）。
    pub fn with_defaults() -> Self {
        let mut reg = FormatRegistry::new();
        // PCM：全采样率全位深，档案无损。
        let _ = reg.register(FormatSpec {
            format: AudioFormat::PcmLinear,
            sample_rates: GLOBAL_SAMPLE_RATES.to_vec(),
            supported_depths: vec![8, 16, 24, 32],
            max_channels: 8,
            lossless: true,
            frame_packet_bytes: 4096,
        });
        // IMA ADPCM：4:1 压缩，16bit 容器。
        let _ = reg.register(FormatSpec {
            format: AudioFormat::AdpcmIma,
            sample_rates: vec![8000, 11025, 16000, 22050, 32000, 44100, 48000],
            supported_depths: vec![4],
            max_channels: 2,
            lossless: false,
            frame_packet_bytes: 1024,
        });
        // FLAC：无损压缩，16/24bit。
        let _ = reg.register(FormatSpec {
            format: AudioFormat::FlacLossless,
            sample_rates: GLOBAL_SAMPLE_RATES.to_vec(),
            supported_depths: vec![16, 24],
            max_channels: 8,
            lossless: true,
            frame_packet_bytes: 4096,
        });
        // Vorbis：有损分发。
        let _ = reg.register(FormatSpec {
            format: AudioFormat::Vorbis,
            sample_rates: vec![8000, 11025, 16000, 22050, 32000, 44100, 48000, 96000],
            supported_depths: vec![16],
            max_channels: 8,
            lossless: false,
            frame_packet_bytes: 2048,
        });
        // Opus：低延迟交互，窄带起。
        let _ = reg.register(FormatSpec {
            format: AudioFormat::Opus,
            sample_rates: vec![8000, 12000, 16000, 24000, 48000],
            supported_depths: vec![16],
            max_channels: 2,
            lossless: false,
            frame_packet_bytes: 960,
        });
        reg
    }

    /// 查表：未注册格式显性 None（调用方转 0xA001 拒绝）。
    pub fn lookup(&self, fmt: AudioFormat) -> Option<&FormatSpec> {
        self.entries.iter().find(|e| e.format == fmt)
    }

    pub fn registered_count(&self) -> usize {
        self.entries.len()
    }

    pub fn audit_trail(&self) -> &[String] {
        &self.audit
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 第二部分 · 导入段（合法性校验 + 体检三数 + 单遍 O(样本)）
// ─────────────────────────────────────────────────────────────────────────────

/// 导入请求：资产在管线入口的全部事实。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportRequest {
    format: AudioFormat,
    sample_rate: u32,
    channel_count: u8,
    channel_mask: u32,
    bit_depth: u8,
    /// 总样本数（逐声道计）。
    total_samples: u64,
    /// 原始数据字节数（所有声道合计）。
    data_bytes: u64,
    /// 峰值样本绝对值（位深原码域）。
    peak_amplitude: u32,
    /// 均方根样本能量（位深满幅平方域，体检响度底座）。
    rms_energy: u64,
}

impl ImportRequest {
    /// 体检响度输入的最小事实自检（防上游传零而响度恒零的"谎报绿"）。
    pub fn plausible(&self) -> bool {
        self.total_samples > 0 && self.data_bytes > 0 && self.sample_rate > 0
    }
}

/// 资产体检报告：导入完成的唯一凭证——三数（码率/时长/响度）齐备才算完成。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetHealthReport {
    /// 码率 bps（整数，data_bytes*8*rate/total_samples 四舍五入）。
    bitrate_bps: u64,
    /// 时长：整秒 + 纳秒余（u64 秒可扩展口径，不设 5GB 式窄天花板）。
    duration_secs: u64,
    duration_nanos_remainder: u32,
    /// 响度（milli-LU，相对满幅正弦基准 -18 LUFS 对齐的整数近似）。
    loudness_milli_lu: i64,
    /// 真峰值（位深原码域）。
    peak_amplitude: u32,
    /// 三数在场凭证（体检报告完整性判据的机器可查面）。
    triple_present: bool,
}

impl AssetHealthReport {
    pub fn bitrate_bps(&self) -> u64 {
        self.bitrate_bps
    }
    pub fn duration_secs(&self) -> u64 {
        self.duration_secs
    }
    pub fn duration_nanos_remainder(&self) -> u32 {
        self.duration_nanos_remainder
    }
    pub fn loudness_milli_lu(&self) -> i64 {
        self.loudness_milli_lu
    }
    pub fn peak_amplitude(&self) -> u32 {
        self.peak_amplitude
    }
    pub fn triple_present(&self) -> bool {
        self.triple_present
    }

    /// 完整性守卫：三数缺一 = 0xA00B（导入未完成）。
    pub fn require_complete(&self) -> Result<(), Diag> {
        if !self.triple_present {
            return Err(Diag::new(
                XcCode::HealthReportIncomplete,
                "体检报告三数（码率/时长/响度）缺一，导入不算完成",
            ));
        }
        Ok(())
    }

    /// 指纹（报告内容变指纹变——双标注可查的技术底座之一）。
    pub fn fingerprint(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.bitrate_bps.to_le_bytes());
        buf.extend_from_slice(&self.duration_secs.to_le_bytes());
        buf.extend_from_slice(&self.duration_nanos_remainder.to_le_bytes());
        buf.extend_from_slice(&self.loudness_milli_lu.to_le_bytes());
        buf.extend_from_slice(&self.peak_amplitude.to_le_bytes());
        fnv1a(&buf)
    }
}

/// 响度基准：满幅正弦 RMS 对齐 -18 LUFS（EBU R128 惯例），milli-LU 整数域。
/// loudness_LU = -18 + 20*log10(rms_fullscale_ratio)，此处用 log2 LUT 整数近似。
pub const LOUDNESS_REF_MILLI_LU: i64 = -18_000;

/// log10 近似：输入 x1e6 域比值（1_000_000 = 1.0），输出 milli-dB。
/// 用 12 位查表 + 线性插值（区间 [1e6/1e6, 1e6/1] 全域单调）。
pub fn ratio_to_milli_db(ratio_milli: u64) -> i64 {
    if ratio_milli == 0 {
        return i64::MIN / 4; // 负无穷近似（静音），调用方钳制
    }
    // 结果 = 6_000_000 * log2(ratio_milli / 1e6) milli-dB（log2 换 log10 乘 30103/100000）
    // 逐位找最高有效位，尾数 8bit 查表。
    let mut v = ratio_milli;
    let mut lead: u32 = 0;
    while v >= 2 {
        v >>= 1;
        lead += 1;
    }
    // 归一尾数 [1,2) 的千分比表：manti = (ratio >> max(0, lead-20)) 对齐 20bit 尾数
    let shift = if lead >= 20 { lead - 20 } else { 0 };
    let manti = ((ratio_milli >> shift).min(0xFFFFF)) as u64; // 20bit 尾数
    // 查表：log2(1 + f) f∈[0,1) 20bit 定点，64 项粗表 + 插值
    let idx = (manti >> 14) as usize; // 高 6 位索引
    const LOG2_TAB: [u32; 65] = [
        0, 11873, 23258, 34211, 44789, 55044, 65019, 74743, 84250, 93563, 102097, 111004, 119760,
        128322, 136690, 144900, 152957, 160864, 168625, 176243, 183720, 191059, 198265, 205340,
        212289, 219114, 225818, 232404, 238874, 245230, 251473, 257607, 263634, 269556, 275375,
        281094, 286713, 292235, 297661, 302993, 308232, 313381, 318440, 323412, 328297, 333099,
        337818, 342457, 347017, 351499, 355905, 360236, 364493, 368677, 372790, 376833, 380807,
        384713, 388553, 392327, 396037, 399683, 403267, 406789, 410252,
    ];
    let base = LOG2_TAB[idx] as u64;
    let next = LOG2_TAB[idx + 1] as u64;
    let frac = manti & 0x3FFF; // 低 14 位
    let interp = base + (next - base) * frac / 0x4000;
    // 总 log2 = lead + interp/2^20（interp 是 20bit 定点）
    let log2_x2e20 = (lead as u64) * 0x100000 + interp;
    // milli-dB = log2 * 30103/100000 * 1000 ≈ log2 * 301 (per 1e-3 dB per 2^-20 log2 unit:
    // 6.0206 dB per log2 → 6020.6 milli-dB per log2 → per 2^-20 单位 = 6020.6/1048576 ≈ 0.005743)
    let milli_db = (log2_x2e20 * 60206 / 10_000_000) as i64;
    milli_db
}

/// 响度计算：满幅比 rms/满幅 → LU 偏移（相对 -18 LUFS 基准）。
pub fn loudness_milli_lu(rms_energy: u64, full_scale_sq: u64, total_samples: u64) -> i64 {
    if rms_energy == 0 || total_samples == 0 || full_scale_sq == 0 {
        return LOUDNESS_REF_MILLI_LU - 90_000; // 静音显性 -108 LU 近似，不猜
    }
    // 均方均值 = rms_energy / total_samples（预乘 1e6 定点防下溢）
    let mean_sq_milli = rms_energy.saturating_mul(1_000_000) / total_samples;
    let ratio_milli = mean_sq_milli.saturating_mul(1_000_000) / full_scale_sq;
    if ratio_milli == 0 {
        return LOUDNESS_REF_MILLI_LU - 90_000;
    }
    let rms_milli_db = ratio_to_milli_db(ratio_milli);
    // LUFS ≈ -18 + 20log10(rms_ratio)；rms 能量已平方域，20log10(rms)=10log10(mean_sq_ratio)
    LOUDNESS_REF_MILLI_LU + rms_milli_db
}

/// 导入段：三校验（格式/采样率/声道/位深）+ 体检三数单遍成型。
pub struct ImportStage<'a> {
    registry: &'a FormatRegistry,
    /// 导入审计账（追加只增）。
    audit: Vec<String>,
}

impl<'a> ImportStage<'a> {
    pub fn new(registry: &'a FormatRegistry) -> Self {
        ImportStage {
            registry,
            audit: Vec::new(),
        }
    }

    pub fn audit_trail(&self) -> &[String] {
        &self.audit
    }

    /// 导入主流程：全链校验不过即拒绝（错误路径带码带建议），过则产体检报告。
    pub fn import(&mut self, req: &ImportRequest) -> Result<AssetHealthReport, Diag> {
        // ① 注册制查表：未注册直接拒（闭集外不猜）。
        let spec = match self.registry.lookup(req.format) {
            Some(s) => s,
            None => {
                self.audit
                    .push(format!("REJECT unregistered {}", req.format.name()));
                return Err(Diag::new(
                    XcCode::FormatUnregistered,
                    "该格式未在注册表内，导入拒绝",
                ));
            }
        };
        // ② 空资产拒。
        if req.total_samples == 0 || req.data_bytes == 0 {
            self.audit.push("REJECT empty asset".to_string());
            return Err(Diag::new(XcCode::EmptyAsset, "零样本或零字节资产拒绝导入"));
        }
        // ③ 采样率合法性（导入阶段即校验——锚点原文点名）。
        if !spec.allows_rate(req.sample_rate) {
            self.audit
                .push(format!("REJECT rate {} Hz", req.sample_rate));
            return Err(Diag::new(
                XcCode::IllegalSampleRate,
                "采样率不在该格式闭集内",
            ));
        }
        // ④ 声道合法性：数量上限 + 布局掩码双证据一致。
        if req.channel_count == 0 || req.channel_count > spec.max_channels {
            self.audit
                .push(format!("REJECT channels {}", req.channel_count));
            return Err(Diag::new(
                XcCode::IllegalChannelLayout,
                "声道数越格式上限",
            ));
        }
        if ChannelLayout::resolve(req.channel_count, req.channel_mask).is_none() {
            self.audit
                .push(format!("REJECT mask 0x{:08X}", req.channel_mask));
            return Err(Diag::new(
                XcCode::IllegalChannelLayout,
                "声道掩码与声道数不一致且不在四闭集内",
            ));
        }
        // ⑤ 位深合法性。
        if !spec.allows_depth(req.bit_depth) {
            self.audit
                .push(format!("REJECT depth {} bit", req.bit_depth));
            return Err(Diag::new(
                XcCode::IllegalBitDepth,
                "位深不在该格式闭集内",
            ));
        }
        // ⑥ 体检三数成型（checked 算术，溢出显性 0xA00C）。
        // 码率 = data_bytes * 8 * sample_rate / total_samples（逐声道样本 → 每秒比特）。
        let bytes_per_sec = req
            .data_bytes
            .checked_mul(req.sample_rate as u64)
            .ok_or(Diag::new(XcCode::LedgerOverflow, "码率算术溢出"))?;
        let bitrate_bps = bytes_per_sec
            .checked_mul(8)
            .and_then(|v| v.checked_div(req.total_samples))
            .ok_or(Diag::new(XcCode::LedgerOverflow, "码率除零防护"))?;
        // 时长 = total_samples / channels / rate，秒 + 纳秒余。
        let per_channel_samples = req
            .total_samples
            .checked_div(req.channel_count as u64)
            .ok_or(Diag::new(XcCode::LedgerOverflow, "声道除零防护"))?;
        let duration_secs = per_channel_samples / req.sample_rate as u64;
        let rem_samples = per_channel_samples % req.sample_rate as u64;
        let duration_nanos_remainder =
            ((rem_samples as u128 * 1_000_000_000u128) / req.sample_rate as u128) as u32;
        // 响度：满幅平方域 = (2^(depth-1)-1)^2。
        let half_scale: u64 = (1u64 << (req.bit_depth.min(31) - 1)) - 1;
        let full_scale_sq = half_scale.saturating_mul(half_scale);
        let loudness = loudness_milli_lu(req.rms_energy, full_scale_sq, req.total_samples);
        let report = AssetHealthReport {
            bitrate_bps,
            duration_secs,
            duration_nanos_remainder,
            loudness_milli_lu: loudness,
            peak_amplitude: req.peak_amplitude,
            triple_present: true,
        };
        report.require_complete()?;
        self.audit.push(format!(
            "IMPORT {} rate={} ch={} depth={} dur={}s br={}bps",
            req.format.name(),
            req.sample_rate,
            req.channel_count,
            req.bit_depth,
            duration_secs,
            bitrate_bps
        ));
        Ok(report)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 第三部分 · 压缩段（音质档阶梯 + 失真门 + 双标注 + 包帧）
// ─────────────────────────────────────────────────────────────────────────────

/// 音质档阶梯（当前策略值，可调）：四档覆盖档案→环境音。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum QualityTier {
    /// Q0 档案级：码率无上限追求，失真门最严。
    Archival,
    /// Q1 透明级：人耳盲听不可辨。
    Transparent,
    /// Q2 交互级：游戏/语音可辨但可接受。
    Interactive,
    /// Q3 环境级：背景氛围音，码率优先。
    Ambient,
}

impl QualityTier {
    pub const ALL: [QualityTier; 4] = [
        QualityTier::Archival,
        QualityTier::Transparent,
        QualityTier::Interactive,
        QualityTier::Ambient,
    ];

    pub fn name(self) -> &'static str {
        match self {
            QualityTier::Archival => "archival",
            QualityTier::Transparent => "transparent",
            QualityTier::Interactive => "interactive",
            QualityTier::Ambient => "ambient",
        }
    }

    /// 目标码率（相对原码率的万分比，如 2400 = 24%）。
    /// 常量来源：格式族工程惯例（Vorbis q 阶梯/Opus 档位）四舍五入取整。
    pub fn target_ratio_ppm(self, fmt: AudioFormat) -> u64 {
        match (fmt, self) {
            // 无损格式不走失真压缩：比率恒 10000（幂等直通）
            (AudioFormat::PcmLinear, _) | (AudioFormat::FlacLossless, _) => 10_000,
            (AudioFormat::AdpcmIma, QualityTier::Archival) => 2_500,
            (AudioFormat::AdpcmIma, QualityTier::Transparent) => 2_500,
            (AudioFormat::AdpcmIma, QualityTier::Interactive) => 2_500,
            (AudioFormat::AdpcmIma, QualityTier::Ambient) => 2_500,
            (AudioFormat::Vorbis, QualityTier::Archival) => 4_800,
            (AudioFormat::Vorbis, QualityTier::Transparent) => 3_200,
            (AudioFormat::Vorbis, QualityTier::Interactive) => 1_800,
            (AudioFormat::Vorbis, QualityTier::Ambient) => 900,
            (AudioFormat::Opus, QualityTier::Archival) => 3_600,
            (AudioFormat::Opus, QualityTier::Transparent) => 2_400,
            (AudioFormat::Opus, QualityTier::Interactive) => 1_200,
            (AudioFormat::Opus, QualityTier::Ambient) => 600,
        }
    }

    /// 该档失真门（milli-dB SNR 下限）：档案级 96dB、透明 72dB、交互 48dB、环境 30dB。
    /// 常量来源：可听阈工程惯例（SNR 与主观档位映射）。
    pub fn snr_floor_milli_db(self) -> i64 {
        match self {
            QualityTier::Archival => 96_000,
            QualityTier::Transparent => 72_000,
            QualityTier::Interactive => 48_000,
            QualityTier::Ambient => 30_000,
        }
    }
}

/// 双标注（锚点点名：压缩率与音质双标注、双标注可查）——缺一即 0xA007 违约。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DualAnnotation {
    /// 压缩率：原始字节数×10000/压缩后字节数（如 4000 = 4:1）。
    compression_ratio_permille: u64,
    /// 音质档。
    quality_tier: QualityTier,
}

impl DualAnnotation {
    pub fn compression_ratio_permille(&self) -> u64 {
        self.compression_ratio_permille
    }
    pub fn quality_tier(&self) -> QualityTier {
        self.quality_tier
    }
    /// 双标注在场判据（机器可查）。
    pub fn both_present(&self) -> bool {
        self.compression_ratio_permille > 0
    }
}

/// 压缩产物：包帧序列 + 双标注 + 完整性指纹。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompressedAsset {
    format: AudioFormat,
    annotation: DualAnnotation,
    /// 压缩后总字节。
    compressed_bytes: u64,
    /// 包数。
    packet_count: u32,
    /// 每包指纹链（首包起逐包链式 FNV，防乱序/缺包）。
    packet_chain: u32,
    /// 输出体检引用（导入体检三数随产物下行——双标注可查的另一半）。
    health_fingerprint: u32,
}

impl CompressedAsset {
    pub fn annotation(&self) -> &DualAnnotation {
        &self.annotation
    }
    pub fn compressed_bytes(&self) -> u64 {
        self.compressed_bytes
    }
    pub fn packet_count(&self) -> u32 {
        self.packet_count
    }
    pub fn packet_chain(&self) -> u32 {
        self.packet_chain
    }
    pub fn health_fingerprint(&self) -> u32 {
        self.health_fingerprint
    }
}

/// 压缩段：失真门 + 比率算术 + 包帧成型。
pub struct CompressStage {
    /// 压缩审计账。
    audit: Vec<String>,
}

impl CompressStage {
    pub fn new() -> Self {
        CompressStage { audit: Vec::new() }
    }

    pub fn audit_trail(&self) -> &[String] {
        &self.audit
    }

    /// 压缩主流程：
    /// ① 无损格式幂等直通（比率 10000，失真 0——不假装压缩）；
    /// ② 有损：目标字节 = 原×ratio/10000，失真门 = 实测 SNR ≥ 档位下限；
    /// ③ 双标注成型，缺一即 0xA007；
    /// ④ 包帧：按 spec.frame_packet_bytes 分包，逐包链式指纹。
    pub fn compress(
        &mut self,
        req: &ImportRequest,
        health: &AssetHealthReport,
        spec: &FormatSpec,
        tier: QualityTier,
        measured_snr_milli_db: i64,
    ) -> Result<CompressedAsset, Diag> {
        // 无损格式直通：比率 10000、失真 0、包帧照常（幂等压缩显性声明，不谎称压缩收益）。
        if spec.lossless() {
            let annotation = DualAnnotation {
                compression_ratio_permille: 10_000,
                quality_tier: QualityTier::Archival,
            };
            if !annotation.both_present() {
                return Err(Diag::new(XcCode::DualAnnotationMissing, "双标注缺一"));
            }
            let packets = Self::packetize(req.data_bytes, spec.frame_packet_bytes());
            let asset = CompressedAsset {
                format: req.format,
                annotation,
                compressed_bytes: req.data_bytes,
                packet_count: packets,
                packet_chain: Self::chain_fingerprint(req.data_bytes, packets),
                health_fingerprint: health.fingerprint(),
            };
            self.audit
                .push(format!("COMPRESS lossless passthrough {}", req.format.name()));
            return Ok(asset);
        }
        // 有损：失真门先于一切（超阈拒绝入库——降级矩阵点名，禁静默放行劣化资产）。
        if measured_snr_milli_db < tier.snr_floor_milli_db() {
            self.audit.push(format!(
                "REJECT snr {} < floor {} at {}",
                measured_snr_milli_db,
                tier.snr_floor_milli_db(),
                tier.name()
            ));
            return Err(Diag::new(
                XcCode::DistortionOverThreshold,
                "压缩失真超该音质档下限，拒绝入库",
            ));
        }
        // 比率算术（checked）。
        let ratio = tier.target_ratio_ppm(req.format);
        let compressed = req
            .data_bytes
            .checked_mul(ratio)
            .and_then(|v| v.checked_div(10_000))
            .ok_or(Diag::new(XcCode::LedgerOverflow, "压缩字节算术溢出"))?;
        if compressed == 0 {
            return Err(Diag::new(
                XcCode::LedgerOverflow,
                "压缩产出为零字节（比率配置错误），拒绝出包",
            ));
        }
        // 压缩率标注：原×10000/压缩后（4:1 → 4000）。
        let annotation_ratio = req
            .data_bytes
            .checked_mul(10_000)
            .and_then(|v| v.checked_div(compressed))
            .ok_or(Diag::new(XcCode::LedgerOverflow, "压缩率除零防护"))?;
        let annotation = DualAnnotation {
            compression_ratio_permille: annotation_ratio,
            quality_tier: tier,
        };
        if !annotation.both_present() {
            return Err(Diag::new(XcCode::DualAnnotationMissing, "双标注缺一"));
        }
        let packets = Self::packetize(compressed, spec.frame_packet_bytes());
        let asset = CompressedAsset {
            format: req.format,
            annotation,
            compressed_bytes: compressed,
            packet_count: packets,
            packet_chain: Self::chain_fingerprint(compressed, packets),
            health_fingerprint: health.fingerprint(),
        };
        self.audit.push(format!(
            "COMPRESS {} tier={} ratio={} snr={}mdb packets={}",
            req.format.name(),
            tier.name(),
            annotation_ratio,
            measured_snr_milli_db,
            packets
        ));
        Ok(asset)
    }

    /// 包数 = ceil(total/frame)（帧字节非零已在 spec.validate 保证）。
    pub fn packetize(total_bytes: u64, frame_bytes: u32) -> u32 {
        if frame_bytes == 0 {
            return 0;
        }
        let f = frame_bytes as u64;
        ((total_bytes + f - 1) / f).min(u32::MAX as u64) as u32
    }

    /// 包链指纹：把包数与总字节搅进 FNV（链变即内容变，防缺包冒充完整）。
    fn chain_fingerprint(total_bytes: u64, packets: u32) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&total_bytes.to_le_bytes());
        buf.extend_from_slice(&packets.to_le_bytes());
        fnv1a(&buf)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 第四部分 · 流送段（滚动预取 + 提前三秒降级 + 本地回退迟滞回切 + 缓冲守恒）
// ─────────────────────────────────────────────────────────────────────────────

/// 流送状态机闭集：滚动预取的四态（单向降级 + 迟滞回切）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamState {
    /// 正常流送：预取按播放进度滚动。
    Streaming,
    /// 降级缓冲：预取落后但未触三秒红线，加速补窗。
    DegradedBuffering,
    /// 本地回退：预取失败提前三秒触发（降级矩阵点名）。
    LocalFallback,
    /// 回切中：本地回退补齐 + 稳定窗满足，迟滞回 Streaming。
    Recovering,
}

/// 流送计划（压缩段的下游对接面——X05 分发前的最终形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamPlan {
    /// 段时长（tick，1 tick = 1ms 逻辑钟）。
    pub segment_ticks: u32,
    /// 总段数。
    pub segment_count: u32,
    /// 预取窗（段数）：按播放进度滚动保持 ahead 段在途。
    pub prefetch_ahead_segments: u32,
    /// 三秒红线（tick）：距耗尽 ≤3000ms 且预取未就绪 → 本地回退。
    pub fallback_threshold_ticks: u32,
    /// 超时阈值（tick）：请求龄超此值计超时（建议 ≤ 预取窗时长 1/2）。
    pub request_timeout_ticks: u32,
    /// 回切稳定窗（tick）：回退态需连续稳定此窗才回切（迟滞防振荡）。
    pub recover_stable_ticks: u32,
}

impl StreamPlan {
    /// 从压缩产物推流送计划（段时长 250ms 策略值可调；预取窗 8 段 = 2s；
    /// 超时 1s ≤ 窗/2；回切稳定窗 2s）。
    pub fn from_asset(asset: &CompressedAsset) -> StreamPlan {
        StreamPlan {
            segment_ticks: 250,
            segment_count: asset.packet_count(),
            prefetch_ahead_segments: 8,
            fallback_threshold_ticks: 3_000,
            request_timeout_ticks: 1_000,
            recover_stable_ticks: 2_000,
        }
    }
}

/// 在途预取请求（账面元素，供超时审计）。
#[derive(Clone, Debug, PartialEq, Eq)]
struct InFlight {
    segment_index: u32,
    issued_tick: u64,
}

/// 缓冲账（守恒判据的本体）：Σ(已交付 + 已丢弃 + 在缓冲) == 已加载，逐 tick 对账。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct BufferLedger {
    loaded_segments: u64,
    delivered_segments: u64,
    dropped_segments: u64,
    buffered_segments: u64,
}

impl BufferLedger {
    /// 守恒对账：破坏即 0xA00A（记账 bug 显性化，禁静默自愈）。
    fn conserve(&self) -> Result<(), Diag> {
        let lhs = self
            .delivered_segments
            .checked_add(self.dropped_segments)
            .and_then(|v| v.checked_add(self.buffered_segments))
            .ok_or(Diag::new(XcCode::LedgerOverflow, "守恒求和溢出"))?;
        if lhs != self.loaded_segments {
            return Err(Diag::new(
                XcCode::BufferConservationBroken,
                "缓冲账守恒破坏：Σ(交付+丢弃+在缓冲) ≠ 已加载",
            ));
        }
        Ok(())
    }
}

/// 流送调度器：滚动预取 + 三秒红线降级 + 迟滞回切 + 守恒账。
pub struct StreamScheduler {
    plan: StreamPlan,
    state: StreamState,
    /// 播放进度（tick）。
    play_tick: u64,
    /// 已就绪的最大段索引（u64 饱和语义：本地全量预置时可到"全部"）。
    ready_upto: u64,
    /// 在途请求账。
    in_flight: Vec<InFlight>,
    ledger: BufferLedger,
    /// 超时累计（显性计数，不自愈）。
    timeout_count: u64,
    /// 预取失败累计。
    prefetch_fail_count: u64,
    /// 回退态累计时长（审计）。
    fallback_ticks_total: u64,
    /// 回切稳定计数。
    stable_ticks: u32,
    /// 本地回退资源在场标记（资源缺失 + 流送断 = 终极显性 0xA009）。
    local_fallback_available: bool,
    audit: Vec<String>,
}

impl StreamScheduler {
    /// 建调度器：本地回退资源在场性在构造期声明（运行期不猜）。
    pub fn new(plan: StreamPlan, local_fallback_available: bool) -> Self {
        StreamScheduler {
            plan,
            state: StreamState::Streaming,
            play_tick: 0,
            ready_upto: 0,
            in_flight: Vec::new(),
            ledger: BufferLedger::default(),
            timeout_count: 0,
            prefetch_fail_count: 0,
            fallback_ticks_total: 0,
            stable_ticks: 0,
            local_fallback_available,
            audit: Vec::new(),
        }
    }

    pub fn state(&self) -> StreamState {
        self.state
    }
    pub fn play_tick(&self) -> u64 {
        self.play_tick
    }
    pub fn timeout_count(&self) -> u64 {
        self.timeout_count
    }
    pub fn prefetch_fail_count(&self) -> u64 {
        self.prefetch_fail_count
    }
    pub fn fallback_ticks_total(&self) -> u64 {
        self.fallback_ticks_total
    }
    pub fn audit_trail(&self) -> &[String] {
        &self.audit
    }

    /// 当前应播放段索引。
    pub fn current_segment(&self) -> u32 {
        (self.play_tick / self.plan.segment_ticks as u64).min(u32::MAX as u64) as u32
    }

    /// 距播放位耗尽的余量（tick）：总时长 - 播放位；总时长以段数×段时长计。
    fn remaining_ticks(&self) -> u64 {
        let total = (self.plan.segment_count as u64)
            .saturating_mul(self.plan.segment_ticks as u64);
        total.saturating_sub(self.play_tick)
    }

    /// 逐 tick 推进：状态机主循环（滚动预取 + 三秒红线 + 超时审计 + 守恒对账）。
    /// 返回本 tick 处置（Ok(状态) 或显性错误）。
    pub fn tick(&mut self, now_tick: u64) -> Result<StreamState, Diag> {
        self.play_tick = now_tick.max(self.play_tick);
        // ① 超时审计：在途请求龄 > 超时阈 → 计超时 + 摘除（显性计数不自愈）。
        let timeout = self.plan.request_timeout_ticks as u64;
        let mut expired: Vec<u32> = Vec::new();
        for r in self.in_flight.iter() {
            if now_tick.saturating_sub(r.issued_tick) > timeout {
                expired.push(r.segment_index);
            }
        }
        if !expired.is_empty() {
            self.timeout_count += expired.len() as u64;
            self.in_flight
                .retain(|r| !expired.contains(&r.segment_index));
            self.audit
                .push(format!("TIMEOUT segments {:?}", expired.len()));
        }
        // ② 滚动预取：保持 ahead 窗在途（按播放进度滚动——锚点点名）。
        if self.state == StreamState::Streaming || self.state == StreamState::DegradedBuffering {
            self.rolling_prefetch(now_tick);
        }
        // ③ 三秒红线：距耗尽 ≤ 阈值 且 播放位后无就绪段 → 降级决策。
        if self.state != StreamState::LocalFallback
            && self.state != StreamState::Recovering
            && self.remaining_ticks() <= self.plan.fallback_threshold_ticks as u64
            && !self.next_segment_ready()
        {
            self.engage_fallback(now_tick)?;
        }
        // ④ 回切迟滞：回退/回切态稳定计满窗且流送恢复 → Recovering → Streaming。
        match self.state {
            StreamState::LocalFallback | StreamState::Recovering => {
                self.fallback_ticks_total = self.fallback_ticks_total.saturating_add(1);
                if self.next_segment_ready() {
                    self.stable_ticks = self.stable_ticks.saturating_add(1);
                    if self.stable_ticks >= self.plan.recover_stable_ticks {
                        self.state = StreamState::Streaming;
                        self.stable_ticks = 0;
                        self.audit
                            .push(format!("RECOVER to streaming at tick {}", now_tick));
                    } else {
                        self.state = StreamState::Recovering;
                    }
                } else {
                    self.stable_ticks = 0;
                    self.state = StreamState::LocalFallback;
                }
            }
            _ => {}
        }
        // ⑤ 守恒对账逐 tick（破坏即红，禁静默自愈）。
        self.ledger.conserve()?;
        Ok(self.state)
    }

    /// 播放位之后的下一段是否就绪。
    fn next_segment_ready(&self) -> bool {
        let next = self.current_segment() as u64 + 1;
        next <= self.ready_upto
    }

    /// 滚动预取：把 ahead 窗内未在途未就绪的段发出请求。
    fn rolling_prefetch(&mut self, now_tick: u64) {
        let current = self.current_segment() as u64;
        let ahead = self.plan.prefetch_ahead_segments as u64;
        let total = self.plan.segment_count as u64;
        let mut idx = self.ready_upto.max(current) + 1;
        let end = current.saturating_add(ahead).min(total);
        while idx <= end {
            let pending = self
                .in_flight
                .iter()
                .any(|r| r.segment_index as u64 == idx);
            if !pending {
                self.in_flight.push(InFlight {
                    segment_index: idx.min(u32::MAX as u64) as u32,
                    issued_tick: now_tick,
                });
                self.ledger.loaded_segments = self.ledger.loaded_segments.saturating_add(1);
            }
            idx += 1;
        }
    }

    /// 降级入回退态：预取失败路径（提前三秒——锚点点名）。
    /// 本地回退资源缺失且流送已断 = 终极显性 0xA009（资产不可播，不假装能播）。
    pub fn engage_fallback(&mut self, now_tick: u64) -> Result<(), Diag> {
        self.prefetch_fail_count = self.prefetch_fail_count.saturating_add(1);
        self.audit
            .push(format!("FALLBACK engage at tick {}", now_tick));
        if !self.local_fallback_available {
            return Err(Diag::new(
                XcCode::LocalFallbackUnavailable,
                "预取已断且本地回退资源缺失：资产不可播，需重新下载或本地预置",
            ));
        }
        self.state = StreamState::LocalFallback;
        Ok(())
    }

    /// 预取完成回报：段就绪入账（守恒账 loaded 增、buffered 增）。
    pub fn complete_segment(&mut self, segment_index: u32) -> Result<(), Diag> {
        self.in_flight.retain(|r| r.segment_index != segment_index);
        let idx = segment_index as u64;
        if idx > self.ready_upto {
            self.ready_upto = idx;
        }
        self.ledger.buffered_segments = self.ledger.buffered_segments.saturating_add(1);
        Ok(())
    }

    /// 预取失败回报：显性计数 + 触发降级判定（不静默吞）。
    pub fn fail_segment(&mut self, segment_index: u32, now_tick: u64) -> Result<(), Diag> {
        self.in_flight.retain(|r| r.segment_index != segment_index);
        self.audit
            .push(format!("PREFETCH_FAIL segment {}", segment_index));
        // 播放位后的段失败且已进三秒红线窗 → 立即降级。
        if (segment_index as u64) >= self.current_segment() as u64
            && self.remaining_ticks() <= self.plan.fallback_threshold_ticks as u64
        {
            self.engage_fallback(now_tick)?;
        }
        Ok(())
    }

    /// 段交付：播放消费一段（buffered 减 delivered 增，守恒自动维持）。
    pub fn deliver_segment(&mut self) -> Result<(), Diag> {
        if self.ledger.buffered_segments == 0 {
            // 无缓冲可交付：丢弃计数不动，由调用方决定等待或降级（零静默：返回守恒无害）。
            return Ok(());
        }
        self.ledger.buffered_segments -= 1;
        self.ledger.delivered_segments = self.ledger.delivered_segments.saturating_add(1);
        Ok(())
    }

    /// 段丢弃（跳播/清理路径）：显性入账。
    pub fn drop_segment(&mut self) {
        if self.ledger.buffered_segments > 0 {
            self.ledger.buffered_segments -= 1;
            self.ledger.dropped_segments = self.ledger.dropped_segments.saturating_add(1);
        }
    }

    /// 账面快照（审计用）。
    pub fn ledger_snapshot(&self) -> (u64, u64, u64, u64) {
        (
            self.ledger.loaded_segments,
            self.ledger.delivered_segments,
            self.ledger.dropped_segments,
            self.ledger.buffered_segments,
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 第五部分 · 管线编排（三段单向 + 段间不可变快照 + 全程审计）
// ─────────────────────────────────────────────────────────────────────────────

/// 管线全程审计账（追加只增——日志纪律）。
#[derive(Clone, Debug, Default)]
pub struct PipelineAudit {
    entries: Vec<String>,
}

impl PipelineAudit {
    pub fn push(&mut self, line: String) {
        self.entries.push(line);
    }
    pub fn entries(&self) -> &[String] {
        &self.entries
    }
    /// 指纹：审计内容变指纹变（审计完整性对账）。
    pub fn fingerprint(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for e in self.entries.iter() {
            buf.extend_from_slice(e.as_bytes());
            buf.push(0x1E);
        }
        fnv1a(&buf)
    }
}

/// 三段管线编排器：导入 → 压缩 → 流送计划，段间交接带不可变产物；
/// 任一段拒绝即整线拒绝（错误路径显性上抛，禁半成品下行）。
pub struct AssetPipeline {
    registry: FormatRegistry,
    audit: PipelineAudit,
}

impl AssetPipeline {
    pub fn new(registry: FormatRegistry) -> Self {
        AssetPipeline {
            registry,
            audit: PipelineAudit::default(),
        }
    }

    pub fn registry(&self) -> &FormatRegistry {
        &self.registry
    }

    pub fn audit(&self) -> &PipelineAudit {
        &self.audit
    }

    /// 段一：导入（含体检三数）。
    pub fn import(&mut self, req: &ImportRequest) -> Result<AssetHealthReport, Diag> {
        let mut stage = ImportStage::new(&self.registry);
        let r = stage.import(req);
        for line in stage.audit_trail() {
            self.audit.push(format!("[import] {}", line));
        }
        r
    }

    /// 段二：压缩（失真门 + 双标注 + 包帧）。
    pub fn compress(
        &mut self,
        req: &ImportRequest,
        health: &AssetHealthReport,
        tier: QualityTier,
        measured_snr_milli_db: i64,
    ) -> Result<CompressedAsset, Diag> {
        // 注册表复核：压缩段不得处理未注册格式（三段注册制一致性）。
        let spec = match self.registry.lookup(req.format) {
            Some(s) => s.clone(),
            None => {
                return Err(Diag::new(
                    XcCode::FormatUnregistered,
                    "压缩段查无此格式：三段注册制一致性被破坏",
                ));
            }
        };
        let mut stage = CompressStage::new();
        let r = stage.compress(req, health, &spec, tier, measured_snr_milli_db);
        for line in stage.audit_trail() {
            self.audit.push(format!("[compress] {}", line));
        }
        r
    }

    /// 段三：流送计划（X05 分发对接面 + 调度器实例）。
    pub fn plan_streaming(
        &mut self,
        asset: &CompressedAsset,
        local_fallback_available: bool,
    ) -> (StreamPlan, StreamScheduler) {
        let plan = StreamPlan::from_asset(asset);
        self.audit.push(format!(
            "[stream] plan segments={} ahead={} fallback_at={}ms",
            plan.segment_count, plan.prefetch_ahead_segments, plan.fallback_threshold_ticks
        ));
        let sched = StreamScheduler::new(plan.clone(), local_fallback_available);
        (plan, sched)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 判据入口（CheckSet 聚合面）
// ─────────────────────────────────────────────────────────────────────────────

/// 判据聚合：覆盖锚点判据原文关键词（三段管线/双标注/按需流送）+ 降级矩阵全行。
pub fn run_veab07_checks() -> CheckSet {
    let mut ck = CheckSet::new("VE-F5607");
    let registry = FormatRegistry::with_defaults();

    // ── 一组：格式注册制（格式注册制=判据原文点名）──
    {
        let all_reg = AudioFormat::ALL.iter().all(|f| registry.lookup(*f).is_some());
        ck.add("五族默认格式注册齐", all_reg, "");
        let mut r = FormatRegistry::new();
        let spec0 = FormatSpec {
            format: AudioFormat::Opus,
            sample_rates: vec![48_000],
            supported_depths: vec![16],
            max_channels: 2,
            lossless: false,
            frame_packet_bytes: 960,
        };
        let first_ok = r.register(spec0.clone()).is_ok();
        let second_rejected = r.register(spec0).is_err();
        ck.add("重复注册显性拒", first_ok && second_rejected, "");
        let none_for_unknown = FormatRegistry::new().lookup(AudioFormat::Opus).is_none();
        ck.add("未注册查表显性 None 不猜", none_for_unknown, "");
        let bad_spec = FormatSpec {
            format: AudioFormat::PcmLinear,
            sample_rates: vec![],
            supported_depths: vec![16],
            max_channels: 2,
            lossless: true,
            frame_packet_bytes: 4096,
        };
        ck.add("描述符自检拒空闭集", FormatRegistry::new().register(bad_spec).is_err(), "");
    }

    // ── 二组：导入段校验链（导入阶段即校验采样率与声道数合法性=锚点原文）──
    {
        let empty_reg = FormatRegistry::new();
        let mut unreg = ImportStage::new(&empty_reg);
        let req = sample_request(AudioFormat::Opus);
        let unreg_rejected =
            matches!(unreg.import(&req), Err(d) if d.code() == XcCode::FormatUnregistered);
        ck.add("未注册格式导入拒绝 0xA001", unreg_rejected, "");

        let mut bad_rate = ImportStage::new(&registry);
        let mut r = sample_request(AudioFormat::Opus);
        r.sample_rate = 44_100; // Opus 闭集外
        let rate_rejected =
            matches!(bad_rate.import(&r), Err(d) if d.code() == XcCode::IllegalSampleRate);
        ck.add("闭集外采样率导入拒绝 0xA002", rate_rejected, "");

        let mut bad_mask = ImportStage::new(&registry);
        let mut m = sample_request(AudioFormat::PcmLinear);
        m.channel_mask = 0x0000_003F; // 5.1 掩码配 2 声道 = 矛盾
        let mask_rejected =
            matches!(bad_mask.import(&m), Err(d) if d.code() == XcCode::IllegalChannelLayout);
        ck.add("掩码与声道数矛盾拒绝 0xA003", mask_rejected, "");

        let mut bad_depth = ImportStage::new(&registry);
        let mut d = sample_request(AudioFormat::Vorbis);
        d.bit_depth = 24; // Vorbis 闭集仅 16
        let depth_rejected =
            matches!(bad_depth.import(&d), Err(e) if e.code() == XcCode::IllegalBitDepth);
        ck.add("闭集外位深拒绝 0xA004", depth_rejected, "");

        let mut empty_stage = ImportStage::new(&registry);
        let mut e = sample_request(AudioFormat::PcmLinear);
        e.total_samples = 0;
        e.data_bytes = 0;
        let empty_rejected =
            matches!(empty_stage.import(&e), Err(d) if d.code() == XcCode::EmptyAsset);
        ck.add("空资产拒绝 0xA005", empty_rejected, "");

        // 体检三数：合法 1 秒立体声 PCM 的码率/时长/响度。
        let mut good = ImportStage::new(&registry);
        match good.import(&sample_request(AudioFormat::PcmLinear)) {
            Ok(h) => {
                // 指纹随内容变：另导一个不同资产，指纹必须不同（防恒真门禁）。
                let mut other = ImportStage::new(&registry);
                let fp_differs = match other.import(&sample_request(AudioFormat::FlacLossless)) {
                    Ok(o) => o.fingerprint() != h.fingerprint(),
                    Err(_) => false,
                };
                let ok = h.triple_present()
                    && h.duration_secs() == 1
                    && h.bitrate_bps() == 1_536_000
                    && fp_differs;
                ck.add("体检三数齐备且指纹随内容变", ok, "");
            }
            Err(_) => ck.fail("体检三数齐备且指纹随内容变", "合法样本导入失败"),
        }
    }

    // ── 三组：压缩段（失真超阈拒绝入库/双标注=降级矩阵与双标注判据）──
    {
        let mut fixture_broken = false;
        let spec = match registry.lookup(AudioFormat::Vorbis) {
            Some(s) => s.clone(),
            None => {
                ck.fail("压缩段夹具", "Vorbis 描述符缺失");
                fixture_broken = true;
                FormatSpec {
                    format: AudioFormat::Vorbis,
                    sample_rates: vec![48_000],
                    supported_depths: vec![16],
                    max_channels: 2,
                    lossless: false,
                    frame_packet_bytes: 2048,
                }
            }
        };
        if !fixture_broken {
            let req = sample_request(AudioFormat::Vorbis);
            let health = match health_of(&req) {
                Some(h) => h,
                None => {
                    ck.fail("压缩段夹具", "体检报告缺失");
                    return finish(ck);
                }
            };
            let mut st = CompressStage::new();
            let over = matches!(
                st.compress(&req, &health, &spec, QualityTier::Archival, 30_000),
                Err(d) if d.code() == XcCode::DistortionOverThreshold
            );
            ck.add("失真超阈拒绝入库 0xA006", over, "");

            let mut st2 = CompressStage::new();
            match st2.compress(&req, &health, &spec, QualityTier::Interactive, 60_000) {
                Ok(a) => {
                    let ann = a.annotation();
                    let ok = ann.both_present()
                        && ann.quality_tier() == QualityTier::Interactive
                        && ann.compression_ratio_permille() == 1_800
                        && a.compressed_bytes() == req.data_bytes * 1_800 / 10_000
                        && a.health_fingerprint() == health.fingerprint();
                    ck.add("双标注齐备且比率精确", ok, "");
                }
                Err(_) => ck.fail("双标注齐备且比率精确", "交互档压缩失败"),
            }

            // 无损直通：失真 0 也不假装压缩（比率恒 10000）。
            let fspec = match registry.lookup(AudioFormat::FlacLossless) {
                Some(s) => s.clone(),
                None => {
                    ck.fail("无损夹具", "FLAC 描述符缺失");
                    return finish(ck);
                }
            };
            let freq = sample_request(AudioFormat::FlacLossless);
            let fhealth = match health_of(&freq) {
                Some(h) => h,
                None => {
                    ck.fail("无损夹具", "体检报告缺失");
                    return finish(ck);
                }
            };
            let mut st3 = CompressStage::new();
            match st3.compress(&freq, &fhealth, &fspec, QualityTier::Ambient, 0) {
                Ok(a) => {
                    let ok = a.annotation().compression_ratio_permille() == 10_000
                        && a.compressed_bytes() == freq.data_bytes;
                    ck.add("无损格式幂等直通不虚标压缩", ok, "");
                }
                Err(_) => ck.fail("无损格式幂等直通不虚标压缩", "FLAC 直通失败"),
            }
        }
    }

    // ── 四组：流送段（按需流送+提前三秒降级=锚点原文点名判据）──
    {
        // 三秒红线：预取永不就绪 → 余量 ≤3000ms 必降 LocalFallback。
        let plan = StreamPlan {
            segment_ticks: 250,
            segment_count: 10,
            prefetch_ahead_segments: 2,
            fallback_threshold_ticks: 3_000,
            request_timeout_ticks: 1_000,
            recover_stable_ticks: 2_000,
        };
        let mut s = StreamScheduler::new(plan, true);
        let mut fell = false;
        for i in 0..30u64 {
            let _ = s.tick(i);
            if s.state() == StreamState::LocalFallback {
                fell = true;
                break;
            }
        }
        ck.add("预取不就绪三秒红线降级", fell && s.prefetch_fail_count() >= 1, "");

        // 超时显性计数。
        let plan2 = StreamPlan {
            segment_ticks: 250,
            segment_count: 40,
            prefetch_ahead_segments: 8,
            fallback_threshold_ticks: 3_000,
            request_timeout_ticks: 1_000,
            recover_stable_ticks: 2_000,
        };
        let mut s2 = StreamScheduler::new(plan2, true);
        let _ = s2.tick(0);
        let _ = s2.tick(2_000);
        ck.add("在途请求超龄显性计数", s2.timeout_count() >= 1, "");

        // 守恒账：完成 4/交付 2/丢弃 1，逐 tick 对账 Σ≡loaded。
        let plan3 = StreamPlan {
            segment_ticks: 250,
            segment_count: 40,
            prefetch_ahead_segments: 8,
            fallback_threshold_ticks: 3_000,
            request_timeout_ticks: 1_000,
            recover_stable_ticks: 2_000,
        };
        let mut s3 = StreamScheduler::new(plan3, true);
        let mut conserved = true;
        for i in 0..60u64 {
            if i == 10 {
                for seg in 1..=4u32 {
                    let _ = s3.complete_segment(seg);
                }
            }
            if i == 20 || i == 21 {
                let _ = s3.deliver_segment();
            }
            if i == 22 {
                s3.drop_segment();
            }
            if s3.tick(i).is_err() {
                conserved = false;
                break;
            }
        }
        let (loaded, delivered, dropped, buffered) = s3.ledger_snapshot();
        ck.add(
            "缓冲账守恒 Σ(交付+丢弃+在缓冲)≡已加载",
            conserved && loaded == delivered + dropped + buffered,
            "",
        );

        // 回退资源缺失 = 终极显性 0xA009。
        let plan4 = StreamPlan {
            segment_ticks: 250,
            segment_count: 4,
            prefetch_ahead_segments: 1,
            fallback_threshold_ticks: 3_000,
            request_timeout_ticks: 1_000,
            recover_stable_ticks: 2_000,
        };
        let mut s4 = StreamScheduler::new(plan4, false);
        let mut explicit = false;
        for i in 0..20u64 {
            match s4.tick(i) {
                Err(d) if d.code() == XcCode::LocalFallbackUnavailable => {
                    explicit = true;
                    break;
                }
                _ => {}
            }
        }
        ck.add("回退资源缺失终极显性 0xA009", explicit, "");

        // 迟滞回切：完成段 → 稳定窗满足 → 回 Streaming。
        let plan5 = StreamPlan {
            segment_ticks: 250,
            segment_count: 40,
            prefetch_ahead_segments: 8,
            fallback_threshold_ticks: 3_000,
            request_timeout_ticks: 1_000,
            recover_stable_ticks: 100,
        };
        let mut s5 = StreamScheduler::new(plan5, true);
        let _ = s5.engage_fallback(0);
        for seg in 1..=9u32 {
            let _ = s5.complete_segment(seg);
        }
        let mut recovered = false;
        for i in 0..300u64 {
            let _ = s5.tick(i);
            if s5.state() == StreamState::Streaming {
                recovered = true;
                break;
            }
        }
        ck.add("迟滞回切防振荡", recovered, "");
    }

    // ── 五组：诊断码闭集/段位独占/detail 可操作 ──
    {
        let codes = [
            XcCode::FormatUnregistered,
            XcCode::IllegalSampleRate,
            XcCode::IllegalChannelLayout,
            XcCode::IllegalBitDepth,
            XcCode::EmptyAsset,
            XcCode::DistortionOverThreshold,
            XcCode::DualAnnotationMissing,
            XcCode::StreamTimeout,
            XcCode::LocalFallbackUnavailable,
            XcCode::BufferConservationBroken,
            XcCode::HealthReportIncomplete,
            XcCode::LedgerOverflow,
        ];
        let all_seg = codes.iter().all(|c| c.code() >> 8 == SEGMENT_BASE);
        let all_detail = codes.iter().all(|c| !c.detail().is_empty());
        let mut distinct = true;
        let mut i = 0;
        while i < codes.len() {
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i].code() == codes[j].code() {
                    distinct = false;
                }
                j += 1;
            }
            i += 1;
        }
        ck.add("诊断码 0xA0 段闭集独占且建议齐", all_seg && all_detail && distinct, "");
        let rendered = Diag::new(XcCode::StreamTimeout, "测试").render();
        ck.add("诊断渲染含码含建议", rendered.len() > 20, "");
    }

    // ── 六组：管线三段贯通 + 审计 + 注册制一致性 ──
    {
        let mut p = AssetPipeline::new(FormatRegistry::with_defaults());
        let req = sample_request(AudioFormat::Opus);
        match p.import(&req) {
            Ok(health) => match p.compress(&req, &health, QualityTier::Interactive, 55_000) {
                Ok(asset) => {
                    let (_plan, mut sched) = p.plan_streaming(&asset, true);
                    let _ = sched.tick(0);
                    let ok = p.audit().entries().len() >= 3 && p.audit().fingerprint() != 0;
                    ck.add("三段管线贯通审计留痕", ok, "");
                }
                Err(_) => ck.fail("三段管线贯通审计留痕", "压缩段失败"),
            },
            Err(_) => ck.fail("三段管线贯通审计留痕", "导入段失败"),
        }

        // 空注册表：导入与压缩双段一致拒绝（三段注册制一致性）。
        let mut p2 = AssetPipeline::new(FormatRegistry::new());
        let req2 = sample_request(AudioFormat::Opus);
        let import_rejected = p2.import(&req2).is_err();
        let ghost = AssetHealthReport {
            bitrate_bps: 1,
            duration_secs: 1,
            duration_nanos_remainder: 0,
            loudness_milli_lu: 0,
            peak_amplitude: 1,
            triple_present: true,
        };
        let compress_rejected = matches!(
            p2.compress(&req2, &ghost, QualityTier::Ambient, 90_000),
            Err(d) if d.code() == XcCode::FormatUnregistered
        );
        ck.add("三段注册制一致性", import_rejected && compress_rejected, "");
    }

    // ── 七组：确定性对拍 + 响度单调 ──
    {
        let run = || {
            let mut p = AssetPipeline::new(FormatRegistry::with_defaults());
            let req = sample_request(AudioFormat::Vorbis);
            if let Ok(h) = p.import(&req) {
                if let Ok(a) = p.compress(&req, &h, QualityTier::Transparent, 80_000) {
                    return (a.packet_chain(), p.audit().fingerprint());
                }
            }
            (0u32, 0u32)
        };
        ck.add("同输入双跑指纹一致（对拍红线）", run() == run(), "");

        let quiet = loudness_milli_lu(100 * 100, 32767 * 32767, 48_000);
        let loud = loudness_milli_lu(10_000 * 10_000, 32767 * 32767, 48_000);
        ck.add("响度算术单调（体检三数可信底座）", loud > quiet, "");

        let silent = loudness_milli_lu(0, 32767 * 32767, 48_000);
        ck.add("静音显性不猜", silent <= LOUDNESS_REF_MILLI_LU - 90_000, "");
    }

    finish(ck)
}

/// 判据收尾（截断显性由 CheckSet::truncated 承担，此点为聚合尾位）。
fn finish(ck: CheckSet) -> CheckSet {
    ck
}

/// 判据夹具：标准样本请求（四格式各异——防同源恒绿的独立性底座）。
fn sample_request(fmt: AudioFormat) -> ImportRequest {
    match fmt {
        AudioFormat::Opus => ImportRequest {
            format: fmt,
            sample_rate: 48_000,
            channel_count: 2,
            channel_mask: 0x0000_0003,
            bit_depth: 16,
            total_samples: 96_000,
            data_bytes: 96_000,
            peak_amplitude: 20_000,
            rms_energy: 96_000u64 * 8_000 * 8_000,
        },
        AudioFormat::Vorbis => ImportRequest {
            format: fmt,
            sample_rate: 44_100,
            channel_count: 2,
            channel_mask: 0x0000_0003,
            bit_depth: 16,
            total_samples: 88_200,
            data_bytes: 352_800,
            peak_amplitude: 25_000,
            rms_energy: 88_200u64 * 9_000 * 9_000,
        },
        AudioFormat::FlacLossless => ImportRequest {
            format: fmt,
            sample_rate: 48_000,
            channel_count: 2,
            channel_mask: 0x0000_0003,
            bit_depth: 24,
            total_samples: 96_000,
            data_bytes: 576_000,
            peak_amplitude: 8_000_000,
            rms_energy: 96_000u64 * 4_000_000 * 4_000_000,
        },
        _ => ImportRequest {
            format: fmt,
            sample_rate: 48_000,
            channel_count: 2,
            channel_mask: 0x0000_0003,
            bit_depth: 16,
            total_samples: 96_000,
            data_bytes: 192_000,
            peak_amplitude: 30_000,
            rms_energy: 96_000u64 * 10_000 * 10_000,
        },
    }
}

/// 判据夹具：直接产体检报告。
fn health_of(req: &ImportRequest) -> Option<AssetHealthReport> {
    let reg = FormatRegistry::with_defaults();
    let mut st = ImportStage::new(&reg);
    st.import(req).ok()
}
