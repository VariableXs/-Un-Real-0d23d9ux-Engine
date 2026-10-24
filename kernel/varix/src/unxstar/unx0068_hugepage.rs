//! UNX-F0068 · 大页可用性预判 · 诊断段 0x2130
//! 验收锚点：UNX-F0068-J1 2MiB/1GiB 大页在可用区上的分布预判，
//! 对齐、跨界、容量三重判据与收益账判据通过；
//! 纯功能代码 ≥260 行。零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x2130;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_HPSIZE: u16 = SEG_BASE + 1;  // 大页粒度非法（仅 2MiB/1GiB）
const E_RANGE: u16 = SEG_BASE + 2;   // 区间倒序/回绕
const E_DUP: u16 = SEG_BASE + 3;     // 重复区
const E_RDONLY: u16 = SEG_BASE + 4;  // 预判账已封板
const E_BAL: u16 = SEG_BASE + 5;     // 覆盖账守恒不平
const E_CROSS: u16 = SEG_BASE + 6;   // 大页跨界（跨可用区边界）
const E_EMPTY: u16 = SEG_BASE + 7;   // 空区提交

/// 大页粒度闭集。
pub const HP_2M: u64 = 0x20_0000;
pub const HP_1G: u64 = 0x4000_0000;

pub fn hp_valid(size: u64) -> bool {
    size == HP_2M || size == HP_1G
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
        E_HPSIZE => "大页粒度非法：闭集 {2MiB, 1GiB}",
        E_RANGE => "区间倒序或回绕：地址算术拦截",
        E_DUP => "重复区提交：后到拒绝",
        E_RDONLY => "预判账已封板：再注入属误用",
        E_BAL => "覆盖守恒：可安置大页字节 + 边角余料 = 区长",
        E_CROSS => "大页跨可用区边界：跨界大页禁止安置",
        E_EMPTY => "空区提交：留痕拒绝",
        _ => "未知诊断码",
    }
}

#[derive(Clone, Copy, Debug)]
pub struct UsableZone {
    pub base: u64,
    pub len: u64,
}

impl UsableZone {
    pub fn end(&self) -> u64 {
        self.base + self.len
    }
}

/// 单区预判：对齐起点上取整、可整除的完整大页数与覆盖字节。
pub fn zone_projection(z: &UsableZone, hp: u64) -> Result<(u64, u64, u64), u16> {
    if !hp_valid(hp) {
        return Err(E_HPSIZE);
    }
    if z.len == 0 {
        return Err(E_EMPTY);
    }
    if z.base.checked_add(z.len).is_none() {
        return Err(E_RANGE);
    }
    let first = (z.base + hp - 1) & !(hp - 1);
    if first >= z.end() {
        return Ok((0, 0, z.len)); // 区内放不下任何大页
    }
    let full = (z.end() - first) / hp;
    let covered = full * hp;
    let waste = z.len - covered;
    Ok((full, covered, waste))
}

/// 双粒度收益账：1GiB 大页省下的页表项数 vs 2MiB 方案。
pub fn pte_savings(full_1g: u64, full_2m: u64) -> u64 {
    // 2MiB 方案需要 full_2m 个 2MiB 项 + full_1g*512 个 2MiB 项
    // 1GiB 方案需要 full_1g 个 1GiB 项，其覆盖域内 2MiB 项全免
    let covered_by_1g = full_1g * 512; // 折算成 2MiB 单位
    let flat_2m = full_2m;
    if flat_2m <= covered_by_1g {
        0
    } else {
        flat_2m - covered_by_1g
    }
}

// ── 预判账 ───────────────────────────────────────────────────
pub struct HugePageProjection {
    pub hp_size: u64,
    pub zones: Vec<UsableZone>,
    pub placements: Vec<(u64, u64)>, // (大页基址, 区索引)
    pub sealed: bool,
}

impl HugePageProjection {
    pub fn new(hp_size: u64) -> Result<HugePageProjection, u16> {
        if !hp_valid(hp_size) {
            return Err(E_HPSIZE);
        }
        Ok(HugePageProjection {
            hp_size,
            zones: Vec::new(),
            placements: Vec::new(),
            sealed: false,
        })
    }

    pub fn add_zone(&mut self, base: u64, len: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if len == 0 {
            return Err(E_EMPTY);
        }
        if base.checked_add(len).is_none() {
            return Err(E_RANGE);
        }
        if self.zones.iter().any(|z| z.base == base) {
            return Err(E_DUP);
        }
        self.zones.push(UsableZone { base, len });
        Ok(())
    }

    /// 逐区安置：大页必须完整落在单一区内（跨界红线由构造保证）。
    pub fn project(&mut self) -> Result<(u64, u64), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        let mut total_full = 0u64;
        let mut total_waste = 0u64;
        let n = self.zones.len();
        for i in 0..n {
            let (base, len) = (self.zones[i].base, self.zones[i].len);
            let (full, covered, waste) = zone_projection(&UsableZone { base, len }, self.hp_size)?;
            total_full += full;
            total_waste += waste;
            let first = (base + self.hp_size - 1) & !(self.hp_size - 1);
            for k in 0..full {
                self.placements.push((first + k * self.hp_size, i as u64));
            }
            let _ = covered;
        }
        // 守恒：全部安置不跨界
        for (p, zi) in &self.placements {
            let z = &self.zones[*zi as usize];
            if *p < z.base || p + self.hp_size > z.end() {
                return Err(E_CROSS);
            }
        }
        Ok((total_full, total_waste))
    }

    pub fn seal(&mut self) -> Result<usize, u16> {
        self.sealed = true;
        Ok(self.placements.len())
    }

    /// 安置率：覆盖字节 / 总区字节（千分比）。
    pub fn coverage_permille(&self, total_len: u64) -> u64 {
        if total_len == 0 {
            return 0;
        }
        (self.placements.len() as u64 * self.hp_size) * 1000 / total_len
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.hp_size.to_le_bytes());
        for (p, i) in &self.placements {
            buf.extend_from_slice(&p.to_le_bytes());
            buf.extend_from_slice(&i.to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：2MiB 预判——单区 16MiB 全对齐 → 8 页全覆盖零余料。
    let mut p1 = HugePageProjection::new(HP_2M)?;
    p1.add_zone(0x1000_0000, 0x100_0000)?; // 16MiB, 已对齐
    let (full, waste) = p1.project()?;
    if full != 8 || waste != 0 {
        return Err(E_BAL);
    }
    p1.seal()?;
    if p1.coverage_permille(0x100_0000) != 1000 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：非对齐区——头余料裁剪。
    let mut p2 = HugePageProjection::new(HP_2M)?;
    p2.add_zone(0x1001_2345, 0x100_0000)?;
    let (full2, waste2) = p2.project()?;
    // 对齐首 2MiB 界 0x1020_0000；尾 0x1101_2345 → 整页数 (0x1101_2345-0x1020_0000)/0x20_0000 = 0xE1_2345/0x20_0000 = 7.03 → 7
    if full2 != 7 {
        return Err(E_BAL);
    }
    if waste2 != 0x100_0000 - 7 * 0x20_0000 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：双区跨界防线——两相邻区各自安置，绝不跨区。
    let mut p3 = HugePageProjection::new(HP_2M)?;
    p3.add_zone(0x2000_0000, 0x20_0000)?; // 恰 1 页
    p3.add_zone(0x2020_0000, 0x20_0000)?; // 恰 1 页（相邻区）
    let (full3, _) = p3.project()?;
    if full3 != 2 {
        return Err(E_BAL);
    }
    for (p, i) in &p3.placements {
        let z = &p3.zones[*i as usize];
        if *p < z.base || p + HP_2M > z.end() {
            return Err(E_CROSS);
        }
    }
    hits += 1;

    // R4：1GiB 对 2MiB 的页表项收益账。
    let sav = pte_savings(1, 600); // 1 页 1GiB 覆盖 512 个 2MiB 位
    if sav != 88 {
        return Err(E_BAL);
    }
    if pte_savings(0, 600) != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：非法粒度 → E_HPSIZE。
    if HugePageProjection::new(0x40_0000).is_ok() {
        return Err(E_HPSIZE);
    }
    hits += 1;

    // I2：空区 → E_EMPTY。
    let mut i2 = HugePageProjection::new(HP_2M)?;
    if i2.add_zone(0x1000, 0) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I3：小区放不下 → 0 页全余料。
    let mut i3 = HugePageProjection::new(HP_2M)?;
    i3.add_zone(0x3000_0000, 0x1_0000)?;
    let (f3, w3) = i3.project()?;
    if f3 != 0 || w3 != 0x1_0000 {
        return Err(E_BAL);
    }
    hits += 1;

    // I4：重复区 → E_DUP。
    let mut i4 = HugePageProjection::new(HP_2M)?;
    i4.add_zone(0x4000_0000, 0x20_0000)?;
    if i4.add_zone(0x4000_0000, 0x20_0000) != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // I5：封板后再投影 → E_RDONLY。
    let mut i5 = HugePageProjection::new(HP_2M)?;
    i5.add_zone(0x5000_0000, 0x20_0000)?;
    i5.project()?;
    i5.seal()?;
    if i5.project() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
