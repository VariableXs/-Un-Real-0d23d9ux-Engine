//! AI-11 开始菜单子模块（F253 渲染 / F254 搜索 / F255 跳转列表）。
//!
//! 菜单本体与任务栏同属一个域，但布局、搜索与跳转列表是独立的一组纯逻辑，
//! 单独成文件便于自检与复用（搜索也供桌面搜索 F246 调用）。
//!
//! 全部定长：`[Option<MenuApp>; 24]`，零分配，`no_std` 可直接跑。

pub const MAX_MENU_APPS: usize = 24;
pub const MAX_JUMP: usize = 4;
pub const MAX_RESULTS: usize = 8;
pub const QUERY_MAX: usize = 32;

/// 菜单里的一项应用。`key` 是 ASCII 检索键（中文名无法用 ASCII 键盘命中）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuApp {
    pub id: u32,
    pub name: &'static str,
    pub key: &'static str,
    pub pinned: bool,
    pub folder: u8,
}

pub struct StartMenu {
    pub apps: [Option<MenuApp>; MAX_MENU_APPS],
    pub count: usize,
    pub open: bool,
    pub query: [u8; QUERY_MAX],
    pub query_len: usize,
    pub sel: usize,
}

impl StartMenu {
    pub const fn new() -> StartMenu {
        StartMenu {
            apps: [None; MAX_MENU_APPS],
            count: 0,
            open: false,
            query: [0; QUERY_MAX],
            query_len: 0,
            sel: 0,
        }
    }

    /// F253 注册应用；同 id 覆盖，容量满则拒绝。
    pub fn add(&mut self, app: MenuApp) -> bool {
        if let Some(i) = (0..self.count).find(|&i| self.apps[i].map(|a| a.id) == Some(app.id)) {
            self.apps[i] = Some(app);
            return true;
        }
        if self.count >= MAX_MENU_APPS {
            return false;
        }
        self.apps[self.count] = Some(app);
        self.count += 1;
        true
    }

    pub fn pinned_count(&self) -> usize {
        (0..self.count)
            .filter(|&i| self.apps[i].map(|a| a.pinned).unwrap_or(false))
            .count()
    }

    pub fn folder_count(&self, folder: u8) -> usize {
        (0..self.count)
            .filter(|&i| self.apps[i].map(|a| a.folder) == Some(folder))
            .count()
    }

    /// F254 设置查询串（内部统一小写保存）。
    pub fn set_query(&mut self, q: &[u8]) -> usize {
        let n = q.len().min(QUERY_MAX);
        for i in 0..n {
            self.query[i] = ascii_lower(q[i]);
        }
        self.query_len = n;
        self.sel = 0;
        n
    }

    pub fn clear_query(&mut self) {
        self.query_len = 0;
        self.sel = 0;
    }

    /// F253 键盘移动选中项（在结果集里循环）。
    pub fn move_sel(&mut self, results: usize, dir: i8) {
        if results == 0 {
            self.sel = 0;
            return;
        }
        let cur = self.sel.min(results - 1) as i32;
        let n = results as i32;
        let next = if dir >= 0 { (cur + 1) % n } else { (cur - 1 + n) % n };
        self.sel = next as usize;
    }
}

/// F253 网格行数：向上取整，count = 0 时为 0。
pub fn grid_rows(count: usize, cols: u8) -> u32 {
    let c = (cols.max(1)) as usize;
    ((count + c - 1) / c) as u32
}

/// F253 网格坐标：第 index 项落在 (col, row)。
pub fn grid_cell(index: usize, cols: u8) -> (u32, u32) {
    let c = (cols.max(1)) as u32;
    ((index as u32) % c, (index as u32) / c)
}

fn ascii_lower(b: u8) -> u8 {
    if b'A' <= b && b <= b'Z' {
        b + 32
    } else {
        b
    }
}

/// 子序列匹配打分：命中 +1，连续命中额外 +2；不匹配返回 None。
fn subsequence_score(hay: &str, needle: &[u8]) -> Option<u32> {
    if needle.is_empty() {
        return None;
    }
    let bytes = hay.as_bytes();
    let mut hi = 0usize;
    let mut score = 0u32;
    let mut last: Option<usize> = None;
    for &nb in needle {
        let want = ascii_lower(nb);
        let mut found = None;
        while hi < bytes.len() {
            if ascii_lower(bytes[hi]) == want {
                found = Some(hi);
                break;
            }
            hi += 1;
        }
        match found {
            Some(pos) => {
                if let Some(prev) = last {
                    if pos == prev + 1 {
                        score += 2;
                    }
                }
                score += 1;
                last = Some(pos);
                hi = pos + 1;
            }
            None => return None,
        }
    }
    Some(score)
}

/// F254 模糊搜索：按分值降序写入 `out`，返回命中数。
pub fn search(menu: &StartMenu, query: &[u8], out: &mut [u32]) -> usize {
    if query.is_empty() || out.is_empty() {
        return 0;
    }
    let mut scored = [(0u32, 0u32); MAX_MENU_APPS];
    let mut n = 0usize;
    for i in 0..menu.count {
        if let Some(app) = menu.apps[i] {
            if let Some(s) = subsequence_score(app.key, query) {
                scored[n] = (s, app.id);
                n += 1;
            }
        }
    }
    // 定长选择排序：分值降序，同分按 id 升序（结果稳定可重现）。
    for i in 0..n {
        let mut best = i;
        for j in i + 1..n {
            if scored[j].0 > scored[best].0
                || (scored[j].0 == scored[best].0 && scored[j].1 < scored[best].1)
            {
                best = j;
            }
        }
        scored.swap(i, best);
    }
    let limit = n.min(out.len());
    for i in 0..limit {
        out[i] = scored[i].1;
    }
    limit
}

// ---------------------------------------------------------------------------
// F255 — 跳转列表
// ---------------------------------------------------------------------------

pub struct JumpList {
    pub entries: [Option<&'static str>; MAX_JUMP],
    pub count: usize,
}

impl JumpList {
    pub const fn new() -> JumpList {
        JumpList { entries: [None; MAX_JUMP], count: 0 }
    }

    pub fn push(&mut self, entry: &'static str) -> bool {
        if self.entries.iter().any(|e| *e == Some(entry)) {
            return true;
        }
        if self.count >= MAX_JUMP {
            return false;
        }
        self.entries[self.count] = Some(entry);
        self.count += 1;
        true
    }

    pub fn clear(&mut self) {
        self.entries = [None; MAX_JUMP];
        self.count = 0;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StartMenu {
        let mut m = StartMenu::new();
        m.add(MenuApp { id: 1, name: "写作空间", key: "write", pinned: true, folder: 0 });
        m.add(MenuApp { id: 2, name: "思维导图", key: "mind", pinned: false, folder: 0 });
        m.add(MenuApp { id: 3, name: "代码分析", key: "code", pinned: true, folder: 1 });
        m
    }

    #[test]
    fn f253_grid_and_pins() {
        let m = sample();
        assert_eq!(m.count, 3);
        assert_eq!(m.pinned_count(), 2);
        assert_eq!(m.folder_count(1), 1);
        assert_eq!(grid_rows(0, 3), 0);
        assert_eq!(grid_rows(7, 3), 3);
        assert_eq!(grid_cell(4, 3), (1, 1));
    }

    #[test]
    fn f254_search_ranks_contiguous_higher() {
        let m = sample();
        let mut out = [0u32; MAX_RESULTS];
        assert_eq!(search(&m, b"wr", &mut out), 1);
        assert_eq!(out[0], 1);
        assert_eq!(search(&m, b"ind", &mut out), 1);
        assert_eq!(out[0], 2);
        assert_eq!(search(&m, b"de", &mut out), 1);
        assert_eq!(out[0], 3);
        assert_eq!(search(&m, b"", &mut out), 0);
        assert_eq!(search(&m, b"zz", &mut out), 0);
        // 大写同样命中（查询大小写不敏感）
        assert_eq!(search(&m, b"MIND", &mut out), 1);
    }

    #[test]
    fn f254_query_state_and_selection() {
        let mut m = sample();
        assert_eq!(m.set_query(b"Code"), 4);
        assert_eq!(&m.query[..m.query_len], b"code");
        m.move_sel(3, 1);
        assert_eq!(m.sel, 1);
        m.move_sel(3, -1);
        assert_eq!(m.sel, 0);
        m.move_sel(3, -1);
        assert_eq!(m.sel, 2);
        m.clear_query();
        assert_eq!(m.query_len, 0);
    }

    #[test]
    fn f255_jump_list_bounded() {
        let mut j = JumpList::new();
        assert!(j.push("新建记录"));
        assert!(j.push("最近：第九章"));
        assert!(j.push("固定：草稿箱"));
        assert!(j.push("设置"));
        assert_eq!(j.count, MAX_JUMP);
        assert!(!j.push("溢出"));
        assert!(j.push("新建记录"));
        assert_eq!(j.count, MAX_JUMP);
        j.clear();
        assert_eq!(j.count, 0);
    }

    #[test]
    fn f253_capacity_is_honest() {
        let mut m = StartMenu::new();
        for i in 0..(MAX_MENU_APPS as u32 + 4) {
            m.add(MenuApp { id: i, name: "a", key: "a", pinned: false, folder: 0 });
        }
        assert_eq!(m.count, MAX_MENU_APPS);
    }
}
