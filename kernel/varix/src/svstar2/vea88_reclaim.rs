//! VE-F0088 · 空闲资源回收线程（VE-A 域 · A42 大资源组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0088`
//!
//! **职责定位（锚点原文）**：空闲资源回收线程——空闲显存资源的后台回收
//! （独立低优先级线程），回收的安全断言（回收前二次确认无引用），
//! 回收与分配的协同（不抖动）。
//!
//! **数据结构（锚点原文）**：回收线程。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 误回收 → 二次确认拦截（提名与真正回收隔一个确认期，期间引用数
//!   一涨就撤销提名——这是**唯一**能挡住「刚查完就被别人拿走」的机制）；
//! - 回收抖动 → 滞后策略（同类资源回收后有静默期，静默期内不重复回收）；
//! - 线程异常 → 主线程兜底（兜底批量减半，且留痕——兜底不是无声接管）。
//!
//! **性能逐项分解（锚点原文）**：O(资源)。两趟线性扫描，资源上限 48。
//!
//! **跨批对接点（锚点原文）**：A03 池协同——本模块只决定「哪些块可以还给
//! 池」，真正的归还动作由 A03 的池执行；两处对 `(id, bytes)` 必须一致，
//! 否则池的字节账会与本模块的回收账对不上。
//!
//! **无障碍与隐私（锚点原文）**：回收状态读屏可达——`status_line()`
//! 输出纯文本「提名/回收/取消/兜底」，不含资源内容。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数 ns 定点；
//! 零浮点、零墙钟、零 IO。
//!
//! 诊断段：0xE7C0 段（0xE7C0~0xE7CF 十六码）。

use alloc::string::String;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 诊断码（0xE7C0 段）
// ---------------------------------------------------------------------------

/// 未登记资源。
pub const XC_UNKNOWN_RES: u16 = 0xE7C0;
/// 重复登记。
pub const XC_DUP_RES: u16 = 0xE7C1;
/// 二次确认拦下了误回收。
pub const XC_FALSE_RECLAIM_BLOCKED: u16 = 0xE7C2;
/// 后台线程异常，转主线程兜底。
pub const XC_THREAD_FALLBACK: u16 = 0xE7C3;
/// 本 tick 批量已满，剩余顺延。
pub const XC_BATCH_DEFER: u16 = 0xE7C4;
/// 滞后静默期内被推迟。
pub const XC_HYSTERESIS_DEFER: u16 = 0xE7C5;
/// 仍有引用，不可提名。
pub const XC_STILL_REFERENCED: u16 = 0xE7C6;
/// 钉住资源不可提名。
pub const XC_RECLAIM_PINNED: u16 = 0xE7C7;
/// 账不平。
pub const XC_LEDGER_DRIFT: u16 = 0xE7C8;
/// 资源表满。
pub const XC_RES_FULL: u16 = 0xE7C9;
/// 分配救援（提名后又被分配）。
pub const XC_ALLOC_RESCUE: u16 = 0xE7CA;
/// 参数自相矛盾。
pub const XC_BAD_PARAMS: u16 = 0xE7CB;

// ---------------------------------------------------------------------------
// 定容常量
// ---------------------------------------------------------------------------

/// 资源表容量。
pub const CAP: usize = 48;
/// 资源种类数（纹理 / 缓冲 / 渲染目标）。
pub const KIND_COUNT: usize = 3;
/// 单 tick 提名批量上限（后台线程）。
pub const BATCH: usize = 4;
/// 兜底线程的批量上限（减半，避免主线程被回收拖住）。
pub const FALLBACK_BATCH: usize = 2;
/// 空闲判定阈值（50ms 未使用才算空闲候选）。
pub const IDLE_NS: u64 = 50_000_000;
/// 二次确认延迟（提名 → 确认，隔 10ms）。
pub const CONFIRM_DELAY_NS: u64 = 10_000_000;
/// 同类回收后的静默期（30ms）。
pub const HYSTERESIS_NS: u64 = 30_000_000;

// ---------------------------------------------------------------------------
// 资源
// ---------------------------------------------------------------------------

/// 资源种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Texture = 0,
    Buffer = 1,
    Target = 2,
}

impl Kind {
    /// 全部种类。
    pub const ALL: [Kind; KIND_COUNT] = [Kind::Texture, Kind::Buffer, Kind::Target];

    /// 位序。
    pub fn idx(self) -> usize {
        self as usize
    }

    /// 读屏名。
    pub fn name(self) -> &'static str {
        match self {
            Kind::Texture => "texture",
            Kind::Buffer => "buffer",
            Kind::Target => "target",
        }
    }

    /// 由位序取种类。
    pub fn from_idx(i: usize) -> Option<Kind> {
        if i < KIND_COUNT {
            Some(Kind::ALL[i])
        } else {
            None
        }
    }
}

/// 一条资源。
#[derive(Clone, Copy)]
pub struct Res {
    /// 资源 id（0 = 空槽）。
    pub id: u32,
    /// 字节数。
    pub bytes: u32,
    /// 种类位序。
    pub kind: u8,
    /// 当前引用数。
    pub refs: u32,
    /// 最后使用时刻。
    pub last_use: u64,
    /// 是否钉住。
    pub pinned: bool,
    /// 是否已被提名回收。
    pub pending: bool,
    /// 提名时刻。
    pub pending_since: u64,
}

impl Res {
    /// 空槽。
    pub const EMPTY: Res = Res {
        id: 0,
        bytes: 0,
        kind: 0,
        refs: 0,
        last_use: 0,
        pinned: false,
        pending: false,
        pending_since: 0,
    };

    /// 是否处于「可提名」的空闲态。
    pub fn idle_candidate(&self, now: u64) -> bool {
        self.id != 0
            && !self.pinned
            && !self.pending
            && self.refs == 0
            && now.saturating_sub(self.last_use) >= IDLE_NS
    }
}

/// 单 tick 报告。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct TickReport {
    /// 本 tick 提名数。
    pub nominated: u32,
    /// 本 tick 实际回收数。
    pub reclaimed: u32,
    /// 本 tick 撤销提名数。
    pub cancelled: u32,
    /// 因批量满顺延数。
    pub deferred_batch: u32,
    /// 因静默期推迟数。
    pub deferred_hysteresis: u32,
    /// 是否走了兜底。
    pub fell_back: bool,
}

// ---------------------------------------------------------------------------
// 回收器
// ---------------------------------------------------------------------------

/// 空闲资源回收器（两阶段 + 滞后 + 兜底）。
#[derive(Clone, Copy)]
pub struct Reclaimer {
    res: [Res; CAP],
    last_reclaim: [u64; KIND_COUNT],
    has_reclaimed: [bool; KIND_COUNT],
    reclaimed_by_kind: [u32; KIND_COUNT],
    thread_alive: bool,
    nominated: u32,
    reclaimed: u32,
    cancelled: u32,
    deferred_batch: u32,
    deferred_hysteresis: u32,
    rescued: u32,
    false_blocked: u32,
    fallback_ticks: u32,
    bytes_reclaimed: u64,
}

impl Reclaimer {
    /// 空回收器。
    pub fn new() -> Reclaimer {
        Reclaimer {
            res: [Res::EMPTY; CAP],
            last_reclaim: [0; KIND_COUNT],
            has_reclaimed: [false; KIND_COUNT],
            reclaimed_by_kind: [0; KIND_COUNT],
            thread_alive: true,
            nominated: 0,
            reclaimed: 0,
            cancelled: 0,
            deferred_batch: 0,
            deferred_hysteresis: 0,
            rescued: 0,
            false_blocked: 0,
            fallback_ticks: 0,
            bytes_reclaimed: 0,
        }
    }

    /// 参数自洽：空闲阈值必须覆盖确认延迟，静默期不短于确认延迟。
    ///
    /// 三者若错位，「刚提名就被判过期」「确认期内又被提名」这类时序
    /// 打架会同时出现，而现象只是「偶尔少回收一块」——极难定位。
    pub fn params_sane() -> bool {
        IDLE_NS > CONFIRM_DELAY_NS && HYSTERESIS_NS >= CONFIRM_DELAY_NS
    }

    /// 查找。
    pub fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < CAP {
            if self.res[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 资源视图。
    pub fn res(&self, id: u32) -> Option<Res> {
        match self.find(id) {
            Some(i) => Some(self.res[i]),
            None => None,
        }
    }

    /// 登记。
    pub fn register(&mut self, id: u32, bytes: u32, kind: Kind) -> Result<(), u16> {
        if id == 0 || bytes == 0 {
            return Err(XC_UNKNOWN_RES);
        }
        if self.find(id).is_some() {
            return Err(XC_DUP_RES);
        }
        let mut slot = usize::MAX;
        let mut i = 0usize;
        while i < CAP {
            if self.res[i].id == 0 {
                slot = i;
                break;
            }
            i += 1;
        }
        if slot == usize::MAX {
            return Err(XC_RES_FULL);
        }
        let mut r = Res::EMPTY;
        r.id = id;
        r.bytes = bytes;
        r.kind = kind.idx() as u8;
        self.res[slot] = r;
        Ok(())
    }

    /// 加引用（会撤销提名——这是二次确认的输入端）。
    pub fn add_ref(&mut self, id: u32, now: u64) -> Result<u32, u16> {
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(XC_UNKNOWN_RES),
        };
        self.res[i].refs = self.res[i].refs.saturating_add(1);
        self.res[i].last_use = now;
        if self.res[i].pending {
            self.res[i].pending = false;
            self.cancelled = self.cancelled.saturating_add(1);
            self.false_blocked = self.false_blocked.saturating_add(1);
        }
        Ok(self.res[i].refs)
    }

    /// 减引用。
    pub fn release_ref(&mut self, id: u32, now: u64) -> Result<u32, u16> {
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(XC_UNKNOWN_RES),
        };
        if self.res[i].refs == 0 {
            return Err(XC_STILL_REFERENCED);
        }
        self.res[i].refs -= 1;
        self.res[i].last_use = now;
        Ok(self.res[i].refs)
    }

    /// 重新分配（救援：提名中的资源被再次使用）。
    pub fn on_alloc(&mut self, id: u32, now: u64) -> Result<(), u16> {
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(XC_UNKNOWN_RES),
        };
        self.res[i].last_use = now;
        if self.res[i].pending {
            self.res[i].pending = false;
            self.rescued = self.rescued.saturating_add(1);
            self.cancelled = self.cancelled.saturating_add(1);
        }
        Ok(())
    }

    /// 钉住/解钉。
    pub fn set_pinned(&mut self, id: u32, pinned: bool) -> Result<(), u16> {
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(XC_UNKNOWN_RES),
        };
        self.res[i].pinned = pinned;
        if pinned && self.res[i].pending {
            self.res[i].pending = false;
            self.cancelled = self.cancelled.saturating_add(1);
        }
        Ok(())
    }

    /// 后台线程存活标记（异常时置假 → 主线程兜底）。
    pub fn set_thread_alive(&mut self, alive: bool) {
        self.thread_alive = alive;
    }

    /// 线程是否存活。
    pub fn thread_alive(&self) -> bool {
        self.thread_alive
    }

    /// 一个 tick：先确认（回收/撤销），再提名。
    pub fn tick(&mut self, now: u64) -> TickReport {
        let mut rep = TickReport::default();
        let batch = if self.thread_alive {
            BATCH
        } else {
            rep.fell_back = true;
            self.fallback_ticks = self.fallback_ticks.saturating_add(1);
            FALLBACK_BATCH
        };
        // ① 确认期已到的提名：二次确认引用数
        let mut i = 0usize;
        while i < CAP {
            let r = self.res[i];
            if r.id != 0 && r.pending && now.saturating_sub(r.pending_since) >= CONFIRM_DELAY_NS {
                if r.refs > 0 || r.pinned {
                    self.res[i].pending = false;
                    self.cancelled = self.cancelled.saturating_add(1);
                    self.false_blocked = self.false_blocked.saturating_add(1);
                    rep.cancelled += 1;
                } else {
                    let k = (r.kind as usize) % KIND_COUNT;
                    let silent = self.has_reclaimed[k]
                        && now.saturating_sub(self.last_reclaim[k]) < HYSTERESIS_NS;
                    if silent {
                        // 静默期内不回收，但提名保留（下个 tick 再判）。
                        self.deferred_hysteresis = self.deferred_hysteresis.saturating_add(1);
                        rep.deferred_hysteresis += 1;
                    } else {
                        self.res[i] = Res::EMPTY;
                        self.reclaimed = self.reclaimed.saturating_add(1);
                        self.reclaimed_by_kind[k] = self.reclaimed_by_kind[k].saturating_add(1);
                        self.last_reclaim[k] = now;
                        self.has_reclaimed[k] = true;
                        self.bytes_reclaimed = self.bytes_reclaimed.saturating_add(r.bytes as u64);
                        rep.reclaimed += 1;
                    }
                }
            }
            i += 1;
        }
        // ② 提名新候选
        let mut n = 0u32;
        let mut j = 0usize;
        while j < CAP {
            if self.res[j].idle_candidate(now) {
                if (n as usize) < batch {
                    self.res[j].pending = true;
                    self.res[j].pending_since = now;
                    self.nominated = self.nominated.saturating_add(1);
                    n += 1;
                } else {
                    self.deferred_batch = self.deferred_batch.saturating_add(1);
                    rep.deferred_batch += 1;
                }
            }
            j += 1;
        }
        rep.nominated = n;
        rep
    }

    /// 当前提名中的数量。
    pub fn pending_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CAP {
            if self.res[i].id != 0 && self.res[i].pending {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 存活资源数。
    pub fn alive_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CAP {
            if self.res[i].id != 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 计数 `(提名, 回收, 撤销, 批量顺延, 静默推迟, 救援, 误回收拦截, 兜底 tick)`。
    pub fn counters(&self) -> (u32, u32, u32, u32, u32, u32, u32, u32) {
        (
            self.nominated,
            self.reclaimed,
            self.cancelled,
            self.deferred_batch,
            self.deferred_hysteresis,
            self.rescued,
            self.false_blocked,
            self.fallback_ticks,
        )
    }

    /// 已回收字节。
    pub fn bytes_reclaimed(&self) -> u64 {
        self.bytes_reclaimed
    }

    /// 分类回收数。
    pub fn reclaimed_by_kind(&self, k: Kind) -> u32 {
        self.reclaimed_by_kind[k.idx()]
    }

    /// 账平衡：提名 ≡ 回收 + 撤销 + 仍在提名。
    pub fn ledger_balanced(&self) -> bool {
        self.nominated == self.reclaimed + self.cancelled + self.pending_count()
    }

    /// 读屏行。
    pub fn status_line(&self) -> String {
        let mut s = String::new();
        s.push_str("存活 ");
        push_u32(&mut s, self.alive_count());
        s.push_str(" 提名 ");
        push_u32(&mut s, self.pending_count());
        s.push_str(" 已回收 ");
        push_u32(&mut s, self.reclaimed);
        s.push_str(" 撤销 ");
        push_u32(&mut s, self.cancelled);
        s.push_str(" 线程 ");
        s.push_str(if self.thread_alive { "在线" } else { "兜底" });
        s
    }
}

impl Default for Reclaimer {
    fn default() -> Reclaimer {
        Reclaimer::new()
    }
}

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

/// VE-F0088 判据入口。
pub fn run_vea88_checks() -> CheckSet {
    let mut set = CheckSet::new("vea88_reclaim");

    // C88-01 登记与重复登记
    {
        let mut r = Reclaimer::new();
        let a = r.register(1, 4096, Kind::Texture);
        let b = r.register(1, 4096, Kind::Texture);
        let ok = a.is_ok() && b == Err(XC_DUP_RES) && r.alive_count() == 1;
        chk(&mut set, "C88-01 登记与重复", ok, "重复登记未拒");
    }

    // C88-02 有引用不提名
    {
        let mut r = Reclaimer::new();
        let _ = r.register(1, 4096, Kind::Texture);
        let _ = r.add_ref(1, 0);
        let rep = r.tick(IDLE_NS * 10);
        let ok = rep.nominated == 0 && r.pending_count() == 0 && r.counters().0 == 0;
        chk(&mut set, "C88-02 有引用不提名", ok, "带引用资源被提名");
    }

    // C88-03 空闲未达阈值不提名（边界）
    {
        let mut r = Reclaimer::new();
        let _ = r.register(1, 4096, Kind::Buffer);
        let early = r.tick(IDLE_NS - 1);
        let edge = r.tick(IDLE_NS);
        let ok = early.nominated == 0 && edge.nominated == 1;
        chk(&mut set, "C88-03 空闲阈值边界", ok, "阈值边界判错");
    }

    // C88-04 第一阶段只提名不回收
    {
        let mut r = Reclaimer::new();
        let _ = r.register(1, 4096, Kind::Texture);
        let rep = r.tick(IDLE_NS);
        let ok = rep.nominated == 1
            && rep.reclaimed == 0
            && r.res(1).map(|x| x.pending) == Some(true)
            && r.counters().1 == 0;
        chk(&mut set, "C88-04 提名不回收", ok, "一阶段就回收了");
    }

    // C88-05 确认期到才回收（边界）
    {
        let mut r = Reclaimer::new();
        let _ = r.register(1, 4096, Kind::Texture);
        let _ = r.tick(IDLE_NS);
        let early = r.tick(IDLE_NS + CONFIRM_DELAY_NS - 1);
        let due = r.tick(IDLE_NS + CONFIRM_DELAY_NS);
        let ok = early.reclaimed == 0
            && due.reclaimed == 1
            && r.res(1).is_none()
            && r.bytes_reclaimed() == 4096;
        chk(&mut set, "C88-05 确认期回收", ok, "确认期边界错");
    }

    // C88-06 二次确认拦下误回收
    {
        let mut r = Reclaimer::new();
        let _ = r.register(1, 4096, Kind::Texture);
        let _ = r.tick(IDLE_NS);
        let _ = r.add_ref(1, IDLE_NS + 1);
        let due = r.tick(IDLE_NS + CONFIRM_DELAY_NS + 10);
        let ok = due.reclaimed == 0
            && r.res(1).map(|x| x.refs) == Some(1)
            && r.counters().2 == 1
            && r.counters().6 == 1;
        chk(&mut set, "C88-06 二次确认拦截", ok, "带引用资源被回收");
    }

    // C88-07 提名后钉住同样撤销
    {
        let mut r = Reclaimer::new();
        let _ = r.register(1, 2048, Kind::Target);
        let _ = r.tick(IDLE_NS);
        let _ = r.set_pinned(1, true);
        let due = r.tick(IDLE_NS + CONFIRM_DELAY_NS + 1);
        // 撤销发生在 `set_pinned` 里（tick 之外），故看累计计数而不是本 tick 报告。
        let ok = due.reclaimed == 0
            && r.counters().2 == 1
            && r.res(1).is_some()
            && r.ledger_balanced();
        chk(&mut set, "C88-07 钉住撤销提名", ok, "钉住后仍被回收");
    }

    // C88-08 滞后静默期推迟同类回收
    {
        let mut r = Reclaimer::new();
        let _ = r.register(1, 1024, Kind::Texture);
        let _ = r.register(2, 1024, Kind::Texture);
        let _ = r.tick(IDLE_NS);
        let t1 = IDLE_NS + CONFIRM_DELAY_NS;
        let first = r.tick(t1);
        // 第二块提名在 t1（批量 4 足够），确认期后落在静默期内
        let second = r.tick(t1 + CONFIRM_DELAY_NS);
        // 静默推迟累计两次：t1 那次与 t1+确认期那次都在静默期内。
        let ok = first.reclaimed == 1
            && second.reclaimed == 0
            && second.deferred_hysteresis == 1
            && r.counters().4 == 2;
        chk(&mut set, "C88-08 滞后静默期", ok, "同类连收未防抖");
    }

    // C88-09 静默期过后可回收
    {
        let mut r = Reclaimer::new();
        let _ = r.register(1, 1024, Kind::Texture);
        let _ = r.register(2, 1024, Kind::Texture);
        let _ = r.tick(IDLE_NS);
        let t1 = IDLE_NS + CONFIRM_DELAY_NS;
        let _ = r.tick(t1);
        let _ = r.tick(t1 + CONFIRM_DELAY_NS); // 静默期内，保留提名
        let ok_tick = r.tick(t1 + HYSTERESIS_NS);
        let ok = ok_tick.reclaimed == 1 && r.alive_count() == 0 && r.ledger_balanced();
        chk(&mut set, "C88-09 静默期后回收", ok, "静默期后仍不回收");
    }

    // C88-10 批量上限
    {
        let mut r = Reclaimer::new();
        let mut i = 0u32;
        while i < 6 {
            let _ = r.register(i + 1, 512, Kind::from_idx(i as usize % KIND_COUNT).unwrap_or(Kind::Buffer));
            i += 1;
        }
        let rep = r.tick(IDLE_NS);
        let ok = rep.nominated as usize == BATCH
            && rep.deferred_batch == (6 - BATCH) as u32
            && r.pending_count() as usize == BATCH;
        chk(&mut set, "C88-10 批量上限", ok, "批量越界或顺延计数错");
    }

    // C88-11 顺延不丢：下一 tick 继续提名
    {
        let mut r = Reclaimer::new();
        let mut i = 0u32;
        while i < 6 {
            let _ = r.register(i + 1, 512, Kind::from_idx(i as usize % KIND_COUNT).unwrap_or(Kind::Buffer));
            i += 1;
        }
        let _ = r.tick(IDLE_NS);
        let rep = r.tick(IDLE_NS + 1);
        let ok = rep.nominated == 2 && r.pending_count() == 6 && r.counters().0 == 6;
        chk(&mut set, "C88-11 顺延不丢", ok, "顺延候选丢失");
    }

    // C88-12 线程异常转兜底并留痕
    {
        let mut r = Reclaimer::new();
        let mut i = 0u32;
        while i < 6 {
            let _ = r.register(i + 1, 512, Kind::from_idx(i as usize % KIND_COUNT).unwrap_or(Kind::Buffer));
            i += 1;
        }
        r.set_thread_alive(false);
        let rep = r.tick(IDLE_NS);
        let ok = rep.fell_back
            && rep.nominated as usize == FALLBACK_BATCH
            && r.counters().7 == 1
            && !r.thread_alive();
        chk(&mut set, "C88-12 兜底接管", ok, "兜底未生效或未留痕");
    }

    // C88-13 兜底仍能把回收做完（不是只报错）
    {
        let mut r = Reclaimer::new();
        let _ = r.register(1, 512, Kind::Buffer);
        r.set_thread_alive(false);
        let _ = r.tick(IDLE_NS);
        let due = r.tick(IDLE_NS + CONFIRM_DELAY_NS);
        let ok = due.reclaimed == 1 && r.alive_count() == 0 && r.counters().7 == 2;
        chk(&mut set, "C88-13 兜底可完成", ok, "兜底只报错不回收");
    }

    // C88-14 分配救援：提名后又被分配即撤销
    {
        let mut r = Reclaimer::new();
        let _ = r.register(1, 512, Kind::Texture);
        let _ = r.tick(IDLE_NS);
        let _ = r.on_alloc(1, IDLE_NS + 5);
        let due = r.tick(IDLE_NS + CONFIRM_DELAY_NS + 100);
        let ok = due.reclaimed == 0
            && r.counters().5 == 1
            && r.res(1).is_some()
            && r.ledger_balanced();
        chk(&mut set, "C88-14 分配救援", ok, "刚分配的资源被回收");
    }

    // C88-15 钉住资源一开始就不提名
    {
        let mut r = Reclaimer::new();
        let _ = r.register(1, 512, Kind::Target);
        let _ = r.set_pinned(1, true);
        let rep = r.tick(IDLE_NS * 4);
        let ok = rep.nominated == 0 && r.pending_count() == 0;
        chk(&mut set, "C88-15 钉住不提名", ok, "钉住资源被提名");
    }

    // C88-16 引用减到零后进入候选
    {
        let mut r = Reclaimer::new();
        let _ = r.register(1, 512, Kind::Buffer);
        let _ = r.add_ref(1, 0);
        let _ = r.release_ref(1, IDLE_NS);
        let rep = r.tick(IDLE_NS * 3);
        let ok = rep.nominated == 1;
        chk(&mut set, "C88-16 释放后成候选", ok, "释放后进不了候选");
    }

    // C88-17 字节账与分类账
    {
        let mut r = Reclaimer::new();
        let _ = r.register(1, 1000, Kind::Texture);
        let _ = r.register(2, 2000, Kind::Buffer);
        let _ = r.tick(IDLE_NS);
        let _ = r.tick(IDLE_NS + CONFIRM_DELAY_NS);
        let ok = r.bytes_reclaimed() == 3000
            && r.reclaimed_by_kind(Kind::Texture) == 1
            && r.reclaimed_by_kind(Kind::Buffer) == 1
            && r.reclaimed_by_kind(Kind::Target) == 0;
        chk(&mut set, "C88-17 字节与分类账", ok, "字节或分类账错");
    }

    // C88-18 满表诚实
    {
        let mut r = Reclaimer::new();
        let mut i = 0u32;
        while i < CAP as u32 {
            let _ = r.register(i + 1, 64, Kind::Buffer);
            i += 1;
        }
        let full = r.register(CAP as u32 + 1, 64, Kind::Buffer);
        let ok = full == Err(XC_RES_FULL) && r.alive_count() == CAP as u32;
        chk(&mut set, "C88-18 满表诚实", ok, "满表静默覆盖");
    }

    // C88-19 未知资源一律拒绝
    {
        let mut r = Reclaimer::new();
        let a = r.add_ref(9, 0);
        let b = r.release_ref(9, 0);
        let c = r.on_alloc(9, 0);
        let d = r.set_pinned(9, true);
        let ok = a == Err(XC_UNKNOWN_RES)
            && b == Err(XC_UNKNOWN_RES)
            && c == Err(XC_UNKNOWN_RES)
            && d == Err(XC_UNKNOWN_RES);
        chk(&mut set, "C88-19 未知资源拒绝", ok, "隐式登记未知资源");
    }

    // C88-20 参数自洽 + 状态行可读
    {
        let r = Reclaimer::new();
        let line = r.status_line();
        let ok = Reclaimer::params_sane()
            && line.contains("存活")
            && line.contains("提名")
            && line.contains("已回收")
            && line.contains("线程");
        chk(&mut set, "C88-20 参数与读屏", ok, "参数错位或读屏缺失");
    }

    set
}

/// 单点演练。
pub fn drill() -> (bool, String) {
    let mut r = Reclaimer::new();
    let mut ok = true;
    let mut s = String::new();
    if r.register(1, 1024, Kind::Texture).is_err() {
        ok = false;
        s.push_str("reg ");
    }
    let _ = r.tick(IDLE_NS);
    let due = r.tick(IDLE_NS + CONFIRM_DELAY_NS);
    if due.reclaimed != 1 {
        ok = false;
        s.push_str("reclaim ");
    }
    if !r.ledger_balanced() || !Reclaimer::params_sane() {
        ok = false;
        s.push_str("invariant ");
    }
    if s.is_empty() {
        s.push_str("回收正常：");
        s.push_str(&r.status_line());
    }
    (ok, s)
}
