//! ves367_s20mem — VE-F4127 · T07 i18n 内存治理
//! 翻译表按需加载（LRU 上限）+ 字体子集常驻策略 + 语言包换出（切语言释放非活跃包）+ 内存上限硬顶 + 超顶降级链逐级（卸载预取→缩 LRU→告警）+ 泄漏追踪立案 + 换出误伤活跃包驻留标记 + 无障碍资源永不卸载（域本色红线）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xD5C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum McCode {
    Ok = 0,
    OverCeiling = SEG_BASE + 0x01,
    LeakSuspect = SEG_BASE + 0x02,
    EvictActive = SEG_BASE + 0x03,
}

/// 内存硬顶 KB。
pub const MEM_CEILING_KB: u32 = 96_000;
/// LRU 上限（条目数）。
pub const LRU_LIMIT: usize = 8;

/// LRU 翻译页缓存。
pub struct LruTable { pub order: Vec<&'static str>, pub cap: usize }

impl LruTable {
    pub fn new(cap: usize) -> Self { LruTable { order: Vec::new(), cap } }

    /// 访问：命中提到队头。
    pub fn touch(&mut self, page: &'static str) {
        self.order.retain(|p| *p != page);
        self.order.insert(0, page);
    }

    /// 插入：超上限从队尾换出，返回被换出页。
    pub fn insert(&mut self, page: &'static str) -> Option<&'static str> {
        self.touch(page);
        if self.order.len() > self.cap {
            self.order.pop()
        } else {
            None
        }
    }

    pub fn len(&self) -> usize { self.order.len() }
    pub fn contains(&self, page: &str) -> bool { self.order.iter().any(|p| *p == page) }

    /// 缩容：直接截到新上限，返回换出清单。
    pub fn shrink(&mut self, new_cap: usize) -> Vec<&'static str> {
        let mut out = Vec::new();
        while self.order.len() > new_cap {
            if let Some(p) = self.order.pop() { out.push(p); }
        }
        self.cap = new_cap;
        out
    }
}

/// 语言包驻留册：活跃标记（换出误伤保护）。
pub struct PackResidency { pub packs: Vec<(&'static str, u32, bool)> } // (包, KB, 活跃)

impl PackResidency {
    pub fn new() -> Self { PackResidency { packs: Vec::new() } }

    pub fn add(&mut self, name: &'static str, kb: u32, active: bool) {
        if let Some(x) = self.packs.iter_mut().find(|x| x.0 == name) {
            x.1 = kb; x.2 = active;
        } else {
            self.packs.push((name, kb, active));
        }
    }

    pub fn total_kb(&self) -> u32 { self.packs.iter().map(|p| p.1).sum() }

    /// 换出非活跃包直到总量 ≤ 目标；活跃包与无障碍包不可换。
    pub fn evict_to(&mut self, target_kb: u32, protected: &[&'static str]) -> Vec<&'static str> {
        let mut evicted = Vec::new();
        // 从尾（最旧登记）开始换非活跃。
        let mut i = self.packs.len();
        while self.total_kb() > target_kb && i > 0 {
            i -= 1;
            let (name, kb, active) = self.packs[i];
            if active || protected.contains(&name) { continue; }
            self.packs.remove(i);
            evicted.push(name);
            i = self.packs.len(); // 重新从头数（已移除）
        }
        evicted
    }

    pub fn contains(&self, name: &str) -> bool { self.packs.iter().any(|p| p.0 == name) }
}

/// 降级链：超硬顶三级（卸载预取 → 缩 LRU → 告警）。
pub fn degrade_chain(used_kb: u32, prefetch_pages: usize, lru: &mut LruTable, alarms: &mut Vec<&'static str>) {
    if used_kb <= MEM_CEILING_KB { return; }
    // 级一：卸载预取。
    if prefetch_pages > 0 { return; } // 卸载动作由调用方执行（此处标记级一足够）
    // 级二：缩 LRU。
    if lru.len() > 2 {
        let _ = lru.shrink(2);
        return;
    }
    // 级三：告警。
    alarms.push("mem-over-ceiling");
}

/// 泄漏追踪：分配计数与释放计数差持续增长 → 立案。
pub struct LeakTracker { pub allocated: u32, pub freed: u32, pub cases: Vec<&'static str> }

impl LeakTracker {
    pub fn new() -> Self { LeakTracker { allocated: 0, freed: 0, cases: Vec::new() } }
    pub fn alloc(&mut self, n: u32) { self.allocated += n; }
    pub fn free(&mut self, n: u32) { self.freed += n; }

    pub fn in_flight(&self) -> u32 { self.allocated - self.freed.min(self.allocated) }

    /// 泄漏判定：在途超阈立案。
    pub fn audit(&mut self, threshold: u32) -> bool {
        if self.in_flight() > threshold {
            self.cases.push("leak-suspect");
            return true;
        }
        false
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves367_checks() -> CheckSet {
    let mut set = CheckSet::new("ves367");

    // C367-01 LRU 上限与硬顶常量单源。
    chk(&mut set, "C367-01", LRU_LIMIT == 8 && MEM_CEILING_KB == 96_000, "LRU=8 顶=96000KB");

    // C367-02 LRU 触顶换出队尾。
    let mut lru = LruTable::new(3);
    for p in ["a", "b", "c", "d"].iter() { let _ = lru.insert(p); }
    chk(&mut set, "C367-02", !lru.contains("a") && lru.contains("d") && lru.len() == 3, "a 换出 d 在队头");

    // C367-03 touch 提队头改变换出序。
    let mut lru2 = LruTable::new(3);
    for p in ["a", "b", "c"].iter() { let _ = lru2.insert(p); }
    lru2.touch("a"); // a 提到队头
    let _ = lru2.insert("d"); // 换出应是 b
    chk(&mut set, "C367-03", !lru2.contains("b") && lru2.contains("a"), "touch 后 b 换出");

    // C367-04 语言包换出：非活跃先换。
    let mut pr = PackResidency::new();
    pr.add("ja-pack", 30_000, false);
    pr.add("zh-pack", 40_000, true);
    pr.add("ko-pack", 20_000, false);
    let evicted = pr.evict_to(50_000, &[]);
    chk(&mut set, "C367-04", pr.total_kb() == 40_000 && evicted.contains(&"ko-pack") && evicted.contains(&"ja-pack") && !pr.contains("ko-pack"), "换出至 40000 非活跃先走");

    // C367-05 活跃包不换。
    chk(&mut set, "C367-05", pr.contains("zh-pack"), "活跃 zh-pack 保留");

    // C367-06 无障碍资源保护名单（域本色）。
    let mut pr2 = PackResidency::new();
    pr2.add("a11y-speech", 5_000, false);
    pr2.add("big-pack", 90_000, false);
    let ev2 = pr2.evict_to(10_000, &["a11y-speech"]);
    chk(&mut set, "C367-06", ev2 == vec!["big-pack"] && pr2.contains("a11y-speech"), "保护名单内不换出");

    // C367-07 超顶降级链：级一卸预取即返回。
    let mut lru3 = LruTable::new(8);
    for i in 0..8 { let _ = lru3.insert("p"); }
    let mut alarms: Vec<&'static str> = Vec::new();
    degrade_chain(100_000, 4, &mut lru3, &mut alarms);
    chk(&mut set, "C367-07", alarms.is_empty() && lru3.len() == 8, "级一（有预取可卸）不下探");

    // C367-08 级二缩 LRU。
    degrade_chain(100_000, 0, &mut lru3, &mut alarms);
    chk(&mut set, "C367-08", lru3.len() == 2 && alarms.is_empty(), "无预取 → 缩 LRU 至 2");

    // C367-09 级三告警。
    degrade_chain(100_000, 0, &mut lru3, &mut alarms);
    chk(&mut set, "C367-09", alarms == vec!["mem-over-ceiling"], "LRU 已最小 → 告警");

    // C367-10 预算内不降级。
    let mut alarms2: Vec<&'static str> = Vec::new();
    let mut lru4 = LruTable::new(4);
    for i in 0..4 { let _ = lru4.insert("x"); }
    degrade_chain(50_000, 0, &mut lru4, &mut alarms2);
    chk(&mut set, "C367-10", alarms2.is_empty() && lru4.len() == 4, "预算内零动作");

    // C367-11 泄漏追踪立案。
    let mut lt = LeakTracker::new();
    lt.alloc(500); lt.free(200);
    chk(&mut set, "C367-11", lt.in_flight() == 300 && lt.audit(1_000) == false, "在途 300 未达阈");

    // C367-12 超阈立案。
    lt.alloc(900);
    chk(&mut set, "C367-12", lt.audit(1_000) && lt.cases.len() == 1, "在途 1200 > 1000 立案");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves367_smoke() {
        let set = run_ves367_checks();
        assert!(set.failed_count() == 0, "ves367 red: {}", set.red_items_text());
    }

    #[test]
    fn ves367_lru() {
        let mut l = LruTable::new(2);
        let _ = l.insert("x");
        let _ = l.insert("y");
        let ev = l.insert("z");
        assert_eq!(ev, Some("x"));
    }
}
