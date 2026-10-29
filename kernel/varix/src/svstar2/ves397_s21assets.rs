//! ves397_s21assets — VE-F4157 · i18n 资产汇总
//! T 域六维资产汇总（测试/语料/示例/文档/工具/性能——T04-T08 归集）；每资产挂组/挂条/挂指针；
//! 与家族资产总表对拍；台账缺回补；对拍不符修正留痕；失联断链修复；无障碍 i18n 专项列（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0040;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsCode {
    Ok = 0,
    LedgerShort = SEG_BASE + 0x01,   // 0x0041
    Mismatch = SEG_BASE + 0x02,      // 0x0042
    LinkBroken = SEG_BASE + 0x03,    // 0x0043
    ProbeReject = SEG_BASE + 0x04,   // 0x0044 边界探针拒绝（二轮深化）
    QuantileBad = SEG_BASE + 0x05,   // 0x0045 分位参数非法（二轮深化）
    InjectReject = SEG_BASE + 0x06,  // 0x0046 注入回归拒绝（二轮深化）
}

/// 六维资产。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetDim { Test = 0, Corpus = 1, Example = 2, Doc = 3, Tool = 4, Perf = 5 }

impl AssetDim {
    pub const ALL: [AssetDim; 6] = [
        AssetDim::Test, AssetDim::Corpus, AssetDim::Example,
        AssetDim::Doc, AssetDim::Tool, AssetDim::Perf];
    pub fn name(self) -> &'static str {
        match self {
            AssetDim::Test => "test", AssetDim::Corpus => "corpus", AssetDim::Example => "example",
            AssetDim::Doc => "doc", AssetDim::Tool => "tool", AssetDim::Perf => "perf",
        }
    }
}

/// 资产条目：三指针（挂组/挂条/挂指针）+ 专项列标记。
#[derive(Clone, Copy)]
pub struct AssetEntry {
    pub dim: AssetDim,
    pub owner: &'static str,  // 挂组（T04-T08）
    pub card: &'static str,   // 挂条（卡号）
    pub pointer: u32,         // 挂指针（资产锚，0 = 失联）
    pub a11y: bool,           // 无障碍 i18n 专项列（域本色）
}

impl AssetEntry {
    pub fn linked(&self) -> bool { self.pointer != 0 && !self.card.is_empty() }
}

/// 资产台账。
pub struct AssetBook { pub entries: Vec<AssetEntry> }

impl AssetBook {
    pub fn new() -> Self { AssetBook { entries: Vec::new() } }

    /// 覆盖登记（幂等：同维同卡覆盖）。
    pub fn put(&mut self, e: AssetEntry) {
        match self.entries.iter_mut().find(|x| x.dim == e.dim && x.card == e.card) {
            Some(x) => *x = e,
            None => self.entries.push(e),
        }
    }

    /// 六维齐备核验。
    pub fn dims_complete(&self) -> bool {
        AssetDim::ALL.iter().all(|d| self.entries.iter().any(|e| e.dim == *d))
    }

    /// 台账缺回补定位。
    pub fn missing_dims(&self) -> Vec<&'static str> {
        AssetDim::ALL.iter()
            .filter(|d| !self.entries.iter().any(|e| e.dim == **d))
            .map(|d| d.name()).collect()
    }

    /// 失联断链检测。
    pub fn broken_links(&self) -> Vec<&'static str> {
        self.entries.iter().filter(|e| !e.linked()).map(|e| e.dim.name()).collect()
    }

    /// 断链修复。
    pub fn fix(&mut self, dim: AssetDim, card: &'static str, pointer: u32) -> bool {
        match self.entries.iter_mut().find(|e| e.dim == dim) {
            Some(e) => { e.card = card; e.pointer = pointer; true }
            None => false,
        }
    }

    /// 与家族资产总表对拍：六维×组名一致。
    pub fn agrees_with_family(&self, family: &[(&'static str, &'static str)]) -> bool {
        family.len() == self.entries.len()
            && family.iter().all(|(owner, dim)| {
                self.entries.iter().any(|e| e.owner == *owner && e.dim.name() == *dim)
            })
    }

    /// 域本色：专项列资产在账。
    pub fn a11y_present(&self) -> bool {
        self.entries.iter().any(|e| e.a11y && e.linked())
    }
}

// ============ 二轮深化块：整数分位投影 / 边界探针 / LCG 注入回归 ============

/// [深化] 整数平方根（牛顿迭代，纯整数，无浮点）。
pub fn isqrt(n: u64) -> u64 {
    if n == 0 { return 0; }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// [深化] 整数分位（排序取点，禁浮点）：q ∈ [0,100]，四舍五入定位。
pub fn quantile_rank(mut vals: Vec<u32>, q: u32) -> Result<u32, AsCode> {
    if vals.is_empty() { return Err(AsCode::QuantileBad); }
    if q > 100 { return Err(AsCode::QuantileBad); }
    // 插入排序（n 小，内核面够用）
    for i in 1..vals.len() {
        let mut j = i;
        while j > 0 && vals[j - 1] > vals[j] {
            vals.swap(j - 1, j);
            j -= 1;
        }
    }
    let n = vals.len();
    let idx = (q as usize * (n - 1) + 50) / 100;
    Ok(vals[idx])
}

/// [深化] 指针值投影统计：min/max/值域开方调制。
#[derive(Clone, Copy)]
pub struct PointerStats {
    pub count: u32,
    pub min: u32,
    pub max: u32,
    pub spread_sqrt: u64,
}

impl PointerStats {
    pub fn of_pointers(ps: &[u32]) -> Self {
        if ps.is_empty() {
            return PointerStats { count: 0, min: 0, max: 0, spread_sqrt: 0 };
        }
        let mut min = u32::MAX;
        let mut max = 0u32;
        for p in ps.iter() {
            if *p < min { min = *p; }
            if *p > max { max = *p; }
        }
        let spread = (max - min) as u64;
        PointerStats { count: ps.len() as u32, min, max, spread_sqrt: isqrt(spread) }
    }
    /// 守恒：min ≤ max（空集退化也算守恒）。
    pub fn conserved(&self) -> bool { self.min <= self.max }
}

/// [深化] 按维取指针值投影。
pub fn pointer_values(bk: &AssetBook, dim: AssetDim) -> Vec<u32> {
    bk.entries.iter().filter(|e| e.dim == dim).map(|e| e.pointer).collect()
}

/// [深化] 台账边界探针：空册 / 满容量 / 未知维修复 / 越界序号。
pub struct AssetProbe { pub max_entries: usize }

impl AssetProbe {
    /// 空册 → ProbeReject。
    pub fn probe_empty(&self, bk: &AssetBook) -> Result<(), AsCode> {
        if bk.entries.is_empty() { Err(AsCode::ProbeReject) } else { Ok(()) }
    }
    /// 满容量 → ProbeReject。
    pub fn probe_capacity(&self, bk: &AssetBook) -> Result<(), AsCode> {
        if bk.entries.len() > self.max_entries { Err(AsCode::ProbeReject) } else { Ok(()) }
    }
    /// 未知维修复 → ProbeReject（修不中即显性回执）。
    pub fn probe_fix_unknown(&self, bk: &mut AssetBook, dim: AssetDim) -> Result<(), AsCode> {
        if bk.fix(dim, "VE-PROBE", 1) { Ok(()) } else { Err(AsCode::ProbeReject) }
    }
    /// 越界序号 → ProbeReject（不 panic）。
    pub fn probe_index(&self, bk: &AssetBook, idx: usize) -> Result<AssetDim, AsCode> {
        match bk.entries.get(idx) {
            Some(e) => Ok(e.dim),
            None => Err(AsCode::ProbeReject),
        }
    }
}

/// [深化] 8 行确定性 LCG（无浮点无墙钟）。
pub struct Lcg { state: u64 }

impl Lcg {
    pub fn new(seed: u64) -> Self { Lcg { state: seed } }
    pub fn next(&mut self) -> u64 {
        self.state = self.state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn below(&mut self, n: u32) -> u32 {
        ((self.next() >> 33) as u32) % if n == 0 { 1 } else { n }
    }
}

/// [深化] 注入回归报告：新入 / 幂等归并双计数。
pub struct InjectReport { pub pushed: u32, pub merged: u32 }

impl InjectReport {
    pub fn conserved(&self, rounds: u32) -> bool { self.pushed + self.merged == rounds }
}

/// [深化] 注入回归：LCG 生成伪资产条目，同 (维,卡) 重复注入必须幂等不增行。
pub fn inject_round(bk: &mut AssetBook, seed: u64, rounds: u32) -> InjectReport {
    let slots: [&'static str; 4] = ["S0", "S1", "S2", "S3"];
    let mut rng = Lcg::new(seed);
    let mut rep = InjectReport { pushed: 0, merged: 0 };
    let mut i = 0;
    while i < rounds {
        let dim = AssetDim::ALL[rng.below(6) as usize];
        let card = slots[rng.below(4) as usize];
        let before = bk.entries.len();
        bk.put(AssetEntry { dim, owner: "INJ", card, pointer: (i % 9 + 1) as u32, a11y: false });
        if bk.entries.len() > before { rep.pushed += 1; } else { rep.merged += 1; }
        i += 1;
    }
    rep
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves397_checks() -> CheckSet {
    let mut set = CheckSet::new("ves397");
    let mut bk = AssetBook::new();

    // C397-01 六维逐条登记（三指针齐）。
    let dims = AssetDim::ALL;
    for (i, d) in dims.iter().enumerate() {
        bk.put(AssetEntry { dim: *d, owner: "T04", card: "VE-F407x", pointer: 0xA000 + i as u32, a11y: false });
    }
    chk(&mut set, "C397-01", bk.dims_complete() && bk.entries.len() == 6, "六维三指针齐");

    // C397-02 台账缺回补定位。
    let mut bk2 = AssetBook::new();
    bk2.put(AssetEntry { dim: AssetDim::Test, owner: "T04", card: "VE-F4075", pointer: 1, a11y: false });
    bk2.put(AssetEntry { dim: AssetDim::Perf, owner: "T07", card: "VE-F4137", pointer: 2, a11y: false });
    chk(&mut set, "C397-02", bk2.missing_dims() == vec!["corpus", "example", "doc", "tool"], "缺四维定位");

    // C397-03 补齐归整。
    for d in bk2.missing_dims() { let _ = d; }
    bk2.put(AssetEntry { dim: AssetDim::Corpus, owner: "T04", card: "VE-F4145", pointer: 3, a11y: false });
    bk2.put(AssetEntry { dim: AssetDim::Example, owner: "T04", card: "VE-F4150", pointer: 4, a11y: false });
    bk2.put(AssetEntry { dim: AssetDim::Doc, owner: "T08", card: "VE-F4151", pointer: 5, a11y: false });
    bk2.put(AssetEntry { dim: AssetDim::Tool, owner: "T08", card: "VE-F4141", pointer: 6, a11y: false });
    chk(&mut set, "C397-03", bk2.dims_complete(), "六维补齐");

    // C397-04 断链检测（指针 0 或卡号空）。
    bk2.put(AssetEntry { dim: AssetDim::Tool, owner: "T08", card: "", pointer: 6, a11y: false });
    bk2.put(AssetEntry { dim: AssetDim::Perf, owner: "T07", card: "VE-F4137", pointer: 0, a11y: false });
    chk(&mut set, "C397-04", bk2.broken_links().len() == 2, "两断链检出");

    // C397-05 断链修复。
    chk(&mut set, "C397-05", bk2.fix(AssetDim::Tool, "VE-F4141", 6)
        && bk2.fix(AssetDim::Perf, "VE-F4137", 7) && bk2.broken_links().is_empty(), "断链全修");

    // C397-06 家族总表对拍一致。
    let family = [
        ("T04", "test"), ("T04", "corpus"), ("T04", "example"),
        ("T08", "doc"), ("T08", "tool"), ("T07", "perf")];
    chk(&mut set, "C397-06", bk2.agrees_with_family(&family), "对拍一致");

    // C397-07 对拍失配检出（家族少一维）。
    let fam5 = [&family[0], &family[1], &family[2], &family[3], &family[4]];
    let f5: Vec<(&'static str, &'static str)> = fam5.iter().map(|x| **x).collect();
    chk(&mut set, "C397-07", !bk2.agrees_with_family(&f5), "家族缺维失配");

    // C397-08 修正留痕。
    let mut notes: Vec<&'static str> = Vec::new();
    if !bk2.agrees_with_family(&f5) { notes.push("补 perf 维对拍"); }
    chk(&mut set, "C397-08", notes.len() == 1, "留痕一条");

    // C397-09 域本色：无障碍专项列资产在账。
    bk2.put(AssetEntry { dim: AssetDim::Test, owner: "T08", card: "VE-F4142-a11y-sr", pointer: 9, a11y: true });
    chk(&mut set, "C397-09", bk2.a11y_present(), "专项列在账");

    // C397-10 覆盖登记幂等。
    let before = bk2.entries.len();
    bk2.put(AssetEntry { dim: AssetDim::Test, owner: "T08", card: "VE-F4142-a11y-sr", pointer: 10, a11y: true });
    chk(&mut set, "C397-10", bk2.entries.len() == before, "覆盖不增行");

    // C397-11 六维名单单源。
    chk(&mut set, "C397-11", AssetDim::ALL.len() == 6 && AssetDim::ALL[0].name() == "test"
        && AssetDim::ALL[5].name() == "perf", "六维单源");

    // C397-12 失联面（空册六维全缺）。
    let empty = AssetBook::new();
    chk(&mut set, "C397-12", empty.missing_dims().len() == 6 && !empty.dims_complete(), "空册全缺");

    // ===== 二轮深化判据（C397-13 起）=====
    // C397-13 整数分位投影：test 维指针中位/四分位（match 回执，免 unwrap）。
    let pv = pointer_values(&bk, AssetDim::Test);
    let mid_ok = match (quantile_rank(pv.clone(), 50), quantile_rank(pv, 100)) {
        (Ok(a), Ok(b)) => a <= b,
        _ => false,
    };
    chk(&mut set, "C397-13", mid_ok && !pointer_values(&bk, AssetDim::Test).is_empty(), "分位投影有序");

    // C397-14 双维分位取点非零。
    let pv2 = pointer_values(&bk, AssetDim::Test);
    let pv3 = pointer_values(&bk, AssetDim::Perf);
    match (quantile_rank(pv2, 50), quantile_rank(pv3, 0)) {
        (Ok(a), Ok(b)) => chk(&mut set, "C397-14", a > 0 && b > 0, "双维分位取点非零"),
        _ => chk(&mut set, "C397-14", false, "分位不应失败"),
    }

    // C397-15 isqrt 恒等 + 分位非法参数显性拒。
    chk(&mut set, "C397-15", isqrt(0) == 0 && isqrt(1) == 1 && isqrt(15) == 3 && isqrt(16) == 4
        && isqrt(1_000_000) == 1000
        && quantile_rank(vec![1], 101) == Err(AsCode::QuantileBad)
        && quantile_rank(vec![], 50) == Err(AsCode::QuantileBad), "isqrt 恒等分位拒");

    // C397-16 边界探针：空册拒 + 越界序号拒 + 容量内放行。
    let probe = AssetProbe { max_entries: 64 };
    let mut bk3 = AssetBook::new();
    chk(&mut set, "C397-16", probe.probe_empty(&bk3) == Err(AsCode::ProbeReject)
        && probe.probe_index(&bk3, 0) == Err(AsCode::ProbeReject)
        && probe.probe_capacity(&bk3).is_ok(), "空册越界探针拒");

    // C397-17 未知维修复探针显性拒。
    chk(&mut set, "C397-17", probe.probe_fix_unknown(&mut bk3, AssetDim::Tool) == Err(AsCode::ProbeReject),
        "未知维修复拒");

    // C397-18 LCG 注入回归：12 轮 + 幂等归并守恒 + 槽位上界。
    let rep = inject_round(&mut bk3, 0x0A57_3970, 12);
    chk(&mut set, "C397-18", rep.conserved(12) && bk3.entries.len() == rep.pushed as usize
        && bk3.entries.len() <= 24, "注入回归幂等归并");

    // C397-19 确定性补扫：六维逐维注入后齐备。
    for d in AssetDim::ALL.iter() {
        bk3.put(AssetEntry { dim: *d, owner: "SWEEP", card: "SW0", pointer: 0xB000, a11y: false });
    }
    chk(&mut set, "C397-19", bk3.dims_complete() && bk3.missing_dims().is_empty()
        && probe.probe_empty(&bk3).is_ok(), "确定性补扫六维齐");

    // C397-20 深化变体段位不撞段 + 空集投影退化安全。
    let es = PointerStats::of_pointers(&[]);
    chk(&mut set, "C397-20", AsCode::ProbeReject as u16 == 0x0044
        && AsCode::QuantileBad as u16 == 0x0045 && AsCode::InjectReject as u16 == 0x0046
        && es.count == 0 && es.conserved(), "段位正确空集退化");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves397_smoke() {
        let set = run_ves397_checks();
        assert!(set.failed_count() == 0, "ves397 red: {}", set.red_items_text());
    }

    #[test]
    fn ves397_link_def() {
        let e = AssetEntry { dim: AssetDim::Tool, owner: "T08", card: "", pointer: 5, a11y: false };
        assert!(!e.linked());
    }
}
