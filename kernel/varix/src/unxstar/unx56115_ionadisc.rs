//! UNX-F56115 IO 缺侧 N/A 纪律实例件（0x60E40 段）
//!
//! 锚点：UNX-F56115-J1 缺侧登记全量（IO 对照缺侧逐例落账：对照键 × 原因
//! 码 × 波次三元组）；J2 N/A 行纪律（缺侧行只出 N/A、数值混入必抓、补
//! 采转正留痕）；J3 缺侧审计链（原因码闭集、指纹留痕、封账只读）三判据
//! 全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·缺侧纪律面）。
//!
//! 结构：IO 缺侧原因码闭集（盘忙拒采/脚本未冻结/窗未闭合三闭集）+ 缺侧
//! 登记册 + N/A 行纪律核 + 补采转正语义 + 指纹留痕 + 注入核（未知原因
//! 码/空键/数值泄漏/重复登记/未登记补采/封账改写/重复转正必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60E40 独占 0x20）。
pub const SEG_BASE: u32 = 0x60E40;
pub const IN_UNK_REASON: u32 = SEG_BASE + 0x01; // 未知缺侧原因码
pub const IN_KEY_EMPTY: u32 = SEG_BASE + 0x02; // 对照键为空
pub const IN_VALUE_LEAK: u32 = SEG_BASE + 0x03; // 缺侧行报数值
pub const IN_DUP: u32 = SEG_BASE + 0x04; // 重复登记
pub const IN_REPAIR_UNREG: u32 = SEG_BASE + 0x05; // 补采对象未登记
pub const IN_SEALED: u32 = SEG_BASE + 0x06; // 缺侧账已封账拒写
pub const IN_NOT_PENDING: u32 = SEG_BASE + 0x07; // 已转正再补采

/// IO 缺侧原因码闭集。
pub const R_BUSY: usize = 0;
pub const R_SCRIPT_UNFROZEN: usize = 1;
pub const R_WINDOW_OPEN: usize = 2;
pub const R_COUNT: usize = 3;
pub const R_NAMES: [&str; R_COUNT] = ["disk_busy_rejected", "script_not_frozen", "window_not_closed"];

/// N/A 行标记。
pub const NA_MARK: &str = "N/A";

/// N/A 行纪律静态核。
pub fn row_discipline_ok(row: &str, is_missing_side: bool) -> bool {
    if is_missing_side {
        row.ends_with(NA_MARK)
    } else {
        !row.ends_with(NA_MARK)
    }
}

/// 缺侧账目：对照键 × 原因码 × 波次 × 待补态。
#[derive(Clone)]
pub struct IoNaEntry {
    pub key: String,
    pub reason: usize,
    pub wave: u32,
    pub pending: bool,
}

/// 缺侧总账。
pub struct IoNaBook {
    pub entries: Vec<IoNaEntry>,
    pub sealed: bool,
    pub audit: Vec<String>,
}

impl IoNaBook {
    pub fn new() -> Self {
        IoNaBook { entries: Vec::new(), sealed: false, audit: Vec::new() }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.entries.len() {
            if self.entries[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 缺侧登记：空键拒 + 原因闭集 + 重复拒三闸。
    pub fn declare(&mut self, key: &str, reason: usize, wave: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IN_SEALED);
        }
        if key.is_empty() {
            return Err(IN_KEY_EMPTY);
        }
        if reason >= R_COUNT {
            return Err(IN_UNK_REASON);
        }
        if wave == 0 {
            return Err(IN_SEALED);
        }
        if self.find(key).is_some() {
            return Err(IN_DUP);
        }
        self.entries.push(IoNaEntry { key: key.to_string(), reason, wave, pending: true });
        self.audit.push(format!("declare|{}|{}|w{}", key, R_NAMES[reason], wave));
        Ok(())
    }

    /// 缺侧行出账（只出 N/A）。
    pub fn na_row(&self, key: &str) -> Result<String, u32> {
        match self.find(key) {
            Some(i) if self.entries[i].pending => Ok(format!("{}|{}", key, NA_MARK)),
            Some(_) => Err(IN_NOT_PENDING),
            None => Err(IN_NOT_PENDING),
        }
    }

    /// 补采转正（待补态才可转正，转正留痕）。
    pub fn repair(&mut self, key: &str, wave: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IN_SEALED);
        }
        let idx = match self.find(key) {
            Some(i) => i,
            None => return Err(IN_REPAIR_UNREG),
        };
        if !self.entries[idx].pending {
            return Err(IN_NOT_PENDING);
        }
        if wave <= self.entries[idx].wave {
            return Err(IN_SEALED);
        }
        self.entries[idx].pending = false;
        self.audit.push(format!("repair|{}|w{}", key, wave));
        Ok(())
    }

    /// J1：登记全量。
    pub fn declared_ok(&self) -> bool {
        !self.entries.is_empty()
            && self.entries.iter().all(|e| !e.key.is_empty() && e.reason < R_COUNT)
    }

    /// J2：待补闭式。
    pub fn pending_census(&self) -> (usize, usize) {
        let mut p = 0;
        let mut f = 0;
        for e in self.entries.iter() {
            if e.pending {
                p += 1;
            } else {
                f += 1;
            }
        }
        (p, f)
    }

    /// J3：审计链完整。
    pub fn audit_ok(&self) -> bool {
        !self.audit.is_empty()
            && self.audit.iter().all(|a| a.starts_with("declare|") || a.starts_with("repair|"))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹（逐例复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for e in self.entries.iter() {
            let line = format!("{}|{}|{}|{}", e.key, e.reason, e.wave, e.pending);
            for b in line.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h
    }
}

/// 标准夹具：3 例缺侧（两待补一已转正）。
pub fn standard_book() -> IoNaBook {
    let mut b = IoNaBook::new();
    let plan: [(&str, usize, u32); 3] =
        [("io|wave7|4krr", R_BUSY, 7), ("io|wave7|4krw", R_SCRIPT_UNFROZEN, 7), ("io|wave6|1msr", R_WINDOW_OPEN, 6)];
    for p in plan.iter() {
        if b.declare(p.0, p.1, p.2).is_err() {
            panic!("standard fixture declare must succeed");
        }
    }
    if b.repair("io|wave6|1msr", 9).is_err() {
        panic!("standard fixture repair must succeed");
    }
    b
}

/// 注入核：7 注入必抓（未知原因码/空键/数值泄漏/重复登记/未登记补采/重复转正/封账改写）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知原因码
    let mut b = IoNaBook::new();
    if b.declare("k", 9, 1) == Err(IN_UNK_REASON) {
        caught += 1;
    }
    // 2) 空键
    if b.declare("", R_BUSY, 1) == Err(IN_KEY_EMPTY) {
        caught += 1;
    }
    // 3) 数值泄漏（缺侧行报数值）
    if !row_discipline_ok("io|wave7|4krr|412000", true) {
        caught += 1;
    }
    // 4) 重复登记
    if b.declare("k", R_BUSY, 1).is_ok() && b.declare("k", R_BUSY, 2) == Err(IN_DUP) {
        caught += 1;
    }
    // 5) 未登记补采
    if b.repair("ghost", 3) == Err(IN_REPAIR_UNREG) {
        caught += 1;
    }
    // 6) 重复转正
    if b.repair("k", 3).is_ok() && b.repair("k", 4) == Err(IN_NOT_PENDING) {
        caught += 1;
    }
    // 7) 封账改写拒
    b.seal();
    if b.declare("late", R_BUSY, 1) == Err(IN_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 缺侧账渲染导出。
pub fn render_book(b: &IoNaBook) -> String {
    let mut out = String::new();
    for e in b.entries.iter() {
        let st = if e.pending { NA_MARK } else { "REPAIRED" };
        out.push_str(&format!("{}|{}|w{}|{}\n", e.key, R_NAMES[e.reason], e.wave, st));
    }
    out
}

/// 渲染完整性核：行数齐 + 状态闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| ln.ends_with(NA_MARK) || ln.ends_with("REPAIRED"))
}

/// 判据入口（CheckSet）。
pub fn run_unx56115_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56115");
    // J1-1 缺侧登记全量（三例逐例合法）
    let b = standard_book();
    let ok = b.declared_ok() && b.entries.len() == 3;
    s.add("56115-J1-缺侧登记全量", ok, if ok { "" } else { "缺侧例残缺" });
    // J1-2 原因码闭集齐
    let ok = R_NAMES.len() == R_COUNT && R_NAMES[1] == "script_not_frozen";
    s.add("56115-J1-原因码闭集齐", ok, if ok { "" } else { "原因码漂移" });
    // J1-3 待补闭式（2 待补 + 1 已转正）
    let (p, f) = b.pending_census();
    let ok = p == 2 && f == 1 && p + f == b.entries.len();
    s.add("56115-J1-待补闭式", ok, if ok { "" } else { "闭式漂移" });
    // J2-1 缺侧行只出 N/A
    let ok = b.na_row("io|wave7|4krr") == Ok("io|wave7|4krr|N/A".to_string());
    s.add("56115-J2-缺侧行NA", ok, if ok { "" } else { "N/A 漂移" });
    // J2-2 数值泄漏必抓 + N/A 行放行
    let ok = !row_discipline_ok("io|wave7|4krr|412000", true)
        && row_discipline_ok("io|wave7|4krr|N/A", true)
        && row_discipline_ok("io|ctrl|412000", false);
    s.add("56115-J2-N/A纪律核", ok, if ok { "" } else { "纪律核漂移" });
    // J2-3 已转正行拒 N/A
    let ok = b.na_row("io|wave6|1msr").is_err();
    s.add("56115-J2-转正拒NA", ok, if ok { "" } else { "转正语义漂移" });
    // J2-4 补采转正留痕
    let mut b2 = standard_book();
    let ok = b2.repair("io|wave7|4krr", 10).is_ok()
        && b2.pending_census() == (1, 2)
        && b2.audit.iter().any(|a| a.starts_with("repair|io|wave7|4krr"));
    s.add("56115-J2-补采转正留痕", ok, if ok { "" } else { "留痕缺失" });
    // J3-1 审计链完整（3 declare + 1 repair）
    let ok = b.audit_ok() && b.audit.len() == 4;
    s.add("56115-J3-审计链完整", ok, if ok { "" } else { "审计链断裂" });
    // J3-2 封账指纹稳定
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("56115-J3-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 指纹敏感（转正后指纹变）
    let fp0 = standard_book().seal_fingerprint();
    let mut b3 = standard_book();
    let _ = b3.repair("io|wave7|4krr", 10);
    let ok = b3.seal_fingerprint() != fp0;
    s.add("56115-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.declare("late", R_BUSY, 1) == Err(IN_SEALED);
    s.add("56115-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3) && text.contains("|N/A") && text.contains("REPAIRED");
    s.add("56115-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56115-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56115_all_checks_green() {
        let set = run_unx56115_checks();
        assert!(set.all_passed(), "UNX-F56115 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56115_fingerprint_stable() {
        assert_eq!(standard_book().seal_fingerprint(), standard_book().seal_fingerprint());
    }
}
