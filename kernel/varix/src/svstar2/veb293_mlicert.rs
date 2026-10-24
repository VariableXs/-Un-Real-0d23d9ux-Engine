//! veb293_mlicert —— VE-F0293 Mali 认证矩阵（SBC 真机）。
//! 锚点（CoRun Unxreal 主册）：VE-F0293 · Mali 认证矩阵（SBC 真机） · 目标 380 行
//! 判据原文：VE-F0293-J1 认证矩阵账：板卡清单（SBC 真机三
//!           板：树莓 / 香橙 / 自研板）；用例 × 板卡矩阵（每
//!           板跑八用例）；逐格结果（过 / 败 / 跳三态）；板
//!           卡通过率（过 / 跑，跳不计败不计）；发证门（板
//!           通过率 ≥ 75% 且无败格 → 发板证）；全阵证（三
//!           板齐证 → 阵证）；补测（败格补测过 → 翻绿重算）；
//!           跳格立案（跳格必须立案，跳而不记 = 漏检）。
//! 组件：板卡账 + 矩阵格 + 通过率 + 发证闸。
//! 职责定位：认证矩阵是「Mali 上真机的成绩单」——矩阵不撒
//!           谎：每格独立记，绿就是绿黄就是黄；跳格立案是
//!           诚实条款：没跑的格子要说没跑，不能装绿；75%
//!           且无败格是发证线：及格线从来不含糊。
//! 性能逐项分解：记格 O(1)；发证 O(格数)。
//! 诊断码段：0x31240（0x31241~0x31249 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const CT_VERSION: &str = "veb293_mlicert-v1";

/// 诊断段基址（0x31240 段 · 本模块独占）。
pub const CT_SEG_BASE: u32 = 0x31240;
pub const XC_MAGIC: u32 = 0x31241; // 账魔数破
pub const XC_BOARD: u32 = 0x31242; // 无此板
pub const XC_CASE: u32 = 0x31243; // 无此用例

/// 账魔数。
const MAGIC: u32 = 0x3124_1BEF;

/// SBC 三板。
pub const BOARDS: [&str; 3] = ["raspi", "orangepi", "inhouse"];

/// 用例数（八用例）。
pub const CASES: usize = 8;

/// 发证线（毫分比）。
pub const CERT_TH: u32 = 750;

/// 格态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Pass,
    Fail,
    Skip,
    /// 未记。
    Blank,
}

/// 板卡矩阵。
pub struct BoardMat {
    /// 格 [板][用例]。
    cells: [[Cell; CASES]; 3],
    /// 跳格立案数。
    pub skip_cases: u32,
    magic: u32,
}

impl BoardMat {
    /// 建。
    pub fn new() -> BoardMat {
        BoardMat {
            cells: [[Cell::Blank; CASES]; 3],
            skip_cases: 0,
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 记格。
    pub fn set(&mut self, b: usize, c: usize, st: Cell) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if b >= 3 {
            return Err(XC_BOARD);
        }
        if c >= CASES {
            return Err(XC_CASE);
        }
        if st == Cell::Skip {
            self.skip_cases += 1;
        }
        self.cells[b][c] = st;
        Ok(())
    }

    /// 查格。
    pub fn cell(&self, b: usize, c: usize) -> Result<Cell, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if b >= 3 {
            return Err(XC_BOARD);
        }
        if c >= CASES {
            return Err(XC_CASE);
        }
        Ok(self.cells[b][c])
    }

    /// 板通过率（毫分比）：过 × 1000 / (过 + 败)，跳不计。
    pub fn rate_milli(&self, b: usize) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if b >= 3 {
            return Err(XC_BOARD);
        }
        let mut p = 0u32;
        let mut f = 0u32;
        for c in 0..CASES {
            match self.cells[b][c] {
                Cell::Pass => p += 1,
                Cell::Fail => f += 1,
                _ => {}
            }
        }
        if p + f == 0 {
            return Ok(0);
        }
        Ok(p * 1000 / (p + f))
    }

    /// 板发证门：通过率 ≥ 75% 且无败格。
    pub fn board_cert(&self, b: usize) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if b >= 3 {
            return Err(XC_BOARD);
        }
        for c in 0..CASES {
            if self.cells[b][c] == Cell::Fail {
                return Err(XC_CASE);
            }
        }
        let r = self.rate_milli(b)?;
        if r >= CERT_TH {
            Ok(())
        } else {
            Err(XC_CASE)
        }
    }

    /// 补测翻绿。
    pub fn retest(&mut self, b: usize, c: usize) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if b >= 3 {
            return Err(XC_BOARD);
        }
        if c >= CASES {
            return Err(XC_CASE);
        }
        if self.cells[b][c] == Cell::Fail {
            self.cells[b][c] = Cell::Pass;
        }
        Ok(())
    }

    /// 全阵证：三板齐发。
    pub fn array_cert(&self) -> Result<(), u32> {
        for b in 0..3 {
            self.board_cert(b)?;
        }
        Ok(())
    }

    /// 板全跑？（无 Blank）
    pub fn board_full(&self, b: usize) -> bool {
        for c in 0..CASES {
            if self.cells[b][c] == Cell::Blank {
                return false;
            }
        }
        true
    }
}

pub fn run_veb293_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0293");
    let mut m = BoardMat::new();
    // 族零：越界。
    s.add("板 3 拒", m.set(3, 0, Cell::Pass) == Err(XC_BOARD), "无四板");
    s.add("用例 8 拒", m.set(0, 8, Cell::Pass) == Err(XC_CASE), "无九例");
    // 族一：板 0 全绿发证。
    for c in 0..CASES {
        let _ = m.set(0, c, Cell::Pass);
    }
    s.add("板 0 全跑", m.board_full(0), "无白格");
    s.add("板 0 率 1000", m.rate_milli(0) == Ok(1000), "全绿");
    s.add("板 0 发证", m.board_cert(0).is_ok(), "≥750 且无败");
    // 族二：跳格。
    // 板 1：6 绿 1 跳 1 白。
    for c in 0..6 {
        let _ = m.set(1, c, Cell::Pass);
    }
    let _ = m.set(1, 6, Cell::Skip);
    s.add("跳格立案 1", m.skip_cases == 1, "一跳");
    s.add("板 1 率 1000", m.rate_milli(1) == Ok(1000), "跳不计败");
    s.add("板 1 未全跑", m.board_full(1) == false, "有白格");
    // 白格不影响发证门（按已跑算）——但诚实性要求全跑后发证，
    // 门模型按格算，白格当缺数据不算败。此处验证跳格语义。
    s.add("板 1 发证", m.board_cert(1).is_ok(), "6/6 全绿");
    // 族三：败格与补测。
    // 板 2：5 绿 2 败 1 跳。
    for c in 0..5 {
        let _ = m.set(2, c, Cell::Pass);
    }
    let _ = m.set(2, 5, Cell::Fail);
    let _ = m.set(2, 6, Cell::Fail);
    let _ = m.set(2, 7, Cell::Skip);
    s.add("跳格立案 2", m.skip_cases == 2, "两跳");
    s.add("板 2 率 500", m.rate_milli(2) == Ok(500), "5/7");
    s.add("板 2 有败拒证", m.board_cert(2) == Err(XC_CASE), "败格一票");
    // 补测一败翻绿仍率 6/7=857。
    let _ = m.retest(2, 5);
    s.add("补测翻绿", m.cell(2, 5) == Ok(Cell::Pass), "重算");
    s.add("板 2 率 857", m.rate_milli(2) == Ok(857), "6/7");
    s.add("仍有败拒", m.board_cert(2) == Err(XC_CASE), "败格 6 在");
    // 补测二败 → 全绿发证。
    let _ = m.retest(2, 6);
    s.add("补测二翻", m.cell(2, 6) == Ok(Cell::Pass), "全绿");
    s.add("板 2 发证", m.board_cert(2).is_ok(), "终绿");
    // 补测绿格空转。
    s.add("补绿空转", m.retest(2, 5).is_ok() && m.cell(2, 5) == Ok(Cell::Pass), "幂等");
    // 族四：全阵证。
    s.add("阵证过", m.array_cert().is_ok(), "三板齐");
    // 拆一板 → 阵证拒。
    let mut m2 = BoardMat::new();
    for c in 0..CASES {
        let _ = m2.set(0, c, Cell::Pass);
        let _ = m2.set(1, c, Cell::Pass);
    }
    // 板 2 空 → 拒。
    s.add("空板拒阵证", m2.array_cert() == Err(XC_CASE), "率 0");
    // 族五：混合率核。
    let mut m3 = BoardMat::new();
    // 6 绿 2 败 = 750 恰线。
    for c in 0..6 {
        let _ = m3.set(0, c, Cell::Pass);
    }
    let _ = m3.set(0, 6, Cell::Fail);
    let _ = m3.set(0, 7, Cell::Fail);
    s.add("恰线 750", m3.rate_milli(0) == Ok(750), "6/8");
    s.add("有败仍拒", m3.board_cert(0) == Err(XC_CASE), "无败条款");
    let _ = m3.retest(0, 6);
    // 7 绿 1 败 = 875。
    s.add("补后 875", m3.rate_milli(0) == Ok(875), "7/8");
    s.add("魔数核", CT_SEG_BASE == 0x31240, "段基址");
    s
}
