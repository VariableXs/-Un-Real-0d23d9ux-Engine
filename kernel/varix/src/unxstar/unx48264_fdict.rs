//! unx48264_fdict — UNX-F48264 fcitx5 词库 .dict 导入导出（CRC 闭环 ×3 词库）（诊断段 0xC1E0）。
//!
//! 判据原文：
//!   J1 三词库往返 CRC 闭环全对（§AI-61 §5 J-3 构成项）；判据锚定 §AI-61 §5 J-3。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 封装定式：.dict 流 = 头（magic "FDIC" + 条目数）+ 定长记录区
//!      （每条 10 字节：code u32 / freq u32 / word u16，小端）+ 尾
//!      （CRC-32 校验字，覆盖头部与记录区）。
//!   2. 往返闭环（J1 主体）：导出 → 导入 → 条目逐条逐字段相等、频次
//!      总量守恒、重导出字节流与首导出逐位相同——三词库（16/40/128 条）
//!      各自成环，逐库全对。
//!   3. CRC 守门：导入侧重算 CRC 与尾字比对，不等即 E_CRC_BAD；头
//!      magic 不符即 E_MAGIC；长度与条目数不闭合即 E_LEN_BAD——坏流
//!      一律拒收，不落半库。
//!   4. 条目账：code 非零、word 非零双校验入库；同 code 重条拒收
//!      （E_ENTRY_OFF），词库内条目守恒可查。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC1E0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_MAGIC: u16 = SEG_BASE + 1; // 头 magic 不符
const E_LEN_BAD: u16 = SEG_BASE + 2; // 长度与条目数不闭合
const E_CRC_BAD: u16 = SEG_BASE + 3; // CRC 校验失守
const E_ENTRY_OFF: u16 = SEG_BASE + 4; // 条目账违规（重码/零字段）
const E_DICT_BAD: u16 = SEG_BASE + 5; // 词库守恒破
const E_STATE_BAD: u16 = SEG_BASE + 6; // 封装器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 封装常量：magic / 记录字长 / CRC 多项式。
pub const MAGIC: u32 = 0x4644_4943; // "FDIC"
pub const REC_BYTES: usize = 10;
pub const CRC_POLY: u32 = 0xEDB8_8320;

/// 词库条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DictEntry {
    pub code: u32,
    pub freq: u32,
    pub word: u16,
}

/// 词库。
#[derive(Clone, Debug)]
pub struct Dict {
    pub id: u16,
    pub entries: Vec<DictEntry>,
}

impl Dict {
    pub fn new(id: u16) -> Dict {
        Dict { id, entries: Vec::new() }
    }
    /// 入库双校验：code/word 非零、code 不重。
    pub fn add(&mut self, code: u32, freq: u32, word: u16) -> Result<(), u16> {
        if code == 0 || word == 0 {
            return Err(E_ENTRY_OFF);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].code == code {
                return Err(E_ENTRY_OFF);
            }
            i += 1;
        }
        self.entries.push(DictEntry { code, freq, word });
        Ok(())
    }
    pub fn freq_sum(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            s += self.entries[i].freq as u64;
            i += 1;
        }
        s
    }
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
    /// 词库守恒审计：条目账与频次账闭合。
    pub fn audit(&self) -> Result<(), u16> {
        if self.entry_count() == 0 {
            return Err(E_DICT_BAD);
        }
        // 重码终扫（导入路径外的复核）。
        let mut a = 0usize;
        while a < self.entries.len() {
            if self.entries[a].code == 0 || self.entries[a].word == 0 {
                return Err(E_ENTRY_OFF);
            }
            let mut b = a + 1;
            while b < self.entries.len() {
                if self.entries[a].code == self.entries[b].code {
                    return Err(E_ENTRY_OFF);
                }
                b += 1;
            }
            a += 1;
        }
        Ok(())
    }
}

/// CRC-32（bitwise，多项式 0xEDB88320，初值/终值 0xFFFFFFFF 惯例）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0usize;
    while i < data.len() {
        crc ^= data[i] as u32;
        let mut b = 0u32;
        while b < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ CRC_POLY;
            } else {
                crc >>= 1;
            }
            b += 1;
        }
        i += 1;
    }
    crc ^ 0xFFFF_FFFF
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
    out.push(((v >> 24) & 0xFF) as u8);
}
fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
}

/// 导出：.dict 流封装（纪律 1），全流程确定性（同库同字节）。
pub fn export(d: &Dict) -> Result<Vec<u8>, u16> {
    if d.entry_count() == 0 {
        return Err(E_DICT_BAD);
    }
    let mut out: Vec<u8> = Vec::new();
    push_u32(&mut out, MAGIC);
    push_u32(&mut out, d.entry_count() as u32);
    let mut i = 0usize;
    while i < d.entries.len() {
        let e = d.entries[i];
        push_u32(&mut out, e.code);
        push_u32(&mut out, e.freq);
        push_u16(&mut out, e.word);
        i += 1;
    }
    let crc = crc32(&out);
    push_u32(&mut out, crc);
    Ok(out)
}

/// 导入：守门三查（magic/长度/CRC）+ 条目重建，坏流拒收不落半库。
pub fn import(bytes: &[u8]) -> Result<Dict, u16> {
    if bytes.len() < 12 {
        return Err(E_LEN_BAD);
    }
    let magic = (bytes[0] as u32)
        | ((bytes[1] as u32) << 8)
        | ((bytes[2] as u32) << 16)
        | ((bytes[3] as u32) << 24);
    if magic != MAGIC {
        return Err(E_MAGIC);
    }
    let count = (bytes[4] as u32)
        | ((bytes[5] as u32) << 8)
        | ((bytes[6] as u32) << 16)
        | ((bytes[7] as u32) << 24);
    let body_end = bytes.len() - 4;
    let want_len = 8 + count as usize * REC_BYTES;
    if body_end != want_len {
        return Err(E_LEN_BAD);
    }
    let crc_stored = (bytes[body_end] as u32)
        | ((bytes[body_end + 1] as u32) << 8)
        | ((bytes[body_end + 2] as u32) << 16)
        | ((bytes[body_end + 3] as u32) << 24);
    if crc32(&bytes[..body_end]) != crc_stored {
        return Err(E_CRC_BAD);
    }
    let mut d = Dict::new(0);
    let mut off = 8usize;
    let mut n = 0u32;
    while n < count {
        let code = (bytes[off] as u32)
            | ((bytes[off + 1] as u32) << 8)
            | ((bytes[off + 2] as u32) << 16)
            | ((bytes[off + 3] as u32) << 24);
        let freq = (bytes[off + 4] as u32)
            | ((bytes[off + 5] as u32) << 8)
            | ((bytes[off + 6] as u32) << 16)
            | ((bytes[off + 7] as u32) << 24);
        let word = (bytes[off + 8] as u16) | ((bytes[off + 9] as u16) << 8);
        d.add(code, freq, word)?; // 条目账双校验沿导入路径继续把关
        off += REC_BYTES;
        n += 1;
    }
    Ok(d)
}

/// 完成写回（判据 J2）：闭环成立 + 非空摘要方放行。
pub fn writeback(done: bool, loops_green: u32, summary_len: u32) -> Result<(), u16> {
    if !done || loops_green == 0 {
        return Err(E_STATE_BAD);
    }
    if summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（三词库往返 CRC 闭环全对），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // 构造三词库：16 / 40 / 128 条，确定性生成（code 步进互不重）。
    let mut d1 = Dict::new(1);
    let mut i = 0u32;
    while i < 16 {
        d1.add(0x1000 + i, 100 + i, 0x4E00 + i as u16)?;
        i += 1;
    }
    let mut d2 = Dict::new(2);
    i = 0;
    while i < 40 {
        d2.add(0x2000 + i * 3, 50 + i, 0x4E10 + i as u16)?;
        i += 1;
    }
    let mut d3 = Dict::new(3);
    i = 0;
    while i < 128 {
        d3.add(0x3000 + i * 7, 10 + (i % 97), 0x4E40 + i as u16)?;
        i += 1;
    }
    d1.audit()?;
    d2.audit()?;
    d3.audit()?;

    // R1 三词库往返闭环（J1 主体）：导出→导入→逐条逐字段相等、
    //    频次总量守恒、重导出与首导出逐位相同。
    let dicts: [&Dict; 3] = [&d1, &d2, &d3];
    let mut loops_green = 0u32;
    let mut k = 0usize;
    while k < 3 {
        let bytes = export(dicts[k])?;
        let back = import(&bytes)?;
        if back.entry_count() != dicts[k].entry_count() {
            return Err(E_DICT_BAD);
        }
        let mut j = 0usize;
        while j < dicts[k].entries.len() {
            let a = dicts[k].entries[j];
            let b = back.entries[j];
            if a.code != b.code || a.freq != b.freq || a.word != b.word {
                return Err(E_DICT_BAD);
            }
            j += 1;
        }
        if back.freq_sum() != dicts[k].freq_sum() {
            return Err(E_DICT_BAD); // 频次总量守恒
        }
        let bytes2 = export(&back)?;
        if bytes2.len() != bytes.len() {
            return Err(E_LEN_BAD);
        }
        let mut m = 0usize;
        while m < bytes.len() {
            if bytes2[m] != bytes[m] {
                return Err(E_STATE_BAD); // 重导出漂移
            }
            m += 1;
        }
        back.audit()?;
        loops_green += 1;
        k += 1;
    }
    if loops_green != 3 {
        return Err(E_DICT_BAD);
    }
    hits += 1;

    // R2 CRC 守门负例：导出流尾字节翻转（模拟传输位错）→ 导入必拒
    //    E_CRC_BAD；换库再验一次（×3 词库守门全关）。
    let bytes = export(&d1)?;
    let mut corrupt = Vec::new();
    let mut m = 0usize;
    while m < bytes.len() {
        corrupt.push(if m == bytes.len() - 2 { bytes[m] ^ 0x40 } else { bytes[m] });
        m += 1;
    }
    if import(&corrupt).is_ok() {
        return Err(E_CRC_BAD);
    }
    let bytes3 = export(&d3)?;
    let mut corrupt3 = Vec::new();
    m = 0;
    while m < bytes3.len() {
        corrupt3.push(if m == 9 { bytes3[m] ^ 0x01 } else { bytes3[m] });
        m += 1;
    }
    if import(&corrupt3).is_ok() {
        return Err(E_CRC_BAD);
    }
    hits += 1;

    // R3 magic 守门：头字改写 → E_MAGIC。
    let bytes = export(&d2)?;
    let mut bad_magic = Vec::new();
    m = 0;
    while m < bytes.len() {
        bad_magic.push(if m == 0 { bytes[m] ^ 0xFF } else { bytes[m] });
        m += 1;
    }
    if import(&bad_magic).is_ok() {
        return Err(E_MAGIC);
    }
    hits += 1;

    // R4 长度守门：截断（吞掉一条记录 + 尾字）→ E_LEN_BAD；
    //    条目数虚标（count+1 而体不变）→ E_LEN_BAD。
    let bytes = export(&d1)?;
    if import(&bytes[..bytes.len() - 6]).is_ok() {
        return Err(E_LEN_BAD);
    }
    let mut inflated = Vec::new();
    // 头部 count 虚标 +1。
    inflated.push(bytes[0]);
    inflated.push(bytes[1]);
    inflated.push(bytes[2]);
    inflated.push(bytes[3]);
    inflated.push(bytes[4]);
    inflated.push(bytes[5]);
    inflated.push(bytes[6]);
    inflated.push(bytes[7] + 1);
    m = 8;
    while m < bytes.len() {
        inflated.push(bytes[m]);
        m += 1;
    }
    if import(&inflated).is_ok() {
        return Err(E_LEN_BAD);
    }
    hits += 1;

    // R5 条目账负例：零 code / 零 word / 重码入库三拒（E_ENTRY_OFF）。
    let mut dx = Dict::new(9);
    if dx.add(0, 10, 0x4E00).is_ok() {
        return Err(E_ENTRY_OFF);
    }
    if dx.add(0x9001, 10, 0).is_ok() {
        return Err(E_ENTRY_OFF);
    }
    dx.add(0x9002, 10, 0x4E01)?;
    if dx.add(0x9002, 20, 0x4E02).is_ok() {
        return Err(E_ENTRY_OFF);
    }
    if dx.entry_count() != 1 {
        return Err(E_DICT_BAD);
    }
    hits += 1;

    // R6 空库拒导出 + 导入短路：空库 → E_DICT_BAD；短流 → E_LEN_BAD。
    let empty = Dict::new(8);
    if export(&empty).is_ok() {
        return Err(E_DICT_BAD);
    }
    if import(&bytes[..8]).is_ok() {
        return Err(E_LEN_BAD);
    }
    hits += 1;

    // R7 写回（判据 J2）：零闭环不写回、空摘要拒、×3 闭环+非空摘要放行。
    if writeback(true, 0, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    if writeback(true, 3, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 3, 0x30)?;
    hits += 1;

    Ok(hits)
}
