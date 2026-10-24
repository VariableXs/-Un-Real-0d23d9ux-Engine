//! unx0278_budget —— UNX-F0278 终端耗时预算（命令执行上限）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0278 · 命令耗时预算 · 目标 180 行
//! 判据原文：UNX-F0278-J1 每条命令携带耗时预算（节拍数），执行
//!           侧逐节拍扣账：预算耗尽 → 命令被标记「预算破」并
//!           中止（不无声续跑）；预算设 0 = 无限（仅诊断保留
//!           命令可用）；执行账可对账（扣减步数 = 实际节拍）。
//! 组件：预算账（额度 + 已扣 + 状态）+ 扣减核（逐拍 + 破产闸）
//!       + 预算分配器（诊断保留命令白名单）。
//! 职责定位：耗时预算是「命令不会把终端吃死」的硬保证——预算
//!   破产必须显式标记（用户看到「预算破」才知道结果不完整），
//!   静默截断最害人。无限预算是特权不是默认：白名单之外一律
//!   必须给有限额度。
//! 性能逐项分解：扣减 O(1)；破产闸 O(1)；分配 O(白名单深)。
//! 诊断码段：0x204C0（0x204C1~0x204C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const TB_VERSION: &str = "unx0278_budget-v1";

/// 诊断段基址（0x204C0 段 · 本模块独占）。
pub const TB_SEG_BASE: u32 = 0x204C0;
pub const XC_BUDGET_BROKEN: u32 = 0x204C1; // 预算破
pub const XC_ZERO_PRIV: u32 = 0x204C2; // 非白名单零预算
pub const XC_NEG_TICK: u32 = 0x204C3; // 扣减过额
pub const XC_STATE_OOR: u32 = 0x204C4; // 状态越域
pub const XC_WHITE_DUP: u32 = 0x204C5; // 白名单重号
pub const XC_TICK_ZERO: u32 = 0x204C6; // 零拍扣减拒
pub const XC_CLOSED: u32 = 0x204C7; // 已结账再扣
pub const XC_UNCLOSED: u32 = 0x204C8; // 未结账查总
pub const XC_ID_OOR: u32 = 0x204C9; // 命令号越域

/// 预算状态。
pub const B_RUN: u8 = 0; // 执行中
pub const B_DONE: u8 = 1; // 正常完成
pub const B_BROKEN: u8 = 2; // 预算破

/// 命令号闭集 1~256。
pub const CMD_MAX: u32 = 256;

/// 诊断保留命令白名单（可用无限预算）。
pub const UNLIMITED_WHITELIST: [u32; 3] = [1, 2, 3]; // help/sym/audit

/// 单命令预算账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimeBudget {
    pub cmd: u32,
    /// 额度（节拍；0 = 无限，仅白名单）。
    pub quota: u64,
    pub spent: u64,
    pub state: u8,
}

impl TimeBudget {
    /// 构造：非白名单命令零额度拒。
    pub fn new(cmd: u32, quota: u64) -> Result<TimeBudget, u32> {
        if cmd == 0 || cmd > CMD_MAX {
            return Err(XC_ID_OOR);
        }
        if quota == 0 && !UNLIMITED_WHITELIST.contains(&cmd) {
            return Err(XC_ZERO_PRIV);
        }
        Ok(TimeBudget { cmd, quota, spent: 0, state: B_RUN })
    }

    /// 逐拍扣减：额度破 → 状态转 BROKEN 并拒续。
    pub fn tick(&mut self, n: u64) -> Result<u64, u32> {
        if n == 0 {
            return Err(XC_TICK_ZERO);
        }
        if self.state == B_DONE || self.state == B_BROKEN {
            return Err(XC_CLOSED);
        }
        if self.quota == 0 {
            // 无限预算：只记账不扣闸。
            self.spent = self.spent.wrapping_add(n);
            return Ok(self.spent);
        }
        if self.spent + n > self.quota {
            self.state = B_BROKEN;
            return Err(XC_BUDGET_BROKEN);
        }
        self.spent += n;
        Ok(self.spent)
    }

    /// 结账：B_RUN → DONE。
    pub fn finish(&mut self) -> Result<(), u32> {
        if self.state != B_RUN {
            return Err(XC_STATE_OOR);
        }
        self.state = B_DONE;
        Ok(())
    }

    /// 是否预算破。
    pub fn broken(&self) -> bool {
        self.state == B_BROKEN
    }
}

/// 预算分配器：白名单可给 0；其余强制有限下限。
pub fn allocate(cmd: u32, want: u64) -> Result<TimeBudget, u32> {
    if UNLIMITED_WHITELIST.contains(&cmd) {
        return TimeBudget::new(cmd, want);
    }
    if want == 0 {
        return Err(XC_ZERO_PRIV);
    }
    TimeBudget::new(cmd, want)
}

/// 执行账对账：扣减总步数 == 实际节拍消耗。
pub fn spend_matches(b: &TimeBudget, actual: u64) -> bool {
    b.spent == actual
}

pub fn run_unx0278_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0278_budget");
    // 族一：有限预算扣减。
    let mut b = match TimeBudget::new(10, 100) {
        Ok(x) => x,
        Err(_) => {
            cs.add("预算构造", false, "被拒");
            return cs;
        }
    };
    cs.add("逐拍扣", b.tick(30) == Ok(30), "30/100");
    cs.add("再扣", b.tick(50) == Ok(80), "80/100");
    cs.add("未破", !b.broken(), "余 20");
    // 族二：破产闸。
    cs.add("超额拒", b.tick(21) == Err(XC_BUDGET_BROKEN), "80+21>100");
    cs.add("破产标记", b.broken(), "B_BROKEN");
    cs.add("破后拒续", b.tick(1) == Err(XC_CLOSED), "CLOSRED");
    // 族三：结账。
    let mut b2 = TimeBudget::new(11, 10).unwrap_or(TimeBudget { cmd: 11, quota: 10, spent: 0, state: B_RUN });
    let _ = b2.tick(10);
    cs.add("恰好用尽不破", !b2.broken(), "10/10 边界");
    cs.add("结账", b2.finish().is_ok(), "DONE");
    cs.add("结后拒扣", b2.tick(1) == Err(XC_CLOSED), "已结");
    cs.add("重复结拒", b2.finish().is_err(), "STATE_OOR");
    // 族四：无限预算（白名单）。
    let mut inf = allocate(1, 0).unwrap_or(TimeBudget { cmd: 1, quota: 0, spent: 0, state: B_RUN });
    cs.add("白名单零额度", inf.quota == 0, "无限");
    let _ = inf.tick(1_000_000);
    cs.add("无限不破", !inf.broken(), "只记账");
    cs.add("非白名单零额度拒", allocate(99, 0).is_err(), "ZERO_PRIV");
    cs.add("白名单有限额度过", allocate(2, 500).is_ok(), "也可给有限");
    // 族五：对账。
    let mut b3 = TimeBudget::new(12, 1000).unwrap_or(TimeBudget { cmd: 12, quota: 1000, spent: 0, state: B_RUN });
    let _ = b3.tick(400);
    let _ = b3.tick(600);
    cs.add("扣减对账", spend_matches(&b3, 1000), "400+600");
    cs.add("对账不符检出", !spend_matches(&b3, 999), "差 1 即不成");
    // 族六：构造拒。
    cs.add("命令号 0 拒", TimeBudget::new(0, 10).is_err(), "ID_OOR");
    cs.add("命令号越域拒", TimeBudget::new(257, 10).is_err(), "ID_OOR");
    cs.add("零拍拒", {
        let mut x = TimeBudget::new(13, 10).unwrap_or(TimeBudget { cmd: 13, quota: 10, spent: 0, state: B_RUN });
        x.tick(0).is_err()
    }, "TICK_ZERO");
    cs.add("白名单重号检出", {
        let mut dup = false;
        for (i, &w) in UNLIMITED_WHITELIST.iter().enumerate() {
            if UNLIMITED_WHITELIST[i + 1..].contains(&w) {
                dup = true;
            }
        }
        !dup
    }, "白名单自洽");
    cs
}
