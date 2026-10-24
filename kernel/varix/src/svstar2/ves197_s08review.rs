#![allow(dead_code)]
//! VE-F3943 · 无障碍审查器统一
//!
//! S 域审查器家族统一（F3151/F3750/F3813 复用统一框架——统一复用声明
//! 五用）。审查台三合一：工具审计 + 产出验证 + 渲染断链三台合一（复述
//! 三台合一全域化）。统一外壳复用框架（功能保全断言全域化）；跨器追踪。
//! 错误路径：功能丢失→插件补全；三台分歧→立案；追踪断链→修正；协议
//! 分歧→单源。
//! 性能：外壳 O(1)；插件 O(1)；保全 O(清单)；追踪 O(上下文)。
//! 跨批对接：F3342/F3151/F3750 三单源复用声明；S01-S07 审查器对端；
//! F2815 协议。无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x0B40）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x0B40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8CCode(pub u16);

impl S8CCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const FUNC_LOST: Self = Self(SEG_BASE + 1);
    pub const TRIAD_SPLIT: Self = Self(SEG_BASE + 2);
    pub const TRACE_BREAK: Self = Self(SEG_BASE + 3);
    pub const PROTO_SPLIT: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::FUNC_LOST => "功能丢失→插件补全",
            Self::TRIAD_SPLIT => "三台分歧→立案",
            Self::TRACE_BREAK => "追踪断链→修正",
            Self::PROTO_SPLIT => "协议分歧→单源",
            _ => "未知",
        }
    }
}

// ── 统一复用声明（五用）──────────────────────────────────────────────

/// 审查器家族单源声明：五处复用点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReusePoint {
    /// F3151 S02 审查器。
    P3151,
    /// F3750 S05 审查器。
    P3750,
    /// F3813 S06 审查器。
    P3813,
    /// F3342 统一框架外壳。
    P3342Shell,
    /// S08 本统一件。
    PThis,
}

pub const REUSE_POINTS: [ReusePoint; 5] = [
    ReusePoint::P3151,
    ReusePoint::P3750,
    ReusePoint::P3813,
    ReusePoint::P3342Shell,
    ReusePoint::PThis,
];

/// 统一复用声明账：五用逐点登记。
#[derive(Debug, Clone)]
pub struct ReuseLedger {
    pub declared: [bool; 5],
}

impl ReuseLedger {
    pub fn full() -> Self {
        Self { declared: [true; 5] }
    }

    pub fn count(&self) -> usize {
        self.declared.iter().filter(|d| **d).count()
    }

    pub fn complete(&self) -> bool {
        self.count() == 5
    }
}

// ── 审查台三合一 ─────────────────────────────────────────────────────

/// 审查台种别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchKind {
    /// 工具审计台。
    ToolAudit,
    /// 产出验证台。
    OutputVerify,
    /// 渲染断链台。
    RenderBreak,
}

/// 单台结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchVerdict {
    pub kind: BenchKind,
    /// 通过与否。
    pub pass: bool,
    /// 证据指纹（跨器追踪锚）。
    pub fp: u64,
}

/// 三合一审查外壳：同输入喂三台，结论必须一致。
#[derive(Debug, Clone)]
pub struct TriadBench {
    pub verdicts: Vec<BenchVerdict>,
}

impl TriadBench {
    pub fn new() -> Self {
        Self { verdicts: Vec::new() }
    }

    pub fn submit(&mut self, v: BenchVerdict) {
        self.verdicts.push(v);
    }

    /// 三台齐备检查。
    pub fn triad_complete(&self) -> bool {
        let mut seen = [false; 3];
        for v in self.verdicts.iter() {
            seen[v.kind as usize] = true;
        }
        seen[0] && seen[1] && seen[2]
    }

    /// 三台分歧检出：通过结论不一致即分歧（返回分歧对）。
    pub fn find_split(&self) -> Option<(BenchKind, BenchKind)> {
        let mut base: Option<(BenchKind, bool)> = None;
        for v in self.verdicts.iter() {
            match base {
                None => base = Some((v.kind, v.pass)),
                Some((_, bp)) if bp != v.pass => {
                    return Some((base.unwrap().0, v.kind));
                }
                _ => {}
            }
        }
        None
    }

    /// 立案：分歧即立案，返回错误码。
    pub fn adjudicate(&self) -> S8CCode {
        match self.find_split() {
            Some(_) => S8CCode::TRIAD_SPLIT,
            None if self.triad_complete() => S8CCode::OK,
            None => S8CCode::FUNC_LOST,
        }
    }
}

impl Default for TriadBench {
    fn default() -> Self {
        Self::new()
    }
}

// ── 功能保全断言（统一外壳不丢功能）──────────────────────────────────

/// 外壳功能清单：统一前逐项登记，统一后逐项核对。
#[derive(Debug, Clone)]
pub struct PreserveLedger {
    /// 功能名 → 统一外壳是否提供。
    pub items: Vec<(&'static str, bool)>,
}

impl PreserveLedger {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn register(&mut self, name: &'static str) {
        self.items.push((name, false));
    }

    /// 统一外壳提供功能：O(清单)。
    pub fn provide(&mut self, name: &str) -> S8CCode {
        match self.items.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => {
                slot.1 = true;
                S8CCode::OK
            }
            None => S8CCode::FUNC_LOST,
        }
    }

    /// 功能丢失检出：登记了但外壳不提供的项。
    pub fn lost(&self) -> Vec<&'static str> {
        self.items.iter().filter(|(_, p)| !*p).map(|(n, _)| *n).collect()
    }

    /// 插件补全：丢失项由插件补齐。
    pub fn patch(&mut self, name: &str) -> S8CCode {
        self.provide(name)
    }

    pub fn preserved(&self) -> bool {
        self.lost().is_empty()
    }
}

impl Default for PreserveLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 跨器追踪（上下文链）──────────────────────────────────────────────

/// 跨器追踪上下文：审查器 A → B → C 的证据指纹链。
#[derive(Debug, Clone)]
pub struct TraceChain {
    /// 链上指纹序（后一项必须引用前一项派生）。
    pub fps: Vec<u64>,
}

impl TraceChain {
    pub fn new() -> Self {
        Self { fps: Vec::new() }
    }

    /// 追加节点：新指纹必须由前指纹派生（异或混淆即派生）。
    pub fn push(&mut self, prev_derived: u64) {
        self.fps.push(prev_derived);
    }

    /// 断链检出：链上相邻节点无派生关系即断。
    pub fn find_break(&self) -> Option<usize> {
        for i in 1..self.fps.len() {
            // 派生规则：child = parent ^ mix，mix 由下标定。
            let expect = self.fps[i - 1] ^ (0x9E37_79B9_7F4A_7C15u64.wrapping_mul(i as u64));
            if self.fps[i] != expect {
                return Some(i);
            }
        }
        None
    }

    /// 修正：重派生第 i 节点之后全链。
    pub fn repair_from(&mut self, i: usize) -> S8CCode {
        if i == 0 || i >= self.fps.len() {
            return S8CCode::TRACE_BREAK;
        }
        for j in i..self.fps.len() {
            self.fps[j] = self.fps[j - 1] ^ (0x9E37_79B9_7F4A_7C15u64.wrapping_mul(j as u64));
        }
        S8CCode::OK
    }
}

impl Default for TraceChain {
    fn default() -> Self {
        Self::new()
    }
}

/// 协议单源仲裁：多器协议版本必须归一到单源版本。
#[derive(Debug, Clone)]
pub struct ProtoArbiter {
    /// 各器协议版本。
    pub versions: Vec<(u8, u32)>,
    /// 单源版本。
    pub canonical: u32,
}

impl ProtoArbiter {
    pub fn new(canonical: u32) -> Self {
        Self { versions: Vec::new(), canonical }
    }

    pub fn declare(&mut self, tool: u8, ver: u32) {
        self.versions.push((tool, ver));
    }

    /// 协议分歧检出。
    pub fn split(&self) -> Vec<u8> {
        self.versions.iter().filter(|(_, v)| *v != self.canonical).map(|(t, _)| *t).collect()
    }

    /// 单源化：全部对齐。
    pub fn unify(&mut self) -> usize {
        let c = self.canonical;
        let mut n = 0;
        for (t, v) in self.versions.iter_mut() {
            if *v != c {
                *v = c;
                n += 1;
                let _ = t;
            }
        }
        n
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves197_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves197_s08review");

    // 01 统一复用声明五用齐备
    {
        let l = ReuseLedger::full();
        chk(&mut set, "C3943-01", l.complete() && l.count() == 5, "err");
    }
    // 02 三台合一：齐备判正
    {
        let mut b = TriadBench::new();
        b.submit(BenchVerdict { kind: BenchKind::ToolAudit, pass: true, fp: 1 });
        b.submit(BenchVerdict { kind: BenchKind::OutputVerify, pass: true, fp: 2 });
        b.submit(BenchVerdict { kind: BenchKind::RenderBreak, pass: true, fp: 3 });
        chk(&mut set, "C3943-02", b.triad_complete() && b.adjudicate() == S8CCode::OK, "err");
    }
    // 03 三台分歧→立案实测
    {
        let mut b = TriadBench::new();
        b.submit(BenchVerdict { kind: BenchKind::ToolAudit, pass: true, fp: 1 });
        b.submit(BenchVerdict { kind: BenchKind::OutputVerify, pass: false, fp: 2 });
        b.submit(BenchVerdict { kind: BenchKind::RenderBreak, pass: true, fp: 3 });
        chk(&mut set, "C3943-03", b.find_split().is_some() && b.adjudicate() == S8CCode::TRIAD_SPLIT, "err");
    }
    // 04 缺台→功能丢失立案
    {
        let mut b = TriadBench::new();
        b.submit(BenchVerdict { kind: BenchKind::ToolAudit, pass: true, fp: 1 });
        chk(&mut set, "C3943-04", !b.triad_complete() && b.adjudicate() == S8CCode::FUNC_LOST, "err");
    }
    // 05 功能保全：全登记全提供
    {
        let mut p = PreserveLedger::new();
        p.register("对比度");
        p.register("焦点序");
        p.provide("对比度");
        p.provide("焦点序");
        chk(&mut set, "C3943-05", p.preserved(), "err");
    }
    // 06 功能丢失→插件补全实测
    {
        let mut p = PreserveLedger::new();
        p.register("朗读序");
        p.provide("对比度"); // 提供了未登记项→FUNC_LOST
        chk(&mut set, "C3943-06", p.provide("对比度") == S8CCode::FUNC_LOST && p.lost() == vec!["朗读序"] && p.patch("朗读序") == S8CCode::OK && p.preserved(), "err");
    }
    // 07 追踪链成立
    {
        let mut t = TraceChain::new();
        t.push(0xABCD);
        t.push(0xABCD ^ (0x9E3779B97F4A7C15u64.wrapping_mul(1)));
        t.push(0xABCD ^ (0x9E3779B97F4A7C15u64.wrapping_mul(1)) ^ (0x9E3779B97F4A7C15u64.wrapping_mul(2)));
        chk(&mut set, "C3943-07", t.find_break().is_none(), "err");
    }
    // 08 追踪断链→修正实测
    {
        let mut t = TraceChain::new();
        t.push(0x1000);
        t.push(0x1000 ^ (0x9E3779B97F4A7C15u64.wrapping_mul(1)));
        t.push(0xDEAD); // 断点
        chk(&mut set, "C3943-08", t.find_break() == Some(2) && t.repair_from(2) == S8CCode::OK && t.find_break().is_none(), "err");
    }
    // 09 修正越界拒收
    {
        let mut t = TraceChain::new();
        t.push(1);
        chk(&mut set, "C3943-09", t.repair_from(0) == S8CCode::TRACE_BREAK && t.repair_from(5) == S8CCode::TRACE_BREAK, "err");
    }
    // 10 协议分歧检出
    {
        let mut a = ProtoArbiter::new(7);
        a.declare(1, 7);
        a.declare(2, 6);
        a.declare(3, 7);
        chk(&mut set, "C3943-10", a.split() == vec![2], "err");
    }
    // 11 单源化归一
    {
        let mut a = ProtoArbiter::new(7);
        a.declare(1, 6);
        a.declare(2, 5);
        chk(&mut set, "C3943-11", a.unify() == 2 && a.split().is_empty(), "err");
    }
    // 12 三台种别守恒（种别值 0/1/2）
    {
        chk(&mut set, "C3943-12", BenchKind::ToolAudit as usize == 0 && BenchKind::OutputVerify as usize == 1 && BenchKind::RenderBreak as usize == 2, "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3943-13", S8CCode::PROTO_SPLIT.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8CCode::OK, S8CCode::FUNC_LOST, S8CCode::TRIAD_SPLIT, S8CCode::TRACE_BREAK, S8CCode::PROTO_SPLIT];
        chk(&mut set, "C3943-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_review_all_green() {
        let s = run_ves197_checks();
        assert!(s.all_passed(), "ves197 red: {}", s.red_items_text());
    }

    #[test]
    fn test_triad_split_detection() {
        let mut b = TriadBench::new();
        b.submit(BenchVerdict { kind: BenchKind::ToolAudit, pass: false, fp: 9 });
        b.submit(BenchVerdict { kind: BenchKind::OutputVerify, pass: true, fp: 8 });
        assert_eq!(b.find_split(), Some((BenchKind::ToolAudit, BenchKind::OutputVerify)));
    }

    #[test]
    fn test_trace_full_chain_roundtrip() {
        let mut t = TraceChain::new();
        t.push(42);
        for i in 1..8 {
            let prev = t.fps[i - 1];
            t.push(prev ^ (0x9E3779B97F4A7C15u64.wrapping_mul(i as u64)));
        }
        assert!(t.find_break().is_none());
    }
}
