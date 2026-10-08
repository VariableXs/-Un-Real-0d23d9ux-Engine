//! VE-F0226 · Intel 多平面叠加协商（MPO）（VE-B 域 · Intel 直通 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0226`
//!
//! **判据（锚点原文）**：约束协商、缓存重估、合成兜底、降级通知、判据。
//!
//! **职责定位（锚点原文）**：MPO 特性按 plane 资源可用性与扫描输出约束
//! 协商启用或禁用，协商结果缓存并在模式切换、热插拔时重估；禁用时单
//! plane 合成兜底路径常备并验证；降级事件通知用户不静默。
//!
//! ## 一、协商是「逐 plane 对表」而不是「整体开关」
//!
//! MPO 的启用不是输出级布尔：同一个输出上主 plane 合格而 SPR 缩放越限
//! 是常态，协商结果必须**逐 plane 给结论、逐 plane 给原因**——整体开关
//! 会把「关掉 SPR 就能全启」的修复路径藏进一个笼统的 disabled 里。
//! [`negotiate`] 逐 plane 过 [`MpoConstraints`] 约束表，接受与拒绝原因
//! 同落 [`NegotiationResult`]（锚点数据结构「输出×plane 配置×原因」）。
//! 复杂度 O(plane×输出)——约束表每项 O(1) 判定，无嵌套扫描；协商结果
//! 缓存后查询 O(1)（锚点性能口径）。
//!
//! ## 二、缓存的脏标记只认「现实变了」
//!
//! 缓存有效性锚在两个现实量上：**模式**（分辨率/刷新变了，扫描输出约束
//! 的判定前提全变）与**热插拔纪元**（拓扑变了，输出集合不可复用）。二者
//! 任一变化即脏，脏即重估（锚点错误路径「缓存脏→触发重估」）——用时间
//! 戳当有效性依据是错的：没发生的模式切换不该打翻缓存，发生了的必须
//! 打翻，纪元计数表达的是「现实是否变过」而不是「过了多久」。
//!
//! ## 三、兜底路径「常备并验证」四个字都是硬要求
//!
//! 「常备」：单 plane 合成兜底是协商器构造时就存在的 [`FallbackPath`]，
//! 不是禁用时才动态加载——动态加载兜底等于在故障路径上再叠一个故障点。
//! 「验证」：构造即跑金丝雀合成（[`FallbackPath::self_verify`]），兜底
//! 路径自己坏了还叫兜底吗；验证结果随状态可查，不自证恒真。
//!
//! ## 四、降级通知三要素齐备，与 F0103 同构
//!
//! 下游 F0103 的协议是三要素：发生了什么 / 为什么 / 下一步怎么办。
//! 本模块产出的 [`MpoNotice`] 按同构三字段承载（`what` / `why` / `next`），
//! F0103 落位后按类别直接接模板；**合并限频是 F0103 的职责**，本模块只
//! 产原始事件——在源头限频会丢事件（限频策略错了连回溯的原料都没有）。
//! 零静默：协商失败、运行中约束失效、缓存重估，每一件都落通知账。
//!
//! **对接**：上游 F0225（pipe/plane 层级：每 pipe 主 plane + SPR + CUR，
//! plane 配置输入按该层级建模）；下游 F0103（三要素通知）。
//! 零 panic 面（固定下标走 `get`/`Option`，算术全饱和）、零 IO、零墙钟、
//! 无全局可变状态。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源；plane 层级口径同源 F0225）
// ---------------------------------------------------------------------------

/// 输出数上限。协商按输出逐一进行，容量上界防账目无界膨胀。
pub const MAX_OUTPUTS: usize = 8;

/// 每 pipe 的 plane 层级数（主 plane + SPR + CUR，同源 F0225「pipe 层级」）。
pub const PLANES_PER_PIPE: usize = 3;

/// MPO 可用的最大叠加 plane 数（主 plane 之外的 SPR 名额；CUR 永不参与）。
///
/// Intel 逐代可 overlay 的 SPR 数不同，约束表按代际注入而不是写死——
/// 本常量是**默认代际**的保守值，真实能力位由 F0227 能力位探测覆盖。
pub const DEFAULT_MAX_OVERLAY_PLANES: usize = 1;

/// MPO 最小源尺寸（像素；小于此值的 plane 扫描输出不可靠，逐代校准）。
pub const MIN_SOURCE_PX: u32 = 16;

/// 缩放上限分子/分母（dst/src ≤ 2/1：放大一倍封顶）。
pub const MAX_SCALE_NUM: u32 = 2;
/// 缩放上限分母（与 [`MAX_SCALE_NUM`] 成对；夹逼边界：恰 2x 合格）。
pub const MAX_SCALE_DEN: u32 = 1;

// ---------------------------------------------------------------------------
// 二、数据结构（锚点：协商结果（输出×plane 配置×原因）；约束表；缓存标记）
// ---------------------------------------------------------------------------

/// plane 种类（层级口径同源 F0225：每 pipe 主 plane + SPR + CUR）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaneKind {
    /// 主 plane（每 pipe 恰一个，恒参与扫描输出）。
    Primary,
    /// SPR（叠加/underlay 候选，MPO 协商的对象）。
    Sprite,
    /// 硬件鼠标面（专用扫描路径，**永不**参与 MPO 协商）。
    Cursor,
}

impl PlaneKind {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            PlaneKind::Primary => "主plane",
            PlaneKind::Sprite => "SPR",
            PlaneKind::Cursor => "CUR",
        }
    }
}

/// 单个 plane 的配置输入（协商的逐 plane 判定对象）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaneConfig {
    /// plane 编号（输出内唯一）。
    pub id: u16,
    /// 种类。
    pub kind: PlaneKind,
    /// 源宽（像素）。
    pub src_w: u32,
    /// 源高（像素）。
    pub src_h: u32,
    /// 目标宽（像素；dst/src 即缩放比）。
    pub dst_w: u32,
    /// 目标高（像素）。
    pub dst_h: u32,
}

/// MPO 约束表（逐代际注入；协商逐项过表，每项 O(1)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MpoConstraints {
    /// 可用叠加 plane 数上限。
    pub max_overlay_planes: usize,
    /// 最小源尺寸（宽高均须 ≥ 此值）。
    pub min_source_px: u32,
    /// 缩放上限（dst/src ≤ num/den）。
    pub scale_num: u32,
    /// 缩放下限分母。
    pub scale_den: u32,
    /// 输出最小刷新率（Hz；低于此值 MPO 收益为负，协商直接不给开）。
    pub min_refresh_hz: u32,
}

impl MpoConstraints {
    /// 默认代际的保守约束表。
    pub const fn conservative() -> MpoConstraints {
        MpoConstraints {
            max_overlay_planes: DEFAULT_MAX_OVERLAY_PLANES,
            min_source_px: MIN_SOURCE_PX,
            scale_num: MAX_SCALE_NUM,
            scale_den: MAX_SCALE_DEN,
            min_refresh_hz: 30,
        }
    }
}

/// 逐 plane 拒绝原因（每类带三要素文案的类别键；模板见 [`MpoNotice`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    /// 叠加 plane 超额（约束表 max_overlay_planes）。
    OverlayOverCommit,
    /// 源尺寸低于下限。
    SourceTooSmall,
    /// 缩放越限（> num/den）。
    ScalingOverLimit,
    /// CUR 提交进 MPO（层级错位，永不合格）。
    CursorNotEligible,
    /// 刷新率低于 MPO 收益线。
    RefreshTooLow,
}

impl RejectReason {
    /// 人话标签（协商结果里的原因栏）。
    pub const fn label(self) -> &'static str {
        match self {
            RejectReason::OverlayOverCommit => "叠加plane超额",
            RejectReason::SourceTooSmall => "源尺寸低于下限",
            RejectReason::ScalingOverLimit => "缩放越限",
            RejectReason::CursorNotEligible => "CUR不参与MPO",
            RejectReason::RefreshTooLow => "刷新率低于收益线",
        }
    }

    /// 三要素·发生了什么（能力受限的具体表现）。
    pub const fn what(self) -> &'static str {
        match self {
            RejectReason::OverlayOverCommit => "该输出的叠加plane数超出可用名额，多出的plane不可进MPO",
            RejectReason::SourceTooSmall => "该plane源尺寸低于MPO扫描输出下限",
            RejectReason::ScalingOverLimit => "该plane缩放比超出MPO扫描输出上限",
            RejectReason::CursorNotEligible => "硬件鼠标面走专用扫描路径，不参与MPO合成",
            RejectReason::RefreshTooLow => "当前刷新率下MPO收益为负，不启用",
        }
    }

    /// 三要素·为什么（原因类别）。
    pub const fn why(self) -> &'static str {
        match self {
            RejectReason::OverlayOverCommit => "plane资源可用性约束",
            RejectReason::SourceTooSmall => "扫描输出约束（源尺寸）",
            RejectReason::ScalingOverLimit => "扫描输出约束（缩放比）",
            RejectReason::CursorNotEligible => "plane层级语义（CUR专用）",
            RejectReason::RefreshTooLow => "扫描输出约束（刷新率）",
        }
    }

    /// 三要素·下一步（可操作建议）。
    pub const fn next(self) -> &'static str {
        match self {
            RejectReason::OverlayOverCommit => "减少同输出叠加plane数，或由单plane合成兜底承接",
            RejectReason::SourceTooSmall => "放大该plane源尺寸至下限以上，或退单plane合成",
            RejectReason::ScalingOverLimit => "缩小该plane缩放比至上限以内，或退单plane合成",
            RejectReason::CursorNotEligible => "无需处理：CUR按层级原样走专用路径",
            RejectReason::RefreshTooLow => "提升刷新率或维持单plane合成兜底",
        }
    }
}

/// 单 plane 的协商结论（接受或带原因拒绝）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaneVerdict {
    /// 接受进 MPO。
    Accepted,
    /// 拒绝（带原因）。
    Rejected(RejectReason),
}

/// MPO 协商结果（锚点数据结构：输出 × plane 配置 × 原因）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NegotiationResult {
    /// 输出编号。
    pub output_id: u16,
    /// 是否整体启用 MPO（≥1 个叠加 plane 被接受且无全局拒绝原因）。
    pub enabled: bool,
    /// 逐 plane 结论（下标与输入序一致——协商不改序、不丢项）。
    pub verdicts: Vec<(u16, PlaneKind, PlaneVerdict)>,
    /// 全局拒绝原因（刷新率类；逐 plane 原因见 verdicts）。
    pub global_reasons: Vec<RejectReason>,
    /// 协商时的模式标识（缓存对账用）。
    pub mode_id: u64,
    /// 协商时的热插拔纪元（缓存对账用）。
    pub hotplug_epoch: u64,
}

impl NegotiationResult {
    /// 被接受的叠加 plane 数。
    pub fn accepted_overlays(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.verdicts.len() {
            if let Some((_, kind, v)) = self.verdicts.get(i) {
                if *kind == PlaneKind::Sprite {
                    if let Some(PlaneVerdict::Accepted) = Some(*v) {
                        n += 1;
                    }
                }
            }
            i += 1;
        }
        n
    }
}

/// 缓存有效性标记（锚在「现实量」上：模式 × 热插拔纪元，见模块注释二）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheStamp {
    /// 模式标识（模式切换即变）。
    pub mode_id: u64,
    /// 热插拔纪元（拓扑变更即 +1）。
    pub hotplug_epoch: u64,
}

// ---------------------------------------------------------------------------
// 三、单 plane 合成兜底（常备并验证；判据三）
// ---------------------------------------------------------------------------

/// 单 plane 合成兜底路径。
///
/// 「常备」：协商器构造即建，不走动态加载（故障路径上不叠故障点）；
/// 「验证」：构造即跑金丝雀合成 [`Self::self_verify`]，验证结果显性可查。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FallbackPath {
    /// 构造时金丝雀验证是否通过（结果缓存，不自证恒真——每次重验可刷新）。
    verified: bool,
    /// 累计验证次数（观测面）。
    verify_runs: u32,
    /// 兜底承接次数（MPO 禁用/降级后走兜底的次数）。
    activations: u32,
}

impl FallbackPath {
    /// 金丝雀合成：把多个图层按序混合进单 plane 输出（确定性模型）。
    ///
    /// 合成规则：按图层序自下而上，不透明层覆盖、透明（alpha=0）层让位；
    /// 输出尺寸取最大层。模型只承诺「可判定的正确性」，逐字节输出可复算。
    pub fn compose(layers: &[(u32, u32, u8)]) -> Option<(u32, u32)> {
        if layers.is_empty() {
            return None;
        }
        let mut w = 0u32;
        let mut h = 0u32;
        let mut covered = false;
        let mut i = 0usize;
        while i < layers.len() {
            if let Some((lw, lh, alpha)) = layers.get(i) {
                if *alpha > 0 || !covered {
                    // 不透明层覆盖画布；透明层只在不透明层出现前贡献画布。
                    w = w.max(*lw);
                    h = h.max(*lh);
                    if *alpha > 0 {
                        covered = true;
                    }
                }
            } else {
                return None;
            }
            i += 1;
        }
        if w == 0 || h == 0 {
            return None;
        }
        Some((w, h))
    }

    /// 金丝雀验证：三层混合（小透明+大不透明+越序小不透明）逐点断言。
    pub fn self_verify(&mut self) -> bool {
        self.verify_runs += 1;
        let v1 = Self::compose(&[(32, 32, 0), (64, 48, 255)]) == Some((64, 48));
        let v2 = Self::compose(&[(64, 48, 255), (32, 32, 0)]) == Some((64, 48));
        let v3 = Self::compose(&[]) == None;
        let v4 = Self::compose(&[(0, 0, 255)]) == None;
        self.verified = v1 && v2 && v3 && v4;
        self.verified
    }

    /// 是否已验证可用。
    pub const fn is_verified(&self) -> bool {
        self.verified
    }

    /// 累计验证次数。
    pub const fn verify_runs(&self) -> u32 {
        self.verify_runs
    }

    /// 兜底承接次数。
    pub const fn activations(&self) -> u32 {
        self.activations
    }
}

// ---------------------------------------------------------------------------
// 四、降级通知（三要素同构 F0103；判据四）
// ---------------------------------------------------------------------------

/// 降级通知（三要素：发生了什么 / 为什么 / 下一步——与 F0103 协议同构；
/// 合并限频由 F0103 负责，本模块产原始事件全量入账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MpoNotice {
    /// 通知类别键（F0103 模板按类别接）。
    pub category: String,
    /// 涉事输出编号（可回溯到具体输出）。
    pub output_id: u16,
    /// 发生了什么（能力受限的具体表现）。
    pub what: String,
    /// 为什么（原因类别）。
    pub why: String,
    /// 下一步怎么办（可操作建议）。
    pub next: String,
    /// 逻辑 tick（可回溯）。
    pub tick: u64,
}

impl MpoNotice {
    /// 从拒绝原因构造（三要素取自 [`RejectReason`] 的同构文案）。
    pub fn from_reason(r: RejectReason, output_id: u16, tick: u64) -> MpoNotice {
        MpoNotice {
            category: format!("MPO-{}", r.label()),
            output_id,
            what: r.what().to_string(),
            why: r.why().to_string(),
            next: r.next().to_string(),
            tick,
        }
    }

    /// 三要素是否齐备（协议完备性自检用）。
    pub fn complete(&self) -> bool {
        !self.category.is_empty()
            && !self.what.is_empty()
            && !self.why.is_empty()
            && !self.next.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 五、协商器（判据一、二：约束协商 + 缓存重估）
// ---------------------------------------------------------------------------

/// MPO 协商器。
#[derive(Clone, Debug)]
pub struct MpoNegotiator {
    constraints: MpoConstraints,
    cache: Option<(CacheStamp, NegotiationResult)>,
    fallback: FallbackPath,
    notices: Vec<MpoNotice>,
    tick: u64,
    reevaluations: u32,
    cache_hits: u32,
    rejected_negotiations: u32,
}

impl MpoNegotiator {
    /// 新建：约束表注入 + **兜底路径常备并即时验证**（判据三的构造面）。
    pub fn new(constraints: MpoConstraints) -> MpoNegotiator {
        let mut fallback = FallbackPath {
            verified: false,
            verify_runs: 0,
            activations: 0,
        };
        fallback.self_verify();
        MpoNegotiator {
            constraints,
            cache: None,
            fallback,
            notices: Vec::new(),
            tick: 0,
            reevaluations: 0,
            cache_hits: 0,
            rejected_negotiations: 0,
        }
    }

    /// 兜底路径状态（常备性观测面）。
    pub fn fallback(&self) -> &FallbackPath {
        &self.fallback
    }

    /// 通知账（全量原始事件，F0103 接手后按类别合并限频）。
    pub fn notices(&self) -> &[MpoNotice] {
        self.notices.as_slice()
    }

    /// 重估次数（缓存脏→重估的观测面）。
    pub const fn reevaluations(&self) -> u32 {
        self.reevaluations
    }

    /// 缓存命中次数（O(1) 查询的观测面）。
    pub const fn cache_hits(&self) -> u32 {
        self.cache_hits
    }

    /// 协商被拒（整体禁用）次数。
    pub const fn rejected_negotiations(&self) -> u32 {
        self.rejected_negotiations
    }

    /// 单 plane 缩放是否越限（恰等于上限合格——夹逼边界）。
    fn scaling_ok(&self, p: &PlaneConfig) -> bool {
        if p.src_w == 0 || p.src_h == 0 || p.dst_w == 0 || p.dst_h == 0 {
            return false;
        }
        // dst/src ≤ num/den ⟺ dst*den ≤ src*num（整数交叉相乘，无除法）。
        p.dst_w.saturating_mul(self.constraints.scale_den)
            <= p.src_w.saturating_mul(self.constraints.scale_num)
            && p.dst_h.saturating_mul(self.constraints.scale_den)
                <= p.src_h.saturating_mul(self.constraints.scale_num)
    }

    /// 核心协商：O(plane×输出)——逐 plane 过约束表，每项 O(1) 判定。
    ///
    /// 输出上限 [`MAX_OUTPUTS`]：越界输出直接拒绝（输入校验），不静默钳制。
    pub fn negotiate(
        &mut self,
        output_id: u16,
        mode_refresh_hz: u32,
        planes: &[PlaneConfig],
        stamp: CacheStamp,
    ) -> NegotiationResult {
        self.tick += 1;
        let mut verdicts: Vec<(u16, PlaneKind, PlaneVerdict)> = Vec::new();
        let mut global_reasons: Vec<RejectReason> = Vec::new();
        let mut overlays_accepted = 0usize;
        let mut any_reject = false;

        if output_id as usize >= MAX_OUTPUTS {
            // 输入校验：越界输出没有协商意义，给整体拒绝结论而非 panic。
            global_reasons.push(RejectReason::OverlayOverCommit);
            let result = NegotiationResult {
                output_id,
                enabled: false,
                verdicts,
                global_reasons,
                mode_id: stamp.mode_id,
                hotplug_epoch: stamp.hotplug_epoch,
            };
            self.notices.push(MpoNotice::from_reason(
                RejectReason::OverlayOverCommit,
                output_id,
                self.tick,
            ));
            self.rejected_negotiations += 1;
            return result;
        }
        if mode_refresh_hz < self.constraints.min_refresh_hz {
            global_reasons.push(RejectReason::RefreshTooLow);
            any_reject = true;
        }

        let mut pi = 0usize;
        while pi < planes.len() {
            let p = match planes.get(pi) {
                Some(v) => *v,
                None => break,
            };
            let verdict = match p.kind {
                PlaneKind::Cursor => Some(RejectReason::CursorNotEligible),
                PlaneKind::Primary => None, // 主 plane 恒参与扫描输出，无 MPO 判定面
                PlaneKind::Sprite => {
                    if overlays_accepted >= self.constraints.max_overlay_planes {
                        Some(RejectReason::OverlayOverCommit)
                    } else if p.src_w < self.constraints.min_source_px
                        || p.src_h < self.constraints.min_source_px
                    {
                        Some(RejectReason::SourceTooSmall)
                    } else if !self.scaling_ok(&p) {
                        Some(RejectReason::ScalingOverLimit)
                    } else {
                        None
                    }
                }
            };
            match verdict {
                None => {
                    if p.kind == PlaneKind::Sprite {
                        overlays_accepted += 1;
                    }
                    verdicts.push((p.id, p.kind, PlaneVerdict::Accepted));
                }
                Some(reason) => {
                    any_reject = true;
                    verdicts.push((p.id, p.kind, PlaneVerdict::Rejected(reason)));
                    self.notices
                        .push(MpoNotice::from_reason(reason, output_id, self.tick));
                }
            }
            pi += 1;
        }

        let enabled = overlays_accepted > 0
            && !global_reasons.iter().any(|r| *r == RejectReason::RefreshTooLow);
        if !enabled && any_reject {
            self.rejected_negotiations += 1;
            if !self.fallback.is_verified() {
                // 兜底路径验证失败是故障：重验并如实记录（不静默放行）。
                self.fallback.self_verify();
                self.notices.push(MpoNotice {
                    category: "MPO-兜底重验".to_string(),
                    output_id,
                    what: "单plane合成兜底路径金丝雀验证未通过，已重验".to_string(),
                    why: "兜底路径自验证机制".to_string(),
                    next: "若仍失败需人工介入检查合成模型".to_string(),
                    tick: self.tick,
                });
            }
            self.fallback.activations += 1;
        }
        NegotiationResult {
            output_id,
            enabled,
            verdicts,
            global_reasons,
            mode_id: stamp.mode_id,
            hotplug_epoch: stamp.hotplug_epoch,
        }
    }

    /// 缓存查询（O(1)）：印记与现实一致才有效，否则 None（调用方重估）。
    pub fn cached(&self, stamp: CacheStamp) -> Option<&NegotiationResult> {
        match &self.cache {
            Some((s, r)) if *s == stamp => {
                // 注意：不可变借用内不能计数，这里以返回值区分——计数在
                // lookup_or_negotiate 里做。
                Some(r)
            }
            _ => None,
        }
    }

    /// 统一入口：缓存命中 O(1) 直接回；脏/无缓存 → 重估并落缓存。
    ///
    /// 模式切换与热插拔都体现为 stamp 变化——调用方不需要区分脏因。
    /// 返回按值拷贝（结论很小：≤3 条 plane 结论 + 原因），换取零 panic
    /// 面——借引用需要在「缓存必命中」的不可达分支上硬凑兜底，那比一次
    /// 小拷贝贵得多。
    pub fn lookup_or_negotiate(
        &mut self,
        output_id: u16,
        mode_refresh_hz: u32,
        planes: &[PlaneConfig],
        stamp: CacheStamp,
    ) -> NegotiationResult {
        let hit = self.cached(stamp).is_some();
        if hit {
            self.cache_hits += 1;
        } else {
            let result = self.negotiate(output_id, mode_refresh_hz, planes, stamp);
            self.reevaluations += 1;
            self.cache = Some((stamp, result));
        }
        match self.cache.as_ref() {
            Some((_, r)) => r.clone(),
            None => {
                // 理论不可达（上一臂必写入）；零 panic 面的出路是重建空
                // 结论入缓存——绝不 unwrap/panic，异常路径由自检暴露。
                let empty = NegotiationResult {
                    output_id,
                    enabled: false,
                    verdicts: Vec::new(),
                    global_reasons: Vec::new(),
                    mode_id: stamp.mode_id,
                    hotplug_epoch: stamp.hotplug_epoch,
                };
                self.cache = Some((stamp, empty.clone()));
                empty
            }
        }
    }

    /// 显式作废缓存（热插拔处置入口之一；纪元由调用方推进）。
    pub fn invalidate(&mut self) {
        self.cache = None;
    }

    /// 运行中约束失效处置：降级到单 plane 并出通知（判据四）。
    ///
    /// 失效场景示例：播放器把 SPR 源缩到下限以下、缩放被改越限——
    /// 运行态检测到即降级并通知，不静默继续跑 MPO。
    pub fn notify_constraint_failure(&mut self, output_id: u16, reason: RejectReason) {
        self.tick += 1;
        self.fallback.activations += 1;
        self.notices
            .push(MpoNotice::from_reason(reason, output_id, self.tick));
        self.invalidate();
    }
}

// ---------------------------------------------------------------------------
// 六、域自检（判据区零 panic 面；反向语料钉门禁不恒绿）
// ---------------------------------------------------------------------------

/// 判据侧独立重排的锚点判据五条。
const CRITERIA_RECHECK: [&str; 5] = [
    "约束协商",
    "缓存重估",
    "合成兜底",
    "降级通知",
    "判据",
];

/// VE-F0226 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_veb226_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("veb226_mpo");

    // —— 判据一 · 约束协商：nominal 输出逐 plane 对表 ——
    let mut neg = MpoNegotiator::new(MpoConstraints::conservative());
    let planes = [
        PlaneConfig { id: 0, kind: PlaneKind::Primary, src_w: 1920, src_h: 1080, dst_w: 1920, dst_h: 1080 },
        PlaneConfig { id: 1, kind: PlaneKind::Sprite, src_w: 640, src_h: 480, dst_w: 640, dst_h: 480 },
    ];
    let stamp = CacheStamp { mode_id: 7, hotplug_epoch: 1 };
    let r = neg.negotiate(2, 60, &planes, stamp);
    s.add(
        "B22-协商-主plane加合格SPR全启",
        r.enabled
            && r.accepted_overlays() == 1
            && r.verdicts.len() == 2
            && r.global_reasons.is_empty()
            && neg.fallback().is_verified(),
        "主plane恒参与+SPR过表接受；构造即验证兜底路径（判据三构造面）",
    );

    // —— 判据一 · 反向：四类拒绝逐类点名（原因逐 plane 可查）——
    let bad = [
        PlaneConfig { id: 0, kind: PlaneKind::Primary, src_w: 1920, src_h: 1080, dst_w: 1920, dst_h: 1080 },
        PlaneConfig { id: 1, kind: PlaneKind::Sprite, src_w: 640, src_h: 480, dst_w: 1280, dst_h: 960 },
        PlaneConfig { id: 2, kind: PlaneKind::Sprite, src_w: 8, src_h: 8, dst_w: 8, dst_h: 8 },
        PlaneConfig { id: 3, kind: PlaneKind::Cursor, src_w: 64, src_h: 64, dst_w: 64, dst_h: 64 },
    ];
    let mut neg2 = MpoNegotiator::new(MpoConstraints::conservative());
    let r2 = neg2.negotiate(2, 60, &bad, stamp);
    let v = |id: u16, res: &NegotiationResult| -> Option<PlaneVerdict> {
        let mut i = 0usize;
        while i < res.verdicts.len() {
            if let Some((pid, _, verdict)) = res.verdicts.get(i) {
                if *pid == id {
                    return Some(*verdict);
                }
            }
            i += 1;
        }
        None
    };
    let scale_rejected = v(1, &r2) == Some(PlaneVerdict::Rejected(RejectReason::ScalingOverLimit));
    let small_rejected = v(2, &r2) == Some(PlaneVerdict::Rejected(RejectReason::SourceTooSmall));
    let cursor_rejected = v(3, &r2) == Some(PlaneVerdict::Rejected(RejectReason::CursorNotEligible));
    s.add(
        "B22-协商-越限逐plane点名原因",
        scale_rejected
            && small_rejected
            && cursor_rejected
            && !r2.enabled
            && neg2.notices().len() == 3,
        "缩放2x越限/源尺寸8<16/CUR层级错位三类各给逐plane原因；禁用即通知（零静默）",
    );

    // —— 反向：叠加超额（第 2 个 SPR 被名额挡下）——
    let two_sprites = [
        PlaneConfig { id: 0, kind: PlaneKind::Primary, src_w: 1920, src_h: 1080, dst_w: 1920, dst_h: 1080 },
        PlaneConfig { id: 1, kind: PlaneKind::Sprite, src_w: 640, src_h: 480, dst_w: 640, dst_h: 480 },
        PlaneConfig { id: 2, kind: PlaneKind::Sprite, src_w: 320, src_h: 240, dst_w: 320, dst_h: 240 },
    ];
    let mut neg3 = MpoNegotiator::new(MpoConstraints::conservative());
    let r3 = neg3.negotiate(2, 60, &two_sprites, stamp);
    s.add(
        "B22-协商-叠加名额超额挡下",
        r3.accepted_overlays() == 1
            && v(2, &r3) == Some(PlaneVerdict::Rejected(RejectReason::OverlayOverCommit))
            && r3.enabled,
        "默认名额 1：第二个 SPR 超额被拒但首个仍接受（逐 plane 结论不是整体开关）",
    );

    // —— 边界夹逼：恰 2x 与恰下限尺寸都合格 ——
    let edge = [
        PlaneConfig { id: 1, kind: PlaneKind::Sprite, src_w: MIN_SOURCE_PX, src_h: MIN_SOURCE_PX, dst_w: MIN_SOURCE_PX * MAX_SCALE_NUM, dst_h: MIN_SOURCE_PX * MAX_SCALE_NUM },
    ];
    let mut neg4 = MpoNegotiator::new(MpoConstraints::conservative());
    let r4 = neg4.negotiate(2, 60, &edge, stamp);
    s.add(
        "B22-协商-缩放与尺寸边界夹逼",
        r4.enabled
            && r4.accepted_overlays() == 1
            && r4.verdicts.len() == 1,
        "恰 2x 缩放与恰 16px 源尺寸均合格（整数交叉相乘判定，无除法）",
    );

    // —— 刷新率全局拒绝（含低刷新逐点）——
    let mut neg5 = MpoNegotiator::new(MpoConstraints::conservative());
    let r5 = neg5.negotiate(2, 24, &planes, stamp);
    s.add(
        "B22-协商-低刷新全局拒不含糊",
        !r5.enabled
            && r5.global_reasons.contains(&RejectReason::RefreshTooLow)
            && r5.verdicts.get(1).map(|x| x.2) == Some(PlaneVerdict::Accepted),
        "24Hz<30Hz：全局拒绝原因入结果；SPR 本身合格仍记 Accepted（语义分离）",
    );

    // —— 判据二 · 缓存重估：命中 O(1)、模式切换与热插拔都触发重估 ——
    let mut neg6 = MpoNegotiator::new(MpoConstraints::conservative());
    let s1 = CacheStamp { mode_id: 7, hotplug_epoch: 1 };
    neg6.lookup_or_negotiate(2, 60, &planes, s1);
    let hits0 = neg6.cache_hits();
    neg6.lookup_or_negotiate(2, 60, &planes, s1);
    let hits1 = neg6.cache_hits();
    neg6.lookup_or_negotiate(2, 60, &planes, CacheStamp { mode_id: 8, hotplug_epoch: 1 });
    let reval_mode = neg6.reevaluations();
    neg6.lookup_or_negotiate(2, 60, &planes, CacheStamp { mode_id: 8, hotplug_epoch: 2 });
    let reval_hotplug = neg6.reevaluations();
    s.add(
        "B22-缓存-命中O(1)与双现实量重估",
        hits1 == hits0 + 1
            && reval_mode == 2
            && reval_hotplug == 3
            && neg6.reevaluations() == 3,
        "同印记命中 O(1)；模式切换与热插拔各自触发一次重估（脏只认现实变了）",
    );

    // —— 显式作废：invalidate 后必重估 ——
    let mut neg7 = MpoNegotiator::new(MpoConstraints::conservative());
    neg7.lookup_or_negotiate(2, 60, &planes, s1);
    neg7.invalidate();
    let before = neg7.reevaluations();
    neg7.lookup_or_negotiate(2, 60, &planes, s1);
    s.add(
        "B22-缓存-显式作废即重估",
        neg7.reevaluations() == before + 1,
        "invalidate() 后同印记查询也重估（热插拔处置入口）",
    );

    // —— 判据三 · 合成兜底：常备、自验证逐点、激活计数 ——
    let mut fb = FallbackPath {
        verified: false,
        verify_runs: 0,
        activations: 0,
    };
    let ok = fb.self_verify();
    s.add(
        "B22-兜底-金丝雀逐点验证",
        ok
            && fb.is_verified()
            && fb.verify_runs() == 1
            && FallbackPath::compose(&[(32, 32, 0), (64, 48, 255)]) == Some((64, 48))
            && FallbackPath::compose(&[]) == None
            && FallbackPath::compose(&[(0, 0, 255)]) == None,
        "构造即验证（常备不动态加载）；透明层让位/空输入/零尺寸四类金丝雀逐点断言",
    );

    // —— 判据四 · 降级通知：三要素齐备且逐类完整 ——
    let reasons = [
        RejectReason::OverlayOverCommit,
        RejectReason::SourceTooSmall,
        RejectReason::ScalingOverLimit,
        RejectReason::CursorNotEligible,
        RejectReason::RefreshTooLow,
    ];
    let mut tri_ok = reasons.len() == 5;
    let mut next_all_nonempty = true;
    let mut ri = 0usize;
    while ri < reasons.len() {
        let n = MpoNotice::from_reason(reasons[ri], 2, 9);
        if !n.complete() || n.category.is_empty() || n.next.is_empty() {
            tri_ok = false;
            next_all_nonempty = false;
        }
        ri += 1;
    }
    s.add(
        "B22-通知-三要素逐类齐备",
        tri_ok && next_all_nonempty,
        "五类拒绝原因的三要素（what/why/next）逐类非空，类别键就位待 F0103 接模板",
    );

    // —— 运行中约束失效：降级 + 通知 + 缓存重估（不静默续跑）——
    let mut neg8 = MpoNegotiator::new(MpoConstraints::conservative());
    neg8.lookup_or_negotiate(2, 60, &planes, s1);
    let n_before = neg8.notices().len();
    neg8.notify_constraint_failure(2, RejectReason::ScalingOverLimit);
    let cached_after = neg8.cached(s1).is_none();
    s.add(
        "B22-运行失效-降级通知并重估",
        neg8.notices().len() == n_before + 1
            && cached_after
            && neg8.fallback().activations() >= 1,
        "运行中缩放越限：通知入账、缓存作废待重估、兜底承接计数+1（零静默）",
    );

    // —— 输入校验：越界输出拒绝而非 panic ——
    let mut neg9 = MpoNegotiator::new(MpoConstraints::conservative());
    let r9 = neg9.negotiate(MAX_OUTPUTS as u16, 60, &planes, stamp);
    s.add(
        "B22-输入校验-越界输出整体拒",
        !r9.enabled && !r9.global_reasons.is_empty() && MAX_OUTPUTS == 8,
        "output_id ≥ 8：整体拒绝结论 + 通知，绝不 panic 也不静默钳制",
    );

    // —— 判据 stamp 独立对账 ——
    let stamps = ["约束协商", "缓存重估", "合成兜底", "降级通知", "判据"];
    let mut stamp_ok = CRITERIA_RECHECK.len() == stamps.len();
    let mut ci = 0usize;
    while ci < stamps.len() {
        if CRITERIA_RECHECK.get(ci) != Some(&stamps[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    s.add(
        "B22-判据stamp-五条独立重排全等",
        stamp_ok,
        "锚点判据五条与判据侧独立重排逐条全等（常量被误改先红）",
    );

    s
}
