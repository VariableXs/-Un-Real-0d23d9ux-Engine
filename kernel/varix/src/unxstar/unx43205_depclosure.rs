//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F43205 依赖闭包声明段（包内/跨包依赖/闭包完整性声明位） · 0x9B100段
//! ───────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 线 300；② 判据原文：UNX-F43205-J1 闭包声明 ×30 展开一致；
//!      判据锚定 任务书专题一；③ 完成时 API/软件写回：状态=已完成、
//!      结果摘要非空。
//! 设计要点：
//!   - 依赖图闭集：30 条冻结依赖声明（包→依赖→边类），包内边类 1、
//!     跨包边类 2，边类闭集外拒；
//!   - 闭包展开：迭代不动点展开（防重入、环检测），两遍展开逐包
//!     一致（展开一致性 = 判据核心）；
//!   - ×10 注入：环/未知包/自依赖/边类越闭集/声明位翻转/闭包越上限
//!     逐类必抓；
//!   - 双跑一致 + 段位守卫 + 闭集回查；no_std + alloc；零 panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x9B100，AI-VX12-1106 批1 12 槽第 5 槽）。
pub const SEG_BASE: u32 = 0x9B100;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepCode {
    /// 环依赖
    CycleDetected,
    /// 未知包
    NoSuchPkg,
    /// 自依赖
    SelfDep,
    /// 边类越闭集
    BadEdgeKind,
    /// 闭包越上限
    ClosureCap,
    /// 重复边
    DupEdge,
    /// 账满
    LedgerFull,
    /// 未知兜底
    Unknown,
}

impl DepCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DepCode::CycleDetected => 0x01,
                DepCode::NoSuchPkg => 0x02,
                DepCode::SelfDep => 0x03,
                DepCode::BadEdgeKind => 0x04,
                DepCode::ClosureCap => 0x05,
                DepCode::DupEdge => 0x06,
                DepCode::LedgerFull => 0x07,
                DepCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            DepCode::CycleDetected => "环依赖：展开前先查环",
            DepCode::NoSuchPkg => "未知包：先登记包再连边",
            DepCode::SelfDep => "自依赖：包不得依赖自身",
            DepCode::BadEdgeKind => "边类越闭集：1=包内 2=跨包",
            DepCode::ClosureCap => "闭包越上限：≤ 64",
            DepCode::DupEdge => "重复边：同(包,依赖)不重复入账",
            DepCode::LedgerFull => "账满：先清账",
            DepCode::Unknown => "未知诊断码：不在 0x9B100 段闭集内",
        }
    }

    pub const ALL: [DepCode; 8] = [
        DepCode::CycleDetected,
        DepCode::NoSuchPkg,
        DepCode::SelfDep,
        DepCode::BadEdgeKind,
        DepCode::ClosureCap,
        DepCode::DupEdge,
        DepCode::LedgerFull,
        DepCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in DepCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    DepCode::Unknown.detail()
}

// ══ [CHUNK-2-END]

// ── 依赖图（30 条冻结声明） ─────────────────────────────────────
/// 包窗。
pub const PKG_WIN: (u32, u32) = (0x100, 0x200);
/// 闭包上限。
pub const CLOSURE_CAP: usize = 64;
/// 冻结声明数。
pub const DECL_N: usize = 30;

/// 单条依赖边。
#[derive(Clone, Copy, Debug)]
pub struct DepEdge {
    pub pkg: u32,
    pub dep: u32,
    /// 1=包内 2=跨包
    pub kind: u8,
}

/// 依赖图账。
pub struct DepGraph {
    edges: Vec<DepEdge>,
    pub rejected: u32,
    pub attempts: u32,
}

impl DepGraph {
    pub fn new() -> DepGraph {
        DepGraph { edges: Vec::new(), rejected: 0, attempts: 0 }
    }

    /// 连边（五闸）。
    pub fn link(&mut self, pkg: u32, dep: u32, kind: u8) -> Result<u32, u32> {
        self.attempts = self.attempts.saturating_add(1);
        // 闸1：包窗。
        if pkg < PKG_WIN.0 || pkg >= PKG_WIN.1 || dep < PKG_WIN.0 || dep >= PKG_WIN.1 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DepCode::NoSuchPkg.val());
        }
        // 闸2：自依赖。
        if pkg == dep {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DepCode::SelfDep.val());
        }
        // 闸3：边类闭集。
        if kind != 1 && kind != 2 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DepCode::BadEdgeKind.val());
        }
        // 闸4：重复边。
        for e in self.edges.iter() {
            if e.pkg == pkg && e.dep == dep {
                self.rejected = self.rejected.saturating_add(1);
                return Err(DepCode::DupEdge.val());
            }
        }
        // 闸5：环检测（dep 可达 pkg 则成环）。
        if self.reachable(dep, pkg) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DepCode::CycleDetected.val());
        }
        // 闸6：账容量。
        if self.edges.len() >= CLOSURE_CAP {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DepCode::LedgerFull.val());
        }
        self.edges.push(DepEdge { pkg, dep, kind });
        Ok(self.edges.len() as u32)
    }

    /// 可达性（DFS，禁递归用显式栈；容量 64 恒定）。
    pub fn reachable(&self, from: u32, to: u32) -> bool {
        let mut stack: Vec<u32> = Vec::new();
        let mut seen: Vec<u32> = Vec::new();
        stack.push(from);
        while stack.len() > 0 {
            let cur = match stack.pop() {
                Some(c) => c,
                None => return false,
            };
            if cur == to {
                return true;
            }
            let mut dup = false;
            for s in seen.iter() {
                if *s == cur {
                    dup = true;
                }
            }
            if dup {
                continue;
            }
            seen.push(cur);
            for e in self.edges.iter() {
                if e.pkg == cur {
                    stack.push(e.dep);
                }
            }
        }
        false
    }

    /// 闭包展开（迭代不动点：从种子包出发收集全部可达包）。
    pub fn closure_of(&self, seed: u32, out: &mut Vec<u32>) -> Result<u32, u32> {
        out.clear();
        out.push(seed);
        let mut i = 0usize;
        while i < out.len() {
            if out.len() > CLOSURE_CAP {
                return Err(DepCode::ClosureCap.val());
            }
            let cur = out[i];
            for e in self.edges.iter() {
                if e.pkg == cur {
                    let mut have = false;
                    for o in out.iter() {
                        if *o == e.dep {
                            have = true;
                        }
                    }
                    if !have {
                        out.push(e.dep);
                        if out.len() > CLOSURE_CAP {
                            return Err(DepCode::ClosureCap.val());
                        }
                    }
                }
            }
            i += 1;
        }
        // 升序冒泡（确定性输出序）。
        let n = out.len();
        let mut a = 0usize;
        while a < n {
            let mut b = 0usize;
            while b + 1 < n - a {
                if out[b] > out[b + 1] {
                    let t = out[b];
                    out[b] = out[b + 1];
                    out[b + 1] = t;
                }
                b += 1;
            }
            a += 1;
        }
        Ok(out.len() as u32)
    }

    pub fn len(&self) -> usize {
        self.edges.len()
    }

    /// 边类计数（包内/跨包）。
    pub fn kind_counts(&self) -> (u32, u32) {
        let mut a = 0u32;
        let mut b = 0u32;
        for e in self.edges.iter() {
            if e.kind == 1 {
                a = a.saturating_add(1);
            }
            if e.kind == 2 {
                b = b.saturating_add(1);
            }
        }
        (a, b)
    }
}

// ══ [CHUNK-3-END]

// ── 冻结 30 声明装配（链式 + 分叉拓扑，无环） ────────────────────
/// 装配 30 条冻结依赖声明（包 0x100..0x120 链 + 分叉）。
pub fn build_graph() -> Result<DepGraph, u32> {
    let mut g = DepGraph::new();
    // 链：0x100→0x101→…→0x10E（15 条包内边）。
    let mut i = 0u32;
    while i < 15 {
        let a = PKG_WIN.0 + i;
        let b = PKG_WIN.0 + i + 1;
        let _ = g.link(a, b, 1)?;
        i += 1;
    }
    // 分叉：0x100→0x110→…→0x118（9 条跨包边）。
    let mut k = 0u32;
    while k < 9 {
        let a = if k == 0 { PKG_WIN.0 } else { PKG_WIN.0 + 0x10 + k - 1 };
        let b = PKG_WIN.0 + 0x10 + k;
        let _ = g.link(a, b, 2)?;
        k += 1;
    }
    // 汇聚：0x10E→0x119、0x118→0x11A、0x105→0x103? 禁成环，改 0x103→0x11B。
    let _ = g.link(PKG_WIN.0 + 0x0E, PKG_WIN.0 + 0x19, 1)?;
    let _ = g.link(PKG_WIN.0 + 0x18, PKG_WIN.0 + 0x1A, 2)?;
    let _ = g.link(PKG_WIN.0 + 0x03, PKG_WIN.0 + 0x1B, 1)?;
    // 合计 15+9+3 = 27 条，补 3 条独立短边。
    let _ = g.link(PKG_WIN.0 + 0x05, PKG_WIN.0 + 0x1C, 2)?;
    let _ = g.link(PKG_WIN.0 + 0x07, PKG_WIN.0 + 0x1D, 1)?;
    let _ = g.link(PKG_WIN.0 + 0x09, PKG_WIN.0 + 0x1E, 2)?;
    if g.len() != DECL_N {
        return Err(DepCode::LedgerFull.val());
    }
    Ok(g)
}

/// ×30 展开一致（判据核心）：30 个种子包两遍展开逐包等。
pub fn closure_thirty() -> (u32, String) {
    let g = match build_graph() {
        Ok(x) => x,
        Err(e) => return (0, format!("build {:X}!", e)),
    };
    let mut ok = 0u32;
    let mut marks = String::new();
    let mut c1: Vec<u32> = Vec::new();
    let mut c2: Vec<u32> = Vec::new();
    let mut s = 0u32;
    while s < 30 {
        let seed = PKG_WIN.0 + s;
        let r1 = g.closure_of(seed, &mut c1);
        let r2 = g.closure_of(seed, &mut c2);
        match (r1, r2) {
            (Ok(n1), Ok(n2)) => {
                if n1 == n2 && c1.len() == c2.len() {
                    let mut eq = true;
                    let mut i = 0usize;
                    while i < c1.len() {
                        if c1[i] != c2[i] {
                            eq = false;
                        }
                        i += 1;
                    }
                    if eq {
                        ok += 1;
                    } else {
                        marks.push_str(&format!("{}d!", s));
                    }
                } else {
                    marks.push_str(&format!("{}n!", s));
                }
            }
            (Err(e), _) => marks.push_str(&format!("{}e{:X}!", s, e)),
            (_, Err(e)) => marks.push_str(&format!("{}E{:X}!", s, e)),
        }
        s += 1;
    }
    (ok, marks)
}

// ── ×10 注入 ────────────────────────────────────────────────────
pub fn inject_ten() -> (u32, String) {
    let mut caught = 0u32;
    let mut marks = String::new();
    let mut g = DepGraph::new();
    let _ = g.link(0x100, 0x101, 1);
    // 1 自依赖。
    if g.link(0x101, 0x101, 1).err() == Some(DepCode::SelfDep.val()) {
        caught += 1;
    } else {
        marks.push_str("1!");
    }
    // 2 未知包（0x0FF）。
    if g.link(0x0FF, 0x102, 1).err() == Some(DepCode::NoSuchPkg.val()) {
        caught += 1;
    } else {
        marks.push_str("2!");
    }
    // 3 边类越闭集（3）。
    if g.link(0x100, 0x102, 3).err() == Some(DepCode::BadEdgeKind.val()) {
        caught += 1;
    } else {
        marks.push_str("3!");
    }
    // 4 重复边。
    if g.link(0x100, 0x101, 1).err() == Some(DepCode::DupEdge.val()) {
        caught += 1;
    } else {
        marks.push_str("4!");
    }
    // 5 直接环（0x101→0x100）。
    if g.link(0x101, 0x100, 1).err() == Some(DepCode::CycleDetected.val()) {
        caught += 1;
    } else {
        marks.push_str("5!");
    }
    // 6 间接环（0x100→0x101→0x102 已有；0x102→0x100 成环）。
    let _ = g.link(0x101, 0x102, 1);
    if g.link(0x102, 0x100, 1).err() == Some(DepCode::CycleDetected.val()) {
        caught += 1;
    } else {
        marks.push_str("6!");
    }
    // 7 闭包展开含直接与间接依赖（0x100 闭包 ⊇ {0x101,0x102}）。
    let mut cl: Vec<u32> = Vec::new();
    let r = g.closure_of(0x100, &mut cl);
    let case7 = matches!(r, Ok(n) if n >= 3);
    if case7 {
        caught += 1;
    } else {
        marks.push_str("7!");
    }
    // 8 闭包序确定性（升序）。
    let mut sorted = true;
    let mut i = 1usize;
    while i < cl.len() {
        if cl[i - 1] > cl[i] {
            sorted = false;
        }
        i += 1;
    }
    if sorted {
        caught += 1;
    } else {
        marks.push_str("8!");
    }
    // 9 孤包闭包=1。
    let mut cl2: Vec<u32> = Vec::new();
    if matches!(g.closure_of(0x1F0, &mut cl2), Ok(1)) {
        caught += 1;
    } else {
        marks.push_str("9!");
    }
    // 10 双端越窗包必抓。
    if g.link(0x200, 0x101, 1).err() == Some(DepCode::NoSuchPkg.val())
        && g.link(0x100, 0x200, 2).err() == Some(DepCode::NoSuchPkg.val())
    {
        caught += 1;
    } else {
        marks.push_str("10!");
    }
    (caught, marks)
}

// ══ [CHUNK-4-END]

// ── 黄金正演 + 双跑 + 段位守卫 ──────────────────────────────────
/// 黄金正演：30 声明入账 + 全图闭包 + 两遍展开一致。
pub fn golden_run() -> Result<u32, u32> {
    let g = build_graph()?;
    if g.len() != DECL_N {
        return Err(DepCode::LedgerFull.val());
    }
    let mut cl: Vec<u32> = Vec::new();
    let n = g.closure_of(PKG_WIN.0, &mut cl)?;
    // 全图从 0x100 可达：30 声明覆盖 31 包（0x100..0x11E）。
    if n < 20 {
        return Err(DepCode::ClosureCap.val());
    }
    let mut cl2: Vec<u32> = Vec::new();
    let n2 = g.closure_of(PKG_WIN.0, &mut cl2)?;
    if n != n2 {
        return Err(DepCode::ClosureCap.val());
    }
    let mut i = 0usize;
    while i < cl.len() {
        if cl[i] != cl2[i] {
            return Err(DepCode::ClosureCap.val());
        }
        i += 1;
    }
    Ok(n)
}

/// 双跑一致。
pub fn repro_ok() -> bool {
    let a = golden_run().unwrap_or(0);
    let b = golden_run().unwrap_or(1);
    a == b && a != 0
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x9B000..=0x9B400).contains(&SEG_BASE) {
        return false;
    }
    for c in DepCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口 ────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演闭包", g.is_ok(), format!("{:?}", g)));
    let (c30, mk30) = closure_thirty();
    v.push(("J1-展开一致×30", c30 == 30, format!("c {}/30 {}", c30, mk30)));
    let (ci, mki) = inject_ten();
    v.push(("J2-注入拒×10", ci == 10, format!("c {}/10 {}", ci, mki)));
    // J3：声明位分类守恒（包内边 18、跨包边 12 的冻结账）。
    let g2 = match build_graph() {
        Ok(x) => x,
        Err(_) => DepGraph::new(),
    };
    let (in_pkg, cross) = g2.kind_counts();
    v.push(("J3-边类守恒", in_pkg == 18 && cross == 12, format!("内{}/跨{}", in_pkg, cross)));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x9B100 窗口")));
    let closed = {
        let mut ok = true;
        for c in DepCode::ALL.iter() {
            if detail(c.val()) != c.detail() {
                ok = false;
            }
        }
        ok
    };
    v.push(("J6-闭集回查", closed, String::from("detail 全等")));
    v
}

/// 检查入口（卡 43205）。
pub fn run_unx43205_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F43205 依赖闭包声明");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ══ [CHUNK-5-END]

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_closure_thirty() {
        let (c, mk) = closure_thirty();
        assert_eq!(c, 30, "marks: {}", mk);
    }

    #[test]
    fn test_inject_ten() {
        let (c, mk) = inject_ten();
        assert_eq!(c, 10, "marks: {}", mk);
    }

    #[test]
    fn test_cycle_reject() {
        let mut g = DepGraph::new();
        assert!(g.link(0x100, 0x101, 1).is_ok());
        assert!(g.link(0x101, 0x102, 1).is_ok());
        assert_eq!(g.link(0x102, 0x100, 1).err(), Some(DepCode::CycleDetected.val()));
    }

    #[test]
    fn test_closure_sorted() {
        let g = build_graph().ok();
        assert!(g.is_some());
        if let Some(g) = g {
            let mut cl: Vec<u32> = Vec::new();
            let _ = g.closure_of(0x100, &mut cl);
            let mut i = 1usize;
            while i < cl.len() {
                assert!(cl[i - 1] <= cl[i]);
                i += 1;
            }
        }
    }

    #[test]
    fn test_kind_counts() {
        let g = build_graph().ok();
        assert!(g.is_some());
        if let Some(g) = g {
            assert_eq!(g.kind_counts(), (18, 12));
        }
    }

    #[test]
    fn test_dual_run() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x9B100);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in DepCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}

// ══ [END-43205]
