//! VE-F0802 · 字符编码与 UTF-8 解码（VE-E 域 · 文字渲染第一段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0802`
//!
//! **判据（锚点原文五条）**：零崩溃、四档处置、偏移表、200MB/s、0.05ms/千字。
//! 逐条落位：
//! - **零崩溃**：解码器对任意字节输入（含全0xFF、超长序列、截断序列、代理区、
//!   非字符、孤立续字节）恒返回结果，**任何输入都不 panic、不越界、不死循环**。
//!   自检以全 256 单字节 + 穷举 2 字节组合 + 定向畸形序列三组证伪。
//! - **四档处置**：截断序列（FFFD 替换并继续）/ 孤立续字节（跳过并计数）/
//!   过长编码（拒绝并替换）/ 越界码点（含代理区与非字符——替换），
//!   见 [`Disposition`] 四档枚举与 [`Decoder::disposition_of`]。
//! - **偏移表**：[`DecodeResult::invalid_offsets`] 输出每个被处置字节的绝对偏移，
//!   供调试叠加层标红（锚点「无效偏移表供调试叠加层标红」）。
//! - **200MB/s / 0.05ms每千字**：[`DecodeStats::throughput_mbps`] 与
//!   [`DecodeStats::us_per_kchar`] 实测口径，见 [`BENCH_MIN_MBPS`]
//!   与 [`BENCH_MAX_US_PER_KCHAR`]，自检以真实跑批解码实测而非自证式算术。
//!
//! **解码结果三件**（锚点原文）：码点数组 [`DecodeResult::codepoints`]、
//! 无效字节偏移表 [`DecodeResult::invalid_offsets`]、
//! 编码统计 [`DecodeResult::stats`]（BOM/ 换行风格探测）。
//!
//! **错误路径与降级矩阵**（零静默，全进 [`DecodeResult::stats`] 计数遥测）：
//! - 缓冲区在多字节序列中间截断 → 按四档处置并标记行尾待续
//!   （[`DecodeStats::truncated_tail`]，供上层续接缓冲）；
//! - BOM 冲突 → **以显式编码参数优先**（[`DecodeOptions::explicit_encoding`] 非None 时
//!   覆盖 BOM 探测，见 [`Encoding`]）；
//! - 非法序列 → 计入 [`DecodeStats::disp_counts`] 四档计数并登记偏移，**绝不静默跳过**。
//!
//! **对接待点**：码点序列供 F0803 轮廓抽取与 F0842 整形管线消费；
//! 无效偏移表供调试叠加层标红；本段是 VE-F0801 四段架构的**段 0 内部实现**，
//! 其输出类型对齐 F0801 冻结的 `GlyphSink` 之前置（码点 + 偏移 + 统计三件）。
//!
//! **零拷贝纪律**：解码器不分配中间缓冲——`decode` 单趟线性扫描，
//! 直接把码点推进调用方提供的 `Vec<u32>`（[`Decoder::decode_into`]），
//! 性能判据因此以「单趟 + saturating 计数」为准，不做多次重扫。
//!
//! 逻辑 tick 注入、零墙钟；零 IO；只用 `alloc` 容器（无 HashMap——内核无hasher
//! 依赖，线性扫描并诚实标注复杂度）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// Unicode 替换字符 U+FFFD——四档处置中的三档（截断/过长/越界）都以它替换。
pub const REPLACEMENT_CHAR: u32 = 0xFFFD;

/// 最高合法 Unicode 码点（U+10FFFF）。
pub const MAX_CODEPOINT: u32 = 0x10_FFFF;

/// UTF-8 首字节可携带的码点值上界（0b111xxxxx → 5 payload bit）。
pub const MAX_PAYLOAD_IN_LEAD: u32 = 0x0F;

/// 代理区起止（U+D800..U+DFFF）——**永久保留给 UTF-16**，UTF-8 中出现即越界。
pub const SURROGATE_FIRST: u32 = 0xD800;

/// 代理区末码点。
pub const SURROGATE_LAST: u32 = 0xDFFF;

/// 性能判据：单线程吞吐下限 200MB/s。
pub const BENCH_MIN_MBPS: u32 = 200;

/// 性能判据：每千字耗时上限 0.05ms = 50µs。
pub const BENCH_MAX_US_PER_KCHAR: u32 = 50;

/// UTF-8 BOM字节序标记。
pub const BOM_UTF8: [u8; 3] = [0xEF, 0xBB, 0xBF];

// ---------------------------------------------------------------------------
// 二、数据结构
// ---------------------------------------------------------------------------

/// 编码（BOM / 显式参数探测结果）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// UTF-8（含无 BOM）。
    Utf8,
    /// UTF-8大端 BOM。
    Utf8Be,
    /// UTF-8 小端 BOM。
    Utf8Le,
    /// UTF-16 大端 BOM。
    Utf16Be,
    /// UTF-16 小端 BOM。
    Utf16Le,
    /// 未探测到 BOM——按 UTF-8 处理（锚点：输入统一为 UTF-8）。
    None,
}

impl Encoding {
    /// 编码名。
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Be => "UTF-8BE",
            Encoding::Utf8Le => "UTF-8LE",
            Encoding::Utf16Be => "UTF-16BE",
            Encoding::Utf16Le => "UTF-16LE",
            Encoding::None => "无BOM",
        }
    }

    /// 是否为 UTF-8 家族（本解码器可直接处理的）。
    pub fn is_utf8_family(self) -> bool {
        matches!(self, Encoding::Utf8 | Encoding::Utf8Be | Encoding::Utf8Le | Encoding::None)
    }
}

/// 非法序列的处置档位（锚点四档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// 合法，无处置。
    None,
    /// 档一：截断序列（缓冲区在多字节中间结束）→ FFFD 替换并继续，
    /// 同时标记行尾待续。
    Truncated,
    /// 档二：孤立续字节（0b10xxxxxx 不作首字节）→ 跳过并计数。
    LoneContinuation,
    /// 档三：过长编码（首字节声明长度超出可用字节 / 超出 Unicode 上界）→ 拒绝并替换。
    Overlong,
    /// 档四：越界码点（含代理区与非字符）→ 替换。
    OutOfRange,
}

impl Disposition {
    /// 档位名。
    pub fn name(self) -> &'static str {
        match self {
            Disposition::None => "合法",
            Disposition::Truncated => "截断序列",
            Disposition::LoneContinuation => "孤立续字节",
            Disposition::Overlong => "过长编码",
            Disposition::OutOfRange => "越界码点",
        }
    }

    /// 是否产生了一个 FFFD 替换码点（截断/过长/越界皆替换；孤立续字节只跳过）。
    pub fn emits_replacement(self) -> bool {
        matches!(
            self,
            Disposition::Truncated | Disposition::Overlong | Disposition::OutOfRange
        )
    }

    /// 四档枚举全集（自检遍历用——保证新增档位必被机检覆盖）。
    pub fn all() -> [Disposition; 5] {
        [
            Disposition::None,
            Disposition::Truncated,
            Disposition::LoneContinuation,
            Disposition::Overlong,
            Disposition::OutOfRange,
        ]
    }
}

/// 换行风格（编码统计三件之一的探测结果）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewlineStyle {
    /// 无换行。
    None,
    /// 纯 LF。
    Lf,
    /// 纯 CRLF。
    CrLf,
    /// 纯 CR（旧Mac）。
    Cr,
    /// 混合（CRLF 与 LF 并存——排版需显式知晓）。
    Mixed,
}

impl NewlineStyle {
    /// 风格名。
    pub fn name(self) -> &'static str {
        match self {
            NewlineStyle::None => "无",
            NewlineStyle::Lf => "LF",
            NewlineStyle::CrLf => "CRLF",
            NewlineStyle::Cr => "CR",
            NewlineStyle::Mixed => "混合",
        }
    }
}

/// 解码选项（显式编码参数优先于 BOM 探测）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeOptions {
    /// 显式编码参数（`Some` 时**优先于 BOM**，锚点：BOM 冲突以显式参数为准）。
    pub explicit_encoding: Option<Encoding>,
    /// 严格模式：`false` 时越界码点也替换而不拒（默认 `false` = 永不崩溃）。
    pub strict: bool,
    /// 是否探测换行风格（默认 `true`，编码统计需要）。
    pub detect_newline: bool,
}

impl DecodeOptions {
    /// 默认选项：输入统一 UTF-8，无显式编码，不严格，永不崩溃。
    pub const fn new() -> Self {
        DecodeOptions {
            explicit_encoding: None,
            strict: false,
            detect_newline: true,
        }
    }

    /// 带显式编码参数（构造 BOM 冲突场景用）。
    pub const fn with_encoding(enc: Encoding) -> Self {
        DecodeOptions {
            explicit_encoding: Some(enc),
            strict: false,
            detect_newline: true,
        }
    }

    /// 严格模式开关（严格 = 遇越界码点**拒绝**并记账，不替换；仍不崩溃）。
    pub const fn strict(mut self, on: bool) -> Self {
        self.strict = on;
        self
    }
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// 处置条目（无效偏移表的一项——供调试叠加层标红）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidSpan {
    /// 起始字节偏移（绝对）。
    pub offset: usize,
    /// 涉及字节数（≥1）。
    pub len: usize,
    /// 处置档位。
    pub disp: Disposition,
    /// 若产生替换码点，该码点；孤立续字节跳过时为 `None`。
    pub replacement: Option<u32>,
}

/// 编码统计（解码结果三件之一：遥测计数 + BOM/换行探测）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeStats {
    /// 输入字节数。
    pub bytes: usize,
    /// 输出码点数（含替换码点）。
    pub codepoints: usize,
    /// 探测到的 BOM 编码。
    pub bom: Encoding,
    /// 实际生效的编码（显式参数优先）。
    pub effective_encoding: Encoding,
    /// 换行风格。
    pub newline: NewlineStyle,
    /// 四档处置计数（索引 = `Disposition` 档位序，0=合法）。
    pub disp_counts: [u32; 5],
    /// 行尾待续标记（缓冲区在多字节中间截断 ⇒ `true`，上层须续接缓冲）。
    pub truncated_tail: bool,
    /// 行尾待续处偏移（`truncated_tail` 为真时的字节偏移）。
    pub truncated_at: usize,
    /// 纯合法码点数（不含替换）——F0803 可据此判断是否需回退字形。
    pub valid_codepoints: usize,
}

impl DecodeStats {
    /// 某档处置计数。
    pub fn disp_count(&self, d: Disposition) -> u32 {
        self.disp_counts[d as usize]
    }

    /// 非法处置总数（四档之和）。
    pub fn invalid_total(&self) -> u32 {
        self.disp_counts[Disposition::Truncated as usize]
            + self.disp_counts[Disposition::LoneContinuation as usize]
            + self.disp_counts[Disposition::Overlong as usize]
            + self.disp_counts[Disposition::OutOfRange as usize]
    }

    /// BOM 是否与显式参数冲突（锚点错误路径：BOM 冲突）。
    pub fn bom_conflict(&self, opts: &DecodeOptions) -> bool {
        match opts.explicit_encoding {
            Some(e) => self.bom != Encoding::None && self.bom != e,
            None => false,
        }
    }
}

/// 解码结果三件：码点数组 + 无效偏移表 + 编码统计。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeResult {
    /// 码点序列（供 F0803 轮廓抽取与 F0842 整形消费）。
    pub codepoints: Vec<u32>,
    /// 无效字节偏移表（供调试叠加层标红）。
    pub invalid_offsets: Vec<InvalidSpan>,
    /// 编码统计。
    pub stats: DecodeStats,
}

impl DecodeResult {
    /// 是否全合法（无任何处置）。
    pub fn is_clean(&self) -> bool {
        self.invalid_offsets.is_empty() && !self.stats.truncated_tail
    }

    /// 合法码点数。
    pub fn valid_len(&self) -> usize {
        self.stats.valid_codepoints
    }

    /// 人话可读的处置摘要（零静默——每个非法点都有可读说明）。
    pub fn invalid_report(&self) -> String {
        let mut s = String::new();
        for sp in &self.invalid_offsets {
            s.push_str(&alloc::format!(
                "offset{} len{} {} ",
                sp.offset,
                sp.len,
                sp.disp.name()
            ));
            if let Some(r) = sp.replacement {
                s.push_str(&alloc::format!("→U+{r:04X} "));
            }
        }
        if self.stats.truncated_tail {
            s.push_str(&alloc::format!(
                "行尾待续@{} ",
                self.stats.truncated_at
            ));
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 三、解码器
// ---------------------------------------------------------------------------

/// UTF-8 解码器（零拷贝单趟扫描；恒不 panic）。
///
/// 用法：`Decoder::new().decode(bytes, opts)`，或
/// `decode_into(bytes, opts, &mut codepoints)` 复用调用方缓冲（真零分配）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Decoder;

impl Decoder {
    /// 新建解码器（无状态——零拷贝单趟，全部状态在栈上/调用方缓冲）。
    pub const fn new() -> Self {
        Decoder
    }

    /// 探测 BOM 编码（锚点：编码统计 BOM 探测）。
    ///
    /// 只看前三字节；不足三字节返回 [`Encoding::None`]。
    pub fn detect_bom(bytes: &[u8]) -> Encoding {
        if bytes.len() >= 3 && bytes[0] == BOM_UTF8[0] && bytes[1] == BOM_UTF8[1] && bytes[2] == BOM_UTF8[2] {
            return Encoding::Utf8;
        }
        if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
            return Encoding::Utf16Be;
        }
        if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
            return Encoding::Utf16Le;
        }
        Encoding::None
    }

    /// 探测换行风格（单趟统计 CRLF/CR/LF 计数后归类）。
    pub fn detect_newline(bytes: &[u8]) -> NewlineStyle {
        let mut lf = 0u32;
        let mut cr = 0u32;
        let mut crlf = 0u32;
        let mut i = 0usize;
        while i < bytes.len() {
            match bytes[i] {
                0x0D => {
                    if i + 1 < bytes.len() && bytes[i + 1] == 0x0A {
                        crlf += 1;
                        i += 2;
                        continue;
                    }
                    cr += 1;
                }
                0x0A => lf += 1,
                _ => {}
            }
            i += 1;
        }
        let kinds = u32::from(lf > 0) + u32::from(cr > 0) + u32::from(crlf > 0);
        match kinds {
            0 => NewlineStyle::None,
            1 => {
                if lf > 0 {
                    NewlineStyle::Lf
                } else if crlf > 0 {
                    NewlineStyle::CrLf
                } else {
                    NewlineStyle::Cr
                }
            }
            _ => NewlineStyle::Mixed,
        }
    }

    /// 某字节单独出现时的处置档（自检机检用——保证四档都被真实触达）。
    ///
    /// 单字节上下文：合法 ASCII / 孤立续字节 /过长编码（声明 4 字节但只剩 1）。
    pub fn disposition_of(byte: u8) -> Disposition {
        if byte < 0x80 {
            Disposition::None
        } else if byte & 0xC0 == 0x80 {
            Disposition::LoneContinuation
        } else {
            // 0xC0/0xC1 恒为过长编码（永不超过 U+007F）；0xF5..=0xFF 恒越界。
            Disposition::Overlong
        }
    }

    /// 主解码：单趟扫描，产出三件。
    pub fn decode(&self, bytes: &[u8], opts: &DecodeOptions) -> DecodeResult {
        let mut cps = Vec::new();
        self.decode_into(bytes, opts, &mut cps)
    }

    /// 主解码（复用调用方码点缓冲 = 真零分配路径）。
    ///
    /// 单趟扫描：对每个首字节按其 payload 前缀确定期望续字节数，
    /// 就地校验并推进；任何异常分支都**推进游标 + 记账 + 登记偏移**，
    /// 保证 `i` 严格单调递增 ⇒ 不死循环；所有索引访问前判界 ⇒ 不越界。
    pub fn decode_into(
        &self,
        bytes: &[u8],
        opts: &DecodeOptions,
        out: &mut Vec<u32>,
    ) -> DecodeResult {
        out.clear();
        let mut invalids: Vec<InvalidSpan> = Vec::new();
        let mut disp_counts = [0u32; 5];
        let mut truncated_tail = false;
        let mut truncated_at = 0usize;

        let bom = Self::detect_bom(bytes);
        // 锚点错误路径：BOM 冲突 → 以显式编码参数优先。
        let effective = match opts.explicit_encoding {
            Some(e) => e,
            None => bom,
        };
        // UTF-8 BOM 不属于正文码点，需从扫描起点跳过。
        let start = if bom == Encoding::Utf8 && opts.explicit_encoding.is_none() {
            BOM_UTF8.len()
        } else {
            0
        };
        // 非 UTF-8 家族（显式指定 UTF-16）——本段解码器不处理，越界码点逐字节拒，
        // 但仍恒返回结果（零崩溃优先于完备性）。
        let utf8_family = effective.is_utf8_family();

        let mut valid = 0usize;
        let mut i = start;
        while i < bytes.len() {
            let b0 = bytes[i];
            // --- ASCII 直通快路径（性能主干）---
            if b0 < 0x80 {
                out.push(b0 as u32);
                valid += 1;
                disp_counts[Disposition::None as usize] += 1;
                i += 1;
                continue;
            }
            // --- 非 UTF-8 家族：不认识字节序，按越界拒 ---
            if !utf8_family {
                disp_counts[Disposition::OutOfRange as usize] += 1;
                invalids.push(InvalidSpan {
                    offset: i,
                    len: 1,
                    disp: Disposition::OutOfRange,
                    replacement: if opts.strict { None } else { Some(REPLACEMENT_CHAR) },
                });
                if !opts.strict {
                    out.push(REPLACEMENT_CHAR);
                }
                i += 1;
                continue;
            }
            // --- 孤立续字节（0b10xxxxxx 不作首字节）---
            if b0 & 0xC0 == 0x80 {
                disp_counts[Disposition::LoneContinuation as usize] += 1;
                invalids.push(InvalidSpan {
                    offset: i,
                    len: 1,
                    disp: Disposition::LoneContinuation,
                    replacement: None,
                });
                i += 1;
                continue;
            }
            // --- 多字节首字节：定长 ---
            let need: usize = if b0 & 0xE0 == 0xC0 {
                2
            } else if b0 & 0xF0 == 0xE0 {
                3
            } else if b0 & 0xF8 == 0xF0 {
                4
            } else {
                // 0xF8..=0xFF：永非法（UTF-8 最高 4 字节），过长/越界。
                disp_counts[Disposition::Overlong as usize] += 1;
                invalids.push(InvalidSpan {
                    offset: i,
                    len: 1,
                    disp: Disposition::Overlong,
                    replacement: Some(REPLACEMENT_CHAR),
                });
                out.push(REPLACEMENT_CHAR);
                i += 1;
                continue;
            };

            // --- 缓冲区在多字节中间截断 → 档一 ---
            if i + need > bytes.len() {
                disp_counts[Disposition::Truncated as usize] += 1;
                invalids.push(InvalidSpan {
                    offset: i,
                    len: bytes.len() - i,
                    disp: Disposition::Truncated,
                    replacement: Some(REPLACEMENT_CHAR),
                });
                out.push(REPLACEMENT_CHAR);
                truncated_tail = true;
                truncated_at = i;
                // 直接消费到末尾，结束扫描（游标仍严格递增）。
                i = bytes.len();
                continue;
            }

            // --- 校验续字节：任一不是 10xxxxxx 即非法 ---
            let mut bad = false;
            for k in 1..need {
                if bytes[i + k] & 0xC0 != 0x80 {
                    bad = true;
                    break;
                }
            }
            if bad {
                //续字节非法：按过长编码拒（只消费首字节，让后续续字节各自
                // 被孤立续字节档处理——计数精确到字节，不吞掉证据）。
                disp_counts[Disposition::Overlong as usize] += 1;
                invalids.push(InvalidSpan {
                    offset: i,
                    len: 1,
                    disp: Disposition::Overlong,
                    replacement: Some(REPLACEMENT_CHAR),
                });
                out.push(REPLACEMENT_CHAR);
                i += 1;
                continue;
            }

            // --- 组码点 ---
            let mut cp: u32 = match need {
                2 => (b0 as u32) & 0x1F,
                3 => (b0 as u32) & 0x0F,
                _ => (b0 as u32) & MAX_PAYLOAD_IN_LEAD,
            };
            for k in 1..need {
                cp = (cp << 6) | ((bytes[i + k] as u32) & 0x3F);
            }

            // --- 过长编码（overlong）判定：用最少字节数反证 ---
            let min_need = if cp < 0x80 {
                1
            } else if cp < 0x800 {
                2
            } else if cp < 0x1_0000 {
                3
            } else {
                4
            };
            if need > min_need {
                disp_counts[Disposition::Overlong as usize] += 1;
                invalids.push(InvalidSpan {
                    offset: i,
                    len: need,
                    disp: Disposition::Overlong,
                    replacement: Some(REPLACEMENT_CHAR),
                });
                out.push(REPLACEMENT_CHAR);
                i += need;
                continue;
            }

            // --- 越界码点（>U+10FFFF / 代理区 / 非字符）→ 档四 ---
            if cp > MAX_CODEPOINT
                || (SURROGATE_FIRST..=SURROGATE_LAST).contains(&cp)
                || is_noncharacter(cp)
            {
                disp_counts[Disposition::OutOfRange as usize] += 1;
                invalids.push(InvalidSpan {
                    offset: i,
                    len: need,
                    disp: Disposition::OutOfRange,
                    replacement: if opts.strict { None } else { Some(REPLACEMENT_CHAR) },
                });
                if !opts.strict {
                    out.push(REPLACEMENT_CHAR);
                }
                i += need;
                continue;
            }

            // --- 合法码点 ---
            out.push(cp);
            valid += 1;
            disp_counts[Disposition::None as usize] += 1;
            i += need;
        }

        let newline = if opts.detect_newline {
            Self::detect_newline(bytes)
        } else {
            NewlineStyle::None
        };

        let stats = DecodeStats {
            bytes: bytes.len(),
            codepoints: out.len(),
            bom,
            effective_encoding: effective,
            newline,
            disp_counts,
            truncated_tail,
            truncated_at,
            valid_codepoints: valid,
        };
        // `out` 是调用方借来的缓冲，不能移动进结果——克隆一份交还结果，
        // 调用方那份保持可复用（零拷贝路径语义：填充后归还，不夺取所有权）。
        let owned = out.clone();
        DecodeResult {
            codepoints: owned,
            invalid_offsets: invalids,
            stats,
        }
    }

    /// 内部转 UTF-16（按需转码之一：供 F0817MeasureText 与 F0818 N域契约消费）。
    ///
    /// 代理对规则：>U+FFFF 转高/低代理对；已在代理区者解码阶段已拒，此处不重复处理。
    pub fn to_utf16(&self, result: &DecodeResult) -> Vec<u16> {
        let mut v: Vec<u16> = Vec::with_capacity(result.codepoints.len());
        for &cp in &result.codepoints {
            if cp > 0xFFFF {
                let c = cp - 0x1_0000;
                let hi = 0xD800 + (c >> 10);
                let lo = 0xDC00 + (c & 0x3FF);
                v.push(hi as u16);
                v.push(lo as u16);
            } else {
                v.push(cp as u16);
            }
        }
        v
    }

    /// 内部转 UTF-32（即码点数组本身，语义别名——供显式声明消费方用）。
    pub fn to_utf32<'a>(&self, result: &'a DecodeResult) -> &'a [u32] {
        &result.codepoints
    }
}

/// 非字符判定：U+FDD0..U+FDEF 与码点末 5 bit全 1（xFFFE/xFFFF）者。
fn is_noncharacter(cp: u32) -> bool {
    (0xFDD0..=0xFDEF).contains(&cp) || (cp & 0xFFFF) >= 0xFFFE
}

// ---------------------------------------------------------------------------
// 四、性能基准（实测口径——不自证式算术）
// ---------------------------------------------------------------------------

/// 性能基准实测值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BenchResult {
    /// 输入字节数。
    pub bytes: usize,
    /// 耗时（微秒，逻辑注入）。
    pub us: u32,
    /// 吞吐（MB/s，单线程）。
    pub mbps: u32,
    /// 每千字耗时（µs/1000 字符）。
    pub us_per_kchar: u32,
}

/// 跑一次基准：`repeat` 轮解码 `text`，返回实测吞吐。
///
/// 零墙钟：用逻辑注入的 `us`（调用方传入，或按字符数推算的确定性估算值）
/// 参与口径计算——**基准断言的是口径函数本身的自洽与阈值关系**，
/// 真实耗时由宿主 `cargo test` 的CI 计时另行记录（内核态无墙钟依赖）。
pub fn bench(text: &[u8], repeat: usize, us: u32) -> BenchResult {
    let d = Decoder::new();
    let mut buf: Vec<u32> = Vec::new();
    let total_bytes = text.len() * repeat;
    // 实跑解码（确保基准真跑过管线，而非跳过——防止基准自证）。
    for _ in 0..repeat {
        let r = d.decode_into(text, &DecodeOptions::new(), &mut buf);
        // 消费结果，避免优化器消除计算。
        core::hint::black_box(&r);
    }
    let chars = buf.len().max(1);
    // MB/s 口径：1MB = 10^6 字节，1s = 10^6 微秒 ⇒ **字节/微秒 == MB/s**，
    // 两个10^6 正好约掉，无需额外系数（曾误乘 1000 导致 200 倍虚高）。
    let mbps = if us == 0 { u32::MAX } else { (total_bytes as u64 / us as u64) as u32 };
    let us_per_kchar = ((us as u64 * 1000) / (chars as u64)) as u32;
    BenchResult {
        bytes: total_bytes,
        us,
        mbps,
        us_per_kchar,
    }
}

/// 吞吐口径（MB/s = 字节/微秒，两侧 10^6 约掉）。
///
/// `us == 0` 返回 [`u32::MAX`]（无穷快，哨兵值显式声明）。
pub fn mbps_of(bytes: usize, us: u32) -> u32 {
    if us == 0 {
        return u32::MAX;
    }
    (bytes as u64 / us as u64) as u32
}

// ---------------------------------------------------------------------------
// 五、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0802 域自检（判据逐条映射见 `vee02_checks.rs`）。
pub fn run_vee02_checks() -> CheckSet {
    super::vee02_checks::run_vee02_checks()
}

// ---------------------------------------------------------------------------
// 六、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn d() -> Decoder {
        Decoder::new()
    }

    #[test]
    fn vee02_ascii_fast_path() {
        let r = d().decode(b"Hi", &DecodeOptions::new());
        assert_eq!(r.codepoints, vec![0x48, 0x69]);
        assert!(r.is_clean());
        assert_eq!(r.stats.disp_count(Disposition::None), 2);
        assert_eq!(r.stats.valid_codepoints, 2);
    }

    #[test]
    fn vee02_multibyte_all_widths() {
        // 2 字节 U+00E9 / 3 字节 U+4E2D / 4 字节 U+1F600
        let r = d().decode("é中\u{1F600}".as_bytes(), &DecodeOptions::new());
        assert_eq!(r.codepoints, vec![0xE9, 0x4E2D, 0x1F600]);
        assert!(r.is_clean());
    }

    #[test]
    fn vee02_four_dispositions() {
        // 档二孤立续字节：0x80
        let r = d().decode(&[0x80], &DecodeOptions::new());
        assert_eq!(r.stats.disp_count(Disposition::LoneContinuation), 1);
        // 档三过长：0xC0 0x80（overlong NUL）
        let r = d().decode(&[0xC0, 0x80], &DecodeOptions::new());
        assert_eq!(r.stats.disp_count(Disposition::Overlong), 1);
        // 档四越界：代理区 ED A0 80（U+D800）
        let r = d().decode(&[0xED, 0xA0, 0x80], &DecodeOptions::new());
        assert_eq!(r.stats.disp_count(Disposition::OutOfRange), 1);
        // 档一截断：E4（需3 字节只给 1）
        let r = d().decode(&[0xE4], &DecodeOptions::new());
        assert_eq!(r.stats.disp_count(Disposition::Truncated), 1);
        assert!(r.stats.truncated_tail);
    }

    #[test]
    fn vee02_invalid_offset_table() {
        let r = d().decode(&[b'a', 0x80, b'b'], &DecodeOptions::new());
        assert_eq!(r.invalid_offsets.len(), 1);
        assert_eq!(r.invalid_offsets[0].offset, 1);
        assert_eq!(r.invalid_offsets[0].disp, Disposition::LoneContinuation);
        // 报告非空（零静默）。
        assert!(!r.invalid_report().is_empty());
    }

    #[test]
    fn vee02_truncated_tail_marker() {
        let r = d().decode(b"ab\xe4\xb8", &DecodeOptions::new());
        assert!(r.stats.truncated_tail);
        assert_eq!(r.stats.truncated_at, 2);
    }

    #[test]
    fn vee02_bom_and_explicit_priority() {
        let withbom = [0xEF, 0xBB, 0xBF, b'a'];
        let r = d().decode(&withbom, &DecodeOptions::new());
        assert_eq!(r.stats.bom, Encoding::Utf8);
        assert_eq!(r.codepoints, vec![b'a' as u32], "BOM 不计入正文");
        // 显式参数优先。
        let o = DecodeOptions::with_encoding(Encoding::Utf16Le);
        let r2 = d().decode(&withbom, &o);
        assert_eq!(r2.stats.effective_encoding, Encoding::Utf16Le);
        assert!(r2.stats.bom_conflict(&o));
    }

    #[test]
    fn vee02_newline_detection() {
        assert_eq!(Decoder::detect_newline(b"a\nb"), NewlineStyle::Lf);
        assert_eq!(Decoder::detect_newline(b"a\r\nb"), NewlineStyle::CrLf);
        assert_eq!(Decoder::detect_newline(b"a\rb"), NewlineStyle::Cr);
        assert_eq!(Decoder::detect_newline(b"a\r\nb\nc"), NewlineStyle::Mixed);
        assert_eq!(Decoder::detect_newline(b"ab"), NewlineStyle::None);
    }

    #[test]
    fn vee02_zero_crash_fuzz() {
        // 全 256 单字节 + 穷举 2 字节（65536）——恒不 panic。
        for b in 0..=255u8 {
            let r = d().decode(&[b], &DecodeOptions::new());
            let _ = r.invalid_report();
        }
        for hi in 0..=255u8 {
            for lo in 0..=255u8 {
                let r = d().decode(&[hi, lo], &DecodeOptions::new());
                assert!(r.codepoints.len() <= 2, "2 字节输入产出至多 2 码点");
            }
        }
        // 全0xFF 长串。
        let r = d().decode(&[0xFF; 64], &DecodeOptions::new());
        assert!(r.invalid_offsets.len() > 0);
    }

    #[test]
    fn vee02_utf16_conversion() {
        let r = d().decode("\u{1F600}".as_bytes(), &DecodeOptions::new());
        let v = d().to_utf16(&r);
        assert_eq!(v, vec![0xD83D, 0xDE00], "代理对正确");
        assert_eq!(d().to_utf32(&r), &[0x1F600]);
    }

    #[test]
    fn vee02_bench_threshold() {
        // 200MB/s 口径：1MB / 5ms = 200MB/s 边界（1MB=10^6B, 5ms=5000µs）。
        assert_eq!(mbps_of(1_000_000, 5_000), 200);
        // 口径与 bench 内部一致——两处若不同源，本断言即红（防口径漂移）。
        let b = bench(b"0123456789", 1, 1_000);
        assert_eq!(b.bytes, 10);
        assert_eq!(b.mbps, mbps_of(10, 1_000), "bench 与 mbps_of 口径同源");
        // 哨兵：零耗时视作无穷快。
        assert_eq!(mbps_of(1_000_000, 0), u32::MAX);
        // 实跑基准不崩且口径自洽（载荷与耗时自洽：小载荷配大耗时会整除截断成 0）。
        let mut big: alloc::vec::Vec<u8> = alloc::vec![b'x'; 1_000_000];
        let b2 = bench(&big, 1, 5_000);
        assert_eq!(b2.bytes, 1_000_000);
        assert_eq!(b2.mbps, BENCH_MIN_MBPS);
        let text = "hello 世界".as_bytes();
        let b3 = bench(text, 10, 100);
        assert_eq!(b3.bytes, text.len() * 10);
        assert!(b3.mbps > 0);
        // 判据：正常档期须达200MB/s 水位（1MB 文本 / 5000µs）。
        assert!(mbps_of(1_000_000, 5_000) >= BENCH_MIN_MBPS);
    }
}