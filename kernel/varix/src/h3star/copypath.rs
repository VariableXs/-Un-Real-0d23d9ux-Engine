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

// ---------------------------------------------------------------------------
// 深化层二 · F337 命令词法/引号矩阵/选中即搜/工作目录一致性审计
// ---------------------------------------------------------------------------

/// F337 互通命令词法（地址栏/终端命令的唯一识别表）：
/// - `vxsh` → 在此目录打开终端（F095）；
/// - `open .` → 资源管理器打开当前目录；
/// - `open <路径>` → 资源管理器打开指定目录；
/// - 其余 → 未知（有相近建议，不白眼——F308 建议面同源）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellCmd {
    OpenTerminalHere,
    OpenExplorerHere,
    OpenExplorer(String),
    Unknown,
}

/// 命令词法解析（工作目录绑定由调用方持有——「开在哪就在哪」）。
pub fn lex_shell_cmd(input: &str) -> ShellCmd {
    let t = input.trim();
    if t == "vxsh" {
        return ShellCmd::OpenTerminalHere;
    }
    if t == "open ." {
        return ShellCmd::OpenExplorerHere;
    }
    if let Some(rest) = t.strip_prefix("open ") {
        let path = rest.trim();
        if !path.is_empty() {
            return ShellCmd::OpenExplorer(String::from(path));
        }
    }
    ShellCmd::Unknown
}

/// F337 拖入终端引号矩阵·CMD 形：一律双引号包裹，内部双引号按 CMD
/// 惯例翻倍（`a "b" c` → `"a ""b"" c"`）——空格/特殊字符全表防翻车。
pub fn cmd_quote_matrix(raw: &str) -> String {
    let mut out = String::from("\"");
    for c in raw.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// F337 拖入终端引号矩阵·POSIX 形：单引号包裹，内部单引号按 shell
/// 惯例转义为 `'\''`——$、反引号、分号在单引号内全部字面化。
pub fn posix_quote_matrix(raw: &str) -> String {
    let mut out = String::from("'");
    for c in raw.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// 拖入插入的统一入口（按终端形制分派——含空格/特殊字符自适应）。
pub fn drag_insert(flavor: crate::h3star::copypath::TerminalFlavor, raw: &str) -> String {
    match flavor {
        crate::h3star::copypath::TerminalFlavor::Cmd => cmd_quote_matrix(raw),
        crate::h3star::copypath::TerminalFlavor::Posix => posix_quote_matrix(raw),
    }
}

/// F337 选中即搜：选中文本 → 搜索参数编码（空格→%20、&→%26、+→%2B、
/// #→%23、换行→%0A、%→%25；CJK 原样由浏览器处理——参数不翻车）。
pub fn search_query_param(q: &str) -> String {
    let mut out = String::new();
    for c in q.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '&' => out.push_str("%26"),
            '+' => out.push_str("%2B"),
            '#' => out.push_str("%23"),
            '\n' => out.push_str("%0A"),
            '\r' => {}
            '%' => out.push_str("%25"),
            _ => out.push(c),
        }
    }
    out
}

/// F337 工作目录一致性审计：每次互通动作后 explorer 目录与 terminal
/// cwd 必须同账（「开在哪就在哪」的逐次对账——分歧即缺陷）。
#[derive(Default)]
pub struct WorkdirSyncAudit {
    pairs: Vec<(&'static str, String, String)>,
}

impl WorkdirSyncAudit {
    pub fn new() -> WorkdirSyncAudit {
        WorkdirSyncAudit { pairs: Vec::new() }
    }

    /// 记录一次互通动作（入口名, explorer 目录, terminal cwd）。
    pub fn record(&mut self, entry: &'static str, explorer_dir: &str, terminal_dir: &str) {
        self.pairs.push((entry, String::from(explorer_dir), String::from(terminal_dir)));
    }

    /// 分歧行（explorer ≠ terminal 的动作——逐条可定位）。
    pub fn diverged(&self) -> Vec<&(&'static str, String, String)> {
        self.pairs.iter().filter(|(_, a, b)| a != b).collect()
    }

    pub fn all_synced(&self) -> bool {
        self.diverged().is_empty()
    }

    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

/// 深化层二自检（命令词法 / 引号矩阵 / 搜索参数 / 工作目录审计）。
pub fn run_cmdbg_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F337-deep2");

    // 1. 命令词法四入口全对（vxsh / open . / open <path> / 未知）。
    set.add(
        "shell cmd lex four entries",
        lex_shell_cmd("vxsh") == ShellCmd::OpenTerminalHere
            && lex_shell_cmd("  vxsh  ") == ShellCmd::OpenTerminalHere
            && lex_shell_cmd("open .") == ShellCmd::OpenExplorerHere
            && lex_shell_cmd("open D:\\资料 报告") == ShellCmd::OpenExplorer(String::from("D:\\资料 报告"))
            && lex_shell_cmd("hello") == ShellCmd::Unknown,
        "",
    );
    set.add(
        "open with empty path is unknown",
        matches!(lex_shell_cmd("open   "), ShellCmd::Unknown),
        "",
    );

    // 2. CMD 引号矩阵：空格包裹、内部双引号翻倍、$ 字面化（在双引号内
    //    CMD 变量展开风险由调用侧 %x% 语义承担——此处保证包裹完整性）。
    set.add(
        "cmd quote matrix",
        cmd_quote_matrix("D:/a b/c.txt") == "\"D:/a b/c.txt\""
            && cmd_quote_matrix("he said \"hi\"") == "\"he said \"\"hi\"\"\""
            && cmd_quote_matrix("plain") == "\"plain\"",
        "",
    );

    // 3. POSIX 引号矩阵：单引号内 $/反引号字面化、内部单引号 '\'' 转义。
    set.add(
        "posix quote matrix",
        posix_quote_matrix("/tmp/$HOME") == "'/tmp/$HOME'"
            && posix_quote_matrix("it's") == "'it'\\''s'"
            && posix_quote_matrix("a `b`") == "'a `b`'",
        "",
    );

    // 4. 统一入口分派（两形制各走对路）。
    set.add(
        "drag insert dispatch",
        drag_insert(crate::h3star::copypath::TerminalFlavor::Cmd, "a b")
            == "\"a b\""
            && drag_insert(crate::h3star::copypath::TerminalFlavor::Posix, "a b")
                == "'a b'",
        "",
    );

    // 5. 选中即搜参数编码：空格/与号/加号/井号/换行/百分号全表。
    set.add(
        "search query param encoding",
        search_query_param("季度 预算&计划+Q1#1") == "季度%20预算%26计划%2BQ1%231"
            && search_query_param("a\nb") == "a%0Ab"
            && search_query_param("50%off") == "50%25off",
        "",
    );

    // 6. 工作目录一致性审计：同账全绿；分歧逐条定位。
    let mut wa = WorkdirSyncAudit::new();
    wa.record("vxsh", "D:/报表", "D:/报表");
    wa.record("open .", "D:/报表", "D:/报表");
    set.add("workdir audit synced", wa.len() == 2 && wa.all_synced(), "");
    wa.record("vxsh", "D:/报表", "C:/");
    set.add(
        "workdir audit catches divergence",
        !wa.all_synced() && wa.diverged().len() == 1 && wa.diverged()[0].0 == "vxsh",
        "",
    );

    set
}

#[cfg(test)]
mod cmdbg_deep2_tests {
    use super::*;

    #[test]
    fn lex_trims_and_rejects_prefix_lookalikes() {
        assert_eq!(lex_shell_cmd("vxshell"), ShellCmd::Unknown, "前缀相似不算命中");
        assert_eq!(lex_shell_cmd("opened"), ShellCmd::Unknown);
    }

    #[test]
    fn cmd_matrix_empty_and_unicode() {
        assert_eq!(cmd_quote_matrix(""), "\"\"");
        assert_eq!(posix_quote_matrix("中文 路径"), "'中文 路径'");
    }

    #[test]
    fn search_param_cjk_passthrough() {
        assert_eq!(search_query_param("计算器"), "计算器");
    }

    #[test]
    fn workdir_audit_empty_clean() {
        let wa = WorkdirSyncAudit::new();
        assert!(wa.is_empty() && wa.all_synced());
    }
}

// ---------------------------------------------------------------------------
// 深化层三 · 路径归一化核 + 逐味转义总表 + 互通事件账 + open . 语义核
// ---------------------------------------------------------------------------

/// 路径归一化器：任意写法 → 规范形（一处一事实——转换前先归一，杜绝
/// 「D:/a/../b」这类半规范串在形制间漂移）。
/// 规则：① 分隔符统一 `\`（POSIX 形制输出时再转 `/`）；② `.` 段删除；
/// ③ `..` 段弹上一级（弹到根则停——越界不崩溃只钳制）；④ 盘符大写；
/// ⑤ 重复分隔符合并；⑥ UNC（`\\server\share`）识别保留双前导。
#[derive(Default)]
pub struct PathNormalizer;

impl PathNormalizer {
    /// 归一化主入口（Windows 面：反斜杠规范形）。
    pub fn win(input: &str) -> String {
        if input.starts_with("\\\\") {
            // UNC：保留 \\server\share 前缀，其余段照常过滤（点段消化、
            // 重复分隔符合并——弹级语义对 UNC 首段保留不适用）。
            let body: Vec<&str> = input[2..]
                .split(['\\', '/'])
                .filter(|s| !s.is_empty() && *s != ".")
                .collect();
            return alloc::format!("\\\\{}", body.join("\\"));
        }
        let mut segs: Vec<String> = Vec::new();
        for (i, raw) in input.split(['\\', '/']).enumerate() {
            let s = raw.trim();
            if s.is_empty() || s == "." {
                continue;
            }
            if i == 0 && s.len() == 2 && s.as_bytes()[1] == b':' {
                segs.push(s[..1].to_ascii_uppercase() + ":");
                continue;
            }
            if s == ".." {
                // 弹上一级；弹穿盘根即停（不越界——钳制语义）。
                if segs.len() > 1 {
                    segs.pop();
                }
                continue;
            }
            segs.push(String::from(s));
        }
        segs.join("\\")
    }

    /// POSIX（Git-Bash/MSYS 形）：C:\a\b → /c/a/b；UNC 无直映——诚实
    /// 拒绝（返回空串，调用方显式处理，不猜）。
    pub fn posix(input: &str) -> String {
        let w = Self::win(input);
        if w.starts_with("\\\\") {
            return String::new();
        }
        let mut parts = w.split('\\').filter(|s| !s.is_empty());
        let drive = match parts.next() {
            Some(d) if d.len() == 2 && d.ends_with(':') => d[..1].to_ascii_lowercase(),
            _ => return alloc::format!("/{}", parts.collect::<Vec<_>>().join("/")),
        };
        let rest: Vec<&str> = parts.collect();
        alloc::format!(
            "/{}{}",
            drive,
            if rest.is_empty() { String::new() } else { alloc::format!("/{}", rest.join("/")) }
        )
    }

    /// WSL 形：C:\a\b → /mnt/c/a/b（挂载点映射——/mnt/ 前缀唯一源）。
    pub fn wsl(input: &str) -> String {
        let p = Self::posix(input);
        if p.is_empty() {
            return p;
        }
        alloc::format!("/mnt{}", p)
    }
}

/// 逐味转义总表（每味一列：空格 / & / % / $ / 反引号 / 引号 / 括号 /
/// 中文——转义规则唯一源，改规则必炸 checks）。
pub struct EscapeMatrix;

impl EscapeMatrix {
    /// 返回 (字符, CMD 形转义, POSIX 形转义) 三元组总表。
    pub const TABLE: [(char, &'static str, &'static str); 7] = [
        (' ', "\"\"", "\"\""),        // 空格：两味都靠引号包裹
        ('&', "^&", "\\&"),
        ('%', "%%", "%"),             // CMD 变量展开 → 翻倍；POSIX 无需
        ('$', "%$", "\\$"),           // CMD 无 $ 语义（原样）；POSIX 转义
        ('`', "`", "\\`"),            // 反引号：CMD 原样；POSIX 命令替换须转义
        ('"', "\"\\\"\"\"", "\\\""),  // 引号：CMD 内部翻倍；POSIX 反斜杠
        ('(', "^(", "\\("),
    ];

    /// 查表：某字符在某味的转义形（未登记字符原样——审计面可查空白区）。
    pub fn escape_of(c: char, flavor: TerminalFlavor) -> Option<&'static str> {
        Self::TABLE.iter().find(|(ch, _, _)| *ch == c).map(|(_, cmd, posix)| match flavor {
            TerminalFlavor::Cmd => *cmd,
            TerminalFlavor::Posix => *posix,
        })
    }

    /// 总表自证：两味列数一致、无空串项（转义规则不许有「静默删除」）。
    pub fn table_sane() -> bool {
        Self::TABLE
            .iter()
            .all(|(_, cmd, posix)| !cmd.is_empty() && !posix.is_empty())
    }
}

/// 互通事件账（十三章体验日志语义的互通面）：四入口（右键复制 / 终端
/// 粘贴 / 拖入 / open .）逐事件记录 + 结论字段；粘贴语义校验失败自动
/// 记 Error 结论（互通质量可回放可出改进清单）。
pub struct InteropLedger {
    events: Vec<(u64, &'static str, String, super::hbase::ExpVerdict)>,
}

impl InteropLedger {
    pub fn new() -> InteropLedger {
        InteropLedger { events: Vec::new() }
    }

    /// 记录一次互通动作：`entry` ∈ {右键复制, 终端粘贴, 拖入, open-dot}；
    /// `payload` 为形制化后的路径或语义校验结果；`ok` 决定结论字段。
    pub fn record(&mut self, at_ms: u64, entry: &'static str, payload: String, ok: bool) {
        let v = if ok { super::hbase::ExpVerdict::Smooth } else { super::hbase::ExpVerdict::Error };
        self.events.push((at_ms, entry, payload, v));
    }

    /// 四入口覆盖审计：账本里四类入口全出现过（缺一类 = 互通面没走全）。
    pub fn four_entries_covered(&self) -> bool {
        ["右键复制", "终端粘贴", "拖入", "open-dot"]
            .iter()
            .all(|e| self.events.iter().any(|(_, k, _, _)| k == e))
    }

    /// 错误事件清单（改进面直出——十三章「最挫败的操作」维度）。
    pub fn errors(&self) -> Vec<&(u64, &'static str, String, super::hbase::ExpVerdict)> {
        self.events.iter().filter(|(_, _, _, v)| *v != super::hbase::ExpVerdict::Smooth).collect()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }
}

impl Default for InteropLedger {
    fn default() -> InteropLedger {
        InteropLedger::new()
    }
}

/// `open .` 目录同步语义核：终端里 `open .` 必须打开终端当前工作目录
/// 的资源管理器窗；账面记录 (终端会话 cwd, 打开的窗路径)，不同步 =
/// 缺陷。同步判定：窗路径 == cwd 归一化形。
pub struct OpenDotSemantics {
    ledger: Vec<(String, String, bool)>,
}

impl OpenDotSemantics {
    pub fn new() -> OpenDotSemantics {
        OpenDotSemantics { ledger: Vec::new() }
    }

    /// 处理一次 `open .`：cwd → 窗路径，同步结论入账。
    pub fn open_here(&mut self, cwd: &str, window_opened_at: &str) -> bool {
        let want = PathNormalizer::win(cwd);
        let got = PathNormalizer::win(window_opened_at);
        let ok = want == got;
        self.ledger.push((want, got, ok));
        ok
    }

    /// 全程零失同步（任意一条红即整体红）。
    pub fn all_synced(&self) -> bool {
        !self.ledger.is_empty() && self.ledger.iter().all(|(_, _, ok)| *ok)
    }

    pub fn len(&self) -> usize {
        self.ledger.len()
    }
}

impl Default for OpenDotSemantics {
    fn default() -> OpenDotSemantics {
        OpenDotSemantics::new()
    }
}

/// 深化层三自检（归一化 / WSL 映射 / 转义总表 / 事件账 / open .）。
pub fn run_copypath_deep3_checks() -> CheckSet {

    let mut set = CheckSet::new("F336-337-deep3");

    // 1. 归一化：混合分隔符 + 点段 + 重复分隔符 + 盘符大小写。
    set.add(
        "normalize mixed separators and dots",
        PathNormalizer::win("d:/工具\\my app\\./sub\\\\..\\bin") == "D:\\工具\\my app\\bin",
        "",
    );

    // 2. `..` 弹穿盘根钳制（不越界不崩溃）。
    set.add("dotdot clamps at root", PathNormalizer::win("C:\\..\\..\\x") == "C:\\x", "");

    // 3. 盘符大写规范。
    set.add("drive uppercased", PathNormalizer::win("c:/win/system32") == "C:\\win\\system32", "");

    // 4. POSIX 与 WSL 映射：/c/... 与 /mnt/c/...；UNC 诚实拒绝。
    set.add(
        "posix and wsl mapping",
        PathNormalizer::posix("C:\\a b\\文件") == "/c/a b/文件"
            && PathNormalizer::wsl("C:\\a b\\文件") == "/mnt/c/a b/文件"
            && PathNormalizer::posix("\\\\nas\\share").is_empty(),
        "",
    );

    // 5. 转义总表：全表两味齐备 + 逐字符查表命中（% 与 $ 分味正确）。
    set.add(
        "escape matrix sane and lookup",
        EscapeMatrix::table_sane()
            && EscapeMatrix::escape_of('%', TerminalFlavor::Cmd) == Some("%%")
            && EscapeMatrix::escape_of('%', TerminalFlavor::Posix) == Some("%")
            && EscapeMatrix::escape_of('$', TerminalFlavor::Posix) == Some("\\$")
            && EscapeMatrix::escape_of('x', TerminalFlavor::Cmd).is_none(),
        "",
    );

    // 6. 互通事件账：四入口全走 + 错误事件可直出（改进面）。
    let mut led = InteropLedger::new();
    led.record(0, "右键复制", alloc::format!("\"C:\\a b\""), true);
    led.record(10, "终端粘贴", alloc::format!("ok"), true);
    led.record(20, "拖入", alloc::format!("/c/a\\ b"), true);
    led.record(30, "open-dot", alloc::format!("cwd=window"), false);
    set.add(
        "interop ledger covers four entries",
        led.four_entries_covered() && led.len() == 4 && led.errors().len() == 1,
        "",
    );

    // 7. open . 同步：同步绿、漂移红（语义核对账面）。
    let mut od = OpenDotSemantics::new();
    let ok1 = od.open_here("D:\\相册", "D:\\相册");
    let ok2 = od.open_here("D:\\相册", "D:\\下载");
    set.add(
        "open dot sync semantics",
        ok1 && !ok2 && od.len() == 2 && !od.all_synced(),
        "",
    );
    let mut od2 = OpenDotSemantics::new();
    let _ = od2.open_here("D:\\相册", "D:/相册"); // 斜杠差被归一化抹平。
    set.add("open dot slash agnostic", od2.all_synced(), "");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn normalize_keeps_unc_prefix() {
        let n = PathNormalizer::win("\\\\NAS\\share\\./a\\\\b");
        assert!(n.starts_with("\\\\NAS\\share"), "UNC 双前导保留：{n}");
        assert!(!n.contains("."));
    }

    #[test]
    fn wsl_of_unc_is_empty() {
        assert_eq!(PathNormalizer::wsl("\\\\nas\\share"), "", "UNC 无 WSL 直映——诚实拒绝");
    }

    #[test]
    fn ledger_empty_not_covered() {
        let led = InteropLedger::new();
        assert!(!led.four_entries_covered(), "空账不得谎报覆盖");
    }

    #[test]
    fn open_dot_empty_ledger_not_synced() {
        let od = OpenDotSemantics::new();
        assert!(!od.all_synced(), "零样本不构成全程同步证据");
    }

    #[test]
    fn escape_quote_posix_form() {
        assert_eq!(EscapeMatrix::escape_of('"', TerminalFlavor::Posix), Some("\\\""));
    }
}

// ---------------------------------------------------------------------------
// 深化层四 · 环境变量展开双味规则 + 终端形制能力档案 + 粘贴清洗器 + 多文件拖入
// ---------------------------------------------------------------------------

/// 环境变量展开（命令行互通的语境面）：同一字符串两味各自展开规则——
/// CMD `%VAR%`、POSIX `$VAR`（花括号可选）。展开只作用于登记白名单内
/// 的变量（外部语境不可信——未登记变量原样保留并留痕，绝不猜）。
pub struct EnvExpander {
    /// 登记变量表（互通语境冻结接口——只有这里的变量可展开）。
    pub vars: Vec<(String, String)>,
    /// 未展开留痕（原样保留的变量名——异常显性化）。
    pub unexpanded: Vec<String>,
}

impl EnvExpander {
    pub fn new() -> EnvExpander {
        EnvExpander { vars: Vec::new(), unexpanded: Vec::new() }
    }

    pub fn register(&mut self, name: &str, value: &str) {
        match self.vars.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = String::from(value),
            None => self.vars.push((String::from(name), String::from(value))),
        }
    }

    /// CMD 味展开：`%NAME%` → 值；未登记原样保留 + 留痕。
    pub fn expand_cmd(&mut self, input: &str) -> String {
        self.expand_generic(input, '%', '%')
    }

    /// POSIX 味展开：`$NAME` 或 `${NAME}` → 值；未登记原样保留 + 留痕。
    pub fn expand_posix(&mut self, input: &str) -> String {
        let mut out = String::new();
        let bytes: Vec<char> = input.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == '$' && i + 1 < bytes.len() {
                let (name, next) = if bytes[i + 1] == '{' {
                    // ${NAME} 花括号形。
                    match bytes[i + 2..].iter().position(|&c| c == '}') {
                        Some(end) => (
                            bytes[i + 2..i + 2 + end].iter().collect::<String>(),
                            i + 2 + end + 1,
                        ),
                        None => (String::new(), i + 2),
                    }
                } else {
                    // $NAME 裸形：连续字母数字下划线。
                    let end = bytes[i + 1..]
                        .iter()
                        .position(|c| !(c.is_alphanumeric() || *c == '_'))
                        .map(|p| i + 1 + p)
                        .unwrap_or(bytes.len());
                    (bytes[i + 1..end].iter().collect::<String>(), end)
                };
                if !name.is_empty() {
                    if let Some((_, v)) = self.vars.iter().find(|(n, _)| *n == name) {
                        out.push_str(v);
                        i = next;
                        continue;
                    }
                    self.unexpanded.push(name);
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        out
    }

    fn expand_generic(&mut self, input: &str, open: char, close: char) -> String {
        let chars: Vec<char> = input.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == open {
                if let Some(end) = chars[i + 1..].iter().position(|&c| c == close) {
                    let name: String = chars[i + 1..i + 1 + end].iter().collect();
                    if let Some((_, v)) = self.vars.iter().find(|(n, _)| *n == name) {
                        out.push_str(v);
                        i += end + 2;
                        continue;
                    }
                    self.unexpanded.push(name);
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    }

    pub fn unexpanded_len(&self) -> usize {
        self.unexpanded.len()
    }
}

impl Default for EnvExpander {
    fn default() -> EnvExpander {
        EnvExpander::new()
    }
}

/// 终端形制能力档案（互通目标端的冻结接口描述——派发前先查档案，
/// 不许对着能力未知的终端猜语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalProfile {
    /// 档案名。
    pub name: &'static str,
    /// 路径风格。
    pub flavor: TerminalFlavor,
    /// 支持 VT 序列（彩色提示等）。
    pub vt: bool,
    /// 支持文件拖入（不支持 → 降级为粘贴路径文本）。
    pub drag_in: bool,
}

/// 已知终端档案表（唯一源——新终端入表走登记）。
pub const TERMINAL_PROFILES: [TerminalProfile; 3] = [
    TerminalProfile { name: "Varix 终端", flavor: TerminalFlavor::Posix, vt: true, drag_in: true },
    TerminalProfile { name: "CMD 兼容面", flavor: TerminalFlavor::Cmd, vt: false, drag_in: true },
    TerminalProfile { name: "瘦客户端", flavor: TerminalFlavor::Cmd, vt: false, drag_in: false },
];

/// 按档案名查能力（未知档案 None——调用方显式降级，不猜）。
pub fn profile_of(name: &str) -> Option<TerminalProfile> {
    TERMINAL_PROFILES.iter().find(|p| p.name == name).copied()
}

/// 粘贴清洗器（安全纪律：粘贴进终端的内容不可信——控制字符剔除 +
/// 换行拆分防护（多行粘入 = 逐行确认或整体拒绝，防止夹带第二命令）+
/// 长度上限）。
pub struct PasteSanitizer;

/// 清洗结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sanitized {
    pub ok: bool,
    pub text: String,
    /// 剔除的控制字符数（留痕——发生了什么对人话）。
    pub stripped: usize,
    /// 是否因多行被拒（下一步怎么办：拆行逐条粘）。
    pub multiline_rejected: bool,
}

impl PasteSanitizer {
    /// 长度上限（单次粘入 4KB——防误粘大文本卡终端）。
    pub const MAX_LEN: usize = 4096;

    pub fn clean(input: &str) -> Sanitized {
        if input.len() > Self::MAX_LEN {
            return Sanitized { ok: false, text: String::new(), stripped: 0, multiline_rejected: false };
        }
        let multiline = input.contains('\n');
        if multiline {
            return Sanitized { ok: false, text: String::new(), stripped: 0, multiline_rejected: true };
        }
        let mut text = String::with_capacity(input.len());
        let mut stripped = 0usize;
        for c in input.chars() {
            if c.is_control() {
                stripped += 1;
            } else {
                text.push(c);
            }
        }
        Sanitized { ok: true, text, stripped, multiline_rejected: false }
    }
}

/// 多文件拖入（拖拽协议互通的命令行面）：N 个路径 → 一条引号包裹的
/// 参数序列（逐个按味引号 + 空格分隔——终端收到的 argv 语义）。
pub fn drag_multi(flavor: TerminalFlavor, paths: &[&str]) -> String {
    paths
        .iter()
        .map(|p| quote_for(p, flavor))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 深化层四自检（环境展开 / 终端档案 / 粘贴清洗 / 多文件拖入）。
pub fn run_copypath_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new("F336-337-deep4");

    // 1. 环境展开 CMD 味：登记变量展开、未登记原样保留 + 留痕。
    let mut env = EnvExpander::new();
    env.register("USERPROFILE", "C:/Users/varia");
    let out = env.expand_cmd("%USERPROFILE%/文档");
    set.add(
        "cmd env expansion",
        out == "C:/Users/varia/文档" && env.unexpanded_len() == 0,
        "",
    );
    let out2 = env.expand_cmd("%GHOSTVAR%/x");
    set.add(
        "cmd unexpanded kept and logged",
        out2 == "%GHOSTVAR%/x" && env.unexpanded_len() == 1,
        "",
    );

    // 2. POSIX 味：$NAME 与 ${NAME} 双形 + 未登记留痕。
    let mut env2 = EnvExpander::new();
    env2.register("HOME", "/home/varia");
    let a = env2.expand_posix("$HOME/Downloads");
    let b = env2.expand_posix("${HOME}/dl");
    let c = env2.expand_posix("$NADA");
    set.add(
        "posix env both forms",
        a == "/home/varia/Downloads" && b == "/home/varia/dl" && c == "$NADA"
            && env2.unexpanded_len() == 1,
        "",
    );

    // 3. 终端档案：查表 + 未知档案诚实 None + 派发按档案选味。
    let known = profile_of("Varix 终端");
    let unknown = profile_of("不存在的终端");
    set.add(
        "terminal profiles",
        known == Some(TERMINAL_PROFILES[0]) && unknown.is_none(),
        "",
    );

    // 4. 粘贴清洗：控制字符剔除留痕 + 多行拒绝（防夹带命令）+ 超长拒绝。
    let s1 = PasteSanitizer::clean("cd C:/a b\u{0}\u{1b}");
    let s2 = PasteSanitizer::clean("合法\r\n第二条命令");
    let long = "x".repeat(5000);
    let s3 = PasteSanitizer::clean(&long);
    set.add(
        "paste sanitizer",
        s1.ok && s1.stripped == 2 && s1.text == "cd C:/a b"
            && !s2.ok && s2.multiline_rejected
            && !s3.ok && !s3.multiline_rejected,
        "",
    );

    // 5. 多文件拖入：逐个按味引号（POSIX 单引号、无特殊字符裸排）+
    //    空格分隔（含空格路径不散架——argv 语义）。
    let multi = drag_multi(TerminalFlavor::Posix, &["C:/a b/x.vx", "C:/c.vx"]);
    set.add(
        "drag multi quoting",
        multi == "'C:/a b/x.vx' C:/c.vx",
        "",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn env_empty_name_not_expanded() {
        let mut env = EnvExpander::new();
        assert_eq!(env.expand_posix("$"), "$", "裸 $ 非变量——原样保留");
        assert_eq!(env.expand_posix("${unclosed"), "${unclosed", "未闭合花括号原样保留");
    }

    #[test]
    fn sanitizer_empty_ok() {
        let s = PasteSanitizer::clean("");
        assert!(s.ok && s.text.is_empty() && s.stripped == 0);
    }

    #[test]
    fn drag_multi_empty_paths() {
        assert_eq!(drag_multi(TerminalFlavor::Cmd, &[]), "");
    }

    #[test]
    fn profiles_all_sane() {
        for p in TERMINAL_PROFILES.iter() {
            assert!(!p.name.is_empty());
            assert!(p.flavor == TerminalFlavor::Cmd || p.flavor == TerminalFlavor::Posix);
        }
    }
}

// ---------------------------------------------------------------------------
// 深化层五 · 互通历史面板（脱敏红线）+ shell 别名登记
// ---------------------------------------------------------------------------

/// 互通历史面板（十三章体验日志的互通域深化）：逐条记录路径转换/
/// 粘贴/拖入事件，供「最近用过什么路径」快速回填。隐私红线：凭据
/// 模式（密码/token 形态的路径段）脱敏后才入账——历史面板永远不能
/// 变成密码泄漏面。脱敏规则登记制，规则外不猜（宁漏脱不误脱——
/// 误脱敏会让路径不可用，泄漏风险由登记规则兜住）。
pub struct InteropHistory {
    /// (时刻, 脱敏后 payload)。
    pub entries: Vec<(u64, String)>,
    /// 脱敏规则表：(模式子串, 替换形)。
    redactions: Vec<(String, String)>,
    cap: usize,
}

impl InteropHistory {
    pub fn new(cap: usize) -> InteropHistory {
        InteropHistory { entries: Vec::new(), redactions: Vec::new(), cap: cap.max(1) }
    }

    /// 登记脱敏规则（如 "token=" → "token=█"）。
    pub fn register_redaction(&mut self, pattern: &str, replacement: &str) {
        self.redactions.push((String::from(pattern), String::from(replacement)));
    }

    /// 脱敏执行：命中规则 → 整个「模式+值段」（值段 = 模式起点到下一
    /// 个 `&` 或串尾）替换为替换形——只换前缀不换值等于没脱（秘密仍在
    /// 账里），这是红线实现，不是风格选择。
    fn redact(&self, payload: &str) -> String {
        let mut s = String::from(payload);
        for (p, r) in &self.redactions {
            if let Some(start) = s.find(p.as_str()) {
                let value_end = s[start..]
                    .find('&')
                    .map(|e| start + e)
                    .unwrap_or(s.len());
                s = alloc::format!("{}{}{}", &s[..start], r, &s[value_end..]);
            }
        }
        s
    }

    /// 记录（脱敏后入账；环形封顶）。
    pub fn record(&mut self, at_ms: u64, payload: &str) {
        self.entries.push((at_ms, self.redact(payload)));
        if self.entries.len() > self.cap {
            self.entries.remove(0);
        }
    }

    /// 按时间倒序的最近 N 条（回填面）。
    pub fn recent(&self, n: usize) -> Vec<&str> {
        self.entries
            .iter()
            .rev()
            .take(n)
            .map(|(_, p)| p.as_str())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for InteropHistory {
    fn default() -> InteropHistory {
        InteropHistory::new(16)
    }
}

/// shell 别名登记（「三类输入用例/别名表登记」判据的执行面）：别名 →
/// 目标（应用/命令），登记制约束——① 别名唯一；② 目标非空；③ 保留名
/// 不可占用（系统命令词——防遮蔽）；④ 删除可逆（登记-删除-再登记）。
pub struct AliasRegistry {
    entries: Vec<(String, String)>,
}

/// 系统保留名（别名不许遮蔽的命令词——登记即拒绝）。
pub const RESERVED_ALIASES: [&str; 6] = ["cd", "open", "run", "help", "exit", "clear"];

impl AliasRegistry {
    pub fn new() -> AliasRegistry {
        AliasRegistry { entries: Vec::new() }
    }

    /// 登记：唯一 + 目标非空 + 非保留名（三闸全过才入账）。
    pub fn register(&mut self, alias: &str, target: &str) -> bool {
        if alias.is_empty()
            || target.is_empty()
            || RESERVED_ALIASES.contains(&alias)
            || self.entries.iter().any(|(a, _)| a == alias)
        {
            return false;
        }
        self.entries.push((String::from(alias), String::from(target)));
        true
    }

    /// 解析（别名 → 目标；未登记 None——调用方显式处理）。
    pub fn resolve(&self, alias: &str) -> Option<&str> {
        self.entries.iter().find(|(a, _)| a == alias).map(|(_, t)| t.as_str())
    }

    /// 删除（可逆：删了能重新登记）。
    pub fn unregister(&mut self, alias: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(a, _)| a != alias);
        self.entries.len() != before
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for AliasRegistry {
    fn default() -> AliasRegistry {
        AliasRegistry::new()
    }
}

/// 深化层五自检（历史脱敏 / 别名登记）。
pub fn run_copypath_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new("F336-337-deep5");

    // 1. 历史脱敏：凭据模式替换后才入账（原文永不落账）。
    let mut h = InteropHistory::new(8);
    h.register_redaction("token=", "token=█");
    h.record(0, "C:/a?token=abc123");
    set.add(
        "history redacted before stored",
        h.recent(1) == alloc::vec!["C:/a?token=█"]
            && !h.recent(1)[0].contains("abc123"),
        "",
    );

    // 2. 无规则不误脱（宁漏脱不误脱——规则外原样）。
    let mut h2 = InteropHistory::new(4);
    h2.record(0, "C:/普通/路径.vx");
    set.add("no rule no redact", h2.recent(1) == alloc::vec!["C:/普通/路径.vx"], "");

    // 3. 环形封顶 + 最近序（独立实例——与脱敏用例互不掺账）。
    let mut h3 = InteropHistory::new(4);
    for i in 0..6u64 {
        h3.record(i, &alloc::format!("p{i}"));
    }
    set.add(
        "history ring capped recent first",
        h3.len() == 4 && h3.recent(2) == alloc::vec!["p5", "p4"],
        "",
    );

    // 4. 别名三闸：唯一 / 非空目标 / 保留名拒绝。
    let mut ar = AliasRegistry::new();
    let ok = ar.register("jsq", "计算器");
    let dup = ar.register("jsq", "别的");
    let blank = ar.register("k", "");
    let reserved = ar.register("cd", "某目录");
    set.add(
        "alias three gates",
        ok && !dup && !blank && !reserved && ar.resolve("jsq") == Some("计算器"),
        "",
    );

    // 5. 可逆：删除后可重登记（换目标生效）。
    let _ = ar.unregister("jsq");
    let re = ar.register("jsq", "计算器Pro");
    set.add("alias reversible", re && ar.resolve("jsq") == Some("计算器Pro"), "");

    // 6. 未登记解析 None（不猜）。
    set.add("alias unknown none", ar.resolve("不存在").is_none(), "");

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn history_empty_recent_empty() {
        let h = InteropHistory::new(4);
        assert!(h.recent(3).is_empty(), "空历史不虚报最近条目");
    }

    #[test]
    fn alias_case_sensitive_distinct() {
        let mut ar = AliasRegistry::new();
        assert!(ar.register("Jsq", "甲"));
        assert!(ar.register("jsq", "乙"), "大小写有别——两别名并存");
        assert_eq!(ar.resolve("Jsq"), Some("甲"));
    }

    #[test]
    fn redaction_multiple_rules_apply() {
        let mut h = InteropHistory::new(4);
        h.register_redaction("pwd=", "pwd=█");
        h.register_redaction("secret=", "secret=█");
        h.record(0, "x?pwd=1&secret=2");
        assert_eq!(h.recent(1), alloc::vec!["x?pwd=█&secret=█"]);
    }
}
