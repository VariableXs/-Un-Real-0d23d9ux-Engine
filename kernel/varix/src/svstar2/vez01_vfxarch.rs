//! VE-F5201 · Z 域开工与特效库总架构（VE-Z 域 · 特效 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5201`
//!
//! **判据（锚点原文）**：四子系统、两翼、特效即内容、层冻结、判据。
//!
//! 1. **四子系统**（判据一）：粒子 / 后处理 / 环境 / 转场。
//!    四者不是四个并列的函数分组，是四条**责任互斥**的产出通道：
//!    每个子系统只允许产出自己那类效果，跨类产出即越界。
//!    「越界」不靠自觉，靠归属判据逐类核对。
//!
//! 2. **两翼**（判据二）：参数系统 + 组合系统。
//!    参数系统解决「同一个特效在不同场景下取不同值」；
//!    组合系统解决「多个特效按什么次序叠加」。两翼**不是两套并行架构**，
//!    而是四子系统唯一的形参来源与唯一的编排入口——
//!    若允许子系统绕过两翼直接取常量，参数系统与组合系统立刻形同虚设。
//!
//! 3. **特效即内容**（判据三，域本色声明）：特效为内容服务，不为炫技服务。
//!    本条落在**可测**的承诺上，不是口号：
//!    - 每个特效必须携带**内容语义声明**（它表达什么），无声明不入册；
//!    - **越权炫技**（无内容语义的纯装饰特效）必须被评审拦截，
//!      且拦截要给出**专属错误码**，与「参数越界」分开归因；
//!    - **预算内特效**（丰盛不以帧率为代价）：每个子系统受帧预算硬约束，
//!      超预算登记降级，不静默截断。
//!
//! 4. **层冻结**（判据四）：四子系统与两翼之间的接口冻结。
//!    冻结 = 冻结期内只能加不能改，且加必须升版本；
//!    改既有语义必须先走 `unfreeze`。直接改即越权。
//!
//! **错误路径**（锚点「层间失配→对拍 / 承接缺源→回溯移交包 / 越权炫技→评审拦截」）：
//! - **层间失配 → 对拍**：拿两侧对同一输入的产出比一遍。
//!   对拍必须能**发现**注入的失配，不是恒真通过。
//! - **承接缺源 → 回溯**：承接 F5193 十件移交包，缺件时回溯到源头件号，
//!   诊断里报源头件号，而不是只说「承接失败」。
//! - **越权炫技 → 评审拦截**：拦截结论要可复核（附被拦特效的语义字段实况）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、四子系统（判据一）
// ---------------------------------------------------------------------------

/// 特效子系统（**责任互斥**：每个子系统只产出自己那类效果）。
///
/// 顺序即登记顺序，不是优先级——优先级由组合系统管，不归这里。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Subsystem {
    /// 粒子：点与力的艺术（烟雾/火花/雨雪/光尘）。
    Particle = 0,
    /// 后处理：全屏合成期效果（色偏/晕影/色调映射后的叠加）。
    PostProcess = 1,
    /// 环境：天气与昼夜的光照气氛。
    Environment = 2,
    /// 转场：镜头与场景之间的过渡。
    Transition = 3,
}

impl Subsystem {
    /// 全部子系统。
    pub const ALL: [Subsystem; SUBSYSTEM_COUNT] = [
        Subsystem::Particle,
        Subsystem::PostProcess,
        Subsystem::Environment,
        Subsystem::Transition,
    ];

    /// 子系统名（人读；特效面向创作者，名字要能被读懂）。
    pub const fn name(self) -> &'static str {
        match self {
            Subsystem::Particle => "粒子",
            Subsystem::PostProcess => "后处理",
            Subsystem::Environment => "环境",
            Subsystem::Transition => "转场",
        }
    }

    /// 本子系统**独占**的效果类别（越界判定的标尺）。
    ///
    /// 越权炫技的第一层含义是「产出不属于自己这一类的效果」：
    /// 一个粒子子系统去改全屏色调就是越界，哪怕它也有内容语义。
    pub const fn exclusive_class(self) -> &'static str {
        match self {
            Subsystem::Particle => "point-force",
            Subsystem::PostProcess => "fullscreen-composite",
            Subsystem::Environment => "ambient-light",
            Subsystem::Transition => "camera-blend",
        }
    }

    /// 该子系统默认的帧预算上限（**丰盛不以帧率为代价**：预算不是建议）。
    ///
    /// 单位：微秒/帧。粒子最贵（要模拟），转场最便宜（一次性混合）。
    pub const fn budget_us(self) -> u32 {
        match self {
            Subsystem::Particle => 2400,
            Subsystem::PostProcess => 1800,
            Subsystem::Environment => 900,
            Subsystem::Transition => 600,
        }
    }

    /// 序（0 起）。
    pub const fn ordinal(self) -> usize {
        self as usize
    }
}

/// 子系统数（锚点「架构 O(子系统数)」的成本模型分母）。
pub const SUBSYSTEM_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// 二、两翼（判据二）
// ---------------------------------------------------------------------------

/// 两翼（参数系统 + 组合系统）。
///
/// 四子系统的**唯一**形参来源与**唯一**编排入口。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Wing {
    /// 参数翼：解决「同一特效在不同场景取不同值」。
    Param = 0,
    /// 组合翼：解决「多特效按什么次序叠加」。
    Compose = 1,
}

impl Wing {
    /// 全部两翼。
    pub const ALL: [Wing; WING_COUNT] = [Wing::Param, Wing::Compose];

    /// 翼名（人读）。
    pub const fn name(self) -> &'static str {
        match self {
            Wing::Param => "参数系统",
            Wing::Compose => "组合系统",
        }
    }

    /// 序（0 起）。
    pub const fn ordinal(self) -> usize {
        self as usize
    }
}

/// 两翼数（判据「两翼」的对账分母）。
pub const WING_COUNT: usize = 2;

// ---------------------------------------------------------------------------
// 三、特效即内容（判据三 · 域本色）
// ---------------------------------------------------------------------------

/// 特效的内容语义（**无语义特效不入册** —— 域本色第一条硬约束）。
///
/// `None` = 纯装饰特效 = 越权炫技，必须被评审拦截。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Semantic {
    /// 声明了内容语义（表达什么）。
    Declared(String),
    /// 未声明语义 —— 炫技特征。
    Undeclared,
}

/// 特效登记项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VfxEntry {
    /// 特效名（人读；创作者按名字找特效）。
    pub name: String,
    /// 产出该特效的子系统。
    pub owner: Subsystem,
    /// 声明的效果类别（必须与 `owner.exclusive_class()` 一致）。
    pub class: &'static str,
    /// 内容语义声明。
    pub semantic: Semantic,
}

/// 特效登记册。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VfxRegistry {
    /// 全部登记项。
    pub entries: Vec<VfxEntry>,
    /// 被评审拦截的炫技特效数（**拦截不删除条目**：删了就把违规藏起来了）。
    pub rejected: u32,
}

impl VfxRegistry {
    /// 构造空册。
    pub fn new() -> VfxRegistry {
        VfxRegistry { entries: Vec::new(), rejected: 0 }
    }

    /// 登记一个特效。
    ///
    /// **拦截时不静默丢弃**：条目照样入册并标记，越权事实留在册里可查。
    /// 三条拦截各有**专属错误码**，不合并归因：
    /// - `UndeclaredSemantic` —— 炫技（无语义特效）；
    /// - `ClassOverreach` —— 越界（产出了不属于本子系统的效果类别）；
    /// - `BudgetExceeded` —— 预算失守（丰盛不以帧率为代价）。
    pub fn register(
        &mut self,
        name: &str,
        owner: Subsystem,
        class: &'static str,
        semantic: Option<&str>,
        cost_us: u32,
    ) -> Result<(), VfxError> {
        // 先判炫技：无语义 = 越权炫技，锚点错误路径「越权炫技 → 评审拦截」。
        let sem = match semantic {
            Some(s) => Semantic::Declared(s.to_string()),
            None => {
                self.entries.push(VfxEntry {
                    name: name.to_string(),
                    owner,
                    class,
                    semantic: Semantic::Undeclared,
                });
                self.rejected += 1;
                return Err(VfxError::UndeclaredSemantic { name: name.to_string() });
            }
        };
        // 再判越界：产出的效果类别不属于本子系统。
        if class != owner.exclusive_class() {
            self.entries.push(VfxEntry {
                name: name.to_string(),
                owner,
                class,
                semantic: sem,
            });
            self.rejected += 1;
            return Err(VfxError::ClassOverreach {
                name: name.to_string(),
                owner,
                got: class,
                want: owner.exclusive_class(),
            });
        }
        // 最后判预算：单特效成本不得超所属子系统的帧预算硬上限。
        // 用 `>` 而非 `>=`：预算恰好用满是允许的（预算就是上限本身）。
        if cost_us > owner.budget_us() {
            self.entries.push(VfxEntry {
                name: name.to_string(),
                owner,
                class,
                semantic: sem,
            });
            self.rejected += 1;
            return Err(VfxError::BudgetExceeded {
                name: name.to_string(),
                owner,
                cost_us,
                budget_us: owner.budget_us(),
            });
        }
        self.entries.push(VfxEntry {
            name: name.to_string(),
            owner,
            class,
            semantic: sem,
        });
        Ok(())
    }

    /// 已登记条数（**绝对值口径**，不用净值）。
    pub fn total(&self) -> u32 {
        self.entries.len() as u32
    }

    /// 入册成功条数（`total - rejected`）。
    pub fn admitted(&self) -> u32 {
        self.total() - self.rejected
    }

    /// 某子系统的入册条数（判据侧按子系统独立核对用）。
    pub fn admitted_of(&self, owner: Subsystem) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.owner == owner && !matches!(e.semantic, Semantic::Undeclared) {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 四、层间接口冻结（判据四）
// ---------------------------------------------------------------------------

/// 接口冻结状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeState {
    /// 草拟：可自由改。
    Draft,
    /// 评审：仍可改，但改动被记为「评审期改动」。
    Review,
    /// 冻结：改既有语义即越权。
    Frozen,
}

/// 改动种类（冻结只禁后者）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmendKind {
    /// 加法：加可选参数/加新类别，不改既有语义。
    Extend,
    /// 修改：改既有语义。
    Change,
}

/// 冻结接口（**两翼 × 四子系统**的层间契约）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenInterface {
    /// 接口名（契约唯一标识；命名规则 `<翼首字母>-<子系统首字母>`）。
    pub name: String,
    /// 上游（必是两翼之一；**子系统不得直接当上游**）。
    pub upstream: Wing,
    /// 下游（必是子系统之一）。
    pub downstream: Subsystem,
    /// 冻结时登记的版本（冻结**不锁版本号**，加法要升版本）。
    pub version: u32,
    /// 冻结状态。
    pub state: FreezeState,
    /// 冻结期内改动次数（供越权追责）。
    pub mutations: u32,
}

impl FrozenInterface {
    /// 新建草拟接口。
    pub fn draft(upstream: Wing, downstream: Subsystem) -> FrozenInterface {
        FrozenInterface {
            name: interface_name(upstream, downstream),
            upstream,
            downstream,
            version: 1,
            state: FreezeState::Draft,
            mutations: 0,
        }
    }

    /// 送评审。
    pub fn to_review(&mut self) {
        if self.state == FreezeState::Draft {
            self.state = FreezeState::Review;
        }
    }

    /// 冻结。
    pub fn freeze(&mut self) {
        if self.state != FreezeState::Frozen {
            self.state = FreezeState::Frozen;
        }
    }

    /// 解冻（**唯一的合法改接口途径**）。返回是否真的解冻了。
    pub fn unfreeze(&mut self) -> bool {
        if self.state == FreezeState::Frozen {
            self.state = FreezeState::Review;
            self.mutations += 1;
            true
        } else {
            false
        }
    }

    /// 登记一次接口改动。
    ///
    /// 冻结态下：`Extend`（加法）允许但**必须升版本**——冻结的是
    /// 「已有语义不得改」，不是「不许长大」；`Change` 直接报越权。
    pub fn amend(&mut self, kind: AmendKind) -> Result<u32, VfxError> {
        match self.state {
            FreezeState::Frozen => match kind {
                AmendKind::Extend => {
                    self.version += 1;
                    Ok(self.version)
                }
                AmendKind::Change => Err(VfxError::AmendFrozen {
                    interface: self.name.clone(),
                }),
            },
            _ => {
                self.mutations += 1;
                if kind == AmendKind::Extend {
                    self.version += 1;
                }
                Ok(self.version)
            }
        }
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.state == FreezeState::Frozen
    }
}

/// 层间接口名（`<翼首字母>-<子系统首字母>`，如 `p-pa`）。
///
/// 接口名是契约的唯一标识：冻结、越权告警、对拍全按名字对齐。
/// 必须**含子系统**——只写 `p-` 无法区分是哪个子系统的形参入口，
/// 四条通道会挤成一条，两翼形同虚设。
pub fn interface_name(upstream: Wing, downstream: Subsystem) -> String {
    let w = match upstream {
        Wing::Param => "p",
        Wing::Compose => "c",
    };
    let s = match downstream {
        Subsystem::Particle => "pa",
        Subsystem::PostProcess => "po",
        Subsystem::Environment => "en",
        Subsystem::Transition => "tr",
    };
    format!("{}-{}", w, s)
}

/// 层间接口总数（两翼 × 四子系统；锚点「冻结 O(接口数)」的操作数）。
pub const INTERFACE_COUNT: usize = WING_COUNT * SUBSYSTEM_COUNT;

/// 全部应冻结的接口（两翼 × 四子系统的笛卡尔积，单源清单）。
pub fn all_interfaces() -> Vec<FrozenInterface> {
    let mut v: Vec<FrozenInterface> = Vec::new();
    let mut i = 0usize;
    while i < WING_COUNT {
        let w = Wing::ALL[i];
        let mut k = 0usize;
        while k < SUBSYSTEM_COUNT {
            v.push(FrozenInterface::draft(w, Subsystem::ALL[k]));
            k += 1;
        }
        i += 1;
    }
    v
}

// ---------------------------------------------------------------------------
// 五、层间对拍（错误路径：层间失配 → 对拍）
// ---------------------------------------------------------------------------

/// 对拍两侧产出（按**边界名**索引的具名摘要，不是裸字节）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideOutput {
    /// 边界名（`p-pa` 之类）。
    pub boundary: String,
    /// 产出内容摘要（逐项拼接，便于定位差异项）。
    pub items: Vec<String>,
}

impl SideOutput {
    /// 构造。
    pub fn new(boundary: &str, items: &[&str]) -> SideOutput {
        let mut v: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < items.len() {
            v.push(items[i].to_string());
            i += 1;
        }
        SideOutput { boundary: boundary.to_string(), items: v }
    }

    /// 摘要（拼接所有项）。
    pub fn digest(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.items.len() {
            if i > 0 {
                s.push('|');
            }
            s.push_str(&self.items[i]);
            i += 1;
        }
        s
    }
}

/// 对拍结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffReport {
    /// 是否一致。
    pub matched: bool,
    /// 失配边界（`None` = 全一致）。
    pub boundary: Option<String>,
    /// 差异项下标（第一个不同处；长度不同也必须给出）。
    pub first_gap: Option<usize>,
    /// 人话说明。
    pub detail: String,
}

/// 层间对拍（锚点错误路径「层间失配 → 对拍」）。
///
/// 两侧逐项比：长度不同报长度失配，同长则找第一处不同。
/// **`first_gap` 在长度不同的情形下也给出** —— 只在同长时找差异，
/// 会让「少了一项」这种最常见失配报不出位置。
pub fn diff_boundary(a: &SideOutput, b: &SideOutput) -> DiffReport {
    if a.boundary != b.boundary {
        return DiffReport {
            matched: false,
            boundary: Some(a.boundary.clone()),
            first_gap: None,
            detail: format!(
                "对拍两侧边界名不同：一边是 {}，另一边是 {}。接口对错了。",
                a.boundary, b.boundary
            ),
        };
    }
    let n = a.items.len();
    let m = b.items.len();
    let mut i = 0usize;
    while i < n && i < m {
        if a.items[i] != b.items[i] {
            return DiffReport {
                matched: false,
                boundary: Some(a.boundary.clone()),
                first_gap: Some(i),
                detail: format!(
                    "{}：第 {} 项不同——一边是「{}」，另一边是「{}」。",
                    a.boundary, i, a.items[i], b.items[i]
                ),
            };
        }
        i += 1;
    }
    if n != m {
        let gap = if n < m { n } else { m };
        return DiffReport {
            matched: false,
            boundary: Some(a.boundary.clone()),
            first_gap: Some(gap),
            detail: format!(
                "{}：项数不同——一边 {} 项，另一边 {} 项，差在第 {} 项之后。",
                a.boundary, n, m, gap
            ),
        };
    }
    DiffReport {
        matched: true,
        boundary: None,
        first_gap: None,
        detail: format!("{}：{} 项逐项一致。", a.boundary, n),
    }
}

// ---------------------------------------------------------------------------
// 六、承接面落地表（判据：承接 F5193 移交包十件）
// ---------------------------------------------------------------------------

/// 承接件来源（锚点「承接 Y 域移交包 F5193 十件」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffFace {
    /// 特效参数消费脚本绑定能力。
    ScriptBinding,
    /// 渲染协议消费。
    RenderProtocol,
    /// 预算口径消费。
    BudgetLedger,
}

/// 承接件（F5193 十件之一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoffItem {
    /// 件号（`F5193-01` … `F5193-10`）。
    pub serial: &'static str,
    /// 承接面。
    pub face: HandoffFace,
    /// 人读标签。
    pub label: &'static str,
}

/// 移交包件数（锚点「F5193 十件」）。
pub const HANDOFF_COUNT: usize = 10;

/// F5193 移交包的十件（**判据侧与实现侧共用的单源清单**）。
pub const HANDOFF_ITEMS: [HandoffItem; HANDOFF_COUNT] = [
    HandoffItem { serial: "F5193-01", face: HandoffFace::ScriptBinding, label: "接口总账·特效绑定面" },
    HandoffItem { serial: "F5193-02", face: HandoffFace::ScriptBinding, label: "接口总账·形参口径" },
    HandoffItem { serial: "F5193-03", face: HandoffFace::ScriptBinding, label: "脚本指标·特效可读性" },
    HandoffItem { serial: "F5193-04", face: HandoffFace::ScriptBinding, label: "场景覆盖·特效挂点" },
    HandoffItem { serial: "F5193-05", face: HandoffFace::RenderProtocol, label: "渲染协议·合成层序" },
    HandoffItem { serial: "F5193-06", face: HandoffFace::RenderProtocol, label: "渲染协议·后处理链" },
    HandoffItem { serial: "F5193-07", face: HandoffFace::RenderProtocol, label: "渲染协议·转场混合" },
    HandoffItem { serial: "F5193-08", face: HandoffFace::BudgetLedger, label: "性能总册·帧预算口径" },
    HandoffItem { serial: "F5193-09", face: HandoffFace::BudgetLedger, label: "性能总册·降级登记" },
    HandoffItem { serial: "F5193-10", face: HandoffFace::BudgetLedger, label: "安全隐私·无障碍门禁" },
];

/// 落地状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LandingState {
    /// 未落地。
    Pending,
    /// 已落地且核验通过。
    Landed,
}

/// 承接面落地表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoffLedger {
    /// 逐件落地状态。
    pub states: Vec<LandingState>,
}

impl HandoffLedger {
    /// 构造（全部未落地）。
    pub fn new() -> HandoffLedger {
        let mut v: Vec<LandingState> = Vec::new();
        let mut i = 0usize;
        while i < HANDOFF_COUNT {
            v.push(LandingState::Pending);
            i += 1;
        }
        HandoffLedger { states: v }
    }

    /// 落地一件（**下标越界即拒绝**，不静默丢弃）。
    pub fn land(&mut self, idx: usize) -> Result<(), VfxError> {
        if idx >= HANDOFF_COUNT {
            return Err(VfxError::HandoffMissing {
                item: "越界下标",
                source: "F5193",
            });
        }
        self.states[idx] = LandingState::Landed;
        Ok(())
    }

    /// 已落地件数。
    pub fn landed_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.states.len() {
            if self.states[i] == LandingState::Landed {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 首个未落地下标（`None` = 全落地）。
    pub fn first_gap(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < self.states.len() {
            if self.states[i] != LandingState::Landed {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 某承接面已落地件数。
    pub fn landed_of(&self, face: HandoffFace) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.states.len() {
            if self.states[i] == LandingState::Landed && HANDOFF_ITEMS[i].face == face {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 该面在清单里的总件数（判据侧核对上界用）。
    pub fn total_of(face: HandoffFace) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < HANDOFF_COUNT {
            if HANDOFF_ITEMS[i].face == face {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 承接面缺口（**回溯到 F5193 源头件号**，锚点「承接缺源 → 回溯移交包」）。
    pub fn first_missing(&self) -> Option<VfxError> {
        match self.first_gap() {
            Some(i) => Some(VfxError::HandoffMissing {
                item: HANDOFF_ITEMS[i].label,
                source: HANDOFF_ITEMS[i].serial,
            }),
            None => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 七、总架构册
// ---------------------------------------------------------------------------

/// 特效库总架构册（四子系统 + 两翼 + 承接面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VfxArch {
    /// 四子系统。
    pub subsystems: Vec<Subsystem>,
    /// 两翼。
    pub wings: Vec<Wing>,
    /// 层间接口（两翼 × 四子系统）。
    pub interfaces: Vec<FrozenInterface>,
    /// 特效登记册。
    pub registry: VfxRegistry,
    /// 承接面落地表。
    pub handoff: HandoffLedger,
}

impl VfxArch {
    /// 构造标准总架构（四子系统 + 两翼 + 空登记册 + 未落地承接）。
    pub fn new() -> VfxArch {
        let mut subsystems: Vec<Subsystem> = Vec::new();
        let mut i = 0usize;
        while i < SUBSYSTEM_COUNT {
            subsystems.push(Subsystem::ALL[i]);
            i += 1;
        }
        let mut wings: Vec<Wing> = Vec::new();
        let mut k = 0usize;
        while k < WING_COUNT {
            wings.push(Wing::ALL[k]);
            k += 1;
        }
        VfxArch {
            subsystems,
            wings,
            interfaces: all_interfaces(),
            registry: VfxRegistry::new(),
            handoff: HandoffLedger::new(),
        }
    }

    /// 子系统数。
    pub fn subsystem_count(&self) -> u32 {
        self.subsystems.len() as u32
    }

    /// 两翼数。
    pub fn wing_count(&self) -> u32 {
        self.wings.len() as u32
    }

    /// 接口数。
    pub fn interface_count(&self) -> u32 {
        self.interfaces.len() as u32
    }

    /// 全部冻结（判据「层冻结」）。
    pub fn freeze_all(&mut self) {
        let mut i = 0usize;
        while i < self.interfaces.len() {
            self.interfaces[i].freeze();
            i += 1;
        }
    }

    /// 冻结态接口数。
    pub fn frozen_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.interfaces.len() {
            if self.interfaces[i].is_frozen() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 按名查接口。
    pub fn interface_of(&self, name: &str) -> Option<&FrozenInterface> {
        let mut i = 0usize;
        while i < self.interfaces.len() {
            if self.interfaces[i].name == name {
                return Some(&self.interfaces[i]);
            }
            i += 1;
        }
        None
    }

    /// 冻结态下改某接口的既有语义（**越权**）。
    pub fn amend_interface(
        &mut self,
        name: &str,
        kind: AmendKind,
    ) -> Result<u32, VfxError> {
        let mut i = 0usize;
        while i < self.interfaces.len() {
            if self.interfaces[i].name == name {
                return self.interfaces[i].amend(kind);
            }
            i += 1;
        }
        Err(VfxError::AmendFrozen { interface: name.to_string() })
    }
}

// ---------------------------------------------------------------------------
// 八、错误路径与降级矩阵
// ---------------------------------------------------------------------------

/// 特效库架构级错误（**失败分类不合并**：五条各有专属码位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VfxError {
    /// 越权炫技：无语义特效（锚点错误路径「越权炫技 → 评审拦截」）。
    UndeclaredSemantic { name: String },
    /// 越界：子系统产出了不属于自己那类的效果。
    ClassOverreach {
        name: String,
        owner: Subsystem,
        got: &'static str,
        want: &'static str,
    },
    /// 预算失守（丰盛不以帧率为代价）。
    BudgetExceeded {
        name: String,
        owner: Subsystem,
        cost_us: u32,
        budget_us: u32,
    },
    /// 接口越权（冻结态改既有语义）。
    AmendFrozen { interface: String },
    /// 承接缺源（回溯到 F5193 源头件号）。
    HandoffMissing { item: &'static str, source: &'static str },
}

impl VfxError {
    /// 诊断码（**五条互不撞码**，一位一位分开占位）。
    ///
    /// 位布局：`0x5B0000 | (分支序号 << 12) | (子系统序 << 8)`，
    /// 即 **32 位码的三个字节段互不重叠**：
    /// - 第 0~7 位（`0x5B`）：故障类标识，恒定；
    /// - 第 12~15 位：分支序号（5 类失败，够 16 类）；
    /// - 第 8~11 位：子系统序（4 子系统，够 16 个）。
    ///
    /// **踩过的坑**：早先写`0x5B00 | (分支 << 8)`，看着分支进了高位段，
    /// 其实 `0x5B` 本身就占着第 8~15 位（`0x5B` = `0101 1011`，第 8 位是 1），
    /// 与分支序号的位段**重叠**，于是分支 0 与分支 1 都被 `0x5B00` 掩掉、
    /// 算出同一个码，五个分支塌成三个码。
    /// 位段不重叠是「失败分类不合并」的物理前提，
    /// 判据侧要能**独立重算**这些段并对拍（见 checks 同名判据）。
    pub const fn code(&self) -> u32 {
        let base: u32 = 0x5B_0000;
        match self {
            VfxError::UndeclaredSemantic { .. } => base | (0u32 << 12),
            VfxError::ClassOverreach { .. } => base | (1u32 << 12),
            VfxError::BudgetExceeded { owner, .. } => {
                base | (2u32 << 12) | (*owner as u32) << 8
            }
            VfxError::AmendFrozen { .. } => base | (4u32 << 12),
            VfxError::HandoffMissing { .. } => base | (5u32 << 12),
        }
    }

    /// 人话说明（特效面向创作者：名字与问题都要能被读懂）。
    pub fn explain(&self) -> String {
        match self {
            VfxError::UndeclaredSemantic { name } => format!(
                "特效「{}」没有声明内容语义。特效是内容的表情，说不清它在表达什么的，\
                 是炫技不是内容，评审不予通过。",
                name
            ),
            VfxError::ClassOverreach { name, owner, got, want } => format!(
                "特效「{}」归属{}子系统，却产出了「{}」类效果——该类效果只属于「{}」。\
                 请把它挂到正确的子系统下。",
                name,
                owner.name(),
                got,
                want
            ),
            VfxError::BudgetExceeded { name, owner, cost_us, budget_us } => format!(
                "特效「{}」耗时 {} 微秒/帧，超出{}子系统的 {} 微秒硬上限。\
                 特效要丰盛，但丰盛不以帧率为代价。",
                name,
                cost_us,
                owner.name(),
                budget_us
            ),
            VfxError::AmendFrozen { interface } => format!(
                "接口 {} 已冻结，不能改它的既有语义。若确实要改，先走解冻流程（unfreeze）再改。",
                interface
            ),
            VfxError::HandoffMissing { item, source } => format!(
                "承接缺件：{} 没拿到。它来自 Y 域移交包的 {}，请回溯那一件补齐。",
                item, source
            ),
        }
    }

    /// 可操作建议（**改什么**，不是复述错在哪）。
    pub fn advise(&self) -> &'static str {
        match self {
            VfxError::UndeclaredSemantic { .. } => {
                "补一句内容语义：这个特效在替内容表达什么？写出来再入库。"
            }
            VfxError::ClassOverreach { .. } => "把效果类别改成所属子系统的独占类别，或改挂到正确子系统。",
            VfxError::BudgetExceeded { .. } => "降粒子数/降后处理采样/把转场移到镜头切点之外，直到落回预算内。",
            VfxError::AmendFrozen { .. } => "改用 Extend（只加不改），或先 unfreeze 该接口再改。",
            VfxError::HandoffMissing { .. } => "到 F5193 移交包核对源头件是否交付，缺则先补上游。",
        }
    }
}

// ---------------------------------------------------------------------------
// 九、人读总述
// ---------------------------------------------------------------------------

/// 架构总览（人话，给创作者看）。
pub fn describe() -> String {
    let mut s = String::new();
    s.push_str("Z 域特效库总架构：\n");
    s.push_str("  四子系统（责任互斥，各产一类效果）：\n");
    let mut i = 0usize;
    while i < SUBSYSTEM_COUNT {
        let sub = Subsystem::ALL[i];
        s.push_str("    ");
        s.push_str(sub.name());
        s.push_str(" → 产出 ");
        s.push_str(sub.exclusive_class());
        s.push_str(" 类，帧预算 ");
        s.push_str(&sub.budget_us().to_string());
        s.push_str(" 微秒\n");
        i += 1;
    }
    s.push_str("  两翼（唯一的形参来源与编排入口）：\n");
    let mut k = 0usize;
    while k < WING_COUNT {
        let w = Wing::ALL[k];
        s.push_str("    ");
        s.push_str(w.name());
        s.push('\n');
        k += 1;
    }
    s.push_str("  接口冻结后，只许加不许改；要改先走解冻。\n");
    s.push_str("  特效即内容：无内容语义声明的特效，评审一律拦截。\n");
    s
}

/// 冒烟：标准总架构 + 全部冻结 + 全部承接落地。
pub fn smoke() -> String {
    let mut a = VfxArch::new();
    a.freeze_all();
    let mut i = 0usize;
    while i < HANDOFF_COUNT {
        let _ = a.handoff.land(i);
        i += 1;
    }
    format!(
        "子系统 {} 两翼 {} 接口 {} 冻结 {} 承接 {}/{}",
        a.subsystem_count(),
        a.wing_count(),
        a.interface_count(),
        a.frozen_count(),
        a.handoff.landed_count(),
        HANDOFF_COUNT
    )
}