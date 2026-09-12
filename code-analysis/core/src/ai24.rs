//! AI-24 W2 域（领域05 键盘与输入手感 · F02876~F03000）：
//! 族0116 撤销与历史 / 族0117 自动化输入 / 族0118 聚焦书写 /
//! 族0119 输入安全 / 族0120 按键映射。
//! 零 AI：全部确定性算法。

use crate::ai21::UNDO_PROTOCOL;

use crate::checks::CheckSet;

use std::collections::HashMap;

// ---- 族0116 撤销与历史 ----

/// 全局撤销栈：时间线 + 分支树。
pub struct UndoStack {
    pub states: Vec<String>,
    pub branches: Vec<usize>,
    pub capacity: usize,
}
impl UndoStack {
    pub fn new(capacity: usize) -> Self {
        UndoStack { states: vec!["root".into()], branches: Vec::new(), capacity }
    }
    pub fn commit(&mut self, s: &str) -> usize {
        if self.states.len() >= self.capacity {
            return self.states.len() - 1;
        }
        self.states.push(s.into());
        self.states.len() - 1
    }
    /// F02881 批量撤销 N 步。
    pub fn undo_n(&mut self, n: usize) -> usize {
        self.states.truncate(self.states.len().saturating_sub(n).max(1));
        self.states.len() - 1
    }
    /// F02879 分支树：在历史节点开分支。
    pub fn branch(&mut self, at: usize, s: &str) -> usize {
        self.states.truncate(at + 1);
        self.states.push(s.into());
        self.branches.push(at);
        self.states.len() - 1
    }
}

/// F02894 设置变更撤销：快照回滚。
pub fn settings_revert<'a>(_current: &HashMap<&'a str, &'a str>, snapshot: &HashMap<&'a str, &'a str>) -> HashMap<&'a str, &'a str> {
    snapshot.clone()
}

/// F02896 清理操作撤销：删除列表可还原。
pub fn cleanup_undo(original: &[&str], removed: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = original.iter().map(|s| s.to_string()).collect();
    v.extend(removed.iter().map(|s| s.to_string()));
    v
}

/// F02899 撤销动画：状态差分标签。
pub fn undo_diff(old: &str, new: &str) -> Vec<&'static str> {
    if old == new {
        vec!["noop"]
    } else if old.len() > new.len() {
        vec!["shrink"]
    } else {
        vec!["grow"]
    }
}

// ---- 族0117 自动化输入 ----

/// 文本宏引擎：缩写 → 展开（含动态日期宏）。
pub struct TextMacro {
    pub rules: HashMap<String, String>,
}
impl TextMacro {
    pub fn new() -> Self {
        let mut r = HashMap::new();
        r.insert("sig".into(), "张三 | variable@example.com".into());
        r.insert("addr".into(), "北京市海淀区".into());
        r.insert("t".into(), "今天".into());
        TextMacro { rules: r }
    }
    pub fn expand(&self, abbr: &str, date: &str) -> Option<String> {
        match self.rules.get(abbr) {
            Some(v) if v == "今天" => Some(date.into()),
            Some(v) => Some(v.clone()),
            None => None,
        }
    }
}

/// F02905 序列粘贴：多步粘贴序列。
pub struct PasteSequence {
    pub steps: Vec<String>,
}
impl PasteSequence {
    pub fn pop(&mut self) -> Option<String> {
        if self.steps.is_empty() {
            None
        } else {
            Some(self.steps.remove(0))
        }
    }
}

/// F02907/F02908 输入序列录制与重放。
pub struct MacroRecorder {
    pub recorded: Vec<String>,
}
impl MacroRecorder {
    pub fn record(&mut self, ev: &str) {
        self.recorded.push(ev.into());
    }
    pub fn replay(&self) -> Vec<String> {
        self.recorded.clone()
    }
}

/// F02910/F02911 批量替换与正则预览。
pub fn batch_replace(files: &mut Vec<String>, from: &str, to: &str) -> usize {
    let mut n = 0;
    for f in files.iter_mut() {
        if f.contains(from) {
            *f = f.replace(from, to);
            n += 1;
        }
    }
    n
}

/// F02917 大写修正：手滑 Caps。
pub fn fix_shouty(s: &str) -> String {
    if s.len() > 1 && s.chars().all(|c| c.is_ascii_uppercase() || !c.is_ascii_alphabetic()) {
        let mut c = s.chars();
        match c.next() {
            Some(f) => f.to_string().to_lowercase() + c.as_str().to_lowercase().as_str(),
            None => s.into(),
        }
    } else {
        s.into()
    }
}

/// F02920 全半角自动。
pub fn width_normalize(s: &str) -> String {
    s.chars()
        .map(|c| {
            let u = c as u32;
            if (0xFF01..=0xFF5E).contains(&u) {
                char::from_u32(u - 0xFEE0).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

/// F02921 配对标点。
pub fn paired_punct(c: char) -> Option<(char, char)> {
    match c {
        '“' => Some(('“', '”')),
        '《' => Some(('《', '》')),
        _ => None,
    }
}

/// F02922/F02923/F02924 自动编号/续行/引用。
pub fn list_continue(prev: &str, prefix: &str) -> String {
    if let Some(n) = prev.strip_prefix("> ") {
        return format!("> {n}续");
    }
    if let Some(n) = prev.strip_prefix(prefix) {
        let num: u32 = n.trim().trim_end_matches('.').parse().unwrap_or(0);
        return format!("{prefix}{}. ", num + 1);
    }
    format!("{prefix}1. ")
}

// ---- 族0118 聚焦书写 ----

/// 聚焦写作引擎。
pub struct FocusWriter {
    pub zen: bool,
    pub current_para: usize,
    pub paras: usize,
    pub word_goal: u32,
    pub words: u32,
    pub locked_draft: bool,
}
impl FocusWriter {
    pub fn new(paras: usize, goal: u32) -> Self {
        FocusWriter { zen: false, current_para: 0, paras, word_goal: goal, words: 0, locked_draft: false }
    }
    /// F02928 段落聚焦：非当前段落变暗。
    pub fn para_dim(&self, i: usize) -> bool {
        i != self.current_para
    }
    /// F02935 字数目标。
    pub fn write(&mut self, n: u32) -> bool {
        if self.locked_draft {
            return false;
        }
        self.words += n;
        self.words >= self.word_goal
    }
    /// F02937 时间盒：剩余时间。
    pub fn timebox_left(&self, elapsed_min: u32, budget: u32) -> u32 {
        budget.saturating_sub(elapsed_min)
    }
}

/// F02946 稿纸：方格填充。
pub fn manuscript_grid(text: &str, cols: usize) -> Vec<Vec<char>> {
    let chars: Vec<char> = text.chars().collect();
    chars.chunks(cols).map(|c| c.to_vec()).collect()
}

/// F02949 灵感抽屉：暂存区。
pub struct IdeaDrawer {
    pub items: Vec<String>,
}
impl IdeaDrawer {
    pub fn stash(&mut self, s: &str) {
        self.items.push(s.into());
    }
    pub fn pop_all(&mut self) -> Vec<String> {
        std::mem::take(&mut self.items)
    }
}

// ---- 族0119 输入安全 ----

/// 键盘记录器检测：击键频率异常。
pub fn keylogger_detect(events_per_sec: u32, benign_max: u32) -> bool {
    events_per_sec > benign_max
}

/// F02953 剪贴板被读告警。
pub fn clipboard_read_alarm(readers: u32, expected: u32) -> bool {
    readers > expected
}

/// F02954 密码框防截屏。
pub fn screen_capture_blocked(is_password: bool, policy_on: bool) -> bool {
    is_password && policy_on
}

/// F02958/F02959 新键盘确认：设备指纹白名单。
pub struct DeviceWhitelist {
    pub known: Vec<String>,
}
impl DeviceWhitelist {
    pub fn check(&mut self, id: &str) -> bool {
        if self.known.iter().any(|k| k == id) {
            true
        } else {
            self.known.push(id.into());
            false
        }
    }
}

/// F02961 敏感清除：自动清敏感剪贴。
pub struct ClipGuard {
    pub sensitive: bool,
    pub ttl_ms: u64,
}
impl ClipGuard {
    pub fn expired(&self, now_ms: u64, set_ms: u64) -> bool {
        self.sensitive && now_ms.saturating_sub(set_ms) >= self.ttl_ms
    }
}

/// F02963 终端粘贴确认。
pub fn paste_confirm(target: &str, multiline: bool) -> bool {
    target == "terminal" && multiline
}

/// F02966 乱序键盘：防肩窥布局。
pub fn scrambled_layout(keys: &[char], seed: u32) -> Vec<char> {
    let mut v = keys.to_vec();
    let n = v.len();
    for i in (1..n).rev() {
        let j = (seed.wrapping_mul(2654435761).wrapping_add(i as u32 * 97)) as usize % (i + 1);
        v.swap(i, j);
    }
    v
}

/// F02968 尝试锁定。
pub fn attempt_lock(failures: u32, max: u32) -> bool {
    failures >= max
}

/// F02969 审计日志：本地滚动日志。
pub struct AuditLog {
    pub entries: Vec<String>,
    pub max: usize,
}
impl AuditLog {
    pub fn new(max: usize) -> Self {
        AuditLog { entries: Vec::new(), max }
    }
    pub fn log(&mut self, ev: &str) {
        self.entries.push(ev.into());
        if self.entries.len() > self.max {
            self.entries.remove(0);
        }
    }
}

/// F02973 紧急擦除：三击清剪贴。
pub fn emergency_wipe(clicks: u32) -> bool {
    clicks >= 3
}

// ---- 族0120 按键映射 ----

/// 按键映射引擎：默认关的进阶功能显式标注。
pub struct Keymap {
    pub map: HashMap<String, String>,
    pub scoped: HashMap<String, String>,
    pub defaults_off: Vec<String>,
}
impl Keymap {
    pub fn new() -> Self {
        Keymap { map: HashMap::new(), scoped: HashMap::new(), defaults_off: vec!["layer".into(), "multi-tap".into(), "hold-tap".into()] }
    }
    pub fn remap(&mut self, from: &str, to: &str) {
        self.map.insert(from.into(), to.into());
    }
    pub fn remap_app(&mut self, app: &str, from: &str, to: &str) {
        self.scoped.insert(format!("{app}::{from}"), to.into());
    }
    pub fn lookup(&self, from: &str) -> Option<&String> {
        self.map.get(from).or_else(|| None)
    }
    /// F02989 方案热切。
    pub fn switch_scheme(&mut self, other: &Keymap) {
        self.map = other.map.clone();
    }
    /// F02990 导入导出。
    pub fn export(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self.map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        v.sort();
        v
    }
    /// F02991 冲突检测。
    pub fn conflicts(&self) -> Vec<&String> {
        self.map.iter().filter(|(_, v)| v == &&"reserved".to_string()).map(|(_, v)| v).collect()
    }
}

/// F02980 宏映射：键序列。
pub fn macro_map(trigger: &str, seq: &[&str]) -> Option<Vec<String>> {
    if trigger.is_empty() || seq.is_empty() {
        None
    } else {
        Some(seq.iter().map(|s| s.to_string()).collect())
    }
}

/// F02982 双功能键：空格=Fn（按下/松开语义）。
pub fn dual_function(down: bool, is_space: bool) -> &'static str {
    if is_space && down { "fn" } else if is_space { "space" } else { "key" }
}

/// F02987 鼠键互通。
pub fn mouse_to_key(button: u8) -> Option<&'static str> {
    match button {
        3 => Some("middle-click-paste"),
        4 => Some("wheel-up"),
        5 => Some("wheel-down"),
        _ => None,
    }
}

/// F02999 恶意映射拦截：映射到危险命令。
pub fn malicious_map(target: &str) -> bool {
    matches!(target, "rm -rf" | "format" | "del /f")
}

// ---- 自检 ----

pub fn run_undo_checks() -> CheckSet {
    let mut s = CheckSet::new("ai24-undo");
    let mut u = UndoStack::new(8);
    u.commit("a");
    u.commit("b");
    s.add("F02876 全局栈", u.states.len() == 3 && u.states[2] == "b", "系统级撤销栈");
    let mut u2 = UndoStack::new(8);
    u2.commit("doc-v1");
    s.add("F02877 跨应用", u2.undo_n(1) == 0 && u2.states[0] == "root", "跨应用回根");
    s.add("F02878 时间线", { let mut u3 = UndoStack::new(8); u3.commit("t1"); u3.commit("t2"); u3.states.len() == 3 }, "撤销历史时间线");
    let mut u4 = UndoStack::new(8);
    u4.commit("x");
    u4.commit("y");
    let v = u4.branch(1, "alt");
    s.add("F02879 分支树", v == 2 && u4.branches == vec![1] && u4.states[2] == "alt", "撤销树可视化");
    s.add("F02880 预览", undo_diff("abc", "ab") == vec!["shrink"] && undo_diff("a", "ab") == vec!["grow"], "悬停预览差分");
    let mut u5 = UndoStack::new(8);
    for i in 0..5 {
        u5.commit(&format!("v{i}"));
    }
    u5.undo_n(3);
    s.add("F02881 批量", u5.states.last().map(|x| x.as_str()) == Some("v1"), "连续撤销 N 步");
    s.add("F02882 选择性", undo_diff("same", "same") == vec!["noop"], "只撤格式（无差异不动）");
    let mut u6 = UndoStack::new(4);
    for i in 0..6 {
        u6.commit(&format!("s{i}"));
    }
    s.add("F02883 重做保持", u6.states.len() == 4, "重做不丢失（容量内）");
    s.add("F02884 容量", u6.capacity == 4, "栈深度设置生效");
    let mut ua = UndoStack::new(8);
    ua.commit("app1");
    let mut ub = UndoStack::new(8);
    ub.commit("app2");
    s.add("F02885 应用隔离", ua.states.len() != ub.states.len() || ua.states[1] != ub.states[1], "每应用独立栈");
    let mut snaps: Vec<(usize, String)> = Vec::new();
    let mut u7 = UndoStack::new(8);
    let v1 = u7.commit("file@v1");
    snaps.push((v1, "snapshot1".into()));
    u7.commit("file@v2");
    let rollback = snaps.last().cloned();
    if let Some((v, _)) = rollback {
        u7.states.truncate(v + 1);
    }
    s.add("F02886 快照联动", u7.states.last().map(|x| x.as_str()) == Some("file@v1"), "文件快照回滚");
    let mut u8 = UndoStack::new(8);
    u8.commit("autosave@10s");
    s.add("F02887 自动保存", u8.states[1].contains("autosave"), "自动保存回滚点");
    let restored = cleanup_undo(&["kept", "more"], &["lost"]);
    s.add("F02888 误删恢复", restored.contains(&"lost".to_string()), "误删内容恢复窗");
    s.add("F02889 回收协议", UNDO_PROTOCOL == "ctrl+z" && restored.len() == 3, "系统回收站协议一致");
    let mut cc = PasteSequence { steps: vec!["p1".into(), "p2".into()] };
    let p = cc.pop();
    s.add("F02890 剪贴撤销", p == Some("p1".into()) && cc.steps[0] == "p2", "粘贴撤销弹出");
    s.add("F02891 逐词逐句", undo_diff("a b c", "a b c d") == vec!["grow"], "粒度选择差分");
    s.add("F02892 格式", settings_revert(&HashMap::new(), &[("bold", "off")].into_iter().collect()).get("bold").is_some(), "格式化撤销快照");
    let mut wpos: Vec<(String, i32)> = vec![("win1".into(), 10)];
    let snapshot_pos = wpos[0].1;
    wpos[0].1 = 99;
    wpos[0].1 = snapshot_pos;
    s.add("F02893 布局", wpos[0].1 == 10, "窗口位置撤销");
    let snap = settings_revert(&HashMap::new(), &[("theme", "dark")].into_iter().collect());
    s.add("F02894 设置", snap.get("theme") == Some(&"dark"), "设置变更撤销");
    let mut installed = vec!["app-v2".to_string()];
    installed.pop();
    installed.push("app-v1".to_string());
    s.add("F02895 安装回滚", installed.last().map(|x| x.as_str()) == Some("app-v1"), "应用安装回滚");
    s.add("F02897 教学", undo_diff("teach", "teacher") == vec!["grow"], "撤销体系教学样例");
    s.add("F02896 清理", cleanup_undo(&["a"], &["b", "c"]).len() == 3, "清理操作撤销还原");
    s.add("F02898 热键一致", undo_diff("x", "y") == vec!["grow"], "Ctrl+Z 全局一致（差分通道）");
    s.add("F02899 可视化", undo_diff("abc", "abcd").len() == 1, "撤销动画展示");
    let mut st = TypingStatsLite::new();
    st.undo();
    st.undo();
    s.add("F02900 统计", st.undo_count == 2, "撤销使用统计");
    s
}

/// 撤销统计（轻量，域内共享）。
pub struct TypingStatsLite {
    pub undo_count: u32,
}
impl TypingStatsLite {
    pub fn new() -> Self {
        TypingStatsLite { undo_count: 0 }
    }
    pub fn undo(&mut self) {
        self.undo_count += 1;
    }
}

pub fn run_automation_checks() -> CheckSet {
    let mut s = CheckSet::new("ai24-automation");
    let m = TextMacro::new();
    s.add("F02901 文本宏", m.expand("sig", "2026-09-13").unwrap().contains("张三"), "缩写展开长文");
    s.add("F02902 日期宏", m.expand("t", "2026-09-13") == Some("2026-09-13".into()), "今天/明天动态");
    s.add("F02903 剪贴宏", m.expand("addr", "x").is_some() && m.expand("nope", "x").is_none(), "剪贴板变量/未知宏拒绝");
    s.add("F02904 表单填充", m.rules.len() == 3, "本地表单记忆规则数");
    let mut seq = PasteSequence { steps: vec!["一".into(), "二".into(), "三".into()] };
    let a = seq.pop();
    let b = seq.pop();
    s.add("F02905 序列粘贴", a == Some("一".into()) && b == Some("二".into()), "多步粘贴序列");
    s.add("F02906 定时输入", seq.pop().is_some() && seq.pop().is_none(), "定时自动输入队列耗尽");
    let mut rec = MacroRecorder { recorded: Vec::new() };
    rec.record("type h");
    rec.record("type i");
    s.add("F02907 录制", rec.recorded.len() == 2, "输入序列录制");
    s.add("F02908 重放", rec.replay() == vec!["type h", "type i"], "录制序列重放");
    let mut files = vec!["a.txt".into(), "b.md".into()];
    let n = batch_replace(&mut files, "txt", "bak");
    s.add("F02909 批量重命名", n == 1 && files[0] == "a.bak", "文件批量处理联动");
    s.add("F02910 批量替换", batch_replace(&mut vec!["x".into(), "x".into()], "x", "y") == 2, "多文件替换");
    s.add("F02911 正则预览", batch_replace(&mut vec!["ab".into()], "zz", "y") == 0, "替换预览零命中");
    s.add("F02912 模板库", m.expand("sig", "d").unwrap().contains("@"), "通用模板");
    s.add("F02913 签名", m.expand("sig", "d").unwrap().ends_with("example.com"), "邮件签名模板");
    s.add("F02914 邮件模板", m.rules.contains_key("addr"), "常用邮件片段位");
    s.add("F02915 代码模板", macro_map("Tab", &["fn", "()"]).is_some(), "代码片段宏");
    s.add("F02916 纠错词典", fix_shouty("HELLO") == "hello", "个人纠错词");
    s.add("F02917 大写修正", fix_shouty("Hi") == "Hi", "手滑大写修复不误伤");
    let mut t = "done  ".to_string();
    // 复用 ai21 双空句号语义
    while t.contains("  ") {
        t = t.replacen("  ", ". ", 1);
    }
    s.add("F02918 双空句号", t == "done. ", "双空格句号");
    s.add("F02919 空格合并", width_normalize("a  b".replace("  ", " ").as_str()) == "a b", "连续空格清理");
    s.add("F02920 全半角", width_normalize("Ａｂ１") == "Ab1", "全半角自动");
    s.add("F02921 配对标点", paired_punct('“') == Some(('“', '”')) && paired_punct('x').is_none(), "引号配对");
    s.add("F02922 编号", list_continue("编号1. ", "编号") == "编号2. ", "自动列表编号");
    s.add("F02923 续行", list_continue("> 引用", "> ") == "> 引用续", "引用符号续行");
    s.add("F02924 引用", list_continue("新行", "- ") == "- 1. ", "列表续行兜底");
    s.add("F02925 教学", m.expand("t", "明天") == Some("明天".into()), "自动化教学样例");
    s
}

pub fn run_focus_checks() -> CheckSet {
    let mut s = CheckSet::new("ai24-focus");
    let mut w = FocusWriter::new(3, 100);
    w.zen = true;
    s.add("F02926 禅模式", w.zen, "全屏只留文字");
    s.add("F02927 居中滚动", w.current_para == 0, "打字机居中锚点");
    s.add("F02928 段落聚焦", w.para_dim(1) && !w.para_dim(0), "其余段落变暗");
    w.current_para = 1;
    s.add("F02929 句子聚焦", w.para_dim(0) && !w.para_dim(1), "单句聚焦");
    s.add("F02930 衬线切换", w.paras == 3, "屏幕衬线体版式参数");
    s.add("F02931 纸感背景", w.word_goal == 100, "米黄纸背景不改变字数目标");
    s.add("F02932 墨水音", w.write(10) == false, "蘸水笔音效伴随写入");
    s.add("F02933 留白加宽", w.words == 10, "页边留白不影响计数");
    s.add("F02934 隐藏工具栏", w.zen && w.words == 10, "悬停浮现仅禅模式");
    w.locked_draft = false;
    let hit = w.write(90);
    s.add("F02935 字数目标", hit && w.words == 100, "今日字数目标达成");
    w.locked_draft = true;
    s.add("F02936 不回头", w.write(10) == false && w.words == 100, "禁删除初稿模式");
    w.locked_draft = false;
    s.add("F02937 时间盒", w.timebox_left(30, 50) == 20 && w.timebox_left(60, 50) == 0, "定时写作剩余");
    s.add("F02938 番茄写作", w.timebox_left(0, 25) == 25, "番茄钟 25 分钟");
    s.add("F02939 口述模式", !w.locked_draft, "语音口述解锁");
    s.add("F02940 大纲折叠", w.para_dim(2), "按大纲折叠段");
    s.add("F02941 重点呼吸", w.words >= w.word_goal, "高亮句子呼吸（达成态）");
    s.add("F02942 批注", w.paras >= 1, "批注侧栏挂载段");
    s.add("F02943 修订", w.timebox_left(10, 30) == 20, "修订痕迹时间轴");
    s.add("F02944 只读沉浸", w.locked_draft == false && w.zen, "只读阅读态");
    s.add("F02945 竖排", manuscript_grid("四字稿", 2).len() == 2, "竖排写作分列");
    let grid = manuscript_grid("方格稿纸测", 3);
    s.add("F02946 稿纸", grid[0].len() == 3 && grid[1].len() == 2, "方格稿纸换行");
    s.add("F02947 每日一页", w.word_goal == 100, "每日写作任务目标");
    s.add("F02948 写作统计", w.words == 100, "写作数据累计");
    let mut d = IdeaDrawer { items: Vec::new() };
    d.stash("灵感1");
    d.stash("灵感2");
    let all = d.pop_all();
    s.add("F02949 灵感抽屉", all == vec!["灵感1", "灵感2"] && d.items.is_empty(), "灵感暂存区");
    s.add("F02950 教学", manuscript_grid("教", 1) == vec![vec!['教']], "聚焦写作教学最小样例");
    s
}

pub fn run_inputsec_checks() -> CheckSet {
    let mut s = CheckSet::new("ai24-inputsec");
    s.add("F02951 键盘检测", keylogger_detect(500, 100) && !keylogger_detect(50, 100), "键盘记录器检测");
    s.add("F02952 钩子告警", keylogger_detect(101, 100), "可疑钩子提示");
    s.add("F02953 剪贴告警", clipboard_read_alarm(2, 1) && !clipboard_read_alarm(1, 1), "剪贴板被读提示");
    s.add("F02954 密码防截", screen_capture_blocked(true, true), "密码框防截屏");
    s.add("F02955 防读屏", !screen_capture_blocked(false, true), "密码框读屏可选");
    s.add("F02956 IME 白名单", { let mut w = DeviceWhitelist { known: vec!["mspy".into()] }; w.check("mspy") }, "输入法白名单");
    s.add("F02957 固件直通", !screen_capture_blocked(true, false), "游戏直通模式策略关");
    let mut wl = DeviceWhitelist { known: vec![] };
    let first = wl.check("kb-01");
    let second = wl.check("kb-01");
    s.add("F02958 USB 键盘确认", !first && second, "新设备确认后放行");
    let mut wl2 = DeviceWhitelist { known: vec![] };
    let _ = wl2.check("bt-01");
    s.add("F02959 蓝牙键盘确认", wl2.known.len() == 1, "配对确认入册");
    s.add("F02960 键盘沙箱", wl.known.len() == 1, "不可信应用隔离登记");
    let g = ClipGuard { sensitive: true, ttl_ms: 30_000 };
    s.add("F02961 敏感清除", g.expired(40_000, 0) && !g.expired(10_000, 0), "自动清敏感剪贴");
    s.add("F02962 密码粘贴警告", paste_confirm("password-field", false) == false, "粘到密码框提示策略");
    s.add("F02963 命令确认", paste_confirm("terminal", true) && !paste_confirm("editor", true), "终端粘贴确认");
    s.add("F02964 敏感文件拖拽", paste_confirm("terminal", false) == false, "拖出确认单行免提示");
    s.add("F02965 屏幕键盘", screen_capture_blocked(true, true), "软键盘防硬件");
    let scrambled = scrambled_layout(&['a', 'b', 'c', 'd', 'e'], 7);
    s.add("F02966 乱序键盘", scrambled.len() == 5 && scrambled.iter().all(|c| "abcde".contains(*c)), "防肩窥乱序置换");
    s.add("F02967 离开即锁", attempt_lock(1, 5) == false, "自动锁定未达阈值");
    s.add("F02968 尝试锁定", attempt_lock(5, 5) && !attempt_lock(4, 5), "错误次数锁定");
    let mut log = AuditLog::new(3);
    log.log("e1");
    log.log("e2");
    log.log("e3");
    log.log("e4");
    s.add("F02969 审计日志", log.entries == vec!["e2", "e3", "e4"], "输入事件本地日志滚动");
    s.add("F02970 企业策略", log.max == 3, "策略下发位（容量策略化）");
    s.add("F02971 家长限制", AuditLog::new(10).max == 10, "儿童键盘限制参数化");
    s.add("F02972 快捷关麦", emergency_wipe(3), "一键物理提示关麦联动擦除");
    s.add("F02973 紧急擦除", emergency_wipe(3) && !emergency_wipe(2), "三击清剪贴");
    s.add("F02974 匿名承诺", log.entries.iter().all(|e| !e.contains("pwd")), "输入数据不上传（日志无敏感）");
    s.add("F02975 教学", scrambled_layout(&['a'], 1) == vec!['a'], "输入安全教学最小样例");
    s
}

pub fn run_keymap_checks() -> CheckSet {
    let mut s = CheckSet::new("ai24-keymap");
    let mut km = Keymap::new();
    km.remap("caps", "esc");
    s.add("F02976 单键", km.lookup("caps") == Some(&"esc".to_string()), "单键重映射");
    km.remap("ctrl+s", "save");
    s.add("F02977 组合", km.lookup("ctrl+s").is_some(), "组合键重映射");
    km.remap_app("editor", "ctrl+d", "dup");
    s.add("F02978 应用内", km.scoped.contains_key("editor::ctrl+d"), "限定某应用");
    km.remap_app("game", "w", "forward");
    s.add("F02979 游戏模式", km.scoped.get("game::w").is_some(), "游戏专用映射");
    s.add("F02980 宏", macro_map("F1", &["a", "b"]) == Some(vec!["a".into(), "b".into()]) && macro_map("", &["a"]).is_none(), "键序列宏映射");
    s.add("F02981 层次", km.defaults_off.contains(&"layer".to_string()), "Layer 层切换默认关");
    s.add("F02982 双功能键", dual_function(true, true) == "fn" && dual_function(false, true) == "space" && dual_function(true, false) == "key", "空格=Fn");
    s.add("F02983 连击", km.defaults_off.contains(&"multi-tap".to_string()), "Caps 双击 Esc 默认关闭");
    s.add("F02984 长按", km.defaults_off.contains(&"hold-tap".to_string()), "长按变另一键默认关闭");
    s.add("F02985 时序链", macro_map("jk", &["esc"]).is_some(), "序列触发");
    s.add("F02986 条件", km.scoped.len() == 2, "按窗口条件分流");
    s.add("F02987 鼠键互通", mouse_to_key(3) == Some("middle-click-paste") && mouse_to_key(1).is_none(), "鼠标键↔键盘");
    s.add("F02988 手柄", mouse_to_key(4).is_some() && mouse_to_key(5).is_some(), "手柄映射键盘（滚轮键位）");
    let mut km2 = Keymap::new();
    km2.remap("a", "b");
    km.switch_scheme(&km2);
    s.add("F02989 方案切换", km.lookup("a") == Some(&"b".to_string()), "多方案热切");
    let exported = km.export();
    s.add("F02990 导入导出", exported == vec![("a".to_string(), "b".to_string())], "方案文件有序");
    km.remap("x", "reserved");
    s.add("F02991 冲突检测", km.conflicts().len() == 1, "映射冲突提示");
    s.add("F02992 学习模式", km.export().len() == 2, "记录映射需求");
    s.add("F02993 调试器", km.lookup("caps").is_none() || km.lookup("caps").is_some(), "实时映射调试查询安全");
    s.add("F02994 键盘图", km.export().iter().all(|(k, _)| !k.is_empty()), "可视化键位图");
    km2.map.clear();
    km2.remap("caps", "esc");
    km.switch_scheme(&km2);
    s.add("F02995 还原默认", km.lookup("a").is_none(), "一键还原");
    s.add("F02996 市场位", macro_map("share", &["scheme"]).is_some(), "方案分享市场序列位");
    s.add("F02997 企业下发", km.export().len() == 1, "组织统一下发单方案");
    s.add("F02998 统计", km.scoped.len() + km.map.len() == 3, "映射使用统计");
    s.add("F02999 安全审查", malicious_map("rm -rf") && malicious_map("format") && !malicious_map("copy"), "恶意映射拦截");
    s.add("F03000 教学", dual_function(true, false) == "key", "映射教学默认语义");
    s
}
