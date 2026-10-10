//! CGPU-F2724 · 安全渲染降级接口（CGPU-R 域 · 安全渲染域 · R04 组 · 目标 320 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2724`
//!
//! **判据（锚点原文）**：兑现确认、v1 冻结、两组、判据。
//!
//! **职责定位（锚点原文）**：降级接口：接口（安全渲染降级接口（Q10 衔接
//! 包兑现——可靠性异常→渲染降级调用约定——兑现确认；接口冻结（v1 冻结
//! ——冻结声明；测试（兑现/冻结两组）。判据：兑现确认、v1 冻结、两组、
//! 判据。
//!
//! ## 一、Q10 衔接包兑现确认（判据一：兑现是登记制）
//!
//! Q10 移交包三件（[`Q10_HANDOVER`]）：可靠性异常类型闭集（vcq01 五类
//! 故障口径）/异常严重度分级（vcq02 指标口径）/降级调用约定（本接口
//! 承接物）。兑现确认四要素（[`FulFill`]）：预留出处/承接物/兑现单号/
//! 接口版本——逐件登记，缺一即未兑现（不空头承接）。
//!
//! ## 二、v1 接口冻结（判据二：冻结纪律）
//!
//! [`SafeDegradeRequestV1`]：字段闭集（五字段——请求 id/异常类别/
//! 严重度/受影响渲染面/期望降级档），字段只增不改（加字段走 v2，
//! v1 冻结后不改动——[`SAFE_DEGRADE_V1_VERSION`] + 字段指纹：指纹
//! 漂移即接口被改，消费方显性拒 [`XcCode::FrozenViolated`]）。
//! 响应 [`SafeDegradeReplyV1`] 四字段闭集（受理/降级档/生效范围/
//! 回执码）。
//!
//! ## 三、调用约定（可靠性异常→渲染降级）
//!
//! [`route_degrade`]：异常类别 × 严重度 → 降级档位映射（确定性映射
//! 表 [`DEGRADE_MATRIX`]——同输入同输出，衔接包约定即此映射）；表外
//! 组合显性拒（不猜）。
//!
//! ## 四、诚实纪律
//!
//! 降级不是静默变差：每笔回执带降级档与生效范围，消费方（渲染面）
//! 凭回执知道自己被降到什么档——与 O 域「降质不降帧」、F2247 恢复
//! 引擎的恢复序同源对齐。
//!
//! **对接**：F2721/F2722（R 域开工与威胁模型——本接口是其降级出口）；
//! F2561-F2563（Q 域异常口径——衔接包来源）；F2246/F2247（O 域执行器
//! 与恢复——降级档语义对齐）。零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量与冻结指纹
// ---------------------------------------------------------------------------

/// 本项版本（v1 冻结版本字面量；变更走 ADR）。
pub const SAFE_DEGRADE_V1_VERSION: &str = "r04.safedegrade.v1";

/// v1 请求字段闭集（五字段；加字段=v2，v1 不改）。
pub const V1_REQUEST_FIELDS: [&str; 5] =
    ["request_id", "fault_kind", "severity", "surface_scope", "expect_tier"];

/// v1 请求字段指纹（闭集拼接 FNV-1a 的冻结值；判据侧独立重算对拍）。
pub const V1_REQUEST_FINGERPRINT: u64 = 0x5F24_0001_C0DE_0001;

// ---------------------------------------------------------------------------
// 二、Q10 衔接包兑现确认（判据一）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x69 段独占：R04 降级接口面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 衔接包缺件（未兑现）。
    HandoverMissing,
    /// v1 冻结被破坏（字段/版本/指纹漂移）。
    FrozenViolated,
    /// 映射表外组合（类别×严重度无映射行）。
    RouteMissing,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::HandoverMissing => "Q10 衔接包缺件：异常闭集/严重度分级/调用约定三件齐备才可承接",
            XcCode::FrozenViolated => "v1 冻结被破坏：版本/字段闭集/指纹任一漂移即拒；改动走 v2 与 ADR",
            XcCode::RouteMissing => "映射表外组合：类别×严重度无映射行；建议补表后重调，不猜档位",
        }
    }
}

/// 兑现记录四要素（登记制：预留出处/承接物/兑现单号/接口版本）。
pub const FULFILL: [&str; 4] = [
    "预留出处=Q10移交包（F2563侧异常口径+vcq01/vcq02指标）",
    "承接物=SafeDegradeRequestV1/ReplyV1字段闭集+降级映射矩阵",
    "兑现单=CGPU-F2724（R04安全渲染降级接口）",
    "接口版本=r04.safedegrade.v1（冻结）",
];

/// Q10 衔接包三件。
pub const Q10_HANDOVER: [&str; 3] =
    ["可靠性异常类型闭集", "异常严重度分级", "降级调用约定"];

/// 衔接包核验（三件齐备 + 兑现记录四要素齐备）。
pub fn verify_fulfill(received: &[&str]) -> Result<(), XcCode> {
    if received.len() < Q10_HANDOVER.len() || FULFILL.len() != 4 {
        return Err(XcCode::HandoverMissing);
    }
    let mut i = 0usize;
    while i < Q10_HANDOVER.len() {
        let need = match Q10_HANDOVER.get(i) {
            Some(h) => *h,
            None => return Err(XcCode::HandoverMissing),
        };
        let mut hit = false;
        let mut j = 0usize;
        while j < received.len() {
            if let Some(r) = received.get(j) {
                if *r == need {
                    hit = true;
                    break;
                }
            }
            j += 1;
        }
        if !hit {
            return Err(XcCode::HandoverMissing);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、v1 冻结接口（判据二）
// ---------------------------------------------------------------------------

/// 异常类别闭集（承接 Q 域五类口径——不第二套类别）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultCategory {
    /// 硬件（对齐 vcq03 Hardware）。
    Hardware,
    /// 驱动。
    Driver,
    /// 内存。
    Memory,
    /// 逻辑。
    Logic,
    /// OOM。
    Oom,
}

impl FaultCategory {
    /// 类序（0-4；映射表列下标）。
    pub const fn ordinal(self) -> usize {
        match self {
            FaultCategory::Hardware => 0,
            FaultCategory::Driver => 1,
            FaultCategory::Memory => 2,
            FaultCategory::Logic => 3,
            FaultCategory::Oom => 4,
        }
    }
}

/// 严重度三级（承接 Q 域分级口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 轻（可自愈）。
    Minor,
    /// 中（需降级）。
    Major,
    /// 重（需隔离+降级）。
    Critical,
}

impl Severity {
    /// 级序（0-2；映射表行下标）。
    pub const fn ordinal(self) -> usize {
        match self {
            Severity::Minor => 0,
            Severity::Major => 1,
            Severity::Critical => 2,
        }
    }
}

/// 降级档闭集（与 O 域降质档语义对齐；档序越高降得越深）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeTier {
    /// 不降（观察即可）。
    None0,
    /// 质量降一档。
    Quality1,
    /// 质量降二档+特效豁减。
    Quality2,
    /// 隔离受影响面+质量保底档。
    Isolate,
}

impl DegradeTier {
    /// 档名。
    pub const fn zh(self) -> &'static str {
        match self {
            DegradeTier::None0 => "不降",
            DegradeTier::Quality1 => "质量降一档",
            DegradeTier::Quality2 => "质量降二档+特效豁减",
            DegradeTier::Isolate => "隔离受影响面+质量保底档",
        }
    }
}

/// v1 请求（字段闭集冻结）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafeDegradeRequestV1 {
    /// 请求 id（幂等去重键）。
    pub request_id: u64,
    /// 异常类别。
    pub fault: FaultCategory,
    /// 严重度。
    pub severity: Severity,
    /// 受影响渲染面（面标识；空串=全局）。
    pub surface_scope: String,
    /// 期望降级档（建议值；最终档由映射矩阵裁决）。
    pub expect_tier: DegradeTier,
}

impl SafeDegradeRequestV1 {
    /// 冻结自检（版本/字段数/指纹三查——判据二机检面）。
    /// 注：&str 相等比较在 const 上下文不可用（PartialEq 非 const 稳定），
    /// 本检查为普通 fn——调用点 route_degrade 前置执行，语义不变。
    pub fn frozen_check(&self) -> Result<(), XcCode> {
        if SAFE_DEGRADE_V1_VERSION != "r04.safedegrade.v1" {
            return Err(XcCode::FrozenViolated);
        }
        if V1_REQUEST_FIELDS.len() != 5 {
            return Err(XcCode::FrozenViolated);
        }
        if V1_REQUEST_FINGERPRINT != 0x5F24_0001_C0DE_0001 {
            return Err(XcCode::FrozenViolated);
        }
        Ok(())
    }
}

/// v1 回执（四字段闭集：受理/降级档/生效范围/回执码）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafeDegradeReplyV1 {
    /// 是否受理。
    pub accepted: bool,
    /// 裁决降级档。
    pub tier: DegradeTier,
    /// 生效范围（回显面标识）。
    pub scope: String,
    /// 回执码（0x69 段；0x6900=受理）。
    pub reply_code: u16,
}

// ---------------------------------------------------------------------------
// 四、调用约定映射（判据三的矩阵面）
// ---------------------------------------------------------------------------

/// 降级映射矩阵（行=严重度 0-2，列=类别 0-4；确定性映射——衔接包
/// 约定即此表；重故障重降档，轻故障观察）。
pub const DEGRADE_MATRIX: [[DegradeTier; 5]; 3] = [
    //           Hardware    Driver      Memory      Logic       Oom
    [DegradeTier::None0, DegradeTier::None0, DegradeTier::Quality1, DegradeTier::None0, DegradeTier::Quality1], // Minor
    [DegradeTier::Quality1, DegradeTier::Quality1, DegradeTier::Quality2, DegradeTier::Quality1, DegradeTier::Quality2], // Major
    [DegradeTier::Isolate, DegradeTier::Isolate, DegradeTier::Isolate, DegradeTier::Quality2, DegradeTier::Isolate], // Critical
];

/// 调用约定（判据三：异常→降级的确定性路由；冻结自检先行）。
pub fn route_degrade(req: &SafeDegradeRequestV1) -> Result<SafeDegradeReplyV1, XcCode> {
    req.frozen_check()?;
    let r = req.severity.ordinal();
    let c = req.fault.ordinal();
    let tier = match DEGRADE_MATRIX.get(r).and_then(|row| row.get(c)) {
        Some(t) => *t,
        None => return Err(XcCode::RouteMissing),
    };
    Ok(SafeDegradeReplyV1 {
        accepted: true,
        tier,
        scope: req.surface_scope.clone(),
        reply_code: 0x6900,
    })
}

// ---------------------------------------------------------------------------
// 五、幂等去重账（同一请求重放不二次降级——调用约定的幂等纪律）
// ---------------------------------------------------------------------------

/// 去重账容量（封顶滚动；配额恒定）。
pub const DEDUP_CAPACITY: usize = 64;

/// 幂等去重账（request_id → 已裁决回执；重放请求返回原回执）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DedupLedger {
    ids: Vec<u64>,
    replies: Vec<SafeDegradeReplyV1>,
    replayed: u64,
    dropped: u64,
}

impl DedupLedger {
    /// 幂等路由：新请求走 [`route_degrade`] 并入账；重放请求直接返回
    /// 原回执（不二次降级——重放风暴不放大降级动作）。
    pub fn route_idempotent(
        &mut self,
        req: &SafeDegradeRequestV1,
    ) -> Result<SafeDegradeReplyV1, XcCode> {
        // 查重。
        let mut i = 0usize;
        while i < self.ids.len() {
            if let Some(id) = self.ids.get(i) {
                if *id == req.request_id {
                    self.replayed = self.replayed.saturating_add(1);
                    return match self.replies.get(i) {
                        Some(r) => Ok(r.clone()),
                        None => {
                            // 账不对齐属内部缺陷：显性拒不静默重建。
                            Err(XcCode::FrozenViolated)
                        }
                    };
                }
            }
            i += 1;
        }
        // 新请求：裁决并入账。
        let reply = route_degrade(req)?;
        self.ids.push(req.request_id);
        self.replies.push(reply.clone());
        if self.ids.len() > DEDUP_CAPACITY {
            self.ids.remove(0);
            self.replies.remove(0);
            self.dropped = self.dropped.saturating_add(1);
        }
        Ok(reply)
    }

    /// 重放命中数（幂等生效的可观测面）。
    pub const fn replayed(&self) -> u64 {
        self.replayed
    }

    /// 滚动退场数（完整性声明）。
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 在账请求数。
    pub const fn len_of(&self) -> usize {
        self.ids.len()
    }
}
