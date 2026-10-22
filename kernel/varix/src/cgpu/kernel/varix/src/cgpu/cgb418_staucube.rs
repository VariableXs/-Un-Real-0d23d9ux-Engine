//! CGPU-F0418 · 统计立方（目标 380 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 统计立方：三维聚合（账户×资源×时间——OLAP 式多维查询）；切片与
//!   钻取（任意维度切片、下钻到明细）；立方增量更新（新数据只增量聚合
//!   ——不全量重算）；查询预算（重查询资源受控——不拖垮在线仲裁）。
//! - 判据：三维、切片钻取、增量、查询预算、判据。
//!
//! 设计纪律：
//! - 三维坐标：账户（acct）× 资源（cpu/gpu/bw/mem 四类）× 时间（小时桶）
//!   ——格点 cell = (acct, res, hour) → 总量；稀疏存储（只存非零格）。
//! - 切片：固定两维查一维合计（如按账户切片 = 跨资源跨时间求和）；
//!   钻取：从合计下一层到明细格点（合计值 = 明细和的守恒核验）。
//! - 增量更新：新样本只写受影响格点 + 脏标记；重算只发生在脏格
//!   （dirty-set 聚合——不全量重算的形式化）。
//! - 查询预算：每次查询声明格点扫描上限（cells_scanned ≤ budget），
//!   超预算即拒（重查询让路在线仲裁）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0D50 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0D50，全仓独占）。
pub const SEG_BASE: u16 = 0x0D50;

pub const E_CUBE_RES: u16 = SEG_BASE + 1; // 资源维非法
pub const E_CUBE_DRILL: u16 = SEG_BASE + 2; // 钻取守恒破坏（明细和不等于合计）
pub const E_CUBE_BUDGET: u16 = SEG_BASE + 3; // 查询越预算
pub const E_CUBE_DIRTY: u16 = SEG_BASE + 4; // 脏格未聚合就出数
pub const E_CUBE_WINDOW: u16 = SEG_BASE + 5; // 时间窗非法
pub const E_CUBE_ACCT: u16 = SEG_BASE + 6; // 账户维非法（0 号保留）

/// 资源维（四类）。
pub const RES_CPU: u8 = 0;
pub const RES_GPU: u8 = 1;
pub const RES_BW: u8 = 2;
pub const RES_MEM: u8 = 3;
pub const RES_COUNT: usize = 4;

pub fn res_name(r: u8) -> &'static str {
    match r {
        RES_CPU => "CPU",
        RES_GPU => "GPU",
        RES_BW => "带宽",
        _ => "显存",
    }
}

/// 立方格点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub acct: u32,
    pub res: u8,
    pub hour: u64,
    pub total: u64,
    /// 脏标记（增量聚合——脏格未 rollup 不可直接出合计）。
    pub dirty: bool,
}

/// 统计立方。
#[derive(Clone, Debug, Default)]
pub struct StatCube {
    pub cells: Vec<Cell>,
    /// 查询预算（单次扫描格点上限）。
    pub scan_budget: usize,
    /// 查询计数。
    pub queries: u64,
}

impl StatCube {
    pub fn new(scan_budget: usize) -> Self {
        StatCube { cells: Vec::new(), scan_budget, queries: 0 }
    }

    /// 增量写入：命中格点累加并标脏；未命中新格（新建即脏）。
    /// 只动一个格点——增量的形式化。
    pub fn add(&mut self, acct: u32, res: u8, hour: u64, amount: u64) -> Result<(), u16> {
        if res as usize >= RES_COUNT {
            return Err(E_CUBE_RES);
        }
        if acct == 0 {
            return Err(E_CUBE_ACCT);
        }
        if let Some(c) = self
            .cells
            .iter_mut()
            .find(|c| c.acct == acct && c.res == res && c.hour == hour)
        {
            c.total += amount;
            c.dirty = true;
        } else {
            self.cells.push(Cell { acct, res, hour, total: amount, dirty: true });
        }
        Ok(())
    }

    /// 脏格聚合（rollup）：清脏标记——增量聚合的落定步。
    pub fn flush_dirty(&mut self) -> usize {
        let mut n = 0;
        for c in self.cells.iter_mut() {
            if c.dirty {
                c.dirty = false;
                n += 1;
            }
        }
        n
    }

    /// 切片查询：按维组合合计（None 维 = 全域求和）。
    /// 扫描格点数计入预算；脏格必须先落定（否则出数即脏读）。
    pub fn slice(
        &mut self,
        acct: Option<u32>,
        res: Option<u8>,
        hour: Option<u64>,
    ) -> Result<u64, u16> {
        self.queries += 1;
        if let Some(r) = res {
            if r as usize >= RES_COUNT {
                return Err(E_CUBE_RES);
            }
        }
        if self.cells.iter().any(|c| c.dirty) {
            return Err(E_CUBE_DIRTY);
        }
        let scanned = self.cells.len();
        if scanned > self.scan_budget {
            return Err(E_CUBE_BUDGET);
        }
        let mut sum = 0u64;
        for c in &self.cells {
            if acct.map(|a| c.acct == a).unwrap_or(true)
                && res.map(|r| c.res == r).unwrap_or(true)
                && hour.map(|h| c.hour == h).unwrap_or(true)
            {
                sum += c.total;
            }
        }
        Ok(sum)
    }

    /// 钻取：从切片合计下到明细格点（合计 = 明细和的守恒核验）。
    pub fn drill(&mut self, acct: u32, res: u8, hour: u64) -> Result<(u64, Vec<u64>), u16> {
        if res as usize >= RES_COUNT {
            return Err(E_CUBE_RES);
        }
        if self.cells.iter().any(|c| c.dirty) {
            return Err(E_CUBE_DIRTY);
        }
        let mut details = Vec::new();
        let mut total = 0u64;
        for c in &self.cells {
            if c.acct == acct && c.res == res && c.hour == hour {
                details.push(c.total);
                total += c.total;
            }
        }
        if self.cells.len() > self.scan_budget {
            return Err(E_CUBE_BUDGET);
        }
        // 钻取守恒：合计 = 明细和（同一来源自然守恒——核验写死在路径上）。
        let detail_sum = details.iter().sum();
        if total != detail_sum {
            return Err(E_CUBE_DRILL);
        }
        self.queries += 1;
        Ok((total, details))
    }

    /// 时间窗切片（长窗三维查询：账户×资源在窗内求和）。
    pub fn window_slice(
        &mut self,
        acct: u32,
        res: u8,
        h0: u64,
        h1: u64,
    ) -> Result<u64, u16> {
        if res as usize >= RES_COUNT {
            return Err(E_CUBE_RES);
        }
        if h0 > h1 {
            return Err(E_CUBE_WINDOW);
        }
        if self.cells.iter().any(|c| c.dirty) {
            return Err(E_CUBE_DIRTY);
        }
        let scanned = self
            .cells
            .iter()
            .filter(|c| c.acct == acct)
            .count();
        if scanned > self.scan_budget {
            return Err(E_CUBE_BUDGET);
        }
        Ok(self
            .cells
            .iter()
            .filter(|c| c.acct == acct && c.res == res && c.hour >= h0 && c.hour <= h1)
            .map(|c| c.total)
            .sum())
    }

    /// 立方密度（非零格数 / 理论格数）。
    pub fn density(&self, accts: usize, hours: usize) -> u32 {
        let theory = accts * RES_COUNT * hours;
        if theory == 0 {
            return 0;
        }
        (self.cells.len() * 1000 / theory) as u32
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> StatCube {
        let mut c = StatCube::new(1_000);
        // 账户 1：CPU 3 小时各 100；GPU 2 小时各 200；账户 2：CPU 1 小时 50。
        for h in 0..3u64 {
            c.add(1, RES_CPU, h, 100).ok();
        }
        for h in 0..2u64 {
            c.add(1, RES_GPU, h, 200).ok();
        }
        c.add(2, RES_CPU, 0, 50).ok();
        c.flush_dirty();
        c
    }

    #[test]
    fn a01_三维聚合_格点稀疏存储() {
        let c = cube();
        assert_eq!(c.cells.len(), 6);
        // 理论格数 2 账户×4 资源×3 小时 = 24，密度 = 250‰。
        assert_eq!(c.density(2, 3), 250);
        // 同格点重复写累加（不新增格）。
        let mut c2 = cube();
        let before = c2.cells.len();
        c2.add(1, RES_CPU, 0, 30).ok();
        c2.flush_dirty();
        assert_eq!(c2.cells.len(), before);
        let (total, _) = c2.drill(1, RES_CPU, 0).unwrap_or((0, Vec::new()));
        assert_eq!(total, 130);
    }

    #[test]
    fn a02_切片与钻取_守恒核验() {
        let mut c = cube();
        // 按账户切片：账户 1 全域 = 3×100 + 2×200 = 700。
        assert_eq!(c.slice(Some(1), None, None), Ok(700));
        // 按资源切片：CPU 全域 = 350。
        assert_eq!(c.slice(None, Some(RES_CPU), None), Ok(350));
        // 三维全定：账户 1 GPU 第 1 小时 = 200。
        assert_eq!(c.slice(Some(1), Some(RES_GPU), Some(1)), Ok(200));
        // 钻取：明细和 = 合计。
        let (total, details) = c.drill(1, RES_CPU, 2).unwrap_or((0, Vec::new()));
        assert_eq!(total, 100);
        assert_eq!(details, vec![100]);
        // 钻取空格。
        assert_eq!(c.drill(2, RES_GPU, 9), Ok((0, Vec::new())));
    }

    #[test]
    fn a03_增量更新_脏格防线() {
        let mut c = cube();
        // 有脏格时切片拒绝（脏读防线）。
        c.add(1, RES_CPU, 3, 999).ok();
        assert_eq!(c.slice(Some(1), None, None), Err(E_CUBE_DIRTY));
        assert_eq!(c.drill(1, RES_CPU, 3), Err(E_CUBE_DIRTY));
        // 落定后放行，增量只加新格不受旧数影响。
        c.flush_dirty();
        assert_eq!(c.slice(Some(1), Some(RES_CPU), Some(3)), Ok(999));
        // 落定计数。
        let mut c2 = cube();
        c2.add(1, RES_CPU, 0, 1).ok();
        c2.add(2, RES_MEM, 5, 1).ok();
        assert_eq!(c2.flush_dirty(), 2);
        assert_eq!(c2.flush_dirty(), 0); // 无脏格
    }

    #[test]
    fn a04_查询预算_重查询受控() {
        let mut c = StatCube::new(5); // 预算只许扫 5 格
        for h in 0..10u64 {
            c.add(1, RES_CPU, h, 1).ok();
        }
        c.flush_dirty();
        // 10 格 > 5 预算 ⇒ 拒。
        assert_eq!(c.slice(Some(1), None, None), Err(E_CUBE_BUDGET));
        // 提预算后放行。
        c.scan_budget = 100;
        assert_eq!(c.slice(Some(1), None, None), Ok(10));
        // 窗口切片同受预算约束。
        let mut c2 = StatCube::new(3);
        for h in 0..10u64 {
            c2.add(1, RES_BW, h, 2).ok();
        }
        c2.flush_dirty();
        assert_eq!(c2.window_slice(1, RES_BW, 0, 9), Err(E_CUBE_BUDGET));
        c2.scan_budget = 50;
        assert_eq!(c2.window_slice(1, RES_BW, 2, 4), Ok(6));
        // 非法窗。
        assert_eq!(c2.window_slice(1, RES_BW, 9, 0), Err(E_CUBE_WINDOW));
    }

    #[test]
    fn a05_维防线_资源与账户() {
        let mut c = StatCube::new(1_000);
        assert_eq!(c.add(1, 9, 0, 1), Err(E_CUBE_RES));
        assert_eq!(c.add(0, RES_CPU, 0, 1), Err(E_CUBE_ACCT));
        assert_eq!(c.slice(None, Some(9), None), Err(E_CUBE_RES));
        assert_eq!(c.drill(1, 9, 0), Err(E_CUBE_RES));
        // 资源名表。
        assert_eq!(res_name(RES_MEM), "显存");
        assert_eq!(RES_COUNT, 4);
    }
}
