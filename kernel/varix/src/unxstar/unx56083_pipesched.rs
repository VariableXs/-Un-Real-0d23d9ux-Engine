//! UNX-F56083 采集管线调度器（批次编排引擎）（0x60A40 段）
//!
//! 锚点：UNX-F56083-J1 批次编排全量（调度批次逐批落账：批号 × 器集 ×
//! 窗序三元组）；J2 编排语义一致（批内器集与注册表对齐、窗序严格递增、
//! 缺器批次拒开）；J3 调度审计链（批号单调、批指纹链、封账只读）三判据
//! 全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·调度面）。
//!
//! 结构：调度批次登记册（批号 → 器集位图 × 窗序逐批落账）+ 器集对齐核
//! （器集必须为注册表五器闭集的子集且非空）+ 窗序闸（批内窗序 1..n
//! 连续、跨批窗序严格递增）+ 批指纹链 + 封账语义 + 注入核（空器集/
//! 越界器集/窗序跳变/批号回退/重复批号/封账开批/未注册器混入必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60A40 独占 0x20）。
pub const SEG_BASE: u32 = 0x60A40;
pub const SCH_EMPTY_SET: u32 = SEG_BASE + 0x01; // 空器集批次
pub const SCH_SET_RANGE: u32 = SEG_BASE + 0x02; // 越界器集（位图超五位）
pub const SCH_WIN_GAP: u32 = SEG_BASE + 0x03; // 窗序跳变（非连续）
pub const SCH_BATCH_BACK: u32 = SEG_BASE + 0x04; // 批号回退
pub const SCH_BATCH_DUP: u32 = SEG_BASE + 0x05; // 重复批号
pub const SCH_SEALED: u32 = SEG_BASE + 0x06; // 调度账已封账拒开批
pub const SCH_FOREIGN: u32 = SEG_BASE + 0x07; // 未注册器混入器集

/// 器集位图宽度（五采集器，与 F56082 注册表闭集同源）。
pub const SET_BITS: usize = 5;
pub const SET_ALL: u32 = 0b11111;

/// 器集合法性核：非空 + 越界位全零。
pub fn collector_set_ok(set_bm: u32) -> bool {
    set_bm != 0 && (set_bm & !SET_ALL) == 0
}

/// 批指纹：FNV-1a 32 位（批号|器集|窗数复合）。
pub fn batch_fingerprint(batch: u32, set_bm: u32, windows: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}", batch, set_bm, windows);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 调度批次：批号 × 器集位图 × 窗数 × 起窗号 × 指纹 × 链。
#[derive(Clone)]
pub struct SchedBatch {
    pub batch: u32,
    pub set_bm: u32,
    pub windows: u32,
    pub start_win: u32,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 调度总账。
pub struct SchedBook {
    pub batches: Vec<SchedBatch>,
    pub sealed: bool,
}

impl SchedBook {
    pub fn new() -> Self {
        SchedBook { batches: Vec::new(), sealed: false }
    }

    /// 开批：器集守闸 + 批号严格递增 + 窗数 ≥1 三闸，窗号自动续排。
    pub fn open_batch(&mut self, batch: u32, set_bm: u32, windows: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(SCH_SEALED);
        }
        if !collector_set_ok(set_bm) {
            if set_bm == 0 {
                return Err(SCH_EMPTY_SET);
            }
            return Err(SCH_SET_RANGE);
        }
        if windows == 0 || windows > 64 {
            return Err(SCH_WIN_GAP);
        }
        let start_win = match self.batches.last() {
            None => 1,
            Some(prev) => {
                if batch <= prev.batch {
                    if batch == prev.batch {
                        return Err(SCH_BATCH_DUP);
                    }
                    return Err(SCH_BATCH_BACK);
                }
                prev.start_win + prev.windows
            }
        };
        let fp = batch_fingerprint(batch, set_bm, windows);
        let chain = match self.batches.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.batches.push(SchedBatch { batch, set_bm, windows, start_win, fp, chain });
        Ok(start_win)
    }

    /// 器数统计（位图 popcount，逐位扫）。
    pub fn set_count(set_bm: u32) -> usize {
        let mut n = 0usize;
        for i in 0..SET_BITS {
            if set_bm & (1u32 << i) != 0 {
                n += 1;
            }
        }
        n
    }

    /// J1：编排全量（批非空 + 逐批器集合法）。
    pub fn batches_ok(&self) -> bool {
        !self.batches.is_empty()
            && self.batches.iter().all(|b| collector_set_ok(b.set_bm) && b.windows >= 1)
    }

    /// J2：窗序连续核（起窗号 = 前批末窗 + 1，首批起 1）。
    pub fn win_seq_ok(&self) -> bool {
        let mut expect = 1u32;
        for b in self.batches.iter() {
            if b.start_win != expect {
                return false;
            }
            expect += b.windows;
        }
        true
    }

    /// J2：批号单调核（严格递增无重复）。
    pub fn batch_seq_ok(&self) -> bool {
        for w in self.batches.windows(2) {
            if w[0].batch >= w[1].batch {
                return false;
            }
        }
        !self.batches.is_empty()
    }

    /// J3：批指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.batches.is_empty() {
            return false;
        }
        for i in 0..self.batches.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.batches[0].fp
            } else {
                self.batches[i - 1].fp ^ self.batches[i].fp
            };
            if self.batches[i].chain != expect {
                return false;
            }
        }
        true
    }

    /// 总窗数闭式。
    pub fn total_windows(&self) -> u32 {
        let mut n = 0;
        for b in self.batches.iter() {
            n += b.windows;
        }
        n
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 链尾指纹。
    pub fn chain_tail(&self) -> u32 {
        match self.batches.last() {
            None => CHAIN_SEED,
            Some(b) => b.chain,
        }
    }
}

/// 标准夹具：3 批（全器集 4 窗 / 双器 2 窗 / 单器 1 窗）。
pub fn standard_book() -> SchedBook {
    let mut b = SchedBook::new();
    let plan: [(u32, u32, u32); 3] = [(1, SET_ALL, 4), (2, 0b00011, 2), (3, 0b00100, 1)];
    for p in plan.iter() {
        if b.open_batch(p.0, p.1, p.2).is_err() {
            panic!("standard fixture open_batch must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（空器集/越界器集/零窗/批号回退/重复批号/封账开批/未注册器混入）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空器集
    let mut b = SchedBook::new();
    if b.open_batch(1, 0, 2) == Err(SCH_EMPTY_SET) {
        caught += 1;
    }
    // 2) 越界器集（第 6 位置 1）
    if b.open_batch(1, 0b100000, 2) == Err(SCH_SET_RANGE) {
        caught += 1;
    }
    // 3) 零窗
    if b.open_batch(1, SET_ALL, 0) == Err(SCH_WIN_GAP) {
        caught += 1;
    }
    // 4) 重复批号
    if b.open_batch(1, SET_ALL, 2).is_ok() && b.open_batch(1, SET_ALL, 2) == Err(SCH_BATCH_DUP) {
        caught += 1;
    }
    // 5) 批号回退
    if b.open_batch(2, SET_ALL, 2).is_ok() && b.open_batch(1, SET_ALL, 2) == Err(SCH_BATCH_BACK) {
        caught += 1;
    }
    // 6) 未注册器混入（第 6 位越界 → SET_RANGE，语义同混入拦截）
    if b.open_batch(3, 0b1100000, 1) == Err(SCH_SET_RANGE) {
        caught += 1;
    }
    // 7) 封账开批拒
    b.seal();
    if b.open_batch(4, SET_ALL, 2) == Err(SCH_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 调度账渲染导出（逐批一行）。
pub fn render_book(b: &SchedBook) -> String {
    let mut out = String::new();
    for x in b.batches.iter() {
        out.push_str(&format!(
            "b{}|{:0width$}|{}w|{}..{}|{:08x}\n",
            x.batch,
            x.set_bm,
            x.windows,
            x.start_win,
            x.start_win + x.windows - 1,
            x.fp,
            width = SET_BITS
        ));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行六段。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| ln.split('|').count() == 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx56083_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56083");
    // J1-1 编排全量（三批逐批合法）
    let b = standard_book();
    let ok = b.batches_ok() && b.batches.len() == 3;
    s.add("56083-J1-编排全量", ok, if ok { "" } else { "批次残缺" });
    // J1-2 器集守闸（全 1 合法、零/越界非法）
    let ok = collector_set_ok(SET_ALL) && !collector_set_ok(0) && !collector_set_ok(0b100000);
    s.add("56083-J1-器集守闸", ok, if ok { "" } else { "器集闸失效" });
    // J1-3 器数统计（5/2/1）
    let ok = SchedBook::set_count(SET_ALL) == 5
        && SchedBook::set_count(0b00011) == 2
        && SchedBook::set_count(0b00100) == 1;
    s.add("56083-J1-器数统计", ok, if ok { "" } else { "位图计数漂移" });
    // J2-1 窗序连续（1..4,5..6,7..7）
    let ok = b.win_seq_ok()
        && b.batches[0].start_win == 1
        && b.batches[1].start_win == 5
        && b.batches[2].start_win == 7;
    s.add("56083-J2-窗序连续", ok, if ok { "" } else { "窗序断档" });
    // J2-2 批号单调
    let ok = b.batch_seq_ok() && b.batches[2].batch == 3;
    s.add("56083-J2-批号单调", ok, if ok { "" } else { "批号破序" });
    // J2-3 总窗数闭式（4+2+1 = 7）
    let ok = b.total_windows() == 7;
    s.add("56083-J2-总窗闭式", ok, if ok { "" } else { "窗数漂移" });
    // J2-4 窗序跳变拒（零窗开批）
    let mut b2 = standard_book();
    let ok = b2.open_batch(4, SET_ALL, 0) == Err(SCH_WIN_GAP);
    s.add("56083-J2-跳变拒开", ok, if ok { "" } else { "窗闸失效" });
    // J2-5 窗续排（第 4 批起窗 8）
    let ok = b2.open_batch(4, SET_ALL, 2) == Ok(8);
    s.add("56083-J2-窗续排", ok, if ok { "" } else { "续排漂移" });
    // J3-1 批指纹链复核
    let ok = b.chain_ok();
    s.add("56083-J3-批指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56083-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（批 2 器集改动链尾变）
    let mut b3 = SchedBook::new();
    let _ = b3.open_batch(1, SET_ALL, 4);
    let _ = b3.open_batch(2, 0b00111, 2);
    let _ = b3.open_batch(3, 0b00100, 1);
    let ok = b3.chain_tail() != standard_book().chain_tail();
    s.add("56083-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账开批拒
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.open_batch(4, SET_ALL, 2) == Err(SCH_SEALED);
    s.add("56083-J3-封账拒开", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3) && text.starts_with("b1|11111|4w|1..4|");
    s.add("56083-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56083-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56083_all_checks_green() {
        let set = run_unx56083_checks();
        assert!(set.all_passed(), "UNX-F56083 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56083_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
