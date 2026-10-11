//! VE-F5607 · 音频资产管线（VE-STAR-II AB 域 · 音频域 · AB07 组 · 深化版）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5607`
//!
//! **判据（锚点原文）**：三段管线、双标注、按需流送、判据。
//!
//! **职责定位（锚点原文）**：音频资产管线——导入、压缩、流送三段
//! （格式注册制、压缩率与音质双标注、流送按需加载）；导入阶段即校验
//! 采样率与声道数合法性；流送预取按播放进度滚动，预取失败提前三秒
//! 降级本地回退；管线含资产体检报告——导入完成的资产附码率、时长、
//! 响度三数。错误路径：格式不支持→明确拒绝；压缩失真超阈→拒绝入库；
//! 流送超时→本地回退。性能：处理 O(资产)。跨批对接点：X05 分发。
//! 无障碍与隐私：双标注可查。
//!
//! 本深化版扩出的真实语义密度（全部纯功能，零 IO 零墙钟零浮点）：
//!
//! 1. **三段管线**（判据一）：[`ImportStage`]（元数据合法性闸）→
//!    [`CompressStage`]（失真预算闸 + 双标注落定）→
//!    [`StreamStage`]（分段预取 + 提前三秒降级回退），段间以
//!    [`AssetPipeline`] 总编排串接，处理 O(资产)。
//! 2. **格式注册制**：[`FormatRegistry`] 闭集注册（五格式缺省 +
//!    运行期追加），重复注册/未注册格式显性拒——"明确拒绝"落在
//!    [`XcCode::FormatUnknown`] 而非静默试探。
//! 3. **双标注**（判据二）：压缩率（bps 口径）与音质档
//!    （[`QualityGrade`] 三档）在压缩段一次落定、全程可查
//!    （[`AssetPipeline::annotate`] / 读屏行）——标注不是注释是账面。
//! 4. **按需流送**（判据三）：[`StreamSession`] 按播放进度滚动预取，
//!    预取窗 = [`PREFETCH_LEAD_MS`]（提前三秒——锚点原文数字）；段失败
//!    落在窗内即提前降级 [`StreamSession::fallback_active`]（本地回退），
//!    连续 [`RESTORE_CONFIRM`] 次成功显性恢复（不自动抖回）。
//! 5. **体检报告**：导入完成即附码率/时长/响度三数
//!    （[`HealthReport`]，响度十分之一 LU 定点——零浮点），三数缺一
//!    拒入库（[`XcCode::HealthFail`]）。
//! 6. **事件账本**：滚动 96 条七类事件 + FNV 链式防篡改
//!    （[`EventLedger::verify_chain`]）。
//! 7. **量账守恒**：段账 produced = delivered + fallback（丢弃恒 0，
//!    对拍 [`AssetPipeline::reconcile`]）。
//! 8. **X05 分发**：[`AssetPipeline::dispatch_x05`] 就绪资产分发清单
//!    （体检过 + 双标注齐 + 未被失真闸拒绝）。
//!
//! **对接**：F5605（音乐系统——曲库消费方）；F5606（语音——实时流
//! 另册）；F5608（场景图声源——听者面另册）。零 panic 面、零 IO、
//! 零墙钟（tick 注入）、零浮点、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（阈值与容量唯一源）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const AUDIO_ASSET_VERSION: &str = "AB07-audioassets-v1";

/// 合法采样率闭集（Hz；导入闸逐一对拍——表外即拒）。
pub const LEGAL_SAMPLE_RATES_HZ: [u32; 8] = [
    8000, 11025, 16000, 22050, 32000, 44100, 48000, 96000,
];

/// 合法声道数上限（1..=8；导入闸）。
pub const MAX_CHANNELS: u8 = 8;

/// 预取提前量（ms；锚点原文"提前三秒"——数字钉死在常量不散抄）。
pub const PREFETCH_LEAD_MS: u64 = 3000;

/// 流送段长（ms；每段一个预取单元）。
pub const SEGMENT_MS: u64 = 2000;

/// 预取窗内段数（提前量/段长；窗口整除常量化）。
pub const PREFETCH_WINDOW_SEGS: u64 = PREFETCH_LEAD_MS / SEGMENT_MS;

/// 失真入库阈值（百分比；超阈拒绝入库——锚点原文闸）。
pub const DISTORTION_MAX_PCT: u64 = 12;

/// 失真映射表（kbps → 失真百分比；闭集插值外推——零浮点查表）。
/// 来源：本域压缩预算缺省标定（128k 起 2%，64k 6%，32k 10%，24k 14%）。
pub const BITRATE_DISTORTION_TABLE: [(u64, u64); 5] = [
    (24, 14),
    (32, 10),
    (64, 6),
    (128, 2),
    (256, 1),
];

/// 音质三档阈值（失真百分比 ≤3 高保 / ≤8 中保 / 其余低保——被闸拒的
/// 根本不入库）。
pub const GRADE_HIGH_MAX_PCT: u64 = 3;
pub const GRADE_MID_MAX_PCT: u64 = 8;

/// 段失败回退确认数（连续 N 次成功才恢复远端——防抖不自动抖回）。
pub const RESTORE_CONFIRM: u32 = 8;

/// 单资产会话数上限（同资产可多会话；超限拒开）。
pub const MAX_SESSIONS_PER_ASSET: usize = 4;

/// 全管线资产上限（O(资产) 的规模护栏）。
pub const MAX_ASSETS: usize = 512;

/// 事件账容量（滚动）。
pub const EVENT_LEDGER_CAPACITY: usize = 96;

/// 体检报告响度合法域（十分之一 LU 定点：-700..=100）。
pub const LOUDNESS_MIN_TENTHS: i16 = -700;
pub const LOUDNESS_MAX_TENTHS: i16 = 100;

/// 会话恢复后窗内连续成功计数清零线（回落即清连击）。
pub const RESTORE_RESET_ON_FAIL: bool = true;

// ---------------------------------------------------------------------------
// 二、诊断码（0x53 段独占：AB07 音频资产面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x53 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 格式未注册（明确拒绝——锚点原文）。
    FormatUnknown,
    /// 采样率不在合法闭集。
    SampleRateIllegal,
    /// 声道数越界（0 或超 8）。
    ChannelCountIllegal,
    /// 压缩失真超阈（拒绝入库——锚点原文）。
    DistortionOverThreshold,
    /// 预取/流送超时（触发本地回退——锚点原文）。
    StreamTimeout,
    /// 格式重复注册。
    RegistryDuplicate,
    /// 资产不存在（按 id 未命中）。
    AssetNotFound,
    /// 播放头回退（进度必须单调）。
    PlayheadReverse,
    /// 体检三数缺一或越域。
    HealthFail,
    /// 段序空洞（交付必须连续）。
    SegmentGap,
    /// 事件账指纹链断裂。
    LedgerCorrupt,
    /// 会话数超限。
    SessionOverflow,
}

impl XcCode {
    /// 人读诊断（全带可操作建议）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::FormatUnknown => "格式未注册：管线明确拒绝不试探，建议先在 FormatRegistry 注册或换已注册格式",
            XcCode::SampleRateIllegal => "采样率不合法：不在 LEGAL_SAMPLE_RATES_HZ 闭集，建议重采样到闭集内再导入",
            XcCode::ChannelCountIllegal => "声道数越界：1..=8 之外（含 0），建议确认源文件声道布局",
            XcCode::DistortionOverThreshold => "压缩失真超阈：目标码率失真估计超过入库线，建议提高目标码率或保 PCM",
            XcCode::StreamTimeout => "流送超时：已触发本地回退，建议检查网络段与预取窗配置，回退期间本地续播",
            XcCode::RegistryDuplicate => "格式重复注册：同 id 已在册，建议复用在册条目或换 id",
            XcCode::AssetNotFound => "资产不存在：按 id 未命中，建议先 import 再压缩/流送，或核对 id",
            XcCode::PlayheadReverse => "播放头回退：进度必须单调不减，建议 seek 走显性重开流路径",
            XcCode::HealthFail => "体检不过：码率/时长/响度三数缺一或越域，建议补齐元数据再入库",
            XcCode::SegmentGap => "段序空洞：交付必须连续，建议核对预取器序号推进逻辑",
            XcCode::LedgerCorrupt => "事件账指纹链断裂：账面被篡改或乱序，建议以快照为准重建",
            XcCode::SessionOverflow => "会话数超限：同资产会话已达上限，建议复用在册会话或关闭闲置会话",
        }
    }

    /// 0x53 段十六进制码。
    pub const fn code_hex(self) -> u16 {
        match self {
            XcCode::FormatUnknown => 0x5301,
            XcCode::SampleRateIllegal => 0x5302,
            XcCode::ChannelCountIllegal => 0x5303,
            XcCode::DistortionOverThreshold => 0x5304,
            XcCode::StreamTimeout => 0x5305,
            XcCode::RegistryDuplicate => 0x5306,
            XcCode::AssetNotFound => 0x5307,
            XcCode::PlayheadReverse => 0x5308,
            XcCode::HealthFail => 0x5309,
            XcCode::SegmentGap => 0x530A,
            XcCode::LedgerCorrupt => 0x530B,
            XcCode::SessionOverflow => 0x530C,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、FNV-1a 指纹基元（事件链/快照共用）
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单字节混入。
pub const fn fnv1a_byte(state: u64, b: u8) -> u64 {
    (state ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a u64 定长混入（8 字节小端逐字节）。
pub const fn fnv1a_u64(state: u64, v: u64) -> u64 {
    let s1 = fnv1a_byte(state, (v & 0xff) as u8);
    let s2 = fnv1a_byte(s1, ((v >> 8) & 0xff) as u8);
    let s3 = fnv1a_byte(s2, ((v >> 16) & 0xff) as u8);
    let s4 = fnv1a_byte(s3, ((v >> 24) & 0xff) as u8);
    let s5 = fnv1a_byte(s4, ((v >> 32) & 0xff) as u8);
    let s6 = fnv1a_byte(s5, ((v >> 40) & 0xff) as u8);
    let s7 = fnv1a_byte(s6, ((v >> 48) & 0xff) as u8);
    fnv1a_byte(s7, ((v >> 56) & 0xff) as u8)
}

/// FNV-1a 字符串混入。
pub const fn fnv1a_str(state: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut st = state;
    while i < b.len() {
        st = fnv1a_byte(st, b[i]);
        i += 1;
    }
    st
}

// ---------------------------------------------------------------------------
// 四、格式注册制（闭集 + 运行期追加，重复/未注册显性拒）
// ---------------------------------------------------------------------------

/// 音频格式规格（注册条目）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioFormatSpec {
    /// 格式 id（注册面唯一键；0 保留不给注册）。
    pub format_id: u8,
    /// 格式名（人读；查重键之二）。
    pub name: String,
    /// 是否有损。
    pub lossy: bool,
    /// 声道数上限（≤ [`MAX_CHANNELS`]）。
    pub max_channels: u8,
    /// 是否支持流式分段（PCM/ADPCM 支持；叙事面声明）。
    pub streamable: bool,
}

/// 格式注册表（缺省五格式 + 追加制）。
#[derive(Clone, Debug)]
pub struct FormatRegistry {
    entries: Vec<AudioFormatSpec>,
    /// 未注册拒绝计数（明确拒绝的显性留痕）。
    pub unknown_rejects: u64,
    /// 重复注册拒绝计数。
    pub dup_rejects: u64,
}

impl FormatRegistry {
    /// 缺省注册表（五格式闭集：PCM/ADPCM/VORB/OPUX/FLAX）。
    pub fn with_defaults() -> FormatRegistry {
        let mut reg = FormatRegistry {
            entries: Vec::new(),
            unknown_rejects: 0,
            dup_rejects: 0,
        };
        let _ = reg.register(1, "PCM", false, 8, true);
        let _ = reg.register(2, "ADPCM", true, 2, true);
        let _ = reg.register(3, "VORB", true, 8, true);
        let _ = reg.register(4, "OPUX", true, 8, true);
        let _ = reg.register(5, "FLAX", false, 8, false);
        reg
    }

    /// 注册格式（id 0 拒；重复 id 或重名拒；声道上限越界拒）。
    pub fn register(
        &mut self,
        format_id: u8,
        name: &str,
        lossy: bool,
        max_channels: u8,
        streamable: bool,
    ) -> Result<(), XcCode> {
        if format_id == 0 || name.is_empty() {
            self.dup_rejects = self.dup_rejects.saturating_add(1);
            return Err(XcCode::RegistryDuplicate);
        }
        if max_channels == 0 || max_channels > MAX_CHANNELS {
            self.dup_rejects = self.dup_rejects.saturating_add(1);
            return Err(XcCode::ChannelCountIllegal);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.format_id == format_id || e.name == name {
                    self.dup_rejects = self.dup_rejects.saturating_add(1);
                    return Err(XcCode::RegistryDuplicate);
                }
            }
            i += 1;
        }
        self.entries.push(AudioFormatSpec {
            format_id,
            name: name.to_string(),
            lossy,
            max_channels,
            streamable,
        });
        Ok(())
    }

    /// 按 id 查（未注册 None——调用方落 unknown_rejects）。
    pub fn lookup(&self, format_id: u8) -> Option<&AudioFormatSpec> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.format_id == format_id {
                    return self.entries.get(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 在册格式数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 五、双标注（压缩率 + 音质档——账面落定全程可查）
// ---------------------------------------------------------------------------

/// 音质三档（失真百分比对档；被失真闸拒的根本不入库）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityGrade {
    /// 高保：失真 ≤ [`GRADE_HIGH_MAX_PCT`]。
    High,
    /// 中保：失真 ≤ [`GRADE_MID_MAX_PCT`]。
    Mid,
    /// 低保：其余（仍在入库线内）。
    Low,
}

impl QualityGrade {
    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            QualityGrade::High => "高保",
            QualityGrade::Mid => "中保",
            QualityGrade::Low => "低保",
        }
    }

    /// 由失真百分比定档（纯函数）。
    pub const fn from_distortion(pct: u64) -> QualityGrade {
        if pct <= GRADE_HIGH_MAX_PCT {
            QualityGrade::High
        } else if pct <= GRADE_MID_MAX_PCT {
            QualityGrade::Mid
        } else {
            QualityGrade::Low
        }
    }
}

/// 双标注视图（可查面——锚点"双标注可查"）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DualAnnotation {
    /// 压缩率（bps：压缩后/原始 ×10000；PCM 恒 10000）。
    pub ratio_bps: u64,
    /// 音质档。
    pub grade: QualityGrade,
    /// 人读双标注行。
    pub zh_line: String,
}

// ---------------------------------------------------------------------------
// 六、导入段（元数据合法性闸 + 体检报告三数）
// ---------------------------------------------------------------------------

/// 导入元数据（调用方申报；管线只做合法性闸，不碰数据本体——零 IO）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportMeta {
    /// 源码率（kbps；申报值，体检闸要求 >0）。
    pub source_bitrate_kbps: u64,
    /// 采样率（Hz；必须落合法闭集）。
    pub sample_rate_hz: u32,
    /// 声道数（1..=[`MAX_CHANNELS`]）。
    pub channels: u8,
    /// 时长（ms；>0）。
    pub duration_ms: u64,
    /// 响度（十分之一 LU 定点；域 [`LOUDNESS_MIN_TENTHS`]..=[`LOUDNESS_MAX_TENTHS`]）。
    pub loudness_tenths: i16,
    /// 声明格式 id（须在册）。
    pub format_id: u8,
    /// 源体量（KB；申报值 >0）。
    pub size_kb: u64,
}

/// 体检报告（导入完成即附——码率/时长/响度三数，锚点原文）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthReport {
    /// 码率（kbps）。
    pub bitrate_kbps: u64,
    /// 时长（ms）。
    pub duration_ms: u64,
    /// 响度（十分之一 LU 定点）。
    pub loudness_tenths: i16,
}

impl HealthReport {
    /// 三数齐备判定（>0 且响度在域）。
    pub const fn complete(&self) -> bool {
        self.bitrate_kbps > 0
            && self.duration_ms > 0
            && self.loudness_tenths >= LOUDNESS_MIN_TENTHS
            && self.loudness_tenths <= LOUDNESS_MAX_TENTHS
    }

    /// 人读行（响度定点回显两位小数形——纯字符串拼装零浮点）。
    pub fn zh_line(&self) -> String {
        let tenths = self.loudness_tenths as i32;
        let whole = tenths / 10;
        let frac = (tenths % 10).abs();
        alloc::format!(
            "体检：码率 {}kbps / 时长 {}ms / 响度 {}.{} LU",
            self.bitrate_kbps,
            self.duration_ms,
            whole,
            frac
        )
    }
}

/// 导入裁决。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportVerdict {
    /// 通过：资产入库（携带分配 id）。
    Accepted(u32),
    /// 拒绝（携带诊断码——明确拒绝不留半入库态）。
    Rejected(XcCode),
}

/// 导入段（合法性闸顺序：格式注册 → 采样率 → 声道 → 体检三数）。
pub struct ImportStage;

impl ImportStage {
    /// 闸序校验（首个不过即拒——短路语义，拒因即首个违规点）。
    pub fn gate(&self, reg: &FormatRegistry, meta: &ImportMeta) -> Result<HealthReport, XcCode> {
        // 一、格式在册（未注册显性计数——"格式不支持→明确拒绝"）。
        if reg.lookup(meta.format_id).is_none() {
            return Err(XcCode::FormatUnknown);
        }
        // 二、采样率闭集。
        let mut legal = false;
        let mut i = 0usize;
        while i < LEGAL_SAMPLE_RATES_HZ.len() {
            if LEGAL_SAMPLE_RATES_HZ.get(i) == Some(&meta.sample_rate_hz) {
                legal = true;
                break;
            }
            i += 1;
        }
        if !legal {
            return Err(XcCode::SampleRateIllegal);
        }
        // 三、声道数（1..=MAX_CHANNELS）。
        if meta.channels == 0 || meta.channels > MAX_CHANNELS {
            return Err(XcCode::ChannelCountIllegal);
        }
        // 四、体检三数（码率/时长/响度缺一或越域即拒）。
        let report = HealthReport {
            bitrate_kbps: meta.source_bitrate_kbps,
            duration_ms: meta.duration_ms,
            loudness_tenths: meta.loudness_tenths,
        };
        if !report.complete() {
            return Err(XcCode::HealthFail);
        }
        Ok(report)
    }
}

// ---------------------------------------------------------------------------
// 七、压缩段（失真预算闸 + 双标注落定）
// ---------------------------------------------------------------------------

/// 压缩请求（目标码率）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompressRequest {
    /// 目标码率（kbps；须 < 源码率才谈压缩）。
    pub target_bitrate_kbps: u64,
}

/// 压缩裁决。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompressVerdict {
    /// 预估失真（百分比；查表闭式——零浮点）。
    pub distortion_pct: u64,
    /// 音质档。
    pub grade: QualityGrade,
    /// 压缩率 bps（目标/源 ×10000）。
    pub ratio_bps: u64,
    /// 是否通过入库闸。
    pub accepted: bool,
    /// 拒因（过闸时 None）。
    pub denied: Option<XcCode>,
    /// 人读说明。
    pub note: String,
}

/// 压缩段（失真映射 + 双标注落定）。
pub struct CompressStage;

impl CompressStage {
    /// 失真查表（bitrate → pct；表外线性外推——纯整数运算）。
    pub fn distortion_pct_of(target_kbps: u64) -> u64 {
        let t = BITRATE_DISTORTION_TABLE;
        // 低于最低档：外推放大（每差 8kbps +2%，饱和封顶 32%）。
        if target_kbps < t.first().map(|x| x.0).unwrap_or(24) {
            let floor_k = t.first().map(|x| x.0).unwrap_or(24);
            let floor_p = t.first().map(|x| x.1).unwrap_or(14);
            let gap = floor_k.saturating_sub(target_kbps);
            let extra = gap / 8 * 2;
            return (floor_p.saturating_add(extra)).min(32);
        }
        let mut i = 0usize;
        while i < t.len() {
            let (k, p) = match t.get(i) {
                Some(x) => *x,
                None => (256, 1),
            };
            if target_kbps <= k {
                return p;
            }
            i += 1;
        }
        // 高于最高档：趋近 1%（无损上沿）。
        1
    }

    /// 压缩裁决（失真超 [`DISTORTION_MAX_PCT`] 即拒绝入库——锚点闸）。
    pub fn judge(&self, src_bitrate_kbps: u64, req: &CompressRequest) -> CompressVerdict {
        if req.target_bitrate_kbps == 0 || req.target_bitrate_kbps >= src_bitrate_kbps {
            return CompressVerdict {
                distortion_pct: 0,
                grade: QualityGrade::High,
                ratio_bps: 10_000,
                accepted: false,
                denied: Some(XcCode::DistortionOverThreshold),
                note: "目标码率须为 0<target<源，否则无从压缩".to_string(),
            };
        }
        let pct = Self::distortion_pct_of(req.target_bitrate_kbps);
        let grade = QualityGrade::from_distortion(pct);
        let ratio = req.target_bitrate_kbps.saturating_mul(10_000) / src_bitrate_kbps.max(1);
        if pct > DISTORTION_MAX_PCT {
            return CompressVerdict {
                distortion_pct: pct,
                grade,
                ratio_bps: ratio,
                accepted: false,
                denied: Some(XcCode::DistortionOverThreshold),
                note: alloc::format!(
                    "失真 {}% 超入库线 {}%：拒绝入库，建议抬高目标码率",
                    pct,
                    DISTORTION_MAX_PCT
                ),
            };
        }
        CompressVerdict {
            distortion_pct: pct,
            grade,
            ratio_bps: ratio,
            accepted: true,
            denied: None,
            note: alloc::format!("入库：失真 {}%，档 {}，压缩率 {}", pct, grade.zh(), ratio),
        }
    }
}

// ---------------------------------------------------------------------------
// 八、事件账（七类闭集 + 滚动 96 + FNV 链）
// ---------------------------------------------------------------------------

/// 事件类别闭集（七类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 导入通过。
    Imported,
    /// 导入拒绝。
    ImportRejected,
    /// 压缩落定。
    Compressed,
    /// 压缩拒绝（失真超阈）。
    CompressRejected,
    /// 流会话开立。
    StreamOpened,
    /// 回退降级（提前三秒）。
    FallbackEngaged,
    /// 回退恢复。
    FallbackRestored,
}

impl EventKind {
    /// 类别序号（0-6）。
    pub const fn kind_id(self) -> u8 {
        match self {
            EventKind::Imported => 0,
            EventKind::ImportRejected => 1,
            EventKind::Compressed => 2,
            EventKind::CompressRejected => 3,
            EventKind::StreamOpened => 4,
            EventKind::FallbackEngaged => 5,
            EventKind::FallbackRestored => 6,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            EventKind::Imported => "导入",
            EventKind::ImportRejected => "导入拒",
            EventKind::Compressed => "压缩",
            EventKind::CompressRejected => "压缩拒",
            EventKind::StreamOpened => "开流",
            EventKind::FallbackEngaged => "降级回退",
            EventKind::FallbackRestored => "回退恢复",
        }
    }
}

/// 账面行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventRecord {
    /// 全账严格递增序号。
    pub seq: u64,
    /// tick（注入）。
    pub tick: u64,
    /// 类别。
    pub kind: EventKind,
    /// 关联资产 id。
    pub asset_id: u32,
    /// 关联量（kbps/段号复用域；无量 0）。
    pub amount: u64,
    /// 人读摘要。
    pub summary: String,
    /// 链上指纹。
    pub fp: u64,
}

/// 滚动事件账。
#[derive(Clone, Debug)]
pub struct EventLedger {
    records: Vec<EventRecord>,
    next_seq: u64,
    /// 逐出累计。
    pub evicted: u64,
    /// 尾指纹。
    pub tail_fp: u64,
}

impl EventLedger {
    /// 空账。
    pub fn new() -> EventLedger {
        EventLedger {
            records: Vec::new(),
            next_seq: 1,
            evicted: 0,
            tail_fp: FNV_OFFSET,
        }
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 下一序号预告。
    pub fn peek_seq(&self) -> u64 {
        self.next_seq
    }

    /// 追加（滚动逐出）。
    pub fn append(&mut self, tick: u64, kind: EventKind, asset_id: u32, amount: u64, summary: &str) -> u64 {
        let seq = self.next_seq;
        let mut fp = fnv1a_u64(self.tail_fp, seq);
        fp = fnv1a_u64(fp, tick);
        fp = fnv1a_u64(fp, kind.kind_id() as u64);
        fp = fnv1a_u64(fp, asset_id as u64);
        fp = fnv1a_u64(fp, amount);
        fp = fnv1a_str(fp, summary);
        self.tail_fp = fp;
        self.next_seq = self.next_seq.saturating_add(1);
        if self.records.len() >= EVENT_LEDGER_CAPACITY {
            let _ = self.records.remove(0);
            self.evicted = self.evicted.saturating_add(1);
        }
        self.records.push(EventRecord {
            seq,
            tick,
            kind,
            asset_id,
            amount,
            summary: summary.to_string(),
            fp,
        });
        seq
    }

    /// 链复验（重算对拍）。
    pub fn verify_chain(&self) -> Result<(), XcCode> {
        let mut prev = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.records.len() {
            let r = match self.records.get(i) {
                Some(r) => r,
                None => return Err(XcCode::LedgerCorrupt),
            };
            let mut fp = fnv1a_u64(prev, r.seq);
            fp = fnv1a_u64(fp, r.tick);
            fp = fnv1a_u64(fp, r.kind.kind_id() as u64);
            fp = fnv1a_u64(fp, r.asset_id as u64);
            fp = fnv1a_u64(fp, r.amount);
            fp = fnv1a_str(fp, &r.summary);
            if fp != r.fp {
                return Err(XcCode::LedgerCorrupt);
            }
            prev = r.fp;
            i += 1;
        }
        if prev != self.tail_fp {
            return Err(XcCode::LedgerCorrupt);
        }
        Ok(())
    }

    /// 按类别计数。
    pub fn count_by_kind(&self, kind: EventKind) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.kind == kind {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 按资产计数。
    pub fn count_by_asset(&self, asset_id: u32) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.asset_id == asset_id {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 九、流送段（按需预取 + 提前三秒降级 + 显性恢复）
// ---------------------------------------------------------------------------

/// 流会话（单资产单消费点）。
#[derive(Clone, Debug)]
pub struct StreamSession {
    /// 会话 id（管线内唯一）。
    pub session_id: u32,
    /// 关联资产。
    pub asset_id: u32,
    /// 播放头（ms；单调不减）。
    pub playhead_ms: u64,
    /// 已预取到（ms；播放头+预取窗对齐边界）。
    pub prefetched_until_ms: u64,
    /// 回退态（本地续播中）。
    pub fallback_active: bool,
    /// 回退后连续成功计数（恢复窗口）。
    pub restore_streak: u32,
    /// 超时计数（段失败累计）。
    pub timeouts: u64,
    /// 回退降级次数（含历史）。
    pub fallbacks: u64,
    /// 段账：产出。
    pub seg_produced: u64,
    /// 段账：远端交付。
    pub seg_delivered: u64,
    /// 段账：本地回退交付。
    pub seg_fallback: u64,
    /// 下一应交付段号（连续性闸）。
    pub next_segment: u64,
}

impl StreamSession {
    /// 新会话（预取窗按播放头 0 对齐）。
    pub fn new(session_id: u32, asset_id: u32) -> StreamSession {
        StreamSession {
            session_id,
            asset_id,
            playhead_ms: 0,
            prefetched_until_ms: 0,
            fallback_active: false,
            restore_streak: 0,
            timeouts: 0,
            fallbacks: 0,
            seg_produced: 0,
            seg_delivered: 0,
            seg_fallback: 0,
            next_segment: 0,
        }
    }

    /// 推进播放头（单调闸；返回需要预取的段号清单——按需滚动）。
    pub fn advance(&mut self, playhead_ms: u64) -> Result<Vec<u64>, XcCode> {
        if playhead_ms < self.playhead_ms {
            return Err(XcCode::PlayheadReverse);
        }
        self.playhead_ms = playhead_ms;
        // 需求上沿：播放头 + 提前三秒。
        let need_until = self.playhead_ms.saturating_add(PREFETCH_LEAD_MS);
        let mut wanted: Vec<u64> = Vec::new();
        while self.prefetched_until_ms < need_until {
            let seg = self.prefetched_until_ms / SEGMENT_MS;
            wanted.push(seg);
            self.prefetched_until_ms = self.prefetched_until_ms.saturating_add(SEGMENT_MS);
        }
        Ok(wanted)
    }

    /// 段产出登记（预取器报告产出——量账 produced）。
    pub fn segment_produced(&mut self, seg: u64) {
        self.seg_produced = self.seg_produced.saturating_add(1);
        let _ = seg;
    }

    /// 段交付（ok=false 即超时；窗内失败提前三秒降级——锚点原文）。
    /// 返回本次是否触发/维持回退。
    pub fn segment_deliver(&mut self, seg: u64, ok: bool) -> Result<bool, XcCode> {
        // 连续性闸：段号必须等于下一应交付段。
        if seg != self.next_segment {
            return Err(XcCode::SegmentGap);
        }
        self.next_segment = self.next_segment.saturating_add(1);
        if ok {
            if self.fallback_active {
                self.restore_streak = self.restore_streak.saturating_add(1);
                if self.restore_streak >= RESTORE_CONFIRM {
                    // 显性恢复（滞回——不自动抖回）。
                    self.fallback_active = false;
                    self.restore_streak = 0;
                    self.seg_fallback = self.seg_fallback.saturating_add(1);
                    return Ok(false);
                }
                // 回退中：本地交付。
                self.seg_fallback = self.seg_fallback.saturating_add(1);
                return Ok(true);
            }
            self.seg_delivered = self.seg_delivered.saturating_add(1);
            return Ok(false);
        }
        // 失败：超时计数；回退中则维持。
        self.timeouts = self.timeouts.saturating_add(1);
        if self.fallback_active {
            self.restore_streak = 0; // 回退中再失败：恢复连击清零。
            self.seg_fallback = self.seg_fallback.saturating_add(1);
            return Ok(true);
        }
        // 提前三秒降级：失败段落在预取窗内（即未来三秒内的供给断档），
        // 立即降级本地回退——不等真正断流。
        self.fallback_active = true;
        self.fallbacks = self.fallbacks.saturating_add(1);
        self.restore_streak = 0;
        self.seg_fallback = self.seg_fallback.saturating_add(1);
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// 十、资产与总编排（三段串接 + 分发 + 审计 + 人读）
// ---------------------------------------------------------------------------

/// 在库资产（导入过闸 + 压缩双标注落定后定格）。
#[derive(Clone, Debug)]
pub struct ImportedAsset {
    /// 资产 id。
    pub asset_id: u32,
    /// 格式 id。
    pub format_id: u8,
    /// 采样率。
    pub sample_rate_hz: u32,
    /// 声道数。
    pub channels: u8,
    /// 体检报告（三数）。
    pub health: HealthReport,
    /// 原始码率（kbps；压缩前）。
    pub source_bitrate_kbps: u64,
    /// 当前码率（kbps；压缩后=目标；未压缩=源）。
    pub bitrate_kbps: u64,
    /// 压缩率 bps（未压缩 10000）。
    pub ratio_bps: u64,
    /// 音质档。
    pub grade: QualityGrade,
    /// 已压缩标记。
    pub compressed: bool,
    /// 本地就绪（回退供给面）。
    pub local_ready: bool,
    /// 失真记录（百分比；未压缩 0）。
    pub distortion_pct: u64,
}

impl ImportedAsset {
    /// 双标注视图（可查面）。
    pub fn annotate(&self, reg: &FormatRegistry) -> DualAnnotation {
        let fname = match reg.lookup(self.format_id) {
            Some(f) => f.name.clone(),
            None => "?".to_string(),
        };
        DualAnnotation {
            ratio_bps: self.ratio_bps,
            grade: self.grade,
            zh_line: alloc::format!(
                "资产{}：{} 压缩率 {}（bps 口径）音质{} 失真{}%",
                self.asset_id,
                fname,
                self.ratio_bps,
                self.grade.zh(),
                self.distortion_pct
            ),
        }
    }

    /// 分发就绪判定（X05：体检齐 + 双标注齐 + 本地就绪）。
    pub const fn dispatchable(&self) -> bool {
        self.health.complete() && self.ratio_bps > 0 && self.local_ready
    }
}

/// 资产管线总编排（三段串接 + 账本 + 审计）。
#[derive(Clone, Debug)]
pub struct AssetPipeline {
    /// 格式注册表。
    pub registry: FormatRegistry,
    assets: Vec<ImportedAsset>,
    sessions: Vec<StreamSession>,
    /// 事件账。
    pub ledger: EventLedger,
    /// 下一资产 id（自 1 起）。
    next_asset_id: u32,
    /// 下一会话 id。
    next_session_id: u32,
    /// 导入拒绝累计。
    pub import_rejects: u64,
    /// 压缩拒绝累计。
    pub compress_rejects: u64,
}

impl AssetPipeline {
    /// 新管线（缺省注册表）。
    pub fn new() -> AssetPipeline {
        AssetPipeline {
            registry: FormatRegistry::with_defaults(),
            assets: Vec::new(),
            sessions: Vec::new(),
            ledger: EventLedger::new(),
            next_asset_id: 1,
            next_session_id: 1,
            import_rejects: 0,
            compress_rejects: 0,
        }
    }

    /// 在库资产数。
    pub fn asset_count(&self) -> usize {
        self.assets.len()
    }

    /// 会话数。
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    // ---------------- 一段：导入 ----------------

    /// 导入（三闸 + 体检 + 入库；O(资产) 扫描拒重复由 id 自增天然规避）。
    pub fn import(&mut self, meta: &ImportMeta, tick: u64) -> ImportVerdict {
        let stage = ImportStage;
        match stage.gate(&self.registry, meta) {
            Ok(health) => {
                if self.assets.len() >= MAX_ASSETS {
                    self.import_rejects = self.import_rejects.saturating_add(1);
                    self.ledger.append(tick, EventKind::ImportRejected, 0, 0, "资产超上限拒");
                    return ImportVerdict::Rejected(XcCode::HealthFail);
                }
                let id = self.next_asset_id;
                self.next_asset_id = self.next_asset_id.saturating_add(1);
                self.assets.push(ImportedAsset {
                    asset_id: id,
                    format_id: meta.format_id,
                    sample_rate_hz: meta.sample_rate_hz,
                    channels: meta.channels,
                    health: health.clone(),
                    source_bitrate_kbps: meta.source_bitrate_kbps,
                    bitrate_kbps: meta.source_bitrate_kbps,
                    ratio_bps: 10_000,
                    grade: QualityGrade::High,
                    compressed: false,
                    local_ready: true,
                    distortion_pct: 0,
                });
                self.ledger.append(
                    tick,
                    EventKind::Imported,
                    id,
                    meta.source_bitrate_kbps,
                    &health.zh_line(),
                );
                ImportVerdict::Accepted(id)
            }
            Err(code) => {
                self.import_rejects = self.import_rejects.saturating_add(1);
                self.ledger.append(
                    tick,
                    EventKind::ImportRejected,
                    0,
                    0,
                    &alloc::format!("导入拒 {:04x}", code.code_hex()),
                );
                ImportVerdict::Rejected(code)
            }
        }
    }

    // ---------------- 二段：压缩 ----------------

    /// 压缩（失真闸 + 双标注落定 + 留痕）。
    pub fn compress(&mut self, asset_id: u32, req: &CompressRequest, tick: u64) -> Result<CompressVerdict, XcCode> {
        let src = match self.find_asset(asset_id) {
            Some(a) => a.source_bitrate_kbps,
            None => {
                self.compress_rejects = self.compress_rejects.saturating_add(1);
                return Err(XcCode::AssetNotFound);
            }
        };
        let stage = CompressStage;
        let verdict = stage.judge(src, req);
        if !verdict.accepted {
            self.compress_rejects = self.compress_rejects.saturating_add(1);
            self.ledger.append(
                tick,
                EventKind::CompressRejected,
                asset_id,
                verdict.distortion_pct,
                &verdict.note,
            );
            return Ok(verdict);
        }
        let mut i = 0usize;
        while i < self.assets.len() {
            if let Some(a) = self.assets.get_mut(i) {
                if a.asset_id == asset_id {
                    a.bitrate_kbps = req.target_bitrate_kbps;
                    a.ratio_bps = verdict.ratio_bps;
                    a.grade = verdict.grade;
                    a.compressed = true;
                    a.distortion_pct = verdict.distortion_pct;
                    break;
                }
            }
            i += 1;
        }
        self.ledger.append(
            tick,
            EventKind::Compressed,
            asset_id,
            req.target_bitrate_kbps,
            &verdict.note,
        );
        Ok(verdict)
    }

    /// 按 id 找资产（只读）。
    pub fn find_asset(&self, asset_id: u32) -> Option<&ImportedAsset> {
        let mut i = 0usize;
        while i < self.assets.len() {
            if let Some(a) = self.assets.get(i) {
                if a.asset_id == asset_id {
                    return self.assets.get(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 双标注查询（资产不存在 None）。
    pub fn annotate(&self, asset_id: u32) -> Option<DualAnnotation> {
        let reg = &self.registry;
        self.find_asset(asset_id).map(|a| a.annotate(reg))
    }

    // ---------------- 三段：流送 ----------------

    /// 开流会话（资产须在册且可流式；会话数上限闸）。
    pub fn open_stream(&mut self, asset_id: u32, tick: u64) -> Result<u32, XcCode> {
        let fmt = match self.find_asset(asset_id) {
            Some(a) => a.format_id,
            None => return Err(XcCode::AssetNotFound),
        };
        let streamable = match self.registry.lookup(fmt) {
            Some(f) => f.streamable,
            None => false,
        };
        if !streamable {
            self.import_rejects = self.import_rejects.saturating_add(1);
            return Err(XcCode::FormatUnknown);
        }
        let mut per_asset = 0usize;
        let mut i = 0usize;
        while i < self.sessions.len() {
            if let Some(s) = self.sessions.get(i) {
                if s.asset_id == asset_id {
                    per_asset = per_asset.saturating_add(1);
                }
            }
            i += 1;
        }
        if per_asset >= MAX_SESSIONS_PER_ASSET {
            return Err(XcCode::SessionOverflow);
        }
        let sid = self.next_session_id;
        self.next_session_id = self.next_session_id.saturating_add(1);
        self.sessions.push(StreamSession::new(sid, asset_id));
        self.ledger.append(tick, EventKind::StreamOpened, asset_id, sid as u64, "开流");
        Ok(sid)
    }

    /// 流推进（播放头 + 产出 + 交付一批；返回回退态）。
    pub fn stream_step(
        &mut self,
        session_id: u32,
        playhead_ms: u64,
        deliver: &[(u64, bool)],
        tick: u64,
    ) -> Result<bool, XcCode> {
        let mut target: Option<usize> = None;
        let mut i = 0usize;
        while i < self.sessions.len() {
            if let Some(s) = self.sessions.get(i) {
                if s.session_id == session_id {
                    target = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let idx = match target {
            Some(i) => i,
            None => return Err(XcCode::AssetNotFound),
        };
        // 播放头推进 + 预取清单（产出登记）。
        let wanted = {
            let s = match self.sessions.get_mut(idx) {
                Some(s) => s,
                None => return Err(XcCode::AssetNotFound),
            };
            let w = s.advance(playhead_ms)?;
            let mut j = 0usize;
            while j < w.len() {
                s.segment_produced(*w.get(j).unwrap_or(&0));
                j += 1;
            }
            w
        };
        let _ = wanted;
        // 逐段交付。
        let mut engaged = false;
        let mut j = 0usize;
        while j < deliver.len() {
            let (seg, ok) = match deliver.get(j) {
                Some(d) => *d,
                None => break,
            };
            let s = match self.sessions.get_mut(idx) {
                Some(s) => s,
                None => return Err(XcCode::AssetNotFound),
            };
            let now_fallback = s.segment_deliver(seg, ok)?;
            if now_fallback && !engaged && s.fallback_active {
                // 首次进入回退：留痕一次。
                if s.fallbacks == 1 || s.restore_streak == 0 {
                    self.ledger.append(
                        tick,
                        EventKind::FallbackEngaged,
                        s.asset_id,
                        seg,
                        "预取窗内失败：提前三秒降级本地回退",
                    );
                }
            }
            engaged = s.fallback_active;
            j += 1;
        }
        // 回退恢复留痕（本步从回退转正常）。
        let s = match self.sessions.get(idx) {
            Some(s) => s,
            None => return Err(XcCode::AssetNotFound),
        };
        if !s.fallback_active && s.restore_streak == 0 && s.fallbacks > 0 && !engaged {
            self.ledger.append(tick, EventKind::FallbackRestored, s.asset_id, 0, "回退恢复：远端续流");
        }
        Ok(engaged)
    }

    /// 会话只读视图。
    pub fn session(&self, session_id: u32) -> Option<&StreamSession> {
        let mut i = 0usize;
        while i < self.sessions.len() {
            if let Some(s) = self.sessions.get(i) {
                if s.session_id == session_id {
                    return self.sessions.get(i);
                }
            }
            i += 1;
        }
        None
    }

    // ---------------- 分发（X05） ----------------

    /// X05 分发清单（就绪资产 id 集——体检齐+双标注齐+本地就绪）。
    pub fn dispatch_x05(&self) -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.assets.len() {
            if let Some(a) = self.assets.get(i) {
                if a.dispatchable() {
                    out.push(a.asset_id);
                }
            }
            i += 1;
        }
        out
    }

    // ---------------- 审计面 ----------------

    /// 账本链复验。
    pub fn ledger_intact(&self) -> Result<(), XcCode> {
        self.ledger.verify_chain()
    }

    /// 量账守恒：逐会话 produced == delivered + fallback（丢弃恒 0）。
    pub fn reconcile(&self) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < self.sessions.len() {
            let s = match self.sessions.get(i) {
                Some(s) => s,
                None => return Err(XcCode::LedgerCorrupt),
            };
            if s.seg_produced != s.seg_delivered.saturating_add(s.seg_fallback) {
                return Err(XcCode::LedgerCorrupt);
            }
            i += 1;
        }
        Ok(())
    }

    /// 全量校验（资产引用格式在册 + 会话引用资产在册 + 账链 + 量账）。
    pub fn validate(&self) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < self.assets.len() {
            let a = match self.assets.get(i) {
                Some(a) => a,
                None => return Err(XcCode::AssetNotFound),
            };
            if self.registry.lookup(a.format_id).is_none() {
                return Err(XcCode::FormatUnknown);
            }
            if !a.health.complete() {
                return Err(XcCode::HealthFail);
            }
            if a.compressed && a.distortion_pct > DISTORTION_MAX_PCT {
                return Err(XcCode::DistortionOverThreshold);
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < self.sessions.len() {
            let s = match self.sessions.get(i) {
                Some(s) => s,
                None => return Err(XcCode::AssetNotFound),
            };
            if self.find_asset(s.asset_id).is_none() {
                return Err(XcCode::AssetNotFound);
            }
            if s.seg_produced != s.seg_delivered.saturating_add(s.seg_fallback) {
                return Err(XcCode::SegmentGap);
            }
            i += 1;
        }
        self.ledger_intact()?;
        Ok(())
    }

    // ---------------- 人读面 ----------------

    /// 人读汇总。
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str("== VE-F5607 音频资产管线 汇报 ==\n");
        s.push_str(&alloc::format!(
            "资产 {} 会话 {} 注册格式 {} 导入拒 {} 压缩拒 {}\n",
            self.asset_count(),
            self.session_count(),
            self.registry.len(),
            self.import_rejects,
            self.compress_rejects
        ));
        let mut i = 0usize;
        while i < self.assets.len() {
            if let Some(a) = self.assets.get(i) {
                let reg = &self.registry;
                s.push_str(&a.annotate(reg).zh_line);
                s.push('\n');
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < self.sessions.len() {
            if let Some(sess) = self.sessions.get(i) {
                s.push_str(&alloc::format!(
                    "会话{} 资产{} 进度 {}ms 预取到 {}ms 回退={} 超时 {} 段(产/远/本) {}/{}/{}\n",
                    sess.session_id,
                    sess.asset_id,
                    sess.playhead_ms,
                    sess.prefetched_until_ms,
                    sess.fallback_active,
                    sess.timeouts,
                    sess.seg_produced,
                    sess.seg_delivered,
                    sess.seg_fallback
                ));
            }
            i += 1;
        }
        s
    }

    /// 无障碍读屏（三段固定预算：状态/双标注/回退态）。
    pub fn read_screen(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(alloc::format!(
            "音频资产管线：资产 {} 个，会话 {} 个，格式注册 {} 种",
            self.asset_count(),
            self.session_count(),
            self.registry.len()
        ));
        // 双标注段（在库资产逐条——上限 8 条防读屏爆窗）。
        let mut shown = 0usize;
        let mut i = 0usize;
        while i < self.assets.len() && shown < 8 {
            if let Some(a) = self.assets.get(i) {
                let reg = &self.registry;
                out.push(a.annotate(reg).zh_line);
                shown = shown.saturating_add(1);
            }
            i += 1;
        }
        // 回退态段（仅回退中会话——常态不占预算）。
        let mut i = 0usize;
        while i < self.sessions.len() {
            if let Some(s) = self.sessions.get(i) {
                if s.fallback_active {
                    out.push(alloc::format!(
                        "会话{}：本地回退中，已连续成功 {} 次（{} 次恢复）",
                        s.session_id,
                        s.restore_streak,
                        RESTORE_CONFIRM
                    ));
                }
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 十一、参考探针（自证面——纯函数口径，零 IO）
// ---------------------------------------------------------------------------

/// 参考探针：核心判据自证。
pub fn run_veab07_checks() -> Vec<(&'static str, bool)> {
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    // 格式注册制。
    let mut reg = FormatRegistry::with_defaults();
    out.push(("缺省五格式在册", reg.len() == 5));
    out.push(("重复注册拒", reg.register(1, "X", false, 2, true).is_err()));
    out.push(("id0 拒", reg.register(0, "Y", false, 2, true).is_err()));
    out.push(("声道越界拒", reg.register(9, "Z", false, 9, true).is_err()));
    out.push(("未注册查无", reg.lookup(99).is_none()));
    // 导入三闸。
    let pipe0 = AssetPipeline::new();
    let good = ImportMeta {
        source_bitrate_kbps: 320,
        sample_rate_hz: 44100,
        channels: 2,
        duration_ms: 180_000,
        loudness_tenths: -140,
        format_id: 1,
        size_kb: 7200,
    };
    out.push(("合法导入过闸", ImportStage.gate(&pipe0.registry, &good).is_ok()));
    let bad_rate = ImportMeta { sample_rate_hz: 12345, ..good.clone() };
    out.push(("采样率表外拒", ImportStage.gate(&pipe0.registry, &bad_rate).is_err()));
    let bad_ch = ImportMeta { channels: 0, ..good.clone() };
    out.push(("零声道拒", ImportStage.gate(&pipe0.registry, &bad_ch).is_err()));
    let bad_fmt = ImportMeta { format_id: 42, ..good.clone() };
    out.push(("未注册格式拒", ImportStage.gate(&pipe0.registry, &bad_fmt).is_err()));
    let bad_health = ImportMeta { source_bitrate_kbps: 0, ..good.clone() };
    out.push(("体检缺数拒", ImportStage.gate(&pipe0.registry, &bad_health).is_err()));
    // 管线全流程。
    let mut pipe = AssetPipeline::new();
    let id = match pipe.import(&good, 1) {
        ImportVerdict::Accepted(id) => id,
        ImportVerdict::Rejected(_) => 0,
    };
    out.push(("导入入库", id != 0));
    let cv = pipe.compress(id, &CompressRequest { target_bitrate_kbps: 128 }, 2);
    out.push(("128k 压缩过闸", cv.as_ref().map(|v| v.accepted).unwrap_or(false)));
    out.push(("双标注落定", pipe.annotate(id).map(|a| a.ratio_bps == 4000).unwrap_or(false)));
    let cv2 = pipe.compress(id, &CompressRequest { target_bitrate_kbps: 24 }, 3);
    out.push(("24k 失真拒入库", cv2.map(|v| !v.accepted && v.denied == Some(XcCode::DistortionOverThreshold)).unwrap_or(false)));
    // 流送：预取窗与提前三秒降级。
    let sid = pipe.open_stream(id, 4);
    out.push(("PCM 可流式开流", sid.is_ok()));
    if let Ok(sid) = sid {
        // 推进到 2000ms：预取窗应覆盖 0..5000ms（2000+3000）→ 段 0/1/2。
        let want = {
            let s = pipe.session(sid);
            let mut tmp = s.cloned();
            match tmp.as_mut().map(|x| x.advance(2000)) {
                Some(Ok(w)) => w.len(),
                _ => 0,
            }
        };
        out.push(("按需预取滚动窗", want == 3));
        let _ = pipe.stream_step(sid, 2000, &[(0, true), (1, false)], 5);
        let sess = pipe.session(sid);
        out.push(("窗内失败提前回退", sess.map(|s| s.fallback_active && s.fallbacks == 1).unwrap_or(false)));
        out.push(("超时留痕", sess.map(|s| s.timeouts == 1).unwrap_or(false)));
        // 连续成功 8 次恢复。
        let mut k = 0u32;
        while k < RESTORE_CONFIRM {
            let seg = 2 + k as u64;
            let _ = pipe.stream_step(sid, 2000 + (k as u64 + 1) * SEGMENT_MS, &[(seg, true)], 6);
            k += 1;
        }
        out.push(("显性恢复", pipe.session(sid).map(|s| !s.fallback_active).unwrap_or(false)));
    }
    // 守恒与审计。
    out.push(("量账守恒", pipe.reconcile().is_ok()));
    out.push(("账链完整", pipe.ledger_intact().is_ok()));
    out.push(("全量校验过", pipe.validate().is_ok()));
    // 分发与读屏。
    out.push(("X05 分发含在库", pipe.dispatch_x05().contains(&id)));
    out.push(("读屏三段", pipe.read_screen().len() >= 3));
    out.push(("诊断码带建议", !XcCode::StreamTimeout.detail().is_empty() && XcCode::StreamTimeout.code_hex() == 0x5305));
    out
}
