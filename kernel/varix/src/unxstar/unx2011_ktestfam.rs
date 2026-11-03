//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2011 · 耦合 ktest 族注册 · 0x72840
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B21）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F2011-J1 本批 19 条判据聚合入 ktest SMP 面新族，
//!     三档调度 intact，skip 态 100% 带因，结果账接入 20 维 SMP 维取数口；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 族注册：19 条判据逐条登记入新族（id 唯一/名非空/维覆盖非空），
//!     族状态机 Empty→Registering→Sealed 单调，封族后再注册显性拒；
//!   · 三档调度：Quick/Normal/Stress 逐档全跑、档内按 id 序，调度序与族面
//!     档位逐条对账；skip 态 100% 带因（空因即违约）；
//!   · 结果账：20 维 SMP 维格账，聚合双路独立对账，取数口越界显性拒；
//!   · 注入 10 路（重号/封后注/空因/抹维/计数篡改/越界取数/空维/空名/
//!     漏账/错挂档）10/10 必抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72840，VX3-1103 ktest 族注册窗口独占）。
pub const SEG_BASE: u32 = 0x72840;
/// 本批判据条数（冻结 19）。
pub const FAMILY_CASES: usize = 19;
/// SMP 维数（冻结 20）。
pub const SMP_DIMS: usize = 20;
/// 三档调度的档数（Quick/Normal/Stress）。
pub const TIER_N: usize = 3;
/// Quick 档案数（0..6）。
pub const TIER_QUICK_N: usize = 6;
/// Normal 档案数（6..13）。
pub const TIER_NORMAL_N: usize = 7;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入路数（冻结 10）。
pub const INJ_ROUTES: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtestCode {
    /// 案重号：族内 id 已存在，重复注册拒
    DupCase,
    /// 注册字段非法：案名为空 / 未封族先跑 / 档位错序
    FieldInvalid,
    /// skip 无因：skip 态必须携带非空原因串，空因即违约
    SkipNoReason,
    /// 维越界：案不触任何维 / 维号出 20 维面
    DimRange,
    /// 封后操作：族已封死再注册显性拒
    FamilySealed,
    /// 结果账不符：聚合双路对不上 / 维有洞 / 档位错挂
    LedgerMismatch,
    /// 取数口关闭：fetch_port 维号越出 20 维面
    PortClosed,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl KtestCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KtestCode::DupCase => "案重号：族内 id 唯一，重复注册拒，先查重再登记",
            KtestCode::FieldInvalid => "注册字段非法：案名非空、封族后才可跑、档位三档闭集",
            KtestCode::SkipNoReason => "skip 无因：skip 态 100% 带因，空因案补声明或改跑",
            KtestCode::DimRange => "维越界：每案至少触 1 维，维号落在 0..20 闭面",
            KtestCode::FamilySealed => "封后操作：族封死即冻结，再注册先解封（本版无解封口）",
            KtestCode::LedgerMismatch => "结果账不符：双路聚合对不上，逐维定位差异格",
            KtestCode::PortClosed => "取数口关闭：维号出 0..20 闭面，先查维号再取数",
            KtestCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [KtestCode; 8] = [
        KtestCode::DupCase,
        KtestCode::FieldInvalid,
        KtestCode::SkipNoReason,
        KtestCode::DimRange,
        KtestCode::FamilySealed,
        KtestCode::LedgerMismatch,
        KtestCode::PortClosed,
        KtestCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < KtestCode::ALL.len() {
        if KtestCode::ALL[i].val() == code {
            return KtestCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72840 段闭集内"
}

// ── 三档调度（Quick→Normal→Stress 单调） ────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 冒烟档：秒级，先跑先拦。
    Quick,
    /// 常规档：核心判据面。
    Normal,
    /// 压强档：长程与边界面（只压整数迭代轮次，不碰墙钟）。
    Stress,
}

impl Tier {
    /// 单调序值：Quick=0 < Normal=1 < Stress=2。
    pub fn rank(self) -> u8 {
        match self {
            Tier::Quick => 0,
            Tier::Normal => 1,
            Tier::Stress => 2,
        }
    }
    /// 档名（诊断文案用）。
    pub fn name(self) -> &'static str {
        match self {
            Tier::Quick => "冒烟",
            Tier::Normal => "常规",
            Tier::Stress => "压强",
        }
    }
}

// ── 判据案与族面 ────────────────────────────────────────────────
/// 单条判据案：id 唯一、名非空、触维面非空；skip 案带非空原因。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseSpec {
    pub id: u32,
    pub name: &'static str,
    pub tier: Tier,
    /// 触维面（20 维位图）。
    pub dims: [bool; SMP_DIMS],
    /// 非空 ⇒ 该案登记为 skip 态（必须带因）。
    pub skip_reason: &'static str,
}

/// 维面派生（确定性）：每案触 5 维，维度 = (i*3 + k*7) % 20。
/// 19 案 × 5 维 = 95 划，覆盖全部 20 维（golden_run 里显性断言）。
pub fn dims_for(i: usize) -> [bool; SMP_DIMS] {
    let mut m = [false; SMP_DIMS];
    for k in 0..5usize {
        let d = (i.wrapping_mul(3).wrapping_add(k.wrapping_mul(7))) % SMP_DIMS;
        m[d] = true;
    }
    m
}

/// 案名派生（确定性 ASCII）。
pub fn name_for(i: usize) -> &'static str {
    const NAMES: [&str; FAMILY_CASES] = [
        "k0001_sched_smoke", "k0002_wake_lat", "k0003_affi_walk", "k0004_topo_probe",
        "k0005_bal_tick", "k0006_idle_menu", "k0007_load_track", "k0008_mig_cost",
        "k0009_pull_govern", "k0010_tick_defer", "k0011_core_hier", "k0012_burst_cap",
        "k0013_slice_quant", "k0014_prio_inher", "k0015_rt_band", "k0016_util_clamp",
        "k0017_hotplug_walk", "k0018_asym_cap", "k0019_ledger_port"];

    match NAMES.get(i) {
        Some(n) => *n,
        None => "",
    }
}

/// skip 派生：下标 3/11/17 三案声明降级原因（其余跑满）。
pub fn skip_reason_for(i: usize) -> &'static str {
    match i {
        3 => "依赖硬件 SMP 拓扑表，本环境降级为声明制跳过",
        11 => "需要 4 核以上并发压床，本环境核数不足声明跳过",
        17 => "热插拔面需平台钩子，本环境无钩子声明跳过",
        _ => "",
    }
}

/// 案派生（本批 19 条判据的确定性登记源）。
pub fn build_spec(i: usize) -> CaseSpec {
    let tier = if i < TIER_QUICK_N {
        Tier::Quick
    } else if i < TIER_QUICK_N + TIER_NORMAL_N {
        Tier::Normal
    } else {
        Tier::Stress
    };
    CaseSpec {
        id: (i + 1) as u32,
        name: name_for(i),
        tier,
        dims: dims_for(i),
        skip_reason: skip_reason_for(i),
    }
}

// ── 族状态机（Empty→Registering→Sealed 单调） ───────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FamState {
    /// 空族：一案未登。
    Empty,
    /// 登记中：至少一案已登，未封。
    Registering,
    /// 已封族：案面冻结，再注册显性拒。
    Sealed,
}

impl FamState {
    /// 单调序值：Empty=0 < Registering=1 < Sealed=2。
    pub fn rank(self) -> u8 {
        match self {
            FamState::Empty => 0,
            FamState::Registering => 1,
            FamState::Sealed => 2,
        }
    }
    pub fn can_advance(self, to: FamState) -> bool {
        to.rank() > self.rank()
    }
}

/// ktest 族：案面 + 状态机。
#[derive(Clone, Debug)]
pub struct KtestFamily {
    pub state: FamState,
    pub cases: Vec<CaseSpec>,
}

impl KtestFamily {
    pub fn new() -> KtestFamily {
        KtestFamily { state: FamState::Empty, cases: Vec::new() }
    }

    /// 登记一案：封后拒 / 重号拒 / 空名拒 / 空维面拒。
    pub fn register(&mut self, c: CaseSpec) -> Result<(), u32> {
        if self.state == FamState::Sealed {
            return Err(KtestCode::FamilySealed.val());
        }
        if c.name.is_empty() {
            return Err(KtestCode::FieldInvalid.val());
        }
        if !c.dims.iter().any(|d| *d) {
            return Err(KtestCode::DimRange.val());
        }
        if self.cases.iter().any(|have| have.id == c.id) {
            return Err(KtestCode::DupCase.val());
        }
        self.cases.push(c);
        if self.state.can_advance(FamState::Registering) {
            self.state = FamState::Registering;
        }
        Ok(())
    }

    /// 封族：案面冻结。
    pub fn seal(&mut self) -> Result<(), u32> {
        if !self.state.can_advance(FamState::Sealed) {
            return Err(KtestCode::FamilySealed.val());
        }
        self.state = FamState::Sealed;
        Ok(())
    }

    /// 按 id 查案。
    pub fn find_case(&self, id: u32) -> Option<&CaseSpec> {
        self.cases.iter().find(|c| c.id == id)
    }
}

/// 建全族：19 案逐条登记 + 封族。
pub fn build_family() -> Result<KtestFamily, u32> {
    let mut f = KtestFamily::new();
    for i in 0..FAMILY_CASES {
        f.register(build_spec(i))?;
    }
    f.seal()?;
    Ok(f)
}

// ── 三档调度 ────────────────────────────────────────────────────
/// 单案执行结果：ran=false ⇒ skip（reason 必带非空）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub case_id: u32,
    /// 执行时档位序值（与族面对账锚）。
    pub tier_rank: u8,
    pub ran: bool,
    pub passed: bool,
    /// skip 原因（ran=false 时非空）。
    pub reason: String,
}

/// 三档调度单：Quick/Normal/Stress 三桶，档内按 id 序。
#[derive(Clone, Debug)]
pub struct Schedule {
    pub buckets: Vec<Vec<Outcome>>,
}

/// 执行面判据（确定性）：案名非空且触维非空即通过（健康面全绿）。
pub fn case_predicate(c: &CaseSpec) -> bool {
    !c.name.is_empty() && c.dims.iter().any(|d| *d)
}

/// 三档调度执行：封族才可跑；档位升序逐桶、桶内按 id 升序。
pub fn run_family(fam: &KtestFamily) -> Result<Schedule, u32> {
    if fam.state != FamState::Sealed {
        return Err(KtestCode::FieldInvalid.val());
    }
    let mut buckets: Vec<Vec<Outcome>> = Vec::new();
    let mut b = 0usize;
    while b < TIER_N {
        buckets.push(Vec::new());
        b += 1;
    }
    // 档内按 id 升序：先收集 id，再排序逐个出案。
    let mut ids: Vec<u32> = fam.cases.iter().map(|c| c.id).collect();
    ids.sort_unstable();
    for id in ids.iter() {
        let c = match fam.find_case(*id) {
            Some(c) => c,
            None => return Err(KtestCode::LedgerMismatch.val()),
        };
        let bucket = match buckets.get_mut(c.tier.rank() as usize) {
            Some(bkt) => bkt,
            None => return Err(KtestCode::DimRange.val()),
        };
        // skip 态：原因串必带（族面声明原样入账）；跑态全绿入账。
        let (ran, passed, reason) = if c.skip_reason.is_empty() {
            (true, case_predicate(c), String::new())
        } else {
            (false, false, String::from(c.skip_reason))
        };
        bucket.push(Outcome { case_id: c.id, tier_rank: c.tier.rank(), ran, passed, reason });
    }
    Ok(Schedule { buckets })
}

/// 调度对账：三桶档位齐、案不重不漏、skip 态 100% 带因、
/// 档位与族面逐条对得上（错挂即账不符）。
pub fn verify_schedule(fam: &KtestFamily, sched: &Schedule) -> Result<(), u32> {
    if sched.buckets.len() != TIER_N {
        return Err(KtestCode::LedgerMismatch.val());
    }
    let mut seen: Vec<u32> = Vec::new();
    for (b, bucket) in sched.buckets.iter().enumerate() {
        for o in bucket.iter() {
            let c = match fam.find_case(o.case_id) {
                Some(c) => c,
                None => return Err(KtestCode::LedgerMismatch.val()),
            };
            // 桶位 = 案档位 = 账档位，三方一致。
            if c.tier.rank() as usize != b || o.tier_rank != c.tier.rank() {
                return Err(KtestCode::LedgerMismatch.val());
            }
            if seen.contains(&o.case_id) {
                return Err(KtestCode::LedgerMismatch.val());
            }
            seen.push(o.case_id);
            if !o.ran {
                // skip 态 100% 带因；且 skip 态不得携带通过旗。
                if o.reason.is_empty() || o.passed || c.skip_reason.is_empty() {
                    return Err(KtestCode::SkipNoReason.val());
                }
            } else if !o.reason.is_empty() {
                // ran 态不得私带原因串（账面语义污染）。
                return Err(KtestCode::LedgerMismatch.val());
            }
        }
    }
    if seen.len() != fam.cases.len() {
        return Err(KtestCode::LedgerMismatch.val());
    }
    Ok(())
}

// ── 结果账（20 维 SMP 维格账） ──────────────────────────────────
/// 单维格：pass/skip/fail 三计数 + touched 触账数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DimCell {
    pub dim: usize,
    pub pass: u32,
    pub skip: u32,
    pub fail: u32,
    pub touched: u32,
}

impl DimCell {
    pub fn empty(dim: usize) -> DimCell {
        DimCell { dim, pass: 0, skip: 0, fail: 0, touched: 0 }
    }
}

/// 结果账：20 维格 + 三总计数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultLedger {
    pub cells: Vec<DimCell>,
    pub total_pass: u32,
    pub total_skip: u32,
    pub total_fail: u32,
}

/// 聚合主路：逐案逐维落格。
pub fn ledger_from(fam: &KtestFamily, sched: &Schedule) -> Result<ResultLedger, u32> {
    let mut cells: Vec<DimCell> = (0..SMP_DIMS).map(DimCell::empty).collect();
    let mut tp = 0u32;
    let mut ts = 0u32;
    let mut tf = 0u32;
    for bucket in sched.buckets.iter() {
        for o in bucket.iter() {
            let c = match fam.find_case(o.case_id) {
                Some(c) => c,
                None => return Err(KtestCode::LedgerMismatch.val()),
            };
            for (dd, hit) in c.dims.iter().enumerate() {
                if !*hit {
                    continue;
                }
                let cell = match cells.get_mut(dd) {
                    Some(cl) => cl,
                    None => return Err(KtestCode::DimRange.val()),
                };
                if o.ran {
                    if o.passed {
                        cell.pass = cell.pass.saturating_add(1);
                    } else {
                        cell.fail = cell.fail.saturating_add(1);
                    }
                } else {
                    cell.skip = cell.skip.saturating_add(1);
                }
                cell.touched = cell.touched.saturating_add(1);
            }
            if o.ran {
                if o.passed {
                    tp = tp.saturating_add(1);
                } else {
                    tf = tf.saturating_add(1);
                }
            } else {
                ts = ts.saturating_add(1);
            }
        }
    }
    Ok(ResultLedger { cells, total_pass: tp, total_skip: ts, total_fail: tf })
}

/// 聚合对路（独立复算）：逐格逐总数与主路逐项比对，任何一格不符即账破。
pub fn verify_ledger(fam: &KtestFamily, sched: &Schedule, led: &ResultLedger) -> Result<(), u32> {
    if led.cells.len() != SMP_DIMS {
        return Err(KtestCode::LedgerMismatch.val());
    }
    let expect = ledger_from(fam, sched)?;
    for (a, e) in led.cells.iter().zip(expect.cells.iter()) {
        if a != e {
            return Err(KtestCode::LedgerMismatch.val());
        }
    }
    if led.total_pass != expect.total_pass
        || led.total_skip != expect.total_skip
        || led.total_fail != expect.total_fail
    {
        return Err(KtestCode::LedgerMismatch.val());
    }
    // 维覆盖：20 维全被触（维有洞即账破）。
    for c in led.cells.iter() {
        if c.touched == 0 {
            return Err(KtestCode::LedgerMismatch.val());
        }
    }
    Ok(())
}

/// 20 维 SMP 维取数口：越界显性拒（PortClosed）。
pub fn fetch_port(led: &ResultLedger, dim: usize) -> Result<DimCell, u32> {
    if dim >= SMP_DIMS {
        return Err(KtestCode::PortClosed.val());
    }
    match led.cells.get(dim) {
        Some(c) => Ok(*c),
        None => Err(KtestCode::PortClosed.val()),
    }
}

// ── 注入器（10 路逐路失真） ─────────────────────────────────────
/// 路语义期望码（声明的唯一权威来源）。
pub fn kind_expected(kind: usize) -> u32 {
    match kind % INJ_ROUTES {
        0 => KtestCode::DupCase.val(),
        1 => KtestCode::FamilySealed.val(),
        2 => KtestCode::SkipNoReason.val(),
        3 | 4 | 8 | 9 => KtestCode::LedgerMismatch.val(),
        5 => KtestCode::PortClosed.val(),
        6 => KtestCode::DimRange.val(),
        _ => KtestCode::FieldInvalid.val(),
    }
}

/// 冻结声明表：10/10 逐路声明，注入前完备率必须 100%。
pub const DECLARED: [(&'static str, u32); INJ_ROUTES] = [
    ("k0 案重号注册", SEG_BASE + 0),
    ("k1 封族后再注册", SEG_BASE + 4),
    ("k2 skip 态抹空因", SEG_BASE + 2),
    ("k3 结果账抹维格", SEG_BASE + 5),
    ("k4 结果账计数篡改", SEG_BASE + 5),
    ("k5 取数口越界", SEG_BASE + 6),
    ("k6 空维面注册", SEG_BASE + 3),
    ("k7 空名注册", SEG_BASE + 1),
    ("k8 skip 漏账", SEG_BASE + 5),
    ("k9 档位错挂", SEG_BASE + 5),
];

/// 声明完备率：10/10 且逐路 expect 与语义期望一致、段内可寻址。
pub fn declared_complete() -> bool {
    if DECLARED.len() != INJ_ROUTES {
        return false;
    }
    for (i, p) in DECLARED.iter().enumerate() {
        let (note, e) = *p;
        if note.is_empty() || e == 0 || e != kind_expected(i) {
            return false;
        }
    }
    let window_ok = SEG_BASE + (INJ_ROUTES as u32) <= SEG_BASE + 0x40;
    window_ok
}

/// 注入第 kind 路失真，核验器必须以声明码命中。
/// 返回 Ok(true)=按声明码抓到；Ok(false)=漏抓/错抓（同判违约）。
pub fn inject_and_detect(kind: usize) -> Result<bool, u32> {
    let expect = kind_expected(kind);
    let res: Result<(), u32> = match kind % INJ_ROUTES {
        // k0 案重号注册（未封族面内登记同 id 两案）。
        0 => {
            let mut f = KtestFamily::new();
            let _ = f.register(build_spec(0));
            f.register(build_spec(0))
        }
        // k1 封族后再注册。
        1 => {
            let mut f = build_family()?;
            let mut c = build_spec(2);
            c.id = 99;
            f.register(c)
        }
        // k2 skip 态抹空因（改账后调度对账必败）。
        2 => {
            let f = build_family()?;
            let mut sched = run_family(&f)?;
            for bk in sched.buckets.iter_mut() {
                for o in bk.iter_mut() {
                    if !o.ran {
                        o.reason = String::new();
                    }
                }
            }
            verify_schedule(&f, &sched)
        }
        // k3 结果账抹维格（截尾一格）。
        3 => {
            let f = build_family()?;
            let sched = run_family(&f)?;
            let mut led = ledger_from(&f, &sched)?;
            led.cells.pop();
            verify_ledger(&f, &sched, &led)
        }
        // k4 结果账计数篡改（dim0 pass +1）。
        4 => {
            let f = build_family()?;
            let sched = run_family(&f)?;
            let mut led = ledger_from(&f, &sched)?;
            if let Some(c) = led.cells.get_mut(0) {
                c.pass = c.pass.saturating_add(1);
            }
            verify_ledger(&f, &sched, &led)
        }
        // k5 取数口越界。
        5 => {
            let f = build_family()?;
            let sched = run_family(&f)?;
            let led = ledger_from(&f, &sched)?;
            fetch_port(&led, SMP_DIMS).map(|_| ())
        }
        // k6 空维面注册。
        6 => {
            let mut f = KtestFamily::new();
            let mut c = build_spec(1);
            c.dims = [false; SMP_DIMS];
            f.register(c)
        }
        // k7 空名注册。
        7 => {
            let mut f = KtestFamily::new();
            let mut c = build_spec(1);
            c.name = "";
            f.register(c)
        }
        // k8 skip 漏账（skip 案首触维 skip/touched 双减）。
        8 => {
            let f = build_family()?;
            let sched = run_family(&f)?;
            let mut led = ledger_from(&f, &sched)?;
            let dim9 = 3usize.wrapping_mul(3) % SMP_DIMS; // 案 i=3 首触维
            if let Some(c) = led.cells.get_mut(dim9) {
                c.skip = c.skip.saturating_sub(1);
                c.touched = c.touched.saturating_sub(1);
            }
            verify_ledger(&f, &sched, &led)
        }
        // k9 档位错挂（Stress 案塞进冒烟桶）。
        _ => {
            let f = build_family()?;
            let mut sched = run_family(&f)?;
            let mut moved: Option<Outcome> = None;
            if let Some(bk) = sched.buckets.get_mut(2) {
                moved = bk.pop();
            }
            if let Some(mut o) = moved {
                o.tier_rank = 0;
                if let Some(bk) = sched.buckets.get_mut(0) {
                    bk.push(o);
                }
            }
            verify_schedule(&f, &sched)
        }
    };
    Ok(match res {
        Err(e) => e == expect,
        Ok(()) => false,
    })
}

// ── 注入账 ──────────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓/错抓的路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入轮：每轮 10 路逐路注入并核验，轮数 = JUDGE_ROUNDS（10/10）。
/// 首轮先跑健康族面自证核验器不误伤。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let fam = build_family()?;
    let sched = run_family(&fam)?;
    verify_schedule(&fam, &sched)?;
    let led = ledger_from(&fam, &sched)?;
    verify_ledger(&fam, &sched, &led)?;
    let mut acc = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let hit = inject_and_detect((r as usize) % INJ_ROUTES)?;
        acc.record((r as usize) % INJ_ROUTES, hit);
        r += 1;
    }
    Ok(acc)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：建族封族 → 三档调度 → 调度对账 → 结果账双路对账 →
/// 20 维取数口全通 → 10/10 注入 → 四拒探。
/// 返回 (正旗数, 拒旗数, 维覆盖数, skip 案数)。
pub fn golden_run() -> Result<(u32, u32, usize, u32), u32> {
    let fam = build_family()?;
    if fam.cases.len() != FAMILY_CASES || fam.state != FamState::Sealed {
        return Err(KtestCode::LedgerMismatch.val());
    }
    let sched = run_family(&fam)?;
    verify_schedule(&fam, &sched)?;
    let led = ledger_from(&fam, &sched)?;
    verify_ledger(&fam, &sched, &led)?;
    // 20 维取数口全通 + 覆盖清点。
    let mut covered = 0usize;
    for d in 0..SMP_DIMS {
        if fetch_port(&led, d)?.touched > 0 {
            covered += 1;
        }
    }
    if covered != SMP_DIMS {
        return Err(KtestCode::LedgerMismatch.val());
    }
    // skip 案清点：3 案带因（skip 态 100% 带因在 verify_schedule 已断言）。
    let skips = sched.buckets.iter().flatten().filter(|o| !o.ran).count() as u32;
    // 10/10 注入全抓。
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() {
        return Err(KtestCode::LedgerMismatch.val());
    }
    let flags = 1
        + (sched.buckets.len() == TIER_N) as u32
        + (covered == SMP_DIMS) as u32
        + (inj.caught == JUDGE_ROUNDS) as u32;
    // 四拒探：重号 / 封后注 / 空维面 / 越界取数。
    let mut f2 = build_family()?;
    let rej_dup = f2.register(build_spec(0)).is_err();
    let mut c99 = build_spec(2);
    c99.id = 99;
    let rej_sealed = f2.register(c99).is_err();
    let mut f3 = KtestFamily::new();
    let mut c0 = build_spec(1);
    c0.dims = [false; SMP_DIMS];
    let rej_dim = f3.register(c0).is_err();
    let rej_port = fetch_port(&led, SMP_DIMS).is_err();
    let rejects = rej_dup as u32 + rej_sealed as u32 + rej_dim as u32 + rej_port as u32;
    Ok((flags, rejects, covered, skips))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, _)) => Ok(()),
        _ => Err(KtestCode::NotReproducible.val()),
    }
}

/// 序 B：失真注入 10 轮全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(KtestCode::LedgerMismatch.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72840
        && KtestCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = FAMILY_CASES == 19 && SMP_DIMS == 20 && TIER_N == 3
        && TIER_QUICK_N == 6 && TIER_NORMAL_N == 7
        && TIER_QUICK_N + TIER_NORMAL_N < FAMILY_CASES
        && JUDGE_ROUNDS == 10 && INJ_ROUTES == 10 && in_window;
    if ok {
        Ok(())
    } else {
        Err(KtestCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(KtestCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < KtestCode::ALL.len() {
        let v = KtestCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let fam = build_family()?;
    let sched = run_family(&fam)?;
    verify_schedule(&fam, &sched)?;
    let led = ledger_from(&fam, &sched)?;
    verify_ledger(&fam, &sched, &led)?;
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() || !declared_complete() {
        return Err(KtestCode::LedgerMismatch.val());
    }
    Ok(format!(
        "状态=已完成 cases={} tiers={} dims={} skips={} caught={}/{}",
        fam.cases.len(), TIER_N, SMP_DIMS, led.total_skip, inj.caught, inj.tries
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 族建面 + 三档调度 + 结果账 + 取数口 + skip 带因 + 注入 + 声明。
    let core = golden_run();
    let (gp, gd) = match &core {
        Ok((f, r, cov, sk)) => (
            *f == 4 && *r == 4,
            format!("flags={} rejects={} covered={} skips={}", f, r, cov, sk),
        ),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链(建面/调度/账/口)", gp, gd));
    // J1 三档调度 intact：三桶桶宽 6/7/6 且档名三档闭集。
    let sched = match &core {
        Ok(_) => build_family().and_then(|f| run_family(&f)),
        Err(_) => Err(KtestCode::FieldInvalid.val()),
    };
    let (sp, sd) = match &sched {
        Ok(s) => {
            let q = s.buckets.first().map(|b| b.len()).unwrap_or(0);
            let n = s.buckets.get(1).map(|b| b.len()).unwrap_or(0);
            let st = s.buckets.get(2).map(|b| b.len()).unwrap_or(0);
            let names_ok = Tier::rank(Tier::Quick) < Tier::rank(Tier::Normal)
                && Tier::rank(Tier::Normal) < Tier::rank(Tier::Stress);
            (
                q == TIER_QUICK_N && n == TIER_NORMAL_N
                    && st == FAMILY_CASES - TIER_QUICK_N - TIER_NORMAL_N
                    && names_ok,
                format!("quick={}({}) normal={}({}) stress={}({})",
                    q, Tier::name(Tier::Quick), n, Tier::name(Tier::Normal), st, Tier::name(Tier::Stress)),
            )
        }
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-三档调度intact", sp, sd));
    // J1 skip 态 100% 带因（3 案逐一清点）。
    let (kp, kd) = match &sched {
        Ok(s) => {
            let total = s.buckets.iter().flatten().filter(|o| !o.ran).count();
            let with_reason = s
                .buckets
                .iter()
                .flatten()
                .filter(|o| !o.ran && !o.reason.is_empty())
                .count();
            (
                total > 0 && with_reason == total,
                format!("skip={} 带因={}", total, with_reason),
            )
        }
        Err(_) => (false, String::from("调度不可用")),
    };
    v.push(("J1-skip态100%带因", kp, kd));
    // J1 注入 10/10 全抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-注入10/10必抓", ip, id));
    // J1 声明完备率 100%。
    v.push((
        "J1-声明完备率100%",
        declared_complete(),
        format!("declared={}/{}", INJ_ROUTES, INJ_ROUTES),
    ));
    // J2 族状态机单调：平移拒 + 倒退拒 + 封后注拒。
    let mut f2 = KtestFamily::new();
    let m1 = f2.seal().is_ok(); // Empty→Sealed 允许（空族直封）
    let m2 = f2.register(build_spec(0)).is_err();
    let mut f3 = KtestFamily::new();
    let _ = f3.register(build_spec(0));
    let m3 = f3.state == FamState::Registering;
    let _ = f3.seal();
    let m4 = f3.register(build_spec(1)).is_err();
    v.push((
        "J2-族状态机单调",
        m1 && m2 && m3 && m4,
        format!("空封{} 封后注拒{} 登记态{} 封后注拒2{}", m1, m2, m3, m4),
    ));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72840 窗口")));
    v
}

pub fn run_unx2011_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2011 耦合 ktest 族注册");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
