//! VE-F0042 · 资源初始化模式语义（VE-A 域 · 资源段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0042`
//!
//! **职责定位（锚点原文）**：资源初始化的语义明确化（清除/复制/未初始化
//! 三种模式显式声明），未初始化使用的检测（声明未初始化又直接读取 = 缺陷）。
//!
//! **数据结构（锚点原文）**：语义声明；检测器。三模式 = [`InitSemMode`]
//! （无 `Default`），语义账本 = [`ResourceInitLedger`]，条目 =
//! [`InitLedEntry`]，统计账 = [`InitLedStats`]；安全默认入口 =
//! [`ResourceInitLedger::declare_default`]（显性返回 [`InitSemMode::Cleared`]
//! 并记账「安全默认」，不是隐式默认）。
//!
//! **判据（锚点原文）**：三模式声明、未初始化检测、安全默认、混用拒绝、判据。
//!
//! 本模块 13 条判据逐条映射：**三模式声明 4 条**（枚举完备且短名互异／声明
//! 入账可查询／declare_default 显性返回 Cleared 并记账安全默认／已声明资源
//! 走 declare_default 返回原模式不再记账）、**未初始化检测 5 条**（未初始化
//! 先读阻断级拒绝／未初始化先写后读合法／Cleared 未清除先读违例且清除后
//! 放行／Copied 未复制先读违例且复制后放行／未声明资源读取显性拒绝）、
//! **混用拒绝 2 条**（同资源二次声明不同模式拒绝／同资源同模式重声明幂等
//! 放行）、**判据自身 2 条**（统计账与账本逐条对账／诊断码互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 未初始化读取 → **阻断级** | [`ResourceInitLedger::record_read`] 对「声明 [`InitSemMode::Uninitialized`] 又未先写就读」直接 [`CODE_UNINIT_READ`] 拒绝——读到的内容无语义、渲染结果不可信，这是阻断不是告警，不设降级通道 |
//! | 声明缺失 → **默认安全模式** | [`ResourceInitLedger::declare_default`] 显性返回 [`InitSemMode::Cleared`] 并在 [`InitLedStats::safe_defaults`] 记账——缺声明按「清除」处置是最保守的默认，且「走了默认」这件事本身入账可查，不是静默补位 |
//! | 模式混用 → **拒绝** | 同资源二次声明且模式不同 = [`CODE_MODE_CONFLICT`] 显性拒绝——同资源两种初始化语义并存即事实矛盾，账本拒绝记录矛盾事实 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **三模式无 `Default`**：[`InitSemMode`] 故意不实现 `Default`——「默认
//!   初始化模式」只能通过 [`ResourceInitLedger::declare_default`] 显性走，
//!   返回值 + 记账两件事同时发生。隐式默认会把「忘了声明」伪装成「声明了
//!   清除」，而显性安全默认把「忘了」留在账面上。
//! - **Cleared 的清除是动作不是声明**：声明 Cleared 只代表「将要清除」，
//!   读之前必须有 [`ResourceInitLedger::record_clear`]（或写入）动作入账；
//!   「声明了清除但还没清就读」同样是违例（[`CODE_READ_BEFORE_CLEAR`]）——
//!   语义声明与语义动作两本账分开记，缺一即拒。
//! - **Copied 同构**：声明 Copied 后必须有 [`ResourceInitLedger::record_copy`]
//!   动作（拷贝源内容入账）才可读——「复制」的语义是内容来源显式，来源
//!   没到位就读与未初始化读取本质相同。
//! - **先写后读是所有模式的合法通道**：[`record_write`] 对三模式一视同仁
//!   入账；Uninitialized 资源「先写后读」正是锚点点名的合法用法。
//! - **统计与 a11y**：阻断/违例/混用/未知各计数独立，[`a11y_summary`]
//!   一行读屏给出账本全貌——初始化状态读屏可达（锚点原文）。
//! - **零 panic 面**：查表走 `position`、计数全 `saturating_*`、所有操作
//!   返回 [`Result`]，资源号 0 与未声明资源显性拒绝不索引。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+format）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x1Cxx，独占——0x1C01..=0x1C07 整仓 grep 零占用后选定）
// ---------------------------------------------------------------------------

/// 阻断级：声明未初始化又直接读取（内容无语义，渲染结果不可信）。
pub const CODE_UNINIT_READ: u16 = 0x1C01;
/// Cleared 声明后未清除（且未写入）先读。
pub const CODE_READ_BEFORE_CLEAR: u16 = 0x1C02;
/// Copied 声明后未复制（且未写入）先读。
pub const CODE_READ_BEFORE_COPY: u16 = 0x1C03;
/// 模式混用：同资源二次声明且模式不同。
pub const CODE_MODE_CONFLICT: u16 = 0x1C04;
/// 账本查无此资源（从未声明）。
pub const CODE_RES_UNDECLARED: u16 = 0x1C05;
/// 资源号为 0（0 号保留给「无资源」，非法）。
pub const CODE_RES_ZERO: u16 = 0x1C06;
/// 清除动作落在非 Cleared 资源上（语义错位）。
pub const CODE_CLEAR_MISAPPLIED: u16 = 0x1C07;

// ---------------------------------------------------------------------------
// 二、三模式枚举（无 Default——安全默认只能显性走 declare_default）
// ---------------------------------------------------------------------------

/// 资源初始化三模式（锚点原文：显式声明，不给默认猜测空间——除「缺失」外）。
///
/// 无 `Default` 实现：隐式默认会把「忘了声明」伪装成「声明了清除」；
/// 显性安全默认（[`ResourceInitLedger::declare_default`]）把「忘了」留在
/// 账面上。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitSemMode {
    /// 清除：写入常量值（最安全；缺声明时的安全默认）。
    Cleared,
    /// 复制：从已有资源拷贝（内容来源显式，拷贝动作入账后才可读）。
    Copied,
    /// 未初始化：内容无语义（必须先写后读；先读即阻断级违例）。
    Uninitialized,
}

/// 模式总数（判据侧按字面量独立重算，不向本常量问答案）。
pub const INIT_SEM_MODE_COUNT: usize = 3;

impl InitSemMode {
    /// 全集（判据枚举完备性断言用）。
    pub const ALL: [InitSemMode; INIT_SEM_MODE_COUNT] = [
        InitSemMode::Cleared,
        InitSemMode::Copied,
        InitSemMode::Uninitialized,
    ];

    /// 短名（读屏与统计口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            InitSemMode::Cleared => "清除",
            InitSemMode::Copied => "复制",
            InitSemMode::Uninitialized => "未初始化",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、语义账本：声明 / 动作 / 读取判定
// ---------------------------------------------------------------------------

/// 账本条目：资源 × 声明模式 × 动作记账位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitLedEntry {
    /// 资源号（≥1，0 号非法）。
    pub resource: u32,
    /// 声明的初始化模式（声明后不可变——变更走混用拒绝）。
    pub mode: InitSemMode,
    /// 清除动作已入账（仅 Cleared 语义有效）。
    pub cleared: bool,
    /// 复制动作已入账（仅 Copied 语义有效）。
    pub copied: bool,
    /// 写入动作已入账（三模式通用；先写后读的合法通道）。
    pub written: bool,
    /// 本条目是否经 declare_default 产生（安全默认记账）。
    pub defaulted: bool,
}

/// 账本统计账（放行/阻断的全部依据都可从这里读出）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InitLedStats {
    /// 显性声明次数（declare 成功，含同模式重声明的幂等放行）。
    pub declared: usize,
    /// 安全默认次数（declare_default 实际补声明并记账的次数）。
    pub safe_defaults: usize,
    /// 同模式重声明幂等放行次数。
    pub redeclare_same: usize,
    /// 读取放行次数。
    pub reads_ok: usize,
    /// 读取阻断/违例次数（三码合计：阻断 + 未清先读 + 未复制先读）。
    pub blocked_reads: usize,
    /// 未声明资源读取拒绝次数。
    pub undeclared_reads: usize,
    /// 模式混用拒绝次数。
    pub conflicts: usize,
    /// 清除动作错位拒绝次数。
    pub clear_misapplied: usize,
}

/// 资源初始化语义账本：declare / record_clear / record_copy / record_write /
/// record_read 五个动作，读取判定按声明模式三分支。
#[derive(Clone, Debug)]
pub struct ResourceInitLedger {
    /// 账本条目（按声明序；查表线性扫——资源数 O(资源) 级语义账，不上哈希）。
    entries: Vec<InitLedEntry>,
    /// 统计账。
    pub stats: InitLedStats,
}

impl ResourceInitLedger {
    /// 新建空账本。
    pub fn new() -> ResourceInitLedger {
        ResourceInitLedger { entries: Vec::new(), stats: InitLedStats::default() }
    }

    /// 查条目下标（内部工具）。
    fn pos(&self, resource: u32) -> Option<usize> {
        self.entries.iter().position(|e| e.resource == resource)
    }

    /// 显性声明初始化模式。
    ///
    /// 资源号 0 拒绝；同资源二次声明模式不同 = 模式混用显性拒绝；
    /// 同模式重声明幂等放行并计数（不重复建条目）。
    pub fn declare(&mut self, resource: u32, mode: InitSemMode) -> Result<(), u16> {
        if resource == 0 {
            return Err(CODE_RES_ZERO);
        }
        if let Some(i) = self.pos(resource) {
            if self.entries[i].mode != mode {
                self.stats.conflicts = self.stats.conflicts.saturating_add(1);
                return Err(CODE_MODE_CONFLICT);
            }
            self.stats.redeclare_same = self.stats.redeclare_same.saturating_add(1);
            return Ok(());
        }
        self.entries.push(InitLedEntry {
            resource,
            mode,
            cleared: false,
            copied: false,
            written: false,
            defaulted: false,
        });
        self.stats.declared = self.stats.declared.saturating_add(1);
        Ok(())
    }

    /// 安全默认声明（锚点：声明缺失 → 默认安全模式）。
    ///
    /// 未声明资源：显性补 [`InitSemMode::Cleared`] 条目并记账「安全默认」；
    /// 已声明资源：返回其现有模式、不补账——默认只对「缺失」发生。
    pub fn declare_default(&mut self, resource: u32) -> Result<InitSemMode, u16> {
        if resource == 0 {
            return Err(CODE_RES_ZERO);
        }
        if let Some(i) = self.pos(resource) {
            return Ok(self.entries[i].mode);
        }
        self.entries.push(InitLedEntry {
            resource,
            mode: InitSemMode::Cleared,
            cleared: false,
            copied: false,
            written: false,
            defaulted: true,
        });
        self.stats.safe_defaults = self.stats.safe_defaults.saturating_add(1);
        Ok(InitSemMode::Cleared)
    }

    /// 清除动作入账（仅 Cleared 语义有效；错位显性拒绝）。
    pub fn record_clear(&mut self, resource: u32) -> Result<(), u16> {
        let i = match self.pos(resource) {
            Some(i) => i,
            None => return Err(CODE_RES_UNDECLARED),
        };
        if self.entries[i].mode != InitSemMode::Cleared {
            self.stats.clear_misapplied = self.stats.clear_misapplied.saturating_add(1);
            return Err(CODE_CLEAR_MISAPPLIED);
        }
        self.entries[i].cleared = true;
        Ok(())
    }

    /// 复制动作入账（仅 Copied 语义有效；错位显性拒绝）。
    pub fn record_copy(&mut self, resource: u32) -> Result<(), u16> {
        let i = match self.pos(resource) {
            Some(i) => i,
            None => return Err(CODE_RES_UNDECLARED),
        };
        if self.entries[i].mode != InitSemMode::Copied {
            self.stats.clear_misapplied = self.stats.clear_misapplied.saturating_add(1);
            return Err(CODE_CLEAR_MISAPPLIED);
        }
        self.entries[i].copied = true;
        Ok(())
    }

    /// 写入动作入账（三模式通用；先写后读的合法通道）。
    pub fn record_write(&mut self, resource: u32) -> Result<(), u16> {
        let i = match self.pos(resource) {
            Some(i) => i,
            None => return Err(CODE_RES_UNDECLARED),
        };
        self.entries[i].written = true;
        Ok(())
    }

    /// 读取判定（锚点主判据：未初始化使用检测）。
    ///
    /// - 未声明资源 → [`CODE_RES_UNDECLARED`]（调用方可先走 declare_default）；
    /// - Uninitialized 且未写 → [`CODE_UNINIT_READ`] **阻断级**；
    /// - Cleared 且未清除未写 → [`CODE_READ_BEFORE_CLEAR`]；
    /// - Copied 且未复制未写 → [`CODE_READ_BEFORE_COPY`]；
    /// - 其余放行并计数。
    pub fn record_read(&mut self, resource: u32) -> Result<(), u16> {
        let i = match self.pos(resource) {
            Some(i) => i,
            None => {
                self.stats.undeclared_reads = self.stats.undeclared_reads.saturating_add(1);
                return Err(CODE_RES_UNDECLARED);
            }
        };
        let e = self.entries[i];
        let blocked = match e.mode {
            InitSemMode::Uninitialized => {
                if e.written {
                    None
                } else {
                    Some(CODE_UNINIT_READ)
                }
            }
            InitSemMode::Cleared => {
                if e.cleared || e.written {
                    None
                } else {
                    Some(CODE_READ_BEFORE_CLEAR)
                }
            }
            InitSemMode::Copied => {
                if e.copied || e.written {
                    None
                } else {
                    Some(CODE_READ_BEFORE_COPY)
                }
            }
        };
        match blocked {
            Some(code) => {
                self.stats.blocked_reads = self.stats.blocked_reads.saturating_add(1);
                Err(code)
            }
            None => {
                self.stats.reads_ok = self.stats.reads_ok.saturating_add(1);
                Ok(())
            }
        }
    }

    /// 查条目快照（只读，判据与读屏用）。
    pub fn entry(&self, resource: u32) -> Option<InitLedEntry> {
        let i = self.pos(resource)?;
        Some(self.entries[i])
    }

    /// 条目总数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 账本是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 读屏摘要（锚点：声明状态读屏可达——一行账本全貌）。
    pub fn a11y_summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "初始化账：声明 {}（安全默认 {}），读放行 {}，阻断违例 {}，混用拒绝 {}",
            self.stats.declared,
            self.stats.safe_defaults,
            self.stats.reads_ok,
            self.stats.blocked_reads,
            self.stats.conflicts
        ));
        for e in self.entries.iter() {
            let tag = if e.defaulted { "（安全默认）" } else { "" };
            lines.push(format!("资源 {}：{}{}", e.resource, e.mode.as_str(), tag));
        }
        lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
// 四、判据（run_vea42_checks —— 13 条，逐条映射锚点五条判据）
// ---------------------------------------------------------------------------

/// VE-F0042 域自检：三模式声明 4 + 未初始化检测 5 + 混用拒绝 2 + 判据自身 2。
pub fn run_vea42_checks() -> CheckSet {
    let mut s = CheckSet::new("vea42_initsem");

    // --- 判据 1.1：三模式声明——枚举完备且三短名互异 ---
    {
        let (a, b, c) = (
            InitSemMode::Cleared.as_str(),
            InitSemMode::Copied.as_str(),
            InitSemMode::Uninitialized.as_str(),
        );
        s.add(
            "A42-三态-枚举完备三短名互异",
            InitSemMode::ALL.len() == 3 && a != b && a != c && b != c && INIT_SEM_MODE_COUNT == 3,
            "三模式全集三变体、读屏短名两两互异——短名相等的两态在读屏口径里就是同一态",
        );
    }

    // --- 判据 1.2：三模式声明——声明入账可查询 ---
    {
        let mut l = ResourceInitLedger::new();
        let r1 = l.declare(3, InitSemMode::Uninitialized);
        let r2 = l.declare(4, InitSemMode::Cleared);
        let r3 = l.declare(5, InitSemMode::Copied);
        let ok = r1.is_ok()
            && r2.is_ok()
            && r3.is_ok()
            && l.entry(3).map(|e| e.mode) == Some(InitSemMode::Uninitialized)
            && l.entry(4).map(|e| e.mode) == Some(InitSemMode::Cleared)
            && l.entry(5).map(|e| e.mode) == Some(InitSemMode::Copied)
            && l.stats.declared == 3
            && l.len() == 3;
        s.add(
            "A42-三态-声明入账可查询",
            ok,
            "三资源各声明一模式：条目模式逐个可查、声明计数 3——声明是账面事实不是内存约定",
        );
    }

    // --- 判据 1.3：安全默认——declare_default 显性返回 Cleared 并记账 ---
    {
        let mut l = ResourceInitLedger::new();
        let mode = l.declare_default(9);
        let ok = match mode {
            Ok(m) => {
                m == InitSemMode::Cleared
                    && l.entry(9).map(|e| e.mode) == Some(InitSemMode::Cleared)
                    && l.entry(9).map(|e| e.defaulted) == Some(true)
                    && l.stats.safe_defaults == 1
            }
            Err(_) => false,
        };
        s.add(
            "A42-默认-declare_default显性返回Cleared并记账",
            ok,
            "缺声明按「清除」处置：返回值是 Cleared、条目标 defaulted、安全默认计数 1——默认是显性事件",
        );
    }

    // --- 判据 1.4：安全默认——已声明资源走 declare_default 返回原模式不补账 ---
    {
        let mut l = ResourceInitLedger::new();
        let _ = l.declare(7, InitSemMode::Copied);
        let mode = l.declare_default(7);
        let ok = mode == Ok(InitSemMode::Copied)
            && l.stats.safe_defaults == 0
            && l.len() == 1
            && l.entry(7).map(|e| e.defaulted) == Some(false);
        s.add(
            "A42-默认-已声明资源走默认入口不补账",
            ok,
            "默认只对「缺失」发生：已声明 Copied 的资源走 declare_default 原样返回 Copied，安全默认计数不动",
        );
    }

    // --- 判据 2.1：未初始化检测——未初始化先读阻断级拒绝 ---
    {
        let mut l = ResourceInitLedger::new();
        let _ = l.declare(1, InitSemMode::Uninitialized);
        let r = l.record_read(1);
        s.add(
            "A42-检测-未初始化先读阻断级拒绝",
            r == Err(CODE_UNINIT_READ) && l.stats.blocked_reads == 1 && l.stats.reads_ok == 0,
            "声明未初始化又直接读取 = 缺陷：CODE_UNINIT_READ 阻断，无降级通道",
        );
    }

    // --- 判据 2.2：未初始化检测——未初始化先写后读合法 ---
    {
        let mut l = ResourceInitLedger::new();
        let _ = l.declare(1, InitSemMode::Uninitialized);
        let w = l.record_write(1);
        let r = l.record_read(1);
        s.add(
            "A42-检测-未初始化先写后读合法",
            w.is_ok() && r.is_ok() && l.stats.reads_ok == 1 && l.stats.blocked_reads == 0,
            "先写后读正是未初始化声明的合法用法：写入入账后读取放行",
        );
    }

    // --- 判据 2.3：未初始化检测——Cleared 未清除先读违例且清除后放行 ---
    {
        let mut l = ResourceInitLedger::new();
        let _ = l.declare(2, InitSemMode::Cleared);
        let early = l.record_read(2);
        let c = l.record_clear(2);
        let late = l.record_read(2);
        s.add(
            "A42-检测-Cleared未清除先读违例清除后放行",
            early == Err(CODE_READ_BEFORE_CLEAR) && c.is_ok() && late.is_ok(),
            "声明清除只是承诺：清除动作入账前读取违例、入账后放行——语义动作与声明两本账",
        );
    }

    // --- 判据 2.4：未初始化检测——Copied 未复制先读违例且复制后放行 ---
    {
        let mut l = ResourceInitLedger::new();
        let _ = l.declare(6, InitSemMode::Copied);
        let early = l.record_read(6);
        let cp = l.record_copy(6);
        let late = l.record_read(6);
        s.add(
            "A42-检测-Copied未复制先读违例复制后放行",
            early == Err(CODE_READ_BEFORE_COPY) && cp.is_ok() && late.is_ok(),
            "复制语义 = 内容来源显式：来源没到位就读与未初始化读取本质相同",
        );
    }

    // --- 判据 2.5：未初始化检测——未声明资源读取显性拒绝（可走安全默认补救） ---
    {
        let mut l = ResourceInitLedger::new();
        let r = l.record_read(42);
        let after = l.declare_default(42);
        let recovered = l.record_clear(42);
        let ok_read = l.record_read(42);
        s.add(
            "A42-检测-未声明资源读取显性拒绝且可走安全默认补救",
            r == Err(CODE_RES_UNDECLARED)
                && after == Ok(InitSemMode::Cleared)
                && recovered.is_ok()
                && ok_read.is_ok()
                && l.stats.undeclared_reads == 1,
            "查无此资源显性拒绝并指路 declare_default：补救后读取放行、拒绝计数保留",
        );
    }

    // --- 判据 3.1：混用拒绝——同资源二次声明不同模式显性拒绝 ---
    {
        let mut l = ResourceInitLedger::new();
        let _ = l.declare(8, InitSemMode::Cleared);
        let clash = l.declare(8, InitSemMode::Uninitialized);
        let clash2 = l.declare(8, InitSemMode::Copied);
        s.add(
            "A42-混用-同资源二次声明不同模式显性拒绝",
            clash == Err(CODE_MODE_CONFLICT)
                && clash2 == Err(CODE_MODE_CONFLICT)
                && l.entry(8).map(|e| e.mode) == Some(InitSemMode::Cleared)
                && l.stats.conflicts == 2,
            "同资源两种初始化语义并存即事实矛盾：账本拒绝记录矛盾，原声明不被覆写",
        );
    }

    // --- 判据 3.2：混用拒绝——同资源同模式重声明幂等放行 ---
    {
        let mut l = ResourceInitLedger::new();
        let _ = l.declare(8, InitSemMode::Cleared);
        let again = l.declare(8, InitSemMode::Cleared);
        let ok = again.is_ok()
            && l.stats.redeclare_same == 1
            && l.stats.declared == 1
            && l.len() == 1;
        s.add(
            "A42-混用-同资源同模式重声明幂等放行",
            ok,
            "同模式重声明不新建条目只计数：混用拒绝只针对模式变化，幂等重申不是冲突",
        );
    }

    // --- 判据 4.1：判据自身——统计账与调用序列逐值对账 ---
    {
        let mut l = ResourceInitLedger::new();
        let _ = l.declare_default(1); // 安全默认 1
        let _ = l.declare(2, InitSemMode::Uninitialized); // 声明 1
        let _ = l.record_read(1); // 放行（Cleared 默认未清？否——defaulted 条目 cleared=false
        // 会被拦！判据侧独立预期：Cleared 未清先读 = 违例 1
        let _ = l.declare(3, InitSemMode::Copied);
        let _ = l.record_copy(3);
        let _ = l.record_read(3); // 放行
        let _ = l.record_read(2); // 阻断 1
        let _ = l.record_read(99); // 未声明拒绝 1
        let _ = l.declare(2, InitSemMode::Cleared); // 混用 1
        s.add(
            "A42-判据-统计账与调用序列逐值对账",
            l.stats.safe_defaults == 1
                && l.stats.declared == 2
                && l.stats.reads_ok == 1
                && l.stats.blocked_reads == 2
                && l.stats.undeclared_reads == 1
                && l.stats.conflicts == 1,
            "九次调用七类结局逐值入账：默认 1 / 声明 2 / 放行 1 / 违例 2 / 未声明 1 / 混用 1",
        );
    }

    // --- 判据 4.2：判据自身——诊断码互异且段独占（0x1C 段七码全对拍） ---
    {
        let codes = [
            CODE_UNINIT_READ,
            CODE_READ_BEFORE_CLEAR,
            CODE_READ_BEFORE_COPY,
            CODE_MODE_CONFLICT,
            CODE_RES_UNDECLARED,
            CODE_RES_ZERO,
            CODE_CLEAR_MISAPPLIED,
        ];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0x1C00);
        s.add(
            "A42-判据-诊断码互异且段独占",
            all_diff && in_segment && codes.len() == 7,
            "0x1C01..=0x1C07 七码两两互异、高字节全部落在 0x1C 独占段",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 五、单元测试（cargo test svstar2）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea42_uninit_lifecycle_write_then_read() {
        let mut l = ResourceInitLedger::new();
        assert!(l.declare(1, InitSemMode::Uninitialized).is_ok());
        assert_eq!(l.record_read(1), Err(CODE_UNINIT_READ), "先读阻断");
        assert!(l.record_write(1).is_ok());
        assert!(l.record_read(1).is_ok(), "先写后读合法");
        assert!(l.record_write(1).is_ok());
        assert!(l.record_read(1).is_ok(), "多次写读均合法");
        assert_eq!(l.stats.reads_ok, 2);
        assert_eq!(l.stats.blocked_reads, 1);
    }

    #[test]
    fn vea42_safe_default_and_recovery() {
        let mut l = ResourceInitLedger::new();
        assert_eq!(l.record_read(50), Err(CODE_RES_UNDECLARED));
        assert_eq!(l.declare_default(50), Ok(InitSemMode::Cleared));
        assert_eq!(l.record_read(50), Err(CODE_READ_BEFORE_CLEAR), "默认 Cleared 也要先清");
        assert!(l.record_clear(50).is_ok());
        assert!(l.record_read(50).is_ok());
        // 已声明资源走默认入口：原样返回不补账
        assert_eq!(l.declare_default(50), Ok(InitSemMode::Cleared));
        assert_eq!(l.stats.safe_defaults, 1);
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn vea42_mode_conflict_and_zero_resource() {
        let mut l = ResourceInitLedger::new();
        assert_eq!(l.declare(0, InitSemMode::Cleared), Err(CODE_RES_ZERO));
        assert_eq!(l.declare_default(0), Err(CODE_RES_ZERO));
        assert!(l.declare(5, InitSemMode::Copied).is_ok());
        assert_eq!(l.declare(5, InitSemMode::Uninitialized), Err(CODE_MODE_CONFLICT));
        assert!(l.declare(5, InitSemMode::Copied).is_ok(), "同模式重申幂等");
        assert_eq!(l.record_clear(5), Err(CODE_CLEAR_MISAPPLIED), "清除落在 Copied 上错位");
        assert!(l.record_copy(5).is_ok());
        assert!(l.record_read(5).is_ok());
        assert_eq!(l.stats.conflicts, 1);
        assert_eq!(l.stats.clear_misapplied, 1);
    }

    #[test]
    fn vea42_a11y_summary_lines() {
        let mut l = ResourceInitLedger::new();
        let _ = l.declare_default(1);
        let _ = l.declare(2, InitSemMode::Uninitialized);
        let text = l.a11y_summary();
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), 3, "读屏行数 = 1 总行 + 2 条目");
        assert!(text.contains("安全默认 1"), "总行带安全默认计数：{}", text);
        assert!(text.contains("资源 1：清除（安全默认）"), "默认条目显性标注：{}", text);
        assert!(text.contains("资源 2：未初始化"), "条目行带模式短名：{}", text);
    }

    #[test]
    fn vea42_checks_all_green() {
        let set = run_vea42_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0042 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
