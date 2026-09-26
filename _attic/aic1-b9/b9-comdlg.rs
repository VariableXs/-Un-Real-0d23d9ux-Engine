
// ---------------------------------------------------------------------------
// F008 · 深化批次九：默认扩展名拼接（GetOpenFileName lpstrDefExt 语义——
// 用户输入无扩展时补默认扩展；已有扩展/显式尾点/隐藏文件边界如实处理）。
// ---------------------------------------------------------------------------

/// 拼接默认扩展名（返回是否发生拼接；out 写入完整文件名）。
/// 边界：① 名含 `.`（任意位置——DOS 语义：最后一个点后是扩展）→ 不拼；
/// ② 名以 `.` 结尾 → 显式空扩展，不拼；③ def_ext 带前导点归一（`.log` 与
/// `log` 同义）；④ 空名不拼（没有可附着的目标）。
pub fn apply_default_ext(name: &str, def_ext: &str, out: &mut alloc::vec::Vec<u8>) -> bool {
    out.clear();
    out.extend_from_slice(name.as_bytes());
    if name.is_empty() || def_ext.is_empty() {
        return false;
    }
    let base = name.as_bytes();
    if base.last() == Some(&b'.') {
        return false; // 显式空扩展——用户说了算
    }
    if base.contains(&b'.') {
        return false; // 已有扩展（DOS 语义：首个点即进入扩展域）
    }
    let ext = def_ext.strip_prefix('.').unwrap_or(def_ext);
    if ext.is_empty() {
        return false;
    }
    out.push(b'.');
    out.extend_from_slice(ext.as_bytes());
    true
}

/// F008 深化批次九自检。
fn run_comdlg_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F008-comdlg-deep8");
    let mut out = alloc::vec::Vec::new();
    // 1) 无扩展 → 补 .log；带点扩展归一（".log" 与 "log" 等价）。
    let a = apply_default_ext("report", "log", &mut out);
    let t1 = out.clone();
    let b = apply_default_ext("report", ".log", &mut out);
    cs.add(
        "default_ext_appended_and_normalized",
        a && b && t1 == b"report.log".to_vec() && out == b"report.log".to_vec(),
        "",
    );
    // 2) 已有扩展（data.csv）与显式尾点（data.）都不拼——用户意图优先。
    let c = apply_default_ext("data.csv", "log", &mut out);
    let d = apply_default_ext("data.", "log", &mut out);
    cs.add(
        "default_ext_respects_existing",
        !c && !d && out == b"data.".to_vec(),
        "",
    );
    // 3) 空名 / 空默认扩展：不拼不炸。
    let e = apply_default_ext("", "log", &mut out);
    let f = apply_default_ext("x", "", &mut out);
    cs.add(
        "default_ext_empty_safe",
        !e && !f,
        "",
    );
    cs
}
