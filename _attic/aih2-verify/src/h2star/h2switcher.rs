//! H2 Alt+Tab 切换器 · 深化批次四·二波（F082 车道经 F277/F284 锚
//! 落位——卡片墙几何、假死标注、焦点屏过滤、键盘流）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F277 焦点屏**：切换器只列焦点屏的窗（跨屏切换走投影/任务
//!   栏车道——两套动线不混）；
//! - **F284 无响应**：假死窗口在卡片墙**标注**（灰卡+「无响应」
//!   角标）——标注是信息不是禁令：假死窗照样可选中（切过去正是
//!   用户想救它的动机）；
//! - **十二章「确定性」**：卡片墙布局纯函数（同窗口集合同布局），
//!   12 窗压力不乱序不重叠；Esc 关闭语义真生效（切到一半 Esc =
//!   留在原窗——切换器不留副作用）。
//!
//! 时间纪律：激活时刻由 z 序账供给（h2zorder 单一数据源）。

use crate::checks::CheckSet;

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 卡片模型
// ---------------------------------------------------------------------------

/// 一张切换卡片。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    pub win: u64,
    pub screen: u8,
    /// 假死标注（F284 判定注入——切换器不自判）。
    pub hung: bool,
    /// 最近激活时刻（序的数据源）。
    pub last_active_ms: u64,
}

/// 切换器键盘流。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchNav {
    Next,
    Prev,
    Commit,
    Cancel,
}

/// 卡片墙：焦点屏过滤 + 最近激活序 + 高亮游标。
pub struct Switcher {
    cards: Vec<Card>,
    cursor: usize,
}

impl Switcher {
    /// 构建：z 序账的 Alt+Tab 序 + 焦点屏过滤 + 假死标注注入。
    /// 顺序（最近激活优先）与 h2zorder::alt_tab_order 同源。
    pub fn build(order: &[(u64, u8, bool, u64)], focus_screen: u8) -> Switcher {
        let mut cards: Vec<Card> = order
            .iter()
            .filter(|(_, s, _, _)| *s == focus_screen)
            .map(|(w, _, hung, t)| Card { win: *w, screen: focus_screen, hung: *hung, last_active_ms: *t })
            .collect();
        cards.sort_by(|a, b| b.last_active_ms.cmp(&a.last_active_ms));
        Switcher { cards, cursor: 0 }
    }

    pub fn len(&self) -> usize {
        self.cards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// 当前游标卡。
    pub fn current(&self) -> Option<&Card> {
        self.cards.get(self.cursor)
    }

    /// 卡片墙序（渲染序 = 数据序——确定性口径）。
    pub fn order(&self) -> Vec<u64> {
        self.cards.iter().map(|c| c.win).collect()
    }

    /// 键盘流：Next/Prev 循环移动（空墙诚实 None）；Commit 返回
    /// 选中的窗；Cancel 返回 None（留在原窗——零副作用）。
    pub fn nav(&mut self, n: SwitchNav) -> Option<u64> {
        match n {
            SwitchNav::Next => {
                if self.cards.is_empty() {
                    return None;
                }
                self.cursor = (self.cursor + 1) % self.cards.len();
                None
            }
            SwitchNav::Prev => {
                if self.cards.is_empty() {
                    return None;
                }
                self.cursor = (self.cursor + self.cards.len() - 1) % self.cards.len();
                None
            }
            SwitchNav::Commit => self.current().map(|c| c.win),
            SwitchNav::Cancel => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 卡片墙几何（12 窗压力不乱序不重叠——纯函数布局）
// ---------------------------------------------------------------------------

/// 卡片尺寸与间距（域内唯一——F300 图标语家族口径）。
pub const CARD_W: u32 = 160;
pub const CARD_H: u32 = 100;
pub const CARD_GAP: u32 = 12;

/// 卡片墙布局：每行 `per_row` 张，行数自动；返回每张卡的原点。
/// 卡数超一行容量自动换行；总高超出 `panel_h` 时缩小间距（最低
/// 4px——再挤就是布局缺陷，交给宽度三档收缩）。
pub fn wall_layout(count: usize, panel_w: u32, panel_h: u32) -> Vec<(i32, i32)> {
    let per_row = (((panel_w + CARD_GAP) / (CARD_W + CARD_GAP)).max(1)) as usize;
    let rows = ((count + per_row - 1) / per_row).max(1);
    let mut gap = CARD_GAP;
    let need_h = rows as u32 * CARD_H + (rows as u32 - 1) * gap;
    if need_h > panel_h && rows > 1 {
        gap = ((panel_h - rows as u32 * CARD_H) / (rows as u32 - 1)).max(4);
    }
    (0..count)
        .map(|i| {
            let (row, col) = (i / per_row, i % per_row);
            (
                (col as u32 * (CARD_W + gap)) as i32,
                (row as u32 * (CARD_H + gap)) as i32,
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2switcher_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2switcher");
    // 构建：焦点屏过滤 + 最近激活序。
    let order = [
        (1u64, 0u8, false, 100u64),
        (2, 1, false, 400),
        (3, 0, false, 300),
        (4, 0, true, 50), // 假死
    ];
    let mut sw = Switcher::build(&order, 0);
    set.add(
        "h2switcher screen filter",
        sw.order() == vec![3, 1, 4] && sw.len() == 3,
        "focus screen only, recent first",
    );
    // 假死标注保留（可选中——标注不是禁令）。
    set.add(
        "h2switcher hung labelled",
        sw.current().map(|c| c.win) == Some(3)
            && sw.nav(SwitchNav::Next).is_none()
            && sw.current().map(|c| c.win) == Some(1)
            && sw.nav(SwitchNav::Next).is_none()
            && sw.current().map(|c| c.hung) == Some(true),
        "hung card selectable",
    );
    // Prev 循环 + Commit + Cancel（零副作用）。
    sw.nav(SwitchNav::Prev);
    sw.nav(SwitchNav::Prev);
    set.add(
        "h2switcher prev wraps",
        sw.current().map(|c| c.win) == Some(3),
        "wrap both ways",
    );
    set.add(
        "h2switcher commit",
        sw.nav(SwitchNav::Commit) == Some(3),
        "commit returns win",
    );
    let before = sw.order();
    set.add(
        "h2switcher cancel no side effect",
        sw.nav(SwitchNav::Cancel).is_none() && sw.order() == before,
        "esc stays put",
    );
    // 空墙诚实。
    let mut empty = Switcher::build(&[], 0);
    set.add(
        "h2switcher empty honest",
        empty.is_empty() && empty.nav(SwitchNav::Next).is_none(),
        "no fake switch",
    );
    // 布局：4 卡 800 宽 → 每行 4；12 卡 400 宽 → 2 行，无重叠。
    let l4 = wall_layout(4, 800, 600);
    set.add(
        "h2switcher wall row",
        l4.len() == 4 && l4[1].0 == (CARD_W + CARD_GAP) as i32 && l4.iter().all(|(x, _)| *x >= 0),
        "single row",
    );
    let l12 = wall_layout(12, 400, 600);
    let mut overlap = false;
    for i in 0..l12.len() {
        for j in (i + 1)..l12.len() {
            let (ax, ay) = l12[i];
            let (bx, by) = l12[j];
            if (ax - bx).abs() < CARD_W as i32 && (ay - by).abs() < CARD_H as i32 {
                overlap = true;
            }
        }
    }
    set.add(
        "h2switcher wall 12 no overlap",
        l12.len() == 12 && !overlap && l12[0].0 == 0,
        "pressure layout holds",
    );
    set.add(
        "h2switcher wall const",
        CARD_W == 160 && CARD_H == 100 && CARD_GAP == 12,
        "card metrics",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2switcher_all_green() {
        let set = run_h2switcher_checks();
        assert!(set.all_passed(), "h2switcher 自检有红项");
        assert!(!set.truncated(), "h2switcher 自检溢出");
    }

    #[test]
    fn layouts_deterministic() {
        // 同输入两次布局逐点相等（十二章确定性——布局不许抖）。
        for count in [1usize, 5, 12, 30] {
            let a = wall_layout(count, 1024, 700);
            let b = wall_layout(count, 1024, 700);
            assert_eq!(a, b);
            assert_eq!(a.len(), count);
        }
    }
}
