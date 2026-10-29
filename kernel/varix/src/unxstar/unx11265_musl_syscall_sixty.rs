//! UNX-F11265 musl 最小 syscall 集实测核验（0x1f410 段）
//! 纯功能模型：60 号最小 syscall 清单逐号登记「F11217 登记态」与「实测态」，
//! 两态一致才算对平；未测号进缺测账，测出与登记相左进偏差账，两账分离。
//! 一致率按千分比给出，100% 且零缺测才算绿。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段（0x1f410 独占 0x10）——注意本卡与 F11265 段号 0x1f410。
pub const SEG_BASE: u32 = 0x1f410;
/// syscall 号越界或未登记。
pub const SY_NR: u32 = SEG_BASE + 0x01;
/// 号重登。
pub const SY_DUP: u32 = SEG_BASE + 0x02;
/// 名空或登记态取值非法。
pub const SY_NAME: u32 = SEG_BASE + 0x03;
/// 未测号（缺测）。
pub const SY_MISS: u32 = SEG_BASE + 0x04;
/// 实测与登记相左。
pub const SY_MISMATCH: u32 = SEG_BASE + 0x05;
/// 封册后写账。
pub const SY_FROZEN: u32 = SEG_BASE + 0x06;
/// 登记表已满。
pub const SY_CAP: u32 = SEG_BASE + 0x07;

/// 清单条数（60 号最小集）。
pub const NEED: usize = 60;
/// 号上界（与定容解耦）。
pub const NR_MAX: u32 = 999;
/// 登记表定容。
pub const CAP: usize = 64;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_65;

/// 单号账：登记态（F11217）与实测态（None=未测）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SysEntry {
    pub nr: u32,
    pub name: String,
    pub reg: bool,
    pub tested: Option<bool>,
}

impl SysEntry {
    pub fn done(&self) -> bool {
        self.tested.is_some()
    }

    pub fn agree(&self) -> bool {
        match self.tested {
            Some(t) => t == self.reg,
            None => false,
        }
    }

    pub fn render(&self) -> String {
        let t = match self.tested {
            Some(true) => "T",
            Some(false) => "F",
            None => "-",
        };
        let r = if self.reg { "1" } else { "0" };
        format!("{:03}|{}|{}|{}", self.nr, self.name, r, t)
    }
}

/// 60 号清单账。
#[derive(Clone, Debug)]
pub struct SysBook {
    pub entries: Vec<SysEntry>,
    pub accepts: u32,
    pub rejects: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl SysBook {
    pub fn new() -> SysBook {
        SysBook { entries: Vec::new(), accepts: 0, rejects: 0, frozen: false, root: None }
    }

    /// 登记一号。判定序：封册 → 号域 → 重号 → 名空 → 定容。
    pub fn enroll(&mut self, nr: u32, name: &str, reg: bool) -> Result<usize, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SY_FROZEN);
        }
        if nr == 0 || nr > NR_MAX {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SY_NR);
        }
        if self.entries.iter().any(|e| e.nr == nr) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SY_DUP);
        }
        if name.is_empty() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SY_NAME);
        }
        if self.entries.len() >= CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SY_CAP);
        }
        self.entries.push(SysEntry {
            nr,
            name: clone_str(name),
            reg,
            tested: None,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.entries.len().wrapping_sub(1))
    }

    /// 实测一号（一号只测一次，重测按重号入账）。
    pub fn probe(&mut self, nr: u32, ok: bool) -> Result<bool, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SY_FROZEN);
        }
        let mut k = 0usize;
        let mut found: Option<(usize, bool, bool)> = None;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                if e.nr == nr {
                    found = Some((k, e.reg, e.tested.is_some()));
                }
            }
            k = k.wrapping_add(1);
        }
        match found {
            Some((idx, reg, already)) => {
                if already {
                    self.rejects = self.rejects.wrapping_add(1);
                    return Err(SY_DUP);
                }
                if let Some(slot) = self.entries.get_mut(idx) {
                    slot.tested = Some(ok);
                }
                self.accepts = self.accepts.wrapping_add(1);
                Ok(ok == reg)
            }
            None => {
                self.rejects = self.rejects.wrapping_add(1);
                Err(SY_NR)
            }
        }
    }

    /// 缺测号清单。
    pub fn missing(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                if !e.done() {
                    v.push(e.nr);
                }
            }
            k = k.wrapping_add(1);
        }
        v
    }

    /// 偏差号清单（已测但与登记相左）。
    pub fn mismatched(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                if e.done() && !e.agree() {
                    v.push(e.nr);
                }
            }
            k = k.wrapping_add(1);
        }
        v
    }

    /// 一致率千分比（分母为登记总数，空账记 0）。
    pub fn permille(&self) -> u32 {
        let n = self.entries.len() as u32;
        if n == 0 {
            return 0;
        }
        let mut a = 0u32;
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                if e.done() && e.agree() {
                    a = a.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        a.wrapping_mul(1000) / n
    }

    /// 绿判：60 号齐、零缺测、零偏差、一致率 1000。
    pub fn green(&self) -> bool {
        self.entries.len() == NEED
            && self.missing().is_empty()
            && self.mismatched().is_empty()
            && self.permille() == 1000
    }

    /// FNV-1a 链指纹：号/名/登记态/实测态序敏感。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                h = (h ^ e.nr as u64).wrapping_mul(FNV_PRIME);
                let b = e.name.as_bytes();
                let mut j = 0usize;
                while j < b.len() {
                    if let Some(x) = b.get(j) {
                        h = (h ^ *x as u64).wrapping_mul(FNV_PRIME);
                    }
                    j = j.wrapping_add(1);
                }
                h = (h ^ e.reg as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ e.done() as u64).wrapping_mul(FNV_PRIME);
                let t = match e.tested {
                    Some(true) => 1u64,
                    Some(false) => 2u64,
                    None => 3u64,
                };
                h = (h ^ t).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h
    }

    /// 封册：根幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.root;
        }
        let r = match self.root {
            Some(v) => v,
            None => self.fold(),
        };
        self.root = Some(r);
        self.frozen = true;
        Some(r)
    }
}

/// 借串入堆。
fn clone_str(s: &str) -> String {
    let mut o = String::new();
    o.push_str(s);
    o
}

/// 登记态样本：7 的倍数登记为未覆盖，其余覆盖。
pub fn sample_reg(nr: u32) -> bool {
    nr % 7 != 0
}

/// 60 号全测账（逐号实测态 = 登记态）。
pub fn full_book() -> SysBook {
    let mut b = SysBook::new();
    let mut i = 1u32;
    while i <= NEED as u32 {
        let _ = b.enroll(i, "SYS_min", sample_reg(i));
        i = i.wrapping_add(1);
    }
    let mut j = 1u32;
    while j <= NEED as u32 {
        let _ = b.probe(j, sample_reg(j));
        j = j.wrapping_add(1);
    }
    b
}

/// 注入核：四路坏样本。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // (a) 号域越界
    let mut a = SysBook::new();
    got[0] = a.enroll(0, "SYS_zero", true).err() == Some(SY_NR);
    // (b) 重号
    let mut b = SysBook::new();
    let _ = b.enroll(1, "SYS_a", true);
    got[1] = b.enroll(1, "SYS_b", true).err() == Some(SY_DUP);
    // (c) 形状违规：名空
    let mut c = SysBook::new();
    got[2] = c.enroll(1, "", true).err() == Some(SY_NAME);
    // (d) 封册后写账
    let mut d = full_book();
    let _ = d.freeze();
    got[3] = d.probe(1, true).err() == Some(SY_FROZEN);
    got
}

/// 60 号 syscall 实测核验判据。
pub fn run_unx11265_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11265");

    // X1 60 号全测全绿
    let x1 = {
        let b = full_book();
        b.green() && b.entries.len() == NEED
    };
    s.add("X1-六十号全绿", x1, "60 号登记齐、逐号实测、零偏差");

    // X2 一致率 1000
    let x2 = {
        let b = full_book();
        b.permille() == 1000 && b.missing().is_empty() && b.mismatched().is_empty()
    };
    s.add("X2-一致率满分", x2, "一致率千分 1000，缺测与偏差双零");

    // X3 偏差立账
    let x3 = {
        let mut b = SysBook::new();
        let mut i = 1u32;
        while i <= NEED as u32 {
            let _ = b.enroll(i, "SYS_min", true);
            i = i.wrapping_add(1);
        }
        let mut j = 1u32;
        while j <= NEED as u32 {
            let ok = j != 4;
            let _ = b.probe(j, ok);
            j = j.wrapping_add(1);
        }
        b.mismatched().len() == 1
            && b.mismatched().first() == Some(&4)
            && b.permille() == 983
            && !b.green()
    };
    s.add("X3-偏差立账", x3, "4 号实测与登记相左，一致率 983");

    // X4 缺测立账
    let x4 = {
        let mut b = SysBook::new();
        let mut i = 1u32;
        while i <= NEED as u32 {
            let _ = b.enroll(i, "SYS_min", true);
            i = i.wrapping_add(1);
        }
        let mut j = 1u32;
        while j < NEED as u32 {
            let _ = b.probe(j, true);
            j = j.wrapping_add(1);
        }
        b.missing().len() == 1 && b.missing().first() == Some(&60) && !b.green()
    };
    s.add("X4-缺测立账", x4, "60 号未测即不绿且缺测可枚举");

    // X5 重号
    let x5 = {
        let mut b = SysBook::new();
        let _ = b.enroll(5, "SYS_a", true);
        b.enroll(5, "SYS_b", true).err() == Some(SY_DUP)
    };
    s.add("X5-重号", x5, "同号二次登记报重号");

    // X6 号域
    let x6 = {
        let mut a = SysBook::new();
        let zero = a.enroll(0, "SYS_z", true).err() == Some(SY_NR);
        let over = a.enroll(NR_MAX.wrapping_add(1), "SYS_o", true).err() == Some(SY_NR);
        zero && over
    };
    s.add("X6-号域", x6, "0 号与 1000 号均越界");

    // X7 名空
    let x7 = SysBook::new().enroll(1, "", true).err() == Some(SY_NAME);
    s.add("X7-名空拒收", x7, "空名号不入账");

    // X8 未登号实测
    let x8 = {
        let mut b = SysBook::new();
        let _ = b.enroll(1, "SYS_a", true);
        b.probe(2, true).err() == Some(SY_NR)
    };
    s.add("X8-未登号实测", x8, "实测未登记号报号不存在");

    // X9 重测
    let x9 = {
        let mut b = SysBook::new();
        let _ = b.enroll(1, "SYS_a", true);
        let _ = b.probe(1, true);
        b.probe(1, true).err() == Some(SY_DUP)
    };
    s.add("X9-重测", x9, "一号二测报重测");

    // X10 定容
    let x10 = {
        let mut b = SysBook::new();
        let mut i = 1u32;
        let mut ok = true;
        while i <= CAP as u32 {
            if b.enroll(i, "SYS_cap", true).is_err() {
                ok = false;
            }
            i = i.wrapping_add(1);
        }
        ok && b.enroll(65, "SYS_over", true).err() == Some(SY_CAP)
    };
    s.add("X10-定容", x10, "64 号后第 65 号报满");

    // X11 指纹确定
    let x11 = {
        let b = full_book();
        let a = b.fold();
        a == b.fold() && a != 0
    };
    s.add("X11-指纹确定", x11, "同账两次指纹一致且非零");

    // X12 封册幂等
    let x12 = {
        let mut b = full_book();
        let r1 = b.freeze();
        let r2 = b.freeze();
        let live = b.fold();
        b.entries.clear();
        let r3 = b.freeze();
        r1 == r2 && r2 == r3 && r1.is_some() && live != b.fold()
    };
    s.add("X12-封册幂等", x12, "二次封册回原根，账变不改根");

    // X13 封册拒写与账行明细
    let (x13, line13) = {
        let mut b = full_book();
        let _ = b.freeze();
        let e1 = b.enroll(61, "SYS_late", true).err() == Some(SY_FROZEN);
        let e2 = b.probe(1, true).err() == Some(SY_FROZEN);
        let line = format!("enroll={} probe={} first={}", e1, e2, match b.entries.first() {
            Some(v) => v.render(),
            None => String::from("-"),
        });
        (e1 && e2 && line.contains("001|SYS_min|1|T"), line)
    };
    s.add_dyn("X13-封册明细", x13, &line13);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11265_flow() {
        let mut b = full_book();
        assert!(b.green());
        assert_eq!(b.entries.len(), 60);
        assert_eq!(b.permille(), 1000);
        assert!(b.missing().is_empty());
        assert!(b.mismatched().is_empty());
        assert_eq!(b.accepts, 120);
        assert_eq!(b.rejects, 0);
        let r1 = b.freeze();
        let r2 = b.freeze();
        assert_eq!(r1, r2);
        assert!(r1.is_some());
        assert!(b.root == r1);
        assert!(b.probe(1, true).err() == Some(SY_FROZEN));
        assert!(b.enroll(61, "SYS_late", true).err() == Some(SY_FROZEN));
    }

    #[test]
    fn unx11265_gates() {
        let mut b = SysBook::new();
        assert!(b.enroll(1, "SYS_a", true).is_ok());
        assert!(b.enroll(1, "SYS_b", true).err() == Some(SY_DUP));
        assert!(b.enroll(0, "SYS_z", true).err() == Some(SY_NR));
        assert!(b.enroll(1000, "SYS_o", true).err() == Some(SY_NR));
        assert!(b.enroll(2, "", true).err() == Some(SY_NAME));
        assert!(b.probe(9, true).err() == Some(SY_NR));
        assert!(b.probe(1, true).is_ok());
        assert!(b.probe(1, true).err() == Some(SY_DUP));
        assert!(b.enroll(2, "SYS_b", false).is_ok());
        let mut i = 3u32;
        while i <= CAP as u32 {
            assert!(b.enroll(i, "SYS_cap", true).is_ok());
            i += 1;
        }
        assert!(b.enroll(65, "SYS_over", true).err() == Some(SY_CAP));
        assert!(b.freeze().is_some());
        assert!(b.enroll(66, "SYS_late", true).err() == Some(SY_FROZEN));
        assert!(b.probe(2, false).err() == Some(SY_FROZEN));
        assert_eq!(b.accepts, 65);
        assert_eq!(b.rejects, 9);
    }

    #[test]
    fn unx11265_checks_green() {
        assert!(run_unx11265_checks().all_passed(), "UNX-F11265 判据未全绿");
    }
}
