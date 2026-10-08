//! VE-F3406 · 令牌覆盖层（四级覆盖）（VE-E 域 · 主题与个性化引擎 · 令牌运行时组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3406`
//!
//! **判据（锚点原文）**：四级栈、优先级仲裁、来源审计、越级警告、判据。
//!
//! **职责定位（锚点原文）**：四级覆盖体系（默认→主题→场景→组件），覆盖优先级仲裁，
//! 覆盖来源审计（谁在什么层级改了什么可追溯）。
//!
//! **数据结构（锚点原文）**：覆盖栈；仲裁器；审计。
//!
//! ## 一、四级为什么是"栈"而不是"一张平表"
//!
//! 朴素写法是把所有覆盖塞进一张 `path -> value` 表，谁后写谁赢。那样写出来的
//! 系统有三个致命问题：
//!
//! 1. **看不见覆盖来自哪一层**。表里只剩最终值，"这个红是主题改的还是组件改的"
//!    答不出来 —— 而"谁在什么层级改了什么可追溯"是锚点明写的要求。
//! 2. **改不回去**。用户在组件里临时调了一次色值（组件层），主题切换后组件层的
//!    残留值会**赢过**新主题，因为它是后写的。平表没有"回落到上一层"的概念。
//! 3. **低层写入静默失效**。默认层在主题定稿之后再被改，单层表看不出这件事，
//!    作者以为改了、实际被主题层压住，永远找不到原因。
//!
//! 所以本模块把每条路径拆成**四个槽位**（[`Level`] × 1），仲裁发生在**读时刻**
//! 而不是写时刻：写入只负责占位与记账，胜出者由 [`OverlayStack::resolve`] 按
//! 层级 rank 现算。写时刻就定胜负的表，无法回答"如果把组件层清掉会落到哪一层"。
//!
//! ## 二、优先级仲裁的规则只有一条，但它必须可预测
//!
//! **层级 rank 高者胜**（组件 3 > 场景 2 > 主题 1 > 默认 0）。同层重复写同一路径
//! 属**冲突**，仲裁规则是**同层后来者胜** —— 与 CSS 自定义属性的重定义同序，
//! 与"先声明者为准"的配置文件语义相反。选后者是有理由的：覆盖层的使用者是
//! **组件作者**，组件作者在同一个文件里连写两次同一个路径时，第二次通常是
//! 修错了第一次（想改值却忘了删旧行），让修正生效比让笔误生效更符合意图。
//!
//! 关键性质：**仲裁是纯函数**（只读槽位，不改状态）。所以"读一次resolve 两次
//! 结果必然相同"，且 resolve 不产生审计 —— 审计只记**写**，不记**读**。
//! 读若也记审计，审计条数会随读次数膨胀，"谁改了什么"就答不干净了。
//!
//! ## 三、越级覆盖为什么是"警告 + 记录但不生效"，而不是"拒绝"
//!
//! [`OverlayStack::freeze_to`] 把某层以上**定稿**。定稿后再往更低层写覆盖，
//! 就是**越级覆盖**：值合法、路径合法、来源合法，它只是**永远不会生效**。
//!
//! 为什么不拒绝？因为拒绝的话作者得到的是"写入失败"，会去查写入 API；
//! 而真实的失败发生在**渲染时**（值看起来没变），作者查不到写入上。记录 +
//! 标记 [`AuditFlags::SHADOWED`] + 警告三者同时给，"我写了但它被压住了"这件事
//! 就在**写入点**当场可见，且事后可从审计里查回"是谁越级改了什么"。
//!
//! 越级条目**仍然入栈**：删掉它会让"这个路径曾经被越级改过"这段历史消失，
//! 而审计的价值恰恰在于历史。代价是 resolve 必须显式跳过 shadowed 槽位 ——
//! 判据 [`ver01f_checks`] 用"shadowed 条目在栈内且 resolve 读不到"双向钉住。
//!
//! ## 四、来源不明为什么是"审计告警"而不是"拒绝"
//!
//! 覆盖的来源标识必须先在 [`OverlayStack::register_source`] 登记。没登记就写
//! 覆盖 → [`OverlayCode::SourceUnknown`] 告警，覆盖**仍然生效**。理由与
//! F3405 的"单位混用 → 告警"同源：来源不明是**记账问题**，不是**取值问题** ——
//! 值本身通过 F3405 的类型校验就能用，拒掉它等于让第三方扩展（E14 对接点）
//! 因为一个字段没填就整个失效，那会让对接点变成死路。
//!
//! 但记账缺失必须**显性化**：未登记来源的覆盖在审计里带
//! [`AuditFlags::UNAUDITED`]，报告单列计数。它可能被渲染，但它是"欠账的"。
//!
//! ## 五、审计是"谁·何时·哪层·什么路径·什么动作"，**不含覆盖值正文**
//!
//! 锚点要"覆盖来源读屏可查"。可查的是**来源与层级**（组件层 / 主题包 X 改的），
//! 不是**值** —— 值可能来自用户自定义字符串，进读屏即隐私泄露。读屏文本只播
//! 层级、来源标识、路径与动作。判据用值集里真实存在的字面量做反例。
//!
//! 动作有四种，对应四条降级路径（锚点"错误路径与降级矩阵"）：
//! - [`AuditAction::Cover`]：正常覆盖。
//! - [`AuditAction::Superseded`]：同层被后来者压掉（**冲突 → 优先级仲裁**）。
//! - [`AuditAction::ShadowedCover`]：越级写入，记录但不生效（**越级 → 警告**）。
//! - [`AuditAction::UnauditedCover`]：来源未登记（**来源不明 → 审计告警**）。
//!
//! ## 六、性能逐项分解（锚点原文 O(覆盖数)）
//!
//! - [`OverlayStack::push`]：O(路径数) 线性定位槽位 + O(1) 占位。
//! - [`OverlayStack::resolve`]：O(路径数) 线性定位槽位 + **O(1)** 仲裁（四个槽
//!   比大小，不扫描）。仲裁本身与覆盖总数无关。
//! - [`OverlayStack::audit_of`]：O(审计数) 线性扫描（审计是 append-only）。
//! - [`OverlayStack::report`]：O(审计数 + 槽位数)，**只在生成报告时付**。
//!
//! 三者都不超过锚点给的 O(覆盖数)：路径数 ≤ 覆盖数（每路径至少一条覆盖），
//! 审计数 ≤ 2×覆盖数（每次 push 至多两条：被压者 + 新覆盖）。**零墙钟、零 IO、
//! 无随机源**，时间与并发均不参与，回归可复现。
//!
//! ## 七、跨批对接点（锚点原文 E14 第三方扩展边界）
//!
//! 交出两件：
//! 1. [`OverlayStack::export_bindings`] —— 把生效覆盖导出为 F3405 的
//!    [`Binding`](super::ver01e_typetree::Binding) 列表，由 F3405 的
//!    [`TypeChecker`](super::ver01e_typetree::TypeChecker) 做类型校验。
//!    **本模块不重写类型判断**：覆盖层只管"哪条生效"，"这条合法吗"归类型系统。
//! 2. [`OverlayReport`] —— 层条目分布、越级数、来源不明数、遮蔽数，E14 拿它
//!    决定第三方扩展的覆盖是否准入。
//!
//! 组件→令牌的作用域绑定归 E14 与后续单，本模块只做层级仲裁，**不做作用域**。
//!
//! ## 八、零 panic 面
//!
//! 解析面与仲裁面零 `unwrap`/零直接索引越界：所有下标访问前都有守卫，越界
//! 走**返回错误码**而不是 panic。判据里对 `checks` 数组的直接下标访问只存在于
//! `run_*_checks()` 内（见 [`ver01f_checks`]），符合内核戒律。

use super::ver01b_parser::Site;
use super::ver01e_typetree::Binding;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 覆盖层版本。覆盖语义变更走版本号（E14 按版本决定是否重审扩展覆盖）。
pub const OVERLAY_VERSION: &str = "E01-overlay-v1";

/// 路径槽上限（覆盖路径数上界）。
pub const MAX_SLOTS: usize = 4096;

/// 生效覆盖条数上限。**超限整批拒**，不做截断（截断会让后写的覆盖凭空消失，
/// 而作者无从知道哪几条被丢掉了）。
pub const MAX_OVERRIDES: usize = 16384;

/// 来源标识字节上限。
pub const MAX_SOURCE_LEN: usize = 64;

/// 覆盖值字节上限。
pub const MAX_VALUE_LEN: usize = 512;

/// 路径字节上限（与 F3405 的 `MAX_PATH_LEN` 同量级，本单独立持有以免跨单耦合）。
pub const MAX_PATH_LEN: usize = 256;

/// 审计条数上限。**每次 push 至多产两条**（被压者 + 新覆盖），故上界 = 2×MAX。
pub const MAX_AUDIT: usize = MAX_OVERRIDES * 2;

/// 层级数（四级）。
pub const LEVEL_COUNT: usize = 4;

/// 路径最小长度（空路径无法定位，恒拒绝）。
pub const PATH_MIN_LEN: usize = 1;

/// 期望说明 / 实得说明的最小长度（空文案在诊断里等于没说明）。
pub const NOTE_MIN_LEN: usize = 1;

/// 动作标签最小长度。
pub const ACTION_MIN_LEN: usize = 1;

/// 读屏播报里单条审计的最长截断长度（读屏念超长文本不可用）。
pub const SPOKEN_ITEM_MAX: usize = 96;

// ---------------------------------------------------------------------------
// 二、层级（四级栈）
// ---------------------------------------------------------------------------

/// 覆盖层级。**rank 越大优先级越高**，与锚点的"默认→主题→场景→组件"顺序一致。
///
/// 判别值刻意与 rank 不同（[`Level::DEFAULT`] 的判别值是 0 而 rank 是 0，
/// 但 [`Level::SCENE`] 判别值 2 / rank 2 之上还有 [`Level::COMPONENT`] 判别值 3）：
/// 线上编码只走 [`Level::wire`]，**禁止 `enum_val as u8`** —— 那会让"加一层"
/// 直接改变线上协议。本模块没有需要判别值的地方，索性不给判别值，让编译期
/// 强制走显式映射。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// 默认层（基底）。rank 0。
    Default,
    /// 主题层。rank 1。
    Theme,
    /// 场景层。rank 2。
    Scene,
    /// 组件层。rank 3。
    Component,
}

impl Level {
    /// 四级全集（封闭表，顺序即 rank 升序）。
    pub const ALL: [Level; LEVEL_COUNT] = [Level::Default, Level::Theme, Level::Scene, Level::Component];

    /// 线上短码（稳定协议面）。
    pub fn wire(self) -> &'static str {
        match self {
            Level::Default => "dflt",
            Level::Theme => "thm",
            Level::Scene => "scn",
            Level::Component => "cmp",
        }
    }

    /// 中文名（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            Level::Default => "默认层",
            Level::Theme => "主题层",
            Level::Scene => "场景层",
            Level::Component => "组件层",
        }
    }

    /// 优先级 rank（0..=3）。
    pub fn rank(self) -> u8 {
        match self {
            Level::Default => 0,
            Level::Theme => 1,
            Level::Scene => 2,
            Level::Component => 3,
        }
    }

    /// 按 rank 反查层级。**越界返回 [`None`]**，不 panic 也不折回。
    pub fn from_rank(r: u8) -> Option<Level> {
        match r {
            0 => Some(Level::Default),
            1 => Some(Level::Theme),
            2 => Some(Level::Scene),
            3 => Some(Level::Component),
            _ => None,
        }
    }

    /// 按线上短码解析。**大小写敏感**：短码是协议不是自然语言，
    /// `"DFLT"` 与 `"dflt"` 混用会让审计记录无法与文件逐字对应。
    pub fn parse(s: &str) -> Result<Level, OverlayCode> {
        for k in Level::ALL.iter() {
            if s == k.wire() {
                return Ok(*k);
            }
        }
        Err(OverlayCode::UnknownLevel)
    }
}

/// 两个层级的仲裁：**rank 高者胜**。rank 相等不可能（层级是封闭枚举），
/// 但函数仍返回 [`Option`]：判据要能对"无候选"与"有候选"分别钉。
///
/// 写成函数而不是内联比较，是为了让判据能**独立重算**期望值而不问被测方
/// —— 直接内联 `a.rank() > b.rank()` 的话，判据复算就变成同源驱动恒真。
pub fn arbitrate(a: Level, b: Level) -> Option<Level> {
    if a.rank() >= b.rank() {
        Some(a)
    } else {
        Some(b)
    }
}

// ---------------------------------------------------------------------------
// 三、覆盖来源
// ---------------------------------------------------------------------------

/// 一个已登记的覆盖来源（"谁"）。登记后才能作为覆盖的来源标识。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Source {
    /// 来源标识（作者名 / 主题包 id / 扩展 id）。**非空且有上界**。
    pub id: String,
    /// 是否为第三方扩展（E14 对接点）。扩展来源在报告里单列计数。
    pub third_party: bool,
}

impl Source {
    /// 构造来源。标识越界或为空 → [`OverlayCode::SourceIdInvalid`]。
    pub fn new(id: &str, third_party: bool) -> Result<Source, OverlayCode> {
        if id.is_empty() || id.len() > MAX_SOURCE_LEN {
            return Err(OverlayCode::SourceIdInvalid);
        }
        Ok(Source {
            id: String::from(id),
            third_party,
        })
    }

    /// 读屏播报。**只念标识与身份类别，不念任何值**。
    pub fn spoken(&self) -> String {
        let mut s = String::from("来源 ");
        s.push_str(&self.id);
        if self.third_party {
            s.push_str("（第三方扩展）");
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 四、错误码（自建诊断码：下游封闭枚举无权加变体）
// ---------------------------------------------------------------------------

/// 覆盖层诊断码。分两级：拒绝级（[`OverlayCode::is_reject`]）与告警级。
///
/// 两级的划分依据锚点降级矩阵：
/// - **拒绝级**：写入本身不合法或超出能力边界（路径空、值空、超限、层已定稿）。
/// - **告警级**：写入合法但**记账或生效上有欠缺**（越级、来源不明）。
///   告警级不阻止生效 —— 见头注第三、四节。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverlayCode {
    /// 层级短码不在封闭表内。
    UnknownLevel,
    /// 路径为空或超长。
    PathInvalid,
    /// 覆盖值为空或超长。
    ValueInvalid,
    /// 来源标识为空或超长。
    SourceIdInvalid,
    /// 来源重复登记（同一 id 登记两次）。
    SourceDup,
    /// 该路径的槽位已满（槽位数超上限）。
    SlotLimit,
    /// 生效覆盖条数超上限。**整批拒，不截断**。
    OverrideLimit,
    /// 审计条数超上限。
    AuditLimit,
    /// 请求的层级高于已定稿层级（试图改动已定稿的层）。
    LevelFrozen,
    /// 诊断三要素缺失（路径/期望/实得有空）。
    DiagIncomplete,
    /// **越级覆盖**：定稿后仍往更低层写。告警级，记录但不生效。
    LayerOutOfOrder,
    /// **来源不明**：来源未登记。告警级，生效但审计标记为欠账。
    SourceUnknown,
    /// resolve 未命中（该路径无任何生效覆盖）。这是查询结果，不是写入错误。
    NotFound,
}

impl OverlayCode {
    /// 线上短码（稳定协议面）。
    pub fn wire(self) -> &'static str {
        match self {
            OverlayCode::UnknownLevel => "E06-LEVEL-UNKNOWN",
            OverlayCode::PathInvalid => "E06-PATH-INVALID",
            OverlayCode::ValueInvalid => "E06-VALUE-INVALID",
            OverlayCode::SourceIdInvalid => "E06-SOURCE-ID-INVALID",
            OverlayCode::SourceDup => "E06-SOURCE-DUP",
            OverlayCode::SlotLimit => "E06-SLOT-LIMIT",
            OverlayCode::OverrideLimit => "E06-OVERRIDE-LIMIT",
            OverlayCode::AuditLimit => "E06-AUDIT-LIMIT",
            OverlayCode::LevelFrozen => "E06-LEVEL-FROZEN",
            OverlayCode::DiagIncomplete => "E06-DIAG-INCOMPLETE",
            OverlayCode::LayerOutOfOrder => "E06-LAYER-OUT-OF-ORDER",
            OverlayCode::SourceUnknown => "E06-SOURCE-UNKNOWN",
            OverlayCode::NotFound => "E06-NOT-FOUND",
        }
    }

    /// 中文说明（读屏用）。
    pub fn zh(self) -> &'static str {
        match self {
            OverlayCode::UnknownLevel => "覆盖层级不在四级封闭表内",
            OverlayCode::PathInvalid => "令牌路径为空或超长",
            OverlayCode::ValueInvalid => "覆盖值为空或超长",
            OverlayCode::SourceIdInvalid => "来源标识为空或超长",
            OverlayCode::SourceDup => "来源标识重复登记",
            OverlayCode::SlotLimit => "覆盖路径槽位已满",
            OverlayCode::OverrideLimit => "生效覆盖条数超上限，整批拒绝",
            OverlayCode::AuditLimit => "审计条数超上限",
            OverlayCode::LevelFrozen => "该层已定稿，不可改写",
            OverlayCode::DiagIncomplete => "诊断三要素缺失",
            OverlayCode::LayerOutOfOrder => "越级覆盖：定稿后仍往更低层写入",
            OverlayCode::SourceUnknown => "覆盖来源未登记，审计欠账",
            OverlayCode::NotFound => "该路径无生效覆盖",
        }
    }

    /// 是否为拒绝级。**告警级返回 false** —— 告警不阻止写入生效。
    pub fn is_reject(self) -> bool {
        !matches!(
            self,
            OverlayCode::LayerOutOfOrder | OverlayCode::SourceUnknown | OverlayCode::NotFound
        )
    }
}

// ---------------------------------------------------------------------------
// 五、审计
// ---------------------------------------------------------------------------

/// 审计动作。四种动作对应四条降级路径（头注第五节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditAction {
    /// 正常覆盖（占位成功）。
    Cover,
    /// 同层被后来者压掉（冲突 → 优先级仲裁）。
    Superseded,
    /// 越级写入，记录但不生效（越级 → 警告）。
    ShadowedCover,
    /// 来源未登记，生效但审计欠账（来源不明 → 审计告警）。
    UnauditedCover,
}

impl AuditAction {
    /// 中文说明。
    pub fn zh(self) -> &'static str {
        match self {
            AuditAction::Cover => "覆盖",
            AuditAction::Superseded => "被同层后来者压掉",
            AuditAction::ShadowedCover => "越级覆盖未生效",
            AuditAction::UnauditedCover => "来源未登记的覆盖",
        }
    }

    /// 该动作是否使覆盖**不生效**。仅越级为否。
    ///
    /// 钉这个谓词是因为它就是 resolve 的过滤条件：写成 `!= SHADOWED_COVER`
    /// 时若有人新增第四种"不生效"动作，仲裁会静默把新动作当生效处理。
    pub fn blocks_effect(self) -> bool {
        matches!(self, AuditAction::ShadowedCover)
    }
}

/// 审计标记位（bit 位，避免一个 bool 表达两件事）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditFlags(pub u8);

impl AuditFlags {
    /// 无标记。
    pub const NONE: AuditFlags = AuditFlags(0);
    /// 越级：记录但不生效。
    pub const SHADOWED: AuditFlags = AuditFlags(0b0001);
    /// 来源未登记。
    pub const UNAUDITED: AuditFlags = AuditFlags(0b0010);
    /// 第三方扩展来源。
    pub const THIRD_PARTY: AuditFlags = AuditFlags(0b0100);
    /// 被同层后来者压掉。
    pub const SUPERSEDED: AuditFlags = AuditFlags(0b1000);

    /// 是否含某标记。
    pub fn has(self, f: AuditFlags) -> bool {
        self.0 & f.0 != 0
    }

    /// 置某标记。
    pub fn with(self, f: AuditFlags) -> AuditFlags {
        AuditFlags(self.0 | f.0)
    }
}

/// 一条审计记录：**谁 · 何时 · 哪层 · 什么路径 · 什么动作**。
///
/// **不含覆盖值正文**（锚点要"来源读屏可查"，不是"值读屏可查"；值可能含用户
/// 自定义字符串，进读屏即隐私泄露）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AuditEntry {
    /// 单调递增的写入序号（1 起算）。**不做时间源** —— 回归可复现优先。
    pub seq: u32,
    /// 令牌路径（逐字等于写入时的路径，不做规范化）。
    pub path: String,
    /// 层级。
    pub level: Level,
    /// 来源标识（**逐字保留写入时的字符串，即使来源未登记** —— 改成占位符就
    /// 查不出"是谁越级改的"了，那正是审计要回答的问题）。
    pub source: String,
    /// 动作。
    pub action: AuditAction,
    /// 标记位。
    pub flags: AuditFlags,
    /// 写入点。
    pub site: Site,
}

impl AuditEntry {
    /// 读屏播报一行。顺序固定：序号 → 层级 → 来源 → 路径 → 动作。
    pub fn spoken(&self) -> String {
        format!(
            "第{}条 {} {} 改 {}：{}",
            self.seq,
            self.level.zh(),
            if self.source.is_empty() { "来源未登记" } else { &self.source },
            if self.path.is_empty() { "路径未记录" } else { &self.path },
            self.action.zh()
        )
    }
}

// ---------------------------------------------------------------------------
// 六、路径槽与覆盖条目
// ---------------------------------------------------------------------------

/// 槽内一条覆盖条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Override {
    /// 层级。
    pub level: Level,
    /// 来源标识（未登记时是原始字符串，不做占位替换）。
    pub source: String,
    /// 覆盖值。**不进审计、不进读屏**。
    pub value: String,
    /// 写入点。
    pub site: Site,
    /// 是否被越级标记（记录但不生效）。
    pub shadowed: bool,
    /// 来源是否未登记（审计欠账）。
    pub unaudited: bool,
    /// 是否第三方扩展来源。
    pub third_party: bool,
}

/// 一个令牌路径的四级槽位。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Slot {
    /// 令牌路径。
    pub path: String,
    /// 四个层级的条目（`None` = 该层无覆盖）。下标 = [`Level`] 的 rank。
    pub cells: [Option<Override>; LEVEL_COUNT],
}

impl Slot {
    /// 格子数组可变引用（仅判据构造非法态用，见
    /// [`OverlayStack::slots_mut_for_test`]）。
    pub fn cells_mut_for_test(&mut self) -> &mut [Option<Override>; LEVEL_COUNT] {
        &mut self.cells
    }

    /// 新建空槽。
    pub fn new(path: &str) -> Slot {
        Slot {
            path: String::from(path),
            cells: [None, None, None, None],
        }
    }

    /// 该槽的生效覆盖条数（不含被越级标记的）。
    pub fn live_count(&self) -> usize {
        let mut n = 0usize;
        for i in 0..LEVEL_COUNT {
            if let Some(c) = self.cells[i].as_ref() {
                if !c.shadowed {
                    n += 1;
                }
            }
        }
        n
    }

    /// 该槽的遮蔽条数（越级写入的）。
    pub fn shadowed_count(&self) -> usize {
        let mut n = 0usize;
        for i in 0..LEVEL_COUNT {
            if let Some(c) = self.cells[i].as_ref() {
                if c.shadowed {
                    n += 1;
                }
            }
        }
        n
    }

    /// 该槽的欠账条数（来源未登记的）。
    pub fn unaudited_count(&self) -> usize {
        let mut n = 0usize;
        for i in 0..LEVEL_COUNT {
            if let Some(c) = self.cells[i].as_ref() {
                if c.unaudited {
                    n += 1;
                }
            }
        }
        n
    }

    /// 该槽的扩展来源条数（E14 准入计数）。
    pub fn third_party_count(&self) -> usize {
        let mut n = 0usize;
        for i in 0..LEVEL_COUNT {
            if let Some(c) = self.cells[i].as_ref() {
                if c.third_party {
                    n += 1;
                }
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 七、写入结果
// ---------------------------------------------------------------------------

/// 一次写入的结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PushOutcome {
    /// 写入是否入栈（**告警级也入栈**，只有拒绝级不入栈）。
    pub stored: bool,
    /// 该路径的生效覆盖是否发生**实际变化**（值或层级变了才算变化）。
    pub changed: bool,
    /// 告警码（`None` = 无告警）。拒绝时此字段为 `None`，拒绝码在 `err`。
    pub warn: Option<OverlayCode>,
    /// 拒绝码（`Some` = 未入栈）。
    pub err: Option<OverlayCode>,
    /// 仲裁后的生效层级（`None` = 写入后该路径仍无生效覆盖，例如唯一一条被越级标记）。
    pub effective_level: Option<Level>,
}

impl PushOutcome {
    /// 是否被拒。
    pub fn rejected(&self) -> bool {
        self.err.is_some()
    }
}

// ---------------------------------------------------------------------------
// 八、仲裁结果
// ---------------------------------------------------------------------------

/// 一次仲裁的结果（**只读，不改状态**）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Resolved {
    /// 令牌路径。
    pub path: String,
    /// 胜出层级。
    pub level: Level,
    /// 胜出来源标识。
    pub source: String,
    /// 胜出值。
    pub value: String,
    /// 胜出者是否来自未登记来源。
    pub unaudited: bool,
    /// 胜出者是否来自第三方扩展。
    pub third_party: bool,
    /// 参与仲裁但被压住的层（rank 低于胜出层且未被遮蔽），按 rank 降序。
    pub shadowed_levels: Vec<Level>,
}

impl Resolved {
    /// 读屏播报。**只念来源与层级，不念值**。
    pub fn spoken(&self) -> String {
        let mut s = String::new();
        s.push_str(self.path.as_str());
        s.push_str(" 生效于 ");
        s.push_str(self.level.zh());
        s.push_str("，来源 ");
        if self.source.is_empty() {
            s.push_str("未登记");
        } else {
            s.push_str(&self.source);
            // **未登记来源必须明说"未登记"**：只念来源标识会让听者以为这是
            // 一个已登记的来源（标识本身长得就像正常的），从而把"欠账"漏掉。
            // 标识在审计里逐字保留是有用的，但读屏播报要额外点明它的状态。
            if self.unaudited {
                s.push_str("（未登记）");
            }
        }
        if self.third_party {
            s.push_str("（第三方扩展）");
        }
        if self.unaudited {
            s.push_str("，审计欠账");
        }
        if !self.shadowed_levels.is_empty() {
            s.push_str("，压住 ");
            for (i, l) in self.shadowed_levels.iter().enumerate() {
                if i > 0 {
                    s.push('、');
                }
                s.push_str(l.zh());
            }
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 九、覆盖栈（数据结构锚点：覆盖栈；仲裁器；审计）
// ---------------------------------------------------------------------------

/// 四级覆盖栈。**写入只占位与记账，胜出者由 [`Self::resolve`] 现算**（头注第一节）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OverlayStack {
    /// 已登记来源（append-only，保持登记序以保证报告可复现）。
    sources: Vec<Source>,
    /// 路径槽。**按 path 升序**（与 F3405 的声明表同纪律：顺序表省内存、
    /// 回归可比对，且 `verify()` 能钉住不变式）。
    slots: Vec<Slot>,
    /// 审计流水（append-only）。
    audit: Vec<AuditEntry>,
    /// 已定稿层级：`None` = 未定稿；`Some(l)` = 层级 rank **≤ l 的层已定稿**。
    ///
    /// 语义：`freeze_to(l)` 之后往 rank < l 的层写入 = 越级（告警 + 不生效）。
    frozen: Option<Level>,
    /// 写入序号（下一条审计用的 seq，1 起算）。
    next_seq: u32,
}

impl OverlayStack {
    /// 新建空栈。四级槽全部为空，无来源，无审计。
    pub fn new() -> OverlayStack {
        OverlayStack {
            sources: Vec::new(),
            slots: Vec::new(),
            audit: Vec::new(),
            frozen: None,
            next_seq: 1,
        }
    }

    // -- 来源登记 ----------------------------------------------------------

    /// 登记一个覆盖来源。重复登记 → [`OverlayCode::SourceDup`] 拒绝。
    ///
    /// **登记失败必须真的让后续 push 判为来源不明**：否则"登记失败被吞掉"
    /// 会变成"覆盖悄悄欠账"，而欠账只在审计里体现，没人会去看。
    pub fn register_source(&mut self, id: &str, third_party: bool) -> Result<(), OverlayCode> {
        let s = Source::new(id, third_party)?;
        for e in self.sources.iter() {
            if e.id == s.id {
                return Err(OverlayCode::SourceDup);
            }
        }
        if self.sources.len() >= MAX_SLOTS {
            return Err(OverlayCode::SlotLimit);
        }
        self.sources.push(s);
        Ok(())
    }

    /// 是否已登记某来源。
    pub fn has_source(&self, id: &str) -> bool {
        for e in self.sources.iter() {
            if e.id == id {
                return true;
            }
        }
        false
    }

    /// 已登记来源条数。
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// 已定稿层级（`None` = 未定稿）。
    pub fn frozen_level(&self) -> Option<Level> {
        self.frozen
    }

    // -- 定稿 --------------------------------------------------------------

    /// 把 `level` 及其**更低层**定稿。
    ///
    /// 幂等：重复定稿同一层或定稿更低层都不改变结果（`frozen` 只上不下）。
    /// **不上不下是刻意的**：定稿后要放开低层，等于让已经"压住低层"的那次仲裁
    /// 结果变得不可信 —— 作者会看到"覆盖突然开始生效"却找不到任何改动。
    /// 要放开请重新建栈。
    pub fn freeze_to(&mut self, level: Level) {
        self.frozen = match self.frozen {
            None => Some(level),
            Some(prev) => {
                if level.rank() > prev.rank() {
                    Some(level)
                } else {
                    Some(prev)
                }
            }
        };
    }

    // -- 写入 --------------------------------------------------------------

    /// 写入一条覆盖。这是**唯一**改变生效结果的入口（审计也只记写）。
    ///
    /// 降级分流（锚点三条路径 + 两条越界）：
    /// 1. 路径/值/层级非法 → 拒绝（[`OverlayCode`] 拒绝级），**不入栈**。
    /// 2. 槽位满 / 生效覆盖数超限 → 拒绝，**不入栈**（不截断）。
    /// 3. `level.rank() > frozen.rank()` → 拒绝 [`OverlayCode::LevelFrozen`]：
    ///    定稿的是"这一层及以上"，改它等于改已生效的契约。
    /// 4. `level.rank() < frozen.rank()` → **越级**：[`OverlayCode::LayerOutOfOrder`]
    ///    告警，**入栈但标 shadowed，永不生效**（头注第三节）。
    /// 5. 来源未登记 → **来源不明**：[`OverlayCode::SourceUnknown`] 告警，
    ///    **入栈且生效**，审计带 [`AuditFlags::UNAUDITED`]（头注第四节）。
    /// 6. 同层已有该路径的覆盖 → **冲突**：旧条目标 [`AuditAction::Superseded`]，
    ///    新条目占位（同层后来者胜，头注第二节）。
    ///
    /// 告警不叠加：`warn` 单值，取**更严重**的一条（越级，因为它连生效都
    /// 做不到）；两个标记位在审计里各自独立存在，不因单值告警而丢失。
    pub fn push(
        &mut self,
        level: Level,
        source: &str,
        path: &str,
        value: &str,
        site: Site,
    ) -> PushOutcome {
        // --- 1. 边界防护：先做完全部拒绝级检查，再动任何状态 ---
        // 顺序刻意是"最便宜的守卫在前"：长度检查 O(1)，槽位扫描 O(槽数)。
        // 顺序反了不会错判，但每条越界输入都要白扫一遍全栈。
        if path.len() < PATH_MIN_LEN || path.len() > MAX_PATH_LEN {
            return Self::reject(OverlayCode::PathInvalid);
        }
        if value.len() < PATH_MIN_LEN || value.len() > MAX_VALUE_LEN {
            return Self::reject(OverlayCode::ValueInvalid);
        }
        if source.len() > MAX_SOURCE_LEN {
            return Self::reject(OverlayCode::SourceIdInvalid);
        }
        // 层级合法性：rank 必须能反查回自己（防越界 rank 被当成合法层）。
        match Level::from_rank(level.rank()) {
            Some(l) if l == level => {}
            _ => return Self::reject(OverlayCode::UnknownLevel),
        }
        if self.slots.len() >= MAX_SLOTS {
            return Self::reject(OverlayCode::SlotLimit);
        }
        // 生效覆盖总数守恒：只有**新占一个格子**才增加总数，同层重写不增。
        if self.cell_would_be_new(level, path) && self.live_cell_count() >= MAX_OVERRIDES {
            return Self::reject(OverlayCode::OverrideLimit);
        }
        // 本次 push 至多产两条审计（被压者 + 新覆盖）。
        if self.audit.len() + 2 > MAX_AUDIT {
            return Self::reject(OverlayCode::AuditLimit);
        }
        // --- 3. 定稿边界 ---
        if let Some(f) = self.frozen {
            if level.rank() > f.rank() {
                return Self::reject(OverlayCode::LevelFrozen);
            }
        }

        // --- 4. 越级判定（告警，不拒绝） ---
        let shadowed = match self.frozen {
            Some(f) => level.rank() < f.rank(),
            None => false,
        };
        // --- 5. 来源登记判定 ---
        let unaudited = !self.has_source(source);
        let third_party = !unaudited && self.is_third_party(source);

        // --- 定位/新建槽 ---
        let at = match self.find_slot(path) {
            Some(i) => i,
            None => {
                self.slots.push(Slot::new(path));
                self.slots.sort_by(|a, b| a.path.cmp(&b.path));
                // 排序后必须**重新定位**，否则索引指向的是排序前的位置 ——
                // 这类"插入后忘了重定位"是排序容器最常见的越界来源。
                match self.find_slot(path) {
                    Some(i) => i,
                    // 理论上不可达：新插入的路径必定能找到。返回拒绝而非 panic，
                    // 报告面的零 panic 是硬要求。
                    None => return Self::reject(OverlayCode::PathInvalid),
                }
            }
        };
        let idx = level.rank() as usize;

        // 变化判定必须在**占位前**取"旧生效者"，占位后再取"新生效者"比较。
        // 反过来写（占位后先存新者、然后拿它和自己比）会恒得 changed=false。
        let before = self.resolve(path);

        // --- 6. 冲突：同层已有覆盖，旧条目记 SUPERSEDED ---
        if let Some(old) = self.slots[at].cells[idx].as_ref() {
            let old_src = old.source.clone();
            let old_site = old.site;
            let mut flags = AuditFlags::SUPERSEDED;
            if old.shadowed {
                flags = flags.with(AuditFlags::SHADOWED);
            }
            if old.unaudited {
                flags = flags.with(AuditFlags::UNAUDITED);
            }
            if old.third_party {
                flags = flags.with(AuditFlags::THIRD_PARTY);
            }
            self.push_audit(path, level, &old_src, AuditAction::Superseded, flags, old_site);
        }

        // --- 占位 ---
        self.slots[at].cells[idx] = Some(Override {
            level,
            source: String::from(source),
            value: String::from(value),
            site,
            shadowed,
            unaudited,
            third_party,
        });

        // --- 记新覆盖的审计 ---
        let action = if shadowed {
            AuditAction::ShadowedCover
        } else if unaudited {
            AuditAction::UnauditedCover
        } else {
            AuditAction::Cover
        };
        let mut flags = AuditFlags::NONE;
        if shadowed {
            flags = flags.with(AuditFlags::SHADOWED);
        }
        if unaudited {
            flags = flags.with(AuditFlags::UNAUDITED);
        }
        if third_party {
            flags = flags.with(AuditFlags::THIRD_PARTY);
        }
        self.push_audit(path, level, source, action, flags, site);

        let after = self.resolve(path);
        let changed = match (&before, &after) {
            (Some(b), Some(a)) => {
                a.level != b.level || a.value != b.value || a.source != b.source
            }
            (None, Some(_)) => true,
            // 越级写入的 before/after 都是 None（或都不变）⇒ 不算变化。
            // **这不是 bug**：越级覆盖从不改变生效结果，把它记成 changed=true
            // 会让"生效变化计数"被无效写入污染，E14 拿它判准入就会误判。
            _ => false,
        };
        let effective_level = after.map(|r| r.level);

        let warn = if shadowed {
            Some(OverlayCode::LayerOutOfOrder)
        } else if unaudited {
            Some(OverlayCode::SourceUnknown)
        } else {
            None
        };

        PushOutcome {
            stored: true,
            changed,
            warn,
            err: None,
            effective_level,
        }
    }

    /// 追加一条审计并递增序号。**审计序号在这里集中分配**，避免各调用点
    /// 各自 `next_seq += 1` 造成重复序号。
    fn push_audit(
        &mut self,
        path: &str,
        level: Level,
        source: &str,
        action: AuditAction,
        flags: AuditFlags,
        site: Site,
    ) {
        self.audit.push(AuditEntry {
            seq: self.next_seq,
            path: String::from(path),
            level,
            source: String::from(source),
            action,
            flags,
            site,
        });
        self.next_seq = self.next_seq.saturating_add(1);
    }

    /// 该层该路径是否会新占一个格子（同层重写不占新格子）。
    fn cell_would_be_new(&self, level: Level, path: &str) -> bool {
        match self.find_slot(path) {
            None => true,
            Some(i) => self.slots[i].cells[level.rank() as usize].is_none(),
        }
    }

    fn is_third_party(&self, id: &str) -> bool {
        for e in self.sources.iter() {
            if e.id == id {
                return e.third_party;
            }
        }
        false
    }

    fn reject(code: OverlayCode) -> PushOutcome {
        PushOutcome {
            stored: false,
            changed: false,
            warn: None,
            err: Some(code),
            effective_level: None,
        }
    }

    // -- 仲裁器 ------------------------------------------------------------

    /// 仲裁一个槽：返回**生效层级**（`None` = 无生效覆盖，即槽里只有被遮蔽的）。
    ///
    /// 纯函数：只读，不改状态。过滤掉 shadowed 之后，在四个格子里取 rank 最大者。
    pub fn arbiter(&self, slot: &Slot) -> Option<Level> {
        let mut best: Option<Level> = None;
        for i in 0..LEVEL_COUNT {
            let c = match slot.cells[i].as_ref() {
                Some(c) => c,
                None => continue,
            };
            if c.shadowed {
                continue;
            }
            let lvl = match Level::from_rank(i as u8) {
                Some(l) => l,
                // 越界的 rank 不可能出现在合法槽里；跳过而不是折回 ——
                // 折回会把"数据坏了"洗成"最低层生效"这种看着正常的错值。
                None => continue,
            };
            best = arbitrate(lvl, best.unwrap_or(lvl));
        }
        best
    }

    /// 仲裁一个路径，返回胜出覆盖（**只读**）。
    pub fn resolve(&self, path: &str) -> Option<Resolved> {
        let i = self.find_slot(path)?;
        let slot = &self.slots.get(i)?;
        let win_rank = self.arbiter(slot)? as usize;
        let win = slot.cells.get(win_rank)?.as_ref()?;
        // 被压住的层：rank 低于胜出层、非空、且未被遮蔽。按 rank 降序。
        let mut shadowed_levels: Vec<Level> = Vec::new();
        for i in 0..win_rank {
            if let Some(c) = slot.cells[i].as_ref() {
                if !c.shadowed {
                    if let Some(l) = Level::from_rank(i as u8) {
                        shadowed_levels.push(l);
                    }
                }
            }
        }
        shadowed_levels.reverse();
        Some(Resolved {
            path: String::from(path),
            level: win.level,
            source: win.source.clone(),
            value: win.value.clone(),
            unaudited: win.unaudited,
            third_party: win.third_party,
            shadowed_levels,
        })
    }

    /// 全量仲裁：按路径升序返回全部生效覆盖（**快照**，之后改栈不影响它）。
    pub fn resolve_all(&self) -> Vec<Resolved> {
        let mut out: Vec<Resolved> = Vec::new();
        for s in self.slots.iter() {
            if let Some(r) = self.resolve(&s.path) {
                out.push(r);
            }
        }
        out
    }

    // -- 审计 --------------------------------------------------------------

    /// 某路径的审计流水（按写入序）。O(审计数)。
    pub fn audit_of(&self, path: &str) -> Vec<AuditEntry> {
        let mut out: Vec<AuditEntry> = Vec::new();
        for e in self.audit.iter() {
            if e.path.as_str() == path {
                out.push(e.clone());
            }
        }
        out
    }

    /// 某来源的审计流水（按写入序）。O(审计数)。
    pub fn audit_by_source(&self, source: &str) -> Vec<AuditEntry> {
        let mut out: Vec<AuditEntry> = Vec::new();
        for e in self.audit.iter() {
            if e.source.as_str() == source {
                out.push(e.clone());
            }
        }
        out
    }

    /// 审计总条数。
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    /// 审计只读引用。
    pub fn audit_ref(&self) -> &[AuditEntry] {
        &self.audit
    }

    /// 槽数（覆盖路径数）。
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// 槽位只读引用。
    pub fn slot_ref(&self, path: &str) -> Option<&Slot> {
        let i = self.find_slot(path)?;
        self.slots.get(i)
    }

    /// **格子口径**的存活覆盖数（不含被遮蔽）。
    ///
    /// 口径说明：一条路径可有多个格子（四层各一格），所以格子数 ≥ 路径数。
    /// 例如 `color.a` 同时有默认与主题两格时，这里算 2，而
    /// [`Self::resolved_count`]（路径口径）算 1。**两个口径都存在是因为它们
    /// 回答的是不同问题**：
    /// - 格子口径 = 栈的内存占用 ⇒ 上限检查（[`Self::push`] 的
    ///   [`OverlayCode::OverrideLimit`]）与报告守恒式用它。
    /// - 路径口径 = 消费方真正拿到的值条数 ⇒ [`Self::export_bindings`] 用它。
    ///
    /// 名字里带 `cell` 就是为了防止再被当成路径口径用 —— 本轮判据第一版就是
    /// 把这两个数当成同一个而红。
    pub fn live_cell_count(&self) -> usize {
        let mut n = 0usize;
        for s in self.slots.iter() {
            n += s.live_count();
        }
        n
    }

    /// **路径口径**的生效覆盖数 = [`Self::resolve_all`] 的长度。
    ///
    /// 恒等式：`resolved_count() == resolve_all().len()`，且
    /// `resolved_count() <= live_cell_count()`。判据两条都断。
    pub fn resolved_count(&self) -> usize {
        let mut n = 0usize;
        for s in self.slots.iter() {
            if self.resolve(&s.path).is_some() {
                n += 1;
            }
        }
        n
    }

    fn find_slot(&self, path: &str) -> Option<usize> {
        for (i, s) in self.slots.iter().enumerate() {
            if s.path.as_str() == path {
                return Some(i);
            }
        }
        None
    }

    // -- 结构不变式 -------------------------------------------------------

    /// 结构校验：槽按 path **严格升序**且无重复（与 F3405 的 `verify()` 同纪律）。
    ///
    /// 没有这条，`push` 里那句"插入后重排序"一旦写错就会静默破坏有序前提；
    /// 而 `find_slot` 是线性扫描所以**查不出来** —— 没有这条不变式，排序坏掉
    /// 对所有外部行为不可观测。这条判据是排序逻辑唯一的观测面。
    pub fn verify(&self) -> Result<(), OverlayCode> {
        if self.slots.len() > MAX_SLOTS {
            return Err(OverlayCode::SlotLimit);
        }
        if self.audit.len() > MAX_AUDIT {
            return Err(OverlayCode::AuditLimit);
        }
        if self.slots.len() > 1 {
            for i in 1..self.slots.len() {
                let prev = self.slots[i - 1].path.as_str();
                let cur = self.slots[i].path.as_str();
                if prev >= cur {
                    return Err(OverlayCode::PathInvalid);
                }
            }
        }
        for s in self.slots.iter() {
            if s.path.len() < PATH_MIN_LEN || s.path.len() > MAX_PATH_LEN {
                return Err(OverlayCode::PathInvalid);
            }
            for i in 0..LEVEL_COUNT {
                if let Some(c) = s.cells[i].as_ref() {
                    if c.value.len() > MAX_VALUE_LEN || c.source.len() > MAX_SOURCE_LEN {
                        return Err(OverlayCode::ValueInvalid);
                    }
                }
            }
        }
        // 审计序号必须严格 1..=len（缺号意味着有写入没记账 = 零静默破口）。
        if !self.audit.is_empty() {
            for (i, e) in self.audit.iter().enumerate() {
                if e.seq as usize != i + 1 {
                    return Err(OverlayCode::DiagIncomplete);
                }
            }
        }
        Ok(())
    }

    // -- 跨批对接：导出给类型系统 ------------------------------------------

    /// 把生效覆盖导出为 F3405 的 [`Binding`] 列表，由 `TypeChecker::check_all`
    /// 做类型校验。**本模块不重写类型判断**（头注第七节）。
    ///
    /// `site` 用于给导出项补定位（导出项本身没有源文件位置）。
    pub fn export_bindings(&self, site: Site) -> Vec<Binding> {
        let mut out: Vec<Binding> = Vec::new();
        for s in self.slots.iter() {
            if let Some(r) = self.resolve(&s.path) {
                out.push(Binding::new(&r.path, &r.value, site));
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 十、报告（跨批对接点：E14 第三方扩展边界）
// ---------------------------------------------------------------------------

/// 覆盖层报告。E14 拿它决定第三方扩展的覆盖是否准入。
///
/// **三个守恒式**（缺一不可，缺了就是"报告不可信"）：
/// 1. 四层分布之和 == 槽内的条目总数（含被遮蔽）。
/// 2. 生效数 + 遮蔽数 == 分布之和。
/// 3. 审计动作计数之和 == 审计总条数。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct OverlayReport {
    /// 报告版本。
    pub version: &'static str,
    /// 已登记来源数。
    pub sources: usize,
    /// 槽数（覆盖路径数）。
    pub slots: usize,
    /// 四层的槽内条目数（下标 = rank）。
    pub per_level: [usize; LEVEL_COUNT],
    /// 生效覆盖数。
    pub live: usize,
    /// 越级遮蔽数。
    pub shadowed: usize,
    /// 来源不明数。
    pub unaudited: usize,
    /// 第三方扩展来源数。
    pub third_party: usize,
    /// 审计总条数。
    pub audit: usize,
    /// 四种动作的审计计数（下标见 `ACT_*` 常量）。
    pub per_action: [usize; 4],
}

/// [`OverlayReport::per_action`] 的动作下标常量。
pub const ACT_COVER: usize = 0;
/// 同层被后来者压掉。
pub const ACT_SUPERSEDED: usize = 1;
/// 越级写入未生效。
pub const ACT_SHADOWED: usize = 2;
/// 来源未登记。
pub const ACT_UNAUDITED: usize = 3;

impl OverlayReport {
    /// 由栈构建报告。守恒性在此**显式可查**（`conserved()`）。
    pub fn build(st: &OverlayStack) -> OverlayReport {
        let mut per_level = [0usize; LEVEL_COUNT];
        let mut live = 0usize;
        let mut shadowed = 0usize;
        let mut unaudited = 0usize;
        let mut third_party = 0usize;
        for s in st.slots_ref() {
            for i in 0..LEVEL_COUNT {
                if s.cells[i].is_some() {
                    per_level[i] += 1;
                }
            }
            live += s.live_count();
            shadowed += s.shadowed_count();
            unaudited += s.unaudited_count();
            third_party += s.third_party_count();
        }
        let mut per_action = [0usize; 4];
        for e in st.audit_ref() {
            let idx = match e.action {
                AuditAction::Cover => ACT_COVER,
                AuditAction::Superseded => ACT_SUPERSEDED,
                AuditAction::ShadowedCover => ACT_SHADOWED,
                AuditAction::UnauditedCover => ACT_UNAUDITED,
            };
            per_action[idx] += 1;
        }
        OverlayReport {
            version: OVERLAY_VERSION,
            sources: st.source_count(),
            slots: st.slot_count(),
            per_level,
            live,
            shadowed,
            unaudited,
            third_party,
            audit: st.audit_len(),
            per_action,
        }
    }

    /// 槽内条目总数（四层分布之和）。
    pub fn cell_total(&self) -> usize {
        let mut s = 0usize;
        for n in self.per_level.iter() {
            s += *n;
        }
        s
    }

    /// 生效数 + 遮蔽数（应等于槽内条目总数）。
    pub fn accounted(&self) -> usize {
        self.live + self.shadowed
    }

    /// 动作计数之和（应等于审计总条数）。
    pub fn action_total(&self) -> usize {
        let mut s = 0usize;
        for n in self.per_action.iter() {
            s += *n;
        }
        s
    }

    /// 三条守恒式是否**全部**成立。
    ///
    /// **不合并成一个布尔**：三个守恒式各自失败的原因不同（分布错 = 统计漏格、
    /// 生效错 = 仲裁漏过滤、动作错 = 审计漏记），合成一个 bool 就再也说不出
    /// 是哪一条坏了。判据逐条断这三个数，不调这里。
    pub fn conserved(&self) -> bool {
        self.cell_total() == self.accounted() && self.action_total() == self.audit
    }

    /// 读屏播报。**只报层级/来源/计数，不报任何覆盖值正文**（锚点要"来源读屏
    /// 可查"，值不是来源；值可能来自用户自定义字符串，进读屏即隐私泄露）。
    pub fn spoken(&self) -> String {
        let mut s = String::new();
        s.push_str("令牌覆盖报告 版本 ");
        s.push_str(self.version);
        s.push_str("：已登记来源");
        s.push_str(&self.sources.to_string());
        s.push_str(" 个，覆盖路径");
        s.push_str(&self.slots.to_string());
        s.push_str(" 条，生效");
        s.push_str(&self.live.to_string());
        s.push_str(" 条，越级未生效");
        s.push_str(&self.shadowed.to_string());
        s.push_str(" 条，来源未登记");
        s.push_str(&self.unaudited.to_string());
        s.push_str(" 条，第三方扩展");
        s.push_str(&self.third_party.to_string());
        s.push_str(" 条。四层分布：");
        for (i, l) in Level::ALL.iter().enumerate() {
            s.push_str(l.zh());
            s.push('=');
            s.push_str(&self.per_level[i].to_string());
            s.push('；');
        }
        s.push_str("审计动作：覆盖");
        s.push_str(&self.per_action[ACT_COVER].to_string());
        s.push_str("，被压掉");
        s.push_str(&self.per_action[ACT_SUPERSEDED].to_string());
        s.push_str("，越级");
        s.push_str(&self.per_action[ACT_SHADOWED].to_string());
        s.push_str("，来源不明");
        s.push_str(&self.per_action[ACT_UNAUDITED].to_string());
        s.push('。');
        s
    }
}

impl OverlayStack {
    /// 槽只读引用（供报告遍历）。
    pub fn slots_ref(&self) -> &[Slot] {
        &self.slots
    }

    /// 审计流**可变**引用。**只给判据构造跳号态用**
    /// （`E06-边界-verify抓审计跳号`），不给生产面。
    pub fn audit_mut_for_test(&mut self) -> &mut Vec<AuditEntry> {
        &mut self.audit
    }

    /// 槽**可变**引用。**只给判据构造非法态用**（`E06-边界-verify抓超长路径`），
    /// 不给生产面 —— 理由见 `ver01f_checks` 里那两条判据的注释。
    pub fn slots_mut_for_test(&mut self) -> &mut Vec<Slot> {
        &mut self.slots
    }
}
