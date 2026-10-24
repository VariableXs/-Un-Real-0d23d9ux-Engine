//! VE-F4318 · U06 联调（U06 场景应用组）
//! 职责：U06 组内互联与跨组联调——组内链贯通（总纲→七景→测试→度量→
//! 反馈闭环五段顺序；越站拦截）+ 跨组对接（上承 U05 工具面、下启 U07
//! 性能组）+ 联调用例十例跨场景全链（违例从 IDE 发现到市场门禁）+ 逐段
//! 探针定位 + 对拍仲裁 + 联调含无障碍回归（域本色）。
//! 段位：0x32A0（续表 idx552）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x32A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UjCode {
    /// 越站（顺序违规）。
    StageSkip = SEG_BASE,
    /// 探针定位失败（断点未定位到段）。
    ProbeMiss = SEG_BASE + 0x01,
    /// 对拍失配（跨组契约不符）。
    Mismatch = SEG_BASE + 0x02,
    /// 无障碍回归缺项（域色硬线）。
    A11yMissing = SEG_BASE + 0x03,
}

/// 五段链：0 总纲 → 1 七景 → 2 测试 → 3 度量 → 4 反馈闭环。
pub const STAGES: usize = 5;

/// 联调台。
#[derive(Default)]
pub struct U06Joint {
    /// 各段通过位。
    pub stage_pass: [bool; STAGES],
    /// 逐段探针记录：(段, 探针命中)。
    pub probes: Vec<(usize, bool)>,
    /// 十例表：(例号, 全链通过)。
    pub cases: [(bool, bool); 10],
    /// 跨组契约卡：(上游 U05 版, 下游 U07 版)。
    pub up_ver: u32,
    pub down_ver: u32,
    /// 无障碍回归项（域本色）。
    pub a11y_done: bool,
    pub arbitration_wins: u32,
}

impl U06Joint {
    pub fn new(up: u32, down: u32) -> Self {
        U06Joint { up_ver: up, down_ver: down, ..Default::default() }
    }

    /// 顺序过段：必须按 0..4 依次；越站拦截。
    pub fn pass_stage(&mut self, stage: usize) -> Result<(), UjCode> {
        if stage >= STAGES {
            return Err(UjCode::StageSkip);
        }
        if stage > 0 && !self.stage_pass[stage - 1] {
            return Err(UjCode::StageSkip);
        }
        self.stage_pass[stage] = true;
        Ok(())
    }

    /// 逐段探针：断点定位到段。
    pub fn probe(&mut self, stage: usize, hit: bool) -> Result<(), UjCode> {
        if stage >= STAGES {
            return Err(UjCode::ProbeMiss);
        }
        self.probes.push((stage, hit));
        Ok(())
    }

    /// 断点归因：最后一条未命中探针所在段。
    pub fn fault_stage(&self) -> Option<usize> {
        self.probes.iter().rev().find(|p| !p.1).map(|p| p.0)
    }

    /// 跨组契约对拍：上承 U05 / 下启 U07 版本一致才通过。
    pub fn contract_ok(&self, up_expect: u32, down_expect: u32) -> Result<(), UjCode> {
        if self.up_ver != up_expect || self.down_ver != down_expect {
            return Err(UjCode::Mismatch);
        }
        Ok(())
    }

    /// 十例执行：例号 0..10；每例须无障碍回归项齐备才记过（域色）。
    pub fn run_case(&mut self, idx: usize, chain_ok: bool) -> Result<(), UjCode> {
        if idx >= 10 {
            return Err(UjCode::StageSkip);
        }
        if !self.a11y_done && chain_ok {
            return Err(UjCode::A11yMissing);
        }
        self.cases[idx] = (true, chain_ok);
        Ok(())
    }

    /// 十例全过断言。
    pub fn all_cases_pass(&self) -> bool {
        self.cases.iter().all(|c| c.0 && c.1)
    }

    /// 对拍仲裁：失配后裁决（赢 = 采纳本组口径，计数留痕）。
    pub fn arbitrate(&mut self, win: bool) {
        if win {
            self.arbitration_wins += 1;
        }
    }

    /// 收口就绪：五段全过 + 十例全过 + 契约对拍一致。
    pub fn ready(&self, up: u32, down: u32) -> bool {
        self.stage_pass.iter().all(|p| *p)
            && self.all_cases_pass()
            && self.contract_ok(up, down).is_ok()
            && self.a11y_done
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves552_checks() -> CheckSet {
    let mut set = CheckSet::new("ves552");
    let mut uj = U06Joint::new(12, 30);

    // C552-01 五段顺序贯通。
    let mut ok = true;
    for s in 0..STAGES {
        if uj.pass_stage(s).is_err() {
            ok = false;
        }
    }
    chk(&mut set, "C552-01", ok && uj.stage_pass.iter().all(|p| *p), "五段顺序贯通");

    // C552-02 越站拦截（未过前段直跳）。
    let mut uj2 = U06Joint::new(12, 30);
    chk(&mut set, "C552-02", uj2.pass_stage(2) == Err(UjCode::StageSkip), "越站拦截");

    // C552-03 段号越界拦截。
    chk(&mut set, "C552-03", uj.pass_stage(5) == Err(UjCode::StageSkip), "段号越界拦");

    // C552-04 逐段探针 + 断点归因。
    let _ = uj2.pass_stage(0);
    let _ = uj2.probe(0, true);
    let _ = uj2.probe(1, false);
    chk(&mut set, "C552-04", uj2.fault_stage() == Some(1), "断点定位到段");

    // C552-05 探针段号越界拒绝。
    chk(&mut set, "C552-05", uj2.probe(9, true) == Err(UjCode::ProbeMiss), "探针越界拒");

    // C552-06 跨组契约对拍一致。
    chk(&mut set, "C552-06", uj.contract_ok(12, 30).is_ok(), "契约对拍一致");

    // C552-07 契约失配检出。
    chk(&mut set, "C552-07", uj.contract_ok(13, 30) == Err(UjCode::Mismatch), "契约失配检出");

    // C552-08 无障碍回归缺项时十例记过被拒（域色硬线）。
    chk(&mut set, "C552-08", uj.run_case(0, true) == Err(UjCode::A11yMissing), "缺无障碍拒记");

    // C552-09 补齐无障碍回归后十例全过。
    uj.a11y_done = true;
    let mut all = true;
    for i in 0..10 {
        if uj.run_case(i, true).is_err() {
            all = false;
        }
    }
    chk(&mut set, "C552-09", all && uj.all_cases_pass(), "十例全过");

    // C552-10 例号越界拒绝。
    chk(&mut set, "C552-10", uj.run_case(10, true).is_err(), "例号越界拒");

    // C552-11 对拍仲裁留痕。
    uj.arbitrate(true);
    uj.arbitrate(false);
    chk(&mut set, "C552-11", uj.arbitration_wins == 1, "仲裁胜负留痕");

    // C552-12 收口就绪总闸（五段 + 十例 + 契约 + 无障碍）。
    chk(&mut set, "C552-12", uj.ready(12, 30) && !uj2.ready(12, 30), "收口总闸判定");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_order_enforced() {
        let mut uj = U06Joint::new(1, 1);
        assert_eq!(uj.pass_stage(1), Err(UjCode::StageSkip));
        assert!(uj.pass_stage(0).is_ok());
        assert!(uj.pass_stage(1).is_ok());
        assert!(uj.pass_stage(0).is_ok()); // 已过段重过幂等
    }

    #[test]
    fn a11y_gate_hardline() {
        let mut uj = U06Joint::new(1, 1);
        assert_eq!(uj.run_case(0, true), Err(UjCode::A11yMissing));
        uj.a11y_done = true;
        assert!(uj.run_case(0, true).is_ok());
        assert!(!uj.all_cases_pass());
    }

    #[test]
    fn ready_needs_all() {
        let mut uj = U06Joint::new(5, 6);
        assert!(!uj.ready(5, 6));
        for s in 0..STAGES {
            let _ = uj.pass_stage(s);
        }
        uj.a11y_done = true;
        for i in 0..10 {
            let _ = uj.run_case(i, true);
        }
        assert!(uj.ready(5, 6));
        assert!(!uj.ready(5, 7));
    }
}
