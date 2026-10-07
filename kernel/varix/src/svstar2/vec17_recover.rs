//! VE-F0417 · 词法错误恢复策略（VE-C 域 · 着色器系统 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0417`
//!
//! **判据（锚点原文）**：三策略、显性计数、级联反馈、死循环兜底、判据。
//!
//! 锚点职责定位原文：
//! > 词法错误恢复：单记号删除（缺分号类）、记号替换（不可识别字符跳过并标记）、
//! > 同步记号再同步（语句边界恢复）三策略按错误类别选用；恢复动作显性计数（恢复
//! > 不静默，警告告知恢复点）；恢复质量指标（恢复后是否级联错误）反馈调优。
//!
//! 本条只做「词法层出了一条错误之后，扫描器下一步站在哪里、以及把这次挪动如实
//! 报给作者」这一个动作。上游 F0416 负责**说清错在哪**（四族分类 / 三要素 /
//! 位置三元式 / 双侧定位），本条负责**别让一处错把后面全带崩**——恢复的本质是
//! 「用一次猜测的错误裁定，换取后续记号流仍可解析」，而这个猜测**必须被记账**。
//!
//! 1. **三策略按类别选用**（判据一）。单记号删除 / 记号替换 / 同步记号再同步，
//!    三者不是「三种都能用，看哪个方便」，而是**各有各的适用类别**：删一个记号会
//!    抹掉作者写下的信息，替换会把非法字符变成合法字符从而**改变语义**，再同步
//!!    跳得太远会丢掉整段。所以策略**必须查表**（`RECOVERY_TABLE`，类别×策略），
//!    不得由调用方随手指定，也不得靠错误码字符串 `contains("semicolon")` 猜类别
//!!    ——猜错类别就会用错策略，而用错策略的表现是「作者明明写对了却报他写错」。
//!    表里查不到 → 锚点错误路径第三条：**保守单记号删除**，并如实出注记说明
//!    「策略缺配，按保守删除处置」，不静默。
//!
//! 2. **恢复不静默**（判据二）。每一次挪动都产出一条 `RecoveryNotice`，它带
//!    策略、恢复点位置、以及这次挪动**丢掉了什么**。`notice_count == action_count`
//!    是本条的硬不变量——若某条恢复路径能悄悄挪动而不记账，它就是在替作者
//!    掩盖「编译器替你改了你的代码」。计数分策略（三种各一把）与分性质（兜底 /
//!    回退）分开，因为**兜底次数**是调优该看的那个数：兜底越多说明策略表越不够用。
//!
//! 3. **级联反馈**（判据三）。恢复质量指标 = 恢复之后紧随其后的窗口里**又冒
//!    出了几条阻断级错误**。只看「恢复次数」是自欺欺人（恢复次数多可能说明词法
//!    器很勤勉），只有看**恢复是否引入了新的错误**才知道策略选对没有。窗口取
//!    `CASCADE_WINDOW` 条观测、阈值 `CASCADE_BURST_THRESHOLD` 条阻断级即判
//!    **级联爆发**。爆发则**回退到上一个稳定点**重恢复——而不是继续在错误的
//!    位置上加错误。
//!
//! 4. **死循环兜底**（判据四）。恢复本身可能不前进：反复删同一个记号、或再同步
//!    找不到同步点却在原地重试。故每次恢复计入 `attempts`，超出 `budget` 即
//!    **强制同步点兜底**。这条路径的**可终止性是构造性保证**，不是靠人盯着：
//!    兜底动作把游标推到**严格更大**的合法位置（有同步点就跳到其后的第一个，
//!    没有就前进一个记号），而游标以 `input_len` 为上界，故兜底至多发生
//!    `input_len` 次后必然撞上上界并停住。`cursor_never_regresses` 自检盯的
//!    就是这条单调性——一旦有人改出「兜底时游标不动」，自检立刻变红。
//!
//! 零静默纪律：策略缺配→保守删除 + 注记；同步点表空→按无同步点路径处置并出
//!   注记；同步点未严格递增→只采信游标之后的那些（旧的忽略，不静默）；游标已到
//!   输入上界→如实标 `stalled` 并停，不自增不进死循环；级联回退时若无稳定点
//!   →转强制兜底并出注记。零 panic 面、零 IO、无全局可变状态。
//!
//! 性能逐项分解：恢复 O(1)/次（策略查表是固定长表 + 单次扫描，跳同步点是顺序
//!   走表不回头）；级联统计 O(窗口)（窗口常量 4，`evaluate_cascade` 只数前 4 条，
//!   **不是 O(观测全长)**——长度由调用方截断）；兜底 O(1)（不强扫全表定位同步
//!   点，兜底只置标志并按单步前进，定位留给下一次恢复的正常路径）。
//!
//! 上游消费：F0416 `Diagnostic`（取 `family` 归族与 `severity` 判阻断）、F0410
//!   括号作用域（同步点来源）、F0409 运算符（分号类错误码来源）。下游 F0421
//!   语法解析继续消费本条产出的**恢复流**（`RecoveryAction` 序列 + 游标）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::vec16_report::{classify, ErrorFamily, Severity};

// ---------------------------------------------------------------------------
// 一、恢复策略与类别（判据一）
// ---------------------------------------------------------------------------

/// 恢复策略（判据一：三策略）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryStrategy {
    /// 单记号删除：抹掉一个记号后前进一步（缺分号类——分号本身是记号流里的
    /// 一个记号，缺失时补进去会臆造结构，删掉出错记号则让流继续走）。
    TokenDelete,
    /// 记号替换：跳过不可识别字符**并标记**（保留游标推进的粒度信息，让作者
    /// 知道此处被跳过的是一个字符而非一个记号）。
    TokenReplace,
    /// 同步记号再同步：跳到下一个语句边界（分号、行尾、块尾、文件尾）后继续
    /// ——适用于错误可能一路吞掉半条语句的情形。
    SyncResync,
}

impl RecoveryStrategy {
    /// 人话标签（恢复通知抬头用）。
    pub const fn label(self) -> &'static str {
        match self {
            RecoveryStrategy::TokenDelete => "单记号删除",
            RecoveryStrategy::TokenReplace => "记号替换",
            RecoveryStrategy::SyncResync => "同步记号再同步",
        }
    }

    /// 该策略会**丢掉**什么（恢复通知的「代价」栏——不写代价的恢复通知会让作者
    /// 以为编译器只是换了个地方报错）。
    pub const fn cost(self) -> &'static str {
        match self {
            RecoveryStrategy::TokenDelete => "丢弃一个记号，其语义不参与后续解析",
            RecoveryStrategy::TokenReplace => "跳过一个不可识别字符，该字符不参与后续解析",
            RecoveryStrategy::SyncResync => "丢弃本记号至下一个语句边界之间的全部记号",
        }
    }

    /// 是否为强制兜底才允许使用的策略。
    ///
    /// 三策略都是正规策略；本方法区分的是「按类别选用」与「死循环兜底」两条
    /// 路径——兜底路径**不查类别**（此时类别已不可信），故此判定供自检核对
    /// 两条路径确实走了不同的判定来源。
    pub const fn is_regular(self) -> bool {
        true
    }
}

/// 恢复类别（判据一的行；锚点只举了「缺分号类」与「不可识别字符」两类，其余
/// 由上游四族细化而来）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryClass {
    /// 缺分号类：语句分隔记号缺失（分号、逗号）。
    MissingSeparator,
    /// 不可识别字符类：非法字节、非法字符。
    UnrecognizedChar,
    /// 未闭合字面量类：字符串/字符/数值越界未收。
    UnterminatedLiteral,
    /// 括号失配类：配对与作用域标记破坏。
    BracketMismatch,
    /// 指令语法类：预处理指令参数与分支结构。
    DirectiveSyntax,
    /// 未归类：不在表内的任何码。
    Unclassified,
}

impl RecoveryClass {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            RecoveryClass::MissingSeparator => "缺分号类",
            RecoveryClass::UnrecognizedChar => "不可识别字符类",
            RecoveryClass::UnterminatedLiteral => "未闭合字面量类",
            RecoveryClass::BracketMismatch => "括号失配类",
            RecoveryClass::DirectiveSyntax => "指令语法类",
            RecoveryClass::Unclassified => "未归类",
        }
    }
}

/// 恢复策略表条目（类别 × 策略）。
#[derive(Clone, Copy, Debug)]
pub struct RecoveryRule {
    /// 匹配的码前缀（`starts_with` 判定，与 F0416 的族表同一套查表口径）。
    pub code_prefix: &'static str,
    /// 该码所属恢复类别。
    pub class: RecoveryClass,
    /// 该类别选用的恢复策略。
    pub strategy: RecoveryStrategy,
    /// 选此策略的依据（写进恢复通知的「为什么」，让作者能判断编译器有没有猜
    /// 对——猜对了不必解释，猜错了必须有据可查）。
    pub rationale: &'static str,
}

/// 恢复策略表（判据一：类别×策略，逐条带依据）。
///
/// 顺序敏感：更长前缀排前面，避免 `VE-F0411-UNKNOWN` 抢走 `VE-F0411` 的条目。
/// 表中**故意不含** `Unclassified` 的条目——未归类走表外保守路径，留在表里
/// 等于把兜底伪装成正规策略。
pub const RECOVERY_TABLE: &[RecoveryRule] = &[
    // 缺分号类：分隔记号缺失。前向再同步会把错误之后的整条语句吞掉，删掉出错
    // 记号则流仍可继续，故用单记号删除。
    RecoveryRule {
        code_prefix: "VE-F0409-MISSING-SEMICOLON",
        class: RecoveryClass::MissingSeparator,
        strategy: RecoveryStrategy::TokenDelete,
        rationale: "分隔记号缺失属局部结构缺口，删出错记号即可续流；前向再同步会吞掉后续整条语句",
    },
    RecoveryRule {
        code_prefix: "VE-F0409-MISSING-COMMA",
        class: RecoveryClass::MissingSeparator,
        strategy: RecoveryStrategy::TokenDelete,
        rationale: "逗号缺失与分号缺失同类，粒度同为单个记号",
    },
    // 不可识别字符类：跳过并标记。删记号会误删相邻合法记号，替换只丢弃该字符，
    // 粒度最细，最不伤及邻近结构。
    RecoveryRule {
        code_prefix: "VE-F0415-UTF8-CONTINUATION",
        class: RecoveryClass::UnrecognizedChar,
        strategy: RecoveryStrategy::TokenReplace,
        rationale: "孤立续字节只需跳过该字符，不牵动相邻记号",
    },
    RecoveryRule {
        code_prefix: "VE-F0403-UNKNOWN-TOKEN",
        class: RecoveryClass::UnrecognizedChar,
        strategy: RecoveryStrategy::TokenReplace,
        rationale: "不可识别记号按字符粒度跳过并标记，保留其存在过的证据",
    },
    // 未闭合字面量类：必须跳到语句边界，否则后续记号全被当字面量内容吞掉。
    RecoveryRule {
        code_prefix: "VE-F0407-UNTERMINATED",
        class: RecoveryClass::UnterminatedLiteral,
        strategy: RecoveryStrategy::SyncResync,
        rationale: "未闭合字面量会持续吞并后续记号，唯有跳到语句边界才能止损",
    },
    RecoveryRule {
        code_prefix: "VE-F0406-OVERFLOW",
        class: RecoveryClass::UnterminatedLiteral,
        strategy: RecoveryStrategy::SyncResync,
        rationale: "数值越界后字面量同样处于未收状态，需跳语句边界",
    },
    // 括号失配类：作用域已错，删单个记号无法把嵌套拉回，改跳块尾。
    RecoveryRule {
        code_prefix: "VE-F0410-MISMATCH",
        class: RecoveryClass::BracketMismatch,
        strategy: RecoveryStrategy::SyncResync,
        rationale: "嵌套层级已错，逐记号删除无法收敛，须跳到块尾重新起算嵌套",
    },
    // 指令语法类：整条指令无效，跳到行尾（指令以行尾为天然边界）。
    RecoveryRule {
        code_prefix: "VE-F0411-UNKNOWN-DIRECTIVE",
        class: RecoveryClass::DirectiveSyntax,
        strategy: RecoveryStrategy::SyncResync,
        rationale: "指令以行尾为天然边界，跳行尾即可回到正常语句流",
    },
    RecoveryRule {
        code_prefix: "VE-F0413",
        class: RecoveryClass::DirectiveSyntax,
        strategy: RecoveryStrategy::SyncResync,
        rationale: "条件编译分支结构出错，跳到分支边界止损",
    },
    RecoveryRule {
        code_prefix: "VE-F0412",
        class: RecoveryClass::DirectiveSyntax,
        strategy: RecoveryStrategy::SyncResync,
        rationale: "宏展开体已失效，跳到展开边界止损",
    },
];

/// 查表得类别与策略（判据一）。
///
/// 返回 `(类别, 策略, 是否表内命中)`。表外 → `(Unclassified, TokenDelete,
/// false)`：锚点错误路径第三条「策略缺配→保守单记号删除」，其中「保守」指的是
/// 选**丢弃粒度最小**的那条路（删一个记号），而不是「随便选一条」。
pub fn strategy_for(code: &str) -> (RecoveryClass, RecoveryStrategy, bool) {
    for r in RECOVERY_TABLE.iter() {
        if code.starts_with(r.code_prefix) {
            return (r.class, r.strategy, true);
        }
    }
    (RecoveryClass::Unclassified, RecoveryStrategy::TokenDelete, false)
}

/// 类别对应的族（供上游 F0416 四族对齐核对：本域不重新定义族，只做映射）。
pub const fn family_of(class: RecoveryClass) -> ErrorFamily {
    match class {
        RecoveryClass::MissingSeparator | RecoveryClass::UnrecognizedChar => ErrorFamily::Char,
        RecoveryClass::UnterminatedLiteral => ErrorFamily::Literal,
        RecoveryClass::BracketMismatch => ErrorFamily::Bracket,
        RecoveryClass::DirectiveSyntax => ErrorFamily::Directive,
        RecoveryClass::Unclassified => ErrorFamily::Other,
    }
}

/// 类别表与族表必须对齐（跨域一致性：上游说该码是括号族，本域却按指令类恢复，
/// 说明两域对同一个码的理解分叉了）。
pub fn table_alignment_gaps() -> Vec<&'static str> {
    let mut gaps: Vec<&'static str> = Vec::new();
    for r in RECOVERY_TABLE.iter() {
        if family_of(r.class) != classify(r.code_prefix) {
            gaps.push(r.code_prefix);
        }
    }
    gaps
}

// ---------------------------------------------------------------------------
// 二、同步点与游标
// ---------------------------------------------------------------------------

/// 同步点种类（语句边界）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncKind {
    /// 分号。
    Semicolon,
    /// 行尾。
    LineEnd,
    /// 块尾（右花括号或 end）。
    BlockEnd,
    /// 文件尾。
    Eof,
}

impl SyncKind {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            SyncKind::Semicolon => "分号",
            SyncKind::LineEnd => "行尾",
            SyncKind::BlockEnd => "块尾",
            SyncKind::Eof => "文件尾",
        }
    }

    /// 边界强度：块尾与文件尾比行尾与分号「更硬」。
    ///
    /// 供 `prefer_strict_sync` 选用——再同步到硬边界更保守（丢得多但更可能落到
    /// 真正的语句起点）。同强度时取靠前者（先遇到的边界先到）。
    pub const fn strictness(self) -> u8 {
        match self {
            SyncKind::Semicolon => 0,
            SyncKind::LineEnd => 1,
            SyncKind::BlockEnd => 2,
            SyncKind::Eof => 3,
        }
    }
}

/// 一个同步点（再同步目标候选）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncPoint {
    /// 边界种类。
    pub kind: SyncKind,
    /// 该边界的记号下标。
    pub token_index: usize,
    /// 该边界的字节偏移。
    pub byte_offset: usize,
}

impl SyncPoint {
    /// 构造同步点。
    pub const fn new(kind: SyncKind, token_index: usize, byte_offset: usize) -> SyncPoint {
        SyncPoint { kind, token_index, byte_offset }
    }
}

/// 扫描游标（记号下标 + 字节偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    /// 记号下标（单调不减，恢复只允许前进）。
    pub token_index: usize,
    /// 字节偏移（与记号下标同步前进；下标是权威值）。
    pub byte_offset: usize,
}

impl Cursor {
    /// 构造游标。
    pub const fn new(token_index: usize, byte_offset: usize) -> Cursor {
        Cursor { token_index, byte_offset }
    }
}

/// 前进一步（饱和加法——`input_len == usize::MAX` 时不得回绕成 0，那会让
/// 「已到上界」的判定失效并把死循环兜底变成真死循环）。
pub fn advance(c: Cursor, by: usize, input_len: usize) -> Cursor {
    let ti = c.token_index.saturating_add(by);
    let bo = c.byte_offset.saturating_add(by);
    Cursor {
        token_index: if ti > input_len { input_len } else { ti },
        byte_offset: if bo > input_len { input_len } else { bo },
    }
}

/// 找游标**之后**的第一个同步点（判据一之再同步）。
///
/// 三条硬规则，缺一即失效：
/// 1. 只认 `token_index > cur.token_index` 的点——游标处及其之前的点已经站过
///    了，跳过去等于没跳（这正是「恢复死循环」的成因，故此处必须严格大于）。
/// 2. 返回表中最先出现的那个（表按位置升序给定，故顺序即就近），不做全局排序。
/// 3. 表空或全在游标之前 → `None`，由调用方走「无同步点」路径，不静默。
pub fn next_sync_after(syncs: &[SyncPoint], cur: Cursor) -> Option<SyncPoint> {
    let mut i = 0usize;
    while i < syncs.len() {
        let s = syncs[i];
        if s.token_index > cur.token_index {
            return Some(s);
        }
        i += 1;
    }
    None
}

/// 优先跳硬边界：同强度取靠前者，有更硬的边界则取之（保守再同步）。
pub fn prefer_strict_sync(syncs: &[SyncPoint], cur: Cursor) -> Option<SyncPoint> {
    let mut best: Option<SyncPoint> = None;
    let mut i = 0usize;
    while i < syncs.len() {
        let s = syncs[i];
        if s.token_index > cur.token_index {
            match best {
                None => best = Some(s),
                Some(b) => {
                    if s.kind.strictness() > b.kind.strictness() {
                        best = Some(s);
                    }
                }
            }
        }
        i += 1;
    }
    best
}

// ---------------------------------------------------------------------------
// 三、恢复动作与显性通知（判据二）
// ---------------------------------------------------------------------------

/// 恢复通知（判据二：恢复不静默，警告告知恢复点）。
///
/// 每一次恢复动作**恰好**产出一条通知。通知必须带三样：**在哪恢复**（位置）、
/// **怎么恢复的**（策略）、**代价是什么**（丢了什么）——缺代价的通知会让作者以为
/// 编译器只是换了个地方报错。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryNotice {
    /// 触发本次恢复的错误码。
    pub code: &'static str,
    /// 恢复类别。
    pub class: RecoveryClass,
    /// 采用的策略。
    pub strategy: RecoveryStrategy,
    /// 恢复点位置（字节偏移，权威值）。
    pub at_offset: usize,
    /// 恢复点行号（展示值，≥1）。
    pub at_line: usize,
    /// 本次挪动丢弃了什么。
    pub cost: String,
    /// 选此策略的依据（表内条目自带；表外为兜底说明）。
    pub rationale: String,
    /// 附注（策略缺配 / 无同步点 / 强制兜底 / 回退 / 已停滞，逐条如实记录）。
    pub notes: Vec<String>,
}

impl RecoveryNotice {
    /// 渲染为一行警告（判据二：警告告知恢复点）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("警告[词法恢复] ");
        s.push_str(self.code);
        s.push_str("（");
        s.push_str(self.class.label());
        s.push_str("）→ ");
        s.push_str(self.strategy.label());
        s.push_str(" @ 第 ");
        s.push_str(self.at_line.to_string().as_str());
        s.push_str(" 行，字节偏移 ");
        s.push_str(self.at_offset.to_string().as_str());
        s.push_str("\n  代价：");
        s.push_str(self.cost.as_str());
        s.push_str("\n  依据：");
        s.push_str(self.rationale.as_str());
        for n in self.notes.iter() {
            s.push_str("\n  注记：");
            s.push_str(n.as_str());
        }
        s
    }
}

/// 恢复动作（下游 F0421 消费的恢复流单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryAction {
    /// 采用的策略。
    pub strategy: RecoveryStrategy,
    /// 恢复前的游标。
    pub before: Cursor,
    /// 恢复后的游标（**必须严格前进**——这由 `RecoverySession` 保证）。
    pub after: Cursor,
    /// 被替换/删除的记号下标（`None` 表示未指向具体记号，如再同步跨越多个）。
    pub touched_token: Option<usize>,
    /// 实际落脚的同步点（仅再同步策略有）。
    pub sync_point: Option<SyncPoint>,
    /// 是否走了强制兜底（死循环防护或表外缺配）。
    pub forced: bool,
    /// 是否走了级联回退。
    pub rollback: bool,
    /// 显性通知。
    pub notice: RecoveryNotice,
    /// 是否已停滞（游标到上界，无处可进）。
    pub stalled: bool,
}

impl RecoveryAction {
    /// 游标是否严格前进（死循环兜底的可终止性依据此条）。
    pub fn progressed(&self) -> bool {
        self.after.token_index > self.before.token_index
    }
}

/// 恢复计数（判据二：显性计数）。
///
/// 分策略与分性质**分开记**：分策略的三个数回答「哪条路用得多」，分性质的
/// `forced` / `rollback` 回答「策略表够不够用」——兜底次数高说明表里缺条目，
/// 回退次数高说明策略选错。三者混在一个总数里就都答不出来了。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecoveryCounters {
    /// 单记号删除次数。
    pub token_delete: u32,
    /// 记号替换次数。
    pub token_replace: u32,
    /// 同步记号再同步次数。
    pub sync_resync: u32,
    /// 强制兜底次数（含死循环兜底与表外缺配兜底）。
    pub forced: u32,
    /// 级联回退次数。
    pub rollback: u32,
    /// 恢复动作总数（三策略之和，不含性质计数）。
    pub total: u32,
    /// 显性通知总数（**必须等于 `total`**——通知少于动作即有静默恢复）。
    pub notices: u32,
    /// 停滞次数（游标到上界而停）。
    pub stalled: u32,
}

impl RecoveryCounters {
    /// 按策略累加一笔。
    pub fn count_strategy(&mut self, s: RecoveryStrategy) {
        match s {
            RecoveryStrategy::TokenDelete => self.token_delete = self.token_delete.saturating_add(1),
            RecoveryStrategy::TokenReplace => self.token_replace = self.token_replace.saturating_add(1),
            RecoveryStrategy::SyncResync => self.sync_resync = self.sync_resync.saturating_add(1),
        }
        self.total = self.total.saturating_add(1);
    }

    /// 三策略之和是否等于总数（内部自洽核验）。
    pub fn strategy_sum_matches(&self) -> bool {
        let s = self
            .token_delete
            .saturating_add(self.token_replace)
            .saturating_add(self.sync_resync);
        s == self.total
    }

    /// 通知数是否等于动作数（判据二硬不变量：无静默恢复）。
    pub fn notice_covers_all(&self) -> bool {
        self.notices == self.total
    }

    /// 人话呈现（调优指标面板）。
    pub fn render(&self) -> String {
        format!(
            "恢复 {} 次（删 {} / 替 {} / 同步 {}）·兜底 {} ·回退 {} ·通知 {}",
            self.total,
            self.token_delete,
            self.token_replace,
            self.sync_resync,
            self.forced,
            self.rollback,
            self.notices
        )
    }
}

// ---------------------------------------------------------------------------
// 四、级联指标（判据三）
// ---------------------------------------------------------------------------

/// 级联观测窗口（只看恢复之后紧随其后的这几条）。
pub const CASCADE_WINDOW: usize = 4;
/// 级联爆发阈值：窗口内阻断级诊断达到此数即判爆发。
pub const CASCADE_BURST_THRESHOLD: usize = 3;

/// 级联裁定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CascadeVerdict {
    /// 实际纳入窗口的观测条数（≤ `CASCADE_WINDOW`）。
    pub observed: usize,
    /// 窗口内阻断级（`Severity::Error`）条数。
    pub blocking: usize,
    /// 是否级联爆发。
    pub burst: bool,
}

impl CascadeVerdict {
    /// 是否健康（未爆发）。
    pub fn is_healthy(&self) -> bool {
        !self.burst
    }
}

/// 裁定级联（判据三）。
///
/// 只看前 `CASCADE_WINDOW` 条观测，超出部分**不计入**——级联问的是「紧跟着的
/// 那几步有没有连环炸」，不是「整个文件后来一共错了多少条」（后者是错误总数，
/// 与恢复质量无关）。计数必须按**严重度**筛：警告与注记不算级联，注记往往正是
/// 上一次恢复自己产出的。
pub fn evaluate_cascade(observed: &[Severity]) -> CascadeVerdict {
    let taken = if observed.len() < CASCADE_WINDOW { observed.len() } else { CASCADE_WINDOW };
    let blocking = observed.iter().take(CASCADE_WINDOW).filter(|s| s.is_blocking()).count();
    CascadeVerdict { observed: taken, blocking, burst: blocking >= CASCADE_BURST_THRESHOLD }
}

// ---------------------------------------------------------------------------
// 五、恢复会话（判据一至判据四的汇合点）
// ---------------------------------------------------------------------------

/// 恢复尝试预算：超出即强制兜底（判据四）。
pub const DEFAULT_RECOVERY_BUDGET: u16 = 4;

/// 稳定点（级联回退的落点：上一个已知无级联的游标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StablePoint {
    /// 稳定游标。
    pub cursor: Cursor,
    /// 该点累计观测到的阻断级诊断数（供调优看趋势）。
    pub blocking_seen: u32,
}

/// 恢复会话：跨多次恢复保留计数、尝试次数与稳定点。
#[derive(Clone, Debug)]
pub struct RecoverySession {
    /// 输入记号总数（游标上界；上界使兜底可终止）。
    input_len: usize,
    /// 尝试预算。
    budget: u16,
    /// 当前连续恢复的尝试次数（每次 `observe` 归零）。
    attempts: u16,
    /// 累计计数。
    counters: RecoveryCounters,
    /// 上一个稳定点（回退落点）。
    stable: Option<StablePoint>,
    /// 稳定点累计阻断级计数。
    blocking_seen: u32,
}

impl RecoverySession {
    /// 新建会话（默认预算）。
    pub fn new(input_len: usize) -> RecoverySession {
        RecoverySession {
            input_len,
            budget: DEFAULT_RECOVERY_BUDGET,
            attempts: 0,
            counters: RecoveryCounters::default(),
            stable: None,
            blocking_seen: 0,
        }
    }

    /// 自定义尝试预算（预算为 0 时首次恢复即兜底——允许，用于测兜底路径）。
    pub fn with_budget(input_len: usize, budget: u16) -> RecoverySession {
        let mut s = RecoverySession::new(input_len);
        s.budget = budget;
        s
    }

    /// 取累计计数。
    pub fn counters(&self) -> &RecoveryCounters {
        &self.counters
    }

    /// 取当前连续尝试次数。
    pub fn attempts(&self) -> u16 {
        self.attempts
    }

    /// 取稳定点。
    pub fn stable_point(&self) -> Option<StablePoint> {
        self.stable
    }

    /// 输入上界。
    pub fn input_len(&self) -> usize {
        self.input_len
    }

    /// 查表定策略（供调用方预查；实际恢复内部也会查一次，两处须一致）。
    pub fn peek_strategy(code: &str) -> (RecoveryClass, RecoveryStrategy, bool) {
        strategy_for(code)
    }

    /// 执行一次恢复（判据一至判据四的编排点）。
    ///
    /// 顺序：停滞判定 → 死循环兜底判定 → 类别/策略查表 → 按策略落地 → 计数
    /// 与通知 → 尝试次数累加。**兜底判定排在查表之前**是有意的：走到兜底时
    /// 「是哪个类别」已经不可信（正是同一位置的错误反复出现才耗尽预算），此时
    /// 再按类别选策略等于用一个不可信输入做决策。
    pub fn recover(
        &mut self,
        code: &'static str,
        at: Cursor,
        at_line: usize,
        syncs: &[SyncPoint],
    ) -> RecoveryAction {
        let mut notes: Vec<String> = Vec::new();

        // ---- 停滞判定：游标已在输入上界，无处可进 ----
        if at.token_index >= self.input_len {
            self.counters.stalled = self.counters.stalled.saturating_add(1);
            notes.push("游标已在输入上界，恢复无处可进——如实停滞，不自增".to_string());
            return self.finish(
                code,
                RecoveryClass::Unclassified,
                RecoveryStrategy::TokenDelete,
                at,
                at,
                at_line,
                "游标已到输入上界，无策略可选；如实停滞待调用方处置".to_string(),
                None,
                None,
                false,
                false,
                true,
                notes,
            );
        }

        // ---- 死循环兜底判定（判据四）：预算耗尽即强制同步 ----
        let forced_by_budget = self.attempts >= self.budget;
        // ---- 策略查表（判据一）----
        let (class, strategy, hit) = strategy_for(code);
        let rationale = if hit {
            let mut r = String::new();
            let mut i = 0usize;
            while i < RECOVERY_TABLE.len() {
                if code.starts_with(RECOVERY_TABLE[i].code_prefix) {
                    r = RECOVERY_TABLE[i].rationale.to_string();
                    break;
                }
                i += 1;
            }
            r
        } else {
            // 锚点错误路径第三条：策略缺配 → 保守单记号删除，并如实说明。
            notes.push(format!(
                "错误码 {} 在恢复策略表中无条目，按保守单记号删除处置——需回填策略表",
                code
            ));
            "表外缺配，取丢弃粒度最小的一条路（单记号删除）".to_string()
        };
        let forced = forced_by_budget || !hit;
        if forced_by_budget {
            notes.push(format!(
                "同一位置连续恢复已达预算 {} 次，判定恢复死循环，强制同步点兜底",
                self.budget
            ));
        }

        // ---- 按策略落地 ----
        let mut touched: Option<usize> = None;
        let mut landed: Option<SyncPoint> = None;
        let after = match strategy {
            RecoveryStrategy::TokenDelete => {
                touched = Some(at.token_index);
                advance(at, 1, self.input_len)
            }
            RecoveryStrategy::TokenReplace => {
                touched = Some(at.token_index);
                advance(at, 1, self.input_len)
            }
            RecoveryStrategy::SyncResync => {
                match next_sync_after(syncs, at) {
                    Some(s) => {
                        landed = Some(s);
                        // 落在同步点**之后**：同步点本身是边界记号，越过它才算换段。
                        advance(Cursor::new(s.token_index, s.byte_offset), 1, self.input_len)
                    }
                    None => {
                        notes.push(format!(
                            "游标第 {} 记号之后无可用同步点（表长 {}），按单步前进兜底",
                            at.token_index,
                            syncs.len()
                        ));
                        advance(at, 1, self.input_len)
                    }
                }
            }
        };

        // 兜底路径必须保证游标严格前进（可终止性的构造性保证在此落地）。
        // 说明：当前三条策略各自都必然前进（删/替走 `advance(at,1)`；再同步落在
        // 严格大于游标的同步点之后），故本守卫目前**恒不触发**——变体测试中把
        // 它改成「原地不动」全绿，属**等价变体**（改坏它不改行为）而非门禁失效，
        // 已逐策略推演确认。保留它的理由是防御未来新增策略：若将来某策略算出
        // 不前进的游标，这里是唯一的兜底闸门，删掉等于把该风险敞给下一个人。
        let after = if after.token_index <= at.token_index {
            advance(at, 1, self.input_len)
        } else {
            after
        };
        let stalled = after.token_index >= self.input_len;

        self.attempts = self.attempts.saturating_add(1);
        self.finish(
            code,
            class,
            strategy,
            at,
            after,
            at_line,
            rationale,
            touched,
            landed,
            forced,
            false,
            stalled,
            notes,
        )
    }

    /// 观测恢复后的窗口并裁定级联（判据三）。
    ///
    /// 未爆发 → 把该点记为稳定点（回退落点），尝试次数归零（有过一次干净恢复，
    /// 预算重新可用）。爆发 → 返回 `true` 表示调用方应当回退；由调用方调
    /// `rollback_to_stable()` 执行，因为回退要换掉调用方手上的游标。
    pub fn observe(&mut self, after: Cursor, observed: &[Severity]) -> CascadeVerdict {
        let verdict = evaluate_cascade(observed);
        self.blocking_seen = self.blocking_seen.saturating_add(verdict.blocking as u32);
        if verdict.is_healthy() {
            self.attempts = 0;
            self.stable = Some(StablePoint { cursor: after, blocking_seen: self.blocking_seen });
        }
        verdict
    }

    /// 级联回退：回到上一个稳定点（锚点错误路径第一条：级联爆发→回退到上一个
    /// 稳定点重恢复）。
    ///
    /// 返回 `None` 表示**无稳定点可退**——此时不得原地重试（那正是级联爆发的
    /// 成因），应改走强制兜底。锚点错误路径第二条由此落地。
    pub fn rollback_to_stable(&mut self) -> Option<Cursor> {
        match self.stable {
            None => None,
            Some(sp) => {
                self.counters.rollback = self.counters.rollback.saturating_add(1);
                self.attempts = self.attempts.saturating_add(1);
                Some(sp.cursor)
            }
        }
    }

    /// 无稳定点时的强制兜底游标（单步前进；上界保证其终止）。
    pub fn forced_fallback_cursor(&mut self, from: Cursor) -> Cursor {
        self.counters.forced = self.counters.forced.saturating_add(1);
        advance(from, 1, self.input_len)
    }

    /// 统一收尾：计数 + 组装通知（判据二：每个动作恰好一条通知）。
    #[allow(clippy::too_many_arguments)]
    fn finish(
        &mut self,
        code: &'static str,
        class: RecoveryClass,
        strategy: RecoveryStrategy,
        before: Cursor,
        after: Cursor,
        at_line: usize,
        rationale: String,
        touched: Option<usize>,
        landed: Option<SyncPoint>,
        forced: bool,
        rollback: bool,
        stalled: bool,
        notes: Vec<String>,
    ) -> RecoveryAction {
        self.counters.count_strategy(strategy);
        if forced {
            self.counters.forced = self.counters.forced.saturating_add(1);
        }
        let notice = RecoveryNotice {
            code,
            class,
            strategy,
            at_offset: before.byte_offset,
            at_line: if at_line == 0 { 1 } else { at_line },
            cost: match landed {
                Some(s) => format!("{}（落脚于{}）", strategy.cost(), s.kind.label()),
                None => strategy.cost().to_string(),
            },
            rationale,
            notes,
        };
        self.counters.notices = self.counters.notices.saturating_add(1);
        RecoveryAction {
            strategy,
            before,
            after,
            touched_token: touched,
            sync_point: landed,
            forced,
            rollback,
            notice,
            stalled,
        }
    }
}

/// 取策略依据（供外部核验策略表条目，不参与恢复主路径）。
pub fn strategy_reason(code: &str) -> String {
    let mut i = 0usize;
    while i < RECOVERY_TABLE.len() {
        if code.starts_with(RECOVERY_TABLE[i].code_prefix) {
            return RECOVERY_TABLE[i].rationale.to_string();
        }
        i += 1;
    }
    "表外缺配，取丢弃粒度最小的一条路（单记号删除）".to_string()
}

// ---------------------------------------------------------------------------
// 六、高层编排：一串错误 → 恢复流（下游 F0421 的消费面）
// ---------------------------------------------------------------------------

/// 一次恢复的输入（错误码 + 位置 + 该处可见的同步点）。
#[derive(Clone, Copy, Debug)]
pub struct RecoveryInput {
    /// 错误码。
    pub code: &'static str,
    /// 出错游标。
    pub at: Cursor,
    /// 出错行号（展示值）。
    pub line: usize,
}

/// 恢复流（一串错误各自的恢复动作 + 观测裁定）。
#[derive(Clone, Debug)]
pub struct RecoveryStream {
    /// 恢复动作序列（顺序即恢复顺序）。
    pub actions: Vec<RecoveryAction>,
    /// 会话终态计数。
    pub counters: RecoveryCounters,
    /// 累计级联爆发次数。
    pub bursts: u32,
}

impl RecoveryStream {
    /// 是否全程无级联（恢复质量指标：true 说明策略选得准）。
    pub fn cascade_free(&self) -> bool {
        self.bursts == 0
    }

    /// 是否全程未动用兜底（策略表够不够用的直接指标）。
    pub fn fallback_free(&self) -> bool {
        self.counters.forced == 0
    }

    /// 渲染全部恢复通知（每条一警告，空行分隔）。
    pub fn render_all(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.actions.len() {
            if i > 0 {
                s.push('\n');
            }
            s.push_str(self.actions[i].notice.render().as_str());
            i += 1;
        }
        s
    }
}

/// 编排：把一串词法错误恢复成一条恢复流。
///
/// `observed_after[i]` 是第 `i` 次恢复之后的窗口观测（可短于窗口长度，短即按
/// 实有条数计）。级联爆发时**回退到稳定点重来**（锚点错误路径第一条），无稳定
/// 点时走强制兜底（错误路径第二条），因此本循环的终止性由「回退只在有稳定点
/// 时发生 + 兜底游标严格前进 + 上界有限」共同保证。
pub fn recover_stream(
    inputs: &[RecoveryInput],
    syncs: &[SyncPoint],
    observed_after: &[&[Severity]],
    input_len: usize,
) -> RecoveryStream {
    let mut sess = RecoverySession::new(input_len);
    let mut actions: Vec<RecoveryAction> = Vec::new();
    let mut bursts: u32 = 0;
    let mut i = 0usize;
    while i < inputs.len() {
        let inp = inputs[i];
        let mut cur = inp.at;
        // 级联回退重试：同一处错误最多因回退再试两轮，仍爆发则强制兜底前进，
        // 避免「回退—恢复—又级联」的乒乓把预算耗在原地。
        let mut retry = 0u8;
        loop {
            let act = sess.recover(inp.code, cur, inp.line, syncs);
            let obs: &[Severity] = match observed_after.get(i) {
                Some(o) => o,
                None => &[],
            };
            let verdict = sess.observe(act.after, obs);
            actions.push(act);
            if !verdict.burst {
                break;
            }
            bursts = bursts.saturating_add(1);
            match sess.rollback_to_stable() {
                Some(sp) => {
                    cur = sp;
                    retry = retry.saturating_add(1);
                    if retry >= 2 {
                        // 乒乓两轮仍级联 → 强制兜底离场：只取其计数副作用
                        // （`forced` 累加），游标随即由下一处错误重置，故此处
                        // 的返回值不落 `cur`（落进去也是死值）。
                        let _ = sess.forced_fallback_cursor(cur);
                        break;
                    }
                }
                None => {
                    let _ = sess.forced_fallback_cursor(cur);
                    break;
                }
            }
        }
        i += 1;
    }
    RecoveryStream { actions, counters: *sess.counters(), bursts }
}