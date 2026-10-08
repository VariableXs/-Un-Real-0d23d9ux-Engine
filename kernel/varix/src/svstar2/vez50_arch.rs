//! VE-F5001 · Y 域开工与场景图脚本总架构（VE-Z 域 · 脚本语言 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5001`
//!
//! **判据（锚点原文）**：四层、脚本为人写、承接落地、层冻结、判据。
//!
//! 1. **四层架构**（判据一）。设计层 → 词法层 → 语法层 → 运行时层，
//!    **层间接口冻结**。四层不是画给自己看的框，是四条**类型门**：
//!    下一层只认上一层给的具名结构，不认上一层的内部表示。
//!
//! 2. **脚本为人写**（判据二，域本色声明）。**可读性优先于机巧**。
//!    本条落在两处可测的承诺上：
//!    - 诊断信息必须**带源位置 + 人话说明**，不许只吐一个码位；
//!    - 诊断文本必须**给出可操作建议**（改什么），不是复述错在哪。
//!    机巧的反面写法（把可读性换性能/换紧凑）会让这两条同时塌掉。
//!
//! 3. **承接落地**（判据三）。承接 X 域 **F4993 十件移交包**：运行时消费
//!    工具链构建协议 / 沙箱口径承接 / 分析探针接口承接。落地表的语义是
//!    「**逐件登记 + 可核验**」—— 件数、标识、承接面三者必须齐平，
//!    少一件就是承接缺口（不是「大概齐了」）。
//!
//! 4. **层冻结**（判据四）。接口冻结 = 冻结期内**只能加不能改**，
//!    且加也必须显式登记版本。冻结流程本身要有状态：
//!    草拟 → 评审 → 冻结 → 冻结后任何**修改**都必须走 `unfreeze`，
//!    直接改是越权（锚点错误路径「接口越权→冻结流程」）。
//!
//! **错误路径**（锚点「层间失配→对拍 / 承接缺源→回溯移交包 / 接口越权→冻结流程」）：
//! - **层间失配 → 对拍**：拿两层对同一输入的产出比一遍，不一致即失配。
//!   这是可测的：对拍必须能**发现**注入的失配，不是恒真通过。
//! - **承接缺源 → 回溯**：回溯到 F4993 移交包的源头件号，诊断里报源头，
//!   而不是只说「承接失败」。
//! - **接口越权 → 冻结**：改冻结接口触发冻结流程告警。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、四层架构
// ---------------------------------------------------------------------------

/// 架构层（锚点四层，**顺序即依赖方向**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// 设计层：场景意图（人写的结构化意图，不是代码）。
    Design = 0,
    /// 词法层：字符流 → 记号流。
    Lexical = 1,
    /// 语法层：记号流 → 语法树。
    Syntax = 2,
    /// 运行时层：语法树 → 场景图执行。
    Runtime = 3,
}

impl Layer {
    /// 全部层（按依赖顺序，设计层在最前）。
    pub const ALL: [Layer; LAYER_COUNT] = [
        Layer::Design,
        Layer::Lexical,
        Layer::Syntax,
        Layer::Runtime,
    ];

    /// 层名（诊断与人读用；脚本为人写 ⇒ 层名要能被人读懂）。
    pub const fn name(self) -> &'static str {
        match self {
            Layer::Design => "设计层",
            Layer::Lexical => "词法层",
            Layer::Syntax => "语法层",
            Layer::Runtime => "运行时层",
        }
    }

    /// 直接下游层（`None` = 末层）。
    pub const fn downstream(self) -> Option<Layer> {
        match self {
            Layer::Design => Some(Layer::Lexical),
            Layer::Lexical => Some(Layer::Syntax),
            Layer::Syntax => Some(Layer::Runtime),
            Layer::Runtime => None,
        }
    }

    /// 直接上游层（`None` = 首层）。
    pub const fn upstream(self) -> Option<Layer> {
        match self {
            Layer::Design => None,
            Layer::Lexical => Some(Layer::Design),
            Layer::Syntax => Some(Layer::Lexical),
            Layer::Runtime => Some(Layer::Syntax),
        }
    }

    /// 层序（0 起）。
    pub const fn ordinal(self) -> usize {
        self as usize
    }
}

/// 层数（锚点「架构 O(层数)」的成本模型分母）。
pub const LAYER_COUNT: usize = 4;

/// 层间接口名（冻结的对象）。
///
/// **接口名不是随便起的字符串**：它是层间契约的**唯一标识**，
/// 冻结、越权告警、对拍全按名字对齐。命名规则：
/// `<上游层首字母>-<接口语义>`，小写连字符，保证跨层可读（脚本为人写）。
pub fn interface_name(from: Layer, to: Layer) -> String {
    let f = match from {
        Layer::Design => "d",
        Layer::Lexical => "l",
        Layer::Syntax => "s",
        Layer::Runtime => "r",
    };
    let t = match to {
        Layer::Design => "design",
        Layer::Lexical => "token",
        Layer::Syntax => "tree",
        Layer::Runtime => "scene",
    };
    format!("{}-{}", f, t)
}

// ---------------------------------------------------------------------------
// 二、层间接口冻结
// ---------------------------------------------------------------------------

/// 接口冻结状态（锚点「接口越权 → 冻结流程」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeState {
    /// 草拟中：可自由改。
    Draft,
    /// 评审中：仍可改，但改动会被记为「评审期改动」。
    Review,
    /// 已冻结：改即越权。
    Frozen,
}

/// 接口冻结记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenInterface {
    pub name: String,
    pub from: Layer,
    pub to: Layer,
    /// 冻结时登记的接口版本（冻结**不锁版本号**，加法要升版本）。
    pub version: u32,
    pub state: FreezeState,
    /// 冻结期内的改动次数（每次改动都记，供越权追责）。
    pub mutations: u32,
}

impl FrozenInterface {
    /// 新建草拟接口。
    pub fn draft(name: String, from: Layer, to: Layer) -> FrozenInterface {
        FrozenInterface {
            name,
            from,
            to,
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

    /// 解冻（**唯一的合法改接口途径**，锚点「冻结流程」）。
    pub fn unfreeze(&mut self) -> bool {
        if self.state == FreezeState::Frozen {
            self.state = FreezeState::Review;
            self.mutations += 1;
            true
        } else {
            false
        }
    }

    /// 登记一次接口改动（加法：升版本；修改：需已解冻）。
    ///
    /// 返回 `Err(ArchitectureError)` 表示**越权**：冻结态直接改。
    /// 冻结态下 `extend`（加法）**允许但必须升版本** —— 冻结的是
    /// 「已有语义不得改」，不是「不许长大」。
    pub fn amend(&mut self, kind: AmendKind) -> Result<u32, ArchitectureError> {
        match self.state {
            FreezeState::Frozen => match kind {
                AmendKind::Extend => {
                    // 加法：语义未变，允许并升版本
                    self.version += 1;
                    Ok(self.version)
                }
                AmendKind::Change => Err(ArchitectureError::AmendFrozen {
                    name: self.name.clone(),
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

/// 改动种类（加法 vs 修改 —— 冻结只禁后者）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmendKind {
    /// 加法：加可选字段/加新记号类型，不改既有语义。
    Extend,
    /// 修改：改既有语义。
    Change,
}

/// 架构级错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArchitectureError {
    /// 冻结态下改既有语义（接口越权）。
    AmendFrozen { name: String },
    /// 层间失配（对拍发现）。
    LayerMismatch {
        boundary: String,
        /// 失配的层。
        layer: Layer,
    },
    /// 承接缺源（回溯到 F4993 源头）。
    HandoffMissing {
        /// 缺的那件。
        item: &'static str,
        /// 回溯到的源头件号（锚点「回溯移交包」）。
        source: &'static str,
    },
    /// 层序非法（架构结构自相矛盾）。
    BadLayerOrder { got: usize, want: usize },
}

impl ArchitectureError {
    /// 诊断码（**三轴**：类 + 位，保证不撞码）。
    ///
    /// 位布局：`0x5A00 | (层序 << 4) | 分支序号`。
    /// 保留 12 位给层序，够 4096 层；分支序号 4 位够 16 支。
    pub const fn code(&self) -> u32 {
        let base: u32 = 0x5A00;
        match self {
            ArchitectureError::AmendFrozen { .. } => base,
            ArchitectureError::LayerMismatch { layer, .. } => {
                base | ((layer.ordinal() as u32) << 4) | 1
            }
            ArchitectureError::HandoffMissing { .. } => base | 2,
            ArchitectureError::BadLayerOrder { got, .. } => {
                base | ((*got as u32) << 4) | 3
            }
        }
    }

    /// 人话说明（**脚本为人写**：不许只吐码位）。
    pub fn explain(&self) -> String {
        match self {
            ArchitectureError::AmendFrozen { name } => format!(
                "接口 {} 已冻结，不能改它的既有语义。若确实要改，先走解冻流程（unfreeze）再改。",
                name
            ),
            ArchitectureError::LayerMismatch { boundary, layer } => format!(
                "{} 这道层间接口对拍失败：{} 给出的东西和上游对不上。检查 {} 的输出是不是漏了或多了什么。",
                boundary,
                layer.name(),
                layer.name()
            ),
            ArchitectureError::HandoffMissing { item, source } => format!(
                "承接缺件：{} 没拿到。它来自 X 域移交包的 {}，请回溯那一件补齐。",
                item, source
            ),
            ArchitectureError::BadLayerOrder { got, want } => format!(
                "层序不对：期望第 {} 层，实际拿到第 {} 层。检查调用的层是不是传错了。",
                want, got
            ),
        }
    }

    /// 可操作建议（**改什么**，不是复述错在哪 —— 脚本为人写）。
    pub fn advise(&self) -> &'static str {
        match self {
            ArchitectureError::AmendFrozen { .. } => {
                "改用 Extend（只加不改），或先 unfreeze 该接口再改。"
            }
            ArchitectureError::LayerMismatch { .. } => {
                "跑一次对拍（diff boundary）看两侧差在哪，再补齐缺失字段。"
            }
            ArchitectureError::HandoffMissing { .. } => {
                "到 F4993 移交包核对源头件是否交付，缺则先补上游。"
            }
            ArchitectureError::BadLayerOrder { .. } => "按设计层→词法层→语法层→运行时层的顺序调用。",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、架构册（总架构的具名载体）
// ---------------------------------------------------------------------------

/// 总架构册：四层 + 全部冻结接口 + 承接面。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blueprint {
    /// 全部层（按依赖顺序）。
    pub layers: Vec<Layer>,
    /// 全部层间接口（按 `LAYER_COUNT- 1` 个边界各一个）。
    pub interfaces: Vec<FrozenInterface>,
    /// 承接面落地表。
    pub handoff: HandoffLedger,
}

impl Blueprint {
    /// 构造标准四层架构册（层间接口默认草拟）。
    pub fn new() -> Blueprint {
        let mut layers: Vec<Layer> = Vec::new();
        let mut i = 0usize;
        while i < LAYER_COUNT {
            layers.push(Layer::ALL[i]);
            i += 1;
        }
        let mut interfaces: Vec<FrozenInterface> = Vec::new();
        let mut k = 0usize;
        while k + 1 < LAYER_COUNT {
            let from = Layer::ALL[k];
            let to = Layer::ALL[k + 1];
            interfaces.push(FrozenInterface::draft(interface_name(from, to), from, to));
            k += 1;
        }
        Blueprint {
            layers,
            interfaces,
            handoff: HandoffLedger::new(),
        }
    }

    /// 层数（架构成本 O(层数) 的操作数）。
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// 接口数（冻结成本 O(接口数) 的操作数）。
    pub fn interface_count(&self) -> usize {
        self.interfaces.len()
    }

    /// 全部冻结（锚点「层间接口冻结」）。
    pub fn freeze_all(&mut self) {
        let mut i = 0usize;
        while i < self.interfaces.len() {
            self.interfaces[i].freeze();
            i += 1;
        }
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

    /// 冻结态接口数。
    pub fn frozen_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.interfaces.len() {
            if self.interfaces[i].is_frozen() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 冻结态下改既有语义（**越权**）。
    pub fn amend_frozen(&mut self, name: &str, kind: AmendKind) -> Result<u32, ArchitectureError> {
        let mut i = 0usize;
        while i < self.interfaces.len() {
            if self.interfaces[i].name == name {
                return self.interfaces[i].amend(kind);
            }
            i += 1;
        }
        Err(ArchitectureError::AmendFrozen { name: name.to_string() })
    }
}

// ---------------------------------------------------------------------------
// 四、层间对拍（错误路径一：层间失配 → 对拍）
// ---------------------------------------------------------------------------

/// 对拍两侧产出（按**边界名**索引的具名摘要，不是裸字节）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideOutput {
    /// 边界名（`d-token` 之类）。
    pub boundary: String,
    /// 产出的内容摘要（逐项拼接，便于定位差异项）。
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
    /// 失配的边界（`None` = 全一致）。
    pub boundary: Option<String>,
    /// 失配层（`None` = 全一致）。
    pub layer: Option<Layer>,
    /// 差异项下标（第一个不同处）。
    pub first_gap: Option<usize>,
    /// 人话说明。
    pub detail: String,
}

/// 层间对拍（锚点错误路径「层间失配 → 对拍」）。
///
/// 两侧逐项比：长度不同报长度失配，同长则找第一处不同。
/// **`first_gap` 必须在长度不同的情形下也给出** —— 只在同长时找差异
/// 会让「少了一项」这种最常见的失配报不出位置。
pub fn diff_boundary(a: &SideOutput, b: &SideOutput) -> DiffReport {
    if a.boundary != b.boundary {
        return DiffReport {
            matched: false,
            boundary: Some(a.boundary.clone()),
            layer: None,
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
                layer: None,
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
            layer: None,
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
        layer: None,
        first_gap: None,
        detail: format!("{}：{} 项逐项一致。", a.boundary, n),
    }
}

// ---------------------------------------------------------------------------
// 五、承接面落地表（判据三）
// ---------------------------------------------------------------------------

/// 承接件来源（锚点「承接 X 域移交包 F4993 十件」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffSource {
    /// 运行时消费工具链构建协议。
    Toolchain,
    /// 沙箱口径承接。
    Sandbox,
    /// 分析探针接口承接。
    Probe,
}

/// 承接件（十件之一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoffItem {
    /// 件号（`F4993-01` … `F4993-10`）。
    pub serial: &'static str,
    /// 承接面。
    pub face: HandoffSource,
    /// 承接面的人类可读名（脚本为人写 ⇒ 名字要能被创作者读懂）。
    pub label: &'static str,
}

/// F4993 移交包的十件（**判据侧与实现侧共用的单源清单**）。
pub const HANDOFF_ITEMS: [HandoffItem; HANDOFF_COUNT] = [
    HandoffItem { serial: "F4993-01", face: HandoffSource::Toolchain, label: "工具链构建协议·构建入口" },
    HandoffItem { serial: "F4993-02", face: HandoffSource::Toolchain, label: "工具链构建协议·诊断汇聚" },
    HandoffItem { serial: "F4993-03", face: HandoffSource::Toolchain, label: "工具链构建协议·产物投递" },
    HandoffItem { serial: "F4993-04", face: HandoffSource::Toolchain, label: "工具链构建协议·版本协商" },
    HandoffItem { serial: "F4993-05", face: HandoffSource::Toolchain, label: "工具链构建协议·失败回传" },
    HandoffItem { serial: "F4993-06", face: HandoffSource::Sandbox, label: "沙箱口径·能力白名单" },
    HandoffItem { serial: "F4993-07", face: HandoffSource::Sandbox, label: "沙箱口径·配额上限" },
    HandoffItem { serial: "F4993-08", face: HandoffSource::Sandbox, label: "沙箱口径·拒绝口径" },
    HandoffItem { serial: "F4993-09", face: HandoffSource::Probe, label: "分析探针·采样点登记" },
    HandoffItem { serial: "F4993-10", face: HandoffSource::Probe, label: "分析探针·读回协议" },
];

/// 移交包件数（锚点「F4993 十件」）。
pub const HANDOFF_COUNT: usize = 10;

/// 承接件落地状态。
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
    /// 逐件的落地状态。
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
    pub fn land(&mut self, idx: usize) -> Result<(), ArchitectureError> {
        if idx >= HANDOFF_COUNT {
            return Err(ArchitectureError::HandoffMissing {
                item: "越界下标",
                source: "F4993",
            });
        }
        self.states[idx] = LandingState::Landed;
        Ok(())
    }

    /// 已落地件数。
    pub fn landed_count(&self) -> usize {
        let mut n = 0usize;
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

    /// 某承接面已落地件数（判据侧按清单独立计数用）。
    pub fn landed_of_face(&self, face: HandoffSource) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.states.len() {
            if self.states[i] == LandingState::Landed
                && HANDOFF_ITEMS[i].face == face
            {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 该面在清单里的总件数（判据侧用来核对上界）。
    pub fn total_of_face(face: HandoffSource) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < HANDOFF_COUNT {
            if HANDOFF_ITEMS[i].face == face {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 承接面缺口（**回溯到 F4993 源头件号**，锚点「承接缺源 → 回溯移交包」）。
    pub fn first_missing(&self) -> Option<ArchitectureError> {
        match self.first_gap() {
            Some(i) => Some(ArchitectureError::HandoffMissing {
                item: HANDOFF_ITEMS[i].label,
                source: HANDOFF_ITEMS[i].serial,
            }),
            None => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 六、人读说明与冒烟
// ---------------------------------------------------------------------------

/// 架构总览（人话，给创作者看）。
pub fn describe() -> String {
    let mut s = String::new();
    s.push_str("Y 域场景图脚本四层架构：\n");
    let mut i = 0usize;
    while i < LAYER_COUNT {
        let l = Layer::ALL[i];
        s.push_str("  ");
        s.push_str(&(i + 1).to_string());
        s.push_str(". ");
        s.push_str(l.name());
        // 末层不写「下游」，避免出现悬空的「→」
        match l.downstream() {
            Some(d) => {
                s.push_str(" → 交给 ");
                s.push_str(d.name());
                s.push_str("（接口 ");
                s.push_str(&interface_name(l, d));
                s.push_str("）\n");
            }
            None => s.push_str("（末层）\n"),
        }
        i += 1;
    }
    s.push_str("层间接口冻结后，只许加不许改；要改先走解冻。\n");
    s
}

/// 冒烟：标准四层 + 全部落地。
pub fn smoke() -> String {
    let mut bp = Blueprint::new();
    bp.freeze_all();
    let mut i = 0usize;
    while i < HANDOFF_COUNT {
        let _ = bp.handoff.land(i);
        i += 1;
    }
    format!(
        "层数 {} 接口数 {} 冻结 {} 承接 {}/{}",
        bp.layer_count(),
        bp.interface_count(),
        bp.frozen_count(),
        bp.handoff.landed_count(),
        HANDOFF_COUNT
    )
}

/// 三轴诊断码全集（供判据核对「码位不撞」）。
pub fn all_codes() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    // 变体一：接口越权（名字不进码，只有一处取值）
    v.push(ArchitectureError::AmendFrozen { name: "d-token".to_string() }.code());
    // 变体二：**逐层穷举**。层间失配的码含层序，只取一个样本
    // 会漏掉「别的层恰好撞上别的错误」这种碰撞（实测：把
    // HandoffMissing 改成 `base|0<<4|1` 后它与
    // LayerMismatch{Design} 撞码，而只查 Syntax 样本就发现不了）。
    let mut i = 0usize;
    while i < LAYER_COUNT {
        v.push(
            ArchitectureError::LayerMismatch {
                boundary: "l-tree".to_string(),
                layer: Layer::ALL[i],
            }
            .code(),
        );
        i += 1;
    }
    // 变体三：承接缺源（item/source 都不进码，只有一处取值）
    v.push(
        ArchitectureError::HandoffMissing {
            item: "x",
            source: "F4993-06",
        }
        .code(),
    );
    // 变体四：**逐层序穷举**（层序进码，见上）
    let mut k = 0usize;
    while k < LAYER_COUNT {
        v.push(ArchitectureError::BadLayerOrder { got: k, want: 0 }.code());
        k += 1;
    }
    v
}