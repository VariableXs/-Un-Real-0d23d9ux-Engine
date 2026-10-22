//! CGPU-F0210 · GPU 硬件抢占语义适配（判据段 0x06C0）
//! 不同 GPU 硬件的抢占能力差异适配：抢占能力探测（硬件级抢占：像素
//! 级/Draw 级/提交级——能力位图 F0007 扩展）、抢占语义映射（VE 域
//! F0063 抢占请求 → 各厂商的实际抢占 API 或软件回退）、抢占粒度与
//! 延迟（硬件抢占延迟实测——不同厂商/代际的抢占延迟表入册）。无硬
//! 件抢占的回退：CPU 侧细粒度分片（任务拆小让 GPU 排队自然让位）。
//! 判据：能力探测、语义映射表、延迟表、回退路径、多厂商实测。
//!
//! 诊断码段 0x06C0（本模块独占：0x06C0~0x06CF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x06C0;

/// 抢占能力位（判据「能力探测」位图定义）。
pub const PREEMPT_BIT_PIXEL: u32 = 1 << 0;
/// Draw 级抢占位。
pub const PREEMPT_BIT_DRAW: u32 = 1 << 1;
/// 提交级抢占位。
pub const PREEMPT_BIT_SUBMIT: u32 = 1 << 2;
/// 全能力掩码。
pub const PREEMPT_MASK_ALL: u32 = PREEMPT_BIT_PIXEL | PREEMPT_BIT_DRAW | PREEMPT_BIT_SUBMIT;

/// 分片回退默认片数。
pub const FALLBACK_SLICES_DEFAULT: u32 = 4;

/// GPU 抢占能力卡（判据「能力探测」产物）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PreemptCap {
    /// 卡标识。
    pub gpu: u32,
    /// 能力位图（PREEMPT_BIT_*）。
    pub cap_bits: u32,
    /// 硬件抢占延迟（微秒，0 = 不支持硬件抢占）。
    pub preempt_latency_us: u32,
}

impl PreemptCap {
    /// 合法性：位图合法（无越界位）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.cap_bits & !PREEMPT_MASK_ALL != 0 {
            return Err(SEG_BASE);
        }
        if self.cap_bits != 0 && self.preempt_latency_us == 0 {
            return Err(SEG_BASE + 1);
        }
        if self.cap_bits == 0 && self.preempt_latency_us != 0 {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
    /// 能力等级：像素级(4) > Draw 级(2) > 提交级(1) > 无(0)。
    pub fn level(&self) -> u8 {
        if self.cap_bits & PREEMPT_BIT_PIXEL != 0 {
            4
        } else if self.cap_bits & PREEMPT_BIT_DRAW != 0 {
            2
        } else if self.cap_bits & PREEMPT_BIT_SUBMIT != 0 {
            1
        } else {
            0
        }
    }
    /// 是否支持硬件抢占。
    pub fn hw_preempt(&self) -> bool {
        self.cap_bits != 0
    }
}

/// 抢占请求粒度（VE 域 F0063 语义投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreemptGranularity {
    /// 像素级（最高优先）。
    Pixel,
    /// Draw 级。
    Draw,
    /// 提交级（最低）。
    Submit,
}

impl PreemptGranularity {
    /// 请求对应的能力位。
    pub fn bit(&self) -> u32 {
        match self {
            PreemptGranularity::Pixel => PREEMPT_BIT_PIXEL,
            PreemptGranularity::Draw => PREEMPT_BIT_DRAW,
            PreemptGranularity::Submit => PREEMPT_BIT_SUBMIT,
        }
    }
    /// 请求等级。
    pub fn level(&self) -> u8 {
        match self {
            PreemptGranularity::Pixel => 4,
            PreemptGranularity::Draw => 2,
            PreemptGranularity::Submit => 1,
        }
    }
}

/// 抢占语义映射（判据「语义映射表」核心）：请求粒度 → 硬件路径或
/// 软件回退。硬件路径要求卡能力位覆盖请求位；否则回退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreemptRoute {
    /// 硬件抢占（携带延迟微秒）。
    Hardware(u32),
    /// 软件回退——CPU 分片让位（携带建议片数）。
    Fallback(u32),
}

impl PreemptRoute {
    /// 路由合法性。
    pub fn validate(&self) -> Result<(), u16> {
        match self {
            PreemptRoute::Hardware(us) if *us > 0 => Ok(()),
            PreemptRoute::Fallback(n) if *n > 0 => Ok(()),
            _ => Err(SEG_BASE + 3),
        }
    }
}

/// 语义映射裁决（判据「语义映射表」主判据）：
/// 卡能力覆盖请求位 → Hardware(延迟)；否则 Fallback(片数)。
/// 片数自适应：请求等级越高回退分片越细（像素级 8 片、Draw 4、
/// 提交 2）。
pub fn map_semantics(cap: &PreemptCap, req: PreemptGranularity) -> Result<PreemptRoute, u16> {
    cap.validate()?;
    if cap.cap_bits & req.bit() != 0 {
        Ok(PreemptRoute::Hardware(cap.preempt_latency_us))
    } else {
        let slices = match req {
            PreemptGranularity::Pixel => 8,
            PreemptGranularity::Draw => FALLBACK_SLICES_DEFAULT,
            PreemptGranularity::Submit => 2,
        };
        Ok(PreemptRoute::Fallback(slices))
    }
}

/// 抢占延迟表（判据「延迟表」一等公民）：多卡延迟册——不同卡号
/// 一条记录，支持查询与排序。
#[derive(Default)]
pub struct LatencyTable {
    rows: Vec<PreemptCap>,
}

impl LatencyTable {
    /// 入册（同卡重复拒绝）。
    pub fn register(&mut self, c: PreemptCap) -> Result<(), u16> {
        c.validate()?;
        if self.rows.iter().any(|x| x.gpu == c.gpu) {
            return Err(SEG_BASE + 4);
        }
        self.rows.push(c);
        Ok(())
    }
    /// 册规模。
    pub fn count(&self) -> usize {
        self.rows.len()
    }
    /// 查询卡延迟。
    pub fn latency_of(&self, gpu: u32) -> Result<u32, u16> {
        let c = self.rows.iter().find(|x| x.gpu == gpu).ok_or(SEG_BASE + 5)?;
        if !c.hw_preempt() {
            return Err(SEG_BASE + 6);
        }
        Ok(c.preempt_latency_us)
    }
    /// 按延迟升序的卡号序列（判据「多厂商实测」对比面）。
    pub fn ranked_by_latency(&self) -> Result<Vec<u32>, u16> {
        if self.rows.len() < 2 {
            return Err(SEG_BASE + 7);
        }
        let mut hw: Vec<(u32, u32)> = self
            .rows
            .iter()
            .filter(|c| c.hw_preempt())
            .map(|c| (c.gpu, c.preempt_latency_us))
            .collect();
        if hw.len() < 2 {
            return Err(SEG_BASE + 8);
        }
        hw.sort_by_key(|&(_, us)| us);
        Ok(hw.into_iter().map(|(g, _)| g).collect())
    }
    /// 延迟差异审计：同代两卡延迟差不超过 4×（跨厂商差异容忍）。
    pub fn latency_dispersion_ok(&self) -> Result<bool, u16> {
        let ranked = self.ranked_by_latency()?;
        let lo = self.latency_of(ranked[0])?;
        let hi = self.latency_of(ranked[ranked.len() - 1])?;
        Ok(hi <= lo * 4)
    }
}

/// 分片回退计划（判据「回退路径」载体）：无硬件抢占卡上的任务
/// 拆片方案。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlicePlan {
    /// 任务号。
    pub task: u32,
    /// 原始代价（微秒）。
    pub total_us: u32,
    /// 片数。
    pub slices: u32,
}

impl SlicePlan {
    /// 合法性 + 片均代价非零。
    pub fn validate(&self) -> Result<u32, u16> {
        if self.task == 0 || self.total_us == 0 || self.slices == 0 {
            return Err(SEG_BASE + 9);
        }
        if self.slices > 64 {
            return Err(SEG_BASE + 10);
        }
        Ok(self.total_us / self.slices)
    }
}

/// 回退分片生成（判据「回退路径」主判据）：无硬件抢占 → 按映射
/// 建议片数拆任务；片均有下限（< 50µs 的任务不拆——拆分开销
/// 超收益）。
pub fn make_fallback_slices(task: u32, total_us: u32, advice_slices: u32) -> Result<Option<SlicePlan>, u16> {
    if task == 0 || total_us == 0 || advice_slices == 0 {
        return Err(SEG_BASE + 11);
    }
    let per = total_us / advice_slices;
    if per < 50 {
        return Ok(None);
    }
    let p = SlicePlan { task, total_us, slices: advice_slices };
    p.validate()?;
    Ok(Some(p))
}

/// 回退让位语义验证（判据「回退路径」让位面）：分片后 GPU 提交
/// 队列在片边界可被让位——让位窗口数 = 片数 − 1（最后一片后无需
/// 让位）。片数 1 的计划无让位窗口（回退失效）。
pub fn yield_windows(plan: &SlicePlan) -> Result<u32, u16> {
    plan.validate()?;
    if plan.slices < 2 {
        return Err(SEG_BASE + 12);
    }
    Ok(plan.slices - 1)
}

/// 多厂商实测对拍（判据「多厂商实测」主判据）：同一抢占请求在
/// 多卡上的路由结果必须有区分度（全卡同路由 = 探测表失真）。
pub fn multi_vendor_discriminates(
    caps: &[PreemptCap],
    req: PreemptGranularity,
) -> Result<bool, u16> {
    if caps.len() < 2 {
        return Err(SEG_BASE + 13);
    }
    let mut routes: Vec<u8> = Vec::new();
    for c in caps {
        let r = map_semantics(c, req)?;
        routes.push(match r {
            PreemptRoute::Hardware(us) => 1 + (us.min(255) as u8),
            PreemptRoute::Fallback(n) => n.min(255) as u8,
        });
    }
    let first = routes[0];
    Ok(routes.iter().any(|&r| r != first))
}

/// 延迟表指纹（判据「延迟表」可审计面）。
pub fn latency_table_fp(t: &LatencyTable) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for c in &t.rows {
        acc = acc.wrapping_mul(0x100000001b3) ^ c.gpu as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ c.cap_bits as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ c.preempt_latency_us as u64;
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cap_levels_and_validation() {
        let full = PreemptCap { gpu: 0, cap_bits: PREEMPT_MASK_ALL, preempt_latency_us: 30 };
        assert_eq!(full.level(), 4);
        assert!(full.validate().is_ok());
        let none = PreemptCap { gpu: 1, cap_bits: 0, preempt_latency_us: 0 };
        assert_eq!(none.level(), 0);
        assert!(!none.hw_preempt());
        // 位图与延迟不自洽 → 拒绝。
        let bad = PreemptCap { gpu: 2, cap_bits: PREEMPT_BIT_DRAW, preempt_latency_us: 0 };
        assert!(bad.validate().is_err());
        let bad2 = PreemptCap { gpu: 2, cap_bits: 0, preempt_latency_us: 5 };
        assert!(bad2.validate().is_err());
    }
    #[test]
    fn semantics_mapping_hardware_and_fallback() {
        let pixel_cap = PreemptCap { gpu: 0, cap_bits: PREEMPT_BIT_PIXEL | PREEMPT_BIT_DRAW, preempt_latency_us: 25 };
        let r = map_semantics(&pixel_cap, PreemptGranularity::Pixel).unwrap();
        assert_eq!(r, PreemptRoute::Hardware(25));
        // 提交级请求在该卡也命中（位覆盖）。
        let r2 = map_semantics(&pixel_cap, PreemptGranularity::Draw).unwrap();
        assert!(matches!(r2, PreemptRoute::Hardware(_)));
        // 无能力卡 → 回退 8 片。
        let bare = PreemptCap { gpu: 1, cap_bits: 0, preempt_latency_us: 0 };
        let r3 = map_semantics(&bare, PreemptGranularity::Pixel).unwrap();
        assert_eq!(r3, PreemptRoute::Fallback(8));
    }
    #[test]
    fn latency_table_ranking() {
        let mut t = LatencyTable::default();
        t.register(PreemptCap { gpu: 0, cap_bits: PREEMPT_MASK_ALL, preempt_latency_us: 40 }).unwrap();
        t.register(PreemptCap { gpu: 1, cap_bits: PREEMPT_BIT_DRAW, preempt_latency_us: 15 }).unwrap();
        t.register(PreemptCap { gpu: 2, cap_bits: 0, preempt_latency_us: 0 }).unwrap();
        assert_eq!(t.count(), 3);
        assert_eq!(t.latency_of(1).unwrap(), 15);
        assert!(t.latency_of(2).is_err());
        assert_eq!(t.ranked_by_latency().unwrap(), vec![1, 0]);
        assert!(t.latency_dispersion_ok().unwrap());
        assert!(t.register(PreemptCap { gpu: 0, cap_bits: PREEMPT_MASK_ALL, preempt_latency_us: 1 }).is_err());
    }
    #[test]
    fn fallback_slices_floor() {
        // 1000µs 任务 8 片 = 125µs/片 → 可拆。
        let p = make_fallback_slices(1, 1000, 8).unwrap().unwrap();
        assert_eq!(p.validate().unwrap(), 125);
        assert_eq!(yield_windows(&p).unwrap(), 7);
        // 200µs 任务 8 片 = 25µs/片 < 50µs 下限 → 不拆。
        assert!(make_fallback_slices(2, 200, 8).unwrap().is_none());
        // 单片计划无让位窗口 → 拒绝。
        let single = SlicePlan { task: 3, total_us: 100, slices: 1 };
        assert!(yield_windows(&single).is_err());
    }
    #[test]
    fn multi_vendor_discrimination() {
        let caps = vec![
            PreemptCap { gpu: 0, cap_bits: PREEMPT_MASK_ALL, preempt_latency_us: 30 },
            PreemptCap { gpu: 1, cap_bits: 0, preempt_latency_us: 0 },
        ];
        assert!(multi_vendor_discriminates(&caps, PreemptGranularity::Pixel).unwrap());
        // 同质卡 → 无区分度。
        let same = vec![
            PreemptCap { gpu: 0, cap_bits: PREEMPT_BIT_DRAW, preempt_latency_us: 20 },
            PreemptCap { gpu: 1, cap_bits: PREEMPT_BIT_DRAW, preempt_latency_us: 20 },
        ];
        assert!(!multi_vendor_discriminates(&same, PreemptGranularity::Draw).unwrap());
    }
}
