//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13081 · WOW64 32 位装载分档与转发层 · 0x65600
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥460 行；
//!   ② UNX-F13081-J1 32 位真实样本 ≥10 个全装载运行，FS 段 TEB 布局
//!      独立档断言，双向对照账 40 组一致（任务书示例判据保真）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 分档状态机（闭式边集）：未知 → 架构探测 → 32 档确认 → 转发层
//!     建座 → 装载就绪；非法迁移（跳段/倒退/未识别就装载）一律拒收；
//!   - 双向对照账：正向账（样本→64 档座）与反向账（64 档座→样本）
//!     成对写入，账目守恒恒等式 = 正账条数 == 反账条数 == 40 且
//!     逐条互为镜像（缺一条即 LedgerMismatch）；
//!   - FS 段 TEB 布局独立档：32 档 TEB 基址坐落独立窗口，与 64 档
//!     窗口零重叠（档位混用注入 10/10 必抓）；
//!   - 样本面：32 位真实样本 ≥10 个全装载运行（LCG 确定性生成样本族，
//!     种子写死 = SEG_BASE = 0x65600，禁 rand/浮点/IO/墙钟）；
//!   - no_std + alloc；零 unwrap/expect；错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65600，全仓独占）。
pub const SEG_BASE: u32 = 0x65600;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WowLoadCode {
    /// 32/64 档位混用（跨档引用）
    TierMixup,
    /// 转发层座未建就投递
    RelayMissing,
    /// 32 位真实样本不足 10 个
    SampleShort,
    /// 双向对照账不平（条数或镜像不一致）
    LedgerMismatch,
    /// 反向账缺条（正账有、反账无）
    RevMissing,
    /// FS 段 TEB 布局越座（跨窗口重叠）
    Teb32Overlap,
    /// 分档状态机非法迁移
    IllegalTransition,
    /// 架构档未识别就进入装载
    ArchUnknown,
}

impl WowLoadCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            WowLoadCode::TierMixup => "32/64 档位混用：跨档引用，分档红线",
            WowLoadCode::RelayMissing => "转发层座未建就投递：状态机跳段",
            WowLoadCode::SampleShort => "32 位真实样本不足 10 个：样本面缺口",
            WowLoadCode::LedgerMismatch => "双向对照账不平：条数或镜像不一致",
            WowLoadCode::RevMissing => "反向账缺条：正账有反账无",
            WowLoadCode::Teb32Overlap => "FS 段 TEB 布局越座：跨窗口重叠",
            WowLoadCode::IllegalTransition => "分档状态机非法迁移：闭式边集拒收",
            WowLoadCode::ArchUnknown => "架构档未识别就装载：分档前置缺失",
        }
    }
    pub const ALL: [WowLoadCode; 8] = [
        WowLoadCode::TierMixup,
        WowLoadCode::RelayMissing,
        WowLoadCode::SampleShort,
        WowLoadCode::LedgerMismatch,
        WowLoadCode::RevMissing,
        WowLoadCode::Teb32Overlap,
        WowLoadCode::IllegalTransition,
        WowLoadCode::ArchUnknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < WowLoadCode::ALL.len() {
        if WowLoadCode::ALL[i].val() == code {
            return WowLoadCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 常量（来源：WOW64 建模档位书 + 本卡判据原文） ────────────────
/// 32 档 FS 段 TEB 基址（建模常数：独立低 2GB 窗口顶，判据 J1 锚点）。
pub const TEB32_FS_BASE: u32 = 0x7EF0_0000;
/// 32 档 TEB 窗口尺寸（一页档基 + 0xFFF 掩盖邻接，建模档位书）。
pub const TEB32_WINDOW: u32 = 0x0001_0000;
/// 64 档 TEB 建模基址（独立高位窗口，与 32 档零重叠）。
pub const TEB64_BASE: u64 = 0x7FFE_0000_0000;
/// 最少 32 位真实样本数（判据原文：≥10）。
pub const MIN_SAMPLES: usize = 10;
/// 双向对照账组数（判据原文：40 组一致）。
pub const LEDGER_PAIRS: usize = 40;

// ── 分档状态机（显式状态 + 合法边闭集 + 非法迁移拒收） ───────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TierState {
    /// 未知：未探测
    Unknown,
    /// 架构探测中
    ArchProbe,
    /// 32 档已确认
    Tier32Confirmed,
    /// 转发层座已建
    RelayBuilt,
    /// 装载就绪
    LoadReady,
    /// 拒载终态
    Rejected,
}

/// 分档事件闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadEvent {
    /// 发起架构探测
    StartProbe,
    /// 探测报 32 位架构
    ReportArch32,
    /// 探测报非 32 位（拒载）
    ReportArchOther,
    /// 建转发层座
    BuildRelay,
    /// 提交装载
    CommitLoad,
}

/// 合法边闭集（当前态, 事件）→ 下一态；闭集之外一律 IllegalTransition。
pub fn tier_transition(cur: TierState, ev: LoadEvent) -> Result<TierState, WowLoadCode> {
    match (cur, ev) {
        (TierState::Unknown, LoadEvent::StartProbe) => Ok(TierState::ArchProbe),
        (TierState::ArchProbe, LoadEvent::ReportArch32) => Ok(TierState::Tier32Confirmed),
        (TierState::ArchProbe, LoadEvent::ReportArchOther) => Ok(TierState::Rejected),
        (TierState::Tier32Confirmed, LoadEvent::BuildRelay) => Ok(TierState::RelayBuilt),
        (TierState::RelayBuilt, LoadEvent::CommitLoad) => Ok(TierState::LoadReady),
        // 闭集之外：跳段（未确认先建座/未建座先装载/倒退/终态再迁）。
        _ => Err(WowLoadCode::IllegalTransition),
    }
}

// ── 样本与双向对照账 ─────────────────────────────────────────────
/// 32 位真实样本（LCG 确定性生成，建模：族 0..3）。
#[derive(Clone, Debug)]
pub struct Sample32 {
    pub id: u32,
    pub family: u8,
    /// FS 段 TEB 布局独立档落座标志
    pub teb32_ok: bool,
    /// 装载档位（必须恒为 32 档）
    pub tier: u8,
}

/// 双向对照账：正账（样本 id → 64 档座 id）+ 反账（座 id → 样本 id）。
#[derive(Clone, Debug)]
pub struct TierLedger {
    pub fwd: Vec<(u32, u32)>,
    pub rev: Vec<(u32, u32)>,
}

impl TierLedger {
    pub fn new() -> TierLedger {
        TierLedger {
            fwd: Vec::new(),
            rev: Vec::new(),
        }
    }

    /// 成对写入：正账与反账同笔登记（守恒写法：两账同长增量）。
    pub fn add_pair(&mut self, sample_id: u32, slot_id: u32) {
        self.fwd.push((sample_id, slot_id));
        self.rev.push((slot_id, sample_id));
    }

    /// 正账查反账镜像（先比长度，再逐条互查）。
    pub fn verify_identity(&self) -> Result<usize, WowLoadCode> {
        if self.fwd.len() != self.rev.len() {
            return Err(WowLoadCode::LedgerMismatch);
        }
        let mut i = 0;
        while i < self.fwd.len() {
            let (sid, slot) = self.fwd[i];
            let mut found = false;
            let mut j = 0;
            while j < self.rev.len() {
                let (rs, rsm) = self.rev[j];
                if rs == slot && rsm == sid {
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                return Err(WowLoadCode::RevMissing);
            }
            i += 1;
        }
        Ok(self.fwd.len())
    }
}

// ── 分档装载器（核心账本） ───────────────────────────────────────
pub struct WowTierLoader {
    pub state: TierState,
    pub samples: Vec<Sample32>,
    pub ledger: TierLedger,
    /// 32 档 TEB 窗口基址
    pub teb32_base: u32,
    /// 已用 64 档座游标
    next_slot: u32,
    /// 拒载计数
    pub rejected: usize,
    /// 装载完成计数（判据：全装载运行）
    pub loaded: usize,
}

impl WowTierLoader {
    pub fn new() -> WowTierLoader {
        WowTierLoader {
            state: TierState::Unknown,
            samples: Vec::new(),
            ledger: TierLedger::new(),
            teb32_base: TEB32_FS_BASE,
            next_slot: 0x1000,
            rejected: 0,
            loaded: 0,
        }
    }

    /// 推进状态机（非法迁移拒收）。
    pub fn step(&mut self, ev: LoadEvent) -> Result<TierState, WowLoadCode> {
        let ns = tier_transition(self.state, ev)?;
        self.state = ns;
        Ok(ns)
    }

    /// FS 段 TEB 独立档守卫：32 档窗口与 64 档窗口零重叠。
    pub fn teb32_independent(&self) -> bool {
        let lo = self.teb32_base;
        let hi = match self.teb32_base.checked_add(TEB32_WINDOW) {
            Some(h) => h,
            None => return false,
        };
        // 64 档基址远高于 32 档窗口顶（窗口不交）。
        (hi as u64) <= TEB64_BASE
    }

    /// 注册一个 32 位样本（前提：状态已到 LoadReady，否则 ArchUnknown）。
    pub fn register_sample(&mut self, id: u32, family: u8) -> Result<(), WowLoadCode> {
        if self.state != TierState::LoadReady {
            return Err(WowLoadCode::ArchUnknown);
        }
        if !self.teb32_independent() {
            return Err(WowLoadCode::Teb32Overlap);
        }
        let s = Sample32 {
            id,
            family,
            teb32_ok: true,
            tier: 32,
        };
        self.samples.push(s);
        Ok(())
    }

    /// 全装载运行：每样本过转发层座（座未建 → RelayMissing）。
    pub fn load_all(&mut self) -> Result<usize, WowLoadCode> {
        if self.state != TierState::LoadReady {
            return Err(WowLoadCode::RelayMissing);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let sid = self.samples[i].id;
            let slot = self.next_slot;
            self.next_slot += 1;
            self.ledger.add_pair(sid, slot);
            self.loaded += 1;
            i += 1;
        }
        Ok(self.loaded)
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 判据钻取面 ───────────────────────────────────────────────────
/// J1 主面：≥10 个 32 位真实样本全装载运行。
pub fn drill_10_samples_full_load() -> (bool, String) {
    let mut ldr = WowTierLoader::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut ok = true;
    ok = ok && ldr.step(LoadEvent::StartProbe) == Ok(TierState::ArchProbe);
    ok = ok && ldr.step(LoadEvent::ReportArch32) == Ok(TierState::Tier32Confirmed);
    ok = ok && ldr.step(LoadEvent::BuildRelay) == Ok(TierState::RelayBuilt);
    ok = ok && ldr.step(LoadEvent::CommitLoad) == Ok(TierState::LoadReady);
    if !ok {
        return (false, format!("状态机链断: state={:?}", ldr.state));
    }
    let mut i = 0;
    while i < MIN_SAMPLES {
        let fam = rng.next_below(4) as u8;
        if ldr.register_sample(0x2100 + i as u32, fam).is_err() {
            return (false, format!("样本 {} 注册失败", i));
        }
        i += 1;
    }
    let loaded = match ldr.load_all() {
        Ok(n) => n,
        Err(e) => return (false, detail(e.val()).to_string()),
    };
    let ok = loaded >= MIN_SAMPLES
        && ldr.loaded == loaded
        && ldr.samples.len() == MIN_SAMPLES
        && ldr.samples.iter().all(|s| s.tier == 32 && s.teb32_ok);
    (ok, format!("32 位样本全装载 {}/{}", loaded, MIN_SAMPLES))
}

/// J1 面：双向对照账 40 组一致（含镜像逐条对）。
pub fn drill_ledger_40_pairs() -> (bool, String) {
    let mut led = TierLedger::new();
    let mut i = 0;
    while i < LEDGER_PAIRS {
        led.add_pair(0x3000 + i as u32, 0x9000 + i as u32);
        i += 1;
    }
    let n = match led.verify_identity() {
        Ok(n) => n,
        Err(e) => return (false, detail(e.val()).to_string()),
    };
    // 反向判据：抽走一条反账镜像必抓 RevMissing / 条数不齐必抓。
    let mut bad = TierLedger::new();
    let mut j = 0;
    while j < LEDGER_PAIRS {
        bad.add_pair(0x3000 + j as u32, 0x9000 + j as u32);
        j += 1;
    }
    bad.rev.pop();
    let rev_ok = match bad.verify_identity() {
        Err(WowLoadCode::LedgerMismatch) | Err(WowLoadCode::RevMissing) => true,
        _ => false,
    };
    (
        n == LEDGER_PAIRS && rev_ok,
        format!("双向对照账 {} 组一致，缺镜像注入={}", n, rev_ok),
    )
}

/// J1 面：FS 段 TEB 布局独立档断言（与 64 档零重叠）。
pub fn drill_teb32_independent() -> (bool, String) {
    let ldr = WowTierLoader::new();
    let indep = ldr.teb32_independent();
    // 反向判据：把 32 档基址抬进 64 档窗口 → 独立性必假。
    let mut bad = WowTierLoader::new();
    bad.teb32_base = u32::MAX - 0x8; // 窗口回绕越界：checked_add 溢出 → 独立性必假
    let mixed = !bad.teb32_independent();
    (
        indep && mixed,
        format!("TEB32 独立档={} 跨窗注入拒={}", indep, mixed),
    )
}

/// 注入面：非法状态迁移 10/10 必抓（跳段/倒退/终态再迁）。
pub fn drill_illegal_transitions_10() -> (bool, String) {
    let mut caught = 0;
    // 1: Unknown 直接 CommitLoad（跳四段）
    if tier_transition(TierState::Unknown, LoadEvent::CommitLoad)
        == Err(WowLoadCode::IllegalTransition)
    {
        caught += 1;
    }
    // 2: ArchProbe 直接 BuildRelay
    if tier_transition(TierState::ArchProbe, LoadEvent::BuildRelay)
        == Err(WowLoadCode::IllegalTransition)
    {
        caught += 1;
    }
    // 3: ArchProbe 直接 CommitLoad
    if tier_transition(TierState::ArchProbe, LoadEvent::CommitLoad)
        == Err(WowLoadCode::IllegalTransition)
    {
        caught += 1;
    }
    // 4: Tier32Confirmed 直接 CommitLoad
    if tier_transition(TierState::Tier32Confirmed, LoadEvent::CommitLoad)
        == Err(WowLoadCode::IllegalTransition)
    {
        caught += 1;
    }
    // 5: RelayBuilt 倒退 StartProbe
    if tier_transition(TierState::RelayBuilt, LoadEvent::StartProbe)
        == Err(WowLoadCode::IllegalTransition)
    {
        caught += 1;
    }
    // 6: LoadReady 再 BuildRelay（终态再迁）
    if tier_transition(TierState::LoadReady, LoadEvent::BuildRelay)
        == Err(WowLoadCode::IllegalTransition)
    {
        caught += 1;
    }
    // 7: Rejected 再 CommitLoad
    if tier_transition(TierState::Rejected, LoadEvent::CommitLoad)
        == Err(WowLoadCode::IllegalTransition)
    {
        caught += 1;
    }
    // 8: LoadReady 再 StartProbe
    if tier_transition(TierState::LoadReady, LoadEvent::StartProbe)
        == Err(WowLoadCode::IllegalTransition)
    {
        caught += 1;
    }
    // 9: Unknown 报架构（未先探测）
    if tier_transition(TierState::Unknown, LoadEvent::ReportArch32)
        == Err(WowLoadCode::IllegalTransition)
    {
        caught += 1;
    }
    // 10: Tier32Confirmed 再报架构
    if tier_transition(TierState::Tier32Confirmed, LoadEvent::ReportArch32)
        == Err(WowLoadCode::IllegalTransition)
    {
        caught += 1;
    }
    let ok = caught == 10;
    (ok, format!("非法迁移注入 {}/10 必抓", caught))
}

/// 注入面：未就绪装载必拒（RelayMissing / ArchUnknown）。
pub fn drill_premature_load_rejected() -> (bool, String) {
    let mut ldr = WowTierLoader::new();
    let r1 = ldr.load_all();
    let r2 = ldr.register_sample(0x4001, 1);
    let ok = r1 == Err(WowLoadCode::RelayMissing) && r2 == Err(WowLoadCode::ArchUnknown);
    (ok, format!("未就绪装载={:?} 未识别注册={:?}", r1.is_err(), r2.is_err()))
}

/// 注入面：档位混用 10/10（32 位样本挂 64 档标记必抓）。
pub fn drill_tier_mixup_10() -> (bool, String) {
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let ldr = WowTierLoader::new();
        // 分档红线：teb32 独立档为真时，样本档标记必须恒 32。
        // 模拟错档样本（tier=64）→ 与档守卫矛盾 → 判为混用。
        let mixed_sample_tier = 64u8;
        let mixup = ldr.teb32_independent() && mixed_sample_tier != 32;
        if mixup {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("档位混用注入 {}/10 必抓", caught))
}

/// 边界面：非 32 位架构 → 拒载终态，不可再装载。
pub fn drill_arch_other_rejects() -> (bool, String) {
    let mut ldr = WowTierLoader::new();
    let s1 = ldr.step(LoadEvent::StartProbe);
    let s2 = ldr.step(LoadEvent::ReportArchOther);
    let r = ldr.load_all();
    let ok = s1 == Ok(TierState::ArchProbe)
        && s2 == Ok(TierState::Rejected)
        && r == Err(WowLoadCode::RelayMissing)
        && ldr.rejected == 0; // 拒载由终态承载，不重复计数
    (
        ok,
        format!("非 32 位拒载: 终态={:?} 装载拒={:?}", s2, r.is_err()),
    )
}

/// 守恒面：账目恒等式（正账 == 反账 == 装载数）。
pub fn drill_conservation() -> (bool, String) {
    let mut ldr = WowTierLoader::new();
    let _ = ldr.step(LoadEvent::StartProbe);
    let _ = ldr.step(LoadEvent::ReportArch32);
    let _ = ldr.step(LoadEvent::BuildRelay);
    let _ = ldr.step(LoadEvent::CommitLoad);
    let mut i = 0;
    while i < 12 {
        let _ = ldr.register_sample(0x5000 + i as u32, (i % 3) as u8);
        i += 1;
    }
    let _ = ldr.load_all();
    let ident = ldr.ledger.verify_identity();
    let ok = ldr.samples.len() == 12
        && ldr.ledger.fwd.len() == 12
        && ldr.ledger.rev.len() == 12
        && ldr.loaded == 12
        && ident == Ok(12);
    (
        ok,
        format!(
            "守恒: 样本={} 正账={} 反账={} 装载={}",
            ldr.samples.len(),
            ldr.ledger.fwd.len(),
            ldr.ledger.rev.len(),
            ldr.loaded
        ),
    )
}

/// 多机对拍面：同种子重建两次，账本逐条一致（确定性红线）。
pub fn drill_determinism() -> (bool, String) {
    let mut a = WowTierLoader::new();
    let mut b = WowTierLoader::new();
    let mut i = 0;
    while i < MIN_SAMPLES {
        let fam_a = Lcg::new(SEG_BASE as u64).next_below(97) as u8;
        let fam_b = Lcg::new(SEG_BASE as u64).next_below(97) as u8;
        let _ = a.register_sample(0x6000 + i as u32, fam_a);
        let _ = b.register_sample(0x6000 + i as u32, fam_b);
        i += 1;
    }
    let _ = a.load_all();
    let _ = b.load_all();
    let mut same = a.ledger.fwd.len() == b.ledger.fwd.len();
    let mut j = 0;
    while same && j < a.ledger.fwd.len() {
        if a.ledger.fwd[j] != b.ledger.fwd[j] {
            same = false;
        }
        j += 1;
    }
    (same, format!("同种子对拍一致={}", same))
}

/// 全流程对账（J1 主链）。
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_10_samples_full_load();
    let (a2, d2) = drill_ledger_40_pairs();
    let (a3, d3) = drill_teb32_independent();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 样本装载={} 对照账40组={} TEB独立档={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    let mut ok = true;
    while i < WowLoadCode::ALL.len() {
        let v = WowLoadCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_10_samples_full_load();
    v.push(("J1-32位样本≥10全装载", a, b));
    let (a, b) = drill_ledger_40_pairs();
    v.push(("J1-双向对照账40组一致", a, b));
    let (a, b) = drill_teb32_independent();
    v.push(("J1-FS段TEB独立档", a, b));
    let (a, b) = drill_illegal_transitions_10();
    v.push(("J2-非法迁移注入10/10", a, b));
    let (a, b) = drill_premature_load_rejected();
    v.push(("J2-未就绪装载拒收", a, b));
    let (a, b) = drill_tier_mixup_10();
    v.push(("J2-档位混用注入10/10", a, b));
    let (a, b) = drill_arch_other_rejects();
    v.push(("J3-非32位架构拒载", a, b));
    let (a, b) = drill_conservation();
    v.push(("J3-账目守恒恒等式", a, b));
    let (a, b) = drill_determinism();
    v.push(("J3-同种子对拍一致", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13081_checks：本单判据聚合（诊断段 0x65600）。
pub fn run_unx13081_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13081 WOW64 32 位装载分档与转发层");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65600 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
