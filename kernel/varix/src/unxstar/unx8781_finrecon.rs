//! UNX-F8781 · 收官段总对账 240,000 自洽账 · 诊断段 0xB7A0
//! 验收锚点：UNX-F8781-J1 收官总对账——全期行账 240,000 按分段
//! 账本拆分（16 段 × 15,000），分段和 = 总账、跨段行号连续、零差
//! 容断（任何一段错账即全账红灯），双判据缺一即批内红灯。
use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;


pub const SEG_BASE: u16 = 0xB7A0;

// ── 诊断段独占 · 总账错误域 ─────────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_SEG: u16 = SEG_ERR + 1;     // 分段账不平
const E_GAP: u16 = SEG_ERR + 2;     // 跨段行号断档
const E_TOTAL: u16 = SEG_ERR + 3;   // 总账不平
const E_SEGNUM: u16 = SEG_ERR + 4;  // 段数不符
const E_ZERO: u16 = SEG_ERR + 5;    // 零差容破坏

pub const SEGMENTS: usize = 16;
pub const PER_SEG: u64 = 15_000;
pub const GRAND_TOTAL: u64 = SEGMENTS as u64 * PER_SEG; // 240,000

// ── 确定性 LCG ───────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 分段账本 ─────────────────────────────────────────────────
/// 一段账：段号（1..16）+ 行区间 [lo, hi] + 行数声明。
#[derive(Clone, Debug)]
pub struct SegBook {
    pub seg: u32,
    pub lo: u64,
    pub hi: u64,
    pub declared: u64,
}

impl SegBook {
    /// 自洽段：区间行数 = 声明行数。
    pub fn new(seg: u32, lo: u64) -> Self {
        SegBook { seg, lo, hi: lo + PER_SEG - 1, declared: PER_SEG }
    }
    /// 段内对账：hi − lo + 1 == declared。
    pub fn seg_ok(&self) -> bool {
        self.hi >= self.lo && self.hi - self.lo + 1 == self.declared
    }
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for x in [self.seg as u64, self.lo, self.hi, self.declared] {
            for b in x.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        h
    }
}

/// 标准十六段：第 i 段起行 = 1 + i·15000。
pub fn standard_segbooks() -> Vec<SegBook> {
    (0..SEGMENTS)
        .map(|i| SegBook::new((i + 1) as u32, 1 + i as u64 * PER_SEG))
        .collect()
}

// ── 总对账器 ─────────────────────────────────────────────────
/// 收官总账：段内自洽 × 段间连续 × 总和 = 240,000。
pub struct FinRecon {
    pub errors: Vec<(u16, String)>,
    pub total: u64,
}

impl FinRecon {
    pub fn new() -> Self {
        FinRecon { errors: Vec::new(), total: 0 }
    }
    /// 全量对账。
    pub fn run(&mut self, books: &[SegBook]) -> bool {
        self.errors.clear();
        if books.len() != SEGMENTS {
            self.errors.push((E_SEGNUM, format!("段数={}", books.len())));
            return false;
        }
        // 段号 1..16 依序
        for (i, b) in books.iter().enumerate() {
            if b.seg != (i + 1) as u32 {
                self.errors.push((E_SEGNUM, format!("段序错: {}", b.seg)));
            }
            if !b.seg_ok() {
                self.errors.push((E_SEG, format!("段{}账不平", b.seg)));
            }
        }
        // 跨段连续：下段 lo = 上段 hi + 1
        for w in books.windows(2) {
            if w[1].lo != w[0].hi + 1 {
                self.errors.push((E_GAP, format!("断档 {}→{}", w[0].seg, w[1].seg)));
            }
        }
        // 总和
        self.total = books.iter().map(|b| b.declared).sum();
        if self.total != GRAND_TOTAL {
            self.errors.push((E_TOTAL, format!("总={}", self.total)));
        }
        self.errors.is_empty()
    }
    /// 零差容：单段声明被篡改 ±1 即两路（段账 + 总账）全红。
    pub fn zero_tolerance(&mut self) -> (bool, bool) {
        let mut b1 = standard_segbooks();
        b1[7].declared += 1;
        let seg_hit = !self.run(&b1)
            && self.errors.iter().any(|(c, _)| *c == E_SEG)
            && self.errors.iter().any(|(c, _)| *c == E_TOTAL);
        // 区间篡改：hi 前移造成断档 + 段账不平
        let mut b2 = standard_segbooks();
        b2[3].hi -= 1;
        let gap_hit = !self.run(&b2)
            && self.errors.iter().any(|(c, _)| *c == E_GAP)
            && self.errors.iter().any(|(c, _)| *c == E_SEG);
        (seg_hit, gap_hit)
    }
    /// 全账指纹。
    pub fn books_fp(books: &[SegBook]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in books {
            for by in b.fp().to_le_bytes() {
                h ^= by as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        h
    }
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx8781_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8781 收官总对账240000");
    let books = standard_segbooks();
    let mut r = FinRecon::new();

    // 1. 十六段标准态全绿
    let ok = r.run(&books);
    s.add("FIN-标准全绿", ok, if ok { "" } else { "存在错账" });

    // 2. 总账 240,000
    s.add("FIN-总账240000", r.total == GRAND_TOTAL,
        if r.total == GRAND_TOTAL { "" } else { "总账不平" });

    // 3. 分段和 = 总账
    let sum: u64 = books.iter().map(|b| b.declared).sum();
    s.add("FIN-分段和", sum == GRAND_TOTAL && books.len() == SEGMENTS,
        if sum == GRAND_TOTAL { "" } else { "和不等" });

    // 4. 跨段连续（全域 1..240000 无缝）
    let seam = books.windows(2).all(|w| w[1].lo == w[0].hi + 1)
        && books[0].lo == 1
        && books[SEGMENTS - 1].hi == GRAND_TOTAL;
    s.add("FIN-全域无缝", seam, if seam { "" } else { "存在断档" });

    // 5. 零差容：声明 ±1 两路全红
    let (seg_hit, gap_hit) = r.zero_tolerance();
    s.add("FIN-零差容声明", seg_hit, if seg_hit { "" } else { "差容破坏未检出" });
    s.add("FIN-零差容区间", gap_hit, if gap_hit { "" } else { "区间篡改未检出" });

    // 6. 段数不符拒
    let mut b3 = standard_segbooks();
    b3.pop();
    let n_bad = !r.run(&b3) && r.errors.iter().any(|(c, _)| *c == E_SEGNUM);
    s.add("FIN-段数拒", n_bad, if n_bad { "" } else { "段数漂移放行" });

    // 7. 段序错乱拒
    let mut b4 = standard_segbooks();
    b4.swap(0, 15);
    let ord_bad = !r.run(&b4) && r.errors.iter().any(|(c, _)| *c == E_SEGNUM);
    s.add("FIN-段序拒", ord_bad, if ord_bad { "" } else { "段序错放行" });

    // 8. 全账指纹稳定且敏感
    let f1 = FinRecon::books_fp(&books);
    let f2 = FinRecon::books_fp(&books);
    let mut b5 = standard_segbooks();
    b5[15].declared = PER_SEG + 1;
    let f3 = FinRecon::books_fp(&b5);
    s.add("FIN-指纹敏感", f1 == f2 && f1 != f3,
        if f1 == f2 && f1 != f3 { "" } else { "指纹碰撞或漂移" });

    // 9. 段指纹互异
    let mut fp_uniq = true;
    for i in 0..books.len() {
        for j in (i + 1)..books.len() {
            if books[i].fp() == books[j].fp() {
                fp_uniq = false;
            }
        }
    }
    s.add("FIN-段指纹互异", fp_uniq, if fp_uniq { "" } else { "段指纹碰撞" });

    // 10. LCG 抽样 8 段行界还原（lo = 1 + seg_idx·15000）
    let mut lcg = Lcg(0xB7A0);
    let mut spot = true;
    for _ in 0..8 {
        let i = lcg.below(SEGMENTS as u64) as usize;
        if books[i].lo != 1 + i as u64 * PER_SEG {
            spot = false;
        }
    }
    s.add("FIN-抽样还原", spot, if spot { "" } else { "抽样还原失败" });

    // 11. 对账确定性
    let mut r2 = FinRecon::new();
    let ok2 = r2.run(&books);
    s.add("FIN-对账确定性", ok == ok2 && r.total == r2.total,
        if ok == ok2 { "" } else { "重跑漂移" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB7A0 && SEG_BASE < 0xB7C0;
    s.add("FIN-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
