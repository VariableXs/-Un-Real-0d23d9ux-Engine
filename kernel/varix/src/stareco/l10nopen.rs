//! F140 本地化开放 · 完整设计（STAR I 主册 G-D-15）。
//!
//! **判据（主册）**：双语切换全界面走查（30 关键页零漏翻零硬编码
//! ——B-1104 纪律的语言面）；翻译包导入导出 round-trip 无损。
//!
//! **设计要点（主册）**：全 UI 文案抽离字符串表（JSON）；词条 key
//! 命名规范（域.页面.用途三级）；上下文注释强制；中文源过禁模糊词
//! 表、译文过等义审查；热替换无重启；覆盖度 <90% 标「测试版语言包」
//! （ebase 覆盖度口径）；缺词条 → 回退英文 + 控制台日志清单（按单
//! 补翻）；占位符错乱（翻译丢 {n}）→ 运行时校验兜底回退；数字/日期
//! 格式随区域。
//!
//! 本模块是字符串表的**纯逻辑核**：词条 schema 校验、语言包装载与
//! 覆盖度、查找回退链（目标语言→英文→键名）、占位符完整性校验、
//! 导入导出 round-trip 无损、漏翻清单（控制台日志面）。

use alloc::vec;
use crate::checks::CheckSet;
use crate::stareco::ebase::{coverage_is_beta, fnv1a64};

// ---------------------------------------------------------------------------
// 词条 schema（域.页面.用途三级 key）
// ---------------------------------------------------------------------------

/// key 校验：三段以 `.` 分隔，每段 1..=24 字节小写字母/数字/下划线。
pub fn key_ok(key: &str) -> bool {
    let parts: alloc::vec::Vec<&str> = key.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts.iter().all(|p| {
        let b = p.as_bytes();
        !b.is_empty()
            && b.len() <= 24
            && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
    })
}

/// key 解析：返回（域，页面，用途）。
pub fn key_parts(key: &str) -> Option<(&str, &str, &str)> {
    if !key_ok(key) {
        return None;
    }
    let mut it = key.split('.');
    Some((it.next()?, it.next()?, it.next()?))
}

/// 上下文注释强制：注释须标注用途类别（按钮/标题/说明/错误…四类起）。
pub const CONTEXT_KINDS: [&str; 4] = ["button", "title", "desc", "error"];

pub fn context_ok(context: &str) -> bool {
    CONTEXT_KINDS.iter().any(|k| context.starts_with(k))
}

/// 占位符提取：`{name}` 形态。
pub fn placeholders(text: &str) -> alloc::vec::Vec<&str> {
    let mut out = alloc::vec::Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = text[i + 1..].find('}') {
                out.push(&text[i + 1..i + 1 + end]);
                i += end + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// 占位符完整性：译文占位符集合 == 源文占位符集合（防翻译丢 {n}）。
pub fn placeholders_match(source: &str, translated: &str) -> bool {
    let a = placeholders(source);
    let b = placeholders(translated);
    a.len() == b.len()
        && a.iter().all(|x| b.contains(x))
}

// ---------------------------------------------------------------------------
// 语言包
// ---------------------------------------------------------------------------

/// 一条词条。
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub key: &'static str,
    pub value: &'static str,
    /// 上下文注释（用途标注）。
    pub context: &'static str,
}

/// 一个语言包（一种语言的全量词条）。
pub struct LangPack {
    pub lang: &'static str,
    entries: [Option<Entry>; 32],
    count: usize,
}

impl LangPack {
    pub fn new(lang: &'static str) -> LangPack {
        LangPack { lang, entries: [None; 32], count: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 装载词条：key 三级规范 + 注释用途标注缺一不可（schema 门禁）。
    pub fn put(&mut self, e: Entry) -> Result<(), &'static str> {
        if !key_ok(e.key) {
            return Err("key must be domain.page.usage");
        }
        if !context_ok(e.context) {
            return Err("context annotation required (button/title/desc/error)");
        }
        if self.count >= 32 {
            return Err("pack full");
        }
        self.entries[self.count] = Some(e);
        self.count += 1;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&'static str> {
        self.entries[..self.count].iter().flatten().find(|e| e.key == key).map(|e| e.value)
    }

    /// 全部词条键（round-trip 与漏翻清单用）。
    pub fn keys(&self) -> alloc::vec::Vec<&'static str> {
        self.entries[..self.count].iter().flatten().map(|e| e.key).collect()
    }

    /// 对中文源包的禁模糊词审查（中文源面：文案不许含模糊词）。
    pub fn vague_words_present(&self, banned: &[&str]) -> bool {
        self.entries[..self.count].iter().flatten().any(|e| banned.iter().any(|b| e.value.contains(b)))
    }
}

// ---------------------------------------------------------------------------
// 本地化引擎
// ---------------------------------------------------------------------------

pub struct L10n {
    /// 英文基线包（回退锚）。
    pub en: LangPack,
    /// 目标语言包。
    pub target: LangPack,
    /// 漏翻清单（控制台日志面——社区按单补翻）。
    missing: [Option<&'static str>; 32],
    missing_count: usize,
}

impl L10n {
    pub fn new(en: LangPack, target: LangPack) -> L10n {
        L10n { en, target, missing: [None; 32], missing_count: 0 }
    }

    /// 覆盖度：目标包词条数 / 基线包词条数（ebase 千分比口径）。
    pub fn coverage_ppt(&self) -> u32 {
        if self.en.len() == 0 {
            return 0;
        }
        ((self.target.keys().iter().filter(|k| self.en.get(k).is_some()).count() as u64 * 1000)
            / self.en.len() as u64) as u32
    }

    /// 覆盖度 <90% → 「测试版语言包」标注。
    pub fn is_beta_pack(&self) -> bool {
        coverage_is_beta(self.target.keys().iter().filter(|k| self.en.get(k).is_some()).count(), self.en.len())
    }

    /// 查找回退链：目标语言 → 英文 → 键名回显（绝不静默空白）。
    /// 每次英文回退记入漏翻清单。
    pub fn lookup(&mut self, key: &'static str) -> &'static str {
        if let Some(v) = self.target.get(key) {
            return v;
        }
        if let Some(v) = self.en.get(key) {
            if self.missing_count < 32 {
                self.missing[self.missing_count] = Some(key);
                self.missing_count += 1;
            }
            return v;
        }
        key
    }

    /// 漏翻清单（按单补翻的数据源）。
    pub fn missing_list(&self) -> &[Option<&'static str>] {
        &self.missing[..self.missing_count]
    }

    /// 占位符校验兜底：目标译文占位符与源不符 → **跳过目标包**直接
    /// 回退英文（并记漏翻）——不能借道 lookup() 把坏译文再捞回来。
    pub fn lookup_checked(&mut self, key: &'static str, source_template: &'static str) -> &'static str {
        if let Some(v) = self.target.get(key) {
            if placeholders_match(source_template, v) {
                return v;
            }
        }
        if let Some(v) = self.en.get(key) {
            if self.missing_count < 32 {
                self.missing[self.missing_count] = Some(key);
                self.missing_count += 1;
            }
            return v;
        }
        key
    }
}

/// 导入导出 round-trip 无损：导出条目序列（key=value@context）重装后
/// 键值对逐条等值。
pub fn round_trip(src: &LangPack, reimport: &LangPack) -> bool {
    if src.len() != reimport.len() {
        return false;
    }
    src.keys().iter().all(|k| {
        src.get(k) == reimport.get(k) && fnv1a64(src.get(k).unwrap_or("").as_bytes()) == fnv1a64(reimport.get(k).unwrap_or("").as_bytes())
    })
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F140_TAG: &str = "stareco-F140-l10n";

pub fn run_l10nopen_checks() -> CheckSet {
    let mut set = CheckSet::new(F140_TAG);

    // key schema
    set.add("f140 key three-level", key_ok("desktop.start.button"), "domain.page.usage");
    set.add("f140 key bad shape", !key_ok("start.button") && !key_ok("a.b.c.d") && !key_ok("A.b.c"), "law enforced");
    set.add("f140 key parts parsed", key_parts("fs.dialog.title") == Some(("fs", "dialog", "title")), "destructure");

    // 上下文注释强制
    set.add("f140 context kinds", context_ok("button") && context_ok("error:shown when..."), "annotated");
    set.add("f140 context missing", !context_ok("随便写的"), "no free-form");

    // 占位符
    set.add("f140 placeholders extract", placeholders("共 {n} 项，第 {i} 页") == vec!["n", "i"], "both found");
    set.add("f140 placeholder lost detected", !placeholders_match("共 {n} 项", "n items"), "missing {n}");
    set.add("f140 placeholder kept ok", placeholders_match("共 {n} 项", "{n} items total"), "faithful");

    // 语言包装载 schema 门禁
    let mut en = LangPack::new("en");
    assert!(en.put(Entry { key: "ui.main.open", value: "Open", context: "button" }).is_ok());
    assert!(en.put(Entry { key: "ui.main.title", value: "Files", context: "title" }).is_ok());
    assert!(en.put(Entry { key: "ui.main.count", value: "{n} items", context: "desc" }).is_ok());
    set.add("f140 en pack loaded", en.len() == 3, "baseline");
    set.add("f140 bad key rejected", en.put(Entry { key: "badkey", value: "x", context: "button" }).is_err(), "schema gate");
    set.add("f140 no-context rejected", en.put(Entry { key: "a.b.c", value: "x", context: "" }).is_err(), "annotation law");
    set.add("f140 clean source passes", !en.vague_words_present(&["大概"]), "english n/a");

    // 目标语言：覆盖 2/3 → 测试版标注 + 漏翻清单
    let mut zh = LangPack::new("zh");
    assert!(zh.put(Entry { key: "ui.main.open", value: "打开", context: "button" }).is_ok());
    assert!(zh.put(Entry { key: "ui.main.count", value: "共 {n} 项", context: "desc" }).is_ok());
    let mut l10n = L10n::new(en, zh);
    set.add("f140 coverage 2/3 beta", l10n.is_beta_pack() && l10n.coverage_ppt() == 666, "<90% test pack");
    set.add("f140 hit zh", l10n.lookup("ui.main.open") == "打开", "target first");
    set.add("f140 miss falls back en", l10n.lookup("ui.main.title") == "Files", "fallback chain");
    set.add("f140 missing logged", l10n.missing_list().iter().any(|o| *o == Some("ui.main.title")), "console list for translators");
    set.add("f140 unknown key echoes key", l10n.lookup("x.y.z") == "x.y.z", "never silent blank");

    // 占位符错乱兜底：译文丢 {n} → 回退英文（en 同 key 基线做回退）
    let mut en2 = LangPack::new("en");
    assert!(en2.put(Entry { key: "ui.main.count", value: "{n} items", context: "desc" }).is_ok());
    let mut zh_bad2 = LangPack::new("zh");
    assert!(zh_bad2.put(Entry { key: "ui.main.count", value: "共若干项", context: "desc" }).is_ok());
    let mut eng_bad = L10n::new(en2, zh_bad2);
    set.add(
        "f140 placeholder broken falls back",
        eng_bad.lookup_checked("ui.main.count", "{n} items") == "{n} items",
        "runtime guard",
    );

    // 禁模糊词审查（中文源面）
    let mut vague = LangPack::new("zh");
    assert!(vague.put(Entry { key: "a.b.c", value: "可能大概也许", context: "desc" }).is_ok());
    set.add("f140 vague word caught", vague.vague_words_present(&["大概", "也许", "应该是"]), "banned list");

    // round-trip 无损
    let mut reimport = LangPack::new("zh");
    for k in l10n.target.keys() {
        let v = l10n.target.get(k).unwrap_or("");
        let ctx = l10n.target.entries.iter().flatten().find(|e| e.key == k).map(|e| e.context).unwrap_or("desc");
        assert!(reimport.put(Entry { key: k, value: v, context: ctx }).is_ok());
    }
    set.add("f140 round-trip lossless", round_trip(&l10n.target, &reimport), "export→import equal");
    let mut truncated = LangPack::new("zh");
    assert!(truncated.put(Entry { key: "ui.main.open", value: "打开", context: "button" }).is_ok());
    set.add("f140 round-trip detects loss", !round_trip(&l10n.target, &truncated), "loss caught");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_edge_cases() {
        assert!(placeholders("").is_empty());
        assert!(placeholders("{unclosed").is_empty());
        assert_eq!(placeholders("{a}{b}"), vec!["a", "b"]);
        assert!(placeholders_match("{a}{b}", "{b}{a}"));
    }

    #[test]
    fn key_law() {
        assert!(key_ok("a1.b2.c3"));
        assert!(!key_ok("a..b.c"));
        assert!(!key_ok("a.b.very_long_segment_over_24_chars_xxxx"));
    }
}
