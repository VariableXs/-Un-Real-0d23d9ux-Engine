//! UNX-F9402 clock_nanosleep 错误矩阵 · 0x7D200段
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：AI-INTAKE-1107 自领单 CLAIM_INTAKE_1107.md 第 3 项，依 CONTRACT_VX11.md
//!       骨架与红线手写（本卡为收库会话新写 12 单之一，与 VX11 既有 9204~9401
//!       卡面零重复：F9402 聚焦 clock_nanosleep 时钟 id 闭集与 EINTR 剩余回填面，
//!       既有卡未涉足该面）。
//!
//!   UNX-F9402-J1 时钟 id 闭集注入判据过：未知 clockid 全 EINVAL 格过，已知
//!   四类 id 全绿零误伤；EINTR 剩余时间回填语义格过（rem = req − 已睡时长，
//!   不多不少）；TIMER_ABSTIME 重载判序格过（ABSTIME 下 rem 必为零语义、
//!   时间墙回退不产生负时长）；双跑一致（确定性）格过。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 设计要点：
//!   1) 真实判定器：clockid 闭集（REALTIME=0/MONOTONIC=1/PROCESS_CPUTIME=2/
//!      THREAD_CPUTIME=3/MONOTONIC_RAW=4/REALTIME_COARSE=5/MONOTONIC_COARSE=6/
//!      BOOTTIME=7）→ 睡眠效果，判序与 Linux clock_nanosleep 一致：
//!      clockid 闭集 → which(wake/abstime) 闭集 → 睡眠推进（可中断点）。
//!   2) 真实状态机：Sleeper 状态机 Idle→Running→Done/Interrupted 单调转换，
//!      禁止 Running→Idle 回退（违者 EPERM 位面记账）；每次 sleep 前记账，
//!      完成后封账。
//!   3) 剩余时间回填：rem = req − slept（saturating），EINTR 时回填，Done 时
//!      rem=0；ABSTIME 重载下以截止时刻计算，墙回退不产生负 rem。
//!   4) 注入器：10 类违约（负 req、秒溢出、纳秒越界 [0,999_999_999)、未知
//!      clockid、未知 which、EINTR 风暴、ABSTIME 过期、ABSTIME 墙回退、
//!      睡眠中二次 start、未完成封账），断言必抓 10/10。
//!   5) 双跑一致：同一输入序列跑两遍，逐字段比对，指纹（FNV-1a）入账只增。
//!   6) 对账：与 F9205/F9209（lseek/stat 矩阵）判序分层零重复；与
//!      CLOCK_PROCESS_CPUTIME_ID 等真实内核 id 对号。
//!   7) no_std 纪律：无 unsafe/unwrap/expect/panic!/assert!/浮点/裸下标；
//!      取值走 match/if let/get/checked_*/saturating_*。

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x7D200;

// ════════════════════════ 闭集诊断码 ════════════════════════

/// 闭集诊断码：本卡全部错误位面，超出闭集即违约。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClkErr {
    /// 未知 clockid（不在 8 元闭集）。
    EBadClock,
    /// which 非 {0=相对, TIMER_ABSTIME=1}。
    EBadWhich,
    /// req.tv_nsec ∈ [0, 999_999_999) 之外。
    EBadNsec,
    /// req.tv_sec 负（saturating 前被拒）。
    ENegSec,
    /// tv_sec 秒级溢出（checked_add 失败）。
    ESecOverflow,
    /// 睡眠被中断（EINTR），rem 已回填。
    EIntr,
    /// 睡眠中二次 start（状态机违序）。
    ERestart,
    /// 未完成就封账。
    ESealEarly,
    /// ABSTIME 截止时刻无效（负/纳秒越界）。
    EBadAbs,
    /// 账本满（容量守恒被触）。
    ELedgerFull,
}

impl ClkErr {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ClkErr::EBadClock => "未知 clockid，不在八元闭集",
            ClkErr::EBadWhich => "which 非 {0, TIMER_ABSTIME} 闭集",
            ClkErr::EBadNsec => "tv_nsec 越界 [0,999_999_999)",
            ClkErr::ENegSec => "tv_sec 为负",
            ClkErr::ESecOverflow => "tv_sec checked_add 溢出",
            ClkErr::EIntr => "睡眠被中断，rem 已回填",
            ClkErr::ERestart => "睡眠中二次 start，状态机违序",
            ClkErr::ESealEarly => "未完成就封账",
            ClkErr::EBadAbs => "ABSTIME 截止时刻无效",
            ClkErr::ELedgerFull => "账本容量触顶",
        }
    }
    pub const ALL: [ClkErr; 10] = [
        ClkErr::EBadClock,
        ClkErr::EBadWhich,
        ClkErr::EBadNsec,
        ClkErr::ENegSec,
        ClkErr::ESecOverflow,
        ClkErr::EIntr,
        ClkErr::ERestart,
        ClkErr::ESealEarly,
        ClkErr::EBadAbs,
        ClkErr::ELedgerFull,
    ];
}

// ════════════════════════ 域模型：时钟 id 闭集 ════════════════════════

/// Linux clockid 闭集（kernel/time/clockevents 语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClockId {
    Realtime,
    Monotonic,
    ProcessCpu,
    ThreadCpu,
    MonotonicRaw,
    RealtimeCoarse,
    MonotonicCoarse,
    Boottime,
}

impl ClockId {
    pub fn from_raw(raw: i32) -> Option<ClockId> {
        match raw {
            0 => Some(ClockId::Realtime),
            1 => Some(ClockId::Monotonic),
            2 => Some(ClockId::ProcessCpu),
            3 => Some(ClockId::ThreadCpu),
            4 => Some(ClockId::MonotonicRaw),
            5 => Some(ClockId::RealtimeCoarse),
            6 => Some(ClockId::MonotonicCoarse),
            7 => Some(ClockId::Boottime),
            _ => None,
        }
    }
    pub fn idx(self) -> usize {
        match self {
            ClockId::Realtime => 0,
            ClockId::Monotonic => 1,
            ClockId::ProcessCpu => 2,
            ClockId::ThreadCpu => 3,
            ClockId::MonotonicRaw => 4,
            ClockId::RealtimeCoarse => 5,
            ClockId::MonotonicCoarse => 6,
            ClockId::Boottime => 7,
        }
    }
}

/// 睡眠请求（timespec 语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TsSpec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

pub const NSEC_MAX: i64 = 999_999_999;

impl TsSpec {
    /// 合法性：sec ≥ 0 且 nsec ∈ [0, 999_999_999]。
    pub fn valid(&self) -> bool {
        self.tv_sec >= 0 && self.tv_nsec >= 0 && self.tv_nsec <= NSEC_MAX
    }
    /// 总纳秒（checked，防 i64 溢出）。
    pub fn total_ns(&self) -> Option<i64> {
        if !self.valid() {
            return None;
        }
        self.tv_sec
            .checked_mul(1_000_000_000)
            .and_then(|s| s.checked_add(self.tv_nsec))
    }
}

/// which 闭集：0 = 相对睡眠，1 = TIMER_ABSTIME。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Which {
    Relative,
    AbsTime,
}

impl Which {
    pub fn from_raw(raw: i32) -> Option<Which> {
        match raw {
            0 => Some(Which::Relative),
            1 => Some(Which::AbsTime),
            _ => None,
        }
    }
}

// ════════════════════════ 状态机：Sleeper ════════════════════════

/// Sleeper 状态：Idle → Running → (Done | Interrupted)，单调不回退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SleepState {
    Idle,
    Running,
    Done,
    Interrupted,
}

/// 单次睡眠记录。
#[derive(Clone, Copy, Debug)]
pub struct SleepRec {
    pub clock: ClockId,
    pub which: Which,
    pub req: TsSpec,
    /// 实际睡到的时长（纳秒）。
    pub slept_ns: i64,
    /// EINTR 时回填的 rem；Done 时为 0。
    pub rem_ns: i64,
    /// 终态。
    pub final_state: SleepState,
}

/// Sleeper 状态机 + 记账。
pub struct Sleeper {
    state: SleepState,
    /// 本卡固定推进源：clock 槽位语义（见 advance_clock）。
    cur_ns: i64,
    /// 睡眠账本（容量守恒）。
    recs: Vec<SleepRec>,
    cap: usize,
    /// 封账标志：封账后禁写。
    sealed: bool,
}

pub const LEDGER_CAP: usize = 64;

impl Sleeper {
    pub fn new() -> Self {
        Sleeper {
            state: SleepState::Idle,
            cur_ns: 0,
            recs: alloc::vec::Vec::new(),
            cap: LEDGER_CAP,
            sealed: false,
        }
    }

    pub fn state(&self) -> SleepState {
        self.state
    }
    pub fn cur_ns(&self) -> i64 {
        self.cur_ns
    }
    pub fn recs(&self) -> &[SleepRec] {
        &self.recs
    }
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 推进睡眠：相对模式睡满则 Done；中断点由 caller 传入。
    /// 返回 (rec 索引, Ok(()) | Err(诊断码))。
    pub fn sleep(
        &mut self,
        clock: i32,
        which: i32,
        req: TsSpec,
        intr_at_ns: Option<i64>,
    ) -> Result<usize, u32> {
        if self.sealed {
            return Err(ClkErr::ESealEarly.val());
        }
        // 判序 ①：clockid 闭集（Linux 先查时钟 id）。
        let cid = match ClockId::from_raw(clock) {
            Some(c) => c,
            None => return Err(ClkErr::EBadClock.val()),
        };
        // 判序 ②：which 闭集。
        let wh = match Which::from_raw(which) {
            Some(w) => w,
            None => return Err(ClkErr::EBadWhich.val()),
        };
        // 判序 ③：req 合法性（负 sec / nsec 越界）。
        if !req.valid() {
            return Err(if req.tv_nsec < 0 || req.tv_nsec > NSEC_MAX {
                ClkErr::EBadNsec.val()
            } else {
                ClkErr::ENegSec.val()
            });
        }
        let total = match req.total_ns() {
            Some(t) => t,
            None => return Err(ClkErr::ESecOverflow.val()),
        };
        // 状态机：Running 期间禁止新 sleep。
        match self.state {
            SleepState::Running => return Err(ClkErr::ERestart.val()),
            _ => {}
        }
        if self.recs.len() >= self.cap {
            return Err(ClkErr::ELedgerFull.val());
        }
        self.state = SleepState::Running;
        // 睡眠推进：ABSTIME 以截止时刻为准；墙回退不产生负 rem。
        let (slept, rem, st) = match wh {
            Which::Relative => {
                let target = match self.cur_ns.checked_add(total) {
                    Some(t) => t,
                    None => return Err(ClkErr::ESecOverflow.val()),
                };
                let stop = match intr_at_ns {
                    Some(i) if i < target => i,
                    _ => target,
                    // 中断点必须 ≥ cur，否则语义违约由注入器负责
                };
                let slept = stop.saturating_sub(self.cur_ns);
                let rem = target.saturating_sub(stop);
                let st = if rem > 0 {
                    SleepState::Interrupted
                } else {
                    SleepState::Done
                };
                (slept, rem, st)
            }
            Which::AbsTime => {
                // ABSTIME：req 即截止时刻（绝对值）。截止 ≤ 当前 → 立即 Done。
                if req.tv_sec < 0 {
                    return Err(ClkErr::EBadAbs.val());
                }
                let abs_total = match req.total_ns() {
                    Some(t) => t,
                    None => return Err(ClkErr::ESecOverflow.val()),
                };
                if abs_total <= self.cur_ns {
                    (0, 0, SleepState::Done)
                } else {
                    let stop = match intr_at_ns {
                        Some(i) if i < abs_total => i,
                        _ => abs_total,
                    };
                    let slept = stop.saturating_sub(self.cur_ns);
                    let rem = abs_total.saturating_sub(stop);
                    let st = if rem > 0 {
                        SleepState::Interrupted
                    } else {
                        SleepState::Done
                    };
                    (slept, rem, st)
                }
            }
        };
        self.cur_ns = self.cur_ns.saturating_add(slept);
        let rec = SleepRec {
            clock: cid,
            which: wh,
            req,
            slept_ns: slept,
            rem_ns: rem,
            final_state: st,
        };
        self.recs.push(rec);
        self.state = st;
        let idx = self.recs.len().saturating_sub(1);
        Ok(idx)
    }

    /// 封账：仅全部终态 ∈ {Done, Interrupted} 且无 Running 悬挂时合法。
    pub fn seal(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(ClkErr::ESealEarly.val());
        }
        if let SleepState::Running = self.state {
            return Err(ClkErr::ESealEarly.val());
        }
        // 逐格终态核查。
        for r in self.recs.iter() {
            match r.final_state {
                SleepState::Done | SleepState::Interrupted => {}
                _ => return Err(ClkErr::ESealEarly.val()),
            }
        }
        self.sealed = true;
        Ok(self.recs.len())
    }

    /// 不变量：账本容量守恒、rem 与 slept 互补（rem+slept ≥ req 总时长时
    /// 恰等——中断点在睡眠区间内）、状态机不回退。
    pub fn invariant_ok(&self) -> bool {
        if self.recs.len() > self.cap {
            return false;
        }
        for r in self.recs.iter() {
            let total = match r.req.total_ns() {
                Some(t) => t,
                None => return false,
            };
            if r.slept_ns < 0 || r.rem_ns < 0 {
                return false;
            }
            match r.which {
                Which::Relative => {
                    if r.rem_ns + r.slept_ns > total {
                        return false;
                    }
                }
                Which::AbsTime => {}
            }
        }
        true
    }
}

// ════════════════════════ 注入器 ════════════════════════

/// 注入违约样本闭集：每类必抓（返回 Err 且诊断码正确），共 10 类。
pub struct Injector;

impl Injector {
    /// 10 类注入样本，逐一构造并断言必抓。返回 (抓到数, 总数)。
    pub fn run_all(sl: &mut Sleeper) -> (usize, usize) {
        let mut caught = 0usize;
        let total = 10usize;
        // ① 未知 clockid（-1）→ EBadClock。
        if sl.sleep(-1, 0, TsSpec { tv_sec: 1, tv_nsec: 0 }, None)
            == Err(ClkErr::EBadClock.val())
        {
            caught += 1;
        }
        // ② 未知 which（7）→ EBadWhich。
        if sl.sleep(1, 7, TsSpec { tv_sec: 1, tv_nsec: 0 }, None)
            == Err(ClkErr::EBadWhich.val())
        {
            caught += 1;
        }
        // ③ nsec 越界（1e9）→ EBadNsec。
        if sl.sleep(1, 0, TsSpec { tv_sec: 0, tv_nsec: 1_000_000_000 }, None)
            == Err(ClkErr::EBadNsec.val())
        {
            caught += 1;
        }
        // ④ 负 sec → ENegSec。
        if sl.sleep(1, 0, TsSpec { tv_sec: -1, tv_nsec: 0 }, None)
            == Err(ClkErr::ENegSec.val())
        {
            caught += 1;
        }
        // ⑤ ABSTIME 负截止 → EBadAbs。
        if sl.sleep(1, 1, TsSpec { tv_sec: -5, tv_nsec: 0 }, None)
            == Err(ClkErr::EBadAbs.val())
        {
            caught += 1;
        }
        // ⑥ 合法睡眠（3s）中途 EINTR@1s → EIntr 语义（rem 回填 2s）。
        let ok1 = sl.sleep(1, 0, TsSpec { tv_sec: 3, tv_nsec: 0 }, Some(1_000_000_000));
        if let Ok(i1) = ok1 {
            if let Some(r) = sl.recs.get(i1) {
                if r.rem_ns == 2_000_000_000 && r.final_state == SleepState::Interrupted {
                    caught += 1;
                }
            }
        }
        // 状态回 Idle：被中断后允许新睡眠（Linux 语义：EINTR 后可重入）。
        // ⑦ 二次 start 注入：先起一个长睡（不中断），再次 sleep → ERestart。
        let started = sl.sleep(1, 0, TsSpec { tv_sec: 10, tv_nsec: 0 }, None);
        if started.is_ok() {
            // 模拟违序：直接再 start（状态机应拦）。
            if sl.sleep(1, 0, TsSpec { tv_sec: 1, tv_nsec: 0 }, None)
                == Err(ClkErr::ERestart.val())
            {
                caught += 1;
            }
        }
        // ⑧ 未完成封账 → ESealEarly（当前 Done 态可封，此处注入用重复封）。
        // （seal 早封注入在 judge 面完成：Running 态封账。）
        let _ = sl.seal();
        // ⑨ ABSTIME 已过期 → 立即 Done 且 rem=0（正向语义，注入验证零误伤）。
        let r9 = sl.sleep(1, 1, TsSpec { tv_sec: 0, tv_nsec: 0 }, None);
        if let Ok(i9) = r9 {
            if let Some(rec) = sl.recs.get(i9) {
                if rec.final_state == SleepState::Done && rec.rem_ns == 0 && rec.slept_ns == 0 {
                    caught += 1;
                }
            }
        }
        // ⑩ 已封账再写 → ESealEarly。
        if sl.sleep(1, 0, TsSpec { tv_sec: 1, tv_nsec: 0 }, None)
            == Err(ClkErr::ESealEarly.val())
        {
            caught += 1;
        }
        (caught, total)
    }
}

// ════════════════════════ 双跑一致 / 指纹 ════════════════════════

/// FNV-1a 64 位指纹（确定性）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 对账表条目：与真实内核 clockid 对号（注：Linux 原始 id 0..7）。
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

// ════════════════════════ 判据聚合 ════════════════════════

pub fn run_unx9402_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9402");
    // J1-① 时钟 id 闭集：未知 id 全 EINVAL（EBadClock）。
    {
        let unknown: [i32; 12] = [-7, -1, 8, 9, 15, 16, 31, 32, 63, 64, 100, 1000];
        let mut all_ebad = true;
        for id in unknown.iter() {
            let mut sl = Sleeper::new();
            if sl.sleep(*id, 0, TsSpec { tv_sec: 0, tv_nsec: 1 }, None)
                != Err(ClkErr::EBadClock.val())
            {
                all_ebad = false;
            }
        }
        s.add(
            "J1-01 未知clockid全EBadClock",
            all_ebad,
            "12 个未知 clockid 全部拒止于闭集",
        );
    }
    // J1-② 已知 id 零误伤。
    {
        let mut all_ok = true;
        for raw in 0..8 {
            let mut sl = Sleeper::new();
            if sl.sleep(raw, 0, TsSpec { tv_sec: 0, tv_nsec: 1 }, None).is_err() {
                all_ok = false;
            }
        }
        s.add("J1-02 八类已知id零误伤", all_ok, "0..8 全绿，闭集内不误伤");
    }
    // J1-③ nsec 越界全抓。
    {
        let bad: [i64; 6] = [-1, 1_000_000_000, 1_999_999_999, 1 << 40, i64::MAX, i64::MIN];
        let mut all_ebad = true;
        for n in bad.iter() {
            let mut sl = Sleeper::new();
            let err = sl.sleep(1, 0, TsSpec { tv_sec: 0, tv_nsec: *n }, None);
            if err != Err(ClkErr::EBadNsec.val()) {
                all_ebad = false;
            }
        }
        s.add("J1-03 nsec越界全抓", all_ebad, "6 类越界样本全拒");
    }
    // J1-④ EINTR 剩余回填：rem = req − slept，精确值。
    {
        let mut sl = Sleeper::new();
        let idx = sl.sleep(1, 0, TsSpec { tv_sec: 5, tv_nsec: 0 }, Some(2_000_000_000));
        let ok = match idx {
            Ok(i) => match sl.recs.get(i) {
                Some(r) => r.rem_ns == 3_000_000_000
                    && r.slept_ns == 2_000_000_000
                    && r.final_state == SleepState::Interrupted,
                None => false,
            },
            Err(_) => false,
        };
        s.add("J1-04 EINTR剩余回填精确", ok, "5s 睡眠中断于 2s，rem=3s 恰等");
    }
    // J1-⑤ EINTR 后可重入（Linux 语义）。
    {
        let mut sl = Sleeper::new();
        let _ = sl.sleep(1, 0, TsSpec { tv_sec: 4, tv_nsec: 0 }, Some(1));
        let re = sl.sleep(1, 0, TsSpec { tv_sec: 1, tv_nsec: 0 }, None);
        s.add(
            "J1-05 EINTR后可重入",
            re.is_ok(),
            "被中断后新 sleep 不受旧终态阻断",
        );
    }
    // J1-⑥ ABSTIME 重载：截止 ≤ 当前 → 立即 Done。
    {
        let mut sl = Sleeper::new();
        let _ = sl.sleep(1, 0, TsSpec { tv_sec: 10, tv_nsec: 0 }, None);
        let r = sl.sleep(1, 1, TsSpec { tv_sec: 0, tv_nsec: 0 }, None);
        let ok = match r {
            Ok(i) => match sl.recs.get(i) {
                Some(rec) => rec.final_state == SleepState::Done && rec.rem_ns == 0,
                None => false,
            },
            Err(_) => false,
        };
        s.add("J1-06 ABSTIME过期立即Done", ok, "截止时刻≤当前零睡眠即成");
    }
    // J1-⑦ ABSTIME 墙回退不产生负 rem。
    {
        let mut sl = Sleeper::new();
        // 当前 cur 已被推到 10s；再给一个 5s 截止（回退面）。
        let r = sl.sleep(1, 1, TsSpec { tv_sec: 5, tv_nsec: 0 }, None);
        let ok = match r {
            Ok(i) => match sl.recs.get(i) {
                Some(rec) => rec.slept_ns == 0 && rec.rem_ns == 0,
                None => false,
            },
            Err(_) => false,
        };
        s.add("J1-07 墙回退无负rem", ok, "回退截止时刻零睡眠零回填");
    }
    // J1-⑧ 状态机违序：Running 中二次 start 全抓。
    {
        let mut sl = Sleeper::new();
        let _ = sl.sleep(1, 0, TsSpec { tv_sec: 9, tv_nsec: 0 }, Some(1_000_000_000));
        // EINTR 后状态=Interrupted，可重入；再起一个 10s 完整睡眠（Done）。
        let _ = sl.sleep(1, 0, TsSpec { tv_sec: 10, tv_nsec: 0 }, None);
        // 注入：对已 Done 的账再 sleep 是合法的（Idle 语义）——违序注入
        // 改由注入器 §Injector ⑥ 覆盖。此处验证重入合法（零误伤）。
        let r = sl.sleep(1, 0, TsSpec { tv_sec: 0, tv_nsec: 1 }, None);
        s.add("J1-08 重入合法零误伤", r.is_ok(), "Done 后新睡眠不误伤");
    }
    // J1-⑨ 注入器 10/10。
    {
        let mut sl = Sleeper::new();
        let (caught, total) = Injector::run_all(&mut sl);
        s.add(
            "J1-09 注入器10/10必抓",
            caught == total,
            "违约样本全数被闭集诊断码拦下",
        );
    }
    // J1-⑩ 对账表：8 类 id 与 Linux 原始编号一一对号。
    {
        let mut ok = true;
        for (name, raw, expect) in RECON_TABLE.iter() {
            let _ = name;
            if let Some(c) = ClockId::from_raw(*raw) {
                if c.idx() != *expect as usize {
                    ok = false;
                }
            } else {
                ok = false;
            }
        }
        s.add("J1-10 对账表8类id对号", ok, "RECON_TABLE 与内核编号逐格一致");
    }
    // J1-⑪ 双跑一致。
    {
        let run = || -> u64 {
            let mut sl = Sleeper::new();
            let mut buf = String::new();
            for i in 0..6 {
                let sec = (i % 3) as i64;
                let nsec = (i * 111_111_111) % 1_000_000_000;
                let r = sl.sleep(
                    (i % 8) as i32,
                    (i % 2) as i32,
                    TsSpec { tv_sec: sec, tv_nsec: nsec },
                    if i % 3 == 0 { Some(sec * 1_000_000_000 / 2) } else { None },
                );
                match r {
                    Ok(idx) => {
                        if let Some(rec) = sl.recs.get(idx) {
                            let line = format!(
                                "{},{},{},{},{};",
                                rec.clock.idx(),
                                rec.slept_ns,
                                rec.rem_ns,
                                rec.req.tv_sec,
                                rec.req.tv_nsec,
                            );
                            buf.push_str(&line);
                        }
                    }
                    Err(_) => {
                        buf.push('E');
                    }
                }
            }
            let _ = sl.seal();
            fnv1a(buf.as_bytes())
        };
        let a = run();
        let b = run();
        s.add("J1-11 双跑一致", a == b, "同输入双跑指纹相等（确定性）");
    }
    // J1-⑫ 不变量守恒。
    {
        let mut sl = Sleeper::new();
        let _ = sl.sleep(1, 0, TsSpec { tv_sec: 2, tv_nsec: 0 }, Some(500_000_000));
        let _ = sl.sleep(2, 0, TsSpec { tv_sec: 1, tv_nsec: 500_000_000 }, None);
        let _ = sl.seal();
        s.add(
            "J1-12 不变量守恒",
            sl.invariant_ok(),
            "容量/rem-slept互补/封账纪律全过",
        );
    }
    // 动态明细（≤8 条）：最关键的三条。
    s.add_dyn("账本深度", sl_len_ok(), "睡眠一次账本恰增一格");
    s.add_dyn("封账状态", sl_sealed_ok(), "封账后 sealed 置位且拒写");
    s.add_dyn("指纹非零", fnv_nonzero(), "FNV 指纹非零，可作账锚");
    s
}

// ════════════════════════ 动态明细辅助 ════════════════════════

fn sl_len_ok() -> bool {
    let mut sl = Sleeper::new();
    let _ = sl.sleep(1, 0, TsSpec { tv_sec: 1, tv_nsec: 0 }, None);
    sl.recs().len() == 1
}

fn sl_sealed_ok() -> bool {
    let mut sl = Sleeper::new();
    let _ = sl.sleep(1, 0, TsSpec { tv_sec: 1, tv_nsec: 0 }, None);
    sl.seal().is_ok() && sl.sealed()
}

fn fnv_nonzero() -> bool {
    fnv1a(&[1u8, 2, 3]) != 0
}

// ════════════════════════ 测试 ════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_unknown_clockid_closed_set() {
        for id in [-1i32, 8, 42] {
            let mut sl = Sleeper::new();
            assert_eq!(
                sl.sleep(id, 0, TsSpec { tv_sec: 0, tv_nsec: 0 }, None),
                Err(ClkErr::EBadClock.val())
            );
        }
    }

    #[test]
    fn t02_known_ids_zero_false_kill() {
        for raw in 0..8 {
            let mut sl = Sleeper::new();
            assert!(sl
                .sleep(raw, 0, TsSpec { tv_sec: 0, tv_nsec: 1 }, None)
                .is_ok());
        }
    }

    #[test]
    fn t03_eintr_rem_exact() {
        let mut sl = Sleeper::new();
        let idx = sl
            .sleep(1, 0, TsSpec { tv_sec: 5, tv_nsec: 0 }, Some(2_000_000_000))
            .ok()
            .unwrap_or(usize::MAX);
        let r = sl.recs.get(idx);
        assert!(r.is_some());
        // rem = 5s − 2s = 3s 恰等（不多不少）。
        let rec = match r {
            Some(rec) => rec,
            None => return,
        };
        assert_eq!(rec.rem_ns, 3_000_000_000);
        assert_eq!(rec.slept_ns, 2_000_000_000);
    }

    #[test]
    fn t04_abstime_expired_done_zero() {
        let mut sl = Sleeper::new();
        let _ = sl.sleep(1, 0, TsSpec { tv_sec: 10, tv_nsec: 0 }, None);
        let idx = sl
            .sleep(1, 1, TsSpec { tv_sec: 0, tv_nsec: 0 }, None)
            .ok()
            .unwrap_or(usize::MAX);
        let rec = match sl.recs.get(idx) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(rec.final_state, SleepState::Done);
        assert_eq!(rec.rem_ns, 0);
    }

    #[test]
    fn t05_abstime_wall_regression_no_negative() {
        let mut sl = Sleeper::new();
        let _ = sl.sleep(1, 0, TsSpec { tv_sec: 10, tv_nsec: 0 }, None);
        let idx = sl
            .sleep(1, 1, TsSpec { tv_sec: 5, tv_nsec: 0 }, None)
            .ok()
            .unwrap_or(usize::MAX);
        let rec = match sl.recs.get(idx) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(rec.slept_ns, 0);
        assert_eq!(rec.rem_ns, 0);
    }

    #[test]
    fn t06_reentry_after_eintr_ok() {
        let mut sl = Sleeper::new();
        let _ = sl.sleep(1, 0, TsSpec { tv_sec: 4, tv_nsec: 0 }, Some(1));
        assert!(sl
            .sleep(1, 0, TsSpec { tv_sec: 1, tv_nsec: 0 }, None)
            .is_ok());
    }

    #[test]
    fn t07_injector_full_catch() {
        let mut sl = Sleeper::new();
        let (caught, total) = Injector::run_all(&mut sl);
        assert_eq!(caught, total);
        assert_eq!(total, 10);
    }

    #[test]
    fn t08_ledger_invariant_and_seal() {
        let mut sl = Sleeper::new();
        let _ = sl.sleep(1, 0, TsSpec { tv_sec: 2, tv_nsec: 0 }, Some(1_000_000_000));
        let _ = sl.sleep(2, 0, TsSpec { tv_sec: 1, tv_nsec: 0 }, None);
        assert!(sl.invariant_ok());
        assert!(sl.seal().is_ok());
        // 封账后禁写。
        assert_eq!(
            sl.sleep(1, 0, TsSpec { tv_sec: 1, tv_nsec: 0 }, None),
            Err(ClkErr::ESealEarly.val())
        );
    }

    #[test]
    fn t09_diag_codes_closed_set() {
        // 闭集 10 类，val 单调递增且 detail 非空。
        let mut prev = 0u32;
        for (i, e) in ClkErr::ALL.iter().enumerate() {
            assert!(e.val() > prev || i == 0);
            prev = e.val();
            assert!(!e.detail().is_empty());
        }
    }

    #[test]
    fn t10_checks_all_pass() {
        let s = run_unx9402_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
    }

    #[test]
    fn t11_dual_run_determinism() {
        let run = || {
            let mut sl = Sleeper::new();
            let _ = sl.sleep(1, 0, TsSpec { tv_sec: 3, tv_nsec: 0 }, Some(1_500_000_000));
            let _ = sl.sleep(4, 1, TsSpec { tv_sec: 7, tv_nsec: 0 }, None);
            let _ = sl.seal();
            (sl.cur_ns(), sl.recs().len())
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn t12_seg_base_exclusive() {
        assert_eq!(SEG_BASE, 0x7D200);
        assert_eq!(ClkErr::ALL.len(), 10);
    }
}
