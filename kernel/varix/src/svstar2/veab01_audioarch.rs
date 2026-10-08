//! VE-AB01 · AB 域开工与音频总架构（VE-F5601）。
//!
//! 锚点原文：「四层架构（设备层、混音层、空间层、内容层）与音频三律
//! （不炸耳、可静音、诚实音量），域色听得见的临场感——声音为体验服务
//! 不为炫技服务；承接 AA 域移交包（F5593 十件）；四层各出接口冻结清单
//! ——层间契约白纸黑字；三律违例的处置流程与升级路径一并公示。」
//!
//! 错误路径与降级矩阵（锚点原文）：层间越权→拒绝；三律违例→红线；
//! 承接缺件→向 AA 域追补。性能逐项分解：架构 O（层）。
//!
//! 无障碍与隐私（锚点原文）：三律声明读屏可达。
//!
//! ## 与 F4601（VE-W 域开工先例）对齐
//!
//! 同为「域开工总架构」，故沿用其落位纪律：
//! - 枚举层 + 稳定短名 `tag()` + 中文名 `label()`（诊断三要素用）；
//! - 错误账本三元组（主体/ 码 / 说明），码位不复用；
//! - 「层序即权限序」：反向依赖即越权，越权必拒（不留静默兜底）。
//!
//! ## 本域特有语义
//!
//! - **音频三律是红线不是目标**：任一律违例即 `RedLine`，不可降级、
//!   不可静音了事——「静音掉」不等于「合规」，见 [`Precedent::MutedOut`]
//!   明确被[`AudioLaw::violations`] 计为违例。
//! - **承接追补**：AA 域 F5593 未交付时，本域不假装已承接，而是产出
//!   [`ChaseRequest`] 逐件追补（锚点「承接缺件→向 AA 域追补」）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 判据在本域的 `veab01_audioarch_checks` 模块（沿 F4601 先例：
// 主模块只放纯功能，判据独立成文件；本模块因此不 import `CheckSet`，
// 真仓no_std 下留空 import 会报 unused 警告）。

/// 域标识（机检与审计用）。
pub const VAB_DOMAIN: &str = "VE-AB";

/// 音频域层数（锚点「架构 O（层）」的驱动面）。
pub const LAYER_COUNT: usize = 4;

/// AA 域移交件数（锚点 F5593「AB 域承接十件」）。
pub const HANDOFF_PIECE_COUNT: usize = 10;

/// 每层可冻结接口数上界（防无限登记；超界显性拒绝）。
pub const MAX_FROZEN_PER_LAYER: usize = 64;

/// 三律条数（不炸耳 / 可静音 / 诚实音量）。
pub const LAW_COUNT: usize = 3;

/// 层冻结清单文档（锚点「四层各出接口冻结清单——层间契约白纸黑字」）。
pub const LAYER_FREEZE_DOC: &str = "\
音频域四层架构 —— 每层一张接口冻结清单，层间契约白纸黑字：
  设备层 (device)：设备枚举 / 热切换 / 默认设备跟随。冻结面=对下暴露的设备态。
  混音层 (mix)：总线树 / 主总线兜底 / 路由表。冻结面 = 对上承诺的混音语义。
  空间层 (spatial)：听者与声源抽象 / 空间开销预算。冻结面 = 对上承诺的定位语义。
  内容层 (content)：资产管线 / 音乐与语音状态机。冻结面 = 对上承诺的播放语义。
依赖方向单向向下（同层可自环，禁止反向）；越权依赖一律拒绝，不静默改道。";

/// 音频三律释义文档（无障碍：三律声明读屏可达 —— 声明本身即朗读文本）。
pub const THREE_LAWS_DOC: &str = "\
音频三律（声音为体验服务，不为炫技服务）：
  第一律 不炸耳：任何输出不得造成突发高音量损伤听力；音量上限按输出设备分档。
  第二律 可静音：用户静音、场景静音、故障静音三条路径都必须真的静音。
  第三律 诚实音量：界面显示的音量数字必须与实际输出一致，不做虚标。
任一律违例即红线，不得以「静音掉」「钳制到阈值内」充抵。";

// ---------------------------------------------------------------------------
// 一、四层架构（设备 / 混音 / 空间 / 内容）
// ---------------------------------------------------------------------------

/// 音频域四层。
///
/// **层序即权限序**：`ordinal()` 越大权限越高，依赖只能向下或同层，
/// 跨层向上即 [`Layer::may_depend_on`] 判否。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// 设备层：与硬件输出打交道的最底层。
    Device = 0,
    /// 混音层：总线树与路由。
    Mix = 1,
    /// 空间层：听者与声源的空间定位。
    Spatial = 2,
    /// 内容层：资产、音乐与语音的播放语义（最高层，面向体验）。
    Content = 3,
}

impl Layer {
    /// 层序（0..3）。
    pub fn ordinal(self) -> usize {
        self as usize
    }

    /// 稳定短名（机检与审计用）。
    pub fn tag(self) -> &'static str {
        match self {
            Layer::Device => "device",
            Layer::Mix => "mix",
            Layer::Spatial => "spatial",
            Layer::Content => "content",
        }
    }

    /// 层的中文名（诊断三要素用）。
    pub fn label(self) -> &'static str {
        match self {
            Layer::Device => "设备层",
            Layer::Mix => "混音层",
            Layer::Spatial => "空间层",
            Layer::Content => "内容层",
        }
    }

    /// 四层穷举（按层序，架构 O(层数) 的驱动面）。
    pub fn all() -> [Layer; LAYER_COUNT] {
        [Layer::Device, Layer::Mix, Layer::Spatial, Layer::Content]
    }

    /// 层间依赖是否合法：`self` 能否依赖 `target`。
    ///
    /// **层序即权限序**：只能向下（ordinal 不增）或同层（自环）。
    /// 向上依赖即越权——上层不该知道下层的存在，混音层去问设备层
    /// 「当前输出设备是哪个」就是越权（应由设备层推送，而非反向拉取）。
    pub fn may_depend_on(self, target: Layer) -> bool {
        self.ordinal() >= target.ordinal()
    }
}

/// 一条冻结接口（某层对外承诺的稳定面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenIface {
    /// 接口稳定标识（层内唯一）。
    pub id: u32,
    /// 归属层。
    pub owner: Layer,
    /// 冻结版本（只升不改：语义变更走冻结流程升版）。
    pub version: u16,
}

/// 层间越权诊断（锚点「层间越权→拒绝」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerMismatch {
    /// 依赖方层（发起越权依赖的一方）。
    pub from: Layer,
    /// 被依赖方层。
    pub to: Layer,
    /// 越权的接口 id。
    pub iface: u32,
    /// 三要素之一：超了什么（层序差）。
    pub exceeded: String,
    /// 三要素之二：为什么（层序即权限序，反向依赖即越权）。
    pub because: &'static str,
    /// 三要素之三：怎么办。
    pub remedy: &'static str,
}

// ---------------------------------------------------------------------------
// 二、音频三律（不炸耳 / 可静音 / 诚实音量）
// ---------------------------------------------------------------------------

/// 音频三律之一。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioLaw {
    /// 不炸耳：音量上限按输出设备分档，超限即钳制。
    NoBlast,
    /// 可静音：三条静音路径都必须真的静音。
    Mutable,
    /// 诚实音量：显示与实际输出一致，不虚标。
    HonestVolume,
}

impl AudioLaw {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            AudioLaw::NoBlast => "no-blast",
            AudioLaw::Mutable => "mutable",
            AudioLaw::HonestVolume => "honest-volume",
        }
    }

    /// 律的中文名（读屏可达的声明文本）。
    pub fn label(self) -> &'static str {
        match self {
            AudioLaw::NoBlast => "不炸耳",
            AudioLaw::Mutable => "可静音",
            AudioLaw::HonestVolume => "诚实音量",
        }
    }

    /// 三律穷举（按声明顺序，朗读顺序即此序）。
    pub fn all() -> [AudioLaw; LAW_COUNT] {
        [AudioLaw::NoBlast, AudioLaw::Mutable, AudioLaw::HonestVolume]
    }

    /// 该律的码位（判据侧可独立重算此映射）。
    pub fn code(self) -> u8 {
        match self {
            AudioLaw::NoBlast => 0,
            AudioLaw::Mutable => 1,
            AudioLaw::HonestVolume => 2,
        }
    }

    /// 由码位反查（未登记码位返回 `None`，不猜）。
    pub fn of_code(c: u8) -> Option<AudioLaw> {
        match c {
            0 => Some(AudioLaw::NoBlast),
            1 => Some(AudioLaw::Mutable),
            2 => Some(AudioLaw::HonestVolume),
            _ => None,
        }
    }
}

/// 违例的先例情形（**决定处置强度**，不是自由裁量）。
///
/// 锚点「三律违例→红线」：一旦成立即红线，处置流程与升级路径一并公示。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precedent {
    /// 未钳制到设备档位上限。
    OverLimit,
    /// 界面音量虚标（显示与实际不一致）。
    Mislabeled,
    /// 静音路径未真静音（仍出声）。
    MutedOut,
    /// 三律声明对读屏不可达。
    UnreachableByScreenReader,
    /// 无法确定所属律（**仍计违例**，不因归类失败而放行）。
    Unclassified,
}

impl Precedent {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            Precedent::OverLimit => "over-limit",
            Precedent::Mislabeled => "mislabeled",
            Precedent::MutedOut => "muted-out",
            Precedent::UnreachableByScreenReader => "unreachable-by-screen-reader",
            Precedent::Unclassified => "unclassified",
        }
    }

    /// 该先例归属哪一律（`Unclassified` 自行归属自己，不假托他人）。
    pub fn law(self) -> AudioLaw {
        match self {
            Precedent::OverLimit => AudioLaw::NoBlast,
            Precedent::Mislabeled => AudioLaw::HonestVolume,
            Precedent::MutedOut => AudioLaw::Mutable,
            Precedent::UnreachableByScreenReader => AudioLaw::Mutable,
            Precedent::Unclassified => AudioLaw::Mutable,
        }
    }

    /// 升级路径（锚点「升级路径一并公示」）：一线处置 → 二线封停 → 三线根因回溯。
    pub fn escalation(self) -> EscalationStage {
        match self {
            Precedent::OverLimit => EscalationStage::ClampNow,
            Precedent::Mislabeled => EscalationStage::SuspendFeature,
            Precedent::MutedOut | Precedent::UnreachableByScreenReader => {
                EscalationStage::HaltAndTrace
            }
            // 归类失败本身是流程缺陷，直接走三线回溯，
            // **不因"不知道算哪条律"而降级处置**。
            Precedent::Unclassified => EscalationStage::HaltAndTrace,
        }
    }

    /// 该先例是否可降级处置（**三律一律不可降级**，此恒为 false，作判据锚点）。
    pub fn degradable(self) -> bool {
        false
    }
}

/// 红线升级阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscalationStage {
    /// 一线：立即钳制到设备档位上限。
    ClampNow,
    /// 二线：封停涉事功能。
    SuspendFeature,
    /// 三线：回溯根因并公示。
    HaltAndTrace,
}

impl EscalationStage {
    pub fn tag(self) -> &'static str {
        match self {
            EscalationStage::ClampNow => "clamp-now",
            EscalationStage::SuspendFeature => "suspend-feature",
            EscalationStage::HaltAndTrace => "halt-and-trace",
        }
    }
}

/// 一条三律违例（红线）。
#[derive(Clone, Debug, PartialEq)]
pub struct RedLine {
    /// 归属律。
    pub law: AudioLaw,
    /// 先例情形（决定升级路径）。
    pub precedent: Precedent,
    /// 三要素之一：超了什么（定量描述）。
    pub exceeded: String,
    /// 三要素之二：为什么。
    pub because: String,
    /// 三要素之三：怎么办（与 [`Precedent::escalation`] 一致；固定文案故为静态串）。
    pub remedy: &'static str,
    /// 升级阶段（由先例推导，不自由填写）。
    pub stage: EscalationStage,
}

/// 三律台账（声明侧 + 违例侧）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioLaws {
    /// 已公示声明的律（读屏可达的文本）。
    pub declared: Vec<AudioLaw>,
    /// 红线违例台账。
    pub violations: Vec<RedLine>,
}

impl AudioLaws {
    /// 公示一条律的声明（重复公示同一律即重复声明，不叠加）。
    ///
    /// **不可达即当场落红线**，不只返回 `Err`——否则调用方忽略返回值时
    /// 违例会凭空消失，「异常零静默」在这里就得靠返回值自律，不可靠。
    pub fn declare(&mut self, law: AudioLaw, reachable: bool) -> Result<(), &'static str> {
        if !self.declared.contains(&law) {
            self.declared.push(law);
        }
        if !reachable {
            // 不可达即违例（读屏可达是锚点明写的要求）。
            // 同一条声明重复登记不可达时**不重复记红线**（否则调用方重试
            // 会把一条缺陷刷成多条，红线台账不再是事实的镜像）。
            if !self
                .violations
                .iter()
                .any(|v| v.precedent == Precedent::UnreachableByScreenReader && v.law == law)
            {
                let stage = Precedent::UnreachableByScreenReader.escalation();
                self.violations.push(RedLine {
                    law,
                    precedent: Precedent::UnreachableByScreenReader,
                    exceeded: format!("律「{}」的声明对读屏不可达", law.label()),
                    because: format!(
                        "违「{}」律（{}）：{}",
                        law.label(),
                        law.tag(),
                        Precedent::UnreachableByScreenReader.tag()
                    ),
                    remedy: match stage {
                        EscalationStage::ClampNow => "立即钳制到该输出设备档位上限并公示处置",
                        EscalationStage::SuspendFeature => "封停涉事功能，修至诚实上报后复启",
                        EscalationStage::HaltAndTrace => "立即停声并回溯根因，公示后复声",
                    },
                    stage,
                });
            }
            return Err("E_LAW_UNREACHABLE");
        }
        Ok(())
    }

    /// 三律是否全部公示（缺任一即未公示）。
    pub fn fully_declared(&self) -> bool {
        AudioLaw::all().iter().all(|l| self.declared.contains(l))
    }

    /// 按律计违例（判据侧独立重算此表即可对账）。
    pub fn count_of(&self, law: AudioLaw) -> usize {
        self.violations.iter().filter(|v| v.law == law).count()
    }

    /// 违例总数。
    pub fn total_violations(&self) -> usize {
        self.violations.len()
    }

    /// 是否清白（无红线）。
    pub fn clean(&self) -> bool {
        self.violations.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 三、承接 AA 域移交包（F5593 十件）
// ---------------------------------------------------------------------------

/// AA 域应移交的十件之一（锚点 F5593 原文列举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffPiece {
    /// 音频事件总线接口。
    EventBus,
    /// 语音网络通道衔接。
    VoiceChannel,
    /// 听者挂场景图契约。
    ListenerBinding,
    /// 预算口径。
    BudgetSpec,
    /// 遥测规范。
    TelemetrySpec,
    /// 安全三律参照。
    SafetyLawRef,
    /// 文档总纲。
    DocOutline,
    /// 测试资产。
    TestAssets,
    /// fuzz 语料。
    FuzzCorpus,
    /// 知识库。
    KnowledgeBase,
}

impl HandoffPiece {
    /// 稳定短名（审计对账用）。
    pub fn tag(self) -> &'static str {
        match self {
            HandoffPiece::EventBus => "event-bus",
            HandoffPiece::VoiceChannel => "voice-channel",
            HandoffPiece::ListenerBinding => "listener-binding",
            HandoffPiece::BudgetSpec => "budget-spec",
            HandoffPiece::TelemetrySpec => "telemetry-spec",
            HandoffPiece::SafetyLawRef => "safety-law-ref",
            HandoffPiece::DocOutline => "doc-outline",
            HandoffPiece::TestAssets => "test-assets",
            HandoffPiece::FuzzCorpus => "fuzz-corpus",
            HandoffPiece::KnowledgeBase => "knowledge-base",
        }
    }

    /// 该件应落到哪一层（承接即入面，不得悬空）。
    ///
    /// **落位判据可独立重算**：按「该件约束的是哪一层的对外语义」定层，
    /// 判据侧用同一映射表即可对账，不问被测实现。
    pub fn target_layer(self) -> Layer {
        match self {
            // 事件总线是混音语义的一部分 → 混音层
            HandoffPiece::EventBus => Layer::Mix,
            // 语音通道衔接的是播放通路 → 内容层
            HandoffPiece::VoiceChannel => Layer::Content,
            // 听者挂场景图是空间定位契约 → 空间层
            HandoffPiece::ListenerBinding => Layer::Spatial,
            // 预算口径约束空间开销 → 空间层
            HandoffPiece::BudgetSpec => Layer::Spatial,
            // 遥测规范源自设备能力上报 → 设备层
            HandoffPiece::TelemetrySpec => Layer::Device,
            // 安全三律参照约束输出上限 → 设备层
            HandoffPiece::SafetyLawRef => Layer::Device,
            // 文档总纲不落单层（域级资产，落最高层以便全域可查）
            HandoffPiece::DocOutline => Layer::Content,
            // 测试资产与 fuzz 语料是域级验收物→ 内容层
            HandoffPiece::TestAssets => Layer::Content,
            HandoffPiece::FuzzCorpus => Layer::Content,
            HandoffPiece::KnowledgeBase => Layer::Content,
        }
    }

    /// 十件穷举（验收面：齐备即十件）。
    pub fn all() -> [HandoffPiece; HANDOFF_PIECE_COUNT] {
        [
            HandoffPiece::EventBus,
            HandoffPiece::VoiceChannel,
            HandoffPiece::ListenerBinding,
            HandoffPiece::BudgetSpec,
            HandoffPiece::TelemetrySpec,
            HandoffPiece::SafetyLawRef,
            HandoffPiece::DocOutline,
            HandoffPiece::TestAssets,
            HandoffPiece::FuzzCorpus,
            HandoffPiece::KnowledgeBase,
        ]
    }
}

/// 一件移交的承接记录（**已验收才算承接**，仅登记不算）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedPiece {
    /// 件。
    pub piece: HandoffPiece,
    /// 落到的层。
    pub layer: Layer,
    /// 是否通过交接面核验（F5593「每件怎么算合格写在清单上」）。
    pub verified: bool,
}

/// 一条追补请求（锚点「承接缺件→向 AA 域追补」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChaseRequest {
    /// 缺失件。
    pub piece: HandoffPiece,
    /// 应落到的层。
    pub layer: Layer,
    /// 三要素之一：缺什么。
    pub missing: String,
    /// 三要素之二：为什么（不追补则该层契约悬空）。
    pub because: &'static str,
    /// 三要素之三：怎么办。
    pub remedy: &'static str,
}

/// 承接台账（已承接 + 待追补）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HandoffLedger {
    /// 已承接且核验通过的件。
    pub accepted: Vec<AcceptedPiece>,
    /// 待追补的件。
    pub chasing: Vec<ChaseRequest>,
}

impl HandoffLedger {
    /// 登记承接（**核验未过不算承接**，直接转追补）。
    pub fn accept(&mut self, piece: HandoffPiece, verified: bool) {
        if !verified {
            self.chase(piece);
            return;
        }
        if self.accepted.iter().any(|a| a.piece == piece) {
            // 重复承接不叠加：同一件只登记一次（判据侧按集合比对）。
            return;
        }
        self.accepted.push(AcceptedPiece {
            piece,
            layer: piece.target_layer(),
            verified: true,
        });
    }

    /// 登记追补（重复追补合并为一条）。
    pub fn chase(&mut self, piece: HandoffPiece) {
        if self.chasing.iter().any(|c| c.piece == piece) {
            return;
        }
        self.chasing.push(ChaseRequest {
            piece,
            layer: piece.target_layer(),
            missing: format!("AA 域未交付「{}」件", piece.tag()),
            because: "该件未落位则对应层的对外契约悬空，层间契约无法白纸黑字",
            remedy: "向 AA 域（F5593）追补该件并附验收标准；补齐前该层接口不得冻结",
        });
    }

    /// 已承接件数。
    pub fn accepted_count(&self) -> usize {
        self.accepted.len()
    }

    /// 待追补件数。
    pub fn chasing_count(&self) -> usize {
        self.chasing.len()
    }

    /// 十件是否齐备（齐备即无追补项）。
    pub fn complete(&self) -> bool {
        self.accepted_count() == HANDOFF_PIECE_COUNT && self.chasing_count() == 0
    }
}

// ---------------------------------------------------------------------------
// 四、主架构编排（四层 + 三律 + 承接）
// ---------------------------------------------------------------------------

/// 域开工闸的结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateVerdict {
    /// 准予开工（无越权、无红线、接口冻结清单齐备）。
    Open,
    /// 暂缓（带阻断原因与待办）。
    Held,
}

impl GateVerdict {
    pub fn tag(self) -> &'static str {
        match self {
            GateVerdict::Open => "open",
            GateVerdict::Held => "held",
        }
    }
}

/// AB 域音频总架构（开工闸对象）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioArchitecture {
    /// 四层各自的冻结接口。
    pub frozen: Vec<FrozenIface>,
    /// 层间依赖声明（`from` 依赖 `to`）。
    pub deps: Vec<(Layer, Layer, u32)>,
    /// 越权诊断。
    pub mismatches: Vec<LayerMismatch>,
    /// 音频三律台账。
    pub laws: AudioLaws,
    /// 承接台账。
    pub handoff: HandoffLedger,
    /// 错误账本（主体 / 码 / 说明）。
    pub errors: Vec<(String, &'static str, String)>,
}

impl AudioArchitecture {
    /// 构造空架构（尚未开工）。
    pub fn new() -> Self {
        AudioArchitecture {
            frozen: Vec::new(),
            deps: Vec::new(),
            mismatches: Vec::new(),
            laws: AudioLaws::default(),
            handoff: HandoffLedger::default(),
            errors: Vec::new(),
        }
    }

    /// 冻结一条接口（层内 id 唯一；同 id 重复即越权重定义）。
    ///
    /// **前置闸**：该层若还有待追补件，则不得冻结（锚点「补齐前该层接口
    /// 不得冻结」）。这不是拒绝一切，而是把「悬空契约」挡在冻结面之外。
    pub fn freeze(&mut self, owner: Layer, id: u32, version: u16) -> Result<(), &'static str> {
        if self.frozen.iter().any(|f| f.owner == owner && f.id == id) {
            self.errors.push((
                owner.tag().to_string(),
                "E_IFACE_REDEFINE",
                format!(
                    "层 {} 接口 {} 已冻结，改语义须走冻结流程升版（不得原地改）",
                    owner.label(),
                    id
                ),
            ));
            return Err("E_IFACE_REDEFINE");
        }
        let per_layer = self.frozen.iter().filter(|f| f.owner == owner).count();
        if per_layer >= MAX_FROZEN_PER_LAYER {
            self.errors.push((
                owner.tag().to_string(),
                "E_IFACE_LIMIT",
                format!(
                    "层 {} 冻结接口数已达上界 {}，再登记须先升版或合并",
                    owner.label(),
                    MAX_FROZEN_PER_LAYER
                ),
            ));
            return Err("E_IFACE_LIMIT");
        }
        let pending = self.handoff.chasing.iter().any(|c| c.layer == owner);
        if pending {
            self.errors.push((
                owner.tag().to_string(),
                "E_IFACE_UNSETTLED_HANDOFF",
                format!(
                    "层 {} 尚有AA 域待追补件，契约未落位不得冻结（层间契约须白纸黑字）",
                    owner.label()
                ),
            ));
            return Err("E_IFACE_UNSETTLED_HANDOFF");
        }
        self.frozen.push(FrozenIface {
            id,
            owner,
            version,
        });
        Ok(())
    }

    /// 升版（冻结流程的唯一合法变更路径）。
    pub fn bump_version(&mut self, owner: Layer, id: u32) -> Result<u16, &'static str> {
        let slot = self
            .frozen
            .iter_mut()
            .find(|f| f.owner == owner && f.id == id)
            .ok_or("E_IFACE_ABSENT")?;
        slot.version = slot.version.saturating_add(1);
        Ok(slot.version)
    }

    /// 声明一条层间依赖（**越权即拒绝**，不静默改道）。
    pub fn declare_dep(&mut self, from: Layer, to: Layer, iface: u32) -> Result<(), &'static str> {
        if !from.may_depend_on(to) {
            let gap = from.ordinal().abs_diff(to.ordinal());
            self.mismatches.push(LayerMismatch {
                from,
                to,
                iface,
                exceeded: format!(
                    "层 {} (序 {}) 反向依赖层 {} (序 {})，跨了 {} 层",
                    from.label(),
                    from.ordinal(),
                    to.label(),
                    to.ordinal(),
                    gap
                ),
                because: "层序即权限序：上层不得反向拉取下层内部态，应由下层推送",
                remedy: "改为由下层向上推送状态事件；或把该依赖上移到合法的同层接口",
            });
            self.errors.push((
                from.tag().to_string(),
                "E_LAYER_OVERREACH",
                format!(
                    "层 {} 越权依赖层 {}（接口 {}）：层序 {} < 依赖序 {}",
                    from.label(),
                    to.label(),
                    iface,
                    from.ordinal(),
                    to.ordinal()
                ),
            ));
            return Err("E_LAYER_OVERREACH");
        }
        self.deps.push((from, to, iface));
        Ok(())
    }

    /// 公示三律（锚点「三律声明读屏可达」—— 不可达即违例）。
    pub fn declare_laws(&mut self, reachable: bool) -> Result<(), &'static str> {
        for law in AudioLaw::all() {
            if let Err(code) = self.laws.declare(law, reachable) {
                // 不可达即红线（读屏可达是锚点明写要求），量词写明「声明不可达」。
                self.raise(law, Precedent::UnreachableByScreenReader, "三律声明对读屏不可达");
                return Err(code);
            }
        }
        Ok(())
    }

    /// 登记一条三律违例（红线），升级阶段由先例推导。
    pub fn raise(&mut self, law: AudioLaw, precedent: Precedent, magnitude: &str) {
        let stage = precedent.escalation();
        self.laws.violations.push(RedLine {
            law,
            precedent,
            exceeded: magnitude.to_string(),
            because: format!(
                "违「{}」律（{}）：{}",
                law.label(),
                law.tag(),
                precedent.tag()
            ),
            remedy: match stage {
                EscalationStage::ClampNow => "立即钳制到该输出设备档位上限并公示处置",
                EscalationStage::SuspendFeature => "封停涉事功能，修至诚实上报后复启",
                EscalationStage::HaltAndTrace => "立即停声并回溯根因，公示后复声",
            },
            stage,
        });
    }

    /// 登记承接件。
    pub fn accept_piece(&mut self, piece: HandoffPiece, verified: bool) {
        self.handoff.accept(piece, verified);
    }

    /// 开工闸裁决（**架构 O(层)**：只按四层与三律的账面裁决，不做别的）。
    pub fn gate(&self) -> GateVerdict {
        let layers_ok = Layer::all()
            .iter()
            .all(|l| self.frozen.iter().any(|f| f.owner == *l));
        let open = layers_ok
            && self.mismatches.is_empty()
            && self.laws.clean()
            && self.laws.fully_declared()
            && self.handoff.complete();
        if open {
            GateVerdict::Open
        } else {
            GateVerdict::Held
        }
    }

    /// 某层是否已冻结（判据侧可独立重算：四层各有≥1 条）。
    pub fn layer_frozen(&self, l: Layer) -> bool {
        self.frozen.iter().any(|f| f.owner == l)
    }
}
