//! F558 键盘测试工具 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：全键位点亮准确性；计数显示；无冲测试；导出凭证；
//! 即开即用（<1s）。
//!
//! **设计要点（主册）**：
//! - 全键位图（Y7000 键盘布局可视化）——按键即亮松开即灭，同时显示触发
//!   次数；
//! - 连击/幽灵键检测（三键无冲测试区）；
//! - 测试结果可导出（送修/换机前的凭证）；
//! - 工具轻量即开即用（<1s）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 全键位图容量（Y7000 布局 104 键 ANSI 扩展位——上限口径）。
pub const KEYS: usize = 104;

/// 无冲测试同按上限（三键无冲区容量）。
pub const ROLLOVER_KEYS: usize = 3;

/// 即开即用红线（ms）。
pub const OPEN_BUDGET_MS: u64 = 1_000;

/// 键位 id（0..104；物理布局映射在 UI 层，内核账按 id 记）。
pub type KeyId = u16;

// ---------------------------------------------------------------------------
// 测试台
// ---------------------------------------------------------------------------

/// 键盘测试台。
pub struct KeybTest {
    /// 每键触发计数。
    counts: [u32; KEYS],
    /// 当前按住集（点亮状态）。
    down: [bool; KEYS],
    /// 无冲测试区当前同按集。
    rollover: [Option<KeyId>; ROLLOVER_KEYS],
    /// 幽灵键判定账：同按超过 ROLLOVER_KEYS 的记录次数。
    ghost_events: u32,
    /// 计时（即开即用账）。
    open_ms: u64,
}

impl KeybTest {
    pub fn new() -> KeybTest {
        KeybTest {
            counts: [0; KEYS],
            down: [false; KEYS],
            rollover: [None; ROLLOVER_KEYS],
            ghost_events: 0,
            open_ms: 0,
        }
    }

    /// 按下：点亮 + 计数 +1；进入无冲区；第四键同按记幽灵事件（不吞键——
    /// 计数照记，检测是检测，输入归输入）。
    pub fn press(&mut self, id: KeyId) -> bool {
        if (id as usize) >= KEYS {
            return false;
        }
        let i = id as usize;
        self.counts[i] += 1;
        if !self.down[i] {
            self.down[i] = true;
        }
        if !self.rollover.contains(&Some(id)) {
            match self.rollover.iter_mut().find(|s| s.is_none()) {
                Some(slot) => *slot = Some(id),
                None => self.ghost_events += 1, // 三键全占再来键 = 无冲红线记录
            }
        }
        true
    }

    /// 松开：熄灭 + 退出无冲区。
    pub fn release(&mut self, id: KeyId) -> bool {
        if (id as usize) >= KEYS {
            return false;
        }
        self.down[id as usize] = false;
        if let Some(slot) = self.rollover.iter_mut().find(|s| **s == Some(id)) {
            *slot = None;
        }
        true
    }

    /// 键位是否点亮。
    pub fn lit(&self, id: KeyId) -> bool {
        (id as usize) < KEYS && self.down[id as usize]
    }

    /// 键位触发计数。
    pub fn count(&self, id: KeyId) -> u32 {
        if (id as usize) < KEYS {
            self.counts[id as usize]
        } else {
            0
        }
    }

    /// 无冲测试区当前同按数。
    pub fn rollover_count(&self) -> usize {
        self.rollover.iter().filter(|s| s.is_some()).count()
    }

    /// 幽灵事件数（三键无冲越线记录）。
    pub fn ghost_count(&self) -> u32 {
        self.ghost_events
    }

    /// 打开耗时记账（即开即用判据：宿主报实测 ms，内核判定红线）。
    pub fn note_open_ms(&mut self, ms: u64) {
        self.open_ms = ms;
    }

    pub fn opens_in_budget(&self) -> bool {
        self.open_ms <= OPEN_BUDGET_MS
    }

    /// 导出凭证：全键位计数摘要（id, 计数）——只含有记录的键。
    pub fn export(&self) -> alloc::vec::Vec<(KeyId, u32)> {
        let mut out = alloc::vec::Vec::new();
        for (i, &c) in self.counts.iter().enumerate() {
            if c > 0 {
                out.push((i as KeyId, c));
            }
        }
        out
    }
}

impl Default for KeybTest {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_keybtest_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 全键位点亮准确性：按下即亮、松开即灭；越界 id 拒绝。
    let mut t = KeybTest::new();
    t.press(10);
    let lit_down = t.lit(10);
    t.release(10);
    set.add(
        "press lights release clears",
        lit_down && !t.lit(10) && !t.press(200) && !t.release(200),
        "",
    );

    // 2. 计数显示：同键三击计数 3；重复 press 幂等点亮但计数照记。
    let mut t2 = KeybTest::new();
    for _ in 0..3 {
        t2.press(5);
        t2.release(5);
    }
    set.add("counts per key", t2.count(5) == 3 && t2.count(6) == 0, "");

    // 3. 无冲测试：三键同按全记录；第四键记幽灵事件且原三键不受扰。
    let mut t3 = KeybTest::new();
    t3.press(1);
    t3.press(2);
    t3.press(3);
    let three_ok = t3.rollover_count() == 3 && t3.ghost_count() == 0;
    t3.press(4);
    let ghost_logged = t3.ghost_count() == 1 && t3.rollover_count() == 3;
    set.add("rollover three keys and ghost log", three_ok && ghost_logged, "");

    // 4. 松开腾位：三键区松一键后新键可入。
    t3.release(2);
    t3.press(9);
    set.add("release frees rollover slot", t3.rollover_count() == 3 && t3.lit(9), "");

    // 5. 导出凭证：只含有记录的键，计数对账（幽灵键 4 的计数也在凭证里）。
    let exported = t3.export();
    let sum: u32 = exported.iter().map(|(_, c)| c).sum();
    let expect: u32 = [1u16, 2, 3, 4, 9].iter().map(|&k| t3.count(k)).sum();
    set.add(
        "export receipt counts",
        exported.len() == 5 && sum == expect,
        "",
    );

    // 6. 即开即用：999ms 过线、1000ms 触线（≤ 红线含等于）。
    let mut t4 = KeybTest::new();
    t4.note_open_ms(999);
    let under = t4.opens_in_budget();
    t4.note_open_ms(OPEN_BUDGET_MS);
    set.add("opens within one second", under && t4.opens_in_budget() && OPEN_BUDGET_MS == 1_000, "");

    // 7. 重复按下不重复入无冲区（按住不动只占一位）。
    let mut t5 = KeybTest::new();
    t5.press(7);
    t5.press(7);
    t5.press(7);
    set.add("held key occupies one slot", t5.rollover_count() == 1 && t5.ghost_count() == 0, "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_release_repeat_cycle() {
        let mut t = KeybTest::new();
        for _ in 0..10 {
            t.press(0);
            t.release(0);
        }
        assert_eq!(t.count(0), 10);
        assert!(!t.lit(0));
    }

    #[test]
    fn export_sorted_by_id() {
        let mut t = KeybTest::new();
        t.press(30);
        t.release(30);
        t.press(3);
        t.release(3);
        let ex = t.export();
        assert_eq!(ex, alloc::vec![(3, 1), (30, 1)]);
    }

    #[test]
    fn ghost_key_still_counts() {
        // 检测是检测、输入归输入：幽灵键计数照记。
        let mut t = KeybTest::new();
        t.press(1);
        t.press(2);
        t.press(3);
        t.press(4);
        assert_eq!(t.count(4), 1);
    }
}
