//! VE-F3401 · 令牌运行时架构（VE-E 域 · 主题与个性化引擎 · 令牌运行时组 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3401`
//!
//! **判据（锚点原文）**：四件两律、单源承诺、有迹可循、越权拒绝、判据。
//!
//! **职责定位（锚点原文）**：设计令牌运行时总纲（解析/求值/订阅/覆盖四件 +
//! 两律：令牌单源、变更有迹），令牌是界面的 DNA——颜色字号间距全都从令牌
//! 流出，写死一个像素值就是破坏单源。
//!
//! **数据结构（锚点原文）**：四件总纲；两律声明。
//!
//! **错误路径与降级矩阵（锚点原文）**：求值失败→降级默认；订阅泄漏→回收；
//! 覆盖越权→拒绝。
//!
//! **性能逐项分解（锚点原文）**：O(令牌数)。
//!
//! **跨批对接点（锚点原文）**：K 域主题消费。
//!
//! **无障碍与隐私（锚点原文）**：架构图读屏替代。
//!
//! **本项的边界（不越界施工，遵守"只做领到的任务"）**：
//! VE-F3401 是**总纲**——它交付四件的契约与两律的**可执行强制点**，不代做
//! 各组的引擎本体。分工在册（见 [`DOWNSTREAM_OWNERSHIP`]）：
//! - F3402 令牌解析器拥有**双格式解析与引用 DAG**；本项只立"解析产出必须
//!   是注册表条目"这条契约，不写解析器；
//! - F3403 依赖图与级联拥有**级联重算与深度上限**；本项只立"求值顺序"契约；
//! - F3406 令牌覆盖层拥有**四级栈与优先级仲裁**；本项只立"覆盖必须持权"契约；
//! - F3416 令牌变更订阅拥有**响应式链与风暴合并**；本项只立"泄漏回收"契约；
//! - F3417 令牌验证器拥有**令牌文件静态三查**；本项的 [`SingleSourceAuditor`]
//!   审的是**界面侧的硬编码字面量**（"写死一个像素值"），与令牌文件静态验证
//!   是两件事，不重复。
//!
//! **设计要点**：
//! - **四件不是四张标签**：每件都有 `PieceSpec` 契约（职责/输入/输出/失败
//!   策略/复杂度/消费方/所服务的律），契约断链由 [`Architecture::check_contracts`]
//!   检出——总纲自己先做到"可追溯"，不允许挂名；
//! - **律一「令牌单源」是可执行的审计，不是口号**：注册表是唯一真值出处；
//!   [`SingleSourceAuditor`] 把界面侧的每处样式写法与注册表对账，产出四类
//!   破律——`HardcodedLiteral`（抄了令牌值的第二处）、`UnregisteredLiteral`
//!   （谁都不是的野生字面量）、`DanglingReference`（引用了不存在的令牌）、
//!   `OrphanToken`（令牌在册没人用，删了没人发现——单源的另一半腐烂）；
//! - **律二「变更有迹」是提交即拒**：[`ChangeLedger::commit`] 对缺 `actor` 或
//!   缺 `reason` 的变更**直接拒绝**（[`E_UNTRACEABLE`]）——无痕变更在架构层
//!   不可表达，不是"事后查不到"；账本只增不改，`seq` 单调，时钟回拨不乱序；
//! - **越权拒绝**：覆盖不是"谁都能写"。[`OverrideAuthority`] 按
//!   `holder × layer` 授权，未持权覆盖返回 [`E_OVERRIDE_UNAUTHORIZED`] 并带上
//!   正确的下一步（申请哪一层的授权），不是一句"权限不足"；
//! - **求值失败→降级默认**：[`TokenResolver::resolve`] 三级判定
//!   （覆盖层 → 注册表 → 默认表），末级兜底给的是**显式声明的降级值**并在
//!   结果里标 [`Resolution::Fallback`]，消费方能分辨"这是真值还是兜底"；
//! - **订阅泄漏→回收**：[`SubscriptionTable::sweep`] 两条判据（主体注销 /
//!   静默超期 `SUB_TTL_TICKS`），回收产出可查记录——回收不是静默消失。
//!
//! **零外部依赖**，只依赖 `crate::checks`（自检侧）与 `alloc`。
//! 确定性：逻辑 tick 注入、零墙钟、零 IO，回归可复现（对拍红线）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、总纲常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 总纲版本。契约变更走版本号，破坏性变更必须升版并留迁移说明。
pub const ARCH_VERSION: &str = "E01-arch-v1";

/// 令牌注册表显性上限。超限报错，不静默增长（扩容走 ADR）。
pub const MAX_TOKENS: usize = 4096;

/// 变更账本显性上限。满后拒绝并计数——账本丢失等于"变更有迹"失效。
pub const LEDGER_CAP: usize = 1024;

/// 订阅静默超期阈值（逻辑 tick；60Hz 下约 15 秒）。
pub const SUB_TTL_TICKS: u64 = 900;

/// 降级兜底的最终值：无类别可依时的最后一句实话。
pub const UNSPECIFIED_FALLBACK: &str = "var(--unspecified, inherit)";

/// 降级默认表（键为类别前缀，按最长匹配生效）。
/// 求值失败时消费方拿到的**不是空值也不是 0**，而是这里声明过的可用值。
pub const FALLBACK_DEFAULTS: [(&str, &str); 6] = [
    ("color.text", "#e8ecf4"),
    ("color.bg", "#0b0d12"),
    ("color.accent", "#4c8dff"),
    ("space", "16px"),
    ("radius", "10px"),
    ("motion", "180ms"),
];

/// 复杂度声明（人读文本）。实现与本表逐条对应，改动必须两处同步走 ADR。
pub const COMPLEXITY_DOC: &str = "\
令牌运行时架构复杂度声明（VE-F3401 · E01-arch-v1）：
C1 单源审计 audit：O(站点数 + 令牌数)（线性对账，四个破律判定各一趟）。
C2 变更提交 commit：O(1)（尾部追加 + 三项入参校验）。
C3 账本回放 replay：O(账本长)（按 seq 顺序重放，供审计对拍）。
C4 覆盖授权 authorize：O(授权条目)（线性查 holder）。
C5 令牌求值 resolve：O(令牌数)（注册表线性查 + 覆盖层定秩）。
C6 降级选取 fallback_for：O(默认表长)（最长前缀匹配，表长为常数 6）。
C7 订阅回收 sweep：O(订阅数)（一趟判定 + 一趟回收记录）。";

// ---------------------------------------------------------------------------
// 二、四件总纲（判据一：解析/求值/订阅/覆盖四件）
// ---------------------------------------------------------------------------

/// 四件标识。顺序即数据流顺序：先解析出值，再求值成结果，订阅者被通知，
/// 覆盖层在求值前介入仲裁。**四件不多不少**（总量铁律在总纲的投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Piece {
    /// 解析：令牌文本 → 注册表条目（含引用完整性）。
    Resolve,
    /// 求值：路径 → 消费方拿到的具体值（三级判定 + 降级兜底）。
    Evaluate,
    /// 订阅：值变化 → 消费方被通知（响应式链）。
    Subscribe,
    /// 覆盖：层级值替换注册表值（须持权）。
    Override,
}

/// 四件全集（判据：四件，一件不多一件不少）。
pub const PIECES: [Piece; 4] = [
    Piece::Resolve,
    Piece::Evaluate,
    Piece::Subscribe,
    Piece::Override,
];

impl Piece {
    /// 中文名（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            Piece::Resolve => "解析",
            Piece::Evaluate => "求值",
            Piece::Subscribe => "订阅",
            Piece::Override => "覆盖",
        }
    }

    /// 英文名（标识符与文档用）。
    pub fn en(self) -> &'static str {
        match self {
            Piece::Resolve => "resolve",
            Piece::Evaluate => "evaluate",
            Piece::Subscribe => "subscribe",
            Piece::Override => "override",
        }
    }

    /// 该件在数据流中的序位（0 解析 → 3 覆盖仲裁介入点）。
    ///
    /// 覆盖件序位刻意不等于"最后"——覆盖在**求值之前**介入仲裁，
    /// 否则就是"先算完再替换"，多一次无谓的全链求值。
    pub fn stage(self) -> u8 {
        match self {
            Piece::Resolve => 0,
            Piece::Override => 1,
            Piece::Evaluate => 2,
            Piece::Subscribe => 3,
        }
    }

    /// 该件服务的律（覆盖多件时取全部）。
    pub fn serves(self) -> &'static [Law] {
        match self {
            // 解析决定"什么是真值出处"——单源律的地基。
            Piece::Resolve => &[Law::SingleSource],
            // 求值产出的一切都必须带出处——两律都服务。
            Piece::Evaluate => &[Law::SingleSource, Law::Traceable],
            // 订阅者要知道"谁变的"——有迹律。
            Piece::Subscribe => &[Law::Traceable],
            // 覆盖是最容易偷偷改值的地方——两律都盯着。
            Piece::Override => &[Law::SingleSource, Law::Traceable],
        }
    }
}

/// 单件契约。总纲不是标签墙——每件都要说清边界，越界即违约。
#[derive(Clone, Copy, Debug)]
pub struct PieceSpec {
    /// 件标识。
    pub piece: Piece,
    /// 中文职责名。
    pub duty_zh: &'static str,
    /// 英文职责名。
    pub duty_en: &'static str,
    /// 输入契约（吃什么）。
    pub input: &'static str,
    /// 输出契约（吐什么）。
    pub output: &'static str,
    /// 失败策略（锚点降级矩阵落到本件的那一格）。
    pub on_failure: &'static str,
    /// 复杂度声明（对应 [`COMPLEXITY_DOC`] 的编号）。
    pub complexity: &'static str,
    /// 下游消费方（跨批对接点）。
    pub consumers: &'static str,
    /// 本件的**不做清单**（越界即违约，防止总纲被当成万能筐）。
    pub not_mine: &'static str,
}

/// 四件契约表（判据：四件总纲）。
pub const PIECE_SPECS: [PieceSpec; 4] = [
    PieceSpec {
        piece: Piece::Resolve,
        duty_zh: "把令牌文本变成注册表条目，并担保引用完整",
        duty_en: "token text -> registry entries, reference integrity guaranteed",
        input: "令牌源文本（JSON/TOML，格式解析本体归 F3402）",
        output: "TokenDef 序列：路径/类型句柄/规范字面量/唯一定义点",
        on_failure: "引用断裂或格式错 -> 拒绝入册并定位；已入册的不受牵连",
        complexity: "C1 O(站点数+令牌数)",
        consumers: "F3403 级联、F3406 覆盖层、F3417 验证器、K 域主题消费",
        not_mine: "不写双格式词法/语法解析器（F3402），不建依赖 DAG（F3403）",
    },
    PieceSpec {
        piece: Piece::Evaluate,
        duty_zh: "按三级判定给出消费方拿到的具体值",
        duty_en: "resolve path -> concrete value with provenance",
        input: "令牌路径 + 覆盖层快照",
        output: "Resolution：值 + 出处（Provenance）或显式降级标记",
        on_failure: "求值失败 -> 降级默认（FALLBACK_DEFAULTS），结果标 Fallback",
        complexity: "C5 O(令牌数)",
        consumers: "F3415 应用读令牌 API、F3422 换肤影响面、F3411 明暗双主题",
        not_mine: "不做增量缓存与批量流水线（F3408），不做类型校验（F3405）",
    },
    PieceSpec {
        piece: Piece::Subscribe,
        duty_zh: "值变化时通知订阅者，并对泄漏订阅者回收",
        duty_en: "notify subscribers on change, reclaim leaked subscriptions",
        input: "变更事件（含 seq/actor/target，见律二账本）",
        output: "投递记录 + 回收记录（ReclaimRecord）",
        on_failure: "订阅泄漏 -> 超期或主体注销即回收，回收留痕不静默消失",
        complexity: "C7 O(订阅数)",
        consumers: "F3416 响应式链、F3409 调试工具、E09 一致性审查",
        not_mine: "不做风暴合并与丢通知补发（F3416），不做批量合并帧配置（F3416）",
    },
    PieceSpec {
        piece: Piece::Override,
        duty_zh: "按层级替换令牌值，且只允许持权者替换",
        duty_en: "layered value replacement, authority-checked",
        input: "覆盖请求：目标路径 + 层级 + 持有者 + 新值 + 理由",
        output: "覆盖条目（带 seq 与出处）或拒绝错误",
        on_failure: "覆盖越权 -> 拒绝并指明应申请的层级授权",
        complexity: "C4 O(授权条目)",
        consumers: "F3406 四级覆盖栈、F3414 第三方扩展、F3404 主题切换事务",
        not_mine: "不做四级栈仲裁与冲突合并（F3406），不做主题事务与快照回滚（F3404）",
    },
];

/// 取单件契约。
pub fn piece_spec(p: Piece) -> &'static PieceSpec {
    PIECE_SPECS
        .iter()
        .find(|s| s.piece == p)
        .expect("PIECE_SPECS 覆盖 PIECES 全部四件")
}

// ---------------------------------------------------------------------------
// 三、两律声明（判据二：令牌单源 + 变更有迹）
// ---------------------------------------------------------------------------

/// 两律标识。**两律不多不少**。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Law {
    /// 律一「令牌单源」：颜色字号间距全都从令牌流出，写死一个像素值就是破坏单源。
    SingleSource,
    /// 律二「变更有迹」：每一次令牌值变化都必须留下主体/动作/对象/前值/后值/理由。
    Traceable,
}

/// 两律全集（判据：两律）。
pub const LAWS: [Law; 2] = [Law::SingleSource, Law::Traceable];

impl Law {
    /// 律名。
    pub fn zh(self) -> &'static str {
        match self {
            Law::SingleSource => "令牌单源",
            Law::Traceable => "变更有迹",
        }
    }

    /// 律编号（对外引用用，E01-L1 / E01-L2）。
    pub fn code(self) -> &'static str {
        match self {
            Law::SingleSource => "E01-L1",
            Law::Traceable => "E01-L2",
        }
    }
}

/// 单条律的声明：律文、违例形态、强制点。
#[derive(Clone, Copy, Debug)]
pub struct LawSpec {
    /// 律标识。
    pub law: Law,
    /// 律文（判据原文口径的正式表述）。
    pub statement: &'static str,
    /// 违例形态（违反长什么样——能观测才能拦）。
    pub violation: &'static str,
    /// 强制点（架构里哪一处代码真正拦它）。
    pub enforcement: &'static str,
    /// 承诺（对下游的硬承诺，写下来就是契约）。
    pub promise: &'static str,
}

/// 两律声明表（判据：两律声明）。
pub const LAW_SPECS: [LawSpec; 2] = [
    LawSpec {
        law: Law::SingleSource,
        statement: "颜色、字号、间距、圆角、动效时长全部从令牌流出；\
                    界面里写死的每一个字面量都是对单源的破坏，无一例外。",
        violation: "HardcodedLiteral（抄了令牌值的第二处）/ UnregisteredLiteral\
                    （野生字面量）/ DanglingReference（引用不存在的令牌）/ \
                    OrphanToken（在册却无人引用的腐烂令牌）",
        enforcement: "SingleSourceAuditor::audit —— 四类破律各一趟判定，\
                      产出可枚举的 BreachReport，不合口径的提交不得进入运行时",
        promise: "任取一个界面像素，都能反查到唯一的令牌出处；\
                  反查失败即视为破律，不接受'先这样后补'。",
    },
    LawSpec {
        law: Law::Traceable,
        statement: "每一次令牌值变化都留下主体、动作、对象、前值、后值、理由六项；\
                    无痕变更在架构层不可表达。",
        violation: "缺 actor 或缺 reason 的变更提交（E_UNTRACEABLE）/ \
                    账本溢出后静默丢弃（E_LEDGER_FULL，带丢弃计数）",
        enforcement: "ChangeLedger::commit —— 入参校验不过即拒绝，\
                      账本只增不改，seq 单调保证时钟回拨不乱序",
        promise: "任取一次界面变化，都能回答'谁在什么理由下把它改成了什么'；\
                  答不出来就说明这次变更本就不该被接受。",
    },
];

/// 取单律声明。
pub fn law_spec(l: Law) -> &'static LawSpec {
    LAW_SPECS
        .iter()
        .find(|s| s.law == l)
        .expect("LAW_SPECS 覆盖 LAWS 全部两律")
}

// ---------------------------------------------------------------------------
// 四、跨批对接与下游归属（判据：K 域主题消费）
// ---------------------------------------------------------------------------

/// K 域主题消费契约（锚点跨批对接点）。
///
/// 令牌运行时对 K 域只承诺一件事：**值与出处一起出去**。K 域拿到的不是
/// 一堆裸值，而是一串带 `path + provenance` 的对——这样主题工坊里改一个
/// 颜色，K 域能说清"我换掉的是哪个令牌的哪个层级的值"。
pub const K_DOMAIN_CONTRACT: &str = "\
K 域消费令牌的三条硬约定（VE-F3401 冻结）：
K1 取值必带出处：K 域拿到的每个值都带 path + provenance（层级 + 变更 seq），
   不接受裸值——裸值会让 K 域无法反查，破坏律一的反查承诺。
K2 降级值必须可辨：兜底值以 Resolution::Fallback 形态给出，K 域渲染时
   必须能区分'这是真值'与'这是兜底'，禁止把兜底当真值继续派生。
K3 变更必可订阅：K 域的换肤动作走订阅链（Piece::Subscribe），
   不允许旁路直接改注册表——旁路改值等于绕过律二。";

/// 下游归属表（防止总纲被当成万能筐，也防止各组互相抢活）。
pub const DOWNSTREAM_OWNERSHIP: [(&str, &str); 8] = [
    ("VE-F3402", "令牌双格式解析器与引用 DAG 构建"),
    ("VE-F3403", "令牌依赖图与级联重算、深度上限"),
    ("VE-F3404", "主题切换事务与快照回滚"),
    ("VE-F3405", "令牌六类类型系统与单位校验"),
    ("VE-F3406", "四级覆盖栈、优先级仲裁与来源审计"),
    ("VE-F3408", "求值缓存与批量流水线、P95 承诺"),
    ("VE-F3414", "第三方令牌扩展的命名空间与冲突检测"),
    ("VE-F3416", "响应式订阅链、风暴合并与丢通知补发"),
];

// ---------------------------------------------------------------------------
// 五、律一：令牌与单源审计器
// ---------------------------------------------------------------------------

/// 令牌类型句柄（不透明 u16）。
///
/// 本项**不解释**类型语义——那是 F3405 的类型系统的地盘。这里只保留一个
/// 不透明句柄，让总纲能在不越界的前提下表达"令牌带类型"这件事。
/// `0` 保留为"未指定"，防止下游把默认值误当合法类型句柄。
pub type KindId = u16;

/// 未指定的类型句柄（保留值）。
pub const KIND_UNSPECIFIED: KindId = 0;

/// 令牌定义（注册表条目）：真值出处的唯一声明。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenDef {
    /// 规范路径（点分，如 `color.accent.default`）。
    pub path: String,
    /// 类型句柄（语义归 F3405；`KIND_UNSPECIFIED` 表示尚未指定）。
    pub kind_id: KindId,
    /// 规范字面量（该令牌的真值）。
    pub literal: String,
    /// 唯一定义点（`文件:行`）。**单源审计据此豁免这一处**——
    /// 定义点就是"单源"本身，不是破律。
    pub canonical_site: String,
    /// 用途说明（令牌没有说明就等于没有共识）。
    pub doc: String,
}

impl TokenDef {
    /// 路径类别（首段，如 `color` / `space`），用于降级默认选取。
    pub fn category(&self) -> &str {
        match self.path.find('.') {
            Some(i) => &self.path[..i],
            None => &self.path,
        }
    }

    /// 读屏可读单行（无障碍：令牌不能只有颜色没有文字）。
    pub fn screen_line(&self) -> String {
        format!(
            "令牌 {}，值 {}，定义点 {}，说明 {}",
            self.path, self.literal, self.canonical_site, self.doc
        )
    }
}

/// 界面侧的一处样式写法（审计输入）。
///
/// `content` 为字面量（如 `"#4c8dff"` / `"16px"`）或引用（如
/// `"{color.accent.default}"`）。审计只看这一处，不猜上下文。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleSite {
    /// 位置（`文件:行`）。
    pub site: String,
    /// 被赋值的属性名（如 `backgroundColor`）。
    pub property: String,
    /// 写法内容：字面量或 `{令牌路径}` 引用。
    pub content: String,
}

impl StyleSite {
    /// 是否为令牌引用写法（`{path}`）。
    pub fn is_reference(&self) -> bool {
        self.content.starts_with('{') && self.content.ends_with('}') && self.content.len() > 2
    }

    /// 引用的令牌路径（仅引用写法有效）。
    pub fn referenced_path(&self) -> Option<&str> {
        if self.is_reference() {
            Some(&self.content[1..self.content.len() - 1])
        } else {
            None
        }
    }

    /// 字面量（仅非引用写法有效）。
    pub fn literal(&self) -> Option<&str> {
        if self.is_reference() {
            None
        } else {
            Some(self.content.as_str())
        }
    }
}

/// 单源破律形态（四类，各有各的烂法）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Breach {
    /// 抄了令牌值的第二处：值已注册，但此处不是它的定义点。
    HardcodedLiteral {
        /// 破律位置。
        site: String,
        /// 属性名。
        property: String,
        /// 被抄的字面量。
        literal: String,
        /// 真值出处（应改为引用的令牌路径）。
        should_reference: String,
    },
    /// 野生字面量：不匹配任何已注册令牌——无主之物。
    UnregisteredLiteral {
        /// 破律位置。
        site: String,
        /// 属性名。
        property: String,
        /// 野生字面量。
        literal: String,
    },
    /// 悬空引用：引用了不存在的令牌路径。
    DanglingReference {
        /// 破律位置。
        site: String,
        /// 被引用的路径。
        path: String,
    },
    /// 腐烂令牌：注册在册却无人引用——单源的另一半腐烂（删了没人发现）。
    OrphanToken {
        /// 无人引用的令牌路径。
        path: String,
    },
}

impl Breach {
    /// 破律类别短码（分类统计与门禁用）。
    pub fn code(&self) -> &'static str {
        match self {
            Breach::HardcodedLiteral { .. } => "HARDCODE",
            Breach::UnregisteredLiteral { .. } => "UNREGISTERED",
            Breach::DanglingReference { .. } => "DANGLING",
            Breach::OrphanToken { .. } => "ORPHAN",
        }
    }

    /// 读屏可读的破律说明（无障碍：破律也要能念出来 + 给出路）。
    pub fn describe(&self) -> String {
        match self {
            Breach::HardcodedLiteral {
                site,
                property,
                literal,
                should_reference,
            } => format!(
                "单源破律（抄值）：{site} 的 {property} 直接写了 {literal}，\
                 而该值的唯一出处是令牌 {should_reference}；改为引用 {should_reference}。",
                site = site,
                property = property,
                literal = literal,
                should_reference = should_reference
            ),
            Breach::UnregisteredLiteral {
                site,
                property,
                literal,
            } => format!(
                "单源破律（野生值）：{site} 的 {property} 写了 {literal}，\
                 它不属于任何已注册令牌；要么改引令牌，要么先把它立为令牌再引用。",
                site = site,
                property = property,
                literal = literal
            ),
            Breach::DanglingReference { site, path } => format!(
                "单源破律（悬空引用）：{site} 引用了令牌 {path}，但它不在注册表里；\
                 补上该令牌的定义，或改引用已存在的令牌。",
                site = site,
                path = path
            ),
            Breach::OrphanToken { path } => format!(
                "单源破律（腐烂令牌）：令牌 {path} 在册却无人引用；\
                 接入界面或从注册表移除，腐烂的令牌会让单源承诺失效。",
                path = path
            ),
        }
    }
}

/// 单源审计报告。
#[derive(Clone, Debug, Default)]
pub struct BreachReport {
    /// 破律明细（确定性次序：先站点序、再令牌序）。
    pub breaches: Vec<Breach>,
    /// 被审计的站点数。
    pub sites_audited: usize,
    /// 在册令牌数。
    pub tokens_audited: usize,
}

impl BreachReport {
    /// 是否合规（零破律）。
    pub fn is_clean(&self) -> bool {
        self.breaches.is_empty()
    }

    /// 破律总数。
    pub fn count(&self) -> usize {
        self.breaches.len()
    }

    /// 按类别计数（四类各自多少）。
    pub fn count_by_code(&self, code: &str) -> usize {
        self.breaches.iter().filter(|b| b.code() == code).count()
    }

    /// 读屏摘要（无障碍：不靠颜色，靠文字说清）。
    pub fn screen_text(&self) -> String {
        format!(
            "单源审计：{} 处写法、{} 个令牌，破律 {} 条\
             （抄值 {}、野生 {}、悬空 {}、腐烂 {}）",
            self.sites_audited,
            self.tokens_audited,
            self.breaches.len(),
            self.count_by_code("HARDCODE"),
            self.count_by_code("UNREGISTERED"),
            self.count_by_code("DANGLING"),
            self.count_by_code("ORPHAN")
        )
    }
}

/// 令牌注册表（真值的唯一在册处）。
#[derive(Clone, Debug, Default)]
pub struct TokenRegistry {
    defs: Vec<TokenDef>,
}

impl TokenRegistry {
    /// 空注册表。
    pub fn new() -> Self {
        TokenRegistry { defs: Vec::new() }
    }

    /// 在册令牌数。
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &TokenDef> {
        self.defs.iter()
    }

    /// 按路径查（复杂度 C5 的一趟：O(令牌数)）。
    pub fn find(&self, path: &str) -> Option<&TokenDef> {
        self.defs.iter().find(|d| d.path == path)
    }

    /// 路径是否在册。
    pub fn contains(&self, path: &str) -> bool {
        self.find(path).is_some()
    }

    /// 按字面量反查令牌（审计"抄值"用；同值多令牌取首条并如实返回歧义）。
    pub fn find_by_literal(&self, literal: &str) -> Option<&TokenDef> {
        self.defs.iter().find(|d| d.literal == literal)
    }

    /// 同一字面量是否被多个令牌占用（单源的反面：真值重复在册）。
    pub fn literal_is_ambiguous(&self, literal: &str) -> bool {
        self.defs.iter().filter(|d| d.literal == literal).count() > 1
    }

    /// 入册一个令牌定义。
    ///
    /// 拒绝三事：路径重复（非幂等合并——入册必须唯一）、字面量为空、
    /// 定义点为空（定义点是律一的豁免凭据，没有它就无从判定抄值）。
    /// 超 [`MAX_TOKENS`] 同样拒绝（不静默增长）。
    pub fn insert(&mut self, def: TokenDef) -> Result<u32, TokenError> {
        if def.path.is_empty() {
            return Err(TokenError::new(
                E_EMPTY_PATH,
                "入册被拒：令牌路径为空",
                "路径是令牌的唯一主键，为空则无法定位也无法审计",
                "补上点分路径（如 color.accent.default）",
                "解析件 F3402",
            ));
        }
        if def.literal.is_empty() {
            return Err(TokenError::new(
                E_EMPTY_LITERAL,
                "入册被拒：令牌字面量为空",
                &format!("令牌 {} 没有真值，界面无从消费", def.path),
                "补上该令牌的真值字面量",
                "解析件 F3402",
            ));
        }
        if def.canonical_site.is_empty() {
            return Err(TokenError::new(
                E_NO_CANONICAL_SITE,
                "入册被拒：令牌没有定义点",
                &format!(
                    "令牌 {} 缺 canonical_site，律一将无法区分'定义点'与'抄值处'",
                    def.path
                ),
                "在令牌源里标注该令牌的定义位置（文件:行）",
                "解析件 F3402",
            ));
        }
        if self.contains(&def.path) {
            return Err(TokenError::new(
                E_DUPLICATE_PATH,
                "入册被拒：令牌路径重复",
                &format!(
                    "路径 {} 已在册；单源不允许同一路径有两个真值",
                    def.path
                ),
                "改路径，或先移除旧条目再入册",
                "解析件 F3402",
            ));
        }
        if self.len() >= MAX_TOKENS {
            return Err(TokenError::new(
                E_TOKEN_CAP,
                "入册被拒：注册表已达上限",
                &format!("在册 {} 条达到显性上限 {}", self.len(), MAX_TOKENS),
                "按 ADR 提升 MAX_TOKENS 后重建注册表",
                "架构维护方",
            ));
        }
        self.defs.push(def);
        Ok((self.defs.len() - 1) as u32)
    }
}

/// 律一强制点：单源审计器。
///
/// 把界面侧每处样式写法与注册表对账，产出四类破律。这是"写死一个像素值就是
/// 破坏单源"的**可执行形态**——不是评审时靠人盯，是提交路径上就会被拦下。
#[derive(Clone, Debug)]
pub struct SingleSourceAuditor {
    registry: TokenRegistry,
}

impl SingleSourceAuditor {
    /// 以注册表构造。
    pub fn new(registry: TokenRegistry) -> Self {
        SingleSourceAuditor { registry }
    }

    /// 只读访问注册表。
    pub fn registry(&self) -> &TokenRegistry {
        &self.registry
    }

    /// 执行审计（复杂度 C1：O(站点数 + 令牌数)）。
    ///
    /// 判定规则（顺序即确定性保证）：
    /// 1. 引用写法 → 路径必须在册，否则 `DanglingReference`；
    /// 2. 字面量写法且此处是某令牌的 `canonical_site` → 豁免（这就是"单源"本身）；
    /// 3. 字面量写法且该值已注册 → `HardcodedLiteral`（抄了令牌值的第二处）；
    /// 4. 其余字面量 → `UnregisteredLiteral`（野生值）；
    /// 5. 全部判完后，被引用集合覆盖不到的令牌 → `OrphanToken`。
    pub fn audit(&self, sites: &[StyleSite]) -> BreachReport {
        let mut breaches = Vec::new();
        // 被引用到的令牌路径集合（用于 OrphanToken 判定）。
        let mut referenced: Vec<String> = Vec::new();

        for site in sites {
            if let Some(path) = site.referenced_path() {
                if self.registry.contains(path) {
                    referenced.push(path.to_string());
                } else {
                    breaches.push(Breach::DanglingReference {
                        site: site.site.clone(),
                        path: path.to_string(),
                    });
                }
                continue;
            }
            // 字面量写法。
            let Some(lit) = site.literal() else {
                // 空内容：既不是引用也不是字面量（非法写法），按野生值处理。
                breaches.push(Breach::UnregisteredLiteral {
                    site: site.site.clone(),
                    property: site.property.clone(),
                    literal: site.content.clone(),
                });
                continue;
            };
            match self.registry.find_by_literal(lit) {
                Some(def) if def.canonical_site == site.site => {
                    // 定义点豁免：这里就是真值出处，律一在此成立。
                    referenced.push(def.path.clone());
                }
                Some(def) => {
                    referenced.push(def.path.clone());
                    breaches.push(Breach::HardcodedLiteral {
                        site: site.site.clone(),
                        property: site.property.clone(),
                        literal: lit.to_string(),
                        should_reference: def.path.clone(),
                    });
                }
                None => breaches.push(Breach::UnregisteredLiteral {
                    site: site.site.clone(),
                    property: site.property.clone(),
                    literal: lit.to_string(),
                }),
            }
        }

        // 腐烂令牌：在册但无人引用（也没人把它当定义点用）。
        for def in self.registry.iter() {
            if !referenced.iter().any(|p| p == &def.path) {
                breaches.push(Breach::OrphanToken {
                    path: def.path.clone(),
                });
            }
        }

        BreachReport {
            breaches,
            sites_audited: sites.len(),
            tokens_audited: self.registry.len(),
        }
    }
}

// ---------------------------------------------------------------------------
// 六、律二：变更账本
// ---------------------------------------------------------------------------

/// 变更动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeAction {
    /// 立册：令牌首次入册。
    Define,
    /// 覆盖：层级值替换注册表值。
    Override,
    /// 废弃：令牌退场。
    Deprecate,
}

impl ChangeAction {
    /// 中文名（读屏播报）。
    pub fn zh(self) -> &'static str {
        match self {
            ChangeAction::Define => "立册",
            ChangeAction::Override => "覆盖",
            ChangeAction::Deprecate => "废弃",
        }
    }

    /// 动作码（审计对拍用）。
    pub fn code(self) -> &'static str {
        match self {
            ChangeAction::Define => "DEFINE",
            ChangeAction::Override => "OVERRIDE",
            ChangeAction::Deprecate => "DEPRECATE",
        }
    }
}

/// 变更六元组（律二的记录形态：主体/动作/对象/前值/后值/理由）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeEntry {
    /// 单调序号（时钟回拨不乱序的保证）。
    pub seq: u64,
    /// 逻辑 tick（不用墙钟——回归可复现）。
    pub tick: u64,
    /// 主体：谁改的。
    pub actor: String,
    /// 动作。
    pub action: ChangeAction,
    /// 对象：改的是哪个令牌路径。
    pub target: String,
    /// 前值（空串表示"此前无值"）。
    pub before: String,
    /// 后值。
    pub after: String,
    /// 理由（**必填**，缺失即拒绝提交）。
    pub reason: String,
}

impl ChangeEntry {
    /// 读屏可读单行（无障碍：审计记录要能念）。
    pub fn screen_line(&self) -> String {
        format!(
            "第 {} 号变更：{} 于 tick {} 把令牌 {} {} 为 {}（原 {}），理由：{}",
            self.seq,
            self.actor,
            self.tick,
            self.target,
            self.action.zh(),
            self.after,
            if self.before.is_empty() {
                "无"
            } else {
                self.before.as_str()
            },
            self.reason
        )
    }
}

/// 变更请求（提交入参）。
#[derive(Clone, Debug)]
pub struct ChangeRequest {
    /// 主体。
    pub actor: String,
    /// 动作。
    pub action: ChangeAction,
    /// 对象路径。
    pub target: String,
    /// 前值。
    pub before: String,
    /// 后值。
    pub after: String,
    /// 理由（必填）。
    pub reason: String,
    /// 逻辑 tick。
    pub tick: u64,
}

impl ChangeRequest {
    /// 构造请求（省掉调用方逐字段摆弄）。
    pub fn new(
        actor: &str,
        action: ChangeAction,
        target: &str,
        before: &str,
        after: &str,
        reason: &str,
        tick: u64,
    ) -> Self {
        ChangeRequest {
            actor: actor.to_string(),
            action,
            target: target.to_string(),
            before: before.to_string(),
            after: after.to_string(),
            reason: reason.to_string(),
            tick,
        }
    }
}

/// 变更回执（提交成功凭证）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeReceipt {
    /// 落账的序号。
    pub seq: u64,
    /// 账本当前长度。
    pub ledger_len: usize,
}

/// 律二强制点：只增不改的变更账本。
#[derive(Clone, Debug, Default)]
pub struct ChangeLedger {
    entries: Vec<ChangeEntry>,
    next_seq: u64,
    dropped: u64,
    rejected: u64,
}

impl ChangeLedger {
    /// 空账本（`next_seq` 从 1 起，0 留作"无变更"哨兵）。
    pub fn new() -> Self {
        ChangeLedger {
            entries: Vec::new(),
            next_seq: 1,
            dropped: 0,
            rejected: 0,
        }
    }

    /// 账本长度。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 因账本满而**未落账**的变更计数（异常显性化：不静默丢弃）。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 因无痕而被拒的变更计数。
    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// 只读遍历（按 seq 升序）。
    pub fn iter(&self) -> impl Iterator<Item = &ChangeEntry> {
        self.entries.iter()
    }

    /// 按序号查（复杂度 O(账本长) 的线性定位；对拍回放用）。
    pub fn entry(&self, seq: u64) -> Option<&ChangeEntry> {
        self.entries.iter().find(|e| e.seq == seq)
    }

    /// 某路径的完整变更史（律二承诺：任取一次变化都能回答谁在什么理由下改的）。
    pub fn history_of(&self, target: &str) -> Vec<&ChangeEntry> {
        self.entries.iter().filter(|e| e.target == target).collect()
    }

    /// 提交变更（复杂度 C2：O(1)）。
    ///
    /// **无痕变更不可表达**：缺主体或缺理由直接 [`E_UNTRACEABLE`] 拒绝。
    /// 账本满则 [`E_LEDGER_FULL`] 拒绝并累加 `dropped`——宁可显式失败，
    /// 也不让"变更有迹"在溢出时悄悄失效。
    pub fn commit(&mut self, req: ChangeRequest) -> Result<ChangeReceipt, TokenError> {
        if req.actor.trim().is_empty() || req.reason.trim().is_empty() {
            self.rejected = self.rejected.saturating_add(1);
            let missing = if req.actor.trim().is_empty() {
                "主体(actor)"
            } else {
                "理由(reason)"
            };
            return Err(TokenError::new(
                E_UNTRACEABLE,
                "变更被拒：无痕变更不可表达",
                &format!("{} 缺失；律二要求主体与理由同时在场", missing),
                "补上主体与理由后重新提交（无痕变更在架构层没有入口）",
                "变更发起方",
            ));
        }
        if req.target.trim().is_empty() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(TokenError::new(
                E_EMPTY_PATH,
                "变更被拒：对象路径为空",
                "变更必须有对象；无对象的变更无法被审计与回溯",
                "补上被改令牌的点分路径",
                "变更发起方",
            ));
        }
        if self.entries.len() >= LEDGER_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(TokenError::new(
                E_LEDGER_FULL,
                "变更被拒：变更账本已满",
                &format!(
                    "账本 {} 条达到显性上限 {}；继续收就会丢掉可追溯性",
                    self.entries.len(),
                    LEDGER_CAP
                ),
                "先归档并轮转账本，或按 ADR 提升 LEDGER_CAP",
                "架构维护方",
            ));
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push(ChangeEntry {
            seq,
            tick: req.tick,
            actor: req.actor,
            action: req.action,
            target: req.target,
            before: req.before,
            after: req.after,
            reason: req.reason,
        });
        Ok(ChangeReceipt {
            seq,
            ledger_len: self.entries.len(),
        })
    }

    /// 回放全部变更（复杂度 C3：O(账本长)），返回逐条读屏文本——
    /// 审计对拍的基准输出（对拍红线）。
    pub fn replay(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.screen_line()).collect()
    }

    /// 读屏摘要（一行版）——变更有迹律的账本侧可听证据。
    ///
    /// 报告四数：已录变更、拒收（缺主体/缺理由/账本满）、因溢出丢弃、下一个
    /// 序号。**丢弃数必须出现**：账本满时静默丢弃会让人以为变更没发生过——
    /// 把丢弃数摆在读屏文本里，丢弃才不是静默的。
    pub fn screen_text(&self) -> String {
        format!(
            "{} 条变更（拒 {}，因溢出丢弃 {}，下一序号 {}）",
            self.entries.len(),
            self.rejected,
            self.dropped,
            self.next_seq
        )
    }
}

// ---------------------------------------------------------------------------
// 七、覆盖授权（错误路径：覆盖越权→拒绝）
// ---------------------------------------------------------------------------

/// 覆盖层级（四级覆盖体系的层级标签；仲裁本体归 F3406）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OverrideLayer {
    /// 默认层（注册表自身的值）。
    Default = 0,
    /// 主题层。
    Theme = 1,
    /// 场景层。
    Scene = 2,
    /// 组件层。
    Component = 3,
}

impl OverrideLayer {
    /// 四级全集（由低到高；仲裁细则归 F3406）。
    pub const ALL: [OverrideLayer; 4] = [
        OverrideLayer::Default,
        OverrideLayer::Theme,
        OverrideLayer::Scene,
        OverrideLayer::Component,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            OverrideLayer::Default => "默认",
            OverrideLayer::Theme => "主题",
            OverrideLayer::Scene => "场景",
            OverrideLayer::Component => "组件",
        }
    }

    /// 该层是否由注册表直接供给（默认层不可被覆盖——那是单源本身）。
    pub fn is_registry_layer(self) -> bool {
        self == OverrideLayer::Default
    }
}

/// 授权条目：某持有者在某层可覆盖。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    /// 持有者（主题 id / 扩展 id / 组件 id）。
    pub holder: String,
    /// 被授予的层。
    pub layer: OverrideLayer,
}

/// 覆盖授权表。
#[derive(Clone, Debug, Default)]
pub struct OverrideAuthority {
    grants: Vec<Grant>,
}

impl OverrideAuthority {
    /// 空授权表（默认状态下任何覆盖都会被拒——默认拒绝，不是默认放行）。
    pub fn new() -> Self {
        OverrideAuthority { grants: Vec::new() }
    }

    /// 授权条目数。
    pub fn len(&self) -> usize {
        self.grants.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.grants.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &Grant> {
        self.grants.iter()
    }

    /// 授予一层覆盖权（重复授予幂等合并）。
    pub fn grant(&mut self, holder: &str, layer: OverrideLayer) {
        if !self
            .grants
            .iter()
            .any(|g| g.holder == holder && g.layer == layer)
        {
            self.grants.push(Grant {
                holder: holder.to_string(),
                layer,
            });
        }
    }

    /// 撤销一层覆盖权。
    pub fn revoke(&mut self, holder: &str, layer: OverrideLayer) -> bool {
        let before = self.grants.len();
        self.grants
            .retain(|g| !(g.holder == holder && g.layer == layer));
        self.grants.len() != before
    }

    /// 授权判定（复杂度 C4：O(授权条目)）。
    ///
    /// **默认拒绝**：没有任何授权条目时一律 [`E_OVERRIDE_UNAUTHORIZED`]。
    /// 错误里带上"应申请哪一层"——拒绝要给出路，不是一句"权限不足"。
    pub fn authorize(&self, holder: &str, layer: OverrideLayer) -> Result<(), TokenError> {
        if layer.is_registry_layer() {
            return Err(TokenError::new(
                E_OVERRIDE_DEFAULT_LAYER,
                "覆盖被拒：默认层是单源本身，不可被覆盖",
                "默认层的值就是注册表的真值出处，覆盖它等于抹掉律一的唯一出处",
                "改在主题/场景/组件层覆盖，并让默认层保持真值",
                holder,
            ));
        }
        if self
            .grants
            .iter()
            .any(|g| g.holder == holder && g.layer == layer)
        {
            return Ok(());
        }
        let why = format!(
            "持有者 {} 未获 {} 层覆盖授权（现持授权：{}）",
            holder,
            layer.zh(),
            self.holders_summary()
        );
        let next = format!(
            "为 {} 申请 {} 层授权（OverrideAuthority::grant），或改在已授权层覆盖",
            holder,
            layer.zh()
        );
        Err(TokenError::new(
            E_OVERRIDE_UNAUTHORIZED,
            "覆盖被拒：越权",
            &why,
            &next,
            holder,
        ))
    }

    /// 现持授权摘要（进错误信息，让拒绝自解释）。
    pub fn holders_summary(&self) -> String {
        if self.grants.is_empty() {
            return "无".to_string();
        }
        let mut lines: Vec<String> = self
            .grants
            .iter()
            .map(|g| format!("{}@{}", g.holder, g.layer.zh()))
            .collect();
        lines.sort();
        lines.join("、")
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "覆盖授权表：{} 条授权，现持授权 {}",
            self.len(),
            self.holders_summary()
        )
    }
}

// ---------------------------------------------------------------------------
// 八、律二与求值：覆盖条目、出处、三级判定
// ---------------------------------------------------------------------------

/// 覆盖条目（层级值 + 出处）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Override {
    /// 目标令牌路径。
    pub path: String,
    /// 层级。
    pub layer: OverrideLayer,
    /// 覆盖值。
    pub value: String,
    /// 落账序号（与律二账本对齐——覆盖也必须有 seq）。
    pub seq: u64,
}

/// 值出处（律一反查承诺的载体：值与出处一起出去）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Provenance {
    /// 值来自哪一层。
    pub layer: OverrideLayer,
    /// 若来自覆盖层，其落账序号（默认层为 0）。
    pub seq: u64,
}

impl Provenance {
    /// 注册表默认层的出处。
    pub const REGISTRY: Provenance = Provenance {
        layer: OverrideLayer::Default,
        seq: 0,
    };

    /// 读屏文本（不靠颜色，靠文字说清"这值从哪来"）。
    pub fn screen_text(&self) -> String {
        if self.layer.is_registry_layer() {
            "出处：注册表真值".to_string()
        } else {
            format!("出处：{}层覆盖（第 {} 号变更）", self.layer.zh(), self.seq)
        }
    }
}

/// 求值结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// 真值（带出处）。
    Value {
        /// 值。
        value: String,
        /// 出处。
        provenance: Provenance,
    },
    /// 降级兜底（求值失败的诚实答复——不是空值也不是 0）。
    Fallback {
        /// 兜底值。
        value: String,
        /// 兜底原因（人话）。
        reason: String,
    },
}

impl Resolution {
    /// 取值（两种形态都给得出字符串——消费方不必先判形态再取值）。
    pub fn value(&self) -> &str {
        match self {
            Resolution::Value { value, .. } => value.as_str(),
            Resolution::Fallback { value, .. } => value.as_str(),
        }
    }

    /// 是否为降级兜底（K 域约定 K2：兜底值必须可辨）。
    pub fn is_fallback(&self) -> bool {
        matches!(self, Resolution::Fallback { .. })
    }

    /// 出处（降级态无出处可言，如实给 None）。
    pub fn provenance(&self) -> Option<Provenance> {
        match self {
            Resolution::Value { provenance, .. } => Some(*provenance),
            Resolution::Fallback { .. } => None,
        }
    }

    /// 读屏单行。
    pub fn screen_line(&self, path: &str) -> String {
        match self {
            Resolution::Value { value, provenance } => {
                format!("令牌 {} = {}，{}", path, value, provenance.screen_text())
            }
            Resolution::Fallback { value, reason } => {
                format!("令牌 {} 无真值，降级为 {}，原因：{}", path, value, reason)
            }
        }
    }
}

/// 令牌求值器（复杂度 C5：O(令牌数)）。
#[derive(Clone, Debug, Default)]
pub struct TokenResolver {
    registry: TokenRegistry,
    overrides: Vec<Override>,
}

impl TokenResolver {
    /// 以注册表构造。
    pub fn new(registry: TokenRegistry) -> Self {
        TokenResolver {
            registry,
            overrides: Vec::new(),
        }
    }

    /// 只读访问注册表。
    pub fn registry(&self) -> &TokenRegistry {
        &self.registry
    }

    /// 挂一条覆盖（**授权判定由调用方先用 [`OverrideAuthority`] 完成**——
    /// 授权与记账分离：授权管"能不能"，记账管"记不记"）。
    pub fn apply_override(&mut self, ov: Override) {
        // 同路径同层重复覆盖：后者取代前者（单层内唯一），保持覆盖表规模有界。
        self.overrides
            .retain(|o| !(o.path == ov.path && o.layer == ov.layer));
        self.overrides.push(ov);
    }

    /// 撤销某路径某层的覆盖。
    pub fn drop_override(&mut self, path: &str, layer: OverrideLayer) -> bool {
        let before = self.overrides.len();
        self.overrides
            .retain(|o| !(o.path == path && o.layer == layer));
        before != self.overrides.len()
    }

    /// 覆盖条目数。
    pub fn override_count(&self) -> usize {
        self.overrides.len()
    }

    /// 三级判定求值（判据降级矩阵第一格：求值失败→降级默认）。
    ///
    /// 1. 覆盖层命中（同路径取层级最高者——仲裁细则归 F3406，此处只要确定性）；
    /// 2. 注册表命中；
    /// 3. 都未命中 → 降级默认（[`fallback_for`]），结果标 [`Resolution::Fallback`]。
    pub fn resolve(&self, path: &str) -> Resolution {
        // 第 1 级：覆盖层（层级越高越优先）。
        let mut best: Option<&Override> = None;
        for ov in self.overrides.iter().filter(|o| o.path == path) {
            match best {
                None => best = Some(ov),
                Some(cur) if ov.layer > cur.layer => best = Some(ov),
                Some(_) => {}
            }
        }
        if let Some(ov) = best {
            return Resolution::Value {
                value: ov.value.clone(),
                provenance: Provenance {
                    layer: ov.layer,
                    seq: ov.seq,
                },
            };
        }
        // 第 2 级：注册表。
        if let Some(def) = self.registry.find(path) {
            return Resolution::Value {
                value: def.literal.clone(),
                provenance: Provenance::REGISTRY,
            };
        }
        // 第 3 级：降级默认。
        Resolution::Fallback {
            value: fallback_for(path).to_string(),
            reason: format!(
                "令牌 {} 既无覆盖也无注册记录（已知令牌 {} 个），按降级矩阵给默认值",
                path,
                self.registry.len()
            ),
        }
    }
}

/// 降级默认选取（复杂度 C6：O(默认表长)，最长前缀匹配）。
///
/// 无任何类别可依时给 [`UNSPECIFIED_FALLBACK`]——"未指定，继承"也是一句
/// 实话，比静默返回空串强。
pub fn fallback_for(path: &str) -> &'static str {
    let mut best: Option<&'static str> = None;
    let mut best_len = 0usize;
    for (prefix, value) in FALLBACK_DEFAULTS.iter() {
        if is_dot_prefix(path, prefix) && prefix.len() >= best_len {
            best = Some(value);
            best_len = prefix.len();
        }
    }
    best.unwrap_or(UNSPECIFIED_FALLBACK)
}

/// 点分前缀判定（`color.bg.canvas` 以 `color.bg` 为前缀；`color.bgx` 不算）。
///
/// 降级默认表按最长前缀匹配取值，这条判定是那条匹配的正确性根基，故对
/// 同域自检（`ver01_checks`）开放——降级判据要能单独复核前缀严格性。
pub fn is_dot_prefix(path: &str, prefix: &str) -> bool {
    if !path.starts_with(prefix) {
        return false;
    }
    match path.as_bytes().get(prefix.len()) {
        Some(b'.') => true,
        None => true,
        Some(_) => false,
    }
}

// ---------------------------------------------------------------------------
// 九、订阅与泄漏回收（错误路径：订阅泄漏→回收）
// ---------------------------------------------------------------------------

/// 一条订阅。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subscription {
    /// 订阅号。
    pub id: u64,
    /// 订阅主体（组件 id / 扩展 id）。
    pub owner: String,
    /// 关注的令牌路径。
    pub paths: Vec<String>,
    /// 出生 tick。
    pub born_tick: u64,
    /// 最近一次心跳 tick。
    pub last_tick: u64,
}

/// 回收记录（回收不是静默消失——留痕）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReclaimRecord {
    /// 被回收的订阅号。
    pub sub_id: u64,
    /// 主体。
    pub owner: String,
    /// 回收原因。
    pub cause: &'static str,
    /// 静默时长（tick；因主体注销而为 0）。
    pub silent_ticks: u64,
}

impl ReclaimRecord {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "回收订阅 #{}（主体 {}）：{}，静默 {} tick",
            self.sub_id, self.owner, self.cause, self.silent_ticks
        )
    }
}

/// 订阅表。
#[derive(Clone, Debug, Default)]
pub struct SubscriptionTable {
    subs: Vec<Subscription>,
    next_id: u64,
    reclaimed_total: u64,
}

impl SubscriptionTable {
    /// 空订阅表。
    pub fn new() -> Self {
        SubscriptionTable {
            subs: Vec::new(),
            next_id: 1,
            reclaimed_total: 0,
        }
    }

    /// 在册订阅数。
    pub fn len(&self) -> usize {
        self.subs.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.subs.is_empty()
    }

    /// 累计回收数（泄漏回收的显式计数）。
    pub fn reclaimed_total(&self) -> u64 {
        self.reclaimed_total
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &Subscription> {
        self.subs.iter()
    }

    /// 登记一条订阅。
    pub fn subscribe(
        &mut self,
        owner: &str,
        paths: &[&str],
        tick: u64,
    ) -> Result<u64, TokenError> {
        if owner.trim().is_empty() {
            return Err(TokenError::new(
                E_NO_OWNER,
                "订阅被拒：主体为空",
                "无主订阅无法判定存活，也无法在泄漏时定位责任方",
                "传入门/组件/扩展的主体 id",
                "订阅方",
            ));
        }
        if paths.is_empty() {
            return Err(TokenError::new(
                E_EMPTY_PATH,
                "订阅被拒：关注路径为空",
                "不关注任何令牌的订阅是空订阅，纯属浪费槽位",
                "至少给一个令牌路径",
                "订阅方",
            ));
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.subs.push(Subscription {
            id,
            owner: owner.to_string(),
            paths: paths.iter().map(|p| p.to_string()).collect(),
            born_tick: tick,
            last_tick: tick,
        });
        Ok(id)
    }

    /// 心跳（订阅方证明自己还活着）。
    pub fn touch(&mut self, id: u64, tick: u64) -> bool {
        match self.subs.iter_mut().find(|s| s.id == id) {
            Some(s) => {
                s.last_tick = tick;
                true
            }
            None => false,
        }
    }

    /// 主动退订（不算泄漏，不计入回收统计）。
    pub fn unsubscribe(&mut self, id: u64) -> bool {
        let before = self.subs.len();
        self.subs.retain(|s| s.id != id);
        before != self.subs.len()
    }

    /// 泄漏扫描与回收（复杂度 C7：O(订阅数)）。
    ///
    /// 两条判据（任一命中即回收，产出 [`ReclaimRecord`]）：
    /// 1. **主体已注销**：主体不在 `live_owners` 里——组件被卸载、扩展被禁用；
    /// 2. **静默超期**：`now - last_tick > SUB_TTL_TICKS`——订阅方失联，
    ///    留着就是永远等不到的回调（最贵的一种泄漏）。
    pub fn sweep(&mut self, live_owners: &[&str], now: u64) -> Vec<ReclaimRecord> {
        let mut records = Vec::new();
        let mut keep: Vec<Subscription> = Vec::with_capacity(self.subs.len());
        for s in self.subs.drain(..) {
            let alive = live_owners.iter().any(|o| *o == s.owner);
            let silent = now.saturating_sub(s.last_tick);
            if !alive {
                records.push(ReclaimRecord {
                    sub_id: s.id,
                    owner: s.owner.clone(),
                    cause: "主体已注销",
                    silent_ticks: silent,
                });
            } else if silent > SUB_TTL_TICKS {
                records.push(ReclaimRecord {
                    sub_id: s.id,
                    owner: s.owner.clone(),
                    cause: "静默超期",
                    silent_ticks: silent,
                });
            } else {
                keep.push(s);
            }
        }
        self.subs = keep;
        self.reclaimed_total = self.reclaimed_total.saturating_add(records.len() as u64);
        records
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "订阅表：在册 {} 条，累计回收 {} 条（静默超期阈值 {} tick）",
            self.len(),
            self.reclaimed_total(),
            SUB_TTL_TICKS
        )
    }
}

// ---------------------------------------------------------------------------
// 十、总纲本体：契约自检与读屏替代
// ---------------------------------------------------------------------------

/// 契约问题（五元组：代码/现象/根因/建议/严重度）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractIssue {
    /// 问题码。
    pub code: &'static str,
    /// 现象。
    pub symptom: String,
    /// 根因。
    pub cause: String,
    /// 建议。
    pub advice: String,
    /// 严重度：`blocker` 阻断 / `warn` 告警。
    pub severity: &'static str,
}

/// 令牌运行时总纲本体。
#[derive(Clone, Debug)]
pub struct Architecture {
    /// 总纲版本。
    pub version: &'static str,
    /// 四件。
    pub pieces: Vec<Piece>,
    /// 两律。
    pub laws: Vec<Law>,
    /// 单源审计器（律一强制点实例）。
    pub auditor: SingleSourceAuditor,
    /// 变更账本（律二强制点实例）。
    pub ledger: ChangeLedger,
    /// 覆盖授权表。
    pub authority: OverrideAuthority,
    /// 订阅表。
    pub subscriptions: SubscriptionTable,
}

impl Architecture {
    /// 标准总纲（空注册表 + 空账本 + 空授权——全部显式构造，不留隐式默认）。
    pub fn standard() -> Self {
        Architecture {
            version: ARCH_VERSION,
            pieces: PIECES.to_vec(),
            laws: LAWS.to_vec(),
            auditor: SingleSourceAuditor::new(TokenRegistry::new()),
            ledger: ChangeLedger::new(),
            authority: OverrideAuthority::new(),
            subscriptions: SubscriptionTable::new(),
        }
    }

    /// 契约自检：四件两律的声明不许断链、不许挂名、不许越界。
    ///
    /// 检出四类问题（总纲自己先做到可追溯，否则凭什么要求别人）：
    /// - `CONTRACT_MISSING`：件/律在册但契约缺失；
    /// - `CONTRACT_EMPTY`：契约字段留空（挂名）；
    /// - `CONTRACT_ORPHAN`：契约在册但没被任何件/律引用；
    /// - `CONTRACT_DUP`：同一件/律重复在册（唯一性）。
    pub fn check_contracts(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();

        // 四件：数量、唯一性、契约完整性、所服务的律在册。
        if self.pieces.len() != PIECES.len() {
            issues.push(ContractIssue {
                code: "CONTRACT_DUP",
                symptom: format!("四件在册 {} 件，应为 {} 件", self.pieces.len(), PIECES.len()),
                cause: "总纲件数被增删——四件是固定契约，不是可增长清单".to_string(),
                advice: "恢复为解析/求值/订阅/覆盖四件".to_string(),
                severity: "blocker",
            });
        }
        for p in PIECES.iter() {
            let registered = self.pieces.iter().filter(|x| *x == p).count();
            if registered != 1 {
                issues.push(ContractIssue {
                    code: "CONTRACT_DUP",
                    symptom: format!("件 {} 在册 {} 次", p.zh(), registered),
                    cause: "同一件被重复登记".to_string(),
                    advice: "去重后重新登记".to_string(),
                    severity: "blocker",
                });
            }
            let spec = piece_spec(*p);
            let fields = [
                spec.duty_zh,
                spec.duty_en,
                spec.input,
                spec.output,
                spec.on_failure,
                spec.complexity,
                spec.consumers,
                spec.not_mine,
            ];
            if fields.iter().any(|f| f.trim().is_empty()) {
                issues.push(ContractIssue {
                    code: "CONTRACT_EMPTY",
                    symptom: format!("件 {} 有空契约字段（挂名）", p.zh()),
                    cause: "契约字段留空——总纲不允许出现只有名字没有边界的件".to_string(),
                    advice: format!("补全 {} 的契约字段", p.zh()),
                    severity: "blocker",
                });
            }
            for law in spec_laws_of(*p) {
                if !self.laws.contains(law) {
                    issues.push(ContractIssue {
                        code: "CONTRACT_ORPHAN",
                        symptom: format!("件 {} 服务的律 {} 不在两律在册表中", p.zh(), law.zh()),
                        cause: "件引用了未宣告的律".to_string(),
                        advice: format!("把 {} 补入两律，或改该件的服务对象", law.zh()),
                        severity: "blocker",
                    });
                }
            }
        }

        // 两律：数量、唯一性、律文与强制点非空。
        if self.laws.len() != LAWS.len() {
            issues.push(ContractIssue {
                code: "CONTRACT_DUP",
                symptom: format!("两律在册 {} 条，应为 {} 条", self.laws.len(), LAWS.len()),
                cause: "总纲律数被增删——两律是固定契约".to_string(),
                advice: "恢复为令牌单源 + 变更有迹两律".to_string(),
                severity: "blocker",
            });
        }
        for l in LAWS.iter() {
            // `filter` 的闭包拿到的是 `&&Law`（迭代器项 `&Law` 的引用），
            // 右侧 `l` 是 `&Law`——两侧各解一层引用才是同类型比较。
            let registered = self.laws.iter().filter(|x| **x == *l).count();
            if registered != 1 {
                issues.push(ContractIssue {
                    code: "CONTRACT_DUP",
                    symptom: format!("律 {} 在册 {} 次", l.zh(), registered),
                    cause: "同一条律被重复登记".to_string(),
                    advice: "去重后重新登记".to_string(),
                    severity: "blocker",
                });
            }
            let spec = law_spec(*l);
            if spec.statement.trim().is_empty()
                || spec.violation.trim().is_empty()
                || spec.enforcement.trim().is_empty()
                || spec.promise.trim().is_empty()
            {
                issues.push(ContractIssue {
                    code: "CONTRACT_EMPTY",
                    symptom: format!("律 {} 声明不完整（律文/违例/强制点/承诺缺一）", l.zh()),
                    cause: "律的声明字段留空—— unenforceable 的律等于没写".to_string(),
                    advice: format!("补全律 {} 的四项声明", l.zh()),
                    severity: "blocker",
                });
            }
            // 律的强制点必须真有代码顶着（每条律至少被一件服务）。
            //
            // 遍历 `self.pieces`（**在册**的件）而非静态 `PIECES`：判据问的是
            // 「这份总纲宣称由谁执行」，答的是在册清单。若拿全集去数，任何总纲
            // 都恒有执行者——纸面律永远检不出来，检查就成了摆设。
            let served = self
                .pieces
                .iter()
                .filter(|p| spec_laws_of(**p).contains(l))
                .count();
            if served == 0 {
                issues.push(ContractIssue {
                    code: "CONTRACT_ORPHAN",
                    symptom: format!("律 {} 没有任何件在执行它", l.zh()),
                    cause: "律被宣告但无执行者——纸面律".to_string(),
                    advice: format!("指派至少一件来服务律 {}", l.zh()),
                    severity: "blocker",
                });
            }
        }

        issues
    }

    /// 无障碍：架构图的读屏替代（判据点名项）。
    ///
    /// 架构图对读屏用户是不可达的——本函数产出**线性文字版**：四件是什么、
    /// 数据怎么流、两律怎么守、失败了会怎样，全部念得出来。文字版与图版
    /// 同源（同一份 `PIECE_SPECS` / `LAW_SPECS`），不另写一份以免漂移。
    pub fn architecture_narration(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "设计令牌运行时架构 {}，共 {} 件 {} 律。",
            self.version,
            self.pieces.len(),
            self.laws.len()
        ));
        out.push_str("数据流顺序：");
        let mut stages: Vec<&'static str> = Vec::new();
        for p in PIECES.iter() {
            stages.push(p.zh());
        }
        // 按 stage 排序讲，保证读屏听到的是真实数据流次序。
        let mut ordered: Vec<Piece> = PIECES.to_vec();
        ordered.sort_by_key(|p| p.stage());
        let flow: Vec<&str> = ordered.iter().map(|p| p.zh()).collect();
        out.push_str(&flow.join("，"));
        out.push_str("，其中覆盖在求值之前介入仲裁。逐件说明：");
        for p in ordered.iter() {
            let s = piece_spec(*p);
            out.push_str(&format!(
                "第 {} 环，{}，职责是{}；吃{}，吐{}；失败则{}；复杂度{}；下游是{}；本件不做{}。",
                s.piece.stage(),
                s.piece.zh(),
                s.duty_zh,
                s.input,
                s.output,
                s.on_failure,
                s.complexity,
                s.consumers,
                s.not_mine
            ));
        }
        out.push_str("两律说明：");
        for l in LAWS.iter() {
            let s = law_spec(*l);
            out.push_str(&format!(
                "{}（{}），律文：{}；违例形态：{}；强制点：{}；对下游的承诺：{}。",
                s.law.code(),
                s.law.zh(),
                s.statement,
                s.violation,
                s.enforcement,
                s.promise
            ));
        }
        out.push_str(&format!(
            "K 域消费约定：{}",
            K_DOMAIN_CONTRACT.replace('\n', " ")
        ));
        out.push_str(&format!(
            "当前状态：{}；{}；{}。",
            self.auditor
                .registry()
                .len(),
            self.ledger.screen_text(),
            self.subscriptions.screen_text()
        ));
        let _ = stages;
        out
    }

    /// 读屏摘要（一行版）。
    pub fn screen_text(&self) -> String {
        format!(
            "令牌运行时 {}：四件{}，两律{}，在册令牌 {} 个，账本 {} 条变更（拒 {} 投 {}），{}",
            self.version,
            PIECES
                .iter()
                .map(|p| p.zh())
                .collect::<Vec<_>>()
                .join("/"),
            LAWS
                .iter()
                .map(|l| l.zh())
                .collect::<Vec<_>>()
                .join("/"),
            self.auditor.registry().len(),
            self.ledger.len(),
            self.ledger.rejected(),
            self.ledger.dropped(),
            self.subscriptions.screen_text()
        )
    }
}

impl Default for Architecture {
    fn default() -> Self {
        Self::standard()
    }
}

/// 取某件所服务的律（[`Piece::serves`] 的静态表版，便于契约自检遍历）。
///
/// `pub(crate)`：契约自检与自检单测都要按「在册件 → 服务了哪些律」反查，
/// 这条查询是判据的一部分（纸面律判定依赖它），不该只对模块内可见。
pub(crate) fn spec_laws_of(p: Piece) -> &'static [Law] {
    p.serves()
}

// ---------------------------------------------------------------------------
// 十一、错误五元组（零静默）
// ---------------------------------------------------------------------------

/// 入册超上限。
pub const E_TOKEN_CAP: &str = "E_TOKEN_CAP";
/// 路径为空。
pub const E_EMPTY_PATH: &str = "E_EMPTY_PATH";
/// 字面量为空。
pub const E_EMPTY_LITERAL: &str = "E_EMPTY_LITERAL";
/// 缺定义点。
pub const E_NO_CANONICAL_SITE: &str = "E_NO_CANONICAL_SITE";
/// 路径重复。
pub const E_DUPLICATE_PATH: &str = "E_DUPLICATE_PATH";
/// 无痕变更（缺主体或缺理由）。
pub const E_UNTRACEABLE: &str = "E_UNTRACEABLE";
/// 账本已满。
pub const E_LEDGER_FULL: &str = "E_LEDGER_FULL";
/// 覆盖越权。
pub const E_OVERRIDE_UNAUTHORIZED: &str = "E_OVERRIDE_UNAUTHORIZED";
/// 试图覆盖默认层。
pub const E_OVERRIDE_DEFAULT_LAYER: &str = "E_OVERRIDE_DEFAULT_LAYER";
/// 订阅无主。
pub const E_NO_OWNER: &str = "E_NO_OWNER";

/// 令牌运行时错误五元组（发生了什么/为什么/下一步/责任方/错误码）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenError {
    /// 错误码。
    pub code: &'static str,
    /// 发生了什么。
    pub what: &'static str,
    /// 为什么。
    pub why: String,
    /// 下一步（必填——拒绝必须给出路）。
    pub next: String,
    /// 责任方。
    pub who: String,
}

impl TokenError {
    fn new(
        code: &'static str,
        what: &'static str,
        why: &str,
        next: &str,
        who: &str,
    ) -> Self {
        TokenError {
            code,
            what,
            why: why.to_string(),
            next: next.to_string(),
            who: who.to_string(),
        }
    }

    /// 读屏可读的完整错误（三要素齐发：现象/原因/怎么办）。
    pub fn screen_text(&self) -> String {
        format!(
            "错误 {}：{}；原因：{}；下一步：{}；责任方：{}",
            self.code, self.what, self.why, self.next, self.who
        )
    }
}

// ---------------------------------------------------------------------------
// 十二、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F3401 域自检（判据逐条映射见 `ver01_checks.rs`）。
pub fn run_ver01_checks() -> CheckSet {
    super::ver01_checks::run_ver01_checks()
}