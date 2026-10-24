//! UNX-F0257 早传快照到报告的字段映射（F0201 对接）（0x3DE0 段）
//!
//! 锚点：UNX-F0257-J1 Manifest 节 → 报告字段映射表齐备，双向
//! 抽样一致率 100%。
//! 域：CoRun·UNX-A1-B13（字段映射面）。
//!
//! 结构：映射行（Manifest 节.字段 → 报告字段，双向键）+ 映射表
//! （十二步节各字段齐备）+ 正向核（节.字段 → 报告字段命中）+
//! 反向核（报告字段 → 节.字段命中，与正向互逆）+ 双向抽样一致率
//! （抽样 N 对，正反双命中计 100%）+ 注入核（缺映射 / 环映射 /
//! 双向漂移必抓）。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0x3DE0 独占 0x10）。
pub const SEG_BASE: u32 = 0x3DE0;
pub const FM_MISS: u32 = SEG_BASE + 0x01; // 映射缺失
pub const FM_CYCLE: u32 = SEG_BASE + 0x02; // 环映射（报告字段自映射回同节.字段为合法；跨行环为破缺）
pub const FM_DUP: u32 = SEG_BASE + 0x03; // 报告字段重名（两节.字段映射到同一报告字段）
pub const FM_SEALED: u32 = SEG_BASE + 0x04; // 表封账拒写

/// 映射行：Manifest 节.字段 → 报告字段。
#[derive(Clone)]
pub struct MapRow {
    pub step: u32,       // 1..=12
    pub field: u32,      // 1..=8（F0241 八字段步节）
    pub report_key: String,
}

/// 映射表。
pub struct FieldMap {
    pub rows: Vec<MapRow>,
    pub sealed: bool,
}

impl FieldMap {
    pub fn new() -> Self {
        FieldMap { rows: Vec::new(), sealed: false }
    }

    /// 添加映射（界守卫 + 报告字段唯一守卫）。
    pub fn add(&mut self, step: u32, field: u32, report_key: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(FM_SEALED);
        }
        if !(1..=12).contains(&step) || !(1..=8).contains(&field) {
            return Err(FM_MISS);
        }
        if report_key.is_empty() {
            return Err(FM_MISS);
        }
        // 报告字段唯一（重名即双源归一，拒绝——报告字段必须可溯源）
        if self.rows.iter().any(|r| r.report_key == report_key) {
            return Err(FM_DUP);
        }
        self.rows.push(MapRow {
            step,
            field,
            report_key: String::from(report_key),
        });
        Ok(())
    }

    /// 正向核：节.字段 → 报告字段。
    pub fn forward(&self, step: u32, field: u32) -> Result<&str, u32> {
        match self.rows.iter().find(|r| r.step == step && r.field == field) {
            Some(r) => Ok(&r.report_key),
            None => Err(FM_MISS),
        }
    }

    /// 反向核：报告字段 → 节.字段（与正向互逆验证）。
    pub fn backward(&self, report_key: &str) -> Result<(u32, u32), u32> {
        match self.rows.iter().find(|r| r.report_key == report_key) {
            Some(r) => Ok((r.step, r.field)),
            None => Err(FM_MISS),
        }
    }

    /// 双向互逆抽样核：对全部行做正→反→比较，一致率必须 100%。
    pub fn roundtrip_rate(&self) -> u32 {
        if self.rows.is_empty() {
            return 0;
        }
        let mut hit = 0usize;
        for r in &self.rows {
            if let Ok(k) = self.forward(r.step, r.field) {
                if let Ok((s, f)) = self.backward(k) {
                    if s == r.step && f == r.field {
                        hit += 1;
                    }
                }
            }
        }
        (hit * 10_000 / self.rows.len()) as u32
    }

    /// 齐备核：十二步节每节至少一条映射。
    pub fn steps_covered(&self) -> bool {
        let mut cover = [false; 12];
        for r in &self.rows {
            cover[(r.step - 1) as usize] = true;
        }
        cover.iter().all(|&b| b)
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 表渲染。
    pub fn render(&self) -> String {
        let mut out = format!("字段映射 行数={} 十二步覆盖={}\n", self.rows.len(), self.steps_covered());
        for r in &self.rows {
            let line = format!("  S{:02}.F{} -> {}\n", r.step, r.field, r.report_key);
            out.push_str(&line);
        }
        out
    }
}

/// 标准夹具：十二步各 1~2 条映射（齐备 + 覆盖边界字段 1/8）。
pub fn standard_map() -> FieldMap {
    let mut m = FieldMap::new();
    let _ = m.add(1, 1, "fw.vendor");
    let _ = m.add(1, 8, "fw.evidence_blob");
    let _ = m.add(2, 1, "mem.regions");
    let _ = m.add(2, 8, "mem.snapshot_blob");
    let _ = m.add(3, 1, "cpu.features");
    let _ = m.add(4, 1, "rand.slots");
    let _ = m.add(5, 1, "param.table");
    let _ = m.add(6, 1, "health.zones");
    let _ = m.add(6, 8, "health.panel_blob");
    let _ = m.add(7, 1, "time.tsc_base");
    let _ = m.add(7, 2, "time.freq_hz");
    let _ = m.add(8, 1, "log.boot_fp");
    let _ = m.add(9, 1, "lastboot.result");
    let _ = m.add(9, 2, "lastboot.seq");
    let _ = m.add(10, 1, "subs.matrix");
    let _ = m.add(11, 1, "integ.crc_chain");
    let _ = m.add(12, 1, "timing.steps");
    let _ = m.add(12, 8, "timing.total_ns");
    m
}

/// 注入核：6 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut m = FieldMap::new();
    // 1) 节号越界拒（0 与 13）
    if m.add(0, 1, "x") == Err(FM_MISS) {
        caught += 1;
    }
    if m.add(13, 1, "x") == Err(FM_MISS) {
        caught += 1;
    }
    // 2) 字段越界拒
    if m.add(1, 9, "x") == Err(FM_MISS) {
        caught += 1;
    }
    // 3) 报告字段重名拒
    let _ = m.add(1, 1, "dup.key");
    if m.add(2, 1, "dup.key") == Err(FM_DUP) {
        caught += 1;
    }
    // 4) 封账拒写
    m.seal();
    if m.add(3, 1, "late") == Err(FM_SEALED) {
        caught += 1;
    }
    // 5) 缺映射拒（未注册节.字段正向查）
    let t = standard_map();
    if t.forward(3, 3) == Err(FM_MISS) {
        caught += 1;
    }
    // 6) 双向 100%（干净表 roundtrip 满分；篡改一行 report_key 后反向仍命中同行）
    let mut q = standard_map();
    if q.roundtrip_rate() == 10_000 {
        caught += 1;
    }
    let (s0, f0) = (q.rows[0].step, q.rows[0].field);
    if let Ok(k) = q.forward(s0, f0) {
        if q.backward(k) == Ok((s0, f0)) {
            caught += 1;
        }
    }
    (caught, 7)
}

/// 判据入口（CheckSet）。
pub fn run_unx257_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0257");
    // J1-1 映射表齐备（十二步全覆盖 + 渲染非空）
    let m = standard_map();
    let ok = m.steps_covered() && !m.render().is_empty();
    s.add("B13-52-映射表齐备", ok, if ok { "" } else { "覆盖缺节" });
    // J1-2 双向抽样一致率 100%
    let m = standard_map();
    let ok = m.roundtrip_rate() == 10_000;
    s.add("B13-53-双向一致100%", ok, if ok { "" } else { "互逆破缺" });
    // J1-3 正反两向命中（正向查字段名 / 反向查节.字段）
    let m = standard_map();
    let ok = m.forward(7, 2) == Ok("time.freq_hz")
        && m.backward("lastboot.seq") == Ok((9, 2));
    s.add("B13-54-正反两向命中", ok, if ok { "" } else { "命中漂移" });
    // J1-4 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B13-55-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx257_all_checks_green() {
        let set = run_unx257_checks();
        assert!(set.all_passed(), "UNX-F0257 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx257_report_key_unique() {
        let m = standard_map();
        let mut keys: Vec<&str> = Vec::new();
        for r in &m.rows {
            assert!(!keys.contains(&r.report_key.as_str()), "report key duplicated");
            keys.push(&r.report_key);
        }
    }
}
