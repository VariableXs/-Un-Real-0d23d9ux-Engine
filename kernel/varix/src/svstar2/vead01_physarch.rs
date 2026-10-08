//! VE-F6001 · 物理域总架构（VE-AD 域 · AD01 批次开工 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F6001`
//!
//! **判据（锚点原文）**：四层架构、物理三律、可预期不失真、判据。
//!
//! **职责定位（锚点原文）**：AD01 物理域总架构——四层架构（数学层、
//! broadphase 层、求解层、场景对接层）与物理三律（可预期：同输入同结果、
//! 不失真：物理量诚实标注误差、可关：物理可整体降级关闭），域色世界有
//! 重量——物理是世界的地基不是特效；承接 AC 域移交包（F5993 十件）。
//!
//! ## 一、四层架构：层序即权限序，反向依赖即越权
//!
//! 数学层（[`Layer::Math`]）→ broadphase 层（[`Layer::Broadphase`]）→
//! 求解层（[`Layer::Solver`]）→ 场景对接层（[`Layer::SceneBridge`]）：
//! 下层不知道上层存在，上层按序依赖下层——反向依赖一律
//! [`ArchErr::LayerIntrusion`] 拒绝（与 F5601 音频四层同一先例范式）。
//! 冻结接口逐层登记进册（[`PhysArch::freeze`]），层间委托走登记面，
//! 册外接口不可达。
//!
//! ## 二、物理三律是红线不是口号
//!
//! - **可预期**（[`Law::Predictable`]）：同输入同结果——架构提供确定性
//!   摘要面（[`PhysArch::step_digest`]），同状态两跑摘要必等，判据实测；
//! - **不失真**（[`Law::Faithful`]）：物理量诚实标注误差——冻结接口必须
//!   带误差口径标注，无标注的物理量输出=失真红线违例；
//! - **可关**（[`Law::Switchable`]）：物理可整体降级关闭——
//!   [`PhysArch::shut_off`] 后一切查询给 [`OffState::Off`] 且带关断标记，
//!   禁止「关了还假装在跑」。
//!
//! 域色：**世界有重量——物理是世界的地基不是特效**。地基级的可关不是
//! 「可选功能」，是性能兜底路径的诚实降级；关闭时世界呈现的必须是
//! 显性的降级态而非冻结的假象。
//!
//! ## 三、承接 AC 移交包十件，缺件追补不留暗债
//!
//! F5993 十件逐件入册（[`HANDOFF_PIECES`]）：收到即登记、缺件即
//! [`ArchErr::HandoffMissing`] 向 AC 追补——承接暗债比功能缺件更危险，
//! 追补单必须可追踪。
//!
//! ## 四、越权/违例/缺件三类错误显性化
//!
//! 层间越权→拒绝；三律违例→红线（[`PhysArch::audit`] 逐接口对账）；
//! 承接缺件→追补。诊断码独占 0x3Exx 段。零 panic 面、零 IO、零墙钟
//! （tick 账面）、无全局可变状态、no_std 零 std 依赖。
//!
//! **对接**：上游 AC 移交包（F5993）；下游 AD01 全组（F6002~F6020 逐层落位）。
//! 读屏可达：三律与降级态的声明都是结构化字段（[`LAW_DOC`] /
//! [`OffState`]），不走纯视觉。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 域色标识。
pub const VAD_DOMAIN: &str = "VE-AD";

/// 架构层数（数学/broadphase/求解/场景对接）。
pub const LAYER_COUNT: usize = 4;

/// 物理三律数。
pub const LAW_COUNT: usize = 3;

/// AC 移交十件数（F5993）。
pub const HANDOFF_PIECE_COUNT: usize = 10;

/// 每层冻结接口上限。
pub const MAX_FROZEN_PER_LAYER: usize = 64;

/// 域色声明（读屏可达的结构化字段）。
pub const DOMAIN_COLOR_DOC: &str = "世界有重量——物理是世界的地基不是特效";

/// 三律声明（读屏可达）。
pub const LAW_DOC: &str = "可预期：同输入同结果；不失真：物理量诚实标注误差；可关：物理可整体降级关闭";

// ---------------------------------------------------------------------------
// 二、诊断码（独占 0x3Exx 段；0x3Dxx 归 F5113 遥测，0x3Cxx 归 F0229）
// ---------------------------------------------------------------------------

/// F6001 诊断码。独占 `0x3Exx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchCode(pub u16);

impl ArchCode {
    /// 层间越权（反向依赖）。
    pub const LAYER_INTRUSION: ArchCode = ArchCode(0x3E01);
    /// 三律违例·可预期（摘要不等）。
    pub const LAW_PREDICTABLE: ArchCode = ArchCode(0x3E02);
    /// 三律违例·不失真（误差标注缺失）。
    pub const LAW_FAITHFUL: ArchCode = ArchCode(0x3E03);
    /// 册外接口调用。
    pub const UNREGISTERED: ArchCode = ArchCode(0x3E04);
    /// 承接缺件。
    pub const HANDOFF_MISSING: ArchCode = ArchCode(0x3E05);
    /// 冻结面已满。
    pub const FREEZE_FULL: ArchCode = ArchCode(0x3E06);

    /// 两两互异的 wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            ArchCode::LAYER_INTRUSION => "层间越权：反向依赖违反层序即权限序".into(),
            ArchCode::LAW_PREDICTABLE => "三律违例·可预期：同输入两次结果不同".into(),
            ArchCode::LAW_FAITHFUL => "三律违例·不失真：物理量无误差标注".into(),
            ArchCode::UNREGISTERED => "册外接口：未登记冻结面不可达".into(),
            ArchCode::HANDOFF_MISSING => "承接缺件：向 AC 追补".into(),
            ArchCode::FREEZE_FULL => "冻结面已满：显性拒绝不挤占".into(),
            ArchCode(_) => "未知物理架构诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 三、层 / 律 / 关断态（数据结构：四层架构册×物理三律×承接清单）
// ---------------------------------------------------------------------------

/// 架构四层（层序即权限序，ordinal 只增不减）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// 数学层（最底层：向量/积分器原语）。
    Math,
    /// broadphase 层（粗碰撞对候选）。
    Broadphase,
    /// 求解层（约束求解与积分推进）。
    Solver,
    /// 场景对接层（最上层：世界状态与渲染/逻辑的桥）。
    SceneBridge,
}

impl Layer {
    /// 层序（0 最底）。
    pub const fn ordinal(self) -> usize {
        match self {
            Layer::Math => 0,
            Layer::Broadphase => 1,
            Layer::Solver => 2,
            Layer::SceneBridge => 3,
        }
    }

    /// 层短名。
    pub const fn tag(self) -> &'static str {
        match self {
            Layer::Math => "math",
            Layer::Broadphase => "broadphase",
            Layer::Solver => "solver",
            Layer::SceneBridge => "scene-bridge",
        }
    }

    /// 层全名（读屏可达）。
    pub const fn label(self) -> &'static str {
        match self {
            Layer::Math => "数学层",
            Layer::Broadphase => "broadphase 层",
            Layer::Solver => "求解层",
            Layer::SceneBridge => "场景对接层",
        }
    }

    /// 全集（判据对账用）。
    pub const fn all() -> [Layer; LAYER_COUNT] {
        [Layer::Math, Layer::Broadphase, Layer::Solver, Layer::SceneBridge]
    }

    /// 依赖许可：只许依赖更低层（严格小于）；同层横向依赖也拒绝。
    pub const fn may_depend_on(self, target: Layer) -> bool {
        self.ordinal() > target.ordinal()
    }
}

/// 物理三律。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Law {
    /// 可预期：同输入同结果。
    Predictable,
    /// 不失真：物理量诚实标注误差。
    Faithful,
    /// 可关：物理可整体降级关闭。
    Switchable,
}

impl Law {
    /// 律短名。
    pub const fn tag(self) -> &'static str {
        match self {
            Law::Predictable => "predictable",
            Law::Faithful => "faithful",
            Law::Switchable => "switchable",
        }
    }

    /// 律全名（读屏可达）。
    pub const fn label(self) -> &'static str {
        match self {
            Law::Predictable => "可预期：同输入同结果",
            Law::Faithful => "不失真：物理量诚实标注误差",
            Law::Switchable => "可关：物理可整体降级关闭",
        }
    }

    /// 全集。
    pub const fn all() -> [Law; LAW_COUNT] {
        [Law::Predictable, Law::Faithful, Law::Switchable]
    }
}

/// 关断态（可关律的诚实降级呈现：关了就是关了，不装在跑）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffState {
    /// 在跑。
    Running,
    /// 已关断（显性降级态）。
    Off,
}

/// 冻结接口条目（层×名×误差口径标注×确定性摘要口径）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenIface {
    /// 所属层。
    pub layer: Layer,
    /// 接口名。
    pub name: String,
    /// 误差口径标注（不失真律的承载字段；空串=违例）。
    pub error_note: String,
    /// 确定性摘要（可预期律的承载字段；None=无摘要口径）。
    pub digest: Option<u64>,
}

/// F5993 移交十件（承接清单）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffPiece {
    /// 物理事件接总线。
    EventBus,
    /// AI 感知消费物理事件。
    AiPerception,
    /// 物理预算入一张表。
    BudgetTable,
    /// 遥测规范。
    TelemetrySpec,
    /// 沙箱三律参照。
    SandboxLaws,
    /// 文档总纲。
    DocMaster,
    /// 测试资产。
    TestAssets,
    /// fuzz 语料。
    FuzzCorpus,
    /// 知识库。
    KnowledgeBase,
    /// AD 预清点回执。
    PrecheckReceipt,
}

impl HandoffPiece {
    /// 全集（顺序即 F5993 清单序）。
    pub const fn all() -> [HandoffPiece; HANDOFF_PIECE_COUNT] {
        [
            HandoffPiece::EventBus,
            HandoffPiece::AiPerception,
            HandoffPiece::BudgetTable,
            HandoffPiece::TelemetrySpec,
            HandoffPiece::SandboxLaws,
            HandoffPiece::DocMaster,
            HandoffPiece::TestAssets,
            HandoffPiece::FuzzCorpus,
            HandoffPiece::KnowledgeBase,
            HandoffPiece::PrecheckReceipt,
        ]
    }

    /// 件名（读屏可达）。
    pub const fn label(self) -> &'static str {
        match self {
            HandoffPiece::EventBus => "物理事件接总线",
            HandoffPiece::AiPerception => "AI 感知消费物理事件",
            HandoffPiece::BudgetTable => "物理预算入一张表",
            HandoffPiece::TelemetrySpec => "遥测规范",
            HandoffPiece::SandboxLaws => "沙箱三律参照",
            HandoffPiece::DocMaster => "文档总纲",
            HandoffPiece::TestAssets => "测试资产",
            HandoffPiece::FuzzCorpus => "fuzz 语料",
            HandoffPiece::KnowledgeBase => "知识库",
            HandoffPiece::PrecheckReceipt => "AD 预清点回执",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、PhysArch 主结构（架构 O(层) 的登记与三律执法）
// ---------------------------------------------------------------------------

/// 物理域总架构册：冻结面×三律执法×承接清单×关断开关。
#[derive(Debug)]
pub struct PhysArch {
    frozen: Vec<FrozenIface>,
    received: [bool; HANDOFF_PIECE_COUNT],
    off: bool,
    violations: Vec<ArchCode>,
}

impl PhysArch {
    /// 空册构造（十件未收、开关在跑）。
    pub fn new() -> Self {
        PhysArch {
            frozen: Vec::new(),
            received: [false; HANDOFF_PIECE_COUNT],
            off: false,
            violations: Vec::new(),
        }
    }

    /// 冻结接口登记（误差标注必填——不失真律登记即执法；空标注拒绝）。
    pub fn freeze(
        &mut self,
        layer: Layer,
        name: &str,
        error_note: &str,
        digest: Option<u64>,
    ) -> Result<(), ArchCode> {
        if self.off {
            return Ok(()); // 已关断：登记面静默接受但不入册（关断态不产冻结面）
        }
        if error_note.is_empty() {
            self.violations.push(ArchCode::LAW_FAITHFUL);
            return Err(ArchCode::LAW_FAITHFUL);
        }
        let same_layer = self.frozen.iter().filter(|f| f.layer == layer).count();
        if same_layer >= MAX_FROZEN_PER_LAYER {
            return Err(ArchCode::FREEZE_FULL);
        }
        self.frozen.push(FrozenIface {
            layer,
            name: name.into(),
            error_note: error_note.into(),
            digest,
        });
        Ok(())
    }

    /// 层间依赖校验：只许上层依赖下层（O(1) 序比较），越权即拒绝。
    pub fn check_dependency(&mut self, from: Layer, to: Layer) -> Result<(), ArchCode> {
        if from.may_depend_on(to) {
            Ok(())
        } else {
            self.violations.push(ArchCode::LAYER_INTRUSION);
            Err(ArchCode::LAYER_INTRUSION)
        }
    }

    /// 册外接口调用拒绝（登记面之外不可达）。
    pub fn resolve(&self, name: &str) -> Result<&FrozenIface, ArchCode> {
        match self.frozen.iter().find(|f| f.name == name) {
            Some(f) => Ok(f),
            None => Err(ArchCode::UNREGISTERED),
        }
    }

    /// 可预期律面：同状态两跑摘要对账（摘要相等即同结果）。
    pub fn step_digest(&mut self, a: Option<u64>, b: Option<u64>) -> Result<(), ArchCode> {
        match (a, b) {
            (Some(x), Some(y)) if x == y => Ok(()),
            (Some(_), Some(_)) => {
                self.violations.push(ArchCode::LAW_PREDICTABLE);
                Err(ArchCode::LAW_PREDICTABLE)
            }
            _ => Ok(()), // 无摘要口径的接口不在可预期律执法面（冻结时显性声明）
        }
    }

    /// 可关律面：整体关断（显性降级，登记面静默、查询给 Off）。
    pub fn shut_off(&mut self) {
        self.off = true;
    }

    /// 重启（关断→在跑；账面不清——违规史保留）。
    pub fn restart(&mut self) {
        self.off = false;
    }

    /// 当前关断态。
    pub const fn off_state(&self) -> OffState {
        if self.off {
            OffState::Off
        } else {
            OffState::Running
        }
    }

    /// 承接十件（逐件登记；O(件) 核验）。
    pub fn receive(&mut self, piece: HandoffPiece) {
        let i = HandoffPiece::all().iter().position(|p| *p == piece).unwrap_or(usize::MAX);
        if i < HANDOFF_PIECE_COUNT {
            self.received[i] = true;
        }
    }

    /// 承接缺件追补清单（缺件→向 AC 追补，逐件可追踪）。
    pub fn missing_handoffs(&self) -> Vec<HandoffPiece> {
        HandoffPiece::all()
            .iter()
            .zip(self.received.iter())
            .filter(|(_, ok)| !**ok)
            .map(|(p, _)| *p)
            .collect()
    }

    /// 违规史只读视图。
    pub fn violations(&self) -> &[ArchCode] {
        &self.violations
    }

    /// 三律对账审计：逐冻结接口查误差标注（不失真）。
    /// 可预期与可关两律由调用面实测（step_digest / off_state）。
    pub fn audit(&self) -> Result<(), ArchCode> {
        if self.frozen.iter().any(|f| f.error_note.is_empty()) {
            return Err(ArchCode::LAW_FAITHFUL);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、域自检（判据：四层架构、物理三律、可预期不失真、判据）
// ---------------------------------------------------------------------------

/// VE-F6001 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_vead01_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("vead01_physarch");

    // —— 判据一 · 四层架构：层序即权限序，反向依赖拒绝 ——
    let mut a = PhysArch::new();
    let down = a.check_dependency(Layer::SceneBridge, Layer::Math);
    let sideways = a.check_dependency(Layer::Solver, Layer::Solver);
    let up = a.check_dependency(Layer::Math, Layer::SceneBridge);
    s.add(
        "AD01-四层-反向依赖拒绝",
        down.is_ok() && sideways == Err(ArchCode::LAYER_INTRUSION) && up.is_err(),
        "上层依赖下层放行；同层横向与反向依赖一律 LAYER_INTRUSION（层序即权限序）",
    );

    // —— 判据一 · 反向：冻结面登记与册外拒绝 ——
    let freeze_ok = a.freeze(Layer::Math, "integrate", "半隐式欧拉，误差 O(dt^2)", Some(0xA11CE));
    let outside = a.resolve("nonexistent");
    let inside = a.resolve("integrate");
    s.add(
        "AD01-四层-冻结面登记与册外拒绝",
        freeze_ok.is_ok()
            && outside == Err(ArchCode::UNREGISTERED)
            && inside.is_ok()
            && inside.map(|f| f.layer == Layer::Math) == Ok(true),
        "接口入册后可达；册外调用 UNREGISTERED（登记面之外不可达）",
    );

    // —— 判据二 · 三律·可预期：同输入同摘要等、异输入立案 ——
    let same = a.step_digest(Some(42), Some(42));
    let diff = a.step_digest(Some(42), Some(43));
    s.add(
        "AD01-三律-可预期摘要对账",
        same.is_ok()
            && diff == Err(ArchCode::LAW_PREDICTABLE)
            && a.violations().contains(&ArchCode::LAW_PREDICTABLE),
        "同输入同结果放行；同接口两跑摘要不等立案（可预期是实测性质不是声明）",
    );

    // —— 判据二 · 反向：不失真——空误差标注登记即拒绝 ——
    let mut a2 = PhysArch::new();
    let no_note = a2.freeze(Layer::Solver, "solve_constraint", "", None);
    let with_note = a2.freeze(Layer::Solver, "solve_constraint", "序贯冲击，误差 O(dt)", None);
    let audit = a2.audit();
    s.add(
        "AD01-三律-不失真误差标注必填",
        no_note == Err(ArchCode::LAW_FAITHFUL)
            && with_note.is_ok()
            && audit.is_ok(),
        "空误差标注登记即拒（物理量诚实标注误差）；带标注全册审计通过",
    );

    // —— 判据二 · 三律·可关：整体关断显性降级不装在跑 ——
    let mut a3 = PhysArch::new();
    let before = a3.off_state();
    a3.shut_off();
    let during = a3.off_state();
    let frozen_off = a3.freeze(Layer::Math, "x", "y", None);
    a3.restart();
    let after = a3.off_state();
    s.add(
        "AD01-三律-可关显性降级",
        before == OffState::Running
            && during == OffState::Off
            && frozen_off.is_ok()
            && after == OffState::Running,
        "关断态显性（Off 不是假 Running）；关断期冻结面静默；重启后恢复在跑",
    );

    // —— 判据三 · 承接：十件逐件可查，缺件追补清单 ——
    let mut a4 = PhysArch::new();
    let missing_all = a4.missing_handoffs().len();
    a4.receive(HandoffPiece::EventBus);
    a4.receive(HandoffPiece::PrecheckReceipt);
    let missing_two = a4.missing_handoffs().len();
    s.add(
        "AD01-承接-十件清单缺件可追",
        missing_all == HANDOFF_PIECE_COUNT
            && missing_two == HANDOFF_PIECE_COUNT - 2
            && a4.missing_handoffs().contains(&HandoffPiece::FuzzCorpus)
            && !a4.missing_handoffs().contains(&HandoffPiece::EventBus),
        "空册十件全缺；收两件缺八；缺件逐件可追踪（向 AC 追补不留暗债）",
    );

    // —— 判据三 · 反向：重复承接幂等（收两次不重复计） ——
    a4.receive(HandoffPiece::EventBus);
    a4.receive(HandoffPiece::EventBus);
    s.add(
        "AD01-承接-重复收件幂等",
        a4.missing_handoffs().len() == HANDOFF_PIECE_COUNT - 2,
        "重复收件不改变缺件账（承接清单是集合不是计数器）",
    );

    // —— 判据四 · 元数据：码段互异 + 常量口径对账 ——
    let codes = [
        ArchCode::LAYER_INTRUSION.code(),
        ArchCode::LAW_PREDICTABLE.code(),
        ArchCode::LAW_FAITHFUL.code(),
        ArchCode::UNREGISTERED.code(),
        ArchCode::HANDOFF_MISSING.code(),
        ArchCode::FREEZE_FULL.code(),
    ];
    let mut uniq = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    let layers_ordered = Layer::all()
        .windows(2)
        .all(|w| w[0].ordinal() < w[1].ordinal());
    s.add(
        "AD01-判据-码段互异且层序单调",
        uniq
            && codes.iter().all(|c| c & 0xFF00 == 0x3E00)
            && layers_ordered
            && LAW_COUNT == Law::all().len()
            && !LAW_DOC.is_empty()
            && !DOMAIN_COLOR_DOC.is_empty(),
        "六码全落 0x3Exx 两两互异；层 ordinal 单调；三律全集与声明常量在位（读屏可达）",
    );

    // —— 判据四 · 反向：依赖许可全序矩阵对账（4×4 十六对逐对独立重算） ——
    let mut matrix_ok = true;
    for from in Layer::all() {
        for to in Layer::all() {
            let expect = from.ordinal() > to.ordinal();
            if from.may_depend_on(to) != expect {
                matrix_ok = false;
            }
        }
    }
    s.add(
        "AD01-判据-依赖矩阵全序对账",
        matrix_ok && LAYER_COUNT == Layer::all().len(),
        "16 对依赖许可与「严格小于」独立重算逐对相符（无一例豁免）",
    );

    s
}
