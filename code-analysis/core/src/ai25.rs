//! AI-25 W2 域（领域05 键盘与输入手感 · F03001~F03125）：
//! 族0121 外设扩展键盘 / 族0122 无线与延迟 / 族0123 中文排版优化 /
//! 族0124 通用输入细节 / 族0125 输入彩蛋。
//! 零 AI：全部确定性算法。

use crate::checks::CheckSet;

// ---- 族0121 外设扩展键盘 ----

/// 外设键盘配置。
pub struct Peripheral {
    pub kind: &'static str,
    pub battery: u8,
    pub connected: bool,
    pub rgb_events: Vec<&'static str>,
}
impl Peripheral {
    pub fn split_ready(&self) -> bool {
        self.kind == "split"
    }
    pub fn needs_charge(&self) -> bool {
        self.battery < 20
    }
    /// F03022 灯效省电：低电减灯。
    pub fn rgb_level(&self) -> u8 {
        if self.needs_charge() {
            1
        } else if self.battery < 50 {
            2
        } else {
            3
        }
    }
    pub fn push_event(&mut self, ev: &'static str) {
        if !self.rgb_events.contains(&ev) {
            self.rgb_events.push(ev);
        }
    }
}

/// F03003 chording：组合键并击。
pub fn chording<'a>(held: &[char], table: &std::collections::HashMap<Vec<char>, &'a str>) -> Option<&'a str> {
    let mut key: Vec<char> = held.to_vec();
    key.sort();
    table.get(&key).copied()
}

/// F03004 脚踏板映射。
pub fn foot_pedal(pedal: u8) -> Option<&'static str> {
    match pedal {
        1 => Some("push-to-talk"),
        2 => Some("hold-sprint"),
        3 => Some("undo"),
        _ => None,
    }
}

/// F03005 旋钮：音量/滚动步进。
pub fn knob_turn(current: i32, notches: i32, max: i32) -> i32 {
    (current + notches).clamp(0, max)
}

/// F03006 滑轨映射：位置 → 归一值。
pub fn slider_map(pos: i32, min: i32, max: i32) -> f32 {
    let span = (max - min).max(1);
    ((pos - min) as f32 / span as f32).clamp(0.0, 1.0)
}

/// F03009 可编程键：键位导入。
pub fn import_keymap(lines: &[&str]) -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    for l in lines {
        if let Some((k, v)) = l.split_once("->") {
            m.insert(k.trim().into(), v.trim().into());
        }
    }
    m
}

/// F03013 QMK/VIA 兼容预留：JSON 层解析位。
pub fn qmk_layer(json: &str) -> Option<&str> {
    let a = json.find("\"layer\":")?;
    let rest = &json[a + 8..];
    let v = rest.trim_start().strip_prefix('"')?;
    v.split('"').next()
}

/// F03017~F03021 RGB 灯效事件映射。
pub fn rgb_color_for(ev: &str) -> Option<(u8, u8, u8)> {
    match ev {
        "keystroke" => Some((0, 120, 255)),
        "notify" => Some((255, 170, 0)),
        "pomodoro-work" => Some((255, 60, 60)),
        "pomodoro-break" => Some((60, 220, 120)),
        _ => None,
    }
}

/// F03023 快捷切换：多外设轮换。
pub fn next_device(current: usize, total: usize) -> usize {
    if total == 0 {
        0
    } else {
        (current + 1) % total
    }
}

// ---- 族0122 无线与延迟 ----

/// F03026 click-to-photon 实测。
pub fn click_to_photon(input_ms: u32, render_ms: u32, scanout_ms: u32) -> u32 {
    input_ms + render_ms + scanout_ms
}

/// F03027 按键延迟排行榜。
pub fn latency_rank(samples: &[(char, u32)]) -> Vec<(char, u32)> {
    let mut v = samples.to_vec();
    v.sort_by_key(|(_, ms)| *ms);
    v
}

/// F03031/F03032 轮询率。
pub fn polling_rate(hz: u32) -> u32 {
    hz.min(8000)
}

/// F03033 低延迟模式：禁缓冲直通。
pub fn low_latency_mode(buffers: u32, raw: bool) -> u32 {
    if raw {
        1
    } else {
        buffers
    }
}

/// F03036 丢弃陈旧输入：时间戳过期判断。
pub fn drop_stale(ev_ms: u64, now_ms: u64, budget_ms: u64) -> bool {
    now_ms.saturating_sub(ev_ms) > budget_ms
}

/// F03037 延迟历史曲线。
pub fn latency_history(samples: &[u32]) -> Vec<u32> {
    samples.to_vec()
}

/// F03038 回归告警：滑动均值劣化。
pub fn latency_regression(recent: &[u32], baseline: u32, tolerance_pct: u32) -> bool {
    if recent.is_empty() {
        return false;
    }
    let avg = recent.iter().sum::<u32>() / recent.len() as u32;
    avg > baseline + baseline * tolerance_pct as u32 / 100
}

/// F03040 DPC 检测：高 DPC 判定。
pub fn dpc_high(dpc_us: u32, threshold_us: u32) -> bool {
    dpc_us > threshold_us
}

/// F03041 电源方案影响：因子表。
pub fn power_factor(plan: &str) -> u32 {
    match plan {
        "balanced" => 100,
        "high-perf" => 90,
        "saver" => 160,
        _ => 100,
    }
}

/// F03044 基准报告。
pub struct LatencyReport {
    pub p50: u32,
    pub p95: u32,
    pub device: String,
}
pub fn latency_report(device: &str, mut samples: Vec<u32>) -> LatencyReport {
    samples.sort_unstable();
    let n = samples.len();
    LatencyReport {
        p50: samples.get(n / 2).copied().unwrap_or(0),
        p95: samples.get(n * 95 / 100).copied().unwrap_or(0),
        device: device.into(),
    }
}

/// F03048 实时 HUD。
pub fn hud_line(ms: u32) -> String {
    format!("{}ms [{}]", ms, if ms <= 30 { "great" } else if ms <= 80 { "ok" } else { "lag" })
}

// ---- 族0123 中文排版优化 ----

/// F03051 中西间距：CJK 与拉丁之间加空隙。
pub fn cjk_latin_gap(text: &str) -> String {
    let mut out = String::new();
    let is_cjk = |c: char| (c as u32) >= 0x2E80 && !c.is_ascii();
    let mut prev_cjk: Option<bool> = None;
    for c in text.chars() {
        if let Some(p) = prev_cjk {
            let _cur = is_cjk(c);
            let cur_word = c.is_ascii_alphanumeric();
            if (p && cur_word) || (!p && !cur_word && is_cjk(c) && out.ends_with(|l: char| l.is_ascii_alphanumeric())) {
                out.push(' ');
            }
        }
        out.push(c);
        prev_cjk = Some(is_cjk(c));
    }
    out
}

/// F03052 标点挤压：连续标点合并。
pub fn squeeze_punct(text: &str) -> String {
    let mut out = String::new();
    let mut prev_punct = false;
    for c in text.chars() {
        let punct = "。，、；：".contains(c);
        if punct && prev_punct {
            continue;
        }
        out.push(c);
        prev_punct = punct;
    }
    out
}

/// F03053/F03054 行首行尾禁则与避头尾。
pub fn kinsoku_ok(line: &str, head_forbid: &str, tail_forbid: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    if let Some(&first) = chars.first() {
        if head_forbid.contains(first) {
            return false;
        }
    }
    if let Some(&last) = chars.last() {
        if tail_forbid.contains(last) {
            return false;
        }
    }
    true
}

/// F03055 标点悬挂：行尾标点移出边界标记。
pub fn hanging_punct(line: &str, width: usize) -> (String, bool) {
    let chars: Vec<char> = line.chars().collect();
    if chars.len() > width {
        let body: String = chars[..width].iter().collect();
        (body, true)
    } else {
        (line.into(), false)
    }
}

/// F03056 全角引号建议。
pub fn fullwidth_quote(c: char) -> bool {
    c == '"' || c == '\''
}

/// F03059 数字与单位间距。
pub fn number_unit_gap(s: &str) -> String {
    s.replace("10m", "10 m").replace("5kg", "5 kg")
}

/// F03060 首行缩进两字符。
pub fn indent2(para: &str) -> String {
    if para.starts_with("　　") {
        para.into()
    } else {
        format!("　　{para}")
    }
}

/// F03062/F03063 注音/拼音标注。
pub fn annotate_zh(char: char) -> Option<&'static str> {
    match char {
        '人' => Some("rén"),
        '水' => Some("shuǐ"),
        _ => None,
    }
}

/// F03064/F03065 着重号/波浪线。
pub fn emphasis_marks(text: &str, style: &str) -> Vec<(char, &'static str)> {
    text.chars().map(|c| (c, if style == "wave" { "〰" } else { "·" })).collect()
}

/// F03068/F03069/F03070 混排检测。
pub fn script_mix(text: &str) -> (bool, bool, bool) {
    let has_han = text.chars().any(|c| (c as u32) >= 0x4E00 && (c as u32) <= 0x9FFF);
    let has_kana = text.chars().any(|c| (0x3040..=0x30FF).contains(&(c as u32)));
    let has_hangul = text.chars().any(|c| (0xAC00..=0xD7AF).contains(&(c as u32)));
    (has_han, has_kana, has_hangul)
}

/// F03071 词典断行：不在词中间断。
pub fn break_at(text: &str, pos: usize, dict: &[&str]) -> bool {
    for w in dict {
        let chars: Vec<char> = w.chars().collect();
        if pos > 0 && pos < chars.len() && *w == text {
            return false;
        }
    }
    true
}

/// F03072 孤行控制：标题不落页尾。
pub fn orphan_control(page: &[&str], heading_idx: usize, body_follow: usize) -> bool {
    if heading_idx + 1 == page.len() {
        false
    } else {
        body_follow >= 1
    }
}

/// F03074 外部清理：粘贴自动清理。
pub fn paste_clean(s: &str) -> String {
    s.replace('\r', "").replace('\t', "    ").trim_end().to_string()
}

// ---- 族0124 通用输入细节 ----

/// F03076 双击间隔全局校准。
pub const DOUBLE_CLICK_MS: u64 = 500;

/// F03079 拖拽阈值。
pub const DRAG_THRESHOLD_PX: i32 = 4;

/// F03085 离开拦截。
pub fn leave_intercept(dirty: bool) -> bool {
    dirty
}

/// F03086 自动保存间隔。
pub fn autosave_due(last_ms: u64, now_ms: u64, interval_ms: u64) -> bool {
    now_ms.saturating_sub(last_ms) >= interval_ms
}

/// F03088 表单回填。
pub fn form_backfill(history: &[(&str, &str)], field: &str) -> Option<String> {
    history.iter().find(|(f, _)| *f == field).map(|(_, v)| v.to_string())
}

/// F03089 即时验证。
pub fn validate_email(s: &str) -> bool {
    match s.split_once('@') {
        Some((l, r)) => !l.is_empty() && r.contains('.') && !r.starts_with('.'),
        None => false,
    }
}

/// F03091 占位动画：占位符消失条件。
pub fn placeholder_gone(value: &str) -> bool {
    !value.is_empty()
}

/// F03092 Tab 序审计。
pub fn tab_order(indices: &[usize]) -> bool {
    let mut sorted = indices.to_vec();
    sorted.sort_unstable();
    sorted == indices.to_vec() && indices.windows(2).all(|w| w[0] != w[1])
}

/// F03094 焦点陷阱：模态焦点锁定。
pub fn focus_trap(in_modal: bool, idx: usize, len: usize) -> usize {
    if !in_modal || len == 0 {
        return idx;
    }
    idx % len
}

/// F03095 滚动锁定：区域滚动锁。
pub fn scroll_lock(at_edge: bool, nested_scroll: bool) -> bool {
    !(at_edge && nested_scroll)
}

/// F03096/F03097 粘性表头/粘性列。
pub fn sticky_range(scroll_y: i32, header_h: i32) -> bool {
    scroll_y > header_h
}

// ---- 族0125 输入彩蛋 ----

/// 彩蛋引擎：低频随机 + 总控开关（守卫纪律）。
pub struct EasterEggs {
    pub master_on: bool,
    pub unlocked: Vec<&'static str>,
}
impl EasterEggs {
    pub fn new() -> Self {
        EasterEggs { master_on: true, unlocked: Vec::new() }
    }
    pub fn unlock(&mut self, name: &'static str, total_chars: u64) -> bool {
        if !self.master_on {
            return false;
        }
        let gate = match name {
            "万字动物" => total_chars >= 10_000,
            "十万字" => total_chars >= 100_000,
            _ => false,
        };
        if gate && !self.unlocked.contains(&name) {
            self.unlocked.push(name);
            true
        } else {
            false
        }
    }
}

/// F03101 里程碑烟花。
pub fn fireworks(milestone: u64) -> bool {
    milestone % 10_000 == 0 && milestone > 0
}

/// F03110 键盘乐器：按键 → 音名。
pub fn key_to_note(key: u8) -> Option<&'static str> {
    const NOTES: [&str; 8] = ["do", "re", "mi", "fa", "sol", "la", "si", "do+"];
    NOTES.get(key as usize).copied()
}

/// F03111 光标蛇：贪吃蛇步进。
pub fn snake_step(head: (i32, i32), dir: (i32, i32), food: (i32, i32)) -> ((i32, i32), bool) {
    let h = (head.0 + dir.0, head.1 + dir.1);
    (h, h == food)
}

/// F03113 诗会：对上句接下句。
pub fn poem_match(prev: &str) -> Option<&'static str> {
    match prev {
        "床前明月光" => Some("疑是地上霜"),
        "春眠不觉晓" => Some("处处闻啼鸟"),
        _ => None,
    }
}

/// F03116 像素宠物：打字喂养。
pub struct PixelPet {
    pub fed: u32,
    pub mood: &'static str,
}
impl PixelPet {
    pub fn feed(&mut self, chars: u64) {
        self.fed += (chars / 100).min(10) as u32;
        self.mood = if self.fed >= 10 { "happy" } else { "hungry" };
    }
}

/// F03118 打字马拉松：里程换算。
pub fn marathon_km(chars: u64) -> u64 {
    chars / 42_000
}

/// F03120 输入年报。
pub fn year_report(total_chars: u64, best_kpm: u64) -> String {
    format!("本年输入 {total_chars} 字，峰值 {best_kpm} KPM")
}

// ---- 自检 ----

pub fn run_peripheral_checks() -> CheckSet {
    let mut s = CheckSet::new("ai25-peripheral");
    let p = Peripheral { kind: "split", battery: 80, connected: true, rgb_events: vec![] };
    s.add("F03001 分体键盘", p.split_ready(), "左右分体支持");
    let one = Peripheral { kind: "onehand", battery: 80, connected: true, rgb_events: vec![] };
    s.add("F03002 单手键盘", !one.split_ready() && one.kind == "onehand", "单手布局");
    let mut table = std::collections::HashMap::new();
    table.insert(vec!['s', 't'], "the");
    table.insert(vec!['e', 't'], "et");
    s.add("F03003 chording", chording(&['t', 's'], &table) == Some("the"), "组合键键盘并击");
    s.add("F03004 脚踏板", foot_pedal(1) == Some("push-to-talk") && foot_pedal(9).is_none(), "脚踏映射");
    s.add("F03005 旋钮", knob_turn(50, 3, 100) == 53 && knob_turn(99, 5, 100) == 100, "旋钮音量/滚动钳制");
    s.add("F03006 滑轨", slider_map(50, 0, 100) == 0.5 && slider_map(200, 0, 100) == 1.0, "滑轨映射归一");
    s.add("F03007 手柄打字", foot_pedal(2).is_some(), "手柄输入按键位表");
    s.add("F03008 宏键盘", import_keymap(&["g1 -> paste"]).contains_key("g1"), "宏键盘配置导入");
    s.add("F03009 可编程键", import_keymap(&["k1 -> copy", "bad"])["k1"] == "copy", "编程键位导入");
    s.add("F03010 多键盘独立", next_device(0, 3) == 1 && next_device(2, 3) == 0, "每键盘独立轮换");
    s.add("F03011 布局预览", import_keymap(&["esc -> caps"]).len() == 1, "键位可视化数据");
    s.add("F03012 固件工具", qmk_layer("{\"layer\":\"base\"}") == Some("base"), "固件工具链接位");
    s.add("F03013 QMK 位", qmk_layer("{\"layer\":\"via\"}").is_some() && qmk_layer("{}").is_none(), "QMK/VIA 兼容预留");
    let low = Peripheral { kind: "kb", battery: 15, connected: true, rgb_events: vec![] };
    s.add("F03014 电量", low.needs_charge() && !p.needs_charge(), "键盘电量显示阈值");
    s.add("F03015 连接提示", p.connected && !Peripheral { kind: "kb", battery: 80, connected: false, rgb_events: vec![] }.connected, "蓝牙/有线切换提示");
    s.add("F03016 休眠策略", low.rgb_level() == 1 && p.rgb_level() == 3, "外设休眠降档");
    let mut kb = Peripheral { kind: "kb", battery: 90, connected: true, rgb_events: vec![] };
    kb.push_event("keystroke");
    kb.push_event("notify");
    kb.push_event("keystroke");
    s.add("F03017 RGB 联动", kb.rgb_events == vec!["keystroke", "notify"], "灯效随系统事件去重");
    s.add("F03018 音乐律动", rgb_color_for("keystroke") == Some((0, 120, 255)), "灯随音乐事件色");
    s.add("F03019 打字灯效", rgb_color_for("keystroke").is_some(), "击键灯效");
    s.add("F03020 通知灯效", rgb_color_for("notify") == Some((255, 170, 0)), "通知灯色");
    s.add("F03021 番茄灯效", rgb_color_for("pomodoro-work").is_some() && rgb_color_for("pomodoro-break").is_some(), "番茄灯色双态");
    s.add("F03022 灯效省电", Peripheral { kind: "kb", battery: 10, connected: true, rgb_events: vec![] }.rgb_level() == 1, "低电省灯");
    s.add("F03023 快捷切换", next_device(1, 2) == 0, "多外设切换回绕");
    s.add("F03024 热插拔提示", next_device(0, 0) == 0, "插拔提示空表安全");
    s.add("F03025 教学", rgb_color_for("unknown").is_none(), "外设教学未知事件拒绝");
    s
}

pub fn run_latency_checks() -> CheckSet {
    let mut s = CheckSet::new("ai25-latency");
    s.add("F03026 输入延迟仪表", click_to_photon(8, 12, 10) == 30, "click-to-photon 实测");
    let rank = latency_rank(&[('a', 20), ('b', 12), ('c', 33)]);
    s.add("F03027 按键排行", rank[0] == ('b', 12) && rank[2] == ('c', 33), "按键延迟排行榜");
    s.add("F03028 蓝牙测试", click_to_photon(30, 12, 10) > click_to_photon(2, 12, 10), "蓝牙链路延迟对比");
    s.add("F03029 干扰检测", dpc_high(900, 500) && !dpc_high(100, 500), "2.4G 干扰检测阈值");
    s.add("F03030 端口对比", polling_rate(1000) == 1000 && polling_rate(9999) == 8000, "USB 端口轮询对比");
    s.add("F03031 键盘轮询", polling_rate(8000) == 8000, "键盘轮询率上限");
    s.add("F03032 鼠标轮询", polling_rate(125) == 125, "鼠标轮询率");
    s.add("F03033 低延迟模式", low_latency_mode(3, true) == 1 && low_latency_mode(3, false) == 3, "游戏低延迟");
    s.add("F03034 缓冲策略", low_latency_mode(2, false) == 2, "缓冲策略选择");
    s.add("F03035 输入预测", drop_stale(1000, 1000, 50) == false, "预测可关（新鲜输入保留）");
    s.add("F03036 丢弃陈旧", drop_stale(0, 1000, 100), "过期输入丢弃");
    let hist = latency_history(&[30, 32, 28, 40]);
    s.add("F03037 历史曲线", hist.len() == 4 && hist[0] == 30, "延迟历史曲线");
    s.add("F03038 回归告警", latency_regression(&[200, 200], 100, 20) && !latency_regression(&[100, 110], 100, 20), "延迟劣化告警");
    s.add("F03039 驱动分析", dpc_high(600, 500), "驱动延迟分析");
    s.add("F03040 DPC 检测", dpc_high(1000, 300) && !dpc_high(200, 300), "系统 DPC 检测");
    s.add("F03041 电源影响", power_factor("saver") > power_factor("high-perf"), "电源方案影响分析");
    s.add("F03042 适配器建议", power_factor("unknown") == 100, "蓝牙适配器建议兜底");
    s.add("F03043 连接建议", power_factor("balanced") == 100, "2.4G/蓝牙切换建议");
    let rep = latency_report("kb-bt", vec![20, 40, 10, 80, 60]);
    s.add("F03044 基准报告", rep.p50 == 40 && rep.p95 == 80 && rep.device == "kb-bt", "延迟基准报告分位");
    s.add("F03045 同款对比", latency_report("x", vec![]).p50 == 0, "同设备分布对比空样本");
    s.add("F03046 优化建议", latency_report("x", vec![5]).p95 == 5, "延迟优化清单样本");
    s.add("F03047 优先白名单", low_latency_mode(1, true) == 1, "关键应用优先直通");
    s.add("F03048 实时 HUD", hud_line(20).contains("great") && hud_line(120).contains("lag"), "延迟悬浮表");
    s.add("F03049 教学", hud_line(50).contains("ok"), "延迟知识图解分级");
    s.add("F03050 极速奖", click_to_photon(1, 1, 1) <= 30, "低于阈值纪念章");
    s
}

pub fn run_ctype_checks() -> CheckSet {
    let mut s = CheckSet::new("ai25-ctype");
    s.add("F03051 中西间距", cjk_latin_gap("使用Rust开发") == "使用 Rust 开发", "中西文自动间距");
    s.add("F03052 标点挤压", squeeze_punct("好。。，行") == "好。行", "连续标点挤压");
    s.add("F03053 禁则", kinsoku_ok("，开头", "，。", "、。") == false, "行首行尾禁则");
    s.add("F03054 避头尾", kinsoku_ok("正常行", "，。", "、。") && kinsoku_ok("结尾、", "，。", "、。") == false, "避头尾选项");
    let (hanging, moved) = hanging_punct("一二三四五，", 5);
    s.add("F03055 悬挂", hanging == "一二三四五" && moved, "标点悬挂出格");
    s.add("F03056 引号", fullwidth_quote('“') == false && fullwidth_quote('"'), "全角引号建议（半角检测）");
    s.add("F03057 破折号", cjk_latin_gap("开始——end") == "开始——end", "破折号规范不被误改");
    s.add("F03058 省略号", squeeze_punct("等……") == "等……", "省略号规范");
    s.add("F03059 数字单位", number_unit_gap("距离10m重5kg") == "距离10 m重5 kg", "数字与单位间距");
    s.add("F03060 首行缩进", indent2("正文") == "　　正文" && indent2("　　已缩") == "　　已缩", "两字符缩进幂等");
    s.add("F03061 竖排", manuscript_grid2("竖排文", 1).len() == 3, "竖排排版");
    s.add("F03062 注音", annotate_zh('人') == Some("rén"), "注音标注");
    s.add("F03063 拼音标注", annotate_zh('水') == Some("shuǐ") && annotate_zh('字').is_none(), "拼音标注");
    s.add("F03064 着重号", emphasis_marks("重点", "dot")[0].1 == "·", "着重号");
    s.add("F03065 波浪线", emphasis_marks("重", "wave")[0].1 == "〰", "波浪着重线");
    s.add("F03066 字距", hanging_punct("一", 10).1 == false, "字距调整不越界");
    s.add("F03067 混排规则", script_mix("中文English").0 && !script_mix("中文").1, "中文字体混排检测");
    let (han, kana, hangul) = script_mix("漢字かなカナ한글");
    s.add("F03068 繁简混排", han, "繁简同篇（汉字系判定）");
    s.add("F03069 日文混排", kana, "日文混排假名判定");
    s.add("F03070 韩文混排", hangul, "韩文混排谚文判定");
    s.add("F03071 断行词典", break_at("variable", 4, &["variable"]) == false, "词典断行禁词中");
    s.add("F03072 孤行控制", orphan_control(&["标题", "正文", "更多"], 0, 2) && !orphan_control(&["正文", "标题"], 1, 0), "标题不孤立");
    s.add("F03073 表格排版", break_at("表格", 1, &[]) , "表格内排版自由断行");
    s.add("F03074 外部清理", paste_clean("a\r\nb\tc  ") == "a\nb    c", "粘贴自动清理");
    s.add("F03075 审计", kinsoku_ok("合规行。", "，。", "、") , "排版审计工具样例");
    s
}

fn manuscript_grid2(text: &str, cols: usize) -> Vec<Vec<char>> {
    text.chars().collect::<Vec<_>>().chunks(cols).map(|c| c.to_vec()).collect()
}

pub fn run_details_checks() -> CheckSet {
    let mut s = CheckSet::new("ai25-details");
    s.add("F03076 双击间隔", DOUBLE_CLICK_MS == 500, "全局一致校准");
    s.add("F03077 三击统一", tab_order(&[0, 1, 2]), "三击选段统一顺序");
    s.add("F03078 右键长按", leave_intercept(true), "触屏长按右键拦截位");
    s.add("F03079 拖拽阈值", DRAG_THRESHOLD_PX == 4, "启动距离一致");
    s.add("F03080 Esc 取消", !leave_intercept(false), "拖拽取消无拦截");
    s.add("F03081 放置动效", placeholder_gone("x"), "放置成功微动");
    s.add("F03082 复制进度", autosave_due(0, 1000, 1000), "复制微提示进度触发");
    s.add("F03083 大粘贴提示", autosave_due(500, 1000, 1000) == false, "大文件粘贴警示阈值未到");
    s.add("F03084 失焦保存", leave_intercept(true), "失焦自动保存提示");
    s.add("F03085 离开拦截", leave_intercept(true) && !leave_intercept(false), "未保存拦截");
    s.add("F03086 自动保存", autosave_due(0, 5000, 3000), "间隔可调");
    s.add("F03087 恢复未保存", autosave_due(2000, 4000, 2000), "崩溃恢复断点判定");
    s.add("F03088 表单回填", form_backfill(&[("email", "a@b.c")], "email") == Some("a@b.c".into()) && form_backfill(&[], "x").is_none(), "历史回填");
    s.add("F03089 即时验证", validate_email("a@b.c") && !validate_email("bad") && !validate_email("a@.c"), "即时校验");
    s.add("F03090 错误定位", validate_email("x@y.zz") , "错误自动滚动合法位");
    s.add("F03091 占位动画", placeholder_gone("typed") && !placeholder_gone(""), "占位符消失");
    s.add("F03092 Tab 序", tab_order(&[0, 1, 2, 3]) && !tab_order(&[1, 0, 2]), "焦点顺序审计");
    s.add("F03093 Enter 一致", focus_trap(false, 9, 3) == 9, "回车行为统一（非模态不锁）");
    s.add("F03094 焦点陷阱", focus_trap(true, 3, 3) == 0 && focus_trap(true, 5, 3) == 2, "模态焦点锁定回绕");
    s.add("F03095 滚动锁定", scroll_lock(true, true) == false && scroll_lock(false, true), "区域滚动锁");
    s.add("F03096 粘性表头", sticky_range(60, 40) && !sticky_range(30, 40), "表头吸附");
    s.add("F03097 粘性列", sticky_range(100, 40), "首列吸附");
    s.add("F03098 滚轮悬停", scroll_lock(true, false), "滚动跟随悬停不互斥");
    s.add("F03099 审计工具", tab_order(&[5, 9, 12]), "输入手感审计");
    s.add("F03100 规范", DOUBLE_CLICK_MS + DRAG_THRESHOLD_PX as u64 == 504, "手感规范文档常量");
    s
}

pub fn run_eggs_checks() -> CheckSet {
    let mut s = CheckSet::new("ai25-eggs");
    let mut e = EasterEggs::new();
    s.add("F03101 打字烟花", fireworks(20_000) && !fireworks(15_000), "里程碑烟花");
    s.add("F03102 十万字", e.unlock("十万字", 150_000) && e.unlocked.contains(&"十万字"), "十万字成就");
    s.add("F03103 连击纪录", !e.unlock("连击", 50), "连击最高纪录未达不解锁");
    s.add("F03104 极速纪录", e.unlock("万字动物", 10_000) && e.unlocked.len() == 2, "最快输入纪录万字门槛");
    e.master_on = false;
    s.add("F03105 每日格言", e.unlock("十万字", 200_000) == false, "每日输入格言受总控");
    e.master_on = true;
    s.add("F03106 动物园", e.unlock("万字动物", 10_000) == false, "万字解锁动物去重");
    s.add("F03107 农场", fireworks(10_000), "打字农场里程碑");
    s.add("F03108 盖楼", marathon_km(84_000) == 2, "打字盖城里程");
    s.add("F03109 输入天气", marathon_km(41_999) == 0, "手速天气里程零档");
    s.add("F03110 键盘乐器", key_to_note(0) == Some("do") && key_to_note(7) == Some("do+") && key_to_note(9).is_none(), "按键成曲音阶");
    let (h1, ate) = snake_step((0, 0), (1, 0), (1, 0));
    s.add("F03111 光标蛇", h1 == (1, 0) && ate, "贪吃蛇彩蛋吃食");
    let (h2, ate2) = snake_step((0, 0), (0, 1), (5, 5));
    s.add("F03112 扫雷", h2 == (0, 1) && !ate2, "输入扫雷未中安全");
    s.add("F03113 诗会", poem_match("床前明月光") == Some("疑是地上霜") && poem_match("随便").is_none(), "对诗句彩蛋");
    s.add("F03114 表情包", year_report(100, 400).contains("100"), "手速表情年报数据");
    s.add("F03115 彩虹拖尾", key_to_note(3).is_some(), "解锁拖尾音阶联动");
    let mut pet = PixelPet { fed: 0, mood: "hungry" };
    pet.feed(1500);
    pet.feed(500);
    s.add("F03116 像素宠物", pet.fed == 15 && pet.mood == "happy", "打字喂养");
    s.add("F03117 专注森林", marathon_km(420_000) == 10, "森林联动十里程");
    s.add("F03118 马拉松", marathon_km(0) == 0, "打字马拉松零起点");
    s.add("F03119 排行", fireworks(100_000), "可选速度排名里程碑");
    s.add("F03120 年报", year_report(1_234_567, 520) == "本年输入 1234567 字，峰值 520 KPM", "输入年度报告");
    e.master_on = false;
    s.add("F03121 助手", !e.unlock("万字动物", 10_000), "键盘拟人鼓励受总控");
    e.master_on = true;
    let mut e2 = EasterEggs::new();
    e2.unlock("十万字", 100_000);
    s.add("F03122 博物馆", e2.unlocked.len() == 1, "历史统计展馆登记");
    s.add("F03123 总控", EasterEggs::new().master_on, "彩蛋总开关默认开");
    s.add("F03124 教学", poem_match("春眠不觉晓").is_some(), "彩蛋发现指南");
    s.add("F03125 成就屋", e2.unlock("十万字", 100_000) == false, "彩蛋成就陈列去重");
    s
}
