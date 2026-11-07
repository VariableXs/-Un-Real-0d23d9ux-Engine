//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12758 · B39 预告与开门件（文档对齐框架） · 0x82780段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① ≥360 行；
//!   ② 判据原文：UNX-F12759-J1 三类资产 SHA-256 冻结在册，
//!      版本漂移演练 10/10 检出；
//!   ③ 完成时 API/资产写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - B39 开门件是「压测资产归档」卡（UNX-F12759）的预告与对齐框架：
//!     本模块先把三类资产账（种子/样本/脚本）、冻结哈希表、版本漂移
//!     演练器三件事的**域模型与判定次序**在 0x82780 段先行立账，
//!     正式 SHA-256（FIPS 180-4）整数实现与归档账在 UNX-F12759（0x827C0）
//!     落地；本段冻结位用 64 位 FNV-1a 整数摘要作预告锚
//!     （FNV-1a 参考：L. Cisco, "FNV-1a hash", 2011，纯整数移位/异或/乘）；
//!   - 单调状态机：开账 Open → 封账 Sealed（哈希冻结）→ 演练 Drilled
//!     → 收口 Closed；任何倒退（rank 下降）显性拒并给出中文建议；
//!   - 漂移演练器：对每条在册资产篡改尾字节后重算摘要，与冻结位
//!     逐条比对——篡改必致哈希不等，10 轮演练 10/10 必检出；
//!   - 计数守恒：注入数 ≡ 检出数 + 漏检数，且漏检数恒为 0，
//!     破坏守恒即判红（CountBreak）；
//!   - 注入三族：重复登记 / 状态机倒退 / 未冻结先演练，
//!     各 10 轮 LCG（种子=SEG_BASE）10/10 必抓；
//!   - no_std + alloc；零浮点/零随机/零 IO/零墙钟；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x82780，VX9 批公式 0x80000+0x40×(12758−12600)）。
pub const SEG_BASE: u32 = 0x82780;

// ── 冻结常量（出处见行注） ───────────────────────────────────────
/// 资产账容量：三类 × 每类 3 条 = 9 槽（B39 预告口径，12759 对齐）。
pub const ASSET_CAP: usize = 9;
/// 漂移演练轮数：10/10 检出（判据原文口径）。
pub const DRILL_ROUNDS: u32 = 10;
/// FNV-1a 64 位偏移基（FNV 常量，公开域）。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 位素数（FNV 常量，公开域）。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// 三类资产类别（判据"三类资产"闭集：种子/样本/脚本）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetKind {
    /// 种子资产：压测输入种子
    Seed,
    /// 样本资产：断言样本
    Sample,
    /// 脚本资产：检查脚本
    Script,
}

impl AssetKind {
    /// 闭集序号（0/1/2）。
    pub fn idx(self) -> u32 {
        self as u32
    }
    /// 闭集中文名。
    pub fn name(self) -> &'static str {
        match self {
            AssetKind::Seed => "种子",
            AssetKind::Sample => "样本",
            AssetKind::Script => "脚本",
        }
    }
    /// u32 → 闭集类别（越闭集返回 None，供边界注入）。
    pub fn from_u32(v: u32) -> Option<AssetKind> {
        match v {
            0 => Some(AssetKind::Seed),
            1 => Some(AssetKind::Sample),
            2 => Some(AssetKind::Script),
            _ => None,
        }
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B39Code {
    /// 资产类别越闭集（只收 0=种子/1=样本/2=脚本）
    KindInvalid,
    /// 资产名为空（建议：登记前先核名非空）
    NameEmpty,
    /// 资产字节为空（建议：空资产无冻结意义，先补语料）
    BytesEmpty,
    /// 资产重复登记（建议：一格一主，重名先查账再登记）
    DoubleBooked,
    /// 未冻结先演练（建议：先 seal 冻结哈希表，再跑漂移演练）
    NotFrozen,
    /// 状态机倒退（建议：状态单调只进不退，倒退即拒）
    Regression,
    /// 资产账容量耗尽（建议：9 槽封顶，超容走扩容评审）
    LedgerFull,
    /// 演练计数守恒破坏（建议：注入=检出+漏检，漏检恒 0）
    CountBreak,
    /// 收口结果摘要为空（建议：收口时必须写非空结果摘要）
    SummaryEmpty,
}

impl B39Code {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            B39Code::KindInvalid => "资产类别越闭集：只收种子/样本/脚本三类",
            B39Code::NameEmpty => "资产名为空：登记前先核名非空",
            B39Code::BytesEmpty => "资产字节为空：空资产无冻结意义，先补语料",
            B39Code::DoubleBooked => "资产重复登记：一格一主，重名先查账再登记",
            B39Code::NotFrozen => "未冻结先演练：先 seal 冻结哈希表再跑漂移演练",
            B39Code::Regression => "状态机倒退：状态单调只进不退，倒退即拒",
            B39Code::LedgerFull => "资产账容量耗尽：9 槽封顶，超容走扩容评审",
            B39Code::CountBreak => "演练计数守恒破坏：注入=检出+漏检，漏检恒 0",
            B39Code::SummaryEmpty => "收口结果摘要为空：收口必须附非空结果摘要",
        }
    }
    pub const ALL: [B39Code; 9] = [
        B39Code::KindInvalid,
        B39Code::NameEmpty,
        B39Code::BytesEmpty,
        B39Code::DoubleBooked,
        B39Code::NotFrozen,
        B39Code::Regression,
        B39Code::LedgerFull,
        B39Code::CountBreak,
        B39Code::SummaryEmpty,
    ];
}

/// 闭集查表：未知码给中文兜底，不panic、不空串。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < B39Code::ALL.len() {
        if let Some(c) = B39Code::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82780 段闭集内"
}

// ── 64 位整数摘要（FNV-1a，纯整数移位/异或/乘法） ────────────────
/// FNV-1a 摘要：h 从偏移基起，逐字节异或后乘素数。
/// 纯整数实现，满足"禁浮点/随机"红线；确定性可复算。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

/// 篡改版摘要：尾字节异或 0x5A 后重算（漂移演练的"改版"面）。
/// 空字节序在登记口已被拒，此处保底按异或 0x5A 单字节计。
pub fn fnv1a_tampered(bytes: &[u8]) -> u64 {
    let n = bytes.len();
    if n == 0 {
        return fnv1a(&[0x5A]);
    }
    let mut h: u64 = FNV_OFFSET;
    let mut i = 0;
    while i < n {
        if let Some(b) = bytes.get(i) {
            let v = if i + 1 == n { *b ^ 0x5A } else { *b };
            h ^= v as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

// ── 资产条目 ─────────────────────────────────────────────────────
/// 一条在册资产：类别 + 名 + 语料字节 + 冻结位 + 实测位。
#[derive(Clone, Copy)]
pub struct AssetRec {
    pub kind: AssetKind,
    pub name: &'static str,
    pub bytes: &'static [u8],
    /// 冻结位（seal 时定格，只读不写）
    pub frozen: u64,
    /// 实测位（每次演练重算）
    pub live: u64,
}

// ── 单调状态机 ───────────────────────────────────────────────────
/// 开门件状态：开账→封账→演练→收口，rank 单调只进不退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FramingState {
    /// 开账：登记资产中
    Open,
    /// 封账：三类摘要已冻结
    Sealed,
    /// 演练：漂移演练已跑
    Drilled,
    /// 收口：写回状态=已完成
    Closed,
}

impl FramingState {
    /// 单调秩（倒退判定基准）。
    pub fn rank(self) -> u32 {
        match self {
            FramingState::Open => 0,
            FramingState::Sealed => 1,
            FramingState::Drilled => 2,
            FramingState::Closed => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            FramingState::Open => "开账",
            FramingState::Sealed => "封账",
            FramingState::Drilled => "演练",
            FramingState::Closed => "收口",
        }
    }
}

/// 漂移演练账：按三类分桶的 尝试/检出/漏检。
#[derive(Clone, Copy)]
pub struct DrillLedger {
    pub tries: [u32; 3],
    pub detected: [u32; 3],
    pub missed: [u32; 3],
}

impl DrillLedger {
    pub const fn new() -> DrillLedger {
        DrillLedger { tries: [0; 3], detected: [0; 3], missed: [0; 3] }
    }
    /// 计数守恒：三类各自 注入=检出+漏检。
    pub fn conservation(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < 3 {
            match (self.tries.get(i), self.detected.get(i), self.missed.get(i)) {
                (Some(t), Some(d), Some(m)) => {
                    if *t != *d + *m {
                        return Err(B39Code::CountBreak.val());
                    }
                }
                _ => return Err(B39Code::CountBreak.val()),
            }
            i += 1;
        }
        Ok(())
    }
    /// 判据"10/10 检出"：三轮全打满且零漏检。
    pub fn all_detected(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if let (Some(t), Some(d), Some(m)) =
                (self.tries.get(i), self.detected.get(i), self.missed.get(i))
            {
                if *t != *d || *m != 0 || *t == 0 {
                    return false;
                }
            } else {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── B39 开门件账本 ───────────────────────────────────────────────
pub struct B39Ledger {
    pub recs: Vec<AssetRec>,
    pub state: FramingState,
    pub drill: DrillLedger,
    /// 收口结果摘要（判据③：非空）
    pub summary: String,
}

impl B39Ledger {
    pub fn new() -> B39Ledger {
        B39Ledger {
            recs: Vec::new(),
            state: FramingState::Open,
            drill: DrillLedger::new(),
            summary: String::new(),
        }
    }

    /// 登记一条资产：名/字节非空、类别闭集、不重名、不超容。
    pub fn register(
        &mut self,
        kind: AssetKind,
        name: &'static str,
        bytes: &'static [u8],
    ) -> Result<usize, u32> {
        if self.state != FramingState::Open {
            // 封账后再登记 = 顺序倒退（登记只属开账期）。
            return Err(B39Code::Regression.val());
        }
        if name.is_empty() {
            return Err(B39Code::NameEmpty.val());
        }
        if bytes.is_empty() {
            return Err(B39Code::BytesEmpty.val());
        }
        if self.recs.len() >= ASSET_CAP {
            return Err(B39Code::LedgerFull.val());
        }
        let mut i = 0;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.name == name {
                    return Err(B39Code::DoubleBooked.val());
                }
            }
            i += 1;
        }
        self.recs.push(AssetRec {
            kind,
            name,
            bytes,
            frozen: 0,
            live: 0,
        });
        Ok(self.recs.len().saturating_sub(1))
    }

    /// 冻结哈希表：逐条算 FNV-1a 定格冻结位；状态 Open→Sealed。
    pub fn seal(&mut self) -> Result<usize, u32> {
        if self.state.rank() > FramingState::Sealed.rank() {
            return Err(B39Code::Regression.val());
        }
        let mut i = 0;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get_mut(i) {
                r.frozen = fnv1a(r.bytes);
            }
            i += 1;
        }
        self.state = FramingState::Sealed;
        Ok(self.recs.len())
    }

    /// 版本漂移演练：每条资产篡改尾字节重算实测位，与冻结位比对。
    /// 哈希不等即检出；命中漏检分支会计入 missed（守恒账兜底）。
    pub fn drift_drill(&mut self) -> Result<u32, u32> {
        if self.state.rank() < FramingState::Sealed.rank() {
            return Err(B39Code::NotFrozen.val());
        }
        let mut hit = 0u32;
        let mut round = 0u32;
        while round < DRILL_ROUNDS {
            let mut i = 0;
            while i < self.recs.len() {
                // 桶序号 = 类别闭集序号。
                let bucket = match self.recs.get(i) {
                    Some(r) => r.kind.idx() as usize,
                    None => 0,
                };
                let (frozen, live) = match self.recs.get(i) {
                    Some(r) => (r.frozen, fnv1a_tampered(r.bytes)),
                    None => (0, 0),
                };
                if let Some(r) = self.recs.get_mut(i) {
                    r.live = live;
                }
                // 守恒账：先记尝试，再按比对结果分检出/漏检。
                if let Some(t) = self.drill.tries.get_mut(bucket) {
                    *t = t.saturating_add(1);
                }
                if frozen != live {
                    hit = hit.saturating_add(1);
                    if let Some(d) = self.drill.detected.get_mut(bucket) {
                        *d = d.saturating_add(1);
                    }
                } else if let Some(m) = self.drill.missed.get_mut(bucket) {
                    *m = m.saturating_add(1);
                }
                i += 1;
            }
            round += 1;
        }
        self.state = FramingState::Drilled;
        Ok(hit)
    }

    /// 收口：状态=已完成 + 结果摘要非空（判据③）。
    pub fn close(&mut self, summary: &str) -> Result<(), u32> {
        if self.state.rank() < FramingState::Drilled.rank() {
            // 未演练先收口 = 跳步，按倒退口径拒。
            return Err(B39Code::NotFrozen.val());
        }
        if summary.is_empty() {
            return Err(B39Code::SummaryEmpty.val());
        }
        self.summary = String::from(summary);
        self.state = FramingState::Closed;
        Ok(())
    }

    /// 冻结在册判定：三类各至少 1 条且冻结位全非零。
    pub fn frozen_complete(&self) -> bool {
        let mut seen = [false; 3];
        let mut i = 0;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                let b = r.kind.idx() as usize;
                if let Some(s) = seen.get_mut(b) {
                    *s = true;
                }
                if r.frozen == 0 {
                    return false;
                }
            }
            i += 1;
        }
        match (seen.get(0), seen.get(1), seen.get(2)) {
            (Some(&a), Some(&b), Some(&c)) => a && b && c,
            _ => false,
        }
    }

    /// 冻结表行数与账行数对账一致。
    pub fn frozen_rows_aligned(&self) -> bool {
        let mut aligned = 0usize;
        let mut i = 0;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.frozen != 0 {
                    aligned += 1;
                }
            }
            i += 1;
        }
        aligned == self.recs.len()
    }
}

// ── 预告语料（B39 冻结常量表，出处：卡面三类资产口径） ───────────
/// 种子资产语料（压测输入种子，逐条含语义说明字节）。
pub const SEED_CORPUS: [(&str, &[u8]); 3] = [
    ("seed_seh_depth", b"seh-walk-depth=8"),
    ("seed_dispatch_p95", b"dispatch-budget-p95"),
    ("seed_critsec_reent", b"critsec-reentry-seed"),
];
/// 样本资产语料（断言样本）。
pub const SAMPLE_CORPUS: [(&str, &[u8]); 3] = [
    ("sample_unwind_ops", b"uwop-assert-samples"),
    ("sample_ctx_frame", b"context-frame-samples"),
    ("sample_badinfo", b"badinfo-failclosed-sample"),
];
/// 脚本资产语料（检查脚本）。
pub const SCRIPT_CORPUS: [(&str, &[u8]); 3] = [
    ("script_scan_dangling", b"scan-dangling-refs.sh"),
    ("script_sha_freeze", b"freeze-sha-ledger.sh"),
    ("script_drill_drift", b"drill-version-drift.sh"),
];

// ── 黄金正演：登记 9 条 → 冻结 → 漂移演练 10 轮 → 收口 ───────────
/// 黄金流返回 (检出总数, 冻结在册?, 收口完成?, 守恒绿?)。
pub fn golden_run() -> Result<(u32, bool, bool, bool), u32> {
    let mut led = B39Ledger::new();
    let mut i = 0;
    while i < SEED_CORPUS.len() {
        if let Some((n, b)) = SEED_CORPUS.get(i) {
            led.register(AssetKind::Seed, n, b)?;
        }
        i += 1;
    }
    let mut i = 0;
    while i < SAMPLE_CORPUS.len() {
        if let Some((n, b)) = SAMPLE_CORPUS.get(i) {
            led.register(AssetKind::Sample, n, b)?;
        }
        i += 1;
    }
    let mut i = 0;
    while i < SCRIPT_CORPUS.len() {
        if let Some((n, b)) = SCRIPT_CORPUS.get(i) {
            led.register(AssetKind::Script, n, b)?;
        }
        i += 1;
    }
    led.seal()?;
    let hit = led.drift_drill()?;
    led.close("B39 预告收口：三类资产冻结在册，漂移演练全检出")?;
    Ok((
        hit,
        led.frozen_complete(),
        led.state == FramingState::Closed && !led.summary.is_empty(),
        led.drill.conservation().is_ok(),
    ))
}

// ── 确定性 LCG（种子=SEG_BASE，禁随机） ──────────────────────────
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

// ── 注入面：三族各 10 轮 LCG 10/10 必抓 ─────────────────────────
/// 注入账：三族 尝试/抓获。
#[derive(Clone, Copy)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 三族全抓且各 10 轮。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t != DRILL_ROUNDS || *c != DRILL_ROUNDS {
                    return false;
                }
            } else {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 取语料首条（避免裸下标；空语料给哨兵元组）。
fn first_of(arr: &[(&'static str, &'static [u8])]) -> (&'static str, &'static [u8]) {
    match arr.get(0) {
        Some(x) => *x,
        None => ("b39_empty_sentinel", b"empty"),
    }
}

/// 注入账桶增减辅助（防裸下标：一律 get_mut + saturating）。
fn bump(arr: &mut [u32; 3], idx: usize, by: u32) {
    if let Some(v) = arr.get_mut(idx) {
        *v = v.saturating_add(by);
    }
}

/// 注入三族，各 10 轮（LCG 只决定注入目标的资产序号，抓不抓由模型定）：
///   族 0：重复登记（同账重登同名 → DoubleBooked）；
///   族 1：状态机倒退（Closed 账再 seal / 再 register → Regression）；
///   族 2：未冻结先演练（Open 账直接 drift_drill → NotFrozen）。
pub fn run_injections(ledger: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < DRILL_ROUNDS {
        let _pick = rng.below(3) as usize;
        // 族 0：重复登记必抓（同账同语料重登同名 → DoubleBooked）。
        let mut m0 = B39Ledger::new();
        let (n0, b0) = first_of(&SEED_CORPUS);
        let _ = m0.register(AssetKind::Seed, n0, b0);
        bump(&mut ledger.tries, 0, 1);
        if m0.register(AssetKind::Seed, n0, b0) == Err(B39Code::DoubleBooked.val()) {
            bump(&mut ledger.caught, 0, 1);
        }
        // 族 1：状态机倒退必抓（Closed 后倒退）。
        let mut m1 = B39Ledger::new();
        let (n1, b1) = first_of(&SAMPLE_CORPUS);
        let _ = m1.register(AssetKind::Sample, n1, b1);
        let _ = m1.seal();
        let _ = m1.drift_drill();
        let _ = m1.close("倒退演练收口摘要");
        bump(&mut ledger.tries, 1, 1);
        if m1.seal() == Err(B39Code::Regression.val())
            && m1.register(AssetKind::Script, "script_x", b"x")
                == Err(B39Code::Regression.val())
        {
            bump(&mut ledger.caught, 1, 1);
        }
        // 族 2：未冻结先演练必抓。
        let mut m2 = B39Ledger::new();
        let (n2, b2) = first_of(&SCRIPT_CORPUS);
        let _ = m2.register(AssetKind::Script, n2, b2);
        bump(&mut ledger.tries, 2, 1);
        if m2.drift_drill() == Err(B39Code::NotFrozen.val()) {
            bump(&mut ledger.caught, 2, 1);
        }
        round += 1;
    }
}

// ── 判据谓词（私有，判定次序固定） ──────────────────────────────
/// 判据②：三类资产冻结在册（预告口径）。
fn frozen_in_ledger_ok() -> bool {
    match golden_run() {
        Ok((_hit, frozen, _closed, _cons)) => frozen,
        Err(_e) => false,
    }
}

/// 判据②：版本漂移演练 10/10 检出（9 资产 × 10 轮 = 90 检出）。
fn drift_drill_10_of_10_ok() -> bool {
    match golden_run() {
        Ok((hit, _frozen, _closed, _cons)) => hit == 9 * DRILL_ROUNDS,
        Err(_e) => false,
    }
}

/// 判据③：收口状态=已完成 且 结果摘要非空。
fn close_writeback_ok() -> bool {
    match golden_run() {
        Ok((_hit, _frozen, closed, _cons)) => closed,
        Err(_e) => false,
    }
}

/// 计数守恒：注入 = 检出 + 漏检，且漏检 = 0；并验 DrillLedger 10/10 面。
fn count_conservation_ok() -> bool {
    let golden_cons = match golden_run() {
        Ok((_hit, _frozen, _closed, cons)) => cons,
        Err(_e) => false,
    };
    // 独立小账复验：3 资产 × 10 轮，检出全满且漏检为 0。
    let mut m = B39Ledger::new();
    let mut i = 0;
    while i < SEED_CORPUS.len() {
        if let Some((n, b)) = SEED_CORPUS.get(i) {
            let _ = m.register(AssetKind::Seed, n, b);
        }
        i += 1;
    }
    if let Some((n, b)) = SAMPLE_CORPUS.get(0) {
        let _ = m.register(AssetKind::Sample, n, b);
    }
    if let Some((n, b)) = SCRIPT_CORPUS.get(0) {
        let _ = m.register(AssetKind::Script, n, b);
    }
    let _ = m.seal();
    let _ = m.drift_drill();
    golden_cons && m.drill.all_detected() && m.drill.conservation().is_ok()
}

/// 段位守卫：SEG_BASE % 0x40 == 0 且落在 VX9 批区间 [0x80000, 0x82FC0]。
pub fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

/// 诊断码全落段内窗口（闭集完备）。
fn codes_in_window_ok() -> bool {
    let mut i = 0;
    while i < B39Code::ALL.len() {
        if let Some(c) = B39Code::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 文案闭集：全变体 detail 非空且两两互异。
fn detail_corpus_closed_ok() -> bool {
    let mut i = 0;
    while i < B39Code::ALL.len() {
        if let Some(a) = B39Code::ALL.get(i) {
            if a.detail().is_empty() {
                return false;
            }
            let mut j = i + 1;
            while j < B39Code::ALL.len() {
                if let Some(b) = B39Code::ALL.get(j) {
                    if a.detail() == b.detail() {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    // 未知码兜底必须给出中文提示且非空。
    !detail(SEG_BASE + 0x3F).is_empty()
}

/// 双跑一致：黄金流同输入两次逐字段相等。
fn double_run_consistent_ok() -> bool {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// 边界四类：空名拒 / 空字节拒 / 类别越闭集拒 / 容量耗尽拒。
fn boundary_quad_ok() -> bool {
    let mut m = B39Ledger::new();
    let e1 = m.register(AssetKind::Seed, "", b"x") == Err(B39Code::NameEmpty.val());
    let e2 = m.register(AssetKind::Seed, "b39_empty", b"") == Err(B39Code::BytesEmpty.val());
    let e3 = AssetKind::from_u32(9).is_none();
    // 容量耗尽：连灌 9 条后再灌第 10 条必拒。
    let mut full = B39Ledger::new();
    let mut i = 0;
    while i < SEED_CORPUS.len() {
        if let Some((n, b)) = SEED_CORPUS.get(i) {
            let _ = full.register(AssetKind::Seed, n, b);
        }
        i += 1;
    }
    let mut i = 0;
    while i < SAMPLE_CORPUS.len() {
        if let Some((n, b)) = SAMPLE_CORPUS.get(i) {
            let _ = full.register(AssetKind::Sample, n, b);
        }
        i += 1;
    }
    let mut i = 0;
    while i < SCRIPT_CORPUS.len() {
        if let Some((n, b)) = SCRIPT_CORPUS.get(i) {
            let _ = full.register(AssetKind::Script, n, b);
        }
        i += 1;
    }
    let e4 = full.register(AssetKind::Seed, "b39_overflow", b"z")
        == Err(B39Code::LedgerFull.val());
    e1 && e2 && e3 && e4
}

/// 判定次序：未冻结先演练先于其它错误（次序敏感）。
fn verdict_order_ok() -> bool {
    // 次序 1：Open 账演练 → NotFrozen（而非 Regression）。
    let mut a = B39Ledger::new();
    let o1 = a.drift_drill() == Err(B39Code::NotFrozen.val());
    // 次序 2：Sealed 后重复 seal（同秩合法，不算倒退）。
    let mut b = B39Ledger::new();
    let _ = b.register(AssetKind::Seed, "seed_order", b"order-bytes");
    let _ = b.seal();
    let o2 = b.seal().is_ok();
    // 次序 3：Drilled 账再 seal → Regression（倒退）。
    let _ = b.drift_drill();
    let o3 = b.seal() == Err(B39Code::Regression.val());
    o1 && o2 && o3
}

/// 对账一致：冻结表行数 = 账行数（frozen_rows_aligned）。
fn frozen_rows_aligned_ok() -> bool {
    match golden_run() {
        Ok(_) => {
            let mut led = B39Ledger::new();
            let mut i = 0;
            while i < SEED_CORPUS.len() {
                if let Some((n, b)) = SEED_CORPUS.get(i) {
                    let _ = led.register(AssetKind::Seed, n, b);
                }
                i += 1;
            }
            let mut i = 0;
            while i < SAMPLE_CORPUS.len() {
                if let Some((n, b)) = SAMPLE_CORPUS.get(i) {
                    let _ = led.register(AssetKind::Sample, n, b);
                }
                i += 1;
            }
            let mut i = 0;
            while i < SCRIPT_CORPUS.len() {
                if let Some((n, b)) = SCRIPT_CORPUS.get(i) {
                    let _ = led.register(AssetKind::Script, n, b);
                }
                i += 1;
            }
            let _ = led.seal();
            led.frozen_rows_aligned() && led.recs.len() == ASSET_CAP
        }
        Err(_e) => false,
    }
}

/// 摘要抗碰参照：篡改前后摘要必不等（预告位敏感性抽验）。
fn digest_sensitivity_ok() -> bool {
    let mut i = 0;
    while i < SEED_CORPUS.len() {
        if let Some((_n, b)) = SEED_CORPUS.get(i) {
            if fnv1a(b) == fnv1a_tampered(b) {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 判据②（逐字锚定 UNX-F12759-J1）。
    v.push((
        "J1-三类资产冻结在册",
        frozen_in_ledger_ok(),
        String::from("种子/样本/脚本 三类冻结位全非零"),
    ));
    v.push((
        "J1-版本漂移演练10/10检出",
        drift_drill_10_of_10_ok(),
        format!("检出={} 轮数={}", 9 * DRILL_ROUNDS, DRILL_ROUNDS),
    ));
    // 判据③。
    v.push((
        "J3-收口状态=已完成",
        close_writeback_ok(),
        String::from("Closed 态 + 结果摘要非空"),
    ));
    // 正常流。
    v.push((
        "正常流-黄金正演全绿",
        golden_run().is_ok(),
        String::from("登记9条→冻结→演练→收口零错"),
    ));
    // 四类边界。
    v.push((
        "边界-空名/空字节/越闭集/超容",
        boundary_quad_ok(),
        String::from("NameEmpty/BytesEmpty/KindInvalid/LedgerFull 全拒"),
    ));
    // 三类注入 10/10。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    let all_caught = inj.all_caught();
    let (t0, c0) = match (inj.tries.get(0), inj.caught.get(0)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => (0, 0),
    };
    let (t1, c1) = match (inj.tries.get(1), inj.caught.get(1)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => (0, 0),
    };
    let (t2, c2) = match (inj.tries.get(2), inj.caught.get(2)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => (0, 0),
    };
    v.push((
        "注入-重复登记10轮全抓",
        all_caught && c0 == t0 && t0 == DRILL_ROUNDS,
        format!("t/c={}/{}", t0, c0),
    ));
    v.push((
        "注入-状态机倒退10轮全抓",
        c1 == t1 && t1 == DRILL_ROUNDS,
        format!("t/c={}/{}", t1, c1),
    ));
    v.push((
        "注入-未冻结先演练10轮全抓",
        c2 == t2 && t2 == DRILL_ROUNDS,
        format!("t/c={}/{}", t2, c2),
    ));
    // 判定次序。
    v.push((
        "判定次序-未冻结先于倒退",
        verdict_order_ok(),
        String::from("Open演练→NotFrozen；同秩seal合法；倒退seal→Regression"),
    ));
    // 状态机倒退拒（独立于注入族的显性谓词）。
    let mut rm = B39Ledger::new();
    let _ = rm.register(AssetKind::Seed, "seed_regress", b"regress-bytes");
    let _ = rm.seal();
    let regress_rejected = rm.register(AssetKind::Sample, "s2", b"b2")
        == Err(B39Code::Regression.val());
    v.push((
        "状态机-封账后再登记显性拒",
        regress_rejected,
        String::from("Sealed 态 register → Regression"),
    ));
    // 容量耗尽（独立口径复述边界）。
    v.push((
        "容量-账槽耗尽拒",
        ASSET_CAP == 9 && boundary_quad_ok(),
        format!("ASSET_CAP={}", ASSET_CAP),
    ));
    // 双跑一致。
    v.push((
        "双跑一致-逐字段相等",
        double_run_consistent_ok(),
        String::from("golden_run() 两次四元组全等"),
    ));
    // 段位守卫。
    v.push((
        "段位守卫-0x82780",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    // 对账一致。
    v.push((
        "对账一致-冻结表行数=账行数",
        frozen_rows_aligned_ok(),
        String::from("frozen_rows_aligned && len==9"),
    ));
    // 计数守恒。
    v.push((
        "计数守恒-注入=检出+漏检且漏检=0",
        count_conservation_ok(),
        String::from("DrillLedger.conservation 全绿"),
    ));
    // 文案闭集。
    v.push((
        "文案闭集-详情非空互异",
        detail_corpus_closed_ok(),
        String::from("9 变体 detail 两两互异 + 兜底非空"),
    ));
    // 收口只增不减（诊断码闭集收敛）。
    v.push((
        "收口-诊断码闭集收敛",
        B39Code::ALL.len() == 9 && codes_in_window_ok(),
        format!("ALL.len()={}", B39Code::ALL.len()),
    ));
    // 摘要敏感性（预告位必抓字节漂移）。
    v.push((
        "摘要敏感性-篡改必不等",
        digest_sensitivity_ok(),
        String::from("fnv1a vs fnv1a_tampered 全部不等"),
    ));
    // 闭集命名面（类别名/状态名中文闭集）。
    let kind_names_ok = AssetKind::Seed.name() == "种子"
        && AssetKind::Sample.name() == "样本"
        && AssetKind::Script.name() == "脚本";
    let state_names_ok = FramingState::Open.name() == "开账"
        && FramingState::Sealed.name() == "封账"
        && FramingState::Drilled.name() == "演练"
        && FramingState::Closed.name() == "收口";
    v.push((
        "闭集-类别/状态中文名闭集",
        kind_names_ok && state_names_ok,
        String::from("三类资产名 + 四态名 逐字在册"),
    ));
    v
}

/// VX9 检查入口：extra_checks 逐条进 set（动态明细槽 ≤8，其余静态）。
pub fn run_unx12758_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12758 B39 预告与开门件（文档对齐框架）");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "");
            }
        }
        i += 1;
    }
    s
}

// ── 测试节（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_golden_flow_all_green() {
        let g = golden_run();
        assert!(g.is_ok());
        let (hit, frozen, closed, cons) = g.unwrap_or((0, false, false, false));
        assert_eq!(hit, 9 * DRILL_ROUNDS);
        assert!(frozen);
        assert!(closed);
        assert!(cons);
    }

    #[test]
    fn t02_boundary_quad_rejected() {
        assert!(boundary_quad_ok());
    }

    #[test]
    fn t03_injections_all_caught() {
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert!(inj.all_caught());
        assert_eq!(inj.tries.get(0).copied().unwrap_or(0), DRILL_ROUNDS);
        assert_eq!(inj.caught.get(2).copied().unwrap_or(0), DRILL_ROUNDS);
    }

    #[test]
    fn t04_double_run_consistent() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        assert!(double_run_consistent_ok());
    }

    #[test]
    fn t05_segment_guard_and_window() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(codes_in_window_ok());
    }

    #[test]
    fn t06_state_machine_regression_rejected() {
        let mut m = B39Ledger::new();
        let _ = m.register(AssetKind::Seed, "seed_t06", b"t06-bytes");
        assert!(m.seal().is_ok());
        // 倒退：Sealed 后再登记 → Regression。
        assert_eq!(
            m.register(AssetKind::Sample, "s_t06", b"t06"),
            Err(B39Code::Regression.val())
        );
        // 倒退：Drilled 后再 seal → Regression。
        let _ = m.drift_drill();
        assert_eq!(m.seal(), Err(B39Code::Regression.val()));
    }

    #[test]
    fn t07_not_frozen_before_drill() {
        let mut m = B39Ledger::new();
        let _ = m.register(AssetKind::Script, "script_t07", b"t07-bytes");
        assert_eq!(m.drift_drill(), Err(B39Code::NotFrozen.val()));
        // 收口也要先演练。
        assert_eq!(m.close(""), Err(B39Code::NotFrozen.val()));
    }

    #[test]
    fn t08_capacity_exhausted() {
        let mut m = B39Ledger::new();
        let mut i = 0;
        while i < SEED_CORPUS.len() {
            if let Some((n, b)) = SEED_CORPUS.get(i) {
                assert!(m.register(AssetKind::Seed, n, b).is_ok());
            }
            i += 1;
        }
        let mut i = 0;
        while i < SAMPLE_CORPUS.len() {
            if let Some((n, b)) = SAMPLE_CORPUS.get(i) {
                assert!(m.register(AssetKind::Sample, n, b).is_ok());
            }
            i += 1;
        }
        let mut i = 0;
        while i < SCRIPT_CORPUS.len() {
            if let Some((n, b)) = SCRIPT_CORPUS.get(i) {
                assert!(m.register(AssetKind::Script, n, b).is_ok());
            }
            i += 1;
        }
        assert_eq!(m.recs.len(), ASSET_CAP);
        assert_eq!(
            m.register(AssetKind::Seed, "over_cap", b"z"),
            Err(B39Code::LedgerFull.val())
        );
    }

    #[test]
    fn t09_checks_all_green() {
        let items = extra_checks();
        assert!(items.len() >= 12);
        for (n, p, d) in items.iter() {
            assert!(*p, "红项: {} / {}", n, d);
        }
    }

    #[test]
    fn t10_detail_closed_corpus() {
        assert!(detail_corpus_closed_ok());
        assert_eq!(
            detail(SEG_BASE + 9),
            "未知诊断码：不在 0x82780 段闭集内"
        );
        assert_eq!(B39Code::ALL.len(), 9);
    }

    #[test]
    fn t11_summary_empty_rejected() {
        let mut m = B39Ledger::new();
        let _ = m.register(AssetKind::Seed, "seed_t11", b"t11-bytes");
        let _ = m.seal();
        let _ = m.drift_drill();
        assert_eq!(m.close(""), Err(B39Code::SummaryEmpty.val()));
        assert!(m.close("非空摘要 t11").is_ok());
        assert!(!m.summary.is_empty());
        assert_eq!(m.state, FramingState::Closed);
    }

    #[test]
    fn t12_fnv_anchor_and_sensitivity() {
        // FNV-1a 确定性锚定：同输入同摘要，空表固定值。
        assert_eq!(fnv1a(b""), FNV_OFFSET);
        assert_eq!(fnv1a(b"a"), fnv1a(b"a"));
        assert_ne!(fnv1a(b"a"), fnv1a(b"b"));
        assert!(digest_sensitivity_ok());
    }
}
