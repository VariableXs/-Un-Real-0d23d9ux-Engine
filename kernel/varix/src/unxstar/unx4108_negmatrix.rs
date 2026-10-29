//! unx4108_negmatrix — UNX-F4108 负缓存的创建条件矩阵（诊断段 0x8F80）。
//!
//! 判据原文（提炼）：
//!   J1 创建条件矩阵 6 格全过：仅 ENOENT 入负账；ENOTDIR/EACCES/ELOOP/
//!      ENAMETOOLONG/EIO 五格一律不入账（负缓存只承载"确认不存在"）。
//!   J2 错把错误码入账注入被检出：非 ENOENT 错误入负账时检出器报警
//!      并拒绝入账（E_CELL_DENY）。
//!   J3 矩阵表驱动：6 格为 const 表（errno → 准入判定），
//!      逐格执行计数与准/拒账独立可查。
//!   J4 ENOENT 格守恒：负账计数 == ENOENT 格入账数（其他格零贡献）。
//!   J5 未知错误码拒收（E_ERRNO_UNKNOWN），不入任何格。
//! 设计纪律：
//!   1. errno 取值写死（Linux uapi/asm-generic/errno.h）：
//!      ENOENT=2、EACCES=13、EIO=5、ENOTDIR=20、ENAMETOOLONG=36、ELOOP=40。
//!   2. 单一准入判定面：on_fs_error 是唯一入口，矩阵表查得 allow
//!      才入负账，deny 一律计拒账。
//!   3. 注入面独立：force_negative 绕表强入被视为违规注入，
//!      检出器核对"负账来源必须为 ENOENT 格"（判据 J2）。
//!   4. 格账分离：每格 hits/denies 两本账，矩阵汇总账可复算。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x8F80 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8F80;

// ── Linux errno 常量（uapi/asm-generic/errno.h 与 errno-base.h）──
pub const ENOENT: u16 = 2;        // 不存在——唯一准入门
pub const EACCES: u16 = 13;       // 权限不足——不入账
pub const EIO: u16 = 5;           // I/O 错——不入账
pub const ENOTDIR: u16 = 20;      // 路径分量非目录——不入账
pub const ENAMETOOLONG: u16 = 36; // 名超长——不入账
pub const ELOOP: u16 = 40;        // 符号链环——不入账

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_CELL_DENY: u16 = SEG_BASE + 1;  // 非准入门错误码请求入负账（判据 J2）
const E_ERRNO_UNKNOWN: u16 = SEG_BASE + 2; // 未知错误码
const E_FORCE_ILLEGAL: u16 = SEG_BASE + 3; // 绕表强入注入被检出
const E_BAL: u16 = SEG_BASE + 4;        // 负账来源守恒失守（判据 J4）
const E_MATRIX_BAD: u16 = SEG_BASE + 5; // 矩阵表损坏（格数/判定失真）
const E_DUP_NAME: u16 = SEG_BASE + 6;   // 同名负条目重复入账

/// 矩阵格：errno + 准入判定（表驱动，判据 J1/J3）。
pub struct MatrixCell {
    pub errno: u16,  // 错误码
    pub allow: bool, // true = 准入负账（仅 ENOENT）
}

/// 6 格 const 矩阵（判据 J1 原文 6 格全列）。
pub const MATRIX: [MatrixCell; 6] = [
    MatrixCell { errno: ENOENT, allow: true },        // 不存在：入账
    MatrixCell { errno: ENOTDIR, allow: false },      // 非目录：不入
    MatrixCell { errno: EACCES, allow: false },       // 权限错：不入
    MatrixCell { errno: ELOOP, allow: false },        // 链环：不入
    MatrixCell { errno: ENAMETOOLONG, allow: false }, // 超长名：不入
    MatrixCell { errno: EIO, allow: false },          // I/O 错：不入
];

/// 单格账：命中数 + 拒绝数。
pub struct CellLedger {
    pub errno: u16, // 对应错误码
    pub hits: u32,  // 送达次数
    pub allows: u32,// 准入次数
    pub denies: u32,// 拒绝次数
}

/// 负条目：名 + 来源错误码（来源必须是 ENOENT，判据 J2/J4 依据）。
pub struct NegItem {
    pub name: &'static str, // 路径名
    pub src_errno: u16,     // 入账来源错误码（恒 ENOENT）
}

/// 负缓存条件矩阵枢纽：矩阵表 + 格账 + 负账 + 注入检出器。
pub struct NegMatrix {
    pub cells: [CellLedger; 6], // 6 格账（与 MATRIX 一一对应）
    pub negatives: Vec<NegItem>,// 负账（判据 J4 对账面）
    pub forced_rejects: u32,    // 绕表强入检出数（判据 J2）
    pub unknown_rejects: u32,   // 未知错误码拒收数（判据 J5）
}

impl NegMatrix {
    pub fn new() -> NegMatrix {
        NegMatrix {
            cells: [
                CellLedger { errno: ENOENT, hits: 0, allows: 0, denies: 0 },
                CellLedger { errno: ENOTDIR, hits: 0, allows: 0, denies: 0 },
                CellLedger { errno: EACCES, hits: 0, allows: 0, denies: 0 },
                CellLedger { errno: ELOOP, hits: 0, allows: 0, denies: 0 },
                CellLedger { errno: ENAMETOOLONG, hits: 0, allows: 0, denies: 0 },
                CellLedger { errno: EIO, hits: 0, allows: 0, denies: 0 },
            ],
            negatives: Vec::new(),
            forced_rejects: 0,
            unknown_rejects: 0,
        }
    }

    /// 矩阵表自检（判据 J1 前置）：6 格、逐格判定与 const 表一致。
    fn matrix_sane(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            if self.cells[i].errno != MATRIX[i].errno {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 按错误码定位格下标（未知码返回 None）。
    fn cell_idx(errno: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < 6 {
            if MATRIX[i].errno == errno {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 单一准入判定入口（判据 J1/J3/J5）：
    /// fs 返回 errno 时决定是否入负账。返回 Ok(true) 表示已入账。
    pub fn on_fs_error(&mut self, name: &'static str, errno: u16) -> Result<bool, u16> {
        let ci = match NegMatrix::cell_idx(errno) {
            Some(ci) => ci,
            None => {
                self.unknown_rejects += 1;
                return Err(E_ERRNO_UNKNOWN); // 未知码不入任何格
            }
        };
        self.cells[ci].hits += 1;
        if !MATRIX[ci].allow {
            self.cells[ci].denies += 1;
            return Ok(false); // 非准入门：不入账，计拒
        }
        // 准入门（ENOENT）：同名防重复
        for n in &self.negatives {
            if n.name == name {
                return Err(E_DUP_NAME);
            }
        }
        self.cells[ci].allows += 1;
        self.negatives.push(NegItem { name, src_errno: errno });
        Ok(true)
    }

    /// 绕表强入注入面（判据 J2）：任非 ENOENT 错误码强入负账
    /// 即被检出器拒绝并计违规账。
    pub fn force_negative(&mut self, name: &'static str, src_errno: u16) -> Result<(), u16> {
        if src_errno != ENOENT {
            self.forced_rejects += 1;
            return Err(E_FORCE_ILLEGAL); // 错误码入账注入被检出
        }
        // 来源合法仍须走正常准入（同名防重复）
        for n in &self.negatives {
            if n.name == name {
                return Err(E_DUP_NAME);
            }
        }
        self.negatives.push(NegItem { name, src_errno });
        Ok(())
    }

    /// 负账来源守恒（判据 J4）：全部负条目来源 == ENOENT，
    /// 且 ENOENT 格准入数 == 负账条数。
    pub fn conserved(&self) -> bool {
        if !self.matrix_sane() {
            return false;
        }
        for n in &self.negatives {
            if n.src_errno != ENOENT {
                return false; // 来源混入即判据 J2 失守
            }
        }
        self.cells[0].allows == self.negatives.len() as u32
    }

    /// 矩阵汇总复算（判据 J3）：逐格 hits == allows + denies。
    pub fn matrix_recheck(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            if self.cells[i].hits != self.cells[i].allows + self.cells[i].denies {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 负账条数快照。
    pub fn negative_count(&self) -> u32 {
        self.negatives.len() as u32
    }

    /// 按错误码读格账（审计面）：未知码拒收（E_ERRNO_UNKNOWN）。
    /// 返回（送达数，准入数，拒绝数）三元组。
    pub fn cell_stats(&self, errno: u16) -> Result<(u32, u32, u32), u16> {
        let ci = match NegMatrix::cell_idx(errno) {
            Some(ci) => ci,
            None => return Err(E_ERRNO_UNKNOWN),
        };
        Ok((self.cells[ci].hits, self.cells[ci].allows, self.cells[ci].denies))
    }

    /// 负账名清单导出（入账序）。
    pub fn neg_names(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for i in 0..self.negatives.len() {
            v.push(self.negatives[i].name);
        }
        v
    }

    /// 全格送达汇总（判据 J3 汇总面）：Σhits 应等于实验送达总数。
    pub fn total_delivered(&self) -> u32 {
        let mut s = 0u32;
        for i in 0..6 {
            s += self.cells[i].hits;
        }
        s
    }

    /// 全格拒绝汇总（拒账面）：五格 denies 之和 + 未知码 + 强入检出。
    pub fn total_denied(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < 6 {
            s += self.cells[i].denies;
            i += 1;
        }
        s + self.unknown_rejects + self.forced_rejects
    }
}

/// 自例程：R1 六格全跑→R2 ENOENT 入账守恒→R3 注入检出→R4 矩阵复算。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 六格全过（判据 J1）：逐格送达一次，验准/拒判定。
    let mut m = NegMatrix::new();
    if m.on_fs_error("p1", ENOENT)? != true {
        return Err(E_CELL_DENY); // ENOENT 必须准入
    }
    let denies: [u16; 5] = [ENOTDIR, EACCES, ELOOP, ENAMETOOLONG, EIO];
    let mut i = 0usize;
    while i < 5 {
        if m.on_fs_error("px", denies[i])? != false {
            return Err(E_CELL_DENY); // 五格必须全拒
        }
        i += 1;
    }
    hits += 1;

    // R2 守恒（判据 J4）：负账 1 条，来源全 ENOENT，格账平。
    if m.negative_count() != 1 || !m.conserved() {
        return Err(E_BAL);
    }
    if m.cells[0].allows != 1 || m.cells[0].denies != 0 {
        return Err(E_BAL);
    }
    let mut deny_sum = 0u32;
    let mut k = 1usize;
    while k < 6 {
        deny_sum += m.cells[k].denies;
        if m.cells[k].allows != 0 {
            return Err(E_BAL); // 非准入门不得有准入
        }
        k += 1;
    }
    if deny_sum != 5 {
        return Err(E_BAL); // 五格各拒一次
    }
    hits += 1;

    // R3 注入检出（判据 J2/J5）：错码强入、未知码、重复入账全拦。
    if m.force_negative("p2", EACCES).is_ok() {
        return Err(E_FORCE_ILLEGAL); // 错误码入账注入必须被检出
    }
    if m.on_fs_error("p3", 4096).is_ok() {
        return Err(E_ERRNO_UNKNOWN); // 未知码应拒
    }
    if m.on_fs_error("p1", ENOENT).is_ok() {
        return Err(E_DUP_NAME); // 同名重复入账应拒
    }
    if m.forced_rejects != 1 || m.unknown_rejects != 1 {
        return Err(E_BAL); // 注入账如实记
    }
    hits += 1;

    // R4 矩阵复算（判据 J3）与终审：多轮送达后 hits==allows+denies。
    let _ = m.on_fs_error("p4", ENOENT)?; // 第二次准入
    let _ = m.on_fs_error("p5", ENOTDIR)?; // 再拒一次
    if !m.matrix_recheck() {
        return Err(E_MATRIX_BAD);
    }
    if !m.conserved() || m.negative_count() != 2 {
        return Err(E_BAL);
    }
    // ENOENT 格最终账：2 次送达 2 准入 0 拒。
    if m.cells[0].hits != 2 || m.cells[0].allows != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 汇总审计面：格账读取、负账名导出、全格汇总复算。
    let (h, a, d) = m.cell_stats(ENOENT)?;
    if h != 2 || a != 2 || d != 0 {
        return Err(E_BAL);
    }
    let (h2, a2, d2) = m.cell_stats(ENOTDIR)?;
    if h2 != 2 || a2 != 0 || d2 != 2 {
        return Err(E_BAL); // ENOTDIR 两次送达两次拒
    }
    if m.cell_stats(4096).is_ok() {
        return Err(E_ERRNO_UNKNOWN); // 未知码格账读取应拒
    }
    let names = m.neg_names();
    if names.len() != 2 || names[0] != "p1" || names[1] != "p4" {
        return Err(E_BAL); // 导出序 == 入账序
    }
    if m.total_delivered() != 8 {
        return Err(E_BAL); // 2(ENOENT) + 5(R1) + 1(R4 ENOTDIR) = 8
    }
    if m.total_denied() < 7 {
        return Err(E_BAL); // 五格 6 拒 + 未知 1 + 强入 1
    }
    hits += 1;

    Ok(hits)
}
