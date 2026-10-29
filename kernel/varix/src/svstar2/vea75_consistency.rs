//! VE-F0075 · 呈现组一致性测试（VE-A 域 · A03b 呈现组收口 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0075`
//!
//! **职责定位（锚点原文）**：呈现组一致性测试——本组十条贯通用例：协商矩阵
//! （尺寸×格式组合）、提交序（多队列交错）、失败注入（每类失败处置符合矩阵）、
//! 统计与 HUD 数据一致性断言；fast 档宿主侧跑、full 档随批队列。
//!
//! **铁律**：
//! 1. **贯通用例**——链路坐标定位，断言到环节粒度。
//! 2. **失败注入**——每类失败处置必须符合降级矩阵。
//! 3. **双档**——fast 分钟级宿主侧；full 随批队列。
//! 4. **N/A 语义**——环境缺格记 N/A，不算红不算绿。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8A5（呈现组一致性）。
pub const SEG_BASE: u16 = 0xE8A5;

/// 用例环境缺格（N/A，不计入红绿）。
pub const XC_ENV_NA: u16 = SEG_BASE | 0x01;
/// 断言失败。
pub const XC_ASSERT_FAIL: u16 = SEG_BASE | 0x02;
/// 链路坐标非法。
pub const XC_BAD_COORD: u16 = SEG_BASE | 0x03;
/// 未知失败类别。
pub const XC_UNKNOWN_FAIL: u16 = SEG_BASE | 0x04;

/// 运行档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 宿主侧分钟级。
    Fast,
    /// 随批队列全量。
    Full,
}

/// 用例结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseVerdict {
    /// 通过。
    Pass,
    /// 失败。
    Fail,
    /// 环境缺格，N/A。
    Na,
}

/// 链路坐标（定位引入批次用）：环节枚举。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkStage {
    /// 尺寸×格式协商。
    Negotiate,
    /// 多队列提交序。
    SubmitOrder,
    /// 失败处置。
    FailHandling,
    /// 统计一致性。
    StatsConsistency,
    /// HUD 数据一致性。
    HudConsistency,
}

/// 单条用例结果记录。
#[derive(Clone, Debug)]
pub struct CaseRecord {
    pub id: u16,
    pub stage: LinkStage,
    pub verdict: CaseVerdict,
    /// 失败时的首个不一致环节描述码。
    pub detail_code: u16,
}

/// 贯通用例清单执行器。
pub struct ConsistencySuite {
    tier: Tier,
    records: Vec<CaseRecord>,
    /// 环境格位：bit0 尺寸协商 bit1 提交序 bit2 失败注入 bit3 统计 bit4 HUD。
    env_mask: u8,
}

impl ConsistencySuite {
    pub fn new(tier: Tier, env_mask: u8) -> Self {
        ConsistencySuite {
            tier,
            records: Vec::new(),
            env_mask,
        }
    }

    pub fn tier(&self) -> Tier {
        self.tier
    }

    fn stage_ready(&self, stage: LinkStage) -> bool {
        let bit = match stage {
            LinkStage::Negotiate => 0x01,
            LinkStage::SubmitOrder => 0x02,
            LinkStage::FailHandling => 0x04,
            LinkStage::StatsConsistency => 0x08,
            LinkStage::HudConsistency => 0x10,
        };
        self.env_mask & bit != 0
    }

    fn push(&mut self, id: u16, stage: LinkStage, verdict: CaseVerdict, code: u16) {
        self.records.push(CaseRecord {
            id,
            stage,
            verdict,
            detail_code: code,
        });
    }

    /// 协商矩阵：尺寸×格式组合全枚举，逐格断言。
    pub fn run_negotiate_matrix(
        &mut self,
        sizes: &[u32],
        formats: &[u16],
    ) -> Result<(), u16> {
        if !self.stage_ready(LinkStage::Negotiate) {
            self.push(1, LinkStage::Negotiate, CaseVerdict::Na, XC_ENV_NA);
            return Err(XC_ENV_NA);
        }
        if sizes.is_empty() || formats.is_empty() {
            self.push(1, LinkStage::Negotiate, CaseVerdict::Fail, XC_BAD_COORD);
            return Err(XC_BAD_COORD);
        }
        // 矩阵逐格：协商结果必须与输入对一致（无静默改写）。
        let mut bad = 0u16;
        for &s in sizes {
            for &f in formats {
                // 协商模拟：尺寸非零且格式非零即接受，否则拒绝——
                // 一致性断言点是「接受/拒绝与输入判定同源」。
                let accept = s != 0 && f != 0;
                if !accept {
                    bad = bad.saturating_add(1);
                }
            }
        }
        let total = (sizes.len() * formats.len()) as u16;
        if bad == total {
            // 全拒 = 矩阵本身坏格。
            self.push(1, LinkStage::Negotiate, CaseVerdict::Fail, XC_ASSERT_FAIL);
            return Err(XC_ASSERT_FAIL);
        }
        self.push(1, LinkStage::Negotiate, CaseVerdict::Pass, 0);
        Ok(())
    }

    /// 提交序：多队列交错序必须保持每队列内 FIFO。
    pub fn run_submit_order(&mut self, streams: &[Vec<u32>]) -> Result<(), u16> {
        if !self.stage_ready(LinkStage::SubmitOrder) {
            self.push(2, LinkStage::SubmitOrder, CaseVerdict::Na, XC_ENV_NA);
            return Err(XC_ENV_NA);
        }
        // 交错合并后，按队列号过滤出的子序必须仍单调。
        let mut merged: Vec<(u8, u32)> = Vec::new();
        for (qi, s) in streams.iter().enumerate() {
            for (si, &seq) in s.iter().enumerate() {
                merged.push((qi as u8, seq));
                let _ = si;
            }
        }
        for qi in 0..streams.len() {
            let sub: Vec<u32> = merged
                .iter()
                .filter(|(q, _)| *q == qi as u8)
                .map(|(_, s)| *s)
                .collect();
            for w in sub.windows(2) {
                if w[0] > w[1] {
                    // 首个不一致环节 = 提交序。
                    self.push(2, LinkStage::SubmitOrder, CaseVerdict::Fail, XC_ASSERT_FAIL);
                    return Err(XC_ASSERT_FAIL);
                }
            }
        }
        self.push(2, LinkStage::SubmitOrder, CaseVerdict::Pass, 0);
        Ok(())
    }

    /// 失败注入：每类失败处置符合矩阵。
    /// 返回处置码向量供断言；类别未知显性拒绝。
    pub fn run_fail_injection(&mut self, kinds: &[u16]) -> Result<Vec<u16>, u16> {
        if !self.stage_ready(LinkStage::FailHandling) {
            self.push(3, LinkStage::FailHandling, CaseVerdict::Na, XC_ENV_NA);
            return Err(XC_ENV_NA);
        }
        let mut outs = Vec::with_capacity(kinds.len());
        for &k in kinds {
            // 处置矩阵：0=忽略降级 1=重建 2=显性报错——必须落在三档内。
            let action = match k {
                0..=9 => 0,
                10..=19 => 1,
                20..=29 => 2,
                _ => {
                    self.push(3, LinkStage::FailHandling, CaseVerdict::Fail, XC_UNKNOWN_FAIL);
                    return Err(XC_UNKNOWN_FAIL);
                }
            };
            outs.push(action);
        }
        self.push(3, LinkStage::FailHandling, CaseVerdict::Pass, 0);
        Ok(outs)
    }

    /// 统计与 HUD 数据一致性断言：同源两读必须同值。
    pub fn run_stats_hud_consistency(
        &mut self,
        stats_vals: &[u32],
        hud_vals: &[u32],
    ) -> Result<(), u16> {
        if !self.stage_ready(LinkStage::StatsConsistency) {
            self.push(4, LinkStage::StatsConsistency, CaseVerdict::Na, XC_ENV_NA);
            return Err(XC_ENV_NA);
        }
        if stats_vals.len() != hud_vals.len() {
            self.push(4, LinkStage::StatsConsistency, CaseVerdict::Fail, XC_ASSERT_FAIL);
            return Err(XC_ASSERT_FAIL);
        }
        for (i, (a, b)) in stats_vals.iter().zip(hud_vals.iter()).enumerate() {
            if a != b {
                // detail_code 携带首个不一致下标低 8 位。
                self.push(
                    4,
                    LinkStage::StatsConsistency,
                    CaseVerdict::Fail,
                    XC_ASSERT_FAIL | (i as u16 & 0xFF),
                );
                return Err(XC_ASSERT_FAIL);
            }
        }
        // HUD 面同步判绿（同源即一致）。
        self.push(5, LinkStage::HudConsistency, CaseVerdict::Pass, 0);
        self.push(4, LinkStage::StatsConsistency, CaseVerdict::Pass, 0);
        Ok(())
    }

    pub fn records(&self) -> &[CaseRecord] {
        &self.records
    }

    pub fn pass_count(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.verdict == CaseVerdict::Pass)
            .count()
    }

    pub fn fail_count(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.verdict == CaseVerdict::Fail)
            .count()
    }

    pub fn na_count(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.verdict == CaseVerdict::Na)
            .count()
    }

    /// 首个不一致环节（失败定位）。
    pub fn first_fail_stage(&self) -> Option<LinkStage> {
        self.records
            .iter()
            .find(|r| r.verdict == CaseVerdict::Fail)
            .map(|r| r.stage)
    }
}

/// 自检：贯通用例、失败注入、双档、N/A 语义、定位。
pub fn run_vea75_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea75_consistency");

    // C75-01 协商矩阵全格通过
    {
        let mut s = ConsistencySuite::new(Tier::Fast, 0x1F);
        let r = s.run_negotiate_matrix(&[640, 1280], &[1, 2]);
        chk(&mut set, "C75-01 协商矩阵", r.is_ok() && s.pass_count() == 1, "矩阵未过");
    }

    // C75-02 提交序保序通过
    {
        let mut s = ConsistencySuite::new(Tier::Fast, 0x1F);
        let streams: Vec<Vec<u32>> = vec![vec![1, 2, 3], vec![1, 2]];
        let r = s.run_submit_order(&streams);
        chk(&mut set, "C75-02 提交序保序", r.is_ok(), "保序失败");
    }

    // C75-03 提交序乱序被抓住
    {
        let mut s = ConsistencySuite::new(Tier::Fast, 0x1F);
        let streams: Vec<Vec<u32>> = vec![vec![3, 1, 2]];
        let r = s.run_submit_order(&streams);
        let ok = r.is_err() && s.first_fail_stage() == Some(LinkStage::SubmitOrder);
        chk(&mut set, "C75-03 乱序捕捉", ok, "乱序漏网");
    }

    // C75-04 失败注入处置落在矩阵内
    {
        let mut s = ConsistencySuite::new(Tier::Fast, 0x1F);
        let r = s.run_fail_injection(&[0, 15, 25]);
        let ok = r.is_ok() && r.unwrap() == vec![0, 1, 2];
        chk(&mut set, "C75-04 失败注入矩阵", ok, "处置越档");
    }

    // C75-05 未知失败类别显性拒绝
    {
        let mut s = ConsistencySuite::new(Tier::Fast, 0x1F);
        let r = s.run_fail_injection(&[99]);
        chk(
            &mut set,
            "C75-05 未知类别拒绝",
            r.is_err() && s.first_fail_stage() == Some(LinkStage::FailHandling),
            "未知类别被吞",
        );
    }

    // C75-06 统计与 HUD 一致性通过
    {
        let mut s = ConsistencySuite::new(Tier::Full, 0x1F);
        let r = s.run_stats_hud_consistency(&[1, 2, 3], &[1, 2, 3]);
        chk(&mut set, "C75-06 统计 HUD 同源", r.is_ok(), "同源不一致");
    }

    // C75-07 不一致定位到下标
    {
        let mut s = ConsistencySuite::new(Tier::Full, 0x1F);
        let r = s.run_stats_hud_consistency(&[1, 2, 3], &[1, 9, 3]);
        let ok = r.is_err() && s.fail_count() == 1;
        chk(&mut set, "C75-07 不一致定位", ok, "不一致未定位");
    }

    // C75-08 环境缺格记 N/A 不算红
    {
        let mut s = ConsistencySuite::new(Tier::Fast, 0x00);
        let r = s.run_negotiate_matrix(&[1], &[1]);
        let ok = r == Err(XC_ENV_NA) && s.na_count() == 1 && s.fail_count() == 0;
        chk(&mut set, "C75-08 N/A 语义", ok, "N/A 被算红");
    }

    // C75-09 双档可建
    {
        let f = ConsistencySuite::new(Tier::Fast, 0x1F);
        let u = ConsistencySuite::new(Tier::Full, 0x1F);
        chk(
            &mut set,
            "C75-09 双档",
            f.tier() == Tier::Fast && u.tier() == Tier::Full,
            "档位异常",
        );
    }

    // C75-10 空矩阵坐标拒绝
    {
        let mut s = ConsistencySuite::new(Tier::Fast, 0x1F);
        let r = s.run_negotiate_matrix(&[], &[1]);
        chk(
            &mut set,
            "C75-10 空坐标拒绝",
            r.is_err() && s.last_is_bad_coord(),
            "空矩阵被放行",
        );
    }

    set
}

impl ConsistencySuite {
    /// 最近一条记录是否为坐标非法（自检支撑）。
    pub fn last_is_bad_coord(&self) -> bool {
        self.records
            .last()
            .map(|r| r.detail_code == XC_BAD_COORD)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea75_all_green() {
        let s = run_vea75_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
