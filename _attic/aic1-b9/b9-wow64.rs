
// ---------------------------------------------------------------------------
// F004 · 深化批次九：注册表 32 位视图映射（WOW64 注册表重定向——
// HKLM\Software\X → HKLM\Software\Wow6432Node\X；**Software\Classes 共享
// 例外**（真规则：CLSID 等主键不进 Wow6432Node）；HKCU\Software 同映射）。
// ---------------------------------------------------------------------------

/// 注册表重定向判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegRedirect {
    /// 应映射进 Wow6432Node。
    ToWow6432Node,
    /// 共享主键（Classes）——32/64 位同视，不映射。
    Shared,
    /// Software 之外——不在重定向域。
    Outside,
}

/// 判定：路径形如 `hklm\software\...` 或 `hkcu\software\...` 时映射；
/// `software\classes` 例外共享；大小写不敏感。
pub fn reg_redirect_verdict(path: &str) -> RegRedirect {
    let p: alloc::vec::Vec<u8> = path
        .as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect();
    let s = core::str::from_utf8(&p).unwrap_or("");
    let sw = "software";
    let after_root = ["hklm\\", "hkcu\\"].iter().find_map(|root| s.strip_prefix(*root));
    let Some(rest) = after_root else {
        return RegRedirect::Outside;
    };
    let Some(after) = rest.strip_prefix(sw) else {
        return RegRedirect::Outside;
    };
    if after.strip_prefix("\\classes").map_or(false, |tail| tail.is_empty() || tail.starts_with('\\')) {
        RegRedirect::Shared
    } else {
        // 空尾（Software 键本身）与 `\X` 子键都映射；`softwarex` 陷阱已被
        // 上面的 strip_prefix 吃掉（尾巴不以 `\` 起）。
        RegRedirect::ToWow6432Node
    }
}

/// 映射后的路径（仅 ToWow6432Node 有输出——其余 None 不编造）。
pub fn reg_redirected_path(path: &str, out: &mut alloc::vec::Vec<u8>) -> bool {
    if reg_redirect_verdict(path) != RegRedirect::ToWow6432Node {
        return false;
    }
    out.clear();
    let lower = path.to_ascii_lowercase();
    let pos = lower.find("software").unwrap_or(0);
    out.extend_from_slice(lower[..pos + "software".len()].as_bytes());
    out.extend_from_slice(b"\\wow6432node");
    out.extend_from_slice(lower[pos + "software".len()..].as_bytes());
    true
}

/// F004 深化批次九自检。
fn run_wow64_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64-deep8");
    let mut out = alloc::vec::Vec::new();
    // 1) HKLM\Software\X → Wow6432Node\X。
    let r = reg_redirected_path("HKLM\\Software\\MyVendor\\MyApp", &mut out);
    cs.add(
        "reg_map_software_to_wow6432",
        r && out == b"hklm\\software\\wow6432node\\myvendor\\myapp".to_vec(),
        "",
    );
    // 2) Software\Classes 共享例外（CLSID 面不映射——真规则）。
    cs.add(
        "reg_classes_shared_exception",
        reg_redirect_verdict("HKLM\\Software\\Classes\\CLSID\\{GUID}") == RegRedirect::Shared
            && reg_redirect_verdict("HKCU\\Software\\Classes\\.txt") == RegRedirect::Shared,
        "",
    );
    // 3) 域外（HKLM\System / softwarex 前缀陷阱）不映射。
    cs.add(
        "reg_outside_domain",
        reg_redirect_verdict("HKLM\\System\\CurrentControlSet") == RegRedirect::Outside
            && reg_redirect_verdict("HKLM\\SoftwareX\\A") == RegRedirect::Outside,
        "",
    );
    cs
}
