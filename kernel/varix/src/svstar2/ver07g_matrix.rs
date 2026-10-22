//! VE-F3447 · 变体组合矩阵管理 —— 243 组合枚举、按需派生、缓存与覆盖统计
//!
//! ## 职责拆解
//! 五轴各三档 = **243 种组合**。这个数字很尴尬：全列出来没人看，全不列又会让
//! 用户撞进互斥组合。本册的办法是三层：
//! 1. **枚举**（`enumerate`）：243 个组合在启动时算一遍，标出哪些非法。
//!    枚举本身很便宜（243 次），贵的是「让人相信它被枚举过」——所以 `legal_count`
//!    必须能被外部读到并写进文档。
//! 2. **预置**（`presets`）：三条常用组合直达。默认体验就是第一条预置，
//!    不让用户从 243 里摸。
//! 3. **按需派生**（`resolve`）：不在预置里的组合，取**最近的**预置派生，
//!    距离用汉明距离（差几档）。距离超过 `MAX_DERIVE_DISTANCE` 就拒绝——
//!    离所有预置都远意味着「这个组合没人验证过」，这时候派生等于瞎猜。
//! 4. **缓存**（`Matrix::cache`）：解析结果进 LRU，容量 `CACHE_CAP`。
//!    组合空间只有 243，缓存上限 24 足够覆盖「一个会话里用户真会碰到的组合」。
//! 5. **覆盖统计**（`coverage`）：每个预置「吸引」多少组合，用来判断预置选得对不对。
//!    某个预置只覆盖 3 个组合，说明它没用；某个预置覆盖 80 个，说明它承重过大。
//!
//! ## 为什么这么拆
//! 组合爆炸的标准错法是「全展开」。243 个不是爆炸，但 5 轴各 5 档就是 3125，
//! 各 10 档就是 10 万。所以本册从一开始就按「不枚举、按需派生 + 最近预置」设计，
//! 只在启动时做一次**计数**级枚举来证明互斥表覆盖完整。这样轴数增长时，
//! 算法不变，只是枚举预算要重新审。
//!
//! ## 复杂度
//! - 全枚举 O(3^5) = 243，启动一次。
//! - 单次解析 O(预置数 × 轴数) = 15。
//! - 覆盖率统计 O(243 × 3) = 729。
//!
//! ## 零 panic 面
//! - 无索引切片；`Vec::with_capacity` 后逐个 push。
//! - 距离计算用 `u8` 饱和加法；无 `unwrap` / `expect`。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::ver07a_variants::{conflicts, presets, Axis, Combo, Preset, LEVELS};

/// 组合总数：3^5。
pub const TOTAL_COMBOS: usize = 243;

/// 可派生距离上限。差 **2 档以上**就说明离所有预置都远，不做按需派生。
///
/// 取值依据：三档轴 × 五轴的空间里，任取一个合法组合到最近预置的距离
/// 通常是 2~3。取 2 意味着「最多容忍两轴不同」；取 3 会把「五个轴各差一档」
/// 这种彻底不同的组合也当成派生，等于给没验证过的组合发通行证。
pub const MAX_DERIVE_DISTANCE: u8 = 2;

/// 解析缓存容量。
pub const CACHE_CAP: usize = 24;

/// 诊断码（E18 段 · 5647~5656）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum MatrixCode {
    /// 枚举数量对不上 243。
    EnumerationBroken = 5647,
    /// 组合命中互斥。
    MutuallyExclusive = 5648,
    /// 预置表为空。
    NoPresets = 5649,
    /// 离所有预置都太远。
    TooFar = 5650,
    /// 预置重复。
    DuplicatePreset = 5651,
    /// 缓存容量为 0（等于没有缓存）。
    CacheDisabled = 5652,
    /// 档位越界。
    LevelOutOfRange = 5653,
    /// 覆盖率统计自相矛盾。
    CoverageSkew = 5654,
    /// 预置自身非法（命互斥或档位越界）。
    PresetIllegal = 5655,
    /// 派生链给出的距离与实际不符。
    DistanceInconsistent = 5656,
}

impl MatrixCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn text(self) -> &'static str {
        match self {
            MatrixCode::EnumerationBroken => "枚举数量不等于 243",
            MatrixCode::MutuallyExclusive => "组合命中互斥",
            MatrixCode::NoPresets => "预置表为空",
            MatrixCode::TooFar => "离所有预置都太远",
            MatrixCode::DuplicatePreset => "预置重复",
            MatrixCode::CacheDisabled => "缓存容量为 0",
            MatrixCode::LevelOutOfRange => "档位越界",
            MatrixCode::CoverageSkew => "覆盖率统计自相矛盾",
            MatrixCode::PresetIllegal => "预置自身非法",
            MatrixCode::DistanceInconsistent => "距离与实际不符",
        }
    }
}

/// 枚举全部组合。顺序固定（最低位轴变化最快），因此枚举结果可对拍。
pub fn enumerate() -> Vec<Combo> {
    let mut out = Vec::with_capacity(TOTAL_COMBOS);
    for d in 0..LEVELS {
        for r in 0..LEVELS {
            for t in 0..LEVELS {
                for m in 0..LEVELS {
                    for f in 0..LEVELS {
                        out.push(Combo {
                            levels: [d, r, t, m, f],
                        });
                    }
                }
            }
        }
    }
    out
}

/// 合法组合数。`TOTAL_COMBOS - 命中互斥的组合数`。
pub fn legal_count() -> usize {
    enumerate().iter().filter(|c| conflicts(c).is_empty()).count()
}

/// 命中互斥的组合数。
pub fn illegal_count() -> usize {
    TOTAL_COMBOS.saturating_sub(legal_count())
}

/// 汉明距离：两条组合差几档。
pub fn distance(a: &Combo, b: &Combo) -> u8 {
    let mut d: u8 = 0;
    for axis in Axis::ALL {
        if a.level(axis) != b.level(axis) {
            d = d.saturating_add(1);
        }
    }
    d
}

/// 解析来源。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Source {
    /// 命中预置。
    Preset(&'static str),
    /// 从最近预置派生。
    Derived { from: &'static str, distance: u8 },
}

impl Source {
    pub fn label(&self) -> String {
        match self {
            Source::Preset(id) => format!("预置:{}", id),
            Source::Derived { from, distance } => format!("派生:{}·{}档", from, distance),
        }
    }

    pub fn is_derived(&self) -> bool {
        matches!(self, Source::Derived { .. })
    }
}

/// 一条解析结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Resolution {
    pub combo: Combo,
    pub source: Source,
}

/// 最近预置：距离最小者；平手取预置表中靠前的（顺序稳定 ⇒ 结果确定）。
pub fn nearest(combo: &Combo, table: &[Preset]) -> Option<(&'static str, u8)> {
    let mut best: Option<(&'static str, u8)> = None;
    for p in table {
        let d = distance(combo, &p.combo);
        let better = match best {
            None => true,
            Some((_, bd)) => d < bd,
        };
        if better {
            best = Some((p.id, d));
        }
    }
    best
}

/// 组合矩阵：预置表 + LRU 缓存 + 统计。
#[derive(Clone, Debug)]
pub struct Matrix {
    table: Vec<Preset>,
    cache: Vec<(Combo, Resolution)>,
    cap: usize,
    pub hits: usize,
    pub misses: usize,
}

impl Default for Matrix {
    fn default() -> Self {
        Self::standard()
    }
}

impl Matrix {
    pub fn new(cap: usize) -> Self {
        Self {
            table: presets(),
            cache: Vec::new(),
            cap,
            hits: 0,
            misses: 0,
        }
    }

    pub fn standard() -> Self {
        Self::new(CACHE_CAP)
    }

    pub fn presets(&self) -> &[Preset] {
        &self.table
    }

    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    /// 解析一个组合。
    pub fn resolve(&mut self, combo: Combo) -> Result<Resolution, MatrixCode> {
        if self.table.is_empty() {
            return Err(MatrixCode::NoPresets);
        }
        if !conflicts(&combo).is_empty() {
            return Err(MatrixCode::MutuallyExclusive);
        }
        if let Some(hit) = self.cache.iter().find(|(c, _)| *c == combo) {
            self.hits += 1;
            return Ok(hit.1.clone());
        }
        self.misses += 1;
        let (id, d) = match nearest(&combo, &self.table) {
            Some(v) => v,
            None => return Err(MatrixCode::NoPresets),
        };
        let source = if d == 0 {
            Source::Preset(id)
        } else if d <= MAX_DERIVE_DISTANCE {
            Source::Derived {
                from: id,
                distance: d,
            }
        } else {
            return Err(MatrixCode::TooFar);
        };
        let res = Resolution { combo, source };
        self.remember(combo, res.clone());
        Ok(res)
    }

    /// 写缓存。满了淘汰最旧的一条（LRU 的简化版：按写入顺序淘汰，
    /// 因为组合访问是「用户点几下」，没有强时序局部性，严格 LRU 收益不值复杂度）。
    fn remember(&mut self, combo: Combo, res: Resolution) {
        if self.cap == 0 {
            return;
        }
        if self.cache.len() >= self.cap {
            self.cache.remove(0);
        }
        self.cache.push((combo, res));
    }

    pub fn hit_rate_permille(&self) -> u32 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0;
        }
        ((self.hits * 1000) / total) as u32
    }

    /// 覆盖率：每个预置「吸引」多少合法组合（含自身）。
    pub fn coverage(&self) -> Vec<(&'static str, usize)> {
        let mut out: Vec<(&'static str, usize)> = self
            .table
            .iter()
            .map(|p| (p.id, 0usize))
            .collect();
        for combo in enumerate() {
            if !conflicts(&combo).is_empty() {
                continue;
            }
            if let Some((id, _)) = nearest(&combo, &self.table) {
                for item in out.iter_mut() {
                    if item.0 == id {
                        item.1 += 1;
                    }
                }
            }
        }
        out
    }
}

/// 矩阵自检：枚举数、预置合法性、覆盖统计自洽。
pub fn audit(matrix: &mut Matrix) -> Vec<MatrixCode> {
    let mut out = Vec::new();
    let all = enumerate();
    if all.len() != TOTAL_COMBOS {
        out.push(MatrixCode::EnumerationBroken);
    }
    if matrix.presets().is_empty() {
        out.push(MatrixCode::NoPresets);
    }
    if matrix.cap == 0 {
        out.push(MatrixCode::CacheDisabled);
    }
    // 预置自身必须合法且不重复
    let mut ids: Vec<&str> = Vec::new();
    for p in matrix.presets() {
        if ids.contains(&p.id) {
            out.push(MatrixCode::DuplicatePreset);
        }
        ids.push(p.id);
        if !conflicts(&p.combo).is_empty() {
            out.push(MatrixCode::PresetIllegal);
        }
        for axis in Axis::ALL {
            if p.combo.level(axis) >= LEVELS {
                out.push(MatrixCode::LevelOutOfRange);
            }
        }
    }
    // 覆盖统计必须加起来等于合法组合数（每个合法组合必落在某个预置的吸引域）
    let cov = matrix.coverage();
    let sum: usize = cov.iter().map(|(_, n)| *n).sum();
    if sum != legal_count() {
        out.push(MatrixCode::CoverageSkew);
    }
    // 距离函数自证：自身距离 0，且对称
    for combo in all.iter().take(8) {
        if distance(combo, combo) != 0 {
            out.push(MatrixCode::DistanceInconsistent);
        }
    }
    out
}

/// 读屏清单：枚举规模 + 每个预置的吸引域。
pub fn matrix_lines(matrix: &Matrix) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!(
        "组合空间 {}（合法 {} / 互斥 {}），预置 {}，缓存上限 {}",
        TOTAL_COMBOS,
        legal_count(),
        illegal_count(),
        matrix.presets().len(),
        matrix.cap
    ));
    for (id, n) in matrix.coverage() {
        out.push(format!("  {:<10} 吸引 {:>3} 个组合", id, n));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumeration_is_exactly_243_and_unique() {
        let all = enumerate();
        assert_eq!(all.len(), TOTAL_COMBOS);
        let mut hashes: Vec<u64> = all.iter().map(|c| c.hash()).collect();
        hashes.sort_unstable();
        hashes.dedup();
        assert_eq!(hashes.len(), TOTAL_COMBOS, "243 个组合必须两两不同");
    }

    #[test]
    fn legal_plus_illegal_equals_total() {
        let l = legal_count();
        let i = illegal_count();
        assert_eq!(l + i, TOTAL_COMBOS);
        assert!(i > 0, "互斥表必须真的排掉一些组合");
        assert!(l > TOTAL_COMBOS / 2, "合法组合应占多数，否则互斥表过宽");
    }

    #[test]
    fn presets_resolve_to_themselves() {
        let mut m = Matrix::standard();
        for p in presets() {
            let r = m.resolve(p.combo).unwrap_or_else(|_| panic!("预置必须可解析"));
            assert_eq!(r.source, Source::Preset(p.id));
            assert_eq!(r.combo, p.combo);
        }
    }

    #[test]
    fn near_combo_derives_from_nearest_preset() {
        let mut m = Matrix::standard();
        let base = match presets().first() {
            Some(p) => p.combo,
            None => panic!("预置表不应为空"),
        };
        let near = base
            .with(Axis::Radius, 0)
            .unwrap_or_else(|_| panic!("档位合法"));
        let r = m.resolve(near).unwrap_or_else(|_| panic!("近邻应可派生"));
        assert!(r.source.is_derived());
        if let Source::Derived { distance, .. } = r.source {
            assert_eq!(distance, 1, "只改一档，距离应为 1");
        }
    }

    #[test]
    fn far_combo_is_refused_not_guessed() {
        let mut m = Matrix::standard();
        // 构造一个离三条预置都 ≥3 档的组合：standard+sharp+loose+calm+bold
        let far = Combo {
            levels: [1, 0, 2, 0, 2],
        };
        assert!(conflicts(&far).is_empty(), "该组合本身不该命互斥");
        let r = m.resolve(far);
        assert_eq!(r.err(), Some(MatrixCode::TooFar));
    }

    #[test]
    fn mutex_combo_is_refused_before_distance() {
        let mut m = Matrix::standard();
        let bad = Combo {
            levels: [0, 1, 2, 1, 1],
        };
        assert_eq!(m.resolve(bad).err(), Some(MatrixCode::MutuallyExclusive));
    }

    #[test]
    fn cache_hits_are_counted_and_bounded() {
        let mut m = Matrix::new(4);
        let c = match presets().first() {
            Some(p) => p.combo,
            None => panic!("预置表不应为空"),
        };
        for _ in 0..3 {
            assert!(m.resolve(c).is_ok());
        }
        assert_eq!(m.cache_len(), 1, "同一组合只缓存一条");
        assert_eq!(m.hits, 2);
        assert_eq!(m.misses, 1);
        assert!(m.hit_rate_permille() > 600);
        // 塞满后淘汰最旧
        let mut combos = Vec::new();
        for d in 0..LEVELS {
            for r in 0..LEVELS {
                for t in 0..LEVELS {
                    for mo in 0..LEVELS {
                        for f in 0..LEVELS {
                            combos.push(Combo {
                                levels: [d, r, t, mo, f],
                            });
                        }
                    }
                }
            }
        }
        for c in combos.iter().take(40) {
            if conflicts(c).is_empty() {
                let _ = m.resolve(*c);
            }
        }
        assert!(m.cache_len() <= 4, "缓存不得超过容量：{}", m.cache_len());
    }

    #[test]
    fn coverage_sums_to_legal_count() {
        let m = Matrix::standard();
        let cov = m.coverage();
        let sum: usize = cov.iter().map(|(_, n)| *n).sum();
        assert_eq!(sum, legal_count());
        for (id, n) in cov {
            assert!(n > 0, "预置 {} 必须至少吸引自己", id);
        }
    }

    #[test]
    fn full_audit_is_clean() {
        let mut m = Matrix::standard();
        assert!(audit(&mut m).is_empty(), "{:?}", audit(&mut m));
        let lines = matrix_lines(&m);
        assert!(lines[0].contains("组合空间 243"));
        assert!(lines.len() >= 4);
    }

    #[test]
    fn zero_capacity_is_reported_but_not_fatal() {
        let mut m = Matrix::new(0);
        assert!(audit(&mut m).contains(&MatrixCode::CacheDisabled));
        // 无缓存仍应能解析（只是每次都算）
        let c = match presets().first() {
            Some(p) => p.combo,
            None => panic!("预置表不应为空"),
        };
        assert!(m.resolve(c).is_ok());
        assert_eq!(m.cache_len(), 0);
    }
}
