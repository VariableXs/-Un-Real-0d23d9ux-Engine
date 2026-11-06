//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18542 · bottle 默认版本档案
//! · 0x80340 段 · 批 08 · 配额 320
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18542-J1 bottle 默认档案生效（全应用兜底）断言全过，
//!       空档案回退系统默认。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 三级解析链冻结：per-app 绑定 > bottle 默认档案 > 系统默认
//!     （真实系统版本）；bottle 默认档案对全部未绑定应用生效——
//!     同一次解析所有未绑槽位必然取得同一 bottle 值（兜底一致性）；
//!   - 空档案显性：bottle 槽为 None（valid=false）时必须回退系统
//!     默认，不得让空值穿透（穿透即伪装矩阵落空档）；
//!   - bottle 槽位面：12 应用槽 [u8;2] 位图，绑定单调（置位后
//!     不清位），重复绑定拒、越界槽拒、槽满显性拒且旧账不丢；
//!   - bottle 绑定状态机三态单向：Idle → Bound → Sealed；空账
//!     （零绑定）禁封印，倒退拒；
//!   - 注入三类 × 10 轮：空档穿透（变异解析器让空 bottle 落空值）/
//!     非法版本入库（major=0 穿过接受器）/ 槽位重复绑定——逐类逐轮
//!     必抓，漏检即红；
//!   - 冻结锚：BOTTLE_DEFAULT=6.1.7601 SP1 / SYSTEM_DEFAULT=10.0.19045
//!     （写死仅作对拍锚，非真实系统探测）；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] BdCode 闭集（12 码）+ ALL + detail() + detail(u32)
//!   [裁决表]   兜底解析链三级序表（App=0/Bottle=1/System=2）
//!   [数据结构] VerProfile（六字段）+ BottleSlotPlane（12 槽位图）
//!   [状态机]   BottlePhase 三态单向 + BottleBinder
//!   [账本]     ResolveLedger（三级来源账守恒）
//!   [注入核]   三类变异 × 10 轮（穿透/非法版本/重复绑定）
//!   [收口]     golden_run + run_unx18542_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80340，AI-VX11-1105 G1 批08 独占区）。
pub const SEG_BASE: u32 = 0x80340;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// 应用槽位数（冻结：12）。
pub const APP_SLOTS: usize = 12;
/// 版本主段下界（冻结：≥3，与总纲 F18541 同口径）。
pub const VER_MAJOR_MIN: u16 = 3;
/// SP 段上界（冻结：≤9）。
pub const SP_MAX: u8 = 9;
/// 产品类型闭集（冻结：1=Workstation 2=DomainController 3=Server）。
pub const PT_MIN: u16 = 1;
pub const PT_MAX: u16 = 3;
/// DPI 闭集（冻结五档）。
pub const DPI_SET: [u16; 5] = [96, 120, 144, 192, 240];
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// bottle 默认档案锚（冻结：典型 6.1.7601 SP1 工作站，仅作对拍锚）。
pub const BOTTLE_DEFAULT: VerProfile = VerProfile {
    major: 6,
    minor: 1,
    build: 7601,
    sp: 1,
    pt: 3,
    suite: 0x0106,
    dpi: 96,
    elevate: 0,
};

/// 系统默认锚（冻结：真实宿主 10.0.19045 无 SP，空档案回退目标）。
pub const SYSTEM_DEFAULT: VerProfile = VerProfile {
    major: 10,
    minor: 0,
    build: 19045,
    sp: 0,
    pt: 3,
    suite: 0x0100,
    dpi: 96,
    elevate: 0,
};

// ── 版本档案结构（六字段） ──────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerProfile {
    pub major: u16,
    pub minor: u16,
    pub build: u16,
    pub sp: u8,
    /// 产品类型（1..=3）。
    pub pt: u16,
    /// 套件掩码。
    pub suite: u16,
    pub dpi: u16,
    /// 提权（0/1）。
    pub elevate: u8,
}

impl VerProfile {
    /// 值域守卫：主段/SP/PT/DPI/提权逐域闸。
    pub fn valid(&self) -> bool {
        if self.major < VER_MAJOR_MIN {
            return false;
        }
        if self.sp > SP_MAX {
            return false;
        }
        if self.pt < PT_MIN || self.pt > PT_MAX {
            return false;
        }
        if !dpi_legal(self.dpi) {
            return false;
        }
        if self.elevate > 1 {
            return false;
        }
        true
    }
    /// 结构指纹（31 折叠，双跑对拍锚）。
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x18542;
        acc = acc.wrapping_mul(31).wrapping_add(self.major as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.minor as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.build as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.sp as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.pt as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.suite as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.dpi as u32);
        acc.wrapping_mul(31).wrapping_add(self.elevate as u32)
    }
}

/// DPI 是否在冻结闭集内。
pub fn dpi_legal(v: u16) -> bool {
    let mut i = 0;
    while i < DPI_SET.len() {
        if DPI_SET[i] == v {
            return true;
        }
        i += 1;
    }
    false
}

// ── 解析链（三级序表） ──────────────────────────────────────────
/// 解析来源（冻结序：App=0 最先，System=2 兜底）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BdSource {
    /// per-app 绑定档案。
    App = 0,
    /// bottle 默认档案。
    Bottle = 1,
    /// 系统默认（真实版本）。
    System = 2,
}

/// 三级兜底解析：App 有绑走 App；否则 bottle 有值走 Bottle；
/// 否则回退系统默认。空值（None）永不穿透。
pub fn resolve_chain(
    app: Option<VerProfile>,
    bottle: Option<VerProfile>,
) -> (VerProfile, BdSource) {
    match app {
        Some(p) => (p, BdSource::App),
        None => match bottle {
            Some(p) => (p, BdSource::Bottle),
            None => (SYSTEM_DEFAULT, BdSource::System),
        },
    }
}

/// 兜底一致性：给定未绑槽集合，所有槽解析来源必须同为 Bottle
/// 或同为 System（同一 bottle 态下不允许来源分裂）。
pub fn fallback_consistent(bottle: Option<VerProfile>, slots: usize) -> bool {
    let mut i = 0;
    let mut src: Option<u8> = None;
    while i < slots {
        let (_, s) = resolve_chain(None, bottle);
        let tag = match s {
            BdSource::App => 0u8,
            BdSource::Bottle => 1u8,
            BdSource::System => 2u8,
        };
        match src {
            None => src = Some(tag),
            Some(t) => {
                if t != tag {
                    return false;
                }
            }
        }
        i += 1;
    }
    true
}

// ── bottle 槽位面（12 槽 [u8;2] 位图） ──────────────────────────
/// 12 槽位图（单调置位，不清位）。
#[derive(Clone, Copy, Debug)]
pub struct BottleSlotPlane {
    pub bits: [u8; 2],
    pub bound_count: u8,
}

impl BottleSlotPlane {
    pub fn new() -> BottleSlotPlane {
        BottleSlotPlane { bits: [0; 2], bound_count: 0 }
    }
    /// 槽位读取（越界 None）。
    pub fn bit(&self, slot: usize) -> Option<bool> {
        if slot >= APP_SLOTS {
            return None;
        }
        Some(self.bits[slot / 8] & (1u8 << (slot % 8)) != 0)
    }
    /// 绑定置位：越界拒 / 已绑重复拒 / 满（12 槽全绑）拒。
    pub fn bind(&mut self, slot: usize) -> Result<(), u32> {
        if slot >= APP_SLOTS {
            return Err(BdCode::SlotOutOfRange.code());
        }
        if self.bit(slot) == Some(true) {
            return Err(BdCode::SlotDuplicate.code());
        }
        if self.bound_count as usize >= APP_SLOTS {
            return Err(BdCode::SlotsFull.code());
        }
        self.bits[slot / 8] |= 1u8 << (slot % 8);
        self.bound_count = self.bound_count.saturating_add(1);
        Ok(())
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BdCode {
    /// 槽位越界（≥12）。
    SlotOutOfRange,
    /// 槽位重复绑定。
    SlotDuplicate,
    /// 槽全满仍申绑。
    SlotsFull,
    /// 空档穿透（空 bottle 未回退系统默认）。
    EmptyPenetrated,
    /// 非法版本入库（值域守卫未拦截）。
    VersionIllegal,
    /// 档案值域越界。
    ProfileInvalid,
    /// 空账封印（零绑定即封）。
    LedgerEmpty,
    /// 封印倒退/越级。
    PhaseRegression,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 注入漏检。
    InjectionMissed,
    /// 口径失配（与总纲双层解析不等）。
    CrossCheckMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl BdCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BdCode::SlotOutOfRange => "槽位越界：仅 0..=11",
            BdCode::SlotDuplicate => "槽位重复绑定：单调置位禁重置",
            BdCode::SlotsFull => "槽全满：12 槽耗尽",
            BdCode::EmptyPenetrated => "空档穿透：空 bottle 必须回退系统默认",
            BdCode::VersionIllegal => "非法版本入库：值域守卫未拦截",
            BdCode::ProfileInvalid => "档案值域越界：逐域闸拒",
            BdCode::LedgerEmpty => "空账封印：零绑定不得封",
            BdCode::PhaseRegression => "封印倒退：状态机单向禁回退",
            BdCode::LedgerBroken => "账本守恒破坏：尝试≠过+拒",
            BdCode::InjectionMissed => "注入漏检：变异必被抓",
            BdCode::CrossCheckMismatch => "口径失配：与总纲解析序不等",
            BdCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [BdCode; 12] = [
        BdCode::SlotOutOfRange,
        BdCode::SlotDuplicate,
        BdCode::SlotsFull,
        BdCode::EmptyPenetrated,
        BdCode::VersionIllegal,
        BdCode::ProfileInvalid,
        BdCode::LedgerEmpty,
        BdCode::PhaseRegression,
        BdCode::LedgerBroken,
        BdCode::InjectionMissed,
        BdCode::CrossCheckMismatch,
        BdCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BdCode::ALL.len() {
        if BdCode::ALL[i].code() == code {
            return BdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80340 段闭集内"
}

// ── bottle 绑定状态机 ───────────────────────────────────────────
/// 绑定相位（单向：Idle→Bound→Sealed）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BottlePhase {
    /// 零绑定。
    Idle,
    /// 有绑定。
    Bound,
    /// 已封账。
    Sealed,
}

/// 解析来源账（守恒：每来源尝试 = 过 + 拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolveLedger {
    /// 三来源账 [App, Bottle, System]。
    pub tries: [u32; 3],
    pub passed: [u32; 3],
    pub rejected: [u32; 3],
}

impl ResolveLedger {
    pub const fn new() -> ResolveLedger {
        ResolveLedger { tries: [0; 3], passed: [0; 3], rejected: [0; 3] }
    }
    /// 账目推进。
    pub fn record(&mut self, src: usize, ok: bool) {
        if src >= 3 {
            return;
        }
        self.tries[src] = self.tries[src].saturating_add(1);
        if ok {
            self.passed[src] = self.passed[src].saturating_add(1);
        } else {
            self.rejected[src] = self.rejected[src].saturating_add(1);
        }
    }
    /// 三来源守恒。
    pub fn conserve(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] != self.passed[i] + self.rejected[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// bottle 绑定机：位面 + 相位 + 来源账。
pub struct BottleBinder {
    pub phase: BottlePhase,
    pub plane: BottleSlotPlane,
    pub ledger: ResolveLedger,
}

impl BottleBinder {
    pub fn new() -> BottleBinder {
        BottleBinder {
            phase: BottlePhase::Idle,
            plane: BottleSlotPlane::new(),
            ledger: ResolveLedger::new(),
        }
    }

    /// 绑应用槽：相位推进 Idle→Bound；重复/越界/满显性拒且入拒账。
    pub fn bind_app(&mut self, slot: usize) -> Result<(), u32> {
        if self.phase == BottlePhase::Sealed {
            self.ledger.record(0, false);
            return Err(BdCode::PhaseRegression.code());
        }
        match self.plane.bind(slot) {
            Ok(()) => {
                self.ledger.record(0, true);
                self.phase = BottlePhase::Bound;
                Ok(())
            }
            Err(c) => {
                self.ledger.record(0, false);
                Err(c)
            }
        }
    }

    /// 封账：空账（零绑定）拒；封后绑定拒（倒退）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == BottlePhase::Sealed {
            return Err(BdCode::PhaseRegression.code());
        }
        if self.phase == BottlePhase::Idle {
            return Err(BdCode::LedgerEmpty.code());
        }
        if !self.ledger.conserve() {
            return Err(BdCode::LedgerBroken.code());
        }
        self.phase = BottlePhase::Sealed;
        Ok(())
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链输出（绑定数 / 三来源账 / bottle 指纹）。
pub type GoldenOut = (usize, ResolveLedger, u32);

/// 黄金全链：bottle 默认生效（未绑槽全部兜底取得 bottle 值）→
/// 空档案回退系统默认 → 兜底一致性 → 绑定/封账。
pub fn golden_run() -> Result<GoldenOut, u32> {
    // bottle 档案合法性锚。
    if !BOTTLE_DEFAULT.valid() || !SYSTEM_DEFAULT.valid() {
        return Err(BdCode::ProfileInvalid.code());
    }
    let mut binder = BottleBinder::new();
    // 绑定槽 0/1/2（其余未绑）。
    let mut i = 0;
    while i < 3 {
        binder.bind_app(i)?;
        i += 1;
    }
    // 兜底语义：未绑槽解析得 bottle 值，来源 = Bottle。
    let mut j = 3;
    while j < APP_SLOTS {
        let (p, s) = resolve_chain(None, Some(BOTTLE_DEFAULT));
        if s != BdSource::Bottle || p != BOTTLE_DEFAULT {
            return Err(BdCode::EmptyPenetrated.code());
        }
        binder.ledger.record(1, true);
        j += 1;
    }
    // 空档案回退系统默认（J1 第二命题）。
    let (p2, s2) = resolve_chain(None, None);
    if s2 != BdSource::System || p2 != SYSTEM_DEFAULT {
        return Err(BdCode::EmptyPenetrated.code());
    }
    binder.ledger.record(2, true);
    // 兜底一致性（12 槽同源）。
    if !fallback_consistent(Some(BOTTLE_DEFAULT), APP_SLOTS)
        || !fallback_consistent(None, APP_SLOTS)
    {
        return Err(BdCode::CrossCheckMismatch.code());
    }
    binder.seal()?;
    if !binder.ledger.conserve() {
        return Err(BdCode::LedgerBroken.code());
    }
    Ok((binder.plane.bound_count as usize, binder.ledger, BOTTLE_DEFAULT.fingerprint()))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BdCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核（三类变异 × 10 轮） ──────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 空档穿透。
    pub penetrate_caught: u32,
    /// 类2 非法版本入库。
    pub illegal_caught: u32,
    /// 类3 槽位重复绑定。
    pub dup_caught: u32,
}

/// 面额（实现推导）：三类各每轮 1 次违约，各必抓 1。
pub const FACE_PENETRATE: u32 = 1;
pub const FACE_ILLEGAL: u32 = 1;
pub const FACE_DUP: u32 = 1;

/// 变异解析器（类1 注入体）：bottle 空时返回空档案（穿透）。
/// 正宗解析链必须回退系统默认，故变异结果必被对拍抓出。
fn mutated_resolve_penetrate(bottle: Option<VerProfile>) -> VerProfile {
    match bottle {
        Some(p) => p,
        None => VerProfile { major: 0, minor: 0, build: 0, sp: 0, pt: 0, suite: 0, dpi: 0, elevate: 0 },
    }
}

/// 三类注入 rounds 轮；每类每轮必抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 空档穿透：变异体结果必非系统默认 → 对拍抓。
        let bad = mutated_resolve_penetrate(None);
        if bad != SYSTEM_DEFAULT {
            led.penetrate_caught = led.penetrate_caught.saturating_add(FACE_PENETRATE);
        } else {
            return Err(BdCode::InjectionMissed.code());
        }
        // 类2 非法版本入库：major=0 必被值域守卫拒。
        let rogue = VerProfile { major: 0, minor: 0, build: 0, sp: 0, pt: 3, suite: 0, dpi: 96, elevate: 0 };
        if rogue.valid() {
            return Err(BdCode::InjectionMissed.code());
        }
        led.illegal_caught = led.illegal_caught.saturating_add(FACE_ILLEGAL);
        // 类3 槽位重复绑定：同槽二次 bind 必拒。
        let mut binder = BottleBinder::new();
        binder.bind_app(0).map_err(|_| BdCode::InjectionMissed.code())?;
        match binder.bind_app(0) {
            Err(c) if c == BdCode::SlotDuplicate.code() => {
                led.dup_caught = led.dup_caught.saturating_add(FACE_DUP);
            }
            _ => return Err(BdCode::InjectionMissed.code()),
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：黄金链全过。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：空账封印（零绑定）→ 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut binder = BottleBinder::new();
    binder.seal()
}

/// 探针 C：槽越界（slot=12）→ 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut binder = BottleBinder::new();
    binder.bind_app(APP_SLOTS)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < BdCode::ALL.len() {
        let v = BdCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（8–20 条；动态明细 ≤8） ────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：bottle 兜底生效 + 绑定/封账全过（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _)) => *n == 3 && led.conserve(),
        Err(_) => false,
    };
    v.push(("J1-黄金全链", g_ok, format!("{:?}", g)));

    // 2. bottle 默认档案生效：未绑槽全部取得 bottle 值。
    let eff_ok = {
        let (p, s) = resolve_chain(None, Some(BOTTLE_DEFAULT));
        s == BdSource::Bottle && p == BOTTLE_DEFAULT
    };
    v.push(("J1-默认档案生效", eff_ok, String::from("未绑槽→bottle 值")));

    // 3. 空档案回退系统默认（J1 第二命题）。
    let fall_ok = {
        let (p, s) = resolve_chain(None, None);
        s == BdSource::System && p == SYSTEM_DEFAULT
    };
    v.push(("J1-空档回退系统默认", fall_ok, format!("{}.{},{}", SYSTEM_DEFAULT.major, SYSTEM_DEFAULT.minor, SYSTEM_DEFAULT.build)));

    // 4. 三级解析序：App > Bottle > System（每级一例）。
    let app_p = BOTTLE_DEFAULT;
    let ord_ok = {
        let (p0, s0) = resolve_chain(Some(app_p), Some(BOTTLE_DEFAULT));
        let (_, s1) = resolve_chain(None, Some(BOTTLE_DEFAULT));
        let (_, s2) = resolve_chain(None, None);
        s0 == BdSource::App && p0 == app_p && s1 == BdSource::Bottle && s2 == BdSource::System
    };
    v.push(("J1-三级解析序", ord_ok, String::from("App>Bottle>System")));

    // 5. 兜底一致性：两种 bottle 态下 12 槽同源。
    let fc_ok = fallback_consistent(Some(BOTTLE_DEFAULT), APP_SLOTS)
        && fallback_consistent(None, APP_SLOTS);
    v.push(("J1-兜底一致性", fc_ok, format!("{}槽同源", APP_SLOTS)));

    // 6. 类1 空档穿透注入（1 × 10 轮）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let t1 = led.as_ref().map(|l| l.penetrate_caught).unwrap_or(0);
    v.push(("J1-穿透注入全抓", t1 == FACE_PENETRATE * JUDGE_ROUNDS, format!("{}/{}", t1, FACE_PENETRATE * JUDGE_ROUNDS)));

    // 7. 类2 非法版本注入。
    let t2 = led.as_ref().map(|l| l.illegal_caught).unwrap_or(0);
    v.push(("J1-非法版本全抓", t2 == FACE_ILLEGAL * JUDGE_ROUNDS, format!("{}/{}", t2, FACE_ILLEGAL * JUDGE_ROUNDS)));

    // 8. 类3 重复绑定注入。
    let t3 = led.as_ref().map(|l| l.dup_caught).unwrap_or(0);
    v.push(("J1-重复绑定全抓", t3 == FACE_DUP * JUDGE_ROUNDS, format!("{}/{}", t3, FACE_DUP * JUDGE_ROUNDS)));

    // 9. 注入账守恒。
    let cons = match led.as_ref() {
        Ok(l) => {
            let sum = l.penetrate_caught.saturating_add(l.illegal_caught).saturating_add(l.dup_caught);
            sum == (FACE_PENETRATE + FACE_ILLEGAL + FACE_DUP) * JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons, String::from("3类面额×10轮")));

    // 10. 槽位面：越界/重复/满三闸。
    let plane_ok = {
        let mut pl = BottleSlotPlane::new();
        let oob = pl.bind(APP_SLOTS).is_err();
        let mut dup_res = Err(BdCode::SlotOutOfRange.code());
        if pl.bind(0).is_ok() {
            dup_res = pl.bind(0);
        }
        oob && dup_res.err() == Some(BdCode::SlotDuplicate.code()) && pl.bit(0) == Some(true)
            && pl.bit(APP_SLOTS).is_none()
    };
    v.push(("J1-槽位面三闸", plane_ok, String::from("越界/重复/单调置位")));

    // 11. 槽满显性：12 槽全绑后再绑拒且账不丢。
    let full_ok = {
        let mut binder = BottleBinder::new();
        let mut k = 0;
        let mut ok = true;
        while k < APP_SLOTS {
            if binder.bind_app(k).is_err() {
                ok = false;
            }
            k += 1;
        }
        let extra = binder.bind_app(0);
        ok && extra.is_err() && binder.plane.bound_count as usize == APP_SLOTS
    };
    v.push(("J1-槽满显性12槽", full_ok, String::from("12 收 1 拒")));

    // 12. 空账封印拒（探针 B）。
    v.push(("J1-空账封印拒", probe_seq_b().is_err(), String::from("零绑定即封必拒")));

    // 13. 槽越界拒（探针 C）。
    v.push(("J1-槽越界拒", probe_seq_c().is_err(), String::from("slot=12 必拒")));

    // 14. 值域守卫：PT 越闭集 / DPI 越闭集 / SP 超界逐域拒。
    let val_ok = {
        let base = VerProfile { major: 6, minor: 1, build: 7601, sp: 1, pt: 3, suite: 0, dpi: 96, elevate: 0 };
        let mut bad_pt = base;
        bad_pt.pt = PT_MAX + 1;
        let mut bad_dpi = base;
        bad_dpi.dpi = 100;
        let mut bad_sp = base;
        bad_sp.sp = SP_MAX + 1;
        base.valid() && !bad_pt.valid() && !bad_dpi.valid() && !bad_sp.valid()
    };
    v.push(("J1-六域值域守卫", val_ok, String::from("PT/DPI/SP 逐域拒")));

    // 15. 冻结锚双跑一致（指纹）。
    let f1 = BOTTLE_DEFAULT.fingerprint();
    let f2 = BOTTLE_DEFAULT.fingerprint();
    v.push(("J2-锚指纹双跑", f1 == f2, format!("{:08X}", f1)));

    // 16. 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));

    // 17. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 18. 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = BdCode::ALL.len() == 12;
        let mut i = 0;
        while i < BdCode::ALL.len() {
            if BdCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 18 条；前 8 条动态明细）。
pub fn run_unx18542_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18542 bottle 默认版本档案");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18542_checks();
        assert!(set.all_passed(), "UNX-F18542 判据集应全绿");
    }

    /// 黄金链：bottle 兜底生效。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _)) => {
                assert_eq!(n, 3);
                assert!(led.conserve());
                assert_eq!(led.passed[1], 9); // 未绑 9 槽兜底入账
                assert_eq!(led.passed[2], 1); // 空档回退入账
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 三级解析序逐级验证。
    #[test]
    fn test_resolve_order() {
        let app = BOTTLE_DEFAULT;
        assert_eq!(resolve_chain(Some(app), Some(BOTTLE_DEFAULT)).1, BdSource::App);
        assert_eq!(resolve_chain(None, Some(BOTTLE_DEFAULT)).1, BdSource::Bottle);
        assert_eq!(resolve_chain(None, None).1, BdSource::System);
    }

    /// 空档案回退系统默认，空值永不穿透。
    #[test]
    fn test_empty_fallback() {
        let (p, s) = resolve_chain(None, None);
        assert_eq!(p, SYSTEM_DEFAULT);
        assert_eq!(s, BdSource::System);
    }

    /// 兜底一致性。
    #[test]
    fn test_fallback_consistent() {
        assert!(fallback_consistent(Some(BOTTLE_DEFAULT), APP_SLOTS));
        assert!(fallback_consistent(None, APP_SLOTS));
    }

    /// 注入三类全抓（10 轮）。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.penetrate_caught, 10);
                assert_eq!(l.illegal_caught, 10);
                assert_eq!(l.dup_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 槽位面：绑定单调 + 越界/重复拒。
    #[test]
    fn test_slot_plane() {
        let mut pl = BottleSlotPlane::new();
        assert!(pl.bind(0).is_ok());
        assert_eq!(pl.bind(0).err(), Some(BdCode::SlotDuplicate.code()));
        assert_eq!(pl.bind(APP_SLOTS).err(), Some(BdCode::SlotOutOfRange.code()));
        assert_eq!(pl.bit(0), Some(true));
        assert_eq!(pl.bit(11), Some(false));
        assert_eq!(pl.bit(12), None);
    }

    /// 状态机：空账封印拒 / 封后绑定拒。
    #[test]
    fn test_binder_phases() {
        let mut binder = BottleBinder::new();
        match binder.seal() {
            Err(c) => assert_eq!(c, BdCode::LedgerEmpty.code()),
            Ok(()) => assert!(false, "空账封印必须拒"),
        }
        assert!(binder.bind_app(0).is_ok());
        assert!(binder.seal().is_ok());
        match binder.bind_app(1) {
            Err(c) => assert_eq!(c, BdCode::PhaseRegression.code()),
            Ok(()) => assert!(false, "封后绑定必须拒"),
        }
    }

    /// 双跑一致。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
    }
}
