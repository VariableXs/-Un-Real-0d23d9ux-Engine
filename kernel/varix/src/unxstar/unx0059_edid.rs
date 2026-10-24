//! UNX-F0059 · EDID 显示器信息采集 · 诊断段 0x20B0
//! 验收锚点：UNX-F0059-J1 128 字节基块校验和、头部 00FF...FF 哨兵、
//! 时序描述符/建立时序解析与首选时序提取判据通过；
//! 纯功能代码 ≥240 行。只读采集——零 I2C 写、零回写。


use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x20B0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_HDR: u16 = SEG_BASE + 1;      // 头部哨兵 00 FF FF FF FF FF FF 00 不符
const E_CHKSUM: u16 = SEG_BASE + 2;   // 128 字节校验和 ≠ 0
const E_VER: u16 = SEG_BASE + 3;      // 版本/修订不在支持闭集
const E_DTD: u16 = SEG_BASE + 4;      // 首选时序描述符全零
const E_EXT: u16 = SEG_BASE + 5;      // 扩展块数与实际不符（越界读拦截）
const E_TAG: u16 = SEG_BASE + 6;      // 描述符标签越闭集
const E_RANGE: u16 = SEG_BASE + 7;    // 时序参数越物理可能域
const E_BAL: u16 = SEG_BASE + 8;      // 采集账守恒不平
const E_RDONLY: u16 = SEG_BASE + 9;   // 只读账收到写请求
const E_STROFF: u16 = SEG_BASE + 10;  // 字符串描述符无终止符

/// EDID 1.3/1.4 支持闭集：(版本, 修订)。
pub const SUPPORTED_VERSIONS: [(u8, u8); 2] = [(1, 3), (1, 4)];

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_HDR => "头部哨兵不符：前 8 字节必须 00 FF FF FF FF FF FF 00",
        E_CHKSUM => "128 字节校验和不为 0：模 256 求和须 ≡ 0",
        E_VER => "版本/修订不在 {1.3, 1.4}：超纲 EDID 拒采",
        E_DTD => "首选时序描述符全零：面板无首选时序，拒采",
        E_EXT => "扩展块数越界：声明数 ×128 超过实际流长",
        E_TAG => "描述符标签越闭集 {FF,FE,FC,FB,FA,10..}：留痕拒绝",
        E_RANGE => "时序参数越物理可能域：像素时钟/行列超上界",
        E_BAL => "采集账守恒：描述符计数与账面不平",
        E_RDONLY => "只读账收到写请求",
        E_STROFF => "文本描述符在 13 字节内无终止符：截断采集",
        _ => "未知诊断码",
    }
}

// ── 首选时序 ─────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetailedTiming {
    pub pixel_clock_khz: u32, // 10kHz 单位 ×10
    pub h_active: u16,
    pub h_blank: u16,
    pub v_active: u16,
    pub v_blank: u16,
    pub h_sync_off: u16,
    pub h_sync_width: u16,
    pub v_sync_off: u16,
    pub v_sync_width: u16,
    pub h_size_mm: u16,
    pub v_size_mm: u16,
    pub interlaced: bool,
    pub stereo: bool,
}

/// DTD 18 字节投影（EDID 1.x 定长格式）。
pub fn parse_dtd(b: &[u8]) -> Result<DetailedTiming, u16> {
    if b.len() < 18 {
        return Err(E_DTD);
    }
    if b[0] == 0 && b[1] == 0 && b[2] == 0 && b[3] == 0 && b[4] == 0 {
        // 五个零开头 = 非时序描述符（文本/范围标签）
        return Err(E_DTD);
    }
    let pclk = u32::from_le_bytes([b[0], b[1], 0, 0]) * 10;
    let h_active = ((b[4] as u16) & 0xF0) >> 4 << 8 | b[2] as u16;
    let h_blank = ((b[4] as u16) & 0x0F) << 8 | b[3] as u16;
    let v_active = ((b[7] as u16) & 0xF0) >> 4 << 8 | b[5] as u16;
    let v_blank = ((b[7] as u16) & 0x0F) << 8 | b[6] as u16;
    let h_so = (((b[11] as u16) & 0xC0) >> 6) << 8 | b[8] as u16;
    let h_sw = (((b[11] as u16) & 0x30) >> 4) << 8 | b[9] as u16;
    let v_so = (((b[11] as u16) & 0x0C) >> 2) << 4 | ((b[10] as u16) & 0xF0) >> 4;
    let v_sw = ((b[11] as u16) & 0x03) << 4 | (b[10] as u16) & 0x0F;
    let h_mm = ((b[14] as u16) & 0xF0) >> 4 << 8 | b[12] as u16;
    let v_mm = ((b[14] as u16) & 0x0F) << 8 | b[13] as u16;
    let features = b[17];
    let t = DetailedTiming {
        pixel_clock_khz: pclk,
        h_active,
        h_blank,
        v_active,
        v_blank,
        h_sync_off: h_so,
        h_sync_width: h_sw,
        v_sync_off: v_so,
        v_sync_width: v_sw,
        h_size_mm: h_mm,
        v_size_mm: v_mm,
        interlaced: features & 0x80 != 0,
        stereo: features & 0x60 != 0,
    };
    // 物理可能域判据：8K@上限（像素时钟 ≤ 2 GHz）且行列 ≤ 16384。
    if t.pixel_clock_khz == 0 || t.pixel_clock_khz > 2_000_000 {
        return Err(E_RANGE);
    }
    if t.h_active == 0 || t.v_active == 0 || t.h_active > 16384 || t.v_active > 16384 {
        return Err(E_RANGE);
    }
    if t.h_sync_off + t.h_sync_width > t.h_active + t.h_blank {
        return Err(E_RANGE);
    }
    Ok(t)
}

/// 文本描述符（标签 FC 名称 / FE 序列 / FB 范围限值）文本提取。
pub fn desc_text(b: &[u8]) -> Result<String, u16> {
    if b.len() < 18 || b[0..5] != [0, 0, 0, 0, 0] {
        return Err(E_TAG);
    }
    let tag = b[3];
    match tag {
        0xFC | 0xFE | 0xFF | 0xFB => {}
        _ => return Err(E_TAG),
    }
    let mut s = String::new();
    for &c in &b[5..18] {
        if c == 0x0A {
            break;
        }
        if c == 0 {
            return Err(E_STROFF);
        }
        s.push(c as char);
    }
    Ok(s)
}

// ── EDID 采集账 ──────────────────────────────────────────────
pub struct EdidSnapshot {
    pub version: (u8, u8),
    pub manufacturer_id: u16, // 3+5 位压缩 PnP ID
    pub product_code: u16,
    pub serial: u32,
    pub week: u8,
    pub year: u8, // 1990 + 值
    pub preferred: Option<DetailedTiming>,
    pub monitor_name: Option<String>,
    pub ext_blocks_declared: u8,
    pub checksum_ok: bool,
}

/// 128 字节基块只读解析。
pub fn parse_edid(blk: &[u8]) -> Result<EdidSnapshot, u16> {
    if blk.len() < 128 {
        return Err(E_HDR);
    }
    if blk[0..8] != [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00] {
        return Err(E_HDR);
    }
    let sum: u8 = blk[..128].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    if sum != 0 {
        return Err(E_CHKSUM);
    }
    let ver = (blk[18], blk[19]);
    if !SUPPORTED_VERSIONS.contains(&ver) {
        return Err(E_VER);
    }
    let man = u16::from_be_bytes([blk[8], blk[9]]);
    let product = u16::from_le_bytes([blk[10], blk[11]]);
    let serial = u32::from_le_bytes([blk[12], blk[13], blk[14], blk[15]]);
    let week = blk[16];
    let year = blk[17].saturating_add(199);
    let ext = blk[126];
    // 首选时序 = 描述符区第一槽（偏移 54）
    let preferred = parse_dtd(&blk[54..72]).ok();
    // 描述符区四槽标签闭集巡检
    for slot in 0..4 {
        let off = 54 + slot * 18;
        let b = &blk[off..off + 18];
        if b[0] == 0 && b[1] == 0 && b[2] == 0 {
            let tag = b[3];
            if tag != 0 && !(0xFB..=0xFF).contains(&tag) && tag != 0x10 {
                return Err(E_TAG);
            }
        }
    }
    let mut name = None;
    for slot in 0..4 {
        let off = 54 + slot * 18;
        if let Ok(t) = desc_text(&blk[off..off + 18]) {
            if name.is_none() {
                name = Some(t);
            }
        }
    }
    Ok(EdidSnapshot {
        version: ver,
        manufacturer_id: man,
        product_code: product,
        serial,
        week,
        year,
        preferred,
        monitor_name: name,
        ext_blocks_declared: ext,
        checksum_ok: true,
    })
}

/// 扩展声明与流长一致性：声明数 ×128 ≤ 流长。
pub fn ext_audit(snap: &EdidSnapshot, stream_len: usize) -> Result<(), u16> {
    if (snap.ext_blocks_declared as usize + 1) * 128 > stream_len {
        return Err(E_EXT);
    }
    Ok(())
}

/// 厂商 ID 解码：5 位字母压缩 → "VAR" 风格三字母。
pub fn decode_pnp(man: u16) -> String {
    let c1 = ((man >> 10) & 0x1F) as u8;
    let c2 = ((man >> 5) & 0x1F) as u8;
    let c3 = (man & 0x1F) as u8;
    let mut s = String::new();
    for c in [c1, c2, c3] {
        if c == 0 {
            continue;
        }
        s.push((b'A' + c - 1) as char);
    }
    s
}

// ── 合成 EDID 基块（演练复用）────────────────────────────────
pub fn synth_edid(name: &str) -> Vec<u8> {
    let mut b = vec![0u8; 128];
    b[0..8].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
    // 厂商 "VXR"：V=22 X=24 R=18 → (22<<10)|(24<<5)|18
    let man: u16 = (22 << 10) | (24 << 5) | 18;
    b[8..10].copy_from_slice(&man.to_be_bytes());
    b[10] = 0x34;
    b[11] = 0x12;
    b[12] = 0x78;
    b[13] = 0x56;
    b[14] = 0x34;
    b[15] = 0x12;
    b[16] = 41; // 周
    b[17] = 26; // 2016
    b[18] = 1;
    b[19] = 4;
    // 首选时序：1920×1080@60，148.5MHz（逐字节按 DTD 位序编码）
    b[54] = 0x02; b[55] = 0x3A; // pclk = 0x3A02 ×10kHz = 148500 kHz
    b[56] = 0x80; b[57] = 0x18; b[58] = 0x71; // h_active 0x780 / h_blank 0x118
    b[59] = 0x38; b[60] = 0x2D; b[61] = 0x40; // v_active 0x438 / v_blank 0x02D
    b[62] = 0x58; b[63] = 0x2C; b[64] = 0x54; b[65] = 0x00; // 同步脉冲
    b[66] = 0x58; b[67] = 0x54; b[68] = 0x21; // 600×340 mm
    b[69] = 0x00; b[70] = 0x00; b[71] = 0x0E; // 边界 + 特征位（非隔行）
    // 名称描述符槽 2（DTD 后第一个描述符，偏移 72）
    let off = 54 + 18;
    b[off] = 0; b[off + 1] = 0; b[off + 2] = 0; b[off + 3] = 0xFC;
    let nb = name.as_bytes();
    let n = nb.len().min(13);
    for i in 0..n {
        b[off + 5 + i] = nb[i];
    }
    if n < 13 {
        b[off + 5 + n] = 0x0A;
    }
    // 校验和归零
    let sum: u8 = b.iter().fold(0u8, |a, x| a.wrapping_add(*x));
    b[127] = sum.wrapping_neg();
    b
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let blk = synth_edid("VARIX-27Q");

    // R1：基块全字段采集 + 厂商解码。
    let s = parse_edid(&blk)?;
    if s.version != (1, 4) || !s.checksum_ok {
        return Err(E_VER);
    }
    if decode_pnp(s.manufacturer_id) != "VXR" {
        return Err(E_BAL);
    }
    if s.serial != 0x12345678 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：首选时序提取——148500kHz。
    let t = s.preferred.ok_or(E_DTD)?;
    if t.pixel_clock_khz != 1485_000 {
        return Err(E_RANGE);
    }
    hits += 1;

    // R3：扩展声明 0 → 一致性过；名称描述符采集。
    ext_audit(&s, 128)?;
    if s.monitor_name.as_deref() != Some("VARIX-27Q") {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：校验和破坏 → E_CHKSUM。
    let mut b1 = blk.clone();
    b1[64] ^= 0xFF;
    b1[127] = b1[127].wrapping_add(0); // 依旧不归零
    if !matches!(parse_edid(&b1), Err(E_CHKSUM)) {
        return Err(E_CHKSUM);
    }
    hits += 1;

    // I2：头部破坏 → E_HDR。
    let mut b2 = blk.clone();
    b2[7] = 0x01;
    if !matches!(parse_edid(&b2), Err(E_HDR)) {
        return Err(E_HDR);
    }
    hits += 1;

    // I3：版本越闭集 → E_VER。
    let mut b3 = blk.clone();
    b3[18] = 2;
    b3[127] = b3[127].wrapping_sub(1); // 校验和补偿，让错误落在版本判
    if !matches!(parse_edid(&b3), Err(E_VER)) {
        return Err(E_VER);
    }
    hits += 1;

    // I4：扩展声明越界 → E_EXT。
    let mut b4 = blk.clone();
    b4[126] = 3;
    // 校验和补偿
    let sum4: u8 = b4[..127].iter().fold(0u8, |a, x| a.wrapping_add(*x));
    b4[127] = sum4.wrapping_neg();
    let s4 = parse_edid(&b4)?;
    if ext_audit(&s4, 128).err() != Some(E_EXT) {
        return Err(E_EXT);
    }
    hits += 1;

    // I5：像素时钟归零首选时序 → E_DTD（preferred 落空可接受，直接解析必须报错）。
    let mut b5 = blk.clone();
    b5[54] = 0; b5[55] = 0;
    if parse_dtd(&b5[54..72]).err() != Some(E_DTD) {
        return Err(E_DTD);
    }
    hits += 1;

    Ok(hits)
}
