//! VE-F0406 · 域自检（判据逐条对应，见 `vec06_lit.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 全族解析 → `C06-整型-*`、`C06-浮点-*`
//! - 溢出报错（带边界建议）→ `C06-溢出-*`
//! - 精度警告（可配置升级）→ `C06-精度-*`
//! - 原文保真 → `C06-保真-*`

use super::vec06_lit::*;
use crate::checks::CheckSet;

/// VE-F0406 域自检。
pub fn run_vec06_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec06");
    let cfg = LiteralConfig::default_config();

    // ---- 判据：全族解析 · 整型 ----

    // 十进制默认 i32
    {
        let p = parse_literal("12345", &cfg).unwrap();
        set.add(
            "C06-整型-十进制默认i32",
            p.value == LiteralValue::Int {
                value: 12345,
                suffix: IntSuffix::I32,
                radix: Radix::Dec,
            },
            "",
        );
    }
    // 十六进制与八进制（含值等价对拍：0x1F == 31 == 0o37）
    {
        let hex = parse_literal("0x1F", &cfg).unwrap();
        let oct = parse_literal("0o37", &cfg).unwrap();
        let val = |p: &ParsedLiteral| match p.value {
            LiteralValue::Int { value, .. } => value,
            _ => 0,
        };
        let same = val(&hex) == 31 && val(&oct) == 31;
        let radixes = matches!(hex.value, LiteralValue::Int { radix: Radix::Hex, .. })
            && matches!(oct.value, LiteralValue::Int { radix: Radix::Oct, .. });
        set.add("C06-整型-十六与八进制", same && radixes, "");
    }
    // 后缀位宽四档
    {
        let s = |t: &str| match parse_literal(t, &cfg).unwrap().value {
            LiteralValue::Int { suffix, .. } => suffix,
            _ => panic!(),
        };
        set.add(
            "C06-整型-后缀位宽四档",
            s("5") == IntSuffix::I32
                && s("5u") == IntSuffix::U32
                && s("5L") == IntSuffix::I64
                && s("5UL") == IntSuffix::U64,
            "",
        );
    }
    // 数字分隔符合法用法
    {
        let p = parse_literal("1_000_000", &cfg).unwrap();
        set.add(
            "C06-整型-分隔符合法",
            p.value == LiteralValue::Int {
                value: 1_000_000,
                suffix: IntSuffix::I32,
                radix: Radix::Dec,
            },
            "",
        );
    }
    // 分隔符前导/尾随/连续拒绝
    {
        let bad = ["_100", "100_", "10__0"];
        let all = bad.iter().all(|t| {
            parse_literal(t, &cfg)
                .as_ref()
                .err()
                .map(|e| e.code == "E_LIT_SEPARATOR" && e.is_complete())
                .unwrap_or(false)
        });
        set.add("C06-整型-分隔符滥用拒绝", all, "");
    }
    // 非法进制数字（0x 后 g）带位置与合法集
    {
        let e = parse_literal("0xG1", &cfg);
        let ok = e.as_ref().err().map(|x| {
            x.code == "E_LIT_MALFORMED" && x.next.contains("a-f")
        }).unwrap_or(false);
        set.add("C06-整型-非法进制数字", ok, "");
    }

    // ---- 判据：全族解析 · 浮点 ----

    // 小数、指数、科学计数、f32/f64 后缀
    {
        let v = |t: &str| match parse_literal(t, &cfg).unwrap().value {
            LiteralValue::Float { value, suffix } => (value, suffix),
            _ => panic!(),
        };
        let (a, sa) = v("2.5");
        let (b, sb) = v("1e3");
        let (c, sc) = v("2.5e-2");
        let (d, sd) = v("0.5f");
        let (e, se) = v("0.5LF");
        set.add(
            "C06-浮点-小数指数科学计数",
            (a - 2.5).abs() < 1e-12
                && sa == FloatSuffix::F32
                && (b - 1000.0).abs() < 1e-9
                && sb == FloatSuffix::F32
                && (c - 0.025).abs() < 1e-12
                && sc == FloatSuffix::F32
                && (d - 0.5).abs() < 1e-12
                && sd == FloatSuffix::F32
                && (e - 0.5).abs() < 1e-12
                && se == FloatSuffix::F64,
            "",
        );
    }
    // 裸数字是整型不是浮点（浮点必须有 . 或指数）
    {
        let p = parse_literal("42", &cfg).unwrap();
        set.add(
            "C06-浮点-裸数字归整型",
            matches!(p.value, LiteralValue::Int { .. }),
            "",
        );
    }
    // 畸形族：裸 e、双点、指数缺数字、前缀缺数字、hex 浮点拒绝
    {
        let bad = ["1e", "1.2.3", "1e+", "0x", "0o", "0x1F.8"];
        let all = bad.iter().all(|t| {
            parse_literal(t, &cfg)
                .as_ref()
                .err()
                .map(|e| e.code == "E_LIT_MALFORMED" && e.is_complete())
                .unwrap_or(false)
        });
        set.add("C06-浮点-畸形全拒", all, "");
    }
    // 非法后缀报错带合法集
    {
        let e = parse_literal("1.2x", &cfg);
        let ok = e.as_ref().err().map(|x| {
            x.code == "E_LIT_SUFFIX" && x.next.contains("lf")
        }).unwrap_or(false);
        set.add("C06-浮点-非法后缀带合法集", ok, "");
    }

    // ---- 判据：溢出报错 ----

    {
        // i32 上界 2147483647：+1 即溢出，报错带上界
        let ok_max = parse_literal("2147483647", &cfg).is_ok();
        let e = parse_literal("2147483648", &cfg);
        let over = e.as_ref().err().map(|x| {
            x.code == "E_LIT_OVERFLOW" && x.what.contains("2147483647") && x.next.contains("位宽")
        }).unwrap_or(false);
        set.add("C06-溢出-i32越界带上界建议", ok_max && over, "");
    }
    // 各位宽上界内放行、上界+1 拒绝
    {
        let cases = [
            ("4294967295u", "4294967296u"),
            ("9223372036854775807l", "9223372036854775808l"),
            ("18446744073709551615ul", "18446744073709551616ul"),
        ];
        let all = cases
            .iter()
            .all(|(ok, over)| parse_literal(ok, &cfg).is_ok() && parse_literal(over, &cfg).is_err());
        set.add("C06-溢出-四位宽边界全测", all, "");
    }
    // 浮点溢出到无穷拒绝
    {
        let e = parse_literal("1e400", &cfg);
        set.add(
            "C06-溢出-浮点指数域",
            e.as_ref().err().map(|x| x.code == "E_LIT_OVERFLOW").unwrap_or(false),
            "",
        );
    }

    // ---- 判据：精度警告（可配置升级） ----

    {
        // 0.1 以 f32 存储有损失 → Warn 挂警告；0.5 无损失零警告
        let warn = parse_literal("0.1", &cfg).unwrap();
        let exact = parse_literal("0.5", &cfg).unwrap();
        set.add(
            "C06-精度-警告按损失挂账",
            warn.warnings.len() == 1 && warn.warnings[0].contains("原文") && exact.warnings.is_empty(),
            "",
        );
    }
    // 策略升级：Error 策略下同一字面量被拒
    {
        let strict = LiteralConfig {
            precision_policy: PrecisionPolicy::Error,
        };
        let e = parse_literal("0.1", &strict);
        let ok = e.as_ref().err().map(|x| {
            x.code == "E_LIT_PRECISION" && x.next.contains("0.5")
        }).unwrap_or(false);
        // 精确值在 Error 策略下仍放行
        let exact_ok = parse_literal("0.5", &strict).is_ok();
        set.add("C06-精度-策略可升级错误", ok && exact_ok, "");
    }

    // ---- 判据：原文保真 ----

    {
        let samples = ["0x1F", "1_000_000", "2.5e-2LF", "18446744073709551615ul"];
        let all = samples.iter().all(|t| {
            parse_literal(t, &cfg)
                .map(|p| p.original == *t)
                .unwrap_or(false)
        });
        set.add(
            "C06-保真-原文逐字节保留",
            all,
            "原文含前缀/分隔符/后缀，诊断引用不变样",
        );
    }

    // ---- 确定性 ----

    {
        let a = parse_literal("3.14159LF", &cfg);
        let b = parse_literal("3.14159LF", &cfg);
        set.add("C06-确定-同输入同结果", a == b, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn vec06_checks_all_green() {
        let set = run_vec06_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-C06 域自检红项：{}/{} 绿", passed, passed + failed);
            for it in items.iter().take(n) {
                if let Some(c) = it {
                    if !c.passed {
                        msg.push_str(&format!("\n  [红] {} — {}", c.name, c.detail));
                    }
                }
            }
            panic!("{}", msg);
        }
    }

    /// 全族 roundtrip：解析值与十进制期望一致（跨进制等价对拍）。
    #[test]
    fn radix_equivalence() {
        let cfg = LiteralConfig::default_config();
        let v = |t: &str| -> u128 {
            match parse_literal(t, &cfg).unwrap().value {
                LiteralValue::Int { value, .. } => value,
                _ => panic!(),
            }
        };
        assert_eq!(v("0xFF"), 255);
        assert_eq!(v("0o377"), 255);
        assert_eq!(v("255"), 255);
        assert_eq!(v("0x0"), 0);
    }

    /// 溢出零静默：所有位宽的越界都报错而不是截断。
    #[test]
    fn overflow_never_silent() {
        let cfg = LiteralConfig::default_config();
        for t in ["2147483648", "4294967296u", "9223372036854775808l", "18446744073709551616ul"] {
            let e = parse_literal(t, &cfg);
            assert!(e.is_err(), "{} 应溢出拒绝", t);
            assert_eq!(e.unwrap_err().code, "E_LIT_OVERFLOW");
        }
    }

    /// 原文保真的实质：警告里引用的原文能逐字对回源文本。
    #[test]
    fn original_is_verbatim() {
        let cfg = LiteralConfig::default_config();
        let src = "0.123456789LF";
        let p = parse_literal(src, &cfg).unwrap();
        assert_eq!(p.original, src);
        // f64 后缀 + 15 位有效数字 → 无启发式告警
        assert!(p.warnings.is_empty());
    }

    /// 精度策略两端：Warn 挂账、Error 拒绝、精确值两头都放行。
    #[test]
    fn precision_policy_both_ends() {
        let warn = LiteralConfig::default_config();
        let strict = LiteralConfig {
            precision_policy: PrecisionPolicy::Error,
        };
        assert_eq!(parse_literal("0.1", &warn).unwrap().warnings.len(), 1);
        assert_eq!(parse_literal("0.1", &strict).unwrap_err().code, "E_LIT_PRECISION");
        assert!(parse_literal("0.5", &strict).is_ok());
    }
}
