//! VE-F2804 · CSS 词法器与分词管线（VE-O 域 · CSS/HTML 表面域 · O01 组 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2804`
//!
//! **判据（锚点原文）**：O01 架构声明、集成边界、解析子集、判据。
//!
//! **职责定位（锚点原文）**：CSS 词法器与分词管线——CSS Syntax L3 分词——
//! token 全类型实现。
//!
//! **数据结构（锚点原文·家族格式）**：数据模型与规格表（逐条规格公开、参数域
//! 钳制、枚举守卫）。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：非法输入→校验拒绝三要素；
//! 边界越界→钳制 + 告警；异常检出→立案流转。
//!
//! **跨批对接点（锚点原文·家族格式）**：上游契约接收（哈希对账）；下游消费接口
//! （前向声明）；跨域衔接对账钩子（复用方义务）。
//!
//! **无障碍与隐私（锚点原文）**：文档替述可读；无隐私面。
//!
//! # 一、本单与 F2801/F2802/F2803 的分工（不重复施工）
//!
//! - F2801 已立七段管线与段契约。本单**只填 `Stage::Tokenize` 那一格**，其余六段
//!   一行不碰；[`derive_stage_budget`] 的预算从 `STAGE_BUDGET_TOTAL_MICROS` 派生，
//!   不另写一份数字（次序与预算单源是 F2801 的判据一）。
//! - F2802 已立 Servo 引入边界（style / style_traits 引入、layout 不引入）。本单
//!   **不引入任何 crate**，纯自持实现：引入第三方 tokenizer 反而会把 F2802 的
//!   符号白名单变成绕过子集的后门。
//! - F2803 已立四族属性子集表（44 条）与规范版本锚定。本单**只消费**它的锚定口径
//!   （[`SourceContract::verify`] 要求上游交来的源带自洽哈希），不重复取材，
//!   也不把「哪些属性合法」写进词法器——那是 F2803 与 F2805 的职责。
//!
//! # 二、为什么「单遍线性、无回溯」是本单的硬约束
//!
//! F2801 的段契约把分词复杂度写死为「C1 O(源长)」并注明「无回溯（回溯会放大到
//! O(N²)）」。这不是洁癖：分词在样式引擎里是**可能重跑**的一段，O(N²) 的最坏
//! 输入（大量失败转义 + 反复重试）在真实页面上就是一次可感知的卡顿。故本单：
//!
//! - 全部前瞻**一律 `peek(n)` 不消费**（n ≤ 3，规范允许的最大前瞻窗口）；
//! - 任何「失败后回头重扫」的写法禁止出现——失败一律**前进一格**并立案，
//!   这正是 CSS 规范自身的容错策略；
//! - 复杂度的可执行断言在 `veo04_lexer_checks` 的 `O04-性能-*` 四条里，用
//!   「两次跑计数完全一致 + 计数与解析式吻合」双向钉，不靠嘴报。
//!
//! # 三、错误恢复点必须带偏移，否则下游无法归因
//!
//! 锚点职责里明写「错误恢复点」。本单的恢复点不是「跳过就行」，而是**每一条
//! BadString / BadUrl / 未闭合注释都带 `[start, end)` 区间**并立案（`CaseLedger`，
//! 承 F2801）。没有偏移的恢复点等于把「哪一段样式被丢弃了」这个问题留给用户去猜
//! ——这是样式引擎最常见也最难解释的失败形态。
//!
//! # 四、零拷贝靠「偏移 + 流侧切片」，不靠token 自带引用
//!
//! F2801 的段契约要求 Token 携带「文本片段引用（零拷贝切片）」。这里刻意**不**把
//! `&str` 塞进 `Token`：token 流与它引用的源同属一个 `TokenStream`，若token 里存
//! 借用流内部 `String` 的引用，就构成自引用结构（既无法安全移动，也会逼出
//! `unsafe` 或`Arc` 一类的额外机制）。故：token 只存 `start` / `end` 两个 `u32`，
//! 文本由 [`TokenStream::slice_text`] / [`TokenStream::payload`] 在**调用时**从
//! 源里切出来——同样是零拷贝切片，且流可自由移动。只有发生过转义解码的 token 才在
//! `payload` 里留一份 owned 文本（语义必须物化，无法从原文反推）。
//!
//! # 五、零 panic 面
//!
//! 全模块用 `get` / `get_mut` 与显式边界检查：前视窗口越界返回 `None`、
//! 偏移转 `u32` 走 `try_from`（不用 `as u32`，那会静默回绕）、嵌套深度走
//! `saturating_*`。**无 `unwrap()`、无 `expect()`、无 `panic!`、无裸 `[i]` 索引**。
//! 零墙钟、零 IO，回归可复现。
//!
//! # 六、无障碍与隐私
//!
//! [`Token::screen_line`] / [`TokenStream::screen_text`] 让分词结果能被读屏逐
//! token 念出（含偏移与规范化值）；[`PRIVACY_SURFACE`] 显式声明**无隐私面**
//! ——词法器不采集、不回传任何用户数据，只有偏移与文本片段。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::veo01_arch::{
    fnv1a64_hex, BudgetEntry, BudgetLedger, CaseLedger, ClampLog, ClampNotice, Stage, StyleError,
    STAGE_BUDGET_TOTAL_MICROS, STAGE_COUNT,
};

/// 本项版本（词法器规格表版本号）。
pub const LEXER_VERSION: &str = "O01-lexer-v1";

/// 锚定的 CSS 语法规范（分词器的取材依据；换规范须走 [`SourceContract`] 重签）。
pub const ANCHORED_SYNTAX_SPEC: &str = "css-syntax-3/CSS Syntax Module Level 3";

// ---------------------------------------------------------------------------
// 一、诊断码（每类拒绝/越界/立案独立可检索，绝不合并成通用错误）
// ---------------------------------------------------------------------------

/// 错误码：参数域非法（存在零上限或字段倒挂）。
pub const E_LIMIT_INVALID: &str = "E_LIMIT_INVALID";

/// 错误码：上游契约名缺失。
pub const E_CONTRACT_NAME_MISSING: &str = "E_CONTRACT_NAME_MISSING";

/// 错误码：上游契约哈希失真（源与登记哈希对不上）。
pub const E_CONTRACT_HASH_DRIFT: &str = "E_CONTRACT_HASH_DRIFT";

/// 错误码：下游消费接口未前向声明。
pub const E_DOWNSTREAM_UNDECLARED: &str = "E_DOWNSTREAM_UNDECLARED";

/// 错误码：token 类型枚举越界（码不可解析）。
pub const E_TOKEN_KIND_INVALID: &str = "E_TOKEN_KIND_INVALID";

/// 错误码：token 偏移越界（`start > end` 或超出源长）。
pub const E_SPAN_INVALID: &str = "E_SPAN_INVALID";

/// 错误码：类型与载荷不匹配（该带的不带，或不该带的带了）。
pub const E_CARRIER_MISMATCH: &str = "E_CARRIER_MISMATCH";

/// 错误码：hash 记号的 type flag 缺失或错位。
pub const E_HASH_FLAG_MISSING: &str = "E_HASH_FLAG_MISSING";

/// 错误码：delim 记号缺字面量载荷或错位。
pub const E_DELIM_CARRIER: &str = "E_DELIM_CARRIER";

/// 错误码：token 流自检发现内部不一致（EOF 位置、计数聚合等）。
pub const E_STREAM_INCONSISTENT: &str = "E_STREAM_INCONSISTENT";

/// 修复建议（静态串——`StyleError::next` 收 `&'static str`，动态内容走 `why`）。
const FIX_LIMITS: &str =
    "把上限调整为正数，并使单token 字节上限不超过源字节上限、嵌套深度不为零";
const FIX_CONTRACT: &str = "重新对账：让上游用本段的流摘要覆盖声明值与实算值一起换契约";
const FIX_DOWNSTREAM: &str = "先在 DOWNSTREAM_SINKS 登记消费方与它的义务，再开始投递 token";
const FIX_KIND: &str = "用 TokenKind::ALL 与 TokenKind::code 推导取值，不要裸写数字";
const FIX_CARRIER: &str = "按 TokenKind::carries_* 判定携带何种载荷，不要凭记忆填";
const FIX_RECOVERY: &str = "在出错token 的偏移区间内修正该段声明；本段已被丢弃，其后声明不受影响";
const FIX_SELFCHECK: &str = "跑 run_veo04_checks() 按红项名称定位，逐项修复";

// ---------------------------------------------------------------------------
// 二、数据模型与规格表：token 类型全集（25 类，逐条规格公开）
// ---------------------------------------------------------------------------

/// token 类型全集长度（**全模块唯一真值**，容量表与守卫都从它派生）。
pub const TOKEN_KIND_COUNT: usize = 25;

/// CSS Syntax L3 §3.3 token 类型全集。
///
/// **逐条规格公开**：每一类在 [`TokenKind::spec`] 里写清「吃什么、吐什么、
/// 失败形态」——只给枚举不给规格的表，等于让下游各自猜语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    /// 标识符（`color` / `--main-color` / 已被转义解码的名字）。
    Ident,
    /// 函数记号（`rgb(`——记号**不含**左括号，载荷是函数名）。
    Function,
    /// at 关键字（`@media`）。
    AtKeyword,
    /// hash 记号（`#id` / `#fff`），带 type flag。
    Hash,
    /// 字符串（引号内的内容，**不含引号**）。
    String,
    /// 坏字符串（字符串内出现未转义换行）。
    BadString,
    /// URL（`url(foo.png)`，未加引号形态）。
    Url,
    /// 坏 URL（`url(` 内出现引号/左括号/不可打印字符/非法转义）。
    BadUrl,
    /// 分隔符（任何不成类别的单字符，如 `*` `/` `+`）。
    Delim,
    /// 数值。
    Number,
    /// 百分比（`50%`）。
    Percentage,
    /// 带单位的数值（`10px`；载荷是单位）。
    Dimension,
    /// 空白（**连续空白合并为一个 token**）。
    Whitespace,
    /// CDO（`<!--`）。
    Cdo,
    /// CDC（`-->`）。
    Cdc,
    /// 冒号。
    Colon,
    /// 分号。
    Semicolon,
    /// 逗号。
    Comma,
    /// 左方括号。
    LeftBracket,
    /// 右方括号。
    RightBracket,
    /// 左圆括号。
    LeftParen,
    /// 右圆括号。
    RightParen,
    /// 左花括号。
    LeftCurly,
    /// 右花括号。
    RightCurly,
    /// 输入流末尾（每个 token 流**恰好一个**，且必为末位）。
    Eof,
}

impl TokenKind {
    /// 25 类全集（顺序即 [`TokenKind::rank`]，唯一真值）。
    pub const ALL: [TokenKind; TOKEN_KIND_COUNT] = [
        TokenKind::Ident,
        TokenKind::Function,
        TokenKind::AtKeyword,
        TokenKind::Hash,
        TokenKind::String,
        TokenKind::BadString,
        TokenKind::Url,
        TokenKind::BadUrl,
        TokenKind::Delim,
        TokenKind::Number,
        TokenKind::Percentage,
        TokenKind::Dimension,
        TokenKind::Whitespace,
        TokenKind::Cdo,
        TokenKind::Cdc,
        TokenKind::Colon,
        TokenKind::Semicolon,
        TokenKind::Comma,
        TokenKind::LeftBracket,
        TokenKind::RightBracket,
        TokenKind::LeftParen,
        TokenKind::RightParen,
        TokenKind::LeftCurly,
        TokenKind::RightCurly,
        TokenKind::Eof,
    ];

    /// 段内序位（0 起）。容量计数表与遍历顺序都由它派生。
    pub fn rank(self) -> usize {
        match self {
            TokenKind::Ident => 0,
            TokenKind::Function => 1,
            TokenKind::AtKeyword => 2,
            TokenKind::Hash => 3,
            TokenKind::String => 4,
            TokenKind::BadString => 5,
            TokenKind::Url => 6,
            TokenKind::BadUrl => 7,
            TokenKind::Delim => 8,
            TokenKind::Number => 9,
            TokenKind::Percentage => 10,
            TokenKind::Dimension => 11,
            TokenKind::Whitespace => 12,
            TokenKind::Cdo => 13,
            TokenKind::Cdc => 14,
            TokenKind::Colon => 15,
            TokenKind::Semicolon => 16,
            TokenKind::Comma => 17,
            TokenKind::LeftBracket => 18,
            TokenKind::RightBracket => 19,
            TokenKind::LeftParen => 20,
            TokenKind::RightParen => 21,
            TokenKind::LeftCurly => 22,
            TokenKind::RightCurly => 23,
            TokenKind::Eof => 24,
        }
    }

    /// 中文名（读屏播报）。
    pub fn zh(self) -> &'static str {
        match self {
            TokenKind::Ident => "标识符",
            TokenKind::Function => "函数记号",
            TokenKind::AtKeyword => "at 关键字",
            TokenKind::Hash => "hash 记号",
            TokenKind::String => "字符串",
            TokenKind::BadString => "坏字符串",
            TokenKind::Url => "URL",
            TokenKind::BadUrl => "坏 URL",
            TokenKind::Delim => "分隔符",
            TokenKind::Number => "数值",
            TokenKind::Percentage => "百分比",
            TokenKind::Dimension => "带单位数值",
            TokenKind::Whitespace => "空白",
            TokenKind::Cdo => "CDO 开注释",
            TokenKind::Cdc => "CDC 闭注释",
            TokenKind::Colon => "冒号",
            TokenKind::Semicolon => "分号",
            TokenKind::Comma => "逗号",
            TokenKind::LeftBracket => "左方括号",
            TokenKind::RightBracket => "右方括号",
            TokenKind::LeftParen => "左圆括号",
            TokenKind::RightParen => "右圆括号",
            TokenKind::LeftCurly => "左花括号",
            TokenKind::RightCurly => "右花括号",
            TokenKind::Eof => "流末尾",
        }
    }

    /// 判据引用码（台账/差分对账用）。**由 rank 合成**，不另立第二张码表
    /// ——两张码表必然漂。
    pub fn code(self) -> String {
        format!("O04-T{:02}", self.rank())
    }

    /// 枚举守卫：码 → 类型。不可解析返回 `None`（调用方必须显性拒绝，不许猜）。
    pub fn from_code(code: &str) -> Option<TokenKind> {
        let b = code.as_bytes();
        if b.len() != 7 || &b[0..5] != b"O04-T" {
            return None;
        }
        // 序位是**十进制**两位——与 `code()` 的 `{:02}` 同源。用 `hex_val` 解析
        // 会把 `O04-T10` 读成 0x10=16，`O04-T19` 之后直接越界返回 None，
        // 枚举往返在第 10 类上就断了。
        let hi = dec_val(*b.get(5)?)?;
        let lo = dec_val(*b.get(6)?)?;
        let rank = (hi * 10 + lo) as usize;
        if rank >= TOKEN_KIND_COUNT {
            return None;
        }
        TokenKind::ALL.get(rank).copied()
    }

    /// 该类是否为**解析错误产物**（BadString / BadUrl）。
    pub fn is_error(self) -> bool {
        matches!(self, TokenKind::BadString | TokenKind::BadUrl)
    }

    /// 该类是否必带数值载荷。
    pub fn carries_number(self) -> bool {
        matches!(
            self,
            TokenKind::Number | TokenKind::Percentage | TokenKind::Dimension
        )
    }

    /// 该类是否必带文本载荷（Dimension 的文本载荷是单位）。
    pub fn carries_text(self) -> bool {
        matches!(
            self,
            TokenKind::Ident
                | TokenKind::Function
                | TokenKind::AtKeyword
                | TokenKind::Hash
                | TokenKind::String
                | TokenKind::BadString
                | TokenKind::Url
                | TokenKind::BadUrl
                | TokenKind::Dimension
        )
    }

    /// 该类是否必带 hash type flag。
    pub fn carries_hash_flag(self) -> bool {
        self == TokenKind::Hash
    }

    /// 该类是否必带 delim 字面量。
    pub fn carries_delim(self) -> bool {
        self == TokenKind::Delim
    }

    /// 该类的文本载荷是否**可从原文零拷贝切出**（转义解码过的除外）。
    pub fn text_is_raw_slice(self) -> bool {
        matches!(
            self,
            TokenKind::Ident
                | TokenKind::Function
                | TokenKind::AtKeyword
                | TokenKind::Hash
                | TokenKind::Dimension
        )
    }

    /// 该类是否开括号类（嵌套深度 +1）。
    ///
    /// **含 `Function`**：函数记号的区间已含左括号（§4.3.10），
    /// 括号不另成记号，故深度调整必须发生在这里；漏掉它会让
    /// `rgb(a[b(c)])` 这类嵌套块的深度恒为 0，错误恢复点失去嵌套上下文。
    pub fn opens_block(self) -> bool {
        matches!(
            self,
            TokenKind::LeftParen | TokenKind::LeftCurly | TokenKind::Function
        )
    }

    /// 该类是否闭括号类（嵌套深度 -1）。
    pub fn closes_block(self) -> bool {
        matches!(self, TokenKind::RightParen | TokenKind::RightCurly)
    }

    /// 逐条规格（吃什么 / 吐什么 / 失败形态）。
    pub fn spec(self) -> &'static str {
        match self {
            TokenKind::Ident => "吃标识符序列（含 -- 自定义属性名与转义解码）；吐规范化名；失败：非法转义被替换并立案",
            TokenKind::Function => "吃标识符序列 + 紧邻左括号；吐函数名（不含括号）；失败：括号前有空白则退回 Ident",
            TokenKind::AtKeyword => "吃 @ 加标识符序列；吐 @ 后的名字；失败：@ 后非标识符起始则退回 Delim('@')",
            TokenKind::Hash => "吃 # 加标识符起始或数字；吐 # 后的名字与 type flag；失败：# 后无名字则退回 Delim('#')",
            TokenKind::String => "吃引号加转义串；吐解码后内容（不含引号）；失败：遇未转义换行吐 BadString，遇 EOF 立案",
            TokenKind::BadString => "吃未闭合字符串；吐已收部分与偏移区间；失败：本身即错误产物，下游须跳过整条声明",
            TokenKind::Url => "吃 url( 加非引号内容加右括号；吐 URL 文本；失败：url( 后紧跟引号则退回 Function 记号",
            TokenKind::BadUrl => "吃 url( 内非法内容；吐已收部分与偏移区间；失败：本身即错误产物",
            TokenKind::Delim => "吃任何不成类别的单字符；吐该字符；失败：无",
            TokenKind::Number => "吃可选符号加数字加小数加指数；吐数值与 integer/real 标记；失败：非数字起始则退回 Delim",
            TokenKind::Percentage => "吃数值加 %；吐数值（百分号不入载荷）；失败：无数值则退回 Delim('%')",
            TokenKind::Dimension => "吃数值加单位标识符；吐数值与单位；失败：单位非法则降级为 Number 并告警",
            TokenKind::Whitespace => "吃一串连续空白；吐合并后的单个记号；失败：无",
            TokenKind::Cdo => "吃 <!--；吐 CDO；失败：无",
            TokenKind::Cdc => "吃 -->；吐 CDC；失败：无",
            TokenKind::Colon => "吃冒号；吐冒号；失败：无",
            TokenKind::Semicolon => "吃分号；吐分号；失败：无",
            TokenKind::Comma => "吃逗号；吐逗号；失败：无",
            TokenKind::LeftBracket => "吃左方括号；吐左方括号；失败：嵌套超上限则钳制并立案",
            TokenKind::RightBracket => "吃右方括号；吐右方括号；失败：无",
            TokenKind::LeftParen => "吃左圆括号；吐左圆括号；失败：嵌套超上限则钳制并立案",
            TokenKind::RightParen => "吃右圆括号；吐右圆括号；失败：深度下溢按零收并立案",
            TokenKind::LeftCurly => "吃左花括号；吐左花括号；失败：嵌套超上限则钳制并立案",
            TokenKind::RightCurly => "吃右花括号；吐右花括号；失败：深度下溢按零收并立案",
            TokenKind::Eof => "吃输入流末尾；吐唯一 EOF 记号；失败：无",
        }
    }

    /// 读屏单行（逐条规格可被读屏念出）。
    pub fn screen_line(self) -> String {
        format!("{}（{}）：{}", self.zh(), self.code(), self.spec())
    }
}

/// hex 数字 → 值（0..=15）。非 hex 返回 `None`。
fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// 十进制数字 → 值（0..=9）。非十进制返回 `None`。
fn dec_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        _ => None,
    }
}

/// hash 记号的 type flag（CSS Syntax L3 §4.3.2）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashType {
    /// 未适用（非 hash 记号）。
    Unset,
    /// `id` 型（`#` 后是标识符起始）。
    Id,
    /// `unrestricted` 型（`#` 后是数字）。
    Unrestricted,
}

impl HashType {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            HashType::Unset => "未适用",
            HashType::Id => "id 型",
            HashType::Unrestricted => "无限制型",
        }
    }
}

/// 数值载荷（CSS Syntax L3 §5.2 数值表示）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberValue {
    /// 数值（f64；符号已计入）。
    pub value: f64,
    /// 是否为**整数型**（无小数部分且无指数部分——按记法判定，不按数值大小）。
    pub is_integer: bool,
    /// 是否为**实型**记法（带小数或带指数）。
    pub is_real: bool,
    /// 是否显式带符号（`+1` 与 `1` 数值相同但记法不同）。
    pub signed: bool,
}

impl NumberValue {
    /// 由「有无小数部分 / 有无指数部分 / 有无符号」构造（**规范口径**：
    /// `1e3` 是 real 不是 integer，`+1` 与 `1` 数值相同但 `signed` 不同）。
    pub fn build(value: f64, has_fraction: bool, has_exponent: bool, signed: bool) -> Self {
        NumberValue {
            value,
            is_integer: !has_fraction && !has_exponent,
            is_real: has_fraction || has_exponent,
            signed,
        }
    }

    /// 读屏单行（数值要念得出「是不是整数」）。
    pub fn screen_line(&self) -> String {
        format!(
            "数值 {}{}（{}型）",
            if self.signed { "带符号" } else { "无符号" },
            self.value,
            if self.is_integer { "整数" } else { "实数" }
        )
    }
}

/// token 的文本载荷：**仅在无法从原文零拷贝切出时才有内容**。
///
/// - [`Payload::Raw`]：载荷就是整个原文区间（如分隔符），零拷贝。
/// - [`Payload::RawShifted(u32)`]：载荷是原文区间的**子段**，相对 `start` 的
///   偏移为该值——字符串要去掉引号、hash 要去掉 `#`、at 关键字要去掉 `@`、
///   函数记号要切到 `(` 之前。这些偏移**由类型逐条派生**，不猜、不回扫。
/// - [`Payload::Decoded`]：发生过转义解码，语义必须物化（无法从原文反推）。
#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    /// 可零拷贝（载荷即整个原文区间，字段为空）。
    Raw,
    /// 可零拷贝（载荷为子段，相对 token 起始的偏移）。
    RawShifted(u32),
    /// 已物化载荷（转义解码后的名字、字符串内容、URL 内容、单位名）。
    Decoded(String),
}

impl Payload {
    /// 是否零拷贝（未物化）。
    pub fn is_raw(&self) -> bool {
        !matches!(self, Payload::Decoded(_))
    }

    /// owned 载荷内容（零拷贝分支返回空串）。
    pub fn as_str(&self) -> &str {
        match self {
            Payload::Decoded(s) => s.as_str(),
            _ => "",
        }
    }

    /// owned 载荷字节数（零拷贝为 0——诊断「解码成本」用）。
    pub fn decoded_len(&self) -> usize {
        match self {
            Payload::Decoded(s) => s.len(),
            _ => 0,
        }
    }
}

/// 一个 token（**只存偏移，不存借用引用**——理由见文件头 §4）。
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// 类型。
    pub kind: TokenKind,
    /// 起始偏移（预处理后源，单位字节，含端点）。
    pub start: u32,
    /// 结束偏移（不含端点；已按单 token 字节上限钳制）。
    pub end: u32,
    /// 载荷（`Raw` 表示可零拷贝）。
    pub payload: Payload,
    /// 数值载荷。
    pub numeric: Option<NumberValue>,
    /// hash type flag。
    pub hash_type: HashType,
    /// delim 字面量。
    pub delim: Option<char>,
    /// 括号/花括号嵌套深度（**深度记在该记号闭合之后的状态**——即「读完它时
    /// 所处的嵌套层」，恢复点用它定位「这段声明嵌在哪一层」）。
    pub depth: u16,
}

impl Token {
    /// token 原始字节长度。
    pub fn byte_len(&self) -> usize {
        self.end.saturating_sub(self.start) as usize
    }

    /// 该 token 是否为错误产物（BadString / BadUrl）。
    pub fn is_error(&self) -> bool {
        self.kind.is_error()
    }

    /// 偏移区间是否自洽（`start ≤ end` 且不超过源长）。
    pub fn span_is_sane(&self, source_len: usize) -> bool {
        self.start <= self.end && (self.end as usize) <= source_len
    }

    /// 载荷与类型是否自洽（**正向：类型 → 必带载荷**；反向由 [`TokenStream::audit`]
    /// 查——两者方向都不能少）。
    pub fn carrier_is_sane(&self) -> bool {
        if self.kind.carries_number() && self.numeric.is_none() {
            return false;
        }
        if self.kind.carries_hash_flag() && self.hash_type == HashType::Unset {
            return false;
        }
        if self.kind.carries_delim() && self.delim.is_none() {
            return false;
        }
        true
    }

    /// 读屏单行（偏移 + 类型 + 载荷，念得出「哪一段被丢了」）。
    pub fn screen_line(&self) -> String {
        let mut s = format!(
            "{}（{}），偏移 [{}, {})，{} 字节，嵌套深度 {}",
            self.kind.zh(),
            self.kind.code(),
            self.start,
            self.end,
            self.byte_len(),
            self.depth
        );
        if let Some(n) = self.numeric {
            s.push_str(&format!("，{}", n.screen_line()));
        }
        if let Some(d) = self.delim {
            s.push_str(&format!("，字面量'{}'", d));
        }
        if self.kind.carries_hash_flag() {
            s.push_str(&format!("，type flag {}", self.hash_type.zh()));
        }
        let p = self.payload.as_str();
        if !p.is_empty() {
            s.push_str(&format!("，载荷「{}」", p));
        } else if self.kind.carries_text() {
            s.push_str("，载荷由原文零拷贝切出");
        }
        if self.kind.is_error() {
            s.push_str("（错误产物：该条声明将被丢弃）");
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 三、参数域钳制：分词段上限表（逐条公开 + 守卫）
// ---------------------------------------------------------------------------

/// 源文本上限（承 F2801 `MAX_SOURCE_BYTES` 同值——**引用而不另定数**）。
pub const DEFAULT_MAX_SOURCE_BYTES: usize = super::veo01_arch::MAX_SOURCE_BYTES;

/// token 数上限。
pub const DEFAULT_MAX_TOKENS: usize = 1 << 20;

/// 单 token 字节上限（防止一个超长选择器或 data URI 撑爆内存）。
pub const DEFAULT_MAX_TOKEN_BYTES: usize = 64 * 1024;

/// 嵌套深度上限（括号 + 花括号合计）。
pub const DEFAULT_MAX_NESTING_DEPTH: u16 = 64;

/// 分词段上限表（**参数域**；构造时逐条守卫）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LexLimits {
    /// 源字节上限。
    pub max_source_bytes: usize,
    /// token 数上限。
    pub max_tokens: usize,
    /// 单 token 字节上限。
    pub max_token_bytes: usize,
    /// 嵌套深度上限。
    pub max_nesting_depth: u16,
}

impl LexLimits {
    /// 标准上限（域默认值）。
    pub fn standard() -> Self {
        LexLimits {
            max_source_bytes: DEFAULT_MAX_SOURCE_BYTES,
            max_tokens: DEFAULT_MAX_TOKENS,
            max_token_bytes: DEFAULT_MAX_TOKEN_BYTES,
            max_nesting_depth: DEFAULT_MAX_NESTING_DEPTH,
        }
    }

    /// 参数域守卫（**守卫而非静默钳制**：非法上限直接拒绝，让调用方看见）。
    ///
    /// 三条闸：
    /// 1. 四项上限都必须为正（0 上限 = 立刻触发，等于把分词段关掉）；
    /// 2. `max_token_bytes ≤ max_source_bytes`（单 token 不可能比整个源还长，
    ///    这条倒挂说明有人把两个字段填反了）；
    /// 3. 嵌套深度不为 0（深度 0 会让每个括号都立刻触发钳制）。
    pub fn verify(&self) -> Result<(), StyleError> {
        if self.max_source_bytes == 0
            || self.max_tokens == 0
            || self.max_token_bytes == 0
            || self.max_nesting_depth == 0
        {
            return Err(StyleError::new(
                E_LIMIT_INVALID,
                "分词上限表被拒：存在零上限",
                &format!(
                    "源 {} / token 数 {} / 单 token 字节 {} / 嵌套深度 {}",
                    self.max_source_bytes,
                    self.max_tokens,
                    self.max_token_bytes,
                    self.max_nesting_depth
                ),
                FIX_LIMITS,
                "分词段维护方",
            ));
        }
        if self.max_token_bytes > self.max_source_bytes {
            return Err(StyleError::new(
                E_LIMIT_INVALID,
                "分词上限表被拒：单 token 上限高于源上限",
                &format!(
                    "单 token {} 字节 > 源{} 字节；两个字段填反了",
                    self.max_token_bytes, self.max_source_bytes
                ),
                FIX_LIMITS,
                "分词段维护方",
            ));
        }
        Ok(())
    }

    /// 读屏单行（上限表要能被念出来查）。
    pub fn screen_line(&self) -> String {
        format!(
            "分词上限：源 {} 字节、token 数 {}、单 token {} 字节、嵌套深度 {}",
            self.max_source_bytes, self.max_tokens, self.max_token_bytes, self.max_nesting_depth
        )
    }
}

// ---------------------------------------------------------------------------
// 四、性能逐项分解：预算联动次序单源（预算从 F2801 派生，不另写数字）
// ---------------------------------------------------------------------------

/// 从 `STAGE_BUDGET_TOTAL_MICROS` 派生某段预算（**七段之和恒等于总预算**）。
///
/// 余数全部落在**末段**，避免「每段四舍五入」——那种写法七段加起来会对不上总预算，
/// 而对不上的那一刻就没人知道该信谁。
pub fn derive_stage_budget(rank: u8) -> u32 {
    let base = STAGE_BUDGET_TOTAL_MICROS / STAGE_COUNT as u32;
    let rest = STAGE_BUDGET_TOTAL_MICROS % STAGE_COUNT as u32;
    base + if rank as usize + 1 == STAGE_COUNT {
        rest
    } else {
        0
    }
}

/// 分词段预算（由 `Stage::Tokenize` 的段位派生，**不另写数字**）。
pub fn tokenize_budget_micros() -> u32 {
    derive_stage_budget(Stage::Tokenize.rank())
}

/// 把本段预算登记进 F2801 的 [`BudgetLedger`]（**次序单源**：必须按段序递增注册，
/// 跳序由 F2801 的次序门拦）。
///
/// 登记的是**逻辑预算**，实测中位留 0——如实标注「待实测」，不填乐观值。
pub fn register_tokenize_budget(ledger: &mut BudgetLedger) -> Result<u8, StyleError> {
    ledger.register(BudgetEntry {
        stage: Stage::Tokenize,
        micros: tokenize_budget_micros(),
        measured_median_micros: 0,
        measured_p99_micros: 0,
    })
}

/// 各项词法操作的复杂度声明（**逐项分解**，判据一附属）。
pub const COMPLEXITY_TABLE: [(&str, &str); 8] = [
    ("预处理（换行归一 + NUL 替换）", "C1 O(N) 单遍"),
    ("取下一 token（主循环）", "C1 O(token 长度)，全流 O(N)"),
    ("前瞻 peek(0..3)", "C1 O(1)（前瞻窗口上限 3，规范不允许更大）"),
    ("标识符 / 数字 / 字符串消费", "C1 O(各自长度)，单遍无回溯"),
    ("URL 与坏 URL 残余消费", "C1 O(到配对右括号为止)，单遍"),
    ("hash 记号判定", "C1 O(1)（判定只看第一个码点）"),
    ("token 统计聚合", "C1 O(token 数)"),
    ("流自检 audit", "C1 O(token 数)，不二次扫描嵌套结构"),
];

/// 复杂度声明的读屏单行。
pub fn complexity_screen_text() -> String {
    let mut s = String::from("分词段复杂度分解：");
    for (op, cx) in COMPLEXITY_TABLE.iter() {
        s.push_str(&format!("{} 为 {}；", op, cx));
    }
    s
}

/// 前瞻窗口上限（**不变量**：全模块任何 `peek(n)` 的 `n` 不得超过此值）。
///
/// 这条常量被自检当作**可机检的不变量**用：超窗即说明有人在主循环里偷看了
/// 规范不允许的位置，那通常意味着「偷偷加了回溯」。
pub const MAX_PEEK_WINDOW: usize = 3;

// ---------------------------------------------------------------------------
// 五、跨批对接点：上游契约接收（哈希对账）/ 下游消费接口（前向声明）
// ---------------------------------------------------------------------------

/// 上游契约（源文本的版本化契约名 + 实算哈希）。
///
/// **哈希对账的输入取「预处理后的源」**：下游消费的是预处理后的 token，
/// 拿原文哈希对账等于对了两份不同的东西（CRLF 与 LF 会被判成漂移）。
#[derive(Clone, Debug)]
pub struct SourceContract {
    /// 契约名（版本化，如 `stylesheet/v1`）。
    pub name: String,
    /// 预处理后源的 FNV-1a64 十六进制哈希。
    pub content_hash: String,
}

impl SourceContract {
    /// 构造（契约名非空由 [`SourceContract::verify`] 兜住，不靠调用方自觉）。
    pub fn new(name: &str, content_hash: &str) -> Self {
        SourceContract {
            name: name.to_string(),
            content_hash: content_hash.to_string(),
        }
    }

    /// 哈希对账（**重算，不是看填了没有**）。
    pub fn verify(&self, preprocessed: &str) -> Result<(), StyleError> {
        if self.name.trim().is_empty() {
            return Err(StyleError::new(
                E_CONTRACT_NAME_MISSING,
                "上游契约对账被拒：契约名为空",
                "无名契约无法在台账里定位，也无法追责到提供方",
                "填入版本化契约名（如 stylesheet/v1）",
                "上游提供方",
            ));
        }
        let actual = fnv1a64_hex(preprocessed.as_bytes());
        if actual != self.content_hash {
            return Err(StyleError::new(
                E_CONTRACT_HASH_DRIFT,
                "上游契约哈希失真",
                &format!(
                    "契约 {} 登记哈希 {} 与实算 {} 不一致；源在传输中被改过或被截断",
                    self.name, self.content_hash, actual
                ),
                FIX_CONTRACT,
                "上游提供方",
            ));
        }
        Ok(())
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!("上游契约 {}，哈希 {}", self.name, self.content_hash)
    }
}

/// 下游消费方义务（**前向声明**：先声明义务，再开始投递）。
#[derive(Clone, Copy, Debug)]
pub struct DownstreamSink {
    /// 消费方标识（对应承接单）。
    pub id: &'static str,
    /// 中文名。
    pub zh: &'static str,
    /// 承接单号。
    pub anchor: &'static str,
    /// 该消费方对 token 流的**义务**（必填——无义务的声明不是契约）。
    pub obligation: &'static str,
}

/// 下游消费方全集（F2805 / F2806 / F2814 / F2819 四家）。
///
/// **为什么是这四家**：F2805 声明解析与属性表注册、F2806 简写展开消费 token 流；
/// F2814 错误恢复消费错误产物与恢复点；F2819 fuzz 以 token 流摘要为变异基准。
/// 其余各家（F2807 及以后）拿到的已是解析后的对象，不在本段的直接消费方之列。
pub const DOWNSTREAM_SINKS: [DownstreamSink; 4] = [
    DownstreamSink {
        id: "declaration-parse",
        zh: "声明解析与属性表注册",
        anchor: "VE-F2805",
        obligation: "按 token 顺序消费，不得回读源；遇 BadString/BadUrl 必须跳过整条声明而非整张表",
    },
    DownstreamSink {
        id: "shorthand-expand",
        zh: "简写属性展开",
        anchor: "VE-F2806",
        obligation: "只消费 Dimension/Percentage/Number 的数值载荷，不得自行再解析字符串",
    },
    DownstreamSink {
        id: "error-recovery",
        zh: "样式错误恢复与容错",
        anchor: "VE-F2814",
        obligation: "以 token 的偏移区间与 depth 作为恢复点与嵌套上下文",
    },
    DownstreamSink {
        id: "style-fuzz",
        zh: "样式 fuzz 与健壮性",
        anchor: "VE-F2819",
        obligation: "以本段 token 流的 digest() 为变异前后对照基准",
    },
];

/// 跨域衔接对账钩子（**复用方义务**：复用本段必须履行对账）。
///
/// 返回「必须履行的义务条目」；空清单 = 无对账义务。
pub fn reconciliation_hooks(reuser: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    for s in DOWNSTREAM_SINKS.iter() {
        if s.id == reuser {
            out.push(s.obligation);
        }
    }
    out
}

/// 下游投递前的守卫：**未前向声明的消费方不得拿到 token**。
pub fn check_sink_declared(reuser: &str) -> Result<&'static DownstreamSink, StyleError> {
    for s in DOWNSTREAM_SINKS.iter() {
        if s.id == reuser {
            return Ok(s);
        }
    }
    Err(StyleError::new(
        E_DOWNSTREAM_UNDECLARED,
        "下游投递被拒：消费方未前向声明",
        &format!("「{}」不在 DOWNSTREAM_SINKS 四家之列", reuser),
        FIX_DOWNSTREAM,
        "分词段维护方",
    ))
}

// ---------------------------------------------------------------------------
// 六、统计（性能与故障归因共用）
// ---------------------------------------------------------------------------

/// 分词统计（**按类型计数，容量 [`TOKEN_KIND_COUNT`] 来自唯一真值**）。
#[derive(Clone, Debug)]
pub struct TokenStats {
    /// 源字节数（截断前）。
    pub bytes_in: usize,
    /// 预处理后字节数。
    pub bytes_pre: usize,
    /// token 总数（含末尾唯一 EOF）。
    pub tokens: u32,
    /// 逐类计数（索引即 [`TokenKind::rank`]）。
    pub by_kind: [u32; TOKEN_KIND_COUNT],
    /// 注释段数（注释不产 token，但计入统计以便核对「扫描是否漏段」）。
    pub comments: u32,
    /// 解码出的转义字符数。
    pub escapes_decoded: u32,
    /// 非 ASCII 码点数（CSS 标识符可含非 ASCII，必须计数以证「没有被吞掉」）。
    pub non_ascii: u32,
    /// 观察到的最大嵌套深度。
    pub max_depth: u16,
    /// 截断次数（源 / token 数 / 单 token 字节）。
    pub truncations: u32,
    /// 是否因上限而提前收尾（**显性标注**：被截断的流不是完整流）。
    pub truncated: bool,
}

impl TokenStats {
    /// 空统计。
    pub fn new() -> Self {
        TokenStats {
            bytes_in: 0,
            bytes_pre: 0,
            tokens: 0,
            by_kind: [0; TOKEN_KIND_COUNT],
            comments: 0,
            escapes_decoded: 0,
            non_ascii: 0,
            max_depth: 0,
            truncations: 0,
            truncated: false,
        }
    }

    /// 记一类 token。
    pub fn count(&mut self, k: TokenKind) {
        self.tokens = self.tokens.saturating_add(1);
        if let Some(c) = self.by_kind.get_mut(k.rank()) {
            *c = c.saturating_add(1);
        }
    }

    /// 取某类计数。
    pub fn kind_count(&self, k: TokenKind) -> u32 {
        self.by_kind.get(k.rank()).copied().unwrap_or(0)
    }

    /// 逐类计数之和（**必须等于 `tokens`**——「计数聚合无遗漏」判据用）。
    pub fn kind_sum(&self) -> u64 {
        let mut s: u64 = 0;
        for c in self.by_kind.iter() {
            s = s.saturating_add(*c as u64);
        }
        s
    }

    /// 非平凡 token 数（去掉空白与 EOF——**真实语法体量**）。
    pub fn significant(&self) -> u32 {
        self.tokens
            .saturating_sub(self.kind_count(TokenKind::Whitespace))
            .saturating_sub(self.kind_count(TokenKind::Eof))
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        let mut s = format!(
            "源 {} 字节（预处理后 {} 字节），token {} 个（其中有效 {} 个），注释 {} 段，转义解码 {} 处，非 ASCII {} 个，最大嵌套深度 {}",
            self.bytes_in,
            self.bytes_pre,
            self.tokens,
            self.significant(),
            self.comments,
            self.escapes_decoded,
            self.non_ascii,
            self.max_depth
        );
        if self.truncated {
            s.push_str("；本流被上限截断，不是完整流");
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 七、隐私面声明（锚点原文「无隐私面」）
// ---------------------------------------------------------------------------

/// 隐私面枚举（本单只有「无」这一项——写成枚举而不是一句注释，是为了让
/// 「将来若引入隐私面」必须改类型，而不是悄悄加字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivacySurface {
    /// 无隐私面：只处理样式文本与其偏移，不采集、不回传任何用户数据。
    None,
}

/// 本单隐私面声明。
pub const PRIVACY_SURFACE: PrivacySurface = PrivacySurface::None;

impl PrivacySurface {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            PrivacySurface::None => "无隐私面（仅处理样式文本与偏移）",
        }
    }
}

// ---------------------------------------------------------------------------
// 八、分词器本体（单遍线性、无回溯）
// ---------------------------------------------------------------------------

/// 分词器。
///
/// **零 panic 面**：前视越界返回 `None`、偏移转换走 `try_from`、深度走
/// `saturating_*`。方法内**不出现任何 `unwrap`/`expect`/`panic!`/裸 `[i]`**
/// ——分词器吃的是不可信输入（用户样式表），这条是纪律不是风格。
pub struct Lexer {
    /// 预处理后的源（`\r\n`/`\r`/`\f` → `\n`，NUL → U+FFFD）。
    pre: String,
    /// 上游契约（可选；给了就强制哈希对账）。
    contract: Option<SourceContract>,
    /// 上限表。
    limits: LexLimits,
    /// 逻辑 tick。
    tick: u64,
    /// 游标（预处理后源的字节偏移）。
    pos: usize,
    /// 嵌套深度。
    depth: u16,
    /// 已产出 token 数。
    emitted: u32,
    /// 告警账（承 F2801：钳制必带告警）。
    clamps: ClampLog,
    /// 案件账（承 F2801：异常检出必立案）。
    cases: CaseLedger,
    /// 统计。
    stats: TokenStats,
    /// 转义解码缓冲（复用，避免每个 token 一次分配）。
    buf: String,
    /// URL 内容切片起点（`url(` 之后跳过空白的位置；仅 URL 分支有效）。
    url_content_start: usize,
}

impl Lexer {
    /// 构造（**不校验上限**：校验在 [`Lexer::run`] 起始处统一做，避免「构造
    /// 成功但一跑就炸」的半可用状态）。
    pub fn new(contract: Option<SourceContract>, limits: LexLimits, tick: u64) -> Self {
        Lexer {
            pre: String::new(),
            contract,
            limits,
            tick,
            pos: 0,
            depth: 0,
            emitted: 0,
            clamps: ClampLog::new(),
            cases: CaseLedger::new(),
            stats: TokenStats::new(),
            buf: String::new(),
            url_content_start: 0,
        }
    }

    /// 跑完整个流（**O(N) 单遍**）。
    ///
    /// 三闸次序固定：
    /// ① 上限表守卫（参数域非法直接拒）；
    /// ② 上游契约对账（哈希失真直接拒，不产出半个流让人误用）；
    /// ③ 源长度钳制 + 截断告警 + 立案（**越界不是拒绝而是钳制**——样式表超长是
    ///    常态不是攻击，拒掉等于整页无样式）。
    pub fn run(&mut self, source: &str) -> Result<TokenStream, StyleError> {
        self.limits.verify()?;
        self.stats.bytes_in = source.len();

        // 闸二：上游契约哈希对账（对**预处理后**的源）。
        let pre = preprocess(source);
        self.pre = pre;
        if let Some(c) = self.contract.clone() {
            c.verify(self.pre.as_str())?;
        }

        // 闸三：源长度越界 → 钳制（截断）+ 告警 + 立案，不静默吞尾巴。
        if self.pre.len() > self.limits.max_source_bytes {
            let cut = floor_char_boundary(self.pre.as_str(), self.limits.max_source_bytes);
            let dropped = self.pre.len() - cut;
            self.pre.truncate(cut);
            self.note_truncation("source.bytes", dropped as f64, cut as f64, cut as f64);
            let _ = self.cases.open_case(
                "样式源超上限被截断",
                &format!("尾部 {} 字节未被分词，依赖其后的声明不生效", dropped),
                "Lexer::run 源长度闸",
                "拆表或抬上限",
                self.tick,
            );
        }
        self.stats.bytes_pre = self.pre.len();

        let mut tokens: Vec<Token> = Vec::new();
        loop {
            if self.emitted as usize >= self.limits.max_tokens {
                // token 数越界 → 截断 + 告警 + 立案。
                let cap = self.limits.max_tokens as f64;
                self.note_truncation("tokens.count", tokens.len() as f64, cap, cap);
                let _ = self.cases.open_case(
                    "token 数超上限被截断",
                    "本流的尾部未被分词",
                    "Lexer::run token 数闸",
                    "拆表或抬上限",
                    self.tick,
                );
                break;
            }
            if self.next_token(&mut tokens) {
                break;
            }
        }
        // 因上限提前 break 时补一个显式 EOF（不留「流无结尾」的悬空状态），
        // 截断标记已在 note_truncation 里置位。
        if !matches!(tokens.last().map(|t| t.kind), Some(TokenKind::Eof)) {
            let t = self.make_token(TokenKind::Eof, self.pos, self.pos, Payload::Raw, None, None);
            self.stats.count(TokenKind::Eof);
            tokens.push(t);
            self.emitted = self.emitted.saturating_add(1);
        }
        self.stats.truncated = self.stats.truncations > 0;

        Ok(TokenStream {
            pre: self.pre.clone(),
            tokens,
            stats: self.stats.clone(),
            clamps: self.clamps.clone(),
            cases: self.cases.clone(),
            truncated: self.stats.truncated,
        })
    }

    /// 记一次截断（钳制告警 + 统计置位，两件事必须成对出现）。
    fn note_truncation(&mut self, field: &str, original: f64, clamped: f64, high: f64) {
        self.stats.truncations = self.stats.truncations.saturating_add(1);
        self.stats.truncated = true;
        let _ = self.clamps.push(ClampNotice {
            field: field.to_string(),
            original,
            clamped,
            low: 0.0,
            high,
            tick: self.tick,
        });
    }

    // -----------------------------------------------------------------------
    // 8.1 码点分类（§3.2）
    // -----------------------------------------------------------------------

    /// 数字。
    fn is_digit(c: char) -> bool {
        c.is_ascii_digit()
    }

    /// 标识符起始。
    fn is_ident_start(c: char) -> bool {
        c.is_ascii_alphabetic() || c == '_' || (c as u32) >= 0x80
    }

    /// 名称码点（标识符续）。
    fn is_name(c: char) -> bool {
        Self::is_ident_start(c) || Self::is_digit(c) || c == '-'
    }

    /// 空白。
    fn is_whitespace(c: char) -> bool {
        c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\u{0C}'
    }

    /// 非打印码点（§3.2）。
    fn is_non_printable(c: char) -> bool {
        let v = c as u32;
        v <= 0x08 || v == 0x0B || (0x0E..=0x1F).contains(&v) || v == 0x7F
    }

    /// 有效转义（§4.3.7）：`\` 后接非换行且非 EOF。
    fn is_valid_escape(&self) -> bool {
        if self.peek(0) != Some('\\') {
            return false;
        }
        match self.peek(1) {
            None => false,
            Some(c) => !Self::is_whitespace(c),
        }
    }

    /// 起始数字（§4.3.9 would start a number）。
    fn would_start_number(&self) -> bool {
        match self.peek(0) {
            Some(c) if Self::is_digit(c) => true,
            Some('+') | Some('-') => match self.peek(1) {
                Some(d) if Self::is_digit(d) => true,
                Some('.') => matches!(self.peek(2), Some(d) if Self::is_digit(d)),
                _ => false,
            },
            Some('.') => matches!(self.peek(1), Some(d) if Self::is_digit(d)),
            _ => false,
        }
    }

    /// 起始标识符序列（§4.3.11 would start an ident sequence）。
    ///
    /// 三种前缀，缺一不可：
    /// - `-?` 后接标识符起始（`color` / `-moz-x`）；
    /// - `--` 后接标识符起始或 `-`（**CSS 自定义属性名**，`--main-color`）。
    ///   漏掉这一支，`--custom` 会被切成 `Delim('-')` + `Ident("-custom")`，
    ///   下游按名字取变量时永远取不到；
    /// - 以有效转义起始（`\31 23` 这类）。
    fn would_start_ident_seq(&self) -> bool {
        if self.peek(0) == Some('-') {
            if self.peek(1) == Some('-') {
                return matches!(self.peek(2), Some(c) if Self::is_ident_start(c) || c == '-');
            }
            return matches!(self.peek(1), Some(c) if Self::is_ident_start(c));
        }
        matches!(self.peek(0), Some(c) if Self::is_ident_start(c) || c == '\\')
    }

    // -----------------------------------------------------------------------
    // 8.2 游标与前视（**一律不消费**）
    // -----------------------------------------------------------------------

    /// 前视第 `n` 个码点（0 起）；越界返回 `None`。
    fn peek(&self, n: usize) -> Option<char> {
        self.pre.get(self.pos..)?.chars().nth(n)
    }

    /// 消费一个码点；EOF 返回 `None`。
    fn bump(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.pos += c.len_utf8();
        if !c.is_ascii() {
            self.stats.non_ascii = self.stats.non_ascii.saturating_add(1);
        }
        Some(c)
    }

    /// 当前是否已到流末尾。
    fn at_eof(&self) -> bool {
        self.pos >= self.pre.len()
    }

    // -----------------------------------------------------------------------
    // 8.3 主分词循环（§4.3 取下一 token）
    // -----------------------------------------------------------------------

    /// 取下一 token 并推入 `out`；返回 `true` 表示流已结束。
    fn next_token(&mut self, out: &mut Vec<Token>) -> bool {
        // 注释不是 token，先吃掉（§4.3.2）。
        while self.peek(0) == Some('/') && self.peek(1) == Some('*') {
            self.consume_comment();
        }
        let Some(c) = self.peek(0) else {
            let t = self.make_token(TokenKind::Eof, self.pos, self.pos, Payload::Raw, None, None);
            self.stats.count(TokenKind::Eof);
            out.push(t);
            self.emitted = self.emitted.saturating_add(1);
            return true;
        };
        let start = self.pos;

        // 空白（§4.3.1）：**一串连续空白合并为单个记号**。此分支必须在分派
        // 最前面——规范把「消费空白」定义为独立于任何记号类别的第一类动作。
        // 少了它，空白会掉到末尾的 `single_char_kind` / Delim 兜底里被逐个
        // 吐成 Delim(' ')，于是 `a   b` 变成 5 个记号而不是 3 个，
        // 下游「声明以空白分隔」的判据全部失真。
        if Self::is_whitespace(c) {
            while self.peek(0).map(Self::is_whitespace).unwrap_or(false) {
                self.bump();
            }
            self.push_plain(out, TokenKind::Whitespace, start);
            return false;
        }

        // CDO：<!--
        if c == '<'
            && self.peek(1) == Some('!')
            && self.peek(2) == Some('-')
            && self.peek(3) == Some('-')
        {
            for _ in 0..4 {
                self.bump();
            }
            self.push_plain(out, TokenKind::Cdo, start);
            return false;
        }
        // CDC：-->
        if c == '-' && self.peek(1) == Some('-') && self.peek(2) == Some('>') {
            for _ in 0..3 {
                self.bump();
            }
            self.push_plain(out, TokenKind::Cdc, start);
            return false;
        }
        // 引号 → 字符串
        if c == '"' || c == '\'' {
            self.consume_string(out, c, start);
            return false;
        }
        // 数字（含 .5 / +1 / -1e3）
        if Self::is_digit(c) || ((c == '+' || c == '-' || c == '.') && self.would_start_number()) {
            self.consume_numeric(out, start);
            return false;
        }
        // # hash
        if c == '#' {
            self.consume_hash(out, start);
            return false;
        }
        // @ at-keyword
        if c == '@' {
            if self
                .peek(1)
                .map(|x| Self::is_ident_start(x) || x == '\\')
                .unwrap_or(false)
            {
                self.bump();
                self.consume_ident_like(out, TokenKind::AtKeyword, start);
            } else {
                self.bump();
                let t =
                    self.make_token(TokenKind::Delim, start, self.pos, Payload::Raw, None, Some('@'));
                self.push_counted(out, t, TokenKind::Delim);
            }
            return false;
        }
        // 标识符起始 / 转义 / `-` 开头（`--custom`、`-moz-x`）
        if Self::is_ident_start(c)
            || c == '\\'
            || (c == '-' && (self.would_start_ident_seq() || self.peek(1) == Some('\\')))
        {
            self.consume_ident_like(out, TokenKind::Ident, start);
            return false;
        }
        // 单字符记号直查（含退化的 `/`，走到这里说明它不是注释）
        if let Some(k) = single_char_kind(c) {
            self.bump();
            let d = if k == TokenKind::Delim { Some(c) } else { None };
            let t = self.make_token(k, start, self.pos, Payload::Raw, None, d);
            self.push_counted(out, t, k);
            return false;
        }
        // 其余：分隔符（含未成类的 '-' '.' '+' '<' '!' 等）
        self.bump();
        let t = self.make_token(TokenKind::Delim, start, self.pos, Payload::Raw, None, Some(c));
        self.push_counted(out, t, TokenKind::Delim);
        false
    }

    /// 无载荷 token 的统一推入（避免五处重复样板）。
    fn push_plain(&mut self, out: &mut Vec<Token>, k: TokenKind, start: usize) {
        let t = self.make_token(k, start, self.pos, Payload::Raw, None, None);
        self.push_counted(out, t, k);
    }

    /// 推入并同步计数（**计数与推入必须成对**，否则统计与 token 数脱钩）。
    fn push_counted(&mut self, out: &mut Vec<Token>, t: Token, k: TokenKind) {
        out.push(t);
        self.stats.count(k);
        self.emitted = self.emitted.saturating_add(1);
    }

    /// 嵌套深度调整（**上溢钳制 + 下溢归零**，两向都要有路）。
    fn adjust_depth(&mut self, k: TokenKind) {
        if k.opens_block() {
            if self.depth >= self.limits.max_nesting_depth {
                let high = self.limits.max_nesting_depth as f64;
                self.stats.truncations = self.stats.truncations.saturating_add(1);
                let _ = self.clamps.push(ClampNotice {
                    field: "nesting.depth".to_string(),
                    original: (self.depth as f64) + 1.0,
                    clamped: self.depth as f64,
                    low: 0.0,
                    high,
                    tick: self.tick,
                });
                let _ = self.cases.open_case(
                    "嵌套深度超上限被钳制",
                    "更深层的括号结构被压平，嵌套选择器可能匹配错误",
                    "Lexer::adjust_depth",
                    FIX_RECOVERY,
                    self.tick,
                );
            } else {
                self.depth = self.depth.saturating_add(1);
                self.stats.max_depth = self.stats.max_depth.max(self.depth);
            }
        } else if k.closes_block() {
            if self.depth == 0 {
                let _ = self.cases.open_case(
                    "闭合括号无对应开启",
                    "多余闭合符按深度零收口，规则结构可能错位",
                    "Lexer::adjust_depth",
                    FIX_RECOVERY,
                    self.tick,
                );
            } else {
                self.depth = self.depth.saturating_sub(1);
            }
        }
    }

    /// 构造一个 token（**统一走这里**，从而保证偏移钳制与单 token 字节上限
    /// 只在一处实现——分散写就是两份必然漂移的边界逻辑）。
    fn make_token(
        &mut self,
        kind: TokenKind,
        start: usize,
        end: usize,
        payload: Payload,
        numeric: Option<NumberValue>,
        delim: Option<char>,
    ) -> Token {
        self.adjust_depth(kind);
        let slen = self.pre.len();
        let s32 = u32::try_from(start.min(slen)).unwrap_or(u32::MAX);
        let mut e32 = u32::try_from(end.min(slen)).unwrap_or(u32::MAX);
        if e32 < s32 {
            e32 = s32;
        }
        // 单 token 字节上限：原文区间截到上限。**规范化载荷不受此限**——它是已
        // 解码的语义，不该被原文上限连坐（否则转义解码后反而被截断）。
        let raw_len = e32.saturating_sub(s32) as usize;
        if raw_len > self.limits.max_token_bytes {
            let cap = self.limits.max_token_bytes;
            e32 = s32.saturating_add(cap as u32);
            self.note_truncation("token.bytes", raw_len as f64, cap as f64, cap as f64);
            let _ = self.cases.open_case(
                "单token 超字节上限被截断",
                "该 token 原文尾部被截断；规范化载荷可能仍完整",
                "Lexer::make_token 单 token 闸",
                FIX_LIMITS,
                self.tick,
            );
        }
        Token {
            kind,
            start: s32,
            end: e32,
            payload,
            numeric,
            hash_type: HashType::Unset,
            delim,
            depth: self.depth,
        }
    }

    // -----------------------------------------------------------------------
    // 8.4 注释（§4.3.2）
    // -----------------------------------------------------------------------

    /// 消费一段注释（**未闭合即立案**：`/*` 后一路吃到 EOF 才收尾是真实的失败
    /// 形态——整张表的其余部分会被当成注释吃掉）。
    fn consume_comment(&mut self) {
        self.stats.comments = self.stats.comments.saturating_add(1);
        self.bump();
        self.bump();
        loop {
            match self.peek(0) {
                None => {
                    let _ = self.cases.open_case(
                        "注释未闭合",
                        "自注释起点至流末尾全部被吞为注释，其后所有声明不生效",
                        "Lexer::consume_comment",
                        FIX_RECOVERY,
                        self.tick,
                    );
                    return;
                }
                Some('*') if self.peek(1) == Some('/') => {
                    self.bump();
                    self.bump();
                    return;
                }
                Some(_) => {
                    self.bump();
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // 8.5 标识符（§4.3.11 / §4.3.13）
    // -----------------------------------------------------------------------

    /// 消费标识符序列到 `self.buf`，返回**是否发生过转义解码**。
    ///
    /// 未发生转义时调用方走零拷贝切片，不物化文本。
    fn consume_name(&mut self) -> bool {
        self.buf.clear();
        let mut escaped = false;
        loop {
            let Some(c) = self.peek(0) else { break };
            if c == '\\' {
                if self.is_valid_escape() {
                    escaped = true;
                    self.consume_escaped_code_point();
                    continue;
                }
                // 非法转义：前进一格并立案（规范要求前进，不许重试）。
                self.bump();
                let _ = self.cases.open_case(
                    "非法转义序列",
                    "该标识符或字符串的此处被按原样保留，可能拼出错误的属性名",
                    "Lexer::consume_name",
                    FIX_RECOVERY,
                    self.tick,
                );
                continue;
            }
            if !Self::is_name(c) {
                break;
            }
            self.bump();
            self.buf.push(c);
        }
        escaped
    }

    /// 消费转义码点（§4.3.7）：`\` + 1~6 位十六进制 + 至多一个空白，
    /// 或 `\` + 单个非换行字符。
    fn consume_escaped_code_point(&mut self) {
        self.bump(); // 吃掉 '\'
        let mut hex: u32 = 0;
        let mut digits = 0usize;
        while digits < 6 {
            match self.peek(0).and_then(hex_digit_value) {
                Some(v) => {
                    hex = hex.saturating_mul(16).saturating_add(v as u32);
                    self.bump();
                    digits += 1;
                }
                None => break,
            }
        }
        if digits == 0 {
            // 无十六进制：取下一个码点原样（非换行，因 is_valid_escape 已挡）。
            if let Some(c) = self.bump() {
                self.buf.push(c);
            }
            self.stats.escapes_decoded = self.stats.escapes_decoded.saturating_add(1);
            return;
        }
        // 十六进制之后至多一个空白。
        if let Some(c) = self.peek(0) {
            if Self::is_whitespace(c) {
                self.bump();
            }
        }
        // 0 / 代理区 / 超上界 → U+FFFD（§5.2.2）。
        let ch = if hex == 0 || (0xD800..=0xDFFF).contains(&hex) || hex > 0x10FFFF {
            '\u{FFFD}'
        } else {
            char::from_u32(hex).unwrap_or('\u{FFFD}')
        };
        self.buf.push(ch);
        self.stats.escapes_decoded = self.stats.escapes_decoded.saturating_add(1);
    }

    /// 类标识符记号（Ident / Function / AtKeyword 共用本函数）。
    ///
    /// 规范 §4.3.13 的三个分支次序**不可调换**：
    /// URL 特例 → 函数 → 标识符。先判 URL 是因为 `url(` 必须收成单个 URL
    /// 记号而不是 `url` + `(`。
    ///
    /// **这里不再判空白**：空白已在 [`Lexer::next_token`] 的分派最前单独成
    /// 记号（§4.3.1）。若在本函数里「消费完名字后再看下一个字符是不是空白」
    /// 并据此把整个 token 改判成 Whitespace，`a   b` 里的 `a` 会被吐成
    /// `Whitespace[0,1)`——**类型与内容彻底对不上号**，且判据只看
    /// `Whitespace` 出现与否会全绿。故此处一律不再改判类型。
    fn consume_ident_like(&mut self, out: &mut Vec<Token>, kind: TokenKind, start: usize) {
        let escaped = self.consume_name();
        let name_is_url = if escaped {
            ascii_case_insensitive_eq(self.buf.as_str(), "url")
        } else {
            match self.pre.get(start..self.pos) {
                Some(s) => ascii_case_insensitive_eq(s, "url"),
                None => false,
            }
        };
        // ② `url(` 特例：仅当名字是 url 的大小写无关匹配、紧邻左括号、
        //    且括号后**不是引号**时才走 URL 消费（`url("x")` 是函数记号）。
        if name_is_url && self.peek(0) == Some('(') && !matches!(self.peek(1), Some('"') | Some('\'')) {
            self.consume_url(out, start);
            return;
        }
        // ③ 紧邻左括号 → 函数记号。
        //
        // 规范 §4.3.10「Consume a function」明令：**左括号被函数记号吃掉**，
        // 不再单独产 `(-token。故 `rgb(` 只产Function 一个记号，且其区间
        // 末字节就是 `(`。早先把 `(` 留给下一轮会让下游多见一个括号记号，
        // 与规范 token 流形状不符（`(` 作为独立记号只在「不被函数吃掉」时出现）。
        //
        // 函数记号同时是**开括号类**：括号已进入本记号，嵌套深度在此 +1。
        if self.peek(0) == Some('(') {
            self.bump(); // 吃掉 '('，并入本记号区间
            // 深度调整由 `make_token` 内的 `adjust_depth` 单点完成（此处不可
            // 重复调用，否则函数记号会把嵌套深度 +2）。
            let payload = self.name_payload(escaped, start);
            let t = self.make_token(TokenKind::Function, start, self.pos, payload, None, None);
            self.push_counted(out, t, TokenKind::Function);
            return;
        }
        // ④ 标识符 / at 关键字。
        let payload = self.name_payload(escaped, start);
        let t = self.make_token(kind, start, self.pos, payload, None, None);
        self.push_counted(out, t, kind);
    }

    /// 名字载荷：转义过则物化，否则零拷贝（at 关键字需去掉 `@` 前缀）。
    fn name_payload(&self, escaped: bool, start: usize) -> Payload {
        if escaped {
            return Payload::Decoded(self.buf.clone());
        }
        let from = if self.pre.get(start..).map(|s| s.starts_with('@')).unwrap_or(false) {
            start + 1
        } else {
            start
        };
        // 零拷贝：用空 Raw 标记，由流侧按 kind 的已知偏移规则切出。
        Payload::RawShifted(u32::try_from(from - start).unwrap_or(u32::MAX))
    }

    // -----------------------------------------------------------------------
    // 8.6 字符串（§4.3.5）
    // -----------------------------------------------------------------------

    /// 消费字符串（含坏字符串）。
    ///
    /// 两种失败的**对外形态不同**，必须分开：
    /// - 遇未转义换行 → `BadString`（规范明令，且换行**不消费**，留给下一轮）；
    /// - 遇 EOF → 仍吐 `String`（已收部分）但**立案**（未闭合是真实失败形态，
    ///   但规范不把它归为 bad-string；把它算进 BadString 会让「换行炸的声明」
    ///   与「少个引号的声明」失去区分，而这两者的修法不同）。
    fn consume_string(&mut self, out: &mut Vec<Token>, quote: char, start: usize) {
        self.bump(); // 吃掉开引号
        self.buf.clear();
        let mut escaped = false;
        let content_start = self.pos;
        loop {
            let Some(c) = self.peek(0) else {
                // EOF：未闭合 → 仍吐 String（已收部分）并立案。
                let payload = self.string_payload(escaped, start, content_start);
                let t = self.make_token(TokenKind::String, start, self.pos, payload, None, None);
                self.push_counted(out, t, TokenKind::String);
                let _ = self.cases.open_case(
                    "字符串未闭合",
                    "该条声明被丢弃；字符串之后直到流末尾的内容一并失效",
                    "Lexer::consume_string",
                    FIX_RECOVERY,
                    self.tick,
                );
                return;
            };
            if c == quote {
                self.bump();
                let payload = self.string_payload(escaped, start, content_start);
                let t = self.make_token(TokenKind::String, start, self.pos, payload, None, None);
                self.push_counted(out, t, TokenKind::String);
                return;
            }
            if c == '\n' {
                // 坏字符串：换行不消费（留给下一 token），按已收部分吐 BadString。
                let payload = self.string_payload(escaped, start, content_start);
                let t = self.make_token(TokenKind::BadString, start, self.pos, payload, None, None);
                self.push_counted(out, t, TokenKind::BadString);
                let _ = self.cases.open_case(
                    "字符串内出现未转义换行",
                    "该条声明被丢弃；坏字符串之后的声明不受影响",
                    "Lexer::consume_string",
                    FIX_RECOVERY,
                    self.tick,
                );
                return;
            }
            if c == '\\' {
                match self.peek(1) {
                    None => {
                        // EOF 前的裸反斜杠：立案并前进（规范：parse error）。
                        self.bump();
                        let _ = self.cases.open_case(
                            "字符串以裸反斜杠结束",
                            "该条声明被丢弃",
                            "Lexer::consume_string",
                            FIX_RECOVERY,
                            self.tick,
                        );
                        continue;
                    }
                    Some(n) if Self::is_whitespace(n) && n != '\n' && n != '\u{0C}' => {
                        // 反斜杠 + 合法空白（§4.3.8 续行）→ 两字节都消费，不入载荷。
                        self.bump();
                        self.bump();
                        continue;
                    }
                    Some('\n') | Some('\u{0C}') => {
                        // 反斜杠 + 换行（续行）：消费两个码点，不写入载荷。
                        self.bump();
                        self.bump();
                        continue;
                    }
                    Some(_) => {
                        escaped = true;
                        self.consume_escaped_code_point();
                        continue;
                    }
                }
            }
            self.bump();
            self.buf.push(c);
        }
    }

    /// 字符串载荷：转义过则物化，否则零拷贝（去掉两侧引号）。
    /// 字符串载荷（§4.3.5）：偏移**相对 token 起始**（token 含两侧引号，
    /// 载荷不含），内容终点由 `TokenStream::content_end` 按类型截到闭引号之前。
    /// 传绝对偏移会让载荷整体右移一个记号长度，`"a"` 的载荷就成了 `a"`。
    fn string_payload(&self, escaped: bool, start: usize, content_start: usize) -> Payload {
        if escaped {
            Payload::Decoded(self.buf.clone())
        } else {
            Payload::RawShifted(u32::try_from(content_start.saturating_sub(start)).unwrap_or(u32::MAX))
        }
    }

    // -----------------------------------------------------------------------
    // 8.7 hash（§4.3.4）
    // -----------------------------------------------------------------------

    /// 消费 hash 记号（带 type flag）。
    fn consume_hash(&mut self, out: &mut Vec<Token>, start: usize) {
        self.bump(); // '#'
        let flag = match self.peek(0) {
            Some(c) if Self::is_ident_start(c) => Some(HashType::Id),
            Some(c) if Self::is_digit(c) => Some(HashType::Unrestricted),
            _ => None,
        };
        let Some(flag) = flag else {
            // 退化：'#' 单独成分隔符（规范明令）。
            let t = self.make_token(TokenKind::Delim, start, self.pos, Payload::Raw, None, Some('#'));
            self.push_counted(out, t, TokenKind::Delim);
            return;
        };
        let content_start = self.pos;
        let escaped = self.consume_name();
        let payload = if escaped {
            Payload::Decoded(self.buf.clone())
        } else {
            Payload::RawShifted(
                u32::try_from(content_start.saturating_sub(start)).unwrap_or(u32::MAX),
            )
        };
        let mut t = self.make_token(TokenKind::Hash, start, self.pos, payload, None, None);
        t.hash_type = flag;
        self.push_counted(out, t, TokenKind::Hash);
    }

    // -----------------------------------------------------------------------
    // 8.8 数值（§4.3.7 / §5.2.2）
    // -----------------------------------------------------------------------

    /// 消费数值记号（Number / Percentage / Dimension）。
    fn consume_numeric(&mut self, out: &mut Vec<Token>, start: usize) {
        let (value, has_fraction, has_exponent, signed) = self.consume_number();
        let num = NumberValue::build(value, has_fraction, has_exponent, signed);
        // 单位？
        let has_unit = matches!(self.peek(0), Some(c) if Self::is_ident_start(c)) || self.is_valid_escape();
        if has_unit {
            let ustart = self.pos;
            let escaped = self.consume_name();
            let unit = if escaped {
                self.buf.clone()
            } else {
                self.pre.get(ustart..self.pos).unwrap_or("").to_string()
            };
            let t = self.make_token(
                TokenKind::Dimension,
                start,
                self.pos,
                Payload::Decoded(unit),
                Some(num),
                None,
            );
            self.push_counted(out, t, TokenKind::Dimension);
            return;
        }
        if self.peek(0) == Some('%') {
            self.bump();
            // 百分号的载荷是数值，% 本身不入载荷——下游按类型取数即可。
            let t = self.make_token(
                TokenKind::Percentage,
                start,
                self.pos,
                Payload::Raw,
                Some(num),
                None,
            );
            self.push_counted(out, t, TokenKind::Percentage);
            return;
        }
        // Number 记号**必须**带数值载荷：`TokenKind::carries_number` 把
        // Number/Percentage/Dimension 三类都列为必带，此处传 `None` 会让
        // `Token::carrier_is_sane` 判红、`TokenStream::audit` 判红，
        // 而「裸数值」恰恰是下游最常取的那一个载荷。
        let t = self.make_token(TokenKind::Number, start, self.pos, Payload::Raw, Some(num), None);
        self.push_counted(out, t, TokenKind::Number);
    }

    /// 消费一个数值（§5.2.2），返回
    /// `(值, 有小数部分, 有指数部分, 显式符号)`。
    ///
    /// **指数不合法时按规范回退**：`1e` 后无数字 → 记解析错误并把 `e` 留作
    /// 标识符起始（此时「起始标识符序列」判定为真，Dimension 分支自然接管
    /// `1e` 这个整体——这是规范要求的**唯一**允许的「回头」形态，且它是
    /// 一次性的常数级，不是 O(N²) 式回溯）。
    fn consume_number(&mut self) -> (f64, bool, bool, bool) {
        let mut signed = false;
        let mut negative = false;
        match self.peek(0) {
            Some('+') => {
                signed = true;
                self.bump();
            }
            Some('-') => {
                signed = true;
                negative = true;
                self.bump();
            }
            _ => {}
        }
        let mut int_part: u64 = 0;
        while let Some(c) = self.peek(0) {
            if !Self::is_digit(c) {
                break;
            }
            let d = c.to_digit(10).unwrap_or(0) as u64;
            int_part = int_part.saturating_mul(10).saturating_add(d);
            self.bump();
        }
        let mut has_fraction = false;
        let mut frac_part = 0.0f64;
        if self.peek(0) == Some('.') && matches!(self.peek(1), Some(d) if Self::is_digit(d)) {
            has_fraction = true;
            self.bump();
            let mut scale = 0.1f64;
            while let Some(c) = self.peek(0) {
                if !Self::is_digit(c) {
                    break;
                }
                frac_part += c.to_digit(10).unwrap_or(0) as f64 * scale;
                scale *= 0.1;
                self.bump();
            }
        }
        let mut has_exponent = false;
        let mut exp: i32 = 0;
        if matches!(self.peek(0), Some('e') | Some('E')) {
            // 前瞻指数尾部；尾部无数字则不消费（留给标识符分支）。
            let mut n = 1usize;
            let mut exp_neg = false;
            if matches!(self.peek(n), Some('+') | Some('-')) {
                exp_neg = self.peek(n) == Some('-');
                n += 1;
            }
            let mut digits = 0usize;
            let mut acc: i32 = 0;
            while let Some(c) = self.peek(n) {
                if !Self::is_digit(c) {
                    break;
                }
                acc = acc
                    .saturating_mul(10)
                    .saturating_add(c.to_digit(10).unwrap_or(0) as i32);
                digits += 1;
                n += 1;
            }
            if digits > 0 {
                has_exponent = true;
                exp = if exp_neg { -acc } else { acc };
                self.bump(); // e / E
                if matches!(self.peek(0), Some('+') | Some('-')) {
                    self.bump();
                }
                for _ in 0..digits {
                    self.bump();
                }
            }
        }
        let mut v = int_part as f64 + frac_part;
        if has_exponent {
            v *= pow10(exp);
        }
        if negative {
            v = -v;
        }
        (v, has_fraction, has_exponent, signed)
    }

    // -----------------------------------------------------------------------
    // 8.9 URL（§4.3.6 / §4.3.14）
    // -----------------------------------------------------------------------

    /// 消费 URL 记号（或坏 URL）。
    fn consume_url(&mut self, out: &mut Vec<Token>, start: usize) {
        self.bump(); // '('
        while self.peek(0).map(Self::is_whitespace).unwrap_or(false) {
            self.bump();
        }
        self.url_content_start = self.pos;
        // `self.buf` 上一轮装的是函数名 `url`（见 consume_name），URL 内容
        // 必须另起缓冲：不清空的话 BadUrl 的 `Decoded(buf.clone())` 会把
        // 函数名一起当成 URL 内容吐出去（`url(a'b)` 载荷成 `"urla"`）。
        self.buf.clear();
        if self.at_eof() {
            let t = self.make_token(TokenKind::Url, start, self.pos, Payload::Raw, None, None);
            self.push_counted(out, t, TokenKind::Url);
            let _ = self.cases.open_case(
                "URL 在左括号后即结束",
                "该条声明被丢弃",
                "Lexer::consume_url",
                FIX_RECOVERY,
                self.tick,
            );
            return;
        }
        let mut escaped = false;
        loop {
            let Some(c) = self.peek(0) else {
                // EOF 未闭合：仍吐 Url（已收部分）并立案。
                let payload = self.url_payload(escaped, start, self.url_content_start);
                let t = self.make_token(TokenKind::Url, start, self.pos, payload, None, None);
                self.push_counted(out, t, TokenKind::Url);
                let _ = self.cases.open_case(
                    "URL 未闭合",
                    "该条声明被丢弃",
                    "Lexer::consume_url",
                    FIX_RECOVERY,
                    self.tick,
                );
                return;
            };
            if c == ')' {
                self.bump();
                let payload = self.url_payload(escaped, start, self.url_content_start);
                let t = self.make_token(TokenKind::Url, start, self.pos, payload, None, None);
                self.push_counted(out, t, TokenKind::Url);
                return;
            }
            if c == '"' || c == '\'' || c == '(' || Self::is_non_printable(c) {
                self.consume_bad_url_remnants();
                // 载荷语义是「已收到的 URL 内容」：转义过则物化（buf 里是解码
                // 结果），未转义则零拷贝切出内容子段。两条路都不含函数名。
                let bad_payload = if escaped {
                    Payload::Decoded(self.buf.clone())
                } else {
                    Payload::RawShifted(
                        u32::try_from(self.url_content_start.saturating_sub(start)).unwrap_or(u32::MAX),
                    )
                };
                let t =
                    self.make_token(TokenKind::BadUrl, start, self.pos, bad_payload, None, None);
                self.push_counted(out, t, TokenKind::BadUrl);
                let _ = self.cases.open_case(
                    "URL 内出现非法字符",
                    &format!("非法字符 '{}'；该条声明被丢弃", c),
                    "Lexer::consume_url",
                    FIX_RECOVERY,
                    self.tick,
                );
                return;
            }
            if c == '\\' {
                if self.is_valid_escape() {
                    escaped = true;
                    self.consume_escaped_code_point();
                    continue;
                }
                self.consume_bad_url_remnants();
                // 载荷语义是「已收到的 URL 内容」：转义过则物化（buf 里是解码
                // 结果），未转义则零拷贝切出内容子段。两条路都不含函数名。
                let bad_payload = if escaped {
                    Payload::Decoded(self.buf.clone())
                } else {
                    Payload::RawShifted(
                        u32::try_from(self.url_content_start.saturating_sub(start)).unwrap_or(u32::MAX),
                    )
                };
                let t =
                    self.make_token(TokenKind::BadUrl, start, self.pos, bad_payload, None, None);
                self.push_counted(out, t, TokenKind::BadUrl);
                let _ = self.cases.open_case(
                    "URL 内出现非法转义",
                    "该条声明被丢弃",
                    "Lexer::consume_url",
                    FIX_RECOVERY,
                    self.tick,
                );
                return;
            }
            self.bump();
            self.buf.push(c);
        }
    }

    /// URL 载荷：转义过则物化，否则从内容起点零拷贝（不含右括号）。
    /// URL 载荷（§4.3.6）：`RawShifted` 里的偏移是**相对 token 起始**的，
    /// 切出时还要按类型截到配对 `)` 之前（见 `TokenStream::content_end`）。
    fn url_payload(&self, escaped: bool, start: usize, content_start: usize) -> Payload {
        if escaped {
            Payload::Decoded(self.buf.clone())
        } else {
            Payload::RawShifted(u32::try_from(content_start.saturating_sub(start)).unwrap_or(u32::MAX))
        }
    }

    /// 消费坏 URL 的残余（§4.3.14：吃到配对 `)` 或 EOF 为止）。
    fn consume_bad_url_remnants(&mut self) {
        loop {
            match self.peek(0) {
                None => return,
                Some(')') => {
                    self.bump();
                    return;
                }
                Some(_) => {
                    self.bump();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 九、自由函数：预处理与码点工具
// ---------------------------------------------------------------------------

/// 预处理（§4.3.1）：`\r\n` → `\n`、`\r` → `\n`、`\f` → `\n`、NUL → U+FFFD。
///
/// 顺序不可调换：先归一换行再换 NUL，否则 `\f` 会被当成 NUL 一起替换，
/// 产生「两个不同的字节都变成 U+FFFD」的不可逆混淆。
pub fn preprocess(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    // 按**码点**推进，不可按字节：`b as char` 会把 UTF-8 的每个后续字节当成
    // Latin-1 码点，`中`（E4 B8 AD）会被压成 `ä¸­`——字符数不变、字节数变，
    // 于是「偏移以字节为单位」与「文本以码点为单位」两套坐标系当场错位，
    // 后续所有切分偏移都不可信。故：ASCII 控制字节按规则替换，
    // 其余（含多字节序列）按 `char` 整体搬运。
    //
    // CRLF 必须**成对**折叠为一个 LF：逐码点处理时`\r` 产一个 `\n`、紧随的
    // `\n` 又产一个，`a\r\nb` 会变成 `a\n\nb`（多出一个空行记号）。
    // 规范口径是「CRLF / CR / FF 三者都归一为单个 LF」。
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                // 窥看下一个：紧跟 LF 则一并吞掉，只产一个 LF。
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\u{0C}' => out.push('\n'),
            '\u{0}' => out.push('\u{FFFD}'),
            _ => out.push(c),
        }
    }
    out
}

/// 10 的整数次幂（小指数内精确；超出用 powi 近似并夹紧）。
fn pow10(exp: i32) -> f64 {
    if exp >= 0 {
        let mut v = 1.0f64;
        for _ in 0..exp.min(308) {
            v *= 10.0;
        }
        v
    } else {
        let mut v = 1.0f64;
        for _ in 0..(-exp).min(308) {
            v /= 10.0;
        }
        v
    }
}

/// 单字符记号直查（**穷举表**：10 类单字符记号 + 退化的 `/`）。
fn single_char_kind(c: char) -> Option<TokenKind> {
    match c {
        ':' => Some(TokenKind::Colon),
        ';' => Some(TokenKind::Semicolon),
        ',' => Some(TokenKind::Comma),
        '[' => Some(TokenKind::LeftBracket),
        ']' => Some(TokenKind::RightBracket),
        '(' => Some(TokenKind::LeftParen),
        ')' => Some(TokenKind::RightParen),
        '{' => Some(TokenKind::LeftCurly),
        '}' => Some(TokenKind::RightCurly),
        '/' => Some(TokenKind::Delim),
        _ => None,
    }
}

/// ASCII 大小写无关相等（**只折 ASCII 大写**：CSS 规定 ident 比较只对 ASCII
/// 大小写折中，非 ASCII 一律原样——写成 `to_lowercase()` 会把非 ASCII 也折了，
/// 那不是 CSS 的规则）。
fn ascii_case_insensitive_eq(s: &str, target: &str) -> bool {
    if s.len() != target.len() {
        return false;
    }
    for (a, b) in s.bytes().zip(target.bytes()) {
        if a != b && !eq_ascii_fold(a, b) {
            return false;
        }
    }
    true
}

/// ASCII 大写折叠相等。
fn eq_ascii_fold(a: u8, b: u8) -> bool {
    matches!(a, b'A'..=b'Z') && a + 32 == b
}

/// 码点 → hex 数字值（0..=15）。
fn hex_digit_value(c: char) -> Option<u8> {
    let v = c as u32;
    if v > 0x7F {
        return None;
    }
    hex_val(c as u8)
}

/// 取不超过 `limit` 的最近字符边界（UTF-8 安全截断）。
pub fn floor_char_boundary(s: &str, limit: usize) -> usize {
    if limit >= s.len() {
        return s.len();
    }
    let mut i = limit;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

// ---------------------------------------------------------------------------
// 十、token 流（下游消费面 + 自检 + 读屏替述）
// ---------------------------------------------------------------------------

/// token 流（**下游唯一消费面**）。
///
/// 流**自己拥有**预处理后的源，token 只存偏移——故流可自由移动、可跨线程
/// （只要 `Send` 条件成立），不需要任何自引用技巧。
#[derive(Clone, Debug)]
pub struct TokenStream {
    /// 预处理后的源（零拷贝切片的锚）。
    pub pre: String,
    /// token 序列（末位必为唯一 [`TokenKind::Eof`]）。
    pub tokens: Vec<Token>,
    /// 统计。
    pub stats: TokenStats,
    /// 钳制告警账（承F2801）。
    pub clamps: ClampLog,
    /// 案件账（承 F2801）。
    pub cases: CaseLedger,
    /// 是否因上限被截断（**被截断的流不是完整流**，下游必须显式处理）。
    pub truncated: bool,
}

impl TokenStream {
    /// token 数。
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// 是否为空（**恒为 false**：空流也有末尾 EOF 记号）。
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &Token> {
        self.tokens.iter()
    }

    /// 取第 `i` 个 token。
    pub fn get(&self, i: usize) -> Option<&Token> {
        self.tokens.get(i)
    }

    /// 错误产物清单（BadString / BadUrl）。
    pub fn errors(&self) -> Vec<&Token> {
        self.tokens.iter().filter(|t| t.is_error()).collect()
    }

    /// 第 `i` 个 token 的原文切片（**零拷贝**；偏移异常返回空串）。
    pub fn slice_text(&self, i: usize) -> &str {
        match self.tokens.get(i) {
            Some(t) => self
                .pre
                .get(t.start as usize..t.end as usize)
                .unwrap_or(""),
            None => "",
        }
    }

    /// 第 `i` 个 token 的**语义载荷**（规范化文本）。
    ///
    /// 零拷贝路径：载荷为 `Raw` 或`RawShifted` 时按已知偏移规则从源切出
    /// （函数记号切到 `(` 之前、字符串/hash 切掉前缀、URL 切到 `)` 之前、
    /// 带单位数值切掉数值部分）。物化路径：直接返回 owned 副本。
    pub fn payload(&self, i: usize) -> &str {
        let Some(t) = self.tokens.get(i) else {
            return "";
        };
        match &t.payload {
            Payload::Decoded(s) => s.as_str(),
            // 次序不可调换：`RawShifted` 必须排在 `Raw` 之前。若把
            // `p if p.is_raw()` 守卫分支放在前面，它会连带吞掉 `RawShifted`
            // ——守卫分支对二者同真，而守卫不计入穷尽性，于是既编译不过
            // （non-exhaustive），偏移逻辑又永远走不到。改为逐变体显式匹配。
            Payload::RawShifted(off) => {
                let from = (t.start as usize).saturating_add(*off as usize);
                let to = self.content_end(i, from);
                self.pre.get(from..to).unwrap_or("")
            }
            Payload::Raw => self.slice_text(i),
        }
    }

    /// 第 `i` 个 token 的载荷内容终点（**按类型逐条派生，不猜**）。
    fn content_end(&self, i: usize, from: usize) -> usize {
        let Some(t) = self.tokens.get(i) else {
            return from;
        };
        let end = t.end as usize;
        match t.kind {
            // 函数记号与 at 关键字的载荷止于标识符末尾，即 `(` 之前 / 标识符末尾。
            TokenKind::Function => {
                let head = self.pre.get(from..end).unwrap_or("");
                match head.find('(') {
                    Some(p) => from + p,
                    None => end,
                }
            }
            // URL 载荷止于配对右括号之前。
            TokenKind::Url | TokenKind::BadUrl => end.saturating_sub(1).max(from),
            // 字符串/坏字符串载荷止于闭引号之前（token 含引号，载荷不含）。
            // BadUrl 的闭字符是 `)`，但坏 URL 可能吞到 EOF 而无闭字符，
            // 故只在确有闭字符时收一格，否则 `end` 原样（`max` 兜底不越界）。
            TokenKind::String => end.saturating_sub(1).max(from),
            // Dimension 载荷是单位：起点是数值末尾。数值长度为「载荷起点 - token起点」
            // 无法直接反推，故 Dimension 一律物化（见 consume_numeric），
            // 这里到达即返回 end（防御性：不该走到物化分支之外的路径）。
            _ => end,
        }
    }

    /// 流摘要哈希（**下游对账基准**：F2819 fuzz 以它为变异前后对照）。
    pub fn digest(&self) -> String {
        let mut buf = String::new();
        for (i, t) in self.tokens.iter().enumerate() {
            buf.push_str(&t.kind.code());
            buf.push(':');
            buf.push_str(self.payload(i));
            buf.push(':');
            match t.numeric {
                Some(n) => buf.push_str(&format!("{},{}", n.value, n.is_integer)),
                None => buf.push('-'),
            }
            buf.push(':');
            buf.push_str(&format!("{},{}", t.start, t.end));
            buf.push('|');
        }
        fnv1a64_hex(buf.as_bytes())
    }

    /// 流自检（**结构一致性；任一不成立即红**）。
    ///
    /// 查八件事：
    /// ① 末位必为唯一 EOF，且EOF 恰好一个；
    /// ② 每 token 偏移自洽且不越源长；
    /// ③ 正向载荷（类型 → 必带载荷）；
    /// ④ 反向载荷（非数值类型不得带数值载荷）；
    /// ⑤ hash type flag 只在 hash 记号上；
    /// ⑥ delim 字面量只在 Delim 上；
    /// ⑦ 逐类计数之和 == `stats.tokens` == token 数；
    /// ⑧ 承载类必需载荷（`carries_text` 的类型必须有可取载荷的长度来源）。
    pub fn audit(&self) -> Result<(), StyleError> {
        // ① EOF 唯一且居末
        let eof_count = self.tokens.iter().filter(|t| t.kind == TokenKind::Eof).count();
        let last_is_eof = matches!(self.tokens.last().map(|t| t.kind), Some(TokenKind::Eof));
        if eof_count != 1 || !last_is_eof {
            return Err(StyleError::new(
                E_STREAM_INCONSISTENT,
                "流自检失败：EOF 记号不唯一或不居末",
                &format!("EOF 出现 {} 次，末位是 EOF：{}", eof_count, last_is_eof),
FIX_SELFCHECK,
                "分词段维护方",
            ));
        }
        let slen = self.pre.len();
        for (i, t) in self.tokens.iter().enumerate() {
            // ② 偏移
            if !t.span_is_sane(slen) {
                return Err(StyleError::new(
                    E_SPAN_INVALID,
                    "流自检失败：token 偏移越界",
                    &format!(
                        "第 {} 号（{}）偏移 [{}, {}) 越出源长 {}",
                        i,
                        t.kind.code(),
                        t.start,
                        t.end,
                        slen
                    ),
                    "偏移一律经 make_token 钳制，不允许旁路构造",
                    "分词段维护方",
                ));
            }
            // ② 记号类型码必须可逆（枚举守卫自洽性）
            //
            // 有了 label / code 不等于可达：若 `TokenKind::from_code` 与
            // `TokenKind::code` 不同源（历史上就错成hex 解析十进制序位），
            // 记号类型码会静默指向别的类型，下游按码分派即全错。故在自检里
            // 显式做一次往返，不让这类失配溜过。
            let tcode = t.kind.code();
            if TokenKind::from_code(tcode.as_str()) != Some(t.kind) {
                return Err(StyleError::new(
                    E_TOKEN_KIND_INVALID,
                    "流自检失败：记号类型码不可逆",
                    &format!(
                        "第 {} 号的类型 {} 编码为 {}，反解不回同一类型",
                        i,
                        t.kind.zh(),
                        tcode
                    ),
                    FIX_KIND,
                    "分词段维护方",
                ));
            }
            // ③ 正向载荷
            if !t.carrier_is_sane() {                let code = if t.kind.carries_hash_flag() {
                    E_HASH_FLAG_MISSING
                } else if t.kind.carries_delim() {
                    E_DELIM_CARRIER
                } else {
                    E_CARRIER_MISMATCH
                };
                return Err(StyleError::new(
                    code,
                    "流自检失败：类型与载荷不匹配",
                    &format!(
                        "第 {} 号 {} 缺必带载荷（数值 {} / hash 标记 {} / delim {}）",
                        i,
                        t.kind.code(),
                        t.numeric.is_some(),
                        t.hash_type != HashType::Unset,
                        t.delim.is_some()
                    ),
                    FIX_CARRIER,
                    "分词段维护方",
                ));
            }
            // ④ 反向载荷
            if !t.kind.carries_number() && t.numeric.is_some() {
                return Err(StyleError::new(
                    E_CARRIER_MISMATCH,
                    "流自检失败：非数值类型带数值载荷",
                    &format!("第 {} 号 {} 不该带数值载荷", i, t.kind.code()),
                    FIX_CARRIER,
                    "分词段维护方",
                ));
            }
            // ⑤ hash flag 只在 hash 上
            if t.kind != TokenKind::Hash && t.hash_type != HashType::Unset {
                return Err(StyleError::new(
                    E_HASH_FLAG_MISSING,
                    "流自检失败：hash type flag 出现在非 hash 记号上",
                    &format!("第 {} 号 {}", i, t.kind.code()),
                    FIX_CARRIER,
                    "分词段维护方",
                ));
            }
            // ⑥ delim 只在 Delim 上
            if t.kind != TokenKind::Delim && t.delim.is_some() {
                return Err(StyleError::new(
                    E_DELIM_CARRIER,
                    "流自检失败：delim 字面量出现在非分隔符上",
                    &format!("第 {} 号 {}", i, t.kind.code()),
                    FIX_CARRIER,
                    "分词段维护方",
                ));
            }
        }
        // ⑦ 计数聚合
        if self.stats.kind_sum() != self.stats.tokens as u64
            || self.stats.tokens as usize != self.tokens.len()
        {
            return Err(StyleError::new(
                E_STREAM_INCONSISTENT,
                "流自检失败：统计与 token 数不一致",
                &format!(
                    "实有 {} 个 / 逐类求和 {} / 统计口径 {}",
                    self.tokens.len(),
                    self.stats.kind_sum(),
                    self.stats.tokens
                ),
                FIX_SELFCHECK,
                "分词段维护方",
            ));
        }
        Ok(())
    }

    /// 未终态案件数（**收口硬门**：非零即不允许宣告收官）。
    pub fn open_case_count(&self) -> usize {
        self.cases.open_case_count()
    }

    /// 读屏替述（无障碍：整条流能被读屏逐类念出，且截断状态被明说）。
    pub fn screen_text(&self) -> String {
        let mut s = String::from("样式源分词结果：");
        s.push_str(&self.stats.screen_text());
        s.push_str("。逐类计数：");
        for k in TokenKind::ALL.iter() {
            let n = self.stats.kind_count(*k);
            if n > 0 {
                s.push_str(&format!("{} {} 个；", k.zh(), n));
            }
        }
        s.push_str(&format!(
            "钳制告警 {} 条，案件 {} 条（未终态 {} 条），隐私面：{}。",
            self.clamps.len(),
            self.cases.len(),
            self.open_case_count(),
            PRIVACY_SURFACE.zh()
        ));
        if self.truncated {
            s.push_str("注意：本流因超上限被截断，不应据此判定源文件已完整解析。");
        }
        s
    }

    /// 逐 token 的读屏逐行文本（前 `limit` 个；序号由流赋予）。
    pub fn screen_lines(&self, limit: usize) -> Vec<String> {
        let mut out = Vec::new();
        for (i, t) in self.tokens.iter().enumerate() {
            if i >= limit {
                break;
            }
            out.push(format!("第 {} 号：{}", i, t.screen_line()));
        }
        out
    }
}

/// 单条 token 的读屏单行（带序号的便捷入口）。
pub fn token_screen_line(index: usize, t: &Token) -> String {
    format!("第 {} 号：{}", index, t.screen_line())
}

/// 本单自述（读屏总览：架构声明 / 集成边界 / 解析子集 / 判据各一句）。
pub fn module_narration() -> String {
    format!(
        "CSS 词法器与分词管线（{}）：取材 {}.集成边界：不引入任何 crate，纯自持实现，\
         layout 仍不引入（归N 域）。解析子集：只切分 token，不判定属性合法性\
         （子集表归 F2803）。判据：{}类 token 逐条规格 + 全载荷守卫 + 单遍线性 + 零 panic + 无隐私面。",
        LEXER_VERSION,
        ANCHORED_SYNTAX_SPEC,
        TOKEN_KIND_COUNT
    )
}