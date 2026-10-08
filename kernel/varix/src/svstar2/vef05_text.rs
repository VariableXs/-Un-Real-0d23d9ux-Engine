//! VE-F1005 · PNG 文本块族（tEXt/iTXt/zTXt，目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1005`
//!
//! **判据（锚点原文逐条）**：
//! - tEXt（Latin-1：keyword 1-79 字符 + text）→ `C05-TEXT-*`
//! - iTXt（UTF-8：压缩标志/压缩方法/语言标签/翻译 keyword **四段结构**，
//!   压缩走 zlib）→ `C05-ITXT-*`
//! - zTXt（压缩 Latin-1）→ `C05-ZTXT-*`
//! - 三块统一产出**元数据条目四元组**（keyword/语言/文本/压缩态）
//!   → `C05-ENTRY-*`
//! - 文本注入安全（只存储展示不执行；**转义非剥离**；单条 2MB 上限、
//!   条数 500 上限防元数据炸弹）→ `C05-SAFE-*`
//! - 错误路径（解压失败→跳过该条并计数；keyword 非法字符→**规整为下划线**
//!   保留条目；重复 keyword→**共存不覆盖**的列表语义）→ `C05-ERR-*`
//!
//! ---
//!
//! ## 设计要点一：为什么自带块扫描器，而不复用 F1004 的
//!
//! 与 [`crate::svstar2::vef04_color`] 的选择同构：F1004 的
//! `scan_color_chunks` 只找四个**色彩**块且**取首个**（其
//! `RawColorChunks` 每字段是 `Option<Vec<u8>>`）。而本单的文本块要的恰恰
//! 是**列表语义**——锚点明写「重复 keyword→**共存不覆盖**」。
//!
//! 复用它有两条路，都不可取：
//!  · 把 `RawColorChunks` 的字段改成 `Vec<Vec<u8>>` →那是让**色彩管理的
//!    产物结构**为文本块的需求改型，与 F1004 已定的「取首个」语义冲突；
//!  · 加一个「通用块收集器」参数 → 把F1004 的窄扫描器撑成通用解析器，
//!    而 F1004 的每一条边界推导（CRC 覆盖范围、溢出 checked、告警计数）
//!    都会变成需要被文本块继承的隐式契约。
//!
//! 故本模块**自带一个只读块扫描器**，边界同样极窄：只找三个目标块、
//! 只校验 CRC、**其余块一概跳过**、**不解释长度以外的任何内容**。
//! 两份遍历逻辑是有意的取舍，代价与收益已在 F1004 头部登记过，此处不重复。
//!
//! ## 设计要点二：列表语义是「收集全部」，不是「覆盖」
//!
//! 锚点：「重复 keyword→共存不覆盖（列表语义）」。这条决定了三件事：
//!  1. 结果容器是 [`MetadataChannel`] 里的 `Vec<TextEntry>`，**不是** map；
//!  2. 扫描器**不能**像 F1004 那样只取首个——每个块都要收；
//!  3. 条数上限 500 是**硬闸**：超限即拒后续条目并计数，不是静默截断。
//!
//! 「不覆盖」这条最容易被实现成map 的 `insert`（后者默认覆盖）。
//! 若用 map，`tEXt` 两条同keyword 只会留一条，且**顺序不定**——
//! 判据 `C05-ERR-03` 专门断这一条：断言两条都在且**顺序与文件序一致**。
//!
//! ## 设计要点三：keyword 非法字符「规整」而非「拒绝」
//!
//! 锚点：「keyword 非法字符→**规整为下划线**保留条目」。
//!
//! 注意锚点用的是「规整…**保留条目**」——即条目**不丢**。这与「拒绝该条」
//! 是两种不同的失败语义。规范说 keyword 限Latin-1 可见字符
//! `0x20..=0x7E` 且 `1..=79` 字节，但**实际文件里超限的极少**，
//! 判它非法就打不开整张图，那才是真错。
//!
//! 故 [`normalize_keyword`] 做两件**可逆性最强**的事：
//!  · 非法字节 → `_`（0x5F），**逐字节**替换，不改变长度；
//!  · 长度裁到 79（若超），空keyword → `"_"`。
//!
//! **长度裁剪必须登记计数**（[`TextFault::keyword_truncated`] 走
//! [`MetadataChannel::warnings`]）：静默裁剪会让下游以为拿到了完整
//! keyword，而它其实是被截断的——那与「伪造」同级。
//!
//! ## 设计要点四：转义而非剥离（保真原则）
//!
//! 锚点：「HTML/脚本类内容在展示层**转义非剥离**（保真原则：元数据是
//! 用户资产，展示安全而非内容破坏）」。
//!
//! 这是本单最容易被做反的一条。把 `<script>` **删掉**看起来更"安全"，
//! 但它**销毁了用户资产**——元数据的价值就在于它忠实记录了文件里有什么。
//! 正确做法是 [`escape_for_display`]：把 `& < > " '` 换成实体，
//! 使其**在任何 HTML 文本节点/属性上下文中都不可执行**，同时
//! **信息完全可还原**（`&amp;` → `&`）。
//!
//! 判据 `C05-SAFE-02` 双向断：既断「`<script>` 原样透出」（不安全），
//! 也断「`<script>` 被整段删掉」（破坏资产）——只接受**转义后形态**。
//!
//! ## 设计要点五：UTF-8 校验须自写并给位置
//!
//! `no_std` 下判据侧不能依赖 `core::str::from_utf8`（它在
//! `core::str`，但本模块对外API 返回自有条目，不产出 `&str`——
//! 产出 `&str` 会把生命周期绑到 `Vec` 的借用上，让调用方难以持有）。
//! 故 [`validate_utf8`] 自写，返回**首个非法序列的字节偏移**，
//! 使错误可定位到具体字节而非只报一个 bool。

#![allow(clippy::needless_range_loop)]

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::perfstar::frameledger_ext;
use crate::perfstar::mech_inflate;
use crate::svstar2::vef01_pngdec as dec;

// ---------------------------------------------------------------------------
// 一、锚点常量
// ---------------------------------------------------------------------------

/// 规范限定的 keyword 最大字节数（锚点「keyword 1-79 字符」）。
pub const KEYWORD_MAX: usize = 79;

/// 锚点「单条 2MB 上限」——元数据炸弹的单条防线。
pub const TEXT_MAX_BYTES: usize = 2 * 1024 * 1024;

/// 锚点「条数上限 500 防元数据炸弹」——整份文件的条目数防线。
pub const ENTRY_MAX_COUNT: usize = 500;

/// 规范允许的 keyword 可见字符上界（含）。
const KEYWORD_CHAR_MAX: u8 = 0x7E;

/// 规范允许的 keyword 可见字符下界（含）。
const KEYWORD_CHAR_MIN: u8 = 0x20;

/// 非法字符的规整目标（锚点「规整为下划线」）。
const REPLACEMENT: u8 = b'_';

// ---------------------------------------------------------------------------
// 二、错误面
// ---------------------------------------------------------------------------

/// 故障种类。
///
/// **码位不复用、不回收**：删除变体时保留空洞（见本文件末`FAULT_REGISTRY`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextFaultKind {
    /// 文件签名不是 PNG。
    BadSignature,
    /// 块被截断（长度字段越界或溢出）。
    ChunkTruncated,
    /// UTF-8 校验失败（`a` = 首个非法字节偏移，`b` = 非法字节值）。
    Utf8Invalid,
    /// iTXt 压缩标志非 0（锚点：标志非 0 即不支持压缩的 iTXt）。
    CompressionFlag,
    /// iTXt 压缩方法非 0（锚点「压缩方法」字段只认 deflate）。
    CompressionMethod,
    /// 解压失败（zTXt / 压缩 iTXt 共用）。
    Inflate,
    /// 解压产出超单条 2MB 上限。
    TooLarge,
    /// 通道条目数超500 上限。
    TooManyEntries,
}

impl TextFaultKind {
    /// 稳定错误码（供遥测与下游定位；**只增不改**）。
    pub fn code(self) -> u8 {
        match self {
            TextFaultKind::BadSignature => 1,
            TextFaultKind::ChunkTruncated => 2,
            TextFaultKind::Utf8Invalid => 3,
            TextFaultKind::CompressionFlag => 4,
            TextFaultKind::CompressionMethod => 5,
            TextFaultKind::Inflate => 6,
            TextFaultKind::TooLarge => 7,
            TextFaultKind::TooManyEntries => 8,
        }
    }

    /// 人类可读名（判据与日志用；不参与裁决）。
    pub fn name(self) -> &'static str {
        match self {
            TextFaultKind::BadSignature => "BadSignature",
            TextFaultKind::ChunkTruncated => "ChunkTruncated",
            TextFaultKind::Utf8Invalid => "Utf8Invalid",
            TextFaultKind::CompressionFlag => "CompressionFlag",
            TextFaultKind::CompressionMethod => "CompressionMethod",
            TextFaultKind::Inflate => "Inflate",
            TextFaultKind::TooLarge => "TooLarge",
            TextFaultKind::TooManyEntries => "TooManyEntries",
        }
    }
}

/// 故障值（`a`/`b` 的含义随 `kind` 变，见各分支注释）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TextFault {
    pub kind: TextFaultKind,
    pub a: u64,
    pub b: u64,
}

impl TextFault {
    pub fn new(kind: TextFaultKind) -> Self {
        TextFault { kind, a: 0, b: 0 }
    }
    pub fn with(kind: TextFaultKind, a: u64, b: u64) -> Self {
        TextFault { kind, a, b }
    }
}

// ---------------------------------------------------------------------------
// 三、元数据条目（四元组）
// ---------------------------------------------------------------------------

/// 压缩态（锚点「文本/压缩态」维度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Compressed {
    /// 未压缩（tEXt / iTXt 压缩标志=0）。
    None,
    /// 本条曾以压缩形式存储（zTXt / iTXt 压缩标志=1），**已解压**。
    ///
    /// 注意语义：这是**来源标记**而非「当前仍压缩」——解析后文本恒为
    /// 明文。保留它是为了下游能告知用户「这条原本是压过的」，
    /// 以及判据能区分「走解压路径」与「走明文路径」。
    WasCompressed,
}

/// 文本块种类（来源标记）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextKind {
    /// tEXt：Latin-1 键值对。
    Text,
    /// iTXt：UTF-8 国际文本（四段结构）。
    InternationalText,
    /// zTXt：压缩 Latin-1。
    CompressedText,
}

impl TextKind {
    pub fn name(self) -> &'static str {
        match self {
            TextKind::Text => "tEXt",
            TextKind::InternationalText => "iTXt",
            TextKind::CompressedText => "zTXt",
        }
    }
}

/// 元数据条目（锚点「keyword/语言/文本/压缩态四元组」）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TextEntry {
    /// 规整后的 keyword（`normalize_keyword` 处理）。
    pub keyword: String,
    /// 语言标签（iTXt 才有；tEXt/zTXt恒为空串）。
    pub language: String,
    /// 已解压的明文文本。
    pub text: Vec<u8>,
    /// 来源压缩态。
    pub compressed: Compressed,
    /// 来源块种类。
    pub kind: TextKind,
}

/// 元数据通道（锚点「三块统一解析产出进图像元数据通道（CGPU-F0069）」）。
///
/// **通道侧只做存储与展示**——本类型不含任何渲染/执行逻辑，
/// 展示安全由 [`escape_for_display`] 在**消费侧**完成（锚点
/// 「展示层转义」）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct MetadataChannel {
    /// 条目列表（**顺序 = 文件内出现顺序**，锚点列表语义）。
    pub entries: Vec<TextEntry>,
    /// CRC 校验失败的块数（告警计数）。
    pub crc_warnings: u32,
    /// 解压失败而**跳过**的条目数（锚点「解压失败→跳过该条并计数」）。
    pub inflate_skipped: u32,
    /// keyword 非法字符被规整的条目数。
    pub keyword_normalized: u32,
    /// keyword 超长被裁剪的条目数。
    pub keyword_truncated: u32,
    /// 因超500 上限而**未收入**的条目数。
    pub dropped_over_limit: u32,
    /// 因单条 2MB 上限被拒的条目数。
    pub rejected_oversize: u32,
}

impl MetadataChannel {
    pub fn new() -> Self {
        Self::default()
    }

    /// 告警总数（供判据与遥测聚合）。
    pub fn warnings(&self) -> u32 {
        self.crc_warnings
            + self.inflate_skipped
            + self.keyword_normalized
            + self.keyword_truncated
            + self.dropped_over_limit
            + self.rejected_oversize
    }

    /// 按 keyword 线性查一条（列表语义下**返回首个**，不覆盖不合并）。
    pub fn find(&self, keyword: &str) -> Option<&TextEntry> {
        self.entries.iter().find(|e| e.keyword == keyword)
    }

    /// 按 keyword 收集**全部**同名条目（锚点共存语义的直接暴露）。
    pub fn find_all(&self, keyword: &str) -> Vec<&TextEntry> {
        self.entries
            .iter()
            .filter(|e| e.keyword == keyword)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 四、纯函数：keyword 规整 / UTF-8 校验 / 展示转义
// ---------------------------------------------------------------------------

/// keyword 规整（锚点「keyword 非法字符→规整为下划线保留条目」）。
///
/// **逐字节替换，不改变长度**——保持长度是为了让「替换了哪些位」可由
/// 长度不变这一性质间接校验，也避免规整动作本身成为长度攻击面。
///
/// 返回 `(规整后, 是否非法被改过, 是否超长被裁)`。
pub fn normalize_keyword(raw: &[u8]) -> (String, bool, bool) {
    let mut changed = false;
    let mut out: Vec<u8> = Vec::with_capacity(raw.len().min(KEYWORD_MAX));
    for &b in raw.iter() {
        if b < KEYWORD_CHAR_MIN || b > KEYWORD_CHAR_MAX {
            out.push(REPLACEMENT);
            changed = true;
        } else {
            out.push(b);
        }
    }
    // 超长裁剪（锚点只说 1-79；超限须登记而非静默）
    let truncated = out.len() > KEYWORD_MAX;
    if truncated {
        out.truncate(KEYWORD_MAX);
    }
    // 空 keyword → 规整为 "_"，条目**保留**（锚点是「保留条目」）
    if out.is_empty() {
        out.push(REPLACEMENT);
        return (bytes_to_latin1(&out), true, truncated);
    }
    (bytes_to_latin1(&out), changed, truncated)
}

/// Latin-1 → `String`（逐字节视为码点，**不做 Unicode 折叠**）。
///
/// Latin-1 每字节即一个 Unicode 码点（U+0000..U+00FF），故这是**无损**
/// 映射：与 UTF-8 的「一字节一码点」不同，此处不涉及变长编码。
pub fn bytes_to_latin1(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len());
    for &x in b.iter() {
        // Latin-1 上界0xFF 在 char 范围内，直接构造
        s.push(x as char);
    }
    s
}

/// UTF-8 校验，返回**首个非法序列的字节偏移**（`None` = 全部合法）。
///
/// 自写而非用 `core::str::from_utf8`，原因有二：
///  1. 本模块对外API 不产出 `&str`（见设计要点五），故拿不到可借用的
///     `str`，而 `from_utf8` 需要 `&[u8]`→`&str` 的生命周期绑定；
///  2. 更根本的是**要错误位置**——`from_utf8` 只给一个
///     `Utf8Error`（含 `valid_up_to`，够用），但把它包一层返回偏移
///     比依赖它更直接，且便于在 `no_std` 下完全自持。
///
/// 校验覆盖：过长编码、代理区（U+D800..U+DFFF）、> U+10FFFF、
/// 截断的多字节序列、**续字节出现在首位**。
pub fn validate_utf8(b: &[u8]) -> Option<usize> {
    let n = b.len();
    let mut i = 0usize;
    while i < n {
        let c = b[i];
        if c < 0x80 {
            i += 1;
            continue;
        }
        // 前导字节决定长度。
        //
        // **首字节取值域必须逐段收紧（此处初版有真实漏判，如实留档）**：
        // 初版只写 `0xC0..=0xDF → 2 / 0xE0..=0xEF → 3 / 0xF0..=0xF7 → 4`，
        // 漏掉了两段的**非法子区间**：
        //  · `0xC0`/`0xC1` —— 它们只能编码 U+0000..U+007F，即 **NUL 的
        //    过长编码**（overlong NUL），规范明令禁止。初版让它们落到
        //    「2 字节」分支，而第二字节 `0x80..=0xBF` 在无 `lo` 约束时
        //    被当作合法续字节 → `C0 80` 被判为**合法**（探针实测
        //    `validate_utf8(&[0xC0,0x80]) == None`，是真实缺陷）。
        //  · `0xF5..=0xF7` —— 4 字节序列只能到 U+10FFFF，`F5` 起已越界；
        //    `F8..=0xFF` 更未定义。初版把 `F5` 收进「4 字节」分支，
        //    探针实测 `[F5 80 80 80] == None`（同样漏判）。
        //
        // 修法：把**首字节**的合法取值域写死，越界者直接拒（偏移报在
        // 首字节上，因为它才是问题的根）。第二字节的范围约束仍需保留
        // （防 `E0 80` / `ED A0` / `F0 80` / `F4 90` 这四类「首字节合法
        // 但整体越界」）。
        let need = match c {
            0xC2..=0xDF => 2usize,
            0xE0..=0xEF => 3usize,
            0xF0..=0xF4 => 4usize,
            // 0x80..=0xBF 续字节出现在首位 / 0xC0..0xC1 过长 NUL
            // / 0xF5..=0xFF 超出 U+10FFFF 或未定义
            _ => return Some(i),
        };
        if i + need > n {
            return Some(i); // 截断
        }
        let c1 = b[i + 1];
        // 第二字节必须是续字节；且对 3/4 字节序列，首位的取值范围受限
        // （防过长编码：E0 后不能是 80..9F，F0 后不能是 80..8F）
        let lo = if need == 3 && c == 0xE0 { 0xA0 } else { 0x80 };
        let hi = if need == 3 && c == 0xED {
            0x9F
        } else if need == 4 && c == 0xF4 {
            0x8F
        } else {
            0xBF
        };
        if c1 < lo || c1 > hi {
            return Some(i + 1);
        }
        for k in 2..need {
            let ck = b[i + k];
            if !(0x80..=0xBF).contains(&ck) {
                return Some(i + k);
            }
        }
        i += need;
    }
    None
}

/// 展示转义（锚点「转义非剥离（保真原则）」）。
///
/// 把五个在 HTML 文本节点/属性上下文中有语法含义的字符换成实体：
/// `&`→`&amp;`、`<`→`&lt;`、`>`→`&gt;`、`"`→`&quot;`、`'`→`&#39;`。
///
/// **不剥离、不替换为���**——任何信息都不丢。`&amp;` 可还原为 `&`，
/// 展示了用户文件的真实内容，同时在任何 HTML 上下文里都不可执行。
pub fn escape_for_display(raw: &[u8]) -> String {
    let mut out = String::with_capacity(raw.len());
    for &b in raw.iter() {
        match b {
            b'&' => out.push_str("&amp;"),
            b'<' => out.push_str("&lt;"),
            b'>' => out.push_str("&gt;"),
            b'"' => out.push_str("&quot;"),
            b'\'' => out.push_str("&#39;"),
            _ => {
                // 非 ASCII：Latin-1 视图保持字节语义可还原；
                // 对控制字符用可见转义形态（`\xNN`），避免展示层
                // 混入不可见字符——这是**展示**，不是数据，故可转义为
                // 人类可读形态而不违反「保真」（数据侧仍是原字节）。
                if b < 0x20 || b == 0x7F {
                    out.push_str("\\x");
                    const HEX: &[u8; 16] = b"0123456789ABCDEF";
                    out.push(HEX[(b >> 4) as usize] as char);
                    out.push(HEX[(b & 0x0F) as usize] as char);
                } else {
                    out.push(b as char);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 五、块扫描器（列表语义：收全部，不取首个）
// ---------------------------------------------------------------------------

/// 文本块 FourCC。
pub mod chunk {
    /// 拉丁文本块。
    pub const TEXT: [u8; 4] = *b"tEXt";
    /// 国际文本块。
    pub const ITXT: [u8; 4] = *b"iTXt";
    /// 压缩拉丁文本块。
    pub const ZTXT: [u8; 4] = *b"zTXt";
    /// 块结束。
    pub const IEND: [u8; 4] = *b"IEND";
}

/// 扫到的原始文本块（**全部**，按文件顺序）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RawTextChunks {
    /// `(块种类, 载荷)` 序列，**顺序 = 文件内出现顺序**（列表语义）。
    pub chunks: Vec<(TextKind, Vec<u8>)>,
    /// CRC 校验失败的块数。
    pub crc_warnings: u32,
}

/// 扫描 PNG 的三个文本块（**收全部**，与 F1004 的取首个不同）。
///
/// 边界算术与 F1004 同构且同样走 `checked_add`——`len` 来自 4 字节
/// 不可信字段，32 位目标上裸加法可回绕（详见 `vef04_color::scan_color_chunks`
/// 的头部注释，本处不重复论证）。
pub fn scan_text_chunks(file: &[u8]) -> Result<RawTextChunks, TextFault> {
    if file.len() < 8 || file[..8] != dec::PNG_SIG {
        return Err(TextFault::new(TextFaultKind::BadSignature));
    }
    let mut out = RawTextChunks::default();
    let mut pos = 8usize;
    // 减法形式：避免 `pos + 8` 在 32 位目标上回绕（与 F1004 同因）。
    while file.len() - pos >= 8 {
        let len_raw = u32::from_be_bytes([file[pos], file[pos + 1], file[pos + 2], file[pos + 3]]);
        let len = len_raw as usize;
        let ty = [file[pos + 4], file[pos + 5], file[pos + 6], file[pos + 7]];
        let total = match pos.checked_add(12).and_then(|v| v.checked_add(len)) {
            Some(v) => v,
            None => {
                return Err(TextFault::with(
                    TextFaultKind::ChunkTruncated,
                    len_raw as u64,
                    file.len() as u64,
                ))
            }
        };
        if len > file.len() || total > file.len() {
            return Err(TextFault::with(
                TextFaultKind::ChunkTruncated,
                len_raw as u64,
                file.len() as u64,
            ));
        }
        let payload_end = match pos.checked_add(8).and_then(|v| v.checked_add(len)) {
            Some(v) => v,
            None => {
                return Err(TextFault::with(
                    TextFaultKind::ChunkTruncated,
                    len_raw as u64,
                    file.len() as u64,
                ))
            }
        };
        let data = &file[pos + 8..payload_end];
        let crc_stored = u32::from_be_bytes([
            file[payload_end],
            file[payload_end + 1],
            file[payload_end + 2],
            file[payload_end + 3],
        ]);
        // CRC 覆盖「类型 + 载荷」两段（规范 §5.1）
        let crc_calc = frameledger_ext::crc32(&file[pos + 4..payload_end]);
        if crc_stored != crc_calc {
            out.crc_warnings += 1;
        } else {
            let kind = if ty == chunk::TEXT {
                TextKind::Text
            } else if ty == chunk::ITXT {
                TextKind::InternationalText
            } else if ty == chunk::ZTXT {
                TextKind::CompressedText
            } else {
                TextKind::Text // 占位，立即被下面过滤掉
            };
            if ty == chunk::TEXT || ty == chunk::ITXT || ty == chunk::ZTXT {
                out.chunks.push((kind, data.to_vec()));
            }
        }
        pos = total; // 复用已校验的 total（严格单调前进，杜绝倒退重扫）
        if ty == chunk::IEND {
            break;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 六、三块解析
// ---------------------------------------------------------------------------

/// tEXt 解析（Latin-1：keyword 1-79 + text，**分隔符是单个 NUL**）。
///
/// 规范：tEXt 载荷 = `keyword` `0x00` `text`，**只有一段NUL 分隔**
/// （对比 iTXt 的多段 NUL 分隔，锚点「四段结构」）。
///
/// 返回 `(keyword原文, text)`。keyword 规整交由调用方统一做（与iTXt
/// 同路径，保证三个块的规整口径完全一致）。
fn parse_text_payload(d: &[u8]) -> Result<(&[u8], &[u8]), TextFault> {
    let nul = match d.iter().position(|&b| b == 0) {
        Some(i) => i,
        // 无NUL：整段都是 keyword，text 为空。
        // 规范要求必须有 NUL，但**判它非法就丢条目**过重——按「保留条目」
        // 的口径，把整段当 keyword、文本留空。
        None => return Ok((d, &[])),
    };
    Ok((&d[..nul], &d[nul + 1..]))
}

/// zTXt 解析结果（**自有所有权**，与 [`ITxtParsed`] 同理避免借用源歧义）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ZTxtParsed {
    /// keyword 原文。
    pub keyword: Vec<u8>,
    /// 解压后的明文文本。
    pub text: Vec<u8>,
}

/// zTXt 解析（压缩 Latin-1）：载荷 = `keyword` `0x00` `压缩方法` `压缩文本`。
///
/// 锚点「压缩走 zlib」——方法字段只认 0（deflate）。
fn parse_ztxt_payload(d: &[u8]) -> Result<ZTxtParsed, TextFault> {
    let nul = match d.iter().position(|&b| b == 0) {
        Some(i) => i,
        // 无 NUL：没有方法字节也没有压缩体 → 条目不可用，但**保留
        // keyword**（锚点是「保留条目」的口径）。
        None => {
            return Ok(ZTxtParsed {
                keyword: d.to_vec(),
                text: Vec::new(),
            })
        }
    };
    let method = match d.get(nul + 1) {
        Some(&m) => m,
        None => return Err(TextFault::new(TextFaultKind::Inflate)),
    };
    if method != 0 {
        return Err(TextFault::with(
            TextFaultKind::CompressionMethod,
            method as u64,
            0,
        ));
    }
    let mut text: Vec<u8> = Vec::new();
    inflate_bounded(&d[nul + 2..], &mut text)?;
    Ok(ZTxtParsed {
        keyword: d[..nul].to_vec(),
        text,
    })
}

/// iTXt 解析结果（**各字段自有所有权**，不跨借用源）。
///
/// **为何不用 `(&[u8], &[u8], &[u8], bool)` 返回签名**
/// （这是编译期逼出来的修正，如实留档）：初版让 `parse_itxt_payload`
/// 既返回 `d` 的切片（keyword / 语言 / 未压缩文本）又返回 `out` 的切片
/// （**压缩文本**），即返回值里混着**两个不同借用源**的引用。Rust 无法
/// 为单个返回类型写出一个统一生命周期（要么全绑 `d`，要么全绑 `out`），
/// 编译器报 E0106。
///
/// 三条路：① 补生命周期参数 `'a` 把两者强行绑在一起 → 但压缩路径的
/// 文本**不在** `d` 里，绑 `d` 会让调用方拿到错的内容；② 让调用方传
/// 缓冲进来自己做偏移换算 → 把偏移算术的责任推给每个调用方，正是
/// 本模块要消灭的那类易错点；③ **本文采用**：解析结果字段自有所有权。
///
/// 顺带修掉一个真实缺陷：初版压缩路径上 `text` 取自 `out.as_slice()`，
/// 而调用方紧接着又要拿 `out` 去装下一条的内容——`out` 被复用会让
/// 已借出的切片指向**下一条**的数据。改为自己拥有后该别名问题消失。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ITxtParsed {
    /// keyword 原文（未规整；规整在通道侧统一做）。
    pub keyword: Vec<u8>,
    /// 语言标签（明文，未压缩部分）。
    pub language: Vec<u8>,
    /// 翻译后的 keyword（明文；规范允许与keyword 同）。
    pub translated_keyword: Vec<u8>,
    /// 文本（未压缩路径取自载荷；压缩路径取自解压产出）。
    pub text: Vec<u8>,
    /// 是否走了压缩路径。
    pub compressed: bool,
}

/// iTXt 解析（UTF-8，锚点「四段结构」）。
///
/// 规范载荷：`keyword` `0x00` `压缩标志(1)` [`压缩方法(1)` `压缩文本`]\
/// `语言标签` `0x00` `翻译keyword` `0x00` `UTF-8文本`
///
/// **压缩标志 = 1 时**才存在「压缩方法 + 压缩文本」两段——这是四段结构
/// 里唯一**变长**的一段，故解析必须先看标志再定位语言标签。
///
/// **四段结构的定位次序**（压缩与非压缩**不同**，故分两条路走）：
///  · 未压缩：`kw NUL flag NUL lang NUL trans NUL text`（三NUL 定界）
///  · 压缩  ：`kw NUL flag method <zlib> NUL lang NUL trans NUL`
///    —— 压缩体是**自定界**的（末尾有 adler32），但规范把语言标签放在
///    **压缩文本之后**，故必须先找到分隔 NUL 才能切出压缩体范围。
fn parse_itxt_payload(d: &[u8]) -> Result<ITxtParsed, TextFault> {
    let nul1 = match d.iter().position(|&b| b == 0) {
        Some(i) => i,
        None => return Err(TextFault::new(TextFaultKind::Utf8Invalid)),
    };
    let flag = match d.get(nul1 + 1) {
        Some(&f) => f,
        None => return Err(TextFault::with(TextFaultKind::CompressionFlag, 0, 1)),
    };
    // 规范：标志只允许 0 或 1。其余值既非「不压缩」也非「压缩」，
    // 无从解释 → 显性拒（而非当成 0，那会静默误解释）。
    if flag > 1 {
        return Err(TextFault::with(
            TextFaultKind::CompressionFlag,
            flag as u64,
            1,
        ));
    }
    let keyword = d[..nul1].to_vec();
    let mut out = ITxtParsed {
        keyword,
        compressed: flag == 1,
        ..ITxtParsed::default()
    };

    if flag == 1 {
        // 压缩：kw NUL 1 method <zlib> NUL lang NUL trans NUL[无裸文本]
        let method = match d.get(nul1 + 2) {
            Some(&m) => m,
            None => return Err(TextFault::with(TextFaultKind::CompressionMethod, 0, 0)),
        };
        if method != 0 {
            return Err(TextFault::with(
                TextFaultKind::CompressionMethod,
                method as u64,
                0,
            ));
        }
        let rest = &d[nul1 + 3..];
        // **压缩体长度必须靠「试探解压」确定，不能靠「找下一个 NUL」**
        // （此处初版是真实缺陷，如实留档）。
        //
        // 初版写 `rest.iter().position(|&b| b == 0)` 找分隔 NUL，理由是
        // 「zlib 流自定界」。**这个前提是错的**：zlib 流**内部可以含NUL
        // 字节**——实测 `zlib_stored(b"abc\0def\0ghi")` 产出的流里NUL
        // 出现在偏移 4/10/14（其中 4 是 stored 块头的 LEN 低字节）。
        // 于是任何**文本含 NUL 的压缩 iTXt 都会被切错**：切出的短流
        // 解压失败 → 该条目被跳过。判据 `C05-ITXT-03`（压缩路径解压出
        // 原文）实测就是被这一条挡住的。
        //
        // 正解：zlib 流末尾是 4 字节 adler32（大端），而 inflate 会**校验
        // adler**。故「能通过 adler 校验的最短前缀」即为流的确切长度。
        // [`split_zlib_prefix`] 用线性试探求出该长度与后续字节的起点。
        let (zlen, after_start) = match split_zlib_prefix(rest) {
            Some(v) => v,
            None => return Err(TextFault::new(TextFaultKind::Inflate)),
        };
        let mut text: Vec<u8> = Vec::new();
        inflate_bounded(&rest[..zlen], &mut text)?;
        let after = &rest[after_start..];
        let lang_nul = match after.iter().position(|&b| b == 0) {
            Some(i) => i,
            None => return Err(TextFault::with(TextFaultKind::Utf8Invalid, 0, 0)),
        };
        out.language = after[..lang_nul].to_vec();
        let after2 = &after[lang_nul + 1..];
        let tr_nul = match after2.iter().position(|&b| b == 0) {
            Some(i) => i,
            None => return Err(TextFault::with(TextFaultKind::Utf8Invalid, 0, 0)),
        };
        out.translated_keyword = after2[..tr_nul].to_vec();
        out.text = text;
        return Ok(out);
    }

    // 未压缩：kw NUL 0 NUL lang NUL trans NUL text
    let rest = &d[nul1 + 2..];
    let lang_nul = match rest.iter().position(|&b| b == 0) {
        Some(i) => i,
        None => return Err(TextFault::with(TextFaultKind::Utf8Invalid, 0, 0)),
    };
    out.language = rest[..lang_nul].to_vec();
    let after2 = &rest[lang_nul + 1..];
    let tr_nul = match after2.iter().position(|&b| b == 0) {
        Some(i) => i,
        None => return Err(TextFault::with(TextFaultKind::Utf8Invalid, 0, 0)),
    };
    out.translated_keyword = after2[..tr_nul].to_vec();
    out.text = after2[tr_nul + 1..].to_vec();
    Ok(out)
}

/// 求出 zlib 流的**确切长度**与其后字节的起始下标。
///
/// 返回 `(zlib 流长度, 后续字节起点)`；后续字节起点已**跳过分隔 NUL**。
/// 无法定界时返回 `None`。
///
/// ## 为什么必须试探而不能扫NUL
///
/// zlib（RFC1950）流的**内部允许出现 NUL 字节**：`stored` 块的长度字段
/// 低字节、动态霍夫曼表的字节流都可能为 0。实测
/// `zlib_stored(b"abc\0def\0ghi")` 产出的流在偏移 4/10/14 处都有 NUL。
/// 故「找下一个 NUL 即流尾」是错的（该错误曾真实存在于本模块）。
///
/// ## 试探原理
///
/// zlib 流的最后 4 字节是 **adler32（大端校验和）**，而本仓库的
/// [`mech_inflate::zlib_inflate_slices`] **会校验它**。于是：
///「能被解压成功（含 adler 校验通过）的前缀」里，**最短**的那个就是
/// 流的确切边界——更短的前缀缺字节（`Truncated`），更长的前缀会让
/// inflate 把尾部多余字节当作 adler（校验必然不符，故也失败）。
///
/// 试探长度**线性递增**（步长 [`ZLIB_PROBE_STEP`]），命中后**二分收缩**
/// 到最短可解前缀。
///
/// ## 为何是「线性 + 二分」而非纯指数（初版写纯指数，是真实缺陷）
///
/// 纯指数增长（11 → 22 → 44 …）**会跳过真实边界**：实测一条 23 字节的
/// 压缩流，11 失败、22 失败、**44 才试**——若输入总长恰好 ≥44，则首次
/// 命中的probe 是 44 而不是 23，于是切出的压缩体多带了 21 字节，
/// `after` 起点错位 → 语言标签/文本全乱。反之若输入总长 <44，压根
/// 试不到 23，直接判失败。故纯指数**既可能跳过边界、又可能永远试不到**
/// 正确边界。
///
/// 正解：线性步进保证**不跳过**任何长度；命中后用**二分**把区间
/// `[last_fail, probe]` 收缩到最短可解点，把试探次数从 O(n/step) 降到
/// O(n/step + log n)。对 2MB 上限、256 字节步长，最坏约 8000 次尝试——
/// 这个量级对「每文件仅几个文本块」的场景可接受，且 [`inflate_bounded`]
/// 内部还有渐进扩容兜底。
fn split_zlib_prefix(rest: &[u8]) -> Option<(usize, usize)> {
    /// 试探起点：zlib 头 2 + 最小空 stored 块 5 + adler 4。
    const ZLIB_MIN_PROBE: usize = 11;
    /// 线性步进步长。
    const ZLIB_PROBE_STEP: usize = 256;
    /// 定位缓冲上限：与单条 2MB 上限同口径。
    const ZLIB_PROBE_BUF_MAX: usize = TEXT_MAX_BYTES + 1;

    if rest.len() < ZLIB_MIN_PROBE {
        return None;
    }

    /// 在给定前缀长度上试解压。返回 `Some(true)` = 可解，
    /// `Some(false)` = 明确失败（前缀不足/adler 不符），`None` = 产出
    /// 超定位缓冲（需加长试探并放大缓冲）。
    fn try_probe(prefix: &[u8]) -> Option<bool> {
        let want_buf = if prefix.len() < ZLIB_PROBE_BUF_MAX {
            // 产出上界不超过压缩输入的合理放大，取 min(前缀长, 2MB+1)；
            // 前缀很小时给2 倍余量避免误判Overflow。
            prefix.len().saturating_mul(2).min(ZLIB_PROBE_BUF_MAX)
        } else {
            ZLIB_PROBE_BUF_MAX
        };
        let mut buf: Vec<u8> = vec![0u8; want_buf.max(64)];
        match mech_inflate::zlib_inflate_slices(&[prefix], &mut buf) {
            Ok(_) => Some(true),
            Err(mech_inflate::PngError::Inflate(mech_inflate::InflateErr::Overflow)) => None,
            Err(_) => Some(false),
        }
    }

    // 阶段一：线性步进，找到首个可解长度
    //
    // **不变式**：`last_fail` 长度已验证**失败**；探测长度恒钳在
    // `[ZLIB_MIN_PROBE, rest.len()]`闭区间内。
    //
    // **越界panic 的真实成因（如实留档）**：初版用 `probe += STEP` 递增
    // 后再判`probe > rest.len()`，于是当 `rest.len()` 不落在步长网格上
    // 时（如 len=42、步长 256），`probe` 会从 11 直接跳到 267 —— 此时
    // `probe - 1 = 266` 仍**大于** `rest.len()`，`&rest[..266]` 当场
    // panic（range end out of range）。判据跑全套语料时必崩。
    //
    // 修法：**先钳后试**。每轮把 `probe` 收敛到 `min(probe, rest.len())`，
    // 且「已试过的长度」用 `last_fail` 记录，保证不重复试同一长度，
    // 从而既不越界也不漏长度（不漏是承重的——漏了正确边界就找不到流尾）。
    //
    // **「不漏」需要尾部余数也被试到**：线性步进 11 → 267 → 523 … 只试
    // 落在网格上的长度。若真实流长 42，它既不在网格上、也不会被循环试到
    // （11 失败后直接跳到 267 > 42 而退出）→ **永远找不到流尾**。
    // 故循环退出前必须补试**恰好 `rest.len()`** 这一长度。
    let mut last_fail = 0usize; // 已验证失败的最大长度（0 = 尚未试过）
    let mut probe = ZLIB_MIN_PROBE;
    let mut hit: Option<usize> = None;
    while probe <= rest.len() {
        match try_probe(&rest[..probe]) {
            Some(true) => {
                hit = Some(probe);
                break;
            }
            Some(false) | None => {
                last_fail = probe;
                let next = probe + ZLIB_PROBE_STEP;
                if next <= probe {
                    break; // 溢出防护
                }
                probe = next;
            }
        }
    }
    // 尾部余数补试：真实流长往往**不落在步长网格上**（如27字节的流遇上
    // 256 步长：11 失败 → 267 已超输入 → 循环退出）。
    //
    // **初版只补试了 `rest.len()` 一个长度（真实缺陷）**：那等于「假设流
    // 一直延伸到载荷末尾」，而 iTXt 的压缩文本**后面还有** NUL + 语言标签
    // + NUL + 翻译 keyword + NUL。对 len=36、真实流=27 的情形，
    // 试36 必然因尾部多 9 字节导致 adler 不符而失败 → 返回 None →
    // 该条目被跳过（判据 `C05-ITXT-03` 转红的直接原因）。
    //
    // 修法：余数区间 `[last_fail + 1, rest.len()]` 逐长度线性扫完。
    // 该区间长度 ≤ 步长（256），故最坏多 256 次试探，且只在「网格未命中」
    // 时才付这个成本。
    if hit.is_none() {
        let mut n = last_fail + 1;
        while n <= rest.len() {
            if try_probe(&rest[..n]) == Some(true) {
                hit = Some(n);
                break;
            }
            n += 1;
        }
    }

    let mut hi = match hit {
        Some(h) => h,
        None => return None,
    };
    // 阶段二：二分收缩到最短可解前缀
    //
    // 不变式：`last_fail` 长度必失败，`hi` 长度必成功。
    let mut lo = last_fail;
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        match try_probe(&rest[..mid]) {
            Some(true) => hi = mid,
            Some(false) => lo = mid,
            None => {
                // mid 缓冲不足 → 更长的 hi 才可用，无法在下方收缩。
                // 保守取 hi（宁多不少：多出的字节会在 inflate 时因
                // adler 不符而失败，但 split 已定位，不会走到那）。
                break;
            }
        }
    }
    // 后续字节起点 = 流尾**之后**的那 1 字节（即分隔 NUL 本身）。
    //
    // **这里初版返回 `(hi, hi)`，是一个 off-by-one（真实缺陷）**：
    // `hi` 指向的字节是「紧随压缩流的分隔 NUL」，故`after = &rest[hi..]`
    // 仍以那个 NUL 开头 → 解析语言标签时 `position(|&b| b == 0)` 命中
    // 下标 0 → `language` 恒为空串。表现是「压缩文本对、语言标签空」。
    //
    // 该缺陷之所以能活到判据才被抓出，是因为**它不产生任何错误**：
    // 语言标签在规范里**允许为空**（`C05-ITXT-01` 用未压缩语料断言非空，
    // 压缩路径无人断言）。这正是「只有真值路径才暴露的 off-by-one」。
    //
    // 判据侧靠 `C05-ITXT-03` 断`language == "zh-CN"` 才抓到——**该断言
    // 不可省**，它正是这条缺陷的唯一防线。
    Some((hi, hi + 1))
}

/// 有界解压：产出超 [`TEXT_MAX_BYTES`] 即拒（锚点「单条 2MB 上限」）。
///
/// **渐进扩容**（与 F1004 的 8MB 闸同款理由）：一次起步 `TEXT_MAX_BYTES`
/// 会让 4KB 的小条目也占 2MB 瞬时内存（内核侧不可接受）。
///
/// 容量取 `TEXT_MAX_BYTES + 1`：多出的 1 字节是「恰好超限」与「恰好等于
/// 上限」的唯一可分辨标志——若缓冲恰好只有 `TEXT_MAX_BYTES`，两种情形
/// 都会让解压报溢出，二者被混成同一种「解压失败」，上限闸退化成
/// 「只会拒不会放」的弱门禁。
fn inflate_bounded(zdata: &[u8], out: &mut Vec<u8>) -> Result<(), TextFault> {
    const PROBE_MIN_BYTES: usize = 64 * 1024;
    out.clear();
    let mut cap = PROBE_MIN_BYTES.min(TEXT_MAX_BYTES + 1);
    loop {
        let mut buf: Vec<u8> = vec![0u8; cap];
        match mech_inflate::zlib_inflate_slices(&[zdata], &mut buf) {
            Ok(n) => {
                // n <= TEXT_MAX_BYTES + 1；用 n 判定真实产出
                if n > TEXT_MAX_BYTES {
                    return Err(TextFault::with(
                        TextFaultKind::TooLarge,
                        n as u64,
                        TEXT_MAX_BYTES as u64,
                    ));
                }
                buf.truncate(n);
                *out = buf;
                return Ok(());
            }
            Err(mech_inflate::PngError::Inflate(mech_inflate::InflateErr::Overflow)) => {
                if cap >= TEXT_MAX_BYTES + 1 {
                    // 封顶缓冲仍溢出⇒ 产出必然超上限（不是别的）
                    return Err(TextFault::with(
                        TextFaultKind::TooLarge,
                        (TEXT_MAX_BYTES + 1) as u64,
                        TEXT_MAX_BYTES as u64,
                    ));
                }
                cap = (cap * 2).min(TEXT_MAX_BYTES + 1);
            }
            Err(_) => return Err(TextFault::new(TextFaultKind::Inflate)),
        }
    }
}

// ---------------------------------------------------------------------------
// 七、统一解析：产出元数据通道
// ---------------------------------------------------------------------------

/// 解析全部文本块，产出元数据通道（**单一出口**，锚点「三块统一解析产出」）。
///
/// 错误处置口径（锚点「错误路径」）：
///  · 解压失败 → **跳过该条并计数**（`inflate_skipped += 1`），不阻断通道；
///  · UTF-8 非法 → 跳过该条并计数（iTXt 专属，同属「该条不可用」）；
///  · keyword 非法字符 → **规整为下划线保留条目**（不跳过）；
///  · 重复 keyword → **共存不覆盖**（列表语义）。
///
/// **为何解压失败「跳过」而不是「整图失败」**：元数据是**附加**信息，
/// 一条坏文本不该让像素解不出来——这与 F1004 的「坏 ICC 不阻断」同构。
/// 但它**必须计数**（`inflate_skipped`），否则下游无从知道有信息丢失。
pub fn parse_metadata(file: &[u8]) -> Result<MetadataChannel, TextFault> {
    let raw = scan_text_chunks(file)?;
    let mut ch = MetadataChannel::new();
    ch.crc_warnings = raw.crc_warnings;

    for (kind, payload) in raw.chunks.iter() {
        // 条数闸：超500 即拒后续并计数（**不静默截断**）
        if ch.entries.len() >= ENTRY_MAX_COUNT {
            ch.dropped_over_limit += 1;
            continue;
        }
        let parsed: Result<(Vec<u8>, String, Vec<u8>, Compressed), TextFault> = match kind {
            TextKind::Text => match parse_text_payload(payload) {
                Ok((kw, txt)) => Ok((
                    kw.to_vec(),
                    String::new(),
                    txt.to_vec(),
                    Compressed::None,
                )),
                Err(e) => Err(e),
            },
            TextKind::CompressedText => match parse_ztxt_payload(payload) {
                Ok(z) => Ok((
                    z.keyword,
                    String::new(),
                    z.text,
                    Compressed::WasCompressed,
                )),
                Err(e) => Err(e),
            },
            TextKind::InternationalText => match parse_itxt_payload(payload) {
                Ok(it) => {
                    // iTXt 文本须 UTF-8 合法（**压缩路径也要校**——
                    // 解压出来的字节同样受 UTF-8 约束）。
                    //
                    // **这里初版写的是 `return Err(...)`，是真实缺陷**：
                    // `return` 会把**整条通道**连同此前已解析的条目一起
                    // 丢掉，与锚点「UTF-8/解压失败→**跳过该条**并计数」
                    // 的口径相反——一条坏文本会让此前所有条目消失。
                    // 现改为把校验失败表达成 `Err`，交由下方统一「跳过
                    // 该条并计数」处置。
                    match validate_utf8(&it.text) {
                        Some(off) => Err(TextFault::with(
                            TextFaultKind::Utf8Invalid,
                            off as u64,
                            it.text[off] as u64,
                        )),
                        None => Ok((
                            it.keyword,
                            bytes_to_latin1(&it.language),
                            it.text,
                            if it.compressed {
                                Compressed::WasCompressed
                            } else {
                                Compressed::None
                            },
                        )),
                    }
                }
                Err(e) => Err(e),
            },
        };

        let (kw_raw, lang, text, compressed) = match parsed {
            Ok(v) => v,
            Err(e) => {
                // 解压失败 / UTF-8 非法 → 跳过该条并计数（锚点）
                if e.kind == TextFaultKind::Inflate
                    || e.kind == TextFaultKind::TooLarge
                    || e.kind == TextFaultKind::Utf8Invalid
                {
                    if e.kind == TextFaultKind::TooLarge {
                        ch.rejected_oversize += 1;
                    } else {
                        ch.inflate_skipped += 1;
                    }
                    continue;
                }
                // 其余（压缩方法错、压缩标志错）同样只跳过该条——
                // 它们都是「这条不可用」而非「文件不可用」。
                ch.inflate_skipped += 1;
                continue;
            }
        };

        // 单条 2MB 上限（锚点）：**压缩态按解压后长度算**（压缩前小、
        // 解压后大是炸弹的典型形态）。
        if text.len() > TEXT_MAX_BYTES {
            ch.rejected_oversize += 1;
            continue;
        }

        let (kw_norm, was_changed, was_trunc) = normalize_keyword(&kw_raw);
        if was_changed {
            ch.keyword_normalized += 1;
        }
        if was_trunc {
            ch.keyword_truncated += 1;
        }

        ch.entries.push(TextEntry {
            keyword: kw_norm,
            language: lang,
            text,
            compressed,
            kind: *kind,
        });
    }
    Ok(ch)
}

// ---------------------------------------------------------------------------
// 八、元数据通道对接（CGPU-F0069）
// ---------------------------------------------------------------------------

/// 元数据通道的对外消费视图（锚点「通道侧只做存储与展示」）。
///
/// **本类型不含任何执行/渲染能力**——只有展示所需的转义文本。
/// 这是锚点「元数据文本**不直接执行**」的类型级保证：下游拿到
/// [`DisplayEntry::escaped`] 只能得到**已转义**的字符串，拿不到原始可执行
/// 片段的展示入口。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DisplayEntry {
    /// 规整后的 keyword（已转义，可直接入 HTML）。
    pub keyword: String,
    /// 语言标签（已转义）。
    pub language: String,
    /// **已转义**的文本（展示安全）。
    pub escaped: String,
    /// 原始字节长度（供下游显示「展开」等 UI 决策）。
    pub raw_len: usize,
    /// 是否曾以压缩形式存储。
    pub was_compressed: bool,
}

/// 把通道转为展示视图（逐条转义，锚点「展示层转义」）。
pub fn to_display_view(ch: &MetadataChannel) -> Vec<DisplayEntry> {
    ch.entries
        .iter()
        .map(|e| DisplayEntry {
            keyword: escape_for_display(e.keyword.as_bytes()),
            language: escape_for_display(e.language.as_bytes()),
            escaped: escape_for_display(&e.text),
            raw_len: e.text.len(),
            was_compressed: e.compressed == Compressed::WasCompressed,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 九、故障码登记表（码位只增不回收）
// ---------------------------------------------------------------------------

/// 故障码登记（`[名称, 码, 可达性]`）。
///
/// **可达性一栏是纪律**：有 code 不等于可达——诊断变体若无真实产生
/// 路径即为死码。这里逐条标注**产生路径在哪个函数**，防止后人误以为
/// 某码永远不会被返回。
pub const FAULT_REGISTRY: [(&str, u8, &str); 8] = [
    ("BadSignature", 1, "parse_metadata/scan_text_chunks 入口"),
    ("ChunkTruncated", 2, "scan_text_chunks 三处 checked_add 与长度校验"),
    ("Utf8Invalid", 3, "parse_itxt_payload 未压缩路径 + 通道侧压缩路径"),
    ("CompressionFlag", 4, "parse_itxt_payload 标志>1"),
    ("CompressionMethod", 5, "parse_ztxt_payload 与 parse_itxt_payload 方法≠0"),
    ("Inflate", 6, "inflate_bounded 非Overflow 分支"),
    ("TooLarge", 7, "inflate_bounded 封顶溢出 + 通道侧长度闸"),
    ("TooManyEntries", 8, "保留码位：当前由 dropped_over_limit 计数承担，未产出Err"),
];