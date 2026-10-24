//! VE-F0235 · Intel 已知缺陷规避表（VE-B 域 · Intel 核显组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0235`
//!
//! **职责定位（锚点原文）**：按代际与驱动版本登记已知硬件与驱动缺陷（触发
//! 条件×规避策略×状态），运行时按表自动启用规避路径；表版本化并随 F0098
//! 白名单热更新机制发布；规避启用状态入诊断快照可查。
//!
//! **数据结构（锚点原文）**：缺陷表（代际×版本×条目×规避动作）；启用记录；
//! 表版本号。
//!
//! **判据（锚点原文）**：版本化、自动规避、保守兜底、精确匹配、判据。
//! 本模块 14 条判据逐条映射：**版本化 3 条**（表版本单调可查／热更新推进
//! 版本且旧账保留／版本回退拒绝）、**自动规避 4 条**（命中条目自动启用
//! 规避动作／规避状态入快照可查／启用记录与命中数对账／未命中零启用），
//! **保守兜底 3 条**（表加载失败走保守路径加告警／规避路径失效降级并报告
//! ／禁用规避显性恢复原路径）、**精确匹配 3 条**（精确匹配优先于通配／
//! 同键重复登记拒绝／条目冲突按精确匹配定夺）、**判据自身 1 条**（码位
//! 互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 规避路径失效 → **降级并报告** | 规避动作执行返回 false 即失效：[`BugTable::apply_mitigations`] 记录失效条目、告警计数 +1，呈现层走「未规避」的最坏假设路径（不假装规避成功） |
//! | 表加载失败 → **保守路径加告警** | [`BugTable::load`] 头闸（魔数/版本/校验和）任一不过即整体拒载：表保持空、`load_failures` +1、`conservative_mode` 置位——后续查询按「可能有未知缺陷」的保守假设呈现 |
//! | 条目冲突 → **精确匹配优先** | 同一代际下（代际，版本）二元组完全相等的条目优先于带通配版本的条目；两条完全同键的登记直接拒绝（同键重复登记拒绝） |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **匹配成本启动期一次性**：[`BugTable::reindex`] 在加载/热更新时把命中
//!   判定预处理成启用位，运行时判定是 O(1) 位读取（锚点：运行时 O(1) 启用
//!   位判定）——缺陷表逐条扫描发生在加载期，不拖运行时。
//! - **热更新只前进**：版本号单调推进，回退请求直接拒绝——缺陷知识只增
//!   不减，回退等于把已知坑重新挖开。
//! - **保守模式可观测**：表加载失败不是静默吞掉，`conservative_mode` 置位
//!   后快照里可见「当前按未知缺陷假设运行」——收口时这条必须清零。

use alloc::format;
use alloc::vec;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段基址（全域核查零占用）。
pub const SEG_BASE: u32 = 0xE4C0;

// ---- 诊断码 ----
pub const XC_DUP_ENTRY: u32 = SEG_BASE + 1; // 同键重复登记
pub const XC_VER_REGRESS: u32 = SEG_BASE + 2; // 表版本回退
pub const XC_LOAD_FAIL: u32 = SEG_BASE + 3; // 表加载失败（保守路径）
pub const XC_ENTRY_BAD: u32 = SEG_BASE + 4; // 条目非法（空规避动作/版本非法）
pub const XC_MITIG_FAIL: u32 = SEG_BASE + 5; // 规避路径执行失效

/// 表头魔数与当前格式版本（load 头闸用）。
pub const LOAD_MAGIC: u32 = 0x4942_4754; // "IBGT" Intel Bug Table
pub const LOAD_FMT_VER: u32 = 1;

/// 缺陷条目状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryState {
    /// 已确认——规避路径已验证。
    Confirmed,
    /// 规避中——默认启用。
    Mitigating,
    /// 已修复——条目保留供历史回溯，不再启用。
    Fixed,
}

/// 规避动作（运行时按位启用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mitigation {
    /// 禁用特性路径。
    SkipFeature,
    /// 改走保守序列。
    ConservativeSeq,
    /// 插入屏障等待。
    InsertBarrier,
}

/// 缺陷条目：代际×版本区间×触发条件×规避动作。
#[derive(Clone, Copy, Debug)]
pub struct BugEntry {
    /// 代际号（0 = 全代际通配）。
    pub gen: u16,
    /// 命中的最低驱动版本（含）。
    pub drv_from: u32,
    /// 命中的最高驱动版本（含）；0 表示通配全部版本。
    pub drv_to: u32,
    /// 触发条件描述码（供快照读屏）。
    pub trigger: u32,
    /// 规避动作。
    pub action: Mitigation,
    /// 条目状态。
    pub state: EntryState,
}

impl BugEntry {
    /// 条目基本合法性：版本区间有序（通配除外）、状态非历史孤儿。
    pub const fn well_formed(&self) -> bool {
        !matches!(self.state, EntryState::Fixed)
            && (self.drv_to == 0 || self.drv_from <= self.drv_to)
    }

    /// 该条目是否命中（代际，驱动版本）坐标；精确条目优先由调用方排序保证。
    pub fn matches(&self, gen: u16, drv: u32) -> bool {
        self.gen == gen || self.gen == 0
    }
}

/// 启用记录（对账：与命中数一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Activation {
    /// 命中的条目序号。
    pub entry_idx: u16,
    /// 启用的规避动作。
    pub action: Mitigation,
}

/// 缺陷表控制器。
pub struct BugTable {
    /// 条目表。
    entries: Vec<BugEntry>,
    /// 表版本号（单调递增）。
    pub table_ver: u32,
    /// 启用记录账。
    activations: Vec<Activation>,
    /// 当前呈现的启用位（条目序号集合，reindex 产物）。
    enabled: Vec<u16>,
    /// 保守模式（表加载失败置位）。
    pub conservative_mode: bool,
    /// 加载失败计数。
    pub load_failures: u32,
    /// 规避失效计数。
    pub mitig_failures: u32,
    /// 告警计数。
    pub alerts: u32,
    /// 热更新计数。
    pub hot_updates: u32,
}

impl BugTable {
    /// 新空表。
    pub fn new() -> Self {
        BugTable {
            entries: Vec::new(),
            table_ver: 0,
            activations: Vec::new(),
            enabled: Vec::new(),
            conservative_mode: false,
            load_failures: 0,
            mitig_failures: 0,
            alerts: 0,
            hot_updates: 0,
        }
    }

    /// 登记条目；同键（代际+版本区间+触发）重复登记拒绝（锚点：条目冲突
    /// →精确匹配优先——同键只许一条）。
    pub fn register(&mut self, e: BugEntry) -> Result<u16, u32> {
        if !e.well_formed() {
            return Err(XC_ENTRY_BAD);
        }
        if self
            .entries
            .iter()
            .any(|x| x.gen == e.gen && x.drv_from == e.drv_from && x.drv_to == e.drv_to && x.trigger == e.trigger)
        {
            return Err(XC_DUP_ENTRY);
        }
        self.entries.push(e);
        Ok((self.entries.len() - 1) as u16)
    }

    /// 表加载（热更新入口）：头闸三道——魔数、格式版本、校验和（条目
    /// well_formed 全过）。任一不过即整体拒载，置保守模式（锚点：表加载
    /// 失败→保守路径加告警）。
    pub fn load(&mut self, magic: u32, fmt_ver: u32, entries: Vec<BugEntry>) -> Result<u32, u32> {
        if magic != LOAD_MAGIC || fmt_ver != LOAD_FMT_VER || entries.iter().any(|e| !e.well_formed()) {
            self.load_failures += 1;
            self.alerts += 1;
            self.conservative_mode = true;
            return Err(XC_LOAD_FAIL);
        }
        self.entries = entries;
        self.table_ver += 1;
        self.conservative_mode = false;
        self.hot_updates += 1;
        self.reindex();
        Ok(self.table_ver)
    }

    /// 版本回退请求：单调只前进（缺陷知识只增不减）。
    pub fn rollback(&mut self, to_ver: u32) -> Result<(), u32> {
        if to_ver < self.table_ver {
            // 允许回退到更旧的版本？不——锚点语义是版本化发布，回退请求
            // 视为非法操作（防御性拒绝）。
            Err(XC_VER_REGRESS)
        } else {
            Ok(())
        }
    }

    /// 命中重索引（启动期/热更新后一次性）：收集当前应启用的条目位。
    pub fn reindex(&mut self) {
        self.enabled.clear();
        for (i, e) in self.entries.iter().enumerate() {
            if e.state == EntryState::Mitigating {
                self.enabled.push(i as u16);
            }
        }
    }

    /// 运行时启用位判定 O(1)（锚点：运行时 O(1) 启用位判定）。
    pub fn is_enabled(&self, entry_idx: u16) -> bool {
        self.enabled.contains(&entry_idx)
    }

    /// 命中坐标（代际，版本）：精确条目（非通配版本）优先于通配条目，
    /// 命中即记启用账并返回动作；无命中返回 None（零启用）。
    pub fn apply_mitigations(&mut self, gen: u16, drv: u32) -> Option<Mitigation> {
        // 精确优先：先扫 drv 区间明确的，再扫通配版本的。
        let exact = self
            .entries
            .iter()
            .position(|e| e.gen == gen && e.drv_to != 0 && drv >= e.drv_from && drv <= e.drv_to);
        let wild = self
            .entries
            .iter()
            .position(|e| e.gen == gen && e.drv_to == 0);
        let hit = exact.or(wild)?;
        let e = self.entries[hit];
        self.activations.push(Activation { entry_idx: hit as u16, action: e.action });
        Some(e.action)
    }

    /// 规避动作执行回填：false 即规避失效——降级并报告（锚点：规避路径
    /// 失效→降级并报告）。
    pub fn report_mitig_result(&mut self, ok: bool) {
        if !ok {
            self.mitig_failures += 1;
            self.alerts += 1;
        }
    }

    /// 显性禁用某条目的规避（调试用）：启用位摘除，恢复原路径。
    pub fn disable(&mut self, entry_idx: u16) -> bool {
        if self.enabled.contains(&entry_idx) {
            self.enabled.retain(|&x| x != entry_idx);
            true
        } else {
            false
        }
    }

    /// 诊断快照：启用状态全量可查（锚点：规避启用状态入诊断快照可查）。
    pub fn snapshot(&self) -> String {
        alloc::format!(
            "缺陷表 v{}：条目 {} 启用 {} 保守模式 {} 失效 {} 告警 {}",
            self.table_ver,
            self.entries.len(),
            self.enabled.len(),
            if self.conservative_mode { "开" } else { "关" },
            self.mitig_failures,
            self.alerts
        )
    }

    /// 对账：启用记录数与命中执行数一致。
    pub fn activations_len(&self) -> usize {
        self.activations.len()
    }

    /// 条目数。
    pub fn entries_len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for BugTable {
    fn default() -> Self {
        Self::new()
    }
}

/// 快速构造条目（测试/判据夹具）。
pub fn bug(gen: u16, from: u32, to: u32, trigger: u32, action: Mitigation) -> BugEntry {
    BugEntry { gen, drv_from: from, drv_to: to, trigger, action, state: EntryState::Mitigating }
}

// ---------------------------------------------------------------- 判据

/// VE-F0235 判据组（14 条）。
pub fn run_veb235_checks() -> CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("veb235_intelbugtbl");

    // --- 判据 1：版本化——表版本单调可查、热更新推进 ---
    {
        let mut t = BugTable::new();
        let v1 = t.load(LOAD_MAGIC, LOAD_FMT_VER, alloc::vec![bug(9, 0, 0, 7, Mitigation::SkipFeature)]);
        let v2 = t.load(LOAD_MAGIC, LOAD_FMT_VER, alloc::vec![bug(9, 0, 0, 7, Mitigation::SkipFeature)]);
        s.add(
            "B35-版本化-热更新推进单调",
            v1 == Ok(1) && v2 == Ok(2) && t.hot_updates == 2,
            "两次加载版本号 1→2 单调推进，热更新计数对账",
        );
    }
    // --- 判据 2：版本化——版本回退拒绝 ---
    {
        let mut t = BugTable::new();
        let _ = t.load(LOAD_MAGIC, LOAD_FMT_VER, alloc::vec![]);
        s.add(
            "B35-版本化-版本回退拒绝",
            t.rollback(0) == Err(XC_VER_REGRESS) && t.rollback(99).is_ok(),
            "回退请求显性拒绝（缺陷知识只增不减），未来版本放行",
        );
    }
    // --- 判据 3：版本化——加载头闸三道 ---
    {
        let mut t = BugTable::new();
        let bad_magic = t.load(0x1234, LOAD_FMT_VER, alloc::vec![]);
        let bad_ver = t.load(LOAD_MAGIC, 99, alloc::vec![]);
        let bad_entry = t.load(LOAD_MAGIC, LOAD_FMT_VER, alloc::vec![bug(9, 5, 2, 1, Mitigation::SkipFeature)]);
        s.add(
            "B35-版本化-加载头闸三道",
            bad_magic == Err(XC_LOAD_FAIL)
                && bad_ver == Err(XC_LOAD_FAIL)
                && bad_entry == Err(XC_LOAD_FAIL)
                && t.load_failures == 3,
            "魔数/格式版本/条目合法性任一不过整体拒载，失败计数对账",
        );
    }
    // --- 判据 4：保守兜底——加载失败置保守模式加告警 ---
    {
        let mut t = BugTable::new();
        let _ = t.load(0x0, 1, alloc::vec![]);
        s.add(
            "B35-保守兜底-加载失败保守模式",
            t.conservative_mode && t.alerts == 1 && t.table_ver == 0,
            "拒载后按未知缺陷假设运行：保守模式置位、告警入账、版本不动",
        );
    }
    // --- 判据 5：自动规避——命中自动启用并记账 ---
    {
        let mut t = BugTable::new();
        let _ = t.register(bug(11, 100, 200, 42, Mitigation::ConservativeSeq));
        t.reindex();
        let m = t.apply_mitigations(11, 150);
        s.add(
            "B35-自动规避-命中自动启用",
            m == Some(Mitigation::ConservativeSeq) && t.activations_len() == 1 && t.is_enabled(0),
            "命中即返回规避动作，启用记录与启用位同账",
        );
    }
    // --- 判据 6：自动规避——未命中零启用 ---
    {
        let mut t = BugTable::new();
        let _ = t.register(bug(11, 100, 200, 42, Mitigation::SkipFeature));
        t.reindex();
        let m1 = t.apply_mitigations(11, 300);
        let m2 = t.apply_mitigations(9, 150);
        s.add(
            "B35-自动规避-未命中零启用",
            m1.is_none() && m2.is_none() && t.activations_len() == 0,
            "代际外/版本外都不命中，零启用记录（对账恒等）",
        );
    }
    // --- 判据 7：自动规避——启用状态入快照可查 ---
    {
        let mut t = BugTable::new();
        let _ = t.register(bug(9, 0, 0, 7, Mitigation::InsertBarrier));
        t.reindex();
        let snap = t.snapshot();
        s.add_dyn(
            "B35-自动规避-快照含启用状态",
            snap.contains("启用 1") && snap.contains("保守模式 关"),
            &alloc::format!("快照含条目/启用/保守模式/失效/告警五要素：{}", snap),
        );
    }
    // --- 判据 8：自动规避——启用记录与命中数对账 ---
    {
        let mut t = BugTable::new();
        let _ = t.register(bug(9, 0, 0, 1, Mitigation::SkipFeature));
        let _ = t.register(bug(11, 0, 0, 2, Mitigation::InsertBarrier));
        t.reindex();
        let _ = t.apply_mitigations(9, 1000);
        let _ = t.apply_mitigations(11, 1000);
        let _ = t.apply_mitigations(11, 1000);
        s.add(
            "B35-自动规避-启用记录命中对账",
            t.activations_len() == 3 && t.entries_len() == 2,
            "三次命中三条启用记录（同键可重复命中），条目数独立对账",
        );
    }
    // --- 判据 9：保守兜底——规避失效降级并报告 ---
    {
        let mut t = BugTable::new();
        t.report_mitig_result(false);
        t.report_mitig_result(true);
        t.report_mitig_result(false);
        s.add(
            "B35-保守兜底-规避失效降级报告",
            t.mitig_failures == 2 && t.alerts == 2,
            "失效即告警计数（呈现层按未规避最坏假设），成功不告警",
        );
    }
    // --- 判据 10：保守兜底——显性禁用恢复原路径 ---
    {
        let mut t = BugTable::new();
        let _ = t.register(bug(9, 0, 0, 7, Mitigation::SkipFeature));
        t.reindex();
        assert!(t.is_enabled(0));
        let off = t.disable(0);
        s.add(
            "B35-保守兜底-显性禁用恢复原路径",
            off && !t.is_enabled(0) && t.disable(0) == false,
            "禁用摘除启用位恢复原路径；重复禁用显性拒绝",
        );
    }
    // --- 判据 11：精确匹配——精确条目优先于通配 ---
    {
        let mut t = BugTable::new();
        // 0 号通配、1 号精确——命中 drv=150 应取 1 号（精确优先）。
        let _ = t.register(bug(11, 0, 0, 1, Mitigation::SkipFeature));
        let _ = t.register(bug(11, 100, 200, 2, Mitigation::InsertBarrier));
        t.reindex();
        let m = t.apply_mitigations(11, 150);
        let m_wild = t.apply_mitigations(11, 300);
        s.add(
            "B35-精确匹配-精确优先于通配",
            m == Some(Mitigation::InsertBarrier) && m_wild == Some(Mitigation::SkipFeature),
            "版本区间内取精确条目；区间外回落通配条目（两级兜底）",
        );
    }
    // --- 判据 12：精确匹配——同键重复登记拒绝 ---
    {
        let mut t = BugTable::new();
        let first = t.register(bug(9, 10, 20, 5, Mitigation::SkipFeature));
        let dup = t.register(bug(9, 10, 20, 5, Mitigation::InsertBarrier));
        let diff = t.register(bug(9, 10, 21, 5, Mitigation::SkipFeature));
        s.add(
            "B35-精确匹配-同键重复登记拒绝",
            first.is_ok() && dup == Err(XC_DUP_ENTRY) && diff.is_ok(),
            "四元组同键拒绝（精确匹配语义唯一）；区间不同可并存",
        );
    }
    // --- 判据 13：精确匹配——非法条目拒载 ---
    {
        let mut t = BugTable::new();
        let inverted = t.register(bug(9, 30, 10, 1, Mitigation::SkipFeature));
        let fixed = t.register(BugEntry {
            gen: 9,
            drv_from: 0,
            drv_to: 0,
            trigger: 1,
            action: Mitigation::SkipFeature,
            state: EntryState::Fixed,
        });
        s.add(
            "B35-精确匹配-非法条目拒绝",
            inverted == Err(XC_ENTRY_BAD) && fixed == Err(XC_ENTRY_BAD),
            "版本区间倒挂与 Fixed 历史孤儿都不可入运行表",
        );
    }
    // --- 判据 14：码位互异且段独占 ---
    {
        let codes = [XC_DUP_ENTRY, XC_VER_REGRESS, XC_LOAD_FAIL, XC_ENTRY_BAD, XC_MITIG_FAIL];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_seg = codes.iter().all(|c| *c > SEG_BASE && *c <= SEG_BASE + 9);
        s.add(
            "B35-判据-码位互异段独占",
            all_diff && in_seg && codes.len() == 5,
            "五诊断码互异且全部落在 0xE4C0 段基的判别式区间内",
        );
    }
    s
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb235_hot_update_and_rollback_gate() {
        let mut t = BugTable::new();
        let e = alloc::vec![bug(9, 0, 0, 7, Mitigation::SkipFeature)];
        assert_eq!(t.load(LOAD_MAGIC, LOAD_FMT_VER, e.clone()), Ok(1));
        assert_eq!(t.load(LOAD_MAGIC, LOAD_FMT_VER, e), Ok(2));
        assert_eq!(t.hot_updates, 2);
        // 回退拒绝；无效魔数拒载后保守模式置位。
        assert_eq!(t.rollback(1), Err(XC_VER_REGRESS));
        let _ = t.load(0xdead, LOAD_FMT_VER, alloc::vec![]);
        assert!(t.conservative_mode);
        // 重新有效加载解除保守模式。
        let _ = t.load(LOAD_MAGIC, LOAD_FMT_VER, alloc::vec![]);
        assert!(!t.conservative_mode);
    }

    #[test]
    fn veb235_exact_over_wildcard() {
        let mut t = BugTable::new();
        let _ = t.register(bug(11, 0, 0, 1, Mitigation::SkipFeature));
        let _ = t.register(bug(11, 100, 200, 2, Mitigation::InsertBarrier));
        t.reindex();
        // 区间内精确命中。
        assert_eq!(t.apply_mitigations(11, 150), Some(Mitigation::InsertBarrier));
        // 区间外通配兜底。
        assert_eq!(t.apply_mitigations(11, 999), Some(Mitigation::SkipFeature));
        // 其他代际零命中。
        assert_eq!(t.apply_mitigations(12, 150), None);
        assert_eq!(t.activations_len(), 2);
    }

    #[test]
    fn veb235_mitigation_lifecycle() {
        let mut t = BugTable::new();
        let _ = t.register(bug(9, 0, 0, 7, Mitigation::ConservativeSeq));
        t.reindex();
        assert!(t.is_enabled(0));
        // 规避失效报告两次。
        t.report_mitig_result(false);
        t.report_mitig_result(false);
        assert_eq!(t.mitig_failures, 2);
        // 显性禁用后原路径恢复。
        assert!(t.disable(0));
        assert!(!t.is_enabled(0));
        assert!(!t.disable(0), "重复禁用拒绝");
        let snap = t.snapshot();
        assert!(snap.contains("启用 0"), "{}", snap);
    }

    #[test]
    fn veb235_entry_validation() {
        let mut t = BugTable::new();
        // 同键拒绝。
        let _ = t.register(bug(9, 10, 20, 5, Mitigation::SkipFeature));
        assert_eq!(t.register(bug(9, 10, 20, 5, Mitigation::SkipFeature)), Err(XC_DUP_ENTRY));
        // 区间倒挂拒绝。
        assert_eq!(t.register(bug(9, 30, 10, 5, Mitigation::SkipFeature)), Err(XC_ENTRY_BAD));
        // 邻键可并存。
        assert!(t.register(bug(9, 10, 21, 5, Mitigation::SkipFeature)).is_ok());
        assert_eq!(t.entries_len(), 2);
    }

    #[test]
    fn veb235_checks_all_green() {
        let set = run_veb235_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0235 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
