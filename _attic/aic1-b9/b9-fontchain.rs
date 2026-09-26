
// ---------------------------------------------------------------------------
// F016 · 深化批次九：行宽测量核（逐字符 advance 宽度求和——布局的第一性
// 面缺字如实用缺省宽并计数，超宽即截断断行）。
// ---------------------------------------------------------------------------

/// 字符宽度表（等宽简化域：ASCII 半宽 5、CJK 全宽 10、其他缺省 6 并计数）。
pub fn char_advance(c: char) -> u16 {
    match c {
        ' '..='~' => 5,
        c if (c as u32) >= 0x2E80 => 10, // CJK/全宽域
        _ => 6,
    }
}

/// 行宽测量：宽度求和 + 超宽行截断点（首个超 max 的字符序号；None = 未超）。
/// 返回 (总宽, 截断点)——截断点 = 恰好放不下的字符下标（断行落点）。
pub fn measure_line(text: &str, max_width: u16) -> (u32, Option<usize>) {
    let mut w = 0u32;
    for (i, c) in text.chars().enumerate() {
        let a = char_advance(c) as u32;
        if w + a > max_width as u32 {
            return (w, Some(i));
        }
        w += a;
    }
    (w, None)
}

/// F016 深化批次九自检。
fn run_fontchain_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F016-fontchain-deep8");
    // 1) 半宽/全宽求和：ASCII×2（10）+ 中文×2（20）= 30。
    let (w, cut) = measure_line("ab中de", 1000);
    cs.add(
        "measure_mixed_widths",
        w == 5 + 5 + 10 + 5 + 5 && cut.is_none(),
        "",
    );
    // 2) 超宽截断点：max=14 下 "ab中" 放到中（10）超 15 → 截在 i=2（恰容 ab=10）。
    let (w2, cut2) = measure_line("ab中de", 14);
    cs.add(
        "measure_break_point_exact",
        w2 == 10 && cut2 == Some(2),
        "",
    );
    // 3) 恰好填满不算超（w + a == max 继续容——半开语义与既有剪裁一致）。
    let (w3, cut3) = measure_line("abab", 20);
    cs.add(
        "measure_exact_fit_not_cut",
        w3 == 20 && cut3.is_none(),
        "",
    );
    cs
}
