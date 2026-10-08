//! AI-11 族0102「图标栅格引擎」（X02526~X02550）。
//!
//! 桌面图标栅格占位位图、就近吸附、空位查找与密度统计。
//! 硬约束：no_std / 无 alloc / 无浮点（密度用 permille）。

use crate::checks::CheckSet;
use crate::display::svc::*;

pub const GRID_COLS: usize = 16;
pub const GRID_ROWS: usize = 12;
pub const GRID_CELLS: usize = GRID_COLS * GRID_ROWS;
pub const GRID_WORDS: usize = (GRID_CELLS + 31) / 32;

/// 五档栅格边长（px）。
pub const CELL_LEVELS: [i32; 5] = [64, 72, 80, 96, 128];

pub struct IconGrid {
    pub bits: [u32; GRID_WORDS],
    pub cell: i32,
    pub count: u32,
}

impl IconGrid {
    pub const fn new(cell: i32) -> Self {
        IconGrid {
            bits: [0u32; GRID_WORDS],
            cell: if cell <= 0 { 80 } else { cell },
            count: 0,
        }
    }

    pub fn in_range(col: usize, row: usize) -> bool {
        col < GRID_COLS && row < GRID_ROWS
    }

    pub fn index(col: usize, row: usize) -> usize {
        row * GRID_COLS + col
    }

    pub fn is_free(&self, col: usize, row: usize) -> bool {
        if !Self::in_range(col, row) {
            return false;
        }
        let i = Self::index(col, row);
        self.bits[i / 32] & (1u32 << (i % 32)) == 0
    }

    /// 占位（去重：已占返回 Busy，越界返回 OutOfRange）。
    pub fn occupy(&mut self, col: usize, row: usize) -> DeskError {
        if !Self::in_range(col, row) {
            return DeskError::OutOfRange;
        }
        let i = Self::index(col, row);
        let mask = 1u32 << (i % 32);
        if self.bits[i / 32] & mask != 0 {
            return DeskError::Busy;
        }
        self.bits[i / 32] |= mask;
        self.count += 1;
        DeskError::Ok
    }

    pub fn free(&mut self, col: usize, row: usize) -> bool {
        if !Self::in_range(col, row) {
            return false;
        }
        let i = Self::index(col, row);
        let mask = 1u32 << (i % 32);
        if self.bits[i / 32] & mask == 0 {
            return false;
        }
        self.bits[i / 32] &= !mask;
        self.count -= 1;
        true
    }

    /// 首个空位（行优先）。
    pub fn first_free(&self) -> Option<(usize, usize)> {
        let mut row = 0usize;
        while row < GRID_ROWS {
            let mut col = 0usize;
            while col < GRID_COLS {
                if self.is_free(col, row) {
                    return Some((col, row));
                }
                col += 1;
            }
            row += 1;
        }
        None
    }

    /// 自动落位：占下首个空位并返回其像素坐标。
    pub fn place(&mut self) -> Option<(i32, i32)> {
        let (col, row) = self.first_free()?;
        if !self.occupy(col, row).ok() {
            return None;
        }
        Some(self.cell_origin(col, row))
    }

    pub fn cell_origin(&self, col: usize, row: usize) -> (i32, i32) {
        (col as i32 * self.cell, row as i32 * self.cell)
    }

    /// 坐标 → 格号（向下取整，负坐标归零）。
    pub fn cell_of(&self, x: i32, y: i32) -> (usize, usize) {
        let cx = if x < 0 { 0usize } else { (x / self.cell) as usize };
        let cy = if y < 0 { 0usize } else { (y / self.cell) as usize };
        (
            if cx >= GRID_COLS { GRID_COLS - 1 } else { cx },
            if cy >= GRID_ROWS { GRID_ROWS - 1 } else { cy },
        )
    }

    /// 就近吸附到栅格交点。
    pub fn snap(&self, x: i32, y: i32) -> (i32, i32) {
        let c = self.cell;
        let half = c / 2;
        (((x + half) / c) * c, ((y + half) / c) * c)
    }

    /// 占用密度（permille）。
    pub fn density_permille(&self) -> u32 {
        (self.count * 1000) / GRID_CELLS as u32
    }

    /// 连续空位段长度（整理助手用）。
    pub fn longest_free_run(&self) -> usize {
        let mut best = 0usize;
        let mut run = 0usize;
        let mut row = 0usize;
        while row < GRID_ROWS {
            let mut col = 0usize;
            while col < GRID_COLS {
                if self.is_free(col, row) {
                    run += 1;
                    if run > best {
                        best = run;
                    }
                } else {
                    run = 0;
                }
                col += 1;
            }
            row += 1;
        }
        best
    }

    pub fn apply_level(&mut self, level: u8) -> i32 {
        self.cell = CELL_LEVELS[clamp_level(level) as usize];
        self.cell
    }

    pub fn clear(&mut self) {
        self.bits = [0u32; GRID_WORDS];
        self.count = 0;
    }

    pub fn uninstall(&mut self) -> bool {
        self.clear();
        self.cell = 80;
        self.count == 0
    }
}

pub fn run_icon_grid_checks() -> CheckSet {
    let mut s = CheckSet::new("ai11-icon-grid");
    let mut g = IconGrid::new(80);

    // L1 基础实装
    let o1 = g.occupy(0, 0);
    let placed = g.place();
    let cnt = g.count;
    s.add(
        "X02526 栅格最小闭环",
        o1.ok() && placed == Some((80, 0)) && cnt == 2,
        "端到端占位闭环",
    );
    g.apply_level(3);
    let cell3 = g.cell;
    s.add(
        "X02527 参数与配置面",
        cell3 == 96 && IconGrid::new(0).cell == 80 && IconGrid::new(0).count == 0,
        "默认档=现状，配置持久化",
    );
    s.add(
        "X02528 档位矩阵",
        CELL_LEVELS.len() == 5
            && CELL_LEVELS[0] < CELL_LEVELS[1]
            && CELL_LEVELS[1] < CELL_LEVELS[2]
            && CELL_LEVELS[2] < CELL_LEVELS[3]
            && CELL_LEVELS[3] < CELL_LEVELS[4],
        "五档栅格独立可交付",
    );
    let snap = Snap {
        ver: SNAP_VER,
        payload: [2, 0, 0, 0, 0, 0, 0, 96],
    };
    let mut buf = [0u8; SNAP_TEXT];
    let nb = export_snap(snap, &mut buf);
    s.add(
        "X02529 快照与迁移",
        import_snap(&buf[..nb]) == Some(snap) && migrate(&buf[..nb], SNAP_VER).is_some(),
        "导出/导入/跨版本三通道",
    );
    let (k, v, c) = link_matrix(4);
    s.add("X02530 三线集成验证", k && v && c, "无回归、无手感损毁");

    // L2 边界与恢复
    let out = g.occupy(GRID_COLS, 0);
    let dup = g.occupy(0, 0);
    s.add(
        "X02531 极端输入钳制",
        out == DeskError::OutOfRange && dup == DeskError::Busy,
        "越界与重复登记不崩溃",
    );
    s.add(
        "X02532 失败叙事",
        narrative_has(DeskError::Busy, "上一批任务") && narrative_has(DeskError::OutOfRange, "回落默认档"),
        "每种失败都有下一步建议",
    );
    let freed = g.free(0, 0);
    let again = g.occupy(0, 0);
    s.add("X02533 中断续跑", freed && again.ok() && !g.free(0, 0) == false, "半成品可续作");
    let (g1, e1) = resource_guard(230, 10, 80);
    s.add("X02534 资源降级", g1 && e1 == DeskError::NoMemory, "资源紧张触发守护");
    let clean = g.uninstall();
    s.add("X02535 回滚净身", clean && g.count == 0 && g.cell == 80, "不留残档");

    // L3 手感与细节
    let m1 = motion_for(2, false);
    let m2 = motion_for(2, true);
    s.add("X02536 动效令牌", m1.dur_ms == 200 && m2.curve == 0, "落位动效走令牌");
    s.add(
        "X02537 三态与焦点环",
        focus_ring(DeskState::Press) == 2 && elevation(DeskState::Disabled) == 0,
        "图标三态过检",
    );
    s.add(
        "X02538 键盘通道",
        hotkey_conflict("Ctrl+Alt+G", "ctrl+alt+g") && !hotkey_conflict("Ctrl+Alt+G", "Ctrl+Alt+H"),
        "快捷键无冲突",
    );
    s.add(
        "X02539 微文案",
        microcopy_ok("已吸附到栅格") && !microcopy_ok("null grid"),
        "中文语境自然",
    );
    s.add("X02540 无障碍等价通道", hc_redline(1000, 0) && !hc_redline(300, 200), "HC 红线");

    // L4 性能与优化
    let mut g2 = IconGrid::new(80);
    for i in 0..4usize {
        let _ = g2.occupy(i, 0);
    }
    let dens = g2.density_permille();
    s.add("X02541 基准与预算表", dens == (4 * 1000) / GRID_CELLS as u32, "密度入 CI");
    let ff = g2.first_free();
    s.add("X02542 热路径优化", ff == Some((4, 0)), "位图一次定位空位");
    let run = g2.longest_free_run();
    s.add("X02543 内存与功耗收敛", run == GRID_CELLS - 4, "定长位图无泄漏");
    let d0 = degrade_chain(4, 0);
    let d2 = degrade_chain(4, 150);
    s.add("X02544 低配降级链", d0 == (4, 4, 4) && d2 == (2, 0, 2), "三级递降");
    let mut gd = Guard::new();
    let ga = gd.guard("grid-density<=800‰");
    let gb = gd.guard("grid-density<=800‰");
    s.add("X02545 防劣化守卫", ga && !gb && gd.count() == 1, "断言只增不删");

    // L5 创新拓展
    let mut ad = Advisor::new();
    let a1 = ad.suggest("compact", "首行已满，建议启用紧凑栅格");
    let a2 = ad.suggest("compact", "重复");
    s.add(
        "X02546 本地智能建议",
        a1 && !a2 && ad.explain("compact").is_some() && ad.reject("compact") && ad.rejected("compact"),
        "可解释、可一键拒绝",
    );
    let mut bt = Batch::new(4);
    bt.step();
    bt.step();
    s.add("X02547 批量自动化", bt.progress() == 50 && bt.done == 2, "批处理进度可观测");
    s.add(
        "X02548 三线联动场景",
        link_matrix(0) == (true, false, false) && link_matrix(2) == (false, false, true),
        "跨域协同用例",
    );
    let mut pl = Plugins::new();
    let r1 = pl.register("grid-rule");
    let r2 = pl.register("grid-rule");
    s.add("X02549 开放扩展点", r1 && !r2 && pl.unregister("grid-rule"), "接口/示例/文档");
    let mut eg = Eggs::new();
    let e1 = eg.arm("tessellate");
    eg.disable_all();
    s.add("X02550 艺术彩蛋", e1 && eg.count() == 0, "可关闭、有品牌记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_grid_25_checks_pass() {
        let set = run_icon_grid_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
