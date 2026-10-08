//! 深化层 · F587 保存的搜索（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F587 节）：
//! ① 条件文本→胶囊→条件文本的 **round-trip 引擎**（可视化可往返编辑）；
//! ② **新鲜度重跑账**——同签名重跑、索引纪元变命中数可变（活查询不缓存）；
//! ③ 侧栏 ≤10 条的 **LRU 淘汰**——满额保存最久未用先出；
//! ④ 与 F306 联动的**条件编译**——胶囊→索引查询谓词。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::savedsearch::{Capsule, CapsuleKind, SavedSearches, SAVED_CAP};

use alloc::string::String;
use alloc::vec::Vec;

// --- ① 胶囊 round-trip 引擎 ------------------------------------------------

/// 胶囊类型 → 文本记号（可视化拼写唯一源）。
fn kind_token(k: CapsuleKind) -> &'static str {
    match k {
        CapsuleKind::Kind => "类型",
        CapsuleKind::Modified => "修改",
        CapsuleKind::Location => "位置",
    }
}

/// 文本记号 → 胶囊类型（未知记号诚实 None）。
fn kind_from_token(t: &str) -> Option<CapsuleKind> {
    if t == "类型" { Some(CapsuleKind::Kind) }
    else if t == "修改" { Some(CapsuleKind::Modified) }
    else if t == "位置" { Some(CapsuleKind::Location) }
    else { None }
}

/// 胶囊序列 → 条件文本（「类型:文档 修改:近7天」形制）。
pub fn capsules_to_text(caps: &[Capsule]) -> String {
    let mut s = String::new();
    for (i, c) in caps.iter().enumerate() {
        if i > 0 { s.push(' '); }
        s.push_str(kind_token(c.kind));
        s.push(':');
        s.push_str(&c.value);
    }
    s
}

/// 条件文本 → 胶囊序列（round-trip 反向；坏记号/空值/空串诚实 None）。
pub fn text_to_capsules(text: &str) -> Option<Vec<Capsule>> {
    let mut out = Vec::new();
    for piece in text.split(' ') {
        let mut it = piece.splitn(2, ':');
        let kind = kind_from_token(it.next()?)?;
        let value = it.next()?;
        if value.is_empty() { return None; }
        out.push(Capsule { kind, value: String::from(value) });
    }
    if out.is_empty() { None } else { Some(out) }
}

// --- ④ 条件编译（F306 联动） -----------------------------------------------

/// 索引文档样本（F306 联动桩：类型 + 修改年龄天）。
pub struct IdxDoc {
    pub kind: &'static str,
    pub age_days: u32,
}

/// 编译后的索引查询谓词。
pub struct CompiledQuery {
    want_kind: Option<String>,
    max_age_days: Option<u32>,
}

/// 「近N天」→ N（无数字诚实 None——「自定义」等不可编译值）。
fn parse_recent_days(v: &str) -> Option<u32> {
    let digits: String = v.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() { None } else { digits.parse::<u32>().ok() }
}

/// 胶囊 → 索引谓词（Modified 值不可编译 = 编译失败诚实 None）。
pub fn compile_query(caps: &[Capsule]) -> Option<CompiledQuery> {
    let mut q = CompiledQuery { want_kind: None, max_age_days: None };
    for c in caps {
        match c.kind {
            CapsuleKind::Kind => q.want_kind = Some(c.value.clone()),
            CapsuleKind::Modified => q.max_age_days = Some(parse_recent_days(&c.value)?),
            CapsuleKind::Location => {} // 位置谓词由宿主路径树承接——本账不编
        }
    }
    Some(q)
}

impl CompiledQuery {
    /// 谓词执行：对一份索引快照逐文档判（true=入结果）。
    pub fn matches(&self, d: &IdxDoc) -> bool {
        let kind_ok = match &self.want_kind {
            Some(k) => d.kind == k.as_str(),
            None => true,
        };
        let age_ok = match self.max_age_days {
            Some(m) => d.age_days <= m,
            None => true,
        };
        kind_ok && age_ok
    }

    /// 对索引快照求命中数（重跑账的结果面）。
    pub fn hits(&self, docs: &[IdxDoc]) -> usize {
        docs.iter().filter(|d| self.matches(d)).count()
    }
}

// --- ② 新鲜度重跑账 ---------------------------------------------------------

/// 活查询重跑账：(查询签名, 索引纪元, 命中数) 逐条记账。
pub struct FreshnessLedger {
    runs: Vec<(u64, u64, usize)>,
}

impl FreshnessLedger {
    pub fn new() -> FreshnessLedger { FreshnessLedger { runs: Vec::new() } }

    pub fn note(&mut self, sig: u64, epoch: u64, hits: usize) {
        self.runs.push((sig, epoch, hits));
    }

    /// 活查询证据：同签名存在命中数不同的两次重跑
    /// （条件没变、结果变新——不缓存结果的语义成立）。
    pub fn same_sig_diff_result(&self, sig: u64) -> bool {
        let hits: Vec<usize> =
            self.runs.iter().filter(|r| r.0 == sig).map(|r| r.2).collect();
        !hits.is_empty() && hits.iter().any(|h| *h != hits[0])
    }
}

impl Default for FreshnessLedger {
    fn default() -> Self {
        Self::new()
    }
}

// --- ③ LRU 上限淘汰 ---------------------------------------------------------

/// 侧栏 LRU 账（10 条上限 · 最久未用先出）。
pub struct LruSidebar {
    entries: Vec<(String, u64)>, // (名, 最近使用时刻)
}

impl LruSidebar {
    pub fn new() -> LruSidebar {
        LruSidebar { entries: Vec::new() }
    }

    /// 保存：存在即续期；满额淘汰最久未用后入列，返回被淘汰名。
    pub fn save(&mut self, name: &str, ms: u64) -> Option<String> {
        if let Some(e) = self.entries.iter_mut().find(|(n, _)| n == name) {
            e.1 = ms;
            return None;
        }
        let evicted = if self.entries.len() >= SAVED_CAP {
            let mut idx = 0;
            let mut best = u64::MAX;
            for (i, (_, t)) in self.entries.iter().enumerate() {
                if *t < best { best = *t; idx = i; }
            }
            Some(self.entries.remove(idx).0)
        } else { None };
        self.entries.push((String::from(name), ms));
        evicted
    }

    /// 点击重跑续期（LRU 新鲜度的来源——不点会被后浪拍死）。
    pub fn touch(&mut self, name: &str, ms: u64) -> bool {
        match self.entries.iter_mut().find(|(n, _)| n == name) {
            Some(e) => {
                e.1 = ms;
                true
            }
            None => false,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for LruSidebar {
    fn default() -> Self {
        Self::new()
    }
}

// --- 深化自检 ---------------------------------------------------------------

/// 按时序填满一栏 LRU（自检 5/6 共用的造账步骤）。
fn fill_lru(names: &[&str]) -> LruSidebar {
    let mut l = LruSidebar::new();
    for (i, n) in names.iter().enumerate() {
        let _ = l.save(n, 100 + i as u64);
    }
    l
}

pub fn run_f587_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);
    let cap = |k: CapsuleKind, v: &str| Capsule { kind: k, value: String::from(v) };

    // 1) 胶囊 round-trip：条件→胶囊→条件文本无损（可视化可往返编辑）。
    let caps = alloc::vec![
        cap(CapsuleKind::Kind, "文档"),
        cap(CapsuleKind::Modified, "近7天"),
        cap(CapsuleKind::Location, "D:\\"),
    ];
    let text = capsules_to_text(&caps);
    let back = text_to_capsules(&text);
    cs.add(
        "capsule round trip lossless",
        text == "类型:文档 修改:近7天 位置:D:\\"
            && back.map(|b| capsules_to_text(&b) == text).unwrap_or(false),
        "",
    );

    // 2) 坏记号诚实拒绝（未知类型/空值不成胶囊）。
    cs.add(
        "bad tokens rejected",
        text_to_capsules("颜色:红").is_none() && text_to_capsules("类型:").is_none(),
        "",
    );

    // 3) 条件编译：类型+修改谓词对文档样本判得准（F306 联动面）。
    let docs = [
        IdxDoc { kind: "文档", age_days: 5 },
        IdxDoc { kind: "图片", age_days: 5 },
        IdxDoc { kind: "文档", age_days: 20 },
    ];
    cs.add(
        "compiled predicate filters",
        compile_query(&caps).map(|q| q.hits(&docs) == 1).unwrap_or(false),
        "",
    );

    // 4) 活查询重跑账：同签名、索引纪元变了命中数变（不缓存结果）。
    let mut base = SavedSearches::new();
    let _ = base.save("本周改动的文档", caps.clone());
    let sig = base.run("本周改动的文档", 1_000);
    let sig_v = sig.unwrap_or(0);
    let mut fr = FreshnessLedger::new();
    let epoch_a = [IdxDoc { kind: "文档", age_days: 5 }];
    let epoch_b = [IdxDoc { kind: "文档", age_days: 5 }, IdxDoc { kind: "文档", age_days: 2 }];
    let hits_of = |docs: &[IdxDoc]| compile_query(&caps).map(|q| q.hits(docs)).unwrap_or(0);
    fr.note(sig_v, 1, hits_of(&epoch_a));
    fr.note(sig_v, 2, hits_of(&epoch_b));
    cs.add(
        "live query fresh rerun",
        sig.is_some() && fr.same_sig_diff_result(sig_v),
        "",
    );

    // 5) LRU 淘汰：满 10 条再存 → 最久未用先出、总量守恒。
    let names = ["甲", "乙", "丙", "丁", "戊", "己", "庚", "辛", "壬", "癸"];
    let mut lru = fill_lru(&names);
    let evicted = lru.save("新搜索", 999);
    cs.add(
        "lru evicts oldest",
        lru.len() == SAVED_CAP && evicted.map(|e| e == "甲").unwrap_or(false),
        "",
    );

    // 6) LRU 续期：点了老搜索 → 淘汰对象换人（最久未用口径被改写）。
    let mut lru2 = fill_lru(&names);
    let _ = lru2.touch("甲", 500);
    let evicted2 = lru2.save("新搜索", 999);
    cs.add(
        "lru touch changes victim",
        evicted2.map(|e| e == "乙").unwrap_or(false),
        "",
    );

    // 7) 重名保存不占新槽（续期而非重复入列）。
    let mut lru3 = LruSidebar::new();
    let _ = lru3.save("搜索", 100);
    let _ = lru3.save("搜索", 200);
    cs.add("same name renews not duplicates", lru3.len() == 1, "");

    // 8) 基础件复核：胶囊编辑后签名变（基础判据不被深化破坏）。
    let _ = base.edit_capsule("本周改动的文档", 1, "近30天");
    let sig_after = base.run("本周改动的文档", 2_000);
    cs.add(
        "base signature reacts to edit",
        sig_after.is_some() && sig_after != sig,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_token_none() {
        assert!(text_to_capsules("颜色:红").is_none());
        assert!(text_to_capsules("").is_none());
    }

    #[test]
    fn parse_recent_days_edges() {
        assert_eq!(parse_recent_days("近7天"), Some(7));
        assert_eq!(parse_recent_days("近30天"), Some(30));
        assert_eq!(parse_recent_days("自定义"), None);
    }

    #[test]
    fn lru_touch_unknown_false() {
        let mut l = LruSidebar::new();
        assert!(!l.touch("无", 1));
    }
}
