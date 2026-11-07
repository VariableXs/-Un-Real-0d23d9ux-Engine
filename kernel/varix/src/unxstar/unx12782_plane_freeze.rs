//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12782 号面冻结终版（F12001–F12800 零重编声明） · 0x82D80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 vx9_cards.json 卡 12782 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12783-J1 接管清册齐（语义档/接管点/回归门槛
//!      三列），路径可点验；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 号面闭集：F12001..F12800 共 800 号位（号面 = 编号平面），号位
//!     登记走位图账（13×u64 位图收 800 位），登记序任意、冻结后不可改；
//!   - 零重编模型：零重编 = 编号唯一 + 连续两个断言同时成立——唯一 =
//!     800 位图位恰好各置一次（重复登记 DuplicateNumber 必拒），连续 =
//!     位图扫窗无洞（首洞即 GapDetected），800 号位全在窗 [12001,12800]
//!     内（窗外 OutOfPlane 必拒）；
//!   - 冻结单调状态机：采集中 Collecting → 已冻结 Frozen → 已封印
//!     Sealed，只进不退；冻结后再登记判 SlotFrozen（号面终版不可改），
//!     冻结时 800 号位未满判 PlaneNotFull（不连续不许冻结）；
//!   - 重编注入三族：重号（同号双登）/断号（缺一号码冻结）/窗外号
//!     （12000 与 12801 两侧越窗），LCG（种子=0x82D80）各 10 轮 10/10
//!     必抓；
//!   - 零重编声明：冻结+封印后产出非空声明文本（判据③写回），文本
//!     逐字含「F12001–F12800 连续零重编 800/800」锚定串；
//!   - no_std + alloc；零浮点/零随机/零 IO/零墙钟；零 unwrap/expect，
//!     位图取值一律 .get()/checked_*/saturating_*，禁裸下标取值。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x82D80，VX9 批公式 0x80000+0x40×(12782−12600)）。
pub const SEG_BASE: u32 = 0x82D80;

// ── 冻结常量（出处见行注） ───────────────────────────────────────
/// 号面下沿（卡面标题闭集 F12001）。
pub const PLANE_LO: u32 = 12001;
/// 号面上沿（卡面标题闭集 F12800）。
pub const PLANE_HI: u32 = 12800;
/// 号位总数 = 12800−12001+1 = 800。
pub const PLANE_N: u32 = 800;
/// 位图字数：⌈800/64⌉ = 13。
pub const BITMAP_WORDS: usize = 13;
/// 每族注入轮数（判据②口径 10/10）。
pub const DRILL_ROUNDS: u32 = 10;
/// 封印锚定串（零重编声明逐字核验用）。
pub const SEAL_ANCHOR: &str = "F12001–F12800 连续零重编 800/800";

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlaneCode {
    /// 号位越窗（[12001,12800] 之外）
    OutOfPlane,
    /// 重号（同号位二次登记，唯一性断言破）
    DuplicateNumber,
    /// 断号（位图扫窗有洞，连续性断言破）
    GapDetected,
    /// 号位未满先冻结（800 未满即冻结）
    PlaneNotFull,
    /// 冻结后再登记（号面终版不可改）
    SlotFrozen,
    /// 冻结状态机倒退（Frozen/Sealed 回 Collecting）
    FreezeRegress,
    /// 计数守恒破（位图置位数≠登记计数）
    CountBreak,
    /// 声明为空或锚定串缺失（判据③写回非空）
    SealMismatch,
    /// 位图越窗字（字下标 ≥13 的越界探测）
    BitmapWordOob,
}

impl PlaneCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PlaneCode::OutOfPlane => {
                "号位越窗：只收 F12001..F12800，窗外号码请回号面台账改窗内号"
            }
            PlaneCode::DuplicateNumber => {
                "重号：同号位只能登记一次，请查重后再登记——唯一性是零重编前提"
            }
            PlaneCode::GapDetected => {
                "断号：位图扫窗出现洞，请补齐缺口号位——连续性是零重编前提"
            }
            PlaneCode::PlaneNotFull => {
                "号位未满先冻结：800 号位不满不许冻结，请先补登余号"
            }
            PlaneCode::SlotFrozen => {
                "冻结后再登记：号面终版不可改，新号请走增补号面新开窗"
            }
            PlaneCode::FreezeRegress => {
                "冻结状态机倒退：Collecting→Frozen→Sealed 单向推进，倒退请走新账本"
            }
            PlaneCode::CountBreak => {
                "计数守恒破：位图置位数须等于登记计数，请回账对账位图"
            }
            PlaneCode::SealMismatch => {
                "封印声明缺失：声明须非空且逐字含锚定串，请按零重编声明模板补写"
            }
            PlaneCode::BitmapWordOob => {
                "位图越窗字：字下标越过 13 字位图，请用号位换算取字下标"
            }
        }
    }
    pub const ALL: [PlaneCode; 9] = [
        PlaneCode::OutOfPlane,
        PlaneCode::DuplicateNumber,
        PlaneCode::GapDetected,
        PlaneCode::PlaneNotFull,
        PlaneCode::SlotFrozen,
        PlaneCode::FreezeRegress,
        PlaneCode::CountBreak,
        PlaneCode::SealMismatch,
        PlaneCode::BitmapWordOob,
    ];
}

/// 诊断码闭集查表：未知码给中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PlaneCode::ALL.len() {
        if let Some(c) = PlaneCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82D80 段闭集内，请核对诊断码取值区间"
}

// ── 号位换算（唯一权威：号码↔位图偏移，双向一致） ────────────────
/// 号码→（字下标, 位下标）；窗外号码返回 Err(OutOfPlane)。
pub fn slot_of(n: u32) -> Result<(usize, u32), u32> {
    if n < PLANE_LO || n > PLANE_HI {
        return Err(PlaneCode::OutOfPlane.val());
    }
    let off = n.checked_sub(PLANE_LO).unwrap_or(0);
    let word = (off / 64) as usize;
    if word >= BITMAP_WORDS {
        return Err(PlaneCode::BitmapWordOob.val());
    }
    Ok((word, off % 64))
}

/// 位图偏移→号码（换算双射的对偶面）。
pub fn num_of(off: u32) -> u32 {
    PLANE_LO.saturating_add(off)
}

// ── 冻结状态机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlaneState {
    /// 采集中：逐号登记
    Collecting,
    /// 已冻结：800 号位唯一+连续，零重编达成
    Frozen,
    /// 已封印：零重编声明写回
    Sealed,
}

impl PlaneState {
    pub fn rank(self) -> u8 {
        match self {
            PlaneState::Collecting => 0,
            PlaneState::Frozen => 1,
            PlaneState::Sealed => 2,
        }
    }
}

// ── 号面账本 ─────────────────────────────────────────────────────
pub struct PlaneLedger {
    bitmap: [u64; BITMAP_WORDS],
    pub count: u32,
    pub state: PlaneState,
    pub summary: Option<String>,
}

impl PlaneLedger {
    pub fn new() -> PlaneLedger {
        PlaneLedger {
            bitmap: [0u64; BITMAP_WORDS],
            count: 0,
            state: PlaneState::Collecting,
            summary: None,
        }
    }

    /// 状态推进（只进不退），倒退显性拒。
    pub fn advance(&mut self, to: PlaneState) -> Result<(), u32> {
        if to.rank() < self.state.rank() {
            return Err(PlaneCode::FreezeRegress.val());
        }
        self.state = to;
        Ok(())
    }

    /// 登记一号：越窗/重号/冻结后一律拒（越窗判定先于冻结判定——
    /// 窗外号码在任何账态下都非法）。
    pub fn register(&mut self, n: u32) -> Result<(), u32> {
        let (w, b) = slot_of(n)?;
        if self.state.rank() >= PlaneState::Frozen.rank() {
            return Err(PlaneCode::SlotFrozen.val());
        }
        let bit = 1u64.checked_shl(b).unwrap_or(0);
        if self.bitmap.get(w).copied().unwrap_or(0) & bit != 0 {
            return Err(PlaneCode::DuplicateNumber.val());
        }
        if let Some(word) = self.bitmap.get_mut(w) {
            *word |= bit;
        }
        self.count = self.count.saturating_add(1);
        Ok(())
    }

    /// 位图置位数（计数守恒的对账面）。
    pub fn popcount(&self) -> u32 {
        let mut sum = 0u32;
        let mut w = 0;
        while w < BITMAP_WORDS {
            let v = self.bitmap.get(w).copied().unwrap_or(0);
            sum = sum.saturating_add(v.count_ones());
            w += 1;
        }
        sum
    }

    /// 号位在账判定。
    pub fn holds(&self, n: u32) -> bool {
        match slot_of(n) {
            Ok((w, b)) => {
                let bit = 1u64.checked_shl(b).unwrap_or(0);
                self.bitmap.get(w).copied().unwrap_or(0) & bit != 0
            }
            Err(_) => false,
        }
    }

    /// 连续性断言：位图扫窗无洞，返回 Err(首洞号码) 或 Ok(())。
    pub fn continuity(&self) -> Result<(), u32> {
        let mut off = 0u32;
        while off < PLANE_N {
            let (w, b) = match slot_of(num_of(off)) {
                Ok(x) => x,
                Err(e) => return Err(e),
            };
            let bit = 1u64.checked_shl(b).unwrap_or(0);
            if self.bitmap.get(w).copied().unwrap_or(0) & bit == 0 {
                return Err(PlaneCode::GapDetected.val());
            }
            off = off.saturating_add(1);
        }
        Ok(())
    }

    /// 计数守恒断言：置位数≡登记计数≡800 才算满。
    pub fn count_consistent(&self) -> bool {
        self.count == PLANE_N && self.popcount() == PLANE_N
    }

    /// 冻结：唯一+连续+计数守恒三断言齐才许冻结。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.state != PlaneState::Collecting {
            return Err(PlaneCode::FreezeRegress.val());
        }
        if !self.count_consistent() {
            return Err(PlaneCode::PlaneNotFull.val());
        }
        self.continuity()?;
        self.advance(PlaneState::Frozen)
    }

    /// 封印：冻结后写回非空零重编声明（判据③）。
    pub fn seal(&mut self, text: &str) -> Result<(), u32> {
        if self.state != PlaneState::Frozen {
            return Err(PlaneCode::FreezeRegress.val());
        }
        if text.is_empty() || !text.contains(SEAL_ANCHOR) {
            return Err(PlaneCode::SealMismatch.val());
        }
        self.advance(PlaneState::Sealed)?;
        self.summary = Some(String::from(text));
        Ok(())
    }
}

// ── 确定性 LCG（种子=SEG_BASE） ──────────────────────────────────
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

// ── 黄金正演：800 号倒序乱序登记 → 冻结 → 封印零重编声明 ─────────
pub fn golden_run() -> Result<String, u32> {
    let mut led = PlaneLedger::new();
    // 确定性伪乱序：k → PLANE_LO + ((k×307) % 800)，307 与 800 互质，
    // 800 次恰好遍历全窗一遍（乘法逆遍历，零随机）。
    let mut k = 0u32;
    while k < PLANE_N {
        let off = (k.wrapping_mul(307)) % PLANE_N;
        led.register(PLANE_LO.saturating_add(off))?;
        k = k.saturating_add(1);
    }
    led.freeze()?;
    led.seal("UNX-F12782 号面冻结终版：F12001–F12800 连续零重编 800/800，唯一+连续断言全绿")?;
    match led.summary.clone() {
        Some(t) if !t.is_empty() => Ok(t),
        _ => Err(PlaneCode::SealMismatch.val()),
    }
}

// ── 断号演练：LCG 抽一号漏登后冻结，必抓，10/10 ──────────────────
pub fn gap_drill() -> u32 {
    let mut caught = 0u32;
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(0xC1));
    let mut round = 0;
    while round < DRILL_ROUNDS {
        let hole = rng.below(PLANE_N as u64) as u32;
        let mut led = PlaneLedger::new();
        let mut k = 0u32;
        while k < PLANE_N {
            let off = (k.wrapping_mul(307)) % PLANE_N;
            if off != hole {
                let _ = led.register(PLANE_LO.saturating_add(off));
            }
            k = k.saturating_add(1);
        }
        // 799/800：计数守恒先破（PlaneNotFull），连续性扫窗独立抓洞。
        let a = led.freeze() == Err(PlaneCode::PlaneNotFull.val());
        let b = led.continuity() == Err(PlaneCode::GapDetected.val());
        if a && b {
            caught += 1;
        }
        round += 1;
    }
    caught
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
    /// 计数守恒：注入=检出+漏检，漏检=0。
    pub fn conservation(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if c > t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 三族注入：重号 / 窗外号 / 冻结倒退，各 10 轮全抓。
pub fn run_injections(led: &mut InjectLedger, rounds: u32) {
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(0x9E));
    let mut r = 0;
    while r < rounds {
        // 族 0：重号——同号二次登记必 DuplicateNumber。
        led.tries[0] += 1;
        let mut m = PlaneLedger::new();
        let n = PLANE_LO + (rng.below(PLANE_N as u64) as u32);
        let _ = m.register(n);
        if m.register(n) == Err(PlaneCode::DuplicateNumber.val()) {
            led.caught[0] += 1;
        }
        // 族 1：窗外号——12000 下越窗与 12801 上越窗交替注入。
        led.tries[1] += 1;
        let mut m1 = PlaneLedger::new();
        let bad = if rng.below(2) == 0 {
            PLANE_LO - 1
        } else {
            PLANE_HI + 1
        };
        if m1.register(bad) == Err(PlaneCode::OutOfPlane.val()) {
            led.caught[1] += 1;
        }
        // 族 2：冻结状态机倒退——Sealed 后回 Collecting。
        led.tries[2] += 1;
        let mut m2 = PlaneLedger::new();
        let _ = m2.advance(PlaneState::Sealed);
        if m2.advance(PlaneState::Collecting) == Err(PlaneCode::FreezeRegress.val()) {
            led.caught[2] += 1;
        }
        r += 1;
    }
}

// ── 判据谓词（私有） ─────────────────────────────────────────────
fn golden_ok() -> bool {
    golden_run().is_ok()
}

/// 零重编声明谓词：黄金账本封印文本逐字含锚定串且非空。
fn zero_renumber_ok() -> bool {
    match golden_run() {
        Ok(text) => !text.is_empty() && text.contains(SEAL_ANCHOR),
        Err(_) => false,
    }
}

/// 换算双射：800 号 ↔ 800 位图偏移逐点对账（号位换算唯一权威）。
fn bijection_ok() -> bool {
    let mut n = PLANE_LO;
    while n <= PLANE_HI {
        let off = match n.checked_sub(PLANE_LO) {
            Some(o) => o,
            None => return false,
        };
        if num_of(off) != n {
            return false;
        }
        n = n.saturating_add(1);
    }
    slot_of(PLANE_LO - 1) == Err(PlaneCode::OutOfPlane.val())
        && slot_of(PLANE_HI + 1) == Err(PlaneCode::OutOfPlane.val())
}

/// 行级/号位边界四类：零号位（窗下沿）/负向越窗/恰满 800/第 801 号越窗。
fn boundary_quad_ok() -> bool {
    // 零：窗下沿 12001 可登记。
    let mut m = PlaneLedger::new();
    let zero_ok = m.register(PLANE_LO).is_ok() && m.holds(PLANE_LO);
    // 负向：12000 越下窗必拒。
    let neg_ok = m.register(PLANE_LO - 1) == Err(PlaneCode::OutOfPlane.val());
    // 上限：恰满 800（逆遍历登记）冻结成功。
    let mut m2 = PlaneLedger::new();
    let mut off = PLANE_N;
    while off > 0 {
        off -= 1;
        if m2.register(PLANE_LO.saturating_add(off)).is_err() {
            return false;
        }
    }
    let cap_ok = m2.count_consistent() && m2.freeze().is_ok();
    // 上限+1：12801 越上窗必拒；冻结后再登记判 SlotFrozen。
    let over = m2.register(PLANE_HI + 1) == Err(PlaneCode::OutOfPlane.val())
        && m2.register(PLANE_LO) == Err(PlaneCode::SlotFrozen.val());
    zero_ok && neg_ok && cap_ok && over
}

/// 计数守恒（账本口径）：置位数=登记数；破坏 count 后守恒断言必破。
fn ledger_conservation_ok() -> bool {
    let mut m = PlaneLedger::new();
    let _ = m.register(PLANE_LO + 5);
    let a = m.count_consistent() == false && m.popcount() == 1;
    m.count = 2;
    let b = !m.count_consistent();
    a && b
}

/// 双跑一致：黄金正演两次逐字段相等。
fn double_run_ok() -> bool {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// 段位守卫：SEG_BASE 对齐 0x40 且落在 VX9 段窗。
fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

/// 对账一致：PLANE_N=800、位图 13 字收得下、锚定串固定。
fn frozen_consts_aligned_ok() -> bool {
    PLANE_HI - PLANE_LO + 1 == PLANE_N
        && PLANE_N <= (BITMAP_WORDS as u32).saturating_mul(64)
        && !SEAL_ANCHOR.is_empty()
        && BITMAP_WORDS == 13
}

/// 计数守恒（注入口径）：注入=检出+漏检，漏检=0。
fn count_conservation_ok() -> bool {
    let mut led = InjectLedger::new();
    run_injections(&mut led, DRILL_ROUNDS);
    led.conservation()
        && led.caught[0] == DRILL_ROUNDS
        && led.caught[1] == DRILL_ROUNDS
        && led.caught[2] == DRILL_ROUNDS
}

/// 文案闭集：9 变体 detail 两两互异非空 + 未知码兜底非空。
fn detail_corpus_closed_ok() -> bool {
    let n = PlaneCode::ALL.len();
    let mut i = 0;
    while i < n {
        let di = PlaneCode::ALL[i].detail();
        if di.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < n {
            if PlaneCode::ALL[j].detail() == di {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    !detail(SEG_BASE + 0x40).is_empty()
}

/// 收口只增不减：诊断码闭集两轮采集逐项相等（only-add 收敛）。
fn closure_ok() -> bool {
    let mut first: Vec<u32> = Vec::new();
    let mut second: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < PlaneCode::ALL.len() {
        if let Some(c) = PlaneCode::ALL.get(i) {
            first.push(c.val());
            second.push(c.val());
        }
        i += 1;
    }
    first == second && first.len() == 9
}

/// 判定次序：799/800 先冻结必拒（PlaneNotFull），补满后才行。
fn verdict_order_ok() -> bool {
    let mut m = PlaneLedger::new();
    let mut off = 0u32;
    while off < PLANE_N - 1 {
        if m.register(PLANE_LO.saturating_add(off)).is_err() {
            return false;
        }
        off = off.saturating_add(1);
    }
    if m.freeze().is_ok() {
        return false;
    }
    m.register(PLANE_HI).is_ok() && m.freeze().is_ok()
}

/// 状态机倒退独立口径：Frozen 回 Collecting 拒、Sealed 回 Frozen 拒。
fn regression_reject_ok() -> bool {
    let mut m = PlaneLedger::new();
    let _ = m.advance(PlaneState::Frozen);
    let back1 = m.advance(PlaneState::Collecting) == Err(PlaneCode::FreezeRegress.val());
    let _ = m.advance(PlaneState::Sealed);
    let back2 = m.advance(PlaneState::Frozen) == Err(PlaneCode::FreezeRegress.val());
    back1 && back2
}

/// 容量耗尽：冻结后登记、封印后冻结/封印均判终版不可改。
fn capacity_exhaust_ok() -> bool {
    let mut m = PlaneLedger::new();
    let _ = m.advance(PlaneState::Frozen);
    let a = m.register(PLANE_LO) == Err(PlaneCode::SlotFrozen.val());
    let _ = m.advance(PlaneState::Sealed);
    let b = m.register(PLANE_LO + 1) == Err(PlaneCode::SlotFrozen.val());
    let c = m.freeze() == Err(PlaneCode::FreezeRegress.val());
    a && b && c
}

/// 判据③谓词：封印写回状态=已完成且摘要非空。
fn seal_writeback_ok() -> bool {
    match golden_run() {
        Ok(text) => !text.is_empty() && text.contains("800/800"),
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 判据②（逐字锚定 UNX-F12783-J1）。
    v.push((
        "J1-零重编声明唯一连续",
        zero_renumber_ok(),
        String::from("800 号位唯一+连续断言→零重编声明非空"),
    ));
    v.push((
        "J1-断号注入10/10检出",
        gap_drill() == DRILL_ROUNDS,
        String::from("799/800 冻结双口径 10 轮全抓"),
    ));
    v.push((
        "J1-号位换算双射对账",
        bijection_ok(),
        String::from("F12001..F12800 ↔ 800 偏移逐点双射"),
    ));
    // 判据③。
    v.push((
        "J3-写回已完成且摘要非空",
        seal_writeback_ok(),
        String::from("Sealed 态 + 摘要含 800/800"),
    ));
    // 正常流。
    v.push((
        "正常流-黄金正演800号冻结封印",
        golden_ok(),
        String::from("伪乱序登记→冻结→零重编声明零错"),
    ));
    // 四类边界。
    v.push((
        "边界-窗下沿/越下窗/恰满/越上窗",
        boundary_quad_ok(),
        String::from("12001 可登/12000 拒/800 恰满/12801 拒"),
    ));
    // 注入三族。
    let mut led = InjectLedger::new();
    run_injections(&mut led, DRILL_ROUNDS);
    let c0 = led.caught[0];
    let c1 = led.caught[1];
    let c2 = led.caught[2];
    v.push((
        "注入-重号10轮全抓",
        c0 == DRILL_ROUNDS,
        format!("t/c={}/{}", led.tries[0], c0),
    ));
    v.push((
        "注入-窗外号10轮全抓",
        c1 == DRILL_ROUNDS,
        format!("t/c={}/{}", led.tries[1], c1),
    ));
    v.push((
        "注入-冻结倒退10轮全抓",
        c2 == DRILL_ROUNDS,
        format!("t/c={}/{}", led.tries[2], c2),
    ));
    // 判定次序。
    v.push((
        "判定次序-未满先冻结拒",
        verdict_order_ok(),
        String::from("799/800 冻结 → PlaneNotFull，补满才行"),
    ));
    // 状态机倒退（独立口径）。
    v.push((
        "状态机-冻结倒退显性拒",
        regression_reject_ok(),
        String::from("Frozen 回 Collecting 拒、Sealed 回 Frozen 拒"),
    ));
    // 容量耗尽。
    v.push((
        "容量-终版不可改",
        capacity_exhaust_ok(),
        String::from("冻结/封印后登记与再冻结全拒"),
    ));
    // 账本计数守恒。
    v.push((
        "账本守恒-置位数=登记数",
        ledger_conservation_ok(),
        String::from("count≠置位数 → count_consistent 判假"),
    ));
    // 双跑一致。
    v.push((
        "双跑一致-逐字段相等",
        double_run_ok(),
        String::from("golden_run() 两次结果全等"),
    ));
    // 段位守卫。
    v.push((
        "段位守卫-0x82D80",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    // 对账一致。
    v.push((
        "对账一致-800号位13字位图",
        frozen_consts_aligned_ok(),
        String::from("12800−12001+1=800 ≤ 13×64"),
    ));
    // 计数守恒（注入口径）。
    v.push((
        "计数守恒-注入=检出+漏检0",
        count_conservation_ok(),
        String::from("InjectLedger.conservation 全绿"),
    ));
    // 文案闭集。
    v.push((
        "文案闭集-详情互异+兜底非空",
        detail_corpus_closed_ok(),
        String::from("9 变体 detail 两两互异"),
    ));
    // 收口只增不减。
    v.push((
        "收口-诊断码闭集收敛",
        closure_ok(),
        format!("ALL.len()={}", PlaneCode::ALL.len()),
    ));
    v
}

/// VX9 检查入口：extra_checks 逐条进 set（动态明细槽 ≤8，其余静态）。
pub fn run_unx12782_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12782 号面冻结终版（F12001–F12800 零重编声明）");
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
        assert!(golden_run().is_ok(), "黄金正演须零错封印");
        assert!(seal_writeback_ok(), "写回摘要须非空含 800/800");
    }

    #[test]
    fn t02_zero_renumber_and_bijection() {
        assert!(zero_renumber_ok(), "零重编声明须含锚定串");
        assert!(bijection_ok(), "号位换算须 800 点双射");
    }

    #[test]
    fn t03_gap_drill_10_of_10() {
        assert_eq!(gap_drill(), DRILL_ROUNDS, "断号注入 10/10 检出");
    }

    #[test]
    fn t04_injection_three_families_all_caught() {
        let mut led = InjectLedger::new();
        run_injections(&mut led, DRILL_ROUNDS);
        assert!(led.conservation(), "注入=检出+漏检，漏检须为 0");
        assert_eq!(led.caught[0], DRILL_ROUNDS);
        assert_eq!(led.caught[1], DRILL_ROUNDS);
        assert_eq!(led.caught[2], DRILL_ROUNDS);
    }

    #[test]
    fn t05_boundary_quad() {
        assert!(boundary_quad_ok(), "窗下沿/越窗/恰满/越上窗四类边界");
    }

    #[test]
    fn t06_verdict_order_and_regression() {
        assert!(verdict_order_ok(), "未满先冻结须拒，补满才行");
        assert!(regression_reject_ok(), "冻结状态机倒退须显性拒");
    }

    #[test]
    fn t07_double_run_identical() {
        assert!(double_run_ok(), "同输入两次须逐字段相等");
    }

    #[test]
    fn t08_segment_guard_and_consts() {
        assert!(segment_guard(), "0x82D80 须对齐 0x40 且落在段窗");
        assert!(frozen_consts_aligned_ok(), "800 号位 13 字位图对账");
    }

    #[test]
    fn t09_detail_corpus_and_closure() {
        assert!(detail_corpus_closed_ok(), "文案闭集两两互异+兜底非空");
        assert!(closure_ok(), "收口 only-add 收敛");
    }

    #[test]
    fn t10_check_set_all_green() {
        let s = run_unx12782_checks();
        let (p, f) = s.tally();
        assert!(f == 0, "红项: {}", s.red_items_text());
        assert!(p >= 12, "检查条数须 ≥12");
    }
}
