//! UNX-F0150 · 镜像体积与拆分策略 · 诊断段 0x2590
//!
//! 判据原文（UNX-F0150-J1）：镜像超限即按段界拆分（主镜像 + 附属
//! 包），拆分账守恒（Σ分片 = Σ原内容字节），分片各自带序号与校验
//! 可独立验真，重组逐字节还原。
//!
//! 设计要点：拆分器（Splitter）——上限分片（除末片外等长）；
//! 分片头 = 魔数 + 序号 + 总片数 + 片内长 + CRC8；守恒式：
//! Σ片内长 == 原长；重组账：收齐 total 片且序号闭集才可拼。
//! 零浮点、零 unwrap/expect。

use crate::alloc::{vec, vec::Vec};

pub const SEG_BASE: u16 = 0x25A0;

const E_LIMIT: u16 = SEG_BASE + 1; // 拆分上限非法（0）
const E_BAL: u16 = SEG_BASE + 2; // 守恒被破坏
const E_CRC: u16 = SEG_BASE + 3; // 分片校验不符
const E_SEQ: u16 = SEG_BASE + 4; // 序号越纲/重复
const E_INCOMPLETE: u16 = SEG_BASE + 5; // 未收齐即重组
const E_MAGIC: u16 = SEG_BASE + 6; // 分片魔数不符

/// 分片魔数 'VRSP'。
pub const CHUNK_MAGIC: u32 = 0x5652_5350;
/// 分片头定长。
pub const CHUNK_HDR: usize = 14;

/// 分片头。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ChunkHeader {
    pub magic: u32,
    pub seq: u16, // 0 基序号
    pub total: u16,
    pub payload_len: u16,
}

impl ChunkHeader {
    pub fn serialize(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(CHUNK_HDR);
        b.extend_from_slice(&self.magic.to_le_bytes());
        b.extend_from_slice(&self.seq.to_le_bytes());
        b.extend_from_slice(&self.total.to_le_bytes());
        b.extend_from_slice(&self.payload_len.to_le_bytes());
        b
    }
}

/// CRC8（多项式 0x07）。
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            let msb = crc & 0x80;
            crc <<= 1;
            if msb != 0 {
                crc ^= 0x07;
            }
        }
    }
    crc
}

/// 上限拆分：除末片外每片恰 cap 字节；返回完整分片帧序列。
pub fn split(data: &[u8], cap: usize) -> Result<Vec<Vec<u8>>, u16> {
    if cap == 0 {
        return Err(E_LIMIT);
    }
    if data.is_empty() {
        return Err(E_BAL);
    }
    let total = ((data.len() + cap - 1) / cap) as u16;
    let mut frames = Vec::new();
    for seq in 0..total {
        let off = seq as usize * cap;
        let end = core::cmp::min(off + cap, data.len());
        let payload = &data[off..end];
        let h = ChunkHeader {
            magic: CHUNK_MAGIC,
            seq,
            total,
            payload_len: payload.len() as u16,
        };
        let mut f = h.serialize();
        f.extend_from_slice(payload);
        let crc = crc8(&f);
        f.push(crc);
        frames.push(f);
    }
    Ok(frames)
}

/// 守恒：Σ片内长 == 原长。
pub fn conserv_ok(data_len: usize, frames: &[Vec<u8>]) -> Result<(), u16> {
    let mut sum = 0usize;
    for f in frames {
        if f.len() < CHUNK_HDR + 1 {
            return Err(E_BAL);
        }
        let h = parse_hdr(f)?;
        sum += h.payload_len as usize;
    }
    if sum != data_len {
        return Err(E_BAL);
    }
    Ok(())
}

fn parse_hdr(f: &[u8]) -> Result<ChunkHeader, u16> {
    if f.len() < CHUNK_HDR {
        return Err(E_MAGIC);
    }
    Ok(ChunkHeader {
        magic: u32::from_le_bytes([f[0], f[1], f[2], f[3]]),
        seq: u16::from_le_bytes([f[4], f[5]]),
        total: u16::from_le_bytes([f[6], f[7]]),
        payload_len: u16::from_le_bytes([f[8], f[9]]),
    })
}

/// 重组器：收片 → 验 CRC/魔数/序号闭集 → 拼原像。
pub struct Assembler {
    pub total: u16,
    pub slots: Vec<Option<Vec<u8>>>, // 按序号存载荷
    pub received: u16,
}

impl Assembler {
    pub fn new(expect_total: u16) -> Result<Self, u16> {
        if expect_total == 0 {
            return Err(E_LIMIT);
        }
        Ok(Assembler {
            total: expect_total,
            slots: (0..expect_total).map(|_| None).collect(),
            received: 0,
        })
    }

    /// 收一片：验真后按序号落位（重复同号同载荷幂等，异载荷拒）。
    pub fn feed(&mut self, frame: &[u8]) -> Result<(), u16> {
        if frame.len() < CHUNK_HDR + 1 {
            return Err(E_MAGIC);
        }
        let body = &frame[..frame.len() - 1];
        if crc8(body) != frame[frame.len() - 1] {
            return Err(E_CRC);
        }
        let h = parse_hdr(body)?;
        if h.magic != CHUNK_MAGIC {
            return Err(E_MAGIC);
        }
        if h.total != self.total || h.seq as usize >= self.total as usize {
            return Err(E_SEQ);
        }
        if body.len() - CHUNK_HDR != h.payload_len as usize {
            return Err(E_BAL);
        }
        let payload = body[CHUNK_HDR..].to_vec();
        match &self.slots[h.seq as usize] {
            Some(p) if *p != payload => return Err(E_SEQ), // 同号异载
            Some(_) => {} // 幂等重收
            None => {
                self.slots[h.seq as usize] = Some(payload);
                self.received += 1;
            }
        }
        Ok(())
    }

    /// 重组：未收齐即拒；收齐后按序拼接。
    pub fn assemble(&self) -> Result<Vec<u8>, u16> {
        if self.received != self.total {
            return Err(E_INCOMPLETE);
        }
        let mut out = Vec::new();
        for s in &self.slots {
            out.extend_from_slice(s.as_ref().ok_or(E_BAL)?);
        }
        Ok(out)
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：跨上限拆分（17 字节 / cap 5 → 4 片）+ 守恒 + 重组还原。
    let data: Vec<u8> = (0..17u8).collect();
    let frames = split(&data, 5)?;
    if frames.len() != 4 {
        return Err(E_BAL);
    }
    conserv_ok(17, &frames)?;
    let mut asm = Assembler::new(4)?;
    for f in &frames {
        asm.feed(f)?;
    }
    if asm.assemble()? != data {
        return Err(E_BAL);
    }
    hits += 1;

    // L2：恰整除（15 / cap 5 → 3 片无残片）。
    let d2: Vec<u8> = (0..15u8).collect();
    let f2 = split(&d2, 5)?;
    if f2.len() != 3 {
        return Err(E_BAL);
    }
    let mut a2 = Assembler::new(3)?;
    // 乱序收片仍可重组。
    a2.feed(&f2[2])?;
    a2.feed(&f2[0])?;
    a2.feed(&f2[1])?;
    if a2.assemble()? != d2 {
        return Err(E_BAL);
    }
    hits += 1;

    // L3：单字节内容拆一片。
    let f3 = split(&[0xAB], 8)?;
    if f3.len() != 1 {
        return Err(E_BAL);
    }
    let mut a3 = Assembler::new(1)?;
    a3.feed(&f3[0])?;
    if a3.assemble()? != vec![0xAB] {
        return Err(E_BAL);
    }
    hits += 1;

    // L4：分片头布局锚（魔数 + 定长头）。
    let h = ChunkHeader {
        magic: CHUNK_MAGIC,
        seq: 0,
        total: 1,
        payload_len: 0,
    };
    if h.serialize().len() != CHUNK_HDR {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：零上限拒。
    if !matches!(split(&[1], 0), Err(E_LIMIT)) {
        return Err(E_LIMIT);
    }
    if !matches!(Assembler::new(0), Err(E_LIMIT)) {
        return Err(E_LIMIT);
    }
    hits += 1;

    // I2：CRC 翻转拒收。
    let data: Vec<u8> = (0..10u8).collect();
    let mut f2 = split(&data, 4)?;
    let n = f2[0].len();
    f2[0][n - 1] ^= 0xFF;
    let mut a2 = Assembler::new(3)?;
    if a2.feed(&f2[0]) != Err(E_CRC) {
        return Err(E_CRC);
    }
    hits += 1;

    // I3：序号越纲拒（total 不符）。
    let f3 = split(&data, 4)?;
    let mut a3 = Assembler::new(2)?; // 期望 total=2 但真 total=3
    if a3.feed(&f3[0]) != Err(E_SEQ) {
        return Err(E_SEQ);
    }
    hits += 1;

    // I4：未收齐重组拒 + 同号异载拒。
    let mut a4 = Assembler::new(3)?;
    a4.feed(&f3[0])?;
    a4.feed(&f3[1])?;
    if a4.assemble() != Err(E_INCOMPLETE) {
        return Err(E_INCOMPLETE);
    }
    // 手工改第 0 片载荷 → 与已收载荷冲突。
    let mut bad = f3[0].clone();
    let m = bad.len();
    bad[CHUNK_HDR] ^= 0xFF;
    bad[m - 1] = crc8(&bad[..m - 1]); // 重算 CRC 过 CRC 闸
    if a4.feed(&bad) != Err(E_SEQ) {
        return Err(E_SEQ);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
