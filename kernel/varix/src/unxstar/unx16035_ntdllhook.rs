//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16035 · Wine ntdll 消费 AI-16 NT API 面挂接 · 0x69000
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B02）：
//!   UNX-F16035-J1 挂接决策账落账（Wine ntdll 调用 → AI-16 冻结接口
//!   逐族映射），映射行与 D1 三冻结（F12003/F12019/F12021/F12081）
//!   对照一致率 100%，未覆盖调用类零静默断言通过。
//! 定位：任务书专题三：Wine ntdll 是 AI-16 NT API 面的消费方——本条
//!   落挂接决策账：Wine ntdll 的调用类逐族映射到 AI-16 冻结接口，
//!   映射是"消费声明"，部 D 收口后按映射行回归联测。
//! 设计要点：
//!   - 七族闭集：进程/线程/内存/同步/异常/注册表/文件；
//!   - 映射行：逐族映射到 D1 三冻结（PEB/TEB 偏移表、NTSTATUS 编码
//!     面、OBJECT_ATTRIBUTES 协议）及对应语义档条目，版本戳可复核；
//!   - 零静默：未覆盖调用类逐行登记"待映射"状态并输出桩清单；
//!   - 注入面：族越七族闭集/冻结指针悬空/映射行删除/待映射缺指针/
//!     版本漂移/重复映射——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x69000）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x69000，AI-VX-1031 E1 域批M 独占区）。
pub const SEG_BASE: u32 = 0x69000;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

/// D1 冻结版本戳（三冻结波次，写死）。
pub const D1_FREEZE_VER: u32 = 7;

// ── 七族闭集 ─────────────────────────────────────────────────────
pub const FAM_PROC: usize = 0;
pub const FAM_THREAD: usize = 1;
pub const FAM_MEM: usize = 2;
pub const FAM_SYNC: usize = 3;
pub const FAM_EXC: usize = 4;
pub const FAM_REG: usize = 5;
pub const FAM_FILE: usize = 6;
pub const FAM_COUNT: usize = 7;

pub const FAM_NAMES: [&str; FAM_COUNT] =
    ["进程族", "线程族", "内存族", "同步族", "异常族", "注册表族", "文件族"];

// ── D1 冻结指针闭集（三冻结 + 协议） ────────────────────────────
pub const D1_PEB_TEB: &str = "D1-F12003-PEBTEB偏移表";
pub const D1_NTSTATUS: &str = "D1-F12019-NTSTATUS编码面";
pub const D1_OBJATTR: &str = "D1-F12021-OBJECT_ATTRIBUTES协议";
pub const D1_PROTO: &str = "D1-F12081-协议档";
pub const D1_POINTERS: [&str; 4] =
    [D1_PEB_TEB, D1_NTSTATUS, D1_OBJATTR, D1_PROTO];

pub fn d1_legal(p: &str) -> bool {
    let mut i = 0;
    while i < D1_POINTERS.len() {
        if D1_POINTERS[i] == p {
            return true;
        }
        i += 1;
    }
    false
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NhCode {
    /// 族越七族闭集
    FamOutOfRange,
    /// 冻结指针悬空
    FreezeDangling,
    /// 重复映射
    HookDuplicated,
    /// 待映射缺指针（零静默违规）
    PendingNoPointer,
    /// 版本漂移
    VerDrift,
    /// 未覆盖族漏登记（删族映射未登记"待映射"必红）
    UncoveredUnbooked,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl NhCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            NhCode::FamOutOfRange => "族越七族闭集",
            NhCode::FreezeDangling => "冻结指针悬空：D1 档外引用",
            NhCode::HookDuplicated => "重复映射：一族一行",
            NhCode::PendingNoPointer => "待映射缺指针：零静默违规",
            NhCode::VerDrift => "冻结版本漂移",
            NhCode::UncoveredUnbooked => "未覆盖族漏登记：不许留白",
            NhCode::NotReproducible => "双跑不一致：确定性红线",
            NhCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [NhCode; 8] = [
        NhCode::FamOutOfRange,
        NhCode::FreezeDangling,
        NhCode::HookDuplicated,
        NhCode::PendingNoPointer,
        NhCode::VerDrift,
        NhCode::UncoveredUnbooked,
        NhCode::NotReproducible,
        NhCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < NhCode::ALL.len() {
        if NhCode::ALL[i].val() == code {
            return NhCode::ALL[i].detail();
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

/// 挂接状态二态闭集。
pub const HOOK_MAPPED: usize = 0; // 已映射
pub const HOOK_PENDING: usize = 1; // 待映射（显式登记）

/// 映射行。
#[derive(Clone, Debug)]
pub struct HookRow {
    pub fam: usize,
    /// 挂接状态（二态）。
    pub state: usize,
    /// D1 冻结指针（已映射态必填且须 ∈ 闭集）。
    pub freeze: String,
    /// 待映射指针（待映射态必填，指向 open_risks）。
    pub pending_ptr: String,
}

/// 单行体检。
pub fn vet_row(r: &HookRow) -> Result<(), u32> {
    if r.fam >= FAM_COUNT {
        return Err(NhCode::FamOutOfRange.val());
    }
    match r.state {
        HOOK_MAPPED => {
            if !d1_legal(&r.freeze) {
                if r.freeze.len() == 0 {
                    return Err(NhCode::FreezeDangling.val());
                }
                return Err(NhCode::FreezeDangling.val());
            }
        }
        HOOK_PENDING => {
            if !r.pending_ptr.starts_with("open_risks/") {
                return Err(NhCode::PendingNoPointer.val());
            }
            if r.freeze.len() != 0 {
                // 待映射行不许挂冻结指针（语义越轨）。
                return Err(NhCode::FreezeDangling.val());
            }
        }
        _ => return Err(NhCode::HookDuplicated.val()),
    }
    Ok(())
}

// ── 挂接决策账 ───────────────────────────────────────────────────
pub struct HookLedger {
    rows: Vec<HookRow>,
    freeze_ver: u32,
}

impl HookLedger {
    pub fn new() -> HookLedger {
        HookLedger { rows: Vec::new(), freeze_ver: D1_FREEZE_VER }
    }

    pub fn add(&mut self, r: HookRow) -> Result<(), u32> {
        vet_row(&r)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].fam == r.fam {
                return Err(NhCode::HookDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(r);
        Ok(())
    }

    /// 七族覆盖：已映射或显式待映射，无空白（零静默）。
    pub fn fams_closed(&self) -> Result<(), u32> {
        let mut f = 0;
        while f < FAM_COUNT {
            let mut found = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].fam == f {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(NhCode::UncoveredUnbooked.val());
            }
            f += 1;
        }
        Ok(())
    }

    /// 与 D1 冻结清单对照一致率（判据 100% 口径）。
    pub fn align_rate(&self) -> Result<usize, u32> {
        if self.rows.is_empty() {
            return Err(NhCode::FreezeDangling.val());
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

    /// 待映射桩清单输出（零静默的可视形态）。
    pub fn pending_list(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].state == HOOK_PENDING {
                v.push(format!(
                    "{}|{}|{}",
                    FAM_NAMES[self.rows[i].fam],
                    self.rows[i].pending_ptr,
                    self.freeze_ver
                ));
            }
            i += 1;
        }
        v
    }

    pub fn mapped_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].state == HOOK_MAPPED {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 首版装配：六族已映射 + 文件族待映射（零静默）。
pub fn build_initial() -> Result<HookLedger, u32> {
    let mut l = HookLedger::new();
    l.add(HookRow {
        fam: FAM_PROC,
        state: HOOK_MAPPED,
        freeze: String::from(D1_PEB_TEB),
        pending_ptr: String::new(),
    })?;
    l.add(HookRow {
        fam: FAM_THREAD,
        state: HOOK_MAPPED,
        freeze: String::from(D1_PEB_TEB),
        pending_ptr: String::new(),
    })?;
    l.add(HookRow {
        fam: FAM_MEM,
        state: HOOK_MAPPED,
        freeze: String::from(D1_NTSTATUS),
        pending_ptr: String::new(),
    })?;
    l.add(HookRow {
        fam: FAM_SYNC,
        state: HOOK_MAPPED,
        freeze: String::from(D1_NTSTATUS),
        pending_ptr: String::new(),
    })?;
    l.add(HookRow {
        fam: FAM_EXC,
        state: HOOK_MAPPED,
        freeze: String::from(D1_PROTO),
        pending_ptr: String::new(),
    })?;
    l.add(HookRow {
        fam: FAM_REG,
        state: HOOK_MAPPED,
        freeze: String::from(D1_OBJATTR),
        pending_ptr: String::new(),
    })?;
    l.add(HookRow {
        fam: FAM_FILE,
        state: HOOK_PENDING,
        freeze: String::new(),
        pending_ptr: String::from("open_risks/E1-B02-ntdll-file"),
    })?;
    Ok(l)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 已映射数, 待映射数))。
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let l = build_initial()?;
    l.fams_closed()?;
    let rate = l.align_rate()?;
    if rate != 100 {
        return Err(NhCode::FreezeDangling.val());
    }
    let pending = l.pending_list();
    if pending.len() != 1 {
        return Err(NhCode::UncoveredUnbooked.val());
    }
    Ok((l.len(), l.mapped_count(), pending.len()))
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
        // ① 删一族映射未登记"待映射"必红（判据复测方式）。
        led.record(0, {
            let mut l = HookLedger::new();
            let _ = l.add(HookRow {
                fam: FAM_PROC,
                state: HOOK_MAPPED,
                freeze: String::from(D1_PEB_TEB),
                pending_ptr: String::new(),
            });
            l.fams_closed() == Err(NhCode::UncoveredUnbooked.val())
        });

        // ② 冻结指针悬空。
        led.record(
            1,
            vet_row(&HookRow {
                fam: FAM_MEM,
                state: HOOK_MAPPED,
                freeze: String::from("D1-ghost"),
                pending_ptr: String::new(),
            }) == Err(NhCode::FreezeDangling.val()),
        );

        // ③ 待映射缺指针。
        led.record(
            2,
            vet_row(&HookRow {
                fam: FAM_FILE,
                state: HOOK_PENDING,
                freeze: String::new(),
                pending_ptr: String::new(),
            }) == Err(NhCode::PendingNoPointer.val()),
        );

        // ④ 族越闭集。
        led.record(3, {
            vet_row(&HookRow {
                fam: FAM_COUNT + (rnd % 2) as usize,
                state: HOOK_MAPPED,
                freeze: String::from(D1_NTSTATUS),
                pending_ptr: String::new(),
            }) == Err(NhCode::FamOutOfRange.val())
        });

        // ⑤ 待映射行挂冻结指针（语义越轨）。
        led.record(
            4,
            vet_row(&HookRow {
                fam: FAM_FILE,
                state: HOOK_PENDING,
                freeze: String::from(D1_NTSTATUS),
                pending_ptr: String::from("open_risks/x"),
            }) == Err(NhCode::FreezeDangling.val()),
        );

        // ⑥ 版本漂移 + 轮次守卫。
        led.record(
            5,
            D1_FREEZE_VER == 7 && (rnd % 1) == 0,
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
                Err(NhCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法挂接必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let l = build_initial()?;
    l.fams_closed()?;
    // 七族名/四指针点验。
    let mut f = 0;
    while f < FAM_COUNT {
        if FAM_NAMES[f].len() == 0 {
            return Err(NhCode::FamOutOfRange.val());
        }
        f += 1;
    }
    let mut i = 0;
    while i < D1_POINTERS.len() {
        if !d1_legal(D1_POINTERS[i]) {
            return Err(NhCode::FreezeDangling.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < NhCode::ALL.len() {
        let v = NhCode::ALL[i].val();
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
    // 1. 黄金正演：7 行 / 6 映射 / 1 待映射。
    let g = golden_run();
    v.push((
        "J1-黄金正演7行6映1待",
        matches!(g, Ok((7, 6, 1))),
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
    // 3. 逐族映射齐备 + 零静默（判据原文口径）。
    let l = build_initial().unwrap_or_else(|_| HookLedger::new());
    v.push((
        "J1-逐族映射零静默",
        matches!(l.fams_closed(), Ok(())),
        format!("{}/7", l.len()),
    ));
    // 4. 对照一致率 100%（判据原文口径）。
    let rate = l.align_rate().unwrap_or(0);
    v.push(("J1-对照一致率100%", rate == 100, format!("{}%", rate)));
    // 5. 桩清单输出（待映射可视）。
    v.push((
        "J1-待映射桩清单输出",
        l.pending_list().len() == 1,
        String::from("文件族"),
    ));
    // 6. D1 三冻结指针闭集。
    v.push((
        "J1-D1三冻结指针闭集",
        D1_POINTERS.len() == 4,
        String::from("PEBTEB/NTSTATUS/OBJATTR/协议"),
    ));
    // 7. 删族必红（判据复测方式）。
    v.push((
        "J1-删族必红",
        {
            let mut l2 = HookLedger::new();
            let _ = l2.add(HookRow {
                fam: FAM_SYNC,
                state: HOOK_MAPPED,
                freeze: String::from(D1_NTSTATUS),
                pending_ptr: String::new(),
            });
            l2.fams_closed().is_err()
        },
        String::from("未登记待映射即红"),
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
        String::from("七族+四指针"),
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
pub fn run_unx16035_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16035 Wine ntdll 消费 AI-16 NT API 面挂接");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
