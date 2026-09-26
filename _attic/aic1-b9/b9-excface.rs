
// ---------------------------------------------------------------------------
// F020 · 深化批次九：minidump 头解析（MDMP 魔数 + 版本 + 流数 + 流目录
// 走查——目录项 (StreamType, DataSize, Rva) 三元组逐项读取；未知流类型
// 如实保留类型号不丢弃——「总日志中心不吞事件」的转储域同纪律）。
// ---------------------------------------------------------------------------

/// 流目录项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DumpStreamRef {
    pub stream_type: u32,
    pub data_size: u32,
    pub rva: u32,
}

/// minidump 头解析（头 32 字节：魔数 "MDMP" LE 0x504D444D、版本两 u16、
/// 流数 @8、目录 Rva @12、校验和 @16=0、时间戳 @20、Flags @24）。
pub fn dump_header_parse(data: &[u8]) -> Option<(u32, u32, alloc::vec::Vec<DumpStreamRef>)> {
    if data.len() < 32 {
        return None;
    }
    if &data[0..4] != b"MDMP" {
        return None;
    }
    let stream_count = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let dir_rva = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    // 流数硬顶 128（正常 minidump 十几条流——超限 = 损坏，如实拒）。
    if stream_count == 0 || stream_count > 128 {
        return None;
    }
    if (dir_rva as u64) + (stream_count as u64) * 12 > data.len() as u64 {
        return None;
    }
    let mut streams = alloc::vec::Vec::with_capacity(stream_count as usize);
    for i in 0..stream_count as usize {
        let o = dir_rva as usize + i * 12;
        streams.push(DumpStreamRef {
            stream_type: u32::from_le_bytes([
                data[o], data[o + 1], data[o + 2], data[o + 3],
            ]),
            data_size: u32::from_le_bytes([
                data[o + 4], data[o + 5], data[o + 6], data[o + 7],
            ]),
            rva: u32::from_le_bytes([
                data[o + 8], data[o + 9], data[o + 10], data[o + 11],
            ]),
        });
    }
    Some((stream_count, dir_rva, streams))
}

/// F020 深化批次九自检。
fn run_excface_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F020-excface-deep8");
    // 构造 2 流目录：类型 4（线程表）+ 类型 0xFEEF（私有未知流——保留）。
    let mut d: alloc::vec::Vec<u8> = alloc::vec![0u8; 32 + 2 * 12];
    d[0..4].copy_from_slice(b"MDMP");
    d[8..12].copy_from_slice(&2u32.to_le_bytes());
    d[12..16].copy_from_slice(&32u32.to_le_bytes());
    // dir[0]: type 4, size 0x100, rva 0x1000
    d[32..36].copy_from_slice(&4u32.to_le_bytes());
    d[36..40].copy_from_slice(&0x100u32.to_le_bytes());
    d[40..44].copy_from_slice(&0x1000u32.to_le_bytes());
    // dir[1]: type 0xFEEF, size 0x40, rva 0x2000
    d[44..48].copy_from_slice(&0xFEEFu32.to_le_bytes());
    d[48..52].copy_from_slice(&0x40u32.to_le_bytes());
    d[52..56].copy_from_slice(&0x2000u32.to_le_bytes());
    // 1) 头 + 全流解出；未知流类型保留不丢。
    let parsed = dump_header_parse(&d);
    cs.add(
        "dump_header_and_streams",
        matches!(parsed, Some((2, 32, ref s)) if s.len() == 2
            && s[0].stream_type == 4 && s[0].data_size == 0x100
            && s[1].stream_type == 0xFEEF && s[1].rva == 0x2000),
        "",
    );
    // 2) 坏魔数（"DMIN"）如实拒——不猜。
    let mut bad = d.clone();
    bad[0] = b'D';
    bad[3] = b'N';
    cs.add(
        "dump_bad_magic_none",
        dump_header_parse(&bad).is_none(),
        "",
    );
    // 3) 目录越界（声称 2 流但目录只有 1 条空间）如实拒。
    cs.add(
        "dump_dir_out_of_bounds",
        dump_header_parse(&d[..32 + 12]).is_none(),
        "",
    );
    cs
}
