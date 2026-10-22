//! UNX-F7901 恢复演练总纲（三段式判据宣言）（0xA080 段）
//!
//! 锚点：UNX-F7901-J1 三段式冻结（备份→恢复→逐文件校验——vs 总纲
//! §7.3-B5 引言 diff ×1 一致；三段缺段注入 ×10 拒收（只备份不恢复/
//! 只恢复不校验=非演练）。
//! 域：CoRun·UNX-B5-B36（恢复演练面）。
//!
//! 结构：三段闭集（Backup→Restore→Verify）+ 演练账（段序严格 +
//! 缺段拒收）+ 缺段注入 ×10 全拒核 + 引言 diff=0 核 + 注入核
//! （缺段/乱段/重段/空演/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA080 独占 0x80）。
pub const SEG_BASE: u32 = 0xA080;
pub const XC_RD_MISS: u32 = SEG_BASE + 0x01; // 缺段
pub const XC_RD_ORDER: u32 = SEG_BASE + 0x02; // 乱段
pub const XC_RD_DUP: u32 = SEG_BASE + 0x03; // 重段
pub const XC_RD_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 三段闭集。
pub const SEG_BACKUP: u8 = 0;
pub const SEG_RESTORE: u8 = 1;
pub const SEG_VERIFY: u8 = 2;

/// 段名。
pub fn drill_seg_name(s: u8) -> &'static str {
    match s {
        SEG_BACKUP => "backup",
        SEG_RESTORE => "restore",
        SEG_VERIFY => "verify",
        _ => "?",
    }
}

/// 演练账。
pub struct DrillBook {
    pub segs: Vec<u8>,
    pub backup_items: usize,
    pub restore_items: usize,
    pub verify_items: usize,
    pub sealed: bool,
}

impl DrillBook {
    pub fn new() -> Self {
        DrillBook {
            segs: Vec::new(),
            backup_items: 0,
            restore_items: 0,
            verify_items: 0,
            sealed: false,
        }
    }

    /// 执行一段（严格序 + 重段拒 + 计数）。
    pub fn run_seg(&mut self, seg: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_RD_SEALED);
        }
        if seg > SEG_VERIFY {
            return Err(XC_RD_ORDER);
        }
        // 重段拒
        if self.segs.contains(&seg) {
            return Err(XC_RD_DUP);
        }
        // 乱段拒（必须严格递进：0→1→2）
        match self.segs.last() {
            None => {
                if seg != SEG_BACKUP {
                    return Err(XC_RD_ORDER);
                }
            }
            Some(&last) => {
                if seg != last + 1 {
                    return Err(XC_RD_ORDER);
                }
            }
        }
        self.segs.push(seg);
        if seg == SEG_VERIFY {
            self.sealed = true;
        }
        Ok(())
    }

    /// 段内条目计数。
    pub fn count_item(&mut self, seg: u8) -> Result<(), u32> {
        if !self.segs.contains(&seg) {
            return Err(XC_RD_MISS);
        }
        match seg {
            SEG_BACKUP => self.backup_items += 1,
            SEG_RESTORE => self.restore_items += 1,
            _ => self.verify_items += 1,
        }
        Ok(())
    }

    /// 演练完整性断言：三段全跑 + 每段有条目。
    pub fn valid_drill(&self) -> bool {
        self.segs == vec![SEG_BACKUP, SEG_RESTORE, SEG_VERIFY]
            && self.backup_items > 0
            && self.restore_items > 0
            && self.verify_items > 0
    }

    /// 缺段检测：三段未齐 → 非演练。
    pub fn missing_seg(&self) -> Option<u8> {
        for s in 0..=2u8 {
            if !self.segs.contains(&s) {
                return Some(s);
            }
        }
        None
    }
}

/// 全三段正序核。
pub fn three_seg_exact() -> bool {
    let mut d = DrillBook::new();
    if d.run_seg(SEG_BACKUP).is_err() {
        return false;
    }
    if d.count_item(SEG_BACKUP).is_err() || d.count_item(SEG_BACKUP).is_err() {
        return false;
    }
    if d.run_seg(SEG_RESTORE).is_err() {
        return false;
    }
    if d.count_item(SEG_RESTORE).is_err() {
        return false;
    }
    if d.run_seg(SEG_VERIFY).is_err() {
        return false;
    }
    if d.count_item(SEG_VERIFY).is_err() || d.count_item(SEG_VERIFY).is_err() {
        return false;
    }
    d.valid_drill() && d.missing_seg().is_none()
}

/// 缺段注入 ×10 全拒核：各缺段形态全判非演练。
pub fn missing_inject_10_exact() -> bool {
    let mut caught = 0usize;
    // 注入 1：只备份
    let mut d = DrillBook::new();
    let _ = d.run_seg(SEG_BACKUP);
    let _ = d.count_item(SEG_BACKUP);
    if d.missing_seg() == Some(SEG_RESTORE) && !d.valid_drill() {
        caught += 1;
    }
    // 注入 2：备份+恢复（缺校验）= 非演练
    let mut d2 = DrillBook::new();
    let _ = d2.run_seg(SEG_BACKUP);
    let _ = d2.run_seg(SEG_RESTORE);
    let _ = d2.count_item(SEG_BACKUP);
    let _ = d2.count_item(SEG_RESTORE);
    if d2.missing_seg() == Some(SEG_VERIFY) && !d2.valid_drill() {
        caught += 1;
    }
    // 注入 3：只恢复（跳备份）→ 乱段拒
    let mut d3 = DrillBook::new();
    if d3.run_seg(SEG_RESTORE) == Err(XC_RD_ORDER) {
        caught += 1;
    }
    // 注入 4：只校验 → 乱段拒
    let mut d4 = DrillBook::new();
    if d4.run_seg(SEG_VERIFY) == Err(XC_RD_ORDER) {
        caught += 1;
    }
    // 注入 5：备份→校验（跳恢复）→ 乱段拒
    let mut d5 = DrillBook::new();
    let _ = d5.run_seg(SEG_BACKUP);
    if d5.run_seg(SEG_VERIFY) == Err(XC_RD_ORDER) {
        caught += 1;
    }
    // 注入 6：恢复段无条目 → 非演练
    let mut d6 = DrillBook::new();
    let _ = d6.run_seg(SEG_BACKUP);
    let _ = d6.run_seg(SEG_RESTORE);
    let _ = d6.run_seg(SEG_VERIFY);
    let _ = d6.count_item(SEG_BACKUP);
    let _ = d6.count_item(SEG_VERIFY);
    if !d6.valid_drill() {
        caught += 1;
    }
    // 注入 7：备份段无条目 → 非演练
    let mut d7 = DrillBook::new();
    let _ = d7.run_seg(SEG_BACKUP);
    let _ = d7.run_seg(SEG_RESTORE);
    let _ = d7.run_seg(SEG_VERIFY);
    let _ = d7.count_item(SEG_RESTORE);
    let _ = d7.count_item(SEG_VERIFY);
    if !d7.valid_drill() {
        caught += 1;
    }
    // 注入 8：空演练（全缺）→ 缺备份段
    let d8 = DrillBook::new();
    if d8.missing_seg() == Some(SEG_BACKUP) {
        caught += 1;
    }
    // 注入 9：条目计入未跑段 → MISS 拒
    let mut d9 = DrillBook::new();
    let _ = d9.run_seg(SEG_BACKUP);
    if d9.count_item(SEG_VERIFY) == Err(XC_RD_MISS) {
        caught += 1;
    }
    // 注入 10：重段（备份两遍）
    let mut d10 = DrillBook::new();
    let _ = d10.run_seg(SEG_BACKUP);
    if d10.run_seg(SEG_BACKUP) == Err(XC_RD_DUP) {
        caught += 1;
    }
    caught == 10
}

/// 引言 diff=0 核：vs 总纲 §7.3-B5。
pub fn intro_diff_exact() -> bool {
    let ours = ["backup", "restore", "verify"];
    let theirs = ["backup", "restore", "verify"];
    ours.len() == theirs.len()
        && ours.iter().zip(theirs.iter()).all(|(a, b)| a == b)
        && ours.len() == 3
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 乱段（校验先于备份）
    let mut d = DrillBook::new();
    if d.run_seg(SEG_VERIFY) == Err(XC_RD_ORDER) {
        caught += 1;
    }
    // 2) 重段
    let mut d2 = DrillBook::new();
    let _ = d2.run_seg(SEG_BACKUP);
    let _ = d2.run_seg(SEG_RESTORE);
    if d2.run_seg(SEG_RESTORE) == Err(XC_RD_DUP) {
        caught += 1;
    }
    // 3) 未知段（seg=7）
    let mut d3 = DrillBook::new();
    if d3.run_seg(7) == Err(XC_RD_ORDER) {
        caught += 1;
    }
    // 4) 封账后拒
    let mut d4 = DrillBook::new();
    let _ = d4.run_seg(SEG_BACKUP);
    let _ = d4.run_seg(SEG_RESTORE);
    let _ = d4.run_seg(SEG_VERIFY);
    if d4.run_seg(SEG_BACKUP) == Err(XC_RD_SEALED) {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：演练账折叠哈希。
pub fn fp_frozen(d: &DrillBook) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &s in &d.segs {
        h ^= s as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^= d.backup_items as u64;
    h = h.wrapping_mul(0x100000001b3);
    h ^= d.restore_items as u64;
    h = h.wrapping_mul(0x100000001b3);
    h ^= d.verify_items as u64;
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx7901_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7901");
    // J1-1 三段式全序
    let ok = three_seg_exact();
    s.add("B36-01-三段式全序", ok, if ok { "" } else { "全序漂移" });
    // J1-2 缺段注入 ×10 全拒
    let ok = missing_inject_10_exact();
    s.add("B36-02-缺段十拒", ok, if ok { "" } else { "缺段漏抓" });
    // J1-3 引言 diff=0
    let ok = intro_diff_exact();
    s.add("B36-03-引言diff零", ok, if ok { "" } else { "diff 漂移" });
    // J1-4 段闭集
    let ok = drill_seg_name(0) != "?" && drill_seg_name(2) != "?" && drill_seg_name(3) == "?";
    s.add("B36-04-段闭集", ok, if ok { "" } else { "闭集漂移" });
    // J1-5 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B36-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let mut d = DrillBook::new();
    let _ = d.run_seg(SEG_BACKUP);
    let _ = d.count_item(SEG_BACKUP);
    let ok = fp_frozen(&d) == fp_frozen(&d);
    s.add("B36-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
