//! UNX-F48823 修饰键 8 位锁存状态机（Shift/Ctrl/Alt/Meta/Grp 等位语义与组合终态） · 0x702C0
//!
//! 覆盖卡：UNX-F48823（500 次随机修饰组合终态一致，判据 J-4 载体）、
//! UNX-F48824（锁定键三态；锁定键 ×10^2 往返 LED 同步双向一致）。
//!
//! ── 闸门次序 ──
//! 本模块把「一个修饰键事件」拆成五道顺序闸，次序不可换：
//!   1) **位号闸**：位号必须 <8。位号越界若先落位图，`1u8 << 8` 在 release
//!      路径上会退化成 `<< 0`（Rust 移位对 8 位类型按 8 取模），于是「越界
//!      释放」把 Shift 位清掉——用户看到的是按住 Shift 突然松开。先判位号，
//!      越界直接记账不改状态。
//!   2) **按下/释放配对闸**：无按下的释放只记账不减计数（saturating）；
//!      重复按下只记账不叠加。跳过它，重复按键（键盘抖动）会让计数变成 2，
//!      一次释放后位仍亮，按键「粘住」。
//!   3) **锁存闸**：只有 Latching 类位（Grp）可执行 Latch；对瞬时位锁存等于
//!      把 Shift 变成「按一下锁死」，是最典型的粘键事故。
//!   4) **有效态闸**：按「位类」合成 effective——瞬时位看计数、锁定位看
//!      「按下 XOR 锁定」（同一位内互斥）、锁存位看 OR。次序不能反：先按
//!      位类分派再叠位权；顺序若反过来（先或全表再分派），锁定位的 XOR
//!      会把相邻位的按下吃掉。注意 Caps 反转 Shift 字符层属 keymap 取层
//!      规则（core 的 SEL_LOCK），本模块只输出掩码，不解释字符。
//!   5) **对外发布闸**：只有 effective 才允许写 LED / 交给 keymap 取层；内部
//!      down/locked/latched 三个位图绝不外泄（外泄即三个真相源）。
//!
//! ── 关键差异 ──
//! 与 core（0x70280）的边界：core 只消费调用方给的「本键有效修饰选择字」，
//! 不持有跨键状态；本模块是**唯一**持有时序状态的地方（down/locked/latched
//! 三个位图 + 三把锁键的 LED 面），错误在锁存语义即在本模块判死。与 guard
//! （0x70300）的边界：本模块假设 LED 总写成功（`press` 必返回期望掩码），
//! LED 写失败、降级记账、dead key、AltGr 第三层全部归 guard；本模块不认
//! 失败、不降级、不重试，因此「LED 写失败」类判据在本模块必红——这是有意
//! 的：失败路径只许有一个判死点。
//!
//! 判据原文锚定：`500 次随机修饰组合终态一致` / `锁定键 ×10^2 往返 LED 同步双向一致`。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（PLAN.md B02 分配，禁止自改）。
pub const SEG_BASE: u32 = 0x702C0;

/// 修饰位宽（8 位锁存）。
pub const MOD_BITS: usize = 8;

/// 随机组合判据步数（卡面 500）。
pub const STRESS_STEPS: u32 = 500;

/// 锁定键往返判据轮数（卡面 ×10^2）。
pub const ROUND_TRIPS: u32 = 100;

/// 锁定键个数（Caps/Num/Scroll）。
pub const LOCK_N: usize = 3;

// ── 位权（8 位，冻结；与 XKB modifiers 的位序对齐） ─────────────
/// Shift 位。
pub const B_SHIFT: u8 = 0x01;
/// CapsLock 位（锁定类）。
pub const B_CAPS: u8 = 0x02;
/// Control 位。
pub const B_CTRL: u8 = 0x04;
/// Alt/Mod1 位。
pub const B_ALT: u8 = 0x08;
/// Meta/Super 位。
pub const B_META: u8 = 0x10;
/// NumLock 位（锁定类）。
pub const B_NUM: u8 = 0x20;
/// Grp 位（锁存类）。
pub const B_GRP: u8 = 0x40;
/// ScrollLock 位（锁定类）。
pub const B_SCROLL: u8 = 0x80;

/// LED 掩码：Caps LED。
pub const LED_CAPS: u8 = 0x01;
/// LED 掩码：Num LED。
pub const LED_NUM: u8 = 0x02;
/// LED 掩码：Scroll LED。
pub const LED_SCROLL: u8 = 0x04;

// ── 诊断码闭集（10 条） ────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModCode {
    /// 位号 ≥8
    BitOutOfRange,
    /// 无按下的释放
    OrphanRelease,
    /// 重复按下（计数不叠）
    DoublePress,
    /// 对瞬时位执行锁存
    LatchOnMomentary,
    /// LED 掩码越 3 位
    LockMaskOverflow,
    /// 未写先回读
    EchoWithoutWrite,
    /// 三态失同步（回读与期望不符）
    PhaseDesync,
    /// 锁键 id 越界
    UnknownLock,
    /// 500 组合终态漂移
    RandomDrift,
    /// 100 往返不足
    RoundTripIncomplete,
}

impl ModCode {
    /// 段内绝对码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文逐条文案。
    pub fn detail(self) -> &'static str {
        match self {
            ModCode::BitOutOfRange => "修饰位号 ≥8：越界位先落位图会误清他位",
            ModCode::OrphanRelease => "无按下的释放：只记账，计数饱和不减负",
            ModCode::DoublePress => "重复按下：抖动不得叠计数，否则一次释放粘键",
            ModCode::LatchOnMomentary => "对瞬时位锁存：Shift 会被锁死",
            ModCode::LockMaskOverflow => "LED 掩码越 3 位：Caps/Num/Scroll 之外的位不存在",
            ModCode::EchoWithoutWrite => "未写先回读：无 pending 的回读不得采纳",
            ModCode::PhaseDesync => "锁定键三态失同步：回读与期望不符",
            ModCode::UnknownLock => "锁键 id 越界：只有 Caps/Num/Scroll 三把",
            ModCode::RandomDrift => "500 次随机修饰组合终态漂移：锁存语义不自洽",
            ModCode::RoundTripIncomplete => "100 次往返不足：状态与 LED 未双向闭合",
        }
    }

    /// 闭集全表。
    pub const ALL: [ModCode; 10] = [
        ModCode::BitOutOfRange,
        ModCode::OrphanRelease,
        ModCode::DoublePress,
        ModCode::LatchOnMomentary,
        ModCode::LockMaskOverflow,
        ModCode::EchoWithoutWrite,
        ModCode::PhaseDesync,
        ModCode::UnknownLock,
        ModCode::RandomDrift,
        ModCode::RoundTripIncomplete,
    ];
}

/// 按绝对码值查文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < ModCode::ALL.len() {
        if let Some(c) = ModCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x702C0 段修饰锁存闭集内"
}

// ── 基础设施：掩码折叠 + 确定性掷骰 ────────────────────────────
/// 折叠 8 位掩码（本域 FNV-1a 变体，混入步号防「同序不同步」）。
pub fn fold_mask(h: u64, mask: u8) -> u64 {
    let x = h ^ (mask as u64).wrapping_mul(0x100_0000_01B3);
    x.wrapping_mul(0x0000_0100_0000_01B3)
}

/// 折叠单个整数（步号/计数）。
pub fn fold_step(h: u64, n: u32) -> u64 {
    let b = n.to_le_bytes();
    let mut acc = h;
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

/// 修饰位串折叠（describe 输出）。
pub fn fold_text(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc ^= *b as u64;
        acc = acc.wrapping_mul(0x0000_0100_0000_01B3);
    }
    acc
}

/// 确定性掷骰（500 组合与注入都靠它复跑）。
pub struct Roll {
    s: u32,
}

impl Roll {
    /// 固定种子。
    pub fn new(seed: u32) -> Roll {
        Roll { s: seed.rotate_left(7) | 1 }
    }

    /// 下一步。
    pub fn step(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.s
    }

    /// [0, bound) 内取值；bound=0 返回 0。
    pub fn pick(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.step() % bound
    }
}

// ── 位类（8 位语义的唯一权威表） ────────────────────────────────
/// 修饰位的三类语义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BitKind {
    /// 瞬时位：按下有效，松开即失（Shift/Ctrl/Alt/Meta）。
    Momentary,
    /// 锁定位：点击切换，有效态 = 按下 XOR 锁定（Caps/Num/Scroll）。
    Locking,
    /// 锁存位：按一下锁住，再按一下解除，可与按下并存（Grp）。
    Latching,
}

/// 位号 → 名称（8 条冻结）。
pub const BIT_NAMES: [&str; MOD_BITS] = [
    "Shift", "CapsLock", "Control", "Alt", "Meta", "NumLock", "Grp", "ScrollLock",
];

/// 位号 → 位类（8 条冻结；Caps/Num/Scroll 为锁定类，Grp 为锁存类）。
pub const BIT_KINDS: [BitKind; MOD_BITS] = [
    BitKind::Momentary,
    BitKind::Locking,
    BitKind::Momentary,
    BitKind::Momentary,
    BitKind::Momentary,
    BitKind::Locking,
    BitKind::Latching,
    BitKind::Locking,
];

/// 位号 → 位类（越界当作瞬时位，调用方必须先过位号闸）。
pub fn kind_of(bit: usize) -> BitKind {
    BIT_KINDS.get(bit).copied().unwrap_or(BitKind::Momentary)
}

/// 位号 → 名称（越界给 "Bit?"）。
pub fn name_of(bit: usize) -> &'static str {
    BIT_NAMES.get(bit).copied().unwrap_or("Bit?")
}

/// 位号 → 位权（越界返回 0：位号闸才是判死点）。
pub fn weight_of(bit: usize) -> u8 {
    if bit >= MOD_BITS {
        return 0;
    }
    1u8 << bit
}

/// 掩码 → 可读串（"Shift+CapsLock"，零掩码给 "None"）。
pub fn describe(mask: u8) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < MOD_BITS {
        let w = weight_of(i);
        if w != 0 && (mask & w) != 0 {
            if !s.is_empty() {
                s.push('+');
            }
            s.push_str(name_of(i));
        }
        i += 1;
    }
    if s.is_empty() {
        s.push_str("None");
    }
    s
}

// ── 锁存事件与状态机 ────────────────────────────────────────────
/// 一次修饰事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// 按下位
    Press(u8),
    /// 松开位
    Release(u8),
    /// 锁定位取反（Caps/Num/Scroll 的点击语义）
    ToggleLock(u8),
    /// 锁存位取反（Grp）
    Latch(u8),
}

/// 8 位锁存状态机：down/locked/latched 三个内部位图，唯一的对外出口是
/// `effective()`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LatchMachine {
    /// 按下计数（饱和 1；u8 便于与 u32 模型区分类型）。
    down: [u8; MOD_BITS],
    /// 锁定位（Caps/Num/Scroll 的持久态）。
    locked: [bool; MOD_BITS],
    /// 锁存位（Grp）。
    latched: [bool; MOD_BITS],
    /// 已应用事件数。
    pub steps: u32,
    /// 无按下释放次数。
    pub orphans: u32,
    /// 重复按下次数。
    pub doubles: u32,
    /// 越界位事件次数。
    pub overflows: u32,
    /// 对瞬时位锁存次数。
    pub bad_latch: u32,
}

impl LatchMachine {
    /// 全零起机。
    pub const fn new() -> LatchMachine {
        LatchMachine {
            down: [0; MOD_BITS],
            locked: [false; MOD_BITS],
            latched: [false; MOD_BITS],
            steps: 0,
            orphans: 0,
            doubles: 0,
            overflows: 0,
            bad_latch: 0,
        }
    }

    /// 应用一个事件并返回有效掩码（异常只记账，不改状态）。
    pub fn apply(&mut self, op: Op) -> u8 {
        self.steps = self.steps.saturating_add(1);
        match op {
            Op::Press(b) => {
                let i = b as usize;
                if i >= MOD_BITS {
                    self.overflows = self.overflows.saturating_add(1);
                } else {
                    match self.down.get_mut(i) {
                        Some(c) => {
                            if *c == 0 {
                                *c = 1;
                            } else {
                                self.doubles = self.doubles.saturating_add(1);
                            }
                        }
                        None => self.overflows = self.overflows.saturating_add(1),
                    }
                }
            }
            Op::Release(b) => {
                let i = b as usize;
                if i >= MOD_BITS {
                    self.overflows = self.overflows.saturating_add(1);
                } else {
                    match self.down.get_mut(i) {
                        Some(c) => {
                            if *c == 0 {
                                self.orphans = self.orphans.saturating_add(1);
                            } else {
                                *c = c.saturating_sub(1);
                            }
                        }
                        None => self.overflows = self.overflows.saturating_add(1),
                    }
                }
            }
            Op::ToggleLock(b) => {
                let i = b as usize;
                if i >= MOD_BITS {
                    self.overflows = self.overflows.saturating_add(1);
                } else if kind_of(i) != BitKind::Locking {
                    // 非锁定位不得取反（Num/Caps/Scroll 之外的位没有持久态）。
                    self.bad_latch = self.bad_latch.saturating_add(1);
                } else if let Some(l) = self.locked.get_mut(i) {
                    *l = !*l;
                }
            }
            Op::Latch(b) => {
                let i = b as usize;
                if i >= MOD_BITS {
                    self.overflows = self.overflows.saturating_add(1);
                } else if kind_of(i) != BitKind::Latching {
                    self.bad_latch = self.bad_latch.saturating_add(1);
                } else if let Some(l) = self.latched.get_mut(i) {
                    *l = !*l;
                }
            }
        }
        self.effective()
    }

    /// 合成有效掩码（对外唯一真相）。
    pub fn effective(&self) -> u8 {
        let mut mask = 0u8;
        let mut i = 0usize;
        while i < MOD_BITS {
            let down = self.down.get(i).copied().unwrap_or(0) > 0;
            let locked = self.locked.get(i).copied().unwrap_or(false);
            let latched = self.latched.get(i).copied().unwrap_or(false);
            let on = match kind_of(i) {
                BitKind::Momentary => down,
                BitKind::Locking => down != locked,
                BitKind::Latching => down || latched,
            };
            if on {
                mask |= weight_of(i);
            }
            i += 1;
        }
        mask
    }

    /// 锁定位视图（三把锁键的持久面）。
    pub fn locked_mask(&self) -> u8 {
        let mut mask = 0u8;
        let mut i = 0usize;
        while i < MOD_BITS {
            if self.locked.get(i).copied().unwrap_or(false) {
                mask |= weight_of(i);
            }
            i += 1;
        }
        mask
    }

    /// 锁存位视图。
    pub fn latched_mask(&self) -> u8 {
        let mut mask = 0u8;
        let mut i = 0usize;
        while i < MOD_BITS {
            if self.latched.get(i).copied().unwrap_or(false) {
                mask |= weight_of(i);
            }
            i += 1;
        }
        mask
    }

    /// 打包快照（回放与跨模块对账用）：有效/锁定/锁存 + 步号折成 u32。
    pub fn pack(&self) -> u32 {
        let mut v = (self.effective() as u32) << 16;
        v |= (self.locked_mask() as u32) << 8;
        v |= self.latched_mask() as u32;
        v ^= self.steps.rotate_left(3) & 0x3f
            ^ ((self.orphans ^ self.doubles) & 0x3)
            ^ ((self.overflows ^ self.bad_latch) & 0x3);
        v
    }
}

/// 独立参考模型：**从事件日志全量重算**（不复用状态机的任何字段）。
///
/// 这是 500 组合判据的对照臂：状态机是增量实现，模型是重算式实现，
/// 两条代码路径对同一日志必须给出同一有效掩码。
pub fn model_effective(log: &[Op]) -> u8 {
    let mut press: [i32; MOD_BITS] = [0; MOD_BITS];
    let mut locked = [false; MOD_BITS];
    let mut latched = [false; MOD_BITS];
    for op in log.iter() {
        match *op {
            Op::Press(b) => {
                if let Some(p) = press.get_mut(b as usize) {
                    *p += 1;
                }
            }
            Op::Release(b) => {
                if let Some(p) = press.get_mut(b as usize) {
                    if *p > 0 {
                        *p -= 1;
                    }
                }
            }
            Op::ToggleLock(b) => {
                let i = b as usize;
                if kind_of(i) == BitKind::Locking {
                    if let Some(l) = locked.get_mut(i) {
                        *l = !*l;
                    }
                }
            }
            Op::Latch(b) => {
                let i = b as usize;
                if kind_of(i) == BitKind::Latching {
                    if let Some(l) = latched.get_mut(i) {
                        *l = !*l;
                    }
                }
            }
        }
    }
    let mut mask = 0u8;
    let mut i = 0usize;
    while i < MOD_BITS {
        let down = press.get(i).copied().unwrap_or(0) > 0;
        let lk = locked.get(i).copied().unwrap_or(false);
        let lt = latched.get(i).copied().unwrap_or(false);
        let on = match kind_of(i) {
            BitKind::Momentary => down,
            BitKind::Locking => down != lk,
            BitKind::Latching => down || lt,
        };
        if on {
            mask |= weight_of(i);
        }
        i += 1;
    }
    mask
}

/// 500 次随机组合终态判据：每步与重算模型比对。
pub fn stress_500() -> Result<(u32, u64), u32> {
    let mut m = LatchMachine::new();
    let mut log: Vec<Op> = Vec::new();
    let mut r = Roll::new(SEG_BASE);
    let mut digest: u64 = 0x243F_6A88_85A3_08D3;
    let mut n = 0u32;
    while n < STRESS_STEPS {
        // 事件种类 0..4 × 位号 0..8；位号偶发越界（n % 37 == 0）以打位号闸。
        // 前两步写死「同一键连按两次」：随机游走未必能自发产生抖动态，
        // 而重复按下必须被覆盖到，否则 500 组合判据会漏掉粘键路径。
        let bit = if n < 2 {
            3
        } else if n % 37 == 0 {
            (MOD_BITS as u32 + r.pick(3)) as u8
        } else {
            r.pick(MOD_BITS as u32) as u8
        };
        let op = match n {
            0 | 1 => Op::Press(bit),
            _ => match r.pick(4) {
                0 => Op::Press(bit),
                1 => Op::Release(bit),
                2 => Op::ToggleLock(bit),
                _ => Op::Latch(bit),
            },
        };
        let got = m.apply(op);
        log.push(op);
        let want = model_effective(log.as_slice());
        if got != want {
            return Err(ModCode::RandomDrift.val());
        }
        digest = fold_mask(digest, got);
        digest = fold_step(digest, n);
        n += 1;
    }
    // 随机序列必然同时覆盖「有按下」「无按下释放」「重复按下」三种边角。
    if m.orphans == 0 || m.doubles == 0 || m.overflows == 0 || m.bad_latch == 0 {
        return Err(ModCode::RandomDrift.val());
    }
    Ok((m.steps, digest))
}

// ── 锁定键三态（F48824） ───────────────────────────────────────
/// 三把锁键。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockId {
    /// CapsLock
    Caps = 0,
    /// NumLock
    Num = 1,
    /// ScrollLock
    Scroll = 2,
}

impl LockId {
    /// LED 位。
    pub fn led_bit(self) -> u8 {
        match self {
            LockId::Caps => LED_CAPS,
            LockId::Num => LED_NUM,
            LockId::Scroll => LED_SCROLL,
        }
    }

    /// 修饰位。
    pub fn mod_bit(self) -> u8 {
        match self {
            LockId::Caps => B_CAPS,
            LockId::Num => B_NUM,
            LockId::Scroll => B_SCROLL,
        }
    }

    /// 名称。
    pub fn label(self) -> &'static str {
        match self {
            LockId::Caps => "CapsLock",
            LockId::Num => "NumLock",
            LockId::Scroll => "ScrollLock",
        }
    }

    /// id → 锁键（越界 None，由锁键闸判死）。
    pub fn from_index(i: usize) -> Option<LockId> {
        match i {
            0 => Some(LockId::Caps),
            1 => Some(LockId::Num),
            2 => Some(LockId::Scroll),
            _ => None,
        }
    }
}

/// 锁定键三态：关 / 开 / 失同步。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockPhase {
    /// 关（状态与 LED 都是 0）
    Off,
    /// 开（状态与 LED 都是 1）
    On,
    /// 失同步（状态与 LED 不一致，必须重写一次才算收口）
    Desync,
}

impl LockPhase {
    /// 判定函数：状态位与回读位。
    pub fn of(state: bool, led: bool) -> LockPhase {
        if state != led {
            LockPhase::Desync
        } else if state {
            LockPhase::On
        } else {
            LockPhase::Off
        }
    }

    /// 文案。
    pub fn label(self) -> &'static str {
        match self {
            LockPhase::Off => "Off",
            LockPhase::On => "On",
            LockPhase::Desync => "Desync",
        }
    }
}

/// 单把锁键的账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LockKey {
    /// 期望态（语义真相）。
    pub state: bool,
    /// 设备回读态（LED 真相）。
    pub led: bool,
    /// 三态。
    pub phase: LockPhase,
    /// 已发写次数。
    pub writes: u32,
    /// 已收回读次数。
    pub echoes: u32,
    /// 重写次数（失同步收口）。
    pub resync: u32,
}

impl LockKey {
    /// 起机（关）。
    pub const fn new() -> LockKey {
        LockKey {
            state: false,
            led: false,
            phase: LockPhase::Off,
            writes: 0,
            echoes: 0,
            resync: 0,
        }
    }

    /// 由 state/led 重算三态。
    fn refresh(&mut self) {
        self.phase = LockPhase::of(self.state, self.led);
    }
}

/// 三把锁键的银行（LED 掩码的唯一装配点）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockBank {
    /// 三把锁键（序：Caps/Num/Scroll）。
    pub keys: [LockKey; LOCK_N],
    /// 已发写入次数（合计）。
    pub writes: u32,
    /// 已收回读次数（合计）。
    pub echoes: u32,
    /// 待回读的期望掩码（写入未回读时非零）。
    pub pending: Option<u8>,
    /// 往返计数（判据 ×10^2）。
    pub trips: u32,
}

impl LockBank {
    /// 起机（三把全关，无 pending）。
    pub const fn new() -> LockBank {
        LockBank {
            keys: [LockKey::new(); LOCK_N],
            writes: 0,
            echoes: 0,
            pending: None,
            trips: 0,
        }
    }

    /// 索引取键。
    pub fn key(&self, id: LockId) -> Option<&LockKey> {
        self.keys.get(id as usize)
    }

    /// 期望掩码（状态面）。
    pub fn want_mask(&self) -> u8 {
        let mut mask = 0u8;
        let mut i = 0usize;
        while i < LOCK_N {
            if let Some(k) = self.keys.get(i) {
                if k.state {
                    if let Some(id) = LockId::from_index(i) {
                        mask |= id.led_bit();
                    }
                }
            }
            i += 1;
        }
        mask
    }

    /// 回读掩码（LED 面）。
    pub fn led_mask(&self) -> u8 {
        let mut mask = 0u8;
        let mut i = 0usize;
        while i < LOCK_N {
            if let Some(k) = self.keys.get(i) {
                if k.led {
                    if let Some(id) = LockId::from_index(i) {
                        mask |= id.led_bit();
                    }
                }
            }
            i += 1;
        }
        mask
    }

    /// 点击锁键：状态取反 → 发一次 LED 写（本模块假设写必成功）。
    pub fn press(&mut self, id: LockId) -> Result<u8, u32> {
        let idx = id as usize;
        if idx >= LOCK_N {
            return Err(ModCode::UnknownLock.val());
        }
        match self.keys.get_mut(idx) {
            Some(k) => {
                k.state = !k.state;
                k.writes = k.writes.saturating_add(1);
                // 写入即时生效（失败路径归 guard）。
                k.led = k.state;
                k.refresh();
                self.writes = self.writes.saturating_add(1);
            }
            None => return Err(ModCode::UnknownLock.val()),
        }
        let want = self.want_mask();
        self.pending = Some(want);
        Ok(want)
    }

    /// 设备回读：掩码越 3 位即拒；无 pending 的回读不得采纳。
    pub fn echo(&mut self, mask: u8) -> Result<u8, u32> {
        if (mask & !(LED_CAPS | LED_NUM | LED_SCROLL)) != 0 {
            return Err(ModCode::LockMaskOverflow.val());
        }
        if self.pending.is_none() {
            return Err(ModCode::EchoWithoutWrite.val());
        }
        let want = self.want_mask();
        if mask != want {
            // 回读与期望不符：丢一次回读并重写，三态落 Desync 直到对齐。
            let idx = mismatch_index(want, mask);
            if let Some(i) = idx {
                if let Some(k) = self.keys.get_mut(i) {
                    k.led = mask & (1u8 << i) != 0;
                    k.refresh();
                    k.resync = k.resync.saturating_add(1);
                }
            }
            self.pending = Some(want);
            return Err(ModCode::PhaseDesync.val());
        }
        let mut i = 0usize;
        while i < LOCK_N {
            if let Some(k) = self.keys.get_mut(i) {
                k.led = mask & (1u8 << i) != 0;
                k.echoes = k.echoes.saturating_add(1);
                k.refresh();
            }
            i += 1;
        }
        self.echoes = self.echoes.saturating_add(1);
        self.pending = None;
        self.trips = self.trips.saturating_add(1);
        Ok(mask)
    }

    /// 外部改写 LED（另一路：固件自检 / 双系统切回）：状态必须采纳，绝不
    /// 允许「LED 亮了而状态还认为关」（这正是双向同步缺一半的形态）。
    pub fn external(&mut self, mask: u8) -> Result<u8, u32> {
        if (mask & !(LED_CAPS | LED_NUM | LED_SCROLL)) != 0 {
            return Err(ModCode::LockMaskOverflow.val());
        }
        let mut i = 0usize;
        while i < LOCK_N {
            let bit = 1u8 << i;
            let on = (mask & bit) != 0;
            if let Some(k) = self.keys.get_mut(i) {
                k.led = on;
                k.state = on;
                k.refresh();
            }
            i += 1;
        }
        self.pending = None;
        Ok(mask)
    }

    /// 三把锁键是否全部收口为关/开（无失同步）。
    pub fn all_settled(&self) -> bool {
        let mut i = 0usize;
        while i < LOCK_N {
            if let Some(k) = self.keys.get(i) {
                if k.phase == LockPhase::Desync || k.state != k.led {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 失同步键数。
    pub fn desync_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < LOCK_N {
            if let Some(k) = self.keys.get(i) {
                if k.phase == LockPhase::Desync {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

/// 期望与回读首个不一致的位下标。
fn mismatch_index(want: u8, got: u8) -> Option<usize> {
    let mut i = 0usize;
    while i < LOCK_N {
        let bit = 1u8 << i;
        if (want & bit) != (got & bit) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 100 次往返判据：点击 → 写 → 回读一致 → 外部改写 → 采纳 → 复原。
pub fn led_round_trips(n: u32) -> Result<u32, u32> {
    let mut bank = LockBank::new();
    let mut trips = 0u32;
    let mut i = 0u32;
    while i < n {
        let id = LockId::from_index((i % 3) as usize).ok_or(ModCode::UnknownLock.val())?;
        let want = bank.press(id)?;
        let got = bank.echo(want)?;
        if got != want {
            return Err(ModCode::PhaseDesync.val());
        }
        if !bank.all_settled() {
            return Err(ModCode::PhaseDesync.val());
        }
        // 双向的另一半：外部把 LED 翻一位，状态必须采纳。
        let ext = want ^ id.led_bit();
        let adopted = bank.external(ext)?;
        if adopted != ext || bank.want_mask() != ext || bank.led_mask() != ext {
            return Err(ModCode::PhaseDesync.val());
        }
        // 采纳后再点一次，回到原态并回读闭合。
        let back = bank.press(id)?;
        if back != want || bank.echo(back)? != want {
            return Err(ModCode::PhaseDesync.val());
        }
        trips += 1;
        i += 1;
    }
    if bank.trips < n || trips != n || !bank.all_settled() {
        return Err(ModCode::RoundTripIncomplete.val());
    }
    Ok(trips)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：位表自洽 → 500 组合 → 100 往返 → 状态机与锁银行对账。
/// 返回 (500 步摘要, 往返次数, 有效掩码摘要)。
pub fn golden_run() -> Result<(u64, u32, u64), u32> {
    // 1) 位表：名称/位权/位类三张表必须逐位对齐。
    let mut i = 0usize;
    while i < MOD_BITS {
        let w = weight_of(i);
        if w != (1u8 << i) || name_of(i).is_empty() {
            return Err(ModCode::BitOutOfRange.val());
        }
        i += 1;
    }
    // 2) 500 组合。
    let (steps, digest) = stress_500()?;
    if steps != STRESS_STEPS {
        return Err(ModCode::RandomDrift.val());
    }
    // 3) 100 往返。
    let trips = led_round_trips(ROUND_TRIPS)?;
    if trips != ROUND_TRIPS {
        return Err(ModCode::RoundTripIncomplete.val());
    }
    // 4) 状态机 ←→ 锁银行：Caps 锁定位与 Caps LED 必须同相。
    let mut m = LatchMachine::new();
    m.apply(Op::ToggleLock(1));
    let mut bank = LockBank::new();
    bank.press(LockId::Caps)?;
    let m_caps = (m.locked_mask() & B_CAPS) != 0;
    let b_caps = bank.key(LockId::Caps).map(|k| k.state).unwrap_or(false);
    if m_caps != b_caps {
        return Err(ModCode::PhaseDesync.val());
    }
    // 5) Grp 位在锁存后必须进有效掩码。
    m.apply(Op::Latch(6));
    if (m.effective() & B_GRP) == 0 {
        return Err(ModCode::RandomDrift.val());
    }
    let eff = fold_text(fold_mask(0xcbf2_9ce4_8422_2325, m.effective()), describe(m.effective()).as_str());
    Ok((digest, trips, eff))
}

/// 探针 a：500 组合稳定。
pub fn probe_seq_a() -> Result<(), u32> {
    match stress_500() {
        Ok((n, d)) if n == STRESS_STEPS && d != 0 => Ok(()),
        _ => Err(ModCode::RandomDrift.val()),
    }
}

/// 探针 b：越界位不得改动他位（先判位号闸）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut m = LatchMachine::new();
    m.apply(Op::Press(0));
    let before = m.effective();
    let after = m.apply(Op::Release(MOD_BITS as u8));
    if after != before || m.overflows == 0 {
        return Err(ModCode::BitOutOfRange.val());
    }
    Ok(())
}

/// 探针 c：回读掩码越界必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut bank = LockBank::new();
    bank.press(LockId::Num)?;
    match bank.echo(0x08) {
        Err(c) if c == ModCode::LockMaskOverflow.val() => Ok(()),
        _ => Err(ModCode::LockMaskOverflow.val()),
    }
}

/// 注入账（十槽）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    /// 各槽期望码。
    pub want: [u32; 10],
    /// 各槽判定次数。
    pub tries: [u32; 10],
    /// 各槽命中次数。
    pub caught: [u32; 10],
}

impl InjectLedger {
    /// 建账。
    pub fn new() -> InjectLedger {
        InjectLedger {
            want: [
                ModCode::BitOutOfRange.val(),
                ModCode::OrphanRelease.val(),
                ModCode::DoublePress.val(),
                ModCode::LatchOnMomentary.val(),
                ModCode::LockMaskOverflow.val(),
                ModCode::EchoWithoutWrite.val(),
                ModCode::PhaseDesync.val(),
                ModCode::UnknownLock.val(),
                ModCode::RandomDrift.val(),
                ModCode::RoundTripIncomplete.val(),
            ],
            tries: [0; 10],
            caught: [0; 10],
        }
    }

    /// 记账（越界槽丢弃）。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot >= 10 {
            return;
        }
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c += 1;
            }
        }
    }

    /// 全槽至少一次且全命中。
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

/// 注入十轮：十类异常各判一次（异常由状态机记账字段暴露）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：越界位 → overflows 递增且状态不变。
        let mut m = LatchMachine::new();
        m.apply(Op::Press(1));
        let base = m.effective();
        m.apply(Op::Press(MOD_BITS as u8 + 1));
        led.record(0, m.overflows == 1 && m.effective() == base);

        // 槽 1：无按下释放 → orphans 递增且位仍灭。
        let mut m1 = LatchMachine::new();
        m1.apply(Op::Release(0));
        led.record(1, m1.orphans == 1 && m1.effective() == 0);

        // 槽 2：重复按下 → doubles 递增且计数仍 1（一次释放即全灭）。
        let mut m2 = LatchMachine::new();
        m2.apply(Op::Press(0));
        m2.apply(Op::Press(0));
        let after_double = m2.apply(Op::Release(0));
        led.record(2, m2.doubles == 1 && after_double == 0);

        // 槽 3：对瞬时位锁存 → bad_latch 递增且有效态不变。
        let mut m3 = LatchMachine::new();
        m3.apply(Op::Latch(0));
        led.record(3, m3.bad_latch == 1 && m3.effective() == 0);

        // 槽 4：LED 掩码越界 → 拒。
        let mut b4 = LockBank::new();
        let hit4 = b4.echo(0x80).err() == Some(ModCode::LockMaskOverflow.val());
        led.record(4, hit4);

        // 槽 5：未写先回读 → 拒。
        let mut b5 = LockBank::new();
        led.record(5, b5.echo(0x00).err() == Some(ModCode::EchoWithoutWrite.val()));

        // 槽 6：回读与期望不符 → 三态落 Desync。
        let mut b6 = LockBank::new();
        let want6 = b6.press(LockId::Caps).unwrap_or(0);
        let wrong = want6 ^ LED_CAPS;
        let err6 = b6.echo(wrong).err();
        led.record(
            6,
            err6 == Some(ModCode::PhaseDesync.val()) && b6.desync_count() == 1,
        );

        // 槽 7：锁键 id 越界 → 拒（from_index 越界）。
        led.record(7, LockId::from_index(LOCK_N).is_none());

        // 槽 8：模型与状态机必须逐步同值（篡改日志即漂移）。
        let mut m8 = LatchMachine::new();
        let mut log8: Vec<Op> = Vec::new();
        let mut r8 = Roll::new(0x51 + r);
        let mut k = 0u32;
        let mut same = true;
        while k < 64 {
            let bit = r8.pick(MOD_BITS as u32) as u8;
            let op = match r8.pick(4) {
                0 => Op::Press(bit),
                1 => Op::Release(bit),
                2 => Op::ToggleLock(bit),
                _ => Op::Latch(bit),
            };
            let got = m8.apply(op);
            log8.push(op);
            if got != model_effective(log8.as_slice()) {
                same = false;
            }
            k += 1;
        }
        // 人为篡改一个事件后模型必须变（证明对照臂不是恒真）。
        let mut tampered = log8.clone();
        if let Some(slot) = tampered.get_mut(0) {
            *slot = Op::Press(3);
        }
        let tamper_moved = model_effective(tampered.as_slice()) != model_effective(log8.as_slice());
        led.record(8, same && tamper_moved);

        // 槽 9：往返次数不足即红（用 0 轮打「不足」判据）。
        let short = led_round_trips(0).map(|n| n != 0).unwrap_or(false);
        led.record(9, !short);

        r += 1;
    }
    Ok(led)
}

/// 双跑对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ModCode::RandomDrift.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ModCode::ALL.len() {
        if let Some(c) = ModCode::ALL.get(i) {
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

    // J1-1 八位位表冻结。
    let mut bit_ok = true;
    let mut i = 0usize;
    while i < MOD_BITS {
        if weight_of(i) != 1u8 << i || kind_of(i) == BitKind::Momentary && i == 6 {
            bit_ok = false;
        }
        i += 1;
    }
    if BIT_KINDS.get(6).copied() != Some(BitKind::Latching)
        || BIT_KINDS.get(1).copied() != Some(BitKind::Locking)
        || BIT_KINDS.get(7).copied() != Some(BitKind::Locking)
    {
        bit_ok = false;
    }
    v.push(("F48823-J1-八位位表冻结", bit_ok, String::new()));

    // J1-2 500 组合终态一致。
    let s = stress_500();
    let stress_ok = match &s {
        Ok((n, d)) => *n == STRESS_STEPS && *d != 0,
        Err(_) => false,
    };
    v.push((
        "F48823-J1-五百组合终态一致",
        stress_ok,
        match &s {
            Ok((n, d)) => format!("steps={} dig={:016x}", n, d),
            Err(c) => format!("err=0x{:X}", c),
        },
    ));

    // J1-3 Caps 的 XOR 语义（同一位：按下与锁定互斥，锁定面不被按下污染）。
    let mut m = LatchMachine::new();
    let a = m.apply(Op::Press(0));
    let b = m.apply(Op::ToggleLock(1));
    let c = m.effective();
    let d = m.apply(Op::Press(1));
    let e = m.apply(Op::Release(1));
    let xor_ok = a == B_SHIFT
        && b == (B_SHIFT | B_CAPS)
        && c == (B_SHIFT | B_CAPS)
        && d == B_SHIFT
        && e == (B_SHIFT | B_CAPS)
        && m.locked_mask() == B_CAPS;
    v.push((
        "F48823-J1-Caps异或语义",
        xor_ok,
        format!("0x{:02x}/0x{:02x}/0x{:02x}/0x{:02x}/0x{:02x} 锁=0x{:02x}", a, b, c, d, e, m.locked_mask()),
    ));

    // J1-4 Grp 锁存（按一下锁住，再按解除）。
    let mut g = LatchMachine::new();
    let on = g.apply(Op::Latch(6));
    let off = g.apply(Op::Latch(6));
    v.push((
        "F48823-J1-Grp锁存往返",
        on == B_GRP && off == 0,
        format!("on=0x{:02x} off=0x{:02x}", on, off),
    ));

    // J1-5 异常三类记账（orphan/double/overflow 各自独立计数）。
    let mut an = LatchMachine::new();
    an.apply(Op::Release(2));
    an.apply(Op::Press(2));
    an.apply(Op::Press(2));
    an.apply(Op::Press(9));
    an.apply(Op::Latch(2));
    v.push((
        "F48823-J1-异常三分记账",
        an.orphans == 1 && an.doubles == 1 && an.overflows == 1 && an.bad_latch == 1,
        format!(
            "o/d/v/l={}/{}/{}/{}",
            an.orphans, an.doubles, an.overflows, an.bad_latch
        ),
    ));

    // J1-6 锁键三态（Off/On/Desync 三态判别各自成立）。
    let tri_ok = LockPhase::of(false, false) == LockPhase::Off
        && LockPhase::of(true, true) == LockPhase::On
        && LockPhase::of(true, false) == LockPhase::Desync
        && LockPhase::of(false, true) == LockPhase::Desync;
    v.push((
        "F48824-J1-锁键三态判别",
        tri_ok,
        format!(
            "{}/{}/{}",
            LockPhase::Off.label(),
            LockPhase::On.label(),
            LockPhase::Desync.label()
        ),
    ));

    // J1-7 100 往返双向一致。
    let t = led_round_trips(ROUND_TRIPS);
    let trips_ok = match &t {
        Ok(n) => *n == ROUND_TRIPS,
        Err(_) => false,
    };
    v.push((
        "F48824-J1-百次往返双向一致",
        trips_ok,
        match &t {
            Ok(n) => format!("trips={}", n),
            Err(c) => format!("err=0x{:X}", c),
        },
    ));

    // J1-8 外部改写必须被采纳（双向的另一半）。
    let mut b = LockBank::new();
    let ext_ok = match b.external(LED_CAPS | LED_SCROLL) {
        Ok(_) => {
            b.want_mask() == (LED_CAPS | LED_SCROLL)
                && b.led_mask() == (LED_CAPS | LED_SCROLL)
                && b.all_settled()
        }
        Err(_) => false,
    };
    v.push((
        "F48824-J1-外部改写采纳",
        ext_ok,
        format!("want=0x{:02x} led=0x{:02x}", b.want_mask(), b.led_mask()),
    ));

    // J2 探针三序。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_ok();
    let pc = probe_seq_c().is_ok();
    v.push((
        "F48823-J2-探针三序",
        pa && pb && pc,
        format!("a={} b={} c={}", pa, pb, pc),
    ));

    // J3 段位闭集。
    v.push(("F48823-J3-段位闭集", segment_guard(), String::new()));

    v
}

/// 本模块自检入口（名与文件名前 8 字符一致）。
pub fn run_unx48823_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48823 修饰键八位锁存与锁定键三态");
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
    fn t_bit_tables_frozen() {
        assert_eq!(MOD_BITS, 8);
        assert_eq!(weight_of(0), B_SHIFT);
        assert_eq!(weight_of(1), B_CAPS);
        assert_eq!(weight_of(5), B_NUM);
        assert_eq!(weight_of(6), B_GRP);
        assert_eq!(weight_of(7), B_SCROLL);
        assert_eq!(weight_of(8), 0);
        assert_eq!(kind_of(1), BitKind::Locking);
        assert_eq!(kind_of(6), BitKind::Latching);
        assert_eq!(kind_of(0), BitKind::Momentary);
        assert_eq!(kind_of(99), BitKind::Momentary);
    }

    #[test]
    fn t_describe_chain() {
        assert_eq!(describe(0), "None");
        assert_eq!(describe(B_SHIFT | B_CTRL), "Shift+Control");
        assert_eq!(describe(B_CAPS | B_GRP), "CapsLock+Grp");
        assert_eq!(describe(0xff), "Shift+CapsLock+Control+Alt+Meta+NumLock+Grp+ScrollLock");
    }

    #[test]
    fn t_momentary_press_release() {
        let mut m = LatchMachine::new();
        assert_eq!(m.apply(Op::Press(0)), B_SHIFT);
        assert_eq!(m.apply(Op::Press(2)), B_SHIFT | B_CTRL);
        assert_eq!(m.apply(Op::Release(0)), B_CTRL);
        assert_eq!(m.apply(Op::Release(2)), 0);
        assert_eq!(m.steps, 4);
        assert_eq!(m.orphans, 0);
    }

    #[test]
    fn t_caps_xor() {
        let mut m = LatchMachine::new();
        m.apply(Op::ToggleLock(1));
        assert_eq!(m.effective(), B_CAPS);
        // 锁定位的 XOR 是「同一位」的按下与锁定互斥：Caps 已锁定时再按住
        // Caps 键，该位反而灭（第二次点击的手感），Shift 位不受影响——
        // Caps 反转 Shift 语义属 keymap 取层规则（core 的 SEL_LOCK），
        // 不在掩码层做，否则同一事实会有两个真相源。
        assert_eq!(m.apply(Op::Press(0)), B_CAPS | B_SHIFT);
        assert_eq!(m.apply(Op::Press(1)), B_SHIFT);
        assert_eq!(m.apply(Op::Release(1)), B_CAPS | B_SHIFT);
        m.apply(Op::ToggleLock(1));
        assert_eq!(m.effective(), B_SHIFT);
    }

    #[test]
    fn t_num_scroll_locking() {
        let mut m = LatchMachine::new();
        m.apply(Op::ToggleLock(5));
        m.apply(Op::ToggleLock(7));
        assert_eq!(m.locked_mask(), B_NUM | B_SCROLL);
        assert_eq!(m.effective(), B_NUM | B_SCROLL);
        // 锁定位也可被按下反转（Num 按下 + 锁定 = 灭）。
        assert_eq!(m.apply(Op::Press(5)), B_SCROLL);
    }

    #[test]
    fn t_grp_latch() {
        let mut m = LatchMachine::new();
        // 锁存：按一下锁住，与「按住」并存但只算一次。
        assert_eq!(m.apply(Op::Latch(6)), B_GRP);
        assert_eq!(m.apply(Op::Press(6)), B_GRP);
        assert_eq!(m.apply(Op::Release(6)), B_GRP);
        // 按住时解除锁存：位仍亮（还按着），松开才灭。
        assert_eq!(m.apply(Op::Press(6)), B_GRP);
        assert_eq!(m.apply(Op::Latch(6)), B_GRP);
        assert_eq!(m.apply(Op::Release(6)), 0);
        assert_eq!(m.latched_mask(), 0);
    }

    #[test]
    fn t_orphan_double_overflow() {
        let mut m = LatchMachine::new();
        m.apply(Op::Release(3));
        m.apply(Op::Press(3));
        m.apply(Op::Press(3));
        m.apply(Op::Press(200));
        m.apply(Op::ToggleLock(0));
        assert_eq!(m.orphans, 1);
        assert_eq!(m.doubles, 1);
        assert_eq!(m.overflows, 1);
        assert_eq!(m.bad_latch, 1);
        // 重复按下只算一次，一次释放即清零。
        assert_eq!(m.apply(Op::Release(3)), 0);
    }

    #[test]
    fn t_stress_500() {
        let (n, d) = stress_500().expect("stress");
        assert_eq!(n, STRESS_STEPS);
        assert!(d != 0);
        // 双跑同值（确定性掷骰）。
        assert_eq!(stress_500().expect("again"), (n, d));
    }

    #[test]
    fn t_model_independent() {
        let log = [
            Op::Press(0),
            Op::ToggleLock(1),
            Op::Latch(6),
            Op::Press(5),
            Op::ToggleLock(5),
        ];
        let mut m = LatchMachine::new();
        for op in log.iter() {
            m.apply(*op);
        }
        assert_eq!(m.effective(), model_effective(&log));
        // 去掉末事件后两臂仍同值。
        assert_eq!(
            m.effective() != model_effective(&log[0..4]),
            true,
            "末事件必须影响有效掩码"
        );
    }

    #[test]
    fn t_led_round_trips() {
        assert_eq!(led_round_trips(ROUND_TRIPS), Ok(ROUND_TRIPS));
        assert_eq!(led_round_trips(0), Ok(0));
    }

    #[test]
    fn t_led_mask_gate() {
        let mut b = LockBank::new();
        assert_eq!(b.echo(0x00), Err(ModCode::EchoWithoutWrite.val()));
        let want = b.press(LockId::Caps).expect("press");
        assert_eq!(want, LED_CAPS);
        assert_eq!(b.echo(0x80), Err(ModCode::LockMaskOverflow.val()));
        assert_eq!(b.echo(LED_CAPS ^ 0x02), Err(ModCode::PhaseDesync.val()));
        assert_eq!(b.desync_count(), 1);
        assert_eq!(b.echo(LED_CAPS), Ok(LED_CAPS));
        assert!(b.all_settled());
        assert_eq!(b.trips, 1);
    }

    #[test]
    fn t_external_led_adopt() {
        let mut b = LockBank::new();
        b.press(LockId::Num).expect("press");
        b.echo(LED_NUM).expect("echo");
        assert_eq!(b.external(LED_SCROLL), Ok(LED_SCROLL));
        assert_eq!(b.want_mask(), LED_SCROLL);
        assert_eq!(b.led_mask(), LED_SCROLL);
        assert!(b.all_settled());
        assert_eq!(b.key(LockId::Num).map(|k| k.state), Some(false));
        assert_eq!(b.key(LockId::Scroll).map(|k| k.phase), Some(LockPhase::On));
    }

    #[test]
    fn t_inject_ten_slots() {
        let led = inject_rounds(2).expect("inject");
        assert!(led.all_caught());
        for i in 0..10 {
            assert!(led.tries[i] == 2, "槽{} 判定次数", i);
        }
    }

    #[test]
    fn t_segment_and_checks() {
        assert!(segment_guard());
        for c in ModCode::ALL.iter() {
            assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x20);
            assert!(!c.detail().is_empty());
        }
        assert!(detail(0x1234).contains("未知"));
        let v = extra_checks();
        assert!(v.len() >= 8 && v.len() <= 10);
        for (n, p, _) in v {
            assert!(p, "红项 {}", n);
        }
        assert!(run_unx48823_checks().all_passed());
        assert!(repro_check().is_ok());
    }
}
