//! 深化层四 · F140 本地化开放（2026-09-27 深化批次四 · g 层）。
//!
//! 翻译记忆库（编辑距离模糊匹配建议）、MT 预填诚实标记、语言×域
//! 覆盖地图与发布门、字符集覆盖检查、复数规则表 v2（扩语种）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 翻译记忆库：源句指纹 → 译文；模糊匹配用有界 Levenshtein（≤16 字节差）
// ---------------------------------------------------------------------------

/// 有界编辑距离：超过 max 即早退返回 max+1（性能护栏，n 小无压力）。
pub fn edit_distance_bounded(a: &str, b: &str, max: usize) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len().abs_diff(b.len()) > max {
        return max + 1;
    }
    let mut prev: alloc::vec::Vec<usize> = (0..=b.len()).collect();
    let mut cur: alloc::vec::Vec<usize> = alloc::vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        core::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

pub struct TranslationMemory {
    /// (源句, 译文)
    entries: alloc::vec::Vec<(&'static str, &'static str)>,
}

impl TranslationMemory {
    pub fn new() -> TranslationMemory {
        TranslationMemory { entries: alloc::vec::Vec::new() }
    }

    pub fn remember(&mut self, src: &'static str, dst: &'static str) {
        self.entries.push((src, dst));
    }

    /// 精确命中。
    pub fn exact(&self, src: &str) -> Option<&'static str> {
        self.entries.iter().find(|(s, _)| *s == src).map(|(_, d)| *d)
    }

    /// 模糊建议：编辑距离 ≤ 源长 20% 的最佳候选（零命中如实 None）。
    pub fn fuzzy(&self, src: &str) -> Option<(&'static str, usize)> {
        let max = src.len() * 20 / 100;
        let mut best: Option<(&'static str, usize)> = None;
        for (s, d) in &self.entries {
            let dist = edit_distance_bounded(src, s, max.max(1));
            if dist <= max {
                match best {
                    Some((_, bd)) if dist >= bd => {}
                    _ => best = Some((d, dist)),
                }
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------
// MT 预填诚实标记：机器翻译来源必须带标记——无标记的 MT 条目点名
// ---------------------------------------------------------------------------

pub struct MtLedger {
    /// MT 预填过的 key 清单。
    mt_keys: alloc::vec::Vec<&'static str>,
}

impl MtLedger {
    pub fn new() -> MtLedger {
        MtLedger { mt_keys: alloc::vec::Vec::new() }
    }

    pub fn prefill(&mut self, key: &'static str) {
        self.mt_keys.push(key);
    }

    /// 审计：MT 来源但标记为人工（honest=false）的 key。
    pub fn unmarked(&self, marked_human: &[&'static str]) -> alloc::vec::Vec<&'static str> {
        self.mt_keys
            .iter()
            .filter(|k| !marked_human.iter().any(|m| m == *k))
            .copied()
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 语言×域覆盖地图：千分比 + 发布门（启用语言全域 ≥800‰）
// ---------------------------------------------------------------------------

pub struct CoverageCell {
    pub lang: &'static str,
    pub domain: u8,
    pub per_mille: u32,
}

pub fn release_gate(cells: &[CoverageCell], enabled: &[&'static str], threshold: u32) -> alloc::vec::Vec<&'static str> {
    let mut below: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    for lang in enabled {
        let doms: alloc::vec::Vec<u32> =
            cells.iter().filter(|c| c.lang == *lang).map(|c| c.per_mille).collect();
        if doms.is_empty() || doms.iter().any(|p| *p < threshold) {
            below.push(lang);
        }
    }
    below
}

// ---------------------------------------------------------------------------
// 字符集覆盖：译文用字 ⊆ 字体覆盖集（缺字检出——渲染豆腐块的预防面）
// ---------------------------------------------------------------------------

/// chars 为译文去重后的字符字节（ASCII 口径示例；中文按 UTF-8 首字节
/// 粗粒度归组——教学示例用，内核真实面走字距表）。
pub fn charset_missing(text: &str, font_covers: &[(u8, u8)]) -> alloc::vec::Vec<u8> {
    let mut missing: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    for b in text.bytes() {
        if !font_covers.iter().any(|(lo, hi)| b >= *lo && b <= *hi) && !missing.contains(&b) {
            missing.push(b);
        }
    }
    missing
}

// ---------------------------------------------------------------------------
// 复数规则表 v2：扩语种（zh1/en2/ru3/ar6）
// ---------------------------------------------------------------------------

pub fn plural_forms_v2(lang: &str) -> u8 {
    match lang {
        "zh" => 1,
        "en" => 2,
        "ru" => 3,
        "ar" => 6,
        _ => 2,
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F140G_TAG: &str = "stareco-F140-deep4";

pub fn run_f140_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F140G_TAG);

    // 翻译记忆库
    let mut tm = TranslationMemory::new();
    tm.remember("Open file", "打开文件");
    tm.remember("Open folder", "打开文件夹");
    set.add("f140g exact", tm.exact("Open file") == Some("打开文件"), "精确命中");
    set.add("f140g exact miss", tm.exact("Open dir").is_none(), "未命中如实");
    let sug = tm.fuzzy("Open fille"); // 距 1 ≤ 20%×9=1
    set.add("f140g fuzzy hit", sug.map(|(d, _)| d) == Some("打开文件"), "模糊建议最佳候选");
    set.add("f140g fuzzy none", tm.fuzzy("Completely different text!").is_none(), "超界无建议");

    // MT 标记
    let mut mt = MtLedger::new();
    mt.prefill("ui.open");
    mt.prefill("ui.save");
    set.add(
        "f140g unmarked",
        mt.unmarked(&["ui.open"]) == alloc::vec!["ui.save"],
        "未标记 MT 点名",
    );
    set.add("f140g all marked", mt.unmarked(&["ui.open", "ui.save"]).is_empty(), "全标记通过");

    // 覆盖地图
    let cells = [
        CoverageCell { lang: "zh", domain: 1, per_mille: 1000 },
        CoverageCell { lang: "zh", domain: 2, per_mille: 950 },
        CoverageCell { lang: "ko", domain: 1, per_mille: 600 },
    ];
    let gate = release_gate(&cells, &["zh", "ko"], 800);
    set.add("f140g gate", gate == alloc::vec!["ko"], "低覆盖语言点名");

    // 字符集
    let font = [(b'a', b'z'), (b'0', b'9')];
    let miss = charset_missing("ab1éz", &font);
    set.add("f140g charset", miss == alloc::vec![0xC3, 0xA9], "缺字检出（UTF-8 字节）");

    // 复数表 v2
    set.add(
        "f140g plural v2",
        plural_forms_v2("zh") == 1 && plural_forms_v2("ar") == 6 && plural_forms_v2("xx") == 2,
        "扩语种与保守缺省",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn edit_distance_basics() {
        assert_eq!(edit_distance_bounded("kitten", "sitting", 10), 3);
        assert_eq!(edit_distance_bounded("abc", "abc", 10), 0);
        // 早退：长度差超 max 直接 max+1，不做全表。
        assert_eq!(edit_distance_bounded("a", "abcdefgh", 2), 3);
    }

    #[test]
    fn fuzzy_prefers_closer() {
        let mut tm = TranslationMemory::new();
        tm.remember("Save now", "立即保存");
        tm.remember("Safe now", "安全保存");
        // "Save noy" 距两者各 1——平局取先登记（确定性口径）。
        let (d, dist) = tm.fuzzy("Save noy").unwrap();
        assert_eq!(d, "立即保存");
        assert_eq!(dist, 1);
    }
}
