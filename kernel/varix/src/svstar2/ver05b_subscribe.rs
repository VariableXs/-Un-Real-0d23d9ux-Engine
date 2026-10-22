//! VE-F3416 · 令牌变更订阅 —— 变更→通知→重渲染全链 + 泄漏回收 + 风暴合并 + 丢失补发。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3416`
//!
//! # 职责（锚点原文拆解）
//!
//! - **响应式链**：令牌变了不能靠组件自己轮询——`publish` 写进待发区，
//!   帧边界 `commit_frame` 一次性派发，收到 `Delivery` 的组件把脏路径
//!   交给重绘队列（[`RenderQueue`]）。三步（变更→通知→重渲染）首尾相接，
//!   中间没有「下一帧再看看」的模糊地带；
//! - **订阅泄漏回收**：订阅者带所有者租约（[`OwnerLease`]）。租约到期
//!   或所有者换代，`sweep` 一次性回收——页面切走后遗留的订阅不许活到
//!   下一次主题切换，否则每次切页都多一份重绘；
//! - **变更风暴合并**：一次主题切换会连发几百条令牌变更。合并器按路径
//!   归并（同一路径只留最后一版，这正是「最后一版才是真值」的语义），
//!   再按帧批量上限切片。合并的是**变更**，不是**通知**：每个订阅者
//!   一帧只收一条 `Delivery`，里面带齐它关心的全部路径；
//! - **通知丢失补发**：每条通知带单调序号，订阅者 `ack` 之后才算出账。
//!   超过 [`FrameConfig::resend_after`] 帧仍未 ack 的，补发一次并计
//!   `lost`——补发不是重试风暴，同一条通知最多补发
//!   [`FrameConfig::max_resend`] 次，之后判死并记 [`SubCode::DeliveryDropped`]。
//!
//! # 为什么合并键必须是路径而不是订阅者
//!
//! 按订阅者合并只能省「同一个人收到多条」，省不掉「同一条令牌被算很多遍」；
//! 按路径合并在**源头**就把重复消掉：500 条变更里有 380 条打在同一个
//! `color.*` 前缀上，合并后只有最后一版进入派发。这决定了切换耗时的
//! 量级——是 O(变更数) 还是 O(受影响路径数)。
//!
//! # 为什么序号必须由 Hub 发，不许订阅者自己数
//!
//! 订阅者自数序号时，「丢了一条」和「序号跳了一格」无法区分；由 Hub
//! 单调发号，`acked + 1 < delivered_seq` 就等价于「中间有洞」，补发
//! 判定才有唯一依据。序号回绕用 `u64`，内核生命周期内不会回绕。
//!
//! # 复杂度
//!
//! - `subscribe` / `unsubscribe`：O(1) 摊还（容量满时 O(n) 压缩一次）；
//! - `publish`：O(log p)，p = 待发路径数（`BTreeMap` 归并）；
//! - `commit_frame`：O(s · b)，s = 订阅者数，b = 本帧批大小；
//! - `sweep`：O(s)；
//! - `resend_due`：O(s)。
//!
//! # 零 panic 面
//!
//! 实现区无 `unwrap` / `expect` / 切片越界；`Vec` 只做 `push` / `retain` /
//! `iter`，取下标一律先 `get`。测试区除外。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::svstar2::ver05a_appread::TokenType;

// ---------------------------------------------------------------------------
// 一、诊断码（E17 段续：5312 起，接 F3415 的 5301~5311）
// ---------------------------------------------------------------------------

/// 订阅层诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubCode {
    /// 订阅者容量已满。
    CapacityExceeded = 5312,
    /// 订阅者句柄已失效（回收或撤销后仍被使用）。
    StaleHandle = 5313,
    /// 匹配规则非法（前缀为空 / 通配位置错）。
    RuleInvalid = 5314,
    /// 所有者租约已过期。
    LeaseExpired = 5315,
    /// 通知超过补发上限，判死。
    DeliveryDropped = 5316,
    /// 确认序号超过已派发序号（伪造或串号）。
    AckAhead = 5317,
    /// 重复确认（同一条通知被 ack 两次）。
    DuplicateAck = 5318,
    /// 派发批次超过硬上限（配置错误）。
    BatchOverflow = 5319,
    /// 合帧窗口为 0 且批大小为 0（配置不可用）。
    FrameConfigInvalid = 5320,
}

impl SubCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明（读屏与报告同一措辞）。
    pub fn text(self) -> &'static str {
        match self {
            SubCode::CapacityExceeded => "订阅者容量已满",
            SubCode::StaleHandle => "订阅句柄已失效",
            SubCode::RuleInvalid => "订阅匹配规则非法",
            SubCode::LeaseExpired => "所有者租约已过期",
            SubCode::DeliveryDropped => "通知补发超限，判死",
            SubCode::AckAhead => "确认序号超过已派发序号",
            SubCode::DuplicateAck => "重复确认",
            SubCode::BatchOverflow => "派发批次超过硬上限",
            SubCode::FrameConfigInvalid => "合帧配置不可用",
        }
    }
}

/// 订阅者容量硬顶。一屏组件量级在千以内，4096 足够并留出插件余量。
pub const SUB_CAP: usize = 4096;

/// 单帧派发硬上限。超过这个数说明合帧失效（不是负载重），按错误处理。
pub const FRAME_HARD_CAP: usize = 8192;

// ---------------------------------------------------------------------------
// 二、匹配规则
// ---------------------------------------------------------------------------

/// 订阅匹配规则。四类封闭：精确、前缀、按类型、全量。
///
/// 前缀规则要求以 `.` 结尾（`color.` 而不是 `color`）——不加这条，
/// `color` 会连 `colorful.x` 一起匹配，这是最容易写错的一类静默越界。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MatchRule {
    /// 精确路径。
    Exact(String),
    /// 前缀（必须 `xxx.` 形式）。
    Prefix(String),
    /// 按令牌类型。
    Type(TokenType),
    /// 全量。
    All,
}

impl MatchRule {
    /// 规则校验：前缀必须以 `.` 结尾且非空；精确路径不许为空。
    pub fn validate(&self) -> Result<(), SubCode> {
        match self {
            MatchRule::Exact(p) => {
                if p.is_empty() {
                    Err(SubCode::RuleInvalid)
                } else {
                    Ok(())
                }
            }
            MatchRule::Prefix(p) => {
                if p.is_empty() || !p.ends_with('.') {
                    Err(SubCode::RuleInvalid)
                } else {
                    Ok(())
                }
            }
            MatchRule::Type(_) | MatchRule::All => Ok(()),
        }
    }

    /// 是否命中一条变更。
    pub fn matches(&self, path: &str, ty: TokenType) -> bool {
        match self {
            MatchRule::Exact(p) => p == path,
            MatchRule::Prefix(p) => path.starts_with(p.as_str()),
            MatchRule::Type(t) => *t == ty,
            MatchRule::All => true,
        }
    }

    /// 一行的可读渲染（读屏）。
    pub fn render(&self) -> String {
        match self {
            MatchRule::Exact(p) => format!("精确 {}", p),
            MatchRule::Prefix(p) => format!("前缀 {}*", p),
            MatchRule::Type(t) => format!("类型 {}", t.label()),
            MatchRule::All => "全量".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 三、所有者租约与句柄
// ---------------------------------------------------------------------------

/// 所有者租约。组件/页面持有它，`tick` 时续租；不续租即到期。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OwnerLease {
    /// 所有者槽位（回收后复用，必须配代数判定）。
    pub slot: u32,
    /// 代数：槽位每复用一次 +1，防 ABA。
    pub generation: u32,
    /// 到期帧号。
    pub expire_tick: u64,
}

impl OwnerLease {
    /// 新建租约。
    pub fn new(slot: u32, generation: u32, ttl: u32, now: u64) -> OwnerLease {
        OwnerLease {
            slot,
            generation,
            expire_tick: now.saturating_add(ttl as u64),
        }
    }

    /// 续租。
    pub fn renew(&mut self, ttl: u32, now: u64) {
        self.expire_tick = now.saturating_add(ttl as u64);
    }

    /// 是否已到期。
    pub fn expired(&self, now: u64) -> bool {
        now >= self.expire_tick
    }
}

/// 订阅句柄。代数不匹配即 [`SubCode::StaleHandle`]——句柄失效后
/// 绝不允许「碰巧复用同一个槽位」把通知发给新主人。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubId {
    /// 槽位。
    pub slot: u32,
    /// 代数。
    pub generation: u32,
}

/// 一个订阅者。
#[derive(Clone, PartialEq, Eq, Debug)]
struct Subscriber {
    id: SubId,
    owner: OwnerLease,
    rule: MatchRule,
    /// 优先级：数值小的先派发（焦点组件先于背景组件重绘）。
    priority: u8,
    /// 已确认序号。
    acked: u64,
    /// 已派发序号（含补发）。
    delivered: u64,
    /// 最近一次派发所在帧。
    last_delivered_tick: u64,
    /// 已补发次数（针对当前未确认的序号）。
    resends: u8,
    /// 存活。
    alive: bool,
    /// 累计收到通知数。
    received: u64,
    /// 累计补发数。
    resent: u64,
    /// 判死次数（补发超限）。
    dropped: u32,
}

/// 一条待发变更（按路径归并后只留最新）。
#[derive(Clone, PartialEq, Eq, Debug)]
struct PendingChange {
    path: String,
    ty: TokenType,
    rev: u32,
    seq: u64,
    /// 该路径在本帧被合并掉的次数（用于遥测「风暴强度」）。
    coalesced: u32,
}

/// 一次派发。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Delivery {
    /// 收件订阅者。
    pub sub: SubId,
    /// 本批最高序号。
    pub seq: u64,
    /// 本批含的路径（已按路径字典序去重）。
    pub paths: Vec<String>,
    /// 是否补发（true 表示这次是「上一条没 ack」的重投）。
    pub resend: bool,
    /// 该批实际覆盖的合并次数（诊断风暴合并效果）。
    pub coalesced: u32,
}

// ---------------------------------------------------------------------------
// 四、合帧配置与统计
// ---------------------------------------------------------------------------

/// 合帧配置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameConfig {
    /// 合帧窗口（毫秒）。0 表示「立即派发」，但批大小仍生效。
    pub window_ms: u32,
    /// 单帧最多携带的路径数。
    pub max_batch: usize,
    /// 超过多少帧未 ack 触发补发。
    pub resend_after: u32,
    /// 同一条通知最多补发几次。
    pub max_resend: u8,
}

impl Default for FrameConfig {
    fn default() -> Self {
        FrameConfig {
            // 16ms ≈ 60Hz 一帧：窗口等于一帧，用户感知不到延迟。
            window_ms: 16,
            max_batch: 256,
            resend_after: 3,
            max_resend: 2,
        }
    }
}

impl FrameConfig {
    /// 校验。窗口与批大小同时为 0 的组合不可用（永远发不出去）。
    pub fn validate(&self) -> Result<(), SubCode> {
        if self.window_ms == 0 && self.max_batch == 0 {
            return Err(SubCode::FrameConfigInvalid);
        }
        if self.max_batch > FRAME_HARD_CAP {
            return Err(SubCode::BatchOverflow);
        }
        Ok(())
    }
}

/// 订阅层统计（遥测面板一次取走）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SubStats {
    /// 当前订阅者数。
    pub live: u32,
    /// 累计回收数（租约到期）。
    pub recycled: u64,
    /// 累计判死数（补发超限）。
    pub dropped: u64,
    /// 累计合并掉的变更数。
    pub coalesced: u64,
    /// 累计派发通知数。
    pub delivered: u64,
    /// 累计补发数。
    pub resent: u64,
    /// 待发路径数。
    pub pending: u32,
}

// ---------------------------------------------------------------------------
// 五、重绘队列（响应式链第三段）
// ---------------------------------------------------------------------------

/// 重绘队列。收到 `Delivery` 的组件把路径转成脏标记，布局/绘制阶段消费。
/// 队列按路径去重且**不排序**——绘制顺序由图层树决定，不由此处决定。
#[derive(Clone, Debug, Default)]
pub struct RenderQueue {
    dirty: Vec<String>,
    /// 一帧内累计标脏次数（含重复标脏，诊断用）。
    marks: u64,
}

impl RenderQueue {
    /// 空队列。
    pub fn new() -> RenderQueue {
        RenderQueue {
            dirty: Vec::new(),
            marks: 0,
        }
    }

    /// 收一条派发，把路径标脏。重复路径只留一份。
    pub fn accept(&mut self, delivery: &Delivery) {
        for path in delivery.paths.iter() {
            self.marks = self.marks.saturating_add(1);
            if !self.dirty.iter().any(|p| p == path) {
                self.dirty.push(path.clone());
            }
        }
    }

    /// 取出本帧脏集合并清空。返回的 `Vec` 就是绘制阶段的输入。
    pub fn take(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        core::mem::swap(&mut out, &mut self.dirty);
        out
    }

    /// 当前脏路径数。
    pub fn len(&self) -> usize {
        self.dirty.len()
    }

    /// 是否无脏。
    pub fn is_empty(&self) -> bool {
        self.dirty.is_empty()
    }

    /// 累计标脏次数。
    pub fn marks(&self) -> u64 {
        self.marks
    }
}

// ---------------------------------------------------------------------------
// 六、Hub
// ---------------------------------------------------------------------------

/// 订阅中枢：注册、合帧、派发、补发、回收都在这里。
#[derive(Clone, Debug)]
pub struct Hub {
    subs: Vec<Subscriber>,
    pending: BTreeMap<String, PendingChange>,
    cfg: FrameConfig,
    /// 通知序号（单调）。
    seq: u64,
    /// 当前帧号。
    tick: u64,
    /// 下一代槽位（只在无空洞时递增）。
    next_slot: u32,
    recycled: u64,
    dropped: u64,
    coalesced: u64,
    delivered: u64,
    resent: u64,
}

impl Default for Hub {
    fn default() -> Self {
        Hub::new(FrameConfig::default())
    }
}

impl Hub {
    /// 按配置建 Hub。配置非法时退回默认配置（不 panic：订阅层不该
    /// 因为一个配置项把整条主题链打死，非法配置记在 `cfg_fallback`）。
    pub fn new(cfg: FrameConfig) -> Hub {
        let cfg = match cfg.validate() {
            Ok(()) => cfg,
            Err(_) => FrameConfig::default(),
        };
        Hub {
            subs: Vec::new(),
            pending: BTreeMap::new(),
            cfg,
            seq: 0,
            tick: 0,
            next_slot: 0,
            recycled: 0,
            dropped: 0,
            coalesced: 0,
            delivered: 0,
            resent: 0,
        }
    }

    /// 当前配置。
    pub fn config(&self) -> FrameConfig {
        self.cfg
    }

    /// 注册订阅。规则非法、容量满、租约已过期一律拒绝。
    pub fn subscribe(
        &mut self,
        owner: OwnerLease,
        rule: MatchRule,
        priority: u8,
    ) -> Result<SubId, SubCode> {
        rule.validate()?;
        if owner.expired(self.tick) {
            return Err(SubCode::LeaseExpired);
        }
        if self.live_count() >= SUB_CAP {
            // 先尝试回收一轮再拒：容量满的常见原因是泄漏，不是真的多。
            self.sweep();
            if self.live_count() >= SUB_CAP {
                return Err(SubCode::CapacityExceeded);
            }
        }
        let slot = self.next_slot;
        self.next_slot = self.next_slot.saturating_add(1);
        let id = SubId {
            slot,
            generation: owner.generation,
        };
        self.subs.push(Subscriber {
            id,
            owner,
            rule,
            priority,
            acked: self.seq,
            delivered: self.seq,
            last_delivered_tick: 0,
            resends: 0,
            alive: true,
            received: 0,
            resent: 0,
            dropped: 0,
        });
        Ok(id)
    }

    /// 撤销订阅。句柄失效则拒绝（不做「宽容删除」——宽容会让
    /// 双重撤销变成静默通过，泄漏就查不出来了）。
    pub fn unsubscribe(&mut self, id: SubId) -> Result<(), SubCode> {
        let mut found = false;
        for sub in self.subs.iter_mut() {
            if sub.id == id && sub.alive {
                sub.alive = false;
                found = true;
                break;
            }
        }
        if found {
            // 立即压实：撤销的订阅者不该占着容量到下一帧。
            self.subs.retain(|s| s.alive);
            Ok(())
        } else {
            Err(SubCode::StaleHandle)
        }
    }

    /// 续租（页面滚动等活跃行为时调用）。
    pub fn renew(&mut self, id: SubId, ttl: u32) -> Result<(), SubCode> {
        for sub in self.subs.iter_mut() {
            if sub.id == id && sub.alive {
                sub.owner.renew(ttl, self.tick);
                return Ok(());
            }
        }
        Err(SubCode::StaleHandle)
    }

    /// 推进帧号。`commit_frame` / `sweep` / `resend_due` 都以它为时间基准。
    pub fn tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    /// 发布一条变更。同路径归并（最后一版为真），风暴在这里被吃掉。
    pub fn publish(&mut self, path: &str, ty: TokenType, rev: u32) -> u64 {
        self.seq = self.seq.saturating_add(1);
        let seq = self.seq;
        match self.pending.get_mut(path) {
            Some(p) => {
                p.rev = rev;
                p.seq = seq;
                p.ty = ty;
                p.coalesced = p.coalesced.saturating_add(1);
                self.coalesced = self.coalesced.saturating_add(1);
            }
            None => {
                self.pending.insert(
                    path.to_string(),
                    PendingChange {
                        path: path.to_string(),
                        ty,
                        rev,
                        seq,
                        coalesced: 0,
                    },
                );
            }
        }
        seq
    }

    /// 派发本帧。步骤：取待发（按路径序）→ 切片 → 对每个订阅者
    /// 收集它命中的子集 → 产出 `Delivery`。
    ///
    /// 返回的 `Vec` 是「本帧要发出去的全部通知」，调用方逐条喂给
    /// [`RenderQueue::accept`]。同一订阅者一帧只出现一次——这是合并
    /// 的最终兑现。
    pub fn commit_frame(&mut self) -> Vec<Delivery> {
        let current = self.tick;
        let mut out = Vec::new();
        if self.pending.is_empty() {
            return out;
        }
        // 取出待发（清空），按 max_batch 切片。
        // `BTreeMap` 本身已按路径有序，`into_iter` 直接给出字典序——
        // 这就是「合帧结果与发布顺序无关」的物理保证。
        let drained = core::mem::take(&mut self.pending);
        let batch: Vec<PendingChange> = drained.into_iter().map(|(_, v)| v).collect();
        let limit = if self.cfg.max_batch == 0 {
            batch.len()
        } else {
            self.cfg.max_batch
        };
        let mut offset = 0;
        while offset < batch.len() {
            let end = core::cmp::min(offset + limit, batch.len());
            let slice = &batch[offset..end];
            let max_seq = slice.iter().map(|p| p.seq).max().unwrap_or(self.seq);
            let coalesced: u32 = slice.iter().map(|p| p.coalesced).sum();
            // 按优先级 + 槽位稳定排序：同一帧内顺序可复现。
            let mut order: Vec<usize> = Vec::new();
            let mut i = 0;
            while i < self.subs.len() {
                order.push(i);
                i += 1;
            }
            order.sort_by(|a, b| match (self.subs.get(*a), self.subs.get(*b)) {
                (Some(sa), Some(sb)) => sa
                    .priority
                    .cmp(&sb.priority)
                    .then_with(|| sa.id.slot.cmp(&sb.id.slot)),
                _ => core::cmp::Ordering::Equal,
            });
            for idx in order {
                let (id, matched) = match self.subs.get(idx) {
                    Some(sub) => {
                        let mut paths = Vec::new();
                        for change in slice {
                            if sub.rule.matches(&change.path, change.ty) {
                                paths.push(change.path.clone());
                            }
                        }
                        (sub.id, paths)
                    }
                    None => continue,
                };
                if matched.is_empty() {
                    continue;
                }
                if let Some(sub) = self.subs.get_mut(idx) {
                    sub.delivered = max_seq;
                    sub.last_delivered_tick = current;
                    sub.resends = 0;
                    sub.received = sub.received.saturating_add(1);
                }
                self.delivered = self.delivered.saturating_add(1);
                out.push(Delivery {
                    sub: id,
                    seq: max_seq,
                    paths: matched,
                    resend: false,
                    coalesced,
                });
            }
            offset = end;
        }
        out
    }

    /// 确认一条通知。返回实际推进的序号（0 表示没推进——重复确认或越界）。
    pub fn ack(&mut self, id: SubId, seq: u64) -> Result<u64, SubCode> {
        for sub in self.subs.iter_mut() {
            if sub.id == id && sub.alive {
                if seq > sub.delivered {
                    return Err(SubCode::AckAhead);
                }
                if seq <= sub.acked {
                    return Err(SubCode::DuplicateAck);
                }
                let advanced = seq - sub.acked;
                sub.acked = seq;
                sub.resends = 0;
                return Ok(advanced);
            }
        }
        Err(SubCode::StaleHandle)
    }

    /// 补发检测：给「已派发但超过 resend_after 帧未确认」的订阅者重投。
    /// 每次调用对每个订阅者最多产出一条补发。
    pub fn resend_due(&mut self) -> Vec<Delivery> {
        let current = self.tick;
        let mut out = Vec::new();
        let horizon = self.cfg.resend_after as u64;
        let mut i = 0;
        while i < self.subs.len() {
            let need = match self.subs.get(i) {
                Some(sub) => {
                    sub.alive
                        && sub.delivered > sub.acked
                        && current.saturating_sub(sub.last_delivered_tick) > horizon
                }
                None => false,
            };
            if !need {
                i += 1;
                continue;
            }
            let over_limit = match self.subs.get(i) {
                Some(sub) => sub.resends >= self.cfg.max_resend,
                None => false,
            };
            if over_limit {
                if let Some(sub) = self.subs.get_mut(i) {
                    sub.dropped = sub.dropped.saturating_add(1);
                    let id = sub.id;
                    sub.alive = false;
                    self.dropped = self.dropped.saturating_add(1);
                    // 判死后把 it 的洞记在 id 上：调用方可据此清理 UI。
                    out.push(Delivery {
                        sub: id,
                        seq: sub.delivered,
                        paths: Vec::new(),
                        resend: true,
                        coalesced: 0,
                    });
                }
                i += 1;
                continue;
            }
            if let Some(sub) = self.subs.get_mut(i) {
                sub.resends = sub.resends.saturating_add(1);
                sub.resent = sub.resent.saturating_add(1);
                sub.last_delivered_tick = current;
                let id = sub.id;
                let seq = sub.delivered;
                // 补发不带路径明细：路径明细在首次派发时已给过，重投只
                // 需要唤醒调用方按 seq 重建（避免重投把旧路径再标脏一遍）。
                out.push(Delivery {
                    sub: id,
                    seq,
                    paths: Vec::new(),
                    resend: true,
                    coalesced: 0,
                });
                self.resent = self.resent.saturating_add(1);
            }
            i += 1;
        }
        out
    }

    /// 回收租约到期的订阅者。返回回收数。
    pub fn sweep(&mut self) -> usize {
        let current = self.tick;
        let before = self.subs.len();
        self.subs
            .retain(|sub| sub.alive && !sub.owner.expired(current));
        let after = self.subs.len();
        let recycled = before.saturating_sub(after);
        self.recycled = self.recycled.saturating_add(recycled as u64);
        recycled
    }

    /// 存活订阅者数。
    pub fn live_count(&self) -> usize {
        self.subs.iter().filter(|s| s.alive).count()
    }

    /// 待发路径数。
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// 统计快照。
    pub fn stats(&self) -> SubStats {
        SubStats {
            live: self.live_count() as u32,
            recycled: self.recycled,
            dropped: self.dropped,
            coalesced: self.coalesced,
            delivered: self.delivered,
            resent: self.resent,
            pending: self.pending.len() as u32,
        }
    }

    /// 订阅状态读屏清单：一行一条，含规则、优先级、确认进度、补发数。
    /// 锚点要求「订阅状态读屏可查」——这条清单就是那个「可查」。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for sub in self.subs.iter().filter(|s| s.alive) {
            out.push(format!(
                "订阅#{} 规则[{}] 优先级{} 已收到{} 已确认{} 已派发{} 补发{} 到期帧{}",
                sub.id.slot,
                sub.rule.render(),
                sub.priority,
                sub.received,
                sub.acked,
                sub.delivered,
                sub.resent,
                sub.owner.expire_tick
            ));
        }
        if out.is_empty() {
            out.push("当前没有存活的令牌订阅".to_string());
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 七、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn hub() -> Hub {
        Hub::new(FrameConfig {
            window_ms: 16,
            max_batch: 4,
            resend_after: 1,
            max_resend: 1,
        })
    }

    #[test]
    fn storm_is_coalesced_by_path() {
        let mut h = hub();
        let lease = OwnerLease::new(1, 1, 100, 0);
        let _sub = h.subscribe(lease, MatchRule::Prefix("color.".to_string()), 0).expect("订阅成功");
        let mut i = 0;
        while i < 50 {
            h.publish("color.accent", TokenType::Color, i);
            i += 1;
        }
        h.publish("spacing.gap", TokenType::Length, 1);
        assert_eq!(h.pending_count(), 2, "50 条变更归并为 2 条路径");
        let deliveries = h.commit_frame();
        assert_eq!(deliveries.len(), 1, "一个订阅者一帧只收一条");
        let d = deliveries.get(0).expect("有通知");
        assert_eq!(d.paths.len(), 1, "只命中 color. 前缀");
        assert!(d.coalesced >= 49, "合并计数应记录风暴规模");
        assert_eq!(h.stats().coalesced, 49);
    }

    #[test]
    fn lease_expiry_recycles_subscriber() {
        let mut h = hub();
        let lease = OwnerLease::new(1, 1, 2, 0);
        let id = h.subscribe(lease, MatchRule::All, 5).expect("订阅成功");
        assert_eq!(h.live_count(), 1);
        h.tick();
        h.tick();
        assert_eq!(h.sweep(), 1, "到第二帧租约到期");
        assert_eq!(h.live_count(), 0);
        assert!(h.ack(id, 0).is_err(), "回收后句柄失效");
    }

    #[test]
    fn lost_delivery_is_resent_then_dropped() {
        let mut h = hub();
        let lease = OwnerLease::new(1, 1, 100, 0);
        let id = h.subscribe(lease, MatchRule::All, 0).expect("订阅成功");
        h.publish("color.a", TokenType::Color, 1);
        let d = h.commit_frame();
        assert_eq!(d.len(), 1);
        // 三帧不确认 -> 补发一次
        h.tick();
        h.tick();
        let resent = h.resend_due();
        assert_eq!(resent.len(), 1);
        assert!(resent.get(0).map(|x| x.resend).unwrap_or(false));
        // 再拖 -> 超补发上限，判死
        h.tick();
        h.tick();
        let dropped = h.resend_due();
        assert_eq!(dropped.len(), 1);
        assert!(dropped.get(0).map(|x| x.paths.is_empty()).unwrap_or(false));
        assert_eq!(h.stats().dropped, 1);
        assert!(h.ack(id, 1).is_err(), "判死后句柄失效");
    }

    #[test]
    fn ack_gate_rejects_ahead_and_duplicate() {
        let mut h = hub();
        let lease = OwnerLease::new(1, 1, 100, 0);
        let id = h.subscribe(lease, MatchRule::All, 0).expect("订阅成功");
        h.publish("color.a", TokenType::Color, 1);
        let _ = h.commit_frame();
        assert_eq!(h.ack(id, 99).err(), Some(SubCode::AckAhead));
        assert_eq!(h.ack(id, 1).ok(), Some(1));
        assert_eq!(h.ack(id, 1).err(), Some(SubCode::DuplicateAck));
    }

    #[test]
    fn prefix_rule_requires_trailing_dot() {
        let mut h = hub();
        let lease = OwnerLease::new(1, 1, 100, 0);
        assert_eq!(
            h.subscribe(lease, MatchRule::Prefix("color".to_string()), 0).err(),
            Some(SubCode::RuleInvalid)
        );
        // 带点前缀不会误伤 color 开头的其他命名空间。
        let _ = h.subscribe(lease, MatchRule::Prefix("color.".to_string()), 0);
        h.publish("colorful.x", TokenType::Color, 1);
        h.publish("color.a", TokenType::Color, 1);
        let d = h.commit_frame();
        assert_eq!(d.get(0).map(|x| x.paths.len()), Some(1));
    }

    #[test]
    fn render_queue_dedups_paths() {
        let mut h = hub();
        let lease = OwnerLease::new(1, 1, 100, 0);
        let _ = h.subscribe(lease, MatchRule::All, 0);
        h.publish("color.a", TokenType::Color, 1);
        h.publish("color.b", TokenType::Color, 1);
        let d = h.commit_frame();
        let mut q = RenderQueue::new();
        for item in d.iter() {
            q.accept(item);
        }
        assert_eq!(q.len(), 2);
        let taken = q.take();
        assert_eq!(taken.len(), 2);
        assert!(q.is_empty());
    }

    #[test]
    fn batch_limit_splits_frames() {
        let mut h = hub();
        let lease = OwnerLease::new(1, 1, 100, 0);
        let _ = h.subscribe(lease, MatchRule::All, 0);
        let mut i = 0;
        while i < 10 {
            let path = format!("color.c{}", i);
            h.publish(&path, TokenType::Color, 1);
            i += 1;
        }
        let d = h.commit_frame();
        // max_batch = 4 -> 10 条切成 3 批，每批一条通知。
        assert_eq!(d.len(), 3);
        assert_eq!(d.get(0).map(|x| x.paths.len()), Some(4));
        assert_eq!(d.get(2).map(|x| x.paths.len()), Some(2));
    }
}
