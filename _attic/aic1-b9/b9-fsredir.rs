
// ---------------------------------------------------------------------------
// F010 · 深化批次九：路径拼接核（基路径 + 相对段——绝对段覆盖基路径、
// `..` 在拼接层即上跳、根上再跳 = 沙盒逃逸企图如实拒；`.` 段跳过）。
// ---------------------------------------------------------------------------

/// 拼接错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JoinError {
    /// 相对段 `..` 越出根（沙盒域 = 逃逸企图，拒绝）。
    EscapeAttempt,
    /// 空相对段且基路径为空。
    EmptyBoth,
}

/// 拼接：`base + rel`；rel 为绝对路径（`\\` UNC 或 `X:` 盘符）→ 覆盖 base；
/// rel 逐段处理 `.`（跳过）与 `..`（上跳；只剩根段时再跳 = 逃逸拒）。
pub fn join_sandbox_path(base: &str, rel: &str, out: &mut alloc::vec::Vec<u8>) -> Result<(), JoinError> {
    out.clear();
    if rel.is_empty() && base.is_empty() {
        return Err(JoinError::EmptyBoth);
    }
    let rel_b = rel.as_bytes();
    if rel_b.starts_with(b"\\\\") || rel.as_bytes().get(1) == Some(&b':') {
        out.extend_from_slice(rel.as_bytes()); // 绝对覆盖
        return Ok(());
    }
    let mut segs: alloc::vec::Vec<&str> = base
        .split('\\')
        .filter(|s| !s.is_empty() && *s != ".")
        .collect();
    for seg in rel.split('\\') {
        match seg {
            "" | "." => continue,
            ".." => {
                if segs.len() <= 1 {
                    return Err(JoinError::EscapeAttempt); // 根上再跳 = 逃逸
                }
                segs.pop();
            }
            s => segs.push(s),
        }
    }
    for (i, seg) in segs.iter().enumerate() {
        if i == 0 {
            out.extend_from_slice(seg.as_bytes());
            if seg.ends_with(':') {
                out.push(b'\\'); // 盘符根形态 `C:\`
            }
        } else {
            out.push(b'\\');
            out.extend_from_slice(seg.as_bytes());
        }
    }
    Ok(())
}

/// F010 深化批次九自检。
fn run_fsredir_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F010-fsredir-deep8");
    let mut out = alloc::vec::Vec::new();
    // 1) 普通拼接：base + rel。
    let r1 = join_sandbox_path("C:\\Apps\\Sandbox", "OldApp\\config", &mut out);
    cs.add(
        "join_basic",
        r1 == Ok(()) && out == b"C:\\Apps\\Sandbox\\OldApp\\config".to_vec(),
        "",
    );
    // 2) `..` 上跳 + `.` 跳过：OldApp\..\.\x → x。
    let r2 = join_sandbox_path("C:\\Apps\\Sandbox", "OldApp\\..\\.\\x", &mut out);
    cs.add(
        "join_dotdot_up",
        r2 == Ok(()) && out == b"C:\\Apps\\Sandbox\\x".to_vec(),
        "",
    );
    // 3) 越根逃逸拒：C:\Sandbox 上跳两次过根。
    let r3 = join_sandbox_path("C:\\Sandbox", "..\\..\\Windows", &mut out);
    cs.add(
        "join_escape_rejected",
        r3 == Err(JoinError::EscapeAttempt),
        "",
    );
    // 4) 绝对段覆盖基路径（盘符与 UNC 两形态）。
    let r4 = join_sandbox_path("C:\\Apps", "D:\\Other\\f", &mut out);
    let t4 = out.clone();
    let r5 = join_sandbox_path("C:\\Apps", "\\\\srv\\share\\g", &mut out);
    cs.add(
        "join_absolute_overrides",
        r4 == Ok(()) && t4 == b"D:\\Other\\f".to_vec()
            && r5 == Ok(()) && out == b"\\\\srv\\share\\g".to_vec(),
        "",
    );
    cs
}
