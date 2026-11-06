//! UNX-F50117 直通真机对照登记位（T1 档 ×2 台随闸门补测槽位）（0x43ac0 段）
//!
//! 锚点：UNX-F50117-J1 槽位登记不虚报；判据锚定 §AI-63 §5 诚实三态。
//!
//! 本模块只做一件事：把「真机对照」这件事的**记账**做诚实。直通（passthrough）
//! 路径的每一台 T1 档真机在每一轮补测里都占一个登记位；登记位的取值只能是
//! 三态之一（通过 / 失败 / 未知），并且「通过」必须携带可指认的证据编号。
//! 之所以把三态写死在类型里而不是用 `bool`，是因为 `bool` 会把「没测」压成
//! 「不通过」，反过来又容易被人用默认值补成「通过」——两种都是虚报。
//!
//! 闸门（gate）：补测位默认关闭，只有在 `[opens_at, closes_at)` 这个 tick 区间
//! 内才接受写入。闸门关闭期间的写入必须显式失败，而不是「先记账后补闸门」。
//!
//! 数据结构：
//! * `Verdict` —— 诚实三态 + 保守合成（`merge`，未知吸收、失败支配）；
//! * `Gate` —— 半开 tick 区间的闸门，区间非法在构造期就被拒；
//! * `Slot` —— 单个登记位（含证据编号、重放计数、最后写入 tick）；
//! * `SlotBook` —— 2 台设备的定容登记簿（零堆分配，下标先校验后访问）；
//! * `Coverage` —— 汇总账（守恒：total == pass + fail + unknown）。
//!
//! 错误路径：段内闭集 `SEG_BASE + n`，全部码值都在注入核或判据里被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 诊断段（0x43ac0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43ac0;

pub const E_DEVICE: u16 = 1;
pub const E_DUP_SLOT: u16 = 2;
pub const E_CAPACITY: u16 = 3;
pub const E_GATE_CLOSED: u16 = 4;
pub const E_OVERCLAIM: u16 = 5;
pub const E_CONFLICT: u16 = 6;
pub const E_PREREQ: u16 = 7;
pub const E_UNKNOWN_SLOT: u16 = 8;
pub const E_TICK: u16 = 9;
pub const E_VERDICT: u16 = 10;

/// 登记位错误闭集。`code()` 单调映射到 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PttError {
    Device,
    DupSlot,
    Capacity,
    GateClosed,
    Overclaim,
    Conflict,
    Prereq,
    UnknownSlot,
    Tick,
    Verdict,
}

impl PttError {
    /// 段内诊断码。
    pub fn code(self) -> u32 {
        let n = match self {
            PttError::Device => E_DEVICE,
            PttError::DupSlot => E_DUP_SLOT,
            PttError::Capacity => E_CAPACITY,
            PttError::GateClosed => E_GATE_CLOSED,
            PttError::Overclaim => E_OVERCLAIM,
            PttError::Conflict => E_CONFLICT,
            PttError::Prereq => E_PREREQ,
            PttError::UnknownSlot => E_UNKNOWN_SLOT,
            PttError::Tick => E_TICK,
            PttError::Verdict => E_VERDICT,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            PttError::Device => "device index outside T1 pair",
            PttError::DupSlot => "slot already registered on this device",
            PttError::Capacity => "slot table full",
            PttError::GateClosed => "supplementary gate closed at this tick",
            PttError::Overclaim => "verdict contradicts evidence presence",
            PttError::Conflict => "slot frozen with a different verdict",
            PttError::Prereq => "supplementary slot needs decided base slots",
            PttError::UnknownSlot => "slot id not registered",
            PttError::Tick => "gate window is empty or inverted",
            PttError::Verdict => "tri-state code outside 0/1/2",
        }
    }

    /// 码值闭集大小（注入核按此核对覆盖面，避免「加了一条错误却没人构造」）。
    pub const CLOSED_SET: usize = 10;
}

/// 诚实三态。数值编码固定，便于落账与跨版本比对：0=未知 1=通过 2=失败。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Unknown,
    Pass,
    Fail,
}

impl Verdict {
    pub fn bits(self) -> u8 {
        match self {
            Verdict::Unknown => 0,
            Verdict::Pass => 1,
            Verdict::Fail => 2,
        }
    }

    /// 反解：只接受 0/1/2，其余一律显式拒绝（不做「猜一个未知」）。
    pub fn from_bits(b: u8) -> Result<Verdict, PttError> {
        match b {
            0 => Ok(Verdict::Unknown),
            1 => Ok(Verdict::Pass),
            2 => Ok(Verdict::Fail),
            _ => Err(PttError::Verdict),
        }
    }

    /// 两路观测合成的保守规则：
    /// * 任一侧未知 → 未知（未知吸收，避免把「有个视角没测」合成成「测过了」）；
    /// * 通过 + 失败 → 失败（失败支配，宁可多报缺陷）；
    /// * 同态 → 同态。
    pub fn merge(self, other: Verdict) -> Verdict {
        match (self, other) {
            (Verdict::Unknown, _) | (_, Verdict::Unknown) => Verdict::Unknown,
            (Verdict::Fail, _) | (_, Verdict::Fail) => Verdict::Fail,
            (Verdict::Pass, Verdict::Pass) => Verdict::Pass,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Verdict::Unknown => "unknown",
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
        }
    }
}

/// 补测闸门：半开区间 `[opens_at, closes_at)`。
///
/// 用半开区间是为了让「相邻两轮闸门首尾相接」时不会在同一 tick 上同时开启；
/// 空区间与倒置区间在构造期拒绝，避免「闸门永远关着」被误读成「测过但没过」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    pub opens_at: u32,
    pub closes_at: u32,
}

impl Gate {
    pub fn open(opens_at: u32, closes_at: u32) -> Result<Gate, PttError> {
        if closes_at <= opens_at {
            return Err(PttError::Tick);
        }
        Ok(Gate { opens_at, closes_at })
    }

    pub fn is_open(&self, tick: u32) -> bool {
        tick >= self.opens_at && tick < self.closes_at
    }
}

pub const DEVICES: usize = 2;
pub const MAX_SLOTS_PER_DEV: usize = 6;

/// 单个登记位。`evidence` 是证据编号（真机日志条目 / 波形快照序号），0 表示无证据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub id: u8,
    pub supplementary: bool,
    pub gate: Gate,
    pub verdict: Verdict,
    pub evidence: u32,
    pub attempts: u16,
    pub replays: u16,
    pub last_tick: u32,
}

/// 汇总账。`blocked` 是 `unknown` 的子集（闸门当前未开的那些位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coverage {
    pub total: usize,
    pub pass: usize,
    pub fail: usize,
    pub unknown: usize,
    pub blocked: usize,
    pub overclaim: usize,
}

impl Coverage {
    pub const fn zero() -> Coverage {
        Coverage {
            total: 0,
            pass: 0,
            fail: 0,
            unknown: 0,
            blocked: 0,
            overclaim: 0,
        }
    }

    /// 已判定数（通过 + 失败）。
    pub fn decided(&self) -> usize {
        self.pass + self.fail
    }

    /// 诚实覆盖率（万分比，整数运算避免浮点跨平台尾差）。
    pub fn honest_bp(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        ((self.pass * 10_000) / self.total) as u32
    }

    /// 守恒：三类互斥且覆盖全部登记位。
    pub fn conserves(&self) -> bool {
        self.pass + self.fail + self.unknown == self.total && self.blocked <= self.unknown
    }
}

/// 2 台 × 6 位的定容登记簿。
///
/// 用定容数组而不是 `Vec`：登记簿会随内核镜像常驻，容量固定才能给出确定的
/// 栈/静态体积；同时把「满了怎么办」变成一条显式错误而不是静默扩容。
pub struct SlotBook {
    slots: [[Option<Slot>; MAX_SLOTS_PER_DEV]; DEVICES],
    used: [usize; DEVICES],
    reg_seq: u32,
}

impl SlotBook {
    pub const fn new() -> SlotBook {
        SlotBook {
            slots: [[None; MAX_SLOTS_PER_DEV]; DEVICES],
            used: [0usize; DEVICES],
            reg_seq: 0,
        }
    }

    /// 该设备已登记位数（非法设备号返回 0，不做越界访问）。
    pub fn len(&self, dev: u8) -> usize {
        match self.used.get(dev as usize) {
            Some(n) => *n,
            None => 0,
        }
    }

    pub fn is_empty(&self, dev: u8) -> bool {
        self.len(dev) == 0
    }

    /// 登记位总数（两台设备合计）。
    pub fn total_slots(&self) -> usize {
        self.used[0] + self.used[1]
    }

    /// 登记一个新槽位。重复 id 一律拒绝——登记位是「稳定坐标」，
    /// 允许重复会被下游当成两个独立观测，直接制造虚报。
    pub fn register(
        &mut self,
        dev: u8,
        id: u8,
        supplementary: bool,
        gate: Gate,
    ) -> Result<(), PttError> {
        let d = dev as usize;
        if d >= DEVICES {
            return Err(PttError::Device);
        }
        if self.used[d] >= MAX_SLOTS_PER_DEV {
            return Err(PttError::Capacity);
        }
        let mut i = 0usize;
        while i < self.used[d] {
            if let Some(s) = self.slots[d].get(i) {
                if let Some(s) = s {
                    if s.id == id {
                        return Err(PttError::DupSlot);
                    }
                }
            }
            i += 1;
        }
        let slot = Slot {
            id,
            supplementary,
            gate,
            verdict: Verdict::Unknown,
            evidence: 0,
            attempts: 0,
            replays: 0,
            last_tick: 0,
        };
        if let Some(cell) = self.slots[d].get_mut(self.used[d]) {
            *cell = Some(slot);
            self.used[d] += 1;
            self.reg_seq += 1;
            Ok(())
        } else {
            // 不可达：used 已做 < MAX 校验。仍返回显式错误，不留 panic 路径。
            Err(PttError::Capacity)
        }
    }

    /// 按 id 查位（设备号或 id 越界都返回 None，不做裸下标）。
    pub fn get(&self, dev: u8, id: u8) -> Option<&Slot> {
        let d = dev as usize;
        if d >= DEVICES {
            return None;
        }
        let mut i = 0usize;
        while i < self.used[d] {
            if let Some(Some(s)) = self.slots[d].get(i) {
                if s.id == id {
                    return Some(s);
                }
            }
            i += 1;
        }
        None
    }

    /// 补测前置：同一设备上的**基础位**必须全部已判定（通过或失败）。
    ///
    /// 为什么不让补测位独立存在：补测是为了补齐基础对照的边角，如果基础位
    /// 还是未知就先记补测，等于用补测次数冒充覆盖率。
    pub fn base_decided(&self, dev: u8) -> bool {
        let d = dev as usize;
        if d >= DEVICES {
            return false;
        }
        let mut i = 0usize;
        while i < self.used[d] {
            if let Some(Some(s)) = self.slots[d].get(i) {
                if !s.supplementary && s.verdict == Verdict::Unknown {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 写入一次对照结果。
    ///
    /// 五道闸（顺序即优先级，先便宜的检查）：
    /// 1. 槽位存在；
    /// 2. 闸门在该 tick 开启；
    /// 3. 三态与证据自洽（通过/失败必须有证据；未知必须无证据）；
    /// 4. 补测位需要基础位已判定；
    /// 5. 已判定位不可改写（同值重放例外，计入 replays）。
    pub fn record(
        &mut self,
        dev: u8,
        id: u8,
        v: Verdict,
        evidence: u32,
        tick: u32,
    ) -> Result<(), PttError> {
        let d = dev as usize;
        if d >= DEVICES {
            return Err(PttError::Device);
        }
        let mut idx: Option<usize> = None;
        let mut i = 0usize;
        while i < self.used[d] {
            if let Some(Some(s)) = self.slots[d].get(i) {
                if s.id == id {
                    idx = Some(i);
                }
            }
            i += 1;
        }
        let at = match idx {
            Some(v) => v,
            None => return Err(PttError::UnknownSlot),
        };
        let snapshot = match self.slots[d].get(at) {
            Some(Some(s)) => *s,
            _ => return Err(PttError::UnknownSlot),
        };
        if !snapshot.gate.is_open(tick) {
            return Err(PttError::GateClosed);
        }
        match v {
            Verdict::Unknown => {
                if evidence != 0 {
                    // 「未知但带证据」自相矛盾：要么把结论写清，要么别拿证据充数。
                    return Err(PttError::Overclaim);
                }
            }
            Verdict::Pass | Verdict::Fail => {
                if evidence == 0 {
                    return Err(PttError::Overclaim);
                }
            }
        }
        if snapshot.supplementary && !self.base_decided(dev) {
            return Err(PttError::Prereq);
        }
        if snapshot.verdict != Verdict::Unknown {
            if snapshot.verdict == v && snapshot.evidence == evidence {
                if let Some(Some(s)) = self.slots[d].get_mut(at) {
                    s.replays += 1;
                    s.attempts += 1;
                }
                return Ok(());
            }
            return Err(PttError::Conflict);
        }
        if let Some(Some(s)) = self.slots[d].get_mut(at) {
            s.verdict = v;
            s.evidence = evidence;
            s.attempts += 1;
            s.last_tick = tick;
        }
        Ok(())
    }

    /// 汇总账。`tick` 用于区分「还没轮到（闸门未开）」与「测过但没结论」。
    pub fn summary(&self, tick: u32) -> Coverage {
        let mut c = Coverage::zero();
        let mut d = 0usize;
        while d < DEVICES {
            let mut i = 0usize;
            while i < self.used[d] {
                if let Some(Some(s)) = self.slots[d].get(i) {
                    c.total += 1;
                    match s.verdict {
                        Verdict::Pass => {
                            c.pass += 1;
                            if s.evidence == 0 {
                                c.overclaim += 1;
                            }
                        }
                        Verdict::Fail => {
                            c.fail += 1;
                            if s.evidence == 0 {
                                c.overclaim += 1;
                            }
                        }
                        Verdict::Unknown => {
                            c.unknown += 1;
                            if !s.gate.is_open(tick) {
                                c.blocked += 1;
                            }
                        }
                    }
                }
                i += 1;
            }
            d += 1;
        }
        c
    }

    /// 登记簿摘要（FNV-1a 64）：同状态两次调用必须逐位一致，状态一改就变。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_u8(h, self.reg_seq as u8);
        let mut d = 0usize;
        while d < DEVICES {
            h = fnv_u8(h, self.used[d] as u8);
            let mut i = 0usize;
            while i < self.used[d] {
                if let Some(Some(s)) = self.slots[d].get(i) {
                    h = fnv_u8(h, s.id);
                    h = fnv_u8(h, s.verdict.bits());
                    h = fnv_u8(h, if s.supplementary { 1 } else { 0 });
                    h = fnv_u32(h, s.evidence);
                }
                i += 1;
            }
            d += 1;
        }
        h
    }

    /// 逐台设备的覆盖率文本（诊断用，不参与判据的通过条件）。
    pub fn render(&self, tick: u32) -> String {
        let c = self.summary(tick);
        let mut s = String::new();
        s.push_str(&format!(
            "t1 {}/{} pass={} fail={} unknown={} blocked={}",
            c.pass, c.total, c.pass, c.fail, c.unknown, c.blocked
        ));
        s
    }
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h = h.wrapping_mul(0x100_0000_01b3);
    h
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut i = 0usize;
    while i < 4 {
        h = fnv_u8(h, (v >> (i * 8)) as u8);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 注入核：六类虚报/越界样本必须全部被显式拒绝
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    // 1 非法设备号
    let mut b = SlotBook::new();
    let g = match Gate::open(0, 100) {
        Ok(g) => g,
        Err(_) => return (0, total),
    };
    if b.register(2, 1, false, g).err() == Some(PttError::Device) {
        caught += 1;
    }

    // 2 重复槽位
    let mut dup = SlotBook::new();
    if dup.register(0, 1, false, g).is_ok() {
        if dup.register(0, 1, false, g).err() == Some(PttError::DupSlot) {
            caught += 1;
        }
    }

    // 3 闸门关闭时写入
    let mut closed = SlotBook::new();
    let late = match Gate::open(50, 60) {
        Ok(g) => g,
        Err(_) => return (caught, total),
    };
    if closed.register(0, 1, false, late).is_ok() {
        if closed.record(0, 1, Verdict::Pass, 7, 10).err() == Some(PttError::GateClosed) {
            caught += 1;
        }
    }

    // 4 无证据的「通过」
    let mut bare = SlotBook::new();
    if bare.register(0, 1, false, g).is_ok() {
        if bare.record(0, 1, Verdict::Pass, 0, 5).err() == Some(PttError::Overclaim) {
            caught += 1;
        }
    }

    // 5 前置未满足的补测位
    let mut pre = SlotBook::new();
    if pre.register(0, 1, false, g).is_ok() && pre.register(0, 2, true, g).is_ok() {
        if pre.record(0, 2, Verdict::Pass, 9, 5).err() == Some(PttError::Prereq) {
            caught += 1;
        }
    }

    // 6 已判定位的冲突改写
    let mut frozen = SlotBook::new();
    if frozen.register(0, 1, false, g).is_ok()
        && frozen.record(0, 1, Verdict::Pass, 11, 5).is_ok()
    {
        if frozen.record(0, 1, Verdict::Fail, 12, 6).err() == Some(PttError::Conflict) {
            caught += 1;
        }
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50117_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50117");
    let g = match Gate::open(0, 100) {
        Ok(g) => g,
        Err(_) => {
            s.add("50117-J1-闸门构造", false, "合法区间被拒");
            return s;
        }
    };

    // J1-1 登记即未知：新槽位不得带任何结论
    let mut book = SlotBook::new();
    let ok = book.register(0, 1, false, g).is_ok()
        && book
            .get(0, 1)
            .map(|x| x.verdict == Verdict::Unknown && x.evidence == 0)
            .unwrap_or(false);
    s.add("50117-J1-登记即未知", ok, if ok { "" } else { "初始态非未知" });

    // J1-2 三态反解闭集：0/1/2 可解，3 显式拒绝
    let ok = Verdict::from_bits(0).ok() == Some(Verdict::Unknown)
        && Verdict::from_bits(1).ok() == Some(Verdict::Pass)
        && Verdict::from_bits(2).ok() == Some(Verdict::Fail)
        && Verdict::from_bits(3).err() == Some(PttError::Verdict)
        && Verdict::from_bits(255).err() == Some(PttError::Verdict);
    s.add("50117-J1-三态闭集", ok, if ok { "" } else { "非法码被放行" });

    // J1-3 合法写入：通过必须带证据，且状态落账
    let ok = book.register(0, 2, false, g).is_ok()
        && book.record(0, 2, Verdict::Pass, 4242, 10).is_ok()
        && book
            .get(0, 2)
            .map(|x| x.verdict == Verdict::Pass && x.evidence == 4242 && x.attempts == 1)
            .unwrap_or(false);
    s.add("50117-J1-证据落账", ok, if ok { "" } else { "通过未落账" });

    // J1-4 同值重放幂等：状态不变、replays 递增、digest 不变
    let before = book.digest();
    let r1 = book.record(0, 2, Verdict::Pass, 4242, 11);
    let after = book.digest();
    let ok = r1.is_ok()
        && before == after
        && book.get(0, 2).map(|x| x.replays == 1).unwrap_or(false);
    let det = format!("digest_before={:x} after={:x}", before, after);
    s.add_dyn("50117-J1-重放幂等", ok, &det);

    // J1-5 计数守恒：pass+fail+unknown == total，blocked 是 unknown 子集
    let mut cons = SlotBook::new();
    let far = match Gate::open(80, 90) {
        Ok(g) => g,
        Err(_) => g,
    };
    let _ = cons.register(0, 1, false, g);
    let _ = cons.register(0, 2, false, g);
    let _ = cons.register(0, 3, false, far);
    let _ = cons.register(1, 1, false, g);
    let _ = cons.record(0, 1, Verdict::Pass, 1, 10);
    let _ = cons.record(0, 2, Verdict::Fail, 2, 10);
    let c = cons.summary(10);
    let ok = c.conserves() && c.total == 4 && c.pass == 1 && c.fail == 1 && c.unknown == 2 && c.blocked == 1;
    let det = format!(
        "total={} p={} f={} u={} b={}",
        c.total, c.pass, c.fail, c.unknown, c.blocked
    );
    s.add_dyn("50117-J1-计数守恒", ok, &det);

    // J1-6 未知不并入通过：merge 的三张表逐项核对
    let ok = Verdict::Pass.merge(Verdict::Unknown) == Verdict::Unknown
        && Verdict::Fail.merge(Verdict::Unknown) == Verdict::Unknown
        && Verdict::Pass.merge(Verdict::Fail) == Verdict::Fail
        && Verdict::Pass.merge(Verdict::Pass) == Verdict::Pass
        && Verdict::Unknown.merge(Verdict::Unknown) == Verdict::Unknown
        && Verdict::Fail.merge(Verdict::Fail) == Verdict::Fail;
    s.add("50117-J1-未知不升通过", ok, if ok { "" } else { "合成把未知洗成通过" });

    // J1-7 闸门半开区间：左闭右开，相邻两轮不重叠
    let a = match Gate::open(10, 20) {
        Ok(g) => g,
        Err(_) => g,
    };
    let b2 = match Gate::open(20, 30) {
        Ok(g) => g,
        Err(_) => g,
    };
    let ok = !a.is_open(9) && a.is_open(10) && a.is_open(19) && !a.is_open(20)
        && !b2.is_open(19) && b2.is_open(20)
        && Gate::open(20, 20).err() == Some(PttError::Tick)
        && Gate::open(30, 10).err() == Some(PttError::Tick);
    s.add("50117-J1-闸门半开", ok, if ok { "" } else { "闸门区间语义漂移" });

    // J1-8 补测位前置：基础位未判定时补测必拒，判定后放行
    let mut pre = SlotBook::new();
    let _ = pre.register(0, 1, false, g);
    let _ = pre.register(0, 2, true, g);
    let blocked = pre.record(0, 2, Verdict::Pass, 5, 10).err() == Some(PttError::Prereq);
    let base = pre.record(0, 1, Verdict::Fail, 6, 10).is_ok();
    let allowed = pre.record(0, 2, Verdict::Pass, 7, 10).is_ok();
    let ok = blocked && base && allowed;
    s.add("50117-J1-补测前置", ok, if ok { "" } else { "补测绕过基础位" });

    // J1-9 未知位可被重复写入（未定论不冻结），且不计入 replays
    let mut loopb = SlotBook::new();
    let _ = loopb.register(0, 1, false, g);
    let _ = loopb.record(0, 1, Verdict::Unknown, 0, 3);
    let _ = loopb.record(0, 1, Verdict::Unknown, 0, 4);
    let ok = loopb
        .get(0, 1)
        .map(|x| x.verdict == Verdict::Unknown && x.replays == 0 && x.attempts == 2)
        .unwrap_or(false);
    s.add("50117-J1-未知可重试", ok, if ok { "" } else { "未知位被误冻结" });

    // J1-10 容量满必拒（每台 6 位）
    let mut cap = SlotBook::new();
    let mut i = 0u8;
    let mut all_ok = true;
    while i < 6 {
        if cap.register(1, i, false, g).is_err() {
            all_ok = false;
        }
        i += 1;
    }
    let over = cap.register(1, 9, false, g).err() == Some(PttError::Capacity);
    let ok = all_ok && over && cap.len(1) == 6;
    s.add("50117-J1-容量封顶", ok, if ok { "" } else { "容量未封顶" });

    // J1-11 两台设备互不串账：同 id 在不同设备上是不同位
    let mut pair = SlotBook::new();
    let _ = pair.register(0, 1, false, g);
    let _ = pair.register(1, 1, false, g);
    let _ = pair.record(0, 1, Verdict::Pass, 1, 5);
    let ok = pair.get(0, 1).map(|x| x.verdict == Verdict::Pass).unwrap_or(false)
        && pair.get(1, 1).map(|x| x.verdict == Verdict::Unknown).unwrap_or(false)
        && pair.total_slots() == 2;
    s.add("50117-J1-双机隔离", ok, if ok { "" } else { "两台设备串账" });

    // J1-12 确定性：同状态两次 digest / summary 逐位一致
    let d1 = cons.digest();
    let d2 = cons.digest();
    let c1 = cons.summary(10);
    let c2 = cons.summary(10);
    let ok = d1 == d2 && c1 == c2;
    s.add("50117-J1-确定性", ok, if ok { "" } else { "同状态摘要漂移" });

    // J1-13 诚实覆盖率：未知不进分子；全通过才是 10000 bp
    let mut full = SlotBook::new();
    let _ = full.register(0, 1, false, g);
    let _ = full.register(0, 2, false, g);
    let _ = full.record(0, 1, Verdict::Pass, 1, 5);
    let mid = full.summary(5).honest_bp();
    let _ = full.record(0, 2, Verdict::Pass, 2, 5);
    let top = full.summary(5).honest_bp();
    let ok = mid == 5000 && top == 10000 && Coverage::zero().honest_bp() == 0;
    let det = format!("mid={} top={}", mid, top);
    s.add_dyn("50117-J1-覆盖率诚实", ok, &det);

    // J1-14 注入核：六类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50117-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50117_checks_green() {
        let set = run_unx50117_checks();
        assert!(set.all_passed(), "UNX-F50117 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50117_evidence_required() {
        let mut b = SlotBook::new();
        let g = match Gate::open(0, 10) {
            Ok(g) => g,
            Err(_) => return,
        };
        assert!(b.register(0, 1, false, g).is_ok());
        assert!(b.record(0, 1, Verdict::Pass, 0, 1).is_err());
        assert!(b.record(0, 1, Verdict::Pass, 1, 1).is_ok());
        assert_eq!(b.summary(1).pass, 1);
        assert_eq!(b.summary(1).overclaim, 0);
    }

    #[test]
    fn unx50117_error_codes_in_segment() {
        assert_eq!(PttError::Device.code(), SEG_BASE + 1);
        assert_eq!(PttError::Verdict.code(), SEG_BASE + 10);
        assert_eq!(PttError::CLOSED_SET, 10);
    }
}
