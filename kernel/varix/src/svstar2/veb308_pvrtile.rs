//! veb308_pvrtile —— VE-F0308 PowerVR tiling 模型研究。
//! 锚点（CoRun Unxreal 主册）：VE-F0308 · PowerVR tiling 模型研究 · 目标 320 行
//! 判据原文：VE-F0308-J1 PVR 分块账：ISP 分块（Image Synthesis
//!           Processor 块级深度合成）；HZR 账（层级 Z 拒绝：
//!           块四分区粗粒度）；区域账（tile 区域划分：区域
//!           尺寸 32×32 固定）；深度合并核（区域合并 → 区内
//!           最浅者胜出即区域代表深度）；粗拒核（片元深度
//!           ≥ 区域代表 → 粗拒）；HZR 命中率；区域清核。
//! 组件：区域账 + 深度合并 + 粗拒 + 命中率。
//! 职责定位：PVR 分块是「IMG 的深度炼金」——ISP 是炼金炉：
//!           深度不是后知后觉是块内先合成；HZR 是粗筛：四分
//!           区合并出代表深度，比代表远的整批出局；区域代表
//!           是粗筛的尺子：尺子准不准决定错杀多少。
//! 性能逐项分解：粗拒 O(1)。
//! 诊断码段：0x31600（0x31601~0x31609 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const PT_VERSION: &str = "veb308_pvrtile-v1";

/// 诊断段基址（0x31600 段 · 本模块独占）。
pub const PT_SEG_BASE: u32 = 0x31600;
pub const XC_MAGIC: u32 = 0x31601; // 账魔数破
pub const XC_REG: u32 = 0x31602; // 区域号越界

/// 账魔数。
const MAGIC: u32 = 0x3160_1B5E;

/// 区域网格（8×8）。
pub const GRID: u32 = 8;

/// 区域记录：四分区深度（象限 0~3，越小越近）+ 已写旗。
#[derive(Clone, Copy)]
pub struct Quad {
    pub d: [u32; 4],
    pub written: [bool; 4],
}

impl Quad {
    /// 空象限。
    fn new() -> Quad {
        Quad { d: [0; 4], written: [false; 4] }
    }

    /// 区域代表深度 = 已写象限的最浅值（min）。
    fn rep(&self) -> Option<u32> {
        let mut best: Option<u32> = None;
        for i in 0..4 {
            if self.written[i] {
                best = match best {
                    None => Some(self.d[i]),
                    Some(b) => {
                        if self.d[i] < b {
                            Some(self.d[i])
                        } else {
                            Some(b)
                        }
                    }
                };
            }
        }
        best
    }
}

/// HZR 账。
pub struct HzrBook {
    regs: [Quad; (GRID * GRID) as usize],
    pub rejected: u32,
    pub tested: u32,
    magic: u32,
}

impl HzrBook {
    /// 建。
    pub fn new() -> HzrBook {
        HzrBook {
            regs: [Quad::new(); (GRID * GRID) as usize],
            rejected: 0,
            tested: 0,
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 记象限深度（合并核：更近者覆盖象限）。
    pub fn write_quad(&mut self, reg: u32, q: u32, z: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if reg >= GRID * GRID || q >= 4 {
            return Err(XC_REG);
        }
        let r = &mut self.regs[reg as usize];
        if !r.written[q as usize] || z < r.d[q as usize] {
            r.d[q as usize] = z;
        }
        r.written[q as usize] = true;
        Ok(())
    }

    /// 区域代表深度。
    pub fn rep_depth(&self, reg: u32) -> Result<Option<u32>, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if reg >= GRID * GRID {
            return Err(XC_REG);
        }
        Ok(self.regs[reg as usize].rep())
    }

    /// 粗拒测试：深度 ≥ 区域代表 → 粗拒（被区域内更近者挡）。
    pub fn test(&mut self, reg: u32, z: u32) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if reg >= GRID * GRID {
            return Err(XC_REG);
        }
        self.tested += 1;
        match self.regs[reg as usize].rep() {
            None => Ok(false), // 空区域不拒
            Some(rp) => {
                if z >= rp {
                    self.rejected += 1;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    }

    /// 区域清。
    pub fn clear(&mut self, reg: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if reg >= GRID * GRID {
            return Err(XC_REG);
        }
        self.regs[reg as usize] = Quad::new();
        Ok(())
    }

    /// 命中率（毫分比）。
    pub fn hit_milli(&self) -> u32 {
        if self.tested == 0 {
            return 0;
        }
        self.rejected * 1000 / self.tested
    }
}

pub fn run_veb308_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0308");
    let mut h = HzrBook::new();
    // 族零：越界。
    s.add("区 64 拒", h.write_quad(64, 0, 1) == Err(XC_REG), "越界");
    s.add("象限 4 拒", h.write_quad(0, 4, 1) == Err(XC_REG), "无四象限外");
    s.add("测 64 拒", h.test(64, 1) == Err(XC_REG), "越界");
    // 族一：深度合并核。
    let _ = h.write_quad(0, 0, 100);
    let _ = h.write_quad(0, 1, 50);
    let _ = h.write_quad(0, 2, 75);
    // 象限 3 未写。代表 = 最浅已写 = 50。
    s.add("代表 50", h.rep_depth(0) == Ok(Some(50)), "min 合并");
    // 同象限更近覆盖。
    let _ = h.write_quad(0, 0, 30);
    s.add("覆盖后代表 30", h.rep_depth(0) == Ok(Some(30)), "更近胜");
    // 同象限更远不覆盖。
    let _ = h.write_quad(0, 0, 200);
    s.add("更远不覆盖", h.rep_depth(0) == Ok(Some(30)), "保浅");
    // 族二：粗拒核。
    s.add("更远粗拒", h.test(0, 40) == Ok(true), "40≥30");
    s.add("更近粗过", h.test(0, 20) == Ok(false), "20<30");
    s.add("等值粗拒", h.test(0, 30) == Ok(true), "=代表");
    // 空区域不拒。
    s.add("空区不拒", h.test(5, 999) == Ok(false), "无代表");
    // 全象限写后代表 = 全区最浅。
    let _ = h.write_quad(0, 3, 10);
    s.add("全象限代表 10", h.rep_depth(0) == Ok(Some(10)), "四合一");
    // 族三：区域清。
    s.add("清区", h.clear(0).is_ok(), "重置");
    s.add("清后无代表", h.rep_depth(0) == Ok(None), "空区");
    s.add("清后不拒", h.test(0, 999) == Ok(false), "起白纸");
    // 清空区幂等。
    s.add("清空区幂等", h.clear(0).is_ok(), "再清");
    // 族四：命中率。
    let mut n = HzrBook::new();
    let _ = n.write_quad(1, 0, 100);
    let _ = n.write_quad(1, 1, 80);
    // 代表 80：测 90 拒 / 70 过 / 80 拒 / 60 过。
    let _ = n.test(1, 90);
    let _ = n.test(1, 70);
    let _ = n.test(1, 80);
    let _ = n.test(1, 60);
    s.add("测 4", n.tested == 4, "计数");
    s.add("拒 2", n.rejected == 2, "计数");
    s.add("率 500", n.hit_milli() == 500, "2/4");
    // 零测零率。
    let z = HzrBook::new();
    s.add("零测零率", z.hit_milli() == 0, "无分母");
    // 族五：多区域独立。
    let mut m = HzrBook::new();
    let _ = m.write_quad(0, 0, 100);
    let _ = m.write_quad(1, 0, 200);
    s.add("区 0 代表 100", m.rep_depth(0) == Ok(Some(100)), "独立");
    s.add("区 1 代表 200", m.rep_depth(1) == Ok(Some(200)), "独立");
    // 区 0 写 150 → 拒（≥100），区 1 写 150 → 过（<200）。
    s.add("区 0 拒 150", m.test(0, 150) == Ok(true), "独立性");
    s.add("区 1 过 150", m.test(1, 150) == Ok(false), "独立性");
    s.add("魔数核", PT_SEG_BASE == 0x31600, "段基址");
    s
}
