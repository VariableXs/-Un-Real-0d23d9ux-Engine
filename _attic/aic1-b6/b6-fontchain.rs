
// ---------------------------------------------------------------------------
// F016 · 深化批次六：缺字清单报告（前 20 字展示——F159 判据的字体面数据源）
//
// 主册依据（G-A-16【状态与异常】）：「字体缺失字符 → 回退链逐级（最终 tofu
// 显式可见）」+ F159【验收判据】：「缺字清单前 20 字展示准确」——回退链扫
// 字符测试集，产出缺失清单（有界 20 条，超额如实计数）。
// ---------------------------------------------------------------------------

/// 缺字报告（最多 20 字 + 超额计数）。
#[derive(Clone, Copy, Debug)]
pub struct MissingGlyphReport {
    pub missing: [&'static str; 20],
    pub n: usize,
    /// 超出 20 条的缺失数（如实计数——不静默截断）。
    pub overflow: u32,
}

/// 扫描字符测试集（chars 为 &'static str 引用集），产出缺字清单。
pub fn missing_glyph_report(
    chain: &[&'static str],
    covers: fn(&str, &str) -> bool,
    chars: &[&'static str],
) -> MissingGlyphReport {
    let mut rep = MissingGlyphReport { missing: [""; 20], n: 0, overflow: 0 };
    for ch in chars {
        let resolved = chain.iter().any(|fam| covers(fam, ch));
        if !resolved {
            if rep.n < 20 {
                rep.missing[rep.n] = ch;
                rep.n += 1;
            } else {
                rep.overflow += 1;
            }
        }
    }
    rep
}

/// F016 深化批次六自检。
pub fn run_fontchain_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F016-fontchain-deep5");
    // 覆盖桩：仅 "varix-sans" 认识 集合 A 中的字。
    fn covers_stub(family: &str, ch: &str) -> bool {
        family == "varix-sans" && matches!(ch, "\u{4E2D}" | "\u{6587}" | "\u{6D4B}")
    }
    // 1) 部分缺字：3 认识 + 2 缺 → 清单恰 2 条（无溢出）。
    let chars = ["\u{4E2D}", "\u{6587}", "\u{6D4B}", "\u{7F3A}", "\u{5B57}"];
    let rep = missing_glyph_report(&["varix-sans"], covers_stub, &chars);
    cs.add(
        "missing_glyph_report_basic",
        rep.n == 2 && rep.overflow == 0 && rep.missing[0] == "\u{7F3A}" && rep.missing[1] == "\u{5B57}",
        "",
    );
    // 2) 超额如实计数：25 字全缺 → 清单 20 条 + 溢出 5（不静默截断）。
    let many: [&'static str; 25] = core::array::from_fn(|i| match i {
        0 => "\u{0001}", 1 => "\u{0002}", 2 => "\u{0003}", 3 => "\u{0004}", 4 => "\u{0005}",
        5 => "\u{0006}", 6 => "\u{0007}", 7 => "\u{0008}", 8 => "\u{0009}", 9 => "\u{000A}",
        10 => "\u{000B}", 11 => "\u{000C}", 12 => "\u{000D}", 13 => "\u{000E}", 14 => "\u{000F}",
        15 => "\u{0010}", 16 => "\u{0011}", 17 => "\u{0012}", 18 => "\u{0013}", 19 => "\u{0014}",
        _ => "\u{0015}",
    });
    let rep2 = missing_glyph_report(&["varix-sans"], covers_stub, &many);
    cs.add(
        "missing_glyph_overflow_honest",
        rep2.n == 20 && rep2.overflow == 5,
        "",
    );
    // 3) 全认识 → 空清单（零噪声报告——诊断面不造假缺字）。
    let rep3 = missing_glyph_report(&["varix-sans"], covers_stub, &["\u{4E2D}", "\u{6587}"]);
    cs.add("missing_glyph_none_clean", rep3.n == 0 && rep3.overflow == 0, "");
    cs
}
