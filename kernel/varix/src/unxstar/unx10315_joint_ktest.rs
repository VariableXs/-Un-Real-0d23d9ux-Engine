//! UNX-F10315 联签面 ktest 集成（0x1b1e0 段）
//!
//! 锚点：UNX-F10315-J1 ktest 挂接——ktest 用例逐条绑联签判据，挂接后
//! 全用例执行且全数通过，未挂接/漏执行/败例即集成失败。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：用例册（ktest 用例：号+名+绑判据号登记）+ 挂接核（达锚才许
//! 挂接）+ 执行账（逐用例执行结果落账，重跑即拒）+ 集成裁断（挂接+
//! 全执行+全通过三重）+ 注入核（未挂接/败例/未知用例/重跑四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b1e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b1e0;
pub const KT_DUP: u32 = SEG_BASE + 0x01; // 用例号重号
pub const KT_EMPTY: u32 = SEG_BASE + 0x02; // 用例名空
pub const KT_ZERO: u32 = SEG_BASE + 0x03; // 绑判据号为零
pub const KT_CAP: u32 = SEG_BASE + 0x04; // 册超容
pub const KT_SHORT: u32 = SEG_BASE + 0x05; // 未达锚不许挂接
pub const KT_NOKID: u32 = SEG_BASE + 0x06; // 未知用例执行
pub const KT_RERUN: u32 = SEG_BASE + 0x07; // 已跑再跑

/// 用例册定容。
pub const CAP: usize = 12;
/// 用例数锚。
pub const CASES_NEEDED: usize = 6;

/// 单条 ktest 用例。
#[derive(Clone)]
pub struct KCase {
    pub kid: u32,
    pub name: String,
    pub joint_cid: u32,
}

/// 联签面 ktest 集成架。
pub struct KtestRig {
    pub cases: Vec<KCase>,
    pub results: Vec<(u32, bool)>,
    pub hooked: bool,
}

impl KtestRig {
    pub fn new() -> KtestRig {
        KtestRig { cases: Vec::new(), results: Vec::new(), hooked: false }
    }

    fn pos(&self, kid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].kid == kid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记用例：重号、空名、零绑判据、超容均拒。
    pub fn register(&mut self, kid: u32, name: &str, joint_cid: u32) -> Result<(), u32> {
        if kid == 0 || self.pos(kid).is_some() {
            return Err(KT_DUP);
        }
        if name.is_empty() {
            return Err(KT_EMPTY);
        }
        if joint_cid == 0 {
            return Err(KT_ZERO);
        }
        if self.cases.len() >= CAP {
            return Err(KT_CAP);
        }
        self.cases.push(KCase { kid, name: name.to_string(), joint_cid });
        Ok(())
    }

    /// 挂接：达锚且全用例名合法才许挂接。
    pub fn hook(&mut self) -> Result<(), u32> {
        if self.cases.len() < CASES_NEEDED {
            return Err(KT_SHORT);
        }
        if !self.cases.iter().all(|c| !c.name.is_empty()) {
            return Err(KT_SHORT);
        }
        self.hooked = true;
        Ok(())
    }

    /// 执行用例：未知号与重跑均拒。
    pub fn exec(&mut self, kid: u32, ok: bool) -> Result<(), u32> {
        if self.pos(kid).is_none() {
            return Err(KT_NOKID);
        }
        if self.results.iter().any(|r| r.0 == kid) {
            return Err(KT_RERUN);
        }
        self.results.push((kid, ok));
        Ok(())
    }

    /// 用例已执行。
    pub fn executed(&self, kid: u32) -> bool {
        self.results.iter().any(|r| r.0 == kid)
    }

    /// 集成裁断：挂接 + 全执行 + 全通过。
    pub fn integrated(&self) -> bool {
        self.hooked
            && self.results.len() == self.cases.len()
            && self.results.iter().all(|r| r.1)
    }

    /// 败例清单。
    pub fn failures(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.results.len() {
            if !self.results[k].1 {
                v.push(self.results[k].0);
            }
            k += 1;
        }
        v
    }

    /// 漏执行用例清单。
    pub fn missing(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.cases.len() {
            if !self.executed(self.cases[k].kid) {
                v.push(self.cases[k].kid);
            }
            k += 1;
        }
        v
    }

    /// 绑定面：用例绑判据号清单（按登记序）。
    pub fn bound_cids(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.cases.len() {
            v.push(self.cases[k].joint_cid);
            k += 1;
        }
        v
    }

    /// 架指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b1e00;
        h = h.wrapping_mul(31).wrapping_add(self.cases.len() as u32);
        let mut k = 0usize;
        while k < self.cases.len() {
            h = h.wrapping_mul(31).wrapping_add(self.cases[k].kid);
            h = h.wrapping_mul(31).wrapping_add(self.cases[k].joint_cid);
            let bytes = self.cases[k].name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = h.wrapping_mul(31).wrapping_add(bytes[b] as u32);
                b += 1;
            }
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.results.len() as u32);
        h = h.wrapping_mul(31).wrapping_add(self.hooked as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut r = KtestRig::new();
    let mut n = 1u32;
    while n <= CASES_NEEDED as u32 {
        let nm = format!("case{}", n);
        let _ = r.register(n, &nm, n * 10);
        n += 1;
    }
    // 1) 未挂接不算集成
    got[0] = !r.integrated();
    // 2) 败例必检出
    let _ = r.hook();
    let mut m = 1u32;
    while m <= CASES_NEEDED as u32 {
        let _ = r.exec(m, m != 3);
        m += 1;
    }
    got[1] = r.failures() == [3u32] && !r.integrated();
    // 3) 未知用例执行必拒
    got[2] = r.exec(99, true).err() == Some(KT_NOKID);
    // 4) 重跑必拒
    got[3] = r.exec(1, true).err() == Some(KT_RERUN);
    got
}

/// 批内判据。
pub fn run_unx10315_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10315");

    // J1 正路径：六用例登记挂接全执行全通过
    let mut r = KtestRig::new();
    let mut ok = true;
    let mut n = 1u32;
    while n <= CASES_NEEDED as u32 {
        let nm = format!("联签ktest{}", n);
        if r.register(n, &nm, n * 10).is_err() {
            ok = false;
        }
        n += 1;
    }
    ok = ok && r.hook().is_ok();
    n = 1u32;
    while n <= CASES_NEEDED as u32 {
        if r.exec(n, true).is_err() {
            ok = false;
        }
        n += 1;
    }
    s.add("J1-六用例集成", ok && r.integrated(), "挂接+全执行+全通过三重成立");

    // J2 绑定面与登记序一致
    let j2 = r.bound_cids() == [10u32, 20, 30, 40, 50, 60];
    s.add("J2-绑定面齐", j2, "六用例绑判据号按登记序齐备");

    // J3 注入检出：未挂接 + 败例
    let inj = inject_four();
    s.add("J3-未挂败例检出", inj[0] && inj[1], "未挂接不成集成且败例必检出");

    // J4 注入检出：未知用例 + 重跑
    s.add("J4-未知重跑检出", inj[2] && inj[3], "未知用例必拒且重跑必拒");

    // J5 未达锚不许挂接
    let j5 = {
        let mut c = KtestRig::new();
        let _ = c.register(1, "孤例", 1);
        c.hook().err() == Some(KT_SHORT)
    };
    s.add("J5-短册拒挂接", j5, "用例数不足锚必拒挂接");

    // J6 零绑判据拒
    let j6 = KtestRig::new().register(1, "x", 0).err() == Some(KT_ZERO);
    s.add("J6-零绑拒收", j6, "绑判据号为零必拒");

    // J7 重号用例拒
    let j7 = {
        let mut c = KtestRig::new();
        let _ = c.register(1, "甲", 1);
        c.register(1, "乙", 2).err() == Some(KT_DUP)
    };
    s.add("J7-重号拒收", j7, "用例号重号必拒");

    // J8 空用例名拒
    let j8 = KtestRig::new().register(1, "", 1).err() == Some(KT_EMPTY);
    s.add("J8-空名拒收", j8, "用例名空必拒");

    // J9 漏执行不成集成
    let j9 = {
        let mut c = KtestRig::new();
        let mut n = 1u32;
        while n <= CASES_NEEDED as u32 {
            let nm = format!("m{}", n);
            let _ = c.register(n, &nm, n * 10);
            n += 1;
        }
        let _ = c.hook();
        let mut m = 2u32;
        while m <= CASES_NEEDED as u32 {
            let _ = c.exec(m, true);
            m += 1;
        }
        !c.integrated() && c.missing() == [1u32]
    };
    s.add("J9-漏执不成集", j9, "漏一条执行即集成失败且缺位可枚举");

    // J10 容量边界
    let j10 = {
        let mut c = KtestRig::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            let nm = format!("k{}", n);
            if c.register(n, &nm, n).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.register(999, "over", 1).err() == Some(KT_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原册不损");

    // J11 架指纹随挂接推进且稳定
    let j11 = {
        let mut c = KtestRig::new();
        let mut n = 1u32;
        while n <= CASES_NEEDED as u32 {
            let nm = format!("f{}", n);
            let _ = c.register(n, &nm, n * 10);
            n += 1;
        }
        let f0 = c.fold();
        let _ = c.hook();
        let f1 = c.fold();
        f0 != f1 && c.fold() == f1
    };
    s.add("J11-指纹随挂接", j11, "挂接位入指纹且终态稳定");

    // J12 动态明细：败例零清单
    let fz = r.failures().len();
    s.add_dyn("J12-败例零明细", fz == 0, &format!("failures={}", fz));

    // J13 动态明细：漏执零清单
    let mz = r.missing().len();
    s.add_dyn("J13-漏执零明细", mz == 0, &format!("missing={}", mz));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10315_ktest_flow() {
        let mut r = KtestRig::new();
        assert!(r.register(1, "a", 10).is_ok());
        assert!(r.register(1, "dup", 20).err() == Some(KT_DUP));
        assert!(r.register(2, "", 20).err() == Some(KT_EMPTY));
        assert!(r.register(3, "z", 0).err() == Some(KT_ZERO));
        assert!(r.exec(1, true).is_ok());
        assert!(r.exec(1, true).err() == Some(KT_RERUN));
        assert!(r.exec(9, true).err() == Some(KT_NOKID));
        assert!(r.executed(1) && !r.executed(2));
        assert!(!r.integrated());
    }

    #[test]
    fn unx10315_checks_green() {
        let s = run_unx10315_checks();
        assert!(s.all_passed(), "UNX-F10315 判据未全绿");
    }
}
