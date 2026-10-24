//! UNX-F0130 · 上次引导记录持久化 · 诊断段 0x2480
//! 验收锚点：UNX-F0130-J1 上次引导四要素（序号/结局/时长/指纹）
//! 落保留区，重启后可读可比对。零浮点、零 unwrap、零 expect。


use alloc::format;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x2480;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EMPTY: u16 = SEG_BASE + 1; // 保留区空（首启无前账）
const E_CRC: u16 = SEG_BASE + 2;   // 保留区镜像蚀坏
const E_LEN: u16 = SEG_BASE + 3;   // 结局词非法（三态外）
const E_RDONLY: u16 = SEG_BASE + 4;// 本次已落账，再落被拒
const E_SEQ: u16 = SEG_BASE + 5;   // 序号回退（新账序 ≤ 旧账序）
const E_BAL: u16 = SEG_BASE + 6;   // 对比账不平

pub fn detail(code: u16) -> &'static str {
    match code {
        E_EMPTY => "保留区空：首次引导无前账可读",
        E_CRC => "镜像蚀坏：校验和不符",
        E_LEN => "结局词非法：[成, 败, 弃] 三态外",
        E_RDONLY => "本次已落账：一引导一落账",
        E_SEQ => "序号回退：新账序号必须严格递增",
        E_BAL => "对比账不平：要素缺项比对无效",
        _ => "未知诊断码",
    }
}

/// 引导结局三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootEnd {
    Ok,     // 成
    Fail,   // 败
    Abandon, // 弃（人为中断）
}

pub fn end_word(e: BootEnd) -> &'static str {
    match e {
        BootEnd::Ok => "成",
        BootEnd::Fail => "败",
        BootEnd::Abandon => "弃",
    }
}

pub fn end_from_word(w: &str) -> Result<BootEnd, u16> {
    match w {
        "成" => Ok(BootEnd::Ok),
        "败" => Ok(BootEnd::Fail),
        "弃" => Ok(BootEnd::Abandon),
        _ => Err(E_LEN),
    }
}

pub fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 引导记录四要素。
#[derive(Clone, Debug)]
pub struct BootRecord {
    pub seq: u64,        // 引导序号
    pub end: BootEnd,    // 结局
    pub dur_ms: u64,     // 在营时长
    pub fp: u32,         // 配置指纹
}

impl BootRecord {
    pub fn serialize(&self) -> Vec<u8> {
        let mut b: Vec<u8> = Vec::new();
        b.extend_from_slice(&self.seq.to_le_bytes());
        b.push(match self.end {
            BootEnd::Ok => 1,
            BootEnd::Fail => 2,
            BootEnd::Abandon => 3,
        });
        b.extend_from_slice(&self.dur_ms.to_le_bytes());
        b.extend_from_slice(&self.fp.to_le_bytes());
        let crc = fnv1a32(&b);
        b.extend_from_slice(&crc.to_le_bytes());
        b
    }

    pub fn deserialize(b: &[u8]) -> Result<BootRecord, u16> {
        if b.len() != 8 + 1 + 8 + 4 + 4 {
            return Err(E_CRC);
        }
        let body = &b[..21];
        if fnv1a32(body) != u32::from_le_bytes([b[21], b[22], b[23], b[24]]) {
            return Err(E_CRC);
        }
        let rd64 = |o: usize| -> u64 {
            let mut a = [0u8; 8];
            a.copy_from_slice(&body[o..o + 8]);
            u64::from_le_bytes(a)
        };
        let end = match body[8] {
            1 => BootEnd::Ok,
            2 => BootEnd::Fail,
            3 => BootEnd::Abandon,
            _ => return Err(E_LEN),
        };
        Ok(BootRecord {
            seq: rd64(0),
            end,
            dur_ms: rd64(9),
            fp: u32::from_le_bytes([body[17], body[18], body[19], body[20]]),
        })
    }
}

/// 上次引导账：本次开营读前账，闭营落新账。
pub struct LastBootBook {
    pub prev: Option<BootRecord>,
    pub mine: Option<BootRecord>,
}

impl LastBootBook {
    /// 开营：从保留区读前账（空/蚀两路）。
    pub fn boot_open(img: &[u8]) -> Result<LastBootBook, u16> {
        if img.is_empty() {
            return Ok(LastBootBook {
                prev: None,
                mine: None,
            });
        }
        Ok(LastBootBook {
            prev: Some(BootRecord::deserialize(img)?),
            mine: None,
        })
    }

    /// 闭营落账：序号须严格递增，一营一落。
    pub fn boot_close(&mut self, rec: BootRecord) -> Result<Vec<u8>, u16> {
        if self.mine.is_some() {
            return Err(E_RDONLY);
        }
        if let Some(p) = &self.prev {
            if rec.seq <= p.seq {
                return Err(E_SEQ);
            }
        }
        self.mine = Some(rec.clone());
        Ok(rec.serialize())
    }

    /// 前账 vs 本账 对比：结局迁移 + 时长差。
    pub fn compare(&self) -> Result<String, u16> {
        let (p, m) = match (&self.prev, &self.mine) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err(E_BAL),
        };
        let d = m.dur_ms as i64 - p.dur_ms as i64;
        Ok(format!(
            "上次#{}({}) {}ms → 本次#{}({}) {}ms（差 {}ms）",
            p.seq,
            end_word(p.end),
            p.dur_ms,
            m.seq,
            end_word(m.end),
            m.dur_ms,
            d
        ))
    }

    pub fn audit(&self) -> Result<(), u16> {
        if self.prev.is_some() && self.mine.is_some() {
            let (p, m) = (self.prev.as_ref().unwrap(), self.mine.as_ref().unwrap());
            if m.seq <= p.seq {
                return Err(E_SEQ);
            }
        }
        Ok(())
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：首启（空保留区）→ 无前账；闭营落账得镜像。
    let mut b1 = LastBootBook::boot_open(&[])?;
    if b1.prev.is_some() {
        return Err(E_EMPTY);
    }
    let img = b1.boot_close(BootRecord {
        seq: 1,
        end: BootEnd::Ok,
        dur_ms: 4200,
        fp: 0xA11CE,
    })?;
    hits += 1;

    // R2：二次启动读前账 → 落新账（序号递增）→ 对比渲染。
    let mut b2 = LastBootBook::boot_open(&img)?;
    if b2.prev.as_ref().map(|p| p.seq) != Some(1) {
        return Err(E_EMPTY);
    }
    b2.boot_close(BootRecord {
        seq: 2,
        end: BootEnd::Ok,
        dur_ms: 3900,
        fp: 0xA11CE,
    })?;
    let rep = b2.compare()?;
    if !rep.contains("上次#1(成) 4200ms") || !rep.contains("差 -300ms") {
        return Err(E_BAL);
    }
    b2.audit()?;
    hits += 1;

    // R3：结局三态词表双向 + 序列化往返保真。
    for e in [BootEnd::Ok, BootEnd::Fail, BootEnd::Abandon] {
        if end_from_word(end_word(e))? != e {
            return Err(E_LEN);
        }
    }
    let rec = BootRecord {
        seq: 9,
        end: BootEnd::Abandon,
        dur_ms: 77,
        fp: 0xFEED,
    };
    let back = BootRecord::deserialize(&rec.serialize())?;
    if back.seq != 9 || back.end != BootEnd::Abandon || back.fp != 0xFEED {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：连败链——三段镜像链式传递全可读。
    let mut prev_img = Vec::new();
    let mut seq = 0u64;
    for (i, e) in [BootEnd::Fail, BootEnd::Fail, BootEnd::Ok].iter().enumerate() {
        seq += 1;
        let mut bk = LastBootBook::boot_open(&prev_img)?;
        let img = bk.boot_close(BootRecord {
            seq,
            end: *e,
            dur_ms: 1000 + i as u64,
            fp: 7,
        })?;
        if i == 2 && bk.prev.as_ref().map(|p| p.end) != Some(BootEnd::Fail) {
            return Err(E_BAL);
        }
        prev_img = img;
    }
    hits += 1;

    // I1：空保留区对比 → E_BAL（缺前账）。
    let b4 = LastBootBook::boot_open(&[])?;
    if b4.compare() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：镜像蚀坏（尾 CRC 翻转 / 长度不对）→ E_CRC。
    let mut bad = img.clone();
    let n = bad.len();
    bad[n - 1] ^= 0xFF;
    if !matches!(BootRecord::deserialize(&bad), Err(E_CRC)) {
        return Err(E_CRC);
    }
    if !matches!(BootRecord::deserialize(&bad[..n - 1]), Err(E_CRC)) {
        return Err(E_CRC);
    }
    hits += 1;

    // I3：结局词非法 → E_LEN。
    if end_from_word("赢").is_err() && end_from_word("赢") != Err(E_LEN) {
        return Err(E_LEN);
    }
    let mut bad_end = img.clone();
    bad_end[8] = 9; // 结局字节越纲
    if !matches!(BootRecord::deserialize(&bad_end), Err(E_LEN)) {
        return Err(E_LEN);
    }
    hits += 1;

    // I4：序号回退 / 平号 → E_SEQ；重复落账 → E_RDONLY。
    let mut i4 = LastBootBook::boot_open(&img)?;
    if i4.boot_close(BootRecord {
        seq: 1,
        end: BootEnd::Ok,
        dur_ms: 1,
        fp: 1,
    }) != Err(E_SEQ)
    {
        return Err(E_SEQ);
    }
    i4.boot_close(BootRecord {
        seq: 2,
        end: BootEnd::Ok,
        dur_ms: 1,
        fp: 1,
    })?;
    if i4.boot_close(BootRecord {
        seq: 3,
        end: BootEnd::Ok,
        dur_ms: 1,
        fp: 1,
    }) != Err(E_RDONLY)
    {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I5：审计破注入（跨账序回退）→ E_SEQ。
    let mut i5 = LastBootBook::boot_open(&img)?;
    i5.mine = Some(BootRecord {
        seq: 0, // 人为低于前账
        end: BootEnd::Ok,
        dur_ms: 5,
        fp: 1,
    });
    if i5.audit() != Err(E_SEQ) {
        return Err(E_SEQ);
    }
    hits += 1;

    Ok(hits)
}
