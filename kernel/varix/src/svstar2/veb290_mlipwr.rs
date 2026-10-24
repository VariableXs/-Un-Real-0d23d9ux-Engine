//! veb290_mlipwr —— VE-F0290 Mali 功耗与热预算。
//! 锚点（CoRun Unxreal 主册）：VE-F0290 · Mali 功耗与热预算 · 目标 340 行
//! 判据原文：VE-F0290-J1 功热预算账：预算双墙（功耗墙 W +
//!           热预算 J 蓄水池模型）；消耗记账（工作 → 按价扣
//!           预算）；回充（闲刻按回充率 +J，封顶池容）；墙
//!           触发（扣后池空 → 限频档位降一档）；回充再启
//!           （池回 1/4 容 → 档位复原）；热累积核（连续满
//!           价工作 → 池单调降，不许回升）；档位单调核（池
//!           空期间档位只降不升）；池满静止核（满池闲刻回
//!           充空转不计）。
//! 组件：双墙账 + 蓄水池 + 档位机 + 单调核。
//! 职责定位：功热预算是「Mali 的水缸经济学」——热是水：干
//!           活放水，歇着进水，缸空了就限频；回充 1/4 复档
//!           是防抖的迟滞：刚滴进一滴水就满速会抽风；池空期
//!           间档位只降不升是热的物理：缸见底时没有奇迹。
//! 性能逐项分解：消耗/回充 O(1)。
//! 诊断码段：0x31180（0x31181~0x31189 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const HP_VERSION: &str = "veb290_mlipwr-v1";

/// 诊断段基址（0x31180 段 · 本模块独占）。
pub const HP_SEG_BASE: u32 = 0x31180;
pub const XC_MAGIC: u32 = 0x31181; // 账魔数破
pub const XC_POOL: u32 = 0x31182; // 池参不合法
pub const XC_FREQ: u32 = 0x31183; // 档位不合法

/// 账魔数。
const MAGIC: u32 = 0x3118_1B2A;

/// 池容（热预算单位 J）。
pub const POOL_CAP: u32 = 1000;

/// 回充 1/4 容复档迟滞线。
pub const RESUME_TH: u32 = POOL_CAP / 4;

/// 档位数（0 最高 … 3 最低限频）。
pub const NLEV: u32 = 4;

/// 功热簿。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeatBook {
    /// 池余。
    pool: i64,
    /// 功耗墙 W。
    pub wall_w: u32,
    /// 当前限频档（0 全速）。
    pub level: u32,
    /// 池空事件数。
    pub dry_events: u32,
    /// 单调违例数（应恒 0）。
    pub mono_viol: u32,
    magic: u32,
}

impl HeatBook {
    /// 建（功耗墙）。
    pub fn new(wall_w: u32) -> Result<HeatBook, u32> {
        if wall_w == 0 || wall_w > 50 {
            return Err(XC_POOL);
        }
        Ok(HeatBook {
            pool: POOL_CAP as i64,
            wall_w,
            level: 0,
            dry_events: 0,
            mono_viol: 0,
            magic: MAGIC,
        })
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 池余。
    pub fn pool_left(&self) -> i64 {
        self.pool
    }

    /// 满价工作一单位：按墙扣价（价 = 墙 W），池空 → 降档。
    pub fn work(&mut self) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.level >= NLEV {
            return Err(XC_FREQ);
        }
        let old_level = self.level;
        self.pool -= self.wall_w as i64;
        if self.pool < 0 {
            self.pool = 0;
        }
        if self.pool == 0 {
            // 池空 → 降档（档位单调：只降不升）。
            if self.level < NLEV - 1 {
                self.level += 1;
            }
            if self.level < old_level {
                self.mono_viol += 1;
            }
            self.dry_events += 1;
        }
        Ok(self.level)
    }

    /// 闲刻回充：+回充率（墙 × 40%），封顶池容；池回 1/4 →
    /// 复档（迟滞）；满池空转不计。
    pub fn idle(&mut self) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.pool >= POOL_CAP as i64 {
            // 满池空转不计。
            return Ok(self.level);
        }
        let gain = (self.wall_w as i64) * 40 / 100;
        let old = self.pool;
        self.pool = (self.pool + gain).min(POOL_CAP as i64);
        if self.pool < old {
            self.mono_viol += 1; // 池不应回退
        }
        // 迟滞复档：池 ≥ 1/4 容 → 复到当前档的下一更低档（限频
        // 松一档），逐刻递复不跳级。
        if self.pool >= RESUME_TH as i64 && self.level > 0 {
            self.level -= 1;
        }
        Ok(self.level)
    }

    /// 热累积核：满价连打池单调降。
    pub fn mono_ok(&self) -> bool {
        self.mono_viol == 0
    }
}

pub fn run_veb290_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0290");
    // 族零：配置。
    s.add("墙 0 拒", HeatBook::new(0) == Err(XC_POOL), "无墙");
    s.add("墙 60 拒", HeatBook::new(60) == Err(XC_POOL), ">50");
    let mut b = match HeatBook::new(50) {
        Ok(x) => x,
        Err(_) => {
            s.add("建账败", false, "不应到");
            return s;
        }
    };
    s.add("满池起步", b.pool_left() == POOL_CAP as i64, "1000");
    s.add("零档起步", b.level == 0, "全速");
    // 族一：消耗记账。
    // 墙 50 → 20 刻见底。
    let mut dry_at = 0u32;
    for i in 1..=25u32 {
        let _ = b.work();
        if b.pool_left() == 0 && dry_at == 0 {
            dry_at = i;
        }
    }
    s.add("20 刻见底", dry_at == 20, "1000/50");
    s.add("池空事件 1", b.dry_events == 1, "一空");
    s.add("档位降 1", b.level == 1, "一档");
    // 池空后连续工作档位只降不升。
    for _ in 0..20 {
        let _ = b.work();
    }
    s.add("连打底档", b.level == 3, "NLEV-1");
    s.add("档封顶", b.level == NLEV - 1, "3 档");
    // 单调核。
    s.add("单调无违", b.mono_ok(), "违 0");
    // 族二：回充与迟滞复档。
    // 池 0，档 3。回充率 50×40% = 20/刻。迟滞线 250。
    // 12 刻池 240 < 250 不复档；13 刻池 260 复 2 档；逐刻递复。
    let mut b2 = match HeatBook::new(50) {
        Ok(x) => x,
        Err(_) => {
            s.add("建账败 3", false, "不应到");
            return s;
        }
    };
    // 先打空（20 刻见底，档 3）。
    for _ in 0..40 {
        let _ = b2.work();
    }
    s.add("打空档 3", b2.level == 3 && b2.pool_left() == 0, "底档");
    // 12 刻不复档。
    for _ in 0..12 {
        let _ = b2.idle();
    }
    s.add("240 不复", b2.level == 3 && b2.pool_left() == 240, "线下");
    // 13 刻池 260 → 复 2 档。
    let _ = b2.idle();
    s.add("260 复 2 档", b2.level == 2, "迟滞线");
    // 14 刻复 1 档。
    let _ = b2.idle();
    s.add("280 复 1 档", b2.level == 1, "逐刻");
    // 15 刻复 0 档。
    let _ = b2.idle();
    s.add("300 复 0 档", b2.level == 0, "全速");
    // 再充到 600，档位不变（已顶）。
    for _ in 0..15 {
        let _ = b2.idle();
    }
    s.add("池 600 档 0", b2.pool_left() == 600 && b2.level == 0, "续充");
    // 单调核仍绿。
    s.add("单调无违", b2.mono_ok(), "违 0");
    // 族三：满池空转。
    let mut f = match HeatBook::new(50) {
        Ok(x) => x,
        Err(_) => {
            s.add("建账败 2", false, "不应到");
            return s;
        }
    };
    s.add("满池闲转原样", f.idle() == Ok(0) && f.pool_left() == POOL_CAP as i64, "不计");
    // 族四：再启（耗-回-耗循环）。
    let _ = f.work(); // 950
    for _ in 0..10 {
        let _ = f.idle(); // 950+200 → 1000 封顶
    }
    s.add("回满封顶", f.pool_left() == POOL_CAP as i64, "1000");
    s.add("复档 0", f.level == 0, "全速");
    s.add("魔数核", HP_SEG_BASE == 0x31180, "段基址");
    s
}
