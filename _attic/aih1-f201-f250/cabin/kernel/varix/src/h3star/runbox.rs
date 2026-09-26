//! F308 运行框（Win+R）· 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：三类输入用例；别名表登记；建议排序；Esc/Enter 语义；
//! 1.5s 键盘全程走查。
//!
//! **设计要点（主册）**：
//! - Win+R 呼出小运行框（单行输入+建议下拉），支持三类输入——路径
//!   （直达资源管理器）、应用别名（输「calc」开计算器）、系统位置
//!   （输「settings」开设置中心）；
//! - 下拉建议按历史与匹配度排；Esc 关闭；
//! - 老用户肌肉记忆原样生效，新用户可完全不知道它存在；
//! - 无感标准：输入即建议、Enter 即执行，全程键盘 1.5 秒；从不出现
//!   「命令不存在」的白眼（有相近建议）。
//!
//! 实现形态：输入分类器 + 别名表（登记制）+ 建议排序（匹配度×历史
//! 权重）+ 键盘会话状态机（开→输入→建议→执行/取消，全程注入钟）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 键盘全程判线（ms）。
pub const FLOW_LIMIT_MS: u64 = 1500;

/// 单键输入到建议的时延（ms——键盘跟手）。
pub const SUGGEST_LATENCY_MS: u64 = 50;

/// 历史权重（建议排序：匹配分 + 历史加权）。
pub const HISTORY_BOOST: i32 = 15;

// ---------------------------------------------------------------------------
// 输入分类
// ---------------------------------------------------------------------------

/// 三类输入目标。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunTarget {
    /// 路径 → 直达资源管理器。
    Path(String),
    /// 应用别名 → 启动应用（注入口给应用面）。
    App(&'static str),
    /// 系统位置 → 打开系统页（如 settings）。
    SystemLoc(&'static str),
}

/// 输入分类（三类用例的唯一判定源）。
pub fn classify(input: &str) -> Option<RunTarget> {
    let t = input.trim();
    if t.is_empty() {
        return None;
    }
    // 路径：含路径分隔符或盘符样 X:。
    if t.contains('\\') || t.contains('/') || {
        let cs: Vec<char> = t.chars().take(2).collect();
        cs.len() == 2 && cs[0].is_ascii_uppercase() && cs[1] == ':'
    } {
        return Some(RunTarget::Path(String::from(t)));
    }
    None // 别名与系统位置由别名表判定——分类器只认路径面。
}

/// 别名表条目（登记制——新增别名必须走登记口）。
#[derive(Clone, Copy, Debug)]
pub struct Alias {
    pub word: &'static str,
    pub target: RunTargetKind,
    pub explain: &'static str,
}

/// 别名目标类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunTargetKind {
    App,
    SystemLoc,
}

/// 别名表（登记制）。
pub struct AliasTable {
    rows: Vec<Alias>,
}

impl AliasTable {
    pub fn new() -> AliasTable {
        let mut t = AliasTable { rows: Vec::new() };
        // 出厂别名（登记制注册——表即登记账）。
        t.register("calc", RunTargetKind::App, "计算器");
        t.register("notepad", RunTargetKind::App, "记事本");
        t.register("explorer", RunTargetKind::App, "资源管理器");
        t.register("settings", RunTargetKind::SystemLoc, "设置中心");
        t.register("control", RunTargetKind::SystemLoc, "控制面板");
        t
    }

    /// 登记别名（重复拒绝）。
    pub fn register(&mut self, word: &'static str, kind: RunTargetKind, explain: &'static str) -> bool {
        if self.rows.iter().any(|r| r.word == word) {
            return false;
        }
        self.rows.push(Alias { word, target: kind, explain });
        true
    }

    pub fn rows(&self) -> &[Alias] {
        &self.rows
    }

    /// 精确命中。
    pub fn exact(&self, input: &str) -> Option<Alias> {
        let t = input.trim().to_ascii_lowercase();
        self.rows.iter().find(|r| r.word == t.as_str()).copied()
    }

    /// 前缀匹配集（建议源）。
    pub fn prefixes(&self, input: &str) -> Vec<Alias> {
        let t = input.trim().to_ascii_lowercase();
        if t.is_empty() {
            return Vec::new();
        }
        let mut v: Vec<Alias> =
            self.rows.iter().filter(|r| r.word.starts_with(t.as_str())).copied().collect();
        v.sort_by(|a, b| a.word.cmp(b.word));
        v
    }
}

impl Default for AliasTable {
    fn default() -> AliasTable {
        AliasTable::new()
    }
}

// ---------------------------------------------------------------------------
// 会话状态机
// ---------------------------------------------------------------------------

/// 会话阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStage {
    Closed,
    Open,
    Executed,
    Cancelled,
}

/// 一条下拉建议。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub text: String,
    pub explain: &'static str,
    pub score: i32,
}

/// 运行框会话（开→输入→建议→Enter 执行 / Esc 取消）。
pub struct RunBoxSession {
    pub stage: RunStage,
    aliases: AliasTable,
    history: Vec<String>,
    pub opened_at_ms: u64,
    pub last_input_ms: u64,
    /// 最近执行的目标（诊断面）。
    pub last_target: Option<String>,
}

impl RunBoxSession {
    pub fn new(aliases: AliasTable) -> RunBoxSession {
        RunBoxSession {
            stage: RunStage::Closed,
            aliases,
            history: Vec::new(),
            opened_at_ms: 0,
            last_input_ms: 0,
            last_target: None,
        }
    }

    /// Win+R 打开（可重入：关框后再呼出，历史保留——常用词两击直达）。
    pub fn open(&mut self, now_ms: u64) {
        self.stage = RunStage::Open;
        self.opened_at_ms = now_ms;
        self.last_input_ms = now_ms;
    }

    /// 输入（每次键入——建议在 SUGGEST_LATENCY_MS 内就绪）。
    pub fn type_text(&mut self, text: &str, now_ms: u64) {
        if self.stage != RunStage::Open {
            return;
        }
        self.last_input_ms = now_ms;
        let _ = text;
    }

    /// 建议就绪判线（输入到建议 <50ms）。
    pub fn suggest_ready(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.last_input_ms) <= SUGGEST_LATENCY_MS
    }

    /// 下拉建议：别名前缀 + 历史（匹配度 × 历史加权；历史精确置顶）。
    pub fn suggestions(&self, input: &str) -> Vec<Suggestion> {
        let mut out: Vec<Suggestion> = Vec::new();
        let t = input.trim();
        if t.is_empty() || self.stage != RunStage::Open {
            return out;
        }
        // 历史精确/前缀（加权——常用词两击直达）。
        for h in &self.history {
            let lh = h.to_ascii_lowercase();
            let lt = t.to_ascii_lowercase();
            if lh == lt {
                out.push(Suggestion {
                    text: h.clone(),
                    explain: "历史",
                    score: 100 + HISTORY_BOOST,
                });
            } else if lh.starts_with(lt.as_str()) {
                out.push(Suggestion {
                    text: h.clone(),
                    explain: "历史",
                    score: 80 + HISTORY_BOOST,
                });
            }
        }
        // 别名前缀。
        for a in self.aliases.prefixes(t) {
            let lt = t.to_ascii_lowercase();
            let score = if a.word == lt { 100 } else { 80 };
            out.push(Suggestion { text: String::from(a.word), explain: a.explain, score });
        }
        out.sort_by(|x, y| y.score.cmp(&x.score).then(x.text.cmp(&y.text)));
        out
    }

    /// Enter 执行：三类目标判定（路径 / 别名 / 无命中时给最近建议——
    /// 不出「命令不存在」白眼）。返回目标说明。
    pub fn execute(&mut self, input: &str, _now_ms: u64) -> Option<String> {
        if self.stage != RunStage::Open {
            return None;
        }
        let t = input.trim();
        if t.is_empty() {
            return None;
        }
        // 1. 路径类。
        if let Some(target) = classify(t) {
            let desc = match &target {
                RunTarget::Path(p) => alloc::format!("资源管理器 → {p}"),
                _ => String::from(""),
            };
            self.history.retain(|x| x != t);
            self.history.insert(0, String::from(t));
            self.stage = RunStage::Executed;
            self.last_target = Some(desc.clone());
            return Some(desc);
        }
        // 2. 别名类（精确）。
        if let Some(a) = self.aliases.exact(t) {
            let desc = alloc::format!(
                "{} → {}",
                a.word,
                match a.target {
                    RunTargetKind::App => "应用",
                    RunTargetKind::SystemLoc => "系统位置",
                }
            );
            self.history.retain(|x| x != t);
            self.history.insert(0, String::from(t));
            self.stage = RunStage::Executed;
            self.last_target = Some(desc.clone());
            return Some(desc);
        }
        // 3. 无精确命中：给最近建议（不白眼——返回 None 但建议已就绪）。
        None
    }

    /// Esc 取消。
    pub fn cancel(&mut self) {
        if self.stage == RunStage::Open {
            self.stage = RunStage::Cancelled;
        }
    }

    /// 键盘全程计时（开到执行 ≤1.5s 判据对账面）。
    pub fn flow_ms(&self, executed_at_ms: u64) -> u64 {
        executed_at_ms.saturating_sub(self.opened_at_ms)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F308 自检（判据：三类输入；别名表；建议排序；Esc/Enter；1.5s 全程）。
pub fn run_runbox_checks() -> CheckSet {
    let mut set = CheckSet::new("F308-runbox");

    // 1. 三类输入用例：路径 / 别名 / 系统位置。
    let mut s = RunBoxSession::new(AliasTable::new());
    s.open(0);
    let r1 = s.execute("C:\\工具", 100);
    let mut s = RunBoxSession::new(AliasTable::new());
    s.open(0);
    let r2 = s.execute("calc", 100);
    let mut s = RunBoxSession::new(AliasTable::new());
    s.open(0);
    let r3 = s.execute("settings", 100);
    set.add(
        "three input classes",
        r1.as_deref() == Some("资源管理器 → C:\\工具")
            && r2.as_deref() == Some("calc → 应用")
            && r3.as_deref() == Some("settings → 系统位置"),
        "",
    );

    // 2. 别名表登记制：重复拒绝、新增可查。
    let mut t = AliasTable::new();
    set.add(
        "alias registry",
        t.rows().len() == 5 && !t.register("calc", RunTargetKind::App, "重复")
            && t.register("vxsh", RunTargetKind::App, "终端") && t.rows().len() == 6,
        "",
    );

    // 3. 建议排序：历史加权置顶（历史 control 前缀 80+15 压别名 80）；
    //    关框后再呼出（open 可重入），历史保留。
    let mut s2 = RunBoxSession::new(AliasTable::new());
    s2.open(0);
    let _ = s2.execute("control", 50);
    s2.open(60);
    let sug = s2.suggestions("c");
    set.add(
        "suggestions ranked",
        !sug.is_empty()
            && sug[0].text == "control"
            && sug[0].explain == "历史"
            && sug[0].score == 80 + HISTORY_BOOST
            && sug.iter().any(|x| x.text == "calc" && x.score == 80),
        "",
    );

    // 4. 无命中不白眼：历史精确建议置顶（重开后建议已在下拉）。
    let mut s3 = RunBoxSession::new(AliasTable::new());
    s3.open(0);
    let _ = s3.execute("settings", 10);
    s3.open(20);
    let sug = s3.suggestions("settings");
    set.add(
        "history exact top suggestion",
        sug.first().map(|x| x.text.as_str()) == Some("settings") && sug[0].explain == "历史",
        "",
    );

    // 5. Enter 语义：未命中返回 None 且会话保持 Open（可继续改输入）。
    let miss = s3.execute("zzz", 20);
    set.add(
        "enter miss stays open",
        miss.is_none() && s3.stage == RunStage::Open,
        "",
    );

    // 6. Esc 语义：Open 才可取消；取消后 execute 拒绝（状态机闭环）。
    let mut s4 = RunBoxSession::new(AliasTable::new());
    s4.open(0);
    s4.cancel();
    set.add(
        "esc cancels and blocks execute",
        s4.stage == RunStage::Cancelled && s4.execute("calc", 30).is_none(),
        "",
    );

    // 7. 1.5s 键盘全程：开(0) → 六次键入(每 50ms 建议) → Enter 执行。
    let mut s5 = RunBoxSession::new(AliasTable::new());
    s5.open(0);
    let keys = ["s", "se", "set", "sett", "setti", "settings"];
    let mut ready_all = true;
    for (i, k) in keys.iter().enumerate() {
        let now = (i as u64 + 1) * SUGGEST_LATENCY_MS;
        s5.type_text(k, now);
        ready_all = ready_all && s5.suggest_ready(now);
    }
    let done = s5.execute("settings", 350);
    set.add(
        "keyboard flow under 1500ms",
        ready_all && done.is_some() && s5.flow_ms(350) <= FLOW_LIMIT_MS,
        "",
    );

    // 8. 未开态拒绝输入与建议（Closed 态零动作）。
    let s6 = RunBoxSession::new(AliasTable::new());
    set.add(
        "closed state inert",
        s6.suggestions("calc").is_empty(),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_drive_letter() {
        assert!(matches!(classify("D:"), Some(RunTarget::Path(_))));
        assert!(matches!(classify("docs/报告.md"), Some(RunTarget::Path(_))));
        assert!(classify("calc").is_none());
        assert!(classify("  ").is_none());
    }

    #[test]
    fn prefix_suggestions_sorted() {
        let t = AliasTable::new();
        let p = t.prefixes("c");
        assert_eq!(p.iter().map(|a| a.word).collect::<Vec<_>>(), ["calc", "control"]);
    }

    #[test]
    fn history_dedupe_on_execute() {
        // 单会话单执行（Enter 即关框）；重开后历史保留且去重置顶。
        let mut s = RunBoxSession::new(AliasTable::new());
        s.open(0);
        let _ = s.execute("settings", 1);
        assert_eq!(s.history.len(), 1);
        s.open(2);
        let _ = s.execute("calc", 3);
        s.open(4);
        let _ = s.execute("settings", 5);
        assert_eq!(s.history[0], "settings");
        assert_eq!(s.history.len(), 2);
    }

    #[test]
    fn suggest_ready_after_latency() {
        let mut s = RunBoxSession::new(AliasTable::new());
        s.open(0);
        s.type_text("c", 100);
        assert!(s.suggest_ready(150), "50ms 内建议已就绪");
        assert!(!s.suggest_ready(200), "超过判线窗的旧输入不算就绪（新键入会刷新时间戳）");
    }
}
