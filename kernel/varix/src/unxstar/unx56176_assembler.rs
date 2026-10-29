//! UNX-F56176 syscall 对照总账行组装件（波 26 总账贡献面）（0x61600 段）
//!
//! 锚点：UNX-F56176-J1 总账行组装账（syscall 对照总账行组装——行键 ×
//! 六分量均值 × 装配态 × 装配指纹五元组逐行落账）；J2 组装语义（分量
//! 均值 = 各深化行分量均值再均值，整数域两段均值）+ 键唯一 + 装配确
//! 定性（同源行集同装配签）；J3 装配印（行集齐装可印）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·总账组装面）。
//!
//! 结构：深化行来源登记（行键 × 六分量）+ 组装器（键唯一 + 分段均值
//! 合成）+ 装配账 + 装配印 + 注入核（重复键/未知源行/均值漂移/装配漂
//! 移/空集印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61600 独占 0x20）。
pub const SEG_BASE: u32 = 0x61600;
pub const AS_DUP_KEY: u32 = SEG_BASE + 0x01; // 重复装配键
pub const AS_SRC_MISS: u32 = SEG_BASE + 0x02; // 未知源行
pub const AS_MEAN_DRIFT: u32 = SEG_BASE + 0x03; // 均值漂移
pub const ASSEMBLE_DRIFT: u32 = SEG_BASE + 0x04; // 装配漂移
pub const AS_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空集印

/// 分量数（与 F56170 同源六分量）。
pub const COMP_N: usize = 6;

/// 源深化行。
#[derive(Clone)]
pub struct SourceRow {
    pub key: String,
    pub comps: [u64; COMP_N],
}

/// 两段整数均值：先对每个分量跨行求均值，再合成行。
pub fn comp_means(rows: &[SourceRow]) -> [u64; COMP_N] {
    let mut out = [0u64; COMP_N];
    if rows.is_empty() {
        return out;
    }
    for c in 0..COMP_N {
        let sum: u128 = rows.iter().map(|r| r.comps[c] as u128).sum();
        out[c] = (sum / rows.len() as u128) as u64;
    }
    out
}

/// 装配行。
#[derive(Clone)]
pub struct AssembledRow {
    pub key: String,
    pub means: [u64; COMP_N],
    pub total: u64,
    pub fp: u32,
}

impl AssembledRow {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|T{}|{:08x}",
            self.key,
            self.means[0], self.means[1], self.means[2],
            self.means[3], self.means[4], self.means[5],
            self.total, self.fp
        )
    }
}

/// 装配指纹：键 × 均值折叠。
pub fn assemble_fp(key: &str, means: &[u64; COMP_N]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in key.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    for m in means {
        h = h.wrapping_mul(0x0100_0193) ^ (*m as u32);
    }
    h
}

/// 总账组装器。
pub struct Assembler {
    sources: Vec<SourceRow>,
    rows: Vec<AssembledRow>,
    sealed: bool,
}

impl Assembler {
    pub fn new() -> Self {
        Assembler {
            sources: Vec::new(),
            rows: Vec::new(),
            sealed: false,
        }
    }

    /// 登记源行。
    pub fn add_source(&mut self, key: &str, comps: [u64; COMP_N]) {
        self.sources.push(SourceRow {
            key: key.to_string(),
            comps,
        });
    }

    /// 装配一行：从源行集选一组（按键列表）合成均值行。
    pub fn assemble(&mut self, key: &str, src_keys: &[&str]) -> Result<u64, u32> {
        if self.sealed {
            return Err(AS_EMPTY_SEAL);
        }
        if key.is_empty() || self.rows.iter().any(|r| r.key == key) {
            return Err(AS_DUP_KEY);
        }
        let mut picked: Vec<SourceRow> = Vec::new();
        for sk in src_keys {
            match self.sources.iter().find(|s| s.key == *sk) {
                None => return Err(AS_SRC_MISS),
                Some(s) => picked.push(s.clone()),
            }
        }
        if picked.is_empty() {
            return Err(AS_SRC_MISS);
        }
        let means = comp_means(&picked);
        let total: u64 = means.iter().sum();
        let fp = assemble_fp(key, &means);
        self.rows.push(AssembledRow {
            key: key.to_string(),
            means,
            total,
            fp,
        });
        Ok(total)
    }

    pub fn row(&self, key: &str) -> Option<&AssembledRow> {
        self.rows.iter().find(|r| r.key == key)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 装配确定核：同源行集重装配同签。
    pub fn deterministic_ok(&self, key: &str, src_keys: &[&str]) -> bool {
        let mut picked: Vec<SourceRow> = Vec::new();
        for sk in src_keys {
            match self.sources.iter().find(|s| s.key == *sk) {
                None => return false,
                Some(s) => picked.push(s.clone()),
            }
        }
        let means = comp_means(&picked);
        match self.row(key) {
            None => false,
            Some(r) => r.fp == assemble_fp(key, &means) && r.means == means,
        }
    }

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

    /// 装配指纹。
    pub fn book_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for r in &self.rows {
            h = h.wrapping_mul(0x0100_0193) ^ r.fp;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for r in &self.rows {
            out.push_str(&r.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("ASSEMBLE-SEAL\n");
        }
        out
    }
}

impl Default for Assembler {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三源行装配一行（波 26 贡献面）。
fn standard_assembler() -> Assembler {
    let mut a = Assembler::new();
    a.add_source("w26_full", [50, 60, 200, 30, 40, 150]);
    a.add_source("w26_mid", [40, 50, 180, 25, 35, 120]);
    a.add_source("w26_idle", [30, 40, 150, 20, 30, 90]);
    let _ = a.assemble("wave26_total", &["w26_full", "w26_mid", "w26_idle"]);
    a
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：重复装配键
    let mut a = standard_assembler();
    if a.assemble("wave26_total", &["w26_full"]) == Err(AS_DUP_KEY) {
        caught += 1;
    }
    // 注 2：未知源行
    let mut a = Assembler::new();
    if a.assemble("x", &["ghost"]) == Err(AS_SRC_MISS) {
        caught += 1;
    }
    // 注 3：均值漂移（手改 means 后确定性核必假）
    let mut a = standard_assembler();
    if let Some(r) = a.rows.get_mut(0) {
        r.means[0] = 999;
    }
    if !a.deterministic_ok("wave26_total", &["w26_full", "w26_mid", "w26_idle"]) {
        caught += 1;
    }
    // 注 4：空源装配
    let mut a = Assembler::new();
    a.add_source("s1", [1, 1, 1, 1, 1, 1]);
    if a.assemble("x", &[]) == Err(AS_SRC_MISS) {
        caught += 1;
    }
    // 注 5：空集印
    let mut a = Assembler::new();
    if !a.seal() && !a.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 组装账；J2 组装语义；J3 装配印。
pub fn run_unx56176_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56176");
    // J1-1 一行装配落账
    let a = standard_assembler();
    let ok = a.len() == 1;
    s.add("56176-J1-装配落账", ok, if ok { "" } else { "装配账不全" });
    // J1-2 渲染导出齐
    let text = standard_assembler().render_book();
    let ok = text.contains("wave26_total|40|50|176|25|35|120|T446|");
    s.add("56176-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 分段均值核（(50+40+30)/3=40 / (200+180+150)/3=176）
    let ok = a.row("wave26_total").map(|r| r.means[0] == 40 && r.means[2] == 176).unwrap_or(false);
    s.add("56176-J2-分段均值", ok, if ok { "" } else { "均值漂移" });
    // J2-2 总和核（40+50+176+25+35+120 = 446）
    let ok = a.row("wave26_total").map(|r| r.total == 446).unwrap_or(false);
    s.add("56176-J2-总和核", ok, if ok { "" } else { "总和漂移" });
    // J2-3 装配确定
    let ok = a.deterministic_ok("wave26_total", &["w26_full", "w26_mid", "w26_idle"]);
    s.add("56176-J2-装配确定", ok, if ok { "" } else { "装配漂移" });
    // J2-4 键入签
    let ok = assemble_fp("k1", &[1, 1, 1, 1, 1, 1]) != assemble_fp("k2", &[1, 1, 1, 1, 1, 1]);
    s.add("56176-J2-键入签", ok, if ok { "" } else { "键穿透" });
    // J2-5 均值敏感
    let ok = assemble_fp("k", &[1, 1, 1, 1, 1, 1]) != assemble_fp("k", &[2, 1, 1, 1, 1, 1]);
    s.add("56176-J2-均值敏感", ok, if ok { "" } else { "均值穿透" });
    // J3-1 装配印
    let mut a = standard_assembler();
    let ok = a.seal() && a.sealed();
    s.add("56176-J3-装配印", ok, if ok { "" } else { "装配印失败" });
    // J3-2 印后拒装
    let ok = a.assemble("x", &["w26_full"]) == Err(AS_EMPTY_SEAL);
    s.add("56176-J3-印后拒装", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let ok = a.render_book().contains("ASSEMBLE-SEAL");
    s.add("56176-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 装配指纹确定
    let ok = standard_assembler().book_fp() == standard_assembler().book_fp();
    s.add("56176-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56176-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56176_all_checks_green() {
        let set = run_unx56176_checks();
        assert!(set.all_passed(), "UNX-F56176 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56176_mean_stable() {
        assert_eq!(comp_means(&[
            SourceRow { key: "a".to_string(), comps: [30, 0, 0, 0, 0, 0] },
            SourceRow { key: "b".to_string(), comps: [50, 0, 0, 0, 0, 0] },
        ])[0], 40);
    }
}
