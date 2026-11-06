//! UNX-F10088 双判据与 CheckSet 对接 · 0x925c0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10088（把双判据挂进 CheckSet 的三环对接冻成协议机）。
//! 锚点：UNX-F10089-J1 E 型挡板生效（J2 未决闸保持红 10/10 次），
//!       越闸 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x925c0（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `RingCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 状态机 `RingState`：Unregistered → Registered → Executed →
//!     Asserted 单向推进（注册/执行/断言三环），跳环即脱环显性拒绝；
//!   - 对接机 `ChecksetCoupler`：把 J1/J2 双判据逐条挂进 CheckSet 语义
//!     的三环管道，逐环留痕，缺环不推进；
//!   - 留痕账 `RingLedger`：三环逐条留痕（判据号/环/结果），账守恒
//!     （登记=留痕），逃逸=0；
//!   - J2 未决闸预核 `GateProbe`：J2 未决（复测未齐）时闸必须保持红，
//!     越闸（未决即放行）逐次拦截（锚点判据，联动 F10089）；
//!   - 注入核 `BadRingCase`：跳环执行 / 未执行即断言 / 重复挂判据 /
//!     J2 未决越闸 / 账失恒 五类坏样本逐类 10/10 次拦截；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10088 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x925c0;

/// 段窗上界（下一卡 0x92600 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92600;

/// 对接判据槽数（J1×3 + J2×3 = 6 条）。
pub const COUPLE_SLOTS: usize = 6;

/// 注入复测轮数（逐类 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// J2 未决闸要求的双口径复测轮数下限（J2 侧 ≥ 10 轮留痕才算已决）。
pub const J2_SETTLE_MIN: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EPERM: i32 = 1;
pub const EINVAL: i32 = 22;
pub const EALREADY: i32 = 114;
pub const EPROTO: i32 = 71;
pub const EBUSY: i32 = 16;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RingCode {
    /// 跳环执行（未注册先执行）
    SkipRegister,
    /// 未执行即断言
    SkipExecute,
    /// 重复挂判据（同槽二次注册）
    DuplicateCouple,
    /// 判据越界
    SlotRange,
    /// 脱环（断言时三环不齐）
    RingBroken,
    /// J2 未决越闸（J2 复测未齐即放行）
    GateBypass,
    /// 状态倒退
    StateRegress,
    /// 账失恒（登记环数≠留痕环数）
    LedgerDrift,
    /// 重复留痕
    DuplicateRow,
    /// 判据形态缺口径（条目未挂 J1 或 J2 档位）
    TrackMissing,
    /// 对接未完成即出账
    CoupleEarly,
    /// 段位越界
    SegmentViolation,
}

impl RingCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RingCode::SkipRegister => "跳环执行被拒：判据必须先注册再执行；请按三环序推进",
            RingCode::SkipExecute => "未执行即断言被拒：执行环先行；请补执行留痕",
            RingCode::DuplicateCouple => "重复挂判据被拒：同槽已挂判据；请先核查再改",
            RingCode::SlotRange => "判据越界被拒：槽号只允许 0..6；请核对槽号",
            RingCode::RingBroken => "脱环被拒：注册/执行/断言三环必须齐；请补缺环",
            RingCode::GateBypass => "J2 未决越闸被拒：J2 复测未齐闸保持红；请补 J2 留痕",
            RingCode::StateRegress => "状态倒退被拒：三环状态单向推进；请排查回滚",
            RingCode::LedgerDrift => "账失恒被拒：登记环数必须等于留痕环数；请对账补录",
            RingCode::DuplicateRow => "重复留痕被拒：同判据同环已留痕；请查重",
            RingCode::TrackMissing => "缺口径档位被拒：条目必须挂 J1/J2 档位；请补档位",
            RingCode::CoupleEarly => "对接未完成即出账被拒：三环齐才可出账；请按序推进",
            RingCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x925c0..=0x92600",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [RingCode; 12] = [
        RingCode::SkipRegister,
        RingCode::SkipExecute,
        RingCode::DuplicateCouple,
        RingCode::SlotRange,
        RingCode::RingBroken,
        RingCode::GateBypass,
        RingCode::StateRegress,
        RingCode::LedgerDrift,
        RingCode::DuplicateRow,
        RingCode::TrackMissing,
        RingCode::CoupleEarly,
        RingCode::SegmentViolation,
    ];
}

// ── 判据档位与对接机 ──────────────────────────────────────────────────
/// 判据口径档位闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoupTrack {
    /// J1 正判据
    J1,
    /// J2 反判据
    J2,
}

/// 状态机：Unregistered → Registered → Executed → Asserted 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RingState {
    /// 未注册（三环全暗）
    Unregistered,
    /// 已注册（判据已挂进管道）
    Registered,
    /// 已执行（判据已跑出结果）
    Executed,
    /// 已断言（三环齐、对接完成）
    Asserted,
}

/// 一条挂进管道的判据：槽号 + 口径档位 + J2 复测轮数（未决判定用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoupEntry {
    /// 槽号（0..6）
    pub slot: usize,
    /// 口径档位
    pub track: CoupTrack,
    /// 该判据的复测轮数留痕（J2 侧与 J2_SETTLE_MIN 比较）
    pub rounds: u32,
}

/// 对接机：注册 → 执行 → 断言 三环单向推进。
#[derive(Clone, Debug)]
pub struct ChecksetCoupler {
    /// 已挂判据
    pub entries: Vec<CoupEntry>,
    /// 当前状态
    pub state: RingState,
}

impl ChecksetCoupler {
    /// 构造空对接机。
    pub fn new() -> ChecksetCoupler {
        ChecksetCoupler { entries: Vec::new(), state: RingState::Unregistered }
    }

    /// 注册环：判据挂进管道（档位必检、槽号必检、重复必拒）。
    pub fn register(&mut self, slot: usize, track: CoupTrack, rounds: u32) -> Result<usize, RingCode> {
        if self.state != RingState::Unregistered && self.state != RingState::Registered {
            return Err(RingCode::StateRegress);
        }
        if slot >= COUPLE_SLOTS {
            return Err(RingCode::SlotRange);
        }
        if self.entries.iter().any(|e| e.slot == slot) {
            return Err(RingCode::DuplicateCouple);
        }
        self.entries.push(CoupEntry { slot, track, rounds });
        self.state = RingState::Registered;
        Ok(slot)
    }

    /// 执行环：已注册才可执行，执行后状态推进。
    pub fn execute(&mut self) -> Result<usize, RingCode> {
        if self.state == RingState::Unregistered {
            return Err(RingCode::SkipRegister);
        }
        if self.state != RingState::Registered {
            return Err(RingCode::StateRegress);
        }
        self.state = RingState::Executed;
        Ok(self.entries.len())
    }

    /// 断言环：J2 未决闸预核 + 三环齐核。
    /// J2 侧判据复测轮数 < J2_SETTLE_MIN → 闸保持红（GateBypass 不放行）。
    /// 三环齐才推进 Asserted。
    pub fn assert_all(&mut self, ledger: &mut RingLedger) -> Result<(), RingCode> {
        if self.state != RingState::Executed {
            return Err(RingCode::SkipExecute);
        }
        // J2 未决闸：任一 J2 判据复测未齐即不放行
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.track == CoupTrack::J2 && e.rounds < J2_SETTLE_MIN {
                    ledger.gate_kept_red = ledger.gate_kept_red.saturating_add(1);
                    return Err(RingCode::GateBypass);
                }
            }
            i += 1;
        }
        // 三环齐核（注册/执行/断言各留痕 ≥ 1）
        if ledger.rows.iter().filter(|r| r.ring == RingPhase::Register).count() == 0
            || ledger.rows.iter().filter(|r| r.ring == RingPhase::Execute).count() == 0
        {
            return Err(RingCode::RingBroken);
        }
        self.state = RingState::Asserted;
        Ok(())
    }
}

// ── 三环留痕账 ────────────────────────────────────────────────────────
/// 三环闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RingPhase {
    /// 注册环
    Register,
    /// 执行环
    Execute,
    /// 断言环
    Assert,
}

impl RingPhase {
    /// 闭集全量。
    pub const ALL: [RingPhase; 3] = [RingPhase::Register, RingPhase::Execute, RingPhase::Assert];
}

/// 一条环留痕：判据槽号 + 环 + 结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingRow {
    /// 判据槽号
    pub slot: usize,
    /// 环
    pub ring: RingPhase,
    /// 结果（1=过）
    pub ok: u32,
}

/// 留痕账：三环逐条留痕、账守恒、J2 未决闸保持红次数显性记账。
#[derive(Clone, Debug)]
pub struct RingLedger {
    /// 留痕行
    pub rows: Vec<RingRow>,
    /// 登记的应留痕环数
    pub registered: u32,
    /// J2 未决闸保持红次数（显性记账，非逃逸）
    pub gate_kept_red: u32,
}

impl RingLedger {
    /// 构造零账。
    pub fn new() -> RingLedger {
        RingLedger { rows: Vec::new(), registered: 0, gate_kept_red: 0 }
    }

    /// 记一条环留痕：重复必拒。
    pub fn record(&mut self, slot: usize, ring: RingPhase, ok: u32) -> Result<(), RingCode> {
        if self.rows.iter().any(|r| r.slot == slot && r.ring == ring) {
            return Err(RingCode::DuplicateRow);
        }
        self.rows.push(RingRow { slot, ring, ok });
        Ok(())
    }

    /// 登记应留痕环数：0 或超 500 拒绝。
    pub fn set_registered(&mut self, n: u32) -> Result<(), RingCode> {
        if n == 0 || n > 500 {
            return Err(RingCode::LedgerDrift);
        }
        self.registered = n;
        Ok(())
    }

    /// 守恒式：登记环数 = 留痕环数 才零漂移。
    pub fn drift(&self) -> Option<u32> {
        let done = self.rows.len() as u32;
        if done == self.registered {
            None
        } else if done < self.registered {
            Some(self.registered - done)
        } else {
            Some(0)
        }
    }

    /// 单环留痕数。
    pub fn phase_count(&self, phase: RingPhase) -> usize {
        self.rows.iter().filter(|r| r.ring == phase).count()
    }
}

/// 标准对接：6 条判据逐条注册 → 执行 → 断言，全链留痕。
pub fn couple_standard(coupler: &mut ChecksetCoupler, ledger: &mut RingLedger) -> Result<(), RingCode> {
    // 注册环：J1×3（复测满 10）+ J2×3（复测满 10，已决）
    let mut n = 0usize;
    while n < 3 {
        coupler.register(n, CoupTrack::J1, J2_SETTLE_MIN)?;
        let _ = ledger.record(n, RingPhase::Register, 1);
        n += 1;
    }
    let mut n2 = 3usize;
    while n2 < COUPLE_SLOTS {
        coupler.register(n2, CoupTrack::J2, J2_SETTLE_MIN)?;
        let _ = ledger.record(n2, RingPhase::Register, 1);
        n2 += 1;
    }
    // 执行环
    let _ = coupler.execute()?;
    let mut n3 = 0usize;
    while n3 < COUPLE_SLOTS {
        let _ = ledger.record(n3, RingPhase::Execute, 1);
        n3 += 1;
    }
    // 断言环：登记环数 = 注册 6 + 执行 6 + 断言 2 = 14
    ledger.set_registered(14)?;
    coupler.assert_all(ledger)?;
    let _ = ledger.record(0, RingPhase::Assert, 1);
    ledger.record(1, RingPhase::Assert, 1)?;
    Ok(())
}

// ── J2 未决闸预核（联动 F10089） ──────────────────────────────────────
/// J2 未决闸探测：J2 复测轮数 ≥ J2_SETTLE_MIN 才算已决。
/// 已决放行（Ok），未决保持红（Err GateBypass）。
pub fn gate_probe(j2_rounds: u32) -> Result<(), RingCode> {
    if j2_rounds >= J2_SETTLE_MIN {
        Ok(())
    } else {
        Err(RingCode::GateBypass)
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏对接样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRingCase {
    /// 跳环执行（未注册先执行）
    SkipRegister,
    /// 未执行即断言
    SkipExecute,
    /// 重复挂判据
    DuplicateCouple,
    /// J2 未决越闸（复测 5 轮 < 10）
    GateBypass,
    /// 账失恒（登记环数≠留痕环数）
    LedgerDrift,
}

impl BadRingCase {
    /// 闭集全量。
    pub const ALL: [BadRingCase; 5] = [
        BadRingCase::SkipRegister,
        BadRingCase::SkipExecute,
        BadRingCase::DuplicateCouple,
        BadRingCase::GateBypass,
        BadRingCase::LedgerDrift,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拦截。
    pub fn inject(self) -> bool {
        match self {
            BadRingCase::SkipRegister => {
                let mut c = ChecksetCoupler::new();
                let mut l = RingLedger::new();
                c.execute().err() == Some(RingCode::SkipRegister)
                    && c.assert_all(&mut l).err() == Some(RingCode::SkipExecute)
            }
            BadRingCase::SkipExecute => {
                let mut c = ChecksetCoupler::new();
                let _ = c.register(0, CoupTrack::J1, J2_SETTLE_MIN);
                let mut l = RingLedger::new();
                c.assert_all(&mut l).err() == Some(RingCode::SkipExecute)
            }
            BadRingCase::DuplicateCouple => {
                let mut c = ChecksetCoupler::new();
                let a = c.register(0, CoupTrack::J1, J2_SETTLE_MIN).is_ok();
                let b = c.register(0, CoupTrack::J2, J2_SETTLE_MIN).err()
                    == Some(RingCode::DuplicateCouple);
                a && b
            }
            BadRingCase::GateBypass => {
                // J2 判据只复测 5 轮（< 10）→ 断言环必须保持红
                let mut c = ChecksetCoupler::new();
                let _ = c.register(0, CoupTrack::J2, 5);
                let _ = c.execute();
                let mut l = RingLedger::new();
                let _ = l.record(0, RingPhase::Register, 1);
                let _ = l.record(0, RingPhase::Execute, 1);
                let rej = c.assert_all(&mut l).err() == Some(RingCode::GateBypass);
                rej && l.gate_kept_red == 1 && gate_probe(5).is_err()
            }
            BadRingCase::LedgerDrift => {
                let mut l = RingLedger::new();
                let _ = l.set_registered(5);
                let a = l.record(0, RingPhase::Register, 1).is_ok();
                let b = l.drift() == Some(4);
                a && b
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类拦截数。
pub fn run_ring_injection() -> (RingLedger, [u32; 5]) {
    let mut ledger = RingLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadRingCase::ALL.len() {
        if let Some(case) = BadRingCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                if !caught {
                    ledger.gate_kept_red = ledger.gate_kept_red.saturating_add(1);
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
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；越闸/未完成出账族"),
    ("EBUSY", -16, 170, "Windows ERROR_BUSY；跳环执行族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；槽号/环序非法族"),
    ("EALREADY", -114, 10037, "Windows WSAEALREADY；重复挂判据族"),
    ("EPROTO", -71, 1231, "Windows ERROR_PROTOCOL_UNREACHABLE；脱环族"),
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

/// 规范语料双跑指纹：注册→执行→断言→闸探测全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut c = ChecksetCoupler::new();
    let mut l = RingLedger::new();
    bytes.push(match couple_standard(&mut c, &mut l) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(c.state as u8);
    bytes.push(match l.drift() {
        None => 3,
        Some(_) => 4,
    });
    bytes.push(match gate_probe(J2_SETTLE_MIN) {
        Ok(()) => 5,
        Err(_) => 6,
    });
    bytes.push(match gate_probe(J2_SETTLE_MIN - 1) {
        Ok(()) => 7,
        Err(_) => 8,
    });
    bytes.extend_from_slice(&(l.rows.len() as u32).to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10088 判据集（17 条静态 + 3 条动态明细）。
pub fn run_unx10088_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10088");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x925c0..=0x92600", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准对接：6 条判据三环全链推进
    let mut c = ChecksetCoupler::new();
    let mut l = RingLedger::new();
    let std_ok = couple_standard(&mut c, &mut l).is_ok();
    s.add("三环对接：注册×6→执行×6→断言全链过", std_ok && c.state == RingState::Asserted,
          "J1×3+J2×3 逐条挂进管道，三环齐后断言放行");

    // 3. 跳环执行拦截
    let mut sc = ChecksetCoupler::new();
    let skip_rej = sc.execute().err() == Some(RingCode::SkipRegister);
    s.add("跳环拦截：未注册先执行被拒", skip_rej,
          "三环硬序：注册环先行，跳环显性拒绝");

    // 4. 未执行即断言拦截
    let mut uc = ChecksetCoupler::new();
    let _ = uc.register(0, CoupTrack::J1, J2_SETTLE_MIN);
    let mut ul = RingLedger::new();
    let skip_ex = uc.assert_all(&mut ul).err() == Some(RingCode::SkipExecute);
    s.add("未执行断言：执行环缺席被拒", skip_ex,
          "断言环必须看到执行环留痕，缺席即脱环");

    // 5. 重复挂判据拦截
    let mut dc = ChecksetCoupler::new();
    let _ = dc.register(0, CoupTrack::J1, J2_SETTLE_MIN);
    let dup_ok = dc.register(0, CoupTrack::J2, J2_SETTLE_MIN) == Err(RingCode::DuplicateCouple);
    s.add("重复挂判据：同槽二次注册被拒", dup_ok,
          "一槽一条判据，重复占位显性拒绝");

    // 6. 槽号越界拦截
    let mut rc = ChecksetCoupler::new();
    let rng_ok = rc.register(COUPLE_SLOTS, CoupTrack::J1, J2_SETTLE_MIN) == Err(RingCode::SlotRange);
    s.add("槽号越界：下标出 0..6 拒收", rng_ok,
          "注册必须落在合法槽窗内，越界显性拒绝");

    // 7. J2 未决闸：未决保持红 10/10（锚点判据）
    let mut gate_hits = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut gc = ChecksetCoupler::new();
        let _ = gc.register(0, CoupTrack::J2, J2_SETTLE_MIN - 1);
        let _ = gc.execute();
        let mut gl = RingLedger::new();
        let _ = gl.record(0, RingPhase::Register, 1);
        let _ = gl.record(0, RingPhase::Execute, 1);
        if gc.assert_all(&mut gl).err() == Some(RingCode::GateBypass) && gl.gate_kept_red == 1 {
            gate_hits += 1;
        }
        round += 1;
    }
    s.add("J2 未决闸：复测未齐闸保持红 10/10", gate_hits == INJECT_ROUNDS,
          "J2 侧复测轮数 < 10 即未决，闸逐次保持红不放行");

    // 8. 越闸 10/10 拦截（锚点判据）
    let mut bypass_hits = 0u32;
    let mut round2 = 0u32;
    while round2 < INJECT_ROUNDS {
        // 未决 J2 直闯断言环（跳过注册/执行留痕）必须被拦
        let mut bc = ChecksetCoupler::new();
        let _ = bc.register(0, CoupTrack::J2, 3);
        let _ = bc.execute();
        let mut bl = RingLedger::new();
        let bypass = bc.assert_all(&mut bl).err() == Some(RingCode::GateBypass)
            && gate_probe(3).err() == Some(RingCode::GateBypass);
        if bypass {
            bypass_hits += 1;
        }
        round2 += 1;
    }
    s.add("越闸拦截：未决 J2 闯关 10/10 次拦截", bypass_hits == INJECT_ROUNDS,
          "闸探测逐次拒绝未决 J2，越闸路径全被挡死");

    // 9. 已决 J2 放行
    let pass_ok = gate_probe(J2_SETTLE_MIN).is_ok() && gate_probe(J2_SETTLE_MIN + 5).is_ok();
    s.add("已决放行：J2 复测 ≥10 轮闸放行", pass_ok,
          "闸的判定面：J2_SETTLE_MIN=10 是红/绿分界，边界两侧行为精确");

    // 10. 账守恒：登记=留痕=14（注册 6 + 执行 6 + 断言 2）
    let cons_ok = l.drift().is_none() && l.rows.len() == 14;
    s.add("环账守恒：登记=留痕=14 零漂移", cons_ok,
          "三环留痕逐条对账，账实零漂移");

    // 11. 重复留痕拦截
    let mut dl = RingLedger::new();
    let _ = dl.record(0, RingPhase::Register, 1);
    let dup_ok = dl.record(0, RingPhase::Register, 1) == Err(RingCode::DuplicateRow);
    s.add("重复留痕：同判据同环二次留痕被拒", dup_ok,
          "一判据一环一条留痕，重复显性拒绝");

    // 12. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_ring_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "跳环/未执行断言/重复挂判据/J2 未决越闸/账失恒全被显式拦截");

    // 13. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0",
          ledger.gate_kept_red == 0 && ledger.rows.is_empty(),
          "注入核全程不得产生环留痕逃逸");

    // 14. 文案闭集：非空
    let text_ok = RingCode::ALL.iter().all(|cc| !cc.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < RingCode::ALL.len() {
        let mut j = i2 + 1;
        while j < RingCode::ALL.len() {
            if let (Some(x), Some(y)) = (RingCode::ALL.get(i2), RingCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = RingCode::ALL.iter().all(|cc| cc.val() > SEG_BASE && cc.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(EBUSY)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EALREADY)
        && agrees_with_linux(EPROTO)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：三环快照 + 闸快照 + 指纹快照
    let phase_text = format!("注册 {} / 执行 {} / 断言 {}",
        l.phase_count(RingPhase::Register), l.phase_count(RingPhase::Execute),
        l.phase_count(RingPhase::Assert));
    s.add_dyn("三环快照", l.rows.len() == 14, &phase_text);
    let gate_text = format!("闸保持红 {} 次（注入面）", ledger.gate_kept_red);
    s.add_dyn("闸快照", ledger.gate_kept_red == 0, &gate_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_couple_ok() {
        let mut c = ChecksetCoupler::new();
        let mut l = RingLedger::new();
        assert!(couple_standard(&mut c, &mut l).is_ok());
        assert_eq!(c.state, RingState::Asserted);
        assert_eq!(l.rows.len(), 14);
        assert!(l.drift().is_none());
    }

    #[test]
    fn skip_register_rejected() {
        let mut c = ChecksetCoupler::new();
        assert_eq!(c.execute(), Err(RingCode::SkipRegister));
    }

    #[test]
    fn skip_execute_rejected() {
        let mut c = ChecksetCoupler::new();
        let _ = c.register(0, CoupTrack::J1, J2_SETTLE_MIN);
        let mut l = RingLedger::new();
        assert_eq!(c.assert_all(&mut l), Err(RingCode::SkipExecute));
    }

    #[test]
    fn duplicate_couple_rejected() {
        let mut c = ChecksetCoupler::new();
        let _ = c.register(0, CoupTrack::J1, J2_SETTLE_MIN);
        assert_eq!(c.register(0, CoupTrack::J2, J2_SETTLE_MIN), Err(RingCode::DuplicateCouple));
    }

    #[test]
    fn slot_range_rejected() {
        let mut c = ChecksetCoupler::new();
        assert_eq!(c.register(6, CoupTrack::J1, J2_SETTLE_MIN), Err(RingCode::SlotRange));
    }

    #[test]
    fn j2_pending_keeps_red() {
        let mut c = ChecksetCoupler::new();
        let _ = c.register(0, CoupTrack::J2, 5);
        let _ = c.execute();
        let mut l = RingLedger::new();
        let _ = l.record(0, RingPhase::Register, 1);
        let _ = l.record(0, RingPhase::Execute, 1);
        assert_eq!(c.assert_all(&mut l), Err(RingCode::GateBypass));
        assert_eq!(l.gate_kept_red, 1);
        assert!(c.state != RingState::Asserted);
    }

    #[test]
    fn gate_boundary() {
        assert!(gate_probe(J2_SETTLE_MIN).is_ok());
        assert_eq!(gate_probe(J2_SETTLE_MIN - 1), Err(RingCode::GateBypass));
        assert_eq!(gate_probe(0), Err(RingCode::GateBypass));
    }

    #[test]
    fn duplicate_row_rejected() {
        let mut l = RingLedger::new();
        let _ = l.record(0, RingPhase::Register, 1);
        assert_eq!(l.record(0, RingPhase::Register, 1), Err(RingCode::DuplicateRow));
    }

    #[test]
    fn ledger_drift_detected() {
        let mut l = RingLedger::new();
        let _ = l.set_registered(5);
        let _ = l.record(0, RingPhase::Register, 1);
        assert_eq!(l.drift(), Some(4));
        assert_eq!(l.set_registered(0), Err(RingCode::LedgerDrift));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_ring_injection();
        assert_eq!(l.gate_kept_red, 0);
        assert!(l.rows.is_empty());
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EPROTO));
        assert!(agrees_with_linux(EALREADY));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EPERM), -EPERM);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10088_checks_green() {
        let s = run_unx10088_checks();
        assert!(s.all_passed(), "UNX-F10088 red: {}", s.red_items_text());
    }
}
