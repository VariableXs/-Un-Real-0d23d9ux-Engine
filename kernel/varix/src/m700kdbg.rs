//! m700kdbg — VARIX-M700 AI-08 内核调试域 (F176~F200)
//!
//! 统一日志谱/环形 trace 缓冲/崩溃解剖台/符号回映器/断点桩协议/
//! 观测探针框架/状态导出器/调试总线/时间旅行缓冲/活锁探测器/
//! 内存水位直播/调用追踪采样/断言飞行记录/远程调试协议/崩溃自动归档/
//! 调试降级阶梯/幻影释放检测/未初始化哨兵/调试信息自检/多核一致性视图/
//! 压测剧本库/调试预算官/探针命名规范/调试金样本/调试域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。
//! 自检入口：`run_m700kdbg_checks()`（铁律：断言中凡"操作→读→再操作"，
//! 中间读数必须先落入独立 `let` 变量，禁止末态读取）。

use crate::checks::CheckSet;

// ===========================================================================
// F176 — 统一日志谱：分级过滤 + 模块登记去重
// ===========================================================================

pub const KDBG_LOG_TRACE: u8 = 0;
pub const KDBG_LOG_DEBUG: u8 = 1;
pub const KDBG_LOG_INFO: u8 = 2;
pub const KDBG_LOG_WARN: u8 = 3;
pub const KDBG_LOG_ERROR: u8 = 4;
pub const KDBG_LOG_FATAL: u8 = 5;
pub const KDBG_LOG_LEVELS: usize = 6;

/// 日志谱过滤：级别数值 ≥ 阈值才放行。
pub fn kdbg_log_passes(min_level: u8, level: u8) -> bool {
    level >= min_level
}

pub const KDBG_LOG_MODULES_MAX: usize = 8;

/// 模块登记表：同一模块码只登记一次（去重），容量封顶拒绝。
pub struct KdbgLogRegistry {
    codes: [u32; KDBG_LOG_MODULES_MAX],
    count: usize,
    pub rejects: u32,
}

impl KdbgLogRegistry {
    pub const fn new() -> KdbgLogRegistry {
        KdbgLogRegistry { codes: [0; KDBG_LOG_MODULES_MAX], count: 0, rejects: 0 }
    }

    pub fn register(&mut self, code: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.codes[i] == code {
                self.rejects += 1;
                return false;
            }
            i += 1;
        }
        if self.count >= KDBG_LOG_MODULES_MAX {
            self.rejects += 1;
            return false;
        }
        self.codes[self.count] = code;
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F177 — 环形 trace 缓冲：定容覆写，溢出记账
// ===========================================================================

pub const KDBG_TRACE_RING: usize = 16;

pub struct KdbgTraceRing {
    seqs: [u64; KDBG_TRACE_RING],
    marks: [u8; KDBG_TRACE_RING],
    head: usize,
    pub pushed: u64,
    pub overwritten: u64,
}

impl KdbgTraceRing {
    pub const fn new() -> KdbgTraceRing {
        KdbgTraceRing { seqs: [0; KDBG_TRACE_RING], marks: [0; KDBG_TRACE_RING], head: 0, pushed: 0, overwritten: 0 }
    }

    pub fn push(&mut self, seq: u64, mark: u8) {
        if self.pushed >= KDBG_TRACE_RING as u64 {
            self.overwritten += 1;
        }
        self.seqs[self.head] = seq;
        self.marks[self.head] = mark;
        self.head = (self.head + 1) % KDBG_TRACE_RING;
        self.pushed += 1;
    }

    /// 读第 i 条（按写入序号取模，读到的是该槽最新值）。
    pub fn seq_at(&self, i: usize) -> u64 {
        self.seqs[i % KDBG_TRACE_RING]
    }

    pub fn mark_at(&self, i: usize) -> u8 {
        self.marks[i % KDBG_TRACE_RING]
    }

    pub fn len(&self) -> usize {
        KDBG_TRACE_RING
    }
}

// ===========================================================================
// F178 — 崩溃解剖台：崩溃分类、严重级与记录合法性
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KdbgCrashKind {
    PageFault,
    Panic,
    Watchdog,
    DoubleFault,
}

/// 严重级：页错 1 < Panic 2 < 看门狗 3 < 双重错 4。
pub fn kdbg_crash_severity(kind: KdbgCrashKind) -> u8 {
    match kind {
        KdbgCrashKind::PageFault => 1,
        KdbgCrashKind::Panic => 2,
        KdbgCrashKind::Watchdog => 3,
        KdbgCrashKind::DoubleFault => 4,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct KdbgCrash {
    pub kind: KdbgCrashKind,
    pub pc: u64,
    pub fault_addr: u64,
    pub depth: u8,
}

/// 剖析记录合法：PC 非零且栈深在 1..=32。
pub fn kdbg_autopsy_valid(c: &KdbgCrash) -> bool {
    c.pc != 0 && c.depth > 0 && c.depth <= 32
}

// ===========================================================================
// F179 — 符号回映器：地址区间 → 符号 id（基址去重登记）
// ===========================================================================

pub const KDBG_SYMBOLS_MAX: usize = 8;

pub struct KdbgSymTab {
    bases: [u64; KDBG_SYMBOLS_MAX],
    ids: [u32; KDBG_SYMBOLS_MAX],
    count: usize,
    pub rejects: u32,
}

impl KdbgSymTab {
    pub const fn new() -> KdbgSymTab {
        KdbgSymTab { bases: [0; KDBG_SYMBOLS_MAX], ids: [0; KDBG_SYMBOLS_MAX], count: 0, rejects: 0 }
    }

    /// 登记符号：基址重复或表满拒绝；按基址升序插入。
    pub fn add_symbol(&mut self, base: u64, id: u32) -> bool {
        if self.count >= KDBG_SYMBOLS_MAX {
            self.rejects += 1;
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.bases[i] == base {
                self.rejects += 1;
                return false;
            }
            i += 1;
        }
        let mut pos = self.count;
        let mut k = 0usize;
        while k < self.count {
            if self.bases[k] > base {
                pos = k;
                break;
            }
            k += 1;
        }
        let mut j = self.count;
        while j > pos {
            self.bases[j] = self.bases[j - 1];
            self.ids[j] = self.ids[j - 1];
            j -= 1;
        }
        self.bases[pos] = base;
        self.ids[pos] = id;
        self.count += 1;
        true
    }

    /// 回映：取 ≤ addr 的最大基址对应的符号 id；无命中返回 0。
    pub fn symbol_for(&self, addr: u64) -> u32 {
        let mut best = 0u32;
        let mut i = 0usize;
        while i < self.count {
            if self.bases[i] <= addr {
                best = self.ids[i];
            }
            i += 1;
        }
        best
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F180 — 断点桩协议：Armed → Hit → Resumed 状态机（0xCC 桩）
// ===========================================================================

pub const KDBG_BP_MAGIC: u8 = 0xCC;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KdbgBpState {
    Armed,
    Hit,
    Resumed,
}

pub struct KdbgBreakpoint {
    pub addr: u64,
    pub state: KdbgBpState,
    pub hits: u32,
}

impl KdbgBreakpoint {
    pub const fn new(addr: u64) -> KdbgBreakpoint {
        KdbgBreakpoint { addr, state: KdbgBpState::Armed, hits: 0 }
    }

    /// 只有 Armed 态可命中。
    pub fn trigger(&mut self) -> bool {
        if matches!(self.state, KdbgBpState::Armed) {
            self.state = KdbgBpState::Hit;
            self.hits += 1;
            true
        } else {
            false
        }
    }

    /// 只有 Hit 态可恢复。
    pub fn resume(&mut self) -> bool {
        if matches!(self.state, KdbgBpState::Hit) {
            self.state = KdbgBpState::Resumed;
            true
        } else {
            false
        }
    }

    /// 只有 Resumed 态可重新布防。
    pub fn rearm(&mut self) -> bool {
        if matches!(self.state, KdbgBpState::Resumed) {
            self.state = KdbgBpState::Armed;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F181 — 观测探针框架：探针登记去重 + 命中计数
// ===========================================================================

pub const KDBG_PROBES_MAX: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct KdbgProbe {
    pub id: u32,
    pub hits: u64,
}

pub struct KdbgProbeBoard {
    probes: [KdbgProbe; KDBG_PROBES_MAX],
    count: usize,
    pub rejects: u32,
}

impl KdbgProbeBoard {
    pub const fn new() -> KdbgProbeBoard {
        KdbgProbeBoard { probes: [KdbgProbe { id: 0, hits: 0 }; KDBG_PROBES_MAX], count: 0, rejects: 0 }
    }

    /// 挂探针：id 重复或板满拒绝。
    pub fn attach(&mut self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.probes[i].id == id {
                self.rejects += 1;
                return false;
            }
            i += 1;
        }
        if self.count >= KDBG_PROBES_MAX {
            self.rejects += 1;
            return false;
        }
        self.probes[self.count] = KdbgProbe { id, hits: 0 };
        self.count += 1;
        true
    }

    /// 触发探针：未知 id 拒绝。
    pub fn fire(&mut self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.probes[i].id == id {
                self.probes[i].hits += 1;
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn hits_of(&self, id: u32) -> u64 {
        let mut i = 0usize;
        while i < self.count {
            if self.probes[i].id == id {
                return self.probes[i].hits;
            }
            i += 1;
        }
        0
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F182 — 状态导出器：定容字节化导出（版本 + 计数器，小端）
// ===========================================================================

pub const KDBG_EXPORT_VERSION: u8 = 0xA7;
/// 1 版本 + 4 uptime + 2 tasks + 4 heap = 11 字节。
pub const KDBG_EXPORT_SIZE: usize = 11;

#[derive(Clone, Copy, Debug)]
pub struct KdbgExportState {
    pub uptime_ms: u32,
    pub tasks: u16,
    pub heap_used_kb: u32,
}

/// 写入 out（可截断），返回实际写字节数；完整需要 KDBG_EXPORT_SIZE。
pub fn kdbg_export(s: &KdbgExportState, out: &mut [u8]) -> usize {
    let full = [
        KDBG_EXPORT_VERSION,
        s.uptime_ms as u8,
        (s.uptime_ms >> 8) as u8,
        (s.uptime_ms >> 16) as u8,
        (s.uptime_ms >> 24) as u8,
        s.tasks as u8,
        (s.tasks >> 8) as u8,
        s.heap_used_kb as u8,
        (s.heap_used_kb >> 8) as u8,
        (s.heap_used_kb >> 16) as u8,
        (s.heap_used_kb >> 24) as u8,
    ];
    let cap = if out.len() < KDBG_EXPORT_SIZE { out.len() } else { KDBG_EXPORT_SIZE };
    let mut n = 0usize;
    while n < cap {
        out[n] = full[n];
        n += 1;
    }
    n
}

// ===========================================================================
// F183 — 调试总线：4 通道，(通道, 监听者) 订阅去重
// ===========================================================================

pub const KDBG_BUS_CHANNELS: usize = 4;
pub const KDBG_BUS_SUBS_MAX: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct KdbgBusSub {
    pub channel: usize,
    pub listener: u32,
}

pub struct KdbgBus {
    subs: [KdbgBusSub; KDBG_BUS_SUBS_MAX],
    count: usize,
    pub rejects: u32,
}

impl KdbgBus {
    pub const fn new() -> KdbgBus {
        KdbgBus { subs: [KdbgBusSub { channel: 0, listener: 0 }; KDBG_BUS_SUBS_MAX], count: 0, rejects: 0 }
    }

    pub fn subscribe(&mut self, channel: usize, listener: u32) -> bool {
        if channel >= KDBG_BUS_CHANNELS {
            self.rejects += 1;
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.subs[i].channel == channel && self.subs[i].listener == listener {
                self.rejects += 1;
                return false;
            }
            i += 1;
        }
        if self.count >= KDBG_BUS_SUBS_MAX {
            self.rejects += 1;
            return false;
        }
        self.subs[self.count] = KdbgBusSub { channel, listener };
        self.count += 1;
        true
    }

    /// 消息路由：投给该通道第一个订阅者（登记序）。
    pub fn route(&self, channel: usize) -> Option<u32> {
        let mut i = 0usize;
        while i < self.count {
            if self.subs[i].channel == channel {
                return Some(self.subs[i].listener);
            }
            i += 1;
        }
        None
    }
}

// ===========================================================================
// F184 — 时间旅行缓冲：4 槽代际快照，只回放活槽代际
// ===========================================================================

pub const KDBG_TT_SLOTS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdbgTtState {
    pub gen: u32,
    pub value: u64,
}

pub struct KdbgTimeMachine {
    slots: [Option<KdbgTtState>; KDBG_TT_SLOTS],
    head: usize,
    pub recorded: u32,
}

impl KdbgTimeMachine {
    pub const fn new() -> KdbgTimeMachine {
        KdbgTimeMachine { slots: [None; KDBG_TT_SLOTS], head: 0, recorded: 0 }
    }

    pub fn record(&mut self, gen: u32, value: u64) {
        self.slots[self.head] = Some(KdbgTtState { gen, value });
        self.head = (self.head + 1) % KDBG_TT_SLOTS;
        self.recorded += 1;
    }

    pub fn replay(&self, gen: u32) -> Option<KdbgTtState> {
        let mut i = 0usize;
        while i < KDBG_TT_SLOTS {
            if let Some(s) = self.slots[i] {
                if s.gen == gen {
                    return Some(s);
                }
            }
            i += 1;
        }
        None
    }
}

// ===========================================================================
// F185 — 活锁探测器：自旋量大且进展比 < 10‰ 判活锁
// ===========================================================================

pub const KDBG_LIVELOCK_MIN_SPINS: u64 = 64;
pub const KDBG_LIVELOCK_PROGRESS_PERMILLE: u64 = 10;

pub fn kdbg_livelock(spins: u64, progress: u64) -> bool {
    spins >= KDBG_LIVELOCK_MIN_SPINS && progress * 1000 < spins * KDBG_LIVELOCK_PROGRESS_PERMILLE
}

// ===========================================================================
// F186 — 内存水位直播：三区判定 + 采样最值
// ===========================================================================

pub const KDBG_MEM_CRITICAL_PERMILLE: u32 = 100;
pub const KDBG_MEM_TIGHT_PERMILLE: u32 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KdbgMemZone {
    Comfort,
    Tight,
    Critical,
}

/// 按空闲 permille 分区：≤100‰ 危急，≤300‰ 紧张，其余舒适。
pub fn kdbg_mem_zone(free_permille: u32) -> KdbgMemZone {
    if free_permille <= KDBG_MEM_CRITICAL_PERMILLE {
        KdbgMemZone::Critical
    } else if free_permille <= KDBG_MEM_TIGHT_PERMILLE {
        KdbgMemZone::Tight
    } else {
        KdbgMemZone::Comfort
    }
}

#[derive(Clone, Copy, Debug)]
pub struct KdbgWaterFeed {
    samples: u32,
    min_free: u32,
    max_free: u32,
}

impl KdbgWaterFeed {
    pub const fn new() -> KdbgWaterFeed {
        KdbgWaterFeed { samples: 0, min_free: 0, max_free: 0 }
    }

    pub fn observe(&mut self, free_permille: u32) {
        if self.samples == 0 {
            self.min_free = free_permille;
            self.max_free = free_permille;
        } else {
            if free_permille < self.min_free {
                self.min_free = free_permille;
            }
            if free_permille > self.max_free {
                self.max_free = free_permille;
            }
        }
        self.samples += 1;
    }

    pub fn min(&self) -> u32 {
        self.min_free
    }

    pub fn max(&self) -> u32 {
        self.max_free
    }

    pub fn samples(&self) -> u32 {
        self.samples
    }
}

// ===========================================================================
// F187 — 调用追踪采样：每 N 次调用采一次（1/N）
// ===========================================================================

pub const fn kdbg_sampler_every(every: u32) -> u32 {
    if every == 0 {
        1
    } else {
        every
    }
}

#[derive(Clone, Copy, Debug)]
pub struct KdbgCallSampler {
    pub every: u32,
    pub seen: u64,
    pub traced: u64,
}

impl KdbgCallSampler {
    pub const fn new(every: u32) -> KdbgCallSampler {
        KdbgCallSampler { every: kdbg_sampler_every(every), seen: 0, traced: 0 }
    }

    pub fn observe(&mut self) -> bool {
        self.seen += 1;
        if self.seen % self.every as u64 == 0 {
            self.traced += 1;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F188 — 断言飞行记录：8 槽黑匣子，满后拒绝并记账
// ===========================================================================

pub const KDBG_FLIGHT_SLOTS: usize = 8;

pub struct KdbgFlightLog {
    lines: [u32; KDBG_FLIGHT_SLOTS],
    count: usize,
    pub overflowed: u32,
}

impl KdbgFlightLog {
    pub const fn new() -> KdbgFlightLog {
        KdbgFlightLog { lines: [0; KDBG_FLIGHT_SLOTS], count: 0, overflowed: 0 }
    }

    pub fn record(&mut self, code: u32) -> bool {
        if self.count >= KDBG_FLIGHT_SLOTS {
            self.overflowed += 1;
            return false;
        }
        self.lines[self.count] = code;
        self.count += 1;
        true
    }

    pub fn last(&self) -> u32 {
        if self.count == 0 {
            0
        } else {
            self.lines[self.count - 1]
        }
    }

    pub fn at(&self, i: usize) -> Option<u32> {
        if i < self.count {
            Some(self.lines[i])
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F189 — 远程调试协议：[magic][len][payload][cksum] 帧校验
// ===========================================================================

pub const KDBG_RDP_MAGIC: u8 = 0x56;

/// 校验和：payload 字节和的补码（和为 0 时仍为 0）。
pub fn kdbg_rdp_cksum(payload: &[u8]) -> u8 {
    let mut sum = 0u8;
    let mut i = 0usize;
    while i < payload.len() {
        sum = sum.wrapping_add(payload[i]);
        i += 1;
    }
    sum.wrapping_neg()
}

pub fn kdbg_rdp_frame_len(payload_len: usize) -> usize {
    payload_len + 3
}

/// 组帧到 out，返回写字节数（out 不足时截断）。
pub fn kdbg_rdp_build(payload: &[u8], out: &mut [u8]) -> usize {
    let need = kdbg_rdp_frame_len(payload.len());
    let cap = if out.len() < need { out.len() } else { need };
    let mut n = 0usize;
    while n < cap {
        let b = if n == 0 {
            KDBG_RDP_MAGIC
        } else if n == 1 {
            payload.len() as u8
        } else if n < need - 1 {
            payload[n - 2]
        } else {
            kdbg_rdp_cksum(payload)
        };
        out[n] = b;
        n += 1;
    }
    n
}

pub fn kdbg_rdp_valid(frame: &[u8]) -> bool {
    if frame.len() < 3 {
        return false;
    }
    let plen = frame[1] as usize;
    if frame.len() != plen + 3 {
        return false;
    }
    if frame[0] != KDBG_RDP_MAGIC {
        return false;
    }
    frame[2 + plen] == kdbg_rdp_cksum(&frame[2..2 + plen])
}

// ===========================================================================
// F190 — 崩溃自动归档：签名（kind, pc）去重归档
// ===========================================================================

pub const KDBG_ARCHIVE_MAX: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdbgCrashSig {
    pub kind: u8,
    pub pc: u64,
}

pub struct KdbgArchive {
    sigs: [KdbgCrashSig; KDBG_ARCHIVE_MAX],
    count: usize,
    pub dup_rejects: u32,
}

impl KdbgArchive {
    pub const fn new() -> KdbgArchive {
        KdbgArchive { sigs: [KdbgCrashSig { kind: 0, pc: 0 }; KDBG_ARCHIVE_MAX], count: 0, dup_rejects: 0 }
    }

    pub fn archive(&mut self, sig: KdbgCrashSig) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.sigs[i] == sig {
                self.dup_rejects += 1;
                return false;
            }
            i += 1;
        }
        if self.count >= KDBG_ARCHIVE_MAX {
            self.dup_rejects += 1;
            return false;
        }
        self.sigs[self.count] = sig;
        self.count += 1;
        true
    }

    pub fn contains(&self, sig: KdbgCrashSig) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.sigs[i] == sig {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F191 — 调试降级阶梯：错误升级（0..=4 级），健康 3 连降一级
// ===========================================================================

pub const KDBG_LADDER_TOP: u32 = 4;
pub const KDBG_LADDER_HEAL_STREAK: u32 = 3;

#[derive(Clone, Copy, Debug)]
pub struct KdbgLadder {
    pub level: u32,
    pub healthy_streak: u32,
}

impl KdbgLadder {
    pub const fn new() -> KdbgLadder {
        KdbgLadder { level: 0, healthy_streak: 0 }
    }

    pub fn escalate(&mut self) -> bool {
        if self.level >= KDBG_LADDER_TOP {
            return false;
        }
        self.level += 1;
        true
    }

    /// 返回 true 表示本次健康报告触发了降级。
    pub fn note_healthy(&mut self) -> bool {
        if self.level == 0 {
            return false;
        }
        self.healthy_streak += 1;
        if self.healthy_streak >= KDBG_LADDER_HEAL_STREAK {
            self.level -= 1;
            self.healthy_streak = 0;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F192 — 幻影释放检测：代际戳，陈旧代际释放一律拒绝
// ===========================================================================

pub const KDBG_SLOTS_MAX: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct KdbgGenSlot {
    pub live: bool,
    pub gen: u32,
}

pub struct KdbgSlotTable {
    slots: [KdbgGenSlot; KDBG_SLOTS_MAX],
}

impl KdbgSlotTable {
    pub const fn new() -> KdbgSlotTable {
        KdbgSlotTable { slots: [KdbgGenSlot { live: false, gen: 0 }; KDBG_SLOTS_MAX] }
    }

    /// 分配最低空闲槽（代际保持不变）。
    pub fn alloc_slot(&mut self) -> Option<usize> {
        let mut i = 0usize;
        while i < KDBG_SLOTS_MAX {
            if !self.slots[i].live {
                self.slots[i].live = true;
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 释放必须带当前代际；代际不符即幻影释放。
    pub fn free_slot(&mut self, idx: usize, gen: u32) -> bool {
        if idx >= KDBG_SLOTS_MAX {
            return false;
        }
        let s = &mut self.slots[idx];
        if !s.live || s.gen != gen {
            return false;
        }
        s.live = false;
        s.gen += 1;
        true
    }

    pub fn slot(&self, idx: usize) -> KdbgGenSlot {
        self.slots[idx]
    }
}

// ===========================================================================
// F193 — 未初始化哨兵：0xA5 毒化填充与越界写检测
// ===========================================================================

pub const KDBG_POISON_BYTE: u8 = 0xA5;

pub fn kdbg_poison(buf: &mut [u8]) {
    let mut i = 0usize;
    while i < buf.len() {
        buf[i] = KDBG_POISON_BYTE;
        i += 1;
    }
}

pub fn kdbg_is_poisoned(buf: &[u8]) -> bool {
    let mut i = 0usize;
    while i < buf.len() {
        if buf[i] != KDBG_POISON_BYTE {
            return false;
        }
        i += 1;
    }
    true
}

/// 下标 `written` 之后是否仍保持毒化（检测只写了前缀）。
pub fn kdbg_tail_unwritten(buf: &[u8], written: usize) -> bool {
    let mut i = written;
    while i < buf.len() {
        if buf[i] != KDBG_POISON_BYTE {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// F194 — 调试信息自检：记录非零尺寸 + 区间有序不重叠
// ===========================================================================

pub const KDBG_DI_MAX: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct KdbgDiRec {
    pub offset: u32,
    pub size: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct KdbgDebugInfo {
    pub recs: [KdbgDiRec; KDBG_DI_MAX],
    pub count: usize,
}

impl KdbgDebugInfo {
    pub const fn new() -> KdbgDebugInfo {
        KdbgDebugInfo { recs: [KdbgDiRec { offset: 0, size: 0 }; KDBG_DI_MAX], count: 0 }
    }
}

pub fn kdbg_debuginfo_ok(di: &KdbgDebugInfo) -> bool {
    if di.count == 0 || di.count > KDBG_DI_MAX {
        return false;
    }
    if di.recs[0].size == 0 {
        return false;
    }
    let mut i = 1usize;
    while i < di.count {
        let prev_end = di.recs[i - 1].offset.wrapping_add(di.recs[i - 1].size);
        if di.recs[i].size == 0 {
            return false;
        }
        if di.recs[i].offset < prev_end {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// F195 — 多核一致性视图：seqlock 奇偶序号一致性读
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct KdbgSeqView {
    pub seq: u32,
    pub value: u64,
}

impl KdbgSeqView {
    pub const fn new(value: u64) -> KdbgSeqView {
        KdbgSeqView { seq: 0, value }
    }

    pub fn write(&mut self, new_value: u64) {
        self.seq = self.seq.wrapping_add(1);
        self.value = new_value;
        self.seq = self.seq.wrapping_add(1);
    }
}

/// 读窗口稳定：前后序号一致且为偶（不在写中）。
pub fn kdbg_view_stable(before: u32, after: u32) -> bool {
    before == after && before % 2 == 0
}

// ===========================================================================
// F196 — 压测剧本库：步骤登记去重 + 重复次数上限
// ===========================================================================

pub const KDBG_PLAYBOOK_MAX: usize = 8;
pub const KDBG_PLAY_STEP_MAX_REPEAT: u32 = 1000;

#[derive(Clone, Copy, Debug)]
pub struct KdbgPlayStep {
    pub op: u32,
    pub repeat: u32,
}

pub struct KdbgPlaybook {
    steps: [KdbgPlayStep; KDBG_PLAYBOOK_MAX],
    count: usize,
    pub rejects: u32,
}

impl KdbgPlaybook {
    pub const fn new() -> KdbgPlaybook {
        KdbgPlaybook { steps: [KdbgPlayStep { op: 0, repeat: 0 }; KDBG_PLAYBOOK_MAX], count: 0, rejects: 0 }
    }

    /// 同一操作码只登一步；repeat 必须 1..=1000；容量封顶拒绝。
    pub fn add_step(&mut self, op: u32, repeat: u32) -> bool {
        if repeat == 0 || repeat > KDBG_PLAY_STEP_MAX_REPEAT {
            self.rejects += 1;
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.steps[i].op == op {
                self.rejects += 1;
                return false;
            }
            i += 1;
        }
        if self.count >= KDBG_PLAYBOOK_MAX {
            self.rejects += 1;
            return false;
        }
        self.steps[self.count] = KdbgPlayStep { op, repeat };
        self.count += 1;
        true
    }

    pub fn total_iterations(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.count {
            sum += self.steps[i].repeat as u64;
            i += 1;
        }
        sum
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F197 — 调试预算官：额度发放永不透支，超支比 permille
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct KdbgBudget {
    pub quota: u32,
    pub spent: u32,
}

impl KdbgBudget {
    pub const fn new(quota: u32) -> KdbgBudget {
        KdbgBudget { quota, spent: 0 }
    }

    pub fn remaining(&self) -> u32 {
        self.quota.saturating_sub(self.spent)
    }

    /// 申请 n，返回实际批准数（≤ 剩余额度）。
    pub fn spend(&mut self, want: u32) -> u32 {
        let avail = self.remaining();
        let granted = if want < avail { want } else { avail };
        self.spent += granted;
        granted
    }

    pub fn overrun_permille(&self) -> u32 {
        if self.quota == 0 {
            0
        } else {
            self.spent * 1000 / self.quota
        }
    }
}

// ===========================================================================
// F198 — 探针命名规范：3..=24 字节，小写字母开头，[a-z0-9_]
// ===========================================================================

pub const KDBG_PROBE_NAME_MIN: usize = 3;
pub const KDBG_PROBE_NAME_MAX: usize = 24;

pub fn kdbg_probe_name_ok(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() < KDBG_PROBE_NAME_MIN || b.len() > KDBG_PROBE_NAME_MAX {
        return false;
    }
    if !b[0].is_ascii_lowercase() {
        return false;
    }
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_') {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// F199 — 调试金样本：固定剧本 → 固定末态
// ===========================================================================

/// 金样剧本：向 trace 环推 5 条（seq 10..=14，mark 按 1/2 交替）。
pub fn kdbg_golden_scenario() -> KdbgTraceRing {
    let mut r = KdbgTraceRing::new();
    let mut i = 0u64;
    while i < 5 {
        r.push(10 + i, ((i % 2) as u8) + 1);
        i += 1;
    }
    r
}

/// 期望末态：pushed=5、零覆写、首尾序列与标记精确一致。
pub fn kdbg_golden_matches(r: &KdbgTraceRing) -> bool {
    r.pushed == 5
        && r.overwritten == 0
        && r.seq_at(4) == 14
        && r.mark_at(4) == 1
        && r.seq_at(0) == 10
        && r.mark_at(3) == 2
}

// ===========================================================================
// F200 — 调试域年报：年报章节完备性
// ===========================================================================

pub const KDBG_REPORT_SECTIONS: [&str; 5] = ["logs", "traces", "crashes", "probes", "budget"];

pub fn kdbg_report_complete(filled: u32) -> bool {
    filled >= KDBG_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700kdbg_checks() -> CheckSet {
    let mut set = CheckSet::new("m700kdbg");

    // F176 统一日志谱
    set.add(
        "F176 log filter",
        kdbg_log_passes(KDBG_LOG_INFO, KDBG_LOG_WARN) && !kdbg_log_passes(KDBG_LOG_INFO, KDBG_LOG_TRACE)
            && kdbg_log_passes(KDBG_LOG_FATAL, KDBG_LOG_FATAL),
        "level >= threshold",
    );
    let mut reg = KdbgLogRegistry::new();
    let first = reg.register(42);
    let dup = reg.register(42);
    let mut code = 43u32;
    while code <= 49 {
        reg.register(code);
        code += 1;
    }
    let overflow = reg.register(50);
    set.add(
        "F176 log registry dedup+cap",
        first && !dup && overflow == false && reg.len() == 8 && reg.rejects == 2,
        "dedup and capacity",
    );

    // F177 环形 trace 缓冲
    let mut ring = KdbgTraceRing::new();
    let mut i = 0u64;
    while i < 20 {
        ring.push(100 + i, ((i % 3) as u8) + 1);
        i += 1;
    }
    let seq_newest = ring.seq_at(19);
    let seq_old_kept = ring.seq_at(4);
    set.add(
        "F177 trace ring wrap",
        ring.pushed == 20 && ring.overwritten == 4 && seq_newest == 119 && seq_old_kept == 104,
        "16 slots, 4 evicted",
    );
    set.add(
        "F177 trace mark tracked",
        ring.mark_at(19) == 2 && ring.mark_at(4) == 2,
        "marks ride the ring",
    );

    // F178 崩溃解剖台
    set.add(
        "F178 crash severity order",
        kdbg_crash_severity(KdbgCrashKind::PageFault) == 1
            && kdbg_crash_severity(KdbgCrashKind::Panic) == 2
            && kdbg_crash_severity(KdbgCrashKind::Watchdog) == 3
            && kdbg_crash_severity(KdbgCrashKind::DoubleFault) == 4,
        "ascending severity",
    );
    let good = KdbgCrash { kind: KdbgCrashKind::PageFault, pc: 0x1000, fault_addr: 0xdead, depth: 4 };
    let no_pc = KdbgCrash { kind: KdbgCrashKind::Panic, pc: 0, fault_addr: 0, depth: 1 };
    let deep = KdbgCrash { kind: KdbgCrashKind::Panic, pc: 0x10, fault_addr: 0, depth: 33 };
    set.add(
        "F178 autopsy validity",
        kdbg_autopsy_valid(&good) && !kdbg_autopsy_valid(&no_pc) && !kdbg_autopsy_valid(&deep),
        "pc and depth bounds",
    );

    // F179 符号回映器
    let mut syms = KdbgSymTab::new();
    let a1 = syms.add_symbol(0x1000, 7);
    let dup = syms.add_symbol(0x1000, 8);
    let a2 = syms.add_symbol(0x2000, 9);
    set.add("F179 symtab register", a1 && !dup && a2 && syms.len() == 2 && syms.rejects == 1, "base dedup");
    set.add(
        "F179 symtab resolve",
        syms.symbol_for(0x1800) == 7 && syms.symbol_for(0x2800) == 9 && syms.symbol_for(0x0500) == 0,
        "largest base <= addr",
    );

    // F180 断点桩协议
    let mut bp = KdbgBreakpoint::new(0x401000);
    let hit1 = bp.trigger();
    let hits_after_first = bp.hits;
    let hit2 = bp.trigger();
    let resumed = bp.resume();
    set.add(
        "F180 bp fire once",
        hit1 && hits_after_first == 1 && !hit2 && resumed,
        "armed -> hit -> resumed",
    );
    let rearmed = bp.rearm();
    let hits_after_rearm = bp.hits;
    set.add("F180 bp rearm", rearmed && hits_after_rearm == 1, "rearm keeps hit count");

    // F181 观测探针框架
    let mut board = KdbgProbeBoard::new();
    let at1 = board.attach(1);
    let at_dup = board.attach(1);
    let at2 = board.attach(2);
    let mut pid = 3u32;
    while pid <= 8 {
        board.attach(pid);
        pid += 1;
    }
    let at_cap = board.attach(9);
    set.add(
        "F181 probe attach dedup+cap",
        at1 && !at_dup && at2 && !at_cap && board.len() == 8 && board.rejects == 2,
        "id dedup, 8 max",
    );
    let f1 = board.fire(1);
    let f2 = board.fire(1);
    let f3 = board.fire(1);
    let f_unknown = board.fire(99);
    set.add(
        "F181 probe fire counts",
        f1 && f2 && f3 && !f_unknown && board.hits_of(1) == 3 && board.hits_of(2) == 0,
        "hits tallied",
    );

    // F182 状态导出器
    let st = KdbgExportState { uptime_ms: 1000, tasks: 5, heap_used_kb: 2048 };
    let mut buf = [0u8; 32];
    let n = kdbg_export(&st, &mut buf);
    set.add(
        "F182 export full",
        n == KDBG_EXPORT_SIZE && buf[0] == KDBG_EXPORT_VERSION && buf[1] == 0xE8 && buf[2] == 0x03,
        "11 bytes, uptime 1000 LE",
    );
    let mut small = [0u8; 4];
    let n_small = kdbg_export(&st, &mut small);
    set.add(
        "F182 export truncated",
        n_small == 4 && KDBG_EXPORT_SIZE == 11,
        "short buffer clamped",
    );

    // F183 调试总线
    let mut bus = KdbgBus::new();
    let s1 = bus.subscribe(1, 100);
    let s_dup = bus.subscribe(1, 100);
    let s_bad = bus.subscribe(7, 100);
    let s2 = bus.subscribe(1, 200);
    let routed = bus.route(1);
    let silent = bus.route(3);
    set.add(
        "F183 bus subscribe",
        s1 && !s_dup && !s_bad && s2 && bus.rejects == 2,
        "channel bound + dedup",
    );
    set.add("F183 bus route", routed == Some(100) && silent.is_none(), "first subscriber wins");

    // F184 时间旅行缓冲
    let mut tm = KdbgTimeMachine::new();
    let mut gen = 1u32;
    while gen <= 6 {
        tm.record(gen, gen as u64 * 10);
        gen += 1;
    }
    let got6 = tm.replay(6);
    let got5 = tm.replay(5);
    let got1 = tm.replay(1);
    let got2 = tm.replay(2);
    set.add(
        "F184 time travel slots",
        tm.recorded == 6 && got6.map(|s| s.value) == Some(60) && got5.map(|s| s.value) == Some(50),
        "youngest survive",
    );
    set.add("F184 evicted gens gone", got1.is_none() && got2.is_none(), "4 slots only");

    // F185 活锁探测器
    set.add(
        "F185 livelock detect",
        kdbg_livelock(100, 0) && !kdbg_livelock(100, 1) && !kdbg_livelock(63, 0),
        "1% progress is alive",
    );

    // F186 内存水位直播
    set.add(
        "F186 mem zones",
        kdbg_mem_zone(50) == KdbgMemZone::Critical
            && kdbg_mem_zone(100) == KdbgMemZone::Critical
            && kdbg_mem_zone(101) == KdbgMemZone::Tight
            && kdbg_mem_zone(300) == KdbgMemZone::Tight
            && kdbg_mem_zone(301) == KdbgMemZone::Comfort,
        "boundary exact",
    );
    let mut feed = KdbgWaterFeed::new();
    feed.observe(500);
    feed.observe(120);
    feed.observe(800);
    set.add("F186 feed minmax", feed.min() == 120 && feed.max() == 800 && feed.samples() == 3, "live extremes");

    // F187 调用追踪采样
    let mut sampler = KdbgCallSampler::new(3);
    let mut observed = 0u8;
    let mut call = 0;
    while call < 7 {
        if sampler.observe() {
            observed += 1;
        }
        call += 1;
    }
    set.add(
        "F187 sampling 1/3",
        sampler.seen == 7 && sampler.traced == 2 && observed == 2,
        "3rd and 6th traced",
    );
    let mut eager = KdbgCallSampler::new(0);
    let eager_first = eager.observe();
    set.add("F187 every zero normalized", eager.every == 1 && eager_first, "no div by zero");

    // F188 断言飞行记录
    let mut flight = KdbgFlightLog::new();
    let mut code = 1u32;
    let mut ok_count = 0u32;
    while code <= 10 {
        if flight.record(code) {
            ok_count += 1;
        }
        code += 1;
    }
    let last_line = flight.last();
    set.add(
        "F188 flight capacity",
        ok_count == 8 && flight.overflowed == 2 && last_line == 8 && flight.len() == 8,
        "8 slots, 2 dropped",
    );
    set.add("F188 flight slots", flight.at(0) == Some(1) && flight.at(7) == Some(8) && flight.at(8).is_none(), "indexed read");

    // F189 远程调试协议
    let payload = [0x01u8, 0x02, 0x03];
    let mut frame = [0u8; 16];
    let fn_len = kdbg_rdp_build(&payload, &mut frame);
    let built_ok = kdbg_rdp_valid(&frame[..fn_len]);
    let mut corrupt = [0u8; 16];
    let corrupt_n = kdbg_rdp_build(&payload, &mut corrupt);
    corrupt[corrupt_n - 1] ^= 0xFF;
    let corrupt_bad = kdbg_rdp_valid(&corrupt[..corrupt_n]);
    set.add(
        "F189 rdp frame",
        fn_len == 6 && built_ok && !corrupt_bad && kdbg_rdp_cksum(&payload) == 250,
        "cksum = negated sum",
    );
    let empty = [KDBG_RDP_MAGIC, 0, kdbg_rdp_cksum(&[])];
    let short = [KDBG_RDP_MAGIC];
    set.add(
        "F189 rdp edge frames",
        kdbg_rdp_valid(&empty) && !kdbg_rdp_valid(&short) && kdbg_rdp_frame_len(0) == 3,
        "empty ok, short rejected",
    );

    // F190 崩溃自动归档
    let mut arch = KdbgArchive::new();
    let c1 = arch.archive(KdbgCrashSig { kind: 1, pc: 0x1000 });
    let c_dup = arch.archive(KdbgCrashSig { kind: 1, pc: 0x1000 });
    let c2 = arch.archive(KdbgCrashSig { kind: 1, pc: 0x2000 });
    let c3 = arch.archive(KdbgCrashSig { kind: 2, pc: 0x1000 });
    set.add(
        "F190 archive dedup",
        c1 && !c_dup && c2 && c3 && arch.len() == 3 && arch.dup_rejects == 1,
        "sig dedup",
    );
    set.add(
        "F190 archive lookup",
        arch.contains(KdbgCrashSig { kind: 1, pc: 0x2000 }) && !arch.contains(KdbgCrashSig { kind: 9, pc: 0x9999 }),
        "contains exact",
    );

    // F191 调试降级阶梯
    let mut ladder = KdbgLadder::new();
    let e1 = ladder.escalate();
    let e2 = ladder.escalate();
    let e3 = ladder.escalate();
    let e4 = ladder.escalate();
    let e5 = ladder.escalate();
    set.add(
        "F191 ladder escalate cap",
        e1 && e2 && e3 && e4 && !e5 && ladder.level == KDBG_LADDER_TOP,
        "4 is top",
    );
    let h1 = ladder.note_healthy();
    let h2 = ladder.note_healthy();
    let level_before_heal = ladder.level;
    let h3 = ladder.note_healthy();
    set.add(
        "F191 ladder heal",
        !h1 && !h2 && level_before_heal == 4 && h3 && ladder.level == 3 && ladder.healthy_streak == 0,
        "3 healthy -> drop one",
    );

    // F192 幻影释放检测
    let mut slots = KdbgSlotTable::new();
    let idx = slots.alloc_slot();
    let stale = slots.free_slot(0, 1);
    let good_free = slots.free_slot(0, 0);
    let double_free = slots.free_slot(0, 0);
    let re = slots.alloc_slot();
    let phantom = slots.free_slot(0, 0);
    set.add(
        "F192 gen guard",
        idx == Some(0) && !stale && good_free && !double_free && re == Some(0) && !phantom,
        "stale gen rejected",
    );
    let after = slots.slot(0);
    let ok_new_gen = slots.free_slot(0, 1);
    set.add(
        "F192 gen bumps on free",
        after.gen == 1 && !after.live && ok_new_gen,
        "gen increments",
    );

    // F193 未初始化哨兵
    let mut poison_buf = [0u8; 16];
    kdbg_poison(&mut poison_buf);
    let pristine = kdbg_is_poisoned(&poison_buf);
    poison_buf[0] = 1;
    poison_buf[1] = 2;
    poison_buf[2] = 3;
    let touched = kdbg_is_poisoned(&poison_buf);
    set.add("F193 poison detect", pristine && !touched, "0xA5 destroyed by write");
    let tail3 = kdbg_tail_unwritten(&poison_buf, 3);
    let tail2 = kdbg_tail_unwritten(&poison_buf, 2);
    set.add("F193 tail guard", tail3 && !tail2, "prefix write localized");

    // F194 调试信息自检
    let mut di = KdbgDebugInfo::new();
    di.recs[0] = KdbgDiRec { offset: 0, size: 4 };
    di.recs[1] = KdbgDiRec { offset: 4, size: 8 };
    di.recs[2] = KdbgDiRec { offset: 16, size: 4 };
    di.count = 3;
    set.add("F194 debuginfo ok", kdbg_debuginfo_ok(&di), "sorted, non-overlap");
    let mut overlap = KdbgDebugInfo::new();
    overlap.recs[0] = KdbgDiRec { offset: 0, size: 4 };
    overlap.recs[1] = KdbgDiRec { offset: 2, size: 4 };
    overlap.count = 2;
    let empty_di = KdbgDebugInfo::new();
    set.add(
        "F194 debuginfo rejects",
        !kdbg_debuginfo_ok(&overlap) && !kdbg_debuginfo_ok(&empty_di),
        "overlap / empty rejected",
    );

    // F195 多核一致性视图
    let mut view = KdbgSeqView::new(7);
    let stable0 = kdbg_view_stable(view.seq, view.seq);
    view.write(9);
    let seq_after_write = view.seq;
    let value_after_write = view.value;
    set.add(
        "F195 seqlock write",
        stable0 && seq_after_write == 2 && value_after_write == 9 && kdbg_view_stable(view.seq, view.seq),
        "write bumps seq by 2",
    );
    let mid_write = 1u32;
    let moved = kdbg_view_stable(0, seq_after_write);
    set.add(
        "F195 seqlock read windows",
        !kdbg_view_stable(mid_write, mid_write) && !moved,
        "odd seq and moved seq unstable",
    );

    // F196 压测剧本库
    let mut play = KdbgPlaybook::new();
    let p1 = play.add_step(1, 3);
    let p_dup = play.add_step(1, 5);
    let p_zero = play.add_step(2, 0);
    let p2 = play.add_step(2, 5);
    set.add(
        "F196 playbook steps",
        p1 && !p_dup && !p_zero && p2 && play.len() == 2 && play.rejects == 2 && play.total_iterations() == 8,
        "dedup + bounds",
    );

    // F197 调试预算官
    let mut budget = KdbgBudget::new(10);
    let g1 = budget.spend(6);
    let rem_mid = budget.remaining();
    let g2 = budget.spend(6);
    set.add("F197 budget grant", g1 == 6 && rem_mid == 4, "partial grant");
    set.add(
        "F197 budget clamp",
        g2 == 4 && budget.remaining() == 0 && budget.overrun_permille() == 1000,
        "never overdraft",
    );

    // F198 探针命名规范
    let long25 = "aaaaaaaaaaaaaaaaaaaaaaaaa";
    set.add(
        "F198 probe names",
        kdbg_probe_name_ok("kdbg_ring") && !kdbg_probe_name_ok("KdbgRing") && !kdbg_probe_name_ok("ab")
            && kdbg_probe_name_ok("x1_y") && !kdbg_probe_name_ok("bad-name") && !kdbg_probe_name_ok(long25),
        "charset + length bounds",
    );

    // F199 调试金样本
    let golden = kdbg_golden_scenario();
    set.add("F199 golden matches", kdbg_golden_matches(&golden), "scripted end state");
    let mut drifted = kdbg_golden_scenario();
    drifted.push(99, 1);
    set.add("F199 golden catches drift", !kdbg_golden_matches(&drifted), "extra push detected");

    // F200 调试域年报
    set.add(
        "F200 kdbg report",
        KDBG_REPORT_SECTIONS.len() == 5 && kdbg_report_complete(5) && !kdbg_report_complete(4),
        "sections complete",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f176_log_spectrum() {
        assert!(kdbg_log_passes(KDBG_LOG_WARN, KDBG_LOG_ERROR));
        assert!(!kdbg_log_passes(KDBG_LOG_ERROR, KDBG_LOG_WARN));
        let mut reg = KdbgLogRegistry::new();
        assert!(reg.register(1));
        assert!(!reg.register(1));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn f177_ring_overwrite() {
        let mut r = KdbgTraceRing::new();
        let mut i = 0u64;
        while i < 18 {
            r.push(i, 1);
            i += 1;
        }
        assert_eq!(r.pushed, 18);
        assert_eq!(r.overwritten, 2);
        assert_eq!(r.seq_at(17), 17);
        assert_eq!(r.seq_at(0), 16); // 槽 0 被 seq 16 覆写
    }

    #[test]
    fn f180_bp_state_machine() {
        let mut bp = KdbgBreakpoint::new(0x1000);
        assert!(!bp.resume()); // Armed 不可恢复
        assert!(bp.trigger());
        assert!(!bp.trigger()); // Hit 不可再命中
        assert!(bp.resume());
        assert!(!bp.resume()); // Resumed 不可重复恢复
        assert!(bp.rearm());
        assert_eq!(bp.hits, 1);
    }

    #[test]
    fn f184_time_travel_eviction() {
        let mut tm = KdbgTimeMachine::new();
        let mut g = 1;
        while g <= 5 {
            tm.record(g, g as u64);
            g += 1;
        }
        assert!(tm.replay(1).is_none()); // 最早一代已被覆写
        assert!(tm.replay(5).is_some());
        assert_eq!(tm.recorded, 5);
    }

    #[test]
    fn f189_rdp_cksum_math() {
        let payload = [0xFFu8, 0x01];
        // sum = 0x00 (wrap), cksum = 0
        assert_eq!(kdbg_rdp_cksum(&payload), 0);
        assert_eq!(kdbg_rdp_cksum(&[]), 0);
        let mut out = [0u8; 8];
        let n = kdbg_rdp_build(&payload, &mut out);
        assert_eq!(n, 5);
        assert!(kdbg_rdp_valid(&out[..n]));
    }

    #[test]
    fn f192_phantom_free_caught() {
        let mut t = KdbgSlotTable::new();
        assert_eq!(t.alloc_slot(), Some(0));
        assert!(!t.free_slot(0, 9)); // 幻影代际
        assert!(t.free_slot(0, 0));
        assert!(!t.free_slot(0, 0)); // 双重释放
        assert_eq!(t.slot(0).gen, 1);
    }

    #[test]
    fn kdbg_selfcheck_all_pass() {
        let set = run_m700kdbg_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
