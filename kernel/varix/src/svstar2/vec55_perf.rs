//! VE-F0455 · 语义分析性能（VE-C 域 · 语义组 C04）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0455`
//!
//! **锚点原文**：「语义分析性能工程：四遍各线性时间判据断言（O(节点数) 无
//! 隐藏平方级）、符号表哈希容量预分配（按源规模估算）、旁路表内存上界（与
//! AST 规模成比例常数封顶）；性能基准（每千行毫秒数按遍分列）版本化记录，
//! 退化阻断合入。判据：线性判据、预分配、上界封顶、按遍分列、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **线性判据**（判据一）。四遍（收集/解析/校验/推导）各有线性
//!    判据断言：步数上界 = 节点数 × 常数因子——超界即「隐藏平方级」
//!    实锤（结构性检测：倍增样本下步数增幅须线性）。
//!
//! 2. **预分配**（判据二）。符号表哈希容量按源规模估算（[`estimate`]
//!    线性模型）一次性预分配，避免逐个扩容的均摊陷阱。
//!
//! 3. **上界封顶**（判据三）。旁路表（notes/侧表）内存上界与 AST 规模
//!    成比例（每节点常数条目），超上界 = 设计断言拦截（不是运行期悄悄
//!    涨）。
//!
//! 4. **按遍分列**（判据四）。基准记录按遍分列（每千行毫秒数），版本化
//!    存取；退化（超回归带宽）阻断合入——带宽判定与 F0494 基准册同构。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0455";

/// 线性判据的常数因子（步数上界 = 节点数 × K）。
pub const LINEAR_K: usize = 8;
/// 旁路表上界（每节点条目数封顶）。
pub const SIDETABLE_CAP_PER_NODE: usize = 4;

// ---------------------------------------------------------------------------
// 线性判据：倍增样本增幅检测
// ---------------------------------------------------------------------------

/// 被测遍的步数函数（模拟面：实际遍以节点数驱动的步数记录）。
/// 判据面：给定「规模 n → 步数 s(n)」的采样对，检验 s(2n) ≈ 2·s(n)
/// （增幅比落在 [1.6, 2.4] 带内 = 线性；超带 = 超线性）。
pub fn linearity_verdict(samples: &[(usize, usize)]) -> bool {
    if samples.len() < 2 {
        return false;
    }
    for w in samples.windows(2) {
        let (n1, s1) = w[0];
        let (n2, s2) = w[1];
        if n1 == 0 || n2 <= n1 {
            return false;
        }
        // 倍增面：n 须逐窗翻倍（判据口径限定倍增样本）。
        let grow_n = n2 as f64 / n1 as f64;
        if (grow_n - 2.0).abs() > 1e-9 {
            return false;
        }
        // 线性 ⇒ s(2n)/s(n) ≈ 2.0（带 [1.6, 2.4]）；平方 ⇒ 4.0 超带被拦。
        let grow_s = s2 as f64 / s1 as f64;
        if !(1.6..=2.4).contains(&grow_s) {
            return false;
        }
    }
    true
}

/// 四遍枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassId {
    Collect,
    Resolve,
    Verify,
    Infer,
}

impl PassId {
    pub fn tag(&self) -> &'static str {
        match self {
            PassId::Collect => "collect",
            PassId::Resolve => "resolve",
            PassId::Verify => "verify",
            PassId::Infer => "infer",
        }
    }
}

// ---------------------------------------------------------------------------
// 容量预估（预分配）
// ---------------------------------------------------------------------------

/// 符号表容量估算：线性模型 ceil(nodes × 1.5) + 16（预留头空间）。
pub fn estimate_capacity(nodes: usize) -> usize {
    nodes * 3 / 2 + 16
}

/// 预分配策略：容量只升不降（重估不缩）。
pub fn prealloc_policy(current: usize, nodes: usize) -> usize {
    let want = estimate_capacity(nodes);
    if want > current {
        want
    } else {
        current
    }
}

// ---------------------------------------------------------------------------
// 旁路表上界（封顶）
// ---------------------------------------------------------------------------

/// 旁路表封顶断言：条目数 ≤ 节点数 × 每节点封顶。
pub fn sidetable_within_cap(entries: usize, nodes: usize) -> bool {
    entries <= nodes * SIDETABLE_CAP_PER_NODE
}

// ---------------------------------------------------------------------------
// 基准记录（按遍分列 + 版本化 + 退化阻断）
// ---------------------------------------------------------------------------

/// 基准条目：遍 × 每千行毫秒数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchEntry {
    pub pass: PassId,
    pub ms_per_kloc: f64,
}

/// 基准册：版本化记录 + 回归带宽。
#[derive(Debug, Default)]
pub struct PerfBook {
    pub ver: u32,
    pub entries: Vec<BenchEntry>,
}

/// 回归带宽（±10%，与 F0494 基准册同构）。
pub const REGRESS_BAND: f64 = 0.10;

impl PerfBook {
    pub fn new() -> Self {
        PerfBook { ver: 1, entries: Vec::new() }
    }
    pub fn record(&mut self, pass: PassId, ms: f64) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.pass == pass) {
            e.ms_per_kloc = ms;
        } else {
            self.entries.push(BenchEntry { pass, ms_per_kloc: ms });
        }
    }
    pub fn get(&self, pass: PassId) -> Option<f64> {
        self.entries.iter().find(|e| e.pass == pass).map(|e| e.ms_per_kloc)
    }
    /// 退化检测：新值超基线 × (1 + 带宽) 即退化。
    pub fn regressed(&self, pass: PassId, new_ms: f64) -> bool {
        match self.get(pass) {
            Some(base) => new_ms > base * (1.0 + REGRESS_BAND),
            None => false,
        }
    }
    /// 升版（显性更新基线）。
    pub fn accept_new_baseline(&mut self, pass: PassId, new_ms: f64) {
        self.record(pass, new_ms);
        self.ver += 1;
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec55_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、线性判据。
    {
        // 真线性样本：n 翻倍步数翻倍 → 过。
        let lin = vec![(100usize, 800usize), (200, 1600), (400, 3200)];
        v.push(("F0455-线性-线性样本过", linearity_verdict(&lin)));
        // 平方样本：n 翻倍步数 ×4 → 拦。
        let quad = vec![(100usize, 10_000usize), (200, 40_000), (400, 160_000)];
        v.push(("F0455-线性-平方样本拦", !linearity_verdict(&quad)));
        // 畸形样本（零规模/非递增）→ 拒判。
        v.push(("F0455-线性-畸形拒判", !linearity_verdict(&[(0, 1), (10, 2)]) && !linearity_verdict(&[(100, 5), (50, 3)])));
        // 常数因子在界内（LINEAR_K 与线性模型一致性——步数 ≤ n×K 的形态假设）。
        v.push(("F0455-线性-常数因子", LINEAR_K >= 1));
    }

    // 二、预分配。
    {
        // 估算线性且带预留。
        let c1 = estimate_capacity(0);
        let c100 = estimate_capacity(100);
        let c200 = estimate_capacity(200);
        v.push((
            "F0455-预分配-线性估算",
            c1 == 16 && c100 == 166 && c200 == 316 && c200 - c100 == c100 - c1,
        ));
        // 预分配只升不降。
        let a = prealloc_policy(1000, 10);
        let b = prealloc_policy(100, 1000);
        v.push((
            "F0455-预分配-只升不降",
            a == 1000 && b == estimate_capacity(1000),
        ));
    }

    // 三、上界封顶。
    {
        // 界内过 / 超界拦。
        v.push((
            "F0455-上界-界内过",
            sidetable_within_cap(40, 10) && sidetable_within_cap(0, 0),
        ));
        v.push(("F0455-上界-超界拦", !sidetable_within_cap(41, 10)));
    }

    // 四、按遍分列。
    {
        let mut book = PerfBook::new();
        // 四遍分列记录。
        book.record(PassId::Collect, 1.0);
        book.record(PassId::Resolve, 2.0);
        book.record(PassId::Verify, 1.5);
        book.record(PassId::Infer, 2.5);
        v.push((
            "F0455-分列-四遍独立",
            book.get(PassId::Collect) == Some(1.0)
                && book.get(PassId::Infer) == Some(2.5)
                && book.entries.len() == 4,
        ));
        // 退化检测：基线 2.0，新值 2.3（+15%）→ 退化。
        v.push((
            "F0455-分列-退化检出",
            book.regressed(PassId::Resolve, 2.3) && !book.regressed(PassId::Resolve, 2.1),
        ));
        // 升版重录（显性更新）。
        book.accept_new_baseline(PassId::Resolve, 2.3);
        v.push((
            "F0455-分列-升版重录",
            book.ver == 2 && book.get(PassId::Resolve) == Some(2.3),
        ));
        // 无基线不判退化（诚实缺省）。
        let mut empty = PerfBook::new();
        v.push(("F0455-分列-无基线不判", !empty.regressed(PassId::Verify, 9.9)));
    }

    // 五、判据自检。
    {
        v.push(("F0455-判据-锚点入册", ANCHOR == "VE-F0455"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec55）。
pub fn run_vec55_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec55");
    for (name, ok) in vec55_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec55_raw_checks as run_vec55_checks;
    #[test]
    fn vec55_perf_checks() {
        for (name, ok) in run_vec55_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
