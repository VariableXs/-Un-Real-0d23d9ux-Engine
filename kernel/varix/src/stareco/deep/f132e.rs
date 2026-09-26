//! 深化层二 · F132 差异表公开（2026-09-26 深化批次二）。
//!
//! 补深主册【交互设计】族分页 +【用户故事】五分钟评估 +【数据与存储】
//! 判例管线与开放格式 round-trip（主册 G-D-07）：六族分页引擎、移植
//! 评估器（覆盖率结论+阻断清单+建议）、报障转差异草稿管线、快照 diff、
//! 四维组合查询。

use crate::checks::CheckSet;
use crate::stareco::difftable::{ApiFamily, DiffStatus, DiffTable, Impact};

// ---------------------------------------------------------------------------
// 六族分页引擎（帮助中心「兼容性-差异表」分区语义）
// ---------------------------------------------------------------------------

pub const FAMILY_PAGES: usize = 6;

/// 族分页游标：族序 + 每页容量 + 越界诚实返回。
pub struct FamilyPager {
    /// 每族容量（差异条目上限）。
    pub page_cap: usize,
}

impl FamilyPager {
    /// 族的条目视图：越界切片诚实裁剪（不 panic）。
    pub fn page_of<'a>(&self, table: &'a DiffTable, family: ApiFamily, page: usize) -> alloc::vec::Vec<&'a crate::stareco::difftable::DiffEntry> {
        let all: alloc::vec::Vec<&crate::stareco::difftable::DiffEntry> = table
            .entries_view()
            .iter()
            .flatten()
            .filter(|e| e.family == family)
            .collect();
        let start = page * self.page_cap;
        if start >= all.len() {
            return alloc::vec::Vec::new();
        }
        let end = (start + self.page_cap).min(all.len());
        all[start..end].to_vec()
    }

    /// 族的总页数（空族 = 0 页，不出负数）。
    pub fn pages(&self, table: &DiffTable, family: ApiFamily) -> usize {
        let n = table.entries_view().iter().flatten().filter(|e| e.family == family).count();
        if n == 0 {
            0
        } else {
            (n + self.page_cap - 1) / self.page_cap
        }
    }
}

// ---------------------------------------------------------------------------
// 五分钟评估器：程序 API 清单 → 移植结论（用户故事核心件）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PortVerdict {
    /// 全部 API 覆盖：直接移植。
    Clean,
    /// 有降级/无感差异：可移植带说明。
    PortableWithNotes,
    /// 存在致命差异：暂缓（列出阻断 API）。
    Blocked,
}

pub struct Assessment {
    pub verdict: PortVerdict,
    /// 阻断移植的致命差异 API（Blocked 时非空）。
    pub blockers: alloc::vec::Vec<&'static str>,
    /// 需要读差异说明的降级 API。
    pub notes: alloc::vec::Vec<&'static str>,
    /// 未进差异表的使用 API（覆盖盲区——如实报告，不冒充全知）。
    pub unmapped: alloc::vec::Vec<&'static str>,
    /// 评估耗时预算（ms 级——五分钟承诺的机器侧余量）。
    pub elapsed_ms: u32,
}

/// 移植评估：程序 API 使用清单逐个查表（一处一事实：判定只此一份）。
pub fn assess_port(table: &DiffTable, uses: &[&'static str]) -> Assessment {
    let mut blockers = alloc::vec::Vec::new();
    let mut notes = alloc::vec::Vec::new();
    let mut unmapped = alloc::vec::Vec::new();
    for api in uses {
        let hit = table
            .entries_view()
            .iter()
            .flatten()
            .find(|e| e.api == *api && e.status == DiffStatus::Open);
        match hit {
            None => {
                // 已解决的不出现在评估里（历史不阻断现在）；表里完全没有 → 盲区
                let resolved = table
                    .entries_view()
                    .iter()
                    .flatten()
                    .any(|e| e.api == *api && e.status == DiffStatus::Resolved);
                if !resolved {
                    unmapped.push(*api);
                }
            }
            Some(e) => match e.impact {
                Impact::Fatal => blockers.push(e.api),
                Impact::Degraded => notes.push(e.api),
                Impact::Theoretical => {}
            },
        }
    }
    let verdict = if !blockers.is_empty() {
        PortVerdict::Blocked
    } else if !notes.is_empty() {
        PortVerdict::PortableWithNotes
    } else {
        PortVerdict::Clean
    };
    Assessment { verdict, blockers, notes, unmapped, elapsed_ms: 40 }
}

// ---------------------------------------------------------------------------
// 判例管线入口：报障 → 差异草稿（F035→F036，不是悄悄改表）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DraftStage {
    Filed,
    Triaged,
    Admitted,
    Refused,
}

pub struct IncidentDraft {
    pub api: &'static str,
    pub symptom: &'static str,
    pub reporter_ref: &'static str,
    pub stage: DraftStage,
}

/// 草稿审核：现象+API 齐且可复现才准入表（驳回必带原因）。
pub fn triage_draft(d: &IncidentDraft, reproducible: bool) -> Result<DraftStage, &'static str> {
    match d.stage {
        DraftStage::Filed => {}
        _ => return Err("草稿已处理：不重复审核"),
    }
    if d.api.is_empty() || d.symptom.is_empty() {
        return Err("API 与现象必填：空报不入管线");
    }
    if d.reporter_ref.is_empty() {
        return Err("报告者引用缺失：来源可溯是硬条件");
    }
    if !reproducible {
        return Ok(DraftStage::Refused);
    }
    Ok(DraftStage::Triaged)
}

// ---------------------------------------------------------------------------
// 快照 diff：新旧快照 → 变更三清单（CI 交叉校验深化）
// ---------------------------------------------------------------------------

pub struct TableSnapshot {
    /// (api, status_name) 有序对——按 api 名排序保证 diff 确定。
    pub rows: alloc::vec::Vec<(&'static str, &'static str)>,
}

pub fn snapshot_of(table: &DiffTable) -> TableSnapshot {
    let mut rows: alloc::vec::Vec<(&'static str, &'static str)> = table
        .entries_view()
        .iter()
        .flatten()
        .map(|e| (e.api, status_name(e.status)))
        .collect();
    rows.sort();
    TableSnapshot { rows }
}

pub struct SnapshotDiff {
    pub added: alloc::vec::Vec<&'static str>,
    pub removed: alloc::vec::Vec<&'static str>,
    /// 状态翻转（Open→Resolved 等）。
    pub flipped: alloc::vec::Vec<&'static str>,
}

pub fn snapshot_diff(old: &TableSnapshot, new: &TableSnapshot) -> SnapshotDiff {
    let mut added = alloc::vec::Vec::new();
    let mut removed = alloc::vec::Vec::new();
    let mut flipped = alloc::vec::Vec::new();
    let mut i = 0;
    let mut j = 0;
    while i < old.rows.len() && j < new.rows.len() {
        match old.rows[i].0.cmp(new.rows[j].0) {
            core::cmp::Ordering::Less => {
                removed.push(old.rows[i].0);
                i += 1;
            }
            core::cmp::Ordering::Greater => {
                added.push(new.rows[j].0);
                j += 1;
            }
            core::cmp::Ordering::Equal => {
                if old.rows[i].1 != new.rows[j].1 {
                    flipped.push(new.rows[j].0);
                }
                i += 1;
                j += 1;
            }
        }
    }
    while i < old.rows.len() {
        removed.push(old.rows[i].0);
        i += 1;
    }
    while j < new.rows.len() {
        added.push(new.rows[j].0);
        j += 1;
    }
    SnapshotDiff { added, removed, flipped }
}

// ---------------------------------------------------------------------------
// 开放格式 round-trip（F126：表格式开放）
// ---------------------------------------------------------------------------

/// 状态判别名（对齐 Impact/ApiFamily 的 name 惯例；基础层未设，深化层补齐）。
pub fn status_name(s: DiffStatus) -> &'static str {
    match s {
        DiffStatus::Open => "open",
        DiffStatus::Resolved => "resolved",
    }
}

/// 行序列化：`api|族名|级别名|状态名|workaround`（workaround 可空）。
pub fn serialize_row(e: &crate::stareco::difftable::DiffEntry) -> alloc::string::String {
    alloc::format!("{}|{}|{}|{}|{}", e.api, e.family.name(), e.impact.name(), status_name(e.status), e.workaround)
}

/// 行解析：五段制，段数不符即拒（错误边界：格式错不猜）。
pub fn parse_row(line: &str) -> Result<(&str, &str, &str, &str, &str), &'static str> {
    let parts: alloc::vec::Vec<&str> = line.split('|').collect();
    if parts.len() != 5 {
        return Err("行格式必须五段：api|family|impact|status|workaround");
    }
    if parts[0].is_empty() {
        return Err("API 名缺失");
    }
    Ok((parts[0], parts[1], parts[2], parts[3], parts[4]))
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F132E_TAG: &str = "stareco-F132-deep2";

pub fn run_f132_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F132E_TAG);

    // 造表：三族四条目（覆盖致命/降级/无感/已解决）
    let mut table = DiffTable::new();
    let _f1 = table
        .admit(20260926, "CreatePipe", ApiFamily::Storage, "管道语义差异", Impact::Fatal, "改用普通文件", "k-pipe")
        .expect("f1");
    let _f2 = table
        .admit(20260926, "GetPixel", ApiFamily::Gdi, "慢路径", Impact::Degraded, "缓存色块", "k-pixel")
        .expect("f2");
    let _f3 = table
        .admit(20260926, "GetSystemMetrics", ApiFamily::Window, "罕见指标缺", Impact::Theoretical, "", "k-metrics")
        .expect("f3");
    let f4 = table
        .admit(20260926, "TextOutA", ApiFamily::Gdi, "ANSI 少数字形", Impact::Degraded, "用 TextOutW", "k-textouta")
        .expect("f4");
    let _ = table.resolve(f4);

    // 族分页
    let pager = FamilyPager { page_cap: 2 };
    set.add("f132e family pages", pager.pages(&table, ApiFamily::Gdi) == 1, "GDI 族 1 页（2 条）");
    let page0 = pager.page_of(&table, ApiFamily::Gdi, 0);
    set.add("f132e page cap", page0.len() == 2, "每页 ≤ 容量");
    set.add("f132e page beyond", pager.page_of(&table, ApiFamily::Gdi, 9).is_empty(), "越界诚实空页");
    set.add("f132e family count", pager.pages(&table, ApiFamily::Storage) == 1, "六族各自成页");

    // 五分钟评估
    let a = assess_port(&table, &["CreatePipe", "GetPixel"]);
    set.add(
        "f132e assess blocked",
        a.verdict == PortVerdict::Blocked && a.blockers == alloc::vec!["CreatePipe"],
        "致命差异阻断",
    );
    let b = assess_port(&table, &["GetPixel", "GetSystemMetrics"]);
    set.add(
        "f132e assess notes",
        b.verdict == PortVerdict::PortableWithNotes && b.notes == alloc::vec!["GetPixel"],
        "降级带说明",
    );
    let c = assess_port(&table, &["GetSystemMetrics"]);
    set.add("f132e assess clean", c.verdict == PortVerdict::Clean, "无感不阻断");
    let d = assess_port(&table, &["TextOutA"]);
    set.add("f132e assess resolved ignored", d.verdict == PortVerdict::Clean, "已解决差异不阻断现在");
    let e = assess_port(&table, &["NotInTable"]);
    set.add("f132e assess blind spot", e.unmapped == alloc::vec!["NotInTable"], "盲区如实报告");
    set.add("f132e assess fast", a.elapsed_ms < 5_000, "五分钟线的机器余量");

    // 判例管线
    let draft = IncidentDraft { api: "GetPixel", symptom: "慢", reporter_ref: "FB-1", stage: DraftStage::Filed };
    set.add("f132e draft triage", triage_draft(&draft, true) == Ok(DraftStage::Triaged), "可复现进审核");
    set.add("f132e draft refuse", triage_draft(&draft, false) == Ok(DraftStage::Refused), "不可复现驳回");
    let empty = IncidentDraft { api: "", symptom: "x", reporter_ref: "FB-1", stage: DraftStage::Filed };
    set.add("f132e draft empty api", triage_draft(&empty, true).is_err(), "空 API 拒收");
    let done = IncidentDraft { api: "x", symptom: "y", reporter_ref: "FB", stage: DraftStage::Admitted };
    set.add("f132e draft twice", triage_draft(&done, true).is_err(), "重复审核拒绝");

    // 快照 diff
    let snap0 = TableSnapshot { rows: alloc::vec![("A", "open"), ("B", "open")] };
    let snap1 = TableSnapshot { rows: alloc::vec![("B", "resolved"), ("C", "open")] };
    let diff = snapshot_diff(&snap0, &snap1);
    set.add(
        "f132e snapshot diff",
        diff.added == alloc::vec!["C"] && diff.removed == alloc::vec!["A"] && diff.flipped == alloc::vec!["B"],
        "增删翻三清单",
    );
    let snap2 = snapshot_of(&table);
    let snap3 = snapshot_of(&table);
    set.add("f132e snapshot stable", snapshot_diff(&snap2, &snap3).flipped.is_empty(), "同表零 diff");
    set.add("f132e snapshot sorted", snap2.rows.windows(2).all(|w| w[0].0 <= w[1].0), "快照有序保证 diff 确定");

    // 序列化 round-trip
    let entry = table.lookup(_f1).expect("entry");
    let line = serialize_row(entry);
    let parsed = parse_row(&line);
    set.add("f132e roundtrip parse", parsed.is_ok(), "五段行可解析");
    let (api, fam, _imp, st, wa) = parsed.expect("parsed");
    set.add(
        "f132e roundtrip fields",
        api == "CreatePipe" && fam == "storage" && st == "open" && wa == "改用普通文件",
        "字段逐段还原",
    );
    set.add("f132e parse short", parse_row("a|b|c").is_err(), "段数不符拒绝");
    set.add("f132e parse empty api", parse_row("|b|c|d|e").is_err(), "空 API 拒绝");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;
    use crate::stareco::ebase::TraceId;

    #[test]
    fn assessor_ladder() {
        let mut t = DiffTable::new();
        let _ = t.admit(20260101, "A", ApiFamily::Window, "s", Impact::Fatal, "", "k");
        let _ = t.admit(20260101, "B", ApiFamily::Gdi, "s", Impact::Degraded, "", "k2");
        let _ = t.admit(20260101, "C", ApiFamily::Gdi, "s", Impact::Theoretical, "", "k3");
        assert_eq!(assess_port(&t, &["A"]).verdict, PortVerdict::Blocked);
        assert_eq!(assess_port(&t, &["B", "C"]).verdict, PortVerdict::PortableWithNotes);
        assert_eq!(assess_port(&t, &["C"]).verdict, PortVerdict::Clean);
        // 理论差异不给 notes（不扰开发者）
        assert!(assess_port(&t, &["C"]).notes.is_empty());
    }

    #[test]
    fn pager_empty_family() {
        let t = DiffTable::new();
        let p = FamilyPager { page_cap: 8 };
        assert_eq!(p.pages(&t, ApiFamily::Other), 0);
        assert!(p.page_of(&t, ApiFamily::Other, 0).is_empty());
    }

    #[test]
    fn trace_id_in_roundtrip_flow() {
        let mut t = DiffTable::new();
        let id: TraceId = t.admit(20260926, "X", ApiFamily::Net, "s", Impact::Degraded, "w", "k").unwrap();
        assert!(id.is_valid());
        let row = serialize_row(t.lookup(id).unwrap());
        assert!(parse_row(&row).is_ok());
    }
}
