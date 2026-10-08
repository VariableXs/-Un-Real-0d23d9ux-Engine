//! 深化层三 · F142 安全披露通道（2026-09-26 深化批次三）。
//!
//! 补深评级与时间线工程面（主册 G-D-17）：CVSS 思路简化评分核
//! （攻击途径/复杂度/影响三轴量化）、CVE 编号格式校验、披露时间线
//! 状态机（报告→确认→修复→披露逐段状态与逾期）、致谢页去重排序。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// CVSS 思路简化评分：三轴 → 0-10 分（登记面口径，非评级机构）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AttackVector {
    Network,
    Local,
    Physical,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Complexity {
    Low,
    High,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ImpactAxis {
    pub confidentiality: u8, // 0..2
    pub integrity: u8,
    pub availability: u8,
}

/// 评分公式（本系统登记口径）：基础 2.0 + 途径权重 + 复杂度权重 +
/// 影响合计×1.2，封顶 10.0（放大 10 倍取整口径——整数运算确定性）。
pub fn cvss_lite(v: AttackVector, c: Complexity, i: ImpactAxis) -> u32 {
    let vec_w = match v {
        AttackVector::Network => 30,
        AttackVector::Local => 15,
        AttackVector::Physical => 5,
    };
    let cpx_w = match c {
        Complexity::Low => 15,
        Complexity::High => 5,
    };
    let imp_sum = (i.confidentiality.min(2) + i.integrity.min(2) + i.availability.min(2)) as u32;
    let raw = 20 + vec_w + cpx_w + imp_sum * 12; // 满格: 20+30+15+72=137
    let scaled = raw * 100 / 137; // 0..=137 → 0..=100（10.0 满格）
    scaled.min(100)
}

// ---------------------------------------------------------------------------
// CVE 编号格式：CVE-YYYY-NNNN+（年 1999..=2099，序号 ≥4 位）
// ---------------------------------------------------------------------------

pub fn cve_id_ok(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("CVE-") else {
        return false;
    };
    let mut parts = rest.split('-');
    let (Some(y), Some(n), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    if y.len() != 4 || !y.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let year: u32 = match y.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !(1999..=2099).contains(&year) {
        return false;
    }
    n.len() >= 4 && n.bytes().all(|b| b.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// 披露时间线：报告→确认(48h)→修复(90d)→披露；逐段状态 + 逾期
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TlStage {
    Reported,
    Confirmed,
    Fixed,
    Disclosed,
}

pub struct TimelineState {
    pub stage: TlStage,
    pub report_day: u32,
    /// 各段完成日（None=未完成）。
    pub confirmed_day: Option<u32>,
    pub fixed_day: Option<u32>,
    pub disclosed_day: Option<u32>,
}

impl TimelineState {
    /// 当前段逾期判定：确认逾期 = 报告后 2 天未确认；修复逾期 =
    /// 确认后 90 天未修复；披露段不逾期（到期强制走披露）。
    pub fn overdue(&self, today: u32) -> Option<&'static str> {
        match self.stage {
            TlStage::Reported => {
                if today > self.report_day + 2 {
                    Some("确认逾期：报告 48h 未确认")
                } else {
                    None
                }
            }
            TlStage::Confirmed => {
                let base = self.confirmed_day.unwrap_or(self.report_day);
                if today > base + 90 {
                    Some("修复逾期：确认后 90 天未修复——到期强制披露预备")
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// 迁移合法性：单步前进 + 完成日必须 ≥ 报告日（时间线诚实）。
    pub fn advance(&mut self, to: TlStage, day: u32) -> Result<(), &'static str> {
        if day < self.report_day {
            return Err("完成日早于报告日：时间线倒置");
        }
        let legal = match (self.stage, to) {
            (TlStage::Reported, TlStage::Confirmed) => {
                self.confirmed_day = Some(day);
                true
            }
            (TlStage::Confirmed, TlStage::Fixed) => {
                self.fixed_day = Some(day);
                true
            }
            (TlStage::Fixed, TlStage::Disclosed) => {
                self.disclosed_day = Some(day);
                true
            }
            (TlStage::Reported, TlStage::Fixed) => {
                // 跳步拒绝——确认是义务段。
                false
            }
            _ => false,
        };
        if !legal {
            return Err("非法时间线迁移：跳步或倒退");
        }
        self.stage = to;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 致谢页：报告者去重（同人多报取首报日）、匿名尊重、按首报日排序
// ---------------------------------------------------------------------------

pub struct ThanksEntry {
    pub reporter: &'static str,
    pub first_report_day: u32,
    pub anonymous: bool,
}

/// 去重（同人保最早首报日）+ 按首报日升序。
pub fn thanks_page(entries: &[ThanksEntry]) -> alloc::vec::Vec<&'static str> {
    let mut uniq: alloc::vec::Vec<ThanksEntry> = alloc::vec::Vec::new();
    for e in entries {
        match uniq.iter_mut().find(|u| u.reporter == e.reporter) {
            Some(u) => {
                if e.first_report_day < u.first_report_day {
                    u.first_report_day = e.first_report_day;
                }
            }
            None => uniq.push(ThanksEntry { ..*e }),
        }
    }
    // 插入序按首报日。
    let mut order: alloc::vec::Vec<usize> = (0..uniq.len()).collect();
    for i in 1..order.len() {
        let k = order[i];
        let mut j = i;
        while j > 0 && uniq[order[j - 1]].first_report_day > uniq[k].first_report_day {
            order[j] = order[j - 1];
            j -= 1;
        }
        order[j] = k;
    }
    order
        .iter()
        .map(|i| {
            if uniq[*i].anonymous {
                "匿名报告者"
            } else {
                uniq[*i].reporter
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F142F_TAG: &str = "stareco-F142-deep3";

pub fn run_f142_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F142F_TAG);

    // 评分核
    let full = ImpactAxis { confidentiality: 2, integrity: 2, availability: 2 };
    let none = ImpactAxis { confidentiality: 0, integrity: 0, availability: 0 };
    let max = cvss_lite(AttackVector::Network, Complexity::Low, full);
    let min = cvss_lite(AttackVector::Physical, Complexity::High, none);
    set.add("f142f max", max == 100, "满格 10.0");
    set.add("f142f min", min < 25 && min > 0, "最低档弱分非零");
    set.add(
        "f142f order",
        cvss_lite(AttackVector::Network, Complexity::Low, full)
            > cvss_lite(AttackVector::Local, Complexity::Low, full),
        "网络途径高于本地",
    );
    set.add(
        "f142f axis clamp",
        cvss_lite(AttackVector::Network, Complexity::Low, ImpactAxis { confidentiality: 9, integrity: 9, availability: 9 })
            == max,
        "影响轴越界钳制",
    );

    // CVE 格式
    set.add("f142f cve ok", cve_id_ok("CVE-2026-0137"), "合规编号");
    set.add("f142f cve short num", !cve_id_ok("CVE-2026-137"), "序号 3 位拒绝");
    set.add("f142f cve bad year", !cve_id_ok("CVE-1998-1234"), "年份越界拒绝");
    set.add("f142f cve extra", !cve_id_ok("CVE-2026-1234-9"), "多段拒绝");
    set.add("f142f cve prefix", !cve_id_ok("GHI-2026-1234"), "前缀拒绝");

    // 时间线
    let mut tl = TimelineState {
        stage: TlStage::Reported,
        report_day: 100,
        confirmed_day: None,
        fixed_day: None,
        disclosed_day: None,
    };
    set.add("f142f confirm window", tl.overdue(102).is_none() && tl.overdue(103).is_some(), "48h 确认线");
    let _ = tl.advance(TlStage::Confirmed, 101);
    set.add("f142f fix window", tl.overdue(191).is_none() && tl.overdue(192).is_some(), "90 天修复线");
    set.add("f142f skip reject", tl.advance(TlStage::Disclosed, 120).is_err(), "修复未完成跳披露拒绝");
    let _ = tl.advance(TlStage::Fixed, 150);
    let _ = tl.advance(TlStage::Disclosed, 160);
    set.add("f142f full path", tl.stage == TlStage::Disclosed && tl.disclosed_day == Some(160), "全链走通");
    let mut t2 = TimelineState {
        stage: TlStage::Reported,
        report_day: 100,
        confirmed_day: None,
        fixed_day: None,
        disclosed_day: None,
    };
    set.add("f142f time invert", t2.advance(TlStage::Confirmed, 99).is_err(), "完成日早于报告拒绝");

    // 致谢页
    let entries = [
        ThanksEntry { reporter: "beta", first_report_day: 110, anonymous: false },
        ThanksEntry { reporter: "alpha", first_report_day: 105, anonymous: false },
        ThanksEntry { reporter: "beta", first_report_day: 108, anonymous: false },
        ThanksEntry { reporter: "gamma", first_report_day: 120, anonymous: true },
    ];
    let page = thanks_page(&entries);
    set.add(
        "f142f thanks order",
        page == alloc::vec!["alpha", "beta", "匿名报告者"],
        "去重+匿名尊重+首报日序",
    );

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn score_monotonic_impact() {
        let i1 = ImpactAxis { confidentiality: 1, integrity: 0, availability: 0 };
        let i2 = ImpactAxis { confidentiality: 2, integrity: 0, availability: 0 };
        assert!(cvss_lite(AttackVector::Local, Complexity::Low, i1) < cvss_lite(AttackVector::Local, Complexity::Low, i2));
    }

    #[test]
    fn cve_digits_only() {
        assert!(!cve_id_ok("CVE-2026-12a4"));
        assert!(!cve_id_ok(""));
        assert!(cve_id_ok("CVE-1999-0001"));
        assert!(cve_id_ok("CVE-2099-99999999"));
    }
}
