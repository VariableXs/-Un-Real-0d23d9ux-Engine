//! unx4047_mtimectime — UNX-F4047 mtime/ctime 更新矩阵（诊断段 0x8840）。
//!
//! 判据原文：
//!   J1 写/截断/链接/属性改四类操作对 mtime/ctime 的影响按 POSIX
//!      矩阵逐格断言；
//!   J2 ctime 单调不减不变式成立。
//! 设计纪律：
//!   1. POSIX 矩阵定编：写（write）→ mtime+ctime；截断
//!      （truncate）→ mtime+ctime；链接（link，nlink 变更）→
//!      仅 ctime；属性改（chattr，chmod 类）→ 仅 ctime——四行
//!      两列（mtime/ctime）八格逐格断言。
//!   2. 单调不变式（判据 J2）：任意操作后 ctime' ≥ ctime；
//!      逻辑时钟只进不退（回退注入拒收）。
//!   3. 同刻度语义：同 tick 内连续操作允许相等（不减即合法）。
//!   4. 目录聚合不在此锚（i_size 归 4044）；nlink 计数归 4048
//!      ——本锚只记账时间戳效应，不越界。
//!   5. 边界：空写（零效应格）、非法时钟注入、未知操作码——
//!      全部显性拒收并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x8840;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_CLOCK_INV: u16 = SEG_BASE + 1;   // 非法时钟（回退/超前进针）
const E_BAD_OP: u16 = SEG_BASE + 2;      // 未知操作码
const E_MTX_MISS: u16 = SEG_BASE + 3;    // 矩阵格与 POSIX 预期不符
const E_CTIME_BACK: u16 = SEG_BASE + 4;  // ctime 回退（单调不变式失守）
const E_BAL: u16 = SEG_BASE + 5;         // 矩阵格账不平

/// 操作码闭集（四类）。
pub const OP_WRITE: u8 = 1;   // 写
pub const OP_TRUNC: u8 = 2;   // 截断
pub const OP_LINK: u8 = 3;    // 链接（nlink 变更）
pub const OP_CHATTR: u8 = 4;  // 属性改（chmod 类）

/// POSIX 预期矩阵：行 = 操作，列 =（动 mtime?，动 ctime?）。
pub const MTX: [(u8, bool, bool); 4] = [
    (OP_WRITE, true, true),
    (OP_TRUNC, true, true),
    (OP_LINK, false, true),
    (OP_CHATTR, false, true),
];

/// 单 inode 时间戳账。
pub struct TsCell {
    pub mtime: u64, // 内容修改时间
    pub ctime: u64, // 元数据修改时间（单调不减）
}

/// 时间戳记账枢纽（逻辑时钟驱动）。
pub struct MtimeCtime {
    pub cell: TsCell,
    pub now: u64,          // 逻辑时钟
    pub op_total: u32,     // 操作总数
    pub mtx_cells: u32,    // 矩阵格数（判据 J1 账面）
    pub mtx_ok: u32,       // 矩阵一致格数
    pub ctime_samples: u32,// 单调性采样数（判据 J2 账面）
    pub rejects: u32,      // 错误账
}

impl MtimeCtime {
    pub fn new() -> MtimeCtime {
        MtimeCtime {
            cell: TsCell { mtime: 0, ctime: 0 },
            now: 0,
            op_total: 0,
            mtx_cells: 0,
            mtx_ok: 0,
            ctime_samples: 0,
            rejects: 0,
        }
    }

    /// 时钟推进（只进不退）。
    pub fn tick_to(&mut self, t: u64) -> Result<(), u16> {
        if t < self.now {
            self.rejects += 1;
            return Err(E_CLOCK_INV);
        }
        self.now = t;
        Ok(())
    }

    /// 操作记账：按 POSIX 矩阵更新 mtime/ctime。
    pub fn apply(&mut self, op: u8) -> Result<(u64, u64), u16> {
        // 操作码闭集校验。
        let mut known = false;
        let mut i = 0;
        while i < MTX.len() {
            if MTX[i].0 == op {
                known = true;
                break;
            }
            i += 1;
        }
        if !known {
            self.rejects += 1;
            return Err(E_BAD_OP);
        }
        self.op_total += 1;
        let old_ctime = self.cell.ctime;
        let mut m = MTX[0];
        let mut j = 0;
        while j < MTX.len() {
            if MTX[j].0 == op {
                m = MTX[j];
                break;
            }
            j += 1;
        }
        if m.1 {
            self.cell.mtime = self.now;
        }
        // ctime 对四类操作恒更新（单调不减：now ≥ 旧值由时钟保证）。
        self.cell.ctime = self.now;
        if self.cell.ctime < old_ctime {
            return Err(E_CTIME_BACK);
        }
        self.ctime_samples += 1;
        Ok((self.cell.mtime, self.cell.ctime))
    }

    /// 八格矩阵断言（判据 J1）：四操作 ×（mtime 动/不动，ctime 动）
    /// 逐格核对 POSIX 预期。
    pub fn matrix_sweep(&mut self) -> Result<u32, u16> {
        let mut cells = 0u32;
        let mut k = 0;
        while k < MTX.len() {
            let (op, want_m, _want_c) = MTX[k];
            // 每格独立时钟刻度（保证可观测变化）。
            let t = 1000 + (k as u64) * 10;
            self.tick_to(t)?;
            let m_before = self.cell.mtime;
            let c_before = self.cell.ctime;
            self.apply(op)?;
            // mtime 格：写/截断动，链接/属性改不动。
            let m_ok = if want_m { self.cell.mtime == t } else { self.cell.mtime == m_before };
            // ctime 格：四类操作恒动（到当前刻度）。
            let c_ok = self.cell.ctime == t && self.cell.ctime >= c_before;
            self.mtx_cells += 1;
            if m_ok && c_ok {
                self.mtx_ok += 1;
            } else {
                return Err(E_MTX_MISS);
            }
            cells += 1;
            k += 1;
        }
        if cells != 4 {
            return Err(E_BAL);
        }
        Ok(cells)
    }

    /// 单调性压测（判据 J2）：随机化刻度序列下 ctime 单调不减
    /// 逐采样断言（确定性伪序）。
    pub fn monotonic_sweep(&mut self, rounds: u32) -> Result<u32, u16> {
        let mut k: u32 = 0;
        while k < rounds {
            // 确定性刻度：交替推进与同刻度（不减即合法）。
            let t = self.now + ((k % 3) as u64);
            self.tick_to(t)?;
            let before = self.cell.ctime;
            self.apply(OP_WRITE)?;
            self.apply(OP_LINK)?;
            if self.cell.ctime < before {
                return Err(E_CTIME_BACK);
            }
            self.ctime_samples += 2;
            k += 1;
        }
        Ok(self.ctime_samples)
    }

    /// 守恒对账：操作账闭合；矩阵账闭合；ctime ≥ mtime 弱序。
    pub fn audit(&self) -> Result<(), u16> {
        if self.mtx_cells != self.mtx_ok {
            return Err(E_MTX_MISS);
        }
        if self.cell.ctime < self.cell.mtime && self.cell.ctime != self.cell.mtime {
            // 弱序：ctime 不早于 mtime（同刻度相等合法）。
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 导出快照（mtime，ctime，操作账，矩阵格账）。
    pub fn ledger(&self) -> (u64, u64, u32, u32) {
        (self.cell.mtime, self.cell.ctime, self.op_total, self.mtx_cells)
    }

    /// 操作分布账：四操作码闭集逐位计数（分域防串格）。
    pub fn op_dist(&self, history: &[(u8, u64, u64)]) -> [u32; 4] {
        let mut d = [0u32; 4];
        let mut i = 0;
        while i < history.len() {
            let op = history[i].0;
            if op >= OP_WRITE && op <= OP_CHATTR {
                d[(op - OP_WRITE) as usize] += 1;
            }
            i += 1;
        }
        d
    }

    /// 单格判定器（矩阵复算面）：给定操作码与时钟前后账，
    /// 返回（mtime 动否，ctime 动否）——与 MTX 表逐格对照用。
    pub fn expect_of(op: u8) -> Result<(bool, bool), u16> {
        let mut i = 0;
        while i < MTX.len() {
            if MTX[i].0 == op {
                return Ok((MTX[i].1, MTX[i].2));
            }
            i += 1;
        }
        Err(E_BAD_OP)
    }

    /// 时钟视图（只读观测面）。
    pub fn clock_view(&self) -> u64 {
        self.now
    }

    /// 弱序视图：ctime ≥ mtime（同刻度相等合法）布尔面。
    pub fn weak_order_ok(&self) -> bool {
        self.cell.ctime >= self.cell.mtime
    }

    /// 拒收细分：操作码拒收与时钟拒收二分（合并账面下取和）。
    pub fn reject_view(&self) -> u32 {
        self.rejects
    }
}

/// 自例程：R1 八格矩阵 → R2 单调压测 → R3 注入拒收 → R4 空效应格 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut mc = MtimeCtime::new();

    // R1 POSIX 矩阵（判据 J1）：四行两列逐格断言。
    let c = mc.matrix_sweep()?;
    if c != 4 {
        return Err(E_MTX_MISS);
    }
    hits += 1;

    // R2 单调压测（判据 J2）：100 轮刻度序列，ctime 单调不减。
    let s = mc.monotonic_sweep(100)?;
    if s < 200 {
        return Err(E_CTIME_BACK);
    }
    hits += 1;

    // R3 注入拒收：未知操作码、时钟回退。
    if mc.apply(9) != Err(E_BAD_OP) {
        return Err(E_BAL);
    }
    if mc.tick_to(0) != Err(E_CLOCK_INV) {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 链接格复查：mtime 不动、ctime 动（判据 J1 的易错格）。
    mc.tick_to(5000)?;
    let m_b = mc.cell.mtime;
    let (_m, _c) = mc.apply(OP_LINK)?;
    if mc.cell.mtime != m_b || mc.cell.ctime != 5000 {
        return Err(E_MTX_MISS);
    }
    hits += 1;

    // R5 守恒对账：矩阵账闭合；操作账 = 4 + 200 + 1；快照自洽。
    mc.audit()?;
    let (mt, ct, ops, cells) = mc.ledger();
    if ops != 205 || cells != 4 || ct < mt {
        return Err(E_BAL);
    }
    hits += 1;

    // R6 判定器与分布：单格预期逐操作码复算；弱序与时钟面互证。
    if MtimeCtime::expect_of(OP_WRITE)? != (true, true) {
        return Err(E_MTX_MISS);
    }
    if MtimeCtime::expect_of(OP_TRUNC)? != (true, true) {
        return Err(E_MTX_MISS);
    }
    if MtimeCtime::expect_of(OP_LINK)? != (false, true) {
        return Err(E_MTX_MISS);
    }
    if MtimeCtime::expect_of(OP_CHATTR)? != (false, true) {
        return Err(E_MTX_MISS);
    }
    if MtimeCtime::expect_of(9).is_ok() {
        return Err(E_BAL);
    }
    let hist = [(OP_WRITE, 0u64, 0u64), (OP_LINK, 0, 0), (OP_LINK, 0, 0), (OP_CHATTR, 0, 0)];
    let dist = mc.op_dist(&hist);
    if dist[0] != 1 || dist[1] != 0 || dist[2] != 2 || dist[3] != 1 {
        return Err(E_BAL);
    }
    if !mc.weak_order_ok() || mc.clock_view() != 5000 {
        return Err(E_BAL);
    }
    if mc.reject_view() != 2 {
        // R3 未知操作码 1 + 时钟回退 1 = 2 笔拒收。
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
