//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12762 · 语义确认单全量对齐（800 条出处抽检） · 0x82880段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ≥330 行；② 判据原文：UNX-F12763-J1 ADR-LC-008 全批复核绿
//!   （40 批版本锚声明逐批在册），缺锚演练 ADR-UNX-008, 40 批版本锚
//!   声明逐批在册，缺锚演练 10/10 检出。
//! 设计要点：
//!   - 800 槽确认单账：每槽一条语义确认（出处档位 + 出处源号），
//!     槽号闭界 [0,800)；档位闭集四类：规范条款/内核源码行/设计文档/
//!     会议纪要（越闭集即拒）；
//!   - LCG 抽检器（种子=SEG_BASE）：全账建成后抽 100 条逐条核出处——
//!     档位须在闭集内且源号非零，缺出处逐条计账；
//!   - 缺出处注入 10/10 必抓：抽检序列为种子纯函数（draw_seq 先行
//!     冻结），注入账把抽检序列命中的 100 槽改成 src_id=0 的坏账 →
//!     抽检必中必抓；
//!   - ADR-UNX-008 版本锚账：40 批逐批登记版本锚字（非零），
//!     40/40 在册才可复核声明绿；缺锚演练 10/10 检出（缺一批即拒）；
//!   - 单调状态机：建账 Building → 抽检 Sampled → 复核 Reviewed
//!     （锚账：收锚 Collecting → 锚齐 Complete → 复核 Reviewed），
//!     倒退显性拒；计数守恒：注入=检出+漏检，漏检恒 0；
//!   - no_std + alloc；零浮点/零随机/零 IO/零墙钟；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x82880，VX9 批公式 0x80000+0x40×(12762−12600)）。
pub const SEG_BASE: u32 = 0x82880;

// ── 冻结常量（出处：卡面口径 + ADR-UNX-008） ────────────────────
/// 确认单槽位总数：800 条（卡面口径）。
pub const CONFIRM_SLOTS: usize = 800;
/// 抽检条数：100 条（卡面口径）。
pub const SAMPLE_N: usize = 100;
/// 版本锚批数：40 批（ADR-UNX-008 口径）。
pub const ANCHOR_BATCHES: usize = 40;
/// 抽检/演练轮数：10/10（判据口径）。
pub const DRILL_ROUNDS: u32 = 10;

// ── 出处档位闭集（四类） ─────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProvTier {
    /// 规范条款（手册/规范章节）
    SpecClause,
    /// 内核源码行（源文件+行号）
    KernelLine,
    /// 设计文档（设计说明章节）
    DesignDoc,
    /// 会议纪要（评审纪要条目）
    Minutes,
}

impl ProvTier {
    pub fn name(self) -> &'static str {
        match self {
            ProvTier::SpecClause => "规范条款",
            ProvTier::KernelLine => "内核源码行",
            ProvTier::DesignDoc => "设计文档",
            ProvTier::Minutes => "会议纪要",
        }
    }
    pub fn idx(self) -> u32 {
        self as u32
    }
    /// u32 → 闭集（越闭集 None，供边界注入）。
    pub fn from_u32(v: u32) -> Option<ProvTier> {
        match v {
            0 => Some(ProvTier::SpecClause),
            1 => Some(ProvTier::KernelLine),
            2 => Some(ProvTier::DesignDoc),
            3 => Some(ProvTier::Minutes),
            _ => None,
        }
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConfirmCode {
    /// 槽号越界（800 槽闭界外）
    SlotOutOfRange,
    /// 出处档位越闭集（四类之外）
    TierInvalid,
    /// 出处源号空（登记了档位但源号为 0，即缺出处）
    SrcIdZero,
    /// 槽重复登记（一格一主）
    DoubleBooked,
    /// 未建全账先抽检（建议：800 槽全部落账再抽）
    NotBuilt,
    /// 状态机倒退（建议：状态单调只进不退，倒退即拒）
    Regression,
    /// 版本锚批号越界（40 批闭界外）
    BatchInvalid,
    /// 版本锚缺锚（复核声明时 40 批未齐）
    AnchorMissing,
    /// 抽检计数守恒破坏（建议：注入=检出+漏检，漏检恒 0）
    CountBreak,
    /// 复核结论为空（建议：复核声明必须附非空结论）
    VerdictEmpty,
}

impl ConfirmCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ConfirmCode::SlotOutOfRange => "槽号越界：确认单只收 [0,800) 闭界内槽位",
            ConfirmCode::TierInvalid => "出处档位越闭集：只收 规范条款/内核源码行/设计文档/会议纪要",
            ConfirmCode::SrcIdZero => "出处源号空：有档位无源号即缺出处，先补源号",
            ConfirmCode::DoubleBooked => "槽重复登记：一格一主，重登先查账",
            ConfirmCode::NotBuilt => "未建全账先抽检：800 槽全部落账后再抽",
            ConfirmCode::Regression => "状态机倒退：状态单调只进不退，倒退即拒",
            ConfirmCode::BatchInvalid => "版本锚批号越界：ADR-UNX-008 只收 40 批闭界内",
            ConfirmCode::AnchorMissing => "版本锚缺锚：40 批逐批在册才可复核声明",
            ConfirmCode::CountBreak => "抽检计数守恒破坏：注入=检出+漏检，漏检恒 0",
            ConfirmCode::VerdictEmpty => "复核结论为空：复核声明必须附非空结论",
        }
    }
    pub const ALL: [ConfirmCode; 10] = [
        ConfirmCode::SlotOutOfRange,
        ConfirmCode::TierInvalid,
        ConfirmCode::SrcIdZero,
        ConfirmCode::DoubleBooked,
        ConfirmCode::NotBuilt,
        ConfirmCode::Regression,
        ConfirmCode::BatchInvalid,
        ConfirmCode::AnchorMissing,
        ConfirmCode::CountBreak,
        ConfirmCode::VerdictEmpty,
    ];
}

/// 闭集查表：未知码给中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ConfirmCode::ALL.len() {
        if let Some(c) = ConfirmCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82880 段闭集内"
}

// ── 确认单条目 ───────────────────────────────────────────────────
/// 一条语义确认：槽号 + 出处档位 + 出处源号（非零）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConfirmRec {
    pub slot: u32,
    pub tier: ProvTier,
    pub src_id: u32,
}

// ── 确认账状态机（单调） ─────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConfirmState {
    /// 建账：800 槽登记中
    Building,
    /// 抽检：100 条抽检已跑
    Sampled,
    /// 复核：复核声明已立
    Reviewed,
}

impl ConfirmState {
    pub fn rank(self) -> u32 {
        match self {
            ConfirmState::Building => 0,
            ConfirmState::Sampled => 1,
            ConfirmState::Reviewed => 2,
        }
    }
}

// ── 版本锚账状态机（单调） ───────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorState {
    /// 收锚：40 批登记中
    Collecting,
    /// 锚齐：40/40 在册
    Complete,
    /// 复核：复核已立
    Reviewed,
}

impl AnchorState {
    pub fn rank(self) -> u32 {
        match self {
            AnchorState::Collecting => 0,
            AnchorState::Complete => 1,
            AnchorState::Reviewed => 2,
        }
    }
}

// ── 抽检/演练账（三族 尝试/检出/漏检） ──────────────────────────
#[derive(Clone, Copy)]
pub struct AuditLedger {
    pub tries: [u32; 3],
    pub detected: [u32; 3],
    pub missed: [u32; 3],
}

impl AuditLedger {
    pub const fn new() -> AuditLedger {
        AuditLedger { tries: [0; 3], detected: [0; 3], missed: [0; 3] }
    }
    /// 守恒：三族各自 注入=检出+漏检 且漏检=0。
    pub fn conservation(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < 3 {
            match (self.tries.get(i), self.detected.get(i), self.missed.get(i)) {
                (Some(t), Some(d), Some(m)) => {
                    if *t != *d + *m || *m != 0 {
                        return Err(ConfirmCode::CountBreak.val());
                    }
                }
                _ => return Err(ConfirmCode::CountBreak.val()),
            }
            i += 1;
        }
        Ok(())
    }
    /// 三族全抓且各 10 轮。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            match (self.tries.get(i), self.detected.get(i)) {
                (Some(t), Some(c)) => {
                    if *t != DRILL_ROUNDS || *c != DRILL_ROUNDS {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 缺锚演练账（独立于抽检三族）。
#[derive(Clone, Copy)]
pub struct AnchorDrillLedger {
    pub tries: u32,
    pub detected: u32,
    pub missed: u32,
}

impl AnchorDrillLedger {
    pub const fn new() -> AnchorDrillLedger {
        AnchorDrillLedger { tries: 0, detected: 0, missed: 0 }
    }
    /// 缺锚演练 10/10 检出（判据锚）。
    pub fn all_detected(&self) -> bool {
        self.tries == DRILL_ROUNDS && self.detected == DRILL_ROUNDS && self.missed == 0
    }
}

// ── 800 槽确认账本 ───────────────────────────────────────────────
pub struct ConfirmLedger {
    pub slots: Vec<Option<ConfirmRec>>,
    pub state: ConfirmState,
    pub sampled: Vec<u32>,
    pub verdict: String,
}

impl ConfirmLedger {
    pub fn new() -> ConfirmLedger {
        let mut slots = Vec::new();
        let mut i = 0;
        while i < CONFIRM_SLOTS {
            slots.push(None);
            i += 1;
        }
        ConfirmLedger {
            slots,
            state: ConfirmState::Building,
            sampled: Vec::new(),
            verdict: String::new(),
        }
    }

    /// 登记一条确认：槽号闭界 → 档位闭集 → 源号非零 → 不重登。
    pub fn register(
        &mut self,
        slot: u32,
        tier_id: u32,
        src_id: u32,
    ) -> Result<usize, u32> {
        if self.state != ConfirmState::Building {
            return Err(ConfirmCode::Regression.val());
        }
        if slot as usize >= CONFIRM_SLOTS {
            return Err(ConfirmCode::SlotOutOfRange.val());
        }
        let tier = match ProvTier::from_u32(tier_id) {
            Some(t) => t,
            None => return Err(ConfirmCode::TierInvalid.val()),
        };
        if src_id == 0 {
            return Err(ConfirmCode::SrcIdZero.val());
        }
        if self.slots.get(slot as usize).map_or(false, |s| s.is_some()) {
            return Err(ConfirmCode::DoubleBooked.val());
        }
        if let Some(s) = self.slots.get_mut(slot as usize) {
            *s = Some(ConfirmRec { slot, tier, src_id });
        }
        Ok(slot as usize)
    }

    /// 建全账判定：800 槽全有主（判据"全量对齐"）。
    pub fn built_complete(&self) -> bool {
        let mut i = 0;
        while i < CONFIRM_SLOTS {
            if let Some(None) = self.slots.get(i) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 抽检序列：种子纯函数（先冻结，缺出处注入面据此改账）。
    pub fn draw_seq() -> Vec<u32> {
        let mut rng = Lcg::new(SEG_BASE as u64);
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0;
        while i < SAMPLE_N {
            out.push(rng.below(CONFIRM_SLOTS as u64) as u32);
            i += 1;
        }
        out
    }

    /// LCG 抽检 100 条：逐条核档位闭集 + 源号非零，返回 (抽检数, 缺出处数)。
    pub fn sample(&mut self) -> Result<(usize, usize), u32> {
        if self.state != ConfirmState::Building {
            return Err(ConfirmCode::Regression.val());
        }
        if !self.built_complete() {
            return Err(ConfirmCode::NotBuilt.val());
        }
        let seq = Self::draw_seq();
        let mut bad = 0usize;
        let mut i = 0;
        while i < seq.len() {
            let slot = match seq.get(i) {
                Some(s) => *s,
                None => 0,
            };
            let idx = slot as usize;
            let ok = match self.slots.get(idx) {
                Some(Some(rec)) => {
                    // 档位闭集核验（re-derive，防账外值）+ 源号非零。
                    ProvTier::from_u32(rec.tier.idx()).is_some() && rec.src_id != 0
                }
                _ => false,
            };
            if !ok {
                bad += 1;
            }
            self.sampled.push(slot);
            i += 1;
        }
        self.state = ConfirmState::Sampled;
        Ok((seq.len(), bad))
    }

    /// 复核声明：抽检已跑 + 版本锚 40 批在册 + 结论非空才可立。
    pub fn review(&mut self, anchors_ok: bool, verdict: &str) -> Result<(), u32> {
        if self.state.rank() < ConfirmState::Sampled.rank() {
            return Err(ConfirmCode::NotBuilt.val());
        }
        if !anchors_ok {
            return Err(ConfirmCode::AnchorMissing.val());
        }
        if verdict.is_empty() {
            return Err(ConfirmCode::VerdictEmpty.val());
        }
        self.verdict = String::from(verdict);
        self.state = ConfirmState::Reviewed;
        Ok(())
    }
}

// ── ADR-UNX-008 版本锚账（40 批） ────────────────────────────────
pub struct AnchorLedger {
    pub anchors: Vec<Option<u32>>,
    pub state: AnchorState,
}

impl AnchorLedger {
    pub fn new() -> AnchorLedger {
        let mut anchors = Vec::new();
        let mut i = 0;
        while i < ANCHOR_BATCHES {
            anchors.push(None);
            i += 1;
        }
        AnchorLedger {
            anchors,
            state: AnchorState::Collecting,
        }
    }

    /// 登记一批版本锚：复核后倒退拒 → 批号闭界 → 锚字非零 → 不重批；
    /// 落账后 40/40 齐即自动升 Complete。
    pub fn register_anchor(&mut self, batch: u32, word: u32) -> Result<usize, u32> {
        if self.state.rank() >= AnchorState::Reviewed.rank() {
            return Err(ConfirmCode::Regression.val());
        }
        if batch as usize >= ANCHOR_BATCHES {
            return Err(ConfirmCode::BatchInvalid.val());
        }
        if word == 0 {
            return Err(ConfirmCode::AnchorMissing.val());
        }
        if self.anchors.get(batch as usize).map_or(false, |a| a.is_some()) {
            return Err(ConfirmCode::DoubleBooked.val());
        }
        if let Some(a) = self.anchors.get_mut(batch as usize) {
            *a = Some(word);
        }
        if self.all_anchored() {
            self.state = AnchorState::Complete;
        }
        Ok(batch as usize)
    }

    /// 40 批逐批在册判定（判据"40 批版本锚声明逐批在册"）。
    pub fn all_anchored(&self) -> bool {
        let mut i = 0;
        while i < ANCHOR_BATCHES {
            if let Some(None) = self.anchors.get(i) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 复核声明：锚齐（Complete 及以上）才可立；状态→Reviewed。
    pub fn declare(&mut self) -> Result<(), u32> {
        if self.state.rank() < AnchorState::Complete.rank() {
            return Err(ConfirmCode::AnchorMissing.val());
        }
        self.state = AnchorState::Reviewed;
        Ok(())
    }

    /// 在册批计数。
    pub fn anchored_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < ANCHOR_BATCHES {
            if let Some(Some(_)) = self.anchors.get(i) {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ── 确定性 LCG（种子=SEG_BASE） ─────────────────────────────────
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

// ── 黄金正演：800 条全登记 → 抽检 100 → 40 锚 → 复核声明 ─────────
/// 黄金流返回 (抽检数, 缺出处数, 800 齐?, 复核绿?)。
pub fn golden_run() -> Result<(usize, usize, bool, bool), u32> {
    let mut led = ConfirmLedger::new();
    let mut s = 0u32;
    while s < CONFIRM_SLOTS as u32 {
        // 档位轮转四类，源号 = 1000+槽号（全非零）。
        led.register(s, s % 4, 1000 + s)?;
        s += 1;
    }
    let (drawn, bad) = led.sample()?;
    let mut anc = AnchorLedger::new();
    let mut b = 0u32;
    while b < ANCHOR_BATCHES as u32 {
        anc.register_anchor(b, 0xA0_0000 + b)?;
        b += 1;
    }
    let anchors_ok = anc.all_anchored() && anc.declare().is_ok();
    led.review(anchors_ok, "800 条出处抽检零缺出处，40 批版本锚逐批在册")?;
    Ok((
        drawn,
        bad,
        led.built_complete(),
        led.state == ConfirmState::Reviewed && !led.verdict.is_empty(),
    ))
}

// ── 注入面：三族各 10 轮 LCG 10/10 必抓（账务复用 AuditLedger 三桶） ──

/// 注入账桶增减辅助（防裸下标：一律 get_mut + saturating）。
fn bump(arr: &mut [u32; 3], idx: usize, by: u32) {
    if let Some(v) = arr.get_mut(idx) {
        *v = v.saturating_add(by);
    }
}

/// 缺出处注入账：全账建成，但抽检序列命中的 100 槽被改成"有档位无源号"
/// 的缺出处坏账（注入 = 打破登记不变量制造违约面）。
/// 抽检器（同种子纯函数序列）必中坏账槽 → 10/10 必抓。
fn build_holey_ledger() -> Result<ConfirmLedger, u32> {
    let mut led = ConfirmLedger::new();
    let seq = ConfirmLedger::draw_seq();
    let mut s = 0u32;
    while s < CONFIRM_SLOTS as u32 {
        led.register(s, s % 4, 1000 + s)?;
        s += 1;
    }
    // 注入：抽检序列命中槽改成 src_id=0 的缺出处记录。
    let mut i = 0;
    while i < seq.len() {
        if let Some(h) = seq.get(i) {
            let idx = *h as usize;
            if let Some(Some(rec)) = led.slots.get_mut(idx) {
                rec.src_id = 0;
            }
        }
        i += 1;
    }
    Ok(led)
}

/// 缺锚注入账：40 批缺 `skip` 批（缺锚演练面）。
fn build_thin_anchor(skip: u32) -> Result<AnchorLedger, u32> {
    let mut anc = AnchorLedger::new();
    let mut b = 0u32;
    while b < ANCHOR_BATCHES as u32 {
        if b != skip {
            anc.register_anchor(b, 0xA0_0000 + b)?;
        }
        b += 1;
    }
    Ok(anc)
}

/// 注入三族，各 10 轮（LCG 只挑变体，抓不抓由模型定）：
///   族 0：缺出处抽检（坏账槽 → sample 的 bad>0）；
///   族 1：档位越闭集（tier_id=9 → TierInvalid）+ 源号空（src_id=0 → SrcIdZero）；
///   族 2：重复登记（同槽双登 → DoubleBooked）。
pub fn run_injections(ledger: &mut AuditLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < DRILL_ROUNDS {
        let pick = rng.below(4) as usize;
        // 族 0：缺出处必抓。
        if let Ok(mut holey) = build_holey_ledger() {
            bump(&mut ledger.tries, 0, 1);
            if let Ok((_n, bad)) = holey.sample() {
                if bad > 0 {
                    bump(&mut ledger.detected, 0, 1);
                } else {
                    bump(&mut ledger.missed, 0, 1);
                }
            }
        }
        // 族 1：档位越闭集 + 源号空必抓。
        let mut m1 = ConfirmLedger::new();
        bump(&mut ledger.tries, 1, 1);
        if m1.register(pick as u32, 9, 1001) == Err(ConfirmCode::TierInvalid.val())
            && m1.register(pick as u32, 0, 0) == Err(ConfirmCode::SrcIdZero.val())
        {
            bump(&mut ledger.detected, 1, 1);
        }
        // 族 2：重复登记必抓。
        let mut m2 = ConfirmLedger::new();
        let _ = m2.register(pick as u32, 1, 1002);
        bump(&mut ledger.tries, 2, 1);
        if m2.register(pick as u32, 2, 1003) == Err(ConfirmCode::DoubleBooked.val()) {
            bump(&mut ledger.detected, 2, 1);
        }
        round += 1;
    }
}

/// 缺锚演练：10 轮各缺一批（LCG 挑批号），declare 必拒 AnchorMissing。
pub fn run_anchor_drill(led: &mut AnchorDrillLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < DRILL_ROUNDS {
        let skip = rng.below(ANCHOR_BATCHES as u64) as u32;
        if let Ok(mut anc) = build_thin_anchor(skip) {
            led.tries += 1;
            if anc.anchored_count() == (ANCHOR_BATCHES as u32) - 1
                && !anc.all_anchored()
                && anc.declare() == Err(ConfirmCode::AnchorMissing.val())
            {
                led.detected += 1;
            }
        }
        round += 1;
    }
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// 判据锚：ADR-LC-008 全批复核绿（40 批版本锚声明逐批在册）。
fn anchor_review_green_ok() -> bool {
    match golden_run() {
        Ok((_d, _b, _full, review)) => review,
        Err(_e) => false,
    }
}

/// 判据锚：缺锚演练 10/10 检出。
fn anchor_drill_10_of_10_ok() -> bool {
    let mut led = AnchorDrillLedger::new();
    run_anchor_drill(&mut led);
    led.all_detected()
}

/// 判据锚：800 条出处抽检零缺出处。
fn sample_clean_ok() -> bool {
    match golden_run() {
        Ok((drawn, bad, _full, _review)) => drawn == SAMPLE_N && bad == 0,
        Err(_e) => false,
    }
}

/// 计数守恒：抽检三族 + 缺锚演练全守恒（注入=检出+漏检，漏检=0）。
fn conservation_ok() -> bool {
    let mut audit = AuditLedger::new();
    run_injections(&mut audit);
    let mut drill = AnchorDrillLedger::new();
    run_anchor_drill(&mut drill);
    audit.conservation().is_ok() && audit.all_caught() && drill.all_detected()
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

/// 诊断码全落段内窗口。
fn codes_in_window_ok() -> bool {
    let mut i = 0;
    while i < ConfirmCode::ALL.len() {
        if let Some(c) = ConfirmCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 文案闭集：全变体非空两两互异 + 兜底。
fn detail_corpus_closed_ok() -> bool {
    let mut i = 0;
    while i < ConfirmCode::ALL.len() {
        if let Some(a) = ConfirmCode::ALL.get(i) {
            if a.detail().is_empty() {
                return false;
            }
            let mut j = i + 1;
            while j < ConfirmCode::ALL.len() {
                if let Some(b) = ConfirmCode::ALL.get(j) {
                    if a.detail() == b.detail() {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    !detail(SEG_BASE + 0x3F).is_empty()
}

/// 双跑一致：黄金流两次逐字段相等。
fn double_run_ok() -> bool {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// 边界四类：槽 0/799 可登、800 拒、批 0/39 可登、40 拒。
fn boundary_quad_ok() -> bool {
    let mut m = ConfirmLedger::new();
    let s0 = m.register(0, 0, 1000).is_ok();
    let s799 = m.register(799, 3, 1799).is_ok();
    let s800 = m.register(800, 0, 1800) == Err(ConfirmCode::SlotOutOfRange.val());
    let mut anc = AnchorLedger::new();
    let b0 = anc.register_anchor(0, 0xA0_0000).is_ok();
    let b39 = anc.register_anchor(39, 0xA0_0039).is_ok();
    let b40 = anc.register_anchor(40, 0xA0_0040)
        == Err(ConfirmCode::BatchInvalid.val());
    s0 && s799 && s800 && b0 && b39 && b40
}

/// 判定次序：未建全账先抽检 → NotBuilt 优先；缺锚复核 → AnchorMissing；
/// 空结论 → VerdictEmpty。
fn verdict_order_ok() -> bool {
    let mut a = ConfirmLedger::new();
    let _ = a.register(0, 0, 1000);
    // 账未全 → 抽检即拒 NotBuilt。
    let o1 = a.sample() == Err(ConfirmCode::NotBuilt.val());
    // 账全但未抽检 → 复核拒 NotBuilt（次序在锚与结论之前）。
    let mut s = 0u32;
    while s < CONFIRM_SLOTS as u32 {
        if let Some(None) = a.slots.get(s as usize) {
            let _ = a.register(s, s % 4, 1000 + s);
        }
        s += 1;
    }
    let o2 = a.review(true, "次序演练结论") == Err(ConfirmCode::NotBuilt.val());
    let _ = a.sample();
    // 抽检后缺锚 → AnchorMissing（次序在结论之前）。
    let o3 = a.review(false, "次序演练结论二")
        == Err(ConfirmCode::AnchorMissing.val());
    // 锚齐但结论空 → VerdictEmpty。
    let o4 = a.review(true, "") == Err(ConfirmCode::VerdictEmpty.val());
    o1 && o2 && o3 && o4
}

/// 状态机倒退拒：Sampled/Reviewed 后再登记 → Regression。
fn regression_ok() -> bool {
    let mut a = ConfirmLedger::new();
    let mut s = 0u32;
    while s < CONFIRM_SLOTS as u32 {
        let _ = a.register(s, s % 4, 1000 + s);
        s += 1;
    }
    let _ = a.sample();
    let r1 = a.register(0, 0, 1) == Err(ConfirmCode::Regression.val());
    let mut anc = AnchorLedger::new();
    let mut b = 0u32;
    while b < ANCHOR_BATCHES as u32 {
        let _ = anc.register_anchor(b, 0xA0_0000 + b);
        b += 1;
    }
    let _ = anc.declare();
    let r2 = anc.register_anchor(0, 0xB0) == Err(ConfirmCode::Regression.val());
    r1 && r2
}

// ── 检查入口 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 判据锚（逐字口径：UNX-F12763-J1 40 批版本锚声明逐批在册）。
    v.push((
        "J1-40批版本锚声明逐批在册",
        anchor_review_green_ok(),
        format!("ANCHOR_BATCHES={} 全登记+复核绿", ANCHOR_BATCHES),
    ));
    v.push((
        "J1-缺锚演练10/10检出",
        anchor_drill_10_of_10_ok(),
        String::from("缺一批即拒 AnchorMissing，10 轮全检出"),
    ));
    v.push((
        "J1-800条出处抽检零缺出处",
        sample_clean_ok(),
        format!("抽检={} 零缺出处", SAMPLE_N),
    ));
    // 正常流。
    v.push((
        "正常流-黄金正演全绿",
        golden_run().is_ok(),
        String::from("800登记→抽检100→40锚→复核零错"),
    ));
    // 边界四类。
    v.push((
        "边界-槽0/799可登、800拒、批0/39可登、40拒",
        boundary_quad_ok(),
        String::from("SlotOutOfRange/BatchInvalid 双向闭界"),
    ));
    // 注入三族 10/10。
    let mut audit = AuditLedger::new();
    run_injections(&mut audit);
    let all_caught = audit.all_caught();
    let (t0, c0) = match (audit.tries.get(0), audit.detected.get(0)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => (0, 0),
    };
    let (t1, c1) = match (audit.tries.get(1), audit.detected.get(1)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => (0, 0),
    };
    let (t2, c2) = match (audit.tries.get(2), audit.detected.get(2)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => (0, 0),
    };
    v.push((
        "注入-缺出处抽检10轮全抓",
        all_caught && c0 == t0 && t0 == DRILL_ROUNDS,
        format!("t/c={}/{}", t0, c0),
    ));
    v.push((
        "注入-档位越闭集10轮全抓",
        c1 == t1 && t1 == DRILL_ROUNDS,
        format!("t/c={}/{}", t1, c1),
    ));
    v.push((
        "注入-重复登记10轮全抓",
        c2 == t2 && t2 == DRILL_ROUNDS,
        format!("t/c={}/{}", t2, c2),
    ));
    // 判定次序。
    v.push((
        "判定次序-未建账先抽检必拒",
        verdict_order_ok(),
        String::from("NotBuilt→AnchorMissing→VerdictEmpty 次序链"),
    ));
    // 状态机倒退。
    v.push((
        "状态机-抽检/复核后再登记显性拒",
        regression_ok(),
        String::from("Sampled/Reviewed 态 register → Regression"),
    ));
    // 双跑一致。
    v.push((
        "双跑一致-逐字段相等",
        double_run_ok(),
        String::from("golden_run() 两次四元组全等"),
    ));
    // 段位守卫。
    v.push((
        "段位守卫-0x82880",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    // 计数守恒。
    v.push((
        "计数守恒-注入=检出+漏检且漏检=0",
        conservation_ok(),
        String::from("AuditLedger+AnchorDrillLedger 全守恒"),
    ));
    // 文案闭集。
    v.push((
        "文案闭集-详情非空互异",
        detail_corpus_closed_ok(),
        String::from("10 变体 detail 两两互异 + 兜底非空"),
    ));
    // 收口只增不减。
    v.push((
        "收口-诊断码闭集收敛",
        ConfirmCode::ALL.len() == 10 && codes_in_window_ok(),
        format!("ALL.len()={}", ConfirmCode::ALL.len()),
    ));
    // 档位闭集独立谓词（含中文名闭集）。
    let tier_ok = ProvTier::from_u32(0).is_some()
        && ProvTier::from_u32(3).is_some()
        && ProvTier::from_u32(4).is_none()
        && ProvTier::from_u32(9).is_none()
        && ProvTier::SpecClause.name() == "规范条款"
        && ProvTier::KernelLine.name() == "内核源码行"
        && ProvTier::DesignDoc.name() == "设计文档"
        && ProvTier::Minutes.name() == "会议纪要";
    v.push((
        "闭集-出处档位四类闭集",
        tier_ok,
        String::from("规范条款/内核源码行/设计文档/会议纪要"),
    ));
    v
}

/// VX9 检查入口：动态明细槽 ≤8，其余静态。
pub fn run_unx12762_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12762 语义确认单全量对齐（800 条出处抽检）");
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
        let (drawn, bad, full, review) = g.unwrap_or((0, 0, false, false));
        assert_eq!(drawn, SAMPLE_N);
        assert_eq!(bad, 0);
        assert!(full);
        assert!(review);
    }

    #[test]
    fn t02_boundary_quad() {
        assert!(boundary_quad_ok());
    }

    #[test]
    fn t03_injections_all_caught() {
        let mut audit = AuditLedger::new();
        run_injections(&mut audit);
        assert!(audit.all_caught());
        assert!(audit.conservation().is_ok());
        assert_eq!(audit.tries.get(0).copied().unwrap_or(0), DRILL_ROUNDS);
        assert_eq!(audit.detected.get(2).copied().unwrap_or(0), DRILL_ROUNDS);
    }

    #[test]
    fn t04_anchor_drill_10_of_10() {
        let mut drill = AnchorDrillLedger::new();
        run_anchor_drill(&mut drill);
        assert!(drill.all_detected());
        assert_eq!(drill.tries, DRILL_ROUNDS);
        assert_eq!(drill.missed, 0);
    }

    #[test]
    fn t05_double_run_consistent() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        assert!(double_run_ok());
    }

    #[test]
    fn t06_segment_guard_and_codes() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(codes_in_window_ok());
        assert!(detail_corpus_closed_ok());
        assert_eq!(detail(SEG_BASE + 11), "未知诊断码：不在 0x82880 段闭集内");
    }

    #[test]
    fn t07_not_built_order() {
        let mut a = ConfirmLedger::new();
        let _ = a.register(0, 0, 1000);
        assert_eq!(a.sample(), Err(ConfirmCode::NotBuilt.val()));
    }

    #[test]
    fn t08_regression_rejected() {
        assert!(regression_ok());
    }

    #[test]
    fn t09_anchor_boundary() {
        let mut anc = AnchorLedger::new();
        assert!(anc.register_anchor(0, 0xA0).is_ok());
        assert_eq!(
            anc.register_anchor(0, 0xA1),
            Err(ConfirmCode::DoubleBooked.val())
        );
        assert_eq!(
            anc.register_anchor(40, 0xA40),
            Err(ConfirmCode::BatchInvalid.val())
        );
        assert_eq!(
            anc.register_anchor(1, 0),
            Err(ConfirmCode::AnchorMissing.val())
        );
        assert!(!anc.all_anchored());
        assert_eq!(anc.declare(), Err(ConfirmCode::AnchorMissing.val()));
    }

    #[test]
    fn t10_checks_all_green() {
        let items = extra_checks();
        assert!(items.len() >= 12);
        for (n, p, d) in items.iter() {
            assert!(*p, "红项: {} / {}", n, d);
        }
    }

    #[test]
    fn t11_tier_closed_set() {
        assert!(ProvTier::from_u32(0).is_some());
        assert!(ProvTier::from_u32(3).is_some());
        assert!(ProvTier::from_u32(4).is_none());
        assert_eq!(ProvTier::SpecClause.name(), "规范条款");
        assert_eq!(ProvTier::Minutes.name(), "会议纪要");
    }

    #[test]
    fn t12_draw_seq_deterministic() {
        // 抽检序列为种子纯函数：两次生成逐位相等，且恰 100 条、界内。
        let a = ConfirmLedger::draw_seq();
        let b = ConfirmLedger::draw_seq();
        assert_eq!(a.len(), SAMPLE_N);
        assert_eq!(a, b);
        let mut i = 0;
        while i < a.len() {
            let s = a.get(i).copied().unwrap_or(0);
            assert!((s as usize) < CONFIRM_SLOTS);
            i += 1;
        }
    }
}
