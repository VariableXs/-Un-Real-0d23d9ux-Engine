//! VE-F0405 · 标识符规则与规范化（VE-C 域 · 着色器系统 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0405`
//!
//! **判据（锚点原文）**：字符集规则、显性裁定、区分大小写、冲突提示、判据。
//! - 合法字符集与首字符规则；长度上限语义——超长截断还是拒绝**显性裁定**；
//! - 规范化策略：**不折叠大小写**——着色器语言区分大小写的语义显性声明；
//! - 与关键字冲突检测（上游 F0404 关键字表联动；下游 F0422 符号语义衔接）。
//!
//! **设计要点**：
//! - 字符集 = ASCII 字母/数字/下划线，首字符字母或下划线（数字开头、
//!   非 ASCII 一律拒绝，拒绝报错带**字节位置**与修正建议）；
//! - 长度裁定是**策略参数**而不是写死行为：`Reject`（默认，安全）与
//!   `Truncate`（截断但留告知：原名长/截后长/截断点）二选一，显性可配；
//! - 规范化保留大小写（`Foo` 与 `foo` 是两个标识符）——折叠大小写会
//!   把用户语义悄悄改掉，这里以声明 + 断言双面钉死；
//! - 关键字冲突查表 O(1)：256 槽线性探测哈希表（探测步数有上界机检）；
//!   冲突报错带换名建议（追加下划线/加前缀，建议名本身要过规则校验）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规则定义
// ---------------------------------------------------------------------------

/// 长度上限裁定策略（判据"显性裁定"：截断还是拒绝必须是声明出来的选择）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LengthPolicy {
    /// 超长拒绝（默认——静默截断标识符会让符号错位到另一个变量）
    Reject,
    /// 超长截断（必须带告知：原名长/截后长/截断点）
    Truncate,
}

/// 标识符规则集。
#[derive(Clone, Copy, Debug)]
pub struct IdentifierRule {
    /// 长度上限（字节）
    pub max_len: usize,
    pub length_policy: LengthPolicy,
}

impl IdentifierRule {
    /// 默认规则：上限 1024 字节，超长拒绝。
    pub fn default_rule() -> IdentifierRule {
        IdentifierRule {
            max_len: 1024,
            length_policy: LengthPolicy::Reject,
        }
    }
}

/// 标识符校验/规范化的失败（三要素 + 字节位置）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentError {
    pub code: &'static str,
    /// 出错字节位置（0 起；越界类错误为输入长度）
    pub pos: usize,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl IdentError {
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

/// 规范化产物。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedIdent {
    /// 规范化后的文本（按裁定可能被截断；大小写**原样保留**）
    pub text: String,
    /// 原始字节长度
    pub original_len: usize,
    /// 是否发生了截断
    pub truncated: bool,
    /// 告知记录（截断必须留告知；非截断为空）
    pub notices: Vec<String>,
}

// ---------------------------------------------------------------------------
// 二、核心判定与规范化
// ---------------------------------------------------------------------------

/// 单字符合法性（identifier 中间字符）。
pub fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 首字符规则：字母或下划线（数字开头 = 拒绝）。
pub fn is_first_byte(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

/// 校验 + 规范化一条标识符（判据：字符集规则 + 显性裁定 + 区分大小写）。
///
/// 大小写**不折叠**：`normalize("Foo") == normalize("Foo")`，
/// `normalize("Foo") != normalize("foo")`——着色器语言区分大小写是语义，
/// 本函数是这一声明的实现现场。
pub fn normalize(input: &str, rule: &IdentifierRule) -> Result<NormalizedIdent, IdentError> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return Err(IdentError {
            code: "E_IDENT_EMPTY",
            pos: 0,
            what: "标识符为空".to_string(),
            why: "空标识符无法进入符号表".to_string(),
            next: "为该符号起一个以字母或下划线开头的名字".to_string(),
        });
    }
    if !is_first_byte(bytes[0]) {
        let suggestion = if bytes[0].is_ascii_digit() {
            "数字开头非法：在前面加字母或下划线（如 `v`、`_`）".to_string()
        } else {
            "首字符须为 ASCII 字母或下划线".to_string()
        };
        return Err(IdentError {
            code: "E_IDENT_FIRST",
            pos: 0,
            what: format!("标识符首字符 {:?} 非法", bytes[0] as char),
            why: "首字符规则：ASCII 字母或下划线——这是词法与数字字面量的分界线"
                .to_string(),
            next: suggestion,
        });
    }
    for (i, &b) in bytes.iter().enumerate().skip(1) {
        if !is_ident_byte(b) {
            return Err(IdentError {
                code: "E_IDENT_CHAR",
                pos: i,
                what: format!("标识符第 {} 字节的字符 {:?} 非法", i, b as char),
                why: "合法字符集：ASCII 字母/数字/下划线；连字符、Unicode、\
空白都会被词法器切成别的 token，混进来必然错位".to_string(),
                next: format!(
                    "把第 {} 字节的字符换成字母/数字/下划线；\
需要分隔语义用下划线（如 `my_var`）",
                    i
                ),
            });
        }
    }
    let original_len = bytes.len();
    if original_len > rule.max_len {
        match rule.length_policy {
            LengthPolicy::Reject => {
                return Err(IdentError {
                    code: "E_IDENT_TOO_LONG",
                    pos: rule.max_len,
                    what: format!("标识符长 {} 超上限 {}", original_len, rule.max_len),
                    why: "长度裁定策略为 Reject：静默截断会让两个长名在截断点后\
不可区分，符号表必然错位".to_string(),
                    next: format!("缩短到 ≤{} 字节，或显式改用 Truncate 策略并接受告知", rule.max_len),
                });
            }
            LengthPolicy::Truncate => {
                let mut cut = rule.max_len;
                while cut > 0 && !is_ident_byte(bytes[cut - 1]) {
                    cut -= 1;
                }
                let text = input[..cut].to_string();
                let notices = vec![format!(
                    "标识符超长已截断：{} 字节 → {} 字节（截断点 {}）——\
请检查生成的符号是否与预期一致",
                    original_len, cut, cut
                )];
                return Ok(NormalizedIdent {
                    text,
                    original_len,
                    truncated: true,
                    notices,
                });
            }
        }
    }
    Ok(NormalizedIdent {
        text: input.to_string(),
        original_len,
        truncated: false,
        notices: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// 三、关键字冲突检测（判据：冲突提示；F0404 联动点）
// ---------------------------------------------------------------------------

/// 关键字注册表：256 槽线性探测哈希（判据：冲突 O(1) 查表）。
///
/// F0404 联动：本表内容来自关键字表（默认内置 VE-Shade/GLSL 保留字全集，
/// F0404 交付后由其供给权威表——接口 `from_keywords` 已冻结）。
pub struct KeywordRegistry {
    slots: [Option<&'static str>; 256],
    count: usize,
    /// 最坏探测步数（机检 O(1) 的代理指标：有上界即可证不是 O(n)）
    worst_probe: usize,
}

impl KeywordRegistry {
    /// 从关键字清单建表（F0404 的接入点）。
    pub fn from_keywords(keys: &'static [&'static str]) -> KeywordRegistry {
        let mut reg = KeywordRegistry {
            slots: [None; 256],
            count: 0,
            worst_probe: 0,
        };
        for k in keys.iter() {
            reg.insert(k);
        }
        reg
    }

    /// 合并多张关键字清单建表（重复跳过）。
    ///
    /// F0404 联动（锚点点名）：权威源 = GLSL 保留字（转译前端需要）
    /// + VE-Shade 自有关键字（vec04 KEYWORDS）+ vec04 未来保留字。
    pub fn merged(slices: &[&'static [&'static str]]) -> KeywordRegistry {
        let mut reg = KeywordRegistry {
            slots: [None; 256],
            count: 0,
            worst_probe: 0,
        };
        for keys in slices.iter() {
            for k in keys.iter() {
                if !reg.is_keyword(k) {
                    reg.insert(k);
                }
            }
        }
        reg
    }

    fn insert(&mut self, key: &'static str) {
        let mut probe = 0usize;
        let mut idx = fnv1a(key.as_bytes()) as usize % 256;
        while self.slots[idx].is_some() {
            probe += 1;
            idx = (idx + 1) % 256;
        }
        self.slots[idx] = Some(key);
        self.count += 1;
        if probe > self.worst_probe {
            self.worst_probe = probe;
        }
    }

    /// O(1) 查表。
    pub fn is_keyword(&self, ident: &str) -> bool {
        let mut idx = fnv1a(ident.as_bytes()) as usize % 256;
        loop {
            match self.slots[idx] {
                Some(k) if k == ident => return true,
                Some(_) => idx = (idx + 1) % 256,
                None => return false,
            }
        }
    }

    /// 最坏探测步数（>8 即表负载失衡，构建期缺陷）。
    pub fn worst_probe(&self) -> usize {
        self.worst_probe
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

/// FNV-1a 32 位（关键字查表用，与域内 64 位指纹函数分工）。
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 内置 GLSL 保留字基线（转译前端共用；与 vec04 自有关键字表合并后为权威全集）。
pub static BUILTIN_KEYWORDS: &[&str] = &[
    "attribute", "const", "uniform", "varying", "layout", "centroid", "flat",
    "smooth", "noperspective", "patch", "sample", "subroutine", "in", "out",
    "inout", "true", "false", "discard", "return", "if", "else", "switch",
    "case", "default", "break", "continue", "for", "while", "do", "struct",
    "void", "float", "int", "bool", "vec2", "vec3", "vec4", "ivec2", "ivec3",
    "ivec4", "bvec2", "bvec3", "bvec4", "mat2", "mat3", "mat4", "sampler1D",
    "sampler2D", "sampler3D", "samplerCube", "image2D", "buffer", "shared",
    "coherent", "volatile", "restrict", "readonly", "writeonly", "atomic_uint",
    "precision", "highp", "mediump", "lowp", "asm", "class", "union", "enum",
    "typedef", "template", "this", "packed", "goto", "inline", "noinline",
    "public", "static", "extern", "interface", "long", "short", "double",
    "half", "fixed", "unsigned", "superp", "input", "output", "sizeof",
    "namespace", "using", "cast", "hvec2", "hvec3", "hvec4", "sampler2DRect",
    "sampler2DShadow", "sampler1DShadow",
];

/// 默认权威关键字注册表：GLSL 基线 + vec04（F0404）自有关键字与未来保留字。
///
/// 这是"与关键字冲突检测（F0404 联动）"的实现现场：F0404 交付后
/// 本表以其内容为权威源之一，合并去重，O(1) 查表。
pub fn default_registry() -> KeywordRegistry {
    KeywordRegistry::merged(&[
        BUILTIN_KEYWORDS,
        &super::vec04_keywords::KEYWORDS,
        &super::vec04_keywords::FUTURE_RESERVED,
    ])
}

/// 建议名是否撞 F0404 保留前缀（gl_ / vx_ / ve_ 是 vec04 的命名保留域）。
fn hits_reserved_prefix(cand: &str) -> bool {
    super::vec04_keywords::RESERVED_PREFIXES
        .iter()
        .any(|p| cand.starts_with(p))
}

/// 冲突检测结果：冲突时带换名建议（建议名本身过规则校验）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictOutcome {
    pub conflict: bool,
    /// 换名建议（无冲突时为空；建议保证合法且不撞关键字）
    pub suggestions: Vec<String>,
    pub what: String,
    pub next: String,
}

/// 与关键字表冲突检测（判据：冲突→报错提示换名建议）。
///
/// 查表 O(1)；冲突时报错提示换名建议，建议名按序尝试
/// `{name}_`、`vx_{name}`、`{name}_2`，取第一个合法且不撞关键字的。
pub fn check_keyword_conflict(
    ident: &str,
    rule: &IdentifierRule,
    reg: &KeywordRegistry,
) -> Result<ConflictOutcome, IdentError> {
    // 冲突检测前先过词法规则（非法标识符谈不上冲突，先报词法错）
    normalize(ident, rule)?;
    if !reg.is_keyword(ident) {
        return Ok(ConflictOutcome {
            conflict: false,
            suggestions: Vec::new(),
            what: String::new(),
            next: String::new(),
        });
    }
    let mut suggestions: Vec<String> = Vec::new();
    // 候选后缀/组合避开 F0404 保留前缀（vx_ 在 vec04 是命名保留域，不可建议）
    for cand in [
        format!("{}_{}", ident, ""),
        format!("{}_ident", ident),
        format!("{}_2", ident),
    ]
    .iter()
    {
        let cand = cand.trim_end_matches('_').to_string();
        let cand = if cand.ends_with("_ident") || cand.ends_with("_2") {
            cand.clone()
        } else {
            format!("{}_", ident)
        };
        if !hits_reserved_prefix(&cand)
            && normalize(&cand, rule).is_ok()
            && !reg.is_keyword(&cand)
        {
            suggestions.push(cand);
            if suggestions.len() >= 3 {
                break;
            }
        }
    }    let next = format!(
        "换名：{}；关键字（含 GLSL 保留字与 VE-Shade 自有保留字）\
不能作标识符——这是词法器的切分边界",
        suggestions
            .first()
            .map(|s| format!("建议使用 {:?}", s))
            .unwrap_or_else(|| "建议加前缀/后缀".to_string())
    );
    let what = format!(
        "标识符 {:?} 与关键字冲突（关键字表 {} 条，O(1) 查表命中）",
        ident,
        reg.count()
    );
    Ok(ConflictOutcome {
        conflict: true,
        suggestions,
        what,
        next,
    })
}

/// F0405 判据自检（域聚合入口）。
pub fn run_vec05_checks() -> crate::checks::CheckSet {
    super::vec05_checks::run_vec05_checks()
}
