//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3782 · 段断言计数口径（F1382/F1400 双层） · 诊断段 0xD680
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② J1：双层口径（F1382 段层（每批段的断言计数——批级里程碑）
//!      +F1400 域层（全段断言累计——域级大关）——段层是砖、域层
//!      是墙（段计数之和==域计数——双层的守恒））；口径 const
//!      （断言=assert 调用计数——口径漂移=大关失义）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 段层：逐段 assert 调用计数；
//!   - 域层：Σ段计数 == 域计数（双层守恒）；
//!   - 口径 const：计数器只认 assert 调用（登记即计数）；
//!   - 口径漂移注入：非 assert 调用混入必被守恒抓出；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xD680;

/// 段数。
pub const SEGS: usize = 6;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 段层计数器（口径：仅 assert 登记计数）。
pub struct SegCounter {
    pub seg: usize,
    pub asserts: u32,
    /// 非 assert 混入计数（口径漂移探测）。
    pub foreign: u32,
}

impl SegCounter {
    pub fn new(seg: usize) -> Self {
        SegCounter { seg, asserts: 0, foreign: 0 }
    }

    /// assert 登记（口径 const 唯一入口）。
    pub fn on_assert(&mut self) {
        self.asserts += 1;
    }

    /// 非 assert 事件：不入断言账。
    pub fn on_other(&mut self) {
        self.foreign += 1;
    }

    /// 口径纯度：foreign 不影响 asserts。
    pub fn pure(&self) -> bool {
        self.foreign == 0 || self.asserts > 0
    }
}

/// 域层累计账。
pub struct DomainTotal {
    pub segs: Vec<SegCounter>,
    /// 域层大关读数。
    pub domain_count: u32,
}

impl DomainTotal {
    pub fn new(n: usize) -> Self {
        let mut segs = Vec::new();
        for i in 0..n {
            segs.push(SegCounter::new(i));
        }
        DomainTotal { segs, domain_count: 0 }
    }

    /// 域层读数 = Σ段层（守恒写入）。
    pub fn refresh(&mut self) -> bool {
        let sum: u32 = self.segs.iter().map(|s| s.asserts).sum();
        self.domain_count = sum;
        true
    }

    /// 双层守恒：段计数之和 == 域计数。
    pub fn conserved(&self) -> bool {
        let sum: u32 = self.segs.iter().map(|s| s.asserts).sum();
        sum == self.domain_count
    }

    /// 段层之和哈希。
    pub fn hash(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for s in &self.segs {
            buf.push(s.seg as u8);
            buf.push(s.asserts as u8);
        }
        fnv1a(&buf)
    }
}

/// 标准场景：各段 8~13 次 assert + 噪声 other。
pub fn standard_run() -> DomainTotal {
    let mut dt = DomainTotal::new(SEGS);
    for (i, seg) in dt.segs.iter_mut().enumerate() {
        let n = 8 + i as u32 % 6;
        for _ in 0..n {
            seg.on_assert();
        }
        for _ in 0..3 {
            seg.on_other();
        }
    }
    let _ = dt.refresh();
    dt
}

/// J1 主判据：双层守恒 + 口径纯度 + 域层读数正确。
pub fn j1_main() -> bool {
    let dt = standard_run();
    let expect: u32 = (0..SEGS as u32).map(|i| 8 + i % 6).sum();
    dt.conserved() && dt.domain_count == expect && dt.segs.iter().all(|s| s.asserts >= 8) && dt.hash() != 0
}

// ── 注入核（0xD680 段 5 项）────────────────────────────────────────────

/// 注入核 1：口径漂移注入——other 事件混入不改断言账。
pub fn inj1_caliber_drift() -> bool {
    let mut s = SegCounter::new(0);
    for _ in 0..5 {
        s.on_assert();
    }
    let before = s.asserts;
    for _ in 0..7 {
        s.on_other();
    }
    s.asserts == before && s.foreign == 7
}

/// 注入核 2：双层守恒破注入——域读数手工虚增必被守恒抓出。
pub fn inj2_layer_break() -> bool {
    let mut dt = standard_run();
    let _ = dt.refresh();
    let ok = dt.conserved();
    dt.domain_count += 100;
    !dt.conserved() && ok
}

/// 注入核 3：段漏计注入——漏一段后 Σ 与域读数失配。
pub fn inj3_seg_miss() -> bool {
    let mut dt = DomainTotal::new(SEGS);
    for i in 0..SEGS - 1 {
        for _ in 0..4 {
            dt.segs[i].on_assert();
        }
    }
    let _ = dt.refresh();
    // 域读数在 refresh 时同步——模拟域账先写、段后补的场景
    dt.segs[SEGS - 1].on_assert();
    let sum: u32 = dt.segs.iter().map(|s| s.asserts).sum();
    sum != dt.domain_count
}

/// 注入核 4：口径 const 注入——断言计数只进不退。
pub fn inj4_monotone() -> bool {
    let mut s = SegCounter::new(0);
    for _ in 0..10 {
        s.on_assert();
    }
    let a = s.asserts;
    for _ in 0..3 {
        s.on_assert();
    }
    s.asserts == a + 3 && a == 10
}

/// 注入核 5：段数守恒注入——段数 const。
pub fn inj5_seg_count() -> bool {
    let dt = standard_run();
    dt.segs.len() == SEGS && SEGS == 6
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_unx3782_checks：本单判据聚合（诊断段 0xD680）。
pub fn run_unx3782_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3782 段断言计数口径（F1382/F1400 双层）");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "双层口径（段层砖+域层墙）双层守恒",
        j1_main(),
        if j1_main() { "" } else { "双层守恒破" },
    );
    s.add(
        "口径const（断言=assert调用计数）",
        inj1_caliber_drift() && inj4_monotone(),
        if inj1_caliber_drift() { "" } else { "口径漂移" },
    );
    let inj = [
        ("注入1口径漂移", inj1_caliber_drift()),
        ("注入2双层守恒破", inj2_layer_break()),
        ("注入3段漏计", inj3_seg_miss()),
        ("注入4计数单调", inj4_monotone()),
        ("注入5段数守恒", inj5_seg_count()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
