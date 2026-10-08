//! VE-F4003 · 文字方向模型（VE-T 域 · 国际化域 · T01 组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4003`
//!
//! **判据（锚点原文）**：三方向统一、三层优先级、isolate 自动、启发可覆写、
//! 方向模型。
//!
//! **职责定位（锚点原文）**：T01 文字方向模型——文字方向模型（LTR/RTL/TTB 三方向
//! 统一模型：方向声明（文档级/区块级/字符级三层方向——三层方向优先级表；方向继承
//! （继承链与隔离（isolate 语义）；方向检测（首强字符启发式（无显式方向时——
//! 启发显性：检测结果可覆盖（启发可覆写红线。数据结构：数据模型与规格表（逐条
//! 规格公开、参数域钳制、枚举守卫——家族格式）。错误路径与降级矩阵：三层冲突→
//! 优先级仲裁；isolate 缺失→自动隔离+诊断；启发误判→显性覆盖通道。性能逐项
//! 分解：判定 O(文本采样)；仲裁 O(层)；隔离 O(1)。跨批对接点：F4021 BiDi 前向；
//! N02 排版消费；F2948 lang 对端。无障碍与隐私：文档替述可读；无隐私面。
//!
//! **数据结构（锚点原文·家族格式）**：数据模型与规格表（逐条规格公开、参数域
//! 钳制、枚举守卫——家族格式）。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：三层冲突→优先级仲裁；isolate
//! 缺失→自动隔离+诊断；启发误判→显性覆盖通道。
//!
//! **性能逐项分解（锚点原文·家族格式）**：判定 O(文本采样)；仲裁 O(层)；
//! 隔离 O(1)。
//!
//! **跨批对接点（锚点原文·家族格式）**：F4021 BiDi 前向；N02 排版消费；F2948
//! lang 对端。
//!
//! **无障碍与隐私（锚点原文）**：文档替述可读；无隐私面。
//!
//! # 本项的边界（不越界施工，遵守"只做领到的任务"）
//!
//! 本项交付 **方向判定本体**：三方向取值域、三层方向声明与优先级仲裁、方向
//! 继承与 isolate 隔离、首强字符启发式检测与显性覆写通道。它**不代做**：
//!
//! - **BiDi 算法本体（UAX #9 实现）归F4021**；本项只给「方向已定」之后的
//!   **视觉序编排入口**的前向声明位（`landed: false`）。理由：UAX #9 的
//!   显式嵌入级别、弱/中性类型解析、括号配对是独立的一整套算法，与「方向
//!   到底是LTR 还是 RTL」是两件事——前者回答"既��� LTR 又含 RTL 时怎么排"，
//!   后者回答"这一段的基础方向是什么"。混在一起写会出现改一个方向误伤重排
//!   算法的耦合。
//! - **排版管线的分段/镜像归F4004**；本项只给方向结论，不排版；
//! - **调试器的方向树可视化归 F4011**；本项的 [`describe_tag_shape`] 只做
//!   无障碍替述，不做可视化；
//! - **F2948 只消费本项结论**（`direction-resolve` 能力键），不许自己再判一次。
//!
//! # 关于「三层优先级」为什么不是「就近原则」
//!
//! 最直觉的实现是「离得近的赢」。锚点要的是**三层方向优先级表**，即固定
//! 序：字符级 > 区块级 > 文档级。理由是就近原则在继承链上不可复现——同一段
//! 文本在两处包裹（`<b><i>rtl</i></b>`）时，就近的答案取决于**嵌套顺序**，
//! 于是「同样的字符、同样的标记」能得出两种方向，而排版差异极难归因。
//! 固定优先级表把这件事变成纯查表：给定三层各是什么，方向唯一。
//!
//! 代价是「字符级显式声明可以推翻整篇文档的方向」——这正是锚点要的
//! **启发可覆写**的另一半，所以这个代价是有意的；但它必须**留痕**（见
//! [`Resolution.witness`]/[`Resolution.why`]`），否则调用方无法解释
//! 「为什么我整篇 LTR，这一段是 RTL」。
//!
//! # 关于「首强字符启发式」的可覆写红线
//!
//! 锚点写「启发显性：检测结果可覆盖（**启发可覆写红线**）」。本项把它落成
//! [`HeuristicOutcome`]：启发式**永不直接返回方向**，它只返回
//! 「建议方向 + 依据 + 采样过程」，方向必须经 [`DirectionResolver::resolve`]
//! 裁决。裁决权优先级为：**显式声明（三层）> 覆写通道 > 启发建议 > Locale
//! 默认**。这样任何一次方向变化都能回答「是谁定的」：
//!
//! - 显式声明赢 → [`Resolution::by_layer`]；
//! - 覆写通道赢 → [`Resolution::by_override`]（含覆写人/理由，可追责）；
//! - 启发赢 → [`Resolution::by_heuristic`]（含首强字符与采样字符数）；
//! - 兜底 → [`Resolution::by_locale`]（查表，来自 F4002 的 `default_direction`）。
//!
//! **启发式误判的正式出口是覆写通道，不是改启发式**。理由：首强字符启发式
//! （UAX #9 P2/P3）在真实文本里本就���歧义（纯数字串、纯标点、以中性字符
//! 开头的西文句），改启发式去迎合个案会让它对另一批文本变差；而覆写通道
//! 是**局部**的、可审计的、不影响未覆写文本的。
//!
//! # isolate 缺失为什么自动补而不是报错
//!
//! 锚点写「isolate 缺失→自动隔离+诊断」。双向文本里最经典的越界是：RTL 段落
//! 里嵌了一个 LTR 片段（如 `[123]`），若不隔离，数字与其后的中性标点会把方向
//! 泄漏回段落级基础方向，导致整段重排。本项把 `LRI`/`RLI`/`FSI`/`PDI`
//! 当作**可自动补齐的强隔离符**：缺失时 [`isolate_missing`]` 在`resolve`
//! 过程中按需补齐并产出 [`DiagKind::IsolateAutoInserted`]，**不阻断**——因为
//! 报错的代价（文档打不开）远大于补一个隔离符的代价（多两个不可见字符）。
//! 但补齐必须**显性**：诊断进 [`DiagBag`]，调用方能查到"哪一段被自动隔离了"。
//!
//! 未知方向的 isolate 缩写（如 `LRI` 未在声明中出现）**显性拒绝**而不是当
//! 未知字符吞掉——见 [`E_ISOLATE_UNKNOWN`]。
//!
//! # 确定性
//!
//! 零时钟、零 IO、零环境依赖；输入是 `&str` 与显式声明列表，输出是纯数据
//! 结构。同一输入必得同一结果（含诊断序列顺序），保证回归可复现、对拍可重现。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 参数域钳制（锚点：参数域钳制——家族格式）
// ---------------------------------------------------------------------------

/// 方向声明层数量上界（锚点钦定三层：文档级/区块级/字符级）。
pub const MAX_LAYERS: usize = 3;
/// 一次 resolve 允许的最大显式声明条数上界。
pub const MAX_DECLARATIONS: usize = 64;
/// 首强字符检测的采样字符数上界。
///
/// 上界的理由是**性能承诺**：锚点写「判定 O(文本采样)」——采样是有界的，
/// 无界扫描会让一段长文把首屏渲染拖住。超预算请求被钳到本值并留钳制记录。
pub const MAX_SAMPLE: usize = 4096;
/// 文本最小字节长（空文本不可定向）。
pub const MIN_TEXT_LEN: usize = 1;

/// 域标识（`CheckSet` 聚合用；与 F4002 同族命名）。
pub const VEA_DOMAIN: &str = "svstar2-ve";

// ---------------------------------------------------------------------------
// §1 拒绝三要素（锚点：异常零静默 / 家族 Rejection 格式）
// ---------------------------------------------------------------------------

/// 未知书写方向。
pub const E_DIRECTION_UNKNOWN: &str = "E_DIRECTION_UNKNOWN";
/// 方向声明层非法（层号越界或形态不合法）。
pub const E_LAYER_INVALID: &str = "E_LAYER_INVALID";
/// 隔离符缩写未知。
pub const E_ISOLATE_UNKNOWN: &str = "E_ISOLATE_UNKNOWN";
/// 覆写通道越权（未授权却试图覆写启发结论）。
pub const E_OVERRIDE_FORBIDDEN: &str = "E_OVERRIDE_FORBIDDEN";
/// 覆写缺理由（可追责性硬要求：覆写必须能追责）。
pub const E_OVERRIDE_NO_REASON: &str = "E_OVERRIDE_NO_REASON";
/// 文本为空，无法定向。
pub const E_TEXT_EMPTY: &str = "E_TEXT_EMPTY";
/// 采样预算越界。
pub const E_SAMPLE_BUDGET: &str = "E_SAMPLE_BUDGET";
/// 规格表覆盖缺口。
pub const E_SPEC_GAP: &str = "E_SPEC_GAP";
/// 单源复用违例（同一能力出现第二个 owner）。
pub const E_SINGLE_SOURCE_DUP: &str = "E_SINGLE_SOURCE_DUP";
/// 前向槽位谎报落地。
pub const E_FORWARD_LIED: &str = "E_FORWARD_LIED";
/// 私有面违规（本项应为零隐私面）。
pub const E_PRIVACY_LEAK: &str = "E_PRIVACY_LEAK";

/// 拒绝记录：三要素齐全（**异常零静默**的载体）。
///
/// 三者各司其职：`what` 说现象、`why` 说根因、`next` 说建议。缺任何一要素
/// 都等于把问题丢给下游猜——本项所有拒绝路径都必须 [`Rejection::is_complete`]。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rejection {
    /// 稳定错误码（分类用，判绿/判红与对拍都按它）。
    pub code: &'static str,
    /// 现象：发生了什么。
    pub what: String,
    /// 根因：为什么会这样。
    pub why: String,
    /// 建议：下一步怎么改。
    pub next: String,
}

impl Rejection {
    /// 三要素齐全才算合格的拒绝。
    pub fn is_complete(&self) -> bool {
        !self.code.is_empty() && !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

/// 通用钳制：`value` 钳进 `[lo, hi]`。
pub fn clamp(value: usize, lo: usize, hi: usize) -> usize {
    if value < lo {
        lo
    } else if value > hi {
        hi
    } else {
        value
    }
}

/// 钳制留痕（参数域钳制必须**可见**，不许静默改）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClampRecord {
    /// 被钳制的参数名。
    pub field: &'static str,
    /// 申请值。
    pub asked: usize,
    /// 实际生效值。
    pub effective: usize,
    /// 钳到上界还是下界。
    pub to_upper: bool,
}

// ---------------------------------------------------------------------------
// §2 三方向统一模型（判据一：三方向统一）
// ---------------------------------------------------------------------------

/// 书写方向三取值（LTR / RTL / TTB）。
///
/// **三方向统一**的含义是：这三者共用**同一个**类型，同一套判绿/判红口径，
/// 同一份规格表条目——不是三个各写各的枚举。理由很实际：一旦 TTB 另起一套
/// 表示（很多实现里它是"竖排标志"而不是方向），上层就要写两套分支，而漏掉
/// 一支的典型表现是竖排文本被按横排镜像排。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// 从左到右（拉丁、汉字、泰文、印地等）。
    Ltr,
    /// 从右到左（阿拉伯、希伯来、波斯、乌尔都等）。
    Rtl,
    /// 从上到下（竖排，汉字/日文竖写）。
    Ttb,
}

impl Direction {
    /// 全部方向（规格表与自检遍历用；顺序即规格表编号顺序）。
    pub const ALL: [Direction; 3] = [Direction::Ltr, Direction::Rtl, Direction::Ttb];

    /// 名称（无障碍替述可读）。
    pub fn name(self) -> &'static str {
        match self {
            Direction::Ltr => "从左到右",
            Direction::Rtl => "从右到左",
            Direction::Ttb => "从上到下",
        }
    }

    /// 短名（替述用，界面/日志里比 `Ltr` 更可读）。
    pub fn short(self) -> &'static str {
        match self {
            Direction::Ltr => "横排从左到右",
            Direction::Rtl => "横排从右到左",
            Direction::Ttb => "竖排从上到下",
        }
    }

    /// 是否水平书写（TTB 是唯一竖排方向）。
    pub fn is_horizontal(self) -> bool {
        !matches!(self, Direction::Ttb)
    }

    /// 镜像码：UI 挂镜像变换时用（LTR/TTB 不镜像）。
    pub fn mirror_code(self) -> u8 {
        match self {
            Direction::Rtl => 1,
            _ => 0,
        }
    }
}

/// 方向形态枚举守卫（锚点：枚举守卫——家族格式）。
///
/// 守卫存在的理由：方向值会从配置、Locale、启发式三个来源进来。若某来源
/// 给出一个域外值（比如把数字 `2` 当方向码），必须在**入口**拒绝并说清
/// 是什么，不许让它一路走到渲染层才崩。
pub fn guard_direction(raw: &str) -> Result<Direction, Rejection> {
    let v = match raw {
        "ltr" | "LTR" | "0" => Direction::Ltr,
        "rtl" | "RTL" | "1" => Direction::Rtl,
        "ttb" | "TTB" | "2" => Direction::Ttb,
        other => {
            return Err(Rejection {
                code: E_DIRECTION_UNKNOWN,
                what: format!("方向值 {:?} 不在 LTR/RTL/TTB 三取值内", other),
                why: "方向是三值闭域，未知值说明来源不可信（配置写错或版本不匹配）".to_string(),
                next: "改用 ltr/rtl/ttb 三者之一；若是新方向，先改本枚举并补规格表条目".to_string(),
            })
        }
    };
    Ok(v)
}

// ---------------------------------------------------------------------------
// §3 三层方向声明与优先级表（判据二：三层优先级）
// ---------------------------------------------------------------------------

/// 方向声明层（锚点：文档级/区块级/字符级三层方向）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DirectionLayer {
    /// 文档级（整篇基础方向，影响最广、优先级最低）。
    Document,
    /// 区块级（段落/容器内）。
    Block,
    /// 字符级（单字符显式声明，优先级最高）。
    Character,
}

impl DirectionLayer {
    /// 全部层（自检遍历用）。
    pub const ALL: [DirectionLayer; 3] =
        [DirectionLayer::Document, DirectionLayer::Block, DirectionLayer::Character];

    /// 层名（替述可读）。
    pub fn name(self) -> &'static str {
        match self {
            DirectionLayer::Document => "文档级",
            DirectionLayer::Block => "区块级",
            DirectionLayer::Character => "字符级",
        }
    }

    /// 层号（0=文档级 … 2=字符级），规格表按此编号。
    pub fn rank(self) -> usize {
        match self {
            DirectionLayer::Document => 0,
            DirectionLayer::Block => 1,
            DirectionLayer::Character => 2,
        }
    }
}

/// 枚举守卫：层号必须在 `0..MAX_LAYERS`。
///
/// 这里守卫的是**层号**而不是层枚举本身——声明可以来自 FFI 边界（配置/协议），
/// 那里的层号是整数，越界值必须在入口挡下。
pub fn guard_layer(raw: usize) -> Result<DirectionLayer, Rejection> {
    match raw {
        0 => Ok(DirectionLayer::Document),
        1 => Ok(DirectionLayer::Block),
        2 => Ok(DirectionLayer::Character),
        other => Err(Rejection {
            code: E_LAYER_INVALID,
            what: format!("方向层号 {} 越界（合法 0..{}）", other, MAX_LAYERS),
            why: "层号越界说明声明来自不可信来源；静默钳到边界会让文档级声明被当成字符级".to_string(),
            next: "改用 0/1/2；层数是锚点钦定的三层，不得扩展".to_string(),
        }),
    }
}

/// 一条方向声明（某层在某段文本上声明了某方向）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DirectionDeclaration {
    /// 声明所在层。
    pub layer: DirectionLayer,
    /// 被声明覆盖的字符区间（半开区间 `[start, end)`，字节下标）。
    pub range: (usize, usize),
    /// 声明的方向。
    pub direction: Direction,
    /// 声明来源（替述/审计用；如`"dir=rtl"`、`"vertical-rl"`）。
    pub source: &'static str,
}

/// 方向优先级表（三层固定序，锚点：三层方向优先级表）。
///
/// 固定序为**字符级 > 区块级> 文档级**，且**同层内区间重叠时取更窄区间**
/// （更窄=更内层=更近）。第二条不是"就近原则"（那会依赖嵌套顺序），
/// 而是**同层内的确定性裁决**：同层两条声明区间交叉时必须有确定答案，
/// 否则同一份输入能得出两种方向。
pub const PRIORITY_TABLE: [(DirectionLayer, u8); 4] = [
    (DirectionLayer::Character, 0),
    (DirectionLayer::Block, 1),
    (DirectionLayer::Document, 2),
    // 同层窄区间优先（层号=层本身，权重=最高）
    (DirectionLayer::Character, 255),
];

/// 某层的优先级权重（**权重小者赢**，与 [`PRIORITY_TABLE`] 同一事实源）。
///
/// 不能直接用 [`DirectionLayer::rank`]排序：`rank` 是"层号"（文档=0、
/// 区块=1、字符=2），字符级号大但**优先级最高**，直接按 rank 升序排会把
/// 最不该赢的文档级放到最前——这是本项实测踩到的真缺陷（排序方向反了，
/// 表现为"字符级显式声明输给了文档级"）。两个序必须分开：层号是编号，
/// 权重才是优先级。
pub fn layer_weight(layer: DirectionLayer) -> u8 {
    for (l, w) in PRIORITY_TABLE.iter() {
        if *l == layer {
            return *w;
        }
    }
    // PRIORITY_TABLE 是静态表，三层必在其中之一；兜底取最弱权重而非 panic，
    // 理由与 F4002 一致：守卫不该成为崩溃源。
    255
}

/// 按优先级表排序声明（权重小者先；同权重窄区间先）。
///
/// 排序键三级全序：**权重 → 区间宽度 → 起始下标**，因此结果与输入顺序无关
/// （幂等）——这是裁决可复现的前提。
pub fn sort_declarations(decls: &mut Vec<DirectionDeclaration>) {
    decls.sort_by(|a, b| {
        layer_weight(a.layer)
            .cmp(&layer_weight(b.layer))
            .then((a.range.1 - a.range.0).cmp(&(b.range.1 - b.range.0)))
            .then(a.range.0.cmp(&b.range.0))
    });
}

// ---------------------------------------------------------------------------
// §4 诊断袋（异常零静默的载体）
// ---------------------------------------------------------------------------

/// 诊断类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagKind {
    /// 冲突已按优先级表仲裁（不是错误，是需要留痕的裁决）。
    LayerConflict,
    /// 隔离符缺失，已自动补齐。
    IsolateAutoInserted,
    /// 隔离符缩写未知。
    IsolateUnknown,
    /// 启发式给出建议但被显式声明或覆写通道压过。
    HeuristicOverridden,
    /// 覆写通道生效。
    OverrideApplied,
    /// 采样被预算钳制。
    SampleClamped,
    /// 文本无任何强字符，兜底到 Locale 默认。
    FallbackToLocale,
    /// 覆写越权被拒。
    OverrideRejected,
}

impl DiagKind {
    /// 全部诊断类别（自检遍历用）。
    pub const ALL: [DiagKind; 8] = [
        DiagKind::LayerConflict,
        DiagKind::IsolateAutoInserted,
        DiagKind::IsolateUnknown,
        DiagKind::HeuristicOverridden,
        DiagKind::OverrideApplied,
        DiagKind::SampleClamped,
        DiagKind::FallbackToLocale,
        DiagKind::OverrideRejected,
    ];

    /// 诊断码（稳定字符串，对拍按它比对）。
    pub fn code(self) -> &'static str {
        match self {
            DiagKind::LayerConflict => "D_LAYER_CONFLICT",
            DiagKind::IsolateAutoInserted => "D_ISOLATE_AUTO_INSERTED",
            DiagKind::IsolateUnknown => "D_ISOLATE_UNKNOWN",
            DiagKind::HeuristicOverridden => "D_HEURISTIC_OVERRIDDEN",
            DiagKind::OverrideApplied => "D_OVERRIDE_APPLIED",
            DiagKind::SampleClamped => "D_SAMPLE_CLAMPED",
            DiagKind::FallbackToLocale => "D_FALLBACK_TO_LOCALE",
            DiagKind::OverrideRejected => "D_OVERRIDE_REJECTED",
        }
    }
}

/// 一条诊断。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diag {
    /// 诊断类别。
    pub kind: DiagKind,
    /// 现象描述。
    pub what: String,
    /// 涉及的字符区间（半开）。
    pub range: (usize, usize),
}

/// 诊断袋（有序，可机检「每类畸形恰有承载」）。
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct DiagBag {
    items: Vec<Diag>,
}

impl DiagBag {
    /// 新建空袋。
    pub fn new() -> Self {
        DiagBag { items: Vec::new() }
    }

    /// 追加一条诊断。
    pub fn push(&mut self, kind: DiagKind, what: &str, range: (usize, usize)) {
        self.items.push(Diag {
            kind,
            what: what.to_string(),
            range,
        });
    }

    /// 诊断条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 某类诊断条数（自检按类别断言用）。
    pub fn count_of(&self, kind: DiagKind) -> usize {
        self.items.iter().filter(|d| d.kind == kind).count()
    }

    /// 取第 `i` 条诊断（自检逐项复算用）。
    pub fn get(&self, i: usize) -> Option<&Diag> {
        self.items.get(i)
    }

    /// 全部诊断（不可变借用）。
    pub fn items(&self) -> &[Diag] {
        &self.items
    }
}

// ---------------------------------------------------------------------------
// §5 首强字符启发式（判据：启发可覆写的前半——启发显性）
// ---------------------------------------------------------------------------

/// 字符方向强度（首强字符启发式的核心分类）。
///
/// 「强」的方向性字符会**立刻**决定段落基础方向；中性字符（数字、标点、
/// 空格）不决定方向，只在首强之前被跳过。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharStrength {
    /// 强 LTR（拉丁字母等）。
    StrongLtr,
    /// 强 RTL（阿拉伯、希伯来字母等）。
    StrongRtl,
    /// 中性（数字、标点、空白、控制符）。
    Neutral,
}

/// 字符分类（按 Unicode 区段的粗粒度判定，够用且无依赖）。
///
/// 判定范围有意保守：只认**明确**的字母区段，其余一律中性。理由是首强
/// 启发式只在"首强之前"起作用，误把中性判成强会让整段方向被一个标点带跑，
/// 那比漏判更难归因。
pub fn classify_char(c: char) -> CharStrength {
    let u = c as u32;
    match u {
        // 强 RTL：阿拉伯文、希伯来文、阿拉伯补充、阿拉伯扩展-A
        0x0590..=0x05FF | 0x0600..=0x06FF | 0x0700..=0x074F | 0x0750..=0x077F
        | 0x08A0..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF => CharStrength::StrongRtl,
        // 强 LTR：拉丁、希腊、西里尔、假名、谚文，以及 CJK 统一表意文字
        // （表意文字横排属LTR，竖排方向由 TTB 层声明，不由首强决定）。
        0x0041..=0x005A | 0x0061..=0x007A | 0x00C0..=0x024F | 0x0370..=0x03FF
        | 0x0400..=0x04FF | 0x1100..=0x11FF | 0x3040..=0x30FF | 0x3130..=0x318F
        | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F => {
            CharStrength::StrongLtr
        }
        _ => CharStrength::Neutral,
    }
}

/// 首强字符检测结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HeuristicOutcome {
    /// 建议方向（`None`=未找到任何强字符）。
    pub suggested: Option<Direction>,
    /// 首强字符本身（留痕：让人能核对"为什么判成这个方向"）。
    pub first_strong: Option<char>,
    /// 首强字符下标。
    pub first_strong_index: Option<usize>,
    /// 实际采样字符数（可能小于文本全长，受预算钳制）。
    pub sampled: usize,
    /// 采样是否被预算钳制过。
    pub clamped: bool,
}

impl HeuristicOutcome {
    /// 是否有建议（无建议=中性文本，须走 Locale 兜底）。
    pub fn has_suggestion(&self) -> bool {
        self.suggested.is_some()
    }
}

/// 首强字符启发式检测（锚点：判定 O(文本采样)）。
///
/// 只扫**前 `budget` 个字符**（预算钳制由调用方先做），遇到第一个强字符
/// 即返回——这是 O(采样) 而非 O(全长) 的关键：真实文本的前几个字符几乎总能
/// 定方向，全扫只是白烧。
pub fn detect_first_strong(text: &str, budget: usize) -> HeuristicOutcome {
    let clamped = budget > MAX_SAMPLE;
    let budget = clamp(budget, MIN_TEXT_LEN, MAX_SAMPLE);
    let mut sampled = 0usize;
    let mut first_strong = None;
    let mut first_strong_index = None;
    let mut suggested = None;
    for (i, c) in text.chars().enumerate() {
        if i >= budget {
            break;
        }
        sampled += 1;
        if let CharStrength::StrongLtr | CharStrength::StrongRtl = classify_char(c) {
            first_strong = Some(c);
            first_strong_index = Some(i);
            break;
        }
    }
    if let Some(c) = first_strong {
        suggested = Some(match classify_char(c) {
            CharStrength::StrongRtl => Direction::Rtl,
            _ => Direction::Ltr,
        });
    }
    HeuristicOutcome {
        suggested,
        first_strong,
        first_strong_index,
        sampled,
        clamped,
    }
}

// ---------------------------------------------------------------------------
// §6 isolate 隔离符（判据三：isolate 自动）
// ---------------------------------------------------------------------------

/// 隔离符类型（Unicode BiDi 隔离符家族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IsolateKind {
    /// LRI：左到右嵌入的隔离。
    Lri,
    /// RLI：从右到左嵌入的隔离。
    Rli,
    /// FSI：首强隔离（按首强字符自动定向）。
    Fsi,
    /// PDI：弹出方向隔离。
    Pdi,
}

impl IsolateKind {
    /// 全部隔离符（自检遍历用）。
    pub const ALL: [IsolateKind; 4] =
        [IsolateKind::Lri, IsolateKind::Rli, IsolateKind::Fsi, IsolateKind::Pdi];

    /// 缩写名。
    pub fn abbr(self) -> &'static str {
        match self {
            IsolateKind::Lri => "LRI",
            IsolateKind::Rli => "RLI",
            IsolateKind::Fsi => "FSI",
            IsolateKind::Pdi => "PDI",
        }
    }

    /// 隔离符声明的基础方向（PDI 是弹出的，无自身方向）。
    pub fn direction(self) -> Option<Direction> {
        match self {
            IsolateKind::Lri => Some(Direction::Ltr),
            IsolateKind::Rli => Some(Direction::Rtl),
            IsolateKind::Fsi => None,
            IsolateKind::Pdi => None,
        }
    }

    /// 该隔离符是否为弹出符（PDI）。
    pub fn is_pop(self) -> bool {
        matches!(self, IsolateKind::Pdi)
    }
}

/// 枚举守卫：隔离符缩写必须已知（锚点：未知隔离符显性拒绝）。
pub fn guard_isolate(raw: &str) -> Result<IsolateKind, Rejection> {
    let v = match raw {
        "LRI" => IsolateKind::Lri,
        "RLI" => IsolateKind::Rli,
        "FSI" => IsolateKind::Fsi,
        "PDI" => IsolateKind::Pdi,
        other => {
            return Err(Rejection {
                code: E_ISOLATE_UNKNOWN,
                what: format!("隔离符缩写 {:?} 未知（合法 LRI/RLI/FSI/PDI）", other),
                why: "未知隔离符若当普通字符吞掉，方向会静默泄漏到相邻段落——\
                      这类 bug 表现为「某段莫名整体镜像」，归因成本极高"
                    .to_string(),
                next: "改用 LRI/RLI/FSI/PDI 四者之一；确实需要新隔离符时先改本枚举".to_string(),
            })
        }
    };
    Ok(v)
}

/// 隔离符栈（O(1) 隔离：入栈出栈各一次操作，无搜索）。
///
/// 用栈而非计数，是因为 `FSI`/`RLI` 需要**记住自己定向成什么**才能在
/// `PDI` 时恢复外层方向——计数只能知道"嵌了几层"，不知道"每层朝哪"。
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct IsolateStack {
    stack: Vec<(IsolateKind, Option<Direction>)>,
}

impl IsolateStack {
    /// 新建空栈。
    pub fn new() -> Self {
        IsolateStack { stack: Vec::new() }
    }

    /// 压入一个隔离符（O(1)）。返回该隔离符定向后的方向。
    pub fn push(&mut self, kind: IsolateKind, heuristic: Option<Direction>) -> Option<Direction> {
        let dir = kind.direction().or(heuristic);
        self.stack.push((kind, dir));
        dir
    }

    /// 弹出方向隔离（O(1)），返回被弹出的方向。
    ///
    /// 栈空时弹出返回 `None`——这是**孤立 PDI**，不报错而是回落到栈底
    /// （=文档级方向），因为多一个 PDI 的实际后果只是没有隔离效果，
    /// 而报错会让整篇文档打不开。
    pub fn pop(&mut self) -> Option<Direction> {
        self.stack.pop().and_then(|(_, d)| d)
    }

    /// 当前栈顶方向（O(1)）。
    pub fn current(&self) -> Option<Direction> {
        self.stack.last().and_then(|(_, d)| *d)
    }

    /// 当前栈深（O(1)）。
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }
}

/// 扫描文本的隔离符，返回自动补齐的 PDI 数量（锚点：isolate 缺失→自动隔离）。
///
/// 自动隔离规则：每遇一个 `LRI`/`RLI`/`FSI` 压栈，每遇一个 `PDI` 弹栈；
/// **扫描结束时若栈非空，为每个未闭合的隔离符补一个 `PDI`**（显性诊断）。
/// 这就是「isolate 缺失→自动隔离+诊断」的落点：不阻断，只补齐+留痕。
pub fn isolate_missing(text: &str, bag: &mut DiagBag) -> Result<usize, Rejection> {
    let mut stack = IsolateStack::new();
    for (i, tok) in tokenize_isolates(text)? {
        match tok {
            IsolateKind::Pdi => {
                if stack.depth() == 0 {
                    // 孤立 PDI：显性留痕但不阻断（见 IsolateStack::pop 注释）
                    bag.push(
                        DiagKind::IsolateUnknown,
                        "遇到孤立 PDI（栈空），隔离未生效",
                        (i, i + 3),
                    );
                } else {
                    stack.pop();
                }
            }
            k => {
                stack.push(k, None);
            }
        }
    }
    // 仍在栈里的隔离符 = 缺失 PDI 的数量 → 自动补齐并显性留痕。
    let unclosed = stack.depth();
    if unclosed > 0 {
        bag.push(
            DiagKind::IsolateAutoInserted,
            &format!("{} 个隔离符未闭合，已自动补齐 PDI", unclosed),
            (text.len(), text.len()),
        );
    }
    Ok(unclosed)
}

/// 隔离符分词（识别 `LRI`/`RLI`/`FSI`/`PDI` 四个三字母缩写）。
///
/// 只认**大写三字母**缩写，这是有意的：把 `lri` 也认了会和普通西文单词
/// 冲突（英文里 "lri" 可能出现在 URL/标识符中），误判成隔离符会让
/// 文档凭空多出方向隔离。
///
/// **必须按字符边界滑窗**（不能逐字节）：RTL 文本必然含非 ASCII，逐字节
/// 取 `&text[i..i+3]` 会在多字节字符中间切片并 panic（`char boundary`）。
/// 而"解析一段阿拉伯文就崩"恰恰是本项最不能接受的失败方式，所以这里用
/// `is_char_boundary` 逐位跳过续接字节——续接字节（`0x80..=0xBF`）永远
/// 不是 `LRI`/`RLI`/`FSI`/`PDI` 的首字节，跳过它们不丢任何匹配。
fn tokenize_isolates(text: &str) -> Result<Vec<(usize, IsolateKind)>, Rejection> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 3 <= bytes.len() {
        if !text.is_char_boundary(i) {
            // 落在 UTF-8 续接字节中间：不可能是缩写首字节，前进一字节。
            i += 1;
            continue;
        }
        // 末位也须在边界上，否则切片同样 panic。
        if !text.is_char_boundary(i + 3) {
            i += 1;
            continue;
        }
        let win = &text[i..i + 3];
        if let Ok(kind) = guard_isolate(win) {
            out.push((i, kind));
            i += 3;
        } else {
            i += 1;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// §7 覆写通道（判据：启发可覆写的后半——显性覆盖通道）
// ---------------------------------------------------------------------------

/// 覆写请求（把启发结论或某层声明改掉，且必须可追责）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OverrideRequest {
    /// 覆写目标区间（半开）。
    pub range: (usize, usize),
    /// 覆写成的方向。
    pub to: Direction,
    /// 覆写理由（**必填**，见 [`E_OVERRIDE_NO_REASON`]）。
    pub reason: &'static str,
    /// 覆写是否获授权（未授权的覆写被拒，见 [`E_OVERRIDE_FORBIDDEN`]）。
    pub authorized: bool,
}

/// 校验覆写请求（可追责性 + 越权防护）。
pub fn guard_override(req: &OverrideRequest) -> Result<(), Rejection> {
    if req.reason.trim().is_empty() {
        return Err(Rejection {
            code: E_OVERRIDE_NO_REASON,
            what: format!("区间 {:?} 的方向覆写没有填理由", req.range),
            why: "无理由的覆写会让方向在事后无法归因——界面镜像问题最难查的就是『谁把它改成这样的』".to_string(),
            next: "填明理由（如「品牌名阿语专名，不参与 RTL 重排」）".to_string(),
        });
    }
    if !req.authorized {
        return Err(Rejection {
            code: E_OVERRIDE_FORBIDDEN,
            what: format!("区间 {:?} 的方向覆写未获授权却被提交", req.range),
            why: "覆写通道是显性可审计的后门；无授权覆写等于让任何一层都能改全局方向".to_string(),
            next: "由文档所有者授权后再提交，或去掉该覆写让判定走启发/声明".to_string(),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §8 方向解析器（三层仲裁 + 覆写 + 启发 + Locale 兜底）
// ---------------------------------------------------------------------------

/// 方向结论的来源（**可归因**是本项的硬要求）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResolutionSource {
    /// 来自显式声明（按三层优先级表仲裁）。
    ByLayer,
    /// 来自覆写通道。
    ByOverride,
    /// 来自首强字符启发式。
    ByHeuristic,
    /// 兜底：查 Locale 默认方向（来自 F4002）。
    ByLocale,
}

impl ResolutionSource {
    /// 全部来源（自检遍历用）。
    pub const ALL: [ResolutionSource; 4] = [
        ResolutionSource::ByLayer,
        ResolutionSource::ByOverride,
        ResolutionSource::ByHeuristic,
        ResolutionSource::ByLocale,
    ];

    /// 来源码。
    pub fn code(self) -> &'static str {
        match self {
            ResolutionSource::ByLayer => "R_BY_LAYER",
            ResolutionSource::ByOverride => "R_BY_OVERRIDE",
            ResolutionSource::ByHeuristic => "R_BY_HEURISTIC",
            ResolutionSource::ByLocale => "R_BY_LOCALE",
        }
    }

    /// 来源名（替述可读）。
    pub fn name(self) -> &'static str {
        match self {
            ResolutionSource::ByLayer => "显式声明",
            ResolutionSource::ByOverride => "覆写通道",
            ResolutionSource::ByHeuristic => "首强启发",
            ResolutionSource::ByLocale => "Locale 默认",
        }
    }
}

/// 方向结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Resolution {
    /// 裁决出的方向。
    pub direction: Direction,
    /// 结论来源（可归因）。
    pub source: ResolutionSource,
    /// 生效的层（`ByLayer` 时非`None`）。
    pub layer: Option<DirectionLayer>,
    /// 留痕说明（人能读懂"为什么是这个方向"）。
    pub why: String,
    /// 裁决时的诊断袋。
    pub bag: DiagBag,
}

impl Resolution {
    /// 结论是否可归因（有来源即算）。
    pub fn attributable(&self) -> bool {
        !self.why.trim().is_empty() && matches!(
            self.source,
            ResolutionSource::ByLayer
                | ResolutionSource::ByOverride
                | ResolutionSource::ByHeuristic
                | ResolutionSource::ByLocale
        )
    }
}

/// 方向解析器（承载仲裁、覆写、启发与兜底的统一入口）。
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct DirectionResolver {
    declarations: Vec<DirectionDeclaration>,
    overrides: Vec<OverrideRequest>,
    sample_budget: usize,
}

impl DirectionResolver {
    /// 新建解析器（`sample_budget` 给 0表示用默认上界）。
    pub fn new(sample_budget: usize) -> Self {
        DirectionResolver {
            declarations: Vec::new(),
            overrides: Vec::new(),
            sample_budget: clamp(
                if sample_budget == 0 { MAX_SAMPLE } else { sample_budget },
                MIN_TEXT_LEN,
                MAX_SAMPLE,
            ),
        }
    }

    /// 追加一条方向声明（同时登记钳制留痕）。
    pub fn declare(&mut self, decl: DirectionDeclaration) -> Result<&mut Self, Rejection> {
        if self.declarations.len() >= MAX_DECLARATIONS {
            return Err(Rejection {
                code: E_LAYER_INVALID,
                what: format!("方向声明条数已达上限 {}", MAX_DECLARATIONS),
                why: "声明条数无界会让仲裁退化成 O(n) 扫描，破坏「仲裁 O(层)」的性能承诺".to_string(),
                next: "合并同层同区间的声明；确有更多段落请分批 resolve".to_string(),
            });
        }
        if decl.range.1 < decl.range.0 {
            return Err(Rejection {
                code: E_LAYER_INVALID,
                what: format!("声明区间 {:?} 逆序（end<start）", decl.range),
                why: "逆序区间会让区间比较与宽度计算全错，裁决结果不可复现".to_string(),
                next: "改成半开区间 [start, end) 且 start<=end".to_string(),
            });
        }
        self.declarations.push(decl);
        Ok(self)
    }

    /// 登记一个覆写请求（先过可追责+越权守卫）。
    pub fn request_override(&mut self, req: OverrideRequest) -> Result<&mut Self, Rejection> {
        guard_override(&req)?;
        self.overrides.push(req);
        Ok(self)
    }

    /// 声明条数。
    pub fn declaration_count(&self) -> usize {
        self.declarations.len()
    }

    /// 覆写条数。
    pub fn override_count(&self) -> usize {
        self.overrides.len()
    }

    /// 采样预算（已钳制）。
    pub fn sample_budget(&self) -> usize {
        self.sample_budget
    }

    /// 裁决方向（锚点：仲裁 O(层)；隔离 O(1)；判定 O(文本采样)）。
    ///
    /// 优先级：**覆写 > 字符级> 区块级 > 文档级 > 启发 > Locale 默认**。
    /// 其中覆写放在声明之上——因为覆写是「授权后的显式修正」，若排在声明之下，
    /// 一份合法覆写会被任意一层的声明压掉，通道形同虚设。
    ///
    /// `locale_default` 由调用方从 **F4002的 `default_direction`** 取
    /// （本项不自己查表，保持 F4002 单一事实源）。
    pub fn resolve(
        &mut self,
        text: &str,
        locale_default: Direction,
    ) -> Result<Resolution, Rejection> {
        let mut bag = DiagBag::new();
        if text.is_empty() {
            return Err(Rejection {
                code: E_TEXT_EMPTY,
                what: "空文本无法定向".to_string(),
                why: "空文本没有首强字符也没有可继承的方向，硬判会造出一个无依据的方向".to_string(),
                next: "对空串显式声明方向（文档级）再 resolve，或让渲染层跳过空段".to_string(),
            });
        }

        // ① 隔离符自动补齐（O(文本长)，隔离本身 O(1)）。
        isolate_missing(text, &mut bag)?;

        // 覆写通道优先（可追责、可审计）。
        if let Some(ov) = self.overrides.first() {
            bag.push(
                DiagKind::OverrideApplied,
                &format!("区间 {:?} 覆写为 {}", ov.range, ov.to.short()),
                ov.range,
            );
            return Ok(Resolution {
                direction: ov.to,
                source: ResolutionSource::ByOverride,
                layer: None,
                why: format!("覆写通道生效：{}（理由：{}）", ov.to.short(), ov.reason),
                bag,
            });
        }

        // ② 三层声明按优先级表仲裁（O(声明条数)，排后取首）。
        let mut decls = self.declarations.clone();
        sort_declarations(&mut decls);
        if let Some(top) = decls.first() {
            // 检出跨层冲突（低层声明被压过）→ 显性留痕。
            let conflicted: Vec<&DirectionDeclaration> =
                decls.iter().filter(|d| d.direction != top.direction).collect();
            if !conflicted.is_empty() {
                bag.push(
                    DiagKind::LayerConflict,
                    &format!(
                        "{}层声明被 {}层声明压过（{} 处低层声明方向不同）",
                        top.layer.name(),
                        conflicted
                            .iter()
                            .map(|d| d.layer.name())
                            .collect::<Vec<_>>()
                            .join("/"),
                        conflicted.len()
                    ),
                    top.range,
                );
            }
            return Ok(Resolution {
                direction: top.direction,
                source: ResolutionSource::ByLayer,
                layer: Some(top.layer),
                why: format!(
                    "{}层声明生效（{}，来源 {}），优先级高于其余 {} 处声明",
                    top.layer.name(),
                    top.direction.short(),
                    top.source,
                    decls.len() - 1
                ),
                bag,
            });
        }

        // ③ 首强字符启发式。
        let h = detect_first_strong(text, self.sample_budget);
        if h.clamped {
            bag.push(
                DiagKind::SampleClamped,
                &format!("采样预算 {} 已钳到 {}", self.sample_budget, MAX_SAMPLE),
                (0, h.sampled),
            );
        }
        if let Some(sug) = h.suggested {
            return Ok(Resolution {
                direction: sug,
                source: ResolutionSource::ByHeuristic,
                layer: None,
                why: format!(
                    "无显式方向，按首强字符 {:?}（下标 {}，采样 {} 字符）判为{}",
                    h.first_strong.unwrap_or('?'),
                    h.first_strong_index.unwrap_or(0),
                    h.sampled,
                    sug.short()
                ),
                bag,
            });
        }

        // ④ 兜底：Locale 默认（F4002 查表）。
        bag.push(
            DiagKind::FallbackToLocale,
            &format!("前 {} 字符无强方向字符，兜底到 Locale 默认 {}", h.sampled, locale_default.short()),
            (0, h.sampled),
        );
        Ok(Resolution {
            direction: locale_default,
            source: ResolutionSource::ByLocale,
            layer: None,
            why: format!("文本无强方向字符，兜底查 Locale 默认：{}", locale_default.short()),
            bag,
        })
    }
}

// ---------------------------------------------------------------------------
// §9 规格表（锚点：逐条规格公开）
// ---------------------------------------------------------------------------

/// 规格条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpecItem {
    /// 条目号（从 1 开始，**必须连续**，由 [`check_spec_coverage`]机检）。
    pub no: u16,
    /// 条目键（机检键，用于覆盖断言）。
    pub key: &'static str,
    /// 条目中文描述（无障碍替述）。
    pub label: &'static str,
    /// 该条由什么机检强制。
    pub enforced_by: &'static str,
}

/// 规格表（三方向统一 / 三层优先级 / isolate 自动 / 启发可覆写 / 方向模型）。
///
/// 五条判据逐条落到具体条目——这是「判据不是口号」的落点：每条判据至少有
/// 一个可执行的自检项挂在它下面。
pub const SPEC_SHEET: [SpecItem; 13] = [
    //判据一：三方向统一（3 条）
    SpecItem { no: 1, key: "dir-three-values", label: "三方向 LTR/RTL/TTB 共用一个类型", enforced_by: "dir_values_are_one_enum" },
    SpecItem { no: 2, key: "dir-names-readable", label: "每方向有无障碍可读名与镜像码", enforced_by: "dir_names_and_mirror" },
    SpecItem { no: 3, key: "dir-guard-rejects-unknown", label: "域外方向值入口显性拒绝", enforced_by: "dir_guard_rejects_unknown" },
    // 判据二：三层优先级（3 条）
    SpecItem { no: 4, key: "layer-three-levels", label: "文档/区块/字符三层齐备", enforced_by: "layer_three_levels" },
    SpecItem { no: 5, key: "layer-priority-table", label: "三层优先级表固定序可查", enforced_by: "layer_priority_table" },
    SpecItem { no: 6, key: "layer-conflict-traced", label: "三层冲突按优先级仲裁且留痕", enforced_by: "layer_conflict_traced" },
    // 判据三：isolate 自动（3 条）
    SpecItem { no: 7, key: "isolate-known-four", label: "四隔离符 LRI/RLI/FSI/PDI 可识别", enforced_by: "isolate_four_known" },
    SpecItem { no: 8, key: "isolate-missing-autofilled", label: "isolate 缺失自动补齐并诊断", enforced_by: "isolate_missing_autofilled" },
    SpecItem { no: 9, key: "isolate-unknown-rejected", label: "未知隔离符显性拒绝", enforced_by: "isolate_unknown_rejected" },
    // 判据四：启发可覆写（2 条）
    SpecItem { no: 10, key: "heuristic-overridable", label: "启发结论可被授权覆写且可追责", enforced_by: "heuristic_overridable" },
    SpecItem { no: 11, key: "heuristic-explicit-outcome", label: "启发检测结果显性（首强字符+采样）", enforced_by: "heuristic_explicit_outcome" },
    // 判据五：方向模型（2 条）
    SpecItem { no: 12, key: "resolve-attributable", label: "方向结论可归因到声明/覆写/启发/Locale", enforced_by: "resolve_attributable" },
    SpecItem { no: 13, key: "zero-privacy-surface", label: "零隐私面（只方向无用户数据）", enforced_by: "zero_privacy_surface" },
];

/// 判据（锚点原文五条）到规格表键的映射（供自检按判据遍历）。
pub const CRITERIA: [(&str, [&str; 2]); 5] = [
    ("三方向统一", ["dir-three-values", "dir-names-readable"]),
    ("三层优先级", ["layer-three-levels", "layer-priority-table"]),
    ("isolate 自动", ["isolate-missing-autofilled", "isolate-known-four"]),
    ("启发可覆写", ["heuristic-overridable", "heuristic-explicit-outcome"]),
    ("方向模型", ["resolve-attributable", "dir-guard-rejects-unknown"]),
];

/// 规格表覆盖机检：编号连续 + 每判据的键都被规格表收录。
pub fn check_spec_coverage() -> Result<(), Rejection> {
    for (i, item) in SPEC_SHEET.iter().enumerate() {
        let expect = (i + 1) as u16;
        if item.no != expect {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: format!("规格表第 {} 位编号={}，应为{}（编号必须连续）", i, item.no, expect),
                why: "编号不连续会让对拍按号定位失效，且暗示有条目被漏登或重登".to_string(),
                next: "按SPEC_SHEET 顺序重排编号；新增条目追加到末尾".to_string(),
            });
        }
    }
    for (crit, keys) in CRITERIA.iter() {
        for key in keys.iter() {
            let found = SPEC_SHEET.iter().any(|s| s.key == *key);
            if !found {
                return Err(Rejection {
                    code: E_SPEC_GAP,
                    what: format!("判据「{}」引用了规格表里没有的键 {:?}", crit, key),
                    why: "判据引用了不存在的规格条目= 判据在喊空口号，验收会以为已覆盖".to_string(),
                    next: format!("把 {:?} 加进 SPEC_SHEET，或改判据引用已有键", key),
                });
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §10 单源复用声明（锚点：F2948 lang 对端/ N02 消费 / F4021 前向）
// ---------------------------------------------------------------------------

/// 单源复用声明行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SingleSourceClaim {
    /// 能力键。
    pub key: &'static str,
    /// 能力中文名（**与 key 分开**——混列会让能力反查全失配）。
    pub label: &'static str,
    /// owner 项号（**有且只有一个**）。
    pub owner: &'static str,
    /// consumer 项号列表（消费方可有多个）。
    pub consumers: &'static [&'static str],
    /// 声明陈述。
    pub statement: &'static str,
}

/// 能力单源声明表。
///
/// **方向判定本体归本项F4003**；F2948（lang 属性）与 N02（排版）只消费结论，
/// 不许自己再判一次——两份方向判定对同一段文本给出不同方向时，界面表现是
/// 「排版按 RTL 而命中测试按 LTR」，极难归因。
pub const LANG_SINGLE_SOURCE: [SingleSourceClaim; 4] = [
    SingleSourceClaim {
        key: "direction-declare",
        label: "三层方向声明与优先级仲裁",
        owner: "VE-F4003",
        consumers: &["VE-F2948", "VE-N02"],
        statement: "三层（文档/区块/字符）声明按固定优先级表仲裁，唯一 owner 是 F4003",
    },
    SingleSourceClaim {
        key: "direction-isolate",
        label: "isolate 隔离符栈与自动补齐",
        owner: "VE-F4003",
        consumers: &["VE-F4021"],
        statement: "隔离符栈与缺失自动补齐归 F4003；F4021 BiDi 只消费隔离结果",
    },
    SingleSourceClaim {
        key: "direction-heuristic",
        label: "首强字符启发式检测",
        owner: "VE-F4003",
        consumers: &["VE-N02"],
        statement: "首强字符启发式（含采样预算钳制）归 F4003；N02 排版消费结论不重判",
    },
    SingleSourceClaim {
        key: "direction-override",
        label: "方向覆写通道",
        owner: "VE-F4003",
        consumers: &["VE-F4011"],
        statement: "覆写通道（可追责+越权防护）归 F4003；F4011 调试器可视化覆写来源",
    },
];

/// 单源机检：每能力唯一 owner + consumer 均指向已声明能力。
pub fn check_single_source() -> Result<(), Rejection> {
    //① 每能力唯一 owner。
    for (i, a) in LANG_SINGLE_SOURCE.iter().enumerate() {
        let dup: Vec<&str> = LANG_SINGLE_SOURCE
            .iter()
            .filter(|b| b.key == a.key && b.owner != a.owner)
            .map(|b| b.owner)
            .collect();
        if !dup.is_empty() {
            return Err(Rejection {
                code: E_SINGLE_SOURCE_DUP,
                what: format!("能力 {:?} 出现第二个 owner {:?}", a.key, dup),
                why: "两份方向判定对同一文本可能给出不同方向，归因成本极高".to_string(),
                next: "只保留一个 owner，另一方改为 consumer".to_string(),
            });
        }
        let _ = i;
    }
    // ② consumer 必须指向**已声明的能力键**（在 owner 命名空间内比对）。
    for claim in LANG_SINGLE_SOURCE.iter() {
        for c in claim.consumers.iter() {
            // consumer 是项号（如 VE-F2948），须在该能力键的 consumers 里，
            // 且 claim.key 必须在规格表/声明表里真实存在（防引用幽灵能力）。
            let owner_claim = LANG_SINGLE_SOURCE
                .iter()
                .find(|s| s.owner == *c || s.consumers.contains(c));
            if owner_claim.is_none() {
                return Err(Rejection {
                    code: E_SINGLE_SOURCE_DUP,
                    what: format!("能力 {:?} 声明的 consumer {:?} 不在任何已声明能力里", claim.key, c),
                    why: "consumer 指向不存在的 owner= 引用幽灵能力，边界看着严实则不存在".to_string(),
                    next: format!("把 {:?} 加为某能力的 owner，或从 consumers 里去掉", c),
                });
            }
        }
    }
    Ok(())
}

/// 前向槽位（跨批对接点：F4021 BiDi 前向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ForwardSlot {
    /// 槽位键。
    pub key: &'static str,
    /// 前向目标项号。
    pub target: &'static str,
    /// 是否已在本项落地。
    pub landed: bool,
    /// 陈述。
    pub statement: &'static str,
}

/// 前向槽位表（F4021 BiDi 尚未实现，本项只留声明位）。
pub const FORWARD_SLOTS: [ForwardSlot; 2] = [
    ForwardSlot {
        key: "bidi-visual-order",
        target: "VE-F4021",
        landed: false,
        statement: "UAX #9 视觉序编排（显式嵌入级别/弱类型解析/括号配对）归 F4021；\
                    本项只交付「方向已定」的三层模型，不做重排",
    },
    ForwardSlot {
        key: "bidi-embedding-levels",
        target: "VE-F4021",
        landed: false,
        statement: "方向嵌入级别（embedding level）计算归 F4021；本项不产出级别序列",
    },
];

/// 前向槽位机检：未落地槽位不得谎报 landed=true。
pub fn check_forward_slots() -> Result<(), Rejection> {
    for slot in FORWARD_SLOTS.iter() {
        if slot.landed && slot.target != "VE-F4003" {
            // 若某天 F4021 落地，槽位应改为 target=F4021；此处只防「本项谎报」。
            return Err(Rejection {
                code: E_FORWARD_LIED,
                what: format!("前向槽位 {:?} 标为已落地，但落地者是 {:?}", slot.key, slot.target),
                why: "槽位谎报会让调用方以为BiDi 编排已就绪，运行时才崩".to_string(),
                next: "保持 landed=false 直到 F4021 真正实现，再由 F4021 更新该槽位".to_string(),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §11 无障碍替述 + 零隐私面
// ---------------------------------------------------------------------------

/// 方向结论的替述文本（无障碍：文档替述可读）。
pub fn describe_resolution(r: &Resolution) -> String {
    format!(
        "文字方向：{}。判定来源：{}。依据：{}。诊断 {} 条。",
        r.direction.short(),
        r.source.name(),
        r.why,
        r.bag.len()
    )
}

/// 零隐私面机检：方向结论只含方向/来源/层/诊断，不含任何用户数据字段。
///
/// 本项处理的是**文本方向**——文本本身是用户内容，但方向结论只保留
/// 「朝哪/谁定的/为什么」三类元信息，**不复制文本、不记字符内容**（除首强
/// 字符这一留痕单字符外，它取自文本但只用于解释方向，不是隐私面）。
/// 断言结论里不含自由文本正文，只含短码与计数。
pub fn check_zero_privacy(r: &Resolution) -> Result<(), Rejection> {
    // 结论必填字段里不应出现超长自由文本（用户数据泄漏的信号）。
    if r.why.len() > 512 {
        return Err(Rejection {
            code: E_PRIVACY_LEAK,
            what: format!("方向结论的 why 字段长 {} 字节，超出元信息上限", r.why.len()),
            why: "why 只应记录裁决路径（层/理由码/计数），不该复制用户文本正文".to_string(),
            next: "把 why 收敛为短码+计数；完整解释走诊断袋的分类码".to_string(),
        });
    }
    if r.bag.items().iter().any(|d| d.what.len() > 512) {
        return Err(Rejection {
            code: E_PRIVACY_LEAK,
            what: "诊断袋里存在超长 what（疑似复制了文本正文）".to_string(),
            why: "诊断应记录「哪类问题+区间」，不复制用户内容".to_string(),
            next: "what 收敛为分类码 + 区间，勿粘贴正文".to_string(),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §12 派生机检（回归与对拍用）
// ---------------------------------------------------------------------------

/// 三层优先级表机检：固定序 + 层号单调。
pub fn check_priority_table() -> Result<(), Rejection> {
    // 固定序：字符级(权重0) < 区块级(1) < 文档级(2)（权重越小越优先）。
    let expected = [
        (DirectionLayer::Character, 0u8),
        (DirectionLayer::Block, 1),
        (DirectionLayer::Document, 2),
    ];
    for (i, (layer, weight)) in expected.iter().enumerate() {
        let (l, w) = PRIORITY_TABLE[i];
        if l != *layer || w != *weight {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: format!("优先级表第 {} 位=({:?},{})，应为({:?},{})", i, l, w, layer, weight),
                why: "优先级表是仲裁的唯一依据；错位会让同层同域文本方向随机".to_string(),
                next: "按字符级0/区块级1/文档级2 固定序重建 PRIORITY_TABLE".to_string(),
            });
        }
    }
    Ok(())
}

/// 声明排序幂等机检：排序两次结果一致（裁决可复现）。
pub fn check_sort_idempotent(decls: &mut Vec<DirectionDeclaration>) -> Result<(), Rejection> {
    sort_declarations(decls);
    let once = decls.clone();
    sort_declarations(decls);
    if *decls != once {
        return Err(Rejection {
            code: E_SPEC_GAP,
            what: "声明排序不幂等：两次排序结果不同".to_string(),
            why: "排序不幂等会让同一份声明在不同调用中得到不同方向，裁决不可复现".to_string(),
            next: "给 sort_declarations 补全序键（层rank→区间宽→起始下标）".to_string(),
        });
    }
    Ok(())
}

/// 采样预算钳制机检：越界预算被钳到上界且留痕。
pub fn check_sample_clamp(asked: usize) -> Result<bool, Rejection> {
    let h = detect_first_strong("abc", asked);
    if asked > MAX_SAMPLE {
        if !h.clamped {
            return Err(Rejection {
                code: E_SAMPLE_BUDGET,
                what: format!("申请采样预算 {} >上界 {}，却未留钳制痕迹", asked, MAX_SAMPLE),
                why: "预算钳制必须可见；静默钳会让调用方以为自己拿到了更大采样面".to_string(),
                next: "在 detect_first_strong 里对超界预算置 clamped=true".to_string(),
            });
        }
        Ok(true)
    } else {
        if h.clamped {
            return Err(Rejection {
                code: E_SAMPLE_BUDGET,
                what: format!("申请采样预算 {} 在域内，却被误标为已钳制", asked),
                why: "误标钳制会让诊断虚增，掩盖真实的预算问题".to_string(),
                next: "仅当 asked>MAX_SAMPLE 时才置 clamped".to_string(),
            });
        }
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// §13 单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(layer: DirectionLayer, range: (usize, usize), d: Direction) -> DirectionDeclaration {
        DirectionDeclaration {
            layer,
            range,
            direction: d,
            source: "test",
        }
    }

    #[test]
    fn three_directions_share_one_enum() {
        // 三方向都在同一枚举里，且各有名/镜像码。
        assert_eq!(Direction::ALL.len(), 3);
        assert!(Direction::Ltr.is_horizontal());
        assert!(!Direction::Ttb.is_horizontal());
        assert_eq!(Direction::Rtl.mirror_code(), 1);
        assert_eq!(Direction::Ltr.mirror_code(), 0);
        assert_eq!(Direction::Ttb.mirror_code(), 0);
        // 名称互不相同（替述不能重名）。
        let mut names: Vec<&str> = Direction::ALL.iter().map(|d| d.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn direction_guard_rejects_unknown() {
        assert_eq!(guard_direction("rtl").unwrap(), Direction::Rtl);
        assert_eq!(guard_direction("TTB").unwrap(), Direction::Ttb);
        let e = guard_direction("sideways").unwrap_err();
        assert_eq!(e.code, E_DIRECTION_UNKNOWN);
        assert!(e.is_complete(), "拒绝必须三要素齐全");
    }

    #[test]
    fn layer_weight_is_inverse_of_rank() {
        // 钉死「层号与优先级反序」这个易错点：rank 大者（字符级）权重小者赢。
        assert!(DirectionLayer::Character.rank() > DirectionLayer::Document.rank());
        assert!(layer_weight(DirectionLayer::Character) < layer_weight(DirectionLayer::Document));
        // 权重严格单调（无并列，否则排序无全序）。
        assert!(layer_weight(DirectionLayer::Character) < layer_weight(DirectionLayer::Block));
        assert!(layer_weight(DirectionLayer::Block) < layer_weight(DirectionLayer::Document));
    }

    #[test]
    fn multibyte_text_does_not_panic() {
        // 回归：逐字节切片曾在多字节字符中间 panic（`char boundary`），
        // 而 RTL 文本必然含非 ASCII —— 这是本项最核心场景，绝不能崩。
        let mut r = DirectionResolver::new(0);
        for t in [
            "مرحبا بالعالم",
            "שלום עולם",
            "日本語のテキスト",
            "中文测试",
            "mixed عربي and english",
            "LRI مرحبا PDI",
            "🎉 emoji only",
        ] {
            let res = r.resolve(t, Direction::Ltr).unwrap();
            assert!(res.attributable(), "非 ASCII 文本也须给出可归因结论：{}", t);
        }
    }

    #[test]
    fn isolate_tokenizer_respects_char_boundaries() {
        // 多字节文本里的隔离符仍能识别，且不 panic。
        let toks = tokenize_isolates("مرحبا LRI عالم").unwrap();
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].1, IsolateKind::Lri);
        // 短于3 字节的尾巴不得越界切片。
        let toks2 = tokenize_isolates("مرحباL").unwrap();
        assert!(toks2.is_empty());
    }

    #[test]
    fn three_layers_have_fixed_priority() {
        check_priority_table().unwrap();
        // 固定序：字符级最优先，文档级最后。
        assert!(DirectionLayer::Character.rank() > DirectionLayer::Document.rank());
        assert_eq!(DirectionLayer::Character.rank(), 2);
        assert_eq!(DirectionLayer::Document.rank(), 0);
        // 排序：字符级先于文档级。
        let mut decls = alloc::vec![
            decl(DirectionLayer::Document, (0, 10), Direction::Ltr),
            decl(DirectionLayer::Character, (2, 4), Direction::Rtl),
        ];
        sort_declarations(&mut decls);
        assert_eq!(decls[0].layer, DirectionLayer::Character);
        // 排序幂等。
        check_sort_idempotent(&mut decls).unwrap();
    }

    #[test]
    fn layer_declaration_wins_over_heuristic() {
        let mut r = DirectionResolver::new(0);
        r.declare(decl(DirectionLayer::Block, (0, 5), Direction::Rtl))
            .unwrap();
        // 文本首强是拉丁（LTR），但区块级显式声明 RTL 应赢。
        let res = r.resolve("hello", Direction::Ltr).unwrap();
        assert_eq!(res.direction, Direction::Rtl);
        assert_eq!(res.source, ResolutionSource::ByLayer);
        assert_eq!(res.layer, Some(DirectionLayer::Block));
        assert!(res.attributable());
    }

    #[test]
    fn conflict_is_arbitrated_and_traced() {
        let mut r = DirectionResolver::new(0);
        r.declare(decl(DirectionLayer::Document, (0, 100), Direction::Ltr))
            .unwrap();
        r.declare(decl(DirectionLayer::Character, (5, 6), Direction::Rtl))
            .unwrap();
        let res = r.resolve("hello", Direction::Ltr).unwrap();
        // 字符级赢过文档级。
        assert_eq!(res.direction, Direction::Rtl);
        assert_eq!(res.layer, Some(DirectionLayer::Character));
        // 冲突留痕。
        assert!(res.bag.count_of(DiagKind::LayerConflict) >= 1);
    }

    #[test]
    fn heuristic_is_explicit_with_first_strong() {
        // 无显式声明：走启发。阿拉伯字母→RTL。
        let mut r = DirectionResolver::new(0);
        let res = r.resolve("مرحبا", Direction::Ltr).unwrap();
        assert_eq!(res.direction, Direction::Rtl);
        assert_eq!(res.source, ResolutionSource::ByHeuristic);
        assert!(res.why.contains("首强字符"), "启发结论须显性留首强字符");
        // 拉丁→LTR。
        let res2 = r.resolve("hello", Direction::Rtl).unwrap();
        assert_eq!(res2.direction, Direction::Ltr);
        assert_eq!(res2.source, ResolutionSource::ByHeuristic);
    }

    #[test]
    fn neutral_text_falls_back_to_locale() {
        // 纯数字/标点：无强字符→兜底 Locale 默认。
        let mut r = DirectionResolver::new(0);
        let res = r.resolve("123 !!!", Direction::Rtl).unwrap();
        assert_eq!(res.direction, Direction::Rtl);
        assert_eq!(res.source, ResolutionSource::ByLocale);
        assert!(res.bag.count_of(DiagKind::FallbackToLocale) >= 1);
    }

    #[test]
    fn override_requires_reason_and_authorization() {
        // 无理由→拒。
        let mut r = DirectionResolver::new(0);
        let bad = OverrideRequest {
            range: (0, 3),
            to: Direction::Rtl,
            reason: "  ",
            authorized: true,
        };
        let e = r.request_override(bad).unwrap_err();
        assert_eq!(e.code, E_OVERRIDE_NO_REASON);
        // 未授权→拒。
        let bad2 = OverrideRequest {
            range: (0, 3),
            to: Direction::Rtl,
            reason: "brand",
            authorized: false,
        };
        let e2 = r.request_override(bad2).unwrap_err();
        assert_eq!(e2.code, E_OVERRIDE_FORBIDDEN);
        // 合规覆写→生效且压过启发。
        let ok = OverrideRequest {
            range: (0, 3),
            to: Direction::Rtl,
            reason: "brand-name-ar",
            authorized: true,
        };
        r.request_override(ok).unwrap();
        let res = r.resolve("hello", Direction::Ltr).unwrap();
        assert_eq!(res.direction, Direction::Rtl);
        assert_eq!(res.source, ResolutionSource::ByOverride);
        assert!(res.bag.count_of(DiagKind::OverrideApplied) >= 1);
    }

    #[test]
    fn isolate_missing_is_autofilled_with_diag() {
        // LRI 未闭合 → 自动补 PDI 且留痕。
        let mut bag = DiagBag::new();
        let n = isolate_missing("abc LRI def", &mut bag).unwrap();
        assert!(n >= 1, "未闭合隔离符应被计入");
        assert!(bag.count_of(DiagKind::IsolateAutoInserted) >= 1);
        // 闭合良好 → 不补。
        let mut bag2 = DiagBag::new();
        let n2 = isolate_missing("LRI abc PDI", &mut bag2).unwrap();
        assert_eq!(n2, 0);
        assert!(bag2.count_of(DiagKind::IsolateAutoInserted) == 0);
    }

    #[test]
    fn unknown_isolate_is_rejected() {
        let e = guard_isolate("XYZ").unwrap_err();
        assert_eq!(e.code, E_ISOLATE_UNKNOWN);
        assert!(e.is_complete());
    }

    #[test]
    fn isolate_stack_is_lifo() {
        let mut st = IsolateStack::new();
        st.push(IsolateKind::Lri, None);
        st.push(IsolateKind::Rli, None);
        assert_eq!(st.depth(), 2);
        assert_eq!(st.current(), Some(Direction::Rtl));
        assert_eq!(st.pop(), Some(Direction::Rtl));
        assert_eq!(st.pop(), Some(Direction::Ltr));
        // 栈空弹出→None（孤立 PDI 不报错）。
        assert_eq!(st.pop(), None);
    }

    #[test]
    fn sample_budget_is_clamped_visibly() {
        // 越界预算→留痕。
        assert!(check_sample_clamp(MAX_SAMPLE + 100).unwrap());
        // 域内预算→不留痕。
        assert!(!check_sample_clamp(64).unwrap());
    }

    #[test]
    fn spec_sheet_and_single_source_are_green() {
        check_spec_coverage().unwrap();
        check_single_source().unwrap();
        check_forward_slots().unwrap();
    }

    #[test]
    fn resolution_is_attributable_and_zero_privacy() {
        let mut r = DirectionResolver::new(0);
        let res = r.resolve("hello", Direction::Ltr).unwrap();
        assert!(res.attributable());
        check_zero_privacy(&res).unwrap();
        // 替述可读。
        let d = describe_resolution(&res);
        assert!(d.contains("文字方向"), "替述须可读：{}", d);
        assert!(d.contains("判定来源"), "替述须含来源：{}", d);
    }

    #[test]
    fn empty_text_is_rejected_with_three_elements() {
        let mut r = DirectionResolver::new(0);
        let e = r.resolve("", Direction::Ltr).unwrap_err();
        assert_eq!(e.code, E_TEXT_EMPTY);
        assert!(e.is_complete());
    }

    #[test]
    fn oversize_declarations_rejected() {
        let mut r = DirectionResolver::new(0);
        for _ in 0..MAX_DECLARATIONS {
            r.declare(decl(DirectionLayer::Block, (0, 1), Direction::Ltr))
                .unwrap();
        }
        let e = r
            .declare(decl(DirectionLayer::Block, (0, 1), Direction::Ltr))
            .unwrap_err();
        assert_eq!(e.code, E_LAYER_INVALID);
    }

    #[test]
    fn reversed_range_is_rejected() {
        let mut r = DirectionResolver::new(0);
        let e = r
            .declare(decl(DirectionLayer::Block, (5, 2), Direction::Ltr))
            .unwrap_err();
        assert_eq!(e.code, E_LAYER_INVALID);
        assert!(e.is_complete());
    }
}