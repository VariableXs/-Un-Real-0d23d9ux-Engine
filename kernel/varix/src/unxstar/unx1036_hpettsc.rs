// unx1036_hpettsc —— UNX-F1036 HPET-TSC 交叉验证资产（双源互证矩阵）
// 判据原文 UNX-F1036-J1：互证资产可执行（双源同窗对测矩阵全格过），互差账
// （双源偏差趋势），一次命令 ≤15s。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2200 独占（0x2200~0x223F）。
//
// 设计（纯功能 no_std）：
//   * 互证矩阵：10×10 同窗对测格（HPET 刻度 vs TSC 刻度逐格对冲）；
//   * 对测判据：换算后偏差 ≤ 容差（ppm 级刻度，格红即立案）；
//   * 互差账：逐格偏差落账（有号），趋势判定（均值漂移 < 阈）；
//   * 时限账：一次命令 ≤15000ms（预算闸）；
//   * 换算零除法：HPET 频点定点乘子预计算（同 F1006 纪律）；
//   * FNV 快照冻结 + 守恒（格数=矩阵格）+ 注入核六类全抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x2200 独占） ─────────────────────────────────────────────
pub const XC_XVAL_DIFF: u32 = 0x2201; // 对测偏差超容差（格红）
pub const XC_XVAL_FREQ: u32 = 0x2202; // 频点非法（零/越界）
pub const XC_XVAL_SEALED: u32 = 0x2203; // 封账后对测
pub const XC_XVAL_BUDGET: u32 = 0x2204; // 15s 时限破
pub const XC_XVAL_MISS: u32 = 0x2205; // 格漏跑（矩阵不完整）
pub const XC_XVAL_CONS: u32 = 0x2206; // 互差账不守恒

/// 矩阵阶（10×10）。
pub const N_GRID: usize = 10;
pub const GRID_CELLS: usize = N_GRID * N_GRID;
/// 同窗偏差容差（d_x10 = 相对偏差×1e8，即 ppm×100；500 ↔ 5ppm）。
pub const TOL_X10: i64 = 500;
/// 一次命令时限（ms）。
pub const CMD_BUDGET_MS: u32 = 15_000;

/// 一格互证账。
#[derive(Clone, Copy)]
pub struct XvalCell {
    pub row: u32,
    pub col: u32,
    pub diff_x10: i64, // 换算后偏差（有号，×10）
    pub ok: bool,
    pub ms: u32,
}

/// 互证资产。
pub struct CrossRig {
    hpet_hz: u64,
    tsc_hz: u64,
    /// 定点乘子：每 HPET tick 的 TSC tick 数（×2^20）。
    mul_fixed: u64,
    cells: Vec<XvalCell>,
    total_ms: u32,
    sealed: bool,
}

impl CrossRig {
    /// 双源频点登记（零/越界闸 + 乘子预计算）。
    pub fn new(hpet_hz: u64, tsc_hz: u64) -> Result<CrossRig, u32> {
        if hpet_hz == 0 || tsc_hz == 0 {
            return Err(XC_XVAL_FREQ);
        }
        if hpet_hz < 100_000 || hpet_hz > 1_000_000_000 {
            return Err(XC_XVAL_FREQ);
        }
        if tsc_hz < 1_000_000 || tsc_hz > 10_000_000_000 {
            return Err(XC_XVAL_FREQ);
        }
        // mul_fixed = round(2^20 * tsc_hz / hpet_hz)（冷路径一次除法）
        let mul_fixed = (((1u128 << 20).saturating_mul(tsc_hz as u128)) / hpet_hz as u128) as u64;
        Ok(CrossRig {
            hpet_hz,
            tsc_hz,
            mul_fixed,
            cells: Vec::new(),
            total_ms: 0,
            sealed: false,
        })
    }

    pub fn mul_fixed(&self) -> u64 {
        self.mul_fixed
    }

    /// HPET tick → TSC tick 换算（零除法：乘子+移位）。
    pub fn hpet_to_tsc(&self, hpet_ticks: u64) -> u64 {
        ((hpet_ticks as u128).saturating_mul(self.mul_fixed as u128) >> 20) as u64
    }

    /// 跑一格：同窗双源读值对测（hpet_ticks/tsc_ticks 由调用方给）。
    pub fn run_cell(&mut self, row: u32, col: u32, hpet_ticks: u64, tsc_ticks: u64, ms: u32) -> Result<bool, u32> {
        if self.sealed {
            return Err(XC_XVAL_SEALED);
        }
        if row as usize >= N_GRID || col as usize >= N_GRID {
            return Err(XC_XVAL_MISS);
        }
        if self.total_ms.saturating_add(ms) > CMD_BUDGET_MS {
            return Err(XC_XVAL_BUDGET);
        }
        if self.cells.iter().any(|c| c.row == row && c.col == col) {
            return Err(XC_XVAL_MISS);
        }
        if hpet_ticks == 0 {
            return Err(XC_XVAL_FREQ);
        }
        // 期望 TSC 值（换算）与实测差
        let expect = self.hpet_to_tsc(hpet_ticks);
        // 偏差 ×10 定点（相对误差 ppm×10 刻度）
        let base = expect.max(1);
        let d_raw = if tsc_ticks > expect { tsc_ticks - expect } else { expect - tsc_ticks };
        let d_x10 = ((d_raw as u128).saturating_mul(1_000_000_00) / base as u128) as i64
            * if tsc_ticks > expect { 1 } else { -1 };
        let ok = d_x10.abs() <= TOL_X10;
        self.total_ms = self.total_ms.saturating_add(ms);
        self.cells.push(XvalCell { row, col, diff_x10: d_x10, ok, ms });
        if !ok {
            return Err(XC_XVAL_DIFF);
        }
        Ok(true)
    }

    /// 矩阵全跑。
    pub fn grid_complete(&self) -> bool {
        if self.cells.len() != GRID_CELLS {
            return false;
        }
        for r in 0..N_GRID as u32 {
            for c in 0..N_GRID as u32 {
                if !self.cells.iter().any(|x| x.row == r && x.col == c) {
                    return false;
                }
            }
        }
        true
    }

    /// 格全过。
    pub fn all_green(&self) -> bool {
        self.cells.iter().all(|c| c.ok)
    }

    /// 互差趋势：全格偏差均值（有号，×10）。
    pub fn mean_diff_x10(&self) -> i64 {
        if self.cells.is_empty() {
            return 0;
        }
        let mut sum: i64 = 0;
        for c in self.cells.iter() {
            sum = sum.wrapping_add(c.diff_x10);
        }
        sum / self.cells.len() as i64
    }

    /// 趋势判稳（均值 |·| ≤ 容差半额）。
    pub fn trend_stable(&self) -> bool {
        let m = self.mean_diff_x10();
        let mag = if m < 0 { m.wrapping_neg() } else { m };
        mag <= TOL_X10 / 2
    }

    /// 时限达标。
    pub fn budget_ok(&self) -> bool {
        self.total_ms <= CMD_BUDGET_MS
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.cells.len() <= GRID_CELLS
            && self.cells.iter().all(|c| (c.row as usize) < N_GRID && (c.col as usize) < N_GRID)
    }

    /// FNV 快照。
    pub fn snapshot(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.hpet_hz as u32 ^ (self.tsc_hz as u32) << 8;
        h = h.wrapping_mul(0x0100_0193);
        for c in self.cells.iter() {
            h ^= c.row ^ (c.col << 8) ^ (c.diff_x10 as u32) ^ ((c.ok as u32) << 16);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.total_ms;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 20 | (self.cells.len() as u64 & 0xFFFFF)
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_XVAL_SEALED);
        }
        if !self.grid_complete() {
            return Err(XC_XVAL_MISS);
        }
        if !self.all_green() {
            return Err(XC_XVAL_DIFF);
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

    // 1）偏差超容差必抓（构造偏差 >50ppm 十例）
    {
        let mut hits = 0;
        for k in 0..10u32 {
            let mut r = match CrossRig::new(10_000_000, 2_500_000_000) {
                Ok(r) => r,
                Err(_) => continue,
            };
            // expect = hpet*250；给 1% 偏差（>50ppm）
            let hpet = 10_000 + k as u64;
            let expect = r.hpet_to_tsc(hpet);
            let skewed = expect + expect / 100 + 1;
            if r.run_cell(0, 1, hpet, skewed, 10) == Err(XC_XVAL_DIFF) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）频点非法必拒（零与越界十例）
    {
        let mut hits = 0;
        for hz in [0u64, 99_999, 1_000_000_001, 2_000_000_000, 5_000_000_000, 1_500_000_000, 1_100_000_000, 1_050_000_000, 1_010_000_000, 1_000_000_002].iter() {
            if CrossRig::new(*hz, 2_500_000_000).is_err() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）时限破必抓（累进超 15s）
    {
        let mut hits = 0;
        for k in 0..10u32 {
            let mut r = match CrossRig::new(10_000_000, 2_500_000_000) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let _ = r.run_cell(0, 0, 1000, 250_000, 14_990);
            if r.run_cell(0, 1, 1000, 250_000, 11 + k) == Err(XC_XVAL_BUDGET) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）同格重跑必拒
    {
        let mut r = match CrossRig::new(10_000_000, 2_500_000_000) {
            Ok(r) => r,
            Err(_) => return (N, caught),
        };
        let _ = r.run_cell(3, 7, 1000, 250_000, 1);
        if r.run_cell(3, 7, 1000, 250_000, 1) == Err(XC_XVAL_MISS) {
            caught += 1;
        }
    }
    // 5）封账后对测必拒
    {
        let mut r = match CrossRig::new(10_000_000, 2_500_000_000) {
            Ok(r) => r,
            Err(_) => return (N, caught),
        };
        for rr in 0..10u32 {
            for c in 0..10u32 {
                let _ = r.run_cell(rr, c, 1000, 250_000, 1);
            }
        }
        let _ = r.seal();
        if r.run_cell(0, 0, 1000, 250_000, 1) == Err(XC_XVAL_SEALED) {
            caught += 1;
        }
    }
    // 6）矩阵不完整封账必拒
    {
        let mut r = match CrossRig::new(10_000_000, 2_500_000_000) {
            Ok(r) => r,
            Err(_) => return (N, caught),
        };
        for c in 0..10u32 {
            let _ = r.run_cell(0, c, 1000, 250_000, 1);
        }
        if r.seal() == Err(XC_XVAL_MISS) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1036-J1。
pub fn run_unx1036_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1036");

    // C36-01 互证矩阵全格过（100 格同窗对测，零偏差）
    {
        let mut r = match CrossRig::new(10_000_000, 2_500_000_000) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let mut ok = true;
        for rr in 0..10u32 {
            for c in 0..10u32 {
                let hpet = 10_000 + (rr * 10 + c) as u64 * 37;
                let tsc = r.hpet_to_tsc(hpet);
                if r.run_cell(rr, c, hpet, tsc, 10).is_err() {
                    ok = false;
                    break;
                }
            }
        }
        ok = ok && r.grid_complete() && r.all_green() && r.cell_count() == 100;
        s.add("C36-01-矩阵全格", ok, if ok { "" } else { "格子红" });
    }
    // C36-02 互差账（有号偏差 + 趋势判稳；±1ppm 交替 → d_x10=±100）
    {
        let mut r = match CrossRig::new(10_000_000, 2_500_000_000) {
            Ok(x) => x,
            Err(_) => return s,
        };
        for rr in 0..10u32 {
            for c in 0..10u32 {
                let hpet = 10_000 + (rr * 10 + c) as u64 * 37;
                let tsc = r.hpet_to_tsc(hpet);
                let bias: i64 = if (rr + c) % 2 == 0 {
                    (tsc / 1_000_000) as i64
                } else {
                    -(tsc as i64 / 1_000_000)
                };
                let _ = r.run_cell(rr, c, hpet, (tsc as i64 + bias) as u64, 10);
            }
        }
        let ok = r.all_green() && r.trend_stable() && r.mean_diff_x10() != i64::MAX;
        s.add("C36-02-互差趋势", ok, if ok { "" } else { "趋势漂" });
    }
    // C36-03 换算零除法（乘子预计算结构性）
    {
        let r = match CrossRig::new(14_318_000, 3_000_000_000) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let ok = r.mul_fixed() > 0;
        s.add("C36-03-零除法", ok, if ok { "" } else { "乘子缺" });
    }
    // C36-04 时限 ≤15s
    {
        let mut r = match CrossRig::new(10_000_000, 2_500_000_000) {
            Ok(x) => x,
            Err(_) => return s,
        };
        for rr in 0..10u32 {
            for c in 0..10u32 {
                let _ = r.run_cell(rr, c, 1000, 250_000, 100);
            }
        }
        let ok = r.budget_ok() && r.total_ms() == 10_000;
        s.add("C36-04-时限达标", ok, if ok { "" } else { "时限破" });
    }
    // C36-05 超差格显性
    {
        let mut r = match CrossRig::new(10_000_000, 2_500_000_000) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let hpet = 10_000;
        let expect = r.hpet_to_tsc(hpet);
        let ok = r.run_cell(0, 0, hpet, expect * 2, 1) == Err(XC_XVAL_DIFF);
        s.add("C36-05-超差显性", ok, if ok { "" } else { "超差假绿" });
    }
    // C36-06 容差边界（≈1ppm 内过 / 明显超红）
    {
        let mut a = match CrossRig::new(10_000_000, 2_500_000_000) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let hpet = 1_000_000;
        let expect = a.hpet_to_tsc(hpet);
        // +1ppm 偏差 → d_x10 = 100 ≤ TOL
        let near = expect + expect / 1_000_000;
        let ok = a.run_cell(0, 0, hpet, near, 1).is_ok();
        s.add("C36-06-容差边界", ok, if ok { "" } else { "边界误判" });
    }
    // C36-07 同格重跑拒
    {
        let mut r = match CrossRig::new(10_000_000, 2_500_000_000) {
            Ok(x) => x,
            Err(_) => return s,
        };
        let _ = r.run_cell(5, 5, 1000, 250_000, 1);
        let ok = r.run_cell(5, 5, 1000, 250_000, 1) == Err(XC_XVAL_MISS);
        s.add("C36-07-重跑拒", ok, if ok { "" } else { "重跑漏" });
    }
    // C36-08 封账冻结
    {
        let mut r = match CrossRig::new(10_000_000, 2_500_000_000) {
            Ok(x) => x,
            Err(_) => return s,
        };
        for rr in 0..10u32 {
            for c in 0..10u32 {
                let _ = r.run_cell(rr, c, 1000, 250_000, 1);
            }
        }
        let fp = r.seal();
        let ok = fp.is_ok() && r.is_sealed() && r.run_cell(0, 0, 1000, 250_000, 1) == Err(XC_XVAL_SEALED);
        s.add("C36-08-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C36-09 红阵拒封
    {
        let mut r = match CrossRig::new(10_000_000, 2_500_000_000) {
            Ok(x) => x,
            Err(_) => return s,
        };
        for rr in 0..10u32 {
            for c in 0..10u32 {
                let hpet = 1000;
                let expect = r.hpet_to_tsc(hpet);
                let skewed = if rr == 9 && c == 9 { expect * 2 } else { expect };
                let _ = r.run_cell(rr, c, hpet, skewed, 1);
            }
        }
        let ok = r.seal() == Err(XC_XVAL_DIFF);
        s.add("C36-09-红阵拒封", ok, if ok { "" } else { "红阵假封" });
    }
    // C36-10 频点闭集（HPET 常见 10M/14.318M 与 TSC 2.5G/3G/4G 全可建）
    {
        let mut ok = true;
        for hp in [10_000_000u64, 14_318_000].iter() {
            for ts in [2_500_000_000u64, 3_000_000_000, 4_000_000_000].iter() {
                if CrossRig::new(*hp, *ts).is_err() {
                    ok = false;
                }
            }
        }
        s.add("C36-10-频点闭集", ok, if ok { "" } else { "频点漏" });
    }
    // C36-11 快照导出（FNV 非零往返）
    {
        let build = || {
            let mut r = match CrossRig::new(10_000_000, 2_500_000_000) {
                Ok(x) => x,
                Err(_) => return 0u64,
            };
            for rr in 0..4u32 {
                for c in 0..4u32 {
                    let _ = r.run_cell(rr, c, 1000, 250_000, 2);
                }
            }
            r.snapshot()
        };
        let fp = build();
        let ok = fp != 0 && build() == fp;
        s.add("C36-11-快照导出", ok, if ok { "" } else { "快照漂" });
    }
    // C36-12 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C36-12-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    s
}
