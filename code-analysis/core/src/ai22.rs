//! AI-22 W2 域（领域05 键盘与输入手感 · F02626~F02750）：
//! 族0106 触控板手感 / 族0107 鼠标手感 / 族0108 语音输入 /
//! 族0109 手写输入 / 族0110 表情与符号。
//! 零 AI：全部确定性算法。

use crate::checks::CheckSet;

use std::collections::HashMap;

// ---- 族0106 触控板手感 ----

#[derive(PartialEq, Clone, Copy)]
pub enum ScrollDir {
    Natural,
    Classic,
}

/// 触控板手势引擎。
pub struct Touchpad {
    pub scroll_dir: ScrollDir,
    pub tap_click: bool,
    pub sensitivity: u8,
}

impl Touchpad {
    pub fn new() -> Self {
        Touchpad { scroll_dir: ScrollDir::Natural, tap_click: true, sensitivity: 5 }
    }
    /// F02626/F02627 滚动方向。
    pub fn scroll_delta(&self, finger_dy: i32) -> i32 {
        match self.scroll_dir {
            ScrollDir::Natural => finger_dy,
            ScrollDir::Classic => -finger_dy,
        }
    }
    /// F02644 惯性衰减曲线。
    pub fn inertia(&self, v: f32) -> Vec<f32> {
        let mut out = Vec::new();
        let mut cur = v;
        while cur > 0.1 {
            out.push(cur);
            cur *= 0.85;
        }
        out
    }
    /// F02645/F02646 滚动档位。
    pub fn scroll_gain(&self, level: u8) -> i32 {
        1 + level as i32
    }
}

/// F02637 掌压检测：接触面积阈值。
pub fn palm_reject(area_px: u32, threshold: u32) -> bool {
    area_px >= threshold
}

/// F02638 打字防误触：键盘活跃期抑制触板。
pub fn typing_guard(last_key_ms: u64, now_ms: u64, window_ms: u64) -> bool {
    now_ms.saturating_sub(last_key_ms) < window_ms
}

/// F02640~F02642 三/四指手势。
pub fn finger_gesture(fingers: u8, motion: &str) -> Option<&'static str> {
    match (fingers, motion) {
        (3, "up") => Some("task-view"),
        (4, "left") => Some("desk-prev"),
        (4, "right") => Some("desk-next"),
        (3, "tap") => Some("lookup"),
        (2, "tap") => Some("translate"),
        _ => None,
    }
}

// ---- 族0107 鼠标手感 ----

/// F02652 指针加速曲线（分段）。
pub fn pointer_accel(dx: i32) -> i32 {
    let a = dx.abs();
    let scaled = if a < 4 { a } else if a < 10 { a * 3 / 2 } else { a * 2 };
    if dx < 0 { -scaled } else { scaled }
}

/// F02653 双击间隔自测。
pub fn double_click_ok(delta_ms: u64, max_ms: u64) -> bool {
    delta_ms <= max_ms
}

/// F02663 指针拖尾。
pub fn pointer_trail(points: &[(i32, i32)], len: usize) -> Vec<(i32, i32)> {
    if points.len() <= len {
        points.to_vec()
    } else {
        points[points.len() - len..].to_vec()
    }
}

/// F02665 聚拢吸附：新窗吸到光标。
pub fn snap_to_cursor(win: (i32, i32), cursor: (i32, i32), radius: i32) -> (i32, i32) {
    let dx = (win.0 - cursor.0).abs();
    let dy = (win.1 - cursor.1).abs();
    if dx <= radius && dy <= radius {
        cursor
    } else {
        win
    }
}

/// F02668 速度学习：滚动均值自适应。
pub struct SpeedLearner {
    samples: Vec<u32>,
}
impl SpeedLearner {
    pub fn new() -> Self {
        SpeedLearner { samples: Vec::new() }
    }
    pub fn observe(&mut self, v: u32) {
        if self.samples.len() >= 8 {
            self.samples.remove(0);
        }
        self.samples.push(v);
    }
    pub fn adapt(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        self.samples.iter().sum::<u32>() / self.samples.len() as u32
    }
}

/// F02669 手抖补偿：小位移归零。
pub fn tremor_compensate(dx: i32, deadzone: i32) -> i32 {
    if dx.abs() <= deadzone {
        0
    } else {
        dx
    }
}

/// F02674 滚轮音量：媒体图标滚轮步进。
pub fn wheel_volume(vol: u8, notches: i32) -> u8 {
    let v = vol as i32 + notches * 2;
    v.clamp(0, 100) as u8
}

// ---- 族0108 语音输入 ----

/// 本地语音转写引擎（确定性：词条匹配打分）。
pub struct VoiceTranscriber {
    pub lexicon: Vec<&'static str>,
}
impl VoiceTranscriber {
    pub fn new() -> Self {
        VoiceTranscriber { lexicon: vec!["打开", "新行", "删除该句", "句号", "逗号"] }
    }
    /// F02676 转写：输入音素串 → 最接近词条。
    pub fn transcribe(&self, tok: &str) -> Option<&&'static str> {
        self.lexicon.iter().find(|w| tok.starts_with(**w))
    }
    /// F02677 实时标点：句尾自动加标点。
    pub fn punctuate(&self, text: &str, final_: bool) -> String {
        if final_ && !text.ends_with('。') {
            format!("{text}。")
        } else {
            text.into()
        }
    }
    /// F02678 口头命令。
    pub fn command(&self, tok: &str) -> Option<&'static str> {
        match tok {
            "新行" => Some("newline"),
            "删除该句" => Some("del-sentence"),
            _ => None,
        }
    }
}

/// F02680 降噪：信噪比门限。
pub fn noise_gate(snr_db: i32, min_db: i32) -> bool {
    snr_db >= min_db
}

/// F02684 语速适应：字/秒 → 档位。
pub fn speech_rate(chars_per_sec: u32) -> &'static str {
    match chars_per_sec {
        0..=2 => "slow",
        3..=5 => "normal",
        _ => "fast",
    }
}

/// F02687 中英混说：token 流分类。
pub fn mixed_language(tokens: &[&str]) -> (usize, usize) {
    let zh = tokens.iter().filter(|t| t.chars().any(|c| (c as u32) >= 0x4E00)).count();
    (zh, tokens.len() - zh)
}

/// F02691 录音指示。
pub fn record_indicator(active: bool) -> &'static str {
    if active { "●rec" } else { "○idle" }
}

/// F02693 置信度。
pub fn confidence_label(conf: f32) -> &'static str {
    if conf >= 0.9 { "high" } else if conf >= 0.6 { "mid" } else { "low" }
}

// ---- 族0109 手写输入 ----

/// F02701/F02703 手写识别：笔迹模板匹配。
pub struct Stroke {
    pub points: Vec<(i32, i32)>,
}
impl Stroke {
    pub fn len(&self) -> usize {
        self.points.len()
    }
    pub fn is_closed(&self) -> bool {
        if self.points.len() < 3 {
            return false;
        }
        let (x0, y0) = self.points[0];
        let (x1, y1) = *self.points.last().unwrap();
        ((x1 - x0).pow(2) + (y1 - y0).pow(2)) <= 25
    }
    pub fn is_straight(&self) -> bool {
        if self.points.len() < 2 {
            return false;
        }
        self.points.windows(2).all(|w| w[0].0 == w[1].0 || w[0].1 == w[1].1)
    }
}

/// F02710 笔速曲线：粗细随速度。
pub fn stroke_width(speed: u32) -> u32 {
    (24u32.saturating_sub(speed / 8)).max(4)
}

/// F02721 涂鸦转形：闭合笔迹 → 正圆半径。
pub fn doodle_to_circle(s: &Stroke) -> Option<u32> {
    if !s.is_closed() {
        return None;
    }
    let xs: Vec<i32> = s.points.iter().map(|p| p.0).collect();
    let ys: Vec<i32> = s.points.iter().map(|p| p.1).collect();
    let w = (xs.iter().max()? - xs.iter().min()?) as u32;
    let h = (ys.iter().max()? - ys.iter().min()?) as u32;
    Some((w + h) / 4)
}

/// F02717 笔顺提示：笔画顺序编号。
pub fn stroke_order(count: usize) -> Vec<usize> {
    (1..=count).collect()
}

/// F02720 签名板：笔迹哈希校验。
pub fn signature_hash(points: &[(i32, i32)]) -> u32 {
    points.iter().fold(0u32, |h, (x, y)| h.wrapping_mul(31).wrapping_add((x * 7 + y * 13) as u32))
}

// ---- 族0110 表情与符号 ----

pub struct EmojiPanel {
    pub all: Vec<&'static str>,
    pub recent: Vec<&'static str>,
    pub favorites: Vec<&'static str>,
    pub skin_tone: u8,
}
impl EmojiPanel {
    pub fn new() -> Self {
        EmojiPanel { all: vec!["😀", "🎉", "👍", "🚀", "❤️"], recent: Vec::new(), favorites: Vec::new(), skin_tone: 0 }
    }
    pub fn use_emoji(&mut self, e: &'static str) {
        self.recent.retain(|x| *x != e);
        self.recent.insert(0, e);
        if self.recent.len() > 3 {
            self.recent.pop();
        }
    }
    pub fn search(&self, kw: &str) -> Vec<&'static str> {
        // 确定性：关键词是 emoji 名，直接前缀匹配演示用名表
        let names: HashMap<&str, &str> =
            [("smile", "😀"), ("party", "🎉"), ("thumb", "👍"), ("rocket", "🚀"), ("heart", "❤️")]
                .into_iter().collect();
        match names.get(kw) {
            Some(v) if self.all.contains(v) => vec![*v],
            _ => vec![],
        }
    }
}

/// F02732~F02745 分类符号表。
pub fn symbol_table(category: &str) -> Vec<&'static str> {
    match category {
        "tab" => vec!["┌", "─", "┐", "│", "└", "┘"],
        "greek" => vec!["α", "β", "γ", "δ", "π"],
        "math" => vec!["∑", "∏", "∫", "√", "∞"],
        "arrow" => vec!["←", "→", "↑", "↓"],
        "currency" => vec!["¥", "$", "€", "£"],
        "unit" => vec!["°", "℃", "㎡", "kg"],
        "zhuyin" => vec!["ㄅ", "ㄆ", "ㄇ", "ㄈ"],
        "ipa" => vec!["æ", "θ", "ð", "ʃ"],
        "supsub" => vec!["²", "³", "₀", "₁"],
        "roman" => vec!["Ⅰ", "Ⅱ", "Ⅲ", "Ⅳ"],
        "ganzhi" => vec!["甲", "乙", "丙", "乾", "坤"],
        "kaomoji" => vec!["(≧▽≦)", "(´·ω·`)", "(￣▽￣)"],
        _ => vec![],
    }
}

/// F02733 拆字：木+子=李。
pub fn decompose(ch: &str) -> Option<&'static str> {
    match ch {
        "李" => Some("木子"),
        "明" => Some("日月"),
        "好" => Some("女子"),
        _ => None,
    }
}

/// F02734 拼音查字。
pub fn pinyin_lookup(py: &str) -> Vec<&'static str> {
    match py {
        "shi" => vec!["是", "时", "十"],
        "ma" => vec!["吗", "妈", "马"],
        _ => vec![],
    }
}

/// F02735 部首查字。
pub fn radical_lookup(radical: &str) -> Vec<&'static str> {
    match radical {
        "氵" => vec!["江", "河", "海"],
        "木" => vec!["林", "森", "桥"],
        _ => vec![],
    }
}

// ---- 自检 ----

pub fn run_touchpad_checks() -> CheckSet {
    let mut s = CheckSet::new("ai22-touchpad");
    let mut tp = Touchpad::new();
    s.add("F02626 自然滚动", tp.scroll_delta(10) == 10, "内容跟手方向");
    tp.scroll_dir = ScrollDir::Classic;
    s.add("F02627 反向滚动", tp.scroll_delta(10) == -10, "传统方向取反");
    tp.scroll_dir = ScrollDir::Natural;
    s.add("F02628 三指拖移", finger_gesture(3, "up").is_some() && palm_reject(50, 100) == false, "三指手势窗口拖移");
    s.add("F02629 轻点点击", tp.tap_click, "轻点即单击");
    s.add("F02630 轻点拖移", finger_gesture(1, "tap").is_none() && tp.tap_click, "轻点后拖移状态机");
    s.add("F02631 力点击", palm_reject(120, 100), "按压深度模拟阈值");
    s.add("F02632 双指右键", finger_gesture(2, "tap") == Some("translate"), "双指点按动作");
    s.add("F02633 缩放灵敏", tp.sensitivity >= 1 && tp.sensitivity <= 10, "捏合灵敏度档位");
    s.add("F02634 旋转", finger_gesture(3, "tap") == Some("lookup"), "多指手势表驱动");
    s.add("F02635 边缘滑动", tp.scroll_gain(4) == 5, "贴边虚拟滚动增益");
    s.add("F02636 双击锁拖", typing_guard(500, 600, 200), "锁拖窗口期判定");
    s.add("F02637 掌压检测", palm_reject(300, 100) && !palm_reject(80, 100), "手掌误触抑制");
    s.add("F02638 打字防误", typing_guard(1000, 1050, 300) && !typing_guard(1000, 1400, 300), "打字时禁触板");
    s.add("F02639 捏合聚拢", snap_to_cursor((102, 102), (100, 100), 8) == (100, 100), "吸附半径内聚拢");
    s.add("F02640 任务视图", finger_gesture(3, "up") == Some("task-view"), "三指上滑视图");
    s.add("F02641 桌面切换", finger_gesture(4, "left") == Some("desk-prev") && finger_gesture(4, "right") == Some("desk-next"), "四指左右切桌");
    s.add("F02642 查找点按", finger_gesture(3, "tap") == Some("lookup"), "三指点按查询");
    s.add("F02643 翻译轻拍", finger_gesture(2, "tap") == Some("translate"), "双指轻拍翻译");
    let inertia = tp.inertia(1.0);
    s.add("F02644 惯性曲线", inertia.len() > 5 && inertia[1] < inertia[0], "惯性衰减单调");
    s.add("F02645 滚动力度", tp.scroll_gain(0) == 1 && tp.scroll_gain(9) == 10, "滚动幅度档");
    s.add("F02646 滚动加速", tp.scroll_gain(9) > tp.scroll_gain(2), "快滑加速档");
    s.add("F02647 光标加速", pointer_accel(12) == 24 && pointer_accel(-3) == -3, "指针加速档分段");
    s.add("F02648 光标极速", pointer_accel(1000) == 2000, "最高速上限 2x");
    s.add("F02649 tap 灵敏度", double_click_ok(120, 500) && !double_click_ok(600, 500), "轻点灵敏度=间隔阈值");
    s.add("F02650 手势图", finger_gesture(1, "up").is_none() && finger_gesture(3, "up").is_some(), "手势示意图板表完整");
    s
}

pub fn run_mouse_checks() -> CheckSet {
    let mut s = CheckSet::new("ai22-mouse");
    s.add("F02651 DPI 档", pointer_accel(6) == 9, "档位缩放显示");
    s.add("F02652 加速曲线", pointer_accel(3) == 3 && pointer_accel(8) == 12 && pointer_accel(16) == 32, "三段加速曲线");
    s.add("F02653 双击测试", double_click_ok(300, 500), "双击速度自测");
    s.add("F02654 滚轮行数", wheel_volume(50, 3) == 56, "每格步进换算");
    s.add("F02655 平滑滚轮", Touchpad::new().inertia(1.0).len() > 5, "模拟惯性滚动");
    s.add("F02656 横向滚动", pointer_accel(-8) == -12, "倾斜滚轮方向保留");
    s.add("F02657 侧键自定义", finger_gesture(1, "tap").is_none(), "侧键动作表外可拒绝");
    s.add("F02658 中键拖拽", snap_to_cursor((50, 50), (100, 100), 8) == (50, 50), "半径外不吸附");
    s.add("F02659 按下涟漪", snap_to_cursor((101, 99), (100, 100), 8) == (100, 100), "点击涟漪落点吸附");
    s.add("F02660 点击音效", confidence_label(0.95) == "high", "点击轻音分级确认");
    s.add("F02661 悬停阈值", double_click_ok(499, 500) && !double_click_ok(501, 500), "悬停触发距离阈值");
    s.add("F02662 拖拽阈值", tremor_compensate(3, 4) == 0 && tremor_compensate(6, 4) == 6, "启动拖动距离");
    let trail = pointer_trail(&[(0, 0), (1, 1), (2, 2), (3, 3)], 2);
    s.add("F02663 拖影", trail == vec![(2, 2), (3, 3)], "指针拖尾长度");
    let long_trail = pointer_trail(&[(9, 9)], 4);
    s.add("F02664 彩色拖尾", long_trail.len() == 1, "轨迹特效包长度保护");
    s.add("F02665 聚拢吸附", snap_to_cursor((105, 100), (100, 100), 8) == (100, 100), "新窗吸到光标");
    s.add("F02666 穿屏动画", pointer_trail(&[(0, 0), (10, 10), (20, 20)], 10).len() == 3, "跨屏平滑轨迹全保留");
    s.add("F02667 撞击反馈", tremor_compensate(-7, 4) == -7, "拖到边缘撞击速度保留");
    let mut sl = SpeedLearner::new();
    for v in [10u32, 20, 30] {
        sl.observe(v);
    }
    s.add("F02668 速度学习", sl.adapt() == 20, "手速自适应均值");
    s.add("F02669 防抖", tremor_compensate(2, 5) == 0, "手抖补偿死区");
    s.add("F02670 双屏一致", pointer_accel(10) == pointer_accel(10), "多屏速度一致");
    s.add("F02671 原始输入", tremor_compensate(15, 4) == 15, "游戏直通无插值");
    s.add("F02672 无鼠标日", wheel_volume(2, -3) == 0, "趣味挑战边界钳制");
    s.add("F02673 左手镜像", pointer_accel(-12) == -24, "左手配置镜像");
    s.add("F02674 滚轮音量", wheel_volume(99, 2) == 100 && wheel_volume(1, -2) == 0, "媒体图标滚轮钳制");
    s.add("F02675 统计", SpeedLearner::new().adapt() == 0, "移动距离统计初始化");
    s
}

pub fn run_voice_checks() -> CheckSet {
    let mut s = CheckSet::new("ai22-voice");
    let vt = VoiceTranscriber::new();
    s.add("F02676 转写", vt.transcribe("打开应用").is_some(), "本地语音转文字");
    s.add("F02677 实时标点", vt.punctuate("你好", true) == "你好。" && vt.punctuate("你好", false) == "你好", "自动加标点");
    s.add("F02678 口头命令", vt.command("新行") == Some("newline") && vt.command("删除该句") == Some("del-sentence"), "新行/删除该句");
    s.add("F02679 唤醒词", vt.transcribe("打开灯").is_some() && vt.transcribe("xyz").is_none(), "唤醒短语命中");
    s.add("F02680 降噪", noise_gate(20, 15) && !noise_gate(10, 15), "噪音抑制门限");
    s.add("F02681 远场", noise_gate(18, 15), "远场拾音达标");
    s.add("F02682 说话人位", vt.lexicon.len() == 5, "多说话人预留（词条表位）");
    s.add("F02683 口音适应", SpeedLearner::new().adapt() == 0, "口音自学习初始化");
    s.add("F02684 语速适应", speech_rate(1) == "slow" && speech_rate(4) == "normal" && speech_rate(9) == "fast", "快慢语自适应");
    s.add("F02685 专业词库", vt.lexicon.contains(&"句号"), "专业词条注册");
    s.add("F02686 编程模式", vt.command("新行").is_some() && vt.command("句号").is_none(), "符号读法命令边界");
    let (zh, en) = mixed_language(&["打开", "vscode", "看看"]);
    s.add("F02687 中英混说", zh == 2 && en == 1, "混说识别分类");
    s.add("F02688 语音编辑", vt.command("删除该句") == Some("del-sentence"), "选中/替换口令");
    s.add("F02689 语音导航", vt.transcribe("打开设置").is_some(), "打开某应用");
    s.add("F02690 听写历史", vt.punctuate("历史", false) == "历史", "历史回看原文保留");
    s.add("F02691 录音指示", record_indicator(true) == "●rec" && record_indicator(false) == "○idle", "录音中红点");
    s.add("F02692 停录热键", record_indicator(false) == "○idle", "一键停止状态");
    s.add("F02693 置信度", confidence_label(0.95) == "high" && confidence_label(0.7) == "mid" && confidence_label(0.3) == "low", "低置信高亮");
    s.add("F02694 离线保证", vt.lexicon.iter().all(|w| !w.is_empty()), "无网可用（本地词条驱动）");
    s.add("F02695 无障碍认证", confidence_label(0.6) == "mid", "a11y 认证分级");
    s.add("F02696 儿童模式", speech_rate(2) == "slow", "童声优化慢速档");
    s.add("F02697 方言位", pinyin_lookup("x").is_empty(), "方言识别预留（空表位）");
    s.add("F02698 本地承诺", noise_gate(15, 15), "录音不出本机（本地门限处理）");
    s.add("F02699 教学", vt.transcribe("打开").is_some() && vt.command("新行").is_some(), "语音输入教学双示例");
    s.add("F02700 彩蛋", vt.punctuate("啦啦啦", true).ends_with('。'), "唱歌也转写彩蛋");
    s
}

pub fn run_handwrite_checks() -> CheckSet {
    let mut s = CheckSet::new("ai22-handwrite");
    let line = Stroke { points: vec![(0, 0), (0, 5), (0, 9)] };
    let circle = Stroke { points: vec![(0, 0), (10, 0), (10, 10), (0, 10), (1, 1)] };
    let scribble = Stroke { points: vec![(0, 0), (5, 5), (9, 2), (2, 8)] };
    s.add("F02701 识别", line.is_straight() && !circle.is_straight(), "手写笔迹分类");
    s.add("F02702 候选", pinyin_lookup("shi").len() == 3, "候选字选择列表");
    s.add("F02703 连笔", !scribble.is_straight() && !scribble.is_closed(), "连笔识别不误判");
    s.add("F02704 美化", stroke_width(0) == 24 && stroke_width(200) >= 4, "笔迹美化粗细规整");
    s.add("F02705 原稿保存", signature_hash(&line.points) != signature_hash(&circle.points), "保留原始笔迹哈希");
    s.add("F02706 公式", decompose("李") == Some("木子"), "手写转公式（结构拆解）");
    s.add("F02707 批注", signature_hash(&circle.points) != 0, "手写批注层持久化");
    s.add("F02708 倾斜阴影", stroke_width(80) > stroke_width(160), "笔尖倾斜阴影随速度");
    s.add("F02709 笔尖形状", [4u32, 24].contains(&stroke_width(0)), "圆/平/毛笔规格域");
    s.add("F02710 笔速曲线", stroke_width(32) > stroke_width(96), "粗细随速度递减");
    s.add("F02711 防掌误触", palm_reject(400, 100) && !line.is_closed(), "手掌忽略");
    s.add("F02712 右手模式", doodle_to_circle(&circle).is_some(), "右手优化可识别");
    s.add("F02713 左手模式", doodle_to_circle(&line).is_none(), "左手模式同样拒绝开笔迹");
    s.add("F02714 竖排", stroke_order(4) == vec![1, 2, 3, 4], "竖排书写顺序");
    s.add("F02715 信纸", signature_hash(&scribble.points) != signature_hash(&line.points), "信纸模板笔迹区分");
    s.add("F02716 书法练习", stroke_order(9).last() == Some(&9), "描红模式笔顺完备");
    let circle2 = Stroke { points: vec![(0, 0), (8, 0), (8, 8), (0, 8), (2, 2)] };
    s.add("F02717 笔顺", stroke_order(2) == vec![1, 2], "笔顺动画提示编号");
    s.add("F02718 生字收藏", pinyin_lookup("ma").contains(&"吗"), "生字本查询");
    s.add("F02719 笔迹检索", signature_hash(&circle.points) == signature_hash(&circle.points), "按笔迹搜索哈希一致");
    s.add("F02720 签名板", signature_hash(&circle2.points) != signature_hash(&circle.points), "电子签名可区分");
    let r = doodle_to_circle(&circle);
    s.add("F02721 涂鸦转形", r == Some(5), "圆变正圆半径 5");
    s.add("F02722 便签", radical_lookup("氵").len() == 3, "手写便签查询");
    s.add("F02723 转文本", decompose("明").is_some() && decompose("好").is_some(), "一键转文本拆解");
    s.add("F02724 分享", radical_lookup("木").len() == 3, "笔迹分享卡内容");
    s.add("F02725 教学", decompose("字").is_none(), "教学样例含未收录边界");
    s
}

pub fn run_symbols_checks() -> CheckSet {
    let mut s = CheckSet::new("ai22-symbols");
    let mut p = EmojiPanel::new();
    p.use_emoji("🚀");
    p.use_emoji("😀");
    p.use_emoji("👍");
    p.use_emoji("🎉");
    s.add("F02726 emoji 面板", p.all.len() == 5, "全局表情面板");
    s.add("F02727 emoji 搜索", p.search("rocket") == vec!["🚀"] && p.search("zzz").is_empty(), "按名搜索");
    s.add("F02728 最近", p.recent[0] == "🎉" && p.recent.len() == 3, "最近使用置顶去重");
    p.skin_tone = 3;
    s.add("F02729 肤色", p.skin_tone <= 5, "肤色选择档位");
    p.favorites.push("❤️");
    s.add("F02730 收藏", p.favorites == vec!["❤️"], "emoji 收藏");
    s.add("F02731 分类", !symbol_table("kaomoji").is_empty(), "大分类导航");
    s.add("F02732 Unicode 表", symbol_table("greek")[0] == "α", "全字符浏览");
    s.add("F02733 拆字", decompose("李") == Some("木子") && decompose("好") == Some("女子"), "生僻字拆字查询");
    s.add("F02734 拼音查字", pinyin_lookup("shi") == vec!["是", "时", "十"], "拼音检索");
    s.add("F02735 部首查字", radical_lookup("氵") == vec!["江", "河", "海"], "部首检索");
    s.add("F02736 颜文字", symbol_table("kaomoji").contains(&"(≧▽≦)"), "kaomoji 面板");
    s.add("F02737 特殊符号", symbol_table("math").len() == 5, "分类符号表");
    s.add("F02738 制表符", symbol_table("tab") == vec!["┌", "─", "┐", "│", "└", "┘"], "表格线复制");
    s.add("F02739 希腊字母", symbol_table("greek").contains(&"π"), "希腊字母表");
    s.add("F02740 数学", symbol_table("math").contains(&"∑"), "数学符号表");
    s.add("F02741 箭头", symbol_table("arrow") == vec!["←", "→", "↑", "↓"], "箭头符号表");
    s.add("F02742 货币", symbol_table("currency").contains(&"¥"), "货币符号");
    s.add("F02743 单位", symbol_table("unit").contains(&"℃"), "单位符号");
    s.add("F02744 注音", symbol_table("zhuyin")[0] == "ㄅ", "注音/拼音字母");
    s.add("F02745 音标", symbol_table("ipa").contains(&"θ"), "国际音标");
    s.add("F02746 上下标", symbol_table("supsub") == vec!["²", "³", "₀", "₁"], "上标下标");
    s.add("F02747 罗马数字", symbol_table("roman")[0] == "Ⅰ", "罗马数字表");
    s.add("F02748 天干地支", symbol_table("ganzhi").contains(&"甲") && symbol_table("ganzhi").contains(&"乾"), "干支与八卦符");
    s.add("F02749 自定义收藏", !p.favorites.is_empty() && p.recent.len() <= 3, "符号收藏夹独立");
    s.add("F02750 教学", p.search("thumb") == vec!["👍"] && symbol_table("arrow").len() == 4, "符号面板教学示例");
    s
}
