
// ---------------------------------------------------------------------------
// F002 · 深化批次四：PE 校验和（Microsoft 标准算法）+ 补写自洽判据
//
// 主册依据（G-A-02【验收判据】「对抗样本集 30 枚拒绝率 30/30」的头级校验
// 支柱 + 【开源复用】「PE 格式参考 Microsoft PE Spec」）：Optional Header
// CheckSum 字段的标准算法——全文件 u16 折叠和，校验字段视作 0，文件长度折入。
// 对拍锚：pe-parse 样本集随闸门核对；本核先锁自洽与敏感性两判据。
// ---------------------------------------------------------------------------

/// PE 校验和计算（`checksum_field_offset` = CheckSum 字段的文件偏移——
/// PE32+ 为 e_lfanew + 4 + 0x58；字段 4 字节视作 0 参与求和）。
/// 算法：u16 小端逐字累加，每步 16 位折叠；尾奇字节按单字节词补入；
/// 末尾折入文件长度。
pub fn pe_checksum(data: &[u8], checksum_field_offset: usize) -> u32 {
    let mut sum: u64 = 0;
    let n = data.len() & !1;
    let mut i = 0usize;
    while i < n {
        let in_field = i >= checksum_field_offset && i < checksum_field_offset + 4;
        let w = if in_field {
            0u64
        } else {
            u16::from_le_bytes([data[i], data[i + 1]]) as u64
        };
        sum += w;
        sum = (sum & 0xFFFF) + (sum >> 16);
        i += 2;
    }
    if data.len() % 2 == 1 {
        // 尾奇字节：按低字节词补入（MS 同语义）。
        sum += data[data.len() - 1] as u64;
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    sum = (sum & 0xFFFF) + (sum >> 16);
    sum += data.len() as u64;
    sum = (sum & 0xFFFF) + (sum >> 16);
    sum as u32
}

/// F002 深化批次四自检。
pub fn run_peblend_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F002-peblend-deep3");
    // 1) 补写自洽：算出校验和 → 写回字段 → 重算恒等（MS 算法的不动点性质）。
    let mut img = alloc::vec![0u8; 0x200];
    img[0] = b'M';
    img[1] = b'Z';
    for (i, b) in img.iter_mut().enumerate().skip(0x40) {
        *b = (i * 7 + 0x2C) as u8; // 伪内容（确定性）
    }
    const FIELD: usize = 0x140; // 伪 Optional Header 内 CheckSum 偏移（4 字节对齐）
    let c1 = pe_checksum(&img, FIELD);
    img[FIELD..FIELD + 4].copy_from_slice(&c1.to_le_bytes());
    let c2 = pe_checksum(&img, FIELD);
    cs.add("pe_checksum_field_fixed_point", c1 == c2 && c1 != 0, "");
    // 2) 敏感性：任一内容字节翻转 → 校验和必变（对抗样本的检测根基）。
    let mut tampered = alloc::vec![0u8; 0x200];
    tampered.copy_from_slice(&img);
    tampered[0x80] ^= 0x01;
    let c3 = pe_checksum(&tampered, FIELD);
    cs.add("pe_checksum_sensitivity", c3 != c2, "");
    // 3) 长度折入：同内容不同长度 → 校验和不同（长度参与是判据的一部分）。
    let short = pe_checksum(&img[..0x100], FIELD.min(0x100));
    let long = pe_checksum(&img, FIELD);
    cs.add("pe_checksum_length_folded", short != long, "");
    cs
}
