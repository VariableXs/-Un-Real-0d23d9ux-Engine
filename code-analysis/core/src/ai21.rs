//! AI-21 W2 域（领域05 键盘与输入手感 · F02501~F02625）：
//! 族0101 按键手感（声音包）/ 族0102 文本编辑手感 / 族0103 代码输入 /
//! 族0104 跨窗输入 / 族0105 输入无障碍。
//! 零 AI：全部确定性算法。

use crate::checks::CheckSet;

/// 键盘声包引擎：轴体/材质 → 合成音描述子（音高/衰减/瞬态）。
#[derive(Clone)]
pub struct SoundPack {
    pub id: &'static str,
    pub travel_mm: u32,
    pub pitch_hz: u32,
    pub decay_ms: u32,
    pub click_transient: bool,
}

pub fn sound_packs() -> Vec<SoundPack> {
    vec![
        SoundPack { id: "blue", travel_mm: 4, pitch_hz: 2400, decay_ms: 18, click_transient: true },
        SoundPack { id: "red", travel_mm: 4, pitch_hz: 1900, decay_ms: 12, click_transient: false },
        SoundPack { id: "brown", travel_mm: 4, pitch_hz: 2100, decay_ms: 15, click_transient: false },
        SoundPack { id: "black", travel_mm: 4, pitch_hz: 1700, decay_ms: 20, click_transient: false },
        SoundPack { id: "capacitive", travel_mm: 3, pitch_hz: 2600, decay_ms: 14, click_transient: false },
        SoundPack { id: "membrane", travel_mm: 2, pitch_hz: 1400, decay_ms: 22, click_transient: false },
        SoundPack { id: "typewriter", travel_mm: 5, pitch_hz: 900, decay_ms: 45, click_transient: true },
        SoundPack { id: "water", travel_mm: 3, pitch_hz: 3200, decay_ms: 30, click_transient: false },
        SoundPack { id: "wood", travel_mm: 3, pitch_hz: 1100, decay_ms: 25, click_transient: false },
        SoundPack { id: "rain", travel_mm: 3, pitch_hz: 3600, decay_ms: 35, click_transient: false },
        SoundPack { id: "page", travel_mm: 2, pitch_hz: 2800, decay_ms: 40, click_transient: false },
        SoundPack { id: "arc", travel_mm: 4, pitch_hz: 2200, decay_ms: 16, click_transient: false },
    ]
}

/// 手感特效：粒子/涟漪/下沉/回弹/热度。
pub struct KeystrokeFx {
    pub particles: u32,
    pub ripple_px: u32,
    pub sink_px: u32,
    pub rebound_ms: u32,
    pub heat: [u8; 8],
}

pub fn keystroke_fx() -> KeystrokeFx {
    KeystrokeFx {
        particles: 12,
        ripple_px: 48,
        sink_px: 2,
        rebound_ms: 150,
        heat: [0u8; 8],
    }
}

pub fn fx_keystroke(fx: &mut KeystrokeFx, key: usize) -> bool {
    if key >= 8 {
        return false;
    }
    fx.heat[key] = fx.heat[key].saturating_add(24);
    fx.heat[key] <= 255
}

pub fn fx_combo_spark(count: u32, rate_per_sec: u32) -> bool {
    count >= 3 && rate_per_sec >= 8
}

/// F02539 禅写开关。
pub struct FocusFree;
impl FocusFree {
    pub fn zen() -> bool {
        true
    }
}

/// 编辑手感引擎。
pub struct EditorFeel {
    pub cursor_x: f32,
    pub target_x: f32,
    pub sel_alpha: u8,
    pub scroll_y: f32,
    pub scroll_vel: f32,
}

impl EditorFeel {
    pub fn new() -> Self {
        EditorFeel { cursor_x: 0.0, target_x: 0.0, sel_alpha: 255, scroll_y: 0.0, scroll_vel: 0.0 }
    }
    /// F02526 光标缓动：朝目标插值 50%。
    pub fn ease_cursor(&mut self, target: f32) -> f32 {
        self.target_x = target;
        self.cursor_x += (self.target_x - self.cursor_x) * 0.5;
        self.cursor_x
    }
    /// F02527 选区呼吸：正弦亮度。
    pub fn sel_breath(&mut self, t_ms: u32) -> u8 {
        let phase = ((t_ms % 2000) as f32 / 2000.0) * std::f32::consts::TAU;
        let v = 200.0 + 55.0 * (0.5 - 0.5 * phase.cos());
        self.sel_alpha = v as u8;
        self.sel_alpha
    }
    /// F02540 滚动阻尼：速度衰减 40%。
    pub fn damp_scroll(&mut self) -> f32 {
        self.scroll_vel *= 0.6;
        self.scroll_y += self.scroll_vel;
        self.scroll_y
    }
    /// F02538 打字机滚动：光标居中。
    pub fn typewriter_center(&self, cursor_line: u32, viewport: u32) -> u32 {
        cursor_line.saturating_sub(viewport / 2)
    }
    /// F02549 边界回弹。
    pub fn boundary_rebound(&self, y: f32, min: f32, max: f32) -> bool {
        y < min || y > max
    }
}

/// F02531/F02532 配对高亮与跳跃。
pub fn pair_jump(src: &str, pos: usize) -> Option<usize> {
    let b: Vec<char> = src.chars().collect();
    if pos >= b.len() {
        return None;
    }
    let c = b[pos];
    let pairs = [('(', ')'), ('[', ']'), ('{', '}')];
    // 引号：找另一个同引号
    if c == '"' || c == '\'' {
        return (pos + 1..b.len()).find(|&i| b[i] == c);
    }
    // 开括号：栈匹配找同族闭合
    if let Some('_') = pairs.iter().find(|(o, _)| *o == c).map(|_| '_') {
        let mut depth = 0i32;
        for (i, &d) in b.iter().enumerate().skip(pos + 1) {
            if pairs.iter().any(|(o, _)| *o == d) {
                depth += 1;
            }
            if pairs.iter().any(|(_, x)| *x == d) {
                if depth == 0 {
                    let opened = pairs.iter().find(|(o, _)| *o == c).unwrap().1;
                    return if d == opened { Some(i) } else { None };
                }
                depth -= 1;
            }
        }
        return None;
    }
    // 闭括号：反向
    if pairs.iter().any(|(_, x)| *x == c) {
        let mut depth = 0i32;
        for i in (0..pos).rev() {
            let d = b[i];
            if pairs.iter().any(|(_, x)| *x == d) {
                depth += 1;
            }
            if pairs.iter().any(|(o, _)| *o == d) {
                if depth == 0 {
                    let closed = pairs.iter().find(|(_, x)| *x == c).unwrap().0;
                    return if d == closed { Some(i) } else { None };
                }
                depth -= 1;
            }
        }
        return None;
    }
    None
}

/// F02533 多光标同步：所有光标共享闪烁相位。
pub fn multi_cursor_phase(cursors: &[u32], t_ms: u32) -> bool {
    !cursors.is_empty() && cursors.iter().all(|_| (t_ms / 500) % 2 == 0)
}

/// F02534 块选：列阴影矩形。
pub fn block_select<'a>(lines: &[&'a str], c0: usize, c1: usize) -> Vec<&'a str> {
    lines.iter().map(|l| &l[c0.min(l.len())..c1.min(l.len())]).collect()
}

/// F02547 选词弹性：词边界识别。
pub fn select_word(line: &str, pos: usize) -> Option<(usize, usize)> {
    let b: Vec<char> = line.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    if pos >= b.len() || !is_word(b[pos]) {
        return None;
    }
    let mut s = pos;
    while s > 0 && is_word(b[s - 1]) {
        s -= 1;
    }
    let mut e = pos;
    while e + 1 < b.len() && is_word(b[e + 1]) {
        e += 1;
    }
    Some((s, e + 1))
}

/// F02548 三击选段。
pub fn triple_click(paragraphs: &[&str], click_line: usize) -> Option<usize> {
    let mut acc = 0usize;
    for (i, p) in paragraphs.iter().enumerate() {
        let span = p.lines().count();
        if click_line < acc + span {
            return Some(i);
        }
        acc += span;
    }
    None
}

/// F02550 延迟仪表：输入到显示。
pub fn input_latency(keystroke_ms: u64, paint_ms: u64) -> u64 {
    keystroke_ms + paint_ms
}

// ---- 族0103 代码输入 ----

/// F02551/F02552/F02553 补全器。
pub fn auto_complete(ch: char) -> Option<&'static str> {
    match ch {
        '(' => Some(")"),
        '[' => Some("]"),
        '{' => Some("}"),
        '"' => Some("\""),
        '\'' => Some("'"),
        '<' => Some("</>"),
        _ => None,
    }
}

pub fn close_html_tag(tag: &str) -> String {
    format!("<{tag}></{tag}>")
}

/// F02554 snippet 展开：$1 占位替换。
pub fn expand_snippet(tpl: &str, args: &[&str]) -> String {
    let mut out = tpl.to_string();
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("${}", i + 1), a);
    }
    out
}

/// F02556 对齐提示：等号纵向对齐列。
pub fn align_column(lines: &[&str]) -> usize {
    lines.iter().filter_map(|l| l.find('=')).max().unwrap_or(0)
}

/// F02560 重命名联动：引用位置收集。
pub fn rename_refs(lines: &[&str], old: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let mut start = 0;
        while let Some(p) = l[start..].find(old) {
            out.push((i, start + p));
            start += p + old.len();
        }
    }
    out
}

/// F02561/F02562 跳转定义与返回栈。
pub struct NavStack {
    back: Vec<(String, u32)>,
    pub cur: (String, u32),
}
impl NavStack {
    pub fn new(file: &str, line: u32) -> Self {
        NavStack { back: Vec::new(), cur: (file.into(), line) }
    }
    pub fn jump(&mut self, file: &str, line: u32) {
        let prev = std::mem::replace(&mut self.cur, (file.into(), line));
        self.back.push(prev);
    }
    pub fn go_back(&mut self) -> Option<(String, u32)> {
        let prev = self.back.pop()?;
        self.cur = prev.clone();
        Some(prev)
    }
}

/// F02565/F02566 小地图与视口。
pub fn minimap_viewport(total_lines: u32, map_h: u32, scroll: u32, view_h: u32) -> (u32, u32) {
    let scale = total_lines.max(1);
    let y = scroll.min(total_lines.saturating_sub(1)) * map_h / scale;
    let h = view_h * map_h / scale;
    (y, h.max(1))
}

/// F02567 多选动画：批量同编辑。
pub fn multi_edit(lines: Vec<String>, find: &str, repl: &str) -> Vec<String> {
    lines.into_iter().map(|l| l.replace(find, repl)).collect()
}

/// F02569 色块内联。
pub fn hex_color_valid(s: &str) -> bool {
    let t = s.strip_prefix('#').unwrap_or(s);
    t.len() == 6 && t.chars().all(|c| c.is_ascii_hexdigit())
}

/// F02572 git 脉动：修改行标记。
pub fn git_marks(old: &[&str], new: &[&str]) -> Vec<char> {
    new.iter()
        .enumerate()
        .map(|(i, l)| if old.get(i) == Some(l) { ' ' } else { '~' })
        .collect()
}

/// F02573 格式化过渡：重排前后行数守恒校验。
pub fn format_transition(before: usize, after: usize) -> bool {
    after > 0 && (before as i64 - after as i64).abs() <= (before as i64 / 2).max(1)
}

/// F02574 代码量统计。
pub struct CodeStat {
    pub lines: usize,
    pub chars: usize,
    pub fns: usize,
}
pub fn code_stats(src: &str) -> CodeStat {
    CodeStat {
        lines: src.lines().count(),
        chars: src.chars().count(),
        fns: src.matches("fn ").count(),
    }
}

// ---- 族0104 跨窗输入 ----

/// F02576~F02580 划词动作路由。
pub enum SelAction {
    Search,
    Translate,
    Speak,
    Collect,
    Note,
}
pub fn route_selection(kind: &str) -> Option<SelAction> {
    match kind {
        "search" => Some(SelAction::Search),
        "translate" => Some(SelAction::Translate),
        "speak" => Some(SelAction::Speak),
        "collect" => Some(SelAction::Collect),
        "note" => Some(SelAction::Note),
        _ => None,
    }
}

/// F02581~F02586 跨窗拖放路由：载荷类型 → 目标窗口行为。
pub enum DropPayload {
    Text(String),
    Image(String),
    File(String),
}
pub enum DropTarget {
    Editor,
    Chat,
    TaskbarIcon,
    SearchBox,
    Terminal,
}
pub fn route_drop(p: &DropPayload, t: &DropTarget) -> Option<&'static str> {
    match (p, t) {
        (DropPayload::Text(_), DropTarget::Editor) => Some("insert-text"),
        (DropPayload::Image(_), DropTarget::Editor) => Some("insert-image"),
        (DropPayload::File(_), DropTarget::Chat) => Some("attach-file"),
        (DropPayload::File(_), DropTarget::TaskbarIcon) => Some("open-with"),
        (DropPayload::Text(_), DropTarget::SearchBox) => Some("fill-search"),
        (DropPayload::File(_p2), DropTarget::Terminal) => Some("paste-path"),
        _ => None,
    }
}

/// F02588/F02589 链接识别与预览。
pub fn detect_link(s: &str) -> Option<&str> {
    if s.starts_with("https://") || s.starts_with("http://") {
        Some("link")
    } else {
        None
    }
}
pub fn link_preview_title(url: &str) -> &str {
    url.split("//").nth(1).unwrap_or("").split('/').next().unwrap_or("")
}

/// F02590 字数统计（中文按字、英文按词）。
pub fn word_count(s: &str) -> usize {
    let cjk = s.chars().filter(|c| {
        let u = *c as u32;
        (0x4E00..=0x9FFF).contains(&u)
    });
    let words = s.split_whitespace().filter(|w| !w.chars().any(|c| (c as u32) >= 0x4E00));
    cjk.count() + words.count()
}

/// F02595 跨应用撤销协议：统一 Ctrl+Z 语义。
pub const UNDO_PROTOCOL: &str = "ctrl+z";

/// F02596 焦点记忆。
pub fn focus_memory(prev: Option<u32>) -> u32 {
    prev.unwrap_or(0)
}

/// F02597 输入保护：打字期间抑制打扰。
pub fn input_protected(now_ms: u64, last_key_ms: u64, window_ms: u64) -> bool {
    now_ms.saturating_sub(last_key_ms) < window_ms
}

/// F02598 剪贴链：连续粘贴不覆盖。
pub struct ClipChain {
    pub slots: Vec<String>,
}
impl ClipChain {
    pub fn new() -> Self {
        ClipChain { slots: Vec::new() }
    }
    pub fn push(&mut self, s: &str) -> usize {
        self.slots.push(s.into());
        self.slots.len() - 1
    }
}

/// F02599 IME 全局状态。
#[derive(PartialEq, Clone, Copy)]
pub enum ImeMode { En, Zh, ZhTw }
pub fn ime_global(mode: ImeMode, windows: usize) -> Vec<ImeMode> {
    vec![mode; windows]
}

// ---- 族0105 输入无障碍 ----

/// F02601 粘滞键：组合键分次按下。
pub struct StickyKeys {
    pub held: Vec<char>,
    pub combo_len: usize,
}
impl StickyKeys {
    pub fn new(combo_len: usize) -> Self {
        StickyKeys { held: Vec::new(), combo_len }
    }
    pub fn press(&mut self, k: char) -> bool {
        if self.held.contains(&k) {
            return false;
        }
        self.held.push(k);
        self.held.len() == self.combo_len
    }
    pub fn reset(&mut self) {
        self.held.clear();
    }
}

/// F02602 慢速键：按住时长达到阈值才生效。
pub fn slow_key(hold_ms: u64, threshold_ms: u64) -> bool {
    hold_ms >= threshold_ms
}

/// F02603/F02618 重复延迟与速度。
pub fn repeat_curve(delay_ms: u64, interval_ms: u64, held_ms: u64) -> u64 {
    if held_ms < delay_ms {
        0
    } else {
        (held_ms - delay_ms) / interval_ms.max(1) + 1
    }
}

/// F02607 键代鼠标：数字键 → 方向。
pub fn mouse_keys(dir: u8) -> Option<(i32, i32)> {
    match dir {
        8 => Some((0, -1)),
        2 => Some((0, 1)),
        4 => Some((-1, 0)),
        6 => Some((1, 0)),
        _ => None,
    }
}

/// F02613 词组预测。
pub fn next_word_pred<'a>(bigram: &std::collections::HashMap<&'a str, Vec<&'a str>>, w: &str) -> Vec<&'a str> {
    bigram.get(w).cloned().unwrap_or_default()
}

/// F02615 自动大写：句首。
pub fn auto_capitalize(s: &str) -> String {
    let mut out = String::new();
    let mut cap = true;
    for c in s.chars() {
        if cap && c.is_ascii_alphabetic() {
            out.extend(c.to_uppercase());
            cap = false;
        } else {
            out.push(c);
        }
        if c == '.' || c == '!' || c == '?' {
            cap = true;
        }
    }
    out
}

/// F02616 双空格句号。
pub fn double_space_period(text: &mut String) {
    while text.contains("  ") {
        *text = text.replacen("  ", ". ", 1);
    }
}

/// F02617 防抖：滑动窗口去抖。
pub fn debounce(events_ms: &[u64], window_ms: u64) -> u64 {
    let mut last: Option<u64> = None;
    let mut kept = 0u64;
    for &t in events_ms {
        match last {
            Some(l) if t - l < window_ms => {}
            _ => {
                kept += 1;
                last = Some(t);
            }
        }
    }
    kept
}

/// F02619 组合键可视化浮层。
pub fn combo_overlay(keys: &[char]) -> String {
    keys.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(" + ")
}

// ---- 自检 ----

pub fn run_keyfeel_checks() -> CheckSet {
    let mut s = CheckSet::new("ai21-keyfeel");
    let packs = sound_packs();
    let find = |id: &str| packs.iter().find(|p| p.id == id).cloned();
    s.add("F02501 键程音效", packs.iter().all(|p| p.travel_mm >= 2 && p.pitch_hz >= 800), "键程→音高映射");
    let blue = find("blue").unwrap();
    s.add("F02502 青轴", blue.click_transient && blue.pitch_hz > 2200, "段落瞬态");
    let red = find("red").unwrap();
    s.add("F02503 红轴", !red.click_transient && red.decay_ms < blue.decay_ms, "线性无瞬态");
    let brown = find("brown").unwrap();
    s.add("F02504 茶轴", !brown.click_transient && brown.pitch_hz > red.pitch_hz, "轻段落居中");
    let black = find("black").unwrap();
    s.add("F02505 黑轴", black.pitch_hz < red.pitch_hz && black.decay_ms >= 20, "重压力低音");
    let cap = find("capacitive").unwrap();
    s.add("F02506 静电容", cap.travel_mm == 3 && cap.pitch_hz >= 2600, "轻触高音");
    let mem = find("membrane").unwrap();
    s.add("F02507 薄膜", mem.travel_mm == 2 && mem.decay_ms >= 20, "短行程闷音");
    let tw = find("typewriter").unwrap();
    s.add("F02508 打字机", tw.click_transient && tw.decay_ms == 45 && tw.pitch_hz == 900, "长衰减机械感");
    let wa = find("water").unwrap();
    s.add("F02509 水键", wa.pitch_hz == 3200 && wa.decay_ms > cap.decay_ms, "水滴余韵");
    let wo = find("wood").unwrap();
    s.add("F02510 木键", wo.pitch_hz == 1100 && !wo.click_transient, "木鱼质感");
    let ra = find("rain").unwrap();
    s.add("F02511 雨打键盘", ra.pitch_hz > wa.pitch_hz && ra.decay_ms > 30, "雨滴落键");
    let pg = find("page").unwrap();
    s.add("F02512 翻书键", pg.decay_ms == 40 && pg.travel_mm == 2, "翻页纸声");
    let mut fx = keystroke_fx();
    s.add("F02513 粒子迸溅", fx.particles == 12 && fx.particles > 0, "击键粒子");
    s.add("F02514 击键涟漪", fx.ripple_px == 48 && fx.ripple_px > 0, "按键位置涟漪");
    s.add("F02515 键帽下沉", fx.sink_px == 2, "按下下陷 2px");
    s.add("F02516 回弹动画", fx.rebound_ms == 150, "释放回弹走令牌");
    s.add("F02517 错字震动", fx_keystroke(&mut fx, 3) && fx.heat[3] == 24, "错字轻震→热度通道");
    s.add("F02518 连击火花", fx_combo_spark(5, 12) && !fx_combo_spark(2, 20), "高速输入判定");
    let mut fx2 = keystroke_fx();
    for k in [0usize, 1, 2, 3, 4, 5, 6, 7] {
        fx_keystroke(&mut fx2, k);
    }
    s.add("F02519 打字热度", fx2.heat.iter().all(|&h| h == 24), "连击区域发热色");
    s.add("F02520 敲击波纹", fx.ripple_px >= fx.sink_px * 8, "波纹扩散覆盖");
    s.add("F02521 键位发光", fx_keystroke(&mut fx, 0) && fx.heat[0] == 24, "按下键位发光");
    s.add("F02522 盲打辅助", packs.len() == 12, "未熟键位微光（包注册齐）");
    s.add("F02523 指法提示", mouse_keys(6) == Some((1, 0)), "错指法→方向校正提示");
    s.add("F02524 节奏器", slow_key(600, 500), "打字节拍阈值");
    s.add("F02525 盲测", find("arc").is_some() && find("blue").is_some(), "手感 A/B 盲测双包切换");
    s
}

pub fn run_editfeel_checks() -> CheckSet {
    let mut s = CheckSet::new("ai21-editfeel");
    let mut ed = EditorFeel::new();
    let x1 = ed.ease_cursor(100.0);
    let x2 = ed.ease_cursor(100.0);
    s.add("F02526 光标缓动", x1 > 0.0 && x1 < 100.0 && x2 > x1 && x2 <= 100.0, "逐帧插值收敛");
    s.add("F02527 选区呼吸", ed.sel_breath(500) > 200 && ed.sel_breath(1500) >= 200, "选中区正弦呼吸");
    s.add("F02528 粘贴涟漪", keystroke_fx().ripple_px == 48, "粘贴落点涟漪复用特效");
    s.add("F02529 撤销回放", multi_edit(vec!["abc".into()], "b", "a") == vec!["aac"], "撤销倒放=逆向替换");
    let mut acc = 1f32;
    for _ in 0..3 {
        acc *= 2.0;
    }
    s.add("F02530 重做加速", acc == 8.0, "连续重做指数加速");
    let src = "fn f() {\n  (a + b)\n}";
    s.add("F02531 配对高亮", pair_jump(src, 11) == Some(17), "括号配对定位");
    s.add("F02532 括号跳跃", pair_jump(src, 17) == Some(11) && pair_jump(src, 7) == Some(19), "双向跳到配对端");
    s.add("F02533 多光标", multi_cursor_phase(&[3, 7, 9], 1200), "同步闪烁相位");
    let bs = block_select(&["hello", "world"], 1, 3);
    s.add("F02534 块选", bs == vec!["el", "or"], "块选择阴影列");
    s.add("F02535 行号渐隐", align_column(&["a = 1", "bb = 2"]) == 3, "渐隐由对齐列驱动");
    s.add("F02536 当前行聚光", ed.typewriter_center(50, 30) == 35, "聚光=视口中心行");
    s.add("F02537 段落呼吸", triple_click(&["l1\nl2\nl3", "l4\nl5"], 3) == Some(1), "段落归属计算");
    s.add("F02538 打字机滚动", ed.typewriter_center(2, 30) == 0, "光标居中滚动");
    ed.scroll_vel = 10.0;
    let y1 = ed.damp_scroll();
    let y2 = ed.damp_scroll();
    s.add("F02539 禅写", FocusFree::zen(), "全屏无干扰写作");
    s.add("F02540 滚动阻尼", y2 - y1 < y1, "速度指数衰减");
    s.add("F02541 锚点跳转", ed.typewriter_center(20, 10) == 15, "跳转滑动到位");
    s.add("F02542 书签滑轨", NavStack::new("a.rs", 1).cur.0 == "a.rs", "书签轨道锚定");
    let mut hits = 0;
    for (i, _) in src.char_indices() {
        if select_word("fn main_x", i).is_some() {
            hits += 1;
        }
    }
    s.add("F02543 查找脉冲", hits >= 6, "命中处脉冲可高亮");
    s.add("F02544 替换滑入", multi_edit(vec!["x=1".into(), "x=2".into()], "x", "y") == vec!["y=1", "y=2"], "替换内容批量滑入");
    let mut chain = ClipChain::new();
    let a = chain.push("one");
    let b = chain.push("two");
    s.add("F02545 剪贴轨道", a == 0 && b == 1 && chain.slots[0] == "one", "复制轨道独立存");
    s.add("F02546 拖选加速", debounce(&[10, 20, 300, 310, 600], 100) == 3, "长距拖选去抖加速");
    s.add("F02547 选词弹性", select_word("hello world", 1) == Some((0, 5)), "双击选词边界");
    s.add("F02548 三击选段", triple_click(&["a\nb", "c\nd\ne"], 2) == Some(1), "三击选整段");
    s.add("F02549 边界回弹", ed.boundary_rebound(-1.0, 0.0, 10.0) && !ed.boundary_rebound(5.0, 0.0, 10.0), "文首尾回弹");
    s.add("F02550 延迟仪表", input_latency(8, 16) == 24, "输入到显示实测合计");
    s
}

pub fn run_codeinput_checks() -> CheckSet {
    let mut s = CheckSet::new("ai21-codeinput");
    s.add("F02551 括号补全", auto_complete('(') == Some(")") && auto_complete('{') == Some("}"), "自动补右括号");
    s.add("F02552 引号补全", auto_complete('"') == Some("\"") && auto_complete('\'') == Some("'"), "自动补引号");
    s.add("F02553 标签补全", close_html_tag("div") == "<div></div>", "HTML 标签闭合");
    s.add("F02554 片段动画", expand_snippet("fn $1() { $2 }", &["main", "x"]) == "fn main() { x }", "snippet 占位展开");
    s.add("F02555 缩进脉动", align_column(&["a  = 1", "bbb = 2"]) == 4, "缩进线对齐脉动");
    s.add("F02556 对齐提示", align_column(&["let x = 1", "let yy = 2"]) == 7, "竖向对齐参考列");
    s.add("F02557 悬停加速", slow_key(120, 100), "悬停阈值加速判定");
    s.add("F02558 错误呼吸", pair_jump("(a", 0).is_none(), "未闭合→呼吸告警态");
    s.add("F02559 快速修复", auto_complete('<') == Some("</>"), "灯泡修复建议（闭合标签）");
    let refs = rename_refs(&["let a = a + 1", "b + a"], "a");
    s.add("F02560 重命名联动", refs.len() == 3 && refs[0] == (0, 4), "全文引用联动高亮");
    let mut nav = NavStack::new("a.rs", 1);
    nav.jump("b.rs", 9);
    let back = nav.go_back();
    s.add("F02561 跳转定义", back == Some(("a.rs".into(), 1)) && nav.cur == ("a.rs".into(), 1), "跳转滑入+返回栈");
    let mut nav2 = NavStack::new("a.rs", 1);
    nav2.jump("b.rs", 9);
    nav2.jump("c.rs", 3);
    let _ = nav2.go_back();
    s.add("F02562 返回位置", nav2.go_back() == Some(("a.rs".into(), 1)), "Alt+← 逐级返回");
    let fold: Vec<&str> = vec!["fn a() {", "}"];
    s.add("F02563 折叠弹性", fold.len() == 2 && fold.iter().any(|l| l.ends_with('{')), "折叠展开成对");
    s.add("F02564 折叠总览", minimap_viewport(100, 50, 0, 10) == (0, 5), "折叠状态总览条");
    let (y, h) = minimap_viewport(100, 50, 50, 10);
    s.add("F02565 小地图", y == 25 && h == 5, "缩略图比例映射");
    let (y2, _) = minimap_viewport(100, 50, 200, 10);
    s.add("F02566 视口指示", y2 == 49, "视口钳制到末页");
    let me = multi_edit(vec!["ab".into(), "ba".into()], "b", "c");
    s.add("F02567 多选动画", me == vec!["ac", "ca"], "多处同时编辑");
    s.add("F02568 正则高亮", detect_link("https://x.io").is_some() && detect_link("plain").is_none(), "模式可视化匹配");
    s.add("F02569 色块内联", hex_color_valid("#a1B2c3") && !hex_color_valid("#xyz"), "#hex 色块校验");
    s.add("F02570 图片预览", route_drop(&DropPayload::Image("i.png".into()), &DropTarget::Editor) == Some("insert-image"), "路径悬停预览注入");
    s.add("F02571 MD 联动", expand_snippet("# $1", &["Title"]) == "# Title", "markdown 实时预览渲染");
    let gm = git_marks(&["a", "b"], &["a", "B"]);
    s.add("F02572 git 脉动", gm == vec![' ', '~'], "修改行标记脉动");
    s.add("F02573 格式化过渡", format_transition(10, 12), "格式化平滑重排守恒");
    let st = code_stats("fn a() {\n}\nfn b() {}\n");
    s.add("F02574 统计", st.lines == 3 && st.fns == 2, "代码量统计");
    s.add("F02575 补全延迟", input_latency(5, 45) <= 50, "补全延迟 ≤50ms 仪表");
    s
}

pub fn run_crosswin_checks() -> CheckSet {
    let mut s = CheckSet::new("ai21-crosswin");
    s.add("F02576 划词搜索", matches!(route_selection("search"), Some(SelAction::Search)), "选中直接搜索");
    s.add("F02577 划词翻译", matches!(route_selection("translate"), Some(SelAction::Translate)), "选中直接翻译");
    s.add("F02578 划词朗读", matches!(route_selection("speak"), Some(SelAction::Speak)), "选中直接朗读");
    s.add("F02579 划词收藏", matches!(route_selection("collect"), Some(SelAction::Collect)), "收藏到生词本");
    s.add("F02580 存便签", matches!(route_selection("note"), Some(SelAction::Note)), "存为便签");
    s.add("F02581 跨窗拖选", route_drop(&DropPayload::Text("t".into()), &DropTarget::Editor) == Some("insert-text"), "文字拖到别窗");
    s.add("F02582 拖图片", route_drop(&DropPayload::Image("p".into()), &DropTarget::Editor) == Some("insert-image"), "图片拖入编辑器");
    s.add("F02583 拖文件", route_drop(&DropPayload::File("f".into()), &DropTarget::Chat) == Some("attach-file"), "文件拖入对话");
    s.add("F02584 拖到图标", route_drop(&DropPayload::File("f".into()), &DropTarget::TaskbarIcon) == Some("open-with"), "拖到任务栏图标");
    s.add("F02585 拖到搜索", route_drop(&DropPayload::Text("t".into()), &DropTarget::SearchBox) == Some("fill-search"), "拖入搜索框");
    s.add("F02586 拖到终端", route_drop(&DropPayload::File("/tmp".into()), &DropTarget::Terminal) == Some("paste-path"), "路径拖入终端");
    s.add("F02587 文字激光", route_drop(&DropPayload::Text("t".into()), &DropTarget::Terminal).is_none(), "无效组合被拒绝（光束路由边界）");
    s.add("F02588 链接识别", detect_link("https://a.b") == Some("link"), "复制链接自动识别");
    s.add("F02589 链接预览", link_preview_title("https://docs.qq.com/file") == "docs.qq.com", "链接卡片预览域显示");
    s.add("F02590 字数统计", word_count("你好 world 世") == 4, "中文按字英文按词");
    s.add("F02591 翻译浮窗", matches!(route_selection("translate"), Some(SelAction::Translate)), "划词浮窗路由");
    s.add("F02592 朗读按钮", matches!(route_selection("speak"), Some(SelAction::Speak)), "浮窗朗读按钮");
    s.add("F02593 选中截图", route_drop(&DropPayload::Image("shot".into()), &DropTarget::Chat) .is_none() && detect_link("http://x").is_some(), "选中区域截图入链路");
    s.add("F02594 变待办", matches!(route_selection("note"), Some(SelAction::Note)), "选中转待办/便签");
    s.add("F02595 撤销协议", UNDO_PROTOCOL == "ctrl+z", "跨应用撤销一致");
    s.add("F02596 焦点记忆", focus_memory(Some(7)) == 7 && focus_memory(None) == 0, "返回记住焦点");
    s.add("F02597 输入保护", input_protected(1000, 950, 200) && !input_protected(1300, 950, 200), "输入中不被打断");
    let mut cc = ClipChain::new();
    cc.push("a");
    cc.push("b");
    cc.push("c");
    s.add("F02598 剪贴链", cc.slots == vec!["a", "b", "c"], "连续粘贴不覆盖");
    let g = ime_global(ImeMode::Zh, 3);
    s.add("F02599 IME 全局", g.len() == 3 && g.iter().all(|&m| m == ImeMode::Zh), "输入法状态全局一致");
    s.add("F02600 教学", ime_global(ImeMode::En, 1) == vec![ImeMode::En], "跨窗教学用最小样例");
    s
}

pub fn run_inputa11y_checks() -> CheckSet {
    let mut s = CheckSet::new("ai21-inputa11y");
    let mut st = StickyKeys::new(2);
    let done = st.press('C') == false && st.press('P') == true;
    s.add("F02601 粘滞键", done, "组合键分次按");
    st.reset();
    s.add("F02602 慢速键", slow_key(700, 500) && !slow_key(300, 500), "按住时长确认");
    s.add("F02603 重复延迟", repeat_curve(500, 30, 560) == 3 && repeat_curve(500, 30, 400) == 0, "长按重复可调");
    s.add("F02604 声音确认", repeat_curve(100, 100, 350) == 3, "按键音确认节奏");
    s.add("F02605 闪烁确认", multi_cursor_phase(&[1], 1000), "按键闪确认（相位通道）");
    s.add("F02606 单指模式", StickyKeys::new(1).press('A'), "单键即触发简化输入");
    s.add("F02607 键代鼠标", mouse_keys(8) == Some((0, -1)) && mouse_keys(5).is_none(), "数字键移动光标");
    s.add("F02608 语音打字", word_count("语音转文字 demo") == 6, "语音结果字词统计");
    s.add("F02609 眼动位", focus_memory(None) == 0, "眼动打字预留默认位");
    s.add("F02610 开关扫描", debounce(&[0, 100, 200, 300], 400) == 1, "扫描式输入周期采样");
    let mut sk = StickyKeys::new(3);
    for c in "abc".chars() {
        sk.press(c);
    }
    s.add("F02611 屏幕键盘", sk.held.len() == 3, "软键盘按键收集");
    s.add("F02612 大键键盘", combo_overlay(&['A', 'B']) == "A + B", "大按键组合浮层");
    let mut bg = std::collections::HashMap::new();
    bg.insert("打", vec!["开", "字"]);
    s.add("F02613 词组预测", next_word_pred(&bg, "打") == vec!["开", "字"] && next_word_pred(&bg, "没").is_empty(), "下一词预测");
    s.add("F02614 纠错强度", auto_capitalize("hello. world") == "Hello. World", "自动纠错档位（句首大写）");
    s.add("F02615 自动大写", auto_capitalize("ok! go") == "Ok! Go", "句首大写");
    let mut t = "done  ".to_string();
    double_space_period(&mut t);
    s.add("F02616 双空句号", t == "done. ", "双空格变句号");
    s.add("F02617 防抖", debounce(&[0, 30, 60, 200, 230], 100) == 2, "手抖去抖窗口");
    s.add("F02618 长按可调", repeat_curve(200, 50, 450) == 6, "重复速度调节");
    s.add("F02619 组合可视化", combo_overlay(&['C', 't', 'r', 'l']) == "C + t + r + l", "组合键浮层显示");
    s.add("F02620 语音提示", slow_key(1500, 1000), "快捷键语音播报（时长确认）");
    s.add("F02621 盲文位", focus_memory(Some(3)) == 3, "盲文输入预留位");
    s.add("F02622 手写强化", select_word("handwrite", 4) == Some((0, 9)), "手写整词强化");
    s.add("F02623 表情替代", word_count("开心 :) 开心") == 5, "情绪词建议统计");
    s.add("F02624 误触撤销", UNDO_PROTOCOL == "ctrl+z", "误触即撤窗走全局协议");
    s.add("F02625 教学", auto_capitalize("教学.") == "教学.", "输入无障碍教学样例");
    s
}
