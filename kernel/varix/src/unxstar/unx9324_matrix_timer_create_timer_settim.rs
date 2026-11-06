//! UNX-F9324 timer_create/timer_settime 错误矩阵（EINVAL/EAGAIN） · 0x7BE80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9324 逐字）：
//!   锚点：UNX-F9324-J1 两错误码注入矩阵判据过，非法 sigevent 全 EINVAL
//!   格过，定时器耗尽全 EAGAIN 格过。
//!
//! 设计要点：
//!   ① 双错误码分面建模：EINVAL 出自参数面（clockid 非法 / sigevent
//!      notify 值越闭集 / it_value 与 it_interval 同时为零），EAGAIN 出自
//!      资源面（每进程 POSIX 定时器数越过 RLIMIT 上限）。两码不得互串：
//!      资源满时传坏 sigevent 仍必须报 EINVAL（参数校验先于资源检查），
//!      参数合法才谈配额。
//!   ② sigevent 解析器：notify 闭集（NONE/SIGNAL/THREAD）逐档校验；
//!      非法 notify 值、非法 sigev_signo（0 或 >31）、NONE 之外的空载荷
//!      逐项拒绝。sigev_notify=NONE 是合法「纯计时」档。
//!   ③ 定时器注册表：定容表 + 配额账；create 消耗一格，delete 归还。
//!      归还后配额可复用（账闭合：活跃数 + 空闲数 = 容量）。
//!   ④ it_value/it_interval 语义：both zero → 一次性 disarm（合法，等效
//!      停表）；value 零 interval 非零 → EINVAL（Linux 口径）；value 非零
//!      → 装表。
//!   ⑤ 注入核四类坏样本：坏 notify、坏 signo、配额满、both-zero 后
//!      interval 单独非零——逐类拒绝，账守恒逃逸 = 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9324 独占 0x7BE80..=0x7BEBF）。
pub const SEG_BASE: u32 = 0x7BE80;

/// 注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 每进程 POSIX 定时器容量（RLIMIT 联动面）。
pub const TIMER_QUOTA: usize = 6;

/// sigev_notify 闭集：无通知。
pub const NOTIFY_NONE: u32 = 0;
/// sigev_notify 闭集：信号通知。
pub const NOTIFY_SIGNAL: u32 = 1;
/// sigev_notify 闭集：线程通知。
pub const NOTIFY_THREAD: u32 = 2;

/// Linux errno：坏参数。
pub const EINVAL: i32 = 22;
/// Linux errno：资源耗尽。
pub const EAGAIN: i32 = 11;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimerCode {
    /// notify 值越闭集
    BadNotify,
    /// signo 非法（0 或 >31）
    BadSigno,
    /// it_value 零而 interval 非零
    BadArming,
    /// 定时器配额耗尽
    QuotaExhausted,
    /// 未注册 timer_id 操作
    UnknownTimerId,
    /// 重复 delete
    DoubleDelete,
    /// 配额账不闭合
    QuotaLedgerBroken,
    /// clockid 非法
    BadClockid,
    /// 账本倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl TimerCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            TimerCode::BadNotify => "notify 越闭集被拒：sigev_notify 只允许 \
                NONE/SIGNAL/THREAD；越界档回 EINVAL",
            TimerCode::BadSigno => "signo 非法被拒：sigev_signo 必须落在 1..=31；\
                0 号与 32+ 号回 EINVAL",
            TimerCode::BadArming => "装表非法被拒：it_value 为零而 it_interval \
                非零回 EINVAL；双零是合法 disarm",
            TimerCode::QuotaExhausted => "配额耗尽被拒：活跃定时器数已达 \
                RLIMIT 上限；请先 delete 释放再 create，回 EAGAIN",
            TimerCode::UnknownTimerId => "未知 timer_id 被拒：操作前必须持有 \
                create 返回的合法 id；幽灵 id 回 EINVAL",
            TimerCode::DoubleDelete => "重复 delete 被拒：一个 id 只能归还一次；\
                双删破坏配额账闭合",
            TimerCode::QuotaLedgerBroken => "配额账不闭合：活跃 + 空闲必须等于 \
                容量；闭合失败即归还路径有漏",
            TimerCode::BadClockid => "clockid 非法被拒：闭集为 \
                REALTIME/MONOTONIC/BOOTTIME；越界回 EINVAL",
            TimerCode::LedgerRegress => "账本倒退被拒：注入账只增不减；请检查 \
                是否有回滚路径误改累计值",
            TimerCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            TimerCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7B400..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [TimerCode; 11] = [
        TimerCode::BadNotify,
        TimerCode::BadSigno,
        TimerCode::BadArming,
        TimerCode::QuotaExhausted,
        TimerCode::UnknownTimerId,
        TimerCode::DoubleDelete,
        TimerCode::QuotaLedgerBroken,
        TimerCode::BadClockid,
        TimerCode::LedgerRegress,
        TimerCode::NotReproducible,
        TimerCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带 errno 正数。
pub type SysResult<T> = Result<T, i32>;

// ── sigevent 解析器 ──────────────────────────────────────────────────
/// sigevent 规格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SigEvent {
    /// 通知档（0/1/2 闭集）
    pub notify: u32,
    /// 信号号（notify=SIGNAL 时必须 1..=31）
    pub signo: u8,
    /// 载荷值
    pub value: u32,
}

/// sigevent 校验：notify 闭集 + signo 域。
pub fn validate_sigevent(ev: &SigEvent) -> SysResult<()> {
    match ev.notify {
        NOTIFY_NONE => Ok(()),
        NOTIFY_SIGNAL | NOTIFY_THREAD => {
            if ev.signo == 0 || ev.signo > 31 {
                Err(EINVAL)
            } else {
                Ok(())
            }
        }
        _ => Err(EINVAL),
    }
}

/// clockid 闭集。
pub const CLOCKID_REALTIME: i32 = 0;
pub const CLOCKID_MONOTONIC: i32 = 1;
pub const CLOCKID_BOOTTIME: i32 = 7;

/// clockid 校验。
pub fn validate_clockid(id: i32) -> SysResult<()> {
    match id {
        CLOCKID_REALTIME | CLOCKID_MONOTONIC | CLOCKID_BOOTTIME => Ok(()),
        _ => Err(EINVAL),
    }
}

/// itimer 规格（value/interval 纳秒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItimerSpec {
    /// 首触发
    pub value: u64,
    /// 周期
    pub interval: u64,
}

/// 装表校验：双零合法 disarm；value 零 interval 非零 EINVAL。
pub fn validate_arming(spec: &ItimerSpec) -> SysResult<()> {
    if spec.value == 0 && spec.interval == 0 {
        return Ok(());
    }
    if spec.value == 0 {
        return Err(EINVAL);
    }
    Ok(())
}

// ── 定时器注册表 + 配额账 ────────────────────────────────────────────
/// 定时器槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimerSlot {
    /// 占用态
    pub used: bool,
    /// clockid
    pub clock: i32,
    /// 通知档
    pub notify: u32,
}

/// 定时器注册表：定容 + 配额账。
#[derive(Clone, Copy, Debug)]
pub struct TimerRegistry {
    slots: [TimerSlot; TIMER_QUOTA],
    /// 活跃数
    active: usize,
    /// 累计 create 成功数（账，只增）
    pub creates: u32,
    /// 累计 delete 成功数（账，只增）
    pub deletes: u32,
}

impl TimerRegistry {
    /// 构造空表。
    pub const fn new() -> TimerRegistry {
        TimerRegistry {
            slots: [TimerSlot { used: false, clock: 0, notify: 0 }; TIMER_QUOTA],
            active: 0,
            creates: 0,
            deletes: 0,
        }
    }

    /// timer_create：参数面校验先于资源面检查。
    /// 返回 timer_id（槽下标）。
    pub fn create(&mut self, clock: i32, ev: &SigEvent) -> SysResult<usize> {
        validate_clockid(clock)?;
        validate_sigevent(ev)?;
        if self.active >= TIMER_QUOTA {
            return Err(EAGAIN);
        }
        let mut i = 0usize;
        while i < TIMER_QUOTA {
            if let Some(slot) = self.slots.get_mut(i) {
                if !slot.used {
                    slot.used = true;
                    slot.clock = clock;
                    slot.notify = ev.notify;
                    self.active += 1;
                    self.creates = self.creates.saturating_add(1);
                    return Ok(i);
                }
            }
            i += 1;
        }
        Err(EAGAIN)
    }

    /// timer_delete：归还能量；幽灵 id / 双删拒绝。
    pub fn delete(&mut self, id: usize) -> SysResult<()> {
        let slot = match self.slots.get(id) {
            Some(s) => s,
            None => return Err(EINVAL),
        };
        if !slot.used {
            return Err(EINVAL);
        }
        if let Some(s) = self.slots.get_mut(id) {
            s.used = false;
            s.clock = 0;
            s.notify = 0;
        }
        self.active -= 1;
        self.deletes = self.deletes.saturating_add(1);
        Ok(())
    }

    /// timer_settime 装表：id 存在性 + 装表合法性。
    pub fn settime(&self, id: usize, spec: &ItimerSpec) -> SysResult<()> {
        match self.slots.get(id) {
            Some(s) if s.used => validate_arming(spec),
            _ => Err(EINVAL),
        }
    }

    /// 配额账闭合：活跃 + 空闲 = 容量。
    pub fn quota_closed(&self) -> bool {
        let mut free = 0usize;
        let mut i = 0usize;
        while i < TIMER_QUOTA {
            if let Some(slot) = self.slots.get(i) {
                if !slot.used {
                    free += 1;
                }
            }
            i += 1;
        }
        self.active + free == TIMER_QUOTA
    }

    /// 活跃数。
    pub fn active_count(&self) -> usize {
        self.active
    }
}

// ── 注入核：四类坏样本 ───────────────────────────────────────────────
/// 四类坏定时器样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadTimerCase {
    /// 坏 notify
    BadNotify,
    /// 坏 signo
    BadSigno,
    /// 配额满
    QuotaFull,
    /// value 零 interval 非零
    BadArming,
}

impl BadTimerCase {
    /// 闭集全量。
    pub const ALL: [BadTimerCase; 4] = [
        BadTimerCase::BadNotify,
        BadTimerCase::BadSigno,
        BadTimerCase::QuotaFull,
        BadTimerCase::BadArming,
    ];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

fn ok_ev() -> SigEvent {
    SigEvent {
        notify: NOTIFY_SIGNAL,
        signo: 14,
        value: 0,
    }
}

/// 执行一轮四类坏定时器注入：每类 INJECT_ROUNDS 次，全部必须被拒。
pub fn run_bad_timer_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadTimerCase::ALL.len() {
        if let Some(case) = BadTimerCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    BadTimerCase::BadNotify => {
                        let mut r = TimerRegistry::new();
                        let ev = SigEvent {
                            notify: 9,
                            signo: 14,
                            value: 0,
                        };
                        r.create(CLOCKID_MONOTONIC, &ev) == Err(EINVAL)
                    }
                    BadTimerCase::BadSigno => {
                        let mut r = TimerRegistry::new();
                        let ev = SigEvent {
                            notify: NOTIFY_SIGNAL,
                            signo: 0,
                            value: 0,
                        };
                        r.create(CLOCKID_MONOTONIC, &ev) == Err(EINVAL)
                    }
                    BadTimerCase::QuotaFull => {
                        let mut r = TimerRegistry::new();
                        let mut n = 0usize;
                        while n < TIMER_QUOTA {
                            let _ = r.create(CLOCKID_MONOTONIC, &ok_ev());
                            n += 1;
                        }
                        r.create(CLOCKID_MONOTONIC, &ok_ev()) == Err(EAGAIN)
                    }
                    BadTimerCase::BadArming => {
                        let mut r = TimerRegistry::new();
                        let id = r.create(CLOCKID_MONOTONIC, &ok_ev()).unwrap_or(99);
                        r.settime(
                            id,
                            &ItimerSpec {
                                value: 0,
                                interval: 5,
                            },
                        ) == Err(EINVAL)
                    }
                };
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("EAGAIN", -11, 1450, "Windows ERROR_NO_SYSTEM_RESOURCES；资源耗尽族"),
    ("ENOTSUP", -95, 50, "Windows ERROR_NOT_SUPPORTED；不支持档"),
];

/// 查表。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：满表→归还→复用 + 双零 disarm + 参数优先序。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut r = TimerRegistry::new();
    let mut n = 0usize;
    while n < TIMER_QUOTA {
        let id = r.create(CLOCKID_MONOTONIC, &ok_ev());
        bytes.push(match id {
            Ok(v) => v as u8,
            Err(e) => (e & 0xFF) as u8,
        });
        n += 1;
    }
    // 满后再 create → EAGAIN
    bytes.push(match r.create(CLOCKID_MONOTONIC, &ok_ev()) {
        Err(EAGAIN) => 1,
        _ => 0,
    });
    // 归还一格再 create 复用
    let _ = r.delete(2);
    bytes.push(match r.create(CLOCKID_REALTIME, &ok_ev()) {
        Ok(v) => v as u8,
        Err(_) => 0xFF,
    });
    // 双零 disarm 合法
    bytes.push(match r.settime(
        0,
        &ItimerSpec {
            value: 0,
            interval: 0,
        },
    ) {
        Ok(()) => 1,
        _ => 0,
    });
    // 资源满时传坏 sigevent 仍 EINVAL（参数先于资源）
    let mut r2 = TimerRegistry::new();
    let mut m = 0usize;
    while m < TIMER_QUOTA {
        let _ = r2.create(CLOCKID_MONOTONIC, &ok_ev());
        m += 1;
    }
    let ev_bad = SigEvent {
        notify: 99,
        signo: 1,
        value: 0,
    };
    bytes.push(match r2.create(CLOCKID_MONOTONIC, &ev_bad) {
        Err(EINVAL) => 1,
        _ => 0,
    });
    bytes.push(if r.quota_closed() { 1 } else { 0 });
    bytes.push(r.creates as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9324 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9324_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9324");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7B400..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本批窗口", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 非法 sigevent 全 EINVAL
    let bad_ev = SigEvent {
        notify: 9,
        signo: 14,
        value: 0,
    };
    let bad_ev2 = SigEvent {
        notify: NOTIFY_SIGNAL,
        signo: 0,
        value: 0,
    };
    let bad_ev3 = SigEvent {
        notify: NOTIFY_THREAD,
        signo: 32,
        value: 0,
    };
    let mut r = TimerRegistry::new();
    let einval_ok = r.create(CLOCKID_MONOTONIC, &bad_ev) == Err(EINVAL)
        && r.create(CLOCKID_MONOTONIC, &bad_ev2) == Err(EINVAL)
        && r.create(CLOCKID_MONOTONIC, &bad_ev3) == Err(EINVAL)
        && r.active_count() == 0;
    s.add("非法 sigevent：notify/signo 越闭集全 EINVAL 且不占格", einval_ok,
          "参数校验先于资源检查，坏参数不得消耗配额");

    // 3. 定时器耗尽全 EAGAIN
    let mut r2 = TimerRegistry::new();
    let mut filled = true;
    let mut i = 0usize;
    while i < TIMER_QUOTA {
        if r2.create(CLOCKID_MONOTONIC, &ok_ev()).is_err() {
            filled = false;
        }
        i += 1;
    }
    let again_ok = filled && r2.create(CLOCKID_MONOTONIC, &ok_ev()) == Err(EAGAIN);
    s.add("配额耗尽：满表后再 create 全 EAGAIN", again_ok,
          "资源面与参数面分码，满表必报 EAGAIN");

    // 4. 资源满时坏参数仍 EINVAL（次序冻结）
    let ev_bad = SigEvent {
        notify: 99,
        signo: 1,
        value: 0,
    };
    let order_ok = r2.create(CLOCKID_MONOTONIC, &ev_bad) == Err(EINVAL);
    s.add("判定次序：满表 + 坏 sigevent → EINVAL 优先", order_ok,
          "参数校验先于配额检查，两码不互串");

    // 5. 双零 disarm 合法，value 零 interval 非零 EINVAL
    let disarm_ok = validate_arming(&ItimerSpec {
        value: 0,
        interval: 0,
    })
    .is_ok()
        && validate_arming(&ItimerSpec {
            value: 0,
            interval: 5,
        })
        == Err(EINVAL)
        && validate_arming(&ItimerSpec {
            value: 10,
            interval: 0,
        })
        .is_ok();
    s.add("装表语义：双零 disarm 合法 / value 零 interval 非零 → EINVAL",
          disarm_ok, "Linux 口径逐格冻结");

    // 6. clockid 闭集
    let clock_ok = validate_clockid(CLOCKID_REALTIME).is_ok()
        && validate_clockid(CLOCKID_MONOTONIC).is_ok()
        && validate_clockid(CLOCKID_BOOTTIME).is_ok()
        && validate_clockid(3).is_err()
        && validate_clockid(-1).is_err();
    s.add("clockid 闭集：REALTIME/MONOTONIC/BOOTTIME 外全拒", clock_ok,
          "号域闭集，越界回 EINVAL");

    // 7. 归还复用与账闭合
    let mut r3 = TimerRegistry::new();
    let id0 = r3.create(CLOCKID_MONOTONIC, &ok_ev()).unwrap_or(99);
    let reuse_ok = r3.delete(id0).is_ok()
        && r3.quota_closed()
        && r3.active_count() == 0
        && r3.create(CLOCKID_REALTIME, &ok_ev()).is_ok()
        && r3.creates == 2
        && r3.deletes == 1;
    s.add("归还复用：delete 后配额账闭合且槽可复用", reuse_ok,
          "create/delete 账目闭合，槽归还后可再用");

    // 8. 幽灵 id 与双删
    let mut r4 = TimerRegistry::new();
    let id4 = r4.create(CLOCKID_MONOTONIC, &ok_ev()).unwrap_or(99);
    let ghost_ok = r4.delete(99) == Err(EINVAL)
        && r4.delete(id4).is_ok()
        && r4.delete(id4) == Err(EINVAL);
    s.add("delete 守卫：幽灵 id/双删全拒", ghost_ok,
          "一次归还一次记账，双删破坏闭合");

    // 9. settime 幽灵 id
    let ghost_set = r4.settime(
        99,
        &ItimerSpec {
            value: 1,
            interval: 0,
        },
    ) == Err(EINVAL);
    s.add("settime 守卫：幽灵 id → EINVAL", ghost_set,
          "装表前必须核对 id 存在性");

    // 10. 注入核：四类坏样本
    let (ledger, per_class) = run_bad_timer_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("四类坏定时器各 10/10 全检出", all_caught,
          "坏 notify/坏 signo/配额满/坏装表必须逐类拒绝");

    // 11. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0,
          "40 次注入必须 40 次检出、0 逃逸");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(EAGAIN)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 文案闭集与码唯一
    let text_ok = TimerCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < TimerCode::ALL.len() {
        let mut j = i2 + 1;
        while j < TimerCode::ALL.len() {
            if let (Some(x), Some(y)) = (TimerCode::ALL.get(i2), TimerCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    s.add("诊断码：11 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let quota_text = format!("活跃 {} / 容量 {} / 账闭合", r2.active_count(), TIMER_QUOTA);
    s.add_dyn("配额快照", r2.quota_closed(), &quota_text);
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_notify_einval() {
        let mut r = TimerRegistry::new();
        let ev = SigEvent {
            notify: 9,
            signo: 14,
            value: 0,
        };
        assert_eq!(r.create(CLOCKID_MONOTONIC, &ev), Err(EINVAL));
    }

    #[test]
    fn bad_signo_einval() {
        assert!(validate_sigevent(&SigEvent {
            notify: NOTIFY_SIGNAL,
            signo: 0,
            value: 0,
        })
        .is_err());
        assert!(validate_sigevent(&SigEvent {
            notify: NOTIFY_THREAD,
            signo: 32,
            value: 0,
        })
        .is_err());
    }

    #[test]
    fn quota_full_eagain() {
        let mut r = TimerRegistry::new();
        let mut i = 0usize;
        while i < TIMER_QUOTA {
            assert!(r.create(CLOCKID_MONOTONIC, &ok_ev()).is_ok());
            i += 1;
        }
        assert_eq!(r.create(CLOCKID_MONOTONIC, &ok_ev()), Err(EAGAIN));
    }

    #[test]
    fn params_before_quota() {
        let mut r = TimerRegistry::new();
        let mut i = 0usize;
        while i < TIMER_QUOTA {
            let _ = r.create(CLOCKID_MONOTONIC, &ok_ev());
            i += 1;
        }
        assert_eq!(
            r.create(
                CLOCKID_MONOTONIC,
                &SigEvent {
                    notify: 99,
                    signo: 1,
                    value: 0,
                }
            ),
            Err(EINVAL)
        );
    }

    #[test]
    fn arming_semantics() {
        assert!(validate_arming(&ItimerSpec {
            value: 0,
            interval: 0,
        })
        .is_ok());
        assert_eq!(
            validate_arming(&ItimerSpec {
                value: 0,
                interval: 5,
            }),
            Err(EINVAL)
        );
    }

    #[test]
    fn delete_reuse_closes_ledger() {
        let mut r = TimerRegistry::new();
        let id = r.create(CLOCKID_MONOTONIC, &ok_ev()).unwrap_or(99);
        assert!(r.delete(id).is_ok());
        assert!(r.quota_closed());
        assert!(r.create(CLOCKID_REALTIME, &ok_ev()).is_ok());
    }

    #[test]
    fn ghost_and_double_delete_rejected() {
        let mut r = TimerRegistry::new();
        let id = r.create(CLOCKID_MONOTONIC, &ok_ev()).unwrap_or(99);
        assert!(r.delete(99).is_err());
        assert!(r.delete(id).is_ok());
        assert!(r.delete(id).is_err());
    }

    #[test]
    fn bad_matrix_green() {
        let (l, per) = run_bad_timer_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EINVAL));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9324_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
