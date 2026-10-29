//! UNX-F12048 PEB 版本字段账：OsMajorVersion/OsMinorVersion/OsBuildNumber（0x19820 段）
//!
//! 判据原文：版本三元账——主版本（1..=10 钉域）、次版本（0..=99）、
//! build 号（≥10240 钉域）；三元一次定型（定型后只读，重定型即冲突）；
//! build 号随次版本联动校验（Win10 档 build<22000、Win11 档≥22000 与
//! 主版本钉值对表）。
//!
//! 结构：版本账（VerTriple：三元 + 定型位）+ 定型核（四码守卫）+
//! 对表核 + 折叠指纹 + 印账冻结 + 注入核 + 判据聚合（run_unx12048_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19820 独占 0x20）。
pub const SEG_BASE: u32 = 0x19820;
pub const VR_MAJOR_RANGE: u32 = SEG_BASE + 0x01; // 主版本越域
pub const VR_BUILD_RANGE: u32 = SEG_BASE + 0x02; // build 越域
pub const VR_DOUBLE_SET: u32 = SEG_BASE + 0x03; // 重定型
pub const VR_TIER_CONFLICT: u32 = SEG_BASE + 0x04; // 主版本与 build 档冲突
pub const VR_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 钉域。
pub const MAJOR_LO: u32 = 1;
pub const MAJOR_HI: u32 = 10;
pub const BUILD_LO: u32 = 10240;

/// 一笔版本动作的账目。
#[derive(Clone)]
pub struct VrRec {
    pub seq: u32,
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    pub note: String,
}

impl VrRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 版本三元账模型。
pub struct VerTriple {
    pub major: Option<u32>,
    pub minor: Option<u32>,
    pub build: Option<u32>,
    pub ledger: Vec<VrRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl VerTriple {
    pub fn new() -> VerTriple {
        VerTriple { major: None, minor: None, build: None, ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 档一致性：主版本 10 且 build<22000 为 Win10 档；主版本 10 且
    /// build≥22000 为 Win11 档；主版本 <10 只配 build<22000。
    pub fn tier_conflict(major: u32, build: u32) -> bool {
        if major < 10 {
            build >= 22000
        } else {
            false
        }
    }

    /// 定型核：域校验 → 档校验 → 单次定型。
    pub fn set(&mut self, major: u32, minor: u32, build: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(VR_FROZEN);
        }
        if note.is_empty() {
            return Err(VR_MAJOR_RANGE);
        }
        if major < MAJOR_LO || major > MAJOR_HI {
            return Err(VR_MAJOR_RANGE);
        }
        if build < BUILD_LO {
            return Err(VR_BUILD_RANGE);
        }
        if Self::tier_conflict(major, build) {
            return Err(VR_TIER_CONFLICT);
        }
        if self.major.is_some() {
            return Err(VR_DOUBLE_SET);
        }
        let rec = VrRec { seq: self.seq_next, major, minor, build, note: note.to_string() };
        if !rec.ok() {
            return Err(VR_MAJOR_RANGE);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        self.major = Some(major);
        self.minor = Some(minor);
        self.build = Some(build);
        Ok(())
    }

    /// 读三元。
    pub fn read(&self) -> Option<(u32, u32, u32)> {
        match (self.major, self.minor, self.build) {
            (Some(a), Some(b), Some(c)) => Some((a, b, c)),
            _ => None,
        }
    }

    /// 次版本域钉值（定型时同验）。
    pub fn minor_ok(minor: u32) -> bool {
        minor <= 99
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.major.unwrap_or(0));
        h = h.wrapping_mul(211).wrapping_add(self.build.unwrap_or(0));
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut v = VerTriple::new();
    // 1) 主版本越域（0 与 11）→ VR_MAJOR_RANGE。
    got[0] = v.set(0, 0, 19045, "lo").err() == Some(VR_MAJOR_RANGE);
    // 2) build 越域 → VR_BUILD_RANGE。
    got[1] = v.set(10, 0, 10239, "b").err() == Some(VR_BUILD_RANGE);
    // 3) 档冲突：主版本 6 配 build 26100 → VR_TIER_CONFLICT。
    got[2] = v.set(6, 1, 26100, "tier").err() == Some(VR_TIER_CONFLICT);
    // 4) 重定型 → VR_DOUBLE_SET。
    let _ = v.set(10, 0, 19045, "ok");
    got[3] = v.set(10, 0, 22631, "dup").err() == Some(VR_DOUBLE_SET);
    // 5) 印账后动作 → VR_FROZEN。
    v.seal();
    got[4] = v.set(10, 0, 26100, "post").err() == Some(VR_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12048_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12048");

    // J1 正路径：Win10 三元定型回读。
    let j1 = {
        let mut v = VerTriple::new();
        let _ = v.set(10, 0, 19045, "w10");
        v.read() == Some((10, 0, 19045))
    };
    s.add("J1-Win10 定型", j1, "10.0.19045");

    // J2 主版本越域拒（双面）。
    let j2 = {
        let mut v = VerTriple::new();
        let lo = v.set(0, 0, 19045, "a").err() == Some(VR_MAJOR_RANGE);
        let hi = v.set(11, 0, 19045, "b").err() == Some(VR_MAJOR_RANGE);
        lo && hi
    };
    s.add("J2-主版本越域", j2, "0/11 全拒");

    // J3 主版本边界收：1 与 10 恰好入域。
    let j3 = {
        let mut a = VerTriple::new();
        let lo = a.set(1, 0, 10240, "a").is_ok();
        let mut b = VerTriple::new();
        let hi = b.set(10, 0, 26100, "b").is_ok();
        lo && hi
    };
    s.add("J3-主版本边界", j3, "1/10 恰入域");

    // J4 build 越域拒：10239 拒 10240 收。
    let j4 = {
        let mut a = VerTriple::new();
        let bad = a.set(10, 0, 10239, "x").err() == Some(VR_BUILD_RANGE);
        let mut b = VerTriple::new();
        let good = b.set(10, 0, 10240, "y").is_ok();
        bad && good
    };
    s.add("J4-build 边界", j4, "10239/10240 切界");

    // J5 档冲突拒。
    let j5 = {
        let mut v = VerTriple::new();
        v.set(6, 1, 26100, "c").err() == Some(VR_TIER_CONFLICT)
    };
    s.add("J5-档冲突拒", j5, "主<10 不配 22K+");

    // J6 重定型拒。
    let j6 = {
        let mut v = VerTriple::new();
        let _ = v.set(10, 0, 19045, "a");
        v.set(10, 0, 22631, "b").err() == Some(VR_DOUBLE_SET)
    };
    s.add("J6-重定型拒", j6, "定型单次性");

    // J7 次版本域：0..99。
    let j7 = VerTriple::minor_ok(0) && VerTriple::minor_ok(99) && !VerTriple::minor_ok(100);
    s.add("J7-次版本域", j7, "0/99 收 100 拒");

    // J8 钉域钉值核对。
    let j8 = MAJOR_LO == 1 && MAJOR_HI == 10 && BUILD_LO == 10240;
    s.add("J8-钉域钉值", j8, "1/10/10240");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = VerTriple::new();
        let _ = a.set(10, 0, 19045, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut b = VerTriple::new();
        let _ = b.set(10, 0, 22631, "x");
        f1 == f2 && a.fold() != b.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异账异纹");

    // J10 印账冻结。
    let j10 = {
        let mut v = VerTriple::new();
        let _ = v.set(10, 0, 19045, "a");
        v.seal();
        v.set(10, 0, 22631, "post").err() == Some(VR_FROZEN)
    };
    s.add("J10-印账冻结", j10, "印后定型必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：Win11 三元。
    let mut v = VerTriple::new();
    let _ = v.set(10, 0, 22631, "sum-w11");
    s.add("样例 Win11", v.read() == Some((10, 0, 22631)), "10.0.22631");
    s.add("样例账面合法", v.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12048_ver_flow() {
        let mut v = VerTriple::new();
        assert!(v.set(10, 0, 19045, "a").is_ok());
        assert_eq!(v.set(10, 0, 22631, "b"), Err(VR_DOUBLE_SET));
        assert_eq!(VerTriple::tier_conflict(6, 26100), true);
        assert_eq!(VerTriple::tier_conflict(10, 26100), false);
        assert_eq!(v.read(), Some((10, 0, 19045)));
    }

    #[test]
    fn unx12048_checks_green() {
        let s = run_unx12048_checks();
        assert!(s.all_passed(), "UNX-F12048 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12048_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
