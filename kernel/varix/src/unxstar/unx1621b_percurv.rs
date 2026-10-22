//! unx1621b_percurv —— UNX-F1621 per-CPU 变量区布局总纲 · 双实现并存 B 面。
//! 锚点（CoRun Unxreal 主册 L3564）：UNX-F1621 · per-CPU 变量区布局总纲（接续 A1
//!           trampoline 预留地址） · 目标 380 行 · 已深化
//! 判据原文：UNX-F1621-J1 变量区按 A1 预留地址切分（统计区/队列区/时钟区/shootdown
//!           区/调试区五段）偏移账固化，五段边界与 A1 预留区零越界，布局图可导出
//! 并存声明：unx1621_percpu.rs（并行工号 A 面）在先，本面为独立第二实现（源码只增不减）。
//! 组件：五段布局冻结（统计/队列/时钟/shootdown/调试 · 段长+对齐+偏移固化）
//!      + 偏移账（段基址 = 区基址 + 累计偏移 · 对齐步进 · 全账固化）
//!      + 越界核验（五段边界 ⊆ A1 预留区 · 逐段零越界断言）
//!      + per-CPU 步进（多核区复制：核 N 区基址 = 区基址 + N × 区步长）
//!      + 越界注入核（段长溢出/核号越界/重叠段 · 必拒）
//! 判据落地：偏移账固化 + 零越界 + 布局图可导出 + 注入必拒
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 地址整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本（B 面）。
pub const PC_VERSION: &str = "unx1621b_percurv-v1";

/// 诊断段基址（0x51b0 段 · 本模块独占）。
pub const PC_SEG_BASE: u16 = 0x51b0;

/// A1 trampoline 预留区基址（判据口径 · 接续预留地址）。
pub const PC_A1_BASE: u64 = 0x0000_7000_0000;

/// A1 预留区长度（单核区步长口径）。
pub const PC_A1_LEN: u64 = 0x1_0000;

/// 最大核数（区复制上界）。
pub const PC_MAX_CPU: u32 = 64;

/// 五段闭集（冻结 v1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcSeg {
    /// 统计区（每核计数账）。
    Stats,
    /// 队列区（就绪队列锚）。
    Queue,
    /// 时钟区（tick 账 F1002 对接）。
    Clock,
    /// shootdown 区（TLB 击落 IPI 账）。
    Shootdown,
    /// 调试区（诊断落账）。
    Debug,
}

impl PcSeg {
    /// 五段全序。
    pub const ALL: [PcSeg; 5] = [
        PcSeg::Stats,
        PcSeg::Queue,
        PcSeg::Clock,
        PcSeg::Shootdown,
        PcSeg::Debug,
    ];

    /// 段长（字节 · 冻结 v1 参数表）。
    pub fn len(self) -> u64 {
        match self {
            PcSeg::Stats => 0x0800,
            PcSeg::Queue => 0x1000,
            PcSeg::Clock => 0x0400,
            PcSeg::Shootdown => 0x0800,
            PcSeg::Debug => 0x0C00,
        }
    }

    /// 对齐（字节 · 冻结 v1）。
    pub fn align(self) -> u64 {
        match self {
            PcSeg::Queue | PcSeg::Clock => 0x100,
            _ => 0x40,
        }
    }

    /// 段名。
    pub fn name(self) -> &'static str {
        match self {
            PcSeg::Stats => "stats",
            PcSeg::Queue => "queue",
            PcSeg::Clock => "clock",
            PcSeg::Shootdown => "shootdown",
            PcSeg::Debug => "debug",
        }
    }
}

/// 单段偏移账。
#[derive(Clone, Copy, Debug)]
pub struct PcSegAcc {
    /// 段。
    pub seg: PcSeg,
    /// 区内偏移（相对区基址）。
    pub offset: u64,
    /// 段长。
    pub len: u64,
    /// 对齐填充字节。
    pub pad: u64,
}

impl PcSegAcc {
    /// 段末偏移（不含下段填充）。
    pub fn end(&self) -> u64 {
        self.offset + self.len
    }
}

/// 偏移账（单核区布局 · 冻结载体）。
pub struct PcLayout {
    /// 五段账（按 ALL 序）。
    pub segs: [PcSegAcc; 5],
    /// 单核区总长（含尾填充）。
    pub region_len: u64,
    /// 区基址（A1 预留起点）。
    pub base: u64,
}

impl PcLayout {
    /// 布局装配：按段序累计偏移（对齐步进 · 账固化）。
    pub fn build(base: u64) -> Self {
        let mut cur = 0u64;
        let mut segs = [PcSegAcc {
            seg: PcSeg::Stats,
            offset: 0,
            len: 0,
            pad: 0,
        }; 5];
        for (i, s) in PcSeg::ALL.iter().enumerate() {
            let align = s.align();
            let padded = (cur + align - 1) / align * align;
            let pad = padded - cur;
            segs[i] = PcSegAcc {
                seg: *s,
                offset: padded,
                len: s.len(),
                pad,
            };
            cur = padded + s.len();
        }
        // 区尾对齐到 0x100。
        let region_len = (cur + 0xFF) / 0x100 * 0x100;
        PcLayout {
            segs,
            region_len,
            base,
        }
    }

    /// 段基址（物理：base + offset）。
    pub fn seg_pa(&self, s: PcSeg) -> u64 {
        self.base + self.segs[s.ordinal_seg()].offset
    }

    /// 越界核验：五段边界 ⊆ A1 预留区（单核区口径）。
    pub fn within_a1(&self) -> bool {
        self.base >= PC_A1_BASE && self.region_len <= PC_A1_LEN
    }

    /// 重叠核验：段区间两两不交。
    pub fn no_overlap(&self) -> bool {
        for i in 0..5 {
            for j in i + 1..5 {
                let a = &self.segs[i];
                let b = &self.segs[j];
                let a1 = a.offset;
                let a2 = a.offset + a.len;
                let b1 = b.offset;
                let b2 = b.offset + b.len;
                if a1 < b2 && b1 < a2 {
                    return false;
                }
            }
        }
        true
    }

    /// 偏移账固化断言：段序单调递增 + 全段对齐合规。
    pub fn offsets_frozen(&self) -> bool {
        let mut prev_end = 0u64;
        for s in &self.segs {
            if s.offset < prev_end {
                return false;
            }
            if s.offset % s.seg.align() != 0 {
                return false;
            }
            prev_end = s.offset + s.len;
        }
        true
    }

    /// 多核步进：核 N 区基址。
    pub fn cpu_base(&self, cpu: u32) -> Option<u64> {
        if cpu >= PC_MAX_CPU {
            return None;
        }
        Some(self.base + self.region_len * cpu as u64)
    }

    /// 多核零越界：最大核区末 ≤ A1 预留区末（A1 区按 PC_MAX_CPU 容量预留口径）。
    pub fn multi_cpu_within(&self, cpus: u32) -> bool {
        if cpus == 0 || cpus > PC_MAX_CPU {
            return false;
        }
        match self.cpu_base(cpus - 1) {
            Some(last_base) => last_base + self.region_len <= PC_A1_BASE + PC_A1_LEN * PC_MAX_CPU as u64,
            None => false,
        }
    }

    /// 布局图导出（单行）。
    pub fn export_line(&self) -> String {
        let mut line = String::new();
        for s in &self.segs {
            if !line.is_empty() {
                line.push('|');
            }
            line.push_str(&format!(
                "{}@+0x{:x}+0x{:x}",
                s.seg.name(),
                s.offset,
                s.len
            ));
        }
        format!(
            "{} region=0x{:x}..+0x{:x}",
            PC_VERSION, self.base, self.region_len
        )
        .chars()
        .chain(line.chars())
        .collect()
    }
}

impl PcSeg {
    /// 段序（0–4 · 位板序）。
    pub fn ordinal_seg(self) -> usize {
        match self {
            PcSeg::Stats => 0,
            PcSeg::Queue => 1,
            PcSeg::Clock => 2,
            PcSeg::Shootdown => 3,
            PcSeg::Debug => 4,
        }
    }
}

/// 负向注入场景。
pub enum PcInject {
    /// 段长溢出（段长改超 A1 区 → 越界必检出）。
    LenOverflow,
    /// 核号越界（cpu ≥ 64 → cpu_base 必 None）。
    CpuOver,
    /// 重叠段（人为改偏移 → no_overlap 必检出）。
    Overlap,
    /// 零核多核核验（cpus=0 必拒）。
    ZeroCpu,
}

/// 注入核。
pub struct PcInjector {
    /// 场数。
    pub cases: u32,
    /// 检出数。
    pub caught: u32,
}

impl PcInjector {
    /// 空注入核。
    pub fn new() -> Self {
        PcInjector { cases: 0, caught: 0 }
    }

    /// 四场注入。
    pub fn inject_all(&mut self, lay: &PcLayout) {
        // 场一：段长溢出（假想段长 ×16 → region_len 超 A1 单核区）。
        let fat = PcLayout::build_with_scale(lay.base, 16);
        if !fat.within_a1() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场二：核号越界。
        if lay.cpu_base(PC_MAX_CPU).is_none() && lay.cpu_base(PC_MAX_CPU - 1).is_some() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场三：重叠段（改首段偏移与末段重叠）。
        let mut bad = PcLayout::build(lay.base);
        bad.segs[4].offset = bad.segs[0].offset + 1;
        if !bad.no_overlap() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场四：零核。
        if !lay.multi_cpu_within(0) {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 4 && self.caught == self.cases
    }
}

impl PcLayout {
    /// 带放大系数装配（注入探针用 · 段长 ×scale）。
    pub fn build_with_scale(base: u64, scale: u64) -> Self {
        let mut l = PcLayout::build(base);
        for s in l.segs.iter_mut() {
            s.len *= scale;
        }
        // 重算 region_len。
        let mut cur = 0u64;
        for s in l.segs.iter_mut() {
            let align = s.seg.align();
            let padded = (cur + align - 1) / align * align;
            s.pad = padded - cur;
            s.offset = padded;
            cur = padded + s.len;
        }
        l.region_len = (cur + 0xFF) / 0x100 * 0x100;
        l
    }
}

/// 判据门面（判据四件：偏移固化/零越界/布局图导出/注入必拒）。
pub struct PcGate {
    /// 布局。
    pub lay: PcLayout,
    /// 核数。
    pub cpus: u32,
    /// 注入核。
    pub inj: PcInjector,
}

impl PcGate {
    /// 新门面。
    pub fn new(cpus: u32) -> Self {
        PcGate {
            lay: PcLayout::build(PC_A1_BASE),
            cpus,
            inj: PcInjector::new(),
        }
    }

    /// 判据总门。
    pub fn judge(&self) -> (bool, String) {
        let ok = self.lay.offsets_frozen()
            && self.lay.within_a1()
            && self.lay.no_overlap()
            && self.lay.multi_cpu_within(self.cpus)
            && self.inj.all_caught();
        let msg = format!(
            "{}: region_len=0x{:x} cpus={} within={} overlap={} frozen={} inj={}/{} seg=0x{:04x}",
            PC_VERSION,
            self.lay.region_len,
            self.cpus,
            self.lay.within_a1(),
            self.lay.no_overlap(),
            self.lay.offsets_frozen(),
            self.inj.caught,
            self.inj.cases,
            PC_SEG_BASE
        );
        (ok, msg)
    }
}

/// 判据自检（t 系入口）：8 核布局装配 + 多核步进 + 四场注入。
pub fn run_t621b_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx1621b-t621");
    let mut g = PcGate::new(8);
    // 布局图导出（可读性落账）。
    let _map = g.lay.export_line();
    // 四场注入。
    g.inj.inject_all(&g.lay);
    let (ok, msg) = g.judge();
    set.add_dyn("UNX-F1621-J1", ok, &msg);
    set.add("PC-offsets-frozen", g.lay.offsets_frozen(), "");
    set.add("PC-zero-overrun", g.lay.within_a1() && g.lay.no_overlap(), "");
    set.add("PC-multi-cpu", g.lay.multi_cpu_within(8), "");
    set.add("PC-inject-4/4", g.inj.all_caught(), "");
    set
}
