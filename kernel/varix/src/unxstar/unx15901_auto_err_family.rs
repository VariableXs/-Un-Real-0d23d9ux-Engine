//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15901 · 自动化错误族总纲（四族面谱 + 漏族注入） · 0x67940
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15901-J1 总纲成文（自动化错误四族——类型失配/成员缺失/
//!      参数错/分发失败 面谱 10/10 次），漏族注入检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 自动化错误四族闭集：类型失配（VT 不符）/ 成员缺失（DISPID
//!     查无）/ 参数错（序数越界或必选缺）/ 分发失败（Invoke 通道断）；
//!   - 分发面账本：接口成员表（名+VT+序数）+ 分发通道状态机；
//!   - 错误落账状态机：Dispatch → Suspect → ErrFamily(k)（终态）；
//!   - 漏族注入：错误账四族缺一 → 巡检必检出；
//!   - VT 闭集写死：I4/BSTR/BOOL/IDISPATCH 四型；
//!   - LCG 种子写死 = SEG_BASE（0x67940），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67940，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67940;

/// 每族正演次数（判据钉版：10/10）。
pub const RUNS_PER_FAMILY: u32 = 10;

/// 四族闭集。
pub const FAM_TYPEMISMATCH: usize = 0;
pub const FAM_MEMBERMISSING: usize = 1;
pub const FAM_PARAMBAD: usize = 2;
pub const FAM_DISPATCHFAIL: usize = 3;
pub const FAM_COUNT: usize = 4;
pub const FAM_NAMES: [&str; FAM_COUNT] =
    ["类型失配族", "成员缺失族", "参数错族", "分发失败族"];

/// VT 闭集（4 型，写死：自动化推定）。
pub const VT_I4: u32 = 3;
pub const VT_BSTR: u32 = 8;
pub const VT_BOOL: u32 = 11;
pub const VT_IDISPATCH: u32 = 9;
pub const VT_SET: [u32; 4] = [VT_I4, VT_BSTR, VT_BOOL, VT_IDISPATCH];
pub const VT_NAMES: [&str; 4] = ["I4", "BSTR", "BOOL", "IDISPATCH"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutoCode {
    /// 族越闭集
    FamilyOutOfRange,
    /// 类型失配（VT 不符）
    TypeMismatch,
    /// 成员缺失（DISPID 查无）
    MemberMissing,
    /// 参数错（序数越界或必选缺）
    ParamBad,
    /// 分发失败（通道断）
    DispatchFail,
    /// VT 越闭集
    VtOutOfRange,
    /// 非法相位边
    IllegalPhaseEdge,
    /// 漏族（错误账四族缺一）
    FamilyMissing,
    /// 双跑不一致
    NotReproducible,
}

impl AutoCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AutoCode::FamilyOutOfRange => "族越闭集：四族冻结",
            AutoCode::TypeMismatch => "类型失配：VT 不符",
            AutoCode::MemberMissing => "成员缺失：DISPID 查无",
            AutoCode::ParamBad => "参数错：序数越界或必选缺",
            AutoCode::DispatchFail => "分发失败：Invoke 通道断",
            AutoCode::VtOutOfRange => "VT 越闭集：四型冻结",
            AutoCode::IllegalPhaseEdge => "非法相位边：落账状态机闭集外",
            AutoCode::FamilyMissing => "漏族：错误账四族缺一",
            AutoCode::NotReproducible => "双跑不一致：面谱复现红线",
        }
    }
    pub const ALL: [AutoCode; 9] = [
        AutoCode::FamilyOutOfRange,
        AutoCode::TypeMismatch,
        AutoCode::MemberMissing,
        AutoCode::ParamBad,
        AutoCode::DispatchFail,
        AutoCode::VtOutOfRange,
        AutoCode::IllegalPhaseEdge,
        AutoCode::FamilyMissing,
        AutoCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AutoCode::ALL.len() {
        if AutoCode::ALL[i].val() == code {
            return AutoCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67940 段闭集内"
}

// ── 接口成员表（写死：8 成员） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DispatchMember {
    pub dispid: i32,
    pub name: &'static str,
    pub vt: u32,
    pub required_params: u32,
}

pub const MEMBER_TABLE: [DispatchMember; 8] = [
    DispatchMember { dispid: 1, name: "Open", vt: VT_BOOL, required_params: 2 },
    DispatchMember { dispid: 2, name: "Close", vt: VT_I4, required_params: 0 },
    DispatchMember { dispid: 3, name: "Read", vt: VT_BSTR, required_params: 1 },
    DispatchMember { dispid: 4, name: "Write", vt: VT_BOOL, required_params: 2 },
    DispatchMember { dispid: 5, name: "Seek", vt: VT_I4, required_params: 1 },
    DispatchMember { dispid: 6, name: "Panel", vt: VT_IDISPATCH, required_params: 0 },
    DispatchMember { dispid: 7, name: "Lock", vt: VT_I4, required_params: 1 },
    DispatchMember { dispid: 8, name: "Unlock", vt: VT_BOOL, required_params: 1 },
];

// ── 落账状态机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrPhase {
    /// 分发通道就绪
    Dispatch,
    /// 可疑（一次失配未定案）
    Suspect,
    /// 定案（族 k，终态）
    ErrFamily(usize),
}

/// 合法边闭集：Dispatch→Suspect、Suspect→ErrFamily(k)（k<4）、
/// ErrFamily(k)→ErrFamily(k)（重犯同族）；其余非法。
pub fn err_edge_legal(from: ErrPhase, to: ErrPhase) -> bool {
    match (from, to) {
        (ErrPhase::Dispatch, ErrPhase::Suspect) => true,
        (ErrPhase::Suspect, ErrPhase::ErrFamily(k)) => k < FAM_COUNT,
        (ErrPhase::ErrFamily(_), ErrPhase::ErrFamily(_)) => true,
        _ => false,
    }
}

// ── 分发面账本 ───────────────────────────────────────────────────
pub struct DispatchLedger {
    pub phase: ErrPhase,
    pub fam_hits: [u32; FAM_COUNT],
    pub invoke_ok: u32,
    pub members: Vec<DispatchMember>,
}

impl DispatchLedger {
    pub fn new() -> DispatchLedger {
        let mut members: Vec<DispatchMember> = Vec::new();
        let mut i = 0;
        while i < MEMBER_TABLE.len() {
            members.push(MEMBER_TABLE[i]);
            i += 1;
        }
        DispatchLedger {
            phase: ErrPhase::Dispatch,
            fam_hits: [0; FAM_COUNT],
            invoke_ok: 0,
            members,
        }
    }

    fn land(&mut self, fam: usize) -> Result<(), u32> {
        if fam >= FAM_COUNT {
            return Err(AutoCode::FamilyOutOfRange.val());
        }
        // Dispatch 必先过 Suspect（两步走）。
        if let ErrPhase::Dispatch = self.phase {
            if !err_edge_legal(self.phase, ErrPhase::Suspect) {
                return Err(AutoCode::IllegalPhaseEdge.val());
            }
            self.phase = ErrPhase::Suspect;
        }
        if !err_edge_legal(ErrPhase::Suspect, ErrPhase::ErrFamily(fam)) {
            return Err(AutoCode::IllegalPhaseEdge.val());
        }
        self.fam_hits[fam] += 1;
        self.phase = ErrPhase::ErrFamily(fam);
        Ok(())
    }

    /// 正常 Invoke：成员查得到、VT 相符、参数齐 → 成功计账。
    pub fn invoke(&mut self, dispid: i32, vt: u32, params: u32) -> Result<usize, u32> {
        // 成员查表（下标先比长度）。
        let mut found = self.members.len();
        let mut i = 0;
        while i < self.members.len() {
            if self.members[i].dispid == dispid {
                found = i;
                break;
            }
            i += 1;
        }
        if found == self.members.len() {
            return self.land_missing();
        }
        // VT 闭集闸。
        let mut vt_ok = false;
        let mut j = 0;
        while j < VT_SET.len() {
            if VT_SET[j] == vt {
                vt_ok = true;
            }
            j += 1;
        }
        if !vt_ok {
            return Err(AutoCode::VtOutOfRange.val());
        }
        if self.members[found].vt != vt {
            return self.land_type_mismatch();
        }
        if params < self.members[found].required_params {
            return self.land_param_bad();
        }
        if let ErrPhase::ErrFamily(_) = self.phase {
            // 通道已断（定案后不可正常分发）。
            return self.land_dispatch_fail();
        }
        self.invoke_ok += 1;
        Ok(found)
    }

    /// 族 0：类型失配。
    pub fn land_type_mismatch(&mut self) -> Result<usize, u32> {
        self.land(FAM_TYPEMISMATCH)?;
        Ok(0)
    }
    /// 族 1：成员缺失。
    pub fn land_missing(&mut self) -> Result<usize, u32> {
        self.land(FAM_MEMBERMISSING)?;
        Ok(0)
    }
    /// 族 2：参数错。
    pub fn land_param_bad(&mut self) -> Result<usize, u32> {
        self.land(FAM_PARAMBAD)?;
        Ok(0)
    }
    /// 族 3：分发失败。
    pub fn land_dispatch_fail(&mut self) -> Result<usize, u32> {
        self.land(FAM_DISPATCHFAIL)?;
        Ok(0)
    }

    /// 漏族巡检。
    pub fn family_audit(&self) -> Result<u32, u32> {
        let mut present = 0u32;
        let mut i = 0;
        while i < FAM_COUNT {
            if self.fam_hits[i] > 0 {
                present += 1;
            }
            i += 1;
        }
        if present < FAM_COUNT as u32 {
            return Err(AutoCode::FamilyMissing.val());
        }
        Ok(present)
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 写死） ──────────────────────────
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
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 注入面：三族各 10 次，10/10 全抓 ─────────────────────────────
pub const INJ_FAMILIES: usize = 3;
pub const INJ_NAMES: [&str; INJ_FAMILIES] = ["漏族账族", "VT越界族", "DISPID越界族"];

pub struct InjectLedger {
    pub tries: [u32; INJ_FAMILIES],
    pub caught: [u32; INJ_FAMILIES],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJ_FAMILIES], caught: [0; INJ_FAMILIES] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJ_FAMILIES {
            if self.caught[i] != self.tries[i] || self.tries[i] != RUNS_PER_FAMILY {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：漏族账注入（只录三族 ×10）→ FamilyMissing。
    let mut k = 0u32;
    while k < RUNS_PER_FAMILY {
        ledger.tries[0] += 1;
        let mut d = DispatchLedger::new();
        let _ = d.land_type_mismatch();
        let _ = d.land_missing();
        let _ = d.land_param_bad();
        if let Err(e) = d.family_audit() {
            if e == AutoCode::FamilyMissing.val() {
                ledger.caught[0] += 1;
            }
        }
        k += 1;
    }
    // 族 1：VT 越界注入（vt=99 ×10）→ VtOutOfRange。
    k = 0;
    while k < RUNS_PER_FAMILY {
        ledger.tries[1] += 1;
        let mut d = DispatchLedger::new();
        if let Err(e) = d.invoke(2, 99, 0) {
            if e == AutoCode::VtOutOfRange.val() {
                ledger.caught[1] += 1;
            }
        }
        k += 1;
    }
    // 族 2：DISPID 越界注入（dispid=999 ×10）→ MemberMissing。
    k = 0;
    while k < RUNS_PER_FAMILY {
        ledger.tries[2] += 1;
        let mut d = DispatchLedger::new();
        if let Err(e) = d.invoke(999, VT_I4, 0) {
            if e == AutoCode::MemberMissing.val() {
                ledger.caught[2] += 1;
            }
        }
        k += 1;
    }
}

// ── 黄金正演：四族面谱逐族 10 次 ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenResult {
    pub fam_hits: [u32; FAM_COUNT],
    pub invoke_ok: u32,
    pub families: u32,
}

pub fn golden_run() -> Result<GoldenResult, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut d = DispatchLedger::new();
    // 族 0：类型失配 ×10（Read 是 BSTR，传 I4）。
    let mut r = 0u32;
    while r < RUNS_PER_FAMILY {
        let _ = g.below(9);
        d.invoke(3, VT_I4, 1)?;
        r += 1;
    }
    // 族 1：成员缺失 ×10（dispid=99）。
    let mut r = 0u32;
    while r < RUNS_PER_FAMILY {
        d.invoke(99, VT_I4, 0)?;
        r += 1;
    }
    // 族 2：参数错 ×10（Write 需 2 参只给 0）。
    let mut r = 0u32;
    while r < RUNS_PER_FAMILY {
        d.invoke(4, VT_BOOL, 0)?;
        r += 1;
    }
    // 族 3：分发失败 ×10（通道已断再正常分发）。
    let mut r = 0u32;
    while r < RUNS_PER_FAMILY {
        d.invoke(2, VT_I4, 0)?;
        r += 1;
    }
    let families = d.family_audit()?;
    Ok(GoldenResult {
        fam_hits: d.fam_hits,
        invoke_ok: d.invoke_ok,
        families,
    })
}

/// 双跑复现。
pub fn reproducible() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    let mut i = 0;
    while i < FAM_COUNT {
        if a.fam_hits[i] != b.fam_hits[i] {
            return Err(AutoCode::NotReproducible.val());
        }
        i += 1;
    }
    if a.invoke_ok != b.invoke_ok || a.families != b.families {
        return Err(AutoCode::NotReproducible.val());
    }
    Ok(())
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AutoCode::ALL.len() {
        let v = AutoCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：四族面谱各 10/10。
    let golden = golden_run();
    let golden_full = match &golden {
        Ok(g) => {
            let mut full = g.families == FAM_COUNT as u32;
            let mut i = 0;
            while i < FAM_COUNT {
                if g.fam_hits[i] < RUNS_PER_FAMILY {
                    full = false;
                }
                i += 1;
            }
            full
        }
        Err(_) => false,
    };
    v.push(("J1-四族面谱10/10", golden_full, format!("{:?}", golden)));
    // 2. 注入三族 10/10 全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-漏族三族10/10",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 双跑复现。
    v.push(("J1-双跑复现一致", matches!(reproducible(), Ok(())), String::from("双跑逐项")));
    // 4. 成员表点验：8 成员名非空 + DISPID 1..8 连续。
    let mut ok = 0;
    let mut i = 0;
    while i < MEMBER_TABLE.len() {
        if MEMBER_TABLE[i].name.len() > 0 && MEMBER_TABLE[i].dispid == i as i32 + 1 {
            ok += 1;
        }
        i += 1;
    }
    v.push(("J1-成员表8条点验", ok == 8, format!("{}/8", ok)));
    // 5. VT 闭集点验。
    let mut okv = 0;
    i = 0;
    while i < VT_SET.len() {
        if VT_NAMES[i].len() > 0 {
            okv += 1;
        }
        i += 1;
    }
    v.push(("J1-VT四型点验", okv == 4, format!("{}/4", okv)));
    // 6. 反向判据：正常调用先于定案必成功（Close 无参 I4）。
    let mut d2 = DispatchLedger::new();
    let rev = matches!(d2.invoke(2, VT_I4, 0), Ok(_));
    v.push(("J1-正常Invoke必成", rev && d2.invoke_ok == 1, format!("ok={}", d2.invoke_ok)));
    // 7. 状态机非法边抽验：Dispatch→ErrFamily 直跳必拒（边闸层）。
    let bad = !err_edge_legal(ErrPhase::Dispatch, ErrPhase::ErrFamily(0));
    v.push(("J1-Dispatch→Err直跳必拒", bad, String::from("边闸闭集外")));
    // 8. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67940 窗口")));
    v
}

pub fn run_unx15901_checks() -> CheckSet {
    let mut s = CheckSet::new("CoRun·UNX-D5-B36 自动化错误族总纲（四族面谱 + 漏族注入）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
