//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12615 D4 未开工联签预演形态声明 · 0x803C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12616-J1 30 例联测全过，拒止码双侧一致断言绿，
//!      零静默；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 预演形态声明：开工前先把「声明件」立账——声明的检验件闭集
//!     （五族参数校验器）+ 声明的双闸结构（客户侧闸/服务侧闸各自
//!     独立实现同一语义），声明落账后不许改（改即 DeclaredDrift）；
//!   - 30 例联测：五族 × 6 例 = 30 例冻结用例，每例同时过双侧闸，
//!     两侧拒止码必须**逐例一致**（双侧一致断言），一侧过一侧拒
//!     即双侧分歧（BilateralSplit 必抓）；
//!   - 零静默：凡违约参数（坏句柄/越权/空例程/坏序号/越窗）必须
//!     双侧都拒，任何一侧放行即静默漏（SilentLeak），静默漏计数
//!     必须为 0 才算联测全过；
//!   - 声明状态机：Declared→Rehearsed→Filed 单调，倒退显性拒；
//!   - 双跑一致：30 例联测重跑指纹逐位相等；LCG 种子写死 =
//!     SEG_BASE（0x803C0）；禁 rand/浮点/IO/墙钟；no_std + alloc；
//!     零 unwrap/expect；诊断码闭集 0x803C0..0x80400。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x803C0，AI-VX9-1105 B01 批 12 槽第 4 槽）。
pub const SEG_BASE: u32 = 0x803C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreformCode {
    /// 坏句柄：句柄不在登记窗
    BadHandle,
    /// 越权：请求权限超出授权掩码
    OverRights,
    /// 空例程：例程指针为 0 不许排队
    EmptyRoutine,
    /// 坏序号：联签序号为 0 或回卷
    BadSeq,
    /// 越窗：参数值超出冻结窗
    OutOfWindow,
    /// 双侧分歧：两闸对同参给出不同码
    BilateralSplit,
    /// 静默漏：违约参数被任一侧放行
    SilentLeak,
    /// 声明漂移：声明件清单被改
    DeclaredDrift,
    /// 声明状态倒退
    StateRegress,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 用例账守恒破：过例数+拒例数≠总例数
    ConservationBreak,
    /// 未知诊断码兜底
    Unknown,
}

impl PreformCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PreformCode::BadHandle => 0x01,
                PreformCode::OverRights => 0x02,
                PreformCode::EmptyRoutine => 0x03,
                PreformCode::BadSeq => 0x04,
                PreformCode::OutOfWindow => 0x05,
                PreformCode::BilateralSplit => 0x06,
                PreformCode::SilentLeak => 0x07,
                PreformCode::DeclaredDrift => 0x08,
                PreformCode::StateRegress => 0x09,
                PreformCode::FingerprintDrift => 0x0A,
                PreformCode::ConservationBreak => 0x0B,
                PreformCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            PreformCode::BadHandle => "句柄不在窗：用登记账里真实存在的句柄值",
            PreformCode::OverRights => "越权：申请位必须是授权掩码子集",
            PreformCode::EmptyRoutine => "例程为空：先注册例程再排队",
            PreformCode::BadSeq => "序号非法：从 1 起单调递增，禁 0 禁回卷",
            PreformCode::OutOfWindow => "参数越窗：落在冻结窗内再试",
            PreformCode::BilateralSplit => "双侧分歧：两闸语义必须同源，逐例对码",
            PreformCode::SilentLeak => "静默漏：违约必拒，放行即漏，零容忍",
            PreformCode::DeclaredDrift => "声明件被改：声明落账后冻结，改版另立新账",
            PreformCode::StateRegress => "声明状态倒退：Declared→Rehearsed→Filed 只进不退",
            PreformCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            PreformCode::ConservationBreak => "守恒破：过+拒=总，逐例补账",
            PreformCode::Unknown => "未知诊断码：不在 0x803C0 段闭集内",
        }
    }

    pub const ALL: [PreformCode; 12] = [
        PreformCode::BadHandle,
        PreformCode::OverRights,
        PreformCode::EmptyRoutine,
        PreformCode::BadSeq,
        PreformCode::OutOfWindow,
        PreformCode::BilateralSplit,
        PreformCode::SilentLeak,
        PreformCode::DeclaredDrift,
        PreformCode::StateRegress,
        PreformCode::FingerprintDrift,
        PreformCode::ConservationBreak,
        PreformCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in PreformCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    PreformCode::Unknown.detail()
}

// ── 冻结常量表（来源：D4 未开工联签预演声明册） ──────────────────
/// 联测总例数（五族 × 6 = 30，冻结）。
pub const CASE_TOTAL: u32 = 30;
/// 每族例数。
pub const CASES_PER_KIND: u32 = 6;
/// 句柄登记窗（下闭上开）。
pub const HANDLE_WIN: (u32, u32) = (0x0100, 0x0200);
/// 序号上界（1..=1023 单调）。
pub const SEQ_MAX: u32 = 1023;
/// 授权掩码冻结基线。
pub const GRANT_MASK: u32 = 0x07;

// ── 用例参数闭集 ─────────────────────────────────────────────────
/// 用例族闭集（五族，每族 6 例）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseKind {
    /// 族 0：句柄合法性。
    Handle,
    /// 族 1：权限收缩。
    Rights,
    /// 族 2：例程非空。
    Routine,
    /// 族 3：序号单调。
    Seq,
    /// 族 4：窗口越界。
    Window,
}

impl CaseKind {
    pub const ALL: [CaseKind; 5] = [
        CaseKind::Handle,
        CaseKind::Rights,
        CaseKind::Routine,
        CaseKind::Seq,
        CaseKind::Window,
    ];

    /// 族内第 i 例的参数（确定性冻结，不依赖运行期状态）。
    pub const fn param(self, i: u32) -> u32 {
        match self {
            // 句柄族：3 合法（0x0100/0x0180/0x01FF）+ 3 越窗。
            CaseKind::Handle => match i {
                0 => 0x0100,
                1 => 0x0180,
                2 => 0x01FF,
                3 => 0x00FF,
                4 => 0x0200,
                _ => 0x0301,
            },
            // 权限族：3 子集（0/1/7）+ 3 越权（0x8/0x10/0x20）。
            CaseKind::Rights => match i {
                0 => 0x0,
                1 => 0x1,
                2 => 0x7,
                3 => 0x8,
                4 => 0x10,
                _ => 0x20,
            },
            // 例程族：3 非空 + 3 空（0）。
            CaseKind::Routine => match i {
                0 => 0x4000,
                1 => 0x4004,
                2 => 0x4008,
                3 => 0x0,
                4 => 0x0,
                _ => 0x0,
            },
            // 序号族：3 合法（1/2/1023）+ 3 非法（0/1024/回卷大值）。
            CaseKind::Seq => match i {
                0 => 0x1,
                1 => 0x2,
                2 => 0x3FF,
                3 => 0x0,
                4 => 0x400,
                _ => 0xFFFF,
            },
            // 窗口族：3 窗内 + 3 窗外。
            CaseKind::Window => match i {
                0 => 0x10,
                1 => 0x20,
                2 => 0x30,
                3 => 0x1,
                4 => 0xF000,
                _ => 0xFFFF,
            },
        }
    }

    /// 该例是否为违约例（期望被拒）——与参数表同源冻结。
    pub const fn expects_reject(self, i: u32) -> bool {
        match self {
            CaseKind::Handle => i >= 3,
            CaseKind::Rights => i >= 3,
            CaseKind::Routine => i >= 3,
            CaseKind::Seq => i >= 3,
            CaseKind::Window => i >= 3,
        }
    }

    /// 该族违约例应得的拒止码（双侧一致基准）。
    pub const fn expect_code(self) -> u32 {
        match self {
            CaseKind::Handle => PreformCode::BadHandle.val(),
            CaseKind::Rights => PreformCode::OverRights.val(),
            CaseKind::Routine => PreformCode::EmptyRoutine.val(),
            CaseKind::Seq => PreformCode::BadSeq.val(),
            CaseKind::Window => PreformCode::OutOfWindow.val(),
        }
    }
}

// ── 双闸：客户侧/服务侧各自独立实现同一语义 ─────────────────────
/// 客户侧闸：按「先窗后族」顺序校验。
pub fn gate_client(kind: CaseKind, param: u32) -> Result<u32, u32> {
    // 第一道：全局窗口（句柄与序号共用窗逻辑）。
    match kind {
        CaseKind::Handle => {
            if param < HANDLE_WIN.0 || param >= HANDLE_WIN.1 {
                return Err(PreformCode::BadHandle.val());
            }
        }
        CaseKind::Seq => {
            if param == 0 || param > SEQ_MAX {
                return Err(PreformCode::BadSeq.val());
            }
        }
        CaseKind::Window => {
            if param < 0x10 || param > 0x30 {
                return Err(PreformCode::OutOfWindow.val());
            }
        }
        CaseKind::Rights | CaseKind::Routine => {
            // 窗道不涉这两族，直接落族道。
        }
    }
    // 第二道：族语义。
    match kind {
        CaseKind::Rights => {
            if param & !GRANT_MASK != 0 {
                return Err(PreformCode::OverRights.val());
            }
        }
        CaseKind::Routine => {
            if param == 0 {
                return Err(PreformCode::EmptyRoutine.val());
            }
        }
        _ => {}
    }
    Ok(param)
}

/// 服务侧闸：按「先族后窗」顺序校验（与客户侧不同序，语义同源）。
pub fn gate_server(kind: CaseKind, param: u32) -> Result<u32, u32> {
    // 第一道：族语义。
    match kind {
        CaseKind::Handle => {
            if param < HANDLE_WIN.0 || param >= HANDLE_WIN.1 {
                return Err(PreformCode::BadHandle.val());
            }
        }
        CaseKind::Rights => {
            if param & !GRANT_MASK != 0 {
                return Err(PreformCode::OverRights.val());
            }
        }
        CaseKind::Routine => {
            if param == 0 {
                return Err(PreformCode::EmptyRoutine.val());
            }
        }
        CaseKind::Seq => {
            if param == 0 || param > SEQ_MAX {
                return Err(PreformCode::BadSeq.val());
            }
        }
        CaseKind::Window => {
            if param < 0x10 || param > 0x30 {
                return Err(PreformCode::OutOfWindow.val());
            }
        }
    }
    // 第二道：复核窗（对已过族道的参数再走一遍窗闭式，防族道漏网）。
    if param == 0 && kind != CaseKind::Rights {
        return Err(PreformCode::OutOfWindow.val());
    }
    Ok(param)
}

// ── 30 例联测账 ──────────────────────────────────────────────────
/// 联测账：总例 / 双侧一致例 / 静默漏例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JointLedger {
    pub total: u32,
    pub agree: u32,
    pub silent_leak: u32,
    /// 双侧码分歧例数。
    pub split: u32,
}

impl JointLedger {
    pub const fn new() -> JointLedger {
        JointLedger { total: 0, agree: 0, silent_leak: 0, split: 0 }
    }

    /// 守恒：total = agree + split，且 total ≤ CASE_TOTAL。
    pub fn conservation(&self) -> Result<(), u32> {
        if self.total > CASE_TOTAL {
            return Err(PreformCode::ConservationBreak.val());
        }
        if self.agree.wrapping_add(self.split) != self.total {
            return Err(PreformCode::ConservationBreak.val());
        }
        Ok(())
    }
}

/// 跑一例：双侧同入，比对码。返回（是否一致, 是否静默漏）。
pub fn run_case(kind: CaseKind, i: u32) -> (bool, bool) {
    let p = kind.param(i);
    let a = gate_client(kind, p);
    let b = gate_server(kind, p);
    match (a, b) {
        (Ok(_), Ok(_)) => {
            // 双侧都放行：合法例一致；违约例放行 = 静默漏。
            (!kind.expects_reject(i), kind.expects_reject(i))
        }
        (Err(ca), Err(cb)) => {
            // 双侧都拒：码必须逐例一致；违约例一致即绿。
            (ca == cb, false)
        }
        _ => (false, false),
    }
}

/// 全 30 例联测：五族 × 6 例，逐例双侧一致 + 零静默。
pub fn joint_thirty() -> Result<JointLedger, u32> {
    let mut led = JointLedger::new();
    for k in CaseKind::ALL.iter() {
        let mut i = 0;
        while i < CASES_PER_KIND {
            let (agree, leak) = run_case(*k, i);
            led.total += 1;
            if leak {
                led.silent_leak += 1;
            }
            if agree {
                led.agree += 1;
            } else {
                led.split += 1;
            }
            i += 1;
        }
    }
    led.conservation()?;
    // 零静默 + 全过。
    if led.silent_leak != 0 {
        return Err(PreformCode::SilentLeak.val());
    }
    if led.agree != CASE_TOTAL {
        return Err(PreformCode::BilateralSplit.val());
    }
    Ok(led)
}

// ── 声明状态机 ───────────────────────────────────────────────────
/// 声明件状态：Declared(0)→Rehearsed(1)→Filed(2)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeclPhase {
    Declared,
    Rehearsed,
    Filed,
}

impl DeclPhase {
    pub const fn rank(self) -> u32 {
        match self {
            DeclPhase::Declared => 0,
            DeclPhase::Rehearsed => 1,
            DeclPhase::Filed => 2,
        }
    }
}

/// 预演形态声明账：状态 + 联测指纹存根。
pub struct PreformDecl {
    pub phase: DeclPhase,
    /// 声明的件数（五族校验器 + 双闸 = 7 件，冻结）。
    pub declared_items: u32,
    /// 联测通过后的指纹存根（Filed 前必须非 0）。
    pub joint_fp: u32,
}

impl PreformDecl {
    pub const fn new() -> PreformDecl {
        PreformDecl { phase: DeclPhase::Declared, declared_items: 7, joint_fp: 0 }
    }

    /// 推进状态（只进不退）。
    pub fn advance(&mut self, next: DeclPhase) -> Result<(), u32> {
        if next.rank() <= self.phase.rank() {
            return Err(PreformCode::StateRegress.val());
        }
        if next == DeclPhase::Filed && self.joint_fp == 0 {
            // 未带联测指纹不许落档。
            return Err(PreformCode::ConservationBreak.val());
        }
        self.phase = next;
        Ok(())
    }

    /// 预演：跑 30 例并把指纹存根写进声明。
    pub fn rehearse(&mut self) -> Result<u32, u32> {
        if self.phase != DeclPhase::Declared {
            return Err(PreformCode::StateRegress.val());
        }
        let led = joint_thirty()?;
        self.joint_fp = led
            .total
            .wrapping_mul(97)
            .wrapping_add(led.agree.wrapping_mul(31))
            .wrapping_add(led.silent_leak);
        self.phase = DeclPhase::Rehearsed;
        Ok(self.joint_fp)
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

/// LCG 抽 10 例（族×例号都从 LCG 抽）做抽样复跑，与冻结期望对平。
pub fn sample_ten() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut agree = 0u32;
    let mut i = 0;
    while i < 10 {
        let ki = rng.below(5) as usize;
        let ci = rng.below(CASES_PER_KIND as u64) as u32;
        let kind = match CaseKind::ALL.get(ki) {
            Some(k) => *k,
            None => return Err(PreformCode::OutOfWindow.val()),
        };
        let (agree_case, leak) = run_case(kind, ci);
        // 冻结期望：双侧一致 + 违约例必拒（无静默）。
        if agree_case && !leak {
            agree += 1;
        }
        i += 1;
    }
    if agree == 10 {
        Ok(10)
    } else {
        Err(PreformCode::BilateralSplit.val())
    }
}

// ── 注入面：违约必拒 + 声明违约必抓 ──────────────────────────────
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

/// 注入族 0：违约例双侧必拒（抽 5 族违约参数逐一打两闸）。
/// 注入族 1：声明倒退（Filed→Declared）必拒。
/// 注入族 2：空指纹落档必拒。
pub fn inject_violations(led: &mut InjectLedger) {
    // 族 0。
    for k in CaseKind::ALL.iter() {
        led.tries += 1;
        let p = k.param(4); // 每族第 4 例恒为违约例
        let a = gate_client(*k, p);
        let b = gate_server(*k, p);
        let both_reject = a.is_err() && b.is_err();
        let code_match = match (a, b) {
            (Err(x), Err(y)) => x == y && x == k.expect_code(),
            _ => false,
        };
        if both_reject && code_match {
            led.caught += 1;
        }
    }
    // 族 1。
    led.tries += 1;
    let mut d = PreformDecl::new();
    let _ = d.rehearse();
    let _ = d.advance(DeclPhase::Filed);
    if d.advance(DeclPhase::Declared) == Err(PreformCode::StateRegress.val()) {
        led.caught += 1;
    }
    // 族 2。
    led.tries += 1;
    let mut d2 = PreformDecl::new();
    if d2.advance(DeclPhase::Rehearsed).is_ok()
        && d2.advance(DeclPhase::Filed) == Err(PreformCode::ConservationBreak.val())
    {
        led.caught += 1;
    }
}

// ── 端到端正演 + 双跑一致 ────────────────────────────────────────
/// 正演：声明→预演 30 例→落档 全绿。
pub fn golden_run() -> Result<u32, u32> {
    let mut d = PreformDecl::new();
    let fp = d.rehearse()?;
    d.advance(DeclPhase::Filed)?;
    if d.joint_fp != fp {
        return Err(PreformCode::DeclaredDrift.val());
    }
    Ok(fp)
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) if a == b => Ok(()),
        _ => Err(PreformCode::FingerprintDrift.val()),
    }
}

// ── 判据谓词（私有 xxx_ok） ──────────────────────────────────────
/// J1：30 例联测全过。
fn joint_ok() -> bool {
    match joint_thirty() {
        Ok(led) => led.agree == CASE_TOTAL && led.silent_leak == 0,
        Err(_) => false,
    }
}

/// J2：双侧一致零静默（与 J1 同账双口径复核）。
fn bilateral_ok() -> bool {
    match joint_thirty() {
        Ok(led) => led.split == 0 && led.silent_leak == 0 && led.conservation().is_ok(),
        Err(_) => false,
    }
}

/// J3：注入全抓。
fn inject_ok() -> bool {
    let mut led = InjectLedger::new();
    inject_violations(&mut led);
    led.all_caught()
}

/// J4：抽样 10 例。
fn sample_ok() -> bool {
    sample_ten() == Ok(10)
}

/// J5：双跑一致。
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
    for c in PreformCode::ALL.iter() {
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
    v.push((
        "J1-30例联测全过",
        joint_ok(),
        format!("{:?}", joint_thirty().map(|l| (l.total, l.agree, l.silent_leak))),
    ));
    v.push((
        "J1-双侧一致零静默",
        bilateral_ok(),
        String::from("split=0 且 leak=0"),
    ));
    // 边界：每族首例（合法例）双侧都过。
    let mut legal_all_pass = true;
    for k in CaseKind::ALL.iter() {
        if run_case(*k, 0).0 == false {
            legal_all_pass = false;
        }
    }
    v.push((
        "J2-合法例双侧放行",
        legal_all_pass,
        String::from("五族首例全绿"),
    ));
    v.push(("J3-注入全抓", inject_ok(), String::from("违约/倒退/空指纹")));
    v.push(("J4-抽样10例对平", sample_ok(), format!("{:?}", sample_ten())));
    // 边界：守恒账等式（先合法账过，再破一侧必抓）。
    let mut probe_led = JointLedger { total: 5, agree: 4, silent_leak: 0, split: 1 };
    let ok_then_break = probe_led.conservation().is_ok() && {
        probe_led.agree = 3;
        probe_led.conservation() == Err(PreformCode::ConservationBreak.val())
    };
    v.push(("J5-守恒账等式", ok_then_break, String::from("agree+split=total")));
    v.push(("J6-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J7-段位守卫", segment_guard(), String::from("0x803C0 窗口")));
    v
}

/// VX9 检查入口（卡 12615，UNX-F12616-J1 判据承载）。
pub fn run_unx12615_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12615 D4 未开工联签预演形态声明");
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
    fn test_joint_thirty_all_pass() {
        let led = joint_thirty().unwrap_or(JointLedger::new());
        assert_eq!(led.total, 30);
        assert_eq!(led.agree, 30);
        assert_eq!(led.silent_leak, 0);
        assert_eq!(led.split, 0);
    }

    #[test]
    fn test_bilateral_consistency() {
        assert!(bilateral_ok());
    }

    #[test]
    fn test_injection_all_caught() {
        let mut led = InjectLedger::new();
        inject_violations(&mut led);
        assert_eq!(led.tries, 7);
        assert_eq!(led.caught, 7);
    }

    #[test]
    fn test_sample_ten() {
        assert_eq!(sample_ten(), Ok(10));
    }

    #[test]
    fn test_state_regress_rejected() {
        let mut d = PreformDecl::new();
        assert!(d.rehearse().is_ok());
        assert_eq!(
            d.advance(DeclPhase::Declared),
            Err(PreformCode::StateRegress.val())
        );
    }

    #[test]
    fn test_filed_requires_fingerprint() {
        let mut d = PreformDecl::new();
        assert_eq!(
            d.advance(DeclPhase::Rehearsed),
            Ok(()),
            "Declared→Rehearsed 允许"
        );
        assert_eq!(
            d.advance(DeclPhase::Filed),
            Err(PreformCode::ConservationBreak.val()),
            "空指纹不许落档"
        );
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
        assert_eq!(SEG_BASE, 0x803C0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in PreformCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0x1234).contains("未知"));
    }
}
