//! veb313_adrcert —— VE-F0313 Adreno 认证矩阵（开发板）。
//! 锚点（CoRun Unxreal 主册）：VE-F0313 · Adreno 认证矩阵（开发板） · 目标 340 行
//! 判据原文：VE-F0313-J1 认证矩阵账：开发板三块（DB845c /
//!           RB5 / QRD）；用例十族（A3xx 遗产一族 + A6 五族 +
//!           A7 四族）；逐格三态（过/败/跳）；板通过率（过/
//!           跑）；发板证门（率 ≥ 800 且无败格）；代际覆盖
//!           核（发证要求 A6 五族全过——遗产族跳不碍事）；
//!           补测翻绿；跳格立案。
//! 组件：板账 + 十族格 + 代际覆盖 + 发证闸。
//! 职责定位：Adreno 认证是「高通线的体检卡」——十族是十项
//!           检查：A6 五族是主干检查，漏一项不发证；遗产族
//!           是老年病科：跳过不扣分但必须记账；800 线比
//!           Mali 的 750 严：高通线是自家地盘标准更高。
//! 性能逐项分解：记格 O(1)；发证 O(格数)。
//! 诊断码段：0x31740（0x31741~0x31749 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const VC_VERSION: &str = "veb313_adrcert-v1";

/// 诊断段基址（0x31740 段 · 本模块独占）。
pub const VC_SEG_BASE: u32 = 0x31740;
pub const XC_MAGIC: u32 = 0x31741; // 账魔数破
pub const XC_BOARD: u32 = 0x31742; // 无此板
pub const XC_CASE: u32 = 0x31743; // 无此族
pub const XC_COVER: u32 = 0x31744; // 代际覆盖不足

/// 账魔数。
const MAGIC: u32 = 0x3174_1B19;

/// 三开发板。
pub const BOARDS: [&str; 3] = ["db845c", "rb5", "qrd"];

/// 用例十族。
pub const NCASE: usize = 10;

/// A6 主干五族（族号 1~5）。
pub const A6_CORE: [usize; 5] = [1, 2, 3, 4, 5];

/// 发证线（毫分比）。
pub const CERT_TH: u32 = 800;

/// 格态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Pass,
    Fail,
    Skip,
    Blank,
}

/// 板矩阵。
pub struct AdrMat {
    cells: [[Cell; NCASE]; 3],
    pub skips: u32,
    magic: u32,
}

impl AdrMat {
    /// 建。
    pub fn new() -> AdrMat {
        AdrMat { cells: [[Cell::Blank; NCASE]; 3], skips: 0, magic: MAGIC }
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
        if c >= NCASE {
            return Err(XC_CASE);
        }
        if st == Cell::Skip {
            self.skips += 1;
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
        if c >= NCASE {
            return Err(XC_CASE);
        }
        Ok(self.cells[b][c])
    }

    /// 板通过率（毫分比）。
    pub fn rate(&self, b: usize) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if b >= 3 {
            return Err(XC_BOARD);
        }
        let (mut p, mut f) = (0u32, 0u32);
        for c in 0..NCASE {
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

    /// 代际覆盖：A6 五族必须全过。
    pub fn a6_cover(&self, b: usize) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if b >= 3 {
            return Err(XC_BOARD);
        }
        for c in A6_CORE.iter() {
            if self.cells[b][*c] != Cell::Pass {
                return Err(XC_COVER);
            }
        }
        Ok(())
    }

    /// 发板证：率 ≥ 800 + 无败格 + A6 覆盖。
    pub fn cert(&self, b: usize) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if b >= 3 {
            return Err(XC_BOARD);
        }
        for c in 0..NCASE {
            if self.cells[b][c] == Cell::Fail {
                return Err(XC_CASE);
            }
        }
        self.a6_cover(b)?;
        let r = self.rate(b)?;
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
        if b >= 3 || c >= NCASE {
            return Err(XC_CASE);
        }
        if self.cells[b][c] == Cell::Fail {
            self.cells[b][c] = Cell::Pass;
        }
        Ok(())
    }

    /// 全阵证。
    pub fn array_cert(&self) -> Result<(), u32> {
        for b in 0..3 {
            self.cert(b)?;
        }
        Ok(())
    }
}

pub fn run_veb313_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0313");
    let mut m = AdrMat::new();
    // 族零：越界。
    s.add("板 3 拒", m.set(3, 0, Cell::Pass) == Err(XC_BOARD), "无四板");
    s.add("族 10 拒", m.set(0, 10, Cell::Pass) == Err(XC_CASE), "无十一族");
    // 族一：板 0 全绿发证。
    for c in 0..NCASE {
        let _ = m.set(0, c, Cell::Pass);
    }
    s.add("率 1000", m.rate(0) == Ok(1000), "全绿");
    s.add("A6 覆盖", m.a6_cover(0).is_ok(), "主干全过");
    s.add("发证", m.cert(0).is_ok(), "三闸全过");
    // 族二：遗产族跳不碍事。
    let mut k = AdrMat::new();
    let _ = k.set(0, 0, Cell::Skip); // 遗产族跳
    for c in 1..NCASE {
        let _ = k.set(0, c, Cell::Pass);
    }
    s.add("跳立案 1", k.skips == 1, "记一笔");
    s.add("率 1000", k.rate(0) == Ok(1000), "9/9");
    s.add("A6 覆盖仍过", k.a6_cover(0).is_ok(), "主干在");
    s.add("跳遗产发证", k.cert(0).is_ok(), "不碍事");
    // 族三：败格一票。
    let mut f = AdrMat::new();
    for c in 0..NCASE {
        let _ = f.set(0, c, Cell::Pass);
    }
    let _ = f.set(0, 7, Cell::Fail); // A7 族败
    s.add("有败拒证", f.cert(0) == Err(XC_CASE), "败格一票");
    s.add("率 900", f.rate(0) == Ok(900), "9/10");
    // 补测翻绿发证。
    let _ = f.retest(0, 7);
    s.add("补测翻绿", f.cell(0, 7) == Ok(Cell::Pass), "重跑过");
    s.add("补后发证", f.cert(0).is_ok(), "终绿");
    // 族四：A6 覆盖闸。
    let mut c = AdrMat::new();
    // 全跳：率 0 且覆盖不足。
    for c2 in 0..NCASE {
        let _ = c.set(0, c2, Cell::Skip);
    }
    s.add("全跳覆盖败", c.a6_cover(0) == Err(XC_COVER), "主干缺");
    s.add("全跳拒证", c.cert(0) == Err(XC_COVER), "覆盖闸先于率");
    // 只有 A6 五族过、其余跳：覆盖过但率 1000（5/5）→ 发证。
    let mut c2 = AdrMat::new();
    for a in A6_CORE.iter() {
        let _ = c2.set(0, *a, Cell::Pass);
    }
    for c3 in [0usize, 6, 7, 8, 9] {
        let _ = c2.set(0, c3, Cell::Skip);
    }
    s.add("仅主干覆盖过", c2.a6_cover(0).is_ok(), "A6 齐");
    s.add("仅主干发证", c2.cert(0).is_ok(), "5/5=1000");
    // A6 缺一 → 覆盖拒。
    let mut c3 = AdrMat::new();
    for a in [1usize, 2, 3, 4] {
        let _ = c3.set(0, a, Cell::Pass);
    }
    let _ = c3.set(0, 5, Cell::Skip);
    s.add("A6 缺 5 拒", c3.a6_cover(0) == Err(XC_COVER), "主干缺一");
    // 族五：全阵证。
    let mut all = AdrMat::new();
    for b in 0..3 {
        for c4 in 0..NCASE {
            let _ = all.set(b, c4, Cell::Pass);
        }
    }
    s.add("阵证过", all.array_cert().is_ok(), "三板齐");
    // 拆一板。
    let mut half = AdrMat::new();
    for b in 0..2 {
        for c4 in 0..NCASE {
            let _ = half.set(b, c4, Cell::Pass);
        }
    }
    s.add("缺板拒阵", half.array_cert() == Err(XC_COVER), "板 2 空覆盖败");
    s.add("魔数核", VC_SEG_BASE == 0x31740, "段基址");
    s
}
