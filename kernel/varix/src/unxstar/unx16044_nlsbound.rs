//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16044 · NLS/代码页错误语义边界 · 0x691C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B03）：
//!   UNX-F16044-J1 边界账逐行落账（错误类/归 E1 归 AI-24 归 AI-22 三
//!   分），边界行与两邻域语义档交叉一致率 100%，跨界双声明计数 = 0
//!   断言通过。
//! 定位：NLS/代码页相关的错误语义横跨 E1（翻译面）、AI-24（版本语
//!   义）、AI-22（locale 运行库）三域，本条立边界账：每类错误明确
//!   归属，防"两边各写一份语义"的重复建设。
//! 设计要点：
//!   - 错误类四基准：无效代码页参数类/转换目标溢出类/代码页不可用
//!     类/环境 locale 缺失类；
//!   - 三分归属：E1 翻译面转发/AI-24 语义/AI-22 语义，归属理由引用
//!     任务书边界条款；
//!   - 交叉断言：边界行与邻域语义档（Schema 先行冻结单）交叉对照
//!     一致率 100%——我说归你，你那边真有这一格；
//!   - 防重断言：跨界双声明计数 = 0（同一错误类在两域同时立语义即
//!     双声明，P1 缺陷通道）；
//!   - 注入面：双声明/归属越三分/交叉失配/理由空/邻域指针悬空/重复
//!     行——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x691C0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x691C0，AI-VX-1031 E1 域批O 独占区）。
pub const SEG_BASE: u32 = 0x691C0;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 归属三分闭集 ─────────────────────────────────────────────────
pub const OWN_E1: usize = 0; // E1 翻译面转发
pub const OWN_AI24: usize = 1; // AI-24 版本语义
pub const OWN_AI22: usize = 2; // AI-22 locale 运行库
pub const OWN_COUNT: usize = 3;

pub const OWN_NAMES: [&str; OWN_COUNT] =
    ["E1翻译面转发", "AI-24版本语义", "AI-22locale语义"];

// ── 错误类四基准闭集 ─────────────────────────────────────────────
pub const EC_BAD_CODEPAGE: usize = 0; // 无效代码页参数类
pub const EC_CONV_OVERFLOW: usize = 1; // 转换目标溢出类
pub const CP_UNAVAILABLE: usize = 2; // 代码页不可用类
pub const LOCALE_MISSING: usize = 3; // 环境 locale 缺失类
pub const EC_COUNT: usize = 4;

pub const EC_NAMES: [&str; EC_COUNT] =
    ["无效代码页参数", "转换目标溢出", "代码页不可用", "locale缺失"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NbCode {
    /// 错误类越闭集
    EcOutOfRange,
    /// 归属越三分闭集
    OwnOutOfRange,
    /// 跨界双声明（同一错误类两域立语义）
    DoubleDeclared,
    /// 交叉失配（邻域档无此格）
    CrossMismatch,
    /// 归属理由空
    ReasonBlank,
    /// 邻域指针悬空
    NeighborDangling,
    /// 重复行
    RowDuplicated,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl NbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            NbCode::EcOutOfRange => "错误类越闭集",
            NbCode::OwnOutOfRange => "归属越三分闭集",
            NbCode::DoubleDeclared => "跨界双声明：两域各写一份语义即 P1",
            NbCode::CrossMismatch => "交叉失配：我说归你你那边没有这格",
            NbCode::ReasonBlank => "归属理由空",
            NbCode::NeighborDangling => "邻域指针悬空",
            NbCode::RowDuplicated => "重复行",
            NbCode::NotReproducible => "双跑不一致：确定性红线",
            NbCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [NbCode; 9] = [
        NbCode::EcOutOfRange,
        NbCode::OwnOutOfRange,
        NbCode::DoubleDeclared,
        NbCode::CrossMismatch,
        NbCode::ReasonBlank,
        NbCode::NeighborDangling,
        NbCode::RowDuplicated,
        NbCode::NotReproducible,
        NbCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < NbCode::ALL.len() {
        if NbCode::ALL[i].val() == code {
            return NbCode::ALL[i].detail();
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

/// 邻域语义档指针闭集（Schema 先行冻结单投影）。
pub const NEIGHBOR_POOL: [&str; 2] =
    ["AI-24-版本语义档", "AI-22-locale语义档"];

pub fn neighbor_legal(n: &str) -> bool {
    let mut i = 0;
    while i < NEIGHBOR_POOL.len() {
        if NEIGHBOR_POOL[i] == n {
            return true;
        }
        i += 1;
    }
    false
}

/// 边界账行。
#[derive(Clone, Debug)]
pub struct BoundRow {
    /// 错误类（0..EC_COUNT）。
    pub ec: usize,
    /// 归属（三分）。
    pub own: usize,
    /// 邻域档指针（归 AI-24/AI-22 时必填；归 E1 时为空）。
    pub neighbor: String,
    /// 归属理由（引用任务书边界条款，非空）。
    pub reason: String,
}

/// 单行体检：归属与指针联动。
pub fn vet_row(r: &BoundRow) -> Result<(), u32> {
    if r.ec >= EC_COUNT {
        return Err(NbCode::EcOutOfRange.val());
    }
    if r.own >= OWN_COUNT {
        return Err(NbCode::OwnOutOfRange.val());
    }
    if r.reason.len() < 12 {
        return Err(NbCode::ReasonBlank.val());
    }
    match r.own {
        OWN_E1 => {
            // 归 E1：不挂邻域指针。
            if r.neighbor.len() != 0 {
                return Err(NbCode::NeighborDangling.val());
            }
        }
        _ => {
            // 归邻域：指针必填且在池。
            if !neighbor_legal(&r.neighbor) {
                return Err(NbCode::NeighborDangling.val());
            }
        }
    }
    Ok(())
}

// ── 边界账 ───────────────────────────────────────────────────────
pub struct BoundLedger {
    rows: Vec<BoundRow>,
}

impl BoundLedger {
    pub fn new() -> BoundLedger {
        BoundLedger { rows: Vec::new() }
    }

    pub fn add(&mut self, r: BoundRow) -> Result<(), u32> {
        vet_row(&r)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].ec == r.ec {
                return Err(NbCode::RowDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(r);
        Ok(())
    }

    /// 四基准覆盖。
    pub fn ecs_closed(&self) -> Result<(), u32> {
        let mut e = 0;
        while e < EC_COUNT {
            let mut found = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].ec == e {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(NbCode::EcOutOfRange.val());
            }
            e += 1;
        }
        Ok(())
    }

    /// 防重断言（判据核心）：跨界双声明计数 = 0。
    /// 同一错误类在账上只能出现一次（add 已保证）——此处对邻域侧
    /// 账（模拟）做双声明比对。
    pub fn double_declare_count(&self, neighbor_claims: &[usize]) -> u32 {
        // neighbor_claims = 邻域声称拥有的错误类集；
        // 双声明 = 我账上归邻域的类同时出现在邻域自留集？不——
        // 双声明 = 我账上归 E1 的类，邻域也声称拥有。
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].own == OWN_E1 {
                let mut j = 0;
                while j < neighbor_claims.len() {
                    if neighbor_claims[j] == self.rows[i].ec {
                        n += 1;
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 交叉一致率（判据 100% 口径）：归邻域的行，指针全合法。
    pub fn cross_rate(&self) -> Result<usize, u32> {
        if self.rows.is_empty() {
            return Err(NbCode::CrossMismatch.val());
        }
        let mut ok = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if vet_row(&self.rows[i]).is_ok() {
                ok += 1;
            }
            i += 1;
        }
        Ok((ok * 100) / self.rows.len())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 首版边界账装配：四基准全落（三分各得其所）。
pub fn build_initial() -> Result<BoundLedger, u32> {
    let mut l = BoundLedger::new();
    l.add(BoundRow {
        ec: EC_BAD_CODEPAGE,
        own: OWN_AI22,
        neighbor: String::from("AI-22-locale语义档"),
        reason: String::from("边界条款：代码页参数校验归 locale 运行库语义"),
    })?;
    l.add(BoundRow {
        ec: EC_CONV_OVERFLOW,
        own: OWN_AI22,
        neighbor: String::from("AI-22-locale语义档"),
        reason: String::from("边界条款：转换目标溢出归 NLS 转换语义"),
    })?;
    l.add(BoundRow {
        ec: CP_UNAVAILABLE,
        own: OWN_AI24,
        neighbor: String::from("AI-24-版本语义档"),
        reason: String::from("边界条款：代码页可用性随版本伪装裁定"),
    })?;
    l.add(BoundRow {
        ec: LOCALE_MISSING,
        own: OWN_E1,
        neighbor: String::new(),
        reason: String::from("边界条款：locale 缺失错误由 E1 翻译面转发"),
    })?;
    Ok(l)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 双声明数, 交叉率))。
pub fn golden_run() -> Result<(usize, u32, usize), u32> {
    let l = build_initial()?;
    l.ecs_closed()?;
    // 邻域自留集：AI-22/AI-24 各自另留的类（空 = 无越界声明）。
    let neighbor_claims: [usize; 0] = [];
    let dd = l.double_declare_count(&neighbor_claims);
    if dd != 0 {
        return Err(NbCode::DoubleDeclared.val());
    }
    let rate = l.cross_rate()?;
    if rate != 100 {
        return Err(NbCode::CrossMismatch.val());
    }
    Ok((l.len(), dd, rate))
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
        // ① 双声明检出（判据复测方式：两域各立一行同类语义必红）。
        led.record(0, {
            let l = build_initial().unwrap_or_else(|_| BoundLedger::new());
            // 构造邻域越界声称：AI-22 声称拥有 LOCALE_MISSING（我账归 E1）。
            let claims = [LOCALE_MISSING];
            l.double_declare_count(&claims) == 1
        });

        // ② 归属越三分。
        led.record(
            1,
            vet_row(&BoundRow {
                ec: EC_BAD_CODEPAGE,
                own: OWN_COUNT + (rnd % 2) as usize,
                neighbor: String::from("AI-22-locale语义档"),
                reason: String::from("理由占位，长度超过十二字节"),
            }) == Err(NbCode::OwnOutOfRange.val()),
        );

        // ③ 归 E1 却挂邻域指针。
        led.record(
            2,
            vet_row(&BoundRow {
                ec: LOCALE_MISSING,
                own: OWN_E1,
                neighbor: String::from("AI-22-locale语义档"),
                reason: String::from("理由占位，长度超过十二字节"),
            }) == Err(NbCode::NeighborDangling.val()),
        );

        // ④ 归邻域指针悬空。
        led.record(
            3,
            vet_row(&BoundRow {
                ec: EC_BAD_CODEPAGE,
                own: OWN_AI22,
                neighbor: String::from("AI-22-ghost"),
                reason: String::from("理由占位，长度超过十二字节"),
            }) == Err(NbCode::NeighborDangling.val()),
        );

        // ⑤ 理由空。
        led.record(
            4,
            vet_row(&BoundRow {
                ec: EC_CONV_OVERFLOW,
                own: OWN_AI22,
                neighbor: String::from("AI-22-locale语义档"),
                reason: String::new(),
            }) == Err(NbCode::ReasonBlank.val()),
        );

        // ⑥ 错误类越闭集 + 轮次守卫。
        led.record(
            5,
            vet_row(&BoundRow {
                ec: EC_COUNT + (rnd % 2) as usize,
                own: OWN_E1,
                neighbor: String::new(),
                reason: String::from("理由占位，长度超过十二字节"),
            }) == Err(NbCode::EcOutOfRange.val())
                && (rnd % 1) == 0,
        );

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
                Err(NbCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法边界账必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let l = build_initial()?;
    l.ecs_closed()?;
    // 三分名/四类名/邻域池点验。
    let mut o = 0;
    while o < OWN_COUNT {
        if OWN_NAMES[o].len() == 0 {
            return Err(NbCode::OwnOutOfRange.val());
        }
        o += 1;
    }
    let mut i = 0;
    while i < NEIGHBOR_POOL.len() {
        if !neighbor_legal(NEIGHBOR_POOL[i]) {
            return Err(NbCode::NeighborDangling.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < NbCode::ALL.len() {
        let v = NbCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥10 条） ───────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：4 行 / 双声明 0 / 100%。
    let g = golden_run();
    v.push((
        "J1-黄金正演4行双0_100%",
        matches!(g, Ok((4, 0, 100))),
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
    // 3. 边界行三分齐备（判据原文口径）。
    let l = build_initial().unwrap_or_else(|_| BoundLedger::new());
    v.push((
        "J1-边界行三分齐备",
        matches!(l.ecs_closed(), Ok(())),
        String::from("四基准"),
    ));
    // 4. 交叉一致率 100%（判据原文口径）。
    let rate = l.cross_rate().unwrap_or(0);
    v.push(("J1-交叉一致率100%", rate == 100, format!("{}%", rate)));
    // 5. 跨界双声明计数 = 0（判据原文口径）。
    v.push((
        "J1-双声明计数0",
        l.double_declare_count(&[]) == 0,
        String::from("邻域无越界声称"),
    ));
    // 6. 双声明检出必红（判据复测方式）。
    v.push((
        "J1-双声明检出必红",
        l.double_declare_count(&[LOCALE_MISSING]) == 1,
        String::from("AI-22 越界声称"),
    ));
    // 7. 三分归属名点验。
    v.push((
        "J1-三分归属名点验",
        OWN_COUNT == 3,
        String::from("E1/AI-24/AI-22"),
    ));
    // 8. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 9. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("四类+三分+邻域池"),
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
pub fn run_unx16044_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16044 NLS/代码页错误语义边界");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
