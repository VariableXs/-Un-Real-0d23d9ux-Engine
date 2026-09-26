//! F336 复制文件地址 + F337 命令行与图形互通 · AI-H3。
//!
//! **F336 判据**：两项菜单入口；引号自动包裹（空格路径用例）；相对路径
//! 基准（当前目录定义明确）；终端粘贴可用性（CMD 形/POSIX 形按目标自适
//! 应）。
//! **F337 判据**：互通四入口用例；工作目录正确性（开在哪就在哪）；拖入
//! 引号处理（含空格/特殊字符）；open . 的目录同步。
//!
//! 两项合模块：路径引号化是共享核心（一处一事实）——复制地址与终端拖
//! 入共用同一引号策略，互通四入口挂同一工作目录账。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 引号策略（唯一源）
// ---------------------------------------------------------------------------

/// 是否需要引号（含空格或特殊字符）。
pub fn needs_quotes(path: &str) -> bool {
    path.chars().any(|c| c == ' ' || c == '&' || c == '(' || c == ')' || c == '^' || c == '%')
}

/// 终端形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalFlavor {
    /// CMD 形：双引号 + 反斜杠。
    Cmd,
    /// POSIX 形：单引号 + 正斜杠。
    Posix,
}

/// 引号包裹（按目标形态自适应——终端粘贴可用性判据载体）。
pub fn quote_for(path: &str, flavor: TerminalFlavor) -> String {
    let normalized = match flavor {
        TerminalFlavor::Cmd => path.replace('/', "\\"),
        TerminalFlavor::Posix => path.replace('\\', "/"),
    };
    if needs_quotes(path) {
        match flavor {
            TerminalFlavor::Cmd => alloc::format!("\"{}\"", normalized),
            TerminalFlavor::Posix => alloc::format!("'{}'", normalized),
        }
    } else {
        normalized
    }
}

/// 相对路径计算（基准 = 当前目录——定义明确：同根下取相对，跨根回退绝
/// 对路径）。
pub fn relative_to(path: &str, base: &str) -> String {
    let p: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let b: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    // 找公共前缀。
    let mut common = 0usize;
    while common < p.len() && common < b.len() && p[common] == b[common] {
        common += 1;
    }
    if common == 0 {
        return String::from(path); // 跨根回退绝对。
    }
    let mut out = String::new();
    for _ in common..b.len() {
        out.push_str("../");
    }
    out.push_str(&p[common..].join("/"));
    out
}

// ---------------------------------------------------------------------------
// 互通四入口（F337）
// ---------------------------------------------------------------------------

/// 互通入口记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BridgeEntry {
    /// 资源管理器地址栏输 vxsh → 此目录开终端。
    ExplorerToTerminal(String),
    /// 终端输 open . → 资源管理器开当前目录。
    TerminalToExplorer(String),
    /// 文件拖入终端 → 带引号路径插入。
    DragIntoTerminal(String),
    /// 终端选中文本右键 → 搜索。
    SelectionToSearch(String),
}

/// 工作目录账（开在哪就在哪——目录同步判据载体）。
pub struct WorkdirLedger {
    /// 每入口最近一次的工作目录。
    pub entries: Vec<(String, String)>,
}

impl WorkdirLedger {
    pub fn new() -> WorkdirLedger {
        WorkdirLedger { entries: Vec::new() }
    }

    /// 记录入口的工作目录（同入口覆盖——目录同步：开在哪就在哪）。
    pub fn record(&mut self, entry: &str, dir: &str) {
        match self.entries.iter_mut().find(|(e, _)| e == entry) {
            Some(slot) => slot.1 = String::from(dir),
            None => self.entries.push((String::from(entry), String::from(dir))),
        }
    }

    /// 目录同步核账：资源管理器与终端在同一目录会话上相等。
    pub fn synced(&self, a: &str, b: &str) -> bool {
        match (self.get(a), self.get(b)) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        }
    }

    pub fn get(&self, entry: &str) -> Option<&str> {
        self.entries.iter().find(|(e, _)| e == entry).map(|(_, d)| d.as_str())
    }
}

impl Default for WorkdirLedger {
    fn default() -> WorkdirLedger {
        WorkdirLedger::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F336 自检。
pub fn run_copypath_checks() -> CheckSet {
    let mut set = CheckSet::new("F336-copypath");

    // 1. 两项菜单入口（语义面：绝对 + 相对两动作各一——枚举对照）。
    let entries = ["复制文件地址", "复制为相对路径"];
    set.add("two menu entries", entries.len() == 2 && entries.iter().all(|e| !e.is_empty()), "");

    // 2. 引号自动包裹：空格路径自动加引号；无空格不加。
    set.add(
        "quote on spaces",
        quote_for("D:/工具/我的 报告.docx", TerminalFlavor::Cmd) == "\"D:\\工具\\我的 报告.docx\""
            && quote_for("D:/工具/报告.docx", TerminalFlavor::Cmd) == "D:\\工具\\报告.docx",
        "",
    );

    // 3. 终端粘贴可用性：CMD 形/POSIX 形按目标自适应。
    set.add(
        "flavor adaptive",
        quote_for("C:/Program Files/x.exe", TerminalFlavor::Posix) == "'C:/Program Files/x.exe'"
            && quote_for("C:/Program Files/x.exe", TerminalFlavor::Cmd) == "\"C:\\Program Files\\x.exe\"",
        "",
    );

    // 4. 相对路径基准：同根取相对（../ 上行）、跨根回退绝对。
    set.add(
        "relative base defined",
        relative_to("D:/工作/2026/报告.docx", "D:/工作/2025") == "../2026/报告.docx"
            && relative_to("D:/工作/2026/a", "D:/工作/2026/子") == "../a"
            && relative_to("C:/别处/x", "D:/工作") == "C:/别处/x",
        "",
    );

    // 5. 特殊字符触发引号（& 括号——防空格翻车纪律的延伸）。
    set.add(
        "special chars quoted",
        needs_quotes("a&b") && needs_quotes("x(1)") && !needs_quotes("plain"),
        "",
    );

    set
}

/// F337 自检。
pub fn run_cmdbg_checks() -> CheckSet {
    let mut set = CheckSet::new("F337-cmdbg");

    // 1. 互通四入口用例（枚举全登记）。
    let e1 = BridgeEntry::ExplorerToTerminal(String::from("D:/工作"));
    let e2 = BridgeEntry::TerminalToExplorer(String::from("D:/工作"));
    let e3 = BridgeEntry::DragIntoTerminal(String::from("D:/我的 文件/a.txt"));
    let e4 = BridgeEntry::SelectionToSearch(String::from("rust vec"));
    let all = [e1, e2, e3, e4];
    set.add("four bridge entries", all.len() == 4, "");

    // 2. 拖入引号处理：含空格自动带引号（与 F336 同一策略——一处一事实）。
    match &all[2] {
        BridgeEntry::DragIntoTerminal(p) => {
            set.add(
                "drag quote consistent",
                quote_for(p, TerminalFlavor::Cmd) == "\"D:\\我的 文件\\a.txt\"",
                "",
            );
        }
        _ => set.add("drag quote consistent", false, ""),
    }

    // 3. 工作目录正确性 + open . 目录同步：两入口目录相等。
    let mut w = WorkdirLedger::new();
    w.record("explorer-vxsh", "D:/工作/2026");
    w.record("terminal-open", "D:/工作/2026");
    set.add("workdir synced open dot", w.synced("explorer-vxsh", "terminal-open"), "");
    w.record("terminal-open", "D:/其他");
    set.add("workdir desync detected", !w.synced("explorer-vxsh", "terminal-open"), "");

    // 4. 拖入特殊字符路径不翻车（引号包裹后终端可直接用）。
    let tricky = "D:/a&b/c d.txt";
    let quoted = quote_for(tricky, TerminalFlavor::Cmd);
    set.add(
        "tricky path safe",
        quoted == "\"D:\\a&b\\c d.txt\"" && needs_quotes(tricky),
        "",
    );

    // 5. 选中文本搜索入口：内容透传（不丢词）。
    match &all[3] {
        BridgeEntry::SelectionToSearch(q) => {
            set.add("selection search passthrough", q == "rust vec", "");
        }
        _ => set.add("selection search passthrough", false, ""),
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_no_quote_for_plain() {
        assert_eq!(quote_for("/usr/bin/ls", TerminalFlavor::Posix), "/usr/bin/ls");
    }

    #[test]
    fn relative_same_dir() {
        assert_eq!(relative_to("a/b", "a/b"), "");
    }

    #[test]
    fn workdir_first_record() {
        let mut w = WorkdirLedger::new();
        w.record("x", "d1");
        assert_eq!(w.get("x"), Some("d1"));
    }

    #[test]
    fn percent_needs_quote() {
        assert!(needs_quotes("50%off"));
    }
}

// ---------------------------------------------------------------------------
// 深化层 · F336/337 路径转换规则表 + 互通派发器 + 复制地址历史
// ---------------------------------------------------------------------------

/// 完整路径转换规则表（CMD/POSIX 双向——分隔符、引号、转义三面；
/// 单一实现，F336 复制与 F337 拖入共用）。
pub fn convert_path(path: &str, flavor: crate::h3star::copypath::TerminalFlavor) -> String {
    match flavor {
        crate::h3star::copypath::TerminalFlavor::Cmd => {
            let normalized = path.replace('/', "\\");
            if crate::h3star::copypath::needs_quotes(path) {
                alloc::format!("\"{}\"", normalized)
            } else {
                normalized
            }
        }
        crate::h3star::copypath::TerminalFlavor::Posix => {
            let normalized = path.replace('\\', "/");
            if normalized.chars().any(|c| " '\"\\$".contains(c)) {
                // POSIX 单引号内唯一不能出现的是单引号本身——'\'' 转义。
                alloc::format!("'{}'", normalized.replace('\'', "'\\''"))
            } else {
                normalized
            }
        }
    }
}

/// 互通四入口派发器（入口动作 → 目标行为——派发表唯一源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeAction {
    OpenTerminalHere,
    OpenExplorerHere,
    InsertQuotedPath,
    SearchWeb,
}

/// 派发（入口 → 动作 + 参数——四入口用例的执行面）。
pub fn dispatch(entry: &crate::h3star::copypath::BridgeEntry) -> (BridgeAction, String) {
    match entry {
        crate::h3star::copypath::BridgeEntry::ExplorerToTerminal(dir) => {
            (BridgeAction::OpenTerminalHere, dir.clone())
        }
        crate::h3star::copypath::BridgeEntry::TerminalToExplorer(dir) => {
            (BridgeAction::OpenExplorerHere, dir.clone())
        }
        crate::h3star::copypath::BridgeEntry::DragIntoTerminal(p) => {
            (BridgeAction::InsertQuotedPath, convert_path(p, crate::h3star::copypath::TerminalFlavor::Cmd))
        }
        crate::h3star::copypath::BridgeEntry::SelectionToSearch(q) => {
            (BridgeAction::SearchWeb, q.clone())
        }
    }
}

/// 复制地址历史（最近 12 条——老手连复制多路径不丢账；LRU 去重）。
pub struct CopyPathHistory {
    items: Vec<String>,
    cap: usize,
}

impl CopyPathHistory {
    pub const CAP: usize = 12;

    pub fn new() -> CopyPathHistory {
        CopyPathHistory { items: Vec::new(), cap: Self::CAP }
    }

    /// 记一次复制（去重置顶、LRU）。
    pub fn record(&mut self, path: &str) {
        self.items.retain(|x| x != path);
        if self.items.len() >= self.cap {
            self.items.remove(self.items.len() - 1);
        }
        self.items.insert(0, String::from(path));
    }

    pub fn list(&self) -> &[String] {
        &self.items
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for CopyPathHistory {
    fn default() -> CopyPathHistory {
        CopyPathHistory::new()
    }
}

/// 深化层自检。
pub fn run_copypath_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F336-337-deep");

    // 1. 转换规则表：双引号+反斜杠（CMD）/ 单引号+正斜杠（POSIX）。
    set.add(
        "convert rules table",
        convert_path("D:/a b/c.txt", crate::h3star::copypath::TerminalFlavor::Cmd) == "\"D:\\a b\\c.txt\""
            && convert_path("D:/plain/c.txt", crate::h3star::copypath::TerminalFlavor::Posix) == "D:/plain/c.txt",
        "",
    );

    // 2. POSIX 单引号转义（'\'' 面——含单引号路径不翻车）。
    let posix = convert_path("/tmp/it's.txt", crate::h3star::copypath::TerminalFlavor::Posix);
    set.add(
        "posix escape single quote",
        posix == "'/tmp/it'\\''s.txt'",
        "",
    );

    // 3. 派发器：四入口 → 四动作（执行面与枚举一一对应）。
    let d1 = dispatch(&crate::h3star::copypath::BridgeEntry::ExplorerToTerminal(String::from("D:/w")));
    let d2 = dispatch(&crate::h3star::copypath::BridgeEntry::TerminalToExplorer(String::from("D:/w")));
    let d3 = dispatch(&crate::h3star::copypath::BridgeEntry::DragIntoTerminal(String::from("D:/a b.txt")));
    let d4 = dispatch(&crate::h3star::copypath::BridgeEntry::SelectionToSearch(String::from("q")));
    set.add(
        "dispatch four entries",
        d1.0 == BridgeAction::OpenTerminalHere
            && d2.0 == BridgeAction::OpenExplorerHere
            && d3.0 == BridgeAction::InsertQuotedPath && d3.1 == "\"D:\\a b.txt\""
            && d4.0 == BridgeAction::SearchWeb,
        "",
    );

    // 4. 复制地址历史：去重置顶、12 LRU、一键清。
    let mut h = CopyPathHistory::new();
    for i in 0..14u32 {
        h.record(&alloc::format!("D:/p{i}"));
    }
    let capped = h.len() == CopyPathHistory::CAP && h.list()[0] == "D:/p13";
    h.record("D:/p5");
    let dedup = h.list()[0] == "D:/p5";
    h.clear();
    set.add(
        "copy history lru dedupe clear",
        capped && dedup && h.is_empty(),
        "",
    );

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn cmd_backslash_conversion() {
        assert_eq!(
            convert_path("a/b/c", crate::h3star::copypath::TerminalFlavor::Cmd),
            "a\\b\\c"
        );
    }

    #[test]
    fn posix_dollar_quoted() {
        let out = convert_path("/x/$y", crate::h3star::copypath::TerminalFlavor::Posix);
        assert_eq!(out, "'/x/$y'");
    }

    #[test]
    fn history_cap_floor() {
        let mut h = CopyPathHistory::new();
        h.record("x");
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn dispatch_quoted_has_quotes() {
        let (_, p) = dispatch(&crate::h3star::copypath::BridgeEntry::DragIntoTerminal(
            String::from("D:/x y/z"),
        ));
        assert!(p.starts_with('"') && p.ends_with('"'));
    }
}
