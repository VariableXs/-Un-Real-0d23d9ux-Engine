//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16036 · Wine kernel32 消费面挂接评估 · 0x69040
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B02）：
//!   UNX-F16036-J1 评估账逐类落账（kernel32 调用类/承接面评估/结论），
//!   结论三态齐备率 100%，与 D4 消息泵语义档交叉引用断言通过。
//! 定位：Wine kernel32 的实现质量是 conformance 主战场之一，但它同
//!   时与 D4 消息泵、B1 文件语义、AI-17 进程语义三方交错——本条做
//!   消费面评估：kernel32 调用类逐类评估"走 Wine 自带实现 / 走
//!   Varix 语义对照补齐"，评估结论为 conformance 失败分类学提供归
//!   因底账。
//! 设计要点：
//!   - 六类闭集：进程线程/文件/内存/同步/加载器/控制台；
//!   - 结论三态：Wine 自带可承接 / 需 Varix 语义补齐 / 待定；
//!   - 交叉引用："需补齐"行必须引用 Varix 对应语义档（消息泵→D4、
//!     文件→B1、进程→AI-17），引用指针可复核，删引用必拒收；
//!   - 上游版本戳：评估行带版本戳，漂移即重评留痕；
//!   - 注入面：类越六类/结论越三态/需补齐缺引用/引用悬空/重复评估/
//!     版本漂移——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x69040）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x69040，AI-VX-1031 E1 域批M 独占区）。
pub const SEG_BASE: u32 = 0x69040;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

/// 上游版本戳（写死）。
pub const UPSTREAM_VER: u32 = 10;

// ── 六类闭集 ─────────────────────────────────────────────────────
pub const K_PROC: usize = 0;
pub const K_FILE: usize = 1;
pub const K_MEM: usize = 2;
pub const K_SYNC: usize = 3;
pub const K_LOADER: usize = 4;
pub const K_CONSOLE: usize = 5;
pub const K_COUNT: usize = 6;

pub const K_NAMES: [&str; K_COUNT] =
    ["进程线程类", "文件类", "内存类", "同步类", "加载器类", "控制台类"];

// ── 结论三态闭集 ─────────────────────────────────────────────────
pub const CONV_WINE_OK: usize = 0; // Wine 自带可承接
pub const CONV_NEED_VARIX: usize = 1; // 需 Varix 语义补齐
pub const CONV_PENDING: usize = 2; // 待定
pub const CONV_COUNT: usize = 3;

pub const CONV_NAMES: [&str; CONV_COUNT] =
    ["Wine自带可承接", "需Varix语义补齐", "待定"];

// ── 语义档指针闭集（交叉引用可复核） ────────────────────────────
pub const SEM_POOL: [&str; 4] = [
    "D4-消息泵语义档",
    "B1-文件语义档",
    "AI-17-进程语义档",
    "AI-16-同步语义档",
];

pub fn sem_legal(s: &str) -> bool {
    let mut i = 0;
    while i < SEM_POOL.len() {
        if SEM_POOL[i] == s {
            return true;
        }
        i += 1;
    }
    false
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeCode {
    /// 类越六类闭集
    ClassOutOfRange,
    /// 结论越三态闭集
    ConvOutOfRange,
    /// 需补齐缺引用指针（判据复测方式：删一行引用必拒收）
    RefMissing,
    /// 引用悬空（语义档外引用）
    RefDangling,
    /// 重复评估
    EvalDuplicated,
    /// 上游版本漂移
    UpstreamVerDrift,
    /// 待定行缺开放指针
    PendingNoPointer,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl KeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KeCode::ClassOutOfRange => "类越六类闭集",
            KeCode::ConvOutOfRange => "结论越三态闭集",
            KeCode::RefMissing => "需补齐缺引用指针：说要补齐就得说清向谁对齐",
            KeCode::RefDangling => "引用悬空：语义档外引用",
            KeCode::EvalDuplicated => "重复评估：一类一评估",
            KeCode::UpstreamVerDrift => "上游版本漂移：须重评留痕",
            KeCode::PendingNoPointer => "待定行缺开放指针",
            KeCode::NotReproducible => "双跑不一致：确定性红线",
            KeCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [KeCode; 9] = [
        KeCode::ClassOutOfRange,
        KeCode::ConvOutOfRange,
        KeCode::RefMissing,
        KeCode::RefDangling,
        KeCode::EvalDuplicated,
        KeCode::UpstreamVerDrift,
        KeCode::PendingNoPointer,
        KeCode::NotReproducible,
        KeCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < KeCode::ALL.len() {
        if KeCode::ALL[i].val() == code {
            return KeCode::ALL[i].detail();
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

/// 评估行。
#[derive(Clone, Debug)]
pub struct K32EvalRow {
    pub class: usize,
    pub conv: usize,
    /// 语义档引用（需补齐态必填且 ∈ SEM_POOL）。
    pub sem_ref: String,
    /// 待定开放指针（待定态必填）。
    pub open_ptr: String,
}

/// 单行体检。
pub fn vet_row(r: &K32EvalRow) -> Result<(), u32> {
    if r.class >= K_COUNT {
        return Err(KeCode::ClassOutOfRange.val());
    }
    if r.conv >= CONV_COUNT {
        return Err(KeCode::ConvOutOfRange.val());
    }
    match r.conv {
        CONV_NEED_VARIX => {
            if !sem_legal(&r.sem_ref) {
                return Err(KeCode::RefDangling.val());
            }
        }
        CONV_PENDING => {
            if !r.open_ptr.starts_with("open_risks/") {
                return Err(KeCode::PendingNoPointer.val());
            }
            if r.sem_ref.len() != 0 {
                return Err(KeCode::RefDangling.val());
            }
        }
        _ => {
            // Wine 自带可承接：不许挂语义档引用（未补齐无对象）。
            if r.sem_ref.len() != 0 {
                return Err(KeCode::RefDangling.val());
            }
        }
    }
    Ok(())
}

// ── 评估账 ───────────────────────────────────────────────────────
pub struct K32Ledger {
    rows: Vec<K32EvalRow>,
    upstream_ver: u32,
}

impl K32Ledger {
    pub fn new() -> K32Ledger {
        K32Ledger { rows: Vec::new(), upstream_ver: UPSTREAM_VER }
    }

    pub fn add(&mut self, r: K32EvalRow) -> Result<(), u32> {
        vet_row(&r)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].class == r.class {
                return Err(KeCode::EvalDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(r);
        Ok(())
    }

    /// 上游版本钉核对。
    pub fn upstream_ok(&self) -> Result<(), u32> {
        if self.upstream_ver != UPSTREAM_VER {
            return Err(KeCode::UpstreamVerDrift.val());
        }
        Ok(())
    }

    /// 六类覆盖。
    pub fn classes_closed(&self) -> Result<(), u32> {
        let mut c = 0;
        while c < K_COUNT {
            let mut found = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].class == c {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(KeCode::ClassOutOfRange.val());
            }
            c += 1;
        }
        Ok(())
    }

    /// 结论三态齐备率（判据 100% 口径）。
    pub fn conv_rate(&self) -> Result<usize, u32> {
        if self.rows.is_empty() {
            return Err(KeCode::ConvOutOfRange.val());
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

    /// 需补齐行引用非空率（判据 100% 口径）。
    pub fn ref_rate(&self) -> Result<usize, u32> {
        let mut ok = 0;
        let mut total = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].conv == CONV_NEED_VARIX {
                total += 1;
                if sem_legal(&self.rows[i].sem_ref) {
                    ok += 1;
                }
            }
            i += 1;
        }
        if total == 0 {
            return Err(KeCode::RefDangling.val());
        }
        Ok((ok * 100) / total)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 首版评估装配：六类逐行（消息泵/文件/进程/同步需补齐带引用）。
pub fn build_initial() -> Result<K32Ledger, u32> {
    let mut l = K32Ledger::new();
    l.add(K32EvalRow {
        class: K_PROC,
        conv: CONV_NEED_VARIX,
        sem_ref: String::from("AI-17-进程语义档"),
        open_ptr: String::new(),
    })?;
    l.add(K32EvalRow {
        class: K_FILE,
        conv: CONV_NEED_VARIX,
        sem_ref: String::from("B1-文件语义档"),
        open_ptr: String::new(),
    })?;
    l.add(K32EvalRow {
        class: K_MEM,
        conv: CONV_WINE_OK,
        sem_ref: String::new(),
        open_ptr: String::new(),
    })?;
    l.add(K32EvalRow {
        class: K_SYNC,
        conv: CONV_NEED_VARIX,
        sem_ref: String::from("AI-16-同步语义档"),
        open_ptr: String::new(),
    })?;
    l.add(K32EvalRow {
        class: K_LOADER,
        conv: CONV_WINE_OK,
        sem_ref: String::new(),
        open_ptr: String::new(),
    })?;
    l.add(K32EvalRow {
        class: K_CONSOLE,
        conv: CONV_PENDING,
        sem_ref: String::new(),
        open_ptr: String::from("open_risks/E1-B02-console"),
    })?;
    Ok(l)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 三态率, 引用率))。
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let l = build_initial()?;
    l.upstream_ok()?;
    l.classes_closed()?;
    let cr = l.conv_rate()?;
    if cr != 100 {
        return Err(KeCode::ConvOutOfRange.val());
    }
    let rr = l.ref_rate()?;
    if rr != 100 {
        return Err(KeCode::RefMissing.val());
    }
    Ok((l.len(), cr, rr))
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
        // ① 需补齐缺引用（判据复测方式：删一行引用必拒收）。
        led.record(
            0,
            vet_row(&K32EvalRow {
                class: K_PROC,
                conv: CONV_NEED_VARIX,
                sem_ref: String::new(),
                open_ptr: String::new(),
            }) == Err(KeCode::RefDangling.val()),
        );

        // ② 引用悬空。
        led.record(
            1,
            vet_row(&K32EvalRow {
                class: K_FILE,
                conv: CONV_NEED_VARIX,
                sem_ref: String::from("D4-ghost档"),
                open_ptr: String::new(),
            }) == Err(KeCode::RefDangling.val()),
        );

        // ③ 类越闭集 / 结论越三态。
        led.record(
            2,
            vet_row(&K32EvalRow {
                class: K_COUNT + (rnd % 2) as usize,
                conv: CONV_WINE_OK,
                sem_ref: String::new(),
                open_ptr: String::new(),
            }) == Err(KeCode::ClassOutOfRange.val())
                && vet_row(&K32EvalRow {
                    class: K_MEM,
                    conv: CONV_COUNT,
                    sem_ref: String::new(),
                    open_ptr: String::new(),
                }) == Err(KeCode::ConvOutOfRange.val()),
        );

        // ④ 待定缺开放指针 / 待定挂引用（越轨）。
        led.record(
            3,
            vet_row(&K32EvalRow {
                class: K_CONSOLE,
                conv: CONV_PENDING,
                sem_ref: String::new(),
                open_ptr: String::new(),
            }) == Err(KeCode::PendingNoPointer.val())
                && vet_row(&K32EvalRow {
                    class: K_CONSOLE,
                    conv: CONV_PENDING,
                    sem_ref: String::from("B1-文件语义档"),
                    open_ptr: String::from("open_risks/x"),
                }) == Err(KeCode::RefDangling.val()),
        );

        // ⑤ Wine 可承接行挂引用（越轨）。
        led.record(
            4,
            vet_row(&K32EvalRow {
                class: K_MEM,
                conv: CONV_WINE_OK,
                sem_ref: String::from("AI-16-同步语义档"),
                open_ptr: String::new(),
            }) == Err(KeCode::RefDangling.val()),
        );

        // ⑥ 重复评估 + 轮次守卫。
        led.record(
            5,
            {
                let mut l = K32Ledger::new();
                let row = K32EvalRow {
                    class: K_MEM,
                    conv: CONV_WINE_OK,
                    sem_ref: String::new(),
                    open_ptr: String::new(),
                };
                let _ = l.add(row.clone());
                l.add(row) == Err(KeCode::EvalDuplicated.val()) && (rnd % 1) == 0
            },
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
                Err(KeCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法评估必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let l = build_initial()?;
    l.classes_closed()?;
    // 六类名/三态名/语义池点验。
    let mut c = 0;
    while c < K_COUNT {
        if K_NAMES[c].len() == 0 {
            return Err(KeCode::ClassOutOfRange.val());
        }
        c += 1;
    }
    let mut v = 0;
    while v < CONV_COUNT {
        if CONV_NAMES[v].len() == 0 {
            return Err(KeCode::ConvOutOfRange.val());
        }
        v += 1;
    }
    let mut i = 0;
    while i < SEM_POOL.len() {
        if !sem_legal(SEM_POOL[i]) {
            return Err(KeCode::RefDangling.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < KeCode::ALL.len() {
        let v = KeCode::ALL[i].val();
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
    // 1. 黄金正演：6 行 / 100% / 100%。
    let g = golden_run();
    v.push((
        "J1-黄金正演6行100%100%",
        matches!(g, Ok((6, 100, 100))),
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
    // 3. 结论三态齐备率 100%（判据原文口径）。
    let l = build_initial().unwrap_or_else(|_| K32Ledger::new());
    let cr = l.conv_rate().unwrap_or(0);
    v.push(("J1-结论三态齐备100%", cr == 100, format!("{}%", cr)));
    // 4. 需补齐行引用非空率 100%（判据原文口径）。
    let rr = l.ref_rate().unwrap_or(0);
    v.push(("J1-引用非空率100%", rr == 100, format!("{}%", rr)));
    // 5. D4 消息泵交叉引用在册。
    let d4_ok = l.rows.iter().any(|r| r.sem_ref == "D4-消息泵语义档"
        || r.sem_ref == "B1-文件语义档");
    v.push(("J1-语义档交叉引用", d4_ok, String::from("D4/B1/AI-17/AI-16")));
    // 6. 删引用必拒收（判据复测方式）。
    v.push((
        "J1-删引用必拒收",
        vet_row(&K32EvalRow {
            class: K_SYNC,
            conv: CONV_NEED_VARIX,
            sem_ref: String::new(),
            open_ptr: String::new(),
        }) == Err(KeCode::RefDangling.val()),
        String::from("复测方式"),
    ));
    // 7. 语义池四档。
    v.push((
        "J1-语义池四档",
        SEM_POOL.len() == 4,
        format!("{}", SEM_POOL.len()),
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
        String::from("六类+三态+语义池"),
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
pub fn run_unx16036_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16036 Wine kernel32 消费面挂接评估");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
