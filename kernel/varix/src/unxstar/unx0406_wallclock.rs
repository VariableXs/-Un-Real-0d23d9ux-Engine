//! unx0406_wallclock —— UNX-F0406 墙钟偏移账（RTC 只读取值）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0406 · 墙钟偏移账 · 目标 220 行
//! 判据原文：UNX-F0406-J1 墙钟 = RTC 只读取值 + 单调偏移；
//!           内核永不回写 RTC（只读纪律：回写企图立案）；
//!           偏移账支持 set（用户校时）与 nudge（微调）两种
//!           变更，变更流水可回放（同流水同终值）；墙钟单调
//!           核：两次读值后者 ≥ 前者（偏移只许正向演进时），
//!           任何回退必须走显式 set（回退立案可定位）。
//! 组件：RTC 快照（只读）+ 偏移账（当前偏移 + 变更流水）+
//!       墙钟读取器（RTC + 偏移合成）+ 单调核 + 只读纪律闸。
//! 职责定位：墙钟偏移账是「时区的单一真源」——RTC 是硬件给
//!   的底值，内核只叠加自己的偏移（时区/校准）；只读纪律是
//!   底线：回写 RTC 会破坏双系统时钟共识，任何回写企图都要
//!   留案底。偏移流水可回放保证校时历史可审计。
//! 性能逐项分解：读墙钟 O(1)；set/nudge O(1)；回放 O(流水长)。
//! 诊断码段：0x20F00（0x20F01~0x20F09 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const WC_VERSION: &str = "unx0406_wallclock-v1";

/// 诊断段基址（0x20F00 段 · 本模块独占）。
pub const WC_SEG_BASE: u32 = 0x20F00;
pub const XC_RTC_WRITE: u32 = 0x20F01; // RTC 回写企图
pub const XC_MONO_VIOL: u32 = 0x20F02; // 单调违例
pub const XC_NUDGE_BIG: u32 = 0x20F03; // 微调超幅
pub const XC_SEQ_REV: u32 = 0x20F04; // 流水步号回退
pub const XC_SET_REV: u32 = 0x20F05; // set 回退未立案
pub const XC_REPLAY: u32 = 0x20F06; // 流水回放不一致
pub const XC_OFF_OVF: u32 = 0x20F07; // 偏移溢出
pub const XC_RTC_OOR: u32 = 0x20F08; // RTC 读值越域
pub const XC_MAGIC: u32 = 0x20F09; // 快照魔数破

/// RTC 快照（只读底值，模型内为秒计）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RtcSnap {
    /// 魔数（只读标记）。
    pub magic: u32,
    /// 底值秒。
    pub base: u64,
}

/// 偏移变更流水行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OffRow {
    /// 步号。
    pub step: u64,
    /// 新偏移（i64，可为负）。
    pub off: i64,
    /// 变更种别：1 = set（显式校时）2 = nudge（微调）。
    pub kind: u8,
}

/// 墙钟账。
pub struct WallClock {
    snap: RtcSnap,
    cur_off: i64,
    rows: Vec<OffRow>,
    next_step: u64,
    /// set 回退立案数。
    regress_cases: u32,
}

/// 微调幅度上限（毫秒模型，此处秒）。
pub const NUDGE_MAX: i64 = 2;

impl WallClock {
    /// 建：RTC 底值。
    pub fn new(base: u64) -> Result<WallClock, u32> {
        if base == 0 || base > 4_102_444_800 {
            return Err(XC_RTC_OOR); // > 2100-01-01 视为越域
        }
        Ok(WallClock {
            snap: RtcSnap { magic: 0x5743_0001, base },
            cur_off: 0,
            rows: Vec::new(),
            next_step: 1,
            regress_cases: 0,
        })
    }

    /// 读墙钟 = RTC 底值 + 当前偏移。
    pub fn read(&self) -> i64 {
        self.snap.base as i64 + self.cur_off
    }

    /// 读 RTC 底值（只读）。
    pub fn rtc_base(&self) -> u64 {
        self.snap.base
    }

    /// 显式校时：设绝对偏移；若导致墙钟回退则立案（允许但记案）。
    pub fn set(&mut self, new_off: i64) -> Result<i64, u32> {
        if new_off.abs() > 86_400 {
            return Err(XC_OFF_OVF); // 偏移超 ±1 天拒
        }
        let prev = self.read();
        let now = self.snap.base as i64 + new_off;
        if now < prev {
            self.regress_cases += 1; // 回退立案
        }
        self.cur_off = new_off;
        let row = OffRow { step: self.next_step, off: new_off, kind: 1 };
        self.next_step += 1;
        self.rows.push(row);
        Ok(now)
    }

    /// 微调：增量变更，幅度限 NUDGE_MAX。
    pub fn nudge(&mut self, delta: i64) -> Result<i64, u32> {
        if delta.abs() > NUDGE_MAX {
            return Err(XC_NUDGE_BIG);
        }
        let new_off = self.cur_off + delta;
        self.cur_off = new_off;
        let row = OffRow { step: self.next_step, off: new_off, kind: 2 };
        self.next_step += 1;
        self.rows.push(row);
        Ok(self.read())
    }

    /// RTC 回写企图（纪律闸：恒拒并立案）。
    pub fn rtc_write_attempt(&mut self) -> Result<(), u32> {
        Err(XC_RTC_WRITE)
    }

    /// 墙钟单调核：给定两次读值（偏移只增不减的时段），后者 ≥ 前者。
    pub fn mono_ok(prev: i64, cur: i64) -> Result<(), u32> {
        if cur < prev {
            return Err(XC_MONO_VIOL);
        }
        Ok(())
    }

    /// 流水回放：从零偏移重演全部 set 行（nudge 行叠加），终值须等于当前偏移。
    pub fn replay(&self) -> Result<i64, u32> {
        let mut off: i64 = 0;
        let mut prev_step: u64 = 0;
        for r in self.rows.iter() {
            if r.step <= prev_step {
                return Err(XC_SEQ_REV);
            }
            prev_step = r.step;
            match r.kind {
                1 => off = r.off,           // set：绝对
                2 => off += 0,              // nudge 行存的是新偏移，不叠加
                _ => return Err(XC_MAGIC),
            }
        }
        if off != self.cur_off {
            return Err(XC_REPLAY);
        }
        Ok(off)
    }

    /// 流水长。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 回退立案数。
    pub fn regress_count(&self) -> u32 {
        self.regress_cases
    }

    /// 快照魔数核。
    pub fn magic_ok(&self) -> Result<(), u32> {
        if self.snap.magic != 0x5743_0001 {
            return Err(XC_MAGIC);
        }
        Ok(())
    }
}

pub fn run_unx0406_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0406_wallclock");
    // 族一：标准读值与校时。
    let mut w = match WallClock::new(1_700_000_000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账", false, "拒");
            return cs;
        }
    };
    cs.add("RTC 底值", w.rtc_base() == 1_700_000_000, "只读底值");
    cs.add("零偏读值", w.read() == 1_700_000_000, "初值即底值");
    let v1 = w.set(3600);
    cs.add("set +1h", v1 == Ok(1_700_003_600), "墙钟进 1 小时");
    cs.add("读值合偏移", w.read() == 1_700_003_600, "底值 + 偏移");
    let v2 = w.nudge(1);
    cs.add("nudge +1s", v2 == Ok(1_700_003_601), "微调生效");
    cs.add("流水 2 行", w.row_count() == 2, "set + nudge");
    cs.add("回放终值合", w.replay() == Ok(3601), "重演一致");
    cs.add("魔数过", w.magic_ok().is_ok(), "快照完好");
    // 族二：单调核。
    cs.add("正向单调过", WallClock::mono_ok(100, 200).is_ok(), "200 ≥ 100");
    cs.add("相等单调过", WallClock::mono_ok(200, 200).is_ok(), "恒等");
    cs.add("回退检出", WallClock::mono_ok(200, 100) == Err(XC_MONO_VIOL), "100 < 200");
    // 族三：set 回退立案。
    let r3 = w.set(0);
    cs.add("set 回退成", r3 == Ok(1_700_000_000), "允许但立案");
    cs.add("立案数 1", w.regress_count() == 1, "一次回退一案");
    // 族四：RTC 回写企图恒拒。
    cs.add("回写企图拒", w.rtc_write_attempt() == Err(XC_RTC_WRITE), "只读纪律");
    // 族五：参数拒绝。
    cs.add("微调超幅拒", w.nudge(3) == Err(XC_NUDGE_BIG), "+3 > 2");
    cs.add("微调负超幅拒", w.nudge(-3) == Err(XC_NUDGE_BIG), "-3");
    cs.add("偏移超天拒", w.set(86_401) == Err(XC_OFF_OVF), "> ±1 天");
    cs.add("RTC 底值零拒", matches!(WallClock::new(0), Err(XC_RTC_OOR)), "base=0");
    cs.add("RTC 底值越域拒", matches!(WallClock::new(4_102_444_801), Err(XC_RTC_OOR)), ">2100");
    // 族六：流水回放核（多行混合）。
    let mut m = match WallClock::new(1_700_000_000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账2", false, "拒");
            return cs;
        }
    };
    let _ = m.set(100);
    let _ = m.nudge(1);
    let _ = m.nudge(1);
    let _ = m.set(500);
    let _ = m.nudge(-1);
    cs.add("五行流水", m.row_count() == 5, "2 set + 3 nudge");
    cs.add("回放 499", m.replay() == Ok(499), "set 500 + nudge -1");
    cs.add("读值 499", m.read() == 1_700_000_499, "底值 + 499");
    cs
}
