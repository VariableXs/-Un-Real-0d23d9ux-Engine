
// ---------------------------------------------------------------------------
// F002 · 深化批次七：DOS stub 惯例锚（"This program cannot be run in DOS
// mode"——PE 文件惯例的检测面）
//
// 主册依据（G-A-02【开源复用】）：「PE 格式参考 Microsoft PE Spec」——DOS stub
// 是 PE 惯例（非强制）：MZ 头 + 0x40 起的提示串。检测面用于对抗样本画像
// （stub 缺失/变形是手工构造样本的常见特征——画像不是判死）。
// ---------------------------------------------------------------------------

/// DOS stub 锚串（Microsoft 惯例文案——大小写敏感精确匹配）。
pub const DOS_STUB_ANCHOR: &[u8] = b"This program cannot be run in DOS mode";

/// 检测 DOS stub 锚串是否在 MZ 头之后（0x40..0x200 窗口内——惯例位域）。
/// 非 MZ 文件如实 false（不判死——stub 非强制）。
pub fn has_dos_stub(data: &[u8]) -> bool {
    if data.len() < 0x40 + DOS_STUB_ANCHOR.len() || data[0..2] != *b"MZ" {
        return false;
    }
    let window_end = 0x200.min(data.len() - DOS_STUB_ANCHOR.len() + 1);
    if window_end <= 0x40 {
        return false;
    }
    (0x40..window_end).any(|i| &data[i..i + DOS_STUB_ANCHOR.len()] == DOS_STUB_ANCHOR)
}

/// F002 深化批次七自检。
pub fn run_peblend_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F002-peblend-deep6");
    // 1) 惯例 PE（MZ + 0x4E 起 stub）→ 检出。
    let mut pe = alloc::vec![0u8; 0x200];
    pe[0..2].copy_from_slice(b"MZ");
    pe[0x4E..0x4E + DOS_STUB_ANCHOR.len()].copy_from_slice(DOS_STUB_ANCHOR);
    cs.add("dos_stub_detected", has_dos_stub(&pe), "");
    // 2) 非 MZ / stub 缺失 / 锚串变形（大小写改动）→ 如实 false（画像面，
    //    不判死——stub 非强制的诚实边界）。
    let mut no_stub = alloc::vec![0u8; 0x200];
    no_stub[0..2].copy_from_slice(b"MZ");
    let mut deformed = pe.clone();
    deformed[0x4E] = b't'; // 大小写变形
    let mut not_mz = pe.clone();
    not_mz[0] = b'X';
    cs.add(
        "dos_stub_absent_honest",
        !has_dos_stub(&no_stub) && !has_dos_stub(&deformed) && !has_dos_stub(&not_mz),
        "",
    );
    // 3) 锚串钉值（Microsoft 惯例文案逐字节——一处一事实）。
    cs.add(
        "dos_stub_anchor_pinned",
        DOS_STUB_ANCHOR.len() == 38,
        "",
    );
    cs
}
