//! veb285_mlitile —— VE-F0285 Mali Tiling 渲染模型感知。
//! 锚点（CoRun Unxreal 主册）：VE-F0285 · Mali Tiling 渲染模型感知 · 目标 460 行
//! 判据原文：VE-F0285-J1 Tiling 感知账：绘制清单登记（三角 /
//!           矩形两类，各带包围盒）；分块网格（块尺寸 16×16
//!           固定）；包围盒分块归属（盒覆盖的所有块号闭集）；
//!           块内清单（块 → 命中它的绘制号集，去重）；剔除
//!           器（零命中块剔除计数）；binned 序核（块遍历行
//!           优先，块内清单按登记序）；重登去重核（同绘制同
//!           块只记一次）；覆盖率（命中块 / 总块）。
//! 组件：绘制账 + 分块器 + 块内清单 + 剔除器。
//! 职责定位：Tiling 感知是「Mali 的分地术」——Binning 是 Mali
//!           的立命之本：先把地分块再按块派活，一遍过而不是
//!           全屏过；块内清单是派工单：命中才派，零命中块直
//!           接免工；去重是派工单的防重章：同一活派两次是浪
//!           费也是错。
//! 性能逐项分解：分块 O(盒面积/块)；清单 O(命中)。
//! 诊断码段：0x31040（0x31041~0x31049 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const TL_VERSION: &str = "veb285_mlitile-v1";

/// 诊断段基址（0x31040 段 · 本模块独占）。
pub const TL_SEG_BASE: u32 = 0x31040;
pub const XC_MAGIC: u32 = 0x31041; // 账魔数破
pub const XC_BOX: u32 = 0x31042; // 包围盒不合法
pub const XC_DUP: u32 = 0x31043; // 绘制号重复
pub const XC_NOITEM: u32 = 0x31044; // 无此绘制

/// 账魔数。
const MAGIC: u32 = 0x3104_1BE9;

/// 块尺寸（16×16）。
pub const TILE: u32 = 16;

/// 网格尺寸（32×32 块 → 512×512 像素）。
pub const GRID_W: u32 = 32;
pub const GRID_H: u32 = 32;

/// 绘制类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prim {
    Tri,
    Rect,
}

/// 绘制记录。
pub struct DrawRec {
    pub id: u32,
    pub kind: Prim,
    /// 包围盒（x0,y0)-(x1,y1]，半开。
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

/// Tiling 簿。
pub struct TileBook {
    draws: Vec<DrawRec>,
    /// 块内清单：block_idx → 绘制号序（去重）。
    bins: Vec<Vec<u32>>,
    /// 零命中块计数。
    pub culled: u32,
    /// 块遍历游标。
    cursor: usize,
    magic: u32,
}

impl TileBook {
    /// 建。
    pub fn new() -> TileBook {
        let mut bins = Vec::new();
        for _ in 0..(GRID_W * GRID_H) as usize {
            bins.push(Vec::new());
        }
        TileBook { draws: Vec::new(), bins, culled: 0, cursor: 0, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 登记绘制：盒合法 + 号不重。
    pub fn add(&mut self, id: u32, kind: Prim, x0: u32, y0: u32, x1: u32, y1: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if x1 <= x0 || y1 <= y0 {
            return Err(XC_BOX);
        }
        if x1 > GRID_W * TILE || y1 > GRID_H * TILE {
            return Err(XC_BOX);
        }
        for d in self.draws.iter() {
            if d.id == id {
                return Err(XC_DUP);
            }
        }
        self.draws.push(DrawRec { id, kind, x0, y0, x1, y1 });
        Ok(())
    }

    /// 盒覆盖块号闭集。
    pub fn blocks_of(x0: u32, y0: u32, x1: u32, y1: u32) -> Vec<u32> {
        let bx0 = x0 / TILE;
        let bx1 = (x1 - 1) / TILE;
        let by0 = y0 / TILE;
        let by1 = (y1 - 1) / TILE;
        let mut out = Vec::new();
        let mut by = by0;
        while by <= by1 {
            let mut bx = bx0;
            while bx <= bx1 {
                out.push(by * GRID_W + bx);
                bx += 1;
            }
            by += 1;
        }
        out
    }

    /// Binning：全绘制按盒分块；同绘制同块去重。
    pub fn binning(&mut self) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        // 清旧清单重分。
        for b in self.bins.iter_mut() {
            b.clear();
        }
        self.culled = 0;
        let mut pairs = 0u32;
        for d in self.draws.iter() {
            let blocks = Self::blocks_of(d.x0, d.y0, d.x1, d.y1);
            for blk in blocks.iter() {
                let bi = *blk as usize;
                // 去重。
                let mut seen = false;
                for e in self.bins[bi].iter() {
                    if *e == d.id {
                        seen = true;
                    }
                }
                if !seen {
                    self.bins[bi].push(d.id);
                    pairs += 1;
                }
            }
        }
        // 零命中块剔除计数。
        let mut zero = 0u32;
        for b in self.bins.iter() {
            if b.is_empty() {
                zero += 1;
            }
        }
        self.culled = zero;
        Ok(pairs)
    }

    /// 查块清单。
    pub fn bin_of(&self, blk: u32) -> Result<&Vec<u32>, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if blk >= GRID_W * GRID_H {
            return Err(XC_NOITEM);
        }
        Ok(&self.bins[blk as usize])
    }

    /// 覆盖率（命中块 × 1000 / 总块，毫分比）。
    pub fn coverage_milli(&self) -> u32 {
        let total = (GRID_W * GRID_H) as u32;
        let hit = total - self.culled;
        hit * 1000 / total
    }

    /// binned 遍历：行优先出（blk, draw_id）对。
    pub fn walk(&mut self) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        while self.cursor < self.bins.len() {
            let bi = self.cursor;
            let n = self.bins[bi].len();
            let mut k = 0;
            while k < n {
                out.push((bi as u32, self.bins[bi][k]));
                k += 1;
            }
            self.cursor += 1;
        }
        out
    }

    /// 绘制数。
    pub fn draw_count(&self) -> usize {
        self.draws.len()
    }
}

pub fn run_veb285_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0285");
    // 族零：盒配置。
    let mut b = TileBook::new();
    s.add("零宽拒", b.add(1, Prim::Tri, 10, 10, 10, 20) == Err(XC_BOX), "x1<=x0");
    s.add("逆盒拒", b.add(1, Prim::Tri, 20, 10, 10, 20) == Err(XC_BOX), "逆序");
    s.add("越界拒", b.add(1, Prim::Tri, 0, 0, 513, 10) == Err(XC_BOX), ">512");
    // 族一：登记。
    s.add("登 1", b.add(1, Prim::Rect, 0, 0, 16, 16).is_ok(), "恰一块");
    s.add("重号拒", b.add(1, Prim::Tri, 0, 0, 8, 8) == Err(XC_DUP), "号重");
    s.add("登 2", b.add(2, Prim::Tri, 8, 8, 32, 32).is_ok(), "跨块");
    s.add("绘制 2", b.draw_count() == 2, "两笔");
    // 族二：块归属闭集。
    let bl = TileBook::blocks_of(0, 0, 16, 16);
    s.add("一盒一块", bl.len() == 1 && bl[0] == 0, "(0,0)-(16,16)");
    let bl2 = TileBook::blocks_of(8, 8, 32, 32);
    s.add("跨块 4", bl2.len() == 4, "块 0,1,32,33");
    s.add("首块 0", bl2[0] == 0, "左上");
    s.add("末块 33", bl2[3] == 33, "右下");
    // 行优先序。
    s.add("序行优先", bl2[1] == 1 && bl2[2] == 32, "先横后纵");
    // 族三：binning 与去重。
    let pairs = b.binning();
    s.add("对数 5", pairs == Ok(5), "1 块 + 4 块");
    // 块 0 清单 [1, 2]（1 先登）。
    s.add("块 0 两绘", matches!(b.bin_of(0), Ok(v) if v.len() == 2 && v[0] == 1 && v[1] == 2), "按登记序");
    s.add("块 33 独绘", matches!(b.bin_of(33), Ok(v) if v.len() == 1 && v[0] == 2), "只 2");
    // 覆盖率：命中 5 / 1024。
    s.add("覆盖毫分 4", b.coverage_milli() == (5 * 1000) / 1024, "小覆盖");
    // 族四：剔除器。
    s.add("剔除 1019", b.culled == 1024 - 5, "零命中块");
    // 全屏一笔 → 全命中。
    let mut f = TileBook::new();
    let _ = f.add(9, Prim::Rect, 0, 0, 512, 512);
    let _ = f.binning();
    s.add("全屏零剔除", f.culled == 0, "全命中");
    s.add("全屏覆盖满", f.coverage_milli() == 1000, "100%");
    // 重 binning 幂等。
    let _ = b.binning();
    s.add("重分幂等", b.culled == 1024 - 5, "同果");
    // 族五：binned 序核。
    let mut w = TileBook::new();
    let _ = w.add(1, Prim::Rect, 0, 0, 16, 16);   // 块 0
    let _ = w.add(2, Prim::Rect, 16, 0, 32, 16);  // 块 1
    let _ = w.add(3, Prim::Rect, 0, 16, 16, 32);  // 块 32
    let _ = w.binning();
    let walk = w.walk();
    s.add("遍历 3 对", walk.len() == 3, "三块三对");
    s.add("序 0,1,32", walk[0].0 == 0 && walk[1].0 == 1 && walk[2].0 == 32, "行优先");
    s.add("遍历尽", w.walk().is_empty(), "游标到底");
    s.add("魔数核", TL_SEG_BASE == 0x31040, "段基址");
    s
}
