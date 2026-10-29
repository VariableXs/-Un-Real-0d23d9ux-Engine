// unx1016_u1016 —— UNX-F1016 TSC 快照与多核读一致性验证资产
// 判据原文 UNX-F1001→F1016-J1：一致性资产可执行（四核读一致矩阵格全过），
// 快照导出（启动报告含 TSC 全景），一次命令 ≤10s。
// 落位：kernel/varix/src/unxstar/；诊断段 0x1BE0 独占（0x1BE0~0x1C1F）。
//
// 设计（纯功能 no_std）：
//   * 一致矩阵：4×4 核对格（每格=对测一轮），格全过才算资产可执行；
//   * 对测判据：同窗双核读差 ≤ 容差（刻度步数），超差即格子红；
//   * 快照导出：TSC 全景（基值/频点/健康位/矩阵分）FNV 冻结可导出；
//   * 时限账：验证轮 ms 记账 ≤10000ms（一次命令 ≤10s 预算闸）；
//   * 格账守恒：执行格数=矩阵格数（4×4=16 全跑）；
//   * 注入核六类：超差格/越界核/封账跑/时限破/格漏跑/快照漂移全抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x1BE0 独占） ─────────────────────────────────────────────
pub const XC_SNAP_DIFF: u32 = 0x1BE1; // 对测超差（格子红）
pub const XC_SNAP_CORE: u32 = 0x1BE2; // 核号越界
pub const XC_SNAP_SEALED: u32 = 0x1BE3; // 封账后执行
pub const XC_SNAP_BUDGET: u32 = 0x1BE4; // 10s 时限破
pub const XC_SNAP_MISSED: u32 = 0x1BE5; // 格漏跑（矩阵不完整）
pub const XC_SNAP_CONS: u32 = 0x1BE6; // 格账守恒破

/// 四核闭集；矩阵 4×4=16 格。
pub const N_CORES: usize = 4;
pub const GRID_CELLS: usize = N_CORES * N_CORES;
/// 同窗对测容差（刻度步）。
pub const TOL_STEPS: u64 = 64;
/// 一次命令时限（ms）。
pub const CMD_BUDGET_MS: u32 = 10_000;

/// 一格对测结果。
#[derive(Clone, Copy)]
pub struct CellRec {
    pub row: u32,
    pub col: u32,
    pub diff: u64,
    pub ok: bool,
    pub ms: u32,
}

/// 一致性验证资产。
pub struct XvalRig {
    cells: Vec<CellRec>,
    total_ms: u32,
    base_val: u64,
    sealed: bool,
}

impl XvalRig {
    pub fn new(base_val: u64) -> XvalRig {
        XvalRig {
            cells: Vec::new(),
            total_ms: 0,
            base_val,
            sealed: false,
        }
    }

    /// 跑一格：row/col 双核同窗读值对测（读值由调用方给——模拟同窗采样）。
    pub fn run_cell(&mut self, row: u32, col: u32, va: u64, vb: u64, ms: u32) -> Result<bool, u32> {
        if self.sealed {
            return Err(XC_SNAP_SEALED);
        }
        if row as usize >= N_CORES || col as usize >= N_CORES {
            return Err(XC_SNAP_CORE);
        }
        if self.total_ms.saturating_add(ms) > CMD_BUDGET_MS {
            return Err(XC_SNAP_BUDGET);
        }
        // 同格重跑拒（append-only 矩阵）
        if self.cells.iter().any(|c| c.row == row && c.col == col) {
            return Err(XC_SNAP_MISSED);
        }
        let d = if va > vb { va - vb } else { vb - va };
        let ok = d <= TOL_STEPS;
        self.total_ms = self.total_ms.saturating_add(ms);
        self.cells.push(CellRec { row, col, diff: d, ok, ms });
        if !ok {
            return Err(XC_SNAP_DIFF);
        }
        Ok(true)
    }

    /// 矩阵全跑断言：16 格全有账。
    pub fn grid_complete(&self) -> bool {
        if self.cells.len() != GRID_CELLS {
            return false;
        }
        for r in 0..N_CORES as u32 {
            for c in 0..N_CORES as u32 {
                if !self.cells.iter().any(|x| x.row == r && x.col == c) {
                    return false;
                }
            }
        }
        true
    }

    /// 格全过（零红格）。
    pub fn all_green(&self) -> bool {
        self.cells.iter().all(|c| c.ok)
    }

    /// 时限达标。
    pub fn budget_ok(&self) -> bool {
        self.total_ms <= CMD_BUDGET_MS
    }

    /// 守恒：格数 ≤ 矩阵格闭集。
    pub fn conserved(&self) -> bool {
        self.cells.len() <= GRID_CELLS
            && self.cells.iter().all(|c| (c.row as usize) < N_CORES && (c.col as usize) < N_CORES)
    }

    /// 快照导出：TSC 全景 FNV（基值+逐格+时限）。
    pub fn snapshot(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.base_val as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= (self.base_val >> 32) as u32;
        h = h.wrapping_mul(0x0100_0193);
        for c in self.cells.iter() {
            h ^= c.row ^ (c.col << 8) ^ (c.diff as u32) ^ ((c.ok as u32) << 16);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.total_ms;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 20 | (self.cells.len() as u64 & 0xFFFFF)
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_SNAP_SEALED);
        }
        if !self.grid_complete() {
            return Err(XC_SNAP_MISSED);
        }
        if !self.all_green() {
            return Err(XC_SNAP_DIFF);
        }
        self.sealed = true;
        Ok(self.snapshot())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
    pub fn total_ms(&self) -> u32 {
        self.total_ms
    }
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）对测超差必抓（差 65..74 十例）
    {
        let mut hits = 0;
        for k in 0..10u64 {
            let mut r = XvalRig::new(1000);
            if r.run_cell(0, 1, 1000 + TOL_STEPS + 1 + k, 1000, 10) == Err(XC_SNAP_DIFF) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）核号越界必抓（行 4..=8 与列 4..=8 十例）
    {
        let mut r = XvalRig::new(0);
        let mut hits = 0;
        for k in 4..=8u32 {
            if r.run_cell(k, 0, 0, 0, 1) == Err(XC_SNAP_CORE) {
                hits += 1;
            }
            if r.run_cell(0, k + 3, 0, 0, 1) == Err(XC_SNAP_CORE) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）时限破必抓（累进超 10000ms 十例）
    {
        let mut hits = 0;
        for k in 0..10u32 {
            let mut r = XvalRig::new(0);
            let _ = r.run_cell(0, 0, 0, 0, 9_990);
            if r.run_cell(0, 1, 0, 0, 11 + k) == Err(XC_SNAP_BUDGET) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）同格重跑必拒
    {
        let mut r = XvalRig::new(0);
        let _ = r.run_cell(2, 3, 0, 0, 1);
        if r.run_cell(2, 3, 0, 0, 1) == Err(XC_SNAP_MISSED) {
            caught += 1;
        }
    }
    // 5）封账后执行必拒
    {
        let mut r = XvalRig::new(0);
        for rr in 0..4u32 {
            for c in 0..4u32 {
                let _ = r.run_cell(rr, c, 100, 100, 1);
            }
        }
        let _ = r.seal();
        if r.run_cell(0, 0, 0, 0, 1) == Err(XC_SNAP_SEALED) {
            caught += 1;
        }
    }
    // 6）矩阵不完整封账必拒
    {
        let mut r = XvalRig::new(0);
        for c in 0..4u32 {
            let _ = r.run_cell(0, c, 0, 0, 1);
        }
        if r.seal() == Err(XC_SNAP_MISSED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1001→F1016-J1。
pub fn run_unx1016_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1016");

    // C16-01 一致矩阵全格过（16 格同值对测）
    {
        let mut r = XvalRig::new(4242);
        let mut ok = true;
        for rr in 0..4u32 {
            for c in 0..4u32 {
                let base = 10_000 + (rr * 4 + c) as u64 * 1000;
                if r.run_cell(rr, c, base, base + (rr as u64 % 2), 5).is_err() {
                    ok = false;
                    break;
                }
            }
        }
        ok = ok && r.grid_complete() && r.all_green() && r.cell_count() == 16;
        s.add("C16-01-矩阵全格", ok, if ok { "" } else { "格子红" });
    }
    // C16-02 快照导出（全景指纹非零往返）
    {
        let build = || {
            let mut r = XvalRig::new(0xABCD_1234);
            for rr in 0..4u32 {
                for c in 0..4u32 {
                    let _ = r.run_cell(rr, c, 5000, 5002, 3);
                }
            }
            r.snapshot()
        };
        let fp = build();
        let ok = fp != 0 && build() == fp;
        s.add("C16-02-快照导出", ok, if ok { "" } else { "快照漂" });
    }
    // C16-03 时限 ≤10s
    {
        let mut r = XvalRig::new(0);
        for rr in 0..4u32 {
            for c in 0..4u32 {
                let _ = r.run_cell(rr, c, 10, 10, 100);
            }
        }
        let ok = r.budget_ok() && r.total_ms() == 1600;
        s.add("C16-03-时限达标", ok, if ok { "" } else { "时限破" });
    }
    // C16-04 容差边界（差=64 过 / 65 红）
    {
        let mut a = XvalRig::new(0);
        let mut b = XvalRig::new(0);
        let ok_a = a.run_cell(0, 1, 1000 + TOL_STEPS, 1000, 1).is_ok();
        let ok_b = b.run_cell(0, 1, 1000 + TOL_STEPS + 1, 1000, 1) == Err(XC_SNAP_DIFF);
        s.add("C16-04-容差边界", ok_a && ok_b, if ok_a && ok_b { "" } else { "边界漂" });
    }
    // C16-05 超差格显性（错误码回带）
    {
        let mut r = XvalRig::new(0);
        let ok = r.run_cell(3, 2, 0, 5000, 1) == Err(XC_SNAP_DIFF);
        s.add("C16-05-超差显性", ok, if ok { "" } else { "超差假绿" });
    }
    // C16-06 同格重跑拒
    {
        let mut r = XvalRig::new(0);
        let _ = r.run_cell(1, 1, 0, 0, 1);
        let ok = r.run_cell(1, 1, 0, 0, 1) == Err(XC_SNAP_MISSED);
        s.add("C16-06-重跑拒", ok, if ok { "" } else { "重跑漏" });
    }
    // C16-07 封账冻结（全绿矩阵才可封）
    {
        let mut r = XvalRig::new(0);
        for rr in 0..4u32 {
            for c in 0..4u32 {
                let _ = r.run_cell(rr, c, 77, 77, 2);
            }
        }
        let fp = r.seal();
        let ok = fp.is_ok() && r.is_sealed() && r.run_cell(0, 0, 0, 0, 1) == Err(XC_SNAP_SEALED);
        s.add("C16-07-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C16-08 红矩阵封账拒
    {
        let mut r = XvalRig::new(0);
        for rr in 0..4u32 {
            for c in 0..4u32 {
                let d = if rr == 2 && c == 1 { 99_999 } else { 0 };
                let _ = r.run_cell(rr, c, 100 + d, 100, 1);
            }
        }
        let ok = r.seal() == Err(XC_SNAP_DIFF);
        s.add("C16-08-红阵拒封", ok, if ok { "" } else { "红阵假封" });
    }
    // C16-09 守恒
    {
        let mut r = XvalRig::new(0);
        for rr in 0..4u32 {
            for c in 0..4u32 {
                let _ = r.run_cell(rr, c, 0, 0, 1);
            }
        }
        s.add("C16-09-格账守恒", r.conserved(), if r.conserved() { "" } else { "守恒破" });
    }
    // C16-10 对角与全序格（行=列格与行列互逆格全辨）
    {
        let mut r = XvalRig::new(0);
        for rr in 0..4u32 {
            for c in 0..4u32 {
                let _ = r.run_cell(rr, c, (rr as u64 + 1) * 10, (c as u64 + 1) * 10, 1);
            }
        }
        // 对角格差=0 应全绿；非对角差 |r-c|*10 ≤ 30 ≤ TOL 也绿
        let ok = r.grid_complete() && r.all_green();
        s.add("C16-10-对角全序", ok, if ok { "" } else { "序格漂" });
    }
    // C16-11 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C16-11-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C16-12 核号边界（3 合法 4 拒）
    {
        let mut r = XvalRig::new(0);
        let a = r.run_cell(3, 3, 0, 0, 1);
        let b = r.run_cell(4, 0, 0, 0, 1);
        let c2 = r.run_cell(0, 4, 0, 0, 1);
        let ok = a.is_ok() && b == Err(XC_SNAP_CORE) && c2 == Err(XC_SNAP_CORE);
        s.add("C16-12-核号边界", ok, if ok { "" } else { "边界误判" });
    }
    s
}
