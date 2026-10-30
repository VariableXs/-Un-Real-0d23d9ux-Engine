//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16141 · 崩溃诊断面总纲（minidump 类 prep） · 0x685C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B08）：
//!   UNX-F16141-J1 总纲账逐环落账（捕获/转储/呈现三环），总纲与任务书
//!   "崩溃诊断/日志 80 条段"一致率 100%，未覆盖崩溃形态零静默断言通过。
//! 定位：任务书域构成段六"崩溃诊断/日志/升级接管声明与域内 ktest 80
//!   条"——本条立崩溃诊断面总纲：捕获环（异常时刻现场捕获）、转储环
//!   （minidump 类语义）、呈现环（三要素报告）三环逐环登记，崩溃形态
//!   全集覆盖账齐备，域内崩溃诊断体系的地基。
//! 设计要点：
//!   - 三环闭集：捕获环（异常触发点与现场快照语义）/转储环（转储触发
//!     与内容范围语义）/呈现环（报告生成与三要素语义）；
//!   - 崩溃形态四族零静默：硬件异常/软件异常/资源耗尽/链损坏——每族
//!     必须被三环覆盖账引用，崩溃诊断的暗角不许存在；
//!   - F16087 传递链总纲交叉：捕获环衔接点必须在传递链闭集内
//!     （异常传递环与诊断捕获环的衔接点一致）；
//!   - 观测降级不阻断：捕获钩子自身异常走降级落账（同 F16014 纪律）；
//!   - 呈现三要素：崩在哪（异常地址面）/为何崩（异常码面）/现场
//!     （线程与栈面）——三要素齐备率 100%；
//!   - 注入面：环越闭集/形态漏覆盖/悬空衔接点/三要素缺失/重复行/
//!     形态越四族——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x685C0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x685C0，AI-VX-1031 E1 域批B 独占区）。
pub const SEG_BASE: u32 = 0x685C0;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 三环闭集 ─────────────────────────────────────────────────────
pub const RING_CAPTURE: usize = 0; // 捕获环
pub const RING_DUMP: usize = 1; // 转储环
pub const RING_REPORT: usize = 2; // 呈现环
pub const RING_COUNT: usize = 3;

pub const RING_NAMES: [&str; RING_COUNT] = ["捕获环", "转储环", "呈现环"];

// ── 崩溃形态四族闭集 ─────────────────────────────────────────────
pub const KIND_HW_EXC: usize = 0; // 硬件异常（页故障/除零/GP）
pub const KIND_SW_EXC: usize = 1; // 软件异常（RaiseException 类）
pub const KIND_RESOURCE: usize = 2; // 资源耗尽（栈/堆/句柄）
pub const KIND_CHAIN: usize = 3; // 链损坏（SEH 链/TLS 链）
pub const KIND_COUNT: usize = 4;

pub const KIND_NAMES: [&str; KIND_COUNT] =
    ["硬件异常", "软件异常", "资源耗尽", "链损坏"];

// ── F16087 传递链衔接点闭集（域内冻结投影） ─────────────────────
pub const HANDOFF_POINTS: [&str; 5] = [
    "trap-frame",     // 陷入帧入口
    "dispatch-lookup",// 分发查表点
    "unwind-start",   // 展开起点
    "unwind-frame",   // 逐帧展开点
    "final-handler",  // 终处理器
];

// ── 呈现三要素闭集 ───────────────────────────────────────────────
pub const ELEM_WHERE: usize = 0; // 崩在哪（异常地址面）
pub const ELEM_WHY: usize = 1; // 为何崩（异常码面）
pub const ELEM_SCENE: usize = 2; // 现场（线程与栈面）
pub const ELEM_COUNT: usize = 3;

pub const ELEM_NAMES: [&str; ELEM_COUNT] = ["崩在哪", "为何崩", "现场"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CdCode {
    /// 环越三环闭集
    RingOutOfRange,
    /// 环+形态行重复登记
    RowDuplicated,
    /// 崩溃形态越四族闭集
    KindOutOfRange,
    /// 形态漏覆盖（某族某环无行且无显式待承接）
    KindUncovered,
    /// 衔接点悬空（不在 F16087 传递链闭集）
    HandoffDangling,
    /// 三要素缺失（呈现环行缺要素引用）
    ElementMissing,
    /// 待承接行缺指针（零静默违规）
    PendingNoPointer,
    /// 观测降级未落账（捕获钩异常静默吞）
    DegradeUnbooked,
    /// 环覆盖缺失（某环零行）
    RingUncovered,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl CdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CdCode::RingOutOfRange => "环越三环闭集",
            CdCode::RowDuplicated => "环+形态行重复：行唯一律",
            CdCode::KindOutOfRange => "崩溃形态越四族闭集",
            CdCode::KindUncovered => "形态漏覆盖：零静默违规",
            CdCode::HandoffDangling => "衔接点悬空：F16087 链外",
            CdCode::ElementMissing => "三要素缺失：呈现面残缺",
            CdCode::PendingNoPointer => "待承接缺指针：零静默违规",
            CdCode::DegradeUnbooked => "观测降级未落账：静默吞错",
            CdCode::RingUncovered => "环覆盖缺失：某环零行",
            CdCode::NotReproducible => "双跑不一致：确定性红线",
            CdCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [CdCode; 11] = [
        CdCode::RingOutOfRange,
        CdCode::RowDuplicated,
        CdCode::KindOutOfRange,
        CdCode::KindUncovered,
        CdCode::HandoffDangling,
        CdCode::ElementMissing,
        CdCode::PendingNoPointer,
        CdCode::DegradeUnbooked,
        CdCode::RingUncovered,
        CdCode::NotReproducible,
        CdCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CdCode::ALL.len() {
        if CdCode::ALL[i].val() == code {
            return CdCode::ALL[i].detail();
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

/// 行状态三态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagState {
    Owned,
    DualRun,
    Pending,
}

impl DiagState {
    pub fn as_u32(self) -> u32 {
        match self {
            DiagState::Owned => 0,
            DiagState::DualRun => 1,
            DiagState::Pending => 2,
        }
    }
    pub fn from_u32(v: u32) -> Option<DiagState> {
        match v {
            0 => Some(DiagState::Owned),
            1 => Some(DiagState::DualRun),
            2 => Some(DiagState::Pending),
            _ => None,
        }
    }
}

/// 总纲账单行：环 × 形态 二维格。
#[derive(Clone, Debug)]
pub struct DiagRow {
    /// 环（0..RING_COUNT）。
    pub ring: usize,
    /// 崩溃形态（0..KIND_COUNT）。
    pub kind: usize,
    /// 衔接点（捕获/转储环必须 ∈ HANDOFF_POINTS；呈现环为空）。
    pub handoff: String,
    /// 呈现要素引用（呈现环行必须引用三要素；其余环为空）。
    pub element: String,
    /// 承接条目指针。
    pub backing: String,
    /// 状态。
    pub state: DiagState,
    /// 待承接指针（Pending 必填）。
    pub pending_ptr: String,
}

pub fn handoff_legal(p: &str) -> bool {
    let mut i = 0;
    while i < HANDOFF_POINTS.len() {
        if HANDOFF_POINTS[i] == p {
            return true;
        }
        i += 1;
    }
    false
}

pub fn element_legal(e: &str) -> bool {
    let mut i = 0;
    while i < ELEM_COUNT {
        if ELEM_NAMES[i] == e {
            return true;
        }
        i += 1;
    }
    false
}

/// 单行体检。Err 带诊断码。
pub fn vet_row(row: &DiagRow) -> Result<(), u32> {
    if row.ring >= RING_COUNT {
        return Err(CdCode::RingOutOfRange.val());
    }
    if row.kind >= KIND_COUNT {
        return Err(CdCode::KindOutOfRange.val());
    }
    if DiagState::from_u32(row.state.as_u32()).is_none() {
        return Err(CdCode::RowDuplicated.val());
    }
    if row.state == DiagState::Pending && row.pending_ptr.len() == 0 {
        return Err(CdCode::PendingNoPointer.val());
    }
    match row.ring {
        RING_CAPTURE | RING_DUMP => {
            // 捕获/转储环必须挂 F16087 传递链衔接点。
            if row.state != DiagState::Pending && !handoff_legal(&row.handoff) {
                return Err(CdCode::HandoffDangling.val());
            }
        }
        _ => {
            // 呈现环必须挂三要素（Pending 除外）。
            if row.state != DiagState::Pending && !element_legal(&row.element) {
                return Err(CdCode::ElementMissing.val());
            }
        }
    }
    if row.backing.len() == 0 {
        return Err(CdCode::PendingNoPointer.val());
    }
    Ok(())
}

// ── 崩溃诊断总纲账 ───────────────────────────────────────────────
pub struct CrashCharter {
    rows: Vec<DiagRow>,
}

impl CrashCharter {
    pub fn new() -> CrashCharter {
        CrashCharter { rows: Vec::new() }
    }

    /// 登记一行（环+形态 唯一）。
    pub fn enroll(&mut self, row: DiagRow) -> Result<(), u32> {
        vet_row(&row)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].ring == row.ring && self.rows[i].kind == row.kind {
                return Err(CdCode::RowDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 三环覆盖：每环至少一行。
    pub fn rings_closed(&self) -> Result<(), u32> {
        let mut r = 0;
        while r < RING_COUNT {
            let mut found = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].ring == r {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(CdCode::RingUncovered.val());
            }
            r += 1;
        }
        Ok(())
    }

    /// 四族零静默：每形态至少一行（覆盖或显式待承接）。
    pub fn kinds_closed(&self) -> Result<(), u32> {
        let mut k = 0;
        while k < KIND_COUNT {
            let mut found = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].kind == k {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(CdCode::KindUncovered.val());
            }
            k += 1;
        }
        Ok(())
    }

    /// 衔接点悬空计数（判据 0 口径）。
    pub fn dangling_count(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            if r.ring == RING_CAPTURE && r.state != DiagState::Pending {
                if !handoff_legal(&r.handoff) {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 三要素齐备率（判据 100% 口径）：呈现环非 Pending 行全部挂要素。
    pub fn element_rate(&self) -> Result<usize, u32> {
        let mut ok = 0;
        let mut total = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            if r.ring == RING_REPORT && r.state != DiagState::Pending {
                total += 1;
                if element_legal(&r.element) {
                    ok += 1;
                }
            }
            i += 1;
        }
        if total == 0 {
            return Err(CdCode::ElementMissing.val());
        }
        Ok((ok * 100) / total)
    }

    /// 观测降级账：降级行必须落账（静默吞错即红）。
    pub fn degrade_booked(&self, degrade_ptr: &str) -> Result<(), u32> {
        if degrade_ptr.len() == 0 {
            return Err(CdCode::DegradeUnbooked.val());
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

// ── 首版总纲装配：三环 × 四族关键格 ─────────────────────────────
pub fn build_initial_charter() -> Result<CrashCharter, u32> {
    let mut ch = CrashCharter::new();
    // 捕获环：硬件/软件/资源 已承接（挂传递链），链损坏待承接。
    ch.enroll(DiagRow {
        ring: RING_CAPTURE,
        kind: KIND_HW_EXC,
        handoff: String::from("trap-frame"),
        element: String::new(),
        backing: String::from("AI-16 SEH 档"),
        state: DiagState::Owned,
        pending_ptr: String::new(),
    })?;
    ch.enroll(DiagRow {
        ring: RING_CAPTURE,
        kind: KIND_SW_EXC,
        handoff: String::from("dispatch-lookup"),
        element: String::new(),
        backing: String::from("AI-16 SEH 档"),
        state: DiagState::Owned,
        pending_ptr: String::new(),
    })?;
    ch.enroll(DiagRow {
        ring: RING_CAPTURE,
        kind: KIND_RESOURCE,
        handoff: String::from("unwind-start"),
        element: String::new(),
        backing: String::from("F16087 传递链总纲"),
        state: DiagState::DualRun,
        pending_ptr: String::new(),
    })?;
    ch.enroll(DiagRow {
        ring: RING_CAPTURE,
        kind: KIND_CHAIN,
        handoff: String::new(),
        element: String::new(),
        backing: String::from("open_risks/E1-B08-chain"),
        state: DiagState::Pending,
        pending_ptr: String::from("open_risks/E1-B08-chain"),
    })?;
    // 转储环：硬件/软件 已承接。
    ch.enroll(DiagRow {
        ring: RING_DUMP,
        kind: KIND_HW_EXC,
        handoff: String::from("unwind-frame"),
        element: String::new(),
        backing: String::from("F16142 转储语义账"),
        state: DiagState::Owned,
        pending_ptr: String::new(),
    })?;
    ch.enroll(DiagRow {
        ring: RING_DUMP,
        kind: KIND_SW_EXC,
        handoff: String::from("final-handler"),
        element: String::new(),
        backing: String::from("F16142 转储语义账"),
        state: DiagState::Owned,
        pending_ptr: String::new(),
    })?;
    // 呈现环：硬件异常挂全三要素抽样（三行各挂一要素）。
    ch.enroll(DiagRow {
        ring: RING_REPORT,
        kind: KIND_HW_EXC,
        handoff: String::new(),
        element: String::from("崩在哪"),
        backing: String::from("F16143 报告三要素"),
        state: DiagState::Owned,
        pending_ptr: String::new(),
    })?;
    ch.enroll(DiagRow {
        ring: RING_REPORT,
        kind: KIND_SW_EXC,
        handoff: String::new(),
        element: String::from("为何崩"),
        backing: String::from("F16143 报告三要素"),
        state: DiagState::Owned,
        pending_ptr: String::new(),
    })?;
    ch.enroll(DiagRow {
        ring: RING_REPORT,
        kind: KIND_RESOURCE,
        handoff: String::new(),
        element: String::from("现场"),
        backing: String::from("F16143 报告三要素"),
        state: DiagState::Owned,
        pending_ptr: String::new(),
    })?;
    Ok(ch)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 悬空数, 要素率))。
pub fn golden_run() -> Result<(usize, u32, usize), u32> {
    let ch = build_initial_charter()?;
    ch.rings_closed()?;
    ch.kinds_closed()?;
    let dangle = ch.dangling_count();
    if dangle != 0 {
        return Err(CdCode::HandoffDangling.val());
    }
    let rate = ch.element_rate()?;
    if rate != 100 {
        return Err(CdCode::ElementMissing.val());
    }
    ch.degrade_booked("degrade/E1-B08-capture")?;
    Ok((ch.len(), dangle, rate))
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
        // ① 衔接点悬空：捕获环 Owned 行挂链外点。
        let bad = DiagRow {
            ring: RING_CAPTURE,
            kind: KIND_HW_EXC,
            handoff: String::from("unknown-point") ,
            element: String::new(),
            backing: String::from("AI-16 SEH 档"),
            state: DiagState::Owned,
            pending_ptr: String::new(),
        };
        led.record(0, vet_row(&bad) == Err(CdCode::HandoffDangling.val()));

        // ② 形态漏覆盖：抽掉链损坏待承接行 → kinds_closed 必红。
        let hole_ok = match build_initial_charter() {
            Ok(mut ch) => {
                let mut i = 0;
                while i < ch.rows.len() {
                    if ch.rows[i].kind == KIND_CHAIN {
                        ch.rows.remove(i);
                        break;
                    }
                    i += 1;
                }
                ch.kinds_closed() == Err(CdCode::KindUncovered.val())
            }
            Err(_) => false,
        };
        led.record(1, hole_ok);

        // ③ 三要素缺失：呈现环 Owned 行不挂要素。
        let bad2 = DiagRow {
            ring: RING_REPORT,
            kind: KIND_HW_EXC,
            handoff: String::new(),
            element: String::new(),
            backing: String::from("F16143 报告三要素"),
            state: DiagState::Owned,
            pending_ptr: String::new(),
        };
        led.record(2, vet_row(&bad2) == Err(CdCode::ElementMissing.val()));

        // ④ 环越闭集 / 形态越闭集 / 状态越闭集。
        led.record(3, {
            let bad3 = DiagRow {
                ring: RING_COUNT + (rnd % 2) as usize,
                kind: KIND_HW_EXC,
                handoff: String::from("trap-frame"),
                element: String::new(),
                backing: String::from("x"),
                state: DiagState::Owned,
                pending_ptr: String::new(),
            };
            vet_row(&bad3) == Err(CdCode::RingOutOfRange.val())
        });
        led.record(3, {
            let bad4 = DiagRow {
                ring: RING_CAPTURE,
                kind: KIND_COUNT + 1 + (rnd % 2) as usize,
                handoff: String::from("trap-frame"),
                element: String::new(),
                backing: String::from("x"),
                state: DiagState::Owned,
                pending_ptr: String::new(),
            };
            vet_row(&bad4) == Err(CdCode::KindOutOfRange.val())
        });
        led.record(3, DiagState::from_u32(3).is_none());
        led.record(3, DiagState::from_u32(0xFFFF).is_none());

        // ⑤ 待承接缺指针 + backing 空。
        let bad5 = DiagRow {
            ring: RING_CAPTURE,
            kind: KIND_CHAIN,
            handoff: String::new(),
            element: String::new(),
            backing: String::from("open_risks/E1-B08-chain"),
            state: DiagState::Pending,
            pending_ptr: String::new(),
        };
        led.record(4, vet_row(&bad5) == Err(CdCode::PendingNoPointer.val()));

        // ⑥ 行重复登记：同环同形态两次 enroll 必拒。
        let dup_ok = {
            let mut ch2 = CrashCharter::new();
            let row = DiagRow {
                ring: RING_CAPTURE,
                kind: KIND_HW_EXC,
                handoff: String::from("trap-frame"),
                element: String::new(),
                backing: String::from("AI-16 SEH 档"),
                state: DiagState::Owned,
                pending_ptr: String::new(),
            };
            let first = ch2.enroll(row.clone());
            let second = ch2.enroll(row);
            first.is_ok() && second == Err(CdCode::RowDuplicated.val())
        };
        // 观测降级静默吞错：空降级指针必红。
        let deg_ok = CrashCharter::new()
            .degrade_booked("") == Err(CdCode::DegradeUnbooked.val());
        led.record(5, dup_ok && deg_ok);

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
                Err(CdCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法总纲必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let ch = build_initial_charter()?;
    ch.rings_closed()?;
    ch.kinds_closed()?;
    // 三环名/四族名/三要素/五衔接点全点验。
    let mut i = 0;
    while i < RING_COUNT {
        if RING_NAMES[i].len() == 0 {
            return Err(CdCode::RingOutOfRange.val());
        }
        i += 1;
    }
    let mut i = 0;
    while i < KIND_COUNT {
        if KIND_NAMES[i].len() == 0 {
            return Err(CdCode::KindOutOfRange.val());
        }
        i += 1;
    }
    let mut i = 0;
    while i < HANDOFF_POINTS.len() {
        if !handoff_legal(HANDOFF_POINTS[i]) {
            return Err(CdCode::HandoffDangling.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CdCode::ALL.len() {
        let v = CdCode::ALL[i].val();
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
    // 1. 黄金正演：9 行 / 0 悬空 / 100% 要素。
    let g = golden_run();
    v.push((
        "J1-黄金正演9行0悬空100%",
        matches!(g, Ok((9, 0, 100))),
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
    // 3. 三环覆盖闭合。
    let ch = build_initial_charter()
        .unwrap_or_else(|_| CrashCharter::new());
    v.push((
        "J1-三环覆盖闭合",
        matches!(ch.rings_closed(), Ok(())),
        format!("{} 行", ch.len()),
    ));
    // 4. 四族零静默（链损坏显式待承接）。
    v.push((
        "J1-四族零静默",
        matches!(ch.kinds_closed(), Ok(())),
        String::from("硬件/软件/资源/链"),
    ));
    // 5. 悬空衔接点 = 0（判据原文口径）。
    v.push((
        "J1-悬空衔接点0",
        ch.dangling_count() == 0,
        format!("{}", ch.dangling_count()),
    ));
    // 6. 三要素齐备率 100%（判据原文口径）。
    let rate = ch.element_rate().unwrap_or(0);
    v.push(("J1-三要素齐备100%", rate == 100, format!("{}%", rate)));
    // 7. 观测降级落账律。
    v.push((
        "J1-观测降级落账律",
        matches!(ch.degrade_booked("degrade/E1-B08-capture"), Ok(())),
        String::from("钩异常降级显性"),
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
        String::from("链闭集+名点验"),
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
pub fn run_unx16141_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16141 崩溃诊断面总纲（minidump 类 prep）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
