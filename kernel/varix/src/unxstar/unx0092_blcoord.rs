//! UNX-F0092 · Bootloader 协同随机化接口 · 诊断段 0x2280
//! 验收锚点：UNX-F0092-J1 引导器与内核间的熵交接帧（结构+校验）
//! 与熵质量门槛判据通过；纯功能代码 ≥220 行。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{vec, vec::Vec};

pub const SEG_BASE: u16 = 0x2280;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MAGIC: u16 = SEG_BASE + 1;    // 熵帧魔数不符
const E_VER: u16 = SEG_BASE + 2;      // 帧版本不支持
const E_LEN: u16 = SEG_BASE + 3;      // 熵载荷长度越界
const E_FP: u16 = SEG_BASE + 4;       // 载荷指纹不符
const E_QUALITY: u16 = SEG_BASE + 5;  // 熵质量不达标
const E_RDONLY: u16 = SEG_BASE + 6;   // 熵帧已消费
const E_BAL: u16 = SEG_BASE + 7;      // 熵账守恒不平

/// 熵帧魔数 "VBE1"（Varix Boot Entropy v1）。
pub const EFR_MAGIC: [u8; 4] = [0x56, 0x42, 0x45, 0x31];
/// 帧版本。
pub const EFR_VER: u32 = 1;
/// 载荷长度上下界：32 字节 .. 4KiB。
pub const LOAD_MIN: usize = 32;
pub const LOAD_MAX: usize = 4096;
/// 帧头长：魔数 4 + 版本 4 + 载荷长 4 + 指纹 8 = 20。
pub const EFR_HDR: usize = 20;
/// 质量门槛：载荷内唯一字节数 ≥ 32（粗粒度卡方替代）。
pub const QUALITY_MIN_UNIQ: usize = 32;

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
        E_MAGIC => "熵帧魔数非 VBE1：引导器未交接熵或内存损坏",
        E_VER => "帧版本不支持：仅 v1",
        E_LEN => "载荷长度越界：32..4096 字节",
        E_FP => "载荷指纹不符：交接途中被改",
        E_QUALITY => "熵质量不达标：唯一字节 <32（RDRAND 缺席嫌疑）",
        E_RDONLY => "熵帧已消费：一次性使用",
        E_BAL => "熵账守恒：消费数 ≤ 供给数",
        _ => "未知诊断码",
    }
}

/// 熵质量粗判：唯一字节数。
pub fn entropy_quality(payload: &[u8]) -> usize {
    let mut seen = [false; 256];
    for b in payload {
        seen[*b as usize] = true;
    }
    seen.iter().filter(|x| **x).count()
}

// ── 熵帧账 ───────────────────────────────────────────────────
pub struct EntropyFrame {
    pub payload: Vec<u8>,
    pub consumed: bool,
}

impl EntropyFrame {
    pub fn new(payload: Vec<u8>) -> Result<EntropyFrame, u16> {
        if payload.len() < LOAD_MIN || payload.len() > LOAD_MAX {
            return Err(E_LEN);
        }
        Ok(EntropyFrame {
            payload,
            consumed: false,
        })
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&EFR_MAGIC);
        out.extend_from_slice(&EFR_VER.to_le_bytes());
        out.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&fnv1a64(&self.payload).to_le_bytes());
        out.extend_from_slice(&self.payload);
        out
    }

    /// 反序列化 + 指纹核验 + 质量门槛。
    pub fn deserialize_and_check(blob: &[u8]) -> Result<EntropyFrame, u16> {
        if blob.len() < EFR_HDR {
            return Err(E_LEN);
        }
        if blob[0..4] != EFR_MAGIC {
            return Err(E_MAGIC);
        }
        let ver = u32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]);
        if ver != EFR_VER {
            return Err(E_VER);
        }
        let plen = u32::from_le_bytes([blob[8], blob[9], blob[10], blob[11]]) as usize;
        if plen < LOAD_MIN || plen > LOAD_MAX || blob.len() != EFR_HDR + plen {
            return Err(E_LEN);
        }
        let fp = u64::from_le_bytes([
            blob[12], blob[13], blob[14], blob[15],
            blob[16], blob[17], blob[18], blob[19],
        ]);
        let payload = blob[EFR_HDR..].to_vec();
        if fnv1a64(&payload) != fp {
            return Err(E_FP);
        }
        if entropy_quality(&payload) < QUALITY_MIN_UNIQ {
            return Err(E_QUALITY);
        }
        Ok(EntropyFrame {
            payload,
            consumed: false,
        })
    }

    /// 消费：取 64 位熵字（从头部折叠），一次性。
    pub fn take_word(&mut self) -> Result<u64, u16> {
        if self.consumed {
            return Err(E_RDONLY);
        }
        if self.payload.len() < 8 {
            return Err(E_LEN);
        }
        self.consumed = true;
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in &self.payload {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        Ok(h)
    }
}

/// 熵账：供给/消费守恒。
pub struct EntropyLedger {
    pub frames: Vec<EntropyFrame>,
    pub consumed: usize,
}

impl EntropyLedger {
    pub fn new() -> EntropyLedger {
        EntropyLedger {
            frames: Vec::new(),
            consumed: 0,
        }
    }

    pub fn offer(&mut self, f: EntropyFrame) -> Result<(), u16> {
        self.frames.push(f);
        Ok(())
    }

    pub fn consume_one(&mut self) -> Result<u64, u16> {
        for f in self.frames.iter_mut() {
            if !f.consumed {
                let w = f.take_word()?;
                self.consumed += 1;
                return Ok(w);
            }
        }
        Err(E_BAL)
    }

    pub fn reconcile(&self) -> Result<(), u16> {
        if self.consumed > self.frames.len() {
            return Err(E_BAL);
        }
        Ok(())
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：合格熵帧全径——构造（高多样性载荷）→ 序列化 → 消费。
    let mut payload: Vec<u8> = Vec::new();
    for i in 0..64u16 {
        payload.push((i * 7 + 13) as u8);
    }
    while payload.len() < LOAD_MIN {
        payload.push(payload.len() as u8);
    }
    let f = EntropyFrame::new(payload.clone())?;
    let blob = f.serialize();
    let f2 = EntropyFrame::deserialize_and_check(&blob)?;
    let mut f2 = f2;
    let w = f2.take_word()?;
    let _ = w;
    if f2.take_word() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // R2：熵账供给/消费守恒。
    let mut led = EntropyLedger::new();
    led.offer(EntropyFrame::new(payload.clone())?)?;
    led.offer(EntropyFrame::new(payload.clone())?)?;
    led.consume_one()?;
    led.consume_one()?;
    if led.consume_one() != Err(E_BAL) {
        return Err(E_BAL);
    }
    led.reconcile()?;
    hits += 1;

    // R3：往返指纹一致。
    let blob2 = EntropyFrame::new(payload.clone())?.serialize();
    let blob3 = EntropyFrame::new(payload.clone())?.serialize();
    if blob2 != blob3 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：魔数注入 → E_MAGIC。
    let mut b1 = blob.clone();
    b1[0] = 0x58;
    if !matches!(EntropyFrame::deserialize_and_check(&b1), Err(E_MAGIC)) {
        return Err(E_MAGIC);
    }
    hits += 1;

    // I2：载荷翻转 → E_FP。
    let mut b2 = blob.clone();
    let n = b2.len();
    b2[n - 1] ^= 0xFF;
    if !matches!(EntropyFrame::deserialize_and_check(&b2), Err(E_FP)) {
        return Err(E_FP);
    }
    hits += 1;

    // I3：载荷过短 → E_LEN。
    if !matches!(EntropyFrame::new(vec![0u8; 16]), Err(E_LEN)) {
        return Err(E_LEN);
    }
    hits += 1;

    // I4：低熵质量（全同字节）→ E_QUALITY。
    let weak = vec![0xA5u8; 128];
    let weak_frame = EntropyFrame::new(weak)?;
    let wb = weak_frame.serialize();
    if !matches!(EntropyFrame::deserialize_and_check(&wb), Err(E_QUALITY)) {
        return Err(E_QUALITY);
    }
    hits += 1;

    // I5：版本越纲 → E_VER。
    let mut b5 = blob.clone();
    b5[4] = 9;
    if !matches!(EntropyFrame::deserialize_and_check(&b5), Err(E_VER)) {
        return Err(E_VER);
    }
    hits += 1;

    Ok(hits)
}
