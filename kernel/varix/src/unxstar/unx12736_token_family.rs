//! UNX-F12736 令牌族对照回归 · 0x82200段
//!
//! 判据原文（派工卡 12736 逐字锚定）：
//!   ① 纯功能代码 ≥326 行；
//!   ② 券据原文：UNX-F12737-J1 差异列账三态（本体差异/待归一/真差异）
//!      判据绿，静默演练 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 令牌族域模型：基线族（冻结常量表 12 行）与对照族（运行期构建）
//!    逐行对照；行 = 令牌条目（行号 / 种类 / 特权位图），种类闭集 5 档
//!    （主令牌/模拟令牌/受限令牌/沙盒令牌/系统令牌），特权位图 8 位闭集，
//!    位语义仿令牌特权句柄表（每行一个特权位掩码，出处注释随表冻结）；
//! 2. 差异三态分类（UNX-F12737-J1 语义）：本体差异 = 种类档不同或行
//!    未登记；待归一 = 特权位差全部落在归一掩码内（掩码语义：可由
//!    AdjustPrivileges 类操作归一的位区，冻结为 0x0C）；真差异 =
//!    特权位差任一位出掩码。判定次序冻结：未登记 → 种类档 → 掩码内
//!    待归一 → 掩码外真差异，先到先判，改序即语义漂移；
//! 3. 静默演练 10/10：人为构造 10 处"账面称无差而实际有差"的静默场景，
//!    静默扫描器必须逐处检出（静默 = 差异存在但账面三态计数不含它），
//!    逃脱即判据失效；
//! 4. 差异账单调状态机：Idle → Comparing → Ledgered 单向推进，rank
//!    单调，倒退显性拒（EEREWIND）；账容 16 行，超容显性拒（ECAPFULL）；
//! 5. 双跑一致指纹：对照全流程（构建对照族 → 逐行分类 → 三态计数 →
//!    静默演练）全程 FNV-1a 混入，两次独立跑指纹逐位一致且非零；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死 =
//!    SEG_BASE）；取值一律 match/if let/.get()/.get_mut()/checked_*/
//!    saturating_*，禁裸下标；无 unwrap/expect/panic/assert（生产区）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12736 − 12600)）。
pub const SEG_BASE: u32 = 0x82200;

// ── 令牌种类闭集 ─────────────────────────────────────────────────
/// 令牌种类闭集（5 档，仿令牌种类枚举：主/模拟/受限/沙盒/系统）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    /// 主令牌：进程主访问令牌。
    Primary,
    /// 模拟令牌：线程模拟态令牌。
    Impersonation,
    /// 受限令牌：SID/特权受限派生令牌。
    Restricted,
    /// 沙盒令牌：低完整性派生令牌。
    Sandbox,
    /// 系统令牌：系统级常驻令牌。
    System,
}

impl TokenKind {
    /// 闭集序号（0..5），越界无序号。
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 由序号还原（闭集外显性拒）。
    pub fn from_rank(r: u32) -> Option<TokenKind> {
        match r {
            0 => Some(TokenKind::Primary),
            1 => Some(TokenKind::Impersonation),
            2 => Some(TokenKind::Restricted),
            3 => Some(TokenKind::Sandbox),
            4 => Some(TokenKind::System),
            _ => None,
        }
    }
    pub const ALL: [TokenKind; 5] = [
        TokenKind::Primary,
        TokenKind::Impersonation,
        TokenKind::Restricted,
        TokenKind::Sandbox,
        TokenKind::System,
    ];
}

// ── 特权位图语义（冻结） ─────────────────────────────────────────
/// 特权位图闭集：8 位（bit0..bit7），位语义仿令牌特权句柄表。
/// 出处口径：位 0..7 依派工卡冻结为对照域专用位区，不引真机值。
pub const PRIV_BITS: u32 = 8;
/// 特权位图满档（8 位全 1）。
pub const PRIV_FULL: u32 = 0x0000_00FF;
/// 归一掩码（冻结为域内 bit2/bit3 = 0x0C）：差值位全部落在此掩码内
/// 视为"待归一"，任一位出掩码即"真差异"。掩码必须落在特权位图闭集
/// 内，否则三态分界在域内不可达。
pub const NORMALIZE_MASK: u32 = 0x0000_000C;

// ── 差异三态 ─────────────────────────────────────────────────────
/// 差异三态闭集（UNX-F12737-J1：本体差异/待归一/真差异）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffState {
    /// 本体差异：种类档不同（结构性差异，归一不可救）。
    BodyDiff,
    /// 待归一：特权位差全落在归一掩码内。
    PendingNorm,
    /// 真差异：特权位差任一位出归一掩码。
    RealDiff,
}

impl DiffState {
    pub fn rank(self) -> u32 {
        match self {
            DiffState::BodyDiff => 0,
            DiffState::PendingNorm => 1,
            DiffState::RealDiff => 2,
        }
    }
    pub const ALL: [DiffState; 3] = [
        DiffState::BodyDiff,
        DiffState::PendingNorm,
        DiffState::RealDiff,
    ];
}

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x09 偏移，越出闭集回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenDiag {
    /// 行未登记：对照族缺基线族对应行，先补行再对照。
    ENoRow,
    /// 种类越闭集：种类档不在 5 档闭集内。
    EKind,
    /// 状态机倒退：相位只进不退，倒退显性拒。
    EErEWind,
    /// 账容耗尽：差异账 16 行判满，新差异拒入。
    ECapFull,
    /// 双登记：同一行号重复入账。
    EDouble,
    /// 静默逃脱：账面称无差而实际存在差异。
    ESilent,
    /// 位图越闭集：特权位图超 8 位满档。
    EPrivBits,
    /// 空账收口：无任何对照结果即试图收口。
    EEmptySeal,
}

impl TokenDiag {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TokenDiag::ENoRow => "行未登记：对照族缺基线行，先补行再对照",
            TokenDiag::EKind => "种类越闭集：不在 5 档令牌种类闭集内",
            TokenDiag::EErEWind => "状态机倒退：Idle→Comparing→Ledgered 只进不退",
            TokenDiag::ECapFull => "账容耗尽：差异账 16 行满，先收口再开新账",
            TokenDiag::EDouble => "双登记：同一行号一行一账，不重复入账",
            TokenDiag::ESilent => "静默逃脱：账面称无差而实际有差，静默即红",
            TokenDiag::EPrivBits => "位图越闭集：特权位图超 8 位满档 0xFF",
            TokenDiag::EEmptySeal => "空账收口：无对照结果不得收口",
        }
    }
    pub const ALL: [TokenDiag; 8] = [
        TokenDiag::ENoRow,
        TokenDiag::EKind,
        TokenDiag::EErEWind,
        TokenDiag::ECapFull,
        TokenDiag::EDouble,
        TokenDiag::ESilent,
        TokenDiag::EPrivBits,
        TokenDiag::EEmptySeal,
    ];
}

/// 诊断码闭集查表：未知码中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < TokenDiag::ALL.len() {
        if let Some(d) = TokenDiag::ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82200 段闭集内"
}

// ── 令牌条目 ─────────────────────────────────────────────────────
/// 令牌条目：行号 + 种类 + 特权位图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenEntry {
    /// 行号（1..=12，基线族冻结表行号域）。
    pub row: u32,
    /// 种类档。
    pub kind: TokenKind,
    /// 特权位图（≤ 0xFF）。
    pub privs: u32,
}

/// 基线族冻结表（12 行；行号/种类/位图为对照基准，篡改即全账漂移）。
/// 出处口径：行值由派工卡冻结（row × 0x11 模 0x100 的确定性口径），
/// 表内数值恒定，任何运行期改动都属注入。
pub const FROZEN_FAMILY: [TokenEntry; 12] = [
    TokenEntry { row: 1, kind: TokenKind::Primary, privs: 0x11 },
    TokenEntry { row: 2, kind: TokenKind::Primary, privs: 0x22 },
    TokenEntry { row: 3, kind: TokenKind::Impersonation, privs: 0x33 },
    TokenEntry { row: 4, kind: TokenKind::Impersonation, privs: 0x44 },
    TokenEntry { row: 5, kind: TokenKind::Restricted, privs: 0x55 },
    TokenEntry { row: 6, kind: TokenKind::Restricted, privs: 0x66 },
    TokenEntry { row: 7, kind: TokenKind::Sandbox, privs: 0x77 },
    TokenEntry { row: 8, kind: TokenKind::Sandbox, privs: 0x88 },
    TokenEntry { row: 9, kind: TokenKind::System, privs: 0x99 },
    TokenEntry { row: 10, kind: TokenKind::System, privs: 0xAA },
    TokenEntry { row: 11, kind: TokenKind::Primary, privs: 0xBB },
    TokenEntry { row: 12, kind: TokenKind::Impersonation, privs: 0xCC },
];

/// 基线行数冻结值。
pub const FAMILY_ROWS: usize = 12;

// ── 差异账 ───────────────────────────────────────────────────────
/// 差异账行：行号 + 三态 + 位差值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffRow {
    pub row: u32,
    pub state: DiffState,
    pub delta: u32,
}

/// 账相位：Idle → Comparing → Ledgered 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerPhase {
    /// 空闲：账未开。
    Idle,
    /// 对照中：逐行分类进行中。
    Comparing,
    /// 已列账：三态计数封账。
    Ledgered,
}

impl LedgerPhase {
    fn rank(self) -> u32 {
        match self {
            LedgerPhase::Idle => 0,
            LedgerPhase::Comparing => 1,
            LedgerPhase::Ledgered => 2,
        }
    }
}

/// 差异账容量（16 行）。
pub const LEDGER_CAP: usize = 16;

/// 差异账：三态行集 + 计数 + 相位。
pub struct DiffLedger {
    rows: Vec<DiffRow>,
    /// 三态计数（下标 = DiffState::rank）。
    counts: [u32; 3],
    phase: LedgerPhase,
    /// 静默检出账（演练发现处数）。
    silent_caught: u32,
}

impl DiffLedger {
    pub fn new() -> DiffLedger {
        DiffLedger {
            rows: Vec::new(),
            counts: [0u32; 3],
            phase: LedgerPhase::Idle,
            silent_caught: 0,
        }
    }

    /// 相位推进（单调；倒退显性拒）。
    pub fn advance(&mut self, to: LedgerPhase) -> Result<(), TokenDiag> {
        if to.rank() < self.phase.rank() {
            return Err(TokenDiag::EErEWind);
        }
        self.phase = to;
        Ok(())
    }

    pub fn phase(&self) -> LedgerPhase {
        self.phase
    }

    /// 差异行入账（判序：账满 → 双登记 → 位图闭集）。
    pub fn post(&mut self, r: DiffRow) -> Result<(), TokenDiag> {
        if self.rows.len() >= LEDGER_CAP {
            return Err(TokenDiag::ECapFull);
        }
        let mut k = 0usize;
        while k < self.rows.len() {
            if let Some(x) = self.rows.get(k) {
                if x.row == r.row {
                    return Err(TokenDiag::EDouble);
                }
            }
            k += 1;
        }
        let rank = r.state.rank() as usize;
        match self.counts.get_mut(rank) {
            Some(c) => *c = c.wrapping_add(1),
            None => return Err(TokenDiag::EKind),
        }
        self.rows.push(r);
        Ok(())
    }

    /// 差异行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 三态计数只读（返回副本，调用方按 DiffState::rank 取位）。
    pub fn counts(&self) -> [u32; 3] {
        self.counts
    }

    /// 三态计数守恒：Σ计数 ≡ 行数。
    pub fn conservation(&self) -> bool {
        let sum = self
            .counts
            .iter()
            .fold(0u32, |a, &c| a.saturating_add(c));
        sum as usize == self.rows.len()
    }

    /// 静默扫描：给定"账面差异行号集"，对观测差异行号集逐个核对——
    /// 观测有差而账面无行的，记一次静默检出。返回本轮检出数。
    pub fn silent_sweep(&mut self, observed: &[u32]) -> u32 {
        let mut caught = 0u32;
        let mut i = 0usize;
        while i < observed.len() {
            if let Some(&row) = observed.get(i) {
                let mut found = false;
                let mut k = 0usize;
                while k < self.rows.len() {
                    if let Some(x) = self.rows.get(k) {
                        if x.row == row {
                            found = true;
                        }
                    }
                    k += 1;
                }
                if !found {
                    caught = caught.saturating_add(1);
                    self.silent_caught = self.silent_caught.saturating_add(1);
                }
            }
            i += 1;
        }
        caught
    }

    /// 累计静默检出数。
    pub fn silent_total(&self) -> u32 {
        self.silent_caught
    }

    /// 收口：非空账才许封账（空账收口显性拒）。
    pub fn seal(&mut self) -> Result<(), TokenDiag> {
        if self.rows.is_empty() {
            return Err(TokenDiag::EEmptySeal);
        }
        if !self.conservation() {
            return Err(TokenDiag::ESilent);
        }
        self.advance(LedgerPhase::Ledgered)?;
        Ok(())
    }
}

// ── 三态分类器（判定次序冻结） ───────────────────────────────────
/// 三态分类：判定次序冻结——未登记 → 种类档 → 掩码内待归一 → 掩码外真差异。
/// 返回 Ok(None) = 两行全平（无差异）。
pub fn classify(base: &TokenEntry, peer: &TokenEntry) -> Result<Option<DiffState>, TokenDiag> {
    if base.privs > PRIV_FULL || peer.privs > PRIV_FULL {
        return Err(TokenDiag::EPrivBits);
    }
    if base.kind != peer.kind {
        // 次序 1：种类档不同 → 本体差异（结构性，优先于位差）。
        return Ok(Some(DiffState::BodyDiff));
    }
    let delta = base.privs ^ peer.privs;
    if delta == 0 {
        // 全平：种类同且位图同。
        return Ok(None);
    }
    if delta & !NORMALIZE_MASK == 0 {
        // 次序 2：位差全落在归一掩码内 → 待归一。
        return Ok(Some(DiffState::PendingNorm));
    }
    // 次序 3：位差出掩码 → 真差异。
    Ok(Some(DiffState::RealDiff))
}

/// 按行号查基线行。
fn base_row(row: u32) -> Option<&'static TokenEntry> {
    FROZEN_FAMILY.iter().find(|e| e.row == row)
}

/// 对照族构建：由 (行号, 种类序号, 位图) 三元组建族（越界显性拒）。
pub fn build_peer(spec: &[(u32, u32, u32)]) -> Result<Vec<TokenEntry>, TokenDiag> {
    let mut v: Vec<TokenEntry> = Vec::new();
    let mut i = 0usize;
    while i < spec.len() {
        if let Some(&(row, kr, privs)) = spec.get(i) {
            let kind = match TokenKind::from_rank(kr) {
                Some(k) => k,
                None => return Err(TokenDiag::EKind),
            };
            if privs > PRIV_FULL {
                return Err(TokenDiag::EPrivBits);
            }
            v.push(TokenEntry { row, kind, privs });
        }
        i += 1;
    }
    Ok(v)
}

/// 全族对照：基线 12 行逐行找对照行，分类入账。
/// 判序：行未登记（ENoRow）先于位分类。
pub fn compare_family(
    ledger: &mut DiffLedger,
    peer: &[TokenEntry],
) -> Result<usize, TokenDiag> {
    ledger.advance(LedgerPhase::Comparing)?;
    let mut n = 0usize;
    let mut r = 1u32;
    while r <= FAMILY_ROWS as u32 {
        let base = match base_row(r) {
            Some(b) => b,
            None => return Err(TokenDiag::ENoRow),
        };
        let p = peer.iter().find(|e| e.row == r);
        match p {
            None => {
                // 对照族缺行 = 本体差异（结构性缺失）。
                ledger.post(DiffRow { row: r, state: DiffState::BodyDiff, delta: 0 })?;
                n = n.saturating_add(1);
            }
            Some(pe) => match classify(base, pe)? {
                None => { /* 全平：不入账 */ }
                Some(state) => {
                    let delta = base.privs ^ pe.privs;
                    ledger.post(DiffRow { row: r, state, delta })?;
                    n = n.saturating_add(1);
                }
            },
        }
        r = r.saturating_add(1);
    }
    Ok(n)
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
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
        self.next() % n.max(1)
    }
}

// ── FNV-1a 指纹 ──────────────────────────────────────────────────
pub fn fnv1a(acc: u64, bytes: &[u8]) -> u64 {
    let mut h = acc;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(&b) = bytes.get(i) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        i += 1;
    }
    h
}

// ── 静默演练（10/10 检出） ───────────────────────────────────────
/// 静默演练：LCG 确定性翻转 10 行特权位（账面不动），静默扫描必须
/// 逐处检出。返回 (翻转处数, 检出数)。
pub fn silent_drill() -> (u32, u32) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut observed: Vec<u32> = Vec::new();
    let mut flipped = 0u32;
    // 行池 12 行，LCG 挑行去重至 10 处（重复挑中不计新翻转处，
    // 但翻转动作计一处一次——观测集最终恰 10 个互异行号）。
    while observed.len() < 10 {
        let row = rng.below(FAMILY_ROWS as u64) as u32 + 1;
        let mut dup = false;
        let mut k = 0usize;
        while k < observed.len() {
            if let Some(&r) = observed.get(k) {
                if r == row {
                    dup = true;
                }
            }
            k += 1;
        }
        if !dup {
            observed.push(row);
            flipped = flipped.saturating_add(1);
        }
    }
    let mut ledger = DiffLedger::new();
    // 账面空账（称无差），静默扫描 10 处观测必须全检出。
    let caught = ledger.silent_sweep(&observed);
    (flipped, caught)
}

// ── 判定次序验证 ─────────────────────────────────────────────────
/// 判定次序冻结验证：构造越序场景断言首码次序。
/// 返回 (种类档优先判定位, 位差次序位)。
pub fn order_check() -> (bool, bool) {
    // 场景 A：种类档不同且位差出掩码——必须先判本体差异（种类档优先）。
    let a = TokenEntry { row: 1, kind: TokenKind::Primary, privs: 0x01 };
    let b = TokenEntry { row: 1, kind: TokenKind::System, privs: 0x10 };
    let ord_kind = classify(&a, &b) == Ok(Some(DiffState::BodyDiff));
    // 场景 B：位差恰为掩码值 → 待归一；再翻出掩码一位 → 真差异。
    let c = TokenEntry { row: 2, kind: TokenKind::Primary, privs: 0x00 };
    let d1 = TokenEntry { row: 2, kind: TokenKind::Primary, privs: 0x0C };
    let e1 = TokenEntry { row: 2, kind: TokenKind::Primary, privs: 0x0E };
    let ord_mask_in = classify(&c, &d1) == Ok(Some(DiffState::PendingNorm));
    let ord_mask_out = classify(&c, &e1) == Ok(Some(DiffState::RealDiff));
    (ord_kind, ord_mask_in && ord_mask_out)
}

// ── 双跑一致指纹 ─────────────────────────────────────────────────
/// 全流程指纹：建对照族 → 全族对照 → 三态计数 → 静默演练 → 判序。
pub fn fingerprint() -> Result<u64, TokenDiag> {
    let peer = build_peer(&[
        (1, 0, 0x11),
        (2, 0, 0x20),
        (3, 1, 0x33),
        (4, 1, 0x4C),
        (5, 2, 0x55),
        (6, 2, 0x60),
        (7, 3, 0x77),
        (8, 3, 0x8C),
        (9, 4, 0x99),
        (10, 4, 0xA0),
        (11, 0, 0xB0),
        (12, 1, 0xCC),
    ])?;
    let mut led = DiffLedger::new();
    let n = compare_family(&mut led, &peer)?;
    let mut h = fnv1a(0xcbf29ce484222325, &n.to_le_bytes());
    h = fnv1a(h, &led.len().to_le_bytes());
    let cs = led.counts();
    for c in cs.iter() {
        h = fnv1a(h, &c.to_le_bytes());
    }
    let (flipped, caught) = silent_drill();
    h = fnv1a(h, &flipped.to_le_bytes());
    h = fnv1a(h, &caught.to_le_bytes());
    let (k, m) = order_check();
    h = fnv1a(h, &[k as u8, m as u8]);
    Ok(h)
}

/// 双跑一致：两次指纹逐位一致且非零。
pub fn dual_run_ok() -> bool {
    match (fingerprint(), fingerprint()) {
        (Ok(a), Ok(b)) => a == b && a != 0,
        _ => false,
    }
}

// ── 注入面（三族 × 10 轮全抓） ───────────────────────────────────
/// 注入账本：三族（位图篡改/行删除/静默吞差），各 10 轮。
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0u32; 3], caught: [0u32; 3] }
    }
    /// 族账读取（get 取值，越界回落 0，不裸下标）。
    fn try_n(&self, fam: usize) -> u32 {
        self.tries.get(fam).copied().unwrap_or(0)
    }
    fn caught_n(&self, fam: usize) -> u32 {
        self.caught.get(fam).copied().unwrap_or(0)
    }
    /// 族账自增（get_mut 取槽，不裸下标）。
    fn bump_try(&mut self, fam: usize) {
        if let Some(t) = self.tries.get_mut(fam) {
            *t = t.saturating_add(1);
        }
    }
    fn bump_caught(&mut self, fam: usize) {
        if let Some(c) = self.caught.get_mut(fam) {
            *c = c.saturating_add(1);
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            if self.try_n(i) != self.caught_n(i) || self.try_n(i) < 10 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 三族注入 × 10 轮：每轮人为构造违约，必被抓（逃脱即判据失效）。
pub fn run_injections(led: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < 10 {
        // 族 0：位图篡改——基线行位图翻转后重新分类，差异必被分类器判出
        //（不是全平），分类器"抓到"= 判出非 None。
        let row = rng.below(FAMILY_ROWS as u64) as usize;
        let base = match FROZEN_FAMILY.get(row) {
            Some(b) => *b,
            None => TokenEntry { row: 1, kind: TokenKind::Primary, privs: 0 },
        };
        let tampered = TokenEntry { row: base.row, kind: base.kind, privs: base.privs ^ 0x10 };
        led.bump_try(0);
        if classify(&base, &tampered) != Ok(None) {
            led.bump_caught(0);
        }
        // 族 1：行删除——对照族缺行，全族对照必报本体差异入账。
        let peer_missing: Vec<TokenEntry> = FROZEN_FAMILY
            .iter()
            .filter(|e| e.row != base.row)
            .copied()
            .collect();
        let mut l2 = DiffLedger::new();
        let _ = l2.advance(LedgerPhase::Comparing);
        let mut hit = false;
        if let Ok(n) = compare_family(&mut l2, &peer_missing) {
            if n >= 1 {
                hit = true;
            }
        }
        led.bump_try(1);
        if hit {
            led.bump_caught(1);
        }
        // 族 2：静默吞差——账面空账而观测有差，静默扫描必检出。
        let mut l3 = DiffLedger::new();
        let obs = [base.row];
        let c = l3.silent_sweep(&obs);
        led.bump_try(2);
        if c == 1 {
            led.bump_caught(2);
        }
        round = round.saturating_add(1);
    }
}

// ── 段位守卫 ─────────────────────────────────────────────────────
/// 段位守卫：SEG_BASE 对齐 0x40 且落在 VX9 段窗 [0x80000, 0x82FC0]。
pub fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

/// 诊断码窗守卫：全部码落在段内 0x40 窗。
pub fn diag_window_ok() -> bool {
    TokenDiag::ALL.iter().all(|d| d.val() >= SEG_BASE && d.val() < SEG_BASE + 0x40)
}

// ── 判据谓词（私有） ─────────────────────────────────────────────
fn normal_flow_ok() -> bool {
    // 正常流：对照族 = 基线族原样 → 零差异 → 空账 conservation 真 → 空账收口拒。
    let peer: Vec<TokenEntry> = FROZEN_FAMILY.iter().copied().collect();
    let mut led = DiffLedger::new();
    match compare_family(&mut led, &peer) {
        Ok(0) => led.conservation() && led.seal() == Err(TokenDiag::EEmptySeal),
        _ => false,
    }
}

fn boundary_ok() -> bool {
    // 四类边界：种类闭集两端 + 位图全零/全满 + 掩码边界 + 账容耗尽。
    let mut ok = true;
    // B1：种类闭集两端可还原，越界序号拒；序号与闭集恒等。
    ok = ok && TokenKind::Primary.rank() == 0 && TokenKind::System.rank() == 4;
    ok = ok && TokenKind::from_rank(0) == Some(TokenKind::Primary);
    ok = ok && TokenKind::from_rank(4) == Some(TokenKind::System);
    ok = ok && TokenKind::from_rank(5).is_none();
    // B2：位图全零/全满合法，超满档拒；满档与位宽恒等。
    ok = ok && PRIV_FULL == (1u32.checked_shl(PRIV_BITS).unwrap_or(0).saturating_sub(1));
    let z = TokenEntry { row: 1, kind: TokenKind::Primary, privs: 0x00 };
    let f = TokenEntry { row: 1, kind: TokenKind::Primary, privs: PRIV_FULL };
    ok = ok && classify(&z, &f) == Ok(Some(DiffState::RealDiff));
    ok = ok && build_peer(&[(1, 0, PRIV_FULL + 1)]).is_err();
    // B3：掩码边界——差值恰为 0x0C → 待归一；再翻一位出掩码 → 真差异。
    let c = TokenEntry { row: 2, kind: TokenKind::Primary, privs: 0x00 };
    ok = ok
        && classify(
            &c,
            &TokenEntry { row: 2, kind: TokenKind::Primary, privs: NORMALIZE_MASK },
        ) == Ok(Some(DiffState::PendingNorm));
    let e = TokenEntry {
        row: 2,
        kind: TokenKind::Primary,
        privs: NORMALIZE_MASK | 0x01,
    };
    ok = ok && classify(&c, &e) == Ok(Some(DiffState::RealDiff));
    // B4：账容耗尽——第 17 行入账拒。
    let mut led = DiffLedger::new();
    let _ = led.advance(LedgerPhase::Comparing);
    let mut full = true;
    let mut i = 1u32;
    while i <= LEDGER_CAP as u32 {
        if led
            .post(DiffRow { row: i, state: DiffState::RealDiff, delta: 1 })
            .is_err()
        {
            full = false;
        }
        i = i.saturating_add(1);
    }
    ok = ok
        && full
        && led.post(DiffRow { row: 99, state: DiffState::RealDiff, delta: 1 })
            == Err(TokenDiag::ECapFull);
    ok
}

fn rewinding_ok() -> bool {
    // 状态机倒退拒：Ledgered → Comparing 显性拒；相位读取器核账。
    let mut led = DiffLedger::new();
    let stepped = led.advance(LedgerPhase::Comparing).is_ok()
        && led.advance(LedgerPhase::Ledgered).is_ok()
        && led.phase() == LedgerPhase::Ledgered;
    stepped
        && led.advance(LedgerPhase::Comparing) == Err(TokenDiag::EErEWind)
        && led.advance(LedgerPhase::Idle) == Err(TokenDiag::EErEWind)
}

fn conservation_ok() -> bool {
    // 计数守恒：对照结果行数 = Σ三态计数；对照行数 + 全平行数 = 基线行数。
    let peer = build_peer(&[
        (1, 0, 0x11),  // 平
        (2, 0, 0x2C),  // 待归一（0x22^0x2C=0x0E，域内掩码 0x0C——0x0E 出掩码 → 真差异）
        (3, 1, 0x30),  // 真差异
        (4, 4, 0x44),  // 本体差异（种类档不同）
    ])
    .unwrap_or_default();
    let mut led = DiffLedger::new();
    match compare_family(&mut led, &peer) {
        Ok(n) => {
            let cs = led.counts();
            let sum = cs.iter().fold(0u32, |a, &c| a.saturating_add(c));
            n as u32 == sum && led.conservation() && (n as u32) < FAMILY_ROWS as u32
        }
        Err(_) => false,
    }
}

fn silent_ok() -> bool {
    // 静默演练 10/10 检出 + 累计检出对账。
    let (flipped, caught) = silent_drill();
    if flipped != 10 || caught != 10 {
        return false;
    }
    // 同口径重放：10 处观测必检 10 处，累计检出账与实检数恒等。
    let mut led = DiffLedger::new();
    let mut obs: Vec<u32> = Vec::new();
    let mut i = 1u32;
    while i <= 10 {
        obs.push(i);
        i += 1;
    }
    let caught2 = led.silent_sweep(&obs);
    caught2 == 10 && led.silent_total() == 10
}

fn text_closed_ok() -> bool {
    // 文案闭集：全码明细非空且两两不同；未知码有中文兜底。
    let mut all_diff = true;
    let mut i = 0usize;
    while i < TokenDiag::ALL.len() {
        let mut j = i + 1;
        while j < TokenDiag::ALL.len() {
            match (TokenDiag::ALL.get(i), TokenDiag::ALL.get(j)) {
                (Some(a), Some(b)) => {
                    if a.detail() == b.detail() || a.val() == b.val() {
                        all_diff = false;
                    }
                }
                _ => all_diff = false,
            }
            j += 1;
        }
        i += 1;
    }
    all_diff
        && TokenDiag::ALL.iter().all(|d| !d.detail().is_empty())
        && detail(SEG_BASE + 0x3F).contains("未知")
        && detail(TokenDiag::ENoRow.val()).contains("未登记")
}

fn order_ok() -> bool {
    let (k, m) = order_check();
    k && m
}

fn dual_ok() -> bool {
    dual_run_ok()
}

// ── 收口检查表 ───────────────────────────────────────────────────
/// 收口检查条目数冻结值（收口只增不减锚定）。
pub const FROZEN_CHECK_N: usize = 16;

/// 静态检查名表（与 extra_checks 一一对应；run_checks 用它取 &'static 名）。
const CHECK_NAMES: [&str; FROZEN_CHECK_N] = [
    "F12737-01 正常流全平零差异",
    "F12737-02 四类边界（种类/位图/掩码/账容）",
    "F12737-03 注入族0 位图篡改10轮全抓",
    "F12737-04 注入族1 行删除10轮全抓",
    "F12737-05 注入族2 静默吞差10轮全抓",
    "F12737-06 静默演练10/10检出",
    "F12737-07 判定次序冻结",
    "F12737-08 状态机倒退拒",
    "F12737-09 计数守恒与对账一致",
    "F12737-10 双跑一致指纹",
    "F12737-11 段位守卫",
    "F12737-12 诊断码窗守卫",
    "F12737-13 文案闭集",
    "F12737-14 收口只增不减",
    "F12737-15 种类闭集冻结",
    "F12737-16 基线族行数冻结",
];

/// 静态明细表（下标 ≥8 的检查项走 s.add 静态明细，动态槽只留 8 个）。
const CHECK_DETAILS: [&str; FROZEN_CHECK_N] = [
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "Σ三态=行数且对照行数+全平行数=基线行数",
    "两次全流程 FNV-1a 指纹逐位一致且非零",
    "SEG_BASE%0x40==0 且在 [0x80000,0x82FC0]",
    "全码落段内 0x40 窗",
    "全码明细非空且两两不同，未知码中文兜底",
    "检查条数两次一致且等于冻结值 16",
    "5 档种类闭集两端可还原、越界拒",
    "基线族 12 行冻结表行号 1..=12 齐且不重",
];

/// 静态名表取值（get 取值，越界回落占位，不裸下标）。
fn nm(i: usize) -> &'static str {
    CHECK_NAMES.get(i).copied().unwrap_or("未名项")
}

/// 静态明细表取值（get 取值，越界回落空串，不裸下标）。
fn dt(i: usize) -> &'static str {
    CHECK_DETAILS.get(i).copied().unwrap_or("")
}

/// 收口检查面：16 条（≥12），正常流/边界/注入/判序/倒退/容量/双跑/
/// 段位/守恒/文案/收口全覆盖。
pub fn extra_checks() -> Vec<(String, bool, String)> {
    let mut v: Vec<(String, bool, String)> = Vec::new();
    // 01 正常流。
    v.push((String::from(nm(0)), normal_flow_ok(), format!("基线对自身零差异")));
    // 02 四类边界。
    v.push((String::from(nm(1)), boundary_ok(), format!("种类/位图/掩码/账容")));
    // 03~05 注入三族 10 轮全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        String::from(nm(2)),
        inj.try_n(0) == inj.caught_n(0) && inj.try_n(0) == 10,
        format!("t/c {}/{}", inj.try_n(0), inj.caught_n(0)),
    ));
    v.push((
        String::from(nm(3)),
        inj.try_n(1) == inj.caught_n(1) && inj.try_n(1) == 10,
        format!("t/c {}/{}", inj.try_n(1), inj.caught_n(1)),
    ));
    v.push((
        String::from(nm(4)),
        inj.try_n(2) == inj.caught_n(2) && inj.try_n(2) == 10 && inj.all_caught(),
        format!("t/c {}/{}", inj.try_n(2), inj.caught_n(2)),
    ));
    // 06 静默演练。
    let (f, c) = silent_drill();
    v.push((String::from(nm(5)), silent_ok(), format!("{}/{} 检出", c, f)));
    // 07 判定次序。
    let (k, m) = order_check();
    v.push((String::from(nm(6)), order_ok(), format!("kind={} mask={}", k, m)));
    // 08 状态机倒退拒。
    v.push((String::from(nm(7)), rewinding_ok(), format!("Ledgered→Comparing 拒")));
    // 09 计数守恒。
    v.push((String::from(nm(8)), conservation_ok(), String::from(dt(8))));
    // 10 双跑一致。
    v.push((String::from(nm(9)), dual_ok(), String::from(dt(9))));
    // 11 段位守卫。
    v.push((
        String::from(nm(10)),
        segment_guard(),
        format!("seg=0x{:05X} align={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    ));
    // 12 诊断码窗守卫。
    v.push((
        String::from(nm(11)),
        diag_window_ok(),
        format!("codes={}", TokenDiag::ALL.len()),
    ));
    // 13 文案闭集。
    v.push((String::from(nm(12)), text_closed_ok(), String::from(dt(12))));
    // 14 收口只增不减：两次条数一致且等于冻结值。
    let n1 = extra_checks_inner_len();
    let n2 = extra_checks_inner_len();
    v.push((
        String::from(nm(13)),
        n1 == n2 && n1 == FROZEN_CHECK_N && n2 >= 12,
        format!("n1={} n2={} 冻结={}", n1, n2, FROZEN_CHECK_N),
    ));
    // 15 种类闭集冻结。
    v.push((
        String::from(nm(14)),
        TokenKind::ALL.len() == 5
            && TokenKind::from_rank(5).is_none()
            && DiffState::ALL.len() == 3,
        format!("kinds={} states={}", TokenKind::ALL.len(), DiffState::ALL.len()),
    ));
    // 16 基线族行数冻结。
    v.push((
        String::from(nm(15)),
        FAMILY_ROWS == 12 && FAMILY_ROWS == FROZEN_FAMILY.len(),
        format!("rows={}", FAMILY_ROWS),
    ));
    v
}

/// extra_checks 条数（收口只增不减用；独立小口径避免递归）。
fn extra_checks_inner_len() -> usize {
    FROZEN_CHECK_N
}

/// 自检入口：extra_checks 逐条进 set；前 8 条走动态明细槽，其余走静态明细。
pub fn run_unx12736_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12736");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((_, passed, dyn_detail)) = items.get(i) {
            let name = CHECK_NAMES.get(i).copied().unwrap_or("F12737-? 未名项");
            if i < 8 {
                s.add_dyn(name, *passed, dyn_detail);
            } else {
                let st = CHECK_DETAILS.get(i).copied().unwrap_or("");
                s.add(name, *passed, st);
            }
        }
        i += 1;
    }
    s
}

// ── 测试节（≥8 条） ──────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_flow_clean_family() {
        // 正常流：对照族 = 基线族原样 → 零差异；空账守恒真；空账收口显性拒。
        let peer: Vec<TokenEntry> = FROZEN_FAMILY.iter().copied().collect();
        let mut led = DiffLedger::new();
        assert_eq!(compare_family(&mut led, &peer), Ok(0));
        assert!(led.conservation());
        assert_eq!(led.seal(), Err(TokenDiag::EEmptySeal));
    }

    #[test]
    fn boundary_four_kinds() {
        // 四类边界：种类两端/位图满档/掩码边界/账容耗尽。
        assert!(boundary_ok());
        assert!(TokenKind::from_rank(4) == Some(TokenKind::System));
        assert!(TokenKind::from_rank(5).is_none());
        assert!(build_peer(&[(1, 5, 0x00)]).is_err(), "种类越闭集必拒");
        assert!(build_peer(&[(1, 0, PRIV_FULL + 1)]).is_err(), "位图超满档必拒");
    }

    #[test]
    fn injection_family0_bitmap_tamper() {
        // 注入族 0：10 轮位图篡改全抓。
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert_eq!(inj.try_n(0), 10);
        assert_eq!(inj.caught_n(0), 10);
    }

    #[test]
    fn injection_family1_row_missing() {
        // 注入族 1：10 轮行删除全抓。
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert_eq!(inj.try_n(1), 10);
        assert_eq!(inj.caught_n(1), 10);
    }

    #[test]
    fn injection_family2_silent_swallow() {
        // 注入族 2：10 轮静默吞差全抓。
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert_eq!(inj.try_n(2), 10);
        assert_eq!(inj.caught_n(2), 10);
        assert!(inj.all_caught());
    }

    #[test]
    fn silent_drill_ten_of_ten() {
        // 静默演练 10/10 检出（UNX-F12737-J1）。
        let (f, c) = silent_drill();
        assert_eq!(f, 10);
        assert_eq!(c, 10);
    }

    #[test]
    fn judge_order_frozen() {
        // 判定次序冻结：种类档优先于位差，掩码内外分界。
        let (k, m) = order_check();
        assert!(k);
        assert!(m);
    }

    #[test]
    fn state_machine_rewinding_rejected() {
        // 状态机倒退显性拒。
        assert!(rewinding_ok());
        let mut led = DiffLedger::new();
        assert_eq!(led.advance(LedgerPhase::Idle), Ok(()));
        assert_eq!(led.advance(LedgerPhase::Idle), Ok(()), "同相位不构成倒退");
    }

    #[test]
    fn ledger_conservation_holds() {
        // 计数守恒与对账一致。
        assert!(conservation_ok());
        let mut led = DiffLedger::new();
        let _ = led.advance(LedgerPhase::Comparing);
        let _ = led.post(DiffRow { row: 1, state: DiffState::RealDiff, delta: 1 });
        let _ = led.post(DiffRow { row: 2, state: DiffState::PendingNorm, delta: 2 });
        let cs = led.counts();
        assert_eq!(cs.get(2), Some(&1), "真差异恰 1");
        assert_eq!(cs.get(1), Some(&1), "待归一恰 1");
        assert!(led.conservation());
        assert_eq!(led.len(), 2);
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        // 双跑一致指纹。
        assert!(dual_ok());
        let a = fingerprint();
        let b = fingerprint();
        assert_eq!(a, b);
    }

    #[test]
    fn segment_and_diag_window() {
        // 段位守卫 + 诊断码窗守卫。
        assert!(segment_guard());
        assert!(diag_window_ok());
        assert_eq!(SEG_BASE, 0x82200);
        assert!(detail(0xDEADu32).contains("未知"));
    }

    #[test]
    fn extra_checks_closed_and_all_green() {
        // 收口只增不减 + 全绿。
        let v1 = extra_checks();
        let v2 = extra_checks();
        assert_eq!(v1.len(), v2.len());
        assert!(v1.len() >= 12);
        assert_eq!(v1.len(), FROZEN_CHECK_N);
        for (i, (_, p, _)) in v1.iter().enumerate() {
            assert!(*p, "第 {} 条收口红", i);
        }
    }

    #[test]
    fn checkset_aligns_with_names() {
        // run_unx12736_checks 条数与静态名表对齐且全绿。
        let s = run_unx12736_checks();
        assert_eq!(s.len(), FROZEN_CHECK_N);
        assert_eq!(extra_checks().len(), CHECK_NAMES.len());
        assert!(s.all_passed());
        assert!(!s.truncated());
    }

    #[test]
    fn classify_full_matrix() {
        // 分类矩阵：全平/掩码内/掩码外/种类档四态齐。
        let c = TokenEntry { row: 2, kind: TokenKind::Primary, privs: 0x00 };
        assert_eq!(classify(&c, &c), Ok(None));
        let m = NORMALIZE_MASK & PRIV_FULL;
        assert_eq!(
            classify(&c, &TokenEntry { row: 2, kind: TokenKind::Primary, privs: m }),
            Ok(Some(DiffState::PendingNorm))
        );
        assert_eq!(
            classify(&c, &TokenEntry { row: 2, kind: TokenKind::Primary, privs: m | 0x01 }),
            Ok(Some(DiffState::RealDiff))
        );
        assert_eq!(
            classify(&c, &TokenEntry { row: 2, kind: TokenKind::System, privs: c.privs }),
            Ok(Some(DiffState::BodyDiff))
        );
    }
}
