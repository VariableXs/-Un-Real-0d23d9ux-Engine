//! VE-F0037 · 深度模板视图管理（VE-A 域 · 资源视图段 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0037`
//!
//! **职责定位（锚点原文）**：DSV 的统一管理（读写模式声明 / 只读深度约束），
//! 只读模式违例检测（声明只读又写入 = 缺陷）；含只读深度的并发安全声明
//! （只读可并发、写入互斥）。
//!
//! **数据结构（锚点原文）**：深度模板视图管理。在 [`DsvMgr`] 上一一落位：
//! 模式全集 = [`DsvMode`]（ReadWrite / ReadOnlyDepth / ReadOnlyAll 三档）、
//! 条目 = [`DsvEntry`]（键 + 模式 + 写入计数）、键 = [`DsvKey`]、
//! 违例台账 = [`DsvViolation`] 的 `Vec`、统计账 = [`DsvStats`]。
//!
//! **判据（锚点原文）**：读写声明、只读约束、违例检测、参数校验、判据。
//!
//! 本模块 14 条判据逐条映射：**读写声明 3 条**（注册产出合法 id／同键同模式
//! 重声明共享同 id／同键异模式重声明冲突拒绝）、**只读约束 3 条**（只读深度
//! 声明下写入显性阻断／只读全部声明下写入同样阻断／读写模式写入放行且计数
//! 递增）、**违例检测 2 条**（并发矩阵只读可并发写互斥——判据侧按 3×3 全对拍
//! 独立重算／违例台账阻断级逐条可读）、**参数校验 3 条**（键校验资源号零
//! 拒绝／未知 id 与失效悬垂 id 分码拒绝／缓存重建后旧句柄走缓存失效码）、
//! **判据自身 3 条**（缓存重建清空且重注册可用／统计账与台账逐条对账／
//! 诊断码互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 只读违例 → **阻断级** | [`DsvMgr::declare_write`] 对只读条目直接拒绝并返回 [`CODE_DSV_READONLY_VIOLATION`]，违例事实入 [`DsvMgr::blocking_violations`] 台账——「声明只读又写入」这个缺陷不降级、不放行、不静默吞掉，写操作在管理器层面就没有通道 |
//! | 缓存失效 → **重建** | [`DsvMgr::cache_rebuild`] 全表清空、全部旧 id 入**缓存失效墓碑**——重建不是「修数据」而是「扔掉可疑缓存」，旧句柄后续操作显性走 [`CODE_DSV_CACHE_STALE`]，与普通悬垂分码可查 |
//! | 参数越界 → **拒绝** | 键校验（资源号 0）与查表（未知 id / 悬垂 id / 缓存失效 id）三道红线各自独立触发、分码返回（[`CODE_DSV_BAD_KEY`] / [`CODE_DSV_ENTRY_UNKNOWN`] / [`CODE_DSV_STALE_ID`] / [`CODE_DSV_CACHE_STALE`]）——参数问题显性拒绝，绝无静默回退 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **模式三档而不是两档**：`ReadWrite / ReadOnlyDepth / ReadOnlyAll` 三档
//!   里 `ReadOnlyDepth` 与 `ReadOnlyAll` 的差异是模板测试（stencil test）
//!   的读写口径——只读深度仍可能写模板。本模块按 D3D 语义保守处理：两个
//!   只读档都禁 `declare_write`（模板侧写入由后续卡以独立通道声明），判据
//!   对两档分别独立验证，档位合并时先红。
//! - **并发矩阵是模式对的纯函数**：[`DsvMode::concurrent_with`] 只看两个
//!   模式是否都不可写——只读 × 只读放行（并发安全声明的本体）、任何一侧
//!   可写即互斥。判据不调用矩阵再问矩阵答案，而是按字面量把 3×3 期望表
//!   重写一遍逐格对拍（判据 3.1）。
//! - **同键异模式是冲突不是覆写**：同一键第二次注册若模式不同，静默覆写
//!   会让「先声明只读后声明读写」绕过只读约束。本模块显性拒绝
//!   （[`CODE_DSV_MODE_CLASH`]）——模式是键的属性的一部分，改模式必须先失效。
//! - **墓碑带码**：失效途径有两条（显性 invalidate / 缓存重建），旧句柄
//!   撞上哪条途径决定返回哪个码——诊断时不只知道「句柄死了」，还知道
//!   「怎么死的」。
//! - **零 panic 面**：查表走 `iter().position`、计数全 `saturating_*`、
//!   所有操作返回 [`Result`]，违例只记账不炸栈——内核态没有 panic 的余地。
//! - **判据侧独立重算**：并发矩阵、墓碑对账、码段断言全部在判据里按
//!   字面量重写，不向被测函数问答案——被测逻辑被误改时判据自身先红。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+ToString）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x15xx，独占——整仓 grep 0x1501 零占用后选定）
// ---------------------------------------------------------------------------

/// 深度模板视图键非法（资源号为 0）。
pub const CODE_DSV_BAD_KEY: u16 = 0x1501;
/// 只读模式违例：声明只读又写入（阻断级缺陷）。
pub const CODE_DSV_READONLY_VIOLATION: u16 = 0x1502;
/// 条目 id 查无此条（从未注册）。
pub const CODE_DSV_ENTRY_UNKNOWN: u16 = 0x1503;
/// 悬垂 id：条目曾被显性失效，旧句柄永久作废。
pub const CODE_DSV_STALE_ID: u16 = 0x1504;
/// 同键异模式重声明冲突（模式是键属性的一部分，改模式必须先失效）。
pub const CODE_DSV_MODE_CLASH: u16 = 0x1505;
/// 缓存已重建：旧句柄随整表清空而失效（与普通悬垂分码）。
pub const CODE_DSV_CACHE_STALE: u16 = 0x1506;

// ---------------------------------------------------------------------------
// 二、读写模式：三档全集与并发矩阵
// ---------------------------------------------------------------------------

/// DSV 读写模式三档。新增档位必须：加变体 + 进 [`ALL`] + 抬
/// [`MODE_REGISTRY_LEN`] + 抬判据 3.1 的字面量期望矩阵。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DsvMode {
    /// 读写：深度可读可写（常规渲染路径）。
    ReadWrite,
    /// 只读深度：深度只读（模板侧口径另管，本模块保守同禁写）。
    ReadOnlyDepth,
    /// 只读全部：深度与模板都只读。
    ReadOnlyAll,
}

/// 模式全集长度（判据枚举完备性断言用：新档位不改此常量即先红）。
pub const MODE_REGISTRY_LEN: usize = 3;

impl DsvMode {
    /// 全集（判据完备性断言用；新变体必须进此表）。
    pub const ALL: [DsvMode; 3] =
        [DsvMode::ReadWrite, DsvMode::ReadOnlyDepth, DsvMode::ReadOnlyAll];

    /// 短名（读屏与统计口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            DsvMode::ReadWrite => "读写",
            DsvMode::ReadOnlyDepth => "只读深度",
            DsvMode::ReadOnlyAll => "只读全部",
        }
    }

    /// 可写性：只有读写档可写。只读约束的唯一事实源。
    pub const fn writable(self) -> bool {
        matches!(self, DsvMode::ReadWrite)
    }

    /// 并发安全声明（锚点主判据）：只读可并发、写入互斥。
    ///
    /// 两个只读条目可并发放行（并发安全声明）；任何一侧可写即互斥。
    /// 纯函数、只看模式对，不看条目身份——同一模式的并发性处处一致。
    pub const fn concurrent_with(self, other: DsvMode) -> bool {
        !self.writable() && !other.writable()
    }
}

// ---------------------------------------------------------------------------
// 三、键、条目、违例台账
// ---------------------------------------------------------------------------

/// DSV 键：资源 + mip + slice 三元组（同键同模式即同一视图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DsvKey {
    /// 资源号（0 号保留给「无资源」，非法）。
    pub resource: u32,
    /// mip 层。
    pub mip: u8,
    /// 数组切片号。
    pub slice: u16,
}

impl DsvKey {
    /// 键校验：资源号非零。参数越界第一道红线。
    pub fn validate(&self) -> Result<(), u16> {
        if self.resource == 0 {
            return Err(CODE_DSV_BAD_KEY);
        }
        Ok(())
    }
}

/// DSV 条目：键 + 模式 + 写入计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DsvEntry {
    /// 条目 id（管理器内单调递增，永不复用）。
    pub id: u32,
    /// 视图键。
    pub key: DsvKey,
    /// 读写模式（注册期声明，注册后不可变——改模式必须先失效）。
    pub mode: DsvMode,
    /// 放行的写入次数（只读条目恒为 0）。
    pub writes: u32,
    /// 放行的读取次数（只读约束只管写，读放行并记账）。
    pub reads: u32,
}

/// 违例台账条目：阻断级缺陷的逐条记录（锚点：声明只读又写入 = 缺陷）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DsvViolation {
    /// 违例条目 id。
    pub id: u32,
    /// 违例诊断码（恒为 [`CODE_DSV_READONLY_VIOLATION`]）。
    pub code: u16,
    /// 违例时条目的模式快照（读屏口径：哪一档只读被撞了）。
    pub mode: DsvMode,
}

// ---------------------------------------------------------------------------
// 四、统计账
// ---------------------------------------------------------------------------

/// DSV 管理统计账（放行/阻断的全部依据都可从这里读出）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DsvStats {
    /// 当前存活条目数。
    pub live: usize,
    /// 实际注册次数（新建条目）。
    pub registered: usize,
    /// 同键同模式共享命中次数。
    pub shares: usize,
    /// 阻断的写入尝试次数（只读违例）。
    pub blocked_writes: usize,
    /// 放行的写入总次数。
    pub passed_writes: usize,
    /// 放行的读取总次数。
    pub passed_reads: usize,
    /// 显性失效次数。
    pub invalidated: usize,
    /// 缓存重建次数。
    pub rebuilds: usize,
    /// 拒绝次数（键非法 / 未知 id / 悬垂 / 模式冲突 / 缓存失效）。
    pub rejects: usize,
}

// ---------------------------------------------------------------------------
// 五、DSV 管理器本体
// ---------------------------------------------------------------------------

/// DSV 统一管理器：注册（读写模式声明）/ 写入放行（只读违例阻断）/
/// 失效与重建三段。
#[derive(Clone, Debug)]
pub struct DsvMgr {
    /// 存活条目（按注册序；失效即物理移除，不留活尸）。
    entries: Vec<DsvEntry>,
    /// 失效墓碑（id + 失效途径码：判据的分码悬垂判定依据）。
    tombstones: Vec<(u32, u16)>,
    /// 违例台账（阻断级，只增不清——清账是后续卡的事，本模块只如实记）。
    ledger: Vec<DsvViolation>,
    /// 下一个条目 id（从 1 起，0 号保留给「无视图」）。
    next_id: u32,
    /// 统计账。
    pub stats: DsvStats,
}

impl DsvMgr {
    /// 新建管理器（空表、零统计）。
    pub fn new() -> DsvMgr {
        DsvMgr {
            entries: Vec::new(),
            tombstones: Vec::new(),
            ledger: Vec::new(),
            next_id: 1,
            stats: DsvStats::default(),
        }
    }

    /// 注册条目（三段之一）：键校验 → 同键同模式共享 / 同键异模式冲突 /
    /// 新建。模式在注册期声明，注册后经本接口不可变。
    pub fn register(&mut self, key: DsvKey, mode: DsvMode) -> Result<u32, u16> {
        // 注册期第一道红线：键非法记入拒绝账（显性拒绝不静默）。
        if let Err(e) = key.validate() {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            return Err(e);
        }
        if let Some(e) = self.entries.iter().find(|e| e.key == key) {
            if e.mode == mode {
                // 同键同模式：共享同一 id（去重共享，与 F0036 同口径）
                self.stats.shares = self.stats.shares.saturating_add(1);
                return Ok(e.id);
            }
            // 同键异模式：冲突拒绝（改模式必须先失效，见模块文档设计要点）
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            return Err(CODE_DSV_MODE_CLASH);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.entries.push(DsvEntry { id, key, mode, writes: 0, reads: 0 });
        self.stats.registered = self.stats.registered.saturating_add(1);
        self.stats.live = self.entries.len();
        Ok(id)
    }

    /// 声明一次写入（只读违例检测的主通道）。
    ///
    /// 读写条目：放行并递增写入计数；只读条目：**阻断级**——写入拒绝、
    /// 违例入台账、统计入账，返回 [`CODE_DSV_READONLY_VIOLATION`]。
    pub fn declare_write(&mut self, id: u32) -> Result<u32, u16> {
        let pos = match self.entry_pos(id) {
            Some(p) => p,
            None => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                return Err(self.stale_code_for(id, CODE_DSV_ENTRY_UNKNOWN));
            }
        };
        if !self.entries[pos].mode.writable() {
            let mode = self.entries[pos].mode;
            self.ledger.push(DsvViolation { id, code: CODE_DSV_READONLY_VIOLATION, mode });
            self.stats.blocked_writes = self.stats.blocked_writes.saturating_add(1);
            return Err(CODE_DSV_READONLY_VIOLATION);
        }
        self.entries[pos].writes = self.entries[pos].writes.saturating_add(1);
        self.stats.passed_writes = self.stats.passed_writes.saturating_add(1);
        Ok(self.entries[pos].writes)
    }

    /// 声明一次读取：三种模式都放行（只读约束只管写），记账可审计。
    pub fn declare_read(&mut self, id: u32) -> Result<u32, u16> {
        let pos = match self.entry_pos(id) {
            Some(p) => p,
            None => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                return Err(self.stale_code_for(id, CODE_DSV_ENTRY_UNKNOWN));
            }
        };
        self.entries[pos].reads = self.entries[pos].reads.saturating_add(1);
        self.stats.passed_reads = self.stats.passed_reads.saturating_add(1);
        Ok(self.entries[pos].reads)
    }

    /// 并发安全声明查询：两个存活条目的模式对能否并发放行。
    ///
    /// 未知条目显性拒绝；模式判定委托 [`DsvMode::concurrent_with`]。
    pub fn concurrent_ok(&self, a: u32, b: u32) -> Result<bool, u16> {
        let ma = self.mode_of(a)?;
        let mb = self.mode_of(b)?;
        Ok(ma.concurrent_with(mb))
    }

    /// 显性失效（三段之一）：单条目移除，墓碑记悬垂途径码。
    pub fn invalidate(&mut self, id: u32) -> Result<(), u16> {
        let pos = match self.entry_pos(id) {
            Some(p) => p,
            None => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                return Err(self.stale_code_for(id, CODE_DSV_ENTRY_UNKNOWN));
            }
        };
        self.entries.remove(pos);
        self.tombstones.push((id, CODE_DSV_STALE_ID));
        self.stats.invalidated = self.stats.invalidated.saturating_add(1);
        self.stats.live = self.entries.len();
        Ok(())
    }

    /// 缓存重建（锚点降级项：缓存失效 → 重建）：全表清空、全部旧 id 入
    /// **缓存失效**墓碑（与普通悬垂分码）、重建次数入账。
    pub fn cache_rebuild(&mut self) -> usize {
        let n = self.entries.len();
        for e in self.entries.iter() {
            self.tombstones.push((e.id, CODE_DSV_CACHE_STALE));
        }
        self.entries.clear();
        self.stats.rebuilds = self.stats.rebuilds.saturating_add(1);
        self.stats.live = 0;
        n
    }

    /// 违例台账快照（阻断级缺陷逐条可读；判据 3.2 与读屏用）。
    pub fn blocking_violations(&self) -> Vec<DsvViolation> {
        self.ledger.clone()
    }

    /// 查条目（只读）：未知/失效 id 返回 `None`（零 panic 面）。
    pub fn entry(&self, id: u32) -> Option<DsvEntry> {
        self.entries.iter().find(|e| e.id == id).copied()
    }

    /// 某资源的存活 DSV 数（只读）。
    pub fn live_for_resource(&self, resource: u32) -> usize {
        self.entries.iter().filter(|e| e.key.resource == resource).count()
    }

    /// 存活条目快照（判据与读屏用）。
    pub fn snapshot(&self) -> Vec<DsvEntry> {
        self.entries.clone()
    }

    /// 墓碑快照（判据分码悬垂判定的依据）。
    pub fn tombstone_snapshot(&self) -> Vec<(u32, u16)> {
        self.tombstones.clone()
    }

    /// 读屏摘要（锚点：DSV 状态读屏可达——存活/违例/拒绝三行起）。
    pub fn a11y_summary(&self) -> String {
        alloc::format!(
            "DSV 管理：存活 {}，注册 {}，共享 {}，违例阻断 {}，放行写 {}，拒绝 {}，重建 {}",
            self.stats.live,
            self.stats.registered,
            self.stats.shares,
            self.stats.blocked_writes,
            self.stats.passed_writes,
            self.stats.rejects,
            self.stats.rebuilds
        )
    }

    /// 查条目模式（内部工具：并发判定的取数路径，纯查询不记统计——
    /// 统计写入留给持有 `&mut self` 的拒绝路径）。
    fn mode_of(&self, id: u32) -> Result<DsvMode, u16> {
        match self.entries.iter().find(|e| e.id == id) {
            Some(e) => Ok(e.mode),
            None => Err(self.stale_code_for(id, CODE_DSV_ENTRY_UNKNOWN)),
        }
    }

    /// 按条目 id 找下标（内部工具）。
    fn entry_pos(&self, id: u32) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    /// 查墓碑取失效途径码；不在墓碑即「从未存在」。
    fn stale_code_for(&self, id: u32, fallback: u16) -> u16 {
        self.tombstones
            .iter()
            .find(|(tid, _)| *tid == id)
            .map(|(_, code)| *code)
            .unwrap_or(fallback)
    }
}

// ---------------------------------------------------------------------------
// 六、判据（run_vea37_checks —— 14 条，逐条映射锚点五条判据）
// ---------------------------------------------------------------------------

/// VE-F0037 域自检：读写声明 3 + 只读约束 3 + 违例检测 2 + 参数校验 3 + 判据自身 3。
pub fn run_vea37_checks() -> CheckSet {
    let mut s = CheckSet::new("vea37_dsview");

    // --- 判据 1.1：读写声明——注册产出合法 id 且统计入账 ---
    {
        let mut m = DsvMgr::new();
        let k = DsvKey { resource: 5, mip: 0, slice: 0 };
        let id = m.register(k, DsvMode::ReadWrite);
        let ok = id.is_ok()
            && m.entry(id.unwrap_or(0)).map(|e| e.mode) == Some(DsvMode::ReadWrite)
            && m.stats.registered == 1
            && m.stats.live == 1;
        s.add(
            "A37-声明-注册产出合法id且统计入账",
            ok,
            "注册即建条目：id 非零、模式按声明落位、registered/live 双计数一致",
        );
    }

    // --- 判据 1.2：读写声明——同键同模式重声明共享同 id ---
    {
        let mut m = DsvMgr::new();
        let k = DsvKey { resource: 5, mip: 0, slice: 0 };
        let a = m.register(k, DsvMode::ReadOnlyDepth);
        let b = m.register(k, DsvMode::ReadOnlyDepth);
        let ok = a.is_ok() && a == b
            && m.stats.registered == 1
            && m.stats.shares == 1
            && m.stats.live == 1;
        s.add(
            "A37-声明-同键同模式重声明共享同id",
            ok,
            "第二次注册命中共享：同 id、不新建条目、shares 入账——去重在注册期完成",
        );
    }

    // --- 判据 1.3：读写声明——同键异模式重声明冲突拒绝（不静默覆写） ---
    {
        let mut m = DsvMgr::new();
        let k = DsvKey { resource: 5, mip: 0, slice: 0 };
        let first = m.register(k, DsvMode::ReadOnlyDepth);
        let clash = m.register(k, DsvMode::ReadWrite);
        // 先失效再改模式：合法路径放行
        let _ = m.invalidate(first.unwrap_or(0));
        let re = m.register(k, DsvMode::ReadWrite);
        s.add(
            "A37-声明-同键异模式重声明冲突拒绝",
            first.is_ok()
                && clash == Err(CODE_DSV_MODE_CLASH)
                && re.is_ok()
                && m.stats.rejects == 1,
            "模式是键属性的一部分：异模式重声明显性拒绝，先失效再改模式才放行",
        );
    }

    // --- 判据 2.1：只读约束——只读深度声明下写入显性阻断（阻断级） ---
    {
        let mut m = DsvMgr::new();
        let id = m
            .register(DsvKey { resource: 6, mip: 0, slice: 0 }, DsvMode::ReadOnlyDepth)
            .unwrap_or(0);
        let w = m.declare_write(id);
        let r = m.declare_read(id);
        s.add(
            "A37-约束-只读深度声明下写入显性阻断",
            w == Err(CODE_DSV_READONLY_VIOLATION)
                && r.is_ok()
                && m.entry(id).map(|e| e.writes) == Some(0),
            "只读深度档：写入拒绝入台账、读取照常放行——只读约束只封写不封读",
        );
    }

    // --- 判据 2.2：只读约束——只读全部声明下写入同样阻断（档位分别验证） ---
    {
        let mut m = DsvMgr::new();
        let id = m
            .register(DsvKey { resource: 6, mip: 0, slice: 0 }, DsvMode::ReadOnlyAll)
            .unwrap_or(0);
        let w = m.declare_write(id);
        s.add(
            "A37-约束-只读全部声明下写入同样阻断",
            w == Err(CODE_DSV_READONLY_VIOLATION)
                && m.stats.blocked_writes == 1
                && m.entry(id).map(|e| e.writes) == Some(0),
            "只读全部档独立验证：两档只读各自封写，档位合并时本条先红",
        );
    }

    // --- 判据 2.3：只读约束——读写模式写入放行且计数递增 ---
    {
        let mut m = DsvMgr::new();
        let id = m
            .register(DsvKey { resource: 6, mip: 0, slice: 0 }, DsvMode::ReadWrite)
            .unwrap_or(0);
        let w1 = m.declare_write(id);
        let w2 = m.declare_write(id);
        s.add(
            "A37-约束-读写模式写入放行且计数递增",
            w1 == Ok(1) && w2 == Ok(2)
                && m.entry(id).map(|e| e.writes) == Some(2)
                && m.stats.passed_writes == 2
                && m.stats.blocked_writes == 0,
            "读写档写入逐次放行、计数逐次递增——放行面与阻断面分账不混",
        );
    }

    // --- 判据 3.1：违例检测——并发矩阵只读可并发写互斥（3×3 全对拍独立重算） ---
    {
        // 判据侧字面量重算期望矩阵：仅「两个都不可写」的格子放行
        let modes = DsvMode::ALL;
        let mut matrix_ok = true;
        for a in modes.iter() {
            for b in modes.iter() {
                // 字面量期望：ReadWrite 是唯一可写档，任一侧可写即互斥
                let expected = !matches!(*a, DsvMode::ReadWrite)
                    && !matches!(*b, DsvMode::ReadWrite);
                if a.concurrent_with(*b) != expected {
                    matrix_ok = false;
                }
            }
        }
        // 管理器层双条目并发判定：两只读放行、一只读一读写互斥
        let mut m = DsvMgr::new();
        let ro1 = m.register(DsvKey { resource: 8, mip: 0, slice: 0 }, DsvMode::ReadOnlyDepth).unwrap_or(0);
        let ro2 = m.register(DsvKey { resource: 9, mip: 0, slice: 0 }, DsvMode::ReadOnlyAll).unwrap_or(0);
        let rw = m.register(DsvKey { resource: 10, mip: 0, slice: 0 }, DsvMode::ReadWrite).unwrap_or(0);
        let pair_ok = m.concurrent_ok(ro1, ro2) == Ok(true)
            && m.concurrent_ok(ro1, rw) == Ok(false)
            && m.concurrent_ok(rw, rw) == Ok(false);
        s.add(
            "A37-违例-并发矩阵只读可并发写互斥",
            matrix_ok && pair_ok && m.stats.rejects == 0,
            "并发安全声明：判据侧按 3×3 字面量重算对拍 + 管理器双条目实证——只读可并发、写入互斥",
        );
    }

    // --- 判据 3.2：违例检测——违例台账阻断级逐条可读（事实与模式快照齐） ---
    {
        let mut m = DsvMgr::new();
        let id = m
            .register(DsvKey { resource: 6, mip: 0, slice: 0 }, DsvMode::ReadOnlyDepth)
            .unwrap_or(0);
        let _ = m.declare_write(id);
        let _ = m.declare_write(id);
        let ledger = m.blocking_violations();
        let ok = ledger.len() == 2
            && ledger.iter().all(|v| v.code == CODE_DSV_READONLY_VIOLATION)
            && ledger.iter().all(|v| v.id == id && v.mode == DsvMode::ReadOnlyDepth)
            && m.stats.blocked_writes == ledger.len();
        s.add(
            "A37-违例-违例台账阻断级逐条可读",
            ok,
            "每次违例一条台账：码、条目 id、模式快照逐条落位，阻断计数与台账逐条对账",
        );
    }

    // --- 判据 4.1：参数校验——键校验资源号零拒绝 ---
    {
        let mut m = DsvMgr::new();
        let bad = m.register(DsvKey { resource: 0, mip: 0, slice: 0 }, DsvMode::ReadWrite);
        s.add(
            "A37-参数-键校验资源号零拒绝",
            bad == Err(CODE_DSV_BAD_KEY) && m.stats.rejects == 1 && m.stats.live == 0,
            "0 号资源是保留位：注册期第一道红线，参数越界显性拒绝不静默",
        );
    }

    // --- 判据 4.2：参数校验——未知 id 与失效悬垂 id 分码拒绝 ---
    {
        let mut m = DsvMgr::new();
        let unknown = m.declare_write(999);
        let id = m
            .register(DsvKey { resource: 6, mip: 0, slice: 0 }, DsvMode::ReadWrite)
            .unwrap_or(0);
        let _ = m.invalidate(id);
        let stale = m.declare_write(id);
        s.add(
            "A37-参数-未知id与失效悬垂id分码拒绝",
            unknown == Err(CODE_DSV_ENTRY_UNKNOWN)
                && stale == Err(CODE_DSV_STALE_ID)
                && m.stats.rejects == 2,
            "「从未存在」与「曾经存在现已失效」分码：诊断不只知句柄死、还知怎么死",
        );
    }

    // --- 判据 4.3：参数校验——缓存重建后旧句柄走缓存失效码（与悬垂分码） ---
    {
        let mut m = DsvMgr::new();
        let id = m
            .register(DsvKey { resource: 6, mip: 0, slice: 0 }, DsvMode::ReadWrite)
            .unwrap_or(0);
        let _ = m.cache_rebuild();
        let after = m.declare_write(id);
        s.add(
            "A37-参数-缓存重建后旧句柄走缓存失效码",
            after == Err(CODE_DSV_CACHE_STALE) && m.stats.live == 0,
            "重建墓碑带缓存失效码：与 invalidate 的悬垂码分码，重建途径可追溯",
        );
    }

    // --- 判据 5.1：判据自身——缓存重建清空全表且重注册在新表可用 ---
    {
        let mut m = DsvMgr::new();
        let _ = m.register(DsvKey { resource: 5, mip: 0, slice: 0 }, DsvMode::ReadWrite);
        let _ = m.register(DsvKey { resource: 7, mip: 0, slice: 0 }, DsvMode::ReadOnlyAll);
        let n = m.cache_rebuild();
        let cleared = m.snapshot().is_empty() && m.stats.live == 0 && m.stats.rebuilds == 1;
        let re = m.register(DsvKey { resource: 5, mip: 0, slice: 0 }, DsvMode::ReadWrite);
        let id_new = m.register(DsvKey { resource: 5, mip: 0, slice: 0 }, DsvMode::ReadWrite);
        let ok = n == 2 && cleared && re.is_ok()
            && m.entry(id_new.unwrap_or(0)).map(|e| e.writes) == Some(0)
            && m.stats.registered == 3;
        s.add(
            "A37-判据-缓存重建清空全表且重注册可用",
            ok,
            "重建 = 扔掉可疑缓存：全表清空、计数如实；同键在新表按新条目重建",
        );
    }

    // --- 判据 5.2：判据自身——统计账与台账逐条对账（判据侧独立重算） ---
    {
        let mut m = DsvMgr::new();
        let rw = m.register(DsvKey { resource: 5, mip: 0, slice: 0 }, DsvMode::ReadWrite).unwrap_or(0);
        let ro = m.register(DsvKey { resource: 7, mip: 0, slice: 0 }, DsvMode::ReadOnlyDepth).unwrap_or(0);
        let _ = m.declare_write(rw);
        let _ = m.declare_write(rw);
        let _ = m.declare_write(rw);
        let _ = m.declare_write(ro);
        let _ = m.declare_read(rw);
        let _ = m.declare_read(ro);
        // 判据侧独立重算：不读 stats，扫条目与台账
        let sum_writes: u32 = m.snapshot().iter().map(|e| e.writes).sum();
        let sum_reads: u32 = m.snapshot().iter().map(|e| e.reads).sum();
        let ledger_n = m.blocking_violations().len();
        let ok = m.stats.passed_writes as u32 == sum_writes && sum_writes == 3
            && m.stats.passed_reads as u32 == sum_reads && sum_reads == 2
            && m.stats.blocked_writes == ledger_n && ledger_n == 1
            && m.stats.live == m.snapshot().len();
        s.add(
            "A37-判据-统计账与条目台账逐条对账",
            ok,
            "放行写/放行读 = 条目计数逐条求和、阻断写 = 台账长度：记账不漂移",
        );
    }

    // --- 判据 5.3：判据自身——诊断码互异且段独占（0x15 六码全对拍） ---
    {
        let codes = [
            CODE_DSV_BAD_KEY,
            CODE_DSV_READONLY_VIOLATION,
            CODE_DSV_ENTRY_UNKNOWN,
            CODE_DSV_STALE_ID,
            CODE_DSV_MODE_CLASH,
            CODE_DSV_CACHE_STALE,
        ];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0x1500);
        s.add(
            "A37-判据-诊断码互异且段独占",
            all_diff && in_segment && codes.len() == 6,
            "0x1501..=0x1506 六码两两互异、高字节全部落在 0x15 独占段",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 七、单元测试（cargo test svstar2）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn k(res: u32) -> DsvKey {
        DsvKey { resource: res, mip: 0, slice: 0 }
    }

    #[test]
    fn vea37_register_share_clash_roundtrip() {
        let mut m = DsvMgr::new();
        let a = m.register(k(7), DsvMode::ReadWrite).unwrap_or(0);
        let b = m.register(k(7), DsvMode::ReadWrite).unwrap_or(0);
        assert_eq!(a, b, "同键同模式共享同 id");
        assert_eq!(m.stats.shares, 1);
        assert_eq!(m.register(k(7), DsvMode::ReadOnlyAll), Err(CODE_DSV_MODE_CLASH));
        assert!(m.invalidate(a).is_ok());
        assert!(m.register(k(7), DsvMode::ReadOnlyAll).is_ok(), "失效后改模式放行");
        assert_eq!(m.stats.registered, 2);
    }

    #[test]
    fn vea37_readonly_block_and_ledger() {
        let mut m = DsvMgr::new();
        let ro = m.register(k(9), DsvMode::ReadOnlyDepth).unwrap_or(0);
        let rw = m.register(k(11), DsvMode::ReadWrite).unwrap_or(0);
        assert_eq!(m.declare_write(ro), Err(CODE_DSV_READONLY_VIOLATION));
        assert_eq!(m.declare_write(ro), Err(CODE_DSV_READONLY_VIOLATION));
        assert!(m.declare_write(rw).is_ok());
        assert!(m.declare_read(ro).is_ok(), "只读封写不封读");
        assert_eq!(m.blocking_violations().len(), 2);
        assert_eq!(m.stats.blocked_writes, 2);
        assert_eq!(m.stats.passed_writes, 1);
        assert_eq!(m.entry(ro).map(|e| e.writes), Some(0));
    }

    #[test]
    fn vea37_concurrent_matrix_and_modes() {
        assert!(DsvMode::ReadOnlyDepth.concurrent_with(DsvMode::ReadOnlyAll));
        assert!(!DsvMode::ReadWrite.concurrent_with(DsvMode::ReadOnlyDepth));
        assert!(!DsvMode::ReadWrite.concurrent_with(DsvMode::ReadWrite));
        assert!(DsvMode::writable(DsvMode::ReadWrite));
        let mut m = DsvMgr::new();
        let ro1 = m.register(k(3), DsvMode::ReadOnlyAll).unwrap_or(0);
        let ro2 = m.register(k(4), DsvMode::ReadOnlyDepth).unwrap_or(0);
        assert_eq!(m.concurrent_ok(ro1, ro2), Ok(true));
    }

    #[test]
    fn vea37_rebuild_and_stale_codes() {
        let mut m = DsvMgr::new();
        let a = m.register(k(7), DsvMode::ReadWrite).unwrap_or(0);
        let b = m.register(k(8), DsvMode::ReadOnlyAll).unwrap_or(0);
        assert_eq!(m.cache_rebuild(), 2);
        assert_eq!(m.declare_write(a), Err(CODE_DSV_CACHE_STALE));
        assert_eq!(m.declare_read(b), Err(CODE_DSV_CACHE_STALE));
        // 重建后同键重注册可用
        assert!(m.register(k(7), DsvMode::ReadWrite).is_ok());
        assert_eq!(m.stats.rebuilds, 1);
        assert_eq!(m.tombstone_snapshot().len(), 2);
    }

    #[test]
    fn vea37_a11y_summary_lines() {
        let mut m = DsvMgr::new();
        let _ = m.register(k(7), DsvMode::ReadWrite);
        let _ = m.register(k(8), DsvMode::ReadOnlyDepth);
        let text = m.a11y_summary();
        assert!(text.contains("存活 2"), "读屏行带存活数：{}", text);
        assert!(text.contains("注册 2"), "读屏行带注册数：{}", text);
    }

    #[test]
    fn vea37_checks_all_green() {
        let set = run_vea37_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0037 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
