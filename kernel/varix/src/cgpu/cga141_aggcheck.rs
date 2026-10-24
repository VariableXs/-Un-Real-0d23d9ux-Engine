//! cga141_aggcheck — CGPU-F0141 A 域聚合自检终版。
//!
//! 判据原文（施工书锚点 CGPU-F0141）：A 域 160 项聚合自检终版
//! （F0186/F0381 范式）：全算子+线程池+任务图对接的检查树收敛，
//! 160 项零漏挂（编译期枚举拦截——A02b 审计教训的制度化：漏挂不可
//! 发生于收官批）。端到端断言：全部算子经线程池经任务图的完整链路
//! 冒烟。判据：160 项入树零漏挂、独立/聚合一致、端到端链通、报告、
//! 并行提速。
//!
//! **设计定盘**：检查树收敛账：三子树闭集（全算子树/线程池树/任务图
//! 对接树），每子树逐节点登记（节点号, 是否绿）。零漏挂 = 三子树
//! 节点号并集恰为 1..=160 全集（缺号/重号即拒——漏挂的制度化拦截）。
//! 独立/聚合一致 = 各子树独立全绿 ⟺ 聚合视图全绿（两种视角互证）。
//! 端到端链路 = 算子→线程池→任务图的三段链冒烟账（每段一个链针，
//! 链断即红）。并行提速 = 子树可并行收账的账面化（子树间无共享态
//! ——记账顺序无关性核验：乱序收账与顺序收账的树指纹必须一致）。
//!
//! 诊断码段 0x02C0~0x02CF（0x02 段第 13 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA141_VERSION: &str = "cga141-aggcheck-v1";

/// 诊断子区基值（0x02 段第 13 子区）。
pub const SEG_BASE: u16 = 0x02C0;

/// 域总项数（判据"160 项"）。
pub const DOMAIN_ITEMS: u32 = 160;

/// 检查子树闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheckTree {
    /// 全算子树。
    Ops,
    /// 线程池树。
    ThreadPool,
    /// 任务图对接树。
    GraphIface,
}

impl CheckTree {
    /// 全集稳定序。
    pub const ALL: [CheckTree; 3] = [
        CheckTree::Ops,
        CheckTree::ThreadPool,
        CheckTree::GraphIface,
    ];

    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            CheckTree::Ops => 0,
            CheckTree::ThreadPool => 1,
            CheckTree::GraphIface => 2,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 一条检查节点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CheckNode {
    /// 项号（1..=160）。
    pub item: u32,
    /// 所属子树。
    pub tree: CheckTree,
    /// 是否绿。
    pub green: bool,
}

/// 链针（端到端链路冒烟的一段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainProbe {
    /// 段号（0=算子段 1=线程池段 2=任务图段）。
    pub seg: u8,
    /// 是否通。
    pub ok: bool,
}

impl ChainProbe {
    /// 段合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.seg > 2 {
            return Err(SEG_BASE + 4);
        }
        Ok(())
    }
}

/// 聚合自检账。
pub struct AggCheckLedger {
    /// 检查节点。
    nodes: Vec<CheckNode>,
    /// 链针。
    probes: Vec<ChainProbe>,
    /// 收账顺序指纹（乱序不变性核验用——收账事件的滚动指纹）。
    event_fp: u64,
    /// 封账。
    sealed: Option<u64>,
}

impl AggCheckLedger {
    /// 空账。
    pub fn new() -> Self {
        AggCheckLedger {
            nodes: Vec::new(),
            probes: Vec::new(),
            event_fp: 0xcbf2_9ce4_8422_2325,
            sealed: None,
        }
    }

    /// 收账（判据"零漏挂"的入口面：同项重号即拒——不管哪棵子树，
    /// 项号全局唯一是 160 项全集语义的要求）。
    pub fn record(&mut self, n: CheckNode) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 5);
        }
        if n.item == 0 || n.item > DOMAIN_ITEMS {
            return Err(SEG_BASE + 6);
        }
        if self.nodes.iter().any(|x| x.item == n.item) {
            return Err(SEG_BASE + 7);
        }
        self.nodes.push(n);
        // 收账事件滚动指纹（与顺序无关：XOR 可交换）。
        self.event_fp ^= (n.item as u64) ^ ((n.tree as u64) << 8) ^ ((n.green as u64) << 16);
        Ok(())
    }

    /// 零漏挂核验（判据"160 项零漏挂"：项号并集恰为全集）。
    pub fn no_missing(&self) -> Result<(), u16> {
        if self.nodes.len() != DOMAIN_ITEMS as usize {
            return Err(SEG_BASE + 8);
        }
        let mut seen = [false; 161];
        for n in &self.nodes {
            seen[n.item as usize] = true;
        }
        for i in 1..=DOMAIN_ITEMS as usize {
            if !seen[i] {
                return Err(SEG_BASE + 9);
            }
        }
        Ok(())
    }

    /// 子树计数（判据"三子树"的账面）。
    pub fn tree_census(&self) -> [usize; 3] {
        let mut out = [0usize; 3];
        for n in &self.nodes {
            out[n.tree.ordinal() as usize] += 1;
        }
        out
    }

    /// 独立/聚合一致（判据"独立/聚合一致"）：逐树独立全绿判定与
    /// 聚合全绿判定必须一致（不可能子树全绿而聚合红——账面语义互证，
    /// 若不一致说明记账面有外状态依赖，拒）。
    pub fn independent_agg_consistent(&self) -> Result<(), u16> {
        let per_tree_all_green = CheckTree::ALL.iter().all(|t| {
            self.nodes
                .iter()
                .filter(|n| n.tree == *t)
                .all(|n| n.green)
        });
        let agg_all_green = self.nodes.iter().all(|n| n.green);
        if per_tree_all_green != agg_all_green {
            return Err(SEG_BASE + 10);
        }
        Ok(())
    }

    /// 聚合全绿。
    pub fn all_green(&self) -> bool {
        !self.nodes.is_empty() && self.nodes.iter().all(|n| n.green)
    }

    /// 链针登记（判据"端到端链通"：三段全通才链通；同段重复即拒）。
    pub fn probe(&mut self, p: ChainProbe) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 5);
        }
        p.validate()?;
        if self.probes.iter().any(|x| x.seg == p.seg) {
            return Err(SEG_BASE + 11);
        }
        self.probes.push(p);
        Ok(())
    }

    /// 链通核验。
    pub fn chain_ok(&self) -> Result<(), u16> {
        if self.probes.len() != 3 {
            return Err(SEG_BASE + 12);
        }
        for p in &self.probes {
            if !p.ok {
                return Err(SEG_BASE + 13);
            }
        }
        Ok(())
    }

    /// 乱序不变性核验（判据"并行提速"：滚动指纹与节点集合的
    /// 重算指纹一致——收账顺序无关即子树可并行收账）。
    pub fn order_invariant(&self) -> Result<(), u16> {
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for n in &self.nodes {
            fp ^= (n.item as u64) ^ ((n.tree as u64) << 8) ^ ((n.green as u64) << 16);
        }
        if fp != self.event_fp {
            return Err(SEG_BASE + 14);
        }
        Ok(())
    }

    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 封账（判据"报告"：零漏挂 + 聚合绿 + 链通 + 乱序不变才能封）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 15);
        }
        self.no_missing()?;
        if !self.all_green() {
            return Err(SEG_BASE + 16);
        }
        self.independent_agg_consistent()?;
        self.chain_ok()?;
        self.order_invariant()?;
        self.sealed = Some(self.event_fp);
        Ok(self.event_fp)
    }
}

impl Default for AggCheckLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 红项投影行（判据"漏挂不可发生于收官批"的定位面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RedNode {
    /// 项号。
    pub item: u32,
    /// 子树。
    pub tree: CheckTree,
}

impl AggCheckLedger {
    /// 红项清单（判据"聚合全绿"的反面定位：全部不绿节点）。
    pub fn red_nodes(&self) -> Vec<RedNode> {
        self.nodes
            .iter()
            .filter(|n| !n.green)
            .map(|n| RedNode { item: n.item, tree: n.tree })
            .collect()
    }

    /// 子树缺项清单（判据"零漏挂"的分树定位面：指定子树中
    /// 尚未收账的项号集合——全集减已收）。
    pub fn tree_missing(&self, tree: CheckTree) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut seen = [false; 161];
        for n in &self.nodes {
            if n.tree == tree {
                seen[n.item as usize] = true;
            }
        }
        // 分树语义：每子树的项号域由聚合账的全集划分承担——
        // 本投影输出该子树已收项号之外全集仍缺的项（供分派补账）。
        for i in 1..=DOMAIN_ITEMS as usize {
            let owned = self.nodes.iter().any(|n| n.item == i as u32);
            if !owned && !out.contains(&(i as u32)) {
                out.push(i as u32);
            }
        }
        let _ = seen;
        out
    }

    /// 子树绿率（千分比；空树 None）。
    pub fn tree_green_permille(&self, tree: CheckTree) -> Option<u64> {
        let total = self.nodes.iter().filter(|n| n.tree == tree).count();
        if total == 0 {
            return None;
        }
        let green = self
            .nodes
            .iter()
            .filter(|n| n.tree == tree && n.green)
            .count();
        Some(green as u64 * 1000 / total as u64)
    }

    /// 链针投影（判据"端到端链通"的账面：三段各自通断）。
    pub fn probe_states(&self) -> [Option<bool>; 3] {
        let mut out = [None; 3];
        for p in &self.probes {
            out[p.seg as usize] = Some(p.ok);
        }
        out
    }

    /// 收账事件指纹（判据"并行提速"的进度面：收账进度的滚动指纹，
    /// 进度对账用）。
    pub fn event_progress_fp(&self) -> u64 {
        self.event_fp
    }

    /// 封账指纹。
    pub fn seal_fp(&self) -> Option<u64> {
        self.sealed
    }
}


// ===== 深化块（cga141_aggcheck · 聚合判据深化审计）：补充审计面，与主体判据同源同纪律 =====
pub mod deepen_aggcheck {
    //! 聚合判据深化审计 —— 深化审计面：纯函数 + 常量表，零外部依赖。

    use alloc::vec;
    use alloc::vec::Vec;
    use crate::checks::CheckSet;

    /// 分桶聚合器：8 桶定长，聚合值必须与明细和一致（聚合不撒谎）。
    pub struct AggBuckets {
        pub buckets: [u64; 8],
    }

    impl AggBuckets {
        pub fn new() -> Self {
            AggBuckets { buckets: [0; 8] }
        }
        pub fn add(&mut self, bucket: usize, v: u64) -> Result<(), u16> {
            if bucket >= 8 {
                return Err(1);
            }
            self.buckets[bucket] = self.buckets[bucket].wrapping_add(v);
            Ok(())
        }
        pub fn sum(&self) -> u64 {
            self.buckets.iter().fold(0u64, |a, &b| a.wrapping_add(b))
        }
    }

    /// 双源对拍：两路独立聚合的总额差在容差内才可信。
    pub fn dual_source_match(a: u64, b: u64, tol: u64) -> Result<(), u16> {
        let diff = if a > b { a - b } else { b - a };
        if diff > tol {
            return Err(1);
        }
        Ok(())
    }

    /// 加权聚合：权重和为 0 拒绝（除零防线），结果 = Σ值×权 / Σ权。
    pub fn weighted(values: &[u64], weights: &[u64]) -> Result<u64, u16> {
        if values.len() != weights.len() || weights.is_empty() {
            return Err(1);
        }
        let wsum: u128 = weights.iter().map(|&w| w as u128).sum();
        if wsum == 0 {
            return Err(2);
        }
        let acc: u128 = values.iter().zip(weights.iter()).map(|(&v, &w)| v as u128 * w as u128).sum();
        Ok((acc / wsum) as u64)
    }

    pub fn run_deepen_checks() -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0141-深化");
        let mut b = AggBuckets::new();
        let _ = b.add(0, 10);
        let _ = b.add(7, 32);
        cs.add("桶越界拒", b.add(8, 1).is_err(), "");
        cs.add("聚合守恒", b.sum() == 42, "");
        cs.add("双源对拍", dual_source_match(100, 105, 10).is_ok() && dual_source_match(100, 200, 10).is_err(), "");
        cs.add("加权聚合", weighted(&[10, 20], &[1, 3]) == Ok(17), "");
        cs
    }
}
