
// ---------------------------------------------------------------------------
// F017 · 深化批次九：BMP 4bpp 解码（每字节两像素——高半位在前；调色板
// 复用批次八 8bpp 面；行尾半字节填充后 4 字节对齐）。
// ---------------------------------------------------------------------------

/// 4bpp 一行读出（展开成 ARGB 写入 out；row 为视觉行序——bottom-up 翻转
/// 与既有面同语义）。数据布局：INFOHEADER(40) + 调色板(pal_count*4) + 像素。
pub fn pal4_read_row(
    data: &[u8],
    w: i32,
    h: i32,
    top_down: bool,
    pal_count: usize,
    row: u32,
    out: &mut [u32],
) -> Option<usize> {
    if w <= 0 || h <= 0 || pal_count == 0 || pal_count > 16 || (out.len() as i32) < w {
        return None;
    }
    let pal_off = 40usize;
    let px_off = pal_off + pal_count * 4;
    let mut palette = [0u32; 16];
    for i in 0..pal_count {
        let o = pal_off + i * 4;
        if o + 4 > data.len() {
            return None;
        }
        let b = data[o] as u32;
        let g = data[o + 1] as u32;
        let r = data[o + 2] as u32;
        palette[i] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
    }
    // 4bpp 行宽 = ceil(w/2) 字节，对齐 4。
    let stride = (((w + 1) / 2) as u32).div_ceil(4) * 4;
    let src_row = if top_down { row } else { (h as u32 - 1).saturating_sub(row) };
    let row_off = px_off + (src_row * stride) as usize;
    for col in 0..w as usize {
        let byte_off = row_off + col / 2;
        if byte_off >= data.len() {
            return None;
        }
        let byte = data[byte_off];
        let idx = if col % 2 == 0 { (byte >> 4) as usize } else { (byte & 0xF) as usize };
        if idx >= pal_count {
            return None; // 调色板外下标如实拒
        }
        out[col] = palette[idx];
    }
    Some(w as usize)
}

/// F017 深化批次九自检。
fn run_clipfmt_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F017-clipfmt-deep8");
    // 4x1 4bpp：调色板 4 项，像素字节 0x0123 → 高半位 0、低半位 1、字节 2 高 2……
    let mut d: alloc::vec::Vec<u8> = alloc::vec![0u8; 40 + 4 * 4 + 4];
    let pal = [(0u8, 0u8, 255u8), (0, 255, 0), (255, 0, 0), (255, 255, 255)];
    for (i, (b, g, r)) in pal.iter().enumerate() {
        let o = 40 + i * 4;
        d[o] = *b;
        d[o + 1] = *g;
        d[o + 2] = *r;
    }
    d[40 + 16] = 0x01; // 像素 0=红(0)，像素 1=绿(1)
    d[40 + 17] = 0x23; // 像素 2=蓝(2)，像素 3=白(3)
    let mut out = [0u32; 4];
    // 1) 高半位在前：0x01 → [红, 绿]，0x23 → [蓝, 白]。
    let n = pal4_read_row(&d, 4, 1, true, 4, 0, &mut out);
    cs.add(
        "pal4_high_nibble_first",
        n == Some(4)
            && out[0] == 0xFFFF_0000
            && out[1] == 0xFF00_FF00
            && out[2] == 0xFF00_00FF
            && out[3] == 0xFFFF_FFFF,
        "",
    );
    // 2) 调色板外下标（pal_count=2 时下标 2）如实拒。
    let bad = pal4_read_row(&d, 4, 1, true, 2, 0, &mut out);
    cs.add(
        "pal4_index_out_of_palette",
        bad.is_none(),
        "",
    );
    // 3) 行宽 4 字节对齐：6px 行 stride = 3→4 字节（像素跨字节不越读）。
    let mut d6 = alloc::vec![0u8; 40 + 4 * 4 + 4];
    for (i, (b, g, r)) in pal.iter().enumerate() {
        let o = 40 + i * 4;
        d6[o] = *b;
        d6[o + 1] = *g;
        d6[o + 2] = *r;
    }
    d6[40 + 16] = 0x01;
    d6[40 + 17] = 0x23;
    d6[40 + 18] = 0x01; // 像素 4=红(0)、像素 5=绿(1)——第三字节跨出 3 字节有效域
    let mut out6 = [0u32; 6];
    let n6 = pal4_read_row(&d6, 6, 1, true, 4, 0, &mut out6);
    cs.add(
        "pal4_six_px_stride_ok",
        n6 == Some(6) && out6[4] == 0xFFFF_0000 && out6[5] == 0xFF00_FF00,
        "",
    );
    cs
}
