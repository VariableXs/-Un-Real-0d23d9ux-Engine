//! UNX-F0069 · 物理内存热插拔位图预留 · 诊断段 0x2140
//! 验收锚点：UNX-F0069-J1 热插拔窗口按位图预留/释放，
//! 位图账守恒与双缴获（重复释放/重复预留）判据通过；
//! 纯功能代码 ≥220 行。零浮点、零 unwrap、零 expect。


use crate::alloc::{vec, vec::Vec};

pub const SEG_BASE: u16 = 0x2140;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_RANGE: u16 = SEG_BASE + 1;     // 页号越出位图域
const E_TWICE_ON: u16 = SEG_BASE + 2;  // 位已预留再预留
const E_TWICE_OFF: u16 = SEG_BASE + 3; // 位已空闲再释放
const E_ORDER: u16 = SEG_BASE + 4;     // 插拔窗口倒序
const E_RDONLY: u16 = SEG_BASE + 5;    // 位图已封板
const E_BAL: u16 = SEG_BASE + 6;       // 位账守恒不平
const E_ALIGN: u16 = SEG_BASE + 7;     // 窗口未按对齐粒度（插拔以区为单位）
const E_EMPTY: u16 = SEG_BASE + 8;     // 空位图域

/// 插拔窗口对齐粒度：128MiB（ACPI 惯例）。
pub const WINDOW_GRAN: u64 = 0x0800_0000;

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
        E_RANGE => "页号越出位图域：先扩域再置位",
        E_TWICE_ON => "位已预留再预留：双缴获拦截",
        E_TWICE_OFF => "位已空闲再释放：双缴获拦截",
        E_ORDER => "窗口倒序：起点必须 ≤ 终点",
        E_RDONLY => "位图已封板：再操作属误用",
        E_BAL => "位账守恒：置位数 + 清位数 = 操作总数",
        E_ALIGN => "窗口未按 128MiB 对齐：插拔以区为单位",
        E_EMPTY => "空位图域：先声明域",
        _ => "未知诊断码",
    }
}

/// 动态位图（Vec<u64> 字字域）。
pub struct BitMap {
    words: Vec<u64>,
    pub bits: usize,
}

impl BitMap {
    pub fn new(bits: usize) -> BitMap {
        let words = (bits + 63) / 64;
        BitMap {
            words: vec![0u64; words],
            bits,
        }
    }

    pub fn get(&self, i: usize) -> Result<bool, u16> {
        if i >= self.bits {
            return Err(E_RANGE);
        }
        Ok(self.words[i / 64] & (1u64 << (i % 64)) != 0)
    }

    pub fn set(&mut self, i: usize) -> Result<(), u16> {
        if i >= self.bits {
            return Err(E_RANGE);
        }
        if self.words[i / 64] & (1u64 << (i % 64)) != 0 {
            return Err(E_TWICE_ON);
        }
        self.words[i / 64] |= 1u64 << (i % 64);
        Ok(())
    }

    pub fn clear(&mut self, i: usize) -> Result<(), u16> {
        if i >= self.bits {
            return Err(E_RANGE);
        }
        if self.words[i / 64] & (1u64 << (i % 64)) == 0 {
            return Err(E_TWICE_OFF);
        }
        self.words[i / 64] &= !(1u64 << (i % 64));
        Ok(())
    }

    pub fn popcount(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }
}

// ── 热插拔账 ─────────────────────────────────────────────────
pub struct HotPlugLedger {
    pub bmp: BitMap,
    pub page_base: u64,   // 位图域起始物理页号
    pub ops_on: usize,
    pub ops_off: usize,
    pub sealed: bool,
}

impl HotPlugLedger {
    /// 声明域：[phys_base, phys_base + pages*4096)，页对齐。
    pub fn new(phys_base: u64, pages: usize) -> Result<HotPlugLedger, u16> {
        if pages == 0 {
            return Err(E_EMPTY);
        }
        if phys_base % 4096 != 0 {
            return Err(E_ALIGN);
        }
        Ok(HotPlugLedger {
            bmp: BitMap::new(pages),
            page_base: phys_base / 4096,
            ops_on: 0,
            ops_off: 0,
            sealed: false,
        })
    }

    fn idx_of(&self, phys: u64) -> Result<usize, u16> {
        if phys % 4096 != 0 {
            return Err(E_ALIGN);
        }
        let p = phys / 4096;
        if p < self.page_base {
            return Err(E_RANGE);
        }
        let d = (p - self.page_base) as usize;
        if d >= self.bmp.bits {
            return Err(E_RANGE);
        }
        Ok(d)
    }

    /// 预留一页（进入热插拔窗口）。
    pub fn reserve_page(&mut self, phys: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        let i = self.idx_of(phys)?;
        self.bmp.set(i)?;
        self.ops_on += 1;
        Ok(())
    }

    /// 释放一页（退出热插拔窗口，归还页分配器）。
    pub fn release_page(&mut self, phys: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        let i = self.idx_of(phys)?;
        self.bmp.clear(i)?;
        self.ops_off += 1;
        Ok(())
    }

    /// 预留整窗口：窗口必须 128MiB 对齐（起点与跨度）。
    pub fn reserve_window(&mut self, start: u64, len: u64) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if len == 0 || start.checked_add(len).is_none() {
            return Err(E_ORDER);
        }
        if start % WINDOW_GRAN != 0 || len % WINDOW_GRAN != 0 {
            return Err(E_ALIGN);
        }
        let mut n = 0usize;
        let mut p = start;
        while p < start + len {
            self.reserve_page(p)?;
            n += 1;
            p += 4096;
        }
        Ok(n)
    }

    /// 批量释放窗口。
    pub fn release_window(&mut self, start: u64, len: u64) -> Result<usize, u16> {
        let mut n = 0usize;
        let mut p = start;
        while p < start + len {
            self.release_page(p)?;
            n += 1;
            p += 4096;
        }
        Ok(n)
    }

    /// 账守恒：当前置位数 = on - off；操作账 = on + off。
    pub fn reconcile(&self) -> Result<(), u16> {
        if self.bmp.popcount() as i64 != self.ops_on as i64 - self.ops_off as i64 {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 已预留窗口清单（连续段折叠）。
    pub fn reserved_windows(&self) -> Vec<(u64, u64)> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.bmp.bits {
            match self.bmp.get(i) {
                Ok(true) => {
                    let start_i = i;
                    while i < self.bmp.bits && self.bmp.get(i) == Ok(true) {
                        i += 1;
                    }
                    let s = self.page_base + start_i as u64;
                    let e = self.page_base + i as u64;
                    out.push((s * 4096, (e - s) * 4096));
                }
                _ => i += 1,
            }
        }
        out
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.page_base.to_le_bytes());
        for w in &self.bmp.words {
            buf.extend_from_slice(&w.to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：窗口预留（2×128MiB）→ 连续段折叠为一段。
    let mut led = HotPlugLedger::new(0x1000_0000, 0x8000)?; // 128MiB 域 = 32768 页
    let n = led.reserve_window(0x1000_0000, WINDOW_GRAN)?;
    if n != 0x8000 {
        return Err(E_BAL);
    }
    let wins = led.reserved_windows();
    if wins.len() != 1 || wins[0] != (0x1000_0000, WINDOW_GRAN) {
        return Err(E_BAL);
    }
    led.reconcile()?;
    hits += 1;

    // R2：窗口半释放 → 账守恒、段折叠更新。
    let half = WINDOW_GRAN / 2;
    let m = led.release_window(0x1000_0000, half)?;
    if m != 0x4000 {
        return Err(E_BAL);
    }
    led.reconcile()?;
    let wins2 = led.reserved_windows();
    if wins2.len() != 1 || wins2[0] != (0x1000_0000 + half, half) {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：指纹对拍。
    let led2 = HotPlugLedger::new(0x1000_0000, 0x8000)?;
    let mut led3 = HotPlugLedger::new(0x1000_0000, 0x8000)?;
    led3.reserve_page(0x1000_0000)?;
    let _ = led2;
    if led3.fingerprint() == led.fingerprint() {
        return Err(E_BAL); // 不同操作序列指纹必须不同
    }
    hits += 1;

    // I1：重复预留 → E_TWICE_ON。
    let mut i1 = HotPlugLedger::new(0x2000_0000, 16)?;
    i1.reserve_page(0x2000_0000)?;
    if i1.reserve_page(0x2000_0000) != Err(E_TWICE_ON) {
        return Err(E_TWICE_ON);
    }
    hits += 1;

    // I2：重复释放 → E_TWICE_OFF。
    let mut i2 = HotPlugLedger::new(0x2000_0000, 16)?;
    i2.reserve_page(0x2000_1000)?;
    i2.release_page(0x2000_1000)?;
    if i2.release_page(0x2000_1000) != Err(E_TWICE_OFF) {
        return Err(E_TWICE_OFF);
    }
    hits += 1;

    // I3：越域 → E_RANGE。
    let mut i3 = HotPlugLedger::new(0x2000_0000, 16)?;
    if i3.reserve_page(0x2100_0000) != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    hits += 1;

    // I4：窗口未按 128MiB 对齐 → E_ALIGN。
    let mut i4 = HotPlugLedger::new(0x3000_0000, 0x8000)?;
    if i4.reserve_window(0x3000_0000, 0x1000) != Err(E_ALIGN) {
        return Err(E_ALIGN);
    }
    hits += 1;

    // I5：域内非页对齐地址 → E_ALIGN。
    let mut i5 = HotPlugLedger::new(0x3000_0000, 16)?;
    if i5.reserve_page(0x3000_0804) != Err(E_ALIGN) {
        return Err(E_ALIGN);
    }
    hits += 1;

    Ok(hits)
}
