//! UNX-F11225 brk/堆初始化消费面（0x1f825 段）
//!
//! 锚点：UNX-F11225-J1 brk 语义（cur_brk 查询/扩展/收缩三操作）消费判据绿，
//! 堆起点对齐与 Linux 一致。
//! 域：CoRun·UNX-C5-B02——cur_brk 是单调游标，扩展只能向上、收缩不得穿底，
//! 所有增量按十六字节对齐；任何一步越界都记拒并保留原游标。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f825;
pub const BR_ZERO: u32 = SEG_BASE + 0x01; // 增量为零
pub const BR_ALIGN: u32 = SEG_BASE + 0x02; // 增量未十六对齐
pub const BR_SHRINK: u32 = SEG_BASE + 0x03; // 收缩穿底
pub const BR_RANGE: u32 = SEG_BASE + 0x04; // 超出上界
pub const BR_ORDER: u32 = SEG_BASE + 0x05; // 操作数不足
pub const BR_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const BR_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 对齐粒度与上界。
pub const GRAIN: u64 = 16;
pub const LIMIT: u64 = 0x7fff_ffff_f000;
pub const WANT_OPS: usize = 20;
pub const CAP: usize = 40;
pub const BR_TAG: u64 = 0x1f825c1;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一次 brk 操作。
#[derive(Clone, Debug)]
pub struct Op {
    pub kind: &'static str,
    pub from: u64,
    pub to: u64,
}

/// 堆游标模型。
#[derive(Clone, Debug)]
pub struct HeapModel {
    pub start: u64,
    pub cur: u64,
    pub ops: Vec<Op>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl HeapModel {
    pub fn new(start: u64) -> HeapModel {
        HeapModel { start, cur: start, ops: Vec::new(), frozen: false, root: None }
    }

    pub fn start_ok(&self) -> bool {
        self.start != 0 && self.start % 4096 == 0
    }

    pub fn query(&self) -> u64 {
        self.cur
    }

    fn push(&mut self, kind: &'static str, from: u64, to: u64) {
        self.ops.push(Op { kind, from, to });
    }

    /// 扩展：零/未对齐/越上界三路拒。
    pub fn grow(&mut self, delta: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(BR_FROZEN);
        }
        if delta == 0 {
            return Err(BR_ZERO);
        }
        if delta % GRAIN != 0 {
            return Err(BR_ALIGN);
        }
        let to = self.cur.saturating_add(delta);
        if to > LIMIT {
            return Err(BR_RANGE);
        }
        if self.ops.len() >= CAP {
            return Err(BR_OVER);
        }
        let from = self.cur;
        self.cur = to;
        self.push("grow", from, to);
        Ok(to)
    }

    /// 收缩：零/未对齐/穿底三路拒。
    pub fn shrink(&mut self, delta: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(BR_FROZEN);
        }
        if delta == 0 {
            return Err(BR_ZERO);
        }
        if delta % GRAIN != 0 {
            return Err(BR_ALIGN);
        }
        if delta > self.cur.saturating_sub(self.start) {
            return Err(BR_SHRINK);
        }
        if self.ops.len() >= CAP {
            return Err(BR_OVER);
        }
        let from = self.cur;
        self.cur -= delta;
        self.push("shrink", from, self.cur);
        Ok(self.cur)
    }

    pub fn aligned(&self) -> bool {
        self.cur % GRAIN == 0 && self.start % GRAIN == 0
    }

    pub fn contiguous(&self) -> bool {
        let mut k = 0usize;
        let mut at = self.start;
        while k < self.ops.len() {
            if self.ops[k].from != at {
                return false;
            }
            at = self.ops[k].to;
            k += 1;
        }
        at == self.cur
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(BR_FROZEN);
        }
        if !self.start_ok() || !self.aligned() {
            return Err(BR_ALIGN);
        }
        if self.ops.len() != WANT_OPS {
            return Err(BR_ORDER);
        }
        if !self.contiguous() {
            return Err(BR_ORDER);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("brk 0x{:x} ops {} cur 0x{:x}", self.start, self.ops.len(), self.cur)
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = BR_TAG;
        let mut k = 0usize;
        while k < self.ops.len() {
            let b = self.ops[k].kind.as_bytes();
            let mut i = 0usize;
            while i < b.len() {
                h = (h ^ b[i] as u64).wrapping_mul(FP_STEP);
                i += 1;
            }
            h = (h ^ self.ops[k].from).wrapping_mul(FP_STEP);
            h = (h ^ self.ops[k].to).wrapping_mul(FP_STEP);
            k += 1;
        }
        h = (h ^ self.cur).wrapping_mul(FP_STEP);
        h
    }
}

/// Linux 风格堆起点（页对齐）。
pub const HEAP_START: u64 = 0x55d0_0000_0000;

/// 组一张二十步交替扩缩的账。
pub fn build() -> HeapModel {
    let mut h = HeapModel::new(HEAP_START);
    let mut k = 0usize;
    while k < WANT_OPS {
        if k % 2 == 0 {
            let _ = h.grow(0x4000);
        } else {
            let _ = h.shrink(0x1000);
        }
        k += 1;
    }
    h
}

/// 四路对抗：零增量、未对齐、收缩穿底、越上界。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = HeapModel::new(HEAP_START);
    out[0] = a.grow(0) == Err(BR_ZERO);
    out[1] = a.grow(24) == Err(BR_ALIGN);
    out[2] = a.shrink(16) == Err(BR_SHRINK);
    let mut b = HeapModel::new(LIMIT);
    out[3] = b.grow(GRAIN) == Err(BR_RANGE);
    out
}

/// 判据入口。
pub fn run_unx11225_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11225");
    let mut h = build();
    let root = h.freeze().unwrap_or(0);

    s.add("J1-起点对齐", h.start_ok(), "堆起点按四页对齐");
    s.add("J2-cur 查询", h.query() == HEAP_START + 10 * 0x4000 - 10 * 0x1000, "cur_brk 查询等于十扩十缩的净位置");
    s.add("J3-扩展", {
        let mut x = HeapModel::new(HEAP_START);
        x.grow(0x1000).unwrap_or(0) == HEAP_START + 0x1000
    }, "一次扩展后游标上移一页");
    s.add("J4-收缩", {
        let mut x = HeapModel::new(HEAP_START);
        let _ = x.grow(0x2000);
        x.shrink(0x1000).unwrap_or(0) == HEAP_START + 0x1000
    }, "收缩一页后游标回落一页");
    s.add("J5-缩不穿底", {
        let mut x = HeapModel::new(HEAP_START);
        x.shrink(0x1000) == Err(BR_SHRINK)
    }, "未扩展即收缩拒为 BR_SHRINK");
    s.add("J6-十六对齐", h.aligned(), "起点与游标均为十六字节对齐");
    s.add("J7-连续账", h.contiguous(), "二十步 from/to 首尾相接");
    s.add("J8-步数二十", h.ops.len() == WANT_OPS, "操作账恰二十步");
    s.add("J9-冻存根", root != 0 && h.root == Some(root), "堆账冻存产出非零根");

    s.add("J10-冻后扩拒", h.grow(GRAIN) == Err(BR_FROZEN), "冻存后扩展拒为 BR_FROZEN");
    s.add("J11-冻后缩拒", h.shrink(GRAIN) == Err(BR_FROZEN), "冻存后收缩拒为 BR_FROZEN");

    let four = inject_four();
    s.add("J12-四路注入", four.iter().all(|x| *x), "零增量/未对齐/穿底/越上界四路全拒");
    s.add_dyn(
        "J13-游标明细",
        h.cur > HEAP_START && h.ops.len() == WANT_OPS,
        &format!("起点 0x{:x}，终点 0x{:x}，净增 {} 字节", h.start, h.cur, h.cur - h.start),
    );
    s.add_dyn(
        "J14-账根",
        root == h.fold(),
        &format!("根 0x{:x}，末步 to 0x{:x}", root, h.ops[19].to),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11225_brk_heap_flow() {
        let mut h = build();
        assert!(h.contiguous());
        assert_eq!(h.ops.len(), 20);
        let r = h.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(h.grow(16), Err(BR_FROZEN));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11225_checks_green() {
        let s = run_unx11225_checks();
        assert!(s.all_passed(), "UNX-F11225 判据未全绿");
    }
}
