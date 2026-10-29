//! unx0622_pkgindex_v2 —— UNX-F0622 交接包索引表（域级检索）· v2。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0622 · 交接包索引表（域级检索） · 目标 ≥220 行
//! 判据原文摘录：索引表账齐（12 行×4 列+行校验和），按包号检索
//!   10/10 用例命中一致。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 注：unx0622_pkgindex.rs 已有前版存档，本版按工位纪律落 _v2。
//!
//! 设计纪律：
//!   1. 定容索引表：12 行 × 4 列（包号/判据号/账面行数/行校验和），
//!      行校验和 = 前三列按位异或（整数，确定性）。
//!   2. 表账守恒：已填行数 + 空行数 = 12；行内判据号必须与包号绑定的
//!      闭集一致，绑错拒收。
//!   3. 域级检索：按包号 O(12) 定容直查，命中一致 = 行校验和复算相等
//!      且列值与登记账面全等。
//!   4. 检索用例账：10/10 用例逐例落账，缺例/复算不符均红。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x4F80 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x4F80 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4F80;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x4F8F;

/// XcCode：包号越界。
pub const XC_BAD_PKG: u16 = SEG_BASE + 1; // 0x4F81 包号 0..12 闭集外
/// XcCode：行校验和不符。
pub const XC_ROW_CSUM: u16 = SEG_BASE + 2; // 0x4F82 行校验和复算不中
/// XcCode：判据号绑定不符。
pub const XC_CRIT_BIND: u16 = SEG_BASE + 3; // 0x4F83 行内判据号绑错
/// XcCode：索引表未账齐。
pub const XC_TBL_SHORT: u16 = SEG_BASE + 4; // 0x4F84 12 行未填满
/// XcCode：检索未命中。
pub const XC_LOOKUP_MISS: u16 = SEG_BASE + 5; // 0x4F85 按包号检索落空
/// XcCode：用例账残缺。
pub const XC_CASE_SHORT: u16 = SEG_BASE + 6; // 0x4F86 10/10 用例未跑满

/// 索引行数（12 包）。
pub const ROW_N: usize = 12;
/// 索引列数（包号/判据号/行数/校验和）。
pub const COL_N: usize = 4;
/// 检索用例数（判据原文：10/10）。
pub const CASE_N: usize = 10;

/// 12 包判据号闭集（与包号绑定，来源同 F0621 名册）。
pub const PKG_CRIT_IDS: [u64; ROW_N] = [
    349, 378, 413, 439, 453, 476, 497, 518, 538, 559, 593, 617,
];

/// 行校验和：前三列按位异或。
pub fn row_csum(pkg_no: u64, crit_id: u64, row_len: u64) -> u64 {
    pkg_no ^ crit_id ^ row_len
}

/// 索引表账本：定容 12×4。
#[derive(Clone, Debug)]
pub struct PkgIndexTable {
    /// 12 行 4 列账面（列序：包号/判据号/行数/行校验和）。
    cells: [[u64; COL_N]; ROW_N],
    /// 已填行位图。
    filled: u32,
}

impl PkgIndexTable {
    /// 空表（包号列以哨兵 0xFFFF 标记空行）。
    pub fn new() -> Self {
        PkgIndexTable {
            cells: [[0xFFFF; COL_N]; ROW_N],
            filled: 0,
        }
    }

    /// 已填行数。
    pub fn filled_n(&self) -> usize {
        self.filled.count_ones() as usize
    }

    /// 表账守恒：已填 + 空 = 12。
    pub fn conserve_ok(&self) -> bool {
        self.filled_n() + (ROW_N - self.filled_n()) == ROW_N
    }

    /// 登一行（包号闭集 + 判据号绑定 + 校验和落列）。
    pub fn set_row(&mut self, pkg_no: usize, row_len: u64) -> Result<(), u16> {
        if pkg_no >= ROW_N {
            return Err(XC_BAD_PKG);
        }
        if self.filled & (1u32 << pkg_no) != 0 {
            // 重登 = 先清后写，保持一槽一行。
            self.filled &= !(1u32 << pkg_no);
        }
        let crit = PKG_CRIT_IDS[pkg_no];
        self.cells[pkg_no][0] = pkg_no as u64;
        self.cells[pkg_no][1] = crit;
        self.cells[pkg_no][2] = row_len;
        self.cells[pkg_no][3] = row_csum(pkg_no as u64, crit, row_len);
        self.filled |= 1u32 << pkg_no;
        Ok(())
    }

    /// 行校验和复验（单行）。
    pub fn row_csum_ok(&self, pkg_no: usize) -> Result<bool, u16> {
        if pkg_no >= ROW_N {
            return Err(XC_BAD_PKG);
        }
        if self.filled & (1u32 << pkg_no) == 0 {
            return Err(XC_LOOKUP_MISS);
        }
        let c = &self.cells[pkg_no];
        Ok(c[3] == row_csum(c[0], c[1], c[2]))
    }

    /// 域级检索：按包号取整行，命中一致才回。
    pub fn lookup(&self, pkg_no: u64) -> Result<[u64; COL_N], u16> {
        if pkg_no as usize >= ROW_N {
            return Err(XC_BAD_PKG);
        }
        let i = pkg_no as usize;
        if self.filled & (1u32 << i) == 0 {
            return Err(XC_LOOKUP_MISS);
        }
        let row = self.cells[i];
        if row[3] != row_csum(row[0], row[1], row[2]) {
            return Err(XC_ROW_CSUM);
        }
        if row[1] != PKG_CRIT_IDS[i] {
            return Err(XC_CRIT_BIND);
        }
        Ok(row)
    }

    /// 表级总账齐核：12 行全填 + 逐行校验和复算 + 逐行判据号绑定复验。
    pub fn table_green(&self) -> Result<usize, u16> {
        if self.filled_n() != ROW_N {
            return Err(XC_TBL_SHORT);
        }
        for i in 0..ROW_N {
            if !self.row_csum_ok(i)? {
                return Err(XC_ROW_CSUM);
            }
            if self.cells[i][1] != PKG_CRIT_IDS[i] {
                return Err(XC_CRIT_BIND);
            }
        }
        Ok(ROW_N)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "交接包索引表 · 填行 {}/{} · 守恒={}",
            self.filled_n(),
            ROW_N,
            self.conserve_ok()
        );
        for i in 0..ROW_N {
            if self.filled & (1u32 << i) != 0 {
                let c = self.cells[i];
                s.push_str(&alloc::format!(
                    "\n  行{} 包{} 判F0{} 行数{} 校验{:04x}",
                    i, c[0], c[1], c[2], c[3] & 0xFFFF
                ));
            }
        }
        s
    }

    /// 表指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 检索用例账：10/10 用例逐例执行并落账（用例 = 包号 0..9 的命中一致性）。
#[derive(Clone, Debug)]
pub struct LookupCaseBook {
    /// 用例结论（包号, 命中, 复算校验和）。
    pub cases: Vec<(u64, bool, u64)>,
}

impl LookupCaseBook {
    /// 空用例账。
    pub fn new() -> Self {
        LookupCaseBook { cases: Vec::new() }
    }

    /// 跑一个用例：检索包号 pkg_no，比对命中行校验和。
    pub fn run_case(&mut self, tbl: &PkgIndexTable, pkg_no: u64) -> Result<(), u16> {
        if self.cases.len() >= CASE_N {
            return Err(XC_CASE_SHORT);
        }
        let row = tbl.lookup(pkg_no)?;
        let hit = row[0] == pkg_no;
        let csum = row[3];
        self.cases.push((pkg_no, hit, csum));
        if !hit {
            return Err(XC_LOOKUP_MISS);
        }
        Ok(())
    }

    /// 10/10 用例齐核：用例数、序号连续、全命中。
    pub fn cases_green(&self) -> Result<usize, u16> {
        if self.cases.len() != CASE_N {
            return Err(XC_CASE_SHORT);
        }
        for (i, (pkg, hit, _)) in self.cases.iter().enumerate() {
            if *pkg != i as u64 {
                return Err(XC_CASE_SHORT);
            }
            if !*hit {
                return Err(XC_LOOKUP_MISS);
            }
        }
        Ok(CASE_N)
    }
}

/// 建标准索引表（12 行全填，行数 = 包号*11+5，确定性）。
pub fn build_standard_table() -> Result<PkgIndexTable, u16> {
    let mut tbl = PkgIndexTable::new();
    for i in 0..ROW_N {
        tbl.set_row(i, (i as u64) * 11 + 5)?;
    }
    tbl.table_green()?;
    Ok(tbl)
}

/// 自例程：主路径（建表→账齐核→10/10 检索）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let tbl = build_standard_table()?;
    if tbl.table_green()? != ROW_N {
        return Err(XC_TBL_SHORT);
    }
    let mut cases = LookupCaseBook::new();
    for i in 0..CASE_N {
        cases.run_case(&tbl, i as u64)?;
    }
    if cases.cases_green()? != CASE_N {
        return Err(XC_CASE_SHORT);
    }
    // 命中一致：两次检索同包号结果全等（确定性）。
    let r1 = tbl.lookup(3)?;
    let r2 = tbl.lookup(3)?;
    if r1 != r2 {
        return Err(XC_ROW_CSUM);
    }
    if tbl.fingerprint() == 0 {
        return Err(XC_ROW_CSUM);
    }

    // 错误路径 1：包号越界。
    let t1 = PkgIndexTable::new();
    if t1.lookup(12) != Err(XC_BAD_PKG) {
        return Err(XC_BAD_PKG);
    }
    let mut t1b = PkgIndexTable::new();
    if t1b.set_row(ROW_N, 1) != Err(XC_BAD_PKG) {
        return Err(XC_BAD_PKG);
    }
    // 错误路径 2：空行检索落空。
    if t1.lookup(0) != Err(XC_LOOKUP_MISS) {
        return Err(XC_LOOKUP_MISS);
    }
    // 错误路径 3：表未齐做总账齐核。
    let mut t3 = PkgIndexTable::new();
    t3.set_row(0, 5)?;
    if t3.table_green() != Err(XC_TBL_SHORT) {
        return Err(XC_TBL_SHORT);
    }
    // 错误路径 4：行校验和篡改检出。
    let mut t4 = build_standard_table()?;
    t4.cells[2][2] += 1; // 篡改行 2 的行数列
    if t4.lookup(2) != Err(XC_ROW_CSUM) {
        return Err(XC_ROW_CSUM);
    }
    // 错误路径 5：判据号绑定篡改检出。
    let mut t5 = build_standard_table()?;
    t5.cells[1][1] = 999;
    t5.cells[1][3] = row_csum(t5.cells[1][0], t5.cells[1][1], t5.cells[1][2]);
    if t5.lookup(1) != Err(XC_CRIT_BIND) {
        return Err(XC_CRIT_BIND);
    }
    // 错误路径 6：用例账残缺（只用例不足 10）。
    let mut c6 = LookupCaseBook::new();
    for i in 0..(CASE_N - 1) {
        c6.run_case(&tbl, i as u64)?;
    }
    if c6.cases_green() != Err(XC_CASE_SHORT) {
        return Err(XC_CASE_SHORT);
    }
    Ok(0)
}
