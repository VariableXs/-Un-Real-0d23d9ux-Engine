//! cga70_deep10bit —— CGPU-F0070 高色深处理路径（10bit）。
//! 锚点（CGPU 施工书）：CGPU-F0070 · 高色深处理路径（10bit）· 目标 340 行
//! 判据摘录：10bit 值域闭集 [0,1023]（越界显性拒绝）；8↔10 位
//!   互换无损往返（<<2 / >>2 精确域——8bit 全域 256 值逐个往返
//!   恒等）；10bit 域亮度方程（Rec.709 系数定点 2126/7152/722
//!   ÷10000）；HDR 裁剪账（≥1023 饱和像素计数落账不静默）；色
//!   深域直方图 1024 桶守恒；8bit 管线输出必须先量化（dither
//!   或截断闭集，不许静默混域）。
//! 组件：10bit 值域模型 + 位深互换往返核 + 定点亮度方程 + 饱和
//!   裁剪账 + 1024 桶直方图 + 量化降深器（截断/抖动两模式）+
//!   混域审计器。
//! 职责定位：HDR/广色深处理的地基——10bit 是 SDR(8bit) 与
//!   HDR(16bit) 的中间精档；混域（8bit 值当 10bit 用）是高色深
//!   管线的头号暗病，本模块用值域闭集+混域审计钉死。
//! 性能逐项分解：往返 O(1)/像素；亮度 O(1)/像素（3 乘 2 移）；
//!   直方图 O(n)。全整数确定性。
//! 诊断码段：0xE6F0（0xE6F1~0xE6F9 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DB_VERSION: &str = "cga70_deep10bit-v1";

/// 诊断段基址（0xE6F0 段 · 本模块独占）。
pub const DB_SEG_BASE: u16 = 0xE6F0;

/// XcCode 闭集（0xE6F1~0xE6F9）。
pub const XC_VAL_OOB: u32 = 0xE6F1;
pub const XC_ROUNDTRIP: u32 = 0xE6F2;
pub const XC_MIXED_DOMAIN: u32 = 0xE6F3;
pub const XC_LUMA_DRIFT: u32 = 0xE6F4;
pub const XC_HIST_OOB: u32 = 0xE6F5;
pub const XC_SAT_OOB: u32 = 0xE6F6;
pub const XC_MODE_OOB: u32 = 0xE6F7;
pub const XC_LEN_MISMATCH: u32 = 0xE6F8;
pub const XC_DIM_ZERO: u32 = 0xE6F9;

/// 10bit 域上界（含）。
pub const D10_MAX: u32 = 1023;

/// 10bit 值域校验。
pub fn d10_ok(v: u32) -> Result<(), u32> {
    if v > D10_MAX {
        return Err(XC_VAL_OOB);
    }
    Ok(())
}

/// 8→10 升深（<<2 精确倍域）。
pub fn up8(v8: u32) -> Result<u32, u32> {
    if v8 > 255 {
        return Err(XC_VAL_OOB);
    }
    Ok(v8 << 2)
}

/// 10→8 截断降深（>>2——量化模式 0）。
pub fn down8_trunc(v10: u32) -> Result<u32, u32> {
    d10_ok(v10)?;
    Ok(v10 >> 2)
}

/// 10→8 抖动降深（+2 舍入 >>2——量化模式 1，误差 ≤ 1 LSB8）。
pub fn down8_dither(v10: u32) -> Result<u32, u32> {
    d10_ok(v10)?;
    Ok(((v10 + 2) >> 2).min(255))
}

/// 往返无损核：8→10→8 恒等（256 值全域穷举——升深恒是精确倍域）。
pub fn roundtrip_exhaustive() -> Result<(), u32> {
    for v in 0..256u32 {
        let up = up8(v)?;
        let back = down8_trunc(up)?;
        if back != v {
            return Err(XC_ROUNDTRIP + v);
        }
    }
    Ok(())
}

/// 定点亮度方程（Rec.709：Y = 0.2126R + 0.7152G + 0.0722B，
/// 万分比定点 → >>14 归一——10bit 域输出 ≤ 1023）。
pub fn luma10(r: u32, g: u32, b: u32) -> Result<u32, u32> {
    for v in [r, g, b].iter() {
        d10_ok(*v)?;
    }
    let y = (r as u64 * 2126 + g as u64 * 7152 + b as u64 * 722) / 10000;
    Ok(y.min(D10_MAX as u64) as u32)
}

/// 饱和裁剪账：输入域钳到 [0,1023] 并计数钳顶像素（不静默）。
pub struct SatLedger {
    pub clips_high: u64,
    pub clips_low: u64,
}

impl SatLedger {
    pub fn new() -> SatLedger {
        SatLedger { clips_high: 0, clips_low: 0 }
    }

    /// 记一像素（i64 域进，10bit 域出）。
    pub fn clip(&mut self, v: i64) -> u32 {
        if v > D10_MAX as i64 {
            self.clips_high += 1;
            D10_MAX
        } else if v < 0 {
            self.clips_low += 1;
            0
        } else {
            v as u32
        }
    }
}

/// 1024 桶直方图（守恒：Σ桶 == 像素数）。
pub struct Hist1024 {
    pub buckets: Vec<u32>,
    pub total: u64,
}

impl Hist1024 {
    pub fn new() -> Hist1024 {
        Hist1024 { buckets: vec![0; 1024], total: 0 }
    }

    pub fn record(&mut self, v: u32) -> Result<(), u32> {
        d10_ok(v)?;
        self.buckets[v as usize] += 1;
        self.total += 1;
        Ok(())
    }

    pub fn conserve(&self) -> bool {
        self.buckets.iter().map(|&b| b as u64).sum::<u64>() == self.total
    }
}

/// 混域审计：8bit 图（值 ≤255）被当 10bit 图消费时——所有值
/// 都落在 10bit 域的最低 1/4 ——审计旗：upper_quartile 占比为 0
/// 即疑似混域（立案提示，不阻断——低动态场景合法）。
pub fn mixed_domain_audit(px: &[u32]) -> Result<bool, u32> {
    if px.is_empty() {
        return Err(XC_DIM_ZERO);
    }
    let mut hi = 0usize;
    for &v in px.iter() {
        d10_ok(v)?;
        if v > 767 {
            hi += 1;
        }
    }
    Ok(hi == 0)
}

/// 量化模式闭集路由（0=截断 1=抖动，越界拒——混域出口唯一）。
pub fn quantize(v10: u32, mode: u8) -> Result<u32, u32> {
    match mode {
        0 => down8_trunc(v10),
        1 => down8_dither(v10),
        _ => Err(XC_MODE_OOB),
    }
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga70_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0070");
    // 族一：值域闭集。
    cs.add("域内过", d10_ok(0).is_ok() && d10_ok(1023).is_ok(), "[0,1023]");
    cs.add("1024 拒", d10_ok(1024).is_err(), "VAL_OOB");
    cs.add("8bit 上界拒", up8(256).is_err(), "VAL_OOB");
    // 族二：往返无损（256 值穷举）。
    cs.add("往返穷举", roundtrip_exhaustive().is_ok(), "8→10→8 恒等");
    cs.add("升深倍域", up8(200) == Ok(800), "<<2 精确");
    // 族三：量化两模式。
    cs.add("截断量化", matches!(quantize(1023, 0), Ok(255)), "顶满");
    cs.add("抖动量化", matches!(quantize(1023, 1), Ok(255)), "顶满钳制");
    cs.add("抖动舍入", matches!(quantize(1000, 1), Ok(250)), "(1002)>>2");
    cs.add("量化越界拒", quantize(500, 2).is_err(), "MODE_OOB");
    cs.add("量化域拒", quantize(2000, 0).is_err(), "VAL_OOB");
    // 族四：亮度方程锚（白/黑/纯绿）。
    cs.add("白亮度", matches!(luma10(1023, 1023, 1023), Ok(1023)), "全 1 → 1023");
    cs.add("黑亮度", matches!(luma10(0, 0, 0), Ok(0)), "全 0 → 0");
    cs.add("纯绿亮度", matches!(luma10(0, 1023, 0), Ok(731)), "7152/10000×1023≈731");
    cs.add("亮度越界拒", luma10(2000, 0, 0).is_err(), "VAL_OOB");
    // 族五：饱和账。
    let mut led = SatLedger::new();
    let _ = led.clip(-5);
    let _ = led.clip(500);
    let _ = led.clip(2000);
    cs.add("钳顶计数", led.clips_high == 1 && led.clips_low == 1, "两端各一");
    cs.add("域内直通", led.clip(500) == 500, "不钳不记");
    // 族六：直方图守恒。
    let mut h = Hist1024::new();
    for v in [0u32, 100, 100, 1023].iter() {
        let _ = h.record(*v).map_err(|e| e as u16);
    }
    cs.add("直方图守恒", h.conserve(), "Σ桶=4");
    cs.add("桶值锚", h.buckets[100] == 2 && h.buckets[0] == 1, "分布对账");
    cs.add("直方图越界拒", h.record(2000).is_err(), "VAL_OOB");
    // 族七：混域审计。
    cs.add("疑似混域", matches!(mixed_domain_audit(&[10, 200, 255]), Ok(true)), "全落低 1/4");
    cs.add("高动态正常", matches!(mixed_domain_audit(&[10, 900, 1023]), Ok(false)), "有高位");
    cs.add("空审计拒", mixed_domain_audit(&[]).is_err(), "DIM_ZERO");
    cs
}
