
// ---------------------------------------------------------------------------
// F004 · 深化批次八：文件系统重定向映射面（32 位进程访问 System32 的
// WOW64 重定向语义——真映射规则三向判定，非记账模型）。
//
// 语义（Microsoft WOW64 文档）：① %windir%\System32 → 重定向到 SysWOW64；
// ② %windir%\SysWOW64 本身 **不再** 重定向（防递归）；③ 其余路径原样。
// 大小写不敏感（NTFS 语义）。
// ---------------------------------------------------------------------------

/// 重定向判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedirectVerdict {
    /// System32 前缀命中 → 应改指 SysWOW64。
    ToSysWow64,
    /// SysWOW64 前缀命中 → 字面访问（不二次重定向）。
    LiteralSysWow64,
    /// 其他路径 → 字面访问。
    LiteralOther,
}

const SYS32: &[u8] = b"c:\\windows\\system32\\";
const SYSWOW: &[u8] = b"c:\\windows\\syswow64\\";

/// 路径重定向判定（大小写不敏感前缀匹配；`/` 归一为 `\` 后再比）。
pub fn wow64_redirect_verdict(path: &str) -> RedirectVerdict {
    let mut norm: alloc::vec::Vec<u8> = path
        .as_bytes()
        .iter()
        .map(|b| if *b == b'/' { b'\\' } else { b.to_ascii_lowercase() })
        .collect();
    norm.push(0);
    let p = &norm[..norm.len() - 1];
    if p.starts_with(SYSWOW) {
        RedirectVerdict::LiteralSysWow64
    } else if p.starts_with(SYS32) {
        RedirectVerdict::ToSysWow64
    } else {
        RedirectVerdict::LiteralOther
    }
}

/// 重定向后的实际路径（仅 ToSysWow64 有映射——其余 None 不编造）。
pub fn wow64_redirected_path(path: &str, out: &mut alloc::vec::Vec<u8>) -> bool {
    if wow64_redirect_verdict(path) != RedirectVerdict::ToSysWow64 {
        return false;
    }
    out.clear();
    out.extend_from_slice(b"c:\\windows\\syswow64\\");
    let skip = b"c:\\windows\\system32\\".len();
    let rest = &path[skip.min(path.len())..]; // 保留原路径大小写（Windows 语义）
    out.extend_from_slice(rest.as_bytes());
    true
}

/// F004 深化批次八自检。
fn run_wow64_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64-deep7");
    let mut out = alloc::vec::Vec::new();
    // 1) System32 → SysWOW64（正斜杠归一；输出保留原大小写）。
    let r1 = wow64_redirected_path("C:\\Windows\\SYSTEM32\\kernel32.dll", &mut out);
    let t1 = core::str::from_utf8(&out).unwrap_or("").to_string();
    let r2 = wow64_redirected_path("c:/windows/system32/user32.dll", &mut out);
    cs.add(
        "redirect_system32_to_syswow64",
        r1 && r2 && t1 == "c:\\windows\\syswow64\\KERNEL32.dll",
        "",
    );
    // 2) SysWOW64 不二次重定向；非系统目录原样。
    let v = wow64_redirect_verdict("C:\\Windows\\SysWOW64\\notepad.exe");
    let v2 = wow64_redirect_verdict("D:\\Apps\\tool.exe");
    cs.add(
        "syswow64_never_re_redirected",
        v == RedirectVerdict::LiteralSysWow64 && v2 == RedirectVerdict::LiteralOther,
        "",
    );
    // 3) 前缀相似但不同（System32Own）不误命中——前缀匹配必须到分隔符。
    let v3 = wow64_redirect_verdict("c:\\windows\\system32own\\x.dll");
    cs.add(
        "prefix_boundary_exact",
        v3 == RedirectVerdict::LiteralOther,
        "",
    );
    cs
}
