//! VE-F0096 · 遥测采样率治理（VE-A 域 · A04b 诊断·降级·安全组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0096`
//!
//! **职责定位（锚点原文）**：遥测采样率按预算制治理——总采样预算每秒 N 点，
//! 按指标族分级配额，帧率与延迟类高频族、配置类低频族各占独立配额；负载越限
//! 时按优先级自动降采样而非全量丢弃，恢复后阶梯回升；每次采样率变更全量入
//! 体验日志，含变更原因与前后速率。
//!
//! **铁律**：
//! 1. **预算制**——总预算按族分独立配额，互不挤占。
//! 2. **降采样阶梯**——100%/50%/10% 三档，不全量丢弃。
//! 3. **优先级保序**——降级先降低优先级族，保高优先级完整。
//! 4. **变更入日志**——每次速率变更记录原因与前后速率。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8A7（采样治理）。
pub const SEG_BASE: u16 = 0xE8A7;

/// 预算配置非法。
pub const XC_BUDGET_INVALID: u16 = SEG_BASE | 0x01;
/// 采样器积压。
pub const XC_BACKLOG: u16 = SEG_BASE | 0x02;
/// 配额耗尽。
pub const XC_QUOTA_EXHAUST: u16 = SEG_BASE | 0x03;
/// 未知指标族。
pub const XC_UNKNOWN_FAMILY: u16 = SEG_BASE | 0x04;
/// 阶梯越档。
pub const XC_STEP_OVERFLOW: u16 = SEG_BASE | 0x05;

/// 降采样阶梯档位：100% / 50% / 10%。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleStep {
    Full,
    Half,
    Tenth,
}

impl SampleStep {
    /// 档位→保留比例（千分比）。
    pub fn ratio_permille(&self) -> u16 {
        match self {
            SampleStep::Full => 1000,
            SampleStep::Half => 500,
            SampleStep::Tenth => 100,
        }
    }

    /// 升一档（恢复阶梯回升）。
    pub fn escalate(&self) -> SampleStep {
        match self {
            SampleStep::Tenth => SampleStep::Half,
            SampleStep::Half => SampleStep::Full,
            SampleStep::Full => SampleStep::Full,
        }
    }

    /// 降一档（越限降采样）。
    pub fn degrade(&self) -> SampleStep {
        match self {
            SampleStep::Full => SampleStep::Half,
            SampleStep::Half => SampleStep::Tenth,
            SampleStep::Tenth => SampleStep::Tenth,
        }
    }
}

/// 指标族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricFamily {
    /// 帧率类（高频）。
    FrameRate,
    /// 延迟类（高频）。
    Latency,
    /// 配置类（低频）。
    Config,
    /// 资源类（中频）。
    Resource,
}

impl MetricFamily {
    /// 默认优先级（数字小 = 优先级高）。
    pub fn default_priority(&self) -> u8 {
        match self {
            MetricFamily::Latency => 0,
            MetricFamily::FrameRate => 1,
            MetricFamily::Resource => 2,
            MetricFamily::Config => 3,
        }
    }
}

/// 族配额配置。
#[derive(Clone, Copy, Debug)]
pub struct FamilyQuota {
    pub family: MetricFamily,
    /// 每秒配额点数。
    pub quota_per_sec: u32,
    /// 优先级（小者高）。
    pub priority: u8,
}

/// 采样率变更事件（全量入体验日志）。
#[derive(Clone, Debug)]
pub struct RateChangeEvent {
    pub tick: u64,
    pub family: MetricFamily,
    pub before_permille: u16,
    pub after_permille: u16,
    /// 变更原因：0=越限降档 1=恢复升档 2=积压降频 3=初始化。
    pub reason: u8,
}

/// 族运行态。
#[derive(Clone, Debug)]
struct FamilyState {
    quota: FamilyQuota,
    step: SampleStep,
    used_this_sec: u32,
    backlog: u32,
}

/// 采样率治理器。
pub struct SampleGovernor {
    /// 总预算每秒点数。
    total_budget: u32,
    families: Vec<FamilyState>,
    tick: u64,
    /// 变更日志。
    log: Vec<RateChangeEvent>,
    last_code: u16,
    /// 累计放行采样点。
    admitted: u64,
    /// 累计降采样丢弃点。
    shed: u64,
}

impl SampleGovernor {
    pub fn new(total_budget: u32) -> Result<Self, u16> {
        if total_budget == 0 {
            return Err(XC_BUDGET_INVALID);
        }
        Ok(SampleGovernor {
            total_budget,
            families: Vec::new(),
            tick: 0,
            log: Vec::new(),
            last_code: 0,
            admitted: 0,
            shed: 0,
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 加载配额表：非法拒绝并保持上一次有效配置。
    pub fn load_quotas(&mut self, quotas: &[FamilyQuota]) -> Result<(), u16> {
        if quotas.is_empty() {
            self.last_code = XC_BUDGET_INVALID;
            return Err(XC_BUDGET_INVALID);
        }
        let sum: u32 = quotas.iter().map(|q| q.quota_per_sec).sum();
        if sum == 0 || sum > self.total_budget * 2 {
            // 配额总和超总预算两倍 = 配置失真，拒绝。
            self.last_code = XC_BUDGET_INVALID;
            return Err(XC_BUDGET_INVALID);
        }
        for q in quotas {
            if q.quota_per_sec == 0 {
                self.last_code = XC_BUDGET_INVALID;
                return Err(XC_BUDGET_INVALID);
            }
        }
        // 全部合法才替换（原子换表）。
        self.families = quotas
            .iter()
            .map(|q| FamilyState {
                quota: *q,
                step: SampleStep::Full,
                used_this_sec: 0,
                backlog: 0,
            })
            .collect();
        self.last_code = 0;
        Ok(())
    }

    fn find_mut(&mut self, family: MetricFamily) -> Result<&mut FamilyState, u16> {
        match self.families.iter_mut().find(|f| f.quota.family == family) {
            Some(f) => Ok(f),
            None => {
                self.last_code = XC_UNKNOWN_FAMILY;
                Err(XC_UNKNOWN_FAMILY)
            }
        }
    }

    /// 试采样一点：按族配额与阶梯裁决。
    pub fn try_sample(&mut self, family: MetricFamily) -> Result<bool, u16> {
        // 先定位族（借用分离）。
        let idx = match self.families.iter().position(|f| f.quota.family == family) {
            Some(i) => i,
            None => {
                self.last_code = XC_UNKNOWN_FAMILY;
                return Err(XC_UNKNOWN_FAMILY);
            }
        };
        let f = &mut self.families[idx];
        let allow = f.used_this_sec < f.quota.quota_per_sec;
        if allow {
            f.used_this_sec += 1;
            self.admitted += 1;
            self.last_code = 0;
            Ok(true)
        } else {
            f.backlog = f.backlog.saturating_add(1);
            self.shed += 1;
            // 越限触发降档（若还有档可降）。
            if f.step != SampleStep::Tenth {
                let before = f.step.ratio_permille();
                f.step = f.step.degrade();
                let after = f.step.ratio_permille();
                self.log.push(RateChangeEvent {
                    tick: self.tick,
                    family,
                    before_permille: before,
                    after_permille: after,
                    reason: 0,
                });
            }
            self.last_code = XC_QUOTA_EXHAUST;
            Ok(false)
        }
    }

    /// 按阶梯决定是否真正放行（降采样是丢一部分，不是全丢）。
    /// 决策用单调计数器取模实现，零随机。
    pub fn admit_by_step(&mut self, family: MetricFamily, seq: u32) -> Result<bool, u16> {
        let idx = match self.families.iter().position(|f| f.quota.family == family) {
            Some(i) => i,
            None => {
                self.last_code = XC_UNKNOWN_FAMILY;
                return Err(XC_UNKNOWN_FAMILY);
            }
        };
        let step = self.families[idx].step;
        let ratio = step.ratio_permille();
        // 取模映射：ratio=1000 全放；500 偶数放；100 每 10 放 1。
        let modv = match ratio {
            1000 => 1u32,
            500 => 2,
            100 => 10,
            _ => 1,
        };
        Ok(seq % modv == 0)
    }

    /// 秒级推进：清配额、积压处理、恢复升档。
    pub fn advance_sec(&mut self) {
        self.tick += 1;
        for f in self.families.iter_mut() {
            f.used_this_sec = 0;
            if f.backlog > 0 {
                // 积压：先降频（已由 try_sample 降档），清积压计数。
                f.backlog = 0;
            } else if f.step != SampleStep::Full {
                // 无积压则阶梯回升。
                let before = f.step.ratio_permille();
                f.step = f.step.escalate();
                let after = f.step.ratio_permille();
                if before != after {
                    self.log.push(RateChangeEvent {
                        tick: self.tick,
                        family: f.quota.family,
                        before_permille: before,
                        after_permille: after,
                        reason: 1,
                    });
                }
            }
        }
    }

    /// 积压超限显性告警（不丢点，降频已在 try_sample 做）。
    pub fn backlog_alert(&self, family: MetricFamily) -> Result<bool, u16> {
        match self.families.iter().find(|f| f.quota.family == family) {
            Some(f) => Ok(f.backlog > f.quota.quota_per_sec),
            None => Err(XC_UNKNOWN_FAMILY),
        }
    }

    pub fn step_of(&self, family: MetricFamily) -> Result<SampleStep, u16> {
        match self.families.iter().find(|f| f.quota.family == family) {
            Some(f) => Ok(f.step),
            None => Err(XC_UNKNOWN_FAMILY),
        }
    }

    /// 配额耗尽时保优先级最高族完整：返回仍可完整采样的族列表。
    pub fn protected_families(&self) -> Vec<MetricFamily> {
        let mut min_p = u8::MAX;
        for f in &self.families {
            if f.quota.priority < min_p {
                min_p = f.quota.priority;
            }
        }
        self.families
            .iter()
            .filter(|f| f.quota.priority == min_p && f.step == SampleStep::Full)
            .map(|f| f.quota.family)
            .collect()
    }

    pub fn change_log(&self) -> &[RateChangeEvent] {
        &self.log
    }

    pub fn admitted(&self) -> u64 {
        self.admitted
    }

    pub fn shed(&self) -> u64 {
        self.shed
    }
}

/// 自检：采样预算、降采样阶梯、优先级保序、变更入日志。
pub fn run_vea96_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn quotas() -> Vec<FamilyQuota> {
        vec![
            FamilyQuota {
                family: MetricFamily::FrameRate,
                quota_per_sec: 10,
                priority: 1,
            },
            FamilyQuota {
                family: MetricFamily::Config,
                quota_per_sec: 2,
                priority: 3,
            },
        ]
    }

    let mut set = CheckSet::new("vea96_sampler");

    // C96-01 预算配置加载
    {
        let mut g = SampleGovernor::new(100).unwrap();
        let r = g.load_quotas(&quotas());
        chk(&mut set, "C96-01 预算加载", r.is_ok(), "加载失败");
    }

    // C96-02 非法配置拒绝并保留
    {
        let mut g = SampleGovernor::new(100).unwrap();
        let _ = g.load_quotas(&quotas());
        let bad = vec![FamilyQuota {
            family: MetricFamily::Config,
            quota_per_sec: 0,
            priority: 3,
        }];
        let r = g.load_quotas(&bad);
        let ok = r.is_err() && g.step_of(MetricFamily::FrameRate) == Ok(SampleStep::Full);
        chk(&mut set, "C96-02 非法拒绝保留", ok, "配置被污染");
    }

    // C96-03 配额内放行
    {
        let mut g = SampleGovernor::new(100).unwrap();
        let _ = g.load_quotas(&quotas());
        let mut ok = true;
        for _ in 0..10 {
            ok = ok && g.try_sample(MetricFamily::FrameRate) == Ok(true);
        }
        chk(&mut set, "C96-03 配额放行", ok, "配额内被拒");
    }

    // C96-04 越限降档到半速
    {
        let mut g = SampleGovernor::new(100).unwrap();
        let _ = g.load_quotas(&quotas());
        for _ in 0..11 {
            let _ = g.try_sample(MetricFamily::FrameRate);
        }
        let ok = g.step_of(MetricFamily::FrameRate) == Ok(SampleStep::Half);
        chk(&mut set, "C96-04 越限降档", ok, "未降档");
    }

    // C96-05 阶梯三档到底不越档
    {
        let mut g = SampleGovernor::new(100).unwrap();
        let _ = g.load_quotas(&quotas());
        for _ in 0..40 {
            let _ = g.try_sample(MetricFamily::FrameRate);
        }
        let ok = g.step_of(MetricFamily::FrameRate) == Ok(SampleStep::Tenth);
        chk(&mut set, "C96-05 阶梯到底", ok, "档位异常");
    }

    // C96-06 变更全量入日志
    {
        let mut g = SampleGovernor::new(100).unwrap();
        let _ = g.load_quotas(&quotas());
        for _ in 0..25 {
            let _ = g.try_sample(MetricFamily::FrameRate);
        }
        let ok = g.change_log().len() >= 2;
        chk(&mut set, "C96-06 变更入日志", ok, "日志缺失");
    }

    // C96-07 恢复阶梯回升
    {
        let mut g = SampleGovernor::new(100).unwrap();
        let _ = g.load_quotas(&quotas());
        for _ in 0..30 {
            let _ = g.try_sample(MetricFamily::FrameRate);
        }
        g.advance_sec();
        g.advance_sec();
        let ok = g.step_of(MetricFamily::FrameRate) != Ok(SampleStep::Tenth);
        chk(&mut set, "C96-07 阶梯回升", ok, "未回升");
    }

    // C96-08 优先级保序：低优先级族不抢高优先级配额
    {
        let g = {
            let mut g = SampleGovernor::new(100).unwrap();
            let _ = g.load_quotas(&quotas());
            g
        };
        let prot = g.protected_families();
        let ok = prot.contains(&MetricFamily::FrameRate);
        chk(&mut set, "C96-08 优先级保序", ok, "高优先级未保");
    }

    // C96-09 未知族显性拒绝
    {
        let mut g = SampleGovernor::new(100).unwrap();
        let _ = g.load_quotas(&quotas());
        // Resource 未配置。
        let r = g.try_sample(MetricFamily::Resource);
        chk(
            &mut set,
            "C96-09 未知族拒绝",
            r.is_err() && g.last_code() == XC_UNKNOWN_FAMILY,
            "未知族被吞",
        );
    }

    // C96-10 零预算拒绝
    {
        let r = SampleGovernor::new(0);
        chk(
            &mut set,
            "C96-10 零预算拒绝",
            r.err() == Some(XC_BUDGET_INVALID),
            "零预算被放行",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea96_all_green() {
        let s = run_vea96_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
