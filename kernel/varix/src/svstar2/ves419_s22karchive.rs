//! ves419_s22karchive — VE-F4179 · 知识库归档
//! T 域自查知识归档：自查模式/对账口径/补齐案例/演练发现四类归档入家族知识库；
//! 归档格式（家族模板条目化 + 可检索关键词）；归档缺类→补档；索引断→重建；条目不合模板→重写；
//! 知识条目含多语言自查专技（域本色）；案例脱敏后归档。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0F80;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KaCode {
    Ok = 0,
    KindMissing = SEG_BASE + 0x01, // 0x0F81
    IndexBroken = SEG_BASE + 0x02, // 0x0F82
    FormatBad = SEG_BASE + 0x03,   // 0x0F83
}

/// 四类归档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArchKind { SelfCheckPattern = 0, AuditCaliber = 1, FillCase = 2, DrillFinding = 3 }

impl ArchKind {
    pub const ALL: [ArchKind; 4] = [
        ArchKind::SelfCheckPattern, ArchKind::AuditCaliber, ArchKind::FillCase, ArchKind::DrillFinding];
    pub fn name(self) -> &'static str {
        match self {
            ArchKind::SelfCheckPattern => "self-check-pattern", ArchKind::AuditCaliber => "audit-caliber",
            ArchKind::FillCase => "fill-case", ArchKind::DrillFinding => "drill-finding",
        }
    }
}

/// 知识条目：模板条目化（类 + 标题 + 关键词）+ 域本色专技标记 + 脱敏。
#[derive(Clone)]
pub struct KEntry {
    pub kind: ArchKind,
    pub title: &'static str,
    pub keywords: Vec<&'static str>, // 可检索关键词
    pub multilang_skill: bool,       // 多语言自查专技（域本色）
    pub sanitized: bool,             // 案例脱敏
    pub template_ok: bool,           // 家族模板合规
}

impl KEntry {
    pub fn searchable(&self, kw: &str) -> bool { self.keywords.contains(&kw) }
    pub fn valid(&self) -> bool { self.template_ok && self.sanitized && !self.title.is_empty() }
}

/// 归档库 + 检索索引。
pub struct KArchive {
    pub entries: Vec<KEntry>,
    pub index: Vec<(&'static str, usize)>, // 关键词 → 条目 idx
}

impl KArchive {
    pub fn new() -> Self { KArchive { entries: Vec::new(), index: Vec::new() } }

    pub fn put(&mut self, e: KEntry) -> bool {
        if !e.valid() { return false; } // 条目不合模板 → 拒收（重写后再归档）
        self.entries.push(e);
        true
    }

    /// 四类完备。
    pub fn kinds_covered(&self) -> bool {
        ArchKind::ALL.iter().all(|k| self.entries.iter().any(|e| e.kind == *k))
    }

    /// 缺类补档定位。
    pub fn missing_kinds(&self) -> Vec<&'static str> {
        ArchKind::ALL.iter()
            .filter(|k| !self.entries.iter().any(|e| e.kind == **k))
            .map(|k| k.name()).collect()
    }

    /// 建索引。
    pub fn build_index(&mut self) {
        self.index.clear();
        for (i, e) in self.entries.iter().enumerate() {
            for kw in e.keywords.iter() {
                self.index.push((kw, i));
            }
        }
    }

    /// 检索。
    pub fn search(&self, kw: &str) -> Vec<usize> {
        self.index.iter().filter(|(k, _)| *k == kw).map(|(_, i)| *i).collect()
    }

    /// 索引断检测：索引命中的 idx 必须真实存在。
    pub fn index_intact(&self) -> bool {
        self.index.iter().all(|(_, i)| *i < self.entries.len())
    }

    /// 重建（自证）。
    pub fn rebuild(&mut self) { self.build_index(); }

    /// 域本色：多语言自查专技条目在库。
    pub fn multilang_skill_present(&self) -> bool {
        self.entries.iter().any(|e| e.multilang_skill && e.valid())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves419_checks() -> CheckSet {
    let mut set = CheckSet::new("ves419");
    let mut ka = KArchive::new();

    // C419-01 四类归档入库。
    let samples = [
        (ArchKind::SelfCheckPattern, "自查模式：组×维矩阵铺排", vec!["矩阵", "自查"], true),
        (ArchKind::AuditCaliber, "对账口径：五维单源", vec!["对账", "五维"], false),
        (ArchKind::FillCase, "补齐案例：P0 硬门降级", vec!["补齐", "硬门"], false),
        (ArchKind::DrillFinding, "演练发现：断链补链", vec!["演练", "断链"], true),
    ];
    for (k, t, kws, ml) in samples {
        assert!(ka.put(KEntry { kind: k, title: t, keywords: kws, multilang_skill: ml, sanitized: true, template_ok: true }));
    }
    chk(&mut set, "C419-01", ka.kinds_covered() && ka.entries.len() == 4, "四类齐");

    // C419-02 建索引 + 检索。
    ka.build_index();
    chk(&mut set, "C419-02", ka.index_intact() && ka.search("矩阵") == vec![0]
        && ka.search("断链") == vec![3], "索引检索命中");

    // C419-03 多关键词条目多命中。
    chk(&mut set, "C419-03", ka.search("自查").len() == 1, "关键词命中");

    // C419-04 缺类补档定位。
    let mut ka2 = KArchive::new();
    ka2.put(KEntry { kind: ArchKind::FillCase, title: "x", keywords: vec!["k"], multilang_skill: false, sanitized: true, template_ok: true });
    chk(&mut set, "C419-04", ka2.missing_kinds() == vec!["self-check-pattern", "audit-caliber", "drill-finding"], "缺三类定位");

    // C419-05 条目不合模板拒收（重写后再归档）。
    chk(&mut set, "C419-05", !ka2.put(KEntry { kind: ArchKind::FillCase, title: "", keywords: vec![], multilang_skill: false, sanitized: false, template_ok: false }), "不合模板拒收");
    chk(&mut set, "C419-05b", ka2.put(KEntry { kind: ArchKind::FillCase, title: "重写后条目", keywords: vec!["重写"], multilang_skill: false, sanitized: true, template_ok: true }), "重写后过");

    // C419-06 未脱敏拒收。
    chk(&mut set, "C419-06", !ka2.put(KEntry { kind: ArchKind::DrillFinding, title: "y", keywords: vec![], multilang_skill: false, sanitized: false, template_ok: true }), "未脱敏拒收");

    // C419-07 索引断检测（手工注入坏 idx）。
    let mut ka3 = KArchive::new();
    ka3.put(KEntry { kind: ArchKind::FillCase, title: "a", keywords: vec!["kw"], multilang_skill: false, sanitized: true, template_ok: true });
    ka3.build_index();
    ka3.index.push(("ghost", 99));
    chk(&mut set, "C419-07", !ka3.index_intact(), "索引断检出");

    // C419-08 重建修复。
    ka3.rebuild();
    chk(&mut set, "C419-08", ka3.index_intact(), "重建复绿");

    // C419-09 域本色：多语言自查专技条目。
    chk(&mut set, "C419-09", ka.multilang_skill_present(), "专技条目在库");

    // C419-10 四类名单单源。
    chk(&mut set, "C419-10", ArchKind::ALL.len() == 4
        && ArchKind::ALL[0].name() == "self-check-pattern"
        && ArchKind::ALL[3].name() == "drill-finding", "四类单源");

    // C419-11 空库检索恒空。
    let empty = KArchive::new();
    chk(&mut set, "C419-11", empty.search("x").is_empty() && empty.index_intact(), "空库自洽");

    // C419-12 覆盖登记幂等（同标题重条目追加为独立条目——知识库允许多版本）。
    let before = ka.entries.len();
    ka.put(KEntry { kind: ArchKind::FillCase, title: "补齐案例：P0 硬门降级", keywords: vec!["补齐"], multilang_skill: false, sanitized: true, template_ok: true });
    chk(&mut set, "C419-12", ka.entries.len() == before + 1, "追加版本条目");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves419_smoke() {
        let set = run_ves419_checks();
        assert!(set.failed_count() == 0, "ves419 red: {}", set.red_items_text());
    }
}
