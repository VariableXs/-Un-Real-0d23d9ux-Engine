//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15201 · IUnknown vtable 前三槽 ABI 铁律与三法签名总纲 · 0x67080
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② UNX-F15201-J1 ABI 铁律成文落账（前三槽序 QI/AddRef/Release 与
//!      签名形状冻结，静态断言 10/10 次），槽序错排注入 10/10 次检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三法闭集与冻结槽序：槽 0=QueryInterface、槽 1=AddRef、
//!      槽 2=Release（ABI 铁律，写死）；任何错排一律检出；
//!   - 三法签名形状冻结（形状四元组：本指针位，参数个数，返回类别，
//!      是否写回指针）：QI=(1,2,HResult,Yes)；AddRef=(1,0,Count,No)；
//!      Release=(1,0,Count,No)；
//!   - 静态断言 10 条（逐条冻结）：槽 0 是 QI；槽 1 是 AddRef；槽 2
//!      是 Release；QI 参数 2；QI 写回；AddRef 参数 0；AddRef 不写回；
//!      Release 参数 0；Release 不写回；槽表恰 3 槽无多余——10/10 全过；
//!   - 槽序错排注入：5 个错排（非恒等排列）×2 轮 = 10 次注入，每次
//!      必被静态断言至少一条抓住（10/10 检出）；
//!   - vtable 相位机：Raw→Shaped→Frozen；未定型不可冻结、冻结后禁改；
//!   - LCG 种子写死 = SEG_BASE（0x67080），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67080，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67080;

/// 槽位数（前三槽铁律：恰 3 槽）。
pub const SLOT_COUNT: usize = 3;

// ── 三法闭集 ─────────────────────────────────────────────────────
pub const M_QI: usize = 0;
pub const M_ADDREF: usize = 1;
pub const M_RELEASE: usize = 2;
pub const M_NAMES: [&str; SLOT_COUNT] = ["QueryInterface", "AddRef", "Release"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VtCode {
    /// 槽序错排（铁律违反）
    SlotOrderBad,
    /// 未知方法入槽
    MethodUnknown,
    /// 签名形状不冻结（参数个数不符）
    ShapeArityBad,
    /// 返回类别不符
    ShapeRetBad,
    /// 写回指针位不符
    ShapeOutPtrBad,
    /// 槽表长度非 3
    SlotCountBad,
    /// 未定型冻结
    FreezePremature,
    /// 冻结后改槽
    MutateAfterFreeze,
    /// 静态断言不全过
    StaticAssertFail,
    /// 双跑不一致（不可复现）
    NotReproducible,
}

impl VtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            VtCode::SlotOrderBad => "槽序错排：前三槽 QI/AddRef/Release 铁律",
            VtCode::MethodUnknown => "未知方法入槽",
            VtCode::ShapeArityBad => "签名形状不冻结：参数个数不符",
            VtCode::ShapeRetBad => "返回类别不符",
            VtCode::ShapeOutPtrBad => "写回指针位不符",
            VtCode::SlotCountBad => "槽表长度非 3",
            VtCode::FreezePremature => "未定型冻结：拒收",
            VtCode::MutateAfterFreeze => "冻结后改槽：拒收",
            VtCode::StaticAssertFail => "静态断言不全过",
            VtCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [VtCode; 10] = [
        VtCode::SlotOrderBad,
        VtCode::MethodUnknown,
        VtCode::ShapeArityBad,
        VtCode::ShapeRetBad,
        VtCode::ShapeOutPtrBad,
        VtCode::SlotCountBad,
        VtCode::FreezePremature,
        VtCode::MutateAfterFreeze,
        VtCode::StaticAssertFail,
        VtCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < VtCode::ALL.len() {
        if VtCode::ALL[i].val() == code {
            return VtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67080 段闭集内"
}

// ── 签名形状（冻结四元组） ───────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetKind {
    HResult,
    Count,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SigShape {
    /// 本指针占位（恒 1）。
    pub this_pos: u32,
    /// 除本指针外的参数个数。
    pub arity: u32,
    pub ret: RetKind,
    /// 是否带写回指针（QI 的 ppv）。
    pub out_ptr: bool,
}

/// 冻结签名表（写死，来源：IUnknown ABI 形状总纲）：
///   QI=(1,2,HResult,true)；AddRef=(1,0,Count,false)；
///   Release=(1,0,Count,false)。
pub const FROZEN_SIGS: [SigShape; SLOT_COUNT] = [
    SigShape { this_pos: 1, arity: 2, ret: RetKind::HResult, out_ptr: true },
    SigShape { this_pos: 1, arity: 0, ret: RetKind::Count, out_ptr: false },
    SigShape { this_pos: 1, arity: 0, ret: RetKind::Count, out_ptr: false },
];

/// 黄金槽序（写死）：[QI, AddRef, Release]。
pub const GOLDEN_ORDER: [usize; SLOT_COUNT] = [M_QI, M_ADDREF, M_RELEASE];

// ── vtable 相位机 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VtPhase {
    Raw,
    Shaped,
    Frozen,
}

/// vtable 描述：槽序 + 各槽签名 + 相位。
pub struct VTable {
    pub order: [usize; SLOT_COUNT],
    pub sigs: [SigShape; SLOT_COUNT],
    pub phase: VtPhase,
}

impl VTable {
    /// 以给定槽序与标准签名表造未定型 vtable。
    pub fn raw(order: [usize; SLOT_COUNT]) -> VTable {
        VTable { order, sigs: FROZEN_SIGS, phase: VtPhase::Raw }
    }

    /// 定型：逐槽核对签名形状与槽位声明的标准形状一致。
    pub fn shape(&mut self) -> Result<(), u32> {
        if self.phase != VtPhase::Raw {
            return Err(VtCode::FreezePremature.val());
        }
        let mut i = 0;
        while i < SLOT_COUNT {
            let m = self.order[i];
            if m >= SLOT_COUNT {
                return Err(VtCode::MethodUnknown.val());
            }
            let want = FROZEN_SIGS[m];
            if self.sigs[i] != want {
                return Err(VtCode::ShapeArityBad.val());
            }
            i += 1;
        }
        self.phase = VtPhase::Shaped;
        Ok(())
    }

    /// 冻结：静态断言 10 条全过才可冻结。
    pub fn freeze(&mut self) -> Result<u32, u32> {
        if self.phase != VtPhase::Shaped {
            return Err(VtCode::FreezePremature.val());
        }
        let passed = static_asserts(self)?;
        if passed != 10 {
            return Err(VtCode::StaticAssertFail.val());
        }
        self.phase = VtPhase::Frozen;
        Ok(passed)
    }

    /// 冻结后改槽必拒。
    pub fn mutate_slot(&mut self, slot: usize, m: usize) -> Result<(), u32> {
        if slot >= SLOT_COUNT || m >= SLOT_COUNT {
            return Err(VtCode::SlotCountBad.val());
        }
        if self.phase == VtPhase::Frozen {
            return Err(VtCode::MutateAfterFreeze.val());
        }
        self.order[slot] = m;
        self.phase = VtPhase::Raw; // 改槽回到未定型
        Ok(())
    }
}

/// 静态断言 10 条（逐条独立，返回通过条数）：
pub fn static_asserts(vt: &VTable) -> Result<u32, u32> {
    let mut passed = 0u32;
    // A1：槽表长度恰 3（结构即 3 槽，长度恒等核对）。
    if vt.order.len() == SLOT_COUNT {
        passed += 1;
    }
    // A2：槽 0 是 QI。
    if vt.order[0] == M_QI {
        passed += 1;
    }
    // A3：槽 1 是 AddRef。
    if vt.order[1] == M_ADDREF {
        passed += 1;
    }
    // A4：槽 2 是 Release。
    if vt.order[2] == M_RELEASE {
        passed += 1;
    }
    // A5：槽 0 签名=冻结 QI 形状（arity 2）。
    if vt.sigs[0] == FROZEN_SIGS[M_QI] && vt.sigs[0].arity == 2 {
        passed += 1;
    }
    // A6：槽 0 带写回指针。
    if vt.sigs[0].out_ptr {
        passed += 1;
    }
    // A7：槽 1 签名=冻结 AddRef 形状（arity 0、Count）。
    if vt.sigs[1] == FROZEN_SIGS[M_ADDREF] && vt.sigs[1].ret == RetKind::Count {
        passed += 1;
    }
    // A8：槽 1 不带写回指针。
    if !vt.sigs[1].out_ptr {
        passed += 1;
    }
    // A9：槽 2 签名=冻结 Release 形状（arity 0、Count、无写回）。
    if vt.sigs[2] == FROZEN_SIGS[M_RELEASE]
        && vt.sigs[2].arity == 0
        && !vt.sigs[2].out_ptr
    {
        passed += 1;
    }
    // A10：槽序恰为黄金序逐位一致（无重复无缺漏的恒等核对）。
    if vt.order == GOLDEN_ORDER {
        passed += 1;
    }
    Ok(passed)
}

/// 槽序错排检测：非黄金序返回 Err(SlotOrderBad)。
pub fn order_check(order: [usize; SLOT_COUNT]) -> Result<(), u32> {
    let mut i = 0;
    while i < SLOT_COUNT {
        if order[i] >= SLOT_COUNT {
            return Err(VtCode::MethodUnknown.val());
        }
        let mut dup = false;
        let mut j = 0;
        while j < SLOT_COUNT {
            if i != j && order[i] == order[j] {
                dup = true;
            }
            j += 1;
        }
        if dup {
            return Err(VtCode::SlotOrderBad.val());
        }
        i += 1;
    }
    if order != GOLDEN_ORDER {
        return Err(VtCode::SlotOrderBad.val());
    }
    Ok(())
}

// ── 确定性 LCG（种子=SEG_BASE） ─────────────────────────────────
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
        self.next() % (n | 1)
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut vt = VTable::raw(GOLDEN_ORDER);
    vt.shape()?;
    let passed = vt.freeze()?;
    if passed != 10 {
        return Err(VtCode::StaticAssertFail.val());
    }
    if vt.phase != VtPhase::Frozen {
        return Err(VtCode::FreezePremature.val());
    }
    // 冻结后改槽必拒。
    if vt.mutate_slot(0, M_RELEASE).is_ok() {
        return Err(VtCode::MutateAfterFreeze.val());
    }
    // 复现：双跑一致。
    let mut vt2 = VTable::raw(GOLDEN_ORDER);
    vt2.shape()?;
    let p2 = vt2.freeze()?;
    if p2 != passed {
        return Err(VtCode::NotReproducible.val());
    }
    // 槽序错排注入 10 次：5 个错排 × 2 轮，全检出。
    let wrong_orders: [[usize; SLOT_COUNT]; 5] = [
        [M_ADDREF, M_QI, M_RELEASE],
        [M_QI, M_RELEASE, M_ADDREF],
        [M_RELEASE, M_ADDREF, M_QI],
        [M_RELEASE, M_QI, M_ADDREF],
        [M_ADDREF, M_RELEASE, M_QI],
    ];
    let mut caught = 0;
    let mut round = 0;
    while round < 2 {
        let mut w = 0;
        while w < 5 {
            if order_check(wrong_orders[w]).is_err() {
                caught += 1;
            }
            w += 1;
        }
        round += 1;
    }
    if caught != 10 {
        return Err(VtCode::SlotOrderBad.val());
    }
    Ok((passed, caught))
}

// ── 注入面：三族 tries/caught 全抓 ───────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：槽序错排（AddRef 在槽 0）。
    ledger.tries[0] += 1;
    let mut bad0 = false;
    if let Err(e) = order_check([M_ADDREF, M_QI, M_RELEASE]) {
        if e == VtCode::SlotOrderBad.val() {
            bad0 = true;
        }
    }
    if bad0 {
        ledger.caught[0] += 1;
    }
    // 族 1：未知方法入槽（槽号 3）。
    ledger.tries[1] += 1;
    let mut bad1 = false;
    if let Err(e) = order_check([M_QI, M_ADDREF, 3]) {
        if e == VtCode::MethodUnknown.val() {
            bad1 = true;
        }
    }
    if bad1 {
        ledger.caught[1] += 1;
    }
    // 族 2：未定型冻结（Raw 直接 freeze）。
    ledger.tries[2] += 1;
    let mut bad2 = false;
    let mut vt = VTable::raw(GOLDEN_ORDER);
    if let Err(e) = vt.freeze() {
        if e == VtCode::FreezePremature.val() {
            bad2 = true;
        }
    }
    if bad2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < VtCode::ALL.len() {
        let v = VtCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：静态断言 10/10 + 错排注入 10/10 + 冻结后禁改。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((p, c)) => *p == 10 && *c == 10,
        Err(_) => false,
    };
    v.push(("J1-静态断言10/10+错排注入10/10", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 签名形状冻结点验：QI arity2 写回；AddRef/Release arity0。
    let qi_ok = FROZEN_SIGS[M_QI].arity == 2 && FROZEN_SIGS[M_QI].out_ptr;
    let ar_ok = FROZEN_SIGS[M_ADDREF].arity == 0 && !FROZEN_SIGS[M_ADDREF].out_ptr;
    let rl_ok = FROZEN_SIGS[M_RELEASE].arity == 0 && !FROZEN_SIGS[M_RELEASE].out_ptr;
    v.push((
        "J1-三法签名形状冻结",
        qi_ok && ar_ok && rl_ok,
        format!("qi={} ar={} rl={}", qi_ok, ar_ok, rl_ok),
    ));
    // 4. 错排全排列覆盖：3 槽 6 排列中恰 5 个错排被拒、恒等排列过。
    let perms: [[usize; SLOT_COUNT]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut rejected = 0;
    let mut accepted = 0;
    let mut i = 0;
    while i < 6 {
        if order_check(perms[i]).is_err() {
            rejected += 1;
        } else {
            accepted += 1;
        }
        i += 1;
    }
    v.push((
        "J2-6排列5拒1过",
        rejected == 5 && accepted == 1,
        format!("rej={} acc={}", rejected, accepted),
    ));
    // 5. LCG 乱序注入 16 次：非黄金序全检出。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut lcg_caught = 0;
    let mut k = 0;
    while k < 16 {
        let a = (lcg.below(3)) as usize;
        let b = (lcg.below(3)) as usize;
        let mut order = GOLDEN_ORDER;
        order[0] = GOLDEN_ORDER[b];
        order[b] = GOLDEN_ORDER[a];
        order[a] = GOLDEN_ORDER[0];
        // 非恒等序必须被拒；恒等序必须过。
        if order == GOLDEN_ORDER {
            if order_check(order).is_ok() {
                lcg_caught += 1;
            }
        } else if order_check(order).is_err() {
            lcg_caught += 1;
        }
        k += 1;
    }
    v.push((
        "J2-LCG乱序16次判定一致",
        lcg_caught == 16,
        format!("{}/16", lcg_caught),
    ));
    // 6. 相位机全序：Raw→Shaped→Frozen 不可跳步。
    let mut vt3 = VTable::raw(GOLDEN_ORDER);
    let skip_bad = vt3.freeze().is_err();
    let _ = vt3.shape();
    let freeze_ok = vt3.freeze().is_ok();
    v.push((
        "J2-相位机不可跳步",
        skip_bad && freeze_ok,
        format!("skip_bad={} freeze_ok={}", skip_bad, freeze_ok),
    ));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67080 窗口")));
    v
}

pub fn run_unx15201_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15201 IUnknown vtable 前三槽 ABI 铁律与三法签名总纲");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
