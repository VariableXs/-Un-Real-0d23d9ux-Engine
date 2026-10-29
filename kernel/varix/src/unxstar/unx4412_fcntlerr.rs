//! unx4412_fcntlerr — UNX-F4412 fcntl 错误码矩阵（诊断段 0xB740）。
//!
//! 判据原文：J1 矩阵四维：命令族（dupfd/getfd/setfd/getfl/setfl/
//!           own/lock 七族）× 错误源（EBADF/EINVAL/EMFILE/ENOLCK/
//!           EINTR 五源）× 持有态（有 fd/无 fd）× 可重试（是/否）
//!           = 140 格建账，期望值全部落 errno 闭集；
//!           J2 语义核抽验：EBADF 只在无 fd 行；EMFILE 只在 dupfd
//!           族；ENOLCK 只在 lock 族；EINTR 可重试行只有 lock 族
//!           （F_SETLKW 可重试，其余不可）；
//!           J3 错误账守恒：probed = ebadf + einval + emfile +
//!           enolck + eintr + passed；
//!           J4 注入全拒：越族下标、未建格查询、重复建格。
//! 设计纪律：
//!   1. POSIX fcntl 错误语义：EBADF（fd 无效）/EINVAL（命令或参数
//!      非法）/EMFILE（dupfd 超上限）/ENOLCK（锁资源尽）/EINTR
//!      （阻塞锁被信号打断，仅 F_SETLKW）。
//!   2. 格值闭集：0=passed / 2=ENOENT 类不出现 / 9=EBADF / 4=EINTR /
//!      12=ENOMEM 不用 / 22=EINVAL / 24=EMFILE / 37=ENOLCK。
//!   3. 四维下标：族×源×持有×重试，线性化入 140 格账。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0xB740 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB740;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_CELL_DUP: u16 = SEG_BASE + 1;  // 格重复建账
const E_VAL_OUT: u16 = SEG_BASE + 2;   // 格值越闭集
const E_IDX_OUT: u16 = SEG_BASE + 3;   // 四维下标越界
const E_BAL: u16 = SEG_BASE + 4;       // 错误账不守恒
const E_SEMANTIC: u16 = SEG_BASE + 5;  // 语义核失守
const E_UNSET: u16 = SEG_BASE + 6;     // 未建格查询

/// 命令族七类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fam {
    DupFd = 0,
    GetFd,
    SetFd,
    GetFl,
    SetFl,
    Own,
    Lock,
}

pub const NFAM: usize = 7;

/// 错误源五类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ESrc {
    BadF = 0,
    Inval,
    Emfile,
    Enolck,
    Intr,
}

pub const NSRC: usize = 5;

/// 持有态 / 重试态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bit2 {
    No = 0,
    Yes,
}

pub const CELLS: usize = NFAM * NSRC * 2 * 2; // 140

// errno 闭集（本矩阵可能出现的值）
pub const V_PASS: u16 = 0;
pub const V_EBADF: u16 = 9;
pub const V_EINTR: u16 = 4;
pub const V_EINVAL: u16 = 22;
pub const V_EMFILE: u16 = 24;
pub const V_ENOLCK: u16 = 37;

fn val_closed(v: u16) -> bool {
    matches!(v, V_PASS | V_EBADF | V_EINTR | V_EINVAL | V_EMFILE | V_ENOLCK)
}

/// fcntl 错误矩阵枢纽。
pub struct FcntlErrMatrix {
    cells: Vec<u16>,
    filled: Vec<bool>,
    pub probed: u32,
    per_err: [u32; 6], // pass/badf/inval/emfile/enolck/eintr
    pub set_ops: u32,
    pub rejected: u32,
}

impl FcntlErrMatrix {
    pub fn new() -> FcntlErrMatrix {
        FcntlErrMatrix {
            cells: vec![0u16; CELLS],
            filled: vec![false; CELLS],
            probed: 0,
            per_err: [0; 6],
            set_ops: 0,
            rejected: 0,
        }
    }

    fn idx(f: Fam, s: ESrc, hold: Bit2, retry: Bit2) -> Result<usize, u16> {
        let f = f as usize;
        let s = s as usize;
        if f >= NFAM || s >= NSRC {
            return Err(E_IDX_OUT);
        }
        Ok(f * (NSRC * 4) + s * 4 + (hold as usize) * 2 + (retry as usize))
    }

    /// 建格。
    pub fn set(&mut self, f: Fam, s: ESrc, hold: Bit2, retry: Bit2, v: u16) -> Result<(), u16> {
        self.set_ops += 1;
        if !val_closed(v) {
            self.rejected += 1;
            return Err(E_VAL_OUT);
        }
        let i = Self::idx(f, s, hold, retry)?;
        if self.filled[i] {
            self.rejected += 1;
            return Err(E_CELL_DUP);
        }
        self.cells[i] = v;
        self.filled[i] = true;
        Ok(())
    }

    /// 探格并入分账（判据 J3）。
    pub fn probe(&mut self, f: Fam, s: ESrc, hold: Bit2, retry: Bit2) -> Result<u16, u16> {
        let i = Self::idx(f, s, hold, retry)?;
        if !self.filled[i] {
            return Err(E_UNSET);
        }
        self.probed += 1;
        let v = self.cells[i];
        let slot = match v {
            V_PASS => 0,
            V_EBADF => 1,
            V_EINVAL => 2,
            V_EMFILE => 3,
            V_ENOLCK => 4,
            _ => 5, // EINTR
        };
        self.per_err[slot] += 1;
        Ok(v)
    }

    pub fn filled_count(&self) -> usize {
        let mut n = 0;
        for f in self.filled.iter() {
            if *f {
                n += 1;
            }
        }
        n
    }

    /// 语义建账（判据 J1/J2 的承载主体）。
    pub fn build_semantic(&mut self) -> Result<(), u16> {
        for f_i in 0..NFAM {
            let f = match f_i {
                0 => Fam::DupFd,
                1 => Fam::GetFd,
                2 => Fam::SetFd,
                3 => Fam::GetFl,
                4 => Fam::SetFl,
                5 => Fam::Own,
                _ => Fam::Lock,
            };
            for s_i in 0..NSRC {
                let s = match s_i {
                    0 => ESrc::BadF,
                    1 => ESrc::Inval,
                    2 => ESrc::Emfile,
                    3 => ESrc::Enolck,
                    _ => ESrc::Intr,
                };
                for h_i in 0..2 {
                    let hold = if h_i == 0 { Bit2::No } else { Bit2::Yes };
                    for r_i in 0..2 {
                        let retry = if r_i == 0 { Bit2::No } else { Bit2::Yes };
                        let v = self.semantic_cell(f, s, hold, retry);
                        self.set(f, s, hold, retry, v)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// 单格语义（判据 J2 的规则表）。
    fn semantic_cell(&self, f: Fam, s: ESrc, hold: Bit2, retry: Bit2) -> u16 {
        // EBADF：只在无 fd 行（hold=No），任何族、任何源标注为 BadF 时
        if s == ESrc::BadF && hold == Bit2::No {
            return V_EBADF;
        }
        // 源与族错配规则：
        match (f, s) {
            // EMFILE 只在 dupfd 族
            (_, ESrc::Emfile) => {
                if f == Fam::DupFd && hold == Bit2::Yes {
                    V_EMFILE
                } else {
                    V_PASS
                }
            }
            // ENOLCK 只在 lock 族
            (_, ESrc::Enolck) => {
                if f == Fam::Lock && hold == Bit2::Yes {
                    V_ENOLCK
                } else {
                    V_PASS
                }
            }
            // EINTR：可重试行只有 lock 族（F_SETLKW）
            (_, ESrc::Intr) => {
                if f == Fam::Lock && retry == Bit2::Yes && hold == Bit2::Yes {
                    V_EINTR
                } else {
                    V_PASS
                }
            }
            // EINVAL：参数非法（族无关，有 fd 才走到参数检查）
            (_, ESrc::Inval) => {
                if hold == Bit2::Yes {
                    V_EINVAL
                } else {
                    V_PASS
                }
            }
            // EBADF 源 + 有 fd：不产生 EBADF（passed）
            (_, ESrc::BadF) => V_PASS,
        }
    }

    /// 错误账守恒（判据 J3）。
    pub fn probe_balanced(&self) -> bool {
        let sum: u32 = self.per_err.iter().sum();
        sum == self.probed
    }

    pub fn set_balanced(&self) -> bool {
        self.set_ops == self.filled_count() as u32 + self.rejected
    }

    /// 语义核抽验（判据 J2 四条）。
    pub fn semantic_probe(&mut self) -> Result<(), u16> {
        // EBADF 只在无 fd 行：Lock×BadF×No×Yes = EBADF
        if self.probe(Fam::Lock, ESrc::BadF, Bit2::No, Bit2::Yes)? != V_EBADF {
            return Err(E_SEMANTIC);
        }
        // EMFILE 只在 dupfd：DupFd×Emfile×Yes×No = EMFILE；GetFl 同源 = PASS
        if self.probe(Fam::DupFd, ESrc::Emfile, Bit2::Yes, Bit2::No)? != V_EMFILE {
            return Err(E_SEMANTIC);
        }
        if self.probe(Fam::GetFl, ESrc::Emfile, Bit2::Yes, Bit2::No)? != V_PASS {
            return Err(E_SEMANTIC);
        }
        // EINTR 可重试仅 lock：Lock×Intr×Yes×Yes = EINTR；SetFl 同源 = PASS
        if self.probe(Fam::Lock, ESrc::Intr, Bit2::Yes, Bit2::Yes)? != V_EINTR {
            return Err(E_SEMANTIC);
        }
        if self.probe(Fam::SetFl, ESrc::Intr, Bit2::Yes, Bit2::Yes)? != V_PASS {
            return Err(E_SEMANTIC);
        }
        Ok(())
    }
}

/// 自例程：R1 语义建账 140 格全满；R2 语义核五格；R3 分账比例；
/// R4 注入全拒；R5 双账守恒终验。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建账全满。
    let mut m = FcntlErrMatrix::new();
    m.build_semantic()?;
    if m.filled_count() != CELLS || CELLS != 140 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：语义核五格。
    m.semantic_probe()?;
    hits += 1;

    // R3：分账比例——全表探一遍：
    //     EBADF 行：7 族 × (No,×) = 14 格；EINVAL 有 fd：7×2=14；
    //     EMFILE：dupfd 有 fd 2 格；ENOLCK：lock 有 fd 2 格；
    //     EINTR：lock YesYes 1 格；其余 PASS。
    let mut m2 = FcntlErrMatrix::new();
    m2.build_semantic()?;
    for f_i in 0..NFAM {
        let f = match f_i {
            0 => Fam::DupFd,
            1 => Fam::GetFd,
            2 => Fam::SetFd,
            3 => Fam::GetFl,
            4 => Fam::SetFl,
            5 => Fam::Own,
            _ => Fam::Lock,
        };
        for s_i in 0..NSRC {
            let s = match s_i {
                0 => ESrc::BadF,
                1 => ESrc::Inval,
                2 => ESrc::Emfile,
                3 => ESrc::Enolck,
                _ => ESrc::Intr,
            };
            for h_i in 0..2 {
                let hold = if h_i == 0 { Bit2::No } else { Bit2::Yes };
                for r_i in 0..2 {
                    let retry = if r_i == 0 { Bit2::No } else { Bit2::Yes };
                    m2.probe(f, s, hold, retry)?;
                }
            }
        }
    }
    if m2.probed != 140 {
        return Err(E_BAL);
    }
    if m2.per_err[1] != 14 {
        return Err(E_BAL); // EBADF 14 格
    }
    if m2.per_err[2] != 14 {
        return Err(E_BAL); // EINVAL 14 格
    }
    if m2.per_err[3] != 2 {
        return Err(E_BAL); // EMFILE 2 格
    }
    if m2.per_err[4] != 2 {
        return Err(E_BAL); // ENOLCK 2 格
    }
    if m2.per_err[5] != 1 {
        return Err(E_BAL); // EINTR 1 格
    }
    hits += 1;

    // R4：注入全拒——越值、重复建格、越下标、未建格。
    let mut m3 = FcntlErrMatrix::new();
    if m3.set(Fam::DupFd, ESrc::Inval, Bit2::Yes, Bit2::No, 99) != Err(E_VAL_OUT) {
        return Err(E_VAL_OUT);
    }
    m3.set(Fam::DupFd, ESrc::Inval, Bit2::Yes, Bit2::No, V_EINVAL)?;
    if m3.set(Fam::DupFd, ESrc::Inval, Bit2::Yes, Bit2::No, V_PASS) != Err(E_CELL_DUP) {
        return Err(E_CELL_DUP);
    }
    if m3.probe(Fam::Lock, ESrc::Inval, Bit2::Yes, Bit2::No) != Err(E_UNSET) {
        return Err(E_UNSET);
    }
    hits += 1;

    // R5：双账守恒终验（判据 J4）。
    if !m2.probe_balanced() || !m2.set_balanced() || !m3.set_balanced() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
