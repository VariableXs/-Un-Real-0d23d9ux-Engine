//! VE-F0083 · 渲染白名单与黑名单（VE-A 域 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0083`
//!
//! **职责定位（锚点原文）**：按驱动 / GPU 型号的行为名单（已知坏驱动降级、
//! 已知好配置直通），名单**版本化热更新**（A98 联动），名单**变更审计**。
//!
//! **判据（锚点原文）**：型号名单、版本热更、变更审计、申诉通道、判据。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 误杀 → **申诉通道** | [`ListStore::appeal`] 只接受 `Deny` 条目，把它改成 `Pending` 并发放申诉号。改状态而不是删条目：删了等于「从没判过」，复核的人看不到原判依据 |
//! | 名单冲突 → **版本仲裁** | 热更新要求新版本号**严格大于**在役版本（[`E_VERSION_NOT_ADVANCING`]）。同版本号的第二份名单直接拒——「同版本不同内容」是最难排查的一类线上事故 |
//! | 热更失败 → **回退** | 校验在**落地前**全部完成：版本递增、条目数上限、无重复键。任一不过，在役名单一个字节都不动，`rejected` 计数 +1 |
//!
//! **性能逐项分解（锚点原文）**：O(条目)。查表两趟线性（精确键 → 型号级），
//! 条目上限 64；热更新的差异计算是 64×64 的常数上界。
//!
//! **跨批对接点**：A98 热更新联动——[`ListStore::hot_update`] 的入参就是
//! A98 下发的新名单，本模块只负责「能不能换、换了记什么」。
//!
//! **无障碍与隐私（锚点原文）**：名单读屏可达（[`ListStore::a11y_lines`]）
//! ——只报版本、条目数、允许/禁止/待复核计数；不报设备序列号。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **两级键**：`(vendor, device, driver)` 精确匹配优先，`driver == 0` 是
//!   型号级兜底。只做精确匹配会让「新驱动版本」全部掉进 Unknown，
//!   只做型号级又无法针对某个坏驱动版本。
//! - **Pending 是第四种裁决**，不是「Deny 的备注」。它要能被查表返回，
//!   否则申诉后行为不变、用户以为申诉没生效。
//! - **审计记差异数而不是整份名单**：整份名单进历史会撑爆定容槽，而
//!   「加了 3 条、删了 1 条、改了 2 条」足以定位问题。
//! - **零 panic 面**：无 `unwrap`、索引经 `get`。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（0xA7DD/E 与 0xA7ED/E —— 全树 grep 确认零占用）
// ---------------------------------------------------------------------------

/// 条目重复（同键二次插入）。
pub const E_DUPLICATE_KEY: u16 = 0xA7DD;
/// 名单超容。
pub const E_LIST_FULL: u16 = 0xA7DE;
/// 新版本号未前进。
pub const E_VERSION_NOT_ADVANCING: u16 = 0xA7ED;
/// 申诉目标不是 Deny 裁决。
pub const E_NOT_DENIED: u16 = 0xA7EE;

// ---------------------------------------------------------------------------
// 二、键与裁决
// ---------------------------------------------------------------------------

/// 名单键。`driver == 0` 表示「该型号的所有驱动」。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Key {
    pub vendor: u16,
    pub device: u16,
    pub driver: u32,
}

impl Key {
    pub const fn model(vendor: u16, device: u16) -> Key {
        Key {
            vendor,
            device,
            driver: 0,
        }
    }

    pub const fn exact(vendor: u16, device: u16, driver: u32) -> Key {
        Key {
            vendor,
            device,
            driver,
        }
    }

    /// 该键的型号级投影。
    pub fn model_key(self) -> Key {
        Key {
            vendor: self.vendor,
            device: self.device,
            driver: 0,
        }
    }
}

/// 名单裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 已知好配置，直通。
    Allow,
    /// 已知坏配置，降级。
    Deny,
    /// 未收录。
    Unknown,
    /// 申诉待复核。
    Pending,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Allow => "allow",
            Verdict::Deny => "deny",
            Verdict::Unknown => "unknown",
            Verdict::Pending => "pending",
        }
    }

    /// 该裁决是否需要降级。
    pub fn degrades(self) -> bool {
        matches!(self, Verdict::Deny)
    }
}

/// 一条名单项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub key: Key,
    pub verdict: Verdict,
    /// 判定依据的一句话（编译期常量，不含用户数据）。
    pub note: &'static str,
}

// ---------------------------------------------------------------------------
// 三、名单
// ---------------------------------------------------------------------------

/// 单条上限。再多的名单不适合塞进内核常量。
pub const MAX_ENTRIES: usize = 64;

/// 一份名单。
#[derive(Clone, Debug)]
pub struct List {
    pub version: u32,
    entries: Vec<Entry>,
}

impl List {
    pub fn new(version: u32) -> List {
        List {
            version,
            entries: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// 插入一条。重复键（含跨裁决）一律拒。
    pub fn insert(&mut self, e: Entry) -> Result<(), u16> {
        if self.entries.len() >= MAX_ENTRIES {
            return Err(E_LIST_FULL);
        }
        if self.entries.iter().any(|x| x.key == e.key) {
            return Err(E_DUPLICATE_KEY);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 查表：精确键 → 型号级 → Unknown。
    pub fn lookup(&self, k: Key) -> Verdict {
        if let Some(e) = self.entries.iter().find(|x| x.key == k) {
            return e.verdict;
        }
        if k.driver != 0 {
            if let Some(e) = self.entries.iter().find(|x| x.key == k.model_key()) {
                return e.verdict;
            }
        }
        Verdict::Unknown
    }

    /// 按裁决计数：(allow, deny, pending, unknown 不入表)。
    pub fn counts(&self) -> (u32, u32, u32) {
        let mut a = 0;
        let mut d = 0;
        let mut p = 0;
        for e in self.entries.iter() {
            match e.verdict {
                Verdict::Allow => a += 1,
                Verdict::Deny => d += 1,
                Verdict::Pending => p += 1,
                Verdict::Unknown => {}
            }
        }
        (a, d, p)
    }
}

// ---------------------------------------------------------------------------
// 四、变更审计
// ---------------------------------------------------------------------------

/// 一次热更的审计记录。只记差异数——整份名单进历史会撑爆定容槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Change {
    pub from: u32,
    pub to: u32,
    pub at: u64,
    pub added: u16,
    pub removed: u16,
    pub changed: u16,
}

impl Change {
    pub fn total_touched(&self) -> u16 {
        self.added
            .saturating_add(self.removed)
            .saturating_add(self.changed)
    }

    pub fn render(&self) -> String {
        format!(
            "list {}->{} at={} +{} -{} ~{}",
            self.from, self.to, self.at, self.added, self.removed, self.changed
        )
    }
}

/// 历史槽数。
pub const MAX_HISTORY: usize = 8;

// ---------------------------------------------------------------------------
// 五、存储
// ---------------------------------------------------------------------------

/// 名单存储：在役一份、历史八条。
pub struct ListStore {
    active: List,
    history: [Option<Change>; MAX_HISTORY],
    history_count: usize,
    rejected: u32,
    appeals: u32,
}

impl ListStore {
    pub fn new(version: u32) -> ListStore {
        ListStore {
            active: List::new(version),
            history: [None; MAX_HISTORY],
            history_count: 0,
            rejected: 0,
            appeals: 0,
        }
    }

    pub fn active(&self) -> &List {
        &self.active
    }

    pub fn version(&self) -> u32 {
        self.active.version
    }

    pub fn lookup(&self, k: Key) -> Verdict {
        self.active.lookup(k)
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn appeals(&self) -> u32 {
        self.appeals
    }

    pub fn history(&self) -> &[Option<Change>; MAX_HISTORY] {
        &self.history
    }

    /// 落地一份新名单。
    ///
    /// 校验顺序固定：版本 → 容量 → 差异计算 → 落地。前三步任一失败，
    /// 在役名单不动。这个顺序保证「版本不对」不会被报成「容量不够」。
    pub fn hot_update(&mut self, next: List, at: u64) -> Result<Change, u16> {
        if next.version <= self.active.version {
            self.rejected = self.rejected.saturating_add(1);
            return Err(E_VERSION_NOT_ADVANCING);
        }
        if next.len() > MAX_ENTRIES {
            self.rejected = self.rejected.saturating_add(1);
            return Err(E_LIST_FULL);
        }
        let mut added = 0u16;
        let mut removed = 0u16;
        let mut changed = 0u16;
        for e in next.entries().iter() {
            match self.active.entries().iter().find(|x| x.key == e.key) {
                Some(old) => {
                    if old.verdict != e.verdict {
                        changed = changed.saturating_add(1);
                    }
                }
                None => added = added.saturating_add(1),
            }
        }
        for old in self.active.entries().iter() {
            if !next.entries().iter().any(|x| x.key == old.key) {
                removed = removed.saturating_add(1);
            }
        }
        let change = Change {
            from: self.active.version,
            to: next.version,
            at,
            added,
            removed,
            changed,
        };
        self.active = next;
        if self.history_count < MAX_HISTORY {
            self.history[self.history_count] = Some(change);
            self.history_count += 1;
        }
        Ok(change)
    }

    /// 申诉：把一条 Deny 改成 Pending，返回申诉号（从 1 起）。
    pub fn appeal(&mut self, k: Key) -> Result<u32, u16> {
        let mut hit = false;
        for e in self.active.entries.iter_mut() {
            if e.key == k && e.verdict == Verdict::Deny {
                e.verdict = Verdict::Pending;
                hit = true;
                break;
            }
        }
        if !hit {
            return Err(E_NOT_DENIED);
        }
        self.appeals = self.appeals.saturating_add(1);
        Ok(self.appeals)
    }

    /// 读屏。
    pub fn a11y_lines(&self) -> Vec<String> {
        let (allow, deny, pending) = self.active.counts();
        let mut out = Vec::new();
        out.push(format!(
            "渲染名单版本 {}，共 {} 条 / list v{} with {} entries",
            self.active.version,
            self.active.len(),
            self.active.version,
            self.active.len()
        ));
        out.push(format!(
            "允许 {}，禁止 {}，待复核 {} / allow {}, deny {}, pending {}",
            allow, deny, pending, allow, deny, pending
        ));
        out.push(format!(
            "热更拒收 {} 次，申诉 {} 次 / {} rejected, {} appeals",
            self.rejected, self.appeals, self.rejected, self.appeals
        ));
        out
    }
}

impl Default for ListStore {
    fn default() -> ListStore {
        ListStore::new(1)
    }
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

fn entry(vendor: u16, device: u16, driver: u32, v: Verdict) -> Entry {
    Entry {
        key: Key::exact(vendor, device, driver),
        verdict: v,
        note: "fixture",
    }
}

fn base_list() -> List {
    let mut l = List::new(1);
    let _ = l.insert(entry(0x10DE, 0x1C82, 0x1234, Verdict::Deny));
    let _ = l.insert(Entry {
        key: Key::model(0x1002, 0x67DF),
        verdict: Verdict::Allow,
        note: "model-wide",
    });
    l
}

/// VE-F0083 的十四条判据。
pub fn run_vea83_checks() -> CheckSet {
    let mut c = CheckSet::new("vea83");
    let l = base_list();

    // 1. 精确键命中。
    if l.lookup(Key::exact(0x10DE, 0x1C82, 0x1234)) == Verdict::Deny {
        c.ok("A83-查表-精确键命中");
    } else {
        c.fail("A83-查表-精确键命中", "exact key missed");
    }

    // 2. 型号级兜底：不同驱动版本落到型号条目。
    if l.lookup(Key::exact(0x1002, 0x67DF, 0x9999)) == Verdict::Allow {
        c.ok("A83-查表-型号级兜底");
    } else {
        c.fail("A83-查表-型号级兜底", "model fallback missed");
    }

    // 3. 精确优先于型号级：同型号下某个驱动被单独禁。
    let mut two = base_list();
    let _ = two.insert(Entry {
        key: Key::model(0x10DE, 0x1C82),
        verdict: Verdict::Allow,
        note: "other drivers ok",
    });
    if two.lookup(Key::exact(0x10DE, 0x1C82, 0x1234)) == Verdict::Deny
        && two.lookup(Key::exact(0x10DE, 0x1C82, 0x5555)) == Verdict::Allow
    {
        c.ok("A83-查表-精确优先型号级");
    } else {
        c.fail("A83-查表-精确优先型号级", "precedence wrong");
    }

    // 4. 未收录返回 Unknown。
    if l.lookup(Key::exact(0x8086, 0x1234, 1)) == Verdict::Unknown {
        c.ok("A83-查表-未收录即未知");
    } else {
        c.fail("A83-查表-未收录即未知", "unknown not reported");
    }

    // 5. 重复键即拒。
    let mut dup = List::new(2);
    let _ = dup.insert(entry(1, 2, 3, Verdict::Deny));
    if dup.insert(entry(1, 2, 3, Verdict::Allow)) == Err(E_DUPLICATE_KEY) {
        c.ok("A83-名单-重复键即拒");
    } else {
        c.fail("A83-名单-重复键即拒", "duplicate key accepted");
    }

    // 6. 热更版本必须前进。
    let mut store = ListStore::new(1);
    let same = List::new(1);
    if store.hot_update(same, 10) == Err(E_VERSION_NOT_ADVANCING) && store.version() == 1 {
        c.ok("A83-热更-同版本即拒");
    } else {
        c.fail("A83-热更-同版本即拒", "same version accepted");
    }

    // 7. 热更成功后版本与内容同时生效。
    let ch = store.hot_update(base_list_with_version(2), 20);
    match ch {
        Ok(x) if store.version() == 2 && x.added == 2 && x.removed == 0 => {
            c.ok("A83-热更-版本内容同步");
        }
        _ => c.fail("A83-热更-版本内容同步", "hot update did not land"),
    }

    // 8. 差异审计：删除与改动分别计数。
    let mut l3 = List::new(3);
    let _ = l3.insert(entry(0x10DE, 0x1C82, 0x1234, Verdict::Allow));
    let _ = l3.insert(Entry {
        key: Key::model(0x1002, 0x67DF),
        verdict: Verdict::Allow,
        note: "model-wide",
    });
    match store.hot_update(l3, 30) {
        Ok(x) if x.changed == 1 && x.to == 3 && x.from == 2 => {
            c.ok("A83-审计-改动单独计数");
        }
        _ => c.fail("A83-审计-改动单独计数", "diff accounting wrong"),
    }

    // 9. 历史槽按序累积。
    let hist = store.history();
    if hist[0].map(|x| x.to) == Some(2) && hist[1].map(|x| x.to) == Some(3) {
        c.ok("A83-审计-历史按序累积");
    } else {
        c.fail("A83-审计-历史按序累积", "history order wrong");
    }

    // 10. 热更失败不回退在役名单。
    let before = store.version();
    let bad = List::new(0);
    let r = store.hot_update(bad, 40);
    if r == Err(E_VERSION_NOT_ADVANCING) && store.version() == before {
        c.ok("A83-热更-失败不动在役");
    } else {
        c.fail("A83-热更-失败不动在役", "failed update rolled the live list");
    }

    // 11. 申诉把 Deny 转 Pending 并给号。
    let key = Key::exact(0x10DE, 0x1C82, 0x1234);
    // 当前在役是 l3（该键为 Allow），先换回含 Deny 的名单。
    let _ = store.hot_update(base_list_with_version(4), 50);
    let id = store.appeal(key);
    if id == Ok(1)
        && store.lookup(key) == Verdict::Pending
        && store.appeals() == 1
    {
        c.ok("A83-申诉-Deny转待复核");
    } else {
        c.fail("A83-申诉-Deny转待复核", "appeal did not take effect");
    }

    // 12. 对非 Deny 申诉即拒。
    if store.appeal(key) == Err(E_NOT_DENIED) {
        c.ok("A83-申诉-非拒绝即拒");
    } else {
        c.fail("A83-申诉-非拒绝即拒", "appeal on pending accepted");
    }

    // 13. 名单超容即拒（插入侧）：第 MAX_ENTRIES+1 条被挡下。
    let mut big = List::new(9);
    for i in 0..MAX_ENTRIES as u16 {
        let _ = big.insert(entry(1, i, 0, Verdict::Allow));
    }
    let overflow = big.insert(entry(1, 9999, 0, Verdict::Allow));
    if overflow == Err(E_LIST_FULL) && big.len() == MAX_ENTRIES {
        c.ok("A83-名单-超容即拒");
    } else {
        c.fail("A83-名单-超容即拒", "oversized list accepted");
    }

    // 14. 读屏三行且含版本。
    let lines = store.a11y_lines();
    if lines.len() == 3 && lines[0].contains(&store.version().to_string()) {
        c.ok("A83-读屏-状态可播");
    } else {
        c.fail("A83-读屏-状态可播", "a11y incomplete");
    }

    c
}

fn base_list_with_version(v: u32) -> List {
    let mut l = List::new(v);
    let _ = l.insert(entry(0x10DE, 0x1C82, 0x1234, Verdict::Deny));
    let _ = l.insert(Entry {
        key: Key::model(0x1002, 0x67DF),
        verdict: Verdict::Allow,
        note: "model-wide",
    });
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_key_projects_driver_to_zero() {
        let k = Key::exact(1, 2, 99);
        assert_eq!(k.model_key(), Key::model(1, 2));
    }

    #[test]
    fn empty_list_returns_unknown() {
        let l = List::new(1);
        assert_eq!(l.lookup(Key::model(1, 1)), Verdict::Unknown);
    }

    #[test]
    fn insert_is_bounded() {
        let mut l = List::new(1);
        for i in 0..MAX_ENTRIES as u16 {
            assert_eq!(l.insert(entry(1, i, 0, Verdict::Allow)), Ok(()));
        }
        assert_eq!(
            l.insert(entry(1, 999, 0, Verdict::Allow)),
            Err(E_LIST_FULL)
        );
    }

    #[test]
    fn counts_split_by_verdict() {
        let mut l = List::new(1);
        let _ = l.insert(entry(1, 1, 0, Verdict::Allow));
        let _ = l.insert(entry(1, 2, 0, Verdict::Deny));
        let _ = l.insert(entry(1, 3, 0, Verdict::Pending));
        assert_eq!(l.counts(), (1, 1, 1));
    }

    #[test]
    fn version_must_advance_strictly() {
        let mut s = ListStore::new(5);
        assert_eq!(
            s.hot_update(List::new(4), 1),
            Err(E_VERSION_NOT_ADVANCING)
        );
        assert_eq!(
            s.hot_update(List::new(5), 1),
            Err(E_VERSION_NOT_ADVANCING)
        );
        assert!(s.hot_update(List::new(6), 1).is_ok());
    }

    #[test]
    fn change_renders_all_counters() {
        let ch = Change {
            from: 1,
            to: 2,
            at: 7,
            added: 3,
            removed: 1,
            changed: 2,
        };
        let s = ch.render();
        assert!(s.contains("1->2"));
        assert_eq!(ch.total_touched(), 6);
    }

    #[test]
    fn appeal_on_allow_is_refused() {
        let mut s = ListStore::new(1);
        let mut l = List::new(2);
        let _ = l.insert(entry(4, 5, 6, Verdict::Allow));
        let _ = s.hot_update(l, 1);
        assert_eq!(s.appeal(Key::exact(4, 5, 6)), Err(E_NOT_DENIED));
    }

    #[test]
    fn checks_are_green() {
        let cs = run_vea83_checks();
        let (p, f) = cs.tally();
        assert_eq!(f, 0, "vea83 red: {}", cs.red_items_text());
        assert!(p >= 14);
    }
}
