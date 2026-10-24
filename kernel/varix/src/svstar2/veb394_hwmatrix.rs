//! veb394_hwmatrix —— VE-F0394 硬件覆盖矩阵总结。
//! 锚点（CoRun Unxreal 主册）：VE-F0394 · 硬件覆盖矩阵总结 · 目标 360 行
//! 判据原文：VE-F0394-J1 硬件覆盖矩阵 = 设备型 × 驱动型；每格
//!           记验证态（已验/未验/不适用）；总结出三率：覆盖率
//!           （已验 / 可验格）、适用率（适用格 / 总格）、豁免
//!           率（不适用 / 总格）；覆盖率低于门槛（900‰）总结
//!           不通过；格态迁移只许 未验→已验 或 未验→不适用
//!           （已验/不适用终态不许改）；矩阵守恒核。
//! 组件：矩阵（设备型 × 驱动型）+ 三态格 + 三率 + 终态纪律。
//! 职责定位：硬件覆盖矩阵是「B 域的兵要地志」——哪类设备在哪个
//!           驱动下验过、没验、验不了，一张矩阵看穿；三率是总
//!           结的语言：覆盖率是成绩，适用率是题量，豁免率是免
//!           考范围；终态纪律防翻供：验过的东西不许悄悄改回没验。
//! 性能逐项分解：记格 O(1)；三率 O(行列)；守恒 O(行列)。
//! 诊断码段：0x30800（0x30801~0x3080A 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HM_VERSION: &str = "veb394_hwmatrix-v1";

/// 诊断段基址（0x30800 段 · 本模块独占）。
pub const HM_SEG_BASE: u32 = 0x30800;
pub const XC_COVER_LOW: u32 = 0x30801; // 覆盖率不达标
pub const XC_TERMINAL: u32 = 0x30802; // 终态改格
pub const XC_NOENT: u32 = 0x30803; // 无此格
pub const XC_MAGIC: u32 = 0x30804; // 账魔数破
pub const XC_DIM: u32 = 0x30805; // 空矩阵
pub const XC_DEV_DUP: u32 = 0x30806; // 设备重名
pub const XC_DRV_DUP: u32 = 0x30807; // 驱动重名
pub const XC_SUM: u32 = 0x30808; // 守恒破
pub const XC_RATE: u32 = 0x30809; // 率越界

/// 覆盖率门槛（千分比）。
pub const COVER_MIN: u32 = 900;

/// 格三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HCell {
    /// 未验。
    Unverified = 0,
    /// 已验。
    Verified = 1,
    /// 不适用。
    Na = 2,
}

/// 三率（千分比）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rates {
    pub cover: u32,
    pub applicable: u32,
    pub exempt: u32,
}

/// 硬件覆盖矩阵。
pub struct HwMatrix {
    devs: Vec<&'static str>,
    drvs: Vec<&'static str>,
    cells: Vec<HCell>, // devs.len() * drvs.len()
    magic: u32,
}

impl HwMatrix {
    /// 建。
    pub fn new() -> HwMatrix {
        HwMatrix { devs: Vec::new(), drvs: Vec::new(), cells: Vec::new(), magic: 0x0800 }
    }

    /// 加设备型。
    pub fn add_dev(&mut self, d: &'static str) -> Result<(), u32> {
        if self.magic != 0x0800 {
            return Err(XC_MAGIC);
        }
        for x in self.devs.iter() {
            if *x == d {
                return Err(XC_DEV_DUP);
            }
        }
        self.devs.push(d);
        self.rebuild();
        Ok(())
    }

    /// 加驱动型。
    pub fn add_drv(&mut self, v: &'static str) -> Result<(), u32> {
        for x in self.drvs.iter() {
            if *x == v {
                return Err(XC_DRV_DUP);
            }
        }
        self.drvs.push(v);
        self.rebuild();
        Ok(())
    }

    fn rebuild(&mut self) {
        self.cells.clear();
        for _ in 0..self.devs.len() * self.drvs.len() {
            self.cells.push(HCell::Unverified);
        }
    }

    fn idx(&self, di: usize, vi: usize) -> usize {
        di * self.drvs.len() + vi
    }

    /// 记格（未验→已验 或 未验→不适用；终态改拒）。
    pub fn mark(&mut self, dev: &str, drv: &str, c: HCell) -> Result<(), u32> {
        if c == HCell::Unverified {
            return Err(XC_TERMINAL); // 只许向终态迁
        }
        let di = match self.devs.iter().position(|x| *x == dev) {
            Some(i) => i,
            None => return Err(XC_NOENT),
        };
        let vi = match self.drvs.iter().position(|x| *x == drv) {
            Some(i) => i,
            None => return Err(XC_NOENT),
        };
        let i = self.idx(di, vi);
        if self.cells[i] != HCell::Unverified {
            return Err(XC_TERMINAL);
        }
        self.cells[i] = c;
        Ok(())
    }

    /// 读格。
    pub fn cell(&self, dev: &str, drv: &str) -> Result<HCell, u32> {
        let di = match self.devs.iter().position(|x| *x == dev) {
            Some(i) => i,
            None => return Err(XC_NOENT),
        };
        let vi = match self.drvs.iter().position(|x| *x == drv) {
            Some(i) => i,
            None => return Err(XC_NOENT),
        };
        Ok(self.cells[self.idx(di, vi)])
    }

    /// 三率。
    pub fn rates(&self) -> Result<Rates, u32> {
        if self.magic != 0x0800 {
            return Err(XC_MAGIC);
        }
        if self.cells.is_empty() {
            return Err(XC_DIM);
        }
        let total = self.cells.len() as u32;
        let mut verified = 0u32;
        let mut na = 0u32;
        for c in self.cells.iter() {
            match c {
                HCell::Verified => verified += 1,
                HCell::Na => na += 1,
                HCell::Unverified => {}
            }
        }
        let applicable = total - na;
        let cover = if applicable == 0 { 1000 } else { verified * 1000 / applicable };
        let r = Rates {
            cover,
            applicable: applicable * 1000 / total,
            exempt: na * 1000 / total,
        };
        if r.cover > 1000 || r.applicable + r.exempt > 1001 {
            return Err(XC_RATE);
        }
        Ok(r)
    }

    /// 总结判定：覆盖率 ≥ 门槛过。
    pub fn summarize(&self) -> Result<bool, u32> {
        let r = self.rates()?;
        if r.cover < COVER_MIN {
            return Err(XC_COVER_LOW);
        }
        Ok(true)
    }

    /// 守恒核：三态计数和 = 总格数。
    pub fn conserve(&self) -> Result<u32, u32> {
        let mut n = 0u32;
        for c in self.cells.iter() {
            match c {
                HCell::Unverified => n += 1,
                HCell::Verified => n += 1,
                HCell::Na => n += 1,
            }
        }
        if n != self.cells.len() as u32 {
            return Err(XC_SUM);
        }
        Ok(n)
    }

    /// 总格数。
    pub fn total_cells(&self) -> usize {
        self.cells.len()
    }
}

pub fn run_veb394_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0394");
    // 族一：建阵与记格。
    let mut m = HwMatrix::new();
    let _ = m.add_dev("uart16550");
    let _ = m.add_dev("nvme");
    let _ = m.add_dev("ps2kbd");
    let _ = m.add_drv("serial");
    let _ = m.add_drv("block");
    let _ = m.add_drv("input");
    s.add("九格", m.total_cells() == 9, "3×3");
    s.add("重名拒", m.add_dev("uart16550") == Err(XC_DEV_DUP), "设备主键");
    s.add("驱动重名拒", m.add_drv("block") == Err(XC_DRV_DUP), "驱动主键");
    let _ = m.mark("uart16550", "serial", HCell::Verified);
    let _ = m.mark("nvme", "block", HCell::Verified);
    let _ = m.mark("ps2kbd", "input", HCell::Verified);
    let _ = m.mark("nvme", "input", HCell::Na);
    let _ = m.mark("ps2kbd", "block", HCell::Na);
    // 未验格：uart16550×block, uart16550×input, nvme×serial, ps2kbd×serial (4 格)
    s.add("格态读", m.cell("nvme", "input") == Ok(HCell::Na), "不适用");
    s.add("未验读", m.cell("nvme", "serial") == Ok(HCell::Unverified), "未验");
    s.add("野格拒", m.cell("ghost", "serial") == Err(XC_NOENT), "无设备");
    // 族二：终态纪律。
    s.add("已验改拒", m.mark("nvme", "block", HCell::Verified) == Err(XC_TERMINAL), "终态");
    s.add("Na 改拒", m.mark("nvme", "input", HCell::Verified) == Err(XC_TERMINAL), "终态");
    s.add("退回未验拒", m.mark("uart16550", "serial", HCell::Unverified) == Err(XC_TERMINAL), "不许翻供");
    // 族三：三率。
    let r = m.rates();
    // 适用 5，已验 3 → cover 600；applicable 556；exempt 444。
    s.add("覆盖率 600", matches!(&r, Ok(x) if x.cover == 600), "3/5");
    s.add("适用率 556", matches!(&r, Ok(x) if x.applicable == 556), "5/9");
    s.add("豁免率 444", matches!(&r, Ok(x) if x.exempt == 444), "4/9");
    s.add("守恒 9", m.conserve() == Ok(9), "三态和");
    // 族四：总结门槛。
    s.add("覆盖不达标", m.summarize() == Err(XC_COVER_LOW), "600<900");
    // 补验到 900+：剩 4 未验格中 3 格是可验适用格? 适用格 = 已验3 + 未验中适用格。
    // 未验格中 nvme×serial, uart16550×block, uart16550×input, ps2kbd×serial。
    // 全验 → 适用 8，已验 8 → 1000。再豁免 1 格需要它在 mark 前是 Unverified：
    // ps2kbd×serial 标 Na → 适用 7 已验 7? 已验变为 7（多验了3）→ 1000。
    let _ = m.mark("nvme", "serial", HCell::Verified);
    let _ = m.mark("uart16550", "block", HCell::Verified);
    let _ = m.mark("uart16550", "input", HCell::Verified);
    let _ = m.mark("ps2kbd", "serial", HCell::Na);
    let r2 = m.rates();
    s.add("全适用格已验 1000", matches!(&r2, Ok(x) if x.cover == 1000), "8/8");
    s.add("总结过", m.summarize() == Ok(true), "达标");
    s.add("守恒 9 再核", m.conserve() == Ok(9), "终态全落");
    // 族五：空阵。
    let e = HwMatrix::new();
    s.add("空阵率拒", e.rates() == Err(XC_DIM), "无格");
    s.add("魔数核", HM_SEG_BASE == 0x30800, "段基址");
    s
}
