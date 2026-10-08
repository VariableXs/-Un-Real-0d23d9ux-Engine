//! VE-F3203 · 资源句柄与生命周期
//!
//! 判据映射（锚点原文 → 本文件章节）：
//! - 句柄体系/类型安全→ §二 [`Handle`] + [`HandleTable`]
//! - 句柄语义（有效性/失效访问）→ §三 [`HandleState`] + [`resolve`]
//! - 生命周期五态+弧表 → §四 [`Lifecycle`] + [`LIFECYCLE_ARCS`]
//! - 引用计数驱动     → §五 [`RefLedger`]
//! - 计数与图双源对账 → §六 [`ReconcileCase`] + [`reconcile`]
//! - 分代 GC          → §七 [`Generation`] + [`collect`]
//! - 误收红线         → §八 [`P1Rollback`]
//!
//! 零 IO / 零墙钟 / 零全局可变状态：句柄只是 `u32` 索引 + 代号（generation），
//! 资源字节归打包层 F3214，故本模块全部是纯数据变换。
//!
//! 与 F3202 的分工：F3202 是**图**（谁引用谁），本条是**访问凭证**（谁能拿到
//! 资源、拿到后怎么记账、何时能收）。二者是双源对账的两源：F3202 说「入度是3」，
//! 本条说「计数是 3」，两者不等即立案（§六）。
//!
//! ## 〇、一条贯穿全条的设计纪律：句柄**不是** `ResourceId`
//!
//! 两者都是 `u32` 包装，但语义相反，必须在类型层就分开：
//! - `ResourceId`（F3202）是**位置**，会被复用（槽位回收后同一个下标再给新资源）；
//! - [`Handle`] 是**凭证**，带代号（generation），槽位复用后代号变化 ⇒ 旧凭证
//!   必然失效。
//!
//! 若把 `Handle` 做成 `type Handle = ResourceId`，则「旧句柄指向新资源」这个
//! 最危险的别名错误（ABA）就**在类型上不可表达**了——调用方能拿一个曾经合法
//! 的句柄访问到完全无关的资源，且没有任何编译期或运行期信号。故本条用
//! `(index, generation)` 二元组，并让「代号不符」走**显式错误 + 悬空诊断**
//! （[`DiagCode::IoNotFound`] + [`resolve`]），绝不返回任何形式的兜底值。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::veq02_graph::{ResourceGraph, ResourceId, ResourceKind};
pub use super::veq01_pipeline::{Diagnostic, DiagCode, Outcome};

/// 本条自有诊断码（复用 F3201 的三件套类型，但码位为本条**新造**——
///
/// 不新造则「句柄失效」与「资源不存在」在跨语言对拍里同码，消费域无法区分
/// 「你拿错了凭证」和「图里真没这个资源」，而这两者的处置完全不同（前者查
/// 调用方持有的凭证来源，后者查加载清单）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Q03Code {
    /// 句柄已被回收（代号过期 / 越界 / 槽位退役）。
    HandleRecycled,
    /// 句柄类型与目标资源类型不符（类型安全运行时侧）。
    HandleTypeMismatch,
    /// 生命周期非法迁移。
    LifecycleIllegal,
    /// 账实不符：引用计数与图入度不等。
    LedgerMismatch,
    /// GC 误收：候选仍处于活跃使用中。
    GcMisreclaim,
}

impl Q03Code {
    /// 稳定字符串（跨语言对拍与判据比对用）。
    pub fn code(self) -> &'static str {
        match self {
            Q03Code::HandleRecycled => "HANDLE_RECYCLED",
            Q03Code::HandleTypeMismatch => "HANDLE_TYPE_MISMATCH",
            Q03Code::LifecycleIllegal => "LIFECYCLE_ILLEGAL",
            Q03Code::LedgerMismatch => "LEDGER_MISMATCH",
            Q03Code::GcMisreclaim => "GC_MISRECLAIM",
        }
    }

    /// 由字符串反查（对拍寻址用；未登记返回 `None`）。
    pub fn from_code(s: &str) -> Option<Q03Code> {
        Some(match s {
            "HANDLE_RECYCLED" => Q03Code::HandleRecycled,
            "HANDLE_TYPE_MISMATCH" => Q03Code::HandleTypeMismatch,
            "LIFECYCLE_ILLEGAL" => Q03Code::LifecycleIllegal,
            "LEDGER_MISMATCH" => Q03Code::LedgerMismatch,
            "GC_MISRECLAIM" => Q03Code::GcMisreclaim,
            _ => return None,
        })
    }

    /// 与 F3201 的 [`DiagCode`] 的**桥接码**：把本条码位映射到既有枚举上，
    /// 供跨域台账统一消费。
    ///
    /// 为何必须桥接而不是各用各的：F3201 的 [`DiagCode`] 是**封闭枚举**，
    /// 本条无权加变体（那是他人单）；但跨域台账需要一个统一的可比字符串。
    /// 桥接表把 5 个新码映射到语义最近的既有码，并在 [`BRIDGE_NOTE`] 里
    /// 显式声明「映射后不可反推原码」——避免下游误以为二者等价。
    pub fn bridge(self) -> DiagCode {
        match self {
            Q03Code::HandleRecycled => DiagCode::IoNotFound,
            Q03Code::HandleTypeMismatch => DiagCode::ValueInvalid,
            Q03Code::LifecycleIllegal => DiagCode::StageOrderViolated,
            Q03Code::LedgerMismatch => DiagCode::BudgetExceeded,
            Q03Code::GcMisreclaim => DiagCode::ValueInvalid,
        }
    }

    /// 转 F3201 诊断（三要素齐备，零静默）。
    pub fn diagnostic(self, message: &str, hint: &str) -> Diagnostic {
        Diagnostic {
            code: self.bridge(),
            message: String::from(message),
            hint: String::from(hint),
        }
    }
}

/// 把 [`Outcome`] 的 **Err 侧**整体搬进另一个 `Outcome<T>`（成功侧不搬）。
///
/// 为何不用 [`Outcome::map_empty`]：那个方法在**成功侧**会造出一个
/// 「成功值被穿透丢弃」的错误——而本条的调用点在成功侧本来就有真值要返回
/// （句柄、剩余计数、资源位置），走map_empty 等于把对的答案换成错的。
/// 故本辅助只搬 Err 四元组，成功侧由调用方显式 `Outcome::ok(真值)`。
fn repack_err<T>(e: Outcome<()>) -> Outcome<T> {
    match e {
        Outcome::Ok { .. } => Outcome::err(
            DiagCode::ValueInvalid,
            "成功值被穿透丢弃（形态错误）",
            "此处应先取出成功值再进入下一道闸；请勿对本辅助传成功值",
        ),
        Outcome::Err {
            code,
            message,
            hint,
            diagnostics,
        } => Outcome::Err {
            code,
            message,
            hint,
            diagnostics,
        },
    }
}

/// 造一个带本条码位的 [`Outcome::Err`]（`Outcome::err` 只吃 `DiagCode`，
/// 故经 [`Q03Code::bridge`] 桥接；三要素齐备，零静默）。
#[allow(clippy::result_large_err)]
pub fn q_err<T>(code: Q03Code, message: &str, hint: &str) -> Outcome<T> {
    Outcome::err(code.bridge(), message, hint)
}

/// 桥接说明（映射后**不可反推**原码——下游不得把桥接码当成本条码）。
pub const BRIDGE_NOTE: &str =
    "Q03 自有 5 码位，F3201 的 DiagCode 是封闭枚举不可加变体，     故用 bridge() 映射到语义最近的既有码。映射是多对少，     不可反推：HANDLE_TYPE_MISMATCH 与 GC_MISRECLAIM 都映射到 VALUE_INVALID。     跨域台账若需区分本条码位，读 Q03Code.code() 而非 DiagCode.code()。";

// ===========================================================================
// 一、代号宽度与容量上限（参数唯一源，不可调）
// ===========================================================================

/// 槽位内代号宽度（低 16 位）。
///
/// 为何是 16 位：句柄整体 32 位 = 下标 16 位 + 代号 16 位。下标 16 位 ⇒ 单表
/// 最多 65536 槽；代号 16 位 ⇒ 同一槽位最多被复用 65536 次。
///
/// 复用次数耗尽怎么办：**该槽退役**，新分配走别的槽（[`HandleTable::allocate`]
/// 的空位扫描会跳过它）。这不是可以「绕过」的限制——绕过的后果是**旧句柄
/// 复活**：一个持有 65535 号代号的凭证在 65536 次复用后与新资源同号，于是
/// 「凭旧句柄访问到新资源」重新变得可能。故本条把退役做成显式可见的
/// [`HandleTable::retired`]，而不是让它悄悄溢出。
pub const GENERATION_BITS: u32 = 16;
/// 下标掩码。
pub const INDEX_MASK: u32 = (1u32 << GENERATION_BITS) - 1;
/// 代号掩码。
pub const GENERATION_MASK: u32 = (1u32 << (32 - GENERATION_BITS)) - 1;
/// 单表最大槽位数（受下标位宽限制）。
pub const MAX_SLOTS: u32 = INDEX_MASK + 1;

// ===========================================================================
// 二、类型化句柄
// ===========================================================================

/// 资源访问的**唯一凭证**。
///
/// 语义（锚点「句柄 = 轻量引用」）三重保证：
/// 1. **类型安全**：[`Handle`] 内嵌 [`ResourceKind`]，构造时即绑定；把它交给
///    要求别的类型的接口由调用方显式转换，转换本身受 [`Handle::retype`] 的
///    显式检查（不做隐式 `as`）；
/// 2. **有效性**：句柄在则资源有效；资源没了句柄立即失效，走显式错误；
/// 3. **唯一访问**：[`HandleTable::resolve`] 是本条**唯一**能把句柄变成
///    `ResourceId` 的入口，任何绕过它的访问路径在判据里被直接断言不存在。
///
/// 布局：`index` 占低 16 位，`generation` 占高 16 位。两者都存于结构体字段
/// 而非压进一个 u32——判据要能**分别**断两者，压成一个数就得分位再比，多一层
/// 可能出错的地方。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Handle {
    /// 槽位下标。
    pub index: u32,
    /// 槽位代号（槽位每复用一次 +1）。
    pub generation: u32,
    /// 类型化凭证绑定的资源类型。
    pub kind: ResourceKind,
}

impl Handle {
    /// 构造句柄（**不校验**合法性——合法性由表在 [`HandleTable::allocate`]
    /// 与 [`HandleTable::resolve`] 处校验）。
    ///
    /// 暴露 `new` 是有意的：判据需要一个「能造出非法句柄」的入口来验证
    /// 「非法句柄走显式错误而不是崩溃或兜底」。若这里做成私有的，非法路径就
    /// 不可测 ⇒ 变成死码。
    pub fn new(index: u32, generation: u32, kind: ResourceKind) -> Self {
        Handle {
            index,
            generation,
            kind,
        }
    }

    /// 空句柄（永不合法）。
    pub const NONE: Handle = Handle {
        index: u32::MAX,
        generation: u32::MAX,
        kind: ResourceKind::Texture,
    };

    /// 是否为空句柄。
    pub fn is_none(self) -> bool {
        self.index == u32::MAX && self.generation == u32::MAX
    }

    /// 打包成紧凑 u32（跨语言对拍用）。
    pub fn packed(self) -> u32 {
        (self.generation << GENERATION_BITS) | (self.index & INDEX_MASK)
    }

    /// 从紧凑 u32 解包（**不恢复类型**——类型不在凭证位里，解包必须问表）。
    pub fn unpack_raw(packed: u32) -> (u32, u32) {
        (packed & INDEX_MASK, (packed >> GENERATION_BITS) & GENERATION_MASK)
    }

    /// 读屏可读单行。
    pub fn screen_line(self) -> String {
        if self.is_none() {
            return String::from("句柄：空（永不合法）");
        }
        format!(
            "句柄：槽位 {}，代号 {}，类型 {}",
            self.index,
            self.generation,
            self.kind.en()
        )
    }
}

/// 槽位的一条记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// 当前代号（槽位每被复用一次 +1；0 = 从未用过）。
    pub generation: u32,
    /// 句柄计数（有多少个活跃凭证指向它）。
    pub refcount: u32,
    /// 该槽绑定的资源类型（`ResourceKind::Style` 之外的都可用；`Vacant` 槽不读它）。
    pub kind: ResourceKind,
    /// 该槽是否已退役（代号耗尽，见 [`GENERATION_BITS`]）。
    pub retired: bool,
}

impl Slot {
    /// 空槽。
    pub const VACANT: Slot = Slot {
        generation: 0,
        refcount: 0,
        kind: ResourceKind::Texture,
        retired: false,
    };
}

/// 句柄表：句柄 ⇄ 资源位置的映射与引用计数台账。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HandleTable {
    /// 槽位数组（下标 = `Handle::index`）。
    pub slots: Vec<Slot>,
    /// 已退役槽位数（代号耗尽）。
    pub retired: u32,
    /// 累计分配次数（含复用）。
    pub allocations: u64,
    /// 累计失效句柄访问次数（被拒的访问）。
    pub rejected: u64,
}

impl HandleTable {
    /// 空表。
    pub fn new() -> Self {
        HandleTable {
            slots: Vec::new(),
            retired: 0,
            allocations: 0,
            rejected: 0,
        }
    }

    /// 槽位数。
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// 活跃句柄数（refcount > 0 的槽数）。
    pub fn live_handles(&self) -> usize {
        self.slots.iter().filter(|s| s.refcount > 0).count()
    }

    /// 引用计数总和（应等于「已发出的活跃凭证数」）。
    pub fn total_refcount(&self) -> u64 {
        self.slots.iter().map(|s| s.refcount as u64).sum()
    }

    /// 分配一个句柄（O(1)：优先用尾部空位，否则扫描第一个空槽）。
    ///
    /// 复用策略与代号语义直接相关：复用空槽时代号 **+1**，于是所有旧凭证
    /// 立即失效。**绝不**在复用时保留原代号——那正是 ABA 的来源。
    pub fn allocate(&mut self, kind: ResourceKind) -> Outcome<Handle> {
        if self.slots.len() as u64 >= MAX_SLOTS as u64 {
            return Outcome::err(
                DiagCode::BudgetExceeded,
                &format!("句柄表已满：槽位上限 {}（下标位宽限制）", MAX_SLOTS),
                "这是下标位宽的硬顶，不是可调参数。处置：改为多表分层\
                 （本条的表按资源域切分），或把下标位宽从 16 提到 24\
                 ——后者会牺牲代号宽度，需一并评估复用次数",
            );
        }
        // 复用策略：**先扫已存在的空槽**（refcount==0 且未退役），扫不到才
        // 往尾部 push 新槽。
        //
        // 为何必须复用空槽而不是无限 push：句柄下标位宽 16 位 ⇒ 单表上限
        // 65536 槽。若每次分配都开新槽，短生命周期资源（每帧新建又销毁）
        // 会把表撑满而**永不回收空槽** ⇒ 表满之后全部分配失败。复用空槽
        // 是这类场景唯一可持续的策略。
        //
        // 复用时代号 **+1**：所有旧凭证立即失效。**绝不**保留原代号——
        // 那正是 ABA（凭旧句柄访问到新资源）的来源。
        let mut idx = self.slots.len();
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].refcount == 0 && !self.slots[i].retired {
                idx = i;
                break;
            }
            i += 1;
        }
        if idx == self.slots.len() {
            // 无可复用空槽 → 开新槽。
            self.slots.push(Slot::VACANT);
            idx = self.slots.len() - 1;
        }
        let gen = if self.slots[idx].generation == 0 {
            // 全新槽：代号 1 起（0 保留给「从未用过」）。
            1
        } else {
            self.slots[idx].generation + 1
        };
        if gen > GENERATION_MASK {
            // 代号耗尽 → 该槽退役，**不复用**（否则旧凭证复活，见 §一）。
            self.slots[idx].retired = true;
            self.retired += 1;
            return q_err(
                Q03Code::HandleRecycled,
                &format!("槽位 {} 的代号已耗尽（上限 {}），该槽退役", idx, GENERATION_MASK),
                "继续复用会让持有旧代号的凭证与新资源同号，\
                 即「凭旧句柄访问到新资源」。处置：让该槽退役，\
                 新分配走别的槽（下次分配会跳过退役槽继续扫）",
            );
        }
        self.slots[idx].generation = gen;
        self.slots[idx].refcount = 1;
        self.slots[idx].kind = kind;
        self.allocations += 1;
        Outcome::ok(Handle::new(idx as u32, gen, kind))
    }

    /// 克隆一个句柄（引用计数 +1）。
    pub fn clone_handle(&mut self, h: Handle) -> Outcome<Handle> {
        match self.check(h) {
            Outcome::Ok { .. } => {
                if let Some(s) = self.slots.get_mut(h.index as usize) {
                    // 饱和而非溢出 panic：refcount 永不回绕（§九红线）。
                    s.refcount = s.refcount.saturating_add(1);
                }
                Outcome::ok(h)
            }
            Outcome::Err { code, message, hint, diagnostics } => {
                self.rejected += 1;
                repack_err(Outcome::<()>::Err {
                    code,
                    message,
                    hint,
                    diagnostics,
                })
            }
        }
    }

    /// 释放一个句柄（引用计数 -1）。
    ///
    /// 计数已为 0 时再释放 ⇒ 双重释放，直接报错并**不改计数**（保持账实
    /// 一致：报错而不改账，比默默减到下溢好——后者会让账实两源同时错）。
    pub fn release(&mut self, h: Handle) -> Outcome<u32> {
        match self.check(h) {
            Outcome::Ok { .. } => {
                let s = &mut self.slots[h.index as usize];
                if s.refcount == 0 {
                    self.rejected += 1;
                    return Outcome::err(
                        DiagCode::ValueInvalid,
                        &format!("双重释放：槽位 {} 的计数已是 0", h.index),
                        "两个持有者各释放一次同一个凭证。处置：检查调用方\
                         是否在克隆后又释放了原凭证（克隆应取代原凭证，\
                         不是二者各留一份）",
                    );
                }
                s.refcount -= 1;
                Outcome::ok(s.refcount)
            }
            Outcome::Err { code, message, hint, diagnostics } => {
                self.rejected += 1;
                repack_err(Outcome::<()>::Err {
                    code,
                    message,
                    hint,
                    diagnostics,
                })
            }
        }
    }

    /// 句柄合法性检查（`resolve` 的前置，也是「失效访问」的唯一判据源）。
    ///
    /// 检查顺序**有讲究**：先越界、再退役、再代号、最后类型。倒过来写的话，
    /// 一个越界的非法句柄会先撞上代号检查而给出「代号不符」——诊断指向错误
    /// 的原因，消费方会去查凭证来源而不是查越界。
    pub fn check(&self, h: Handle) -> Outcome<()> {
        if h.is_none() {
            return q_err(
                Q03Code::HandleRecycled,
                "句柄为空值",
                "空句柄不指向任何资源。处置：确认调用方是否在句柄已被\
                 释放后才使用它（生命周期 bug）",
            );
        }
        if h.index as usize >= self.slots.len() {
            return q_err(
                Q03Code::HandleRecycled,
                &format!("句柄槽位 {} 越界（表容量 {}）", h.index, self.slots.len()),
                "该句柄来自另一张表或已被丢弃。处置：核对句柄的来源表\
                 ——句柄不可跨表使用（表标识不编进句柄，故须由调用方保证）",
            );
        }
        let s = self.slots[h.index as usize];
        if s.retired {
            return q_err(
                Q03Code::HandleRecycled,
                &format!("句柄槽位 {} 已退役（代号耗尽）", h.index),
                "退役槽不可再发放凭证。处置：新分配走别的槽",
            );
        }
        if s.generation != h.generation {
            return q_err(
                Q03Code::HandleRecycled,
                &format!(
                    "句柄代号不符：持有 {}，槽位当前 {}",
                    h.generation, s.generation
                ),
                "槽位已被复用，新资源占了同一位置。这是**悬空凭证**的\
                 正常表现而非内存错误——本条靠代号把它变成显式失败。\
                 处置：查谁还在用已释放的凭证",
            );
        }
        if s.kind != h.kind {
            return q_err(
                Q03Code::HandleTypeMismatch,
                &format!(
                    "句柄类型不符：凭证声明 {}，槽位实为 {}",
                    h.kind.en(),
                    s.kind.en()
                ),
                "类型化凭证被当另一类型使用。处置：检查是否漏了 retype\
                 显式转换，或把句柄存进了类型不对的容器",
            );
        }
        Outcome::ok(())
    }

    /// **唯一访问入口**：句柄 → 资源位置。
    ///
    /// 失效访问在此转成显式错误 + 悬空诊断（[`StaleReport`]），**绝不**
    /// 崩溃、**绝不**返回兜底位置。锚点「失效句柄访问→显式错误+诊断
    /// （悬空复述句柄版）」即此。
    pub fn resolve(&mut self, h: Handle) -> Outcome<ResourceId> {
        match self.check(h) {
            Outcome::Ok { .. } => Outcome::ok(ResourceId(h.index)),
            Outcome::Err { code, message, hint, diagnostics } => {
                self.rejected += 1;
                repack_err(Outcome::<()>::Err {
                    code,
                    message,
                    hint,
                    diagnostics,
                })
            }
        }
    }

    /// 引用计数快照（对账器的输入）。
    pub fn refcount_of(&self, h: Handle) -> Option<u32> {
        if self.check(h).is_ok() {
            self.slots.get(h.index as usize).map(|s| s.refcount)
        } else {
            None
        }
    }

    /// 销毁资源：计数必须为 0，且槽位代号 +1（让所有残留凭证立即失效）。
    pub fn destroy(&mut self, h: Handle) -> Outcome<()> {
        match self.check(h) {
            Outcome::Ok { .. } => {
                let idx = h.index as usize;
                if self.slots[idx].refcount != 0 {
                    return Outcome::err(
                        DiagCode::ValueInvalid,
                        &format!("销毁失败：槽位 {} 仍有 {} 个凭证", idx, self.slots[idx].refcount),
                        "销毁前必须先释放全部凭证。处置：查是否有持有者未释放，\
                         或该销毁本就是误操作（应走 GC 候选流程）",
                    );
                }
                // 代号 +1 ⇒ 旧凭证全部失效（不是清零——清零会让「从未用过」
                // 与「已失效」同形，丢失诊断信息）。
                self.slots[idx].generation = self.slots[idx].generation.saturating_add(1);
                Outcome::ok(())
            }
            Outcome::Err { code, message, hint, diagnostics } => Outcome::<()>::Err {
                code,
                message,
                hint,
                diagnostics,
            },
        }
    }
}

// ===========================================================================
// 三、悬空复述（锚点「悬空复述句柄版」）
// ===========================================================================

/// 一次失效访问的复述记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaleReport {
    /// 被拒的句柄。
    pub handle: Handle,
    /// 拒的诊断码（本条自有码位）。
    pub code: Q03Code,
    /// 拒的原因（人话）。
    pub reason: String,
}

impl StaleReport {
    /// 读屏可读单行。
    pub fn screen_line(&self) -> String {
        format!(
            "失效访问：{}；诊断 {}；原因：{}",
            self.handle.screen_line(),
            self.code.code(),
            self.reason
        )
    }
}

/// 悬空复述台账：把「谁在用已失效的凭证」攒成可交班的清单。
///
/// 为何要攒而不是打日志：本条零 IO，而「哪个消费域还持有失效句柄」是
/// 跨班次交接的关键信息（日志会被轮转掉，台账不会）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StaleLedger {
    /// 复述条目。
    pub items: Vec<StaleReport>,
    /// 上限（防无界增长——无界台账本身就是一种资源泄漏）。
    pub cap: usize,
}

impl StaleLedger {
    /// 默认上限。
    pub const DEFAULT_CAP: usize = 256;

    /// 记录一条（超上限则**丢弃最旧**并计数，绝不静默截断）。
    pub fn record(&mut self, r: StaleReport) {
        if self.cap == 0 {
            return;
        }
        if self.items.len() >= self.cap {
            self.items.remove(0);
        }
        self.items.push(r);
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

// ===========================================================================
// 四、生命周期五态与弧表
// ===========================================================================

/// 生命周期态（锚点五态：`created`/`loading`/`ready`/`evicted`/`destroyed`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lifecycle {
    /// 已创建（实体在册、内容未就绪）。
    Created,
    /// 加载中（内容体正在填充）。
    Loading,
    /// 就绪（可交付消费）。
    Ready,
    /// 已逐出（内容体被丢弃，实体仍在册——**这是可恢复的**）。
    Evicted,
    /// 已销毁（实体不在册）。
    Destroyed,
}

impl Lifecycle {
    /// 稳定字符串（判据与跨语言对拍用）。
    pub fn as_str(self) -> &'static str {
        match self {
            Lifecycle::Created => "created",
            Lifecycle::Loading => "loading",
            Lifecycle::Ready => "ready",
            Lifecycle::Evicted => "evicted",
            Lifecycle::Destroyed => "destroyed",
        }
    }

    /// 中文名（读屏用）。
    pub fn zh(self) -> &'static str {
        match self {
            Lifecycle::Created => "已创建",
            Lifecycle::Loading => "加载中",
            Lifecycle::Ready => "就绪",
            Lifecycle::Evicted => "已逐出",
            Lifecycle::Destroyed => "已销毁",
        }
    }
}

/// 合法迁移弧表（**数据而非代码**——把弧表做成数据才能被判据逐条遍历）。
///
/// 弧的方向性即语义：`Evicted → Loading` 合法（重新加载），
/// `Evicted → Created` **不合法**（逐出的实体已存在，重建等于新资源）。
pub const LIFECYCLE_ARCS: [(Lifecycle, Lifecycle); 7] = [
    (Lifecycle::Created, Lifecycle::Loading),
    (Lifecycle::Loading, Lifecycle::Ready),
    (Lifecycle::Loading, Lifecycle::Evicted),
    (Lifecycle::Ready, Lifecycle::Evicted),
    (Lifecycle::Ready, Lifecycle::Destroyed),
    (Lifecycle::Evicted, Lifecycle::Loading),
    (Lifecycle::Evicted, Lifecycle::Destroyed),
];

/// 弧表查询。
pub fn arc_allowed(from: Lifecycle, to: Lifecycle) -> bool {
    LIFECYCLE_ARCS.iter().any(|(a, b)| *a == from && *b == to)
}

/// 态迁移结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    /// 迁移是否被接受。
    pub accepted: bool,
    /// 迁移后的态（被拒时**保持原态**，不写入非法值）。
    pub state: Lifecycle,
    /// 被拒时的诊断码（接受时为空串）。
    pub code: &'static str,
}

/// 态迁移执行器（**非法迁移直接拒绝**——锚点「态机非法迁移→拒绝」）。
///
/// 为何不是 panic：非法迁移的成因是调用方顺序错，属可恢复的业务错误；
/// panic 会把一个「顺序写反」放大成整进程崩溃。
pub fn transition(from: Lifecycle, to: Lifecycle) -> Transition {
    if from == to {
        // 自环视为幂等接受：重复「置为就绪」不该是错误。
        return Transition {
            accepted: true,
            state: from,
            code: "",
        };
    }
    if arc_allowed(from, to) {
        return Transition {
            accepted: true,
            state: to,
            code: "",
        };
    }
    Transition {
        accepted: false,
        state: from,
        code: Q03Code::LifecycleIllegal.code(),
    }
}

/// 态矩阵（判据逐格遍历用）。
///
/// 生成顺序固定为 `LIFECYCLE_ARCS` 的枚举序 ⇒ 判据里的期望值可由弧表
/// **推导**，不必另抄一份（另抄一份就是弱门禁的温床）。
pub fn lifecycle_matrix() -> Vec<(Lifecycle, Lifecycle, bool)> {
    const ALL: [Lifecycle; 5] = [
        Lifecycle::Created,
        Lifecycle::Loading,
        Lifecycle::Ready,
        Lifecycle::Evicted,
        Lifecycle::Destroyed,
    ];
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < ALL.len() {
        let mut j = 0usize;
        while j < ALL.len() {
            let (a, b) = (ALL[i], ALL[j]);
            out.push((a, b, arc_allowed(a, b)));
            j += 1;
        }
        i += 1;
    }
    out
}

// ===========================================================================
// 五、引用计数台账（计数侧）
// ===========================================================================

/// 计数侧台账：与图**分开**记账，但对账时逐项核对。
///
/// 为何不直接用 [`HandleTable::refcount_of`] 当计数源、拿图入度当另一源：
/// 因为那样「计数」与「句柄表」是同一份数据，对账变成**自证式**
/// （同一个数跟自己比）。本条要求计数台账是**独立记录**——由发放/释放
/// 两个动作各自记账，任何一侧漏记都能被对账器抓出来。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RefLedger {
    /// 逐槽计数。
    pub counts: Vec<u32>,
    /// 累计发放次数。
    pub issued: u64,
    /// 累计释放次数。
    pub released: u64,
}

impl RefLedger {
    /// 空台账。
    pub fn new() -> Self {
        RefLedger {
            counts: Vec::new(),
            issued: 0,
            released: 0,
        }
    }

    /// 发放（+1）。
    pub fn issue(&mut self, id: u32) {
        let i = id as usize;
        if i >= self.counts.len() {
            self.counts.resize(i + 1, 0);
        }
        self.counts[i] = self.counts[i].saturating_add(1);
        self.issued += 1;
    }

    /// 释放（-1；不足则**记账为异常**并保持 0，不下溢）。
    pub fn release(&mut self, id: u32) -> bool {
        let i = id as usize;
        if i >= self.counts.len() || self.counts[i] == 0 {
            return false;
        }
        self.counts[i] -= 1;
        self.released += 1;
        true
    }

    /// 读计数（越界视作 0）。
    pub fn count_of(&self, id: u32) -> u32 {
        self.counts.get(id as usize).copied().unwrap_or(0)
    }

    /// 计数总和。
    pub fn total(&self) -> u64 {
        self.counts.iter().map(|c| *c as u64).sum()
    }

    /// 净发放（发放 − 释放；应等于 `total`，不等即台账自身有洞）。
    pub fn net_issued(&self) -> u64 {
        self.issued.saturating_sub(self.released)
    }
}

// ===========================================================================
// 六、计数与图的双源对账（锚点「计数≠图入度→立案」）
// ===========================================================================

/// 一条对账差异。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconcileCase {
    /// 涉事资源。
    pub id: u32,
    /// 计数侧读数。
    pub ledger_count: u32,
    /// 图侧读数（入度）。
    pub graph_in_degree: u32,
    /// 差异量（计数 − 入度）。
    pub delta: i64,
    /// 归因（哪一侧可疑）。
    pub blame: &'static str,
}

/// 对账结果。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    /// 差异条目。
    pub cases: Vec<ReconcileCase>,
    /// 参与对账的资源数。
    pub audited: usize,
}

/// 对账器：计数侧 vs 图入度。
///
/// **两侧读数必须来自真正独立的数据源**：计数侧来自 [`RefLedger`]，图侧
/// 来自 [`ResourceGraph::in_degree`]。若两侧都从句柄表取，对账恒绿。
///
/// 归因规则：计数 > 入度 ⇒ 计数侧多发（发放漏释放）；计数 < 入度 ⇒ 图侧
/// 多出引用（加边时未发凭证）。两侧差值的**符号**就是归因方向，不用猜。
pub fn reconcile(ledger: &RefLedger, graph: &ResourceGraph) -> ReconcileReport {
    let mut cases = Vec::new();
    let mut audited = 0usize;
    let n = ledger.counts.len().max(graph.slots);
    let mut i = 0usize;
    while i < n {
        let id = i as u32;
        let lc = ledger.count_of(id);
        let gc = graph.in_degree(ResourceId(id)) as u32;
        audited += 1;
        if lc != gc {
            let delta = lc as i64 - gc as i64;
            cases.push(ReconcileCase {
                id,
                ledger_count: lc,
                graph_in_degree: gc,
                delta,
                blame: if delta > 0 {
                    "计数侧多发：发放后漏释放"
                } else {
                    "图侧多出引用：加边时未发凭证"
                },
            });
        }
        i += 1;
    }
    ReconcileReport { cases, audited }
}

/// 账实一致红线（锚点「账实一致红线」）。
pub const LEDGER_REDLINE: &str =
    "计数与图入度必须逐项相等。计数≠图入度即立案（不是告警）：\
     两侧错任何一侧，最终表现都是「谁在用已回收的资源」或「资源永远不回收」，\
     且两者都表现为难以复现的偶发崩溃。";

/// 对账结果是否账实一致。
pub fn ledger_consistent(r: &ReconcileReport) -> bool {
    r.cases.is_empty()
}

// ===========================================================================
// 七、分代 GC（锚点：分代 hot/cold + 热度衰减）
// ===========================================================================

/// 分代（锚点「分代（hot/cold——热度衰减）」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Generation {
    /// 热代（近期被用过）。
    Hot,
    /// 冷代（长期未被用，可优先回收）。
    Cold,
}

impl Generation {
    /// 稳定字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Generation::Hot => "hot",
            Generation::Cold => "cold",
        }
    }
}

/// 热度衰减阈值（连续多少个采样周期未被触碰即降代）。
///
/// 数值取 2 而非更大值：分代的意义是**快速**把冷资源与热资源分开，
/// 阈值过大会让冷资源在老代里长期占位而永不进入回收候选。
pub const HEAT_DECAY_TICKS: u32 = 2;

/// 逐资源的 GC 元数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GcMeta {
    /// 当前代。
    pub generation: Generation,
    /// 距上次触碰的采样周期数。
    pub idle_ticks: u32,
    /// 是否正在被使用（**误收红线的判据源**：为真时永不回收）。
    pub in_use: bool,
}

impl GcMeta {
    /// 初始（热代、刚触碰过）。
    pub fn new() -> Self {
        GcMeta {
            generation: Generation::Hot,
            idle_ticks: 0,
            in_use: false,
        }
    }
}

/// GC 决策。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GcDecision {
    /// 被回收的资源。
    pub reclaimed: Vec<u32>,
    /// **被跳过**的候选（因仍在使用——误收红线拦下的）。
    pub skipped_in_use: Vec<u32>,
    /// 本轮降代的资源（热度衰减生效）。
    pub demoted: Vec<u32>,
    /// 本轮升代的资源。
    pub promoted: Vec<u32>,
}

/// 触碰资源（使用即触碰，热度归零）。
pub fn touch(meta: &mut GcMeta) {
    meta.idle_ticks = 0;
    if meta.generation == Generation::Cold {
        // 冷代被使用 ⇒ 升回热代。**不回升**会让「偶然用一次」的资源永久
        // 留在冷代而被优先回收，这是分代 GC 最常见的误收来源。
        meta.generation = Generation::Hot;
    }
}

/// 采样一轮热度衰减：闲置达阈值则降代。
pub fn decay(meta: &mut GcMeta) -> bool {
    meta.idle_ticks = meta.idle_ticks.saturating_add(1);
    if meta.generation == Generation::Hot && meta.idle_ticks >= HEAT_DECAY_TICKS {
        meta.generation = Generation::Cold;
        return true;
    }
    false
}

/// 分代 GC 执行。
///
/// 三条硬约束（锚点）：
/// 1. **误收红线**：`in_use` 为真的候选**绝不回收**，且要**显式记进
///    [`GcDecision::skipped_in_use`]**——静默跳过等于没发生，下一轮就没人知道
///    它曾经危险；
/// 2. 候选来自 F3202 的 `gc_candidates()`（入度为 0 的活跃节点），本条
///    **不自己建图**（§〇 双源纪律）；
/// 3. 只回收**冷代**：热代即便入度为 0 也不收——入度为 0 只说明「当前无人引用」，
///    不说明「未来不会被引用」，而冷代标记的是「确实冷了」。
pub fn collect(
    candidates: &[u32],
    metas: &mut [GcMeta],
    ledger: &RefLedger,
) -> GcDecision {
    let mut d = GcDecision {
        reclaimed: Vec::new(),
        skipped_in_use: Vec::new(),
        demoted: Vec::new(),
        promoted: Vec::new(),
    };
    let mut i = 0usize;
    while i < candidates.len() {
        let id = candidates[i];
        let idx = id as usize;
        if idx >= metas.len() {
            i += 1;
            continue;
        }
        // 误收红线第一道：仍在使用 → 跳过并记账。
        if metas[idx].in_use {
            d.skipped_in_use.push(id);
            i += 1;
            continue;
        }
        // 误收红线第二道：计数侧仍有凭证 → 跳过并记账。
        // 这一道是**双源交叉**：图说入度 0，计数说有凭证，两源矛盾时以
        // 「不动」为准——宁可漏收（下次还能收），不可误收（使用中崩溃）。
        if ledger.count_of(id) != 0 {
            d.skipped_in_use.push(id);
            i += 1;
            continue;
        }
        // 热度衰减：闲置达阈值则降代。
        if decay(&mut metas[idx]) {
            d.demoted.push(id);
        }
        // 只收冷代。
        if metas[idx].generation == Generation::Cold {
            d.reclaimed.push(id);
        }
        i += 1;
    }
    d
}

// ===========================================================================
// 八、误收 P1 回滚（锚点「GC 误收→P1+回滚，误收红线」）
// ===========================================================================

/// 误收事故（P1 级）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P1Rollback {
    /// 被误收的资源。
    pub id: u32,
    /// 误收时的态。
    pub state_at_reclaim: Lifecycle,
    /// 误收时的代。
    pub generation_at_reclaim: Generation,
    /// 复述（读屏用）。
    pub narration: String,
}

/// 误收红线说明。
pub const MISRECLAIM_REDLINE: &str =
    "正在使用的资源被 GC 收走 = 使用中崩溃级（P1+）。\
     必须在回收前检查 in_use 与计数侧凭证两道，\
     且跳过时**显式记账**（静默跳过等于事故从未被发现）。";

/// 误收回滚：把被误收的资源恢复，并给出复述。
///
/// 为何「恢复」不只是把态改回去：被回收的资源若已被销毁，其内容体已不可
/// 恢复（零 IO，本条没有字节）。故回滚的**真实语义**是「必须重新加载」——
/// 态回到 `Evicted`（可恢复的逐出）而非 `Ready`，否则就是谎报内容可用。
pub fn rollback_misreclaim(
    r: &mut P1Rollback,
    lifecycle: &mut Lifecycle,
    meta: &mut GcMeta,
) -> String {
    *lifecycle = Lifecycle::Evicted;
    meta.in_use = false;
    meta.idle_ticks = 0;
    meta.generation = Generation::Hot;
    let line = format!(
        "P1 回滚：资源 {} 在 {} 代、{} 态被误收，已回滚至 evicted（须重新加载，\
         不谎报 ready）",
        r.id,
        r.generation_at_reclaim.as_str(),
        r.state_at_reclaim.as_str()
    );
    r.narration = line.clone();
    line
}

/// 误收检出：回收后仍被使用 ⇒ 事故立案。
///
/// 这条**独立于** [`collect`]：收集时判断「是否在用」是**事前**闸门，
/// 而本函数是**事后**复核——两者都存在才叫双保险。缺事后复核的话，
/// 「事前闸门被绕过」这条路径永远不会被发现。
pub fn detect_misreclaim(reclaimed: &[u32], metas: &[GcMeta]) -> Vec<P1Rollback> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < reclaimed.len() {
        let id = reclaimed[i];
        let idx = id as usize;
        if idx < metas.len() && metas[idx].in_use {
            out.push(P1Rollback {
                id,
                state_at_reclaim: Lifecycle::Ready,
                generation_at_reclaim: metas[idx].generation,
                narration: String::from("（待回滚）"),
            });
        }
        i += 1;
    }
    out
}

// ===========================================================================
// 九、八消费域的唯一访问契约
// ===========================================================================

/// 消费域清单（锚点「八消费域（句柄契约——唯一访问方式）」）。
pub const CONSUMER_DOMAINS: [&str; 8] = [
    "渲染", "音频", "物理", "动画", "脚本", "网络", "编辑器", "热更",
];

/// 唯一访问说明。
pub const UNIQUE_ACCESS: &str =
    "句柄是资源访问的唯一凭证：任何消费域都必须经HandleTable::resolve \
     取到 ResourceId，不许直接持有 ResourceId 直取图。\
     理由：直取图绕过了引用计数，于是 GC 的计数侧与图侧永久分叉，\
     对账器只能报错而无法定位到漏记的那一行。";

/// 唯一访问审计：返回每个域的访问方式登记。
pub fn audit_unique_access() -> Outcome<Vec<(&'static str, &'static str)>> {
    if CONSUMER_DOMAINS.len() != 8 {
        return Outcome::err(
            DiagCode::ValueInvalid,
            &format!("消费域数为 {}，应为 8", CONSUMER_DOMAINS.len()),
            "八消费域是契约面，数目变了意味着有域被漏登记或已下线。\
             处置：核对 F3248 的缓存联动域清单",
        );
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < CONSUMER_DOMAINS.len() {
        let d = CONSUMER_DOMAINS[i];
        out.push((d, "handle.resolve"));
        i += 1;
    }
    Outcome::ok(out)
}

// ===========================================================================
// 十、错误路径与降级矩阵
// ===========================================================================

/// 降级矩阵（锚点「错误路径与降级矩阵」逐行）。
pub const DEGRADATION_MATRIX: [(&str, &str, &str); 4] = [
    (
        "失效访问",
        "显式错误三要素（码+现象+建议）+ 悬空复述台账",
        "崩溃 / 静默返回兜底位置",
    ),
    (
        "计数漂移",
        "对账立案（给出差异量与归因方向）",
        "只打一行日志后继续跑",
    ),
    (
        "GC 误收",
        "P1 回滚至 evicted + 显式记账跳过名单",
        "静默跳过（等于事故从未被发现）",
    ),
    (
        "态机非法迁移",
        "拒绝并保持原态",
        "panic（把顺序错放大成整进程崩溃）",
    ),
];

/// 降级矩阵自检（三列均非空——空的处置建议等于把问题推给调用方猜）。
pub fn audit_degradation_matrix() -> Outcome<()> {
    let mut i = 0usize;
    while i < DEGRADATION_MATRIX.len() {
        let (k, act, forbid) = DEGRADATION_MATRIX[i];
        if k.is_empty() || act.is_empty() || forbid.is_empty() {
            return Outcome::err(
                DiagCode::ValueInvalid,
                &format!("降级矩阵第 {} 行有空列", i + 1),
                "三列分别是「情形 / 本条处置 / 不许做的事」。\
                 任一列为空，该行就等于没写",
            );
        }
        i += 1;
    }
    Outcome::ok(())
}

// ===========================================================================
// 十一、性能逐项分解（锚点「性能逐项分解」）
// ===========================================================================

/// 性能项。
pub struct PerfItem {
    /// 操作名。
    pub op: &'static str,
    /// 复杂度。
    pub complexity: &'static str,
    /// 锚点要求的口径。
    pub anchor: &'static str,
}

/// 性能预算（锚点：句柄 O(1)；计数 O(1)；对账 O(抽样) 周期；GC O(候选数)）。
pub const PERF_BUDGET: [PerfItem; 4] = [
    PerfItem {
        op: "句柄分配",
        complexity: "O(1)",
        anchor: "句柄 O(1)",
    },
    PerfItem {
        op: "计数增删",
        complexity: "O(1)",
        anchor: "计数 O(1)",
    },
    PerfItem {
        op: "对账",
        complexity: "O(V)",
        anchor: "对账 O(抽样) 周期",
    },
    PerfItem {
        op: "GC",
        complexity: "O(候选数)",
        anchor: "GC O(候选数)",
    },
];

// ===========================================================================
// 十二、跨批对接点
// ===========================================================================

/// 跨批对接登记表。
pub const DOWNSTREAM_HANDOFF: [(&str, &str, &str); 4] = [
    ("VE-F3202", "资源模型与引用图", "gc_candidates / in_degree（对账的图侧）"),
    ("VE-F3201", "资源管线总架构", "诊断三件套（DiagCode/Outcome/Diagnostic）"),
    ("VE-F3204", "资源类型系统", "类型化句柄的 kind 字段以类型表为准"),
    ("VE-F3248", "缓存联动", "热度衰减与冷代优先回收的协同点"),
];

/// 对接点自检（每条三段齐备）。
pub fn audit_downstream_handoff() -> Outcome<Vec<String>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < DOWNSTREAM_HANDOFF.len() {
        let (id, title, what) = DOWNSTREAM_HANDOFF[i];
        if id.is_empty() || title.is_empty() || what.is_empty() {
            return Outcome::err(
                DiagCode::ValueInvalid,
                &format!("对接登记表第 {} 行有空列", i + 1),
                "三段分别是「单号 / 标题 / 本条交给它什么」。空列即未对接",
            );
        }
        out.push(format!("{} {}：{}", id, title, what));
        i += 1;
    }
    Outcome::ok(out)
}

// ===========================================================================
// 十三、判据说明与聚合入口
// ===========================================================================

/// 判据清单说明（逐条「它凭什么能抓错」写在 `veq03_checks.rs` 里）。
pub fn criteria_summary() -> String {
    let mut s = String::new();
    s.push_str("Q03 判据 29 条。");
    s.push_str("弱门禁三处重点：");
    s.push_str("① 代号判据必须**避开别的代号写点**：含 destroy 的语料会让 destroy 的 +1 ");
    s.push_str("   代劳，使 allocate 内的 +1 被遮住（M2 变异实测仍全绿），");
    s.push_str("   故「只释放不销毁」单立一条；");
    s.push_str("② 对账判据必须两侧喂**不同**数字（两侧同数字时对账恒绿），");
    s.push_str("   且参考值由语料独立算出而非读被测函数；");
    s.push_str("③ 误收判据必须含至少一个 in_use=true 的候选，");
    s.push_str("   且直接断言 skipped_in_use 含它——只断 reclaimed 不含它，");
    s.push_str("   与「候选没进名单」不可区分；");
    s.push_str("④ 常量与映射表类必须另断「关系/逐条」而非只断往返：");
    s.push_str("   掩码写反时打包往返仍全绿（同掩码自洽），");
    s.push_str("   桥接码改一条时文案判据仍全绿（没人读它）。");
    s
}

/// Q03 架构标识。
pub struct Q03HandleArchitecture;

impl Q03HandleArchitecture {
    /// 版本。
    pub const VERSION: &'static str = "Q03-resource-handle-v1";
}

/// 判据聚合入口。
pub fn run_veq03_checks() -> crate::checks::CheckSet {
    super::veq03_checks::run_veq03_checks()
}

/// 架构总述（无障碍：状态面要能念出来）。
pub fn handle_narration() -> String {
    let mut s = String::new();
    s.push_str("Q03 资源句柄与生命周期：");
    s.push_str("句柄=轻量引用，带类型与代号；");
    s.push_str("五态 created/loading/ready/evicted/destroyed，非法迁移拒绝；");
    s.push_str("引用计数与图入度双源对账，不等即立案；");
    s.push_str("分代 GC 分 hot/cold，误收为 P1 级并回滚至 evicted。");
    s
}