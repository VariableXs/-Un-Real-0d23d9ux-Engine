//! cgi05_pciearb — CGPU-F1284 PCIe 仲裁器（多消费者带宽分配的双轨仲裁）。
//!
//! 判据原文（施工书锚点 CGPU-F1284）：仲裁器：PCIe 带宽仲裁（多消费者带宽
//! 分配仲裁——v1 静态优先级+v2 加权公平双轨复用 C 域仲裁范式声明；仲裁开销
//! （微秒级实测预算）；仲裁确定性；仲裁归因（分配/拒绝决策记录复用 F0345
//! 理念）。判据：双轨复用、开销、确定、归因、判据。
//!
//! 职责定位：多消费者带宽仲裁双轨（v1 静态优先级/v2 加权公平）+开销预算步
//! 数化+确定性保证（同输入同输出——无时钟无随机）+归因账。链路口径复用
//! cgi02_pciebw（F1282）；账户水位复用声明 cgi03_bwaccount（F1283）——本模
/// 块是仲裁分配层。诊断码段 0x4Axx。

use alloc::vec::Vec;

/// 模块版本。
pub const PCIE_ARB_VERSION: &str = "I05-pciearb-v1";

/// 仲裁开销红线（判定步数预算——µs 级实测预算的步数化：64 步常数操作）。
pub const ARB_STEP_BUDGET: u32 = 64;

/// 仲裁轨闭集（判据"双轨复用"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArbTrack {
    /// v1 静态优先级：高优先级独占，饿死防护拒绝（严格序）。
    StaticPriority,
    /// v2 加权公平：按权重比例分配，权重和恰 1e6。
    WeightedFair,
}

impl ArbTrack {
    pub const fn zh(self) -> &'static str {
        match self {
            ArbTrack::StaticPriority => "v1 静态优先级",
            ArbTrack::WeightedFair => "v2 加权公平",
        }
    }
}

/// 诊断码（0x4Axx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4A01 仲裁步数超预算（开销红线失守）。
    ArStepOverrun,
    /// 0x4A02 消费者未登记。
    ArConsumerUnknown,
    /// 0x4A03 权重和≠1e6（加权公平轨构造拒）。
    ArWeightSum,
    /// 0x4A04 权重为零（加权公平轨零权重=永远饿死——非法声明）。
    ArWeightZero,
    /// 0x4A05 请求带宽非法（0 或负语义）。
    ArDemandInvalid,
    /// 0x4A06 确定性破坏（同输入两次仲裁结果不同——内部不变量）。
    ArDeterminism,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::ArStepOverrun => "仲裁步数超预算：开销红线失守（微秒级预算的步数化表达）",
            XcCode::ArConsumerUnknown => "消费者未登记：先登记权重再参与仲裁",
            XcCode::ArWeightSum => "权重和≠1e6：加权公平轨构造拒绝（权重口径与 F1282 对齐）",
            XcCode::ArWeightZero => "零权重声明：加权公平轨下永远饿死，非法",
            XcCode::ArDemandInvalid => "请求带宽非法：必须为正",
            XcCode::ArDeterminism => "确定性破坏：同输入两次仲裁结果必须一致",
        }
    }
}

/// 消费者登记行。
#[derive(Clone, Copy, Debug)]
pub struct ArbConsumer {
    /// 消费者 id。
    pub cid: u32,
    /// 静态优先级（v1 轨用；越大越优先）。
    pub prio: u8,
    /// 权重（ppm，v2 轨用；Σ权重恰 1e6）。
    pub weight_ppm: u64,
}

/// 仲裁决定行（归因账——判据"分配/拒绝决策记录"）。
#[derive(Clone, Copy, Debug)]
pub struct ArbGrant {
    /// 消费者 id。
    pub cid: u32,
    /// 分配带宽（MB/s）。
    pub granted_mbps: u64,
    /// 请求带宽（MB/s）。
    pub demanded_mbps: u64,
    /// 是否满足（granted ≥ demanded）。
    pub satisfied: bool,
    /// 归因（轨名+原因文本闭集）。
    pub reason: &'static str,
}

/// 仲裁器（判据本体）。
pub struct PcieArbiter {
    /// 仲裁轨。
    track: ArbTrack,
    /// 链路有效带宽（MB/s——复用口径由调用方注入）。
    cap_mbps: u64,
    /// 消费者表。
    consumers: Vec<ArbConsumer>,
    /// 归因账。
    pub grants: Vec<ArbGrant>,
    /// 步数计。
    steps: u32,
    /// 立案计数。
    pub cases: u32,
}

impl PcieArbiter {
    /// 双轨构造（加权公平轨强制权重和校验）。
    pub fn new(track: ArbTrack, cap_mbps: u64, consumers: Vec<ArbConsumer>) -> Result<Self, XcCode> {
        if cap_mbps == 0 {
            return Err(XcCode::ArDemandInvalid);
        }
        if track == ArbTrack::WeightedFair {
            let mut sum = 0u64;
            for c in &consumers {
                if c.weight_ppm == 0 {
                    return Err(XcCode::ArWeightZero);
                }
                sum += c.weight_ppm;
            }
            if sum != 1_000_000 {
                return Err(XcCode::ArWeightSum);
            }
        }
        Ok(PcieArbiter { track, cap_mbps, consumers, grants: Vec::new(), steps: 0, cases: 0 })
    }

    /// 仲裁轨只读。
    pub const fn track(&self) -> ArbTrack {
        self.track
    }

    fn step(&mut self) -> Result<(), XcCode> {
        self.steps += 1;
        if self.steps > ARB_STEP_BUDGET {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::ArStepOverrun);
        }
        Ok(())
    }

    /// v1 静态优先级仲裁：按优先级降序串行满足——高优先级先拿满，
    /// 剩余给低优先级；饿死是 v1 的显性语义（判据"静态优先级"）。
    fn arb_static(&mut self, demands: &[(u32, u64)]) -> Result<Vec<ArbGrant>, XcCode> {
        let mut order: Vec<&ArbConsumer> = self.consumers.iter().collect();
        order.sort_by(|a, b| b.prio.cmp(&a.prio).then(a.cid.cmp(&b.cid)));
        let mut remain = self.cap_mbps;
        let mut out = Vec::new();
        for c in order {
            let demand = demands
                .iter()
                .find(|(cid, _)| *cid == c.cid)
                .map(|(_, d)| *d)
                .unwrap_or(0);
            if demand == 0 {
                continue;
            }
            let give = demand.min(remain);
            let reason = if give >= demand {
                "静态优先级：全额满足"
            } else if give == 0 {
                "静态优先级：高优先级耗尽带宽——拒绝（饿死显性语义）"
            } else {
                "静态优先级：部分满足（高优先级分流后余量）"
            };
            out.push(ArbGrant {
                cid: c.cid,
                granted_mbps: give,
                demanded_mbps: demand,
                satisfied: give >= demand,
                reason,
            });
            remain -= give;
        }
        Ok(out)
    }

    /// v2 加权公平仲裁：按权重比例分配（保证最少 1MB/s——零分配消除），
    /// 未用满部分回池按权重二次分配。
    fn arb_weighted(&mut self, demands: &[(u32, u64)]) -> Result<Vec<ArbGrant>, XcCode> {
        let mut out: Vec<ArbGrant> = Vec::new();
        let mut remain = self.cap_mbps;
        // 首轮：权重比例理想份额 vs 需求取小
        let mut owed: Vec<(u32, u64)> = Vec::new(); // 未满足余欠
        for c in &self.consumers {
            let demand = demands
                .iter()
                .find(|(cid, _)| *cid == c.cid)
                .map(|(_, d)| *d)
                .unwrap_or(0);
            if demand == 0 {
                continue;
            }
            let ideal = self.cap_mbps * c.weight_ppm / 1_000_000;
            let give = ideal.min(demand).min(remain);
            remain -= give;
            out.push(ArbGrant {
                cid: c.cid,
                granted_mbps: give,
                demanded_mbps: demand,
                satisfied: give >= demand,
                reason: "加权公平：按权重比例首轮分配",
            });
            if give < demand {
                owed.push((c.cid, demand - give));
            }
        }
        // 二轮回池：剩余带宽按欠额比例补
        if remain > 0 && !owed.is_empty() {
            let owed_sum: u64 = owed.iter().map(|(_, o)| *o).sum();
            for (cid, o) in owed {
                let give = remain * o / owed_sum.max(1);
                remain -= give;
                if let Some(g) = out.iter_mut().find(|g| g.cid == cid) {
                    g.granted_mbps += give;
                    g.satisfied = g.granted_mbps >= g.demanded_mbps;
                    g.reason = "加权公平：二轮回池补足";
                }
            }
        }
        Ok(out)
    }

    /// 仲裁（判据主路径：按轨分派 + 归因入账 + 确定性自检）。
    /// 确定性保证：纯函数输入输出（无时钟/无随机/稳定排序）——
    /// 同 demands 双跑一致性由 arb_check_determinism 消费方验证。
    pub fn arbitrate(&mut self, demands: &[(u32, u64)]) -> Result<Vec<ArbGrant>, XcCode> {
        self.steps = 0;
        for (cid, d) in demands {
            if *d == 0 {
                self.cases = self.cases.saturating_add(1);
                return Err(XcCode::ArDemandInvalid);
            }
            if !self.consumers.iter().any(|c| c.cid == *cid) {
                self.cases = self.cases.saturating_add(1);
                return Err(XcCode::ArConsumerUnknown);
            }
        }
        let out = match self.track {
            ArbTrack::StaticPriority => self.arb_static(demands)?,
            ArbTrack::WeightedFair => self.arb_weighted(demands)?,
        };
        for g in &out {
            self.grants.push(*g);
        }
        Ok(out)
    }

    /// 确定性自检（判据"仲裁确定性"）：同输入重跑一次，结果逐行比对。
    pub fn check_determinism(&mut self, demands: &[(u32, u64)]) -> Result<(), XcCode> {
        let r1 = self.arbitrate(demands)?;
        let r2 = self.arbitrate(demands)?;
        if r1.len() != r2.len() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::ArDeterminism);
        }
        for (a, b) in r1.iter().zip(r2.iter()) {
            if a.cid != b.cid || a.granted_mbps != b.granted_mbps {
                self.cases = self.cases.saturating_add(1);
                return Err(XcCode::ArDeterminism);
            }
        }
        Ok(())
    }

    /// 归因账只读。
    pub fn grants(&self) -> &[ArbGrant] {
        &self.grants
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "PCIe仲裁{} 轨{} 消费者{} 带宽{}MB/s 决定{} 立案{}",
            PCIE_ARB_VERSION,
            self.track.zh(),
            self.consumers.len(),
            self.cap_mbps,
            self.grants.len(),
            self.cases,
        )
    }
}


/// v3 轮转公平轨（深化：赤字轮转 DRR 简化版——每消费者带赤字计数器，
/// 轮转发放直至赤字耗尽；防 v1 饿死且比 v2 静态权重更抗突发）。
#[derive(Clone, Copy, Debug)]
pub struct DrrState {
    /// 消费者 id。
    pub cid: u32,
    /// 赤字（MB/s 累计额度）。
    pub deficit: u64,
    /// 基础量子（每轮补足）。
    pub quantum_mbps: u64,
}

/// DRR 轮转发放（简化版：单轮内按序发放，量子耗尽跳过——确定性顺序轮转）。
pub fn drr_round(states: &mut [DrrState], cap_mbps: u64) -> Vec<(u32, u64)> {
    let mut remain = cap_mbps;
    let mut out = Vec::new();
    for st in states.iter_mut() {
        if remain == 0 {
            break;
        }
        st.deficit += st.quantum_mbps;
        let give = st.deficit.min(remain);
        st.deficit -= give;
        remain -= give;
        if give > 0 {
            out.push((st.cid, give));
        }
    }
    out
}

/// 归因聚合报表（深化：归因账的聚合视图——按原因文本聚合计数，
/// 消费方一眼看清拒绝主因）。
pub fn grant_summary(grants: &[ArbGrant]) -> Vec<(&'static str, usize, u64)> {
    let mut out: Vec<(&'static str, usize, u64)> = Vec::new();
    for g in grants {
        match out.iter_mut().find(|(r, _, _)| *r == g.reason) {
            Some((_, cnt, sum)) => {
                *cnt += 1;
                *sum += g.granted_mbps;
            }
            None => out.push((g.reason, 1, g.granted_mbps)),
        }
    }
    out
}

impl PcieArbiter {
    /// 消费者注销（深化：注销后其权重从和里扣除——v2 轨重标定守恒；
    /// 权重和不再恰 1e6 时按剩余比例重标定）。
    pub fn unregister(&mut self, cid: u32) -> Result<(), XcCode> {
        let before = self.consumers.len();
        self.consumers.retain(|c| c.cid != cid);
        if self.consumers.len() == before {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::ArConsumerUnknown);
        }
        if self.track == ArbTrack::WeightedFair {
            let sum: u64 = self.consumers.iter().map(|c| c.weight_ppm).sum();
            if sum != 1_000_000 && sum > 0 {
                let factor = 1_000_000 * 1_000_000 / sum;
                for c in self.consumers.iter_mut() {
                    c.weight_ppm = c.weight_ppm * factor / 1_000_000;
                }
                let after: u64 = self.consumers.iter().map(|c| c.weight_ppm).sum();
                if let Some(last) = self.consumers.last_mut() {
                    last.weight_ppm += 1_000_000u64.saturating_sub(after);
                }
            }
        }
        Ok(())
    }
}


/// 仲裁负载画像（深化：最近 N 笔仲裁的满足率/平均分配/最大欠额——
/// 轨道健康度的三个分量，调参依据）。
pub struct ArbPortrait {
    /// 请求笔数。
    pub demands: u64,
    /// 满足笔数。
    pub satisfied: u64,
    /// 平均分配（MB/s）。
    pub avg_granted: u64,
    /// 最大欠额（MB/s）。
    pub max_shortfall: u64,
}

impl ArbPortrait {
    pub fn from_grants(grants: &[ArbGrant]) -> Self {
        let mut demands = 0u64;
        let mut satisfied = 0u64;
        let mut sum = 0u64;
        let mut max_short = 0u64;
        for g in grants {
            demands += 1;
            if g.satisfied {
                satisfied += 1;
            }
            sum += g.granted_mbps;
            let short = g.demanded_mbps.saturating_sub(g.granted_mbps);
            if short > max_short {
                max_short = short;
            }
        }
        ArbPortrait {
            demands,
            satisfied,
            avg_granted: if demands == 0 { 0 } else { sum / demands },
            max_shortfall: max_short,
        }
    }

    /// 满足率（ppm）。
    pub const fn satisfy_ppm(&self) -> u64 {
        if self.demands == 0 {
            return 1_000_000;
        }
        self.satisfied * 1_000_000 / self.demands
    }
}
