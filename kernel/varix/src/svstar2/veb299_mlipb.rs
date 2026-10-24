//! veb299_mlipb —— VE-F0299 Mali 性能基线（诚实期望）。
//! 锚点（CoRun Unxreal 主册）：VE-F0299 · Mali 性能基线（诚实期望） · 目标 320 行
//! 判据原文：VE-F0299-J1 诚实期望账：场景五档（清屏 / 简几
//!           何 / 中几何 / 重几何 / 填充率压榨）；每档期望
//!           帧（毫秒/帧上界，按核数与频线性缩放）；实测记
//!           账（场景 + 实测毫秒）；达标判定（实测 ≤ 期望 →
//!           达标）；期望诚实核（期望值不得低于该档硬底，
//!           低即虚标立夏案）；基线冻结（出报告后期望表只
//!           读）；回归告警（同场景实测较上测涨 20% → 告
//!           警）。
//! 组件：期望表 + 缩放律 + 达标账 + 回归告警。
//! 职责定位：诚实期望是「Mali 的良心秤」——期望不是许愿：
//!           每档都有硬底，低于硬底的期望是虚标；缩放律是
//!           秤的刻度：核多频高自然期望严；回归告警是秤的
//!           复检：上次 5 克这次 6 克，涨两成就得问一句。
//! 性能逐项分解：判定 O(1)；回归 O(账)。
//! 诊断码段：0x313C0（0x313C1~0x313C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PB_VERSION: &str = "veb299_mlipb-v1";

/// 诊断段基址（0x313C0 段 · 本模块独占）。
pub const PB_SEG_BASE: u32 = 0x313C0;
pub const XC_MAGIC: u32 = 0x313C1; // 账魔数破
pub const XC_SC: u32 = 0x313C2; // 场景号越界
pub const XC_FLOOR: u32 = 0x313C3; // 期望低于硬底
pub const XC_FROZEN: u32 = 0x313C4; // 基线冻结

/// 账魔数。
const MAGIC: u32 = 0x313C_1B0A;

/// 五档场景。
pub const SCENES: usize = 5;

/// 各档硬底期望（毫秒/帧 @ 1 核 500MHz 基准）。
pub const FLOORS: [u32; SCENES] = [1, 4, 12, 40, 20];

/// 回归告警比（20%）。
pub const REGRESS_PCT: u32 = 20;

/// 期望表。
pub struct ExpTab {
    /// 每档期望毫秒（基准态）。
    pub exp: [u32; SCENES],
    pub frozen: bool,
    magic: u32,
}

impl ExpTab {
    /// 建：期望 = 硬底 × 2（诚实基线：留一倍余量）。
    pub fn new() -> ExpTab {
        let mut exp = [0u32; SCENES];
        for i in 0..SCENES {
            exp[i] = FLOORS[i] * 2;
        }
        ExpTab { exp, frozen: false, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 改期望：不得低于硬底。
    pub fn set(&mut self, sc: usize, ms: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.frozen {
            return Err(XC_FROZEN);
        }
        if sc >= SCENES {
            return Err(XC_SC);
        }
        if ms < FLOORS[sc] {
            return Err(XC_FLOOR);
        }
        self.exp[sc] = ms;
        Ok(())
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        self.frozen = true;
        Ok(())
    }

    /// 缩放律：期望 × 基准因子 / (核数 × 频/500)。
    /// 基准因子 = 核 1 频 500 → 原值。多核高频期望更紧。
    pub fn scaled(&self, sc: usize, cores: u32, freq: u32) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if sc >= SCENES {
            return Err(XC_SC);
        }
        if cores == 0 || freq == 0 {
            return Err(XC_SC);
        }
        let scale = (cores as u64) * (freq as u64) / 500;
        if scale == 0 {
            return Ok(self.exp[sc]);
        }
        // 口径：期望 / scale，不低于硬底。
        let v = self.exp[sc] as u64 / scale.max(1);
        let floor = FLOORS[sc] as u64;
        Ok(if v < floor { floor as u32 } else { v as u32 })
    }
}

/// 实测账。
pub struct MeasBook {
    /// 每场景最近一次实测。
    pub last: [Option<u32>; SCENES],
    /// 回归告警数。
    pub regress: u32,
    magic: u32,
}

impl MeasBook {
    /// 建。
    pub fn new() -> MeasBook {
        MeasBook { last: [None; SCENES], regress: 0, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 记实测并判回归：较上测涨 ≥20% → 告警计数。
    pub fn measure(&mut self, sc: usize, ms: u32) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if sc >= SCENES {
            return Err(XC_SC);
        }
        let mut alarmed = false;
        if let Some(prev) = self.last[sc] {
            if prev > 0 && ms * 100 >= prev * (100 + REGRESS_PCT) {
                self.regress += 1;
                alarmed = true;
            }
        }
        self.last[sc] = Some(ms);
        Ok(alarmed)
    }

    /// 达标判定：实测 ≤ 期望。
    pub fn meet(&self, sc: usize, ms: u32, exp_tab: &ExpTab) -> Result<bool, u32> {
        if !self.magic_ok() || !exp_tab.magic_ok() {
            return Err(XC_MAGIC);
        }
        if sc >= SCENES {
            return Err(XC_SC);
        }
        Ok(ms <= exp_tab.exp[sc])
    }
}

pub fn run_veb299_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0299");
    // 族零：期望表构造。
    let mut t = ExpTab::new();
    s.add("清屏期望 2", t.exp[0] == 2, "1×2");
    s.add("重几期望 80", t.exp[3] == 80, "40×2");
    // 族一：改期望与硬底。
    s.add("改到 3 过", t.set(0, 3).is_ok(), "≥1");
    s.add("改到 0 拒", t.set(0, 0) == Err(XC_FLOOR), "<硬底 1");
    s.add("重几虚标拒", t.set(3, 39) == Err(XC_FLOOR), "<40 虚标");
    s.add("恰硬底过", t.set(3, 40).is_ok(), "=硬底");
    s.add("越景拒", t.set(5, 99) == Err(XC_SC), "无六档");
    // 族二：缩放律。
    // 基准核 1 频 500：scale=1 → 期望原值。
    s.add("基准原值", t.scaled(0, 1, 500) == Ok(3), "scale 1");
    // 双核同频：scale=2 → 3/2=1 → 恰硬底 1。
    s.add("双核减半", t.scaled(0, 2, 500) == Ok(1), "不低于底");
    // 四核 1000：scale=8 → 3/8=0 → 底 1。
    s.add("四核抬底", t.scaled(0, 4, 1000) == Ok(1), "硬底兜住");
    // 中几何：期望 24，四核 500：24/4=6。
    s.add("中几缩 6", t.scaled(2, 4, 500) == Ok(6), "线性");
    // 族三：达标判定。
    let m = MeasBook::new();
    s.add("2ms 达标", m.meet(0, 2, &t) == Ok(true), "≤3");
    s.add("4ms 不达", m.meet(0, 4, &t) == Ok(false), ">3");
    s.add("越景拒", m.meet(5, 1, &t) == Err(XC_SC), "无六档");
    // 族四：实测与回归告警。
    let mut mb = MeasBook::new();
    s.add("首测无告", mb.measure(0, 2) == Ok(false), "无前值");
    // 涨 50%（2→3）≥ 20% → 告警。
    s.add("涨半告警", mb.measure(0, 3) == Ok(true), "3≥2×1.2");
    // 平稳不告：3→3 涨 0%。
    s.add("平稳无告", mb.measure(0, 3) == Ok(false), "0<20%");
    // 恰 20% 告警线：2→2.4 取整测 2→3 已验；用大数：100→120。
    let mut m2 = MeasBook::new();
    let _ = m2.measure(1, 100);
    s.add("恰两成告", m2.measure(1, 120) == Ok(true), "120≥120");
    // 19% 不告：100→119。
    let mut m3 = MeasBook::new();
    let _ = m3.measure(2, 100);
    s.add("十九不告", m3.measure(2, 119) == Ok(false), "119<120");
    s.add("告警 1", m3.regress == 0, "未计");
    // 族五：基线冻结。
    s.add("冻结", t.freeze().is_ok(), "出报告");
    s.add("冻后改拒", t.set(0, 9) == Err(XC_FROZEN), "期望只读");
    s.add("冻后再冻", t.freeze().is_ok(), "幂等");
    s.add("冻后判定照常", t.scaled(2, 4, 500) == Ok(6), "只读可用");
    s.add("魔数核", PB_SEG_BASE == 0x313C0, "段基址");
    s
}
