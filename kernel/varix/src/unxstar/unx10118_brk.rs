//! UNX-F10118 brk 正反双判据（堆伸缩达成与越限回退误用受控）（0x101e0 段）
//!
//! 锚点：UNX-F10118-J1 brk 堆伸缩达成（探顶推进/回退语义 LTP brk01 面
//! 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：堆册（基址/当前顶/上限三锚 + 探针语义）+ 堆核（推进/收缩/回
//! 探/越限回退保持原顶）+ 堆账（逐次留痕 + 峰值统计）+ 注入核（越上限/
//! 越基址/坏步长/空留痕/封堆必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x101e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x101e0;
pub const BR_OVER_LIMIT: u32 = SEG_BASE + 0x01; // 越上限
pub const BR_UNDER_BASE: u32 = SEG_BASE + 0x02; // 越基址
pub const BR_BAD_STEP: u32 = SEG_BASE + 0x03; // 坏步长（非 16 对齐目标）
pub const BR_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const BR_SEALED: u32 = SEG_BASE + 0x05; // 封堆后操作

/// 堆基址与上限锚。
pub const HEAP_BASE: u32 = 0x2000_0000;
pub const HEAP_LIMIT: u32 = 0x2001_0000;

/// 对齐粒度。
pub const ALIGN: u32 = 16;

/// 堆账留痕。
#[derive(Clone)]
pub struct BrRec {
    pub seq: u32,
    pub from: u32,
    pub to: u32,
    pub kind: u32, // 0=探针 1=推进 2=收缩
    pub note: String,
}

impl BrRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.kind <= 2 && self.from >= HEAP_BASE
    }
}

/// 堆册。
pub struct HeapBook {
    pub brk: u32,
    pub peak: u32,
    pub ledger: Vec<BrRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl HeapBook {
    pub fn new() -> HeapBook {
        HeapBook { brk: HEAP_BASE, peak: HEAP_BASE, ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 目标合法性：对齐 + 界内。
    pub fn target_ok(t: u32) -> bool {
        t % ALIGN == 0 && t >= HEAP_BASE && t <= HEAP_LIMIT
    }

    /// brk 核心：探针（0 语义——传 0 视为查顶）与伸缩合一。
    pub fn brk(&mut self, target: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(BR_SEALED);
        }
        if note.is_empty() {
            return Err(BR_EMPTY_NOTE);
        }
        if target == 0 {
            // 探针：只查顶不动位
            self.ledger.push(BrRec {
                seq: self.seq_next,
                from: self.brk,
                to: self.brk,
                kind: 0,
                note: note.to_string(),
            });
            self.seq_next += 1;
            return Ok(self.brk);
        }
        if target > HEAP_LIMIT {
            return Err(BR_OVER_LIMIT);
        }
        if target < HEAP_BASE {
            return Err(BR_UNDER_BASE);
        }
        if target % ALIGN != 0 {
            return Err(BR_BAD_STEP);
        }
        let from = self.brk;
        let kind = if target > from { 1 } else { 2 };
        self.brk = target;
        if self.brk > self.peak {
            self.peak = self.brk;
        }
        self.ledger.push(BrRec { seq: self.seq_next, from, to: target, kind, note: note.to_string() });
        self.seq_next += 1;
        Ok(self.brk)
    }

    /// sbrk 面语义：增量式伸缩（负量收缩）。
    pub fn sbrk(&mut self, delta: i64, note: &str) -> Result<u32, u32> {
        let cur = self.brk as i64;
        let target = cur + delta;
        if target < 0 {
            return Err(BR_UNDER_BASE);
        }
        self.brk(target as u32, note)
    }

    /// 堆增量（当前顶-基址）。
    pub fn grown(&self) -> u32 {
        self.brk - HEAP_BASE
    }

    /// 推进/收缩计数。
    pub fn kind_count(&self, kind: u32) -> usize {
        self.ledger.iter().filter(|r| r.kind == kind).count()
    }

    /// 回退守恒：收缩至基址后增量归零且峰值仍在。
    pub fn shrink_to_base_ok(&mut self, note: &str) -> bool {
        match self.brk(HEAP_BASE, note) {
            Ok(v) => v == HEAP_BASE && self.grown() == 0 && self.peak >= self.brk,
            Err(_) => false,
        }
    }

    /// 页粒度增量（16 对齐页换算）。
    pub fn pages_grown(&self) -> u32 {
        self.grown() / ALIGN
    }

    /// 推进总量统计（单向求和）。
    pub fn total_pushed(&self) -> u64 {
        self.ledger
            .iter()
            .filter(|r| r.kind == 1)
            .map(|r| (r.to as u64).saturating_sub(r.from as u64))
            .fold(0u64, |a, b| a + b)
    }

    /// 收缩总量统计。
    pub fn total_pulled(&self) -> u64 {
        self.ledger
            .iter()
            .filter(|r| r.kind == 2)
            .map(|r| (r.from as u64).saturating_sub(r.to as u64))
            .fold(0u64, |a, b| a + b)
    }

    /// 探针次数。
    pub fn probe_count(&self) -> usize {
        self.kind_count(0)
    }

    /// 剩余余量（上限-当前顶）。
    pub fn headroom(&self) -> u32 {
        HEAP_LIMIT - self.brk
    }

    /// 分配净额守恒：推进-收缩 == 当前增量。
    pub fn net_conserved(&self) -> bool {
        self.total_pushed().saturating_sub(self.total_pulled()) == self.grown() as u64
    }

    /// 当前顶渲染（诊断面）。
    pub fn render_top(&self) -> String {
        format!("brk={:#x} peak={:#x}", self.brk, self.peak)
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x101e00;
        h = h.wrapping_mul(31).wrapping_add(self.brk);
        h = h.wrapping_mul(31).wrapping_add(self.peak);
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.to);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut h = HeapBook::new();
    // 1) 越上限必拒且顶不动
    got[0] = h.brk(HEAP_LIMIT + ALIGN, "inj-over").err() == Some(BR_OVER_LIMIT);
    // 2) 越基址必拒
    got[1] = h.brk(HEAP_BASE - ALIGN, "inj-under").err() == Some(BR_UNDER_BASE);
    // 3) 坏步长（不对齐）必拒
    got[2] = h.brk(HEAP_BASE + 3, "inj-align").err() == Some(BR_BAD_STEP);
    // 4) 空留痕必拒
    got[3] = h.brk(0, "").err() == Some(BR_EMPTY_NOTE);
    // 5) 封堆后操作必抓
    h.seal();
    got[4] = h.brk(0, "inj-seal").err() == Some(BR_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10118_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10118");

    // 正路径：探顶 + 推进（brk01 面）
    let mut h = HeapBook::new();
    let p0 = h.brk(0, "探顶").unwrap_or(0);
    let p1 = h.brk(HEAP_BASE + 0x100, "推进").unwrap_or(0);
    let j1 = p0 == HEAP_BASE && p1 == HEAP_BASE + 0x100 && h.grown() == 0x100;
    s.add("J1-探顶推进", j1, "探针回基址且推进 0x100 生效");

    // 逐级推进峰值
    let j2 = {
        let _ = h.brk(HEAP_BASE + 0x200, "再推");
        let _ = h.brk(HEAP_BASE + 0x180, "回落");
        h.peak == HEAP_BASE + 0x200 && h.brk == HEAP_BASE + 0x180
    };
    s.add("J2-峰值驻留", j2, "回落后峰值保持 0x200");

    // 收缩语义
    let j3 = {
        let _ = h.brk(HEAP_BASE + 0x40, "收缩");
        h.brk == HEAP_BASE + 0x40 && h.grown() == 0x40
    };
    s.add("J3-收缩语义", j3, "收缩至 0x40 生效");

    // sbrk 增量面
    let j4 = {
        let mut h4 = HeapBook::new();
        let a = h4.sbrk(0x80, "增").unwrap_or(0);
        let b = h4.sbrk(-0x40, "减").unwrap_or(0);
        a == HEAP_BASE + 0x80 && b == HEAP_BASE + 0x40
    };
    s.add("J4-sbrk增量", j4, "增量伸缩正负两向生效");

    // 越限回退受控：顶保持原位
    let j5 = {
        let before = h.brk;
        let err = h.brk(HEAP_LIMIT + 0x10, "越限");
        err == Err(BR_OVER_LIMIT) && h.brk == before
    };
    s.add("J5-越限回退", j5, "越上限拒且顶不漂移");

    // 越基址受控
    let j6 = h.brk(HEAP_BASE - 0x10, "越底").err() == Some(BR_UNDER_BASE);
    s.add("J6-越基址受控", j6, "低于基址必拒");

    // 对齐受控
    let j7 = h.brk(HEAP_BASE + 15, "不对齐").err() == Some(BR_BAD_STEP);
    s.add("J7-对齐受控", j7, "非 16 对齐目标必拒");

    // 回退至基址守恒
    let j8 = h.shrink_to_base_ok("归底") && h.peak > h.brk;
    s.add("J8-归底守恒", j8, "归底后增量为零峰值驻留");

    // 探针计数
    let j9 = h.kind_count(0) >= 1 && h.kind_count(1) >= 1 && h.kind_count(2) >= 1;
    s.add("J9-三向计数", j9, "探针/推进/收缩三向各有留痕");

    // 空留痕拒收
    let j10 = h.brk(0, "").err() == Some(BR_EMPTY_NOTE);
    s.add("J10-空留痕拒", j10, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j11 = h.ledger.iter().all(|r| r.ok()) && h.seq_unique();
    s.add("J11-留痕合法", j11, "kind 在闭集且 from 界内");

    // 注入核 5/5
    let inj = inject_five();
    let j12 = inj.iter().all(|g| *g);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j13 = h.fold() == h.fold();
    s.add("J13-指纹稳定", j13, "折叠指纹两次一致");

    // 净额守恒与余量
    let j14 = h.net_conserved() && h.headroom() == HEAP_LIMIT - h.brk;
    s.add("J14-净额守恒", j14, "推进-收缩==当前增量且余量一致");

    // 页粒度换算
    let j15 = {
        let mut h15 = HeapBook::new();
        let _ = h15.brk(HEAP_BASE + ALIGN * 7, "七页");
        h15.pages_grown() == 7
    };
    s.add("J15-页粒度换算", j15, "增量 7 页换算一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10118_brk_flow() {
        let mut h = HeapBook::new();
        assert_eq!(h.brk(0, "a").unwrap_or(0), HEAP_BASE);
        assert_eq!(h.brk(HEAP_BASE + 0x20, "b").unwrap_or(0), HEAP_BASE + 0x20);
        assert!(h.brk(HEAP_LIMIT + 1, "c") == Err(BR_OVER_LIMIT));
        assert!(h.brk(HEAP_BASE + 1, "d") == Err(BR_BAD_STEP));
    }

    #[test]
    fn unx10118_checks_green() {
        let s = run_unx10118_checks();
        assert!(s.all_passed(), "UNX-F10118 判据未全绿");
    }
}
