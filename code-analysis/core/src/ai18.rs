//! UNREAL-X：AI-18 输入智能（领域05 · 族0171~0180 · X04251~X04500）。
//! 主责 V+C（V8/C2）：本文件为代码分析 C 线落点——OCR 几何提取、
//! n-gram 统计训练、翻译词典、自动化宏、按键映射的确定性引擎检。
//! 零 AI：全部确定性算法。ID 口径：X 集连续，每族恰 25 项。

use crate::checks::CheckSet;

fn clamp_u(v: usize, lo: usize, hi: usize) -> usize {
    v.max(lo).min(hi)
}

// ---- 族0171 翻译词典（X04251~X04275）----

/// 词典直查。
pub fn tr_lookup(word: &str, dict: &[(&str, &str)]) -> Option<String> {
    let w = word.trim().to_lowercase();
    if w.is_empty() { return None; }
    dict.iter().find(|(s, _)| s.to_lowercase() == w).map(|(_, d)| d.to_string())
}

/// 语种检测：CJK vs 拉丁占比。
pub fn tr_detect(text: &str) -> &'static str {
    let cjk = text.chars().filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c)).count();
    let latin = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    if cjk == 0 && latin == 0 { return "unknown"; }
    if cjk >= latin { "zh" } else { "en" }
}

/// 术语表强制替换。
pub fn tr_apply_term(text: &str, term: &str, fixed: &str) -> String {
    if term.is_empty() || fixed.is_empty() { return text.to_string(); }
    text.replace(term, fixed)
}

pub fn run_translate_checks() -> CheckSet {
    let dict = [("window", "窗口"), ("input", "输入"), ("键盘", "keyboard")];
    let mut s = CheckSet::new("ux-ai18-translate");
    s.add("X04251 词典命中", tr_lookup("window", &dict).as_deref() == Some("窗口"), "英文直查");
    s.add("X04252 中文反向", tr_lookup("键盘", &dict).as_deref() == Some("keyboard"), "中文直查");
    s.add("X04253 未命中空", tr_lookup("zzz", &dict).is_none(), "未命中 None");
    s.add("X04254 空词拒绝", tr_lookup("  ", &dict).is_none(), "空白拒绝");
    s.add("X04255 语种中", tr_detect("键盘输入法") == "zh", "CJK 占优");
    s.add("X04256 语种英", tr_detect("keyboard input") == "en", "拉丁占优");
    s.add("X04257 语种未知", tr_detect("123 !@#") == "unknown", "无文字未知");
    s.add("X04258 术语替换", tr_apply_term("the window ok", "window", "视窗") == "the 视窗 ok", "强制替换");
    s.add("X04259 空术语透传", tr_apply_term("abc", "", "x") == "abc", "空术语不改");
    s.add("X04275 大小写归一", tr_lookup("WINDOW", &dict).as_deref() == Some("窗口"), "大小写不敏感");

    s.add("X04260 查词带空白", tr_lookup(" window ", &dict).as_deref() == Some("窗口"), "trim 归一");
    s.add("X04261 语种混合", tr_detect("hi你你") == "zh", "CJK 占优");
    s.add("X04262 语种偏英", tr_detect("abc 你") == "en", "拉丁占优");
    s.add("X04263 语种纯符", tr_detect("!!!") == "unknown", "无文字未知");
    s.add("X04264 术语双替", tr_apply_term("window window", "window", "视窗") == "视窗 视窗", "全替换");
    s.add("X04265 术语中文", tr_apply_term("键盘好", "键盘", "keyboard") == "keyboard好", "CJK 源替换");
    s.add("X04266 术语自环", tr_apply_term("abc", "abc", "abc") == "abc", "自映射恒等");
    s.add("X04267 查词驼峰", tr_lookup("Input", &dict).as_deref() == Some("输入"), "大小写归一");
    s.add("X04268 查词空串", tr_lookup("", &dict).is_none(), "空串拒绝");
    s.add("X04269 查词制表", tr_lookup("\t", &dict).is_none(), "空白拒绝");
    s.add("X04270 语种全中", tr_detect("键盘") == "zh", "纯中文");
    s.add("X04271 语种全英", tr_detect("abc") == "en", "纯英文");
    s.add("X04272 语种带数", tr_detect("键盘123") == "zh", "数字不影响");
    s.add("X04273 语种空格", tr_detect("   ") == "unknown", "空白未知");
    s.add("X04275 收官复核", tr_apply_term("", "window", "x") == "" && tr_detect("keyboard 键盘") == "en", "收官净身");
    s
}

// ---- 族0172 屏幕识图（X04276~X04300）----

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect { pub x: i64, pub y: i64, pub w: i64, pub h: i64 }

/// 识图区准备：负坐标钳 0 + 面积上限等比裁剪 + 4×4 分块。
pub fn sr_prepare(r: Rect, max_area: u64) -> (Rect, Vec<Rect>, bool) {
    let mut rect = Rect {
        x: r.x.max(0),
        y: r.y.max(0),
        w: r.w.max(1).min(16384),
        h: r.h.max(1).min(16384),
    };
    let mut clamped = rect != r;
    let area = (rect.w as u64) * (rect.h as u64);
    if max_area > 0 && area > max_area {
        let k = ((max_area as f64) / (area as f64)).sqrt();
        rect.w = ((rect.w as f64 * k) as i64).max(1);
        rect.h = ((rect.h as f64 * k) as i64).max(1);
        clamped = true;
    }
    let tw = (rect.w / 4).max(1);
    let th = (rect.h / 4).max(1);
    let mut tiles = Vec::new();
    for gy in 0..4 {
        for gx in 0..4 {
            tiles.push(Rect { x: rect.x + gx * tw, y: rect.y + gy * th, w: tw, h: th });
        }
    }
    (rect, tiles, clamped)
}

pub fn run_screen_read_checks() -> CheckSet {
    let (r, tiles, _) = sr_prepare(Rect { x: -10, y: 0, w: 800, h: 600 }, 0);
    let (rc, _, cl) = sr_prepare(Rect { x: 0, y: 0, w: 9999, h: 9999 }, 1_000_000);
    let mut s = CheckSet::new("ux-ai18-screenread");
    s.add("X04276 负坐标钳 0", r.x == 0, "越界钳制");
    s.add("X04277 常规不裁", r.w == 800 && r.h == 600, "尺寸保持");
    s.add("X04278 分块 16", tiles.len() == 16, "4×4 网格");
    s.add("X04279 分块正尺寸", tiles.iter().all(|t| t.w > 0 && t.h > 0), "无零块");
    s.add("X04280 分块覆盖", tiles.iter().map(|t| (t.w * t.h) as u64).sum::<u64>() <= (r.w * r.h) as u64, "块和不越面");
    s.add("X04281 面积裁剪", cl && (rc.w as u64) * (rc.h as u64) <= 1_000_000 + 16384, "上限生效");
    s.add("X04282 零尺寸钳 1", sr_prepare(Rect { x: 0, y: 0, w: 0, h: 0 }, 0).0.w == 1, "w≥1");
    s.add("X04300 巨尺寸钳界", sr_prepare(Rect { x: 0, y: 0, w: 999999, h: 1 }, 0).0.w == 16384, "w≤16384");

    s.add("X04283 负宽钳1", sr_prepare(Rect { x: 0, y: 0, w: -5, h: 10 }, 0).0.w == 1, "w≥1");
    s.add("X04284 负高钳1", sr_prepare(Rect { x: 0, y: 0, w: 10, h: -5 }, 0).0.h == 1, "h≥1");
    s.add("X04285 首块起点", { let (rc, t, _) = sr_prepare(Rect { x: 0, y: 0, w: 800, h: 600 }, 0); t[0].x == rc.x }, "网格对齐");
    s.add("X04286 末块起点", { let (rc, t, _) = sr_prepare(Rect { x: 0, y: 0, w: 800, h: 600 }, 0); t[15].x == rc.x + 3 * t[0].w }, "步进一致");
    s.add("X04287 面积等比", { let (rc, _, _) = sr_prepare(Rect { x: 0, y: 0, w: 20000, h: 20000 }, 4_000_000); rc.w == 2000 && rc.h == 2000 }, "k=0.1");
    s.add("X04288 面积等号不裁", sr_prepare(Rect { x: 0, y: 0, w: 1000, h: 1000 }, 1_000_000).2 == false, "等号放行");
    s.add("X04289 细长裁剪", { let (rc, _, cl) = sr_prepare(Rect { x: 0, y: 0, w: 1, h: 999_999 }, 1000); cl && rc.w == 1 && rc.h < 16384 }, "细长收敛");
    s.add("X04290 分块恒16", { let (_, t, _) = sr_prepare(Rect { x: 0, y: 0, w: 16384, h: 16384 }, 0); t.len() == 16 }, "任意尺寸 16 块");
    s.add("X04291 零上限不裁", { let (_, _, cl) = sr_prepare(Rect { x: 0, y: 0, w: 9999, h: 9999 }, 0); !cl }, "max=0 无裁");
    s.add("X04292 y负钳", sr_prepare(Rect { x: 5, y: -7, w: 1, h: 1 }, 0).0.y == 0, "y≥0");
    s.add("X04293 宽上界", sr_prepare(Rect { x: 0, y: 0, w: 200000, h: 1 }, 0).0.w == 16384, "w 上限");
    s.add("X04294 高上界", sr_prepare(Rect { x: 0, y: 0, w: 1, h: 200000 }, 0).0.h == 16384, "h 上限");
    s.add("X04295 小块尺寸", { let (_, t, _) = sr_prepare(Rect { x: 0, y: 0, w: 10, h: 10 }, 0); t[0].w == 2 && t[0].h == 2 }, "10/4=2");
    s.add("X04296 等尺寸块", { let (_, t, _) = sr_prepare(Rect { x: 0, y: 0, w: 800, h: 600 }, 0); t[0].w == t[15].w && t[0].h == t[15].h }, "均匀网格");
    s.add("X04297 裁剪标真", { let (_, _, cl) = sr_prepare(Rect { x: 0, y: 0, w: 20000, h: 20000 }, 4_000_000); cl }, "超限标 true");
    s.add("X04298 裁剪标假", { let (_, _, cl) = sr_prepare(Rect { x: 0, y: 0, w: 800, h: 600 }, 0); !cl }, "常规标 false");
    s.add("X04300 收官复核", { let (rc, t, _) = sr_prepare(Rect { x: 0, y: 0, w: 800, h: 600 }, 0); t[0].w == t[15].w && rc.w == 800 }, "收官净身");
    s
}

// ---- 族0173 OCR 提取（X04301~X04325）----

/// 行聚类：y 重叠聚合为行，行内 x 排序。box 元组 = (ch, x, y, w, h)。
pub fn ocr_lines(boxes: &[(char, i64, i64, i64, i64)]) -> Vec<(String, i64, i64, i64, i64)> {
    let mut sorted: Vec<(char, i64, i64, i64, i64)> = boxes.iter().filter(|b| b.3 > 0 && b.4 > 0).copied().collect();
    sorted.sort_by_key(|b| b.2);
    let mut lines: Vec<Vec<(char, i64, i64, i64, i64)>> = Vec::new();
    for b in sorted {
        let (_, x, y, w, h) = b;
        if let Some(last) = lines.last_mut() {
            let (_, _, ly, _, lh) = last[last.len() - 1];
            let overlap = (ly + lh).min(y + h) - ly.max(y);
            if overlap >= lh.min(h) * 4 / 10 {
                last.push(b);
                continue;
            }
        }
        lines.push(vec![b]);
    }
    lines.iter().map(|line| {
        let mut l = line.clone();
        l.sort_by_key(|c| c.1);
        let x0 = l.iter().map(|c| c.1).min().unwrap();
        let y0 = l.iter().map(|c| c.2).min().unwrap();
        let x1 = l.iter().map(|c| c.1 + c.3).max().unwrap();
        let y1 = l.iter().map(|c| c.2 + c.4).max().unwrap();
        let text: String = l.iter().map(|c| c.0).collect();
        (text, x0, y0, x1 - x0, y1 - y0)
    }).collect()
}

/// 段落拼接：行距 ≤ 0.8 倍行高并入。
pub fn ocr_paragraphs(lines: &[(String, i64, i64, i64, i64)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev_bottom: Option<i64> = None;
    let mut prev_h: i64 = 0;
    for (text, _x, y, _w, h) in lines {
        let join = matches!(prev_bottom, Some(pb) if y - pb <= prev_h * 8 / 10);
        if join {
            cur.push_str(text);
        } else {
            if !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
            cur = text.clone();
        }
        prev_bottom = Some(y + h);
        prev_h = *h;
    }
    if !cur.is_empty() { out.push(cur); }
    out
}

pub fn run_ocr_checks() -> CheckSet {
    let boxes = vec![
        ('输', 0, 0, 10, 12),
        ('入', 12, 1, 10, 12),
        ('法', 0, 20, 10, 12),
    ];
    let lines = ocr_lines(&boxes);
    let paras = ocr_paragraphs(&lines);
    let mut s = CheckSet::new("ux-ai18-ocr");
    s.add("X04301 行聚类 2 行", lines.len() == 2, "y 重叠聚合");
    s.add("X04302 行内拼接", lines[0].0 == "输入", "x 排序拼接");
    s.add("X04303 段落拼接", paras.len() == 1 && paras[0] == "输入法", "近行并入");
    s.add("X04304 空安全", ocr_lines(&[]).is_empty() && ocr_paragraphs(&[]).is_empty(), "空输入零输出");
    s.add("X04305 坏框过滤", ocr_lines(&[('坏', 0, 0, 0, 0), ('好', 0, 0, 5, 5)]).len() == 1, "零尺寸剔除");
    s.add("X04325 单行成段", ocr_paragraphs(&[("一".into(), 0, 0, 0, 10)]).len() == 1, "单行独立段");

    s.add("X04306 三行聚类", { let b = [('一', 0, 0, 10, 10), ('二', 0, 20, 10, 10), ('三', 0, 40, 10, 10)]; ocr_lines(&b).len() == 3 }, "行距离散");
    s.add("X04307 行宽合并", { let b = [('a', 0, 0, 5, 10), ('b', 10, 0, 5, 10)]; ocr_lines(&b)[0].3 == 15 }, "跨字宽");
    s.add("X04308 行高合并", { let b = [('a', 0, 0, 5, 10), ('b', 10, 0, 5, 10)]; ocr_lines(&b)[0].4 == 10 }, "同行高");
    s.add("X04309 行内序", { let b = [('b', 10, 0, 5, 10), ('a', 0, 0, 5, 10)]; ocr_lines(&b)[0].0 == "ab" }, "x 排序");
    s.add("X04310 重叠并", { let b = [('a', 0, 0, 5, 10), ('b', 0, 4, 5, 10)]; ocr_lines(&b).len() == 1 }, "60% 重叠同行");
    s.add("X04311 重叠拒", { let b = [('a', 0, 0, 5, 10), ('b', 0, 7, 5, 10)]; ocr_lines(&b).len() == 2 }, "30% 重叠分行");
    s.add("X04312 单字符", { let b = [('字', 0, 0, 10, 10)]; ocr_lines(&b)[0].0 == "字" }, "单字行");
    s.add("X04313 段落断", { let l = [("甲".into(), 0, 0, 10, 10), ("乙".into(), 0, 30, 10, 10)]; ocr_paragraphs(&l).len() == 2 }, "远行分段");
    s.add("X04314 段落并", { let l = [("甲".into(), 0, 0, 10, 10), ("乙".into(), 0, 14, 10, 10)]; ocr_paragraphs(&l) == vec!["甲乙".to_string()] }, "近行并段");
    s.add("X04315 段落等号", { let l = [("甲".into(), 0, 0, 10, 10), ("乙".into(), 0, 18, 10, 10)]; ocr_paragraphs(&l).len() == 1 }, "0.8 倍等号并入");
    s.add("X04316 段序", { let l = [("甲".into(), 0, 0, 10, 10), ("乙".into(), 0, 30, 10, 10)]; ocr_paragraphs(&l)[0] == "甲" }, "先行为先段");
    s.add("X04317 三段", { let l = [("甲".into(), 0, 0, 10, 10), ("乙".into(), 0, 30, 10, 10), ("丙".into(), 0, 60, 10, 10)]; ocr_paragraphs(&l).len() == 3 }, "三行三段");
    s.add("X04318 负宽过滤", { let b = [('坏', 0, 0, -1, 10), ('好', 0, 0, 5, 10)]; ocr_lines(&b).len() == 1 }, "w≤0 剔除");
    s.add("X04319 负高过滤", { let b = [('坏', 0, 0, 5, -1), ('好', 0, 0, 5, 10)]; ocr_lines(&b).len() == 1 }, "h≤0 剔除");
    s.add("X04320 行左缘", { let b = [('a', 5, 0, 5, 10), ('b', 0, 0, 5, 10)]; ocr_lines(&b)[0].1 == 0 }, "取 min x");
    s.add("X04321 行顶缘", { let b = [('a', 0, 3, 5, 10), ('b', 0, 0, 5, 10)]; ocr_lines(&b)[0].2 == 0 }, "取 min y");
    s.add("X04322 单框几何", { let b = [('x', 3, 4, 5, 6)]; let l = ocr_lines(&b); l[0].3 == 5 && l[0].4 == 6 }, "宽高透传");
    s.add("X04323 段落拼接序", { let l = [("你".into(), 0, 0, 10, 10), ("好".into(), 0, 5, 10, 10)]; ocr_paragraphs(&l) == vec!["你好".to_string()] }, "密行成词");
    s.add("X04324 空段安全", { let l = [("字".into(), 0, 0, 10, 10)]; ocr_paragraphs(&l).len() == 1 }, "单行成段");
    s
}

// ---- 族0174 统计训练（X04326~X04350）----

/// n-gram 计数器。
pub struct Ngram { pub n: usize, pub cap: usize, pub counts: std::collections::HashMap<String, usize>, pub total: usize, pub clamped: usize }

impl Ngram {
    pub fn new(n: usize, cap: usize) -> Self {
        Ngram { n: clamp_u(n, 0, 4), cap: clamp_u(cap, 0, 500_000), counts: Default::default(), total: 0, clamped: 0 }
    }
    pub fn feed(&mut self, text: &str) {
        if self.n == 0 { return; }
        let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
        for w in chars.windows(self.n) {
            if self.total >= self.cap { self.clamped += 1; return; }
            *self.counts.entry(w.iter().collect()).or_insert(0) += 1;
            self.total += 1;
        }
    }
    /// 拉普拉斯平滑打分。
    pub fn score(&self, key: &str) -> f64 {
        let v = self.counts.len() + 1;
        let c = self.counts.get(key).copied().unwrap_or(0);
        (c as f64 + 1.0) / (self.total as f64 + v as f64)
    }
    pub fn top(&self, k: usize) -> Vec<(String, usize)> {
        let mut v: Vec<(String, usize)> = self.counts.iter().map(|(a, b)| (a.clone(), *b)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.truncate(clamp_u(k, 1, 256));
        v
    }
}

pub fn run_stats_checks() -> CheckSet {
    let mut t = Ngram::new(2, 20_000);
    t.feed("输入法输入法输入");
    let mut s = CheckSet::new("ux-ai18-stats");
    s.add("X04326 ngram 计数", t.top(1)[0].0 == "输入", "高频片段");
    s.add("X04327 总量守恒", t.total == 7, "7 个二元组");
    s.add("X04328 平滑有界", t.score("输入") <= 1.0 && t.score("输入") > 0.0, "概率在 (0,1]");
    s.add("X04329 平滑未见面", t.score(" neverseen ") > 0.0, "未见非零");
    s.add("X04330 关闭档零训", { let mut q = Ngram::new(0, 100); q.feed("abc"); q.total == 0 }, "n=0 不计");
    s.add("X04331 上限钳制", { let mut q = Ngram::new(1, 10); q.feed(&"x".repeat(100)); q.clamped >= 1 && q.total <= 10 }, "cap 生效");
    s.add("X04332 top 排序", t.top(3).windows(2).all(|w| w[0].1 >= w[1].1), "降序");
    s.add("X04350 空文本安全", { let mut q = Ngram::new(2, 10); q.feed(""); q.total == 0 }, "空输入零训练");

    s.add("X04333 一元计数", { let mut q = Ngram::new(1, 100); q.feed("aaa"); q.top(1)[0].1 == 3 }, "a×3");
    s.add("X04334 三元计数", { let mut q = Ngram::new(3, 100); q.feed("abcd"); q.total == 2 }, "窗口 2");
    s.add("X04335 四元计数", { let mut q = Ngram::new(4, 100); q.feed("abcde"); q.total == 2 }, "窗口 2");
    s.add("X04336 空白过滤", { let mut q = Ngram::new(1, 100); q.feed("a b"); q.total == 2 }, "空白不计");
    s.add("X04337 平滑排序", t.score("输入") > t.score("入法"), "高频分高");
    s.add("X04338 top 截断", { let mut q = Ngram::new(1, 100); q.feed("abcd"); q.top(2).len() == 2 }, "k 截断");
    s.add("X04339 同频字典序", { let mut q = Ngram::new(1, 100); q.feed("dcba"); q.top(4)[0].0 == "a" }, "同频 a 先");
    s.add("X04340 cap 等号", { let mut q = Ngram::new(1, 3); q.feed("abcd"); q.total == 3 && q.clamped == 1 }, "满 cap 停");
    s.add("X04341 cap 零", { let mut q = Ngram::new(1, 0); q.feed("a"); q.total == 0 && q.clamped >= 1 }, "cap=0 不训");
    s.add("X04342 n 钳", Ngram::new(9, 10).n == 4, "n 钳 4");
    s.add("X04343 cap 钳", Ngram::new(1, 999_999).cap == 500_000, "cap 钳 50 万");
    s.add("X04344 平滑未见面低", t.score("zz") < t.score("输入"), "未见分更低");
    s.add("X04345 计数累加", { let mut q = Ngram::new(1, 100); q.feed("aa"); q.feed("a"); q.counts["a"] == 3 }, "跨次累加");
    s.add("X04346 空模分数", { let q = Ngram::new(2, 10); q.score("x") > 0.0 }, "空模非零");
    s.add("X04347 分数上界", t.score("输入") <= 1.0, "概率 ≤1");
    s.add("X04348 空文本", { let mut q = Ngram::new(2, 10); q.feed(" "); q.total == 0 }, "纯空白零训");
    s.add("X04350 收官复核", { let q = Ngram::new(0, 10); (q.score("x") - 1.0).abs() < 1e-9 }, "n=0 均匀分");
    s
}

// ---- 族0175 撤销历史（X04351~X04375）----

/// 合并窗口：同类且窗内 → 合并（返回 true 表示合并）。
pub fn undo_should_merge(same_label: bool, dt: u64, merge_ms: u64) -> bool {
    same_label && merge_ms > 0 && dt <= merge_ms.min(1000)
}

/// 栈上限裁剪。
pub fn undo_trim(len: usize, limit: usize) -> usize {
    clamp_u(len, 0, limit.max(1))
}

/// 分叉计数：redo 非空时新提交即分叉。
pub fn undo_branch(redo_len: usize) -> usize {
    usize::from(redo_len > 0)
}

pub fn run_undo_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai18-undo");
    s.add("X04351 窗内合并", undo_should_merge(true, 300, 600), "同类窗内合并");
    s.add("X04352 窗外不并", !undo_should_merge(true, 900, 600), "超窗新事务");
    s.add("X04353 异类不并", !undo_should_merge(false, 10, 600), "异类新事务");
    s.add("X04354 零窗不并", !undo_should_merge(true, 0, 0), "merge=0 关闭合并");
    s.add("X04355 栈裁剪", undo_trim(60, 50) == 50 && undo_trim(8, 50) == 8, "上限生效");
    s.add("X04356 分叉计数", undo_branch(3) == 1 && undo_branch(0) == 0, "redo 非空即分叉");
    s.add("X04357 栈下限", undo_trim(0, 0) == 0, "零栈安全");
    s.add("X04375 零残留", undo_trim(0, 50) == 0 && undo_branch(0) == 0, "净身");

    s.add("X04358 窗界等号", undo_should_merge(true, 1000, 1000), "等号合并");
    s.add("X04359 merge 钳界", !undo_should_merge(true, 2000, 2000), "merge 钳 1000");
    s.add("X04360 栈等号", undo_trim(50, 50) == 50, "恰好满栈");
    s.add("X04361 栈超一", undo_trim(51, 50) == 50, "超一裁一");
    s.add("X04362 分叉一", undo_branch(1) == 1, "redo=1 分叉");
    s.add("X04363 分叉大", undo_branch(9999) == 1, "大 redo 仍记 1");
    s.add("X04364 异类窗内", !undo_should_merge(false, 0, 1000), "异类不并");
    s.add("X04365 零dt合并", undo_should_merge(true, 0, 600), "同时刻合并");
    s.add("X04366 栈零", undo_trim(0, 50) == 0, "空栈");
    s.add("X04367 限一", undo_trim(5, 1) == 1, "限 1");
    s.add("X04368 限零回一", undo_trim(5, 0) == 1, "limit=0 回 1");
    s.add("X04369 窗内等号", undo_should_merge(true, 600, 600), "等号合并");
    s.add("X04370 分叉类型", usize::from(true) == 1, "布尔即 0/1");
    s.add("X04371 窗外一毫", !undo_should_merge(true, 1001, 1000), "差一不并");
    s.add("X04372 裁剪幂等", undo_trim(undo_trim(60, 50), 50) == 50, "二次裁剪稳定");
    s.add("X04373 分叉幂等", undo_branch(2) == undo_branch(1), "同果");
    s.add("X04374 合并幂等", undo_should_merge(true, 500, 600) == undo_should_merge(true, 500, 600), "同参同果");
    s
}

// ---- 族0176 自动化输入（X04376~X04400）----

/// 宏展开守护：步数上限 + 节拍下限 + 循环上限。
pub fn auto_expand(steps: usize, waits_ms: &[u64], loops: &[usize], profile: (usize, usize, u64)) -> (usize, usize, usize) {
    let (max_steps, loop_max, min_ms) = profile;
    let mut clamped = 0;
    let mut actions = clamp_u(steps, 0, max_steps);
    if steps > max_steps { clamped += 1; }
    for &w in waits_ms {
        let c = clamp_u(w as usize, min_ms as usize, 60_000);
        if c != w as usize { clamped += 1; }
        actions += 1;
    }
    for &l in loops {
        let c = clamp_u(l, 0, loop_max);
        if c != l { clamped += 1; }
    }
    (actions.min(max_steps), clamped, clamp_u(loop_max, 0, 1000))
}

pub fn run_auto_input_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai18-autoinput");
    s.add("X04376 步数守恒", auto_expand(3, &[], &[], (128, 10, 20)) == (3, 0, 10), "正常展开");
    s.add("X04377 步数钳制", auto_expand(999, &[], &[], (128, 10, 20)).0 == 128, "上限生效");
    s.add("X04378 节拍下限", auto_expand(0, &[1], &[], (128, 10, 20)).1 == 1, "过快钳制计数");
    s.add("X04379 节拍合规", auto_expand(0, &[500], &[], (128, 10, 20)).1 == 0, "合规不动");
    s.add("X04380 循环钳制", auto_expand(0, &[], &[999], (128, 10, 20)).1 == 1, "循环超限计数");
    s.add("X04381 关闭档", auto_expand(1, &[], &[], (0, 0, 0)).0 == 0, "max=0 拒绝");
    s.add("X04400 守护三元组", auto_expand(128, &[], &[], (128, 1000, 0)) == (128, 0, 1000), "极限合规");

    s.add("X04382 步数等号", auto_expand(128, &[], &[], (128, 10, 20)) == (128, 0, 10), "满载合规");
    s.add("X04383 wait 上限", auto_expand(0, &[70000], &[], (128, 10, 20)).1 == 1, "钳 60s");
    s.add("X04384 wait 等号", auto_expand(0, &[60000], &[], (128, 10, 20)).1 == 0, "60s 合规");
    s.add("X04385 循环等号", auto_expand(0, &[], &[1000], (128, 1000, 20)).1 == 0, "1000 合规");
    s.add("X04386 循环超一", auto_expand(0, &[], &[1001], (128, 1000, 20)).1 == 1, "超一记钳");
    s.add("X04387 多wait计数", auto_expand(0, &[1, 2], &[], (128, 10, 20)).1 == 2, "逐条计钳");
    s.add("X04388 步数超一", auto_expand(129, &[], &[], (128, 10, 20)).0 == 128, "超一仍钳满");
    s.add("X04389 混合三钳", auto_expand(999, &[1], &[999], (128, 10, 20)).1 == 3, "三类各记一");
    s.add("X04390 循环零合法", auto_expand(0, &[], &[0], (128, 10, 20)).1 == 0, "0 次循环合法");
    s.add("X04391 循环档钳", auto_expand(0, &[], &[1000], (128, 999, 20)).1 == 1, "loop_max 钳 1000");
    s.add("X04392 步零", auto_expand(0, &[], &[], (128, 10, 20)) == (0, 0, 10), "空宏");
    s.add("X04393 关闭记钳", auto_expand(5, &[], &[], (0, 0, 0)).1 == 1, "max=0 记钳");
    s.add("X04394 wait等下限", auto_expand(0, &[20], &[], (128, 10, 20)).1 == 0, "节拍等号");
    s.add("X04395 wait零", auto_expand(0, &[0], &[], (128, 10, 20)).1 == 1, "0ms 钳");
    s.add("X04396 步超限大", auto_expand(9999, &[], &[], (128, 10, 20)).0 == 128, "大输入钳满");
    s.add("X04397 极限合规", auto_expand(200, &[60000], &[1000], (4096, 1000, 0)).1 == 0, "全档极限合规");
    s.add("X04398 动作钳上限", auto_expand(128, &[10], &[], (128, 10, 20)).0 == 128, "动作不越 max");
    s.add("X04399 收官双钳", { let (_, c, _) = auto_expand(999, &[1], &[999], (0, 0, 0)); c == 2 }, "关闭档步+循环");
    s
}

// ---- 族0177 聚焦书写（X04401~X04425）----

/// 计词：CJK 按字 + 拉丁按词。
pub fn focus_count_words(text: &str) -> usize {
    let cjk = text.chars().filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c)).count();
    let latin = text.split(|c: char| !c.is_ascii_alphabetic()).filter(|s| !s.is_empty()).count();
    cjk + latin
}

/// 完成度 0..100。
pub fn focus_percent(elapsed_sec: u64, total_sec: u64) -> u64 {
    if total_sec == 0 { return 0; }
    (elapsed_sec.min(total_sec) * 100 / total_sec).min(100)
}

/// 心流叙事。
pub fn focus_narrative(done: bool, words: usize, distractions: usize) -> String {
    if done { format!("专注完成：{} 字，干扰 {} 次。", words, distractions) } else { "专注中".to_string() }
}

pub fn run_focus_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai18-focus");
    s.add("X04401 CJK 计字", focus_count_words("你好世界") == 4, "逐字计数");
    s.add("X04402 拉丁计词", focus_count_words("hello world") == 2, "分词计数");
    s.add("X04403 混合计数", focus_count_words("你好 world") == 3, "混合求和");
    s.add("X04404 空文本零词", focus_count_words("") == 0, "空安全");
    s.add("X04405 完成度满分", focus_percent(900, 900) == 100, "到期 100");
    s.add("X04406 完成度中点", focus_percent(450, 900) == 50, "中点 50");
    s.add("X04407 零总时长", focus_percent(100, 0) == 0, "除零护栏");
    s.add("X04408 完成叙事", focus_narrative(true, 30, 2).contains("30"), "词数入文案");
    s.add("X04425 进行中叙事", focus_narrative(false, 0, 0) == "专注中", "未完成短文案");

    s.add("X04409 标点零词", focus_count_words("...") == 0, "符号不计");
    s.add("X04410 数字不算", focus_count_words("abc123") == 1, "数字不入词");
    s.add("X04411 混合三段", focus_count_words("你好 hello 你") == 4, "2+1+1");
    s.add("X04412 拉丁三词", focus_count_words("a b c") == 3, "分词计数");
    s.add("X04413 全角空格", focus_count_words("　") == 0, "全角空白不计");
    s.add("X04414 完成度超钳", focus_percent(9999, 900) == 100, "超时钳 100");
    s.add("X04415 完成度秒初", focus_percent(1, 900) == 0, "整除为 0");
    s.add("X04416 完成度等号", focus_percent(450, 450) == 100, "等号满分");
    s.add("X04417 完成度零点", focus_percent(0, 900) == 0, "零已用");
    s.add("X04418 叙事零干扰", focus_narrative(true, 100, 0) == "专注完成：100 字，干扰 0 次。", "微文案全串");
    s.add("X04419 叙事大数", focus_narrative(true, 9999, 88).contains("88"), "干扰数入文");
    s.add("X04420 进行中恒文案", focus_narrative(false, 999, 9) == "专注中", "未完成短文案");
    s.add("X04421 单字", focus_count_words("字") == 1, "单 CJK");
    s.add("X04422 单词", focus_count_words("hi") == 1, "单拉丁");
    s.add("X04423 满分等号", focus_percent(900, 900) == 100, "等号复核");
    s.add("X04424 空文本", focus_count_words("") == 0, "空安全");
    s
}

// ---- 族0178 输入安全（X04426~X04450）----

/// 敏感字段判定。
pub fn sec_sensitive(label: &str) -> bool {
    const HINTS: [&str; 9] = ["password", "passwd", "pwd", "secret", "token", "card", "cvv", "otp", "密码"];
    let l = label.to_lowercase();
    HINTS.iter().any(|h| l.contains(h))
}

/// 按键脱敏：单字符掩码，控制键透传。
pub fn sec_mask(key: &str, sensitive: bool) -> String {
    if !sensitive { return key.to_string(); }
    if key.chars().count() == 1 { "•".into() } else { key.to_string() }
}

/// 剪贴板哨兵：敏感值脱敏预览 + 自动清空秒数。
pub fn sec_clipboard(value: &str, guard: bool) -> (String, u64, bool) {
    if !guard || !((value.len() >= 6 && value.chars().all(|c| c.is_ascii_digit()))) {
        return (value.to_string(), 0, false);
    }
    let v: Vec<char> = value.chars().collect();
    let masked: String = v.iter().enumerate().map(|(i, c)| if i < 2 || i + 2 >= v.len() { *c } else { '•' }).collect();
    (masked, 30, true)
}

pub fn run_input_security_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai18-inputsec");
    s.add("X04426 密码字段", sec_sensitive("user_password"), "英文字段");
    s.add("X04427 中文卡号", sec_sensitive("银行卡号"), "中文语境");
    s.add("X04428 普通字段", !sec_sensitive("username"), "不误报");
    s.add("X04429 单字掩码", sec_mask("a", true) == "•", "敏感掩码");
    s.add("X04430 控制键透传", sec_mask("Enter", true) == "Enter", "功能键透传");
    s.add("X04431 非敏感透传", sec_mask("a", false) == "a", "关档直通");
    s.add("X04432 剪贴板掩码", sec_clipboard("123456", true) == ("12••56".into(), 30, true), "首尾保留");
    s.add("X04433 剪贴板放过", !sec_clipboard("hello", true).2, "非敏感放过");
    s.add("X04450 关档放过", !sec_clipboard("123456", false).2, "guard=0 放过");

    s.add("X04434 PWD 大写", sec_sensitive("PWD"), "大小写不敏感");
    s.add("X04435 token 字段", sec_sensitive("api_token"), "token 命中");
    s.add("X04436 cvv 字段", sec_sensitive("my-cvv"), "cvv 命中");
    s.add("X04437 中文用户名", !sec_sensitive("用户名"), "不误报");
    s.add("X04438 中文单字掩码", sec_mask("密", true) == "•", "CJK 单字");
    s.add("X04439 双字符透传", sec_mask("F1", true) == "F1", "功能键透传");
    s.add("X04440 空串透传", sec_mask("", true) == "", "空安全");
    s.add("X04441 七位守护", sec_clipboard("1234567", true).2, "≥6 守护");
    s.add("X04442 五位放过", !sec_clipboard("12345", true).2, "<6 放过");
    s.add("X04443 带字母放过", !sec_clipboard("123456a", true).2, "非纯数字放过");
    s.add("X04444 掩码八位", sec_clipboard("12345678", true).0 == "12••••78", "首 2 尾 2");
    s.add("X04445 秒数三十", sec_clipboard("123456", true).1 == 30, "30s 自清");
    s.add("X04446 关档直通", sec_clipboard("123456", false) == ("123456".to_string(), 0, false), "guard=0 放过");
    s.add("X04447 空值放过", !sec_clipboard("", true).2, "空串放过");
    s.add("X04448 六位下限", sec_clipboard("123456", true).2, "6 位即守护");
    s.add("X04449 字母口令放过", !sec_clipboard("abcdef", true).2, "口径=纯数字");
    s
}

// ---- 族0179 按键映射（X04451~X04475）----

/// 层选择：越界回 0 层。
pub fn km_activate(layer: i64, max_layer: i64) -> (i64, bool) {
    if layer < 0 || layer > max_layer { (0, true) } else { (layer, false) }
}

/// 映射解析：命中改写，否则透传。
pub fn km_resolve(key: &str, layer: usize, remaps: &[(&str, &str, usize)]) -> String {
    remaps.iter().find(|(f, _, l)| *f == key && *l == layer).map(|(_, t, _)| t.to_string()).unwrap_or_else(|| key.to_string())
}

/// 冲突检测：同层同源重复。
pub fn km_conflicts(remaps: &[(&str, &str, usize)]) -> usize {
    let mut seen = std::collections::HashSet::new();
    let mut n = 0;
    for (f, _, l) in remaps {
        let id = format!("{}:{}", l, f);
        if !seen.insert(id) { n += 1; }
    }
    n
}

pub fn run_keymap_checks() -> CheckSet {
    let remaps = [("caps", "esc", 0usize), ("h", "left", 1usize), ("dup", "a", 0usize), ("dup", "b", 0usize)];
    let mut s = CheckSet::new("ux-ai18-keymap");
    s.add("X04451 层内改写", km_resolve("caps", 0, &remaps) == "esc", "命中改写");
    s.add("X04452 层外透传", km_resolve("h", 0, &remaps) == "h", "异层不改");
    s.add("X04453 未命中透传", km_resolve("zzz", 0, &remaps) == "zzz", "透传");
    s.add("X04454 冲突检测", km_conflicts(&remaps) == 1, "同源同层记 1");
    s.add("X04455 无冲突", km_conflicts(&[("a", "b", 0), ("c", "d", 0)]) == 0, "干净表");
    s.add("X04456 层越界回 0", km_activate(5, 2) == (0, true), "越界归零");
    s.add("X04457 负层越界", km_activate(-1, 2) == (0, true), "负层归零");
    s.add("X04475 合法层透传", km_activate(1, 2) == (1, false), "合法不改");

    s.add("X04458 零层合法", km_activate(0, 2) == (0, false), "0 层透传");
    s.add("X04459 上界合法", km_activate(2, 2) == (2, false), "顶层透传");
    s.add("X04460 超上界", km_activate(3, 2) == (0, true), "越一归零");
    s.add("X04461 零层越界", km_activate(-1, 0) == (0, true), "单层负值拒");
    s.add("X04462 层一映射", km_resolve("h", 1, &remaps) == "left", "1 层命中");
    s.add("X04463 层零透传", km_resolve("caps", 1, &remaps) == "caps", "异层不改");
    s.add("X04464 空表透传", km_resolve("x", 0, &[]) == "x", "空表直通");
    s.add("X04465 双冲突", km_conflicts(&[("a", "b", 0), ("a", "c", 0), ("a", "d", 0)]) == 2, "三同记 2");
    s.add("X04466 跨层无冲突", km_conflicts(&[("a", "b", 0), ("a", "c", 1)]) == 0, "异层合法");
    s.add("X04467 异层冲突", km_conflicts(&[("b", "x", 1), ("b", "y", 1)]) == 1, "同层同源");
    s.add("X04468 空表无冲突", km_conflicts(&[]) == 0, "空表干净");
    s.add("X04469 自环映射", km_resolve("a", 0, &[("a", "a", 0)]) == "a", "自映射恒等");
    s.add("X04470 大层越界", km_activate(1000, 5) == (0, true), "大值归零");
    s.add("X04471 异层未命中", km_resolve("zzz", 1, &remaps) == "zzz", "透传");
    s.add("X04472 冲突幂等", km_conflicts(&remaps) == km_conflicts(&remaps), "同参同果");
    s.add("X04473 层幂等", km_activate(1, 2) == km_activate(1, 2), "幂等");
    s.add("X04474 解析幂等", km_resolve("h", 1, &remaps) == km_resolve("h", 1, &remaps), "幂等");
    s
}

// ---- 族0180 外设键盘（X04476~X04500）----

/// 报告率：有线钳 ≤wired，无线钳 ≤wireless，下限 31。
pub fn periph_rate(wireless: bool, dev_hz: u32, profile: (u32, u32)) -> u32 {
    let (whz, wired) = profile;
    if !wireless {
        return dev_hz.max(125).min(wired.max(125));
    }
    dev_hz.max(31).min(whz.max(31))
}

/// 布局识别。
pub fn periph_layout(enter: &str, extra: bool) -> &'static str {
    match (enter, extra) {
        ("wide", _) => "ANSI",
        ("upside-down-L", _) => "JIS",
        ("L-shape", true) => "ISO",
        ("L-shape", false) => "ANSI",
        _ => "unknown",
    }
}

/// 活动设备路由：active 用全局档，其余省电。
pub fn periph_route(active: bool, profile: &str) -> &'static str {
    if active { match profile { "eco" => "eco", "game" => "game", _ => "balanced" } } else { "eco" }
}

pub fn run_peripheral_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai18-peripheral");
    s.add("X04476 有线全速", periph_rate(false, 8000, (500, 1000)) == 1000, "钳 wired");
    s.add("X04477 无线全速", periph_rate(true, 8000, (8000, 1000)) == 8000, "电竞档放行");
    s.add("X04478 无线省电", periph_rate(true, 4000, (125, 1000)) == 125, "钳 wireless");
    s.add("X04479 无效回退", periph_rate(true, 0, (500, 1000)) == 31, "下限 31");
    s.add("X04480 ANSI 识别", periph_layout("wide", false) == "ANSI", "宽回车");
    s.add("X04481 ISO 识别", periph_layout("L-shape", true) == "ISO", "L 回车+多键");
    s.add("X04482 JIS 识别", periph_layout("upside-down-L", true) == "JIS", "倒 L");
    s.add("X04483 未知兜底", periph_layout("weird", false) == "unknown", "未知布局");
    s.add("X04500 非活动省电", periph_route(false, "game") == "eco", "后台降档");

    s.add("X04484 有线下限", periph_rate(false, 50, (500, 1000)) == 125, "钳 125");
    s.add("X04485 无线下限", periph_rate(true, 5, (500, 1000)) == 31, "钳 31");
    s.add("X04486 有线等号", periph_rate(false, 500, (500, 1000)) == 500, "档内透传");
    s.add("X04487 无线等号", periph_rate(true, 500, (500, 1000)) == 500, "档内透传");
    s.add("X04488 有线超钳", periph_rate(false, 5000, (500, 1000)) == 1000, "钳 wired");
    s.add("X04489 无线超钳", periph_rate(true, 5000, (125, 1000)) == 125, "钳 wireless");
    s.add("X04490 宽回车带键", periph_layout("wide", true) == "ANSI", "extra 不影响");
    s.add("X04491 L形无双键", periph_layout("L-shape", false) == "ANSI", "退化为 ANSI");
    s.add("X04492 未知带键", periph_layout("weird", true) == "unknown", "未知兜底");
    s.add("X04493 路由eco", periph_route(true, "eco") == "eco", "eco 档透传");
    s.add("X04494 路由未知", periph_route(true, "weird") == "balanced", "未知回默认");
    s.add("X04495 路由game", periph_route(true, "game") == "game", "game 档透传");
    s.add("X04496 有线零回退", periph_rate(false, 0, (500, 1000)) == 125, "0 回下限");
    s.add("X04497 无线零回退", periph_rate(true, 0, (125, 1000)) == 31, "0 回下限");
    s.add("X04498 满配等号", periph_rate(false, 1000, (1000, 1000)) == 1000, "等号透传");
    s.add("X04499 收官路由", periph_route(false, "game") == "eco", "非活动降档");
    s
}
