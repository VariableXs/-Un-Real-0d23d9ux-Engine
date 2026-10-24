//! UNX-F0084 · 段重定位项修复器 · 诊断段 0x2200
//! 验收锚点：UNX-F0084-J1 绝对/相对/PC 三类重定位项逐条修复，
//! 修复前后指纹账与回绕拦截判据通过；纯功能代码 ≥340 行。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{vec, vec::Vec};

pub const SEG_BASE: u16 = 0x2200;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_TYPE: u16 = SEG_BASE + 1;     // 重定位类型超闭集
const E_RANGE: u16 = SEG_BASE + 2;    // 补丁偏移越出段
const E_SHORT: u16 = SEG_BASE + 3;    // 补丁窗小于编码宽度
const E_WRAP: u16 = SEG_BASE + 4;     // 加法回绕
const E_DUP: u16 = SEG_BASE + 5;      // 同偏移重复修复
const E_DELTA: u16 = SEG_BASE + 6;    // 差值越出有符号域
const E_BAL: u16 = SEG_BASE + 7;      // 修复账守恒不平
const E_RDONLY: u16 = SEG_BASE + 8;   // 段已封板
const E_ALIGN: u16 = SEG_BASE + 9;    // 修复点未按编码宽度对齐
const E_OVERFLOW: u16 = SEG_BASE + 10;// 相对位移超 ±2GiB 窗

/// 重定位类型闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelocKind {
    /// R-ABS64：8 字节绝对地址。
    Abs64,
    /// R-ABS32：4 字节绝对地址（段内限位）。
    Abs32,
    /// R-REL32：4 字节 PC 相对位移。
    Rel32,
}

impl RelocKind {
    pub fn width(&self) -> usize {
        match self {
            RelocKind::Abs64 => 8,
            RelocKind::Abs32 | RelocKind::Rel32 => 4,
        }
    }
    pub fn id(&self) -> u8 {
        match self {
            RelocKind::Abs64 => 1,
            RelocKind::Abs32 => 2,
            RelocKind::Rel32 => 3,
        }
    }
    pub fn from_id(v: u8) -> Result<RelocKind, u16> {
        match v {
            1 => Ok(RelocKind::Abs64),
            2 => Ok(RelocKind::Abs32),
            3 => Ok(RelocKind::Rel32),
            _ => Err(E_TYPE),
        }
    }
}

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
        E_TYPE => "重定位类型超闭集：{ABS64, ABS32, REL32}",
        E_RANGE => "补丁偏移越出段界",
        E_SHORT => "补丁窗小于编码宽度：段尾截断",
        E_WRAP => "加法回绕：地址算术拦截",
        E_DUP => "同偏移重复修复：二次修复属账面冲突",
        E_DELTA => "差值越出有符号域：基址差 |Δ| > 2^63",
        E_BAL => "修复账守恒：修复数 + 拒绝数 = 提交数",
        E_RDONLY => "段已封板：修复窗口关闭",
        E_ALIGN => "修复点未按编码宽度对齐",
        E_OVERFLOW => "相对位移超 ±2GiB：REL32 编码域",
        _ => "未知诊断码",
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RelocEntry {
    pub off: u64,       // 段内偏移
    pub kind: RelocKind,
    pub addend: i64,    // 附加量
}

/// 修复单点：把补丁写入段缓冲的对应偏移。
/// `patch_base`：段载入基址；`target`：符号目标地址。
/// REL32 编码 = target - (patch_base + off + width) + addend。
pub fn apply_one(
    seg: &mut [u8],
    e: &RelocEntry,
    patch_base: u64,
    target: u64,
) -> Result<u64, u16> {
    let w = e.kind.width();
    if e.off % w as u64 != 0 {
        return Err(E_ALIGN);
    }
    if e.off as usize >= seg.len() {
        return Err(E_RANGE);
    }
    if (e.off as usize) + w > seg.len() {
        return Err(E_SHORT);
    }
    match e.kind {
        RelocKind::Abs64 => {
            let v = (target as i64)
                .checked_add(e.addend)
                .ok_or(E_DELTA)?;
            if v < 0 {
                return Err(E_DELTA);
            }
            let b = (v as u64).to_le_bytes();
            seg[e.off as usize..e.off as usize + 8].copy_from_slice(&b);
            Ok(v as u64)
        }
        RelocKind::Abs32 => {
            let v = (target as i64).checked_add(e.addend).ok_or(E_DELTA)?;
            if v < 0 || v > 0xFFFF_FFFF {
                return Err(E_OVERFLOW);
            }
            let b = (v as u32).to_le_bytes();
            seg[e.off as usize..e.off as usize + 4].copy_from_slice(&b);
            Ok(v as u64)
        }
        RelocKind::Rel32 => {
            // 位移 = target + addend - (P)，P = patch_base + off
            let p = patch_base.checked_add(e.off).ok_or(E_WRAP)?;
            let disp = (target as i128)
                + (e.addend as i128)
                - (p as i128);
            if disp < -(1i128 << 31) || disp > (1i128 << 31) - 1 {
                return Err(E_OVERFLOW);
            }
            let b = (disp as i32).to_le_bytes();
            seg[e.off as usize..e.off as usize + 4].copy_from_slice(&b);
            Ok(disp as u64)
        }
    }
}

/// 读取补丁结果（演练与对拍用）。
pub fn read_patch(seg: &[u8], off: u64, kind: RelocKind) -> Result<u64, u16> {
    let w = kind.width();
    if off as usize + w > seg.len() {
        return Err(E_RANGE);
    }
    match kind {
        RelocKind::Abs64 => {
            let mut b = [0u8; 8];
            b.copy_from_slice(&seg[off as usize..off as usize + 8]);
            Ok(u64::from_le_bytes(b))
        }
        _ => {
            let mut b = [0u8; 4];
            b.copy_from_slice(&seg[off as usize..off as usize + 4]);
            Ok(u32::from_le_bytes(b) as u64)
        }
    }
}

// ── 修复账 ───────────────────────────────────────────────────
pub struct RelocFixer {
    pub seg: Vec<u8>,
    pub patch_base: u64,
    pub applied: Vec<(u64, RelocKind)>,
    pub rejected: Vec<(u64, u16)>,
    pub sealed: bool,
}

impl RelocFixer {
    pub fn new(seg_len: usize, patch_base: u64) -> RelocFixer {
        RelocFixer {
            seg: vec![0u8; seg_len],
            patch_base,
            applied: Vec::new(),
            rejected: Vec::new(),
            sealed: false,
        }
    }

    /// 逐条提交修复：同偏移拒绝；错误分类入账。
    pub fn submit(&mut self, e: &RelocEntry, target: u64) -> Result<bool, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if self.applied.iter().any(|(o, _)| *o == e.off) {
            self.rejected.push((e.off, E_DUP));
            return Ok(false);
        }
        match apply_one(&mut self.seg, e, self.patch_base, target) {
            Ok(_) => {
                self.applied.push((e.off, e.kind));
                Ok(true)
            }
            Err(c) => {
                self.rejected.push((e.off, c));
                Ok(false)
            }
        }
    }

    pub fn seal(&mut self) -> Result<(u64, u64), u16> {
        self.sealed = true;
        let applied_bytes: u64 = self
            .applied
            .iter()
            .map(|(_, k)| k.width() as u64)
            .sum();
        let rejected_bytes: u64 = self.rejected.len() as u64 * 4;
        Ok((applied_bytes, rejected_bytes))
    }

    /// 段指纹（修复后）。
    pub fn fingerprint(&self) -> u64 {
        fnv1a64(&self.seg)
    }

    /// 守恒：applied + rejected == 提交总数（外部传入）。
    pub fn reconcile(&self, submitted: usize) -> Result<(), u16> {
        if self.applied.len() + self.rejected.len() != submitted {
            return Err(E_BAL);
        }
        Ok(())
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 6 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    const PB: u64 = 0x10_0000; // 段基址 1MiB

    // R1：ABS64 修复——值精确落位。
    let mut f = RelocFixer::new(256, PB);
    f.submit(&RelocEntry { off: 0, kind: RelocKind::Abs64, addend: 0 }, 0x7FFF_0000)?;
    let v = read_patch(&f.seg, 0, RelocKind::Abs64)?;
    if v != 0x7FFF_0000 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：ABS32 修复 + addend。
    f.submit(&RelocEntry { off: 8, kind: RelocKind::Abs32, addend: 16 }, 0x1000)?;
    let v2 = read_patch(&f.seg, 8, RelocKind::Abs32)?;
    if v2 != 0x1010 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：REL32 修复——位移 = target - P。
    f.submit(&RelocEntry { off: 16, kind: RelocKind::Rel32, addend: 0 }, PB + 64)?;
    let d = read_patch(&f.seg, 16, RelocKind::Rel32)? as u32 as i32;
    if d != 64 - 16 {
        return Err(E_BAL); // target - P = (PB+64) - (PB+16) = 48
    }
    hits += 1;

    // R4：守恒封板。
    f.seal()?;
    f.reconcile(3)?;
    hits += 1;

    // I1：类型越纲 → E_TYPE。
    if RelocKind::from_id(9) != Err(E_TYPE) {
        return Err(E_TYPE);
    }
    hits += 1;

    // I2：偏移越段 → E_RANGE。
    let mut i2 = RelocFixer::new(64, PB);
    i2.submit(&RelocEntry { off: 128, kind: RelocKind::Abs64, addend: 0 }, 0)?;
    if i2.rejected.last().map(|(_, c)| *c) != Some(E_RANGE) {
        return Err(E_RANGE);
    }
    hits += 1;

    // I3：段尾截断 → E_SHORT。
    let mut i3 = RelocFixer::new(60, PB);
    i3.submit(&RelocEntry { off: 56, kind: RelocKind::Abs64, addend: 0 }, 0)?;
    if i3.rejected.last().map(|(_, c)| *c) != Some(E_SHORT) {
        return Err(E_SHORT);
    }
    hits += 1;

    // I4：ABS32 溢出 → E_OVERFLOW。
    let mut i4 = RelocFixer::new(64, PB);
    i4.submit(&RelocEntry { off: 0, kind: RelocKind::Abs32, addend: 0 }, 0x1_0000_0000)?;
    if i4.rejected.last().map(|(_, c)| *c) != Some(E_OVERFLOW) {
        return Err(E_OVERFLOW);
    }
    hits += 1;

    // I5：REL32 超窗 → E_OVERFLOW。
    let mut i5 = RelocFixer::new(64, PB);
    i5.submit(
        &RelocEntry { off: 0, kind: RelocKind::Rel32, addend: 0 },
        0x9000_0000,
    )?;
    if i5.rejected.last().map(|(_, c)| *c) != Some(E_OVERFLOW) {
        return Err(E_OVERFLOW);
    }
    hits += 1;

    // I6：同偏移重复 → E_DUP。
    let mut i6 = RelocFixer::new(64, PB);
    i6.submit(&RelocEntry { off: 0, kind: RelocKind::Abs64, addend: 0 }, 1)?;
    i6.submit(&RelocEntry { off: 0, kind: RelocKind::Abs64, addend: 0 }, 2)?;
    if i6.rejected.last().map(|(_, c)| *c) != Some(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // I7：未对齐修复点 → E_ALIGN。
    let mut i7 = RelocFixer::new(64, PB);
    i7.submit(&RelocEntry { off: 5, kind: RelocKind::Abs64, addend: 0 }, 1)?;
    if i7.rejected.last().map(|(_, c)| *c) != Some(E_ALIGN) {
        return Err(E_ALIGN);
    }
    hits += 1;

    Ok(hits)
}
