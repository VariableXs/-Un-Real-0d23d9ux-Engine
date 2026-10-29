//! UNX-F56072 缺侧 N/A 纪律内存实例件（0x608E0 段）
//!
//! 锚点：UNX-F56072-J1 缺侧登记全量（对照缺侧逐例落账：缺侧原因 × 波次
//! × N/A 行标记）；J2 N/A 行纪律（缺侧行必须标 N/A 且禁报数值，数值行
//! 混入即红）；J3 缺侧审计链（原因码闭集、指纹留痕、封账只读）三判据
//! 全绿。
//! 域：CoRun·UNX-F2-B40（内存域·缺侧纪律面）。
//!
//! 结构：缺侧原因码闭集（冷启动未采/采集器掉线/窗未闭合三闭集）+ 缺侧
//! 登记册（对照键 → 原因码逐例落账）+ N/A 行纪律核（缺侧只出 N/A、
//! 数值行混入必抓）+ 修复补采语义（补采后缺侧转正并留痕）+ 封账语义 +
//! 注入核（未知原因码/空对照键/缺侧行报数值/重复登记/封账改写/未登记
//! 补采必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x608E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x608E0;
pub const NA_UNK_REASON: u32 = SEG_BASE + 0x01; // 未知缺侧原因码（闭集外）
pub const NA_KEY_EMPTY: u32 = SEG_BASE + 0x02; // 对照键为空
pub const NA_VALUE_LEAK: u32 = SEG_BASE + 0x03; // 缺侧行报数值（N/A 纪律穿透）
pub const NA_DUP: u32 = SEG_BASE + 0x04; // 同对照键重复登记
pub const NA_REPAIR_UNREG: u32 = SEG_BASE + 0x05; // 补采对象未在缺侧册
pub const NA_SEALED: u32 = SEG_BASE + 0x06; // 缺侧账已封账拒写
pub const NA_NOT_PENDING: u32 = SEG_BASE + 0x07; // 补采对象已转正（非待补态）

/// 缺侧原因码闭集。
pub const R_COLDSTART: usize = 0;
pub const R_COLLECT_DOWN: usize = 1;
pub const R_WINDOW_OPEN: usize = 2;
pub const R_COUNT: usize = 3;
pub const R_NAMES: [&str; R_COUNT] = ["cold_start_uncollected", "collector_down", "window_not_closed"];

/// N/A 行标记（缺侧行唯一合法出口）。
pub const NA_MARK: &str = "N/A";

/// 缺侧账目：对照键 × 原因码 × 波次 × 待补态 四元组。
#[derive(Clone)]
pub struct NaEntry {
    pub key: String,
    pub reason: usize,
    pub wave: u32,
    pub pending: bool,
}

/// 缺侧登记总账。
pub struct NaBook {
    pub entries: Vec<NaEntry>,
    pub sealed: bool,
    pub audit: Vec<String>,
}

impl NaBook {
    pub fn new() -> Self {
        NaBook { entries: Vec::new(), sealed: false, audit: Vec::new() }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.entries.len() {
            if self.entries[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 缺侧登记：空键拒、未知原因拒、重复登记拒。
    pub fn declare(&mut self, key: &str, reason: usize, wave: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(NA_SEALED);
        }
        if key.is_empty() {
            return Err(NA_KEY_EMPTY);
        }
        if reason >= R_COUNT {
            return Err(NA_UNK_REASON);
        }
        if wave == 0 {
            return Err(NA_SEALED);
        }
        if self.find(key).is_some() {
            return Err(NA_DUP);
        }
        self.entries.push(NaEntry {
            key: key.to_string(),
            reason,
            wave,
            pending: true,
        });
        self.audit.push(format!("declare|{}|{}|w{}", key, R_NAMES[reason], wave));
        Ok(())
    }

    /// 缺侧行出账核：只许 N/A，数值混入即抓。
    pub fn na_row(&self, key: &str) -> Result<String, u32> {
        match self.find(key) {
            None => Err(NA_NOT_PENDING),
            Some(i) => {
                if self.entries[i].pending {
                    Ok(format!("{}|{}", key, NA_MARK))
                } else {
                    Err(NA_NOT_PENDING)
                }
            }
        }
    }

    /// N/A 行纪律静态核：给定行文本，缺侧键必须以 N/A 收尾。
    pub fn row_discipline_ok(row: &str, is_missing_side: bool) -> bool {
        if !is_missing_side {
            return !row.ends_with(NA_MARK);
        }
        row.ends_with(NA_MARK)
    }

    /// 补采转正：待补态才可转正，转正留痕。
    pub fn repair(&mut self, key: &str, wave: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(NA_SEALED);
        }
        let idx = match self.find(key) {
            Some(i) => i,
            None => return Err(NA_REPAIR_UNREG),
        };
        if !self.entries[idx].pending {
            return Err(NA_NOT_PENDING);
        }
        if wave <= self.entries[idx].wave {
            return Err(NA_SEALED);
        }
        self.entries[idx].pending = false;
        self.audit.push(format!("repair|{}|w{}", key, wave));
        Ok(())
    }

    /// J1：缺侧登记全量（每例原因码合法 + 键非空）。
    pub fn declared_ok(&self) -> bool {
        !self.entries.is_empty()
            && self.entries.iter().all(|e| !e.key.is_empty() && e.reason < R_COUNT)
    }

    /// J2：待补集闭式（待补数 + 已转正数 = 总数）。
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

    /// J3：审计链完整（逐条带波次或转正痕迹）。
    pub fn audit_ok(&self) -> bool {
        !self.audit.is_empty()
            && self.audit.iter().all(|a| a.contains("|w") || a.contains("repair|"))
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

/// 标准夹具：3 例缺侧（两例待补、一例已转正）。
pub fn standard_book() -> NaBook {
    let mut b = NaBook::new();
    let plan: [(&str, usize, u32); 3] =
        [("wave7|guest", R_COLDSTART, 7), ("wave7|prefetch", R_COLLECT_DOWN, 7), ("wave6|shell", R_WINDOW_OPEN, 6)];
    for p in plan.iter() {
        if b.declare(p.0, p.1, p.2).is_err() {
            panic!("standard fixture declare must succeed");
        }
    }
    if b.repair("wave6|shell", 9).is_err() {
        panic!("standard fixture repair must succeed");
    }
    b
}

/// 注入核：7 注入必抓（未知原因码/空键/数值泄漏/重复登记/未登记补采/封账改写/重复转正）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知原因码
    let mut b = NaBook::new();
    if b.declare("k1", 9, 1) == Err(NA_UNK_REASON) {
        caught += 1;
    }
    // 2) 空对照键
    if b.declare("", R_COLDSTART, 1) == Err(NA_KEY_EMPTY) {
        caught += 1;
    }
    // 3) 缺侧行报数值（纪律核静态判）
    if !NaBook::row_discipline_ok("wave1|guest|51200", true) {
        caught += 1;
    }
    // 4) 重复登记
    if b.declare("k2", R_COLDSTART, 1).is_ok() && b.declare("k2", R_COLLECT_DOWN, 2) == Err(NA_DUP) {
        caught += 1;
    }
    // 5) 未登记补采
    if b.repair("ghost", 3) == Err(NA_REPAIR_UNREG) {
        caught += 1;
    }
    // 6) 重复转正（已转正再补采）
    if b.repair("k2", 3).is_ok() && b.repair("k2", 4) == Err(NA_NOT_PENDING) {
        caught += 1;
    }
    // 7) 封账后改写拒
    b.seal();
    if b.declare("k3", R_COLDSTART, 1) == Err(NA_SEALED) && b.repair("k2", 5) == Err(NA_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 缺侧账渲染导出（逐例一行：key|reason|wave|状态）。
pub fn render_book(b: &NaBook) -> String {
    let mut out = String::new();
    for e in b.entries.iter() {
        let st = if e.pending { NA_MARK } else { "REPAIRED" };
        out.push_str(&format!("{}|{}|w{}|{}\n", e.key, R_NAMES[e.reason], e.wave, st));
    }
    out
}

/// 渲染完整性核：行数齐 + 待补行以 N/A 收尾 + 已转正行 REPAIRED。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| ln.ends_with(NA_MARK) || ln.ends_with("REPAIRED"))
}

/// 判据入口（CheckSet）。
pub fn run_unx56072_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56072");
    // J1-1 缺侧登记全量（三例逐例合法）
    let b = standard_book();
    let ok = b.declared_ok() && b.entries.len() == 3;
    s.add("56072-J1-缺侧登记全量", ok, if ok { "" } else { "缺侧例残缺" });
    // J1-2 待补集闭式（2 待补 + 1 已转正 = 3）
    let (p, f) = b.pending_census();
    let ok = p == 2 && f == 1 && p + f == b.entries.len();
    s.add("56072-J1-待补闭式", ok, if ok { "" } else { "闭式计数漂移" });
    // J1-3 原因码闭集齐
    let ok = R_NAMES.len() == R_COUNT && R_NAMES[0] == "cold_start_uncollected";
    s.add("56072-J1-原因码闭集齐", ok, if ok { "" } else { "原因码漂移" });
    // J2-1 缺侧行只出 N/A
    let ok = b.na_row("wave7|guest") == Ok("wave7|guest|N/A".to_string());
    s.add("56072-J2-缺侧行NA", ok, if ok { "" } else { "N/A 行漂移" });
    // J2-2 数值行混入必抓（静态纪律核）
    let ok = !NaBook::row_discipline_ok("wave7|guest|51200", true)
        && NaBook::row_discipline_ok("wave7|guest|N/A", true)
        && NaBook::row_discipline_ok("wave7|ctrl|51200", false);
    s.add("56072-J2-N/A纪律核", ok, if ok { "" } else { "纪律核漂移" });
    // J2-3 已转正行不再出 N/A
    let ok = b.na_row("wave6|shell").is_err();
    s.add("56072-J2-转正后拒NA", ok, if ok { "" } else { "转正语义漂移" });
    // J2-4 补采转正留痕
    let mut b2 = standard_book();
    let ok = b2.repair("wave7|guest", 10).is_ok()
        && b2.pending_census() == (1, 2)
        && b2.audit.iter().any(|a| a.starts_with("repair|wave7|guest"));
    s.add("56072-J2-补采转正留痕", ok, if ok { "" } else { "转正留痕缺失" });
    // J3-1 审计链完整
    let ok = b.audit_ok() && b.audit.len() == 4;
    s.add("56072-J3-审计链完整", ok, if ok { "" } else { "审计链断裂" });
    // J3-2 封账指纹确定性
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("56072-J3-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 转正后指纹敏感
    let fp0 = standard_book().seal_fingerprint();
    let mut b3 = standard_book();
    let _ = b3.repair("wave7|guest", 10);
    let ok = b3.seal_fingerprint() != fp0;
    s.add("56072-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.declare("late", R_COLDSTART, 1) == Err(NA_SEALED);
    s.add("56072-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐（N/A 与 REPAIRED 混排）
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3) && text.contains("|N/A") && text.contains("REPAIRED");
    s.add("56072-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56072-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56072_all_checks_green() {
        let set = run_unx56072_checks();
        assert!(set.all_passed(), "UNX-F56072 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56072_fingerprint_stable() {
        assert_eq!(standard_book().seal_fingerprint(), standard_book().seal_fingerprint());
    }
}
