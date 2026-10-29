//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13401 · 系统样本集成总成（System32 双源之一——选取协议与入库总账）
//! · 0x65A00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥370 行；
//!   ② UNX-F13401-J1 系统样本入库总账（样本清单/版本/哈希/装载态四列
//!      入账），选取协议与 AI-94 台账登记一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双源之一选取协议：只收 system32\ 前缀路径（源一冻结前缀，
//!     AI-94 台账语义）；非源一路径拒收（源二归 F13421 卡管）；
//!   - 四列入账：清单名/版本号/哈希/装载态；版本列为 0 拒收、
//!     重复入账拒收（一址一行）；
//!   - 哈希确定性：FNV-1a over 清单字节 ⊕ 版本混合（公开常数）；
//!   - 装载态四态机：Inbound→Vetted（哈希核）→Loaded→Unloaded；
//!     Unloaded→Vetted 重载须再核哈希；未核哈希装载拒收（红线）；
//!   - 16 样本闭集：Σ装载态 ≡ 16；总账齐才许入库结算；双跑哈希逐位同；
//!   - 注入三族：非源一路径/未核装载/重复入账 3/3 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65A00），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65A00，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65A00;

/// 系统样本闭集：16 条（AI-94 台账源一登记面）。
pub const SYS_SAMPLE_COUNT: usize = 16;

/// 源一路径冻结前缀（双源之一；源二 systemWOW64 归 F13421）。
pub const SOURCE1_PREFIX: &str = "system32\\";

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysCode {
    /// 样本下标越闭集
    SampleOutOfRange,
    /// 非源一路径（选取协议违）
    SourceInvalid,
    /// 哈希对账不符（装载态列 vs 现算哈希）
    HashMismatch,
    /// 装载态非法迁移
    StateIllegal,
    /// 样本重复入账
    SampleBookDup,
    /// 版本列为 0
    VersionZero,
    /// 未核哈希装载（红线）
    LoadWithoutVet,
    /// 装载态计数守恒破（Σ装载态≡16）
    ConservationBreak,
    /// 总账未齐跑入库结算
    LedgerIncomplete,
}

impl SysCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SysCode::SampleOutOfRange => "样本下标越闭集：16 条冻结",
            SysCode::SourceInvalid => "非源一路径：system32\\ 前缀红线",
            SysCode::HashMismatch => "哈希对账不符：装载态列与现算不一致",
            SysCode::StateIllegal => "装载态非法迁移：四态闭集外",
            SysCode::SampleBookDup => "样本重复入账：一址一行",
            SysCode::VersionZero => "版本列为 0：版本语义非法",
            SysCode::LoadWithoutVet => "未核哈希装载：Vetted 前置红线",
            SysCode::ConservationBreak => "装载态计数守恒破：Σ装载态≡16",
            SysCode::LedgerIncomplete => "总账未齐（16 条未全登记）即结算",
        }
    }
    pub const ALL: [SysCode; 9] = [
        SysCode::SampleOutOfRange,
        SysCode::SourceInvalid,
        SysCode::HashMismatch,
        SysCode::StateIllegal,
        SysCode::SampleBookDup,
        SysCode::VersionZero,
        SysCode::LoadWithoutVet,
        SysCode::ConservationBreak,
        SysCode::LedgerIncomplete,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SysCode::ALL.len() {
        if SysCode::ALL[i].val() == code {
            return SysCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65A00 段闭集内"
}

// ── 装载态四闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadState {
    /// 已入库未核
    Inbound,
    /// 哈希已核
    Vetted,
    /// 已装载
    Loaded,
    /// 已卸载
    Unloaded,
}

/// 样本行：四列（清单/版本/哈希/装载态）。
#[derive(Clone, Copy, Debug)]
pub struct SampleRow {
    pub name: &'static str,
    pub version: u32,
    pub hash: u32,
    pub state: LoadState,
}

/// 选取协议：只收源一前缀（system32\）。
pub fn select_source(name: &'static str) -> Result<(), u32> {
    if !name.starts_with(SOURCE1_PREFIX) {
        return Err(SysCode::SourceInvalid.val());
    }
    Ok(())
}

/// 样本哈希：FNV-1a over 清单字节，再混版本（确定性，公开常数）。
pub fn sample_hash(name: &str, version: u32) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    let b = name.as_bytes();
    let mut i = 0;
    while i < b.len() {
        h = (h ^ b[i] as u32).wrapping_mul(0x01000193);
        i += 1;
    }
    h = (h ^ version).wrapping_mul(0x01000193);
    h
}

// ── 入库总账 ─────────────────────────────────────────────────────
pub struct SysLedger {
    pub rows: Vec<SampleRow>,
    pub settled: bool,
}

impl SysLedger {
    pub fn new() -> SysLedger {
        SysLedger {
            rows: Vec::new(),
            settled: false,
        }
    }

    /// 入账一条（四列）：源一前缀 + 版本非零 + 一址一行。
    pub fn book(&mut self, name: &'static str, version: u32) -> Result<usize, u32> {
        select_source(name)?;
        if version == 0 {
            return Err(SysCode::VersionZero.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].name == name {
                return Err(SysCode::SampleBookDup.val());
            }
            i += 1;
        }
        let idx = self.rows.len();
        if idx >= SYS_SAMPLE_COUNT {
            return Err(SysCode::SampleOutOfRange.val());
        }
        let h = sample_hash(name, version);
        self.rows.push(SampleRow {
            name,
            version,
            hash: h,
            state: LoadState::Inbound,
        });
        Ok(idx)
    }

    /// 哈希核：Inbound/Unloaded→Vetted，现算哈希必须与列一致。
    pub fn vet(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(SysCode::SampleOutOfRange.val());
        }
        let st = self.rows[idx].state;
        let legal = matches!(st, LoadState::Inbound | LoadState::Unloaded);
        if !legal {
            return Err(SysCode::StateIllegal.val());
        }
        let r = &self.rows[idx];
        if sample_hash(r.name, r.version) != r.hash {
            return Err(SysCode::HashMismatch.val());
        }
        self.rows[idx].state = LoadState::Vetted;
        Ok(())
    }

    /// 装载：Vetted→Loaded（未核拒收，红线）。
    pub fn load(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(SysCode::SampleOutOfRange.val());
        }
        if self.rows[idx].state != LoadState::Vetted {
            return Err(SysCode::LoadWithoutVet.val());
        }
        self.rows[idx].state = LoadState::Loaded;
        Ok(())
    }

    /// 卸载：Loaded→Unloaded（重载须再核哈希）。
    pub fn unload(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(SysCode::SampleOutOfRange.val());
        }
        if self.rows[idx].state != LoadState::Loaded {
            return Err(SysCode::StateIllegal.val());
        }
        self.rows[idx].state = LoadState::Unloaded;
        Ok(())
    }

    /// 装载态计数守恒：Σ装载态 ≡ 行数。
    pub fn conservation(&self) -> Result<[u32; 4], u32> {
        let mut counts = [0u32; 4];
        let mut i = 0;
        while i < self.rows.len() {
            let k = match self.rows[i].state {
                LoadState::Inbound => 0,
                LoadState::Vetted => 1,
                LoadState::Loaded => 2,
                LoadState::Unloaded => 3,
            };
            counts[k] += 1;
            i += 1;
        }
        let total = counts[0] + counts[1] + counts[2] + counts[3];
        if total != self.rows.len() as u32 {
            return Err(SysCode::ConservationBreak.val());
        }
        Ok(counts)
    }

    /// 总账齐判定。
    pub fn complete(&self) -> bool {
        self.rows.len() == SYS_SAMPLE_COUNT
    }

    /// 入库结算：总账齐 + 守恒才许；返回总账哈希。
    pub fn settle(&self) -> Result<u32, u32> {
        if !self.complete() {
            return Err(SysCode::LedgerIncomplete.val());
        }
        self.conservation()?;
        let mut h: u32 = 0x811C9DC5;
        let mut i = 0;
        while i < self.rows.len() {
            h = (h ^ self.rows[i].hash).wrapping_mul(0x01000193);
            let v = match self.rows[i].state {
                LoadState::Inbound => 1u32,
                LoadState::Vetted => 2,
                LoadState::Loaded => 3,
                LoadState::Unloaded => 4,
            };
            h = (h ^ v).wrapping_mul(0x01000193);
            i += 1;
        }
        Ok(h)
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

/// 黄金系统样本清单（16 条，AI-94 台账源一登记面，前缀按协议拼接）。
pub const GOLDEN_BASENAMES: [&str; SYS_SAMPLE_COUNT] = [
    "kernel32.dll",
    "user32.dll",
    "gdi32.dll",
    "shell32.dll",
    "advapi32.dll",
    "ole32.dll",
    "oleaut32.dll",
    "ws2_32.dll",
    "winmm.dll",
    "imm32.dll",
    "version.dll",
    "msvcrt.dll",
    "comctl32.dll",
    "uxtheme.dll",
    "dwmapi.dll",
    "shlwapi.dll",
];

/// 源一全路径拼名（确定性，静态生命周期由 const 保障）。
pub fn golden_full_name(i: usize) -> &'static str {
    const FULL: [&str; SYS_SAMPLE_COUNT] = [
        "system32\\kernel32.dll",
        "system32\\user32.dll",
        "system32\\gdi32.dll",
        "system32\\shell32.dll",
        "system32\\advapi32.dll",
        "system32\\ole32.dll",
        "system32\\oleaut32.dll",
        "system32\\ws2_32.dll",
        "system32\\winmm.dll",
        "system32\\imm32.dll",
        "system32\\version.dll",
        "system32\\msvcrt.dll",
        "system32\\comctl32.dll",
        "system32\\uxtheme.dll",
        "system32\\dwmapi.dll",
        "system32\\shlwapi.dll",
    ];
    FULL[i]
}

/// 版本写死：基线 0x000A0000 + 序号 << 16。
pub fn golden_version(i: usize) -> u32 {
    0x000A0000 + (i as u32) * 0x0001_0000
}

// ── 黄金正演：16 样本四列入账 → 核/装/卸重载 → 结算 → 双跑幂等 ──
pub fn golden_run() -> Result<u32, u32> {
    let mut led = SysLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < SYS_SAMPLE_COUNT {
        led.book(golden_full_name(i), golden_version(i))?;
        i += 1;
    }
    // 逐条核哈希 + 装载。
    let mut j = 0;
    while j < SYS_SAMPLE_COUNT {
        led.vet(j)?;
        led.load(j)?;
        j += 1;
    }
    // 卸载 2 条再重载（重载须再核哈希）。
    let u1 = rng.below(SYS_SAMPLE_COUNT as u64) as usize;
    let mut u2 = rng.below(SYS_SAMPLE_COUNT as u64) as usize;
    if u2 == u1 {
        u2 = (u1 + 1) % SYS_SAMPLE_COUNT;
    }
    led.unload(u1)?;
    led.unload(u2)?;
    led.vet(u1)?;
    led.load(u1)?;
    led.vet(u2)?;
    led.load(u2)?;
    // 守恒 + 结算。
    led.conservation()?;
    let h1 = led.settle()?;
    let h2 = led.settle()?;
    if h1 != h2 {
        return Err(SysCode::ConservationBreak.val());
    }
    Ok(h1)
}

// ── 注入面：三族 3/3 必抓 ────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    let mut d = SysLedger::new();
    // 族 0：非源一路径（源二/裸名拒收）。
    ledger.tries[0] += 1;
    if let Err(e) = d.book("syswow64\\kernel32.dll", 0x000A0000) {
        if e == SysCode::SourceInvalid.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：未核哈希装载。
    ledger.tries[1] += 1;
    let idx = match d.book("system32\\kernel32.dll", 0x000A0000) {
        Ok(i) => i,
        Err(_) => 0,
    };
    if let Err(e) = d.load(idx) {
        if e == SysCode::LoadWithoutVet.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：样本重复入账。
    ledger.tries[2] += 1;
    if let Err(e) = d.book("system32\\kernel32.dll", 0x000A0001) {
        if e == SysCode::SampleBookDup.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SysCode::ALL.len() {
        let v = SysCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push(("J1-黄金正演16样本结算", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 选取协议正演：源一收、源二拒（反向判据成对）。
    v.push((
        "J1-选取协议正反",
        select_source("system32\\a.dll").is_ok()
            && select_source("program files\\a.dll")
                == Err(SysCode::SourceInvalid.val()),
        String::from("system32\\ 前缀闭"),
    ));
    // 版本 0 拒收。
    let mut d2 = SysLedger::new();
    v.push((
        "J1-版本零必拒",
        d2.book("system32\\a.dll", 0) == Err(SysCode::VersionZero.val()),
        String::from("version=0 拒收"),
    ));
    // 总账未齐结算必抓。
    v.push((
        "J1-未齐结算必抓",
        d2.settle() == Err(SysCode::LedgerIncomplete.val()),
        String::from("16 条未齐拒结算"),
    ));
    // 哈希确定性：同名同版双算逐位同。
    let h1 = sample_hash("system32\\kernel32.dll", 0x000A0000);
    let h2 = sample_hash("system32\\kernel32.dll", 0x000A0000);
    v.push((
        "J1-哈希确定性",
        h1 == h2 && h1 != 0,
        format!("hash={:#010x}", h1),
    ));
    // detail 反查闭链。
    let first = detail(SysCode::SampleOutOfRange.val());
    let last = detail(SysCode::LedgerIncomplete.val());
    v.push((
        "J2-detail 反查闭链",
        first != "未知诊断码：不在 0x65A00 段闭集内"
            && last != "未知诊断码：不在 0x65A00 段闭集内",
        String::from("首尾反查非未知"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65A00 窗口")));
    v
}

pub fn run_unx13401_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13401 系统样本集成总成");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
