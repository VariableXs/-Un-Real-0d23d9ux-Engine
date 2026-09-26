
// ---------------------------------------------------------------------------
// F020 · 深化批次四：dump 压缩存储（自研 RLE——栈回溯长零段的主压缩面）
//
// 主册依据（G-A-20【设计细节】）：「dump 压缩存储（zstd 级评估）」。**诚实
/// 边界**：本核是 RLE（对栈回溯的长零段有效——64 帧 × 8 字节中大量 0 高位是
/// 典型形态）；zstd 级压缩比随闸门评估，差异如实登记（RLE 不是 zstd，格式
/// 自定轻量格式惯例的延伸）。格式：魔数 VXRL + u32 原长 + (u8 计数, u8 字节)
/// 对序列（计数 1..=255）。
// ---------------------------------------------------------------------------

/// RLE 魔数（VXRL——Varix RLE）。
pub const RLE_MAGIC: [u8; 4] = *b"VXRL";
/// 头尺寸（魔数 4 + 原长 u32）。
pub const RLE_HDR_SIZE: usize = 8;

/// 压缩。缓冲不足 → 返回 0（调用方走未压缩落盘路径——不冒充压缩成功）。
pub fn rle_compress(data: &[u8], out: &mut [u8]) -> usize {
    if out.len() < RLE_HDR_SIZE {
        return 0;
    }
    out[..4].copy_from_slice(&RLE_MAGIC);
    out[4..8].copy_from_slice(&(data.len() as u32).to_le_bytes());
    let mut n = RLE_HDR_SIZE;
    let mut i = 0usize;
    while i < data.len() {
        let b = data[i];
        let mut run = 1usize;
        while i + run < data.len() && data[i + run] == b && run < 255 {
            run += 1;
        }
        if n + 2 > out.len() {
            return 0; // 中途容量不足 → 整体放弃（调用方回落未压缩）
        }
        out[n] = run as u8;
        out[n + 1] = b;
        n += 2;
        i += run;
    }
    n
}

/// 解压。魔数/长度/对序列任一不符 → 如实错误（坏块不静默）。
pub fn rle_decompress(data: &[u8], out: &mut [u8]) -> Result<usize, &'static str> {
    if data.len() < RLE_HDR_SIZE {
        return Err("rle: truncated");
    }
    if data[..4] != RLE_MAGIC {
        return Err("rle: bad magic");
    }
    let orig = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    if out.len() < orig {
        return Err("rle: output too small");
    }
    let mut n = 0usize;
    let mut i = RLE_HDR_SIZE;
    while i + 2 <= data.len() {
        let run = data[i] as usize;
        let b = data[i + 1];
        if run == 0 {
            return Err("rle: zero run");
        }
        if n + run > orig {
            return Err("rle: length mismatch");
        }
        out[n..n + run].fill(b);
        n += run;
        i += 2;
    }
    if i != data.len() || n != orig {
        return Err("rle: length mismatch");
    }
    Ok(n)
}

/// F020 深化批次四自检。
pub fn run_excface_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F020-excface-deep3");
    // 1) dump 形态压缩：64 帧栈（每帧 8 字节、高位 5 字节全 0）→ 显著缩小；
    //    round-trip 逐字节一致。
    let mut dump = alloc::vec![0u8; 64 * 8];
    for f in 0..64usize {
        let base = f * 8 + 5;
        dump[base] = (f + 1) as u8; // 低 3 字节是地址，高 5 字节全 0（典型形态）
        dump[base + 1] = 0x80;
        dump[base + 2] = 0xFF;
    }
    let mut comp = alloc::vec![0u8; 4096];
    let cn = rle_compress(&dump, &mut comp);
    let mut back = alloc::vec![0u8; 64 * 8];
    let dn = rle_decompress(&comp[..cn], &mut back);
    cs.add(
        "rle_dump_zero_runs",
        cn > 0 && cn < dump.len() / 4 && dn == Ok(dump.len()) && back == dump,
        "",
    );
    // 2) 不可压数据：最坏 2 倍展开仍 round-trip 正确（不丢不坏——压缩是优化
    //    不是有损）。
    let noisy: alloc::vec::Vec<u8> = (0..64u32).map(|i| (i * 37 + 11) as u8).collect();
    let mut comp2 = alloc::vec![0u8; 4096];
    let cn2 = rle_compress(&noisy, &mut comp2);
    let mut back2 = alloc::vec![0u8; 64];
    let dn2 = rle_decompress(&comp2[..cn2], &mut back2);
    cs.add(
        "rle_incompressible_roundtrip",
        cn2 == RLE_HDR_SIZE + 128 && dn2 == Ok(64) && back2 == noisy,
        "",
    );
    // 3) 坏块三关：魔数错/截断/长度不符 → 如实错误（不静默）。
    let mut bad = comp2.clone();
    bad[0] = b'X';
    let mut out3 = alloc::vec![0u8; 64];
    cs.add(
        "rle_corruption_rejected",
        matches!(rle_decompress(&bad, &mut out3), Err("rle: bad magic"))
            && matches!(rle_decompress(&comp2[..4], &mut out3), Err("rle: truncated"))
            && matches!(rle_decompress(&comp2[..cn2 - 2], &mut out3), Err("rle: length mismatch")),
        "",
    );
    cs
}
