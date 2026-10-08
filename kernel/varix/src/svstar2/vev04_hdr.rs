//! VE-F4404 · HDR 管线（VE-W 域 · 显示与色彩批 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4404`
//!
//! **判据（锚点原文）**：四段、元数据抽象、色调映射、亮度钳制、判据。
//!
//! **职责定位（锚点原文）**：HDR 管线——**四段**：HDR 能力探测 / 元数据抽象
//! （HDR10 与动态元数据类统一抽象层）/ 色调映射（HDR→SDR 降级映射曲线
//! 可调）/ SDR 同屏混合（HDR 内容与 SDR 界面同屏亮度协调）。错误路径与
//! 降级矩阵：探测失准→以 EDID 声明为准标注；元数据缺失→静态映射回退；
//! 混合过曝→亮度钳制。
//!
//! # 一、为什么是四段而不是一个「HDR 开关」
//!
//! HDR 的四种失败需要四种独立处置：探测撒谎（探测）、格式各自为政
//! （元数据）、亮部死白或死灰（色调映射）、HDR 视频把 SDR 界面晃瞎
//! （混合）。四段各自可独立断言（判据「四段」的本意），且降级矩阵的
//! 三条降级各自落在不同段：探测失准在段一标注、元数据缺失在段二回退、
//! 混合过曝在段四钳制——揉成一个入口，「画面发灰是探测的锅还是映射的
//! 锅」就永远查不清。
//!
//! # 二、探测失准为什么以 EDID 声明为准
//!
//! 段一有两个真源：F4402 能力快照（运行时探测）与 EDID 静态声明位。
//! 快照会受线缆/驱动状态抖动影响（锚点「探测失准」的现实来源）；
//! EDID 是显示器出厂固化的自我声明——**声明了 HDR 而快照没测到**，
//! 按声明走 HDR 并标注失准（宁可错在出厂声明上，不错在瞬态探测上）；
//! 反向分歧（快照测到而 EDID 无声明）同样标注。分歧不是错误，是
//! **可观测的标注态**：静默选边才是事故。
//!
//! # 三、元数据抽象层为什么把 HDR10 与动态类归一
//!
//! HDR10 的静态元数据（MaxCLL/MaxFALL）与 HDR10+ / Dolby Vision 类的
//! 动态元数据，对**下游要问的问题其实是同一个**：这一帧/这一段的亮度
//! 上限是多少、平均亮度是多少。把答案归一成 [`HdrMeta`]（毫尼特整数，
//! 无浮点——内核面 f32 无 floor/powf），格式差异只落在 kind 位上，
//! 深化归 F4409（媒体深化，前向声明）。元数据缺失（流不带或解析失败）
//! 不是死路：回退静态映射，用缺省峰值曲线——降级但可播。
//!
//! # 四、色调映射为什么是「可调曲线表」而不是写死公式
//!
//! HDR→SDR 的映射没有全局正确答案：暗房要压肩保暗部，日光房要抬膝点
//! 保亮部。故曲线 = 表（[`TONE_CURVES`] 三档预置）+ 参数（膝点/目标
//! 白/斜率均可调），按批次整批映射（锚点「映射 O(像素批次)」——逐
//! 像素 O(1)，批次总量线性）。映射公式用整数 Reinhard 类压缩：
//! `out = in·S·K / (K + in·S)`（K 膝点、S 目标白，全毫尼特），零溢出
//! 面用 saturating，输出渐近不超目标白——亮部压得下去，暗部线性保真。
//!
//! # 五、同屏混合为什么钳制在 203 nit
//!
//! SDR 参考白普遍锚 203 nit（Bt.2408 口径）。HDR 视频调图后叠上 SDR
//! 界面，合成亮度可能超屏与观察者的舒适域——锚点「混合过曝→亮度钳制」。
//! 钳制在混合段做（O(1)：一条 checked 加法 + 上限），不在映射段做——
//! 映射段的输出要保真给 HDR 语义，钳制是**合成层**的职责。超限本身
//! 计数留痕（过曝不是被吞掉的事实），供诊断面板读屏播报。
//!
//! # 六、跨批对接与无障碍（锚点原文）
//!
//! - 上游：F4402 [`crate::svstar2::vev02_monitor::HdrCapability`] 能力
//!   快照（探测段真源之一）；F4403 色彩引擎的降级语义在此延伸。
//! - 下游：F4409 媒体深化（动态元数据逐段解析）、F4426 多屏协调
//!   （每屏一份 HdrPlan，协调层汇总）——均前向声明，本项只定契约位。
//! - 无障碍：HDR 亮度警告读屏播报（[`HdrPlan::screen_line`]）——
//!   亮度警告是域本色输出，不是日志。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::svstar2::vev02_monitor::HdrCapability;

// ---------------------------------------------------------------------------
// 一、错误码（字符串码家族格式，与 F4403 同族风格）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const HDR_PIPELINE_VERSION: &str = "V04-hdr-v1";

/// 探测失准（两源分歧，已按 EDID 声明选边 + 标注）。
pub const E_HDR_PROBE: &str = "E_HDR_PROBE";
/// 元数据缺失（静态映射回退）。
pub const E_HDR_META: &str = "E_HDR_META";
/// 色调映射输入非法（亮度表越界/曲线参数非法）。
pub const E_HDR_TONEMAP: &str = "E_HDR_TONEMAP";
/// 混合过曝（已亮度钳制 + 计数留痕）。
pub const E_HDR_MIX: &str = "E_HDR_MIX";

// ---------------------------------------------------------------------------
// 二、段一：HDR 能力探测（O(1)，双源对账）
// ---------------------------------------------------------------------------

/// 探测结论（锚点「探测失准→以 EDID 声明为准标注」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeVerdict {
    /// 两源一致：HDR 可用。
    Active,
    /// 两源一致：HDR 不可用（走 SDR 常规路径）。
    Inactive,
    /// 失准标注：快照未测到但 EDID 声明——按声明走 HDR 并标注。
    DivergedEdidWins { snapshot: HdrCapability, note: String },
    /// 失准标注：快照测到但 EDID 无声明——按快照走 HDR 并标注。
    DivergedSnapshotWins { note: String },
}

impl ProbeVerdict {
    /// 是否走 HDR（Active 与两种分歧态都走；分歧态带标注）。
    pub fn hdr_active(&self) -> bool {
        !matches!(self, ProbeVerdict::Inactive)
    }

    /// 失准标注码（分歧态返回 Some，一致态 None）。
    pub fn note_reason(&self) -> Option<&'static str> {
        match self {
            ProbeVerdict::DivergedEdidWins { .. } | ProbeVerdict::DivergedSnapshotWins { .. } => {
                Some(E_HDR_PROBE)
            }
            _ => None,
        }
    }
}

/// 探测（O(1)：两源各读一字段对账，无循环）。
///
/// - 快照 `Present` + EDID 声明 → Active；
/// - 快照 `NotDetected` + 无声明 → Inactive；
/// - 分歧 → 以 EDID 为准选边 + 标注（锚点降级矩阵第一条）。
pub fn probe(cap: &HdrCapability, edid_declares_hdr: bool) -> ProbeVerdict {
    match (cap, edid_declares_hdr) {
        (HdrCapability::Present, true) => ProbeVerdict::Active,
        (HdrCapability::NotDetected, false) => ProbeVerdict::Inactive,
        (HdrCapability::NotDetected, true) => ProbeVerdict::DivergedEdidWins {
            snapshot: HdrCapability::NotDetected,
            note: "运行时快照未测到 HDR，按 EDID 出厂声明走 HDR 并标注探测失准".to_string(),
        },
        (HdrCapability::Present, false) => ProbeVerdict::DivergedSnapshotWins {
            note: "运行时快照测到 HDR 但 EDID 无声明，按快照走 HDR 并标注失准".to_string(),
        },
    }
}

// ---------------------------------------------------------------------------
// 三、段二：元数据抽象层（HDR10 静态 / 动态类归一，O(1)）
// ---------------------------------------------------------------------------

/// 元数据类别（抽象层只认「静态/动态」二分；格式差异归 F4409 深化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetaKind {
    /// HDR10 静态元数据（全片一套）。
    Static10,
    /// 动态元数据类（HDR10+/Dolby Vision 类，逐段参数）。
    Dynamic,
}

/// 统一元数据抽象（**毫尼特整数**——1 nit = 1000 millinit，内核零浮点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HdrMeta {
    /// 类别。
    pub kind: MetaKind,
    /// 内容峰值亮度（毫尼特，MaxCLL 归一）。
    pub max_cll_minit: u32,
    /// 帧平均峰值（毫尼特，MaxFALL 归一）。
    pub max_fall_minit: u32,
    /// 屏侧峰值（毫尼特，来自 F4402 档位/EDID；映射目标上限的输入之一）。
    pub panel_peak_minit: u32,
}

/// 元数据缺失的静态回退峰值（缺省 MaxCLL 假设：1000 nit）。
pub const DEFAULT_CLL_MINIT: u32 = 1_000_000;

/// 元数据解析（O(1)）。kind 缺失或峰值非法（0）即判「元数据缺失」——
/// 调用方走静态映射回退（锚点降级矩阵第二条）。
pub fn parse_meta(
    kind: Option<MetaKind>,
    max_cll_minit: u32,
    max_fall_minit: u32,
    panel_peak_minit: u32,
) -> Result<HdrMeta, ()> {
    match kind {
        Some(k) if max_cll_minit > 0 && max_cll_minit >= max_fall_minit => Ok(HdrMeta {
            kind: k,
            max_cll_minit,
            max_fall_minit,
            panel_peak_minit,
        }),
        _ => Err(()),
    }
}

/// 静态回退元数据（元数据缺失时构造：类别=静态、峰值=缺省假设）。
pub fn static_fallback_meta(panel_peak_minit: u32) -> HdrMeta {
    HdrMeta {
        kind: MetaKind::Static10,
        max_cll_minit: DEFAULT_CLL_MINIT,
        max_fall_minit: DEFAULT_CLL_MINIT / 4,
        panel_peak_minit,
    }
}

// ---------------------------------------------------------------------------
// 四、段三：色调映射（HDR→SDR 曲线表 + 可调参数，O(像素批次)）
// ---------------------------------------------------------------------------

/// SDR 目标白（毫尼特，Bt.2408 参考白 203 nit）。
pub const SDR_REF_WHITE_MINIT: u32 = 203_000;

/// 曲线参数（**可调**：膝点/目标白/斜率三参决定压缩形状）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToneCurve {
    /// 膝点（毫尼特）：低于此值线性保真。
    pub knee_minit: u32,
    /// 目标白（毫尼特）：输出上限。
    pub target_minit: u32,
    /// 斜率放大（千分比，1000=1.0x；可调曝光补偿）。
    pub gain_per_mille: u32,
}

impl ToneCurve {
    /// 参数合法性（膝点>0、目标白>0、斜率千分比 ∈ [1000, 8_000_000]——
    /// 上限防 u128 中间量溢出，只许提不许压过零）。
    pub fn legal(&self) -> bool {
        self.knee_minit > 0
            && self.target_minit > 0
            && self.gain_per_mille >= 1000
            && self.gain_per_mille <= 8_000_000
    }

    /// 单点映射（O(1)/像素）。
    ///
    /// 整数 Reinhard 类：`out = s·T / (C + s)`，s = in·gain/1000（提亮
    /// 入射）、T = 目标白、C = 膝点（**半饱和入射**：s=C 时 out=T/2）。
    /// 小入射近似线性（out ≈ s·T/C），大入射渐近 T 不超（钳在渐近性上，
    /// 不靠 if 截断）；膝点越大压缩越晚——三参皆可调。u128 中间量防溢出。
    pub fn map(&self, luma_minit: u32) -> u32 {
        if !self.legal() || luma_minit == 0 {
            return 0;
        }
        let s = (luma_minit as u128) * (self.gain_per_mille as u128) / 1000;
        let t = self.target_minit as u128;
        let c = self.knee_minit as u128;
        let out = (s * t) / (c + s);
        out.min(t as u128) as u32
    }
}

/// 预置曲线表（映射曲线表——锚点数据结构之三）。
///
/// | 名 | 膝点 | 目标白 | 用途 |
/// |---|---|---|---|
/// | `bt2390-like` | 100 nit | 203 nit | 暗房口径（保暗部） |
/// | `bright-room` | 300 nit | 203 nit | 日光房（抬膝保亮部可辨） |
/// | `clip-risk` | 500 nit | 203 nit | 激进（亮部细节换观感冲击） |
pub const TONE_CURVES: [(&str, u32, u32); 3] =
    [("bt2390-like", 100_000, SDR_REF_WHITE_MINIT), ("bright-room", 300_000, SDR_REF_WHITE_MINIT), ("clip-risk", 500_000, SDR_REF_WHITE_MINIT)];

/// 按名取曲线（O(1) 查表；未知名 → None——表外形态拒绝）。
pub fn curve_by_name(name: &str) -> Option<ToneCurve> {
    TONE_CURVES
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, k, t)| ToneCurve { knee_minit: *k, target_minit: *t, gain_per_mille: 1000 })
}

/// 曲线选择：按内容峰值分档（判据侧字面量：>4000nit→clip-risk、
/// >2000nit→bright-room、否则 bt2390-like）。返回曲线名与曲线一并，
/// 供计划单直接落名（不做 knee 反查——反查在表含重复膝点时会错配）。
pub fn pick_curve(meta: &HdrMeta) -> (&'static str, ToneCurve) {
    let name = if meta.max_cll_minit > 4_000_000 {
        "clip-risk"
    } else if meta.max_cll_minit > 2_000_000 {
        "bright-room"
    } else {
        "bt2390-like"
    };
    let curve = curve_by_name(name)
        .unwrap_or(ToneCurve { knee_minit: 100_000, target_minit: SDR_REF_WHITE_MINIT, gain_per_mille: 1000 });
    (name, curve)
}

/// 批次映射（O(像素批次)：逐像素 O(1)，总量线性；就地产输出批次）。
pub fn tonemap_batch(hdr_luma: &[u32], curve: &ToneCurve) -> Vec<u32> {
    hdr_luma.iter().map(|v| curve.map(*v)).collect()
}

// ---------------------------------------------------------------------------
// 五、段四：SDR 同屏混合（O(1)，亮度钳制）
// ---------------------------------------------------------------------------

/// 同屏合成钳制上限（毫尼特，203 nit 参考白 ×1000）。
pub const MIX_CLAMP_MINIT: u32 = 203_000;

/// 混合方案（O(1) 产物：HDR 已映射亮度 + SDR 界面亮度 → 钳制后合成值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixPlan {
    /// 映射后 HDR 亮度（毫尼特）。
    pub hdr_mapped_minit: u32,
    /// SDR 界面亮度（毫尼特）。
    pub sdr_ui_minit: u32,
    /// 钳制后合成亮度（毫尼特）。
    pub composited_minit: u32,
    /// 本次合成是否发生钳制（过曝留痕——被钳不是被吞）。
    pub clamped: bool,
}

/// 同屏混合（O(1)：一条 saturating 加法 + 上限对账）。
///
/// 锚点「混合过曝→亮度钳制」：合成值超 [`MIX_CLAMP_MINIT`] 即钳到上限，
/// `clamped` 置真留痕（计数进 [`HdrPipeline::clamped_mixes`]）。
pub fn mix_overlay(hdr_mapped_minit: u32, sdr_ui_minit: u32) -> MixPlan {
    let raw = hdr_mapped_minit.saturating_add(sdr_ui_minit);
    let clamped = raw > MIX_CLAMP_MINIT;
    MixPlan {
        hdr_mapped_minit,
        sdr_ui_minit,
        composited_minit: if clamped { MIX_CLAMP_MINIT } else { raw },
        clamped,
    }
}

// ---------------------------------------------------------------------------
// 六、四段管线总成（主流程编排 + 读屏播报）
// ---------------------------------------------------------------------------

/// 单屏 HDR 管线计划（四段产物钉在一张单上，可查可读屏）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HdrPlan {
    /// 段一结论。
    pub verdict: ProbeVerdict,
    /// 段二元数据（None = 缺失已回退静态，reason 在 `meta_reason`）。
    pub meta: Option<HdrMeta>,
    /// 元数据缺失回退码（E_HDR_META；正常态 None）。
    pub meta_reason: Option<&'static str>,
    /// 段三曲线名（映射曲线表键）。
    pub curve_name: &'static str,
    /// 段四混合方案（峰值像素 + 界面亮度合成口径）。
    pub mix: MixPlan,
    /// 失准标注（段一分歧态的人话，可查）。
    pub probe_note: Option<String>,
}

/// 四段管线总成（每屏一份；F4426 多屏协调层持有 Vec<HdrPlan>）。
#[derive(Clone, Debug, Default)]
pub struct HdrPipeline {
    /// 钳制发生的合成次数（过曝计数——诊断面板读屏用，只增不清）。
    pub clamped_mixes: u32,
    /// 已编排的屏数。
    pub planned_displays: u32,
}

impl HdrPipeline {
    /// 主流程编排：四段串接，段间降级就地落地（锚点五类要点主流程）。
    ///
    /// 流程：探测对账 →（HDR 不活跃则产 SDR 计划直接返回）→ 元数据
    /// 解析（缺失回退静态）→ 曲线选择与峰值映射 → 混合钳制 → 读屏
    /// 播报行可查。探测/解析 O(1)，映射 O(像素批次)（此处按峰值单点
    /// 计划，整批映射走 [`tonemap_batch`]），混合 O(1)。
    pub fn plan_for_display(
        &mut self,
        cap: &HdrCapability,
        edid_declares_hdr: bool,
        meta_in: Result<HdrMeta, ()>,
        sdr_ui_minit: u32,
    ) -> HdrPlan {
        self.planned_displays = self.planned_displays.saturating_add(1);
        let verdict = probe(cap, edid_declares_hdr);
        if !verdict.hdr_active() {
            // SDR 常规路径：无映射无混合，混合段退化为纯界面亮度。
            let mix = mix_overlay(0, sdr_ui_minit.min(MIX_CLAMP_MINIT));
            return HdrPlan {
                verdict,
                meta: None,
                meta_reason: None,
                curve_name: "bt2390-like",
                mix,
                probe_note: None,
            };
        }
        // 段二：元数据缺失 → 静态回退（标注 E_HDR_META，不静默）。
        let (meta, meta_reason) = match meta_in {
            Ok(m) => (m, None),
            Err(()) => (static_fallback_meta(DEFAULT_CLL_MINIT), Some(E_HDR_META)),
        };
        // 段三：曲线选择（按名落档）+ 峰值映射（内容峰值过映射得合成入参）。
        let (curve_name, curve) = pick_curve(&meta);
        let hdr_mapped = curve.map(meta.max_cll_minit);
        // 段四：混合 + 钳制留痕。
        let mut mix = mix_overlay(hdr_mapped, sdr_ui_minit);
        if mix.clamped {
            self.clamped_mixes = self.clamped_mixes.saturating_add(1);
        }
        let probe_note = match &verdict {
            ProbeVerdict::DivergedEdidWins { note, .. } | ProbeVerdict::DivergedSnapshotWins { note } => {
                Some(note.clone())
            }
            _ => None,
        };
        HdrPlan {
            verdict,
            meta: Some(meta),
            meta_reason,
            curve_name,
            mix,
            probe_note,
        }
    }

    /// 读屏播报（锚点「HDR 亮度警告读屏播报」——域本色输出）。
    ///
    /// 播报优先级：失准标注 > 钳制警告 > 正常态一句话。人话、单行、
    /// 可查（诊断面板与设置页同一行文本）。
    pub fn screen_line(plan: &HdrPlan) -> String {
        if let Some(note) = &plan.probe_note {
            return format!("HDR 探测失准标注：{}（{}）", note, E_HDR_PROBE);
        }
        if plan.mix.clamped {
            return format!(
                "HDR 亮度警告：同屏合成 {} 毫尼特超上限，已钳制到 {}（{}）",
                plan.mix.hdr_mapped_minit.saturating_add(plan.mix.sdr_ui_minit),
                MIX_CLAMP_MINIT,
                E_HDR_MIX
            );
        }
        if plan.verdict.hdr_active() {
            format!(
                "HDR 播放中：{} 曲线，内容峰值 {} 毫尼特映射至 {}",
                plan.curve_name,
                plan.meta.map(|m| m.max_cll_minit).unwrap_or(0),
                plan.mix.hdr_mapped_minit
            )
        } else {
            "HDR 未启用：本屏无 HDR 能力，走 SDR 常规路径".to_string()
        }
    }
}
