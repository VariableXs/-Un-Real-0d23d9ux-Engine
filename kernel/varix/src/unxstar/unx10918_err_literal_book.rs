//! UNX-F10918 复用器错误码字面总对账（0x1d240 段）
//!
//! 锚点：UNX-F10918-J1 正面：EBADF/EINVAL/EFAULT/EINTR/EPERM/ENOMEM/
//! POLLNVAL 七面字面对照（各 10/10）；反面：字面零偏离（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·字面对账面）。
//!
//! 结构：字面册（七错误面名→诊断码定容登记）+ 对账器（字面逐面比对
//! + 面序稳定）+ 注入核（面名漂移/面码错配/重面/缺面四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d240 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d240;
pub const LT_MISMATCH: u32 = SEG_BASE + 0x01; // 字面错配
pub const LT_DUP: u32 = SEG_BASE + 0x02; // 重面
pub const LT_MISSING: u32 = SEG_BASE + 0x03; // 缺面
pub const LT_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改册
pub const LT_BADNAME: u32 = SEG_BASE + 0x05; // 面名非法
pub const LT_ORDER: u32 = SEG_BASE + 0x06; // 面序漂移

/// 七错误面标准面序（字面总对账基准）。
pub const FACES_STD: [&str; 7] = [
    "EBADF", "EINVAL", "EFAULT", "EINTR", "EPERM", "ENOMEM", "POLLNVAL",
];
/// 字面册定容。
pub const CAP: usize = 12;
/// LT 域标（fold 种子）。
pub const LT_TAG: u64 = 0x1d24212;

/// 字面册条目。
#[derive(Clone, Debug)]
pub struct FaceEntry {
    pub name: String,
    pub code: u32,
}

impl FaceEntry {
    pub fn render(&self) -> String {
        format!("{}|{:#x}", self.name, self.code)
    }
}

/// 字面总对账册。
pub struct LiteralBook {
    pub faces: Vec<FaceEntry>,
    pub frozen: bool,
}

impl LiteralBook {
    pub fn new() -> LiteralBook {
        LiteralBook { faces: Vec::new(), frozen: false }
    }

    /// 面名合法性：必须落在标准七面内。
    pub fn name_ok(name: &str) -> bool {
        let mut k = 0usize;
        while k < FACES_STD.len() {
            if FACES_STD[k] == name {
                return true;
            }
            k += 1;
        }
        false
    }

    /// 面码合法性：必须在段内 0x01..=0x0f。
    pub fn code_ok(code: u32) -> bool {
        code >= SEG_BASE + 0x01 && code <= SEG_BASE + 0x0f
    }

    /// 登记面：冻结、重面、坏名、坏码均拒。
    pub fn enroll(&mut self, name: &str, code: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(LT_FROZEN);
        }
        if !Self::name_ok(name) {
            return Err(LT_BADNAME);
        }
        if !Self::code_ok(code) {
            return Err(LT_MISMATCH);
        }
        if self.faces.iter().any(|e| e.name == name) {
            return Err(LT_DUP);
        }
        if self.faces.len() >= CAP {
            return Err(LT_MISSING);
        }
        self.faces.push(FaceEntry { name: name.to_string(), code });
        Ok(())
    }

    /// 对账：标准七面逐面比对（名字序与面码双对）。
    pub fn reconcile(&self) -> Result<usize, u32> {
        if self.faces.len() != FACES_STD.len() {
            return Err(LT_MISSING);
        }
        let mut ok = 0usize;
        let mut k = 0usize;
        while k < FACES_STD.len() {
            if self.faces[k].name != FACES_STD[k] {
                return Err(LT_ORDER);
            }
            if !Self::code_ok(self.faces[k].code) {
                return Err(LT_MISMATCH);
            }
            ok += 1;
            k += 1;
        }
        Ok(ok)
    }

    /// 面码查询。
    pub fn code_of(&self, name: &str) -> Result<u32, u32> {
        let mut k = 0usize;
        while k < self.faces.len() {
            if self.faces[k].name == name {
                return Ok(self.faces[k].code);
            }
            k += 1;
        }
        Err(LT_MISSING)
    }

    /// 链式 fold 指纹（序敏感，含面名与面码）。
    pub fn fold(&self) -> u64 {
        let mut h = LT_TAG;
        let mut k = 0usize;
        while k < self.faces.len() {
            let e = &self.faces[k];
            h = (h ^ e.code as u64).wrapping_mul(0x100000001b3);
            let bytes = e.name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(0x100000001b3);
                b += 1;
            }
            k += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 装入标准七面（面码按面序 +1）。
pub fn load_std_book() -> LiteralBook {
    let mut b = LiteralBook::new();
    let mut k = 0usize;
    while k < FACES_STD.len() {
        let _ = b.enroll(FACES_STD[k], SEG_BASE + (k as u32 + 1));
        k += 1;
    }
    b
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 面名漂移（换名）必抓
    let mut b = load_std_book();
    b.faces[0].name = "EBADF2".to_string();
    got[0] = b.reconcile().err() == Some(LT_ORDER);
    // 2) 面码错配必抓
    let mut c = load_std_book();
    c.faces[1].code = 0x999;
    got[1] = c.reconcile().err() == Some(LT_MISMATCH);
    // 3) 重面必拒
    let mut d = load_std_book();
    got[2] = d.enroll("EBADF", SEG_BASE + 1).err() == Some(LT_DUP);
    // 4) 冻结后登记必抓
    let mut f = LiteralBook::new();
    f.freeze();
    got[3] = f.enroll("EBADF", SEG_BASE + 1).err() == Some(LT_FROZEN);
    got
}

/// 面码确定性样本：第 k 面的标准面码。
pub fn lt_sample_code(k: u32) -> u32 {
    SEG_BASE.wrapping_add(k % 7 + 1)
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10918_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10918");

    // J1 正面：七面字面对照全绿（十轮全册重建比对）
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let b = load_std_book();
        if b.reconcile() == Ok(7) && b.faces.len() == 7 {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-七面对照十连", h1 == 10, "七面字面十轮全绿");

    // J2 面序稳定：标准面序逐面比对
    let j2 = {
        let b = load_std_book();
        let mut seq_ok = true;
        let mut k = 0usize;
        while k < FACES_STD.len() {
            if b.faces[k].name != FACES_STD[k] {
                seq_ok = false;
            }
            k += 1;
        }
        seq_ok && b.reconcile() == Ok(7)
    };
    s.add("J2-面序稳定", j2, "七面面序逐面一致");

    // J3 反面：字面零偏离千次
    let mut ok = 0u32;
    let mut r3 = 0u32;
    while r3 < 1000 {
        let b = load_std_book();
        let c = lt_sample_code(r3 % 7);
        if b.code_of(FACES_STD[(r3 % 7) as usize]) == Ok(c) {
            ok += 1;
        }
        r3 += 1;
    }
    s.add("J3-千次零偏离", ok == 1000, "千轮面码查询与标准一致");

    // J4 注入四路全抓
    let inj = inject_four();
    s.add("J4-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "换名/错码/重面/冻结全数命中");

    // J5 坏面名拒
    let j5 = LiteralBook::new().enroll("EFOO", SEG_BASE + 1).err() == Some(LT_BADNAME);
    s.add("J5-坏名拒收", j5, "标准七面外名字必拒");

    // J6 坏面码拒
    let j6 = {
        let mut b = LiteralBook::new();
        b.enroll("EBADF", SEG_BASE + 0x10).err() == Some(LT_MISMATCH)
            && b.enroll("EBADF", SEG_BASE).err() == Some(LT_MISMATCH)
    };
    s.add("J6-坏码拒收", j6, "段外面码必拒");

    // J7 缺面对账拒：只装六面必报缺面
    let j7 = {
        let mut b = LiteralBook::new();
        let mut k = 0usize;
        while k < 6 {
            let _ = b.enroll(FACES_STD[k], SEG_BASE + k as u32 + 1);
            k += 1;
        }
        b.reconcile().err() == Some(LT_MISSING)
    };
    s.add("J7-缺面拒", j7, "面数不足对账必报缺面");

    // J8 重面拒收
    let j8 = {
        let mut b = LiteralBook::new();
        let _ = b.enroll("EINTR", SEG_BASE + 4);
        b.enroll("EINTR", SEG_BASE + 4).err() == Some(LT_DUP)
    };
    s.add("J8-重面拒收", j8, "同面名二次登记必拒");

    // J9 冻结后登记拒
    let j9 = {
        let mut b = LiteralBook::new();
        b.freeze();
        b.enroll("EPERM", SEG_BASE + 5).err() == Some(LT_FROZEN)
    };
    s.add("J9-冻结拒登记", j9, "冻结后登记必拒");

    // J10 面码查询缺失面拒
    let j10 = load_std_book().code_of("ENOENT").err() == Some(LT_MISSING);
    s.add("J10-缺失查询拒", j10, "查标准外面名必报缺面");

    // J11 渲染面：面名与面码渲染可区分
    let j11 = {
        let a = FaceEntry { name: "EBADF".to_string(), code: SEG_BASE + 1 };
        let b = FaceEntry { name: "EINVAL".to_string(), code: SEG_BASE + 2 };
        a.render() != b.render() && a.render().contains("EBADF")
    };
    s.add("J11-渲染分面", j11, "面渲染可区分");

    // J12 fold 稳定：换码必变、同态一致
    let j12 = {
        let mut b = load_std_book();
        let f0 = b.fold();
        let f1 = b.fold();
        b.faces[2].code = b.faces[2].code.wrapping_add(1);
        f0 == f1 && f0 != b.fold() && f0 != 0
    };
    s.add("J12-指纹随码变", j12, "面码入指纹且同态稳定");

    // J13 动态明细：千次零偏离计数落账
    s.add_dyn("J13-千次明细", ok == 1000, &format!("literal_ok={}/1000", ok));

    // J14 动态明细：七面指纹十六进制落账
    let fp = load_std_book().fold();
    s.add_dyn("J14-七面指纹", fp != 0, &format!("book_fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10918_literalbook_flow() {
        let mut b = load_std_book();
        assert_eq!(b.reconcile(), Ok(7));
        assert_eq!(b.code_of("EFAULT"), Ok(SEG_BASE + 3));
        assert!(b.code_of("ENOENT").err() == Some(LT_MISSING));
        assert!(b.enroll("EBADF", SEG_BASE + 1).err() == Some(LT_DUP));
        assert!(b.enroll("EFOO", SEG_BASE + 1).err() == Some(LT_BADNAME));
        b.faces[0].name = "X".to_string();
        assert!(b.reconcile().err() == Some(LT_ORDER));
        b.freeze();
        assert!(b.enroll("ENOMEM", SEG_BASE + 6).err() == Some(LT_FROZEN));
    }

    #[test]
    fn unx10918_checks_green() {
        let s = run_unx10918_checks();
        assert!(s.all_passed(), "UNX-F10918 判据未全绿");
    }
}
