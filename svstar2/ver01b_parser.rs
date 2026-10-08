//! VE-F3402 ·令牌解析器（VE-E 域 · 主题与个性化引擎 · 令牌运行时组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3402`
//!
//! **判据（锚点原文）**：双格式、引用 DAG、循环检测、断链三要素、判据。
//!
//! **职责定位（锚点原文）**：令牌文件解析（JSON/TOML 双格式），引用链解析
//! （令牌引用令牌的 DAG），循环引用检测与断链报错三要素。
//!
//! **数据结构（锚点原文）**：解析器；DAG 构建器。
//!
//! **错误路径与降级矩阵（锚点原文）**：循环→拒绝+定位；断链→三要素报错；
//! 格式错→行号定位。
//!
//! **性能逐项分解（锚点原文）**：O(令牌数)。
//!
//! **跨批对接点（锚点原文）**：E06 覆盖层衔接 —— 本模块交出的 [`TokenSet`]
//! 与 [`TokenDag`] 是覆盖层仲裁的输入面，覆盖层不需要重解源文件。
//!
//! **无障碍与隐私（锚点原文）**：错误定位读屏可达 —— 每条 [`Diag`] 都带
//! [`Site`]（行/列/字节偏移）与一句 `spoken()` 播报文本，字段顺序即朗读
//! 顺序：先位置、再现象、后处置。
//!
//! **本项的边界（不越界施工）**：
//! - 本项拥有**双格式词法/语法解析**与**引用 DAG 的构建与环检测**。这两件
//!   是 F3401 总纲里 [`Piece::Resolve`] 明写"不写"而交给 F3402 的
//!   （见 `ver01_arch::PIECE_SPECS` 的 `not_mine` 字段）；
//! - **不做**级联重算与深度上限（F3403 拥有）、**不做**类型系统与单位校验
//!   （F3405 拥有）、**不做**四级覆盖仲裁（F3406 拥有）、**不做**响应式链
//!   （F3416 拥有）。本项只保证交给它们的**图是可信的**：无环、无断链、
//!   每个节点都有唯一定义点。
//!
//! **设计要点**：
//! - **双格式不是两套半实现**：JSON 与 TOML 各有完整词法与语法，但**产出同一个
//!   [`Val`] 值树**，其后的扁平化、引用抽取、DAG 构建全部共用。同一令牌集合写成
//!   JSON 或 TOML，出来的 [`TokenSet`] 必须逐字段相同（自检里有双格式对拍）。
//! - **格式错→行号定位**：[`Scanner`] 在**字节级**推进的同时按 UTF-8 续接字节
//!   规则推进**字符列**，所以 `{` 落在多字节字符后面时列号仍对得上读屏Expect。
//!   每条 [`Diag`] 都带 [`Site`]，没有"位置未知"的诊断。
//! - **断链三要素**：现象/原因/处置三段**都不许为空**——[`Diag::new`] 在构造
//!   时就把空要素挡在门外（[`E_DIAG_INCOMPLETE`]），杜绝"报错但没给出路"。
//! - **循环→拒绝+定位**：环不是"检测到有环"，而是**把环本身报出来**
//!   （`a → b → c → a`，逐节点附定义点行号），并**拒绝**建图——不给一张含环的图
//!   让下游去猜。环检测用**显式栈的迭代 DFS**（不是递归）：内核栈浅，一个千节点
//!   的令牌链用递归会溢栈。
//! - **O(令牌数) 靠数据结构兑现，不靠口号**：路径查找走 [`PathIndex`]（FNV-1a
//!   + 开放寻址），不是 `Vec::contains` 的线性扫描；出边走 CSR 邻接
//!   （[`TokenDag`] 的 `out_start`/`out_target`），不是 `Vec<Vec<u32>>`。
//!   [`PathIndex::probes`] 公开累计探测步数，性能自检**实测**它随规模的变化，
//!   而不是写一句 `n * CONST` 的自证式算术。
//! - **数字保留原文**：令牌文件里 `1.50` 与 `1.5` 都是作者写下的字面量，解析成
//!   `f64` 再格式化回去就是篡改。数字在值树里存原始文本，需要时由
//!   [`NumLit::as_f64`] 按需转换，转不动返回 `None` 而不是猜一个。
//! - **重复定义是错误不是覆盖**：同一路径写两次就是令牌单源被破坏，报
//!   [`E_DUP_PATH`] 并同时给出两处定义点——单源律的破坏必须在解析期拦住。
//!
//! **零外部依赖**，只依赖 `crate::checks`（自检侧）与 `alloc`。
//! 确定性：零墙钟、零 IO、无随机源，回归可复现。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 解析器版本。词法/语法规则变更走版本号，破坏性变更必须升版并留迁移说明。
pub const PARSER_VERSION: &str = "E01-parser-v1";

/// 嵌套深度上限。**这不是"防御性编程"**——JSON/TOML 解析是递归下降，内核栈浅，
/// 无界嵌套就是栈溢出。超限报错（[`E_DEPTH_LIMIT`]），绝不"解析到哪儿算哪儿"。
pub const MAX_DEPTH: usize = 64;

/// 单个令牌集的令牌数上限。对齐 F3401 的 `MAX_TOKENS`，两处必须同值——解析器
/// 放行 5000 条而注册表拒收，是"解析成功但入册失败"的空转。
pub const MAX_TOKENS: usize = 4096;

/// 路径索引初始槽位（2 的幂，开放寻址要求）。负载因子上限 0.5。
pub const PATH_INDEX_MIN_SLOTS: usize = 1024;

/// 引用占位符的定界符。令牌值里`{color.bg}`这样的花括号块是引用。
pub const REF_OPEN: char = '{';
/// 引用占位符的收定界符。
pub const REF_CLOSE: char = '}';

// ---------------------------------------------------------------------------
// 二、诊断码与诊断（断链三要素的载体）
// ---------------------------------------------------------------------------

/// 诊断码。**枚举判别值不是线上编码值**——自检里断言 `wire()` 与判别值解耦。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagCode {
    /// 诊断三要素里有空段（构造期拦截）。
    IncompleteDiag,
    /// 嵌套超深。
    DepthLimit,
    /// 字节序列不是合法 UTF-8。
    BadUtf8,
    /// 语法错：期待某记号却看到别的。
    Unexpected,
    /// 语法错：字符串/表/数组未闭合。
    Unclosed,
    /// 路径段非法（空段、含分隔符、含花括号）。
    BadSegment,
    /// 路径重复定义。
    DupPath,
    /// 引用目标不存在（断链）。
    BrokenRef,
    /// 引用成环。
    Cycle,
    /// 令牌数超上限。
    TokenLimit,
    /// 字面量无法转成请求的类型。
    BadLiteral,
}

impl DiagCode {
    /// 该诊断的稳定短码，进诊断台账与读屏播报。
    pub fn wire(self) -> &'static str {
        match self {
            DiagCode::IncompleteDiag => "E_DIAG_INCOMPLETE",
            DiagCode::DepthLimit => "E_DEPTH_LIMIT",
            DiagCode::BadUtf8 => "E_BAD_UTF8",
            DiagCode::Unexpected => "E_UNEXPECTED",
            DiagCode::Unclosed => "E_UNCLOSED",
            DiagCode::BadSegment => "E_BAD_SEGMENT",
            DiagCode::DupPath => "E_DUP_PATH",
            DiagCode::BrokenRef => "E_BROKEN_REF",
            DiagCode::Cycle => "E_CYCLE",
            DiagCode::TokenLimit => "E_TOKEN_LIMIT",
            DiagCode::BadLiteral => "E_BAD_LITERAL",
        }
    }

    /// 该码是否代表**阻断**（必须拒绝）。断链三要素与环检测是阻断项，
    /// 格式错也是——半张图比没有图更危险。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            DiagCode::IncompleteDiag
                | DiagCode::DepthLimit
                | DiagCode::BadUtf8
                | DiagCode::Unexpected
                | DiagCode::Unclosed
                | DiagCode::BadSegment
                | DiagCode::DupPath
                | DiagCode::BrokenRef
                | DiagCode::Cycle
                | DiagCode::TokenLimit
        )
    }
}

/// 源内位置。**字节偏移 + 字符列**双轨：字节偏移用于切片，字符列用于读屏。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Site {
    /// 1 起算的行号。1 起算是给人看的，0起算会被读屏念成"第 0 行"。
    pub line: u32,
    /// 1 起算的字符列。
    pub col: u32,
    /// 0 起算的字节偏移。
    pub byte: u32,
}

impl Site {
    /// 文件起点的位置。
    pub const fn start() -> Site {
        Site {
            line: 1,
            col: 1,
            byte: 0,
        }
    }
}

/// 断链/语法错诊断。**三要素缺一不可**。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diag {
    /// 诊断码。
    pub code: DiagCode,
    /// 位置（读屏第一句就念它）。
    pub site: Site,
    /// 现象：发生了什么。
    pub what: String,
    /// 原因：为什么发生。
    pub why: String,
    /// 处置：读者下一步该做什么。
    pub fix: String,
}

impl Diag {
    /// 构造诊断。**任一要素为空则降级为 [`E_DIAG_INCOMPLETE`]**——
    /// "报错但没给出路"的诊断比不报更坏，因为它让人以为已经处理过了。
    pub fn new(
        code: DiagCode,
        site: Site,
        what: &str,
        why: &str,
        fix: &str,
    ) -> Diag {
        if what.is_empty() || why.is_empty() || fix.is_empty() {
            // 不把残缺的原始内容带出去：残缺本身就是需要报的事实。
            return Diag {
                code: DiagCode::IncompleteDiag,
                site,
                what: "诊断三要素不完整".to_string(),
                why: if what.is_empty() && why.is_empty() && fix.is_empty() {
                    "三要素全空：调用方没有给出任何可播报的信息".to_string()
                } else if what.is_empty() {
                    "现象段为空：调用方没说明发生了什么".to_string()
                } else if why.is_empty() {
                    "原因段为空：调用方没说明为什么发生".to_string()
                } else {
                    "处置段为空：调用方没说明读者下一步该做什么".to_string()
                },
                fix: "补齐缺失的那一段后重试；解析器不接受残缺诊断".to_string(),
            };
        }
        Diag {
            code,
            site,
            what: what.to_string(),
            why: why.to_string(),
            fix: fix.to_string(),
        }
    }

    /// 断链三要素是否齐备。
    pub fn complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.fix.is_empty()
    }

    /// 读屏播报文本：**先位置、再现象、后原因与处置**，字段顺序即朗读顺序。
    /// 这是无障碍面的唯一出口——不依赖颜色、不依赖缩进、不依赖终端宽度。
    pub fn spoken(&self) -> String {
        format!(
            "{}第 {} 行第 {} 列（字节 {}）：{}。原因：{}。处置：{}",
            self.code.wire(),
            self.site.line,
            self.site.col,
            self.site.byte,
            self.what,
            self.why,
            self.fix
        )
    }
}

// ---------------------------------------------------------------------------
// 三、值树（双格式的共同产物）
// ---------------------------------------------------------------------------

/// 源格式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceFormat {
    /// JSON。
    Json,
    /// TOML。
    Toml,
}

impl SourceFormat {
    /// 该格式的规范名（进 `TokenSet` 供对拍用）。
    pub fn name(self) -> &'static str {
        match self {
            SourceFormat::Json => "json",
            SourceFormat::Toml => "toml",
        }
    }
}

/// 带位置的数字字面量。**存原始文本**，不在解析期做浮点往返。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NumLit {
    /// 作者写下的字面量原文（`1.50` 不会被改写成 `1.5`）。
    pub raw: String,
    /// 位置。
    pub site: Site,
}

impl NumLit {
    /// 按需转`f64`。转不动（溢出/非十进制）返回 `None`，**不猜一个值**。
    pub fn as_f64(&self) -> Option<f64> {
        // TOML 允许 `_` 分隔数字，JSON 不允许。两种都在这里归一，
        // 因为读到的 raw 属于对应格式的合法子集。
        let cleaned: String = self.raw.chars().filter(|c| *c != '_').collect();
        cleaned.parse::<f64>().ok().filter(|v| v.is_finite())
    }

    /// 按需转 `i64`。
    pub fn as_i64(&self) -> Option<i64> {
        let cleaned: String = self.raw.chars().filter(|c| *c != '_').collect();
        cleaned.parse::<i64>().ok()
    }
}

/// 带位置的值。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Val {
    /// 字符串。
    Str(String),
    /// 数字（保留原文）。
    Num(NumLit),
    /// 布尔。
    Bool(bool),
    /// 数组。
    Arr(Vec<Val>),
    /// 对象/表。键保持文件里的书写顺序——顺序变了就是另一份令牌集。
    Table(Vec<(String, Val)>),
}

impl Val {
    /// 按需取字符串。**不是字符串就返回 `None`**，不静默类型转换。
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、路径索引（FNV-1a + 开放寻址，把 O(令牌数) 兑现成数据结构）
// ---------------------------------------------------------------------------

/// 路径 → 节点号索引。**开放寻址**（线性探测），负载因子上限 0.5。
///
/// 为什么不用 `Vec<(String, u32)>` 线性扫描：断链检查要为每个引用查一次目标，
/// n 个令牌 × m 条引用就是 O(n·m)。线性扫描让它变成 O(n²)，令牌一多就超时。
#[derive(Clone, Debug)]
pub struct PathIndex {
    /// 槽位 → 存的是 `keys[slot-1]` 的下标+1（0 表示空槽）。
    slots: Vec<u32>,
    /// 与 `slots` 平行的键表。
    keys: Vec<String>,
    /// 累计探测步数。**公开出来是为了让性能自检实测真实工作量**，
    /// 而不是写一句 `n * CONST` 的自证式算术。
    pub probes: u64,
}

impl PathIndex {
    /// 建一个空索引，槽位数为 2 的幂且不小于 [`PATH_INDEX_MIN_SLOTS`]。
    pub fn new() -> PathIndex {
        let mut slots = Vec::new();
        slots.resize(PATH_INDEX_MIN_SLOTS, 0u32);
        PathIndex {
            slots,
            keys: Vec::new(),
            probes: 0,
        }
    }

    /// 槽位掩码（容量是 2 的幂）。
    fn mask(&self) -> usize {
        self.slots.len() - 1
    }

    /// FNV-1a 32 位。逐字节混入，取低 32 位。
    pub fn hash(path: &str) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        for b in path.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 容量翻倍并**全部重插**。开放寻址在满载后退化成线性扫描，
    /// 所以扩容是正确性前提，不是性能优化。
    ///
    /// **重插只搬槽位、不动键表**：键表的下标就是节点号，扩容时若把键再
    /// `push` 一遍，键表会随扩容次数膨胀，下标与节点号就对不上了。
    fn grow(&mut self) {
        let new_len = self.slots.len() * 2;
        let mut slots = Vec::new();
        slots.resize(new_len, 0u32);
        let old_slots = core::mem::replace(&mut self.slots, slots);
        for s in old_slots.iter() {
            if *s == 0 {
                continue;
            }
            self.place(*s - 1);
        }
    }

    /// 把 `keys[ki]` 放到它该在的槽位。调用方保证槽位里有空位。
    ///
    /// **只写槽位，绝不碰键表**——这是扩容正确性的关键。
    fn place(&mut self, ki: u32) {
        let key = match self.keys.get(ki as usize) {
            Some(k) => k.clone(),
            None => return,
        };
        let mut idx = (PathIndex::hash(&key) as usize) & self.mask();
        loop {
            self.probes += 1;
            if self.slots[idx] == 0 {
                self.slots[idx] = ki + 1;
                return;
            }
            idx = (idx + 1) & self.mask();
        }
    }

    /// 插入一个路径 → 节点号。**同路径重复插入返回 `false`**
    /// （重复定义是错误，由调用方报 [`E_DUP_PATH`]，不在这里悄悄覆盖）。
    pub fn insert(&mut self, path: &str, node: u32) -> bool {
        if (self.keys.len() + 1) * 2 > self.slots.len() {
            self.grow();
        }
        let mut idx = (PathIndex::hash(path) as usize) & self.mask();
        loop {
            self.probes += 1;
            let s = self.slots[idx];
            if s == 0 {
                let ki = self.keys.len() as u32;
                self.keys.push(path.to_string());
                self.slots[idx] = ki + 1;
                let _ = node;
                return true;
            }
            let ki = (s - 1) as usize;
            if let Some(k) = self.keys.get(ki) {
                if k.as_str() == path {
                    return false;
                }
            }
            idx = (idx + 1) & self.mask();
        }
    }

    /// 查一个路径的节点号。
    pub fn get(&self, path: &str) -> Option<u32> {
        let mut idx = (PathIndex::hash(path) as usize) & self.mask();
        loop {
            let s = self.slots[idx];
            if s == 0 {
                return None;
            }
            let ki = (s - 1) as usize;
            if let Some(k) = self.keys.get(ki) {
                if k.as_str() == path {
                    return Some(ki as u32);
                }
            }
            idx = (idx + 1) & self.mask();
        }
    }

    /// 已插入的路径数。
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// 第 `i` 个插入的路径（按插入顺序）。
    pub fn key_at(&self, i: usize) -> Option<&str> {
        self.keys.get(i).map(|s| s.as_str())
    }
}

impl Default for PathIndex {
    fn default() -> Self {
        PathIndex::new()
    }
}// ---------------------------------------------------------------------------
// 五、扫描器（字节级推进，字符列按 UTF-8 续接规则推进）
// ---------------------------------------------------------------------------

/// 词法扫描器。**格式错→行号定位**的定位能力全在这里。
///
/// `col` 记的是**字符列**而不是字节列：推进到多字节字符的续接字节时不加列，
/// 只有起始字节才加。这么做的理由是读屏按字符念"第 7 列"，不按字节念。
pub struct Scanner<'a> {
    /// 源字节。
    src: &'a [u8],
    /// 当前字节下标。
    pub pos: usize,
    /// 当前行（1 起算）。
    pub line: u32,
    /// 当前列（1 起算，字符列）。
    pub col: u32,
}

impl<'a> Scanner<'a> {
    /// 从源字节建扫描器。
    pub fn new(src: &'a [u8]) -> Scanner<'a> {
        Scanner {
            src,
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    /// 窥视下一个字节。
    pub fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    /// 窥视第 `n` 个字节后的字节（不推进）。
    pub fn peek_at(&self, n: usize) -> Option<u8> {
        self.src.get(self.pos + n).copied()
    }

    /// 当前位置。
    pub fn site(&self) -> Site {
        Site {
            line: self.line,
            col: self.col,
            byte: self.pos as u32,
        }
    }

    /// 是否已到末尾。
    pub fn eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    /// 取一个字节并推进。列的推进按 **UTF-8 续接字节规则**：
    /// `b & 0xC0 != 0x80` 才是字符起始，起始才加列；换行另起一行。
    pub fn bump(&mut self) -> Option<u8> {
        let b = self.src.get(self.pos).copied()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else if b & 0xC0 != 0x80 {
            // 起始字节（含 ASCII）才推进列；续接字节（10xxxxxx）不推。
            self.col += 1;
        }
        Some(b)
    }

    /// 跳过空格与制表（不跳换行——换行要计入行号定位）。
    pub fn skip_spaces(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\r' {
                self.bump();
            } else {
                break;
            }
        }
    }

    /// 跳过空白，**含换行**。只在值之间的分隔处用。
    pub fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
                self.bump();
            } else {
                break;
            }
        }
    }

    /// 跳到下一个有内容的行首（用于 TOML 的行首判定）。
    pub fn skip_to_content(&mut self) {
        loop {
            self.skip_ws();
            if self.peek() == Some(b'#') {
                // TOML 行注释：吃掉到行尾（不含换行），让下一次 site()
                // 仍停在正确的行首。
                while let Some(b) = self.peek() {
                    if b == b'\n' {
                        break;
                    }
                    self.bump();
                }
            } else {
                return;
            }
        }
    }

    /// 在当前位置截一个诊断（三要素齐备）。
    pub fn diag(&self, code: DiagCode, what: &str, why: &str, fix: &str) -> Diag {
        Diag::new(code, self.site(), what, why, fix)
    }

    /// 期待一个字节，是则吃掉。
    pub fn expect(&mut self, want: u8, ctx: &str) -> Result<(), Diag> {
        match self.peek() {
            Some(b) if b == want => {
                self.bump();
                Ok(())
            }
            Some(b) => Err(self.diag(
                DiagCode::Unexpected,
                &format!("这里期待 {:?}，实际看到 {:?}", want as char, b as char),
                ctx,
                "检查是否漏了分隔符，或把两种格式混写了",
            )),
            None => Err(self.diag(
                DiagCode::Unclosed,
                &format!("期待 {:?} 但源文件到此结束", want as char),
                ctx,
                "补齐缺失的记号，或确认这段是不是被截断了",
            )),
        }
    }
}

/// 判断一个字节是不是 TOML 的裸键字符。
pub fn toml_bare(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// UTF-8 序列长度：首字节高位有多少个 1。
pub fn utf8_seq_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

// ---------------------------------------------------------------------------
// 六、JSON 解析（完整词法 + 语法）
// ---------------------------------------------------------------------------

/// 解析 JSON 文本为值树。
pub fn parse_json(src: &str) -> Result<Val, Diag> {
    let mut s = Scanner::new(src.as_bytes());
    let v = json_value(&mut s, 0)?;
    s.skip_ws();
    if !s.eof() {
        return Err(s.diag(
            DiagCode::Unexpected,
            "顶层值之后还有内容",
            "一个 JSON 文本只允许一个顶层值",
            "删去多余内容；若本意是多份令牌，请拆成多个文件或用数组包起来",
        ));
    }
    Ok(v)
}

/// 解析一个 JSON 值。
fn json_value(s: &mut Scanner, depth: usize) -> Result<Val, Diag> {
    if depth > MAX_DEPTH {
        return Err(s.diag(
            DiagCode::DepthLimit,
            &format!("嵌套深度超过上限 {}", MAX_DEPTH),
            "JSON 解析是递归下降，深度无界就是栈溢出",
            "把深层结构拆平：令牌集本来就应该是一层扁平的路径到值的映射",
        ));
    }
    s.skip_ws();
    let site = s.site();
    match s.peek() {
        None => Err(s.diag(
            DiagCode::Unclosed,
            "期待一个值，但源文件到此结束",
            "JSON 文本在这里是不完整的",
            "补上缺失的值，或确认这段是不是被截断了",
        )),
        Some(b'{') => json_object(s, depth, site),
        Some(b'[') => json_array(s, depth, site),
        Some(b'"') => Ok(Val::Str(json_string(s)?)),
        Some(b't') => {
            json_lit(s, "true")?;
            Ok(Val::Bool(true))
        }
        Some(b'f') => {
            json_lit(s, "false")?;
            Ok(Val::Bool(false))
        }
        Some(b'n') => {
            json_lit(s, "null")?;
            // JSON 有 null，令牌集里没有 null 语义——映射成空串会让"作者写了
            // null"和"作者写了空串"在下游无法区分。显式报错更诚实。
            Err(Diag::new(
                DiagCode::BadLiteral,
                site,
                "令牌值不能是 null",
                "null 与空字符串在求值后无法区分，引用它会拿到歧义值",
                "改写成具体值；若本意是留空，写空字符串",
            ))
        }
        Some(b'-') | Some(b'0'..=b'9') => json_number(s, site),
        Some(b) => Err(s.diag(
            DiagCode::Unexpected,
            &format!("这里期待一个值，实际看到 {:?}", b as char),
            "JSON 的值只能是对象、数组、字符串、数字、true、false、null",
            "检查是否漏了引号，或把 TOML 的写法放进了 JSON 文件",
        )),
    }
}

/// 匹配一个字面量关键字。
fn json_lit(s: &mut Scanner, want: &str) -> Result<(), Diag> {
    let bytes = want.as_bytes();
    for (i, wb) in bytes.iter().enumerate() {
        match s.peek_at(i) {
            Some(b) if b == *wb => {}
            Some(_) => {
                return Err(s.diag(
                    DiagCode::Unexpected,
                    &format!("关键字 {} 写错了，第 {} 个字节不符", want, i + 1),
                    "JSON 的 true、false、null 必须全小写",
                    "按 JSON 规范改成全小写",
                ))
            }
            None => {
                return Err(s.diag(
                    DiagCode::Unclosed,
                    &format!("关键字 {} 在源文件中途结束", want),
                    "文本被截断",
                    "补齐关键字",
                ))
            }
        }
    }
    for _ in 0..bytes.len() {
        s.bump();
    }
    Ok(())
}

/// 解析 JSON 数字，**保留原始文本**。
fn json_number(s: &mut Scanner, site: Site) -> Result<Val, Diag> {
    let start = s.pos;
    if s.peek() == Some(b'-') {
        s.bump();
    }
    match s.peek() {
        Some(b'0') => {
            s.bump();
            if let Some(n) = s.peek() {
                if n.is_ascii_digit() {
                    return Err(s.diag(
                        DiagCode::BadLiteral,
                        "JSON 数字有前导零",
                        "JSON 规范不允许 01 这类写法",
                        "去掉前导零",
                    ));
                }
            }
        }
        Some(b) if b.is_ascii_digit() => {
            while let Some(n) = s.peek() {
                if n.is_ascii_digit() {
                    s.bump();
                } else {
                    break;
                }
            }
        }
        _ => {
            return Err(s.diag(
                DiagCode::BadLiteral,
                "负号后面没有数字",
                "JSON 数字的整数位不能为空",
                "补上数字，或删掉这个负号",
            ))
        }
    }
    if s.peek() == Some(b'.') {
        s.bump();
        if !matches!(s.peek(), Some(n) if n.is_ascii_digit()) {
            return Err(s.diag(
                DiagCode::BadLiteral,
                "小数点后没有数字",
                "JSON 要求小数点后至少一位数字",
                "补上数字，或删掉这个小数点",
            ));
        }
        while let Some(n) = s.peek() {
            if n.is_ascii_digit() {
                s.bump();
            } else {
                break;
            }
        }
    }
    if matches!(s.peek(), Some(b'e') | Some(b'E')) {
        s.bump();
        if matches!(s.peek(), Some(b'+') | Some(b'-')) {
            s.bump();
        }
        if !matches!(s.peek(), Some(n) if n.is_ascii_digit()) {
            return Err(s.diag(
                DiagCode::BadLiteral,
                "指数部分没有数字",
                "JSON 要求 e 或 E 之后至少一位数字",
                "补上指数数字，或删掉这个指数部分",
            ));
        }
        while let Some(n) = s.peek() {
            if n.is_ascii_digit() {
                s.bump();
            } else {
                break;
            }
        }
    }
    let raw = match core::str::from_utf8(&s.src[start..s.pos]) {
        Ok(t) => t.to_string(),
        Err(_) => {
            return Err(s.diag(
                DiagCode::BadUtf8,
                "数字字面量不是合法 UTF-8",
                "源文件被截断在多字节字符中间",
                "检查文件编码与是否被截断",
            ))
        }
    };
    Ok(Val::Num(NumLit { raw, site }))
}

/// 解析 JSON 字符串（含全部转义与 Unicode 代理对）。
fn json_string(s: &mut Scanner) -> Result<String, Diag> {
    s.expect(b'"', "这里是字符串的起始引号")?;
    let mut out = String::new();
    loop {
        let b = match s.peek() {
            Some(b) => b,
            None => {
                return Err(s.diag(
                    DiagCode::Unclosed,
                    "字符串没有闭合引号",
                    "JSON 字符串必须以双引号结束",
                    "补上闭合引号；若本意是多行文本，改用 TOML 的多行字面串",
                ))
            }
        };
        match b {
            b'"' => {
                s.bump();
                return Ok(out);
            }
            b'\\' => {
                s.bump();
                let e = match s.peek() {
                    Some(e) => e,
                    None => {
                        return Err(s.diag(
                            DiagCode::Unclosed,
                            "转义符后面没有内容",
                            "反斜杠必须跟一个转义字符",
                            "补上转义字符，或删掉这个悬空的反斜杠",
                        ))
                    }
                };
                s.bump();
                match e {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let hi = json_hex4(s)?;
                        if (0xD800..0xDC00).contains(&hi) {
                            // 高代理：必须紧跟一个低代理，否则是非法对。
                            if s.peek() != Some(b'\\') || s.peek_at(1) != Some(b'u') {
                                return Err(s.diag(
                                    DiagCode::BadLiteral,
                                    "高代理项后面没有低代理项",
                                    "高代理必须与低代理成对出现",
                                    "补上低代理项，或改用直接写入的字符",
                                ));
                            }
                            s.bump();
                            s.bump();
                            let lo = json_hex4(s)?;
                            if !(0xDC00..0xE000).contains(&lo) {
                                return Err(s.diag(
                                    DiagCode::BadLiteral,
                                    "代理对的低位不是低代理项",
                                    "低代理的取值范围是固定的",
                                    "改成合法的低代理项",
                                ));
                            }
                            let cp = 0x10000u32 + (((hi - 0xD800) as u32) << 10)
                                + (lo - 0xDC00) as u32;
                            match char::from_u32(cp) {
                                Some(c) => out.push(c),
                                None => {
                                    return Err(s.diag(
                                        DiagCode::BadLiteral,
                                        "代理对算出的码位不是有效字符",
                                        "该组合落在 Unicode 的无效区间",
                                        "改成一个有效的码位",
                                    ))
                                }
                            }
                        } else if (0xDC00..0xE000).contains(&hi) {
                            return Err(s.diag(
                                DiagCode::BadLiteral,
                                "出现了没有高代理项的低代理项",
                                "低代理只能作为代理对的低半出现",
                                "补上高代理项，或改成一个普通字符",
                            ));
                        } else {
                            match char::from_u32(hi as u32) {
                                Some(c) => out.push(c),
                                None => {
                                    return Err(s.diag(
                                        DiagCode::BadLiteral,
                                        "Unicode 转义不是一个有效字符",
                                        "该码位在 Unicode 中未分配",
                                        "换一个有效的码位",
                                    ))
                                }
                            }
                        }
                    }
                    _ => {
                        return Err(s.diag(
                            DiagCode::Unexpected,
                            &format!("未知的转义字符 {:?}", e as char),
                            "JSON 只认双引号、反斜杠、斜杠、b、f、n、r、t、u 这几个转义",
                            "改用合法转义，或去掉这个反斜杠",
                        ))
                    }
                }
            }
            b'\n' => {
                return Err(s.diag(
                    DiagCode::Unexpected,
                    "字符串里出现了裸换行",
                    "JSON 字符串不允许字面换行",
                    "写成换行转义；若本意是多行文本，改用 TOML 的多行字面串",
                ))
            }
            b if b < 0x20 => {
                return Err(s.diag(
                    DiagCode::Unexpected,
                    "字符串里有未加转义的控制字符",
                    "JSON 要求控制字符一律转义",
                    "写成对应的转义序列",
                ))
            }
            _ => {
                // 普通字节：按 UTF-8 续接规则整段搬进结果。
                let start = s.pos;
                let n = utf8_seq_len(b);
                for _ in 0..n {
                    if s.bump().is_none() {
                        break;
                    }
                }
                match core::str::from_utf8(&s.src[start..s.pos]) {
                    Ok(t) => out.push_str(t),
                    Err(_) => {
                        return Err(s.diag(
                            DiagCode::BadUtf8,
                            "字符串里有非法 UTF-8 字节序列",
                            "源文件不是合法 UTF-8",
                            "把文件转成 UTF-8 后重试",
                        ))
                    }
                }
            }
        }
    }
}

/// 读 4 位十六进制。
fn json_hex4(s: &mut Scanner) -> Result<u16, Diag> {
    let mut v: u16 = 0;
    for _ in 0..4 {
        let b = match s.peek() {
            Some(b) => b,
            None => {
                return Err(s.diag(
                    DiagCode::Unclosed,
                    "Unicode 转义在源文件中途结束",
                    "转义后面必须正好跟 4 位十六进制",
                    "补齐 4 位十六进制",
                ))
            }
        };
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => {
                return Err(s.diag(
                    DiagCode::Unexpected,
                    "Unicode 转义里出现了非十六进制字符",
                    "转义后面必须正好跟 4 位十六进制",
                    "改成 0-9 或 a-f",
                ))
            }
        };
        s.bump();
        v = (v << 4) | d as u16;
    }
    Ok(v)
}

/// 解析 JSON 对象。
fn json_object(s: &mut Scanner, depth: usize, open: Site) -> Result<Val, Diag> {
    s.expect(b'{', "这里是对象的起始花括号")?;
    let mut out: Vec<(String, Val)> = Vec::new();
    s.skip_ws();
    if s.peek() == Some(b'}') {
        s.bump();
        return Ok(Val::Table(out));
    }
    loop {
        s.skip_ws();
        if s.peek() != Some(b'"') {
            return Err(s.diag(
                DiagCode::Unexpected,
                "对象的键必须是字符串",
                "JSON 要求每个键都用双引号包起来",
                "给键加双引号；或若本意是 TOML，改写为裸键",
            ));
        }
        let key = json_string(s)?;
        s.skip_ws();
        s.expect(b':', "键与值之间缺少冒号")?;
        let v = json_value(s, depth + 1)?;
        out.push((key, v));
        s.skip_ws();
        match s.peek() {
            Some(b',') => {
                s.bump();
                s.skip_ws();
                if s.peek() == Some(b'}') {
                    return Err(s.diag(
                        DiagCode::Unexpected,
                        "对象里出现了悬空的逗号",
                        "JSON 不允许尾随逗号",
                        "删掉这个逗号",
                    ));
                }
            }
            Some(b'}') => {
                s.bump();
                return Ok(Val::Table(out));
            }
            Some(b) => {
                return Err(s.diag(
                    DiagCode::Unexpected,
                    &format!("对象成员之间期待逗号或右花括号，实际看到 {:?}", b as char),
                    "成员分隔符缺失",
                    "补上逗号，或删掉多余字符",
                ))
            }
            None => {
                return Err(s.diag(
                    DiagCode::Unclosed,
                    "对象没有闭合花括号",
                    &format!(
                        "对象在第 {} 行第 {} 列被打开，但源文件先结束了",
                        open.line, open.col
                    ),
                    "补上闭合花括号",
                ))
            }
        }
    }
}

/// 解析 JSON 数组。
fn json_array(s: &mut Scanner, depth: usize, open: Site) -> Result<Val, Diag> {
    s.expect(b'[', "这里是数组的起始方括号")?;
    let mut out: Vec<Val> = Vec::new();
    s.skip_ws();
    if s.peek() == Some(b']') {
        s.bump();
        return Ok(Val::Arr(out));
    }
    loop {
        let v = json_value(s, depth + 1)?;
        out.push(v);
        s.skip_ws();
        match s.peek() {
            Some(b',') => {
                s.bump();
                s.skip_ws();
                if s.peek() == Some(b']') {
                    return Err(s.diag(
                        DiagCode::Unexpected,
                        "数组里出现了悬空的逗号",
                        "JSON 不允许尾随逗号",
                        "删掉这个逗号",
                    ));
                }
            }
            Some(b']') => {
                s.bump();
                return Ok(Val::Arr(out));
            }
            Some(b) => {
                return Err(s.diag(
                    DiagCode::Unexpected,
                    &format!("数组成员之间期待逗号或右方括号，实际看到 {:?}", b as char),
                    "成员分隔符缺失",
                    "补上逗号，或删掉多余字符",
                ))
            }
            None => {
                return Err(s.diag(
                    DiagCode::Unclosed,
                    "数组没有闭合方括号",
                    &format!(
                        "数组在第 {} 行第 {} 列被打开，但源文件先结束了",
                        open.line, open.col
                    ),
                    "补上闭合方括号",
                ))
            }
        }
    }
}// ---------------------------------------------------------------------------
// 七、TOML 解析（完整词法 + 语法，产出同一个 Val 值树）
// ---------------------------------------------------------------------------

/// 解析 TOML 文本为值树。**表头决定路径前缀**，所以两种格式最终形状一致。
pub fn parse_toml(src: &str) -> Result<Val, Diag> {
    let mut s = Scanner::new(src.as_bytes());
    let mut root: Vec<(String, Val)> = Vec::new();
    // 当前表路径（表头累积）。空表示还在根表。
    let mut prefix: Vec<String> = Vec::new();

    loop {
        s.skip_to_content();
        if s.eof() {
            break;
        }
        // 表头判定必须在行首，否则 `key = "x"` 里的 `[` 会被误当表头。
        if s.peek() == Some(b'[') {
            s.bump();
            let is_array = s.peek() == Some(b'[');
            if is_array {
                // 数组表 `[[x]]`：本解析器**显式拒绝**而不是半支持。
                return Err(s.diag(
                    DiagCode::Unexpected,
                    "不支持数组表",
                    "令牌集要求每个路径有唯一定义点，数组表会让同一路径有多个下标来源",
                    "改用普通表；若本意是表达一组值，写成显式的不同路径",
                ));
            }
            let path = toml_key_path(&mut s)?;
            s.skip_spaces();
            s.expect(b']', "表头缺少右方括号")?;
            // 表头里的重复段在 key_path 内已拒。
            prefix = path;
            // 表头本身就是一次定义：空表（后面没键）也必须出现在值树里，
            // 否则 `color = {}` 与 `color` 不写这两种情况在下游无法区分。
            toml_ensure_table(&mut root, &prefix)?;
            continue;
        }

        // 普通键值对。
        let keypath = toml_key_path(&mut s)?;
        s.skip_spaces();
        s.expect(b'=', "键与值之间缺少等号")?;
        s.skip_spaces();
        let v = toml_value(&mut s, 0)?;
        let mut full: Vec<String> = prefix.clone();
        full.extend(keypath);
        if full.is_empty() {
            return Err(s.diag(
                DiagCode::BadSegment,
                "键路径为空",
                "空路径无法定位令牌",
                "给这一行写一个键名",
            ));
        }
        toml_put(&mut root, &full, v)?;
    }

    Ok(Val::Table(root))
}

/// 按路径建出表节点（表头用）。
///
/// **不用 `mem::replace` 做借用 gymnastics**——递归下降天然避开别名问题：
/// 每一层只在自己的那一个子表上操作，不同时持有父子两个可变借用。
fn toml_ensure_table(root: &mut Vec<(String, Val)>, path: &[String]) -> Result<(), Diag> {
    if path.is_empty() {
        return Ok(());
    }
    let seg = &path[0];
    let pos = match root.iter().position(|(k, _)| k == seg) {
        Some(i) => i,
        None => {
            root.push((seg.clone(), Val::Table(Vec::new())));
            root.len() - 1
        }
    };
    let is_table = matches!(&root[pos].1, Val::Table(_));
    if !is_table {
        // 路径前缀撞上标量：这是定义冲突，不能默默覆盖。
        let raw_seg = seg.clone();
        return Err(Diag::new(
            DiagCode::DupPath,
            Site::start(),
            &format!("路径段 {:?} 已经是标量，不能再当表用", raw_seg),
            "同一段路径既是标量又是表，说明两处定义互相矛盾",
            "把其中一处改名，或把标量移进另一个路径段",
        ));
    }
    match &mut root[pos].1 {
        Val::Table(sub) => toml_ensure_table(sub, &path[1..]),
        _ => Ok(()),
    }
}

/// 按路径写入一个叶子值。
fn toml_put(root: &mut Vec<(String, Val)>, path: &[String], v: Val) -> Result<(), Diag> {
    if path.len() == 1 {
        let seg = &path[0];
        if root.iter().any(|(k, _)| k == seg) {
            return Err(Diag::new(
                DiagCode::DupPath,
                Site::start(),
                &format!("路径 {:?} 被定义了两次", path.join(".")),
                "同一个键在同一个表里出现了两次，或同名表头与键值对指向同一路径",
                "删掉其中一处；令牌单源律要求每个路径只有一个定义点",
            ));
        }
        root.push((seg.clone(), v));
        return Ok(());
    }
    // 先把父表建出来（缺哪层补哪层），再逐段下行把叶子挂到**最深前缀**上。
    toml_ensure_table(root, &path[..path.len() - 1])?;
    let mut cur: &mut Vec<(String, Val)> = root;
    for seg in path[..path.len() - 1].iter() {
        let pos = match cur.iter().position(|(k, _)| k == seg) {
            Some(p) => p,
            None => {
                return Err(Diag::new(
                    DiagCode::BadSegment,
                    Site::start(),
                    &format!("路径 {:?} 的父表不存在", path.join(".")),
                    "建表时中途遇到缺失的父节点",
                    "补上缺失的表头",
                ))
            }
        };
        match &mut cur[pos].1 {
            Val::Table(t) => cur = t,
            _ => {
                return Err(Diag::new(
                    DiagCode::DupPath,
                    Site::start(),
                    &format!("路径 {:?} 的父节点是标量，不能再放子键", path.join(".")),
                    "同一段路径既是标量又是表",
                    "把子键挪到别的路径段下",
                ))
            }
        }
    }
    let last = &path[path.len() - 1];
    if cur.iter().any(|(k, _)| k == last) {
        return Err(Diag::new(
            DiagCode::DupPath,
            Site::start(),
            &format!("路径 {:?} 被定义了两次", path.join(".")),
            "同一个键在同一个表里出现了两次",
            "删掉其中一处；令牌单源律要求每个路径只有一个定义点",
        ));
    }
    cur.push((last.clone(), v));
    Ok(())
}

/// 读一个键路径（点分，支持引号段）。
fn toml_key_path(s: &mut Scanner) -> Result<Vec<String>, Diag> {
    let mut out: Vec<String> = Vec::new();
    loop {
        s.skip_spaces();
        let seg = match s.peek() {
            Some(b'"') => toml_basic_string(s)?,
            Some(b'\'') => toml_literal_string(s)?,
            Some(b) if toml_bare(b) => {
                let start = s.pos;
                while let Some(c) = s.peek() {
                    if toml_bare(c) {
                        s.bump();
                    } else {
                        break;
                    }
                }
                match core::str::from_utf8(&s.src[start..s.pos]) {
                    Ok(t) => t.to_string(),
                    Err(_) => {
                        return Err(s.diag(
                            DiagCode::BadUtf8,
                            "裸键里有非法 UTF-8 字节",
                            "源文件不是合法 UTF-8",
                            "把文件转成 UTF-8 后重试",
                        ))
                    }
                }
            }
            Some(b) => {
                return Err(s.diag(
                    DiagCode::Unexpected,
                    &format!("这里期待一个键名，实际看到 {:?}", b as char),
                    "TOML 的键名可以是裸键、双引号串或单引号串",
                    "改成合法键名；或若本意是字符串值，补上等号",
                ))
            }
            None => {
                return Err(s.diag(
                    DiagCode::Unclosed,
                    "键名在源文件中途结束",
                    "文本被截断",
                    "补齐键名",
                ))
            }
        };
        check_segment(s, &seg)?;
        out.push(seg);
        s.skip_spaces();
        if s.peek() == Some(b'.') {
            s.bump();
            continue;
        }
        return Ok(out);
    }
}

/// 校验路径段。**引用侧与定义侧走同一个函数**——两边规范化不一致，
/// 断链红线就永远不触发，而类型检查还抓不到。
fn check_segment(s: &Scanner, seg: &str) -> Result<(), Diag> {
    if seg.is_empty() {
        return Err(s.diag(
            DiagCode::BadSegment,
            "路径里有空段",
            "连续的点号或首尾的点号会产生空段，空段无法定位令牌",
            "删掉多余的点号",
        ));
    }
    for c in seg.chars() {
        if c == '.' || c == '{' || c == '}' {
            return Err(s.diag(
                DiagCode::BadSegment,
                &format!("路径段 {:?} 里含有分隔符或定界符 {:?}", seg, c),
                "路径段里不能出现点号或花括号，否则路径边界无法确定",
                "给这一段改名，去掉这些字符",
            ));
        }
    }
    Ok(())
}

/// 解析一个 TOML 值。
fn toml_value(s: &mut Scanner, depth: usize) -> Result<Val, Diag> {
    if depth > MAX_DEPTH {
        return Err(s.diag(
            DiagCode::DepthLimit,
            &format!("嵌套深度超过上限 {}", MAX_DEPTH),
            "TOML 解析是递归下降，深度无界就是栈溢出",
            "把深层结构拆平：令牌集本来就应该是一层扁平的路径到值的映射",
        ));
    }
    match s.peek() {
        Some(b'"') => {
            // 三引号多行串要先判。
            if s.peek_at(1) == Some(b'"') && s.peek_at(2) == Some(b'"') {
                Ok(Val::Str(toml_multiline_basic(s)?))
            } else {
                Ok(Val::Str(toml_basic_string(s)?))
            }
        }
        Some(b'\'') => {
            if s.peek_at(1) == Some(b'\'') && s.peek_at(2) == Some(b'\'') {
                Ok(Val::Str(toml_multiline_literal(s)?))
            } else {
                Ok(Val::Str(toml_literal_string(s)?))
            }
        }
        Some(b'[') => toml_array(s, depth),
        Some(b'{') => toml_inline_table(s, depth),
        Some(b't') if s.peek_at(1) == Some(b'r') => {
            skip_word(s, "true")?;
            Ok(Val::Bool(true))
        }
        Some(b'f') if s.peek_at(1) == Some(b'a') => {
            skip_word(s, "false")?;
            Ok(Val::Bool(false))
        }
        Some(b'+') | Some(b'-') | Some(b'0'..=b'9') | Some(b'i') | Some(b'n') => toml_number(s),
        Some(b) => Err(s.diag(
            DiagCode::Unexpected,
            &format!("这里期待一个值，实际看到 {:?}", b as char),
            "TOML 的值可以是字符串、数组、内联表、数字、true、false",
            "检查是否漏了引号，或把 JSON 的写法放进了 TOML 文件",
        )),
        None => Err(s.diag(
            DiagCode::Unclosed,
            "期待一个值，但源文件到此结束",
            "TOML 文本在这里是不完整的",
            "补上缺失的值，或确认这段是不是被截断了",
        )),
    }
}

/// 吃掉一个单词（true/false/inf/nan）。
fn skip_word(s: &mut Scanner, want: &str) -> Result<(), Diag> {
    for wb in want.as_bytes() {
        match s.peek() {
            Some(b) if b == *wb => {
                s.bump();
            }
            Some(_) => {
                return Err(s.diag(
                    DiagCode::Unexpected,
                    &format!("{} 写错了", want),
                    "TOML 的布尔与特殊值必须精确拼写",
                    &format!("改成 {}", want),
                ))
            }
            None => {
                return Err(s.diag(
                    DiagCode::Unclosed,
                    &format!("{} 在源文件中途结束", want),
                    "文本被截断",
                    &format!("补齐 {}", want),
                ))
            }
        }
    }
    Ok(())
}

/// 读基本字符串的内容部分（调用方已确认起始引号）。
fn toml_basic_string_body(s: &mut Scanner) -> Result<String, Diag> {
    let mut out = String::new();
    loop {
        let b = match s.peek() {
            Some(b) => b,
            None => {
                return Err(s.diag(
                    DiagCode::Unclosed,
                    "字符串没有闭合引号",
                    "基本字符串必须以双引号结束",
                    "补上闭合引号",
                ))
            }
        };
        match b {
            b'"' => {
                s.bump();
                return Ok(out);
            }
            b'\n' => {
                return Err(s.diag(
                    DiagCode::Unexpected,
                    "单行基本字符串里出现了裸换行",
                    "单行字符串不允许字面换行",
                    "改用三引号多行字符串",
                ))
            }
            b'\\' => {
                s.bump();
                let e = match s.peek() {
                    Some(e) => e,
                    None => {
                        return Err(s.diag(
                            DiagCode::Unclosed,
                            "转义符后面没有内容",
                            "反斜杠必须跟一个转义字符",
                            "补上转义字符，或删掉这个悬空的反斜杠",
                        ))
                    }
                };
                s.bump();
                match e {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let cp = toml_hex(s, 4)?;
                        push_cp(s, &mut out, cp)?;
                    }
                    b'U' => {
                        let cp = toml_hex(s, 8)?;
                        if cp > 0x10FFFF {
                            return Err(s.diag(
                                DiagCode::BadLiteral,
                                "转义出的码位超出 Unicode 范围",
                                "合法的最大码位不超过 0x10FFFF",
                                "换一个范围内的码位",
                            ));
                        }
                        push_cp(s, &mut out, cp)?;
                    }
                    _ => {
                        return Err(s.diag(
                            DiagCode::Unexpected,
                            &format!("未知的转义字符 {:?}", e as char),
                            "TOML 只认这几个转义",
                            "改用合法转义，或去掉这个反斜杠",
                        ))
                    }
                }
            }
            _ => {
                let start = s.pos;
                let n = utf8_seq_len(b);
                for _ in 0..n {
                    if s.bump().is_none() {
                        break;
                    }
                }
                match core::str::from_utf8(&s.src[start..s.pos]) {
                    Ok(t) => out.push_str(t),
                    Err(_) => {
                        return Err(s.diag(
                            DiagCode::BadUtf8,
                            "字符串里有非法 UTF-8 字节序列",
                            "源文件不是合法 UTF-8",
                            "把文件转成 UTF-8 后重试",
                        ))
                    }
                }
            }
        }
    }
}

/// 压入一个码位（代理项按 TOML 规则直接拒绝）。
fn push_cp(s: &Scanner, out: &mut String, cp: u32) -> Result<(), Diag> {
    match char::from_u32(cp) {
        Some(c) => {
            out.push(c);
            Ok(())
        }
        None => Err(s.diag(
            DiagCode::BadLiteral,
            "转义出的码位不是有效字符",
            "该码位在 Unicode 中未分配，或落在代理区",
            "换一个有效的码位",
        )),
    }
}

/// 读 n 位十六进制。
fn toml_hex(s: &mut Scanner, n: usize) -> Result<u32, Diag> {
    let mut v: u32 = 0;
    for _ in 0..n {
        let b = match s.peek() {
            Some(b) => b,
            None => {
                return Err(s.diag(
                    DiagCode::Unclosed,
                    "转义在源文件中途结束",
                    "转义后面必须跟指定位数的十六进制",
                    "补齐十六进制位",
                ))
            }
        };
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => {
                return Err(s.diag(
                    DiagCode::Unexpected,
                    "转义里出现了非十六进制字符",
                    "转义后面必须跟指定位数的十六进制",
                    "改成 0-9 或 a-f",
                ))
            }
        };
        s.bump();
        v = (v << 4) | d as u32;
    }
    Ok(v)
}

/// 读三引号多行基本字符串。
fn toml_multiline_basic(s: &mut Scanner) -> Result<String, Diag> {
    for _ in 0..3 {
        s.bump();
    }
    // 紧跟的换行要被吃掉（TOML 规定多行串首行换行不计入内容）。
    if s.peek() == Some(b'\r') {
        s.bump();
    }
    if s.peek() == Some(b'\n') {
        s.bump();
    }
    let mut out = String::new();
    loop {
        if s.eof() {
            return Err(s.diag(
                DiagCode::Unclosed,
                "多行字符串没有闭合的三引号",
                "多行字符串必须以三个双引号结束",
                "补上闭合的三引号",
            ));
        }
        if s.peek() == Some(b'"') && s.peek_at(1) == Some(b'"') && s.peek_at(2) == Some(b'"') {
            // 允许末尾多写一个引号：TOML 允许用四个或五个引号收尾，
            // 前面多出来的引号属于内容。**先判5再判 4再判 3**。
            if s.peek_at(3) == Some(b'"') && s.peek_at(4) == Some(b'"') {
                for _ in 0..5 {
                    s.bump();
                }
                out.push_str("\"\"");
                return Ok(out);
            }
            if s.peek_at(3) == Some(b'"') {
                for _ in 0..4 {
                    s.bump();
                }
                out.push('"');
                return Ok(out);
            }
            for _ in 0..3 {
                s.bump();
            }
            return Ok(out);
        }
        let b = match s.peek() {
            Some(b) => b,
            None => break,
        };
        if b == b'\\' {
            // 行尾反斜杠续行：吃掉反斜杠与随后的空白（含换行）。
            let mut k = 1;
            while matches!(s.peek_at(k), Some(b' ') | Some(b'\t') | Some(b'\r')) {
                k += 1;
            }
            if s.peek_at(k) == Some(b'\n') {
                for _ in 0..=k {
                    s.bump();
                }
                while matches!(s.peek(), Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')) {
                    s.bump();
                }
                continue;
            }
            s.bump();
            let e = match s.peek() {
                Some(e) => e,
                None => {
                    return Err(s.diag(
                        DiagCode::Unclosed,
                        "转义符后面没有内容",
                        "反斜杠必须跟一个转义字符",
                        "补上转义字符，或删掉这个悬空的反斜杠",
                    ))
                }
            };
            s.bump();
            match e {
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'b' => out.push('\u{8}'),
                b'f' => out.push('\u{c}'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'u' => {
                    let cp = toml_hex(s, 4)?;
                    push_cp(s, &mut out, cp)?;
                }
                b'U' => {
                    let cp = toml_hex(s, 8)?;
                    if cp > 0x10FFFF {
                        return Err(s.diag(
                            DiagCode::BadLiteral,
                            "转义出的码位超出 Unicode 范围",
                            "合法的最大码位不超过 0x10FFFF",
                            "换一个范围内的码位",
                        ));
                    }
                    push_cp(s, &mut out, cp)?;
                }
                _ => {
                    return Err(s.diag(
                        DiagCode::Unexpected,
                        &format!("未知的转义字符 {:?}", e as char),
                        "TOML 只认这几个转义",
                        "改用合法转义，或去掉这个反斜杠",
                    ))
                }
            }
            continue;
        }
        let start = s.pos;
        let n = utf8_seq_len(b);
        for _ in 0..n {
            if s.bump().is_none() {
                break;
            }
        }
        match core::str::from_utf8(&s.src[start..s.pos]) {
            Ok(t) => out.push_str(t),
            Err(_) => {
                return Err(s.diag(
                    DiagCode::BadUtf8,
                    "多行字符串里有非法 UTF-8 字节序列",
                    "源文件不是合法 UTF-8",
                    "把文件转成 UTF-8 后重试",
                ))
            }
        }
    }
    Err(s.diag(
        DiagCode::Unclosed,
        "多行字符串没有闭合的三引号",
        "源文件结束了",
        "补上闭合的三引号",
    ))
}

/// 读基本字符串（含起始引号）。
fn toml_basic_string(s: &mut Scanner) -> Result<String, Diag> {
    s.expect(b'"', "这里是基本字符串的起始引号")?;
    toml_basic_string_body(s)
}

/// 读字面字符串（含起始引号）。
fn toml_literal_string(s: &mut Scanner) -> Result<String, Diag> {
    s.expect(b'\'', "这里是字面字符串的起始引号")?;
    toml_literal_string_body(s)
}

/// 读字面字符串的内容部分（调用方已吃掉起始引号）。
fn toml_literal_string_body(s: &mut Scanner) -> Result<String, Diag> {
    let start = s.pos;
    loop {
        match s.peek() {
            Some(b'\'') => break,
            Some(b'\n') => {
                return Err(s.diag(
                    DiagCode::Unexpected,
                    "单行字面字符串里出现了裸换行",
                    "单行字面串不允许字面换行",
                    "改用三引号多行字面串",
                ))
            }
            Some(_) => {
                s.bump();
            }
            None => {
                return Err(s.diag(
                    DiagCode::Unclosed,
                    "字面字符串没有闭合的引号",
                    "字面串必须以单引号结束",
                    "补上闭合引号",
                ))
            }
        }
    }
    let out = match core::str::from_utf8(&s.src[start..s.pos]) {
        Ok(t) => t.to_string(),
        Err(_) => {
            return Err(s.diag(
                DiagCode::BadUtf8,
                "字面字符串里有非法 UTF-8 字节序列",
                "源文件不是合法 UTF-8",
                "把文件转成 UTF-8 后重试",
            ))
        }
    };
    s.bump();
    Ok(out)
}

/// 读三引号多行字面串（无转义）。
fn toml_multiline_literal(s: &mut Scanner) -> Result<String, Diag> {
    for _ in 0..3 {
        s.bump();
    }
    if s.peek() == Some(b'\r') {
        s.bump();
    }
    if s.peek() == Some(b'\n') {
        s.bump();
    }
    let start = s.pos;
    loop {
        if s.eof() {
            return Err(s.diag(
                DiagCode::Unclosed,
                "多行字面字符串没有闭合的三引号",
                "多行字面串必须以三个单引号结束",
                "补上闭合的三引号",
            ));
        }
        if s.peek() == Some(b'\'')
            && s.peek_at(1) == Some(b'\'')
            && s.peek_at(2) == Some(b'\'')
        {
            let text = match core::str::from_utf8(&s.src[start..s.pos]) {
                Ok(t) => t.to_string(),
                Err(_) => {
                    return Err(s.diag(
                        DiagCode::BadUtf8,
                        "多行字面串里有非法 UTF-8 字节序列",
                        "源文件不是合法 UTF-8",
                        "把文件转成 UTF-8 后重试",
                    ))
                }
            };
            // 与基本串同理：4 个或 5 个引号收尾时，多出的引号属于内容。
            let extra = if s.peek_at(3) == Some(b'\'') && s.peek_at(4) == Some(b'\'') {
                2
            } else if s.peek_at(3) == Some(b'\'') {
                1
            } else {
                0
            };
            for _ in 0..3 + extra {
                s.bump();
            }
            let mut out = text;
            for _ in 0..extra {
                out.push('\'');
            }
            return Ok(out);
        }
        s.bump();
    }
}

/// 解析 TOML 数字（含 inf/nan 与下划线分隔）。
fn toml_number(s: &mut Scanner) -> Result<Val, Diag> {
    let start = s.pos;
    let site = s.site();
    if matches!(s.peek(), Some(b'+') | Some(b'-')) {
        s.bump();
    }
    // inf / nan
    if s.peek() == Some(b'i') {
        skip_word(s, "inf")?;
        let raw = match core::str::from_utf8(&s.src[start..s.pos]) {
            Ok(t) => t.to_string(),
            Err(_) => {
                return Err(s.diag(
                    DiagCode::BadUtf8,
                    "字面量不是合法 UTF-8",
                    "源文件被截断",
                    "检查文件编码",
                ))
            }
        };
        return Ok(Val::Num(NumLit { raw, site }));
    }
    if s.peek() == Some(b'n') {
        skip_word(s, "nan")?;
        let raw = match core::str::from_utf8(&s.src[start..s.pos]) {
            Ok(t) => t.to_string(),
            Err(_) => {
                return Err(s.diag(
                    DiagCode::BadUtf8,
                    "字面量不是合法 UTF-8",
                    "源文件被截断",
                    "检查文件编码",
                ))
            }
        };
        return Ok(Val::Num(NumLit { raw, site }));
    }
    let mut seen_digit = false;
    let mut seen_dot = false;
    let mut seen_exp = false;
    while let Some(c) = s.peek() {
        if c.is_ascii_digit() {
            seen_digit = true;
            s.bump();
        } else if c == b'_' {
            // 下划线不能出现在首尾，也不能连着。
            let prev = if s.pos > start {
                s.src.get(s.pos - 1).copied()
            } else {
                None
            };
            let next = s.peek_at(1);
            match (prev, next) {
                (Some(p), Some(n)) if p.is_ascii_digit() && n.is_ascii_digit() => {
                    s.bump();
                }
                _ => {
                    return Err(s.diag(
                        DiagCode::BadLiteral,
                        "下划线分隔符位置不对",
                        "下划线必须夹在两个数字之间",
                        "去掉这个下划线，或补上它后面的数字",
                    ))
                }
            }
        } else if c == b'.' && !seen_dot && !seen_exp {
            seen_dot = true;
            s.bump();
        } else if (c == b'e' || c == b'E') && seen_digit && !seen_exp {
            seen_exp = true;
            s.bump();
            if matches!(s.peek(), Some(b'+') | Some(b'-')) {
                s.bump();
            }
        } else if c == b':' || c == b'T' || c == b'Z' || c == b'.' {
            // 日期时间：令牌集里没有日期语义，显式拒绝而不是半解析。
            return Err(s.diag(
                DiagCode::BadLiteral,
                "令牌值不能是日期时间",
                "令牌只承载颜色、尺寸、字重、时长这类外观值，日期语义不在其中",
                "改写成字符串或数字；若确需日期，请另建专门的键域",
            ));
        } else {
            break;
        }
    }
    if !seen_digit {
        return Err(s.diag(
            DiagCode::BadLiteral,
            "数字字面量里没有数字",
            "只有符号或小数点不构成数字",
            "补上数字，或删掉这个值",
        ));
    }
    let raw = match core::str::from_utf8(&s.src[start..s.pos]) {
        Ok(t) => t.to_string(),
        Err(_) => {
            return Err(s.diag(
                DiagCode::BadUtf8,
                "字面量不是合法 UTF-8",
                "源文件被截断",
                "检查文件编码",
            ))
        }
    };
    Ok(Val::Num(NumLit { raw, site }))
}

/// 解析 TOML 数组。
fn toml_array(s: &mut Scanner, depth: usize) -> Result<Val, Diag> {
    let open = s.site();
    s.bump();
    let mut out: Vec<Val> = Vec::new();
    loop {
        // 数组里允许换行与注释。
        s.skip_ws();
        while s.peek() == Some(b'#') {
            while let Some(c) = s.peek() {
                if c == b'\n' {
                    break;
                }
                s.bump();
            }
            s.skip_ws();
        }
        if s.peek() == Some(b']') {
            s.bump();
            return Ok(Val::Arr(out));
        }
        if s.eof() {
            return Err(s.diag(
                DiagCode::Unclosed,
                "数组没有闭合的方括号",
                &format!(
                    "数组在第 {} 行第 {} 列被打开，但源文件先结束了",
                    open.line, open.col
                ),
                "补上闭合的方括号",
            ));
        }
        out.push(toml_value(s, depth + 1)?);
        s.skip_ws();
        while s.peek() == Some(b'#') {
            while let Some(c) = s.peek() {
                if c == b'\n' {
                    break;
                }
                s.bump();
            }
            s.skip_ws();
        }
        match s.peek() {
            Some(b',') => {
                s.bump();
            }
            Some(b']') => {
                s.bump();
                return Ok(Val::Arr(out));
            }
            Some(c) => {
                return Err(s.diag(
                    DiagCode::Unexpected,
                    &format!("数组成员之间期待逗号或右方括号，实际看到 {:?}", c as char),
                    "成员分隔符缺失",
                    "补上逗号，或删掉多余字符",
                ))
            }
            None => {
                return Err(s.diag(
                    DiagCode::Unclosed,
                    "数组没有闭合的方括号",
                    "源文件结束了",
                    "补上闭合的方括号",
                ))
            }
        }
    }
}

/// 解析 TOML 内联表。
fn toml_inline_table(s: &mut Scanner, depth: usize) -> Result<Val, Diag> {
    let open = s.site();
    s.bump();
    let mut out: Vec<(String, Val)> = Vec::new();
    s.skip_spaces();
    if s.peek() == Some(b'}') {
        s.bump();
        return Ok(Val::Table(out));
    }
    loop {
        s.skip_spaces();
        let kp = toml_key_path(s)?;
        s.skip_spaces();
        s.expect(b'=', "内联表的键与值之间缺少等号")?;
        s.skip_spaces();
        let v = toml_value(s, depth + 1)?;
        // 内联表允许点分键，等价于嵌套表。
        let mut full = kp;
        if full.is_empty() {
            return Err(s.diag(
                DiagCode::BadSegment,
                "内联表里有空键",
                "空键无法定位令牌",
                "给这一项写一个键名",
            ));
        }
        let head = full.remove(0);
        if full.is_empty() {
            if out.iter().any(|(k, _)| *k == head) {
                return Err(s.diag(
                    DiagCode::DupPath,
                    "内联表里有重复的键",
                    "同一个键在一张内联表里出现了两次",
                    "删掉其中一处；令牌单源律要求每个路径只有一个定义点",
                ));
            }
            out.push((head, v));
        } else {
            let mut t: Vec<(String, Val)> = Vec::new();
            let sub: Vec<String> = full;
            toml_put(&mut t, &sub, v)?;
            if out.iter().any(|(k, _)| *k == head) {
                return Err(s.diag(
                    DiagCode::DupPath,
                    "内联表里有重复的键",
                    "同一个点分前缀在一张内联表里出现了两次",
                    "合并这两处，或给其中一处改名",
                ));
            }
            out.push((head, Val::Table(t)));
        }
        s.skip_spaces();
        match s.peek() {
            Some(b',') => {
                s.bump();
                s.skip_spaces();
                if s.peek() == Some(b'}') {
                    return Err(s.diag(
                        DiagCode::Unexpected,
                        "内联表里出现了悬空的逗号",
                        "TOML 不允许尾随逗号",
                        "删掉这个逗号",
                    ));
                }
            }
            Some(b'}') => {
                s.bump();
                return Ok(Val::Table(out));
            }
            Some(c) => {
                return Err(s.diag(
                    DiagCode::Unexpected,
                    &format!("内联表成员之间期待逗号或右花括号，实际看到 {:?}", c as char),
                    "成员分隔符缺失",
                    "补上逗号，或删掉多余字符",
                ))
            }
            None => {
                return Err(s.diag(
                    DiagCode::Unclosed,
                    "内联表没有闭合的花括号",
                    &format!(
                        "内联表在第 {} 行第 {} 列被打开，但源文件先结束了",
                        open.line, open.col
                    ),
                    "补上闭合的花括号",
                ))
            }
        }
    }
}// ---------------------------------------------------------------------------
// 八、扁平化：值树 → 路径到值的令牌表
// ---------------------------------------------------------------------------

/// 令牌表里的一条：路径 + 原始值文本 + 定义点。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TokenEntry {
    /// 点分路径。
    pub path: String,
    /// 值的原始文本（数字保留原文）。
    pub raw: String,
    /// 定义点。
    pub site: Site,
}

/// 扁平化后的令牌集。**两种格式产出的本结构必须逐字段相同**。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TokenSet {
    /// 源格式。
    pub format: SourceFormat,
    /// 令牌条目，按路径字典序（保证双格式对拍不受书写顺序影响）。
    pub entries: Vec<TokenEntry>,
}

impl TokenSet {
    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按路径取条目。
    pub fn get(&self, path: &str) -> Option<&TokenEntry> {
        self.entries.iter().find(|e| e.path == path)
    }
}

/// 把值树扁平化成令牌表。
///
/// **数组不参与扁平**：令牌集是路径到标量的映射，数组元素没有稳定的路径
/// （下标不是作者命名的语义）。数组出现在令牌位置时报错而不是悄悄拍平成
/// `path.0`、`path.1` ——那会让下游拿到作者没命名的路径。
pub fn flatten(v: &Val, fmt: SourceFormat) -> Result<TokenSet, Vec<Diag>> {
    let mut out: Vec<TokenEntry> = Vec::new();
    let mut diags: Vec<Diag> = Vec::new();
    match v {
        Val::Table(members) => {
            let mut prefix: Vec<String> = Vec::new();
            flatten_table(members, &mut prefix, &mut out, &mut diags);
        }
        other => {
            diags.push(Diag::new(
                DiagCode::Unexpected,
                Site::start(),
                "令牌文件的顶层必须是一个表或对象",
                "顶层若不是表，就没有一个可以命名的根来承载路径",
                "把顶层包进一个对象；TOML 则是至少写一个键值对",
            ));
            let _ = other;
        }
    }
    if !diags.is_empty() {
        return Err(diags);
    }
    if out.len() > MAX_TOKENS {
        return Err(vec![Diag::new(
            DiagCode::TokenLimit,
            Site::start(),
            &format!("令牌数 {} 超过上限 {}", out.len(), MAX_TOKENS),
            "令牌集超过上限后，下游注册表也装不下，硬放行只会造成解析成功但入册失败",
            &format!("把令牌数压到 {} 以内，或拆成多份令牌集", MAX_TOKENS),
        )]);
    }
    // 重复定义在 flatten 阶段就要拦住：单源律的破坏必须在解析期暴露。
    let mut sorted = out;
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut dup: Vec<Diag> = Vec::new();
    let mut i = 1;
    while i < sorted.len() {
        if sorted[i - 1].path == sorted[i].path {
            dup.push(Diag::new(
                DiagCode::DupPath,
                sorted[i].site,
                &format!("路径 {:?} 被定义了两次", sorted[i].path),
                "同一个对象里出现了两个同名键，或表头与键值对指向同一路径",
                "删掉其中一处；令牌单源律要求每个路径只有一个定义点",
            ));
        }
        i += 1;
    }
    if !dup.is_empty() {
        return Err(dup);
    }
    Ok(TokenSet {
        format: fmt,
        entries: sorted,
    })
}

/// 递归展开一张表。
fn flatten_table(
    members: &[(String, Val)],
    prefix: &mut Vec<String>,
    out: &mut Vec<TokenEntry>,
    diags: &mut Vec<Diag>,
) {
    for (k, v) in members.iter() {
        match v {
            Val::Table(sub) => {
                prefix.push(k.clone());
                flatten_table(sub, prefix, out, diags);
                prefix.pop();
            }
            Val::Arr(_) => {
                diags.push(Diag::new(
                    DiagCode::Unexpected,
                    Site::start(),
                    &format!(
                        "路径 {:?} 的值是数组",
                        join_path(prefix, k)
                    ),
                    "数组元素没有作者命名的路径，下标不是语义",
                    "把数组改成显式的不同路径；若本意是一组同义令牌，用前缀加名字展开",
                ));
            }
            Val::Str(s) => out.push(TokenEntry {
                path: join_path(prefix, k),
                raw: s.clone(),
                site: Site::start(),
            }),
            Val::Num(n) => out.push(TokenEntry {
                path: join_path(prefix, k),
                raw: n.raw.clone(),
                site: n.site,
            }),
            Val::Bool(b) => out.push(TokenEntry {
                path: join_path(prefix, k),
                raw: if *b { "true" } else { "false" }.to_string(),
                site: Site::start(),
            }),
        }
    }
}

/// 拼路径。
fn join_path(prefix: &[String], key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{}.{}", prefix.join("."), key)
    }
}

// ---------------------------------------------------------------------------
// 九、引用抽取（令牌值里的 {path}）
// ---------------------------------------------------------------------------

/// 引用种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefKind {
    /// 整个值就是一个引用，可以直接求值成目标的值。
    Whole,
    /// 值里嵌着引用，需要把目标的值拼进来（形如 `1px solid {color.border}`）。
    Embedded,
}

/// 一条引用边。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RefEdge {
    /// 源节点（引用方）。
    pub from: u32,
    /// 目标路径（被引用方）。
    pub target: String,
    /// 目标节点（断链时无意义）。
    pub to: u32,
    /// 引用种类。
    pub kind: RefKind,
    /// 引用出现在源文件的位置（断链时能报到行）。
    pub site: Site,
}

/// 从一个值文本里抽出全部 `{...}` 引用。
///
/// **值里没有花括号就返回空**（多数令牌是字面量，不该为它们付扫描代价）。
pub fn extract_refs(text: &str, site: Site) -> Result<Vec<(String, RefKind)>, Diag> {
    if !text.contains(REF_OPEN) {
        return Ok(Vec::new());
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<(String, RefKind)> = Vec::new();
    let mut i = 0usize;
    let mut covered = 0usize;
    while i < chars.len() {
        if chars[i] != REF_OPEN {
            i += 1;
            continue;
        }
        // 花括号转义：{{ 与}} 是字面花括号，不是引用定界。
        if i + 1 < chars.len() && chars[i + 1] == REF_OPEN {
            i += 2;
            continue;
        }
        let mut j = i + 1;
        let mut inner = String::new();
        let mut closed = false;
        while j < chars.len() {
            if chars[j] == REF_CLOSE {
                closed = true;
                break;
            }
            inner.push(chars[j]);
            j += 1;
        }
        if !closed {
            return Err(Diag::new(
                DiagCode::Unexpected,
                site,
                "值里有没闭合的引用花括号",
                "引用必须成对出现，右花括号缺失",
                "补上右花括号；若本意是字面花括号，写成两个左花括号",
            ));
        }
        let target = inner.trim().to_string();
        if target.is_empty() {
            return Err(Diag::new(
                DiagCode::BadSegment,
                site,
                "引用花括号里是空的",
                "空引用没有目标，求值时无从查起",
                "在花括号里写上被引用的路径",
            ));
        }
        for seg in target.split('.') {
            check_segment_text(site, seg)?;
        }
        let is_whole = i == 0 && j + 1 == chars.len();
        let kind = if is_whole {
            RefKind::Whole
        } else {
            RefKind::Embedded
        };
        if !out.iter().any(|(p, _)| *p == target) {
            out.push((target, kind));
        }
        covered += j - i + 1;
        i = j + 1;
    }
    // "整个值就是引用"的判据：剥掉定界后正好覆盖全部字符。
    if covered == chars.len() && out.len() == 1 {
        if let Some(last) = out.last_mut() {
            last.1 = RefKind::Whole;
        }
    }
    Ok(out)
}

/// 无扫描器版本的段校验（引用侧用）。
fn check_segment_text(site: Site, seg: &str) -> Result<(), Diag> {
    if seg.is_empty() {
        return Err(Diag::new(
            DiagCode::BadSegment,
            site,
            "引用路径里有空段",
            "连续的点号会产生空段，空段无法定位令牌",
            "删掉多余的点号",
        ));
    }
    for c in seg.chars() {
        if c == '.' || c == '{' || c == '}' {
            return Err(Diag::new(
                DiagCode::BadSegment,
                site,
                &format!("引用路径段 {:?} 里含有分隔符或定界符 {:?}", seg, c),
                "路径段里不能出现点号或花括号",
                "改掉这个字符",
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 十、DAG 构建（CSR 邻接 + 断链检查 + 环检测）
// ---------------------------------------------------------------------------

/// 令牌引用图。**出边用 CSR 存储**（`out_start` +扁平的 `out_target`），
/// 不是 `Vec<Vec<u32>>`——后者在no_std 下每个节点一次堆分配，几千个令牌
/// 就是几千次分配加几千个指针跳转。
#[derive(Clone, Debug)]
pub struct TokenDag {
    /// 节点数（等于令牌数）。
    pub nodes: usize,
    /// CSR 行偏移，长度 `nodes + 1`。
    pub out_start: Vec<u32>,
    /// CSR 目标列，扁平存储。
    pub out_target: Vec<u32>,
    /// 全部边（保序，便于报错时按源文件顺序列出）。
    pub edges: Vec<RefEdge>,
    /// 路径索引。
    pub index: PathIndex,
}

impl TokenDag {
    /// 节点 `n` 的出度。
    pub fn out_degree(&self, n: u32) -> usize {
        let n = n as usize;
        if n >= self.nodes {
            return 0;
        }
        (self.out_start[n + 1] - self.out_start[n]) as usize
    }

    /// 节点 `n` 的第 `k` 个出边目标。
    pub fn out_target_at(&self, n: u32, k: usize) -> Option<u32> {
        let n = n as usize;
        if n >= self.nodes {
            return None;
        }
        let base = self.out_start[n] as usize;
        self.out_target.get(base + k).copied()
    }

    /// 节点 `n` 的第 `i` 条边（`i` 是边在 `edges` 里的下标）。
    pub fn edge_at(&self, i: usize) -> Option<&RefEdge> {
        self.edges.get(i)
    }

    /// 边数。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

/// 构建引用图。**断链在此拦下，环在此拦下**——不给下游一张坏图。
pub fn build_dag(ts: &TokenSet) -> Result<TokenDag, Vec<Diag>> {
    let mut index = PathIndex::new();
    for (i, e) in ts.entries.iter().enumerate() {
        if !index.insert(&e.path, i as u32) {
            // flatten 已拦过重复路径；到这里说明内部不一致，必须显性化。
            return Err(vec![Diag::new(
                DiagCode::DupPath,
                e.site,
                &format!("路径 {:?} 在建图时重复", e.path),
                "路径索引拒绝重复插入，而扁平化本应已排除重复",
                "这是解析器内部不一致，请把这份令牌集与解析器版本一起上报",
            )]);
        }
    }

    let mut edges: Vec<RefEdge> = Vec::new();
    let mut diags: Vec<Diag> = Vec::new();
    for (i, e) in ts.entries.iter().enumerate() {
        let refs = match extract_refs(&e.raw, e.site) {
            Ok(r) => r,
            Err(d) => {
                diags.push(d);
                continue;
            }
        };
        for (target, kind) in refs {
            // **断链**：目标路径不在本文件里。
            match index.get(&target) {
                None => {
                    diags.push(Diag::new(
                        DiagCode::BrokenRef,
                        e.site,
                        &format!(
                            "路径 {:?} 引用了 {:?}，但本文件里没有这个路径",
                            e.path, target
                        ),
                        "被引用的路径没有定义点，引用链在这里断开",
                        &format!(
                            "在令牌文件里补上 {:?} 的定义，或把引用改到已存在的路径上",
                            target
                        ),
                    ));
                    continue;
                }
                Some(to) => {
                    // 自引用是无环的退化情形：自己引用自己永远求不出值，
                    // 直接按环报（长度为 1 的环），不留给下游。
                    edges.push(RefEdge {
                        from: i as u32,
                        target,
                        to,
                        kind,
                        site: e.site,
                    });
                }
            }
        }
    }
    if !diags.is_empty() {
        return Err(diags);
    }

    // 建CSR：先数入度，再前缀和，最后按源节点顺序填。
    let nodes = ts.entries.len();
    let mut out_start = Vec::new();
    out_start.resize(nodes + 1, 0u32);
    for e in edges.iter() {
        let f = e.from as usize;
        if f < nodes {
            out_start[f + 1] += 1;
        }
    }
    let mut i = 1;
    while i <= nodes {
        out_start[i] += out_start[i - 1];
        i += 1;
    }
    let mut cursor: Vec<u32> = out_start[..nodes].to_vec();
    let mut out_target = Vec::new();
    out_target.resize(edges.len(), 0u32);
    for e in edges.iter() {
        let f = e.from as usize;
        if f >= nodes {
            continue;
        }
        let slot = cursor[f] as usize;
        if slot < out_target.len() {
            out_target[slot] = e.to;
        }
        cursor[f] += 1;
    }

    let dag = TokenDag {
        nodes,
        out_start,
        out_target,
        edges,
        index,
    };

    // 环检测在建图之后立即做：给下游的图必须无环。
    let cycles = detect_cycles(&dag, ts);
    if !cycles.is_empty() {
        return Err(cycles);
    }
    Ok(dag)
}

/// 迭代 DFS 找环（**显式栈，不是递归**：内核栈浅，千节点链用递归会溢栈）。
///
/// 返回的每条诊断都把**环本身**报出来：`a → b → c → a`，逐节点附定义点行号。
pub fn detect_cycles(dag: &TokenDag, ts: &TokenSet) -> Vec<Diag> {
    // 0=未访问 1=在当前路径上 2=已完成
    let mut state = Vec::new();
    state.resize(dag.nodes, 0u8);
    let mut out: Vec<Diag> = Vec::new();
    // 每个起点的当前边游标
    let mut cursor: Vec<u32> = Vec::new();
    cursor.resize(dag.nodes, 0u32);

    for start in 0..dag.nodes {
        if state[start] != 0 {
            continue;
        }
        // 显式栈：存节点号与它的入边下标（入边用来回溯环的起点）。
        let mut stack: Vec<(u32, usize)> = vec![(start as u32, usize::MAX)];
        let mut path: Vec<u32> = vec![start as u32];
        state[start] = 1;
        cursor[start] = 0;

        while let Some((node, _)) = stack.pop() {
            let n = node as usize;
            let deg = dag.out_degree(node);
            if cursor[n] as usize >= deg {
                // 出边走完，回退。
                state[n] = 2;
                if let Some(p) = path.pop() {
                    if p as usize != start {
                        stack.push((p, 0));
                    }
                }
                continue;
            }
            let k = cursor[n] as usize;
            cursor[n] += 1;
            let next = match dag.out_target_at(node, k) {
                Some(t) => t,
                None => continue,
            };
            let t = next as usize;
            if t >= dag.nodes {
                continue;
            }
            if state[t] == 1 {
                // t 在当前路径上 → 找到环。从 t 在path 里的位置截出环。
                if let Some(pos) = path.iter().position(|x| *x as usize == t) {
                    let ring: Vec<u32> = path[pos..].to_vec();
                    out.push(cycle_diag(&ring, ts));
                }
                continue;
            }
            if state[t] == 0 {
                state[t] = 1;
                cursor[t] = 0;
                path.push(next);
                stack.push((next, 0));
            }
        }
    }
    out
}

/// 把一个环写成三要素诊断（逐节点附定义点）。
fn cycle_diag(ring: &[u32], ts: &TokenSet) -> Diag {
    let mut names: Vec<String> = Vec::new();
    let mut where_ = Vec::new();
    for n in ring.iter() {
        let e = ts.entries.get(*n as usize);
        match e {
            Some(e) => {
                names.push(e.path.clone());
                where_.push(format!("第 {} 行第 {} 列", e.site.line, e.site.col));
            }
            None => {
                names.push(format!("<节点 {}>", n));
                where_.push("位置未知".to_string());
            }
        }
    }
    let mut chain = names.join(" → ");
    chain.push_str(&format!(" → {}", names.first().cloned().unwrap_or_default()));
    let site = match ring.first().and_then(|n| ts.entries.get(*n as usize)) {
        Some(e) => e.site,
        None => Site::start(),
    };
    Diag::new(
        DiagCode::Cycle,
        site,
        &format!("引用成环：{}", chain),
        &format!(
            "这条引用链首尾相接（{}），任何起点都求不出值",
            where_.join("，")
        ),
        "断开环上的任意一条引用；若本意是让两者取同一个值，改成让其中一方引用一个更基础的令牌",
    )
}

// ---------------------------------------------------------------------------
// 十一、端到端入口
// ---------------------------------------------------------------------------

/// 一步到位：源文本 → 令牌集 → 引用图。
pub fn parse_and_build(src: &str, fmt: SourceFormat) -> Result<(TokenSet, TokenDag), Vec<Diag>> {
    let v = match fmt {
        SourceFormat::Json => parse_json(src),
        SourceFormat::Toml => parse_toml(src),
    };
    let v = match v {
        Ok(v) => v,
        // 语法错只有一条（后续内容不可信），但仍用 Vec 统一返回形态。
        Err(d) => return Err(vec![d]),
    };
    let ts = flatten(&v, fmt)?;
    let dag = build_dag(&ts)?;
    Ok((ts, dag))
}

/// VE-F3402 域自检入口（判据逐条映射见 `ver01b_checks.rs`）。
pub fn run_ver01b_checks() -> CheckSet {
    super::ver01b_checks::run_ver01b_checks()
}