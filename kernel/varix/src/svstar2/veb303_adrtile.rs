//! veb303_adrtile —— VE-F0303 Adreno tiling 模型感知。
//! 锚点（CoRun Unxreal 主册）：VE-F0303 · Adreno tiling 模型感知 · 目标 320 行
//! 判据原文：VE-F0303-J1 Adreno 分块账：LRZ 预处理账（深度
//!           预剔除：块级最深深度缓存）；块深度账（每块记录
//!           已写最深层值）；快速拒核（新片元块内深度 ≥ 缓
//!           深度 → 整块拒写，被已写更近者挡）；深度回退核
//!           （块被清 → 缓深度重置未写态）；LRZ 命中率（拒
//!           写数 / 总测试数）；方向核（近到远绘制 → 命中
//!           率高于远到近）；块界核（块号越界拒）。
//! 组件：LRZ 缓存 + 快速拒 + 命中率账 + 方向对比。
//! 职责定位：Adreno 分块是「高通的省电术」——LRZ 是门卫：
//!           缓存记住块内已写的最深（最远）深度，比它更远的
//!           片元连门都不进，功耗省在没启动的部分；近到远绘
//!           制是门卫的最爱：最近的先站住，后面的全被拒——
//!           顺序即性能。
//! 性能逐项分解：测试 O(1)；清块 O(1)。
//! 诊断码段：0x314C0（0x314C1~0x314C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const LT_VERSION: &str = "veb303_adrtile-v1";

/// 诊断段基址（0x314C0 段 · 本模块独占）。
pub const LT_SEG_BASE: u32 = 0x314C0;
pub const XC_MAGIC: u32 = 0x314C1; // 账魔数破
pub const XC_BLK: u32 = 0x314C2; // 块号越界

/// 账魔数。
const MAGIC: u32 = 0x314C_1B94;

/// 块网格（16×16 块）。
pub const GRID: u32 = 16;

/// LRZ 账。
pub struct LrzBook {
    /// 每块已写最深层值（越小越近；拒写条件：已写且新值 ≥ 缓值）。
    depth: [u32; (GRID * GRID) as usize],
    /// 块已写旗。
    written: [bool; (GRID * GRID) as usize],
    /// 拒写数。
    pub rejected: u32,
    /// 总测试数。
    pub tested: u32,
    magic: u32,
}

impl LrzBook {
    /// 建：全块未写。
    pub fn new() -> LrzBook {
        LrzBook {
            depth: [0; (GRID * GRID) as usize],
            written: [false; (GRID * GRID) as usize],
            rejected: 0,
            tested: 0,
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 快速拒测试：Ok(true) = 被拒（被更近已写者挡）。
    pub fn test(&mut self, blk: u32, z: u32) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if blk >= GRID * GRID {
            return Err(XC_BLK);
        }
        self.tested += 1;
        let i = blk as usize;
        if self.written[i] && z >= self.depth[i] {
            // 比块内已写最远者还远（或等远）→ 必被挡 → 拒。
            self.rejected += 1;
            return Ok(true);
        }
        // 通过：更新块缓存为更远方向的最大值。
        if !self.written[i] || z > self.depth[i] {
            self.depth[i] = z;
        }
        self.written[i] = true;
        Ok(false)
    }

    /// 深度回退：清块 → 未写态。
    pub fn clear_blk(&mut self, blk: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if blk >= GRID * GRID {
            return Err(XC_BLK);
        }
        self.depth[blk as usize] = 0;
        self.written[blk as usize] = false;
        Ok(())
    }

    /// 清全屏。
    pub fn clear_all(&mut self) {
        for i in 0..(GRID * GRID) as usize {
            self.depth[i] = 0;
            self.written[i] = false;
        }
    }

    /// 查块缓存深度。
    pub fn depth_of(&self, blk: u32) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if blk >= GRID * GRID {
            return Err(XC_BLK);
        }
        Ok(self.depth[blk as usize])
    }

    /// 查块已写旗。
    pub fn written_of(&self, blk: u32) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if blk >= GRID * GRID {
            return Err(XC_BLK);
        }
        Ok(self.written[blk as usize])
    }

    /// 命中率（毫分比）。
    pub fn hit_milli(&self) -> u32 {
        if self.tested == 0 {
            return 0;
        }
        self.rejected * 1000 / self.tested
    }
}

pub fn run_veb303_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0303");
    // 族零：块界。
    let mut b = LrzBook::new();
    s.add("块 256 拒", b.test(256, 100) == Err(XC_BLK), "越界");
    s.add("清 256 拒", b.clear_blk(256) == Err(XC_BLK), "越界");
    s.add("查 256 拒", b.depth_of(256) == Err(XC_BLK), "越界");
    // 族一：快速拒核（缓存存已写最远，拒更远）。
    s.add("初态过", b.test(0, 0x1000) == Ok(false), "未写不拒");
    s.add("缓存更新", b.depth_of(0) == Ok(0x1000), "记远");
    // 更远（更大）值被拒。
    s.add("更远被拒", b.test(0, 0x8000) == Ok(true), "≥ 缓值");
    // 更近（更小）值通过，缓存保持最远。
    s.add("更近通过", b.test(0, 0x0800) == Ok(false), "< 缓值");
    s.add("缓保持远", b.depth_of(0) == Ok(0x1000), "不回退");
    // 等值拒。
    s.add("等值拒", b.test(0, 0x1000) == Ok(true), "等远挡");
    // 族二：深度回退。
    s.add("清块", b.clear_blk(0).is_ok(), "回退");
    s.add("清后未写", b.depth_of(0) == Ok(0) && b.written_of(0) == Ok(false), "重置");
    s.add("清后可过", b.test(0, 0x8000) == Ok(false), "重新起算");
    // 族三：命中率账。
    let c = LrzBook::new();
    s.add("零测零率", c.hit_milli() == 0, "无分母");
    // 近到远（z 递增 1..100）：首片过，其余全拒 → 99 拒。
    let mut near = LrzBook::new();
    for i in 0..100u32 {
        let _ = near.test(5, i + 1);
    }
    s.add("近序 99 拒", near.rejected == 99, "首片外全拒");
    s.add("近序率 990", near.hit_milli() == 990, "99/100");
    // 远到近（z 递减 100..1）：全过 → 0 拒。
    let mut far = LrzBook::new();
    for i in 0..100u32 {
        let _ = far.test(5, 100 - i);
    }
    s.add("远序零拒", far.rejected == 0, "更近不拒");
    // 方向核：近到远比远到近命中高。
    s.add("方向核", near.hit_milli() > far.hit_milli(), "近先优");
    // 族四：多块独立。
    let mut m = LrzBook::new();
    let _ = m.test(0, 0x1000);
    let _ = m.test(1, 0xF000);
    s.add("块 0 缓近", m.depth_of(0) == Ok(0x1000), "独立");
    s.add("块 1 缓远", m.depth_of(1) == Ok(0xF000), "独立");
    // 块 0 上写更远值（0xF000 ≥ 0x1000）应拒——块独立不串。
    s.add("块 0 远值拒", m.test(0, 0xF000) == Ok(true), "独立缓存");
    // 清全屏。
    m.clear_all();
    s.add("全清未写", m.written_of(0) == Ok(false) && m.written_of(1) == Ok(false), "两块");
    // 族五：计数核。
    let mut n = LrzBook::new();
    let _ = n.test(2, 10);  // 过
    let _ = n.test(2, 20);  // 拒 1（更远）
    let _ = n.test(2, 5);   // 过（更近）
    let _ = n.test(2, 6);   // 拒 2
    s.add("测 4", n.tested == 4, "计数");
    s.add("拒 2", n.rejected == 2, "计数");
    s.add("率 500", n.hit_milli() == 500, "2/4");
    s.add("魔数核", LT_SEG_BASE == 0x314C0, "段基址");
    s
}
