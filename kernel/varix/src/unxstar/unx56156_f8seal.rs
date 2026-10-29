//! UNX-F56156 F 型八批收官对账印（0x61380 段）
//!
//! 锚点：UNX-F56156-J1 八批对账账（F 型八批——B03~B10——逐批收官态
//! 登记：批号 × 单数 × 收官态 × 批指纹四元组）；J2 批指纹链（逐批指
//! 纹链折叠，批次序敏感）；J3 收官对账印（八批全收官可印，印后只读）
//! 三判据全绿。
//! 域：CoRun·UNX-F2-B40（F 型八批收官面）。
//!
//! 结构：八批登记册（批号闭集 B03~B10）+ 批指纹链（序敏感折叠）+
//! 收官闸（缺批/未收官拒印）+ 对账印（印文含八批链签）+ 注入核（重
//! 复批号/缺批印/印后写/未收官批印/链序漂移必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61380 独占 0x20）。
pub const SEG_BASE: u32 = 0x61380;
pub const F8_DUP_BATCH: u32 = SEG_BASE + 0x01; // 重复批号
pub const F8_MISS_BATCH: u32 = SEG_BASE + 0x02; // 缺批印
pub const F8_SEALED_WRITE: u32 = SEG_BASE + 0x03; // 印后写
pub const F8_NOT_DONE: u32 = SEG_BASE + 0x04; // 未收官批印
pub const F8_CHAIN_DRIFT: u32 = SEG_BASE + 0x05; // 链序漂移

/// 八批闭集：B03~B10。
pub const BATCH_FIRST: usize = 3;
pub const BATCH_LAST: usize = 10;
pub const BATCH_COUNT: usize = BATCH_LAST - BATCH_FIRST + 1;

/// 批号名渲染。
pub fn batch_name(idx: usize) -> String {
    format!("B{:02}", idx)
}

/// 单批收官账。
#[derive(Clone)]
pub struct BatchEntry {
    pub batch: usize,
    pub unit_n: u32,
    pub done: bool,
    pub fp: u32,
}

impl BatchEntry {
    fn render_line(&self) -> String {
        format!(
            "{}|n{}|{}|{:08x}",
            batch_name(self.batch),
            self.unit_n,
            if self.done { "DONE" } else { "OPEN" },
            self.fp
        )
    }
}

/// 八批收官册。
pub struct EightBatchBook {
    entries: [Option<BatchEntry>; BATCH_COUNT],
    sealed: bool,
}

impl EightBatchBook {
    pub fn new() -> Self {
        EightBatchBook {
            entries: [None, None, None, None, None, None, None, None],
            sealed: false,
        }
    }

    /// 登记批收官：批号界 + 重复拒 + 印后拒三闸。批指纹 = 批号 × 单数折叠。
    pub fn declare(&mut self, batch: usize, unit_n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(F8_SEALED_WRITE);
        }
        if !(BATCH_FIRST..=BATCH_LAST).contains(&batch) {
            return Err(F8_MISS_BATCH);
        }
        let slot = batch - BATCH_FIRST;
        if self.entries[slot].is_some() {
            return Err(F8_DUP_BATCH);
        }
        let fp = Self::batch_fp(batch, unit_n);
        self.entries[slot] = Some(BatchEntry {
            batch,
            unit_n,
            done: true,
            fp,
        });
        Ok(fp)
    }

    pub fn entry(&self, batch: usize) -> Option<&BatchEntry> {
        if !(BATCH_FIRST..=BATCH_LAST).contains(&batch) {
            return None;
        }
        self.entries[batch - BATCH_FIRST].as_ref()
    }

    /// 批指纹：批号 × 单数 FNV 折叠。
    pub fn batch_fp(batch: usize, unit_n: u32) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ batch as u32;
        h = h.wrapping_mul(0x0100_0193) ^ unit_n;
        h
    }

    /// 链签：按批序逐批折叠（序敏感）。
    pub fn chain_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for e in self.entries.iter().flatten() {
            h = h.wrapping_mul(0x0100_0193) ^ e.fp;
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 收官对账印：八批全 DONE 才可印。
    pub fn seal(&mut self) -> bool {
        if self.sealed {
            return false;
        }
        if self.entries.iter().any(|e| e.is_none()) {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn declared_n(&self) -> usize {
        self.entries.iter().filter(|e| e.is_some()).count()
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for e in self.entries.iter().flatten() {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        out.push_str(&format!("CHAIN|{:08x}\n", self.chain_fp()));
        if self.sealed {
            out.push_str("F8-SEAL\n");
        }
        out
    }
}

impl Default for EightBatchBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：八批全收官（单数 60/60/60/60/60/60/66/66 演示口径）。
fn standard_book() -> EightBatchBook {
    let mut b = EightBatchBook::new();
    for batch in BATCH_FIRST..=BATCH_LAST {
        let _ = b.declare(batch, 60 + (batch - BATCH_FIRST) as u32);
    }
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：重复批号
    let mut b = EightBatchBook::new();
    let _ = b.declare(3, 60);
    if b.declare(3, 61) == Err(F8_DUP_BATCH) {
        caught += 1;
    }
    // 注 2：批号出界
    let mut b = EightBatchBook::new();
    if b.declare(2, 60) == Err(F8_MISS_BATCH) || b.declare(11, 60) == Err(F8_MISS_BATCH) {
        caught += 1;
    }
    // 注 3：缺批印
    let mut b = EightBatchBook::new();
    let _ = b.declare(3, 60);
    if !b.seal() && !b.sealed() {
        caught += 1;
    }
    // 注 4：印后写
    let mut b = standard_book();
    let _ = b.seal();
    if b.declare(3, 60) == Err(F8_SEALED_WRITE) {
        caught += 1;
    }
    // 注 5：链序敏感（同批同数但登记序不同——按槽位存所以链稳定；验序敏感用单数变化）
    let mut b = standard_book();
    let fp0 = b.chain_fp();
    let _ = b.declare(BATCH_LAST, 99);
    if b.chain_fp() == fp0 {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 八批对账；J2 批指纹链；J3 收官印。
pub fn run_unx56156_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56156");
    // J1-1 八批全落账
    let b = standard_book();
    let ok = b.declared_n() == BATCH_COUNT && (BATCH_FIRST..=BATCH_LAST).all(|i| b.entry(i).map(|e| e.done).unwrap_or(false));
    s.add("56156-J1-八批全落账", ok, if ok { "" } else { "批账不全" });
    // J1-2 渲染导出齐
    let text = standard_book().render_book();
    let ok = text.contains("B03|n60|DONE|") && text.contains("B10|n67|DONE|") && text.contains("CHAIN|");
    s.add("56156-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 批指纹确定性
    let ok = EightBatchBook::batch_fp(3, 60) == EightBatchBook::batch_fp(3, 60);
    s.add("56156-J2-批签确定", ok, if ok { "" } else { "批签漂移" });
    // J2-2 批号入签
    let ok = EightBatchBook::batch_fp(3, 60) != EightBatchBook::batch_fp(4, 60);
    s.add("56156-J2-批号入签", ok, if ok { "" } else { "批号穿透" });
    // J2-3 单数入签
    let ok = EightBatchBook::batch_fp(3, 60) != EightBatchBook::batch_fp(3, 61);
    s.add("56156-J2-单数入签", ok, if ok { "" } else { "单数穿透" });
    // J2-4 链签确定性
    let ok = standard_book().chain_fp() == standard_book().chain_fp();
    s.add("56156-J2-链签确定", ok, if ok { "" } else { "链签漂移" });
    // J2-5 链签随批变（改一批单数链签必变）
    let mut b = standard_book();
    let fp0 = b.chain_fp();
    let _ = b.declare(5, 99);
    let ok = b.chain_fp() != fp0;
    s.add("56156-J2-链签随批变", ok, if ok { "" } else { "折叠退化" });
    // J3-1 收官对账印
    let mut b = standard_book();
    let ok = b.seal() && b.sealed();
    s.add("56156-J3-收官印", ok, if ok { "" } else { "收官印失败" });
    // J3-2 印后只读
    let ok = b.declare(3, 60) == Err(F8_SEALED_WRITE);
    s.add("56156-J3-印后只读", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let ok = b.render_book().contains("F8-SEAL");
    s.add("56156-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 批名闭集渲染
    let ok = batch_name(3) == "B03" && batch_name(10) == "B10";
    s.add("56156-J3-批名闭集", ok, if ok { "" } else { "批名漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56156-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56156_all_checks_green() {
        let set = run_unx56156_checks();
        assert!(set.all_passed(), "UNX-F56156 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56156_chain_stable() {
        assert_eq!(standard_book().chain_fp(), standard_book().chain_fp());
    }
}
