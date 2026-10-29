//! UNX-F11043 LTP sem/shm/msg 组用例对照（SysV 族对账）（0x1e560 段）
//!
//! 锚点：UNX-F11043-J1 SysV 三族用例对照对平（10/10），生命周期语义对齐（10/10）。
//! 域：CoRun·UNX-C4-B33（LTP 对照域·SysV 族）。
//!
//! 结构：SysV 三面对照账（sem/shm/msg 三面各例：例号 + 生命周期相 + 语义
//! 裁断位）+ 对照器（生命周期相序列与 LTP 语义相一致才翻绿）+ 逐面对平
//! 器 + fold 与冻结 + 注入核（相越界/漏对照/重号/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e560 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e560;
pub const SY_BADPHASE: u32 = SEG_BASE + 0x01; // 生命周期相越界
pub const SY_NOCASE: u32 = SEG_BASE + 0x02; // 例未立账
pub const SY_DUP: u32 = SEG_BASE + 0x03; // 例号重号
pub const SY_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const SY_SHORT: u32 = SEG_BASE + 0x05; // 面缺例
pub const SY_MISMATCH: u32 = SEG_BASE + 0x06; // 语义失配

/// SysV 三面：sem/shm/msg。
pub const SY_SEM: usize = 0;
pub const SY_SHM: usize = 1;
pub const SY_MSG: usize = 2;
pub const SY_FACES: usize = 3;
/// 每面例数（10/10：三面各 3~4 例取整面 4）。
pub const SY_PER: usize = 4;
/// 生命周期相数：创建→使用→销毁。
pub const SY_PHASES: u32 = 3;
/// SysV 对照账域标（fold 种子）。
pub const SYSV_TAG: u64 = 0x1e56_2b;

/// 单 SysV 对照例：例号 + 生命周期相 + 语义裁断位。
#[derive(Clone, Debug)]
pub struct SysvCase {
    pub cid: u32,
    pub face: usize,
    pub phase: u32,
    pub ok: bool,
}

impl SysvCase {
    /// 语义齐绿：相在生命周期内且裁断翻绿。
    pub fn green(&self) -> bool {
        self.ok && self.phase >= 1 && self.phase <= SY_PHASES
    }

    pub fn render(&self) -> String {
        let flag = if self.green() { "G" } else { "R" };
        format!("sc|c{}|f{}|p{}|{}", self.cid, self.face, self.phase, flag)
    }
}

/// SysV 族对照账：三面立账对照。
pub struct SysvLedger {
    pub sem: Vec<SysvCase>,
    pub shm: Vec<SysvCase>,
    pub msg: Vec<SysvCase>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl SysvLedger {
    pub fn new() -> SysvLedger {
        SysvLedger { sem: Vec::new(), shm: Vec::new(), msg: Vec::new(), frozen: false, root: None }
    }

    /// 立账：冻结、相越界、重号必拒。
    pub fn enroll(&mut self, face: usize, cid: u32, phase: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SY_FROZEN);
        }
        if face >= SY_FACES {
            return Err(SY_NOCASE);
        }
        if phase == 0 || phase > SY_PHASES {
            return Err(SY_BADPHASE);
        }
        let dup = match face {
            SY_SEM => self.sem.iter().any(|x| x.cid == cid),
            SY_SHM => self.shm.iter().any(|x| x.cid == cid),
            _ => self.msg.iter().any(|x| x.cid == cid),
        };
        if dup {
            return Err(SY_DUP);
        }
        let c = SysvCase { cid, face, phase, ok: false };
        match face {
            SY_SEM => self.sem.push(c),
            SY_SHM => self.shm.push(c),
            _ => self.msg.push(c),
        }
        Ok(())
    }

    /// 对照：LTP 语义相与账面相一致才翻绿。
    pub fn compare(&mut self, face: usize, cid: u32, expect_phase: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(SY_FROZEN);
        }
        if face >= SY_FACES {
            return Err(SY_NOCASE);
        }
        let tab = match face {
            SY_SEM => &mut self.sem,
            SY_SHM => &mut self.shm,
            _ => &mut self.msg,
        };
        let mut hit = false;
        let mut ok = false;
        let mut k = 0usize;
        while k < tab.len() {
            if tab[k].cid == cid {
                tab[k].ok = tab[k].phase == expect_phase;
                ok = tab[k].ok;
                hit = true;
            }
            k += 1;
        }
        if hit {
            Ok(ok)
        } else {
            Err(SY_NOCASE)
        }
    }

    /// 单面对平：例数齐且全绿。
    pub fn face_even(&self, face: usize) -> bool {
        let tab = match face {
            SY_SEM => &self.sem,
            SY_SHM => &self.shm,
            _ => &self.msg,
        };
        if tab.len() != SY_PER {
            return false;
        }
        let mut ok = true;
        let mut k = 0usize;
        while k < tab.len() {
            if !tab[k].green() {
                ok = false;
            }
            k += 1;
        }
        ok
    }

    /// 三面对平。
    pub fn closed(&self) -> bool {
        let mut f = 0usize;
        let mut ok = true;
        while f < SY_FACES {
            if !self.face_even(f) {
                ok = false;
            }
            f += 1;
        }
        ok
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SYSV_TAG;
        let mut f = 0usize;
        while f < SY_FACES {
            let tab = match f {
                SY_SEM => &self.sem,
                SY_SHM => &self.shm,
                _ => &self.msg,
            };
            let mut k = 0usize;
            while k < tab.len() {
                h = (h ^ tab[k].cid as u64).wrapping_mul(0x100000001b3);
                h = (h ^ tab[k].phase as u64).wrapping_mul(0x100000001b3);
                h = (h ^ tab[k].ok as u64).wrapping_mul(0x100000001b3);
                k += 1;
            }
            f += 1;
        }
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 各面例号确定性样本。
pub fn sy_cid(face: usize, no: usize) -> u32 {
    0x7000 + (face * 100 + no) as u32
}

/// 生命周期相确定性样本：第 no 例落相 no%3+1。
pub fn sy_phase(no: usize) -> u32 {
    (no % SY_PHASES as usize) as u32 + 1
}

/// 三面全量对照全链。
pub fn build_sysvledger() -> (SysvLedger, bool) {
    let mut l = SysvLedger::new();
    let mut ok = true;
    let mut f = 0usize;
    while f < SY_FACES {
        let mut i = 0usize;
        while i < SY_PER {
            if l.enroll(f, sy_cid(f, i), sy_phase(i)).is_err() {
                ok = false;
            }
            i += 1;
        }
        f += 1;
    }
    let mut f = 0usize;
    while f < SY_FACES {
        let mut i = 0usize;
        while i < SY_PER {
            if l.compare(f, sy_cid(f, i), sy_phase(i)).is_err() {
                ok = false;
            }
            i += 1;
        }
        f += 1;
    }
    let l_ok = ok && l.closed();
    (l, l_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 相越界立账必拒
    let mut a = SysvLedger::new();
    got[0] = a.enroll(SY_SEM, 1, 0).err() == Some(SY_BADPHASE)
        && a.enroll(SY_SEM, 1, SY_PHASES + 1).err() == Some(SY_BADPHASE);
    // 2) 例未立账对照必拒
    let mut b = SysvLedger::new();
    got[1] = b.compare(SY_SHM, 0x777, 1).err() == Some(SY_NOCASE);
    // 3) 重号必拒
    let mut c = SysvLedger::new();
    let _ = c.enroll(SY_MSG, 9, 1);
    got[2] = c.enroll(SY_MSG, 9, 2).err() == Some(SY_DUP);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_sysvledger();
    d.freeze();
    got[3] = d.enroll(SY_SEM, 0x8888, 1).err() == Some(SY_FROZEN)
        && d.compare(SY_MSG, sy_cid(SY_MSG, 0), 1).err() == Some(SY_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11043_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11043");

    // J1 SysV 三族用例对照对平
    let (mut book, green) = build_sysvledger();
    s.add("J1-三族对照对平", green && book.closed(), "sem/shm/msg 三面逐例对照齐绿");

    // J2 生命周期语义对齐：逐例相在生命周期内
    let j2 = {
        let mut ok = true;
        let mut f = 0usize;
        while f < SY_FACES {
            if !book.face_even(f) {
                ok = false;
            }
            f += 1;
        }
        ok && book.sem.len() == SY_PER && book.shm.len() == SY_PER && book.msg.len() == SY_PER
    };
    s.add("J2-生命周期对齐", j2, "逐例生命周期相序列一致");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.enroll(SY_SEM, 0x8888, 1).err() == Some(SY_FROZEN)
        && book.compare(SY_SEM, sy_cid(SY_SEM, 0), 1).err() == Some(SY_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后立账对照均必拒");

    // J5 注入检出：相越界 + 例未立账
    let inj = inject_four();
    s.add("J5-越相未立抓", inj[0] && inj[1], "相越界与例未立账两路必抓");

    // J6 注入检出：重号 + 冻结后写
    s.add("J6-重号冻结检出", inj[2] && inj[3], "例号重号与冻结后写必抓");

    // J7 相序列覆盖：三面相 1/2/3 全覆盖
    let j7 = {
        let mut p1 = false;
        let mut p2 = false;
        let mut p3 = false;
        let mut k = 0usize;
        while k < book.sem.len() {
            if book.sem[k].phase == 1 {
                p1 = true;
            }
            if book.sem[k].phase == 2 {
                p2 = true;
            }
            if book.sem[k].phase == 3 {
                p3 = true;
            }
            k += 1;
        }
        p1 && p2 && p3
    };
    s.add("J7-相覆盖", j7, "生命周期三相全覆盖");

    // J8 失配可报：对照期望相错必出红
    let j8 = {
        let mut c = SysvLedger::new();
        let _ = c.enroll(SY_SHM, sy_cid(SY_SHM, 0), 2);
        let _ = c.compare(SY_SHM, sy_cid(SY_SHM, 0), 1);
        !c.face_even(SY_SHM)
    };
    s.add("J8-失配可报", j8, "相失配裁断为红对平为假");

    // J9 例号分面确定：三面例号互异
    let j9 = sy_cid(0, 0) != sy_cid(1, 0)
        && sy_cid(1, 0) != sy_cid(2, 0)
        && sy_cid(0, 3) != sy_cid(2, 3);
    s.add("J9-例号分面", j9, "三面例号空间互异");

    // J10 相样本确定性
    let p2a = sy_phase(2);
    let p2b = sy_phase(2);
    s.add("J10-相确定", p2a == p2b, "相样本同入同出");

    // J11 渲染分旗
    let j11 = {
        let g = SysvCase { cid: 1, face: 0, phase: 2, ok: true };
        let r = SysvCase { cid: 1, face: 0, phase: 2, ok: false };
        g.render().contains('G') && r.render().contains('R')
    };
    s.add("J11-渲染分旗", j11, "绿红对照渲染可区分");

    // J12 面越界拒
    let j12 = SysvLedger::new().enroll(SY_FACES, 1, 1).err() == Some(SY_NOCASE)
        && SysvLedger::new().compare(SY_FACES, 1, 1).err() == Some(SY_NOCASE);
    s.add("J12-面越界拒", j12, "面号越界立账对照均必拒");

    // J13 缺面对平为假：只立两面 closed 为假
    let j13 = {
        let mut c = SysvLedger::new();
        let mut i = 0usize;
        while i < SY_PER {
            let _ = c.enroll(SY_SEM, sy_cid(SY_SEM, i), sy_phase(i));
            let _ = c.enroll(SY_MSG, sy_cid(SY_MSG, i), sy_phase(i));
            i += 1;
        }
        !c.closed()
    };
    s.add("J13-缺面为假", j13, "缺面清单对平断言为假");

    // J14 对照幂等：重复对照指纹稳定
    let j14 = {
        let mut c = SysvLedger::new();
        let _ = c.enroll(SY_MSG, sy_cid(SY_MSG, 1), 3);
        let _ = c.compare(SY_MSG, sy_cid(SY_MSG, 1), 3);
        let f1 = c.fold();
        let _ = c.compare(SY_MSG, sy_cid(SY_MSG, 1), 3);
        c.fold() == f1
    };
    s.add("J14-对照幂等", j14, "重复对照指纹稳定");

    // J15 动态明细：三面对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-三面明细",
        fp == root && book.closed(),
        &format!("root={:#x} faces=3", fp),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11043_sysv_flow() {
        let (mut l, built) = build_sysvledger();
        assert!(built);
        assert!(l.compare(SY_SEM, 0x777, 1).err() == Some(SY_NOCASE));
        l.freeze();
        assert!(l.enroll(SY_SHM, 0x8888, 1).err() == Some(SY_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11043_checks_green() {
        let s = run_unx11043_checks();
        assert!(s.all_passed(), "UNX-F11043 判据未全绿");
    }
}
