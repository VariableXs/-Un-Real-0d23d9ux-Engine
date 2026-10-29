//! CGPU-F0724 · 负载画像验证（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥320 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0724，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：负载画像验证——网页负载八类画像（千分比权重）的
//!    结构校验、权重守恒（Σ=1000‰）、区间合规、画像指纹
//!    与版本漂移检测。
//!
//! 设计纪律：
//! - 八类闭集：Dom/Style/Layout/Paint/Script/Net/Decode/Gpu，
//  类别序固定，禁裸索引。
// - 权重守恒恒等式：Σ权重 == 1000‰（显式校验，偏差即拒绝）。
// - 区间合规表写死：每类 [min,max] 千分比（规格默认档）。
// - 指纹：FNV-1a 64 按类序折叠权重；同画像同指纹。
// - 漂移闭集：NoDrift/Minor(≤DRIFT_PM)/Major（三态），
//   DRIFT_PM=50。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x48c0 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x48c0，批内 CGPU-F0724 独占）。
pub const SEG_BASE: u16 = 0x48c0;

/// 权重守恒失败（Σ ≠ 1000‰）。
pub const E_SUM: u16 = SEG_BASE + 1;
/// 权重越出类区间。
pub const E_RANGE: u16 = SEG_BASE + 2;
/// 画像版本非法（0 或回退）。
pub const E_VERSION: u16 = SEG_BASE + 3;
/// 类别数不符（必须 8）。
pub const E_CLASSES: u16 = SEG_BASE + 4;
/// 漂移参数非法。
pub const E_DRIFT: u16 = SEG_BASE + 5;

/// 权重总和目标（千分比）。
pub const SUM_TARGET: u32 = 1000;
/// 漂移阈值（千分比，单类最大绝对变化）。
pub const DRIFT_PM: u32 = 50;

/// 八类合规区间（千分比，规格默认档，序 = 类序）：
/// Dom[100..400] Style[50..300] Layout[50..300] Paint[50..250]
/// Script[50..400] Net[20..200] Decode[10..150] Gpu[20..300]。
pub const RANGES: [(u32, u32); 8] = [
    (100, 400),
    (50, 300),
    (50, 300),
    (50, 250),
    (50, 400),
    (20, 200),
    (10, 150),
    (20, 300),
];

/// 负载类别（闭集八类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadCat {
    /// DOM 操作。
    Dom,
    /// 样式计算。
    Style,
    /// 布局。
    Layout,
    /// 绘制。
    Paint,
    /// 脚本。
    Script,
    /// 网络。
    Net,
    /// 解码。
    Decode,
    /// 合成 GPU。
    Gpu,
}

impl LoadCat {
    /// 类序。
    pub fn ord(self) -> usize {
        match self {
            LoadCat::Dom => 0,
            LoadCat::Style => 1,
            LoadCat::Layout => 2,
            LoadCat::Paint => 3,
            LoadCat::Script => 4,
            LoadCat::Net => 5,
            LoadCat::Decode => 6,
            LoadCat::Gpu => 7,
        }
    }
    /// 类名。
    pub fn name(self) -> &'static str {
        match self {
            LoadCat::Dom => "dom",
            LoadCat::Style => "style",
            LoadCat::Layout => "layout",
            LoadCat::Paint => "paint",
            LoadCat::Script => "script",
            LoadCat::Net => "net",
            LoadCat::Decode => "decode",
            LoadCat::Gpu => "gpu",
        }
    }
}

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 负载画像：八类千分比权重 + 版本号。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadProfile {
    /// 版本号（1 起，单调）。
    pub ver: u32,
    /// 权重（按类序 0..8）。
    pub weights: [u32; 8],
}

impl LoadProfile {
    /// 裸构造（不校验，用 validate 走门）。
    pub fn raw(ver: u32, weights: [u32; 8]) -> Self {
        Self { ver, weights }
    }
    /// 权重和。
    pub fn sum(&self) -> u32 {
        self.weights.iter().sum()
    }
    /// 结构校验：8 类天然定长；版本 ≥1；守恒 Σ=1000；区间合规。
    pub fn validate(&self) -> Result<(), u16> {
        if self.ver == 0 {
            return Err(E_VERSION);
        }
        if self.sum() != SUM_TARGET {
            return Err(E_SUM);
        }
        for (i, w) in self.weights.iter().enumerate() {
            let (lo, hi) = RANGES[i];
            if *w < lo || *w > hi {
                return Err(E_RANGE);
            }
        }
        Ok(())
    }
    /// 画像指纹：类序折叠（类序 byte + 权重 LE）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes = Vec::with_capacity(8 * 5);
        for (i, w) in self.weights.iter().enumerate() {
            bytes.push(i as u8);
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        fnv1a64(&bytes)
    }
    /// 单类读权（按枚举）。
    pub fn weight_of(&self, c: LoadCat) -> u32 {
        self.weights[c.ord()]
    }
}

/// 漂移三态（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drift {
    /// 无漂移（逐类相同）。
    NoDrift,
    /// 轻微（最大单类变化 ≤ DRIFT_PM）。
    Minor,
    /// 显著（超出阈值）。
    Major,
}

/// 漂移检测：返回（三态，最大单类绝对变化）。
pub fn drift(old: &LoadProfile, new: &LoadProfile) -> Result<(Drift, u32), u16> {
    if old.ver >= new.ver {
        return Err(E_VERSION);
    }
    let mut maxd = 0u32;
    for i in 0..8 {
        let d = if old.weights[i] > new.weights[i] {
            old.weights[i] - new.weights[i]
        } else {
            new.weights[i] - old.weights[i]
        };
        if d > maxd {
            maxd = d;
        }
    }
    let st = if maxd == 0 {
        Drift::NoDrift
    } else if maxd <= DRIFT_PM {
        Drift::Minor
    } else {
        Drift::Major
    };
    Ok((st, maxd))
}

/// 漂移报告行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriftRow {
    /// 类序。
    pub ord: u8,
    /// 类名。
    pub cat: &'static str,
    /// 旧权。
    pub old_w: u32,
    /// 新权。
    pub new_w: u32,
    /// 有向变化（新-旧）。
    pub delta: i32,
}

/// 逐类漂移明细（按类序）。
pub fn drift_detail(old: &LoadProfile, new: &LoadProfile) -> Result<Vec<DriftRow>, u16> {
    if old.ver >= new.ver {
        return Err(E_VERSION);
    }
    let mut out = Vec::with_capacity(8);
    let cats = [
        LoadCat::Dom,
        LoadCat::Style,
        LoadCat::Layout,
        LoadCat::Paint,
        LoadCat::Script,
        LoadCat::Net,
        LoadCat::Decode,
        LoadCat::Gpu,
    ];
    for c in cats {
        let o = old.weight_of(c);
        let n = new.weight_of(c);
        out.push(DriftRow {
            ord: c.ord() as u8,
            cat: c.name(),
            old_w: o,
            new_w: n,
            delta: n as i32 - o as i32,
        });
    }
    Ok(out)
}

/// 画像档案：版本链（登记即校验，版本单调）。
#[derive(Clone, Debug, Default)]
pub struct ProfileStore {
    chain: Vec<LoadProfile>,
}

impl ProfileStore {
    pub fn new() -> Self {
        Self { chain: Vec::new() }
    }
    /// 追加画像：validate + 版本严格递增 + 与前一版 drift ≤ Major 也允许登记
    /// （档案只记账，不阻断；阻断由调用方按 drift 判定）。
    pub fn append(&mut self, p: LoadProfile) -> Result<(), u16> {
        p.validate()?;
        if let Some(last) = self.chain.last() {
            if p.ver <= last.ver {
                return Err(E_VERSION);
            }
        }
        self.chain.push(p);
        Ok(())
    }
    /// 最新版。
    pub fn latest(&self) -> Option<&LoadProfile> {
        self.chain.last()
    }
    /// 链长。
    pub fn len(&self) -> usize {
        self.chain.len()
    }
    /// 全链指纹（版本+画像指纹折叠）。
    pub fn chain_fp(&self) -> u64 {
        let mut h: u64 = 0x9e37_79b9_7f4a_7c15;
        for p in &self.chain {
            h ^= p.ver as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            h ^= p.fingerprint();
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }
}

/// 自例程：主路径（合法画像→校验→档案→漂移）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 主路径：标准新闻页画像（Σ=1000，区间内）。
    let p1 = LoadProfile::raw(1, [300, 120, 150, 100, 200, 60, 40, 30]);
    if p1.sum() != 1000 {
        return Err(E_SUM);
    }
    p1.validate()?;
    let f1 = p1.fingerprint();
    // 同内容同指纹。
    let p1b = LoadProfile::raw(1, [300, 120, 150, 100, 200, 60, 40, 30]);
    if p1b.fingerprint() != f1 {
        return Err(E_SUM);
    }
    // 主路径：v2 微调（Script +30 → 230？Script 上限 400，200+30=230 ok；
    // Paint -30 → 70；总和不变 1000）。
    let p2 = LoadProfile::raw(2, [300, 120, 150, 70, 230, 60, 40, 30]);
    p2.validate()?;
    let (d, maxd) = drift(&p1, &p2)?;
    if d != Drift::Minor || maxd != 30 {
        return Err(E_DRIFT);
    }
    // 档案：v1 → v2。
    let mut store = ProfileStore::new();
    store.append(p1.clone())?;
    store.append(p2.clone())?;
    if store.len() != 2 {
        return Err(E_CLASSES);
    }
    let fp_chain = store.chain_fp();
    if fp_chain == 0 {
        return Err(E_SUM);
    }
    // 错误路径 0：守恒失败（Σ=999）。
    let bad_sum = LoadProfile::raw(3, [300, 120, 150, 100, 200, 60, 40, 29]);
    if bad_sum.validate() != Err(E_SUM) {
        return Err(E_SUM);
    }
    // 错误路径 1：区间越界（Dom 90 < 100）。
    let bad_range = LoadProfile::raw(3, [90, 150, 150, 100, 200, 60, 150, 100]);
    if bad_range.validate() != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    // 错误路径 2：版本 0。
    let bad_ver = LoadProfile::raw(0, [300, 120, 150, 100, 200, 60, 40, 30]);
    if bad_ver.validate() != Err(E_VERSION) {
        return Err(E_VERSION);
    }
    // 错误路径 3：版本回退追加。
    let mut store2 = ProfileStore::new();
    store2.append(LoadProfile::raw(5, [300, 120, 150, 100, 200, 60, 40, 30]))?;
    if store2.append(LoadProfile::raw(4, [300, 120, 150, 100, 200, 60, 40, 30])) != Err(E_VERSION) {
        return Err(E_VERSION);
    }
    // 错误路径 4：漂移方向（旧版 ≥ 新版）拒绝。
    if drift(&p2, &p1) != Err(E_VERSION) {
        return Err(E_VERSION);
    }
    // 主路径：显著漂移（Script 200→400 上限，Paint 100→50 下限，
    // 布局 150→0？越界不行——保持区间内最大变化：Dom 300→100 (-200)）。
    let p3 = LoadProfile::raw(3, [100, 120, 150, 100, 400, 60, 40, 30]);
    p3.validate()?;
    let (d3, maxd3) = drift(&p1, &p3)?;
    if d3 != Drift::Major || maxd3 != 200 {
        return Err(E_DRIFT);
    }
    // 漂移明细：8 行，首行 Dom delta=-200。
    let rows = drift_detail(&p1, &p3)?;
    if rows.len() != 8 || rows[0].delta != -200 || rows[0].cat != "dom" {
        return Err(E_DRIFT);
    }
    // 错误路径 5：单类读权。
    if p1.weight_of(LoadCat::Script) != 200 {
        return Err(E_CLASSES);
    }
    Ok(0)
}
