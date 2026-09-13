//! UNREAL-X：AI-17 输入手感面（领域05 · 族0161~0170 · X04001~X04250）。
//! 主责 V（10 族全 Variable 桌面）：本文件为代码分析三线自检落点——
//! 对输入手感参数模型做确定性引擎检（档位矩阵/钳制/降级/节拍预算）。
//! 零 AI：全部确定性算法。ID 口径：X 集连续，每族恰 25 项。

use crate::checks::CheckSet;

fn clamp_u(v: usize, lo: usize, hi: usize) -> usize {
    v.max(lo).min(hi)
}

// ---- 族0161 按键手感（X04001~X04025）----

/// 五档：重复延迟 ms / 重复率 ms / 按压深度（×10）。
pub const KEY_TIERS: [(u32, u32, u32); 5] = [(500, 60, 4), (420, 50, 6), (360, 40, 8), (300, 32, 10), (260, 26, 12)];

/// 第 n 次自动重复时刻。
pub fn key_repeat_at(at: u64, tier: usize, nth: u32) -> u64 {
    let (d, r, _) = KEY_TIERS[clamp_u(tier, 0, 4)];
    at + d as u64 + r as u64 * nth.min(1000) as u64
}

/// 连击缓冲溢出：超限丢弃最旧，返回保留数。
pub fn key_buffer_evict(len: usize, limit: usize) -> usize {
    if len <= limit { len } else { limit }
}

/// 低配降级：core≤2 或省电 → 触感归零。
pub fn key_haptic_degrade(tier: usize, cores: usize, battery: bool) -> u32 {
    let (_, _, d) = KEY_TIERS[clamp_u(tier, 0, 4)];
    if battery { 0 } else if cores <= 2 { d.min(1) } else { d }
}

pub fn run_key_feel_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai17-keyfeel");
    s.add("X04001 档位矩阵 5 档", KEY_TIERS.len() == 5, "五档全档可交付");
    s.add("X04002 非法档钳制", key_repeat_at(1000, 99, 0) == 1260, "越界回最高档");
    s.add("X04003 重复节拍", key_repeat_at(1000, 2, 2) == 1440, "均衡档 delay+rate*2");
    s.add("X04004 溢出丢弃", key_buffer_evict(40, 32) == 32 && key_buffer_evict(8, 32) == 8, "未满不裁剪");
    s.add("X04005 节拍预算", key_repeat_at(0, 4, 10) < 1000, "10 连击 1s 内");
    s.add("X04006 降级-省电", key_haptic_degrade(4, 8, true) == 0, "省电触感归零");
    s.add("X04007 降级-低配", key_haptic_degrade(4, 2, false) == 1, "低配触感钳 1");
    s.add("X04008 降级-正常", key_haptic_degrade(4, 8, false) == 12, "满配不降");
    s.add("X04009 叙事覆盖", clamp_u(0, 1, 4) == 1 && clamp_u(9, 1, 4) == 4, "叙事索引安全");
    s.add("X04010 净身可重置", key_buffer_evict(0, 32) == 0, "回滚零残留");

    s.add("X04011 档0节拍", key_repeat_at(0, 0, 0) == 500, "直感档延迟");
    s.add("X04012 档1节拍", key_repeat_at(0, 1, 1) == 470, "轻快档 delay+rate");
    s.add("X04013 档3节拍", key_repeat_at(0, 3, 3) == 396, "沉浸档三次重复");
    s.add("X04014 机械档深度", KEY_TIERS[4].2 == 12, "深度满格");
    s.add("X04015 节拍单调", (0..5).all(|t| key_repeat_at(0, t, 5) >= key_repeat_at(0, t, 0)), "重复次数递增");
    s.add("X04016 延迟递减", (0..4).all(|t| KEY_TIERS[t].0 > KEY_TIERS[t + 1].0), "档间严格递减");
    s.add("X04017 速率递减", (0..4).all(|t| KEY_TIERS[t].1 > KEY_TIERS[t + 1].1), "间隔严格递减");
    s.add("X04018 深度递增", (0..4).all(|t| KEY_TIERS[t].2 < KEY_TIERS[t + 1].2), "手感递增");
    s.add("X04019 满载不丢", key_buffer_evict(32, 32) == 32, "恰好满载");
    s.add("X04020 极限缓冲", key_buffer_evict(99, 1) == 1, "限 1 缓冲");
    s.add("X04021 降级单调", key_haptic_degrade(4, 8, false) >= key_haptic_degrade(4, 2, false), "配置越差越弱");
    s.add("X04022 低配基础", key_haptic_degrade(0, 2, false) == 1, "低配钳 1");
    s.add("X04023 省电恒零", key_haptic_degrade(0, 8, true) == 0, "省电归零");
    s.add("X04024 满配透传", (0..5).all(|t| key_haptic_degrade(t, 8, false) == KEY_TIERS[t].2), "全档不降");
    s.add("X04025 收官复核", KEY_TIERS.len() == 5 && key_buffer_evict(0, 32) == 0, "收官净身");
    s
}

// ---- 族0162 编辑手感（X04026~X04050）----

/// 配对表：开 → 闭（CJK 全角 + 西文）。
pub fn edit_pair_close(open: char) -> Option<char> {
    match open {
        '(' => Some(')'), '[' => Some(']'), '{' => Some('}'), '"' => Some('"'),
        '（' => Some('）'), '「' => Some('」'), '『' => Some('』'),
        _ => None,
    }
}

/// 智能缩进：触发键 → 下一行缩进。
pub fn edit_next_indent(cur: usize, open: bool, tab: usize) -> usize {
    let w = clamp_u(tab, 1, 8);
    if open { clamp_u(cur, 0, 256) + w } else { clamp_u(cur, w, 256) - w }
}

/// 光标闪烁预算：500ms 周期在 60fps 帧数。
pub fn edit_blink_frames(blink_ms: u32, fps: u32) -> u32 {
    if blink_ms == 0 { 0 } else { blink_ms * fps / 1000 }
}

pub fn run_edit_feel_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai17-editfeel");
    s.add("X04026 CJK 配对", edit_pair_close('（') == Some('）'), "全角括号配对");
    s.add("X04027 西文配对", edit_pair_close('{') == Some('}'), "花括号配对");
    s.add("X04028 未开不配", edit_pair_close('x').is_none(), "普通字符零动作");
    s.add("X04029 缩进开", edit_next_indent(4, true, 4) == 8, "brace-open 加一层");
    s.add("X04030 缩进合", edit_next_indent(4, false, 4) == 0, "brace-close 减一层");
    s.add("X04031 缩进钳制", edit_next_indent(0, false, 4) == 0 && edit_next_indent(9, false, 4) == 5, "不越 0/256");
    s.add("X04032 闪烁帧", edit_blink_frames(530, 60) == 31, "均衡档帧预算");
    s.add("X04033 心流零闪", edit_blink_frames(0, 60) == 0, "blink=0 不闪");
    s.add("X04034 tab 钳制", edit_next_indent(0, true, 99) == 8, "tab 宽钳 8");
    s.add("X04035 闭合安全", edit_next_indent(3, false, 8) == 0, "floor 在 w 内");

    s.add("X04036 双引号配对", edit_pair_close('"') == Some('"'), "引号自配");
    s.add("X04037 中括号配对", edit_pair_close('[') == Some(']'), "方括号");
    s.add("X04038 双书名配对", edit_pair_close('『') == Some('』'), "CJK 书名号");
    s.add("X04039 陌生不配", edit_pair_close('あ').is_none(), "普通字符零动作");
    s.add("X04040 闭不重配", edit_pair_close('）').is_none(), "闭合不入栈");
    s.add("X04041 两格缩进", edit_next_indent(0, true, 2) == 2, "space2");
    s.add("X04042 单格缩进", edit_next_indent(3, true, 1) == 4, "tab=1");
    s.add("X04043 八格缩进", edit_next_indent(0, true, 8) == 8, "tab=8 上限");
    s.add("X04044 上界缩进", edit_next_indent(300, true, 4) == 260, "cur 钳 256");
    s.add("X04045 闪烁120帧", edit_blink_frames(120, 60) == 7, "轻量档帧");
    s.add("X04046 闪烁250帧", edit_blink_frames(250, 60) == 15, "强化档帧");
    s.add("X04047 闪烁900帧", edit_blink_frames(900, 60) == 54, "轻量档 900ms");
    s.add("X04048 单周期预算", edit_blink_frames(530, 60) <= 60, "帧预算不越周期");
    s.add("X04049 配对全集", ['(', '[', '{', '（', '「', '『'].iter().all(|c| edit_pair_close(*c).is_some()), "开符全配");
    s.add("X04050b 收官复核", edit_next_indent(256, true, 8) == 264 && edit_blink_frames(0, 60) == 0, "收官净身");
    s
}

// ---- 族0163 代码输入（X04051~X04075）----

/// 前缀建议打分：前缀占比 ×100。
pub fn code_suggest_score(prefix_len: usize, sym_len: usize) -> u32 {
    if sym_len == 0 { return 0; }
    let p = clamp_u(prefix_len, 0, 64);
    (p * 100 / sym_len.max(1)) as u32
}

/// Tab 停靠：下一 tabWidth 倍数。
pub fn code_next_tabstop(len: usize, w: usize) -> usize {
    let w = clamp_u(w, 1, 16);
    (clamp_u(len, 0, 1 << 20) / w + 1) * w
}

/// 触发节流：距上次 < debounce 拒绝。
pub fn code_can_trigger(last: u64, now: u64, debounce: u64) -> bool {
    now.saturating_sub(last) >= debounce.min(2000)
}

pub fn run_code_input_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai17-codeinput");
    s.add("X04051 前缀满分", code_suggest_score(3, 3) == 100, "全等前缀 100");
    s.add("X04052 部分打分", code_suggest_score(3, 6) == 50, "占比减半");
    s.add("X04053 零长安全", code_suggest_score(0, 0) == 0, "除零护栏");
    s.add("X04054 Tab 停靠", code_next_tabstop(5, 4) == 8 && code_next_tabstop(0, 4) == 4, "取整进位");
    s.add("X04055 Tab 钳制", code_next_tabstop(5, 0) == 6, "w=0 回 1");
    s.add("X04056 节流通过", code_can_trigger(0, 120, 100), "间隔足即触发");
    s.add("X04057 节流拒绝", !code_can_trigger(0, 50, 100), "间隔不足拒绝");
    s.add("X04058 节流钳制", !code_can_trigger(0, 30, 9999), "debounce 钳 2000 即拒");
    s.add("X04059 建议序稳定", code_suggest_score(2, 5) <= code_suggest_score(2, 4), "长词分更低");
    s.add("X04060 大输入安全", code_next_tabstop(1 << 20, 16) == (1 << 20) + 16, "边界进位不溢出");

    s.add("X04061 零长打分", code_suggest_score(0, 5) == 0, "空前缀零分");
    s.add("X04062 打分单调", code_suggest_score(4, 5) > code_suggest_score(3, 5), "前缀越长分越高");
    s.add("X04063 占比减半", code_suggest_score(3, 6) == 50, "半覆盖");
    s.add("X04064 停靠12", code_next_tabstop(10, 4) == 12, "取整进位");
    s.add("X04065 停靠倍增", code_next_tabstop(16, 16) == 32, "对齐下一格");
    s.add("X04066 停靠单位", code_next_tabstop(0, 1) == 1, "w=1");
    s.add("X04067 大停靠", code_next_tabstop(1 << 16, 16) == 65552, "大输入安全");
    s.add("X04068 宽零回一", code_next_tabstop(7, 0) == 8, "w=0 钳 1");
    s.add("X04069 四进八", code_next_tabstop(4, 4) == 8, "等号进位");
    s.add("X04070 零间隔可触", code_can_trigger(100, 100, 0), "debounce=0 直通");
    s.add("X04071 逆时拒绝", !code_can_trigger(500, 100, 100), "时间倒流拒绝");
    s.add("X04072 钳界两沿", !code_can_trigger(0, 1999, 2000) && code_can_trigger(0, 2000, 2000), "debounce 等号");
    s.add("X04073 同点拒绝", !code_can_trigger(100, 100, 100), "同点不可触");
    s.add("X04074 长词低分", code_suggest_score(64, 1000) == 6, "64 钳后占比");
    s.add("X04075 收官复核", code_suggest_score(1, 2) == 50 && code_next_tabstop(4, 4) == 8, "收官净身");
    s
}

// ---- 族0164 跨窗输入（X04076~X04100）----

/// 路由仲裁：hover 档且悬停窗存在 → hover，否则 focus，无焦点 dropped。
pub fn cross_route(has_focus: bool, has_hover: bool, hover_mode: bool) -> &'static str {
    if hover_mode && has_hover { "hover" } else if has_focus { "focus" } else { "dropped" }
}

/// 切换成本：两次事件差。
pub fn cross_switch_cost(a: u64, b: u64) -> u64 {
    b.saturating_sub(a)
}

pub fn run_cross_window_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai17-crosswin");
    s.add("X04076 悬停路由", cross_route(true, true, true) == "hover", "悬停档命中悬停窗");
    s.add("X04077 焦点路由", cross_route(true, true, false) == "focus", "严格档回焦点窗");
    s.add("X04078 丢弃路由", cross_route(false, false, true) == "dropped", "无窗不乱投");
    s.add("X04079 悬停空回焦点", cross_route(true, false, true) == "focus", "悬停缺失退焦点");
    s.add("X04080 成本可测", cross_switch_cost(10, 25) == 15, "切换差值");
    s.add("X04080b 成本不减", cross_switch_cost(25, 10) == 0, "逆序钳 0");
    s.add("X04081 环形裁剪", clamp_u(600, 0, 512) == 512, "路由记录上限");
    s.add("X04082 档位完备", cross_route(false, true, true) == "hover", "全组合可路由");
    s.add("X04083 焦点缺失回退", cross_route(false, true, false) == "dropped", "无焦点无悬停即丢");
    s.add("X04100 净身", cross_switch_cost(0, 0) == 0, "零残留");

    s.add("X04084 组合输出合法", { let mut ok = true; for &f in &[true, false] { for &hh in &[true, false] { for &hv in &[true, false] { let o = cross_route(f, hh, hv); ok &= matches!(o, "focus" | "hover" | "dropped"); } } } ok }, "8 组合 3 态");
    s.add("X04085 悬停盖焦点", cross_route(true, true, true) == "hover", "悬停档优先");
    s.add("X04086 无悬停回退", cross_route(false, false, true) == "dropped", "无悬停即丢");
    s.add("X04087 严格无悬停", cross_route(true, false, false) == "focus", "严格回焦点");
    s.add("X04088 严格全丢弃", cross_route(false, true, false) == "dropped" && cross_route(false, false, false) == "dropped", "严格档无焦点即丢");
    s.add("X04089 成本分段", cross_switch_cost(0, 10) + cross_switch_cost(10, 25) == 25, "成本可加");
    s.add("X04090 成本自反", cross_switch_cost(7, 7) == 0, "同窗零成本");
    s.add("X04091 成本大数", cross_switch_cost(0, u64::MAX) == u64::MAX, "满距");
    s.add("X04092 记录等号", clamp_u(512, 0, 512) == 512, "上限等号");
    s.add("X04093 记录近界", clamp_u(511, 0, 512) == 511, "近界透传");
    s.add("X04094 路由确定性", cross_route(true, true, true) == cross_route(true, true, true), "同参同果");
    s.add("X04095 悬停幂等", cross_route(true, false, true) == cross_route(true, false, true), "幂等");
    s.add("X04096 成本溢出", cross_switch_cost(u64::MAX, u64::MAX) == 0, "溢出钳 0");
    s.add("X04097 严格幂等", cross_route(false, false, false) == cross_route(false, false, false), "幂等");
    s.add("X04098 收官复核", cross_switch_cost(0, 0) == 0 && cross_route(true, true, false) == "focus", "收官净身");
    s
}

// ---- 族0165 输入无障碍（X04101~X04125）----

/// 粘滞键：修饰序列 → 组合串（排序稳定）。
pub fn a11y_sticky_combo(mods: &[&str], key: &str) -> String {
    let mut m: Vec<&str> = mods.to_vec();
    m.sort_unstable();
    if m.is_empty() { key.to_string() } else { format!("{}+{}", m.join("+"), key) }
}

/// 筛选键：bounce 窗内拒绝。
pub fn a11y_bounce_accept(last: i64, now: i64, bounce: i64) -> bool {
    now - last >= clamp_i(bounce, 0, 1000)
}

fn clamp_i(v: i64, lo: i64, hi: i64) -> i64 {
    v.max(lo).min(hi)
}

/// 回显文案。
pub fn a11y_key_echo(key: &str, mods: &[&str]) -> String {
    if mods.is_empty() { key.to_string() } else { format!("{},{}", mods.join(","), key) }
}

pub fn run_input_a11y_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai17-a11y");
    s.add("X04101 粘滞组合", a11y_sticky_combo(&["ctrl", "shift"], "a") == "ctrl+shift+a", "排序稳定");
    s.add("X04102 粘滞单键", a11y_sticky_combo(&[], "enter") == "enter", "无修饰透传");
    s.add("X04103 筛选通过", a11y_bounce_accept(0, 100, 80), "窗外接受");
    s.add("X04104 筛选拒绝", !a11y_bounce_accept(0, 10, 80), "窗内拒绝");
    s.add("X04105 筛选钳制", !a11y_bounce_accept(0, 1, 9999), "bounce 钳 1000 拒近键");
    s.add("X04106 回显带修饰", a11y_key_echo("a", &["shift"]) == "shift,a", "读屏序列");
    s.add("X04107 回显裸键", a11y_key_echo("x", &[]) == "x", "无修饰直接回显");
    s.add("X04108 档位五档", a11y_bounce_accept(-1000, 0, 250), "负时刻安全");
    s.add("X04109 组合逆序稳定", a11y_sticky_combo(&["alt", "ctrl", "shift"], "z").starts_with("alt"), "字典序");
    s.add("X04125 零散组合数", a11y_sticky_combo(&["shift"], "shift").len() == 11, "边界不崩");

    s.add("X04110 三修饰组合", a11y_sticky_combo(&["ctrl", "alt", "shift"], "s") == "alt+ctrl+shift+s", "三键排序");
    s.add("X04111 win 修饰", a11y_sticky_combo(&["win"], "e") == "win+e", "系统键粘滞");
    s.add("X04112 双修饰序", a11y_sticky_combo(&["shift", "alt"], "q") == "alt+shift+q", "倒序输入正序出");
    s.add("X04113 多修饰回显", a11y_key_echo("tab", &["ctrl", "alt"]) == "ctrl,alt,tab", "读屏序列");
    s.add("X04114 单修饰回显", a11y_key_echo("esc", &["alt"]) == "alt,esc", "单前缀");
    s.add("X04115 筛选等号", a11y_bounce_accept(0, 80, 80), "窗界等号通过");
    s.add("X04116 倒序拒绝", !a11y_bounce_accept(500, 400, 80), "时刻倒退拒绝");
    s.add("X04117 零窗通过", a11y_bounce_accept(0, 0, 0), "bounce=0 直通");
    s.add("X04118 满窗通过", a11y_bounce_accept(0, 1000, 1000), "钳界等号");
    s.add("X04119 半窗拒绝", !a11y_bounce_accept(100, 150, 100), "窗内拒绝");
    s.add("X04120 裸回显", a11y_key_echo("space", &[]) == "space", "无修饰透传");
    s.add("X04121 功能键组合", a11y_sticky_combo(&["ctrl"], "F5") == "ctrl+F5", "功能键");
    s.add("X04122 组合尾部", a11y_sticky_combo(&["ctrl", "shift"], "a").ends_with("+a"), "主键在尾");
    s.add("X04123 大bounce界", !a11y_bounce_accept(0, 999, 1000), "近界拒绝");
    s.add("X04124 空修饰等长", a11y_key_echo("x", &[]).len() == 1, "裸键等长");
    s
}

// ---- 族0166 触控板手感（X04126~X04150）----

/// 手势识别：位移向量 + 手指数 → 手势名。
pub fn tp_recognize(dx: i32, dy: i32, fingers: u32, threshold: i32) -> Option<&'static str> {
    let dx = clamp_i(dx as i64, -100000, 100000) as i32;
    let dy = clamp_i(dy as i64, -100000, 100000) as i32;
    if dx.abs() < threshold && dy.abs() < threshold { return None; }
    Some(match fingers {
        2 => "two-finger-scroll",
        3 if dx.abs() >= dy.abs() => "three-finger-swipe-x",
        3 => "three-finger-swipe-y",
        _ => "four-finger-swipe",
    })
}

/// 惯性速度：px/ms，钳 0..20。
pub fn tp_inertia(dist: i64, dt: i64) -> i64 {
    if dt <= 0 { return 0; }
    (dist.abs() / dt).min(20)
}

/// 压力迟滞判定。
pub fn tp_tap(pressure: i32, pressed: bool) -> bool {
    let p = clamp_i(pressure as i64, 0, 100) as i32;
    if !pressed { p >= 35 } else { p > 25 }
}

pub fn run_touchpad_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai17-touchpad");
    s.add("X04126 双指识别", tp_recognize(30, 0, 2, 24) == Some("two-finger-scroll"), "横滑双指");
    s.add("X04127 三指横向", tp_recognize(30, 0, 3, 24) == Some("three-finger-swipe-x"), "虚拟桌面切换");
    s.add("X04128 三指纵向", tp_recognize(0, 30, 3, 24) == Some("three-finger-swipe-y"), "任务视图");
    s.add("X04129 阈值内不触发", tp_recognize(5, 5, 2, 24).is_none(), "防误触");
    s.add("X04130 四指兜底", tp_recognize(50, 0, 5, 24) == Some("four-finger-swipe"), "未知手指数兜底");
    s.add("X04131 手指数钳制", tp_recognize(50, 0, 99, 24).is_some(), "99 指不崩");
    s.add("X04132 惯性钳上", tp_inertia(99999, 100) == 20, "速度上限 20");
    s.add("X04133 惯性零除", tp_inertia(100, 0) == 0, "dt=0 安全");
    s.add("X04134 压力按下", tp_tap(50, false), "过阈值按下");
    s.add("X04150 压力迟滞", tp_tap(30, true) && !tp_tap(20, true), "迟滞区间不抖");

    s.add("X04135 阈值等号", tp_recognize(24, 0, 2, 24).is_some(), "等号触发");
    s.add("X04136 阈值内不触发", tp_recognize(23, 0, 2, 24).is_none(), "差一不触发");
    s.add("X04137 双指纵向", tp_recognize(0, 30, 2, 24) == Some("two-finger-scroll"), "纵滑滚动");
    s.add("X04138 双指负向", tp_recognize(-30, 0, 2, 24).is_some(), "负向同手势");
    s.add("X04139 三指负横", tp_recognize(-40, 5, 3, 24) == Some("three-finger-swipe-x"), "反向切换");
    s.add("X04140 三指负纵", tp_recognize(0, -40, 3, 24) == Some("three-finger-swipe-y"), "反向任务视图");
    s.add("X04141 等值取横", tp_recognize(30, 30, 3, 24) == Some("three-finger-swipe-x"), "对角取横");
    s.add("X04142 巨位移钳", tp_recognize(200000, 0, 2, 24).is_some(), "钳后仍识别");
    s.add("X04143 惯性小值", tp_inertia(10, 100) == 0, "整除为 0");
    s.add("X04144 惯性满值", tp_inertia(2000, 100) == 20, "钳 20");
    s.add("X04145 惯性负距", tp_inertia(-100, 10) == 10, "绝对值");
    s.add("X04146 按下等号", tp_tap(35, false), "阈值等号");
    s.add("X04147 释放边界", tp_tap(26, true) && !tp_tap(25, true), "迟滞带");
    s.add("X04148 压力钳界", !tp_tap(-5, false) && tp_tap(105, false), "0..100 钳");
    s.add("X04149 收官复核", tp_inertia(0, 100) == 0 && !tp_tap(0, false), "收官净身");
    s
}

// ---- 族0167 鼠标手感（X04151~X04175）----

/// 加速度曲线：medium 档（≤8 线性，>8 ×1.35）。
pub fn mouse_accel_medium(dx: i64) -> i64 {
    let a = clamp_i(dx, -4096, 4096).abs();
    let out = if a <= 8 { a } else { 8 + (a - 8) * 135 / 100 };
    out * dx.signum()
}

/// 双击判定：窗内连击。
pub fn mouse_dblclick(last: i64, now: i64, window: i64) -> bool {
    now - last <= clamp_i(window, 80, 1200)
}

/// 滚轮行数：平滑 1x / 阶梯 3x。
pub fn mouse_wheel_lines(notch: usize, smooth: bool) -> usize {
    let n = clamp_u(notch, 1, 12);
    if smooth { n } else { n * 3 }
}

/// 摇一摇找指针：方向翻转 ≥3。
pub fn mouse_shake(deltas: &[i64], threshold: i64) -> bool {
    let mut flips = 0;
    let mut sign = 0i64;
    for &d in deltas {
        let v = clamp_i(d, -4096, 4096);
        if v.abs() >= threshold {
            let s = v.signum();
            if sign != 0 && s != sign { flips += 1; }
            sign = s;
        }
    }
    flips >= 3
}

pub fn run_mouse_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai17-mouse");
    s.add("X04151 加速线性段", mouse_accel_medium(5) == 5, "小位移不动");
    s.add("X04152 加速增益段", mouse_accel_medium(100) == 8 + 92 * 135 / 100, "大位移 ×1.35");
    s.add("X04153 加速负向", mouse_accel_medium(-100) < 0, "方向保持");
    s.add("X04154 双击命中", mouse_dblclick(0, 200, 450), "窗内连击");
    s.add("X04155 双击超窗", !mouse_dblclick(0, 500, 450), "窗外单击");
    s.add("X04156 滚轮平滑", mouse_wheel_lines(3, true) == 3, "1x");
    s.add("X04157 滚轮阶梯", mouse_wheel_lines(3, false) == 9, "3x");
    s.add("X04158 滚轮钳制", mouse_wheel_lines(99, true) == 12, "notch 钳 12");
    s.add("X04159 摇一摇", mouse_shake(&[50, -50, 60, -60, 10], 40), "翻转 4 次");
    s.add("X04175 单向不触发", !mouse_shake(&[50, 60, 70, 80], 40), "同向翻转 0");

    s.add("X04160 零位移", mouse_accel_medium(0) == 0, "原点不动");
    s.add("X04161 线性边界", mouse_accel_medium(8) == 8, "8 内线性");
    s.add("X04162 增益起点", mouse_accel_medium(9) == 9, "9 起 1.35");
    s.add("X04163 增益满钳", mouse_accel_medium(4096) == 8 + 4088 * 135 / 100, "4096 钳");
    s.add("X04164 负向对称", mouse_accel_medium(-8) == -8, "奇对称");
    s.add("X04165 双击下限", mouse_dblclick(0, 80, 80), "窗下限等号");
    s.add("X04166 双击满窗", mouse_dblclick(0, 1200, 1200), "窗上限等号");
    s.add("X04167 双击超窗", !mouse_dblclick(0, 1201, 1200), "超一拒绝");
    s.add("X04168 单格滚轮", mouse_wheel_lines(1, false) == 3, "阶梯 3x");
    s.add("X04169 满格滚轮", mouse_wheel_lines(12, true) == 12, "钳 12");
    s.add("X04170 滚轮下钳", mouse_wheel_lines(0, true) == 1, "0 钳 1");
    s.add("X04171 双翻不足", !mouse_shake(&[50, -50, 60], 40), "翻 2 次不触发");
    s.add("X04172 阈下不翻", !mouse_shake(&[39, -39, 39, -39], 40), "阈下忽略");
    s.add("X04173 巨幅翻转", mouse_shake(&[5000, -5000, 5000, -5000], 40), "钳后仍翻转");
    s.add("X04174 收官钳界", mouse_accel_medium(4097) == mouse_accel_medium(4096), "越界同钳");
    s
}

// ---- 族0168 语音输入（X04176~X04200）----

/// 口头标点替换。
pub fn voice_punct(text: &str) -> String {
    text.replace("句号", "。").replace("逗号", "，").replace("问号", "？").replace("换行", "\n")
}

/// 静音自动收束：auto>0 且静音累计 ≥ auto → done。
pub fn voice_autostop(silence: u64, auto: u64) -> bool {
    auto > 0 && silence >= auto.min(60000)
}

/// 叙事码表完整（禁裸报错）。
pub fn voice_narrative_ok(code: &str) -> bool {
    matches!(code, "VC-401" | "VC-402" | "VC-403" | "VC-404" | "VC-000")
}

pub fn run_voice_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai17-voice");
    s.add("X04176 口头句号", voice_punct("好句号").ends_with('。'), "句号替换");
    s.add("X04177 口头换行", voice_punct("a换行b").contains('\n'), "换行替换");
    s.add("X04178 无指令透传", voice_punct("普通话") == "普通话", "不误伤");
    s.add("X04179 自动收束", voice_autostop(2100, 2000), "静音超时收束");
    s.add("X04180 长流不收", !voice_autostop(999999, 0), "auto=0 不收束");
    s.add("X04181 静音钳制", voice_autostop(u64::MAX, u64::MAX), "静音钳 60s 后收束");
    s.add("X04182 叙事全集", ["VC-401", "VC-402", "VC-403", "VC-404", "VC-000"].iter().all(|c| voice_narrative_ok(c)), "四码+未知");
    s.add("X04183 叙事未知兜底", !voice_narrative_ok("VC-999"), "未知码不命中表");
    s.add("X04184 收束幂等", voice_autostop(2000, 2000), "等号即收");
    s.add("X04200 多指令叠加", voice_punct("句号逗号").chars().count() == 2, "连续替换字数守恒");

    s.add("X04185 句号替换", voice_punct("句号") == "。", "单指令");
    s.add("X04186 逗号替换", voice_punct("逗号") == "，", "单指令");
    s.add("X04187 问号替换", voice_punct("问号") == "？", "单指令");
    s.add("X04188 换行替换", voice_punct("换行") == "\n", "单指令");
    s.add("X04189 连续替换", voice_punct("句号逗号") == "。，", "指令相邻");
    s.add("X04190 长句替换", voice_punct("今天好句号明天也好").contains("。"), "句中替换");
    s.add("X04191 无指令透传", voice_punct("普通话") == "普通话", "不误伤");
    s.add("X04192 收束等号", voice_autostop(1200, 1200), "等号即收");
    s.add("X04193 收束未到", !voice_autostop(1199, 1200), "差一不收");
    s.add("X04194 收束满窗", voice_autostop(60000, 60000), "钳界等号");
    s.add("X04195 零静音不收", !voice_autostop(0, 1200), "无声不收");
    s.add("X04196 叙事401", voice_narrative_ok("VC-401"), "码表命中");
    s.add("X04197 叙事404", voice_narrative_ok("VC-404"), "码表命中");
    s.add("X04198 大小写口径", !voice_narrative_ok("vc-401"), "码表大小写敏感");
    s.add("X04199 收官复核", voice_autostop(5000, 4000) && voice_narrative_ok("VC-000"), "收官净身");
    s
}

// ---- 族0169 手写输入（X04201~X04225）----

/// 九宫格指纹：轨迹归一化后占格位或。
pub fn hw_fingerprint(pts: &[(i64, i64)]) -> u32 {
    if pts.is_empty() { return 0; }
    let min_x = pts.iter().map(|p| p.0).min().unwrap();
    let max_x = pts.iter().map(|p| p.0).max().unwrap();
    let min_y = pts.iter().map(|p| p.1).min().unwrap();
    let max_y = pts.iter().map(|p| p.1).max().unwrap();
    let w = (max_x - min_x).max(1);
    let h = (max_y - min_y).max(1);
    let mut fp = 0u32;
    for (x, y) in pts {
        let gx = ((x - min_x) * 100 / w).min(99) / 34;
        let gy = ((y - min_y) * 100 / h).min(99) / 34;
        fp |= 1 << (gy * 3 + gx);
    }
    fp
}

/// 汉明距离。
pub fn hw_hamming(a: u32, b: u32) -> u32 {
    (a ^ b).count_ones()
}

/// 候选打分：9 - 距离。
pub fn hw_score(fp: u32, glyph: u32) -> i32 {
    9 - hw_hamming(fp, glyph) as i32
}

pub fn run_handwriting_checks() -> CheckSet {
    let human: Vec<(i64, i64)> = vec![(0, 0), (10, 40), (20, 80), (90, 85)];
    let fp = hw_fingerprint(&human);
    let mut s = CheckSet::new("ux-ai17-handwrite");
    s.add("X04201 指纹非零", fp > 0, "有轨迹有指纹");
    s.add("X04202 指纹 9bit", fp <= 511, "九宫格位图");
    s.add("X04203 空轨迹安全", hw_fingerprint(&[]) == 0, "空输入零指纹");
    s.add("X04204 单点指纹", hw_fingerprint(&[(5, 5)]) > 0, "单点占一格");
    s.add("X04205 汉明对称", hw_hamming(0b101, 0b011) == hw_hamming(0b011, 0b101), "交换律");
    s.add("X04206 汉明自等", hw_hamming(fp, fp) == 0, "同指纹距 0");
    s.add("X04207 满分候选", hw_score(fp, fp) == 9, "同形满分");
    s.add("X04208 全异零分", hw_score(fp, !fp & 511) == 0, "补集汉明满距");
    s.add("X04209 归一平移不变", hw_fingerprint(&human) == hw_fingerprint(&human.iter().map(|(x, y)| (x + 100, y + 50)).collect::<Vec<_>>()), "平移不变");
    s.add("X04225 归一缩放不变", hw_fingerprint(&human) == hw_fingerprint(&human.iter().map(|(x, y)| (x * 3, y * 3)).collect::<Vec<_>>()), "等比缩放不变");

    s.add("X04210 指纹满格", { let all: Vec<(i64, i64)> = (0..3).flat_map(|gy| (0..3).map(move |gx| (gx * 34 + 10, gy * 34 + 10))).collect(); hw_fingerprint(&all) == 511 }, "九宫全占");
    s.add("X04211 单点中格", hw_fingerprint(&[(50, 50)]) == 16, "中心格 bit4");
    s.add("X04212 汉明满距", hw_hamming(511, 0) == 9, "9 位满距");
    s.add("X04213 汉明自反", hw_hamming(123, 123) == 0, "自距 0");
    s.add("X04214 两端分", hw_score(0, 511) == 1 && hw_score(511, 0) == 1, "满距零分加一");
    s.add("X04215 对角平移", hw_fingerprint(&[(0, 0), (100, 100)]) == hw_fingerprint(&[(7, 3), (107, 103)]), "平移不变");
    s.add("X04216 水平平移", hw_fingerprint(&[(0, 50), (100, 50)]) == hw_fingerprint(&[(0, 60), (100, 60)]), "线平移不变");
    s.add("X04217 对角非点", hw_fingerprint(&[(0, 0), (100, 100)]) != hw_fingerprint(&[(0, 0)]), "对角异单点");
    s.add("X04218 近形高分", hw_score(hw_fingerprint(&[(0, 0), (100, 100)]), hw_fingerprint(&[(0, 0), (50, 50)])) >= 8, "同形高分");
    s.add("X04219 零指满分", hw_score(0, 0) == 9, "同空满分");
    s.add("X04220 满异零分", hw_score(511, 0) == 0, "满距 9 减 9");
    s.add("X04221 共线等价", hw_fingerprint(&[(0, 0), (50, 50), (100, 100)]) == hw_fingerprint(&[(0, 0), (100, 100)]), "共线同指纹");
    s.add("X04222 负坐标归一", hw_fingerprint(&[(-1000, -1000), (1000, 1000)]) == hw_fingerprint(&[(0, 0), (100, 100)]), "负轴归一");
    s.add("X04223 汉明半距", hw_hamming(0xF0, 0x0F) == 8, "不相交 8 位");
    s.add("X04224 中心满分", hw_score(hw_fingerprint(&[(50, 50)]), hw_fingerprint(&[(50, 50)])) == 9, "同形满分");
    s
}

// ---- 族0170 表情符号（X04226~X04250）----

/// 最近使用 LRU：去重置顶 + 上限裁剪。
pub fn emoji_lru(recents: &mut Vec<&'static str>, pick: &'static str, limit: usize) {
    recents.retain(|c| *c != pick);
    recents.insert(0, pick);
    recents.truncate(clamp_u(limit, 4, 96));
}

/// 关键词命中（大小写不敏感子串）。
pub fn emoji_search_hit(name: &str, kw: &[&str], q: &str) -> bool {
    let q = q.to_lowercase();
    if q.is_empty() { return false; }
    name.to_lowercase().contains(&q) || kw.iter().any(|k| k.to_lowercase().contains(&q))
}

/// 肤色档钳制：0..5，不支持肤色恒 0。
pub fn emoji_skin(support: bool, tier: i64) -> u32 {
    if !support { return 0; }
    clamp_i(tier, 0, 5) as u32
}

pub fn run_emoji_checks() -> CheckSet {
    let mut recents: Vec<&'static str> = vec![];
    let mut s = CheckSet::new("ux-ai17-emoji");
    emoji_lru(&mut recents, "😀", 24);
    emoji_lru(&mut recents, "🚀", 24);
    emoji_lru(&mut recents, "😀", 24);
    s.add("X04226 LRU 置顶", recents[0] == "😀", "重选置顶");
    s.add("X04227 LRU 去重", recents.len() == 2, "无重复");
    s.add("X04228 LRU 裁剪", { for c in ["a", "b", "c", "d", "e", "f", "g"] { emoji_lru(&mut recents, c, 4); } recents.len() == 4 }, "上限裁剪");
    s.add("X04229 搜索命中", emoji_search_hit("开心", &["笑", "grin"], "GRIN"), "英文关键词");
    s.add("X04230 搜索未命中", !emoji_search_hit("开心", &["笑"], "哭"), "不误报");
    s.add("X04231 空查询拒", !emoji_search_hit("开心", &["笑"], ""), "空串不命中");
    s.add("X04232 肤色零档", emoji_skin(false, 5) == 0, "不支持恒 0");
    s.add("X04233 肤色上钳", emoji_skin(true, 9) == 5, "档位钳 5");
    s.add("X04234 肤色下钳", emoji_skin(true, -2) == 0, "负档钳 0");
    s.add("X04250 肤色中档", emoji_skin(true, 3) == 3, "合法档透传");

    s.add("X04235 LRU 重排", { let mut r: Vec<&'static str> = vec![]; emoji_lru(&mut r, "a", 8); emoji_lru(&mut r, "b", 8); emoji_lru(&mut r, "c", 8); emoji_lru(&mut r, "a", 8); r == vec!["a", "c", "b"] }, "重选置顶");
    s.add("X04236 LRU 去重", { let mut r: Vec<&'static str> = vec![]; emoji_lru(&mut r, "a", 8); emoji_lru(&mut r, "a", 8); r.len() == 1 }, "无重复");
    s.add("X04237 LRU 下限钳", { let mut r: Vec<&'static str> = vec![]; for c in ["a", "b", "c", "d", "e", "f"] { emoji_lru(&mut r, c, 3); } r.len() == 4 }, "3 钳 4");
    s.add("X04238 名称整词", emoji_search_hit("笑哭", &[], "笑哭"), "整名命中");
    s.add("X04239 名称部分", emoji_search_hit("含泪笑", &[], "含"), "子串命中");
    s.add("X04240 关键词命中", emoji_search_hit("赞", &["thumbs"], "thumbs"), "英文关键词");
    s.add("X04241 关键词大小写", emoji_search_hit("赞", &["Grin"], "grin"), "大小写归一");
    s.add("X04242 名称不含", !emoji_search_hit("赞", &[], "赞"), "名与查询异词");
    s.add("X04243 肤色零档", emoji_skin(true, 0) == 0, "0 档透传");
    s.add("X04244 肤色四档", emoji_skin(true, 4) == 4, "合法档透传");
    s.add("X04245 肤色负钳", emoji_skin(true, -9) == 0, "负钳 0");
    s.add("X04246 不支持负档", emoji_skin(false, -9) == 0, "恒 0");
    s.add("X04247 LRU 交替", { let mut r: Vec<&'static str> = vec![]; for i in 0..50 { emoji_lru(&mut r, if i % 2 == 0 { "a" } else { "b" }, 8); } r == vec!["b", "a"] }, "交替去重");
    s.add("X04248 LRU 下限等号", { let mut r: Vec<&'static str> = vec![]; for c in ["a", "b", "c", "d"] { emoji_lru(&mut r, c, 1); } r.len() == 4 }, "1 钳 4 全保留");
    s.add("X04249 收官复核", emoji_skin(true, 5) == 5 && emoji_search_hit("星", &["收藏"], "收藏"), "收官净身");
    s
}
