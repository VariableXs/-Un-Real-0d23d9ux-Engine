
// ---------------------------------------------------------------------------
// F013 · 深化批次三：相对路径目标解析（按快捷方式所在目录）+ ShowCommand 解码
//
// 主册依据（G-A-13【设计细节】）：「相对路径目标按快捷方式所在目录解析」；
// 【功能定义】六字段之「窗口风格」（ShowCommand）。既有面：expand_target
// （环境变量目标）、RESOLVE_DEPTH_MAX（链式跳转深度）不重复。
// ---------------------------------------------------------------------------

/// 相对路径判定：无盘符（不含 ':'）且非 UNC（不以 '\\' 开头）= 相对目标。
fn is_relative_target(target: &str) -> bool {
    !target.contains(':') && !target.starts_with('\\')
}

/// 相对路径目标解析：相对目标 → 快捷方式所在目录拼接；绝对/UNC 目标原样。
/// 返回写入字节数（缓冲不足如实截断——截断结果不冒充完整路径）。
pub fn resolve_relative_target(lnk_dir: &str, target: &str, buf: &mut [u8]) -> usize {
    let joined: alloc::string::String = if is_relative_target(target) {
        if lnk_dir.is_empty() {
            // 无所在目录信息 → 相对目标无处落脚，如实落成相对形态（不猜盘符）。
            alloc::string::String::from(target)
        } else {
            let mut s = alloc::string::String::from(lnk_dir);
            if !s.ends_with('\\') {
                s.push('\\');
            }
            s.push_str(target);
            s
        }
    } else {
        alloc::string::String::from(target)
    };
    let src = joined.as_bytes();
    let n = src.len().min(buf.len());
    buf[..n].copy_from_slice(&src[..n]);
    n
}

/// 快捷方式窗口风格解码（ShowCommand——六字段之「窗口风格」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LnkShowCmd {
    /// SW_SHOWNORMAL（1）。
    Normal,
    /// SW_SHOWMAXIMIZED（3）。
    Maximized,
    /// SW_SHOWMINNOACTIVE（7）。
    Minimized,
    /// 其余值如实携带（不猜语义——向前兼容）。
    Other(u32),
}

pub fn decode_show_cmd(v: u32) -> LnkShowCmd {
    match v {
        SW_SHOWNORMAL => LnkShowCmd::Normal,
        SW_SHOWMAXIMIZED => LnkShowCmd::Maximized,
        SW_SHOWMINNOACTIVE => LnkShowCmd::Minimized,
        other => LnkShowCmd::Other(other),
    }
}

/// F013 深化批次三自检。
pub fn run_lnkfile_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F013-lnkfile-deep2");
    // 1) 相对目标拼接：所在目录 + 相对目标（含多级段）；目录尾分隔符不重复。
    let mut buf = [0u8; 96];
    let n1 = resolve_relative_target("C:\\Tools", "app\\tool.exe", &mut buf);
    let joined_ok = &buf[..n1] == b"C:\\Tools\\app\\tool.exe";
    let n2 = resolve_relative_target("C:\\Tools\\", "tool.exe", &mut buf);
    let no_double_slash = &buf[..n2] == b"C:\\Tools\\tool.exe";
    cs.add(
        "relative_target_joins_lnk_dir",
        joined_ok && no_double_slash,
        "",
    );
    // 2) 绝对/UNC 目标原样；无目录信息的相对目标保持相对形态（不猜盘符）。
    let n3 = resolve_relative_target("C:\\Tools", "D:\\Apps\\a.exe", &mut buf);
    let abs_ok = &buf[..n3] == b"D:\\Apps\\a.exe";
    let n4 = resolve_relative_target("", "tool.exe", &mut buf);
    let bare_ok = &buf[..n4] == b"tool.exe";
    cs.add(
        "absolute_passthrough_and_bare_relative",
        abs_ok && bare_ok,
        "",
    );
    // 3) ShowCommand：1/3/7 三钉值解码；未知值如实 Other 携带（向前兼容）。
    cs.add(
        "show_cmd_decode",
        decode_show_cmd(1) == LnkShowCmd::Normal
            && decode_show_cmd(3) == LnkShowCmd::Maximized
            && decode_show_cmd(7) == LnkShowCmd::Minimized
            && decode_show_cmd(11) == LnkShowCmd::Other(11)
            && SW_SHOWNORMAL == 1
            && SW_SHOWMAXIMIZED == 3
            && SW_SHOWMINNOACTIVE == 7,
        "",
    );
    cs
}
