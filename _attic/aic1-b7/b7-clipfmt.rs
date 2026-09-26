
// ---------------------------------------------------------------------------
// F017 · 深化批次七：**BMP/DIB 真解码**（BITMAPFILEHEADER + BITMAPINFOHEADER
// + 24bpp 像素行读——图像格式族的第二块真实现）
//
// 主册依据（G-A-17【功能定义】）：「CF_BITMAP/CF_DIB」的像素数据面——剪贴板
// 位图格式承载的是 DIB 数据；本核解析 BMP 文件头（BM/尺寸/offBits）与信息头
// （宽高/平面/位深/压缩），24bpp 行读（BGR→ARGB + 4 字节行对齐 padding +
// 自底向上行序翻转）。诚实边界：本核承诺 BI_RGB 无压缩 24/32bpp——RLE/位场
// 压缩不在面内，如实拒（差异表）。
// ---------------------------------------------------------------------------

/// BMP 文件头尺寸（BITMAPFILEHEADER 14 字节）。
pub const BMP_FILE_HDR: usize = 14;
/// BITMAPINFOHEADER 最小尺寸（40 字节）。
pub const BMP_INFO_HDR: usize = 40;

/// DIB 几何信息（解析产出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DibInfo {
    pub w: i32,
    pub h: i32,
    pub bpp: u16,
    pub data_off: u32,
    /// h 为负 = 自顶向下（Top-Down DIB——Windows 惯例反面）。
    pub top_down: bool,
}

/// 解析 BMP：魔数 BM / 头尺寸 ≥40 / 宽 >0 / 高 ≠0 / 平面 =1 / 位深 24|32 /
/// 压缩 = BI_RGB(0)。任一不符 → None（结构化拒绝——不猜）。
pub fn parse_bmp(data: &[u8]) -> Option<DibInfo> {
    if data.len() < BMP_FILE_HDR + BMP_INFO_HDR {
        return None;
    }
    if data[0..2] != *b"BM" {
        return None;
    }
    let data_off = u32::from_le_bytes([data[10], data[11], data[12], data[13]]);
    let hdr_size = u32::from_le_bytes([data[14], data[15], data[16], data[17]]);
    if hdr_size < BMP_INFO_HDR as u32 {
        return None;
    }
    let w = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
    let h_raw = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
    if w <= 0 || h_raw == 0 {
        return None;
    }
    let planes = u16::from_le_bytes([data[26], data[27]]);
    let bpp = u16::from_le_bytes([data[28], data[29]]);
    let compression = u32::from_le_bytes([data[30], data[31], data[32], data[33]]);
    if planes != 1 || compression != 0 {
        return None;
    }
    if bpp != 24 && bpp != 32 {
        return None;
    }
    Some(DibInfo { w, h: h_raw.abs(), bpp, data_off, top_down: h_raw < 0 })
}

/// 行字节数（4 字节对齐——DIB 行 stride 纪律）。
pub fn dib_row_bytes(info: &DibInfo) -> u32 {
    (info.w as u32 * (info.bpp as u32 / 8) + 3) & !3
}

/// 读一行像素（BGR→ARGB；自底向上行序翻转——h>0 的 DIB 惯例；输出满即止）。
/// 返回写入像素数；越界/数据不足 → None。
pub fn dib_read_row(data: &[u8], info: &DibInfo, row: u32, out: &mut [u32]) -> Option<usize> {
    if row >= info.h as u32 {
        return None;
    }
    let src_row = if info.top_down { row } else { info.h as u32 - 1 - row };
    let base = info.data_off as usize + (src_row * dib_row_bytes(info)) as usize;
    let step = (info.bpp / 8) as usize;
    let mut n = 0usize;
    for x in 0..info.w as usize {
        let off = base + x * step;
        if off + 3 > data.len() {
            return None;
        }
        let b = data[off] as u32;
        let g = data[off + 1] as u32;
        let r = data[off + 2] as u32;
        if n < out.len() {
            out[n] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
            n += 1;
        }
    }
    Some(n)
}

/// F017 深化批次七自检。
pub fn run_clipfmt_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F017-clipfmt-deep6");
    // 1) 构造 3×2 24bpp BMP（行 stride = (9+3)&!3 = 12）：解析 + 行读 +
    //    BGR→ARGB + 自底向上翻转逐像素对。
    let mut bmp = alloc::vec![0u8; 14 + 40 + 12 * 2];
    bmp[0..2].copy_from_slice(b"BM");
    bmp[10..14].copy_from_slice(&(14u32 + 40u32).to_le_bytes()); // data_off
    bmp[14..18].copy_from_slice(&40u32.to_le_bytes()); // info hdr size
    bmp[18..22].copy_from_slice(&3i32.to_le_bytes()); // w
    bmp[22..26].copy_from_slice(&2i32.to_le_bytes()); // h（正 = 自底向上）
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes()); // planes
    bmp[28..30].copy_from_slice(&24u16.to_le_bytes()); // bpp
    let base = 14 + 40;
    // 底行（DIB 行 0）：BGR (0x10,0x20,0x30) (0x40,0x50,0x60) (0x70,0x80,0x90)
    bmp[base..base + 3].copy_from_slice(&[0x10, 0x20, 0x30]);
    bmp[base + 3..base + 6].copy_from_slice(&[0x40, 0x50, 0x60]);
    bmp[base + 6..base + 9].copy_from_slice(&[0x70, 0x80, 0x90]);
    // 顶行（DIB 行 1）：全 (1,2,3)
    for x in 0..3 {
        bmp[base + 12 + x * 3..base + 12 + x * 3 + 3].copy_from_slice(&[1, 2, 3]);
    }
    let info = parse_bmp(&bmp);
    let px_ok = match info {
        Some(inf) => {
            let mut row0 = [0u32; 4];
            let mut row1 = [0u32; 4];
            let n0 = dib_read_row(&bmp, &inf, 0, &mut row0);
            let n1 = dib_read_row(&bmp, &inf, 1, &mut row1);
            // 逻辑行 0 = DIB 底行；BGR → ARGB 翻转。
            n0 == Some(3)
                && n1 == Some(3)
                && row0[0] == 0xFF30_2010
                && row0[1] == 0xFF60_5040
                && row0[2] == 0xFF90_8070
                && row1[0] == 0xFF03_0201
        }
        None => false,
    };
    cs.add("bmp_dib_parse_and_row_read", px_ok, "");
    // 2) 结构化拒绝：坏魔数 / planes≠1 / 压缩≠0 / 16bpp 不承诺 / 头过短。
    let mut bad = bmp.clone();
    bad[26..28].copy_from_slice(&2u16.to_le_bytes());
    let mut comp = bmp.clone();
    comp[30..34].copy_from_slice(&1u32.to_le_bytes()); // BI_RLE8
    let mut bpp16 = bmp.clone();
    bpp16[28..30].copy_from_slice(&16u16.to_le_bytes());
    let short = &bmp[..20];
    cs.add(
        "bmp_malformed_rejected",
        parse_bmp(&bad).is_none()
            && parse_bmp(&comp).is_none()
            && parse_bmp(&bpp16).is_none()
            && parse_bmp(short).is_none(),
        "",
    );
    // 3) Top-Down DIB（h 为负）：行序不翻转（row 0 = 顶行）。
    let mut td = bmp.clone();
    td[22..26].copy_from_slice(&(-2i32).to_le_bytes());
    let td_ok = match parse_bmp(&td) {
        Some(inf) => {
            let mut row0 = [0u32; 4];
            let _ = dib_read_row(&td, &inf, 0, &mut row0);
            row0[0] == 0xFF03_0201 // row 0 直接是顶行
        }
        None => false,
    };
    cs.add("bmp_topdown_row_order", td_ok, "");
    cs
}
