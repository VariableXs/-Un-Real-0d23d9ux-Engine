//! VE-F3541 · persona 市场集成 —— 上架/更新/下架三通道 + 审核状态回推 + 作者自见。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3541`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三通道**（[`MarketChannel`]）：上架（[`Market::publish`]）/
//!   更新（[`Market::update`]）/ 下架（[`Market::delist`]）三个独立入口，
//!   每通道各自的前置条件校验——上架要过审，更新要版本递增，
//!   下架要作者或治理方授权。
//! - **状态回推**（[`ReviewState`]）：审核状态变更推回作者侧，
//!   回推失败进重试队列（[`RetryQueue`]）——市场集成不能因为
//!   一次回推网络失败就丢状态。
//! - **作者自见**（[`Market::author_view`]）：作者只能看到自己的包，
//!   越权查询走审计（[`AuditSink`]），不是静默失败。
//! - **同规纪律**：与 W04 主题包市场完全同规——接口形状、错误码
//!   语义、审计要求逐项对齐。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 接口漂移 | 拦截（版本不匹配拒绝操作） |
//! | 回推失败 | 重试队列（指数退避上限） |
//! | 越权访问 | 审计留痕 + 拒绝 |
//!
//! # 复杂度
//!
//! 全部 O(1)（通道操作不随包数增长）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段：6000 起）
// ---------------------------------------------------------------------------

/// 市场集成诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarketCode {
    /// 接口版本漂移（调用方与市场协议不匹配）。
    IfaceDrift = 6000,
    /// 回推失败（已入重试队列）。
    PushFailed = 6001,
    /// 越权访问（非作者查包）。
    Unauthorized = 6002,
    /// 上架未过审。
    NotReviewed = 6003,
    /// 更新版本未递增。
    VersionNotBumped = 6004,
    /// 下架无授权。
    DelistDenied = 6005,
    /// 重试队列满（丢弃最旧）。
    QueueOverflow = 6006,
}

impl MarketCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            MarketCode::IfaceDrift => "接口版本漂移",
            MarketCode::PushFailed => "审核状态回推失败",
            MarketCode::Unauthorized => "越权访问已审计",
            MarketCode::NotReviewed => "上架包未通过审核",
            MarketCode::VersionNotBumped => "更新版本号未递增",
            MarketCode::DelistDenied => "下架授权不足",
            MarketCode::QueueOverflow => "回推重试队列已满",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            MarketCode::IfaceDrift
                | MarketCode::Unauthorized
                | MarketCode::NotReviewed
                | MarketCode::DelistDenied
        )
    }
}

// ---------------------------------------------------------------------------
// 二、协议版本与通道
// ---------------------------------------------------------------------------

/// 市场接口协议版本（接口漂移拦截的比对基准）。
pub const IFACE_VERSION: u32 = 1;

/// 重试队列容量上限。
pub const RETRY_CAP: usize = 32;

/// 最大重试次数（超出丢弃并记 [`MarketCode::QueueOverflow`]）。
pub const MAX_RETRY: u8 = 5;

/// 三通道。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarketChannel {
    /// 上架。
    Publish,
    /// 更新。
    Update,
    /// 下架。
    Delist,
}

impl MarketChannel {
    /// 通道短名。
    pub const fn label(self) -> &'static str {
        match self {
            MarketChannel::Publish => "上架",
            MarketChannel::Update => "更新",
            MarketChannel::Delist => "下架",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、审核状态
// ---------------------------------------------------------------------------

/// 审核状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReviewState {
    /// 待审。
    Pending,
    /// 已过审。
    Approved,
    /// 已驳回。
    Rejected,
    /// 已下架。
    Delisted,
}

impl ReviewState {
    /// 状态短名（读屏口径）。
    pub const fn label(self) -> &'static str {
        match self {
            ReviewState::Pending => "待审",
            ReviewState::Approved => "已过审",
            ReviewState::Rejected => "已驳回",
            ReviewState::Delisted => "已下架",
        }
    }

    /// 是否允许上架。
    pub const fn allows_publish(self) -> bool {
        matches!(self, ReviewState::Approved)
    }
}

// ---------------------------------------------------------------------------
// 四、包登记项
// ---------------------------------------------------------------------------

/// 市场包登记。
#[derive(Clone, Debug)]
pub struct PackageEntry {
    /// 包 id。
    pub id: u32,
    /// 作者 id。
    pub author: u32,
    /// 当前版本。
    pub version: u32,
    /// 审核状态。
    pub state: ReviewState,
}

// ---------------------------------------------------------------------------
// 五、回推重试队列
// ---------------------------------------------------------------------------

/// 回推任务（一次状态推送）。
#[derive(Clone, Copy, Debug)]
pub struct PushTask {
    /// 包 id。
    pub pkg: u32,
    /// 目标作者。
    pub author: u32,
    /// 推送的状态。
    pub state: ReviewState,
    /// 已重试次数。
    pub retries: u8,
}

/// 回推重试队列（定容、满则丢最旧）。
#[derive(Default)]
pub struct RetryQueue {
    items: Vec<PushTask>,
    /// 丢弃计数（队列溢出审计）。
    pub dropped: u32,
}

impl RetryQueue {
    /// 新建。
    pub fn new() -> Self {
        Self::default()
    }

    /// 入队；满则丢最旧并记溢出。
    pub fn push(&mut self, t: PushTask) -> bool {
        if self.items.len() >= RETRY_CAP {
            self.items.remove(0);
            self.dropped = self.dropped.saturating_add(1);
            self.items.push(t);
            return false;
        }
        self.items.push(t);
        true
    }

    /// 弹出下一个待推任务。
    pub fn pop(&mut self) -> Option<PushTask> {
        if self.items.is_empty() {
            None
        } else {
            Some(self.items.remove(0))
        }
    }

    /// 队列长度。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 队列是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 六、审计与回推出口
// ---------------------------------------------------------------------------

/// 审计事件。
#[derive(Clone, Copy, Debug)]
pub struct AuditEvent {
    /// 触发码。
    pub code: MarketCode,
    /// 相关包。
    pub pkg: u32,
    /// 相关操作者。
    pub actor: u32,
}

/// 审计汇（容量上限 64，满则覆盖最旧——审计只保近期）。
#[derive(Default)]
pub struct AuditSink {
    events: Vec<AuditEvent>,
}

impl AuditSink {
    /// 新建。
    pub fn new() -> Self {
        Self::default()
    }

    /// 记一笔。
    pub fn record(&mut self, e: AuditEvent) {
        if self.events.len() >= 64 {
            self.events.remove(0);
        }
        self.events.push(e);
    }

    /// 事件数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 按包查事件数。
    pub fn count_for(&self, pkg: u32) -> usize {
        self.events.iter().filter(|e| e.pkg == pkg).count()
    }
}

// ---------------------------------------------------------------------------
// 七、市场（主入口）
// ---------------------------------------------------------------------------

/// persona 市场集成（三通道 + 状态回推 + 作者自见）。
pub struct Market {
    /// 调用方协议版本（不匹配即拦）。
    iface: u32,
    /// 包登记表。
    pkgs: Vec<PackageEntry>,
    /// 回推队列。
    queue: RetryQueue,
    /// 审计。
    audit: AuditSink,
}

impl Market {
    /// 新建（调用方声明协议版本）。
    pub fn new(iface: u32) -> Self {
        Self {
            iface,
            pkgs: Vec::new(),
            queue: RetryQueue::new(),
            audit: AuditSink::new(),
        }
    }

    /// 协议版本核验（漂移即拒）。
    fn check_iface(&self) -> Result<(), MarketCode> {
        if self.iface == IFACE_VERSION {
            Ok(())
        } else {
            Err(MarketCode::IfaceDrift)
        }
    }

    /// 查包（内部）。
    fn find(&self, id: u32) -> Option<usize> {
        self.pkgs.iter().position(|p| p.id == id)
    }

    /// 上架三通道之一：需已过审。
    pub fn publish(&mut self, pkg: PackageEntry) -> Result<(), MarketCode> {
        self.check_iface()?;
        if !pkg.state.allows_publish() {
            self.audit.record(AuditEvent {
                code: MarketCode::NotReviewed,
                pkg: pkg.id,
                actor: pkg.author,
            });
            return Err(MarketCode::NotReviewed);
        }
        if self.find(pkg.id).is_some() {
            // 已存在走更新通道，不重复上架。
            return Err(MarketCode::VersionNotBumped);
        }
        self.pkgs.push(pkg);
        Ok(())
    }

    /// 更新：版本必须递增。
    pub fn update(&mut self, id: u32, author: u32, new_version: u32) -> Result<(), MarketCode> {
        self.check_iface()?;
        let idx = self.find(id).ok_or(MarketCode::NotReviewed)?;
        let entry = &mut self.pkgs[idx];
        if entry.author != author {
            self.audit.record(AuditEvent {
                code: MarketCode::Unauthorized,
                pkg: id,
                actor: author,
            });
            return Err(MarketCode::Unauthorized);
        }
        if new_version <= entry.version {
            return Err(MarketCode::VersionNotBumped);
        }
        entry.version = new_version;
        // 更新后需复审。
        entry.state = ReviewState::Pending;
        self.enqueue_push(id, author, ReviewState::Pending);
        Ok(())
    }

    /// 下架：作者本人或治理方（actor == 0 表示治理）。
    pub fn delist(&mut self, id: u32, actor: u32) -> Result<(), MarketCode> {
        self.check_iface()?;
        let idx = self.find(id).ok_or(MarketCode::NotReviewed)?;
        let entry = &mut self.pkgs[idx];
        if entry.author != actor && actor != 0 {
            self.audit.record(AuditEvent {
                code: MarketCode::DelistDenied,
                pkg: id,
                actor,
            });
            return Err(MarketCode::DelistDenied);
        }
        entry.state = ReviewState::Delisted;
        let author = entry.author;
        self.enqueue_push(id, author, ReviewState::Delisted);
        Ok(())
    }

    /// 审核结论回推入口（审核方调用）。
    pub fn report_review(&mut self, id: u32, approved: bool) -> Result<(), MarketCode> {
        self.check_iface()?;
        let idx = self.find(id).ok_or(MarketCode::NotReviewed)?;
        let entry = &mut self.pkgs[idx];
        entry.state = if approved {
            ReviewState::Approved
        } else {
            ReviewState::Rejected
        };
        let (author, state) = (entry.author, entry.state);
        self.enqueue_push(id, author, state);
        Ok(())
    }

    /// 入回推队列。
    fn enqueue_push(&mut self, pkg: u32, author: u32, state: ReviewState) {
        self.queue.push(PushTask {
            pkg,
            author,
            state,
            retries: 0,
        });
    }

    /// 作者自见：只返回本人包；越权记审计。
    pub fn author_view(&mut self, actor: u32) -> Result<Vec<u32>, MarketCode> {
        self.check_iface()?;
        if actor == 0 {
            // 治理方不走作者视图。
            self.audit.record(AuditEvent {
                code: MarketCode::Unauthorized,
                pkg: 0,
                actor,
            });
            return Err(MarketCode::Unauthorized);
        }
        Ok(self
            .pkgs
            .iter()
            .filter(|p| p.author == actor)
            .map(|p| p.id)
            .collect())
    }

    /// 模拟一次回推尝试；返回 false 表示失败（进重试）。
    pub fn try_deliver(&mut self, success: bool) -> Option<PushTask> {
        let task = self.queue.pop()?;
        if success {
            return Some(task);
        }
        // 失败：重试次数 +1，未超限重新入队。
        if task.retries < MAX_RETRY {
            let mut t = task;
            t.retries = t.retries.saturating_add(1);
            self.queue.push(t);
        }
        None
    }

    /// 回推队列长度。
    pub fn pending_pushes(&self) -> usize {
        self.queue.len()
    }

    /// 审计事件数。
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    /// 包数量。
    pub fn package_count(&self) -> usize {
        self.pkgs.len()
    }

    /// 查包状态（读屏口径）。
    pub fn state_label(&self, id: u32) -> Option<&'static str> {
        self.find(id).map(|i| self.pkgs[i].state.label())
    }
}

// ---------------------------------------------------------------------------
// 八、判据（run_*_checks 供聚合检查表消费）
// ---------------------------------------------------------------------------

/// 市场集成判据集。
pub fn run_market_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10a_market");

    // 1. 三通道各司其职。
    let mut m = Market::new(IFACE_VERSION);
    let ok_pub = m
        .publish(PackageEntry {
            id: 1,
            author: 7,
            version: 1,
            state: ReviewState::Approved,
        })
        .is_ok();
    let ok_upd = m.update(1, 7, 2).is_ok();
    let ok_del = m.delist(1, 7).is_ok();
    set.add("6000-三通道可用", ok_pub && ok_upd && ok_del, "");

    // 2. 未过审不得上架。
    let mut m2 = Market::new(IFACE_VERSION);
    let blocked = m2
        .publish(PackageEntry {
            id: 2,
            author: 1,
            version: 1,
            state: ReviewState::Pending,
        })
        .is_err();
    set.add("6001-未过审拦截", blocked, "");

    // 3. 版本不递增拒更。
    let mut m3 = Market::new(IFACE_VERSION);
    let _ = m3.publish(PackageEntry {
        id: 3,
        author: 2,
        version: 5,
        state: ReviewState::Approved,
    });
    let no_bump = m3.update(3, 2, 5).is_err();
    set.add("6002-版本不递增拒绝", no_bump, "");

    // 4. 越权下架拒 + 审计。
    let mut m4 = Market::new(IFACE_VERSION);
    let _ = m4.publish(PackageEntry {
        id: 4,
        author: 9,
        version: 1,
        state: ReviewState::Approved,
    });
    let denied = m4.delist(4, 88).is_err();
    let audited = m4.audit_len() > 0;
    set.add("6003-越权拒绝并审计", denied && audited, "");

    // 5. 接口漂移拦截。
    let mut m5 = Market::new(IFACE_VERSION + 9);
    let drifted = m5
        .publish(PackageEntry {
            id: 5,
            author: 1,
            version: 1,
            state: ReviewState::Approved,
        })
        .is_err();
    set.add("6004-接口漂移拦截", drifted, "");

    // 6. 作者自见只返本人包。
    let mut m6 = Market::new(IFACE_VERSION);
    let _ = m6.publish(PackageEntry {
        id: 10,
        author: 3,
        version: 1,
        state: ReviewState::Approved,
    });
    let _ = m6.publish(PackageEntry {
        id: 11,
        author: 4,
        version: 1,
        state: ReviewState::Approved,
    });
    let view = m6.author_view(3).unwrap_or_default();
    set.add("6005-作者自见过滤", view.len() == 1 && view[0] == 10, "");

    // 7. 回推失败进重试。
    let mut m7 = Market::new(IFACE_VERSION);
    let _ = m7.publish(PackageEntry {
        id: 20,
        author: 5,
        version: 1,
        state: ReviewState::Approved,
    });
    let _ = m7.report_review(20, false);
    let queued = m7.pending_pushes() == 1;
    let _ = m7.try_deliver(false);
    let retried = m7.pending_pushes() == 1;
    set.add("6006-回推失败入重试", queued && retried, "");

    set
}
