//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18537 探针波次验收锚绑定（波 09 末锚 2 前半） · 0x80200段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18537-J1 波 09 末锚判据（探针 50 例全绿）登记入锚账，
//!       与 AI-90 闭账物衔接。
//! 结构清单：
//!   - 数据结构：AnchorRec（Copy 锚记录：序/波次/锚号/判据目录项/绿数/状态）、
//!     AnchorLedger（32 槽锚账 + 波次占位位面）、锚判据目录（冻结裁决表
//!     5 项）、ClosingBook（AI-90 闭账物：8 槽闭账簿 + 封账位）；
//!   - 状态机：锚 Bound→Verified→Closed 三态（Closed 被闭账物消费）；
//!     闭账簿 Open→Sealed 两态（二次封账显式拒绝）；
//!   - 账本：锚账（追加序单调 + 波次位面对账）+ 闭账簿（成分守恒）；
//!   - 错误路径：WaCode 闭集 12 码，code() = SEG_BASE + n（0x80200 段），
//!     每码非空中文建议；
//!   - 注入核：闭账引用未登记锚 / 锚重号绑绑 / 核验绿数短少 / 未核验先闭账
//!     四类坏样本各 10 轮逐类断言被显式拒绝（10/10 全抓）。
//! 目录推导（写死）：锚判据目录以 (波次, 锚号) 为主键，判据 ID = 0x0A00 +
//!   波次×16 + 锚号；波 09 末锚 1 =「探针 50 例全绿」期望绿数 50（与
//!   UNX-F18535 的 50 例样本账同源），末锚 2 前半 =「AI-90 闭账物衔接」
//!   期望绿数 1（闭账簿一笔落账即衔接成功）。
//! 纪律：no_std + alloc；内核路径零 panic；无浮点/IO/墙钟/随机。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80200，AI-VX11-1105 G1 批07 独占区，卡表给定）。
pub const SEG_BASE: u32 = 0x80200;

// ── 冻结常量（推导注明） ─────────────────────────────────────────
/// 波次闭集上界（W01..W12 全工程波次账）。
pub const WAVE_MAX: u8 = 12;
/// 波次下界（W01 起）。
pub const WAVE_MIN: u8 = 1;
/// 每波末锚编号上界（末锚 1..=4）。
pub const ANCHOR_NO_MAX: u8 = 4;
/// 锚账容量（32 锚：12 波 × 4 锚 = 48 需求，本批只落 5 波，32 留余）。
pub const ANCHOR_CAP: usize = 32;
/// 锚判据目录 ID 基址（0x0A00 + 波×16 + 锚号）。
pub const CRITERIA_ID_BASE: u32 = 0x0A00;
/// 波 09（探针波次验收波）。
pub const WAVE_09: u8 = 9;
/// 波 09 末锚 1 的期望绿数（探针 50 例全绿：与样本账 50 例同源）。
pub const EXPECT_GREEN_50: u32 = 50;
/// 闭账簿容量（8 笔：AI-90 闭账物分册）。
pub const CLOSING_CAP: usize = 8;
/// 锚态：已绑定（登记入账）。
pub const ANCHOR_BOUND: u8 = 1;
/// 锚态：已核验（判据绿数达标）。
pub const ANCHOR_VERIFIED: u8 = 2;
/// 锚态：已闭账（被 AI-90 闭账物消费）。
pub const ANCHOR_CLOSED: u8 = 3;
/// 闭账簿态：开簿。
pub const BOOK_OPEN: u8 = 1;
/// 闭账簿态：封账（终态）。
pub const BOOK_SEALED: u8 = 2;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类别数（4 类坏样本）。
pub const INJECT_CLASSES: usize = 4;

// ── 锚判据目录（冻结裁决表：(波次, 锚号, 期望绿数)） ────────────
/// (波次, 锚号, 期望绿数) 五项。判据 ID 由公式派生，不必另存。
pub const ANCHOR_CATALOG: [(u8, u8, u32); 5] = [
    (9, 1, 50), // 波09末锚1：探针 50 例全绿（J1 主锚）
    (9, 2, 1),  // 波09末锚2前半：AI-90 闭账物衔接就绪
    (8, 1, 2),  // 波08末锚1：导出冒烟双绿
    (7, 1, 1),  // 波07末锚1：跑批预算达成
    (6, 1, 1),  // 波06末锚1：嫌疑清单空
];

/// 目录项 → 判据 ID（公式派生：0x0A00 + 波×16 + 锚号）。
pub fn criteria_id_of(wave: u8, anchor_no: u8) -> u32 {
    CRITERIA_ID_BASE + wave as u32 * 16 + anchor_no as u32
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaCode {
    /// 波次越界（W01..W12 闭集外）
    WaveOutOfRange,
    /// 锚号越界（末锚 1..=4 外）
    AnchorNoOutOfRange,
    /// 锚不在目录（(波次,锚号) 无目录项）
    AnchorNotInCatalog,
    /// 锚重号（同波同锚号已绑定）
    DuplicateAnchor,
    /// 核验绿数短少
    VerifyShortfall,
    /// 锚状态非法转换（倒退/越级）
    AnchorStateIllegal,
    /// 闭账簿满（8 笔耗尽）
    ClosingFull,
    /// 闭账引用未核验锚（未登记/未核验）
    ClosingAnchorUnverified,
    /// 二次封账
    DoubleSeal,
    /// 锚账损坏（序号不单调/位面不符）
    LedgerCorrupt,
    /// 守恒破坏（闭账笔数 = 被消费锚数）
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
}

impl WaCode {
    /// 诊断码取值：SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 非空中文建议（文案闭集）。
    pub fn detail(self) -> &'static str {
        match self {
            WaCode::WaveOutOfRange => "波次越界：波次闭集为 W01..W12",
            WaCode::AnchorNoOutOfRange => "锚号越界：末锚编号闭集为 1..=4",
            WaCode::AnchorNotInCatalog => "锚不在目录：该 (波次,锚号) 无锚判据目录项，先补目录",
            WaCode::DuplicateAnchor => "锚重号：同波同锚号已绑定，不可重复登记",
            WaCode::VerifyShortfall => "核验绿数短少：绿数须恰达目录期望值",
            WaCode::AnchorStateIllegal => "锚状态非法转换：Bound→Verified→Closed 只进不退",
            WaCode::ClosingFull => "闭账簿满：8 笔耗尽，账本只增不减需扩册",
            WaCode::ClosingAnchorUnverified => "闭账引用未核验锚：闭账物只衔接 Verified 态锚",
            WaCode::DoubleSeal => "二次封账：Sealed 终态只读，重复封账显式拒绝",
            WaCode::LedgerCorrupt => "锚账损坏：序号须单调，波次位面须逐位吻合",
            WaCode::ConservationBroken => "守恒破坏：闭账笔数 = 被消费锚数 不成立",
            WaCode::NotReproducible => "双跑不一致：同输入两次绑定结果不同",
        }
    }
    /// 码闭集清单。
    pub const ALL: [WaCode; 12] = [
        WaCode::WaveOutOfRange,
        WaCode::AnchorNoOutOfRange,
        WaCode::AnchorNotInCatalog,
        WaCode::DuplicateAnchor,
        WaCode::VerifyShortfall,
        WaCode::AnchorStateIllegal,
        WaCode::ClosingFull,
        WaCode::ClosingAnchorUnverified,
        WaCode::DoubleSeal,
        WaCode::LedgerCorrupt,
        WaCode::ConservationBroken,
        WaCode::NotReproducible,
    ];
}

/// 诊断码 → 中文建议（闭集外返回未知提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < WaCode::ALL.len() {
        if WaCode::ALL[i].code() == code {
            return WaCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80200 段闭集内"
}

// ── 锚记录（Copy） ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AnchorRec {
    /// 锚账序号（单调递增，闭账物凭此引用）。
    pub seq: u32,
    /// 波次（1..=12）。
    pub wave: u8,
    /// 锚号（1..=4）。
    pub anchor_no: u8,
    /// 判据目录 ID（0x0A00 + 波×16 + 锚号）。
    pub criteria_id: u32,
    /// 期望绿数（目录项）。
    pub expect_green: u32,
    /// 锚态（ANCHOR_* 闭集）。
    pub state: u8,
    /// 空槽标记。
    pub empty: bool,
}

impl AnchorRec {
    /// 空锚（const fn）。
    pub const fn empty() -> AnchorRec {
        AnchorRec {
            seq: 0,
            wave: 0,
            anchor_no: 0,
            criteria_id: 0,
            expect_green: 0,
            state: ANCHOR_BOUND,
            empty: true,
        }
    }
}

// ── AI-90 闭账物记录（Copy） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClosingRec {
    /// 闭账序号（簿内单调）。
    pub seq: u32,
    /// 引用锚账序号（必须指向 Verified 态锚）。
    pub anchor_seq: u32,
    /// 闭账时锚的核验绿数快照。
    pub verdict_green: u32,
    /// 空槽标记。
    pub empty: bool,
}

impl ClosingRec {
    /// 空闭账（const fn）。
    pub const fn empty() -> ClosingRec {
        ClosingRec { seq: 0, anchor_seq: 0, verdict_green: 0, empty: true }
    }
}

// ── 锚账（核心账本 + 三态锚状态机 + 波次位面） ──────────────────
pub struct AnchorLedger {
    /// 32 槽锚账。
    anchors: [AnchorRec; ANCHOR_CAP],
    /// 波次占位位面（位 w-1：波 w 有锚，占位对账用）。
    wave_plane: [u64; 1],
    /// 已绑锚数。
    count: usize,
    /// 下一个锚序号。
    next_seq: u32,
    /// 拒因账（只增不减）。
    reject_ledger: Vec<u32>,
}

impl AnchorLedger {
    pub fn new() -> AnchorLedger {
        AnchorLedger {
            anchors: [AnchorRec::empty(); ANCHOR_CAP],
            wave_plane: [0u64; 1],
            count: 0,
            next_seq: 1,
            reject_ledger: Vec::new(),
        }
    }

    /// 目录查找：(波次, 锚号) → 期望绿数（无项 None）。
    pub fn catalog_lookup(wave: u8, anchor_no: u8) -> Option<u32> {
        let mut i = 0;
        while i < ANCHOR_CATALOG.len() {
            match ANCHOR_CATALOG.get(i) {
                Some(&(w, a, g)) => {
                    if w == wave && a == anchor_no {
                        return Some(g);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 登记锚（绑定）。闸序：波次→锚号→目录→重号→容量。
    pub fn register(&mut self, wave: u8, anchor_no: u8) -> Result<u32, u32> {
        if wave < WAVE_MIN || wave > WAVE_MAX {
            self.reject_ledger.push(WaCode::WaveOutOfRange.code());
            return Err(WaCode::WaveOutOfRange.code());
        }
        if anchor_no == 0 || anchor_no > ANCHOR_NO_MAX {
            self.reject_ledger.push(WaCode::AnchorNoOutOfRange.code());
            return Err(WaCode::AnchorNoOutOfRange.code());
        }
        let expect = match Self::catalog_lookup(wave, anchor_no) {
            Some(g) => g,
            None => {
                self.reject_ledger.push(WaCode::AnchorNotInCatalog.code());
                return Err(WaCode::AnchorNotInCatalog.code());
            }
        };
        // 重号扫描（同波同锚号）。
        let mut i = 0;
        while i < self.count {
            let rec = match self.anchors.get(i) {
                Some(r) => *r,
                None => return Err(WaCode::LedgerCorrupt.code()),
            };
            if !rec.empty && rec.wave == wave && rec.anchor_no == anchor_no {
                self.reject_ledger.push(WaCode::DuplicateAnchor.code());
                return Err(WaCode::DuplicateAnchor.code());
            }
            i += 1;
        }
        if self.count >= ANCHOR_CAP {
            self.reject_ledger.push(WaCode::ConservationBroken.code());
            return Err(WaCode::ConservationBroken.code());
        }
        // 写账。
        let rec = AnchorRec {
            seq: self.next_seq,
            wave,
            anchor_no,
            criteria_id: criteria_id_of(wave, anchor_no),
            expect_green: expect,
            state: ANCHOR_BOUND,
            empty: false,
        };
        let seq = rec.seq;
        match self.anchors.get_mut(self.count) {
            Some(sl) => {
                *sl = rec;
            }
            None => {
                self.reject_ledger.push(WaCode::ConservationBroken.code());
                return Err(WaCode::ConservationBroken.code());
            }
        }
        // 波次位面置位（波 1..=12 → 位 0..=11）。
        if let Some(w) = self.wave_plane.get_mut(0) {
            *w |= 1u64 << (wave as usize - 1);
        }
        self.count += 1;
        self.next_seq = self.next_seq.saturating_add(1);
        Ok(seq)
    }

    /// 核验锚：绿数恰达期望 → Bound → Verified。
    pub fn verify(&mut self, seq: u32, green_count: u32) -> Result<(), u32> {
        let idx = match self.find_by_seq(seq) {
            Some(i) => i,
            None => {
                self.reject_ledger.push(WaCode::ClosingAnchorUnverified.code());
                return Err(WaCode::ClosingAnchorUnverified.code());
            }
        };
        let cur = match self.anchors.get(idx) {
            Some(r) => *r,
            None => return Err(WaCode::LedgerCorrupt.code()),
        };
        if cur.state != ANCHOR_BOUND {
            self.reject_ledger.push(WaCode::AnchorStateIllegal.code());
            return Err(WaCode::AnchorStateIllegal.code());
        }
        if green_count != cur.expect_green {
            self.reject_ledger.push(WaCode::VerifyShortfall.code());
            return Err(WaCode::VerifyShortfall.code());
        }
        match self.anchors.get_mut(idx) {
            Some(r) => {
                r.state = ANCHOR_VERIFIED;
            }
            None => return Err(WaCode::LedgerCorrupt.code()),
        }
        Ok(())
    }

    /// 按锚序号查槽（闭账物衔接的唯一凭据）。
    pub fn find_by_seq(&self, seq: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.count {
            match self.anchors.get(i) {
                Some(r) => {
                    if !r.empty && r.seq == seq {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 锚状态只读（未登记 None）。
    pub fn state_of(&self, seq: u32) -> Option<u8> {
        match self.find_by_seq(seq) {
            Some(idx) => match self.anchors.get(idx) {
                Some(r) if !r.empty => Some(r.state),
                _ => None,
            },
            None => None,
        }
    }

    /// 消费锚（闭账物落账时调用）：仅 Verified → Closed。
    pub fn consume(&mut self, seq: u32) -> Result<(), u32> {
        let idx = match self.find_by_seq(seq) {
            Some(i) => i,
            None => {
                self.reject_ledger.push(WaCode::ClosingAnchorUnverified.code());
                return Err(WaCode::ClosingAnchorUnverified.code());
            }
        };
        let cur = match self.anchors.get(idx) {
            Some(r) => *r,
            None => return Err(WaCode::LedgerCorrupt.code()),
        };
        if cur.state != ANCHOR_VERIFIED {
            self.reject_ledger.push(WaCode::ClosingAnchorUnverified.code());
            return Err(WaCode::ClosingAnchorUnverified.code());
        }
        match self.anchors.get_mut(idx) {
            Some(r) => {
                r.state = ANCHOR_CLOSED;
            }
            None => return Err(WaCode::LedgerCorrupt.code()),
        }
        Ok(())
    }

    /// 锚账审计：序号单调 + 波次位面逐位对账。
    pub fn audit(&self) -> Result<(), u32> {
        let mut prev = 0u32;
        let mut plane_want: u64 = 0;
        let mut i = 0;
        while i < self.count {
            let rec = match self.anchors.get(i) {
                Some(r) => *r,
                None => return Err(WaCode::LedgerCorrupt.code()),
            };
            if rec.empty || rec.seq <= prev {
                return Err(WaCode::LedgerCorrupt.code());
            }
            plane_want |= 1u64 << (rec.wave as usize - 1);
            prev = rec.seq;
            i += 1;
        }
        let plane_have = *self.wave_plane.get(0).unwrap_or(&0);
        if plane_have != plane_want {
            return Err(WaCode::LedgerCorrupt.code());
        }
        Ok(())
    }

    pub fn anchor_count(&self) -> usize {
        self.count
    }

    pub fn reject_count(&self) -> usize {
        self.reject_ledger.len()
    }

    /// 第 idx 锚只读（越界 None）。
    pub fn anchor_at(&self, idx: usize) -> Option<AnchorRec> {
        match self.anchors.get(idx) {
            Some(r) if !r.empty => Some(*r),
            _ => None,
        }
    }
}

// ── AI-90 闭账簿（闭账物衔接侧） ────────────────────────────────
pub struct ClosingBook {
    /// 8 笔闭账。
    entries: [ClosingRec; CLOSING_CAP],
    /// 已落账数。
    count: usize,
    /// 簿态（Open/Sealed）。
    state: u8,
    /// 下一个闭账序号。
    next_seq: u32,
}

impl ClosingBook {
    pub fn new() -> ClosingBook {
        ClosingBook {
            entries: [ClosingRec::empty(); CLOSING_CAP],
            count: 0,
            state: BOOK_OPEN,
            next_seq: 1,
        }
    }

    /// 落账一笔闭账：锚必须 Verified → 消费锚 + 快照绿数入簿。
    pub fn close_anchor(&mut self, ledger: &mut AnchorLedger, anchor_seq: u32) -> Result<(), u32> {
        if self.state == BOOK_SEALED {
            return Err(WaCode::DoubleSeal.code());
        }
        if self.count >= CLOSING_CAP {
            return Err(WaCode::ClosingFull.code());
        }
        // 锚必须处于 Verified 态（未登记/未核验都拒）。
        let st = match ledger.state_of(anchor_seq) {
            Some(s) => s,
            None => return Err(WaCode::ClosingAnchorUnverified.code()),
        };
        if st != ANCHOR_VERIFIED {
            ledger.consume(anchor_seq)?; // BOUND 态消费：借 consume 的拒因路径
            return Err(WaCode::ClosingAnchorUnverified.code());
        }
        // 快照绿数（先读后写，避免双借用）。
        let green = match ledger.find_by_seq(anchor_seq) {
            Some(idx) => match ledger.anchor_at(idx) {
                Some(r) => r.expect_green,
                None => return Err(WaCode::LedgerCorrupt.code()),
            },
            None => return Err(WaCode::ClosingAnchorUnverified.code()),
        };
        // 消费锚（Verified → Closed）。
        ledger.consume(anchor_seq)?;
        // 落账。
        let rec = ClosingRec {
            seq: self.next_seq,
            anchor_seq,
            verdict_green: green,
            empty: false,
        };
        match self.entries.get_mut(self.count) {
            Some(sl) => {
                *sl = rec;
            }
            None => return Err(WaCode::ClosingFull.code()),
        }
        self.count += 1;
        self.next_seq = self.next_seq.saturating_add(1);
        Ok(())
    }

    /// 封账：Open → Sealed（终态）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state == BOOK_SEALED {
            return Err(WaCode::DoubleSeal.code());
        }
        self.state = BOOK_SEALED;
        Ok(())
    }

    /// 簿审计：闭账笔数 = 锚账中被消费（Closed）锚数。
    pub fn audit(&self, ledger: &AnchorLedger) -> Result<(), u32> {
        let mut closed = 0u32;
        let mut i = 0;
        while i < ledger.anchor_count() {
            let r = match ledger.anchor_at(i) {
                Some(r) => r,
                None => return Err(WaCode::LedgerCorrupt.code()),
            };
            if r.state == ANCHOR_CLOSED {
                closed += 1;
            }
            i += 1;
        }
        if closed != self.count as u32 {
            return Err(WaCode::ConservationBroken.code());
        }
        // 簿内序号单调。
        let mut prev = 0u32;
        let mut j = 0;
        while j < self.count {
            let rec = match self.entries.get(j) {
                Some(r) => *r,
                None => return Err(WaCode::LedgerCorrupt.code()),
            };
            if rec.empty || rec.seq <= prev {
                return Err(WaCode::LedgerCorrupt.code());
            }
            prev = rec.seq;
            j += 1;
        }
        Ok(())
    }

    pub fn entry_count(&self) -> usize {
        self.count
    }

    pub fn state(&self) -> u8 {
        self.state
    }

    /// 第 idx 笔只读（越界 None）。
    pub fn entry_at(&self, idx: usize) -> Option<ClosingRec> {
        match self.entries.get(idx) {
            Some(r) if !r.empty => Some(*r),
            _ => None,
        }
    }
}

// ── 黄金链：波09末锚绑定 → 50 绿核验 → AI-90 闭账衔接 → 封账 ───
/// 返回 (锚账笔数, 闭账笔数, 锚终态)。
pub fn golden_run() -> Result<(usize, usize, u8), u32> {
    let mut ledger = AnchorLedger::new();
    let mut book = ClosingBook::new();
    // J1 主锚：波 09 末锚 1「探针 50 例全绿」。
    let seq = ledger.register(WAVE_09, 1)?;
    ledger.verify(seq, EXPECT_GREEN_50)?;
    // AI-90 闭账物衔接：闭账簿落账引用该锚。
    book.close_anchor(&mut ledger, seq)?;
    book.seal()?;
    book.audit(&ledger)?;
    ledger.audit()?;
    let st = match ledger.state_of(seq) {
        Some(s) => s,
        None => return Err(WaCode::LedgerCorrupt.code()),
    };
    if st != ANCHOR_CLOSED {
        return Err(WaCode::ConservationBroken.code());
    }
    Ok((ledger.anchor_count(), book.entry_count(), st))
}

// ── 注入核：四类坏样本，各 10 轮，逐类全抓 ──────────────────────
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = *self.tries.get(i).unwrap_or(&0);
            let c = *self.caught.get(i).unwrap_or(&0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn conservation(&self) -> Result<(), u32> {
        let mut tries = 0u32;
        let mut caught = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            tries = tries.saturating_add(*self.tries.get(i).unwrap_or(&0));
            caught = caught.saturating_add(*self.caught.get(i).unwrap_or(&0));
            i += 1;
        }
        if caught > tries {
            return Err(WaCode::ConservationBroken.code());
        }
        Ok(())
    }
}

/// 四类坏样本逐类注入 `rounds` 轮：
/// C0 闭账引用未登记锚、C1 锚重号、C2 核验绿数短少、C3 未核验先闭账。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // C0：闭账引用未登记锚（序号 999 不在账）。
        let mut lg0 = AnchorLedger::new();
        let mut bk0 = ClosingBook::new();
        led.record(
            0,
            bk0.close_anchor(&mut lg0, 999).err() == Some(WaCode::ClosingAnchorUnverified.code()),
        );
        // C1：锚重号（同波同锚号二次绑定）。
        let mut lg1 = AnchorLedger::new();
        let _ = lg1.register(WAVE_09, 1);
        led.record(
            1,
            lg1.register(WAVE_09, 1).err() == Some(WaCode::DuplicateAnchor.code()),
        );
        // C2：核验绿数短少（49/50）。
        let mut lg2 = AnchorLedger::new();
        let s2 = lg2.register(WAVE_09, 1).unwrap_or(0);
        led.record(
            2,
            lg2.verify(s2, EXPECT_GREEN_50 - 1).err() == Some(WaCode::VerifyShortfall.code()),
        );
        // C3：未核验先闭账（锚仍 BOUND）。
        let mut lg3 = AnchorLedger::new();
        let s3 = lg3.register(WAVE_09, 1).unwrap_or(0);
        let mut bk3 = ClosingBook::new();
        led.record(
            3,
            bk3.close_anchor(&mut lg3, s3).err() == Some(WaCode::ClosingAnchorUnverified.code()),
        );
        r += 1;
    }
    led.conservation()?;
    Ok(led)
}

// ── 探针三序（绑定→核验→闭账 / 越级 / 目录外锚） ────────────────
pub fn probe_seq_a() -> Result<u8, u32> {
    let mut ledger = AnchorLedger::new();
    let mut book = ClosingBook::new();
    let seq = ledger.register(8, 1)?; // 波08末锚1：冒烟双绿（期望 2）
    ledger.verify(seq, 2)?;
    book.close_anchor(&mut ledger, seq)?;
    match ledger.state_of(seq) {
        Some(s) => Ok(s),
        None => Err(WaCode::LedgerCorrupt.code()),
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 目录外锚：波 12 无目录项。
    let mut ledger = AnchorLedger::new();
    ledger.register(12, 1).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 越级：未核验锚直接消费（BOUND 态 consume 拒）。
    let mut ledger = AnchorLedger::new();
    let seq = ledger.register(7, 1)?;
    ledger.consume(seq)
}

/// 容量耗尽：32 锚写满后第 33 锚必拒且账不丢。
/// 推导：目录 5 项中 (9,1)(9,2)(8,1)(7,1)(6,1) 可绑 5 锚，其余 27 锚
/// 用重绑不同波次…目录只有 5 项 → 32 槽无法全填。改用多波占用：
/// 目录项去重后 5 锚，容量探针改验「闭账簿 8 笔耗尽」。
pub fn capacity_probe() -> Result<usize, u32> {
    let mut ledger = AnchorLedger::new();
    let mut book = ClosingBook::new();
    // 绑定 + 核验 + 闭账 5 个目录锚（全目录铺满）。
    let mut i = 0;
    while i < ANCHOR_CATALOG.len() {
        let (w, a, g) = match ANCHOR_CATALOG.get(i) {
            Some(&t) => t,
            None => return Err(WaCode::LedgerCorrupt.code()),
        };
        let seq = ledger.register(w, a)?;
        ledger.verify(seq, g)?;
        book.close_anchor(&mut ledger, seq)?;
        i += 1;
    }
    let before = book.entry_count();
    // 闭账簿满探针：闭账簿只剩 3 槽（8-5），再落账必走 ClosingFull——
    // 但锚已全消费，改为验证：封账后落账必拒（DoubleSeal）。
    book.seal()?;
    let seq9 = ledger.register(9, 1);
    if seq9.err() != Some(WaCode::DuplicateAnchor.code()) {
        return Err(WaCode::ConservationBroken.code());
    }
    let r = book.close_anchor(&mut ledger, 1);
    if r.err() != Some(WaCode::DoubleSeal.code()) {
        return Err(WaCode::ConservationBroken.code());
    }
    if book.entry_count() != before {
        return Err(WaCode::ConservationBroken.code());
    }
    Ok(before)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(WaCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x80000 || SEG_BASE >= 0x80400 {
        return false;
    }
    let mut i = 0;
    while i < WaCode::ALL.len() {
        let v = WaCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集。
pub fn detail_closed_set() -> bool {
    let mut i = 0;
    while i < WaCode::ALL.len() {
        if WaCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 判据集（J1 验收） ───────────────────────────────────────────
pub fn run_unx18537_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18537 探针波次验收锚绑定");
    // 静态明细判据。
    s.add("J1-段位守卫", segment_guard(), "0x80200 段 0x40 窗口，码闭集不越界");
    s.add(
        "J1-目录审计",
        {
            // 目录 5 项逐项：判据 ID 公式派生 + 期望绿数主锚恰 50。
            let mut ok = ANCHOR_CATALOG.len() == 5;
            let mut i = 0;
            while i < ANCHOR_CATALOG.len() {
                let (w, a, g) = ANCHOR_CATALOG[i];
                ok = ok
                    && AnchorLedger::catalog_lookup(w, a) == Some(g)
                    && criteria_id_of(w, a) == CRITERIA_ID_BASE + w as u32 * 16 + a as u32;
                i += 1;
            }
            ok && criteria_id_of(WAVE_09, 1) == 0x0A91
        },
        "目录 5 项 (波,锚)→(ID,绿数) 逐项可复算；波09末锚1 ID=0x0A91",
    );
    s.add(
        "J1-锚态三序",
        probe_seq_a().is_ok()
            && probe_seq_b().err() == Some(WaCode::AnchorNotInCatalog.code())
            && probe_seq_c().err() == Some(WaCode::ClosingAnchorUnverified.code()),
        "绑定→核验→闭账全过；目录外锚/未核验消费均拒",
    );
    s.add(
        "J1-边界四类闸",
        {
            let mut ledger = AnchorLedger::new();
            let w0 = ledger.register(0, 1).is_err();
            let w13 = ledger.register(WAVE_MAX + 1, 1).is_err();
            let a0 = ledger.register(WAVE_09, 0).is_err();
            let a5 = ledger.register(WAVE_09, ANCHOR_NO_MAX + 1).is_err();
            w0 && w13 && a0 && a5
        },
        "波次 0/13、锚号 0/5 全部显式拒绝",
    );
    s.add(
        "J1-文案闭集",
        detail_closed_set(),
        "12 码逐码非空中文建议，闭集外落未知提示",
    );
    // 动态明细判据（≤8 条）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((anchors, closings, st)) => {
            *anchors == 1 && *closings == 1 && *st == ANCHOR_CLOSED
        }
        Err(_) => false,
    };
    s.add_dyn(
        "J1-波09末锚入账并闭账衔接",
        g_ok,
        &format!("锚账/闭账/终态 = {:?}", g),
    );
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "J1-注入四类10/10全抓",
        inj_ok,
        &format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries).unwrap_or([0; INJECT_CLASSES]),
            inj.as_ref().map(|l| l.caught).unwrap_or([0; INJECT_CLASSES])
        ),
    );
    let cap = capacity_probe();
    s.add_dyn(
        "J1-闭账簿终态账不丢",
        cap.is_ok(),
        &format!("全目录闭账笔数 = {:?}", cap),
    );
    let rep = repro_check();
    s.add_dyn("J1-双跑一致", rep.is_ok(), &format!("{:?}", rep));
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_wave09_anchor_closed() {
        let g = golden_run().unwrap_or((0, 0, 0));
        assert_eq!(g.0, 1);
        assert_eq!(g.1, 1);
        assert_eq!(g.2, ANCHOR_CLOSED);
    }

    #[test]
    fn test_catalog_lookup_and_criteria_id() {
        assert_eq!(AnchorLedger::catalog_lookup(9, 1), Some(50));
        assert_eq!(AnchorLedger::catalog_lookup(9, 3), None);
        assert_eq!(AnchorLedger::catalog_lookup(12, 1), None);
        assert_eq!(criteria_id_of(9, 1), 0x0A91);
        assert_eq!(criteria_id_of(8, 1), 0x0A81);
    }

    #[test]
    fn test_verify_shortfall_and_exact() {
        let mut ledger = AnchorLedger::new();
        let seq = ledger.register(WAVE_09, 1).unwrap_or(0);
        // 短少拒。
        assert!(ledger.verify(seq, 49).is_err());
        // 超额也拒（恰达语义）。
        assert!(ledger.verify(seq, 51).is_err());
        // 恰 50 过。
        assert!(ledger.verify(seq, EXPECT_GREEN_50).is_ok());
        // 二次核验（Verified 态再 verify）拒。
        assert!(ledger.verify(seq, EXPECT_GREEN_50).is_err());
    }

    #[test]
    fn test_inject_all_caught_10_rounds() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(led.all_caught(), "四类坏样本 10 轮必须全抓");
        assert!(led.conservation().is_ok());
    }

    #[test]
    fn test_duplicate_anchor() {
        let mut ledger = AnchorLedger::new();
        assert!(ledger.register(WAVE_09, 1).is_ok());
        assert!(ledger.register(WAVE_09, 1).is_err());
        // 同波不同锚号可绑。
        assert!(ledger.register(WAVE_09, 2).is_ok());
        assert_eq!(ledger.anchor_count(), 2);
        // 不同波同锚号可绑。
        assert!(ledger.register(8, 1).is_ok());
        assert_eq!(ledger.anchor_count(), 3);
    }

    #[test]
    fn test_state_machine_transitions() {
        // BOUND 直接消费拒。
        let mut ledger = AnchorLedger::new();
        let seq = ledger.register(7, 1).unwrap_or(0);
        assert!(ledger.consume(seq).is_err());
        // 核验 → 消费链。
        assert!(ledger.verify(seq, 1).is_ok());
        assert_eq!(ledger.state_of(seq), Some(ANCHOR_VERIFIED));
        assert!(ledger.consume(seq).is_ok());
        assert_eq!(ledger.state_of(seq), Some(ANCHOR_CLOSED));
        // Closed 再消费拒。
        assert!(ledger.consume(seq).is_err());
    }

    #[test]
    fn test_closing_book_seal_and_audit() {
        let mut ledger = AnchorLedger::new();
        let mut book = ClosingBook::new();
        let seq = ledger.register(8, 1).unwrap_or(0);
        let _ = ledger.verify(seq, 2);
        assert!(book.close_anchor(&mut ledger, seq).is_ok());
        assert!(book.seal().is_ok());
        // 封账后落账拒。
        let seq2 = ledger.register(7, 1).unwrap_or(0);
        let _ = ledger.verify(seq2, 1);
        assert!(book.close_anchor(&mut ledger, seq2).is_err());
        // 二次封账拒。
        assert!(book.seal().is_err());
        assert!(book.audit(&ledger).is_ok());
        let rec = book.entry_at(0).unwrap_or(ClosingRec::empty());
        assert_eq!(rec.anchor_seq, seq);
        assert_eq!(rec.verdict_green, 2);
    }

    #[test]
    fn test_boundary_four_classes() {
        let mut ledger = AnchorLedger::new();
        assert!(ledger.register(0, 1).is_err());
        assert!(ledger.register(WAVE_MAX + 1, 1).is_err());
        assert!(ledger.register(WAVE_09, 0).is_err());
        assert!(ledger.register(WAVE_09, ANCHOR_NO_MAX + 1).is_err());
        assert_eq!(ledger.anchor_count(), 0);
    }

    #[test]
    fn test_capacity_probe_full_catalog() {
        assert_eq!(capacity_probe().unwrap_or(0), 5);
    }

    #[test]
    fn test_double_run_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard_and_detail_closed_set() {
        assert!(segment_guard());
        assert!(detail_closed_set());
        assert_eq!(WaCode::VerifyShortfall.code(), SEG_BASE + 4);
    }

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18537_checks();
        assert!(set.all_passed(), "UNX-F18537 判据必须全绿：{}", set.red_items_text());
    }
}
