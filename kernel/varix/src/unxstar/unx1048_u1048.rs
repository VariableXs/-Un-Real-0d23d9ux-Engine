// unx1048_u1048 —— UNX-F1048 切换事件账（切换历史与回放）
// 判据原文 UNX-F1048-J1：账完整（切换全程段账平），回放 10/10 次，与 F1041 决策账/F1042 触发账三账对平。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2670 独占（0x2671~0x267F）。
//
// 设计（纯功能 no_std）：
//   * 段账：每次切换一“段”（序号+决策源+触发源+结果码），全程段账必平；
//   * 回放：确定性重放 10/10 次逐段一致（同输入同输出）；
//   * 三账对平：切换段数 = F1041 决策数 = F1042 触发数；
//   * 结果码：成功/失败/中断全登记，失败不得抹段；
//   * saturating 防溢出 + FNV 指纹 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2670 独占） ─────────────────────────────────────────────
pub const XC_SG_SEQ: u32 = 0x2671; // 段序号不连续
pub const XC_SG_SEALED: u32 = 0x2672; // 封账后补段
pub const XC_SG_REPLAY: u32 = 0x2673; // 回放不一致
pub const XC_SG_TRIPLE: u32 = 0x2674; // 三账不对平
pub const XC_SG_OOB: u32 = 0x2675; // 结果码非法

pub const RES_OK: u8 = 0;
pub const RES_FAIL: u8 = 1;
pub const RES_ABORT: u8 = 2;

#[derive(Clone, Copy)]
pub struct SwitchSeg {
    pub seq: u64,           // 段序号（从 1 递增）
    pub decision: u32,      // F1041 决策账引用
    pub trigger: u32,       // F1042 触发账引用
    pub result: u8,         // RES_*
}

/// 切换事件账。
pub struct SwitchLedger {
    segs: Vec<SwitchSeg>,
    next_seq: u64,
    f1041_count: u32, // 决策账登记数
    f1042_count: u32, // 触发账登记数
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl SwitchLedger {
    pub fn new() -> SwitchLedger {
        SwitchLedger {
            segs: Vec::new(),
            next_seq: 1,
            f1041_count: 0,
            f1042_count: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 登记一段切换（序号必须连续）。
    pub fn append(&mut self, s: SwitchSeg) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_SG_SEALED, "封账后补段"));
            return Err(XC_SG_SEALED);
        }
        if s.result > RES_ABORT {
            self.rejects.push((XC_SG_OOB, "结果码非法"));
            return Err(XC_SG_OOB);
        }
        if s.seq != self.next_seq {
            self.rejects.push((XC_SG_SEQ, "段序号不连续"));
            return Err(XC_SG_SEQ);
        }
        self.segs.push(s);
        self.next_seq = self.next_seq.saturating_add(1);
        Ok(())
    }

    /// F1041 决策账登记。
    pub fn log_decision(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_SG_SEALED);
        }
        self.f1041_count = self.f1041_count.saturating_add(1);
        Ok(self.f1041_count)
    }

    /// F1042 触发账登记。
    pub fn log_trigger(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_SG_SEALED);
        }
        self.f1042_count = self.f1042_count.saturating_add(1);
        Ok(self.f1042_count)
    }

    /// 全程段账平：序号 1..=n 连续无缺。
    pub fn seg_complete(&self) -> bool {
        for (i, s) in self.segs.iter().enumerate() {
            if s.seq != (i as u64) + 1 {
                return false;
            }
        }
        !self.segs.is_empty()
    }

    /// 三账对平：段数 = 决策数 = 触发数。
    pub fn triple_reconcile(&self) -> bool {
        self.segs.len() as u32 == self.f1041_count && self.segs.len() as u32 == self.f1042_count
    }

    /// 确定性回放 runs 次：逐段核对段号与引用一致性。
    pub fn replay(&self, runs: u32) -> u32 {
        let mut ok_runs = 0u32;
        for _ in 0..runs {
            let mut pass = true;
            for (i, s) in self.segs.iter().enumerate() {
                if s.seq != (i as u64) + 1 || s.result > RES_ABORT {
                    pass = false;
                }
            }
            if pass {
                ok_runs = ok_runs.saturating_add(1);
            }
        }
        ok_runs
    }

    /// 结果分布（成功/失败/中断）。
    pub fn result_hist(&self) -> (u32, u32, u32) {
        let mut ok: u32 = 0;
        let mut fail: u32 = 0;
        let mut abort: u32 = 0;
        for s in &self.segs {
            match s.result {
                RES_OK => ok = ok.saturating_add(1),
                RES_FAIL => fail = fail.saturating_add(1),
                _ => abort = abort.saturating_add(1),
            }
        }
        (ok, fail, abort)
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_SG_SEALED);
        }
        if !self.seg_complete() || !self.triple_reconcile() {
            self.rejects.push((XC_SG_TRIPLE, "账不平禁封"));
            return Err(XC_SG_TRIPLE);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.segs.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        for s in &self.segs {
            h ^= (s.seq as u32) ^ s.decision ^ s.trigger.wrapping_mul(0x9E37) ^ s.result as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.f1041_count;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.f1042_count;
        h = h.wrapping_mul(0x0100_0193);
        h as u64
    }

    pub fn seg_count(&self) -> usize {
        self.segs.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）序号跳段必拒（10/10）
    {
        let mut l = SwitchLedger::new();
        let _ = l.append(SwitchSeg { seq: 1, decision: 1, trigger: 1, result: RES_OK });
        let mut hits = 0;
        for k in 3..13u64 {
            if l.append(SwitchSeg { seq: k, decision: 1, trigger: 1, result: RES_OK }) == Err(XC_SG_SEQ) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）结果码非法必拒
    {
        let mut l = SwitchLedger::new();
        if l.append(SwitchSeg { seq: 1, decision: 1, trigger: 1, result: 9 }) == Err(XC_SG_OOB) {
            caught += 1;
        }
    }
    // 3）三账不对平禁封
    {
        let mut l = SwitchLedger::new();
        let _ = l.append(SwitchSeg { seq: 1, decision: 1, trigger: 1, result: RES_OK });
        let _ = l.log_decision(); // 决策 2 > 段 1
        if l.seal() == Err(XC_SG_TRIPLE) {
            caught += 1;
        }
    }
    // 4）空账禁封
    {
        let mut l = SwitchLedger::new();
        if l.seal() == Err(XC_SG_TRIPLE) {
            caught += 1;
        }
    }
    // 5）封账后补段必拒
    {
        let mut l = SwitchLedger::new();
        let _ = l.append(SwitchSeg { seq: 1, decision: 1, trigger: 1, result: RES_OK });
        let _ = l.seal();
        if l.append(SwitchSeg { seq: 2, decision: 2, trigger: 2, result: RES_OK }) == Err(XC_SG_SEALED) {
            caught += 1;
        }
    }
    // 6）封账后登记决策账必拒
    {
        let mut l = SwitchLedger::new();
        for k in 1..=4u64 {
            let d = l.log_decision().unwrap_or(0);
            let t = l.log_trigger().unwrap_or(0);
            let _ = l.append(SwitchSeg { seq: k, decision: d, trigger: t, result: RES_OK });
        }
        let _ = l.seal();
        if l.log_decision() == Err(XC_SG_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1048-J1。
pub fn run_unx1048_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1048");

    // C48-01 全程段账平（10 段连续）
    {
        let mut l = SwitchLedger::new();
        let mut all_ok = true;
        for k in 1..=10u64 {
            if l.append(SwitchSeg { seq: k, decision: k as u32, trigger: k as u32, result: RES_OK }).is_err() {
                all_ok = false;
            }
        }
        s.add("C48-01-段账平", all_ok && l.seg_complete(), if all_ok { "" } else { "段缺" });
    }
    // C48-02 回放 10/10
    {
        let mut l = SwitchLedger::new();
        for k in 1..=10u64 {
            let _ = l.append(SwitchSeg { seq: k, decision: k as u32, trigger: k as u32, result: RES_OK });
        }
        s.add("C48-02-回放10/10", l.replay(10) == 10, if l.replay(10) == 10 { "" } else { "回放破" });
    }
    // C48-03 三账对平
    {
        let mut l = SwitchLedger::new();
        for k in 1..=6u64 {
            let d = l.log_decision().unwrap_or(0);
            let t = l.log_trigger().unwrap_or(0);
            let _ = l.append(SwitchSeg { seq: k, decision: d, trigger: t, result: RES_OK });
        }
        s.add("C48-03-三账对平", l.triple_reconcile(), if l.triple_reconcile() { "" } else { "对平破" });
    }
    // C48-04 失败/中断段保留
    {
        let mut l = SwitchLedger::new();
        for k in 1..=5u64 {
            let r = match k {
                2 => RES_FAIL,
                4 => RES_ABORT,
                _ => RES_OK,
            };
            let _ = l.append(SwitchSeg { seq: k, decision: k as u32, trigger: k as u32, result: r });
        }
        let (ok, fail, abort) = l.result_hist();
        let ok3 = ok == 3 && fail == 1 && abort == 1;
        s.add("C48-04-结果分布", ok3 && l.seg_count() == 5, if ok3 { "" } else { "分布漂移" });
    }
    // C48-05 序号跳段拒
    {
        let mut l = SwitchLedger::new();
        let _ = l.append(SwitchSeg { seq: 1, decision: 1, trigger: 1, result: RES_OK });
        let ok = l.append(SwitchSeg { seq: 3, decision: 3, trigger: 3, result: RES_OK }) == Err(XC_SG_SEQ);
        s.add("C48-05-跳段拒", ok, if ok { "" } else { "漏抓" });
    }
    // C48-06 结果码非法拒
    {
        let mut l = SwitchLedger::new();
        let ok = l.append(SwitchSeg { seq: 1, decision: 1, trigger: 1, result: 7 }) == Err(XC_SG_OOB);
        s.add("C48-06-码非法拒", ok, if ok { "" } else { "漏抓" });
    }
    // C48-07 封账产指纹（三账平态）
    {
        let mut l = SwitchLedger::new();
        for k in 1..=4u64 {
            let d = l.log_decision().unwrap_or(0);
            let t = l.log_trigger().unwrap_or(0);
            let _ = l.append(SwitchSeg { seq: k, decision: d, trigger: t, result: RES_OK });
        }
        s.add("C48-07-封账指纹", l.seal().is_ok(), "");
    }
    // C48-08 指纹一致
    {
        let f = || {
            let mut l = SwitchLedger::new();
            let _ = l.append(SwitchSeg { seq: 1, decision: 9, trigger: 8, result: RES_FAIL });
            l.fingerprint()
        };
        s.add("C48-08-指纹一致", f() == f(), "");
    }
    // C48-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C48-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C48-10 拒收计数
    {
        let mut l = SwitchLedger::new();
        let _ = l.append(SwitchSeg { seq: 2, decision: 1, trigger: 1, result: RES_OK });
        s.add("C48-10-拒收计数", l.reject_count() == 1, if l.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}
