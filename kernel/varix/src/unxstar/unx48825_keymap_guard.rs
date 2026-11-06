//! UNX-F48825 LED 写失败降级（降级记账不阻塞键事件） · 0x70300
//!
//! 覆盖卡：UNX-F48825（注入写失败 ×10 键事件零阻塞且降级账可查）、
//! UNX-F48826（dead key ×40 全对）、UNX-F48827（AltGr ×10^2 组合零漂移）。
//!
//! ── 闸门次序 ──
//! 本模块是 B02 唯一的「失败路径判死点」，闸门按下列次序串行：
//!   1) **键事件闸**：先产出 keysym（键事件本体），再谈 LED。次序反了会
//!      发生什么：先写 LED 并把写失败上抛，一个坏 LED 就把整条按键路径拖红，
//!      用户「按 CapsLock 没反应」——真因是背光芯片挂了，但键事件被吞。
//!   2) **LED 写闸**：写失败 → 记降级账（原因/纪元/掩码/第几次尝试）→ 退避
//!      重试（1/2/4 拍，预算 3 次）→ 预算耗尽后置「LED 停机」，此后不再
//!      写（skipped 计数），但**键事件照旧放行**。
//!   3) **dead key 闸**：死键必须先在表内登记（40 组冻结）；未登记的死键
//!      一律拒。跳过它，未知死键会进入 pending，把下一个正常字符吞掉。
//!   4) **组合闸**：pending 死键 + 基字符查表；查不到就「吐死键的占位符 +
//!      原样放行基字符」（FlushThen），绝不吞字符（吞字符是死键最经典的
//!      丢字事故）。
//!   5) **AltGr 闸**：先判第三层是否启用（lv3 option），再按「Caps 不影响
//!      AltGr 层」定层。若 Caps 也参与 AltGr 层，欧文键盘 AltGr+Shift 会与
//!      AltGr 撞层，第三层字符全错。
//!
//! ── 关键差异 ──
//! 与 meta（0x702C0）的边界：meta 假设 LED 写必成功（`press` 直接返回期望
//! 掩码），三把锁键的三态在那里判死；本模块接管「写不成功」的世界——重试、
//! 退避、停机、降级账全在这里，并且**只降级不阻塞**。与 core（0x70280）的
//! 边界：core 判「三层配置下取哪一层」，本模块判「第三层被 Caps/Ctrl 干扰
//! 时终态是否仍稳定」。dead key 与 AltGr 的字符数据都在本模块，core 只认
//! 符号名（含死键名），不解释组合结果。
//!
//! 判据原文锚定：`注入写失败 ×10 键事件零阻塞且降级账可查` /
//! `dead key ×40 全对` / `AltGr ×10^2 组合零漂移`。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（PLAN.md B02 分配，禁止自改）。
pub const SEG_BASE: u32 = 0x70300;

/// 降级账容量（定容，满了显式记 dropped，不静默吞）。
pub const LEDGER_CAP: usize = 16;

/// 重试预算（连续失败到这个数就停机）。
pub const RETRY_BUDGET: u8 = 3;

/// 退避拍数（1/2/4，见 `backoff_ticks`）。
pub const BACKOFF_STEPS: u8 = 3;

/// dead key 组数（卡面 ×40 冻结）。
pub const DEAD_GROUPS: usize = 40;

/// AltGr 组合判据轮数（卡面 ×10^2）。
pub const ALTGR_COMBOS: u32 = 100;

/// LED 写失败注入的键事件数（卡面 ×10）。
pub const FAULT_EVENTS: u32 = 10;

/// CapsLock keysym（锁键事件本体，LED 失败也必须送出）。
pub const KSYM_CAPS_LOCK: u32 = 0xffe5;
/// NumLock keysym。
pub const KSYM_NUM_LOCK: u32 = 0xff7f;
/// ScrollLock keysym。
pub const KSYM_SCROLL_LOCK: u32 = 0xff14;

// ── 选择字（AltGr 判据用；与 core 的选择字位序一致但含义只在本模块） ──
/// Shift。
pub const A_SHIFT: u8 = 0x01;
/// CapsLock。
pub const A_LOCK: u8 = 0x02;
/// AltGr。
pub const A_ALTGR: u8 = 0x04;
/// Control。
pub const A_CTRL: u8 = 0x08;

// ── 诊断码闭集（12 条） ────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuardCode {
    /// LED 写失败（单次，可重试）
    LedWriteFailed,
    /// 重试预算耗尽，LED 停机
    RetryBudgetExhausted,
    /// LED 已停机仍被要求写（只记账不阻塞）
    LedDisabled,
    /// 降级账满（溢出显式记 dropped）
    LedgerFull,
    /// 键事件被 LED 失败阻塞（红线，必须恒 0）
    EventBlocked,
    /// 未登记的死键
    DeadUnbound,
    /// 死键表组数不足 40
    DeadTableShort,
    /// 死键组合查不到表项（走 FlushThen，不算错，但需显式记账）
    DeadComposeMiss,
    /// lv3 未启用却要求第三层
    AltGrDisabled,
    /// 第三层为空符号
    AltGrLevelEmpty,
    /// AltGr 层不接受 Ctrl 组合
    CtrlOnAltGr,
    /// 双跑漂移
    NotReproducible,
}

impl GuardCode {
    /// 段内绝对码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文逐条文案。
    pub fn detail(self) -> &'static str {
        match self {
            GuardCode::LedWriteFailed => "LED 写失败：单次可重试，键事件不阻塞",
            GuardCode::RetryBudgetExhausted => "重试预算耗尽：LED 停机，只降级不阻塞",
            GuardCode::LedDisabled => "LED 已停机仍写：只记账，不写总线",
            GuardCode::LedgerFull => "降级账满：溢出显式计 dropped",
            GuardCode::EventBlocked => "键事件被 LED 失败阻塞：红线，必须恒 0",
            GuardCode::DeadUnbound => "未登记的死键：不得进 pending 吞字",
            GuardCode::DeadTableShort => "死键表组数不足 40：判据载体缺项",
            GuardCode::DeadComposeMiss => "死键组合未命中：吐占位符后原样放行",
            GuardCode::AltGrDisabled => "lv3 未启用：AltGr 不得产出第三层",
            GuardCode::AltGrLevelEmpty => "第三层空符号：strict 取用报错",
            GuardCode::CtrlOnAltGr => "AltGr 层不接受 Ctrl 组合",
            GuardCode::NotReproducible => "双跑漂移：降级/死键/AltGr 三条链不幂等",
        }
    }

    /// 闭集全表。
    pub const ALL: [GuardCode; 12] = [
        GuardCode::LedWriteFailed,
        GuardCode::RetryBudgetExhausted,
        GuardCode::LedDisabled,
        GuardCode::LedgerFull,
        GuardCode::EventBlocked,
        GuardCode::DeadUnbound,
        GuardCode::DeadTableShort,
        GuardCode::DeadComposeMiss,
        GuardCode::AltGrDisabled,
        GuardCode::AltGrLevelEmpty,
        GuardCode::CtrlOnAltGr,
        GuardCode::NotReproducible,
    ];
}

/// 按绝对码值查文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < GuardCode::ALL.len() {
        if let Some(c) = GuardCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x70300 段降级闭集内"
}

// ── 基础设施：账本折叠 + 注入掷骰 ──────────────────────────────
/// 折叠一个账本字段（原因/纪元/尝试/掩码四元组）。
pub fn fold_row(h: u64, reason: u32, epoch: u32, attempt: u8, mask: u8) -> u64 {
    let mut acc = h ^ (reason as u64);
    acc = acc.wrapping_mul(0x0000_0100_0000_01B3);
    acc ^= (epoch as u64).rotate_left(17);
    acc = acc.wrapping_mul(0x0000_0100_0000_01B3);
    acc ^= ((attempt as u64) << 8) | (mask as u64);
    acc = acc.wrapping_mul(0x0000_0100_0000_01B3);
    acc
}

/// 折叠一个 keysym（死键组合与 AltGr 终态用）。
pub fn fold_ksym(h: u64, ks: u32) -> u64 {
    let mut acc = h;
    let b = ks.to_le_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            acc ^= *v as u64;
            acc = acc.wrapping_mul(0x0000_0100_0000_01B3);
        }
        i += 1;
    }
    acc
}

/// 故障掷骰（簇首种子写死，复跑一致）。
pub struct Fault {
    s: u32,
}

impl Fault {
    /// 固定种子。
    pub fn new(seed: u32) -> Fault {
        Fault { s: seed ^ 0xA5A5_5A5A }
    }

    /// 下一步。
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(2_262_903_037).wrapping_add(1);
        self.s ^= self.s >> 13;
        self.s
    }

    /// [0, bound)。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
}

/// 退避拍数：1/2/4（attempt 从 1 起）。
pub fn backoff_ticks(attempt: u8) -> u32 {
    let mut t = 1u32;
    let mut i = 1u8;
    while i < attempt && i < BACKOFF_STEPS {
        t = t.saturating_mul(2);
        i += 1;
    }
    t
}

// ── LED 总线与降级账 ───────────────────────────────────────────
/// 降级账一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerRow {
    /// 原因码。
    pub reason: u32,
    /// 总线纪元（第几次写）。
    pub epoch: u32,
    /// 第几次尝试（1 起）。
    pub attempt: u8,
    /// 目标掩码。
    pub mask: u8,
}

/// 定容降级账：满了显式 dropped，绝不静默吞。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradeLedger {
    /// 账目。
    pub rows: Vec<LedgerRow>,
    /// 溢出条数。
    pub dropped: u32,
    /// 按原因计数（写失败/预算耗尽/停机/查表未命中）。
    pub by_reason: [u32; 4],
}

impl DegradeLedger {
    /// 空账。
    pub fn new() -> DegradeLedger {
        DegradeLedger {
            rows: Vec::new(),
            dropped: 0,
            by_reason: [0; 4],
        }
    }

    /// 记一行；满则 dropped+1 并返回 Err(LedgerFull)（调用方不得因此阻塞键）。
    pub fn push(&mut self, reason: u32, epoch: u32, attempt: u8, mask: u8) -> Result<(), u32> {
        let slot = reason_index(reason);
        if let Some(n) = self.by_reason.get_mut(slot) {
            *n = n.saturating_add(1);
        }
        if self.rows.len() >= LEDGER_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(GuardCode::LedgerFull.val());
        }
        self.rows.push(LedgerRow { reason, epoch, attempt, mask });
        Ok(())
    }

    /// 账内某原因条数。
    pub fn count_of(&self, reason: u32) -> u32 {
        self.by_reason.get(reason_index(reason)).copied().unwrap_or(0)
    }

    /// 账本摘要指纹。
    pub fn digest(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for r in self.rows.iter() {
            h = fold_row(h, r.reason, r.epoch, r.attempt, r.mask);
        }
        h
    }

    /// 可查性：账非空且 pinned 行（写失败）可数。
    pub fn queryable(&self) -> bool {
        !self.rows.is_empty() && self.count_of(GuardCode::LedWriteFailed.val()) > 0
    }
}

/// 原因码 → 计数槽（四类）。
fn reason_index(reason: u32) -> usize {
    let base = SEG_BASE;
    if reason == base + GuardCode::LedWriteFailed as u32 {
        return 0;
    }
    if reason == base + GuardCode::RetryBudgetExhausted as u32 {
        return 1;
    }
    if reason == base + GuardCode::LedDisabled as u32 {
        return 2;
    }
    3
}

/// LED 总线模型（注入失败：前 `faults_left` 次写必失败）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedBus {
    /// 剩余故障次数。
    pub faults_left: u32,
    /// 写尝试次数。
    pub writes: u32,
    /// 成功次数。
    pub ok: u32,
    /// 总线纪元。
    pub epoch: u32,
    /// 最近一次成功写入的掩码。
    pub last_mask: u8,
}

impl LedBus {
    /// 健康总线。
    pub const fn healthy() -> LedBus {
        LedBus {
            faults_left: 0,
            writes: 0,
            ok: 0,
            epoch: 0,
            last_mask: 0,
        }
    }

    /// 注入 n 次写失败。
    pub const fn with_faults(n: u32) -> LedBus {
        LedBus {
            faults_left: n,
            writes: 0,
            ok: 0,
            epoch: 0,
            last_mask: 0,
        }
    }

    /// 写一次 LED 掩码。
    pub fn write(&mut self, mask: u8) -> Result<u32, u32> {
        self.writes = self.writes.saturating_add(1);
        self.epoch = self.epoch.saturating_add(1);
        if self.faults_left > 0 {
            self.faults_left -= 1;
            return Err(GuardCode::LedWriteFailed.val());
        }
        self.ok = self.ok.saturating_add(1);
        self.last_mask = mask;
        Ok(self.epoch)
    }
}

/// LED 面状态（对外可见的三态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedFace {
    /// 同步（写成功）
    Synced,
    /// 降级（写失败但未停机）
    Degraded,
    /// 停机（预算耗尽，不再写）
    Disabled,
}

/// 键事件种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvKind {
    /// 普通字符键（键码）
    Plain(u16),
    /// 锁键（LED 面）
    Lock(u8),
}

/// 一次投递的结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Delivered {
    /// 送出的 keysym（LED 失败也必须非零）
    pub keysym: u32,
    /// LED 面状态
    pub face: LedFace,
    /// 是否走了重试
    pub retried: u8,
}

/// 降级护栏：键事件先出，LED 后记账；LED 永不阻塞键事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardRail {
    /// 总线。
    pub bus: LedBus,
    /// 降级账。
    pub ledger: DegradeLedger,
    /// 期望 LED 掩码（状态面）。
    pub want: u8,
    /// 连续失败次数。
    pub streak: u8,
    /// 是否已停机。
    pub disabled: bool,
    /// 已服务键事件数。
    pub served: u32,
    /// 被 LED 失败阻塞的键事件数（红线：必须 0）。
    pub blocked: u32,
    /// 因停机被跳过的写次数。
    pub skipped: u32,
}

impl GuardRail {
    /// 健康起机。
    pub fn new(bus: LedBus) -> GuardRail {
        GuardRail {
            bus,
            ledger: DegradeLedger::new(),
            want: 0,
            streak: 0,
            disabled: false,
            served: 0,
            blocked: 0,
            skipped: 0,
        }
    }

    /// 带故障的起机（注入）。
    pub fn with_faults(n: u32) -> GuardRail {
        GuardRail::new(LedBus::with_faults(n))
    }

    /// 投递键事件：**先产 keysym，后谈 LED**。
    pub fn deliver(&mut self, ev: EvKind) -> Result<Delivered, u32> {
        let (keysym, lock_bit) = match ev {
            EvKind::Plain(_kc) => (0x1000_0000 | ((_kc as u32) & 0xffff), None),
            EvKind::Lock(id) => match id {
                0 => (KSYM_CAPS_LOCK, Some(0x01u8)),
                1 => (KSYM_NUM_LOCK, Some(0x02u8)),
                2 => (KSYM_SCROLL_LOCK, Some(0x04u8)),
                _ => return Err(GuardCode::EventBlocked.val()),
            },
        };
        self.served = self.served.saturating_add(1);
        let mut face = LedFace::Synced;
        let mut retried = 0u8;
        if let Some(bit) = lock_bit {
            self.want ^= bit;
            match self.flush_led() {
                Ok(n) => {
                    retried = n;
                    // 只要重试过（n>0）或已停机，LED 面就不是「同步」——
                    // 降级必须对外可见，不能报平安。
                    face = if n > 0 || self.disabled {
                        LedFace::Degraded
                    } else {
                        LedFace::Synced
                    };
                }
                Err(_) => {
                    // 关键纪律：这里绝不 return Err —— 键事件已产出。
                    face = if self.disabled {
                        LedFace::Disabled
                    } else {
                        LedFace::Degraded
                    };
                }
            }
        }
        if keysym == 0 {
            return Err(GuardCode::EventBlocked.val());
        }
        Ok(Delivered { keysym, face, retried })
    }

    /// 写 LED（含退避重试与停机）。返回重试次数。
    fn flush_led(&mut self) -> Result<u8, u32> {
        if self.disabled {
            self.skipped = self.skipped.saturating_add(1);
            let _ = self.ledger.push(
                GuardCode::LedDisabled.val(),
                self.bus.epoch,
                self.streak,
                self.want,
            );
            return Err(GuardCode::LedDisabled.val());
        }
        let mask = self.want;
        let mut attempt = 1u8;
        loop {
            match self.bus.write(mask) {
                Ok(_) => {
                    self.streak = 0;
                    return Ok(attempt.saturating_sub(1));
                }
                Err(code) => {
                    let _ = self.ledger.push(code, self.bus.epoch, attempt, mask);
                    self.streak = self.streak.saturating_add(1);
                    if self.streak >= RETRY_BUDGET {
                        self.disabled = true;
                        let _ = self.ledger.push(
                            GuardCode::RetryBudgetExhausted.val(),
                            self.bus.epoch,
                            attempt,
                            mask,
                        );
                        return Ok(attempt.saturating_sub(1));
                    }
                    // 退避拍数只用于账（本模块不真睡）。
                    let _ticks = backoff_ticks(attempt);
                    attempt = attempt.saturating_add(1);
                }
            }
        }
    }

    /// 键事件零阻塞判据：服务数 == 注入数 且 blocked == 0。
    pub fn zero_block(&self, expected: u32) -> bool {
        self.served == expected && self.blocked == 0
    }
}

// ── dead key 40 组（冻结） ─────────────────────────────────────
/// 一行死键组合：(死键 keysym, 基字符 keysym, 合成字符 keysym)。
pub type DeadRow = (u32, u32, u32);

/// 40 组死键（7 个死键族：acute/grave/circumflex/tilde/diaeresis/cedilla/caron）。
pub const DEAD_TABLE: [DeadRow; DEAD_GROUPS] = [
    // dead_acute ×8
    (0xfe51, 0x0061, 0x00e1),
    (0xfe51, 0x0065, 0x00e9),
    (0xfe51, 0x0069, 0x00ed),
    (0xfe51, 0x006f, 0x00f3),
    (0xfe51, 0x0075, 0x00fa),
    (0xfe51, 0x0079, 0x00fd),
    (0xfe51, 0x0063, 0x0107),
    (0xfe51, 0x006e, 0x0144),
    // dead_grave ×6
    (0xfe50, 0x0061, 0x00e0),
    (0xfe50, 0x0065, 0x00e8),
    (0xfe50, 0x0069, 0x00ec),
    (0xfe50, 0x006f, 0x00f2),
    (0xfe50, 0x0075, 0x00f9),
    (0xfe50, 0x0041, 0x00c0),
    // dead_circumflex ×6
    (0xfe52, 0x0061, 0x00e2),
    (0xfe52, 0x0065, 0x00ea),
    (0xfe52, 0x0069, 0x00ee),
    (0xfe52, 0x006f, 0x00f4),
    (0xfe52, 0x0075, 0x00fb),
    (0xfe52, 0x0041, 0x00c2),
    // dead_tilde ×6
    (0xfe53, 0x0061, 0x00e3),
    (0xfe53, 0x006e, 0x00f1),
    (0xfe53, 0x006f, 0x00f5),
    (0xfe53, 0x0041, 0x00c3),
    (0xfe53, 0x004e, 0x00d1),
    (0xfe53, 0x004f, 0x00d5),
    // dead_diaeresis ×6
    (0xfe57, 0x0061, 0x00e4),
    (0xfe57, 0x0065, 0x00eb),
    (0xfe57, 0x0069, 0x00ef),
    (0xfe57, 0x006f, 0x00f6),
    (0xfe57, 0x0075, 0x00fc),
    (0xfe57, 0x0079, 0x00ff),
    // dead_cedilla ×4
    (0xfe5b, 0x0063, 0x00e7),
    (0xfe5b, 0x0043, 0x00c7),
    (0xfe5b, 0x0073, 0x015f),
    (0xfe5b, 0x0053, 0x015e),
    // dead_caron ×4
    (0xfe5a, 0x0063, 0x010d),
    (0xfe5a, 0x0065, 0x011b),
    (0xfe5a, 0x0073, 0x0161),
    (0xfe5a, 0x007a, 0x017e),
];

/// 死键 keysym 区间（X11 死键键区 0xfe50..=0xfe6f）。
pub fn is_dead(ks: u32) -> bool {
    (0xfe50..=0xfe6f).contains(&ks)
}

/// 死键是否在表内（表内没有的死键必须拒，不得进 pending）。
pub fn dead_known(ks: u32) -> bool {
    let mut i = 0usize;
    while i < DEAD_TABLE.len() {
        if let Some(r) = DEAD_TABLE.get(i) {
            if r.0 == ks {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 死键组合查表。
pub fn compose(dead: u32, base: u32) -> Option<u32> {
    let mut i = 0usize;
    while i < DEAD_TABLE.len() {
        if let Some(r) = DEAD_TABLE.get(i) {
            if r.0 == dead && r.1 == base {
                return Some(r.2);
            }
        }
        i += 1;
    }
    None
}

/// 死键的占位符（组合失败时吐出，避免吞字）。
pub fn spacing_of(dead: u32) -> u32 {
    match dead {
        0xfe50 => 0x0060, // grave
        0xfe51 => 0x00b4, // acute
        0xfe52 => 0x005e, // circumflex
        0xfe53 => 0x007e, // tilde
        0xfe57 => 0x00a8, // diaeresis
        0xfe5b => 0x00b8, // cedilla
        0xfe5a => 0x02c7, // caron
        _ => 0,
    }
}

/// 死键状态机：pending 只挂一个死键。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeadState {
    /// 挂起的死键。
    pub pending: Option<u32>,
    /// 武装次数。
    pub armed: u32,
    /// 成功合成次数。
    pub composed: u32,
    /// 未命中冲刷次数。
    pub flushed: u32,
}

/// 一次喂入的产出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feed {
    /// 普通字符（原样放行）
    Single(u32),
    /// 合成成功
    Composed(u32),
    /// 死键武装（暂不产出）
    Armed(u32),
    /// 未命中：先吐占位符 amount，再放行基字符
    FlushThen { spacing: u32, base: u32 },
}

impl DeadState {
    /// 清空起机。
    pub const fn new() -> DeadState {
        DeadState {
            pending: None,
            armed: 0,
            composed: 0,
            flushed: 0,
        }
    }

    /// 喂入一个 keysym。
    pub fn feed(&mut self, ks: u32) -> Result<Feed, u32> {
        if is_dead(ks) {
            if !dead_known(ks) {
                return Err(GuardCode::DeadUnbound.val());
            }
            self.pending = Some(ks);
            self.armed = self.armed.saturating_add(1);
            return Ok(Feed::Armed(ks));
        }
        if let Some(d) = self.pending {
            self.pending = None;
            if let Some(c) = compose(d, ks) {
                self.composed = self.composed.saturating_add(1);
                return Ok(Feed::Composed(c));
            }
            self.flushed = self.flushed.saturating_add(1);
            return Ok(Feed::FlushThen {
                spacing: spacing_of(d),
                base: ks,
            });
        }
        Ok(Feed::Single(ks))
    }
}

/// 死键表长闸：声称组数与冻结点表必须一致（少一组即告警，不静默）。
pub fn dead_len_gate(claim: usize) -> Result<(), u32> {
    if claim != DEAD_GROUPS || DEAD_TABLE.len() != claim {
        return Err(GuardCode::DeadTableShort.val());
    }
    Ok(())
}

/// 40 组全对判据：逐行喂死键再喂基字符，必须得到表中合成字符。
pub fn dead_all_correct() -> Result<u32, u32> {
    if DEAD_TABLE.len() != DEAD_GROUPS {
        return Err(GuardCode::DeadTableShort.val());
    }
    let mut st = DeadState::new();
    let mut ok = 0u32;
    let mut i = 0usize;
    while i < DEAD_TABLE.len() {
        let row = match DEAD_TABLE.get(i) {
            Some(r) => *r,
            None => break,
        };
        if !dead_known(row.0) {
            return Err(GuardCode::DeadUnbound.val());
        }
        match st.feed(row.0) {
            Ok(Feed::Armed(d)) if d == row.0 => {}
            _ => return Err(GuardCode::DeadTableShort.val()),
        }
        match st.feed(row.1) {
            Ok(Feed::Composed(c)) if c == row.2 => ok = ok.saturating_add(1),
            _ => return Err(GuardCode::DeadComposeMiss.val()),
        }
        st.pending = None;
        i += 1;
    }
    if ok != DEAD_GROUPS as u32 {
        return Err(GuardCode::DeadTableShort.val());
    }
    Ok(ok)
}

// ── AltGr 第三层（20 键 × 5 选择 = 100 组合） ──────────────────
/// 单键四层（base/shift/altgr/altgr+shift）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AltRow {
    /// 键码（evdev+8）。
    pub kc: u16,
    /// 键名。
    pub name: &'static str,
    /// 四层 keysym。
    pub lv: [u32; 4],
}

/// 20 键（de 布局的数字行 + 主字母行）。
pub const ALT_TABLE: [AltRow; 20] = [
    AltRow { kc: 10, name: "AE01", lv: [0x0031, 0x0021, 0x00b9, 0x00a1] },
    AltRow { kc: 11, name: "AE02", lv: [0x0032, 0x0022, 0x00b2, 0x215b] },
    AltRow { kc: 12, name: "AE03", lv: [0x0033, 0x00a7, 0x00b3, 0x00a3] },
    AltRow { kc: 13, name: "AE04", lv: [0x0034, 0x0024, 0x00bc, 0x00a4] },
    AltRow { kc: 14, name: "AE05", lv: [0x0035, 0x0025, 0x00bd, 0x20ac] },
    AltRow { kc: 15, name: "AE06", lv: [0x0036, 0x0026, 0x00ac, 0x003f] },
    AltRow { kc: 16, name: "AE07", lv: [0x0037, 0x002f, 0x007b, 0x005c] },
    AltRow { kc: 17, name: "AE08", lv: [0x0038, 0x0028, 0x005b, 0x005d] },
    AltRow { kc: 18, name: "AE09", lv: [0x0039, 0x0029, 0x005d, 0x007d] },
    AltRow { kc: 19, name: "AE10", lv: [0x0030, 0x003d, 0x007d, 0x2260] },
    AltRow { kc: 24, name: "AD01", lv: [0x0071, 0x0051, 0x0040, 0x03a9] },
    AltRow { kc: 25, name: "AD02", lv: [0x0077, 0x0057, 0x2022, 0x00bf] },
    AltRow { kc: 26, name: "AD03", lv: [0x0065, 0x0045, 0x20ac, 0x0045] },
    AltRow { kc: 27, name: "AD04", lv: [0x0072, 0x0052, 0x00b6, 0x00ae] },
    AltRow { kc: 28, name: "AD05", lv: [0x0074, 0x0054, 0x2192, 0x2190] },
    AltRow { kc: 29, name: "AD06", lv: [0x007a, 0x005a, 0x2190, 0x2192] },
    AltRow { kc: 30, name: "AD07", lv: [0x0075, 0x0055, 0x00a6, 0x00a8] },
    AltRow { kc: 31, name: "AD08", lv: [0x0069, 0x0049, 0x00b8, 0x00b4] },
    AltRow { kc: 32, name: "AD09", lv: [0x006f, 0x004f, 0x00f8, 0x00d8] },
    AltRow { kc: 33, name: "AD10", lv: [0x0070, 0x0050, 0x00fe, 0x00de] },
];

/// AltGr 第三层解析器（lv3 开关 + Caps 不参与第三层）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AltGrLayer {
    /// lv3 是否启用（lv3:ralt_switch）。
    pub enabled: bool,
}

impl AltGrLayer {
    /// 按 option 位构造。
    pub fn from_opts(opts: u32) -> AltGrLayer {
        AltGrLayer { enabled: (opts & 0x01) != 0 }
    }

    /// 取层：返回 keysym。
    pub fn resolve(&self, kc: u16, sel: u8) -> Result<u32, u32> {
        let row = self.row(kc).ok_or(GuardCode::AltGrLevelEmpty.val())?;
        let altgr = (sel & A_ALTGR) != 0;
        let shift = (sel & A_SHIFT) != 0;
        let ctrl = (sel & A_CTRL) != 0;
        if ctrl && altgr {
            return Err(GuardCode::CtrlOnAltGr.val());
        }
        let lvl = if altgr {
            if !self.enabled {
                // 未启用 lv3：AltGr 不产第三层，退回 base/shift。
                if shift { 1 } else { 0 }
            } else if shift {
                3
            } else {
                2
            }
        } else if shift {
            1
        } else {
            0
        };
        // Caps 只作用于 base/shift 两层（0/1），对 2/3 层无影响。
        let mut idx = lvl;
        if lvl <= 1 && (sel & A_LOCK) != 0 {
            let base = row.lv.get(lvl).copied().unwrap_or(0);
            if is_letter(base) {
                idx = if lvl == 0 { 1 } else { 0 };
            }
        }
        let sym = row.lv.get(idx).copied().unwrap_or(0);
        if sym == 0 {
            return Err(GuardCode::AltGrLevelEmpty.val());
        }
        Ok(sym)
    }

    /// 键码 → 行。
    pub fn row(&self, kc: u16) -> Option<AltRow> {
        let mut i = 0usize;
        while i < ALT_TABLE.len() {
            if let Some(r) = ALT_TABLE.get(i) {
                if r.kc == kc {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }
}

/// 字母判定（只看 Latin-1 小写与 ASCII 小写）。
fn is_letter(ks: u32) -> bool {
    if (0x61..=0x7a).contains(&ks) {
        return true;
    }
    if (0x00e0..=0x00ff).contains(&ks) && ks != 0x00f7 {
        return true;
    }
    false
}

/// 100 组合判据：20 键 × 5 种选择字，逐项与「定层规则」对拍（两跑零漂移）。
pub fn altgr_100() -> Result<(u32, u64), u32> {
    let layer = AltGrLayer::from_opts(0x01);
    let sels = [A_ALTGR, A_ALTGR | A_SHIFT, A_ALTGR | A_LOCK, A_ALTGR | A_SHIFT | A_LOCK, A_CTRL | A_ALTGR];
    let mut n = 0u32;
    let mut h = 0x510E_527F_ADE6_82D1u64;
    let mut i = 0usize;
    while i < ALT_TABLE.len() {
        let row = match ALT_TABLE.get(i) {
            Some(r) => *r,
            None => break,
        };
        let mut s = 0usize;
        while s < sels.len() {
            let sel = match sels.get(s) {
                Some(v) => *v,
                None => break,
            };
            let got = layer.resolve(row.kc, sel);
            let want = rule_expect(&row, sel);
            match (got, want) {
                (Ok(g), Ok(w)) => {
                    if g != w {
                        return Err(GuardCode::NotReproducible.val());
                    }
                }
                (Err(a), Err(b)) => {
                    if a != b {
                        return Err(GuardCode::NotReproducible.val());
                    }
                }
                _ => return Err(GuardCode::NotReproducible.val()),
            }
            h = fold_ksym(h, got.unwrap_or(u32::MAX));
            n = n.saturating_add(1);
            s += 1;
        }
        i += 1;
    }
    if n != ALTGR_COMBOS {
        return Err(GuardCode::NotReproducible.val());
    }
    Ok((n, h))
}

/// 定层规则的独立表述（不复用 `resolve` 的分支）。
fn rule_expect(row: &AltRow, sel: u8) -> Result<u32, u32> {
    let altgr = (sel & A_ALTGR) != 0;
    let shift = (sel & A_SHIFT) != 0;
    let lock = (sel & A_LOCK) != 0;
    let ctrl = (sel & A_CTRL) != 0;
    if ctrl && altgr {
        return Err(GuardCode::CtrlOnAltGr.val());
    }
    if altgr {
        // 第三层：Caps 不参与；Shift 决定 2 或 3。
        let lvl = if shift { 3 } else { 2 };
        let v = row.lv.get(lvl).copied().unwrap_or(0);
        if v == 0 {
            return Err(GuardCode::AltGrLevelEmpty.val());
        }
        return Ok(v);
    }
    // base/shift：Caps 只翻字母。
    let mut idx = usize::from(shift);
    let cur = row.lv.get(idx).copied().unwrap_or(0);
    if lock && is_letter(cur) {
        idx ^= 1;
    }
    let v = row.lv.get(idx).copied().unwrap_or(0);
    if v == 0 {
        return Err(GuardCode::AltGrLevelEmpty.val());
    }
    Ok(v)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：40 死键 → 100 AltGr → 10 故障键事件零阻塞。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let dead = dead_all_correct()?;
    if dead != DEAD_GROUPS as u32 {
        return Err(GuardCode::DeadTableShort.val());
    }
    let (alt, _h) = altgr_100()?;
    if alt != ALTGR_COMBOS {
        return Err(GuardCode::NotReproducible.val());
    }
    let mut rail = GuardRail::with_faults(FAULT_EVENTS * 2);
    let mut delivered = 0u32;
    let mut i = 0u32;
    while i < FAULT_EVENTS {
        let ev = match i % 3 {
            0 => EvKind::Lock(0),
            1 => EvKind::Lock(1),
            _ => EvKind::Lock(2),
        };
        match rail.deliver(ev) {
            Ok(d) => {
                if d.keysym == 0 {
                    return Err(GuardCode::EventBlocked.val());
                }
                delivered = delivered.saturating_add(1);
            }
            Err(_) => return Err(GuardCode::EventBlocked.val()),
        }
        i += 1;
    }
    if !rail.zero_block(FAULT_EVENTS) {
        return Err(GuardCode::EventBlocked.val());
    }
    if !rail.ledger.queryable() {
        return Err(GuardCode::LedWriteFailed.val());
    }
    Ok((dead, alt, delivered))
}

/// 探针 a：死键 40 全对。
pub fn probe_seq_a() -> Result<(), u32> {
    match dead_all_correct() {
        Ok(40) => Ok(()),
        _ => Err(GuardCode::DeadComposeMiss.val()),
    }
}

/// 探针 b：未登记死键必拒（不得挂 pending）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut st = DeadState::new();
    match st.feed(0xfe6f) {
        Err(c) if c == GuardCode::DeadUnbound.val() && st.pending.is_none() => Ok(()),
        _ => Err(GuardCode::DeadUnbound.val()),
    }
}

/// 探针 c：AltGr 未启用时不产第三层。
pub fn probe_seq_c() -> Result<(), u32> {
    let off = AltGrLayer::from_opts(0x00);
    match off.resolve(10, A_ALTGR) {
        Ok(v) if v == 0x0031 => Ok(()),
        _ => Err(GuardCode::AltGrDisabled.val()),
    }
}

/// 注入账（十槽；want=0 表示该槽期望「正常放行」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    /// 期望码（0 = 期望成功）。
    pub want: [u32; 10],
    /// 判定次数。
    pub tries: [u32; 10],
    /// 命中次数。
    pub caught: [u32; 10],
}

impl InjectLedger {
    /// 建账。
    pub fn new() -> InjectLedger {
        InjectLedger {
            want: [
                0, // 槽 0：写失败只入账，键事件仍必须送出（期望成功）
                GuardCode::RetryBudgetExhausted.val(),
                GuardCode::LedDisabled.val(),
                GuardCode::LedgerFull.val(),
                0,
                GuardCode::DeadUnbound.val(),
                GuardCode::DeadTableShort.val(),
                GuardCode::DeadComposeMiss.val(),
                GuardCode::CtrlOnAltGr.val(),
                0,
            ],
            tries: [0; 10],
            caught: [0; 10],
        }
    }

    /// 记账（越界槽丢弃）。
    pub fn record(&mut self, slot: usize, got: u32) {
        if slot >= 10 {
            return;
        }
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
        }
        let want = self.want.get(slot).copied().unwrap_or(0);
        if got == want {
            if let Some(c) = self.caught.get_mut(slot) {
                *c += 1;
            }
        }
    }

    /// 全槽命中。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 10 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入十轮。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：单次写失败 → 账里出现 LedWriteFailed，事件仍送出。
        let mut rail = GuardRail::with_faults(1);
        let d0 = rail.deliver(EvKind::Lock(0));
        let got0 = match d0 {
            Ok(d) => {
                if d.keysym == KSYM_CAPS_LOCK
                    && rail.ledger.count_of(GuardCode::LedWriteFailed.val()) >= 1
                    && d.face != LedFace::Synced
                {
                    0
                } else {
                    d.face as u32
                }
            }
            Err(c) => c,
        };
        led.record(0, got0);
        // 槽 1：连续失败到预算 → 记预算耗尽。
        let mut rail1 = GuardRail::with_faults(32);
        let _ = rail1.deliver(EvKind::Lock(0));
        let hit1 = rail1
            .ledger
            .count_of(GuardCode::RetryBudgetExhausted.val())
            >= 1
            && rail1.disabled;
        led.record(
            1,
            if hit1 {
                GuardCode::RetryBudgetExhausted.val()
            } else {
                0
            },
        );
        // 槽 2：停机后仍要求写 → 记 LedDisabled 且 skipped 递增。
        let _ = rail1.deliver(EvKind::Lock(1));
        let hit2 = rail1.ledger.count_of(GuardCode::LedDisabled.val()) >= 1 && rail1.skipped >= 1;
        led.record(
            2,
            if hit2 {
                GuardCode::LedDisabled.val()
            } else {
                0
            },
        );
        // 槽 3：账满 → dropped 显式递增。
        let mut book = DegradeLedger::new();
        let mut i = 0u32;
        while i < (LEDGER_CAP as u32 + 3) {
            let _ = book.push(GuardCode::LedWriteFailed.val(), i, 1, 0x01);
            i += 1;
        }
        led.record(
            3,
            if book.dropped >= 3 {
                GuardCode::LedgerFull.val()
            } else {
                0
            },
        );
        // 槽 4：注入 10 键事件 → 零阻塞（期望成功 = 0）。
        let mut rail4 = GuardRail::with_faults(FAULT_EVENTS);
        let mut k = 0u32;
        let mut ok4 = true;
        while k < FAULT_EVENTS {
            match rail4.deliver(EvKind::Lock((k % 3) as u8)) {
                Ok(d) => {
                    if d.keysym == 0 {
                        ok4 = false;
                    }
                }
                Err(_) => ok4 = false,
            }
            k += 1;
        }
        ok4 = ok4 && rail4.zero_block(FAULT_EVENTS) && rail4.ledger.queryable();
        led.record(4, if ok4 { 0 } else { GuardCode::EventBlocked.val() });
        // 槽 5：未登记死键。
        let mut st = DeadState::new();
        let got5 = match st.feed(0xfe6f) {
            Err(c) => c,
            Ok(_) => 0,
        };
        led.record(5, got5);
        // 槽 6：死键表长闸——声称 39 组必须报 DeadTableShort，声称 40 组必须放行。
        let short_six = match dead_len_gate(DEAD_GROUPS - 1) {
            Err(c) => c == GuardCode::DeadTableShort.val(),
            Ok(_) => false,
        };
        let full_six = dead_len_gate(DEAD_GROUPS).is_ok();
        led.record(
            6,
            if short_six && full_six {
                GuardCode::DeadTableShort.val()
            } else {
                0
            },
        );
        // 槽 7：死键未命中 → FlushThen（吐占位符，不吞字）。
        let mut st7 = DeadState::new();
        let hit7 = match st7.feed(0xfe51) {
            Ok(Feed::Armed(_)) => match st7.feed(0x0031) {
                Ok(Feed::FlushThen { spacing, base }) => spacing == 0x00b4 && base == 0x0031 && st7.flushed == 1,
                _ => false,
            },
            _ => false,
        };
        led.record(
            7,
            if hit7 {
                GuardCode::DeadComposeMiss.val()
            } else {
                0
            },
        );
        // 槽 8：Ctrl+AltGr 必拒。
        let layer = AltGrLayer::from_opts(0x01);
        let got8 = match layer.resolve(24, A_CTRL | A_ALTGR) {
            Err(c) => c,
            Ok(_) => 0,
        };
        led.record(8, got8);
        // 槽 9：Caps 不影响第三层（AltGr+Lock 与 AltGr 同值）。
        let a = layer.resolve(24, A_ALTGR);
        let b = layer.resolve(24, A_ALTGR | A_LOCK);
        led.record(9, if a == b && a == Ok(0x0040) { 0 } else { GuardCode::NotReproducible.val() });

        r += 1;
    }
    Ok(led)
}

/// 双跑对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GuardCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < GuardCode::ALL.len() {
        if let Some(c) = GuardCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x20 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 九条判据。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // J1-1 十键事件注入写失败零阻塞 + 账可查。
    let mut rail = GuardRail::with_faults(FAULT_EVENTS);
    let mut syms_ok = true;
    let mut n = 0u32;
    while n < FAULT_EVENTS {
        match rail.deliver(EvKind::Lock((n % 3) as u8)) {
            Ok(d) => {
                if d.keysym != KSYM_CAPS_LOCK && d.keysym != KSYM_NUM_LOCK && d.keysym != KSYM_SCROLL_LOCK {
                    syms_ok = false;
                }
            }
            Err(_) => syms_ok = false,
        }
        n += 1;
    }
    v.push((
        "F48825-J1-十键事件零阻塞",
        rail.zero_block(FAULT_EVENTS) && syms_ok,
        format!("served={} blocked={}", rail.served, rail.blocked),
    ));

    // J1-2 降级账可查（原因分类 + 指纹非零）。
    v.push((
        "F48825-J1-降级账可查",
        rail.ledger.queryable() && rail.ledger.digest() != 0,
        format!(
            "rows={} drop={} fail={}",
            rail.ledger.rows.len(),
            rail.ledger.dropped,
            rail.ledger.count_of(GuardCode::LedWriteFailed.val())
        ),
    ));

    // J1-3 预算耗尽后停机，写被跳过但事件照发。
    let mut r3 = GuardRail::with_faults(64);
    let before = r3.served;
    let d3 = r3.deliver(EvKind::Lock(0));
    let d4 = r3.deliver(EvKind::Lock(2));
    let stop_ok = r3.disabled
        && r3.skipped >= 1
        && d3.is_ok()
        && d4.is_ok()
        && r3.served == before.saturating_add(2)
        && r3.blocked == 0;
    v.push((
        "F48825-J1-预算耗尽停机",
        stop_ok,
        format!(
            "disabled={} skipped={} streak={}",
            r3.disabled, r3.skipped, r3.streak
        ),
    ));

    // J1-4 退避拍数 1/2/4（单调、封顶）。
    let b1 = backoff_ticks(1);
    let b2 = backoff_ticks(2);
    let b3 = backoff_ticks(3);
    v.push((
        "F48825-J1-退避拍数",
        b1 == 1 && b2 == 2 && b3 == 4 && backoff_ticks(9) == 4,
        format!("{}/{}/{}", b1, b2, b3),
    ));

    // J1-5 dead key 40 全对。
    let dead = dead_all_correct();
    let dead_ok = matches!(&dead, Ok(40));
    v.push((
        "F48826-J1-dead四十组全对",
        dead_ok,
        match &dead {
            Ok(n) => format!("ok={}", n),
            Err(c) => format!("err=0x{:X}", c),
        },
    ));

    // J1-6 死键未命中不吞字（FlushThen 吐占位符 + 基字符）。
    let mut st = DeadState::new();
    let mut flush_ok = false;
    if let Ok(Feed::Armed(_)) = st.feed(0xfe57) {
        if let Ok(Feed::FlushThen { spacing, base }) = st.feed(0x0039) {
            flush_ok = spacing == 0x00a8 && base == 0x0039 && st.flushed == 1;
        }
    }
    v.push((
        "F48826-J1-未命中不吞字",
        flush_ok,
        format!("flushed={} pending={:?}", st.flushed, st.pending),
    ));

    // J1-7 AltGr 100 组合零漂移。
    let alt = altgr_100();
    let alt_ok = match &alt {
        Ok((n, h)) => *n == ALTGR_COMBOS && *h != 0,
        Err(_) => false,
    };
    v.push((
        "F48827-J1-AltGr百组合零漂移",
        alt_ok,
        match &alt {
            Ok((n, h)) => format!("n={} h={:016x}", n, h),
            Err(c) => format!("err=0x{:X}", c),
        },
    ));

    // J1-8 Caps 不参与第三层，但翻转 base 层字母。
    let layer = AltGrLayer::from_opts(0x01);
    let caps_ok = layer.resolve(24, A_ALTGR) == layer.resolve(24, A_ALTGR | A_LOCK)
        && layer.resolve(24, 0) == Ok(0x0071)
        && layer.resolve(24, A_LOCK) == Ok(0x0051)
        && layer.resolve(10, A_LOCK) == Ok(0x0031);
    v.push((
        "F48827-J1-Caps不参与第三层",
        caps_ok,
        format!(
            "altgr=0x{:04x} base=0x{:04x}",
            layer.resolve(24, A_ALTGR).unwrap_or(0),
            layer.resolve(24, 0).unwrap_or(0)
        ),
    ));

    // J2 探针三序。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_ok();
    let pc = probe_seq_c().is_ok();
    v.push((
        "F48825-J2-探针三序",
        pa && pb && pc,
        format!("a={} b={} c={}", pa, pb, pc),
    ));

    // J3 段位闭集。
    v.push(("F48825-J3-段位闭集", segment_guard(), String::new()));

    v
}

/// 本模块自检入口。
pub fn run_unx48825_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48825 LED 降级与 dead key/AltGr");
    for (n, p, d) in extra_checks() {
        if d.is_empty() {
            s.add(n, p, "");
        } else {
            s.add_dyn(n, p, d.as_str());
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_ledger_capacity() {
        let mut b = DegradeLedger::new();
        let mut i = 0u32;
        while i < LEDGER_CAP as u32 {
            assert!(b.push(GuardCode::LedWriteFailed.val(), i, 1, 0x01).is_ok());
            i += 1;
        }
        assert_eq!(
            b.push(GuardCode::LedWriteFailed.val(), 99, 1, 0x01),
            Err(GuardCode::LedgerFull.val())
        );
        assert_eq!(b.dropped, 1);
        assert_eq!(b.rows.len(), LEDGER_CAP);
        assert!(b.queryable());
        assert!(b.digest() != 0);
    }

    #[test]
    fn t_bus_faults_exhaust() {
        let mut bus = LedBus::with_faults(2);
        assert_eq!(bus.write(0x01), Err(GuardCode::LedWriteFailed.val()));
        assert_eq!(bus.write(0x01), Err(GuardCode::LedWriteFailed.val()));
        assert_eq!(bus.write(0x01), Ok(3));
        assert_eq!(bus.ok, 1);
        assert_eq!(bus.last_mask, 0x01);
        assert_eq!(bus.writes, 3);
    }

    #[test]
    fn t_zero_block_ten() {
        let mut rail = GuardRail::with_faults(FAULT_EVENTS);
        let mut i = 0u32;
        while i < FAULT_EVENTS {
            let d = rail.deliver(EvKind::Lock((i % 3) as u8)).expect("deliver");
            assert!(d.keysym != 0);
            i += 1;
        }
        assert!(rail.zero_block(FAULT_EVENTS));
        assert_eq!(rail.blocked, 0);
        assert!(rail.disabled);
        assert!(rail.ledger.count_of(GuardCode::RetryBudgetExhausted.val()) >= 1);
    }

    #[test]
    fn t_plain_event_has_no_led() {
        let mut rail = GuardRail::with_faults(8);
        let d = rail.deliver(EvKind::Plain(24)).expect("plain");
        assert_eq!(d.face, LedFace::Synced);
        assert_eq!(rail.bus.writes, 0);
        assert_eq!(d.keysym & 0xffff, 24);
    }

    #[test]
    fn t_dead_table_shape() {
        assert_eq!(DEAD_TABLE.len(), DEAD_GROUPS);
        let mut i = 0usize;
        while i < DEAD_TABLE.len() {
            let r = DEAD_TABLE.get(i).copied().expect("row");
            assert!(is_dead(r.0) && dead_known(r.0));
            assert!(spacing_of(r.0) != 0);
            assert!(compose(r.0, r.1) == Some(r.2));
            i += 1;
        }
        assert!(!dead_known(0xfe6f));
        assert_eq!(compose(0xfe51, 0x0039), None);
    }

    #[test]
    fn t_dead_compose_flow() {
        let mut st = DeadState::new();
        assert_eq!(st.feed(0xfe51), Ok(Feed::Armed(0xfe51)));
        assert_eq!(st.feed(0x0065), Ok(Feed::Composed(0x00e9)));
        assert_eq!(st.composed, 1);
        assert_eq!(st.pending, None);
        // 普通字符直通。
        assert_eq!(st.feed(0x0020), Ok(Feed::Single(0x0020)));
        // 未登记死键：拒，且不挂 pending。
        assert_eq!(st.feed(0xfe68), Err(GuardCode::DeadUnbound.val()));
        assert_eq!(st.pending, None);
    }

    #[test]
    fn t_altgr_levels() {
        let on = AltGrLayer::from_opts(0x01);
        assert_eq!(on.resolve(26, A_ALTGR), Ok(0x20ac));
        assert_eq!(on.resolve(26, A_ALTGR | A_SHIFT), Ok(0x0045));
        assert_eq!(on.resolve(26, 0), Ok(0x0065));
        assert_eq!(on.resolve(26, A_SHIFT), Ok(0x0045));
        assert_eq!(on.resolve(26, A_CTRL | A_ALTGR), Err(GuardCode::CtrlOnAltGr.val()));
        assert_eq!(on.resolve(26, A_CTRL), Ok(0x0065));
        assert_eq!(on.resolve(999, 0), Err(GuardCode::AltGrLevelEmpty.val()));
    }

    #[test]
    fn t_altgr_disabled() {
        let off = AltGrLayer::from_opts(0x00);
        assert_eq!(off.resolve(10, A_ALTGR), Ok(0x0031));
        assert_eq!(off.resolve(10, A_ALTGR | A_SHIFT), Ok(0x0021));
        assert_eq!(off.resolve(24, A_ALTGR | A_LOCK), Ok(0x0051));
    }

    #[test]
    fn t_altgr_100_combos() {
        let (n, h) = altgr_100().expect("altgr");
        assert_eq!(n, ALTGR_COMBOS);
        assert!(h != 0);
        assert_eq!(altgr_100().expect("again"), (n, h));
    }

    #[test]
    fn t_inject_ten_slots() {
        let led = inject_rounds(1).expect("inject");
        assert!(led.all_caught());
        for i in 0..10 {
            assert_eq!(led.tries[i], 1, "槽{}", i);
        }
    }

    #[test]
    fn t_golden_and_segment() {
        let g = golden_run().expect("golden");
        assert_eq!(g.0, DEAD_GROUPS as u32);
        assert_eq!(g.1, ALTGR_COMBOS);
        assert_eq!(g.2, FAULT_EVENTS);
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        for c in GuardCode::ALL.iter() {
            assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x20);
        }
        assert!(detail(0).contains("未知"));
        let v = extra_checks();
        assert!(v.len() >= 8 && v.len() <= 10);
        for (n, p, _) in v {
            assert!(p, "红项 {}", n);
        }
        assert!(run_unx48825_checks().all_passed());
    }
}
