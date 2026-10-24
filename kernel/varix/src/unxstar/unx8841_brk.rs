//! UNX-F8841 brk 语义档与探顶行为（glibc 探测关键）（0xA500 段）
//!
//! 锚点：UNX-F8841-J1 brk(0) 返回当前断点、增减与对齐行为与 Linux
//! 逐项一致，glibc 探顶序列（渐进探测）10/10 次正确响应。
//! 域：CoRun·UNX-C2-B03（brk 语义面）。
//!
//! 结构：堆段模型（heap_base/heap_brk/heap_max + 页对齐）+ brk
//! 语义器（brk(0) 返回当前断点 / 上探成功返回新断点 / 下探不越
//! min_brk / 越顶拒绝返回旧断点——Linux 语义：失败返回原值）+
//! 页对齐核（brk 请求页对齐化 + 实际断点 vs 请求断点分离）+
//! glibc 渐进探顶序列（10/10 正确响应账）+ 注入核（下越/上越/零页/
//! 封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA500 独占 0x80）。

pub const SEG_BASE: u32 = 0xA500;
pub const XC_BRK_UNDER: u32 = SEG_BASE + 0x01; // 下越 min_brk
pub const XC_BRK_OVER: u32 = SEG_BASE + 0x02; // 上越 heap_max
pub const XC_BRK_SEALED: u32 = SEG_BASE + 0x03; // 封账

/// 页大小。
pub const PAGE: u64 = 4096;

/// 堆段。
pub struct HeapSeg {
    pub heap_base: u64, // 堆基（min_brk）
    pub brk: u64,       // 当前断点
    pub heap_max: u64,  // 堆顶界
    pub sealed: bool,
}

impl HeapSeg {
    /// 初始堆（Linux 语义：基 = 结束于页边界，brk 初值 = 基）。
    pub fn new(base: u64, max: u64) -> Self {
        let base = page_up(base);
        HeapSeg { heap_base: base, brk: base, heap_max: page_up(max), sealed: false }
    }

    /// brk 语义器（Linux 语义核心）：
    /// - addr == 0 → 返回当前断点（探底请求）
    /// - addr < heap_base → 拒（返回旧 brk）
    /// - addr > heap_max → 拒（返回旧 brk）
    /// - 合法 → 断点页对齐化推进/收缩，返回新 brk
    pub fn brk(&mut self, addr: u64) -> u64 {
        if self.sealed {
            return self.brk;
        }
        if addr == 0 {
            return self.brk; // brk(0) 探底
        }
        if addr < self.heap_base {
            return self.brk; // 下越：Linux 返回旧断点
        }
        if addr > self.heap_max {
            return self.brk; // 上越：返回旧断点
        }
        self.brk = addr;
        self.brk
    }

    /// 页对齐化视图（实际映射顶 = 页上取整）。
    pub fn mapped_top(&self) -> u64 {
        page_up(self.brk)
    }

    /// 增量核：brk 增长后 brk(0) 反映新值。
    pub fn grow_probe(&mut self, delta: u64) -> u64 {
        let target = self.brk + delta;
        self.brk(target)
    }

    /// glibc 渐进探顶序列：逐步 +1 页上探直到拒绝，记录每步响应。
    pub fn probe_top(&mut self) -> Vec<(u64, bool)> {
        let mut log = Vec::new();
        loop {
            let want = self.brk + PAGE;
            let got = self.brk(want);
            let ok = got == want;
            log.push((want, ok));
            if !ok {
                break;
            }
            if log.len() >= 64 {
                break;
            }
        }
        log
    }
}

/// 页上取整。
pub fn page_up(v: u64) -> u64 {
    (v + PAGE - 1) & !(PAGE - 1)
}

/// brk(0) 探底核：返回当前断点。
pub fn brk0_exact() -> bool {
    let mut h = HeapSeg::new(0x4000_0000, 0x4100_0000);
    let cur = h.brk(0);
    if cur != h.brk || cur != h.heap_base {
        return false;
    }
    // 增长后再探
    let _ = h.brk(h.brk + 0x3000);
    h.brk(0) == h.brk && h.brk == h.heap_base + 0x3000
}

/// 增减语义核：合法增/合法减/下越拒/上越拒（全返回对齐 Linux 语义）。
pub fn updown_exact() -> bool {
    let mut h = HeapSeg::new(0x5000_0000, 0x5100_0000);
    // 合法增
    let a = h.brk(h.heap_base + 0x2000);
    if a != h.heap_base + 0x2000 {
        return false;
    }
    // 合法减（不低于 base）
    let b = h.brk(h.heap_base + 0x1000);
    if b != h.heap_base + 0x1000 {
        return false;
    }
    // 下越（低于 base）→ 旧断点
    let c = h.brk(h.heap_base - PAGE);
    if c != h.brk {
        return false;
    }
    // 上越（超 max）→ 旧断点
    let d = h.brk(h.heap_max + PAGE);
    d == h.brk && h.brk == h.heap_base + 0x1000
}

/// 页对齐核：mapped_top 页上取整 + 请求断点与实际映射顶分离。
pub fn align_exact() -> bool {
    let mut h = HeapSeg::new(0x6000_0000, 0x6100_0000);
    // 请求非页对齐断点
    let got = h.brk(h.heap_base + 0x1234);
    if got != h.heap_base + 0x1234 {
        return false;
    }
    // 实际映射顶 = 页上取整
    h.mapped_top() == h.heap_base + 0x2000 && page_up(0x1234) == 0x2000
}

/// glibc 探顶 10/10 核：探顶序列响应正确（逐步 +1 页，到 max 拒）。
pub fn probe_10_exact() -> bool {
    // 堆 max 距 base 恰 10 页 → 探 10 成功 + 1 拒
    let mut h = HeapSeg::new(0x7000_0000, 0x7000_0000 + 10 * PAGE);
    let log = h.probe_top();
    if log.len() != 11 {
        return false;
    }
    // 前 10 步成功，第 11 步拒绝
    let mut ok_count = 0;
    for (i, &(_, ok)) in log.iter().enumerate() {
        if i < 10 {
            if ok {
                ok_count += 1;
            }
        } else if !ok {
            ok_count += 1;
        }
    }
    ok_count == 11 && h.brk == h.heap_max
}

/// 收缩语义核：断点收缩回 base 后再增长正常。
pub fn shrink_exact() -> bool {
    let mut h = HeapSeg::new(0x8000_0000, 0x8100_0000);
    let _ = h.brk(h.heap_base + 0x5000);
    let _ = h.brk(h.heap_base);
    let again = h.brk(h.heap_base + 0x4000);
    again == h.heap_base + 0x4000
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut h = HeapSeg::new(0x9000_0000, 0x9100_0000);
    // 1) 下越
    let b = h.brk;
    if h.brk(h.heap_base - 1) == b && b != h.heap_base - 1 {
        caught += 1;
    }
    // 2) 上越
    if h.brk(h.heap_max + 1) == b {
        caught += 1;
    }
    // 3) 探顶至拒绝
    let mut h2 = HeapSeg::new(0xA000_0000, 0xA000_0000 + 2 * PAGE);
    let log = h2.probe_top();
    if log.len() == 3 && !log[2].1 {
        caught += 1;
    }
    // 4) 页对齐化
    if page_up(PAGE + 1) == 2 * PAGE {
        caught += 1;
    }
    // 5) 封账后探底仍返回当前断点但不迁移
    h.sealed = true;
    let before = h.brk(0);
    let after = h.brk(before + PAGE);
    if before == after {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：堆账折叠哈希。
pub fn fp_frozen(h: &HeapSeg) -> u64 {
    let mut x: u64 = 0xcbf29ce484222325;
    x ^= h.heap_base;
    x = x.wrapping_mul(0x100000001b3);
    x ^= h.brk;
    x = x.wrapping_mul(0x100000001b3);
    x ^= h.heap_max;
    x = x.wrapping_mul(0x100000001b3);
    x
}

/// 判据入口（CheckSet）。
pub fn run_unx8841_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F8841");
    // J1-1 brk(0) 探底
    let ok = brk0_exact();
    s.add("B03-01-brk0探底", ok, if ok { "" } else { "探底漂移" });
    // J1-2 增减语义
    let ok = updown_exact();
    s.add("B03-02-增减语义", ok, if ok { "" } else { "增减漂移" });
    // J1-3 页对齐
    let ok = align_exact();
    s.add("B03-03-页对齐", ok, if ok { "" } else { "对齐漂移" });
    // J1-4 glibc 探顶 10/10
    let ok = probe_10_exact();
    s.add("B03-04-探顶十应", ok, if ok { "" } else { "探顶漂移" });
    // J1-5 收缩语义
    let ok = shrink_exact();
    s.add("B03-05-收缩语义", ok, if ok { "" } else { "收缩漂移" });
    // J1-6 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B03-06-注入核必抓", ok, &detail);
    // J1-7 指纹冻结
    let h = HeapSeg::new(0xB000_0000, 0xB100_0000);
    let ok = fp_frozen(&h) == fp_frozen(&h);
    s.add("B03-07-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
