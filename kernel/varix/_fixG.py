# -*- coding: utf-8 -*-
import io

# 1) alttab: set_candidates 不截断——24 上限移到墙视图分页层（scroll 语义）
p = 'src/deskstar/alttab.rs'
s = io.open(p, encoding='utf-8').read()
s = s.replace('''    /// 候选注入（F072 最近序投影——调用方把 ranked 应用序灌进来；
    /// UAC 类系统窗在此被挡在墙外并记账）。
    pub fn set_candidates(&mut self, cards: Vec<TabCard>) {
        self.excluded += cards.iter().filter(|c| c.system).count() as u64;
        self.cards = cards
            .into_iter()
            .filter(|c| !c.system)
            .take(WALL_CAP)
            .collect();
    }''', '''    /// 候选注入（F072 最近序投影——调用方把 ranked 应用序灌进来；
    /// UAC 类系统窗在此被挡在墙外并记账。24 上限在**墙视图分页层**
    /// 生效——超限滚动页查其余，不再静默截断候选清单）。
    pub fn set_candidates(&mut self, cards: Vec<TabCard>) {
        self.excluded += cards.iter().filter(|c| c.system).count() as u64;
        self.cards = cards.into_iter().filter(|c| !c.system).collect();
        self.scroll_page = 0;
    }''')
# serpentine 布局改为按当前页切片
s = s.replace('''    fn layout_serpentine(&mut self) {
        self.serpentine.clear();
        let n = self.wall_cards().len();
        let cols = 4usize;''', '''    fn layout_serpentine(&mut self) {
        self.serpentine.clear();
        let n = self.page_cards().len();
        let cols = 4usize;''')
# layout_is_tidy 对齐页切片
s = s.replace('''    pub fn layout_is_tidy(&self) -> bool {
        let n = self.serpentine.len();
        if n != self.cards.len() {
            return false;
        }''', '''    pub fn layout_is_tidy(&self) -> bool {
        let n = self.serpentine.len();
        if n != self.page_cards().len() {
            return false;
        }''')
io.open(p, 'w', encoding='utf-8').write(s)

# 2) alttab 主自检 cap-24 语义更新
s = io.open(p, encoding='utf-8').read()
s = s.replace('''    // 8. 上限 24（超出滚动截断）。
    let mut many: Vec<TabCard> = Vec::new();
    for i in 0..30u64 {
        many.push(mk(i, "窗", false, false));
    }
    at.set_candidates(many);
    set.add(
        "cap-24",
        at.candidates().len() == WALL_CAP,
        "scroll beyond 24",
    );''', '''    // 8. 上限 24（墙视图分页语义：候选全保留、单页 24——深化层 scroll-pages 细化）。
    let mut many: Vec<TabCard> = Vec::new();
    for i in 0..30u64 {
        many.push(mk(i, "窗", false, false));
    }
    at.set_candidates(many);
    set.add(
        "cap-24",
        at.candidates().len() == 30 && at.page_cards().len() == WALL_CAP,
        "paged wall view",
    );''')
io.open(p, 'w', encoding='utf-8').write(s)

# 3) taskview: wall-close 断言改桌 0；merge-notice 改关桌 0（持 6 窗）
p = 'src/deskstar/taskview.rs'
s = io.open(p, encoding='utf-8').read()
s = s.replace('''    // 4. 墙内关窗（Delete）。
    let closed = tv.wall_close(6) && !tv.wall_close(99);
    set.add(
        "wall-close",
        closed && tv.desk_window_count(1) == 6,
        "delete closes window",
    );''', '''    // 4. 墙内关窗（Delete）——窗 6 在桌 1（idx 0；悬停切换只动 active 不动归属）。
    let count_before = tv.desk_window_count(0);
    let closed = tv.wall_close(6) && !tv.wall_close(99);
    set.add(
        "wall-close",
        closed && tv.desk_window_count(0) == count_before - 1,
        "delete closes window",
    );''')
s = s.replace('''    // 8. 结构化并入通知（toast 数据源含并入桌名；v2 树里桌 2 持 6 窗）。
    let notice = tv2.close_desk_notice(1, 5_000);
    set.add(
        "merge-notice",
        notice.map(|n| n.moved_count == 6) == Some(true),
        "structured toast source",
    );''', '''    // 8. 结构化并入通知（关桌 1 = 工作台，持 6 窗并入桌 2）。
    let notice = tv2.close_desk_notice(0, 5_000);
    set.add(
        "merge-notice",
        notice.map(|n| n.moved_count == 6) == Some(true),
        "structured toast source",
    );''')
io.open(p, 'w', encoding='utf-8').write(s)
print('fixG ok')
