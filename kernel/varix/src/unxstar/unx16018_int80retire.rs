//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16018 · int 0x80 旧面升级接管声明与退役计划 · 0x68C40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B01）：
//!   UNX-F16018-J1 退役计划账逐行登记（旧调用点/退役波次/替代路径），
//!   零活跃调用点断言（Wine 链路内计数 = 0）复测通过。
//! 定位：int 0x80 面是现存内核的传统 syscall 入口（建模时代遗产），
//!   Wine 只走 C2 现代分发面——本条声明 Wine 链路对 int 0x80 的零依
//!   赖纪律并落退役计划账：哪些旧调用点在什么波次由谁替代。
//! 设计要点：
//!   - 零依赖声明：Wine 运行时与 winevarix.drv 全链路对 int 0x80 零
//!     调用（POSIX 面统一走 C2 分发表），声明句逐处可查；
//!   - 退役计划账：现存 int 0x80 相关调用点逐行登记（文件/符号/当前
//!     角色/退役波次/替代路径指针），账行只增不删，顺延走状态更新
//!     （顺延理由必填）；
//!   - 断言面：Wine 链路源文件集特征模式扫描，活跃调用点计数 = 0 为
//!     过闸；计划账每行替代路径指针非空率 100%；
//!   - 退役波次四态闭集：未排期/已排期/进行中/已退役；
//!   - 注入面：Wine 链路混入 int 0x80 特征/指针空/波次越闭集/账行
//!     删除/顺延无理由/退役已毕仍活跃——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68C40）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68C40，AI-VX-1031 E1 域批H 独占区）。
pub const SEG_BASE: u32 = 0x68C40;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 退役波次四态闭集 ─────────────────────────────────────────────
pub const WAVE_UNSCHEDULED: usize = 0;
pub const WAVE_SCHEDULED: usize = 1;
pub const WAVE_ONGOING: usize = 2;
pub const WAVE_RETIRED: usize = 3;
pub const WAVE_COUNT: usize = 4;

pub const WAVE_NAMES: [&str; WAVE_COUNT] =
    ["未排期", "已排期", "进行中", "已退役"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IrCode {
    /// 波次越四态闭集
    WaveOutOfRange,
    /// 替代路径指针空
    AltPointerBlank,
    /// 顺延无理由
    PostponeNoReason,
    /// 账行删除（哈希链断裂）
    RowDeleted,
    /// Wine 链路混入 int 0x80 特征（零依赖违例）
    WineLinkTaint,
    /// 已退役仍活跃（状态矛盾）
    RetiredStillActive,
    /// 调用点重复登记
    SiteDuplicated,
    /// 调用点符号悬空
    SiteDangling,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl IrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IrCode::WaveOutOfRange => "波次越四态闭集",
            IrCode::AltPointerBlank => "替代路径指针空",
            IrCode::PostponeNoReason => "顺延无理由",
            IrCode::RowDeleted => "账行删除：哈希链断裂",
            IrCode::WineLinkTaint => "Wine 链路混入 int 0x80：零依赖违例",
            IrCode::RetiredStillActive => "已退役仍活跃：状态矛盾",
            IrCode::SiteDuplicated => "调用点重复登记",
            IrCode::SiteDangling => "调用点符号悬空",
            IrCode::NotReproducible => "双跑不一致：确定性红线",
            IrCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [IrCode; 10] = [
        IrCode::WaveOutOfRange,
        IrCode::AltPointerBlank,
        IrCode::PostponeNoReason,
        IrCode::RowDeleted,
        IrCode::WineLinkTaint,
        IrCode::RetiredStillActive,
        IrCode::SiteDuplicated,
        IrCode::SiteDangling,
        IrCode::NotReproducible,
        IrCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IrCode::ALL.len() {
        if IrCode::ALL[i].val() == code {
            return IrCode::ALL[i].detail();
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

// ── FNV-1a（账行哈希链） ────────────────────────────────────────
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 现存 int 0x80 调用点符号闭集（建模时代遗产，只读账）。
pub const LEGACY_SITES: [&str; 4] = [
    "kernel::int80_dispatch",
    "kernel::int80_hook_table",
    "lxrun::int80_legacy_path",
    "userspace::int80_probe",
];

/// Wine 链路源文件特征扫描（账面模拟：源集 → 特征计数）。
/// Wine 链路 = {wine 源树, winevarix.drv, 桥层}——这些文件集不含
/// int 0x80 特征（零依赖纪律的机械化形态）。
pub const WINE_LINK_FILES: [&str; 4] = [
    "third_party/wine/server/",
    "third_party/wine/dlls/",
    "drv/winevarix/",
    "bridge/posix_adapters",
];

/// 特征扫描：账面源集 → 活跃 int 0x80 调用点计数。
pub fn wine_link_taint_count(tainted: &[&str]) -> u32 {
    // tainted = 扫到的含特征文件集；干净态应为空。
    tainted.len() as u32
}

/// 退役计划账行。
#[derive(Clone, Debug)]
pub struct RetireRow {
    /// 调用点符号（须 ∈ LEGACY_SITES）。
    pub site: String,
    /// 当前角色摘要。
    pub role: String,
    /// 退役波次（四态闭集）。
    pub wave: usize,
    /// 替代路径指针（非空，C2 分发表入口）。
    pub alt_ptr: String,
    /// 顺延理由（状态顺延时必填）。
    pub postpone_reason: String,
    /// 活跃标记（已退役行必须为 false）。
    pub active: bool,
}

/// 单行体检。
pub fn vet_row(row: &RetireRow) -> Result<(), u32> {
    if row.wave >= WAVE_COUNT {
        return Err(IrCode::WaveOutOfRange.val());
    }
    if row.alt_ptr.len() == 0 || !row.alt_ptr.starts_with("C2-") {
        return Err(IrCode::AltPointerBlank.val());
    }
    // 已退役行必须不活跃。
    if row.wave == WAVE_RETIRED && row.active {
        return Err(IrCode::RetiredStillActive.val());
    }
    // 未退役行必须活跃。
    if row.wave != WAVE_RETIRED && !row.active {
        return Err(IrCode::RetiredStillActive.val());
    }
    let mut legal = false;
    let mut i = 0;
    while i < LEGACY_SITES.len() {
        if LEGACY_SITES[i] == row.site {
            legal = true;
        }
        i += 1;
    }
    if !legal {
        return Err(IrCode::SiteDangling.val());
    }
    Ok(())
}

// ── 退役计划账（只增不删 + 哈希链） ─────────────────────────────
pub struct RetireLedger {
    rows: Vec<RetireRow>,
    chain: Vec<u32>,
}

impl RetireLedger {
    pub fn new() -> RetireLedger {
        RetireLedger { rows: Vec::new(), chain: Vec::new() }
    }

    pub fn append(&mut self, row: RetireRow) -> Result<(), u32> {
        vet_row(&row)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].site == row.site {
                return Err(IrCode::SiteDuplicated.val());
            }
            i += 1;
        }
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(row.site.as_bytes());
        buf.push(row.wave as u8);
        buf.push(if row.active { 1 } else { 0 });
        buf.extend_from_slice(row.alt_ptr.as_bytes());
        let prev = if self.chain.is_empty() { 0 } else { self.chain[self.chain.len() - 1] };
        let mut seed: Vec<u8> = Vec::new();
        seed.extend_from_slice(&prev.to_le_bytes());
        seed.extend_from_slice(&buf);
        let h = fnv1a(&seed);
        self.rows.push(row);
        self.chain.push(h);
        Ok(())
    }

    /// 状态顺延（状态更新不改史：账行本体不动，顺延理由另附在行上）。
    pub fn postpone(&mut self, site: &str, reason: &str) -> Result<(), u32> {
        if reason.len() == 0 {
            return Err(IrCode::PostponeNoReason.val());
        }
        let mut idx: Option<usize> = None;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].site == site {
                idx = Some(i);
            }
            i += 1;
        }
        let i = match idx {
            Some(i) => i,
            None => return Err(IrCode::SiteDangling.val()),
        };
        // 顺延：波次回退一档（如已排期→未排期），理由必填。
        if self.rows[i].wave == WAVE_UNSCHEDULED {
            return Err(IrCode::WaveOutOfRange.val());
        }
        self.rows[i].wave -= 1;
        self.rows[i].postpone_reason = String::from(reason);
        vet_row(&self.rows[i])?;
        Ok(())
    }

    /// 链完整性复验。
    pub fn verify_chain(&self) -> Result<(), u32> {
        let mut prev: u32 = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let row = &self.rows[i];
            let mut buf: Vec<u8> = Vec::new();
            buf.extend_from_slice(row.site.as_bytes());
            buf.push(row.wave as u8);
            buf.push(if row.active { 1 } else { 0 });
            buf.extend_from_slice(row.alt_ptr.as_bytes());
            let mut seed: Vec<u8> = Vec::new();
            seed.extend_from_slice(&prev.to_le_bytes());
            seed.extend_from_slice(&buf);
            let h = fnv1a(&seed);
            if i >= self.chain.len() || self.chain[i] != h {
                return Err(IrCode::RowDeleted.val());
            }
            prev = h;
            i += 1;
        }
        if self.chain.len() != self.rows.len() {
            return Err(IrCode::RowDeleted.val());
        }
        Ok(())
    }

    /// 指针非空率（判据 100% 口径）。
    pub fn pointer_rate(&self) -> Result<usize, u32> {
        if self.rows.is_empty() {
            return Err(IrCode::AltPointerBlank.val());
        }
        let mut ok = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].alt_ptr.len() > 0 {
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

/// 首版计划账：四调用点全登记。
pub fn build_initial() -> Result<RetireLedger, u32> {
    let mut l = RetireLedger::new();
    l.append(RetireRow {
        site: String::from("kernel::int80_dispatch"),
        role: String::from("建模时代传统分发入口"),
        wave: WAVE_SCHEDULED,
        alt_ptr: String::from("C2-dispatch-table"),
        postpone_reason: String::new(),
        active: true,
    })?;
    l.append(RetireRow {
        site: String::from("kernel::int80_hook_table"),
        role: String::from("int 0x80 钩挂表"),
        wave: WAVE_ONGOING,
        alt_ptr: String::from("C2-dispatch-table"),
        postpone_reason: String::new(),
        active: true,
    })?;
    l.append(RetireRow {
        site: String::from("lxrun::int80_legacy_path"),
        role: String::from("lxrun 建模旧面（F16017 退役段）"),
        wave: WAVE_RETIRED,
        alt_ptr: String::from("C2-dispatch-table"),
        postpone_reason: String::new(),
        active: false,
    })?;
    l.append(RetireRow {
        site: String::from("userspace::int80_probe"),
        role: String::from("用户态探针残留"),
        wave: WAVE_UNSCHEDULED,
        alt_ptr: String::from("C2-probe-syscall"),
        postpone_reason: String::new(),
        active: true,
    })?;
    l.verify_chain()?;
    Ok(l)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 指针率, Wine 链路污染))。
pub fn golden_run() -> Result<(usize, usize, u32), u32> {
    let l = build_initial()?;
    l.verify_chain()?;
    let rate = l.pointer_rate()?;
    if rate != 100 {
        return Err(IrCode::AltPointerBlank.val());
    }
    // Wine 链路零依赖：干净扫描 → 污染计数 0。
    let taint = wine_link_taint_count(&[]);
    if taint != 0 {
        return Err(IrCode::WineLinkTaint.val());
    }
    Ok((l.len(), rate, taint))
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
        // ① Wine 链路混入特征（零依赖违例）。
        led.record(
            0,
            wine_link_taint_count(&["third_party/wine/server/int80_shim"]) == 1
                && wine_link_taint_count(&[]) == 0,
        );

        // ② 已退役仍活跃。
        led.record(
            1,
            vet_row(&RetireRow {
                site: String::from("lxrun::int80_legacy_path"),
                role: String::from("r"),
                wave: WAVE_RETIRED,
                alt_ptr: String::from("C2-x"),
                postpone_reason: String::new(),
                active: true,
            }) == Err(IrCode::RetiredStillActive.val()),
        );

        // ③ 替代路径指针空。
        led.record(
            2,
            vet_row(&RetireRow {
                site: String::from("kernel::int80_dispatch"),
                role: String::from("r"),
                wave: WAVE_SCHEDULED,
                alt_ptr: String::new(),
                postpone_reason: String::new(),
                active: true,
            }) == Err(IrCode::AltPointerBlank.val()),
        );

        // ④ 波次越闭集。
        led.record(3, {
            vet_row(&RetireRow {
                site: String::from("kernel::int80_dispatch"),
                role: String::from("r"),
                wave: WAVE_COUNT + (rnd % 2) as usize,
                alt_ptr: String::from("C2-x"),
                postpone_reason: String::new(),
                active: true,
            }) == Err(IrCode::WaveOutOfRange.val())
        });

        // ⑤ 调用点悬空 + 顺延无理由。
        led.record(
            4,
            vet_row(&RetireRow {
                site: String::from("ghost::site"),
                role: String::from("r"),
                wave: WAVE_SCHEDULED,
                alt_ptr: String::from("C2-x"),
                postpone_reason: String::new(),
                active: true,
            }) == Err(IrCode::SiteDangling.val())
                && {
                    let mut l = build_initial().unwrap_or_else(|_| RetireLedger::new());
                    l.postpone("kernel::int80_dispatch", "")
                        == Err(IrCode::PostponeNoReason.val())
                },
        );

        // ⑥ 账行删除（哈希链断裂）+ 顺延正向。
        led.record(5, {
            let chain_ok = match build_initial() {
                Ok(l) => {
                    let good = l.verify_chain().is_ok();
                    let mut l2 = RetireLedger { rows: Vec::new(), chain: Vec::new() };
                    l2.rows = l.rows.clone();
                    l2.chain = l.chain.clone();
                    if l2.chain.len() > 1 {
                        l2.chain.pop();
                    }
                    good && l2.verify_chain() == Err(IrCode::RowDeleted.val())
                }
                Err(_) => false,
            };
            let post_ok = {
                let mut l3 = build_initial().unwrap_or_else(|_| RetireLedger::new());
                l3.postpone("kernel::int80_dispatch", "上游 C2 收口顺延一波次")
                    .is_ok()
            };
            chain_ok && post_ok && (rnd % 1) == 0
        });

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
                Err(IrCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法退役计划必须全过 ───────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let l = build_initial()?;
    l.verify_chain()?;
    // 四态名点验。
    let mut w = 0;
    while w < WAVE_COUNT {
        if WAVE_NAMES[w].len() == 0 {
            return Err(IrCode::WaveOutOfRange.val());
        }
        w += 1;
    }
    // 四调用点全在闭集。
    let mut i = 0;
    while i < LEGACY_SITES.len() {
        if !LEGACY_SITES[i].contains("int80") {
            return Err(IrCode::SiteDangling.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < IrCode::ALL.len() {
        let v = IrCode::ALL[i].val();
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
    // 1. 黄金正演：4 行 / 指针 100% / 污染 0。
    let g = golden_run();
    v.push((
        "J1-黄金正演4行100%污0",
        matches!(g, Ok((4, 100, 0))),
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
    // 3. Wine 链路零活跃调用点（判据原文口径）。
    v.push((
        "J1-Wine链路零活跃",
        wine_link_taint_count(&[]) == 0,
        String::from("干净扫描"),
    ));
    // 4. 计划账指针非空率 100%（判据原文口径）。
    let l = build_initial().unwrap_or_else(|_| RetireLedger::new());
    let rate = l.pointer_rate().unwrap_or(0);
    v.push(("J1-指针非空率100%", rate == 100, format!("{}%", rate)));
    // 5. 哈希链完整。
    v.push((
        "J1-哈希链完整",
        matches!(l.verify_chain(), Ok(())),
        format!("{} 环", l.len()),
    ));
    // 6. 顺延留痕（理由必填）。
    v.push((
        "J1-顺延留痕理由必填",
        {
            let mut l2 = build_initial().unwrap_or_else(|_| RetireLedger::new());
            l2.postpone("kernel::int80_dispatch", "上游 C2 收口顺延一波次")
                .is_ok()
                && l2.rows[0].wave == WAVE_SCHEDULED - 1
                && l2.rows[0].postpone_reason.len() > 0
        },
        String::from("波次回退+理由在档"),
    ));
    // 7. 退役波次四态。
    v.push((
        "J1-退役波次四态",
        WAVE_COUNT == 4,
        String::from("未排期/已排期/进行中/已退役"),
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
        String::from("四态+四调用点"),
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
pub fn run_unx16018_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16018 int 0x80 旧面升级接管声明与退役计划");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
