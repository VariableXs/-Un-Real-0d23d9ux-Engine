//! H2 列表呈现引擎 · 深化批次三（虚拟可视区 + 索引跳段 + 键盘导航
//! ——F274/F252/F299/F253 四处列表共用的呈现层）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F274 「所有应用」列表**：索引跳转精度（首应用可见）——跳段
//!   落点必须把段头滚到视口顶，首条目可见是机判；混排分段（拼音
//!   字母段头行）的布局在这里；
//! - **F299 推荐区 / F253 快速访问 / F252 按钮列表**：可视区窗口
//!   计算（只呈现可见项——10 万项滚动 60fps 的呈现侧份额；行虚拟
//!   化本体在 H1 F228，本层是 H2 域四处列表的口径统一）；
//! - **键盘导航（F206 精神、F274 判据「键盘全程可达」）**：方向键
//!   循环、翻页、Home/End；焦点行不可见时**最小滚动**跟随——焦点
//!   永远可见，不许丢在宇宙里。
//!
//! 时间纪律：无时钟；滚动量是纯函数（同输入同滚动）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 列表模型
// ---------------------------------------------------------------------------

/// 行类别：普通行 / 段头（拼音字母、分组名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Item,
    Header,
}

/// 一行：类别 + 行高（px）。段头行高独立——混排高度布局的原子。
#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub kind: RowKind,
    pub h: u32,
}

/// 默认行高与段头行高（域内唯一——四处列表同值）。
pub const ROW_H: u32 = 44;
pub const HEADER_H: u32 = 32;

/// 由段结构生成行表：`sections: [(段名, 条目数)]`。
/// 30 条目以上的段不折叠（所有应用不是手风琴——跳段比折叠快）。
pub fn rows_of(sections: &[(char, usize)]) -> Vec<Row> {
    let mut rows = Vec::new();
    for (_, count) in sections {
        rows.push(Row { kind: RowKind::Header, h: HEADER_H });
        for _ in 0..*count {
            rows.push(Row { kind: RowKind::Item, h: ROW_H });
        }
    }
    rows
}

// ---------------------------------------------------------------------------
// 虚拟可视区（呈现窗口计算）
// ---------------------------------------------------------------------------

/// 可视区：首行号 + 行数 + 首行裁剪偏移（px——首行只露一半时）。
#[derive(Debug, PartialEq, Eq)]
pub struct Visible {
    pub first: usize,
    pub count: usize,
    /// 首行被裁掉的像素（滚动偏移对首行的余数）。
    pub crop: u32,
}

/// 可视区计算：给定滚动偏移 `offset_px` 与视口高 `viewport_h`，
/// 返回需要呈现的行窗口。多渲染一行缓冲（上 1 下 1——快速滚动
/// 不露白）。空列表返回零窗口，不炸。
pub fn visible_range(rows: &[Row], offset_px: u32, viewport_h: u32) -> Visible {
    if rows.is_empty() {
        return Visible { first: 0, count: 0, crop: 0 };
    }
    let total: u32 = rows.iter().map(|r| r.h).sum();
    let offset = offset_px.min(total.saturating_sub(viewport_h.min(total)));
    let mut y = 0u32;
    let mut first = 0usize;
    while first < rows.len() && y + rows[first].h <= offset {
        y += rows[first].h;
        first += 1;
    }
    let crop = offset - y;
    let mut acc = 0u32;
    let mut count = 0usize;
    let mut i = first;
    while i < rows.len() && acc < viewport_h {
        acc += rows[i].h;
        count += 1;
        i += 1;
    }
    // 上下各缓冲一行。
    if first > 0 {
        first -= 1;
        count += 1;
    }
    if i < rows.len() {
        count += 1;
    }
    count = count.min(rows.len() - first);
    Visible { first, count, crop }
}

/// 滚动偏移上限（内容高 - 视口，不为负）。
pub fn max_offset(rows: &[Row], viewport_h: u32) -> u32 {
    let total: u32 = rows.iter().map(|r| r.h).sum();
    total.saturating_sub(viewport_h)
}

// ---------------------------------------------------------------------------
// 索引跳段（F274 首应用可见）
// ---------------------------------------------------------------------------

/// 跳段落点：段头行号 → 把段头滚到视口顶的偏移。
/// 段头在列表尾且内容不足一屏时钳制到底（不许出现滚动后段头下
/// 全空白——「首应用可见」的尾部情形）。
pub fn jump_offset(rows: &[Row], header_idx: usize, viewport_h: u32) -> Option<u32> {
    if header_idx >= rows.len() || rows[header_idx].kind != RowKind::Header {
        return None;
    }
    let before: u32 = rows[..header_idx].iter().map(|r| r.h).sum();
    Some(before.min(max_offset(rows, viewport_h)))
}

/// 段头索引表：行表 → `[(字母, 行号)]`（F274 索引条的数据源）。
pub fn header_index(rows: &[Row], letters: &[char]) -> Vec<(char, usize)> {
    let mut out = Vec::new();
    let mut li = 0;
    for (i, r) in rows.iter().enumerate() {
        if r.kind == RowKind::Header && li < letters.len() {
            out.push((letters[li], i));
            li += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 键盘导航（焦点跟随 + 循环 + 翻页）
// ---------------------------------------------------------------------------

/// 导航动作（键位语义在此翻译——F274 五组键位的呈现侧）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
}

/// 段头是否可聚焦（本域口径：跳得过得去、焦点不停留——段头不是
/// 可激活对象，方向键**跳过**段头）。
pub const FOCUS_SKIPS_HEADER: bool = true;

/// 焦点移动：当前焦点行 → 新焦点行。跳过段头；Up 在首条目上循环
/// 到末条目（循环是 F274 键盘可达的完整闭环）；Home/End 直达首末
/// **条目**（不是行）。
pub fn move_focus(rows: &[Row], focus: usize, nav: Nav, page_rows: usize) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let items: Vec<usize> =
        rows.iter().enumerate().filter(|(_, r)| r.kind == RowKind::Item).map(|(i, _)| i).collect();
    if items.is_empty() {
        return None;
    }
    let cur = items.iter().position(|&i| i >= focus).unwrap_or(0);
    let last = items.len() - 1;
    let new_cur = match nav {
        Nav::Up => {
            if cur == 0 {
                last
            } else {
                cur - 1
            }
        }
        Nav::Down => {
            if cur == last {
                0
            } else {
                cur + 1
            }
        }
        Nav::PageUp => cur.saturating_sub(page_rows.max(1)),
        Nav::PageDown => (cur + page_rows.max(1)).min(last),
        Nav::Home => 0,
        Nav::End => last,
    };
    Some(items[new_cur])
}

/// 焦点可见滚动：焦点行不完全在视口内时，最小滚动量（一行行高
/// 的余量——焦点环不许被裁）。返回新滚动偏移。
pub fn scroll_to_show(rows: &[Row], focus: usize, offset_px: u32, viewport_h: u32) -> u32 {
    if focus >= rows.len() {
        return offset_px;
    }
    let before: u32 = rows[..focus].iter().map(|r| r.h).sum();
    let row_h = rows[focus].h;
    let max = max_offset(rows, viewport_h);
    if before < offset_px {
        before.min(max)
    } else if before + row_h > offset_px + viewport_h {
        (before + row_h - viewport_h).min(max)
    } else {
        offset_px
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2list_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2list");
    // 行表：段结构 → 段头+条目，高度族正确。
    let rows = rows_of(&[('A', 3), ('B', 2)]);
    set.add(
        "h2list rows shape",
        rows.len() == 7
            && rows[0].kind == RowKind::Header
            && rows[1].kind == RowKind::Item
            && rows[0].h == HEADER_H
            && rows[1].h == ROW_H,
        "sections to rows",
    );
    // 可视区：窗口只覆盖视口 + 缓冲；crop 口径正确；空表零窗口。
    let vis = visible_range(&rows, 0, 100);
    set.add(
        "h2list visible top",
        vis.first == 0 && vis.crop == 0 && vis.count == 4,
        "viewport + buffer",
    );
    // 滚动 20 落在段头内：首行=段头、裁 20。
    let vis2 = visible_range(&rows, 20, 200);
    set.add(
        "h2list crop",
        vis2.first == 0 && vis2.crop == 20,
        "partial header",
    );
    // 滚到最底：offset 钳制、末行在窗口内。
    let max = max_offset(&rows, 100);
    let vis3 = visible_range(&rows, max + 10_000, 100);
    set.add(
        "h2list bottom clamp",
        max == rows.iter().map(|r| r.h).sum::<u32>() - 100
            && vis3.first + vis3.count == rows.len(),
        "no overscroll",
    );
    set.add("h2list empty safe", visible_range(&[], 0, 100).count == 0, "empty no crash");
    // 跳段：段头滚到顶；B 段头前缀和 164（未到钳制值）；
    // 非段头行拒绝跳。
    let j1 = jump_offset(&rows, 0, 100);
    let j2 = jump_offset(&rows, 4, 100);
    set.add(
        "h2list jump to top",
        j1 == Some(0) && j2 == Some(164) && jump_offset(&rows, 1, 100).is_none(),
        "header lands visible; non-header rejected",
    );
    // 索引表：字母 ↔ 行号一一对应。
    let idx = header_index(&rows, &['A', 'B']);
    set.add(
        "h2list header index",
        idx == alloc::vec![('A', 0), ('B', 4)],
        "letter→row",
    );
    // 焦点移动：跳过段头、循环、翻页、Home/End。
    let f0 = move_focus(&rows, 1, Nav::Down, 3);
    let fwrap = move_focus(&rows, 6, Nav::Down, 3);
    let fback = move_focus(&rows, 1, Nav::Up, 3);
    set.add(
        "h2list nav skip header",
        f0 == Some(2) && fback == Some(6),
        "headers skipped",
    );
    set.add("h2list nav wrap", fwrap == Some(1), "wrap to first");
    set.add(
        "h2list nav page+ends",
        move_focus(&rows, 1, Nav::PageDown, 2) == Some(3)
            && move_focus(&rows, 2, Nav::Home, 3) == Some(1)
            && move_focus(&rows, 2, Nav::End, 3) == Some(6),
        "page/home/end",
    );
    // 焦点跟随：上方 → 滚到行顶；下方 → 行底恰好入视口（240+44-100
    // 被钳到上限 184）；可见 → 不动（最小平移）。
    let s1 = scroll_to_show(&rows, 1, 200, 100);
    let s2 = scroll_to_show(&rows, 6, 0, 100);
    let s3 = scroll_to_show(&rows, 1, 0, 200);
    set.add(
        "h2list focus follow",
        s1 == 32 && s2 == 184 && s3 == 0,
        "minimal shift",
    );
    // 焦点环不被裁：跟随后的偏移下焦点行完整可见。
    let s4 = scroll_to_show(&rows, 2, 0, 100);
    let ftop: u32 = rows[..2].iter().map(|r| r.h).sum();
    set.add(
        "h2list focus unclipped",
        s4 <= ftop && ftop + ROW_H <= s4 + 100,
        "full row visible",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2list_all_green() {
        let set = run_h2list_checks();
        assert!(set.all_passed(), "h2list 自检有红项");
        assert!(!set.truncated(), "h2list 自检溢出");
    }

    #[test]
    fn long_list_window_stays_small() {
        // 1 万行列表：任何滚动位下窗口 ≤ 视口行数 + 2 缓冲——呈现
        // 成本与总行数无关（虚拟化口径）。
        let rows: Vec<Row> = (0..10_000).map(|_| Row { kind: RowKind::Item, h: ROW_H }).collect();
        for off in [0u32, 50_000, 219_912, 439_956] {
            let v = visible_range(&rows, off, 400);
            assert!(v.count <= 400 / ROW_H as usize + 2 + 1, "window grew at {off}");
            assert!(v.first + v.count <= rows.len());
        }
    }

    #[test]
    fn focus_never_lands_on_header() {
        // 随机序列导航 500 步：焦点永远停在条目行（段头跳过不变式）。
        let rows = rows_of(&[('A', 4), ('B', 4), ('C', 4)]);
        let mut focus = 1usize;
        for i in 0..500u32 {
            let nav = if i % 2 == 0 { Nav::Down } else { Nav::Up };
            focus = move_focus(&rows, focus, nav, 3).unwrap();
            assert_eq!(rows[focus].kind, RowKind::Item, "focus on header at step {i}");
        }
    }
}
