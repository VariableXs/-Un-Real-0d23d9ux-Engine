//! UNX-F0114 · 诊断模式控制台 UI 骨架 · 诊断段 0x23C0
//! 验收锚点：UNX-F0114-J1 菜单树/选中态/翻页/热键四要素骨架账，
//! 越纲操作全拦。零浮点、零 unwrap、零 expect。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x23C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ITEM: u16 = SEG_BASE + 1; // 菜单项越纲（页内闭集外）
const E_PAGE: u16 = SEG_BASE + 2; // 翻页越界（无上/下页）
const E_KEY: u16 = SEG_BASE + 3;  // 热键未绑（闭集外按键）
const E_EMPTY: u16 = SEG_BASE + 4;// 空页渲染
const E_TWICE: u16 = SEG_BASE + 5;// 热键重复绑定
const E_RDONLY: u16 = SEG_BASE + 6;// UI 已退出，再操作被拒

pub fn detail(code: u16) -> &'static str {
    match code {
        E_ITEM => "菜单项越纲：选中下标出页内闭集",
        E_PAGE => "翻页越界：首页无上翻，末页无下翻",
        E_KEY => "热键未绑：按键不在绑定表",
        E_EMPTY => "空页：无菜单项禁止渲染",
        E_TWICE => "热键重复绑定：一键一动作",
        E_RDONLY => "UI 已退出：退出态禁止再操作",
        _ => "未知诊断码",
    }
}

pub const ITEMS_PER_PAGE: usize = 6;

/// 菜单项。
#[derive(Clone)]
pub struct MenuItem {
    pub title: String,
    pub action: u16, // 动作码（页内闭集）
}

/// 单页。
pub struct MenuPage {
    pub items: Vec<MenuItem>,
}

impl MenuPage {
    pub fn render(&self, sel: usize) -> Result<String, u16> {
        if self.items.is_empty() {
            return Err(E_EMPTY);
        }
        if sel >= self.items.len() {
            return Err(E_ITEM);
        }
        let mut s = String::new();
        for (i, it) in self.items.iter().enumerate() {
            let mark = if i == sel { ">" } else { " " };
            s.push_str(&format!("{} [{}] {}\n", mark, it.action, it.title));
        }
        Ok(s)
    }
}

/// UI 骨架：多页菜单 + 选中态 + 热键表。
pub struct DiagUi {
    pub pages: Vec<MenuPage>,
    pub page: usize,
    pub sel: usize,
    pub keys: Vec<(char, u16)>, // 键 → 动作码
    pub exited: bool,
    pub page_turns: u64,
}

impl DiagUi {
    pub fn new() -> DiagUi {
        DiagUi {
            pages: Vec::new(),
            page: 0,
            sel: 0,
            keys: Vec::new(),
            exited: false,
            page_turns: 0,
        }
    }

    /// 装页：菜单项按 ITEMS_PER_PAGE 自动分页。
    pub fn load(&mut self, titles: &[&str], base_action: u16) -> Result<usize, u16> {
        if self.exited {
            return Err(E_RDONLY);
        }
        if titles.is_empty() {
            return Err(E_EMPTY);
        }
        let mut n = 0;
        for chunk in titles.chunks(ITEMS_PER_PAGE) {
            let mut p = MenuPage {
                items: Vec::new(),
            };
            for (i, t) in chunk.iter().enumerate() {
                p.items.push(MenuItem {
                    title: t.to_string(),
                    action: base_action + i as u16,
                });
            }
            self.pages.push(p);
            n += 1;
        }
        Ok(n)
    }

    pub fn bind_key(&mut self, key: char, action: u16) -> Result<(), u16> {
        if self.exited {
            return Err(E_RDONLY);
        }
        if self.keys.iter().any(|(k, _)| *k == key) {
            return Err(E_TWICE);
        }
        self.keys.push((key, action));
        Ok(())
    }

    /// 下移选中项。
    pub fn move_down(&mut self) -> Result<usize, u16> {
        if self.exited {
            return Err(E_RDONLY);
        }
        let n = self.pages[self.page].items.len();
        if self.sel + 1 >= n {
            return Err(E_ITEM);
        }
        self.sel += 1;
        Ok(self.sel)
    }

    /// 上移选中项。
    pub fn move_up(&mut self) -> Result<usize, u16> {
        if self.exited {
            return Err(E_RDONLY);
        }
        if self.sel == 0 {
            return Err(E_ITEM);
        }
        self.sel -= 1;
        Ok(self.sel)
    }

    /// 下翻页。
    pub fn next_page(&mut self) -> Result<usize, u16> {
        if self.exited {
            return Err(E_RDONLY);
        }
        if self.page + 1 >= self.pages.len() {
            return Err(E_PAGE);
        }
        self.page += 1;
        self.sel = 0;
        self.page_turns += 1;
        Ok(self.page)
    }

    /// 上翻页。
    pub fn prev_page(&mut self) -> Result<usize, u16> {
        if self.exited {
            return Err(E_RDONLY);
        }
        if self.page == 0 {
            return Err(E_PAGE);
        }
        self.page -= 1;
        self.sel = 0;
        self.page_turns += 1;
        Ok(self.page)
    }

    /// 热键命中：查绑定表 → 动作码。
    pub fn press(&self, key: char) -> Result<u16, u16> {
        if self.exited {
            return Err(E_RDONLY);
        }
        for (k, a) in &self.keys {
            if *k == key {
                return Ok(*a);
            }
        }
        Err(E_KEY)
    }

    /// 确认执行：当前选中项动作码。
    pub fn confirm(&self) -> Result<u16, u16> {
        if self.exited {
            return Err(E_RDONLY);
        }
        let p = &self.pages[self.page];
        if self.sel >= p.items.len() {
            return Err(E_ITEM);
        }
        Ok(p.items[self.sel].action)
    }

    /// 退出：终态锁。
    pub fn quit(&mut self) -> Result<(), u16> {
        if self.exited {
            return Err(E_RDONLY);
        }
        self.exited = true;
        Ok(())
    }

    /// 渲染当前页 + 翻页脚注。
    pub fn render(&self) -> Result<String, u16> {
        if self.exited {
            return Err(E_RDONLY);
        }
        if self.pages.is_empty() {
            return Err(E_EMPTY);
        }
        let mut s = self.pages[self.page].render(self.sel)?;
        s.push_str(&format!(
            "—— 第 {}/{} 页，翻页 {} 次 ——",
            self.page + 1,
            self.pages.len(),
            self.page_turns
        ));
        Ok(s)
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：13 项装页 → 3 页（6/6/1），翻页两转，选中态复位。
    let mut ui = DiagUi::new();
    let titles: Vec<&str> = (1..=13).map(|_| "菜单项").collect();
    let n = ui.load(&titles, 100)?;
    if n != 3 || ui.pages.len() != 3 {
        return Err(E_EMPTY);
    }
    if ui.pages[0].items.len() != 6 || ui.pages[2].items.len() != 1 {
        return Err(E_ITEM);
    }
    ui.next_page()?;
    ui.next_page()?;
    if ui.page != 2 || ui.sel != 0 || ui.page_turns != 2 {
        return Err(E_PAGE);
    }
    hits += 1;

    // R2：渲染含选中标记与翻页脚注；确认返回当前动作码。
    ui.prev_page()?;
    ui.move_down()?;
    ui.move_down()?;
    let rep = ui.render()?;
    if !rep.contains("> [102]") || !rep.contains("第 2/3 页") {
        return Err(E_ITEM);
    }
    if ui.confirm()? != 102 {
        return Err(E_ITEM);
    }
    hits += 1;

    // R3：热键绑定与命中（q=9 退出，d=2 下翻）。
    let mut ui3 = DiagUi::new();
    ui3.load(&["甲", "乙"], 10)?;
    ui3.bind_key('d', 2)?;
    ui3.bind_key('q', 9)?;
    if ui3.press('d')? != 2 || ui3.press('q')? != 9 {
        return Err(E_KEY);
    }
    hits += 1;

    // R4：选中态上下移动闭环 + 边界内往返。
    let mut ui4 = DiagUi::new();
    ui4.load(&["p", "q", "r"], 20)?;
    ui4.move_down()?;
    ui4.move_down()?;
    if ui4.sel != 2 {
        return Err(E_ITEM);
    }
    ui4.move_up()?;
    ui4.move_up()?;
    if ui4.sel != 0 {
        return Err(E_ITEM);
    }
    hits += 1;

    // I1：选中越纲（顶端上移 / 底端下移）→ E_ITEM。
    let mut i1 = DiagUi::new();
    i1.load(&["x"], 30)?;
    if i1.move_up() != Err(E_ITEM) {
        return Err(E_ITEM);
    }
    if i1.move_down() != Err(E_ITEM) {
        return Err(E_ITEM);
    }
    hits += 1;

    // I2：翻页越界（首页上翻 / 末页下翻）→ E_PAGE。
    let mut i2 = DiagUi::new();
    i2.load(&["a", "b", "c", "d", "e", "f", "g"], 40)?; // 2 页
    if i2.prev_page() != Err(E_PAGE) {
        return Err(E_PAGE);
    }
    i2.next_page()?;
    if i2.next_page() != Err(E_PAGE) {
        return Err(E_PAGE);
    }
    hits += 1;

    // I3：未绑热键 → E_KEY；重复绑定 → E_TWICE。
    if i2.press('z') != Err(E_KEY) {
        return Err(E_KEY);
    }
    i2.bind_key('h', 7)?;
    if i2.bind_key('h', 8) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    // I4：空装页 → E_EMPTY；空页渲染 → E_EMPTY。
    let mut i4 = DiagUi::new();
    if i4.load(&[], 1) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if i4.render() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I5：退出态全操作拒绝（移动/翻页/热键/确认/渲染/退出/装页）。
    let mut i5 = DiagUi::new();
    i5.load(&["solo"], 50)?;
    i5.quit()?;
    if i5.quit() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i5.move_down() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i5.next_page() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i5.press('s') != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i5.confirm() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i5.render().is_ok() {
        return Err(E_RDONLY);
    }
    if i5.load(&["y"], 1) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
