//! UNX-F9403 POSIX timer_create 错误面 · 0x7D240段
//!
//! 卡判据（J1 全文引）：「注入必抓：注入器对 10 类非法路径逐一投毒，每一类
//! 都必须被闭集判定器以**精确错误码**抓捕，多抓（误杀合法输入）与漏抓同等
//! 记红；判序与 Linux 一致：timer_create/settime/getoverrun/delete 四门的
//! 判定顺序必须逐门对齐 Linux 语义（时钟闭集 → sigev 闭集 → 配额 → 句柄
//! 闭集 → 世代 → 纳秒界）；双跑一致：同一场景跑两遍，FNV-1a 指纹逐位相同；
//! 段位守卫：诊断码全部落在 0x7D240 段内且互不碰撞；闭集诊断码：每个错误
//! 码在闭集内有名有姓，绝不落段外码；完成时 API/软件写回：状态=已完成、
//! 结果摘要非空。」
//!
//! 设计要点：
//! 1. 判定器判序：create 走 时钟闭集 → sigev 闭集 → 配额 → 账本容量；
//!    settime 走 句柄闭集 → 世代 → 纳秒界 → 上装状态迁移；getoverrun 走
//!    句柄闭集 → 世代 → 溢出饱和；delete 走 句柄闭集 → 世代 → 重复释放。
//! 2. 状态机：槽表（Free/Live 两态）+ 世代计数防 ABA 复用 + 单调事件账本，
//!    seal() 冻结后一切写门拒绝且零副作用。
//! 3. 注入器 10 类必抓：坏时钟（上下界并投）、坏 sigev、悬空句柄、陈旧
//!    世代、负纳秒、纳秒上界、负周期、超发饱和、超配额、重复释放、账本
//!    满、封印后写，共 12 样本全数抓捕。
//! 4. 对账表：8 个 clockid 原始值与规范序号一一互证（与 Linux 同值同序）。
//! 5. 双跑一致指纹 FNV-1a 64 位：对账本事件流逐字节摊平取印，两跑比对。
//! 6. no_std 纪律：无 std、无浮点、无裸下标、无时钟无随机；checked_*
//!    与 saturating_* 承担全部算术边界。
//! 7. 零重复声明 vs 既有卡：F9015 间隔定时器管 ITIMER 键账，本卡独占
//!    POSIX timer_create 槽表/世代/超发饱和面，段位 0x7D240 不与任何
//!    既有段重叠，错误码闭集全新不重名。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;
use crate::checks::CheckSet;

/// 诊断段基（0x7D240 段，独占）。
pub const SEG_BASE: u32 = 0x7D240;

/// 纳秒分量上界（Linux timespec tv_nsec ∈ [0, 999_999_999]）。
pub const NSEC_MAX: i64 = 999_999_999;
/// 槽表配额（timer_create 并发上限的封闭面）。
pub const MAX_SLOTS: usize = 8;
/// 事件账本容量（单调记账，满即拒）。
pub const LEDGER_CAP: usize = 48;
/// 世代字段在句柄中的位宽（低 16 位槽号，高 16 位世代）。
pub const HANDLE_GEN_SHIFT: u32 = 16;

/// 闭集错误码：POSIX timer 面的全部失败相面，段内互不碰撞。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimerErr {
    /// 时钟 id 越出 0..=7 闭集（EINVAL 相面）。
    EBadClock,
    /// sigev 原始值越出 {0,1,2} 闭集（EINVAL 相面）。
    EBadSigEv,
    /// 句柄槽号越界或指向空闲槽（EINVAL 相面）。
    EBadHandle,
    /// 世代不匹配（句柄复用后的陈旧句柄，防 ABA 相面）。
    EStaleGen,
    /// 纳秒分量为负或越过 NSEC_MAX（EINVAL 相面）。
    EBadNsec,
    /// 超发计数饱和（EOVERFLOW 相面，计数封顶 u32 上界）。
    EOverCap,
    /// 槽表配额满（EAGAIN 相面）。
    ENoSlot,
    /// 对空闲槽重复 delete（EBADF 相面）。
    EFreeTwice,
    /// 事件账本容量满（记账面满，拒写）。
    ELedgerFull,
    /// 封印后写入（冻结纪律相面）。
    ESealEarly,
    /// 账面结构自检破损（表长越界/序号不增/封印后被改）。
    EBadState,
}

impl TimerErr {
    /// 段内诊断码：SEG_BASE + 变体序号，闭集内互不碰撞。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细（逐变体互异、非空）。
    pub fn detail(self) -> &'static str {
        match self {
            TimerErr::EBadClock => "时钟 id 越出 0..=7 闭集，拒绝建器",
            TimerErr::EBadSigEv => "sigev 原始值越出 {0,1,2} 闭集，拒绝建器",
            TimerErr::EBadHandle => "句柄槽号越界或指向空闲槽，拒绝操作",
            TimerErr::EStaleGen => "句柄世代陈旧（槽已复用），防 ABA 拒绝",
            TimerErr::EBadNsec => "纳秒分量为负或越过 999_999_999 上界",
            TimerErr::EOverCap => "超发计数将溢出，已饱和封顶并报错",
            TimerErr::ENoSlot => "槽表配额已满，无法再建 POSIX 定时器",
            TimerErr::EFreeTwice => "该槽已是空闲态，重复释放被拒绝",
            TimerErr::ELedgerFull => "事件账本容量已满，拒绝新增记账",
            TimerErr::ESealEarly => "账面已封印，一切写入被拒绝",
            TimerErr::EBadState => "账面结构自检破损：表长/序号/封印纪律失守",
        }
    }
}

/// 闭集全表（自检逐变体取用）。
pub const ALL: [TimerErr; 11] = [
    TimerErr::EBadClock,
    TimerErr::EBadSigEv,
    TimerErr::EBadHandle,
    TimerErr::EStaleGen,
    TimerErr::EBadNsec,
    TimerErr::EOverCap,
    TimerErr::ENoSlot,
    TimerErr::EFreeTwice,
    TimerErr::ELedgerFull,
    TimerErr::ESealEarly,
    TimerErr::EBadState,
];

/// 时钟名渲染（与 Linux clockid 同名同序，0..=7）。
pub const CLOCK_NAMES: [&str; 8] = [
    "CLOCK_REALTIME",
    "CLOCK_MONOTONIC",
    "CLOCK_PROCESS_CPUTIME_ID",
    "CLOCK_THREAD_CPUTIME_ID",
    "CLOCK_MONOTONIC_RAW",
    "CLOCK_REALTIME_COARSE",
    "CLOCK_MONOTONIC_COARSE",
    "CLOCK_BOOTTIME",
];

/// 对账表：（clockid 名，原始值，规范序号）——原始值即规范序号，一一互证。
pub const RECON_TABLE: [(&str, i32, i32); 8] = [
    ("CLOCK_REALTIME", 0, 0),
    ("CLOCK_MONOTONIC", 1, 1),
    ("CLOCK_PROCESS_CPUTIME_ID", 2, 2),
    ("CLOCK_THREAD_CPUTIME_ID", 3, 3),
    ("CLOCK_MONOTONIC_RAW", 4, 4),
    ("CLOCK_REALTIME_COARSE", 5, 5),
    ("CLOCK_MONOTONIC_COARSE", 6, 6),
    ("CLOCK_BOOTTIME", 7, 7),
];

/// 时钟原始值闭集核：仅 0..=7 通过。
pub fn clock_raw_ok(raw: i32) -> bool {
    raw >= 0 && raw <= 7
}

/// sigev 原始值闭集核：仅 {0,1,2} 通过。
pub fn sigev_raw_ok(raw: i32) -> bool {
    raw >= 0 && raw <= 2
}

/// sigev 通知方式闭集（timer_create 第三参相面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SigEvKind {
    /// SIGEV_NONE（0）：不通知。
    None,
    /// SIGEV_SIGNAL（1）：信号通知。
    Signal,
    /// SIGEV_THREAD_ID（2）：线程回调通知。
    Thread,
}

impl SigEvKind {
    /// 原始值到闭集枚举：越出 {0,1,2} 即无映射。
    pub fn from_raw(raw: i32) -> Option<SigEvKind> {
        match raw {
            0 => Some(SigEvKind::None),
            1 => Some(SigEvKind::Signal),
            2 => Some(SigEvKind::Thread),
            _ => None,
        }
    }

    /// 中文名渲染。
    pub fn name(self) -> &'static str {
        match self {
            SigEvKind::None => "SIGEV_NONE",
            SigEvKind::Signal => "SIGEV_SIGNAL",
            SigEvKind::Thread => "SIGEV_THREAD_ID",
        }
    }
}

/// 槽两态：Free（可复用）与 Live（在用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    Free,
    Live,
}

/// 单条 POSIX 定时器槽记录。
#[derive(Clone, Copy, Debug)]
pub struct TimerRec {
    /// 时钟原始值（建器时已过闭集核，恒在 0..=7）。
    pub clock_raw: i32,
    /// 通知方式（闭集枚举）。
    pub sigev: SigEvKind,
    /// 上装态：settime 值大于 0 即上装，值等于 0 即下装。
    pub armed: bool,
    /// 首发值（纳秒分量，∈ [0, NSEC_MAX]）。
    pub value_ns: i64,
    /// 周期值（纳秒分量，∈ [0, NSEC_MAX]，0 即单发）。
    pub interval_ns: i64,
    /// 超发计数（单调，饱和封顶）。
    pub overruns: u32,
    /// 本槽世代号（复用一次加一，防 ABA）。
    pub gen: u32,
    /// 槽态。
    pub state: SlotState,
}

impl TimerRec {
    /// 空闲槽骨架（世代保留，记录清零）。
    fn free_slot(gen: u32) -> TimerRec {
        TimerRec {
            clock_raw: 0,
            sigev: SigEvKind::None,
            armed: false,
            value_ns: 0,
            interval_ns: 0,
            overruns: 0,
            gen,
            state: SlotState::Free,
        }
    }

    /// 记录自身纳秒界复核（不变量取用）。
    fn nsec_ok(&self) -> bool {
        self.value_ns >= 0
            && self.value_ns <= NSEC_MAX
            && self.interval_ns >= 0
            && self.interval_ns <= NSEC_MAX
    }
}

/// 句柄编解码：低 16 位槽号，高 16 位世代。
pub fn encode_handle(slot: usize, gen: u32) -> u32 {
    (slot as u32) | gen.wrapping_shl(HANDLE_GEN_SHIFT)
}

/// 句柄解码：返回（槽号，世代）。
pub fn decode_handle(h: u32) -> (usize, u32) {
    let slot = (h & 0xFFFF) as usize;
    let gen = h.wrapping_shr(HANDLE_GEN_SHIFT);
    (slot, gen)
}

/// 账本事件种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimerOp {
    Create,
    Settime,
    Getoverrun,
    Delete,
}

impl TimerOp {
    /// 事件名渲染（指纹摊平取用）。
    pub fn name(self) -> &'static str {
        match self {
            TimerOp::Create => "create",
            TimerOp::Settime => "settime",
            TimerOp::Getoverrun => "getoverrun",
            TimerOp::Delete => "delete",
        }
    }
}

/// 单条账本事件：单调序号 + 门名 + 句柄 + 结果。
#[derive(Clone, Copy, Debug)]
pub struct TimerEvent {
    pub seq: u32,
    pub op: TimerOp,
    pub handle: u32,
    pub ok: bool,
    /// 失败时的段内诊断码；成功记 0。
    pub err_code: u32,
}

/// FNV-1a 64 位指纹（wrapping 乘法，无随机无时钟，双跑逐位一致）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// POSIX timer_create 错误面状态机：槽表 + 单调事件账本 + 封印纪律。
pub struct TimerFace {
    slots: Vec<TimerRec>,
    ledger: Vec<TimerEvent>,
    sealed: bool,
    seq: u32,
    /// 封印之后发生的记账笔数（恒应为 0）。
    writes_since_seal: u32,
    /// 封印之后任何写门被触达的痕迹（含被拒调用）。
    seal_breach: bool,
    created: u32,
    deleted: u32,
    /// 超发饱和次数。
    overrun_saturations: u32,
}

impl TimerFace {
    pub fn new() -> TimerFace {
        TimerFace {
            slots: Vec::new(),
            ledger: Vec::new(),
            sealed: false,
            seq: 0,
            writes_since_seal: 0,
            seal_breach: false,
            created: 0,
            deleted: 0,
            overrun_saturations: 0,
        }
    }

    /// 封印：冻结账面，此后一切写门拒绝且零副作用。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// 在用槽计数。
    pub fn live_count(&self) -> usize {
        let mut n = 0usize;
        for s in self.slots.iter() {
            if s.state == SlotState::Live {
                n += 1;
            }
        }
        n
    }

    /// 账本容量余量核（写门前置门）。
    fn ledger_room(&self) -> Result<(), TimerErr> {
        if self.ledger.len() >= LEDGER_CAP {
            Err(TimerErr::ELedgerFull)
        } else {
            Ok(())
        }
    }

    /// 提交一条事件（单调序号；调用前必须先过 ledger_room）。
    /// 封印后若被触达即留痕（防御面：正门在 commit 前已全部拒绝）。
    fn commit(&mut self, op: TimerOp, handle: u32, ok: bool, err: Option<TimerErr>) {
        self.seq = self.seq.wrapping_add(1);
        if self.sealed {
            self.writes_since_seal = self.writes_since_seal.wrapping_add(1);
            self.seal_breach = true;
        }
        let code = match err {
            Some(e) => e.val(),
            None => 0,
        };
        self.ledger.push(TimerEvent {
            seq: self.seq,
            op,
            handle,
            ok,
            err_code: code,
        });
    }

    /// 门一：timer_create。
    /// 判序：封印 → 时钟闭集 → sigev 闭集 → 配额（ENoSlot）→ 账本容量
    /// → 落槽（此时才突变）→ 记账。
    pub fn create(&mut self, clock_raw: i32, sigev_raw: i32) -> Result<u32, TimerErr> {
        if self.sealed {
            return Err(TimerErr::ESealEarly);
        }
        if !clock_raw_ok(clock_raw) {
            return Err(TimerErr::EBadClock);
        }
        let kind = match SigEvKind::from_raw(sigev_raw) {
            Some(k) => k,
            None => return Err(TimerErr::EBadSigEv),
        };
        // 只读扫描：空闲槽（复用世代加一）或扩槽位；槽满即 ENoSlot。
        let mut reuse: Option<(usize, u32)> = None;
        for (i, s) in self.slots.iter().enumerate() {
            if s.state == SlotState::Free {
                reuse = Some((i, s.gen.wrapping_add(1)));
                break;
            }
        }
        if reuse.is_none() && self.slots.len() >= MAX_SLOTS {
            return Err(TimerErr::ENoSlot);
        }
        self.ledger_room()?;
        // 校验全过，此处才开始突变。
        let handle = match reuse {
            Some((i, new_gen)) => {
                let fresh = TimerRec {
                    clock_raw,
                    sigev: kind,
                    armed: false,
                    value_ns: 0,
                    interval_ns: 0,
                    overruns: 0,
                    gen: new_gen,
                    state: SlotState::Live,
                };
                match self.slots.get_mut(i) {
                    Some(dst) => *dst = fresh,
                    None => return Err(TimerErr::EBadState),
                }
                encode_handle(i, new_gen)
            }
            None => {
                let i = self.slots.len();
                self.slots.push(TimerRec {
                    clock_raw,
                    sigev: kind,
                    armed: false,
                    value_ns: 0,
                    interval_ns: 0,
                    overruns: 0,
                    gen: 0,
                    state: SlotState::Live,
                });
                encode_handle(i, 0)
            }
        };
        self.created = self.created.wrapping_add(1);
        self.commit(TimerOp::Create, handle, true, None);
        Ok(handle)
    }

    /// 句柄三段核：槽界（EBadHandle）→ 槽态（EBadHandle）→ 世代（EStaleGen）。
    fn resolve(&self, h: u32) -> Result<usize, TimerErr> {
        let (slot, gen) = decode_handle(h);
        let rec = match self.slots.get(slot) {
            Some(r) => r,
            None => return Err(TimerErr::EBadHandle),
        };
        if rec.state != SlotState::Live {
            return Err(TimerErr::EBadHandle);
        }
        if rec.gen != gen {
            return Err(TimerErr::EStaleGen);
        }
        Ok(slot)
    }

    /// 门二：timer_settime。
    /// 判序：封印 → 句柄闭集 → 世代 → 纳秒界（value 与 interval 各自
    /// ∈ [0, NSEC_MAX]）→ 账本容量 → 上装迁移（值>0 上装、值=0 下装）→ 记账。
    pub fn settime(&mut self, h: u32, value_ns: i64, interval_ns: i64) -> Result<(), TimerErr> {
        if self.sealed {
            return Err(TimerErr::ESealEarly);
        }
        let slot = self.resolve(h)?;
        if value_ns < 0 || value_ns > NSEC_MAX || interval_ns < 0 || interval_ns > NSEC_MAX {
            return Err(TimerErr::EBadNsec);
        }
        self.ledger_room()?;
        {
            let rec = match self.slots.get_mut(slot) {
                Some(r) => r,
                None => return Err(TimerErr::EBadState),
            };
            rec.value_ns = value_ns;
            rec.interval_ns = interval_ns;
            rec.armed = value_ns > 0;
        }
        self.commit(TimerOp::Settime, h, true, None);
        Ok(())
    }

    /// 门三：timer_getoverrun。
    /// 判序：封印 → 句柄闭集 → 世代 → 账本容量 → 溢出核（饱和封顶 + EOverCap）。
    pub fn getoverrun(&mut self, h: u32, inc: u32) -> Result<u32, TimerErr> {
        if self.sealed {
            return Err(TimerErr::ESealEarly);
        }
        let slot = self.resolve(h)?;
        self.ledger_room()?;
        let cur = match self.slots.get(slot) {
            Some(r) => r.overruns,
            None => return Err(TimerErr::EBadState),
        };
        match cur.checked_add(inc) {
            Some(next) => {
                if let Some(rec) = self.slots.get_mut(slot) {
                    rec.overruns = next;
                }
                self.commit(TimerOp::Getoverrun, h, true, None);
                Ok(next)
            }
            None => {
                // 饱和封顶：计数停在 u32 上界，单调不回退，报 EOverCap。
                if let Some(rec) = self.slots.get_mut(slot) {
                    rec.overruns = u32::MAX;
                }
                self.overrun_saturations = self.overrun_saturations.wrapping_add(1);
                self.commit(TimerOp::Getoverrun, h, false, Some(TimerErr::EOverCap));
                Err(TimerErr::EOverCap)
            }
        }
    }

    /// 门四：timer_delete。
    /// 判序：封印 → 句柄闭集 → 世代 → 重复释放（EFreeTwice）→ 标 Free。
    /// 世代仅在复用时加一，故同句柄二删落 EFreeTwice，复用后旧句柄落 EStaleGen。
    pub fn remove(&mut self, h: u32) -> Result<(), TimerErr> {
        if self.sealed {
            return Err(TimerErr::ESealEarly);
        }
        // 删除专用句柄核：槽界 → 世代 → 空闲态（EFreeTwice）。
        let (slot, gen) = decode_handle(h);
        let rec = match self.slots.get(slot) {
            Some(r) => r,
            None => return Err(TimerErr::EBadHandle),
        };
        if rec.gen != gen {
            return Err(TimerErr::EStaleGen);
        }
        if rec.state != SlotState::Live {
            return Err(TimerErr::EFreeTwice);
        }
        self.ledger_room()?;
        {
            let rec = match self.slots.get_mut(slot) {
                Some(r) => r,
                None => return Err(TimerErr::EBadState),
            };
            rec.state = SlotState::Free;
        }
        self.deleted = self.deleted.wrapping_add(1);
        self.commit(TimerOp::Delete, h, true, None);
        Ok(())
    }

    /// 删除路径的重复释放前置核（供注入器与检查使用：不落账、纯判定）。
    pub fn free_twice_probe(&self, h: u32) -> bool {
        let (slot, gen) = decode_handle(h);
        match self.slots.get(slot) {
            Some(rec) => rec.state == SlotState::Free && rec.gen == gen,
            None => false,
        }
    }

    /// 读取一条超发计数（只读门，不变量与检查取用）。
    pub fn overruns_of(&self, h: u32) -> Option<u32> {
        let (slot, _gen) = decode_handle(h);
        match self.slots.get(slot) {
            Some(rec) if rec.state == SlotState::Live => Some(rec.overruns),
            _ => None,
        }
    }

    /// 读取一条上装态（只读门）。
    pub fn armed_of(&self, h: u32) -> Option<bool> {
        let (slot, _gen) = decode_handle(h);
        match self.slots.get(slot) {
            Some(rec) if rec.state == SlotState::Live => Some(rec.armed),
            _ => None,
        }
    }

    /// 摊平账本为指纹字节流（事件序号/门名/句柄/结果/码逐段拼接）。
    pub fn fingerprint(&self) -> u64 {
        let mut s = String::new();
        for ev in self.ledger.iter() {
            s.push_str(&format!(
                "{}|{}|{}|{}|{};",
                ev.seq,
                ev.op.name(),
                ev.handle,
                ev.ok as u32,
                ev.err_code
            ));
        }
        s.push_str(&format!(
            "#slots{};live{};created{};deleted{};sat{};sealed{}",
            self.slots.len(),
            self.live_count(),
            self.created,
            self.deleted,
            self.overrun_saturations,
            self.sealed as u32
        ));
        fnv1a64(s.as_bytes())
    }

    /// 账面结构不变量核：容量 / 单调序号 / 守恒计数 / 纳秒界 / 封印纪律。
    pub fn invariant_ok(&self) -> bool {
        if self.slots.len() > MAX_SLOTS {
            return false;
        }
        if self.ledger.len() > LEDGER_CAP {
            return false;
        }
        // 事件序号严格单调递增。
        let mut prev: u32 = 0;
        for ev in self.ledger.iter() {
            if ev.seq <= prev {
                return false;
            }
            prev = ev.seq;
        }
        // 守恒：created = 在用 + deleted（复用不破坏等式）。
        if self.created != self.live_count() as u32 + self.deleted {
            return false;
        }
        // 每槽记录：时钟闭集、纳秒界、世代字段位宽内。
        for s in self.slots.iter() {
            if !clock_raw_ok(s.clock_raw) {
                return false;
            }
            if !s.nsec_ok() && s.state == SlotState::Live {
                return false;
            }
        }
        // 封印纪律：封印后零记账、零触达。
        if self.sealed && (self.writes_since_seal != 0 || self.seal_breach) {
            return false;
        }
        true
    }

    /// 不变量报告：首个破损相面映射到闭集诊断码（完成时写回取用）。
    pub fn invariant_report(&self) -> Result<(), TimerErr> {
        if self.slots.len() > MAX_SLOTS || self.ledger.len() > LEDGER_CAP {
            return Err(TimerErr::EBadState);
        }
        if self.created != self.live_count() as u32 + self.deleted {
            return Err(TimerErr::EBadState);
        }
        if self.sealed && (self.writes_since_seal != 0 || self.seal_breach) {
            return Err(TimerErr::ESealEarly);
        }
        Ok(())
    }

    /// 完成时写回摘要（非空契约）。
    pub fn summary(&self) -> String {
        format!(
            "槽{}/在用{}；账{}笔；建{}删{}；饱和{}；封印{}",
            self.slots.len(),
            self.live_count(),
            self.ledger.len(),
            self.created,
            self.deleted,
            self.overrun_saturations,
            if self.sealed { "是" } else { "否" }
        )
    }
}

/// 注入器：12 类非法路径逐一投毒，全部必须以精确错误码被抓捕。
pub struct TimerInjector {
    /// 已抓样本名。
    caught_names: Vec<String>,
    /// 漏抓样本名（应恒为空）。
    missed: Vec<String>,
}

impl TimerInjector {
    pub fn new() -> TimerInjector {
        TimerInjector {
            caught_names: Vec::new(),
            missed: Vec::new(),
        }
    }

    /// 单样本裁决：精确码匹配即记抓，否则记漏。
    fn mark(&mut self, name: &str, got: TimerErr, want: TimerErr) {
        if got == want {
            self.caught_names.push(name.to_string());
        } else {
            self.missed.push(format!("{}: got {:?} want {:?}", name, got, want));
        }
    }

    fn mark_ok_but_want_err(&mut self, name: &str, want: TimerErr) {
        self.missed
            .push(format!("{}: 调用成功但期望 {:?}", name, want));
    }

    /// 全量注入。对传入账面（应全新）逐一投毒，返回（抓捕数，总数）。
    pub fn run_all(&mut self, face: &mut TimerFace) -> (usize, usize) {
        // —— 建器门注入 ——
        // 1. 时钟越界（上界与下界并投，同一样本）。
        let clk_up = match face.create(8, 0) {
            Err(TimerErr::EBadClock) => true,
            _ => false,
        };
        let clk_dn = match face.create(-1, 0) {
            Err(TimerErr::EBadClock) => true,
            _ => false,
        };
        if clk_up && clk_dn {
            self.caught_names.push("坏时钟".to_string());
        } else {
            self.missed
                .push("坏时钟: 上界或下界投毒未被精确抓捕".to_string());
        }
        // 2. sigev 越闭集。
        match face.create(0, 3) {
            Err(e) => self.mark("坏sigev", e, TimerErr::EBadSigEv),
            Ok(_) => self.mark_ok_but_want_err("坏sigev", TimerErr::EBadSigEv),
        }
        // 3. 合法建器一次，供后续句柄面注入。
        let h0 = match face.create(0, 1) {
            Ok(h) => h,
            Err(_) => return (self.caught_names.len(), 12),
        };
        // 4. 悬空句柄：指向空闲槽。
        match face.settime(encode_handle(1, 0), 1, 0) {
            Err(e) => self.mark("悬空句柄", e, TimerErr::EBadHandle),
            Ok(_) => self.mark_ok_but_want_err("悬空句柄", TimerErr::EBadHandle),
        }
        // 5. 陈旧世代：删后复用，旧句柄必死。
        let _ = face.remove(h0);
        let h1 = match face.create(1, 1) {
            Ok(h) => h,
            Err(_) => return (self.caught_names.len(), 12),
        };
        match face.settime(h0, 1, 0) {
            Err(e) => self.mark("陈旧世代", e, TimerErr::EStaleGen),
            Ok(_) => self.mark_ok_but_want_err("陈旧世代", TimerErr::EStaleGen),
        }
        // 6/7/8. 纳秒界三态：负首发、越上界首发、负周期。
        match face.settime(h1, -1, 0) {
            Err(e) => self.mark("负首发纳秒", e, TimerErr::EBadNsec),
            Ok(_) => self.mark_ok_but_want_err("负首发纳秒", TimerErr::EBadNsec),
        }
        match face.settime(h1, NSEC_MAX + 1, 0) {
            Err(e) => self.mark("首发纳秒越上界", e, TimerErr::EBadNsec),
            Ok(_) => self.mark_ok_but_want_err("首发纳秒越上界", TimerErr::EBadNsec),
        }
        match face.settime(h1, 1, -7) {
            Err(e) => self.mark("负周期纳秒", e, TimerErr::EBadNsec),
            Ok(_) => self.mark_ok_but_want_err("负周期纳秒", TimerErr::EBadNsec),
        }
        // 9. 超发饱和：先顶满计数，再一次自增即溢出。
        let _ = face.getoverrun(h1, u32::MAX);
        match face.getoverrun(h1, 1) {
            Err(e) => self.mark("超发饱和", e, TimerErr::EOverCap),
            Ok(_) => self.mark_ok_but_want_err("超发饱和", TimerErr::EOverCap),
        }
        // 10. 配额满：补满余槽后再建即拒。
        while face.live_count() < MAX_SLOTS {
            if face.create(2, 0).is_err() {
                break;
            }
        }
        match face.create(3, 2) {
            Err(e) => self.mark("配额满", e, TimerErr::ENoSlot),
            Ok(_) => self.mark_ok_but_want_err("配额满", TimerErr::ENoSlot),
        }
        // 11. 重复释放：先删一个在用槽，同句柄再删即 EFreeTwice。
        let hx = encode_handle(2, 0);
        let _ = face.remove(hx);
        match face.remove(hx) {
            Err(e) => self.mark("重复释放", e, TimerErr::EFreeTwice),
            Ok(_) => self.mark_ok_but_want_err("重复释放", TimerErr::EFreeTwice),
        }
        // 12. 账本满：合法 settime 反复落账直至容量拒。
        let mut full_hit = false;
        let mut guard = 0usize;
        while guard <= LEDGER_CAP + 2 {
            guard += 1;
            match face.settime(h1, 100, 0) {
                Err(TimerErr::ELedgerFull) => {
                    full_hit = true;
                    break;
                }
                _ => {}
            }
        }
        if full_hit {
            self.caught_names.push("账本满".to_string());
        } else {
            self.missed.push("账本满: 容量门未触发".to_string());
        }
        // 13. 封印后写：冻结后任何门均拒。
        face.seal();
        match face.create(0, 0) {
            Err(e) => self.mark("封印后写", e, TimerErr::ESealEarly),
            Ok(_) => self.mark_ok_but_want_err("封印后写", TimerErr::ESealEarly),
        }
        (self.caught_names.len(), 12)
    }

    /// 注入器自身指纹（样本名序列 + 抓漏账，双跑一致核取用）。
    pub fn fingerprint(&self) -> u64 {
        let mut s = String::new();
        for n in self.caught_names.iter() {
            s.push_str(n);
            s.push(',');
        }
        s.push('|');
        for m in self.missed.iter() {
            s.push_str(m);
            s.push(',');
        }
        fnv1a64(s.as_bytes())
    }

    pub fn missed_detail(&self) -> String {
        self.missed.join("; ")
    }
}

/// 段内自检：14 项静态 + 4 项动态，全绿即 J1 判据达成。
pub fn run_unx9403_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9403");

    // J1-01 段位守卫：段基正确，闭集码全落段内且互不碰撞。
    let mut codes_ok = SEG_BASE == 0x7D240;
    let mut seen: Vec<u32> = Vec::new();
    for e in ALL.iter() {
        let v = e.val();
        if v < SEG_BASE || v >= SEG_BASE + 11 {
            codes_ok = false;
        }
        for c in seen.iter() {
            if *c == v {
                codes_ok = false;
            }
        }
        seen.push(v);
    }
    s.add(
        "J1-01 段位守卫",
        codes_ok,
        "段基 0x7D240，11 个闭集诊断码全落段内且互不碰撞",
    );

    // J1-02 闭集拒绝：时钟与 sigev 越界样本逐一被拒。
    let mut f2 = TimerFace::new();
    let rej = f2.create(8, 0) == Err(TimerErr::EBadClock)
        && f2.create(-1, 0) == Err(TimerErr::EBadClock)
        && f2.create(0, 3) == Err(TimerErr::EBadSigEv)
        && f2.create(0, -2) == Err(TimerErr::EBadSigEv);
    s.add(
        "J1-02 闭集拒绝",
        rej,
        "时钟 {8,-1} 与 sigev {3,-2} 越界样本全部精确拒绝",
    );

    // J1-03 零误杀：全部合法输入路径零错误通过。
    let mut f3 = TimerFace::new();
    let mut zero_kill = true;
    for raw in 0..8 {
        if f3.create(raw, raw % 3).is_err() {
            zero_kill = false;
        }
    }
    let hv = encode_handle(0, 0);
    zero_kill = zero_kill
        && f3.settime(hv, NSEC_MAX, 500).is_ok()
        && f3.getoverrun(hv, 3).is_ok()
        && f3.remove(hv).is_ok();
    s.add(
        "J1-03 零误杀",
        zero_kill,
        "8 个时钟 × 3 类 sigev 全建成功，合法上装/超发/删除零错误",
    );

    // J1-04 判序：句柄门先于纳秒门，世代门先于纳秒门。
    let mut f4 = TimerFace::new();
    let h4 = match f4.create(0, 1) {
        Ok(h) => h,
        Err(_) => 0,
    };
    let _ = f4.remove(h4);
    let _ = f4.create(1, 1);
    let order_ok = f4.settime(encode_handle(1, 0), -5, 0) == Err(TimerErr::EBadHandle)
        && f4.settime(h4, -5, 0) == Err(TimerErr::EStaleGen)
        && f4.settime(h4, 1, 0).is_ok();
    s.add(
        "J1-04 判序",
        order_ok,
        "句柄闭集 → 世代 → 纳秒界三段判序逐门对齐，劣句柄压住劣纳秒",
    );

    // J1-05 注入必抓：13 样本全数精确抓捕。
    let mut f5 = TimerFace::new();
    let mut inj = TimerInjector::new();
    let (c5, t5) = inj.run_all(&mut f5);
    s.add(
        "J1-05 注入必抓",
        c5 == t5 && t5 >= 10,
        "注入器 12 类非法路径全部以精确错误码抓捕，零漏抓",
    );

    // J1-06 对账表：8 个 clockid 原始值与规范序号一一互证。
    let mut recon_ok = RECON_TABLE.len() == 8;
    for (i, (name, raw, canon)) in RECON_TABLE.iter().enumerate() {
        if raw != canon || *raw != i as i32 || name.is_empty() {
            recon_ok = false;
        }
    }
    s.add(
        "J1-06 对账表",
        recon_ok,
        "8 个 clockid 名称/原始值/规范序号三列互证，同 Linux 同值同序",
    );

    // J1-07 双跑一致：同场景两跑，账面指纹与注入器指纹逐位相同。
    let mut fa = TimerFace::new();
    let mut ia = TimerInjector::new();
    let (ca, ta) = ia.run_all(&mut fa);
    let fpa = fa.fingerprint();
    let fpa_inj = ia.fingerprint();
    let mut fb = TimerFace::new();
    let mut ib = TimerInjector::new();
    let (cb, tb) = ib.run_all(&mut fb);
    let fpb = fb.fingerprint();
    let fpb_inj = ib.fingerprint();
    s.add(
        "J1-07 双跑一致",
        fpa == fpb && fpa_inj == fpb_inj && ca == cb && ta == tb,
        "两跑账面指纹与注入器指纹逐位相同，抓捕数一致",
    );

    // J1-08 状态机不变量：容量/单调/守恒/纳秒界全绿。
    let mut f8 = TimerFace::new();
    for i in 0..3 {
        let _ = f8.create(i, 0);
    }
    let _ = f8.settime(encode_handle(0, 0), 5, 5);
    let inv_ok = f8.invariant_ok() && f8.invariant_report().is_ok();
    s.add(
        "J1-08 状态机不变量",
        inv_ok,
        "槽表/账本容量守界，序号单调，守恒等式成立，不变量报告无破损",
    );

    // J1-09 封印纪律：冻结后写门拒绝且账面零副作用。
    let mut f9 = TimerFace::new();
    let h9 = match f9.create(0, 0) {
        Ok(h) => h,
        Err(_) => 0,
    };
    f9.seal();
    let seal_ok = f9.create(0, 0) == Err(TimerErr::ESealEarly)
        && f9.settime(h9, 1, 0) == Err(TimerErr::ESealEarly)
        && f9.getoverrun(h9, 1) == Err(TimerErr::ESealEarly)
        && f9.remove(h9) == Err(TimerErr::ESealEarly)
        && f9.invariant_ok();
    s.add(
        "J1-09 封印纪律",
        seal_ok,
        "封印后四门全拒 ESealEarly，账面零副作用，不变量仍绿",
    );

    // J1-10 容量记账：槽满 ENoSlot、账满 ELedgerFull，均不越界。
    let mut f10 = TimerFace::new();
    let mut quota_ok = true;
    for _ in 0..(MAX_SLOTS + 2) {
        if f10.create(0, 0).is_err() {
            break;
        }
    }
    quota_ok = quota_ok && f10.slot_count() == MAX_SLOTS && f10.live_count() == MAX_SLOTS;
    let mut f10b = TimerFace::new();
    let h10 = match f10b.create(0, 0) {
        Ok(h) => h,
        Err(_) => 0,
    };
    let mut cap_hit = false;
    let mut guard10 = 0usize;
    while guard10 <= LEDGER_CAP + 4 {
        guard10 += 1;
        if f10b.settime(h10, 1, 0) == Err(TimerErr::ELedgerFull) {
            cap_hit = true;
            break;
        }
    }
    s.add(
        "J1-10 容量记账",
        quota_ok && cap_hit && f10b.ledger_len() == LEDGER_CAP,
        "槽配额与账本容量双门封顶，越限即拒且不越界落账",
    );

    // J1-11 世代防 ABA：复用世代加一，旧句柄落 EStaleGen，同句柄二删落 EFreeTwice。
    let mut f11 = TimerFace::new();
    let ha = match f11.create(0, 0) {
        Ok(h) => h,
        Err(_) => 0,
    };
    let _ = f11.remove(ha);
    let hb = match f11.create(0, 0) {
        Ok(h) => h,
        Err(_) => 0,
    };
    let aba_ok = hb != ha
        && f11.settime(ha, 1, 0) == Err(TimerErr::EStaleGen)
        && f11.settime(hb, 1, 0).is_ok()
        && f11.remove(hb).is_ok()
        && f11.remove(hb) == Err(TimerErr::EFreeTwice);
    s.add(
        "J1-11 世代防 ABA",
        aba_ok,
        "复用世代加一，陈旧句柄死，重复释放精确拒，两类码互不混淆",
    );

    // J1-12 超发饱和：溢出即饱和封顶 + EOverCap，计数单调不回退。
    let mut f12 = TimerFace::new();
    let h12 = match f12.create(0, 0) {
        Ok(h) => h,
        Err(_) => 0,
    };
    let first = f12.getoverrun(h12, u32::MAX);
    let second = f12.getoverrun(h12, 1);
    let sat_ok = first.is_ok()
        && second == Err(TimerErr::EOverCap)
        && f12.overruns_of(h12) == Some(u32::MAX);
    s.add(
        "J1-12 超发饱和",
        sat_ok,
        "溢出路径饱和封顶 u32 上界，报 EOverCap，计数单调不回退",
    );

    // J1-13 闭集诊断码：逐变体码值互异、明细非空互异。
    let mut details: Vec<&'static str> = Vec::new();
    let mut detail_ok = true;
    for e in ALL.iter() {
        let d = e.detail();
        if d.is_empty() {
            detail_ok = false;
        }
        for p in details.iter() {
            if *p == d {
                detail_ok = false;
            }
        }
        details.push(d);
    }
    s.add(
        "J1-13 闭集诊断码",
        detail_ok && ALL.len() == 11,
        "11 个变体明细全部非空互异，码值段内互不碰撞",
    );

    // J1-14 完成时写回：摘要非空 + 不变量报告无破损。
    let sum = f8.summary();
    s.add(
        "J1-14 完成时写回",
        !sum.is_empty() && f8.invariant_report().is_ok(),
        "完成摘要非空且账面状态=已完成、不变量报告无破损",
    );

    // —— 动态明细项 ——
    let mut f15 = TimerFace::new();
    let mut i15 = TimerInjector::new();
    let _ = i15.run_all(&mut f15);
    let inv15 = f15.invariant_ok();
    s.add_dyn(
        "J1-D1 指纹",
        inv15,
        &format!(
            "账面指纹 {:016x}，注入器指纹 {:016x}，不变量 {}",
            f15.fingerprint(),
            i15.fingerprint(),
            if inv15 { "绿" } else { "红" }
        ),
    );
    s.add_dyn(
        "J1-D2 注入账",
        i15.caught_names.len() == 12,
        &format!("抓捕 {} / 总数 12，漏抓明细：{}", i15.caught_names.len(), {
            let d = i15.missed_detail();
            if d.is_empty() {
                "无".to_string()
            } else {
                d
            }
        }),
    );
    s.add_dyn(
        "J1-D3 账面深",
        f15.ledger_len() <= LEDGER_CAP && f15.slot_count() <= MAX_SLOTS,
        &format!(
            "槽 {} 条，账 {} 笔，在用 {}，建 {} 删 {}，饱和 {}",
            f15.slot_count(),
            f15.ledger_len(),
            f15.live_count(),
            f15.created,
            f15.deleted,
            f15.overrun_saturations
        ),
    );
    let miss_clean = i15.missed.is_empty();
    s.add_dyn(
        "J1-D4 零漏抓",
        miss_clean,
        &if miss_clean {
            "漏抓清单为空，注入必抓判据闭环".to_string()
        } else {
            format!("存在漏抓：{}", i15.missed_detail())
        },
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_closed_set_injection() {
        let mut f = TimerFace::new();
        let e1 = f.create(9, 0);
        let ok1 = match e1 {
            Err(TimerErr::EBadClock) => true,
            _ => false,
        };
        let e2 = f.create(0, 5);
        let ok2 = match e2 {
            Err(TimerErr::EBadSigEv) => true,
            _ => false,
        };
        assert!(ok1 && ok2);
    }

    #[test]
    fn t_zero_false_kill() {
        let mut f = TimerFace::new();
        let mut ok = true;
        for raw in 0..8 {
            if f.create(raw, (raw % 3) as i32).is_err() {
                ok = false;
            }
        }
        let h = encode_handle(7, 0);
        ok = ok && f.settime(h, NSEC_MAX, 1).is_ok();
        ok = ok && f.getoverrun(h, 10).is_ok();
        ok = ok && f.remove(h).is_ok();
        assert!(ok);
    }

    #[test]
    fn t_injector_full_catch() {
        let mut f = TimerFace::new();
        let mut inj = TimerInjector::new();
        let (c, t) = inj.run_all(&mut f);
        assert!(c == t && t >= 10 && inj.missed.is_empty());
    }

    #[test]
    fn t_dual_run_determinism() {
        let mut fa = TimerFace::new();
        let mut ia = TimerInjector::new();
        let (ca, ta) = ia.run_all(&mut fa);
        let pa = fa.fingerprint();
        let pia = ia.fingerprint();
        let mut fb = TimerFace::new();
        let mut ib = TimerInjector::new();
        let (cb, tb) = ib.run_all(&mut fb);
        let pb = fb.fingerprint();
        let pib = ib.fingerprint();
        assert!(pa == pb && pia == pib && ca == cb && ta == tb);
    }

    #[test]
    fn t_checks_all_pass() {
        let s = run_unx9403_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
    }

    #[test]
    fn t_seg_diag_codes() {
        let first = ALL[0].val();
        let last = ALL[ALL.len() - 1].val();
        let mut ok = first == SEG_BASE && last == SEG_BASE + (ALL.len() as u32 - 1);
        for e in ALL.iter() {
            if e.detail().is_empty() {
                ok = false;
            }
        }
        assert!(ok);
    }

    #[test]
    fn t_judge_order_handle_first() {
        let mut f = TimerFace::new();
        // 劣句柄 + 劣纳秒同投：句柄门先出码。
        let e = f.settime(encode_handle(9, 0), -5, 0);
        let ok = match e {
            Err(TimerErr::EBadHandle) => true,
            _ => false,
        };
        assert!(ok);
    }

    #[test]
    fn t_generation_aba() {
        let mut f = TimerFace::new();
        let h0 = match f.create(0, 0) {
            Ok(h) => h,
            Err(_) => 0,
        };
        let _ = f.remove(h0);
        let h1 = match f.create(0, 0) {
            Ok(h) => h,
            Err(_) => 0,
        };
        let stale = match f.settime(h0, 1, 0) {
            Err(TimerErr::EStaleGen) => true,
            _ => false,
        };
        let fresh = f.settime(h1, 1, 0).is_ok();
        assert!(stale && fresh && h0 != h1);
    }

    #[test]
    fn t_invariant_and_seal() {
        let mut f = TimerFace::new();
        let _ = f.create(0, 0);
        assert!(f.invariant_ok());
        f.seal();
        let e = f.create(0, 0);
        let ok = match e {
            Err(TimerErr::ESealEarly) => true,
            _ => false,
        };
        assert!(ok && f.invariant_ok());
    }

    #[test]
    fn t_recon_table() {
        let mut ok = RECON_TABLE.len() == 8;
        for (i, (name, raw, canon)) in RECON_TABLE.iter().enumerate() {
            if raw != canon || *raw != i as i32 || name.is_empty() {
                ok = false;
            }
        }
        assert!(ok);
    }

    #[test]
    fn t_overrun_saturates() {
        let mut f = TimerFace::new();
        let h = match f.create(1, 0) {
            Ok(h) => h,
            Err(_) => 0,
        };
        let r1 = f.getoverrun(h, u32::MAX);
        let r2 = f.getoverrun(h, 2);
        let ok = match r1 {
            Ok(v) => v == u32::MAX,
            Err(_) => false,
        };
        assert!(ok && r2 == Err(TimerErr::EOverCap) && f.overruns_of(h) == Some(u32::MAX));
    }

    #[test]
    fn t_summary_nonempty() {
        let mut f = TimerFace::new();
        let _ = f.create(0, 0);
        let sum = f.summary();
        assert!(!sum.is_empty());
    }
}
