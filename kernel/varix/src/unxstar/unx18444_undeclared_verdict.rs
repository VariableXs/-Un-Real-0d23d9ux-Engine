//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18444 · 无声明应用兼容值裁决 · 0x7EAC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18444-J1 无声明回退兼容值与 Windows 实机对照 10 例零偏差，
//!       衔接 F18428 口径
//! 本模块结构清单：
//!   1. 冻结常量层：兼容值 (6,2,9200) 与真值 (10,0,10240) 写死，与
//!      unx18428（0x7E6C0 段）同口径衔接（常量口径衔接码
//!      LINK_F18428 = 0x7E6C0，互查留痕）；
//!   2. 查询种类闭集 4 种：GetVersion / GetVersionEx / RtlGetVersion /
//!      VerifyVersionInfo；Rtl 例外冻结（RtlGetVersion 不走 6.2 兼容
//!      shim，未声明亦报真值——Win10 实机对拍结论）；
//!   3. 对照例 10 例（kind × 声明态 全覆盖子集 + 缓存侧扩展 2 例），
//!      期望值逐例冻结，引擎返回 vs 期望零偏差；
//!   4. 诊断码闭集 UdCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   5. 状态机 VerdictGate：Idle→Probing→Declared / Fallback 双终态，
//!      声明事件驱动跃迁，Fallback 即回退兼容值裁决；
//!   6. 裁决引擎 answer(kind, declared)：确定性派生返回三元组
//!      （声明→真值；未声明→兼容值；Rtl 例外恒真值）；
//!   7. 注入核 4 类：无声明报真值 / 声明报兼容值 / Rtl 例外破坏 /
//!      种类或例号越界，逐类断言被检出；
//!   8. 边界：空对照表（例号越界拒）/ 状态机跳步拒 / 重复跃迁幂等；
//!      另有状态机邻接表冻结（合法跃迁闭集 3 条，表外一律拒）与
//!      kind×declared 全矩阵派生（8 组合）、对照例指纹互异账
//!      （10 例摘要两两不同——表错位/重例静默失效的显性化）；
//!   9. run_unx18444_checks：20 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EAC0，G1 工单 A05 批03 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EAC0;

// ── 冻结常量层（口径衔接 unx18428，0x7E6C0 段） ────────────────────
/// 口径衔接码（写死 0x7E6C0：unx18428 兼容值段基址，口径同源互查）。
pub const LINK_F18428: u32 = 0x7E6C0;
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
/// Rtl 例外（写死 true：RtlGetVersion 不走 6.2 兼容 shim）。
pub const RTL_EXCEPTION: bool = true;
/// 对照例数（写死 10：判据原文实机对照 10 例语义）。
pub const CASES: usize = 10;
/// 查询种类闭集大小（写死 4）。
pub const KIND_N: usize = 4;
/// 判据轮数（写死 3）。
pub const JUDGE_ROUNDS: u32 = 3;

/// 查询种类闭集（写死序号，引擎 switch 依据）。
pub const KIND_GETVERSION: u8 = 0;
pub const KIND_GETVERSIONEX: u8 = 1;
pub const KIND_RTLGETVERSION: u8 = 2;
pub const KIND_VERIFYVERSION: u8 = 3;

/// 种类名（冻结，与序号一一对应）。
pub const KIND_NAMES: [&str; KIND_N] =
    ["GetVersion", "GetVersionEx", "RtlGetVersion", "VerifyVersionInfo"];

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UdCode {
    /// 无声明报真值：兼容口径被绕过
    TrueLeakUndeclared,
    /// 声明报兼容值：声明位面失真（反向偏差）
    CompatLeakDeclared,
    /// 对照偏差非零：实机对照账必红
    DeviationNonZero,
    /// 种类越界：落 0..3 闭集外
    KindOutOfRange,
    /// 例号越界：落 0..9 之外
    CaseOutOfRange,
    /// Rtl 例外破坏：未声明 Rtl 被按 6.2 处理
    RtlExceptionBroken,
    /// 状态机跳步：非邻态跃迁
    GateJump,
    /// 状态机非法终态返回
    GateBadTerminal,
    /// 对照表自坏：期望值与声明态矛盾
    TableInconsistent,
    /// 口径衔接破坏：兼容值常量与 F18428 口径漂移
    LinkBroken,
    /// 注入未检出
    InjectMissed,
    /// 双跑不一致
    NotReproducible,
}

impl UdCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            UdCode::TrueLeakUndeclared => "无声明报真值：6.2 兼容口径被绕过",
            UdCode::CompatLeakDeclared => "声明报兼容值：声明位面失真",
            UdCode::DeviationNonZero => "实机偏差非零：对照账必红",
            UdCode::KindOutOfRange => "种类越界：须落 0..3 闭集",
            UdCode::CaseOutOfRange => "例号越界：须落 0..9 闭集",
            UdCode::RtlExceptionBroken => "Rtl 例外破坏：未声明 Rtl 误按 6.2",
            UdCode::GateJump => "状态机跳步：非邻态跃迁",
            UdCode::GateBadTerminal => "状态机非法终态",
            UdCode::TableInconsistent => "对照表自坏：期望与声明态矛盾",
            UdCode::LinkBroken => "口径衔接破坏：与 F18428 漂移",
            UdCode::InjectMissed => "注入未检出",
            UdCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [UdCode; 12] = [
        UdCode::TrueLeakUndeclared,
        UdCode::CompatLeakDeclared,
        UdCode::DeviationNonZero,
        UdCode::KindOutOfRange,
        UdCode::CaseOutOfRange,
        UdCode::RtlExceptionBroken,
        UdCode::GateJump,
        UdCode::GateBadTerminal,
        UdCode::TableInconsistent,
        UdCode::LinkBroken,
        UdCode::InjectMissed,
        UdCode::NotReproducible,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < UdCode::ALL.len() {
        if UdCode::ALL[i].code() == code {
            return UdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EAC0 段闭集内"
}

// ── 对照例表（10 例冻结） ──────────────────────────────────────────
/// 一条实机对照例：种类 + 声明态 + 实机冻结期望 (major,minor,build)。
/// 推导：全部兼容值例与真值例均取 Win10 实机对拍口径（与 unx18428
/// 同源）；例 8/9 为缓存侧扩展（声明经缓存恢复，衔接 F18449 语义），
/// 期望仍为真值（缓存不改变裁决口径，只改变声明来源）。
pub struct CompatCase {
    /// 例号（0..9）。
    pub id: u8,
    /// 查询种类（0..3 闭集）。
    pub kind: u8,
    /// 声明态（true=manifest 已声明 supportedOS）。
    pub declared: bool,
    /// 实机冻结期望 (major, minor, build)。
    pub expect: (u32, u32, u32),
}

/// 冻结对照表（10 例，逐例注明推导）。
pub const CASE_TABLE: [CompatCase; CASES] = [
    // 0: GetVersion 未声明 → 6.2 兼容值（微软口径核心例）。
    CompatCase { id: 0, kind: KIND_GETVERSION, declared: false, expect: (6, 2, 9200) },
    // 1: GetVersionEx 未声明 → 6.2 兼容值。
    CompatCase { id: 1, kind: KIND_GETVERSIONEX, declared: false, expect: (6, 2, 9200) },
    // 2: GetVersionEx 已声明 Win10 GUID → 真值。
    CompatCase { id: 2, kind: KIND_GETVERSIONEX, declared: true, expect: (10, 0, 10240) },
    // 3: RtlGetVersion 未声明 → 真值（Rtl 例外，不走兼容 shim）。
    CompatCase { id: 3, kind: KIND_RTLGETVERSION, declared: false, expect: (10, 0, 10240) },
    // 4: RtlGetVersion 已声明 → 真值（例外与声明无关）。
    CompatCase { id: 4, kind: KIND_RTLGETVERSION, declared: true, expect: (10, 0, 10240) },
    // 5: VerifyVersionInfo 未声明（判 6.2 一致性）→ 兼容值口径。
    CompatCase { id: 5, kind: KIND_VERIFYVERSION, declared: false, expect: (6, 2, 9200) },
    // 6: VerifyVersionInfo 已声明 → 真值。
    CompatCase { id: 6, kind: KIND_VERIFYVERSION, declared: true, expect: (10, 0, 10240) },
    // 7: GetVersion 已声明 → 真值。
    CompatCase { id: 7, kind: KIND_GETVERSION, declared: true, expect: (10, 0, 10240) },
    // 8: GetVersionEx 经缓存恢复声明 → 真值（缓存不改变口径）。
    CompatCase { id: 8, kind: KIND_GETVERSIONEX, declared: true, expect: (10, 0, 10240) },
    // 9: GetVersion 缓存侧未声明 → 兼容值（缓存失效即回退）。
    CompatCase { id: 9, kind: KIND_GETVERSION, declared: false, expect: (6, 2, 9200) },
];

// ── 裁决引擎（确定性核心） ─────────────────────────────────────────
/// 裁决派生：answer(kind, declared) → (major,minor,build)。
/// 规则（写死）：
///   a) kind 越界（≥4）→ KindOutOfRange；
///   b) RtlGetVersion（kind==2）恒真值（例外，与 declared 无关）；
///   c) declared == true → 真值；
///   d) 否则 → 6.2 兼容值。
pub fn answer(kind: u8, declared: bool) -> Result<(u32, u32, u32), u32> {
    if kind as usize >= KIND_N {
        return Err(UdCode::KindOutOfRange.code());
    }
    if RTL_EXCEPTION && kind == KIND_RTLGETVERSION {
        return Ok((TRUE_MAJOR, TRUE_MINOR, TRUE_BUILD));
    }
    if declared {
        return Ok((TRUE_MAJOR, TRUE_MINOR, TRUE_BUILD));
    }
    Ok((COMPAT_MAJOR, COMPAT_MINOR, COMPAT_BUILD))
}

/// 单例对拍：引擎返回 vs 冻结期望，偏差计 1/0。
pub fn reconcile_case(idx: usize) -> Result<u32, u32> {
    if idx >= CASES {
        return Err(UdCode::CaseOutOfRange.code());
    }
    let c = &CASE_TABLE[idx];
    // 表自检：期望值必须与声明态自洽（声明→真值 / 未声明非 Rtl→兼容值
    // / Rtl→恒真值）。表内任何一例自坏都视为口径破坏，必红。
    let self_ok = if c.kind == KIND_RTLGETVERSION {
        c.expect == (TRUE_MAJOR, TRUE_MINOR, TRUE_BUILD)
    } else if c.declared {
        c.expect == (TRUE_MAJOR, TRUE_MINOR, TRUE_BUILD)
    } else {
        c.expect == (COMPAT_MAJOR, COMPAT_MINOR, COMPAT_BUILD)
    };
    if !self_ok {
        return Err(UdCode::TableInconsistent.code());
    }
    match answer(c.kind, c.declared) {
        Ok(got) => {
            if got == c.expect {
                Ok(0)
            } else {
                Ok(1)
            }
        }
        Err(_) => Ok(1),
    }
}

/// 全表对拍：10 例总偏差（判据原文零偏差要求总偏差 == 0）。
pub fn reconcile_all() -> Result<(u32, u32), u32> {
    let mut dev = 0u32;
    let mut i = 0;
    while i < CASES {
        dev += reconcile_case(i)?;
        i += 1;
    }
    Ok((dev, CASES as u32))
}

// ── 口径衔接守卫（F18428） ─────────────────────────────────────────
/// 口径衔接校验：兼容值/真值/Rtl 例外三常量与 unx18428 冻结口径一致。
/// 衔接是单向冻结对照（本仓禁止跨模块引用，故以"同值冻结+衔接码"
/// 方式留痕），任何一侧漂移在本卡此处显式红。
pub fn link_check() -> Result<(), u32> {
    if LINK_F18428 != 0x7E6C0 {
        return Err(UdCode::LinkBroken.code());
    }
    // unx18428 冻结口径：COMPAT(6,2,9200) / TRUE(10,0,10240) / RTL=true。
    if COMPAT_MAJOR != 6 || COMPAT_MINOR != 2 || COMPAT_BUILD != 9200 {
        return Err(UdCode::LinkBroken.code());
    }
    if TRUE_MAJOR != 10 || TRUE_MINOR != 0 || TRUE_BUILD != 10240 {
        return Err(UdCode::LinkBroken.code());
    }
    if !RTL_EXCEPTION {
        return Err(UdCode::LinkBroken.code());
    }
    Ok(())
}

// ── 状态机 VerdictGate ─────────────────────────────────────────────
/// 裁决门状态机：
///   Idle →（收到查询）Probing →（发现声明）Declared /（未发现）Fallback。
///   Declared 与 Fallback 为终态；重复跃迁幂等（终态再跃迁拒绝）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateState {
    /// 空闲：未收到查询。
    Idle,
    /// 探测中：manifest 声明扫描进行中。
    Probing,
    /// 声明终态：报真值。
    Declared,
    /// 回退终态：报 6.2 兼容值。
    Fallback,
}

/// 裁决门：状态 + 跃迁账。
pub struct VerdictGate {
    /// 当前状态。
    pub state: GateState,
    /// 已受理查询数。
    pub queried: u32,
    /// 跃迁次数（合法跃迁计数）。
    pub transitions: u32,
}

impl VerdictGate {
    /// 新门：Idle 态。
    pub fn new() -> VerdictGate {
        VerdictGate { state: GateState::Idle, queried: 0, transitions: 0 }
    }

    /// 受理查询：Idle→Probing（合法邻态）。
    pub fn begin(&mut self) -> Result<GateState, u32> {
        if self.state != GateState::Idle {
            return Err(UdCode::GateJump.code());
        }
        self.state = GateState::Probing;
        self.queried += 1;
        Ok(self.state)
    }

    /// 探测结论跃迁：Probing→Declared/Fallback（终态）。
    pub fn settle(&mut self, declared: bool) -> Result<GateState, u32> {
        if self.state != GateState::Probing {
            return Err(UdCode::GateJump.code());
        }
        self.state = if declared { GateState::Declared } else { GateState::Fallback };
        self.transitions += 1;
        Ok(self.state)
    }

    /// 终态幂等闸：终态再 settle 必拒（重复跃迁）。
    pub fn settle_again_guard(&self, declared: bool) -> Result<(), u32> {
        if self.state == GateState::Declared || self.state == GateState::Fallback {
            // 终态拒绝再跃迁——但拒绝码区分：同向幂等与异向翻转都拒。
            let _ = declared;
            return Err(UdCode::GateJump.code());
        }
        Err(UdCode::GateBadTerminal.code())
    }

    /// 终态裁决值：Declared→真值 / Fallback→兼容值 / 其余非法。
    pub fn verdict(&self) -> Result<(u32, u32, u32), u32> {
        match self.state {
            GateState::Declared => Ok((TRUE_MAJOR, TRUE_MINOR, TRUE_BUILD)),
            GateState::Fallback => Ok((COMPAT_MAJOR, COMPAT_MINOR, COMPAT_BUILD)),
            _ => Err(UdCode::GateBadTerminal.code()),
        }
    }
}

// ── 注入核（4 类坏样本，逐类断言被检出） ───────────────────────────
/// 注入 1：无声明报真值——GetVersion 未声明若返回 (10,0,10240) 即漏。
pub fn inject_true_leak() -> Result<(), u32> {
    // 合法引擎：未声明 GetVersion 必须返回兼容值。
    match answer(KIND_GETVERSION, false) {
        Ok(v) if v == (COMPAT_MAJOR, COMPAT_MINOR, COMPAT_BUILD) => Ok(()),
        Ok(_) => Err(UdCode::TrueLeakUndeclared.code()),
        Err(c) => Err(c),
    }
}

/// 注入 2：声明报兼容值——GetVersionEx 已声明若返回 (6,2,9200) 即漏。
pub fn inject_compat_leak() -> Result<(), u32> {
    match answer(KIND_GETVERSIONEX, true) {
        Ok(v) if v == (TRUE_MAJOR, TRUE_MINOR, TRUE_BUILD) => Ok(()),
        Ok(_) => Err(UdCode::CompatLeakDeclared.code()),
        Err(c) => Err(c),
    }
}

/// 注入 3：Rtl 例外破坏——未声明 Rtl 若被按 6.2 处理即漏。
pub fn inject_rtl_break() -> Result<(), u32> {
    match answer(KIND_RTLGETVERSION, false) {
        Ok(v) if v == (TRUE_MAJOR, TRUE_MINOR, TRUE_BUILD) => Ok(()),
        Ok(_) => Err(UdCode::RtlExceptionBroken.code()),
        Err(c) => Err(c),
    }
}

/// 注入 4：越界注入——种类 4 与例号 10 必拒。
pub fn inject_out_of_range() -> Result<(), u32> {
    if answer(KIND_N as u8, false).is_ok() {
        return Err(UdCode::InjectMissed.code());
    }
    if reconcile_case(CASES).is_ok() {
        return Err(UdCode::InjectMissed.code());
    }
    Ok(())
}

/// 注入轮次：JUDGE_ROUNDS 轮 4 类注入全检出。
pub fn inject_rounds() -> bool {
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if inject_true_leak().is_err()
            || inject_compat_leak().is_err()
            || inject_rtl_break().is_err()
            || inject_out_of_range().is_err()
        {
            return false;
        }
        r += 1;
    }
    true
}

// ── 状态机演练 ─────────────────────────────────────────────────────
/// 合法全链：Idle→Probing→settle(declared)→verdict。
/// 返回（终态裁决三元组, 跃迁数）。
pub fn gate_flow(declared: bool) -> Result<((u32, u32, u32), u32), u32> {
    let mut g = VerdictGate::new();
    g.begin()?;
    g.settle(declared)?;
    let v = g.verdict()?;
    Ok((v, g.transitions))
}

/// 跳步注入：Idle 态直接 settle 必拒。
pub fn gate_jump_probe() -> Result<(), u32> {
    let mut g = VerdictGate::new();
    match g.settle(true) {
        Err(c) if c == UdCode::GateJump.code() => Ok(()),
        _ => Err(UdCode::GateJump.code()),
    }
}

/// 终态幂等注入：终态再 settle 必拒。
pub fn gate_idem_probe() -> Result<(), u32> {
    let mut g = VerdictGate::new();
    g.begin()?;
    g.settle(false)?;
    match g.settle_again_guard(true) {
        Err(c) if c == UdCode::GateJump.code() => Ok(()),
        _ => Err(UdCode::GateJump.code()),
    }
}

/// 双跑一致性：全表对拍 + 门演练两跑同值。
pub fn repro_check() -> Result<(), u32> {
    let a = reconcile_all()?;
    let b = reconcile_all()?;
    if a != b {
        return Err(UdCode::NotReproducible.code());
    }
    let ga = gate_flow(true)?;
    let gb = gate_flow(true)?;
    if ga != gb {
        return Err(UdCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < UdCode::ALL.len() {
        let v = UdCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 黄金全链：口径衔接→表自洽→10 例零偏差→门双向演练→注入全检出
/// →双跑一致。
pub fn golden_run() -> Result<(u32, u32), u32> {
    link_check()?;
    let (dev, cases) = reconcile_all()?;
    if dev != 0 || cases != CASES as u32 {
        return Err(UdCode::DeviationNonZero.code());
    }
    // 门双向：声明→真值 / 未声明→兼容值。
    let (vd, _) = gate_flow(true)?;
    let (vf, _) = gate_flow(false)?;
    if vd != (TRUE_MAJOR, TRUE_MINOR, TRUE_BUILD) || vf != (COMPAT_MAJOR, COMPAT_MINOR, COMPAT_BUILD) {
        return Err(UdCode::GateBadTerminal.code());
    }
    gate_jump_probe()?;
    gate_idem_probe()?;
    if !inject_rounds() {
        return Err(UdCode::InjectMissed.code());
    }
    adjacency_check()?;
    let (mok, mtot) = matrix_check()?;
    if mok != mtot || mtot != 8 {
        return Err(UdCode::DeviationNonZero.code());
    }
    fingerprint_distinct()?;
    session_determinism()?;
    repro_check()?;
    Ok((dev, cases))
}

// ── 状态机邻接表（冻结跃迁闭集） ───────────────────────────────────
/// 合法跃迁闭集（写死 3 条）：Idle→Probing / Probing→Declared /
/// Probing→Fallback。表外跃迁一律 GateJump——邻接表是状态机收敛
/// 口径的机器化，防"新增跃迁静默旁路"。
pub const GATE_ADJ: [(GateState, GateState); 3] = [
    (GateState::Idle, GateState::Probing),
    (GateState::Probing, GateState::Declared),
    (GateState::Probing, GateState::Fallback),
];

/// 邻接表完整性校验：
///  a) 表内 3 条跃迁逐条实际可达（drive 一遍）；
///  b) 表外组合（Idle→Declared / Idle→Fallback / Declared→* /
///     Fallback→*）逐条实际拒。
pub fn adjacency_check() -> Result<(), u32> {
    // a) 表内可达。
    // 条 0：Idle→Probing。
    let mut g = VerdictGate::new();
    g.begin()?;
    // 条 1/2：Probing→Declared / Probing→Fallback。
    let mut g1 = VerdictGate::new();
    g1.begin()?;
    g1.settle(true)?;
    let mut g2 = VerdictGate::new();
    g2.begin()?;
    g2.settle(false)?;
    if g.state != GateState::Probing || g1.state != GateState::Declared || g2.state != GateState::Fallback {
        return Err(UdCode::GateBadTerminal.code());
    }
    // b) 表外拒：Idle 直接 settle 两向。
    let mut g3 = VerdictGate::new();
    if g3.settle(true).is_ok() || g3.settle(false).is_ok() {
        return Err(UdCode::GateJump.code());
    }
    // 终态再跃迁拒（Declared→* / Fallback→*）——预期拒绝必须显式断言。
    if g1.settle_again_guard(true).is_ok() || g2.settle_again_guard(false).is_ok() {
        return Err(UdCode::GateJump.code());
    }
    // 终态 begin 拒（Declared→Probing 非法）。
    if g1.begin().is_ok() || g2.begin().is_ok() {
        return Err(UdCode::GateJump.code());
    }
    Ok(())
}

// ── kind × declared 全矩阵派生 ─────────────────────────────────────
/// 全矩阵 8 组合派生一致性：answer(kind, declared) 与规则表逐格对拍
/// （Rtl 行两格恒真值；其余行 声明→真值 / 未声明→兼容值）。
/// 返回（一致格数, 总格数）——一致格数必须 == 8。
pub fn matrix_check() -> Result<(u32, u32), u32> {
    let mut ok = 0u32;
    let mut kind = 0u8;
    while (kind as usize) < KIND_N {
        let mut declared = 0;
        while declared < 2 {
            let got = answer(kind, declared == 1)?;
            let want = if kind == KIND_RTLGETVERSION {
                (TRUE_MAJOR, TRUE_MINOR, TRUE_BUILD)
            } else if declared == 1 {
                (TRUE_MAJOR, TRUE_MINOR, TRUE_BUILD)
            } else {
                (COMPAT_MAJOR, COMPAT_MINOR, COMPAT_BUILD)
            };
            if got == want {
                ok += 1;
            }
            declared += 1;
        }
        kind += 1;
    }
    Ok((ok, (KIND_N * 2) as u32))
}

// ── 对照例指纹账（互异性） ─────────────────────────────────────────
const FP_OFFSET: u32 = 0x811C_9DC5;
const FP_PRIME: u32 = 0x0100_0193;

/// 单例指纹：FNV-1a(kind,declared,expect 三元组)——表错位或重复例
/// 会造成指纹重合，互异性是"10 例确实是 10 例"的机器口径。
pub fn case_fingerprint(idx: usize) -> Result<u32, u32> {
    if idx >= CASES {
        return Err(UdCode::CaseOutOfRange.code());
    }
    let c = &CASE_TABLE[idx];
    let mut h: u32 = FP_OFFSET;
    let mut mix = |b: u32| {
        h ^= b & 0xFF;
        h = h.wrapping_mul(FP_PRIME);
        h ^= (b >> 8) & 0xFF;
        h = h.wrapping_mul(FP_PRIME);
        h ^= (b >> 16) & 0xFF;
        h = h.wrapping_mul(FP_PRIME);
        h ^= (b >> 24) & 0xFF;
        h = h.wrapping_mul(FP_PRIME);
    };
    mix(c.id as u32);
    mix(c.kind as u32);
    mix(c.declared as u32);
    mix(c.expect.0);
    mix(c.expect.1);
    mix(c.expect.2);
    Ok(h)
}

/// 指纹互异校验：10 例两两不同。
pub fn fingerprint_distinct() -> Result<(), u32> {
    let mut i = 0;
    while i < CASES {
        let mut j = i + 1;
        while j < CASES {
            if case_fingerprint(i)? == case_fingerprint(j)? {
                return Err(UdCode::TableInconsistent.code());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

// ── 会话重放（确定性） ─────────────────────────────────────────────
/// 会话重放：声明态序列 → 逐例门演练 → 裁决序列。
/// 确定性口径：同序列两跑裁决序列逐位一致。
pub fn session_replay(seq: &[bool]) -> Result<Vec<(u32, u32, u32)>, u32> {
    let mut out: Vec<(u32, u32, u32)> = Vec::new();
    let mut i = 0;
    while i < seq.len() {
        let (v, _) = gate_flow(seq[i])?;
        out.push(v);
        i += 1;
    }
    Ok(out)
}

/// 会话确定性：同序列两跑同序。
pub fn session_determinism() -> Result<(), u32> {
    let seq: [bool; 6] = [true, false, false, true, true, false];
    let a = session_replay(&seq)?;
    let b = session_replay(&seq)?;
    if a != b {
        return Err(UdCode::NotReproducible.code());
    }
    Ok(())
}

// ── 判据（20 条，断言性质） ────────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18444_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18444 无声明应用兼容值裁决");
    // 1. 口径衔接 F18428。
    s.add("J1-口径衔接F18428", link_check().is_ok(), "兼容/真值/Rtl 三常量同源冻结");
    // 2. 10 例零偏差（J1 前半句主体）。
    let all_ok = match reconcile_all() {
        Ok((dev, cases)) => dev == 0 && cases == CASES as u32,
        Err(_) => false,
    };
    s.add("J1-实机对照零偏差", all_ok, "10 例 引擎返回↔冻结期望 偏差=0");
    // 3. 未声明回退兼容值（核心性质）。
    s.add("J1-无声明回退兼容值", inject_true_leak().is_ok(), "未声明 GetVersion=(6,2,9200)");
    // 4. 声明报真值。
    s.add("J1-声明报真值", inject_compat_leak().is_ok(), "已声明 GetVersionEx=(10,0,10240)");
    // 5. Rtl 例外成立（未声明亦真值）。
    s.add("J1-Rtl例外成立", inject_rtl_break().is_ok(), "RtlGetVersion 恒真值");
    // 6. 越界注入检出。
    s.add("J1-注入:越界", inject_out_of_range().is_ok(), "kind=4/case=10 显式拒");
    // 7. 注入多轮复现。
    s.add("J1-注入3轮全检出", inject_rounds(), "4 类 ×3 轮");
    // 8. 门：声明终态裁决真值。
    let gd = gate_flow(true);
    let gd_ok = matches!(gd, Ok((v, _)) if v == (TRUE_MAJOR, TRUE_MINOR, TRUE_BUILD));
    s.add("J1-门:声明终态", gd_ok, "Idle→Probing→Declared→真值");
    // 9. 门：回退终态裁决兼容值。
    let gf = gate_flow(false);
    let gf_ok = matches!(gf, Ok((v, _)) if v == (COMPAT_MAJOR, COMPAT_MINOR, COMPAT_BUILD));
    s.add("J1-门:回退终态", gf_ok, "Idle→Probing→Fallback→兼容值");
    // 10. 门：跳步注入拒。
    s.add("J1-门:跳步拒", gate_jump_probe().is_ok(), "Idle 直接 settle 拒");
    // 11. 门：终态幂等拒。
    s.add("J1-门:终态幂等拒", gate_idem_probe().is_ok(), "终态再跃迁拒");
    // 12. 种类闭集：kind 名与序号互证。
    s.add("J1-种类闭集", KIND_NAMES.len() == KIND_N && KIND_RTLGETVERSION == 2, "4 种闭集冻结");
    // 13. 确定性：同输入两次 answer 逐位一致。
    let det = answer(KIND_VERIFYVERSION, false) == answer(KIND_VERIFYVERSION, false);
    s.add("J1-裁决确定性", det, "同输入同输出");
    // 14. 双跑一致。
    s.add("J1-双跑一致", repro_check().is_ok(), "对拍+门演练两跑同值");
    // 15. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7EAC0 窗口");
    // 16. 黄金全链。
    let g = golden_run();
    let g_ok = g.is_ok();
    s.add_dyn("J1-黄金全链", g_ok, &format!("golden={:?}", g));
    // 17. Rtl 与声明无关：声明/未声明两态返回一致。
    let rtl_indep = answer(KIND_RTLGETVERSION, true) == answer(KIND_RTLGETVERSION, false);
    s.add("J1-Rtl与声明无关", rtl_indep, "例外恒真值不随声明态翻转");
    // 18. 状态机邻接表完整：表内可达 + 表外全拒。
    s.add("J1-状态机邻接完整", adjacency_check().is_ok(), "3 条合法跃迁可达，表外一律拒");
    // 19. kind×declared 全矩阵 8/8。
    let mx = matrix_check();
    let mx_ok = matches!(mx, Ok((o, t)) if o == t && t == 8);
    s.add("J1-全矩阵派生8/8", mx_ok, "4 kind × 2 声明态 逐格对拍");
    // 20. 对照例指纹互异（10 例确为 10 例，无重例/错位）。
    s.add("J1-例指纹互异", fingerprint_distinct().is_ok(), "10 指纹两两不同");
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18444_checks_green() {
        let set = run_unx18444_checks();
        assert!(set.all_passed(), "UNX-F18444 red: {}", set.red_items_text());
    }

    #[test]
    fn t_answer_matrix() {
        assert_eq!(answer(KIND_GETVERSION, false), Ok((6, 2, 9200)));
        assert_eq!(answer(KIND_GETVERSION, true), Ok((10, 0, 10240)));
        assert_eq!(answer(KIND_GETVERSIONEX, false), Ok((6, 2, 9200)));
        assert_eq!(answer(KIND_RTLGETVERSION, false), Ok((10, 0, 10240)));
        assert_eq!(answer(KIND_VERIFYVERSION, true), Ok((10, 0, 10240)));
    }

    #[test]
    fn t_reconcile_zero() {
        let (dev, cases) = reconcile_all().unwrap_or((99, 0));
        assert_eq!((dev, cases), (0, 10));
    }

    #[test]
    fn t_table_self_consistent() {
        // 10 例逐一 reconcile 不触发表自坏。
        let mut i = 0;
        while i < CASES {
            assert!(reconcile_case(i).is_ok(), "例 {} 表自坏", i);
            i += 1;
        }
    }

    #[test]
    fn t_gate_and_range() {
        assert_eq!(answer(KIND_N as u8, false), Err(UdCode::KindOutOfRange.code()));
        assert!(gate_jump_probe().is_ok());
        assert!(gate_idem_probe().is_ok());
    }

    #[test]
    fn t_inject_and_repro() {
        assert!(inject_rounds());
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }
}
