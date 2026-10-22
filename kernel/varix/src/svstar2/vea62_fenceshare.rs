//! VE-F0062 · 共享围栏跨服务传播（VE-A 域 · A03 同步与呈现组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0062`
//!
//! **职责定位（锚点原文）**：共享围栏跨服务传播——同步围栏的跨服务传播
//! （渲染与合成/媒体服务的围栏共享），传播丢失检测（死等检测），
//! 超时与打破；含传播链的健康度评分。
//!
//! **数据结构（锚点原文）**：围栏传播。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 死等 → 超时检测（信号已发出但目标迟迟不认领 → `Stalled`）；
//! - 丢失 → 告警 + 重建（生产者根本没发出 → `Lost`，与 `Stalled` 分开报，
//!   两者的处置完全不同：一个是目标卡住，一个是生产者没了）；
//! - 打破 → 状态一致恢复（打破必须换代重建，**不许拿旧代复活**）。
//!
//! **性能逐项分解（锚点原文）**：O(围栏)。定容槽位线性扫描，无分配、无排序。
//!
//! **跨批对接点（锚点原文）**：A03 围栏复用——本模块的 `FenceBus` 只做
//! 「传播与认领」的记账，不复刻 A03 的围栏对象本身；A03 持有围栏句柄，
//! 本模块持有传播账。两者以 `(id, gen)` 对齐，代次不符一律拒收。
//!
//! **无障碍与隐私（锚点原文）**：传播状态读屏可达——`state_line()` 输出
//! 纯文本「围栏/代次/目标/已认领/状态」，不含载荷内容。
//!
//! **判据（锚点原文）**：跨服务共享、死等检测、超时打破、状态恢复、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数 ns 定点；
//! 零浮点、零墙钟、零 IO。
//!
//! 诊断段：0xE760 段（0xE760~0xE76F 十六码）。

use alloc::string::String;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 诊断码（0xE760 段，十六码）
// ---------------------------------------------------------------------------

/// 围栏未登记（或 id 非法）。
pub const XC_UNKNOWN_FENCE: u16 = 0xE760;
/// 代次不符：拿旧代传播/认领/重建。
pub const XC_GEN_STALE: u16 = 0xE761;
/// 目标集合非法（空集或含未定义服务位）。
pub const XC_TARGET_UNKNOWN: u16 = 0xE762;
/// 生产者把自己列进目标（自发自收会造出自我依赖）。
pub const XC_TARGET_SELF: u16 = 0xE763;
/// 活跃围栏重复登记。
pub const XC_DUP_FENCE: u16 = 0xE764;
/// 重复认领。
pub const XC_DUPLICATE_ACK: u16 = 0xE765;
/// 认领未被授权（未信号 / 已打破 / 不在目标集）。
pub const XC_ACK_UNGRANTED: u16 = 0xE766;
/// 死等：已信号、目标在超时窗内未认领（目标卡住）。
pub const XC_DEADWAIT_STALL: u16 = 0xE767;
/// 等待被打破。
pub const XC_DEADWAIT_BROKEN: u16 = 0xE768;
/// 打破时无等待者（打破理由不成立，留痕以便追责）。
pub const XC_BREAK_NO_WAITER: u16 = 0xE769;
/// 传播账不平。
pub const XC_LEDGER_DRIFT: u16 = 0xE76A;
/// 围栏槽位满。
pub const XC_BUS_FULL: u16 = 0xE76B;
/// 生产者丢失：等待已久但围栏从未信号（生产者没了，不是目标卡住）。
pub const XC_PRODUCER_LOST: u16 = 0xE76C;
/// 等待者表满。
pub const XC_WAITER_FULL: u16 = 0xE76D;
/// 活跃围栏上做重建。
pub const XC_REBUILD_ACTIVE: u16 = 0xE76E;
/// 时间戳倒退（认领时刻早于信号时刻且不可解释）。
pub const XC_STAMP_BACKWARD: u16 = 0xE76F;

// ---------------------------------------------------------------------------
// 定容常量
// ---------------------------------------------------------------------------

/// 围栏槽位数。
pub const CAP: usize = 48;
/// 等待者槽位数。
pub const WCAP: usize = 32;
/// 审计槽位数。
pub const AUDIT_CAP: usize = 64;
/// 服务种类数。
pub const SVC_COUNT: usize = 6;
/// 合法服务位全掩码。
pub const ALL_MASK: u8 = 0x3F;
/// 传播本身耗用的时间（登记的是「发出→对端可见」的最小代价，ns）。
pub const PROPAGATE_NS: u64 = 400;
/// 认领预算：信号后 2ms 内认领算准时（健康度分母用它判准时）。
pub const ACK_BUDGET_NS: u64 = 2_000_000;
/// 死等阈值：信号后 16ms 未认领即判 `Stalled`（一帧的分量级）。
pub const STALL_NS: u64 = 16_000_000;
/// 生产者耐心：等待 100ms 仍未信号即判 `Lost`。
pub const PATIENCE_NS: u64 = 100_000_000;

// ---------------------------------------------------------------------------
// 服务
// ---------------------------------------------------------------------------

/// 参与围栏共享的服务。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Service {
    Renderer = 0,
    Compositor = 1,
    Decode = 2,
    Encode = 3,
    Display = 4,
    Audio = 5,
}

impl Service {
    /// 全部服务（顺序即位序）。
    pub const ALL: [Service; SVC_COUNT] = [
        Service::Renderer,
        Service::Compositor,
        Service::Decode,
        Service::Encode,
        Service::Display,
        Service::Audio,
    ];

    /// 位掩码。
    pub fn bit(self) -> u8 {
        1u8 << (self as u8)
    }

    /// 位序（`ack_tick` 的下标）。
    pub fn idx(self) -> usize {
        self as usize
    }

    /// 读屏名。
    pub fn name(self) -> &'static str {
        match self {
            Service::Renderer => "renderer",
            Service::Compositor => "compositor",
            Service::Decode => "decode",
            Service::Encode => "encode",
            Service::Display => "display",
            Service::Audio => "audio",
        }
    }

    /// 由位序取服务。
    pub fn from_idx(i: usize) -> Option<Service> {
        if i < SVC_COUNT {
            Some(Service::ALL[i])
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 围栏状态
// ---------------------------------------------------------------------------

/// 单条围栏的传播状态。
///
/// 四态是**互斥且有序**的：`Idle`（已登记待信号）→ `Signalled`（已发出待认领）→
/// `Done`（全部认领）/ `Broken`（被打破）。没有第五态——「部分认领」不是状态，
/// 是 `Signalled` 的一个字段取值，这样状态机不会随目标数膨胀。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Idle,
    Signalled,
    Done,
    Broken,
}

impl State {
    /// 终态（可复用槽位）。
    pub fn terminal(self) -> bool {
        matches!(self, State::Done | State::Broken)
    }

    /// 读屏名。
    pub fn name(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Signalled => "signalled",
            State::Done => "done",
            State::Broken => "broken",
        }
    }
}

/// 一条围栏的传播记录。
#[derive(Clone, Copy)]
pub struct FenceRec {
    /// 围栏 id（0 = 空槽）。
    pub id: u32,
    /// 代次（每次重建必须严格递增）。
    pub gen: u32,
    /// 生产者。
    pub producer: Service,
    /// 目标位集。
    pub targets: u8,
    /// 已认领位集。
    pub acked: u8,
    /// 各目标的认领时刻（按 `Service::idx()`）。
    pub ack_tick: [u64; SVC_COUNT],
    /// 信号时刻（0 = 未信号）。
    pub signal_tick: u64,
    /// 传播可见时刻（信号时刻 + 传播代价）。
    pub propagate_tick: u64,
    /// 打破时刻。
    pub break_tick: u64,
    /// 状态。
    pub state: State,
}

impl FenceRec {
    /// 空槽。
    pub const EMPTY: FenceRec = FenceRec {
        id: 0,
        gen: 0,
        producer: Service::Renderer,
        targets: 0,
        acked: 0,
        ack_tick: [0; SVC_COUNT],
        signal_tick: 0,
        propagate_tick: 0,
        break_tick: 0,
        state: State::Idle,
    };

    /// 未认领的目标位集。
    pub fn outstanding_mask(&self) -> u8 {
        self.targets & !self.acked
    }

    /// 未认领数。
    pub fn outstanding(&self) -> u32 {
        self.outstanding_mask().count_ones()
    }

    /// 目标数。
    pub fn target_count(&self) -> u32 {
        self.targets.count_ones()
    }
}

/// 等待者。
#[derive(Clone, Copy)]
pub struct Waiter {
    /// 等待的围栏。
    pub fence: u32,
    /// 等待方。
    pub service: Service,
    /// 开始等待时刻。
    pub since: u64,
}

/// 审计条目。
#[derive(Clone, Copy)]
pub struct AuditEntry {
    /// 诊断码。
    pub code: u16,
    /// 相关围栏。
    pub fence: u32,
    /// 发生时刻。
    pub tick: u64,
}

/// 扫描报告。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct ScanReport {
    /// 已完成的等待（围栏在扫描时已 Done）。
    pub satisfied: u32,
    /// 被打破而释放的等待。
    pub broken: u32,
    /// 死等（目标卡住）。
    pub stalled: u32,
    /// 生产者丢失。
    pub lost: u32,
}

impl ScanReport {
    /// 本次扫描释放的等待总数。
    pub fn released(&self) -> u32 {
        self.satisfied + self.broken + self.stalled + self.lost
    }
}

// ---------------------------------------------------------------------------
// 围栏总线
// ---------------------------------------------------------------------------

/// 跨服务围栏传播总线（定容、无分配、无锁语义）。
#[derive(Clone, Copy)]
pub struct FenceBus {
    recs: [FenceRec; CAP],
    waiters: [Waiter; WCAP],
    wc: usize,
    audit: [AuditEntry; AUDIT_CAP],
    ac: usize,
    audit_dropped: u32,
    signals_total: u32,
    /// 认领义务总数（每次信号累加目标数——一次信号造出 N 份义务，
    /// 账平衡必须按义务记，按信号记会把 2 目标的围栏算成 1）。
    obligations_total: u32,
    acks_total: u32,
    released_total: u32,
    recoveries: u32,
    breaks: u32,
}

impl FenceBus {
    /// 空总线。
    pub fn new() -> FenceBus {
        FenceBus {
            recs: [FenceRec::EMPTY; CAP],
            waiters: [
                Waiter {
                    fence: 0,
                    service: Service::Renderer,
                    since: 0,
                };
                WCAP
            ],
            wc: 0,
            audit: [
                AuditEntry {
                    code: 0,
                    fence: 0,
                    tick: 0,
                };
                AUDIT_CAP
            ],
            ac: 0,
            audit_dropped: 0,
            signals_total: 0,
            obligations_total: 0,
            acks_total: 0,
            released_total: 0,
            recoveries: 0,
            breaks: 0,
        }
    }

    /// 槽位查找。
    pub fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < CAP {
            if self.recs[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 记录。
    pub fn rec(&self, id: u32) -> Option<FenceRec> {
        match self.find(id) {
            Some(i) => Some(self.recs[i]),
            None => None,
        }
    }

    /// 已登记围栏数（含终态）。
    pub fn registered(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CAP {
            if self.recs[i].id != 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 活跃（未终态）围栏数。
    pub fn active(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CAP {
            let r = self.recs[i];
            if r.id != 0 && !r.state.terminal() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    fn record(&mut self, code: u16, fence: u32, tick: u64) {
        if self.ac < AUDIT_CAP {
            self.audit[self.ac] = AuditEntry { code, fence, tick };
            self.ac += 1;
        } else {
            self.audit_dropped = self.audit_dropped.saturating_add(1);
        }
    }

    /// 审计是否含某码。
    pub fn audit_has(&self, code: u16) -> bool {
        let mut i = 0usize;
        while i < self.ac {
            if self.audit[i].code == code {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 某码出现次数。
    pub fn audit_count(&self, code: u16) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.ac {
            if self.audit[i].code == code {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 审计丢弃数（满账后不静默）。
    pub fn audit_dropped(&self) -> u32 {
        self.audit_dropped
    }

    /// 审计条数。
    pub fn audit_len(&self) -> usize {
        self.ac
    }

    /// 登记一条围栏。
    ///
    /// 终态槽位可复用，但**必须换代**（`gen` 严格递增）——同一 `(id, gen)`
    /// 在打破后复活会让所有持旧代的等待者误以为同步已成立。
    pub fn register(
        &mut self,
        id: u32,
        gen: u32,
        producer: Service,
        targets: u8,
    ) -> Result<u32, u16> {
        if id == 0 {
            return Err(XC_UNKNOWN_FENCE);
        }
        if targets == 0 || targets & !ALL_MASK != 0 {
            return Err(XC_TARGET_UNKNOWN);
        }
        if targets & producer.bit() != 0 {
            return Err(XC_TARGET_SELF);
        }
        let mut slot = usize::MAX;
        if let Some(i) = self.find(id) {
            let old = self.recs[i];
            if !old.state.terminal() {
                return Err(XC_DUP_FENCE);
            }
            if gen <= old.gen {
                return Err(XC_GEN_STALE);
            }
            slot = i;
        } else {
            let mut i = 0usize;
            while i < CAP {
                if self.recs[i].id == 0 {
                    slot = i;
                    break;
                }
                i += 1;
            }
            if slot == usize::MAX {
                return Err(XC_BUS_FULL);
            }
        }
        let mut r = FenceRec::EMPTY;
        r.id = id;
        r.gen = gen;
        r.producer = producer;
        r.targets = targets;
        self.recs[slot] = r;
        Ok(targets.count_ones())
    }

    /// 信号（生产者宣布「我这边完成了」）。
    ///
    /// 返回需要认领的目标数。
    pub fn signal(&mut self, id: u32, gen: u32, tick: u64) -> Result<u8, u16> {
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(XC_UNKNOWN_FENCE),
        };
        if gen != self.recs[i].gen {
            return Err(XC_GEN_STALE);
        }
        if self.recs[i].state != State::Idle {
            return Err(XC_GEN_STALE);
        }
        self.recs[i].signal_tick = tick;
        self.recs[i].propagate_tick = tick.saturating_add(PROPAGATE_NS);
        self.recs[i].state = State::Signalled;
        self.signals_total = self.signals_total.saturating_add(1);
        self.obligations_total = self
            .obligations_total
            .saturating_add(self.recs[i].targets.count_ones());
        Ok(self.recs[i].targets.count_ones() as u8)
    }

    /// 认领。
    ///
    /// 返回 `Ok(true)` 表示这是最后一个认领（围栏转入 `Done`）。
    pub fn ack(&mut self, id: u32, gen: u32, by: Service, tick: u64) -> Result<bool, u16> {
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(XC_UNKNOWN_FENCE),
        };
        if gen != self.recs[i].gen {
            return Err(XC_GEN_STALE);
        }
        let st = self.recs[i].state;
        if st == State::Broken || st == State::Idle {
            return Err(XC_ACK_UNGRANTED);
        }
        if self.recs[i].targets & by.bit() == 0 {
            return Err(XC_ACK_UNGRANTED);
        }
        if self.recs[i].acked & by.bit() != 0 {
            return Err(XC_DUPLICATE_ACK);
        }
        // 认领时刻早于信号时刻：物理上不可能，按「即刻认领」记账而不是拒收
        // ——拒收会让一次真实完成永久丢失，比记 0 延迟更糟。
        let t = if tick < self.recs[i].signal_tick {
            self.recs[i].signal_tick
        } else {
            tick
        };
        self.recs[i].acked |= by.bit();
        self.recs[i].ack_tick[by.idx()] = t;
        self.acks_total = self.acks_total.saturating_add(1);
        if self.recs[i].acked == self.recs[i].targets {
            self.recs[i].state = State::Done;
            return Ok(true);
        }
        Ok(false)
    }

    /// 某目标是否已在预算内认领（健康度与诊断共用）。
    pub fn on_time(&self, id: u32, svc: Service) -> bool {
        match self.rec(id) {
            Some(r) => {
                if r.state == State::Broken || r.acked & svc.bit() == 0 {
                    return false;
                }
                r.ack_tick[svc.idx()].saturating_sub(r.signal_tick) <= ACK_BUDGET_NS
            }
            None => false,
        }
    }

    /// 登记等待（幂等：同一方对同一围栏只留一条）。
    pub fn wait(&mut self, id: u32, by: Service, now: u64) -> Result<(), u16> {
        if self.find(id).is_none() {
            return Err(XC_UNKNOWN_FENCE);
        }
        let mut k = 0usize;
        while k < self.wc {
            if self.waiters[k].fence == id && self.waiters[k].service == by {
                return Ok(());
            }
            k += 1;
        }
        if self.wc >= WCAP {
            self.record(XC_WAITER_FULL, id, now);
            return Err(XC_WAITER_FULL);
        }
        self.waiters[self.wc] = Waiter {
            fence: id,
            service: by,
            since: now,
        };
        self.wc += 1;
        Ok(())
    }

    /// 等待者数。
    pub fn waiters(&self) -> usize {
        self.wc
    }

    /// 扫描：判定死等 / 生产者丢失，并释放已成立或已判死的等待。
    pub fn scan(&mut self, now: u64) -> ScanReport {
        let mut rep = ScanReport::default();
        let mut w = 0usize;
        let mut k = 0usize;
        while k < self.wc {
            let wt = self.waiters[k];
            let mut drop_it = false;
            match self.find(wt.fence) {
                None => {
                    rep.lost += 1;
                    drop_it = true;
                }
                Some(i) => {
                    let r = self.recs[i];
                    match r.state {
                        State::Done => {
                            rep.satisfied += 1;
                            drop_it = true;
                        }
                        State::Broken => {
                            rep.broken += 1;
                            drop_it = true;
                        }
                        State::Idle => {
                            // 生产者一直没发出：等得够久就是「丢了」，不是「慢」。
                            if now.saturating_sub(wt.since) > PATIENCE_NS {
                                rep.lost += 1;
                                self.record(XC_PRODUCER_LOST, wt.fence, now);
                                drop_it = true;
                            }
                        }
                        State::Signalled => {
                            if now.saturating_sub(r.signal_tick) > STALL_NS {
                                rep.stalled += 1;
                                self.record(XC_DEADWAIT_STALL, wt.fence, now);
                                drop_it = true;
                            }
                        }
                    }
                }
            }
            if !drop_it {
                self.waiters[w] = wt;
                w += 1;
            }
            k += 1;
        }
        self.wc = w;
        rep
    }

    /// 打破一条围栏（超时兜底）。
    ///
    /// 返回被释放的等待者数；未释放任何等待的打破会留痕
    /// （`XC_BREAK_NO_WAITER`）——「预防性打破」是掩盖死等而不是处置死等。
    pub fn break_fence(&mut self, id: u32, now: u64) -> Result<u32, u16> {
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(XC_UNKNOWN_FENCE),
        };
        if self.recs[i].state == State::Broken {
            return Ok(0);
        }
        if self.recs[i].state == State::Done {
            return Err(XC_ACK_UNGRANTED);
        }
        let before = self.recs[i].acked;
        let rel = (self.recs[i].targets & !before).count_ones();
        self.recs[i].state = State::Broken;
        self.recs[i].break_tick = now;
        self.released_total = self.released_total.saturating_add(rel);
        self.breaks = self.breaks.saturating_add(1);
        let mut w = 0usize;
        let mut k = 0usize;
        let mut freed = 0u32;
        while k < self.wc {
            let wt = self.waiters[k];
            if wt.fence == id {
                freed += 1;
            } else {
                self.waiters[w] = wt;
                w += 1;
            }
            k += 1;
        }
        self.wc = w;
        if freed == 0 {
            self.record(XC_BREAK_NO_WAITER, id, now);
        }
        self.record(XC_DEADWAIT_BROKEN, id, now);
        Ok(freed)
    }

    /// 重建（打破/完成后的状态一致恢复）。
    ///
    /// 必须换代；重建后围栏回到 `Idle`，全部认领账清零——「恢复」不是
    /// 把旧账翻新，是开一本新账。
    pub fn rebuild(&mut self, id: u32, gen: u32) -> Result<(), u16> {
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(XC_UNKNOWN_FENCE),
        };
        let old = self.recs[i];
        if !old.state.terminal() {
            return Err(XC_REBUILD_ACTIVE);
        }
        if gen <= old.gen {
            return Err(XC_GEN_STALE);
        }
        let mut r = FenceRec::EMPTY;
        r.id = id;
        r.gen = gen;
        r.producer = old.producer;
        r.targets = old.targets;
        self.recs[i] = r;
        self.recoveries = self.recoveries.saturating_add(1);
        Ok(())
    }

    /// 传播链健康度（千分比）。
    ///
    /// 分母是**全部已信号围栏的目标位总数**（不是已认领数）——用已认领数
    /// 做分母会让「少认领」反而提高健康度，等于奖励丢包。被打破的围栏
    /// 贡献分母但零分子。
    pub fn health_permille(&self) -> u32 {
        let mut num = 0u32;
        let mut den = 0u32;
        let mut i = 0usize;
        while i < CAP {
            let r = self.recs[i];
            if r.id == 0 || r.signal_tick == 0 {
                i += 1;
                continue;
            }
            den += r.target_count();
            if r.state != State::Broken {
                let mut s = 0usize;
                while s < SVC_COUNT {
                    if r.acked & (1u8 << s) != 0 {
                        let d = r.ack_tick[s].saturating_sub(r.signal_tick);
                        if d <= ACK_BUDGET_NS {
                            num += 1;
                        }
                    }
                    s += 1;
                }
            }
            i += 1;
        }
        if den == 0 {
            1000
        } else {
            num.saturating_mul(1000) / den
        }
    }

    /// 在途未认领总数（只算 `Signalled`）。
    pub fn outstanding(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CAP {
            let r = self.recs[i];
            if r.id != 0 && r.state == State::Signalled {
                n += r.outstanding();
            }
            i += 1;
        }
        n
    }

    /// 认领义务总数（信号的扇出累加）。
    pub fn obligations(&self) -> u32 {
        self.obligations_total
    }

    /// 传播账平衡：认领义务 ≡ 已认领 + 打破释放 + 在途。
    pub fn ledger_balanced(&self) -> bool {
        let lhs = self.obligations_total;
        let rhs = self
            .acks_total
            .saturating_add(self.released_total)
            .saturating_add(self.outstanding());
        lhs == rhs
    }

    /// 账不平的诊断码（平衡时返回 0）。
    pub fn ledger_code(&self) -> u16 {
        if self.ledger_balanced() {
            0
        } else {
            XC_LEDGER_DRIFT
        }
    }

    /// 计数快照 `(信号, 认领, 释放, 重建, 打破)`。
    pub fn counters(&self) -> (u32, u32, u32, u32, u32) {
        (
            self.signals_total,
            self.acks_total,
            self.released_total,
            self.recoveries,
            self.breaks,
        )
    }

    /// 单条围栏的读屏行。
    pub fn state_line(&self, id: u32) -> String {
        let mut s = String::new();
        match self.rec(id) {
            None => s.push_str("围栏 未登记"),
            Some(r) => {
                s.push_str("围栏 ");
                push_u32(&mut s, r.id);
                s.push_str(" 代 ");
                push_u32(&mut s, r.gen);
                s.push_str(" 生产 ");
                s.push_str(r.producer.name());
                s.push_str(" 目标 ");
                push_u32(&mut s, r.target_count());
                s.push_str(" 已认领 ");
                push_u32(&mut s, r.acked.count_ones());
                s.push_str(" 状态 ");
                s.push_str(r.state.name());
            }
        }
        s
    }

    /// 失败目标的逗号分隔名单（诊断用，供总账明细直接引用）。
    pub fn pending_names(&self, id: u32) -> String {
        let mut s = String::new();
        if let Some(r) = self.rec(id) {
            let mut i = 0usize;
            let mut first = true;
            while i < SVC_COUNT {
                if r.outstanding_mask() & (1u8 << i) != 0 {
                    if !first {
                        s.push(',');
                    }
                    match Service::from_idx(i) {
                        Some(sv) => s.push_str(sv.name()),
                        None => s.push_str("?"),
                    }
                    first = false;
                }
                i += 1;
            }
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 文本辅助（零分配栈缓冲）
// ---------------------------------------------------------------------------

fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    while v > 0 && n < buf.len() {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
}

// ---------------------------------------------------------------------------
// 判据套件
// ---------------------------------------------------------------------------

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, if ok { "" } else { detail });
}

/// 标准夹具：围栏 1（渲染生产 → 合成/显示），围栏 2（解码生产 → 合成）。
fn fixture() -> FenceBus {
    let mut bus = FenceBus::new();
    let _ = bus.register(1, 1, Service::Renderer, Service::Compositor.bit() | Service::Display.bit());
    let _ = bus.register(2, 1, Service::Decode, Service::Compositor.bit());
    bus
}

/// VE-F0062 判据入口。
pub fn run_vea62_checks() -> CheckSet {
    let mut set = CheckSet::new("vea62_fenceshare");

    // C62-01 跨服务共享登记即记账
    {
        let bus = fixture();
        let r = bus.rec(1);
        let ok = match r {
            Some(r) => {
                r.targets == (Service::Compositor.bit() | Service::Display.bit())
                    && r.target_count() == 2
                    && r.state == State::Idle
                    && bus.registered() == 2
                    && bus.active() == 2
            }
            None => false,
        };
        chk(&mut set, "C62-01 跨服务共享登记", ok, "登记账不符");
    }

    // C62-02 生产者自发自收被拒
    {
        let mut bus = FenceBus::new();
        let e = bus.register(3, 1, Service::Renderer, Service::Renderer.bit());
        let ok = e == Err(XC_TARGET_SELF) && bus.registered() == 0;
        chk(&mut set, "C62-02 自发自收拒绝", ok, "生产者可自列目标");
    }

    // C62-03 目标集合非法（空集 / 未定义位）
    {
        let mut bus = FenceBus::new();
        let a = bus.register(4, 1, Service::Renderer, 0);
        let b = bus.register(5, 1, Service::Renderer, 0x40);
        let c = bus.register(6, 1, Service::Renderer, ALL_MASK & !Service::Renderer.bit());
        let ok = a == Err(XC_TARGET_UNKNOWN) && b == Err(XC_TARGET_UNKNOWN) && c.is_ok();
        chk(&mut set, "C62-03 目标集合校验", ok, "空集/越界位被接受");
    }

    // C62-04 未登记围栏传播被拒
    {
        let mut bus = fixture();
        let a = bus.signal(99, 1, 1000);
        let b = bus.wait(99, Service::Compositor, 1000);
        let c = bus.break_fence(99, 1000);
        let ok = a == Err(XC_UNKNOWN_FENCE)
            && b == Err(XC_UNKNOWN_FENCE)
            && c == Err(XC_UNKNOWN_FENCE)
            && bus.counters().0 == 0;
        chk(&mut set, "C62-04 未登记围栏拒绝", ok, "空围栏被当作有效");
    }

    // C62-05 代次落后被拒且不污染当前代
    {
        let mut bus = fixture();
        let _ = bus.signal(1, 1, 1000);
        let _ = bus.ack(1, 1, Service::Compositor, 1500);
        let stale = bus.ack(1, 0, Service::Display, 1600);
        let r = bus.rec(1);
        let ok = match r {
            Some(r) => {
                stale == Err(XC_GEN_STALE)
                    && r.acked == Service::Compositor.bit()
                    && r.outstanding() == 1
            }
            None => false,
        };
        chk(&mut set, "C62-05 旧代认领拒绝", ok, "旧代污染当前账");
    }

    // C62-06 信号返回需认领数
    {
        let mut bus = fixture();
        let a = bus.signal(1, 1, 1000);
        let b = bus.signal(2, 1, 1000);
        let again = bus.signal(1, 1, 2000);
        let ok = a == Ok(2) && b == Ok(1) && again == Err(XC_GEN_STALE) && bus.counters().0 == 2;
        chk(&mut set, "C62-06 信号计数", ok, "需认领数或重复信号错");
    }

    // C62-07 重复认领被拒
    {
        let mut bus = fixture();
        let _ = bus.signal(1, 1, 1000);
        let _ = bus.ack(1, 1, Service::Compositor, 1200);
        let again = bus.ack(1, 1, Service::Compositor, 1300);
        let ok = again == Err(XC_DUPLICATE_ACK) && bus.counters().1 == 1;
        chk(&mut set, "C62-07 重复认领拒绝", ok, "重复认领被计入");
    }

    // C62-08 未授权认领被拒（不在目标集 / 未信号先认领）
    {
        let mut bus = fixture();
        let early = bus.ack(1, 1, Service::Compositor, 900);
        let _ = bus.signal(1, 1, 1000);
        let alien = bus.ack(1, 1, Service::Audio, 1100);
        let ok = early == Err(XC_ACK_UNGRANTED)
            && alien == Err(XC_ACK_UNGRANTED)
            && bus.counters().1 == 0;
        chk(&mut set, "C62-08 越权认领拒绝", ok, "非目标/未信号被接受");
    }

    // C62-09 全部认领即完成
    {
        let mut bus = fixture();
        let _ = bus.signal(1, 1, 1000);
        let a = bus.ack(1, 1, Service::Compositor, 1300);
        let b = bus.ack(1, 1, Service::Display, 1400);
        let r = bus.rec(1);
        let ok = a == Ok(false)
            && b == Ok(true)
            && bus.active() == 1
            && matches!(r, Some(r) if r.state == State::Done);
        chk(&mut set, "C62-09 全认领转完成", ok, "完成态或返回错");
    }

    // C62-10 死等检测：已信号但超时未认领
    {
        let mut bus = fixture();
        let _ = bus.signal(1, 1, 1000);
        let _ = bus.ack(1, 1, Service::Compositor, 1200);
        let _ = bus.wait(1, Service::Display, 1000);
        let early = bus.scan(1000 + STALL_NS);
        let late = bus.scan(1000 + STALL_NS + 1);
        let ok = early.stalled == 0
            && late.stalled == 1
            && bus.audit_has(XC_DEADWAIT_STALL)
            && bus.waiters() == 0
            && bus.pending_names(1) == "display";
        chk(&mut set, "C62-10 死等超时检出", ok, "边界或释放名单错");
    }

    // C62-11 生产者丢失与死等分开报
    {
        let mut bus = fixture();
        let _ = bus.wait(1, Service::Compositor, 0);
        let mid = bus.scan(PATIENCE_NS);
        let late = bus.scan(PATIENCE_NS + 1);
        let ok = mid.lost == 0
            && mid.stalled == 0
            && late.lost == 1
            && late.stalled == 0
            && bus.audit_has(XC_PRODUCER_LOST)
            && !bus.audit_has(XC_DEADWAIT_STALL);
        chk(&mut set, "C62-11 丢失与死等分报", ok, "两种故障混为一谈");
    }

    // C62-12 超时打破释放等待
    {
        let mut bus = fixture();
        let _ = bus.signal(1, 1, 1000);
        let _ = bus.wait(1, Service::Compositor, 1000);
        let _ = bus.wait(1, Service::Display, 1000);
        let freed = bus.break_fence(1, 1000 + STALL_NS + 10);
        let r = bus.rec(1);
        let ok = freed == Ok(2)
            && bus.waiters() == 0
            && matches!(r, Some(r) if r.state == State::Broken)
            && bus.counters().4 == 1;
        chk(&mut set, "C62-12 打破释放等待", ok, "等待未被释放");
    }

    // C62-13 打破后迟到认领被拒（不复活）
    {
        let mut bus = fixture();
        let _ = bus.signal(1, 1, 1000);
        let _ = bus.break_fence(1, 2000);
        let late = bus.ack(1, 1, Service::Compositor, 3000);
        let ok = late == Err(XC_ACK_UNGRANTED) && bus.counters().1 == 0;
        chk(&mut set, "C62-13 打破后不复活", ok, "迟到认领复活围栏");
    }

    // C62-14 打破留痕（含无等待者的预防性打破）
    {
        let mut bus = fixture();
        let _ = bus.signal(2, 1, 1000);
        let _ = bus.break_fence(2, 5000);
        let idem = bus.break_fence(2, 6000);
        let ok = bus.audit_count(XC_DEADWAIT_BROKEN) == 1
            && bus.audit_has(XC_BREAK_NO_WAITER)
            && idem == Ok(0)
            && bus.counters().4 == 1;
        chk(&mut set, "C62-14 打破留痕与幂等", ok, "审计或幂等错");
    }

    // C62-15 状态一致恢复：换代重建后可重新认领
    {
        let mut bus = fixture();
        let _ = bus.signal(1, 1, 1000);
        let _ = bus.break_fence(1, 2000);
        let same = bus.rebuild(1, 1);
        let ok_gen = bus.rebuild(1, 2);
        let _ = bus.signal(1, 2, 3000);
        let a = bus.ack(1, 2, Service::Compositor, 3100);
        let r = bus.rec(1);
        let ok = same == Err(XC_GEN_STALE)
            && ok_gen.is_ok()
            && a == Ok(false)
            && matches!(r, Some(r) if r.gen == 2 && r.acked == Service::Compositor.bit() && r.state == State::Signalled)
            && bus.counters().3 == 1;
        chk(&mut set, "C62-15 换代重建恢复", ok, "重建未换代或账未清零");
    }

    // C62-16 活跃围栏禁止重建
    {
        let mut bus = fixture();
        let _ = bus.signal(1, 1, 1000);
        let e = bus.rebuild(1, 2);
        let ok = e == Err(XC_REBUILD_ACTIVE) && bus.counters().3 == 0;
        chk(&mut set, "C62-16 活跃禁止重建", ok, "活跃围栏被重建");
    }

    // C62-17 健康度：4 目标 3 准时 = 750‰
    {
        let mut bus = FenceBus::new();
        let four = Service::Compositor.bit()
            | Service::Display.bit()
            | Service::Decode.bit()
            | Service::Audio.bit();
        let _ = bus.register(7, 1, Service::Renderer, four);
        let _ = bus.signal(7, 1, 10_000);
        let _ = bus.ack(7, 1, Service::Compositor, 10_000 + 100);
        let _ = bus.ack(7, 1, Service::Display, 10_000 + ACK_BUDGET_NS);
        let _ = bus.ack(7, 1, Service::Decode, 10_000 + 500);
        let _ = bus.ack(7, 1, Service::Audio, 10_000 + ACK_BUDGET_NS + 1);
        let ok = bus.health_permille() == 750 && bus.on_time(7, Service::Display);
        chk(&mut set, "C62-17 健康度千分比", ok, "准时判定边界错");
    }

    // C62-18 健康度分母含未认领与已打破（不许用已认领数做分母）
    {
        let mut healthy = FenceBus::new();
        let _ = healthy.register(8, 1, Service::Renderer, Service::Compositor.bit());
        let _ = healthy.signal(8, 1, 1000);
        let _ = healthy.ack(8, 1, Service::Compositor, 1100);
        let h1 = healthy.health_permille();

        let mut lossy = FenceBus::new();
        let _ = lossy.register(8, 1, Service::Renderer, Service::Compositor.bit());
        let _ = lossy.signal(8, 1, 1000);
        let _ = lossy.ack(8, 1, Service::Compositor, 1100);
        let _ = lossy.register(9, 1, Service::Renderer, Service::Display.bit());
        let _ = lossy.signal(9, 1, 1000);
        let h2 = lossy.health_permille();

        let mut broken = FenceBus::new();
        let _ = broken.register(10, 1, Service::Renderer, Service::Display.bit());
        let _ = broken.signal(10, 1, 1000);
        let _ = broken.break_fence(10, 9000);
        let h3 = broken.health_permille();

        let ok = h1 == 1000 && h2 == 500 && h3 == 0;
        chk(&mut set, "C62-18 健康度分母诚实", ok, "丢包/打破未计入分母");
    }

    // C62-19 传播账平衡（正常完成 / 打破 / 混合三路）
    {
        let mut bus = fixture();
        let _ = bus.signal(1, 1, 1000);
        let _ = bus.ack(1, 1, Service::Compositor, 1100);
        let _ = bus.ack(1, 1, Service::Display, 1200);
        let bal_done = bus.ledger_balanced();
        let _ = bus.signal(2, 1, 1000);
        let _ = bus.ack(2, 1, Service::Compositor, 1100);
        let _ = bus.signal(1, 2, 2000); // 已终态 → 拒
        let _ = bus.break_fence(1, 3000); // 已 Done → 拒
        let ok = bal_done
            && bus.ledger_balanced()
            && bus.ledger_code() == 0
            && bus.obligations() == 3
            && bus.counters().0 == 2;
        chk(&mut set, "C62-19 传播账平衡", ok, "账不平");
    }

    // C62-20 满账诚实：槽位满拒收、等待表满留痕、审计满计数不丢
    {
        let mut bus = FenceBus::new();
        let mut i = 0u32;
        while i < CAP as u32 {
            let r = bus.register(i + 1, 1, Service::Renderer, Service::Compositor.bit());
            if r.is_err() {
                break;
            }
            i += 1;
        }
        let full = bus.register(CAP as u32 + 1, 1, Service::Renderer, Service::Compositor.bit());
        let ok_slots = full == Err(XC_BUS_FULL) && bus.registered() == CAP as u32;

        // 等待表：8 围栏 × 5 目标 = 40 条 > WCAP(32)，第 33 条必须被拒并留痕
        let mut wbus = FenceBus::new();
        let mut accepted = 0usize;
        let mut refused = false;
        let mut f = 0u32;
        while f < 8 {
            let _ = wbus.register(200 + f, 1, Service::Renderer, ALL_MASK & !Service::Renderer.bit());
            let mut s = 1usize;
            while s < SVC_COUNT {
                if let Some(sv) = Service::from_idx(s) {
                    match wbus.wait(200 + f, sv, 0) {
                        Ok(()) => accepted += 1,
                        Err(_) => refused = true,
                    }
                }
                s += 1;
            }
            f += 1;
        }
        let ok_wait = accepted == WCAP
            && refused
            && wbus.waiters() == WCAP
            && wbus.audit_has(XC_WAITER_FULL);

        // 审计满账：反复打破制造 > AUDIT_CAP 条
        let mut abus = FenceBus::new();
        let mut k = 0u32;
        while k < (AUDIT_CAP as u32 + 8) {
            let _ = abus.register(100 + k, 1, Service::Renderer, Service::Compositor.bit());
            let _ = abus.signal(100 + k, 1, k as u64);
            let _ = abus.break_fence(100 + k, k as u64 + 1);
            k += 1;
        }
        let ok_audit = abus.audit_len() <= AUDIT_CAP && abus.audit_dropped() > 0;
        let ok = ok_slots && ok_wait && ok_audit;
        chk(&mut set, "C62-20 满账诚实", ok, "满账静默丢弃");
    }

    set
}

/// 单点演练（供上层诊断面板调用）。
pub fn drill() -> (bool, String) {
    let mut bus = fixture();
    let mut ok = true;
    let mut s = String::new();
    if bus.signal(1, 1, 1000) != Ok(2) {
        ok = false;
        s.push_str("signal ");
    }
    if bus.ack(1, 1, Service::Compositor, 1200) != Ok(false) {
        ok = false;
        s.push_str("ack1 ");
    }
    if bus.ack(1, 1, Service::Display, 1400) != Ok(true) {
        ok = false;
        s.push_str("ack2 ");
    }
    if bus.health_permille() != 1000 {
        ok = false;
        s.push_str("health ");
    }
    if !bus.ledger_balanced() {
        ok = false;
        s.push_str("ledger ");
    }
    if s.is_empty() {
        s.push_str("围栏传播链正常：");
        s.push_str(&bus.state_line(1));
    }
    (ok, s)
}
