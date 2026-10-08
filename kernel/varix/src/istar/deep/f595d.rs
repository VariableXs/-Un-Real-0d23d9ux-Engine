//! 深化层 · F595 固定区域重截（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F595 节）：
//! ①「保存区域随会话（重启清）」的**区域账本**——会话内每次框选入账
//!   （最近一次是重截源），代账边界锚（新代账空——重启清零可验）；
//! ②「同区域重截一键」的**管线状态账**——有记忆跳过框选直取捕获，
//!   无记忆诚实回框选（不假装知道上次在哪）；
//! ③「区域框显示 200ms 确认视觉」的**闪账时序**——t<200 可见、
//!   t≥200 自收，且连拍不等闪完（确认视觉不挡节奏）；
//! ④「与 F593 组合」的**组合状态机**——倒计时到期→按记忆区域捕获
//!   （Fired 边沿恰好一次，自动盯梢不重拍）。

use crate::checks::CheckSet;
use crate::istar::delayshot::{CountState, DelayShot};
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::reshot::{Reshot, Rect, CONFIRM_FLASH_MS};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// ① 区域账本
// ---------------------------------------------------------------------------

/// 区域账本：会话内框选历史（最近一次入账尾——重截源）。
pub struct RegionLedger {
    generation: u64,
    history: Vec<Rect>,
}

impl RegionLedger {
    pub fn new(generation: u64) -> RegionLedger {
        RegionLedger { generation, history: Vec::new() }
    }

    /// 框选入账。
    pub fn note(&mut self, r: Rect) {
        self.history.push(r);
    }

    /// 最近区域（账尾）。
    pub fn last(&self) -> Option<Rect> {
        self.history.last().copied()
    }

    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// 重启清零边界锚：新代账必须全新（代不同 + 零历史）。
    pub fn rebooted_fresh(&self, new_gen: &RegionLedger) -> bool {
        new_gen.generation != self.generation && new_gen.history.is_empty()
    }
}

// ---------------------------------------------------------------------------
// ② 一键重截管线
// ---------------------------------------------------------------------------

/// 一键重截管线：有记忆→直接捕获（跳框选=true）；无记忆→诚实 None
/// （跳框选=false——会话内没框过就回框选，不假装）。
pub fn one_key_reshoot(rs: &mut Reshot) -> (bool, Option<Rect>) {
    if rs.has_region() {
        (true, rs.reshot())
    } else {
        (false, None)
    }
}

// ---------------------------------------------------------------------------
// ③ 200ms 确认闪账
// ---------------------------------------------------------------------------

/// 确认视觉可见判定：t<CONFIRM_FLASH_MS 可见，到点自收。
pub fn flash_visible(elapsed_ms: u32) -> bool {
    elapsed_ms < CONFIRM_FLASH_MS
}

// ---------------------------------------------------------------------------
// ④ 与 F593 的组合状态机
// ---------------------------------------------------------------------------

/// 组合观察器：倒计时 Fired 边沿→按记忆区域捕获恰好一次
/// （DelayShot 到点后恒 Fired——边沿判别保证不重拍）。
pub struct ComboWatcher {
    fired_seen: bool,
    captures: u32,
}

impl ComboWatcher {
    pub fn new() -> ComboWatcher {
        ComboWatcher { fired_seen: false, captures: 0 }
    }

    /// 每帧喂倒计时状态；Fired 边沿触发一次记忆区域捕获。
    pub fn feed(&mut self, st: CountState, rs: &mut Reshot) -> Option<Rect> {
        if st == CountState::Fired && !self.fired_seen {
            self.fired_seen = true;
            if let Some(r) = rs.reshot() {
                self.captures += 1;
                return Some(r);
            }
        }
        None
    }

    pub fn capture_count(&self) -> u32 {
        self.captures
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f595_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 区域账本：两次框选两笔账，账尾是最近一次。
    let mut led = RegionLedger::new(7);
    led.note((100, 200, 800, 600));
    led.note((0, 0, 50, 50));
    cs.add(
        "region ledger appends",
        led.len() == 2 && led.last() == Some((0, 0, 50, 50)),
        "",
    );

    // 2) 重启清零边界锚：代 8 新账对代 7 旧账——全新零历史。
    let fresh = RegionLedger::new(8);
    cs.add(
        "reboot clears ledger",
        led.rebooted_fresh(&fresh) && fresh.last().is_none() && !led.rebooted_fresh(&led),
        "",
    );

    // 3) 一键重截管线：有记忆跳框选直取；无记忆诚实回框选且不闪。
    let mut r = Reshot::new(1);
    let (skipped_none, got) = one_key_reshoot(&mut r);
    let mut r2 = Reshot::new(1);
    r2.capture_region((10, 10, 100, 100));
    let (skipped_yes, got2) = one_key_reshoot(&mut r2);
    cs.add(
        "one key pipeline",
        !skipped_none && got.is_none() && skipped_yes && got2 == Some((10, 10, 100, 100)),
        "",
    );

    // 4) 200ms 闪账时序：t=199 可见、t=200 自收（精确到点不拖尾）。
    let visible = flash_visible(199);
    r2.tick(CONFIRM_FLASH_MS);
    cs.add(
        "flash timing 200ms",
        visible && !flash_visible(CONFIRM_FLASH_MS) && !r2.flashing() && CONFIRM_FLASH_MS == 200,
        "",
    );

    // 5) 连拍不等闪完：确认视觉在途照样再截（一秒一张的节奏保障）。
    r2.tick(CONFIRM_FLASH_MS); // 先把上一发的闪清掉，账面干净
    let _ = r2.reshot();
    let in_flash = r2.flashing();
    let again = r2.reshot();
    cs.add(
        "reshoot during flash allowed",
        in_flash && again.is_some() && r2.reshot_count() >= 2,
        "",
    );

    // 6) F593 组合状态机：3 秒倒计时到期→按记忆区域捕获恰好一次。
    let mut delay = DelayShot::new();
    let _ = delay.start(3, 0);
    let mut combo_rs = Reshot::new(1);
    combo_rs.capture_region((40, 40, 320, 240));
    let mut watcher = ComboWatcher::new();
    let mut first_shot = None;
    for i in 0..400u64 {
        let st = delay.frame(i * 16, 16);
        if let Some(shot) = watcher.feed(st, &mut combo_rs) {
            first_shot = Some(shot);
        }
    }
    cs.add(
        "f593 combo fires once",
        delay.state() == CountState::Fired
            && first_shot == Some((40, 40, 320, 240))
            && watcher.capture_count() == 1,
        "",
    );

    // 7) 基础件契约不被深化破坏：确认时长常量与快捷键注册值原样。
    cs.add(
        "base contract kept",
        CONFIRM_FLASH_MS == 200 && r.hotkey() == Some("Ctrl+Shift+R"),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_boundaries() {
        assert!(flash_visible(0));
        assert!(flash_visible(CONFIRM_FLASH_MS - 1));
        assert!(!flash_visible(CONFIRM_FLASH_MS));
        assert!(!flash_visible(u32::MAX));
    }

    #[test]
    fn combo_without_region_honest() {
        // 倒计时到期但无记忆区域——不假装捕获（诚实零拍）。
        let mut delay = DelayShot::new();
        let _ = delay.start(3, 0);
        let mut empty = Reshot::new(1);
        let mut w = ComboWatcher::new();
        for i in 0..300u64 {
            let st = delay.frame(i * 16, 16);
            let _ = w.feed(st, &mut empty);
        }
        assert_eq!(w.capture_count(), 0);
        assert!(delay.state() == CountState::Fired);
    }

    #[test]
    fn ledger_empty_last_none() {
        let led = RegionLedger::new(1);
        assert!(led.last().is_none());
        assert_eq!(led.len(), 0);
    }
}
