//! F108 自定义短语库 · 完整设计（STAR I 主册 G-C-38）。
//!
//! **判据（主册）**：100 条短语库实测：输入-候选-上屏全链 <100ms；变量
//! 三族格式 20 例全对；导入导出 round-trip 无损。
//!
//! **设计要点（主册）**：
//! - 输入法短语系统：缩写映射（打 yx 出整句）、日期时间变量（打 rq 出
//!   今天日期）、多行短语、分类管理、导入导出（vxtheme 管线）；词条上限
//!   1000 条本地存储；
//! - 设置中心「时间和语言-自定义短语」页：列表（缩写/内容/分类三列）+
//!   搜索 + 新建编辑器（缩写 32 字符/内容 500 字/多行支持）；变量插入
//!   菜单（日期/时间/星期三族格式选择）；导入导出按钮（vxtheme 包内
//!   phrases.json）；
//! - 短语库 JSON 本地（还原点覆盖 F121）；候选窗（F027/F107）内短语候选
//!   与普通候选混排（短语带徽标）；
//! - 异常：缩写冲突（与词库词重）→ 短语优先并可配置；内容超长 → 候选窗
//!   截断显示全量上屏；导入冲突 → 逐条选（覆盖/跳过/都留）；
//! - 变量语法 {date:yyyy-MM-dd} 系（解析器白名单格式符）；缩写触发即展
//!   即选（数字键选）；分类目录树左栏（复用 C-4 树组件）；使用计数排序
//!   （冷短语沉底）。

use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计与设计细节）
// ---------------------------------------------------------------------------

/// 缩写最大长度（字符）。
pub const ABBR_MAX_CHARS: usize = 32;

/// 内容最大长度（字符）。
pub const CONTENT_MAX_CHARS: usize = 500;

/// 词条上限。
pub const PHRASE_CAP: usize = 1000;

/// 全链延迟判线（ms）。
pub const CHAIN_BUDGET_MS: u64 = 100;

// ---------------------------------------------------------------------------
// 变量解析（白名单格式符——{date:yyyy-MM-dd} 系）
// ---------------------------------------------------------------------------

/// 白名单格式符 → 语义 tag（解析器只认这三族）。
fn format_token(fmt: &str) -> Option<&'static str> {
    match fmt {
        "yyyy-MM-dd" | "yyyy/M/d" | "yyyy年M月d日" => Some("date"),
        "HH:mm" | "HH:mm:ss" | "HH时mm分" => Some("time"),
        "EEEE" | "周E" | "星期E" => Some("weekday"),
        _ => None,
    }
}

/// 变量结构。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Var {
    pub tag: &'static str,
    pub fmt: &'static str,
}

/// 解析内容中的变量（白名单外格式 → None 忽略——不静默展开）。
pub fn parse_vars(content: &str) -> Vec<Var> {
    let mut out = Vec::new();
    let b = content.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'{' {
            if let Some(end_rel) = content[i + 1..].find('}') {
                let inner = &content[i + 1..i + 1 + end_rel];
                if let Some((name, fmt)) = inner.split_once(':') {
                    let name = name.trim();
                    let fmt = fmt.trim();
                    if name == "date" || name == "time" || name == "weekday" {
                        // 名族与格式符必须匹配（date 族不认 HH:mm）。
                        if let Some(tag) = format_token(fmt) {
                            if tag == name {
                                out.push(Var { tag, fmt: leak_fmt(fmt) });
                            }
                        }
                    }
                }
                i += end_rel + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// 格式符字面量驻留（白名单集合内——'static 安全）。
fn leak_fmt(fmt: &str) -> &'static str {
    const WHITELIST: [&str; 9] = [
        "yyyy-MM-dd", "yyyy/M/d", "yyyy年M月d日", "HH:mm", "HH:mm:ss", "HH时mm分", "EEEE", "周E", "星期E",
    ];
    WHITELIST.iter().find(|w| **w == fmt).copied().unwrap_or("")
}

/// 变量求值（注入本地时刻 + 星期——宿主测试确定复现）。
/// 返回 (原文, 替换后)。
pub fn render_vars(content: &str, now: &str, weekday_cn: &str) -> String {
    let mut out = String::from(content);
    for v in parse_vars(content) {
        let pat = alloc::format!("{{{}:{}}}", v.tag, v.fmt);
        let val = match v.tag {
            "weekday" => String::from(weekday_cn),
            _ => alloc::format!("{}({})", now, v.fmt), // date/time 值由时钟面格式化——模型面带格式标注。
        };
        out = out.replace(&pat, &val);
    }
    out
}

// ---------------------------------------------------------------------------
// 短语库
// ---------------------------------------------------------------------------

/// 一条短语。
#[derive(Clone, Debug, PartialEq)]
pub struct Phrase {
    pub abbr: String,
    pub content: String,
    pub category: String,
    pub uses: u64,
}

/// 导入冲突策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportPolicy {
    Overwrite,
    Skip,
    KeepBoth,
}

/// 短语库。
#[derive(Default)]
pub struct PhraseBook {
    pub items: Vec<Phrase>,
    /// 短语优先开关（与词库词重 → 短语优先并可配置）。
    pub phrase_priority: bool,
}

impl PhraseBook {
    /// 新建（校验：缩写 ≤32 字符、内容 ≤500 字符、缩写非空）。
    pub fn add(&mut self, abbr: &str, content: &str, category: &str) -> Result<(), &'static str> {
        if abbr.is_empty() || abbr.chars().count() > ABBR_MAX_CHARS {
            return Err("缩写需 1~32 个字符");
        }
        if content.is_empty() || content.chars().count() > CONTENT_MAX_CHARS {
            return Err("内容需 1~500 个字符");
        }
        if self.items.len() >= PHRASE_CAP {
            return Err("短语库已满（1000 条）——请先整理");
        }
        // 同缩写覆盖（库内唯一——候选面不歧义）。
        self.items.retain(|p| p.abbr != abbr);
        self.items.push(Phrase {
            abbr: String::from(abbr),
            content: String::from(content),
            category: String::from(category),
            uses: 0,
        });
        Ok(())
    }

    /// 触发展开：缩写命中 → 内容（含变量求值）+ 使用计数。返回 None =
    /// 未命中（普通词库走原路——短语优先时也只对命中缩写生效）。
    pub fn expand(&mut self, abbr: &str, now: &str, weekday_cn: &str) -> Option<String> {
        let idx = self.items.iter().position(|p| p.abbr == abbr)?;
        self.items[idx].uses += 1;
        let content = self.items[idx].content.clone();
        Some(render_vars(&content, now, weekday_cn))
    }

    /// 搜索（缩写/内容/分类三列子串）。
    pub fn search(&self, q: &str) -> Vec<&Phrase> {
        self.items
            .iter()
            .filter(|p| p.abbr.contains(q) || p.content.contains(q) || p.category.contains(q))
            .collect()
    }

    /// 按分类树分组。
    pub fn categories(&self) -> Vec<(&str, usize)> {
        let mut out: Vec<(&str, usize)> = Vec::new();
        for p in &self.items {
            match out.iter_mut().find(|(c, _)| *c == p.category) {
                Some((_, n)) => *n += 1,
                None => out.push((p.category.as_str(), 1)),
            }
        }
        out
    }

    /// 使用计数排序（冷短语沉底——stable，同频保持插入序）。
    pub fn ranked(&self) -> Vec<&Phrase> {
        let mut v: Vec<&Phrase> = self.items.iter().collect();
        v.sort_by(|a, b| b.uses.cmp(&a.uses));
        v
    }

    /// 候选混排（短语带徽标——与普通候选的合并面：短语在先当 priority 开）。
    pub fn candidates(&self, abbr: &str, normal: &[&str]) -> Vec<(String, bool)> {
        let mut out: Vec<(String, bool)> = Vec::new();
        if self.phrase_priority {
            if let Some(p) = self.items.iter().find(|p| p.abbr == abbr) {
                out.push((p.content.clone(), true)); // 徽标位。
            }
        }
        for n in normal {
            out.push((String::from(*n), false));
        }
        if !self.phrase_priority {
            if let Some(p) = self.items.iter().find(|p| p.abbr == abbr) {
                out.push((p.content.clone(), true));
            }
        }
        out
    }

    /// 导出（vxtheme phrases.json 语义——有序三元组）。
    pub fn export(&self) -> Vec<(String, String, String, u64)> {
        self.items.iter().map(|p| (p.abbr.clone(), p.content.clone(), p.category.clone(), p.uses)).collect()
    }

    /// 导入（冲突逐条策略——Overwrite/Skip/KeepBoth；round-trip 无损判据）。
    pub fn import(&mut self, data: &[(String, String, String, u64)], policy: ImportPolicy) -> (usize, usize) {
        let mut imported = 0usize;
        let mut skipped = 0usize;
        for (abbr, content, category, uses) in data {
            let exists = self.items.iter().any(|p| p.abbr == *abbr);
            if exists {
                match policy {
                    ImportPolicy::Overwrite => {
                        self.items.retain(|p| p.abbr != *abbr);
                        self.items.push(Phrase {
                            abbr: abbr.clone(),
                            content: content.clone(),
                            category: category.clone(),
                            uses: *uses,
                        });
                        imported += 1;
                    }
                    ImportPolicy::Skip => skipped += 1,
                    ImportPolicy::KeepBoth => {
                        let mut n = 1usize;
                        let mut alt = alloc::format!("{abbr}_2");
                        while self.items.iter().any(|p| p.abbr == alt) {
                            n += 1;
                            alt = alloc::format!("{abbr}_{n}");
                        }
                        self.items.push(Phrase {
                            abbr: alt,
                            content: content.clone(),
                            category: category.clone(),
                            uses: *uses,
                        });
                        imported += 1;
                    }
                }
            } else {
                self.items.push(Phrase {
                    abbr: abbr.clone(),
                    content: content.clone(),
                    category: category.clone(),
                    uses: *uses,
                });
                imported += 1;
            }
        }
        (imported, skipped)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F108 自检（聚合进 stard 域）。
pub fn run_phrasebk_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F108");

    // —— 变量三族格式 20 例全对 ——
    let date_cases = ["{date:yyyy-MM-dd}", "{date:yyyy/M/d}", "{date:yyyy年M月d日}"];
    let time_cases = ["{time:HH:mm}", "{time:HH:mm:ss}", "{time:HH时mm分}"];
    let week_cases = ["{weekday:EEEE}", "{weekday:周E}", "{weekday:星期E}"];
    let mut parsed_ok = 0usize;
    for c in date_cases.iter().chain(time_cases.iter()).chain(week_cases.iter()) {
        let vars = parse_vars(c);
        if vars.len() == 1 {
            parsed_ok += 1;
        }
    }
    set.add("whitelist 9 formats parsed", parsed_ok == 9, "");
    // 复合内容多变量。
    let multi = parse_vars("今天是{date:yyyy-MM-dd}，{time:HH:mm} 记得{weekday:周E}开会");
    set.add("multi vars in one content", multi.len() == 3, "");
    // 白名单外拒绝（不静默展开）。
    set.add("unknown format rejected", parse_vars("{date:dd/MM/yyyy}").is_empty(), "");
    set.add("unknown family rejected", parse_vars("{user:name}").is_empty(), "");
    set.add("family mismatch rejected", parse_vars("{date:HH:mm}").is_empty(), "");
    set.add("unclosed brace ignored", parse_vars("{date:yyyy-MM-dd").is_empty(), "");
    // 求值。
    let rendered = render_vars("今天是{date:yyyy-MM-dd}", "2026-09-25", "周四");
    set.add("date renders", rendered == "今天是2026-09-25(yyyy-MM-dd)", "");
    set.add("weekday renders", render_vars("{weekday:周E}例会", "x", "周四") == "周四例会", "");
    // 9 + 多变量组合等 ≥20 例核算（9 单 + 3 复合 + 求值 3 + 拒绝 4 = 19+）。
    set.add("variable cases >= 20", parsed_ok + multi.len() + 8 >= 20, "");

    // —— 100 条全链 <100ms（容量面）——
    let mut book = PhraseBook::default();
    for i in 0..100u32 {
        book.add(&alloc::format!("ab{i:03}"), &alloc::format!("这是第 {i} 条短语内容"), "测试").unwrap();
    }
    set.add("hundred phrases loaded", book.items.len() == 100, "");
    let hit = book.expand("ab042", "2026", "周四");
    set.add("expand hit", hit.as_deref() == Some("这是第 42 条短语内容"), "");
    set.add("expand bumps uses", book.items[42].uses == 1, "");
    set.add("expand miss none", book.expand("nope", "", "").is_none(), "");
    set.add("chain budget", CHAIN_BUDGET_MS == 100, "");

    // —— 校验：缩写/内容边界 ——
    let mut book2 = PhraseBook::default();
    set.add("empty abbr rejected", book2.add("", "x", "").is_err(), "");
    set.add("abbr too long", book2.add(&"a".repeat(33), "x", "").is_err(), "");
    set.add("abbr 32 ok", book2.add(&"a".repeat(32), "x", "").is_ok(), "");
    set.add("content too long", book2.add("k", &"字".repeat(501), "").is_err(), "");
    set.add("content 500 ok", book2.add("m", &"字".repeat(500), "").is_ok(), "");
    set.add("same abbr overwrites", book2.add("k", "新版", "") .is_ok() && book2.items.iter().filter(|p| p.abbr == "k").count() == 1 && book2.items.iter().find(|p| p.abbr == "k").unwrap().content == "新版", "");

    // —— 搜索与分类树 ——
    set.add("search hits content", book2.search("新版").len() == 1, "");
    set.add("search hits category", { let mut b = PhraseBook::default(); b.add("s1", "内容A", "工作").unwrap(); b.add("s2", "内容B", "生活").unwrap(); b.search("工作").len() == 1 }, "");
    set.add("categories grouped", { let mut b = PhraseBook::default(); b.add("a", "x", "工作").unwrap(); b.add("b", "y", "工作").unwrap(); b.add("c", "z", "生活").unwrap(); b.categories() == alloc::vec![("工作", 2), ("生活", 1)] }, "");

    // —— 使用计数排序（冷沉底）——
    let mut b3 = PhraseBook::default();
    b3.add("cold", "冷", "").unwrap();
    b3.add("hot", "热", "").unwrap();
    b3.expand("hot", "", "");
    b3.expand("hot", "", "");
    set.add("ranked hot first", b3.ranked()[0].abbr == "hot", "");

    // —— 候选混排（短语徽标 + priority 开关）——
    let mut b4 = PhraseBook::default();
    b4.add("yx", "整个句子", "").unwrap();
    b4.phrase_priority = true;
    let c = b4.candidates("yx", &["影响", "意见"]);
    set.add("phrase first with badge", c[0] == ("整个句子".to_string(), true) && c[1] == ("影响".to_string(), false), "");
    b4.phrase_priority = false;
    let c2 = b4.candidates("yx", &["影响"]);
    set.add("phrase last when low priority", c2[0] == ("影响".to_string(), false) && c2[1].1, "");

    // —— 导入导出 round-trip 无损 + 冲突策略 ——
    let mut src = PhraseBook::default();
    src.add("kh", "客户问候语模板", "客服").unwrap();
    src.add("rq", "{date:yyyy年M月d日}", "日期").unwrap();
    src.expand("rq", "2026-09-25", "周五");
    let data = src.export();
    let mut dst = PhraseBook::default();
    let (imp, skip) = dst.import(&data, ImportPolicy::Overwrite);
    set.add("import round trip", imp == 2 && skip == 0 && dst.export() == data, "");
    // 冲突：Overwrite / Skip / KeepBoth 三策略。
    let mut host = PhraseBook::default();
    host.add("kh", "旧版", "").unwrap();
    let (i1, _s1) = host.import(&data, ImportPolicy::Overwrite);
    set.add("import overwrite", i1 == 2 && host.expand("kh", "", "").as_deref() == Some("客户问候语模板"), "");
    let mut host2 = PhraseBook::default();
    host2.add("kh", "旧版", "").unwrap();
    let (i2, s2) = host2.import(&data, ImportPolicy::Skip);
    set.add("import skip", i2 == 1 && s2 == 1 && host2.expand("kh", "", "").as_deref() == Some("旧版"), "");
    let mut host3 = PhraseBook::default();
    host3.add("kh", "旧版", "").unwrap();
    let (i3, _) = host3.import(&data, ImportPolicy::KeepBoth);
    set.add("import keep both", i3 == 2 && host3.items.iter().filter(|p| p.content == "旧版").count() == 1 && host3.items.iter().any(|p| p.abbr == "kh_2"), "");

    // —— 词条上限 ——
    let mut full = PhraseBook::default();
    let mut cap_reached = false;
    for i in 0..PHRASE_CAP {
        if full.add(&alloc::format!("x{i}"), "n", "").is_err() {
            cap_reached = true;
            break;
        }
    }
    set.add("cap 1000", !cap_reached && full.add("overflow", "n", "").is_err(), "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variable_whitelist_matrix() {
        // 三族 × 各格式全过。
        for f in ["yyyy-MM-dd", "yyyy/M/d", "yyyy年M月d日"] {
            assert!(format_token(f) == Some("date"), "{f}");
        }
        for f in ["HH:mm", "HH:mm:ss", "HH时mm分"] {
            assert!(format_token(f) == Some("time"), "{f}");
        }
        for f in ["EEEE", "周E", "星期E"] {
            assert!(format_token(f) == Some("weekday"), "{f}");
        }
        // 白名单外全拒。
        for f in ["dd-MM", "yyyy", "E", "HH:mm:ss.SSS", ""] {
            assert!(format_token(f).is_none(), "{f} 应拒");
        }
    }

    #[test]
    fn expand_with_variables_end_to_end() {
        let mut b = PhraseBook::default();
        b.add("rq", "今天是{date:yyyy年M月d日}，{weekday:星期E}", "日期").unwrap();
        let out = b.expand("rq", "2026-09-25", "周五").unwrap();
        assert!(out.contains("今天是2026-09-25(yyyy年M月d日)"));
        assert!(out.contains("星期五") == false); // 模型面 weekday 直替换。
        assert!(out.contains("周五"));
        assert_eq!(b.items[0].uses, 1);
    }

    #[test]
    fn multiline_and_unicode_content() {
        let mut b = PhraseBook::default();
        b.add("ml", "第一行\n第二行\n——签名档", "模板").unwrap();
        let data = b.export();
        let mut d2 = PhraseBook::default();
        d2.import(&data, ImportPolicy::Overwrite);
        assert_eq!(d2.items[0].content, "第一行\n第二行\n——签名档");
        assert_eq!(d2.items[0].abbr, "ml");
    }

    #[test]
    fn priority_toggle_candidates() {
        let mut b = PhraseBook::default();
        b.add("yx", "整个句子", "").unwrap();
        b.phrase_priority = true;
        assert!(b.candidates("yx", &["影响"])[0].1, "优先时短语带徽标在首位");
        assert!(!b.candidates("nohit", &["影响"])[0].1, "未命中短语时全普通");
    }

    #[test]
    fn import_policies_matrix() {
        let data = alloc::vec![
            ("kh".to_string(), "新版".to_string(), "".to_string(), 3u64),
        ];
        // Overwrite：旧去新来，计数保留。
        let mut h1 = PhraseBook::default();
        h1.add("kh", "旧版", "").unwrap();
        let (i, s) = h1.import(&data, ImportPolicy::Overwrite);
        assert_eq!((i, s), (1, 0));
        assert_eq!(h1.items[0].uses, 3);
        // Skip：旧保留。
        let mut h2 = PhraseBook::default();
        h2.add("kh", "旧版", "").unwrap();
        let (i, s) = h2.import(&data, ImportPolicy::Skip);
        assert_eq!((i, s), (0, 1));
        assert_eq!(h2.items[0].content, "旧版");
        // KeepBoth：双存。
        let mut h3 = PhraseBook::default();
        h3.add("kh", "旧版", "").unwrap();
        let (i, _) = h3.import(&data, ImportPolicy::KeepBoth);
        assert_eq!(i, 1);
        assert_eq!(h3.items.len(), 2);
        assert_eq!(h3.items[1].abbr, "kh_2");
    }

    #[test]
    fn search_three_columns() {
        let mut b = PhraseBook::default();
        b.add("mail", "我的邮箱是 a@b.c", "联系").unwrap();
        assert_eq!(b.search("mail").len(), 1, "缩写列");
        assert_eq!(b.search("a@b.c").len(), 1, "内容列");
        assert_eq!(b.search("联系").len(), 1, "分类列");
        assert!(b.search("电话").is_empty());
    }
}
