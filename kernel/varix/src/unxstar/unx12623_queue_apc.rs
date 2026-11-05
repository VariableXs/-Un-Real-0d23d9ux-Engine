//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12623 NtQueueApcThread→D4 联签 · 0x805C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12624-J1 KernelMode APC 禁越消费断言绿
//!      （PreviousMode 联动回指 B28），越权排队注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - PreviousMode 闸（回指 B28）：排队者的前模式必须取自 B28 访问
//!     模式账（冻结登记），账外调用者拒收（ModeAccountMissing）——
//!     禁止自报模式（自报即伪装面）；
//!   - 禁越闭式：UserMode 调用者排 KernelMode APC 一律拒
//!     （ModeCrossDenied）；KernelMode 排 KernelMode/UserMode APC
//!     放行；UserMode 排 UserMode APC 放行——好路零误伤；
//!   - 越权排队注入 10/10：UserMode×KernelAPC 十次轮换（换目标/
//!     换例程）每次必须被 ModeCrossDenied 拒且码落段闭集；
//!   - Nt→D4 联签：每笔放行的用户 APC 排队落一笔联签（Nt 层签出
//!     +D4 泵回签），联签数 ≡ 用户 APC 排队数（CosignParityBreak
//!     抓不等）；联签状态机 Unsigned→Signed→Cosigned 单调；
//!   - 双跑指纹一致；LCG 种子写死 = SEG_BASE（0x805C0）；
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect；
//!     诊断码闭集 0x805C0..0x80600。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x805C0，AI-VX9-1105 B01 批 12 槽第 12 槽）。
pub const SEG_BASE: u32 = 0x805C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModeGateCode {
    /// 模式越权：UserMode 排 KernelMode APC
    ModeCrossDenied,
    /// 模式账缺项：调用者未在 B28 模式账登记
    ModeAccountMissing,
    /// 自报模式：模式来源非账面（伪装面）
    ModeSelfClaimed,
    /// 联签守恒破：联签数≠用户 APC 排队数
    CosignParityBreak,
    /// 联签重复回签
    CosignDupSign,
    /// 联签状态倒退
    CosignRegress,
    /// 坏线程句柄：目标不在注册窗
    BadTargetHandle,
    /// 空例程：例程指针为 0
    EmptyRoutine,
    /// 排队账溢出
    QueueOverflow,
    /// 好路误伤：合法组合被拒
    GoodPathHit,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl ModeGateCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ModeGateCode::ModeCrossDenied => 0x01,
                ModeGateCode::ModeAccountMissing => 0x02,
                ModeGateCode::ModeSelfClaimed => 0x03,
                ModeGateCode::CosignParityBreak => 0x04,
                ModeGateCode::CosignDupSign => 0x05,
                ModeGateCode::CosignRegress => 0x06,
                ModeGateCode::BadTargetHandle => 0x07,
                ModeGateCode::EmptyRoutine => 0x08,
                ModeGateCode::QueueOverflow => 0x09,
                ModeGateCode::GoodPathHit => 0x0A,
                ModeGateCode::FingerprintDrift => 0x0B,
                ModeGateCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            ModeGateCode::ModeCrossDenied => "禁越：用户态不许排内核态 APC，走系统调用提升",
            ModeGateCode::ModeAccountMissing => "模式账缺项：调用者先在 B28 账登记模式",
            ModeGateCode::ModeSelfClaimed => "自报模式：PreviousMode 只认账面，不认参数",
            ModeGateCode::CosignParityBreak => "联签守恒破：联签=用户APC排队，逐笔对账",
            ModeGateCode::CosignDupSign => "重复回签：一笔联签只许一次回签",
            ModeGateCode::CosignRegress => "联签倒退：Unsigned→Signed→Cosigned 只进不退",
            ModeGateCode::BadTargetHandle => "目标句柄越窗：用注册窗内线程句柄",
            ModeGateCode::EmptyRoutine => "例程为空：先注册例程再排队",
            ModeGateCode::QueueOverflow => "排队账溢出：先消费积压再排",
            ModeGateCode::GoodPathHit => "好路误伤：合法组合必须放行",
            ModeGateCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            ModeGateCode::Unknown => "未知诊断码：不在 0x805C0 段闭集内",
        }
    }

    pub const ALL: [ModeGateCode; 12] = [
        ModeGateCode::ModeCrossDenied,
        ModeGateCode::ModeAccountMissing,
        ModeGateCode::ModeSelfClaimed,
        ModeGateCode::CosignParityBreak,
        ModeGateCode::CosignDupSign,
        ModeGateCode::CosignRegress,
        ModeGateCode::BadTargetHandle,
        ModeGateCode::EmptyRoutine,
        ModeGateCode::QueueOverflow,
        ModeGateCode::GoodPathHit,
        ModeGateCode::FingerprintDrift,
        ModeGateCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in ModeGateCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    ModeGateCode::Unknown.detail()
}

// ── 冻结常量表 ───────────────────────────────────────────────────
/// 目标线程句柄注册窗（下闭上开）。
pub const TARGET_WIN: (u32, u32) = (0xC00, 0xD00);
/// 注册线程数。
pub const REGISTERED: usize = 3;
/// 排队账容量。
pub const QUEUE_CAP: usize = 16;
/// B28 模式账登记的调用者号窗（下闭上开）。
pub const CALLER_WIN: (u32, u32) = (0x10, 0x20);

/// 调用者前模式（B28 联动口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrevMode {
    /// 用户态。
    UserMode,
    /// 内核态。
    KernelMode,
}

/// APC 模式闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApcMode {
    /// 用户 APC。
    UserApc,
    /// 内核 APC（禁越面）。
    KernelApc,
}

/// 一条排队请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueReq {
    /// 调用者号（B28 模式账键）。
    pub caller: u32,
    /// APC 模式。
    pub apc_mode: ApcMode,
    /// 目标线程句柄。
    pub target: u32,
    /// 例程指针。
    pub routine: u32,
}

// ── B28 模式账（回指冻结登记） ──────────────────────────────────
/// 模式账：调用者号 → 前模式（只读账，登记后不变）。
pub struct ModeAccount {
    entries: Vec<(u32, PrevMode)>,
}

impl ModeAccount {
    /// 预置账：0x10/0x11 为用户态，0x12/0x13 为内核态（冻结）。
    pub fn frozen() -> ModeAccount {
        let mut m = ModeAccount { entries: Vec::new() };
        m.entries.push((0x10, PrevMode::UserMode));
        m.entries.push((0x11, PrevMode::UserMode));
        m.entries.push((0x12, PrevMode::KernelMode));
        m.entries.push((0x13, PrevMode::KernelMode));
        m
    }

    /// 查前模式（账外 None）。
    pub fn mode_of(&self, caller: u32) -> Option<PrevMode> {
        for e in self.entries.iter() {
            if e.0 == caller {
                return Some(e.1);
            }
        }
        None
    }

    /// 账项数（点验用）。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

// ── 联签账 ───────────────────────────────────────────────────────
/// 联签状态：Unsigned(0)→Signed(1)→Cosigned(2)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignPhase {
    Unsigned,
    Signed,
    Cosigned,
}

impl SignPhase {
    pub const fn rank(self) -> u32 {
        match self {
            SignPhase::Unsigned => 0,
            SignPhase::Signed => 1,
            SignPhase::Cosigned => 2,
        }
    }
}

/// 一笔 Nt→D4 联签。
#[derive(Clone, Copy, Debug)]
pub struct CosignRow {
    pub seq: u32,
    pub target: u32,
    pub state: SignPhase,
}

// ── 排队闸（含禁越闸） ───────────────────────────────────────────
/// NtQueueApcThread 闸：模式账 + 禁越 + 参数校验 + 联签落账。
pub struct QueueGate {
    pub account: ModeAccount,
    pub rows: Vec<CosignRow>,
    /// 用户 APC 放行数（联签基准）。
    pub user_apc_ok: u32,
    /// 内核 APC 放行数。
    pub kernel_apc_ok: u32,
    /// 拒止数。
    pub rejected: u32,
    /// 总尝试数。
    pub attempts: u32,
    /// 自报模式被拒数。
    pub self_claimed: u32,
}

impl QueueGate {
    pub fn new() -> QueueGate {
        QueueGate {
            account: ModeAccount::frozen(),
            rows: Vec::new(),
            user_apc_ok: 0,
            kernel_apc_ok: 0,
            rejected: 0,
            attempts: 0,
            self_claimed: 0,
        }
    }

    /// 排队判定：先模式账（回指 B28），再禁越闭式，再参数校验。
    /// claimed 参数是调用者**自报**的模式——只用于与账面对比识别伪装，
    /// 闸的裁定永远以账面为准。
    pub fn submit(&mut self, req: &QueueReq, claimed: PrevMode) -> Result<(), u32> {
        self.attempts += 1;
        // 参 0：模式账（回指 B28）。账外拒；自报≠账面即伪装面拒。
        let real = match self.account.mode_of(req.caller) {
            None => {
                self.rejected += 1;
                return Err(ModeGateCode::ModeAccountMissing.val());
            }
            Some(m) => m,
        };
        if real != claimed {
            self.self_claimed += 1;
            self.rejected += 1;
            return Err(ModeGateCode::ModeSelfClaimed.val());
        }
        // 参 1：禁越闭式（PreviousMode 联动核心）。
        if real == PrevMode::UserMode && req.apc_mode == ApcMode::KernelApc {
            self.rejected += 1;
            return Err(ModeGateCode::ModeCrossDenied.val());
        }
        // 参 2：目标线程句柄。
        if req.target < TARGET_WIN.0 || req.target >= TARGET_WIN.0 + REGISTERED as u32 {
            self.rejected += 1;
            return Err(ModeGateCode::BadTargetHandle.val());
        }
        // 参 3：例程非空。
        if req.routine == 0 {
            self.rejected += 1;
            return Err(ModeGateCode::EmptyRoutine.val());
        }
        // 容量。
        if self.rows.len() >= QUEUE_CAP {
            self.rejected += 1;
            return Err(ModeGateCode::QueueOverflow.val());
        }
        // 放行：落联签（Nt 签出）。
        let seq = (self.user_apc_ok.wrapping_add(self.kernel_apc_ok)).wrapping_add(1);
        self.rows.push(CosignRow { seq, target: req.target, state: SignPhase::Signed });
        match req.apc_mode {
            ApcMode::UserApc => self.user_apc_ok += 1,
            ApcMode::KernelApc => self.kernel_apc_ok += 1,
        }
        Ok(())
    }

    /// D4 泵回签：Signed → Cosigned（一次）。
    pub fn cosign(&mut self, seq: u32) -> Result<(), u32> {
        let mut idx: Option<usize> = None;
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.seq == seq {
                    idx = Some(i);
                }
            }
            i += 1;
        }
        let i = match idx {
            None => return Err(ModeGateCode::CosignRegress.val()),
            Some(x) => x,
        };
        let st = match self.rows.get(i) {
            Some(r) => r.state,
            None => return Err(ModeGateCode::CosignRegress.val()),
        };
        match st {
            SignPhase::Unsigned => Err(ModeGateCode::CosignRegress.val()),
            SignPhase::Cosigned => Err(ModeGateCode::CosignDupSign.val()),
            SignPhase::Signed => {
                if let Some(r) = self.rows.get_mut(i) {
                    r.state = SignPhase::Cosigned;
                }
                Ok(())
            }
        }
    }

    /// 联签守恒：联签行数 == 用户 APC 放行数 + 内核 APC 放行数。
    pub fn cosign_parity(&self) -> Result<(), u32> {
        if self.rows.len() as u32
            != self.user_apc_ok.wrapping_add(self.kernel_apc_ok)
        {
            return Err(ModeGateCode::CosignParityBreak.val());
        }
        Ok(())
    }

    /// 参数账守恒：放行+拒止=尝试。
    pub fn param_parity(&self) -> Result<(), u32> {
        if self.user_apc_ok.wrapping_add(self.kernel_apc_ok).wrapping_add(self.rejected)
            != self.attempts
        {
            return Err(ModeGateCode::CosignParityBreak.val());
        }
        Ok(())
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u32 {
        self.user_apc_ok
            .wrapping_mul(17)
            .wrapping_add(self.kernel_apc_ok.wrapping_mul(11))
            .wrapping_add(self.rejected.wrapping_mul(5))
            .wrapping_add(self.self_claimed)
            .wrapping_add(self.rows.len() as u32)
    }
}

// ── 黄金判定闭式 ─────────────────────────────────────────────────
/// 黄金闭式：给定（账面模式, APC 模式）→ 期望（放行?）。
pub fn golden_allow(real: PrevMode, apc: ApcMode) -> bool {
    match (real, apc) {
        (PrevMode::UserMode, ApcMode::KernelApc) => false,
        _ => true,
    }
}

pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 抽样 10 例：LCG 抽（账面模式×APC 模式×目标）与黄金闭式对平。
pub fn sample_ten() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut g = QueueGate::new();
    let mut agree = 0u32;
    let mut i = 0;
    while i < 10 {
        let caller = match rng.below(4) {
            0 => 0x10,
            1 => 0x11,
            2 => 0x12,
            _ => 0x13,
        };
        let real = g.account.mode_of(caller).unwrap_or(PrevMode::UserMode);
        let apc = if rng.below(2) == 0 { ApcMode::UserApc } else { ApcMode::KernelApc };
        let target = TARGET_WIN.0 + rng.below(REGISTERED as u64) as u32;
        let req = QueueReq { caller, apc_mode: apc, target, routine: 0x5000 + i as u32 };
        let want = golden_allow(real, apc);
        let got = g.submit(&req, real).is_ok();
        if got == want {
            agree += 1;
        }
        i += 1;
    }
    g.cosign_parity()?;
    if agree == 10 {
        Ok(10)
    } else {
        Err(ModeGateCode::GoodPathHit.val())
    }
}

// ── 注入面：越权排队 10/10 必抓 ──────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0 }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 越权排队注入 10 次：UserMode×KernelAPC 轮换目标与例程，
/// 每次必须被 ModeCrossDenied 拒且码落段闭集。
pub fn inject_cross_mode(led: &mut InjectLedger) {
    let mut g = QueueGate::new();
    let mut i = 0;
    while i < 10 {
        led.tries += 1;
        // 账面用户态调用者（0x10/0x11 轮换）。
        let caller = if i % 2 == 0 { 0x10 } else { 0x11 };
        let req = QueueReq {
            caller,
            apc_mode: ApcMode::KernelApc,
            target: TARGET_WIN.0 + (i % REGISTERED) as u32,
            routine: 0x6000 + i as u32,
        };
        let res = g.submit(&req, PrevMode::UserMode);
        let hit = res == Err(ModeGateCode::ModeCrossDenied.val());
        if hit {
            led.caught += 1;
        }
        i += 1;
    }
}

/// 伪装注入：自报内核态的账面用户态调用者 → ModeSelfClaimed。
pub fn inject_self_claim(led: &mut InjectLedger) {
    led.tries += 1;
    let mut g = QueueGate::new();
    let req = QueueReq {
        caller: 0x10,
        apc_mode: ApcMode::UserApc,
        target: TARGET_WIN.0,
        routine: 0x6100,
    };
    if g.submit(&req, PrevMode::KernelMode) == Err(ModeGateCode::ModeSelfClaimed.val()) {
        led.caught += 1;
    }
    led.tries += 1;
    let mut g2 = QueueGate::new();
    let req2 = QueueReq {
        caller: 0x99,
        apc_mode: ApcMode::UserApc,
        target: TARGET_WIN.0,
        routine: 0x6200,
    };
    if g2.submit(&req2, PrevMode::UserMode) == Err(ModeGateCode::ModeAccountMissing.val()) {
        led.caught += 1;
    }
}

// ── 端到端正演 + 双跑一致 ────────────────────────────────────────
/// 正演：用户排用户 APC ×2 → 内核排内核 APC ×1 → 全部回签 →
/// 越权注入全拒 → 守恒复核。
pub fn golden_run() -> Result<u32, u32> {
    let mut g = QueueGate::new();
    // 用户态排用户 APC 两条（0xC00/0xC01）。
    g.submit(
        &QueueReq { caller: 0x10, apc_mode: ApcMode::UserApc, target: TARGET_WIN.0, routine: 0x7000 },
        PrevMode::UserMode,
    )?;
    g.submit(
        &QueueReq { caller: 0x11, apc_mode: ApcMode::UserApc, target: TARGET_WIN.0 + 1, routine: 0x7001 },
        PrevMode::UserMode,
    )?;
    // 内核态排内核 APC 一条。
    g.submit(
        &QueueReq { caller: 0x12, apc_mode: ApcMode::KernelApc, target: TARGET_WIN.0 + 2, routine: 0x7002 },
        PrevMode::KernelMode,
    )?;
    // D4 泵全部回签。
    let mut s = 1;
    while s <= 3 {
        g.cosign(s)?;
        s += 1;
    }
    g.cosign_parity()?;
    g.param_parity()?;
    Ok(g.fingerprint())
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) if a == b => Ok(()),
        _ => Err(ModeGateCode::FingerprintDrift.val()),
    }
}

// ── 判据谓词（私有 xxx_ok） ──────────────────────────────────────
/// J1：禁越消费断言绿（闭式四象限 + 正演绿）。
fn cross_ban_ok() -> bool {
    let quadrant = !golden_allow(PrevMode::UserMode, ApcMode::KernelApc)
        && golden_allow(PrevMode::UserMode, ApcMode::UserApc)
        && golden_allow(PrevMode::KernelMode, ApcMode::KernelApc)
        && golden_allow(PrevMode::KernelMode, ApcMode::UserApc);
    quadrant && golden_run().is_ok()
}

/// J2：越权排队注入 10/10 拒止（含伪装两例）。
fn inject_ok() -> bool {
    let mut led = InjectLedger::new();
    inject_cross_mode(&mut led);
    inject_self_claim(&mut led);
    led.all_caught()
}

/// J3：双跑一致。
fn repro_ok() -> bool {
    repro_check().is_ok()
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in ModeGateCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    v.push(("J1-禁越断言四象限", cross_ban_ok(), String::from("仅(User,Kernel)拒")));
    v.push((
        "J1-PreviousMode账回指",
        {
            let acc = ModeAccount::frozen();
            acc.mode_of(0x12) == Some(PrevMode::KernelMode)
                && acc.mode_of(0x10) == Some(PrevMode::UserMode)
                && acc.mode_of(0x99).is_none()
                && acc.len() == 4
        },
        String::from("B28 账面冻结"),
    ));
    // 注入。
    let mut led = InjectLedger::new();
    inject_cross_mode(&mut led);
    v.push((
        "J2-越权注入10/10",
        led.tries == 10 && led.caught == 10,
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    let mut led2 = InjectLedger::new();
    inject_self_claim(&mut led2);
    v.push((
        "J2-伪装/缺账注入",
        led2.all_caught(),
        format!("t/c {:?}/{:?}", led2.tries, led2.caught),
    ));
    // 抽样。
    v.push(("J3-抽样10例对平", sample_ten() == Ok(10), format!("{:?}", sample_ten())));
    // 边界：好路零误伤（内核排用户 APC 必须放行）。
    let mut gg = QueueGate::new();
    let good = gg
        .submit(
            &QueueReq { caller: 0x13, apc_mode: ApcMode::UserApc, target: TARGET_WIN.0, routine: 0x7100 },
            PrevMode::KernelMode,
        )
        .is_ok();
    v.push(("J3-好路零误伤", good, String::from("(Kernel,User) 放行")));
    // 边界：空例程/坏目标。
    let mut g3 = QueueGate::new();
    let empty = g3
        .submit(
            &QueueReq { caller: 0x10, apc_mode: ApcMode::UserApc, target: TARGET_WIN.0, routine: 0 },
            PrevMode::UserMode,
        )
        == Err(ModeGateCode::EmptyRoutine.val());
    let bad_target = g3
        .submit(
            &QueueReq { caller: 0x10, apc_mode: ApcMode::UserApc, target: 0xFFFF, routine: 0x7200 },
            PrevMode::UserMode,
        )
        == Err(ModeGateCode::BadTargetHandle.val());
    v.push(("J4-参数边界", empty && bad_target, String::from("例程/目标两界")));
    // 边界：重复回签。
    let mut g4 = QueueGate::new();
    let _ = g4.submit(
        &QueueReq { caller: 0x10, apc_mode: ApcMode::UserApc, target: TARGET_WIN.0, routine: 0x7300 },
        PrevMode::UserMode,
    );
    let dup = g4.cosign(1).is_ok() && g4.cosign(1) == Err(ModeGateCode::CosignDupSign.val());
    v.push(("J4-重复回签拒", dup, String::from("一笔一签")));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x805C0 窗口")));
    v
}

/// VX9 检查入口（卡 12623，UNX-F12624-J1 判据承载）。
pub fn run_unx12623_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12623 NtQueueApcThread→D4 联签");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_cross_ban_quadrant() {
        assert!(cross_ban_ok());
    }

    #[test]
    fn test_cross_mode_injection_ten() {
        let mut led = InjectLedger::new();
        inject_cross_mode(&mut led);
        assert_eq!(led.tries, 10);
        assert_eq!(led.caught, 10);
    }

    #[test]
    fn test_self_claim_and_missing_caught() {
        let mut led = InjectLedger::new();
        inject_self_claim(&mut led);
        assert_eq!(led.tries, 2);
        assert_eq!(led.caught, 2);
    }

    #[test]
    fn test_kernel_can_queue_both() {
        let mut g = QueueGate::new();
        assert!(g
            .submit(
                &QueueReq { caller: 0x12, apc_mode: ApcMode::KernelApc, target: TARGET_WIN.0, routine: 0x7400 },
                PrevMode::KernelMode,
            )
            .is_ok());
        assert!(g
            .submit(
                &QueueReq { caller: 0x12, apc_mode: ApcMode::UserApc, target: TARGET_WIN.0 + 1, routine: 0x7401 },
                PrevMode::KernelMode,
            )
            .is_ok());
        assert!(g.cosign_parity().is_ok());
    }

    #[test]
    fn test_user_cannot_queue_kernel() {
        let mut g = QueueGate::new();
        assert_eq!(
            g.submit(
                &QueueReq { caller: 0x10, apc_mode: ApcMode::KernelApc, target: TARGET_WIN.0, routine: 0x7500 },
                PrevMode::UserMode,
            ),
            Err(ModeGateCode::ModeCrossDenied.val())
        );
    }

    #[test]
    fn test_cosign_dup_rejected() {
        let mut g = QueueGate::new();
        let _ = g.submit(
            &QueueReq { caller: 0x11, apc_mode: ApcMode::UserApc, target: TARGET_WIN.0, routine: 0x7600 },
            PrevMode::UserMode,
        );
        assert!(g.cosign(1).is_ok());
        assert_eq!(g.cosign(1), Err(ModeGateCode::CosignDupSign.val()));
        assert_eq!(g.cosign(42), Err(ModeGateCode::CosignRegress.val()));
    }

    #[test]
    fn test_param_parity_holds() {
        let mut g = QueueGate::new();
        let _ = g.submit(
            &QueueReq { caller: 0x10, apc_mode: ApcMode::KernelApc, target: TARGET_WIN.0, routine: 0x7700 },
            PrevMode::UserMode,
        );
        let _ = g.submit(
            &QueueReq { caller: 0x10, apc_mode: ApcMode::UserApc, target: TARGET_WIN.0, routine: 0x7701 },
            PrevMode::UserMode,
        );
        assert!(g.param_parity().is_ok());
        assert_eq!(g.rejected, 1);
        assert_eq!(g.attempts, 2);
    }

    #[test]
    fn test_sample_ten_agree() {
        assert_eq!(sample_ten(), Ok(10));
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_check().is_ok());
        let a = golden_run().unwrap_or(0);
        let b = golden_run().unwrap_or(1);
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x805C0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in ModeGateCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}
