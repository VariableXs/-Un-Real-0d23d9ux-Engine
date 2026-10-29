//! UNX-F56158 账本存储布局与容量规划件（0x613C0 段）
//!
//! 锚点：UNX-F56158-J1 存储布局账（账本三区——行区/索引区/审计区——
//! 容量与水位逐区落账：区 × 容量 × 已用 × 水位四元组）；J2 容量守卫
//! 语义（行区满拒写 / 索引区满拒建键 / 审计区满拒拍快照三闸）；J3 扩
//! 容语义（水位 ≥ 80% 触发扩容建议，扩容只升不降）三判据全绿。
//! 域：CoRun·UNX-F2-B40（账本域·存储布局面）。
//!
//! 结构：三区布局（容量常量闭集）+ 水位核（万分比水位 + 扩容阈值）+
//! 写入守卫（区满三闸）+ 扩容账（扩容记录只升不降）+ 注入核（区满强
//! 写/水位算术漂移/扩容回退/未知区/容量回退必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x613C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x613C0;
pub const ST_ROW_FULL: u32 = SEG_BASE + 0x01; // 行区满强写
pub const ST_IDX_FULL: u32 = SEG_BASE + 0x02; // 索引区满建键
pub const ST_AUD_FULL: u32 = SEG_BASE + 0x03; // 审计区满快照
pub const ST_UNK_ZONE: u32 = SEG_BASE + 0x04; // 未知区
pub const ST_SHRINK: u32 = SEG_BASE + 0x05; // 容量回退

/// 三区闭集。
pub const Z_ROWS: usize = 0;
pub const Z_INDEX: usize = 1;
pub const Z_AUDIT: usize = 2;
pub const ZONE_COUNT: usize = 3;

/// 区名渲染。
pub const ZONE_NAMES: [&str; ZONE_COUNT] = ["rows", "index", "audit"];

/// 区容量闭集。
pub const CAP_ROWS: usize = 4096;
pub const CAP_INDEX: usize = 1024;
pub const CAP_AUDIT: usize = 512;

/// 区容量查询。
pub fn zone_cap(zone: usize) -> Result<usize, u32> {
    match zone {
        Z_ROWS => Ok(CAP_ROWS),
        Z_INDEX => Ok(CAP_INDEX),
        Z_AUDIT => Ok(CAP_AUDIT),
        _ => Err(ST_UNK_ZONE),
    }
}

/// 水位核：万分比整数域。
pub fn level_permille(used: usize, cap: usize) -> u32 {
    if cap == 0 {
        return 0;
    }
    ((used as u64 * 10_000) / cap as u64) as u32
}

/// 扩容阈值（水位万分比 8000 = 80%）。
pub const EXPAND_AT: u32 = 8_000;

/// 扩容建议核。
pub fn expand_advised(used: usize, cap: usize) -> bool {
    level_permille(used, cap) >= EXPAND_AT
}

/// 单区状态。
#[derive(Clone)]
pub struct ZoneState {
    pub zone: usize,
    pub used: usize,
    pub cap: usize,
}

impl ZoneState {
    fn render_line(&self) -> String {
        format!(
            "{}|{}/{}|lv{}|{}",
            ZONE_NAMES[self.zone],
            self.used,
            self.cap,
            level_permille(self.used, self.cap),
            if expand_advised(self.used, self.cap) { "EXPAND" } else { "OK" }
        )
    }
}

/// 存储布局总账。
pub struct StorageLayout {
    zones: [ZoneState; ZONE_COUNT],
    expansions: Vec<(usize, usize, usize)>, // 区 × 旧容 × 新容
}

impl StorageLayout {
    pub fn new() -> Self {
        StorageLayout {
            zones: [
                ZoneState { zone: Z_ROWS, used: 0, cap: CAP_ROWS },
                ZoneState { zone: Z_INDEX, used: 0, cap: CAP_INDEX },
                ZoneState { zone: Z_AUDIT, used: 0, cap: CAP_AUDIT },
            ],
            expansions: Vec::new(),
        }
    }

    /// 写入记账：行区满拒。
    pub fn write_rows(&mut self, n: usize) -> Result<(), u32> {
        let z = &mut self.zones[Z_ROWS];
        if z.used + n > z.cap {
            return Err(ST_ROW_FULL);
        }
        z.used += n;
        Ok(())
    }

    /// 建键记账：索引区满拒。
    pub fn write_index(&mut self, n: usize) -> Result<(), u32> {
        let z = &mut self.zones[Z_INDEX];
        if z.used + n > z.cap {
            return Err(ST_IDX_FULL);
        }
        z.used += n;
        Ok(())
    }

    /// 快照记账：审计区满拒。
    pub fn write_audit(&mut self, n: usize) -> Result<(), u32> {
        let z = &mut self.zones[Z_AUDIT];
        if z.used + n > z.cap {
            return Err(ST_AUD_FULL);
        }
        z.used += n;
        Ok(())
    }

    /// 扩容：只升不降，扩容留痕。
    pub fn expand(&mut self, zone: usize, new_cap: usize) -> Result<(), u32> {
        if zone >= ZONE_COUNT {
            return Err(ST_UNK_ZONE);
        }
        let z = &mut self.zones[zone];
        if new_cap <= z.cap {
            return Err(ST_SHRINK);
        }
        self.expansions.push((zone, z.cap, new_cap));
        z.cap = new_cap;
        Ok(())
    }

    pub fn zone(&self, zone: usize) -> Option<&ZoneState> {
        self.zones.get(zone)
    }

    /// 水位全读。
    pub fn levels(&self) -> [u32; ZONE_COUNT] {
        [
            level_permille(self.zones[0].used, self.zones[0].cap),
            level_permille(self.zones[1].used, self.zones[1].cap),
            level_permille(self.zones[2].used, self.zones[2].cap),
        ]
    }

    /// 渲染导出。
    pub fn render_layout(&self) -> String {
        let mut out = String::new();
        for z in &self.zones {
            out.push_str(&z.render_line());
            out.push('\n');
        }
        for (zone, old, new) in &self.expansions {
            out.push_str(&format!(
                "expand:{}:{}->{}\n",
                ZONE_NAMES[*zone], old, new
            ));
        }
        out
    }
}

impl Default for StorageLayout {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：行区 80 满水位 / 索引 10 / 审计 5，触发行区扩容。
fn standard_layout() -> StorageLayout {
    let mut st = StorageLayout::new();
    let _ = st.write_rows(3277); // 3277/4096 = 80.0%
    let _ = st.write_index(100);
    let _ = st.write_audit(50);
    let _ = st.expand(Z_ROWS, 8192);
    st
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：行区满强写
    let mut st = StorageLayout::new();
    let _ = st.write_rows(4096);
    if st.write_rows(1) == Err(ST_ROW_FULL) {
        caught += 1;
    }
    // 注 2：索引区满建键
    let mut st = StorageLayout::new();
    let _ = st.write_index(1024);
    if st.write_index(1) == Err(ST_IDX_FULL) {
        caught += 1;
    }
    // 注 3：审计区满快照
    let mut st = StorageLayout::new();
    let _ = st.write_audit(512);
    if st.write_audit(1) == Err(ST_AUD_FULL) {
        caught += 1;
    }
    // 注 4：扩容回退
    let mut st = StorageLayout::new();
    let _ = st.expand(Z_ROWS, 8192);
    if st.expand(Z_ROWS, 4096) == Err(ST_SHRINK) {
        caught += 1;
    }
    // 注 5：未知区
    let mut st = StorageLayout::new();
    if st.expand(9, 100) == Err(ST_UNK_ZONE) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 布局账；J2 容量守卫；J3 扩容语义。
pub fn run_unx56158_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56158");
    // J1-1 三区布局落账
    let st = standard_layout();
    let ok = st.zone(Z_ROWS).map(|z| z.used == 3277 && z.cap == 8192).unwrap_or(false)
        && st.zone(Z_INDEX).map(|z| z.used == 100).unwrap_or(false);
    s.add("56158-J1-三区落账", ok, if ok { "" } else { "布局漂移" });
    // J1-2 渲染导出齐
    let text = standard_layout().render_layout();
    let ok = text.contains("rows|3277/8192|") && text.contains("expand:rows:4096->8192");
    s.add("56158-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 水位算术锚（1000/4096 = 2441）
    let ok = level_permille(1000, 4096) == 2_441;
    s.add("56158-J2-水位锚", ok, if ok { "" } else { "水位算术漂移" });
    // J2-2 满水位锚（4096/4096 = 10000）
    let ok = level_permille(4096, 4096) == 10_000;
    s.add("56158-J2-满水位锚", ok, if ok { "" } else { "满水位漂移" });
    // J2-3 零容量守卫
    let ok = level_permille(1, 0) == 0;
    s.add("56158-J2-零容守卫", ok, if ok { "" } else { "除零穿透" });
    // J2-4 扩容阈值触发（3277/4096 = 80.0% 恰触发）
    let ok = expand_advised(3277, 4096) && !expand_advised(3276, 4096);
    s.add("56158-J2-扩容阈值", ok, if ok { "" } else { "阈值漂移" });
    // J2-5 容量闭集查询
    let ok = zone_cap(Z_ROWS) == Ok(4096) && zone_cap(Z_INDEX) == Ok(1024)
        && zone_cap(Z_AUDIT) == Ok(512) && zone_cap(9) == Err(ST_UNK_ZONE);
    s.add("56158-J2-容量闭集", ok, if ok { "" } else { "容量漂移" });
    // J3-1 扩容只升不降
    let mut st = StorageLayout::new();
    let ok = st.expand(Z_ROWS, 8192).is_ok() && st.expand(Z_ROWS, 4096) == Err(ST_SHRINK);
    s.add("56158-J3-只升不降", ok, if ok { "" } else { "扩容闸失效" });
    // J3-2 扩容留痕
    let ok = standard_layout().render_layout().contains("expand:rows:4096->8192");
    s.add("56158-J3-扩容留痕", ok, if ok { "" } else { "留痕面漂移" });
    // J3-3 扩容后水位回落（3277/8192 = 40.0%）
    let st = standard_layout();
    let ok = st.zone(Z_ROWS).map(|z| level_permille(z.used, z.cap) == 4_000).unwrap_or(false);
    s.add("56158-J3-水位回落", ok, if ok { "" } else { "回落算术漂移" });
    // J3-4 全区水位可读
    let ok = st.levels() == [4_000, 976, 976];
    s.add("56158-J3-全区水位", ok, if ok { "" } else { "水位读漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56158-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56158_all_checks_green() {
        let set = run_unx56158_checks();
        assert!(set.all_passed(), "UNX-F56158 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56158_level_stable() {
        assert_eq!(level_permille(1000, 4096), 2_441);
    }
}
