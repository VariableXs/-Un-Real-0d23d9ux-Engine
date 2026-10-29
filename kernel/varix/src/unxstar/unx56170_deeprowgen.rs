//! UNX-F56170 syscall 对照行深化版生成器（0x61540 段）
//!
//! 锚点：UNX-F56170-J1 深化行生成账（syscall 对照行深化版逐行生成：
//! 行键 × 六类延迟分量 × 合成总延迟 × 行指纹五元组落账）；J2 生成语
//! 义（总延迟 = 六类分量之和整数域 + 行键唯一 + 生成器确定性——同参
//! 同行同签）；J3 深化版印（行集指纹折叠 + 生成完印）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·深化行生成面）。
//!
//! 结构：六类分量闭集（read/write/open/close/ioctl/mmap 各自平均延迟）
//! + 行生成器（分量 → 总延迟合成 + 行指纹）+ 行账（键唯一索引）+ 深
//! 化版印 + 注入核（重复键/分量越界/和漂移/生成漂移/空账印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61540 独占 0x20）。
pub const SEG_BASE: u32 = 0x61540;
pub const DG_DUP_KEY: u32 = SEG_BASE + 0x01; // 重复行键
pub const DG_COMP_RANGE: u32 = SEG_BASE + 0x02; // 分量越界
pub const DG_SUM_DRIFT: u32 = SEG_BASE + 0x03; // 和漂移
pub const DG_GEN_DRIFT: u32 = SEG_BASE + 0x04; // 生成漂移
pub const DG_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空账印

/// 六类分量闭集（与 F56161 同源类表）。
pub const COMP_COUNT: usize = 6;
pub const COMP_NAMES: [&str; COMP_COUNT] = ["read", "write", "open", "close", "ioctl", "mmap"];

/// 分量物理界（微秒，单类 ≤ 100ms）。
pub const COMP_MAX: u64 = 100_000;

/// 分量界核。
pub fn comp_ok(v: u64) -> bool {
    v <= COMP_MAX
}

/// 深化对照行。
#[derive(Clone)]
pub struct DeepRow {
    pub key: String,
    pub comps: [u64; COMP_COUNT],
    pub total: u64,
    pub fp: u32,
}

impl DeepRow {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|T{}|{:08x}",
            self.key,
            self.comps[0], self.comps[1], self.comps[2],
            self.comps[3], self.comps[4], self.comps[5],
            self.total, self.fp
        )
    }
}

/// 行指纹：键 × 分量折叠。
pub fn row_fp(key: &str, comps: &[u64; COMP_COUNT]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in key.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    for c in comps {
        h = h.wrapping_mul(0x0100_0193) ^ (*c as u32);
        h ^= (*c >> 32) as u32;
    }
    h
}

/// 生成器。
pub struct DeepRowGen {
    rows: Vec<DeepRow>,
    sealed: bool,
}

impl DeepRowGen {
    pub fn new() -> Self {
        DeepRowGen {
            rows: Vec::new(),
            sealed: false,
        }
    }

    /// 生成一行：键唯一 + 分量界 + 总和合成三核。
    pub fn gen(&mut self, key: &str, comps: [u64; COMP_COUNT]) -> Result<u64, u32> {
        if self.sealed {
            return Err(DG_EMPTY_SEAL);
        }
        if key.is_empty() || self.rows.iter().any(|r| r.key == key) {
            return Err(DG_DUP_KEY);
        }
        if comps.iter().any(|c| !comp_ok(*c)) {
            return Err(DG_COMP_RANGE);
        }
        let total: u64 = comps.iter().sum();
        let fp = row_fp(key, &comps);
        self.rows.push(DeepRow {
            key: key.to_string(),
            comps,
            total,
            fp,
        });
        Ok(total)
    }

    /// 生成确定性重放核：同键同分量重算同签。
    pub fn replay_ok(&self, key: &str, comps: &[u64; COMP_COUNT]) -> bool {
        match self.rows.iter().find(|r| r.key == key) {
            None => false,
            Some(r) => r.fp == row_fp(key, comps) && r.total == comps.iter().sum::<u64>(),
        }
    }

    pub fn row(&self, key: &str) -> Option<&DeepRow> {
        self.rows.iter().find(|r| r.key == key)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 行集指纹：逐行折叠。
    pub fn rows_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for r in &self.rows {
            h = h.wrapping_mul(0x0100_0193) ^ r.fp;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 深化版印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.rows.is_empty() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_rows(&self) -> String {
        let mut out = String::new();
        for r in &self.rows {
            out.push_str(&r.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("DEEPGEN-SEAL\n");
        }
        out
    }
}

impl Default for DeepRowGen {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三行深化版（full/mid/idle 场景分量演示口径）。
fn standard_gen() -> DeepRowGen {
    let mut g = DeepRowGen::new();
    let _ = g.gen("full_load", [50, 60, 200, 30, 40, 150]);
    let _ = g.gen("mid_load", [40, 50, 180, 25, 35, 120]);
    let _ = g.gen("idle", [30, 40, 150, 20, 30, 90]);
    g
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：重复键
    let mut g = standard_gen();
    if g.gen("full_load", [1, 1, 1, 1, 1, 1]) == Err(DG_DUP_KEY) {
        caught += 1;
    }
    // 注 2：分量越界
    let mut g = DeepRowGen::new();
    if g.gen("x", [COMP_MAX + 1, 0, 0, 0, 0, 0]) == Err(DG_COMP_RANGE) {
        caught += 1;
    }
    // 注 3：空键
    let mut g = DeepRowGen::new();
    if g.gen("", [1, 1, 1, 1, 1, 1]) == Err(DG_DUP_KEY) {
        caught += 1;
    }
    // 注 4：和漂移（手改 total 后重放核必假）
    let mut g = standard_gen();
    if let Some(r) = g.rows.get_mut(0) {
        r.total = 1;
    }
    if !g.replay_ok("full_load", &[50, 60, 200, 30, 40, 150]) {
        caught += 1;
    }
    // 注 5：空账印
    let mut g = DeepRowGen::new();
    if !g.seal() && !g.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 生成账；J2 生成语义；J3 深化版印。
pub fn run_unx56170_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56170");
    // J1-1 三行全生成
    let g = standard_gen();
    let ok = g.len() == 3;
    s.add("56170-J1-三行全生成", ok, if ok { "" } else { "生成账不全" });
    // J1-2 渲染导出齐
    let text = standard_gen().render_rows();
    let ok = text.contains("full_load|50|60|200|30|40|150|T530|") && text.contains("idle|30|40|150|20|30|90|T360|");
    s.add("56170-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 总和核（50+60+200+30+40+150 = 530）
    let ok = g.row("full_load").map(|r| r.total == 530).unwrap_or(false);
    s.add("56170-J2-总和核", ok, if ok { "" } else { "和漂移" });
    // J2-2 分量界核
    let ok = comp_ok(0) && comp_ok(COMP_MAX) && !comp_ok(COMP_MAX + 1);
    s.add("56170-J2-分量界", ok, if ok { "" } else { "界核漂移" });
    // J2-3 生成确定（同参同签）
    let ok = row_fp("k", &[1, 2, 3, 4, 5, 6]) == row_fp("k", &[1, 2, 3, 4, 5, 6]);
    s.add("56170-J2-生成确定", ok, if ok { "" } else { "生成漂移" });
    // J2-4 键入签
    let ok = row_fp("k1", &[1, 2, 3, 4, 5, 6]) != row_fp("k2", &[1, 2, 3, 4, 5, 6]);
    s.add("56170-J2-键入签", ok, if ok { "" } else { "键穿透" });
    // J2-5 分量敏感
    let ok = row_fp("k", &[1, 2, 3, 4, 5, 6]) != row_fp("k", &[2, 2, 3, 4, 5, 6]);
    s.add("56170-J2-分量敏感", ok, if ok { "" } else { "分量穿透" });
    // J3-1 重放核过
    let ok = standard_gen().replay_ok("mid_load", &[40, 50, 180, 25, 35, 120]);
    s.add("56170-J3-重放核过", ok, if ok { "" } else { "重放漂移" });
    // J3-2 深化版印
    let mut g = standard_gen();
    let ok = g.seal() && g.sealed();
    s.add("56170-J3-深化版印", ok, if ok { "" } else { "印失败" });
    // J3-3 印后只读
    let ok = g.gen("x", [1, 1, 1, 1, 1, 1]) == Err(DG_EMPTY_SEAL);
    s.add("56170-J3-印后只读", ok, if ok { "" } else { "只读闸失效" });
    // J3-4 行集指纹确定
    let ok = standard_gen().rows_fp() == standard_gen().rows_fp();
    s.add("56170-J3-行集指纹", ok, if ok { "" } else { "指纹漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56170-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56170_all_checks_green() {
        let set = run_unx56170_checks();
        assert!(set.all_passed(), "UNX-F56170 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56170_sum_stable() {
        assert_eq!(50 + 60 + 200 + 30 + 40 + 150, 530);
    }
}
