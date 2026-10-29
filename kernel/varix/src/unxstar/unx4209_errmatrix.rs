//! unx4209_errmatrix — UNX-F4209 ENOENT/ENOTDIR 错误矩阵细化（诊断段 0x9B60）。
//!
//! 判据原文：J1 矩阵四维细化：缺失层级（末缺/中缺）× 父类型
//!           （DIR/LNK/REG）× 尾斜杠（有/无）× 大小写错配（是/否）
//!           共 24 格全建账，ENOENT/ENOTDIR 分账无一混淆；
//!           J2 语义核：中间分量缺 → ENOENT（无论尾斜杠）；中间分
//!           量是普通文件再往下走 → ENOTDIR；末分量缺 → ENOENT，
//!           但带尾斜杠且父在 → ENOENT（POSIX：目录名缺仍 ENOENT）；
//!           大小写错配 → ENOENT（大小写敏感全局声明）；
//!           J3 错误账守恒：probed = enoent + enotdir + passed，且
//!           与矩阵格账一致；
//!           J4 注入全拒：越矩阵四维下标、未建格查询、重复建格。
//! 设计纪律：
//!   1. POSIX namei 语义：ENOENT = 分量不存在；ENOTDIR = 路径前缀
//!      中非目录分量被当目录用。两错误在 walk 层产生，本矩阵将其
//!      细化到四维账格。
//!   2. 大小写敏感全局声明（B09 联签）：错配即不存在。
//!   3. 账格值域：0=passed（应答成功）/1=ENOENT/2=ENOTDIR，越值拒。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9B60 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9B60;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_CELL_DUP: u16 = SEG_BASE + 1;  // 格重复建账
const E_VAL_OUT: u16 = SEG_BASE + 2;   // 格值越 {0,1,2}
const E_IDX_OUT: u16 = SEG_BASE + 3;   // 四维下标越界
const E_BAL: u16 = SEG_BASE + 4;       // 错误账不守恒
const E_SEMANTIC: u16 = SEG_BASE + 5;  // 语义核抽验失守
const E_UNSET: u16 = SEG_BASE + 6;     // 未建格查询

/// 缺失层级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MissLevel {
    Last = 0, // 末分量缺
    Mid = 1,  // 中间分量缺
}

/// 父（缺失点前驱）类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParentTy {
    Dir = 0,
    Lnk,
    Reg,
}

/// 尾斜杠。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tail {
    No = 0,
    Yes,
}

/// 大小写错配。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseMis {
    No = 0,
    Yes,
}

pub const CELLS: usize = 2 * 3 * 2 * 2; // 24

/// 格值：0 passed / 1 ENOENT / 2 ENOTDIR。
pub const V_PASS: u8 = 0;
pub const V_ENOENT: u8 = 1;
pub const V_ENOTDIR: u8 = 2;

/// 错误矩阵枢纽。
pub struct ErrMatrix {
    cells: [u8; CELLS],
    filled: [bool; CELLS],
    pub probed: u32,
    pub enoent: u32,
    pub enotdir: u32,
    pub passed: u32,
    pub set_ops: u32,
    pub rejected: u32,
}

impl ErrMatrix {
    pub fn new() -> ErrMatrix {
        ErrMatrix {
            cells: [0; CELLS],
            filled: [false; CELLS],
            probed: 0,
            enoent: 0,
            enotdir: 0,
            passed: 0,
            set_ops: 0,
            rejected: 0,
        }
    }

    fn idx(m: MissLevel, p: ParentTy, t: Tail, c: CaseMis) -> Result<usize, u16> {
        let m = m as usize;
        let p = p as usize;
        let t = t as usize;
        let c = c as usize;
        if m >= 2 || p >= 3 || t >= 2 || c >= 2 {
            return Err(E_IDX_OUT);
        }
        Ok(m * 12 + p * 4 + t * 2 + c)
    }

    /// 建格。
    pub fn set(&mut self, m: MissLevel, p: ParentTy, t: Tail, c: CaseMis, v: u8) -> Result<(), u16> {
        self.set_ops += 1;
        if v > V_ENOTDIR {
            self.rejected += 1;
            return Err(E_VAL_OUT);
        }
        let i = Self::idx(m, p, t, c)?;
        if self.filled[i] {
            self.rejected += 1;
            return Err(E_CELL_DUP);
        }
        self.cells[i] = v;
        self.filled[i] = true;
        Ok(())
    }

    /// 查格并入探账（判据 J3 分账）。
    pub fn probe(&mut self, m: MissLevel, p: ParentTy, t: Tail, c: CaseMis) -> Result<u8, u16> {
        let i = Self::idx(m, p, t, c)?;
        if !self.filled[i] {
            return Err(E_UNSET);
        }
        self.probed += 1;
        let v = self.cells[i];
        match v {
            V_ENOENT => self.enoent += 1,
            V_ENOTDIR => self.enotdir += 1,
            _ => self.passed += 1,
        }
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

    /// 错误账守恒（判据 J3）。
    pub fn probe_balanced(&self) -> bool {
        self.probed == self.enoent + self.enotdir + self.passed
    }

    /// 矩阵账守恒（判据 J4）。
    pub fn set_balanced(&self) -> bool {
        self.set_ops == self.filled_count() as u32 + self.rejected
    }
}

/// 语义建账（判据 J1/J2 的承载主体）：24 格按 POSIX 语义填。
pub fn build_semantic(m: &mut ErrMatrix) -> Result<(), u16> {
    for m_i in 0..2 {
        let ml = if m_i == 0 { MissLevel::Last } else { MissLevel::Mid };
        for p_i in 0..3 {
            let pt = match p_i {
                0 => ParentTy::Dir,
                1 => ParentTy::Lnk,
                _ => ParentTy::Reg,
            };
            for t_i in 0..2 {
                let tl = if t_i == 0 { Tail::No } else { Tail::Yes };
                for c_i in 0..2 {
                    let cm = if c_i == 0 { CaseMis::No } else { CaseMis::Yes };
                    // 大小写错配 → 恒 ENOENT（判据 J2：敏感声明）
                    if cm == CaseMis::Yes {
                        m.set(ml, pt, tl, cm, V_ENOENT)?;
                        continue;
                    }
                    let v = match (ml, pt, tl) {
                        // 中间缺：父 DIR/LNK 可展开继续查 → ENOENT；
                        // 父 REG 无法当目录 → ENOTDIR（无论尾斜杠）
                        (MissLevel::Mid, ParentTy::Dir, _) => V_ENOENT,
                        (MissLevel::Mid, ParentTy::Lnk, _) => V_ENOENT,
                        (MissLevel::Mid, ParentTy::Reg, _) => V_ENOTDIR,
                        // 末缺：父在 → ENOENT（尾斜杠不改变：目录名缺仍 ENOENT）
                        (MissLevel::Last, ParentTy::Dir, _) => V_ENOENT,
                        (MissLevel::Last, ParentTy::Lnk, _) => V_ENOENT,
                        // 末缺且父是 REG：前缀把 REG 当目录用 → ENOTDIR
                        (MissLevel::Last, ParentTy::Reg, _) => V_ENOTDIR,
                    };
                    m.set(ml, pt, tl, cm, v)?;
                }
            }
        }
    }
    Ok(())
}

/// 语义核抽验（判据 J2 的四条关键格）。
pub fn semantic_probe(m: &mut ErrMatrix) -> Result<(), u16> {
    // 中缺 + 父 REG + 无尾斜杠 → ENOTDIR
    if m.probe(MissLevel::Mid, ParentTy::Reg, Tail::No, CaseMis::No)? != V_ENOTDIR {
        return Err(E_SEMANTIC);
    }
    // 中缺 + 父 DIR + 带尾斜杠 → ENOENT
    if m.probe(MissLevel::Mid, ParentTy::Dir, Tail::Yes, CaseMis::No)? != V_ENOENT {
        return Err(E_SEMANTIC);
    }
    // 末缺 + 父 DIR → ENOENT
    if m.probe(MissLevel::Last, ParentTy::Dir, Tail::No, CaseMis::No)? != V_ENOENT {
        return Err(E_SEMANTIC);
    }
    // 大小写错配恒 ENOENT（REG 父也不升 ENOTDIR——错配先于类型判）
    if m.probe(MissLevel::Last, ParentTy::Reg, Tail::Yes, CaseMis::Yes)? != V_ENOENT {
        return Err(E_SEMANTIC);
    }
    Ok(())
}

/// 自例程：R1 语义建账 24 格全满；R2 语义核四格；R3 分账比例核对；
/// R4 注入全拒（越值/重复/越下标/未建格）；R5 双账守恒终验。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建账全满。
    let mut m = ErrMatrix::new();
    build_semantic(&mut m)?;
    if m.filled_count() != CELLS || CELLS != 24 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：语义核四格。
    semantic_probe(&mut m)?;
    hits += 1;

    // R3：分账比例核对——中缺 REG 两格 ENOTDIR + 末缺 REG 两格 ENOTDIR
    //     = 4 格 ENOTDIR；大小写错配 12 格 ENOENT；其余 ENOENT。
    //     探全表 24 格后：enoent=20, enotdir=4, passed=0。
    let mut m2 = ErrMatrix::new();
    build_semantic(&mut m2)?;
    for m_i in 0..2 {
        for p_i in 0..3 {
            for t_i in 0..2 {
                for c_i in 0..2 {
                    let ml = if m_i == 0 { MissLevel::Last } else { MissLevel::Mid };
                    let pt = match p_i {
                        0 => ParentTy::Dir,
                        1 => ParentTy::Lnk,
                        _ => ParentTy::Reg,
                    };
                    let tl = if t_i == 0 { Tail::No } else { Tail::Yes };
                    let cm = if c_i == 0 { CaseMis::No } else { CaseMis::Yes };
                    m2.probe(ml, pt, tl, cm)?;
                }
            }
        }
    }
    if m2.probed != 24 || m2.enoent != 20 || m2.enotdir != 4 || m2.passed != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：注入全拒。
    let mut m3 = ErrMatrix::new();
    if m3.set(MissLevel::Last, ParentTy::Dir, Tail::No, CaseMis::No, 3) != Err(E_VAL_OUT) {
        return Err(E_VAL_OUT);
    }
    m3.set(MissLevel::Last, ParentTy::Dir, Tail::No, CaseMis::No, V_ENOENT)?;
    if m3.set(MissLevel::Last, ParentTy::Dir, Tail::No, CaseMis::No, V_PASS) != Err(E_CELL_DUP) {
        return Err(E_CELL_DUP);
    }
    if m3.probe(MissLevel::Mid, ParentTy::Lnk, Tail::Yes, CaseMis::Yes) != Err(E_UNSET) {
        return Err(E_UNSET);
    }
    hits += 1;

    // R5：双账守恒终验。
    if !m2.probe_balanced() || !m2.set_balanced() || !m3.set_balanced() {
        return Err(E_BAL);
    }
    if m3.set_ops != m3.filled_count() as u32 + m3.rejected {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
