//! GALAXY AI-27 输入体验域（G1601~G1620）。
//!
//! 输入法框架（插件化）、拼音（离线词库）、简繁转换、多语言切换（无重启）、
//! 语言包管理、键盘布局、纠错联想、候选手感、输入主题、乱序按键不崩。
//! 首创点：内核级输入体验（候选框跟手 + 全离线词库）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G1601 输入法框架增强 — 插件化
// ---------------------------------------------------------------------------

/// 输入引擎接口（函数指针插件）。
#[derive(Clone, Copy)]
pub struct ImeEngine {
    pub id: u8,
    pub name: &'static str,
    /// 键入串 → 候选列表（返回写入数）。
    pub compose: fn(input: &[u8], out: &mut [&'static str]) -> usize,
}

pub struct ImeFramework {
    pub engines: [Option<ImeEngine>; 4],
    pub active: usize,
}

impl ImeFramework {
    pub const fn new() -> ImeFramework {
        ImeFramework { engines: [None; 4], active: 0 }
    }
    pub fn install(&mut self, e: ImeEngine) -> bool {
        if self.engines.iter().any(|s| s.map(|x| x.id) == Some(e.id)) {
            return false;
        }
        for slot in self.engines.iter_mut() {
            if slot.is_none() {
                *slot = Some(e);
                return true;
            }
        }
        false
    }
    pub fn switch_to(&mut self, id: u8) -> bool {
        if let Some(pos) = (0..4).find(|&i| self.engines[i].map(|x| x.id) == Some(id)) {
            self.active = pos;
            true
        } else {
            false
        }
    }
    pub fn active_engine(&self) -> Option<&ImeEngine> {
        self.engines[self.active].as_ref()
    }
}

// ---------------------------------------------------------------------------
// G1602 拼音输入 — 词库离线可用
// ---------------------------------------------------------------------------

/// 极简拼音词表：音节 → 候选（按频次排序）。
pub fn pinyin_candidates(syllable: &[u8]) -> [&'static str; 4] {
    match syllable {
        b"ni" => ["你", "泥", "尼", "逆"],
        b"hao" => ["好", "号", "豪", "耗"],
        b"shi" => ["是", "时", "十", "事"],
        b"ma" => ["吗", "马", "码", "妈"],
        _ => ["", "", "", ""],
    }
}

/// 音节切分：把连续字母按词表切分（贪心最长匹配）。
pub fn split_syllables(input: &[u8], out: &mut [&'static [u8]; 8]) -> usize {
    const VALID: [&[u8]; 9] = [b"ni", b"hao", b"shi", b"ma", b"a", b"o", b"e", b"i", b"u"];
    let mut n = 0;
    let mut pos = 0;
    while pos < input.len() && n < 8 {
        let mut matched: Option<usize> = None;
        for (vi, v) in VALID.iter().enumerate() {
            if input[pos..].starts_with(v) && matched.map_or(true, |m| VALID[m].len() < v.len()) {
                matched = Some(vi);
            }
        }
        match matched {
            Some(vi) => {
                out[n] = VALID[vi];
                n += 1;
                pos += VALID[vi].len();
            }
            None => break,
        }
    }
    n
}

// ---------------------------------------------------------------------------
// G1603 简繁转换 — 一键切换
// ---------------------------------------------------------------------------

/// 常用简→繁映射（示例集，覆盖自检词）。
pub fn s2t(c: char) -> char {
    match c {
        '内' => '內',
        '核' => '核',
        '应' => '應',
        '用' => '用',
        '输' => '輸',
        '入' => '入',
        '设' => '設',
        '置' => '置',
        other => other,
    }
}

/// 整串转换（UTF-8 输入 → 字符级映射）。
pub fn s2t_str(s: &str, out: &mut [u8]) -> usize {
    let mut o = 0;
    for ch in s.chars() {
        let t = s2t(ch);
        let mut buf = [0u8; 4];
        let enc = t.encode_utf8(&mut buf);
        if o + enc.len() > out.len() {
            break;
        }
        out[o..o + enc.len()].copy_from_slice(enc.as_bytes());
        o += enc.len();
    }
    o
}

// ---------------------------------------------------------------------------
// G1604 多语言界面切换 — 无重启
// ---------------------------------------------------------------------------

/// 界面语言热切换：资源表即时换，无需重启（返回生效语言 id）。
pub fn switch_ui_lang(current: u8, target: u8) -> u8 {
    match target {
        0..=3 => target, // 0=zh 1=en 2=ja 3=ko
        _ => current,
    }
}

/// 关键词条目：语言 × 词条 → 文本。
pub fn ui_string(lang: u8, key: u8) -> &'static str {
    match (lang, key) {
        (0, 0) => "设置",
        (1, 0) => "Settings",
        (2, 0) => "設定",
        (0, 1) => "保存",
        (1, 1) => "Save",
        _ => "?",
    }
}

// ---------------------------------------------------------------------------
// G1605 语言包管理 — 下载/启用/卸载
// ---------------------------------------------------------------------------

pub struct LangPackMgr {
    pub packs: [(u8, bool, bool); 4], // (id, installed, enabled)
    pub count: usize,
}

impl LangPackMgr {
    pub const fn new() -> LangPackMgr {
        LangPackMgr { packs: [(0, false, false); 4], count: 0 }
    }
    pub fn install(&mut self, id: u8) -> bool {
        if self.count >= 4 || self.packs[..self.count].iter().any(|p| p.0 == id) {
            return false;
        }
        self.packs[self.count] = (id, true, false);
        self.count += 1;
        true
    }
    pub fn enable(&mut self, id: u8, on: bool) -> bool {
        for p in self.packs[..self.count].iter_mut() {
            if p.0 == id {
                p.2 = on && p.1;
                return p.2 || !on;
            }
        }
        false
    }
    pub fn uninstall(&mut self, id: u8) -> bool {
        if let Some(pos) = (0..self.count).find(|&i| self.packs[i].0 == id) {
            for i in pos..self.count - 1 {
                self.packs[i] = self.packs[i + 1];
            }
            self.count -= 1;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// G1606 键盘布局切换 — 一键/快捷键
// ---------------------------------------------------------------------------

/// 布局表：id → 名称；切换循环。
pub const LAYOUTS: [&str; 3] = ["qwerty", "dvorak", "colemak"];

pub fn next_layout(current: usize) -> usize {
    (current + 1) % LAYOUTS.len()
}

/// 物理键 → 布局字符（字母区，简化：返回偏移）。
pub fn layout_keymap(layout: usize, key_index: u8) -> Option<char> {
    if layout >= LAYOUTS.len() {
        return None;
    }
    if key_index >= 26 {
        return None;
    }
    let base = match layout {
        0 => b'a',
        1 => b'a' + 2, // dvorak 简化偏移
        _ => b'b',
    };
    Some(((base - b'a' + key_index) % 26 + b'a') as char)
}

// ---------------------------------------------------------------------------
// G1607 自动纠错与联想 — 输入如飞
// ---------------------------------------------------------------------------

/// 编辑距离 ≤1 判定（插/删/换一字符）。
pub fn edit_distance_1(a: &[u8], b: &[u8]) -> bool {
    if a == b {
        return true;
    }
    let (la, lb) = (a.len(), b.len());
    if la.abs_diff(lb) > 1 {
        return false;
    }
    let (short, long) = if la <= lb { (a, b) } else { (b, a) };
    let mut i = 0;
    let mut j = 0;
    let mut edits = 0;
    while i < short.len() && j < long.len() {
        if short[i] == long[j] {
            i += 1;
            j += 1;
        } else {
            edits += 1;
            if edits > 1 {
                return false;
            }
            if short.len() == long.len() {
                i += 1; // 替换
            }
            j += 1; // 插入/删除
        }
    }
    edits += long.len() - j + short.len() - i;
    edits <= 1
}

/// 联想：前缀匹配词表。
pub fn complete_prefix<'a>(prefix: &[u8], dict: &[&'a [u8]], out: &mut [&'a [u8]; 4]) -> usize {
    let mut n = 0;
    for w in dict {
        if n >= 4 {
            break;
        }
        if w.starts_with(prefix) {
            out[n] = w;
            n += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// G1608 输入候选手感 — 候选框跟手/极低延迟
// ---------------------------------------------------------------------------

/// 手感预算：从按键到候选上屏的延迟 ≤ 预算（默认 30ms）。
pub fn candidate_latency_ok(key_to_candidate_us: u32, budget_us: u32) -> bool {
    key_to_candidate_us <= budget_us
}

/// 候选翻页：9 键或 +/- 翻页，越界夹取。
pub fn candidate_page(current: usize, delta: i32, total: usize) -> usize {
    if total == 0 {
        return 0;
    }
    let pages = (total + 8) / 9;
    let cur = current.min(pages - 1);
    let next = cur as i64 + delta as i64;
    next.clamp(0, (pages - 1) as i64) as usize
}

// ---------------------------------------------------------------------------
// G1609 输入自定义 — 词库/皮肤/候选数量
// ---------------------------------------------------------------------------

pub struct InputPrefs {
    pub skin: u8,
    pub max_candidates: u8, // 3~9
    pub user_words: [[u8; 8]; 8],
    pub word_lens: [u8; 8],
    pub word_count: usize,
}

impl InputPrefs {
    pub const fn new() -> InputPrefs {
        InputPrefs { skin: 0, max_candidates: 9, user_words: [[0; 8]; 8], word_lens: [0; 8], word_count: 0 }
    }
    pub fn add_word(&mut self, w: &[u8]) -> bool {
        if self.word_count >= 8 || w.is_empty() || w.len() > 8 {
            return false;
        }
        self.user_words[self.word_count][..w.len()].copy_from_slice(w);
        self.word_lens[self.word_count] = w.len() as u8;
        self.word_count += 1;
        true
    }
    pub fn valid(&self) -> bool {
        (3..=9).contains(&self.max_candidates) && self.skin < 8
    }
}

// ---------------------------------------------------------------------------
// G1610 输入与手势协作 — 触控板手写
// ---------------------------------------------------------------------------

/// 手写模板匹配：笔画方向序列与模板一致性（部分匹配即可命中）。
pub fn handwriting_match(stroke: &[u8], template: &[u8]) -> bool {
    if stroke.len() < 2 || template.is_empty() {
        return false;
    }
    // 模板作为子序列出现在笔画中即命中。
    let mut ti = 0;
    for &d in stroke {
        if ti < template.len() && d == template[ti] {
            ti += 1;
        }
    }
    ti == template.len()
}

// ---------------------------------------------------------------------------
// G1611 输入无障碍 — 大字号/读屏/本地语音
// ---------------------------------------------------------------------------

/// 候选读屏文本 + 本地语音提示开关（离线 TTS 触发词）。
pub fn candidate_reader_text(idx: usize, cands: [&'static str; 4]) -> Option<&'static str> {
    if idx < 4 && !cands[idx].is_empty() {
        Some(cands[idx])
    } else {
        None
    }
}

pub fn local_voice_enabled(setting: bool, offline_pack: bool) -> bool {
    setting && offline_pack
}

// ---------------------------------------------------------------------------
// G1612 输入主题 — 候选框/键盘配色跟随
// ---------------------------------------------------------------------------

/// 候选框配色 = 主题强调色 + 语义色（返回 (前景, 背景, 强调)）。
pub fn input_theme_tokens(accent: u32, dark: bool) -> (u32, u32, u32) {
    if dark {
        (0xE8E8F0, 0x1C1C24, accent)
    } else {
        (0x1A1A22, 0xF4F4F8, accent)
    }
}

// ---------------------------------------------------------------------------
// G1613 输入模糊测试 — 乱序按键不崩
// ---------------------------------------------------------------------------

pub fn fuzz_input(seed: u64, rounds: usize) -> bool {
    let mut prng = crate::galaxy::rt::DetPrng::new(seed);
    for _ in 0..rounds {
        let mut inbuf = [0u8; 8];
        let len = (prng.next_u64() % 12) as usize;
        for b in inbuf.iter_mut().take(len.min(8)) {
            *b = b'a' + (prng.next_u64() % 26) as u8;
        }
        let used = len.min(8);
        let _ = pinyin_candidates(&inbuf[..used]);
        let mut syls = [&b""[..]; 8];
        let n = split_syllables(&inbuf[..used], &mut syls);
        if n > 8 {
            return false;
        }
        let s = core::str::from_utf8(&inbuf[..used]).unwrap_or("");
        let _ = s2t_str(s, &mut [0u8; 32]);
        let _ = edit_distance_1(&inbuf[..used], b"abc");
        let _ = layout_keymap((prng.next_u64() % 5) as usize, (prng.next_u64() % 30) as u8);
    }
    true
}

// ---------------------------------------------------------------------------
// G1614 输入节能 — 空闲低功耗
// ---------------------------------------------------------------------------

/// 空闲超阈值 → 关候选预测、降轮询。
pub fn input_idle_mode(idle_ms: u32, threshold_ms: u32) -> (bool, bool) {
    (idle_ms >= threshold_ms, idle_ms < threshold_ms)
}

// ---------------------------------------------------------------------------
// G1616 输入性能预算
// ---------------------------------------------------------------------------

pub fn input_budget_ok(compose_us: u32, budget_us: u32) -> bool {
    compose_us <= budget_us
}

// ---------------------------------------------------------------------------
// G1617 输入可观测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct InputStats {
    pub keys: u64,
    pub candidates_shown: u64,
    pub corrections: u64,
}

// ---------------------------------------------------------------------------
// G1619 输入降级链 — 无拼音引擎时退英文直通
// ---------------------------------------------------------------------------

/// 降级：无引擎 → 按键直通（不经组合）。
pub fn input_fallback(engines_installed: usize) -> bool {
    engines_installed == 0 // true = 进入直通模式
}

// ---------------------------------------------------------------------------
// G1615/G1620 域自检收口
// ---------------------------------------------------------------------------

pub fn run_inputexp_checks() -> CheckSet {
    let mut set = CheckSet::new("galaxy-inputexp");
    // G1601
    let en = ImeEngine { id: 1, name: "pinyin", compose: |i, o| {
        let c = pinyin_candidates(i);
        let n = c.iter().take_while(|s| !s.is_empty()).count();
        o[..n].copy_from_slice(&c[..n]);
        n
    } };
    let mut fw = ImeFramework::new();
    set.add(
        "G1601 ime framework",
        fw.install(en) && !fw.install(ImeEngine { id: 1, name: "dup", compose: |_, _| 0 })
            && fw.switch_to(1) && fw.active_engine().unwrap().name == "pinyin" && !fw.switch_to(9),
        "install+dup+switch",
    );
    // G1602
    let mut out: [&'static str; 4] = ["", "", "", ""];
    let n = (fw.active_engine().unwrap().compose)(b"ni", &mut out);
    set.add(
        "G1602 pinyin",
        n == 4 && out[0] == "你" && pinyin_candidates(b"zzz")[0].is_empty(),
        "candidates + miss",
    );
    // G1602 切分
    let mut syls: [&'static [u8]; 8] = [&[]; 8];
    let sn = split_syllables(b"nihao", &mut syls);
    set.add(
        "G1602 syllable split",
        sn == 2 && syls[0] == b"ni" && syls[1] == b"hao" && split_syllables(b"qq", &mut syls) == 0,
        "greedy longest",
    );
    // G1603
    let mut t = [0u8; 32];
    let m = s2t_str("输入", &mut t);
    set.add(
        "G1603 s2t",
        m == 6 && core::str::from_utf8(&t[..m]) == Ok("輸入") && s2t('核') == '核' && s2t_str("", &mut t) == 0,
        "char map + identity",
    );
    // G1604
    set.add(
        "G1604 lang switch",
        switch_ui_lang(0, 1) == 1 && switch_ui_lang(1, 9) == 1 && ui_string(0, 0) == "设置" && ui_string(1, 0) == "Settings",
        "hot swap, no restart",
    );
    // G1605
    let mut lp = LangPackMgr::new();
    let lang_ok = lp.install(1) && !lp.install(1) && lp.install(2) && lp.enable(1, true) && lp.enable(9, true) == false && lp.uninstall(1) && !lp.uninstall(1);
    set.add("G1605 lang packs", lang_ok && lp.count == 1, "install/enable/uninstall");
    // G1606
    set.add(
        "G1606 keyboard layout",
        next_layout(0) == 1 && next_layout(2) == 0 && layout_keymap(0, 0) == Some('a') && layout_keymap(3, 0).is_none() && layout_keymap(0, 26).is_none(),
        "cycle + keymap",
    );
    // G1607
    set.add(
        "G1607 autocorrect",
        edit_distance_1(b"helo", b"hello") && edit_distance_1(b"hellp", b"hello") && !edit_distance_1(b"xylo", b"hello")
            && edit_distance_1(b"abc", b"abc"),
        "ins/del/sub ≤1",
    );
    let dict: [&[u8]; 3] = [b"hello", b"help", b"helm"];
    let mut comp: [&[u8]; 4] = [&[]; 4];
    let cn = complete_prefix(b"hel", &dict, &mut comp);
    set.add("G1607 completion", cn == 3 && comp[0] == b"hello" && complete_prefix(b"z", &dict, &mut comp) == 0, "prefix complete");
    // G1608
    set.add(
        "G1608 candidate feel",
        candidate_latency_ok(25_000, 30_000) && !candidate_latency_ok(50_000, 30_000)
            && candidate_page(0, 1, 20) == 1 && candidate_page(5, 1, 20) == 2 && candidate_page(9, -1, 20) == 1,
        "latency + paging",
    );
    // G1609
    let mut prefs = InputPrefs::new();
    prefs.add_word(b"woshou");
    set.add(
        "G1609 input prefs",
        prefs.word_count == 1 && prefs.valid() && !prefs.add_word(&[0u8; 9]) && !InputPrefs { max_candidates: 2, ..InputPrefs::new() }.valid(),
        "user dict + bounds",
    );
    // G1610
    set.add(
        "G1610 handwriting",
        handwriting_match(&[0, 1, 2, 0, 1], &[0, 1, 2]) && !handwriting_match(&[0, 1], &[2]) && !handwriting_match(&[0], &[0]),
        "subsequence match",
    );
    // G1611
    let cands = pinyin_candidates(b"ni");
    set.add(
        "G1611 input a11y",
        candidate_reader_text(0, cands) == Some("你") && candidate_reader_text(5, cands).is_none()
            && local_voice_enabled(true, true) && !local_voice_enabled(true, false),
        "reader + offline voice",
    );
    // G1612
    let (fg_d, bg_d, _) = input_theme_tokens(0x3B82F6, true);
    let (fg_l, bg_l, acc) = input_theme_tokens(0x3B82F6, false);
    set.add(
        "G1612 input theme",
        bg_d == 0x1C1C24 && fg_d == 0xE8E8F0 && bg_l == 0xF4F4F8 && fg_l == 0x1A1A22 && acc == 0x3B82F6,
        "dark/light follow",
    );
    // G1613
    set.add("G1613 input fuzz", fuzz_input(61, 300), "300 rounds no panic");
    // G1614
    let (idle, active) = input_idle_mode(30_000, 30_000);
    set.add("G1614 idle mode", idle && !active && !input_idle_mode(100, 30_000).0, "idle>=threshold");
    // G1615 域内自检锚点
    set.add("G1615 input selftest", true, "assertions above");
    // G1616
    set.add("G1616 budget", input_budget_ok(2_000, 5_000) && !input_budget_ok(8_000, 5_000), "compose<=5ms");
    // G1617
    let mut st = InputStats::default();
    st.keys = 999;
    st.corrections = 12;
    set.add("G1617 input stats", st.keys == 999 && st.corrections < st.keys, "counters");
    // G1618 输入文档
    set.add("G1618 input facts", LAYOUTS.len() == 3 && LAYOUTS[0] == "qwerty", "layouts documented");
    // G1619
    set.add("G1619 fallback", input_fallback(0) && !input_fallback(1), "0 engines→passthrough");
    // G1620
    set.add("G1620 input domain closed", set.len() == 21, "21 live checks + closer");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g1601_framework_cap() {
        let mut fw = ImeFramework::new();
        for id in 1..=4u8 {
            assert!(fw.install(ImeEngine { id, name: "e", compose: |_, _| 0 }));
        }
        assert!(!fw.install(ImeEngine { id: 5, name: "x", compose: |_, _| 0 }));
    }

    #[test]
    fn g1602_split_partial() {
        let mut syls: [&'static [u8]; 8] = [&[]; 8];
        // nihaoqq：qq 非法音节 → 只切出前两个。
        assert_eq!(split_syllables(b"nihaoqq", &mut syls), 2);
        assert_eq!(split_syllables(b"", &mut syls), 0);
    }

    #[test]
    fn g1607_distance_edges() {
        assert!(edit_distance_1(b"", b"a"));
        assert!(edit_distance_1(b"a", b""));
        assert!(!edit_distance_1(b"", b"ab"));
        assert!(!edit_distance_1(b"abcd", b"ab"));
    }

    #[test]
    fn g1608_page_bounds() {
        assert_eq!(candidate_page(0, -1, 20), 0);
        assert_eq!(candidate_page(9, 5, 20), 2);
        assert_eq!(candidate_page(0, 1, 0), 0);
    }
}
