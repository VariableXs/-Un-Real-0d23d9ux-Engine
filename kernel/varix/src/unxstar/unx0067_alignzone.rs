//! UNX-F0067 · 可用区边界对齐器 · 诊断段 0x2120
//! 验收锚点：UNX-F0067-J1 可用区间按 2^N 粒度对齐裁剪，
//! 裁剪余料记账与守恒判据通过；纯功能代码 ≥240 行。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x2120;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ALIGN: u16 = SEG_BASE + 1;    // 粒度非 2 的幂或超上限
const E_RANGE: u16 = SEG_BASE + 2;    // 区间倒序/回绕
const E_TOOSMALL: u16 = SEG_BASE + 3; // 对齐后残余小于最小可用块
const E_EMPTY: u16 = SEG_BASE + 4;    // 空区
const E_BAL: u16 = SEG_BASE + 5;      // 余料账守恒不平
const E_DUP: u16 = SEG_BASE + 6;      // 同基址区重复提交
const E_RDONLY: u16 = SEG_BASE + 7;   // 对齐器已封板
const E_ORDER: u16 = SEG_BASE + 8;    // 提交流乱序

/// 粒度上下限：4KiB .. 1GiB。
pub const GRAN_MIN: u64 = 4096;
pub const GRAN_MAX_SHIFT: u8 = 30;

/// 对齐器产出的最小可用块（默认 4KiB 页）。
pub const MIN_BLOCK: u64 = 4096;

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
        E_ALIGN => "粒度非法：必须为 2 的幂且在 4KiB..=1GiB",
        E_RANGE => "区间倒序或回绕：地址算术拦截",
        E_TOOSMALL => "对齐后残余 < 最小块：整区退化为余料",
        E_EMPTY => "空区提交：留痕拒绝",
        E_BAL => "余料守恒：原长 = 产出长 + 头余料 + 尾余料",
        E_DUP => "同基址重复提交：后到拒绝",
        E_RDONLY => "对齐器已封板：再提交属误用",
        E_ORDER => "提交流按基址乱序：升序纪律",
        _ => "未知诊断码",
    }
}

/// 粒度合法性：2 的幂 + 界内。
pub fn valid_granularity(g: u64) -> bool {
    g >= GRAN_MIN
        && g <= (1u64 << GRAN_MAX_SHIFT)
        && g & g.wrapping_sub(1) == 0
}

/// 上取整到粒度倍数。
pub fn align_up(v: u64, g: u64) -> Result<u64, u16> {
    if !valid_granularity(g) {
        return Err(E_ALIGN);
    }
    v.checked_add(g - 1)
        .map(|x| x & !(g - 1))
        .ok_or(E_RANGE)
}

/// 下取整到粒度倍数。
pub fn align_down(v: u64, g: u64) -> Result<u64, u16> {
    if !valid_granularity(g) {
        return Err(E_ALIGN);
    }
    Ok(v & !(g - 1))
}

// ── 对齐产出 ─────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlignedZone {
    pub base: u64,
    pub len: u64,     // 粒度整倍数
    pub head_waste: u64,
    pub tail_waste: u64,
}

impl AlignedZone {
    pub fn end(&self) -> u64 {
        self.base + self.len
    }
    pub fn pages(&self) -> u64 {
        self.len / MIN_BLOCK
    }
}

// ── 对齐器 ───────────────────────────────────────────────────
pub struct ZoneAligner {
    pub gran: u64,
    pub zones: Vec<AlignedZone>,
    pub waste_total: u64,
    pub submitted: Vec<(u64, u64)>,
    pub sealed: bool,
}

impl ZoneAligner {
    pub fn new(gran: u64) -> Result<ZoneAligner, u16> {
        if !valid_granularity(gran) {
            return Err(E_ALIGN);
        }
        Ok(ZoneAligner {
            gran,
            zones: Vec::new(),
            waste_total: 0,
            submitted: Vec::new(),
            sealed: false,
        })
    }

    /// 提交原始可用区：升序纪律 + 对齐裁剪 + 余料记账。
    pub fn submit(&mut self, raw_base: u64, raw_len: u64) -> Result<Option<AlignedZone>, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if raw_len == 0 {
            return Err(E_EMPTY);
        }
        let raw_end = raw_base.checked_add(raw_len).ok_or(E_RANGE)?;
        if let Some((b, _)) = self.submitted.last() {
            if raw_base < *b {
                return Err(E_ORDER);
            }
        }
        if self.submitted.iter().any(|(b, _)| *b == raw_base) {
            return Err(E_DUP);
        }
        self.submitted.push((raw_base, raw_len));

        let a_base = align_up(raw_base, self.gran)?;
        let a_end = align_down(raw_end, self.gran)?;
        if a_end <= a_base {
            // 全区退化为余料
            self.waste_total += raw_len;
            return Ok(None);
        }
        let head = a_base - raw_base;
        let tail = raw_end - a_end;
        let len = a_end - a_base;
        // 产出必须为最小块整数倍（粒度是 2^N 且 ≥ 4KiB 时自动满足）
        if len % MIN_BLOCK != 0 {
            self.waste_total += raw_len;
            return Ok(None);
        }
        self.waste_total += head + tail;
        let z = AlignedZone {
            base: a_base,
            len,
            head_waste: head,
            tail_waste: tail,
        };
        self.zones.push(z);
        Ok(Some(z))
    }

    /// 封板：余料守恒总审。
    pub fn seal(&mut self) -> Result<(u64, u64), u16> {
        let raw_total: u64 = self.submitted.iter().map(|(_, l)| l).sum();
        let out_total: u64 = self.zones.iter().map(|z| z.len).sum();
        if raw_total != out_total + self.waste_total {
            return Err(E_BAL);
        }
        self.sealed = true;
        Ok((out_total, self.waste_total))
    }

    /// 可分配总页数。
    pub fn total_pages(&self) -> u64 {
        self.zones.iter().map(|z| z.pages()).sum()
    }

    /// 基址对齐率：产区内基址符合粒度整倍数的占比（应恒 1）。
    pub fn alignment_ratio_permille(&self) -> u64 {
        if self.zones.is_empty() {
            return 0;
        }
        let ok = self
            .zones
            .iter()
            .filter(|z| z.base % self.gran == 0)
            .count();
        (ok * 1000 / self.zones.len()) as u64
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.gran.to_le_bytes());
        for z in &self.zones {
            buf.extend_from_slice(&z.base.to_le_bytes());
            buf.extend_from_slice(&z.len.to_le_bytes());
            buf.extend_from_slice(&z.head_waste.to_le_bytes());
            buf.extend_from_slice(&z.tail_waste.to_le_bytes());
        }
        buf.extend_from_slice(&self.waste_total.to_le_bytes());
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：2MiB 粒度对齐——头尾余料精确裁剪。
    let mut a = ZoneAligner::new(0x20_0000)?;
    let z1 = a.submit(0x0010_0123, 0x100_0000)?; // 头余料到 0x20_0000，尾裁到粒度
    let z = z1.ok_or(E_TOOSMALL)?;
    if z.base != 0x20_0000 {
        return Err(E_ALIGN);
    }
    if z.len % 0x20_0000 != 0 {
        return Err(E_ALIGN);
    }
    if z.head_waste != 0x20_0000 - 0x10_0123 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：守恒封板——原长 = 产出 + 余料。
    a.submit(0x101_0000, 0x30_0000)?; // 起点已 2MiB 对齐？0x101_0000 % 0x20_0000 = 0x1_0000 → 头余料
    let (out, waste) = a.seal()?;
    let raw: u64 = 0x100_0000 + 0x30_0000;
    if out + waste != raw {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：全区退化（区间小于粒度）→ None + 全余料。
    let mut a2 = ZoneAligner::new(0x20_0000)?;
    if a2.submit(0x2000_0000, 0x1_0000)?.is_some() {
        return Err(E_TOOSMALL);
    }
    let (out2, waste2) = a2.seal()?;
    if out2 != 0 || waste2 != 0x1_0000 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：基址对齐率恒千分之千。
    let mut a3 = ZoneAligner::new(0x1000)?;
    a3.submit(0x1234_5678, 0x10_0000)?;
    a3.submit(0x2234_0000, 0x10_0000)?;
    if a3.alignment_ratio_permille() != 1000 {
        return Err(E_ALIGN);
    }
    if a3.total_pages() != 0x10_0000 / 4096 * 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：粒度非 2 的幂 → E_ALIGN。
    if ZoneAligner::new(0x3000).is_ok() {
        return Err(E_ALIGN);
    }
    hits += 1;

    // I2：粒度越上界 → E_ALIGN。
    if ZoneAligner::new(1u64 << 31).is_ok() {
        return Err(E_ALIGN);
    }
    hits += 1;

    // I3：提交流乱序 → E_ORDER。
    let mut i3 = ZoneAligner::new(0x1000)?;
    i3.submit(0x5000, 0x1000)?;
    if i3.submit(0x4000, 0x1000) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // I4：封板后再提交 → E_RDONLY。
    let mut i4 = ZoneAligner::new(0x1000)?;
    i4.submit(0x1000, 0x2000)?;
    i4.seal()?;
    if i4.submit(0x8000, 0x1000) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
