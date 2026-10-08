//! VE-F0404 · 关键字与保留字管理（VE-C 域 · 着色器系统 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0404`
//!
//! **判据（锚点原文）**：完美哈希、版本裁定、历史保留、编译期断言、判据。
//!
//! **职责定位（锚点原文）**：关键字与保留字登记管理——当前版本关键字集、
//! 历史保留字（兼容旧着色器源，识别但语义按版本裁定）、未来保留域；查找用
//! 完美哈希（编译期生成）；版本化（语言版本差异表驱动哪些词在哪个版本是
//! 关键字）。
//!
//! **设计要点**：
//! - **完美哈希（编译期生成）**：槽位表与种子全部由 `const fn` 在编译期
//!   计算——种子搜索（`find_seed`）在 const 求值里穷举种子直到全部关键字
//!   槽位互异，产物是纯静态表。运行期查找 O(1)：哈希 → 槽位 → 关键字比对
//!   （防非关键字碰撞误报）；
//! - **编译期断言（表损坏拦截）**：`const _: () = assert!(...)` 两条——种子
//!   搜索失败即整 crate 编译失败、槽位冲突即编译失败。表损坏在编译期炸，
//!   不给运行期留任何"带病表"的余地（锚点：表损坏→编译期断言拦截）；
//! - **版本裁定**：版本差异表（`VERSION_DIFFS`）驱动"哪个词在哪个版本是
//!   什么"——同一词在旧版是关键字、新版可能降为历史保留或反之。裁定
//!   O(1)（差弢单条直查 + 关键字表 O(1)）；
//! - **历史保留**：旧版关键字在新版识别但不作为关键字——语义按版本裁定，
//!   误用时报错并提示版本（"这是 V1Legacy 的关键字，当前版本已保留"），
//!   不静默吞掉也不静默放行；
//! - **未来保留域**：未来语言演进预留的词（`FUTURE_RESERVED`）与保留前缀
//!   （`RESERVED_PREFIXES`，如 `gl_`/`vx_`）——标识符不得占用；
//! - **版本未知→按最新版保守**：`LangVersion::conservative()` 语义——未知
//!   版本按当前版裁定（宁严勿宽：把边界词当保留，报错可查；放行错词才是
//!   事故）；
//! - **零静默**：误用产出结构化 `KeywordError`（词/裁定/版本提示/建议）。
//!
//! **性能逐项分解**：查找 O(1) 完美哈希；版本裁定 O(1)；登记 O(1)（静态表
//! 无运行期登记——登记发生在编译期 const 求值，运行期零成本）。
//!
//! **跨批对接点**：上游 F0402 规范（VS-GR 册）、F0415 编码处理；下游 F0405
//! 标识符区分语义（词法层收 Ident，关键字判定在此层完成——LEX-D3 决策的
//! 消费方）。条款引用：VS-G-07（收编按标准）、VS-G-10（判据引用链）。
//!
//! 确定性：纯静态表 + 纯函数，无时钟无 IO。零外部依赖，只用 `alloc` 与
//! `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 一、完美哈希（编译期生成 + 编译期断言）
// ---------------------------------------------------------------------------

/// 当前版本关键字集（VE-Shade V2Current；收编三族共用一张当前表，
/// 方言差异走版本差异表——收编不私改关键字表）。
pub const KEYWORDS: [&str; 24] = [
    "as", "alias", "break", "const", "continue", "else", "false", "fn", "for", "if", "let",
    "loop", "override", "requires", "return", "struct", "true", "uniform", "var", "while",
    "binding", "precision", "sampler", "texture",
];

/// 槽位表尺寸（24 关键字 / 64 槽 ≈ 37% 负载——种子搜索空间充分）。
pub const TABLE_SIZE: usize = 64;

/// 种子化 FNV-1a + fmix32 avalanche 终混（const fn——编译期与运行期同函数，
/// 杜绝两套哈希漂移）。
///
/// 终混是必需的而非装饰：FNV-1a 的低 6 位严重偏置（乘法只把信息向上推，
/// 低位由最后几字节主导），mod 64 槽位期望碰撞数比均匀分布高一个数量级，
/// 完美种子实际不存在。fmix32 把高位信息混回低位后，种子空间才有效。
const fn kw_hash(w: &[u8], seed: u32) -> u32 {
    let mut h: u32 = 2166136261 ^ seed.wrapping_mul(0x9E37_79B1);
    let mut i = 0;
    while i < w.len() {
        h ^= w[i] as u32;
        h = h.wrapping_mul(16777619);
        i += 1;
    }
    // fmix32（Murmur3 终混）：位雪崩，使 mod 2^k 槽位均匀。
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    h
}

/// 种子验证：全部关键字槽位互异（const fn，编译期可调用；运行期镜像用）。
pub(crate) const fn slots_unique(seed: u32) -> bool {
    let mut seen = [false; TABLE_SIZE];
    let mut i = 0;
    while i < KEYWORDS.len() {
        let s = (kw_hash(KEYWORDS[i].as_bytes(), seed) as usize) % TABLE_SIZE;
        if seen[s] {
            return false;
        }
        seen[s] = true;
        i += 1;
    }
    true
}

/// 种子搜索（const fn）：从 1 起穷举，返回首个全部槽位互异的种子。
/// 搜索上限 100 万——种子空间充分，实际首枚可用种子出现在低区。
const fn find_seed() -> u32 {
    let mut s: u32 = 1;
    while s < 1_000_000 {
        if slots_unique(s) {
            return s;
        }
        s += 1;
    }
    0
}

/// 编译期求值：完美种子与槽位表（运行期零计算）。
pub const PERFECT_SEED: u32 = find_seed();

/// 槽位表（0 = 空；n>0 = 关键字下标 n-1 + 1，避开 0 作空标记）。
pub const SLOTS: [i16; TABLE_SIZE] = build_slots(PERFECT_SEED);

const fn build_slots(seed: u32) -> [i16; TABLE_SIZE] {
    let mut t = [0i16; TABLE_SIZE];
    let mut i = 0;
    while i < KEYWORDS.len() {
        let s = (kw_hash(KEYWORDS[i].as_bytes(), seed) as usize) % TABLE_SIZE;
        t[s] = (i + 1) as i16;
        i += 1;
    }
    t
}

// ---- 编译期断言（表损坏 → 编译失败，运行期不存在带病表）----

const _: () = assert!(PERFECT_SEED > 0, "关键字表损坏：完美哈希种子搜索失败");
const _: () = assert!(slots_unique(PERFECT_SEED), "关键字表损坏：槽位冲突");
const _: () = assert!(
    {
        // 全表恰有 KEYWORDS.len() 个非空槽（多一个 = 求值污染，少一个 = 漏登记）。
        let mut n = 0;
        let mut i = 0;
        while i < TABLE_SIZE {
            if SLOTS[i] > 0 {
                n += 1;
            }
            i += 1;
        }
        n == KEYWORDS.len()
    },
    "关键字表损坏：槽位表非空槽数与关键字数不符"
);

/// 关键字查找（O(1) 完美哈希）：命中返回关键字下标。
///
/// 槽位命中后仍须逐字比对——完美哈希只保证关键字互不冲突，
/// 不保证非关键字不落入已占槽位（`if` 与 `ifx` 的槽位可能相同）。
pub fn lookup(word: &str) -> Option<usize> {
    let s = (kw_hash(word.as_bytes(), PERFECT_SEED) as usize) % TABLE_SIZE;
    let idx = SLOTS[s];
    if idx > 0 {
        let i = (idx - 1) as usize;
        if i < KEYWORDS.len() && KEYWORDS[i] == word {
            return Some(i);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 二、版本裁定与历史保留（判据二：版本裁定；判据三：历史保留）
// ---------------------------------------------------------------------------

/// 语言版本（差异表驱动的裁定轴）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LangVersion {
    /// V1 旧版（历史着色器源兼容语境）。
    V1Legacy,
    /// V2 当前版。
    V2Current,
}

impl LangVersion {
    /// 全部版本（差异表覆盖面核对用）。
    pub const ALL: [LangVersion; 2] = [LangVersion::V1Legacy, LangVersion::V2Current];

    /// 版本未知 → 按最新版保守（锚点降级矩阵：版本未知→按最新版保守）。
    pub fn conservative() -> Self {
        LangVersion::V2Current
    }

    /// 人话名（错误提示用）。
    pub fn name(self) -> &'static str {
        match self {
            LangVersion::V1Legacy => "V1Legacy（旧版）",
            LangVersion::V2Current => "V2Current（当前版）",
        }
    }
}

/// 词在某个版本的状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WordStatus {
    /// 该版本的关键字。
    Keyword,
    /// 历史保留：识别但不是关键字（语义按版本裁定）。
    HistoryReserved,
    /// 普通标识符（无特殊语义）。
    Identifier,
}

/// 单词的版本差异（一个词跨版本状态不同的登记条目）。
pub struct VersionDiff {
    /// 词。
    pub word: &'static str,
    /// V1 状态。
    pub v1: WordStatus,
    /// V2 状态。
    pub v2: WordStatus,
}

/// 版本差异表：哪些词在哪个版本是关键字（锚点：版本差异表驱动）。
///
/// 三类登记：
/// 1. 降级词——V1 关键字、V2 历史保留（GLSL 旧管线语义收编的兼容面）；
/// 2. 升级词——V1 标识符、V2 关键字（新语言构造引入）；
/// 3. 双版关键字——两版都是关键字（与当前表交叉核对）。
pub const VERSION_DIFFS: [VersionDiff; 6] = [
    VersionDiff { word: "attribute", v1: WordStatus::Keyword, v2: WordStatus::HistoryReserved },
    VersionDiff { word: "varying", v1: WordStatus::Keyword, v2: WordStatus::HistoryReserved },
    VersionDiff { word: "in", v1: WordStatus::Keyword, v2: WordStatus::HistoryReserved },
    VersionDiff { word: "out", v1: WordStatus::Keyword, v2: WordStatus::HistoryReserved },
    VersionDiff { word: "alias", v1: WordStatus::Identifier, v2: WordStatus::Keyword },
    VersionDiff { word: "requires", v1: WordStatus::Identifier, v2: WordStatus::Keyword },
];

/// 未来保留词（语言演进预留——标识符不得占用）。
pub const FUTURE_RESERVED: [&str; 6] = ["class", "namespace", "virtual", "template", "async", "await"];

/// 保留前缀（命名空间预留：`gl_` 业界惯例、`vx_`/`ve_` 引擎内建域）。
pub const RESERVED_PREFIXES: [&str; 3] = ["gl_", "vx_", "ve_"];

/// 词的完整裁定结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WordClass {
    /// 当前语境（按版本裁定后）的关键字。
    Keyword,
    /// 历史保留字（识别但非关键字）。
    HistoryReserved,
    /// 未来保留字。
    FutureReserved,
    /// 保留前缀命中的标识符。
    ReservedPrefix,
    /// 普通标识符。
    Identifier,
}

/// 裁定（O(1)）：查完美哈希 + 差异表 + 保留域，按版本出结论。
///
/// 版本未知（`None`）按最新版保守裁定（锚点降级矩阵）。
pub fn classify(word: &str, version: Option<LangVersion>) -> WordClass {
    let v = version.unwrap_or_else(LangVersion::conservative);
    // 1. 当前版本关键字表（V2）命中 → 再看差异表是否该版本有不同裁定。
    let kw = lookup(word);
    // 2. 版本差异表（差异词只有 6 条，线性扫描即 O(1) 量级；表驱动扩展点）。
    let diff = VERSION_DIFFS.iter().find(|d| d.word == word);
    if let Some(d) = diff {
        let status = match v {
            LangVersion::V1Legacy => d.v1,
            LangVersion::V2Current => d.v2,
        };
        return match status {
            WordStatus::Keyword => WordClass::Keyword,
            WordStatus::HistoryReserved => WordClass::HistoryReserved,
            WordStatus::Identifier => WordClass::Identifier,
        };
    }
    // 3. 非差异词：V2 表命中即关键字（V1Legacy 同样命中——双版关键字）。
    if kw.is_some() {
        return WordClass::Keyword;
    }
    // 4. 未来保留域。
    if FUTURE_RESERVED.iter().any(|w| *w == word) {
        return WordClass::FutureReserved;
    }
    if RESERVED_PREFIXES.iter().any(|p| word.starts_with(p)) {
        return WordClass::ReservedPrefix;
    }
    WordClass::Identifier
}

/// 保留字误用检查（下游 F0405 标识符区分语义的入口判据）。
///
/// 标识符命名位的词必须是 `Identifier` 类；否则报错带版本裁定与人话提示
/// （锚点：保留字误用→按版本语义报错并提示版本）。
pub fn check_identifier(word: &str, version: Option<LangVersion>) -> Result<(), KeywordError> {
    let v = version.unwrap_or_else(LangVersion::conservative);
    let class = classify(word, Some(v));
    if class == WordClass::Identifier {
        return Ok(());
    }
    let note = match class {
        WordClass::Keyword => format!("「{word}」是 {} 的关键字，不能用作标识符", v.name()),
        WordClass::HistoryReserved => format!(
            "「{word}」是历史保留字（V1Legacy 关键字，{} 已保留）——换名；若确为旧版源请按旧版语义迁移",
            LangVersion::V2Current.name()
        ),
        WordClass::FutureReserved => format!(
            "「{word}」是未来保留字（语言演进预留）——换名",
        ),
        WordClass::ReservedPrefix => format!(
            "「{word}」命中保留前缀（{}）——该前缀域归引擎内建，应用标识符不得占用",
            RESERVED_PREFIXES
                .iter()
                .find(|p| word.starts_with(*p))
                .copied()
                .unwrap_or("?")
        ),
        WordClass::Identifier => unreachable!("Ok 分支已返回"),
    };
    Err(KeywordError::ReservedMisuse {
        word: word.to_string(),
        class,
        version_note: note,
    })
}

/// 关键字管理错误（零静默：裁定 + 版本提示 + 建议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeywordError {
    /// 保留字误用（关键字/历史保留/未来保留/保留前缀）。
    ReservedMisuse {
        /// 词。
        word: String,
        /// 裁定类。
        class: WordClass,
        /// 版本语义提示（人话）。
        version_note: String,
    },
}

impl KeywordError {
    /// 人话呈现（发生了什么 + 怎么修）。
    pub fn describe(&self) -> String {
        match self {
            KeywordError::ReservedMisuse { version_note, .. } => {
                format!("{version_note}。建议：按提示换名或按版本迁移语义")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 三、登记自检（运行期镜像编译期断言——双保险 + 可观测）
// ---------------------------------------------------------------------------

/// 表完整性自检（运行期复算编译期已保证的性质——断言失败即结构缺陷）。
pub fn table_selfcheck() -> Result<(), String> {
    if PERFECT_SEED == 0 {
        return Err("种子为 0：编译期断言本应拦截".to_string());
    }
    let mut occupied = 0;
    for s in SLOTS.iter() {
        if *s > 0 {
            occupied += 1;
        }
    }
    if occupied != KEYWORDS.len() {
        return Err(format!("槽位表 {} 个非空槽 != {} 个关键字", occupied, KEYWORDS.len()));
    }
    for (i, kw) in KEYWORDS.iter().enumerate() {
        match lookup(kw) {
            Some(idx) if idx == i => {}
            other => return Err(format!("关键字 {kw} 查找错位：{other:?} 期望 {}", i)),
        }
    }
    Ok(())
}

/// 纯功能行数自证（正式门禁见 `vec04_checks.rs`）。
pub fn keywords_smoke() -> usize {
    KEYWORDS.len() + VERSION_DIFFS.len() + FUTURE_RESERVED.len() + RESERVED_PREFIXES.len()
}

/// VE-F0404 域自检（判据逐条对应，见 `vec04_checks.rs`）。
pub fn run_vec04_checks() -> CheckSet {
    super::vec04_checks::run_vec04_checks()
}
