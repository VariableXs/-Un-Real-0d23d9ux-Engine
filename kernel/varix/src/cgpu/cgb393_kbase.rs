//! CGPU-F0393 · 预算知识库（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 知识库：超载模式手册（常见超载模式与处置——滚动超载/动画风暴/显存
//!   挤兑等案例）；标准处方（每模式的处置建议）；沉淀流程（新案例从归因
//!   数据提炼——持续生长）。判据：模式手册、处方、沉淀流程、判据。
//!
//! 设计纪律：
//! - 模式指纹可判别：归因读数 → 命中最匹配模式（确定性，平票取先登）。
//! - 处方与模式一一绑定；沉淀流程三步（捕获→验签→入册），验签不过不入。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0B70 独占。

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;

/// 诊断段基址（0x0B70，全仓独占）。
pub const SEG_BASE: u16 = 0x0B70;

pub const E_PATTERN_UNKNOWN: u16 = SEG_BASE + 1;
pub const E_RX_MISSING: u16 = SEG_BASE + 2;
pub const E_DISTILL_REJECTED: u16 = SEG_BASE + 3;
pub const E_AMBIGUOUS: u16 = SEG_BASE + 4;

/// 超载模式代号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pattern {
    /// 滚动超载：多账户轮番贴顶。
    RollingOverload,
    /// 动画风暴：短时大量同源申请。
    AnimationStorm,
    /// 显存挤兑：显存账户互抢致连锁降档。
    VramSqueeze,
}

impl Pattern {
    pub fn name(self) -> &'static str {
        match self {
            Pattern::RollingOverload => "rolling-overload",
            Pattern::AnimationStorm => "animation-storm",
            Pattern::VramSqueeze => "vram-squeeze",
        }
    }
}

/// 归因读数（判别输入——与 F0385 视图三同源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttributionReading {
    /// 贴顶账户占比（千分位）。
    pub saturate_ratio_pm: u32,
    /// 窗口申请突增倍数（×100 计，200=2 倍）。
    pub request_surge_hx: u32,
    /// 连锁降档账户数。
    pub chained_downgrades: u32,
    /// 显存类申请占比（千分位）。
    pub vram_ratio_pm: u32,
}

/// 标准处方（判据「处方」——模式绑定 + 步骤化）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prescription {
    pub pattern: Pattern,
    pub steps: Vec<&'static str>,
}

/// 知识库。
pub struct KnowledgeBase {
    manual: Vec<(Pattern, &'static str)>,
    rx: Vec<Prescription>,
    distill_seq: u64,
}

impl KnowledgeBase {
    pub fn new() -> Self {
        let mut kb = KnowledgeBase {
            manual: Vec::new(),
            rx: Vec::new(),
            distill_seq: 0,
        };
        kb.seed_builtin();
        kb
    }

    /// 内置手册与处方（判据「模式手册」——三案例预置）。
    fn seed_builtin(&mut self) {
        self.manual.push((Pattern::RollingOverload, "多账户轮番贴顶：检查公平指数是否被单方长期挤压"));
        self.manual.push((Pattern::AnimationStorm, "短时同源申请潮：确认风暴合并窗口与限频是否生效"));
        self.manual.push((Pattern::VramSqueeze, "显存互抢连锁降档：核对预留池下界与挤兑隔离带"));
        self.rx.push(Prescription {
            pattern: Pattern::RollingOverload,
            steps: vec!["冻结贴顶账户增量", "启用公平指数回归带", "观察两窗口后解除"],
        });
        self.rx.push(Prescription {
            pattern: Pattern::AnimationStorm,
            steps: vec!["合并同源申请", "限频到窗口基线", "风暴结束回吐预留"],
        });
        self.rx.push(Prescription {
            pattern: Pattern::VramSqueeze,
            steps: vec!["抬升显存预留下界", "隔离带内先予后夺", "挤兑平息后回收隔离"],
        });
    }

    /// 模式判别（确定性）：三条判别式加权最近的模式命中。
    /// 判别式（冻结）：
    /// - 滚动超载：贴顶率 ≥700 且 突增 <150；
    /// - 动画风暴：突增 ≥200 且 贴顶率 <700；
    /// - 显存挤兑：连锁降档 ≥3 且 显存占比 ≥500。
    pub fn classify(&self, r: &AttributionReading) -> Result<Pattern, u16> {
        let hit_rolling = r.saturate_ratio_pm >= 700 && r.request_surge_hx < 150;
        let hit_storm = r.request_surge_hx >= 200 && r.saturate_ratio_pm < 700;
        let hit_vram = r.chained_downgrades >= 3 && r.vram_ratio_pm >= 500;
        let hits = [hit_rolling, hit_storm, hit_vram];
        let count = hits.iter().filter(|h| **h).count();
        if count > 1 {
            return Err(E_AMBIGUOUS);
        }
        if count == 0 {
            return Err(E_PATTERN_UNKNOWN);
        }
        if hit_rolling {
            Ok(Pattern::RollingOverload)
        } else if hit_storm {
            Ok(Pattern::AnimationStorm)
        } else {
            Ok(Pattern::VramSqueeze)
        }
    }

    /// 手册查询。
    pub fn manual_entry(&self, p: Pattern) -> Result<&'static str, u16> {
        self.manual.iter().find(|(k, _)| *k == p).map(|(_, v)| *v).ok_or(E_PATTERN_UNKNOWN)
    }

    /// 处方查询（判据「处方」——缺处方显性报错）。
    pub fn prescription(&self, p: Pattern) -> Result<&Prescription, u16> {
        self.rx.iter().find(|r| r.pattern == p).ok_or(E_RX_MISSING)
    }

    /// 沉淀流程（判据「沉淀」）：捕获 → 验签 → 入册。
    /// 验签三条件（冻结）：有模式命中、有手册条目（或新增）、有至少一步处方。
    pub fn distill(&mut self, r: &AttributionReading, case_note: &str, steps: Vec<&'static str>) -> Result<u64, u16> {
        let p = self.classify(r)?;
        if case_note.len() < 12 {
            return Err(E_DISTILL_REJECTED); // 案例描述过薄不入册
        }
        // 手册无此模式则新增（持续生长）。
        if !self.manual.iter().any(|(k, _)| *k == p) {
            self.manual.push((p, "distilled"));
        }
        // 处方：已有则不覆盖内置；没有则入新（但至少一步）。
        if !self.rx.iter().any(|x| x.pattern == p) {
            if steps.is_empty() {
                return Err(E_RX_MISSING);
            }
            self.rx.push(Prescription { pattern: p, steps });
        }
        // 处方已有：内置不覆盖、无新步也合法（样本复现即沉淀）。
        self.distill_seq += 1;
        Ok(self.distill_seq)
    }

    pub fn manual_len(&self) -> usize {
        self.manual.len()
    }

    pub fn rx_len(&self) -> usize {
        self.rx.len()
    }
}

impl Default for KnowledgeBase {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn kb() -> KnowledgeBase {
        KnowledgeBase::new()
    }

    fn rolling() -> AttributionReading {
        AttributionReading { saturate_ratio_pm: 850, request_surge_hx: 100, chained_downgrades: 1, vram_ratio_pm: 100 }
    }

    fn storm() -> AttributionReading {
        AttributionReading { saturate_ratio_pm: 400, request_surge_hx: 300, chained_downgrades: 1, vram_ratio_pm: 100 }
    }

    fn vram() -> AttributionReading {
        AttributionReading { saturate_ratio_pm: 400, request_surge_hx: 100, chained_downgrades: 4, vram_ratio_pm: 800 }
    }

    #[test]
    fn a93_手册_三案例预置与判别() {
        let k = kb();
        assert_eq!(k.manual_len(), 3);
        assert_eq!(k.rx_len(), 3);
        // 判别：三读数各归其位。
        assert_eq!(k.classify(&rolling()).unwrap(), Pattern::RollingOverload);
        assert_eq!(k.classify(&storm()).unwrap(), Pattern::AnimationStorm);
        assert_eq!(k.classify(&vram()).unwrap(), Pattern::VramSqueeze);
        // 未知模式（无命中）。
        let calm = AttributionReading { saturate_ratio_pm: 100, request_surge_hx: 50, chained_downgrades: 0, vram_ratio_pm: 10 };
        assert_eq!(k.classify(&calm).unwrap_err(), E_PATTERN_UNKNOWN);
        // 歧义（滚动+挤兑两式同中）拒：贴顶率高且连锁降档足量。
        let amb = AttributionReading { saturate_ratio_pm: 850, request_surge_hx: 100, chained_downgrades: 5, vram_ratio_pm: 600 };
        assert_eq!(k.classify(&amb).unwrap_err(), E_AMBIGUOUS);
        // 手册与处方查询。
        assert_eq!(k.manual_entry(Pattern::AnimationStorm).unwrap(), "短时同源申请潮：确认风暴合并窗口与限频是否生效");
        assert_eq!(k.prescription(Pattern::VramSqueeze).unwrap().steps.len(), 3);
        // 模式名。
        assert_eq!(Pattern::RollingOverload.name(), "rolling-overload");
    }

    #[test]
    fn a93_处方_步骤化与缺失显性() {
        let k = kb();
        let rx = k.prescription(Pattern::RollingOverload).unwrap();
        assert_eq!(rx.pattern, Pattern::RollingOverload);
        assert_eq!(rx.steps, vec!["冻结贴顶账户增量", "启用公平指数回归带", "观察两窗口后解除"]);
        // 内置库三模式处方齐套。
        for p in [Pattern::RollingOverload, Pattern::AnimationStorm, Pattern::VramSqueeze] {
            assert!(k.prescription(p).is_ok());
        }
    }

    #[test]
    fn a93_沉淀_三步验签与持续生长() {
        let mut k = kb();
        // 合法沉淀（描述足够 + 处方已在）→ 序号递增。
        let s1 = k.distill(&rolling(), "rolling case 001: 贴顶率长期 850‰", vec![]).unwrap();
        let s2 = k.distill(&storm(), "storm case 002: 同源 3 倍突增", vec![]).unwrap();
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        // 描述过薄拒（验签）。
        assert_eq!(k.distill(&vram(), "too short", vec![]).unwrap_err(), E_DISTILL_REJECTED);
        // 未知模式沉淀拒（无命中不可入册）。
        let calm = AttributionReading { saturate_ratio_pm: 100, request_surge_hx: 50, chained_downgrades: 0, vram_ratio_pm: 10 };
        assert_eq!(k.distill(&calm, "calm case 003: 无模式", vec![]).unwrap_err(), E_PATTERN_UNKNOWN);
        // 处方步数空且库中无该模式处方 → 拒。
        let mut k2 = KnowledgeBase::new();
        k2.rx.clear();
        assert_eq!(k2.distill(&vram(), "vram case 004: 挤兑样本", vec![]).unwrap_err(), E_RX_MISSING);
        // 带处方步数的沉淀：入册生长。
        let s = k2.distill(&vram(), "vram case 005: 连锁降档 4 户", vec!["抬下界", "隔离"]).unwrap();
        assert_eq!(s, 1);
        assert_eq!(k2.prescription(Pattern::VramSqueeze).unwrap().steps, vec!["抬下界", "隔离"]);
        assert_eq!(k2.rx_len(), 1);
        // 内置处方不被沉淀覆盖。
        let mut k3 = kb();
        k3.distill(&storm(), "storm case 006: 复现样本", vec!["fake-step"]).unwrap();
        assert_ne!(k3.prescription(Pattern::AnimationStorm).unwrap().steps, vec!["fake-step"]);
    }
}
