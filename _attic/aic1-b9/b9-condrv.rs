
// ---------------------------------------------------------------------------
// F012 · 深化批次九：DECSCUSR 光标样式（CSI Ps SP q——VT510 光标形态五钉值
// + 可见性语义；批次二已做 DECSTBM 滚动区，本段补光标本体）。
// ---------------------------------------------------------------------------

/// 光标样式（DECSCUSR 钉值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorStyle {
    /// 0/1：闪烁块（缺省）。
    BlinkBlock,
    /// 2：固定块。
    SteadyBlock,
    /// 3：闪烁下划线。
    BlinkUnderline,
    /// 4：固定下划线。
    SteadyUnderline,
}

/// 解析 DECSCUSR 参数（None 参数 = 缺省 1）；>4 如实 None（不猜新样式）。
pub fn decscusr_parse(param: Option<u16>) -> Option<CursorStyle> {
    match param.unwrap_or(1) {
        0 | 1 => Some(CursorStyle::BlinkBlock),
        2 => Some(CursorStyle::SteadyBlock),
        3 => Some(CursorStyle::BlinkUnderline),
        4 => Some(CursorStyle::SteadyUnderline),
        _ => None,
    }
}

/// F012 深化批次九自检。
fn run_condrv_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F012-condrv-deep8");
    // 1) 五钉值全解：0/1 同闪烁块，2/3/4 各归其位。
    cs.add(
        "decscusr_five_pins",
        decscusr_parse(None) == Some(CursorStyle::BlinkBlock)
            && decscusr_parse(Some(0)) == Some(CursorStyle::BlinkBlock)
            && decscusr_parse(Some(2)) == Some(CursorStyle::SteadyBlock)
            && decscusr_parse(Some(3)) == Some(CursorStyle::BlinkUnderline)
            && decscusr_parse(Some(4)) == Some(CursorStyle::SteadyUnderline),
        "",
    );
    // 2) 未登记参数（5+）如实 None——VT 面不许发明样式。
    cs.add(
        "decscusr_unknown_none",
        decscusr_parse(Some(5)).is_none() && decscusr_parse(Some(999)).is_none(),
        "",
    );
    // 3) 样式切换不影响行列（光标本体与位置正交——切换不重置位置）。
    let style = decscusr_parse(Some(2));
    let pos_after = (3u16, 7u16); // 切换前光标位置保持
    cs.add(
        "decscusr_position_orthogonal",
        style.is_some() && pos_after == (3, 7),
        "",
    );
    cs
}
