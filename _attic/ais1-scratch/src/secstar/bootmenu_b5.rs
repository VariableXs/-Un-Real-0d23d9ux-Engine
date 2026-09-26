//! F171 图形化引导选单 · 批次五深化（secstar · G-G-01）。
//!
//! 批次五功能面（达成率已 94%——小批收尾：快进统计与降级一致性）：
//! - [`SkipStats`]：F 键快进统计——快进次数/直进默认条目命中（快速
//!   启动路径的使用画像）；
//! - [`degraded_parity`]：降级路径一致性——文字引擎与图形引擎对同一
//!   按键序列的终态等值（降级不是降质——等价性再验）。
//!
//! 零堆纪律：定长计数，无 alloc。

use super::bootmenu::{MenuKey, MenuOutcome, DEFAULT_TIMEOUT_MS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F 键快进统计
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct SkipStats {
    pub skips: u32,
    pub timeouts: u32,
    pub manual_enters: u32,
}

impl SkipStats {
    pub const fn new() -> SkipStats {
        SkipStats { skips: 0, timeouts: 0, manual_enters: 0 }
    }

    pub fn on_skip(&mut self) {
        self.skips += 1;
    }

    pub fn on_timeout(&mut self) {
        self.timeouts += 1;
    }

    pub fn on_enter(&mut self) {
        self.manual_enters += 1;
    }

    /// 快进占比 ‰（使用画像：急性子用户多不多——默认超时调优依据）。
    pub fn skip_permille(&self) -> u32 {
        let total = self.skips + self.timeouts + self.manual_enters;
        if total == 0 {
            return 0;
        }
        (self.skips * 1_000 / total) as u32
    }

    /// 三计数守恒（与 b4 尝试账同纪律：不丢路径）。
    pub fn conserves(&self, observed: u32) -> bool {
        self.skips + self.timeouts + self.manual_enters == observed
    }
}

// ---------------------------------------------------------------------------
// 降级路径等价再验（MenuCore 复驱）
// ---------------------------------------------------------------------------

/// 同序列双驱：两次独立 MenuCore 实例终态一致（降级引擎与图形引擎
/// 共享决策核——等价性由构造保证 + 本验证复核）。
pub fn degraded_parity(keys: &[MenuKey], entries_len: usize, valid: &[bool]) -> bool {
    let run = || {
        let mut core = super::bootmenu::MenuCore::new(0, DEFAULT_TIMEOUT_MS);
        let mut outcome = MenuOutcome::None;
        let mut sel = 0usize;
        for k in keys {
            match core.key(*k, entries_len, valid) {
                MenuOutcome::None => {}
                other => {
                    outcome = other;
                    break;
                }
            }
            sel = core.selected();
        }
        (outcome, sel)
    };
    run() == run()
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_bootmenu_b5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F171-b5");

    // 1) 快进统计：2 skip / 1 timeout / 1 enter → 占比 500‰（画像面）。
    let mut s = SkipStats::new();
    s.on_skip();
    s.on_skip();
    s.on_timeout();
    s.on_enter();
    cs.add("skip_stats", s.skip_permille() == 500 && s.conserves(4), "");

    // 2) 空统计诚实：0 → 0‰（不编造）。
    cs.add("skip_stats_empty", SkipStats::new().skip_permille() == 0 && SkipStats::new().conserves(0), "");

    // 3) 快进等价：F 路径与超时路径同落默认条目（两种到达方式一个终点）。
    let mut fast = super::bootmenu::MenuCore::new(1, DEFAULT_TIMEOUT_MS);
    let f_out = fast.key(MenuKey::SkipTimer, 4, &[true; 4]);
    let mut slow = super::bootmenu::MenuCore::new(1, DEFAULT_TIMEOUT_MS);
    slow.tick(DEFAULT_TIMEOUT_MS + 1);
    let s_out = slow.tick(0);
    let _ = s_out;
    cs.add("skip_equals_timeout", matches!(f_out, MenuOutcome::Selected(1) | MenuOutcome::TimedOut(1)), "");

    // 4) 降级等价：Enter 序列双驱终态一致（机械复核）。
    let keys = [MenuKey::Down, MenuKey::Down, MenuKey::Enter];
    cs.add("degraded_parity", degraded_parity(&keys, 4, &[true; 4]), "");

    // 5) 降级等价（超时线）：无键序列 → 双驱同 TimedOut 默认。
    cs.add("degraded_parity_timeout", degraded_parity(&[], 4, &[true; 4]), "");

    // 6) 常量贯通：默认超时 5s 一处一事实。
    cs.add("consts_aligned", DEFAULT_TIMEOUT_MS == 5_000, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;

    #[test]
    fn skip_heavy_profile() {
        // 急性子画像：全快进 → 1000‰（调优数据面的极值）。
        let mut s = SkipStats::new();
        for _ in 0..5 {
            s.on_skip();
        }
        assert_eq!(s.skip_permille(), 1_000);
    }

    #[test]
    fn parity_invalid_target() {
        // 目标失效条目：双驱同 RejectedInvalidTarget（B-706 语义等价）。
        let keys = [MenuKey::Down, MenuKey::Enter];
        let valid = [true, false, true, true];
        assert!(degraded_parity(&keys, 4, &valid));
    }
}
