//! unx0417_freqdrift —— UNX-F0417 计时异常注入（频率漂移）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0417 · 计时异常注入 · 目标 220 行
//! 判据原文：UNX-F0417-J1 可控注入频率漂移（比例千分 + 漂移方
//!           向），要求：漂移后计时器累积误差可算（理论值 =
//!           名义时长 × 漂移率 × 步数）；误差超阈立案（千分
//!           制）；补偿器按漂移率反调（补偿后残差 ≤ 补偿精度
//!           下限）；漂移率越域拒（|率| > 500‰）；补偿器在
//!           零漂移下零干预（不补偿不存在的问题）。
//! 组件：漂移注入器（率 + 方向 + 生效步窗）+ 计时模型（名义
//!       tick × 率）+ 误差账（理论差 + 实测差）+ 补偿器（反
//!       调 + 残差核）+ 立案账。
//! 职责定位：频率漂移注入是「计时面的压力测试」——晶振不是
//!   理想的：温漂、老化、电压波动都会让实际频率偏离标称；
//!   注入器让测试可以在确定性条件下重现漂移，验证两件事：
//!   误差能被算出来（理论值与实测一致），补偿器能把残差压回
//!   精度下限内。零漂移下零干预是补偿器的诚实底线。
//! 性能逐项分解：单步 O(1)；误差核 O(1)；补偿 O(1)。
//! 诊断码段：0x211C0（0x211C1~0x211C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FD_VERSION: &str = "unx0417_freqdrift-v1";

/// 诊断段基址（0x211C0 段 · 本模块独占）。
pub const FD_SEG_BASE: u32 = 0x211C0;
pub const XC_RATE_OOR: u32 = 0x211C1; // 漂移率越域
pub const XC_ERR_HI: u32 = 0x211C2; // 误差超阈立案
pub const XC_RESIDUAL: u32 = 0x211C3; // 补偿后残差超下限
pub const XC_ZERO_DRIFT_ACT: u32 = 0x211C4; // 零漂移下干预
pub const XC_REPLAY: u32 = 0x211C5; // 误差重放不一致
pub const XC_STEPS_OOR: u32 = 0x211C6; // 步数越域
pub const XC_COMP_PREC: u32 = 0x211C7; // 补偿精度越域
pub const XC_MAGIC: u32 = 0x211C8; // 注入器魔数破
pub const XC_DIR_OOR: u32 = 0x211C9; // 方向越域

/// 漂移方向：1 = 变快（正）2 = 变慢（负）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriftDir {
    Fast = 1,
    Slow = 2,
}

/// 漂移率上限（千分）。
pub const RATE_MAX: i64 = 500;

/// 漂移注入器。
#[derive(Clone, Copy, Debug)]
pub struct DriftInjector {
    /// 漂移率（千分，正 = 快）。
    pub rate_milli: i64,
    /// 方向。
    pub dir: DriftDir,
}

impl DriftInjector {
    /// 建：率 + 方向；方向与率符号必须一致。
    pub fn new(rate_milli: i64, dir: DriftDir) -> Result<DriftInjector, u32> {
        if rate_milli.abs() > RATE_MAX {
            return Err(XC_RATE_OOR);
        }
        let dir_ok = match dir {
            DriftDir::Fast => rate_milli >= 0,
            DriftDir::Slow => rate_milli <= 0,
        };
        if !dir_ok {
            return Err(XC_DIR_OOR);
        }
        Ok(DriftInjector { rate_milli, dir })
    }

    /// 实际 tick：名义 tick 经漂移缩放（快 = 多 tick，慢 = 少）。
    pub fn real_ticks(&self, nominal: u64) -> u64 {
        let rate = self.rate_milli;
        if rate >= 0 {
            nominal + nominal * rate as u64 / 1000
        } else {
            nominal - nominal * (-rate) as u64 / 1000
        }
    }

    /// 理论误差（tick）：real - nominal（带符号）。
    pub fn theory_error(&self, nominal: u64) -> i64 {
        self.real_ticks(nominal) as i64 - nominal as i64
    }
}

/// 误差账：累积名义与实际。
pub struct ErrBook {
    nominal_sum: u64,
    real_sum: u64,
    inj: DriftInjector,
}

impl ErrBook {
    /// 建：注入器。
    pub fn new(inj: DriftInjector) -> Result<ErrBook, u32> {
        match inj.rate_milli {
            r if r.abs() > RATE_MAX => Err(XC_RATE_OOR),
            _ => Ok(ErrBook { nominal_sum: 0, real_sum: 0, inj }),
        }
    }

    /// 单步：名义 tick 经注入器累积。
    pub fn step(&mut self, nominal: u64) {
        self.nominal_sum += nominal;
        self.real_sum += self.inj.real_ticks(nominal);
    }

    /// 实测误差（带符号）。
    pub fn measured_error(&self) -> i64 {
        self.real_sum as i64 - self.nominal_sum as i64
    }

    /// 理论误差。
    pub fn theory_error(&self) -> i64 {
        self.inj.theory_error(self.nominal_sum)
    }

    /// 实测 = 理论核。
    pub fn theory_ok(&self) -> Result<(), u32> {
        if self.measured_error() != self.theory_error() {
            return Err(XC_REPLAY);
        }
        Ok(())
    }

    /// 误差超阈立案（千分制：|误差| × 1000 / 名义）。
    pub fn err_gate(&self, tol_milli: i64) -> Result<u32, u32> {
        if self.nominal_sum == 0 {
            return Err(XC_STEPS_OOR);
        }
        let e = self.measured_error().abs();
        let milli = e * 1000 / self.nominal_sum as i64;
        if milli > tol_milli {
            return Err(XC_ERR_HI);
        }
        Ok(milli as u32)
    }
}

/// 补偿器：按漂移率反调（名义 → 补偿后 ≈ 真实名义）。
pub struct Compensator {
    inj: DriftInjector,
    /// 补偿精度下限（tick）。
    pub precision: u64,
}

impl Compensator {
    /// 建：注入器 + 精度下限。
    pub fn new(inj: DriftInjector, precision: u64) -> Result<Compensator, u32> {
        if precision == 0 || precision > 1 << 20 {
            return Err(XC_COMP_PREC);
        }
        Ok(Compensator { inj, precision })
    }

    /// 补偿后的 tick（目标：补偿 real → 回名义）。
    pub fn compensate(&self, real: u64) -> u64 {
        let rate = self.inj.rate_milli;
        if rate == 0 {
            return real;
        }
        if rate > 0 {
            // real = nominal × (1 + r/1000) → nominal = real × 1000 / (1000 + r)
            real * 1000 / (1000 + rate as u64)
        } else {
            real * 1000 / (1000 - (-rate) as u64)
        }
    }

    /// 残差核：补偿后 |comp - nominal| ≤ precision。
    pub fn residual_ok(&self, real: u64, nominal: u64) -> Result<(), u32> {
        let comp = self.compensate(real);
        let diff = if comp > nominal { comp - nominal } else { nominal - comp };
        if diff > self.precision {
            return Err(XC_RESIDUAL);
        }
        Ok(())
    }

    /// 零漂移零干预核：率 0 时补偿恒等。
    pub fn zero_drift_noop(&self, x: u64) -> Result<(), u32> {
        if self.inj.rate_milli == 0 && self.compensate(x) != x {
            return Err(XC_ZERO_DRIFT_ACT);
        }
        Ok(())
    }
}

pub fn run_unx0417_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0417_freqdrift");
    // 族一：快漂 50‰。
    let inj = match DriftInjector::new(50, DriftDir::Fast) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建注入器", false, "拒");
            return cs;
        }
    };
    cs.add("快 50‰ tick", inj.real_ticks(1000) == 1050, "1000 → 1050");
    cs.add("理论误差 +50", inj.theory_error(1000) == 50, "多 50 tick");
    let mut b = match ErrBook::new(inj) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建误差账", false, "拒");
            return cs;
        }
    };
    for _ in 0..10u32 {
        b.step(1000);
    }
    cs.add("累积名义 10000", b.theory_error() == 500, "10 × 50");
    cs.add("实测 = 理论", b.theory_ok().is_ok(), "模型自洽");
    cs.add("误差 50‰ 定性", b.err_gate(100).is_ok(), "≤ 100‰");
    cs.add("误差 10‰ 立案", b.err_gate(10) == Err(XC_ERR_HI), "50 > 10");
    // 族二：慢漂 100‰。
    let slow = match DriftInjector::new(-100, DriftDir::Slow) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建慢注入", false, "拒");
            return cs;
        }
    };
    cs.add("慢 100‰ tick", slow.real_ticks(1000) == 900, "1000 → 900");
    cs.add("理论误差 -100", slow.theory_error(1000) == -100, "少 100");
    let mut s = match ErrBook::new(slow) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建慢账", false, "拒");
            return cs;
        }
    };
    s.step(5000);
    cs.add("慢漂实测 -500", s.measured_error() == -500, "5000 × -0.1");
    cs.add("慢漂理论合", s.theory_ok().is_ok(), "自洽");
    // 族三：补偿器。
    let comp = match Compensator::new(inj, 2) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建补偿器", false, "拒");
            return cs;
        }
    };
    cs.add("补偿回名义", comp.compensate(1050) == 1000, "1050 → 1000");
    cs.add("残差核过", comp.residual_ok(1050, 1000).is_ok(), "差 0 ≤ 2");
    cs.add("慢补偿回名义", Compensator::new(slow, 2).map(|c| c.compensate(900)) == Ok(1000), "900 → 1000");
    cs.add("残差超限立案", comp.residual_ok(1100, 1000) == Err(XC_RESIDUAL), "1100→1047 ≠ 1000");
    // 族四：零漂移零干预。
    let zero = match DriftInjector::new(0, DriftDir::Fast) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建零注入", false, "拒");
            return cs;
        }
    };
    let zc = match Compensator::new(zero, 1) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建零补偿", false, "拒");
            return cs;
        }
    };
    cs.add("零漂 tick 恒等", zero.real_ticks(777) == 777, "不缩放");
    cs.add("零漂补偿恒等", zc.compensate(777) == 777, "零干预");
    cs.add("零干预核过", zc.zero_drift_noop(123).is_ok(), "诚实底线");
    // 零漂误差账恒零。
    let zb = match ErrBook::new(zero) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建零账", false, "拒");
            return cs;
        }
    };
    cs.add("零漂误差 0", zb.measured_error() == 0, "未步进");
    // 族五：参数拒绝。
    cs.add("率超域拒", matches!(DriftInjector::new(501, DriftDir::Fast), Err(XC_RATE_OOR)), "501 > 500");
    cs.add("率负超域拒", matches!(DriftInjector::new(-501, DriftDir::Slow), Err(XC_RATE_OOR)), "-501");
    cs.add("率上限过", DriftInjector::new(500, DriftDir::Fast).is_ok(), "=500");
    cs.add("方向不符拒", matches!(DriftInjector::new(-50, DriftDir::Fast), Err(XC_DIR_OOR)), "负率配快向");
    cs.add("方向不符拒2", matches!(DriftInjector::new(50, DriftDir::Slow), Err(XC_DIR_OOR)), "正率配慢向");
    cs.add("精度零拒", matches!(Compensator::new(inj, 0), Err(XC_COMP_PREC)), "prec=0");
    let eb = match ErrBook::new(zero) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建空账", false, "拒");
            return cs;
        }
    };
    cs.add("空账闸拒", matches!(eb.err_gate(10), Err(XC_STEPS_OOR)), "名义 0");
    cs
}
