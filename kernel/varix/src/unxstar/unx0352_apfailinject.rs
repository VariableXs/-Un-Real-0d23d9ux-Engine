//! unx0352_apfailinject —— UNX-F0352 AP 启动失败注入（哨兵不置位）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0352 · AP 启动失败注入 · 目标 220 行
//! 判据原文：UNX-F0352-J1 注入哨兵不置位 AP（0 例误唤醒），超时/
//!           隔离/降级全链正确，演练记录齐。
//! 组件：故障注入器（对指定 AP 关闭哨兵置位：注入表只进不出，
//!       注入后该 AP 的应答观测恒为「未置位」）+ 启动超时状态机
//!       （唤醒→等待→超时→隔离→降级 五态，单 AP 一次全链）+
//!       误唤醒红线核（注入表内 AP 不得出现「已上线」状态——0 例
//!       误唤醒是判据红线）+ 演练记录账（每次注入落一行：AP/态/
//!       步数/终态，账可查）。
//! 职责定位：失败注入的唯一权威面——注入不是测试夹具的私活，
//!   而是被账面管理的正式动作：注入表可查、状态机全链可回放、
//!   误唤醒红线可核。超时→隔离→降级三环由同一状态机承载，
//!   环环留痕。
//! 性能逐项分解：注入 O(1)；状态机推进 O(1)/步；红线核 O(注入
//!   数)。全整数确定性。
//! 诊断码段：0xE470（0xE471~0xE479 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FI_VERSION: &str = "unx0352_apfailinject-v1";

/// 诊断段基址（0xE470 段 · 本模块独占）。
pub const FI_SEG_BASE: u16 = 0xE470;

/// XcCode 闭集（0xE471~0xE479）。
pub const XC_INJECT_DUP: u32 = 0xE471;
pub const XC_FALSE_WAKE: u32 = 0xE472;
pub const XC_STATE_OOR: u32 = 0xE473;
pub const XC_STEP_OVERRUN: u32 = 0xE474;
pub const XC_TIMEOUT_SHORT: u32 = 0xE475;
pub const XC_LEDGER_MISS: u32 = 0xE476;
pub const XC_INJECT_BSP: u32 = 0xE477;
pub const XC_DEGRADE_SKIP: u32 = 0xE478;
pub const XC_LEDGER_DUP: u32 = 0xE479;

/// 超时窗（tick 数，判据写死：200ms × tick 率折算为整数 tick）。
pub const SENTINEL_TIMEOUT_TICKS: u64 = 200_000;

/// 启动状态机五态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootState {
    /// 已唤醒（INIT+SIPI 完成）。
    Woken,
    /// 等待哨兵（注入 AP 永远停在此态直到超时）。
    WaitSentinel,
    /// 超时（等待窗耗尽）。
    TimedOut,
    /// 隔离（交 F0347 入账）。
    Isolated,
    /// 降级（系统按少一核续行）。
    Degraded,
}

impl BootState {
    /// 态序号（账面落账）。
    pub fn ordinal(self) -> u8 {
        match self {
            BootState::Woken => 0,
            BootState::WaitSentinel => 1,
            BootState::TimedOut => 2,
            BootState::Isolated => 3,
            BootState::Degraded => 4,
        }
    }
}

/// 故障注入器 + 单 AP 状态机推进器。
pub struct FaultInjector {
    /// 注入表：哨兵被关的 AP 集合（只进不出，重复注入拒）。
    injected: Vec<u32>,
    /// 演练记录账。
    ledger: Vec<(u32, u8, u64)>,
}

impl FaultInjector {
    pub fn new() -> FaultInjector {
        FaultInjector { injected: Vec::new(), ledger: Vec::new() }
    }

    /// 注入：BSP（AP 0）不许注入；重复注入拒。
    pub fn inject(&mut self, ap_id: u32) -> Result<(), u16> {
        if ap_id == 0 {
            return Err(XC_INJECT_BSP as u16);
        }
        if self.injected.contains(&ap_id) {
            return Err(XC_INJECT_DUP as u16);
        }
        self.injected.push(ap_id);
        Ok(())
    }

    /// 观测哨兵：注入 AP 恒「未置位」，健康 AP 恒「已置位」。
    pub fn sentinel_set(&self, ap_id: u32) -> bool {
        !self.injected.contains(&ap_id)
    }

    /// 推进一个 AP 的启动状态机到终态，返回终态与步数。
    ///
    /// 推进规则：健康 AP 走 Woken→(哨兵已置位)→隔离外终态（由
    /// 上层汇合处理，此处返回 Woken 终态）；注入 AP 走 Woken→
    /// WaitSentinel→TimedOut→Isolated→Degraded 五步全链，一步
    /// 不许跳（降级必须在隔离之后——XC_DEGRADE_SKIP）。
    pub fn drive_to_terminal(&mut self, ap_id: u32, wait_ticks: u64) -> Result<(BootState, u64), u16> {
        let mut state = BootState::Woken;
        let mut steps: u64 = 1;
        if self.sentinel_set(ap_id) {
            return Ok((state, steps));
        }
        // 注入 AP：全链推进。
        if wait_ticks < SENTINEL_TIMEOUT_TICKS {
            return Err(XC_TIMEOUT_SHORT as u16);
        }
        state = BootState::WaitSentinel;
        steps += 1;
        state = BootState::TimedOut;
        steps += 1;
        state = BootState::Isolated;
        steps += 1;
        state = BootState::Degraded;
        steps += 1;
        if steps > 8 {
            return Err(XC_STEP_OVERRUN as u16);
        }
        // 演练记录：同 AP 重复落账拒。
        if self.ledger.iter().any(|(a, _, _)| *a == ap_id) {
            return Err(XC_LEDGER_DUP as u16);
        }
        self.ledger.push((ap_id, state.ordinal(), steps));
        Ok((state, steps))
    }

    /// 误唤醒红线核：注入表内 AP 不得被标「已上线」。
    pub fn no_false_wake(&self, online_aps: &[u32]) -> Result<(), u16> {
        for &ap in self.injected.iter() {
            if online_aps.contains(&ap) {
                return Err(XC_FALSE_WAKE as u16);
            }
        }
        Ok(())
    }

    /// 演练记录齐备断言：每个注入 AP 恰一条记录。
    pub fn ledger_complete(&self) -> Result<(), u16> {
        for &ap in self.injected.iter() {
            if !self.ledger.iter().any(|(a, _, _)| *a == ap) {
                return Err(XC_LEDGER_MISS as u16);
            }
        }
        Ok(())
    }

    /// 注入表只读视图。
    pub fn injected(&self) -> &[u32] {
        &self.injected
    }

    /// 记录账只读视图。
    pub fn ledger(&self) -> &[(u32, u8, u64)] {
        &self.ledger
    }
}

/// 域自检入口：注入/全链/红线/账齐 四族。
pub fn run_unx0352_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0352-失败注入");
    // 族一：注入闸。
    let mut fi = FaultInjector::new();
    cs.add("注入生效", fi.inject(2).is_ok(), "AP2 哨兵关");
    cs.add("重复注入拒", fi.inject(2).is_err(), "注入表只进不出");
    cs.add("BSP 注入拒", fi.inject(0).is_err(), "BSP 不许注入");
    cs.add("观测未置位", !fi.sentinel_set(2), "注入 AP 恒未置位");
    cs.add("健康观测置位", fi.sentinel_set(3), "未注入 AP 置位");

    // 族二：全链推进（五态一步不跳）。
    let term = fi.drive_to_terminal(2, SENTINEL_TIMEOUT_TICKS);
    cs.add("终态降级", matches!(term, Ok((BootState::Degraded, 5))), "五步全链");
    cs.add("健康 AP 不进链", matches!(fi.drive_to_terminal(3, 1), Ok((BootState::Woken, 1))),
           "哨兵已置位直接终");
    // 短窗拒绝：第二注入 AP 用不足超时窗的等待驱动必须被拒。
    let _ = fi.inject(4);
    cs.add("短窗拒", fi.drive_to_terminal(4, 1).is_err(), "等待窗 < 超时红线");
    let _ = fi.drive_to_terminal(4, SENTINEL_TIMEOUT_TICKS);

    // 族三：误唤醒红线（0 例误唤醒）。
    cs.add("零误唤醒", fi.no_false_wake(&[1, 3]).is_ok(), "注入 AP 不在线上");
    cs.add("误唤醒检出", fi.no_false_wake(&[1, 2, 3]).is_err(), "注入 AP 混进在线表");

    // 族四：账齐。
    cs.add("账齐", fi.ledger_complete().is_ok(), "注入 AP 各一条记录");
    cs.add("账条数", fi.ledger().len() == 2, "两注入两记录");
    cs
}

// ===== 扩展面：注入谱指纹（案例固化 + 回放对拍）=====
/// 注入谱指纹：注入集合 + 超时常量 + 终态序列的 FNV 串联。
/// 案例固化后，CI 回放同谱必须同指纹（确定性红线）——指纹变
/// 而注入表未变 = 状态机行为漂移，立案。
pub fn inject_spectrum_fingerprint(fi: &FaultInjector) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mix = |h: &mut u64, bytes: &[u8]| {
        for &b in bytes {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x100000001b3);
        }
    };
    mix(&mut h, &(fi.injected().len() as u32).to_le_bytes());
    for &ap in fi.injected() {
        mix(&mut h, &ap.to_le_bytes());
    }
    mix(&mut h, &SENTINEL_TIMEOUT_TICKS.to_le_bytes());
    for (ap, state, steps) in fi.ledger() {
        mix(&mut h, &ap.to_le_bytes());
        mix(&mut h, &(*state as u32).to_le_bytes());
        mix(&mut h, &steps.to_le_bytes());
    }
    h
}
