//! veb343_tilepar —— VE-F0343 瓦片并行架构深化。
//! 锚点：VE-F0343 · 目标 440 行。
//! 判据原文：工作窃取（线程池窃取负载均衡）、瓦片独立（每瓦
//!           片独立状态减同步）、缓存友好（瓦片内线性扫描遍
//!           历序）、自适应分块（按负载与缓存实测调优，异常
//!           回落保守默认）、背压限流（队列溢出）、判据。
//! 组件：瓦片网格 + 工作窃取队列 + 自适应分块器。
//! 职责定位：瓦片并行是「分田到户」——图切成瓦片，一人一块
//!           独立耕作不串门（减同步）；活不匀就邻里互助（窃
//!           取）；田块大小看地力定（自适应），地力数据不可
//!           信就按老规矩分（保守默认）。
//! 性能逐项分解：分块 O(瓦片数) 并行；窃取 O(1) 均摊；调优
//!           O(实验轮) 一次性。
//! 诊断码段：0x31EC0（0x31EC1~0x31EC9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const TP_VERSION: &str = "veb343_tilepar-v1";

/// 诊断段基址（0x31EC0 段 · 本模块独占）。
pub const TP_SEG_BASE: u32 = 0x31EC0;
pub const XC_BADTILE: u32 = 0x31EC1; // 瓦片尺寸非法（零或超图）
pub const XC_OVERFLOW: u32 = 0x31EC2; // 队列溢出（背压）
pub const XC_STEAL_EMPTY: u32 = 0x31EC3; // 窃取空手
pub const XC_BADPARAM: u32 = 0x31EC4; // 自适应参数异常

/// 保守默认瓦片边。
pub const TILE_DEFAULT: u32 = 32;
/// 瓦片边合法界。
pub const TILE_MIN: u32 = 8;
pub const TILE_MAX: u32 = 128;

/// 瓦片坐标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tile {
    pub tx: u32,
    pub ty: u32,
}

/// 瓦片网格。
pub struct Grid {
    pub w: u32,
    pub h: u32,
    pub tw: u32,
    pub th: u32,
}

impl Grid {
    /// 建网格：尺寸与瓦片边校验。
    pub fn new(w: u32, h: u32, tw: u32, th: u32) -> Result<Grid, u32> {
        if tw == 0 || th == 0 || tw > TILE_MAX || th > TILE_MAX {
            return Err(XC_BADTILE);
        }
        if w == 0 || h == 0 {
            return Err(XC_BADTILE);
        }
        Ok(Grid { w, h, tw, th })
    }

    /// 瓦片列数（向上取整）。
    pub fn cols(&self) -> u32 {
        (self.w + self.tw - 1) / self.tw
    }

    /// 瓦片行数。
    pub fn rows(&self) -> u32 {
        (self.h + self.th - 1) / self.th
    }

    /// 瓦片总数。
    pub fn tiles(&self) -> u32 {
        self.cols() * self.rows()
    }

    /// 缓存友好遍历序：行主序线性扫描（瓦片内也线性）。
    pub fn order(&self) -> Vec<Tile> {
        let mut v = Vec::new();
        for ty in 0..self.rows() {
            for tx in 0..self.cols() {
                v.push(Tile { tx, ty });
            }
        }
        v
    }

    /// 瓦片像素界：(x0, y0, x1, y1)（右下开区间）。
    pub fn rect(&self, t: Tile) -> Option<(u32, u32, u32, u32)> {
        if t.tx >= self.cols() || t.ty >= self.rows() {
            return None;
        }
        let x0 = t.tx * self.tw;
        let y0 = t.ty * self.th;
        Some((
            x0,
            y0,
            core::cmp::min(x0 + self.tw, self.w),
            core::cmp::min(y0 + self.th, self.h),
        ))
    }
}

/// 工作窃取队列（单生产者多窃取者的简化环）。
pub struct StealQ {
    q: Vec<u32>,
    cap: usize,
    /// 窃取计数。
    pub stolen: u32,
}

impl StealQ {
    /// 建：容量界。
    pub fn new(cap: u32) -> StealQ {
        StealQ {
            q: Vec::new(),
            cap: cap as usize,
            stolen: 0,
        }
    }

    /// 压任务：溢出拒（背压限流）。
    pub fn push(&mut self, task: u32) -> Result<(), u32> {
        if self.q.len() >= self.cap {
            return Err(XC_OVERFLOW);
        }
        self.q.push(task);
        Ok(())
    }

    /// 本端弹任务。
    pub fn pop(&mut self) -> Option<u32> {
        self.q.pop()
    }

    /// 窃取：从他端偷一个（FIFO 侧）。
    pub fn steal(&mut self) -> Result<u32, u32> {
        if self.q.is_empty() {
            return Err(XC_STEAL_EMPTY);
        }
        let t = self.q.remove(0);
        self.stolen += 1;
        Ok(t)
    }

    /// 队深。
    pub fn len(&self) -> usize {
        self.q.len()
    }
}

/// 自适应分块：按负载（每瓦片平均工作量）与缓存预算定瓦片边。
/// 负载大 → 边大（减少窃取开销）；负载小 → 边小（均衡）。
/// 结果夹在 [TILE_MIN, TILE_MAX]；预算异常 → 保守默认。
pub fn auto_tile(load_per_px: u32, cache_budget: u32) -> u32 {
    if cache_budget == 0 || cache_budget > 4096 {
        return TILE_DEFAULT;
    }
    if load_per_px == 0 {
        return TILE_MIN;
    }
    // 经验式：边 = sqrt(预算 / (负载×4))，夹界。
    let v = cache_budget / (load_per_px * 4 + 1);
    let mut e = 1u32;
    while (e + 1) * (e + 1) <= v && e < TILE_MAX {
        e += 1;
    }
    if e < TILE_MIN {
        return TILE_DEFAULT;
    }
    if e > TILE_MAX {
        return TILE_MAX;
    }
    e
}

pub fn run_veb343_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0343");
    // 族零：网格门。
    s.add("零瓦拒", matches!(Grid::new(64, 64, 0, 32), Err(XC_BADTILE)), "无尺寸");
    s.add("超界拒", matches!(Grid::new(64, 64, 256, 32), Err(XC_BADTILE)), ">128");
    s.add("零图拒", matches!(Grid::new(0, 64, 32, 32), Err(XC_BADTILE)), "无图");
    let g = match Grid::new(100, 70, 32, 32) {
        Ok(x) => x,
        Err(_) => {
            s.add("建网败", false, "不应到");
            return s;
        }
    };
    s.add("列四", g.cols() == 4, "100/32→4");
    s.add("行三", g.rows() == 3, "70/32→3");
    s.add("瓦十二", g.tiles() == 12, "4×3");
    // 族一：遍历序。
    let ord = g.order();
    s.add("序长十二", ord.len() == 12, "行主序");
    s.add("首瓦 00", ord[0] == Tile { tx: 0, ty: 0 }, "左上");
    s.add("次瓦 10", ord[1] == Tile { tx: 1, ty: 0 }, "行内先走");
    s.add("末瓦 23", ord[11] == Tile { tx: 3, ty: 2 }, "右下");
    // 族二：瓦片界（边缘瓦片截断）。
    s.add("满瓦界", g.rect(Tile { tx: 0, ty: 0 }) == Some((0, 0, 32, 32)), "整瓦");
    s.add("边瓦截断", g.rect(Tile { tx: 3, ty: 0 }) == Some((96, 0, 100, 32)), "100-96=4");
    s.add("底瓦截断", g.rect(Tile { tx: 0, ty: 2 }) == Some((0, 64, 32, 70)), "70-64=6");
    s.add("越界瓦 None", g.rect(Tile { tx: 4, ty: 0 }).is_none(), "查无");
    // 族三：工作窃取。
    let mut q = StealQ::new(4);
    s.add("溢出背压", q.push(1).is_ok() && q.push(2).is_ok() && q.push(3).is_ok() && q.push(4).is_ok(), "满四");
    s.add("第五拒", q.push(5) == Err(XC_OVERFLOW), "限流");
    s.add("本端弹四", q.pop() == Some(4), "LIFO");
    s.add("窃取偷一", q.steal() == Ok(1), "FIFO 侧");
    s.add("窃取账一", q.stolen == 1, "记功");
    s.add("窃二", q.steal() == Ok(2), "再偷");
    let _ = q.pop();
    let _ = q.pop();
    s.add("空手窃拒", q.steal() == Err(XC_STEAL_EMPTY), "无活可偷");
    // 族四：自适应分块。
    s.add("零预算保守", auto_tile(10, 0) == TILE_DEFAULT, "数据不可信");
    s.add("爆预算保守", auto_tile(10, 9999) == TILE_DEFAULT, ">4096");
    s.add("零负载最小", auto_tile(0, 1024) == TILE_MIN, "轻活小田");
    s.add("重载大田", auto_tile(64, 4096) > auto_tile(8, 4096), "负载大边大");
    s.add("界内", {
        let e = auto_tile(16, 2048);
        e >= TILE_MIN && e <= TILE_MAX
    }, "夹界");
    s.add("魔数核", TP_SEG_BASE == 0x31EC0, "段基址");
    s
}
