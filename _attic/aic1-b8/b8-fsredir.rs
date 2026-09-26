
// ---------------------------------------------------------------------------
// F010 · 深化批次八：verbatim 长路径前缀（\\?\ 语义——前缀内跳过一切
// 规范化：不折叠大小写、不解析相对段、不做正斜杠归一——字面路径直通）。
//
// Microsoft 文档：\\?\ 关闭路径规范化，长度上限扩展到 32K 字符。
// ---------------------------------------------------------------------------

/// 路径是否 verbatim（\\?\ 或 \\.\ 前缀——设备/字面直通域）。
pub fn is_verbatim(path: &str) -> bool {
    path.as_bytes().starts_with(br#"\\?\"#) || path.as_bytes().starts_with(br#"\\.\"#)
}

/// 规范化动作选择：verbatim → 原样；否则做常规归一（正斜杠转反斜杠 +
/// 小写折叠——既有面语义）。返回写入 out 的字节数。
pub fn normalize_path_semantics(path: &str, out: &mut alloc::vec::Vec<u8>) -> usize {
    out.clear();
    if is_verbatim(path) {
        out.extend_from_slice(path.as_bytes()); // 字面直通——一个字节都不动
    } else {
        out.extend_from_slice(path.to_ascii_lowercase().replace('/', "\\").as_bytes());
    }
    out.len()
}

/// F010 深化批次八自检。
fn run_fsredir_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F010-fsredir-deep7");
    let mut out = alloc::vec::Vec::new();
    // 1) verbatim 内的正斜杠、大写、相对段全部保留原样。
    let raw = r#"\\?\C:\Apps\MyApp\..\Config"#;
    let n = normalize_path_semantics(raw, &mut out);
    cs.add(
        "verbatim_passthrough_bytes",
        n == raw.len() && out == raw.as_bytes(),
        "",
    );
    // 2) 非 verbatim 照常归一（正斜杠 + 小写折叠）。
    let n2 = normalize_path_semantics("D:/Apps/Tool.EXE", &mut out);
    cs.add(
        "non_verbatim_normalized",
        n2 == "d:\\apps\\tool.exe".len() && out == b"d:\\apps\\tool.exe".to_vec(),
        "",
    );
    // 3) 判定边界：\\?\ 与 \\.\ 均为 verbatim；\\?\ 的双反斜杠不可写作单反斜杠。
    let v = is_verbatim(r#"\\.\PhysicalDrive0"#);
    let v2 = is_verbatim(r"\?\C:\x");
    cs.add(
        "verbatim_prefix_boundary",
        v && !v2,
        "",
    );
    cs
}
