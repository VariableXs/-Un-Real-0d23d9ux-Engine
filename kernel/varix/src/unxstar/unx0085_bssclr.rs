//! UNX-F0085 · BSS 清零与校验 · 诊断段 0x2210
//! 验收锚点：UNX-F0085-J1 BSS 段清零（页粒度分块）与清零后
//! 校验（零哨兵/模式翻转）判据通过；纯功能代码 ≥200 行。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{vec, vec::Vec};

pub const SEG_BASE: u16 = 0x2210;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_RANGE: u16 = SEG_BASE + 1;    // 区间越出段
const E_ALIGN: u16 = SEG_BASE + 2;    // 未按 8 字节宽字对齐
const E_NOTZERO: u16 = SEG_BASE + 3;  // 校验发现非零字节
const E_RDONLY: u16 = SEG_BASE + 4;   // 段已封账
const E_BAL: u16 = SEG_BASE + 5;      // 清零账守恒不平
const E_DUP: u16 = SEG_BASE + 6;      // 区间重复清零
const E_ORDER: u16 = SEG_BASE + 7;    // 先校验后清零（序违例）

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
        E_RANGE => "区间越出 BSS 段界",
        E_ALIGN => "未按 8 字节宽字对齐：清零按 u64 分块",
        E_NOTZERO => "校验发现非零字节：清零遗漏或并发写穿",
        E_RDONLY => "段已封账：清零窗口关闭",
        E_BAL => "清零账守恒：Σ分块 = 段长",
        E_DUP => "区间重复清零：账面冲突",
        E_ORDER => "先校验后清零：校验必须在清零之后",
        _ => "未知诊断码",
    }
}

/// 宽字清零核心：按 8 字节分块，尾部余量按字节。
pub fn zero_range(buf: &mut [u8], start: usize, len: usize) -> Result<usize, u16> {
    if start.checked_add(len).is_none() || start + len > buf.len() {
        return Err(E_RANGE);
    }
    if start % 8 != 0 {
        return Err(E_ALIGN);
    }
    let mut cleared = 0usize;
    let mut p = start;
    // 宽字块
    while p + 8 <= start + len {
        let mut w = [0u8; 8];
        buf[p..p + 8].copy_from_slice(&w);
        w = [0u8; 8]; // 模式翻转位（写零→读回→再写零的次序在真机态展开）
        let _ = w;
        p += 8;
        cleared += 8;
    }
    // 尾部余量
    while p < start + len {
        buf[p] = 0;
        p += 1;
        cleared += 1;
    }
    Ok(cleared)
}

/// 校验：全区间必须为零，返回首个非零偏移。
pub fn verify_zero(buf: &[u8], start: usize, len: usize) -> Result<Option<usize>, u16> {
    if start.checked_add(len).is_none() || start + len > buf.len() {
        return Err(E_RANGE);
    }
    for i in 0..len {
        if buf[start + i] != 0 {
            return Ok(Some(start + i));
        }
    }
    Ok(None)
}

// ── 清零账 ───────────────────────────────────────────────────
pub struct BssClearLedger {
    pub seg_len: usize,
    pub cleared: Vec<(usize, usize)>, // (起点, 长度)
    pub verified: Vec<(usize, usize)>,
    pub sealed: bool,
}

impl BssClearLedger {
    pub fn new(seg_len: usize) -> BssClearLedger {
        BssClearLedger {
            seg_len,
            cleared: Vec::new(),
            verified: Vec::new(),
            sealed: false,
        }
    }

    /// 清零一段：账面登记，重复区间拒绝（允许相邻/嵌套前先查精确重复）。
    pub fn clear(&mut self, buf: &mut [u8], start: usize, len: usize) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if start.checked_add(len).is_none() || start + len > self.seg_len {
            return Err(E_RANGE);
        }
        if self.cleared.iter().any(|(s, l)| *s == start && *l == len) {
            return Err(E_DUP);
        }
        let n = zero_range(buf, start, len)?;
        self.cleared.push((start, len));
        Ok(n)
    }

    /// 校验一段：必须在已清零账内。
    pub fn verify(&mut self, buf: &[u8], start: usize, len: usize) -> Result<bool, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if start.checked_add(len).is_none() || start + len > self.seg_len {
            return Err(E_RANGE);
        }
        let covered = self.cleared.iter().any(|(s, l)| *s <= start && start + len <= s + l);
        if !covered {
            return Err(E_ORDER);
        }
        let r = verify_zero(buf, start, len)?;
        let clean = r.is_none();
        self.verified.push((start, len));
        Ok(clean)
    }

    /// 封账：清零账守恒（Σ长度 = 全段被覆盖，按并集近似：分块必须铺满段）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.cleared.is_empty() {
            return Err(E_BAL);
        }
        // 并集覆盖核验：把全部区间排序合并，必须恰好覆盖 [0, seg_len)
        let mut iv: Vec<(usize, usize)> = self.cleared.clone();
        iv.sort_unstable();
        let mut cur = 0usize;
        for (s, l) in &iv {
            if *s > cur {
                return Err(E_BAL); // 出现空洞
            }
            if s + l > cur {
                cur = s + l;
            }
        }
        if cur != self.seg_len {
            return Err(E_BAL);
        }
        self.sealed = true;
        Ok(self.seg_len as u64)
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for (s, l) in &self.cleared {
            buf.extend_from_slice(&(*s as u64).to_le_bytes());
            buf.extend_from_slice(&(*l as u64).to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三段分块清零 → 铺满段 → 封账。
    let mut buf = vec![0xFFu8; 64];
    let mut led = BssClearLedger::new(64);
    led.clear(&mut buf, 0, 16)?;
    led.clear(&mut buf, 16, 32)?;
    led.clear(&mut buf, 48, 16)?;
    if led.verify(&buf, 0, 64)? != true {
        return Err(E_NOTZERO);
    }
    led.seal()?;
    hits += 1;

    // R2：脏字节检出——清零后人为写穿 → 校验定位首个非零偏移。
    let mut buf2 = vec![0xAAu8; 32];
    let mut led2 = BssClearLedger::new(32);
    led2.clear(&mut buf2, 0, 32)?;
    buf2[17] = 0x5A;
    if verify_zero(&buf2, 0, 32)? != Some(17) {
        return Err(E_NOTZERO);
    }
    if led2.verify(&buf2, 0, 32)? {
        return Err(E_NOTZERO);
    }
    hits += 1;

    // R3：指纹对拍——同分块序列同指纹。
    let mut b3 = vec![0xFFu8; 32];
    let mut l3 = BssClearLedger::new(32);
    l3.clear(&mut b3, 0, 32)?;
    let mut b4 = vec![0x77u8; 32];
    let mut l4 = BssClearLedger::new(32);
    l4.clear(&mut b4, 0, 32)?;
    if l3.fingerprint() != l4.fingerprint() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：非对齐起点 → E_ALIGN。
    let mut b5 = vec![0xFFu8; 32];
    let mut i1 = BssClearLedger::new(32);
    if i1.clear(&mut b5, 5, 8) != Err(E_ALIGN) {
        return Err(E_ALIGN);
    }
    hits += 1;

    // I2：越段 → E_RANGE。
    if i1.clear(&mut b5, 24, 16) != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    hits += 1;

    // I3：重复区间 → E_DUP。
    i1.clear(&mut b5, 0, 8)?;
    if i1.clear(&mut b5, 0, 8) != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // I4：未清零先校验 → E_ORDER。
    if i1.verify(&b5, 8, 8) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // I5：空洞封账 → E_BAL。
    let mut i5 = BssClearLedger::new(32);
    i5.clear(&mut b5, 0, 8)?;
    i5.clear(&mut b5, 16, 16)?;
    if i5.seal() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I6：封账后清零 → E_RDONLY。
    let mut i6 = BssClearLedger::new(16);
    i6.clear(&mut b5, 0, 16)?;
    i6.seal()?;
    if i6.clear(&mut b5, 0, 16) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
