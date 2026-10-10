//! VE-F5203 · 粒子特效子系统（VE-Z 域 · 任务段 · 锚点目标 300 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5203`
//!
//! **判据（锚点原文）**：点与力、接入与编排、引擎分工、预算绑定、判据。
//!
//! **职责定位（锚点原文）**：粒子特效子系统——粒子子域定位：点与力
//! 的艺术——烟雾火花雨雪光尘的基础发生器；与 Z02 粒子库分工声明：
//! 本条管**场景接入与编排约定**、Z02 管粒子引擎本体——不重叠不缺位；
//! 粒子特效与预算口径绑定声明。
//!
//! **错误路径与降级矩阵（锚点原文）**：越界实现（子域私写引擎）→
//! 归位拦截；预算失守→降级联动；失配→对拍。
//!
//! **性能逐项分解（锚点原文）**：定位 O(1)；拦截 O(1)；对拍 O(1)。
//!
//! **跨批对接点（锚点原文）**：F5076 引擎口径上游；Z02 粒子库下游；
//! F5220 双签闸。
//!
//! # 深化说明（锚点四句矩阵逐句落成可机检机制）
//!
//! ## 深化一：分工声明不是注释，是**管辖面定表**（不重叠不缺位）
//!
//! 「本条管接入与编排、Z02 管引擎本体」若只写在注释里，三个月后
//! 第一个图省事的人就会在编排面里私写一个 mini 求解器——越界从
//! 那一刻起不可逆。故 [`DomainCard`] 把两侧管辖面落成**可机检定表**：
//! 本条侧职责项（接入挂载/编排约定/预算申报）与 Z02 侧职责项
//! （发射器/求解器/缓冲管理）逐项编号，共享接口点单列；判定函数
//! [`DomainCard::overlap_free`] 机械证明两表**无交集**，判定函数
//! [`DomainCard::gap_free`] 机械证明引擎职责七项**每一项都有归属**
//! ——「不重叠」与「不缺位」从口号变成两条可跑的断言。
//!
//! ## 深化二：越界实现 → 归位拦截（拦截 O(1)——越界词表查表）
//!
//! 接入面 [`SceneAttach`] 只暴露**编排参数**（挂载点/节拍/预算申报）。
//! 若编排请求携带引擎本体 API 意图（直写发射缓冲/私设求解器步长/
//! 绕过预算闸直发），[`BoundaryGuard::check`] 按**越界词表**逐词
//! O(1) 拦截，返回 [`BoundaryError`] 并给**归位指引**（「该能力属
//! Z02 发射器面，经编排申报后由引擎代执行」）——不是静默忽略
//! （静默=越界成功），也不是报错了事（报错无指引=下一个越界者
//! 原样再犯）。
//!
//! ## 深化三：六族基础发生器（点与力的艺术——参数面完整可机检）
//!
//! 烟雾/火花/雨/雪/光/尘六族，每族一份 [`EmitterPreset`]：发射率、
//! 初速域、重力系数、阻力系数、寿命域、透明度曲线族、预算权重
//! 七要素。「点与力」的语义在此显性：**点**=出生位置域（跟随点/
//! 世界盒/屏幕带），**力**=寿命期受力（重力+阻力+风场系数）。
//! 预设只报**意图与参数域**，不触引擎——预设本身即编排约定册的
//! 一节（锚点「数据结构：编排约定册」）。
//!
//! ## 深化四：预算口径绑定 → 三级降级联动（预算失守不硬撑）
//!
//! [`VfxBudget`] 三账：每帧模拟 µs、活动粒子总量、每秒发射数。
//! 申报超账即拒（[`BudgetGate::admit`]——不是事后 kill，是事前
//! 拒入）；在账系统失守（实测超申报 20%）触发 [`Degradation`]
//! 三级联动：一级降发射率（×1/2）、二级降寿命（×1/2，粒子量减半）、
//! 三级停次要系统（预算权重低的先停）——联动次序由**预算权重**
//! 排序决定，不是数组顺序（顺序降级会把「烟停了火还烧」的荒诞
//! 排序固化）。恢复路径对称：预算余量连续 3 拍才逐级回升（同
//! F5404 质量分级「降 1 升 3」迟滞纪律）。
//!
//! ## 深化五：失配 → 对拍（编排参数 × 引擎能力显性对拍）
//!
//! 编排请求的能力位（风场/透明度曲线/世界盒剔除）与引擎申报的
//! 能力位做**位对拍**：请求位 ∩ 能力位 = 请求位 ⇒ 全支持放行；
//! 有差 ⇒ [`CapabilityMismatch`] 逐位列明「请求了什么、引擎没有
//! 什么、编排侧的替代方案」——不静默降级（静默=画面不对但没人
//! 知道为什么）。
//!
//! ## 深化六：三对接点落地（F5076 上游 / Z02 下游 / F5220 双签闸）
//!
//! [`ENGINE_CALIBER_NOTE`]（F5076 口径引用）；[`EngineSurface`]
//! trait（Z02 消费面——预设投放与能力申报的唯一接缝）；
//! [`DualSignGate`]（F5220 双签闸——定位卡与约定册双双签署后
//! 才允许接入面开闸，单签拒绝并留痕）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const PARTICLES_VERSION: &str = "F5203-particles-v2-deepened";

/// 越界实现（子域私写引擎）错误码。
pub const E_VFX_BOUNDARY: &str = "E_VFX_BOUNDARY";

/// 预算失守（拒入/降级联动）错误码。
pub const E_VFX_BUDGET: &str = "E_VFX_BUDGET";

/// 编排×引擎能力失配错误码。
pub const E_VFX_MISMATCH: &str = "E_VFX_MISMATCH";

/// 双签闸未签署错误码。
pub const E_VFX_DUALSIGN: &str = "E_VFX_DUALSIGN";

/// F5076 引擎口径上游声明（口径卡引用注记）。
pub const ENGINE_CALIBER_NOTE: &str =
    "F5203 粒子特效子系统口径承接 F5076 引擎口径上游：发射器/求解器/缓冲管理语义以 F5076 为唯一事实源；本条只管场景接入与编排约定（Z02 粒子库下游消费面见 EngineSurface）；F5220 双签闸管控定位卡与约定册的生效";

/// 引擎侧职责项数（Z02 管辖——缺位检测的期望值）。
pub const ENGINE_DUTY_COUNT: usize = 7;

/// 接入侧职责项数（本条管辖）。
pub const SCENE_DUTY_COUNT: usize = 4;

/// 预算失守判定线（实测超申报比例——千分比）。
pub const BUDGET_BREACH_PERMILLE: u32 = 1_200; // 120% 即失守

/// 一级降级：发射率折减分子/分母（×1/2）。
pub const DEGRADE_L1_NUM: u32 = 1;
pub const DEGRADE_L1_DEN: u32 = 2;

/// 二级降级：寿命折减（×1/2）。
pub const DEGRADE_L2_NUM: u32 = 1;
pub const DEGRADE_L2_DEN: u32 = 2;

/// 逐级回升所需的连续余量拍数（降 1 升 3 迟滞）。
pub const RESTORE_STREAK: u32 = 3;

// ---------------------------------------------------------------------------
// 二、子域定位卡（分工定表——不重叠不缺位可机检）
// ---------------------------------------------------------------------------

/// 管辖职责项（两侧职责的编号闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duty {
    // —— 本条（F5203）管辖：场景接入与编排约定 ——
    /// 场景挂载（粒子系统挂到实体/世界/UI 层）。
    SceneAttach,
    /// 编排约定（节拍/剔除/优先级/预设选择）。
    Orchestration,
    /// 预算申报（三账申报与降级联动）。
    BudgetDeclare,
    /// 越界守卫（接入面词表拦截）。
    BoundaryGuard,
    // —— Z02（粒子库）管辖：引擎本体 ——
    /// 发射器（出生/初速/寿命指派）。
    Emitter,
    /// 求解器（积分/碰撞/力场步进）。
    Solver,
    /// 缓冲管理（粒子池/句柄重用/显存账）。
    BufferMgr,
    /// 渲染代理（点精灵/公告板/特效网格投递）。
    RenderProxy,
    /// 引擎内部调度（发射器生命周期/系统休眠）。
    EngineSched,
    /// 引擎侧确定性（步进序/种子管理）。
    EngineDeterminism,
    /// 引擎侧遥测（引擎内部统计上报）。
    EngineTelemetry,
}

impl Duty {
    /// 全部职责项（11 项=本条 4 + Z02 7）。
    pub const ALL: [Duty; 11] = [
        Duty::SceneAttach,
        Duty::Orchestration,
        Duty::BudgetDeclare,
        Duty::BoundaryGuard,
        Duty::Emitter,
        Duty::Solver,
        Duty::BufferMgr,
        Duty::RenderProxy,
        Duty::EngineSched,
        Duty::EngineDeterminism,
        Duty::EngineTelemetry,
    ];

    /// 是否本条（场景接入面）管辖。
    pub fn is_scene_side(self) -> bool {
        matches!(self, Duty::SceneAttach | Duty::Orchestration | Duty::BudgetDeclare | Duty::BoundaryGuard)
    }

    /// 是否引擎侧（Z02）管辖。
    pub fn is_engine_side(self) -> bool {
        !self.is_scene_side()
    }

    /// 职责项名（读屏可达）。
    pub fn zh(self) -> &'static str {
        match self {
            Duty::SceneAttach => "场景挂载",
            Duty::Orchestration => "编排约定",
            Duty::BudgetDeclare => "预算申报",
            Duty::BoundaryGuard => "越界守卫",
            Duty::Emitter => "发射器",
            Duty::Solver => "求解器",
            Duty::BufferMgr => "缓冲管理",
            Duty::RenderProxy => "渲染代理",
            Duty::EngineSched => "引擎调度",
            Duty::EngineDeterminism => "引擎确定性",
            Duty::EngineTelemetry => "引擎遥测",
        }
    }
}

/// 子域定位卡（分工定表——锚点「数据结构：子域定位卡」）。
///
/// 「不重叠不缺位」的两条机械证明：
/// · [`DomainCard::overlap_free`]：两表无交集（每个职责项恰好一侧）。
/// · [`DomainCard::gap_free`]：引擎职责恰 [`ENGINE_DUTY_COUNT`] 项、
///   接入职责恰 [`SCENE_DUTY_COUNT`] 项——任一侧写漏一项即刻红。
#[derive(Clone, Copy, Debug)]
pub struct DomainCard {
    /// 引擎侧（Z02）管辖项。
    pub engine_duties: [Option<Duty>; ENGINE_DUTY_COUNT],
    /// 接入侧（本条）管辖项。
    pub scene_duties: [Option<Duty>; SCENE_DUTY_COUNT],
}

impl DomainCard {
    /// 标准定位卡（锚点分工声明逐项落位）。
    pub fn standard() -> DomainCard {
        DomainCard {
            engine_duties: [
                Some(Duty::Emitter),
                Some(Duty::Solver),
                Some(Duty::BufferMgr),
                Some(Duty::RenderProxy),
                Some(Duty::EngineSched),
                Some(Duty::EngineDeterminism),
                Some(Duty::EngineTelemetry),
            ],
            scene_duties: [
                Some(Duty::SceneAttach),
                Some(Duty::Orchestration),
                Some(Duty::BudgetDeclare),
                Some(Duty::BoundaryGuard),
            ],
        }
    }

    /// 不重叠判定：任一职责项不得同时出现在两侧。
    pub fn overlap_free(&self) -> bool {
        for e in self.engine_duties.iter().flatten() {
            for s in self.scene_duties.iter().flatten() {
                if e == s {
                    return false;
                }
            }
        }
        true
    }

    /// 不缺位判定：两表行数恰好、且无空槽（少登记=缺位）。
    pub fn gap_free(&self) -> bool {
        self.engine_duties.iter().all(|d| d.is_some()) && self.scene_duties.iter().all(|d| d.is_some())
    }

    /// 全量守恒：两侧合计覆盖 11 项闭集无重复（三重证明的第三条）。
    pub fn covers_all(&self) -> bool {
        let mut seen = [false; 11];
        for d in self.engine_duties.iter().flatten().chain(self.scene_duties.iter().flatten()) {
            let idx = match d {
                Duty::SceneAttach => 0,
                Duty::Orchestration => 1,
                Duty::BudgetDeclare => 2,
                Duty::BoundaryGuard => 3,
                Duty::Emitter => 4,
                Duty::Solver => 5,
                Duty::BufferMgr => 6,
                Duty::RenderProxy => 7,
                Duty::EngineSched => 8,
                Duty::EngineDeterminism => 9,
                Duty::EngineTelemetry => 10,
            };
            if seen[idx] {
                return false; // 闭集内重复=变形的越界
            }
            seen[idx] = true;
        }
        seen.iter().all(|&s| s)
    }

    /// 归属查询（O(1)——11 项定长闭集查表；锚点「定位 O(1)」）。
    pub fn owner_of(d: Duty) -> &'static str {
        if d.is_scene_side() {
            "F5203 场景接入面"
        } else {
            "Z02 粒子库引擎面"
        }
    }
}

// ---------------------------------------------------------------------------
// 三、越界守卫（越界词表 O(1) 拦截 + 归位指引）
// ---------------------------------------------------------------------------

/// 越界意图（接入面不允许直触的引擎本体操作——词表闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryIntent {
    /// 直写发射缓冲（绕过发射器）。
    WriteEmitterBuffer,
    /// 私设求解器步长（绕过引擎调度）。
    SetSolverStep,
    /// 绕过预算闸直发粒子。
    BypassBudgetGate,
    /// 直取粒子池句柄（绕过缓冲管理）。
    GrabPoolHandle,
    /// 私改引擎种子（破坏确定性管理）。
    MutateEngineSeed,
    /// 直调渲染代理底层（绕过投递约定）。
    CallRenderProxyRaw,
}

impl BoundaryIntent {
    /// 归位指引（越界者需要的不是「不许」，是「那该去哪」）。
    pub fn guidance(self) -> &'static str {
        match self {
            BoundaryIntent::WriteEmitterBuffer => "发射缓冲属 Z02 发射器面：经编排申报发射意图，由引擎代执行写入",
            BoundaryIntent::SetSolverStep => "求解步长属 Z02 引擎调度：编排侧只申报节拍偏好，步长由引擎按确定性序定",
            BoundaryIntent::BypassBudgetGate => "直发绕闸=预算失守根源：一律走 BudgetGate 拒入/降级联动，无例外通道",
            BoundaryIntent::GrabPoolHandle => "粒子池句柄属 Z02 缓冲管理：编排侧只持系统级句柄，粒子级句柄不出引擎",
            BoundaryIntent::MutateEngineSeed => "引擎种子属 Z02 确定性管理：编排侧种子经 EngineSurface 申报注入，不可直改",
            BoundaryIntent::CallRenderProxyRaw => "渲染代理底层属 Z02：编排侧只投递系统级可见性与层级，绘制由引擎代理",
        }
    }

    /// 词表全量（拦截面完整性对账用）。
    pub const ALL: [BoundaryIntent; 6] = [
        BoundaryIntent::WriteEmitterBuffer,
        BoundaryIntent::SetSolverStep,
        BoundaryIntent::BypassBudgetGate,
        BoundaryIntent::GrabPoolHandle,
        BoundaryIntent::MutateEngineSeed,
        BoundaryIntent::CallRenderProxyRaw,
    ];
}

/// 越界守卫（拦截计数+归位指引产出——拦截 O(1)：词表 6 项定长）。
#[derive(Clone, Copy, Debug, Default)]
pub struct BoundaryGuard {
    /// 各类越界拦截计数（读屏可达——越界热力面）。
    counts: [u32; 6],
}

impl BoundaryGuard {
    /// 越界检查：命中词表 → 拦截+指引；未命中 → 放行。
    pub fn check(&mut self, intent: Option<BoundaryIntent>) -> Result<(), String> {
        match intent {
            None => Ok(()),
            Some(b) => {
                let idx = match b {
                    BoundaryIntent::WriteEmitterBuffer => 0,
                    BoundaryIntent::SetSolverStep => 1,
                    BoundaryIntent::BypassBudgetGate => 2,
                    BoundaryIntent::GrabPoolHandle => 3,
                    BoundaryIntent::MutateEngineSeed => 4,
                    BoundaryIntent::CallRenderProxyRaw => 5,
                };
                self.counts[idx] = self.counts[idx].saturating_add(1);
                Err(format!(
                    "{}：越界实现被拦截（{:?}）——归位：{}（本条只管场景接入与编排约定）",
                    E_VFX_BOUNDARY,
                    b,
                    b.guidance()
                ))
            }
        }
    }

    /// 某类越界的拦截计数（读屏可达）。
    pub fn count_of(&self, b: BoundaryIntent) -> u32 {
        let idx = match b {
            BoundaryIntent::WriteEmitterBuffer => 0,
            BoundaryIntent::SetSolverStep => 1,
            BoundaryIntent::BypassBudgetGate => 2,
            BoundaryIntent::GrabPoolHandle => 3,
            BoundaryIntent::MutateEngineSeed => 4,
            BoundaryIntent::CallRenderProxyRaw => 5,
        };
        self.counts[idx]
    }

    /// 总拦截数。
    pub fn total(&self) -> u32 {
        self.counts.iter().copied().fold(0u32, u32::saturating_add)
    }
}

// ---------------------------------------------------------------------------
// 四、六族基础发生器预设（点与力的艺术——参数面）
// ---------------------------------------------------------------------------

/// 出生位置域（「点」的语义面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnDomain {
    /// 跟随点（附着实体锚点，烟/光常用）。
    FollowPoint,
    /// 世界盒（盒域随机，雪/尘常用）。
    WorldBox,
    /// 屏幕带（屏幕空间带域，雨常用）。
    ScreenBand,
    /// 事件点（一次性爆发点，火花常用）。
    BurstPoint,
}

/// 受力构成（「力」的语义面——重力+阻力+风场三系数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForceSpec {
    /// 重力系数（×1000 定点：1000=标准重力，负=上浮）。
    pub gravity_milli: i32,
    /// 阻力系数（×1000 定点：0=无阻力，1000=强阻力）。
    pub drag_milli: u32,
    /// 风场响应系数（×1000 定点：0=不受风）。
    pub wind_milli: u32,
}

/// 透明度曲线族（渲染约定——编排侧只选族不写曲线本体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaCurve {
    /// 线性淡出（烟/尘）。
    FadeOut,
    /// 快进慢出（火花）。
    FastInSlowOut,
    /// 恒定（雨/雪以运动表达）。
    Constant,
    /// 慢进快出（光尘）。
    SlowInFastOut,
}

/// 发生器预设（六族各一份——锚点「烟雾火花雨雪光尘的基础发生器」）。
#[derive(Clone, Copy, Debug)]
pub struct EmitterPreset {
    /// 预设名（读屏可达）。
    pub name: &'static str,
    /// 发射率（粒/秒）。
    pub emit_per_sec: u32,
    /// 初速域（cm/s 下界,上界）。
    pub speed_cm_s: (u32, u32),
    /// 受力构成。
    pub force: ForceSpec,
    /// 寿命域（ms 下界,上界）。
    pub life_ms: (u32, u32),
    /// 透明度曲线族。
    pub alpha: AlphaCurve,
    /// 出生位置域。
    pub spawn: SpawnDomain,
    /// 预算权重（0-100：降级时权重低者先停——次序语义）。
    pub budget_weight: u32,
}

/// 六族预设定表（判据逐族对账的独立预期源）。
pub const PRESETS: [EmitterPreset; 6] = [
    EmitterPreset {
        name: "烟雾",
        emit_per_sec: 40,
        speed_cm_s: (5, 30),
        force: ForceSpec { gravity_milli: -200, drag_milli: 600, wind_milli: 500 },
        life_ms: (1_500, 3_500),
        alpha: AlphaCurve::FadeOut,
        spawn: SpawnDomain::FollowPoint,
        budget_weight: 30,
    },
    EmitterPreset {
        name: "火花",
        emit_per_sec: 120,
        speed_cm_s: (150, 600),
        force: ForceSpec { gravity_milli: 900, drag_milli: 200, wind_milli: 100 },
        life_ms: (300, 900),
        alpha: AlphaCurve::FastInSlowOut,
        spawn: SpawnDomain::BurstPoint,
        budget_weight: 70,
    },
    EmitterPreset {
        name: "雨",
        emit_per_sec: 400,
        speed_cm_s: (900, 1_200),
        force: ForceSpec { gravity_milli: 1_000, drag_milli: 50, wind_milli: 300 },
        life_ms: (600, 900),
        alpha: AlphaCurve::Constant,
        spawn: SpawnDomain::ScreenBand,
        budget_weight: 80,
    },
    EmitterPreset {
        name: "雪",
        emit_per_sec: 150,
        speed_cm_s: (20, 60),
        force: ForceSpec { gravity_milli: 150, drag_milli: 800, wind_milli: 700 },
        life_ms: (4_000, 9_000),
        alpha: AlphaCurve::Constant,
        spawn: SpawnDomain::WorldBox,
        budget_weight: 50,
    },
    EmitterPreset {
        name: "光尘",
        emit_per_sec: 25,
        speed_cm_s: (1, 8),
        force: ForceSpec { gravity_milli: -50, drag_milli: 900, wind_milli: 200 },
        life_ms: (2_000, 5_000),
        alpha: AlphaCurve::SlowInFastOut,
        spawn: SpawnDomain::WorldBox,
        budget_weight: 20,
    },
    EmitterPreset {
        name: "尘",
        emit_per_sec: 30,
        speed_cm_s: (2, 12),
        force: ForceSpec { gravity_milli: 80, drag_milli: 850, wind_milli: 400 },
        life_ms: (1_000, 2_500),
        alpha: AlphaCurve::FadeOut,
        spawn: SpawnDomain::WorldBox,
        budget_weight: 10,
    },
];

/// 按名取预设（O(1)——6 项定长查表）。
pub fn preset_of(name: &str) -> Option<&'static EmitterPreset> {
    PRESETS.iter().find(|p| p.name == name)
}

// ---------------------------------------------------------------------------
// 五、编排约定册（场景接入的编排面）
// ---------------------------------------------------------------------------

/// 挂载点（粒子系统挂在哪）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachPoint {
    /// 跟随实体（实体移动粒子跟随）。
    FollowEntity,
    /// 世界锚定（固定世界坐标）。
    WorldAnchored,
    /// 屏幕层（UI 特效——不进世界剔除）。
    ScreenLayer,
}

/// 更新节拍约定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateTempo {
    /// 每帧（近景/主角特效）。
    EveryFrame,
    /// 每 N 帧（远景降噪——N 由编排申报，引擎可下调不可上调）。
    EveryNFrames(u32),
    /// 事件驱动（只在发射事件时步进——爆发型）。
    OnEmit,
}

/// 编排约定（一个粒子系统在场景里的接入声明）。
#[derive(Clone, Copy, Debug)]
pub struct Orchestration {
    /// 系统名（读屏可达）。
    pub system: &'static str,
    /// 挂载点。
    pub attach: AttachPoint,
    /// 更新节拍。
    pub tempo: UpdateTempo,
    /// 可见性剔除距离（cm；0=不剔除）。
    pub cull_dist_cm: u32,
    /// 使用的预设族名（六族之一）。
    pub preset: &'static str,
    /// 预算申报（三账）。
    pub declared: VfxBudget,
}

impl Orchestration {
    /// 节拍规约（引擎可下调不可上调——远景降噪由编排发起）。
    pub fn effective_tempo_frames(&self) -> u32 {
        match self.tempo {
            UpdateTempo::EveryFrame => 1,
            UpdateTempo::EveryNFrames(n) => n.max(1),
            UpdateTempo::OnEmit => 0, // 事件驱动：帧步进为 0
        }
    }
}

/// 粒子特效预算（三账——锚点「粒子特效与预算口径绑定声明」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VfxBudget {
    /// 每帧模拟预算（µs）。
    pub sim_us_per_frame: u32,
    /// 活动粒子总量上限。
    pub max_alive: u32,
    /// 每秒发射数上限。
    pub emit_per_sec_cap: u32,
}

/// 预算账本（全场景三账合计——申报超账即拒）。
#[derive(Clone, Copy, Debug)]
pub struct BudgetLedger {
    /// 场景总账。
    pub scene_cap: VfxBudget,
    /// 已申报合计。
    pub declared_sum: VfxBudget,
    /// 拒入计数（读屏可达）。
    pub rejects: u32,
}

impl BudgetLedger {
    /// 新账本。
    pub fn new(scene_cap: VfxBudget) -> BudgetLedger {
        BudgetLedger { scene_cap, declared_sum: VfxBudget { sim_us_per_frame: 0, max_alive: 0, emit_per_sec_cap: 0 }, rejects: 0 }
    }

    /// 申报（逐账超限即拒——不是事后 kill 是事前拒入）。
    pub fn declare(&mut self, b: VfxBudget) -> Result<(), String> {
        let sim_ok = self.declared_sum.sim_us_per_frame.saturating_add(b.sim_us_per_frame) <= self.scene_cap.sim_us_per_frame;
        let alive_ok = self.declared_sum.max_alive.saturating_add(b.max_alive) <= self.scene_cap.max_alive;
        let emit_ok = self.declared_sum.emit_per_sec_cap.saturating_add(b.emit_per_sec_cap) <= self.scene_cap.emit_per_sec_cap;
        if sim_ok && alive_ok && emit_ok {
            self.declared_sum.sim_us_per_frame += b.sim_us_per_frame;
            self.declared_sum.max_alive += b.max_alive;
            self.declared_sum.emit_per_sec_cap += b.emit_per_sec_cap;
            Ok(())
        } else {
            self.rejects = self.rejects.saturating_add(1);
            Err(format!(
                "{}：预算申报超场景总账（sim {}/{} µs, alive {}/{}, emit {}/s）——拒入。\
                 指引：降申报值或先释放低预算权重系统",
                E_VFX_BUDGET,
                self.declared_sum.sim_us_per_frame + b.sim_us_per_frame,
                self.scene_cap.sim_us_per_frame,
                self.declared_sum.max_alive + b.max_alive,
                self.scene_cap.max_alive,
                self.declared_sum.emit_per_sec_cap + b.emit_per_sec_cap
            ))
        }
    }

    /// 已用比例（sim 账——千分比）。
    pub fn sim_used_permille(&self) -> u32 {
        if self.scene_cap.sim_us_per_frame == 0 {
            return 0;
        }
        (self.declared_sum.sim_us_per_frame as u64 * 1_000 / self.scene_cap.sim_us_per_frame as u64).min(u32::MAX as u64) as u32
    }
}

// ---------------------------------------------------------------------------
// 六、降级联动（预算失守 → 三级联动 + 对称回升迟滞）
// ---------------------------------------------------------------------------

/// 降级等级（0=全速，1=降发射率，2=降寿命，3=停次要）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Degradation(pub u8);

/// 降级联动器（实测超账触发升档；余量连续 RESTORE_STREAK 拍才降档）。
#[derive(Clone, Copy, Debug)]
pub struct DegradationEngine {
    /// 当前等级。
    pub level: Degradation,
    /// 连续余量拍数（回升迟滞计数）。
    pub headroom_streak: u32,
    /// 升降档总次数（读屏可达）。
    pub changes: u32,
}

impl DegradationEngine {
    /// 新联动器（全速起步）。
    pub fn new() -> DegradationEngine {
        DegradationEngine { level: Degradation(0), headroom_streak: 0, changes: 0 }
    }

    /// 喂一拍实测（used_permille：实测/申报 千分比）。
    /// 失守（≥BUDGET_BREACH_PERMILLE）→ 升档（宁早勿晚）；
    /// 余量（<1_000）→ 连续 3 拍才降档（恢复要证实）。
    pub fn observe(&mut self, used_permille: u32) -> (Degradation, bool) {
        if used_permille >= BUDGET_BREACH_PERMILLE {
            if self.level.0 < 3 {
                self.level = Degradation(self.level.0 + 1);
                self.changes = self.changes.saturating_add(1);
            }
            self.headroom_streak = 0;
            (self.level, true)
        } else if used_permille < 1_000 {
            self.headroom_streak = self.headroom_streak.saturating_add(1);
            if self.headroom_streak >= RESTORE_STREAK && self.level.0 > 0 {
                self.level = Degradation(self.level.0 - 1);
                self.headroom_streak = 0;
                self.changes = self.changes.saturating_add(1);
                (self.level, true)
            } else {
                (self.level, false)
            }
        } else {
            self.headroom_streak = 0;
            (self.level, false)
        }
    }

    /// 当前档位对发射率的折减（分子/分母——无浮点）。
    pub fn emit_scale(&self) -> (u32, u32) {
        match self.level.0 {
            0 => (1, 1),
            _ => (DEGRADE_L1_NUM, DEGRADE_L1_DEN),
        }
    }

    /// 当前档位对寿命的折减。
    pub fn life_scale(&self) -> (u32, u32) {
        match self.level.0 {
            0 | 1 => (1, 1),
            _ => (DEGRADE_L2_NUM, DEGRADE_L2_DEN),
        }
    }

    /// 当前档位是否停次要系统（预算权重 < 40 的停）。
    pub fn minor_suspended(&self, budget_weight: u32) -> bool {
        self.level.0 >= 3 && budget_weight < 40
    }
}

/// 按预算权重排序的停用序（联动次序由权重定——不是数组顺序）。
pub fn suspension_order(systems: &[( &'static str, u32 )]) -> Vec<&'static str> {
    let mut v: Vec<(&'static str, u32)> = systems.iter().map(|(n, w)| (*n, *w)).collect();
    // 稳定升序：权重小者先停。
    v.sort_by_key(|&(_, w)| w);
    v.into_iter().map(|(n, _)| n).collect()
}

// ---------------------------------------------------------------------------
// 七、能力对拍（编排请求 × 引擎能力——失配显性）
// ---------------------------------------------------------------------------

/// 编排能力位（bit0=风场 bit1=透明度曲线 bit2=世界盒剔除 bit3=事件爆发）。
pub const CAP_WIND: u32 = 1 << 0;
/// 透明度曲线能力位。
pub const CAP_ALPHA: u32 = 1 << 1;
/// 世界盒剔除能力位。
pub const CAP_CULLBOX: u32 = 1 << 2;
/// 事件爆发能力位。
pub const CAP_BURST: u32 = 1 << 3;

/// 能力失配报告（逐位列明：请求了什么、缺什么、替代方案）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityMismatch {
    /// 失配位名（读屏可达）。
    pub cap_name: &'static str,
    /// 编排侧替代方案。
    pub fallback: &'static str,
}

/// 能力对拍（O(1)——4 位定长；锚点「失配→对拍」「对拍 O(1)」）。
pub fn capability_check(requested: u32, engine_caps: u32) -> Result<(), Vec<CapabilityMismatch>> {
    let mut bad = Vec::new();
    let caps: [(u32, &str, &str); 4] = [
        (CAP_WIND, "风场", "改用阻力系数近似风效果"),
        (CAP_ALPHA, "透明度曲线", "降级为引擎缺省曲线（FadeOut）"),
        (CAP_CULLBOX, "世界盒剔除", "改用距离剔除（cull_dist_cm）"),
        (CAP_BURST, "事件爆发", "改用高频短寿命连续发射近似"),
    ];
    for (bit, name, fallback) in caps.iter() {
        if requested & bit != 0 && engine_caps & bit == 0 {
            bad.push(CapabilityMismatch { cap_name: name, fallback });
        }
    }
    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad)
    }
}

/// 失配报告串（一行读屏可达）。
pub fn mismatch_report(ms: &[CapabilityMismatch]) -> String {
    let mut s = format!("{}：编排×引擎能力失配 {} 项——", E_VFX_MISMATCH, ms.len());
    for (i, m) in ms.iter().enumerate() {
        if i > 0 {
            s.push_str("；");
        }
        s.push_str(&format!("{} 缺失，替代：{}", m.cap_name, m.fallback));
    }
    s
}

// ---------------------------------------------------------------------------
// 八、Z02 引擎消费面（EngineSurface trait——下游唯一接缝）
// ---------------------------------------------------------------------------

/// Z02 引擎消费面（锚点「Z02 粒子库下游」——编排侧向引擎投放的唯一
/// 接缝；引擎本体实现此 trait，编排侧只依赖此抽象）。
pub trait EngineSurface {
    /// 投放预设（引擎按预设参数域自行实例化发射器）。
    fn deploy_preset(&mut self, preset: &'static str) -> Result<(), String>;
    /// 申报引擎能力位（对拍数据源）。
    fn engine_caps(&self) -> u32;
    /// 引擎实测负载（千分比——降级联动数据源）。
    fn load_permille(&self) -> u32;
}

/// 记录式引擎桩（判据与调试用——Z02 真实现替换之）。
#[derive(Clone, Debug, Default)]
pub struct StubEngine {
    /// 已投放预设序列。
    pub deployed: Vec<&'static str>,
    /// 能力位。
    pub caps: u32,
    /// 实测负载（千分比）。
    pub load: u32,
}

impl EngineSurface for StubEngine {
    fn deploy_preset(&mut self, preset: &'static str) -> Result<(), String> {
        self.deployed.push(preset);
        Ok(())
    }
    fn engine_caps(&self) -> u32 {
        self.caps
    }
    fn load_permille(&self) -> u32 {
        self.load
    }
}

// ---------------------------------------------------------------------------
// 九、F5220 双签闸（定位卡与约定册双签生效）
// ---------------------------------------------------------------------------

/// 双签闸（锚点「F5220 双签闸」——定位卡与约定册双双签署后接入面
/// 才开闸；单签拒绝并留痕——半生效比不生效更危险）。
#[derive(Clone, Copy, Debug)]
pub struct DualSignGate {
    /// 定位卡签署。
    pub card_signed: bool,
    /// 约定册签署。
    pub book_signed: bool,
    /// 拒绝计数（单签期内的接入尝试——留痕）。
    pub denied: u32,
}

impl DualSignGate {
    /// 新闸（双未签）。
    pub fn new() -> DualSignGate {
        DualSignGate { card_signed: false, book_signed: false, denied: 0 }
    }

    /// 签署定位卡。
    pub fn sign_card(&mut self) {
        self.card_signed = true;
    }

    /// 签署约定册。
    pub fn sign_book(&mut self) {
        self.book_signed = true;
    }

    /// 接入开闸判定（双签才过；单签拒绝留痕）。
    pub fn admit(&mut self) -> Result<(), String> {
        if self.card_signed && self.book_signed {
            Ok(())
        } else {
            self.denied = self.denied.saturating_add(1);
            Err(format!(
                "{}：双签闸未齐（定位卡={} 约定册={}）——接入面不开闸。\
                 指引：两册分别签署后生效，半生效状态被拒绝是设计行为",
                E_VFX_DUALSIGN, self.card_signed, self.book_signed
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// 十、场景接入门面（编排全链编排）
// ---------------------------------------------------------------------------

/// 场景接入器（定位卡 × 越界守卫 × 预算账本 × 降级联动 × 能力对拍
/// × 双签闸的全链编排——本条「场景接入与编排约定」的运行时面）。
#[derive(Clone, Debug)]
pub struct SceneFacade {
    pub card: DomainCard,
    pub guard: BoundaryGuard,
    pub ledger: BudgetLedger,
    pub degrade: DegradationEngine,
    pub gate: DualSignGate,
    /// 已接入系统名（对账面）。
    pub attached: Vec<&'static str>,
}

impl SceneFacade {
    /// 新接入器（标准定位卡+场景总账）。
    pub fn new(scene_cap: VfxBudget) -> SceneFacade {
        SceneFacade {
            card: DomainCard::standard(),
            guard: BoundaryGuard::default(),
            ledger: BudgetLedger::new(scene_cap),
            degrade: DegradationEngine::new(),
            gate: DualSignGate::new(),
            attached: Vec::new(),
        }
    }

    /// 接入一个粒子系统（全链：双签闸 → 越界检查 → 预算申报）。
    pub fn attach(
        &mut self,
        orch: &Orchestration,
        intent: Option<BoundaryIntent>,
        engine: &mut dyn EngineSurface,
    ) -> Result<(), String> {
        // 1. 双签闸（未双签不开闸）。
        self.gate.admit()?;
        // 2. 越界拦截（词表 O(1)）。
        self.guard.check(intent)?;
        // 3. 预算申报（超账拒入）。
        self.ledger.declare(orch.declared)?;
        // 4. 能力对拍（预设意图 vs 引擎能力）。
        let want = match orch.preset {
            "烟雾" | "尘" | "光尘" => 0,
            "雪" => CAP_WIND | CAP_CULLBOX,
            "雨" => CAP_CULLBOX,
            "火花" => CAP_BURST | CAP_ALPHA,
            _ => 0,
        };
        if let Err(ms) = capability_check(want, engine.engine_caps()) {
            return Err(mismatch_report(&ms));
        }
        // 5. 引擎投放（Z02 下游消费）。
        engine.deploy_preset(orch.preset)?;
        self.attached.push(orch.system);
        Ok(())
    }

    /// 周期巡检（喂引擎实测负载 → 降级联动）。
    pub fn tick(&mut self, engine: &dyn EngineSurface) -> (Degradation, bool) {
        self.degrade.observe(engine.load_permille())
    }

    /// 当前发射率折减（判据/读屏可达）。
    pub fn emit_scale(&self) -> (u32, u32) {
        self.degrade.emit_scale()
    }

    /// 停用序（降级三级时按预算权重定序）。
    pub fn suspension_order(&self) -> Vec<&'static str> {
        let pairs: Vec<(&'static str, u32)> = PRESETS
            .iter()
            .filter(|p| self.attached.iter().any(|n| *n == p.name))
            .map(|p| (p.name, p.budget_weight))
            .collect();
        suspension_order(&pairs)
    }

    /// 接入摘要（一行读屏可达）。
    pub fn summary(&self) -> String {
        format!(
            "粒子接入：系统={}个 预算sim={}/{}µs({}‰) 降级=L{} 拦截={}次 拒入={}次",
            self.attached.len(),
            self.ledger.declared_sum.sim_us_per_frame,
            self.ledger.scene_cap.sim_us_per_frame,
            self.ledger.sim_used_permille(),
            self.degrade.level.0,
            self.guard.total(),
            self.ledger.rejects,
        )
    }
}

// ---------------------------------------------------------------------------
// 十一、判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F5203 域自检（判据分六组：分工定表/越界拦截/发生器/预算降级/对拍/对接收尾）。
pub fn run_vez03_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5203");

    // --- 分工定表（判据一：引擎分工——不重叠不缺位）---
    let card = DomainCard::standard();
    s.add(
        "VZ3-分工-01",
        card.overlap_free(),
        "不重叠（11 项两表无交集）",
    );
    s.add(
        "VZ3-分工-02",
        card.gap_free(),
        "不缺位（两表无空槽——任一职责都有归属）",
    );
    s.add(
        "VZ3-分工-03",
        card.covers_all(),
        "全量守恒（11 项闭集全覆盖无重复）",
    );
    // 归属查询 O(1)（引擎侧/接入侧各查一项）。
    s.add(
        "VZ3-分工-04",
        DomainCard::owner_of(Duty::Emitter).contains("Z02")
            && DomainCard::owner_of(Duty::Orchestration).contains("F5203"),
        "归属查询（发射器归 Z02、编排归本条）",
    );

    // --- 越界拦截（判据二：越界实现→归位拦截）---
    let mut guard = BoundaryGuard::default();
    // 合法编排请求放行。
    s.add("VZ3-越界-01", guard.check(None).is_ok(), "合法编排放行（未命中词表）");
    // 直写缓冲拦截+归位指引。
    let r1 = guard.check(Some(BoundaryIntent::WriteEmitterBuffer));
    let r1_ok = r1.as_ref().err().map(|x| x.starts_with(E_VFX_BOUNDARY) && x.contains("发射器")).unwrap_or(false);
    s.add(
        "VZ3-越界-02",
        r1.is_err() && r1_ok && guard.count_of(BoundaryIntent::WriteEmitterBuffer) == 1,
        "直写缓冲拦截+归位（去 Z02 发射器面）",
    );
    // 绕闸直发拦截。
    let r2 = guard.check(Some(BoundaryIntent::BypassBudgetGate));
    s.add(
        "VZ3-越界-03",
        r2.as_ref().err().map(|x| x.contains("BudgetGate")).unwrap_or(false),
        "绕闸直发拦截（一律走预算闸）",
    );
    // 词表全量可裁决（6 项逐项拦截不 panic）。
    let all_intercepted = BoundaryIntent::ALL
        .iter()
        .all(|b| guard.check(Some(*b)).is_err());
    s.add(
        "VZ3-越界-04",
        all_intercepted && guard.total() == 8,
        "词表 6 项全拦截（计数守恒 1+1+6=8）",
    );

    // --- 六族发生器（判据三：点与力）---
    // 六族齐备（烟雾火花雨雪光尘）。
    let six = ["烟雾", "火花", "雨", "雪", "光尘", "尘"];
    let all_present = six.iter().all(|n| preset_of(n).is_some());
    s.add("VZ3-发生-01", all_present && PRESETS.len() == 6, "六族基础发生器齐备");
    // 点面：出生域四型在表中至少各出现一次。
    let mut dom_seen = [false; 4];
    for p in PRESETS.iter() {
        let idx = match p.spawn {
            SpawnDomain::FollowPoint => 0,
            SpawnDomain::WorldBox => 1,
            SpawnDomain::ScreenBand => 2,
            SpawnDomain::BurstPoint => 3,
        };
        dom_seen[idx] = true;
    }
    s.add("VZ3-发生-02", dom_seen.iter().all(|&x| x), "点面四型全覆盖（跟随/世界盒/屏幕带/爆发）");
    // 力面：重力有正有负（上浮烟/下落雨）、阻力有强弱、风场有响应有不响应。
    let has_float = PRESETS.iter().any(|p| p.force.gravity_milli < 0);
    let has_fall = PRESETS.iter().any(|p| p.force.gravity_milli >= 900);
    let has_drag = PRESETS.iter().any(|p| p.force.drag_milli >= 800) && PRESETS.iter().any(|p| p.force.drag_milli <= 100);
    let has_wind = PRESETS.iter().any(|p| p.force.wind_milli >= 500) && PRESETS.iter().any(|p| p.force.wind_milli <= 100);
    s.add(
        "VZ3-发生-03",
        has_float && has_fall && has_drag && has_wind,
        "力面三系数语义覆盖（浮/落/阻强阻弱/风响应与不响应）",
    );
    // 预算权重互异（降级停用序可判定）。
    let mut ws = [0u32; 6];
    for (i, p) in PRESETS.iter().enumerate() {
        ws[i] = p.budget_weight;
    }
    ws.sort();
    let weights_distinct = ws.windows(2).all(|w| w[0] < w[1]);
    s.add(
        "VZ3-发生-04",
        weights_distinct && ws[0] == 10 && ws[5] == 80,
        "预算权重互异（10..80——停用序全序可判定）",
    );

    // --- 预算绑定与降级（判据四：预算绑定）---
    // 申报超账拒入（事前拒非事后 kill）。
    let mut ledger = BudgetLedger::new(VfxBudget { sim_us_per_frame: 1_000, max_alive: 10_000, emit_per_sec_cap: 2_000 });
    let d1 = ledger.declare(VfxBudget { sim_us_per_frame: 600, max_alive: 4_000, emit_per_sec_cap: 800 });
    let d2 = ledger.declare(VfxBudget { sim_us_per_frame: 600, max_alive: 0, emit_per_sec_cap: 0 });
    s.add(
        "VZ3-预算-01",
        d1.is_ok() && d2.is_err() && ledger.rejects == 1,
        "逐账申报（600+600>1000 拒入——事前拒）",
    );
    // sim 用量比例对账（600/1000=600‰）。
    s.add("VZ3-预算-02", ledger.sim_used_permille() == 600, "账面用量比例（600‰）");
    // 降级三级联动：失守升档。
    let mut de = DegradationEngine::new();
    let (l1, ch1) = de.observe(1_500);
    s.add(
        "VZ3-预算-03",
        l1 == Degradation(1) && ch1 && de.emit_scale() == (DEGRADE_L1_NUM, DEGRADE_L1_DEN) && de.life_scale() == (1, 1),
        "一级降级（超 120%→降发射率，寿命不动）",
    );
    let (l2, _) = de.observe(1_400);
    s.add(
        "VZ3-预算-04",
        l2 == Degradation(2) && de.life_scale() == (DEGRADE_L2_NUM, DEGRADE_L2_DEN),
        "二级降级（再失守→降寿命）",
    );
    let (l3, _) = de.observe(1_300);
    s.add(
        "VZ3-预算-05",
        l3 == Degradation(3) && de.minor_suspended(30) && !de.minor_suspended(70),
        "三级降级（停次要——权重<40 停，≥40 不停）",
    );
    // 停用序按权重（尘 10 最先、雨 80 最后）。
    let order = suspension_order(&[("雨", 80), ("尘", 10), ("雪", 50), ("火花", 70)]);
    s.add(
        "VZ3-预算-06",
        order.len() == 4 && order[0] == "尘" && order[3] == "雨",
        "停用序按权重定（尘先雨后——不是数组顺序）",
    );
    // 对称回升迟滞（余量连续 3 拍才降档）。
    let mut de2 = DegradationEngine::new();
    de2.observe(1_500);
    de2.observe(1_500);
    let r1d = de2.observe(500);
    let r2d = de2.observe(500);
    let r3d = de2.observe(500);
    let r4d = de2.observe(500);
    s.add(
        "VZ3-预算-07",
        !r1d.1 && !r2d.1 && r3d.1 && r3d.0 == Degradation(1) && !r4d.1,
        "回升迟滞（连续 3 拍余量才逐级降档）",
    );
    // 回落后再失守：升档路径正常（对称双向联动）。
    de2.observe(2_000);
    s.add("VZ3-预算-08", de2.level == Degradation(2), "回落后再失守（回升后重新升级路径正常 L1→L2）");

    // --- 能力对拍（判据五：失配→对拍）---
    // 全支持放行。
    s.add(
        "VZ3-对拍-01",
        capability_check(CAP_WIND | CAP_ALPHA, CAP_WIND | CAP_ALPHA | CAP_BURST).is_ok(),
        "全支持放行（请求⊆能力）",
    );
    // 失配逐位列明。
    let mm = capability_check(CAP_WIND | CAP_CULLBOX | CAP_BURST, CAP_WIND);
    let mm_ok = match &mm {
        Err(v) => v.len() == 2 && v.iter().any(|m| m.cap_name == "世界盒剔除") && v.iter().any(|m| m.cap_name == "事件爆发"),
        Ok(_) => false,
    };
    s.add("VZ3-对拍-02", mm_ok, "失配逐位列明（2 项缺失各带替代方案）");
    // 失配报告串（一行读屏可达）。
    let rep = match capability_check(CAP_ALPHA, 0) {
        Err(v) => mismatch_report(&v),
        Ok(_) => String::new(),
    };
    s.add(
        "VZ3-对拍-03",
        rep.starts_with(E_VFX_MISMATCH) && rep.contains("透明度曲线") && rep.contains("FadeOut"),
        "失配报告含替代方案（降级为缺省曲线）",
    );

    // --- 双签闸与编排全链（判据六：接入与编排 + 对接收尾）---
    // 单签拒绝留痕。
    let mut gate = DualSignGate::new();
    gate.sign_card();
    let g1 = gate.admit();
    gate.sign_book();
    let g2 = gate.admit();
    s.add(
        "VZ3-双签-01",
        g1.is_err() && g1.as_ref().err().map(|x| x.starts_with(E_VFX_DUALSIGN)).unwrap_or(false) && gate.denied == 1 && g2.is_ok(),
        "双签闸（单签拒绝留痕、双签开闸）",
    );
    // 编排全链（双签→越界→预算→对拍→投放）。
    let mut fac = SceneFacade::new(VfxBudget { sim_us_per_frame: 5_000, max_alive: 50_000, emit_per_sec_cap: 10_000 });
    let mut eng = StubEngine { deployed: Vec::new(), caps: CAP_WIND | CAP_CULLBOX | CAP_ALPHA | CAP_BURST, load: 1_500 };
    fac.gate.sign_card();
    fac.gate.sign_book();
    let orch = Orchestration {
        system: "雪-世界盒",
        attach: AttachPoint::WorldAnchored,
        tempo: UpdateTempo::EveryNFrames(2),
        cull_dist_cm: 3_000,
        preset: "雪",
        declared: VfxBudget { sim_us_per_frame: 1_200, max_alive: 8_000, emit_per_sec_cap: 1_500 },
    };
    let a1 = fac.attach(&orch, None, &mut eng);
    s.add(
        "VZ3-全链-01",
        a1.is_ok() && fac.attached.len() == 1 && eng.deployed.len() == 1 && eng.deployed[0] == "雪",
        "编排全链走通（双签→越界→预算→对拍→投放）",
    );
    // 全链越界拒绝（不带预算痕迹——拦截在预算之前）。
    let a2 = fac.attach(&orch, Some(BoundaryIntent::GrabPoolHandle), &mut eng);
    s.add(
        "VZ3-全链-02",
        a2.as_ref().err().map(|x| x.starts_with(E_VFX_BOUNDARY)).unwrap_or(false) && eng.deployed.len() == 1,
        "全链越界拦截（投放未发生）",
    );
    // 能力失配拒绝（火花需 BURST+ALPHA，引擎收窄后失配）。
    let mut eng2 = StubEngine { deployed: Vec::new(), caps: 0, load: 1_500 };
    let a3 = fac.attach(
        &Orchestration {
            system: "火花-事件",
            attach: AttachPoint::FollowEntity,
            tempo: UpdateTempo::OnEmit,
            cull_dist_cm: 0,
            preset: "火花",
            declared: VfxBudget { sim_us_per_frame: 300, max_alive: 1_000, emit_per_sec_cap: 500 },
        },
        None,
        &mut eng2,
    );
    s.add(
        "VZ3-全链-03",
        a3.as_ref().err().map(|x| x.starts_with(E_VFX_MISMATCH) && x.contains("事件爆发")).unwrap_or(false) && eng2.deployed.is_empty(),
        "能力失配拒绝（不静默降级投放）",
    );
    // 巡检降级联动（喂负载 1500‰ → 一级）。
    let (lv, ch) = fac.tick(&eng);
    s.add(
        "VZ3-全链-04",
        lv == Degradation(1) && ch && fac.emit_scale() == (1, 2),
        "巡检降级联动（实测 1500‰→一级降发射率）",
    );
    // 摘要行（读屏可达）。
    let sum = fac.summary();
    s.add(
        "VZ3-全链-05",
        sum.contains("粒子接入") && sum.contains("降级=L1"),
        "接入摘要一行可读（无障碍解释面）",
    );
    // F5076 上游口径声明在案。
    s.add(
        "VZ3-对接-01",
        ENGINE_CALIBER_NOTE.contains("F5076") && ENGINE_CALIBER_NOTE.contains("Z02") && ENGINE_CALIBER_NOTE.contains("F5220"),
        "三对接点口径声明在案",
    );

    // --- 版本与收尾 ---
    s.add(
        "VZ3-版本-01",
        PARTICLES_VERSION.contains("F5203") && PARTICLES_VERSION.ends_with("deepened"),
        "版本指纹（F5203-particles-v2-deepened）",
    );
    let declared = s.len();
    s.add("VZ3-收尾-01", declared >= 28, "判据条数对账（深化版 ≥28 条）");

    s
}

/// F 域账本暂挂声明（跨批对接点：F5076 引擎口径上游；Z02 粒子库下游；
/// F5220 双签闸——建账前暂挂，移交期模式延续）。
pub const VZ3_LEDGER_SUSPENDED_NOTE: &str =
    "粒子特效子系统（子域定位卡+编排约定册+预算绑定）入 F 域账本：建账前暂挂声明（移交期模式延续）；F5203 与 Z02 分工不重叠不缺位";
