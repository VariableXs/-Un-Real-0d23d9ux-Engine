//! UNX-F2783 · 大页整块回收（整页无引用判定 → 整块逐出）· 诊断段 0x7800
//! 验收锚点：UNX-F2783-J1 2MB 大页整块回收 10/10 次成功，碎块拒绝路径正确——
//! 大页由 512 个 4KB 子页组成；回收判定：整页 512 子页均无引用才可整块逐出；
//! 任一子页被引用即判"碎块"，拒绝回收并留诊断账（拒因：碎块）。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0x7800;

pub const SUB_PAGES: usize = 512;
pub const HUGE_ORDER: usize = 9; // 2^9 * 4KB = 2MB

/// 大页：512 子页引用计数。
pub struct HugePage {
    pub base: u64,
    pub refs: [u32; SUB_PAGES],
    pub evict_ok_total: u32,
    pub evict_reject_total: u32,
    pub last_reject: Option<&'static str>,
}

impl HugePage {
    pub fn new(base: u64) -> Self {
        HugePage { base, refs: [0; SUB_PAGES], evict_ok_total: 0, evict_reject_total: 0, last_reject: None }
    }

    pub fn get(&mut self, sub: usize) -> bool {
        if sub >= SUB_PAGES || self.refs[sub] == u32::MAX {
            return false;
        }
        self.refs[sub] += 1;
        true
    }

    pub fn put(&mut self, sub: usize) -> bool {
        if sub >= SUB_PAGES || self.refs[sub] == 0 {
            return false;
        }
        self.refs[sub] -= 1;
        true
    }

    /// 整页无引用判定。
    pub fn all_free(&self) -> bool {
        self.refs.iter().all(|r| *r == 0)
    }

    pub fn used_count(&self) -> u32 {
        self.refs.iter().filter(|r| **r > 0).count() as u32
    }

    /// 回收：整块无引用 → 逐出（清账返回 ok）；碎块 → 拒绝（留拒因）。
    pub fn try_evict(&mut self) -> bool {
        if self.all_free() {
            self.evict_ok_total += 1;
            self.last_reject = None;
            true
        } else {
            self.evict_reject_total += 1;
            self.last_reject = Some("fragment");
            false
        }
    }
}

/// 回收器：页池管理 + 10/10 回收演练。
pub struct HugeReclaimer {
    pub pool: Vec<HugePage>,
    pub reclaimed: u32,
    pub rejected: u32,
}

impl HugeReclaimer {
    pub fn new() -> Self {
        HugeReclaimer { pool: Vec::new(), reclaimed: 0, rejected: 0 }
    }

    pub fn add_page(&mut self, base: u64) {
        self.pool.push(HugePage::new(base));
    }

    /// 对第 i 页执行回收流程。
    pub fn reclaim(&mut self, i: usize) -> bool {
        if i >= self.pool.len() {
            return false;
        }
        if self.pool[i].try_evict() {
            self.reclaimed += 1;
            true
        } else {
            self.rejected += 1;
            false
        }
    }

    /// 扫描回收：回收池内所有整块无引用页。
    pub fn scan_reclaim(&mut self) -> u32 {
        let mut n = 0u32;
        for i in 0..self.pool.len() {
            if self.pool[i].all_free() && self.pool[i].try_evict() {
                self.reclaimed += 1;
                n += 1;
            } else if !self.pool[i].all_free() {
                self.pool[i].evict_reject_total += 1;
                self.pool[i].last_reject = Some("fragment");
            }
        }
        n
    }
}

/// 碎块拆分：2MB 大页降级为 512 个 4KB 小页（拒绝路径的后续处置声明）。
pub struct SplitDown {
    pub small_pages: Vec<(u64, u32)>, // (4KB 页基址, 引用)
    pub split_total: u32,
}

impl SplitDown {
    pub fn new() -> Self {
        SplitDown { small_pages: Vec::new(), split_total: 0 }
    }

    /// 把大页按现引用拆小页：引用随迁。
    pub fn split(&mut self, hp: &HugePage) -> u32 {
        let mut moved = 0u32;
        for (i, r) in hp.refs.iter().enumerate() {
            self.small_pages.push((hp.base + (i as u64) * 4096, *r));
            if *r > 0 {
                moved += 1;
            }
        }
        self.split_total += 1;
        moved
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0x7800 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x2783_7800;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("大页-空页整块可逐", HugePage::new(0x1000).try_evict()));
    out.push(("大页-越界get拒绝", !HugePage::new(0).get(512)));
    out.push(("大页-空页put拒绝", !HugePage::new(0).put(0)));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx2783_checks：本单判据聚合（诊断段 0x7800）。
pub fn run_unx2783_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F2783 大页整块回收");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 引用计数基本盘
    let mut hp = HugePage::new(0x2000_0000);
    let g1 = hp.get(0) && hp.get(511);
    s.add("大页-首尾子页get成功", g1, if g1 { "" } else { "get 路径漂移" });
    let used = hp.used_count();
    s.add("大页-2子页占用计数", used == 2, if used == 2 { "" } else { "占用计数漂移" });
    let p1 = hp.put(0);
    s.add("大页-put归还成功", p1, if p1 { "" } else { "put 路径漂移" });
    let dup_put = !hp.put(0);
    s.add("大页-重复put拒绝", dup_put, if dup_put { "" } else { "重复 put 未拒" });

    // 碎块拒绝路径：任一子页被引用即拒
    let frag = !hp.try_evict() && hp.evict_reject_total == 1 && hp.last_reject == Some("fragment");
    s.add("碎块-单子页引用拒绝整块回收", frag, if frag { "" } else { "碎块拒绝路径失效" });

    // 归还后整块可逐
    let ok_put = hp.put(511);
    let full_evict = ok_put && hp.try_evict() && hp.evict_ok_total == 1;
    s.add("整块-全归还后逐出成功", full_evict, if full_evict { "" } else { "整块回收路径失效" });

    // 10/10 整块回收演练：10 页全空 → 10 次全逐
    let mut rc = HugeReclaimer::new();
    for i in 0..10u64 {
        rc.add_page(0x3000_0000 + i * 0x2000_00);
    }
    let mut all_ok = true;
    for i in 0..10 {
        if !rc.reclaim(i) {
            all_ok = false;
        }
    }
    let ten_ok = all_ok && rc.reclaimed == 10 && rc.rejected == 0;
    s.add("10/10-整块回收全成功", ten_ok, if ten_ok { "" } else { "10/10 演练缺口" });

    // 碎块混合池扫描：8 空页 + 2 碎页 → 扫描回收恰 8
    let mut rc2 = HugeReclaimer::new();
    for i in 0..10u64 {
        rc2.add_page(0x4000_0000 + i * 0x2000_00);
        if i == 3 || i == 7 {
            let _ = rc2.pool[i as usize].get(256);
        }
    }
    let n = rc2.scan_reclaim();
    let mix_ok = n == 8 && rc2.rejected == 0 && rc2.pool[3].last_reject == Some("fragment");
    s.add("混合池-扫描回收恰8碎2留账", mix_ok, if mix_ok { "" } else { "混合扫描漂移" });

    // 碎块拆分：引用随迁
    let mut sd = SplitDown::new();
    let moved = sd.split(&rc2.pool[3]);
    let split_ok = moved == 1 && sd.small_pages.len() == SUB_PAGES && sd.split_total == 1;
    s.add("拆分-512小页落账引用随迁", split_ok, if split_ok { "" } else { "拆分账漂移" });

    // 深引用页（多点引用）拒绝 + 拆分随迁多点
    let mut hp2 = HugePage::new(0x5000_0000);
    for k in 0..8u32 {
        for _ in 0..=k {
            let _ = hp2.get(k as usize);
        }
    }
    let deep_reject = !hp2.try_evict();
    let mut sd2 = SplitDown::new();
    let moved2 = sd2.split(&hp2);
    let deep_ok = deep_reject && moved2 == 8;
    s.add("深引用-拒绝回收拆分随迁8点", deep_ok, if deep_ok { "" } else { "深引用路径漂移" });

    // FNV 页账指纹稳定
    let h1 = fnv1a(format!("huge {:X} used {}", hp2.base, hp2.used_count()).as_bytes());
    let h2 = fnv1a(format!("huge {:X} used {}", 0x5000_0000u64, 8u32).as_bytes());
    s.add("FNV-页账指纹稳定", h1 == h2, if h1 == h2 { "" } else { "指纹不稳" });

    s
}
