
// ---------------------------------------------------------------------------
// F011 · 深化批次八：环境块渲染面（进程环境块真纪律——按名大小写不敏感
// 排序 + 大小写重复键拒绝 + 双 NUL 终止；SetEnvironmentVariable 的块级
// 不变量）。
// ---------------------------------------------------------------------------

/// 环境块渲染错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnvBlockError {
    /// 名字重复（大小写不敏感判定——Windows 环境变量名不区分大小写）。
    DuplicateName,
    /// 名字含 `=`（环境名本身不许带等号——值为空也算合法条目，名不行）。
    NameHasEquals,
}

/// 渲染环境块：NAME=VALUE\0 ... \0（双 NUL 终止），按名 case-insensitive 排序。
pub fn render_env_block(
    pairs: &[(&str, &str)],
    out: &mut alloc::vec::Vec<u8>,
) -> Result<usize, EnvBlockError> {
    for (n, _) in pairs {
        if n.as_bytes().contains(&b'=') {
            return Err(EnvBlockError::NameHasEquals);
        }
    }
    let mut idx: alloc::vec::Vec<usize> = (0..pairs.len()).collect();
    // 稳定排序按名（大小写不敏感）——插入序在同名时保留（虽然同名会被拒）。
    idx.sort_by(|&a, &b| {
        pairs[a]
            .0
            .to_ascii_lowercase()
            .cmp(&pairs[b].0.to_ascii_lowercase())
    });
    for w in idx.windows(2) {
        if pairs[w[0]].0.eq_ignore_ascii_case(pairs[w[1]].0) {
            return Err(EnvBlockError::DuplicateName);
        }
    }
    out.clear();
    for i in idx {
        out.extend_from_slice(pairs[i].0.as_bytes());
        out.push(b'=');
        out.extend_from_slice(pairs[i].1.as_bytes());
        out.push(0);
    }
    out.push(0); // 块终止双 NUL 的第二个
    Ok(out.len())
}

/// F011 深化批次八自检。
fn run_envsess_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F011-envsess-deep7");
    let mut out = alloc::vec::Vec::new();
    // 1) 排序不变量：插入乱序，块内有序（大小写不敏感）+ 双 NUL 终止。
    let n = render_env_block(
        &[("WindIR", "C:\\Windows"), ("Path", "C:\\bin"), ("APPDATA", "C:\\Users\\v\\AppData")],
        &mut out,
    );
    let tail = &out[out.len().saturating_sub(2)..];
    cs.add(
        "env_block_sorted_and_terminated",
        n.is_ok()
            && tail == [0u8, 0]
            && out.starts_with(b"APPDATA=C:\\Users\\v\\AppData\0")
            && core::str::from_utf8(&out).unwrap_or("").contains("Path=C:\\bin\0WindIR=C:\\Windows\0"),
        "",
    );
    // 2) 大小写重复键拒绝（path vs PATH）——同键不同写法是数据事故。
    let dup = render_env_block(&[("Path", "a"), ("PATH", "b")], &mut out);
    cs.add(
        "env_block_rejects_case_duplicate",
        dup == Err(EnvBlockError::DuplicateName),
        "",
    );
    // 3) 名含 `=` 拒绝；空值条目合法（NAME= 后直接 NUL）。
    let eq = render_env_block(&[("A=B", "v")], &mut out);
    let empty_val = render_env_block(&[("EMPTY", "")], &mut out);
    cs.add(
        "env_block_name_rules",
        eq == Err(EnvBlockError::NameHasEquals)
            && matches!(empty_val, Ok(_))
            && out.starts_with(b"EMPTY=\0"),
        "",
    );
    cs
}
