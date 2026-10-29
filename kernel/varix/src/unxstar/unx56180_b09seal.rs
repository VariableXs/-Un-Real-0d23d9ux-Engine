//! UNX-F56180 B09 批防重声明与批收官印（0x61680 段）
//!
//! 锚点：UNX-F56180-J1 批防重声明账（B09 批全部 20 单逐单声明：单号
//! × 模块名 × 段位 × 收官态四元组，无重号无重段）；J2 批收官印（批指
//! 纹折叠 + 印文渲染，印后批账只读）；J3 防重核（同号/同段/同模块名
//! 三类重复必抓）三判据全绿。
//! 域：CoRun·UNX-F2-B40（B09 批收官面）。
//!
//! 结构：批登记册（20 单逐条落账，单号连续闭集校验）+ 段位独占核 +
//! 批指纹折叠核 + 收官印 + 注入核（重号/重段/缺单/印后写/空批印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61680 独占 0x20）。
pub const SEG_BASE: u32 = 0x61680;
pub const B09_DUP_ID: u32 = SEG_BASE + 0x01;
pub const B09_DUP_SEG: u32 = SEG_BASE + 0x02;
pub const B09_DUP_MOD: u32 = SEG_BASE + 0x03;
pub const B09_GAP: u32 = SEG_BASE + 0x04;
pub const B09_WRITE_SEALED: u32 = SEG_BASE + 0x05;

/// B09 批单号闭集：F56161~F56180（20 单）。
pub const B09_FIRST: u32 = 56_161;
pub const B09_LAST: u32 = 56_180;
pub const B09_COUNT: usize = (B09_LAST - B09_FIRST + 1) as usize;

/// 单条批声明。
#[derive(Clone)]
pub struct BatchEntry {
    pub id: u32,
    pub module: String,
    pub seg: u32,
    pub done: bool,
}

impl BatchEntry {
    fn render_line(&self) -> String {
        format!(
            "F{}|{}|0x{:04X}|{}",
            self.id,
            self.module,
            self.seg,
            if self.done { "DONE" } else { "OPEN" }
        )
    }
}

/// 批登记册。
pub struct BatchBook {
    entries: Vec<BatchEntry>,
    sealed: bool,
}

impl BatchBook {
    pub fn new() -> Self {
        BatchBook {
            entries: Vec::new(),
            sealed: false,
        }
    }

    /// 落一条声明：单号界 + 三类重复四闸。
    pub fn declare(&mut self, id: u32, module: &str, seg: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(B09_WRITE_SEALED);
        }
        if !(B09_FIRST..=B09_LAST).contains(&id) {
            return Err(B09_GAP);
        }
        if self.entries.iter().any(|e| e.id == id) {
            return Err(B09_DUP_ID);
        }
        if self.entries.iter().any(|e| e.seg == seg) {
            return Err(B09_DUP_SEG);
        }
        if self.entries.iter().any(|e| e.module == module) {
            return Err(B09_DUP_MOD);
        }
        self.entries.push(BatchEntry {
            id,
            module: module.to_string(),
            seg,
            done: true,
        });
        Ok(())
    }

    pub fn seal(&mut self) -> bool {
        if self.sealed || self.entries.len() != B09_COUNT {
            return false;
        }
        let mut ids: Vec<u32> = self.entries.iter().map(|e| e.id).collect();
        ids.sort();
        for (i, id) in ids.iter().enumerate() {
            if *id != B09_FIRST + i as u32 {
                return false;
            }
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 批指纹。
    pub fn batch_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut feed = |h: &mut u32, v: u32| {
            for i in 0..4 {
                *h ^= (v >> (i * 8)) as u32 & 0xFF;
                *h = h.wrapping_mul(0x0100_0193);
            }
        };
        for e in &self.entries {
            feed(&mut h, e.id);
            feed(&mut h, e.seg);
            for b in e.module.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h
    }

    /// 段位独占核。
    pub fn segs_exclusive(&self) -> bool {
        let lo = SEG_BASE - (B09_COUNT as u32 - 1) * 0x20;
        let mut segs: Vec<u32> = self.entries.iter().map(|e| e.seg).collect();
        segs.sort();
        segs.windows(2).all(|w| w[0] < w[1]) && segs.first().map(|s| *s >= lo).unwrap_or(false)
    }

    /// 印文渲染。
    pub fn render_seal(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        out.push_str(&format!(
            "B09-SEAL|{:08x}|{}\n",
            self.batch_fingerprint(),
            self.entries.len()
        ));
        out
    }
}

impl Default for BatchBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：B09 批 20 单全量落账。
fn standard_book() -> BatchBook {
    let mut book = BatchBook::new();
    for i in 0..B09_COUNT {
        let id = B09_FIRST + i as u32;
        let seg = SEG_BASE - (B09_COUNT as u32 - 1 - i as u32) * 0x20;
        let _ = book.declare(id, &format!("unx{}", id), seg);
    }
    book
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：单号重复
    let mut book = BatchBook::new();
    let _ = book.declare(B09_FIRST, "unx56161", SEG_BASE - 19 * 0x20);
    if book.declare(B09_FIRST, "unx56162", SEG_BASE - 18 * 0x20) == Err(B09_DUP_ID) {
        caught += 1;
    }
    // 注 2：段位重复
    let mut book = BatchBook::new();
    let _ = book.declare(B09_FIRST, "unx56161", 0x61600);
    if book.declare(B09_FIRST + 1, "unx56162", 0x61600) == Err(B09_DUP_SEG) {
        caught += 1;
    }
    // 注 3：模块名重复
    let mut book = BatchBook::new();
    let _ = book.declare(B09_FIRST, "unx56161", 0x61600);
    if book.declare(B09_FIRST + 1, "unx56161", 0x61620) == Err(B09_DUP_MOD) {
        caught += 1;
    }
    // 注 4：单号出界
    let mut book = BatchBook::new();
    if book.declare(B09_LAST + 1, "unx56181", 0x61600) == Err(B09_GAP) {
        caught += 1;
    }
    // 注 5：缺单拒印
    let mut book = BatchBook::new();
    let _ = book.declare(B09_FIRST, "unx56161", 0x61600);
    if !book.seal() && !book.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 防重声明账；J2 收官印；J3 防重核。
pub fn run_unx56180_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56180");
    // J1-1 20 单全量落账
    let book = standard_book();
    let ok = book.len() == B09_COUNT && book.segs_exclusive();
    s.add("56180-J1-二十单全量", ok, if ok { "" } else { "批账不全" });
    // J1-2 渲染导出齐
    let text = standard_book().render_seal();
    let ok = text.contains("F56161|unx56161|") && text.contains("B09-SEAL|");
    s.add("56180-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 收官印
    let mut book = standard_book();
    let ok = book.seal() && book.sealed();
    s.add("56180-J2-收官印", ok, if ok { "" } else { "收官印失败" });
    // J2-2 批指纹确定
    let ok = standard_book().batch_fingerprint() == standard_book().batch_fingerprint();
    s.add("56180-J2-批指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J2-3 指纹随单变
    let mut book2 = standard_book();
    if let Some(e) = book2.entries.get_mut(0) {
        e.seg ^= 0x20;
    }
    let ok = book2.batch_fingerprint() != standard_book().batch_fingerprint();
    s.add("56180-J2-指纹随单变", ok, if ok { "" } else { "折叠失效" });
    // J3-1 断档拒印
    let mut book2 = standard_book();
    book2.entries.pop();
    let ok = !book2.seal() && !book2.sealed();
    s.add("56180-J3-断档拒印", ok, if ok { "" } else { "闭集闸失效" });
    // J3-2 印后写账拒
    let mut book = standard_book();
    let _ = book.seal();
    let ok = book.declare(B09_FIRST, "unx56161", 0x61600) == Err(B09_WRITE_SEALED);
    s.add("56180-J3-印后写拒", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文含单数
    let ok = standard_book().render_seal().contains("|20\n");
    s.add("56180-J3-印文含单数", ok, if ok { "" } else { "印文漂移" });
    // J3-4 空满异签
    let empty = BatchBook::new();
    let ok = empty.batch_fingerprint() != standard_book().batch_fingerprint();
    s.add("56180-J3-空满异签", ok, if ok { "" } else { "折叠退化" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56180-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56180_all_checks_green() {
        let set = run_unx56180_checks();
        assert!(set.all_passed(), "UNX-F56180 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56180_fp_stable() {
        assert_eq!(standard_book().batch_fingerprint(), standard_book().batch_fingerprint());
    }
}
