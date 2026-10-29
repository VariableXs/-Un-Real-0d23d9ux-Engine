//! UNX-F56024 mmap/munmap 对延迟基准件（0x602E0 段）
//!
//! 锚点：UNX-F56024-J1 映射粒度 4K/64K/2M 三档配对计时（建/拆分离账
//! + 合并口径双账）；J2 页缓存态冷/热分账（同档不许混样）；J3 双机对
//! 照 ×3 档全落（逐档 ratio_p95）三判据全绿。判据锚定 §AI-71.2 专题
//! 二①。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·映射对面）。
//!
//! 结构：粒度闭集登记册（4K/64K/2M 逐档）+ 配对计时核（建账 × 拆账
//! 逐对落账，合并口径由对推导）+ 缓存态分账（冷/热闭集逐账分离）+
//! 对照行生成核 + 注入核（越闭集粒度/越闭集缓存态/零纳秒/拆无建配对
//! /封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x602E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x602E0;
pub const MM_GRAN_UNK: u32 = SEG_BASE + 0x01; // 越闭集映射粒度
pub const MM_CACHE_UNK: u32 = SEG_BASE + 0x02; // 越闭集页缓存态
pub const MM_NS_ZERO: u32 = SEG_BASE + 0x03; // 零纳秒计时
pub const MM_PAIR_MISS: u32 = SEG_BASE + 0x04; // 拆无建配对破
pub const MM_SEALED: u32 = SEG_BASE + 0x05; // 配对账已封账拒写
pub const MM_STATS_MISS: u32 = SEG_BASE + 0x06; // 样本不足无统计
pub const MM_DUP_CELL: u32 = SEG_BASE + 0x07; // 同格重复建账

/// 映射粒度闭集。
pub const GR_4K: usize = 0;
pub const GR_64K: usize = 1;
pub const GR_2M: usize = 2;
pub const GR_COUNT: usize = 3;
pub const GR_BYTES: [u64; GR_COUNT] = [4_096, 65_536, 2_097_152];
pub const GR_NAMES: [&str; GR_COUNT] = ["4K", "64K", "2M"];

/// 页缓存态闭集。
pub const PC_COLD: usize = 0;
pub const PC_HOT: usize = 1;
pub const PC_COUNT: usize = 2;
pub const PC_NAMES: [&str; PC_COUNT] = ["cold", "hot"];

/// 格位（粒度 × 缓存态）。
pub fn cell_of(gran: usize, cache: usize) -> Result<usize, u32> {
    if gran >= GR_COUNT {
        return Err(MM_GRAN_UNK);
    }
    if cache >= PC_COUNT {
        return Err(MM_CACHE_UNK);
    }
    Ok(gran * PC_COUNT + cache)
}

/// 单格配对账：建账 × 拆账 × 合并口径。
pub struct PairCell {
    pub gran: usize,
    pub cache: usize,
    pub map_ns: Vec<u64>,
    pub unmap_ns: Vec<u64>,
    pub sealed: bool,
}

impl PairCell {
    pub fn new(gran: usize, cache: usize) -> Result<Self, u32> {
        if gran >= GR_COUNT {
            return Err(MM_GRAN_UNK);
        }
        if cache >= PC_COUNT {
            return Err(MM_CACHE_UNK);
        }
        Ok(PairCell {
            gran,
            cache,
            map_ns: Vec::new(),
            unmap_ns: Vec::new(),
            sealed: false,
        })
    }

    /// 建账计时（零值拒 + 封账拒）。
    pub fn record_map(&mut self, ns: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(MM_SEALED);
        }
        if ns == 0 {
            return Err(MM_NS_ZERO);
        }
        self.map_ns.push(ns);
        Ok(())
    }

    /// 拆账计时：必须已有建账（配对闸）。
    pub fn record_unmap(&mut self, ns: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(MM_SEALED);
        }
        if ns == 0 {
            return Err(MM_NS_ZERO);
        }
        if self.unmap_ns.len() >= self.map_ns.len() {
            return Err(MM_PAIR_MISS);
        }
        self.unmap_ns.push(ns);
        Ok(())
    }

    /// 合并口径：逐对 (map+unmap) 序列（取前 n 对）。
    pub fn combined(&self) -> Vec<u64> {
        let n = self.map_ns.len().min(self.unmap_ns.len());
        let mut out = Vec::new();
        for i in 0..n {
            out.push(self.map_ns[i] + self.unmap_ns[i]);
        }
        out
    }

    /// p95（口径选择：0=map / 1=unmap / 2=combined；样本 ≥30）。
    pub fn p95(&self, mode: usize) -> Result<u64, u32> {
        let v: Vec<u64> = match mode {
            0 => self.map_ns.clone(),
            1 => self.unmap_ns.clone(),
            2 => self.combined(),
            _ => return Err(MM_STATS_MISS),
        };
        if v.len() < 30 {
            return Err(MM_STATS_MISS);
        }
        let mut s = v.clone();
        s.sort();
        let idx = (s.len() * 95 / 100).min(s.len() - 1);
        Ok(s[idx])
    }

    /// 已配对数。
    pub fn pairs(&self) -> usize {
        self.map_ns.len().min(self.unmap_ns.len())
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 双账本：冷/热各一套六格。
pub struct CacheBook {
    pub cache: usize,
    pub cells: Vec<PairCell>,
    pub sealed: bool,
}

impl CacheBook {
    pub fn new(cache: usize) -> Result<Self, u32> {
        if cache >= PC_COUNT {
            return Err(MM_CACHE_UNK);
        }
        Ok(CacheBook { cache, cells: Vec::new(), sealed: false })
    }

    /// 取或建格（同格重复建账拒）。
    pub fn cell_mut(&mut self, gran: usize) -> Result<&mut PairCell, u32> {
        if self.sealed {
            return Err(MM_SEALED);
        }
        if gran >= GR_COUNT {
            return Err(MM_GRAN_UNK);
        }
        for c in self.cells.iter_mut() {
            if c.gran == gran {
                return Err(MM_DUP_CELL);
            }
        }
        self.cells.push(PairCell::new(gran, self.cache)?);
        Ok(self.cells.last_mut().expect("pushed"))
    }

    /// 查格。
    pub fn cell(&self, gran: usize) -> Option<&PairCell> {
        for c in self.cells.iter() {
            if c.gran == gran {
                return Some(c);
            }
        }
        None
    }

    /// 三档全落账。
    pub fn full(&self) -> bool {
        self.cells.len() == GR_COUNT
            && self.cells.iter().all(|c| c.pairs() >= 30)
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
        for c in self.cells.iter_mut() {
            c.seal();
        }
    }
}

/// 标准夹具：冷/热双册 × 三档（建 = 500×(gran+1)+i；拆 = 建/2；40 对/格）。
pub fn standard_pair() -> (CacheBook, CacheBook) {
    let mut cold = CacheBook::new(PC_COLD).expect("cold");
    let mut hot = CacheBook::new(PC_HOT).expect("hot");
    for gran in 0..GR_COUNT {
        for book in [&mut cold, &mut hot].iter_mut() {
            let base = (gran as u64 + 1) * 500;
            let c = book.cell_mut(gran).expect("cell");
            for i in 0..40u64 {
                c.record_map(base + i).expect("map");
            }
            for i in 0..40u64 {
                c.record_unmap((base + i) / 2).expect("unmap");
            }
        }
    }
    (cold, hot)
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集粒度（gran=7）
    if PairCell::new(7, PC_COLD).err() == Some(MM_GRAN_UNK) {
        caught += 1;
    }
    // 2) 越闭集缓存态（cache=5）
    if CacheBook::new(5).err() == Some(MM_CACHE_UNK) {
        caught += 1;
    }
    // 3) 零纳秒计时
    let mut c = PairCell::new(GR_4K, PC_HOT).expect("c");
    if c.record_map(0).err() == Some(MM_NS_ZERO) {
        caught += 1;
    }
    // 4) 拆无建配对破
    let mut c2 = PairCell::new(GR_4K, PC_COLD).expect("c2");
    if c2.record_unmap(100).err() == Some(MM_PAIR_MISS) {
        caught += 1;
    }
    // 5) 同格重复建账
    let mut b = CacheBook::new(PC_COLD).expect("b");
    let _ = b.cell_mut(GR_64K);
    if b.cell_mut(GR_64K).err() == Some(MM_DUP_CELL) {
        caught += 1;
    }
    (caught, 5)
}

/// 对照行：档 × 缓存态 × 双机三口径 p95。
#[derive(Clone)]
pub struct PairCmpRow {
    pub gran: usize,
    pub cache: usize,
    pub map_ratio: u64,
    pub unmap_ratio: u64,
    pub combined_ratio: u64,
}

/// 逐档对照（ratio = win/varix 万分比；三口径双账）。
pub fn pair_cmp(v: &CacheBook, w: &CacheBook, cache: usize) -> Vec<PairCmpRow> {
    let mut out = Vec::new();
    for gran in 0..GR_COUNT {
        let (vc, wc) = (v.cell(gran), w.cell(gran));
        if let (Some(a), Some(b)) = (vc, wc) {
            let mut ratios: [u64; 3] = [0; 3];
            let mut ok = true;
            for mode in 0..3 {
                match (a.p95(mode), b.p95(mode)) {
                    (Ok(x), Ok(y)) if x > 0 => ratios[mode] = y * 10_000 / x,
                    _ => ok = false,
                }
            }
            if ok {
                out.push(PairCmpRow {
                    gran,
                    cache,
                    map_ratio: ratios[0],
                    unmap_ratio: ratios[1],
                    combined_ratio: ratios[2],
                });
            }
        }
    }
    out
}

/// 判据入口（CheckSet）。
pub fn run_unx56024_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56024");
    // J1-1 三档配对全落账（冷热双册 40 对/格）
    let (cold, hot) = standard_pair();
    let ok = cold.full() && hot.full();
    s.add("56024-J1-三档配对全落账", ok, if ok { "" } else { "配对账缺" });
    // J1-2 粒度闭集语义（4096/65536/2097152）
    let ok = GR_BYTES == [4_096, 65_536, 2_097_152] && GR_NAMES[2] == "2M";
    s.add("56024-J1-粒度闭集", ok, if ok { "" } else { "粒度漂移" });
    // J1-3 格位算式（gran×2+cache 双向可验）
    let ok = cell_of(GR_4K, PC_COLD) == Ok(0)
        && cell_of(GR_2M, PC_HOT) == Ok(5)
        && cell_of(9, 0).err() == Some(MM_GRAN_UNK)
        && cell_of(0, 9).err() == Some(MM_CACHE_UNK);
    s.add("56024-J1-格位算式", ok, if ok { "" } else { "格位漂移" });
    // J2-1 建/拆分离双账（map 账与 unmap 账各 40 独立）
    let c0 = cold.cell(GR_4K).expect("c0");
    let ok = c0.map_ns.len() == 40 && c0.unmap_ns.len() == 40 && c0.pairs() == 40;
    s.add("56024-J2-建拆分离双账", ok, if ok { "" } else { "建拆混账" });
    // J2-2 合并口径语义（第 0 对 = 500+250 = 750；第 39 对 = 539+269 = 808）
    let comb = c0.combined();
    let ok = comb.len() == 40 && comb[0] == 750 && comb[39] == 808;
    s.add("56024-J2-合并口径语义", ok, if ok { "" } else { "合并口径漂移" });
    // J2-3 三口径 p95（map：基线 500 步 1 → idx38=538；unmap：269；combined：807）
    let pm = c0.p95(0).expect("pm");
    let pu = c0.p95(1).expect("pu");
    let pc = c0.p95(2).expect("pc");
    let ok = pm == 538 && pu == 269 && pc == 807;
    s.add("56024-J2-三口径p95", ok, if ok { "" } else { "口径分位漂移" });
    // J2-4 冷/热分账不混（冷热同格账独立成册）
    let ok = cold.cache == PC_COLD && hot.cache == PC_HOT
        && cold.cell(GR_2M).expect("c2m").map_ns[0] == 1500
        && hot.cell(GR_2M).expect("c2h").map_ns[0] == 1500;
    s.add("56024-J2-冷热分账", ok, if ok { "" } else { "冷热混样" });
    // J3-1 双机对照 ×3 档全落（同夹具三口径 ratio 全 10000）
    let (v, w) = standard_pair();
    let rows = pair_cmp(&v, &w, PC_COLD);
    let mut all_eq = rows.len() == GR_COUNT;
    for r in rows.iter() {
        if r.map_ratio != 10_000 || r.unmap_ratio != 10_000 || r.combined_ratio != 10_000 {
            all_eq = false;
        }
    }
    let ok = all_eq;
    s.add("56024-J3-对照三档齐", ok, if ok { "" } else { "对照行缺" });
    // J3-2 对照 ratio 敏感（win 建账翻倍 → map_ratio=20000）
    let mut w2 = CacheBook::new(PC_COLD).expect("w2");
    for gran in 0..GR_COUNT {
        let base = (gran as u64 + 1) * 1000;
        let c = w2.cell_mut(gran).expect("cell2");
        for i in 0..40u64 {
            c.record_map(base + i).expect("map2");
        }
        for i in 0..40u64 {
            c.record_unmap((base + i) / 2).expect("unmap2");
        }
    }
    let rows2 = pair_cmp(&v, &w2, PC_COLD);
    // win2 4K p95=1038，varix p95=538 → 1038×10000/538 = 19293
    let ok = rows2.len() == GR_COUNT && rows2[0].map_ratio == 19_293;
    s.add("56024-J3-ratio敏感", ok, if ok { "" } else { "ratio 漂移" });
    // J3-3 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56024-J3-注入核必抓", ok, &detail);
    // J1-4 封账冻结
    let mut cold2 = standard_pair().0;
    cold2.seal();
    let ok = cold2.cell_mut(GR_4K).err() == Some(MM_SEALED);
    s.add("56024-J1-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J2-5 样本不足拒 p95（29 对）
    let mut c4 = PairCell::new(GR_64K, PC_HOT).expect("c4");
    for i in 0..29u64 {
        c4.record_map(100 + i).expect("m");
        c4.record_unmap(50).expect("u");
    }
    let ok = c4.p95(0).err() == Some(MM_STATS_MISS) && c4.p95(2).err() == Some(MM_STATS_MISS);
    s.add("56024-J2-短样本拒p95", ok, if ok { "" } else { "短样本出账" });
    // J1-5 缓存态名闭集同源
    let ok = PC_NAMES[PC_COLD] == "cold" && PC_NAMES[PC_HOT] == "hot";
    s.add("56024-J1-缓存态名同源", ok, if ok { "" } else { "命名漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56024_all_checks_green() {
        let set = run_unx56024_checks();
        assert!(set.all_passed(), "UNX-F56024 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56024_combined_math() {
        let (cold, _) = standard_pair();
        let comb = cold.cell(GR_4K).expect("c").combined();
        assert_eq!(comb[0], 750);
        assert_eq!(comb[39], 808);
    }
}
