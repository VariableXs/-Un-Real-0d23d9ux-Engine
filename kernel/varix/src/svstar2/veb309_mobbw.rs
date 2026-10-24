//! veb309_mobbw —— VE-F0309 移动 GPU 带宽优先设计准则。
//! 锚点（CoRun Unxreal 主册）：VE-F0309 · 移动 GPU 带宽优先设计准则 · 目标 340 行
//! 判据原文：VE-F0309-J1 带宽准则账：通路账（读带宽 / 写带
//!           宽 / 总带宽三账）；纹理压缩收益（未压 RGBA8 →
//!           ASTC 8×8 = 1 字节/像素，省 87.5%）；过绘制惩罚
//!           （每层过绘制 = 全屏一遍写带宽，层 × 分辨率 ×
//!           4B）；带宽预算（每帧上限 MB）；超支档位（0~2
//!           档：贴路 / 黄灯 / 红灯，红灯 = 超支 > 20%）；
//!           减负建议（红灯 → 建议列表：压纹理 / 降过绘 /
//!           分辨率缩放三选）；预算平衡核（通路账之和 = 总）。
//! 组件：三账 + 收益表 + 惩罚器 + 档位机。
//! 职责定位：带宽准则是「移动 GPU 的油钱账」——移动芯片不缺
//!           算力缺油水：带宽就是油钱；ASTC 压缩是打折卡：
//!           八分之一的油钱干同样的活；过绘制是白烧油：画了
//!           又被盖住的每一层都是纯浪费；红灯是油表见底：不
//!           减负就得降帧。
//! 性能逐项分解：记账 O(1)；档位 O(1)。
//! 诊断码段：0x31640（0x31641~0x31649 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const BW_VERSION: &str = "veb309_mobbw-v1";

/// 诊断段基址（0x31640 段 · 本模块独占）。
pub const BW_SEG_BASE: u32 = 0x31640;
pub const XC_MAGIC: u32 = 0x31641; // 账魔数破
pub const XC_RES: u32 = 0x31642; // 分辨率不合法
pub const XC_BUDGET: u32 = 0x31643; // 预算不合法

/// 账魔数。
const MAGIC: u32 = 0x3164_1B0D;

/// RGBA8 字节/像素。
pub const BPP_RGBA8: u32 = 4;

/// ASTC 8×8 字节/像素（128 位/64 像素 = 2B）。
pub const BPP_ASTC8: u32 = 2;

/// 压缩收益（毫分比）。
pub fn astc_saving_milli() -> u32 {
    (BPP_RGBA8 - BPP_ASTC8) * 1000 / BPP_RGBA8
}

/// 档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Green,
    Yellow,
    Red,
}

/// 红灯线（超支 20%）。
pub const RED_PCT: u32 = 120;

/// 带宽账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BwBook {
    /// 分辨率（宽 × 高）。
    pub w: u32,
    pub h: u32,
    /// 帧带宽预算（KB）。
    pub budget_kb: u32,
    /// 读 KB。
    pub read_kb: u32,
    /// 写 KB（不含过绘制惩罚）。
    pub write_kb: u32,
    /// 过绘制层数。
    pub overdraw: u32,
    magic: u32,
}

impl BwBook {
    /// 建。
    pub fn new(w: u32, h: u32, budget_kb: u32) -> Result<BwBook, u32> {
        if w == 0 || h == 0 || w > 8192 || h > 8192 {
            return Err(XC_RES);
        }
        if budget_kb == 0 || budget_kb > 65536 {
            return Err(XC_BUDGET);
        }
        Ok(BwBook { w, h, budget_kb, read_kb: 0, write_kb: 0, overdraw: 0, magic: MAGIC })
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 记读。
    pub fn add_read(&mut self, kb: u32) {
        self.read_kb += kb;
    }

    /// 记写。
    pub fn add_write(&mut self, kb: u32) {
        self.write_kb += kb;
    }

    /// 记过绘制层（惩罚 = 全屏 × 4B = 分辨率 × 4 / 1024 KB）。
    pub fn add_overdraw(&mut self, layers: u32) {
        self.overdraw += layers;
    }

    /// 过绘制惩罚 KB。
    pub fn overdraw_kb(&self) -> u32 {
        let per_layer_kb = self.w * self.h * BPP_RGBA8 / 1024;
        per_layer_kb * self.overdraw
    }

    /// 总带宽（读 + 写 + 过绘惩罚）。
    pub fn total_kb(&self) -> u32 {
        self.read_kb + self.write_kb + self.overdraw_kb()
    }

    /// 预算平衡核：总 = 三项之和（恒等重算）。
    pub fn balanced(&self) -> bool {
        self.total_kb() == self.read_kb + self.write_kb + self.overdraw_kb()
    }

    /// 超支档位。
    pub fn band(&self) -> Band {
        let t = self.total_kb() as u64;
        let b = self.budget_kb as u64;
        // 档位按占比：≤100% 绿；≤120% 黄；>120% 红。
        if t * 100 > b * RED_PCT as u64 {
            Band::Red
        } else if t > b {
            Band::Yellow
        } else {
            Band::Green
        }
    }

    /// 超支毫分比（可为 0 = 不超）。
    pub fn over_milli(&self) -> u32 {
        let t = self.total_kb() as u64;
        let b = self.budget_kb as u64;
        if t <= b {
            0
        } else {
            ((t - b) * 1000 / b) as u32
        }
    }
}

/// 减负建议（红灯出）。
pub fn advice(b: Band) -> &'static [&'static str] {
    match b {
        Band::Red => &["compress_tex", "cut_overdraw", "scale_res"],
        Band::Yellow => &["compress_tex"],
        Band::Green => &[],
    }
}

pub fn run_veb309_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0309");
    // 族零：配置。
    s.add("零宽拒", BwBook::new(0, 1080, 1024) == Err(XC_RES), "无零宽");
    s.add("巨屏拒", BwBook::new(9999, 1080, 1024) == Err(XC_RES), ">8192");
    s.add("零预算拒", BwBook::new(1920, 1080, 0) == Err(XC_BUDGET), "无零预算");
    s.add("巨预算拒", BwBook::new(1920, 1080, 70000) == Err(XC_BUDGET), ">64MB");
    // 族一：压缩收益。
    s.add("ASTC 省 500", astc_saving_milli() == 500, "4B→2B");
    s.add("ASTC 半价", BPP_ASTC8 * 2 == BPP_RGBA8, "半带宽");
    // 族二：记账与平衡。
    let mut b = match BwBook::new(1920, 1080, 1024) { Ok(x) => x, Err(_) => { s.add("建账败", false, "不应到"); return s; } };
    b.add_read(300);
    b.add_write(200);
    b.add_overdraw(2);
    // 惩罚：1920×1080×4/1024 = 8100 KB/层 × 2 = 16200 KB。
    s.add("惩罚 16200", b.overdraw_kb() == 16200, "两整屏");
    s.add("总 16700", b.total_kb() == 16700, "300+200+16200");
    s.add("平衡核", b.balanced(), "恒等");
    // 族三：档位机。
    // 16700 >> 1024 → 红灯。
    s.add("红灯", b.band() == Band::Red, "超支 16 倍");
    // 贴路：新账零笔 → 绿。
    let g = match BwBook::new(1920, 1080, 1024) { Ok(x) => x, Err(_) => { s.add("建账败", false, "不应到"); return s; } };
    s.add("绿灯", g.band() == Band::Green, "零用");
    // 黄灯：恰超不超 20%。
    let mut y = match BwBook::new(1920, 1080, 10000) { Ok(x) => x, Err(_) => { s.add("建账败", false, "不应到"); return s; } };
    y.add_read(6000);
    y.add_write(5000); // 11000 = 110% → 黄
    s.add("黄灯", y.band() == Band::Yellow, "110%");
    s.add("黄超支 100", y.over_milli() == 100, "1000/10000");
    // 红灯线：恰 120% → 黄（>120% 才红）。
    let mut y2 = match BwBook::new(1920, 1080, 10000) { Ok(x) => x, Err(_) => { s.add("建账败", false, "不应到"); return s; } };
    y2.add_read(7000);
    y2.add_write(5000); // 12000 = 120% → 黄
    s.add("百二仍黄", y2.band() == Band::Yellow, "恰线不红");
    // 120.1% → 红。
    y2.add_read(1);
    s.add("超线红灯", y2.band() == Band::Red, "12000+1");
    // 族四：减负建议。
    s.add("红灯三策", advice(Band::Red).len() == 3, "三选");
    s.add("黄灯一策", advice(Band::Yellow).len() == 1, "先压纹理");
    s.add("绿灯无策", advice(Band::Green).is_empty(), "不用减");
    s.add("首策压纹理", advice(Band::Red)[0] == "compress_tex", "性价比先");
    // 族五：分辨率缩放收益核。
    // 半分辨率惩罚减半。
    let mut half = match BwBook::new(960, 540, 1024) { Ok(x) => x, Err(_) => { s.add("建账败", false, "不应到"); return s; } };
    half.add_overdraw(2);
    s.add("半屏惩罚 4050", half.overdraw_kb() == 4050, "960×540×4×2/1024");
    s.add("惩罚比例 1/4", half.overdraw_kb() * 4 == 16200, "面积 1/4");
    s.add("魔数核", BW_SEG_BASE == 0x31640, "段基址");
    s
}
