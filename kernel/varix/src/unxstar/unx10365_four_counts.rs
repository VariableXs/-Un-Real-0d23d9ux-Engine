//! UNX-F10365 回归结果四计数（PASS/FAIL/SKIP/TIMEOUT+EXEMPT 全量出数）（0x1b820 段）
//!
//! 锚点：UNX-F10365-J1 回归结果四计数——四判定（过/失/跳/超时）逐例
//! 计数入桶，豁免单独立账（名单唯一且不超帽），四桶计数与计划出数逐
//! 桶对平，有桶不符或豁免超帽即出数失败，四桶对平豁免在帽方算全量出
//! 数成立。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：四桶账（判定计数器）+ 计划核（与计划出数逐桶对平）+ 豁免账
//! （名单唯一不超帽）+ 出数核（四桶+豁免全出）+ 注入核（越型/重豁免/
//! 超帽/计数不符四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b820 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b820;
pub const FC_VERDICT: u32 = SEG_BASE + 0x01; // 判定越型
pub const FC_ZERO: u32 = SEG_BASE + 0x02; // 豁免空名
pub const FC_DUP: u32 = SEG_BASE + 0x03; // 豁免重名
pub const FC_EXCAP: u32 = SEG_BASE + 0x04; // 豁免超帽
pub const FC_NOPLAN: u32 = SEG_BASE + 0x05; // 未设计划出数
pub const FC_MISMATCH: u32 = SEG_BASE + 0x06; // 计数与计划不符
pub const FC_SEALED: u32 = SEG_BASE + 0x07; // 封账后再计

/// 判定四型。
pub const V_PASS: u8 = 1;
pub const V_FAIL: u8 = 2;
pub const V_SKIP: u8 = 3;
pub const V_TIMEOUT: u8 = 4;
pub const V_N: u8 = 5;

/// 豁免帽。
pub const EXEMPT_CAP: usize = 2;
/// 豁免账定容。
pub const CAP: usize = 8;

/// 四计数账。
#[derive(Clone)]
pub struct FourCounts {
    pub pass: u32,
    pub fail: u32,
    pub skip: u32,
    pub timeout: u32,
    pub exempt: Vec<String>,
    pub plan: Option<[u32; 4]>,
    pub sealed: bool,
}

impl FourCounts {
    /// 空账。
    pub fn new() -> Self {
        FourCounts {
            pass: 0,
            fail: 0,
            skip: 0,
            timeout: 0,
            exempt: Vec::new(),
            plan: None,
            sealed: false,
        }
    }

    /// 记一判。
    pub fn record(&mut self, verdict: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(FC_SEALED);
        }
        match verdict {
            V_PASS => self.pass += 1,
            V_FAIL => self.fail += 1,
            V_SKIP => self.skip += 1,
            V_TIMEOUT => self.timeout += 1,
            _ => return Err(FC_VERDICT),
        }
        Ok(())
    }

    /// 设计划出数。
    pub fn set_plan(&mut self, plan: [u32; 4]) -> Result<(), u32> {
        if self.sealed {
            return Err(FC_SEALED);
        }
        self.plan = Some(plan);
        Ok(())
    }

    /// 豁免挂名（唯一不超帽）。
    pub fn exempt_entry(&mut self, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(FC_SEALED);
        }
        if name.is_empty() {
            return Err(FC_ZERO);
        }
        if self.exempt.len() >= EXEMPT_CAP || self.exempt.len() >= CAP {
            return Err(FC_EXCAP);
        }
        let mut i = 0;
        while i < self.exempt.len() {
            if self.exempt[i] == name {
                return Err(FC_DUP);
            }
            i += 1;
        }
        self.exempt.push(name.to_string());
        Ok(())
    }

    /// 四桶总数。
    pub fn total(&self) -> u32 {
        self.pass + self.fail + self.skip + self.timeout
    }

    /// 四桶快照。
    pub fn snapshot(&self) -> [u32; 4] {
        [self.pass, self.fail, self.skip, self.timeout]
    }

    /// 与计划逐桶对平。
    pub fn matches_plan(&self) -> bool {
        match self.plan {
            Some(p) => {
                let mut i = 0;
                while i < 4 {
                    if self.snapshot()[i] != p[i] {
                        return false;
                    }
                    i += 1;
                }
                true
            }
            None => false,
        }
    }

    /// 出数成立：计划在册对平且豁免在帽。
    pub fn out_ok(&self) -> bool {
        self.matches_plan() && self.exempt.len() <= EXEMPT_CAP
    }

    /// 封账：出数不成即拒（缺计划/不符分诊）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(FC_SEALED);
        }
        if self.plan.is_none() {
            return Err(FC_NOPLAN);
        }
        if !self.matches_plan() {
            return Err(FC_MISMATCH);
        }
        self.sealed = true;
        Ok(())
    }

    /// 折叠指纹：四桶+豁免名单叠算。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b8201b8;
        h = (h ^ self.pass).wrapping_mul(0x01000193);
        h = (h ^ self.fail).wrapping_mul(0x01000193);
        h = (h ^ self.skip).wrapping_mul(0x01000193);
        h = (h ^ self.timeout).wrapping_mul(0x01000193);
        let mut i = 0;
        while i < self.exempt.len() {
            let bytes = self.exempt[i].as_bytes();
            let mut j = 0;
            while j < bytes.len() {
                h = (h ^ (bytes[j] as u32)).wrapping_mul(0x01000193);
                j += 1;
            }
            i += 1;
        }
        h
    }
}

/// 计划出数样例：PASS 8 / FAIL 2 / SKIP 1 / TIMEOUT 1。
pub fn plan_counts() -> [u32; 4] {
    [8, 2, 1, 1]
}

/// 计数流水样例：与计划同序重排（2 失 + 1 跳 + 1 超时穿插 8 过）。
pub fn record_stream() -> [u8; 12] {
    [
        V_PASS, V_PASS, V_FAIL, V_PASS, V_PASS, V_SKIP, V_PASS, V_FAIL, V_PASS, V_TIMEOUT,
        V_PASS, V_PASS,
    ]
}

/// 豁免样例名单。
pub fn exempt_samples() -> [&'static str; 1] {
    ["perf-豁免-周期抖动"]
}

/// 注入核：四路坏账必抓。
/// 0: 判定越型；1: 豁免重名；2: 豁免超帽；3: 计数不符封账拒。
pub fn inject_four() -> [bool; 4] {
    let mut f = FourCounts::new();
    let _ = f.set_plan(plan_counts());
    let mut k = 0;
    while k < record_stream().len() {
        let _ = f.record(record_stream()[k]);
        k += 1;
    }
    let _ = f.exempt_entry(exempt_samples()[0]);
    let d0 = f.record(9).err() == Some(FC_VERDICT);
    let d1 = f.exempt_entry(exempt_samples()[0]).err() == Some(FC_DUP);
    let d2 = {
        let mut cap = FourCounts::new();
        let _ = cap.exempt_entry("甲");
        let _ = cap.exempt_entry("乙");
        cap.exempt_entry("丙").err() == Some(FC_EXCAP)
    };
    let d3 = {
        let mut bad = FourCounts::new();
        let _ = bad.set_plan([1, 1, 1, 1]);
        let _ = bad.record(V_PASS);
        let _ = bad.record(V_FAIL);
        let _ = bad.record(V_SKIP);
        let _ = bad.record(V_PASS);
        bad.seal().err() == Some(FC_MISMATCH)
    };
    [d0, d1, d2, d3]
}

/// 判据：run_unx10365_checks。
pub fn run_unx10365_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10365");
    // J1 正路径：设计划出数。
    let mut f = FourCounts::new();
    let j1 = f.set_plan(plan_counts()).is_ok() && f.plan.is_some();
    s.add("J1-计划在册", j1, "四桶计划 8/2/1/1 在册");
    // J2 正路径：流水逐判入桶。
    let mut j2 = true;
    let mut k = 0;
    while k < record_stream().len() {
        if f.record(record_stream()[k]).is_err() {
            j2 = false;
        }
        k += 1;
    }
    s.add("J2-流水入桶", j2 && f.total() == 12, "十二判全入四桶");
    // J3 注入：判定越型必抓。
    let mut f3 = FourCounts::new();
    let j3 = f3.record(0).err() == Some(FC_VERDICT) && f3.record(5).err() == Some(FC_VERDICT);
    s.add("J3-越型抓", j3, "判定不在 1..=4 即 FC_VERDICT");
    // J4 正路径：豁免挂名在帽。
    let mut j4 = true;
    let mut k4 = 0;
    while k4 < exempt_samples().len() {
        if f.exempt_entry(exempt_samples()[k4]).is_err() {
            j4 = false;
        }
        k4 += 1;
    }
    s.add("J4-豁免在帽", j4 && f.exempt.len() == 1, "豁免一名在帽");
    // J5 注入：豁免重名必抓。
    let mut f5 = FourCounts::new();
    let _ = f5.exempt_entry("重名样本");
    let j5 = f5.exempt_entry("重名样本").err() == Some(FC_DUP);
    s.add("J5-重豁抓", j5, "豁免重名即 FC_DUP");
    // J6 注入：豁免超帽必抓。
    let mut f6 = FourCounts::new();
    let _ = f6.exempt_entry("豁一");
    let _ = f6.exempt_entry("豁二");
    let j6 = f6.exempt_entry("豁三").err() == Some(FC_EXCAP);
    s.add("J6-超帽抓", j6, "第三笔豁免即 FC_EXCAP");
    // J7 边界：豁免空名拒。
    let mut f7 = FourCounts::new();
    let j7 = f7.exempt_entry("").err() == Some(FC_ZERO);
    s.add("J7-空名拒", j7, "豁免空名即 FC_ZERO");
    // J8 正路径：四桶与计划逐桶对平。
    let j8 = f.matches_plan() && f.out_ok();
    s.add("J8-对平成", j8, "四桶 8/2/1/1 逐桶同");
    // J9 正路径：出数成立封账。
    let mut f9 = f.clone();
    let j9 = f9.seal().is_ok() && f9.sealed;
    s.add("J9-封账成", j9, "出数成立封账");
    // J10 注入：计数不符封账拒。
    let mut f10 = FourCounts::new();
    let _ = f10.set_plan([1, 1, 1, 1]);
    let _ = f10.record(V_PASS);
    let _ = f10.record(V_FAIL);
    let _ = f10.record(V_SKIP);
    let _ = f10.record(V_PASS);
    let j10 = f10.seal().err() == Some(FC_MISMATCH) && f10.snapshot()[3] == 0;
    s.add("J10-不符拒", j10, "超时桶缺一即 FC_MISMATCH");
    // J11 边界：未设计划封账拒。
    let mut f11 = FourCounts::new();
    let _ = f11.record(V_PASS);
    let j11 = f11.seal().err() == Some(FC_NOPLAN);
    s.add("J11-无计拒", j11, "未设计划即 FC_NOPLAN");
    // J12 边界：封账后再计拒。
    let mut f12 = f.clone();
    let _ = f12.seal();
    let j12 = f12.record(V_PASS).err() == Some(FC_SEALED)
        && f12.exempt_entry("封后").err() == Some(FC_SEALED);
    s.add("J12-封后拒", j12, "封账后计/豁即 FC_SEALED");
    // J13 动态：四桶快照出数与指纹。
    let snap = f.snapshot();
    let j13 = snap == [8, 2, 1, 1] && f.fold() != 0;
    s.add_dyn(
        "J13-快照出数",
        j13,
        &format!(
            "四桶 {:?} + 豁免 {} 笔，指纹 {:08x}",
            snap,
            f.exempt.len(),
            f.fold()
        ),
    );
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "越型/重豁/超帽/不符四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_seal() {
        let mut f = FourCounts::new();
        assert!(f.set_plan(plan_counts()).is_ok());
        let mut k = 0;
        while k < record_stream().len() {
            assert!(f.record(record_stream()[k]).is_ok());
            k += 1;
        }
        assert!(f.exempt_entry(exempt_samples()[0]).is_ok());
        assert!(f.exempt_entry(exempt_samples()[0]).err() == Some(FC_DUP));
        assert!(f.out_ok());
        assert!(f.seal().is_ok());
        assert!(f.record(V_PASS).err() == Some(FC_SEALED));
        assert!(f.fold() != 0);
        assert_eq!(f.snapshot(), [8, 2, 1, 1]);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10365_checks();
        assert!(s.all_passed());
    }
}
