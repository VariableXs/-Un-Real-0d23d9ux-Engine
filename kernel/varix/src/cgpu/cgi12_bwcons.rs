//! cgi12_bwcons — CGPU-F1291 带宽一致性（仲裁确定+跨代归一+ADR 语义门）。
//!
//! 判据原文（施工书锚点 CGPU-F1291）：一致性：仲裁确定性（同负载同分配）；
//! 跨硬件归一（Gen3/4/5 归一化口径）；跨版本稳定（仲裁语义变更→ADR）。
//! 判据：确定、归一、ADR、判据。
//!
//! 职责定位：仲裁确定性（同负载快照→同分配序列是仲裁器的物理定律：负载
//! 指纹→分配指纹，同负载异分配 0x7D01 立案——仲裁不是掷骰子）+跨代归一
//! （Gen3/4/5 成绩沿代际理论带宽折算归一后可比——绕过归一比裸吞吐 0x7D02）
//! +跨版本 ADR 门（仲裁语义变更→ADR 单号强制——无 ADR 语义变更 0x7D03；
//! 分配账与语义账交叉对拍不平 0x7D04；ADR 链断环 0x7D05）。与 cgf16_certcons
//! 同族模式。诊断码段 0x7D01~0x7D05。

use alloc::vec::Vec;

/// 模块版本。
pub const BW_CONS_VERSION: &str = "I12-bwcons-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x7D 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x7D01 仲裁不确定（同负载异分配）。
pub const XC_ARB_NONDET: u32 = 0x7D01;
/// 0x7D02 归一失据（跨代比较未走归一口径）。
pub const XC_GEN_NORM_UNBASED: u32 = 0x7D02;
/// 0x7D03 ADR 缺席（仲裁语义变更无 ADR）。
pub const XC_BW_ADR_MISSING: u32 = 0x7D03;
/// 0x7D04 语义账不平（分配账与语义账对拍失败）。
pub const XC_SEM_IMBALANCE: u32 = 0x7D04;
/// 0x7D05 ADR 链断环。
pub const XC_ADR_CHAIN_BROKEN: u32 = 0x7D05;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 代际归一档数（Gen1~Gen5）。
pub const GEN_NORM_COUNT: usize = 5;
/// 代际理论带宽（GB/s——与 cgi11 同源闭集）。
pub const GEN_THEORY_GBPS: [u32; GEN_NORM_COUNT] = [4, 8, 16, 32, 64];
/// ADR 链容量。
pub const BW_ADR_CAP: usize = 16;

// ---------------------------------------------------------------------------
// 一、仲裁确定性（同负载同分配）
// ---------------------------------------------------------------------------

/// 负载快照键（负载内容 FNV）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoadKey {
    /// 负载指纹。
    pub load_fp: u64,
}

/// 仲裁确定性账（判据"同负载同分配"）。
#[derive(Debug)]
pub struct ArbiterDeterminism {
    /// 分配记录（负载键→分配指纹）。
    pub records: Vec<(LoadKey, u64)>,
    /// 不确定事件数（0x7D01）。
    pub nondet_events: u32,
}

impl ArbiterDeterminism {
    /// 新账。
    pub const fn new() -> ArbiterDeterminism {
        ArbiterDeterminism {
            records: Vec::new(),
            nondet_events: 0,
        }
    }

    /// 分配入账（同负载键必须同分配指纹——异分配 0x7D01）。
    pub fn arbitrate(&mut self, key: LoadKey, alloc_fp: u64) -> Result<u64, u32> {
        let mut i = 0;
        while i < self.records.len() {
            if self.records[i].0 == key {
                if self.records[i].1 != alloc_fp {
                    self.nondet_events += 1;
                    return Err(XC_ARB_NONDET);
                }
                return Ok(alloc_fp);
            }
            i += 1;
        }
        self.records.push((key, alloc_fp));
        Ok(alloc_fp)
    }
}

// ---------------------------------------------------------------------------
// 二、跨代归一（Gen3/4/5 归一化口径）
// ---------------------------------------------------------------------------

/// 跨代归一表（判据"跨硬件归一（Gen3/4/5 归一化口径"）。
#[derive(Debug)]
pub struct CrossGenNorm {
    /// 已登记代位图。
    pub registered: u32,
    /// 失据事件数（0x7D02）。
    pub unbased_events: u32,
}

impl CrossGenNorm {
    /// 新表。
    pub const fn new() -> CrossGenNorm {
        CrossGenNorm {
            registered: 0,
            unbased_events: 0,
        }
    }

    /// 登记代（幂等）。
    pub fn register(&mut self, gen: usize) -> Result<(), u32> {
        if gen >= GEN_NORM_COUNT {
            self.unbased_events += 1;
            return Err(XC_GEN_NORM_UNBASED);
        }
        self.registered |= 1 << gen;
        Ok(())
    }

    /// 归一折算：实测吞吐（GB/s）×理论基准代带宽/本代理论带宽——统一折到
    /// Gen1 当量。未登记代 0x7D02。
    pub fn normalize_to_gen1(&mut self, gen: usize, measured_gbps: u32) -> Result<u32, u32> {
        if gen >= GEN_NORM_COUNT || self.registered & (1 << gen) == 0 {
            self.unbased_events += 1;
            return Err(XC_GEN_NORM_UNBASED);
        }
        // Gen1 当量 = measured × GEN_THEORY[0] / GEN_THEORY[gen]（万分比保留）。
        let v = measured_gbps as u64 * GEN_THEORY_GBPS[0] as u64 * 10_000
            / GEN_THEORY_GBPS[gen] as u64;
        Ok(if v > u32::MAX as u64 { u32::MAX } else { v as u32 })
    }

    /// 跨代可比：两代实测归一后等值判定。
    pub fn comparable(&mut self, a_gen: usize, a_gbps: u32, b_gen: usize, b_gbps: u32) -> Result<bool, u32> {
        let a = self.normalize_to_gen1(a_gen, a_gbps)?;
        let b = self.normalize_to_gen1(b_gen, b_gbps)?;
        Ok(a == b)
    }
}

// ---------------------------------------------------------------------------
// 三、跨版本 ADR 门（仲裁语义变更→ADR）
// ---------------------------------------------------------------------------

/// ADR 语义门（判据"仲裁语义变更→ADR"）。
#[derive(Debug)]
pub struct AdrSemantics {
    /// 当前语义版本。
    pub current_ver: u32,
    /// ADR 单号账。
    pub adrs: Vec<u32>,
    /// 无 ADR 变更事件数（0x7D03）。
    pub miss_events: u32,
}

impl AdrSemantics {
    /// 新门（v1 基线）。
    pub const fn new() -> AdrSemantics {
        AdrSemantics {
            current_ver: 1,
            adrs: Vec::new(),
            miss_events: 0,
        }
    }

    /// 语义变更（ADR 单号必填 0x7D03；链容量上限）。
    pub fn bump(&mut self, adr_no: u32) -> Result<u32, u32> {
        if adr_no == 0 {
            self.miss_events += 1;
            return Err(XC_BW_ADR_MISSING);
        }
        if self.adrs.len() >= BW_ADR_CAP {
            self.miss_events += 1;
            return Err(XC_BW_ADR_MISSING);
        }
        self.current_ver += 1;
        self.adrs.push(adr_no);
        Ok(self.current_ver)
    }

    /// 链闭合：ADR 数≡版本差（每版本恰一 ADR——语义账与版本账对拍）。
    pub fn chain_ok(&self) -> Result<(), u32> {
        if self.adrs.len() as u32 != self.current_ver - 1 {
            return Err(XC_ADR_CHAIN_BROKEN);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、总引擎 + 自检（确定/归一/ADR/判据四组+段对账）
// ---------------------------------------------------------------------------

/// 带宽一致性总引擎。
#[derive(Debug)]
pub struct BwConsEngine {
    /// 仲裁确定性账。
    pub arb: ArbiterDeterminism,
    /// 跨代归一表。
    pub norm: CrossGenNorm,
    /// ADR 语义门。
    pub gate: AdrSemantics,
}

impl BwConsEngine {
    /// 出厂。
    pub const fn new() -> BwConsEngine {
        BwConsEngine {
            arb: ArbiterDeterminism::new(),
            norm: CrossGenNorm::new(),
            gate: AdrSemantics::new(),
        }
    }

    /// 组一：仲裁确定性——同负载同分配过；异分配 0x7D01。
    pub fn selfcheck_arb(&mut self) -> Result<(), u32> {
        let mut e = BwConsEngine::new();
        let k = LoadKey { load_fp: 0x7007 };
        e.arb.arbitrate(k, 0xA1B2)?;
        e.arb.arbitrate(k, 0xA1B2)?;
        if e.arb.arbitrate(k, 0xC3D4) != Err(XC_ARB_NONDET) {
            return Err(XC_ARB_NONDET);
        }
        if e.arb.nondet_events != 1 {
            return Err(XC_ARB_NONDET);
        }
        Ok(())
    }

    /// 组二：跨代归一——未登记拒；折算对；跨代可比。
    pub fn selfcheck_norm(&mut self) -> Result<(), u32> {
        let mut e = BwConsEngine::new();
        // 未登记 Gen3 → 0x7D02。
        if e.norm.normalize_to_gen1(2, 12).is_ok() {
            return Err(XC_GEN_NORM_UNBASED);
        }
        if e.norm.unbased_events != 1 {
            return Err(XC_GEN_NORM_UNBASED);
        }
        // 登记三代。
        e.norm.register(2)?; // Gen3
        e.norm.register(3)?; // Gen4
        e.norm.register(4)?; // Gen5
        // 折算：Gen3 12GB/s → 12×4/16×10000=30000；Gen4 24GB/s → 24×4/32×10000=30000。
        if e.norm.normalize_to_gen1(2, 12)? != 30_000 {
            return Err(XC_GEN_NORM_UNBASED);
        }
        // 跨代可比：Gen3 12 ≡ Gen4 24（同为 Gen1 当量 30000）。
        if !e.norm.comparable(2, 12, 3, 24)? {
            return Err(XC_GEN_NORM_UNBASED);
        }
        if e.norm.comparable(2, 12, 4, 24)? {
            return Err(XC_GEN_NORM_UNBASED); // Gen5 未参与 comparable 的 a 侧合法——b 侧 24 在 Gen4 已可比；这里 Gen4 24 vs Gen5 24 归一不同
        }
        Ok(())
    }

    /// 组三：ADR——无 ADR 0x7D03；链闭合；容量上限。
    pub fn selfcheck_adr(&mut self) -> Result<(), u32> {
        let mut e = BwConsEngine::new();
        if e.gate.bump(0) != Err(XC_BW_ADR_MISSING) {
            return Err(XC_BW_ADR_MISSING);
        }
        if e.gate.miss_events != 1 {
            return Err(XC_BW_ADR_MISSING);
        }
        e.gate.bump(0x8001)?;
        e.gate.bump(0x8002)?;
        e.gate.chain_ok()?;
        if e.gate.current_ver != 3 {
            return Err(XC_ADR_CHAIN_BROKEN);
        }
        Ok(())
    }

    /// 组四：判据——端到端：确定+归一+ADR 全链绿。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = BwConsEngine::new();
        e.norm.register(2)?;
        e.norm.register(3)?;
        let k = LoadKey { load_fp: 0x911 };
        e.arb.arbitrate(k, 0x77)?;
        // 语义升级（ADR 合法）后同负载分配不变（语义版本与分配内容解耦场景）。
        e.gate.bump(0x8001)?;
        e.arb.arbitrate(k, 0x77)?;
        if e.arb.nondet_events != 0 || e.norm.unbased_events != 0 || e.gate.miss_events != 0 {
            return Err(XC_SEM_IMBALANCE);
        }
        e.gate.chain_ok()?;
        Ok(())
    }

    /// 全量自检入口（四组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_arb()?;
        self.selfcheck_norm()?;
        self.selfcheck_adr()?;
        self.selfcheck_verdict()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x7D00..0x7E00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_ARB_NONDET,
            XC_GEN_NORM_UNBASED,
            XC_BW_ADR_MISSING,
            XC_SEM_IMBALANCE,
            XC_ADR_CHAIN_BROKEN,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7D00 || codes[i] >= 0x7E00 {
                return Err(XC_ARB_NONDET);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_ARB_NONDET);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
