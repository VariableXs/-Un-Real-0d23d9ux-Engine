
// ---------------------------------------------------------------------------
// F001 · 深化批次八：文件关联命令模板展开面（ProgID Shell\Open\Command 的
// %1 / "%1" 语义——关联查询的「打开命令怎么拼」真逻辑）。
//
// 主册依据（G-A-01【设计细节】关联注册面）：双击按关联键查到命令模板后，
// 以目标文件代入执行；%1 为文件占位，带引号的 "%1" 代入时保留引号（含空格
// 路径安全）。
// ---------------------------------------------------------------------------

/// 模板展开错误（如实分类，不静默拼出坏命令行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssocExpandError {
    /// 模板没有 %1 占位——文件参数无处安放，拒绝展开。
    NoPlaceholder,
    /// %1 后还有字符（如 %1x）——非规范模板，拒绝展开不猜意图。
    TrailingAfterPlaceholder,
    /// 空模板。
    EmptyTemplate,
}

/// 展开关联命令模板：`app "%1"` + `a b.exe` → `app "a b.exe"`；
/// `app %1` + `a b.exe` → `app a b.exe`（裸占位不代引号——模板责任）。
/// 只认首个 %1；%10 之类不是占位（%1 后必须紧跟引号/空白/串尾才算占位边界）。
pub fn assoc_expand(template: &str, file: &str, out: &mut alloc::vec::Vec<u8>) -> Result<(), AssocExpandError> {
    if template.is_empty() {
        return Err(AssocExpandError::EmptyTemplate);
    }
    let b = template.as_bytes();
    let pos = match (0..b.len().saturating_sub(1)).find(|&i| b[i] == b'%' && b[i + 1] == b'1') {
        Some(i) => {
            let after = b.get(i + 2).copied();
            match after {
                None | Some(b'"') | Some(b' ') => i,
                _ => return Err(AssocExpandError::TrailingAfterPlaceholder),
            }
        }
        None => return Err(AssocExpandError::NoPlaceholder),
    };
    out.clear();
    out.extend_from_slice(&b[..pos]);
    out.extend_from_slice(file.as_bytes());
    out.extend_from_slice(&b[pos + 2..]);
    Ok(())
}

/// F001 深化批次八自检。
fn run_dblrun_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F001-dblrun-deep7");
    // 1) 带引号占位：空格路径安全代入。
    let mut out = alloc::vec::Vec::new();
    let r1 = assoc_expand(r#"app.exe "%1""#, "my docs\\a b.exe", &mut out);
    cs.add(
        "assoc_expand_quoted",
        r1 == Ok(())
            && out == r#"app.exe "my docs\a b.exe""#.as_bytes(),
        "",
    );
    // 2) 裸占位：原样代入；%1x 非占位边界如实拒。
    let r2 = assoc_expand("copy %1 bak", "f.txt", &mut out);
    let r3 = assoc_expand("app %1x", "f.txt", &mut out);
    cs.add(
        "assoc_expand_bare_and_boundary",
        r2 == Ok(()) && out == b"copy f.txt bak".to_vec()
            && r3 == Err(AssocExpandError::TrailingAfterPlaceholder),
        "",
    );
    // 3) 无占位 / 空模板：显式错误（异常零静默——坏模板不拼出丢参数的命令）。
    let r4 = assoc_expand("app.exe --stdin", "f.txt", &mut out);
    let r5 = assoc_expand("", "f.txt", &mut out);
    cs.add(
        "assoc_expand_rejects_bad_templates",
        r4 == Err(AssocExpandError::NoPlaceholder)
            && r5 == Err(AssocExpandError::EmptyTemplate),
        "",
    );
    cs
}
