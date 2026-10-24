//! veb314_pvrcert —— VE-F0314 PowerVR 认证矩阵预留。
//! 锚点（CoRun Unxreal 主册）：VE-F0314 · PowerVR 认证矩阵预留 · 目标 300 行
//! 判据原文：VE-F0314-J1 预留矩阵账：矩阵骨架（三板 × 六族
//!           预留格）；预留态（全 Blank 起步）；骨架完整性
//!           （板与族界恒拒越界）；预约登记（板卡到货登记
//!           → 板激活）；激活门槛（激活板才可记格）；预留
//!           填充率（已记格 / 总格，毫分比）；冻结预约核
//!           （未激活板记格拒）；转移核（预留矩阵可整包转
//!           正式矩阵：三数出账）。
//! 组件：骨架账 + 激活闸 + 填充率 + 转移。
//! 职责定位：预留矩阵是「PVR 线的毛坯房」——格子都打好但
//!           没人住：激活是拿钥匙、记格是搬家；未激活记格是
//!           撬锁：门都没有谈什么检查；转移是毛坯变精装：
//!           结构不动只换门牌。
//! 性能逐项分解：记格 O(1)。
//! 诊断码段：0x31780（0x31781~0x31789 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const PR_VERSION: &str = "veb314_pvrcert-v1";

/// 诊断段基址（0x31780 段 · 本模块独占）。
pub const PR_SEG_BASE: u32 = 0x31780;
pub const XC_MAGIC: u32 = 0x31781; // 账魔数破
pub const XC_BOARD: u32 = 0x31782; // 无此板/未激活
pub const XC_CASE: u32 = 0x31783; // 无此族
pub const XC_INACTIVE: u32 = 0x31784; // 未激活记格

/// 账魔数。
const MAGIC: u32 = 0x3178_1B1A;

/// 三板位。
pub const NBOARD: usize = 3;
/// 六族位。
pub const NCASE: usize = 6;

/// 格态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Pass,
    Fail,
    Skip,
    Blank,
}

/// 预留矩阵。
pub struct ResMat {
    cells: [[Cell; NCASE]; NBOARD],
    /// 板激活旗。
    pub active: [bool; NBOARD],
    magic: u32,
}

impl ResMat {
    /// 建：全 Blank 全未激活。
    pub fn new() -> ResMat {
        ResMat {
            cells: [[Cell::Blank; NCASE]; NBOARD],
            active: [false; NBOARD],
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 板到货激活。
    pub fn activate(&mut self, b: usize) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if b >= NBOARD {
            return Err(XC_BOARD);
        }
        self.active[b] = true;
        Ok(())
    }

    /// 激活？界外恒否。
    pub fn is_active(&self, b: usize) -> bool {
        b < NBOARD && self.active[b]
    }

    /// 记格（未激活拒）。
    pub fn set(&mut self, b: usize, c: usize, st: Cell) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if b >= NBOARD {
            return Err(XC_BOARD);
        }
        if c >= NCASE {
            return Err(XC_CASE);
        }
        if !self.active[b] {
            return Err(XC_INACTIVE);
        }
        self.cells[b][c] = st;
        Ok(())
    }

    /// 查格。
    pub fn cell(&self, b: usize, c: usize) -> Result<Cell, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if b >= NBOARD {
            return Err(XC_BOARD);
        }
        if c >= NCASE {
            return Err(XC_CASE);
        }
        Ok(self.cells[b][c])
    }

    /// 填充率（已记格 / 总格，毫分比）。
    pub fn fill_milli(&self) -> u32 {
        let total = (NBOARD * NCASE) as u32;
        let mut n = 0;
        for b in 0..NBOARD {
            for c in 0..NCASE {
                if self.cells[b][c] != Cell::Blank {
                    n += 1;
                }
            }
        }
        n * 1000 / total
    }

    /// 已记格数。
    pub fn filled(&self) -> u32 {
        let mut n = 0;
        for b in 0..NBOARD {
            for c in 0..NCASE {
                if self.cells[b][c] != Cell::Blank {
                    n += 1;
                }
            }
        }
        n
    }

    /// 激活板数。
    pub fn active_count(&self) -> u32 {
        let mut n = 0;
        for a in self.active.iter() {
            if *a {
                n += 1;
            }
        }
        n
    }

    /// 转移三数（板激活数 / 已记格 / 总格）。
    pub fn transfer_summary(&self) -> (u32, u32, u32) {
        (self.active_count(), self.filled(), (NBOARD * NCASE) as u32)
    }
}

pub fn run_veb314_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0314");
    let mut m = ResMat::new();
    // 族零：骨架完整性（界恒拒）。
    s.add("板 3 拒", m.activate(3) == Err(XC_BOARD), "无四板位");
    s.add("记格板 3 拒", m.set(3, 0, Cell::Pass) == Err(XC_BOARD), "越界");
    s.add("记格族 6 拒", m.set(0, 6, Cell::Pass) == Err(XC_CASE), "越界");
    // 族一：未激活拒记。
    s.add("未激活记拒", m.set(0, 0, Cell::Pass) == Err(XC_INACTIVE), "撬锁拒");
    s.add("激活查否", m.is_active(0) == false, "未到货");
    // 族二：激活。
    s.add("激活板 0", m.activate(0).is_ok(), "到货");
    s.add("激活查真", m.is_active(0), "可记格");
    s.add("板 1 仍未激活", m.is_active(1) == false, "未到货");
    // 激活后可记。
    s.add("记格过", m.set(0, 0, Cell::Pass).is_ok(), "搬家");
    s.add("查格过", m.cell(0, 0) == Ok(Cell::Pass), "在住");
    // 板 1 未激活仍拒。
    s.add("板 1 记拒", m.set(1, 0, Cell::Pass) == Err(XC_INACTIVE), "未拿钥匙");
    // 族三：填充率。
    s.add("填 1 格", m.fill_milli() == 1000 / (NBOARD * NCASE) as u32, "1/18");
    // 激活板 1 板 2 各记三格。
    let _ = m.activate(1);
    let _ = m.activate(2);
    for c in 0..3 {
        let _ = m.set(1, c, Cell::Pass);
    }
    for c in 0..3 {
        let _ = m.set(2, c, Cell::Skip);
    }
    s.add("填 7 格", m.filled() == 7, "1+3+3");
    // 7*1000/18 = 388。
    s.add("填充率 388", m.fill_milli() == 388, "7/18");
    // 族四：转移三数。
    let (act, fill, total) = m.transfer_summary();
    s.add("转出激活 3", act == 3, "三板");
    s.add("转出已填 7", fill == 7, "七格");
    s.add("转出总格 18", total == 18, "骨架");
    // 激活幂等。
    s.add("重复激活", m.activate(0).is_ok() && m.active_count() == 3, "幂等");
    // 全 Blank 起点。
    let z = ResMat::new();
    s.add("毛坯零填", z.fill_milli() == 0 && z.filled() == 0, "全 Blank");
    s.add("毛坯零激活", z.active_count() == 0, "无钥匙");
    s.add("魔数核", PR_SEG_BASE == 0x31780, "段基址");
    s
}
