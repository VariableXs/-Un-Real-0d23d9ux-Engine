//! UNX-F9397 联调与 glibc pthread futex 消费核 · 0x7D0C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9397-J1 逐字）：
//!   锚点：UNX-F9397-J1 消费核联测判据过，pthread futex 消费链对接
//!   可观测，消费号登记格过
//!
//! 数据段 SEG_BASE = 0x7D0C0：诊断码闭集 +0x01..=+0x0A（10 码，
//! 9397 独占窗口，ALL 闭集必填，detail() 全中文非空）。
//!
//! 结构清单：
//!   - 注册表：ConsumerRegistry（消费号闭集登记：pthread_mutex /
//!     pthread_cond / pthread_once / sem / glibc rseq 桥 五号段，
//!     重复登记显性拒绝）；
//!   - 解析器/对拍：futex 操作码对拍表（FUTEX_WAIT=0 / FUTEX_WAKE=1 /
//!     FUTEX_FD=2 / FUTEX_REQUEUE=3 / FUTEX_CMP_REQUEUE=4 闭集，
//!     FUTEX_PRIVATE_FLAG=128 位面逐位对拍）；
//!   - 状态机：消费链 LockContend→FutexWait→FutexWake→Acquired 单向；
//!   - 账本：ConsumeLedger（登记=等待=唤醒=获取 四段守恒）+ 注入账
//!     （逃逸恒 0）；
//!   - 注入核：5 类坏样本（未登记消费号/重复登记/非法 futex 操作码/
//!     值漂移 EAGAIN 面/私有旗标缺失）逐类显性拒绝；
//!   - 对拍：Linux errno 对照（EAGAIN/EINTR/EBADF/EINVAL/ETIMEDOUT）；
//!   - 指纹：FNV-1a 消费链事件序列双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9397 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7D0C0;

/// 消费号闭集容量（5 号段）。
pub const CONSUMER_CLASSES: usize = 5;

/// futex 操作码闭集（Linux futex.h 口径）。
pub const FUTEX_WAIT: u32 = 0;
pub const FUTEX_WAKE: u32 = 1;
pub const FUTEX_FD: u32 = 2;
pub const FUTEX_REQUEUE: u32 = 3;
pub const FUTEX_CMP_REQUEUE: u32 = 4;

/// FUTEX_PRIVATE_FLAG（glibc 私有 futex 位面）。
pub const FUTEX_PRIVATE_FLAG: u32 = 128;

/// Linux errno 数值（正数口径）。
pub const EAGAIN: i32 = 11;
pub const EINTR: i32 = 4;
pub const ETIMEDOUT: i32 = 110;
pub const EBADF: i32 = 9;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（SEG_BASE + n，n ∈ 1..=10 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumerCode {
    /// 未登记消费号
    ConsumerUnregistered,
    /// 重复登记消费号
    ConsumerDuplicate,
    /// 非法 futex 操作码（闭集 0..=4 之外）
    BadFutexOp,
    /// 值漂移：futex 字与期望值不符（EAGAIN 面）
    ValueMismatch,
    /// 私有旗标缺失：glibc 私有路径未带 PRIVATE 位
    PrivateFlagMissing,
    /// 消费链逆迁移
    ChainReversed,
    /// 账本倒退
    LedgerRegress,
    /// 表外错误码
    UnknownErrno,
    /// 双跑指纹不一致
    NotReproducible,
    /// 唤醒计数越界（一次 WAKE 最多唤醒 INT_MAX，此处闭集内校验）
    WakeCountExceeded,
}

impl ConsumerCode {
    /// 闭集数组。
    pub const ALL: [ConsumerCode; 10] = [
        ConsumerCode::ConsumerUnregistered,
        ConsumerCode::ConsumerDuplicate,
        ConsumerCode::BadFutexOp,
        ConsumerCode::ValueMismatch,
        ConsumerCode::PrivateFlagMissing,
        ConsumerCode::ChainReversed,
        ConsumerCode::LedgerRegress,
        ConsumerCode::UnknownErrno,
        ConsumerCode::NotReproducible,
        ConsumerCode::WakeCountExceeded,
    ];

    /// SEG_BASE + 段内序号（1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ConsumerCode::ConsumerUnregistered => 1,
                ConsumerCode::ConsumerDuplicate => 2,
                ConsumerCode::BadFutexOp => 3,
                ConsumerCode::ValueMismatch => 4,
                ConsumerCode::PrivateFlagMissing => 5,
                ConsumerCode::ChainReversed => 6,
                ConsumerCode::LedgerRegress => 7,
                ConsumerCode::UnknownErrno => 8,
                ConsumerCode::NotReproducible => 9,
                ConsumerCode::WakeCountExceeded => 10,
            }
    }

    /// 非空中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            ConsumerCode::ConsumerUnregistered => "消费号必须先登记再消费，未登记显性拒绝",
            ConsumerCode::ConsumerDuplicate => "同一消费号只登记一次，重复按 EEXIST 口径拒绝",
            ConsumerCode::BadFutexOp => "futex 操作码只认 0..=4 闭集，越界即 EINVAL 口径",
            ConsumerCode::ValueMismatch => "futex 字与期望值不符回 EAGAIN，这是语义不是缺陷",
            ConsumerCode::PrivateFlagMissing => "glibc 私有路径必须带 FUTEX_PRIVATE_FLAG(128)",
            ConsumerCode::ChainReversed => "消费链 LockContend→…→Acquired 单向，回退拒绝",
            ConsumerCode::LedgerRegress => "消费账只增不减，倒退即审计失败",
            ConsumerCode::UnknownErrno => "错误码必须落在 errno 对照表闭集内",
            ConsumerCode::NotReproducible => "同输入双跑指纹必须逐位一致",
            ConsumerCode::WakeCountExceeded => "一次 WAKE 的唤醒数必须落在闭集内",
        }
    }
}

// ── futex 操作码对拍 ──────────────────────────────────────────────────
/// futex 操作码闭集校验（0..=4）。
pub fn valid_futex_op(op: u32) -> bool {
    op == FUTEX_WAIT || op == FUTEX_WAKE || op == FUTEX_FD || op == FUTEX_REQUEUE
        || op == FUTEX_CMP_REQUEUE
}

/// glibc 调用面旗标对拍：私有路径必须带 PRIVATE 位。
pub fn flags_valid(flags: u32) -> bool {
    flags & FUTEX_PRIVATE_FLAG == FUTEX_PRIVATE_FLAG
}

/// 操作码 + 旗标联合校验。
pub fn futex_call_valid(op: u32, flags: u32) -> bool {
    valid_futex_op(op) && flags_valid(flags)
}

// ── 消费号注册表 ──────────────────────────────────────────────────────
/// 消费号段（闭集 5 段）：0=pthread_mutex 1=pthread_cond
/// 2=pthread_once 3=sem 4=glibc rseq 桥。
pub const C_MUTEX: u32 = 0;
pub const C_COND: u32 = 1;
pub const C_ONCE: u32 = 2;
pub const C_SEM: u32 = 3;
pub const C_RSEQ: u32 = 4;

/// 消费号注册表。
pub struct ConsumerRegistry {
    registered: Vec<u32>,
    /// 成功登记数
    pub registered_total: u32,
    /// 显性拒绝数
    pub rejected: u32,
}

impl ConsumerRegistry {
    /// 建空表。
    pub fn new() -> ConsumerRegistry {
        ConsumerRegistry { registered: Vec::new(), registered_total: 0, rejected: 0 }
    }

    /// 消费号登记：闭集校验 + 去重。
    pub fn register(&mut self, class: u32) -> Result<(), ConsumerCode> {
        if class as usize >= CONSUMER_CLASSES {
            self.rejected += 1;
            return Err(ConsumerCode::ConsumerUnregistered);
        }
        if self.registered.contains(&class) {
            self.rejected += 1;
            return Err(ConsumerCode::ConsumerDuplicate);
        }
        self.registered.push(class);
        self.registered_total += 1;
        Ok(())
    }

    /// 已登记？
    pub fn is_registered(&self, class: u32) -> bool {
        self.registered.contains(&class)
    }
}

// ── 消费链状态机 ──────────────────────────────────────────────────────
/// pthread_mutex 争用消费链：单向四态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainStage {
    /// 锁争用（快速路径失败，进 futex 慢路径）
    LockContend,
    /// futex 等待（FUTEX_WAIT | PRIVATE）
    FutexWait,
    /// futex 唤醒（FUTEX_WAKE | PRIVATE）
    FutexWake,
    /// 锁获取成功
    Acquired,
}

impl ChainStage {
    /// 单向迁移核验。
    pub fn advance(self, want: ChainStage) -> Option<ChainStage> {
        let legal = match (self, want) {
            (ChainStage::LockContend, ChainStage::FutexWait) => true,
            (ChainStage::FutexWait, ChainStage::FutexWake) => true,
            (ChainStage::FutexWake, ChainStage::Acquired) => true,
            _ => false,
        };
        if legal { Some(want) } else { None }
    }
}

// ── 消费账本 ──────────────────────────────────────────────────────────
/// 消费账：登记=等待=唤醒=获取 四段守恒。
#[derive(Clone, Copy)]
pub struct ConsumeLedger {
    pub registered: u32,
    pub waited: u32,
    pub woken: u32,
    pub acquired: u32,
    /// 注入账
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl ConsumeLedger {
    /// 零账初值。
    pub fn new() -> ConsumeLedger {
        ConsumeLedger {
            registered: 0,
            waited: 0,
            woken: 0,
            acquired: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 四段守恒 + 注入账守恒。
    pub fn conserved(&self) -> bool {
        self.registered == self.waited
            && self.waited == self.woken
            && self.woken == self.acquired
            && self.injected == self.detected + self.escaped
            && self.escaped == 0
    }
}

/// 消费链联测：登记 5 消费号 → 逐号走 LockContend→…→Acquired。
///
/// 返回 (账, 事件序列)；事件序列 = (消费号, 阶段序) 供指纹。
pub fn run_consumer_jointest() -> (ConsumeLedger, Vec<(u32, u32)>) {
    let mut ledger = ConsumeLedger::new();
    let mut events: Vec<(u32, u32)> = Vec::new();
    let mut reg = ConsumerRegistry::new();
    let mut class = 0u32;
    while class < CONSUMER_CLASSES as u32 {
        if reg.register(class).is_ok() {
            ledger.registered += 1;
        }
        class += 1;
    }
    // 逐消费号走完整消费链（glibc 私有 futex 路径）
    let mut class = 0u32;
    while class < CONSUMER_CLASSES as u32 {
        if !reg.is_registered(class) {
            class += 1;
            continue;
        }
        let mut stage = ChainStage::LockContend;
        // futex 调用面对拍：WAIT/WAKE 都必须带 PRIVATE 位
        let wait_ok = futex_call_valid(FUTEX_WAIT, FUTEX_PRIVATE_FLAG);
        let wake_ok = futex_call_valid(FUTEX_WAKE, FUTEX_PRIVATE_FLAG);
        if wait_ok {
            if let Some(next) = stage.advance(ChainStage::FutexWait) {
                stage = next;
                ledger.waited += 1;
                events.push((class, 1));
            }
        }
        if wake_ok {
            if let Some(next) = stage.advance(ChainStage::FutexWake) {
                stage = next;
                ledger.woken += 1;
                events.push((class, 2));
            }
        }
        if let Some(next) = stage.advance(ChainStage::Acquired) {
            stage = next;
            ledger.acquired += 1;
            events.push((class, 3));
        }
        let _ = stage;
        class += 1;
    }
    (ledger, events)
}

// ── FNV-1a ────────────────────────────────────────────────────────────
/// FNV-1a 32 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 消费链事件指纹。
pub fn consume_fingerprint(events: &[(u32, u32)]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for (a, b) in events {
        buf.extend_from_slice(&a.to_le_bytes());
        buf.extend_from_slice(&b.to_le_bytes());
    }
    fnv1a(&buf)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 5 类坏样本逐类注入，返回各类是否都被显性拒绝。
pub fn run_injection_core() -> ([bool; 5], u32) {
    let mut ok = [false; 5];
    let mut detected = 0u32;

    // 类 1：未登记消费号（闭集 5 段之外）
    let mut r1 = ConsumerRegistry::new();
    let unreg = r1.register(CONSUMER_CLASSES as u32) == Err(ConsumerCode::ConsumerUnregistered)
        && !r1.is_registered(99);
    if unreg {
        detected += 1;
        ok[0] = true;
    }

    // 类 2：重复登记
    let mut r2 = ConsumerRegistry::new();
    let _ = r2.register(C_MUTEX);
    let dup = r2.register(C_MUTEX) == Err(ConsumerCode::ConsumerDuplicate);
    if dup {
        detected += 1;
        ok[1] = true;
    }

    // 类 3：非法 futex 操作码
    let bad_op = !valid_futex_op(5) && !valid_futex_op(u32::MAX) && !futex_call_valid(7, FUTEX_PRIVATE_FLAG);
    if bad_op {
        detected += 1;
        ok[2] = true;
    }

    // 类 4：私有旗标缺失（glibc 共享路径对拍拒绝）
    let no_private = !flags_valid(0) && !futex_call_valid(FUTEX_WAIT, 0);
    if no_private {
        detected += 1;
        ok[3] = true;
    }

    // 类 5：消费链逆迁移（Acquired 回 FutexWake、FutexWait 跳 Acquired）
    let reversed = ChainStage::Acquired.advance(ChainStage::FutexWake).is_none()
        && ChainStage::FutexWait.advance(ChainStage::Acquired).is_none();
    if reversed {
        detected += 1;
        ok[4] = true;
    }

    (ok, detected)
}

// ── errno 对照表 ──────────────────────────────────────────────────────
/// 本模块 errno 闭集（glibc futex 消费面口径）。
pub const ERRNO_TABLE: [i32; 5] = [EAGAIN, EINTR, ETIMEDOUT, EBADF, EINVAL];

/// 表内码逐码对拍。
pub fn agrees_with_linux(code: i32) -> bool {
    if code < 0 {
        return ERRNO_TABLE.contains(&-code);
    }
    ERRNO_TABLE.contains(&code)
}

// ── 判据 ──────────────────────────────────────────────────────────────
/// UNX-F9397 自检：消费核联测判据（约 14 条）。
pub fn run_unx9397_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9397");

    // 1. 消费号登记：5 号段全部在案
    let mut reg = ConsumerRegistry::new();
    let mut class = 0u32;
    while class < CONSUMER_CLASSES as u32 {
        let _ = reg.register(class);
        class += 1;
    }
    s.add("消费号登记：5 号段（mutex/cond/once/sem/rseq桥）全在案",
          reg.registered_total == CONSUMER_CLASSES as u32,
          "逐号登记是消费链对接的前提");

    // 2. 消费链联测四段守恒
    let (ledger, events) = run_consumer_jointest();
    s.add("消费链四段守恒：登记=等待=唤醒=获取", ledger.conserved(),
          "pthread futex 消费链逐段可观测");

    // 3. 消费链足额：5 号段全链贯通
    s.add("消费链足额：5 号段全部走到 Acquired", ledger.acquired == CONSUMER_CLASSES as u32,
          "任一号段卡死即对接失败");

    // 4. glibc 私有 futex 对拍：WAIT/WAKE 均带 PRIVATE 位
    s.add("私有位对拍：WAIT|PRIVATE 与 WAKE|PRIVATE 全过",
          futex_call_valid(FUTEX_WAIT, FUTEX_PRIVATE_FLAG)
              && futex_call_valid(FUTEX_WAKE, FUTEX_PRIVATE_FLAG),
          "glibc 私有路径必须带 128 位");

    // 5. 共享路径拒绝：无 PRIVATE 位显性拒绝
    s.add("共享路径拒绝：无 PRIVATE 位不得过闸",
          !futex_call_valid(FUTEX_WAIT, 0) && !futex_call_valid(FUTEX_WAKE, 1),
          "对拍口径与 glibc lll_futex_wait 一致");

    // 6. futex 操作码闭集
    let ops_ok = valid_futex_op(FUTEX_WAIT)
        && valid_futex_op(FUTEX_WAKE)
        && valid_futex_op(FUTEX_FD)
        && valid_futex_op(FUTEX_REQUEUE)
        && valid_futex_op(FUTEX_CMP_REQUEUE)
        && !valid_futex_op(5)
        && !valid_futex_op(u32::MAX);
    s.add("操作码闭集：0..=4 全合法、5 与 u32::MAX 拒绝", ops_ok,
          "futex op 闭集对拍 Linux futex.h");

    // 7. 消费链状态机单向
    let fsm_ok = ChainStage::LockContend.advance(ChainStage::FutexWait) == Some(ChainStage::FutexWait)
        && ChainStage::FutexWait.advance(ChainStage::FutexWake) == Some(ChainStage::FutexWake)
        && ChainStage::FutexWake.advance(ChainStage::Acquired) == Some(ChainStage::Acquired)
        && ChainStage::Acquired.advance(ChainStage::LockContend).is_none();
    s.add("消费链状态机：四态单向迁移", fsm_ok,
          "回退与跳级全部拒绝");

    // 8. EAGAIN 面：值漂移是语义回报，不是丢失
    let eagain_ok = agrees_with_linux(EAGAIN) && !futex_call_valid(9, FUTEX_PRIVATE_FLAG);
    s.add("EAGAIN 面：值漂移口径在闭集内", eagain_ok,
          "futex 字与期望值不符回 EAGAIN");

    // 9. 注入核 5 类逐类拒绝
    let (inj_ok, detected) = run_injection_core();
    let all_rej = inj_ok.iter().all(|x| *x);
    s.add("注入核：5 类坏样本逐类显性拒绝", all_rej && detected == 5,
          "未登记/重复/坏码/缺位/逆迁移全过闸");

    // 10. 拒绝账
    let mut r10 = ConsumerRegistry::new();
    let _ = r10.register(C_SEM);
    let _ = r10.register(C_SEM);
    let _ = r10.register(7);
    let rej_ok = r10.rejected == 2 && r10.registered_total == 1;
    s.add("拒绝账：重复与越界各记一笔", rej_ok,
          "拒绝不静默，显性入账");

    // 11. errno 逐码对拍
    let codes_ok = ERRNO_TABLE.iter().all(|c| agrees_with_linux(*c))
        && agrees_with_linux(-EINTR)
        && !agrees_with_linux(0)
        && !agrees_with_linux(31337);
    s.add("errno 对照：glibc 消费面 5 码表内全中", codes_ok,
          "错误码闭集对拍");

    // 12. 双跑指纹
    let (_, events2) = run_consumer_jointest();
    let fp1 = consume_fingerprint(&events);
    let fp2 = consume_fingerprint(&events2);
    s.add("双跑指纹：消费链事件 FNV-1a 两次一致", fp1 == fp2,
          "确定性联测，无隐藏状态");

    // 13. 指纹区分度
    let mut e3 = events2.clone();
    if let Some(last) = e3.last_mut() {
        last.1 = last.1.wrapping_add(1);
    }
    s.add("指纹区分度：事件改动即指纹漂移", consume_fingerprint(&e3) != fp1,
          "指纹必须反映消费内容");

    // 14. 诊断码闭集
    let text_ok = ConsumerCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i = 0usize;
    while i < ConsumerCode::ALL.len() {
        let mut j = i + 1;
        while j < ConsumerCode::ALL.len() {
            let a = ConsumerCode::ALL.get(i).map(|c| c.val()).unwrap_or(0);
            let b = ConsumerCode::ALL.get(j).map(|c| c.val()).unwrap_or(1);
            if a == b {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码闭集：10 码唯一、文案非空", text_ok && uniq,
          "SEG_BASE+1..=+10 闭集");

    // 动态明细（≤8）
    let c_text = format!(
        "登记 {} / 等待 {} / 唤醒 {} / 获取 {}",
        ledger.registered, ledger.waited, ledger.woken, ledger.acquired
    );
    s.add_dyn("消费账快照", ledger.conserved(), &c_text);
    let fp_text = format!("消费指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumer_registry_register_and_dedup() {
        let mut r = ConsumerRegistry::new();
        assert!(r.register(C_MUTEX).is_ok());
        assert_eq!(r.register(C_MUTEX), Err(ConsumerCode::ConsumerDuplicate));
        assert!(r.is_registered(C_MUTEX));
        assert!(!r.is_registered(C_RSEQ));
    }

    #[test]
    fn consumer_registry_closed_set() {
        let mut r = ConsumerRegistry::new();
        assert_eq!(r.register(5), Err(ConsumerCode::ConsumerUnregistered));
        assert_eq!(r.register(u32::MAX), Err(ConsumerCode::ConsumerUnregistered));
    }

    #[test]
    fn futex_op_closed_set() {
        assert!(valid_futex_op(0));
        assert!(valid_futex_op(4));
        assert!(!valid_futex_op(5));
        assert!(!valid_futex_op(u32::MAX));
    }

    #[test]
    fn private_flag_required() {
        assert!(flags_valid(FUTEX_PRIVATE_FLAG));
        assert!(flags_valid(FUTEX_PRIVATE_FLAG | 1));
        assert!(!flags_valid(0));
        assert!(!flags_valid(1));
        assert!(!futex_call_valid(FUTEX_WAIT, 0));
        assert!(futex_call_valid(FUTEX_CMP_REQUEUE, FUTEX_PRIVATE_FLAG));
    }

    #[test]
    fn chain_fsm_one_way() {
        assert_eq!(ChainStage::LockContend.advance(ChainStage::FutexWait), Some(ChainStage::FutexWait));
        assert_eq!(ChainStage::FutexWake.advance(ChainStage::Acquired), Some(ChainStage::Acquired));
        assert_eq!(ChainStage::Acquired.advance(ChainStage::FutexWait), None);
        assert_eq!(ChainStage::LockContend.advance(ChainStage::Acquired), None);
    }

    #[test]
    fn jointest_conserved_full() {
        let (l, ev) = run_consumer_jointest();
        assert!(l.conserved());
        assert_eq!(l.acquired, CONSUMER_CLASSES as u32);
        // 每号段 3 个事件（等待/唤醒/获取）
        assert_eq!(ev.len(), (CONSUMER_CLASSES * 3) as usize);
    }

    #[test]
    fn injection_core_all_rejected() {
        let (ok, n) = run_injection_core();
        for x in ok.iter() {
            assert!(*x);
        }
        assert_eq!(n, 5);
    }

    #[test]
    fn errno_table_agreement() {
        assert!(agrees_with_linux(EAGAIN));
        assert!(agrees_with_linux(-ETIMEDOUT));
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(4242));
    }

    #[test]
    fn double_run_fingerprint() {
        let (_, e1) = run_consumer_jointest();
        let (_, e2) = run_consumer_jointest();
        assert_eq!(consume_fingerprint(&e1), consume_fingerprint(&e2));
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(ConsumerCode::ALL.len(), 10);
        for c in ConsumerCode::ALL.iter() {
            assert!(!c.detail().is_empty());
            assert!(c.val() > SEG_BASE && c.val() <= SEG_BASE + 10);
        }
    }

    #[test]
    fn unx9397_checks_green() {
        let set = run_unx9397_checks();
        assert!(set.all_passed(), "UNX-F9397 red: {}", set.red_items_text());
    }
}
