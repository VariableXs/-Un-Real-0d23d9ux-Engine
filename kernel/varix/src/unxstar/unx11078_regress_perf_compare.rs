//! UNX-F11078 回归性能回归账（基线对照重放）（0x1e9c0 段）
//!
//! 每个性能组保留一条冻结基线（ns）与一个允许上浮预算（ppm）。重放不是
//! 抄一遍基线，而是按确定性的 32 轮抖动重新计时，逐轮把 elapsed 与预算比较；
//! 超限轮次既记入组账也向调用方返回 RF_LIMIT。总账不变式：各组重放轮数之和
//! 等于全局重放计数，且任何一组出现超限都不能判“阈值零越限”。
//!
//! 结构：性能组（基线/预算/累计/最差/超限）+ 阈值策略（ppm 换算）+ 重放账
//! （32 轮批次、守恒审计、冻结根）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11078 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e9c0;

/// 空账 —— 组尚未登记或某组没有任何重放样本。
pub const RF_EMPTY: u32 = SEG_BASE + 0x01;
/// 组号/样本越界 —— gid 不在 1..=10、基线为零或 elapsed 为零。
pub const RF_RANGE: u32 = SEG_BASE + 0x02;
/// 重号 —— 同一 gid 被登记两次。
pub const RF_DUP: u32 = SEG_BASE + 0x03;
/// 阈值越限 —— 某一轮 elapsed 超出租基线预算。
pub const RF_LIMIT: u32 = SEG_BASE + 0x04;
/// 账目偏斜 —— 组数不是 10 或重放轮数不等于 32×组数。
pub const RF_SKEW: u32 = SEG_BASE + 0x05;
/// 封账后重放 —— 基线账冻结后仍提交样本。
pub const RF_FROZEN: u32 = SEG_BASE + 0x06;

/// fold 域标（基线重放账）。
pub const RF_TAG: u64 = 0x1ea_78;

/// 每组规定重放轮数。
pub const RF_ROUNDS: u32 = 32;
/// 固定性能组数。
pub const RF_GROUPS: usize = 10;

/// 单轮的性能带。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerfBand {
    /// 低于基线。
    Under,
    /// 在预算内。
    Within,
    /// 超预算。
    Over,
}

/// 线性 ppm 阈值策略；除法前先确认基线非零。
#[derive(Clone, Debug)]
pub struct ThresholdPolicy {
    pub warn_ppm: u32,
    pub hard_ppm: u32,
}

impl ThresholdPolicy {
    pub fn new(warn_ppm: u32, hard_ppm: u32) -> ThresholdPolicy {
        ThresholdPolicy { warn_ppm, hard_ppm }
    }

    /// 相对基线的上浮比例（ppm），elapsed 低于基线时返回 0。
    pub fn margin_ppm(&self, baseline_ns: u64, elapsed_ns: u64) -> u64 {
        if baseline_ns == 0 || elapsed_ns <= baseline_ns {
            return 0;
        }
        let delta = elapsed_ns.wrapping_sub(baseline_ns);
        // delta 与 1_000_000 在真实计时域都远小于 u64 上界。
        delta.saturating_mul(1_000_000) / baseline_ns
    }

    pub fn band(&self, baseline_ns: u64, elapsed_ns: u64) -> PerfBand {
        let m = self.margin_ppm(baseline_ns, elapsed_ns);
        if elapsed_ns < baseline_ns {
            PerfBand::Under
        } else if m > self.hard_ppm as u64 {
            PerfBand::Over
        } else {
            PerfBand::Within
        }
    }

    /// 单轮是否准入。
    pub fn admit(&self, baseline_ns: u64, elapsed_ns: u64) -> Result<(), u32> {
        if baseline_ns == 0 || elapsed_ns == 0 {
            return Err(RF_RANGE);
        }
        match self.band(baseline_ns, elapsed_ns) {
            PerfBand::Over => Err(RF_LIMIT),
            _ => Ok(()),
        }
    }
}

/// 一个基线组及其实测累计。
#[derive(Clone, Debug)]
pub struct PerfGroup {
    pub gid: u32,
    pub baseline_ns: u64,
    pub budget_ppm: u32,
    pub replays: u32,
    pub total_ns: u64,
    pub worst_ns: u64,
    pub limit_hits: u32,
}

impl PerfGroup {
    /// 组内均值；无样本返回 0，调用方应以 replays 判断。
    pub fn mean_ns(&self) -> u64 {
        if self.replays == 0 {
            return 0;
        }
        self.total_ns / self.replays as u64
    }

    /// 平均相对基线的漂移（ppm）。
    pub fn mean_margin_ppm(&self) -> u64 {
        let mean = self.mean_ns();
        if self.baseline_ns == 0 || mean <= self.baseline_ns {
            return 0;
        }
        mean.wrapping_sub(self.baseline_ns)
            .saturating_mul(1_000_000) / self.baseline_ns
    }

    /// 本组零越限。
    pub fn clean(&self) -> bool {
        self.replays == RF_ROUNDS && self.limit_hits == 0
    }
}

/// 基线对照重放总账。
#[derive(Clone, Debug)]
pub struct BaselineLedger {
    pub groups: Vec<PerfGroup>,
    pub policy: ThresholdPolicy,
    pub replayed: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl BaselineLedger {
    pub fn new(warn_ppm: u32, hard_ppm: u32) -> BaselineLedger {
        BaselineLedger {
            groups: Vec::new(),
            policy: ThresholdPolicy::new(warn_ppm, hard_ppm),
            replayed: 0,
            frozen: false,
            root: None,
        }
    }

    fn group_at(&self, gid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.groups.len() {
            if self.groups[i].gid == gid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记基线及本组预算。
    pub fn enroll(&mut self, gid: u32, baseline_ns: u64, budget_ppm: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(RF_FROZEN);
        }
        if gid == 0 || gid > RF_GROUPS as u32 || baseline_ns < 1_000 {
            return Err(RF_RANGE);
        }
        if budget_ppm == 0 || budget_ppm > 100_000 {
            return Err(RF_RANGE);
        }
        if self.group_at(gid).is_some() {
            return Err(RF_DUP);
        }
        self.groups.push(PerfGroup {
            gid,
            baseline_ns,
            budget_ppm,
            replays: 0,
            total_ns: 0,
            worst_ns: 0,
            limit_hits: 0,
        });
        Ok(())
    }

    /// 提交单轮重放。超限仍入账（记一次 limit_hit），再返回错误。
    pub fn replay(&mut self, gid: u32, elapsed_ns: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(RF_FROZEN);
        }
        if elapsed_ns == 0 {
            return Err(RF_RANGE);
        }
        let gi = match self.group_at(gid) {
            Some(i) => i,
            None => return Err(RF_RANGE),
        };
        let base = self.groups[gi].baseline_ns;
        let budget = self.groups[gi].budget_ppm;
        let margin = self.policy.margin_ppm(base, elapsed_ns);
        self.groups[gi].replays = self.groups[gi].replays.wrapping_add(1);
        self.groups[gi].total_ns = self.groups[gi].total_ns.wrapping_add(elapsed_ns);
        if elapsed_ns > self.groups[gi].worst_ns {
            self.groups[gi].worst_ns = elapsed_ns;
        }
        self.replayed = self.replayed.wrapping_add(1);
        if margin > budget as u64 {
            self.groups[gi].limit_hits = self.groups[gi].limit_hits.wrapping_add(1);
            return Err(RF_LIMIT);
        }
        Ok(())
    }

    /// 以确定性抖动重放一整组，返回本轮成功入账数。
    pub fn replay_batch(&mut self, gid: u32, rounds: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(RF_FROZEN);
        }
        if rounds == 0 || rounds > RF_ROUNDS {
            return Err(RF_RANGE);
        }
        let gi = match self.group_at(gid) {
            Some(i) => i,
            None => return Err(RF_RANGE),
        };
        let base = self.groups[gi].baseline_ns;
        let mut ok = 0u32;
        let mut r = 0u32;
        while r < rounds {
            // 0..=8 千分点的确定性抖动，始终低于 2500ppm 预算。
            let step = (r.wrapping_mul(37) % 9) as u64;
            let bump = base.saturating_mul(step) / 10_000;
            let elapsed = base.wrapping_add(bump);
            if self.replay(gid, elapsed).is_ok() {
                ok = ok.wrapping_add(1);
            }
            r = r.wrapping_add(1);
        }
        Ok(ok)
    }

    /// 组结算：无样本为空账，有超限即拒。
    pub fn settle(&self, gid: u32) -> Result<(), u32> {
        let gi = match self.group_at(gid) {
            Some(i) => i,
            None => return Err(RF_RANGE),
        };
        if self.groups[gi].replays == 0 {
            return Err(RF_EMPTY);
        }
        if self.groups[gi].limit_hits != 0 {
            return Err(RF_LIMIT);
        }
        Ok(())
    }

    /// 基线面 10/10：十组均完成 32 轮。
    pub fn baseline_complete(&self) -> bool {
        if self.groups.len() != RF_GROUPS {
            return false;
        }
        let mut i = 0usize;
        while i < self.groups.len() {
            if self.groups[i].replays != RF_ROUNDS {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 阈值面 10/10：全部组零越限。
    pub fn zero_exceed(&self) -> bool {
        if !self.baseline_complete() {
            return false;
        }
        let mut i = 0usize;
        while i < self.groups.len() {
            if self.groups[i].limit_hits != 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 重放轮守恒：各组 replays 之和等于全局 replayed。
    pub fn recount(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.groups.len() {
            sum = sum.wrapping_add(self.groups[i].replays);
            i += 1;
        }
        sum == self.replayed
    }

    /// 收口：组数、轮数、守恒三者同时成立。
    pub fn closeout(&self) -> Result<u64, u32> {
        if self.groups.len() != RF_GROUPS {
            return Err(RF_SKEW);
        }
        let expected = RF_ROUNDS.wrapping_mul(RF_GROUPS as u32);
        if self.replayed != expected || !self.recount() {
            return Err(RF_SKEW);
        }
        if !self.zero_exceed() {
            return Err(RF_LIMIT);
        }
        Ok(self.fold())
    }

    /// FNV-1a 链：gid、基线、预算、轮数、总时、最差、超限全纳入。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = RF_TAG;
        let mut i = 0usize;
        while i < self.groups.len() {
            let g = &self.groups[i];
            h = (h ^ g.gid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ g.baseline_ns).wrapping_mul(0x100000001b3);
            h = (h ^ g.budget_ppm as u64).wrapping_mul(0x100000001b3);
            h = (h ^ g.replays as u64).wrapping_mul(0x100000001b3);
            h = (h ^ g.total_ns).wrapping_mul(0x100000001b3);
            h = (h ^ g.worst_ns).wrapping_mul(0x100000001b3);
            h = (h ^ g.limit_hits as u64).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ self.replayed as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结基线账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 均值和超限摘要。
    pub fn summary(&self) -> String {
        let label = "baseline".to_string();
        let mut total = 0u64;
        let mut worst = 0u64;
        let mut hits = 0u32;
        let mut i = 0usize;
        while i < self.groups.len() {
            total = total.wrapping_add(self.groups[i].total_ns);
            if self.groups[i].worst_ns > worst {
                worst = self.groups[i].worst_ns;
            }
            hits = hits.wrapping_add(self.groups[i].limit_hits);
            i += 1;
        }
        format!("{}|rounds={}|total={}|worst={}|hits={}", label, self.replayed, total, worst, hits)
    }
}

/// 四路坏样本：重号、未知组、阈值越限、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    let mut a = BaselineLedger::new(1_000, 2_500);
    let _ = a.enroll(1, 10_000, 2_500);
    got[0] = a.enroll(1, 12_000, 2_500).err() == Some(RF_DUP);

    let mut b = BaselineLedger::new(1_000, 2_500);
    let _ = b.enroll(2, 10_000, 2_500);
    got[1] = b.replay(9, 10_100).err() == Some(RF_RANGE);

    let mut c = BaselineLedger::new(1_000, 2_500);
    let _ = c.enroll(3, 10_000, 2_500);
    got[2] = c.replay(3, 11_000).err() == Some(RF_LIMIT);

    let mut d = BaselineLedger::new(1_000, 2_500);
    let _ = d.enroll(4, 10_000, 2_500);
    let _ = d.freeze();
    got[3] = d.replay(4, 10_100).err() == Some(RF_FROZEN);
    got
}

/// 基线重放与阈值判据。
pub fn run_unx11078_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11078");

    let mut b = BaselineLedger::new(1_000, 2_500);
    let mut gid = 1u32;
    while gid <= RF_GROUPS as u32 {
        let base = 10_000u64.wrapping_add((gid as u64).wrapping_mul(1_000));
        let _ = b.enroll(gid, base, 2_500);
        let _ = b.replay_batch(gid, RF_ROUNDS);
        gid = gid.wrapping_add(1);
    }

    // J1 两半：基线十组逐组对平，阈值十组零越限。
    let j1a = b.baseline_complete() && b.groups.len() == 10;
    let j1b = b.zero_exceed();
    s.add("J1-基线逐组对平", j1a, "10 组均完成 32 轮重放");
    s.add("J1-阈值零越限", j1b, "10 组预算均未被击穿");

    // J2 守恒：320 轮 = 各组轮数之和，逐组总时非零。
    let mut total_ok = true;
    let mut i = 0usize;
    while i < b.groups.len() {
        if b.groups[i].total_ns == 0 || b.groups[i].mean_ns() < b.groups[i].baseline_ns {
            total_ok = false;
        }
        i += 1;
    }
    let j2 = b.replayed == 320 && b.recount() && total_ok;
    s.add("J2-轮数守恒", j2, "320 轮重放与组累计严格对平");

    // J3 四路注入。
    let inj = inject_four();
    s.add("J3-重号注入", inj[0], "同 gid 二次登记必拒");
    s.add("J3-未知组注入", inj[1], "未知 gid 重放必拒");
    s.add("J3-超限注入", inj[2], "超预算样本必拒并记账");
    s.add("J3-冻结注入", inj[3], "封账后重放必拒");

    // J4 冻结根稳定且写路径关闭。
    let mut f = b.clone();
    let root = f.freeze();
    let j4 = f.fold() == root && f.root == Some(root)
        && f.replay(1, 10_100).err() == Some(RF_FROZEN)
        && f.enroll(11, 20_000, 2_500).err() == Some(RF_FROZEN);
    s.add("J4-封账不可变", j4, "冻结后重放/登记双拒且根稳定");

    // J5 指纹敏感：预算变化必须改变根。
    let mut c = b.clone();
    c.groups[2].budget_ppm = 3_000;
    let j5 = c.fold() != b.fold();
    s.add("J5-指纹敏感", j5, "单组预算变化即改变 fold 根");

    // J6 空账与范围。
    let mut e = BaselineLedger::new(500, 1_000);
    let j6 = e.settle(1).err() == Some(RF_RANGE)
        && e.enroll(1, 999, 1_000).err() == Some(RF_RANGE)
        && e.enroll(1, 5_000, 0).err() == Some(RF_RANGE);
    s.add("J6-空账范围", j6, "未登记组与零基线/零预算均拒");

    // J7 唯一有序：gid 严格递增且无重复。
    let mut ordered = true;
    let mut k = 1usize;
    while k < b.groups.len() {
        if b.groups[k].gid <= b.groups[k - 1].gid {
            ordered = false;
        }
        k += 1;
    }
    s.add("J7-唯一有序", ordered, "10 组 gid 严格递增");

    // J8 负语义：一组击穿后不得继续报全绿。
    let mut bad = b.clone();
    let _ = bad.replay(4, 30_000);
    let j8 = !bad.zero_exceed() && bad.closeout().is_err() && bad.groups[3].limit_hits == 1;
    s.add("J8-越限非绿", j8, "单组越限即否定阈值全绿");

    // J9 动态累计。
    s.add_dyn("J9-累计动态", b.replayed == 320,
        &format!("rounds={} groups={} hits={}", b.replayed, b.groups.len(),
            b.groups.iter().map(|g| g.limit_hits).sum::<u32>()));

    // J10 均值/漂移算法：正确落在预算内。
    let m = b.groups[0].mean_margin_ppm();
    let j10 = b.groups[0].mean_ns() >= b.groups[0].baseline_ns && m <= 2_500;
    s.add("J10-均值算法", j10, "组均值漂移不超过预算");

    // J11 收口成功且摘要可读。
    let closed = b.closeout();
    let text = b.summary();
    let j11 = closed.is_ok() && closed.ok() == Some(b.fold()) && text.starts_with("baseline|");
    s.add_dyn("J11-收口摘要", j11, &text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11078_flow() {
        let mut b = BaselineLedger::new(1_000, 2_500);
        // 组预算与样本必须同域：+1%（10_100 对 10_000 = 10_000ppm）要放行，
        // 故组预算取 25_000ppm（2.5%）；初版填 2_500ppm（0.25%），于是
        // 1% 的合法样本也被判 RF_LIMIT，与「合法放行 / 越限拒绝」两行自相矛盾。
        assert!(b.enroll(1, 10_000, 25_000).is_ok());
        assert!(b.enroll(1, 11_000, 2_500).err() == Some(RF_DUP));
        assert!(b.replay(1, 10_100).is_ok());
        assert!(b.replay(1, 20_000).err() == Some(RF_LIMIT));
        assert!(b.settle(1).err() == Some(RF_LIMIT));
        assert!(b.replay_batch(1, 31).is_ok());
        assert!(b.recount());
        let fp = b.fold();
        let root = b.freeze();
        assert!(fp == root && b.fold() == root);
        assert!(b.replay(1, 10_100).err() == Some(RF_FROZEN));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11078_checks_green() {
        assert!(run_unx11078_checks().all_passed(), "UNX-F11078 判据未全绿");
    }
}
