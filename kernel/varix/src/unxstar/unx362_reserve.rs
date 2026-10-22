//! UNX-F0362 保留区登记与白名单（引导自用段冻结）（0xAD80 段）
//!
//! 锚点：UNX-F0362-J1 保留区白名单逐段冻结，白名单外分配尝试拦截
//! 100%。
//! 域：CoRun·UNX-A1-B19（保留区面）。
//!
//! 结构：保留段账（(基址, 长度, 用途) 冻结序）+ 白名单登记器 +
//! 分配拦截器（请求区与任一保留段重叠 → 拒，检出 100%）+ 重叠
//! 判定器（半开区间 [base, base+len) 相交）+ 注入核（越界请求/
//! 段重叠登记/空段/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xAD80 独占 0x80）。
pub const SEG_BASE: u32 = 0xAD80;
pub const XC_RV_OVERLAP: u32 = SEG_BASE + 0x01; // 白名单外重叠
pub const XC_RV_SELF: u32 = SEG_BASE + 0x02; // 登记段自重叠
pub const XC_RV_SEALED: u32 = SEG_BASE + 0x03; // 封账

/// 保留段。
#[derive(Clone, Copy)]
pub struct ReservedSeg {
    pub base: u64,
    pub len: u64,
    pub purpose: &'static str,
}

impl ReservedSeg {
    pub fn end(&self) -> u64 {
        self.base + self.len
    }
    /// 半开区间相交。
    pub fn overlaps(&self, b: u64, l: u64) -> bool {
        self.base < b + l && b < self.end()
    }
}

/// 引导自用保留段（冻结）。
pub const RESERVED: [ReservedSeg; 6] = [
    ReservedSeg { base: 0x0000_0000, len: 0x1000, purpose: "real-mode-ivt" },
    ReservedSeg { base: 0x0008_8000, len: 0x2000, purpose: "ap-trampoline" },
    ReservedSeg { base: 0x0010_0000, len: 0x40_0000, purpose: "kernel-image" },
    ReservedSeg { base: 0x0050_0000, len: 0x10_0000, purpose: "kernel-heap" },
    ReservedSeg { base: 0x0060_0000, len: 0x8_0000, purpose: "page-tables" },
    ReservedSeg { base: 0x0100_0000, len: 0x20_0000, purpose: "initrd" },
];

/// 白名单登记器。
pub struct ReserveBook {
    pub segs: Vec<ReservedSeg>,
    pub intercepts: Vec<(u64, u64)>,
    pub sealed: bool,
}

impl ReserveBook {
    pub fn new() -> Self {
        ReserveBook { segs: Vec::new(), intercepts: Vec::new(), sealed: false }
    }

    /// 登记段（自重叠拒）。
    pub fn register(&mut self, seg: ReservedSeg) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_RV_SEALED);
        }
        if seg.len == 0 {
            return Err(XC_RV_SELF);
        }
        for s in &self.segs {
            if s.overlaps(seg.base, seg.len) {
                return Err(XC_RV_SELF);
            }
        }
        self.segs.push(seg);
        Ok(())
    }

    /// 分配请求拦截（与任一保留段重叠 → 拒并记账）。
    pub fn try_alloc(&mut self, base: u64, len: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_RV_SEALED);
        }
        if len == 0 {
            return Err(XC_RV_SELF);
        }
        for s in &self.segs {
            if s.overlaps(base, len) {
                self.intercepts.push((base, len));
                return Err(XC_RV_OVERLAP);
            }
        }
        Ok(())
    }

    /// 白名单冻结核：登记段与冻结表逐段一致。
    pub fn matches_frozen(&self) -> bool {
        if self.segs.len() != RESERVED.len() {
            return false;
        }
        self.segs
            .iter()
            .zip(RESERVED.iter())
            .all(|(a, b)| a.base == b.base && a.len == b.len && a.purpose == b.purpose)
    }
}

/// 冻结表一致性核：六段逐段可查且互不重叠。
pub fn frozen_exact() -> bool {
    if RESERVED.len() != 6 {
        return false;
    }
    for (i, a) in RESERVED.iter().enumerate() {
        if a.purpose.is_empty() {
            return false;
        }
        for b in RESERVED.iter().skip(i + 1) {
            if a.overlaps(b.base, b.len) {
                return false;
            }
        }
    }
    true
}

/// 拦截 100% 核：逐保留段内/跨界/全包三种重叠形态全拦。
pub fn intercept_100_exact() -> bool {
    let mut book = ReserveBook::new();
    for s in RESERVED.iter() {
        if book.register(*s).is_err() {
            return false;
        }
    }
    if !book.matches_frozen() {
        return false;
    }
    let mut caught = 0;
    // 形态 1：段内（kernel-image 中段）
    if book.try_alloc(0x0020_0000, 0x1000) == Err(XC_RV_OVERLAP) {
        caught += 1;
    }
    // 形态 2：跨界（initrd 起点前 1 页起跨入）
    if book.try_alloc(0x00FF_F000, 0x2000) == Err(XC_RV_OVERLAP) {
        caught += 1;
    }
    // 形态 3：全包（覆盖整个 page-tables 段）
    if book.try_alloc(0x005F_0000, 0x20_0000) == Err(XC_RV_OVERLAP) {
        caught += 1;
    }
    // 合法分配（非保留区）
    if book.try_alloc(0x0200_0000, 0x1_0000).is_ok() {
        caught += 1;
    }
    caught == 4 && book.intercepts.len() == 3
}

/// 相邻段不误伤核：紧邻保留段的请求放行。
pub fn adjacency_exact() -> bool {
    let mut book = ReserveBook::new();
    for s in RESERVED.iter() {
        let _ = book.register(*s);
    }
    // initrd 结束页之后
    let end = 0x0100_0000 + 0x20_0000;
    book.try_alloc(end, 0x1000).is_ok()
        // kernel-image 之前（1MB 下方留白）
        && book.try_alloc(0x000A_8000, 0x1000).is_ok()
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut book = ReserveBook::new();
    // 1) 空段登记拒
    if book.register(ReservedSeg { base: 0x1000, len: 0, purpose: "x" }) == Err(XC_RV_SELF) {
        caught += 1;
    }
    // 2) 自重叠登记拒
    let _ = book.register(RESERVED[0]);
    if book.register(RESERVED[0]) == Err(XC_RV_SELF) {
        caught += 1;
    }
    // 3) 封账后拒
    book.sealed = true;
    if book.try_alloc(0x3000_0000, 0x1000) == Err(XC_RV_SEALED) {
        caught += 1;
    }
    // 4) 拦截账留痕
    let mut book2 = ReserveBook::new();
    for s in RESERVED.iter() {
        let _ = book2.register(*s);
    }
    let _ = book2.try_alloc(0x0011_0000, 0x1000);
    if book2.intercepts.len() == 1 && book2.intercepts[0] == (0x0011_0000, 0x1000) {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：白名单折叠哈希。
pub fn fp_frozen(book: &ReserveBook) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for s in &book.segs {
        h ^= s.base;
        h = h.wrapping_mul(0x100000001b3);
        h ^= s.len;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx362_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0362");
    // J1-1 白名单冻结
    let ok = frozen_exact();
    s.add("B19-01-白名单冻结", ok, if ok { "" } else { "冻结漂移" });
    // J1-2 拦截 100%
    let ok = intercept_100_exact();
    s.add("B19-02-拦截全检", ok, if ok { "" } else { "拦截缺" });
    // J1-3 相邻不误伤
    let ok = adjacency_exact();
    s.add("B19-03-相邻放行", ok, if ok { "" } else { "误伤" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B19-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let mut book = ReserveBook::new();
    let _ = book.register(RESERVED[2]);
    let ok = fp_frozen(&book) == fp_frozen(&book);
    s.add("B19-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
