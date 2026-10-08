//! 深化层三 · F139 反馈闭环通道（2026-09-26 深化批次三）。
//!
//! 补深 tracker 后端自建件面（主册 G-D-14 + 账本回炉扩列方向）：分带
//! 相似指纹（4 带命中 ≥3 判重复——SimHash 思路的整数实现）、优先级
//! 评分器（严重度×新鲜度×报告者信任）、SLA 分段时限钟、重复聚类
//! 升级器（同类报告越多优先级越高）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 分带相似指纹：payload 指纹切 4 带，两报告带命中 ≥3 → 同簇
// ---------------------------------------------------------------------------

/// 由 64 位 payload 指纹切出 4 个 16 位带。
pub fn bands(fp: u64) -> [u16; 4] {
    [
        (fp >> 48) as u16,
        ((fp >> 32) & 0xFFFF) as u16,
        ((fp >> 16) & 0xFFFF) as u16,
        (fp & 0xFFFF) as u16,
    ]
}

/// 相似度 = 带命中数（0..=4）；≥3 判重复报告。
pub fn similar(a: u64, b: u64) -> bool {
    let (ba, bb) = (bands(a), bands(b));
    let mut hit = 0;
    for i in 0..4 {
        if ba[i] == bb[i] {
            hit += 1;
        }
    }
    hit >= 3
}

/// 聚类器：报告指纹流 → 簇（首指纹为簇心）。
pub struct Clusterizer {
    /// (簇心指纹, 成员数)
    clusters: alloc::vec::Vec<(u64, u32)>,
}

impl Clusterizer {
    pub fn new() -> Clusterizer {
        Clusterizer { clusters: alloc::vec::Vec::new() }
    }

    /// 归簇：与某簇心相似 → 并入；否则自立新簇。
    pub fn admit(&mut self, fp: u64) -> usize {
        for (i, (center, count)) in self.clusters.iter_mut().enumerate() {
            if similar(*center, fp) {
                *count += 1;
                return i;
            }
        }
        self.clusters.push((fp, 1));
        self.clusters.len() - 1
    }

    pub fn cluster_count(&self) -> usize {
        self.clusters.len()
    }

    pub fn members(&self, idx: usize) -> Option<u32> {
        self.clusters.get(idx).map(|(_, c)| *c)
    }

    /// 簇成员数 ≥ 阈值 → 升级（重复报告即压力信号）。
    pub fn escalated(&self, idx: usize, threshold: u32) -> bool {
        self.members(idx).map(|c| c >= threshold).unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// 优先级评分：严重度(1-4)×100 + 新鲜度衰减 + 信任(0-9)×10
// ---------------------------------------------------------------------------

/// freshness：今日 - 报告日，7 日内满分 50，每过 7 日减 10，0 为底。
pub fn priority_score(severity: u32, report_day: u32, today: u32, trust: u32) -> u32 {
    let sev = severity.min(4) * 100;
    let age = today.saturating_sub(report_day);
    let fresh = 50u32.saturating_sub((age / 7) * 10);
    let tr = trust.min(9) * 10;
    sev + fresh + tr
}

/// 队列排序：分数降序，平分按报告日早者先（老报告不让饿死）。
pub fn queue_order(items: &[(u32, u32, u32, u32)]) -> alloc::vec::Vec<usize> {
    // items: (severity, report_day, today, trust)
    let mut idx: alloc::vec::Vec<usize> = (0..items.len()).collect();
    let key = |i: usize| -> (u32, u32) {
        let (s, d, t, tr) = items[i];
        (u32::MAX - priority_score(s, d, t, tr), d)
    };
    for i in 1..idx.len() {
        let k = idx[i];
        let mut j = i;
        while j > 0 && key(idx[j - 1]) > key(k) {
            idx[j] = idx[j - 1];
            j -= 1;
        }
        idx[j] = k;
    }
    idx
}

// ---------------------------------------------------------------------------
// SLA 分段时限钟：确认 48h(2d)/分诊 7d/修复 30d → 逾期检出
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SlaStage {
    Confirmed,
    Triaged,
    Fixed,
}

/// 返回 (阶段截止日, 是否逾期)。阶段未到的截止日如实返回。
pub fn sla_deadline(stage: SlaStage, report_day: u32) -> u32 {
    match stage {
        SlaStage::Confirmed => report_day + 2,
        SlaStage::Triaged => report_day + 7,
        SlaStage::Fixed => report_day + 30,
    }
}

pub fn sla_overdue(stage: SlaStage, report_day: u32, today: u32) -> bool {
    today > sla_deadline(stage, report_day)
}

/// 全链逾期体检：三段逐一给出结论（体验日志口径——可解释）。
pub fn sla_report(report_day: u32, today: u32) -> [bool; 3] {
    [
        sla_overdue(SlaStage::Confirmed, report_day, today),
        sla_overdue(SlaStage::Triaged, report_day, today),
        sla_overdue(SlaStage::Fixed, report_day, today),
    ]
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F139F_TAG: &str = "stareco-F139-deep3";

pub fn run_f139_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F139F_TAG);

    // 分带相似
    let fp_a = 0x1234_5678_9ABC_DEF0u64;
    let fp_b = 0x1234_5678_9ABC_00FFu64; // 3 带同
    let fp_c = 0xFFFF_0000_1111_2222u64; // 0 带同
    set.add("f139f similar 3 bands", similar(fp_a, fp_b), "三带命中判重");
    set.add("f139f dissimilar", !similar(fp_a, fp_c), "零带不判重");

    // 聚类
    let mut cl = Clusterizer::new();
    let c0 = cl.admit(fp_a);
    let c1 = cl.admit(fp_b);
    let c2 = cl.admit(fp_c);
    set.add("f139f cluster merge", c0 == c1, "相似并入同簇");
    set.add("f139f cluster new", c2 != c0, "不相似自立簇");
    set.add("f139f cluster count", cl.cluster_count() == 2, "两簇");
    set.add("f139f members", cl.members(c0) == Some(2), "簇成员计数");
    set.add("f139f escalate", cl.escalated(c0, 2) && !cl.escalated(c2, 2), "阈值升级");

    // 优先级
    let s_hot = priority_score(4, 100, 101, 9);
    let s_cold = priority_score(1, 100, 200, 0);
    set.add("f139f hot > cold", s_hot > s_cold, "高严重新鲜信任分高");
    set.add("f139f fresh decay", priority_score(2, 100, 107, 5) < priority_score(2, 100, 101, 5), "7 日衰减 10");
    set.add("f139f severity clamp", priority_score(9, 0, 0, 0) == priority_score(4, 0, 0, 0), "严重度钳制 4");
    let q = queue_order(&[(1, 10, 200, 0), (4, 10, 200, 9), (2, 5, 200, 5)]);
    set.add("f139f queue top", q[0] == 1, "4 级高信任居首");
    set.add("f139f queue last", q[2] == 0, "1 级陈旧沉底");

    // SLA
    set.add("f139f sla confirm", sla_deadline(SlaStage::Confirmed, 100) == 102, "确认 48h 线");
    set.add("f139f sla fixed", sla_deadline(SlaStage::Fixed, 100) == 130, "修复 30 天线");
    set.add("f139f overdue flag", sla_overdue(SlaStage::Triaged, 100, 108), "分诊逾期");
    set.add("f139f in time", !sla_overdue(SlaStage::Fixed, 100, 130), "30 天整不逾期");
    set.add(
        "f139f report shape",
        sla_report(100, 200) == [true, true, true],
        "远超期三段全红",
    );

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn band_identity() {
        let fp_a = 0x1234_5678_9ABC_DEF0u64;
        // 完全同指纹 4 带全中。
        assert!(similar(fp_a, fp_a));
        // 单带不同 → 3 带中 → 仍判重（口径如实：≥3）。
        let fp_d = 0x1234_5678_9ABC_DEF1u64;
        assert!(similar(fp_a, fp_d));
        // 两带不同 → 不判重。
        let fp_e = 0x1234_0000_0000_DEF0u64;
        assert!(!similar(fp_a, fp_e));
    }

    #[test]
    fn decay_floor() {
        // 衰减到底 50 天后仍保 0 新鲜分、不出现下溢。
        assert_eq!(priority_score(4, 0, 365, 0), 400);
        // 信任钳制 9：0 级+满新鲜+信任顶格 = 0+50+90。
        assert_eq!(priority_score(0, 0, 0, 99), 140);
    }
}
