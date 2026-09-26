//! F132 差异表公开 · 完整设计（STAR I 主册 G-D-07）。
//!
//! **判据（主册）**：差异表条目与 F040 账本实测结果零矛盾（CI 交叉
//! 校验）；每条有追踪编号可查。
//!
//! **设计要点（主册）**：条目模板五字段（API/差异/影响/替代/状态）；
//! 影响级别定义：致命=功能不可用/降级=可用但体验降/无感=理论差异；
//! 差异被修复 → 条目转「已解决」保留（历史可溯）；新差异发现 → 判例
//! 流程进（不是悄悄改表）；按 API 族分页（窗口/GDI/网络/存储…）；
//! 星卡页反向链接；「已解决」条目保留计数进季报（F149 兼容面成长
//! 曲线）；季度刷新随账本。
//!
//! 本模块是差异表的**纯逻辑核**：五字段条目模型、影响级别裁决、
//! 族分页索引、与账本记录的交叉校验引擎（CI 红/绿）、追踪编号接入
//! （ebase::TraceId）。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use crate::stareco::ebase::TraceId;

// ---------------------------------------------------------------------------
// 影响级别
// ---------------------------------------------------------------------------

/// 致命=功能不可用；降级=可用但体验降；无感=理论差异。
/// 判定标准文档化即本枚举的文档注记（一处一事实：判定公式只此一份）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Impact {
    /// 理论差异：行为与 Windows 不同但用户路径不触达。
    Theoretical,
    /// 可用但体验降。
    Degraded,
    /// 功能不可用。
    Fatal,
}

impl Impact {
    pub fn name(self) -> &'static str {
        match self {
            Impact::Theoretical => "theoretical",
            Impact::Degraded => "degraded",
            Impact::Fatal => "fatal",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffStatus {
    /// 未解决（在册）。
    Open,
    /// 已解决——保留在表（历史可溯），计入季报成长曲线。
    Resolved,
}

/// API 族分页（主册口径：窗口/GDI/网络/存储……）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiFamily {
    Window,
    Gdi,
    Net,
    Storage,
    Input,
    Other,
}

impl ApiFamily {
    pub const ALL: [ApiFamily; 6] = [
        ApiFamily::Window,
        ApiFamily::Gdi,
        ApiFamily::Net,
        ApiFamily::Storage,
        ApiFamily::Input,
        ApiFamily::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ApiFamily::Window => "window",
            ApiFamily::Gdi => "gdi",
            ApiFamily::Net => "net",
            ApiFamily::Storage => "storage",
            ApiFamily::Input => "input",
            ApiFamily::Other => "other",
        }
    }
}

// ---------------------------------------------------------------------------
// 条目模型（五字段模板）
// ---------------------------------------------------------------------------

/// 五字段条目：API / 差异描述 / 影响级别 / 替代方案 / 状态。
/// 外加：追踪编号（判据「每条有追踪编号可查」）与账本对账键。
#[derive(Clone, Copy, Debug)]
pub struct DiffEntry {
    pub id: TraceId,
    pub api: &'static str,
    pub family: ApiFamily,
    pub delta: &'static str,
    pub impact: Impact,
    /// 替代方案（致命/降级条目必填——没出路的差异不许刊出）。
    pub workaround: &'static str,
    pub status: DiffStatus,
    /// 关联 F040 账本判例键（对账用；无关联 = 对账红）。
    pub ledger_key: &'static str,
}

impl DiffEntry {
    /// 条目形态校验：致命必须有替代方案；已解决条目必须保留原描述
    /// （不许清空了事）；API 名非空。
    pub fn well_formed(&self) -> bool {
        if self.api.is_empty() || self.delta.is_empty() {
            return false;
        }
        if !self.id.is_valid() {
            return false;
        }
        match (self.impact, self.status) {
            (Impact::Fatal, _) | (Impact::Degraded, _) => !self.workaround.is_empty(),
            (Impact::Theoretical, DiffStatus::Open) => true,
            (Impact::Theoretical, DiffStatus::Resolved) => true,
        }
    }
}

// ---------------------------------------------------------------------------
// 差异表（族分页 + 季度刷新 + 已解决保留计数）
// ---------------------------------------------------------------------------

pub struct DiffTable {
    entries: [Option<DiffEntry>; 32],
    count: usize,
    /// 每族已解决条目计数（季报「兼容面成长曲线」数据源）。
    resolved_per_family: [u16; 6],
    next_seq: u32,
}

impl DiffTable {
    pub fn new() -> DiffTable {
        DiffTable { entries: [None; 32], count: 0, resolved_per_family: [0; 6], next_seq: 1 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 新差异入表：判例流程键必填（「不是悄悄改表」——没有账本关联
    /// 的条目拒绝刊出）。
    pub fn admit(
        &mut self,
        day: u32,
        api: &'static str,
        family: ApiFamily,
        delta: &'static str,
        impact: Impact,
        workaround: &'static str,
        ledger_key: &'static str,
    ) -> Result<TraceId, &'static str> {
        if ledger_key.is_empty() {
            return Err("ledger key required");
        }
        let id = TraceId::new("DIFF", day, self.next_seq);
        if !id.is_valid() {
            return Err("bad trace id");
        }
        self.next_seq += 1;
        if self.count >= 32 {
            return Err("table full");
        }
        self.entries[self.count] = Some(DiffEntry {
            id,
            api,
            family,
            delta,
            impact,
            workaround,
            status: DiffStatus::Open,
            ledger_key,
        });
        self.count += 1;
        Ok(id)
    }

    /// 差异被修复：转已解决（保留条目 + 成长曲线 +1）。
    pub fn resolve(&mut self, id: TraceId) -> Result<(), &'static str> {
        for slot in self.entries[..self.count].iter_mut().flatten() {
            if slot.id == id {
                if slot.status == DiffStatus::Resolved {
                    return Err("already resolved");
                }
                slot.status = DiffStatus::Resolved;
                self.resolved_per_family[slot.family as usize] += 1;
                return Ok(());
            }
        }
        Err("unknown trace id")
    }

    /// 按族取页（帮助中心「兼容性-差异表」分页数据源）。
    pub fn page(&self, family: ApiFamily) -> Vec<&DiffEntry> {
        self.entries[..self.count]
            .iter()
            .filter_map(|o| o.as_ref())
            .filter(|e| e.family == family)
            .collect()
    }

    pub fn resolved_total(&self) -> u16 {
        self.resolved_per_family.iter().sum()
    }

    /// 查编号（判据「每条有追踪编号可查」）。
    pub fn lookup(&self, id: TraceId) -> Option<&DiffEntry> {
        self.entries[..self.count].iter().flatten().find(|e| e.id == id)
    }

    /// 只读条目视图（季度刷新/星卡生成用——不泄漏内部存储）。
    pub fn entries_view(&self) -> &[Option<DiffEntry>] {
        &self.entries[..self.count]
    }

    /// 星卡反向链接：给定账本判例键，列出影响该应用的未解决差异。
    pub fn impacts_of(&self, ledger_key: &str) -> Vec<&DiffEntry> {
        self.entries[..self.count]
            .iter()
            .filter_map(|o| o.as_ref())
            .filter(|e| e.ledger_key == ledger_key && e.status == DiffStatus::Open)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// CI 交叉校验引擎（与 F040 账本零矛盾）
// ---------------------------------------------------------------------------

/// 账本侧的实测结论（F040 逐件实测三关记录的抽象——由 K/C 分队供给，
/// 本层只按键读取结论）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerVerdict {
    /// 实测：该 API 表现与差异描述一致。
    Confirms,
    /// 实测：该 API 表现正常，与「在册差异」矛盾。
    NoDiff,
}

/// 交叉校验：差异表里每条 Open 且账本对得上 → 绿；账本说 NoDiff 而
/// 表还在册 → 矛盾（CI 红）；表条目引用的账本键查无实测 → 断链红
/// （防「悄悄改表」的镜像面：表单方面删账本键同样过不了 CI）。
pub fn cross_check(
    table: &DiffTable,
    ledger_lookup: impl Fn(&str) -> Option<LedgerVerdict>,
) -> Result<(), &'static str> {
    for e in table.entries[..table.count].iter().flatten() {
        let verdict = ledger_lookup(e.ledger_key).ok_or("ledger key missing")?;
        match (e.status, verdict) {
            (DiffStatus::Open, LedgerVerdict::Confirms) => {}
            (DiffStatus::Resolved, LedgerVerdict::Confirms) => {}
            (DiffStatus::Open, LedgerVerdict::NoDiff) => return Err("open diff contradicted by ledger"),
            (DiffStatus::Resolved, LedgerVerdict::NoDiff) => {
                // 已解决与账本「实测正常」一致——正是修复后的稳态。
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F132_TAG: &str = "stareco-F132-difftable";

pub fn run_difftable_checks() -> CheckSet {
    let mut set = CheckSet::new(F132_TAG);
    let day = 20260926;

    let mut table = DiffTable::new();
    let d1 = table
        .admit(day, "CreateFileW", ApiFamily::Storage, "命名管道语义未覆盖", Impact::Degraded, "改用普通文件", "F040-ledger#7z")
        .expect("admit d1");
    let d2 = table
        .admit(day, "GetPixel", ApiFamily::Gdi, "慢路径未实现", Impact::Theoretical, "", "F040-ledger#notepad2")
        .expect("admit d2");
    let d3 = table
        .admit(day, "SetWorldTransform", ApiFamily::Gdi, "世界变换矩阵部分系数", Impact::Fatal, "用位图路径替代", "F040-ledger#imgtool")
        .expect("admit d3");

    set.add("f132 entries admitted", table.len() == 3, "3 entries");
    set.add("f132 fatal requires workaround", !d3.is_valid() || table.lookup(d3).map(|e| e.well_formed()) == Some(true), "fatal+workaround");
    set.add("f132 theoretical no workaround ok", table.lookup(d2).map(|e| e.well_formed()) == Some(true), "theoretical free");

    // 族分页
    set.add("f132 family paging", table.page(ApiFamily::Gdi).len() == 2 && table.page(ApiFamily::Net).is_empty(), "gdi=2 net=0");

    // 查编号
    set.add("f132 lookup by id", table.lookup(d1).map(|e| e.api) == Some("CreateFileW"), "trace id hit");

    // 无账本键的条目拒绝（不是悄悄改表）
    set.add("f132 ledger key required", table.admit(day, "X", ApiFamily::Other, "y", Impact::Theoretical, "", "").is_err(), "no silent entry");

    // 交叉校验：全 Confirms → 绿；一条 NoDiff → 红；键缺失 → 红
    let ok = cross_check(&table, |k| {
        if k.is_empty() {
            None
        } else {
            Some(LedgerVerdict::Confirms)
        }
    });
    set.add("f132 cross-check green", ok.is_ok(), "all confirmed");
    let contra = cross_check(&table, |k| {
        if k == "F040-ledger#7z" {
            Some(LedgerVerdict::NoDiff)
        } else {
            Some(LedgerVerdict::Confirms)
        }
    });
    set.add("f132 contradiction red", contra.is_err(), "open vs nodiff");
    let missing = cross_check(&table, |k| if k == "F040-ledger#imgtool" { None } else { Some(LedgerVerdict::Confirms) });
    set.add("f132 missing ledger key red", missing.is_err(), "broken link");

    // 修复 → 保留 + 成长曲线 + 与账本 NoDiff 的稳态一致
    assert!(table.resolve(d1).is_ok());
    set.add("f132 resolve keeps entry", table.lookup(d1).is_some() && table.len() == 3, "history kept");
    set.add("f132 growth curve counter", table.resolved_total() == 1, "1 resolved");
    let steady = cross_check(&table, |k| {
        if k == "F040-ledger#7z" {
            Some(LedgerVerdict::NoDiff)
        } else {
            Some(LedgerVerdict::Confirms)
        }
    });
    set.add("f132 resolved+nodiff steady", steady.is_ok(), "post-fix steady state");
    set.add("f132 double resolve rejected", table.resolve(d1).is_err(), "idempotent guard");

    // 星卡反向链接
    let hit = table.impacts_of("F040-ledger#notepad2");
    set.add("f132 starcard backlink", hit.len() == 1 && hit[0].id == d2, "open impact only");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_lifecycle() {
        let mut t = DiffTable::new();
        let day = 20260101;
        let a = t.admit(day, "A", ApiFamily::Window, "d", Impact::Fatal, "w", "k1").unwrap();
        let b = t.admit(day, "B", ApiFamily::Window, "d", Impact::Theoretical, "", "k2").unwrap();
        assert_eq!(t.page(ApiFamily::Window).len(), 2);
        t.resolve(a).unwrap();
        t.resolve(b).unwrap();
        assert_eq!(t.resolved_total(), 2);
        assert!(t.lookup(a).unwrap().well_formed());
    }
}
