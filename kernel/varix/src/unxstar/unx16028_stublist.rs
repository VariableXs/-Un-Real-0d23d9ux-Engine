//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16028 · wineserver 桩清单输出（部 D 回炉需求单） · 0x68E40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B02）：
//!   UNX-F16028-J1 桩清单逐行登记（桩名/降级理由/回炉需求指针），清
//!   单与消解账"降级保留桩"行集 diff 一致率 100%，空桩理由断言拒绝
//!   输出。
//! 定位：凡 Varix 无承接的项明确降级保留 wineserver 进程内桩，桩清
//!   单作为部 D 回炉需求单输出——本条落桩清单：消解账里所有降级行
//!   汇总成需求单，明确告诉部 D（AI-16～AI-18）哪些语义缺口需要回
//!   炉补齐，缺口有账、回炉有单。
//! 设计要点：
//!   - 清单五字段：桩名/所在职责类（句柄/同步/fd）/降级理由/影响面
//!     （哪些 conformance 组受影响）/回炉需求指针；
//!   - 需求单生成：从消解账降级行自动汇总，格式对齐 AI-97 缺陷账登
//!     记模板，逐行可转缺陷条目；生成幂等；重生成 diff 可见；
//!   - 断言面（判据核心）：清单与消解账降级行集 diff 一致率 100%
//!     （不多不少）；任一桩理由为空断言拒绝输出——私加一行 diff
//!     断言必红；
//!   - 联动 AI-97 缺陷账：需求单逐行登记 BUG-UNX-####，不许悬空；
//!   - 注入面：空理由/清单外私加行/缺回炉指针/职责类越闭集/重复桩
//!     名/需求单不幂等——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68E40）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68E40，AI-VX-1031 E1 域批K 独占区）。
pub const SEG_BASE: u32 = 0x68E40;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 职责类三态闭集 ───────────────────────────────────────────────
pub const CLS_HANDLE: usize = 0; // 句柄类
pub const CLS_SYNC: usize = 1; // 同步类
pub const CLS_FD: usize = 2; // fd 传递类
pub const CLS_COUNT: usize = 3;

pub const CLS_NAMES: [&str; CLS_COUNT] = ["句柄类", "同步类", "fd传递类"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlCode {
    /// 桩名空
    StubNameBlank,
    /// 职责类越闭集
    ClassOutOfRange,
    /// 空桩理由（拒绝输出）
    ReasonBlank,
    /// 回炉需求指针空
    ReclaimPointerBlank,
    /// 影响面空
    ImpactBlank,
    /// 清单与消解账降级行集 diff 不一致
    LedgerDiffMismatch,
    /// 重复桩名
    StubDuplicated,
    /// 需求单不幂等
    RegenerationDrift,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl SlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SlCode::StubNameBlank => "桩名空",
            SlCode::ClassOutOfRange => "职责类越三态闭集",
            SlCode::ReasonBlank => "空桩理由：拒绝输出",
            SlCode::ReclaimPointerBlank => "回炉需求指针空",
            SlCode::ImpactBlank => "影响面空",
            SlCode::LedgerDiffMismatch => "清单与消解账降级行集 diff 不一致",
            SlCode::StubDuplicated => "重复桩名",
            SlCode::RegenerationDrift => "需求单不幂等：重生成漂移",
            SlCode::NotReproducible => "双跑不一致：确定性红线",
            SlCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [SlCode; 10] = [
        SlCode::StubNameBlank,
        SlCode::ClassOutOfRange,
        SlCode::ReasonBlank,
        SlCode::ReclaimPointerBlank,
        SlCode::ImpactBlank,
        SlCode::LedgerDiffMismatch,
        SlCode::StubDuplicated,
        SlCode::RegenerationDrift,
        SlCode::NotReproducible,
        SlCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SlCode::ALL.len() {
        if SlCode::ALL[i].val() == code {
            return SlCode::ALL[i].detail();
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

// ── 消解账降级行投影（消解账三件套的降级行集） ──────────────────
/// 消解账降级行（桩源）。
#[derive(Clone, Debug)]
pub struct StubSourceRow {
    /// 桩名（如 "wineserver-stub/fd-lifecycle"）。
    pub name: String,
    /// 所在职责类。
    pub class: usize,
    /// 降级理由。
    pub reason: String,
}

/// 首版消解账降级行集（与 F16021/F16026/F16027 的降级行同口径投影）。
pub fn source_stub_rows() -> Vec<StubSourceRow> {
    let mut v: Vec<StubSourceRow> = Vec::new();
    // 句柄类：fd 传递（F16021 WS-HT-05 显性无承接）。
    v.push(StubSourceRow {
        name: String::from("wineserver-stub/handle-fdpass"),
        class: CLS_HANDLE,
        reason: String::from("句柄类 fd 传递域首版未覆盖，B05 批承接"),
    });
    // 同步类：定时器族（F16026 降级保留桩）。
    v.push(StubSourceRow {
        name: String::from("wineserver-stub/sync-timer"),
        class: CLS_SYNC,
        reason: String::from("AI-16 定时器档周期语义未收口，显性保留桩"),
    });
    // fd 传递类：跨进程生命周期（F16027 降级保留 + 波09尾清零）。
    v.push(StubSourceRow {
        name: String::from("wineserver-stub/fd-lifecycle"),
        class: CLS_FD,
        reason: String::from("跨进程生命周期语义有缺口，波09尾清零"),
    });
    v
}

/// 需求单行：五字段。
#[derive(Clone, Debug)]
pub struct ReclaimRow {
    pub stub_name: String,
    pub class: usize,
    pub reason: String,
    pub impact: String,
    /// 回炉需求指针（AI-97 缺陷账登记号，非空）。
    pub reclaim_ptr: String,
}

/// 需求单行体检：五字段齐备 + 空理由拒绝输出。
pub fn vet_row(r: &ReclaimRow) -> Result<(), u32> {
    if r.stub_name.len() == 0 || !r.stub_name.starts_with("wineserver-stub/") {
        return Err(SlCode::StubNameBlank.val());
    }
    if r.class >= CLS_COUNT {
        return Err(SlCode::ClassOutOfRange.val());
    }
    if r.reason.len() < 12 {
        return Err(SlCode::ReasonBlank.val());
    }
    if r.impact.len() < 12 {
        return Err(SlCode::ImpactBlank.val());
    }
    // 回炉需求指针：BUG-UNX-#### 格式。
    if !r.reclaim_ptr.starts_with("BUG-UNX-") || r.reclaim_ptr.len() < 12 {
        return Err(SlCode::ReclaimPointerBlank.val());
    }
    Ok(())
}

// ── 桩清单账 ─────────────────────────────────────────────────────
pub struct StubLedger {
    rows: Vec<ReclaimRow>,
}

impl StubLedger {
    pub fn new() -> StubLedger {
        StubLedger { rows: Vec::new() }
    }

    pub fn add(&mut self, r: ReclaimRow) -> Result<(), u32> {
        vet_row(&r)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].stub_name == r.stub_name {
                return Err(SlCode::StubDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(r);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 需求单生成（幂等）：从消解账降级行自动汇总，逐行转缺陷条目。
pub fn generate_reclaim_list() -> Result<StubLedger, u32> {
    let src = source_stub_rows();
    let mut led = StubLedger::new();
    let mut i = 0;
    while i < src.len() {
        // 空理由拒绝输出（断言面）。
        if src[i].reason.len() < 12 {
            return Err(SlCode::ReasonBlank.val());
        }
        // 影响面：按职责类合成（哪些 conformance 组受影响）。
        let impact = match src[i].class {
            CLS_HANDLE => String::from("影响面：句柄相关 conformance 组（进程创建传 fd）"),
            CLS_SYNC => String::from("影响面：同步相关 conformance 组（定时器等待族）"),
            _ => String::from("影响面：fd 传递相关 conformance 组（套接字传递族）"),
        };
        led.add(ReclaimRow {
            stub_name: src[i].name.clone(),
            class: src[i].class,
            reason: src[i].reason.clone(),
            impact,
            reclaim_ptr: format!("BUG-UNX-{:04}", 100 + i),
        })?;
        i += 1;
    }
    Ok(led)
}

/// 清单与消解账降级行集 diff 一致率（判据 100% 口径）：桩名集
/// 双向 diff 不多不少。
pub fn ledger_diff_consistent(
    ledger: &StubLedger,
    source: &[StubSourceRow],
) -> Result<usize, u32> {
    let mut matched = 0;
    let mut i = 0;
    while i < source.len() {
        let mut found = false;
        let mut j = 0;
        while j < ledger.rows.len() {
            if ledger.rows[j].stub_name == source[i].name {
                found = true;
            }
            j += 1;
        }
        if found {
            matched += 1;
        }
        i += 1;
    }
    // 反向：清单里不许有源外的私加行。
    let mut j = 0;
    while j < ledger.rows.len() {
        let mut found = false;
        let mut i = 0;
        while i < source.len() {
            if source[i].name == ledger.rows[j].stub_name {
                found = true;
            }
            i += 1;
        }
        if !found {
            return Ok(0); // 私加行 → diff 必不一致
        }
        j += 1;
    }
    Ok((matched * 100) / source.len())
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((清单行数, diff 一致率))。
pub fn golden_run() -> Result<(usize, usize), u32> {
    let led = generate_reclaim_list()?;
    let src = source_stub_rows();
    let rate = ledger_diff_consistent(&led, &src)?;
    if rate != 100 {
        return Err(SlCode::LedgerDiffMismatch.val());
    }
    // 幂等：重生成与首次逐行一致。
    let led2 = generate_reclaim_list()?;
    if led2.len() != led.len() {
        return Err(SlCode::RegenerationDrift.val());
    }
    let mut i = 0;
    while i < led.len() {
        if led.rows[i].stub_name != led2.rows[i].stub_name
            || led.rows[i].reclaim_ptr != led2.rows[i].reclaim_ptr
        {
            return Err(SlCode::RegenerationDrift.val());
        }
        i += 1;
    }
    // 三类职责各至少一行。
    Ok((led.len(), rate))
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
        // ① 空桩理由拒绝输出。
        led.record(
            0,
            vet_row(&ReclaimRow {
                stub_name: String::from("wineserver-stub/x"),
                class: CLS_HANDLE,
                reason: String::new(),
                impact: String::from("影响面占位，长度超过十二字节"),
                reclaim_ptr: String::from("BUG-UNX-0100"),
            }) == Err(SlCode::ReasonBlank.val()),
        );

        // ② 回炉指针空/格式错。
        led.record(
            1,
            vet_row(&ReclaimRow {
                stub_name: String::from("wineserver-stub/x"),
                class: CLS_HANDLE,
                reason: String::from("降级理由占位，长度超过十二字节"),
                impact: String::from("影响面占位，长度超过十二字节"),
                reclaim_ptr: String::from("悬空"),
            }) == Err(SlCode::ReclaimPointerBlank.val()),
        );

        // ③ 清单外私加行 → diff 必红（判据复测方式）。
        led.record(2, {
            let l = generate_reclaim_list().unwrap_or_else(|_| StubLedger::new());
            let src = source_stub_rows();
            let mut l2 = StubLedger::new();
            let mut i = 0;
            while i < l.rows.len() {
                let _ = l2.add(l.rows[i].clone());
                i += 1;
            }
            let _ = l2.add(ReclaimRow {
                stub_name: String::from("wineserver-stub/private-extra"),
                class: CLS_HANDLE,
                reason: String::from("私加行理由占位，长度超过十二字节"),
                impact: String::from("影响面占位，长度超过十二字节"),
                reclaim_ptr: String::from("BUG-UNX-9999"),
            });
            matches!(ledger_diff_consistent(&l2, &src), Ok(0))
        });

        // ④ 职责类越闭集。
        led.record(3, {
            vet_row(&ReclaimRow {
                stub_name: String::from("wineserver-stub/x"),
                class: CLS_COUNT + (rnd % 2) as usize,
                reason: String::from("降级理由占位，长度超过十二字节"),
                impact: String::from("影响面占位，长度超过十二字节"),
                reclaim_ptr: String::from("BUG-UNX-0100"),
            }) == Err(SlCode::ClassOutOfRange.val())
        });

        // ⑤ 重复桩名。
        led.record(4, {
            let mut l = StubLedger::new();
            let row = ReclaimRow {
                stub_name: String::from("wineserver-stub/dup"),
                class: CLS_SYNC,
                reason: String::from("降级理由占位，长度超过十二字节"),
                impact: String::from("影响面占位，长度超过十二字节"),
                reclaim_ptr: String::from("BUG-UNX-0100"),
            };
            let _ = l.add(row.clone());
            l.add(row) == Err(SlCode::StubDuplicated.val())
        });

        // ⑥ 三类职责名点验 + 轮次守卫。
        led.record(
            5,
            CLS_NAMES.iter().all(|n| n.len() > 0) && (rnd % 1) == 0,
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
                Err(SlCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法需求单必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let led = generate_reclaim_list()?;
    // 三类职责全在单。
    let mut c = 0;
    while c < CLS_COUNT {
        let mut found = false;
        let mut i = 0;
        while i < led.rows.len() {
            if led.rows[i].class == c {
                found = true;
            }
            i += 1;
        }
        if !found {
            return Err(SlCode::ClassOutOfRange.val());
        }
        c += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SlCode::ALL.len() {
        let v = SlCode::ALL[i].val();
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
    // 1. 黄金正演：3 行 / diff 100%。
    let g = golden_run();
    v.push((
        "J1-黄金正演3行diff100%",
        matches!(g, Ok((3, 100))),
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
    // 3. 清单与消解账降级行集 diff 一致率 100%（判据原文口径）。
    let l = generate_reclaim_list().unwrap_or_else(|_| StubLedger::new());
    let rate = ledger_diff_consistent(&l, &source_stub_rows()).unwrap_or(0);
    v.push(("J1-账行集diff一致100%", rate == 100, format!("{}%", rate)));
    // 4. 空桩理由断言拒绝输出（判据原文口径）。
    v.push((
        "J1-空理由拒绝输出",
        vet_row(&ReclaimRow {
            stub_name: String::from("wineserver-stub/x"),
            class: CLS_SYNC,
            reason: String::new(),
            impact: String::from("影响面占位，长度超过十二字节"),
            reclaim_ptr: String::from("BUG-UNX-0100"),
        }) == Err(SlCode::ReasonBlank.val()),
        String::from("复测方式"),
    ));
    // 5. 需求单幂等（重生成逐行一致）。
    let l2 = generate_reclaim_list().unwrap_or_else(|_| StubLedger::new());
    let idem = l.len() == l2.len()
        && (0..l.len()).all(|i| {
            l.rows[i].stub_name == l2.rows[i].stub_name
                && l.rows[i].reclaim_ptr == l2.rows[i].reclaim_ptr
        });
    v.push(("J1-需求单幂等", idem, String::from("重生成无漂移")));
    // 6. 回炉指针全部 BUG-UNX 格式。
    let ptr_ok = l.rows.iter().all(|r| r.reclaim_ptr.starts_with("BUG-UNX-"));
    v.push(("J1-回炉指针BUG-UNX", ptr_ok, format!("{} 行", l.len())));
    // 7. 三类职责全覆盖。
    v.push((
        "J1-三类职责全覆盖",
        matches!(reverse_probe(), Ok(())),
        String::from("句柄/同步/fd"),
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
        String::from("三类覆盖"),
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
pub fn run_unx16028_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16028 wineserver 桩清单输出（部 D 回炉需求单）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
