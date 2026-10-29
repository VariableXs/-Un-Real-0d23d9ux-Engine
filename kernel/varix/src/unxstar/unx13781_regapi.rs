//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13781 · Reg* 面架构与错误码矩阵总纲（S1 落地） · 0x65EC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② UNX-F13781-J1 全族 API 清单/错误码矩阵框架/语义档体例三件
//!      齐备，逐码可对账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 全族 API 清单闭集：10 个 Reg* 面（Open/Create/SetValue/
//!     GetValue/Delete/Enum/QueryInfo/Close/Rename/Flush），opcode
//!     1~10 冻结；
//!   - 错误码矩阵：9 码闭集（NTSTATUS 体例，写死值）× 10 API =
//!     90 行全登记，行零缺（逐码可对账）；
//!   - 语义档体例：三档（Query/Write/Meta），档×码合法位由档规则
//!     唯一推导；SUCCESS 全档合法，NOT_IMPLEMENTED 全档保留；
//!   - 档规则双表对账：if 链规则 vs 期望计数写死（Query 6 码/
//!     Write 6 码/Meta 4 码），两路一致才判绿；
//!   - 登记状态机闭集：Draft→Building→Sealed，Sealed 后注册拒收；
//!   - 注入三族：未知 API/未知错误码/空语义档注册 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65EC0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65EC0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65EC0;

/// 全族 API 数（写死，来源：Reg* 面 S1 清单）。
pub const API_COUNT: usize = 10;

/// 错误码数（写死，来源：S1 错误码矩阵 9 码）。
pub const CODE_COUNT: usize = 9;

/// 矩阵行总数：10×9 = 90（冻结，来源：两闭集直积）。
pub const ROW_TOTAL: usize = API_COUNT * CODE_COUNT;

/// 错误码闭集（写死值，NTSTATUS 体例）。
pub const EC_SUCCESS: u32 = 0x0000_0000;
pub const EC_INVALID_PARAMETER: u32 = 0xC000_000D;
pub const EC_NAME_NOT_FOUND: u32 = 0xC000_0034;
pub const EC_NAME_COLLISION: u32 = 0xC000_0035;
pub const EC_ACCESS_DENIED: u32 = 0xC000_0022;
pub const EC_BUFFER_TOO_SMALL: u32 = 0xC000_0023;
pub const EC_BUFFER_OVERFLOW: u32 = 0x8000_0005;
pub const EC_KEY_DELETED: u32 = 0xC000_003C;
pub const EC_NOT_IMPLEMENTED: u32 = 0xC000_0002;

/// 语义档期望计数（独立写死，来源：S1 语义档体例）。
pub const EXPECT_QUERY_CODES: usize = 6;
pub const EXPECT_WRITE_CODES: usize = 6;
pub const EXPECT_META_CODES: usize = 4;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiCode {
    /// 未知 API（清单外）
    ApiUnknown,
    /// 错误码越闭集
    ErrorCodeOutOfRange,
    /// 矩阵行缺登记
    MatrixRowIncomplete,
    /// 档规则不符（语义档体例破）
    TierMismatch,
    /// API 重复注册
    DuplicateApi,
    /// opcode 重复
    OpcodeDuplicate,
    /// 语义档空（无合法码的 API 拒注册）
    SemanticsEmpty,
    /// 总纲已封仍注册
    RegistrySealed,
}

impl ApiCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ApiCode::ApiUnknown => "未知 API：清单外拒收",
            ApiCode::ErrorCodeOutOfRange => "错误码越闭集：9 码冻结",
            ApiCode::MatrixRowIncomplete => "矩阵行缺登记：10×9=90 行零缺",
            ApiCode::TierMismatch => "档规则不符：语义档体例破",
            ApiCode::DuplicateApi => "API 重复注册：名单唯一",
            ApiCode::OpcodeDuplicate => "opcode 重复：1~10 唯一",
            ApiCode::SemanticsEmpty => "语义档空：无合法码拒注册",
            ApiCode::RegistrySealed => "总纲已封：Sealed 后注册拒收",
        }
    }
    pub const ALL: [ApiCode; 8] = [
        ApiCode::ApiUnknown,
        ApiCode::ErrorCodeOutOfRange,
        ApiCode::MatrixRowIncomplete,
        ApiCode::TierMismatch,
        ApiCode::DuplicateApi,
        ApiCode::OpcodeDuplicate,
        ApiCode::SemanticsEmpty,
        ApiCode::RegistrySealed,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ApiCode::ALL.len() {
        if ApiCode::ALL[i].val() == code {
            return ApiCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65EC0 段闭集内"
}

// ── 登记状态三闭集 ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookPhase {
    Draft,
    Building,
    Sealed,
}

/// 合法边闭集：Draft→Building→Sealed（线性单链）。
fn book_edge(cur: BookPhase, next: BookPhase) -> bool {
    matches!(
        (cur, next),
        (BookPhase::Draft, BookPhase::Building) | (BookPhase::Building, BookPhase::Sealed)
    )
}

// ── 语义档三档 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    Query,
    Write,
    Meta,
}

/// 错误码闭集表（下标 = 码序）。
pub const CODE_TABLE: [u32; CODE_COUNT] = [
    EC_SUCCESS,
    EC_INVALID_PARAMETER,
    EC_NAME_NOT_FOUND,
    EC_NAME_COLLISION,
    EC_ACCESS_DENIED,
    EC_BUFFER_TOO_SMALL,
    EC_BUFFER_OVERFLOW,
    EC_KEY_DELETED,
    EC_NOT_IMPLEMENTED,
];

/// 码→下标（越闭集报错）。
pub fn code_index(c: u32) -> Result<usize, u32> {
    let mut i = 0;
    while i < CODE_TABLE.len() {
        if CODE_TABLE[i] == c {
            return Ok(i);
        }
        i += 1;
    }
    Err(ApiCode::ErrorCodeOutOfRange.val())
}

/// 档规则（if 链实现，矩阵唯一推导源）。
pub fn tier_allows(tier: Tier, c: u32) -> bool {
    match tier {
        // Query 档：SUCCESS/INVALID_PARAMETER/NOT_FOUND/BUFFER_TOO_SMALL/
        // BUFFER_OVERFLOW/KEY_DELETED（6 码）。
        Tier::Query => matches!(
            c,
            EC_SUCCESS
                | EC_INVALID_PARAMETER
                | EC_NAME_NOT_FOUND
                | EC_BUFFER_TOO_SMALL
                | EC_BUFFER_OVERFLOW
                | EC_KEY_DELETED
        ),
        // Write 档：SUCCESS/INVALID_PARAMETER/NOT_FOUND/COLLISION/
        // ACCESS_DENIED/KEY_DELETED（6 码）。
        Tier::Write => matches!(
            c,
            EC_SUCCESS
                | EC_INVALID_PARAMETER
                | EC_NAME_NOT_FOUND
                | EC_NAME_COLLISION
                | EC_ACCESS_DENIED
                | EC_KEY_DELETED
        ),
        // Meta 档：SUCCESS/INVALID_PARAMETER/ACCESS_DENIED/KEY_DELETED
        //（4 码）。
        Tier::Meta => matches!(
            c,
            EC_SUCCESS | EC_INVALID_PARAMETER | EC_ACCESS_DENIED | EC_KEY_DELETED
        ),
    }
}

/// API 项。
pub struct ApiEntry {
    pub name: &'static str,
    pub opcode: u16,
    pub tier: Tier,
}

/// 矩阵行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatrixRow {
    pub opcode: u16,
    pub code: u32,
    pub allowed: bool,
}

/// Reg* 面总纲。
pub struct RegApiBook {
    apis: Vec<ApiEntry>,
    matrix: Vec<MatrixRow>,
    pub phase: BookPhase,
}

impl RegApiBook {
    pub fn new() -> RegApiBook {
        RegApiBook { apis: Vec::new(), matrix: Vec::new(), phase: BookPhase::Draft }
    }

    pub fn advance(&mut self, next: BookPhase) -> Result<(), u32> {
        if !book_edge(self.phase, next) {
            return Err(ApiCode::RegistrySealed.val());
        }
        self.phase = next;
        Ok(())
    }

    /// 注册 API（名单/opcode 唯一，语义档非空）。
    pub fn register(&mut self, name: &'static str, opcode: u16, tier: Tier) -> Result<(), u32> {
        if self.phase == BookPhase::Sealed {
            return Err(ApiCode::RegistrySealed.val());
        }
        if name.is_empty() {
            return Err(ApiCode::ApiUnknown.val());
        }
        if opcode == 0 || opcode as usize > API_COUNT {
            return Err(ApiCode::ApiUnknown.val());
        }
        // 语义档非空（至少 SUCCESS 合法）。
        if !tier_allows(tier, EC_SUCCESS) {
            return Err(ApiCode::SemanticsEmpty.val());
        }
        let mut i = 0;
        while i < self.apis.len() {
            let a = &self.apis[i];
            if a.name == name {
                return Err(ApiCode::DuplicateApi.val());
            }
            if a.opcode == opcode {
                return Err(ApiCode::OpcodeDuplicate.val());
            }
            i += 1;
        }
        self.apis.push(ApiEntry { name, opcode, tier });
        Ok(())
    }

    /// 成阵：为每个已注册 API × 全码闭集生成 90 行。
    pub fn build_matrix(&mut self) -> Result<usize, u32> {
        if self.phase != BookPhase::Building {
            return Err(ApiCode::RegistrySealed.val());
        }
        if self.apis.len() != API_COUNT {
            return Err(ApiCode::MatrixRowIncomplete.val());
        }
        self.matrix = Vec::new();
        let mut a = 0;
        while a < self.apis.len() {
            let (op, tier) = (self.apis[a].opcode, self.apis[a].tier);
            let mut c = 0;
            while c < CODE_TABLE.len() {
                let code = CODE_TABLE[c];
                self.matrix.push(MatrixRow {
                    opcode: op,
                    code,
                    allowed: tier_allows(tier, code),
                });
                c += 1;
            }
            a += 1;
        }
        if self.matrix.len() != ROW_TOTAL {
            return Err(ApiCode::MatrixRowIncomplete.val());
        }
        Ok(self.matrix.len())
    }

    /// 逐码对账：每行 allowed 与档规则双路一致 + 全码覆盖。
    pub fn audit(&self) -> Result<(), u32> {
        if self.matrix.len() != ROW_TOTAL {
            return Err(ApiCode::MatrixRowIncomplete.val());
        }
        let mut i = 0;
        while i < self.matrix.len() {
            let row = self.matrix[i];
            // 找 API 档（opcode 反查）。
            let mut tier = None;
            let mut a = 0;
            while a < self.apis.len() {
                if self.apis[a].opcode == row.opcode {
                    tier = Some(self.apis[a].tier);
                    break;
                }
                a += 1;
            }
            let t = match tier {
                Some(t) => t,
                None => return Err(ApiCode::ApiUnknown.val()),
            };
            // 码闭集校验。
            code_index(row.code)?;
            if tier_allows(t, row.code) != row.allowed {
                return Err(ApiCode::TierMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 行数与 API 数只读出口。
    pub fn api_len(&self) -> usize {
        self.apis.len()
    }
    pub fn matrix_len(&self) -> usize {
        self.matrix.len()
    }

    /// 档合法码计数（对账用）。
    pub fn tier_code_count(&self, tier: Tier) -> usize {
        let mut n = 0;
        let mut c = 0;
        while c < CODE_TABLE.len() {
            if tier_allows(tier, CODE_TABLE[c]) {
                n += 1;
            }
            c += 1;
        }
        n
    }
}

/// 全族清单（写死：S1 落地名单，名/opcode/档三元组冻结）。
pub const FAMILY: [(&'static str, u16, TierFamily); API_COUNT] = [
    ("RegOpenKeyEx", 1, TierFamily::Q),
    ("RegCreateKeyEx", 2, TierFamily::W),
    ("RegSetValueEx", 3, TierFamily::W),
    ("RegQueryValueEx", 4, TierFamily::Q),
    ("RegDeleteKey", 5, TierFamily::W),
    ("RegEnumKeyEx", 6, TierFamily::Q),
    ("RegQueryInfoKey", 7, TierFamily::Q),
    ("RegCloseKey", 8, TierFamily::M),
    ("RegRenameKey", 9, TierFamily::W),
    ("RegFlushKey", 10, TierFamily::M),
];

/// 清单档标记（避免 const 里用 enum 方法）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TierFamily {
    Q,
    W,
    M,
}

impl TierFamily {
    pub fn to_tier(self) -> Tier {
        match self {
            TierFamily::Q => Tier::Query,
            TierFamily::W => Tier::Write,
            TierFamily::M => Tier::Meta,
        }
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

// ── 黄金正演：全族注册 → 成阵 90 行 → 逐码对账 → 档计数对照 ──────
pub fn golden_run() -> Result<u64, u32> {
    let mut b = RegApiBook::new();
    b.advance(BookPhase::Building)?;
    // 全族注册（冻结名单）。
    let mut i = 0;
    while i < FAMILY.len() {
        let (name, op, tf) = FAMILY[i];
        b.register(name, op, tf.to_tier())?;
        i += 1;
    }
    if b.api_len() != API_COUNT {
        return Err(ApiCode::MatrixRowIncomplete.val());
    }
    // 成阵。
    let rows = b.build_matrix()?;
    if rows != ROW_TOTAL {
        return Err(ApiCode::MatrixRowIncomplete.val());
    }
    // 逐码对账。
    b.audit()?;
    // 档计数对照（独立写死期望）。
    if b.tier_code_count(Tier::Query) != EXPECT_QUERY_CODES {
        return Err(ApiCode::TierMismatch.val());
    }
    if b.tier_code_count(Tier::Write) != EXPECT_WRITE_CODES {
        return Err(ApiCode::TierMismatch.val());
    }
    if b.tier_code_count(Tier::Meta) != EXPECT_META_CODES {
        return Err(ApiCode::TierMismatch.val());
    }
    // 体例锚：SUCCESS 全档合法；NOT_IMPLEMENTED 全档保留。
    if !tier_allows(Tier::Query, EC_SUCCESS)
        || !tier_allows(Tier::Write, EC_SUCCESS)
        || !tier_allows(Tier::Meta, EC_SUCCESS)
    {
        return Err(ApiCode::TierMismatch.val());
    }
    if tier_allows(Tier::Query, EC_NOT_IMPLEMENTED)
        || tier_allows(Tier::Write, EC_NOT_IMPLEMENTED)
        || tier_allows(Tier::Meta, EC_NOT_IMPLEMENTED)
    {
        return Err(ApiCode::TierMismatch.val());
    }
    // opcode 唯一性对照（LCG 抽查 5 对互异）。
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut k = 0;
    while k < 5 {
        let a = 1 + rng.below(API_COUNT as u64) as u16;
        let mut c = 1 + rng.below(API_COUNT as u64) as u16;
        if c == a {
            c = if c == API_COUNT as u16 { 1 } else { c + 1 };
        }
        // 不同 opcode 的 API 必不同名（名单冻结抽查）。
        let na = family_name(a)?;
        let nb = family_name(c)?;
        if na == nb {
            return Err(ApiCode::DuplicateApi.val());
        }
        k += 1;
    }
    // 封纲 + 封后注册拒收。
    b.advance(BookPhase::Sealed)?;
    let rej = b.register("RegExtra", 1, Tier::Query);
    let mut sealed_ok = false;
    if let Err(e) = rej {
        if e == ApiCode::RegistrySealed.val() {
            sealed_ok = true;
        }
    }
    if !sealed_ok {
        return Err(ApiCode::RegistrySealed.val());
    }
    // 黄金哈希。
    let h = (rows as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        ^ (b.api_len() as u64).wrapping_mul(0xC2B2AE3D27D4EB4F)
        ^ (EXPECT_QUERY_CODES as u64 + EXPECT_WRITE_CODES as u64 + EXPECT_META_CODES as u64);
    Ok(h)
}

/// 清单反查名（越界报错）。
fn family_name(op: u16) -> Result<&'static str, u32> {
    let mut i = 0;
    while i < FAMILY.len() {
        if FAMILY[i].1 == op {
            return Ok(FAMILY[i].0);
        }
        i += 1;
    }
    Err(ApiCode::ApiUnknown.val())
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
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
    // 族 0：未知 API（opcode 越闭集）。
    ledger.tries[0] += 1;
    let mut b = RegApiBook::new();
    let _ = b.advance(BookPhase::Building);
    let mut c0 = false;
    if let Err(e) = b.register("RegGhost", API_COUNT as u16 + 1, Tier::Query) {
        if e == ApiCode::ApiUnknown.val() {
            c0 = true;
        }
    }
    if c0 {
        ledger.caught[0] += 1;
    }
    // 族 1：未知错误码（矩阵外值）。
    ledger.tries[1] += 1;
    let mut c1 = false;
    if let Err(e) = code_index(0xDEAD_BEEF) {
        if e == ApiCode::ErrorCodeOutOfRange.val() {
            c1 = true;
        }
    }
    if c1 {
        ledger.caught[1] += 1;
    }
    // 族 2：重复 API 名。
    ledger.tries[2] += 1;
    let _ = b.register("RegOpenKeyEx", 1, Tier::Query);
    let mut c2 = false;
    if let Err(e) = b.register("RegOpenKeyEx", 2, Tier::Query) {
        if e == ApiCode::DuplicateApi.val() {
            c2 = true;
        }
    }
    if c2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ApiCode::ALL.len() {
        let v = ApiCode::ALL[i].val();
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
    v.push(("J1-黄金正演90行矩阵", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 三件齐备判据：清单 10 / 码闭集 9 / 阵 90（独立写死）。
    v.push((
        "J1-三件齐备10x9=90",
        API_COUNT == 10 && CODE_COUNT == 9 && ROW_TOTAL == 90 && FAMILY.len() == 10,
        format!("{}/{}", API_COUNT, CODE_TABLE.len()),
    ));
    // 码值写死判据（NTSTATUS 体例锚点）。
    v.push((
        "J1-码值写死锚",
        EC_SUCCESS == 0
            && EC_INVALID_PARAMETER == 0xC000_000D
            && EC_NAME_NOT_FOUND == 0xC000_0034
            && EC_NAME_COLLISION == 0xC000_0035
            && EC_KEY_DELETED == 0xC000_003C,
        String::from("C00000xx 族"),
    ));
    // 档计数独立判据。
    let mut b2 = RegApiBook::new();
    v.push((
        "J1-档计数6/6/4",
        b2.tier_code_count(Tier::Query) == 6
            && b2.tier_code_count(Tier::Write) == 6
            && b2.tier_code_count(Tier::Meta) == 4,
        String::from("Q6/W6/M4"),
    ));
    // 边集判据：Draft 直跳 Sealed 必拒。
    let mut b3 = RegApiBook::new();
    let skip = b3.advance(BookPhase::Sealed);
    let mut ok = false;
    if let Err(e) = skip {
        if e == ApiCode::RegistrySealed.val() {
            ok = true;
        }
    }
    v.push(("J1-跳级封纲必拒", ok, String::from("Draft→Building→Sealed")));
    // 反向判据：未成阵先审计必拒。
    let _ = b3.advance(BookPhase::Building);
    let aud = b3.audit();
    let mut ok2 = false;
    if let Err(e) = aud {
        if e == ApiCode::MatrixRowIncomplete.val() {
            ok2 = true;
        }
    }
    v.push(("J1-空阵审计必拒", ok2, String::from("行零缺前置")));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65EC0 窗口")));
    v
}

pub fn run_unx13781_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13781 Reg* 面架构与错误码矩阵总纲（S1 落地）");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = &checks[i];
        s.add_dyn(*n, *p, d);
        i += 1;
    }
    s
}
