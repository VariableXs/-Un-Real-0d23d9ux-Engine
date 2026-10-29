//! UNX-F10372 正演判定书（0x1b900 段）
//!
//! 锚点：UNX-F10372-J1 正演判定书——硬闸通过/不通过单值裁决，
//! 失败残留下的通过裁决 10/10 次检出。
//! 域：CoRun·UNX-C3（硬闸正演·判定谱系）。
//!
//! 结构：四计数输入（PASS/FAIL/SKIP/TIMEOUT）+ 单值裁决器（fail/timeout
//! 归零才通过）+ 一次性裁决封印 + 分歧注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b900 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b900;
pub const VD_REJUDGE: u32 = SEG_BASE + 0x01; // 重复裁决
pub const VD_DIVERGE: u32 = SEG_BASE + 0x02; // 分歧裁决（有失败却判通过）
pub const VD_NO_INPUT: u32 = SEG_BASE + 0x03; // 无输入计数即裁决
pub const VD_NEGATIVE: u32 = SEG_BASE + 0x04; // 负计数
pub const VD_SEALED: u32 = SEG_BASE + 0x05; // 封账后写账

/// 判定书定容（输入快照行数上限）。
pub const CAP: usize = 16;
/// SKIP 不参与通过裁定的上限（超限视为执行面异常）。
pub const SKIP_CEILING: u32 = 64;

/// 四计数输入快照。
#[derive(Clone)]
pub struct Counts {
    pub pass: u32,
    pub fail: u32,
    pub skip: u32,
    pub timeout: u32,
}

impl Counts {
    pub fn new(pass: u32, fail: u32, skip: u32, timeout: u32) -> Counts {
        Counts { pass, fail, skip, timeout }
    }

    /// 合法输入：四计数非负（构造保证）且总量非零、skip 不超顶。
    pub fn ok(&self) -> bool {
        self.pass + self.fail + self.skip + self.timeout > 0
            && self.skip <= SKIP_CEILING
    }

    /// 总量。
    pub fn total(&self) -> u32 {
        self.pass + self.fail + self.skip + self.timeout
    }
}

/// 裁决结果。
#[derive(Clone, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Reject,
}

impl Verdict {
    pub fn as_u32(&self) -> u32 {
        match self {
            Verdict::Pass => 1,
            Verdict::Reject => 0,
        }
    }
}

/// 判定书：输入快照 + 单值裁决（一次性封印）。
pub struct VerdictBook {
    pub snapshot: Option<Counts>,
    pub verdict: Option<Verdict>,
    pub reason: String,
    pub sealed: bool,
}

impl VerdictBook {
    pub fn new() -> VerdictBook {
        VerdictBook {
            snapshot: None,
            verdict: None,
            reason: String::new(),
            sealed: false,
        }
    }

    /// 登记输入快照：负口径（总量为零或 skip 超顶）拒。
    pub fn feed(&mut self, c: Counts) -> Result<(), u32> {
        if self.sealed {
            return Err(VD_SEALED);
        }
        if !c.ok() {
            return Err(VD_NO_INPUT);
        }
        self.snapshot = Some(c);
        Ok(())
    }

    /// 单值裁决：fail==0 且 timeout==0 才通过；无输入即裁决拒。
    pub fn judge(&mut self) -> Result<Verdict, u32> {
        if self.sealed {
            return Err(VD_SEALED);
        }
        let c = match &self.snapshot {
            None => return Err(VD_NO_INPUT),
            Some(c) => c.clone(),
        };
        let v = if c.fail == 0 && c.timeout == 0 {
            Verdict::Pass
        } else {
            Verdict::Reject
        };
        self.reason = format!(
            "pass={} fail={} skip={} timeout={}",
            c.pass, c.fail, c.skip, c.timeout
        );
        self.verdict = Some(v.clone());
        self.sealed = true;
        Ok(v)
    }

    /// 裁决一致性核：登记后重放同一输入，裁决必须复现（单值语义）。
    pub fn replay_matches(&self, c: &Counts) -> Option<bool> {
        let v = match &self.verdict {
            None => return None,
            Some(v) => v.clone(),
        };
        let expect = if c.fail == 0 && c.timeout == 0 {
            Verdict::Pass
        } else {
            Verdict::Reject
        };
        Some(v == expect)
    }

    /// 通过裁决在有失败输入下的分歧检出（注入核用）。
    pub fn diverges(&self) -> bool {
        match (&self.snapshot, &self.verdict) {
            (Some(c), Some(Verdict::Pass)) => c.fail > 0 || c.timeout > 0,
            _ => false,
        }
    }

    /// 裁决指纹（输入+结果折叠）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b9000;
        if let Some(c) = &self.snapshot {
            h = h.wrapping_mul(31).wrapping_add(c.pass);
            h = h.wrapping_mul(31).wrapping_add(c.fail);
            h = h.wrapping_mul(31).wrapping_add(c.skip);
            h = h.wrapping_mul(31).wrapping_add(c.timeout);
        }
        if let Some(v) = &self.verdict {
            h = h.wrapping_mul(31).wrapping_add(v.as_u32());
        }
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        for b in self.reason.bytes() {
            h = h.wrapping_mul(31).wrapping_add(b as u32);
        }
        h
    }
}

/// 独立纯函数裁决器（批量口径：多项输入逐一裁决）。
pub fn judge_batch(inputs: &[Counts]) -> Vec<Verdict> {
    inputs
        .iter()
        .map(|c| {
            if c.fail == 0 && c.timeout == 0 {
                Verdict::Pass
            } else {
                Verdict::Reject
            }
        })
        .collect()
}

/// 注入核（四项）：[无输入裁决, 分歧裁决, 封账再裁, 封账再喂] 检出。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 无输入即裁决必拒
    let mut v = VerdictBook::new();
    got[0] = v.judge() == Err(VD_NO_INPUT);
    // 2) 有失败却判通过（外部伪判分歧）必检出
    let mut v2 = VerdictBook::new();
    let _ = v2.feed(Counts::new(10, 1, 0, 0));
    // 强行写通过后 diverges() 必须抓到
    v2.verdict = Some(Verdict::Pass);
    got[1] = v2.diverges();
    // 3) 封账后再裁决必拒
    let mut v3 = VerdictBook::new();
    let _ = v3.feed(Counts::new(10, 0, 0, 0));
    let _ = v3.judge();
    got[2] = v3.judge() == Err(VD_SEALED);
    // 4) 封账后再喂输入必拒
    got[3] = v3.feed(Counts::new(1, 0, 0, 0)) == Err(VD_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10372_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10372");

    // 正路径：零失败零超时 → 通过裁决
    let mut v = VerdictBook::new();
    let _ = v.feed(Counts::new(120, 0, 4, 0));
    let j1 = v.judge() == Ok(Verdict::Pass);
    s.add("J1-全绿判通过", j1, "零失败零超时判通过");

    // 单值裁决：有失败判不通过
    let j2 = {
        let mut c = VerdictBook::new();
        let _ = c.feed(Counts::new(119, 1, 4, 0));
        c.judge() == Ok(Verdict::Reject)
    };
    s.add("J2-失败判不通过", j2, "任一失败即不通过");

    // 单值裁决：有超时判不通过
    let j3 = {
        let mut c = VerdictBook::new();
        let _ = c.feed(Counts::new(120, 0, 4, 1));
        c.judge() == Ok(Verdict::Reject)
    };
    s.add("J3-超时判不通过", j3, "任一超时即不通过");

    // 裁决复现：同输入重放裁决一致
    let j4 = {
        let c = Counts::new(50, 0, 2, 0);
        let mut a = VerdictBook::new();
        let _ = a.feed(c.clone());
        let _ = a.judge();
        a.replay_matches(&c) == Some(true)
    };
    s.add("J4-裁决复现", j4, "同输入重放裁决复现");

    // 无输入裁决拒（注入核 1）
    let inj = inject_four();
    let j5 = inj[0] && inj[1];
    s.add("J5-注入核前二", j5, "无输入裁决与分歧裁决必检出");
    let j6 = inj[2] && inj[3];
    s.add("J6-注入核后二", j6, "封账再裁与封账再喂必检出");

    // skip 不顶失败：skip 在顶内不影响通过
    let j7 = {
        let mut c = VerdictBook::new();
        let _ = c.feed(Counts::new(10, 0, SKIP_CEILING, 0));
        c.judge() == Ok(Verdict::Pass)
    };
    s.add("J7-skip 不影响", j7, "skip 顶内不影响通过裁定");

    // skip 超顶输入拒
    let j8 = {
        let mut c = VerdictBook::new();
        c.feed(Counts::new(10, 0, SKIP_CEILING + 1, 0)) == Err(VD_NO_INPUT)
    };
    s.add("J8-skip 超顶拒", j8, "skip 超限按无输入口径拒");

    // 裁决编码：通过=1 不通过=0
    let j9 = Verdict::Pass.as_u32() == 1 && Verdict::Reject.as_u32() == 0;
    s.add("J9-编码单调", j9, "通过/不通过编码固定");

    // 批量裁决器与单值裁决一致
    let j10 = {
        let inputs = vec![
            Counts::new(1, 0, 0, 0),
            Counts::new(1, 1, 0, 0),
            Counts::new(1, 0, 0, 1),
            Counts::new(0, 0, 1, 0),
        ];
        judge_batch(&inputs)
            == vec![
                Verdict::Pass,
                Verdict::Reject,
                Verdict::Reject,
                Verdict::Pass,
            ]
    };
    s.add("J10-批量一致", j10, "批量裁决与单值口径一致");

    // 总量口径：输入总量非零
    let j11 = {
        let mut c = VerdictBook::new();
        c.feed(Counts::new(0, 0, 0, 0)) == Err(VD_NO_INPUT)
    };
    s.add("J11-零总量拒", j11, "零总量输入拒");

    // 裁决理由落账非空
    let j12 = {
        let mut c = VerdictBook::new();
        let _ = c.feed(Counts::new(9, 0, 1, 0));
        let _ = c.judge();
        !c.reason.is_empty() && c.reason.contains("pass=9")
    };
    s.add("J12-理由落账", j12, "裁决理由含四计数明细");

    // 裁决指纹：输入变则指纹变
    let j13 = {
        let mut a = VerdictBook::new();
        let _ = a.feed(Counts::new(9, 0, 1, 0));
        let _ = a.judge();
        let mut b = VerdictBook::new();
        let _ = b.feed(Counts::new(9, 1, 1, 0));
        let _ = b.judge();
        a.fold() != b.fold()
    };
    s.add("J13-指纹分辨", j13, "不同输入不同指纹");

    // 分歧注入正路自查：诚实裁决永不分歧
    let j14 = {
        let mut c = VerdictBook::new();
        let _ = c.feed(Counts::new(9, 0, 1, 0));
        let _ = c.judge();
        !c.diverges()
    };
    s.add("J14-诚实无分歧", j14, "诚实裁决不触发分歧检出");

    s.add_dyn(
        "J15-判定指纹落账",
        v.fold() != 0,
        &format!("fold={:#x}", v.fold()),
    );
    let empty = VerdictBook::new().fold();
    s.add_dyn("J16-空册指纹", empty == 0x1b9000u32.wrapping_mul(31), &format!("fold={:#x}", empty));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10372_verdict_flow() {
        let mut v = VerdictBook::new();
        assert!(v.judge() == Err(VD_NO_INPUT));
        assert!(v.feed(Counts::new(5, 0, 0, 0)).is_ok());
        assert!(v.judge() == Ok(Verdict::Pass));
        assert!(v.judge() == Err(VD_SEALED));
        assert!(v.feed(Counts::new(1, 0, 0, 0)) == Err(VD_SEALED));
    }

    #[test]
    fn unx10372_checks_green() {
        let s = run_unx10372_checks();
        assert!(s.all_passed(), "UNX-F10372 判据未全绿");
    }
}
