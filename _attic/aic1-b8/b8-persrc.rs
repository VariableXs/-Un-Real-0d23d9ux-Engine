
// ---------------------------------------------------------------------------
// F014 · 深化批次八：RT_STRING 块模型（字符串表资源真结构——每块 16 条，
// 块 ID = (字符串 ID >> 4) + 1，槽位 = ID & 15；条目 = u16 长度前缀 +
// UTF-16LE 字符，空串 = 长度 0）。
// ---------------------------------------------------------------------------

/// 从块数据取字符串（UTF-16LE 解出 UTF-8 写入 out，返回长度；超界 None）。
pub fn rt_string_get(block: &[u8], slot: u16, out: &mut alloc::vec::Vec<u8>) -> Option<u16> {
    if slot >= 16 {
        return None;
    }
    let mut off = 0usize;
    for _ in 0..slot {
        if off + 2 > block.len() {
            return None;
        }
        let len = u16::from_le_bytes([block[off], block[off + 1]]) as usize;
        off += 2 + len * 2;
    }
    if off + 2 > block.len() {
        return None;
    }
    let len = u16::from_le_bytes([block[off], block[off + 1]]) as usize;
    off += 2;
    if off + len * 2 > block.len() {
        return None;
    }
    out.clear();
    let mut units = alloc::vec::Vec::with_capacity(len);
    for k in 0..len {
        let b0 = block[off + k * 2];
        let b1 = block[off + k * 2 + 1];
        units.push(u16::from_le_bytes([b0, b1]));
    }
    // UTF-16LE → UTF-8（BMP 域——字符串表资源不承载代理对面，如实按单单元解）。
    for u in units {
        if (0xD800..0xE000).contains(&u) {
            out.extend_from_slice("\u{FFFD}".as_bytes());
        } else {
            let c = char::from_u32(u as u32).unwrap_or('\u{FFFD}');
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    Some(len as u16)
}

/// 字符串 ID → (块 ID, 槽位)（MS 资源编译器语义）。
pub fn rt_string_block_of(id: u16) -> (u16, u16) {
    ((id >> 4) + 1, id & 15)
}

/// F014 深化批次八自检。
fn run_persrc_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F014-persrc-deep7");
    // 块构造：槽 0 = "Ab"（len2），槽 1 = ""（len0），槽 2 = 中文「好」（len1）。
    let mut block: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    block.extend_from_slice(&2u16.to_le_bytes());
    block.extend_from_slice(&[b'A', 0, b'b', 0]);
    block.extend_from_slice(&0u16.to_le_bytes());
    block.extend_from_slice(&1u16.to_le_bytes());
    block.extend_from_slice(&0x597Cu16.to_le_bytes()); // 好
    // 1) 槽位逐条读出：内容与长度全对（含空串槽 1 跳过正确）。
    let mut out = alloc::vec::Vec::new();
    let l0 = rt_string_get(&block, 0, &mut out);
    let t0 = out.clone();
    let l1 = rt_string_get(&block, 1, &mut out);
    let l2 = rt_string_get(&block, 2, &mut out);
    cs.add(
        "rt_string_slots_roundtrip",
        l0 == Some(2) && t0 == b"Ab".to_vec()
            && l1 == Some(0)
            && l2 == Some(1) && out == "好".as_bytes(),
        "",
    );
    // 2) 块/槽映射：ID 0 → 块 1 槽 0；ID 33 → 块 3 槽 1；ID 4095 → 块 256 槽 15。
    cs.add(
        "rt_string_block_mapping",
        rt_string_block_of(0) == (1, 0)
            && rt_string_block_of(33) == (3, 1)
            && rt_string_block_of(4095) == (256, 15),
        "",
    );
    // 3) 槽位越界（>=16）与截断块如实 None。
    let over = rt_string_get(&block, 16, &mut out);
    let short = rt_string_get(&block[..3], 2, &mut out);
    cs.add(
        "rt_string_bounds_honest",
        over.is_none() && short.is_none(),
        "",
    );
    cs
}
