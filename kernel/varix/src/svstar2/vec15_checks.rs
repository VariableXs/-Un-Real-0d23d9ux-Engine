//! VE-F0415 · 域自检（判据逐条对应，见 `vec15_encoding.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - BOM 优先 → `C15-BOM-*`（五种 BOM 形态检出、UTF-32LE 不被 UTF-16LE 抢判
//!   （最长匹配）、BOM 压过构建系统声明、BOM 压过 UTF-8 假定、冲突出注记、探测
//!   最多看 4 字节）
//! - UTF-8 假定 → `C15-假定-*`（无 BOM 走假定、判定来源显式留痕、RequireBom 缺
//!   BOM 报错、声明优先于假定、假定注记非空）
//! - 非法报错 → `C15-非法-*`（五类 UTF-8 非法形态各自独立码、越界方向分类、错误带字节下标、
//!   带字节值、带行列、带建议、UTF-16 孤立高/低代理项、UTF-16 奇长度、UTF-32
//!   越界、UTF-32 代理项区、UTF-32 长度不整除、错误码不合并）
//! - 一次转换 → `C15-转换-*`（UTF-8 全族往返、UTF-16 双端往返、代理对合成、
//!   UTF-32 双端往返、单遍自证、产物恒无 U+FFFD、BOM 剥离后正文正确、词法桥
//!   产出 UTF-8）
//! - 元数据 → `C15-元数据-*`（编码信息入元数据、O(1) 构造、摘要非空、判定来源
//!   可区分、字节跨度记账）
//! - 零静默 → `C15-显性-*`（三要素齐备、注记非空、无替换字符）

// no_std 下 std prelude 不存在：`String` 与 `format!`/`vec!` 都得显式引入。
// 宿主 `cargo test` 有 std prelude 会掩盖这一点，整树 `cargo check --lib`
// 才暴露——两处都写上，两条链路都成立。
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::vec15_encoding::*;
use crate::checks::CheckSet;

/// 构造带指定 BOM 前缀的原始字节。
fn with_bom(bom: &[u8], body: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.extend_from_slice(bom);
    v.extend_from_slice(body);
    v
}

/// 编码一段 UTF-16LE 文本（含代理对）。
fn utf16le(text: &str) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for u in text.encode_utf16() {
        out.push((u & 0xFF) as u8);
        out.push((u >> 8) as u8);
    }
    out
}

/// 编码一段 UTF-16BE 文本（含代理对）。
fn utf16be(text: &str) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for u in text.encode_utf16() {
        out.push((u >> 8) as u8);
        out.push((u & 0xFF) as u8);
    }
    out
}

/// 编码一段 UTF-32LE 文本。
fn utf32(text: &str, little: bool) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for c in text.chars() {
        let v = c as u32;
        if little {
            out.extend_from_slice(&v.to_le_bytes());
        } else {
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
    out
}

/// 判定码是否以某前缀开头（`&'static str` 详情用）。
fn has_prefix(s: &str, p: &str) -> bool {
    s.len() >= p.len() && s.get(..p.len()) == Some(p)
}

/// 取注记里是否含指定种类（按 `describe` 文本判定，避免额外派生 PartialEq）。
fn notes_contain(d: &DecodedSource, needle: &str) -> bool {
    d.notes.iter().any(|n| has_prefix(n.describe().as_str(), needle))
}

/// VE-F0415 域自检。
pub fn run_vec15_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec15");

    // ---- 判据：BOM 优先 ----

    let m8 = detect_bom(&[0xEF, 0xBB, 0xBF, b'a']);
    set.add(
        "C15-BOM-UTF8 形态检出",
        m8.kind == BomKind::Utf8 && m8.len == 3,
        "",
    );

    let m16le = detect_bom(&[0xFF, 0xFE, b'a', 0x00]);
    set.add(
        "C15-BOM-UTF16LE 形态检出",
        m16le.kind == BomKind::Utf16Le && m16le.len == 2,
        "",
    );

    let m16be = detect_bom(&[0xFE, 0xFF, 0x00, b'a']);
    set.add(
        "C15-BOM-UTF16BE 形态检出",
        m16be.kind == BomKind::Utf16Be && m16be.len == 2,
        "",
    );

    // 最长匹配要害：FF FE 00 00 既是 UTF-32LE BOM 又是 UTF-16LE BOM 的前缀。
    // 短的可能先判就会把 UTF-32 文件切成 UTF-16LE 并静默吃掉后两字节。
    let m32le = detect_bom(&[0xFF, 0xFE, 0x00, 0x00]);
    set.add(
        "C15-BOM-UTF32LE 最长匹配（不被 UTF-16LE 抢判）",
        m32le.kind == BomKind::Utf32Le && m32le.len == 4,
        "",
    );

    let m32be = detect_bom(&[0x00, 0x00, 0xFE, 0xFF]);
    set.add(
        "C15-BOM-UTF32BE 最长匹配（不被 UTF-16BE 抢判）",
        m32be.kind == BomKind::Utf32Be && m32be.len == 4,
        "",
    );

    set.add("C15-BOM-无 BOM 不误判", detect_bom(b"void main(){}").kind == BomKind::None, "");

    set.add(
        "C15-BOM-探测只看的字节数有界（≤4）",
        m8.head_examined <= BOM_MAX_LEN && m32le.head_examined <= BOM_MAX_LEN,
        "",
    );

    // BOM 压过构建系统声明：BOM 说 UTF-8，声明说 UTF-16 → 以 BOM 为准。
    let raw_mix = with_bom(&[0xEF, 0xBB, 0xBF], b"abc");
    let d_mix = convert_source(&raw_mix, &DecodeConfig::new().with_declared(SourceEncoding::Utf16Le));
    set.add(
        "C15-BOM-压过构建系统声明",
        matches!(&d_mix, Ok(d) if d.meta.encoding == SourceEncoding::Utf8
            && d.meta.source == DetectionSource::Bom),
        "",
    );

    // BOM 压过 UTF-8 假定：BOM 说 UTF-16LE 且内容真是 UTF-16 → 转出正确正文。
    let raw_u16 = with_bom(&[0xFF, 0xFE], &utf16le("hi"));
    let d_u16 = convert_source(&raw_u16, &DecodeConfig::new());
    set.add(
        "C15-BOM-压过 UTF-8 假定（UTF-16LE BOM 走宽编码）",
        matches!(&d_u16, Ok(d) if d.meta.encoding == SourceEncoding::Utf16Le
            && d.as_str() == "hi"),
        "",
    );

    set.add(
        "C15-BOM-与声明冲突出注记（BOM 为准）",
        matches!(&d_mix, Ok(d) if notes_contain(d, "源文件带")),
        "",
    );

    // ---- 判据：UTF-8 假定 ----

    let plain = b"vec4 main() { return vec4(1.0); }";
    let d_plain = convert_source(plain, &DecodeConfig::new());
    set.add(
        "C15-假定-无 BOM 走 UTF-8 假定",
        matches!(&d_plain, Ok(d) if d.meta.encoding == SourceEncoding::Utf8
            && d.meta.source == DetectionSource::AssumedUtf8
            && d.as_str() == "vec4 main() { return vec4(1.0); }"),
        "",
    );

    set.add(
        "C15-假定-判定来源显式留痕（假定可识别）",
        matches!(&d_plain, Ok(d) if d.meta.source.is_assumption()),
        "",
    );

    set.add(
        "C15-假定-走假定时出注记（不静默）",
        matches!(&d_plain, Ok(d) if notes_contain(d, "源文件无 BOM")),
        "",
    );

    // RequireBom：缺 BOM 硬门。
    let d_req = convert_source(plain, &DecodeConfig::new().with_policy(AssumePolicy::RequireBom));
    set.add(
        "C15-假定-RequireBom 缺 BOM 报错",
        matches!(&d_req, Err(f) if f.code() == "VE-F0415-BOM-REQUIRED"),
        "",
    );

    let d_req_ok = convert_source(
        &with_bom(&[0xEF, 0xBB, 0xBF], b"x"),
        &DecodeConfig::new().with_policy(AssumePolicy::RequireBom),
    );
    set.add(
        "C15-假定-RequireBom 带 BOM 通过",
        matches!(&d_req_ok, Ok(d) if d.as_str() == "x"),
        "",
    );

    // 声明优先于假定（无 BOM 时）。
    let d_decl = convert_source(
        &utf16le("ok"),
        &DecodeConfig::new().with_declared(SourceEncoding::Utf16Le),
    );
    set.add(
        "C15-假定-声明优先于假定（无 BOM）",
        matches!(&d_decl, Ok(d) if d.meta.encoding == SourceEncoding::Utf16Le
            && d.meta.source == DetectionSource::Declared
            && d.as_str() == "ok"),
        "",
    );

    // ---- 判据：非法报错（六类 UTF-8 分立 + 宽编码故障）----

    // 六类 UTF-8 非法形态：孤立续字节 / 过长编码 / 代理项 / 超上限 / 截断 / 非法首字节。
    let six: [(&'static str, Vec<u8>, &'static str); 5] = [
        ("VE-F0415-UTF8-CONTINUATION", vec![0x41, 0x80, 0x42], "孤立续字节"),
        ("VE-F0415-UTF8-OVERLONG", vec![0xC0, 0xAF], "过长编码 C0"),
        ("VE-F0415-UTF8-SURROGATE", vec![0xED, 0xA0, 0x80], "代理项"),
        ("VE-F0415-UTF8-RANGE", vec![0xF5, 0x80, 0x80, 0x80], "超上限"),
        ("VE-F0415-UTF8-TRUNCATED", vec![0x41, 0xE2, 0x82], "截断"),
    ];
    let mut all_six = true;
    let mut six_detail: &'static str = "";
    for (code, bytes, human) in six.iter() {
        match convert_source(bytes, &DecodeConfig::new()) {
            Err(f) => {
                if f.code() != *code {
                    all_six = false;
                    six_detail = human;
                }
            }
            Ok(_) => {
                all_six = false;
                six_detail = human;
            }
        }
    }
    set.add("C15-非法-五类 UTF-8 非法形态各自独立码", all_six, six_detail);

    // 过编码的第二字节收紧：E0 80 必须判过编码而非通过。
    let d_e0 = convert_source(&[0xE0, 0x80, 0xAF], &DecodeConfig::new());
    set.add(
        "C15-非法-过长编码第二字节收紧（E0 80 拒）",
        matches!(&d_e0, Err(f) if f.code() == "VE-F0415-UTF8-OVERLONG"),
        "",
    );

    // F4 90 超上限必须被 F4 的紧上界 8F 拒掉。
    let d_f4 = convert_source(&[0xF4, 0x90, 0x80, 0x80], &DecodeConfig::new());
    set.add(
        "C15-非法-F4 第二字节上界收紧（> U+10FFFF 拒）",
        matches!(&d_f4, Err(f) if f.code() == "VE-F0415-UTF8-RANGE"),
        "",
    );

    // 错误带字节下标（判据三：位置）。
    let d_pos = convert_source(&[0x41, 0x42, 0x43, 0x80], &DecodeConfig::new());
    set.add(
        "C15-非法-错误带字节下标（位置）",
        matches!(&d_pos, Err(f) if f.index() == 3),
        "",
    );

    // 错误带字节值（判据三：字节值）。
    set.add(
        "C15-非法-错误带字节值",
        matches!(&d_pos, Err(f) if f.byte_value() == 0x80),
        "",
    );

    // 三要素齐备：码 + 位置 + 建议 + 行列。
    let rep = convert_source_report(&[0x41, 0x42, 0x43, 0x80], &DecodeConfig::new());
    let rep_ok = match &rep {
        Err(s) => {
            // 注意用 contains：行列子串并不在文首（文首是错误码），
            // 早前版本误用 has_prefix 导致本项恒假。
            has_prefix(s.as_str(), "[VE-F0415-UTF8-CONTINUATION]")
                && s.contains("第 1 行第 4 列")
                && s.contains("字节偏移 3")
                && s.contains("0x80")
                && s.contains("建议：")
        }
        Ok(_) => false,
    };
    set.add("C15-非法-三要素齐备（码+行列+字节值+建议）", rep_ok, "");

    // 行列随出错位置推进（第二个出错字节在第 2 行）。
    let two_lines: Vec<u8> = {
        let mut v: Vec<u8> = Vec::new();
        v.extend_from_slice(b"ok\n");
        v.push(0x80);
        v
    };
    let rep2 = convert_source_report(&two_lines, &DecodeConfig::new());
    set.add(
        "C15-非法-行列随出错位置推进",
        matches!(&rep2, Err(s) if has_prefix(s.as_str(), "[VE-F0415-UTF8-CONTINUATION]")
            && s.contains("第 2 行第 1 列")),
        "",
    );

    // UTF-16 孤立高代理项。
    let lone_high: Vec<u8> = vec![0x00, 0xD8, 0x41, 0x00];
    set.add(
        "C15-非法-UTF-16 孤立高代理项报错",
        matches!(&convert_source(&lone_high, &DecodeConfig::new().with_declared(SourceEncoding::Utf16Le)),
            Err(f) if f.code() == "VE-F0415-UTF16-HIGH"),
        "",
    );

    // UTF-16 孤立低代理项。
    let lone_low: Vec<u8> = vec![0x00, 0xDC, 0x41, 0x00];
    set.add(
        "C15-非法-UTF-16 孤立低代理项报错",
        matches!(&convert_source(&lone_low, &DecodeConfig::new().with_declared(SourceEncoding::Utf16Le)),
            Err(f) if f.code() == "VE-F0415-UTF16-LOW"),
        "",
    );

    // UTF-16 奇数长度。
    let odd: Vec<u8> = vec![0x41, 0x00, 0x42];
    set.add(
        "C15-非法-UTF-16 奇数字节长度报错",
        matches!(&convert_source(&odd, &DecodeConfig::new().with_declared(SourceEncoding::Utf16Le)),
            Err(f) if f.code() == "VE-F0415-UTF16-ODD"),
        "",
    );

    // UTF-32 越界标量（> U+10FFFF）。
    let oob: Vec<u8> = 0x0011_0000u32.to_le_bytes().to_vec();
    set.add(
        "C15-非法-UTF-32 标量越界报错",
        matches!(&convert_source(&oob, &DecodeConfig::new().with_declared(SourceEncoding::Utf32Le)),
            Err(f) if f.code() == "VE-F0415-UTF32-SCALAR"),
        "",
    );

    // UTF-32 落在代理项区（D800..DFFF）。
    let sur: Vec<u8> = 0x0000_D800u32.to_le_bytes().to_vec();
    set.add(
        "C15-非法-UTF-32 代理项区标量报错",
        matches!(&convert_source(&sur, &DecodeConfig::new().with_declared(SourceEncoding::Utf32Le)),
            Err(f) if f.code() == "VE-F0415-UTF32-SCALAR"),
        "",
    );

    // UTF-32 长度不整除。
    let bad_len: Vec<u8> = vec![0x41, 0x00, 0x00];
    set.add(
        "C15-非法-UTF-32 长度不整除报错",
        matches!(&convert_source(&bad_len, &DecodeConfig::new().with_declared(SourceEncoding::Utf32Le)),
            Err(f) if f.code() == "VE-F0415-UTF32-LENGTH"),
        "",
    );

    // 错误码不合并：六类 UTF-8 码两两不同 + 与宽编码码不同。
    let mut codes: Vec<&'static str> = Vec::new();
    for (_, bytes, _) in six.iter() {
        if let Err(f) = convert_source(bytes, &DecodeConfig::new()) {
            codes.push(f.code());
        }
    }
    let mut uniq = codes.clone();
    uniq.sort_unstable();
    uniq.dedup();
    set.add(
        "C15-非法-错误码不合并（五类互异）",
        codes.len() == 5 && uniq.len() == 5,
        "",
    );

    // ---- 判据：一次转换 ----

    // UTF-8 全族：ASCII / 两字节 / 三字节 / 四字节。
    let fam: &str = "A\u{00E9}\u{4E2D}\u{1F600}";
    let d_fam = convert_source(fam.as_bytes(), &DecodeConfig::new());
    set.add(
        "C15-转换-UTF-8 全族（1/2/3/4 字节）往返一致",
        matches!(&d_fam, Ok(d) if d.as_str() == fam && d.meta.char_len == 4),
        "",
    );

    // UTF-16LE / BE 往返一致（含四字节代理对）。
    let d_16le = convert_source(&utf16le(fam), &DecodeConfig::new().with_declared(SourceEncoding::Utf16Le));
    set.add(
        "C15-转换-UTF-16LE 往返一致（含代理对）",
        matches!(&d_16le, Ok(d) if d.as_str() == fam),
        "",
    );
    let d_16be = convert_source(&utf16be(fam), &DecodeConfig::new().with_declared(SourceEncoding::Utf16Be));
    set.add(
        "C15-转换-UTF-16BE 往返一致（含代理对）",
        matches!(&d_16be, Ok(d) if d.as_str() == fam),
        "",
    );

    // 代理对确实合成了单个四字节标量（而非两个孤立项）。
    let d_pair = convert_source(&[0x3D, 0xD8, 0x00, 0xDE], &DecodeConfig::new().with_declared(SourceEncoding::Utf16Le));
    set.add(
        "C15-转换-代理对合成单标量（U+1F600）",
        matches!(&d_pair, Ok(d) if d.as_str() == "\u{1F600}" && d.meta.char_len == 1),
        "",
    );

    // UTF-32 双端往返。
    let d_32le = convert_source(&utf32(fam, true), &DecodeConfig::new().with_declared(SourceEncoding::Utf32Le));
    set.add(
        "C15-转换-UTF-32LE 往返一致",
        matches!(&d_32le, Ok(d) if d.as_str() == fam),
        "",
    );
    let d_32be = convert_source(&utf32(fam, false), &DecodeConfig::new().with_declared(SourceEncoding::Utf32Be));
    set.add(
        "C15-转换-UTF-32BE 往返一致",
        matches!(&d_32be, Ok(d) if d.as_str() == fam),
        "",
    );

    // 单遍自证：消费字节数 == 源字节数，遍数恒 1。
    let big: Vec<u8> = {
        let mut v: Vec<u8> = Vec::new();
        for i in 0..512u32 {
            v.extend_from_slice(format!("n{} ", i).as_bytes());
        }
        v
    };
    let d_big = convert_source(&big, &DecodeConfig::new());
    set.add(
        "C15-转换-单遍自证（消费字节=源字节、遍数=1）",
        matches!(&d_big, Ok(d) if d.boundary.is_single_pass()
            && d.boundary.bytes_consumed == big.len()
            && d.boundary.passes == 1),
        "",
    );

    // 带 BOM 时单遍也成立（消费 = 正文 + BOM）。
    let big_bom = with_bom(&[0xEF, 0xBB, 0xBF], &big);
    let d_big_bom = convert_source(&big_bom, &DecodeConfig::new());
    set.add(
        "C15-转换-带 BOM 单遍自证（消费=正文+BOM）",
        matches!(&d_big_bom, Ok(d) if d.boundary.is_single_pass()
            && d.boundary.bytes_consumed == big_bom.len()
            && d.boundary.bom_stripped == 3),
        "",
    );

    // UTF-8 无 BOM 走「只校验+整块拷贝」路径；带 BOM 走重建路径。
    set.add(
        "C15-转换-UTF-8 无 BOM 走只校验路径",
        matches!(&d_big, Ok(d) if d.boundary.validate_only && d.meta.validate_only),
        "",
    );
    set.add(
        "C15-转换-宽编码必走重建路径（不谎称只校验）",
        matches!(&d_16le, Ok(d) if !d.boundary.validate_only),
        "",
    );

    // 产物恒不含替换字符（锚点：转换失败不静默替换）。
    let mut no_repl = matches!(&d_fam, Ok(d) if !contains_replacement_char(d.as_str()));
    no_repl &= matches!(&d_16le, Ok(d) if !contains_replacement_char(d.as_str()));
    no_repl &= matches!(&d_32be, Ok(d) if !contains_replacement_char(d.as_str()));
    // 注：不对 U+FFFD 字面量本身做断言——那是恒真/恒假的自证式写法，无判据价值。
    set.add("C15-显性-产物恒不含 U+FFFD 替换字符", no_repl, "");

    // BOM 剥离后正文正确（BOM 不混进正文）：正文须与原文逐字同值、跨度相同，
    // 且首字符不是 U+FEFF。用合法 UTF-8 原文作逐字比对，不用逐字节 as char 映射
    // （那对非 ASCII 是错的），也不用 || 兜底成弱门禁。
    let big_text: String = {
        let mut t = String::new();
        for i in 0..512u32 {
            t.push_str(format!("n{} ", i).as_str());
        }
        t
    };
    let big2 = big_text.as_bytes().to_vec();
    let big2_bom = with_bom(&[0xEF, 0xBB, 0xBF], &big2);
    let d_big2_bom = convert_source(&big2_bom, &DecodeConfig::new());
    set.add(
        "C15-转换-BOM 剥离后正文与原文逐字一致",
        matches!(&d_big2_bom, Ok(d) if d.as_str() == big_text.as_str()
            && d.meta.body_bytes == big2.len()
            && !d.as_str().starts_with('\u{FEFF}')),
        "",
    );

    // strip_bom=false 时 BOM 字节按 U+FEFF 参与正文（ZWNBSP 歧义显式化）。
    let zwsp = convert_source(
        &with_bom(&[0xEF, 0xBB, 0xBF], b"a"),
        &DecodeConfig::new().with_strip_bom(false),
    );
    set.add(
        "C15-转换-strip_bom=false 时 BOM 按 U+FEFF 入正文",
        matches!(&zwsp, Ok(d) if d.as_str() == "\u{FEFF}a"),
        "",
    );

    // 词法桥产出 UTF-8（判据四的对外边界）。
    let bridged = to_lexer_text(fam.as_bytes(), &DecodeConfig::new());
    set.add(
        "C15-转换-词法桥产出 UTF-8 正文",
        matches!(&bridged, Ok(s) if s.as_str() == fam),
        "",
    );

    // 桥遇非法字节必须 Err（不得带替换字符过桥）。
    set.add(
        "C15-转换-词法桥遇非法字节显性失败（不过桥）",
        matches!(to_lexer_text(&[0x41, 0x80], &DecodeConfig::new()), Err(_)),
        "",
    );

    // 空输入不炸（BOM 探测越界防护）。
    let empty: [u8; 0] = [];
    set.add(
        "C15-转换-空输入安全（空正文）",
        matches!(&convert_source(&empty, &DecodeConfig::new()),
            Ok(d) if d.as_str().is_empty() && d.meta.byte_len == 0),
        "",
    );

    // 截断的 BOM（只有 EF BB）不误判为 UTF-8 BOM，按非法序列报。
    set.add(
        "C15-转换-截断 BOM 不误判",
        matches!(&convert_source(&[0xEF, 0xBB], &DecodeConfig::new()),
            Err(f) if f.code() == "VE-F0415-UTF8-TRUNCATED"),
        "",
    );

    // ---- 判据：编码信息入诊断元数据 ----

    set.add(
        "C15-元数据-编码信息入元数据（BOM 路径）",
        matches!(&d_big_bom, Ok(d) if d.meta.encoding == SourceEncoding::Utf8
            && d.meta.source == DetectionSource::Bom
            && d.meta.bom_len == 3
            && d.meta.byte_len == big_bom.len()
            && d.meta.body_bytes == big.len()
            && d.meta.char_len > 0),
        "",
    );

    set.add(
        "C15-元数据-判定来源三态可区分",
        matches!(&d_plain, Ok(a) if a.meta.source == DetectionSource::AssumedUtf8)
            && matches!(&d_decl, Ok(b) if b.meta.source == DetectionSource::Declared)
            && matches!(&d_big_bom, Ok(c) if c.meta.source == DetectionSource::Bom),
        "",
    );

    set.add(
        "C15-元数据-目标形态恒为 UTF-8",
        matches!(&d_16be, Ok(d) if d.boundary.target == "UTF-8"),
        "",
    );

    // 摘要非空且含编码标签（诊断抬头可用）。
    let summary_ok = match &d_16le {
        Ok(d) => {
            let s = d.meta.summary();
            s.contains("UTF-16LE") && s.contains("字符")
        }
        Err(_) => false,
    };
    set.add("C15-元数据-摘要非空且含编码标签", summary_ok, "");

    // 注记报告非空（BOM 剥离 + 冲突告知都进注记流）。
    let note_ok = match &d_mix {
        Ok(d) => {
            let r = d.note_report();
            has_prefix(r.as_str(), "源文件带")
        }
        Err(_) => false,
    };
    set.add("C15-显性-注记报告非空（冲突进注记流）", note_ok, "");

    // ZWNBSP 歧义注记存在（strip_bom=true 且带 UTF-8 BOM）。
    set.add(
        "C15-显性-ZWNBSP 歧义出注记",
        matches!(&d_mix, Ok(d) if d.notes.iter().any(|n| n.describe().contains("既是 BOM"))),
        "",
    );

    // BOM 剥离出注记。
    set.add(
        "C15-显性-BOM 剥离出注记（记字节数）",
        matches!(&d_mix, Ok(d) if d.notes.iter().any(|n| {
            let t = n.describe();
            t.contains("检测到UTF-8 BOM") && t.contains("3 字节") && t.contains("已剥离")
        })),
        "",
    );

    // 码元宽与编码对应（UTF-16 报 2、UTF-32 报 4、UTF-8 报 1）。
    set.add(
        "C15-元数据-码元宽与编码对应",
        SourceEncoding::Utf8.unit_bytes() == 1
            && SourceEncoding::Utf16Le.unit_bytes() == 2
            && SourceEncoding::Utf16Be.unit_bytes() == 2
            && SourceEncoding::Utf32Le.unit_bytes() == 4
            && SourceEncoding::Utf32Be.unit_bytes() == 4,
        "",
    );

    // 宽编码的字节可编码性裁定（供 F0407 `\xNN` 用）。
    set.add(
        "C15-元数据-字节可编码性裁定（UTF-8 限 ASCII）",
        SourceEncoding::Utf8.byte_encodable(0x41)
            && !SourceEncoding::Utf8.byte_encodable(0xC3)
            && SourceEncoding::Utf16Le.byte_encodable(0xC3),
        "",
    );

    set
}

#[cfg(test)]
mod red_report {
    use super::*;
    #[test]
    fn report_red_items() {
        let set = run_vec15_checks();
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    println!("RED: {} | {}", c.name, c.detail);
                }
            }
        }
        println!("total={} dropped={}", set.len(), set.dropped());
    }
}
