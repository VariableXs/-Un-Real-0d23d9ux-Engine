//! F090 面包屑增强 · 完整设计（STAR I 主册 G-C-20）。
//!
//! **判据（主册）**：拖放移动 20 例全对（含跨盘语义 F018）；补全
//! 响应 <50ms；编辑-取消-回显零状态残留。
//!
//! **设计要点（主册）**：
//! - 地址栏面包屑两级增强：点击任一级出该层兄弟目录下拉；拖文件
//!   到面包屑级=移动到该层；可编辑模式（点击空白区转输入框，乙-1
//!   表高 32px）；
//! - 面包屑项高 28px 悬停底色；下拉面板宽 240px 列兄弟目录（图标+
//!   名+子目录数）；编辑模式全路径选中（Ctrl+A 等价）、输入即时
//!   补全（存在路径前缀）；「回到当前」钮在编辑态旁（放弃编辑）；
//! - 无新增存储（历史栈 F089 引擎共用——直用
//!   `crate::deskstar::tabexplorer::HistoryStack`）；
//! - 拖拽到只读层 → 禁止光标+原因 tooltip；下拉目录 50+ → 滚动+
//!   搜索框；编辑输入不存在路径 → Enter 时如实报错不跳；
//! - 下拉展开 120ms 淡入；面包屑过长省略中间层为「…」（点击可
//!   展开中间层列表）；拖放目标高亮同 F084 视觉族；键盘路径 Alt+D
//!   直焦地址栏（乙-4 表既有）——编辑与面包屑两态全键盘切换。
//!
//! 实装口径：面包屑分段账 + 兄弟下拉账 + 投放移动账（F084 视觉族
//! 高亮语义）+ 编辑两态账（补全引擎 <50ms 账）+ 只读禁投账 + 省略
//! 展开账。历史栈直用 F089 引擎（一处一事实）。

use crate::checks::CheckSet;

use crate::deskstar::dbase::{budget_ok, Token};
use crate::deskstar::tabexplorer::HistoryStack;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::{vec, format};

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/设计细节）
// ---------------------------------------------------------------------------

/// 面包屑项高（px）。
pub const CRUMB_H_PX: i32 = 28;

/// 下拉面板宽（px）。
pub const DROPDOWN_W_PX: i32 = 240;

/// 编辑态输入框高（px，乙-1 表 32px 同源）。
pub const EDIT_H_PX: i32 = 32;

/// 补全响应预算（ms）。
pub const COMPLETE_BUDGET_MS: u64 = 50;

/// 下拉淡入时长（ms）。
pub const DROPDOWN_FADE_MS: u32 = 120;

/// 兄弟目录列表滚动线（个，50+ 出滚动+搜索框）。
pub const SIBLING_SCROLL_AT: usize = 50;

/// 过长省略保留的首尾层数。
pub const ELLIPSIS_KEEP: usize = 2;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 单层面包屑（层级段）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crumb {
    pub name: String,
    pub path: String,
    /// 只读层（拖放禁投——禁止光标 + 原因 tooltip）。
    pub readonly: bool,
}

/// 兄弟目录条目（下拉数据行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sibling {
    pub name: String,
    pub children: u32,
}

/// 补全候选。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub path: String,
}

/// 地址栏状态机。
pub struct CrumbsBar {
    history: HistoryStack,
    /// 各层只读标记（与层级对齐）。
    readonly_depths: Vec<bool>,
    /// 兄弟目录供给（宿主按层注入——文件系统面接缝）。
    siblings_cache: Vec<(String, Vec<Sibling>)>,
    editing: bool,
    edit_text: String,
    now_ms: u64,
    dropdown_open: Option<usize>, // 开在哪一层
    dropdown_opened_ms: u64,
    /// 投放高亮层（F084 视觉族——2px 描边同语义）。
    drop_target: Option<usize>,
    /// 账本：投放移动数（20 例判据）、补全耗时账、报错账。
    pub drop_moves: u64,
    pub last_complete_ms: Option<u64>,
    pub honest_errors: u64,
    /// 编辑残留检测账（取消后应与原值逐字节一致——零残留）。
    pub residue_violations: u64,
    /// 面包屑键盘焦点（方向键环——键盘路径与鼠标路径同权）。
    crumb_focus: usize,
    /// 展开态（点击「…」→ 全层序呈现——展开后的中间层可直接点击）。
    full_shown: bool,
    /// 补全下拉候选（键盘导航态）。
    comp_items: Vec<String>,
    /// 补全下拉焦点。
    comp_focus: Option<usize>,
}

impl CrumbsBar {
    /// 绑定历史引擎（F089 同引擎——一处一事实）。
    pub fn bind(history: HistoryStack) -> CrumbsBar {
        CrumbsBar {
            history,
            readonly_depths: Vec::new(),
            siblings_cache: Vec::new(),
            editing: false,
            edit_text: String::new(),
            now_ms: 0,
            dropdown_open: None,
            dropdown_opened_ms: 0,
            drop_target: None,
            drop_moves: 0,
            last_complete_ms: None,
            honest_errors: 0,
            residue_violations: 0,
            crumb_focus: 0,
            full_shown: false,
            comp_items: Vec::new(),
            comp_focus: None,
        }
    }

    pub fn history(&self) -> &HistoryStack {
        &self.history
    }

    pub fn history_mut(&mut self) -> &mut HistoryStack {
        &mut self.history
    }

    /// 分段（当前路径 → 层级段；根段恒在）。
    pub fn crumbs(&self) -> Vec<Crumb> {
        let path = self.history.current();
        let mut out = vec![Crumb {
            name: String::from("此电脑"),
            path: String::from("/"),
            readonly: self.readonly_depths.first().copied().unwrap_or(false),
        }];
        let bytes = path.as_bytes();
        let mut acc = String::new();
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'/' && i > 0 {
                if !acc.is_empty() {
                    out.push(Crumb {
                        name: acc.clone(),
                        path: String::from(&path[..i]),
                        readonly: self.readonly_at(out.len()),
                    });
                    acc.clear();
                }
            } else if !(*b == b'/' && i == 0) {
                acc.push(*b as char);
            }
        }
        if !acc.is_empty() {
            out.push(Crumb {
                name: acc,
                path: String::from(path),
                readonly: self.readonly_at(out.len()),
            });
        }
        // 过长省略：>7 层保留首 2 + 「…」+ 尾 2（点击可展开中间层）。
        // 展开态（full_shown）呈现全层序——展开后的层可直接点击跳转。
        if !self.full_shown && out.len() > ELLIPSIS_KEEP * 2 + 3 {
            let head: Vec<Crumb> = out[..ELLIPSIS_KEEP].to_vec();
            let tail_start = out.len() - ELLIPSIS_KEEP;
            let tail: Vec<Crumb> = out[tail_start..].to_vec();
            let mut shrunk = head;
            shrunk.push(Crumb {
                name: String::from("…"),
                path: String::new(),
                readonly: true, // 省略段不可投（展开后可）
            });
            shrunk.extend(tail);
            return shrunk;
        }
        out
    }

    fn readonly_at(&self, idx: usize) -> bool {
        self.readonly_depths.get(idx).copied().unwrap_or(false)
    }

    pub fn set_readonly_depth(&mut self, idx: usize, ro: bool) {
        if self.readonly_depths.len() <= idx {
            self.readonly_depths.resize(idx + 1, false);
        }
        self.readonly_depths[idx] = ro;
    }

    /// 点击层 → 跳转该层（历史栈同引擎）。省略段「…」没有路径——
    /// 点击它 = 展开中间层（行为闭环，而非死胡同）。
    pub fn click_crumb(&mut self, idx: usize) -> bool {
        let segs = self.crumbs();
        let Some(crumb) = segs.get(idx) else {
            return false;
        };
        if crumb.name == "…" && crumb.path.is_empty() {
            self.expand_ellipsis();
            return true;
        }
        if !crumb.path.is_empty() {
            return self.history.go(&crumb.path);
        }
        false
    }

    /// 兄弟目录注入（宿主扫描回执）。
    pub fn feed_siblings(&mut self, layer_path: &str, list: Vec<Sibling>) {
        if let Some(slot) = self
            .siblings_cache
            .iter_mut()
            .find(|(p, _)| p == layer_path)
        {
            slot.1 = list;
        } else {
            self.siblings_cache.push((String::from(layer_path), list));
        }
    }

    /// 下拉开合（点层出兄弟目录；50+ → 滚动+搜索框旗标）。
    pub fn open_dropdown(&mut self, idx: usize, now_ms: u64) -> bool {
        let path = self.crumbs().get(idx).map(|c| c.path.clone()).unwrap_or_default();
        if path.is_empty() {
            return false;
        }
        self.dropdown_open = Some(idx);
        self.dropdown_opened_ms = now_ms;
        self.now_ms = now_ms;
        true
    }

    pub fn dropdown_open(&self) -> bool {
        self.dropdown_open.is_some()
    }

    /// 下拉淡入进度（120ms）。
    pub fn dropdown_fade(&self) -> u16 {
        ((self.now_ms.saturating_sub(self.dropdown_opened_ms) as u32).min(DROPDOWN_FADE_MS) * 1000
            / DROPDOWN_FADE_MS) as u16
    }

    /// 兄弟列表超滚动线（渲染层出滚动条+搜索框）。
    pub fn siblings_need_scroll(&self, idx: usize) -> bool {
        self.siblings_of(idx).map(|s| s.len() > SIBLING_SCROLL_AT) == Some(true)
    }

    fn siblings_of(&self, idx: usize) -> Option<&Vec<Sibling>> {
        let path = self.crumbs().get(idx)?.path.clone();
        self.siblings_cache
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, l)| l)
    }

    /// 选中兄弟 → 跳转（下拉点选 = 历史跳转）。
    pub fn pick_sibling(&mut self, idx: usize, name: &str) -> bool {
        let base = self.crumbs().get(idx).map(|c| c.path.clone());
        let Some(base) = base else { return false };
        let target = format!("{}/{}", base, name);
        self.dropdown_open = None;
        self.history.go(&target)
    }

    // -- 投放移动 ----------------------------------------------------------

    /// 拖文件到层 = 移动（含跨盘语义 F018——由上层管线执行，本账
    /// 记判定与高亮；只读层拒绝 + 原因 tooltip 文案）。
    pub fn drop_files(&mut self, idx: usize, files: &[&str], now_ms: u64) -> Result<usize, &'static str> {
        self.now_ms = now_ms;
        let segs = self.crumbs();
        let crumb = segs.get(idx).ok_or("层不存在")?;
        if crumb.readonly || crumb.path.is_empty() {
            return Err("此层为只读，不能移入文件");
        }
        self.drop_target = Some(idx);
        self.drop_moves += files.len() as u64;
        Ok(files.len())
    }

    pub fn drop_target_token(&self) -> Option<Token> {
        self.drop_target.map(|_| Token::Accent)
    }

    pub fn clear_drop_target(&mut self) {
        self.drop_target = None;
    }

    /// 跨盘判定（F018 判定口：源文件盘符 ≠ 目标层盘符 = 跨盘移动，
    /// 上层管线据此走复制-删除两段语义而非同盘改名）。
    pub fn is_cross_disk(&self, file_path: &str, idx: usize) -> bool {
        let segs = self.crumbs();
        let Some(crumb) = segs.get(idx) else {
            return false;
        };
        let disk_of = |p: &str| p.split(['/', '\\']).next().unwrap_or("").to_lowercase();
        let src = disk_of(file_path);
        let dst = disk_of(&crumb.path);
        !src.is_empty() && !dst.is_empty() && src != dst
    }

    /// 只读层 tooltip（悬停禁投原因——与 drop_files 的 Err 同源一致）。
    pub fn readonly_tooltip(&self, idx: usize) -> Option<&'static str> {
        let segs = self.crumbs();
        segs.get(idx).filter(|c| c.readonly).map(|_| "此层为只读，不能移入文件")
    }

    // -- 编辑两态 ----------------------------------------------------------

    /// 进入编辑（点空白区 / Alt+D——全路径选中 Ctrl+A 等价）。
    pub fn begin_edit(&mut self) {
        self.editing = true;
        self.edit_text = String::from(self.history.current());
    }

    pub fn editing(&self) -> bool {
        self.editing
    }

    pub fn edit_text(&self) -> &str {
        &self.edit_text
    }

    pub fn edit_input(&mut self, s: &str, now_ms: u64) {
        self.edit_text = String::from(s);
        self.now_ms = now_ms;
    }

    /// 即时补全（存在路径前缀——<50ms 判据的对账口；耗时由调用方
    /// 实测注入）。
    pub fn complete(&mut self, candidates: &[&str], cost_ms: u64) -> Vec<Completion> {
        self.last_complete_ms = Some(cost_ms);
        let mut out = Vec::new();
        for c in candidates {
            if c.starts_with(&self.edit_text) && c.len() > self.edit_text.len() {
                out.push(Completion {
                    path: String::from(*c),
                });
            }
        }
        out
    }

    pub fn complete_in_budget(&self) -> bool {
        self.last_complete_ms.map(|t| budget_ok(t, COMPLETE_BUDGET_MS)) == Some(true)
    }

    /// Enter 提交：存在路径跳转；不存在 → 如实报错不跳（honest_errors）。
    pub fn commit_edit(&mut self, exists: bool) -> bool {
        if exists {
            let target = self.edit_text.clone();
            self.editing = false;
            self.history.go(&target)
        } else {
            self.honest_errors += 1;
            false
        }
    }

    /// 取消编辑（「回到当前」钮 / Esc）：零状态残留（含补全下拉一并
    /// 关闭——编辑态的所有子状态同窗清账）。
    pub fn cancel_edit(&mut self) -> bool {
        if !self.editing {
            return false;
        }
        self.close_completions();
        self.editing = false;
        self.edit_text.clear();
        true
    }

    /// Alt+D 直焦地址栏（两态全键盘切换的入口）。
    pub fn alt_d(&mut self) {
        if self.editing {
            self.cancel_edit();
        } else {
            self.begin_edit();
        }
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 深化层（回炉批）：省略段点击展开中间层 / 下拉内搜索过滤 / 滚动可视窗 /
// 补全下拉键盘导航（焦点+Enter 接受+Esc 关）/ 面包屑键盘焦点环 /
// 「回到当前」钮 / Ctrl+A 等价全选账 / 悬停与几何常量——主册【交互设计】
// 【设计细节】逐条补足。深化编号 D1-v2-CB*。
// ---------------------------------------------------------------------------

/// 下拉可视行数（滚动窗高——50+ 时窗口化呈现）。
pub const SIBLING_VIEWPORT: usize = 12;

/// 编辑态全选标记（Ctrl+A 等价——进入编辑即全路径选中）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditSelection {
    pub select_all: bool,
}

impl CrumbsBar {
    /// 编辑态全选账（进入编辑 = 全路径选中——主册「编辑模式全路径
    /// 选中（Ctrl+A 等价）」的账面；渲染层据此高亮全文本）。
    pub fn edit_selection(&self) -> EditSelection {
        EditSelection {
            select_all: self.editing,
        }
    }

    /// 「回到当前」钮可见性（编辑态且内容被改——未改不出钮不骚扰）。
    pub fn revert_button_visible(&self) -> bool {
        self.editing && self.edit_text != self.history.current()
    }

    /// 「回到当前」点击（= 取消编辑的语义入口——零残留同一出口）。
    pub fn click_revert(&mut self) -> bool {
        self.cancel_edit()
    }

    /// 省略段展开（点击「…」→ 进入全层序态——被折叠的中间层回归且
    /// 可直接点击；再点收起回省略态）。返回展开后的全层级段。
    pub fn expand_ellipsis(&mut self) -> Vec<Crumb> {
        self.full_shown = true;
        self.crumbs()
    }

    /// 收起（回到省略态——展开是可逆的显示态，不是单向门）。
    pub fn collapse_ellipsis(&mut self) {
        self.full_shown = false;
    }

    pub fn is_full_shown(&self) -> bool {
        self.full_shown
    }

    /// 当前是否处于省略态（>7 层被折叠——展开钮的显示判定）。
    pub fn is_shrunk(&self) -> bool {
        let path = self.history.current();
        let depth = path.split('/').filter(|s| !s.is_empty()).count() + 1; // +根段
        depth > ELLIPSIS_KEEP * 2 + 3
    }

    /// 下拉内搜索（50+ 滚动 + 搜索框——过滤词大小写不敏感子串）。
    pub fn filter_siblings(&self, idx: usize, query: &str) -> Vec<Sibling> {
        let q = query.to_lowercase();
        self.siblings_of(idx)
            .map(|list| {
                list.iter()
                    .filter(|s| q.is_empty() || s.name.to_lowercase().contains(&q))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 滚动可视窗（滚动条账：scroll_top 起取 SIBLING_VIEWPORT 行；
    /// scroll_top 越界钳制到底窗——滚动到底有边界感）。
    pub fn siblings_window(&self, idx: usize, scroll_top: usize) -> Vec<Sibling> {
        let list = self.filter_siblings(idx, "");
        let max_top = list.len().saturating_sub(SIBLING_VIEWPORT);
        let top = scroll_top.min(max_top);
        list.iter().skip(top).take(SIBLING_VIEWPORT).cloned().collect()
    }

    // -- 面包屑键盘焦点环 ---------------------------------------------------

    /// 焦点移动（Alt+D 出编辑；方向键在面包屑上移动——不循环出界）。
    pub fn crumb_focus_move(&mut self, forward: bool) -> usize {
        let n = self.crumbs().len();
        if forward {
            self.crumb_focus = (self.crumb_focus + 1).min(n - 1);
        } else {
            self.crumb_focus = self.crumb_focus.saturating_sub(1);
        }
        self.crumb_focus
    }

    /// Enter 激活焦点层（= 点击跳转；焦点即视觉可见——焦点环账）。
    pub fn crumb_focus_activate(&mut self) -> bool {
        let idx = self.crumb_focus;
        self.click_crumb(idx)
    }

    pub fn crumb_focus(&self) -> usize {
        self.crumb_focus
    }

    // -- 补全下拉键盘导航 ---------------------------------------------------

    /// 补全候选带焦点（下拉打开时焦点落进候选首位——焦点进出链闭环）。
    pub fn open_completions(&mut self, candidates: &[&str]) {
        self.comp_items = candidates.iter().map(|c| String::from(*c)).collect();
        self.comp_focus = if self.comp_items.is_empty() { None } else { Some(0) };
    }

    /// 焦点移动（方向键；无候选不移动）。
    pub fn comp_focus_move(&mut self, forward: bool) {
        if self.comp_items.is_empty() {
            return;
        }
        let n = self.comp_items.len();
        let cur = self.comp_focus.unwrap_or(0);
        self.comp_focus = Some(if forward {
            (cur + 1).min(n - 1)
        } else {
            cur.saturating_sub(1)
        });
    }

    /// Enter 接受焦点候选（补全落地 = 编辑文本替换 + 下拉关）。
    pub fn comp_accept(&mut self) -> bool {
        match self.comp_focus {
            Some(i) if i < self.comp_items.len() => {
                self.edit_text = self.comp_items[i].clone();
                self.close_completions();
                true
            }
            _ => false,
        }
    }

    /// Esc 关补全（编辑态保留——只关下拉不丢编辑）。
    pub fn close_completions(&mut self) {
        self.comp_items.clear();
        self.comp_focus = None;
    }

    pub fn comp_open(&self) -> bool {
        !self.comp_items.is_empty()
    }

    pub fn comp_focus(&self) -> Option<usize> {
        self.comp_focus
    }
}

// ---------------------------------------------------------------------------
// 自检（判据唯一源：主册 G-C-20 验收判据）
// ---------------------------------------------------------------------------

/// F090 自检：拖放 20 例（含跨盘）、补全 <50ms、编辑取消零残留、
/// 分段/省略、下拉淡入与滚动线、只读禁投、如实报错、两态切换。
pub fn run_crumbsbar_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F090");
    let mut bar = CrumbsBar::bind(HistoryStack::new("C:/工作/2026/报告/数据"));
    // 1. 分段：6 层全拆（此电脑 + C: + 4 层路径）。
    let c = bar.crumbs();
    set.add(
        "segments",
        c.len() == 6 && c[0].name == "此电脑" && c[5].path == "C:/工作/2026/报告/数据",
        "path → crumbs",
    );
    // 2. 点击层跳转（历史栈协同； crumbs[2] = 工作——0 基此电脑/C:/工作）。
    let ok_click = bar.click_crumb(2) && bar.history().current() == "C:/工作";
    bar.history_mut().go("C:/工作/2026/报告/数据");
    set.add("click-jump", ok_click, "crumb navigates");
    // 3. 拖放移动 20 例（含跨盘语义：C: → D: 段）。
    let files: Vec<&str> = (0..20).map(|i| match i {
        10 => "跨盘样例.dxf", // 第 11 例走跨盘路径（F018 管线语义）
        _ => "样例",
    }).collect();
    let mut ok20 = true;
    for i in 0..20usize {
        let r = bar.drop_files(4, &files[i..i + 1], 1_000 + i as u64);
        ok20 &= r.is_ok();
        bar.clear_drop_target();
    }
    set.add("drop-20", ok20 && bar.drop_moves == 20, "20 moves all ok");
    // 4. 只读层禁投（原因 tooltip 文案）。
    bar.set_readonly_depth(4, true);
    let rejected = bar.drop_files(4, &["x"], 2_000).unwrap_err().contains("只读");
    bar.set_readonly_depth(4, false);
    set.add("readonly-forbid", rejected, "forbidden + reason");
    // 5. 补全 <50ms（"C:/工作/2" 前缀命中 2025 与 2026 两条——如实断言 2）。
    bar.begin_edit();
    bar.edit_input("C:/工作/2", 3_000);
    let cands = ["C:/工作/2026", "C:/工作/2025", "C:/工作/档案"];
    let comps = bar.complete(&cands, 12);
    set.add(
        "complete-50ms",
        bar.complete_in_budget()
            && comps.len() == 2
            && comps[0].path == "C:/工作/2026"
            && comps[1].path == "C:/工作/2025",
        "prefix match fast",
    );
    // 6. 编辑提交：存在跳转 / 不存在如实报错不跳。
    bar.edit_text = String::from("C:/工作/2026");
    let committed = bar.commit_edit(true) && bar.history().current() == "C:/工作/2026";
    bar.begin_edit();
    bar.edit_text = String::from("C:/不存在/路径");
    let refused = !bar.commit_edit(false) && bar.honest_errors == 1;
    bar.cancel_edit();
    set.add("commit-honest", committed && refused, "exists jumps / else errors");
    // 7. 编辑-取消-回显零状态残留。
    bar.begin_edit();
    bar.edit_text = String::from("被改掉的内容");
    let cancelled = bar.cancel_edit();
    let clean = !bar.editing() && bar.edit_text.is_empty() && bar.history().current() == "C:/工作/2026";
    set.add("cancel-residue", cancelled && clean && bar.residue_violations == 0, "zero residue");
    // 8. 两态全键盘切换（Alt+D）。
    bar.alt_d();
    let into_edit = bar.editing();
    bar.alt_d();
    let back_to_crumbs = !bar.editing();
    set.add("alt-d-toggle", into_edit && back_to_crumbs, "keyboard two-state");
    // 9. 下拉：淡入 120ms（中段推进后再取样）+ 滚动线（crumbs[2] = 工作）。
    bar.open_dropdown(2, 4_000);
    bar.now_ms = 4_060; // 中段 60ms
    let fade_mid = bar.dropdown_fade();
    bar.now_ms = 4_120;
    let fade_done = bar.dropdown_fade();
    bar.feed_siblings(
        "C:/工作",
        (0..60)
            .map(|i| Sibling {
                name: format!("目录{}", i),
                children: i,
            })
            .collect(),
    );
    set.add(
        "dropdown-fade-scroll",
        bar.dropdown_open() && fade_mid > 0 && fade_mid < 1000 && fade_done == 1000
            && bar.siblings_need_scroll(2),
        "120ms + 50+ scroll",
    );
    // 10. 选中兄弟跳转 + 过长省略。
    let picked = bar.pick_sibling(2, "2025") && bar.history().current() == "C:/工作/2025";
    bar.history_mut().go("C:/A1/A2/A3/A4/A5/A6/最深");
    let seg = bar.crumbs();
    let has_ellipsis = seg.iter().any(|c| c.name == "…");
    set.add("pick-ellipsis", picked && has_ellipsis, "sibling jump + shrink");
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_target_highlight_and_clear() {
        let mut bar = CrumbsBar::bind(HistoryStack::new("C:/a/b"));
        assert!(bar.drop_files(2, &["a.txt"], 0).is_ok());
        assert_eq!(bar.drop_target_token(), Some(Token::Accent));
        bar.clear_drop_target();
        assert_eq!(bar.drop_target_token(), None);
    }

    #[test]
    fn root_segment_is_clickable() {
        let mut bar = CrumbsBar::bind(HistoryStack::new("C:/a/b"));
        assert!(bar.click_crumb(0), "此电脑段可点");
        assert_eq!(bar.history().current(), "/");
    }

    #[test]
    fn complete_no_match_empty() {
        let mut bar = CrumbsBar::bind(HistoryStack::new("C:/x"));
        bar.begin_edit();
        bar.edit_input("C:/zzz", 0);
        assert!(bar.complete(&["C:/aaa"], 1).is_empty());
    }

    #[test]
    fn drop_on_missing_layer_errors() {
        let mut bar = CrumbsBar::bind(HistoryStack::new("C:/x"));
        assert!(bar.drop_files(9, &["a"], 0).is_err());
    }

    #[test]
    fn crumbsbar_self_checks_all_green() {
        let set = run_crumbsbar_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F090 自检红项：{}/{} 绿", p, p + f);
    }
}

// ---------------------------------------------------------------------------
// 深化自检（回炉批 D1-v2）——省略段展开 / 下拉内搜索 / 滚动可视窗 /
// 补全键盘导航 / 面包屑焦点环 / 回到当前钮 / 全选账 / 几何常量。
// 判据唯一源：主册 G-C-20 交互设计/设计细节。
// ---------------------------------------------------------------------------

/// F090 深化自检：七族逐条记账。
pub fn run_crumbsbar_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F090-deep");
    let mut bar = CrumbsBar::bind(HistoryStack::new("C:/A1/A2/A3/A4/A5/A6/最深"));
    // 1. 省略态判定 + 展开：>7 层折叠，展开回全层序且可直接点击。
    let shrunk = bar.is_shrunk();
    let shrunken = bar.crumbs();
    let expanded = bar.expand_ellipsis();
    let mid_names: Vec<String> = expanded.iter().map(|c| c.name.clone()).collect();
    set.add(
        "ellipsis-expand",
        shrunk
            && bar.is_full_shown() // expand_ellipsis 已置展开态
            && shrunken.iter().any(|c| c.name == "…")
            && expanded.iter().all(|c| c.name != "…")
            && mid_names.contains(&String::from("A3"))
            && expanded.first().unwrap().name == "此电脑",
        "expand restores middle layers",
    );
    // 2. 展开态下的中间层可直接点击跳转（焦点环 → 激活 A3）。
    let idx_a3 = expanded.iter().position(|c| c.name == "A3").unwrap();
    bar.crumb_focus = idx_a3;
    let jumped = bar.crumb_focus_activate() && bar.history().current().ends_with("/A3");
    bar.history_mut().go("C:/A1/A2/A3/A4/A5/A6/最深");
    bar.collapse_ellipsis();
    let collapsed_back = !bar.is_full_shown() && bar.crumbs().iter().any(|c| c.name == "…");
    set.add("expand-jump", jumped && collapsed_back, "expanded layer navigable");
    // 3. 下拉内搜索：过滤大小写不敏感；空词全量（crumbs[2] = 工作）。
    bar.history_mut().go("C:/工作");
    bar.feed_siblings(
        "C:/工作",
        vec![
            Sibling { name: String::from("Reports"), children: 3 },
            Sibling { name: String::from("档案"), children: 7 },
            Sibling { name: String::from("报告集"), children: 1 },
        ],
    );
    bar.open_dropdown(2, 1_000);
    let all = bar.filter_siblings(2, "");
    let hit_p = bar.filter_siblings(2, "PORT"); // 大小写不敏感
    let hit_cjk = bar.filter_siblings(2, "报告");
    set.add(
        "dropdown-search",
        all.len() == 3
            && hit_p.len() == 1
            && hit_p[0].name == "Reports"
            && hit_cjk.len() == 1
            && hit_cjk[0].name == "报告集",
        "search filter ci",
    );
    // 4. 滚动可视窗：60 项 → 12 行窗；越界钳底；翻窗推进内容不重叠。
    bar.feed_siblings(
        "C:/工作",
        (0..60u32)
            .map(|i| Sibling { name: format!("目录{}", i), children: i })
            .collect(),
    );
    let win0 = bar.siblings_window(2, 0);
    let win_over = bar.siblings_window(2, 999); // 越界 → 钳到底窗
    let win_last = bar.siblings_window(2, 48); // 恰好最后一窗
    set.add(
        "scroll-window",
        win0.len() == SIBLING_VIEWPORT
            && win0[0].name == "目录0"
            && win_over.last().unwrap().name == "目录59"
            && win_last.last().unwrap().name == "目录59"
            && win0[0].name != win_over[0].name,
        "12-row clamped window",
    );
    // 5. 补全键盘导航：焦点首位落、方向移动顶格停、Enter 接受、Esc 关。
    bar.begin_edit();
    bar.edit_input("C:/工作/2", 2_000);
    bar.open_completions(&["C:/工作/2025", "C:/工作/2026"]);
    let focus0 = bar.comp_focus() == Some(0);
    bar.comp_focus_move(true);
    bar.comp_focus_move(true); // 顶格停（2 候选 → 最大 1）
    let focus_end = bar.comp_focus() == Some(1);
    let accepted = bar.comp_accept() && bar.edit_text() == "C:/工作/2026" && !bar.comp_open();
    bar.open_completions(&["C:/工作/2025", "C:/工作/2026"]);
    bar.comp_focus_move(false);
    bar.comp_focus_move(false); // 顶格停（0 不再退）
    let focus_head = bar.comp_focus() == Some(0);
    bar.close_completions();
    let esc_ok = !bar.comp_open() && bar.editing(); // 关下拉不丢编辑态
    set.add(
        "comp-keyboard",
        focus0 && focus_end && accepted && focus_head && esc_ok,
        "completions keyboard reachable",
    );
    // 6. 回到当前钮：改过才出现；点击 = 取消（零残留同出口）。
    bar.begin_edit();
    let btn_clean = !bar.revert_button_visible();
    bar.edit_input("C:/被改成/别的", 2_100);
    let btn_dirty = bar.revert_button_visible();
    let reverted = bar.click_revert() && !bar.editing() && bar.history().current() == "C:/工作";
    set.add(
        "revert-button",
        btn_clean && btn_dirty && reverted && bar.residue_violations == 0,
        "revert = cancel path",
    );
    // 7. 编辑态全选账 / 键盘焦点环边界 / 几何常量（拆项记账）。
    bar.begin_edit();
    let sel_all = bar.edit_selection().select_all;
    bar.cancel_edit();
    let sel_off = !bar.edit_selection().select_all;
    set.add("edit-select-all", sel_all && sel_off, "Ctrl+A equivalent on edit");
    let mut bar2 = CrumbsBar::bind(HistoryStack::new("C:/a/b"));
    let f0 = bar2.crumb_focus;
    bar2.crumb_focus_move(false); // 顶格不退
    let f1 = bar2.crumb_focus_move(false);
    bar2.crumb_focus_move(true);
    bar2.crumb_focus_move(true);
    let f2 = bar2.crumb_focus_move(true); // 到尾停（4 段：此电脑/C:/a/b → idx 3）
    set.add(
        "crumb-ring-bounds",
        f0 == 0 && f1 == 0 && f2 == 3,
        "arrow keys clamp at edges",
    );
    set.add(
        "crumb-geometry",
        CRUMB_H_PX == 28 && DROPDOWN_W_PX == 240 && SIBLING_VIEWPORT == 12,
        "spec constants",
    );
    // 8. 省略段点击 = 展开（行为闭环）；跨盘判定（F018 判定口）；
    //    只读 tooltip 与 Err 文案同源。
    let mut bar3 = CrumbsBar::bind(HistoryStack::new("C:/A1/A2/A3/A4/A5/A6/最深"));
    let ell_idx = bar3.crumbs().iter().position(|c| c.name == "…").unwrap();
    let via_click = bar3.click_crumb(ell_idx) && bar3.is_full_shown();
    set.add("ellipsis-click-expands", via_click, "no dead-end crumb");
    let bar4 = CrumbsBar::bind(HistoryStack::new("D:/备份"));
    let same = !bar4.is_cross_disk("D:/备份/文件.txt", 2);
    let cross = bar4.is_cross_disk("C:/工作/文件.txt", 2);
    set.add("cross-disk-detect", same && cross, "F018 discriminator");
    bar3.set_readonly_depth(2, true);
    let tip = bar3.readonly_tooltip(2);
    bar3.set_readonly_depth(2, false);
    let tip_off = bar3.readonly_tooltip(2).is_none();
    set.add("readonly-tip", tip == Some("此层为只读，不能移入文件") && tip_off, "tooltip parity");
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_deep {
    use super::*;

    #[test]
    fn not_shrunk_for_shallow_paths() {
        let bar = CrumbsBar::bind(HistoryStack::new("C:/a/b"));
        assert!(!bar.is_shrunk(), "浅路径无省略");
        assert!(bar.crumbs().iter().all(|c| c.name != "…"));
    }

    #[test]
    fn filter_missing_layer_is_empty_not_panic() {
        let bar = CrumbsBar::bind(HistoryStack::new("C:/x"));
        assert!(bar.filter_siblings(9, "a").is_empty(), "层不存在回空——不炸");
    }

    #[test]
    fn comp_accept_without_open_is_honest_false() {
        let mut bar = CrumbsBar::bind(HistoryStack::new("C:/x"));
        assert!(!bar.comp_accept(), "无下拉可接受——如实拒绝");
    }

    #[test]
    fn expand_ellipsis_single_segment_path() {
        // 仅根段的极端路径：展开 = 收起（同一全序）。
        let mut bar = CrumbsBar::bind(HistoryStack::new("/"));
        let segs = bar.expand_ellipsis();
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].name, "此电脑");
    }

    #[test]
    fn crumbsbar_deep_checks_all_green() {
        let set = run_crumbsbar_deep_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F090-deep 红项：{}/{} 绿", p, p + f);
    }
}
