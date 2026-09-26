
// ---------------------------------------------------------------------------
// F015 · 深化批次八：BOM 嗅探（字节序标记三签名判定——UTF-8 / UTF-16LE /
// UTF-16BE / UTF-32 双向四态；重叠签名按最长匹配，不误判）。
// ---------------------------------------------------------------------------

/// BOM 判定结果（码页方向 + 前缀长度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BomKind {
    Utf8,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
}

/// 嗅探 BOM（None = 无 BOM——按字节内容另行判定，不猜编码）。
/// 重叠规则：FF FE 00 00 是 UTF-32LE，必须先于 FF FE（UTF-16LE）判——
/// 否则 UTF-32LE 文件被误判成 UTF-16LE 且多吞两字节。
pub fn sniff_bom(data: &[u8]) -> Option<(BomKind, usize)> {
    if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Some((BomKind::Utf8, 3));
    }
    if data.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        return Some((BomKind::Utf32Le, 4));
    }
    if data.starts_with(&[0xFF, 0xFE]) {
        return Some((BomKind::Utf16Le, 2));
    }
    if data.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return Some((BomKind::Utf32Be, 4));
    }
    if data.starts_with(&[0xFE, 0xFF]) {
        return Some((BomKind::Utf16Be, 2));
    }
    None
}

/// F015 深化批次八自检。
fn run_mlangres_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F015-mlangres-deep7");
    // 1) 四签名全判中，前缀长度正确。
    cs.add(
        "bom_four_signatures",
        sniff_bom(&[0xEF, 0xBB, 0xBF, 0x61]) == Some((BomKind::Utf8, 3))
            && sniff_bom(&[0xFF, 0xFE, 0x61, 0x00]) == Some((BomKind::Utf16Le, 2))
            && sniff_bom(&[0xFE, 0xFF, 0x00, 0x61]) == Some((BomKind::Utf16Be, 2))
            && sniff_bom(&[0x00, 0x00, 0xFE, 0xFF]) == Some((BomKind::Utf32Be, 4)),
        "",
    );
    // 2) 重叠签名：FF FE 00 00 → UTF-32LE（不是 UTF-16LE）——最长匹配纪律。
    cs.add(
        "bom_overlap_longest_match",
        sniff_bom(&[0xFF, 0xFE, 0x00, 0x00]) == Some((BomKind::Utf32Le, 4)),
        "",
    );
    // 3) 无 BOM / 近似序列（FF FD 不是 BOM）如实 None。
    cs.add(
        "bom_absent_honest",
        sniff_bom(b"hello").is_none() && sniff_bom(&[0xFF, 0xFD]).is_none(),
        "",
    );
    cs
}
