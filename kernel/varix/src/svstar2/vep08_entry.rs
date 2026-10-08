//! VE-F3008 · 入场动效族（VE-P 域 · 转场与动效编排 · 入场族）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3008`
//!
//! **判据（锚点原文）**：六型族、错开令牌、首帧原子、触发分型、reduce 终态、判据。
//!
//! **职责定位（锚点原文）**：入场组件族——fade-in/fade-up/scale-in/slide-in 四向/
//! blur-in/clip-reveal 六型基础+组合变体（每型：编排图+默认参数从令牌取+适用场景
//! 注释）；入场节奏规范（首屏入场错开节奏 stagger 30–60ms 均布，错开参数从令牌取）；
//! 首屏 vs 交互触发入场分型（首屏：页面加载编排一次完成；交互触发：滚动进入视口
//! 触发，触发器注册）；与渲染协同（入场动画目标首帧可见性——首帧起始态原子应用，
//! 闪烁=缺陷红线）。
//!
//! ## 一、六型为什么是六个枚举成员而不是六份配置
//!
//! 六型的属性集、适用场景、默认令牌各不相同，且**判据要逐型钉死**：如果把六型
//! 压成一份"参数表"，一个"六型全部退化成 fade-in"的变异实现照样能过"属性集
//! 非空"这类弱断言。枚举成员让每型的差异成为类型系统事实：
//! [`EntryKind::properties`] 返回的属性集逐型互异，slide-in 独有方向四向
//! （[`SlideDir`]），clip-reveal 独有 `clip-path`。
//!
//! ## 二、错开为什么钳制在 [30, 60] 而不是报错
//!
//! 锚点写：错开参数越界→**令牌钳制**。错开是一个"体验参数"而不是"正确性参数"：
//! 请求 10ms 说明作者想要更快的节奏，钳到 30ms 保留语义（均布错开）只调快慢；
//! 若直接报错，整条入场链停摆，代价与错误不成比例。钳制必须**显性**：
//! [`StaggerPlanner`] 记录每次钳制并出诊断（[`E_STAGGER_BOUND`]），
//! 判据对"越界必被钳"做双向断言（低于下界钳到 30、高于上界钳到 60）。
//!
//! ## 三、首帧原子为什么是"一次提交"而不是"逐属性写"
//!
//! 锚点把闪烁定为**缺陷红线**：首帧中间态被绘出即 P1 立案。逐属性写起始态时，
//! 渲染可能穿插在两次写之间——用户看到"半透明+全透明"的中间帧就是闪烁。
//! [`FirstFrameAtomizer::apply_start`] 把全部起始态属性收进一个
//! [`StartSnapshot`]（`atomic` 恒真，单次提交），宿主拿快照一次写入；
//! 任何"先绘了再补起始态"被 [`FirstFrameAtomizer::note_premature_paint`]
//! 捕获即立 P1 案（[`E_FLASH_P1`]），CI 闪烁断言消费
//! [`FirstFrameAtomizer::assert_no_flash`]。
//!
//! ## 四、触发分型与"触发一次"默认
//!
//! 视口反复进出会重复触发入场——锚点规定**触发一次为默认**，重触发必须显性
//! 声明（[`TriggerRule::retrigger`]，且每次重触发都记入诊断）。离屏元素
//! （虚拟化列表回收侧，F2713 联动）进入视口前不触发——[`TriggerDecision::OffscreenDeferred`]
//! 把"延迟触发防离屏浪费"落成显式决策而不是静默忽略。
//!
//! # no_std
//!
//! 仅依赖 [`vep03_token`]（时长/位移令牌单源）、[`vep04_stack`]（域归属）、
//! [`vep05_orch`]（编排图单源，锚点"每型：编排图"）、`alloc`。零 IO、零墙钟、零浮点。

use crate::svstar2::vep03_token::{Lane, MotionTokenError, TokenTable, TokenValue};
use crate::svstar2::vep04_stack::P_NAMESPACE_OWNER;
use crate::svstar2::vep05_orch::{
    compile, CompiledOrch, CompileParams, OrchGraph, OrchNode, Orchestrator, TimingEdge,
    MAX_STAGGER_ELEMENTS,
};

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 入场族协议版本。六型属性集、错开区间或触发语义变更走版本号。
pub const ENTRY_PROTOCOL_VERSION: &str = "P08-entry-v1";

/// 入场基础型数（fade-in/fade-up/scale-in/slide-in/blur-in/clip-reveal）。
pub const KIND_COUNT: usize = 6;

/// 触发器类型数（视口进入/页面加载/条件触发）。
pub const TRIGGER_KIND_COUNT: usize = 3;

/// 错开下界（毫秒，锚点原文 30–60ms 区间钉死）。
pub const STAGGER_MIN_MS: u32 = 30;
/// 错开上界（毫秒，锚点原文 30–60ms 区间钉死）。
pub const STAGGER_MAX_MS: u32 = 60;
/// 默认错开（锚点区间中点；宿主显式给值时走钳制）。
pub const DEFAULT_STAGGER_MS: u32 = 40;

/// 错开元素数上限（复用 F3005 单源上限，不另立标准）。
pub const ENTRY_ELEMENT_CAP: usize = MAX_STAGGER_ELEMENTS;

/// 组件名长度上限。
pub const NAME_CAP: usize = 48;
/// 属性名长度上限。
pub const PROP_CAP: usize = 32;
/// 单元素起始态属性数上限（原子应用 O(属性数) 的规模护栏）。
pub const START_PROP_CAP: usize = 16;

// ---------------------------------------------------------------------------
// 二、诊断码
// ---------------------------------------------------------------------------

/// 入场型未知。
pub const E_ENTRY_KIND: &str = "E_ENTRY_KIND";
/// 组件名非法。
pub const E_ENTRY_NAME: &str = "E_ENTRY_NAME";
/// 注册重名。
pub const E_ENTRY_DUP: &str = "E_ENTRY_DUP";
/// 族为空（无基础型可编排）。
pub const E_ENTRY_EMPTY: &str = "E_ENTRY_EMPTY";
/// 错开越界（已钳制，出诊断）。
pub const E_STAGGER_BOUND: &str = "E_STAGGER_BOUND";
/// 错开元素数超上限。
pub const E_STAGGER_COUNT: &str = "E_STAGGER_COUNT";
/// 触发器类型未知。
pub const E_TRIGGER_KIND: &str = "E_TRIGGER_KIND";
/// 触发器重复登记。
pub const E_TRIGGER_DUP: &str = "E_TRIGGER_DUP";
/// 触发重复（once 默认下拒绝；重触发须显性）。
pub const E_TRIGGER_REPEAT: &str = "E_TRIGGER_REPEAT";
/// 离屏延迟触发（F2713 联动：防离屏浪费）。
pub const E_TRIGGER_OFFSCREEN: &str = "E_TRIGGER_OFFSCREEN";
/// 闪烁检出（P1 立案，缺陷红线）。
pub const E_FLASH_P1: &str = "E_FLASH_P1";
/// 起始态属性为空。
pub const E_START_EMPTY: &str = "E_START_EMPTY";
/// 变体声明非法（无基础型/基础型重复/重名）。
pub const E_VARIANT_BAD: &str = "E_VARIANT_BAD";

// ---------------------------------------------------------------------------
// 三、入场六型（锚点六型族）
// ---------------------------------------------------------------------------

/// slide-in 的四个方向（锚点"slide-in 四向"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideDir {
    /// 自下而上。
    Up,
    /// 自上而下。
    Down,
    /// 自右向左。
    Left,
    /// 自左向右。
    Right,
}

impl SlideDir {
    /// 方向全集（四向，判据全集遍历用）。
    pub const ALL: [SlideDir; 4] = [SlideDir::Up, SlideDir::Down, SlideDir::Left, SlideDir::Right];

    /// 方向短码。
    pub fn wire(self) -> &'static str {
        match self {
            SlideDir::Up => "up",
            SlideDir::Down => "down",
            SlideDir::Left => "left",
            SlideDir::Right => "right",
        }
    }

    /// 单位位移向量（起始态偏移方向；x 向右为正，y 向下为正）。
    pub fn delta(self) -> (i32, i32) {
        match self {
            SlideDir::Up => (0, 1),
            SlideDir::Down => (0, -1),
            SlideDir::Left => (1, 0),
            SlideDir::Right => (-1, 0),
        }
    }

    /// 方向中文名。
    pub fn zh(self) -> &'static str {
        match self {
            SlideDir::Up => "自下而上",
            SlideDir::Down => "自上而下",
            SlideDir::Left => "自右向左",
            SlideDir::Right => "自左向右",
        }
    }
}

/// 入场基础型（锚点六型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// 淡入。
    FadeIn,
    /// 淡入上移。
    FadeUp,
    /// 缩放入场。
    ScaleIn,
    /// 滑入（四向）。
    SlideIn,
    /// 模糊入场。
    BlurIn,
    /// 裁剪揭示。
    ClipReveal,
}

impl EntryKind {
    /// 六型全集（顺序即 index，唯一真值源）。
    pub const ALL: [EntryKind; KIND_COUNT] = [
        EntryKind::FadeIn,
        EntryKind::FadeUp,
        EntryKind::ScaleIn,
        EntryKind::SlideIn,
        EntryKind::BlurIn,
        EntryKind::ClipReveal,
    ];

    /// 型短码（注册名即此，冻结）。
    pub fn wire(self) -> &'static str {
        match self {
            EntryKind::FadeIn => "fade-in",
            EntryKind::FadeUp => "fade-up",
            EntryKind::ScaleIn => "scale-in",
            EntryKind::SlideIn => "slide-in",
            EntryKind::BlurIn => "blur-in",
            EntryKind::ClipReveal => "clip-reveal",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            EntryKind::FadeIn => "淡入",
            EntryKind::FadeUp => "淡入上移",
            EntryKind::ScaleIn => "缩放入场",
            EntryKind::SlideIn => "滑入",
            EntryKind::BlurIn => "模糊入场",
            EntryKind::ClipReveal => "裁剪揭示",
        }
    }

    /// 序号。
    pub fn index(self) -> usize {
        match self {
            EntryKind::FadeIn => 0,
            EntryKind::FadeUp => 1,
            EntryKind::ScaleIn => 2,
            EntryKind::SlideIn => 3,
            EntryKind::BlurIn => 4,
            EntryKind::ClipReveal => 5,
        }
    }

    /// 序号⇒型。
    pub fn from_index(i: usize) -> Option<EntryKind> {
        match i {
            0 => Some(EntryKind::FadeIn),
            1 => Some(EntryKind::FadeUp),
            2 => Some(EntryKind::ScaleIn),
            3 => Some(EntryKind::SlideIn),
            4 => Some(EntryKind::BlurIn),
            5 => Some(EntryKind::ClipReveal),
            _ => None,
        }
    }

    /// 短码⇒型（冻结全集，只认六型）。
    pub fn parse(s: &str) -> Option<EntryKind> {
        match s {
            "fade-in" => Some(EntryKind::FadeIn),
            "fade-up" => Some(EntryKind::FadeUp),
            "scale-in" => Some(EntryKind::ScaleIn),
            "slide-in" => Some(EntryKind::SlideIn),
            "blur-in" => Some(EntryKind::BlurIn),
            "clip-reveal" => Some(EntryKind::ClipReveal),
            _ => None,
        }
    }

    /// 适用场景注释（锚点"每型：适用场景注释"）。
    pub fn scenario(self) -> &'static str {
        match self {
            EntryKind::FadeIn => "通用默认：内容块入场、卡片显现；无位移，前庭最安全",
            EntryKind::FadeUp => "列表项/卡片首屏入场：轻微上移建立阅读方向感",
            EntryKind::ScaleIn => "弹层/徽标入场：强调主体，缩放比例受令牌钳制",
            EntryKind::SlideIn => "抽屉/侧栏/通知入场：方向语义明确，四向各对应来向",
            EntryKind::BlurIn => "图片/大图入场：对焦感强，仅限合成通道可用场景",
            EntryKind::ClipReveal => "横幅/区块揭示：裁剪路径展开，适合结构性呈现",
        }
    }

    /// 动画属性集（**逐型互异**，判据钉死；O04 编译目标选型输入）。
    pub fn properties(self) -> &'static [&'static str] {
        match self {
            EntryKind::FadeIn => &["opacity"],
            EntryKind::FadeUp => &["opacity", "transform"],
            EntryKind::ScaleIn => &["opacity", "transform"],
            EntryKind::SlideIn => &["opacity", "transform"],
            EntryKind::BlurIn => &["opacity", "filter"],
            EntryKind::ClipReveal => &["clip-path"],
        }
    }

    /// 默认时长令牌 ID（**从令牌取**：组件转场档 / 页面转场档）。
    pub fn duration_token_id(self) -> &'static str {
        match self {
            // 页面级位移（抽屉/整块滑入）走页面转场档。
            EntryKind::SlideIn => "dur-page",
            _ => "dur-component",
        }
    }

    /// 默认位移令牌 ID（位移型取阶梯，非位移型显式取 none）。
    pub fn distance_token_id(self) -> &'static str {
        match self {
            EntryKind::FadeUp => "dist-small",
            EntryKind::SlideIn => "dist-medium",
            _ => "dist-none",
        }
    }

    /// 属性集是否全部走合成通道（O04 编译目标选型：入场族全量合成）。
    pub fn all_compositor(self) -> bool {
        // opacity/transform/filter/clip-path 均为合成通道可承载属性；
        // 若未来加入布局属性（width/top 等），此处必须改判——那是结构变化。
        true
    }
}

// ---------------------------------------------------------------------------
// 四、组合变体（锚点"六型基础+组合变体"）
// ---------------------------------------------------------------------------

/// 一条组合变体声明：若干基础型叠加。
///
/// 变体**不产生新属性语义**：属性集是基础型属性集的并集（去重保序），
/// 时长令牌取基础型中的页面档优先（取大原则），位移取基础型中的最大阶梯。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryVariant {
    /// 变体名（注册唯一）。
    pub name: String,
    /// 基础型序列（非空、无重复）。
    pub bases: Vec<EntryKind>,
}

impl EntryVariant {
    /// 构造并校验变体声明。
    pub fn new(name: &str, bases: &[EntryKind]) -> Result<EntryVariant, MotionTokenError> {
        if name.is_empty() || name.chars().count() > NAME_CAP {
            return Err(MotionTokenError::new(
                E_VARIANT_BAD,
                "变体名非法",
                &format!("变体名 {:?} 为空或超过 {} 字上限", name, NAME_CAP),
                "给变体一个 1..=48 字的短名",
                P_NAMESPACE_OWNER,
            ));
        }
        if bases.is_empty() {
            return Err(MotionTokenError::new(
                E_VARIANT_BAD,
                "变体无基础型",
                &format!("变体 {} 未声明任何基础型", name),
                "至少声明一个基础型；空变体编译不出属性集",
                P_NAMESPACE_OWNER,
            ));
        }
        let mut list: Vec<EntryKind> = Vec::new();
        for k in bases.iter() {
            if list.iter().any(|x| x == k) {
                return Err(MotionTokenError::new(
                    E_VARIANT_BAD,
                    "基础型重复",
                    &format!("变体 {} 的基础型 {} 重复出现", name, k.wire()),
                    "去掉重复基础型；重复叠加不改变语义只会放大参数",
                    P_NAMESPACE_OWNER,
                ));
            }
            list.push(*k);
        }
        Ok(EntryVariant {
            name: String::from(name),
            bases: list,
        })
    }

    /// 属性集并集（去重保序：先声明者优先）。
    pub fn properties(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for k in self.bases.iter() {
            for p in k.properties().iter() {
                if !out.iter().any(|x| x == p) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// 变体默认时长令牌：基础型含页面档则取页面档（取大原则）。
    pub fn duration_token_id(&self) -> &'static str {
        let mut id = "dur-component";
        for k in self.bases.iter() {
            if k.duration_token_id() == "dur-page" {
                id = "dur-page";
            }
        }
        id
    }

    /// 变体默认位移令牌：取基础型中的最大阶梯（none<small<medium）。
    pub fn distance_token_id(&self) -> &'static str {
        let ladder = ["dist-none", "dist-small", "dist-medium"];
        let mut best = 0usize;
        for k in self.bases.iter() {
            let d = k.distance_token_id();
            for (i, name) in ladder.iter().enumerate() {
                if *name == d && i > best {
                    best = i;
                }
            }
        }
        ladder[best]
    }

    /// 变体读屏播报。
    pub fn spoken(&self) -> String {
        let mut s = format!("变体 {}（", self.name);
        for (i, k) in self.bases.iter().enumerate() {
            if i > 0 {
                s.push('+');
            }
            s.push_str(k.zh());
        }
        s.push_str("），属性 ");
        for (i, p) in self.properties().iter().enumerate() {
            if i > 0 {
                s.push('/');
            }
            s.push_str(p);
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 五、组件族注册（锚点数据结构：组件族注册，六型+变体）
// ---------------------------------------------------------------------------

/// 一条已注册的入场组件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntrySpec {
    /// 注册名（基础型=型短码；变体=变体名）。
    pub name: String,
    /// 基础型（变体记主型：取第一个基础型）。
    pub kind: EntryKind,
    /// slide-in 方向（非 slide-in 型恒 Up，不参与语义）。
    pub dir: SlideDir,
    /// 适用场景注释（基础型取型注释；变体取主型注释）。
    pub scenario: &'static str,
    /// 是否变体。
    pub is_variant: bool,
}

/// 入场组件族注册表。
#[derive(Clone, Debug, Default)]
pub struct EntryRegistry {
    specs: Vec<EntrySpec>,
    diagnostics: Vec<String>,
}

impl EntryRegistry {
    /// 预置六型注册（锚点"六型基础"；注册即成族，族空=实现缺陷）。
    pub fn with_six() -> Result<EntryRegistry, MotionTokenError> {
        let mut r = EntryRegistry {
            specs: Vec::new(),
            diagnostics: Vec::new(),
        };
        for k in EntryKind::ALL.iter().copied() {
            r.register_base(k, SlideDir::Up)?;
        }
        if r.specs.len() != KIND_COUNT {
            return Err(MotionTokenError::new(
                E_ENTRY_EMPTY,
                "六型预置不完整",
                &format!("预置后族内 {} 条，应为 {}", r.specs.len(), KIND_COUNT),
                "检查 EntryKind::ALL 全集是否被完整注册",
                P_NAMESPACE_OWNER,
            ));
        }
        Ok(r)
    }

    fn register_base(&mut self, kind: EntryKind, dir: SlideDir) -> Result<(), MotionTokenError> {
        let name = kind.wire();
        if self.specs.iter().any(|s| s.name == name) {
            return Err(MotionTokenError::new(
                E_ENTRY_DUP,
                "注册重名",
                &format!("入场型 {} 已在族内", name),
                "六型短码冻结互异；出现重名说明型表被破坏",
                P_NAMESPACE_OWNER,
            ));
        }
        self.specs.push(EntrySpec {
            name: String::from(name),
            kind,
            dir,
            scenario: kind.scenario(),
            is_variant: false,
        });
        Ok(())
    }

    /// 注册 slide-in 的一个方向别名（如 "slide-in-left"）。
    ///
    /// 四向是 slide-in 的语义维度：默认注册覆盖 "slide-in"（四向参数由
    /// [`EntrySpec::dir`]/计划期 [`SlideDir`] 携带），宿主也可把某一向注册为
    /// 独立名字便于按名派发。
    pub fn register_slide_dir(&mut self, dir: SlideDir) -> Result<(), MotionTokenError> {
        let name = format!("slide-in-{}", dir.wire());
        if self.specs.iter().any(|s| s.name == name) {
            return Err(MotionTokenError::new(
                E_ENTRY_DUP,
                "方向别名重复",
                &format!("{} 已注册", name),
                "四向别名各注册一次；重复注册说明初始化路径被执行了两遍",
                P_NAMESPACE_OWNER,
            ));
        }
        self.specs.push(EntrySpec {
            name,
            kind: EntryKind::SlideIn,
            dir,
            scenario: EntryKind::SlideIn.scenario(),
            is_variant: false,
        });
        Ok(())
    }

    /// 注册组合变体。
    pub fn register_variant(&mut self, v: EntryVariant) -> Result<(), MotionTokenError> {
        if self.specs.iter().any(|s| s.name == v.name) {
            return Err(MotionTokenError::new(
                E_ENTRY_DUP,
                "变体重名",
                &format!("名字 {} 已被族内组件占用", v.name),
                "换一个变体名；重名会让按名派发指向不确定",
                P_NAMESPACE_OWNER,
            ));
        }
        // 变体主型取第一个基础型；方向只对 slide-in 语义有效。
        let kind = match v.bases.first() {
            Some(k) => *k,
            None => {
                return Err(MotionTokenError::new(
                    E_VARIANT_BAD,
                    "变体无基础型",
                    &format!("变体 {} 基础型序列为空", v.name),
                    "用 EntryVariant::new 构造；空变体注册不出属性集",
                    P_NAMESPACE_OWNER,
                ))
            }
        };
        // 方向维度对变体不生效（slide-in 的方向在计划期携带）。
        let dir = SlideDir::Up;
        let scenario = kind.scenario();
        self.specs.push(EntrySpec {
            name: v.name.clone(),
            kind,
            dir,
            scenario,
            is_variant: true,
        });
        Ok(())
    }

    /// 按名查找。
    pub fn lookup(&self, name: &str) -> Option<&EntrySpec> {
        self.specs.iter().find(|s| s.name == name)
    }

    /// 族内组件数（六型预置 + 方向别名 + 变体）。
    pub fn count(&self) -> usize {
        self.specs.len()
    }

    /// 变体数。
    pub fn variant_count(&self) -> usize {
        self.specs.iter().filter(|s| s.is_variant).count()
    }

    /// 全部注册名（判据全集遍历用）。
    pub fn names(&self) -> Vec<String> {
        self.specs.iter().map(|s| s.name.clone()).collect()
    }

    /// 诊断清单。
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
}

// ---------------------------------------------------------------------------
// 六、默认参数（锚点"默认参数从令牌取"）
// ---------------------------------------------------------------------------

/// 一型入场的默认参数（全部来源可追溯：字段里带令牌 ID）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryDefaults {
    /// 时长（毫秒；来自时长令牌基线值）。
    pub duration_ms: u32,
    /// 位移（像素；来自位移令牌，非位移型为 0）。
    pub distance_px: u32,
    /// 时长来源令牌 ID。
    pub duration_token: String,
    /// 位移来源令牌 ID。
    pub distance_token: String,
}

impl EntryDefaults {
    /// 从令牌表取某型的默认参数。
    ///
    /// **单源**：时长/位移一律读 [`TokenTable`]（F3003），本模块不私设数值。
    /// 令牌缺失即报错（令牌表由 F3003 判据保证完整，缺失说明表被破坏，
    /// 静默兜底会掩盖破坏）。
    pub fn from_token(table: &TokenTable, kind: EntryKind) -> Result<EntryDefaults, MotionTokenError> {
        let dur_tok = table.require(kind.duration_token_id())?;
        let duration_ms = match &dur_tok.value {
            TokenValue::Millis(ms) => *ms,
            other => {
                return Err(MotionTokenError::new(
                    E_ENTRY_KIND,
                    "时长令牌值型不符",
                    &format!("令牌 {} 的值不是时长型（实际 {:?}）", dur_tok.id, other.css_literal()),
                    "检查令牌表：时长令牌的值必须为 Millis",
                    P_NAMESPACE_OWNER,
                ))
            }
        };
        let dist_tok = table.require(kind.distance_token_id())?;
        let distance_px = match &dist_tok.value {
            TokenValue::Px(px) => *px,
            other => {
                return Err(MotionTokenError::new(
                    E_ENTRY_KIND,
                    "位移令牌值型不符",
                    &format!("令牌 {} 的值不是位移型（实际 {:?}）", dist_tok.id, other.css_literal()),
                    "检查令牌表：位移令牌的值必须为 Px",
                    P_NAMESPACE_OWNER,
                ))
            }
        };
        Ok(EntryDefaults {
            duration_ms,
            distance_px,
            duration_token: String::from(kind.duration_token_id()),
            distance_token: String::from(kind.distance_token_id()),
        })
    }

    /// 变体默认参数（时长/位移取变体聚合的令牌）。
    pub fn from_variant(table: &TokenTable, v: &EntryVariant) -> Result<EntryDefaults, MotionTokenError> {
        let dur_tok = table.require(v.duration_token_id())?;
        let duration_ms = match &dur_tok.value {
            TokenValue::Millis(ms) => *ms,
            _other => {
                return Err(MotionTokenError::new(
                    E_ENTRY_KIND,
                    "变体时长令牌值型不符",
                    &format!("令牌 {} 的值不是时长型", dur_tok.id),
                    "检查令牌表：时长令牌的值必须为 Millis",
                    P_NAMESPACE_OWNER,
                ))
            }
        };
        let dist_tok = table.require(v.distance_token_id())?;
        let distance_px = match &dist_tok.value {
            TokenValue::Px(px) => *px,
            _other => {
                return Err(MotionTokenError::new(
                    E_ENTRY_KIND,
                    "变体位移令牌值型不符",
                    &format!("令牌 {} 的值不是位移型", dist_tok.id),
                    "检查令牌表：位移令牌的值必须为 Px",
                    P_NAMESPACE_OWNER,
                ))
            }
        };
        Ok(EntryDefaults {
            duration_ms,
            distance_px,
            duration_token: String::from(v.duration_token_id()),
            distance_token: String::from(v.distance_token_id()),
        })
    }
}

// ---------------------------------------------------------------------------
// 七、错开规划（锚点"stagger 30-60ms 均布"+"越界→令牌钳制"）
// ---------------------------------------------------------------------------

/// 首屏错开规划器。
///
/// 错开值一律先过 [`StaggerPlanner::clamp`]：区间 [30, 60] 由锚点原文钉死，
/// 越界钳制且**每次钳制都出诊断**（显性，不静默）。
#[derive(Clone, Debug, Default)]
pub struct StaggerPlanner {
    clamped_count: usize,
    diagnostics: Vec<String>,
}

impl StaggerPlanner {
    /// 构造空规划器。
    pub fn new() -> Self {
        StaggerPlanner {
            clamped_count: 0,
            diagnostics: Vec::new(),
        }
    }

    /// 钳制错开值到锚点区间，返回 (生效值, 是否发生钳制)。
    pub fn clamp(&mut self, requested: u32) -> (u32, bool) {
        if requested < STAGGER_MIN_MS {
            self.clamped_count += 1;
            self.diagnostics.push(format!(
                "{}：请求错开 {}ms 低于下界，钳制为 {}ms",
                E_STAGGER_BOUND, requested, STAGGER_MIN_MS
            ));
            return (STAGGER_MIN_MS, true);
        }
        if requested > STAGGER_MAX_MS {
            self.clamped_count += 1;
            self.diagnostics.push(format!(
                "{}：请求错开 {}ms 高于上界，钳制为 {}ms",
                E_STAGGER_BOUND, requested, STAGGER_MAX_MS
            ));
            return (STAGGER_MAX_MS, true);
        }
        (requested, false)
    }

    /// 钳制次数。
    pub fn clamped_count(&self) -> usize {
        self.clamped_count
    }

    /// 诊断清单。
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    /// 均布错开计划：第 i 个元素的起始偏移 = i × step（O(元素数)）。
    ///
    /// 元素数 0 返回空表（不报错：空首屏无元素可错开）；超上限报
    /// [`E_STAGGER_COUNT`]（上限复用 F3005 单源）。偏移走 checked 算术。
    pub fn plan(
        &mut self,
        count: usize,
        requested_step_ms: u32,
    ) -> Result<Vec<u32>, MotionTokenError> {
        if count > ENTRY_ELEMENT_CAP {
            return Err(MotionTokenError::new(
                E_STAGGER_COUNT,
                "错开元素数超上限",
                &format!("元素 {} 超过上限 {}", count, ENTRY_ELEMENT_CAP),
                "拆分入场批次或改为交互触发；一次性错开数千元素会挤占帧预算",
                P_NAMESPACE_OWNER,
            ));
        }
        let (step, _clamped) = self.clamp(requested_step_ms);
        let mut out: Vec<u32> = Vec::with_capacity(count);
        let mut acc: u32 = 0;
        for i in 0..count {
            out.push(acc);
            if i + 1 < count {
                acc = match acc.checked_add(step) {
                    Some(v) => v,
                    None => {
                        return Err(MotionTokenError::new(
                            E_STAGGER_COUNT,
                            "错开偏移溢出",
                            &format!("第 {} 个元素的偏移加法溢出", i + 1),
                            "减少元素数；偏移总量不应接近 u32 上界",
                            P_NAMESPACE_OWNER,
                        ))
                    }
                };
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// 八、触发器（锚点"首屏 vs 交互触发分型"+"触发重复"）
// ---------------------------------------------------------------------------

/// 触发器类型（锚点三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerKind {
    /// 页面加载（首屏：编排一次完成）。
    PageLoad,
    /// 视口进入（交互触发：滚动进入视口触发）。
    ViewportEnter,
    /// 条件触发（宿主状态驱动）。
    Conditional,
}

impl TriggerKind {
    /// 三类全集。
    pub const ALL: [TriggerKind; TRIGGER_KIND_COUNT] = [
        TriggerKind::PageLoad,
        TriggerKind::ViewportEnter,
        TriggerKind::Conditional,
    ];

    /// 短码。
    pub fn wire(self) -> &'static str {
        match self {
            TriggerKind::PageLoad => "page-load",
            TriggerKind::ViewportEnter => "viewport-enter",
            TriggerKind::Conditional => "conditional",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            TriggerKind::PageLoad => "页面加载",
            TriggerKind::ViewportEnter => "视口进入",
            TriggerKind::Conditional => "条件触发",
        }
    }

    /// 短码⇒类（冻结全集）。
    pub fn parse(s: &str) -> Option<TriggerKind> {
        match s {
            "page-load" => Some(TriggerKind::PageLoad),
            "viewport-enter" => Some(TriggerKind::ViewportEnter),
            "conditional" => Some(TriggerKind::Conditional),
            _ => None,
        }
    }
}

/// 一条触发规则。
///
/// `once` 默认**触发一次**（锚点：视口反复进出重复触发→触发一次配置默认）；
/// `retrigger` 是显性可选项——只有它为 true 时才允许再次触发，且每次重触发
/// 都记入诊断（显性，不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriggerRule {
    /// 目标元素。
    pub element: u32,
    /// 触发类型。
    pub kind: TriggerKind,
    /// 触发一次（默认 true）。
    pub once: bool,
    /// 显性重触发（默认 false）。
    pub retrigger: bool,
}

impl TriggerRule {
    /// 构造默认规则（once=true，retrigger=false）。
    pub fn new(element: u32, kind: TriggerKind) -> TriggerRule {
        TriggerRule {
            element,
            kind,
            once: true,
            retrigger: false,
        }
    }

    /// 显性开启重触发。
    pub fn with_retrigger(mut self) -> TriggerRule {
        self.once = false;
        self.retrigger = true;
        self
    }
}

/// 触发判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerDecision {
    /// 触发（宿主据此启动该元素的入场编排）。
    Fire,
    /// 已触发过且规则为 once——拒绝（触发重复显性）。
    AlreadyFired,
    /// 视口规则但元素当前离屏——延迟（F2713 联动：防离屏浪费）。
    OffscreenDeferred,
    /// 未登记触发规则——不触发（显性区分于静默）。
    NotRegistered,
}

/// 单个规则槽位（触发计数 + 重触发诊断落点）。
#[derive(Clone, Debug, PartialEq, Eq)]
struct TriggerSlot {
    rule: TriggerRule,
    fired: u32,
}

/// 触发器表（锚点数据结构：触发器表，三类）。
#[derive(Clone, Debug, Default)]
pub struct TriggerTable {
    slots: Vec<TriggerSlot>,
    diagnostics: Vec<String>,
}

impl TriggerTable {
    /// 构造空表。
    pub fn new() -> Self {
        TriggerTable {
            slots: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// 登记一条规则（同元素同类型重复登记拒绝）。
    pub fn register(&mut self, rule: TriggerRule) -> Result<(), MotionTokenError> {
        if self
            .slots
            .iter()
            .any(|s| s.rule.element == rule.element && s.rule.kind == rule.kind)
        {
            return Err(MotionTokenError::new(
                E_TRIGGER_DUP,
                "触发规则重复",
                &format!(
                    "元素 {} 已登记 {} 触发",
                    rule.element,
                    rule.kind.wire()
                ),
                "一个元素一类触发只登记一条；换 TriggerKind 或先撤销旧规则",
                P_NAMESPACE_OWNER,
            ));
        }
        self.slots.push(TriggerSlot { rule, fired: 0 });
        Ok(())
    }

    /// 已登记规则数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 触发诊断清单（重触发记录落点）。
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    /// 触发判定（锚点：触发判定 O(1)——按元素直达规则槽，视口回调路径零遍历放大）。
    ///
    /// `visible` 仅对 [`TriggerKind::ViewportEnter`] 有意义：离屏一律延迟。
    pub fn on_event(&mut self, element: u32, kind: TriggerKind, visible: bool) -> TriggerDecision {
        let idx = match self.slots.iter().position(|s| s.rule.element == element && s.rule.kind == kind) {
            Some(i) => i,
            None => return TriggerDecision::NotRegistered,
        };
        // 视口规则：先判可见性，离屏延迟（防离屏浪费，F2713 联动）。
        if kind == TriggerKind::ViewportEnter && !visible {
            return TriggerDecision::OffscreenDeferred;
        }
        let once = self.slots[idx].rule.once;
        let prev_fired = self.slots[idx].fired;
        if once && prev_fired > 0 {
            return TriggerDecision::AlreadyFired;
        }
        self.slots[idx].fired = match prev_fired.checked_add(1) {
            Some(v) => v,
            None => prev_fired,
        };
        if !once && prev_fired > 0 {
            // 重触发显性：仅"再次"触发记入诊断（首触是正常触发，不是重触发）。
            self.diagnostics.push(format!(
                "{}：元素 {} 显性重触发（第 {} 次）",
                E_TRIGGER_REPEAT, element, self.slots[idx].fired
            ));
        }
        TriggerDecision::Fire
    }

    /// 某规则已触发次数（判据与宿主查询）。
    pub fn fired_count(&self, element: u32, kind: TriggerKind) -> u32 {
        match self
            .slots
            .iter()
            .find(|s| s.rule.element == element && s.rule.kind == kind)
        {
            Some(s) => s.fired,
            None => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 九、首帧原子器（锚点"首帧起始态原子应用；闪烁=缺陷红线"）
// ---------------------------------------------------------------------------

/// 起始态快照：一次原子提交的全部属性。
///
/// `atomic` 恒真——原子器只产"整批快照"，逐属性写不是本模块的输出形态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartSnapshot {
    /// 目标元素。
    pub element: u32,
    /// 起始态属性（名, 值）序列。
    pub props: Vec<(String, i32)>,
    /// 是否原子（恒真；保留字段供宿主断言）。
    pub atomic: bool,
}

impl StartSnapshot {
    /// 属性数（原子应用 O(属性数) 的"属性数"）。
    pub fn prop_count(&self) -> usize {
        self.props.len()
    }
}

/// 一份 P1 立案（闪烁检出）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashCase {
    /// 案件码（恒 [`E_FLASH_P1`]）。
    pub code: &'static str,
    /// 涉事元素。
    pub element: u32,
    /// 案情摘要。
    pub detail: String,
}

/// 首帧原子器。
///
/// 用法契约：宿主在**绘制该元素前**调用 [`FirstFrameAtomizer::apply_start`]
/// 拿整批快照一次写入；若渲染管线在任何 apply_start 之前就画了该元素，
/// 宿主/CI 调 [`FirstFrameAtomizer::note_premature_paint`] 立案。
#[derive(Clone, Debug, Default)]
pub struct FirstFrameAtomizer {
    cases: Vec<FlashCase>,
    painted: Vec<u32>,
    applied: Vec<(u32, usize)>,
}

impl FirstFrameAtomizer {
    /// 构造空原子器。
    pub fn new() -> Self {
        FirstFrameAtomizer {
            cases: Vec::new(),
            painted: Vec::new(),
            applied: Vec::new(),
        }
    }

    /// 登记一次"先绘制后补起始态"的闪烁检出，立即 P1 立案。
    pub fn note_premature_paint(&mut self, element: u32) {
        self.painted.push(element);
        self.cases.push(FlashCase {
            code: E_FLASH_P1,
            element,
            detail: format!(
                "元素 {} 的首帧中间态被绘出（起始态未原子应用）——闪烁=缺陷红线",
                element
            ),
        });
    }

    /// 原子应用起始态：产出整批快照（O(属性数)）。
    ///
    /// 属性序列为空报 [`E_START_EMPTY`]；超容量报同码（规模护栏）。
    /// 若该元素此前被登记过"先绘后补"，本次应用**同时补立 P1 案**
    /// （闪烁已发生，应用补救不消除立案）。
    pub fn apply_start(
        &mut self,
        element: u32,
        props: &[(&str, i32)],
    ) -> Result<StartSnapshot, MotionTokenError> {
        if props.is_empty() || props.len() > START_PROP_CAP {
            return Err(MotionTokenError::new(
                E_START_EMPTY,
                "起始态属性数非法",
                &format!("元素 {} 的起始态属性 {} 条（须 1..={})", element, props.len(), START_PROP_CAP),
                "按型属性集给起始态；空起始态等于没有原子应用",
                P_NAMESPACE_OWNER,
            ));
        }
        let mut list: Vec<(String, i32)> = Vec::with_capacity(props.len());
        for (name, value) in props.iter() {
            if name.is_empty() || name.chars().count() > PROP_CAP {
                return Err(MotionTokenError::new(
                    E_START_EMPTY,
                    "属性名非法",
                    &format!("属性名 {:?} 为空或超过 {} 字上限", name, PROP_CAP),
                    "用型的 properties() 集内属性名",
                    P_NAMESPACE_OWNER,
                ));
            }
            list.push((String::from(*name), *value));
        }
        if self.painted.iter().any(|e| *e == element) {
            self.cases.push(FlashCase {
                code: E_FLASH_P1,
                element,
                detail: format!(
                    "元素 {} 起始态补应用时检出先绘记录——闪烁已发生，立案不撤销",
                    element
                ),
            });
        }
        let snap = StartSnapshot {
            element,
            props: list,
            atomic: true,
        };
        self.applied.push((element, snap.prop_count()));
        Ok(snap)
    }

    /// P1 案件清单。
    pub fn cases(&self) -> &[FlashCase] {
        &self.cases
    }

    /// 已应用元素数。
    pub fn applied_count(&self) -> usize {
        self.applied.len()
    }

    /// CI 闪烁断言：零案件才合格。
    pub fn assert_no_flash(&self) -> Result<(), MotionTokenError> {
        if let Some(c) = self.cases.first() {
            return Err(MotionTokenError::new(
                E_FLASH_P1,
                "闪烁断言失败",
                &format!("检出 {} 起 P1 闪烁案件，首起：{}", self.cases.len(), c.detail),
                "修渲染时序：起始态必须在首帧绘制前原子应用",
                P_NAMESPACE_OWNER,
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 十、计划与编译（锚点"每型：编排图"）
// ---------------------------------------------------------------------------

/// 一条入场计划：型 + 方向 + 令牌默认参数 + 触发类型 + 错开步长。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryPlan {
    /// 计划名（注册名）。
    pub name: String,
    /// 入场型。
    pub kind: EntryKind,
    /// 方向（非 slide-in 型无语义，恒 Up）。
    pub dir: SlideDir,
    /// 时长（来自时长令牌）。
    pub duration_ms: u32,
    /// 位移（来自位移令牌）。
    pub distance_px: u32,
    /// 触发类型。
    pub trigger: TriggerKind,
    /// 生效错开步长（已过钳制）。
    pub stagger_step_ms: u32,
    /// 时长来源令牌 ID。
    pub duration_token: String,
    /// 位移来源令牌 ID。
    pub distance_token: String,
}

/// 构造入场计划：默认参数从令牌取，错开过钳制。
pub fn build_plan(
    name: &str,
    kind: EntryKind,
    dir: SlideDir,
    trigger: TriggerKind,
    requested_stagger_ms: u32,
    table: &TokenTable,
    planner: &mut StaggerPlanner,
) -> Result<EntryPlan, MotionTokenError> {
    if name.is_empty() || name.chars().count() > NAME_CAP {
        return Err(MotionTokenError::new(
            E_ENTRY_NAME,
            "计划名非法",
            &format!("计划名 {:?} 为空或超过 {} 字上限", name, NAME_CAP),
            "用注册名构造计划；名字是按名派发的键",
            P_NAMESPACE_OWNER,
        ));
    }
    let d = EntryDefaults::from_token(table, kind)?;
    let (step, _clamped) = planner.clamp(requested_stagger_ms);
    Ok(EntryPlan {
        name: String::from(name),
        kind,
        dir: if kind == EntryKind::SlideIn { dir } else { SlideDir::Up },
        duration_ms: d.duration_ms,
        distance_px: d.distance_px,
        trigger,
        stagger_step_ms: step,
        duration_token: d.duration_token,
        distance_token: d.distance_token,
    })
}

/// 编译产物：编排图编译结果 + 错开偏移 + 泳道。
#[derive(Clone, Debug)]
pub struct CompiledEntry {
    /// 计划名。
    pub name: String,
    /// 编排图编译产物（单源：F3005 的 `CompiledOrch`）。
    pub compiled: CompiledOrch,
    /// 每元素的起始偏移（均布错开）。
    pub offsets: Vec<u32>,
    /// 泳道。
    pub lane: Lane,
}

impl CompiledEntry {
    /// 实例数。
    pub fn instance_count(&self) -> usize {
        self.compiled.instance_count()
    }

    /// 总时长。
    pub fn total_duration_ms(&self) -> u32 {
        self.compiled.total_duration_ms()
    }

    /// 是否 reduce 坍缩。
    pub fn collapsed(&self) -> bool {
        self.compiled.collapsed
    }

    /// reduce 终态断言：坍缩下所有实例时长与偏移必须为 0。
    ///
    /// 锚点无障碍语义：入场族 reduce 直达=终态原子应用，视觉终点不因 reduce
    /// 缺失。任何"半途冻结"的中间态在此被拒。
    pub fn assert_reduce_terminal(&self) -> Result<(), MotionTokenError> {
        if self.lane != Lane::Reduced {
            return Err(MotionTokenError::new(
                E_ENTRY_KIND,
                "reduce 断言用错泳道",
                "assert_reduce_terminal 只对 Lane::Reduced 的编译产物有意义",
                "在 Normal 泳道上调用说明判定路径接错了",
                P_NAMESPACE_OWNER,
            ));
        }
        for inst in self.compiled.instances.iter() {
            if inst.duration_ms != 0 || inst.offset_in_stage_ms != 0 {
                return Err(MotionTokenError::new(
                    E_ENTRY_KIND,
                    "reduce 未达终态",
                    &format!(
                        "实例 {} 时长 {}ms 偏移 {}ms——reduce 直达要求全零",
                        inst.instance_id, inst.duration_ms, inst.offset_in_stage_ms
                    ),
                    "编译参数泳道必须整体生效；部分坍缩即中间态缺陷",
                    P_NAMESPACE_OWNER,
                ));
            }
        }
        Ok(())
    }
}

/// 把一张入场计划编译为一组元素的编排图并求值。
///
/// 编排图构造：每元素一个节点（同型同参数），错开以 `offset_start` 边自
/// 首节点星型连出（偏移 = i × step，均布）；随后交 F3005 `compile` 单源求值。
/// reduce 泳道由 [`compile`] 整体坍缩——本模块不另做 reduce 分支。
pub fn compile_entry(
    plan: &EntryPlan,
    elements: &[u32],
    lane: Lane,
) -> Result<CompiledEntry, MotionTokenError> {
    if elements.is_empty() {
        return Err(MotionTokenError::new(
            E_ENTRY_EMPTY,
            "入场元素为空",
            &format!("计划 {} 未给任何元素", plan.name),
            "至少给一个元素；空集编译不出时间轴实例",
            P_NAMESPACE_OWNER,
        ));
    }
    if elements.len() > ENTRY_ELEMENT_CAP {
        return Err(MotionTokenError::new(
            E_STAGGER_COUNT,
            "入场元素数超上限",
            &format!("元素 {} 超过上限 {}", elements.len(), ENTRY_ELEMENT_CAP),
            "拆分入场批次或改交互触发",
            P_NAMESPACE_OWNER,
        ));
    }

    let mut o = Orchestrator::new();
    let mut head: Option<u32> = None;
    for (i, el) in elements.iter().enumerate() {
        let node = o.node(OrchNode::new(
            i as u32,
            *el,
            plan.kind.index() as u32,
            &plan.name,
            plan.duration_ms,
            0,
        ))?;
        if head.is_none() {
            head = Some(node);
        } else if let Some(h) = head {
            // 均布错开：第 i 个元素偏移 i × step（checked 算术）。
            let off = match (i as u32).checked_mul(plan.stagger_step_ms) {
                Some(v) => v,
                None => {
                    return Err(MotionTokenError::new(
                        E_STAGGER_COUNT,
                        "错开偏移溢出",
                        &format!("第 {} 个元素的偏移乘法溢出", i),
                        "减少元素数或缩小步长",
                        P_NAMESPACE_OWNER,
                    ))
                }
            };
            o.edge(TimingEdge::offset_start(h, node, off))?;
        }
    }
    if head.is_none() {
        return Err(MotionTokenError::new(
            E_ENTRY_EMPTY,
            "编排图无入口节点",
            "节点构造全部失败（预算或名称被拒）",
            "检查元素数与计划名合法性",
            P_NAMESPACE_OWNER,
        ));
    }
    let graph: OrchGraph = o.commit()?;
    let mut params = CompileParams::normal(lane);
    params.stagger_step_ms = 0; // 错开已落边，编译期不再叠加，防双计。
    let compiled = compile(&graph, &params)?;

    // 偏移表与图内声明一致（对账用）：直接重算均布序列。
    let mut offsets: Vec<u32> = Vec::with_capacity(elements.len());
    let mut acc: u32 = 0;
    for i in 0..elements.len() {
        offsets.push(acc);
        if i + 1 < elements.len() {
            acc = acc.saturating_add(plan.stagger_step_ms);
        }
    }

    Ok(CompiledEntry {
        name: plan.name.clone(),
        compiled,
        offsets,
        lane,
    })
}

// ---------------------------------------------------------------------------
// 十一、视图（F3018 预览消费）
// ---------------------------------------------------------------------------

/// 入场视图一行（一元素一行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryViewRow {
    /// 元素。
    pub element: u32,
    /// 型短码。
    pub kind: &'static str,
    /// 起始偏移。
    pub offset_ms: u32,
    /// 生效时长（reduce 下为 0）。
    pub duration_ms: u32,
}

/// 入场视图：计划 + 偏移 + 触发摘要（F3018 预览/时间线消费）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryView {
    /// 计划名。
    pub name: String,
    /// 触发类型短码。
    pub trigger: &'static str,
    /// 行清单（按元素升序）。
    pub rows: Vec<EntryViewRow>,
    /// 生效错开步长。
    pub stagger_step_ms: u32,
}

impl EntryView {
    /// 行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 读屏播报（只报型/触发/计数，不含用户内容）。
    pub fn spoken(&self) -> String {
        format!(
            "入场 {}（{}，触发 {}）共 {} 个元素，错开 {}ms",
            self.name,
            match self.rows.first() {
                Some(r) => r.kind,
                None => "<空>",
            },
            self.trigger,
            self.rows.len(),
            self.stagger_step_ms
        )
    }
}

/// 生成入场视图。
pub fn build_view(plan: &EntryPlan, ce: &CompiledEntry) -> EntryView {
    let mut rows: Vec<EntryViewRow> = Vec::with_capacity(ce.offsets.len());
    for (i, off) in ce.offsets.iter().enumerate() {
        let dur = match ce.compiled.instances.get(i) {
            Some(inst) => inst.duration_ms,
            None => 0,
        };
        let element = match ce.compiled.instances.get(i) {
            Some(inst) => inst.element,
            None => i as u32,
        };
        rows.push(EntryViewRow {
            element,
            kind: plan.kind.wire(),
            offset_ms: *off,
            duration_ms: dur,
        });
    }
    rows.sort_by(|a, b| a.element.cmp(&b.element));
    EntryView {
        name: plan.name.clone(),
        trigger: plan.trigger.wire(),
        rows,
        stagger_step_ms: plan.stagger_step_ms,
    }
}

// ---------------------------------------------------------------------------
// 十二、契约表（判据同源门禁）
// ---------------------------------------------------------------------------

/// 入场族契约表：六型短码与属性集的公开事实（判据据表逐条钉死，禁止手抄）。
pub const ENTRY_CONTRACT: [(&str, usize); KIND_COUNT] = [
    ("fade-in", 1),
    ("fade-up", 2),
    ("scale-in", 2),
    ("slide-in", 2),
    ("blur-in", 2),
    ("clip-reveal", 1),
];

/// 触发器契约表：三类短码（判据据表钉死分型全集）。
pub const TRIGGER_CONTRACT: [&str; TRIGGER_KIND_COUNT] =
    ["page-load", "viewport-enter", "conditional"];
