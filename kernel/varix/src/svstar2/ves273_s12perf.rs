//! ves273_s12perf — VE-F4033 · RTL 性能
//! BiDi 计算/镜像布局/切换 三段预算分项表复述制度 + BiDi 结果缓存（复用热表单源）+ 镜像布局缓存 + RTL 基准（复用制度）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x6EC0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum RtlPerfCode {
    Ok = 0,
    BudgetOver = SEG_BASE + 0x01,
    CacheBroken = SEG_BASE + 0x02,
    BaselineMiss = SEG_BASE + 0x03,
    LinearBreach = SEG_BASE + 0x04,
    NotRegistered = SEG_BASE + 0x05,
    StaleRetold = SEG_BASE + 0x06,
}

/// 三段预算项。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Segment { Bidi = 0, Mirror = 1, Switch = 2 }

impl Segment {
    pub const ALL: [Segment; 3] = [Segment::Bidi, Segment::Mirror, Segment::Switch];
    pub fn name(self) -> &'static str {
        match self { Segment::Bidi => "bidi", Segment::Mirror => "mirror", Segment::Switch => "switch" }
    }
}

/// 分项预算表：三段各自预算上限（μs）。
#[derive(Clone)]
pub struct BudgetTable {
    pub limits: [(Segment, u32); 3],
}

impl BudgetTable {
    pub fn canonical() -> Self {
        BudgetTable { limits: [(Segment::Bidi, 800), (Segment::Mirror, 400), (Segment::Switch, 1200)] }
    }
    pub fn limit_of(&self, s: Segment) -> u32 {
        for l in self.limits.iter() {
            if l.0 == s { return l.1; }
        }
        0
    }
    /// 分项复述：实测值与预算构成复述对。
    pub fn retell(&self, s: Segment, measured: u32) -> (u32, u32, bool) {
        let lim = self.limit_of(s);
        (measured, lim, measured <= lim)
    }
    pub fn over(&self, s: Segment, measured: u32) -> bool {
        measured > self.limit_of(s)
    }
}

/// 热表：BiDi 结果缓存。命中按 (文本指纹, 方向) 查。
#[derive(Clone, PartialEq, Eq)]
pub struct BidiKey {
    pub text_fp: u64,
    pub rtl: bool,
}

#[derive(Clone)]
pub struct BidiEntry {
    pub key: BidiKey,
    pub levels: Vec<u8>,
    pub hits: u32,
}

pub struct HotTable {
    pub entries: Vec<BidiEntry>,
    pub capacity: usize,
    pub misses: u32,
}

impl HotTable {
    pub fn new(capacity: usize) -> Self {
        HotTable { entries: Vec::new(), capacity, misses: 0 }
    }
    pub fn lookup(&mut self, key: &BidiKey) -> Option<Vec<u8>> {
        for e in self.entries.iter_mut() {
            if e.key == *key {
                e.hits += 1;
                return Some(e.levels.clone());
            }
        }
        self.misses += 1;
        None
    }
    pub fn insert(&mut self, key: BidiKey, levels: Vec<u8>) -> Result<(), RtlPerfCode> {
        for e in self.entries.iter() {
            if e.key == key { return Err(RtlPerfCode::CacheBroken); }
        }
        if self.entries.len() >= self.capacity {
            // LRU 简化：逐出命中数最低者。
            let mut worst = 0;
            for i in 1..self.entries.len() {
                if self.entries[i].hits < self.entries[worst].hits { worst = i; }
            }
            self.entries.remove(worst);
        }
        self.entries.push(BidiEntry { key, levels, hits: 0 });
        Ok(())
    }
    /// 热表单源断言：同 key 不允许双份缓存。
    pub fn single_source(&self) -> bool {
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                if self.entries[i].key == self.entries[j].key { return false; }
            }
        }
        true
    }
}

/// 镜像布局缓存：缓存镜像判定结果（镜像/不镜像）。
pub struct MirrorCache {
    pub map: Vec<(u32, bool)>,
}

impl MirrorCache {
    pub fn new() -> Self { MirrorCache { map: Vec::new() } }
    pub fn put(&mut self, widget: u32, mirrored: bool) -> Result<(), RtlPerfCode> {
        for m in self.map.iter() {
            if m.0 == widget {
                if m.1 != mirrored { return Err(RtlPerfCode::CacheBroken); }
                return Ok(());
            }
        }
        self.map.push((widget, mirrored));
        Ok(())
    }
    pub fn get(&self, widget: u32) -> Option<bool> {
        for m in self.map.iter() {
            if m.0 == widget { return Some(m.1); }
        }
        None
    }
}

/// RTL 基准：三段基准项注册 + 实测达标。
#[derive(Clone)]
pub struct Baseline {
    pub seg: Segment,
    pub measured_us: u32,
}

pub struct BaselineBook {
    pub items: Vec<Baseline>,
}

impl BaselineBook {
    pub fn new() -> Self { BaselineBook { items: Vec::new() } }
    pub fn register(&mut self, seg: Segment, measured_us: u32) -> Result<(), RtlPerfCode> {
        for it in self.items.iter() {
            if it.seg == seg { return Err(RtlPerfCode::NotRegistered); }
        }
        self.items.push(Baseline { seg, measured_us });
        Ok(())
    }
    /// 基准达标：对预算表逐段断言。
    pub fn judge(&self, budget: &BudgetTable) -> bool {
        for it in self.items.iter() {
            if budget.over(it.seg, it.measured_us) { return false; }
        }
        true
    }
}

/// 线性断言：BiDi 计算量随文本长度近线性（实测斜率不超阈值倍数）。
pub fn linear_assert(samples: &[(u32, u32)], max_ratio_ppm: u32) -> Result<(), RtlPerfCode> {
    if samples.len() < 2 { return Err(RtlPerfCode::LinearBreach); }
    let (l0, t0) = samples[0];
    let (l1, t1) = samples[samples.len() - 1];
    if l1 <= l0 { return Err(RtlPerfCode::LinearBreach); }
    let slope = (t1 as u64 * 1_000_000) / (l1 as u64);
    let base = (t0 as u64 * 1_000_000) / (l0 as u64);
    if base == 0 { return Err(RtlPerfCode::LinearBreach); }
    let ratio_ppm = (slope * 1000 / base) as u32;
    if ratio_ppm > max_ratio_ppm { return Err(RtlPerfCode::LinearBreach); }
    Ok(())
}

pub fn run_ves273_checks() -> CheckSet {
    let mut set = CheckSet::new("ves273_s12perf");
    let budget = BudgetTable::canonical();

    // 1) 分项预算表三段齐。
    set.add("C4033-01",
        budget.limit_of(Segment::Bidi) == 800 && budget.limit_of(Segment::Mirror) == 400
            && budget.limit_of(Segment::Switch) == 1200, "三段预算");

    // 2) 分项复述对构成。
    let (m, lim, ok) = budget.retell(Segment::Bidi, 640);
    set.add("C4033-02", m == 640 && lim == 800 && ok, "分项复述对");

    // 3) 超预算检出。
    set.add("C4033-03", budget.over(Segment::Mirror, 500) && !budget.over(Segment::Mirror, 399), "超预算检出");

    // 4) 热表命中。
    let mut ht = HotTable::new(4);
    let k1 = BidiKey { text_fp: 0xABCD, rtl: true };
    let _ = ht.insert(k1.clone(), vec![0, 1, 1]);
    set.add("C4033-04", ht.lookup(&k1) == Some(vec![0, 1, 1]), "热表命中");

    // 5) 热表未命中计数。
    let k2 = BidiKey { text_fp: 0x1234, rtl: false };
    set.add("C4033-05", ht.lookup(&k2) == None && ht.misses == 1, "未命中计数");

    // 6) 热表单源：同 key 双份拒。
    set.add("C4033-06", ht.insert(k1.clone(), vec![0]) == Err(RtlPerfCode::CacheBroken) && ht.single_source(), "双份缓存拒");

    // 7) 容量逐出：满后插入仍保持单源。
    let mut ht2 = HotTable::new(2);
    for i in 0..3u64 {
        let _ = ht2.insert(BidiKey { text_fp: 0xF000 + i, rtl: true }, vec![i as u8]);
    }
    set.add("C4033-07", ht2.entries.len() == 2 && ht2.single_source(), "容量逐出");

    // 8) 镜像缓存写读一致。
    let mut mc = MirrorCache::new();
    let _ = mc.put(7, true);
    set.add("C4033-08", mc.get(7) == Some(true) && mc.get(8) == None, "镜像缓存读写");

    // 9) 镜像缓存矛盾写拒。
    set.add("C4033-09", mc.put(7, false) == Err(RtlPerfCode::CacheBroken), "矛盾写拒");

    // 10) 基准注册：重复段拒。
    let mut bb = BaselineBook::new();
    let _ = bb.register(Segment::Bidi, 700);
    set.add("C4033-10", bb.register(Segment::Bidi, 100) == Err(RtlPerfCode::NotRegistered), "重复注册拒");

    // 11) 基准达标判定。
    let mut bb2 = BaselineBook::new();
    let _ = bb2.register(Segment::Bidi, 700);
    let _ = bb2.register(Segment::Mirror, 300);
    let _ = bb2.register(Segment::Switch, 1100);
    set.add("C4033-11", bb2.judge(&budget), "基准达标");

    // 12) 基准不达检出。
    let mut bb3 = BaselineBook::new();
    let _ = bb3.register(Segment::Switch, 1300);
    set.add("C4033-12", !bb3.judge(&budget), "基准不达检出");

    // 13) 线性断言：近线性通过。
    let samples = [(100u32, 90u32), (1000, 850), (4000, 3300)];
    set.add("C4033-13", linear_assert(&samples, 1500).is_ok(), "近线性通过");

    // 14) 线性破断言：超线性检出。
    let bad_samples = [(100u32, 50u32), (1000, 4000), (4000, 80000)];
    set.add("C4033-14", linear_assert(&bad_samples, 1500) == Err(RtlPerfCode::LinearBreach), "超线性检出");

    // 15) 线性断言样本不足拒。
    set.add("C4033-15", linear_assert(&[(1u32, 2u32)], 100) == Err(RtlPerfCode::LinearBreach), "样本不足拒");

    // 16) 三段 Segment 命名双射。
    let mut names: Vec<&str> = Vec::new();
    for s in Segment::ALL.iter() { names.push(s.name()); }
    let mut dup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { dup = true; }
        }
    }
    set.add("C4033-16", !dup && names.len() == 3, "三段名双射");

    // 17) 空基准书判定为达标（无项不否决）。
    set.add("C4033-17", BaselineBook::new().judge(&budget), "空基准书");

    // 18) 段位自查。
    set.add("C4033-18", SEG_BASE == 0x6EC0, "段位 0x6EC0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hot_table_lru() {
        let mut ht = HotTable::new(2);
        let _ = ht.insert(BidiKey { text_fp: 1, rtl: true }, vec![0]);
        let _ = ht.insert(BidiKey { text_fp: 2, rtl: true }, vec![1]);
        let _ = ht.insert(BidiKey { text_fp: 3, rtl: true }, vec![2]);
        assert_eq!(ht.entries.len(), 2);
        assert!(ht.single_source());
    }
}
