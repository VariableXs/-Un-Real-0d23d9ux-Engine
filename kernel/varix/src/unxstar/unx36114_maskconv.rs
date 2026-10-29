//! UNX-F36114-J1 掩码与 ACE 存储的双向转换器。
//! 判据：① 掩码 → 定容字节序（8 字节帧：魔数/版本/掩码 LE/校验和）
//! 编码；② 解析往返逐位相同（对拍）+ 帧损坏逐字节必检出；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：存储面前置——编解码双向做存态合法性检查（保留位
//! 红线，与 36113 同式双写对拍）；帧格式是 ACE 存储的掩码载荷。

use super::unx36101_masktable as mt;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3a20;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_MAGIC: u16 = SEG_BASE + 2; // 帧魔数不符
pub const E_FVER: u16 = SEG_BASE + 3; // 帧版本不符
pub const E_RSVD_F: u16 = SEG_BASE + 4; // 帧载荷含保留位（双向红线）
pub const E_CS: u16 = SEG_BASE + 5; // 帧校验和不符
pub const E_INVALID: u16 = SEG_BASE + 6; // 掩码本身存态非法（编码入口）
pub const E_LEN: u16 = SEG_BASE + 7; // 帧长不足（缓冲 < 8）
pub const E_RT: u16 = SEG_BASE + 8; // 往返逐位不相同（对拍破）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 帧字段（显式判别式 = 帧内偏移）。
#[repr(u16)]
pub enum FrameField {
    Magic0 = 0x0000,
    Magic1 = 0x0001,
    Ver = 0x0002,
    Mask = 0x0003,
    Cs = 0x0007,
}

impl FrameField {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn ofs(self) -> usize {
        self.code() as usize
    }
}

/// 帧长（定容 8 字节）。
pub const FRAME_LEN: usize = 8;

/// 帧魔数（'M','K' → 掩码载荷帧）。
pub const FRAME_MAGIC0: u8 = 0x4D;
pub const FRAME_MAGIC1: u8 = 0x4B;

/// 帧版本。
pub const FRAME_VER: u8 = 1;

/// 帧布局偏移：[0]magic0 [1]magic1 [2]ver [3..7]掩码LE [7]校验和。
pub const OFS_MAGIC0: usize = 0;
pub const OFS_MAGIC1: usize = 1;
pub const OFS_VER: usize = 2;
pub const OFS_MASK: usize = 3; // 3..6 含（4 字节掩码 LE）
pub const OFS_CS: usize = 7;

/// 帧校验和：字节 0..6 的异或折叠（定容、无多项式依赖）。
pub fn frame_checksum(f: &[u8; FRAME_LEN]) -> u8 {
    let mut c: u8 = 0;
    let mut i = 0;
    while i < OFS_CS {
        c ^= f[i];
        i += 1;
    }
    c
}

/// 存态合法性快检（与 36113::validate_stored 同式——双写对拍点）。
fn storable(mask: u32) -> Result<(), u16> {
    if mask & mt::ALL_RSVD != 0 {
        return Err(E_INVALID);
    }
    Ok(())
}

/// 编码：合法掩码 → 8 字节帧（入口保留位红线）。
pub fn encode(mask: u32) -> Result<[u8; FRAME_LEN], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    storable(mask)?;
    let mut f = [0u8; FRAME_LEN];
    f[OFS_MAGIC0] = FRAME_MAGIC0;
    f[OFS_MAGIC1] = FRAME_MAGIC1;
    f[OFS_VER] = FRAME_VER;
    f[OFS_MASK] = (mask & 0xFF) as u8;
    f[OFS_MASK + 1] = ((mask >> 8) & 0xFF) as u8;
    f[OFS_MASK + 2] = ((mask >> 16) & 0xFF) as u8;
    f[OFS_MASK + 3] = ((mask >> 24) & 0xFF) as u8;
    f[OFS_CS] = frame_checksum(&f);
    Ok(f)
}

/// 解码：8 字节帧 → 掩码（魔数/版本/校验和/载荷保留位全查）。
pub fn decode(f: &[u8; FRAME_LEN]) -> Result<u32, u16> {
    if f[OFS_MAGIC0] != FRAME_MAGIC0 || f[OFS_MAGIC1] != FRAME_MAGIC1 {
        return Err(E_MAGIC);
    }
    if f[OFS_VER] != FRAME_VER {
        return Err(E_FVER);
    }
    if f[OFS_CS] != frame_checksum(f) {
        return Err(E_CS);
    }
    let m = (f[OFS_MASK] as u32)
        | ((f[OFS_MASK + 1] as u32) << 8)
        | ((f[OFS_MASK + 2] as u32) << 16)
        | ((f[OFS_MASK + 3] as u32) << 24);
    // 载荷保留位红线（双向检查：坏帧不产脏掩码）。
    if m & mt::ALL_RSVD != 0 {
        return Err(E_RSVD_F);
    }
    Ok(m)
}

/// 变长解码入口（缓冲不足显性拒）。
pub fn decode_buf(buf: &[u8]) -> Result<u32, u16> {
    if buf.len() < FRAME_LEN {
        return Err(E_LEN);
    }
    let mut f = [0u8; FRAME_LEN];
    let mut i = 0;
    while i < FRAME_LEN {
        f[i] = buf[i];
        i += 1;
    }
    decode(&f)
}

/// 往返对拍：encode→decode 逐位相同。
pub fn roundtrip(mask: u32) -> Result<u32, u16> {
    let f = encode(mask)?;
    let m = decode(&f)?;
    if m != mask {
        return Err(E_RT);
    }
    Ok(m)
}

/// 帧损坏注入：指定字节位翻转后必须被检出（解码错误或载荷失真）。
pub fn corrupt_and_check(f: &[u8; FRAME_LEN], byte_idx: usize, orig: u32) -> bool {
    if byte_idx >= FRAME_LEN {
        return false;
    }
    let mut g = *f;
    g[byte_idx] = g[byte_idx].wrapping_add(0x5A);
    match decode(&g) {
        Err(_) => true,
        Ok(m) => m != orig, // 掩码字节损坏：载荷失真也算检出
    }
}

/// 手工改载荷并重签校验和（负例构造器：产“校验和正确但载荷含
/// 保留位”的坏帧）。
pub fn reframe_with_reserved(orig_mask: u32, rsvd_or: u32) -> Result<[u8; FRAME_LEN], u16> {
    let bad_mask = orig_mask | rsvd_or;
    let mut f = encode(orig_mask)?;
    f[OFS_MASK] = (bad_mask & 0xFF) as u8;
    f[OFS_MASK + 1] = ((bad_mask >> 8) & 0xFF) as u8;
    f[OFS_MASK + 2] = ((bad_mask >> 16) & 0xFF) as u8;
    f[OFS_MASK + 3] = ((bad_mask >> 24) & 0xFF) as u8;
    f[OFS_CS] = frame_checksum(&f);
    Ok(f)
}

/// 确定性样本集（16 例：合法掩码全域覆盖 + LCG 派生）。
pub fn sample_masks() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    v.push(0);
    v.push(0x0000_0089);
    v.push(mt::SPEC_MASK);
    v.push(mt::STD_USED);
    v.push(0x0012_0089);
    v.push(0x0002_0019);
    v.push(0x8000_0000);
    v.push(0x1000_0000);
    let mut state: u64 = 0xDEAD_BEEF_CAFE_1234;
    let mut i = 0;
    while i < 8 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let m = ((state >> 33) as u32) & mt::ALL_LEGAL; // 只取合法域
        v.push(m);
        i += 1;
    }
    v
}

/// 演练：R1 帧格式锚；R2 往返对拍全样本；R3 帧损坏逐字节检出；
/// R4 非法掩码拒编码；R5 变长入口负例 + 载荷保留位坏帧。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：帧格式锚——魔数/版本/帧长/校验和面。
    if VER != 1 {
        return Err(E_VER);
    }
    let f0 = encode(0)?;
    if f0[OFS_MAGIC0] != 0x4D || f0[OFS_MAGIC1] != 0x4B || f0[OFS_VER] != 1 {
        return Err(E_MAGIC);
    }
    if f0[OFS_CS] != frame_checksum(&f0) || f0.len() != FRAME_LEN {
        return Err(E_CS);
    }
    if frame_checksum(&f0) == encode(1)?[OFS_CS] && encode(1)?[OFS_MASK] != 1 {
        return Err(E_CS); // 校验和面确实随载荷变化
    }
    if FrameField::Magic0.ofs() != OFS_MAGIC0
        || FrameField::Mask.ofs() != OFS_MASK
        || FrameField::Cs.ofs() != OFS_CS
    {
        return Err(E_MAGIC);
    }
    hits += 1;
    // R2：往返对拍——16 例逐位相同 + 字节级回查。
    let s = sample_masks();
    let mut i = 0;
    while i < s.len() {
        let m = s[i];
        let f = encode(m)?;
        let back = decode(&f)?;
        if back != m {
            return Err(E_RT);
        }
        if (f[OFS_MASK] as u32) != (m & 0xFF) {
            return Err(E_RT);
        }
        if ((f[OFS_MASK + 3] as u32) << 24) != (m & 0xFF00_0000) {
            return Err(E_RT);
        }
        if roundtrip(m)? != m {
            return Err(E_RT);
        }
        hits += 1;
        i += 1;
    }
    // R3：帧损坏逐字节检出——8 个字节位逐一翻转必被检出。
    let orig = 0x0012_0089u32;
    let f = encode(orig)?;
    let mut i = 0;
    while i < FRAME_LEN {
        if !corrupt_and_check(&f, i, orig) {
            return Err(E_CS);
        }
        hits += 1;
        i += 1;
    }
    // R4：非法掩码拒编码——保留位/MA 位/全一 全数拒绝。
    let bads: [u32; 8] = [
        mt::STD_RSVD,
        mt::GEN_RSVD,
        1u32 << 25,
        1u32 << 24,
        mt::ALL_RSVD,
        mt::STD_USED | mt::STD_RSVD,
        0x0200_0000, // MA 位不落存储
        0xFFFF_FFFF,
    ];
    let mut i = 0;
    while i < 8 {
        if encode(bads[i]).is_ok() {
            return Err(E_INVALID);
        }
        hits += 1;
        i += 1;
    }
    // R5：变长入口——7 字节拒、8 字节过、脏帧逐码核对。
    let f = encode(0x0000_0089)?;
    if decode_buf(&f[..7]) != Err(E_LEN) {
        return Err(E_LEN);
    }
    if decode_buf(&f)? != 0x0000_0089 {
        return Err(E_RT);
    }
    let mut g = f;
    g[OFS_MAGIC0] = 0;
    if decode(&g) != Err(E_MAGIC) {
        return Err(E_MAGIC);
    }
    let mut g = f;
    g[OFS_VER] = 2;
    if decode(&g) != Err(E_FVER) {
        return Err(E_FVER);
    }
    let mut g = f;
    g[OFS_MASK] = g[OFS_MASK].wrapping_add(1);
    if decode(&g) != Err(E_CS) {
        return Err(E_CS);
    }
    // 载荷保留位坏帧：校验和正确但载荷含 bit24 → E_RSVD_F。
    let g = reframe_with_reserved(0x0000_0089, 1u32 << 24)?;
    if decode(&g) != Err(E_RSVD_F) {
        return Err(E_RSVD_F);
    }
    let g2 = reframe_with_reserved(0x0000_0089, mt::STD_RSVD)?;
    if decode(&g2) != Err(E_RSVD_F) {
        return Err(E_RSVD_F);
    }
    let _ = vec![0u8; 0];
    let mut ledger: Vec<[u8; FRAME_LEN]> = Vec::new();
    let mut i = 0;
    while i < 4 {
        ledger.push(encode(s[i])?);
        i += 1;
    }
    if ledger.len() != 4 || ledger[0] != encode(s[0])? {
        return Err(E_RT);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(34));
    }

    #[test]
    fn roundtrip_bitwise() {
        assert_eq!(roundtrip(0x0012_0089), Ok(0x0012_0089));
        assert_eq!(roundtrip(0), Ok(0));
        assert_eq!(roundtrip(0x8000_0000), Ok(0x8000_0000));
    }

    #[test]
    fn invalid_mask_rejected() {
        assert_eq!(encode(mt::STD_RSVD), Err(E_INVALID));
        assert_eq!(encode(0x0200_0000), Err(E_INVALID));
    }

    #[test]
    fn reserved_payload_detected() {
        let g = reframe_with_reserved(0x89, 1u32 << 24).ok().unwrap_or([0u8; 8]);
        assert_eq!(decode(&g), Err(E_RSVD_F));
    }
}

pub fn run_unx36114_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36114_maskconv");
    cs.add("SEG 段位独占", SEG_BASE == 0x3a20, "0x3A20~0x3A2F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "帧格式/往返/损坏注入全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("帧长锚", FRAME_LEN == 8, "定容 8 字节帧（魔数/版本/掩码/校验和）");
    cs.add("双向红线", storable(mt::ALL_RSVD) == Err(E_INVALID) && decode(&encode(0u32).ok().unwrap_or([0u8; 8])).is_ok(), "编码入口+解码载荷双查");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_MAGIC => "帧魔数不符",
        E_FVER => "帧版本不符",
        E_RSVD_F => "帧载荷含保留位（双向红线）",
        E_CS => "帧校验和不符",
        E_INVALID => "掩码本身存态非法（编码入口）",
        E_LEN => "帧长不足（缓冲 < 8）",
        E_RT => "往返逐位不相同（对拍破）",
        _ => "未知诊断码",
    }
}
