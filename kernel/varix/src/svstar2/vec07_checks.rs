//! VE-F0407 · 域自检（判据逐条对应，见 `vec07_string.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 转义全集 → `C07-转义-*`（含非法转义报错带位置与合法集）
//! - 原始串 → `C07-原始串-*`
//! - 编码显性 → `C07-编码-*`（字节 × 编码标记；\xNN 与 \uXXXX 裁定）
//! - 未闭合指向 → `C07-未闭合-*`（报错指向开引号）

use super::vec07_string::*;
use crate::checks::CheckSet;

fn decoded_str(t: &StringToken) -> String {
    // 测试辅助：解码缓冲按 UTF-8 语义还原（Latin1 分支的用例不走此函数）
    t.decoded.iter().map(|&b| b as char).collect()
}

/// VE-F0407 域自检。
pub fn run_vec07_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec07");
    let cfg = StringConfig::default_config();

    // ---- 判据：转义全集 ----

    // 七个直映射转义逐一 O(1) 查表
    {
        let t = parse_string("\"a\\nb\\tc\\rd\\\\e\\\"f\\'g\\0h\"", &cfg).unwrap();
        let d = decoded_str(&t);
        set.add(
            "C07-转义-直映射七种",
            d == "a\nb\tc\rd\\e\"f'g\0h" && t.kind == StringKind::Quoted,
            "",
        );
    }
    // \xNN 十六进制字节（两位硬性）
    {
        let t = parse_string("\"\\x41\\x00\\x7F\"", &cfg).unwrap();
        set.add(
            "C07-转义-十六进制字节",
            t.decoded == vec![0x41u8, 0x00, 0x7F],
            "",
        );
    }
    // \uXXXX Unicode 码点按 UTF-8 编码；\u{…} 花括号形式达增补平面
    {
        let t = parse_string("\"\\u4E2D\\u00E9\\u{1F600}\"", &cfg).unwrap();
        set.add(
            "C07-转义-Unicode码点",
            t.decoded == {
                let mut v = Vec::new();
                // 中 = E4 B8 AD；é = C3 A9；😀 = F0 9F 98 80
                v.extend_from_slice(&[0xE4, 0xB8, 0xAD, 0xC3, 0xA9, 0xF0, 0x9F, 0x98, 0x80]);
                v
            },
            "",
        );
    }
    // 非法转义：报错带位置 + 三要素含合法集
    {
        let e = parse_string("\"ab\\qcd\"", &cfg).unwrap_err();
        let (line, col) = StrError::line_col("\"ab\\qcd\"", e.pos);
        set.add(
            "C07-转义-非法转义报错",
            e.code == "E_STR_BAD_ESCAPE"
                && e.pos == 4
                && line == 1
                && col == 5
                && e.is_complete()
                && e.next.contains("\\xNN"),
            "",
        );
    }
    // 缺位/缺定界截断与非十六进制数字分层拒绝：
    // 收尾引号顶替数字位 → HEX；物理缺位/缺 } → TRUNC；超 Unicode 上界 → ENCODING
    {
        let trunc_x = parse_string("\"\\x\"", &cfg).unwrap_err();
        let trunc_u = parse_string("\"\\u4E\"", &cfg).unwrap_err();
        let trunc_brace_open = parse_string("\"\\u{\"", &cfg).unwrap_err();
        let trunc_brace_close = parse_string("\"\\u{12\"", &cfg).unwrap_err();
        let hex_x = parse_string("\"\\x4\"", &cfg).unwrap_err();
        let hex_u = parse_string("\"\\u4E2\"", &cfg).unwrap_err();
        let hex_brace = parse_string("\"\\u{G}\"", &cfg).unwrap_err();
        let enc_range = parse_string("\"\\u{110000}\"", &cfg).unwrap_err();
        set.add(
            "C07-转义-缺位截断拒绝",
            trunc_x.code == "E_STR_ESCAPE_TRUNC"
                && trunc_u.code == "E_STR_ESCAPE_TRUNC"
                && trunc_brace_open.code == "E_STR_ESCAPE_TRUNC"
                && trunc_brace_close.code == "E_STR_ESCAPE_TRUNC"
                && hex_x.code == "E_STR_ESCAPE_HEX"
                && hex_u.code == "E_STR_ESCAPE_HEX"
                && hex_brace.code == "E_STR_ESCAPE_HEX"
                && enc_range.code == "E_STR_ENCODING",
            "",
        );
    }
    // 非十六进制数字拒绝
    {
        let a = parse_string("\"\\xG1\"", &cfg).unwrap_err();
        let b = parse_string("\"\\u00G2\"", &cfg).unwrap_err();
        set.add(
            "C07-转义-非十六进制拒绝",
            a.code == "E_STR_ESCAPE_HEX" && b.code == "E_STR_ESCAPE_HEX",
            "",
        );
    }

    // ---- 判据：原始串 ----

    // r"…" 零转义：\n 就是两个字节
    {
        let t = parse_string("r\"a\\nb\"", &cfg).unwrap();
        set.add(
            "C07-原始串-零转义",
            t.kind == StringKind::Raw { hashes: 0 }
                && t.decoded == vec![b'a', b'\\', b'n', b'b'],
            "",
        );
    }
    // r#"…"# 井号定界：内容可含裸引号
    {
        let t = parse_string("r#\"he said \"hi\"\"#", &cfg).unwrap();
        set.add(
            "C07-原始串-井号定界",
            t.kind == StringKind::Raw { hashes: 1 }
                && decoded_str(&t) == "he said \"hi\"",
            "",
        );
    }
    // 原始串跨行合法，行数记账
    {
        let t = parse_string("r\"line1\nline2\"", &cfg).unwrap();
        set.add(
            "C07-原始串-跨行合法",
            t.decoded == vec![b'l', b'i', b'n', b'e', b'1', b'\n', b'l', b'i', b'n', b'e', b'2']
                && t.lines_spanned == 2,
            "",
        );
    }
    // 井号不配对 = 内容引号继续扫；定界残缺显性报错
    {
        let ok = parse_string("r#\"a\"b\"#", &cfg).unwrap();
        let bad = parse_string("r##\"abc\"", &cfg).unwrap_err();
        set.add(
            "C07-原始串-定界纪律",
            decoded_str(&ok) == "a\"b" && bad.code == "E_STR_UNCLOSED",
            "",
        );
    }

    // ---- 判据：编码显性 ----

    // 串记号携带编码标记（三编码可注入）
    {
        let a = parse_string("\"x\"", &StringConfig { encoding: SourceEncoding::Utf8 }).unwrap();
        let b = parse_string("\"x\"", &StringConfig { encoding: SourceEncoding::Latin1 }).unwrap();
        let c = parse_string("\"x\"", &StringConfig { encoding: SourceEncoding::Utf16 }).unwrap();
        set.add(
            "C07-编码-记号携带标记",
            a.encoding == SourceEncoding::Utf8
                && b.encoding == SourceEncoding::Latin1
                && c.encoding == SourceEncoding::Utf16,
            "",
        );
    }
    // UTF-8 下 \x80 裸高位字节拒绝（建议 \uXXXX）
    {
        let e = parse_string("\"\\x80\"", &cfg).unwrap_err();
        set.add(
            "C07-编码-UTF8裸高位字节拒绝",
            e.code == "E_STR_ENCODING" && e.next.contains("\\u"),
            "",
        );
    }
    // Latin-1 下 \u4E2D 超域拒绝；\xFF 直通
    {
        let enc = StringConfig { encoding: SourceEncoding::Latin1 };
        let a = parse_string("\"\\u4E2D\"", &enc).unwrap_err();
        let b = parse_string("\"\\xFF\"", &enc).unwrap();
        set.add(
            "C07-编码-Latin1裁定",
            a.code == "E_STR_ENCODING" && b.decoded == vec![0xFFu8],
            "",
        );
    }
    // UTF-16：BMP 单码元，增补平面（\u{…}）代理对
    {
        let enc = StringConfig { encoding: SourceEncoding::Utf16 };
        let bmp = parse_string("\"\\u4E2D\"", &enc).unwrap();
        let astral = parse_string("\"\\u{1F600}\"", &enc).unwrap();
        let mut expect = Vec::new();
        expect.extend_from_slice(&[0x2Du8, 0x4E]);
        set.add("C07-编码-UTF16-BMP", bmp.decoded == expect, "");
        let mut expect2 = Vec::new();
        expect2.extend_from_slice(&0xD83Du16.to_le_bytes());
        expect2.extend_from_slice(&0xDE00u16.to_le_bytes());
        set.add("C07-编码-UTF16-代理对", astral.decoded == expect2, "");
    }
    // 代理区码点全编码拒绝
    {
        let e = parse_string("\"\\uD800\"", &cfg).unwrap_err();
        set.add("C07-编码-代理区拒绝", e.code == "E_STR_ENCODING", "");
    }

    // ---- 判据：未闭合指向 ----

    // EOF 未闭合：报错位置 = 开引号
    {
        let e = parse_string("\"abc", &cfg).unwrap_err();
        let (line, col) = StrError::line_col("\"abc", e.pos);
        set.add(
            "C07-未闭合-EOF指向开引号",
            e.code == "E_STR_UNCLOSED" && e.pos == e.open_quote && line == 1 && col == 1,
            "",
        );
    }
    // 裸换行未闭合：指向开引号
    {
        let e = parse_string("\"ab\ncd\"", &cfg).unwrap_err();
        set.add(
            "C07-未闭合-裸换行指向开引号",
            e.code == "E_STR_UNCLOSED" && e.pos == 0 && e.is_complete(),
            "",
        );
    }
    // 孤悬反斜杠按未闭合处置
    {
        let e = parse_string("\"ab\\", &cfg).unwrap_err();
        set.add("C07-未闭合-孤悬反斜杠", e.code == "E_STR_UNCLOSED", "");
    }

    // ---- 边界与守护 ----

    // 空串与空内容
    {
        let empty_input = parse_string("", &cfg).unwrap_err();
        let empty_body = parse_string("\"\"", &cfg).unwrap();
        set.add(
            "C07-边界-空输入与空串",
            empty_input.code == "E_STR_EMPTY" && empty_body.decoded.is_empty(),
            "",
        );
    }
    // 非引号开头拒绝（单引号是另一族）
    {
        let e = parse_string("'a'", &cfg).unwrap_err();
        set.add("C07-边界-单引号拒绝", e.code == "E_STR_NO_QUOTE", "");
    }
    // 错误三要素完整性（抽全部错误码变体）
    {
        let errs = [
            parse_string("", &cfg).unwrap_err(),
            parse_string("'a'", &cfg).unwrap_err(),
            parse_string("\"ab\\q\"", &cfg).unwrap_err(),
            parse_string("\"\\x8\"", &cfg).unwrap_err(),
            parse_string("\"\\x80\"", &cfg).unwrap_err(),
            parse_string("\"abc", &cfg).unwrap_err(),
            parse_string("r##\"abc\"", &cfg).unwrap_err(),
        ];
        set.add(
            "C07-边界-三要素完整",
            errs.iter().all(|e| e.is_complete() && !e.what.is_empty()),
            "",
        );
    }
    // 原文保真：记号 original 逐字节等于输入
    {
        let src = "r#\"a\\n\"b\"#";
        let t = parse_string(src, &cfg).unwrap();
        set.add("C07-边界-原文保真", t.original == src, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// VE-F0407 全绿闸：红项逐行枚举（定位用），零红才算过。
    #[test]
    fn vec07_checks_all_green() {
        let set = run_vec07_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "vec07 自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
                .0
                .iter()
                .flatten()
                .filter(|c| !c.passed)
                .map(|c| c.name)
                .collect::<Vec<_>>()
        );
    }
}
