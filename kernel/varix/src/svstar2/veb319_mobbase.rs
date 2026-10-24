//! veb319_mobbase —— VE-F0319 移动组性能基线。
//! 锚点（CoRun Unxreal 主册）：VE-F0319 · 移动组性能基线 · 目标 320 行
//! 判据原文：VE-F0319-J1 移动基线账：场景四档（UI 滑动 / 视
//!           频解码 / 相机预览 / 游戏中载）；功耗口径（毫瓦
//!           每帧上界）；缩放律（随分辨率线性、随核数反比）；
//!           实测记账（场景 + 毫瓦）；达标判定（实测 ≤ 期
//!           望）；能效比核（每毫瓦帧率，越大越好）；回归告
//!           警（涨 15% 告警，比桌面线更敏感）；硬底诚实核
//!           （期望不得低于硬底）。
//! 组件：期望表 + 缩放律 + 能效比 + 回归账。
//! 职责定位：移动基线是「手机的电表读数」——桌面看帧率、移
//!           动看毫瓦：同样 60 帧，费电的是差基线；能效比是
//!           电表的良心：每瓦能换几帧；回归线 15% 比桌面紧：
//!           手机发热用户最先感知。
//! 性能逐项分解：判定 O(1)。
//! 诊断码段：0x318C0（0x318C1~0x318C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const MB_VERSION: &str = "veb319_mobbase-v1";

/// 诊断段基址（0x318C0 段 · 本模块独占）。
pub const MB_SEG_BASE: u32 = 0x318C0;
pub const XC_MAGIC: u32 = 0x318C1; // 账魔数破
pub const XC_SC: u32 = 0x318C2; // 场景越界
pub const XC_FLOOR: u32 = 0x318C3; // 低于硬底
pub const XC_ZERO: u32 = 0x318C4; // 零值算能效

/// 账魔数。
const MAGIC: u32 = 0x318C_1B0C;

/// 四档场景。
pub const NSC: usize = 4;

/// 硬底（毫瓦/帧 @ 1080p 4 核基准）。
pub const FLOORS: [u32; NSC] = [5, 40, 60, 150];

/// 回归告警比（15%）。
pub const REGRESS_PCT: u32 = 15;

/// 基准分辨率（宽×高，K 像素）。
pub const BASE_KPX: u32 = (1920 * 1080) / 1000;
/// 基准核数。
pub const BASE_CORES: u32 = 4;

/// 基线簿。
pub struct BaseBook {
    /// 每档期望毫瓦（基准态）。
    pub exp: [u32; NSC],
    pub frozen: bool,
    magic: u32,
}

impl BaseBook {
    /// 建：期望 = 硬底 × 3（移动留两倍余量——热余量比帧余量值钱）。
    pub fn new() -> BaseBook {
        let mut exp = [0u32; NSC];
        for i in 0..NSC {
            exp[i] = FLOORS[i] * 3;
        }
        BaseBook { exp, frozen: false, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 改期望。
    pub fn set(&mut self, sc: usize, mw: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.frozen {
            return Err(XC_MAGIC);
        }
        if sc >= NSC {
            return Err(XC_SC);
        }
        if mw < FLOORS[sc] {
            return Err(XC_FLOOR);
        }
        self.exp[sc] = mw;
        Ok(())
    }

    /// 冻结。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 缩放律：期望 × BASE_KPX/实际KPX × BASE_CORES/实际核数，
    /// 不低于硬底。
    pub fn scaled(&self, sc: usize, kpx: u32, cores: u32) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if sc >= NSC || kpx == 0 || cores == 0 {
            return Err(XC_SC);
        }
        let v = (self.exp[sc] as u64 * BASE_KPX as u64 * BASE_CORES as u64
            / (kpx as u64 * cores as u64)) as u64;
        let floor = FLOORS[sc] as u64;
        Ok(if v < floor { floor as u32 } else { v as u32 })
    }

    /// 达标。
    pub fn meet(&self, sc: usize, mw: u32) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if sc >= NSC {
            return Err(XC_SC);
        }
        Ok(mw <= self.exp[sc])
    }
}

/// 能效比：帧率 × 1000 / 毫瓦（帧每瓦 × 1000）。
pub fn effi_milli(fps: u32, mw: u32) -> Result<u32, u32> {
    if mw == 0 {
        return Err(XC_ZERO);
    }
    Ok(fps * 1000 / mw)
}

/// 回归账。
pub struct RegBook {
    pub last: [Option<u32>; NSC],
    pub alarms: u32,
    magic: u32,
}

impl RegBook {
    /// 建。
    pub fn new() -> RegBook {
        RegBook { last: [None; NSC], alarms: 0, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 记实测并判回归（涨 ≥15% 告警）。
    pub fn measure(&mut self, sc: usize, mw: u32) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if sc >= NSC {
            return Err(XC_SC);
        }
        let mut alarm = false;
        if let Some(p) = self.last[sc] {
            if p > 0 && mw * 100 >= p * (100 + REGRESS_PCT) {
                self.alarms += 1;
                alarm = true;
            }
        }
        self.last[sc] = Some(mw);
        Ok(alarm)
    }
}

pub fn run_veb319_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0319");
    // 族零：期望构造。
    let mut b = BaseBook::new();
    s.add("UI 期望 15", b.exp[0] == 15, "5×3");
    s.add("游戏期望 450", b.exp[3] == 450, "150×3");
    // 族一：硬底诚实。
    s.add("改到 4 拒", b.set(0, 4) == Err(XC_FLOOR), "<5 虚标");
    s.add("改到 5 过", b.set(0, 5).is_ok(), "恰硬底");
    s.add("改到 149 拒", b.set(3, 149) == Err(XC_FLOOR), "<150");
    s.add("越景拒", b.set(4, 999) == Err(XC_SC), "无五档");
    // 族二：缩放律。
    // UI 期望 15，基准 1080p（2073 KPx）4 核 → 原值 15。
    s.add("基准原值", b.scaled(0, BASE_KPX, 4) == Ok(15), "scale 1");
    // 半分辨率 1037 KPx（下取整）：15×2073/1037×4/4 ≈ 30。
    let half = (1920 * 1080) / 2 / 1000;
    s.add("半屏翻倍", b.scaled(0, half, 4) == Ok(30), "线性");
    // 双核：再翻倍 → 60。
    s.add("双核再倍", b.scaled(0, half, 2) == Ok(60), "核反比");
    // 游戏 450，四倍分辨率四核：450×2073/(4×2073×4)=450/16=28 → 硬底 150 兜。
    s.add("游戏硬底兜", b.scaled(3, BASE_KPX * 4, 4) == Ok(150), "不穿底");
    // 族三：达标。
    s.add("UI 14 达标", b.meet(0, 14) == Ok(true), "≤15");
    s.add("UI 16 不达", b.meet(0, 16) == Ok(false), ">15");
    // 族四：能效比。
    s.add("60 帧 500 毫瓦", effi_milli(60, 500) == Ok(120), "0.12 千帧/瓦");
    s.add("零瓦拒", effi_milli(60, 0) == Err(XC_ZERO), "无分母");
    s.add("高能效比较", effi_milli(60, 300) > effi_milli(60, 500), "省电者优");
    // 族五：回归告警（15%）。
    let mut r = RegBook::new();
    s.add("首测无告", r.measure(0, 100) == Ok(false), "无前值");
    // 涨 20%（100→120）→ 告。
    s.add("涨两成告", r.measure(0, 120) == Ok(true), "120≥115");
    // 平稳不告。
    s.add("平稳无告", r.measure(0, 120) == Ok(false), "0%");
    // 涨 14% 不告：100→114。
    let mut r2 = RegBook::new();
    let _ = r2.measure(1, 100);
    s.add("十四不告", r2.measure(1, 114) == Ok(false), "114<115");
    // 恰 15% 告。
    s.add("恰十五告", r2.measure(1, 115) == Ok(true), "115≥115");
    s.add("告账 1", r2.alarms == 1, "一告");
    s.add("魔数核", MB_SEG_BASE == 0x318C0, "段基址");
    s
}
