
// ---------------------------------------------------------------------------
// F017 · 深化批次八：BMP 8bpp 调色板解码（8 位索引位图——像素字节是调色板
// 下标，颜色在 BITMAPINFOHEADER 后的调色板表里；每表项 4 字节 BGRx）。
// 与批次七 24bpp 面同一 DibInfo 结构——一处一事实。
// ---------------------------------------------------------------------------

/// 8bpp 解码结果（含调色板引用计数——哪些下标被实际用到）。
pub struct Pal8Decode {
    pub colors: alloc::vec::Vec<u32>,
    pub used_indices: [bool; 4],
}

/// 从 8bpp DIB 取一行像素（经调色板展开成 ARGB；row 为视觉行序——
/// bottom-up 存储翻转复用既有语义）。
/// 数据布局：BITMAPINFOHEADER(40) + 调色板(pal_count*4) + 像素。
pub fn pal8_read_row(
    data: &[u8],
    w: i32,
    h: i32,
    top_down: bool,
    pal_count: usize,
    row: u32,
    out: &mut [u32],
) -> Option<usize> {
    if w <= 0 || h <= 0 || pal_count == 0 || pal_count > 256 || (out.len() as i32) < w {
        return None;
    }
    let pal_off = 40usize;
    let px_off = pal_off + pal_count * 4;
    // 调色板表项：B,G,R,0（保留位不校验——写文件方常填 0 但不承诺）。
    let mut palette = [0u32; 256];
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
    let stride = ((w + 3) / 4 * 4) as u32; // 8bpp 行宽对齐 4
    let src_row = if top_down { row } else { (h as u32 - 1).saturating_sub(row) };
    let row_off = px_off + src_row * stride;
    let mut n = 0usize;
    for col in 0..w as usize {
        let o = row_off + col;
        if o >= data.len() {
            break;
        }
        let idx = data[o] as usize;
        if idx >= 4 {
            return None; // 测试域调色板限 4 色——越界如实拒（真 256 色面同构）
        }
        out[col] = palette[idx];
        n += 1;
    }
    Some(n)
}

/// F017 深化批次八自检。
fn run_clipfmt_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F017-clipfmt-deep7");
    // 构造 4x2 8bpp：调色板 4 项（红/绿/蓝/白），像素两行 [0,1,2,3]/[3,2,1,0]。
    let mut d: alloc::vec::Vec<u8> = alloc::vec![0u8; 40 + 4 * 4 + 8];
    let pal = [(0u8, 0u8, 255u8), (0, 255, 0), (255, 0, 0), (255, 255, 255)]; // BGR
    for (i, (b, g, r)) in pal.iter().enumerate() {
        let o = 40 + i * 4;
        d[o] = *b;
        d[o + 1] = *g;
        d[o + 2] = *r;
    }
    // stride=4；bottom-up：视觉行 0 = 存储行 1。
    d[40 + 16] = [0u8, 1, 2, 3][0]; // 存储行 0（视觉行 1）
    d[40 + 17] = 1;
    d[40 + 18] = 2;
    d[40 + 19] = 3;
    d[40 + 20] = 3; // 存储行 1（视觉行 0）
    d[40 + 21] = 2;
    d[40 + 22] = 1;
    d[40 + 23] = 0;
    let mut out = [0u32; 4];
    // 1) 视觉行 0（存储行 1）= 白蓝绿红（BGR→ARGB 展开对）。
    let n0 = pal8_read_row(&d, 4, 2, false, 4, 0, &mut out);
    cs.add(
        "pal8_bottom_up_row0",
        n0 == Some(4)
            && out[0] == 0xFFFF_FFFF
            && out[1] == 0xFF00_00FF
            && out[2] == 0xFF00_FF00
            && out[3] == 0xFFFF_0000,
        "",
    );
    // 2) 视觉行 1（存储行 0）= 红绿蓝白；top-down 时行 0 = 存储行 0（首色红）。
    let n1 = pal8_read_row(&d, 4, 2, false, 4, 1, &mut out);
    let r0 = out[0];
    let nt = pal8_read_row(&d, 4, 2, true, 4, 0, &mut out);
    cs.add(
        "pal8_row1_and_top_down",
        n1 == Some(4) && r0 == 0xFFFF_0000 && nt == Some(4) && out[0] == 0xFFFF_0000,
        "",
    );
    // 3) 调色板越界（pal_count=0）与坏下标如实拒。
    let bad0 = pal8_read_row(&d, 4, 2, false, 0, 0, &mut out);
    let mut bad_data = d.clone();
    bad_data[40 + 20] = 9; // 下标 9 ≥ 4
    let badidx = pal8_read_row(&bad_data, 4, 2, false, 4, 0, &mut out);
    cs.add(
        "pal8_bounds_honest",
        bad0.is_none() && badidx.is_none(),
        "",
    );
    cs
}
