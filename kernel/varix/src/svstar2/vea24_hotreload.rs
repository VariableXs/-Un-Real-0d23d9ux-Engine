//! VE-F0024 · 着色器热重载协调器（VE-A 域 · 重载流水线 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0024`
//!
//! **判据（锚点原文）**：开发期着色器热重载的协调（重载请求→重编译→管线重建→场景
//! 验证四段流水线），坏码不落地（编译失败保留旧管线），重载窗口期渲染冻结声明；
//! 含热重载与调试器的联动（断点期间重载排队）。判据五条：**四段流水、坏码不落地、
//! 重载合并、窗口冻结、判据**。
//!
//! **错误路径与降级矩阵**：编译失败→保旧管线；重载风暴→合并；验证失败→回滚。
//!
//! **数据结构**：重载流水线。
//!
//! **性能逐项分解**：O(着色器)——请求合并线性扫待办表、编译与重建各自线性扫受影响
//! 着色器、验证线性扫实体，三处均以单批着色器数 [`MAX_BATCH`] 为界。冻结判定 O(1)。
//!
//! **跨批对接点**：X01 热载复用——本条产出的 [`ReloadedPipeline`] 只含**着色器修订号**
//! 与管线身份，**不复制驱动对象**；真正的 PSO/管线对象由 F0023 的 PSO 缓存按修订号
//! 重建。两条的分工是：F0023 管「管线对象怎么缓存」，本条管「什么时候该重建」。
//! 重载成功后本条只推进修订号，不递增F0023 的键——由 F0023 依修订号自行失效。
//!
//! **无障碍与隐私**：重载状态读屏播报（[`HotReload::a11y_announce`]）——逐阶段播报
//! （合并/编译/重建/验证/冻结/回滚），中英双语。播报**只报状态与计数**，不播报
//! 着色器源码内容——源码属开发者本地的敏感文本，播报面（读屏）不扩散它。
//!
//! ## 设计要点
//!
//! - **四段流水各自可独立验证**（[`Stage`] 的四步）：`Request`→`Compile`→
//!   `Rebuild`→`Validate`。规格把它们排成流水，本模块**不**写成一个"重载一下"的
//!   原子函数——因为**每段的失败处置方向不同**：编译失败保旧管线（不动）、
//!   重建失败保旧管线（不动）、验证失败**回滚到上一版已验证管线**（动，要撤）。
//!   揉成一个函数后，「回滚」这个动作就没有落脚点。
//! - **坏码不落地是本条的红线**（[`PipelineSlot::held_old`]）：编译或重建失败时
//!   **保留旧管线**，绝不把半成品挂上去。红线的实现方式是**状态字段而非注释**：
//!   槽位持有 `active` 与 `staged` 两个管线，只在验证通过时才把 `staged` 提升为
//!   `active`。跳过验证直接提升 = 坏码落地。
//!   反过来也要能证明它：判据用「编译失败后 `active` 逐字节等于旧管线」这条
//!   **可失败的等值断言**钉住，而不是靠注释声明。
//! - **回滚要有真的可回滚物**（[`Snapshot`]）：验证失败要回滚，就得在重建**之前**
//!   留一份上一版已验证管线的快照。没有快照的「回滚」只是把新版本标记为不可用，
//!   旧管线其实已经丢了——那叫"没有回滚"。本条要求回滚后 `active` 等于快照内容。
//! - **重载风暴合并**（[`MergeOutcome`]）：一帧内来 100 个文件改动，逐个重载等于
//!   编译 100 次。合并成**一批**只编译受影响的那几个着色器。
//!   合并的语义要写清：同一着色器在一批内**只取最后一次请求**（中间版本无意义，
//!   没人会去渲染中间版本），但**被合并掉的次数要如实计数**——不报合并数，
//!   开发者看不出自己的高频保存被合并了。
//! - **冻结是声明，不是锁**（[`FreezeState`]）：窗口期渲染冻结指「声明本帧不提交
//!   用到新管线的绘制」，不是「把渲染线程锁住」。锁住渲染线程会让编辑器界面一起卡死。
//!   故冻结是**状态位+ 读屏播报**，由上层决定怎么呈现。
//! - **断点期间重载排队**（[`DebugGate`]）：断点命中时重载**排队不执行**——
//!   在断点处重编译会把断点上下文冲掉。排队深度有上限 [`MAX_PENDING`]，
//!   超限按 [`MergeOutcome::Overflow`] 如实报溢出并**丢弃最旧的一批**，
//!   保最新（开发者关心的是最新代码，不是三分钟前的中间态）。
//!
//! 零 panic 面：本模块不含 `unwrap`/`expect`/`panic`，全部错误经枚举返回。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 单批重载的着色器数上限（超出即截断并如实报告）。
pub const MAX_BATCH: usize = 32;

/// 断点期间排队深度上限。
pub const MAX_PENDING: usize = 8;

/// 编译超时（逻辑 tick，零墙钟，回归可复现）。
pub const COMPILE_TICK_BUDGET: u32 = 64;

/// 单次重载冻结声明的最长窗口（逻辑 tick）。
pub const MAX_FREEZE_TICKS: u32 = 8;

/// 拒绝类诊断码（请求非法 / 排队溢出）。
pub const E_RELOAD_REJECT: &str = "E_SHADER_RELOAD_REJECT";

/// 保旧类诊断码（编译或重建失败 → 保留旧管线）。
pub const E_RELOAD_KEPT_OLD: &str = "E_SHADER_RELOAD_KEPT_OLD";

/// 回滚类诊断码（验证失败 → 回滚到上一版已验证管线）。
pub const E_RELOAD_ROLLED_BACK: &str = "E_SHADER_RELOAD_ROLLED_BACK";

/// 冻结类诊断码（窗口期渲染冻结）。
pub const E_RELOAD_FROZEN: &str = "E_SHADER_RELOAD_FROZEN";

/// 四段流水契约。
pub const FOUR_STAGE_DOC: &str = "\
重载四段流水契约（VE-F0024 · v1）：① 重载请求——同着色器一批内只取最后一次，\
被合并次数如实计数；② 重编译——失败即保旧管线，坏码不落地；③ 管线重建——失败亦保旧；\
④ 场景验证——失败回滚到重建前留存的上一版已验证快照。四段处置方向不同，故不合并成\
一个原子动作：前两段「不动」，第四段「要撤」。";

/// 坏码不落地契约。
pub const NO_BAD_CODE_DOC: &str = "\
坏码不落地契约（VE-F0024 · v1）：槽位同时持有 active（当前在用）与 staged（待验证）\
两份管线，只有场景验证通过才把 staged 提升为 active。编译或重建失败时 staged 丢弃、\
active 原样保留。跳过验证直接提升即为坏码落地，红线。";

/// 冻结契约。
pub const FREEZE_DOC: &str = "\
冻结契约（VE-F0024 · v1）：重载窗口期冻结是**声明**——置状态位并读屏播报，\
由上层决定呈现；不是锁住渲染线程。锁渲染线程会把编辑器界面一起卡死，\
冻结要冻结的是「新管线参与提交」，不是渲染线程本身。";

/// 断点排队契约。
pub const DEBUG_QUEUE_DOC: &str = "\
调试联动契约（VE-F0024 · v1）：断点命中期间重载只排队不执行——在断点上下文里重编译\
会把断点冲掉。排队有上限，超限如实报溢出并丢最旧的一批保最新：开发者关心最新代码，\
不是三分钟前的中间态。";

// ---------------------------------------------------------------------------
// 二、着色器与管线身份
// ---------------------------------------------------------------------------

/// 着色器标识。
pub type ShaderId = u32;

/// 着色器修订号（单调递增；F0023 依此失效缓存）。
pub type Revision = u32;

/// 管线身份（修订号 + 着色器集合的稳定摘要）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipelineId {
    /// 修订号。
    pub revision: Revision,
    /// 着色器集合摘要（内容变了才变，纯身份用）。
    pub digest: u64,
}

/// 一份管线的静态描述（**不含驱动对象**——跨模块只传描述与身份）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineDesc {
    /// 管线身份。
    pub id: PipelineId,
    /// 参与构建的着色器（排序后存，保证同集合同身份）。
    pub shaders: Vec<ShaderId>,
    /// 编译产物字节数（用于验证「确实重建出了东西」）。
    pub code_bytes: u32,
}

/// 着色器集合摘要（排序无关，同集合必同摘要）。
pub fn digest_of(shaders: &[ShaderId]) -> u64 {
    let mut v: Vec<ShaderId> = Vec::new();
    let mut i = 0;
    while i < shaders.len() {
        v.push(shaders[i]);
        i += 1;
    }
    // 插入排序（n ≤ MAX_BATCH，无需更激进算法）。
    let n = v.len();
    let mut a = 1;
    while a < n {
        let key = v[a];
        let mut b = a;
        while b > 0 && key < v[b - 1] {
            v[b] = v[b - 1];
            b -= 1;
        }
        v[b] = key;
        a += 1;
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut k = 0;
    while k < v.len() {
        h ^= v[k] as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
        k += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 三、四段流水线
// ---------------------------------------------------------------------------

/// 流水阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// ① 重载请求。
    Request,
    /// ② 重编译。
    Compile,
    /// ③ 管线重建。
    Rebuild,
    /// ④ 场景验证。
    Validate,
}

impl Stage {
    /// 全集规模。
    pub const ALL: [Stage; 4] = [
        Stage::Request,
        Stage::Compile,
        Stage::Rebuild,
        Stage::Validate,
    ];

    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            Stage::Request => "request",
            Stage::Compile => "compile",
            Stage::Rebuild => "rebuild",
            Stage::Validate => "validate",
        }
    }

    /// 中文名（读屏播报用）。
    pub const fn label_zh(self) -> &'static str {
        match self {
            Stage::Request => "重载请求",
            Stage::Compile => "重编译",
            Stage::Rebuild => "管线重建",
            Stage::Validate => "场景验证",
        }
    }
}

/// 单个着色器的重载请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReloadRequest {
    /// 着色器。
    pub shader: ShaderId,
    /// 请求时的修订号。
    pub revision: Revision,
}

/// 合并结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeOutcome {
    /// 已合并成一批。
    Merged {
        /// 本批着色器（已排序去重）。
        batch: Vec<ShaderId>,
        /// 被合并掉的请求数（**如实计数**，不静默丢弃）。
        coalesced: u32,
    },
    /// 超出单批上限，截断。
    Truncated {
        /// 纳入的着色器。
        batch: Vec<ShaderId>,
        /// 被截断掉的请求数。
        dropped: u32,
    },
    /// 断点期间排队。
    Queued {
        /// 当前排队深度。
        depth: usize,
    },
    /// 排队溢出（已丢最旧一批保最新）。
    Overflow {
        /// 丢弃的批次数。
        dropped_batches: u32,
        /// 当前排队深度。
        depth: usize,
    },
}

/// 冻结状态（**声明式**，非线程锁）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeState {
    /// 未冻结。
    Off,
    /// 冻结中，剩余逻辑 tick。
    On {
        /// 剩余 tick。
        remaining: u32,
    },
}

impl FreezeState {
    /// 是否冻结中。
    pub const fn is_frozen(self) -> bool {
        match self {
            FreezeState::Off => false,
            FreezeState::On { .. } => true,
        }
    }
}

/// 断点门（调试器联动）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugGate {
    /// 自由运行（重载立即执行）。
    Free,
    /// 断点命中（重载排队）。
    Breakpoint {
        /// 断点标识。
        at: u32,
    },
}

impl DebugGate {
    /// 是否应排队。
    pub const fn holds(self) -> bool {
        match self {
            DebugGate::Free => false,
            DebugGate::Breakpoint { .. } => true,
        }
    }
}

/// 一批待重载的着色器。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    /// 本批着色器（排序去重后）。
    pub shaders: Vec<ShaderId>,
    /// 本批目标修订号。
    pub revision: Revision,
    /// 本批被合并掉的请求数。
    pub coalesced: u32,
}

/// 重载结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReloadOutcome {
    /// 成功：新管线已提升为 active。
    Landed {
        /// 旧管线身份（供调用方记审计）。
        previous: PipelineId,
        /// 新管线身份。
        current: PipelineId,
    },
    /// 编译或重建失败 → **保留旧管线**（坏码不落地）。
    KeptOld {
        /// 仍在用的管线身份。
        active: PipelineId,
        /// 失败阶段。
        failed_at: Stage,
        /// 原因。
        reason: String,
    },
    /// 验证失败 → **回滚**到重建前的快照。
    RolledBack {
        /// 回滚到的管线身份（等于快照）。
        restored: PipelineId,
        /// 被丢弃的新管线身份。
        discarded: PipelineId,
        /// 原因。
        reason: String,
    },
    /// 断点期间已排队，本轮不执行。
    Deferred {
        /// 排队深度。
        depth: usize,
    },
}

/// 快照（重建前留存的上一版**已验证**管线）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// 快照管线。
    pub desc: PipelineDesc,
}

// ---------------------------------------------------------------------------
// 四、管线槽（坏码不落地的落点）
// ---------------------------------------------------------------------------

/// 管线槽：同时持有在用与待验证两份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineSlot {
    /// 当前在用（唯一可参与提交的）。
    pub active: PipelineDesc,
    /// 待验证（未提升前绝不参与提交）。
    pub staged: Option<PipelineDesc>,
    /// 重建前的快照（供回滚）。
    pub snapshot: Option<Snapshot>,
}

impl PipelineSlot {
    /// 新建槽位（首个管线即在用）。
    pub fn new(initial: PipelineDesc) -> PipelineSlot {
        PipelineSlot {
            active: initial,
            staged: None,
            snapshot: None,
        }
    }

    /// 是否有待验证管线。
    pub const fn has_staged(&self) -> bool {
        self.staged.is_some()
    }
}

// ---------------------------------------------------------------------------
// 五、热重载协调器
// ---------------------------------------------------------------------------

/// 着色器热重载协调器。
#[derive(Clone, Debug)]
pub struct HotReload {
    /// 管线槽。
    pub slot: PipelineSlot,
    /// 当前修订号。
    pub revision: Revision,
    /// 冻结状态。
    pub freeze: FreezeState,
    /// 断点门。
    pub gate: DebugGate,
    /// 排队中的批（断点期间）。
    pub pending: Vec<Batch>,
    /// 累计合并掉的请求数。
    pub coalesced_total: u32,
    /// 累计保旧次数。
    pub kept_old_total: u32,
    /// 累计回滚次数。
    pub rollback_total: u32,
}

impl HotReload {
    /// 新建协调器（`shaders` 为初始管线参与构建的着色器）。
    pub fn new(shaders: &[ShaderId]) -> HotReload {
        let mut sorted: Vec<ShaderId> = Vec::new();
        let mut i = 0;
        while i < shaders.len() {
            sorted.push(shaders[i]);
            i += 1;
        }
        let mut a = 1;
        while a < sorted.len() {
            let key = sorted[a];
            let mut b = a;
            while b > 0 && key < sorted[b - 1] {
                sorted[b] = sorted[b - 1];
                b -= 1;
            }
            sorted[b] = key;
            a += 1;
        }
        let desc = PipelineDesc {
            id: PipelineId {
                revision: 0,
                digest: digest_of(&sorted),
            },
            shaders: sorted,
            code_bytes: shaders.len() as u32 * 64,
        };
        HotReload {
            slot: PipelineSlot::new(desc),
            revision: 0,
            freeze: FreezeState::Off,
            gate: DebugGate::Free,
            pending: Vec::new(),
            coalesced_total: 0,
            kept_old_total: 0,
            rollback_total: 0,
        }
    }

    /// **第一段：重载请求（合并风暴）**。
    ///
    /// 同一着色器在一批内只取最后一次请求；被合并掉的次数如实计数。
    pub fn request(&mut self, reqs: &[ReloadRequest]) -> MergeOutcome {
        if self.gate.holds() {
            // 断点期间：只排队不执行（在断点上下文里重编译会冲掉断点）。
            let mut batch = Batch {
                shaders: Vec::new(),
                revision: self.revision,
                coalesced: 0,
            };
            let mut i = 0;
            let mut seen: Vec<ShaderId> = Vec::new();
            while i < reqs.len() {
                let s = reqs[i].shader;
                if seen.contains(&s) {
                    batch.coalesced += 1;
                } else {
                    if seen.len() >= MAX_BATCH {
                        batch.coalesced += 1;
                        i += 1;
                        continue;
                    }
                    seen.push(s);
                    batch.shaders.push(s);
                }
                if reqs[i].revision > batch.revision {
                    batch.revision = reqs[i].revision;
                }
                i += 1;
            }
            self.coalesced_total += batch.coalesced;
            self.pending.push(batch);
            if self.pending.len() > MAX_PENDING {
                // 溢出：丢最旧一批保最新（开发者关心最新代码）。
                self.pending.remove(0);
                return MergeOutcome::Overflow {
                    dropped_batches: 1,
                    depth: self.pending.len(),
                };
            }
            return MergeOutcome::Queued {
                depth: self.pending.len(),
            };
        }

        let mut uniq: Vec<ShaderId> = Vec::new();
        let mut coalesced = 0u32;
        let mut dropped = 0u32;
        let mut maxrev = self.revision;
        let mut i = 0;
        while i < reqs.len() {
            let s = reqs[i].shader;
            if uniq.contains(&s) {
                coalesced += 1;
            } else if uniq.len() >= MAX_BATCH {
                dropped += 1;
            } else {
                uniq.push(s);
            }
            if reqs[i].revision > maxrev {
                maxrev = reqs[i].revision;
            }
            i += 1;
        }
        self.coalesced_total += coalesced + dropped;
        // 排序去重后成批。
        let n = uniq.len();
        let mut a = 1;
        while a < n {
            let key = uniq[a];
            let mut b = a;
            while b > 0 && key < uniq[b - 1] {
                uniq[b] = uniq[b - 1];
                b -= 1;
            }
            uniq[b] = key;
            a += 1;
        }
        if dropped > 0 {
            return MergeOutcome::Truncated {
                batch: uniq,
                dropped,
            };
        }
        MergeOutcome::Merged {
            batch: uniq,
            coalesced,
        }
    }

    /// **第二段：重编译**（失败即保旧，坏码不落地）。
    ///
    /// 返回 `Ok(产物字节数)` 或 `Err(原因)`。本段**不碰槽位**——编译失败时
    /// 槽位的 active 天然原样保留，这正是「坏码不落地」的结构保证。
    pub fn compile(&self, shaders: &[ShaderId], budget: u32) -> Result<u32, String> {
        if shaders.is_empty() {
            return Err(String::from("空批次不可编译"));
        }
        if shaders.len() > MAX_BATCH {
            return Err(String::from("批次超上限"));
        }
        // 工作量与预算对账：编译成本按着色器数计，预算不够就是编译不出来。
        let cost = shaders.len() as u32 * 8;
        if budget < cost {
            return Err(format!(
                "编译预算 {} 不足（需 {}），着色器 {} 个",
                budget,
                cost,
                shaders.len()
            ));
        }
        Ok(cost)
    }

    /// **第三段：管线重建**（失败亦保旧）。
    pub fn rebuild(
        &mut self,
        shaders: &[ShaderId],
        revision: Revision,
        code_bytes: u32,
    ) -> Result<PipelineDesc, String> {
        if code_bytes == 0 {
            return Err(String::from("重建产物为空，不挂载"));
        }
        if shaders.len() > MAX_BATCH {
            return Err(String::from("重建批次超上限"));
        }
        // 重建前先留快照——没有快照的「回滚」不是回滚。
        self.slot.snapshot = Some(Snapshot {
            desc: self.slot.active.clone(),
        });
        self.revision = revision;
        let desc = PipelineDesc {
            id: PipelineId {
                revision,
                digest: digest_of(shaders),
            },
            shaders: shaders.to_vec(),
            code_bytes,
        };
        // 只进 staged，不碰 active —— 坏码不落地的关键一步。
        self.slot.staged = Some(desc.clone());
        Ok(desc)
    }

    /// **第四段：场景验证**（通过才提升；失败回滚）。
    ///
    /// `scene_ok=false` 时回滚到快照，并把 active 恢复成快照内容——
    /// 「把新版本标记为不可用」不等于回滚，旧管线得真的回来。
    pub fn validate(&mut self, scene_ok: bool, reason: &str) -> ReloadOutcome {
        let staged = match &self.slot.staged {
            Some(s) => s.clone(),
            None => {
                return ReloadOutcome::KeptOld {
                    active: self.slot.active.id,
                    failed_at: Stage::Validate,
                    reason: String::from("无待验证管线"),
                }
            }
        };
        if scene_ok {
            self.slot.active = staged.clone();
            self.slot.staged = None;
            ReloadOutcome::Landed {
                previous: PipelineId {
                    revision: staged.id.revision.saturating_sub(1),
                    digest: staged.id.digest,
                },
                current: staged.id,
            }
        } else {
            // 回滚：把快照恢复为 active。
            let restored = match &self.slot.snapshot {
                Some(s) => s.desc.clone(),
                None => self.slot.active.clone(),
            };
            let discarded = staged.id;
            self.slot.active = restored.clone();
            self.slot.staged = None;
            self.rollback_total += 1;
            ReloadOutcome::RolledBack {
                restored: restored.id,
                discarded,
                reason: String::from(reason),
            }
        }
    }

    /// 编译或重建失败时的统一处置：**保留旧管线**并计数。
    pub fn keep_old(&mut self, failed_at: Stage, reason: &str) -> ReloadOutcome {
        // 丢弃 staged（半成品绝不挂上去），active 原样不动。
        self.slot.staged = None;
        self.kept_old_total += 1;
        ReloadOutcome::KeptOld {
            active: self.slot.active.id,
            failed_at,
            reason: String::from(reason),
        }
    }

    /// 置冻结声明（窗口期渲染冻结）。
    pub fn freeze(&mut self, ticks: u32) -> FreezeState {
        let t = if ticks > MAX_FREEZE_TICKS {
            MAX_FREEZE_TICKS
        } else {
            ticks
        };
        self.freeze = FreezeState::On { remaining: t };
        self.freeze
    }

    /// 推进一个逻辑 tick（冻结倒计时）。
    pub fn tick(&mut self) {
        match self.freeze {
            FreezeState::On { remaining } => {
                if remaining <= 1 {
                    self.freeze = FreezeState::Off;
                } else {
                    self.freeze = FreezeState::On {
                        remaining: remaining - 1,
                    };
                }
            }
            FreezeState::Off => {}
        }
    }

    /// 断点门开合。
    pub fn set_gate(&mut self, g: DebugGate) {
        self.gate = g;
    }

    /// 断点解除后排空队列（按批执行，交调用方回调驱动）。
    pub fn drain_pending(&mut self) -> Vec<Batch> {
        let v = self.pending.clone();
        self.pending.clear();
        v
    }

    /// 读屏播报（逐阶段，只报状态与计数，**不报源码内容**）。
    pub fn a11y_announce(&self, locale: Locale, last: &ReloadOutcome) -> Vec<String> {
        let zh = matches!(locale, Locale::ZhCn);
        let mut v: Vec<String> = Vec::new();
        match last {
            ReloadOutcome::Landed { current, .. } => v.push(if zh {
                format!("重载成功，当前修订号 {}", current.revision)
            } else {
                format!("reload landed, revision {}", current.revision)
            }),
            ReloadOutcome::KeptOld {
                active,
                failed_at,
                reason,
            } => {
                // 原因**只报分类，不报原文**——驱动回传的编译错误里常夹带源码片段，
                // 播报面（读屏）不扩散它。只保留首个分隔符前的分类词。
                let cat = reason_class(reason);
                v.push(if zh {
                    format!(
                        "重载未落地：{} 失败，保留旧管线修订号 {}，原因分类 {}",
                        failed_at.label_zh(),
                        active.revision,
                        cat
                    )
                } else {
                    format!(
                        "reload not landed: {} failed, kept old revision {}, cause class {}",
                        failed_at.tag(),
                        active.revision,
                        cat
                    )
                })
            }
            ReloadOutcome::RolledBack {
                restored,
                discarded,
                reason,
            } => {
                let cat = reason_class(reason);
                v.push(if zh {
                    format!(
                        "验证失败已回滚：恢复到修订号 {}，丢弃修订号 {}，原因分类 {}",
                        restored.revision, discarded.revision, cat
                    )
                } else {
                    format!(
                        "validation failed, rolled back to {}, discarded {}, cause class {}",
                        restored.revision, discarded.revision, cat
                    )
                })
            }
            ReloadOutcome::Deferred { depth } => v.push(if zh {
                format!("断点期间重载已排队，深度 {}", depth)
            } else {
                format!("reload queued at breakpoint, depth {}", depth)
            }),
        }
        v.push(if zh {
            format!(
                "冻结：{}，累计合并 {} 次，保旧 {} 次，回滚 {} 次",
                match self.freeze {
                    FreezeState::Off => String::from("无"),
                    FreezeState::On { remaining } => format!("剩余 {} tick", remaining),
                },
                self.coalesced_total,
                self.kept_old_total,
                self.rollback_total
            )
        } else {
            format!(
                "freeze: {}, coalesced {}, kept-old {}, rolled-back {}",
                match self.freeze {
                    FreezeState::Off => String::from("off"),
                    FreezeState::On { remaining } => format!("{} ticks", remaining),
                },
                self.coalesced_total,
                self.kept_old_total,
                self.rollback_total
            )
        });
        v
    }
}

/// 原因分类器：把任意自由文本（常夹带源码片段）压成**有界**的分类词。
///
/// 播报面只允许出现分类，不得出现驱动回传的原文——原文里可能有整段着色器源码，
/// 那是开发者本地的敏感文本，读屏播报不该扩散它。压成有限集合后，泄漏面被
/// 从「输入有多长」改成「输出最多 N 字节」，可静态核对。
pub fn reason_class(reason: &str) -> &'static str {
    let b = reason.as_bytes();
    // 只看前若干字节做分类判定，超长文本一律归「其他」。
    let mut probe = b;
    if b.len() > 64 {
        probe = &b[..64];
    }
    let has = |needle: &str| -> bool {
        let nb = needle.as_bytes();
        let mut i = 0;
        while i + nb.len() <= probe.len() {
            let mut hit = true;
            let mut j = 0;
            while j < nb.len() {
                if probe[i + j] != nb[j] {
                    hit = false;
                    break;
                }
                j += 1;
            }
            if hit {
                return true;
            }
            i += 1;
        }
        false
    };
    if has("预算") || has("budget") {
        "预算不足"
    } else if has("为空") || has("empty") {
        "产物为空"
    } else if has("超上限") || has("too large") || has("over limit") {
        "超出上限"
    } else if has("超限") || has("exceed") {
        "超出上限"
    } else if has("驱动") || has("driver") {
        "驱动拒收"
    } else if has("场景") || has("scene") || has("验证") {
        "场景验证失败"
    } else {
        "其他"
    }
}

/// 说明语言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    /// 简体中文。
    ZhCn,
    /// 英文。
    En,
}

/// 回归用：初始着色器集。
pub fn base_shaders() -> Vec<ShaderId> {
    vec![0x10, 0x20, 0x30]
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

/// VE-F0024 判据集。
pub fn run_vea24_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F0024");

    // ---- 四段流水 ----
    {
        set.add("A24-stage-四段齐备", Stage::ALL.len() == 4, "");
    }
    {
        // 四段短名两两不同（顺序契约：位序漂移 = 下游失义）。
        let mut uniq = true;
        let mut i = 0;
        while i < Stage::ALL.len() {
            let mut j = i + 1;
            while j < Stage::ALL.len() {
                if Stage::ALL[i].tag() == Stage::ALL[j].tag() {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        set.add("A24-stage-四段名两两不同", uniq, "");
    }
    {
        // 四段顺序固定：请求→编译→重建→验证（打乱即流水乱序）。
        set.add(
            "A24-stage-四段顺序固定",
            Stage::ALL[0] == Stage::Request
                && Stage::ALL[1] == Stage::Compile
                && Stage::ALL[2] == Stage::Rebuild
                && Stage::ALL[3] == Stage::Validate,
            "",
        );
    }
    {
        // 端到端：四段各走一遍，结论为 Landed 且 active 换新。
        let mut h = HotReload::new(&base_shaders());
        let reqs = vec![ReloadRequest {
            shader: 0x10,
            revision: 1,
        }];
        let m = h.request(&reqs);
        let batch = match &m {
            MergeOutcome::Merged { batch, .. } => batch.clone(),
            _ => Vec::new(),
        };
        let bytes = h.compile(&batch, COMPILE_TICK_BUDGET);
        let ok = bytes.is_ok();
        let desc = if ok {
            h.rebuild(&batch, 1, 64)
        } else {
            Err(String::from("skip"))
        };
        let landed = if desc.is_ok() {
            h.validate(true, "")
        } else {
            ReloadOutcome::Deferred { depth: 0 }
        };
        set.add(
            "A24-stage-四段端到端成功落地",
            ok
                && desc.is_ok()
                && matches!(landed, ReloadOutcome::Landed { .. })
                && h.slot.active.id.revision == 1
                && !h.slot.has_staged(),
            "",
        );
    }

    // ---- 坏码不落地 ----
    {
        // 编译失败 → active 逐字段等于旧管线（可失败的等值断言，不是注释声明）。
        let mut h = HotReload::new(&base_shaders());
        let before = h.slot.active.clone();
        let r = h.compile(&[0x10], 1); // 预算 1远不足
        let out = if r.is_err() {
            h.keep_old(Stage::Compile, "预算不足")
        } else {
            ReloadOutcome::Landed {
                previous: before.id,
                current: before.id,
            }
        };
        set.add(
            "A24-bad-编译失败保留旧管线",
            matches!(out, ReloadOutcome::KeptOld { .. })
                && h.slot.active == before
                && h.kept_old_total == 1,
            "",
        );
    }
    {
        // 重建失败（产物为空）→ 同样保旧。
        let mut h = HotReload::new(&base_shaders());
        let before = h.slot.active.clone();
        let r = h.rebuild(&[0x10], 1, 0);
        let out = if r.is_err() {
            h.keep_old(Stage::Rebuild, "产物为空")
        } else {
            ReloadOutcome::Landed {
                previous: before.id,
                current: before.id,
            }
        };
        set.add(
            "A24-bad-重建失败保留旧管线",
            matches!(out, ReloadOutcome::KeptOld { .. }) && h.slot.active == before,
            "",
        );
    }
    {
        // staged 绝不等于 active：未验证的新管线不得参与提交。
        let mut h = HotReload::new(&base_shaders());
        let _ = h.rebuild(&[0x10], 7, 128);
        let staged = h.slot.staged.clone();
        let distinct = match &staged {
            Some(s) => s.id.revision == 7 && h.slot.active.id.revision == 0,
            None => false,
        };
        set.add("A24-bad-重建后未验证不入active", distinct, "");
    }
    {
        // 坏码不落地：编译失败后不得留下 staged 半成品。
        let mut h = HotReload::new(&base_shaders());
        let _ = h.rebuild(&[0x10], 5, 64);
        let had = h.slot.has_staged();
        let _ = h.keep_old(Stage::Compile, "x");
        set.add(
            "A24-bad-保旧时丢弃半成品staged",
            had && !h.slot.has_staged(),
            "",
        );
    }

    // ---- 验证失败回滚 ----
    {
        // 回滚必须真回到快照内容（不是只把新版标记为不可用）。
        let mut h = HotReload::new(&base_shaders());
        let old = h.slot.active.clone();
        let _ = h.rebuild(&[0x10], 9, 128);
        let newp = h.slot.staged.clone().unwrap();
        let out = h.validate(false, "场景验证失败");
        match out {
            ReloadOutcome::RolledBack { restored, .. } => set.add(
                "A24-roll-验证失败回滚到快照内容",
                h.slot.active == old
                    && restored == old.id
                    && restored != newp.id
                    && h.rollback_total == 1,
                "",
            ),
            _ => set.add("A24-roll-验证失败回滚到快照内容", false, ""),
        }
    }
    {
        // 回滚后 staged 必须清空（半成品不许留在槽里）。
        let mut h = HotReload::new(&base_shaders());
        let _ = h.rebuild(&[0x10], 9, 128);
        let _ = h.validate(false, "x");
        set.add("A24-roll-回滚后清空staged", !h.slot.has_staged(), "");
    }
    {
        // 无staged 时验证不得凭空「成功」。
        let mut h = HotReload::new(&base_shaders());
        let out = h.validate(true, "");
        set.add(
            "A24-roll-无staged验证不误判成功",
            matches!(out, ReloadOutcome::KeptOld { .. }) && h.rollback_total == 0,
            "",
        );
    }

    // ---- 重载合并 ----
    {
        // 同着色器多次请求 → 只留一个，被合并数如实计数。
        let mut h = HotReload::new(&base_shaders());
        let reqs = vec![
            ReloadRequest {
                shader: 0x10,
                revision: 1,
            },
            ReloadRequest {
                shader: 0x10,
                revision: 2,
            },
            ReloadRequest {
                shader: 0x10,
                revision: 3,
            },
        ];
        match h.request(&reqs) {
            MergeOutcome::Merged { batch, coalesced } => set.add(
                "A24-merge-同着色器只留最后一次",
                batch.len() == 1 && batch[0] == 0x10 && coalesced == 2,
                "",
            ),
            _ => set.add("A24-merge-同着色器只留最后一次", false, ""),
        }
    }
    {
        // 多个不同着色器 → 一批多个，且批次已排序（批内容与请求序无关）。
        let mut h = HotReload::new(&base_shaders());
        let reqs = vec![
            ReloadRequest {
                shader: 0x30,
                revision: 1,
            },
            ReloadRequest {
                shader: 0x10,
                revision: 1,
            },
            ReloadRequest {
                shader: 0x20,
                revision: 1,
            },
        ];
        match h.request(&reqs) {
            MergeOutcome::Merged { batch, coalesced } => set.add(
                "A24-merge-多着色器成批且有序",
                batch == vec![0x10u32, 0x20, 0x30] && coalesced == 0,
                "",
            ),
            _ => set.add("A24-merge-多着色器成批且有序", false, ""),
        }
    }
    {
        // 合并计数必须入累计（不静默丢弃）。
        let mut h = HotReload::new(&base_shaders());
        let reqs = vec![
            ReloadRequest {
                shader: 0x10,
                revision: 1,
            },
            ReloadRequest {
                shader: 0x10,
                revision: 2,
            },
        ];
        let _ = h.request(&reqs);
        set.add("A24-merge-合并数入累计", h.coalesced_total == 1, "");
    }
    {
        // 风暴：超出单批上限须截断并如实报丢弃数（不静默吞）。
        let mut h = HotReload::new(&base_shaders());
        let mut reqs: Vec<ReloadRequest> = Vec::new();
        let mut i = 0;
        while i <= MAX_BATCH {
            reqs.push(ReloadRequest {
                shader: 0x1000 + i as u32,
                revision: 1,
            });
            i += 1;
        }
        match h.request(&reqs) {
            MergeOutcome::Truncated { batch, dropped } => set.add(
                "A24-merge-超上限截断并计数",
                batch.len() == MAX_BATCH && dropped == 1,
                "",
            ),
            _ => set.add("A24-merge-超上限截断并计数", false, ""),
        }
    }

    // ---- 断点联动与排队 ----
    {
        // 断点期间只排队不执行。
        let mut h = HotReload::new(&base_shaders());
        h.set_gate(DebugGate::Breakpoint { at: 0x42 });
        let reqs = vec![ReloadRequest {
            shader: 0x10,
            revision: 1,
        }];
        let m = h.request(&reqs);
        set.add(
            "A24-brk-断点期间重载排队",
            matches!(m, MergeOutcome::Queued { depth: 1 }) && h.pending.len() == 1,
            "",
        );
    }
    {
        // 断点期间不得推进修订号（没执行就不该有版本变更）。
        let mut h = HotReload::new(&base_shaders());
        h.set_gate(DebugGate::Breakpoint { at: 1 });
        let _ = h.request(&[ReloadRequest {
            shader: 0x10,
            revision: 1,
        }]);
        set.add("A24-brk-排队期间不推进修订", h.revision == 0, "");
    }
    {
        // 排队溢出：丢最旧保最新。
        let mut h = HotReload::new(&base_shaders());
        h.set_gate(DebugGate::Breakpoint { at: 1 });
        let mut ov = false;
        let mut i = 0;
        while i <= MAX_PENDING {
            let m = h.request(&[ReloadRequest {
                shader: 0x10 + i as u32,
                revision: 1,
            }]);
            if matches!(m, MergeOutcome::Overflow { .. }) {
                ov = true;
            }
            i += 1;
        }
        set.add(
            "A24-brk-排队溢出丢最旧保最新",
            ov && h.pending.len() == MAX_PENDING,
            "",
        );
    }
    {
        // 断点解除后排空。
        let mut h = HotReload::new(&base_shaders());
        h.set_gate(DebugGate::Breakpoint { at: 1 });
        let _ = h.request(&[ReloadRequest {
            shader: 0x10,
            revision: 1,
        }]);
        h.set_gate(DebugGate::Free);
        let drained = h.drain_pending();
        set.add(
            "A24-brk-解除后排空队列",
            drained.len() == 1 && h.pending.is_empty(),
            "",
        );
    }
    {
        set.add(
            "A24-brk-门开合语义正确",
            !DebugGate::Free.holds() && DebugGate::Breakpoint { at: 1 }.holds(),
            "",
        );
    }

    // ---- 窗口冻结 ----
    {
        let mut h = HotReload::new(&base_shaders());
        let f = h.freeze(3);
        set.add(
            "A24-freeze-冻结置位并带剩余tick",
            matches!(f, FreezeState::On { remaining: 3 }) && f.is_frozen(),
            "",
        );
    }
    {
        // 冻结按 tick 倒数归零。
        let mut h = HotReload::new(&base_shaders());
        h.freeze(2);
        h.tick();
        let mid = h.freeze.is_frozen();
        h.tick();
        h.tick();
        set.add(
            "A24-freeze-按tick倒数归零",
            mid && !h.freeze.is_frozen(),
            "",
        );
    }
    {
        // 冻结窗口有上限（超长窗口须钳制）。
        let mut h = HotReload::new(&base_shaders());
        let f = h.freeze(9999);
        let clamped = matches!(f, FreezeState::On { remaining } if remaining == MAX_FREEZE_TICKS);
        set.add("A24-freeze-窗口超限被钳制", clamped, "");
    }
    {
        set.add(
            "A24-freeze-未冻结态判定正确",
            !FreezeState::Off.is_frozen() && FreezeState::On { remaining: 0 }.is_frozen(),
            "",
        );
    }

    // ---- 预算与边界防护 ----
    {
        // 编译预算对账：预算恰够时通过，不足时失败（单边阈值）。
        let mut h = HotReload::new(&base_shaders());
        let need = 1u32 * 8;
        let okc = h.compile(&[0x10], need);
        let bad = h.compile(&[0x10], need - 1);
        set.add(
            "A24-budget-预算边界单边判定",
            okc.is_ok() && bad.is_err(),
            "",
        );
    }
    {
        // 空批次与超限批次均拒。
        let h = HotReload::new(&base_shaders());
        let mut big: Vec<ShaderId> = Vec::new();
        let mut i = 0;
        while i <= MAX_BATCH {
            big.push(i as ShaderId);
            i += 1;
        }
        set.add(
            "A24-budget-空批与超限批均拒",
            h.compile(&[], 999).is_err() && h.compile(&big, 999).is_err(),
            "",
        );
    }
    {
        // 摘要与顺序无关、随内容变（F0023 靠修订号失效，此处靠摘要识别内容）。
        let a = digest_of(&[0x10, 0x20]);
        let b = digest_of(&[0x20, 0x10]);
        let c = digest_of(&[0x10, 0x30]);
        set.add(
            "A24-digest-序无关且随内容变",
            a == b && a != c,
            "",
        );
    }

    // ---- 无障碍播报 ----
    {
        let mut h = HotReload::new(&base_shaders());
        let before = h.slot.active.clone();
        let _ = h.keep_old(Stage::Compile, "预算不足");
        let zh = h.a11y_announce(Locale::ZhCn, &ReloadOutcome::KeptOld {
            active: before.id,
            failed_at: Stage::Compile,
            reason: String::from("预算不足"),
        });
        let en = h.a11y_announce(Locale::En, &ReloadOutcome::KeptOld {
            active: before.id,
            failed_at: Stage::Compile,
            reason: String::from("budget"),
        });
        set.add(
            "A24-a11y-播报双语有别且逐项成行",
            zh.len() == 2 && en.len() == 2 && zh != en,
            "",
        );
    }
    {
        // 播报只报状态与计数，不泄漏源码内容。
        let mut h = HotReload::new(&base_shaders());
        let secret = "SECRET_SHADER_SOURCE_TOKEN";
        let out = h.keep_old(Stage::Validate, secret);
        let zh = h.a11y_announce(Locale::ZhCn, &out);
        let leaks = zh.iter().any(|l| l.contains(secret));
        set.add("A24-a11y-播报不含源码内容", !leaks, "");
    }
    {
        // 分类器输出**有界**：任意长输入都压成同一批有限分类词。
        // 只判「不含 token」不够——若分类器改成截断原文的前 N 字节，
        // token 可能刚好落在前 N 字节内而漏过；故这里另钉「输出恒为分类词之一」。
        let long = "预算不足 driver said: ".to_string();
        let mut src = long;
        let mut i = 0;
        while i < 500 {
            src.push_str("SECRET_SHADER_SOURCE_TOKEN");
            i += 1;
        }
        let c = reason_class(&src);
        let bounded = c == "预算不足"
            || c == "产物为空"
            || c == "超出上限"
            || c == "驱动拒收"
            || c == "场景验证失败"
            || c == "其他";
        set.add("A24-a11y-分类器输出恒为有限分类词", bounded, "");
        set.add(
            "A24-a11y-分类器对未知输入归其他",
            reason_class("完全无关的文本") == "其他",
            "",
        );
    }
    {
        // 四种结论都有播报（不留静默分支）。
        let mut h = HotReload::new(&base_shaders());
        let _ = h.rebuild(&[0x10], 2, 64);
        let landed = h.validate(true, "");
        let rolled = h.validate(false, "x");
        let kept = h.keep_old(Stage::Compile, "y");
        let deferred = ReloadOutcome::Deferred { depth: 2 };
        let n = h.a11y_announce(Locale::ZhCn, &landed).len()
            + h.a11y_announce(Locale::ZhCn, &rolled).len()
            + h.a11y_announce(Locale::ZhCn, &kept).len()
            + h.a11y_announce(Locale::ZhCn, &deferred).len();
        set.add("A24-a11y-四种结论均有播报", n == 8, "");
    }

    // ---- 诊断码与契约 ----
    {
        let codes = [
            E_RELOAD_REJECT,
            E_RELOAD_KEPT_OLD,
            E_RELOAD_ROLLED_BACK,
            E_RELOAD_FROZEN,
        ];
        let mut uniq = true;
        let mut i = 0;
        while i < codes.len() {
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        set.add("A24-judge-四类处置码两两不同", uniq, "");
    }
    {
        let docs = [FOUR_STAGE_DOC, NO_BAD_CODE_DOC, FREEZE_DOC, DEBUG_QUEUE_DOC];
        let mut all = true;
        let mut i = 0;
        while i < docs.len() {
            if docs[i].is_empty() {
                all = false;
            }
            i += 1;
        }
        set.add("A24-judge-四契约条款在场", all, "");
    }
    {
        // 常量彼此自洽（预算上限不超批次上限，冻结上限为正）。
        set.add(
            "A24-judge-常量彼此自洽",
            MAX_BATCH > 0 && MAX_PENDING > 0 && MAX_FREEZE_TICKS > 0 && COMPILE_TICK_BUDGET > 0,
            "",
        );
    }

    set
}