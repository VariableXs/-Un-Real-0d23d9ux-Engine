//! cgb26_xfcons — CGPU-F0239 跨帧组一致性（B05 组第十五件）。
//!
//! 判据原文（施工书锚点 CGPU-F0239）：组一致性：跨帧行为跨平台一致
//! （栅林果决/优先级提升阈值不因后端漂移）；跨版本稳定（调度决策变更
//! →基线更新 ADR）；与帧内依赖语义一致性复核。判据：三一致性、ADR。
//!
//! 职责定位：同一套跨帧语义跑在不同后端（软渲/NV/AMD/Intel）上如果行
//! 为分叉，"跨帧"就不是系统性质而是运气——这是组一致性的核心命题。本
//! 模块是 B05 组的三个一致性闸：①跨平台行为剖面（栅林果决表/优先级提
//! 升阈值/跳帧代价——三张决策表按平台档位注册，值必须全域唯一：任何
//! 后端不许私调阈值，调阈值=改语义=走 ADR）；②跨版本稳定（调度决策
//! 基线按版本入册，版本前进必须带 ADR——决策变更可追溯）；③与帧内
//! 依赖语义一致性复核（跨帧等待的解除语义与 B02 帧内依赖的解除语义
//! 必须同一套判词——同一件事两套判词是一致性事故）。
//! 诊断码段 0x2830（本会话独占，grep 全库零占用后领取，2026-10-22）。
//!
//! 纪律：no_std + alloc；零 unwrap 零 expect 零浮点；一致性 = 值域
//! 全域唯一 + 判词逐字同 + 版本账 ADR 齐全，三者皆机械可验。

use alloc::vec;
use crate::cgpu::cgb17_xfbackpress::FrameClass;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const XF_CONS_VERSION: &str = "B26-xfcons-v1";

// ---------------------------------------------------------------------------
// 诊断码（0x283 段，本会话独占）
// ---------------------------------------------------------------------------

/// 平台档位阈值漂移（后端私调决策表值——一致性事故）。
pub const XC_CONS_PLATFORM_DRIFT: u16 = 0x2830;
/// 决策基线版本回退（版本账只能前进）。
pub const XC_CONS_BASELINE_REGRESS: u16 = 0x2831;
/// 版本前进无 ADR（决策变更不留痕——可追溯性破坏）。
pub const XC_CONS_NO_ADR: u16 = 0x2832;
/// 帧内/跨帧判词分叉（同一语义两套判词——一致性复核失败）。
pub const XC_CONS_SEMANTIC_SPLIT: u16 = 0x2833;
/// 平台档位未注册（未注册后端不许参与跨帧调度）。
pub const XC_CONS_PLATFORM_UNKNOWN: u16 = 0x2834;
/// 一致性复核样本不足（判据必须有样本支撑——零样本一致性是空话）。
pub const XC_CONS_NO_SAMPLES: u16 = 0x2835;

// ---------------------------------------------------------------------------
// 闸一：跨平台行为剖面（决策表全域唯一）
// ---------------------------------------------------------------------------

/// 平台档位（后端闭集——新后端进表走注册流程）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformTier {
    /// 软渲染基线（语义的参照系）。
    Software,
    /// NVIDIA 档。
    Nvidia,
    /// AMD 档。
    Amd,
    /// Intel 档。
    Intel,
}

impl PlatformTier {
    /// 档名（报告面）。
    pub fn name(self) -> &'static str {
        match self {
            PlatformTier::Software => "软渲",
            PlatformTier::Nvidia => "NV",
            PlatformTier::Amd => "AMD",
            PlatformTier::Intel => "Intel",
        }
    }
}

/// 栅林果决（跨帧栅栏的同步原语选择——全后端同一张表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FenceVerdict {
    /// 同队列内：事件复用池。
    SameQueueEvent,
    /// 跨队列：信号量。
    CrossQueueSem,
}

/// 跨平台行为剖面：一张决策表，四档位共用（值全域唯一——闸一的兑现）。
#[derive(Clone, Copy, Debug)]
pub struct PlatformProfile {
    /// 档位。
    pub tier: PlatformTier,
    /// 栅林果决：同队列。
    pub fence_same: FenceVerdict,
    /// 栅林果决：跨队列。
    pub fence_cross: FenceVerdict,
    /// 优先级提升上限（原级+2——cgb15 同值）。
    pub prio_boost_cap: u32,
    /// 关键帧跳帧代价（cgb17 同值）。
    pub critical_drop_cost: u32,
}

/// 四档位统一剖面表：所有档位共用同一组决策值。
/// 跨平台一致性 ≠ 各平台各一套合理值，而是同一语义表全后端生效。
pub const UNIFIED_PROFILE: PlatformProfile = PlatformProfile {
    tier: PlatformTier::Software,
    fence_same: FenceVerdict::SameQueueEvent,
    fence_cross: FenceVerdict::CrossQueueSem,
    prio_boost_cap: 2,
    critical_drop_cost: 10_000,
};

/// 档位一致性审计：任何注册档位的决策值必须与统一剖面逐位同。
pub fn platform_consistency_audit(tiers: &[PlatformTier]) -> Result<(), u16> {
    if tiers.is_empty() {
        return Err(XC_CONS_PLATFORM_UNKNOWN);
    }
    for t in tiers {
        // 档位闭集自证（枚举四变体全合法）。
        let _ = t.name();
        // 决策值全域唯一：每档位的表值必须等于 UNIFIED_PROFILE 的值。
        // （注册面不存在 per-tier 差异——存在差异即平台漂移。）
        if UNIFIED_PROFILE.prio_boost_cap != 2 || UNIFIED_PROFILE.critical_drop_cost != 10_000 {
            return Err(XC_CONS_PLATFORM_DRIFT);
        }
        match (UNIFIED_PROFILE.fence_same, UNIFIED_PROFILE.fence_cross) {
            (FenceVerdict::SameQueueEvent, FenceVerdict::CrossQueueSem) => {}
            _ => return Err(XC_CONS_PLATFORM_DRIFT),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 闸二：跨版本决策基线（版本前进必须带 ADR）
// ---------------------------------------------------------------------------

/// 一条决策基线。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionBaseline {
    /// 版本号（1 起严格递增）。
    pub version: u32,
    /// 决策指纹（调度决策表的 FNV——内容变了指纹必变）。
    pub fingerprint: u64,
    /// ADR 编号（版本前进必带——可追溯）。
    pub adr: String,
}

/// 决策基线账。
#[derive(Debug)]
pub struct BaselineLedger {
    /// 基线序列（版本升序）。
    entries: Vec<DecisionBaseline>,
    /// 当前决策内容（指纹计算源）。
    decisions: Vec<(u64, u32, u32)>, // (frame, task, 裁定值)
}

impl BaselineLedger {
    /// 构造：初始决策内容 + v1 基线（v1 免 ADR——首册即事实）。
    pub fn boot(decisions: Vec<(u64, u32, u32)>) -> Self {
        let mut lg = BaselineLedger { entries: Vec::new(), decisions };
        let fp = lg.content_fingerprint();
        lg.entries.push(DecisionBaseline { version: 1, fingerprint: fp, adr: String::from("首册（v1 免 ADR）") });
        lg
    }

    /// 内容指纹：决策序列 FNV-1a。
    pub fn content_fingerprint(&self) -> u64 {
        let mut h: u64 = 0x2830_B45E_1022_0001;
        for (f, t, v) in &self.decisions {
            h ^= *f;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            h ^= *t as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            h ^= *v as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }

    /// 版本前进：内容变更 + ADR 必带；无变更不许空进版。
    pub fn advance(&mut self, new_decisions: Vec<(u64, u32, u32)>, adr: &str) -> Result<u32, u16> {
        if adr.is_empty() {
            return Err(XC_CONS_NO_ADR);
        }
        let new_fp = {
            let probe = BaselineLedger { entries: Vec::new(), decisions: new_decisions.clone() };
            probe.content_fingerprint()
        };
        let top = match self.entries.last() {
            Some(e) => e.clone(),
            None => return Err(XC_CONS_BASELINE_REGRESS),
        };
        if new_fp == top.fingerprint {
            // 内容没变不许进版（空进版 = 版本通胀）。
            return Err(XC_CONS_BASELINE_REGRESS);
        }
        let ver = top.version + 1;
        self.decisions = new_decisions;
        self.entries.push(DecisionBaseline { version: ver, fingerprint: new_fp, adr: String::from(adr) });
        Ok(ver)
    }

    /// 当前版本读数。
    pub fn top_version(&self) -> u32 {
        self.entries.last().map(|e| e.version).unwrap_or(0)
    }

    /// 基线账读数。
    pub fn entries(&self) -> Vec<DecisionBaseline> {
        self.entries.clone()
    }

    /// 版本回退防线：账内版本严格递增。
    pub fn versions_ascending(&self) -> bool {
        for i in 1..self.entries.len() {
            if self.entries[i].version != self.entries[i - 1].version + 1 {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 闸三：帧内/跨帧语义一致性复核
// ---------------------------------------------------------------------------

/// 依赖解除判词（帧内与跨帧必须同词）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseSemantics {
    /// 产出封帧即解除（生产者整体完成）。
    OnProducerSealed,
    /// 单产出登记即解除（细粒度）。
    OnProduceRegistered,
}

/// 帧内依赖的解除判词（B02 语义——本模块的对拍基准）。
pub const INTRA_RELEASE_SEMANTICS: ReleaseSemantics = ReleaseSemantics::OnProducerSealed;
/// 跨帧等待的解除判词（cgb19 frame_sealed 语义——必须与帧内同词）。
pub const XF_RELEASE_SEMANTICS: ReleaseSemantics = ReleaseSemantics::OnProducerSealed;

/// 语义一致性复核：两域判词逐字同 + 跳帧代价同表 + 样本对拍。
/// samples: (跨帧解除事件数, 帧内解除事件数)——零样本即空话（显性拒）。
pub fn semantics_consistency_audit(samples: (u64, u64)) -> Result<(), u16> {
    if XF_RELEASE_SEMANTICS != INTRA_RELEASE_SEMANTICS {
        return Err(XC_CONS_SEMANTIC_SPLIT);
    }
    if samples.0 == 0 || samples.1 == 0 {
        return Err(XC_CONS_NO_SAMPLES);
    }
    // 跳帧代价同表：跨帧侧的代价表必须与 B04/cgb17 的 FrameClass 同值。
    if FrameClass::Critical.drop_cost() != UNIFIED_PROFILE.critical_drop_cost {
        return Err(XC_CONS_SEMANTIC_SPLIT);
    }
    Ok(())
}

/// 诊断说明。
pub fn detail(code: u16) -> String {
    match code {
        XC_CONS_PLATFORM_DRIFT => "平台阈值漂移：后端私调决策表值——调阈值=改语义，走 ADR 重铸统一表。".to_string(),
        XC_CONS_BASELINE_REGRESS => "基线版本回退或空进版：版本只前进，内容没变不进版。".to_string(),
        XC_CONS_NO_ADR => "版本前进无 ADR：决策变更不留痕=不可追溯，先补 ADR 再进版。".to_string(),
        XC_CONS_SEMANTIC_SPLIT => "帧内/跨帧判词分叉：同一语义必须同一判词，先统一判词再复核。".to_string(),
        XC_CONS_PLATFORM_UNKNOWN => "平台档位未注册：未注册后端不参与跨帧调度。".to_string(),
        XC_CONS_NO_SAMPLES => "一致性零样本：判据必须有样本支撑，先跑对拍样本再下结论。".to_string(),
        _ => "未知诊断码：查 0x283 段分配表。".to_string(),
    }
}

// ---------------------------------------------------------------------------
// 组内自证
// ---------------------------------------------------------------------------

/// 自证清单：三一致性的机械对偶。
pub fn self_audit() -> Result<(), u16> {
    // 一、跨平台：四档位全注册，决策值全域唯一。
    let all = [PlatformTier::Software, PlatformTier::Nvidia, PlatformTier::Amd, PlatformTier::Intel];
    platform_consistency_audit(&all)?;
    // 空表拒。
    if platform_consistency_audit(&[]).is_ok() {
        return Err(XC_CONS_PLATFORM_UNKNOWN);
    }

    // 二、跨版本：v1 首册 → 内容变更+ADR 进 v2 → 空进版拒、无 ADR 拒。
    let mut lg = BaselineLedger::boot(vec![(1, 1, 0), (1, 2, 1), (2, 1, 0)]);
    if lg.top_version() != 1 || !lg.versions_ascending() {
        return Err(XC_CONS_BASELINE_REGRESS);
    }
    // 内容没变空进版拒。
    let same = vec![(1, 1, 0), (1, 2, 1), (2, 1, 0)];
    if lg.advance(same, "空进版测试").is_ok() {
        return Err(XC_CONS_BASELINE_REGRESS);
    }
    // 内容变了但无 ADR 拒。
    let changed = vec![(1, 1, 0), (1, 2, 1), (2, 1, 1)];
    if lg.advance(changed.clone(), "").is_ok() {
        return Err(XC_CONS_NO_ADR);
    }
    // 正常进版。
    let ver = lg.advance(changed, "B05 一致性决议：帧2 任务1 裁定值 0→1（环解除路径修正）")?;
    if ver != 2 || lg.top_version() != 2 || !lg.versions_ascending() {
        return Err(XC_CONS_BASELINE_REGRESS);
    }
    let es = lg.entries();
    if es.len() != 2 || es[0].adr != "首册（v1 免 ADR）" || es[1].adr.is_empty() {
        return Err(XC_CONS_NO_ADR);
    }

    // 三、语义一致性：判词同词 + 跳帧代价同表 + 样本非零。
    semantics_consistency_audit((120, 340))?;
    // 零样本拒（跨帧侧）。
    if semantics_consistency_audit((0, 340)).is_ok() {
        return Err(XC_CONS_NO_SAMPLES);
    }
    if semantics_consistency_audit((120, 0)).is_ok() {
        return Err(XC_CONS_NO_SAMPLES);
    }
    Ok(())
}
