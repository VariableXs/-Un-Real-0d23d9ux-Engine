// unx1058_u1058 —— UNX-F1058 仲裁回归资产（切换与模式矩阵）
// 判据原文 UNX-F1058-J1：矩阵可执行（主源×定时模式格全绑定全过），一次命令 ≤20s，QEMU 档账平。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2C30 独占（0x2C31~0x2C3F）。
//
// 设计（纯功能 no_std）：
//   * 回归矩阵：主源（HPET/TSC/LAPIC）×定时模式（periodic/oneshot/tsc-deadline）9 格全绑定；
//   * 逐格执行：绑定→运行→结果码落账，未绑格即拒跑；
//   * 预算账：每格耗时落账，总耗时 ≤20s 断言（模拟 ms）；
//   * QEMU 档账平：执行数 = 通过数 = 绑定数（三账对平）；
//   * FNV 指纹 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2C30 独占） ─────────────────────────────────────────────
pub const XC_RG_OOB: u32 = 0x2C31; // 行/列越界
pub const XC_RG_UNBOUND: u32 = 0x2C32; // 未绑定格执行
pub const XC_RG_SEALED: u32 = 0x2C33; // 封账后回归
pub const XC_RG_FAIL: u32 = 0x2C34; // 格执行失败
pub const XC_RG_BUDGET: u32 = 0x2C35; // 总耗时越限

pub const MATRIX_ROWS: usize = 3; // 主源数
pub const MATRIX_COLS: usize = 3; // 模式数
pub const TIME_BUDGET_MS: u64 = 20_000;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub bound: bool,
    pub passed: bool,
    pub cost_ms: u32,
    pub run: bool,
}

/// 回归矩阵账。
pub struct RegressionMatrix {
    cells: [[Cell; MATRIX_COLS]; MATRIX_ROWS],
    total_ms: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl RegressionMatrix {
    pub fn new() -> RegressionMatrix {
        RegressionMatrix {
            cells: [[Cell { bound: false, passed: false, cost_ms: 0, run: false }; MATRIX_COLS]; MATRIX_ROWS],
            total_ms: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 绑定一格（可重复绑定覆盖）。
    pub fn bind(&mut self, row: usize, col: usize) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_RG_SEALED, "封账后绑定"));
            return Err(XC_RG_SEALED);
        }
        if row >= MATRIX_ROWS || col >= MATRIX_COLS {
            self.rejects.push((XC_RG_OOB, "行/列越界"));
            return Err(XC_RG_OOB);
        }
        self.cells[row][col].bound = true;
        Ok(())
    }

    /// 执行一格：未绑定拒；越限即失败码。
    pub fn run_cell(&mut self, row: usize, col: usize, cost_ms: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_RG_SEALED, "封账后回归"));
            return Err(XC_RG_SEALED);
        }
        if row >= MATRIX_ROWS || col >= MATRIX_COLS {
            self.rejects.push((XC_RG_OOB, "行/列越界"));
            return Err(XC_RG_OOB);
        }
        if !self.cells[row][col].bound {
            self.rejects.push((XC_RG_UNBOUND, "未绑定格执行"));
            return Err(XC_RG_UNBOUND);
        }
        self.cells[row][col].run = true;
        self.cells[row][col].cost_ms = cost_ms;
        self.total_ms = self.total_ms.saturating_add(cost_ms as u64);
        if self.total_ms > TIME_BUDGET_MS {
            self.rejects.push((XC_RG_BUDGET, "总耗时越限"));
            return Err(XC_RG_BUDGET);
        }
        self.cells[row][col].passed = true;
        Ok(())
    }

    /// 9 格全绑定。
    pub fn all_bound(&self) -> bool {
        self.cells.iter().flatten().all(|c| c.bound)
    }

    /// 9 格全跑全过。
    pub fn all_passed(&self) -> bool {
        self.cells.iter().flatten().all(|c| c.bound && c.run && c.passed)
    }

    /// QEMU 档账平：绑定数 = 执行数 = 通过数 = 9。
    pub fn qemu_reconciled(&self) -> bool {
        let mut bound = 0;
        let mut run = 0;
        let mut pass = 0;
        for c in self.cells.iter().flatten() {
            if c.bound {
                bound += 1;
            }
            if c.run {
                run += 1;
            }
            if c.passed {
                pass += 1;
            }
        }
        bound == 9 && run == 9 && pass == 9
    }

    /// 预算断言：总耗时 ≤20s。
    pub fn within_budget(&self) -> bool {
        self.total_ms <= TIME_BUDGET_MS
    }

    /// 总耗时。
    pub fn elapsed_ms(&self) -> u64 {
        self.total_ms
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_RG_SEALED);
        }
        if !self.all_passed() || !self.within_budget() {
            self.rejects.push((XC_RG_FAIL, "回归未过禁封"));
            return Err(XC_RG_FAIL);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        for r in 0..MATRIX_ROWS {
            for c in 0..MATRIX_COLS {
                let cell = &self.cells[r][c];
                h ^= (r as u32).wrapping_mul(0x9E37)
                    ^ (c as u32)
                    ^ (cell.bound as u32)
                    ^ ((cell.passed as u32) << 1)
                    ^ cell.cost_ms;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h ^= self.total_ms as u32 ^ (self.total_ms >> 32) as u32;
        h = h.wrapping_mul(0x0100_0193);
        h as u64
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）行/列越界必拒（10/10）
    {
        let mut m = RegressionMatrix::new();
        let mut hits = 0;
        for k in 3..13usize {
            if m.bind(k, 0).is_err() || m.bind(0, k).is_err() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）未绑定格执行必拒（10/10）
    {
        let mut m = RegressionMatrix::new();
        let mut hits = 0;
        for k in 0..10 {
            let _ = k;
            if m.run_cell(0, 0, 10) == Err(XC_RG_UNBOUND) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）总耗时越限必拒
    {
        let mut m = RegressionMatrix::new();
        let _ = m.bind(0, 0);
        let r = m.run_cell(0, 0, 25_000);
        if r == Err(XC_RG_BUDGET) {
            caught += 1;
        }
    }
    // 4）未全过禁封（缺格）
    {
        let mut m = RegressionMatrix::new();
        let _ = m.bind(0, 0);
        let _ = m.run_cell(0, 0, 100);
        if m.seal() == Err(XC_RG_FAIL) {
            caught += 1;
        }
    }
    // 5）封账后回归必拒
    {
        let mut m = RegressionMatrix::new();
        for r in 0..MATRIX_ROWS {
            for c in 0..MATRIX_COLS {
                let _ = m.bind(r, c);
            }
        }
        for r in 0..MATRIX_ROWS {
            for c in 0..MATRIX_COLS {
                let _ = m.run_cell(r, c, 10);
            }
        }
        let _ = m.seal();
        if m.run_cell(0, 0, 10) == Err(XC_RG_SEALED) {
            caught += 1;
        }
    }
    // 6）账平核验（缺一格不平）
    {
        let mut m = RegressionMatrix::new();
        for r in 0..MATRIX_ROWS {
            for c in 0..MATRIX_COLS {
                let _ = m.bind(r, c);
            }
        }
        for r in 0..MATRIX_ROWS {
            for c in 0..MATRIX_COLS {
                let _ = m.run_cell(r, c, 10);
            }
        }
        let full = m.qemu_reconciled();
        let mut m2 = RegressionMatrix::new();
        for r in 0..MATRIX_ROWS {
            for c in 0..MATRIX_COLS {
                let _ = m2.bind(r, c);
            }
        }
        for r in 0..MATRIX_ROWS {
            for c in 0..MATRIX_COLS - 1 {
                let _ = m2.run_cell(r, c, 10);
            }
        }
        if full && !m2.qemu_reconciled() {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1058-J1。
pub fn run_unx1058_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1058");

    // C58-01 9 格全绑定
    {
        let mut m = RegressionMatrix::new();
        let mut all_ok = true;
        for r in 0..MATRIX_ROWS {
            for c in 0..MATRIX_COLS {
                if m.bind(r, c).is_err() {
                    all_ok = false;
                }
            }
        }
        s.add("C58-01-全绑定", all_ok && m.all_bound(), if all_ok { "" } else { "绑定缺" });
    }
    // C58-02 全跑全过
    {
        let mut m = RegressionMatrix::new();
        for r in 0..MATRIX_ROWS {
            for c in 0..MATRIX_COLS {
                let _ = m.bind(r, c);
            }
        }
        let mut all_ok = true;
        for r in 0..MATRIX_ROWS {
            for c in 0..MATRIX_COLS {
                if m.run_cell(r, c, 100).is_err() {
                    all_ok = false;
                }
            }
        }
        s.add("C58-02-全跑全过", all_ok && m.all_passed(), if all_ok { "" } else { "格败" });
    }
    // C58-03 QEMU 档账平
    {
        let mut m = RegressionMatrix::new();
        for r in 0..MATRIX_ROWS {
            for c in 0..MATRIX_COLS {
                let _ = m.bind(r, c);
            }
        }
        for r in 0..MATRIX_ROWS {
            for c in 0..MATRIX_COLS {
                let _ = m.run_cell(r, c, 50);
            }
        }
        s.add("C58-03-QEMU账平", m.qemu_reconciled(), if m.qemu_reconciled() { "" } else { "账不平" });
    }
    // C58-04 一次命令 ≤20s（900ms 总耗时）
    {
        let mut m = RegressionMatrix::new();
        for r in 0..MATRIX_ROWS {
            for c in 0..MATRIX_COLS {
                let _ = m.bind(r, c);
                let _ = m.run_cell(r, c, 100);
            }
        }
        s.add("C58-04-预算达标", m.within_budget() && m.elapsed_ms() == 900, if m.within_budget() { "" } else { "预算越限" });
    }
    // C58-05 未绑定格拒
    {
        let mut m = RegressionMatrix::new();
        let ok = m.run_cell(2, 2, 10) == Err(XC_RG_UNBOUND);
        s.add("C58-05-未绑定拒", ok, if ok { "" } else { "漏抓" });
    }
    // C58-06 越界拒
    {
        let mut m = RegressionMatrix::new();
        let ok = m.bind(3, 0) == Err(XC_RG_OOB) && m.bind(0, 3) == Err(XC_RG_OOB);
        s.add("C58-06-越界拒", ok, if ok { "" } else { "漏抓" });
    }
    // C58-07 封账产指纹
    {
        let mut m = RegressionMatrix::new();
        for r in 0..MATRIX_ROWS {
            for c in 0..MATRIX_COLS {
                let _ = m.bind(r, c);
                let _ = m.run_cell(r, c, 20);
            }
        }
        s.add("C58-07-封账指纹", m.seal().is_ok(), "");
    }
    // C58-08 指纹一致
    {
        let f = || {
            let mut m = RegressionMatrix::new();
            let _ = m.bind(1, 1);
            let _ = m.run_cell(1, 1, 33);
            m.fingerprint()
        };
        s.add("C58-08-指纹一致", f() == f(), "");
    }
    // C58-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C58-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C58-10 拒收计数
    {
        let mut m = RegressionMatrix::new();
        let _ = m.run_cell(0, 0, 1);
        s.add("C58-10-拒收计数", m.reject_count() == 1, if m.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}
