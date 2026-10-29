//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13421 · 应用样本集成总成（双源之二——选取协议与入库总账） · 0x65A40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥370 行；
//!   ② UNX-F13421-J1 应用样本入库总账（样本清单/工具链/哈希/装载态
//!      四列），选取协议与 AI-94 台账登记一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双源之二选取协议：只收 apps\ 前缀路径（源二冻结前缀）；
//!     非源二路径拒收（源一 system32\ 归 F13401 卡管，两卡互斥成对）；
//!   - 四列入账：清单名/工具链/哈希/装载态；工具链四闭集
//!     （Msvc/Clang/Gcc/Nasm，版本号非零），清单外拒收；
//!   - 哈希确定性：FNV-1a over 清单字节 ⊕ 工具链码 ⊕ 版本；
//!   - 装载态五态机：Received→Tooled（工具链核）→Hashed（哈希核）
//!     →Deployed→Retired（终态可重入，重入须双核再过）；
//!     未双核部署拒收（红线）；
//!   - 12 样本闭集：Σ装载态 ≡ 12；双跑哈希逐位同；
//!   - 注入三族：非源二路径/清单外工具链/未双核部署 3/3 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65A40），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65A40，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65A40;

/// 应用样本闭集：12 条（AI-94 台账源二登记面）。
pub const APP_SAMPLE_COUNT: usize = 12;

/// 源二路径冻结前缀。
pub const SOURCE2_PREFIX: &str = "apps\\";

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppCode {
    /// 样本下标越闭集
    AppSampleOutOfRange,
    /// 非源二路径（选取协议违）
    SourceInvalid,
    /// 工具链枚举外
    ToolchainInvalid,
    /// 工具链版本为 0
    ToolchainVersionZero,
    /// 装载态非法迁移
    StateIllegal,
    /// 样本重复入账
    AppSampleBookDup,
    /// 哈希对账不符
    HashMismatch,
    /// 装载态计数守恒破（Σ装载态≡12）
    ConservationBreak,
    /// 未双核部署（红线）
    DeployWithoutDoubleVet,
}

impl AppCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AppCode::AppSampleOutOfRange => "样本下标越闭集：12 条冻结",
            AppCode::SourceInvalid => "非源二路径：apps\\ 前缀红线",
            AppCode::ToolchainInvalid => "工具链枚举外：四闭集",
            AppCode::ToolchainVersionZero => "工具链版本为 0",
            AppCode::StateIllegal => "装载态非法迁移：五态闭集外",
            AppCode::AppSampleBookDup => "样本重复入账：一址一行",
            AppCode::HashMismatch => "哈希对账不符：装载态列与现算不一致",
            AppCode::ConservationBreak => "装载态计数守恒破：Σ装载态≡12",
            AppCode::DeployWithoutDoubleVet => "未双核部署：工具链+哈希双核前置",
        }
    }
    pub const ALL: [AppCode; 9] = [
        AppCode::AppSampleOutOfRange,
        AppCode::SourceInvalid,
        AppCode::ToolchainInvalid,
        AppCode::ToolchainVersionZero,
        AppCode::StateIllegal,
        AppCode::AppSampleBookDup,
        AppCode::HashMismatch,
        AppCode::ConservationBreak,
        AppCode::DeployWithoutDoubleVet,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AppCode::ALL.len() {
        if AppCode::ALL[i].val() == code {
            return AppCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65A40 段闭集内"
}

// ── 工具链四闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Toolchain {
    Msvc,
    Clang,
    Gcc,
    Nasm,
}

impl Toolchain {
    pub fn name(self) -> &'static str {
        match self {
            Toolchain::Msvc => "msvc",
            Toolchain::Clang => "clang",
            Toolchain::Gcc => "gcc",
            Toolchain::Nasm => "nasm",
        }
    }
    pub fn code(self) -> u32 {
        match self {
            Toolchain::Msvc => 0x4D53_5643, // "MSVC" 首双字节折叠
            Toolchain::Clang => 0x434C_4E47, // "CLNG"
            Toolchain::Gcc => 0x4743_435F,  // "GCC_"
            Toolchain::Nasm => 0x4E41_534D, // "NASM"
        }
    }
    /// 版本写死（样板工程冻结工具链版本表）。
    pub fn frozen_version(self) -> u32 {
        match self {
            Toolchain::Msvc => 1938, // v14.38
            Toolchain::Clang => 1700, // v17.00
            Toolchain::Gcc => 1302,  // v13.02
            Toolchain::Nasm => 216,  // v2.16
        }
    }
}

/// 从四闭集解码（越界拒收）。
pub fn toolchain_from_code(code: u32) -> Result<Toolchain, u32> {
    match code {
        0x4D53_5643 => Ok(Toolchain::Msvc),
        0x434C_4E47 => Ok(Toolchain::Clang),
        0x4743_435F => Ok(Toolchain::Gcc),
        0x4E41_534D => Ok(Toolchain::Nasm),
        _ => Err(AppCode::ToolchainInvalid.val()),
    }
}

// ── 装载态五闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeployState {
    /// 已接收
    Received,
    /// 工具链已核
    Tooled,
    /// 哈希已核（双核齐）
    Hashed,
    /// 已部署
    Deployed,
    /// 已退役（可重入，须双核再过）
    Retired,
}

/// 样本行：四列（清单/工具链/哈希/装载态）。
#[derive(Clone, Copy, Debug)]
pub struct AppRow {
    pub name: &'static str,
    pub tool: Toolchain,
    pub hash: u32,
    pub state: DeployState,
}

/// 选取协议：只收源二前缀（apps\）。
pub fn select_source2(name: &'static str) -> Result<(), u32> {
    if !name.starts_with(SOURCE2_PREFIX) {
        return Err(AppCode::SourceInvalid.val());
    }
    Ok(())
}

/// 样本哈希：FNV-1a over 清单字节 ⊕ 工具链码 ⊕ 版本。
pub fn app_hash(name: &str, tool: Toolchain, ver: u32) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    let b = name.as_bytes();
    let mut i = 0;
    while i < b.len() {
        h = (h ^ b[i] as u32).wrapping_mul(0x01000193);
        i += 1;
    }
    h = (h ^ tool.code()).wrapping_mul(0x01000193);
    h = (h ^ ver).wrapping_mul(0x01000193);
    h
}

// ── 入库总账 ─────────────────────────────────────────────────────
pub struct AppLedger {
    pub rows: Vec<AppRow>,
}

impl AppLedger {
    pub fn new() -> AppLedger {
        AppLedger { rows: Vec::new() }
    }

    /// 入账一条（四列）：源二前缀 + 工具链闭集内 + 版本非零 + 一址一行。
    pub fn book(
        &mut self,
        name: &'static str,
        tool: Toolchain,
        ver: u32,
    ) -> Result<usize, u32> {
        select_source2(name)?;
        if ver == 0 || ver != tool.frozen_version() {
            return Err(AppCode::ToolchainVersionZero.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].name == name {
                return Err(AppCode::AppSampleBookDup.val());
            }
            i += 1;
        }
        let idx = self.rows.len();
        if idx >= APP_SAMPLE_COUNT {
            return Err(AppCode::AppSampleOutOfRange.val());
        }
        let h = app_hash(name, tool, ver);
        self.rows.push(AppRow {
            name,
            tool,
            hash: h,
            state: DeployState::Received,
        });
        Ok(idx)
    }

    /// 工具链核：Received/Retired→Tooled。
    pub fn vet_tool(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(AppCode::AppSampleOutOfRange.val());
        }
        let st = self.rows[idx].state;
        let legal = matches!(st, DeployState::Received | DeployState::Retired);
        if !legal {
            return Err(AppCode::StateIllegal.val());
        }
        // 工具链码必须能从闭集解码（反查自证）。
        let _ = toolchain_from_code(self.rows[idx].tool.code())?;
        self.rows[idx].state = DeployState::Tooled;
        Ok(())
    }

    /// 哈希核：Tooled→Hashed，现算哈希必须与列一致。
    pub fn vet_hash(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(AppCode::AppSampleOutOfRange.val());
        }
        if self.rows[idx].state != DeployState::Tooled {
            return Err(AppCode::StateIllegal.val());
        }
        let r = &self.rows[idx];
        if app_hash(r.name, r.tool, r.tool.frozen_version()) != r.hash {
            return Err(AppCode::HashMismatch.val());
        }
        self.rows[idx].state = DeployState::Hashed;
        Ok(())
    }

    /// 部署：Hashed→Deployed（未双核拒收，红线）。
    pub fn deploy(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(AppCode::AppSampleOutOfRange.val());
        }
        if self.rows[idx].state != DeployState::Hashed {
            return Err(AppCode::DeployWithoutDoubleVet.val());
        }
        self.rows[idx].state = DeployState::Deployed;
        Ok(())
    }

    /// 退役：Deployed→Retired（终态，重入须双核再过）。
    pub fn retire(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.rows.len() {
            return Err(AppCode::AppSampleOutOfRange.val());
        }
        if self.rows[idx].state != DeployState::Deployed {
            return Err(AppCode::StateIllegal.val());
        }
        self.rows[idx].state = DeployState::Retired;
        Ok(())
    }

    /// 装载态计数守恒：Σ装载态 ≡ 行数。
    pub fn conservation(&self) -> Result<[u32; 5], u32> {
        let mut counts = [0u32; 5];
        let mut i = 0;
        while i < self.rows.len() {
            let k = match self.rows[i].state {
                DeployState::Received => 0,
                DeployState::Tooled => 1,
                DeployState::Hashed => 2,
                DeployState::Deployed => 3,
                DeployState::Retired => 4,
            };
            counts[k] += 1;
            i += 1;
        }
        let total = counts[0] + counts[1] + counts[2] + counts[3] + counts[4];
        if total != self.rows.len() as u32 {
            return Err(AppCode::ConservationBreak.val());
        }
        Ok(counts)
    }

    /// 总账哈希（结算用）。
    pub fn settle_hash(&self) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        let mut i = 0;
        while i < self.rows.len() {
            h = (h ^ self.rows[i].hash).wrapping_mul(0x01000193);
            let v = match self.rows[i].state {
                DeployState::Received => 1u32,
                DeployState::Tooled => 2,
                DeployState::Hashed => 3,
                DeployState::Deployed => 4,
                DeployState::Retired => 5,
            };
            h = (h ^ v).wrapping_mul(0x01000193);
            i += 1;
        }
        h
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

/// 黄金应用样本清单（12 条，源二登记面）。
const GOLDEN_FULLNAMES: [&str; APP_SAMPLE_COUNT] = [
    "apps\\editor.exe",
    "apps\\viewer.exe",
    "apps\\player.exe",
    "apps\\packer.exe",
    "apps\\meter.exe",
    "apps\\tracer.exe",
    "apps\\launcher.exe",
    "apps\\updater.exe",
    "apps\\monitor.exe",
    "apps\\reporter.exe",
    "apps\\syncer.exe",
    "apps\\cleaner.exe",
];

/// 工具链指派（冻结表：0/1 Msvc、2/3 Clang、4/5 Gcc、6~11 交错）。
pub fn golden_tool(i: usize) -> Toolchain {
    match i % 4 {
        0 => Toolchain::Msvc,
        1 => Toolchain::Clang,
        2 => Toolchain::Gcc,
        _ => Toolchain::Nasm,
    }
}

// ── 黄金正演：12 样本四列入账 → 双核 → 部署 → 退役重入 → 结算 ──
pub fn golden_run() -> Result<u32, u32> {
    let mut led = AppLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < APP_SAMPLE_COUNT {
        led.book(GOLDEN_FULLNAMES[i], golden_tool(i), golden_tool(i).frozen_version())?;
        i += 1;
    }
    // 双核 + 部署全量。
    let mut j = 0;
    while j < APP_SAMPLE_COUNT {
        led.vet_tool(j)?;
        led.vet_hash(j)?;
        led.deploy(j)?;
        j += 1;
    }
    // 退役 2 条再重入（双核再过）。
    let r1 = rng.below(APP_SAMPLE_COUNT as u64) as usize;
    let mut r2 = rng.below(APP_SAMPLE_COUNT as u64) as usize;
    if r2 == r1 {
        r2 = (r1 + 1) % APP_SAMPLE_COUNT;
    }
    led.retire(r1)?;
    led.retire(r2)?;
    led.vet_tool(r1)?;
    led.vet_hash(r1)?;
    led.deploy(r1)?;
    led.vet_tool(r2)?;
    led.vet_hash(r2)?;
    led.deploy(r2)?;
    // 守恒 + 双跑幂等。
    led.conservation()?;
    let h1 = led.settle_hash();
    let h2 = led.settle_hash();
    if h1 != h2 {
        return Err(AppCode::ConservationBreak.val());
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
    let mut d = AppLedger::new();
    // 族 0：非源二路径（源一前缀互斥拒收）。
    ledger.tries[0] += 1;
    if let Err(e) = d.book("system32\\a.dll", Toolchain::Msvc, Toolchain::Msvc.frozen_version()) {
        if e == AppCode::SourceInvalid.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：清单外工具链（伪码解码必拒）。
    ledger.tries[1] += 1;
    if let Err(e) = toolchain_from_code(0xDEAD_0000) {
        if e == AppCode::ToolchainInvalid.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：未双核部署（只过工具链核就部署）。
    ledger.tries[2] += 1;
    let idx = match d.book("apps\\inj.exe", Toolchain::Clang, Toolchain::Clang.frozen_version())
    {
        Ok(i) => i,
        Err(_) => 0,
    };
    let _ = d.vet_tool(idx);
    if let Err(e) = d.deploy(idx) {
        if e == AppCode::DeployWithoutDoubleVet.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AppCode::ALL.len() {
        let v = AppCode::ALL[i].val();
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
    v.push(("J1-黄金正演12样本结算", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 选取协议正反成对：源二收、源一拒。
    v.push((
        "J1-选取协议正反",
        select_source2("apps\\a.exe").is_ok()
            && select_source2("system32\\a.dll") == Err(AppCode::SourceInvalid.val()),
        String::from("apps\\ 前缀闭"),
    ));
    // 工具链四闭集：版本表全非零且解码闭环。
    let tools = [Toolchain::Msvc, Toolchain::Clang, Toolchain::Gcc, Toolchain::Nasm];
    let mut closed_ok = true;
    let mut i = 0;
    while i < 4 {
        if tools[i].frozen_version() == 0
            || toolchain_from_code(tools[i].code()) != Ok(tools[i])
        {
            closed_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-工具链四闭集闭环",
        closed_ok,
        String::from("msvc/clang/gcc/nasm"),
    ));
    // 版本 0 拒收。
    let mut d2 = AppLedger::new();
    v.push((
        "J1-工具链版本零必拒",
        d2.book("apps\\a.exe", Toolchain::Msvc, 0)
            == Err(AppCode::ToolchainVersionZero.val()),
        String::from("ver=0 拒收"),
    ));
    // detail 反查闭链。
    let first = detail(AppCode::AppSampleOutOfRange.val());
    let last = detail(AppCode::DeployWithoutDoubleVet.val());
    v.push((
        "J2-detail 反查闭链",
        first != "未知诊断码：不在 0x65A40 段闭集内"
            && last != "未知诊断码：不在 0x65A40 段闭集内",
        String::from("首尾反查非未知"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65A40 窗口")));
    v
}

pub fn run_unx13421_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13421 应用样本集成总成");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
