//! UNX-F11111 判据重放域级抽检（跨批抽样复核）（0x1f591 段）
//!
//! 锚点：UNX-F11111-J1 抽检逐条复测对平（10/10），抽检覆盖六批达标（10/10）。
//! 域：CoRun·UNX-C4-B36（抽检面）。
//!
//! 结构：抽检计划（SamplePlan：跨批抽点 → 首跑落指纹 → 复测比对 → 覆盖
//! 达标）+ 抽点（Sample：批号 + 判据号 + 首跑/复测双指纹）+ 冻结根 + 注入核
//! （重复抽点/未抽先测/越段批/复测不符四路必抓）。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1f591 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f591;
pub const SP_NOBATCH: u32 = SEG_BASE + 0x01; // 抽点不存在
pub const SP_DUP: u32 = SEG_BASE + 0x02; // 批内判据重复抽
pub const SP_MISMATCH: u32 = SEG_BASE + 0x03; // 复测指纹不符
pub const SP_RANGE: u32 = SEG_BASE + 0x04; // 批号越段
pub const SP_COVER: u32 = SEG_BASE + 0x05; // 覆盖批数不足
pub const SP_THIN: u32 = SEG_BASE + 0x06; // 抽检条数不足
pub const SP_FROZEN: u32 = SEG_BASE + 0x07; // 冻结后写账
pub const SP_ZERO: u32 = SEG_BASE + 0x08; // 零判据或零指纹

/// 段内六批（B30–B35）。
pub const BATCH_FIRST: u32 = 30;
pub const BATCH_LAST: u32 = 35;
/// 覆盖批数下限（六批全覆盖）。
pub const COVER_NEEDED: usize = 6;
/// 抽检条数下限（每批 2 条 × 6）。
pub const SAMPLE_MIN: usize = 12;
/// 抽点定容。
pub const CAP: usize = 24;
/// 抽检面标（fold 种子）。
pub const SP_TAG: u64 = 0x1f5_9191;
/// 指纹步进。
pub const FP_STEP: u64 = 0x100000001b3;

/// 单个抽点：批号 + 判据号 + 首跑指纹 + 复测指纹。
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub batch: u32,
    pub crit: u32,
    pub first: u64,
    pub replay: Option<u64>,
}

impl Sample {
    /// 复测对平：复测指纹存在且与首跑一致。
    pub fn matched(&self) -> bool {
        match self.replay {
            Some(r) => r == self.first,
            None => false,
        }
    }

    pub fn render(&self) -> String {
        let flag = if self.matched() { "=" } else { "!" };
        format!("B{}J{}|{}|{:#x}", self.batch, self.crit, flag, self.first)
    }
}

/// 抽检计划。
#[derive(Clone, Debug)]
pub struct SamplePlan {
    pub samples: Vec<Sample>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl SamplePlan {
    pub fn new() -> SamplePlan {
        SamplePlan { samples: Vec::new(), frozen: false, root: None }
    }

    /// 抽点：越段批/零判据/零指纹/重复抽/超容皆拒。
    pub fn draw(&mut self, batch: u32, crit: u32, first: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(SP_FROZEN);
        }
        if batch < BATCH_FIRST || batch > BATCH_LAST {
            return Err(SP_RANGE);
        }
        if crit == 0 || first == 0 {
            return Err(SP_ZERO);
        }
        if self.samples.iter().any(|s| s.batch == batch && s.crit == crit) {
            return Err(SP_DUP);
        }
        if self.samples.len() >= CAP {
            return Err(SP_COVER);
        }
        self.samples.push(Sample { batch, crit, first, replay: None });
        Ok(())
    }

    fn find(&mut self, batch: u32, crit: u32) -> Option<&mut Sample> {
        let mut k = 0usize;
        while k < self.samples.len() {
            if self.samples[k].batch == batch && self.samples[k].crit == crit {
                return Some(&mut self.samples[k]);
            }
            k += 1;
        }
        None
    }

    /// 复测：未抽点拒、指纹不符拒（不改账）、一致则落复测指纹。
    pub fn replay(&mut self, batch: u32, crit: u32, fp: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(SP_FROZEN);
        }
        let s = self.find(batch, crit).ok_or(SP_NOBATCH)?;
        if fp != s.first {
            return Err(SP_MISMATCH);
        }
        s.replay = Some(fp);
        Ok(())
    }

    /// 已覆盖批号（升序去重）。
    pub fn covered(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.samples.len() {
            let b = self.samples[k].batch;
            if !v.contains(&b) {
                v.push(b);
            }
            k += 1;
        }
        v.sort_unstable();
        v
    }

    /// 覆盖达标：六批全覆盖且条数不少于下限。
    pub fn coverage_ok(&self) -> bool {
        self.covered().len() == COVER_NEEDED && self.samples.len() >= SAMPLE_MIN
    }

    /// 逐条复测一致。
    pub fn consistent(&self) -> bool {
        let mut k = 0usize;
        while k < self.samples.len() {
            if !self.samples[k].matched() {
                return false;
            }
            k += 1;
        }
        !self.samples.is_empty()
    }

    /// 未复测点名（批号十位 + 判据个位编码，便于诊断）。
    pub fn pending(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.samples.len() {
            if self.samples[k].replay.is_none() {
                v.push(self.samples[k].batch.saturating_mul(100).saturating_add(self.samples[k].crit));
            }
            k += 1;
        }
        v
    }

    /// 抽检闭合：覆盖达标且逐条复测对平。
    pub fn closed(&self) -> bool {
        self.coverage_ok() && self.consistent()
    }

    /// 冻结并落根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 计划指纹：批号 + 判据号 + 双指纹链式入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SP_TAG;
        let mut k = 0usize;
        while k < self.samples.len() {
            let s = &self.samples[k];
            h = (h ^ s.batch as u64).wrapping_mul(FP_STEP);
            h = (h ^ s.crit as u64).wrapping_mul(FP_STEP);
            h = (h ^ s.first).wrapping_mul(FP_STEP);
            let r = s.replay.unwrap_or(0);
            h = (h ^ r).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }
}

/// 首跑指纹样本：批号与判据号共同决定。
pub fn first_fp(batch: u32, crit: u32) -> u64 {
    0x51ed_2701u64
        .wrapping_mul(batch as u64)
        .wrapping_add(crit as u64)
        .wrapping_add(1)
}

/// 标准计划：每批抽 2 条并复测对平。
pub fn build_plan() -> (SamplePlan, bool) {
    let mut p = SamplePlan::new();
    let mut ok = true;
    let mut batch = BATCH_FIRST;
    while batch <= BATCH_LAST {
        let mut crit = 1u32;
        while crit <= 2 {
            let fp = first_fp(batch, crit);
            if p.draw(batch, crit, fp).is_err() {
                ok = false;
            }
            if p.replay(batch, crit, fp).is_err() {
                ok = false;
            }
            crit += 1;
        }
        batch += 1;
    }
    (p, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 重复抽点必拒
    let mut a = SamplePlan::new();
    let _ = a.draw(BATCH_FIRST, 1, 7);
    got[0] = a.draw(BATCH_FIRST, 1, 8).err() == Some(SP_DUP);
    // 2) 未抽先复测必拒
    let mut b = SamplePlan::new();
    got[1] = b.replay(BATCH_FIRST, 1, 7).err() == Some(SP_NOBATCH);
    // 3) 越段批必拒
    let mut c = SamplePlan::new();
    got[2] = c.draw(BATCH_LAST + 1, 1, 7).err() == Some(SP_RANGE)
        && c.draw(BATCH_FIRST - 1, 1, 7).err() == Some(SP_RANGE);
    // 4) 复测不符必拒且不落账
    let mut d = SamplePlan::new();
    let fp = first_fp(BATCH_FIRST, 3);
    let _ = d.draw(BATCH_FIRST, 3, fp);
    got[3] = d.replay(BATCH_FIRST, 3, fp.wrapping_add(1)).err() == Some(SP_MISMATCH)
        && d.pending().len() == 1;
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11111_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11111");

    let (mut plan, built) = build_plan();
    // J1 抽检逐条复测对平
    s.add(
        "J1-逐条复测对平",
        built && plan.samples.len() == SAMPLE_MIN && plan.consistent() && plan.pending().is_empty(),
        "12 个抽点逐条复测与首跑一致",
    );

    // J2 抽检覆盖六批达标
    s.add(
        "J2-覆盖六批",
        plan.coverage_ok() && plan.covered().len() == COVER_NEEDED,
        "B30–B35 六批全覆盖且条数达标",
    );

    // J3 计划闭合
    s.add("J3-抽检闭合", plan.closed(), "覆盖达标且逐条一致");

    // J4 复跑不一致检出
    let j4 = {
        let mut c = SamplePlan::new();
        let fp = first_fp(BATCH_FIRST, 5);
        let _ = c.draw(BATCH_FIRST, 5, fp);
        c.replay(BATCH_FIRST, 5, fp.wrapping_add(3)).err() == Some(SP_MISMATCH)
            && c.pending().len() == 1
            && !c.consistent()
    };
    s.add("J4-不一致检出", j4, "复测指纹偏移即拒且不落账");

    // J5 重复抽点拒
    let j5 = {
        let mut c = SamplePlan::new();
        let _ = c.draw(BATCH_FIRST, 1, 9);
        c.draw(BATCH_FIRST, 1, 10).err() == Some(SP_DUP)
    };
    s.add("J5-重复抽点拒", j5, "批内同判据二次抽点必拒");

    // J6 未抽先复测拒
    let j6 = {
        let mut c = SamplePlan::new();
        c.replay(BATCH_LAST, 9, 1).err() == Some(SP_NOBATCH)
    };
    s.add("J6-未抽拒复测", j6, "未抽点即复测必拒");

    // J7 越段批与零指纹拒
    let j7 = {
        let mut c = SamplePlan::new();
        let a = c.draw(BATCH_LAST + 1, 1, 5).err() == Some(SP_RANGE);
        let b = c.draw(BATCH_FIRST, 0, 5).err() == Some(SP_ZERO);
        let d = c.draw(BATCH_FIRST, 1, 0).err() == Some(SP_ZERO);
        a && b && d && c.samples.is_empty()
    };
    s.add("J7-越段零值拒", j7, "越段批与零判据/零指纹三路必拒");

    // J8 注入检出：重复抽点 + 未抽复测
    let inj = inject_four();
    s.add("J8-重抽未抽检出", inj[0] && inj[1], "重复抽点与未抽复测两路必抓");

    // J9 注入检出：越段 + 复测不符
    s.add("J9-越段不符检出", inj[2] && inj[3], "越段批与复测不符两路必抓");

    // J10 覆盖不足不闭合
    let j10 = {
        let mut c = SamplePlan::new();
        let mut batch = BATCH_FIRST;
        while batch <= BATCH_LAST {
            let fp = first_fp(batch, 1);
            let _ = c.draw(batch, 1, fp);
            let _ = c.replay(batch, 1, fp);
            if batch != 33 {
                let fp2 = first_fp(batch, 2);
                let _ = c.draw(batch, 2, fp2);
                let _ = c.replay(batch, 2, fp2);
            }
            batch += 1;
        }
        !c.coverage_ok() && c.covered().len() == COVER_NEEDED && !c.closed()
    };
    s.add("J10-条数不足不闭合", j10, "覆盖六批但少一条即不闭合");

    // J11 冻结拒写
    let root = plan.seal();
    let j11 = plan.draw(BATCH_FIRST, 9, 1).err() == Some(SP_FROZEN)
        && plan.replay(BATCH_FIRST, 1, first_fp(BATCH_FIRST, 1)).err() == Some(SP_FROZEN);
    s.add("J11-冻结拒写", j11, "封存后抽点与复测两路必拒");

    // J12 冻结根稳定
    s.add(
        "J12-冻结根稳定",
        plan.root == Some(root) && root == plan.fold() && root != 0,
        "抽检计划根可重算零漂移",
    );

    // J13 渲染面
    let j13 = plan.samples[0].render().contains("=")
        && plan.samples[0].render().contains("B30")
        && plan.samples[0].render().contains("J1");
    s.add("J13-抽点渲染", j13, "已对平抽点渲染带等旗与批判号");

    // J14 动态明细
    s.add_dyn(
        "J14-抽检明细",
        plan.closed() && plan.fold() == root,
        &format!("n={} cov={} pending={} root={:#x}", plan.samples.len(), plan.covered().len(), plan.pending().len(), root),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11111_sample_flow() {
        let (mut p, built) = build_plan();
        assert!(built);
        assert!(p.closed());
        assert!(p.covered().len() == 6);
        assert!(p.seal() != 0);
        assert!(p.draw(30, 9, 1).err() == Some(SP_FROZEN));
        let mut q = SamplePlan::new();
        assert!(q.draw(31, 1, first_fp(31, 1)).is_ok());
        assert!(q.replay(31, 1, 1).err() == Some(SP_MISMATCH));
    }

    #[test]
    fn unx11111_checks_green() {
        let s = run_unx11111_checks();
        assert!(s.all_passed(), "UNX-F11111 判据未全绿");
    }
}
