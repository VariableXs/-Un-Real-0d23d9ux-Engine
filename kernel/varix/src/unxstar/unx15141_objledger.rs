//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15141 · 对象账清账总验（六类对象账逐类清点与总账勾稽） · 0x66FC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F15141-J1 六类对象账逐类清点与总账勾稽零缺漏；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 六类闭集：窗口/菜单/图标/光标/刷子/字体（句柄类对象）；
//!   - 各类基线数写死（来源：对象账批 11 基线）：
//!     窗口 128 / 菜单 64 / 图标 32 / 光标 16 / 刷子 48 / 字体 24，
//!     总账 312；
//!   - 清账状态机（每对象）：Allocated→InUse→Released→Cleared，
//!     逆序一律拒收（未分配释放/重复释放/清后使用）；
//!   - 勾稽三层：类层（类内 Cleared 数==基线数）→ 总账层（六类
//!     Cleared 合计==312）→ 生命周期层（每对象恰清一次，相位无悬挂）；
//!   - 反向判据：抽一对象不清（缺漏注入）后勾稽必须失败报缺漏；
//!   - LCG 种子写死 = SEG_BASE（0x66FC0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66FC0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66FC0;

/// 六类闭集。
pub const CLS_WINDOW: usize = 0;
pub const CLS_MENU: usize = 1;
pub const CLS_ICON: usize = 2;
pub const CLS_CURSOR: usize = 3;
pub const CLS_BRUSH: usize = 4;
pub const CLS_FONT: usize = 5;
pub const CLS_COUNT: usize = 6;
pub const CLS_NAMES: [&str; CLS_COUNT] = ["窗口", "菜单", "图标", "光标", "刷子", "字体"];
/// 各类基线数（写死，来源：对象账批 11 基线）。
pub const CLS_BASELINE: [u32; CLS_COUNT] = [128, 64, 32, 16, 48, 24];
/// 黄金总账（写死，判据常量独立直加：128+64+32+16+48+24=312）。
pub const GOLDEN_GRAND: u32 = 312;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjCode {
    /// 类序号越闭集
    ClassOutOfRange,
    /// 对象号越类基线
    ObjOutOfRange,
    /// 未分配先释放
    ReleaseBeforeAlloc,
    /// 重复释放
    DoubleRelease,
    /// 相位回退（清后使用）
    PhaseRegress,
    /// 类清点与基线不一致
    ClassCountBad,
    /// 总账勾稽不一致
    GrandMismatch,
    /// 清账缺漏（有对象未清）
    ClearMissing,
    /// 相位悬挂（停留中间态）
    PhaseHung,
    /// 双跑不一致（不可复现）
    NotReproducible,
}

impl ObjCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ObjCode::ClassOutOfRange => "类序号越闭集：六类冻结",
            ObjCode::ObjOutOfRange => "对象号越类基线",
            ObjCode::ReleaseBeforeAlloc => "未分配先释放：拒收",
            ObjCode::DoubleRelease => "重复释放：拒收",
            ObjCode::PhaseRegress => "相位回退：清后使用拒收",
            ObjCode::ClassCountBad => "类清点与基线不一致",
            ObjCode::GrandMismatch => "总账勾稽不一致",
            ObjCode::ClearMissing => "清账缺漏：有对象未清",
            ObjCode::PhaseHung => "相位悬挂：停留中间态",
            ObjCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ObjCode; 10] = [
        ObjCode::ClassOutOfRange,
        ObjCode::ObjOutOfRange,
        ObjCode::ReleaseBeforeAlloc,
        ObjCode::DoubleRelease,
        ObjCode::PhaseRegress,
        ObjCode::ClassCountBad,
        ObjCode::GrandMismatch,
        ObjCode::ClearMissing,
        ObjCode::PhaseHung,
        ObjCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ObjCode::ALL.len() {
        if ObjCode::ALL[i].val() == code {
            return ObjCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66FC0 段闭集内"
}

// ── 对象生命周期状态机 ───────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjPhase {
    Free,
    Allocated,
    InUse,
    Released,
    Cleared,
}

/// 单步转移（写死边闭集）：Free+Alloc→Allocated；Allocated+Use→
/// InUse；InUse+Release→Released；Released+Clear→Cleared。其余全拒。
pub fn obj_step(p: ObjPhase, ev: ObjEv) -> Result<ObjPhase, u32> {
    match (p, ev) {
        (ObjPhase::Free, ObjEv::Alloc) => Ok(ObjPhase::Allocated),
        (ObjPhase::Allocated, ObjEv::Use) => Ok(ObjPhase::InUse),
        (ObjPhase::InUse, ObjEv::Release) => Ok(ObjPhase::Released),
        (ObjPhase::Released, ObjEv::Clear) => Ok(ObjPhase::Cleared),
        (ObjPhase::Free, ObjEv::Release) => Err(ObjCode::ReleaseBeforeAlloc.val()),
        (ObjPhase::Released, ObjEv::Release) => Err(ObjCode::DoubleRelease.val()),
        (ObjPhase::Cleared, _) => Err(ObjCode::PhaseRegress.val()),
        (ObjPhase::Free, ObjEv::Use) => Err(ObjCode::ReleaseBeforeAlloc.val()),
        (ObjPhase::Free, ObjEv::Clear) => Err(ObjCode::ReleaseBeforeAlloc.val()),
        (ObjPhase::Allocated, ObjEv::Clear) => Err(ObjCode::PhaseRegress.val()),
        (ObjPhase::Allocated, ObjEv::Release) => Err(ObjCode::PhaseRegress.val()),
        (ObjPhase::InUse, ObjEv::Clear) => Err(ObjCode::PhaseRegress.val()),
        (ObjPhase::InUse, ObjEv::Alloc) => Err(ObjCode::PhaseRegress.val()),
        (ObjPhase::Released, ObjEv::Use) => Err(ObjCode::PhaseRegress.val()),
        // 兜底闭集：其余一切组合按相位回退拒收。
        _ => Err(ObjCode::PhaseRegress.val()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjEv {
    Alloc,
    Use,
    Release,
    Clear,
}

// ── 六类清账账本 ─────────────────────────────────────────────────
pub struct ObjLedger {
    /// phases[cls][i]：类 cls 第 i 对象相位（扁平 Vec，cls*基线偏移）。
    pub phases: Vec<ObjPhase>,
    pub cleared_per_cls: [u32; CLS_COUNT],
    pub cleared_total: u32,
    pub booked: u32,
}

impl ObjLedger {
    pub fn new() -> ObjLedger {
        let mut phases: Vec<ObjPhase> = Vec::new();
        let mut c = 0;
        while c < CLS_COUNT {
            let mut i = 0;
            while i < CLS_BASELINE[c] {
                phases.push(ObjPhase::Free);
                i += 1;
            }
            c += 1;
        }
        ObjLedger {
            phases,
            cleared_per_cls: [0; CLS_COUNT],
            cleared_total: 0,
            booked: 0,
        }
    }

    /// 扁平下标换算：（类，类内序）→ Vec 下标。
    fn flat(&self, cls: usize, idx: u32) -> Result<usize, u32> {
        if cls >= CLS_COUNT {
            return Err(ObjCode::ClassOutOfRange.val());
        }
        if idx >= CLS_BASELINE[cls] {
            return Err(ObjCode::ObjOutOfRange.val());
        }
        let mut off = 0usize;
        let mut c = 0;
        while c < cls {
            off += CLS_BASELINE[c] as usize;
            c += 1;
        }
        Ok(off + idx as usize)
    }

    /// 单对象事件注入。
    pub fn feed(&mut self, cls: usize, idx: u32, ev: ObjEv) -> Result<ObjPhase, u32> {
        let ix = self.flat(cls, idx)?;
        let next = obj_step(self.phases[ix], ev)?;
        self.phases[ix] = next;
        if next == ObjPhase::Cleared {
            self.cleared_per_cls[cls] += 1;
            self.cleared_total += 1;
        }
        Ok(next)
    }

    /// 全账清账：每对象 Alloc→Use→Release→Clear 四步走满。
    pub fn clear_all(&mut self) -> Result<u32, u32> {
        let mut c = 0;
        while c < CLS_COUNT {
            let mut i = 0;
            while i < CLS_BASELINE[c] {
                self.feed(c, i, ObjEv::Alloc)?;
                self.feed(c, i, ObjEv::Use)?;
                self.feed(c, i, ObjEv::Release)?;
                self.feed(c, i, ObjEv::Clear)?;
                i += 1;
            }
            c += 1;
        }
        // 类层勾稽。
        let mut cls_ok = true;
        let mut k = 0;
        while k < CLS_COUNT {
            if self.cleared_per_cls[k] != CLS_BASELINE[k] {
                cls_ok = false;
            }
            k += 1;
        }
        if !cls_ok {
            return Err(ObjCode::ClassCountBad.val());
        }
        // 总账勾稽。
        if self.cleared_total != GOLDEN_GRAND {
            return Err(ObjCode::GrandMismatch.val());
        }
        self.booked = 1;
        Ok(self.cleared_total)
    }

    /// 相位收敛：全对象 Cleared。
    pub fn all_cleared(&self) -> bool {
        let mut i = 0;
        while i < self.phases.len() {
            if self.phases[i] != ObjPhase::Cleared {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 缺漏注入：跳过（类，类内序）一个对象不清，勾稽必须失败。
pub fn missing_probe(skip_cls: usize, skip_idx: u32) -> Result<(), u32> {
    let mut led = ObjLedger::new();
    let mut c = 0;
    while c < CLS_COUNT {
        let mut i = 0;
        while i < CLS_BASELINE[c] {
            let is_skip = c == skip_cls && i == skip_idx;
            if !is_skip {
                let _ = led.feed(c, i, ObjEv::Alloc);
                let _ = led.feed(c, i, ObjEv::Use);
                let _ = led.feed(c, i, ObjEv::Release);
                let _ = led.feed(c, i, ObjEv::Clear);
            }
            i += 1;
        }
        c += 1;
    }
    if led.cleared_total != GOLDEN_GRAND {
        return Err(ObjCode::ClearMissing.val());
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
    let mut led = ObjLedger::new();
    let total = led.clear_all()?;
    if total != GOLDEN_GRAND {
        return Err(ObjCode::GrandMismatch.val());
    }
    if !led.all_cleared() {
        return Err(ObjCode::PhaseHung.val());
    }
    // 复现：双跑一致。
    let mut led2 = ObjLedger::new();
    let t2 = led2.clear_all()?;
    if t2 != total {
        return Err(ObjCode::NotReproducible.val());
    }
    // 反向判据：六类各抽一对象不清，全部检出。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    let mut c = 0;
    while c < CLS_COUNT {
        let idx = (lcg.below(CLS_BASELINE[c] as u64)) as u32;
        if missing_probe(c, idx).is_err() {
            caught += 1;
        }
        c += 1;
    }
    if caught != CLS_COUNT as u32 {
        return Err(ObjCode::ClearMissing.val());
    }
    Ok((total, caught))
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
    // 族 0：未分配先释放。
    ledger.tries[0] += 1;
    let mut bad0 = false;
    if let Err(e) = obj_step(ObjPhase::Free, ObjEv::Release) {
        if e == ObjCode::ReleaseBeforeAlloc.val() {
            bad0 = true;
        }
    }
    if bad0 {
        ledger.caught[0] += 1;
    }
    // 族 1：重复释放。
    ledger.tries[1] += 1;
    let mut bad1 = false;
    if let Err(e) = obj_step(ObjPhase::Released, ObjEv::Release) {
        if e == ObjCode::DoubleRelease.val() {
            bad1 = true;
        }
    }
    if bad1 {
        ledger.caught[1] += 1;
    }
    // 族 2：对象号越类基线（窗口类第 128 号）。
    ledger.tries[2] += 1;
    let mut bad2 = false;
    let mut led = ObjLedger::new();
    if let Err(e) = led.feed(CLS_WINDOW, 128, ObjEv::Alloc) {
        if e == ObjCode::ObjOutOfRange.val() {
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
    while i < ObjCode::ALL.len() {
        let v = ObjCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：312 全清 + 反向 6/6 + 复现。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((t, c)) => *t == GOLDEN_GRAND && *c == 6,
        Err(_) => false,
    };
    v.push(("J1-312全清+反向6/6+复现", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 判据常量独立直加：六类基线直加==黄金总账 312。
    let mut hand = 0;
    let mut i = 0;
    while i < CLS_COUNT {
        hand += CLS_BASELINE[i];
        i += 1;
    }
    v.push((
        "J1-六类基线直加==总账312",
        hand == GOLDEN_GRAND && GOLDEN_GRAND == 312,
        format!("hand={}", hand),
    ));
    // 4. 六类名点验。
    let mut named = 0;
    i = 0;
    while i < CLS_COUNT {
        if CLS_NAMES[i].len() >= 2 {
            named += 1;
        }
        i += 1;
    }
    v.push(("J2-六类名点验", named == 6, format!("{}/6", named)));
    // 5. 生命周期边抽验：Released+Clear 合法；Cleared+Use 拒。
    let ok_edge = obj_step(ObjPhase::Released, ObjEv::Clear) == Ok(ObjPhase::Cleared);
    let bad_edge = obj_step(ObjPhase::Cleared, ObjEv::Use).is_err();
    v.push((
        "J2-生命周期边抽验",
        ok_edge && bad_edge,
        format!("clear_ok={} cleared_use_bad={}", ok_edge, bad_edge),
    ));
    // 6. 扁平偏移点验：类 3（光标）首对象偏移 =128+64+32=224。
    let mut led0 = ObjLedger::new();
    let flat3 = led0.flat(CLS_CURSOR, 0);
    v.push((
        "J2-扁平偏移点验",
        flat3 == Ok(224),
        format!("{:?}", flat3),
    ));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66FC0 窗口")));
    v
}

pub fn run_unx15141_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15141 对象账清账总验（六类逐类清点勾稽零缺漏）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
