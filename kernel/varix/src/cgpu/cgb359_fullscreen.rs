//! CGPU-F0359 · 全屏互动场景配方（判据段 0x08D0）
//! 全屏配方：全屏互动授予（全屏应用最大预算——游戏/演示场景）；
//! 系统保底线（系统 UI/输入响应的不可侵占保底）；全屏退出恢复
//! （退场即恢复——无残留）；与 J 域功耗预留（全屏高负载的功耗标
//! 注位）。判据：最大授予、保底线、恢复、功耗预留、判据。
//!
//! 锚点原文摘录（施工书 1599 行）：
//! 「全屏互动授予（全屏应用最大预算）；系统保底线（系统 UI/输入
//! 响应的不可侵占保底）；全屏退出恢复（退场即恢复——无残留）；
//! 与 J 域功耗预留（全屏高负载的功耗标注位）。判据：最大授予、
//! 保底线、恢复、功耗预留、判据。」
//!
//! 诊断码段 0x08D0（本模块独占：0x08D0~0x08DF）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x08D0;

/// 全屏最大授予（千分比）：全屏互动独占帧预算的上限 = 920。
/// 来源：判据「最大授予」——永远不给 1000，系统保底必须先扣。
pub const FULLSCREEN_GRANT_MAX_PERMILLE: u32 = 920;

/// 系统保底线（千分比）：系统 UI 300 + 输入响应 100 = 400 不可侵占。
/// 来源：判据「保底线」——输入响应比 UI 更薄但同样不可侵占。
pub const SYS_UI_FLOOR_PERMILLE: u32 = 300;
pub const INPUT_FLOOR_PERMILLE: u32 = 100;

/// 保底合计（编译期可验：300+100 = 400，与授予上限 920 的关系是
/// 1000-920 = 80 的余量来自非保底的杂项回收，保底本身不动）。
pub const SYS_FLOOR_TOTAL_PERMILLE: u32 = SYS_UI_FLOOR_PERMILLE + INPUT_FLOOR_PERMILLE;

/// 功耗标注位（判据「功耗预留」）：全屏高负载档 J 域功耗预留档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerReserve {
    /// 无预留（低负载全屏）。
    None,
    /// 标准预留（典型游戏负载）。
    Standard,
    /// 高预留（重负载全屏）。
    Heavy,
}

impl PowerReserve {
    /// 档位号。
    pub fn ordinal(&self) -> u8 {
        match self {
            PowerReserve::None => 0,
            PowerReserve::Standard => 1,
            PowerReserve::Heavy => 2,
        }
    }
    /// 预留功耗份额（千分比，从全屏授予内切出）。
    pub fn reserve_permille(&self) -> u32 {
        match self {
            PowerReserve::None => 0,
            PowerReserve::Standard => 120,
            PowerReserve::Heavy => 250,
        }
    }
}

/// 全屏会话（判据「最大授予」的授予对象）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FullscreenSession {
    /// 会话号（0 非法）。
    pub session: u32,
    /// 负载类别：true=重负载（游戏）false=轻负载（演示）。
    pub heavy: bool,
    /// J 域功耗预留档。
    pub reserve: PowerReserve,
}

impl FullscreenSession {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.session == 0 {
            return Err(SEG_BASE);
        }
        if self.heavy && self.reserve == PowerReserve::None {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// 全屏授予计算（判据「最大授予」主判据）：授予 = 1000 − 保底合计
/// − 功耗预留，且不超 FULLSCREEN_GRANT_MAX_PERMILLE。重负载会话
/// 必须带功耗预留（不带即拒——判据「功耗预留」联动硬闸）。
pub fn fullscreen_grant(s: &FullscreenSession) -> Result<u32, u16> {
    s.validate()?;
    let grant = 1000u32
        .saturating_sub(SYS_FLOOR_TOTAL_PERMILLE)
        .saturating_sub(s.reserve.reserve_permille());
    if grant > FULLSCREEN_GRANT_MAX_PERMILLE {
        return Err(SEG_BASE + 2);
    }
    if grant == 0 {
        return Err(SEG_BASE + 3);
    }
    Ok(grant)
}

/// 系统保底核（判据「保底线」主判据）：任何全屏授予下，系统 UI
/// 与输入响应的份额必须完整保留——授予 + 保底 ≤ 1000 且保底两
/// 项一分不少。
pub fn floor_intact(grant_permille: u32) -> Result<(), u16> {
    if grant_permille == 0 {
        return Err(SEG_BASE + 4);
    }
    if grant_permille + SYS_FLOOR_TOTAL_PERMILLE > 1000 {
        return Err(SEG_BASE + 5);
    }
    Ok(())
}

/// 授予账（判据「恢复」的对账单元）：进场合账 + 退场快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrantLedger {
    /// 进场前各应用份额快照（表面号, 份额千分比）——最多 8 行。
    pub before: [(u32, u32); 8],
    /// 进场前有效行数。
    pub before_len: usize,
    /// 全屏会话拿走的量。
    pub taken_permille: u32,
    /// 是否已退场。
    pub exited: bool,
}

impl GrantLedger {
    /// 空账。
    pub fn new() -> Self {
        GrantLedger { before: [(0, 0); 8], before_len: 0, taken_permille: 0, exited: false }
    }
    /// 记录进场前份额（判据「恢复」的前提——先记账才有得还）。
    pub fn push_before(&mut self, surface: u32, share: u32) -> Result<(), u16> {
        if surface == 0 || self.exited {
            return Err(SEG_BASE + 6);
        }
        if self.before_len >= 8 {
            return Err(SEG_BASE + 7);
        }
        if share == 0 {
            return Err(SEG_BASE + 8);
        }
        self.before[self.before_len] = (surface, share);
        self.before_len += 1;
        Ok(())
    }
    /// 记录全屏拿走的量。
    pub fn take(&mut self, amount: u32) -> Result<(), u16> {
        if self.exited || amount == 0 {
            return Err(SEG_BASE + 9);
        }
        if amount + SYS_FLOOR_TOTAL_PERMILLE > 1000 {
            return Err(SEG_BASE + 10);
        }
        self.taken_permille = amount;
        Ok(())
    }
    /// 退场恢复（判据「恢复」主判据）：逐行还原进场所扣——返回还
    /// 原表；已退场再退 = 拒（幂等闸防双重归还超发）。
    pub fn exit_restore(&mut self) -> Result<Vec<(u32, u32)>, u16> {
        if self.exited {
            return Err(SEG_BASE + 11);
        }
        if self.taken_permille == 0 {
            return Err(SEG_BASE + 12);
        }
        self.exited = true;
        let mut out = Vec::new();
        for i in 0..self.before_len {
            out.push(self.before[i]);
        }
        // 无残留核：账面拿走的量在退场后必须归零可还。
        self.taken_permille = 0;
        Ok(out)
    }
}

impl Default for GrantLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 恢复无残留核（判据「恢复」守卫）：还原因子 Σ ≡ 1000 − 授予 −
/// 保底（被挤掉的杂项）的账面闭环。这里核的是「退场后账面干净」：
/// exited=true 且 taken=0。
pub fn restore_no_residual(ledger: &GrantLedger) -> Result<bool, u16> {
    if !ledger.exited {
        return Err(SEG_BASE + 13);
    }
    Ok(ledger.taken_permille == 0)
}

/// 功耗预留单调核（判据「功耗预留」守卫）：档位升高预留单调不降。
pub fn reserve_monotonic_ok() -> Result<(), u16> {
    let a = PowerReserve::None.reserve_permille();
    let b = PowerReserve::Standard.reserve_permille();
    let c = PowerReserve::Heavy.reserve_permille();
    if !(a <= b && b <= c) {
        return Err(SEG_BASE + 14);
    }
    if c + SYS_FLOOR_TOTAL_PERMILLE > 1000 {
        return Err(SEG_BASE + 15);
    }
    Ok(())
}

/// 全屏配方指纹（可审计面）。
pub fn recipe_fp(s: &FullscreenSession, grant: u32) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    acc = acc.wrapping_mul(0x100000001b3) ^ s.session as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ s.heavy as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ s.reserve.ordinal() as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ grant as u64;
    acc
}

/// 配方诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "会话号为 0 非法",
        1 => "重负载全屏必须带 J 域功耗预留档",
        2 => "授予超全屏上限（920）",
        3 => "授予为 0——保底+预留吞没全部预算",
        4 => "授予量为 0 非法",
        5 => "授予+保底 >1000——保底被侵占",
        6 => "记账面非法（0 号表面或已退场）",
        7 => "进场前快照满（8 行上限）",
        8 => "份额为 0 不入账",
        9 => "拿走量为 0 或账已关",
        10 => "拿走量侵占系统保底",
        11 => "已退场——双重归还超发被拦截",
        12 => "未拿走过任何量——无可恢复",
        13 => "账未退场——无法核无残留",
        14 => "功耗预留档位未单调",
        15 => "最高预留+保底 >1000——预留表超卖",
        _ => "未知诊断码——查本模块 SEG_BASE 0x08D0",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_grant_with_floor() {
        let light = FullscreenSession { session: 1, heavy: false, reserve: PowerReserve::None };
        let g = fullscreen_grant(&light).unwrap();
        // 1000-400-0=600，未超 920。
        assert_eq!(g, 600);
        assert!(floor_intact(g).is_ok());
        let heavy = FullscreenSession { session: 2, heavy: true, reserve: PowerReserve::Heavy };
        let gh = fullscreen_grant(&heavy).unwrap();
        // 1000-400-250=350。
        assert_eq!(gh, 350);
        assert!(floor_intact(gh).is_ok());
        let fp = recipe_fp(&heavy, gh);
        assert!(fp != 0);
    }

    #[test]
    fn heavy_requires_reserve() {
        let bad = FullscreenSession { session: 1, heavy: true, reserve: PowerReserve::None };
        assert_eq!(fullscreen_grant(&bad).unwrap_err(), SEG_BASE + 1);
        assert!(diagnose(SEG_BASE + 1).contains("预留"));
    }

    #[test]
    fn exit_restore_no_residual() {
        let mut led = GrantLedger::new();
        led.push_before(1, 300).unwrap();
        led.push_before(2, 200).unwrap();
        led.push_before(3, 100).unwrap();
        led.take(600).unwrap();
        let restored = led.exit_restore().unwrap();
        assert_eq!(restored.len(), 3);
        assert_eq!(restored[0], (1, 300));
        assert!(restore_no_residual(&led).unwrap());
        // 双重退场拒绝。
        assert_eq!(led.exit_restore().unwrap_err(), SEG_BASE + 11);
        assert!(diagnose(SEG_BASE + 11).contains("双重"));
    }

    #[test]
    fn reserve_monotonic_and_rejects() {
        assert!(reserve_monotonic_ok().is_ok());
        assert_eq!(PowerReserve::Standard.reserve_permille(), 120);
        let mut led = GrantLedger::new();
        assert_eq!(led.take(0).unwrap_err(), SEG_BASE + 9);
        assert_eq!(led.take(700).unwrap_err(), SEG_BASE + 10);
        assert!(restore_no_residual(&GrantLedger::new()).is_err());
    }
}
