//! UNX-F1783 · 跨批语义冲突扫描（B01–B09 判据/边界/依赖交叉核对）
//!
//! 诊断段 0x9D51~0x9D58（审计族扫描段）。
//! 判据（施工书 J1）：九批 180 条判据/语义边界/依赖声明交叉扫描完成，冲突清单归零或显式豁免登记。

use alloc::vec::Vec;

/// 扫描诊断码闭集（0x9D51~0x9D58 八码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanCode {
    /// 0x9D51 批内判据扫描完成。
    BatchScanned = 0x9D51,
    /// 0x9D52 跨批判据冲突检出。
    AssertClash = 0x9D52,
    /// 0x9D53 语义边界冲突检出。
    BoundClash = 0x9D53,
    /// 0x9D54 依赖声明冲突检出（A 依赖 B 但 B 声明不依赖 A）。
    DepClash = 0x9D54,
    /// 0x9D55 冲突豁免登记（显式豁免）。
    WaiverFiled = 0x9D55,
    /// 0x9D56 扫描账归零核通过。
    CleanSealed = 0x9D56,
    /// 0x9D57 未豁免冲突残留。
    ConflictResidue = 0x9D57,
    /// 0x9D58 扫描账守恒破坏。
    LedgerBroken = 0x9D58,
}

impl ScanCode {
    /// 三要素：现象。
    pub const fn what(self) -> &'static str {
        match self {
            ScanCode::BatchScanned => "批内判据扫描完成",
            ScanCode::AssertClash => "跨批判据冲突",
            ScanCode::BoundClash => "语义边界冲突",
            ScanCode::DepClash => "依赖声明互相矛盾",
            ScanCode::WaiverFiled => "冲突已显式豁免登记",
            ScanCode::CleanSealed => "冲突清单归零核通过",
            ScanCode::ConflictResidue => "存在未豁免冲突残留",
            ScanCode::LedgerBroken => "扫描账守恒破坏",
        }
    }
    /// 三要素：原因。
    pub const fn why(self) -> &'static str {
        match self {
            ScanCode::BatchScanned => "批内全量判据过扫",
            ScanCode::AssertClash => "两批判据对同一对象给出矛盾断言",
            ScanCode::BoundClash => "边界声明区间重叠且规则相斥",
            ScanCode::DepClash => "依赖图存在方向矛盾边",
            ScanCode::WaiverFiled => "冲突经审查属可接受设计取舍",
            ScanCode::CleanSealed => "全部冲突已豁免或消解",
            ScanCode::ConflictResidue => "冲突未走豁免流程",
            ScanCode::LedgerBroken => "检出数与账目不一致",
        }
    }
    /// 三要素：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            ScanCode::BatchScanned => "无需处置",
            ScanCode::AssertClash => "对齐两批判据口径",
            ScanCode::BoundClash => "重划边界或拆分区间",
            ScanCode::DepClash => "修正依赖方向声明",
            ScanCode::WaiverFiled => "登记后放行",
            ScanCode::CleanSealed => "无需处置",
            ScanCode::ConflictResidue => "补豁免登记或消解冲突",
            ScanCode::LedgerBroken => "冻结扫描账并上报 0x9D58",
        }
    }
}

/// 判据声明（扫描输入单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Assert {
    /// 所属批（1..=9 对应 B01..B09）。
    pub batch: u8,
    /// 判据指纹。
    pub fp: u64,
    /// 边界区间 (lo, hi)（无边界语义填 (0,0)）。
    pub bound: (u64, u64),
    /// 依赖目标批（0 = 无依赖）。
    pub dep: u8,
}

/// 冲突记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Clash {
    /// 冲突类型。
    pub kind: ScanCode,
    /// 双方批号。
    pub a: u8,
    pub b: u8,
}

/// 豁免登记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Waiver {
    /// 对应冲突序号。
    pub clash_idx: usize,
    /// 豁免理由码。
    pub reason: u32,
}

/// 跨批扫描引擎。
pub struct CrossScanner {
    /// 判据全集。
    pub asserts: Vec<Assert>,
    /// 冲突清单。
    clashes: Vec<Clash>,
    /// 豁免账。
    waivers: Vec<Waiver>,
    /// 已扫批位图。
    scanned: u16,
}

impl CrossScanner {
    /// 新扫描器。
    pub fn new() -> Self {
        CrossScanner { asserts: Vec::new(), clashes: Vec::new(), waivers: Vec::new(), scanned: 0 }
    }

    /// 批内扫描（登记批号）。
    pub fn scan_batch(&mut self, batch: u8, items: &[Assert]) -> ScanCode {
        for a in items {
            if a.batch == batch {
                self.asserts.push(*a);
            }
        }
        if batch >= 1 && batch <= 9 {
            self.scanned |= 1 << (batch - 1);
        }
        ScanCode::BatchScanned
    }

    /// 跨批交叉扫描：判据/边界/依赖三类核对。
    pub fn cross_scan(&mut self) -> usize {
        self.clashes.clear();
        let n = self.asserts.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let a = self.asserts[i];
                let b = self.asserts[j];
                if a.batch == b.batch {
                    continue;
                }
                // 判据冲突：同指纹不同批（同一判据被两批矛盾声明）。
                if a.fp == b.fp {
                    self.clashes.push(Clash { kind: ScanCode::AssertClash, a: a.batch, b: b.batch });
                    continue;
                }
                // 边界冲突：区间重叠且互斥规则（此处以完全相同区间 + 不同 dep 语义近似）。
                if a.bound != (0, 0) && a.bound == b.bound && a.dep != b.dep && a.dep != 0 && b.dep != 0 {
                    self.clashes.push(Clash { kind: ScanCode::BoundClash, a: a.batch, b: b.batch });
                    continue;
                }
                // 依赖冲突：A 批判据依赖 B 批但 B 批判据依赖 A 批（双向依赖环）。
                if a.dep == b.batch && b.dep == a.batch {
                    self.clashes.push(Clash { kind: ScanCode::DepClash, a: a.batch, b: b.batch });
                }
            }
        }
        self.clashes.len()
    }

    /// 豁免登记（显式豁免）。
    pub fn waive(&mut self, clash_idx: usize, reason: u32) -> ScanCode {
        if clash_idx >= self.clashes.len() {
            return ScanCode::LedgerBroken;
        }
        self.waivers.push(Waiver { clash_idx, reason });
        ScanCode::WaiverFiled
    }

    /// 归零核：全部冲突已豁免 → CleanSealed；否则残留。
    pub fn seal_check(&self) -> ScanCode {
        if self.clashes.is_empty() {
            return ScanCode::CleanSealed;
        }
        let mut waived = [false; 64];
        for w in &self.waivers {
            if w.clash_idx < 64 {
                waived[w.clash_idx] = true;
            }
        }
        for (i, _) in self.clashes.iter().enumerate() {
            if i < 64 && !waived[i] {
                return ScanCode::ConflictResidue;
            }
        }
        ScanCode::CleanSealed
    }

    /// 冲突清单。
    pub fn clash_list(&self) -> &[Clash] {
        &self.clashes
    }

    /// 九批全覆盖核。
    pub const fn full_coverage(&self) -> bool {
        self.scanned == 0x1FF
    }

    /// 账守恒：豁免序号全部落在冲突清单内。
    pub fn ledger_consistent(&self) -> bool {
        self.waivers.iter().all(|w| w.clash_idx < self.clashes.len())
    }
}

/// 判据测试（段 0x9D51~0x9D58）。
pub fn run_t783_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t783");
    // c1 九批全扫覆盖。
    let mut sc = CrossScanner::new();
    for b in 1..=9u8 {
        let _ = sc.scan_batch(b, &[Assert { batch: b, fp: 0x1000 + b as u64, bound: (0, 0), dep: 0 }]);
    }
    set.add("t783-c1-nine-batches", sc.full_coverage() && sc.asserts.len() == 9, "");
    // c2 判据冲突检出。
    let mut sc2 = CrossScanner::new();
    let _ = sc2.scan_batch(1, &[Assert { batch: 1, fp: 0xBEEF, bound: (0, 0), dep: 0 }]);
    let _ = sc2.scan_batch(2, &[Assert { batch: 2, fp: 0xBEEF, bound: (0, 0), dep: 0 }]);
    let n = sc2.cross_scan();
    set.add("t783-c2-assert-clash", n == 1 && sc2.clash_list()[0].kind == ScanCode::AssertClash, "");
    // c3 依赖环检出。
    let mut sc3 = CrossScanner::new();
    let _ = sc3.scan_batch(3, &[Assert { batch: 3, fp: 1, bound: (0, 0), dep: 4 }]);
    let _ = sc3.scan_batch(4, &[Assert { batch: 4, fp: 2, bound: (0, 0), dep: 3 }]);
    let n3 = sc3.cross_scan();
    set.add("t783-c3-dep-cycle", n3 == 1 && sc3.clash_list()[0].kind == ScanCode::DepClash, "");
    // c4 边界冲突检出。
    let mut sc4 = CrossScanner::new();
    let _ = sc4.scan_batch(5, &[Assert { batch: 5, fp: 10, bound: (100, 200), dep: 6 }]);
    let _ = sc4.scan_batch(6, &[Assert { batch: 6, fp: 11, bound: (100, 200), dep: 7 }]);
    let n4 = sc4.cross_scan();
    set.add("t783-c4-bound-clash", n4 == 1 && sc4.clash_list()[0].kind == ScanCode::BoundClash, "");
    // c5 豁免登记 + 归零核。
    let w = sc4.waive(0, 0x1);
    set.add("t783-c5-waiver-seal", w == ScanCode::WaiverFiled && sc4.seal_check() == ScanCode::CleanSealed && sc4.ledger_consistent(), "");
    // c6 未豁免残留。
    set.add("t783-c6-residue", sc2.seal_check() == ScanCode::ConflictResidue, "");
    // c7 干净扫描归零。
    let mut sc7 = CrossScanner::new();
    for b in 1..=9u8 {
        let _ = sc7.scan_batch(b, &[Assert { batch: b, fp: b as u64 * 100, bound: (b as u64 * 10, b as u64 * 10 + 5), dep: 0 }]);
    }
    let n7 = sc7.cross_scan();
    set.add("t783-c7-clean-zero", n7 == 0 && sc7.seal_check() == ScanCode::CleanSealed, "");
    // c8 LCG 512 轮注入：随机判据集扫描，账守恒恒成立。
    let mut seed: u64 = 0x7830_9D51_0000_0001;
    let mut lc = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    let mut inj_ok = true;
    for _ in 0..512u32 {
        let mut s = CrossScanner::new();
        for b in 1..=9u8 {
            let mut items = Vec::new();
            for _ in 0..((lc() % 4) + 1) {
                items.push(Assert {
                    batch: b,
                    fp: lc() % 5,
                    bound: if lc() % 2 == 0 { (lc() % 100, lc() % 100 + 100) } else { (0, 0) },
                    dep: (lc() % 10) as u8,
                });
            }
            let _ = s.scan_batch(b, &items);
        }
        let n = s.cross_scan();
        for i in 0..n {
            let _ = s.waive(i, (lc() % 1000) as u32);
        }
        inj_ok &= s.ledger_consistent();
    }
    set.add("t783-c8-lcg-inject", inj_ok, "");
    // 三要素自核。
    let mut tri_ok = true;
    for c in [
        ScanCode::BatchScanned,
        ScanCode::AssertClash,
        ScanCode::BoundClash,
        ScanCode::DepClash,
        ScanCode::WaiverFiled,
        ScanCode::CleanSealed,
        ScanCode::ConflictResidue,
        ScanCode::LedgerBroken,
    ] {
        tri_ok &= !c.what().is_empty() && !c.why().is_empty() && !c.advice().is_empty();
    }
    set.add("t783-c9-triad", tri_ok, "");
    set
}
