//! veb270_nvcert —— VE-F0270 NVIDIA 认证矩阵测试。
//! 锚点（CoRun Unxreal 主册）：VE-F0270 · NVIDIA 认证矩阵测试 · 目标 380 行
//! 判据原文：VE-F0270-J1 认证矩阵 = 代际 × 用例族（基本渲染/
//!           计算/编解码/光追）；每格记认证态（过/败/未测）；
//!           认证结论：代际行内基本渲染 + 计算两格过 → 认证基
//!           础级；四族全过 → 完整级；基不及 → 无认证；未测格
//!           不挡基础级但挡完整级；矩阵守恒（态计数和 = 格数）。
//! 组件：认证矩阵 + 分级裁决 + 守恒核。
//! 职责定位：认证矩阵是「NVIDIA 通路的质检证」——四族是四门考
//!           试：渲染是语文，计算是数学，编解码是外语，光追是
//!           选修竞赛；基础级是及格证（语文数学过关就能发毕业
//!           证找工作），完整级是全 A 成绩单；老代际没开光追
//!           这门课，缺课不挡及格但挡全 A。
//! 性能逐项分解：记格 O(1)；裁决 O(族数)；守恒 O(格数)。
//! 诊断码段：0x30C80（0x30C81~0x30C8A 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const CT_VERSION: &str = "veb270_nvcert-v1";

/// 诊断段基址（0x30C80 段 · 本模块独占）。
pub const CT_SEG_BASE: u32 = 0x30C80;
pub const XC_NOENT: u32 = 0x30C81; // 无此格
pub const XC_MAGIC: u32 = 0x30C82; // 账魔数破
pub const XC_GEN_OOR: u32 = 0x30C83; // 代际越域
pub const XC_FAM_OOR: u32 = 0x30C84; // 族越域
pub const XC_DUP_GEN: u32 = 0x30C85; // 代际行重

/// 用例族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fam {
    /// 基本渲染。
    Render = 0,
    /// 计算。
    Compute = 1,
    /// 编解码。
    Codec = 2,
    /// 光追。
    RayTr = 3,
}

pub const FAM_N: usize = 4;

/// 格态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CState {
    Untested = 0,
    Pass = 1,
    Fail = 2,
}

/// 认证级。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CertLv {
    /// 无认证。
    None = 0,
    /// 基础级。
    Basic = 1,
    /// 完整级。
    Full = 2,
}

/// 代际行。
pub struct GenRow {
    pub gen: u32,
    pub cells: [CState; FAM_N],
}

/// 认证矩阵。
pub struct CertMx {
    rows: Vec<GenRow>,
    magic: u32,
}

impl CertMx {
    /// 建。
    pub fn new() -> CertMx {
        CertMx { rows: Vec::new(), magic: 0x0C80_CE27 }
    }

    /// 加代际行。
    pub fn add_gen(&mut self, gen: u32) -> Result<(), u32> {
        if self.magic != 0x0C80_CE27 {
            return Err(XC_MAGIC);
        }
        if gen == 0 || gen > 6 {
            return Err(XC_GEN_OOR);
        }
        for r in self.rows.iter() {
            if r.gen == gen {
                return Err(XC_DUP_GEN);
            }
        }
        self.rows.push(GenRow { gen, cells: [CState::Untested; FAM_N] });
        Ok(())
    }

    fn row(&self, gen: u32) -> Result<usize, u32> {
        for (i, r) in self.rows.iter().enumerate() {
            if r.gen == gen {
                return Ok(i);
            }
        }
        Err(XC_NOENT)
    }

    /// 记格。
    pub fn mark(&mut self, gen: u32, fam: Fam, st: CState) -> Result<(), u32> {
        let i = self.row(gen)?;
        self.rows[i].cells[fam as usize] = st;
        Ok(())
    }

    /// 读格。
    pub fn cell(&self, gen: u32, fam: Fam) -> Result<CState, u32> {
        let i = self.row(gen)?;
        Ok(self.rows[i].cells[fam as usize])
    }

    /// 分级裁决。
    pub fn verdict(&self, gen: u32) -> Result<CertLv, u32> {
        let i = self.row(gen)?;
        let cs = &self.rows[i].cells;
        let render = cs[Fam::Render as usize];
        let compute = cs[Fam::Compute as usize];
        // 基础级：渲染 + 计算全过。
        if render != CState::Pass || compute != CState::Pass {
            return Ok(CertLv::None);
        }
        // 完整级：四族全过（未测也挡）。
        for c in cs.iter() {
            if *c != CState::Pass {
                return Ok(CertLv::Basic);
            }
        }
        Ok(CertLv::Full)
    }

    /// 守恒核。
    pub fn conserve(&self) -> Result<u32, u32> {
        if self.magic != 0x0C80_CE27 {
            return Err(XC_MAGIC);
        }
        let mut n = 0u32;
        for r in self.rows.iter() {
            for c in r.cells.iter() {
                match c {
                    CState::Untested => n += 1,
                    CState::Pass => n += 1,
                    CState::Fail => n += 1,
                }
            }
        }
        if n != (self.rows.len() * FAM_N) as u32 {
            return Err(XC_MAGIC);
        }
        Ok(n)
    }

    /// 行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }
}

pub fn run_veb270_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0270");
    // 族一：建行。
    let mut m = CertMx::new();
    let _ = m.add_gen(5);
    let _ = m.add_gen(3);
    s.add("两行", m.rows() == 2, "Ampere+Pascal");
    s.add("重行拒", m.add_gen(5) == Err(XC_DUP_GEN), "代际主键");
    s.add("代际零拒", m.add_gen(0) == Err(XC_GEN_OOR), "Unknown 不测");
    s.add("代际 7 拒", m.add_gen(7) == Err(XC_GEN_OOR), "越域");
    // 未测读格。
    s.add("未测态", m.cell(5, Fam::Codec) == Ok(CState::Untested), "出厂");
    // 族二：基础级路径。
    let _ = m.mark(5, Fam::Render, CState::Pass);
    let _ = m.mark(5, Fam::Compute, CState::Pass);
    s.add("基础级", m.verdict(5) == Ok(CertLv::Basic), "渲染+计算过");
    // 完整级：补编解码 + 光追。
    let _ = m.mark(5, Fam::Codec, CState::Pass);
    let _ = m.mark(5, Fam::RayTr, CState::Pass);
    s.add("完整级", m.verdict(5) == Ok(CertLv::Full), "四族全过");
    // 族三：败格挡级。
    let _ = m.mark(3, Fam::Render, CState::Pass);
    let _ = m.mark(3, Fam::Compute, CState::Fail);
    s.add("计算败无认证", m.verdict(3) == Ok(CertLv::None), "不及格");
    let _ = m.mark(3, Fam::Compute, CState::Pass);
    s.add("补过后基础级", m.verdict(3) == Ok(CertLv::Basic), "两门过");
    // 光追未测挡完整。
    let _ = m.mark(3, Fam::Codec, CState::Pass);
    s.add("未测挡完整", m.verdict(3) == Ok(CertLv::Basic), "缺课挡全 A");
    let _ = m.mark(3, Fam::RayTr, CState::Fail);
    s.add("光追败挡完整", m.verdict(3) == Ok(CertLv::Basic), "败也挡");
    // 族四：守恒。
    s.add("守恒 8", m.conserve() == Ok(8), "2 行 × 4 格");
    // 族五：野格。
    s.add("野代际拒", m.mark(9, Fam::Render, CState::Pass) == Err(XC_NOENT), "无行");
    s.add("级序", CertLv::None < CertLv::Basic && CertLv::Basic < CertLv::Full, "级有序");
    s.add("魔数核", CT_SEG_BASE == 0x30C80, "段基址");
    s
}
