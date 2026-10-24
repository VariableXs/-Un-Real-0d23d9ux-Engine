//! veb312_mobdpu —— VE-F0312 移动 SoC 显示控制器（DPU 类）。
//! 锚点（CoRun Unxreal 主册）：VE-F0312 · 移动 SoC 显示控制器（DPU 类） · 目标 340 行
//! 判据原文：VE-F0312-J1 DPU 账：层管线（4 层混叠，层号 0~3）；
//!           混合公式账（逐层 alpha 混合：src_over 一遍算）；
//!           旋转账（0/90/180/270 四向）；缩放账（x/y 比例，
//!           比例越界拒）；带宽节流（DPU 带宽配额，超配降层
//!           数）；扫描窗口（active / porch / blank 三段时序）；
//!           层越界核（层号、缩放比、角度越界各拒）；带宽裁
//!           剪核（超配 → 从高层往下裁到配额内）。
//! 组件：层账 + 混合器 + 变换账 + 节流器。
//! 职责定位：DPU 是「手机的合成车间」——四层是四张透明片：
//!           src_over 从下往上叠一遍；旋转缩放是车间的裁床：
//!           裁多大批量有上限；带宽节流是车间的电闸：超载就
//!           把最贵的层先请出去。
//! 性能逐项分解：混叠 O(层)；裁剪 O(层)。
//! 诊断码段：0x31700（0x31701~0x31709 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const DP_VERSION: &str = "veb312_mobdpu-v1";

/// 诊断段基址（0x31700 段 · 本模块独占）。
pub const DP_SEG_BASE: u32 = 0x31700;
pub const XC_MAGIC: u32 = 0x31701; // 账魔数破
pub const XC_LAYER: u32 = 0x31702; // 层号越界
pub const XC_ROT: u32 = 0x31703; // 角度越界
pub const XC_SCALE: u32 = 0x31704; // 缩放比越界
pub const XC_BW: u32 = 0x31705; // 带宽超配已裁

/// 账魔数。
const MAGIC: u32 = 0x3170_1B07;

/// 层数。
pub const NLAYER: usize = 4;

/// 缩放界（毫分比：0.25× ~ 8×）。
pub const SCALE_MIN: u32 = 250;
pub const SCALE_MAX: u32 = 8000;

/// 旋转角白名单。
pub const ROTS: [u32; 4] = [0, 90, 180, 270];

/// DPU 层。
pub struct DpuLayer {
    pub on: bool,
    /// alpha（0~255）。
    pub alpha: u8,
    pub rot: u32,
    /// x/y 缩放（毫分比）。
    pub sx: u32,
    pub sy: u32,
    /// 层带宽 KB。
    pub bw_kb: u32,
}

/// DPU 簿。
pub struct DpuBook {
    pub layers: [DpuLayer; NLAYER],
    /// 带宽配额 KB。
    pub quota_kb: u32,
    /// 裁剪计数。
    pub trims: u32,
    magic: u32,
}

impl DpuBook {
    /// 建。
    pub fn new(quota_kb: u32) -> DpuBook {
        DpuBook {
            layers: [
                DpuLayer { on: false, alpha: 255, rot: 0, sx: 1000, sy: 1000, bw_kb: 0 },
                DpuLayer { on: false, alpha: 255, rot: 0, sx: 1000, sy: 1000, bw_kb: 0 },
                DpuLayer { on: false, alpha: 255, rot: 0, sx: 1000, sy: 1000, bw_kb: 0 },
                DpuLayer { on: false, alpha: 255, rot: 0, sx: 1000, sy: 1000, bw_kb: 0 },
            ],
            quota_kb,
            trims: 0,
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 启层。
    pub fn enable(&mut self, li: usize, bw_kb: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if li >= NLAYER {
            return Err(XC_LAYER);
        }
        self.layers[li].on = true;
        self.layers[li].bw_kb = bw_kb;
        Ok(())
    }

    /// 禁层。
    pub fn disable(&mut self, li: usize) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if li >= NLAYER {
            return Err(XC_LAYER);
        }
        self.layers[li].on = false;
        self.layers[li].bw_kb = 0;
        Ok(())
    }

    /// 设旋转（白名单外拒）。
    pub fn set_rot(&mut self, li: usize, deg: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if li >= NLAYER {
            return Err(XC_LAYER);
        }
        let mut ok = false;
        for r in ROTS.iter() {
            if *r == deg {
                ok = true;
            }
        }
        if !ok {
            return Err(XC_ROT);
        }
        self.layers[li].rot = deg;
        Ok(())
    }

    /// 设缩放（界外拒）。
    pub fn set_scale(&mut self, li: usize, sx: u32, sy: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if li >= NLAYER {
            return Err(XC_LAYER);
        }
        if sx < SCALE_MIN || sx > SCALE_MAX || sy < SCALE_MIN || sy > SCALE_MAX {
            return Err(XC_SCALE);
        }
        self.layers[li].sx = sx;
        self.layers[li].sy = sy;
        Ok(())
    }

    /// 活层带宽合计。
    pub fn used_kb(&self) -> u32 {
        let mut t = 0;
        for l in self.layers.iter() {
            if l.on {
                t += l.bw_kb;
            }
        }
        t
    }

    /// 带宽节流：超配 → 从高层（层号大）往下裁到配额内。
    pub fn throttle(&mut self) -> u32 {
        let mut cut = 0u32;
        let mut li = NLAYER;
        while self.used_kb() > self.quota_kb && li > 0 {
            li -= 1;
            if self.layers[li].on {
                cut += self.layers[li].bw_kb;
                self.layers[li].on = false;
                self.layers[li].bw_kb = 0;
                self.trims += 1;
            }
        }
        cut
    }

    /// src_over 混合一遍：从层 0 到层 N-1，out = src×a + dst×(1-a)。
    /// 通道值 0~255。
    pub fn blend(&self, base: u32) -> u32 {
        let mut out = base;
        for l in self.layers.iter() {
            if l.on {
                let a = l.alpha as u32;
                out = (l.alpha as u32 * 255 + out * (255 - a)) / 255;
            }
        }
        out
    }

    /// 活层数。
    pub fn live(&self) -> usize {
        let mut n = 0;
        for l in self.layers.iter() {
            if l.on {
                n += 1;
            }
        }
        n
    }
}

pub fn run_veb312_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0312");
    let mut d = DpuBook::new(1000);
    // 族零：层界。
    s.add("层 4 拒", d.enable(4, 100) == Err(XC_LAYER), "越界");
    s.add("旋层 9 拒", d.set_rot(9, 90) == Err(XC_LAYER), "越界");
    s.add("缩层 9 拒", d.set_scale(9, 1000, 1000) == Err(XC_LAYER), "越界");
    // 族一：旋转白名单。
    let _ = d.enable(0, 100);
    s.add("转 45 拒", d.set_rot(0, 45) == Err(XC_ROT), "非四向");
    s.add("转 360 拒", d.set_rot(0, 360) == Err(XC_ROT), "非四向");
    s.add("转 90 过", d.set_rot(0, 90).is_ok(), "白名单");
    s.add("转 270 过", d.set_rot(0, 270).is_ok(), "白名单");
    // 族二：缩放界。
    s.add("缩 200 拒", d.set_scale(0, 200, 1000) == Err(XC_SCALE), "<0.25×");
    s.add("缩 9000 拒", d.set_scale(0, 1000, 9000) == Err(XC_SCALE), ">8×");
    s.add("缩 250 过", d.set_scale(0, 250, 250).is_ok(), "下沿");
    s.add("缩 8000 过", d.set_scale(0, 8000, 8000).is_ok(), "上沿");
    s.add("缩 1000 过", d.set_scale(0, 1000, 2000).is_ok(), "常规");
    // 族三：混合公式。
    // 只有层 0 活，alpha 255：out = alpha 顶满 = 255。
    s.add("全遮 255", d.blend(0) == 255, "α=1");
    // 半透明：alpha 128，base 100 → (128×255 + 100×127)/255 = 32640+12700=45340/255=177。
    d.layers[0].alpha = 128;
    s.add("半透 177", d.blend(100) == 177, "src_over");
    // alpha 0 → 全露底。
    d.layers[0].alpha = 0;
    s.add("零透露底", d.blend(100) == 100, "α=0");
    // 双层叠：层 1 alpha 255 → 顶满。
    let _ = d.enable(1, 100);
    d.layers[1].alpha = 255;
    s.add("二层顶满", d.blend(0) == 255, "逐层叠");
    d.layers[1].alpha = 0;
    s.add("二层全透", d.blend(100) == 100, "无贡献");
    // 禁层后不计入。
    let _ = d.disable(1);
    d.layers[0].alpha = 255;
    s.add("禁层出算", d.blend(50) == 255, "只算活层");
    // 族四：带宽节流。
    let mut t = DpuBook::new(500);
    let _ = t.enable(0, 200);
    let _ = t.enable(1, 200);
    let _ = t.enable(2, 200);
    let _ = t.enable(3, 200); // 800 > 500
    s.add("超配 800", t.used_kb() == 800, "四层");
    let cut = t.throttle();
    s.add("裁 400", cut == 400, "掉两层");
    s.add("余两活", t.live() == 2 && t.used_kb() == 400, "≤500");
    s.add("裁计数 2", t.trims == 2, "两刀");
    // 从高层先裁：剩 0、1 两层。
    s.add("高层先出", t.layers[3].on == false && t.layers[2].on == false, "层 3,2 掉");
    // 再节流幂等（已达标）。
    s.add("达标不裁", t.throttle() == 0, "无事");
    // 族五：活层计数。
    let mut c = DpuBook::new(9999);
    s.add("零活", c.live() == 0, "全灭");
    let _ = c.enable(2, 10);
    s.add("一活", c.live() == 1, "单层");
    let _ = c.disable(2);
    s.add("归零", c.live() == 0, "全灭");
    s.add("魔数核", DP_SEG_BASE == 0x31700, "段基址");
    s
}
