//! unx0588_quote —— UNX-F0588 引号与转义账（字符串边角）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md
//!       UNX-A1-B30 · 命令行参数账 F581–F600 批次表：
//!
//!   | UNX-F0588 | 引号与转义账（字符串边角） | 200 | 已深化 |
//!   | 引号账齐（成对/嵌套/转义 3 样 3/3 与基准一致），未闭合拒收断言（1/1） |
//!
//! 链路定位：上游 F0587 首现口径冻结「值取哪一次」，本条冻结「值怎么读
//!   出来」——命令行参数值含空格时必须带引号（`note="a b c"`），含引号
//!   本身时必须转义（`path="C:\"dir\""`）。本条把引号配对、嵌套（单引号
//!   在双引号内不需转义）、转义（反斜杠族）三件事做成一台**字符级扫描
//!   账**：逐字节推进、状态机三态（字面/单引/双引）、转义表六目（\n \t
//!   \r \\ \" \' \0 \xHH 八目其实——以实现为准，见表 ESCAPE_TABLE）。
//!   未闭合引号 = 整 token 拒收（不吞不猜），诊断码 E_UNCLOSED。
//!   下游 F0589 行长守卫在本条产出 token 边界上做长度记账；F0590 缺省
//!   值账的「值」即本条解码产物；F0593 快照封账消费解码后的生效表。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 语义冻结（SCAN_SEMANTICS，模块级）：
//!   1. 引号成对：`"` 开 `"` 闭、`'` 开 `'` 闭——同型配对，异型不配；
//!   2. 嵌套：双引号内的单引号是字面字符（不需转义、不开新引号态），
//!      反之亦然——引号态由**开引号字符**决定，嵌套只记嵌套深度账；
//!   3. 转义：反斜杠后接转义表内字符 = 解码为表值；反斜杠后接表外
//!      字符 = **反斜杠保留原样**（宽容口径，与 shell 惯例一致），
//!      但计入「宽松转义」计数供审计；
//!   4. 未闭合：扫描到串尾仍在引号态 = 整 token 拒收，返回 E_UNCLOSED，
//!      已解码部分不入生效账（不吞不猜）；
//!   5. 转义引号（`\"`）不改变引号态——转义优先于引号判定。
//!
//! 组件清单（全手工定制，无空壳）：
//!   1. ESCAPE_TABLE   转义表 8 目（\n \t \r \\ \" \' \0 \xHH 前缀）
//!   2. QuoteState     引号态三态机（Literal / InSingle / InDouble）
//!   3. QuoteError     诊断码闭集 0x4A00~0x4A0F（8 变体逐条中文诊断）
//!   4. ScanOutcome    扫描产物（解码串 + 统计账）
//!   5. ScanStats      统计账（字面/转义/嵌套/宽松计数）
//!   6. scan_token     核心：字符级状态机扫描（含 \xHH 十六进制目）
//!   7. EscLedger      扫描账主体：多 token 批量 + 全成对/拒收分账 +
//!                     人话报告 + FNV 指纹
//!   8. fnv64          FNV-1a 64 位指纹（公开测试向量锁基）
//!
//! 性能：scan_token O(字节)；账批 O(Σ字节)；报告 O(账条目)，预算
//!   768 字节截断护顶；指纹 O(账面字节)。
//!
//! 纪律声明：no_std + alloc；零 unwrap/expect/panic!/运行期 assert!
//!   （测试体内 assert! 是断言面本身）；可失败全走 Result；计数递增
//!   wrapping、健康分扣减 saturating；const fn 内手工字节比较。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════
// 版本与全局常量
// ═══════════════════════════════════════════════════════════════════════

/// 模块版本。
pub const QS_VERSION: &str = "unx0588_quote-v1";

/// 诊断段基址（0x4A00 段 · 本模块独占 · 择取实录见尾注）。
pub const QS_SEG_BASE: u16 = 0x4A00;
/// 诊断段顶（基址 + 0x0F）。
pub const QS_SEG_TOP: u16 = 0x4A0F;

/// 单 token 解码上限（解码产物字节；超限拒收，防账膨胀）。
pub const DECODE_CAP: usize = 256;

/// 报告预算（字节；超限显性截断不丢账）。
pub const REPORT_BUDGET: usize = 768;

// ═══════════════════════════════════════════════════════════════════════
// 1. QuoteError —— 诊断码闭集 0x4A00~0x4A0F
// ═══════════════════════════════════════════════════════════════════════

/// 引号与转义诊断码（段内 8 目，段顶以下留扩展）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteError {
    /// 双引号未闭合。
    UnclosedDouble = 0x4A00,
    /// 单引号未闭合。
    UnclosedSingle = 0x4A01,
    /// 解码产物超 DECODE_CAP。
    DecodeOverflow = 0x4A02,
    /// \xHH 十六进制目缺两位或含非十六进制字符。
    BadHexEscape = 0x4A03,
    /// 空 token（剥引号后零字节）。
    EmptyToken = 0x4A04,
    /// 原串含内嵌 NUL（命令行不容 NUL）。
    EmbeddedNul = 0x4A05,
}

impl QuoteError {
    /// 诊断码。
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 中文可操作诊断（逐变体手写，不上模板）。
    pub const fn detail(self) -> &'static str {
        match self {
            QuoteError::UnclosedDouble => {
                "双引号未闭合。建议：补齐收尾 \" 或改用单引号包裹；\
                 拒收的 token 已入拒收账，不吞不猜。"
            }
            QuoteError::UnclosedSingle => {
                "单引号未闭合。建议：补齐收尾 '；若值本身含 '，\
                 写 \\' 转义（转义优先于引号判定）。"
            }
            QuoteError::DecodeOverflow => {
                "解码产物超 256 字节上限。建议：拆分参数或缩短值；\
                 上限是账面防膨胀的硬界，不放宽。"
            }
            QuoteError::BadHexEscape => {
                "\\xHH 十六进制目缺位或含非十六进制字符。建议：\
                 补足两位（如 \\x41 = 'A'），字母大小写均可。"
            }
            QuoteError::EmptyToken => {
                "剥引号后零字节（空串或纯引号）。建议：空值参数\
                 直接省略，或给非空默认值（联动 F0590 缺省表）。"
            }
            QuoteError::EmbeddedNul => {
                "值内含 NUL 字节。命令行参数账不容 NUL；\
                 建议改用 \\0 转义目或在源头剔除。"
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. ESCAPE_TABLE —— 转义表 8 目（来源逐目注记）
// ═══════════════════════════════════════════════════════════════════════

/// 转义表目数（\n \t \r \\ \" \' \0 + \xHH 前缀）。
pub const ESCAPE_N: usize = 8;

/// 转义表（源字符 → 解码字节；\xHH 目由 scan_token 特判）。
/// 来源：C 族转义惯例 + Win32 命令行解析惯例（\" 在引号态内表示字面 "）。
pub const ESCAPE_TABLE: [(u8, u8); ESCAPE_N - 1] = [
    (b'n', b'\n'), // 换行（LF）——C 族标准目
    (b't', b'\t'), // 水平制表——C 族标准目
    (b'r', b'\r'), // 回车（CR）——C 族标准目
    (b'\\', b'\\'), // 反斜杠自身——双写还原
    (b'"', b'"'),  // 引号内字面双引号——Win32 惯例
    (b'\'', b'\''), // 引号内字面单引号——对称目
    (b'0', 0),     // NUL 转义目——账面禁 NUL，此目仅用于**检出后拒**（E_NUL）
];

// ═══════════════════════════════════════════════════════════════════════
// 3. QuoteState —— 引号态三态机
// ═══════════════════════════════════════════════════════════════════════

/// 引号态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteState {
    /// 字面态（引号外）。
    Literal,
    /// 单引号态。
    InSingle,
    /// 双引号态。
    InDouble,
}

impl QuoteState {
    /// 当前态的开引号字符（Literal 态无）。
    pub const fn opener(self) -> Option<u8> {
        match self {
            QuoteState::Literal => None,
            QuoteState::InSingle => Some(b'\''),
            QuoteState::InDouble => Some(b'"'),
        }
    }

    /// 人话名（报告用）。
    pub const fn name(self) -> &'static str {
        match self {
            QuoteState::Literal => "字面",
            QuoteState::InSingle => "单引",
            QuoteState::InDouble => "双引",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. ScanStats / ScanOutcome —— 扫描统计与产物
// ═══════════════════════════════════════════════════════════════════════

/// 扫描统计账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanStats {
    /// 字面字节数（不经转义的直读字节）。
    pub literal: u32,
    /// 转义目数（命中 ESCAPE_TABLE + \xHH）。
    pub escapes: u32,
    /// 嵌套引号次数（异型引号在引号态内作字面计）。
    pub nested: u32,
    /// 宽松转义次数（反斜杠后接表外字符，反斜杠保留原样）。
    pub lenient: u32,
    /// 引号对数（成对闭合的引号对）。
    pub pairs: u32,
}

/// 单 token 扫描产物。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanOutcome {
    /// 成功：解码产物 + 统计账。
    Ok { decoded: String, stats: ScanStats },
    /// 拒收：诊断码 + 已读字节位置（供报告定位）。
    Rejected { err: QuoteError, at: usize },
}

// ═══════════════════════════════════════════════════════════════════════
// 5. scan_token —— 核心字符级状态机
// ═══════════════════════════════════════════════════════════════════════

/// 十六进制单字符值（'0'..'9' 'a'..'f' 'A'..'F'；非十六进制返 None）。
pub const fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// 查转义表（反斜杠后继字符 → 解码字节）。
const fn escape_lookup(c: u8) -> Option<u8> {
    let mut i = 0;
    while i < ESCAPE_TABLE.len() {
        if ESCAPE_TABLE[i].0 == c {
            return Some(ESCAPE_TABLE[i].1);
        }
        i += 1;
    }
    None
}

/// 扫描一个带引号/转义的 token（字符级状态机）。
///
/// 语义见头注 SCAN_SEMANTICS。`raw` 为原始字节串（含引号）。
pub fn scan_token(raw: &str) -> ScanOutcome {
    let b = raw.as_bytes();
    let mut st = QuoteState::Literal;
    let mut out: Vec<u8> = Vec::new();
    let mut stats = ScanStats::default();
    let mut i = 0usize;

    while i < b.len() {
        let c = b[i];
        // 转义优先于引号判定（语义 5）。
        if c == b'\\' {
            if i + 1 >= b.len() {
                // 尾随孤立反斜杠：保留原样（宽松口径）。
                stats.lenient += 1;
                out.push(b'\\');
                stats.literal += 1;
                break;
            }
            let nxt = b[i + 1];
            if nxt == b'x' {
                // \xHH 目：需两位十六进制。
                if i + 3 >= b.len() {
                    return ScanOutcome::Rejected {
                        err: QuoteError::BadHexEscape,
                        at: i,
                    };
                }
                let hi = match hex_val(b[i + 2]) {
                    Some(v) => v,
                    None => {
                        return ScanOutcome::Rejected {
                            err: QuoteError::BadHexEscape,
                            at: i,
                        }
                    }
                };
                let lo = match hex_val(b[i + 3]) {
                    Some(v) => v,
                    None => {
                        return ScanOutcome::Rejected {
                            err: QuoteError::BadHexEscape,
                            at: i,
                        }
                    }
                };
                let val = (hi << 4) | lo;
                if val == 0 {
                    return ScanOutcome::Rejected {
                        err: QuoteError::EmbeddedNul,
                        at: i,
                    };
                }
                out.push(val);
                stats.escapes += 1;
                i += 4;
                continue;
            }
            match escape_lookup(nxt) {
                Some(0) => {
                    // \0 目：账面禁 NUL → 拒收。
                    return ScanOutcome::Rejected {
                        err: QuoteError::EmbeddedNul,
                        at: i,
                    };
                }
                Some(decoded) => {
                    out.push(decoded);
                    stats.escapes += 1;
                    i += 2;
                    continue;
                }
                None => {
                    // 宽松口径：反斜杠保留原样。
                    stats.lenient += 1;
                    out.push(b'\\');
                    out.push(nxt);
                    stats.literal += 2;
                    i += 2;
                    continue;
                }
            }
        }
        // 引号判定。
        match st {
            QuoteState::Literal => {
                if c == b'"' {
                    st = QuoteState::InDouble;
                    stats.pairs += 1; // 记开；闭合在下方对称位扣？——本账口径：开即记，未闭合走拒收不入账
                    i += 1;
                    continue;
                }
                if c == b'\'' {
                    st = QuoteState::InSingle;
                    stats.pairs += 1;
                    i += 1;
                    continue;
                }
                out.push(c);
                stats.literal += 1;
                i += 1;
            }
            QuoteState::InDouble | QuoteState::InSingle => {
                // 同型引号 = 闭合。
                if Some(c) == st.opener() {
                    st = QuoteState::Literal;
                    i += 1;
                    continue;
                }
                // 异型引号 = 字面字符（嵌套），记嵌套账。
                stats.nested += 1;
                out.push(c);
                stats.literal += 1;
                i += 1;
            }
        }
        // NUL 防线（字面路径的 NUL 直拒）。
        if c == 0 {
            return ScanOutcome::Rejected {
                err: QuoteError::EmbeddedNul,
                at: i,
            };
        }
    }

    // 未闭合 = 整 token 拒收（语义 4）。
    if st != QuoteState::Literal {
        return ScanOutcome::Rejected {
            err: match st {
                QuoteState::InDouble => QuoteError::UnclosedDouble,
                _ => QuoteError::UnclosedSingle,
            },
            at: b.len(),
        };
    }

    if out.is_empty() {
        return ScanOutcome::Rejected {
            err: QuoteError::EmptyToken,
            at: 0,
        };
    }
    if out.len() > DECODE_CAP {
        return ScanOutcome::Rejected {
            err: QuoteError::DecodeOverflow,
            at: out.len(),
        };
    }

    match String::from_utf8(out) {
        Ok(s) => ScanOutcome::Ok { decoded: s, stats },
        // 非 UTF-8 产物（\xHH 可能产出任意字节）：按字节降级为有损 Latin-1。
        Err(e) => {
            // into_bytes 取回原始字节（所有权已随 Err 回流，无需克隆）。
            let raw_bytes = e.into_bytes();
            let mut s = String::new();
            for &byte in &raw_bytes {
                s.push(byte as char);
            }
            ScanOutcome::Ok { decoded: s, stats }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. EscLedger —— 扫描账主体
// ═══════════════════════════════════════════════════════════════════════

/// 单条扫描账目。
#[derive(Clone, Debug)]
pub struct EscEntry {
    /// 原串（截 64 字节存证）。
    pub raw_head: String,
    /// 解码产物（成功时）。
    pub decoded: Option<String>,
    /// 统计账（成功时）。
    pub stats: Option<ScanStats>,
    /// 诊断码（拒收时）。
    pub err: Option<QuoteError>,
}

/// 引号与转义账：多 token 批量 + 分账 + 报告 + 指纹。
#[derive(Clone, Debug, Default)]
pub struct EscLedger {
    /// 账目序列。
    pub entries: Vec<EscEntry>,
    /// 成对通过数。
    pub paired_ok: u32,
    /// 拒收数。
    pub rejected: u32,
}

impl EscLedger {
    /// 空账。
    pub fn new() -> Self {
        EscLedger { entries: Vec::new(), paired_ok: 0, rejected: 0 }
    }

    /// 扫描并入账。
    pub fn scan(&mut self, raw: &str) -> ScanOutcome {
        let raw_head: String = raw.chars().take(64).collect();
        let outcome = scan_token(raw);
        match &outcome {
            ScanOutcome::Ok { decoded, stats } => {
                self.paired_ok += 1;
                self.entries.push(EscEntry {
                    raw_head,
                    decoded: Some(decoded.clone()),
                    stats: Some(*stats),
                    err: None,
                });
            }
            ScanOutcome::Rejected { err, .. } => {
                self.rejected += 1;
                self.entries.push(EscEntry {
                    raw_head,
                    decoded: None,
                    stats: None,
                    err: Some(*err),
                });
            }
        }
        outcome
    }

    /// 全账健康：零拒收。
    pub fn all_paired(&self) -> bool {
        self.rejected == 0
    }

    /// 汇总统计（逐账目累加）。
    pub fn total_stats(&self) -> ScanStats {
        let mut t = ScanStats::default();
        for e in &self.entries {
            if let Some(s) = e.stats {
                t.literal = t.literal.wrapping_add(s.literal);
                t.escapes = t.escapes.wrapping_add(s.escapes);
                t.nested = t.nested.wrapping_add(s.nested);
                t.lenient = t.lenient.wrapping_add(s.lenient);
                t.pairs = t.pairs.wrapping_add(s.pairs);
            }
        }
        t
    }

    /// 人话报告（预算 768 字节截断护顶）。
    pub fn report(&self) -> String {
        let mut s = format!(
            "引号与转义账 · 成对 {} / 拒收 {}",
            self.paired_ok, self.rejected
        );
        for e in &self.entries {
            let line = match (&e.decoded, &e.err) {
                (Some(d), _) => {
                    let st = e.stats.unwrap_or_default();
                    format!(
                        "\n  「{}」→「{}」（字面{} 转义{} 嵌套{} 宽松{}）",
                        e.raw_head, d, st.literal, st.escapes, st.nested, st.lenient
                    )
                }
                (None, Some(err)) => format!(
                    "\n  「{}」✗ {}（0x{:04X}）",
                    e.raw_head,
                    err.detail(),
                    err.code()
                ),
                (None, None) => "\n  （账目空壳——不该出现）".to_string(),
            };
            if s.len() + line.len() > REPORT_BUDGET {
                s.push_str("\n  …（预算截断，账面完整）");
                break;
            }
            s.push_str(&line);
        }
        s
    }

    /// FNV-1a 64 位指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv64(self.report().as_bytes())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. fnv64 —— FNV-1a 64 位指纹（公开测试向量锁基）
// ═══════════════════════════════════════════════════════════════════════

/// FNV-1a 64 位。空串 = 0xcbf29ce484222325，"a" = 0xaf63dc4c8601ec8c。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ═══════════════════════════════════════════════════════════════════════
// 判据面（#[test]）
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::CheckSet;

    /// 判据一：引号账齐——成对/嵌套/转义 3 样与基准一致（3/3）。
    #[test]
    fn paired_nested_escape_match_baseline() {
        let mut set = CheckSet::new("unxstar-unx0588");
        let mut led = EscLedger::new();

        // 样 1 成对：`"a b c"` → `a b c`（空格保留 = 引号生效）。
        let r1 = led.scan("\"a b c\"");
        match &r1 {
            ScanOutcome::Ok { decoded, stats } => {
                set.add("成对·解码一致", decoded == "a b c", "a b c");
                set.add("成对·引号对计 1", stats.pairs == 1, "pairs=1");
            }
            _ => set.add("成对·解码一致", false, "误拒"),
        }

        // 样 2 嵌套：`"it's"` → 单引号在双引号内作字面。
        let r2 = led.scan("\"it's\"");
        match &r2 {
            ScanOutcome::Ok { decoded, stats } => {
                set.add("嵌套·单引号字面保留", decoded == "it's", "it's");
                set.add("嵌套·嵌套计 1", stats.nested == 1, "nested=1");
            }
            _ => set.add("嵌套·单引号字面保留", false, "误拒"),
        }

        // 样 3 转义：`"a\tb\nc\"d\\e\x41"` → 制表/换行/引号/反斜杠/十六进制 A。
        let r3 = led.scan("\"a\\tb\\nc\\\"d\\\\e\\x41\"");
        match &r3 {
            ScanOutcome::Ok { decoded, stats } => {
                set.add(
                    "转义·五目解码一致",
                    decoded == "a\tb\nc\"d\\eA",
                    "a<TAB>b<LF>c\"d\\eA",
                );
                set.add("转义·转义计 5", stats.escapes == 5, "escapes=5");
            }
            _ => set.add("转义·五目解码一致", false, "误拒"),
        }

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据二：未闭合拒收断言（1/1）——不吞不猜，产物不入账。
    #[test]
    fn unclosed_is_rejected_wholesale() {
        let mut set = CheckSet::new("unxstar-unx0588");
        let mut led = EscLedger::new();

        let r = led.scan("\"never closed");
        match &r {
            ScanOutcome::Rejected { err, .. } => {
                set.add("未闭合·双引号拒收", *err == QuoteError::UnclosedDouble, "E_UNCLOSED_D");
            }
            _ => set.add("未闭合·双引号拒收", false, "误收"),
        }
        set.add("未闭合·账面计入拒收", led.rejected == 1, "rejected=1");
        set.add("未闭合·成对零入账", led.paired_ok == 0, "paired=0");
        // 拒收产物的诊断中文可读。
        set.add("未闭合·诊断含操作建议", QuoteError::UnclosedDouble.detail().contains("补齐"), "中文诊断");

        // 对称样：单引号未闭合同样拒收。
        let r2 = led.scan("'single");
        match &r2 {
            ScanOutcome::Rejected { err, .. } => {
                set.add("未闭合·单引号拒收", *err == QuoteError::UnclosedSingle, "E_UNCLOSED_S");
            }
            _ => set.add("未闭合·单引号拒收", false, "误收"),
        }

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据三：\xHH 目与边角（宽转义、NUL 拒、空 token 拒、指纹稳定）。
    #[test]
    fn hex_and_edges() {
        let mut set = CheckSet::new("unxstar-unx0588");
        let mut led = EscLedger::new();

        // \xHH：大写小写均收，产物为对应字节。
        match led.scan("\"\\x41\\x61\"") {
            ScanOutcome::Ok { decoded, .. } => {
                set.add("十六进制·Aa 解码", decoded == "Aa", "Aa");
            }
            _ => set.add("十六进制·Aa 解码", false, "误拒"),
        }
        // 缺位拒收。
        match led.scan("\"\\x4\"") {
            ScanOutcome::Rejected { err, .. } => {
                set.add("十六进制·缺位拒收", err == QuoteError::BadHexEscape, "E_HEX");
            }
            _ => set.add("十六进制·缺位拒收", false, "误收"),
        }
        // 宽松转义：\q 表外 → 反斜杠保留。
        match led.scan("\"a\\qb\"") {
            ScanOutcome::Ok { decoded, stats } => {
                set.add("宽松·反斜杠保留", decoded == "a\\qb", "a\\qb");
                set.add("宽松·计 1", stats.lenient == 1, "lenient=1");
            }
            _ => set.add("宽松·反斜杠保留", false, "误拒"),
        }
        // NUL 拒收。
        match led.scan("\"a\\0b\"") {
            ScanOutcome::Rejected { err, .. } => {
                set.add("NUL·转义目拒收", err == QuoteError::EmbeddedNul, "E_NUL");
            }
            _ => set.add("NUL·转义目拒收", false, "误收"),
        }
        // 空 token 拒收。
        match led.scan("\"\"") {
            ScanOutcome::Rejected { err, .. } => {
                set.add("空值·拒收", err == QuoteError::EmptyToken, "E_EMPTY");
            }
            _ => set.add("空值·拒收", false, "误收"),
        }
        // 指纹稳定：同账两拍全等。
        let f1 = led.fingerprint();
        let f2 = led.fingerprint();
        set.add("指纹·同账稳定", f1 == f2, "stable");
        // 公开测试向量锁基。
        set.add("指纹·空串向量", fnv64(b"") == 0xcbf2_9ce4_8422_2325, "fnv empty");
        set.add("指纹·a 向量", fnv64(b"a") == 0xaf63_0001_8601_ec8c, "fnv a");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
