//! VE-F0758 布尔一致性（跨平台确定性）
//! 职责：浮点确定性纪律（无 FMA 依赖/无并行重排/排序全序化）、
//!       执行轨迹快照（操作序列哈希）、同轨迹必同结果、平台档案登记。
//! 契约：同输入同轨迹哈希；结果哈希由轨迹哈希决定（不依赖平台）。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::{FillRule, Poly, DirEdge, scan_row, inside_by_rule};

/// 操作记录
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Op {
    ScanRow(f64),
    Winding,
    SortX,
}

/// 执行轨迹
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Trace {
    pub ops: Vec<Op>,
}

impl Trace {
    pub fn new() -> Trace {
        Trace::default()
    }
    pub fn push(&mut self, op: Op) {
        self.ops.push(op);
    }
    /// 轨迹哈希（FNV-1a，逐操作 opcode + 参数位）
    pub fn hash(&self) -> u64 {
        let mut h = 0xcbf29ce484222325u64;
        for op in &self.ops {
            let (code, bits) = match op {
                Op::ScanRow(y) => (1u64, y.to_bits()),
                Op::Winding => (2, 0),
                Op::SortX => (3, 0),
            };
            h ^= code;
            h = h.wrapping_mul(0x100000001b3);
            h ^= bits;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// 确定性扫描：显式全序排序（strict total order on (x, winding)）
pub fn scan_ordered(edges: &[DirEdge], y: f64, trace: &mut Trace) -> Vec<(f64, i32)> {
    trace.push(Op::ScanRow(y));
    let mut evs: Vec<(f64, i32)> = alloc::vec![];
    for e in edges {
        let (y0, y1) = (e.a.y.min(e.b.y), e.a.y.max(e.b.y));
        if y >= y0 && y < y1 {
            let t = (y - e.a.y) / (e.b.y - e.a.y);
            let x = e.a.x + t * (e.b.x - e.a.x);
            evs.push((x, if e.b.y > e.a.y { 1 } else { -1 }));
        }
    }
    trace.push(Op::SortX);
    // 全序：先 x 后 winding（即使 x 相等也稳定）
    evs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    evs
}

/// 确定性环绕判定
pub fn inside_ordered(evs: &[(f64, i32)], trace: &mut Trace) -> Vec<bool> {
    trace.push(Op::Winding);
    let mut out: Vec<bool> = alloc::vec![];
    let mut wind = 0i32;
    for (x, w) in evs {
        let _ = x;
        wind += w;
        out.push(wind != 0);
    }
    out
}

/// 结果摘要哈希（内部位串 → u64）
pub fn result_hash(flags: &[bool]) -> u64 {
    let mut h = 0x811c9dc5u64;
    for b in flags {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 平台档案
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Platform {
    pub name: &'static str,
    /// 是否启用 FMA（本项目禁用）
    pub fma: bool,
    /// 是否并行扫描（本项目禁用）
    pub parallel: bool,
}

/// 合规平台（确定性纪律：串行 + 无 FMA 重排）
pub fn platform_ok(p: Platform) -> bool {
    !p.fma && !p.parallel
}

pub fn run_vec358_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec358");
    let square = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    let edges = square.edges();
    // 族一：轨迹哈希
    let mut t1 = Trace::new();
    scan_ordered(&edges, 2.0, &mut t1);
    inside_ordered(&scan_ordered(&edges, 2.0, &mut Trace::new()), &mut t1);
    let h1 = t1.hash();
    // 重跑
    let mut t2 = Trace::new();
    scan_ordered(&edges, 2.0, &mut t2);
    inside_ordered(&scan_ordered(&edges, 2.0, &mut Trace::new()), &mut t2);
    s.add("轨迹-同序同哈希", t2.hash() == h1, "same ops same hash");
    // 不同操作序列不同哈希
    let mut t3 = Trace::new();
    scan_ordered(&edges, 1.0, &mut t3);
    s.add("轨迹-异序异哈希", t3.hash() != h1, "diff ops diff hash");
    // 空轨迹
    s.add("轨迹-空哈希", Trace::new().hash() == 0xcbf29ce484222325, "FNV offset basis");
    // 族二：确定性扫描
    let mut ta = Trace::new();
    let eva = scan_ordered(&edges, 2.0, &mut ta);
    let mut tb = Trace::new();
    let evb = scan_ordered(&edges, 2.0, &mut tb);
    s.add("扫描-同果", eva == evb, "same events");
    s.add("扫描-有序", eva.windows(2).all(|w| w[0].0 <= w[1].0), "x sorted");
    // 全序稳定性：x 相等时按 winding
    let tie_evs = alloc::vec![(1.0, 1), (1.0, -1), (2.0, 1)];
    let mut tie = tie_evs.clone();
    tie.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    s.add("全序-winding次键", tie == alloc::vec![(1.0, -1), (1.0, 1), (2.0, 1)], "winding tiebreak");
    // 族三：结果哈希
    let flags = inside_ordered(&eva, &mut ta);
    s.add("判定-单区间", flags == alloc::vec![true], "inside between");
    let rh = result_hash(&flags);
    s.add("结果-同果同哈希", result_hash(&inside_ordered(&evb, &mut tb)) == rh, "same flags same hash");
    s.add("结果-异果异哈希", result_hash(&alloc::vec![false]) != rh, "diff flags diff hash");
    s.add("结果-空哈希", result_hash(&alloc::vec![]) == 0x811c9dc5, "FNV-1a basis");
    // 族四：跨 y 行一致性
    let mut tx1 = Trace::new();
    let f1 = inside_ordered(&scan_ordered(&edges, 1.5, &mut tx1), &mut tx1);
    let mut tx2 = Trace::new();
    let f2 = inside_ordered(&scan_ordered(&edges, 2.5, &mut tx2), &mut tx2);
    s.add("跨行-同判定", f1 == f2, "same rows same flags");
    s.add("跨行-轨迹异", tx1.hash() != tx2.hash(), "different y → different trace");
    // 族五：平台档案
    let good = Platform { name: "wasm-serial", fma: false, parallel: false };
    let bad1 = Platform { name: "x86-fma", fma: true, parallel: false };
    let bad2 = Platform { name: "gpu-parallel", fma: false, parallel: true };
    s.add("平台-串行合规", platform_ok(good), "serial ok");
    s.add("平台-FMA违规", !platform_ok(bad1), "fma rejected");
    s.add("平台-并行违规", !platform_ok(bad2), "parallel rejected");
    // 族六：规则一致性（奇偶与非零在单环上同判定）
    let eo_flags: Vec<bool> = {
        let evs = scan_ordered(&edges, 2.0, &mut Trace::new());
        let mut out = alloc::vec![];
        let mut wind = 0i32;
        for (_, w) in &evs {
            wind += w;
            out.push(wind % 2 != 0);
        }
        out
    };
    s.add("规则-单环同", eo_flags == flags, "evenodd == nonzero here");
    let _ = FillRule::NonZero;
    s
}
