//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12732 · 同步族语义对照回归（Wait/Event/Semaphore/Mutant） · 0x82100段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F12733-J1 Virtual* 三态状态机六弧对照回归绿，
//!      PAGE_* 全集矩阵维持；
//!   ③ 完成时 API/软件写回：状态=已完成、结果转换非空。
//! 设计要点：
//!   - 同步族四对象闭集：Wait（等待工单）/Event（事件）/Semaphore（信号量）/
//!     Mutant（变异体），四对象各自带真实账本，语义对照互不混同；
//!   - 等待三态状态机（观望→排队→了结）六弧冻结表：挂起等待/对象置信/
//!     超时/属主弃权/续等改期/取回收口——六弧逐条对照回归，越表弧与
//!     倒退弧一律显性拒；
//!   - PAGE_* 全集矩阵（8 基础保护 × 2 修饰 = 16 格）冻结登记：
//!     全集维持 = 16 格全标记且组合规则（NOCACHE 与 WRITECOMBINE 互斥）
//!     守恒，作为共享内存同步对象的访问权对照面；
//!   - 注入三族（状态倒退/PAGE 违规组合/非属主释放）各 10 轮必抓全抓；
//!   - LCG 种子写死 = SEG_BASE（0x82100）；no_std + alloc；
//!     零 std/零浮点/零 IO/零墙钟/零随机/零 unwrap/零裸下标。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（VX9 公式：0x80000 + 0x40 × (12732−12600) = 0x82100）。
pub const SEG_BASE: u32 = 0x82100;

// ── 冻结常量表 ─────────────────────────────────────────────────────
/// 等待队列容量（同步工单并发上限；冻结值，超容必拒）。
pub const WAIT_QUEUE_CAP: usize = 8;
/// 六弧表弧数（三态状态机对照弧全集）。
pub const ARC_COUNT: usize = 6;
/// PAGE_* 基础保护档数（winnt.h 内存保护常量基族）。
pub const PAGE_BASE_N: usize = 8;
/// PAGE 矩阵列数：裸保护 / +GUARD 修饰（PAGE_GUARD 0x100，winnt.h）。
pub const PAGE_MATRIX_COLS: usize = 2;
/// PAGE 矩阵格数 = 8 × 2 = 16。
pub const PAGE_MATRIX_CELLS: usize = PAGE_BASE_N * PAGE_MATRIX_COLS;
/// PAGE_NOCACHE（winnt.h 0x200）。
pub const PAGE_NOCACHE: u32 = 0x200;
/// PAGE_WRITECOMBINE（winnt.h 0x400）。
pub const PAGE_WRITECOMBINE: u32 = 0x400;
/// 信号量计数上限（冻结语义：释放越上限即拒）。
pub const SEMA_MAX: u32 = 7;
/// 变异体重入深度上限（Mutant 递归获取上限）。
pub const MUTANT_DEPTH_MAX: u32 = 4;

/// PAGE_* 基础保护八档（出处：winnt.h 内存保护常量族，值冻结）。
pub const PAGE_BASE: [u32; PAGE_BASE_N] = [
    0x01, // PAGE_NOACCESS
    0x02, // PAGE_READONLY
    0x04, // PAGE_READWRITE
    0x08, // PAGE_WRITECOPY
    0x10, // PAGE_EXECUTE
    0x20, // PAGE_EXECUTE_READ
    0x40, // PAGE_EXECUTE_READWRITE
    0x80, // PAGE_EXECUTE_WRITECOPY
];

// ── 诊断码闭集（段内 0x40 窗口，10 码） ────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncDiag {
    /// 弧号越六弧闭集（查无此弧）
    ArcUnknown,
    /// 弧起点与当前态不符（越表弧/跳弧）
    ArcOriginMismatch,
    /// 状态倒退（空槽收单/重复收单等账目回退）
    ArcBackward,
    /// 了结后未取回即再收口（重放拒）
    SettleReplay,
    /// 等待队列容量耗尽（超 8 工单）
    WaitQueueFull,
    /// 信号量计数倒退（账本不守恒）
    SemaLedgerBreak,
    /// 信号量释放越上限
    SemaMaxReached,
    /// 非属主释放变异体
    MutantNotOwner,
    /// PAGE 矩阵缺格（16 格未登记齐）
    PageMatrixHole,
    /// PAGE 组合违规（NOCACHE 与 WRITECOMBINE 互斥）
    PageCombineRule,
}

impl SyncDiag {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SyncDiag::ArcUnknown => "弧号越六弧闭集：查表后重试",
            SyncDiag::ArcOriginMismatch => "弧起点与当前态不符：按六弧表走",
            SyncDiag::ArcBackward => "状态倒退：先挂单再收单",
            SyncDiag::SettleReplay => "了结重放：先取回收口再重等",
            SyncDiag::WaitQueueFull => "等待队列满：先收口旧工单再挂新等",
            SyncDiag::SemaLedgerBreak => "信号量账本倒退：计数守恒被破坏",
            SyncDiag::SemaMaxReached => "释放越上限：先取用再释放",
            SyncDiag::MutantNotOwner => "非属主释放：仅属主可弃",
            SyncDiag::PageMatrixHole => "PAGE矩阵缺格：16格登记齐再收口",
            SyncDiag::PageCombineRule => "NOCACHE与WRITECOMBINE互斥：二选一",
        }
    }
    pub const ALL: [SyncDiag; 10] = [
        SyncDiag::ArcUnknown,
        SyncDiag::ArcOriginMismatch,
        SyncDiag::ArcBackward,
        SyncDiag::SettleReplay,
        SyncDiag::WaitQueueFull,
        SyncDiag::SemaLedgerBreak,
        SyncDiag::SemaMaxReached,
        SyncDiag::MutantNotOwner,
        SyncDiag::PageMatrixHole,
        SyncDiag::PageCombineRule,
    ];
}

/// 诊断码查表（闭集返回；未知码给中文兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < SyncDiag::ALL.len() {
        if let Some(c) = SyncDiag::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82100 段闭集内"
}

// ── 三态状态机 ─────────────────────────────────────────────────────
/// 等待三态：观望（未挂等）/ 排队（等挂起中）/ 了结（结果已定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitPhase {
    /// 观望：工单未挂入等待队列
    Idle,
    /// 排队：等待挂起，等对象置信/超时/弃权
    Pending,
    /// 了结：等待结果已定，待取回收口
    Settled,
}

impl WaitPhase {
    /// 三态中文名（对照回归文案面）。
    pub fn name(self) -> &'static str {
        match self {
            WaitPhase::Idle => "观望",
            WaitPhase::Pending => "排队",
            WaitPhase::Settled => "了结",
        }
    }
}

/// 六弧冻结表：(弧名, 起点, 终点, 触发语)。
/// 出处：等待语义对照规格——三态间合法迁移恰此六条，越表即违约。
pub const WAIT_ARCS: [(&'static str, WaitPhase, WaitPhase, &'static str); ARC_COUNT] = [
    ("弧一·挂起等待", WaitPhase::Idle, WaitPhase::Pending, "wait挂起"),
    ("弧二·对象置信", WaitPhase::Pending, WaitPhase::Settled, "对象置信唤醒"),
    ("弧三·等待超时", WaitPhase::Pending, WaitPhase::Settled, "超时到期"),
    ("弧四·属主弃权", WaitPhase::Pending, WaitPhase::Settled, "变异体被弃"),
    ("弧五·续等改期", WaitPhase::Pending, WaitPhase::Pending, "改期续等"),
    ("弧六·取回收口", WaitPhase::Settled, WaitPhase::Idle, "取回结果复位"),
];

/// 同步对象四族闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncKind {
    /// 等待工单（Wait）
    Wait,
    /// 事件（Event）
    Event,
    /// 信号量（Semaphore）
    Semaphore,
    /// 变异体（Mutant）
    Mutant,
}

/// 等待工单：三态状态机的单实例账本。
#[derive(Clone, Copy)]
pub struct WaitTicket {
    /// 所属同步对象族
    pub kind: SyncKind,
    /// 当前态
    pub phase: WaitPhase,
    /// 累计走弧数
    pub arcs_walked: u32,
    /// 累计了结次数（未取回前不得再了结）
    pub settle_count: u32,
    /// 最近走的弧号（None=尚未走弧）
    pub last_arc: Option<usize>,
}

impl WaitTicket {
    pub fn new(kind: SyncKind) -> WaitTicket {
        WaitTicket {
            kind,
            phase: WaitPhase::Idle,
            arcs_walked: 0,
            settle_count: 0,
            last_arc: None,
        }
    }

    /// 走弧：查六弧表→验了结重放→验起点。三关全过才迁移。
    /// （单调性由闭集弧表本身保证：表内唯一下行弧是弧六取回收口，
    /// 其余越表弧/倒退弧一律被起点关或重放关拦下，显性拒。）
    pub fn step(&mut self, arc: usize) -> Result<WaitPhase, SyncDiag> {
        let entry = match WAIT_ARCS.get(arc) {
            Some(e) => *e,
            None => return Err(SyncDiag::ArcUnknown),
        };
        let (_name, from, to, _trigger) = entry;
        // 了结重放拒：未取回收口（了结态）不得再触发了结型弧。
        // （此关先于起点关——重放的定性比起点错位更精确。）
        if to == WaitPhase::Settled && self.settle_count > 0 {
            return Err(SyncDiag::SettleReplay);
        }
        if self.phase != from {
            return Err(SyncDiag::ArcOriginMismatch);
        }
        if to == WaitPhase::Settled {
            self.settle_count += 1;
        }
        if to == WaitPhase::Idle && self.phase == WaitPhase::Settled {
            // 弧六取回收口：了结账随取回清零，下轮等待重新起算。
            self.settle_count = 0;
        }
        self.phase = to;
        self.arcs_walked = self.arcs_walked.saturating_add(1);
        self.last_arc = Some(arc);
        Ok(self.phase)
    }
}

// ── 事件（Event） ──────────────────────────────────────────────────
/// 事件型别闭集：通知型（置信保持）/ 同步型（单次放行自动复位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventMode {
    /// 通知型：set 后持续置信
    Notification,
    /// 同步型：放行一个等待者后自动复位
    Synchronization,
}

/// 事件账本：型别 + 置信态 + 翻转次数。
#[derive(Clone, Copy)]
pub struct SyncEvent {
    pub mode: EventMode,
    pub signaled: bool,
    pub toggles: u32,
}

impl SyncEvent {
    pub fn new(mode: EventMode) -> SyncEvent {
        SyncEvent { mode, signaled: false, toggles: 0 }
    }
    /// 置信：通知型保持；同步型放行后立即复位（语义对照点）。
    pub fn set(&mut self) -> bool {
        self.signaled = true;
        self.toggles = self.toggles.saturating_add(1);
        if self.mode == EventMode::Synchronization {
            self.signaled = false;
        }
        self.signaled
    }
    /// 复位：两型皆可显性复位。
    pub fn reset(&mut self) {
        self.signaled = false;
        self.toggles = self.toggles.saturating_add(1);
    }
}

// ── 信号量（Semaphore） ────────────────────────────────────────────
/// 信号量账本：计数守恒（count ≡ releases − acquires）+ 上限拒。
#[derive(Clone, Copy)]
pub struct Semaphore {
    pub count: u32,
    pub releases: u32,
    pub acquires: u32,
}

impl Semaphore {
    pub fn new(initial: u32) -> Semaphore {
        let c = if initial > SEMA_MAX { SEMA_MAX } else { initial };
        Semaphore { count: c, releases: c, acquires: 0 }
    }
    /// 取用：零计数即拒（耗尽注入面）。
    pub fn acquire(&mut self) -> Result<u32, SyncDiag> {
        if self.count == 0 {
            return Err(SyncDiag::SemaLedgerBreak);
        }
        self.count -= 1;
        self.acquires = self.acquires.saturating_add(1);
        self.verify_ledger()?;
        Ok(self.count)
    }
    /// 释放：越上限即拒（满额注入面）。
    pub fn release(&mut self) -> Result<u32, SyncDiag> {
        if self.count >= SEMA_MAX {
            return Err(SyncDiag::SemaMaxReached);
        }
        self.count += 1;
        self.releases = self.releases.saturating_add(1);
        self.verify_ledger()?;
        Ok(self.count)
    }
    /// 计数守恒校验（对账面）。
    pub fn verify_ledger(&self) -> Result<(), SyncDiag> {
        if self.count != self.releases - self.acquires {
            return Err(SyncDiag::SemaLedgerBreak);
        }
        Ok(())
    }
}

// ── 变异体（Mutant） ───────────────────────────────────────────────
/// 变异体账本：单属主 + 递归深度 + 弃权语义。
#[derive(Clone, Copy)]
pub struct Mutant {
    /// 属主令牌（None=无主可取）
    pub owner: Option<u32>,
    /// 已被弃权（下一个等待者按弧四了结）
    pub abandoned: bool,
    /// 属主递归深度
    pub depth: u32,
}

impl Mutant {
    pub fn new() -> Mutant {
        Mutant { owner: None, abandoned: false, depth: 0 }
    }
    /// 获取：无主即取；属主重入加深（越深度上限拒）；有主他客拒（注入面）。
    pub fn acquire(&mut self, token: u32) -> Result<u32, SyncDiag> {
        match self.owner {
            None => {
                self.owner = Some(token);
                self.depth = 1;
                self.abandoned = false;
                Ok(self.depth)
            }
            Some(o) if o == token => {
                if self.depth >= MUTANT_DEPTH_MAX {
                    return Err(SyncDiag::MutantNotOwner);
                }
                self.depth += 1;
                Ok(self.depth)
            }
            Some(_) => Err(SyncDiag::MutantNotOwner),
        }
    }
    /// 释放：仅属主；深度归零即弃主。
    pub fn release(&mut self, token: u32) -> Result<(), SyncDiag> {
        match self.owner {
            Some(o) if o == token => {
                if self.depth <= 1 {
                    self.owner = None;
                    self.depth = 0;
                } else {
                    self.depth -= 1;
                }
                Ok(())
            }
            _ => Err(SyncDiag::MutantNotOwner),
        }
    }
    /// 弃权：属主失联，标记弃权（触发弧四语义）。
    pub fn abandon(&mut self, token: u32) -> Result<(), SyncDiag> {
        match self.owner {
            Some(o) if o == token => {
                self.abandoned = true;
                self.owner = None;
                self.depth = 0;
                Ok(())
            }
            _ => Err(SyncDiag::MutantNotOwner),
        }
    }
}

// ── 等待队列（容量耗尽注入面） ─────────────────────────────────────
/// 等待队列：定容 8 槽，槽满即拒（WaitQueueFull）。
#[derive(Clone, Copy)]
pub struct WaitQueue {
    pub used: [bool; WAIT_QUEUE_CAP],
    pub count: usize,
}

impl WaitQueue {
    pub fn new() -> WaitQueue {
        WaitQueue { used: [false; WAIT_QUEUE_CAP], count: 0 }
    }
    /// 挂单：找空槽，槽满即拒。
    pub fn enqueue(&mut self) -> Result<usize, SyncDiag> {
        if self.count >= WAIT_QUEUE_CAP {
            return Err(SyncDiag::WaitQueueFull);
        }
        let mut i = 0usize;
        while i < WAIT_QUEUE_CAP {
            let free = match self.used.get(i) {
                Some(false) => true,
                _ => false,
            };
            if free {
                // 空槽命中（借 .get/.get_mut 防越界、无裸下标）。
                if let Some(slot) = self.used.get_mut(i) {
                    *slot = true;
                }
                self.count += 1;
                return Ok(i);
            }
            i += 1;
        }
        Err(SyncDiag::WaitQueueFull)
    }
    /// 收单：占槽释放。空槽/越界槽收单 = 账目倒退（ArcBackward 显性拒）。
    pub fn dequeue(&mut self, slot: usize) -> Result<(), SyncDiag> {
        let occupied = match self.used.get(slot) {
            Some(true) => true,
            _ => false,
        };
        if !occupied || self.count == 0 {
            return Err(SyncDiag::ArcBackward);
        }
        if let Some(s) = self.used.get_mut(slot) {
            *s = false;
        }
        self.count = self.count.saturating_sub(1);
        Ok(())
    }
}

// ── PAGE_* 全集矩阵 ────────────────────────────────────────────────
/// PAGE 矩阵：8 基础档 × 2 修饰列 = 16 格，格位标记账。
/// 全集维持 = 16 格全标记 + 组合互斥规则全程守恒。
#[derive(Clone, Copy)]
pub struct PageMatrix {
    pub marks: [u8; PAGE_MATRIX_CELLS],
    pub marked: usize,
}

impl PageMatrix {
    pub fn new() -> PageMatrix {
        PageMatrix { marks: [0u8; PAGE_MATRIX_CELLS], marked: 0 }
    }

    /// 格位线性化（基础档 × 修饰列，越表拒）。
    pub fn cell_index(base: usize, guarded: bool) -> Result<usize, SyncDiag> {
        if base >= PAGE_BASE_N {
            return Err(SyncDiag::PageMatrixHole);
        }
        let col = if guarded { 1usize } else { 0usize };
        Ok(base * PAGE_MATRIX_COLS + col)
    }

    /// 组合求值：基础档必须查表命中；NOCACHE 与 WRITECOMBINE 互斥由
    /// 调用侧 modifiers 参数表达（bit0=NOCACHE，bit1=WRITECOMBINE）。
    pub fn combine(base: usize, guarded: bool, modifiers: u32) -> Result<u32, SyncDiag> {
        let v = match PAGE_BASE.get(base) {
            Some(v) => *v,
            None => return Err(SyncDiag::PageMatrixHole),
        };
        let mut out = v;
        if guarded {
            out |= 0x100u32; // PAGE_GUARD（winnt.h）
        }
        if modifiers & 1 != 0 {
            out |= PAGE_NOCACHE;
        }
        if modifiers & 2 != 0 {
            out |= PAGE_WRITECOMBINE;
        }
        // 互斥规则：NOCACHE 与 WRITECOMBINE 不得同用（冻结语义）。
        if (out & PAGE_NOCACHE) != 0 && (out & PAGE_WRITECOMBINE) != 0 {
            return Err(SyncDiag::PageCombineRule);
        }
        Ok(out)
    }

    /// 登记一格（重复登记并入计数，不重复占格）。
    pub fn mark(&mut self, base: usize, guarded: bool) -> Result<usize, SyncDiag> {
        let idx = Self::cell_index(base, guarded)?;
        let fresh = match self.marks.get(idx) {
            Some(&0) => true,
            _ => false,
        };
        if fresh {
            self.marked = self.marked.saturating_add(1);
        }
        if let Some(m) = self.marks.get_mut(idx) {
            *m = m.saturating_add(1);
        }
        Ok(idx)
    }

    /// 全集维持判定：16 格全标记。
    pub fn complete(&self) -> bool {
        self.marked == PAGE_MATRIX_CELLS
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演：六弧全走 + 矩阵登记齐 + 四对象语义对照 ───────────────
/// 黄金正演返回断言通过数（满绿 = 12）。
pub fn golden_run() -> Result<u32, SyncDiag> {
    // 第一段：六弧逐条走通（用一张工单按合法序走完六弧全表）。
    let mut t = WaitTicket::new(SyncKind::Wait);
    // 弧一：观望→排队
    t.step(0)?;
    // 弧五：排队→排队（改期）
    t.step(4)?;
    // 弧二：排队→了结（对象置信）
    t.step(1)?;
    // 弧六：了结→观望（取回收口）
    t.step(5)?;
    if t.arcs_walked != 4 || t.settle_count != 0 {
        return Err(SyncDiag::ArcBackward);
    }
    // 再走弧一→弧三（超时了结）→弧六，覆盖弧三。
    t.step(0)?;
    t.step(2)?;
    t.step(5)?;
    // 再走弧一→弧四（弃权了结）→弧六，覆盖弧四。至此六弧全覆盖。
    t.step(0)?;
    t.step(3)?;
    t.step(5)?;

    // 第二段：四对象语义对照。
    let mut ev_n = SyncEvent::new(EventMode::Notification);
    if !ev_n.set() || !ev_n.signaled {
        return Err(SyncDiag::ArcOriginMismatch);
    }
    let mut ev_s = SyncEvent::new(EventMode::Synchronization);
    if ev_s.set() {
        return Err(SyncDiag::ArcOriginMismatch);
    }
    let mut sema = Semaphore::new(3);
    let _ = sema.acquire()?;
    let _ = sema.release()?;
    let mut mutx = Mutant::new();
    let _ = mutx.acquire(0x11)?;
    let _ = mutx.release(0x11)?;
    let _ = mutx.acquire(0x22)?;
    mutx.abandon(0x22)?;

    // 第三段：等待队列满容 + 收单（容量面）。
    let mut q = WaitQueue::new();
    let mut booked: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < WAIT_QUEUE_CAP {
        booked.push(q.enqueue()?);
        i += 1;
    }
    if q.enqueue().is_ok() {
        return Err(SyncDiag::WaitQueueFull);
    }
    // 收单全部归还（计数守恒）。
    let mut k = 0usize;
    while k < booked.len() {
        if let Some(slot) = booked.get(k) {
            q.dequeue(*slot)?;
        }
        k += 1;
    }
    if q.count != 0 {
        return Err(SyncDiag::SemaLedgerBreak);
    }

    // 第四段：PAGE 矩阵 16 格登记齐。
    let mut pm = PageMatrix::new();
    let mut b = 0usize;
    while b < PAGE_BASE_N {
        let _ = pm.mark(b, false)?;
        let _ = pm.mark(b, true)?;
        b += 1;
    }
    if !pm.complete() {
        return Err(SyncDiag::PageMatrixHole);
    }

    Ok(run_asserts())
}

/// 对照断言组（12 条，全过 = 12）。
pub fn run_asserts() -> u32 {
    let mut pass = 0u32;
    // A1：六弧表弧数冻结为 6。
    if WAIT_ARCS.len() == ARC_COUNT {
        pass += 1;
    }
    // A2：六弧表名与触发语全非空（文案闭集前置）。
    let mut names_ok = true;
    let mut i = 0usize;
    while i < WAIT_ARCS.len() {
        if let Some((n, _f, _t, trig)) = WAIT_ARCS.get(i) {
            if n.is_empty() || trig.is_empty() {
                names_ok = false;
            }
        }
        i += 1;
    }
    if names_ok {
        pass += 1;
    }
    // A3：倒退注入必抓（了结态走弧一=观望起点不符）。
    let mut probe = WaitTicket::new(SyncKind::Wait);
    let _ = probe.step(0);
    let _ = probe.step(1);
    if probe.step(0).is_err() {
        pass += 1;
    }
    // A4：了结重放拒（未取回再了结）。
    let mut p2 = WaitTicket::new(SyncKind::Event);
    let _ = p2.step(0);
    let _ = p2.step(2);
    if p2.step(2).is_err() {
        pass += 1;
    }
    // A5：弧号越表必抓。
    if WaitTicket::new(SyncKind::Semaphore).step(ARC_COUNT).is_err() {
        pass += 1;
    }
    // A6：PAGE 矩阵格位双射（16 格线性化不重不漏）。
    let mut pm2 = PageMatrix::new();
    let mut all_marked = true;
    let mut b = 0usize;
    while b < PAGE_BASE_N {
        if pm2.mark(b, false).is_err() || pm2.mark(b, true).is_err() {
            all_marked = false;
        }
        b += 1;
    }
    if all_marked && pm2.complete() {
        pass += 1;
    }
    // A7：PAGE 互斥组合必抓。
    if PageMatrix::combine(2, false, 3).is_err() {
        pass += 1;
    }
    // A8：信号量耗尽必抓。
    let mut s0 = Semaphore::new(0);
    if s0.acquire().is_err() {
        pass += 1;
    }
    // A9：信号量满额释放必抓。
    let mut s1 = Semaphore::new(SEMA_MAX);
    if s1.release().is_err() {
        pass += 1;
    }
    // A10：非属主释放必抓。
    let mut m0 = Mutant::new();
    let _ = m0.acquire(1);
    if m0.release(2).is_err() {
        pass += 1;
    }
    // A11：等待队列满容必抓。
    let mut q2 = WaitQueue::new();
    let mut full = true;
    let mut j = 0usize;
    while j < WAIT_QUEUE_CAP {
        if q2.enqueue().is_err() {
            full = false;
        }
        j += 1;
    }
    if full && q2.enqueue().is_err() {
        pass += 1;
    }
    // A12：弃权变异体清主（弧四语义前置成立）。
    let mut m1 = Mutant::new();
    let _ = m1.acquire(9);
    let _ = m1.abandon(9);
    if m1.owner.is_none() && m1.abandoned {
        pass += 1;
    }
    pass
}

// ── 注入面：三族 × 10 轮全抓 ──────────────────────────────────────
/// 注入账本：三族各自 10 轮尝试/抓获。
#[derive(Clone, Copy)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 全抓判定：三族抓获数 ≡ 尝试数。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if c != t {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
    /// 轮数守恒：每族恰 10 轮。
    pub fn ten_rounds_each(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            if let Some(t) = self.tries.get(i) {
                if *t != 10 {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 注入三族 × 10 轮：族0=状态倒退、族1=PAGE违规组合、族2=非属主释放。
/// 每轮用 LCG 变化参数（种子=SEG_BASE），违约必抓。
pub fn run_injections(ledger: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < 10 {
        // 族0：状态倒退注入——了结后试图走弧0~4（重放/起点不符必抓；
        // 弧5取回是合法弧，故倒退面只扫 0..5）。
        if let Some(t0) = ledger.tries.get_mut(0) {
            *t0 += 1;
        }
        let mut t = WaitTicket::new(SyncKind::Wait);
        let _ = t.step(0);
        let _ = t.step(1 + (rng.below(3) as usize)); // 弧二/三/四之一了结
        let arc = rng.below(5) as usize;
        if t.step(arc).is_err() {
            if let Some(c0) = ledger.caught.get_mut(0) {
                *c0 += 1;
            }
        }
        // 族1：PAGE 违规组合注入——NOCACHE 与 WRITECOMBINE 同用必抓。
        if let Some(t1) = ledger.tries.get_mut(1) {
            *t1 += 1;
        }
        let base = rng.below(PAGE_BASE_N as u64) as usize;
        let guarded = rng.below(2) == 0;
        if PageMatrix::combine(base, guarded, 3).is_err() {
            if let Some(c1) = ledger.caught.get_mut(1) {
                *c1 += 1;
            }
        }
        // 族2：非属主释放注入——他客令牌释放必抓。
        if let Some(t2) = ledger.tries.get_mut(2) {
            *t2 += 1;
        }
        let owner = (rng.below(0x1000) as u32) | 1;
        let mut m = Mutant::new();
        let _ = m.acquire(owner);
        let intruder = owner.wrapping_add(1 + (rng.below(0xFF) as u32));
        if m.release(intruder).is_err() {
            if let Some(c2) = ledger.caught.get_mut(2) {
                *c2 += 1;
            }
        }
        round += 1;
    }
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ──────────────────────
/// 六弧对照回归绿：黄金正演走通 + 弧表冻结。
fn six_arc_ok() -> bool {
    match golden_run() {
        Ok(n) => n == 12 && WAIT_ARCS.len() == ARC_COUNT,
        Err(_) => false,
    }
}

/// PAGE_* 全集矩阵维持：16 格登记齐 + 互斥规则守恒。
fn page_matrix_ok() -> bool {
    let mut pm = PageMatrix::new();
    let mut b = 0usize;
    while b < PAGE_BASE_N {
        if pm.mark(b, false).is_err() || pm.mark(b, true).is_err() {
            return false;
        }
        b += 1;
    }
    if !pm.complete() {
        return false;
    }
    // 互斥规则抽验：全基础档下违规组合一律拒。
    let mut b2 = 0usize;
    while b2 < PAGE_BASE_N {
        if PageMatrix::combine(b2, false, 3).is_ok() {
            return false;
        }
        b2 += 1;
    }
    true
}

/// 三态倒退显性拒：越表倒退被起点关拦下，账目倒退（空槽收单）被队列关拦下。
fn backward_reject_ok() -> bool {
    // 了结态（重放）走结型弧 →重放拒；观望起点弧 →起点不符拒。
    let mut t = WaitTicket::new(SyncKind::Wait);
    let _ = t.step(0);
    let _ = t.step(1);
    if t.step(4).is_ok() {
        return false;
    }
    // 账目倒退：空队列收单必拒；挂单后正常收单放行；重复收单（倒退）必拒。
    let mut q = WaitQueue::new();
    if q.dequeue(0).is_ok() {
        return false;
    }
    let slot = match q.enqueue() {
        Ok(s) => s,
        Err(_) => return false,
    };
    if q.dequeue(slot).is_err() {
        return false;
    }
    q.dequeue(slot).is_err()
}

/// 判定次序：挂等先于了结、了结先于取回，乱序必拒。
fn settle_order_ok() -> bool {
    // 未挂等先了结：起点不符拒。
    let mut t = WaitTicket::new(SyncKind::Mutant);
    if t.step(2).is_ok() {
        return false;
    }
    // 未了结先取回：起点不符拒。
    let mut t2 = WaitTicket::new(SyncKind::Semaphore);
    let _ = t2.step(0);
    t2.step(5).is_err()
}

/// 容量耗尽：等待队列 8 槽满后必拒，收单后可复挂。
fn capacity_ok() -> bool {
    let mut q = WaitQueue::new();
    let mut slots: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < WAIT_QUEUE_CAP {
        match q.enqueue() {
            Ok(s) => slots.push(s),
            Err(_) => return false,
        }
        i += 1;
    }
    if q.enqueue().is_ok() {
        return false;
    }
    if let Some(first) = slots.get(0) {
        if q.dequeue(*first).is_err() {
            return false;
        }
    }
    // 收一槽后恰可再挂一单（计数守恒）。
    q.enqueue().is_ok() && q.count == WAIT_QUEUE_CAP
}

/// 双跑一致：黄金正演跑两遍结果逐位一致。
fn double_run_ok() -> bool {
    let a = golden_run();
    let b = golden_run();
    a == b
}

/// 对账一致/计数守恒：信号量账本与注入轮数守恒。
fn conservation_ok() -> bool {
    let mut sema = Semaphore::new(2);
    let mut i = 0u32;
    while i < 5 {
        let _ = sema.acquire();
        let _ = sema.release();
        i += 1;
    }
    if sema.verify_ledger().is_err() {
        return false;
    }
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    led.all_caught() && led.ten_rounds_each()
}

/// 文案闭集：10 码全查得非空中文，未知码给兜底。
fn text_closed_ok() -> bool {
    let mut i = 0usize;
    while i < SyncDiag::ALL.len() {
        if let Some(c) = SyncDiag::ALL.get(i) {
            let d = detail(c.val());
            if d.is_empty() || d == "未知诊断码：不在 0x82100 段闭集内" {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F) == "未知诊断码：不在 0x82100 段闭集内"
}

/// 段位守卫：SEG_BASE 对齐 0x40 且落在 VX9 区间，诊断码全在段窗内。
fn segment_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x82FC0 {
        return false;
    }
    let mut i = 0usize;
    while i < SyncDiag::ALL.len() {
        if let Some(c) = SyncDiag::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 收口只增不减：断言组重跑不劣化。
fn closing_ok() -> bool {
    let a = run_asserts();
    let b = run_asserts();
    b >= a && a == 12
}

// ── 检查名/静态明细闭集（match 查表，无裸下标；name 须 &'static str） ──
/// 第 i 条检查的静态名（闭集 match 查表，越表给兜底）。
fn check_name(i: usize) -> &'static str {
    match i {
        0 => "J1-六弧对照回归绿",
        1 => "J1-PAGE全集矩阵维持",
        2 => "J1-同步族四对象语义对照",
        3 => "J1-三态倒退显性拒",
        4 => "J1-判定次序先行",
        5 => "J1-容量耗尽必抓",
        6 => "J1-注入族一·倒退10轮全抓",
        7 => "J1-注入族二·PAGE组合全抓",
        8 => "J1-注入族三·属主10轮全抓",
        9 => "J1-双跑一致",
        10 => "J1-对账一致·计数守恒",
        11 => "J1-文案闭集",
        12 => "J1-段位守卫0x82100",
        13 => "J1-收口只增不减",
        _ => "越表检查项",
    }
}

/// 第 i 条检查的静态明细兜底（第 9 条起走 s.add 静态明细，动态槽只留 8 个）。
fn check_static(i: usize) -> &'static str {
    match i {
        0 => "三态六弧全走通·断言12/12",
        1 => "16格登记齐·互斥规则守恒",
        2 => "Wait/Event/Semaphore/Mutant全过",
        3 => "越表倒退·空槽收单一律拒",
        4 => "挂等→了结→取回乱序必拒",
        5 => "8槽满拒·收单复挂守恒",
        6 => "倒退注入10/10",
        7 => "违规组合注入10/10",
        8 => "非属主注入10/10",
        9 => "两次黄金正演逐位一致",
        10 => "信号量账本·注入轮数守恒",
        11 => "10码查表非空·未知码兜底",
        12 => "0x40对齐·VX9区间·段窗内",
        13 => "断言组重跑不劣化",
        _ => "明细缺表",
    }
}

/// 注入账本族读数（.get 取值，无裸下标）。
fn ledger_pair(led: &InjectLedger, family: usize) -> (u32, u32) {
    let t = match led.tries.get(family) {
        Some(v) => *v,
        None => 0,
    };
    let c = match led.caught.get(family) {
        Some(v) => *v,
        None => 0,
    };
    (c, t)
}

/// extra_checks：14 条检查（正常流/边界/注入/判定次序/双跑/守恒/守卫/收口）。
pub fn extra_checks() -> Vec<(String, bool, String)> {
    let mut v: Vec<(String, bool, String)> = Vec::new();
    // 1：六弧对照回归绿（判据锚点主谓词）。
    let six = six_arc_ok();
    v.push((
        String::from(check_name(0)),
        six,
        format!("asserts={:?}", golden_run()),
    ));
    // 2：PAGE 全集矩阵维持（判据锚点副谓词）。
    let mut pm_probe = PageMatrix::new();
    let mut b = 0usize;
    while b < PAGE_BASE_N {
        let _ = pm_probe.mark(b, false);
        let _ = pm_probe.mark(b, true);
        b += 1;
    }
    v.push((
        String::from(check_name(1)),
        page_matrix_ok(),
        format!("cells={}/{}", pm_probe.marked, PAGE_MATRIX_CELLS),
    ));
    // 3：同步族四对象语义对照。
    v.push((
        String::from(check_name(2)),
        sync_family_ok(),
        String::from("Event两型·Semaphore守恒·Mutant弃权"),
    ));
    // 4：三态倒退显性拒。
    v.push((String::from(check_name(3)), backward_reject_ok(), String::from(check_static(3))));
    // 5：判定次序先行。
    v.push((String::from(check_name(4)), settle_order_ok(), String::from(check_static(4))));
    // 6：容量耗尽必抓。
    v.push((String::from(check_name(5)), capacity_ok(), String::from(check_static(5))));
    // 7~9：注入三族各 10 轮全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    let p0 = ledger_pair(&led, 0);
    v.push((
        String::from(check_name(6)),
        p0.0 == p0.1 && p0.1 == 10,
        format!("c/t={}/{}", p0.0, p0.1),
    ));
    let p1 = ledger_pair(&led, 1);
    v.push((
        String::from(check_name(7)),
        p1.0 == p1.1 && p1.1 == 10,
        format!("c/t={}/{}", p1.0, p1.1),
    ));
    let p2 = ledger_pair(&led, 2);
    v.push((
        String::from(check_name(8)),
        p2.0 == p2.1 && p2.1 == 10,
        format!("c/t={}/{}", p2.0, p2.1),
    ));
    // 10：双跑一致。
    v.push((String::from(check_name(9)), double_run_ok(), String::from(check_static(9))));
    // 11：对账一致/计数守恒。
    v.push((
        String::from(check_name(10)),
        conservation_ok(),
        String::from("信号量账本+注入账本双守恒"),
    ));
    // 12：文案闭集。
    v.push((String::from(check_name(11)), text_closed_ok(), String::from(check_static(11))));
    // 13：段位守卫。
    v.push((String::from(check_name(12)), segment_guard_ok(), String::from(check_static(12))));
    // 14：收口只增不减。
    v.push((String::from(check_name(13)), closing_ok(), String::from(check_static(13))));
    v
}

/// 同步族四对象语义对照谓词（第 3 条检查的判定核）。
fn sync_family_ok() -> bool {
    // 通知型事件：置信保持。
    let mut ev_n = SyncEvent::new(EventMode::Notification);
    if !ev_n.set() || !ev_n.signaled {
        return false;
    }
    ev_n.reset();
    if ev_n.signaled {
        return false;
    }
    // 同步型事件：放行即自动复位。
    let mut ev_s = SyncEvent::new(EventMode::Synchronization);
    if ev_s.set() {
        return false;
    }
    // 信号量：取用/释放闭环 + 守恒。
    let mut sema = Semaphore::new(1);
    if sema.acquire() != Ok(0) {
        return false;
    }
    if sema.release() != Ok(1) {
        return false;
    }
    // 变异体：属主闭环 + 弃权后可被他人接管。
    let mut mutx = Mutant::new();
    if mutx.acquire(7) != Ok(1) {
        return false;
    }
    if mutx.release(7).is_err() {
        return false;
    }
    let _ = mutx.acquire(8);
    match mutx.abandon(8) {
        Ok(()) => mutx.owner.is_none() && mutx.abandoned,
        Err(_) => false,
    }
}

// ── 检查入口 ───────────────────────────────────────────────────────
/// VX9 检查入口：extra_checks 逐条进 set（动态槽只给最关键 8 条）。
pub fn run_unx12732_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12732 同步族语义对照回归（Wait/Event/Semaphore/Mutant）");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((_name_dyn, passed, detail_dyn)) = items.get(i) {
            let name = check_name(i);
            if i < 8 {
                // 动态明细槽（≤8）：判据锚点与注入族优先。
                s.add_dyn(name, *passed, detail_dyn);
            } else {
                // 静态明细：不占动态槽，红项照记不丢。
                s.add(name, *passed, check_static(i));
            }
        }
        i += 1;
    }
    s
}

// ── 测试节（≥8 条 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_arcs_full_walk_green() {
        // 正常流：六弧按合法序全走通，黄金正演 12/12。
        assert_eq!(golden_run(), Ok(12));
        assert!(six_arc_ok());
    }

    #[test]
    fn page_matrix_full_set_maintained() {
        // 正常流：16 格登记齐 + 互斥规则守恒。
        let mut pm = PageMatrix::new();
        let mut b = 0usize;
        while b < PAGE_BASE_N {
            assert!(pm.mark(b, false).is_ok());
            assert!(pm.mark(b, true).is_ok());
            b += 1;
        }
        assert!(pm.complete());
        assert!(page_matrix_ok());
    }

    #[test]
    fn backward_move_explicitly_rejected() {
        // 注入：倒退/起点不符必抓（了结态挂新等、未挂等先了结）。
        let mut t = WaitTicket::new(SyncKind::Wait);
        let _ = t.step(0);
        let _ = t.step(1);
        assert_eq!(t.step(0), Err(SyncDiag::ArcOriginMismatch));
        assert!(backward_reject_ok());
    }

    #[test]
    fn settle_replay_rejected() {
        // 注入：了结重放必抓。
        let mut t = WaitTicket::new(SyncKind::Event);
        let _ = t.step(0);
        let _ = t.step(2);
        assert_eq!(t.step(2), Err(SyncDiag::SettleReplay));
        assert!(t.step(5).is_ok());
    }

    #[test]
    fn page_combine_rule_violation_caught() {
        // 注入：NOCACHE 与 WRITECOMBINE 同用必抓；合法组合放行。
        assert_eq!(PageMatrix::combine(2, false, 3), Err(SyncDiag::PageCombineRule));
        assert!(PageMatrix::combine(2, true, 1).is_ok());
        assert_eq!(PageMatrix::combine(9, false, 0), Err(SyncDiag::PageMatrixHole));
    }

    #[test]
    fn mutant_ownership_enforced() {
        // 注入：非属主释放必抓；属主闭环绿；弃权后清主。
        let mut m = Mutant::new();
        assert!(m.acquire(5).is_ok());
        assert_eq!(m.release(6), Err(SyncDiag::MutantNotOwner));
        assert!(m.release(5).is_ok());
        let _ = m.acquire(9);
        assert!(m.abandon(9).is_ok());
        assert!(m.owner.is_none() && m.abandoned);
    }

    #[test]
    fn semaphore_bounds_and_ledger() {
        // 边界：零计数取用拒、满额释放拒、账本守恒。
        let mut s = Semaphore::new(0);
        assert_eq!(s.acquire(), Err(SyncDiag::SemaLedgerBreak));
        let mut s2 = Semaphore::new(SEMA_MAX);
        assert_eq!(s2.release(), Err(SyncDiag::SemaMaxReached));
        let mut s3 = Semaphore::new(2);
        let _ = s3.acquire();
        let _ = s3.release();
        assert!(s3.verify_ledger().is_ok());
    }

    #[test]
    fn queue_capacity_exhaustion() {
        // 边界：8 槽满拒，收单复挂守恒。
        assert!(capacity_ok());
        let mut q = WaitQueue::new();
        let mut i = 0usize;
        while i < WAIT_QUEUE_CAP {
            assert!(q.enqueue().is_ok());
            i += 1;
        }
        assert_eq!(q.enqueue(), Err(SyncDiag::WaitQueueFull));
    }

    #[test]
    fn injections_three_families_all_caught() {
        // 注入：三族各 10 轮全抓。
        let mut led = InjectLedger::new();
        run_injections(&mut led);
        assert!(led.all_caught());
        assert!(led.ten_rounds_each());
    }

    #[test]
    fn double_run_identical() {
        // 双跑一致：两次黄金正演逐位一致。
        assert!(double_run_ok());
        assert_eq!(extra_checks().len(), 14);
    }

    #[test]
    fn segment_and_text_closed() {
        // 段位守卫 + 文案闭集 + 未知码兜底。
        assert!(segment_guard_ok());
        assert!(text_closed_ok());
        assert_eq!(SEG_BASE, 0x82100);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(0xFFFFFFFF), "未知诊断码：不在 0x82100 段闭集内");
    }

    #[test]
    fn checks_all_green_in_set() {
        // 全绿：检查入口 14 条全过、无截断。
        let s = run_unx12732_checks();
        assert_eq!(s.len(), 14);
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.dyn_used() <= 8);
    }
}
