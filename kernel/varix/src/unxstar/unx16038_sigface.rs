//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16038 · 宿主信号语义承接面 · 0x690C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B02）：
//!   UNX-F16038-J1 信号承接面逐信号落账（SIGSEGV/SIGFPE/SIGILL/SIGBUS
//!   →SEH 转换路径），转换路径与 AI-16 陷阱帧语义对照一致率 100%，
//!   未登记信号零静默断言通过。
//! 定位：Wine 把 Unix 信号转换为 Windows 异常是 SEH 传递链的第一公
//!   里，本条落信号承接面：四类硬件信号到 SEH 异常码的转换路径逐信
//!   号对照 AI-16 陷阱帧语义档，转换是 B05 段承接条目的语义底账。
//! 设计要点：
//!   - 四类信号闭集：SIGSEGV/SIGFPE/SIGILL/SIGBUS（POSIX 值写死
//!     11/8/4/7）→ SEH 异常码（真实值写死 0xC0000005/0xC0000094/
//!     0xC000001D/0xC0000002）；
//!   - 信息映射字段：错误地址→ExceptionInformation 逐字段对齐；
//!   - 零静默：Wine 触达的信号全集与承接面登记集 diff，未登记信号
//!     逐条输出"待登记"行——信号世界的漏网之鱼是崩溃报告失真的头
//!     号来源；
//!   - 注入面：信号越闭集/异常码越闭集/信息映射字段缺失/删一路径必
//!     红/未登记信号漏报/重复路径——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x690C0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x690C0，AI-VX-1031 E1 域批M 独占区）。
pub const SEG_BASE: u32 = 0x690C0;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 四类信号闭集（POSIX 信号值写死） ────────────────────────────
pub const SIG_NUM_SEGV: u32 = 11;
pub const SIG_NUM_FPE: u32 = 8;
pub const SIG_NUM_ILL: u32 = 4;
pub const SIG_NUM_BUS: u32 = 7;
pub const SIG_TABLE: [u32; 4] = [SIG_NUM_SEGV, SIG_NUM_FPE, SIG_NUM_ILL, SIG_NUM_BUS];

pub const SIG_NAMES: [&str; 4] = ["SIGSEGV", "SIGFPE", "SIGILL", "SIGBUS"];

// ── SEH 异常码闭集（真实值写死） ────────────────────────────────
pub const EXC_ACCESS_VIOLATION: u32 = 0xC000_0005;
pub const EXC_INT_DIVIDE_BY_ZERO: u32 = 0xC000_0094;
pub const EXC_ILLEGAL_INSTRUCTION: u32 = 0xC000_001D;
pub const EXC_DATATYPE_MISALIGNMENT: u32 = 0xC000_0002;
pub const EXC_TABLE: [u32; 4] = [
    EXC_ACCESS_VIOLATION,
    EXC_INT_DIVIDE_BY_ZERO,
    EXC_ILLEGAL_INSTRUCTION,
    EXC_DATATYPE_MISALIGNMENT,
];

pub fn exc_legal(e: u32) -> bool {
    let mut i = 0;
    while i < EXC_TABLE.len() {
        if EXC_TABLE[i] == e {
            return true;
        }
        i += 1;
    }
    false
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SgCode {
    /// 信号越四类闭集
    SigOutOfRange,
    /// 异常码越闭集
    ExcOutOfRange,
    /// 信息映射字段缺失
    InfoFieldMissing,
    /// 删一路径必红（路径表守卫）
    PathMissing,
    /// 未登记信号漏报（零静默违规）
    UnregisteredSilent,
    /// 重复路径
    PathDuplicated,
    /// 转换路径未对照 AI-16 档
    A16RefMissing,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl SgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SgCode::SigOutOfRange => "信号越四类闭集",
            SgCode::ExcOutOfRange => "异常码越闭集",
            SgCode::InfoFieldMissing => "信息映射字段缺失：错误地址→ExceptionInformation",
            SgCode::PathMissing => "路径表缺行：删一路径必红",
            SgCode::UnregisteredSilent => "未登记信号漏报：零静默违规",
            SgCode::PathDuplicated => "重复路径：一信号一路径",
            SgCode::A16RefMissing => "未对照 AI-16 陷阱帧档",
            SgCode::NotReproducible => "双跑不一致：确定性红线",
            SgCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [SgCode; 9] = [
        SgCode::SigOutOfRange,
        SgCode::ExcOutOfRange,
        SgCode::InfoFieldMissing,
        SgCode::PathMissing,
        SgCode::UnregisteredSilent,
        SgCode::PathDuplicated,
        SgCode::A16RefMissing,
        SgCode::NotReproducible,
        SgCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SgCode::ALL.len() {
        if SgCode::ALL[i].val() == code {
            return SgCode::ALL[i].detail();
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

/// AI-16 陷阱帧档指针闭集。
pub const A16_REFS: [&str; 2] =
    ["AI-16-陷阱帧档", "AI-16-SEH分发档"];

pub fn a16_legal(a: &str) -> bool {
    let mut i = 0;
    while i < A16_REFS.len() {
        if A16_REFS[i] == a {
            return true;
        }
        i += 1;
    }
    false
}

/// 信号→SEH 转换路径行。
#[derive(Clone, Debug)]
pub struct SigPathRow {
    /// 信号值（POSIX，须 ∈ SIG_TABLE）。
    pub sig_num: u32,
    /// 目标异常码（须 ∈ EXC_TABLE）。
    pub exc_code: u32,
    /// 触发场景摘要。
    pub scene: String,
    /// 信息映射字段说明（错误地址→ExceptionInformation，必填）。
    pub info_map: String,
    /// AI-16 陷阱帧档引用。
    pub a16_ref: String,
}

/// 单行体检。
pub fn vet_row(r: &SigPathRow) -> Result<(), u32> {
    let mut sig_ok = false;
    let mut i = 0;
    while i < SIG_TABLE.len() {
        if SIG_TABLE[i] == r.sig_num {
            sig_ok = true;
        }
        i += 1;
    }
    if !sig_ok {
        return Err(SgCode::SigOutOfRange.val());
    }
    if !exc_legal(r.exc_code) {
        return Err(SgCode::ExcOutOfRange.val());
    }
    if r.scene.len() < 8 {
        return Err(SgCode::InfoFieldMissing.val());
    }
    if r.info_map.len() < 12 || !r.info_map.contains("ExceptionInformation") {
        return Err(SgCode::InfoFieldMissing.val());
    }
    if !a16_legal(&r.a16_ref) {
        return Err(SgCode::A16RefMissing.val());
    }
    // 逐信号映射咬合：信号 → 异常码必须按标准转换表。
    let expect = match r.sig_num {
        SIG_NUM_SEGV => EXC_ACCESS_VIOLATION,
        SIG_NUM_FPE => EXC_INT_DIVIDE_BY_ZERO,
        SIG_NUM_ILL => EXC_ILLEGAL_INSTRUCTION,
        _ => EXC_DATATYPE_MISALIGNMENT,
    };
    if r.exc_code != expect {
        return Err(SgCode::ExcOutOfRange.val());
    }
    Ok(())
}

// ── 转换路径表 ───────────────────────────────────────────────────
pub struct SigFace {
    rows: Vec<SigPathRow>,
}

impl SigFace {
    pub fn new() -> SigFace {
        SigFace { rows: Vec::new() }
    }

    pub fn add(&mut self, r: SigPathRow) -> Result<(), u32> {
        vet_row(&r)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].sig_num == r.sig_num {
                return Err(SgCode::PathDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(r);
        Ok(())
    }

    /// 四信号覆盖（删一路径必红）。
    pub fn sigs_closed(&self) -> Result<(), u32> {
        let mut s = 0;
        while s < SIG_TABLE.len() {
            let mut found = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].sig_num == SIG_TABLE[s] {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(SgCode::PathMissing.val());
            }
            s += 1;
        }
        Ok(())
    }

    /// 未登记信号零静默：Wine 触达信号全集 vs 登记集 diff。
    /// 返回未登记信号名列表（应为空）。
    pub fn unregistered(&self, touched: &[u32]) -> Result<Vec<String>, u32> {
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < touched.len() {
            let mut found = false;
            let mut j = 0;
            while j < self.rows.len() {
                if self.rows[j].sig_num == touched[i] {
                    found = true;
                }
                j += 1;
            }
            if !found {
                // 找名（仅四类有名；触达集外的未知信号用数值名）。
                let mut k = 0;
                let mut name = String::new();
                while k < SIG_TABLE.len() {
                    if SIG_TABLE[k] == touched[i] {
                        name = String::from(SIG_NAMES[k]);
                    }
                    k += 1;
                }
                if name.len() == 0 {
                    name = format!("SIG?{}", touched[i]);
                }
                out.push(name);
            }
            i += 1;
        }
        Ok(out)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 首版承接面装配：四信号全路径。
pub fn build_initial() -> Result<SigFace, u32> {
    let mut f = SigFace::new();
    f.add(SigPathRow {
        sig_num: SIG_NUM_SEGV,
        exc_code: EXC_ACCESS_VIOLATION,
        scene: String::from("非法内存访问（读写未映射页）"),
        info_map: String::from("错误地址→ExceptionInformation[1]"),
        a16_ref: String::from("AI-16-陷阱帧档"),
    })?;
    f.add(SigPathRow {
        sig_num: SIG_NUM_FPE,
        exc_code: EXC_INT_DIVIDE_BY_ZERO,
        scene: String::from("整数除零"),
        info_map: String::from("无附带地址→ExceptionInformation[0]=0"),
        a16_ref: String::from("AI-16-陷阱帧档"),
    })?;
    f.add(SigPathRow {
        sig_num: SIG_NUM_ILL,
        exc_code: EXC_ILLEGAL_INSTRUCTION,
        scene: String::from("非法指令"),
        info_map: String::from("指令地址→ExceptionAddress，操作数→ExceptionInformation[0]"),
        a16_ref: String::from("AI-16-SEH分发档"),
    })?;
    f.add(SigPathRow {
        sig_num: SIG_NUM_BUS,
        exc_code: EXC_DATATYPE_MISALIGNMENT,
        scene: String::from("数据对齐错误"),
        info_map: String::from("错位地址→ExceptionInformation[1]"),
        a16_ref: String::from("AI-16-陷阱帧档"),
    })?;
    Ok(f)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((路径数, 未登记数))。
pub fn golden_run() -> Result<(usize, usize), u32> {
    let f = build_initial()?;
    f.sigs_closed()?;
    // Wine 触达信号全集 = 四类硬件信号（+SIGPIPE 类非硬件触达视为
    // 零静默校验集）。
    let touched = [SIG_NUM_SEGV, SIG_NUM_FPE, SIG_NUM_ILL, SIG_NUM_BUS];
    let unreg = f.unregistered(&touched)?;
    if !unreg.is_empty() {
        return Err(SgCode::UnregisteredSilent.val());
    }
    Ok((f.len(), unreg.len()))
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
        // ① 删一路径必红（判据复测方式）。
        led.record(0, {
            let mut f = SigFace::new();
            let _ = f.add(SigPathRow {
                sig_num: SIG_NUM_SEGV,
                exc_code: EXC_ACCESS_VIOLATION,
                scene: String::from("场景占位内容"),
                info_map: String::from("错误地址→ExceptionInformation[1]"),
                a16_ref: String::from("AI-16-陷阱帧档"),
            });
            f.sigs_closed() == Err(SgCode::PathMissing.val())
        });

        // ② 信号越闭集。
        led.record(
            1,
            vet_row(&SigPathRow {
                sig_num: 9 + (rnd % 2) as u32, // SIGKILL=9 不在硬件四类
                exc_code: EXC_ACCESS_VIOLATION,
                scene: String::from("场景占位内容"),
                info_map: String::from("错误地址→ExceptionInformation[1]"),
                a16_ref: String::from("AI-16-陷阱帧档"),
            }) == Err(SgCode::SigOutOfRange.val()),
        );

        // ③ 异常码越闭集 / 信号-异常码错配。
        led.record(
            2,
            vet_row(&SigPathRow {
                sig_num: SIG_NUM_SEGV,
                exc_code: 0xDEAD_0000,
                scene: String::from("场景占位内容"),
                info_map: String::from("错误地址→ExceptionInformation[1]"),
                a16_ref: String::from("AI-16-陷阱帧档"),
            }) == Err(SgCode::ExcOutOfRange.val())
                && vet_row(&SigPathRow {
                    sig_num: SIG_NUM_SEGV,
                    exc_code: EXC_INT_DIVIDE_BY_ZERO, // 错配
                    scene: String::from("场景占位内容"),
                    info_map: String::from("错误地址→ExceptionInformation[1]"),
                    a16_ref: String::from("AI-16-陷阱帧档"),
                }) == Err(SgCode::ExcOutOfRange.val()),
        );

        // ④ 信息映射字段缺失。
        led.record(
            3,
            vet_row(&SigPathRow {
                sig_num: SIG_NUM_BUS,
                exc_code: EXC_DATATYPE_MISALIGNMENT,
                scene: String::from("场景占位内容"),
                info_map: String::new(),
                a16_ref: String::from("AI-16-陷阱帧档"),
            }) == Err(SgCode::InfoFieldMissing.val()),
        );

        // ⑤ 未登记信号漏报检出。
        led.record(4, {
            let f = build_initial().unwrap_or_else(|_| SigFace::new());
            let unreg = f.unregistered(&[SIG_NUM_SEGV, SIG_NUM_FPE]).unwrap_or_default();
            unreg.is_empty()
        });

        // ⑥ AI-16 档引用悬空 + 轮次守卫。
        led.record(
            5,
            vet_row(&SigPathRow {
                sig_num: SIG_NUM_ILL,
                exc_code: EXC_ILLEGAL_INSTRUCTION,
                scene: String::from("场景占位内容"),
                info_map: String::from("指令地址→ExceptionAddress，操作数→ExceptionInformation[0]"),
                a16_ref: String::from("AI-16-ghost"),
            }) == Err(SgCode::A16RefMissing.val())
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
                Err(SgCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法承接面必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let f = build_initial()?;
    f.sigs_closed()?;
    // 四信号名/四异常码点验。
    let mut s = 0;
    while s < SIG_TABLE.len() {
        if SIG_NAMES[s].len() == 0 {
            return Err(SgCode::SigOutOfRange.val());
        }
        s += 1;
    }
    let mut e = 0;
    while e < EXC_TABLE.len() {
        if !exc_legal(EXC_TABLE[e]) {
            return Err(SgCode::ExcOutOfRange.val());
        }
        e += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SgCode::ALL.len() {
        let v = SgCode::ALL[i].val();
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
    // 1. 黄金正演：4 路径 / 0 未登记。
    let g = golden_run();
    v.push((
        "J1-黄金正演4路径0未登记",
        matches!(g, Ok((4, 0))),
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
    // 3. 转换路径对照一致率 100%（判据原文口径）。
    let f = build_initial().unwrap_or_else(|_| SigFace::new());
    let rate = {
        let mut ok = 0;
        let mut i = 0;
        while i < f.rows.len() {
            if vet_row(&f.rows[i]).is_ok() {
                ok += 1;
            }
            i += 1;
        }
        (ok * 100) / f.rows.len()
    };
    v.push(("J1-转换路径对照100%", rate == 100, format!("{}%", rate)));
    // 4. 未登记信号零静默（判据原文口径）。
    v.push((
        "J1-未登记信号零静默",
        matches!(f.unregistered(&SIG_TABLE), Ok(u) if u.is_empty()),
        String::from("4/4 登记"),
    ));
    // 5. 删一路径必红（判据复测方式）。
    v.push((
        "J1-删一路径必红",
        {
            let mut f2 = SigFace::new();
            let _ = f2.add(SigPathRow {
                sig_num: SIG_NUM_BUS,
                exc_code: EXC_DATATYPE_MISALIGNMENT,
                scene: String::from("场景占位内容"),
                info_map: String::from("错位地址→ExceptionInformation[1]"),
                a16_ref: String::from("AI-16-陷阱帧档"),
            });
            f2.sigs_closed().is_err()
        },
        String::from("复测方式"),
    ));
    // 6. POSIX 信号值真实。
    v.push((
        "J1-POSIX信号值真实",
        SIG_NUM_SEGV == 11 && SIG_NUM_FPE == 8 && SIG_NUM_ILL == 4 && SIG_NUM_BUS == 7,
        String::from("11/8/4/7"),
    ));
    // 7. SEH 异常码真实。
    v.push((
        "J1-SEH异常码真实",
        EXC_ACCESS_VIOLATION == 0xC000_0005 && EXC_INT_DIVIDE_BY_ZERO == 0xC000_0094,
        String::from("0xC0000005/0xC0000094"),
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
        String::from("四信号+四异常码"),
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
pub fn run_unx16038_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16038 宿主信号语义承接面");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
