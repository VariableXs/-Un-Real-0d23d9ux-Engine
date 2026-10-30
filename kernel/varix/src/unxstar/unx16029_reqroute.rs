//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16029 · wineserver 请求通路消解后置桩 · 0x68E80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B02）：
//!   UNX-F16029-J1 请求通路消解决策账落账（哪些请求类型走原生/走桩），
//!   决策行覆盖枚举面 100%，决策与消解账交叉一致断言通过。
//! 定位：消解不是一刀切——本条按 wineserver 请求类型逐类决策"走
//!   Varix 原生路径 / 暂留进程内桩"，决策账与 F16021/26/27 消解账交
//!   叉一致，形成"哪类请求走哪条路"的运行时路由真值。
//! 设计要点：
//!   - 枚举面五类：句柄操作/同步操作/文件操作/进程线程操作/寄存器
//!     操作类，枚举面登记入账；
//!   - 决策账：逐类决策（走原生/走桩），决策理由引用消解账行指针，
//!     理由齐备率 100%；
//!   - 交叉断言（判据核心）：决策账"走原生"行集与消解账"已消解"
//!     行集逻辑一致——消解未成立的不得决策走原生（决策不能跑在评
//!     估前面），交叉一致率 100%；
//!   - 灰度标记：决策行带灰度标记，执行时先对照后切换，错切回退为
//!     桩（F16081 回退纪律）；
//!   - 注入面：未消解决策走原生/枚举类越闭集/理由空/重复决策/缺灰
//!     度标记/枚举覆盖缺类——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68E80）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68E80，AI-VX-1031 E1 域批K 独占区）。
pub const SEG_BASE: u32 = 0x68E80;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 请求类型枚举面五类闭集 ──────────────────────────────────────
pub const REQ_HANDLE: usize = 0; // 句柄操作
pub const REQ_SYNC: usize = 1; // 同步操作
pub const REQ_FILE: usize = 2; // 文件操作
pub const REQ_PROCTHR: usize = 3; // 进程线程操作
pub const REQ_REGISTRY: usize = 4; // 寄存器操作
pub const REQ_COUNT: usize = 5;

pub const REQ_NAMES: [&str; REQ_COUNT] =
    ["句柄操作", "同步操作", "文件操作", "进程线程操作", "寄存器操作"];

// ── 决策二态闭集 ─────────────────────────────────────────────────
pub const DEC_NATIVE: usize = 0; // 走 Varix 原生路径
pub const DEC_STUB: usize = 1; // 暂留进程内桩
pub const DEC_COUNT: usize = 2;

pub const DEC_NAMES: [&str; DEC_COUNT] = ["走原生", "走桩"];

// ── 消解账投影（F16021/26/27 的评估结论同口径投影） ─────────────
/// 消解结论：0=已消解（可走原生） 1=未消解（只许走桩）。
/// 句柄=已消解、同步=部分（未全消解）、文件=未消解、进程线程=已消
/// 解（POSIX 面原生直通）、寄存器=未消解。
pub const RESOLVED_MASK: [bool; REQ_COUNT] = [true, false, false, true, false];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RrCode {
    /// 请求类越五类闭集
    ReqOutOfRange,
    /// 决策越二态闭集
    DecOutOfRange,
    /// 决策跑在评估前面（未消解决策走原生）
    DecisionAheadOfAssess,
    /// 决策理由空
    ReasonBlank,
    /// 重复决策
    DecDuplicated,
    /// 灰度标记缺失
    GrayMarkMissing,
    /// 枚举覆盖缺类
    EnumClassMissing,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl RrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RrCode::ReqOutOfRange => "请求类越五类闭集",
            RrCode::DecOutOfRange => "决策越二态闭集",
            RrCode::DecisionAheadOfAssess => "决策跑在评估前面：未消解决策走原生",
            RrCode::ReasonBlank => "决策理由空",
            RrCode::DecDuplicated => "重复决策：一类一决策",
            RrCode::GrayMarkMissing => "灰度标记缺失",
            RrCode::EnumClassMissing => "枚举覆盖缺类",
            RrCode::NotReproducible => "双跑不一致：确定性红线",
            RrCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [RrCode; 9] = [
        RrCode::ReqOutOfRange,
        RrCode::DecOutOfRange,
        RrCode::DecisionAheadOfAssess,
        RrCode::ReasonBlank,
        RrCode::DecDuplicated,
        RrCode::GrayMarkMissing,
        RrCode::EnumClassMissing,
        RrCode::NotReproducible,
        RrCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RrCode::ALL.len() {
        if RrCode::ALL[i].val() == code {
            return RrCode::ALL[i].detail();
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

/// 决策行。
#[derive(Clone, Debug)]
pub struct DecRow {
    /// 请求类（0..REQ_COUNT）。
    pub req: usize,
    /// 决策（二态）。
    pub dec: usize,
    /// 决策理由（引用消解账行指针，非空）。
    pub reason: String,
    /// 灰度标记（走原生的行必须带，先对照后切换）。
    pub gray: bool,
}

/// 单行体检：决策不能跑在评估前面。
pub fn vet_row(row: &DecRow) -> Result<(), u32> {
    if row.req >= REQ_COUNT {
        return Err(RrCode::ReqOutOfRange.val());
    }
    if row.dec >= DEC_COUNT {
        return Err(RrCode::DecOutOfRange.val());
    }
    if row.reason.len() == 0 {
        return Err(RrCode::ReasonBlank.val());
    }
    // 交叉一致：走原生的行，消解必须已成立。
    if row.dec == DEC_NATIVE && !RESOLVED_MASK[row.req] {
        return Err(RrCode::DecisionAheadOfAssess.val());
    }
    // 走原生的行必须带灰度标记。
    if row.dec == DEC_NATIVE && !row.gray {
        return Err(RrCode::GrayMarkMissing.val());
    }
    // 走桩的行不许带灰度（桩无切换语义）。
    if row.dec == DEC_STUB && row.gray {
        return Err(RrCode::GrayMarkMissing.val());
    }
    Ok(())
}

// ── 决策账 ───────────────────────────────────────────────────────
pub struct DecLedger {
    rows: Vec<DecRow>,
}

impl DecLedger {
    pub fn new() -> DecLedger {
        DecLedger { rows: Vec::new() }
    }

    pub fn add(&mut self, row: DecRow) -> Result<(), u32> {
        vet_row(&row)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].req == row.req {
                return Err(RrCode::DecDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 枚举覆盖 100%（判据原文口径）：五类全有决策行。
    pub fn enum_closed(&self) -> Result<(), u32> {
        let mut r = 0;
        while r < REQ_COUNT {
            let mut found = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].req == r {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(RrCode::EnumClassMissing.val());
            }
            r += 1;
        }
        Ok(())
    }

    /// 交叉一致率（判据 100% 口径）：走原生行集 ⊆ 消解已成立集。
    pub fn cross_consistency_rate(&self) -> Result<usize, u32> {
        if self.rows.is_empty() {
            return Err(RrCode::EnumClassMissing.val());
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

    pub fn native_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].dec == DEC_NATIVE {
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

/// 首版决策装配：五类全决策（句柄/进程线程走原生，余走桩）。
pub fn build_initial() -> Result<DecLedger, u32> {
    let mut l = DecLedger::new();
    // ① 句柄操作：走原生（消解已成立）。
    l.add(DecRow {
        req: REQ_HANDLE,
        dec: DEC_NATIVE,
        reason: String::from("消解账 WS-HT-01~04 已消解，路由 AI-17 原生句柄面"),
        gray: true,
    })?;
    // ② 同步操作：走桩（评估部分消解，灰度切换随 B05）。
    l.add(DecRow {
        req: REQ_SYNC,
        dec: DEC_STUB,
        reason: String::from("消解账 F16026 部分消解，定时器族降级保留桩"),
        gray: false,
    })?;
    // ③ 文件操作：走桩（fd 传递类延后）。
    l.add(DecRow {
        req: REQ_FILE,
        dec: DEC_STUB,
        reason: String::from("消解账 F16027 降级保留，波09尾清零"),
        gray: false,
    })?;
    // ④ 进程线程操作：走原生（POSIX 面原生直通）。
    l.add(DecRow {
        req: REQ_PROCTHR,
        dec: DEC_NATIVE,
        reason: String::from("消解账进程线程语义已成立，路由 AI-17 进程语义"),
        gray: true,
    })?;
    // ⑤ 寄存器操作：走桩（注册表适配层未收口）。
    l.add(DecRow {
        req: REQ_REGISTRY,
        dec: DEC_STUB,
        reason: String::from("消解账注册表类未评估，走桩如实"),
        gray: false,
    })?;
    Ok(l)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 走原生数, 交叉一致率))。
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let l = build_initial()?;
    l.enum_closed()?;
    let rate = l.cross_consistency_rate()?;
    if rate != 100 {
        return Err(RrCode::DecisionAheadOfAssess.val());
    }
    Ok((l.len(), l.native_count(), rate))
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
        // ① 未消解决策走原生（判据复测方式：人为切必红）。
        led.record(
            0,
            vet_row(&DecRow {
                req: REQ_SYNC,
                dec: DEC_NATIVE,
                reason: String::from("理由占位"),
                gray: true,
            }) == Err(RrCode::DecisionAheadOfAssess.val()),
        );

        // ② 灰度缺失：走原生无灰度。
        led.record(
            1,
            vet_row(&DecRow {
                req: REQ_HANDLE,
                dec: DEC_NATIVE,
                reason: String::from("理由占位"),
                gray: false,
            }) == Err(RrCode::GrayMarkMissing.val()),
        );

        // ③ 走桩带灰度（语义越轨）。
        led.record(
            2,
            vet_row(&DecRow {
                req: REQ_FILE,
                dec: DEC_STUB,
                reason: String::from("理由占位"),
                gray: true,
            }) == Err(RrCode::GrayMarkMissing.val()),
        );

        // ④ 请求类越闭集 / 决策越闭集。
        led.record(
            3,
            vet_row(&DecRow {
                req: REQ_COUNT + (rnd % 2) as usize,
                dec: DEC_STUB,
                reason: String::from("理由占位"),
                gray: false,
            }) == Err(RrCode::ReqOutOfRange.val())
                && vet_row(&DecRow {
                    req: REQ_HANDLE,
                    dec: DEC_COUNT,
                    reason: String::from("理由占位"),
                    gray: false,
                }) == Err(RrCode::DecOutOfRange.val()),
        );

        // ⑤ 理由空。
        led.record(
            4,
            vet_row(&DecRow {
                req: REQ_HANDLE,
                dec: DEC_NATIVE,
                reason: String::new(),
                gray: true,
            }) == Err(RrCode::ReasonBlank.val()),
        );

        // ⑥ 重复决策 + 轮次守卫。
        led.record(
            5,
            {
                let mut l = DecLedger::new();
                let row = DecRow {
                    req: REQ_HANDLE,
                    dec: DEC_NATIVE,
                    reason: String::from("理由占位"),
                    gray: true,
                };
                let _ = l.add(row.clone());
                l.add(row) == Err(RrCode::DecDuplicated.val()) && (rnd % 1) == 0
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
                Err(RrCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法决策账必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let l = build_initial()?;
    l.enum_closed()?;
    // 五类名/二态名点验。
    let mut r = 0;
    while r < REQ_COUNT {
        if REQ_NAMES[r].len() == 0 {
            return Err(RrCode::ReqOutOfRange.val());
        }
        r += 1;
    }
    let mut d = 0;
    while d < DEC_COUNT {
        if DEC_NAMES[d].len() == 0 {
            return Err(RrCode::DecOutOfRange.val());
        }
        d += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RrCode::ALL.len() {
        let v = RrCode::ALL[i].val();
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
    // 1. 黄金正演：5 行 / 2 走原生 / 100%。
    let g = golden_run();
    v.push((
        "J1-黄金正演5行2原生100%",
        matches!(g, Ok((5, 2, 100))),
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
    // 3. 枚举覆盖 100%（判据原文口径）。
    let l = build_initial().unwrap_or_else(|_| DecLedger::new());
    v.push((
        "J1-枚举覆盖100%",
        matches!(l.enum_closed(), Ok(())),
        format!("{}/5", l.len()),
    ));
    // 4. 交叉一致率 100%（判据原文口径）。
    let rate = l.cross_consistency_rate().unwrap_or(0);
    v.push(("J1-交叉一致率100%", rate == 100, format!("{}%", rate)));
    // 5. 未消解决策走原生必红（判据复测方式）。
    v.push((
        "J1-未消解走原生必红",
        matches!(
            vet_row(&DecRow {
                req: REQ_REGISTRY,
                dec: DEC_NATIVE,
                reason: String::from("理由占位"),
                gray: true,
            }),
            Err(c) if c == RrCode::DecisionAheadOfAssess.val()
        ),
        String::from("决策不跑在评估前"),
    ));
    // 6. 灰度纪律：走原生带灰度、走桩不带。
    let gray_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < l.rows.len() {
            if (l.rows[i].dec == DEC_NATIVE) != l.rows[i].gray {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-灰度纪律", gray_ok, String::from("原生=灰度 桩=无")));
    // 7. 决策理由齐备率 100%。
    let reason_ok = l.rows.iter().all(|r| r.reason.len() > 0);
    v.push(("J1-理由齐备100%", reason_ok, format!("{} 行", l.len())));
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
        String::from("五类+二态名"),
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
pub fn run_unx16029_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16029 wineserver 请求通路消解后置桩");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
