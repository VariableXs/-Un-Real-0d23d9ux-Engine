//! cga159_crosstarget.rs —— CGPU-F0159 · A 域跨平台终验
//!
//! F0059 交叉编译的域级版：x86（AVX2/AVX512）+ ARM（NEON）+
//! RISC-V（RVV/QEMU）的全平台测试矩阵终跑，平台间一致性断言
//! （同输入同输出——跨架构确定性 F0114 的平台维度）。
//! 判据：全平台矩阵、一致性断言、差异入册、CI 固化、归档。
//! 诊断码段 0x03E0（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x03E0;

/// 平台（判据「全平台矩阵」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Platform {
    /// x86 + AVX2。
    X86Avx2,
    /// x86 + AVX512。
    X86Avx512,
    /// ARM + NEON。
    ArmNeon,
    /// RISC-V + RVV（QEMU）。
    RiscvRvv,
}

impl Platform {
    /// 全平台序。
    pub const ALL: [Platform; 4] = [
        Platform::X86Avx2,
        Platform::X86Avx512,
        Platform::ArmNeon,
        Platform::RiscvRvv,
    ];

    /// 序号。
    pub fn rank(self) -> u8 {
        match self {
            Platform::X86Avx2 => 0,
            Platform::X86Avx512 => 1,
            Platform::ArmNeon => 2,
            Platform::RiscvRvv => 3,
        }
    }
}

/// 单平台单测试执行行。
#[derive(Clone, Copy, Debug)]
pub struct MatrixCell {
    /// 平台。
    pub plat: Platform,
    /// 测试用例号。
    pub case: u32,
    /// 输出指纹（同输入同输出的「输出」端）。
    pub out_fp: u64,
    /// 执行绿。
    pub ok: bool,
}

impl MatrixCell {
    /// 自检：绿行必须有输出指纹（绿而无输出 = 假绿）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.ok && self.out_fp == 0 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// 平台差异行（判据「差异入册」）。
#[derive(Clone, Debug)]
pub struct DiffRow {
    /// 测试用例号。
    pub case: u32,
    /// 平台 A。
    pub plat_a: Platform,
    /// 平台 B。
    pub plat_b: Platform,
    /// 差异定性：0=可接受（如定点舍入容差内）1=已修复 2=豁免(ADR)。
    pub disposition: u8,
    /// 差异记录指纹。
    pub note_fp: u64,
}

/// 跨平台终验台账。
pub struct CrossTargetAudit {
    /// 矩阵行。
    cells: Vec<MatrixCell>,
    /// 差异册。
    diffs: Vec<DiffRow>,
    /// CI 固化签章（判据「CI 固化」）。
    ci_seal: Option<u64>,
    /// 定点容差：一致性断言允许的指纹容差位数（0 = 逐位严格）。
    fp_tolerance_bits: u8,
    /// 归档。
    archived: Option<u64>,
}

impl CrossTargetAudit {
    /// 建账：容差界 0~8 位（>8 位的一致性没有意义）。
    pub fn new(fp_tolerance_bits: u8) -> Result<Self, u16> {
        if fp_tolerance_bits > 8 {
            return Err(SEG_BASE + 2);
        }
        Ok(CrossTargetAudit {
            cells: Vec::new(),
            diffs: Vec::new(),
            ci_seal: None,
            fp_tolerance_bits,
            archived: None,
        })
    }

    /// 矩阵行数。
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// 差异行数。
    pub fn diff_count(&self) -> usize {
        self.diffs.len()
    }

    /// 落矩阵行（判据「全平台矩阵」）。
    pub fn record(&mut self, c: MatrixCell) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 3);
        }
        c.validate()?;
        self.cells.push(c);
        Ok(())
    }

    /// 全平台矩阵完整（判据「全平台矩阵」）：每平台至少一行。
    pub fn matrix_full(&self) -> Result<(), u16> {
        if self.cells.is_empty() {
            return Err(SEG_BASE + 4);
        }
        for p in Platform::ALL {
            if !self.cells.iter().any(|c| c.plat == p) {
                return Err(SEG_BASE + 5);
            }
        }
        Ok(())
    }

    /// 执行全绿（判据「终跑」）。
    pub fn exec_green(&self) -> Result<(), u16> {
        if self.cells.is_empty() {
            return Err(SEG_BASE + 4);
        }
        if self.cells.iter().all(|c| c.ok) {
            Ok(())
        } else {
            Err(SEG_BASE + 6)
        }
    }

    /// 一致性断言（判据「一致性断言」）：同用例跨平台输出指纹
    /// 一致（容差内视为一致——低位 fp_tolerance_bits 可异）。
    /// 不一致者必须已在差异册处置，否则断言失败。
    pub fn consistency_green(&self) -> Result<(), u16> {
        if self.cells.is_empty() {
            return Err(SEG_BASE + 4);
        }
        let mask: u64 = if self.fp_tolerance_bits == 0 {
            u64::MAX
        } else {
            !((1u64 << self.fp_tolerance_bits) - 1)
        };
        let mut cases: Vec<u32> = Vec::new();
        for c in &self.cells {
            if !cases.contains(&c.case) {
                cases.push(c.case);
            }
        }
        for case in cases {
            let rows: Vec<&MatrixCell> =
                self.cells.iter().filter(|c| c.case == case).collect();
            if rows.len() < 2 {
                // 单平台用例不参与跨平台断言。
                continue;
            }
            let base = rows[0].out_fp & mask;
            let mut agree = true;
            for r in &rows[1..] {
                if r.out_fp & mask != base {
                    agree = false;
                    break;
                }
            }
            if !agree && !self.diffs.iter().any(|d| d.case == case) {
                return Err(SEG_BASE + 7);
            }
        }
        Ok(())
    }

    /// 入差异册（判据「差异入册」）：处置闭集 + 记录指纹必填。
    pub fn log_diff(
        &mut self,
        case: u32,
        plat_a: Platform,
        plat_b: Platform,
        disposition: u8,
        note: &[u8],
    ) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 3);
        }
        if disposition > 2 {
            return Err(SEG_BASE + 8);
        }
        if note.is_empty() {
            return Err(SEG_BASE + 9);
        }
        if plat_a == plat_b {
            return Err(SEG_BASE + 10);
        }
        let mut fp: u64 = 0xcbf29ce484222325;
        for &b in note {
            fp = fp.wrapping_mul(0x100000001b3) ^ (b as u64);
        }
        self.diffs.push(DiffRow {
            case,
            plat_a,
            plat_b,
            disposition,
            note_fp: fp,
        });
        Ok(())
    }

    /// 差异处置闭环（判据支撑）：全部差异必须已修复或豁免，
    /// 「可接受」态不允许存量（存量可接受差异 = 债务静默）。
    pub fn diffs_closed(&self) -> Result<(), u16> {
        if self.diffs.iter().any(|d| d.disposition == 0) {
            return Err(SEG_BASE + 11);
        }
        Ok(())
    }

    /// CI 固化（判据「CI 固化」）：矩阵 + 断言配置指纹签章。
    pub fn seal_ci(&mut self, config: &[u8]) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 3);
        }
        if self.ci_seal.is_some() {
            return Err(SEG_BASE + 12);
        }
        if config.is_empty() {
            return Err(SEG_BASE + 13);
        }
        self.matrix_full()?;
        let mut fp: u64 = 0xcbf29ce484222325;
        for c in &self.cells {
            fp = fp.wrapping_mul(0x100000001b3) ^ (c.plat.rank() as u64);
            fp = fp.wrapping_mul(0x100000001b3) ^ (c.case as u64);
        }
        for &b in config {
            fp = fp.wrapping_mul(0x100000001b3) ^ (b as u64);
        }
        self.ci_seal = Some(fp);
        Ok(fp)
    }

    /// 归档前置总检：矩阵齐 + 全绿 + 一致性 + 差异闭环 + CI 固化，
    /// 五关全过才许归档。
    pub fn pre_archive(&mut self, canon: &[u8]) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 14);
        }
        self.matrix_full()?;
        self.exec_green()?;
        self.consistency_green()?;
        self.diffs_closed()?;
        if self.ci_seal.is_none() {
            return Err(SEG_BASE + 15);
        }
        let mut fp: u64 = self.ci_seal.unwrap_or(0);
        for d in &self.diffs {
            fp = fp.wrapping_mul(0x100000001b3) ^ d.note_fp;
        }
        for &b in canon {
            fp = fp.wrapping_mul(0x100000001b3) ^ (b as u64);
        }
        if fp == 0 {
            return Err(SEG_BASE + 16);
        }
        self.archived = Some(fp);
        Ok(fp)
    }

    /// 归档指纹。
    pub fn archive_fp(&self) -> Option<u64> {
        self.archived
    }

    /// 平台覆盖普查：四平台各多少行（覆盖偏科可见）。
    pub fn platform_census(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for c in &self.cells {
            out[c.plat.rank() as usize] += 1;
        }
        out
    }
}

/// 用例覆盖均衡审计（独立函数面）：统计每个用例覆盖的平台数，
/// 要求全部用例至少覆盖 2 个平台（单平台用例不构成跨平台断言，
/// 存量占比超 25% = 矩阵是拼盘不是矩阵）。
pub fn case_balance_audit(cells: &[MatrixCell]) -> Result<u32, u16> {
    if cells.is_empty() {
        return Err(SEG_BASE + 17);
    }
    let mut cases: Vec<u32> = Vec::new();
    let mut counts: Vec<u8> = Vec::new();
    for c in cells {
        match cases.iter().position(|x| *x == c.case) {
            Some(i) => {
                let seen = counts[i];
                if (seen >> c.plat.rank()) & 1 == 0 {
                    counts[i] = seen | (1 << c.plat.rank());
                }
            }
            None => {
                cases.push(c.case);
                counts.push(1u8 << c.plat.rank());
            }
        }
    }
    let single = counts.iter().filter(|m| m.count_ones() < 2).count();
    let permille = (single * 1000) / cases.len();
    if permille > 250 {
        return Err(SEG_BASE + 18);
    }
    Ok(permille as u32)
}
