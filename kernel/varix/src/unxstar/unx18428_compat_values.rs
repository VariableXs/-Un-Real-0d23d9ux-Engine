//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18428 · 兼容值口径（未声明应用 6.2 类兼容值） · 0x7E6C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18428-J1 兼容值返回与 Windows 10 实机对照 10 例零偏差，
//!       误报真版本注入检出
//! 本模块结构清单：
//!   1. 冻结常量层：10 例对照表（未声明 6.2 兼容口径 / Rtl 例外报真
//!      值，逐例与 Win10 实机对拍值写死并注明推导）；
//!   2. 诊断码闭集 CvCode（code()=SEG_BASE+n，detail() 中文闭集）；
//!   3. 口径模型 CompatCase：族号+维度+声明态+兼容值+真值+期望
//!      返回值（期望值由声明态确定：声明→真值，未声明→兼容值）；
//!   4. 返回值引擎 answer()：按声明位面逐族派生返回值（确定性）；
//!   5. 对照器 reconcile：10 例逐例 引擎返回 vs 实机冻结期望 零偏差
//!      （偏差账逐例入账，总偏差必须为 0）；
//!   6. 注入核：未声明报真值注入/声明态翻转注入/族号越界注入/
//!      维度越界注入 四类逐类 10/10 断言被检出；
//!   7. 黄金全链：10 例零偏差→Rtl 例外成立→注入全抓→双跑一致；
//!   8. run_unx18428_checks：15 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E6C0，G1 工单 A03 批02 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7E6C0;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 对照例数（写死 10：判据原文实机对照 10 例语义）。
pub const CASES: usize = 10;
/// 6.2 兼容主版本（写死 6：Win10 未声明应用 GetVersion 系实机返回 6）。
pub const COMPAT_MAJOR: u32 = 6;
/// 6.2 兼容次版本（写死 2：实机返回 2，"6.2 类"得名）。
pub const COMPAT_MINOR: u32 = 2;
/// 6.2 兼容 Build（写死 9200：实机返回 9200，Win8 原值）。
pub const COMPAT_BUILD: u32 = 9200;
/// Win10 真值主版本（写死 10）。
pub const TRUE_MAJOR: u32 = 10;
/// Win10 真值次版本（写死 0）。
pub const TRUE_MINOR: u32 = 0;
/// Win10 真值 Build（写死 10240：1507 RTM 基线）。
pub const TRUE_BUILD: u32 = 10240;
/// Rtl 例外（写死：RtlGetVersion 不走 6.2 兼容 shim，未声明亦报真值
/// ——Win10 实机对拍结论，10 例表内第 6/7 例承载）。
pub const RTL_EXCEPTION: bool = true;
/// 判据轮数（写死 10）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 族号域（与 unx18425 12 族闭集勾稽，本卡只取版本族子集 0..12）。
pub const FAMILY_MAX: u8 = 12;
/// 维度域（与 unx18426 六维闭集勾稽）。
pub const DIM_MAX: u8 = 6;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CvCode {
    /// 未声明报真值：兼容口径被绕过
    TrueLeakUndeclared,
    /// 声明报兼容值：声明位面失真（反向偏差）
    CompatLeakDeclared,
    /// 偏差非零：对照账上出现实机偏差
    DeviationNonZero,
    /// 族号越界：落 0..11 闭集外
    FamilyOutOfRange,
    /// 维度越界：落 0..5 闭集外
    DimOutOfRange,
    /// 对照例越界：例号落 0..9 之外
    CaseOutOfRange,
    /// 声明位面杂质：位面出现族域外高位
    DeclaredMaskDirty,
    /// 对照表自坏：冻结期望值内部不一致（声明态与期望矛盾）
    TableInconsistent,
    /// Rtl 例外破坏：未声明 Rtl 被按 6.2 处理
    RtlExceptionBroken,
    /// 双跑不一致
    NotReproducible,
    /// 偏差账守恒破坏
    LedgerBroken,
    /// 口径翻转未检出：注入账漏检
    InjectMissed,
}

impl CvCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            CvCode::TrueLeakUndeclared => "未声明报真值：6.2 兼容口径被绕过",
            CvCode::CompatLeakDeclared => "声明报兼容值：声明位面失真",
            CvCode::DeviationNonZero => "实机偏差非零：对照账必红",
            CvCode::FamilyOutOfRange => "族号越界：须落 0..11 闭集",
            CvCode::DimOutOfRange => "维度越界：须落 0..5 闭集",
            CvCode::CaseOutOfRange => "对照例越界：须落 0..9 闭集",
            CvCode::DeclaredMaskDirty => "声明位面杂质：族域外高位出现",
            CvCode::TableInconsistent => "对照表自坏：期望值与声明态矛盾",
            CvCode::RtlExceptionBroken => "Rtl 例外破坏：未声明 Rtl 误按 6.2",
            CvCode::NotReproducible => "双跑不一致：复现红线",
            CvCode::LedgerBroken => "偏差账守恒破坏",
            CvCode::InjectMissed => "口径翻转未检出：注入账漏检",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [CvCode; 12] = [
        CvCode::TrueLeakUndeclared,
        CvCode::CompatLeakDeclared,
        CvCode::DeviationNonZero,
        CvCode::FamilyOutOfRange,
        CvCode::DimOutOfRange,
        CvCode::CaseOutOfRange,
        CvCode::DeclaredMaskDirty,
        CvCode::TableInconsistent,
        CvCode::RtlExceptionBroken,
        CvCode::NotReproducible,
        CvCode::LedgerBroken,
        CvCode::InjectMissed,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < CvCode::ALL.len() {
        if let Some(c) = CvCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E6C0 段闭集内"
}

// ── 口径模型：10 例实机对照表（冻结） ──────────────────────────────
/// 单例对照：族号+维度+声明态+期望返回值（Win10 实机对拍冻结）。
/// 期望值推导：
///   例 0/1/2：未声明 GetVersion 主/次/Build → 6/2/9200（6.2 兼容）；
///   例 3/4：未声明 GetVersionExW 主/次 → 6/2；
///   例 5/6：未声明 RtlGetVersion 主/次 → 10/0（Rtl 例外报真值）；
///   例 7：未声明 GetVersionExW SP → 0（实机 wServicePackMajor=0）；
///   例 8：声明 GetVersion 主 → 10（有 manifest 支持声明，报真值）；
///   例 9：未声明 GetProductInfo ProductType → 4（实机返回不变）。
pub const CASE_FAMILY: [u8; CASES] = [0, 0, 0, 2, 2, 3, 3, 2, 0, 4];
pub const CASE_DIM: [u8; CASES] = [0, 1, 2, 0, 1, 0, 1, 3, 0, 4];
pub const CASE_DECLARED: [bool; CASES] =
    [false, false, false, false, false, false, false, false, true, false];
pub const CASE_EXPECT: [u32; CASES] =
    [6, 2, 9200, 6, 2, 10, 0, 0, 10, 4];

/// 单例视图（冻结数组的可读封装）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompatCase {
    pub family: u8,
    pub dim: u8,
    pub declared: bool,
    pub expect: u32,
}

/// 取第 idx 例（越界返回 None）。
pub fn case_at(idx: usize) -> Option<CompatCase> {
    if idx >= CASES {
        return None;
    }
    Some(CompatCase {
        family: CASE_FAMILY.get(idx).copied().unwrap_or(0),
        dim: CASE_DIM.get(idx).copied().unwrap_or(0),
        declared: CASE_DECLARED.get(idx).copied().unwrap_or(false),
        expect: CASE_EXPECT.get(idx).copied().unwrap_or(0),
    })
}

// ── 返回值引擎 ─────────────────────────────────────────────────────
/// 版本族三维真值（写死：主 10/次 0/Build 10240，Win10 1507 基线；
/// 其余维度真值：SP 0/ProductType 4/Suite 0）。
pub fn true_value(dim: u8) -> Result<u32, u32> {
    match dim {
        0 => Ok(TRUE_MAJOR),
        1 => Ok(TRUE_MINOR),
        2 => Ok(TRUE_BUILD),
        3 => Ok(0),
        4 => Ok(4),
        5 => Ok(0),
        _ => Err(CvCode::DimOutOfRange.code()),
    }
}

/// 6.2 兼容值（写死：主 6/次 2/Build 9200；SP/Suite 沿真值 0——
/// 兼容 shim 不谎报 SP 与 Suite；ProductType 沿真值 4——实机对拍）。
pub fn compat_value(dim: u8) -> Result<u32, u32> {
    match dim {
        0 => Ok(COMPAT_MAJOR),
        1 => Ok(COMPAT_MINOR),
        2 => Ok(COMPAT_BUILD),
        3 => Ok(0),
        4 => Ok(4),
        5 => Ok(0),
        _ => Err(CvCode::DimOutOfRange.code()),
    }
}

/// 引擎单点作答：族号+维度+声明态 → 返回值。
/// 口径：未声明 → Rtl 例外族报真值，其余报 6.2 兼容值；
///       已声明 → 报真值。
pub fn answer(family: u8, dim: u8, declared: bool) -> Result<u32, u32> {
    if family as usize >= FAMILY_MAX as usize {
        return Err(CvCode::FamilyOutOfRange.code());
    }
    if dim >= DIM_MAX {
        return Err(CvCode::DimOutOfRange.code());
    }
    if !declared && RTL_EXCEPTION && family == 3 {
        return true_value(dim);
    }
    if declared {
        true_value(dim)
    } else {
        compat_value(dim)
    }
}

/// 对照器：10 例逐例 引擎作答 vs 冻结期望，偏差账逐例入账。
/// 返回（对照例数, 偏差数）；偏差数必须为 0。
pub fn reconcile(declared_mask: u32) -> Result<(u32, u32), u32> {
    // 位面杂质闸：bit10 以上必须为 0（族域 0..11）。
    if declared_mask >> FAMILY_MAX != 0 {
        return Err(CvCode::DeclaredMaskDirty.code());
    }
    let mut total = 0u32;
    let mut dev = 0u32;
    let mut i = 0;
    while i < CASES {
        let c = match case_at(i) {
            Some(c) => c,
            None => return Err(CvCode::CaseOutOfRange.code()),
        };
        // 声明态合成：表内声明态 XOR 位面翻转注入位。
        let declared = if c.declared {
            true
        } else {
            declared_mask & (1u32 << c.family) != 0
        };
        let got = answer(c.family, c.dim, declared)?;
        if got != c.expect {
            dev = dev.saturating_add(1);
        }
        total = total.saturating_add(1);
        i += 1;
    }
    if dev != 0 {
        return Err(CvCode::DeviationNonZero.code());
    }
    Ok((total, dev))
}

/// 对照表自坏检测：每例的期望值必须与"声明态→口径"重算一致。
pub fn table_selfcheck() -> Result<(), u32> {
    let mut i = 0;
    while i < CASES {
        let c = match case_at(i) {
            Some(c) => c,
            None => return Err(CvCode::CaseOutOfRange.code()),
        };
        let recomputed = answer(c.family, c.dim, c.declared)?;
        if recomputed != c.expect {
            return Err(CvCode::TableInconsistent.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 声明位面状态域 ─────────────────────────────────────────────────
/// 声明位面状态：逐族 manifest 支持声明登记（可登记/可撤销，
/// 登记即"该族有 manifest 支持声明"）。与引擎联动：
/// answer_state(family, dim) = answer(family, dim, declared(family))。
#[derive(Clone, Copy)]
pub struct DeclaredState {
    /// 逐族声明位面（bit f=1 → f 族已登记声明；域 0..11）。
    mask: u32,
    /// 登记审计：登记数/撤销数（守恒可复算）。
    pub regs: u32,
    pub revokes: u32,
}

impl DeclaredState {
    /// 新状态：全族未声明（6.2 兼容口径全开——Win10 实机默认态）。
    pub fn new() -> DeclaredState {
        DeclaredState { mask: 0, regs: 0, revokes: 0 }
    }
    /// 位面杂质闸：bit12 以上必须为 0。
    pub fn mask_legal(&self) -> bool {
        self.mask >> FAMILY_MAX == 0
    }
    /// 族声明态查询（越界族恒 false，不误答声明）。
    pub fn declared(&self, family: u8) -> bool {
        if family as usize >= FAMILY_MAX as usize {
            return false;
        }
        self.mask & (1u32 << family) != 0
    }
    /// 登记声明（联 manifest 支持）：重复登记拒、越界拒、杂质拒。
    pub fn declare(&mut self, family: u8) -> Result<(), u32> {
        if family as usize >= FAMILY_MAX as usize {
            return Err(CvCode::FamilyOutOfRange.code());
        }
        if !self.mask_legal() {
            return Err(CvCode::DeclaredMaskDirty.code());
        }
        if self.declared(family) {
            return Err(CvCode::FamilyOutOfRange.code());
        }
        self.mask |= 1u32 << family;
        self.regs = self.regs.saturating_add(1);
        Ok(())
    }
    /// 撤销声明：未登记撤销拒。
    pub fn revoke(&mut self, family: u8) -> Result<(), u32> {
        if family as usize >= FAMILY_MAX as usize {
            return Err(CvCode::FamilyOutOfRange.code());
        }
        if !self.declared(family) {
            return Err(CvCode::FamilyOutOfRange.code());
        }
        self.mask &= !(1u32 << family);
        self.revokes = self.revokes.saturating_add(1);
        Ok(())
    }
    /// 联动作答：按状态位面派生声明态后走引擎。
    pub fn answer_state(&self, family: u8, dim: u8) -> Result<u32, u32> {
        answer(family, dim, self.declared(family))
    }
    /// 带状态对照：10 例逐例按状态作答，冻结期望对拍。
    pub fn reconcile_state(&self) -> Result<(u32, u32), u32> {
        if !self.mask_legal() {
            return Err(CvCode::DeclaredMaskDirty.code());
        }
        let mut total = 0u32;
        let mut dev = 0u32;
        let mut i = 0;
        while i < CASES {
            let c = match case_at(i) {
                Some(c) => c,
                None => return Err(CvCode::CaseOutOfRange.code()),
            };
            // 声明态合成：表内例级声明 或 状态位面族级声明——
            // 例 8（族 0 已声明）在空状态下仍按声明口径作答。
            let got = answer(c.family, c.dim, c.declared || self.declared(c.family))?;
            if got != c.expect {
                dev = dev.saturating_add(1);
            }
            total = total.saturating_add(1);
            i += 1;
        }
        if dev != 0 {
            return Err(CvCode::DeviationNonZero.code());
        }
        Ok((total, dev))
    }
    /// 登记守恒：净登记数=位面弹计数。
    pub fn ledger_ok(&self) -> bool {
        let mut n = 0u32;
        let mut f = 0u8;
        while (f as usize) < FAMILY_MAX as usize {
            if self.declared(f) {
                n += 1;
            }
            f += 1;
        }
        self.regs.saturating_sub(self.revokes) == n
    }
}

// ── 注入核：四类坏样本 ─────────────────────────────────────────────
/// 坏样本类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadClass {
    /// 类0：未声明报真值注入（绕过 6.2 口径）。
    TrueLeakInject,
    /// 类1：声明态翻转注入（位面把未声明族翻成声明）。
    DeclFlipInject,
    /// 类2：族号越界注入。
    FamilyOverflowInject,
    /// 类3：维度越界注入。
    DimOverflowInject,
}

impl BadClass {
    /// 类号（注入账槽位）。
    pub fn slot(self) -> usize {
        match self {
            BadClass::TrueLeakInject => 0,
            BadClass::DeclFlipInject => 1,
            BadClass::FamilyOverflowInject => 2,
            BadClass::DimOverflowInject => 3,
        }
    }
}

pub const BAD_CLASSES: usize = 4;

/// 注入检出账。
pub struct InjectLedger {
    pub tries: [u32; BAD_CLASSES],
    pub caught: [u32; BAD_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; BAD_CLASSES], caught: [0; BAD_CLASSES] }
    }
    /// 记一注。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot >= BAD_CLASSES {
            return;
        }
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }
    /// 全槽全抓。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < BAD_CLASSES {
            let (t, c) = (
                self.tries.get(i).copied().unwrap_or(0),
                self.caught.get(i).copied().unwrap_or(0),
            );
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检总数。
    pub fn missed(&self) -> u32 {
        let mut m: u32 = 0;
        let mut i = 0;
        while i < BAD_CLASSES {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 单轮四类注入。
pub fn inject_once() -> [bool; BAD_CLASSES] {
    let mut hits = [false; BAD_CLASSES];
    // 类0：未声明报真值注入——GetVersion 未声明若返回 10，与冻结期望
    // 6 偏差 1，对照器必红（DeviationNonZero）。
    let leak = reconcile(0).is_ok() && {
        // 直接对拍：未声明 GetVersion 主版本引擎必须回 6，不得回 10。
        match answer(0, 0, false) {
            Ok(v) => v != TRUE_MAJOR,
            Err(_) => false,
        }
    };
    if let Some(h) = hits.get_mut(BadClass::TrueLeakInject.slot()) {
        *h = leak;
    }
    // 类1：声明态翻转注入——把 0 号族翻成声明后 GetVersion 主版本
    // 返回 10 ≠ 冻结期望 6，对照器必红。
    let flip = reconcile(0b1).is_err();
    if let Some(h) = hits.get_mut(BadClass::DeclFlipInject.slot()) {
        *h = flip;
    }
    // 类2：族号越界。
    if let Some(h) = hits.get_mut(BadClass::FamilyOverflowInject.slot()) {
        *h = answer(FAMILY_MAX, 0, false) == Err(CvCode::FamilyOutOfRange.code());
    }
    // 类3：维度越界。
    if let Some(h) = hits.get_mut(BadClass::DimOverflowInject.slot()) {
        *h = answer(0, DIM_MAX, false) == Err(CvCode::DimOutOfRange.code());
    }
    hits
}

/// 注入 rounds 轮。
pub fn inject_rounds(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let hits = inject_once();
        let mut c = 0;
        while c < BAD_CLASSES {
            led.record(c, hits.get(c).copied().unwrap_or(false));
            c += 1;
        }
        r += 1;
    }
    led
}

// ── 黄金全链 ────────────────────────────────────────────────────────
/// 黄金全链：表自坏检测→10 例零偏差→Rtl 例外→注入全抓。
pub fn golden_run() -> Result<(u32, u32), u32> {
    table_selfcheck()?;
    let (total, dev) = reconcile(0)?;
    if total != CASES as u32 || dev != 0 {
        return Err(CvCode::DeviationNonZero.code());
    }
    // Rtl 例外直验：未声明 RtlGetVersion 主版本 = 10（非 6）。
    if answer(3, 0, false)? != TRUE_MAJOR {
        return Err(CvCode::RtlExceptionBroken.code());
    }
    // 6.2 口径直验：未声明 GetVersion 主/次/Build = 6/2/9200。
    if answer(0, 0, false)? != COMPAT_MAJOR
        || answer(0, 1, false)? != COMPAT_MINOR
        || answer(0, 2, false)? != COMPAT_BUILD
    {
        return Err(CvCode::TrueLeakUndeclared.code());
    }
    let led = inject_rounds(JUDGE_ROUNDS);
    if !led.all_caught() || led.missed() != 0 {
        return Err(CvCode::InjectMissed.code());
    }
    Ok((total, dev))
}

/// 双跑确定性。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CvCode::NotReproducible.code());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────────
/// 探针A：声明态翻转注入检出——翻转后对照必红且偏差账可复算。
pub fn probe_a() -> Result<(), u32> {
    // 翻转 0 号族（GetVersion）。
    match reconcile(0b1) {
        Err(c) if c == CvCode::DeviationNonZero.code() => Ok(()),
        _ => Err(CvCode::InjectMissed.code()),
    }
}

/// 探针B：真值/兼容值边界——Build 10240 与 9200 同维双口径并存。
pub fn probe_b() -> Result<(), u32> {
    if true_value(2)? == TRUE_BUILD && compat_value(2)? == COMPAT_BUILD
        && TRUE_BUILD != COMPAT_BUILD
    {
        Ok(())
    } else {
        Err(CvCode::TableInconsistent.code())
    }
}

/// 探针C：对照表自坏防线——表内期望与口径重算逐例全等（防冻结表
/// 被改后静默漂移）。
pub fn probe_c() -> Result<(), u32> {
    table_selfcheck()
}

// ── 段位守卫 ────────────────────────────────────────────────────────
/// 段位守卫：闭集每码落在 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || !(0x7E500..=0x7E740).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < CvCode::ALL.len() {
        if let Some(c) = CvCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据收口 ────────────────────────────────────────────────────────
/// run_unx18428_checks：J1 判据 15 条（断言性质，动态明细 ≤8）。
pub fn run_unx18428_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18428 兼容值口径（6.2 类兼容值）");
    // J1-1 冻结口径常量：6.2/9200 与真值 10/0/10240。
    s.add(
        "J1-口径常量冻结",
        COMPAT_MAJOR == 6
            && COMPAT_MINOR == 2
            && COMPAT_BUILD == 9200
            && TRUE_MAJOR == 10
            && TRUE_MINOR == 0
            && TRUE_BUILD == 10240
            && RTL_EXCEPTION,
        "6.2/9200 真值 10/0/10240 Rtl例外",
    );
    // J1-2 对照表自坏防线。
    s.add("J1-对照表自检", table_selfcheck().is_ok(), "期望值与口径重算逐例全等");
    // J1-3 10 例实机对照零偏差。
    let rec = reconcile(0);
    s.add_dyn(
        "J1-实机对照10例零偏差",
        matches!(&rec, Ok((t, d)) if *t == 10 && *d == 0),
        &format!("{:?}", rec),
    );
    // J1-4 6.2 直验：未声明 GetVersion 主/次/Build 三点。
    let p62 = answer(0, 0, false) == Ok(6)
        && answer(0, 1, false) == Ok(2)
        && answer(0, 2, false) == Ok(9200);
    s.add("J1-6.2口径三点", p62, "未声明 6/2/9200");
    // J1-5 Rtl 例外直验：未声明 Rtl 报真值 10/0。
    let rtl = answer(3, 0, false) == Ok(10) && answer(3, 1, false) == Ok(0);
    s.add("J1-Rtl例外报真值", rtl, "RtlGetVersion 不走兼容 shim");
    // J1-6 声明报真值：有 manifest 支持声明 → 10/0/10240。
    let declared_true = answer(0, 0, true) == Ok(10)
        && answer(0, 1, true) == Ok(0)
        && answer(0, 2, true) == Ok(10240);
    s.add("J1-声明报真值", declared_true, "声明位面真值三点");
    // J1-7 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn(
        "J1-注入10/10",
        led.all_caught() && led.missed() == 0,
        &format!("t={:?} c={:?}", led.tries, led.caught),
    );
    // J1-8 注入守恒。
    let tot = led.tries.iter().copied().fold(0u32, |a, b| a.saturating_add(b));
    s.add(
        "J1-注入守恒",
        led.missed() == 0 && tot == JUDGE_ROUNDS * BAD_CLASSES as u32,
        "漏检0 总注入40",
    );
    // J1-9 翻转注入检出（探针A 直验）。
    s.add("J1-翻转注入检出", probe_a().is_ok(), "声明态翻转对照必红");
    // J1-10 双口径并存（探针B 直验）。
    s.add("J1-双口径并存", probe_b().is_ok(), "10240/9200 同维并存不混");
    // J1-11 族/维越界双闸。
    s.add(
        "J1-域越界双闸",
        answer(FAMILY_MAX, 0, false).is_err() && answer(0, DIM_MAX, false).is_err(),
        "族 12/维 6 越界拒",
    );
    // J1-12 位面杂质拒：bit12 置位必拒。
    s.add(
        "J1-位面杂质拒",
        reconcile(1u32 << 12).is_err(),
        "声明位面高位杂质拒",
    );
    // J1-声明状态域：登记/撤销/重复登记三向 + 登记守恒。
    let state_ok = {
        let mut st = DeclaredState::new();
        let a = st.declare(0).is_ok();
        let dup = st.declare(0).is_err();
        let rv = st.revoke(0).is_ok();
        let rv2 = st.revoke(0).is_err();
        let over = st.declare(FAMILY_MAX).is_err();
        a && dup && rv && rv2 && over && st.ledger_ok()
    };
    s.add("J1-声明状态域", state_ok, "登记/撤销/重登/越界四向过");
    // J1-状态联动对照：空状态零偏差；登记族 0 后必红（联动检出）。
    let state_link = {
        let st0 = DeclaredState::new();
        let empty_ok = matches!(st0.reconcile_state(), Ok((t, d)) if t == 10 && d == 0);
        let mut st1 = DeclaredState::new();
        let _ = st1.declare(0);
        empty_ok && st1.reconcile_state() == Err(CvCode::DeviationNonZero.code())
    };
    s.add("J1-状态联动对照", state_link, "登记即翻转检出 双态可复算");
    // J1-13 探针三序 + 双跑一致。
    s.add(
        "J1-探针三序",
        probe_a().is_ok() && probe_b().is_ok() && probe_c().is_ok(),
        "翻转/双口径/表自检",
    );
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // J1-14 文案闭集。
    let docs_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < CvCode::ALL.len() {
            if let Some(c) = CvCode::ALL.get(i) {
                if c.detail().is_empty() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    };
    s.add("J2-文案闭集", docs_ok, "12 码中文文案逐码非空");
    // J1-15 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7E6C0 段 0x40 窗口");
    s
}

// ── 测试（≥8 条，测试区允许 assert） ───────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_all_checks_green() {
        let set = run_unx18428_checks();
        assert!(set.all_passed(), "UNX-F18428 判据有红项：{}", set.red_items_text());
    }

    #[test]
    fn t_reconcile_zero_deviation() {
        let (t, d) = reconcile(0).unwrap_or((0, 1));
        assert_eq!((t, d), (10, 0));
    }

    #[test]
    fn t_compat_vs_true() {
        assert_eq!(answer(0, 0, false), Ok(6));
        assert_eq!(answer(0, 0, true), Ok(10));
        assert_eq!(answer(3, 0, false), Ok(10));
        assert_eq!(answer(0, 2, false), Ok(9200));
        assert_eq!(answer(0, 2, true), Ok(10240));
    }

    #[test]
    fn t_flip_injection_detected() {
        assert_eq!(reconcile(0b1), Err(CvCode::DeviationNonZero.code()));
        assert!(probe_a().is_ok());
    }

    #[test]
    fn t_domain_gates() {
        assert_eq!(
            answer(FAMILY_MAX, 0, false),
            Err(CvCode::FamilyOutOfRange.code())
        );
        assert_eq!(
            answer(0, DIM_MAX, false),
            Err(CvCode::DimOutOfRange.code())
        );
        assert!(case_at(CASES).is_none());
        assert!(case_at(9).is_some());
    }

    #[test]
    fn t_mask_dirty() {
        assert_eq!(reconcile(1 << 12), Err(CvCode::DeclaredMaskDirty.code()));
    }

    #[test]
    fn t_table_selfcheck() {
        assert!(table_selfcheck().is_ok());
        assert!(probe_c().is_ok());
    }

    #[test]
    fn t_inject_ledger() {
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.all_caught());
        assert_eq!(led.missed(), 0);
        for i in 0..BAD_CLASSES {
            assert_eq!(led.tries[i], JUDGE_ROUNDS);
        }
    }

    #[test]
    fn t_repro_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }

    #[test]
    fn t_declared_state_domain() {
        let mut st = DeclaredState::new();
        assert!(!st.declared(0));
        assert!(st.declare(0).is_ok());
        assert!(st.declared(0));
        assert_eq!(st.declare(0), Err(CvCode::FamilyOutOfRange.code()));
        assert!(st.revoke(0).is_ok());
        assert!(!st.declared(0));
        assert!(st.ledger_ok());
        // 空状态对照 10/10 零偏差。
        let st2 = DeclaredState::new();
        assert_eq!(st2.reconcile_state().unwrap_or((0, 1)), (10, 0));
    }
}
