//! UNX-F10364 全量回归执行（回归集全量跑批）（0x1b800 段）
//!
//! 锚点：UNX-F10364-J1 全量回归执行——回归集用例（名+类别）全数入册，
//! 判定四型（过/失/跳/超时）逐例执行一例一定不得重跑，跑批全量接口
//! 按册序整批出判，判定越型即拒，全册执行零漏跑方算全量回归走完。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：回归册（用例=名+类别+判定）+ 定音核（一例一判不重跑）+
//! 全量核（整批出判零漏）+ 出数核（四判定计数）+ 注入核（重跑/重名/
//! 越判/短判四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b800 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b800;
pub const FR_ZERO: u32 = SEG_BASE + 0x01; // 用例空名
pub const FR_DUP: u32 = SEG_BASE + 0x02; // 用例重名
pub const FR_KIND: u32 = SEG_BASE + 0x03; // 类别越界
pub const FR_CAP: u32 = SEG_BASE + 0x04; // 册超容
pub const FR_NOCASE: u32 = SEG_BASE + 0x05; // 用例未在册
pub const FR_RERUN: u32 = SEG_BASE + 0x06; // 一例两判
pub const FR_VERDICT: u32 = SEG_BASE + 0x07; // 判定越型
pub const FR_LEN: u32 = SEG_BASE + 0x08; // 整批判表长度不合

/// 用例三类别。
pub const RK_FUNC: u8 = 1;
pub const RK_PFM: u8 = 2;
pub const RK_SEC: u8 = 3;
pub const RK_N: u8 = 4;

/// 判定四型。
pub const V_PASS: u8 = 1;
pub const V_FAIL: u8 = 2;
pub const V_SKIP: u8 = 3;
pub const V_TIMEOUT: u8 = 4;
pub const V_N: u8 = 5;

/// 用例数锚（全量下限）。
pub const REG_NEEDED: usize = 8;
/// 册定容。
pub const CAP: usize = 14;

/// 回归用例：名 + 类别 + 判定。
#[derive(Clone)]
pub struct RegCase {
    pub rid: String,
    pub kind: u8,
    pub executed: bool,
    pub verdict: Option<u8>,
}

impl RegCase {
    /// 已执行？
    pub fn done(&self) -> bool {
        self.executed && self.verdict.is_some()
    }
}

/// 全量回归册。
#[derive(Clone)]
pub struct FullRegression {
    pub cases: Vec<RegCase>,
    pub sealed: bool,
}

impl FullRegression {
    /// 空册。
    pub fn new() -> Self {
        FullRegression {
            cases: Vec::new(),
            sealed: false,
        }
    }

    /// 找用例。
    pub fn find(&self, rid: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.cases.len() {
            if self.cases[i].rid == rid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 录用例。
    pub fn enroll(&mut self, rid: &str, kind: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(FR_RERUN);
        }
        if rid.is_empty() {
            return Err(FR_ZERO);
        }
        if kind < RK_FUNC || kind >= RK_N {
            return Err(FR_KIND);
        }
        if self.cases.len() >= CAP {
            return Err(FR_CAP);
        }
        if self.find(rid).is_some() {
            return Err(FR_DUP);
        }
        self.cases.push(RegCase {
            rid: rid.to_string(),
            kind,
            executed: false,
            verdict: None,
        });
        Ok(())
    }

    /// 单例执行（一次为定）。
    pub fn exec(&mut self, rid: &str, verdict: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(FR_RERUN);
        }
        let idx = match self.find(rid) {
            Some(i) => i,
            None => return Err(FR_NOCASE),
        };
        if verdict < V_PASS || verdict >= V_N {
            return Err(FR_VERDICT);
        }
        if self.cases[idx].done() {
            return Err(FR_RERUN);
        }
        self.cases[idx].executed = true;
        self.cases[idx].verdict = Some(verdict);
        Ok(())
    }

    /// 全量跑批：按册序以判表整批出判，判表须覆盖全部未执行用例。
    pub fn full(&mut self, verdicts: &[u8]) -> Result<usize, u32> {
        if self.sealed {
            return Err(FR_RERUN);
        }
        let mut pending = 0;
        let mut i = 0;
        while i < self.cases.len() {
            if !self.cases[i].done() {
                pending += 1;
            }
            i += 1;
        }
        if verdicts.len() < pending {
            return Err(FR_LEN);
        }
        let mut n = 0;
        let mut vi = 0;
        let mut ci = 0;
        while ci < self.cases.len() {
            if !self.cases[ci].done() {
                let v = verdicts[vi];
                if v < V_PASS || v >= V_N {
                    return Err(FR_VERDICT);
                }
                self.cases[ci].executed = true;
                self.cases[ci].verdict = Some(v);
                vi += 1;
                n += 1;
            }
            ci += 1;
        }
        Ok(n)
    }

    /// 漏跑数。
    pub fn unexecuted(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.cases.len() {
            if !self.cases[i].done() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 四判定计数。
    pub fn counts(&self) -> [usize; 4] {
        let mut c = [0usize; 4];
        let mut i = 0;
        while i < self.cases.len() {
            if let Some(v) = self.cases[i].verdict {
                if v >= V_PASS && v < V_N {
                    c[(v - 1) as usize] += 1;
                }
            }
            i += 1;
        }
        c
    }

    /// 失绿清单。
    pub fn fail_list(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.cases.len() {
            if self.cases[i].verdict == Some(V_FAIL) {
                out.push(self.cases[i].rid.clone());
            }
            i += 1;
        }
        out
    }

    /// 全量走完：达锚且零漏跑。
    pub fn full_ok(&self) -> bool {
        self.cases.len() >= REG_NEEDED && self.unexecuted() == 0
    }

    /// 封账：未走完即拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(FR_RERUN);
        }
        if self.cases.len() < REG_NEEDED {
            return Err(FR_LEN);
        }
        if self.unexecuted() > 0 {
            return Err(FR_LEN);
        }
        self.sealed = true;
        Ok(())
    }

    /// 折叠指纹：判定逐例叠算。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b8001b8;
        let mut i = 0;
        while i < self.cases.len() {
            let v = self.cases[i].verdict.unwrap_or(0) as u32;
            h = (h ^ v).wrapping_mul(0x01000193);
            let bytes = self.cases[i].rid.as_bytes();
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

/// 回归集样例：八例（名, 类别）。
pub fn reg_samples() -> [(&'static str, u8); 8] {
    [
        ("reg-open", RK_FUNC),
        ("reg-read", RK_FUNC),
        ("reg-write", RK_FUNC),
        ("reg-flush", RK_FUNC),
        ("reg-close", RK_FUNC),
        ("reg-lat", RK_PFM),
        ("reg-mem", RK_PFM),
        ("reg-auth", RK_SEC),
    ]
}

/// 全量判表样例：前六过，reg-mem 失，reg-auth 过。
pub fn reg_verdicts() -> [u8; 8] {
    [V_PASS, V_PASS, V_PASS, V_PASS, V_PASS, V_PASS, V_FAIL, V_PASS]
}

/// 注入核：四路坏账必抓。
/// 0: 一例两判；1: 用例重名；2: 判定越型；3: 判表过短。
pub fn inject_four() -> [bool; 4] {
    let mut r = FullRegression::new();
    let mut k = 0;
    while k < reg_samples().len() {
        let (rid, kind) = reg_samples()[k];
        let _ = r.enroll(rid, kind);
        k += 1;
    }
    let _ = r.exec("reg-open", V_PASS);
    let d0 = r.exec("reg-open", V_FAIL).err() == Some(FR_RERUN);
    let d1 = r.enroll("reg-open", RK_FUNC).err() == Some(FR_DUP);
    let d2 = r.exec("reg-read", 9).err() == Some(FR_VERDICT);
    let d3 = r.full(&[V_PASS, V_PASS]).err() == Some(FR_LEN);
    [d0, d1, d2, d3]
}

/// 判据：run_unx10364_checks。
pub fn run_unx10364_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10364");
    // J1 正路径：八例全入册。
    let mut r = FullRegression::new();
    let mut j1 = true;
    let mut k = 0;
    while k < reg_samples().len() {
        let (rid, kind) = reg_samples()[k];
        if r.enroll(rid, kind).is_err() {
            j1 = false;
        }
        k += 1;
    }
    s.add("J1-八例入册", j1 && r.cases.len() == 8, "回归集全数入册");
    // J2 边界：空名拒。
    let mut r2 = FullRegression::new();
    let j2 = r2.enroll("", RK_FUNC).err() == Some(FR_ZERO);
    s.add("J2-空名拒", j2, "用例空名即 FR_ZERO");
    // J3 边界：类别越界拒。
    let mut r3 = FullRegression::new();
    let j3 = r3.enroll("x", 0).err() == Some(FR_KIND)
        && r3.enroll("y", 4).err() == Some(FR_KIND);
    s.add("J3-越类拒", j3, "类别不在 1..=3 即 FR_KIND");
    // J4 注入：一例两判必抓。
    let mut r4 = FullRegression::new();
    let _ = r4.enroll("solo", RK_FUNC);
    let _ = r4.exec("solo", V_PASS);
    let j4 = r4.exec("solo", V_FAIL).err() == Some(FR_RERUN);
    s.add("J4-重跑抓", j4, "一例两判即 FR_RERUN");
    // J5 注入：用例重名必抓。
    let mut r5 = FullRegression::new();
    let _ = r5.enroll("dup", RK_FUNC);
    let j5 = r5.enroll("dup", RK_PFM).err() == Some(FR_DUP);
    s.add("J5-重名抓", j5, "用例重名即 FR_DUP");
    // J6 注入：判定越型必抓。
    let mut r6 = FullRegression::new();
    let _ = r6.enroll("bad", RK_FUNC);
    let j6 = r6.exec("bad", 0).err() == Some(FR_VERDICT)
        && r6.exec("bad", 5).err() == Some(FR_VERDICT);
    s.add("J6-越判抓", j6, "判定不在 1..=4 即 FR_VERDICT");
    // J7 注入：判表过短必抓。
    let mut r7 = FullRegression::new();
    let mut k7 = 0;
    while k7 < reg_samples().len() {
        let (rid, kind) = reg_samples()[k7];
        let _ = r7.enroll(rid, kind);
        k7 += 1;
    }
    let j7 = r7.full(&[V_PASS; 3]).err() == Some(FR_LEN);
    s.add("J7-短表抓", j7, "判表短于待跑即 FR_LEN");
    // J8 正路径：整批出判全量走完。
    let mut ok8 = true;
    let mut k8 = 0;
    while k8 < reg_verdicts().len() {
        let (rid, _) = reg_samples()[k8];
        if r.exec(rid, reg_verdicts()[k8]).is_err() {
            ok8 = false;
        }
        k8 += 1;
    }
    let _ = ok8;
    let j8 = r.full_ok() && r.unexecuted() == 0;
    s.add("J8-全量完", j8, "八例全判零漏跑");
    // J9 正路径：走完封账。
    let mut r9 = r.clone();
    let j9 = r9.seal().is_ok() && r9.sealed;
    s.add("J9-封账成", j9, "全量走完封账");
    // J10 边界：漏跑封账拒。
    let mut r10 = FullRegression::new();
    let mut k10 = 0;
    while k10 < 5 {
        let (rid, kind) = reg_samples()[k10];
        let _ = r10.enroll(rid, kind);
        let _ = r10.exec(rid, V_PASS);
        k10 += 1;
    }
    let j10 = r10.seal().err() == Some(FR_LEN) && r10.unexecuted() == 0;
    s.add("J10-缺例拒", j10, "五例不足锚即 FR_LEN");
    // J11 边界：封账后再判拒。
    let mut r11 = r.clone();
    let _ = r11.seal();
    let j11 = r11.exec("reg-open", V_PASS).err() == Some(FR_RERUN);
    s.add("J11-封后判拒", j11, "封账后执行即 FR_RERUN");
    // J12 动态：四计数出数。
    let c = r.counts();
    let j12 = c[0] == 7 && c[1] == 1 && c[2] == 0 && c[3] == 0;
    s.add_dyn(
        "J12-四计数",
        j12,
        &format!(
            "PASS {} FAIL {} SKIP {} TIMEOUT {}",
            c[0], c[1], c[2], c[3]
        ),
    );
    // J13 动态：失绿清单与指纹。
    let fl = r.fail_list();
    let j13 = fl == vec!["reg-mem".to_string()] && r.fold() != 0;
    s.add_dyn(
        "J13-失绿出数",
        j13,
        &format!(
            "失绿 {} 例：{:?}，指纹 {:08x}",
            fl.len(),
            fl,
            r.fold()
        ),
    );
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "重跑/重名/越判/短表四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_seal() {
        let mut r = FullRegression::new();
        let mut k = 0;
        while k < reg_samples().len() {
            let (rid, kind) = reg_samples()[k];
            assert!(r.enroll(rid, kind).is_ok());
            k += 1;
        }
        // 先单判一例，再整批补齐，验漏跑补位。
        assert!(r.exec("reg-open", V_PASS).is_ok());
        let n = r.full(&reg_verdicts()[1..]);
        assert!(n == Ok(7));
        assert!(r.full_ok());
        assert!(r.seal().is_ok());
        assert!(r.exec("reg-open", V_PASS).err() == Some(FR_RERUN));
        assert!(r.fold() != 0);
        assert_eq!(r.counts()[1], 1);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10364_checks();
        assert!(s.all_passed());
    }
}
