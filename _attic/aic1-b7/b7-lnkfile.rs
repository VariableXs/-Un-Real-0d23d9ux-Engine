
// ---------------------------------------------------------------------------
// F013 · 深化批次七：目标串清洗（外部输入纪律——控制字符拒绝）
//
// 主册依据（G-A-13【数据与存储】）+【安全与隐私】全局纪律：「外部输入全清
// 洗」——.lnk 目标串来自外部文件：控制字符（<0x20，除已消费的转义外）拒绝
// 入属性页与装载链；引号合法（含空格路径的 Windows 惯例）。
// ---------------------------------------------------------------------------

/// 目标串清洗判定：含 C0 控制字符（除 NUL 由解析器终结语义处理外——0x01..=0x1F
/// 与 0x7F）→ 拒绝；可打印与扩展字符（含中文/空格/引号）放行。
pub fn target_sanitized(target: &str) -> bool {
    !target
        .bytes()
        .any(|b| (0x01..=0x1F).contains(&b) || b == 0x7F)
}

/// F013 深化批次七自检。
pub fn run_lnkfile_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F013-lnkfile-deep6");
    // 1) 正常目标放行：含空格、引号、中文、UNC 全部合法。
    cs.add(
        "target_sanitized_normal_paths",
        target_sanitized("C:\\Program Files\\app.exe")
            && target_sanitized("\"C:\\my tools\\a b.exe\" -x")
            && target_sanitized("\\\\server\\share\\tool.exe")
            && target_sanitized("C:\\工具\\程序.exe"),
        "",
    );
    // 2) 控制字符拒绝：TAB/CR/LF/ESC/DEL 注入全拒（外部输入全清洗红线）。
    cs.add(
        "target_control_chars_refused",
        !target_sanitized("C:\\a\tb.exe")
            && !target_sanitized("C:\\a\rb.exe")
            && !target_sanitized("C:\\a\nb.exe")
            && !target_sanitized("\u{1B}[31m注入")
            && !target_sanitized("C:\\a\u{7F}b.exe"),
        "",
    );
    // 3) 空串放行（空目标由断链三分支处理——清洗面不越权判死）。
    cs.add("target_empty_not_sanitize_fault", target_sanitized(""), "");
    cs
}
