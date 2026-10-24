//! VE-F3997 · S 域知识库与经验归档
//!
//! 归档：S 域收官知识归档（终审模式 / 联测案例 / 走查发现 / 变更案例
//! 四类入家族知识库 + 与 F3979 自查档合并去重）。
//! 数据结构：归档包（四类）；合并去重表；检索索引。
//! 错误路径：重复条→去重合并；索引断→重建；类缺→补档。
//! 性能：归档 O(条)；去重 O(索引)；重建 O(索引)。
//! 跨批对接：F3979 上游；家族知识库；T 域可检索。
//! 无障碍与隐私：归档含五角色走查方法沉淀（域本色）；案例脱敏。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x49C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x49C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T17KCode(pub u16);

impl T17KCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const DUP_MERGED: Self = Self(SEG_BASE + 1);
    pub const INDEX_BROKEN: Self = Self(SEG_BASE + 2);
    pub const KIND_GAP: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::DUP_MERGED => "重复条→去重合并",
            Self::INDEX_BROKEN => "索引断→重建",
            Self::KIND_GAP => "类缺→补档",
            _ => "未知",
        }
    }
}

// ── 四类归档 ─────────────────────────────────────────────────────────

/// 归档四类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchKind {
    /// 终审模式。
    FinalPattern,
    /// 联测案例。
    JoinCase,
    /// 走查发现。
    WalkFinding,
    /// 变更案例。
    ChangeCase,
}

pub const ARCH_KINDS: [ArchKind; 4] = [
    ArchKind::FinalPattern, ArchKind::JoinCase, ArchKind::WalkFinding, ArchKind::ChangeCase,
];

impl ArchKind {
    pub fn name(self) -> &'static str {
        match self {
            ArchKind::FinalPattern => "终审模式",
            ArchKind::JoinCase => "联测案例",
            ArchKind::WalkFinding => "走查发现",
            ArchKind::ChangeCase => "变更案例",
        }
    }

    pub fn slot(self) -> usize {
        match self {
            ArchKind::FinalPattern => 0,
            ArchKind::JoinCase => 1,
            ArchKind::WalkFinding => 2,
            ArchKind::ChangeCase => 3,
        }
    }
}

/// 知识条目：指纹 + 类 + 关键词 + 脱敏标记。
#[derive(Debug, Clone)]
pub struct KnowledgeItem {
    /// 内容指纹（FNV-1a）。
    pub fp: u64,
    pub kind: ArchKind,
    /// 关键词（检索索引源）。
    pub keywords: Vec<String>,
    /// 脱敏标记。
    pub sanitized: bool,
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 归档包。
#[derive(Debug, Clone)]
pub struct ArchivePack {
    pub items: Vec<KnowledgeItem>,
}

impl ArchivePack {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// 归档一条（未脱敏拒收）。
    pub fn put(&mut self, content: &str, kind: ArchKind, keywords: &[&str], sanitized: bool) -> bool {
        if !sanitized || keywords.is_empty() {
            return false;
        }
        self.items.push(KnowledgeItem {
            fp: fnv1a(content.as_bytes()),
            kind,
            keywords: keywords.iter().map(|k| String::from(*k)).collect(),
            sanitized: true,
        });
        true
    }

    /// 类缺检出。
    pub fn kind_gaps(&self) -> Vec<&'static str> {
        ARCH_KINDS.iter()
            .filter(|k| !self.items.iter().any(|i| i.kind == **k))
            .map(|k| k.name())
            .collect()
    }

    /// 与 F3979 自查档合并去重：指纹同 → 合并（保留一方）。
    pub fn merge_dedupe(&mut self, other: &ArchivePack) -> u32 {
        let mut merged = 0u32;
        for item in other.items.iter() {
            if self.items.iter().any(|m| m.fp == item.fp) {
                merged += 1;
            } else {
                self.items.push(item.clone());
            }
        }
        merged
    }
}

// ── 检索索引 ─────────────────────────────────────────────────────────

/// 倒排索引：关键词 → 条目下标集。
#[derive(Debug, Clone)]
pub struct KwIndex {
    pub map: Vec<(String, Vec<usize>)>,
}

impl KwIndex {
    /// 从归档包建索引。
    pub fn build(pack: &ArchivePack) -> Self {
        let mut map: Vec<(String, Vec<usize>)> = Vec::new();
        for (i, item) in pack.items.iter().enumerate() {
            for kw in item.keywords.iter() {
                match map.iter_mut().find(|(k, _)| k == kw) {
                    Some((_, idxs)) => idxs.push(i),
                    None => map.push((kw.clone(), vec![i])),
                }
            }
        }
        Self { map }
    }

    /// 检索。
    pub fn search(&self, kw: &str) -> Vec<usize> {
        self.map.iter().find(|(k, _)| k == kw).map(|(_, v)| v.clone()).unwrap_or_default()
    }

    /// 索引一致性：重算索引与现索引比对。
    pub fn consistent(&self, pack: &ArchivePack) -> bool {
        let rebuilt = KwIndex::build(pack);
        if rebuilt.map.len() != self.map.len() {
            return false;
        }
        for (k, v) in self.map.iter() {
            match rebuilt.map.iter().find(|(rk, _)| rk == k) {
                Some((_, rv)) if rv == v => {}
                _ => return false,
            }
        }
        true
    }

    /// 索引断检出：索引里的下标越界。
    pub fn broken(&self, pack: &ArchivePack) -> bool {
        self.map.iter().any(|(_, v)| v.iter().any(|i| *i >= pack.items.len()))
    }
}

// ── 五角色走查方法沉淀（域本色）─────────────────────────────────────

pub const METHOD_ITEMS: [&str; 5] = [
    "视障任务动线法", "听障字幕对拍法", "运动替代输入法", "认知降载引导法", "老年字号容错法",
];

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves251_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves251_s10kb");

    // C3997-01 诊断码段不越窗。
    chk(&mut set, "C3997-01",
        T17KCode::KIND_GAP.0 < SEG_BASE + 16 && SEG_BASE == 0x49C0, "err");

    // C3997-02 四类齐备表。
    chk(&mut set, "C3997-02",
        ARCH_KINDS.len() == 4
            && ARCH_KINDS[0].name() == "终审模式"
            && ARCH_KINDS[3].name() == "变更案例", "err");

    // C3997-03 归档：未脱敏拒收。
    let mut a = ArchivePack::new();
    chk(&mut set, "C3997-03",
        !a.put("原始录证", ArchKind::WalkFinding, &["录证"], false), "err");

    // C3997-04 脱敏后归档 + 类缺检出。
    a.put("读屏焦点序终审模式", ArchKind::FinalPattern, &["读屏", "焦点序"], true);
    a.put("联测场景 12 案例", ArchKind::JoinCase, &["联测"], true);
    a.put("走查发现：字号", ArchKind::WalkFinding, &["走查"], true);
    chk(&mut set, "C3997-04",
        a.kind_gaps() == vec!["变更案例"], "err");

    // C3997-05 补档后类齐。
    a.put("冻结后变更五步案例", ArchKind::ChangeCase, &["变更", "五步"], true);
    chk(&mut set, "C3997-05", a.kind_gaps().is_empty(), "err");

    // C3997-06 关键词空拒收。
    chk(&mut set, "C3997-06", !a.put("无关键词", ArchKind::JoinCase, &[], true), "err");

    // C3997-07 合并去重：同指纹合并。
    let mut b = ArchivePack::new();
    b.put("读屏焦点序终审模式", ArchKind::FinalPattern, &["读屏"], true);
    b.put("新知识条目", ArchKind::JoinCase, &["新"], true);
    let merged = a.merge_dedupe(&b);
    chk(&mut set, "C3997-07",
        merged == 1 && a.items.len() == 4, "err");

    // C3997-08 索引建立 + 检索。
    let idx = KwIndex::build(&a);
    chk(&mut set, "C3997-08",
        idx.search("读屏") == vec![0usize] && idx.search("五步") == vec![3usize], "err");

    // C3997-09 多关键词条目命中多槽。
    let hits = idx.search("变更");
    chk(&mut set, "C3997-09", hits == vec![3usize], "err");

    // C3997-10 索引一致性。
    chk(&mut set, "C3997-10", idx.consistent(&a), "err");

    // C3997-11 追加后索引断（未重建）。
    a.put("追加条目", ArchKind::WalkFinding, &["追加"], true);
    chk(&mut set, "C3997-11", !idx.consistent(&a), "err");

    // C3997-12 重建后一致。
    let idx2 = KwIndex::build(&a);
    chk(&mut set, "C3997-12", idx2.consistent(&a) && idx2.search("追加") == vec![4usize], "err");

    // C3997-13 索引断检出（越界下标）。
    let mut broken_idx = KwIndex { map: Vec::new() };
    broken_idx.map.push((String::from("幽灵"), vec![99]));
    chk(&mut set, "C3997-13", broken_idx.broken(&a), "err");

    // C3997-14 好索引不误报。
    chk(&mut set, "C3997-14", !idx2.broken(&a), "err");

    // C3997-15 五角色走查方法沉淀齐备（域本色）。
    chk(&mut set, "C3997-15",
        METHOD_ITEMS.len() == 5 && METHOD_ITEMS[0] == "视障任务动线法", "err");

    // C3997-16 类槽双射。
    let slots: Vec<usize> = ARCH_KINDS.iter().map(|k| k.slot()).collect();
    let mut uniq = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if slots[i] == slots[j] { uniq = false; }
        }
    }
    chk(&mut set, "C3997-16", uniq, "err");

    // C3997-17 FNV 指纹确定性 + 区分度。
    chk(&mut set, "C3997-17",
        fnv1a(b"k1") == fnv1a(b"k1") && fnv1a(b"k1") != fnv1a(b"k2"), "err");

    // C3997-18 空包索引合法且检索空。
    let idx3 = KwIndex::build(&ArchivePack::new());
    chk(&mut set, "C3997-18", idx3.search("任意").is_empty() && idx3.consistent(&ArchivePack::new()), "err");

    // C3997-19 去重计数语义（无重复为 0）。
    let mut c = ArchivePack::new();
    c.put("甲", ArchKind::JoinCase, &["甲"], true);
    let mut d = ArchivePack::new();
    d.put("乙", ArchKind::JoinCase, &["乙"], true);
    chk(&mut set, "C3997-19", c.merge_dedupe(&d) == 0 && c.items.len() == 2, "err");

    // C3997-20 错误码互异。
    let codes = [T17KCode::OK.0, T17KCode::DUP_MERGED.0, T17KCode::INDEX_BROKEN.0, T17KCode::KIND_GAP.0];
    chk(&mut set, "C3997-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsanitized_rejected() {
        let mut a = ArchivePack::new();
        assert!(!a.put("raw", ArchKind::JoinCase, &["k"], false));
    }

    #[test]
    fn dedupe_by_fingerprint() {
        let mut a = ArchivePack::new();
        a.put("same", ArchKind::JoinCase, &["k"], true);
        let mut b = ArchivePack::new();
        b.put("same", ArchKind::JoinCase, &["k"], true);
        assert_eq!(a.merge_dedupe(&b), 1);
        assert_eq!(a.items.len(), 1);
    }

    #[test]
    fn index_rebuild() {
        let mut a = ArchivePack::new();
        a.put("读屏案例", ArchKind::FinalPattern, &["读屏"], true);
        let idx = KwIndex::build(&a);
        assert!(idx.consistent(&a));
        a.put("新", ArchKind::JoinCase, &["新"], true);
        assert!(!idx.consistent(&a));
    }
}
