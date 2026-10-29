//! UNX-F56173 syscall 缺侧 N/A 深化实例件（0x615A0 段）
//!
//! 锚点：UNX-F56173-J1 缺侧实例账（syscall 对照行缺侧 N/A 深化实例
//! 逐行落账：行键 × 缺侧侧别 × 原因码 × 守闸态四元组）；J2 N/A 强制
//! 语义（缺侧行无实测值 + 原因码可溯 + 双侧独立三性质）；J3 实例印
//! （全实例守闸绿可印）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·缺侧实例面）。
//!
//! 结构：缺侧原因码闭集（无对机/无代理/无路由/无权限/资源耗尽五码）
//! + 实例登记核（缺侧强制 N/A：无实测字段）+ 守闸核 + 实例印 + 注入
//! 核（缺侧带值/原因越界/重复登记/未知侧/守闸破坏必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x615A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x615A0;
pub const NA_FABRICATED: u32 = SEG_BASE + 0x01; // 缺侧带实测值
pub const NA_REASON_RANGE: u32 = SEG_BASE + 0x02; // 原因码越界
pub const NA_DUP_MARK: u32 = SEG_BASE + 0x03; // 重复登记
pub const NA_UNK_SIDE: u32 = SEG_BASE + 0x04; // 未知侧
pub const NA_GATE_BROKEN: u32 = SEG_BASE + 0x05; // 守闸破坏

/// 双侧闭集。
pub const SIDE_LOCAL: usize = 0;
pub const SIDE_REMOTE: usize = 1;
pub const SIDE_COUNT: usize = 2;

/// 侧名渲染。
pub const SIDE_NAMES: [&str; SIDE_COUNT] = ["local", "remote"];

/// 缺侧原因码闭集（五码）。
pub const R_NO_HOST: usize = 0;
pub const R_NO_AGENT: usize = 1;
pub const R_NO_ROUTE: usize = 2;
pub const R_NO_PERM: usize = 3;
pub const R_NO_RES: usize = 4;
pub const REASON_COUNT: usize = 5;

/// 原因名渲染。
pub const REASON_NAMES: [&str; REASON_COUNT] =
    ["no_host", "no_agent", "no_route", "no_perm", "no_res"];

/// 单实例行。
#[derive(Clone)]
pub struct NaRow {
    pub key: String,
    pub missing: [bool; SIDE_COUNT],
    pub reason: [Option<usize>; SIDE_COUNT],
    pub measured: [Option<u64>; SIDE_COUNT],
}

impl NaRow {
    fn new(key: &str) -> Self {
        NaRow {
            key: key.to_string(),
            missing: [false; SIDE_COUNT],
            reason: [None, None],
            measured: [None, None],
        }
    }
}

/// 缺侧实例账。
pub struct NaInstanceBook {
    rows: Vec<NaRow>,
    sealed: bool,
}

impl NaInstanceBook {
    pub fn new() -> Self {
        NaInstanceBook {
            rows: Vec::new(),
            sealed: false,
        }
    }

    /// 登记缺侧：侧闭集 + 原因闭集 + 重复拒 + 缺侧强制无实测。
    pub fn mark_missing(&mut self, key: &str, side: usize, reason: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(NA_GATE_BROKEN);
        }
        if side >= SIDE_COUNT {
            return Err(NA_UNK_SIDE);
        }
        if reason >= REASON_COUNT {
            return Err(NA_REASON_RANGE);
        }
        let idx = match self.rows.iter().position(|r| r.key == key) {
            Some(i) => i,
            None => {
                self.rows.push(NaRow::new(key));
                self.rows.len() - 1
            }
        };
        let r = &mut self.rows[idx];
        if r.missing[side] {
            return Err(NA_DUP_MARK);
        }
        r.missing[side] = true;
        r.reason[side] = Some(reason);
        r.measured[side] = None; // 编造闸
        Ok(())
    }

    /// 登记实测（仅未缺侧可走）。
    pub fn put_measured(&mut self, key: &str, side: usize, val: u64) -> Result<(), u32> {
        if side >= SIDE_COUNT {
            return Err(NA_UNK_SIDE);
        }
        let idx = match self.rows.iter().position(|r| r.key == key) {
            Some(i) => i,
            None => {
                self.rows.push(NaRow::new(key));
                self.rows.len() - 1
            }
        };
        let r = &mut self.rows[idx];
        if r.missing[side] {
            return Err(NA_FABRICATED);
        }
        r.measured[side] = Some(val);
        Ok(())
    }

    pub fn row(&self, key: &str) -> Option<&NaRow> {
        self.rows.iter().find(|r| r.key == key)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 守闸核：缺侧必无实测 + 原因在场。
    pub fn gate_ok(&self) -> bool {
        self.rows.iter().all(|r| {
            (0..SIDE_COUNT).all(|s| {
                if r.missing[s] {
                    r.reason[s].is_some() && r.measured[s].is_none()
                } else {
                    true
                }
            })
        })
    }

    /// 缺侧实例计数。
    pub fn na_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.missing.iter().any(|m| *m))
            .count()
    }

    pub fn seal(&mut self) -> bool {
        if self.sealed || self.rows.is_empty() || !self.gate_ok() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for r in &self.rows {
            for s in 0..SIDE_COUNT {
                let cell = if r.missing[s] {
                    format!("NA:{}", REASON_NAMES[r.reason[s].unwrap_or(0)])
                } else {
                    match r.measured[s] {
                        Some(v) => format!("M{}", v),
                        None => "WAIT".to_string(),
                    }
                };
                out.push_str(&format!("{}|{}|{}\n", r.key, SIDE_NAMES[s], cell));
            }
        }
        if self.sealed {
            out.push_str("NA-SEAL\n");
        }
        out
    }
}

impl Default for NaInstanceBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三行实例（本机实测 + 两缺侧 + 一双全实测对照）。
fn standard_book() -> NaInstanceBook {
    let mut b = NaInstanceBook::new();
    let _ = b.put_measured("io_read", SIDE_LOCAL, 5300);
    let _ = b.mark_missing("io_read", SIDE_REMOTE, R_NO_HOST);
    let _ = b.put_measured("io_write", SIDE_LOCAL, 6100);
    let _ = b.mark_missing("io_write", SIDE_REMOTE, R_NO_AGENT);
    let _ = b.put_measured("io_ctl", SIDE_LOCAL, 900);
    let _ = b.put_measured("io_ctl", SIDE_REMOTE, 950);
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：缺侧后实测（编造路径拒）
    let mut b = NaInstanceBook::new();
    let _ = b.mark_missing("x", SIDE_LOCAL, R_NO_ROUTE);
    if b.put_measured("x", SIDE_LOCAL, 100) == Err(NA_FABRICATED) {
        caught += 1;
    }
    // 注 2：原因越界
    let mut b = NaInstanceBook::new();
    if b.mark_missing("x", SIDE_LOCAL, 9) == Err(NA_REASON_RANGE) {
        caught += 1;
    }
    // 注 3：重复登记
    let mut b = NaInstanceBook::new();
    let _ = b.mark_missing("x", SIDE_REMOTE, R_NO_PERM);
    if b.mark_missing("x", SIDE_REMOTE, R_NO_RES) == Err(NA_DUP_MARK) {
        caught += 1;
    }
    // 注 4：未知侧
    let mut b = NaInstanceBook::new();
    if b.mark_missing("x", 9, R_NO_HOST) == Err(NA_UNK_SIDE) {
        caught += 1;
    }
    // 注 5：守闸破坏（内存级塞值后 gate_ok 必假）
    let mut b = standard_book();
    if let Some(r) = b.rows.iter_mut().find(|r| r.key == "io_read") {
        r.measured[SIDE_REMOTE] = Some(1);
    }
    if !b.gate_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 实例账；J2 N/A 语义；J3 实例印。
pub fn run_unx56173_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56173");
    // J1-1 三行落账守闸绿
    let b = standard_book();
    let ok = b.len() == 3 && b.gate_ok() && b.na_count() == 2;
    s.add("56173-J1-三行守闸", ok, if ok { "" } else { "实例账漂移" });
    // J1-2 渲染导出齐
    let text = standard_book().render_book();
    let ok = text.contains("io_read|remote|NA:no_host") && text.contains("io_ctl|remote|M950");
    s.add("56173-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 缺侧无实测
    let b = standard_book();
    let ok = b.row("io_read").map(|r| r.measured[SIDE_REMOTE].is_none()).unwrap_or(false);
    s.add("56173-J2-缺侧无值", ok, if ok { "" } else { "编造闸失效" });
    // J2-2 原因可溯
    let ok = b.row("io_write").map(|r| r.reason[SIDE_REMOTE] == Some(R_NO_AGENT)).unwrap_or(false);
    s.add("56173-J2-原因可溯", ok, if ok { "" } else { "原因漂移" });
    // J2-3 原因码闭集渲染
    let ok = REASON_NAMES[R_NO_RES] == "no_res" && REASON_NAMES.len() == 5;
    s.add("56173-J2-原因闭集", ok, if ok { "" } else { "闭集漂移" });
    // J2-4 双侧独立
    let ok = b.row("io_read").map(|r| r.measured[SIDE_LOCAL] == Some(5300)).unwrap_or(false);
    s.add("56173-J2-双侧独立", ok, if ok { "" } else { "侧间穿透" });
    // J2-5 对照行全实测不算缺侧
    let ok = b.na_count() == 2 && b.row("io_ctl").map(|r| !r.missing[0] && !r.missing[1]).unwrap_or(false);
    s.add("56173-J2-对照非缺", ok, if ok { "" } else { "计数漂移" });
    // J3-1 实例印
    let mut b = standard_book();
    let ok = b.seal() && b.sealed();
    s.add("56173-J3-实例印", ok, if ok { "" } else { "实例印失败" });
    // J3-2 印后拒写
    let ok = b.mark_missing("y", SIDE_LOCAL, R_NO_HOST) == Err(NA_GATE_BROKEN);
    s.add("56173-J3-印后拒写", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let ok = b.render_book().contains("NA-SEAL");
    s.add("56173-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 空账拒印
    let mut b = NaInstanceBook::new();
    let ok = !b.seal() && !b.sealed();
    s.add("56173-J3-空账拒印", ok, if ok { "" } else { "印闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56173-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56173_all_checks_green() {
        let set = run_unx56173_checks();
        assert!(set.all_passed(), "UNX-F56173 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56173_gate_stable() {
        assert!(standard_book().gate_ok());
    }
}
