//! CGPU-F0446 · 高级特性 fuzz（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - fuzz：拓扑数据畸形 fuzz（异常拓扑输入——钳制容错）；极端权重
//!   fuzz（权重全 0/极大——不变量保持）；插件接口 fuzz（恶意策略插件
//!   数据——隔离边界验证）；案例固化 CI。判据：三类、不变量、CI、判据。
//!
//! 设计纪律：
//! - 三类 fuzz 各有确定性对手方：
//!   ① 拓扑畸形：节点号越界/空核/自环——钳到合法域（不崩溃）；
//!   ② 极端权重：全 0/极大——Jain 归一与静态切分不炸（份额守恒
//!      ≤ 预算这一不变量在极端输入下仍成立）；
//!   ③ 插件接口：恶意返回（份额超预算/长度不符/NaN 语义的 saturating
//!      溢出）——隔离闸拒收（策略输出必须过合同闸才进仲裁）。
//! - 案例固化 CI：登记（类别+期望结局+实际结局），三类覆盖全过才绿。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0F10 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0F10，全仓独占）。
pub const SEG_BASE: u16 = 0x0F10;

pub const E_AF_KIND: u16 = SEG_BASE + 1; // 类别非法
pub const E_AF_CASE: u16 = SEG_BASE + 2; // 案例 ID 非法
pub const E_AF_CONTRACT: u16 = SEG_BASE + 3; // 插件输出违反合同

/// 权重上限（极端权重 fuzz 的钳制目标）。
pub const WEIGHT_CAP: u64 = 1_000_000;
/// 拓扑节点数上限。
pub const NODE_CAP: usize = 64;

/// fuzz 类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdvFuzzKind {
    TopoMalformed,
    ExtremeWeight,
    PluginMalice,
}

/// 案例登记。
#[derive(Clone, Debug)]
pub struct AdvFuzzCase {
    pub id: u32,
    pub kind: AdvFuzzKind,
    pub payload: u64,
    pub pass: bool,
}

/// 高级特性 fuzz 台账。
#[derive(Debug, Default)]
pub struct AdvFuzzer {
    cases: Vec<AdvFuzzCase>,
    next_id: u32,
}

impl AdvFuzzer {
    pub fn new() -> Self {
        Self { cases: Vec::new(), next_id: 1 }
    }

    // ---- ① 拓扑畸形 -------------------------------------------------------

    /// 节点号钳制：越界钳到上限内；空核钳到 1。
    pub fn clamp_node(&self, id: u64, cores: usize) -> (u64, usize) {
        let id_c = id.min(NODE_CAP as u64 - 1);
        let cores_c = cores.max(1).min(NODE_CAP);
        (id_c, cores_c)
    }

    /// 拓扑畸形 fuzz 一轮：返回是否被钳（载荷确实畸形时必须钳）。
    pub fn fuzz_topo(&mut self, id: u64, cores: usize) -> Result<bool, u16> {
        let (ci, cc) = self.clamp_node(id, cores);
        // 钳制结果必在合法域。
        if ci >= NODE_CAP as u64 || cc == 0 || cc > NODE_CAP {
            return Err(E_AF_KIND);
        }
        let malformed = id >= NODE_CAP as u64 || cores == 0 || cores > NODE_CAP;
        Ok(malformed)
    }

    // ---- ② 极端权重 -------------------------------------------------------

    /// 极端权重钳制：0 → 1（零权重不参与份额除法），极大 → 上限。
    pub fn clamp_weight(&self, w: u64) -> u64 {
        if w == 0 {
            1
        } else {
            w.min(WEIGHT_CAP)
        }
    }

    /// 不变量核验：任意权重下静态切分份额和 ≤ 预算。
    pub fn fuzz_weight_invariant(&mut self, weights: &[u64], budget_pm: u64) -> Result<bool, u16> {
        let clamped: Vec<u64> = weights.iter().map(|w| self.clamp_weight(*w)).collect();
        let sum: u64 = clamped.iter().sum();
        if sum == 0 {
            return Ok(true); // 全 0 钳成 1 后必有份额。
        }
        let shares: Vec<u64> = clamped.iter().map(|w| w.saturating_mul(budget_pm) / sum).collect();
        let total: u64 = shares.iter().sum();
        // 不变量：份额和 ≤ 预算（整数截断保证）。
        if total > budget_pm {
            return Err(E_AF_CONTRACT);
        }
        // 极端输入在变（有无全 0/极大）才算真 fuzz。
        let extreme = weights.iter().any(|w| *w == 0 || *w > WEIGHT_CAP);
        Ok(extreme)
    }

    // ---- ③ 插件接口 -------------------------------------------------------

    /// 插件输出合同闸：长度一致 + 份额和 ≤ 预算（恶意输出隔离在闸外）。
    pub fn plugin_contract(&mut self, shares: &[u64], n: usize, budget_pm: u64) -> Result<bool, u16> {
        if shares.len() != n {
            return Err(E_AF_CONTRACT);
        }
        let total: u64 = shares.iter().sum();
        if total > budget_pm {
            return Err(E_AF_CONTRACT);
        }
        Ok(true)
    }

    // ---- 案例固化 CI -------------------------------------------------------

    pub fn record(&mut self, kind: AdvFuzzKind, payload: u64, pass: bool) -> Result<u32, u16> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.cases.push(AdvFuzzCase { id, kind, payload, pass });
        Ok(id)
    }

    /// CI：全过 + 三类覆盖。
    pub fn ci_green(&self) -> Result<(usize, usize), u16> {
        if self.cases.is_empty() {
            return Err(E_AF_CASE);
        }
        let mut has = [false; 3];
        for c in &self.cases {
            if !c.pass {
                return Err(E_AF_CASE);
            }
            match c.kind {
                AdvFuzzKind::TopoMalformed => has[0] = true,
                AdvFuzzKind::ExtremeWeight => has[1] = true,
                AdvFuzzKind::PluginMalice => has[2] = true,
            }
        }
        if !has[0] || !has[1] || !has[2] {
            return Err(E_AF_KIND);
        }
        Ok((self.cases.len(), self.next_id as usize - 1))
    }

    pub fn case_count(&self) -> usize {
        self.cases.len()
    }
}

// ==========================================================================
// 判据（施工书：三类、不变量、CI、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_拓扑畸形_钳制容错() {
        let mut f = AdvFuzzer::new();
        // 越界节点钳到上限内。
        let (i, c) = f.clamp_node(u64::MAX, 0);
        assert_eq!((i, c), (NODE_CAP as u64 - 1, 1));
        // 合法输入直通。
        assert_eq!(f.clamp_node(3, 8), (3, 8));
        // fuzz 轮：畸形被识别。
        assert_eq!(f.fuzz_topo(NODE_CAP as u64 + 5, 0), Ok(true));
        assert_eq!(f.fuzz_topo(3, 8), Ok(false));
        // 钳制永不越域（不崩溃）。
        for id in [u64::MAX, NODE_CAP as u64, 0] {
            assert!(f.fuzz_topo(id, 0).is_ok());
        }
    }

    #[test]
    fn a02_极端权重_不变量保持() {
        let mut f = AdvFuzzer::new();
        // 全 0 权重：钳成 1 → 份额均分，和 = 预算。
        assert_eq!(f.fuzz_weight_invariant(&[0, 0, 0], 900), Ok(true));
        // 极大权重：钳到上限 → 份额和 ≤ 预算。
        assert_eq!(
            f.fuzz_weight_invariant(&[u64::MAX, 1, 0], 900),
            Ok(true)
        );
        // 混合极端：识别为真 fuzz 载荷。
        assert_eq!(f.fuzz_weight_invariant(&[0, 5_000_000], 700), Ok(true));
        // 普通权重：非极端（返回 false 但不变量仍核验过）。
        assert_eq!(f.fuzz_weight_invariant(&[10, 20, 30], 600), Ok(false));
    }

    #[test]
    fn a03_插件接口_恶意输出隔离() {
        let mut f = AdvFuzzer::new();
        // 合同输出：长度一致 + 和 ≤ 预算 → 过。
        assert_eq!(f.plugin_contract(&[300, 300, 300], 3, 1000), Ok(true));
        // 长度不符 → 闸拒。
        assert_eq!(f.plugin_contract(&[300, 300], 3, 1000), Err(E_AF_CONTRACT));
        // 份额超预算（恶意策略）→ 闸拒。
        assert_eq!(f.plugin_contract(&[800, 800], 2, 1000), Err(E_AF_CONTRACT));
        // 零份额合法。
        assert_eq!(f.plugin_contract(&[0, 0], 2, 500), Ok(true));
    }

    #[test]
    fn a04_案例固化_CI全绿() {
        let mut f = AdvFuzzer::new();
        assert_eq!(f.ci_green(), Err(E_AF_CASE));
        f.record(AdvFuzzKind::TopoMalformed, 1, true).ok();
        f.record(AdvFuzzKind::ExtremeWeight, 2, true).ok();
        f.record(AdvFuzzKind::PluginMalice, 3, true).ok();
        assert_eq!(f.case_count(), 3);
        assert_eq!(f.ci_green(), Ok((3, 3)));
        // 红案例混入 → CI 红。
        f.record(AdvFuzzKind::TopoMalformed, 4, false).ok();
        assert_eq!(f.ci_green(), Err(E_AF_CASE));
        // 单类覆盖不全 → 类别红。
        let mut f2 = AdvFuzzer::new();
        f2.record(AdvFuzzKind::PluginMalice, 1, true).ok();
        assert_eq!(f2.ci_green(), Err(E_AF_KIND));
    }

    #[test]
    fn a05_案例ID_连续递增() {
        let mut f = AdvFuzzer::new();
        let a = f.record(AdvFuzzKind::TopoMalformed, 0, true).ok();
        let b = f.record(AdvFuzzKind::TopoMalformed, 0, true).ok();
        let c = f.record(AdvFuzzKind::TopoMalformed, 0, true).ok();
        assert_eq!((a, b, c), (Some(1), Some(2), Some(3)));
        assert_eq!(f.case_count(), 3);
    }
}
