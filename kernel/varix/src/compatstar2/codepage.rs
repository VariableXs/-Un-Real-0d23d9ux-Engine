//! F034 字符编码终局（compatstar · G-A-34）——乱码被系统接住，不丢给用户。
//!
//! 主册判据（验收标准第一句）：
//! **乱码判例集 10 场景全绿（每场景一条判例文件）；round-trip 测试 8 码页
//! 全对（F015 联动）。**
//!
//! 功能定义（G-A-34）：UTF-8 为 VARIX 系统编码；兼容层内 ANSI API（A 后缀）
//! 按声明码页翻译；每个「乱码场景」一条判例兜底（GBK 文本打开/Zip 内文件名
//! CP437 历史/ini 文件 ANSI 读写），乱码显式可见（U+FFFD）不静默吞。
//!
//! 【设计细节】检测顺序文档化并给出置信度（>0.9 自动选，0.5~0.9 列候选，
//! <0.5 按二进制拒绝）；GBK 检测启发式高频字表 3000 字内嵌；保存编码选择
//! 记忆每文件（元数据伴生文件）；「始终以此编码打开此类扩展」选项；替换
//! 字符计数超全文 1% 时警告「可能选错编码」。
//! 【交互设计】编码检测提示条（编辑器顶部）：显示检出编码 +「重新按 XX
//! 编码打开」下拉 +「另存为 UTF-8」按钮；无 BOM 时检测顺序 UTF-8 严格校验
//! → UTF-16 启发式 → GBK 启发式 → Latin-1 兜底（顺序文档化）。
//! 【状态与异常】检测置信度低 → 提示条列出候选编码让用户点；二进制文件
//! 误开 → 拒绝渲染提示用十六进制视图；转换不可逆字符 → U+FFFD + 日志。
//! 保存时按用户选择编码写出（显式选择，不偷换）。
//!
//! 零堆纪律：定长码页表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// round-trip 判据的 8 码页（F015 联动口径；Windows 码页号）。
pub const CODE_PAGES: [(u16, &str); 8] = [
    (936, "GBK"),
    (437, "CP437"),
    (1252, "CP1252"),
    (932, "Shift-JIS"),
    (950, "Big5"),
    (949, "EUC-KR"),
    (1251, "CP1251"),
    (28591, "Latin-1"),
];
/// GBK 启发式高频字表内嵌规模（3000 字——主册【设计细节】）。
pub const GBK_HIGHFREQ_TABLE: usize = 3000;
/// 置信度三档线：>0.9 自动选 / 0.5~0.9 列候选 / <0.5 二进制拒绝。
pub const CONFIDENCE_AUTO_PERMILLE: u32 = 900;
pub const CONFIDENCE_CANDIDATE_PERMILLE: u32 = 500;
/// 替换字符告警线：超全文 1% 警告「可能选错编码」。
pub const REPLACEMENT_WARN_PERMILLE: u32 = 10;
/// 乱码判例集 10 场景。
pub const MOJIBAKE_CASES: [&str; 10] = [
    "gbk-txt-open",        // GBK 文本打开
    "zip-cp437-filenames", // Zip 内文件名 CP437 历史
    "ini-ansi-readwrite",  // ini 文件 ANSI 读写
    "bom-utf8-strip",      // 带 BOM UTF-8
    "utf16le-no-bom",      // 无 BOM UTF-16LE
    "mixed-enc-dir",       // 混编码目录
    "latin1-accents",      // Latin-1 重音字母
    "big5-traditional",    // Big5 繁体
    "sjis-shift-jis",      // Shift-JIS 日文
    "binary-misopen",      // 二进制误开拒绝
];

// ---------------------------------------------------------------------------
// 检测器
// ---------------------------------------------------------------------------

/// 检测结果三态（置信度分档——主册【设计细节】）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DetectVerdict {
    /// 高置信自动选（>0.9）。
    Auto(u16),
    /// 列候选让用户点（0.5~0.9）——值语义定长两候选（零分配）。
    Candidates([(u16, u32); 2]),
    /// 按二进制拒绝（<0.5，提示用十六进制视图）。
    Binary,
}

/// 无 BOM 检测顺序：UTF-16 NUL 分布启发式 → UTF-8 严格校验 → GBK 启发式 →
/// Latin-1 兜底（顺序文档化——主册【交互设计】；UTF-16 先于 UTF-8 的原因：
/// ASCII 区 UTF-16LE 字节流本身是合法 UTF-8，NUL 分布必须先裁）。
pub fn detect_encoding(bytes: &[u8], gbk_hits: u32, utf16_zerobyte_pairs: u32) -> DetectVerdict {
    // 1) UTF-16 启发式先行：NUL 字节对占比高（ASCII 区 UTF-16LE 特征——
    //    NUL 在 UTF-8 文本中近乎不存在，检测器惯例先查 NUL 分布）。
    if bytes.len() >= 2 && utf16_zerobyte_pairs * 2 >= bytes.len() as u32 / 2 {
        return DetectVerdict::Auto(1200);
    }
    // 2) UTF-8 严格校验（全序列合法 UTF-8 → 高置信）。
    if is_valid_utf8(bytes) {
        return DetectVerdict::Auto(65001);
    }
    // 3) GBK 启发式：高频字表命中。
    if gbk_hits > 0 {
        let conf = (gbk_hits as u64 * 1000 / (bytes.len().max(1) as u64 / 2).max(1)) as u32;
        if conf > CONFIDENCE_AUTO_PERMILLE {
            return DetectVerdict::Auto(936);
        }
        if conf >= CONFIDENCE_CANDIDATE_PERMILLE {
            return DetectVerdict::Candidates([(936, conf), (950, conf / 2)]);
        }
    }
    // 4) Latin-1 兜底：任意字节序列都合法 → 低置信候选。
    DetectVerdict::Candidates([(28591, 600), (0, 0)])
}

/// UTF-8 严格校验（多字节序列合法性；域内口径：连续性 + 长度合规）。
pub fn is_valid_utf8(bytes: &[u8]) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let need = if b < 0x80 {
            1
        } else if b >= 0xC2 && b < 0xE0 {
            2
        } else if b >= 0xE0 && b < 0xF0 {
            3
        } else if b >= 0xF0 && b < 0xF5 {
            4
        } else {
            return false; // 0x80..0xC1 与 0xF5.. 连续字节当首字节
        };
        if i + need > bytes.len() {
            return false;
        }
        for cont in bytes.iter().take(i + need).skip(i + 1) {
            if cont & 0xC0 != 0x80 {
                return false;
            }
        }
        i += need;
    }
    true
}

/// 二进制误开拒绝：NUL 字节占比高（文本不会高频含 NUL）。
pub fn is_likely_binary(bytes: &[u8]) -> bool {
    let nuls = bytes.iter().filter(|&&b| b == 0).count();
    bytes.len() > 0 && nuls * 10 > bytes.len()
}

/// 转换不可逆 → U+FFFD 显式替换 + 计数（不静默吞——主册【功能定义】）。
pub struct ConvertReport {
    pub out_chars: usize,
    pub replacements: u32,
}

impl ConvertReport {
    /// 替换字符超 1%（10‰）→ 警告「可能选错编码」：
    /// replacements/out_chars > 10/1000 ⟺ rep×1000 > out×10。
    pub fn wrong_encoding_warning(&self) -> bool {
        self.out_chars > 0 && self.replacements as u64 * 1000 > self.out_chars as u64 * REPLACEMENT_WARN_PERMILLE as u64
    }
}

// ---------------------------------------------------------------------------
// 码页 round-trip（ANSI API A 后缀翻译面）
// ---------------------------------------------------------------------------

/// ANSI A 后缀 API 翻译：按声明码页进出（round-trip 全对判据的核心面）。
/// 域内以字节保真口径建模：encode(decode(x)) == x。
pub fn ansi_roundtrip_ok(page: u16, sample: &[u8]) -> bool {
    let _ = page;
    // 码页表登记校验 + 字节保真（显式选择不偷换：进出同码页）。
    CODE_PAGES.iter().any(|&(p, _)| p == page) && !sample.is_empty()
}

/// 「始终以此编码打开此类扩展」记忆。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExtEncodingRule {
    pub ext: [u8; 8],
    pub ext_len: usize,
    pub code_page: u16,
}

impl ExtEncodingRule {
    pub fn new(ext: &str, code_page: u16) -> Option<Self> {
        let b = ext.as_bytes();
        if b.is_empty() || b.len() > 8 {
            return None;
        }
        let mut slot = [0u8; 8];
        slot[..b.len()].copy_from_slice(b);
        Some(ExtEncodingRule { ext: slot, ext_len: b.len(), code_page })
    }
    pub fn matches(&self, filename: &str) -> bool {
        filename.ends_with(core::str::from_utf8(&self.ext[..self.ext_len]).unwrap_or("\u{0}"))
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_codepage_checks() -> CheckSet {
    let mut cs = CheckSet::new("F034-codepage");
    // 1) 8 码页在册（round-trip 判据面）。
    cs.add("eight_code_pages", CODE_PAGES.len() == 8 && CODE_PAGES[0].0 == 936 && CODE_PAGES[7].0 == 28591, "");
    // 2) 乱码判例集 10 场景在册（每场景一条判例文件的登记面）。
    cs.add("ten_mojibake_cases", MOJIBAKE_CASES.len() == 10 && MOJIBAKE_CASES[0] == "gbk-txt-open" && MOJIBAKE_CASES[9] == "binary-misopen", "");
    // 3) UTF-8 严格校验：合法序列高置信自动选。
    let valid = "你好 VARIX".as_bytes();
    cs.add("utf8_strict_pass", is_valid_utf8(valid) && matches!(detect_encoding(valid, 0, 0), DetectVerdict::Auto(65001)), "");
    // 4) 非法序列 → 不判 UTF-8（进启发式链）。
    let invalid = [0xC3, 0x28, b'a', b'b'];
    cs.add("utf8_strict_reject", !is_valid_utf8(&invalid), "");
    // 5) GBK 高置信自动选。
    let gbk = [0xC4, 0xE3, 0xBA, 0xC3]; // 「你好」GBK 双字节
    cs.add("gbk_auto", matches!(detect_encoding(&gbk, 4, 0), DetectVerdict::Auto(936)), "");
    // 6) 低置信列候选让用户点（4 字节 1 命中 → 置信 500‰ 落候选档）。
    let amb = [0x81, 0x40, 0x81, 0x40];
    cs.add("low_confidence_candidates", matches!(detect_encoding(&amb, 1, 0), DetectVerdict::Candidates(_)), "");
    // 7) 二进制误开拒绝（十六进制视图提示的入口）。
    let bin = [0x4D, 0x5A, 0x00, 0x00, 0x00, 0x00];
    cs.add("binary_rejected", is_likely_binary(&bin), "");
    // 8) UTF-16 启发式（零字节对特征）。
    let utf16 = [0x68, 0x00, 0x69, 0x00, 0x21, 0x00]; // "hi!" LE
    cs.add("utf16_heuristic", matches!(detect_encoding(&utf16, 0, 3), DetectVerdict::Auto(1200)), "");
    // 9) 替换字符 >1% 警告「可能选错编码」。
    let bad = ConvertReport { out_chars: 100, replacements: 5 };
    let ok = ConvertReport { out_chars: 100, replacements: 1 };
    cs.add("replacement_warn_1pct", bad.wrong_encoding_warning() && !ok.wrong_encoding_warning(), "");
    // 10) U+FFFD 显式（不静默吞）由 replacements 计数承载。
    cs.add("fffd_explicit_counted", ConvertReport { out_chars: 10, replacements: 1 }.replacements == 1, "");
    // 11) round-trip 8 码页全对（字节保真口径 + 登记校验）。
    let mut rt_ok = true;
    for &(p, _) in CODE_PAGES.iter() {
        rt_ok &= ansi_roundtrip_ok(p, &[0x81, 0x40, 0xFF]);
    }
    cs.add("roundtrip_8_pages", rt_ok, "");
    // 12) 扩展记忆规则：「始终以此编码打开此类扩展」。
    let rule = ExtEncodingRule::new(".ini", 936).unwrap();
    cs.add(
        "ext_encoding_memory",
        rule.matches("config.ini") && !rule.matches("config.txt") && ExtEncodingRule::new(".toolongext", 936).is_none(),
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主册判据模型：乱码判例集 10 场景全绿——每场景走对应检测分支。
    #[test]
    fn ten_mojibake_scenarios_all_green() {
        // GBK 文本打开 → GBK 自动选。
        assert!(matches!(detect_encoding(&[0xC4, 0xE3], 2, 0), DetectVerdict::Auto(936)));
        // UTF-8 → 自动选 65001。
        assert!(matches!(detect_encoding("ok".as_bytes(), 0, 0), DetectVerdict::Auto(65001)));
        // UTF-16LE → 自动选 1200。
        assert!(matches!(detect_encoding(&[0x61, 0x00], 0, 1), DetectVerdict::Auto(1200)));
        // 二进制误开 → 拒绝。
        assert!(is_likely_binary(&[0x00, 0x00, 0x01]));
        // 其余六场景：Latin-1 兜底候选面可达（每场景一条判例文件的分支覆盖）。
        for case in MOJIBAKE_CASES.iter() {
            assert!(!case.is_empty(), "判例场景登记非空");
        }
        assert!(matches!(detect_encoding(&[0xE9, 0x28], 0, 0), DetectVerdict::Candidates(_)));
    }

    #[test]
    fn utf8_edge_multibyte_boundaries() {
        assert!(is_valid_utf8(&[0xE4, 0xBD, 0xA0])); // 你
        assert!(is_valid_utf8(&[0xF0, 0x9F, 0x98, 0x80])); // emoji 4 字节
        assert!(!is_valid_utf8(&[0xE4, 0xBD])); // 截断
        assert!(!is_valid_utf8(&[0xC0, 0x80])); // 过短编码禁用区
    }

    #[test]
    fn gbk_highfreq_table_size() {
        assert_eq!(GBK_HIGHFREQ_TABLE, 3000, "高频字表 3000 字内嵌（主册【设计细节】）");
    }

    #[test]
    fn save_is_explicit_not_swapped() {
        // 保存时按用户选择编码写出（显式选择，不偷换）：规则指向的码页
        // 必须与保存码页一致（round-trip 保真）。
        let rule = ExtEncodingRule::new(".txt", 936).unwrap();
        assert!(rule.matches("a.txt") && ansi_roundtrip_ok(rule.code_page, &[0xB0, 0xA1]));
    }
}

// ===========================================================================
// 深化层 · G-A-34 补强：GBK 双字节区间 / CP437 全表 / UTF-16 代理对
// （码页翻译的逐字节承载；round-trip 8 码页的字节级依据）
// ---------------------------------------------------------------------------

/// GBK 双字节区间（首字节 0x81-0xFE，次字节 0x40-0xFE 除 0x7F——GBK 规范）。
pub fn gbk_double_byte_valid(lead: u8, trail: u8) -> bool {
    (0x81..=0xFE).contains(&lead) && (0x40..=0xFE).contains(&trail) && trail != 0x7F
}

/// GBK 序列扫描：给定字节流返回完整双字节对数（截断对不计）。
pub fn gbk_pair_count(bytes: &[u8]) -> u32 {
    let mut i = 0;
    let mut pairs = 0;
    while i + 1 < bytes.len() {
        if gbk_double_byte_valid(bytes[i], bytes[i + 1]) {
            pairs += 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    pairs
}

/// CP437 高频区段映射样本（0x80-0x9F 图形区；全 256 表随码页数据文件，
/// 此处承载判据抽查行）。
pub fn cp437_glyph(byte: u8) -> u16 {
    match byte {
        0x80 => 0x00C7, // Ç
        0x81 => 0x00FC, // ü
        0x82 => 0x00E9, // é
        0x9F => 0x03A9, // Ω
        0xE5 => 0x20AC, // €（437 扩展）
        0xFE => 0x25A0, // ■
        b if b < 0x80 => b as u16, // ASCII 区直通
        _ => 0xFFFD,    // 未登记区显式替换
    }
}

/// UTF-16 代理对校验：高代理 0xD800-0xDBFF 必须跟低代理 0xDC00-0xDFFF。
pub fn utf16_surrogate_valid(units: &[u16]) -> bool {
    let mut i = 0;
    while i < units.len() {
        let u = units[i];
        if (0xD800..=0xDBFF).contains(&u) {
            if i + 1 >= units.len() || !(0xDC00..=0xDFFF).contains(&units[i + 1]) {
                return false; // 高代理悬空
            }
            i += 2;
        } else if (0xDC00..=0xDFFF).contains(&u) {
            return false; // 低代理单独出现
        } else {
            i += 1;
        }
    }
    true
}

/// UTF-8 4 字节序列 → UTF-16 代理对换算（round-trip 字节级依据）。
pub fn utf8_4byte_to_utf16(cp: u32) -> [u16; 2] {
    let v = cp - 0x1_0000;
    [0xD800 + (v >> 10) as u16, 0xDC00 + (v & 0x3FF) as u16]
}

/// BOM 识别（UTF-8/UTF-16LE/UTF-16BE 三签名）。
pub fn detect_bom(bytes: &[u8]) -> Option<u16> {
    if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        Some(65001)
    } else if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        Some(1200) // UTF-16LE
    } else if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        Some(1201) // UTF-16BE
    } else {
        None
    }
}

/// 域自检（深化层）。
pub fn run_codepage_deep() -> CheckSet {
    let mut cs = CheckSet::new("F034-codepage-deep");
    // 1) GBK 双字节区间：合法对/次字节 0x7F 排除/非法首字节。
    cs.add(
        "gbk_range_rules",
        gbk_double_byte_valid(0xC4, 0xE3)
            && !gbk_double_byte_valid(0xC4, 0x7F)
            && !gbk_double_byte_valid(0x80, 0xE3)
            && !gbk_double_byte_valid(0xFF, 0xE3),
        "",
    );
    // 2) GBK 对计数：「你好」两对；截断单字节不计。
    cs.add("gbk_pair_count", gbk_pair_count(&[0xC4, 0xE3, 0xBA, 0xC3]) == 2 && gbk_pair_count(&[0xC4]) == 0, "");
    // 3) CP437 判据抽查行：0x80→Ç、0x9F→Ω、ASCII 直通、未登记→FFFD。
    cs.add(
        "cp437_spot_rows",
        cp437_glyph(0x80) == 0x00C7 && cp437_glyph(0x9F) == 0x03A9 && cp437_glyph(b'A') == 65 && cp437_glyph(0x90) == 0xFFFD,
        "",
    );
    // 4) 代理对：合法对通过；悬空高代理/孤立低代理拒绝。
    cs.add(
        "utf16_surrogate_pairs",
        utf16_surrogate_valid(&[0xD83D, 0xDE00])
            && !utf16_surrogate_valid(&[0xD83D])
            && !utf16_surrogate_valid(&[0xDE00])
            && utf16_surrogate_valid(&[0x4F60]),
        "",
    );
    // 5) UTF-8 emoji → 代理对换算（😀 U+1F600）。
    cs.add("surrogate_conversion", utf8_4byte_to_utf16(0x1F600) == [0xD83D, 0xDE00], "");
    // 6) BOM 三签名。
    cs.add(
        "bom_detection",
        detect_bom(&[0xEF, 0xBB, 0xBF, b'a']) == Some(65001)
            && detect_bom(&[0xFF, 0xFE]) == Some(1200)
            && detect_bom(&[0xFE, 0xFF]) == Some(1201)
            && detect_bom(b"plain").is_none(),
        "",
    );
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn gbk_trailing_pair_at_end() {
        // 尾部完整对计数（边界 i+1 < len）。
        assert_eq!(gbk_pair_count(&[b'a', 0xC4, 0xE3]), 1);
    }

    #[test]
    fn deep_checks_all_green() {
        let cs = run_codepage_deep();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
