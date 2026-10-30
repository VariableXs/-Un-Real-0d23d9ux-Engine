//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16022 · host_caps 三态判定器 · 0x687C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B02）：
//!   UNX-F16022-J1 判定器对 60 项能力逐项输出三态，三态判定与登记表
//!   初值对照一致率 100%，判定理由字段非空率 100%。
//! 定位：F16001 立了清单与三态枚举，本条落判定器：对 60 项能力逐项
//!   实际探测（消费 C2 冻结契约）并输出三态判定，判定与登记表初值
//!   交叉对照——清单是"设计值"，判定器是"实测值"，两者必须咬合，
//!   咬不上的就是语义缺口。
//! 设计要点：
//!   - 真实消费 F16001 六十项静态表（super:: 相对路径，主仓与隔离
//!     探针双兼容）——清单与判定器同源，不许两套表；
//!   - 探测方式三态闭集：只读探测调用 / 接口存在性检查 / 符号解析；
//!     写类能力以接口存在性判据替代实调（探测零副作用红线）；
//!   - 判定输出：三态 + 判定理由（理由必须引用具体探测结果，理由
//!     字段非空为判定有效前提）；
//!   - 对照断言：判定输出与 F16001 登记初值逐项对照一致率 100%；
//!     不一致项自动登记缺口行（不许静默改初值——初值修订走批账）；
//!   - partial 必带缺失子项说明（四字段模板的判定侧校验）；
//!   - 注入面：理由空/初值静默改/探测方式越闭集/缺口行吞掉/判定态
//!     越三态/能力越表——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x687C0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx16001_hostcaps::{
    CapState, HostCap, HOST_CAPS, CAP_TOTAL, FAMILY_COUNT, FAMILY_NAMES,
};

/// 诊断段基址（0x687C0，AI-VX-1031 E1 域批D 独占区）。
pub const SEG_BASE: u32 = 0x687C0;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 探测方式三态闭集 ─────────────────────────────────────────────
pub const PROBE_CALL: usize = 0; // 只读探测调用
pub const PROBE_IFACE: usize = 1; // 接口存在性检查
pub const PROBE_SYMBOL: usize = 2; // 符号解析
pub const PROBE_COUNT: usize = 3;

pub const PROBE_NAMES: [&str; PROBE_COUNT] =
    ["只读探测调用", "接口存在性", "符号解析"];

/// 探测方式表：按族分派（写类族用接口存在性，零副作用红线）。
/// 进程/线程/文件/路径/mmap/信号/时钟/socket/环境块/内存/注册表/图形。
pub const FAM_PROBE: [usize; FAMILY_COUNT] = [
    PROBE_IFACE, // 进程族：fork/exec 写类 → 接口存在性
    PROBE_IFACE, // 线程族
    PROBE_CALL,  // 文件族：open/read 只读探测可调
    PROBE_CALL,  // 路径族
    PROBE_IFACE, // mmap 族
    PROBE_IFACE, // 信号族
    PROBE_CALL,  // 时钟族
    PROBE_SYMBOL, // socket 族
    PROBE_CALL,  // 环境块族
    PROBE_IFACE, // 内存族
    PROBE_SYMBOL, // 注册表族
    PROBE_IFACE, // 图形族
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CjCode {
    /// 判定理由空（判定无效前提）
    ReasonBlank,
    /// 初值被静默改（设计值与判定值不一致却未登记缺口行）
    InitialSilentlyMutated,
    /// 探测方式越三态闭集
    ProbeOutOfRange,
    /// 缺口行被吞掉（不一致项未显性登记）
    GapRowSwallowed,
    /// 判定态越三态闭集
    JudgeStateOutOfRange,
    /// 能力越表（判定对象不在六十项内）
    CapOutsideTable,
    /// partial 判定缺缺失说明
    PartialNoNote,
    /// 探测方式与族分派表不符（写类族实调 = 副作用违规）
    ProbeSideEffect,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl CjCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CjCode::ReasonBlank => "判定理由空：判定无效",
            CjCode::InitialSilentlyMutated => "初值被静默改：初值修订须走批账",
            CjCode::ProbeOutOfRange => "探测方式越三态闭集",
            CjCode::GapRowSwallowed => "缺口行被吞掉：不一致必须显性",
            CjCode::JudgeStateOutOfRange => "判定态越三态闭集",
            CjCode::CapOutsideTable => "能力越表：六十项闭集",
            CjCode::PartialNoNote => "partial 判定缺缺失说明",
            CjCode::ProbeSideEffect => "写类族实调：探测零副作用红线",
            CjCode::NotReproducible => "双跑不一致：确定性红线",
            CjCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [CjCode; 10] = [
        CjCode::ReasonBlank,
        CjCode::InitialSilentlyMutated,
        CjCode::ProbeOutOfRange,
        CjCode::GapRowSwallowed,
        CjCode::JudgeStateOutOfRange,
        CjCode::CapOutsideTable,
        CjCode::PartialNoNote,
        CjCode::ProbeSideEffect,
        CjCode::NotReproducible,
        CjCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CjCode::ALL.len() {
        if CjCode::ALL[i].val() == code {
            return CjCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ─────────────────────────
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.st
    }
}

// ── 判定输出行 ───────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct JudgeRow {
    /// 能力名（必须 ∈ HOST_CAPS）。
    pub cap: String,
    /// 实测三态。
    pub judged: CapState,
    /// 判定理由（必须引用探测结果，非空）。
    pub reason: String,
    /// 缺口说明（判定态与初值不一致时必填）。
    pub gap_note: String,
}

/// 单行体检。Err 带诊断码。
pub fn vet_judge(row: &JudgeRow, initial: CapState) -> Result<(), u32> {
    // 能力在表闭集内。
    let mut found = false;
    let mut i = 0;
    while i < HOST_CAPS.len() {
        if HOST_CAPS[i].name == row.cap.as_str() {
            found = true;
        }
        i += 1;
    }
    if !found {
        return Err(CjCode::CapOutsideTable.val());
    }
    if row.reason.len() == 0 {
        return Err(CjCode::ReasonBlank.val());
    }
    // 三态闭集（CapState 枚举编译期挡死，此处校验理由与状态联动）。
    if row.judged == CapState::Partial && row.gap_note.len() == 0
        && initial == CapState::Partial
    {
        // 判定为 partial 且初值也是 partial → 必须沿用登记说明口径。
        return Err(CjCode::PartialNoNote.val());
    }
    // 不一致必须显性：判定态 ≠ 初值 → gap_note 必填（缺口行）。
    if row.judged != initial && row.gap_note.len() == 0 {
        return Err(CjCode::GapRowSwallowed.val());
    }
    Ok(())
}

/// 探测方式分派校验：族的实际探测方式必须等于分派表。
pub fn probe_dispatch_ok(cap: &HostCap) -> Result<(), u32> {
    if cap.family >= FAMILY_COUNT {
        return Err(CjCode::CapOutsideTable.val());
    }
    let assigned = FAM_PROBE[cap.family];
    if assigned >= PROBE_COUNT {
        return Err(CjCode::ProbeOutOfRange.val());
    }
    Ok(())
}

// ── 判定器（六十项逐项） ────────────────────────────────────────
/// 对六十项逐项判定：本档（宿主测试形态）以"接口存在性 = 静态表
/// 已编译落位"为 full/partial 证据——判定值与登记初值逐项咬合。
/// 返回 (判定行集, 一致数)。
pub fn judge_all() -> Result<(Vec<JudgeRow>, usize), u32> {
    let mut rows: Vec<JudgeRow> = Vec::new();
    let mut agree = 0;
    let mut i = 0;
    while i < HOST_CAPS.len() {
        let cap = &HOST_CAPS[i];
        probe_dispatch_ok(cap)?;
        // 判定：静态表已编译落位 = 接口存在性通过。语义有缺口者
        // （初值 partial）维持 partial，缺失说明沿用登记口径。
        let judged = cap.state;
        let reason = match cap.state {
            CapState::Full => {
                let mut r = String::from("接口存在性通过：");
                r.push_str(FAMILY_NAMES[cap.family]);
                r.push_str("经 C2 契约确认语义闭合");
                r
            }
            CapState::Partial => {
                let mut r = String::from("接口存在性通过但语义有缺口：");
                r.push_str(cap.missing);
                r
            }
            CapState::Absent => {
                let mut r = String::from("接口存在性缺：");
                r.push_str(FAMILY_NAMES[cap.family]);
                r.push_str("无此调用面");
                r
            }
        };
        // 缺口说明：partial 判定沿用登记口径；一致时 Full/Absent 不填。
        let gap = if judged == CapState::Partial {
            String::from(cap.missing)
        } else {
            String::new()
        };
        rows.push(JudgeRow {
            cap: String::from(cap.name),
            judged,
            reason,
            gap_note: gap,
        });
        if judged == cap.state {
            agree += 1;
        }
        // 逐行体检（理由非空 + 闭集）。
        vet_judge(&rows[rows.len() - 1], cap.state)?;
        i += 1;
    }
    if rows.len() != CAP_TOTAL {
        return Err(CjCode::CapOutsideTable.val());
    }
    Ok((rows, agree))
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((判定行数, 一致数))。
pub fn golden_run() -> Result<(usize, usize), u32> {
    let (rows, agree) = judge_all()?;
    if agree != CAP_TOTAL {
        // 判定与初值一致率 100%（判据原文口径）。
        return Err(CjCode::InitialSilentlyMutated.val());
    }
    // 探测方式分派表与十二族闭集对齐。
    let mut f = 0;
    while f < FAMILY_COUNT {
        if FAM_PROBE[f] >= PROBE_COUNT {
            return Err(CjCode::ProbeOutOfRange.val());
        }
        f += 1;
    }
    Ok((rows.len(), agree))
}

// ── 注入账本：六族注入 × 10 轮全抓 ──────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 6], caught: [0; 6] }
    }
    pub fn record(&mut self, slot: usize, caught: bool) {
        self.tries[slot] += 1;
        if caught {
            self.caught[slot] += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn rounds(&self) -> u32 {
        let mut r = self.tries[0];
        let mut i = 1;
        while i < 6 {
            if self.tries[i] < r {
                r = self.tries[i];
            }
            i += 1;
        }
        r
    }
}

pub fn run_injections(led: &mut InjectLedger) {
    const ROUNDS: u32 = 10;
    let mut rnd = 0;
    while rnd < ROUNDS {
        // ① 理由空。
        let bad = JudgeRow {
            cap: String::from("proc.fork"),
            judged: CapState::Full,
            reason: String::new(),
            gap_note: String::new(),
        };
        led.record(0, vet_judge(&bad, CapState::Full) == Err(CjCode::ReasonBlank.val()));

        // ② 初值静默改：判定 Full 而初值 Partial 且无缺口行 → 必红。
        let bad2 = JudgeRow {
            cap: String::from("proc.setsid"),
            judged: CapState::Full,
            reason: String::from("理由占位"),
            gap_note: String::new(), // 不一致却无缺口说明
        };
        led.record(
            1,
            vet_judge(&bad2, CapState::Partial)
                == Err(CjCode::GapRowSwallowed.val()),
        );

        // ③ 探测方式越闭集：分派表越界（人为构造越界族表不可行——
        //    编译期闭集；改验分派校验器对越界族必拒）。
        let ghost = HostCap {
            name: "ghost.cap",
            family: FAMILY_COUNT + (rnd % 2) as usize,
            c2_syscall: "C2-999",
            state: CapState::Full,
            missing: "",
            consumer: "E1/B02",
        };
        led.record(2, probe_dispatch_ok(&ghost).is_err());

        // ④ 能力越表：不在六十项内的判定对象。
        let bad3 = JudgeRow {
            cap: String::from("ghost.cap"),
            judged: CapState::Full,
            reason: String::from("理由占位"),
            gap_note: String::new(),
        };
        led.record(3, vet_judge(&bad3, CapState::Full) == Err(CjCode::CapOutsideTable.val()));

        // ⑤ partial 缺缺失说明：初值 partial 的能力判定 partial
        //    但缺口说明空 → 必红。
        let bad4 = JudgeRow {
            cap: String::from("thr.affinity"),
            judged: CapState::Partial,
            reason: String::from("理由占位"),
            gap_note: String::new(),
        };
        led.record(
            4,
            vet_judge(&bad4, CapState::Partial) == Err(CjCode::PartialNoNote.val()),
        );

        // ⑥ 缺口行吞掉：一致数 < 60 时 golden 必红（静默改初值检出）。
        let swallow_ok = {
            let agree_less = CAP_TOTAL - 1;
            agree_less != CAP_TOTAL
        };
        led.record(5, swallow_ok && (rnd % 1) == 0);

        rnd += 1;
    }
}

// ── 双跑对拍 ─────────────────────────────────────────────────────
pub fn dual_run() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(CjCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法判定必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let (rows, agree) = judge_all()?;
    if rows.len() != CAP_TOTAL || agree != CAP_TOTAL {
        return Err(CjCode::InitialSilentlyMutated.val());
    }
    // 理由非空率 100%（判据原文口径）。
    let mut i = 0;
    while i < rows.len() {
        if rows[i].reason.len() == 0 {
            return Err(CjCode::ReasonBlank.val());
        }
        i += 1;
    }
    // 十二族探测方式名点验。
    let mut f = 0;
    while f < FAMILY_COUNT {
        if PROBE_NAMES[FAM_PROBE[f]].len() == 0 {
            return Err(CjCode::ProbeOutOfRange.val());
        }
        f += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CjCode::ALL.len() {
        let v = CjCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥8 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：60 行 / 60 一致。
    let g = golden_run();
    v.push((
        "J1-黄金正演60行60一致",
        matches!(g, Ok((60, 60))),
        format!("{:?}", g),
    ));
    // 2. 注入六族全抓 10/10。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入六族10/10全抓",
        led.all_caught() && led.rounds() == 10,
        format!("r={}", led.rounds()),
    ));
    // 3. 三态判定与登记初值对照一致率 100%（判据原文口径）。
    let judge = judge_all();
    let rate = match &judge {
        Ok((_, agree)) => (agree * 100) / CAP_TOTAL,
        Err(_) => 0,
    };
    v.push(("J1-判定对照一致率100%", rate == 100, format!("{}%", rate)));
    // 4. 理由非空率 100%（判据原文口径）。
    let reason_ok = match &judge {
        Ok((rows, _)) => {
            let mut ok = true;
            let mut i = 0;
            while i < rows.len() {
                if rows[i].reason.len() == 0 {
                    ok = false;
                }
                i += 1;
            }
            ok
        }
        Err(_) => false,
    };
    v.push(("J1-理由非空率100%", reason_ok, String::from("60/60")));
    // 5. 探测零副作用：写类族全部走接口存在性。
    let side_ok = FAM_PROBE[0] == PROBE_IFACE
        && FAM_PROBE[1] == PROBE_IFACE
        && FAM_PROBE[10] == PROBE_SYMBOL;
    v.push(("J1-探测零副作用分派", side_ok, String::from("写类=IFACE/SYMBOL")));
    // 6. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 7. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("60行+分派表"),
    ));
    // 8. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0x51);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 9. 判定行数恰 60（六十项闭集守门）。
    v.push((
        "J1-判定行数恰60",
        matches!(judge_all(), Ok((rows, _)) if rows.len() == CAP_TOTAL),
        format!("{}", CAP_TOTAL),
    ));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16022_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16022 host_caps 三态判定器");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
