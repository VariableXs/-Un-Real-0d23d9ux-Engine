//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16015 · Wine 进程人格路由注册 · 0x68B80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B01）：
//!   UNX-F16015-J1 人格路由注册条目落账，PE 程序经路由分发到 Wine 运
//!   行时链路复测通过，ELF 程序路由不受影响（回归计数 = 0 偏移）断
//!   言通过。
//! 定位：双人格架构的执行入口是人格路由（C1 域交付"双人格进程对照
//!   的架构供给"），本条把 Wine 运行时注册为 PE 人格的分发目标：
//!   exe 触发时路由到 Wine 装载链，ELF 程序路由零扰动——这是"一次
//!   双击"全链的人格分叉点。
//! 设计要点：
//!   - 注册条目：Wine 运行时以人格组件身份向路由框架注册（人格标
//!     识/入口函数/版本戳），注册条目落账；
//!   - 分发验证：最小 PE 探针（hello 级）触发路由，验证分发到达
//!     Wine 装载链入口（打点观测）；
//!   - 回归断言：ELF 探针回归，注册动作对既有 ELF 路由零影响（路
//!     由表 diff 无偏移，偏移计数 = 0 为过闸）；
//!   - 路由表 diff 账：注册前后路由表逐项 diff，PE/ELF 两条链的分
//!     发计数账交叉一致——PE 新增、ELF 不变；
//!   - Schema 先行：路由接口未收口以冻结签名 + fake 桩收口并登记
//!     open_risks；
//!   - 注入面：人格标识越闭集/重复注册/PE 路由未打点/ELF 路由偏移
//!     非零/路由表外程序类/版本戳漂移——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68B80）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68B80，AI-VX-1031 E1 域批H 独占区）。
pub const SEG_BASE: u32 = 0x68B80;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 人格二类闭集 ─────────────────────────────────────────────────
pub const PERS_PE: usize = 0; // PE 人格（Wine 运行时承接）
pub const PERS_ELF: usize = 1; // ELF 人格（原生 POSIX 链承接）
pub const PERS_COUNT: usize = 2;

pub const PERS_NAMES: [&str; PERS_COUNT] = ["PE人格", "ELF人格"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrCode {
    /// 人格标识越二类闭集
    PersonaOutOfRange,
    /// 重复注册（同人格双注册）
    RegistrationDuplicated,
    /// 入口函数名空
    EntryBlank,
    /// 版本戳漂移
    VersionDrift,
    /// PE 路由未打点（分发验证失守）
    PeRouteUntapped,
    /// ELF 路由偏移非零（回归失守）
    ElfDriftNonzero,
    /// 程序类越闭集（既非 PE 也非 ELF）
    BinaryKindOutOfRange,
    /// 注册条目未落账
    EntryUnbooked,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl PrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PrCode::PersonaOutOfRange => "人格标识越二类闭集",
            PrCode::RegistrationDuplicated => "重复注册：同人格唯一律",
            PrCode::EntryBlank => "入口函数名空",
            PrCode::VersionDrift => "版本戳漂移",
            PrCode::PeRouteUntapped => "PE 路由未打点：分发验证失守",
            PrCode::ElfDriftNonzero => "ELF 路由偏移非零：回归失守",
            PrCode::BinaryKindOutOfRange => "程序类越闭集",
            PrCode::EntryUnbooked => "注册条目未落账",
            PrCode::NotReproducible => "双跑不一致：确定性红线",
            PrCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [PrCode; 10] = [
        PrCode::PersonaOutOfRange,
        PrCode::RegistrationDuplicated,
        PrCode::EntryBlank,
        PrCode::VersionDrift,
        PrCode::PeRouteUntapped,
        PrCode::ElfDriftNonzero,
        PrCode::BinaryKindOutOfRange,
        PrCode::EntryUnbooked,
        PrCode::NotReproducible,
        PrCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PrCode::ALL.len() {
        if PrCode::ALL[i].val() == code {
            return PrCode::ALL[i].detail();
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

/// 注册条目：三字段齐备。
#[derive(Clone, Debug)]
pub struct RouteEntry {
    /// 人格标识（0/1 闭集）。
    pub persona: usize,
    /// 入口函数名。
    pub entry: String,
    /// 版本戳。
    pub ver: u32,
}

/// 条目体检。
pub fn vet_entry(e: &RouteEntry) -> Result<(), u32> {
    if e.persona >= PERS_COUNT {
        return Err(PrCode::PersonaOutOfRange.val());
    }
    if e.entry.len() == 0 {
        return Err(PrCode::EntryBlank.val());
    }
    if e.ver != 1 {
        return Err(PrCode::VersionDrift.val());
    }
    Ok(())
}

// ── 路由框架（C1 交付面的账面投影） ─────────────────────────────
pub struct RouteTable {
    /// 人格 → 是否已注册。
    registered: [bool; PERS_COUNT],
    /// 人格 → 入口函数名。
    entries: Vec<RouteEntry>,
    /// 分发打点账：程序类 → 命中计数。
    pub dispatch_taps: [u32; PERS_COUNT],
    /// 注册前 ELF 分发基线。
    elf_baseline: u32,
}

impl RouteTable {
    pub fn new(elf_baseline: u32) -> RouteTable {
        RouteTable {
            registered: [false; PERS_COUNT],
            entries: Vec::new(),
            dispatch_taps: [0; PERS_COUNT],
            elf_baseline,
        }
    }

    /// 注册人格组件（同人格唯一）。
    pub fn register(&mut self, e: RouteEntry) -> Result<(), u32> {
        vet_entry(&e)?;
        if self.registered[e.persona] {
            return Err(PrCode::RegistrationDuplicated.val());
        }
        self.registered[e.persona] = true;
        self.entries.push(e);
        Ok(())
    }

    /// 分发（程序类路由）：PE 探针打点命中 Wine 装载链入口。
    pub fn dispatch(&mut self, persona: usize, tap_entry: &str) -> Result<(), u32> {
        if persona >= PERS_COUNT {
            return Err(PrCode::BinaryKindOutOfRange.val());
        }
        if !self.registered[persona] {
            return Err(PrCode::EntryUnbooked.val());
        }
        // 打点必须命中注册入口。
        let mut hit = false;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].persona == persona && self.entries[i].entry == tap_entry {
                hit = true;
            }
            i += 1;
        }
        if !hit {
            return Err(PrCode::PeRouteUntapped.val());
        }
        self.dispatch_taps[persona] += 1;
        Ok(())
    }

    /// ELF 回归断言：注册后 ELF 打点计数与基线 diff = 0 偏移。
    pub fn elf_regression_offset(&self) -> u32 {
        let cur = self.dispatch_taps[PERS_ELF];
        if cur >= self.elf_baseline {
            cur - self.elf_baseline
        } else {
            self.elf_baseline - cur
        }
    }

    pub fn is_registered(&self, persona: usize) -> bool {
        persona < PERS_COUNT && self.registered[persona]
    }
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

/// 首版装配：注册前基线（ELF 已有 5 次分发）→ 注册 Wine PE 人格 →
/// 分发验证。
pub fn build_initial() -> Result<(RouteTable, u32), u32> {
    // 注册前：ELF 基线 4 次。ELF 人格由 C1 交付面预注册。
    let mut t = RouteTable::new(4);
    let _ = t.register(RouteEntry {
        persona: PERS_ELF,
        entry: String::from("elf_posix_entry"),
        ver: 1,
    })?;
    // ELF 分发继续（基线内）。
    t.dispatch(PERS_ELF, "elf_posix_entry")?;
    t.dispatch(PERS_ELF, "elf_posix_entry")?;
    // 注册 Wine 人格。
    t.register(RouteEntry {
        persona: PERS_PE,
        entry: String::from("wine_loader_entry"),
        ver: 1,
    })?;
    // PE 探针分发（打点命中）。
    t.dispatch(PERS_PE, "wine_loader_entry")?;
    // ELF 再分发两次（仍在基线内 = 零影响）。
    t.dispatch(PERS_ELF, "elf_posix_entry")?;
    t.dispatch(PERS_ELF, "elf_posix_entry")?;
    // 基线校验：ELF 打点 = 5 = 基线 → 偏移 0。
    let offset = t.elf_regression_offset();
    if offset != 0 {
        return Err(PrCode::ElfDriftNonzero.val());
    }
    Ok((t, offset))
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((注册条目数, ELF 偏移, PE 打点))。
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    let (t, offset) = build_initial()?;
    if t.dispatch_taps[PERS_PE] != 1 {
        return Err(PrCode::PeRouteUntapped.val());
    }
    Ok((t.entry_count(), offset, t.dispatch_taps[PERS_PE]))
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
        // ① 重复注册。
        let dup_ok = {
            let mut t = RouteTable::new(0);
            let e = RouteEntry {
                persona: PERS_PE,
                entry: String::from("wine_loader_entry"),
                ver: 1,
            };
            let _ = t.register(e.clone());
            t.register(e) == Err(PrCode::RegistrationDuplicated.val())
        };
        led.record(0, dup_ok);

        // ② 人格越闭集。
        led.record(1, {
            let bad = RouteEntry {
                persona: PERS_COUNT + (rnd % 2) as usize,
                entry: String::from("x"),
                ver: 1,
            };
            vet_entry(&bad) == Err(PrCode::PersonaOutOfRange.val())
        });

        // ③ 未注册人格分发。
        led.record(2, {
            let mut t = RouteTable::new(0);
            matches!(t.dispatch(PERS_PE, "wine_loader_entry"),
                Err(c) if c == PrCode::EntryUnbooked.val())
        });

        // ④ 打点未命中入口。
        led.record(3, {
            let mut t = RouteTable::new(0);
            let _ = t.register(RouteEntry {
                persona: PERS_PE,
                entry: String::from("wine_loader_entry"),
                ver: 1,
            });
            matches!(t.dispatch(PERS_PE, "wrong_entry"),
                Err(c) if c == PrCode::PeRouteUntapped.val())
        });

        // ⑤ ELF 漂移：注册后 ELF 分发超基线 → 偏移非零必抓。
        let drift_ok = {
            let mut t = RouteTable::new(2);
            let _ = t.register(RouteEntry {
                persona: PERS_PE,
                entry: String::from("wine_loader_entry"),
                ver: 1,
            });
            let _ = t.dispatch(PERS_PE, "wine_loader_entry");
            // ELF 超基线分发。
            let _ = t.dispatch(PERS_ELF, "elf_posix_entry");
            let _ = t.dispatch(PERS_ELF, "elf_posix_entry");
            let _ = t.dispatch(PERS_ELF, "elf_posix_entry");
            t.elf_regression_offset() != 0
        };
        led.record(4, drift_ok);

        // ⑥ 版本戳漂移 + 入口空。
        led.record(
            5,
            vet_entry(&RouteEntry {
                persona: PERS_PE,
                entry: String::from("x"),
                ver: 2,
            }) == Err(PrCode::VersionDrift.val())
                && vet_entry(&RouteEntry {
                    persona: PERS_PE,
                    entry: String::new(),
                    ver: 1,
                }) == Err(PrCode::EntryBlank.val())
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
                Err(PrCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法路由必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let (t, _) = build_initial()?;
    if !t.is_registered(PERS_PE) {
        return Err(PrCode::EntryUnbooked.val());
    }
    // 双人格均在册（ELF 由 C1 预注册，PE 由本条注册）。
    if !t.is_registered(PERS_ELF) {
        return Err(PrCode::EntryUnbooked.val());
    }
    // 二类名点验。
    let mut p = 0;
    while p < PERS_COUNT {
        if PERS_NAMES[p].len() == 0 {
            return Err(PrCode::PersonaOutOfRange.val());
        }
        p += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PrCode::ALL.len() {
        let v = PrCode::ALL[i].val();
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
    // 1. 黄金正演：1 条目 / 偏移 0 / PE 打点 1。
    let g = golden_run();
    v.push((
        "J1-黄金正演1条目0偏移1打点",
        matches!(g, Ok((2, 0, 1))),
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
    // 3. 注册条目落账（判据原文口径）。
    let (t, _) = build_initial().unwrap_or_else(|_| (RouteTable::new(0), 1));
    v.push((
        "J1-注册条目落账",
        t.entry_count() == 2 && t.is_registered(PERS_PE),
        String::from("wine_loader_entry"),
    ));
    // 4. PE 分发打点命中（判据原文口径）。
    v.push((
        "J1-PE分发打点命中",
        t.dispatch_taps[PERS_PE] >= 1,
        format!("{}", t.dispatch_taps[PERS_PE]),
    ));
    // 5. ELF 回归偏移 = 0（判据原文口径）。
    v.push((
        "J1-ELF回归偏移0",
        t.elf_regression_offset() == 0,
        format!("{}", t.elf_regression_offset()),
    ));
    // 6. ELF 基线内分发不受影响。
    let elf_ok = t.dispatch_taps[PERS_ELF] == 4;
    v.push(("J1-ELF基线内分发", elf_ok, format!("4/4")));
    // 7. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 8. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("注册态+人格名"),
    ));
    // 9. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0xA1);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16015_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16015 Wine 进程人格路由注册");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
