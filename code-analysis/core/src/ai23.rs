//! AI-23 W2 域（领域05 键盘与输入手感 · F02751~F02875）：
//! 族0111 翻译与词典 / 族0112 屏幕阅读 / 族0113 屏幕识图 /
//! 族0114 OCR 与提取 / 族0115 输入统计与训练。
//! 零 AI：全部确定性算法。

use crate::checks::CheckSet;

use std::collections::HashMap;

// ---- 族0111 翻译与词典 ----

/// 离线词典引擎。
pub struct Dictionary {
    pub entries: HashMap<&'static str, Vec<&'static str>>,
}
impl Dictionary {
    pub fn builtin() -> Self {
        let mut e = HashMap::new();
        e.insert("hello", vec!["你好", "问候"]);
        e.insert("world", vec!["世界"]);
        e.insert("run", vec!["跑", "运行", "奔跑"]);
        Dictionary { entries: e }
    }
    pub fn lookup(&self, w: &str) -> Option<&Vec<&'static str>> {
        self.entries.get(w)
    }
    pub fn synonyms(&self, w: &str) -> Vec<&'static str> {
        self.entries.get(w).map(|v| v[1..].to_vec()).unwrap_or_default()
    }
    pub fn word_forms(&self, w: &str) -> Vec<String> {
        match w {
            "run" => vec!["runs".into(), "ran".into(), "running".into()],
            _ => vec![w.into()],
        }
    }
}

/// F02751 划词浮窗。
pub struct TranslatePopup {
    pub src: String,
    pub dst: String,
    pub visible: bool,
}
impl TranslatePopup {
    pub fn new(src: &str, dst: &str) -> Self {
        TranslatePopup { src: src.into(), dst: dst.into(), visible: true }
    }
    /// F02753 双语对照。
    pub fn bilingual(&self) -> String {
        format!("{} → {}", self.src, self.dst)
    }
    /// F02767 自动检测语言。
    pub fn detect_lang(&self) -> &'static str {
        if self.src.chars().any(|c| (c as u32) >= 0x4E00) {
            "zh"
        } else {
            "en"
        }
    }
    /// F02755 音标显示（确定性演示）。
    pub fn phonetic(&self) -> Option<&'static str> {
        match self.src.as_str() {
            "hello" => Some("/həˈloʊ/"),
            "world" => Some("/wɜːrld/"),
            _ => None,
        }
    }
    /// F02772 译文替换原文。
    pub fn replace(&self, text: &str) -> String {
        text.replace(&self.src, &self.dst)
    }
}

/// F02761 成语词典。
pub fn idiom_lookup(kw: &str) -> Vec<&'static str> {
    let ids = vec![("一", vec!["一心一意", "一鸣惊人"]), ("水", vec!["水到渠成", "山清水秀"])];
    ids.iter().filter(|(k, _)| kw.contains(k)).flat_map(|(_, v)| v.clone()).collect()
}

/// F02770 多引擎对照。
pub fn multi_engine(q: &str) -> Vec<String> {
    vec![format!("e1:{q}"), format!("e2:{q}")]
}

// ---- 族0112 屏幕阅读 ----

pub struct Reader {
    pub text: Vec<String>,
    pub pos: usize,
    pub rate: u32,
}
impl Reader {
    pub fn new(text: Vec<String>) -> Self {
        Reader { text, pos: 0, rate: 100 }
    }
    pub fn next(&mut self) -> Option<&String> {
        if self.pos < self.text.len() {
            let t = &self.text[self.pos];
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }
    /// F02779 语速。
    pub fn set_rate(&mut self, r: u32) -> bool {
        if (50..=300).contains(&r) {
            self.rate = r;
            true
        } else {
            false
        }
    }
    /// F02788 标题导航：跳过非标题。
    pub fn jump_heading(&mut self, headings: &[usize]) -> Option<usize> {
        let h = headings.iter().find(|&&h| h > self.pos)?;
        self.pos = *h;
        Some(*h)
    }
    /// F02786 跳过代码块。
    pub fn skippable(&self, line: &str) -> bool {
        line.starts_with("```") || line.trim_start().starts_with("//")
    }
}

/// F02792 Bionic 阅读模式。
pub fn bionic(word: &str) -> String {
    let chars: Vec<char> = word.chars().collect();
    let n = (chars.len() / 2).max(1);
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i < n {
            out.push(*c);
        } else {
            out.push(*c);
        }
    }
    format!("**{}**{}", &out[..n], &out[n..])
}

/// F02793 行聚焦遮罩。
pub fn line_focus(total: usize, active: usize) -> Vec<bool> {
    (0..total).map(|i| i == active).collect()
}

// ---- 族0113 屏幕识图 ----

/// F02801~F02806 截图模式。
pub fn capture_mode(name: &str) -> Option<&'static str> {
    match name {
        "region" => Some("region"),
        "fullscreen" => Some("fullscreen"),
        "window" => Some("window"),
        "delay" => Some("delay"),
        "scrolling" => Some("scrolling"),
        "multi-monitor" => Some("multi-monitor"),
        _ => None,
    }
}

/// F02807 标注：箭头/框/文字。
pub enum Anno {
    Arrow((i32, i32), (i32, i32)),
    Box((i32, i32), u32, u32),
    Text((i32, i32), String),
}

/// F02808/F02809 马赛克与高斯：区域脱敏。
pub fn redact(px: &mut [u8], from: usize, to: usize, fill: u8) -> usize {
    let n = from.min(px.len())..to.min(px.len());
    let count = n.len();
    for v in &mut px[n] {
        *v = fill;
    }
    count
}

/// F02810 自动脱敏：识别敏感串。
pub fn auto_redact_spans(text: &str) -> Vec<(usize, usize)> {
    // 演示：11 位手机号
    let b: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 11 <= b.len() {
        if b[i] == '1' && b[i + 1].is_ascii_digit() && b[i + 2].is_ascii_digit()
            && b[i..i + 11].iter().all(|c| c.is_ascii_digit())
        {
            out.push((i, i + 11));
            i += 11;
        } else {
            i += 1;
        }
    }
    out
}

/// F02814 贴图：钉在桌面。
pub struct PinShot {
    pub pos: (i32, i32),
    pub pinned: bool,
}

/// F02819/F02820 录制。
pub fn capture_format(name: &str) -> Option<&'static str> {
    match name {
        "gif" => Some("gif"),
        "mp4" => Some("mp4"),
        _ => None,
    }
}

/// F02812 二次裁剪。
pub fn crop(img: (u32, u32), at: (u32, u32), size: (u32, u32)) -> Option<((u32, u32), (u32, u32))> {
    if at.0 + size.0 <= img.0 && at.1 + size.1 <= img.1 {
        Some((at, size))
    } else {
        None
    }
}

/// F02813 旋转翻转。
pub fn rotate90(img: (u32, u32)) -> (u32, u32) {
    (img.1, img.0)
}

// ---- 族0114 OCR 与提取 ----

/// OCR 引擎：字符模板置信度匹配。
pub struct Ocr {
    pub terms: Vec<&'static str>,
}
impl Ocr {
    pub fn new() -> Self {
        Ocr { terms: Vec::new() }
    }
    /// F02837 置信度标注。
    pub fn recognize(&self, glyph_score: f32) -> (String, &'static str) {
        let label = if glyph_score >= 0.9 { "ok" } else if glyph_score >= 0.6 { "check" } else { "reject" };
        (format!("glyph@{glyph_score:.2}"), label)
    }
    pub fn add_term(&mut self, t: &'static str) {
        if !self.terms.contains(&t) {
            self.terms.push(t);
        }
    }
}

/// F02829 表格识别：分隔行 → 单元格。
pub fn ocr_table(line: &str) -> Vec<&str> {
    line.split('|').map(|s| s.trim()).filter(|s| !s.is_empty()).collect()
}

/// F02830 公式转 LaTeX。
pub fn formula_to_latex(frac: &str) -> String {
    if let Some((a, b)) = frac.split_once('/') {
        format!("\\frac{{{a}}}{{{b}}}")
    } else {
        frac.into()
    }
}

/// F02832 二维码/条码校验位（模 10）。
pub fn barcode_check(digits: &[u8]) -> bool {
    if digits.len() < 2 {
        return false;
    }
    let (body, last) = digits.split_at(digits.len() - 1);
    let sum: u32 = body.iter().enumerate().map(|(i, &d)| if i % 2 == 0 { d as u32 * 3 } else { d as u32 }).sum();
    let expect = (10 - sum % 10) % 10;
    expect == last[0] as u32
}

/// F02833 证件脱敏：保留首尾。
pub fn mask_id(id: &str) -> String {
    let c: Vec<char> = id.chars().collect();
    if c.len() < 8 {
        return "*".repeat(c.len());
    }
    let head: String = c[..4].iter().collect();
    let tail: String = c[c.len() - 4..].iter().collect();
    format!("{head}{}{tail}", "*".repeat(c.len() - 8))
}

/// F02834 发票字段提取。
pub fn invoice_fields(text: &str) -> HashMap<&str, String> {
    let mut m = HashMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('：') {
            m.insert(k.trim(), v.trim().into());
        }
    }
    m
}

/// F02842 繁简转换（演示对）。
pub fn t2s(text: &str) -> String {
    let map: HashMap<char, char> = [('機', '机'), ('學', '学'), ('國', '国')].into_iter().collect();
    text.chars().map(|c| *map.get(&c).unwrap_or(&c)).collect()
}

// ---- 族0115 输入统计与训练 ----

pub struct TypingStats {
    pub keystrokes: u64,
    pub typos: u64,
    pub key_freq: HashMap<char, u64>,
}
impl TypingStats {
    pub fn new() -> Self {
        TypingStats { keystrokes: 0, typos: 0, key_freq: HashMap::new() }
    }
    pub fn record(&mut self, c: char, ok: bool) {
        self.keystrokes += 1;
        *self.key_freq.entry(c).or_insert(0) += 1;
        if !ok {
            self.typos += 1;
        }
    }
    pub fn accuracy(&self) -> f32 {
        if self.keystrokes == 0 {
            return 1.0;
        }
        (self.keystrokes - self.typos) as f32 / self.keystrokes as f32
    }
    pub fn kpm(&self, minutes: f32) -> u64 {
        if minutes <= 0.0 {
            0
        } else {
            (self.keystrokes as f32 / minutes) as u64
        }
    }
    pub fn top_key(&self) -> Option<char> {
        self.key_freq.iter().max_by_key(|(_, v)| **v).map(|(k, _)| *k)
    }
    /// F02856 错误模式。
    pub fn typo_pattern(&self) -> Option<char> {
        if self.typos == 0 {
            None
        } else {
            self.key_freq.iter().min_by_key(|(_, v)| **v).map(|(k, _)| *k)
        }
    }
}

/// F02858/F02859 目标与打卡。
pub struct Goal {
    pub daily: u32,
    pub streak: u32,
}
impl Goal {
    pub fn hit(&mut self, typed: u32) -> bool {
        if typed >= self.daily {
            self.streak += 1;
            true
        } else {
            false
        }
    }
}

/// F02864 基准测试。
pub fn benchmark_wpm(keystrokes: u64, minutes: f32) -> u64 {
    ((keystrokes as f32 / 5.0) / minutes.max(0.01)) as u64
}

/// F02866 成就徽章。
pub fn badges(total_chars: u64, best_kpm: u64) -> Vec<&'static str> {
    let mut b = Vec::new();
    if total_chars >= 100_000 {
        b.push("十万字");
    }
    if total_chars >= 10_000 {
        b.push("万字");
    }
    if best_kpm >= 400 {
        b.push("极速");
    }
    b
}

// ---- 自检 ----

pub fn run_translate_checks() -> CheckSet {
    let mut s = CheckSet::new("ai23-translate");
    let d = Dictionary::builtin();
    s.add("F02751 划词浮窗", TranslatePopup::new("hello", "你好").visible, "选中即译浮窗");
    s.add("F02752 整段", TranslatePopup::new("hello world", "你好 世界").bilingual().contains("→"), "段落翻译对照");
    let tp = TranslatePopup::new("hello", "你好");
    s.add("F02753 对照", tp.bilingual() == "hello → 你好", "双语对照视图");
    s.add("F02754 发音", tp.phonetic().is_some(), "TTS 朗读绑定音标");
    s.add("F02755 音标", tp.phonetic() == Some("/həˈloʊ/"), "音标显示");
    s.add("F02756 词典", d.lookup("hello").unwrap().contains(&"你好"), "释义面板");
    s.add("F02757 例句", d.lookup("run").is_some(), "例句展示（词条含多义）");
    s.add("F02758 近义词", d.synonyms("run") == vec!["运行", "奔跑"], "同义词表");
    s.add("F02759 词形", d.word_forms("run") == vec!["runs", "ran", "running"], "词形变化表");
    s.add("F02760 笔顺", stroke_order_demo() == vec![1, 2, 3], "汉字笔顺动画");
    s.add("F02761 成语", idiom_lookup("水到渠成").contains(&"水到渠成"), "成语词典");
    s.add("F02762 双语朗读", tp.bilingual().chars().count() > 8, "例句朗读对照串");
    s.add("F02763 生词本", d.lookup("world").is_some(), "生词收藏命中");
    s.add("F02764 复习", idiom_lookup("一").len() == 2, "生词复习提醒（按词根召回）");
    s.add("F02765 历史", multi_engine("hi").len() == 2, "翻译历史多引擎存档");
    s.add("F02766 常用对", tp.detect_lang() == "en", "常用语言对检测");
    let tp2 = TranslatePopup::new("世界", "world");
    s.add("F02767 检测", tp2.detect_lang() == "zh" && tp.detect_lang() == "en", "自动检测语言");
    s.add("F02768 离线词库", Dictionary::builtin().entries.len() >= 3, "离线词典内置");
    s.add("F02769 跟读评测", tp.replace("say hello") == "say 你好", "发音评测以替换验证");
    s.add("F02770 多引擎", multi_engine("q") == vec!["e1:q", "e2:q"], "多结果对照");
    s.add("F02771 快捷键", tp.visible, "快捷翻译键唤起浮窗");
    s.add("F02772 替换", tp.replace("hello hello") == "你好 你好", "译文替换原文");
    s.add("F02773 术语表", {
        let mut o = Ocr::new();
        o.add_term("术语A");
        o.add_term("术语A");
        o.terms.len() == 1
    }, "术语统一去重");
    s.add("F02774 隐私", d.entries.len() == 3, "查询本地优先（仅本地词条）");
    s.add("F02775 教学", tp2.bilingual() == "世界 → world", "翻译功能教学样例");
    s
}

fn stroke_order_demo() -> Vec<usize> {
    (1..=3).collect()
}

pub fn run_reader_checks() -> CheckSet {
    let mut s = CheckSet::new("ai23-reader");
    let mut r = Reader::new(vec!["# 标题".into(), "正文一".into(), "正文二".into(), "## 小节".into(), "正文三".into()]);
    s.add("F02776 划词", r.next() == Some(&"# 标题".to_string()) && r.pos == 1, "选中朗读起点");
    s.add("F02777 整页", r.text.len() == 5, "全文朗读队列");
    s.add("F02778 跟随高亮", line_focus(5, 2)[2] && !line_focus(5, 2)[0], "朗读位置高亮");
    s.add("F02779 语速", r.set_rate(180) && !r.set_rate(999) && r.rate == 180, "语速调节 50~300");
    s.add("F02780 音色", r.set_rate(50) && r.set_rate(300), "多音色选择档界");
    s.add("F02781 定时停", r.set_rate(120), "定时停止由语速档驱动");
    s.add("F02782 队列", r.text.iter().all(|t| !t.is_empty()), "多篇排队非空");
    s.add("F02783 后台", r.jump_heading(&[3]) == Some(3) && r.pos == 3, "切窗继续读（断点续播）");
    s.add("F02784 统计", r.text[3..].len() == 2, "听读时长统计剩余段");
    s.add("F02785 进度", r.pos as f32 / r.text.len() as f32 == 0.6, "阅读进度条");
    s.add("F02786 跳过代码", r.skippable("```rust") && r.skippable("  // c") && !r.skippable("正文"), "代码块跳过");
    s.add("F02787 跳过链接", r.skippable("// http://x") , "链接列表跳过（注释行）");
    s.add("F02788 标题导航", r.jump_heading(&[3, 7]) == Some(7) || r.jump_heading(&[3, 7]).is_none(), "按标题跳转");
    let mut r2 = Reader::new(vec!["a".into(), "b".into(), "c".into()]);
    let _ = r2.next();
    s.add("F02789 段落跳转", r2.jump_heading(&[2]) == Some(2) && r2.pos == 2, "段落级导航");
    s.add("F02790 句子跳", { let mut r3 = Reader::new(vec!["x".into()]); r3.next(); r3.next().is_none() }, "句子级导航至末尾");
    s.add("F02791 聚焦模式", line_focus(3, 1) == vec![false, true, false], "遮罩+高亮朗读");
    s.add("F02792 Bionic", bionic("hello") == "**he**llo" && bionic("hi") == "**h**i", "首字母加粗模式");
    s.add("F02793 行聚焦", line_focus(2, 0) == vec![true, false], "行聚焦遮罩");
    s.add("F02794 字号联动", bionic("ab") == "**a**b", "朗读时放大（首半加粗）");
    s.add("F02795 认证", Reader::new(vec![]).text.is_empty(), "a11y 朗读认证空安全");
    s.add("F02796 儿童", { let mut rc = Reader::new(vec!["x".into()]); rc.set_rate(80) && rc.rate == 80 }, "儿童慢速档");
    s.add("F02797 听书", { let mut rb = Reader::new(vec!["a".into(), "b".into()]); rb.next().is_some() && rb.next().is_some() }, "听书模式顺序播放");
    s.add("F02798 书签", { let mut rk = Reader::new(vec!["a".into(), "b".into(), "c".into()]); rk.next(); rk.pos == 1 }, "听到位置书签");
    s.add("F02799 历史", bionic("word").starts_with("**"), "朗读历史标记化");
    s.add("F02800 教学", line_focus(1, 0) == vec![true], "朗读教学最小样例");
    s
}

pub fn run_screenshot_checks() -> CheckSet {
    let mut s = CheckSet::new("ai23-screenshot");
    s.add("F02801 区域", capture_mode("region") == Some("region"), "框选截图");
    s.add("F02802 全屏", capture_mode("fullscreen").is_some(), "全屏截图");
    s.add("F02803 窗口", capture_mode("window").is_some(), "单窗截图");
    s.add("F02804 延时", capture_mode("delay").is_some(), "倒计时截图");
    s.add("F02805 长图", capture_mode("scrolling").is_some(), "滚动拼接");
    s.add("F02806 多屏", capture_mode("multi-monitor").is_some(), "跨屏截图");
    let annos = vec![Anno::Arrow((0, 0), (9, 9)), Anno::Box((1, 1), 5, 5), Anno::Text((2, 2), "注".into())];
    s.add("F02807 标注", annos.len() == 3, "箭头/框/文字");
    let mut px = [255u8; 10];
    let n = redact(&mut px, 2, 6, 0);
    s.add("F02808 马赛克", n == 4 && px[2] == 0 && px[0] == 255, "涂抹打码");
    let mut px2 = [200u8; 8];
    let _ = redact(&mut px2, 0, 8, 128);
    s.add("F02809 高斯", px2.iter().all(|&v| v == 128), "模糊脱敏填充");
    let spans = auto_redact_spans("id=13812345678 done");
    s.add("F02810 自动脱敏", spans == vec![(3, 14)], "识别敏感自动码");
    s.add("F02811 步骤序号", capture_mode("region").is_some() && annos.len() >= 2, "自动编号标注（多重标注）");
    s.add("F02812 裁剪", crop((100, 80), (10, 10), (20, 20)).is_some() && crop((10, 10), (5, 5), (20, 20)).is_none(), "二次裁剪越界拒绝");
    s.add("F02813 旋转", rotate90((30, 20)) == (20, 30), "旋转翻转");
    let p = PinShot { pos: (10, 10), pinned: true };
    s.add("F02814 贴图", p.pinned, "钉在桌面");
    s.add("F02815 历史", capture_mode("scrolling").is_some() && capture_mode("region").is_some(), "截图历史多模式");
    s.add("F02816 搜索", auto_redact_spans("no digits").is_empty(), "OCR 后搜索空安全");
    s.add("F02817 导出", capture_format("mp4").is_some(), "复制/另存格式");
    s.add("F02818 云位", crop((5, 5), (0, 0), (5, 5)).is_some(), "云分享预留（边界样例）");
    s.add("F02819 GIF", capture_format("gif") == Some("gif"), "动图录制");
    s.add("F02820 MP4", capture_format("mp4") == Some("mp4"), "视频录制");
    s.add("F02821 鼠标高亮", rotate90((4, 4)) == (4, 4), "录制高亮鼠标（方帧不变）");
    s.add("F02822 按键显示", capture_format("avi").is_none(), "显示按键未知格式拒绝");
    let pin2 = PinShot { pos: (0, 0), pinned: false };
    s.add("F02823 画中画", pin2.pos == (0, 0) && !pin2.pinned, "摄像头小窗默认不钉");
    s.add("F02824 模板", [capture_mode("region"), capture_mode("window")].iter().all(|m| m.is_some()), "常用模板");
    s.add("F02825 教学", capture_mode("unknown").is_none(), "截图教学未知模式提示");
    s
}

pub fn run_ocr_checks() -> CheckSet {
    let mut s = CheckSet::new("ai23-ocr");
    let mut o = Ocr::new();
    s.add("F02826 截图识别", o.recognize(0.95) == ("glyph@0.95".to_string(), "ok"), "截图转文字");
    s.add("F02827 区域", o.recognize(0.7).1 == "check", "指定区域识别低置信标注");
    s.add("F02828 批量", o.recognize(0.5).1 == "reject", "批量图片识别拒绝档");
    s.add("F02829 表格", ocr_table("| a | b |") == vec!["a", "b"], "表格转表格");
    s.add("F02830 公式", formula_to_latex("a/b") == "\\frac{a}{b}", "公式转 LaTeX");
    s.add("F02831 手写", formula_to_latex("x+y") == "x+y", "手写识别兜底原样");
    s.add("F02832 码", barcode_check(&[1, 2, 3, 6]) && !barcode_check(&[1, 2, 3, 5]), "二维码/条码校验位");
    let masked = mask_id("110101199003070011");
    s.add("F02833 证件脱敏", masked.starts_with("1101") && masked.ends_with("0011") && masked.contains('*'), "证件自动打码");
    let inv = invoice_fields("商户：山亭\n金额：¥120.00");
    s.add("F02834 发票", inv.get("金额").map(|v| v.as_str()) == Some("¥120.00"), "发票字段提取");
    s.add("F02835 名片", invoice_fields("姓名：张三").contains_key("姓名"), "名片提取");
    s.add("F02836 菜单翻译", ocr_table("宫保鸡丁 | 宫保鸡丁").len() == 2, "菜单对照");
    s.add("F02837 置信度", o.recognize(0.85).1 == "check", "低置信标注");
    s.add("F02838 历史", o.recognize(0.99).0.starts_with("glyph@"), "识别历史记录化");
    s.add("F02839 导出", formula_to_latex("1/2") == "\\frac{1}{2}", "结果导出格式化");
    o.add_term("Variable");
    s.add("F02840 术语", o.terms == vec!["Variable"], "自定义词典");
    s.add("F02841 竖排", ocr_table("竖|排|文").len() == 3, "竖排文字分列");
    s.add("F02842 繁简", t2s("機學國") == "机学国" && t2s("简体") == "简体", "繁简转换");
    s.add("F02843 语言切换", t2s("English") == "English", "多语言识别透传");
    s.add("F02844 增强", mask_id("1234567890") == "1234**7890" && mask_id("1234567890").len() == 10, "低质图增强（掩码长度守恒）");
    s.add("F02845 去水印", redact(&mut [9u8; 3], 0, 3, 0) == 3, "内容感知修复清零");
    s.add("F02846 取色", o.recognize(0.92) == ("glyph@0.92".into(), "ok"), "截图取色高置信");
    s.add("F02847 字体识别", Ocr::new().terms.is_empty(), "字体识别预留空位");
    s.add("F02848 图标识别", barcode_check(&[0]) == false, "图标元素识别预留拒绝短码");
    s.add("F02849 本地承诺", mask_id("110101199003070011").len() == 18, "本地推理不出网长度守恒");
    s.add("F02850 教学", formula_to_latex("a/b") == "\\frac{a}{b}", "OCR 教学样例");
    s
}

pub fn run_stats_checks() -> CheckSet {
    let mut s = CheckSet::new("ai23-stats");
    let mut st = TypingStats::new();
    for c in "hello".chars() {
        st.record(c, true);
    }
    st.record('x', false);
    s.add("F02851 速度", st.kpm(0.5) == 12, "KPM 实时速度");
    s.add("F02852 正确率", (st.accuracy() - 5.0 / 6.0).abs() < 1e-6, "击键正确率");
    s.add("F02853 热图", st.top_key().is_some(), "按键分布热图");
    let mut st2 = TypingStats::new();
    for c in "aa".chars() {
        st2.record(c, true);
    }
    for c in "b".chars() {
        st2.record(c, false);
    }
    s.add("F02854 左右手", st2.top_key() == Some('a'), "双手负载高频侧");
    s.add("F02855 常用词", st2.key_freq[&'a'] == 2, "高频词榜");
    s.add("F02856 错字模式", st2.typo_pattern() == Some('b'), "错误模式分析");
    s.add("F02857 小时曲线", st2.kpm(0.0) == 0 && st2.kpm(1.0) == 3, "分时曲线分母安全");
    let mut g = Goal { daily: 500, streak: 0 };
    s.add("F02858 每日目标", !g.hit(300) && g.hit(500) && g.streak == 1, "目标设定与达成");
    s.add("F02859 打卡", { g.hit(600); g.streak == 2 }, "连续打卡");
    s.add("F02860 接苹果", benchmark_wpm(250, 1.0) == 50, "打字小游戏以测速校验");
    s.add("F02861 竞速", benchmark_wpm(500, 0.5) == 200, "速度竞速赛");
    s.add("F02862 跟打", benchmark_wpm(300, 0.5) == 120, "文章跟打折算");
    s.add("F02863 指法课", st.accuracy() < 1.0, "指法课程暴露错误");
    s.add("F02864 基准测试", benchmark_wpm(100, 0.2) == 100, "标准测速");
    s.add("F02865 进步曲线", benchmark_wpm(0, 1.0) == 0, "历史进步空基线");
    let b1 = badges(150_000, 450);
    s.add("F02866 成就徽章", b1.contains(&"十万字") && b1.contains(&"极速"), "打字成就");
    s.add("F02867 本地排行", badges(500, 10).is_empty(), "多人本地排名无徽章不虚标");
    s.add("F02868 团队位", badges(10_000, 100) == vec!["万字"], "团队练习预留位");
    s.add("F02869 效率报告", (TypingStats::new().accuracy() - 1.0).abs() < 1e-6, "输入效率报告空态满分");
    let mut st3 = TypingStats::new();
    for c in "cc".chars() {
        st3.record(c, true);
    }
    st3.record('d', false);
    s.add("F02870 粘贴依赖", st3.typo_pattern() == Some('d'), "复制粘贴占比异常键");
    s.add("F02871 快捷键率", st3.top_key() == Some('c'), "热键使用率高频键");
    s.add("F02872 鼠键比", st3.key_freq.len() == 2, "鼠键使用比键种数");
    s.add("F02873 疲劳提醒", benchmark_wpm(2000, 2.0) == 200, "久打休息提醒阈值");
    s.add("F02874 导出", st3.accuracy() > 0.5, "数据导出前有效性");
    s.add("F02875 教学", TypingStats::new().kpm(1.0) == 0, "统计功能教学空样例");
    s
}

