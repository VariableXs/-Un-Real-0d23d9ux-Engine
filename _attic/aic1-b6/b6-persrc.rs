
// ---------------------------------------------------------------------------
// F014 · 深化批次六：PNG IHDR 尺寸读出（256px PNG 图标验证的像素数据源）
//
// 主册依据（G-A-14【设计细节】）：「256px PNG 压缩格式图标支持」——PNG 图标
// 的尺寸验证：IHDR 块（首块）宽高读出（大端 u32 ×2，偏移 16..24），配既有
// validate_png 签名/CRC 面。
// ---------------------------------------------------------------------------

/// PNG IHDR 宽高读出（`data` 为完整 PNG 文件：签名 8B + IHDR 长度 4B +
/// "IHDR" 4B + 宽 4B + 高 4B）。结构不符 → None。
pub fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if data.len() < 24 || data[..8] != SIG {
        return None;
    }
    if &data[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h))
}

/// F014 深化批次六自检。
pub fn run_persrc_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F014-persrc-deep5");
    // 1) 256px PNG 图标尺寸读出（256×256）。
    let mut png = alloc::vec![0u8; 32];
    png[..8].copy_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    png[12..16].copy_from_slice(b"IHDR");
    png[16..20].copy_from_slice(&256u32.to_be_bytes());
    png[20..24].copy_from_slice(&256u32.to_be_bytes());
    cs.add(
        "png_dimensions_256_icon",
        png_dimensions(&png) == Some((256, 256)),
        "",
    );
    // 2) 多尺寸梯场景：48×48 与 256×256 区分（尺寸梯判据的数据源面）。
    let mut small = png.clone();
    small[16..20].copy_from_slice(&48u32.to_be_bytes());
    small[20..24].copy_from_slice(&48u32.to_be_bytes());
    cs.add(
        "png_dimensions_multi_size",
        png_dimensions(&small) == Some((48, 48)),
        "",
    );
    // 3) 结构不符如实 None：坏签名 / 非 IHDR 首块 / 零宽。
    let mut bad_sig = png.clone();
    bad_sig[0] = 0x88;
    let mut bad_chunk = png.clone();
    bad_chunk[12..16].copy_from_slice(b"JHDR");
    let mut zero_w = png.clone();
    zero_w[16..20].copy_from_slice(&0u32.to_be_bytes());
    cs.add(
        "png_dimensions_malformed_rejected",
        png_dimensions(&bad_sig).is_none()
            && png_dimensions(&bad_chunk).is_none()
            && png_dimensions(&zero_w).is_none(),
        "",
    );
    cs
}
