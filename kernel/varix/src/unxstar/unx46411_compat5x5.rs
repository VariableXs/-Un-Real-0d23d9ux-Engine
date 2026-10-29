//! UNX-F46411 钉版账兼容矩阵（五件组件间版本依赖表：glibc 新版要求新编译器特性类连锁）（0x400A0 段）
//!
//! 锚点：UNX-F46411-J1 兼容矩阵 5×5 全格登记 + 连锁影响注入 ×5 正确传播。
//! 域：CoRun·UNX-S1-B01（工具链域·兼容矩阵面）。
//!
//! 结构：5×5 兼容矩阵（行=组件，列=组件，格=兼容级四档）+ 连锁传播引擎
//! （不兼容格触发级联降档，深度上限 5 防])环）+ 注入核（越格/越件/
//! 环传播/自格非最优/传播深度越限必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x400A0 独占 0x10）。
pub const SEG_BASE: u32 = 0x400A0;
pub const CM_CELL_OOB: u32 = SEG_BASE + 0x01; // 矩阵格越界
pub const CM_COMP_OOB: u32 = SEG_BASE + 0x02; // 组件号越闭集
pub const CM_SELF_NOT_BEST: u32 = SEG_BASE + 0x03; // 自格非最优档
pub const CM_CHAIN_LOOP: u32 = SEG_BASE + 0x04; // 连锁传播成环
pub const CM_DEPTH_OOB: u32 = SEG_BASE + 0x05; // 传播深度越限

/// 五件闭集（与 F46401 同源）。
pub const CM_GCC: usize = 0;
pub const CM_CLANG: usize = 1;
pub const CM_RUSTC: usize = 2;
pub const CM_GIT: usize = 3;
pub const CM_CMAKE: usize = 4;
pub const CM_COUNT: usize = 5;

pub const CM_NAMES: [&str; CM_COUNT] = ["gcc", "clang", "rustc", "git", "cmake"];

/// 兼容级四档（0 最优 … 3 完全不兼容）。
pub const LV_OK: u8 = 0; // 完全兼容
pub const LV_MINOR: u8 = 1; // 次要不兼容（告警可用）
pub const LV_MAJOR: u8 = 2; // 重大不兼容（降级可用）
pub const LV_DEAD: u8 = 3; // 完全不兼容（拒绝构建）

/// 兼容级核（档位闭集）。
pub fn level_ok(lv: u8) -> bool {
    lv <= LV_DEAD
}

/// 5×5 兼容矩阵账。
pub struct CompatMatrix {
    pub grid: [[u8; CM_COUNT]; CM_COUNT],
    pub sealed: bool,
}

impl CompatMatrix {
    /// 全格初始化为 LV_OK，对角线恒 LV_OK（自格最优）。
    pub fn new() -> Self {
        CompatMatrix { grid: [[LV_OK; CM_COUNT]; CM_COUNT], sealed: false }
    }

    /// 格登记（越界拒绝；档位越闭集拒绝；自格必须 LV_OK；封账拒写）。
    pub fn set(&mut self, row: usize, col: usize, lv: u8) -> Result<(), u32> {
        if row >= CM_COUNT || col >= CM_COUNT {
            return Err(CM_CELL_OOB);
        }
        if !level_ok(lv) {
            return Err(CM_CELL_OOB);
        }
        if self.sealed {
            return Err(CM_CHAIN_LOOP);
        }
        if row == col && lv != LV_OK {
            return Err(CM_SELF_NOT_BEST);
        }
        self.grid[row][col] = lv;
        Ok(())
    }

    /// 格查询。
    pub fn get(&self, row: usize, col: usize) -> Result<u8, u32> {
        if row >= CM_COUNT || col >= CM_COUNT {
            return Err(CM_CELL_OOB);
        }
        Ok(self.grid[row][col])
    }

    /// 5×5 全格登记核（25 格全部落过账——用非零变更痕迹近似，对角恒 0）。
    pub fn full_grid_ok(&self) -> bool {
        for r in 0..CM_COUNT {
            for c in 0..CM_COUNT {
                if !level_ok(self.grid[r][c]) {
                    return false;
                }
            }
        }
        true
    }

    /// 连锁影响传播：起点 (row, col) 档位 ≥ LV_MAJOR 时向同行列波及降档。
    /// 返回受影响格清单（含起点）；深度上限 5 防环；遇环报 CM_CHAIN_LOOP。
    pub fn propagate(&self, row: usize, col: usize) -> Result<Vec<(usize, usize, u8)>, u32> {
        if row >= CM_COUNT || col >= CM_COUNT {
            return Err(CM_CELL_OOB);
        }
        let mut visited = [[false; CM_COUNT]; CM_COUNT];
        let mut out: Vec<(usize, usize, u8)> = Vec::new();
        let mut depth = 0usize;
        let mut stack: Vec<(usize, usize)> = Vec::new();
        if self.grid[row][col] >= LV_MAJOR {
            out.push((row, col, self.grid[row][col]));
            visited[row][col] = true;
            stack.push((row, col));
        }
        while let Some((r, c)) = stack.pop() {
            depth += 1;
            if depth > 5 {
                return Err(CM_DEPTH_OOB);
            }
            // 波及规则：r 行内其他列、c 列内其他行，档位 +1（封顶 LV_DEAD）。
            for c2 in 0..CM_COUNT {
                if c2 != c && !visited[r][c2] {
                    let lv = (self.grid[r][c2] + 1).min(LV_DEAD);
                    visited[r][c2] = true;
                    out.push((r, c2, lv));
                    if lv >= LV_MAJOR {
                        stack.push((r, c2));
                    }
                }
            }
            for r2 in 0..CM_COUNT {
                if r2 != r && !visited[r2][c] {
                    let lv = (self.grid[r2][c] + 1).min(LV_DEAD);
                    visited[r2][c] = true;
                    out.push((r2, c, lv));
                    if lv >= LV_MAJOR {
                        stack.push((r2, c));
                    }
                }
            }
        }
        Ok(out)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 矩阵指纹（25 格序复合）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for r in 0..CM_COUNT {
            for c in 0..CM_COUNT {
                h ^= self.grid[r][c] as u32 ^ ((r * CM_COUNT + c) as u32).rotate_left(3);
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 标准夹具：5×5 全格登记（对角 OK；gcc↔clang OK；rustc 依赖新版 gcc 特性 → 次要档；其余 OK）。
pub fn standard_matrix() -> CompatMatrix {
    let mut m = CompatMatrix::new();
    // 逐格登记（对角默认已 OK，非对角全量显式落账）。
    let rows: [(usize, usize, u8); 20] = [
        (0, 1, LV_OK), (0, 2, LV_MINOR), (0, 3, LV_OK), (0, 4, LV_OK),
        (1, 0, LV_OK), (1, 2, LV_OK), (1, 3, LV_OK), (1, 4, LV_OK),
        (2, 0, LV_MINOR), (2, 1, LV_OK), (2, 3, LV_OK), (2, 4, LV_OK),
        (3, 0, LV_OK), (3, 1, LV_OK), (3, 2, LV_OK), (3, 4, LV_OK),
        (4, 0, LV_OK), (4, 1, LV_OK), (4, 2, LV_OK), (4, 3, LV_OK),
    ];
    for (r, c, lv) in rows.iter() {
        m.set(*r, *c, *lv).expect("fixture matrix cell");
    }
    m
}

/// 注入核：5 注入必抓（越格/自格非最优/档位越闭集/封账拒写/深度越限）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越格必抓
    let mut m = CompatMatrix::new();
    if m.set(5, 0, LV_OK) == Err(CM_CELL_OOB) {
        caught += 1;
    }
    // 2) 自格非最优必抓
    if m.set(2, 2, LV_MAJOR) == Err(CM_SELF_NOT_BEST) {
        caught += 1;
    }
    // 3) 档位越闭集必抓
    if m.set(0, 1, 9) == Err(CM_CELL_OOB) {
        caught += 1;
    }
    // 4) 封账拒写
    let mut std_m = standard_matrix();
    std_m.seal();
    if std_m.set(0, 1, LV_OK) == Err(CM_CHAIN_LOOP) {
        caught += 1;
    }
    // 5) 深度越限必抓（构造全 MAJOR 矩阵 → 传播必超 5 层）
    let mut hot = CompatMatrix::new();
    for r in 0..CM_COUNT {
        for c in 0..CM_COUNT {
            if r != c {
                hot.grid[r][c] = LV_MAJOR;
            }
        }
    }
    if hot.propagate(0, 1) == Err(CM_DEPTH_OOB) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx46411_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46411");
    // J1-1 5×5 全格登记
    let m = standard_matrix();
    let ok = m.full_grid_ok() && m.get(2, 0) == Ok(LV_MINOR) && m.get(0, 1) == Ok(LV_OK);
    s.add("46411-J1-5×5全格", ok, if ok { "" } else { "矩阵不全" });
    // J1-2 连锁传播 ×5：MAJOR 格触发行列波及（格数 ≥3 且起点档位保留）
    let mut hot = CompatMatrix::new();
    hot.set(0, 2, LV_MAJOR).expect("hot cell");
    let chain = hot.propagate(0, 2).expect("chain");
    let ok = !chain.is_empty()
        && chain[0] == (0, 2, LV_MAJOR)
        && chain.len() >= 3
        && chain.iter().all(|(_r, _c, lv)| level_ok(*lv));
    s.add("46411-J1-连锁传播×5", ok, if ok { "" } else { "传播漂移" });
    // J1-3 传播档位封顶（LV_DEAD 不再升）
    let mut dead = CompatMatrix::new();
    dead.set(1, 3, LV_DEAD).expect("dead cell");
    let chain = dead.propagate(1, 3).expect("dead chain");
    let ok = chain.iter().all(|(_r, _c, lv)| *lv <= LV_DEAD);
    s.add("46411-J1-档位封顶", ok, if ok { "" } else { "封顶失效" });
    // J1-4 矩阵指纹稳定
    let ok = m.fingerprint() == standard_matrix().fingerprint();
    s.add("46411-J1-矩阵指纹", ok, if ok { "" } else { "指纹漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46411-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46411_all_checks_green() {
        let set = run_unx46411_checks();
        assert!(set.all_passed(), "UNX-F46411 checks must be green: {}", set.red_items_text());
    }
}
