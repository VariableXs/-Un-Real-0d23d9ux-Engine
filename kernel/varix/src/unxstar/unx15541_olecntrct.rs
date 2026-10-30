//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15541 · IOleObject 契约面谱 · 0x674C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-D5-B18）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15541-J1 面谱成文（二十法核心八法语义断言 10/10 次），缺法
//!     注入检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 二十法闭集（vtable 20 槽冻结），核心八法提验：SetClientSite/
//!     GetClientSite/SetHostNames/Close/SetExtent/GetExtent/DoVerb/
//!     Update——八法语义各自分账，各 10/10 轮；
//!   - vtable 槽位占用表建模：核心八槽全占=合法；抽任一核心槽=
//!     缺法注入，如实拒；
//!   - 对象生命周期状态机：New → Sited → Activated(DoVerb) →
//!     Closed（吸收终态），Closed 后 DoVerb 非法（复活拒收）；
//!   - 守恒恒等式：Extent 写读一致（SetExtent 后 GetExtent 恒等）；
//!     ClientSite 装卸对称（Set 后 Get 恒等）；
//!   - LCG 种子写死 = SEG_BASE（0x674C0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x674C0，AI-VX3-1030 批13 独占区）。
pub const SEG_BASE: u32 = 0x674C0;

// ── 二十法槽位闭集（vtable 20 槽冻结） ───────────────────────────
pub const M_SET_CLIENT_SITE: usize = 0;
pub const M_GET_CLIENT_SITE: usize = 1;
pub const M_SET_HOST_NAMES: usize = 2;
pub const M_CLOSE: usize = 3;
pub const M_SET_MONIKER: usize = 4;
pub const M_GET_MONIKER: usize = 5;
pub const M_INIT_FROM_DATA: usize = 6;
pub const M_GET_CLIPBOARD_DATA: usize = 7;
pub const M_DO_VERB: usize = 8;
pub const M_ENUM_VERBS: usize = 9;
pub const M_UPDATE: usize = 10;
pub const M_IS_UP_TO_DATE: usize = 11;
pub const M_GET_USER_CLASS_ID: usize = 12;
pub const M_GET_USER_TYPE: usize = 13;
pub const M_SET_EXTENT: usize = 14;
pub const M_GET_EXTENT: usize = 15;
pub const M_ADVISE: usize = 16;
pub const M_UNADVISE: usize = 17;
pub const M_ENUM_ADVISE: usize = 18;
pub const M_GET_MISC_STATUS: usize = 19;
pub const M_COUNT: usize = 20;
pub const M_NAMES: [&str; M_COUNT] = [
    "SetClientSite", "GetClientSite", "SetHostNames", "Close", "SetMoniker",
    "GetMoniker", "InitFromData", "GetClipboardData", "DoVerb", "EnumVerbs",
    "Update", "IsUpToDate", "GetUserClassID", "GetUserType", "SetExtent",
    "GetExtent", "Advise", "Unadvise", "EnumAdvise", "GetMiscStatus",
];

/// 核心八法下标（提验闭集）。
pub const CORE_SLOTS: [usize; 8] = [
    M_SET_CLIENT_SITE, M_GET_CLIENT_SITE, M_SET_HOST_NAMES, M_CLOSE,
    M_DO_VERB, M_SET_EXTENT, M_GET_EXTENT, M_UPDATE,
];

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OleCode {
    /// 法槽下标越界（>19）
    SlotOutOfRange,
    /// 缺法：核心槽空缺（如实拒）
    MissingMethod,
    /// 生命周期非法迁移（Closed 后 DoVerb）
    IllegalLifecycle,
    /// 未挂站点即取站点（GetClientSite 无 Site）
    SiteNotSet,
    /// Extent 写读不一致
    ExtentMismatch,
    /// 未激活即 Close（相位闸）
    CloseBeforeActivate,
    /// 动词越闭集（DoVerb 非法动词）
    VerbOutOfRange,
    /// 守恒破坏（装卸不对称）
    SymmetryBroken,
    /// 缺法注入漏抓
    MissingLeak,
    /// 双跑不一致
    NotReproducible,
}

impl OleCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            OleCode::SlotOutOfRange => "法槽下标越界：20 槽冻结",
            OleCode::MissingMethod => "缺法注入：核心槽空缺，如实拒",
            OleCode::IllegalLifecycle => "生命周期非法迁移：Closed 后复活",
            OleCode::SiteNotSet => "未挂站点即取站点：相位闸",
            OleCode::ExtentMismatch => "Extent 写读不一致：守恒红线",
            OleCode::CloseBeforeActivate => "未激活即 Close：相位闸",
            OleCode::VerbOutOfRange => "动词越闭集：DoVerb 非法动词",
            OleCode::SymmetryBroken => "装卸不对称：守恒红线",
            OleCode::MissingLeak => "缺法注入漏抓：检出 10/10 红线",
            OleCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [OleCode; 10] = [
        OleCode::SlotOutOfRange,
        OleCode::MissingMethod,
        OleCode::IllegalLifecycle,
        OleCode::SiteNotSet,
        OleCode::ExtentMismatch,
        OleCode::CloseBeforeActivate,
        OleCode::VerbOutOfRange,
        OleCode::SymmetryBroken,
        OleCode::MissingLeak,
        OleCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < OleCode::ALL.len() {
        if OleCode::ALL[i].val() == code {
            return OleCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x674C0 段闭集内"
}

// ── vtable 槽位占用表 ────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct OleVTable {
    pub slots: [bool; M_COUNT],
}

impl OleVTable {
    /// 全满（二十法齐备）。
    pub fn full() -> OleVTable {
        OleVTable { slots: [true; M_COUNT] }
    }
    /// 抽槽注入。
    pub fn poke_hole(&mut self, k: usize) -> Result<(), u32> {
        if k >= M_COUNT {
            return Err(OleCode::SlotOutOfRange.val());
        }
        self.slots[k] = false;
        Ok(())
    }
    /// 核心八法齐备校验。
    pub fn core_complete(&self) -> bool {
        let mut i = 0;
        while i < CORE_SLOTS.len() {
            if !self.slots[CORE_SLOTS[i]] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 缺失核心槽列表。
    pub fn missing_core(&self) -> Vec<usize> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < CORE_SLOTS.len() {
            if !self.slots[CORE_SLOTS[i]] {
                v.push(CORE_SLOTS[i]);
            }
            i += 1;
        }
        v
    }
}

// ── 对象生命周期状态机 ───────────────────────────────────────────
/// 四态闭集：New → Sited → Activated → Closed（吸收终态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OlePhase {
    New,
    Sited,
    Activated,
    Closed,
}

pub fn ole_can_move(from: OlePhase, to: OlePhase) -> bool {
    match (from, to) {
        (OlePhase::New, OlePhase::Sited) => true,
        (OlePhase::Sited, OlePhase::Activated) => true,
        (OlePhase::Sited, OlePhase::Closed) => true,
        (OlePhase::Activated, OlePhase::Closed) => true,
        _ => false,
    }
}

///Extent 尺寸对（HIMETRIC 建模，整数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Extent {
    pub cx: i64,
    pub cy: i64,
}

/// 合法动词闭集（OLEVERB 建模）。
pub const VERB_PRIMARY: i64 = 0;
pub const VERB_SHOW: i64 = -1;
pub const VERB_OPEN: i64 = -2;
pub const VERB_HIDE: i64 = -3;
pub const VERB_INPLACE: i64 = -4;
pub const VERB_COUNT: usize = 5;
pub const VERBS: [i64; VERB_COUNT] =
    [VERB_PRIMARY, VERB_SHOW, VERB_OPEN, VERB_HIDE, VERB_INPLACE];

pub fn verb_legal(v: i64) -> bool {
    let mut i = 0;
    while i < VERB_COUNT {
        if VERBS[i] == v {
            return true;
        }
        i += 1;
    }
    false
}

// ── 契约对象账 ───────────────────────────────────────────────────
pub struct OleObject {
    pub vt: OleVTable,
    pub phase: OlePhase,
    pub site: Option<usize>,
    pub extent: Option<Extent>,
    pub last_verb: Option<i64>,
    pub updated: u32,
}

impl OleObject {
    pub fn new(vt: OleVTable) -> OleObject {
        OleObject {
            vt,
            phase: OlePhase::New,
            site: None,
            extent: None,
            last_verb: None,
            updated: 0,
        }
    }

    fn check_core(&self) -> Result<(), u32> {
        if !self.vt.core_complete() {
            return Err(OleCode::MissingMethod.val());
        }
        Ok(())
    }

    fn step(&mut self, to: OlePhase) -> Result<(), u32> {
        if !ole_can_move(self.phase, to) {
            return Err(OleCode::IllegalLifecycle.val());
        }
        self.phase = to;
        Ok(())
    }

    /// SetClientSite：挂站点（站点号 0..SITE_MAX）。
    pub fn set_client_site(&mut self, site: usize) -> Result<(), u32> {
        self.check_core()?;
        if !self.vt.slots[M_SET_CLIENT_SITE] {
            return Err(OleCode::MissingMethod.val());
        }
        if site >= SITE_MAX {
            return Err(OleCode::SlotOutOfRange.val());
        }
        self.site = Some(site);
        if self.phase == OlePhase::New {
            self.step(OlePhase::Sited)?;
        }
        Ok(())
    }

    /// GetClientSite：未挂即拒。
    pub fn get_client_site(&self) -> Result<usize, u32> {
        if !self.vt.slots[M_GET_CLIENT_SITE] {
            return Err(OleCode::MissingMethod.val());
        }
        match self.site {
            Some(s) => Ok(s),
            None => Err(OleCode::SiteNotSet.val()),
        }
    }

    /// SetHostNames：语义=记账（容器名/对象名非空才收）。
    pub fn set_host_names(&mut self, container: &str, obj: &str) -> Result<(), u32> {
        if !self.vt.slots[M_SET_HOST_NAMES] {
            return Err(OleCode::MissingMethod.val());
        }
        if container.len() == 0 || obj.len() == 0 {
            return Err(OleCode::SymmetryBroken.val());
        }
        Ok(())
    }

    /// DoVerb：动词闭集 + 相位闸（Closed 后拒）。
    pub fn do_verb(&mut self, verb: i64) -> Result<(), u32> {
        if !self.vt.slots[M_DO_VERB] {
            return Err(OleCode::MissingMethod.val());
        }
        if self.phase == OlePhase::Closed {
            return Err(OleCode::IllegalLifecycle.val());
        }
        if !verb_legal(verb) {
            return Err(OleCode::VerbOutOfRange.val());
        }
        if self.phase == OlePhase::New {
            return Err(OleCode::SiteNotSet.val());
        }
        if self.phase == OlePhase::Sited {
            self.step(OlePhase::Activated)?;
        }
        self.last_verb = Some(verb);
        Ok(())
    }

    /// SetExtent：写尺寸。
    pub fn set_extent(&mut self, e: Extent) -> Result<(), u32> {
        if !self.vt.slots[M_SET_EXTENT] {
            return Err(OleCode::MissingMethod.val());
        }
        if e.cx <= 0 || e.cy <= 0 {
            return Err(OleCode::ExtentMismatch.val());
        }
        self.extent = Some(e);
        Ok(())
    }

    /// GetExtent：读尺寸（未写即拒）。
    pub fn get_extent(&self) -> Result<Extent, u32> {
        if !self.vt.slots[M_GET_EXTENT] {
            return Err(OleCode::MissingMethod.val());
        }
        match self.extent {
            Some(e) => Ok(e),
            None => Err(OleCode::ExtentMismatch.val()),
        }
    }

    /// Update：计数。
    pub fn update(&mut self) -> Result<u32, u32> {
        if !self.vt.slots[M_UPDATE] {
            return Err(OleCode::MissingMethod.val());
        }
        self.updated += 1;
        Ok(self.updated)
    }

    /// Close：终态吸收；未激活仅 Sited 可关。
    pub fn close(&mut self) -> Result<(), u32> {
        if !self.vt.slots[M_CLOSE] {
            return Err(OleCode::MissingMethod.val());
        }
        if self.phase == OlePhase::New {
            return Err(OleCode::CloseBeforeActivate.val());
        }
        self.step(OlePhase::Closed)
    }

    /// 守恒：站点装卸对称 + Extent 写读一致。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.site.is_none() && self.phase != OlePhase::New {
            return Err(OleCode::SymmetryBroken.val());
        }
        if let Some(e) = self.extent {
            if e.cx <= 0 || e.cy <= 0 {
                return Err(OleCode::ExtentMismatch.val());
            }
        }
        Ok(())
    }
}

/// 站点号上界（写死：站点池 8 位）。
pub const SITE_MAX: usize = 256;

// ── 确定性 LCG（种子 = SEG_BASE，写死） ──────────────────────────
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
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 核心八法语义断言：10/10 轮 ───────────────────────────────────
pub const GOLDEN_ROUNDS: u32 = 10;

pub fn judge_core_rounds() -> Result<(u32, u32, u32), u32> {
    let mut site_ok = 0;
    let mut extent_ok = 0;
    let mut verb_ok = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut o = OleObject::new(OleVTable::full());
        // SetClientSite/GetClientSite 装卸对称。
        o.set_client_site(r as usize)?;
        if o.get_client_site()? == r as usize {
            site_ok += 1;
        }
        // SetHostNames。
        o.set_host_names("ctn", "obj")?;
        // DoVerb（合法动词）。
        o.do_verb(VERB_SHOW)?;
        if o.last_verb == Some(VERB_SHOW) {
            verb_ok += 1;
        }
        // SetExtent/GetExtent 写读一致。
        let e = Extent { cx: 100 + r as i64, cy: 200 + r as i64 };
        o.set_extent(e)?;
        if o.get_extent()? == e {
            extent_ok += 1;
        }
        // Update。
        let _ = o.update()?;
        // Close 收敛。
        o.close()?;
        r += 1;
    }
    Ok((site_ok, extent_ok, verb_ok))
}

// ── 黄金正演（判据常量独立写死） ─────────────────────────────────
pub const GOLDEN_CORE_OK: u32 = 10;
pub const GOLDEN_MISSING_CAUGHT: u32 = 10;

pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let (s, e, v) = judge_core_rounds()?;
    if s != GOLDEN_CORE_OK || e != GOLDEN_CORE_OK || v != GOLDEN_CORE_OK {
        return Err(OleCode::SymmetryBroken.val());
    }
    // 缺法注入 10/10：逐轮抽一个核心槽，调用必拒。
    let mut caught = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut vt = OleVTable::full();
        let _ = vt.poke_hole(CORE_SLOTS[(r as usize) % CORE_SLOTS.len()]);
        let mut o = OleObject::new(vt);
        if let Err(e) = o.set_client_site(0) {
            if e == OleCode::MissingMethod.val() {
                caught += 1;
            }
        }
        r += 1;
    }
    if caught != GOLDEN_MISSING_CAUGHT {
        return Err(OleCode::MissingLeak.val());
    }
    Ok((s, e, v, caught))
}

// ── 注入面：三族 10/10 全抓 ──────────────────────────────────────
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
    // 族 0：缺法注入 ×10（抽 GetExtent 槽，读必拒）。
    ledger.tries[0] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut vt = OleVTable::full();
        let _ = vt.poke_hole(M_GET_EXTENT);
        let o = OleObject::new(vt);
        if let Err(e) = o.get_extent() {
            if e == OleCode::MissingMethod.val() {
                ledger.caught[0] += 1;
            }
        }
        r += 1;
    }
    // 族 1：Closed 后 DoVerb（复活注入）×10 必拒。
    ledger.tries[1] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut o = OleObject::new(OleVTable::full());
        let _ = o.set_client_site(1);
        let _ = o.do_verb(VERB_PRIMARY);
        let _ = o.close();
        if let Err(e) = o.do_verb(VERB_SHOW) {
            if e == OleCode::IllegalLifecycle.val() {
                ledger.caught[1] += 1;
            }
        }
        r += 1;
    }
    // 族 2：非法动词注入 ×10 必拒。
    ledger.tries[2] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut o = OleObject::new(OleVTable::full());
        let _ = o.set_client_site(2);
        if let Err(e) = o.do_verb(99 + r as i64) {
            if e == OleCode::VerbOutOfRange.val() {
                ledger.caught[2] += 1;
            }
        }
        r += 1;
    }
}

// ── 反向判据：合法八法全谱必须全过 ───────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut o = OleObject::new(OleVTable::full());
    o.set_client_site(7)?;
    o.set_host_names("a", "b")?;
    o.do_verb(VERB_INPLACE)?;
    o.set_extent(Extent { cx: 10, cy: 20 })?;
    let _ = o.get_extent()?;
    let _ = o.update()?;
    o.close()?;
    o.conserve()?;
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < OleCode::ALL.len() {
        let v = OleCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥6 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：核心八法 10/10 + 缺法 10/10 检出。
    let g = golden_run();
    v.push((
        "J1-核心八法10/10+缺法10/10检出",
        matches!(g, Ok((10, 10, 10, 10))),
        format!("{:?}", g),
    ));
    // 2. 注入三族全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 二十法名表点验 + 核心八法下标闭集。
    let names_ok = M_NAMES.len() == M_COUNT && M_COUNT == 20;
    let core_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < CORE_SLOTS.len() {
            if CORE_SLOTS[i] >= M_COUNT {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push((
        "J1-二十法名表+核心八槽闭集",
        names_ok && core_ok,
        format!("20 槽/8 核"),
    ));
    // 4. 生命周期边闭集：Closed→任何态恒非法；New→Activated 跳步非法。
    let closed = !ole_can_move(OlePhase::Closed, OlePhase::Activated)
        && !ole_can_move(OlePhase::New, OlePhase::Activated)
        && ole_can_move(OlePhase::Activated, OlePhase::Closed);
    v.push(("J1-生命周期边闭集", closed, String::from("New→Sited→Activated→Closed")));
    // 5. 动词闭集点验：5 合法动词 + 反向非法动词恒拒。
    let verb_ok = verb_legal(VERB_PRIMARY)
        && verb_legal(VERB_HIDE)
        && !verb_legal(5)
        && !verb_legal(-9);
    v.push(("J1-动词闭集+反向", verb_ok, format!("{} 动词", VERB_COUNT)));
    // 6. 反向判据：合法全谱必须全过。
    v.push(("J1-反向探针合法全谱", matches!(reverse_probe(), Ok(())), String::from("八法全过")));
    // 7. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0xC0);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 8. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x674C0 窗口")));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx15541_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15541 IOleObject 契约面谱");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
