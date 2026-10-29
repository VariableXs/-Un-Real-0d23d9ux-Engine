//! unx0617_handoff —— UNX-F0617 initrd 交接字段（文件域预备）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0617 initrd 交接字段（文件域预备）  <SEG 0x4ee0>
//! 目标行数：≥240（判据：纯功能代码 ≥240 行）
//! 判据原文摘录：
//!   ② 判据原文：交接字段 v1 冻结（8 字段），消费方 mock diff 零差。
//!
//! 设计纪律：
//!   1. 交接字段 v1 冻结八件（写死序）：① 档基址 ② 档长 ③ 条目
//      数 ④ 魔数核旗 ⑤ TRAILER 旗 ⑥ 校验失配数 ⑦ 清单指纹
//      ⑧ 域版本号 v1——字段序与字段宽冻结，变更即 v2（v1 内零
//      变更）；
//   2. 字段宽冻结：①② u32 / ③ u16 / ④⑤⑥ u8 / ⑦ u32 / ⑧ u16，
//      序列化 19 字节定长（4+4+2+1+1+1+4+2）；
//   3. 消费方 mock diff 零差：mock 以同序列化规则重建交接帧，
//      与封存帧逐字节对拍，零差 = 一致；
//   4. 冻结校验：字段表哈希（FNV-1a 序列化字节）封存后恒定；
//   5. 消费闸：消费方必须出示域版本号 v1（=1）才可取帧，版本
//      不符红码（预备面：文件域消费起点）；
//   6. 禁浮点禁 std，无 unwrap/expect/panic，确定性。
//!
//! 诊断段声明：0x4ee0 段为本模块独占（SEG_BASE=0x4ee0），判别式
//   SEG_BASE+1..+N；与 0x4ec0/0x4f00 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const HF_VERSION: &str = "unx0617_handoff-v1";

/// 诊断段基址（0x4ee0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4EE0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4ee1 域版本不符（非 v1）。
pub const XC_BAD_DOMAIN_VER: u16 = SEG_BASE + 1;
/// 0x4ee2 交接帧尺寸漂移（≠19 字节）。
pub const XC_FRAME_SIZE: u16 = SEG_BASE + 2;
/// 0x4ee3 mock diff 有差。
pub const XC_MOCK_DIFF: u16 = SEG_BASE + 3;
/// 0x4ee4 封存后仍试图改字段。
pub const XC_SEALED_WRITE: u16 = SEG_BASE + 4;
/// 0x4ee5 哈希复算失配。
pub const XC_HASH_MISMATCH: u16 = SEG_BASE + 5;
/// 0x4ee6 未封存就出消费帧。
pub const XC_NOT_SEALED: u16 = SEG_BASE + 6;
/// 0x4ee7 档基址未对齐（非 4K 界——预备面写死）。
pub const XC_BASE_ALIGN: u16 = SEG_BASE + 7;
/// 0x4ee8 档长为零。
pub const XC_ZERO_LEN: u16 = SEG_BASE + 8;

/// 交接帧宽（写死：19 字节 = 4+4+2+1+1+1+4+2）。
pub const FRAME_W: usize = 19;
/// 域版本 v1（写死）。
pub const DOMAIN_VER: u16 = 1;
/// 档基址对齐界（写死：4096）。
pub const BASE_ALIGN: u32 = 4096;

/// 交接八字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoffFields {
    /// ① 档基址（4K 对齐）。
    pub base: u32,
    /// ② 档长。
    pub arc_len: u32,
    /// ③ 条目数。
    pub entries: u16,
    /// ④ 魔数核旗（1 = 命中）。
    pub magic_ok: u8,
    /// ⑤ TRAILER 旗（1 = 末条目命中）。
    pub trailer_ok: u8,
    /// ⑥ 校验失配数。
    pub check_miss: u8,
    /// ⑦ 清单指纹。
    pub manifest_fp: u32,
    /// ⑧ 域版本号 v1。
    pub domain_ver: u16,
}

/// FNV-1a 字节序列哈希。
pub fn fnv_bytes(data: &[u8]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 交接帧序列化（19 字节定长，字段序冻结，小端）。
/// 布局：[0..4) base [4..8) arc_len [8..10) entries [10] magic_ok
///       [11] trailer_ok [12] check_miss [13..17) manifest_fp
///       [17..19) domain_ver。
pub fn serialize(f: &HandoffFields) -> Result<[u8; FRAME_W], u16> {
    if f.domain_ver != DOMAIN_VER {
        return Err(XC_BAD_DOMAIN_VER);
    }
    let mut out = [0u8; FRAME_W];
    out[0] = (f.base & 0xFF) as u8;
    out[1] = ((f.base >> 8) & 0xFF) as u8;
    out[2] = ((f.base >> 16) & 0xFF) as u8;
    out[3] = (f.base >> 24) as u8;
    out[4] = (f.arc_len & 0xFF) as u8;
    out[5] = ((f.arc_len >> 8) & 0xFF) as u8;
    out[6] = ((f.arc_len >> 16) & 0xFF) as u8;
    out[7] = (f.arc_len >> 24) as u8;
    out[8] = (f.entries & 0xFF) as u8;
    out[9] = (f.entries >> 8) as u8;
    out[10] = f.magic_ok;
    out[11] = f.trailer_ok;
    out[12] = f.check_miss;
    let fp = f.manifest_fp.to_le_bytes();
    out[13] = fp[0];
    out[14] = fp[1];
    out[15] = fp[2];
    out[16] = fp[3];
    out[17] = (f.domain_ver & 0xFF) as u8;
    out[18] = (f.domain_ver >> 8) as u8;
    Ok(out)
}

/// 交接帧反序列化（mock 消费面）。
pub fn deserialize(frame: &[u8]) -> Result<HandoffFields, u16> {
    if frame.len() != FRAME_W {
        return Err(XC_FRAME_SIZE);
    }
    let rd32 = |o: usize| -> u32 {
        frame[o] as u32
            | ((frame[o + 1] as u32) << 8)
            | ((frame[o + 2] as u32) << 16)
            | ((frame[o + 3] as u32) << 24)
    };
    let f = HandoffFields {
        base: rd32(0),
        arc_len: rd32(4),
        entries: frame[8] as u16 | ((frame[9] as u16) << 8),
        magic_ok: frame[10],
        trailer_ok: frame[11],
        check_miss: frame[12],
        manifest_fp: rd32(13),
        domain_ver: frame[17] as u16 | ((frame[18] as u16) << 8),
    };
    if f.domain_ver != DOMAIN_VER {
        return Err(XC_BAD_DOMAIN_VER);
    }
    Ok(f)
}

/// 交接字段封存账。
pub struct HandoffSeal {
    fields: HandoffFields,
    /// 封存哈希。
    hash: Option<u32>,
}

impl HandoffSeal {
    /// 新封存账（字段面就绪，未封存）。
    pub fn new(fields: HandoffFields) -> Result<Self, u16> {
        if fields.domain_ver != DOMAIN_VER {
            return Err(XC_BAD_DOMAIN_VER);
        }
        if fields.arc_len == 0 {
            return Err(XC_ZERO_LEN);
        }
        if fields.base % BASE_ALIGN != 0 {
            return Err(XC_BASE_ALIGN);
        }
        Ok(HandoffSeal {
            fields,
            hash: None,
        })
    }

    /// 字段改写（封存前限定）。
    pub fn amend(&mut self, f: HandoffFields) -> Result<(), u16> {
        if self.hash.is_some() {
            return Err(XC_SEALED_WRITE);
        }
        if f.domain_ver != DOMAIN_VER {
            return Err(XC_BAD_DOMAIN_VER);
        }
        self.fields = f;
        Ok(())
    }

    /// 封存：序列化 → 哈希落账。二次封红码。
    pub fn seal(&mut self) -> Result<u32, u16> {
        if self.hash.is_some() {
            return Err(XC_SEALED_WRITE);
        }
        let frame = serialize(&self.fields)?;
        let h = fnv_bytes(&frame);
        self.hash = Some(h);
        Ok(h)
    }

    /// 消费帧（封存限定；哈希复算核）。
    pub fn frame(&self) -> Result<[u8; FRAME_W], u16> {
        let h = self.hash.ok_or(XC_NOT_SEALED)?;
        let frame = serialize(&self.fields)?;
        if fnv_bytes(&frame) != h {
            return Err(XC_HASH_MISMATCH);
        }
        Ok(frame)
    }

    /// 封存态。
    pub fn is_sealed(&self) -> bool {
        self.hash.is_some()
    }

    /// 字段只读。
    pub fn fields(&self) -> &HandoffFields {
        &self.fields
    }
}

/// 消费闸：消费方出示域版本 → 取帧。
pub fn consume(seal: &HandoffSeal, claimed_ver: u16) -> Result<HandoffFields, u16> {
    if claimed_ver != DOMAIN_VER {
        return Err(XC_BAD_DOMAIN_VER);
    }
    let frame = seal.frame()?;
    deserialize(&frame)
}

/// mock diff 零差：消费方独立重建字段 → 序列化 → 与封存帧逐字节对拍。
pub fn mock_diff(seal: &HandoffSeal, rebuilt: &HandoffFields) -> Result<(), u16> {
    if !seal.is_sealed() {
        return Err(XC_NOT_SEALED);
    }
    let a = seal.frame()?;
    let b = serialize(rebuilt)?;
    for i in 0..FRAME_W {
        if a[i] != b[i] {
            return Err(XC_MOCK_DIFF);
        }
    }
    Ok(())
}

/// 从真实档提取字段面（预备面生产路径）。
/// 提取三件：条目数 / 魔数核旗 / TRAILER 旗 + 校验失配数 +
/// 清单指纹（复用 0x4ee0 段自足实现——不跨模块依赖）。
pub fn extract_fields(base: u32, stream: &[u8]) -> Result<HandoffFields, u16> {
    if stream.is_empty() {
        return Err(XC_ZERO_LEN);
    }
    if base % BASE_ALIGN != 0 {
        return Err(XC_BASE_ALIGN);
    }
    const HEAD: usize = 110;
    const MAGIC: &[u8] = b"070701";
    const TRAILER: &str = "TRAILER!!!";
    let mut entries = 0u16;
    let mut check_miss = 0u8;
    let mut magic_ok = 0u8;
    let mut trailer_ok = 0u8;
    // 魔数核。
    if stream.len() >= 6 && &stream[0..6] == MAGIC {
        magic_ok = 1;
    }
    // 遍历。
    let mut off = 0usize;
    let mut fp_hash = 0x811C_9DC5u32;
    loop {
        if off + HEAD > stream.len() {
            break;
        }
        let head = &stream[off..off + HEAD];
        if &head[0..6] != MAGIC {
            break;
        }
        let rd = |o: usize| -> u32 {
            let mut v = 0u32;
            for i in 0..8 {
                let c = head[o + i];
                let d = match c {
                    b'0'..=b'9' => (c - b'0') as u32,
                    b'a'..=b'f' => (c - b'a' + 10) as u32,
                    _ => 0xFFFF_0001,
                };
                v = (v << 4) | d;
            }
            v
        };
        let filesize = rd(54);
        let namesize = rd(94);
        let declared = rd(102);
        if namesize < 1 {
            break;
        }
        let ns = namesize as usize;
        let name_off = off + HEAD;
        if name_off + ns > stream.len() {
            break;
        }
        if stream[name_off + ns - 1] != 0 {
            break;
        }
        let mut name_bytes: Vec<u8> = Vec::new();
        for b in &stream[name_off..name_off + ns - 1] {
            name_bytes.push(*b);
        }
        let data_off = name_off + ns + (4 - ns % 4) % 4;
        // 校验和核。
        let mut s = 0u32;
        if data_off + filesize as usize <= stream.len() {
            for b in &stream[data_off..data_off + filesize as usize] {
                s = (s + *b as u32) % 65536;
            }
        }
        if (declared & 0xFFFF) != s && check_miss < 255 {
            check_miss += 1;
        }
        entries += 1;
        // 指纹吃：名 + filesize。
        for b in &name_bytes {
            fp_hash ^= *b as u32;
            fp_hash = fp_hash.wrapping_mul(0x0100_0193);
        }
        fp_hash ^= 0;
        fp_hash = fp_hash.wrapping_mul(0x0100_0193);
        let fb = filesize.to_le_bytes();
        for b in fb {
            fp_hash ^= b as u32;
            fp_hash = fp_hash.wrapping_mul(0x0100_0193);
        }
        // TRAILER 检出。
        let name_is_trailer = name_bytes == TRAILER.as_bytes();
        if name_is_trailer {
            trailer_ok = 1;
            break;
        }
        let data_end = data_off + filesize as usize + (4 - filesize as usize % 4) % 4;
        if data_end >= stream.len() {
            break;
        }
        off = data_end;
    }
    Ok(HandoffFields {
        base,
        arc_len: stream.len() as u32,
        entries,
        magic_ok,
        trailer_ok,
        check_miss,
        manifest_fp: fp_hash,
        domain_ver: DOMAIN_VER,
    })
}

/// 自例程：真档提取字段 → 封存 → 消费 → mock 零差 → 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 构造真档（自足构造器，与其它单解耦的简版）。
    const HEAD: usize = 110;
    let mut arc: Vec<u8> = Vec::new();
    let mk = |name: &str, data: &[u8]| -> Vec<u8> {
        let mut h = [0u8; HEAD];
        h[0..6].copy_from_slice(b"070701");
        let mut put = |o: usize, v: u32, h: &mut [u8; HEAD]| {
            let be = v.to_be_bytes();
            for i in 0..8 {
                let nib = if i % 2 == 0 { (be[i / 2] >> 4) & 0xF } else { be[i / 2] & 0xF };
                h[o + i] = if nib < 10 { b'0' + nib } else { b'a' + nib - 10 };
            }
        };
        put(54, data.len() as u32, &mut h);
        put(94, (name.len() + 1) as u32, &mut h);
        let mut s = 0u32;
        for b in data {
            s = (s + *b as u32) % 65536;
        }
        put(102, s, &mut h);
        let mut out = Vec::from(h.as_slice());
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        while (out.len() - HEAD) % 4 != 0 {
            out.push(0);
        }
        out.extend_from_slice(data);
        while data.len() % 4 != 0 {
            out.push(0);
            break;
        }
        out
    };
    arc.extend(mk("init", b"boot"));
    arc.extend(mk("cfg", b"{}"));
    arc.extend(mk("TRAILER!!!", &[]));

    // 主路径：提取 → 封存 → 消费。
    let fields = extract_fields(0x0040_0000, &arc)?;
    if fields.magic_ok != 1 || fields.trailer_ok != 1 {
        return Err(XC_MOCK_DIFF);
    }
    if fields.entries != 3 || fields.check_miss != 0 {
        return Err(XC_MOCK_DIFF);
    }
    if fields.arc_len != arc.len() as u32 {
        return Err(XC_MOCK_DIFF);
    }
    let mut seal = HandoffSeal::new(fields)?;
    if seal.is_sealed() {
        return Err(XC_NOT_SEALED);
    }
    let h = seal.seal()?;
    let frame = seal.frame()?;
    if frame.len() != FRAME_W {
        return Err(XC_FRAME_SIZE);
    }
    // 消费闸：版本对。
    let got = consume(&seal, DOMAIN_VER)?;
    if got.base != 0x0040_0000 || got.entries != 3 {
        return Err(XC_MOCK_DIFF);
    }
    // mock 零差：独立重建同字段。
    let rebuilt = extract_fields(0x0040_0000, &arc)?;
    mock_diff(&seal, &rebuilt)?;
    // 封存哈希稳定。
    if seal.frame()? [0] != frame[0] {
        return Err(XC_HASH_MISMATCH);
    }

    // 错误路径 1：版本不符消费。
    if consume(&seal, 2) != Err(XC_BAD_DOMAIN_VER) {
        return Err(XC_BAD_DOMAIN_VER);
    }
    // 错误路径 2：mock diff 有差（字段不同）。
    let mut diff = rebuilt;
    diff.entries += 1;
    if mock_diff(&seal, &diff) != Err(XC_MOCK_DIFF) {
        return Err(XC_MOCK_DIFF);
    }
    // 错误路径 3：封存后改字段。
    if seal.amend(rebuilt) != Err(XC_SEALED_WRITE) {
        return Err(XC_SEALED_WRITE);
    }
    // 错误路径 4：基址未对齐。
    let f4 = extract_fields(0x1234, &arc)?;
    if HandoffSeal::new(f4).err() != Some(XC_BASE_ALIGN) {
        return Err(XC_BASE_ALIGN);
    }
    // 错误路径 5：档长零。
    if extract_fields(0x0040_0000, &[]) != Err(XC_ZERO_LEN) {
        return Err(XC_ZERO_LEN);
    }
    // 错误路径 6：反序列化尺寸漂移。
    if deserialize(&frame[..10]) != Err(XC_FRAME_SIZE) {
        return Err(XC_FRAME_SIZE);
    }
    // 序列化确定性：同字段两帧全等。
    let f2 = serialize(&rebuilt)?;
    for i in 0..FRAME_W {
        if f2[i] != frame[i] {
            return Err(XC_MOCK_DIFF);
        }
    }
    let _ = h;
    Ok(0)
}
