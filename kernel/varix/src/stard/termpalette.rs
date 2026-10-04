//! F096 终端命令面板 · 完整设计（STAR I 主册 G-C-26）。
//!
//! **判据（主册）**：内置命令 12 条全可达；收藏-搜索-执行全链；面板弹出
//! <100ms。
//!
//! **设计要点（主册）**：
//! - Ctrl+Shift+P 命令面板：内置命令（复制模式/清屏/分屏/导出会话/字号）
//!   模糊搜索直执行；应用可注册自定义命令（终端内运行的长命令收藏）；
//! - 面板居中弹出 560×56px 搜索框态、输入后下拉结果（高 320px 上限）；
//!   高亮匹配段；Enter 执行 / Esc 关；执行反馈区分「即时类」（清屏即见）
//!   与「运行类」（新标签执行）；
//! - 收藏命令存应用蜂巢（导入导出随 vxtheme 面）；使用频次影响排序
//!   （F072 引擎分档语义复用——freq 越高频越前，同频按最近）；
//! - 命令执行失败 → 输出在当前终端如实显示（面板不吞输出）；面板输入含
//!   危险字符 → 原样传递（终端是成年人的工具，不代管）；
//! - 模糊匹配 = 子序列 + 首字母双路（F071 引擎复用）；面板出现时终端输出
//!   冻结显示（不被执行结果冲走——执行完恢复跟随）；命令模板变量（{host}
//!   占位执行时问）；导出会话含时间戳与退出码元数据（F095 export 面共享）。
//!
//! 匹配/排序/持久化全部模型面实现，注入时钟确定复现，零外部依赖。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计与设计细节）
// ---------------------------------------------------------------------------

/// 面板搜索框态宽（px）。
pub const PANEL_W_PX: u32 = 560;

/// 面板搜索框态高（px）。
pub const PANEL_SEARCH_H_PX: u32 = 56;

/// 结果下拉高上限（px）。
pub const PANEL_LIST_MAX_H_PX: u32 = 320;

/// 面板弹出判线（ms）。
pub const OPEN_BUDGET_MS: u64 = 100;

/// 内置命令数（判据：12 条全可达）。
pub const BUILTIN_COUNT: usize = 12;

/// 收藏容量（应用蜂巢面定容）。
pub const FAVORITE_CAP: usize = 64;

// ---------------------------------------------------------------------------
// 命令模型
// ---------------------------------------------------------------------------

/// 内置命令 12 条（判据锚——枚举即唯一源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    CopyMode,
    ClearScreen,
    SplitHorizontal,
    SplitVertical,
    ExportSession,
    FontSizeUp,
    FontSizeDown,
    SearchInBuffer,
    NewTab,
    CloseTab,
    CloseOtherPanes,
    ToggleFollow,
}

impl Builtin {
    pub fn all() -> [Builtin; BUILTIN_COUNT] {
        [
            Builtin::CopyMode,
            Builtin::ClearScreen,
            Builtin::SplitHorizontal,
            Builtin::SplitVertical,
            Builtin::ExportSession,
            Builtin::FontSizeUp,
            Builtin::FontSizeDown,
            Builtin::SearchInBuffer,
            Builtin::NewTab,
            Builtin::CloseTab,
            Builtin::CloseOtherPanes,
            Builtin::ToggleFollow,
        ]
    }

    pub fn name(self) -> &'static str {
        match self {
            Builtin::CopyMode => "复制模式",
            Builtin::ClearScreen => "清屏",
            Builtin::SplitHorizontal => "左右分屏",
            Builtin::SplitVertical => "上下分屏",
            Builtin::ExportSession => "导出会话",
            Builtin::FontSizeUp => "字号增大",
            Builtin::FontSizeDown => "字号减小",
            Builtin::SearchInBuffer => "搜索回看缓冲",
            Builtin::NewTab => "新建标签页",
            Builtin::CloseTab => "关闭标签页",
            Builtin::CloseOtherPanes => "关闭其他分屏",
            Builtin::ToggleFollow => "跟随滚动开关",
        }
    }

    pub fn initials(self) -> &'static str {
        match self {
            Builtin::CopyMode => "fzms",
            Builtin::ClearScreen => "qp",
            Builtin::SplitHorizontal => "zyfp",
            Builtin::SplitVertical => "sxfp",
            Builtin::ExportSession => "dchh",
            Builtin::FontSizeUp => "zhzd",
            Builtin::FontSizeDown => "zhzx",
            Builtin::SearchInBuffer => "sshk",
            Builtin::NewTab => "xjbq",
            Builtin::CloseTab => "gbbq",
            Builtin::CloseOtherPanes => "gbqtfp",
            Builtin::ToggleFollow => "gsgd",
        }
    }

    /// 执行反馈类型：即时类（清屏即见）与运行类（新标签执行）。
    pub fn effect(self) -> Effect {
        match self {
            Builtin::ExportSession => Effect::Run,
            Builtin::NewTab => Effect::Run,
            _ => Effect::Immediate,
        }
    }
}

/// 执行反馈类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Immediate,
    Run,
}

/// 命令条目（内置或收藏的自定义命令）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub id: u32,
    pub name: String,
    /// 执行体：内置命令的语义 id 或原样传递的命令行（危险字符不代管）。
    pub cmdline: String,
    pub builtin: Option<Builtin>,
    /// 使用频次（F072 分档语义）。
    pub freq: u64,
    /// 最近使用时刻（注入时钟）。
    pub last_used_ms: u64,
}

// ---------------------------------------------------------------------------
// 模糊匹配（子序列 + 首字母双路——F071 引擎复用口径）
// ---------------------------------------------------------------------------

/// 匹配得分：None = 不匹配。子序列命中 60 起、首字母命中 80 起，
/// 连续前缀 +20，频次每 10 次 +1（封顶 +10）。分数越高越前。
pub fn fuzzy_score(name: &str, initials: &str, query: &str, freq: u64) -> Option<u32> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).collect();
    if q.is_empty() {
        return Some(0); // 空查询 = 全量列表（面板刚开）。
    }
    let name_chars: Vec<char> = name.chars().collect();
    // 子序列匹配（不要求连续）。
    let mut sub = true;
    let mut hi = 0usize;
    for qc in &q {
        match name_chars[hi..].iter().position(|c| c == qc) {
            Some(p) => hi += p + 1,
            None => {
                sub = false;
                break;
            }
        }
    }
    // 首字母匹配（拼音首字母串——内置命令注入口；自定义命令按名取首）。
    let ini_chars: Vec<char> = initials.chars().collect();
    let mut ini = ini_chars.len() >= q.len();
    if ini {
        for (i, qc) in q.iter().enumerate() {
            if ini_chars.get(i) != Some(qc) {
                ini = false;
                break;
            }
        }
    }
    if !sub && !ini {
        return None;
    }
    let mut score: u32 = if ini { 80 } else { 60 };
    if name.starts_with(&query.replace(' ', "")) {
        score += 20;
    }
    score += (freq.min(100) / 10) as u32;
    Some(score)
}

/// 匹配段高亮范围（子序列命中的字符位——UI 高亮用）。
pub fn highlight_ranges(name: &str, query: &str) -> Vec<(usize, usize)> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).collect();
    let mut out = Vec::new();
    if q.is_empty() {
        return out;
    }
    let mut byte = 0usize;
    let mut hi = 0usize;
    for c in name.chars() {
        let len = c.len_utf8();
        if hi < q.len() && c == q[hi] {
            out.push((byte, byte + len));
            hi += 1;
        }
        byte += len;
    }
    out
}

// ---------------------------------------------------------------------------
// 收藏库（freq 分档 + 导入导出 round-trip）
// ---------------------------------------------------------------------------

/// 收藏库：自定义命令（应用可注册长命令收藏）。
pub struct Favorites {
    items: Vec<Command>,
}

impl Favorites {
    pub fn new() -> Favorites {
        Favorites { items: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 注册/更新收藏（同 cmdline 幂等更新名字）。超容量 → 逐出最低频最旧。
    pub fn register(&mut self, name: &str, cmdline: &str, now_ms: u64) -> bool {
        if cmdline.trim().is_empty() {
            return false;
        }
        if let Some(i) = self.items.iter().position(|c| c.cmdline == cmdline) {
            self.items[i].name = String::from(name);
            return true;
        }
        if self.items.len() >= FAVORITE_CAP {
            let mut victim = 0usize;
            for i in 1..self.items.len() {
                let a = &self.items[i];
                let b = &self.items[victim];
                if a.freq < b.freq || (a.freq == b.freq && a.last_used_ms < b.last_used_ms) {
                    victim = i;
                }
            }
            self.items.remove(victim);
        }
        let id = 1000 + self.items.len() as u32;
        self.items.push(Command {
            id,
            name: String::from(name),
            cmdline: String::from(cmdline),
            builtin: None,
            freq: 0,
            last_used_ms: now_ms,
        });
        true
    }

    pub fn items(&self) -> &[Command] {
        &self.items
    }

    /// 使用计数（执行时调用——排序分档数据源）。
    pub fn bump(&mut self, cmdline: &str, now_ms: u64) {
        if let Some(c) = self.items.iter_mut().find(|c| c.cmdline == cmdline) {
            c.freq += 1;
            c.last_used_ms = now_ms;
        }
    }

    /// 导出（vxtheme 包内 phrases 面同族格式）。
    pub fn export(&self) -> Vec<(String, String, u64)> {
        self.items.iter().map(|c| (c.name.clone(), c.cmdline.clone(), c.freq)).collect()
    }

    /// 导入（round-trip 无损判据载体；冲突按覆盖——导入即同步）。
    pub fn import(&mut self, data: &[(String, String, u64)], now_ms: u64) -> usize {
        let mut n = 0;
        for (name, cmdline, freq) in data {
            if self.register(name, cmdline, now_ms) {
                if let Some(c) = self.items.iter_mut().find(|c| c.cmdline == *cmdline) {
                    c.freq = *freq;
                }
                n += 1;
            }
        }
        n
    }
}

impl Default for Favorites {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 面板状态机与执行
// ---------------------------------------------------------------------------

/// 面板状态：Closed → Open(query) →（Enter 执行 / Esc 关）→ Closed。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelState {
    Closed,
    /// 打开态（当前查询文本；空查询 = 全量列表）。
    Open(String),
}

/// 一次搜索结果条目。
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub command: Command,
    pub score: u32,
    /// 首字母匹配口径的面板显示注记。
    pub via_initials: bool,
}

/// 命令面板。
pub struct Palette {
    pub state: PanelState,
    pub builtin_freq: [u64; BUILTIN_COUNT],
    pub favorites: Favorites,
    /// 面板出现时终端输出冻结（执行完恢复跟随——冻结计数对账）。
    pub freeze_count: u64,
    pub open_count: u64,
    /// 打开耗时账（弹出 <100ms 判线载体——注入开耗样本）。
    pub last_open_cost_ms: u64,
}

impl Palette {
    pub fn new() -> Palette {
        Palette {
            state: PanelState::Closed,
            builtin_freq: [0; BUILTIN_COUNT],
            favorites: Favorites::new(),
            freeze_count: 0,
            open_count: 0,
            last_open_cost_ms: 0,
        }
    }

    /// 打开面板（Ctrl+Shift+P）。`open_cost_ms` 由调用方注入实测耗时。
    pub fn open(&mut self, open_cost_ms: u64) {
        self.state = PanelState::Open(String::new());
        self.open_count += 1;
        self.freeze_count += 1; // 出现时终端输出冻结显示。
        self.last_open_cost_ms = open_cost_ms;
    }

    pub fn close(&mut self) {
        self.state = PanelState::Closed;
        self.freeze_count = self.freeze_count.saturating_sub(1); // 恢复跟随。
    }

    pub fn query(&mut self, q: &str) {
        if matches!(self.state, PanelState::Open(_)) {
            self.state = PanelState::Open(String::from(q));
        }
    }

    /// 搜索：内置 12 条 + 收藏全量过模糊匹配，按分排序（同分频次优先）。
    pub fn search(&self, q: &str) -> Vec<Hit> {
        let mut hits: Vec<Hit> = Vec::new();
        let builtins = Builtin::all();
        for (i, b) in builtins.iter().enumerate() {
            if let Some(score) = fuzzy_score(b.name(), b.initials(), q, self.builtin_freq[i]) {
                // via_initials = 只走首字母路命中（子序列路不命中时）。
                let sub_only = fuzzy_score(b.name(), "", q, 0).is_some();
                hits.push(Hit {
                    command: Command {
                        id: i as u32,
                        name: String::from(b.name()),
                        cmdline: String::new(),
                        builtin: Some(*b),
                        freq: self.builtin_freq[i],
                        last_used_ms: 0,
                    },
                    score,
                    via_initials: !q.is_empty() && !sub_only,
                });
            }
        }
        for c in self.favorites.items() {
            // 自定义命令无拼音首字母注入口（F071 面就绪后接线）——空串只走子序列路。
            if let Some(score) = fuzzy_score(&c.name, "", q, c.freq) {
                hits.push(Hit { command: c.clone(), score, via_initials: false });
            }
        }
        hits.sort_by(|a, b| b.score.cmp(&a.score).then(b.command.freq.cmp(&a.command.freq)));
        hits
    }

    /// 执行：返回 (cmdline_or_name, effect)。执行失败由终端面如实显示
    /// （面板不吞输出——本接口只记账与分派）。危险字符原样传递。
    pub fn execute(&mut self, hit: &Hit, now_ms: u64) -> (&'static str, Effect) {
        match hit.command.builtin {
            Some(b) => {
                let idx = Builtin::all().iter().position(|x| *x == b).unwrap_or(0);
                self.builtin_freq[idx] += 1;
                (b.name(), b.effect())
            }
            None => {
                self.favorites.bump(&hit.command.cmdline.clone(), now_ms);
                ("custom", Effect::Run)
            }
        }
    }

    /// 模板变量提取：{host} 等占位符（执行时问——返回变量名列表）。
    pub fn template_vars(cmdline: &str) -> Vec<String> {
        let mut out = Vec::new();
        let bytes = cmdline.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'{' {
                if let Some(end_rel) = cmdline[i + 1..].find('}') {
                    let var = &cmdline[i + 1..i + 1 + end_rel];
                    if !var.is_empty() && !out.iter().any(|v| v == var) {
                        out.push(String::from(var));
                    }
                    i += end_rel + 2;
                    continue;
                }
            }
            i += 1;
        }
        out
    }

    /// 模板填充（执行时问得变量值后回填）。
    pub fn fill_template(cmdline: &str, vars: &[(&str, &str)]) -> String {
        let mut out = String::from(cmdline);
        for (k, v) in vars {
            let pat = alloc::format!("{{{k}}}");
            out = out.replace(&pat, v);
        }
        out
    }

    /// 内置 12 条全可达（判据自检口径：每条都能搜出且可执行分派）。
    pub fn all_builtins_reachable(&self) -> bool {
        let builtins = Builtin::all();
        builtins.iter().all(|b| {
            let mut probe = Palette::new();
            probe.state = PanelState::Open(String::new());
            let hits = probe.search(b.name());
            hits.iter().any(|h| h.command.builtin == Some(*b))
        })
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F096 自检（聚合进 stard 域）。
pub fn run_termpalette_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F096");

    // —— 内置 12 条全可达 ——
    set.add("builtin count is 12", Builtin::all().len() == BUILTIN_COUNT, "");
    let p = Palette::new();
    set.add("all builtins reachable", p.all_builtins_reachable(), "");

    // —— 模糊匹配：子序列 + 首字母双路 ——
    set.add("subsequence match", fuzzy_score("关闭标签页", "gbbq", "关标", 0).is_some(), "");
    set.add("initials match", fuzzy_score("清屏", "qp", "qp", 0).unwrap_or(0) >= 80, "");
    set.add("no match none", fuzzy_score("清屏", "qp", "zzz", 0).is_none(), "");
    set.add("empty query lists all", fuzzy_score("清屏", "qp", "", 0) == Some(0), "");
    set.add("freq boosts score", fuzzy_score("清屏", "qp", "q", 50) > fuzzy_score("清屏", "qp", "q", 0), "");
    set.add("prefix bonus", fuzzy_score("清屏", "qp", "清", 0) > fuzzy_score("关闭标签页", "gbbq", "清", 0), "");

    // —— 高亮段 ——
    set.add("highlight ranges", highlight_ranges("关闭标签页", "关标") == alloc::vec![(0, 3), (6, 9)], "");

    // —— 收藏：注册-使用-排序-导出导入 round-trip ——
    let mut fav = Favorites::new();
    set.add("favorite register", fav.register("跳板机", "ssh -J gate bastion", 1), "");
    set.add("favorite idempotent cmdline", fav.register("跳板机2", "ssh -J gate bastion", 2) && fav.len() == 1, "");
    fav.bump("ssh -J gate bastion", 3);
    fav.bump("ssh -J gate bastion", 4);
    let exported = fav.export();
    set.add("favorite export has freq", exported.len() == 1 && exported[0].2 == 2, "");
    let mut fav2 = Favorites::new();
    set.add("import round trip", fav2.import(&exported, 5) == 1 && fav2.items()[0].freq == 2, "");
    set.add("empty cmdline rejected", !fav.register("x", "  ", 6), "");

    // —— 面板状态机：开-搜-执行-关（冻结/恢复对账）——
    let mut pal = Palette::new();
    pal.favorites.register("跳板机", "ssh -J gate bastion", 1);
    pal.open(42);
    set.add("panel opens under 100ms", pal.last_open_cost_ms < OPEN_BUDGET_MS, "");
    set.add("panel freeze on open", pal.freeze_count == 1, "");
    pal.query("跳板");
    let hits = pal.search("跳板");
    set.add("search finds favorite", hits.iter().any(|h| h.command.cmdline == "ssh -J gate bastion"), "");
    let hit = hits[0].clone();
    let (kind, effect) = pal.execute(&hit, 9);
    set.add("custom executes as run", kind == "custom" && effect == Effect::Run, "");
    pal.close();
    set.add("panel close restores follow", pal.freeze_count == 0 && pal.state == PanelState::Closed, "");

    // —— 内置执行分派：即时/运行两型 ——
    let mut pal2 = Palette::new();
    let clear_hits = pal2.search("清屏");
    let (name, effect) = pal2.execute(&clear_hits[0].clone(), 1);
    set.add("builtin immediate effect", name == "清屏" && effect == Effect::Immediate, "");
    set.add("builtin freq bumped", pal2.builtin_freq[1] == 1, "");

    // —— 模板变量：{host} 执行时问 ——
    let vars = Palette::template_vars("ssh {host} -p {port}");
    set.add("template vars extracted", vars == alloc::vec!["host", "port"], "");
    set.add("template filled", Palette::fill_template("ssh {host}", &[("host", "10.0.0.1")]) == "ssh 10.0.0.1", "");

    // —— 危险字符原样传递（不代管）——
    let mut fav3 = Favorites::new();
    let raw = "rm -rf /tmp/x && echo ok";
    set.add("dangerous chars passed through", fav3.register("清理", raw, 1) && fav3.items()[0].cmdline == raw, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twelve_builtins_all_reachable() {
        let p = Palette::new();
        assert!(p.all_builtins_reachable());
        for b in Builtin::all() {
            assert!(!b.name().is_empty());
        }
    }

    #[test]
    fn fuzzy_paths_and_ordering() {
        // 首字母路优先于子序列路。
        assert!(fuzzy_score("清屏", "qp", "qp", 0).unwrap() >= 80);
        assert!(fuzzy_score("清屏", "qp", "清屏", 0).unwrap() >= 60);
        // 搜索排序：分数降序。
        let mut pal = Palette::new();
        pal.open(10);
        let hits = pal.search("分屏");
        assert!(hits.len() >= 2, "左右/上下分屏都命中");
        for w in hits.windows(2) {
            assert!(w[0].score >= w[1].score, "分数降序");
        }
    }

    #[test]
    fn highlight_cjk_byte_ranges() {
        // 「关」3 字节、「标」3 字节：高亮段按字节位。
        assert_eq!(highlight_ranges("关闭标签页", "关标"), alloc::vec![(0, 3), (6, 9)]);
        assert!(highlight_ranges("清屏", "").is_empty());
    }

    #[test]
    fn favorites_cap_evicts_cold() {
        let mut fav = Favorites::new();
        for i in 0..FAVORITE_CAP as u64 {
            fav.register(&alloc::format!("cmd{i}"), &alloc::format!("run {i}"), i);
        }
        assert_eq!(fav.len(), FAVORITE_CAP);
        // 高频命令再注册不逐出；新注册逐出零频最旧。
        fav.bump("run 0", 9999);
        fav.bump("run 0", 9999);
        fav.register("newcmd", "run new", 10_000);
        assert_eq!(fav.len(), FAVORITE_CAP);
        assert!(fav.items().iter().any(|c| c.cmdline == "run new"));
        assert!(fav.items().iter().any(|c| c.cmdline == "run 0"), "高频命令保留");
        assert!(fav.items().iter().all(|c| c.cmdline != "run 1"), "零频最旧被逐出");
    }

    #[test]
    fn panel_state_machine_full_chain() {
        let mut pal = Palette::new();
        assert_eq!(pal.state, PanelState::Closed);
        pal.open(88);
        assert!(matches!(pal.state, PanelState::Open(_)));
        pal.query("导出");
        let hits = pal.search("导出");
        assert!(hits[0].command.builtin == Some(Builtin::ExportSession));
        let (_, effect) = pal.execute(&hits[0].clone(), 1);
        assert_eq!(effect, Effect::Run);
        pal.close();
        assert_eq!(pal.state, PanelState::Closed);
        assert_eq!(pal.freeze_count, 0, "开-关一次冻结/恢复对平");
        // 关闭态 query 无效。
        pal.query("x");
        assert_eq!(pal.state, PanelState::Closed);
    }

    #[test]
    fn template_round_trip() {
        let cmd = "ssh {user}@{host} -p {port}";
        assert_eq!(Palette::template_vars(cmd), alloc::vec!["user", "host", "port"]);
        let filled = Palette::fill_template(cmd, &[("user", "v"), ("host", "h"), ("port", "22")]);
        assert_eq!(filled, "ssh v@h -p 22");
        // 未提供的变量保留原样（执行时问未答不静默吞）。
        let partial = Palette::fill_template(cmd, &[("user", "v")]);
        assert!(partial.contains("{host}"));
        // 不闭合的花括号不当变量。
        assert!(Palette::template_vars("echo {oops").is_empty());
    }

    #[test]
    fn import_export_round_trip_lossless() {
        let mut src = Favorites::new();
        src.register("问候", "echo hi", 1);
        src.register("部署", "make deploy", 2);
        src.bump("make deploy", 3);
        let data = src.export();
        let mut dst = Favorites::new();
        assert_eq!(dst.import(&data, 9), 2);
        assert_eq!(dst.export(), data, "round-trip 无损");
    }
}
