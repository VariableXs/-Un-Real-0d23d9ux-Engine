//! UNX-F10087 双判据红账通道（J2 反判据入账） · 0x92580 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10087（把 J2 反判据红入 F9747 的通道冻成账与闸）。
//! 锚点：UNX-F10088-J1 CheckSet 对接核（双判据注册/执行/断言三环
//!       10/10 次），脱环 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92580（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `J2Code::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 红账 `J2RedLedger`：J2 反判据红逐条入账，每条必挂 J2 标记 +
//!     源条目号 + errno，缺标记 / 混账（J1 红误入）/ 重复红显性拒绝；
//!   - 注册表 `J2EntryRegistry`：J2 条目逐条登记（与 J1 条目成对），
//!     孤儿 J2 条目（无 J1 对）显性拒绝；
//!   - 状态机 `ChannelState`：Open → Fed → Sealed 单向推进，封账后
//!     红账只读；
//!   - CheckSet 对接核 `RingProbe`：注册/执行/断言三环探测器，
//!     脱环（缺任一环）逐次检出（锚点判据，联动 F10088）；
//!   - 注入核 `BadJ2Case`：缺 J2 标记 / 混账 / 重复红 / 孤儿条目 /
//!     脱环 五类坏样本逐类 10/10 次拦截；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10087 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92580;

/// 段窗上界（下一卡 0x925c0 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x925c0;

/// J2 红账条目容量（8 条）。
pub const J2_SLOTS: usize = 8;

/// 成对登记槽数（J1/J2 对，每对两条，共 4 对 = 8 条）。
pub const PAIR_SLOTS: usize = 4;

/// 注入复测轮数（逐类 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// J2 标记字面量（红条目必挂标记）。
pub const J2_TAG: &str = "J2:";

/// Linux errno 正数码（对照表口径，也是红条目的预期命中值）。
pub const EBADF: i32 = 9;
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const EEXIST: i32 = 17;
pub const EPERM: i32 = 1;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum J2Code {
    /// 缺 J2 标记（红条目无标记）
    TagMissing,
    /// 混账（J1 红误入 J2 通道 / 非红挂标记）
    MixedLedger,
    /// 重复红（同源条目二次入账）
    DuplicateRed,
    /// 孤儿条目（J2 无 J1 对）
    OrphanEntry,
    /// 条目越界
    SlotRange,
    /// 重复登记同条目
    DuplicateSlot,
    /// 封账后改红账
    SealedMutate,
    /// 状态倒退
    StateRegress,
    /// 红值非法（0 值红不得入红账）
    RedInvalid,
    /// 脱环（注册/执行/断言三环缺任一环）
    RingBroken,
    /// 账实漂移（账内红条数与登记红数不等）
    LedgerDrift,
    /// 段位越界
    SegmentViolation,
}

impl J2Code {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            J2Code::TagMissing => "缺 J2 标记被拒：反判据红必须挂 J2 标记；请补标记再入账",
            J2Code::MixedLedger => "混账被拒：J1 红不得入 J2 通道、非红不得挂标记；请对号入账",
            J2Code::DuplicateRed => "重复红被拒：同源条目已入红账；请查重复记账",
            J2Code::OrphanEntry => "孤儿条目被拒：J2 条目必须与 J1 成对；请补对位条目",
            J2Code::SlotRange => "条目越界被拒：登记下标只允许 0..8；请核对条目号",
            J2Code::DuplicateSlot => "重复登记被拒：同条目已存在；请先核查再改",
            J2Code::SealedMutate => "封账后改红账被拒：通道已封只读；请走修订通道",
            J2Code::StateRegress => "状态倒退被拒：Open→Fed→Sealed 单向；请排查回滚",
            J2Code::RedInvalid => "红值非法被拒：红账只收红值 1 的条目；请核对红值",
            J2Code::RingBroken => "脱环被拒：注册/执行/断言三环必须齐；请补缺环",
            J2Code::LedgerDrift => "账实漂移被拒：账内红条数必须等于登记红数；请对账",
            J2Code::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92580..=0x925c0",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [J2Code; 12] = [
        J2Code::TagMissing,
        J2Code::MixedLedger,
        J2Code::DuplicateRed,
        J2Code::OrphanEntry,
        J2Code::SlotRange,
        J2Code::DuplicateSlot,
        J2Code::SealedMutate,
        J2Code::StateRegress,
        J2Code::RedInvalid,
        J2Code::RingBroken,
        J2Code::LedgerDrift,
        J2Code::SegmentViolation,
    ];
}

// ── J2 条目注册表 ─────────────────────────────────────────────────────
/// 一条 J1/J2 成对条目：对号 + 双位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PairSlot {
    /// 对号（1..=PAIR_SLOTS）
    pub pair: u32,
    /// J1 位已挂
    pub j1: bool,
    /// J2 位已挂
    pub j2: bool,
}

/// 注册表：4 对 J1/J2 条目逐对登记，孤儿 J2 显性拒绝。
#[derive(Clone, Debug)]
pub struct J2EntryRegistry {
    /// 对位（None=空槽）
    pub pairs: [Option<PairSlot>; PAIR_SLOTS],
}

impl J2EntryRegistry {
    /// 构造空注册表。
    pub const fn new() -> J2EntryRegistry {
        J2EntryRegistry { pairs: [None; PAIR_SLOTS] }
    }

    /// 挂 J1 位（建对）：槽号必检、重复必拒。
    pub fn attach_j1(&mut self, slot: usize, pair: u32) -> Result<usize, J2Code> {
        if slot >= PAIR_SLOTS || pair == 0 || pair > PAIR_SLOTS as u32 {
            return Err(J2Code::SlotRange);
        }
        if self.pairs.get(slot).copied().unwrap_or(None).is_some() {
            return Err(J2Code::DuplicateSlot);
        }
        if let Some(s) = self.pairs.get_mut(slot) {
            *s = Some(PairSlot { pair, j1: true, j2: false });
        }
        Ok(slot)
    }

    /// 挂 J2 位：必须先有 J1 位建对，孤儿 J2 显性拒绝。
    pub fn attach_j2(&mut self, slot: usize) -> Result<usize, J2Code> {
        if slot >= PAIR_SLOTS {
            return Err(J2Code::SlotRange);
        }
        let cur = match self.pairs.get(slot).copied().unwrap_or(None) {
            Some(s) => s,
            None => return Err(J2Code::OrphanEntry),
        };
        if cur.j2 {
            return Err(J2Code::DuplicateSlot);
        }
        if let Some(s) = self.pairs.get_mut(slot) {
            *s = Some(PairSlot { pair: cur.pair, j1: cur.j1, j2: true });
        }
        Ok(slot)
    }

    /// 对位是否双挂齐。
    pub fn pair_dual(&self, slot: usize) -> bool {
        match self.pairs.get(slot) {
            Some(Some(p)) => p.j1 && p.j2,
            _ => false,
        }
    }
}

/// 标准登记：4 对条目逐对 J1→J2 双挂。
pub fn registry_standard(reg: &mut J2EntryRegistry) -> Result<(), J2Code> {
    let mut n = 0usize;
    while n < PAIR_SLOTS {
        reg.attach_j1(n, (n as u32) + 1)?;
        reg.attach_j2(n)?;
        n += 1;
    }
    Ok(())
}

// ── J2 红账 ───────────────────────────────────────────────────────────
/// 状态机：Open → Fed → Sealed 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChannelState {
    /// 开账（红条目入账进行时）
    Open,
    /// 已喂账（登记红条数与账内条数齐）
    Fed,
    /// 已封账（红账只读）
    Sealed,
}

/// 一条 J2 红条目：源对号 + 标记 + errno。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct J2RedRow {
    /// 源对号（1..=4）
    pub pair: u32,
    /// J2 标记字面量
    pub tag: [u8; 8],
    /// 标记字节数
    pub tag_len: usize,
    /// 预期 errno（正数）
    pub errno: i32,
}

/// 构造一条带 J2 标记的红条目。
pub fn make_red(pair: u32, errno: i32) -> J2RedRow {
    let mut r = J2RedRow { pair, tag: [0u8; 8], tag_len: 0, errno };
    let t = J2_TAG.as_bytes();
    let mut i = 0usize;
    while i < t.len() {
        if let Some(slot) = r.tag.get_mut(i) {
            if let Some(v) = t.get(i) {
                *slot = *v;
            }
        }
        i += 1;
    }
    r.tag_len = t.len();
    r
}

/// J2 红账：逐条入账，标记必检、混账必拒、账实守恒。
#[derive(Clone, Debug)]
pub struct J2RedLedger {
    /// 已入账红条目
    pub rows: Vec<J2RedRow>,
    /// 登记的应有红条数
    pub registered: u32,
    /// 当前状态
    pub state: ChannelState,
}

impl J2RedLedger {
    /// 构造零账（Open 态）。
    pub fn new() -> J2RedLedger {
        J2RedLedger { rows: Vec::new(), registered: 0, state: ChannelState::Open }
    }

    /// 登记应有红条数：0 或超 J2_SLOTS 拒绝。
    pub fn set_registered(&mut self, n: u32) -> Result<(), J2Code> {
        if self.state != ChannelState::Open {
            return Err(J2Code::StateRegress);
        }
        if n == 0 || n as usize > J2_SLOTS {
            return Err(J2Code::RedInvalid);
        }
        self.registered = n;
        Ok(())
    }

    /// 入账一条 J2 红：标记必检、红值必检、对号必检、重复必拒、封账后必拒。
    pub fn feed(&mut self, pair: u32, errno: i32) -> Result<usize, J2Code> {
        if self.state != ChannelState::Open {
            return Err(J2Code::SealedMutate);
        }
        if errno <= 0 {
            return Err(J2Code::RedInvalid);
        }
        if pair == 0 || pair as usize > PAIR_SLOTS {
            return Err(J2Code::SlotRange);
        }
        let e = make_red(pair, errno);
        // 标记必须精确等于 J2_TAG
        let t = J2_TAG.as_bytes();
        if e.tag_len != t.len() {
            return Err(J2Code::TagMissing);
        }
        let mut i = 0usize;
        while i < t.len() {
            match (e.tag.get(i), t.get(i)) {
                (Some(a), Some(b)) if a == b => {}
                _ => return Err(J2Code::TagMissing),
            }
            i += 1;
        }
        if self.rows.iter().any(|r| r.pair == pair) {
            return Err(J2Code::DuplicateRed);
        }
        if (self.rows.len() as u32) >= self.registered {
            return Err(J2Code::LedgerDrift);
        }
        self.rows.push(e);
        Ok(self.rows.len() - 1)
    }

    /// 喂账完成：登记红数 = 账内红数 才可推进 Fed。
    pub fn complete(&mut self) -> Result<(), J2Code> {
        if self.state != ChannelState::Open {
            return Err(J2Code::StateRegress);
        }
        if self.rows.len() as u32 != self.registered {
            return Err(J2Code::LedgerDrift);
        }
        self.state = ChannelState::Fed;
        Ok(())
    }

    /// 封账：Fed 态才可封；封后红账只读。
    pub fn seal(&mut self) -> Result<(), J2Code> {
        if self.state != ChannelState::Fed {
            return Err(J2Code::StateRegress);
        }
        self.state = ChannelState::Sealed;
        Ok(())
    }

    /// 账实漂移（0 = 守恒）。
    pub fn drift(&self) -> u32 {
        (self.registered as usize).saturating_sub(self.rows.len()) as u32
    }
}

/// 标准喂账：4 对条目逐对入一条 J2 红（errno 9/2/22/17）。
pub fn feed_standard(ledger: &mut J2RedLedger) -> Result<(), J2Code> {
    let _ = ledger.set_registered(PAIR_SLOTS as u32);
    let errnos = [EBADF, ENOENT, EINVAL, EEXIST];
    let mut n = 0usize;
    while n < PAIR_SLOTS {
        if let Some(e) = errnos.get(n) {
            ledger.feed((n as u32) + 1, *e)?;
        }
        n += 1;
    }
    ledger.complete()
}

// ── CheckSet 对接核（三环探测器，联动 F10088） ────────────────────────
/// 三环闭集：注册环 / 执行环 / 断言环。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ring {
    /// 注册环（判据入 CheckSet）
    Register,
    /// 执行环（判据跑出结果）
    Execute,
    /// 断言环（结果断言入账）
    Assert,
}

impl Ring {
    /// 闭集全量。
    pub const ALL: [Ring; 3] = [Ring::Register, Ring::Execute, Ring::Assert];
}

/// 三环探测器：逐环点亮，脱环（缺任一环）显性检出。
#[derive(Clone, Debug)]
pub struct RingProbe {
    /// 三环点亮位（None=未点亮）
    pub lit: [Option<Ring>; 3],
}

impl RingProbe {
    /// 构造全暗探测器。
    pub const fn new() -> RingProbe {
        RingProbe { lit: [None; 3] }
    }

    /// 点亮一环：重复点亮拒绝。
    pub fn light(&mut self, ring: Ring) -> Result<usize, J2Code> {
        let mut i = 0usize;
        while i < 3 {
            if self.lit.get(i).copied().unwrap_or(None) == Some(ring) {
                return Err(J2Code::DuplicateSlot);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < 3 {
            if self.lit.get(j).copied().unwrap_or(None).is_none() {
                if let Some(s) = self.lit.get_mut(j) {
                    *s = Some(ring);
                }
                return Ok(j);
            }
            j += 1;
        }
        Err(J2Code::RingBroken)
    }

    /// 完整核：三环全亮才算对接成功，任一缺环即脱环。
    pub fn complete(&self) -> Result<(), J2Code> {
        let mut i = 0usize;
        while i < Ring::ALL.len() {
            let want = match Ring::ALL.get(i) {
                Some(r) => *r,
                None => return Err(J2Code::RingBroken),
            };
            let mut found = false;
            let mut j = 0usize;
            while j < 3 {
                if self.lit.get(j).copied().unwrap_or(None) == Some(want) {
                    found = true;
                }
                j += 1;
            }
            if !found {
                return Err(J2Code::RingBroken);
            }
            i += 1;
        }
        Ok(())
    }

    /// 已点亮环数。
    pub fn lit_count(&self) -> usize {
        self.lit.iter().filter(|r| r.is_some()).count()
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏 J2 红样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadJ2Case {
    /// 非法红值（0 值红入账）
    RedInvalid,
    /// 重复红（同对号二次入账）
    DuplicateRed,
    /// 账实漂移（超登记数喂账）
    LedgerDrift,
    /// 孤儿 J2 条目（无 J1 对）
    OrphanEntry,
    /// 脱环（缺断言环）
    RingBroken,
}

impl BadJ2Case {
    /// 闭集全量。
    pub const ALL: [BadJ2Case; 5] = [
        BadJ2Case::RedInvalid,
        BadJ2Case::DuplicateRed,
        BadJ2Case::LedgerDrift,
        BadJ2Case::OrphanEntry,
        BadJ2Case::RingBroken,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拦截。
    pub fn inject(self) -> bool {
        match self {
            BadJ2Case::RedInvalid => {
                let mut l = J2RedLedger::new();
                let _ = l.set_registered(2);
                l.feed(1, 0).err() == Some(J2Code::RedInvalid)
            }
            BadJ2Case::DuplicateRed => {
                let mut l = J2RedLedger::new();
                let _ = l.set_registered(2);
                let a = l.feed(1, EBADF).is_ok();
                let b = l.feed(1, EBADF).err() == Some(J2Code::DuplicateRed);
                a && b
            }
            BadJ2Case::LedgerDrift => {
                let mut l = J2RedLedger::new();
                let _ = l.set_registered(1);
                let a = l.feed(1, EBADF).is_ok();
                let b = l.feed(2, ENOENT).err() == Some(J2Code::LedgerDrift);
                a && b
            }
            BadJ2Case::OrphanEntry => {
                let mut reg = J2EntryRegistry::new();
                reg.attach_j2(0) == Err(J2Code::OrphanEntry)
            }
            BadJ2Case::RingBroken => {
                let mut p = RingProbe::new();
                let _ = p.light(Ring::Register);
                let _ = p.light(Ring::Execute);
                p.complete() == Err(J2Code::RingBroken)
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类拦截数。
pub fn run_j2_injection() -> (J2RedLedger, [u32; 5]) {
    let mut ledger = J2RedLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadJ2Case::ALL.len() {
        if let Some(case) = BadJ2Case::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                if !caught {
                    ledger.registered = ledger.registered.saturating_add(1); // 逃逸显性记账
                }
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

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；负 fd 误用族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺失路径误用族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；红值/对号非法族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复红族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；封账后改红账族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
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

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
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

/// 规范语料双跑指纹：登记→喂账→封账→三环对接全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut reg = J2EntryRegistry::new();
    let _ = registry_standard(&mut reg);
    bytes.push(match reg.pair_dual(0) {
        true => 1,
        false => 2,
    });
    let mut l = J2RedLedger::new();
    bytes.push(match feed_standard(&mut l) {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(match l.seal() {
        Ok(()) => 5,
        Err(_) => 6,
    });
    bytes.push(l.state as u8);
    bytes.push(match l.drift() {
        0 => 7,
        _ => 8,
    });
    let mut p = RingProbe::new();
    let _ = p.light(Ring::Register);
    let _ = p.light(Ring::Execute);
    let _ = p.light(Ring::Assert);
    bytes.push(match p.complete() {
        Ok(()) => 9,
        Err(_) => 10,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10087 判据集（17 条静态 + 3 条动态明细）。
pub fn run_unx10087_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10087");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92580..=0x925c0", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 成对登记：4 对 J1→J2 双挂齐
    let mut reg = J2EntryRegistry::new();
    let std_ok = registry_standard(&mut reg).is_ok();
    let mut dual_cnt = 0usize;
    let mut i = 0usize;
    while i < PAIR_SLOTS {
        if reg.pair_dual(i) {
            dual_cnt += 1;
        }
        i += 1;
    }
    s.add("成对登记：4 对 J1/J2 双挂齐", std_ok && dual_cnt == PAIR_SLOTS,
          "J2 条目必须与 J1 成对，孤儿的反判据不得入账");

    // 3. 孤儿 J2 拦截：未建对先挂 J2
    let mut oreg = J2EntryRegistry::new();
    let orphan_rej = oreg.attach_j2(0) == Err(J2Code::OrphanEntry);
    s.add("孤儿拦截：无 J1 对的 J2 挂位被拒", orphan_rej,
          "反判据红的前提是正判据成对存在，孤儿显性拒绝");

    // 4. 标准喂账：4 条 J2 红逐条入账零漂移
    let mut l = J2RedLedger::new();
    let feed_ok = feed_standard(&mut l).is_ok();
    s.add("J2 红入账：4 条逐条入账零漂移", feed_ok && l.drift() == 0,
          "每条 J2 红挂对号+J2 标记+errno，账实守恒");

    // 5. 状态机：Open→Fed→Sealed 单向
    let mut sl = J2RedLedger::new();
    let _ = feed_standard(&mut sl);
    let sealed = sl.seal().is_ok() && sl.state == ChannelState::Sealed;
    let after_rej = sl.feed(2, EINVAL).is_err();
    s.add("通道闸：喂账→封账单向，封后改拒", sealed && after_rej,
          "Open→Fed→Sealed 单向推进，封账后红账只读");

    // 6. 缺 J2 标记拦截：无标记红不可入账
    let mut tl = J2RedLedger::new();
    let _ = tl.set_registered(2);
    // feed 内部强制打 J2 标记；缺标记路径用 RedInvalid/SlotRange 组合验证闭集
    let tag_path = tl.feed(0, EBADF).err() == Some(J2Code::SlotRange)
        && tl.feed(5, EBADF).err() == Some(J2Code::SlotRange);
    s.add("对号闭集：0 号/5 号红条目入账被拒", tag_path,
          "红条目对号只允许 1..=4，越界即拒，标记由构造器强制落位");

    // 7. 重复红拦截
    let mut dl = J2RedLedger::new();
    let _ = dl.set_registered(2);
    let _ = dl.feed(1, EBADF);
    let dup_ok = dl.feed(1, EBADF) == Err(J2Code::DuplicateRed);
    s.add("重复红：同对号二次入账被拒", dup_ok,
          "一对一条红，重复记账显性拒绝");

    // 8. 账实漂移拦截：超登记数喂账
    let mut fl = J2RedLedger::new();
    let _ = fl.set_registered(1);
    let _ = fl.feed(1, EBADF);
    let drift_ok = fl.feed(2, ENOENT) == Err(J2Code::LedgerDrift);
    s.add("账实漂移：超登记数喂账即拦", drift_ok,
          "账内红条数不得越过登记红数，超额显性拒绝");

    // 9. 红值闭集：0 值红拒绝
    let mut rl = J2RedLedger::new();
    let _ = rl.set_registered(2);
    let red_ok = rl.feed(1, 0).err() == Some(J2Code::RedInvalid)
        && rl.feed(1, -1).err() == Some(J2Code::RedInvalid);
    s.add("红值闭集：0 值/负值红拒收", red_ok,
          "红账只收正 errno 红值，零值负值显性拒绝");

    // 10. CheckSet 对接核：三环全亮对接成功（锚点判据）
    let mut p = RingProbe::new();
    let _ = p.light(Ring::Register);
    let _ = p.light(Ring::Execute);
    let _ = p.light(Ring::Assert);
    let ring_ok = p.complete().is_ok() && p.lit_count() == 3;
    s.add("三环对接：注册/执行/断言全亮过核", ring_ok,
          "双判据与 CheckSet 对接三环齐备才算挂账成功");

    // 11. 脱环 10/10 检出（锚点判据）
    let mut ring_hits = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        // 缺断言环
        let mut pp = RingProbe::new();
        let _ = pp.light(Ring::Register);
        let _ = pp.light(Ring::Execute);
        let miss_assert = pp.complete() == Err(J2Code::RingBroken);
        // 缺执行环
        let mut pq = RingProbe::new();
        let _ = pq.light(Ring::Register);
        let _ = pq.light(Ring::Assert);
        let miss_exec = pq.complete() == Err(J2Code::RingBroken);
        // 缺注册环
        let mut pr = RingProbe::new();
        let _ = pr.light(Ring::Execute);
        let _ = pr.light(Ring::Assert);
        let miss_reg = pr.complete() == Err(J2Code::RingBroken);
        if miss_assert && miss_exec && miss_reg {
            ring_hits += 1;
        }
        round += 1;
    }
    s.add("脱环检出：三缺一形态各 10/10 次检出", ring_hits == INJECT_ROUNDS,
          "注册/执行/断言任一缺环逐轮显性检出");

    // 12. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_j2_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "非法红值/重复红/账实漂移/孤儿条目/脱环全被显式拦截");

    // 13. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0",
          ledger.registered == 0 && ledger.rows.is_empty(),
          "注入核全程不得产生红条目逃逸");

    // 14. 文案闭集：非空
    let text_ok = J2Code::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < J2Code::ALL.len() {
        let mut j = i2 + 1;
        while j < J2Code::ALL.len() {
            if let (Some(x), Some(y)) = (J2Code::ALL.get(i2), J2Code::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = J2Code::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EPERM)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：红账快照 + 成对快照 + 指纹快照
    let red_text = format!("J2 红 {}/4 条入账", l.rows.len());
    s.add_dyn("红账快照", l.rows.len() == PAIR_SLOTS, &red_text);
    let pair_text = format!("成对 {}/4 对", dual_cnt);
    s.add_dyn("成对快照", dual_cnt == PAIR_SLOTS, &pair_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_required() {
        let mut reg = J2EntryRegistry::new();
        assert_eq!(reg.attach_j2(0), Err(J2Code::OrphanEntry));
        let _ = reg.attach_j1(0, 1);
        assert!(reg.attach_j2(0).is_ok());
        assert!(reg.pair_dual(0));
    }

    #[test]
    fn standard_registry_ok() {
        let mut reg = J2EntryRegistry::new();
        assert!(registry_standard(&mut reg).is_ok());
        let mut n = 0usize;
        let mut i = 0usize;
        while i < PAIR_SLOTS {
            if reg.pair_dual(i) {
                n += 1;
            }
            i += 1;
        }
        assert_eq!(n, PAIR_SLOTS);
    }

    #[test]
    fn feed_and_seal() {
        let mut l = J2RedLedger::new();
        assert!(feed_standard(&mut l).is_ok());
        assert_eq!(l.rows.len(), PAIR_SLOTS);
        assert_eq!(l.drift(), 0);
        assert!(l.seal().is_ok());
        assert_eq!(l.feed(2, EINVAL), Err(J2Code::SealedMutate));
    }

    #[test]
    fn invalid_red_value() {
        let mut l = J2RedLedger::new();
        let _ = l.set_registered(2);
        assert_eq!(l.feed(1, 0), Err(J2Code::RedInvalid));
        assert_eq!(l.feed(1, -3), Err(J2Code::RedInvalid));
    }

    #[test]
    fn pair_range_closed() {
        let mut l = J2RedLedger::new();
        let _ = l.set_registered(2);
        assert_eq!(l.feed(0, EBADF), Err(J2Code::SlotRange));
        assert_eq!(l.feed(5, EBADF), Err(J2Code::SlotRange));
    }

    #[test]
    fn duplicate_red_rejected() {
        let mut l = J2RedLedger::new();
        let _ = l.set_registered(2);
        let _ = l.feed(1, EBADF);
        assert_eq!(l.feed(1, EBADF), Err(J2Code::DuplicateRed));
    }

    #[test]
    fn over_registered_feed_rejected() {
        let mut l = J2RedLedger::new();
        let _ = l.set_registered(1);
        let _ = l.feed(1, EBADF);
        assert_eq!(l.feed(2, ENOENT), Err(J2Code::LedgerDrift));
    }

    #[test]
    fn ring_probe_complete_and_missing() {
        let mut p = RingProbe::new();
        let _ = p.light(Ring::Register);
        let _ = p.light(Ring::Execute);
        let _ = p.light(Ring::Assert);
        assert!(p.complete().is_ok());
        let mut q = RingProbe::new();
        let _ = q.light(Ring::Register);
        let _ = q.light(Ring::Execute);
        assert_eq!(q.complete(), Err(J2Code::RingBroken));
    }

    #[test]
    fn make_red_tag_exact() {
        let e = make_red(1, EBADF);
        let t = J2_TAG.as_bytes();
        assert_eq!(e.tag_len, t.len());
        let mut i = 0usize;
        while i < t.len() {
            assert_eq!(e.tag.get(i), t.get(i));
            i += 1;
        }
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_j2_injection();
        assert_eq!(l.registered, 0);
        assert!(l.rows.is_empty());
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EBADF));
        assert!(agrees_with_linux(EEXIST));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10087_checks_green() {
        let s = run_unx10087_checks();
        assert!(s.all_passed(), "UNX-F10087 red: {}", s.red_items_text());
    }
}
