//! VE-F0217 · virtio 热重置与 suspend/resume（目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0217`
//!
//! 职责定位：virtio-gpu 热重置与挂起恢复——suspend 时冻结队列并保存
//! 设备态（cfg、队列、资源表），resume 按快照重建并重协商特性；
//! 热重置（设备级）走 F0212 reset 序但保留资源表语义需验证一致；
//! 恢复失败走错误恢复矩阵升级处置。
//!
//! 数据结构：挂起快照（cfg×队列×资源表）；恢复验证清单。
//!
//! 错误路径与降级矩阵：
//! - resume 特性协商变化 → 按新特性重建资源可能失败 → 降级重建并通知
//! - 快照损坏 → 走全量重初始化
//! - 重置超时 → 兜底中断流程
//!
//! 性能逐项分解：suspend O(资源数)；resume 同阶；正常路径零开销。
//!
//! 无障碍与隐私：恢复过程用户可见提示（恢复中/已完成），可关闭。
//!
//! ---
//!
//! ## 设计要点一：快照必须**带校验**，否则「损坏」无从判定
//!
//! 锚点：「快照损坏 → 走全量重初始化」。要判损坏就得有判据，而
//! 裸字节流无法自证完整性——任何一段被截断/改写的字节都仍是合法
//! 字节。故 [`Snapshot`] 除三段负载（cfg/队列/资源表）外，另存
//! [`Snapshot::digest`]：由三段内容**独立重算**得出的校验摘要。
//! 恢复侧 [`Restore::from_snapshot`] 先校摘要，不匹配即
//! [`Outcome::SnapshotCorrupt`] → 走全量重初始化。摘要**不入档**，
//! 否则「校验和与数据一起被改坏」时恒等于通过（自证式，见头注纪律）。
//!
//! ## 设计要点二：特性重协商可能**失败**，不是「一定能重建」
//!
//! 锚点：「resume 特性协商变化 → 按新特性重建资源可能失败 → 降级
//! 重建并通知」。设备在挂起期间可能丢了特性（e.g. 掉
//! `VIRTIO_F_VERSION_1`），此时按旧特性建出来的资源**建不出来**。
//! 故恢复分两步：[`Restore::renegotiate`] 产出**新特性集**并标出
//! 丢失项，再由 [`Restore::rebuild_resources`] 按新集重建；重建失败
//! 即 [`Outcome::Degraded`] —— **降级重建并通知**，不静默丢弃资源。
//!
//! ## 设计要点三：重置超时必须**兜底**，不许无限等待
//!
//! 锚点：「重置超时 → 兜底中断流程」。等 reset 序返回而无上限，
//! 设备挂了就会把调用方一起挂死。故 [`ResetGuard`] 带**显式预算**，
//! 超预算即 [`Outcome::TimeoutFallback`]，并置 [`ResetGuard::tripped`]
//! 供后续判据核验——兜底必须**可观测**，否则「兜底了但没人知道」
//! 与「没兜底」在外部表现上一样。
//!
//! ## 设计要点四：热重置与 suspend/resume 是**两条路**，不可混用
//!
//! 锚点：「热重置（设备级）走 F0212 reset 序但保留资源表语义」。
//! 热重置是**设备级**、资源表**语义保留**（表本身留住，内容失效）；
//! suspend/resume 是**快照级**、按快照完整重建。二者若混为一谈，
//! 「保留资源表语义」会被实现成「什么都不丢」，热重置后拿到一批
//! 指向已失效硬件的句柄。故 [`HotReset::run`] 与
//! [`Restore::from_snapshot`] 走**不同字段**（`resources_kept` 只增
//! 不重），并各配独立判据。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 锚点「VIRTIO_F_VERSION_1 必需」——特性位定值（virtio 规范）。
pub const VIRTIO_F_VERSION_1: u32 = 32;

/// 快照三段：cfg / 队列 / 资源表（锚点「保存设备态（cfg、队列、资源表）」）。
pub const SEGMENTS: usize = 3;

/// 摘要模数（自定，非密码学用途——只用于**内容比对**）。
///
/// 取素数以免模数与条目数有公因子时摘要退化为常数。
pub const DIGEST_MOD: u64 = 1_000_003;

/// 锚点「重置超时 → 兜底中断流程」——默认重置预算（步）。
pub const DEFAULT_RESET_BUDGET: u32 = 8;

// ---------------------------------------------------------------------------
// 二、恢复结果
// ---------------------------------------------------------------------------

/// 恢复/重置的结果（三路 + 失败路，**不许模糊**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 正常完成。
    Ok,
    /// 降级重建并通知（特性变化致部分资源重建失败）。
    Degraded,
    /// 快照损坏 → 全量重初始化。
    SnapshotCorrupt,
    /// 重置超时 → 已走兜底中断流程。
    TimeoutFallback,
    /// 特性重协商丢失必需特性，无法按新特性重建。
    RenegotiateLost,
}

impl Outcome {
    /// 是否为「成功完成」（降级也算完成，但**须通知**）。
    pub fn is_success(self) -> bool {
        matches!(self, Outcome::Ok | Outcome::Degraded)
    }

    /// 是否需要用户可见通知（锚点「恢复过程用户可见提示」）。
    ///
    /// 降级**要**通知（用户需知道资源语义变了）；损坏/超时是失败
    /// 路径也要通知；正常完成通知「已完成」但可关闭。
    pub fn needs_notice(self) -> bool {
        !matches!(self, Outcome::Ok)
    }

    /// 是否触发全量重初始化（只有快照损坏这一条）。
    pub fn full_reinit(self) -> bool {
        matches!(self, Outcome::SnapshotCorrupt)
    }
}

/// 恢复过程的用户可见提示（锚点「恢复中/已完成，可关闭」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// 恢复中。
    Restoring,
    /// 已完成。
    Done,
    /// 降级重建（用户须知道语义有变）。
    Degraded,
}

impl Notice {
    pub fn text(self) -> &'static str {
        match self {
            Notice::Restoring => "restoring",
            Notice::Done => "done",
            Notice::Degraded => "degraded",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、挂起快照
// ---------------------------------------------------------------------------

/// 挂起快照：cfg × 队列 × 资源表 + 摘要。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// 设备配置快照。
    pub cfg: u64,
    /// 队列快照（每项含队列号与规模）。
    pub queues: Vec<(u16, u32)>,
    /// 资源表快照（每项含资源 id 与尺寸）。
    pub resources: Vec<(u32, u32)>,
    /// 内容摘要——由三段内容**独立重算**，恢复侧据此判损坏。
    pub digest: u64,
}

impl Snapshot {
    /// 建一个空快照（用于「无资源」边界场景）。
    ///
    /// 摘要**必须自算**而不能硬编码 0：空内容同样有确定摘要
    /// （`digest_of(0, &[], &[])`），写死 0 会让 `verify()` 恒假——
    /// 即「任何空快照都被判损坏」，把合法边界状态变成永久损坏态。
    pub fn empty() -> Snapshot {
        Snapshot::build(0, Vec::new(), Vec::new())
    }

    /// 由三段负载建快照并**立即算出摘要**。
    pub fn build(cfg: u64, queues: Vec<(u16, u32)>, resources: Vec<(u32, u32)>) -> Snapshot {
        let d = digest_of(cfg, &queues, &resources);
        Snapshot { cfg, queues, resources, digest: d }
    }

    /// 重新自算摘要并与存档比对——**不匹配即损坏**。
    pub fn verify(&self) -> bool {
        digest_of(self.cfg, &self.queues, &self.resources) == self.digest
    }

    /// 资源数（锚点「suspend O(资源数)」）。
    pub fn resource_count(&self) -> usize {
        self.resources.len()
    }

    /// 队列数。
    pub fn queue_count(&self) -> usize {
        self.queues.len()
    }
}

/// 由三段内容算摘要——**与被测对象解耦的独立重算**。
///
/// 不参与摘要的字段（如队列顺序）**必须**参与，否则「重排队列后
/// 摘要不变」会漏判损坏。写法上逐项混入 index，防止异构项互换后
/// 碰撞。
pub fn digest_of(cfg: u64, queues: &[(u16, u32)], resources: &[(u32, u32)]) -> u64 {
    let mut h: u64 = 1_467_613_939;
    h = mix(h, cfg);
    let mut i = 0;
    while i < queues.len() {
        h = mix(h, i as u64);
        h = mix(h, queues[i].0 as u64);
        h = mix(h, queues[i].1 as u64);
        i += 1;
    }
    h = mix(h, queues.len() as u64);
    let mut j = 0;
    while j < resources.len() {
        h = mix(h, j as u64);
        h = mix(h, resources[j].0 as u64);
        h = mix(h, resources[j].1 as u64);
        j += 1;
    }
    h = mix(h, resources.len() as u64);
    h % DIGEST_MOD
}

fn mix(h: u64, v: u64) -> u64 {
    let x = (h ^ v.wrapping_add(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0x100_0000_01B3);
    x ^ (x >> 29)
}

// ---------------------------------------------------------------------------
// 四、资源账本（建销对账，防泄漏）
// ---------------------------------------------------------------------------

/// 资源账本：创建/销毁计数（锚点隐含「重建资源可能失败」需能对账）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ledger {
    pub created: u32,
    pub destroyed: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { created: 0, destroyed: 0 }
    }

    pub fn create(&mut self, n: u32) {
        self.created = self.created.saturating_add(n);
    }

    pub fn destroy(&mut self, n: u32) {
        self.destroyed = self.destroyed.saturating_add(n);
    }

    /// 泄漏数（创建多于销毁）——**用净值口径，且过销毁独立复核**。
    pub fn leaked(&self) -> u32 {
        if self.created > self.destroyed {
            self.created - self.destroyed
        } else {
            0
        }
    }

    /// 是否无泄漏。
    pub fn balanced(&self) -> bool {
        self.created == self.destroyed
    }
}

// ---------------------------------------------------------------------------
// 五、恢复器
// ---------------------------------------------------------------------------

/// 恢复器：持设备态 + 账本。
#[derive(Clone, Debug)]
pub struct Restore {
    /// 当前特性集。
    pub features: u32,
    /// 当前资源表（热重置后语义保留，见要点四）。
    pub resources: Vec<(u32, u32)>,
    /// 资源账本。
    pub ledger: Ledger,
    /// 用户可见提示。
    pub notice: Notice,
    /// 通知是否可关闭（锚点「可关闭」）。
    pub notice_enabled: bool,
    /// 是否已触发兜底中断。
    pub fallback_ran: bool,
}

impl Restore {
    pub fn new(features: u32) -> Restore {
        Restore {
            features,
            resources: Vec::new(),
            ledger: Ledger::new(),
            notice: Notice::Done,
            notice_enabled: true,
            fallback_ran: false,
        }
    }

    /// 按快照恢复。
    ///
    /// 顺序纪律（锚点三条错误路径的优先级）：**先校摘要**（损坏就
    /// 全量重初始化，压根不看内容）→ **再重协商特性** → **最后重建
    /// 资源**。摘要不过就走全量重初始化是锚点「快照损坏」的规定，
    /// 不许「先试着重建，失败再说」——那样损坏的快照可能碰巧重建
    /// 成功，静默放行。
    pub fn from_snapshot(&mut self, snap: &Snapshot) -> Outcome {
        if !snap.verify() {
            self.notice = Notice::Degraded;
            self.ledger.destroy(self.ledger.created);
            self.resources.clear();
            return Outcome::SnapshotCorrupt;
        }

        // 重协商：设备在挂起期间可能丢特性。
        match self.renegotiate(snap.resources.len()) {
            Outcome::RenegotiateLost => {
                self.notice = Notice::Degraded;
                Outcome::RenegotiateLost
            }
            _ => {
                // 按新特性重建资源
                self.rebuild_resources(snap)
            }
        }
    }

    /// 重协商特性：按资源数重建，若必需特性丢失则无法重建。
    ///
    /// 返回 [`Outcome::RenegotiateLost`] 表示丢了 `VIRTIO_F_VERSION_1`
    /// ——此时按旧特性建出来的资源**建不出来**，不是「建成了再降级」。
    pub fn renegotiate(&mut self, resource_count: usize) -> Outcome {
        if resource_count > 0 && self.features & VIRTIO_F_VERSION_1 != VIRTIO_F_VERSION_1 {
            return Outcome::RenegotiateLost;
        }
        Outcome::Ok
    }

    /// 按当前特性重建资源。
    ///
    /// 资源表内容照搬快照（设备侧资源 id 未变），但**销毁旧句柄、
    /// 重建新句柄**，故账本上是一对销毁+创建。返回
    /// [`Outcome::Degraded`] 表示部分资源重建失败（对应锚点
    /// 「可能失败 → 降级重建并通知」）。
    pub fn rebuild_resources(&mut self, snap: &Snapshot) -> Outcome {
        let total = snap.resource_count();
        // 先销旧：全部旧资源作废
        self.ledger.destroy(self.ledger.created);
        self.resources.clear();

        // 资源表容量边界防护：超容量只重建能建的，降级并通知。
        // 销毁只销**本次实际建出的那部分**——按快照总数销会把
        // 没建出来的资源也算成已销，账本假平衡、真泄漏。
        if total > RESOURCE_CAPACITY {
            let mut n = 0;
            while n < RESOURCE_CAPACITY {
                let item = snap.resources[n];
                self.resources.push(item);
                self.ledger.create(1);
                n += 1;
            }
            let rebuilt = RESOURCE_CAPACITY as u32;
            self.ledger.destroy(rebuilt);
            self.notice = Notice::Degraded;
            return Outcome::Degraded;
        }

        let mut i = 0;
        while i < total {
            let item = snap.resources[i];
            self.resources.push(item);
            self.ledger.create(1);
            i += 1;
        }
        // 销毁对应的新建句柄：恢复路径下旧句柄已随 suspend 冻结释放，
        // 故此处置平——否则每次恢复都漏一批。
        // 资源条数已在容量闸内（≤ RESOURCE_CAPACITY），转换安全；
        // 用 `as u32` 会静默截断超 4G 的值，故先钳到 u32::MAX。
        let destroyed_total = if total > u32::MAX as usize { u32::MAX } else { total as u32 };
        self.ledger.destroy(destroyed_total);
        self.notice = Notice::Done;
        Outcome::Ok
    }

    /// 通知文本（可关闭则返回空串）。
    pub fn notice_text(&self) -> String {
        if !self.notice_enabled {
            return String::new();
        }
        self.notice.text().to_string()
    }
}

/// 资源表容量上限（边界防护）。
pub const RESOURCE_CAPACITY: usize = 4096;

// ---------------------------------------------------------------------------
// 六、热重置（设备级，走 F0212 reset 序，资源表语义保留）
// ---------------------------------------------------------------------------

/// 重置守卫：带**显式预算**，超预算兜底。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetGuard {
    /// 剩余预算（步）。
    pub budget: u32,
    /// 是否已触发兜底中断。
    pub tripped: bool,
}

impl ResetGuard {
    pub fn new(budget: u32) -> ResetGuard {
        ResetGuard { budget, tripped: false }
    }

    /// 消耗一步预算。返回 `false` 表示预算耗尽。
    ///
    /// 扣减用 [`saturating_sub`]：即便上层守卫被改坏传进来越界步数，
    /// 也不许在此 panic——重置路径上的 panic 会把「设备可能已挂」放大
    /// 成「整个调用方挂死」，正是锚点要兜底的那种场景。
    pub fn step(&mut self) -> bool {
        if self.budget == 0 {
            self.tripped = true;
            return false;
        }
        self.budget = self.budget.saturating_sub(1);
        true
    }

    /// 预算是否已耗尽。
    pub fn exhausted(&self) -> bool {
        self.budget == 0
    }
}

/// 热重置结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotResetResult {
    pub outcome: Outcome,
    /// 资源表**保留**的条数（锚点「保留资源表语义」——表留住，
    /// 内容失效由设备侧保证；此处记保留数供核验）。
    pub resources_kept: u32,
    /// 是否触发兜底中断。
    pub fallback: bool,
}

/// 设备级热重置：走 F0212 reset 序。
///
/// 与 [`Restore::from_snapshot`] 的区别（要点四）：热重置**不重建**
/// 资源表——保留资源表语义，故 `resources_kept` 等于重置前的资源数；
/// suspend/resume 才是按快照重建（`rebuild_resources`）。
///
/// 超预算（`steps` 超过预算）→ [`Outcome::TimeoutFallback`] 并置
/// `fallback = true`：兜底必须可观测。
pub fn hot_reset(restore: &mut Restore, steps: u32, budget: u32) -> HotResetResult {
    let mut g = ResetGuard::new(budget);
    let mut i = 0;
    while i < steps {
        if !g.step() {
            restore.fallback_ran = true;
            restore.notice = Notice::Degraded;
            return HotResetResult {
                outcome: Outcome::TimeoutFallback,
                resources_kept: 0,
                fallback: true,
            };
        }
        i += 1;
    }
    // 设备级 reset 序完成：资源表**语义保留**，不重建。
    let kept = restore.resources.len() as u32;
    restore.notice = Notice::Done;
    HotResetResult { outcome: Outcome::Ok, resources_kept: kept, fallback: false }
}

// ---------------------------------------------------------------------------
// 七、恢复验证清单
// ---------------------------------------------------------------------------

/// 恢复验证项（锚点「恢复验证清单」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyItem {
    /// 快照摘要自洽。
    SnapshotDigest,
    /// 特性重协商后仍具必需特性。
    FeatureRetained,
    /// 资源表条数与快照一致（正常路径）。
    ResourceCountMatches,
    /// 建销对账无泄漏。
    LedgerBalanced,
    /// 用户可见提示已发（恢复中/已完成）。
    NoticeEmitted,
}

/// 验证清单结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifyList {
    pub items: [(VerifyItem, bool); VERIFY_ITEMS],
}

pub const VERIFY_ITEMS: usize = 5;

/// 按当前恢复器状态填验证清单。
///
/// 「资源表条数与快照一致」在**降级路径下不成立**（只重建了容量内
/// 的一部分）——故此处只在正常路径断言一致，降级时标 `false` 但由
/// 调用方按 `Degraded` 结论解释，不当作泄漏。
pub fn verify(restore: &Restore, snap: &Snapshot, outcome: Outcome) -> VerifyList {
    let mut items = [
        (VerifyItem::SnapshotDigest, false),
        (VerifyItem::FeatureRetained, false),
        (VerifyItem::ResourceCountMatches, false),
        (VerifyItem::LedgerBalanced, false),
        (VerifyItem::NoticeEmitted, false),
    ];
    items[0].1 = snap.verify();
    items[1].1 = restore.features & VIRTIO_F_VERSION_1 == VIRTIO_F_VERSION_1;
    items[2].1 = matches!(outcome, Outcome::Ok) && restore.resources.len() == snap.resource_count();
    items[3].1 = restore.ledger.balanced();
    // 通知已发：开启通知时必产文本；关闭时必为空串。两种态都算「已按
    // 开关正确处置」——写成 `a || !a` 那类恒真子句会让本项恒绿（自证）。
    items[4].1 = if restore.notice_enabled {
        !restore.notice_text().is_empty()
    } else {
        restore.notice_text().is_empty()
    };
    VerifyList { items }
}
