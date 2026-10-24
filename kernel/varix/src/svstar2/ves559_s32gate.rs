//! VE-F4325 · 一致性回归门与看板（U07 性能与预算组）
//! 职责：性能回归门与看板——回归门（基准结果对比上版——劣化超阈即拒
//! 合并；环境指纹排除后重判防门误拒）+ 看板三视图（趋势 / 分档 / 预算
//! 热力）+ 门禁豁免（带时效 + 仲裁批准同 F4271 口径——豁免滥用频次收紧）
//! + 看板断流置灰标注 + 三视图附数据表（域本色）。
//! 段位：0x3460（续表 idx559）。行数预算 360。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3460;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RgCode {
    /// 劣化超阈（拒合并）。
    Regression = SEG_BASE,
    /// 环境漂移（排除后重判）。
    EnvDrift = SEG_BASE + 0x01,
    /// 豁免过期（拒绝沿用）。
    ExpiryPassed = SEG_BASE + 0x02,
    /// 看板断流（置灰标注）。
    BoardStalled = SEG_BASE + 0x03,
}

/// 回归门判定。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateVerdict {
    /// 劣化率 bp（相对上版）。
    pub degrade_bp: u32,
    /// 是否拒合并。
    pub reject: bool,
    /// 是否因环境漂移重判。
    pub rejudged: bool,
}

/// 豁免单（带时效 + 仲裁批准）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exemption {
    pub id: u32,
    pub granted_at: u64,
    pub ttl: u64,
    pub arb_approved: bool,
}

/// 看板点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoardPoint {
    pub t: u64,
    pub ms: u32,
}

/// 回归门台。
#[derive(Default)]
pub struct RegGate {
    /// 劣化容忍阈（bp）。
    pub tolerance_bp: u32,
    pub exemptions: Vec<Exemption>,
    pub trend: Vec<BoardPoint>,
    pub env_fp: u32,
    now: u64,
    pub stalls: u32,
    pub tightened: u32,
}

impl RegGate {
    pub fn new(tol_bp: u32, env_fp: u32) -> Self {
        RegGate { tolerance_bp: tol_bp.max(1), env_fp, ..Default::default() }
    }

    /// 基线回归判定：劣化率 = (now − base) / base。
    pub fn judge(&self, base_ms: u32, now_ms: u32) -> GateVerdict {
        let bp = if base_ms == 0 {
            if now_ms == 0 { 0 } else { u32::MAX / 2 }
        } else if now_ms > base_ms {
            ((now_ms - base_ms) as u64 * 10000 / base_ms as u64) as u32
        } else {
            0
        };
        GateVerdict { degrade_bp: bp, reject: bp > self.tolerance_bp, rejudged: false }
    }

    /// 环境排除重判：指纹漂移 → 剔环境因素后按上版同环境折算重判。
    pub fn judge_env(&self, base_ms: u32, now_ms: u32, now_fp: u32, env_coef_bp: u32) -> GateVerdict {
        if now_fp == self.env_fp {
            return self.judge(base_ms, now_ms);
        }
        // 排除环境差异：折算到基准环境再判。
        let adjusted = now_ms as u64 * env_coef_bp as u64 / 10000;
        let mut v = self.judge(base_ms, adjusted as u32);
        v.rejudged = true;
        v
    }

    /// 豁免发放（须仲裁批准；未批拒绝）。
    pub fn grant(&mut self, id: u32, ttl: u64, arb: bool) -> Result<(), RgCode> {
        if !arb {
            return Err(RgCode::ExpiryPassed);
        }
        self.exemptions.push(Exemption { id, granted_at: self.now, ttl, arb_approved: true });
        Ok(())
    }

    /// 豁免有效核验（时效内；过期失效）。
    pub fn exempt(&self, id: u32) -> Result<bool, RgCode> {
        let e = self.exemptions.iter().find(|e| e.id == id).ok_or(RgCode::ExpiryPassed)?;
        Ok(self.now < e.granted_at + e.ttl)
    }

    /// 豁免滥用收紧：同 id 续期第三次起阈值减半（返回新容忍）。
    pub fn tighten(&mut self, id: u32) -> u32 {
        let cnt = self.exemptions.iter().filter(|e| e.id == id).count();
        if cnt >= 3 {
            self.tolerance_bp /= 2;
            self.tightened += 1;
        }
        self.tolerance_bp
    }

    /// 趋势点入板。
    pub fn push_point(&mut self, t: u64, ms: u32) {
        self.now = self.now.max(t);
        self.trend.push(BoardPoint { t, ms });
    }

    /// 时钟推进（不落板——用于断流检测的时间源）。
    pub fn advance(&mut self, dt: u64) {
        self.now += dt;
    }

    /// 断流检测：最后点距 now 超窗 = 断流（置灰标注计数）。
    pub fn stalled(&mut self, window: u64) -> bool {
        let stale = self.trend.last().map(|p| self.now >= p.t + window).unwrap_or(true);
        if stale {
            self.stalls += 1;
        }
        stale
    }

    /// 分档视图：趋势均值按档位映射（快/正常/慢）。
    pub fn tier_view(&self, budget: u32) -> (&'static str, u32) {
        let n = self.trend.len() as u64;
        if n == 0 {
            return ("无样", 0);
        }
        let sum: u64 = self.trend.iter().map(|p| p.ms as u64).sum();
        let avg = (sum / n) as u32;
        let label = if avg <= budget { "快" } else if avg <= budget * 2 { "正常" } else { "慢" };
        (label, avg)
    }

    /// 热力视图：劣化程度归一 0-9（0 最冷）。
    pub fn heat(&self) -> u8 {
        let (_, avg) = self.tier_view(100);
        ((avg / 25).min(9)) as u8
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves559_checks() -> CheckSet {
    let mut set = CheckSet::new("ves559");
    let mut rg = RegGate::new(1000, 0xE0);

    // C559-01 预算内过门。
    let v = rg.judge(100, 105);
    chk(&mut set, "C559-01", v.degrade_bp == 500 && !v.reject, "劣化容忍内过门");

    // C559-02 劣化超阈拒合并。
    let v2 = rg.judge(100, 115);
    chk(&mut set, "C559-02", v2.reject, "超阈拒合并");

    // C559-03 环境排除重判（折算后合格放行 + rejudged 标注）。
    let v3 = rg.judge_env(100, 200, 0xF0, 5000); // 折算 100ms
    chk(&mut set, "C559-03", v3.rejudged && !v3.reject, "环境排除重判");

    // C559-04 同指纹直判不重判。
    let v4 = rg.judge_env(100, 105, 0xE0, 5000);
    chk(&mut set, "C559-04", !v4.rejudged && !v4.reject, "同指纹直判");

    // C559-05 豁免须仲裁批准。
    chk(&mut set, "C559-05", rg.grant(1, 100, false).is_err() && rg.exemptions.is_empty(), "未批不豁免");

    // C559-06 批准后豁免时效内核验通过。
    chk(&mut set, "C559-06", rg.grant(1, 100, true).is_ok() && rg.exempt(1) == Ok(true), "豁免时效内");

    // C559-07 豁免过期失效。
    rg.advance(101);
    chk(&mut set, "C559-07", rg.exempt(1) == Ok(false), "豁免过期失效");

    // C559-08 豁免滥用收紧（三次续期容忍减半）。
    let _ = rg.grant(2, 10, true);
    let _ = rg.grant(2, 10, true);
    let t0 = rg.tolerance_bp;
    let _ = rg.grant(2, 10, true);
    let t1 = rg.tighten(2);
    chk(&mut set, "C559-08", t0 == 1000 && t1 == 500 && rg.tightened == 1, "滥用收紧减半");

    // C559-09 趋势入板与分档视图。
    rg.push_point(1, 100);
    rg.push_point(2, 100);
    let (label, avg) = rg.tier_view(100);
    chk(&mut set, "C559-09", !label.is_empty() && avg > 0, "趋势分档视图");

    // C559-10 断流置灰检测（时钟推进不落板 → 超窗置灰计数）。
    rg.push_point(rg.now, 90); // 落板一点（last.t 追上 now）
    let s1 = rg.stalled(50);
    rg.advance(100);
    let s2 = rg.stalled(50);
    chk(&mut set, "C559-10", !s1 && s2 && rg.stalls == 1, "断流置灰计数");

    // C559-11 热力归一 0-9。
    let h = rg.heat();
    chk(&mut set, "C559-11", h <= 9, "热力归一有界");

    // C559-12 空板断流恒真 + 分档无样不臆测。
    let mut rg2 = RegGate::new(500, 1);
    let (l2, a2) = rg2.tier_view(100);
    chk(&mut set, "C559-12", rg2.stalled(10) && l2 == "无样" && a2 == 0, "空板不臆测");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_thresholds() {
        let rg = RegGate::new(500, 1);
        assert!(!rg.judge(100, 104).reject);
        assert!(rg.judge(100, 106).reject);
        assert_eq!(rg.judge(0, 0).degrade_bp, 0);
        assert!(rg.judge(0, 1).reject); // 零基线非零实测 = 最大嫌疑
    }

    #[test]
    fn exemption_ttl() {
        let mut rg = RegGate::new(500, 1);
        assert!(rg.grant(1, 10, true).is_ok());
        assert_eq!(rg.exempt(1), Ok(true));
        assert_eq!(rg.exempt(2), Err(RgCode::ExpiryPassed));
        for _ in 0..11u64 {
            rg.push_point(rg.now + 1, 1);
        }
        assert_eq!(rg.exempt(1), Ok(false));
    }

    #[test]
    fn stall_detect() {
        let mut rg = RegGate::new(500, 1);
        assert!(rg.stalled(10)); // 空板 = 断流
        rg.push_point(5, 10);
        assert!(!rg.stalled(10));
        rg.advance(100);
        assert!(rg.stalled(10));
    }
}
