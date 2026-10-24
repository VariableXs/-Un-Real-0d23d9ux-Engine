//! unx0408_tscdead —— UNX-F0408 TSC deadline 模式占位。
//! 锚点（CoRun Unxreal 主册）：UNX-F0408 · TSC deadline 模式占位 · 目标 220 行
//! 判据原文：UNX-F0408-J1 TSC deadline 模式是「占位面」：能力
//!           位探测先行（CPUID 位缺失即占位态）；占位态下所有
//!           操作显式拒绝（拒绝码带占位标记）不半途执行；能力
//!           就绪后模式激活（deadline 值写入受理）；模式切换
//!           留痕（占位 → 激活单向，激活后不可退回占位）；频
//!           率换算账（TSC tick ↔ 纳秒，比例可配、非零核）。
//! 组件：能力位表（探测结果 + 占位态）+ 模式账（态机：占位/
//!       激活单向）+ deadline 寄存器模型（写入受理/拒绝）+
//!       频率换算器（tick ↔ ns 双向）+ 切换留痕账。
//! 职责定位：TSC deadline 是「最省电的定时方式」（写 MSR 而非
//!   数 PIT tick），但依赖 CPUID 能力位；占位面保证能力缺失的
//!   机器上行为显式可判（拒绝码而不是静默失败）；单向态机防
//!   「激活后又退回占位」的语义混乱——能力位不会消失，模式
//!   就不该回退。
//! 性能逐项分解：探测 O(1)；写入受理/拒绝 O(1)；换算 O(1)。
//! 诊断码段：0x20F80（0x20F81~0x20F89 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const TD_VERSION: &str = "unx0408_tscdead-v1";

/// 诊断段基址（0x20F80 段 · 本模块独占）。
pub const TD_SEG_BASE: u32 = 0x20F80;
pub const XC_NOT_PRESENT: u32 = 0x20F81; // 占位态操作拒
pub const XC_FREQ_ZERO: u32 = 0x20F82; // 频率比例零拒
pub const XC_ALREADY_ON: u32 = 0x20F83; // 重复激活
pub const XC_DL_REV: u32 = 0x20F84; // deadline 回退
pub const XC_NS_OVF: u32 = 0x20F85; // 换算溢出
pub const XC_MODE_BAD: u32 = 0x20F86; // 态机非法跳转
pub const XC_TRACE_LOST: u32 = 0x20F87; // 切换留痕缺失
pub const XC_FREQ_BIG: u32 = 0x20F88; // 频率超域
pub const XC_MAGIC: u32 = 0x20F89; // 能力表魔数破

/// 模式两态（单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdMode {
    /// 占位：能力未就绪或未激活。
    Placeholder,
    /// 激活。
    Active,
}

/// 能力位表。
#[derive(Clone, Copy, Debug)]
pub struct CapBits {
    /// 魔数。
    pub magic: u32,
    /// CPUID TSC-deadline 位（bit 24 of leaf 0x80000007 模型）。
    pub tsc_deadline: bool,
    /// 不变量 TSC 位。
    pub invariant_tsc: bool,
}

/// TSC deadline 模式账。
pub struct TscDeadline {
    caps: CapBits,
    mode: TdMode,
    /// 当前 deadline（TSC tick）。
    deadline: u64,
    /// TSC 频率（Hz 模型，tick/秒）。
    freq_hz: u64,
    /// 切换留痕：(步号, 从态, 到态)。
    trace: Vec<(u64, u8, u8)>,
    step: u64,
}

/// 频率上限（10 GHz 模型）。
pub const FREQ_MAX: u64 = 10_000_000_000;

impl TscDeadline {
    /// 建：探测结果 + 频率。
    pub fn new(tsc_deadline: bool, invariant_tsc: bool, freq_hz: u64) -> Result<TscDeadline, u32> {
        if freq_hz == 0 {
            return Err(XC_FREQ_ZERO);
        }
        if freq_hz > FREQ_MAX {
            return Err(XC_FREQ_BIG);
        }
        Ok(TscDeadline {
            caps: CapBits { magic: 0x5444_0001, tsc_deadline, invariant_tsc },
            mode: TdMode::Placeholder,
            deadline: 0,
            freq_hz,
            trace: Vec::new(),
            step: 1,
        })
    }

    /// 占位态判定。
    pub fn is_placeholder(&self) -> bool {
        self.mode == TdMode::Placeholder
    }

    /// 激活：能力位齐才受理；已激活拒；激活留痕。
    pub fn activate(&mut self) -> Result<(), u32> {
        if !self.caps.tsc_deadline || !self.caps.invariant_tsc {
            return Err(XC_NOT_PRESENT);
        }
        if self.mode == TdMode::Active {
            return Err(XC_ALREADY_ON);
        }
        self.mode = TdMode::Active;
        self.trace.push((self.step, 0, 1));
        self.step += 1;
        Ok(())
    }

    /// 退回占位（态机非法跳转恒拒）。
    pub fn deactivate(&mut self) -> Result<(), u32> {
        Err(XC_MODE_BAD)
    }

    /// deadline 写入：占位态拒；回退拒；受理留当前值。
    pub fn set_deadline(&mut self, tick: u64) -> Result<u64, u32> {
        if self.mode == TdMode::Placeholder {
            return Err(XC_NOT_PRESENT);
        }
        if tick < self.deadline {
            return Err(XC_DL_REV);
        }
        self.deadline = tick;
        Ok(tick)
    }

    /// 当前 deadline。
    pub fn deadline(&self) -> u64 {
        self.deadline
    }

    /// tick → ns 换算（截断）。
    pub fn tick_to_ns(&self, tick: u64) -> Result<u64, u32> {
        if self.freq_hz == 0 {
            return Err(XC_FREQ_ZERO);
        }
        // ns = tick * 1e9 / freq
        let ns = (tick / self.freq_hz).saturating_mul(1_000_000_000)
            + ((tick % self.freq_hz) * 1_000_000_000 / self.freq_hz);
        if ns > u64::MAX / 2 {
            return Err(XC_NS_OVF);
        }
        Ok(ns)
    }

    /// ns → tick 换算（截断）。
    pub fn ns_to_tick(&self, ns: u64) -> Result<u64, u32> {
        if self.freq_hz == 0 {
            return Err(XC_FREQ_ZERO);
        }
        Ok(ns / 1_000_000_000 * self.freq_hz + ns % 1_000_000_000 * self.freq_hz / 1_000_000_000)
    }

    /// 留痕核：激活过必有 0→1 痕迹。
    pub fn trace_ok(&self) -> Result<(), u32> {
        if self.mode == TdMode::Active {
            for &(_, from, to) in self.trace.iter() {
                if from == 0 && to == 1 {
                    return Ok(());
                }
            }
            return Err(XC_TRACE_LOST);
        }
        Ok(())
    }

    /// 留痕数。
    pub fn trace_count(&self) -> usize {
        self.trace.len()
    }

    /// 能力魔数核。
    pub fn magic_ok(&self) -> Result<(), u32> {
        if self.caps.magic != 0x5444_0001 {
            return Err(XC_MAGIC);
        }
        Ok(())
    }
}

pub fn run_unx0408_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0408_tscdead");
    // 族一：占位态（能力缺失）。
    let mut p = match TscDeadline::new(false, true, 2_400_000_000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建占位", false, "拒");
            return cs;
        }
    };
    cs.add("初态占位", p.is_placeholder(), "能力位缺");
    cs.add("占位激活拒", p.activate() == Err(XC_NOT_PRESENT), "缺 deadline 位");
    cs.add("占位写 deadline 拒", p.set_deadline(1000) == Err(XC_NOT_PRESENT), "占位拒写");
    cs.add("占位退回拒", p.deactivate() == Err(XC_MODE_BAD), "恒拒");
    // 族二：能力齐但未激活。
    let mut q = match TscDeadline::new(true, true, 2_400_000_000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建未激活", false, "拒");
            return cs;
        }
    };
    cs.add("未激活仍占位", q.is_placeholder(), "待激活");
    cs.add("未激活写拒", q.set_deadline(1) == Err(XC_NOT_PRESENT), "先激活");
    cs.add("激活成", q.activate().is_ok(), "单向切换");
    cs.add("激活态", !q.is_placeholder(), "Active");
    cs.add("重复激活拒", q.activate() == Err(XC_ALREADY_ON), "已开");
    cs.add("留痕 1", q.trace_count() == 1, "0→1");
    cs.add("留痕核过", q.trace_ok().is_ok(), "痕迹在");
    // 族三：deadline 写入受理与回退拒。
    cs.add("写 1000 受理", q.set_deadline(1000) == Ok(1000), "激活后可写");
    cs.add("写 2000 受理", q.set_deadline(2000) == Ok(2000), "递增");
    cs.add("当前值 2000", q.deadline() == 2000, "寄存器模型");
    cs.add("回退拒", q.set_deadline(1999) == Err(XC_DL_REV), "1999 < 2000");
    cs.add("平写受理", q.set_deadline(2000) == Ok(2000), "等值可写");
    // 激活后退回占位恒拒（单向）。
    cs.add("激活后退回拒", q.deactivate() == Err(XC_MODE_BAD), "单向态机");
    // 族四：频率换算。
    cs.add("1 秒 tick", q.tick_to_ns(2_400_000_000) == Ok(1_000_000_000), "2.4G → 1s");
    cs.add("半秒 tick", q.tick_to_ns(1_200_000_000) == Ok(500_000_000), "→ 500ms");
    cs.add("ns → tick", q.ns_to_tick(1_000_000_000) == Ok(2_400_000_000), "1s → 2.4G");
    cs.add("小 tick 截断", q.tick_to_ns(1) == Ok(0), "1 tick < 1ns 档截断");
    // 族五：频率参数拒绝。
    cs.add("频率零拒", matches!(TscDeadline::new(true, true, 0), Err(XC_FREQ_ZERO)), "0 Hz");
    cs.add("频率超域拒", matches!(TscDeadline::new(true, true, FREQ_MAX + 1), Err(XC_FREQ_BIG)), ">10G");
    // 族六：能力位部分缺失。
    let mut r = match TscDeadline::new(true, false, 2_400_000_000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建部分缺", false, "拒");
            return cs;
        }
    };
    cs.add("缺不变 TSC 拒激活", r.activate() == Err(XC_NOT_PRESENT), "invariant 缺");
    cs.add("占位保持", r.is_placeholder(), "未误开");
    cs.add("魔数核", q.magic_ok().is_ok(), "能力表完好");
    cs
}
