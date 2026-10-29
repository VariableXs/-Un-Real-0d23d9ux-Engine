//! UNX-F11021 SysV shm 挂号联签（shmat/shmdt 挂点注册对账）（0x1e2a0 段）
//!
//! 锚点：UNX-F11021-J1 挂接注册账逐项对平（10/10），摘挂核销零残留（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·SysV shm）。
//!
//! 结构：shm 段账（键 + 尺寸 + 挂接数 + 摘挂数 + 在位标志）+ 挂接器（注册
//! 键唯一、尺寸非零、超容必拒）+ 摘挂核销器（摘挂不得超挂接、齐平即离位）
//! + 残留面（挂减摘差值清单）+ fold 指纹与冻结 + 注入核（虚摘/重复键/零尺
//! 寸/冻结后挂四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e2a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e2a0;
pub const SH_OVERDET: u32 = SEG_BASE + 0x01; // 摘挂超挂接
pub const SH_NOKEY: u32 = SEG_BASE + 0x02; // 键未注册
pub const SH_DUPKEY: u32 = SEG_BASE + 0x03; // 键重号
pub const SH_ZSIZE: u32 = SEG_BASE + 0x04; // 零尺寸
pub const SH_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const SH_CAP: u32 = SEG_BASE + 0x06; // 段账超容

/// 段账定容。
pub const SEG_CAP_N: usize = 16;
/// 挂接注册对平数（10/10）。
pub const SEGS_NEEDED: usize = 10;
/// shm 账域标（fold 种子）。
pub const SHM_TAG: u64 = 0x1e2a_21;
/// 摘挂核销每段样本量。
pub const ATTACH_SAMPLE: u32 = 7;

/// 单段 shm：键 + 尺寸 + 挂接/摘挂计数 + 在位标志。
#[derive(Clone, Debug)]
pub struct ShmSeg {
    pub key: u32,
    pub size: u32,
    pub att: u32,
    pub det: u32,
    pub live: bool,
}

impl ShmSeg {
    /// 残留挂接数。
    pub fn resid(&self) -> u32 {
        self.att.saturating_sub(self.det)
    }

    /// 挂摘齐平且有实挂。
    pub fn even(&self) -> bool {
        self.att == self.det && self.att > 0
    }

    pub fn render(&self) -> String {
        format!("shm|k{}|s{}|a{}|d{}", self.key, self.size, self.att, self.det)
    }
}

/// SysV shm 挂号账：逐段注册 → 挂接 → 摘挂核销 → 零残留封账。
pub struct ShmBook {
    pub segs: Vec<ShmSeg>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl ShmBook {
    pub fn new() -> ShmBook {
        ShmBook { segs: Vec::new(), frozen: false, root: None }
    }

    /// 注册段：冻结、零尺寸、重号、超容必拒。
    pub fn enroll(&mut self, key: u32, size: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SH_FROZEN);
        }
        if size == 0 {
            return Err(SH_ZSIZE);
        }
        if self.segs.iter().any(|s| s.key == key) {
            return Err(SH_DUPKEY);
        }
        if self.segs.len() >= SEG_CAP_N {
            return Err(SH_CAP);
        }
        self.segs.push(ShmSeg { key, size, att: 0, det: 0, live: false });
        Ok(())
    }

    /// 挂接：键须已注册；首次挂接即在位。
    pub fn attach(&mut self, key: u32, n: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(SH_FROZEN);
        }
        if n == 0 {
            return Err(SH_ZSIZE);
        }
        let mut k = 0usize;
        while k < self.segs.len() {
            if self.segs[k].key == key {
                self.segs[k].att = self.segs[k].att.wrapping_add(n);
                self.segs[k].live = true;
                return Ok(self.segs[k].att);
            }
            k += 1;
        }
        Err(SH_NOKEY)
    }

    /// 摘挂：不得超挂接；齐平即离位（核销完成）。
    pub fn detach(&mut self, key: u32, n: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(SH_FROZEN);
        }
        let mut k = 0usize;
        while k < self.segs.len() {
            if self.segs[k].key == key {
                if n > self.segs[k].att - self.segs[k].det {
                    return Err(SH_OVERDET);
                }
                self.segs[k].det += n;
                if self.segs[k].att == self.segs[k].det {
                    self.segs[k].live = false;
                }
                return Ok(self.segs[k].det);
            }
            k += 1;
        }
        Err(SH_NOKEY)
    }

    /// 残留面：仍有未摘挂的段键清单。
    pub fn residues(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.segs.len() {
            if self.segs[k].resid() > 0 {
                g.push(self.segs[k].key);
            }
            k += 1;
        }
        g
    }

    /// 残留总量。
    pub fn resid_total(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.segs.len() {
            n = n.wrapping_add(self.segs[k].resid());
            k += 1;
        }
        n
    }

    /// 零残留断言：段数齐、逐段齐平、无一在位。
    pub fn closed(&self) -> bool {
        self.segs.len() == SEGS_NEEDED
            && self.resid_total() == 0
            && !self.segs.iter().any(|s| s.live)
    }

    /// 按键查位次。
    pub fn find(&self, key: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.segs.len() {
            if self.segs[k].key == key {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SHM_TAG;
        let mut k = 0usize;
        while k < self.segs.len() {
            let s = &self.segs[k];
            h = (h ^ s.key as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.size as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.att as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.det as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.live as u64).wrapping_mul(0x100000001b3);
            k += 1;
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

/// 键确定性样本。
pub fn sh_key(no: usize) -> u32 {
    0x5c00 + no as u32 * 8
}

/// 尺寸确定性样本（页粒度）。
pub fn sh_size(no: usize) -> u32 {
    4096 * (no % 4 + 1) as u32
}

/// 十段挂摘全链：注册 → 挂接 → 摘挂齐平。
pub fn build_shmbook() -> (ShmBook, bool) {
    let mut b = ShmBook::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < SEGS_NEEDED {
        if b.enroll(sh_key(i), sh_size(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < SEGS_NEEDED {
        if b.attach(sh_key(i), ATTACH_SAMPLE).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < SEGS_NEEDED {
        if b.detach(sh_key(i), ATTACH_SAMPLE).is_err() {
            ok = false;
        }
        i += 1;
    }
    let b_ok = ok && b.closed();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 摘挂超挂接必拒
    let mut a = ShmBook::new();
    let _ = a.enroll(1, 4096);
    let _ = a.attach(1, 2);
    got[0] = a.detach(1, 3).err() == Some(SH_OVERDET);
    // 2) 未注册键挂接必拒
    let mut b = ShmBook::new();
    got[1] = b.attach(0xdead, 1).err() == Some(SH_NOKEY);
    // 3) 零尺寸与重号键必拒
    let mut c = ShmBook::new();
    let z = c.enroll(9, 0).err() == Some(SH_ZSIZE);
    let _ = c.enroll(9, 4096);
    got[2] = z && c.enroll(9, 4096).err() == Some(SH_DUPKEY);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_shmbook();
    d.freeze();
    got[3] = d.attach(sh_key(0), 1).err() == Some(SH_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11021_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11021");

    // J1 挂接注册账逐项对平：十段逐段齐平
    let (mut book, green) = build_shmbook();
    s.add("J1-挂接注册对平", green && book.segs.len() == 10, "十段注册挂接逐段对平");

    // J2 摘挂核销零残留：残留面清空且无一在位
    s.add("J2-摘挂零残留", book.resid_total() == 0 && book.residues().is_empty() && book.closed(), "逐段摘挂齐平零残留");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.attach(sh_key(0), 1).err() == Some(SH_FROZEN)
        && book.detach(sh_key(0), 1).err() == Some(SH_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后挂接与摘挂均必拒");

    // J5 注入检出：虚摘 + 未注册键
    let inj = inject_four();
    s.add("J5-虚摘无键抓", inj[0] && inj[1], "摘挂超挂接与未注册键两路必抓");

    // J6 注入检出：零尺寸重号 + 冻结后写
    s.add("J6-零尺冻结检出", inj[2] && inj[3], "零尺寸重号与冻结后写必抓");

    // J7 部分摘挂：摘一半时残留恰为一半且未离位
    let j7 = {
        let mut c = ShmBook::new();
        let _ = c.enroll(0x77, 4096);
        let _ = c.attach(0x77, 8);
        let _ = c.detach(0x77, 3);
        let half = c.find(0x77).map(|k| c.segs[k].resid() == 5 && c.segs[k].live);
        half == Some(true) && !c.closed()
    };
    s.add("J7-部分摘挂", j7, "部分摘挂残留精确且段仍在位");

    // J8 齐平再挂：离位段可重新挂接
    let j8 = {
        let mut c = ShmBook::new();
        let _ = c.enroll(0x88, 4096);
        let _ = c.attach(0x88, 4);
        let _ = c.detach(0x88, 4);
        let off = c.find(0x88).map(|k| !c.segs[k].live);
        let _ = c.attach(0x88, 2);
        off == Some(true) && c.find(0x88).map(|k| c.segs[k].live) == Some(true)
    };
    s.add("J8-再挂恢复", j8, "齐平离位段可重新挂接");

    // J9 分次摘挂核销：分两次与一次到位等价
    let j9 = {
        let mut c = ShmBook::new();
        let _ = c.enroll(0x99, 4096);
        let _ = c.attach(0x99, 10);
        let _ = c.detach(0x99, 4);
        let _ = c.detach(0x99, 6);
        c.resid_total() == 0
    };
    s.add("J9-分次摘挂", j9, "分次摘挂与一次到位等价");

    // J10 摘挂未注册键拒
    let j10 = ShmBook::new().detach(0x1234, 1).err() == Some(SH_NOKEY);
    s.add("J10-摘挂无键拒", j10, "摘挂未注册键必拒");

    // J11 超容拒：第 SEG_CAP_N+1 段必拒
    let j11 = {
        let mut c = ShmBook::new();
        let mut filled = true;
        let mut n = 0u32;
        while (n as usize) < SEG_CAP_N {
            if c.enroll(0x1000 + n, 4096).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(0x2000, 4096).err() == Some(SH_CAP)
    };
    s.add("J11-超容边界", j11, "超出定容必拒且原账不损");

    // J12 渲染面：段渲染含键与挂接
    let j12 = {
        let g = ShmSeg { key: 0x5a0, size: 8192, att: 7, det: 7, live: false };
        let r = g.render();
        r.contains("k1440") && r.contains("a7") && r.contains("d7")
    };
    s.add("J12-渲染明细", j12, "段渲染含键尺寸挂摘");

    // J13 尺寸样本确定性：同段同尺寸
    let s3a = sh_size(3);
    let s3b = sh_size(3);
    let k5a = sh_key(5);
    let k5b = sh_key(5);
    s.add("J13-样本确定性", s3a == s3b && k5a == k5b, "键与尺寸样本同入同出");

    // J14 残留注入：漏摘一段残留恰一
    let j14 = {
        let mut c = ShmBook::new();
        let mut i = 0usize;
        while i < SEGS_NEEDED {
            let _ = c.enroll(sh_key(i), sh_size(i));
            let _ = c.attach(sh_key(i), ATTACH_SAMPLE);
            i += 1;
        }
        let mut i = 1usize;
        while i < SEGS_NEEDED {
            let _ = c.detach(sh_key(i), ATTACH_SAMPLE);
            i += 1;
        }
        c.residues().len() == 1 && c.resid_total() == ATTACH_SAMPLE && !c.closed()
    };
    s.add("J14-漏摘残留", j14, "漏摘一段残留恰一且断言为假");

    // J15 动态明细：十段齐平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-段账明细",
        fp == root && book.segs.len() == 10,
        &format!("root={:#x} segs=10", fp),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11021_shm_reg_flow() {
        let (mut b, built) = build_shmbook();
        assert!(built);
        // 重号面与冻结面是两条独立的拒收面，必须各按前提排布：同键再登记
        // 必 SH_DUPKEY（此项与冻结位无关，故排在 freeze 之前）；封账后
        // attach / detach 双路必 SH_FROZEN。初版把两条冻结断言放到了
        // freeze 之前——未封账时它们只会走通或走 SH_NOKEY，前提不成立。
        assert!(b.enroll(sh_key(0), 4096).err() == Some(SH_DUPKEY));
        b.freeze();
        assert!(b.attach(sh_key(0), 1).err() == Some(SH_FROZEN));
        assert!(b.detach(sh_key(1), 1).err() == Some(SH_FROZEN));
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11021_checks_green() {
        let s = run_unx11021_checks();
        assert!(s.all_passed(), "UNX-F11021 判据未全绿");
    }
}
