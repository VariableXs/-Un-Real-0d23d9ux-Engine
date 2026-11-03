//! UNX-F1483 补全执行一（中断面 B01–B10 缺口批补齐） · 0x75A40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（DISPATCH_w7I 逐字锚定，AI-VX7-1104 批4 I 组）：
//!   F1482 缺口清单逐项补齐（中断面 10 批），补全断言入总账只增
//!   不减，补全漏项注入 10/10 必抓。
//! 逐卡要点（分派表逐字锚定）：
//!   缺口清单闭集（中断面 B01~B10 十批），逐项补齐执行（清单→补全→
//!   复验三段），补全断言入总账只增不减（总账只追加），补全漏项注入
//!   （抽清单项跳过）10/10 必抓。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① 缺口清单闭集：中断面 B01~B10 十批（批号+缺口项数冻结表，合计
//!     26 项），清单由冻结表确定性展开（批号/槽位/判据键三列）——
//!     表外项（批 11、槽越界）显性拒（UnknownItem）；
//!   ② 三段单向状态机：每项 Listed→Filled→Verified 单调推进，倒退
//!     显性拒（未填即验 NotFilled、已填再填 DuplicateFill、跳段验
//!     StageRegress）——三段即分派判据「清单→补全→复验」；
//!   ③ 补全断言总账只增：每次补全追加一行断言账（断言号段内连续），
//!     无删除/回滚入口——抹账调用显性拒并计数（LedgerShrink），
//!     总账行数单调不减（补齐后恰 26 行）；
//!   ④ 批级复验：批关闭要求该批全部项 Verified，缺项显性检出
//!     （SkipDetected）——补全漏项（抽清单项跳过）注入必抓，
//!     LCG 固定种子 10 轮 10/10；
//!   ⑤ 面级收口：十批全关 + 总账 26 行 + 三段全 Verified 才出
//!     面级绿（面未收口即绿判定显性拒）；
//!   ⑥ 全确定性：零墙钟零随机零浮点，诊断码 0x40 窗口闭集，
//!     中文诊断文案带可操作建议。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75A40，批4 I 组独占窗口）。
pub const SEG_BASE: u32 = 0x75A40;

/// 中断面批数（冻结 10：B01~B10）。
pub const BATCH_COUNT: usize = 10;
/// 各批缺口项数冻结表（合计 26：判据点名中断面十批逐项补齐）。
pub const GAP_PER_BATCH: [u32; BATCH_COUNT] = [3, 2, 4, 2, 3, 2, 3, 2, 3, 2];
/// 缺口项总数（冻结 26）。
pub const GAP_TOTAL: u32 = 26;
/// 断言号基（段内 +0x10 起，总账断言号连续）。
pub const ASSERT_BASE: u32 = SEG_BASE + 0x10;
/// 判据轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
/// 诊断码闭集：每码带可操作中文建议（detail 文案）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FillCode {
    /// 清单项未知：批号/槽位不在冻结清单内
    UnknownItem,
    /// 阶段倒退：Listed→Filled→Verified 之外方向
    StageRegress,
    /// 重复补全：该项已 Filled
    DuplicateFill,
    /// 未填即验：复验前置缺失
    NotFilled,
    /// 漏项检出：批内存在未 Verified 项
    SkipDetected,
    /// 抹账拒：总账只追加
    LedgerShrink,
    /// 批未知：B01~B10 之外
    BatchUnknown,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl FillCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FillCode::UnknownItem => {
                "清单项未知：中断面缺口清单仅 B01~B10 冻结 26 项，请按清单项补齐，勿自造项"
            }
            FillCode::StageRegress => {
                "阶段倒退拒：Listed→Filled→Verified 单向，请按 清单→补全→复验 序推进"
            }
            FillCode::DuplicateFill => {
                "重复补全拒：该项已补全，请走复验段而非重复补"
            }
            FillCode::NotFilled => {
                "未填即验拒：复验前置是补全段已完成，请先补全再复验"
            }
            FillCode::SkipDetected => {
                "漏项检出：批内存在未复验项，请按缺口清单逐项回补后关批"
            }
            FillCode::LedgerShrink => {
                "抹账拒：补全断言总账只增不减，请开新账册追加更正而非删旧行"
            }
            FillCode::BatchUnknown => {
                "批未知：中断面仅 B01~B10 十批，请按批号闭集操作"
            }
            FillCode::NotReproducible => {
                "双跑不一致：复现红线，请检查是否有墙钟/随机参与取值"
            }
        }
    }
    pub const ALL: [FillCode; 8] = [
        FillCode::UnknownItem,
        FillCode::StageRegress,
        FillCode::DuplicateFill,
        FillCode::NotFilled,
        FillCode::SkipDetected,
        FillCode::LedgerShrink,
        FillCode::BatchUnknown,
        FillCode::NotReproducible,
    ];
}

/// 段内查表闭集返回。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < FillCode::ALL.len() {
        if let Some(c) = FillCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75A40 段闭集内"
}

// ── 域模型：缺口清单闭集 ────────────────────────────────────────
/// 缺口项三段状态（Listed→Filled→Verified 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemStage {
    /// 已列清单未补。
    Listed,
    /// 已补全（断言已入总账）。
    Filled,
    /// 已复验。
    Verified,
}

/// 缺口清单项（批号 1~10 + 槽位 + 判据键）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GapItem {
    /// 批号（1=B01 … 10=B10）。
    pub batch: u8,
    /// 批内槽位（0 起）。
    pub slot: u32,
    /// 判据键（中断面词汇表派生，含批号前缀）。
    pub key: String,
}

/// 中断面词汇表（中断域冻结键面：B01~B10 缺口判据键池）。
pub const IRQ_VOCAB: [&str; 8] = [
    "IRQ-SHARE",
    "IRQ-AFFIN",
    "MSI-EN",
    "MSIX-EN",
    "IRQ-MASK",
    "IRQ-EOI",
    "IRQ-SPUR",
    "INTACK",
];

/// 批号名（1→"B01" … 10→"B10"）。
pub fn batch_name(b: u8) -> Result<&'static str, u32> {
    match b {
        1 => Ok("B01"),
        2 => Ok("B02"),
        3 => Ok("B03"),
        4 => Ok("B04"),
        5 => Ok("B05"),
        6 => Ok("B06"),
        7 => Ok("B07"),
        8 => Ok("B08"),
        9 => Ok("B09"),
        10 => Ok("B10"),
        _ => Err(FillCode::BatchUnknown.val()),
    }
}

/// 冻结缺口清单展开（十批 26 项，批内槽位升序，键=批名-词汇键）。
pub fn gap_list() -> Vec<GapItem> {
    let mut v: Vec<GapItem> = Vec::new();
    let mut b = 1u8;
    while (b as usize) <= BATCH_COUNT {
        let n = match GAP_PER_BATCH.get((b as usize).saturating_sub(1)) {
            Some(n) => *n,
            None => 0,
        };
        let bn = match batch_name(b) {
            Ok(s) => s,
            Err(_) => "B??",
        };
        let mut slot = 0u32;
        while slot < n {
            let word = match IRQ_VOCAB.get((slot as usize) % IRQ_VOCAB.len()) {
                Some(w) => *w,
                None => "IRQ-?",
            };
            v.push(GapItem {
                batch: b,
                slot,
                key: format!("{}-{}", bn, word),
            });
            slot += 1;
        }
        b += 1;
    }
    v
}

/// 清单闭集核：项数 26、批号 1~10、批内项数与冻结表一致、键非空。
pub fn gap_list_ok(items: &[GapItem]) -> Result<(), u32> {
    if items.len() != GAP_TOTAL as usize {
        return Err(FillCode::UnknownItem.val());
    }
    let mut b = 1u8;
    while (b as usize) <= BATCH_COUNT {
        let want = match GAP_PER_BATCH.get((b as usize).saturating_sub(1)) {
            Some(n) => *n as usize,
            None => 0,
        };
        let mut got = 0usize;
        let mut i = 0usize;
        while i < items.len() {
            if let Some(it) = items.get(i) {
                if it.batch == b {
                    if it.key.is_empty() {
                        return Err(FillCode::UnknownItem.val());
                    }
                    if it.slot >= want as u32 {
                        return Err(FillCode::UnknownItem.val());
                    }
                    got += 1;
                }
            }
            i += 1;
        }
        if got != want {
            return Err(FillCode::UnknownItem.val());
        }
        b += 1;
    }
    Ok(())
}

/// 清单项定位（批号+槽位 → 全局序号；表外 None → UnknownItem）。
pub fn item_index(items: &[GapItem], batch: u8, slot: u32) -> Result<usize, u32> {
    if batch == 0 || batch as usize > BATCH_COUNT {
        return Err(FillCode::BatchUnknown.val());
    }
    let want = match GAP_PER_BATCH.get((batch as usize).saturating_sub(1)) {
        Some(n) => *n,
        None => 0,
    };
    if slot >= want {
        return Err(FillCode::UnknownItem.val());
    }
    let mut i = 0usize;
    while i < items.len() {
        if let Some(it) = items.get(i) {
            if it.batch == batch && it.slot == slot {
                return Ok(i);
            }
        }
        i += 1;
    }
    Err(FillCode::UnknownItem.val())
}

// ── 补全总账（只追加） ──────────────────────────────────────────
/// 总账行（补全断言）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerLine {
    /// 断言号（段内连续）。
    pub aid: u32,
    /// 所属批号。
    pub batch: u8,
    /// 批内槽位。
    pub slot: u32,
    /// 判据键。
    pub key: String,
}

/// 补全状态机（Open→Filling→Closed 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookState {
    /// 开面：清单已列。
    Open,
    /// 补全执行中。
    Filling,
    /// 面已收口。
    Closed,
}

/// 补全执行面总成。
#[derive(Clone, Debug)]
pub struct FillBook {
    pub items: Vec<GapItem>,
    pub stages: Vec<ItemStage>,
    pub ledger: Vec<LedgerLine>,
    pub state: BookState,
    /// 抹账拦截计数（生产路径恒 0）。
    pub shrink_blocked: u32,
    /// 已关闭批数。
    pub closed_batches: u32,
}

impl Default for FillBook {
    fn default() -> FillBook {
        FillBook {
            items: gap_list(),
            stages: Vec::new(),
            ledger: Vec::new(),
            state: BookState::Open,
            shrink_blocked: 0,
            closed_batches: 0,
        }
    }
}

impl FillBook {
    pub fn new() -> FillBook {
        let mut b = FillBook::default();
        let mut i = 0usize;
        while i < b.items.len() {
            b.stages.push(ItemStage::Listed);
            i += 1;
        }
        b
    }

    /// 补全一项（Listed→Filled，断言入总账；重复补/表外项/阶段拒）。
    pub fn fill(&mut self, batch: u8, slot: u32) -> Result<u32, u32> {
        if self.state == BookState::Closed {
            return Err(FillCode::StageRegress.val());
        }
        if self.state == BookState::Open {
            self.state = BookState::Filling;
        }
        let idx = item_index(&self.items, batch, slot)?;
        let stage = match self.stages.get(idx) {
            Some(s) => *s,
            None => return Err(FillCode::UnknownItem.val()),
        };
        if stage != ItemStage::Listed {
            return Err(FillCode::DuplicateFill.val());
        }
        let key = match self.items.get(idx) {
            Some(it) => it.key.clone(),
            None => return Err(FillCode::UnknownItem.val()),
        };
        let aid = ASSERT_BASE + self.ledger.len() as u32;
        self.ledger.push(LedgerLine { aid, batch, slot, key });
        if let Some(s) = self.stages.get_mut(idx) {
            *s = ItemStage::Filled;
        }
        Ok(aid)
    }

    /// 复验一项（Filled→Verified；未填即验拒）。
    pub fn verify(&mut self, batch: u8, slot: u32) -> Result<(), u32> {
        let idx = item_index(&self.items, batch, slot)?;
        let stage = match self.stages.get(idx) {
            Some(s) => *s,
            None => return Err(FillCode::UnknownItem.val()),
        };
        match stage {
            ItemStage::Listed => Err(FillCode::NotFilled.val()),
            ItemStage::Filled => {
                if let Some(s) = self.stages.get_mut(idx) {
                    *s = ItemStage::Verified;
                }
                Ok(())
            }
            ItemStage::Verified => Err(FillCode::StageRegress.val()),
        }
    }

    /// 关批（批内全部 Verified 才可关，缺项显性检出）。
    pub fn close_batch(&mut self, batch: u8) -> Result<usize, u32> {
        if batch == 0 || batch as usize > BATCH_COUNT {
            return Err(FillCode::BatchUnknown.val());
        }
        let mut verified = 0usize;
        let mut total = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(it) = self.items.get(i) {
                if it.batch == batch {
                    total += 1;
                    match self.stages.get(i) {
                        Some(ItemStage::Verified) => verified += 1,
                        _ => {}
                    }
                }
            }
            i += 1;
        }
        if verified != total {
            return Err(FillCode::SkipDetected.val());
        }
        self.closed_batches = self.closed_batches.saturating_add(1);
        if self.closed_batches == BATCH_COUNT as u32 {
            self.state = BookState::Closed;
        }
        Ok(total)
    }

    /// 抹账显性拒（总账只追加；拦截计数入账）。
    pub fn erase_line(&mut self, _idx: usize) -> Result<(), u32> {
        self.shrink_blocked = self.shrink_blocked.saturating_add(1);
        Err(FillCode::LedgerShrink.val())
    }

    /// 面级收口核：十批全关+总账 26 行+三段全 Verified。
    pub fn face_sealed_ok(&self) -> Result<(), u32> {
        if self.state != BookState::Closed {
            return Err(FillCode::StageRegress.val());
        }
        if self.ledger.len() != GAP_TOTAL as usize {
            return Err(FillCode::SkipDetected.val());
        }
        let mut i = 0usize;
        while i < self.stages.len() {
            match self.stages.get(i) {
                Some(ItemStage::Verified) => {}
                _ => return Err(FillCode::SkipDetected.val()),
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 补全全链：清单→逐项补→逐项验→十批关→面收口 ─────────────────
pub fn fill_run() -> Result<(usize, usize, u32, u32), u32> {
    let mut book = FillBook::new();
    gap_list_ok(&book.items)?;
    // 逐项补全（清单序）。
    let mut i = 0usize;
    while i < book.items.len() {
        let (b, s) = match book.items.get(i) {
            Some(it) => (it.batch, it.slot),
            None => return Err(FillCode::UnknownItem.val()),
        };
        book.fill(b, s)?;
        i += 1;
    }
    let filled = book.ledger.len();
    // 逐项复验。
    let mut i = 0usize;
    while i < book.items.len() {
        let (b, s) = match book.items.get(i) {
            Some(it) => (it.batch, it.slot),
            None => return Err(FillCode::UnknownItem.val()),
        };
        book.verify(b, s)?;
        i += 1;
    }
    // 十批关闭。
    let mut b = 1u8;
    while (b as usize) <= BATCH_COUNT {
        book.close_batch(b)?;
        b += 1;
    }
    book.face_sealed_ok()?;
    Ok((filled, book.ledger.len(), book.closed_batches, book.shrink_blocked))
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 漏项注入单轮：抽全局第 skip_idx 项跳过，其余补齐验齐后逐批关，
/// 跳过项所在批必须显性检出（SkipDetected）。
pub fn skip_inject_once(skip_idx: usize) -> Result<bool, u32> {
    let items = gap_list();
    if skip_idx >= items.len() {
        return Err(FillCode::UnknownItem.val());
    }
    let mut book = FillBook::new();
    let skip = match items.get(skip_idx) {
        Some(it) => (it.batch, it.slot),
        None => return Err(FillCode::UnknownItem.val()),
    };
    let mut i = 0usize;
    while i < items.len() {
        if let Some(it) = items.get(i) {
            if (it.batch, it.slot) != skip {
                book.fill(it.batch, it.slot)?;
                book.verify(it.batch, it.slot)?;
            }
        }
        i += 1;
    }
    // 逐批关：跳过项所在批必须检出，其余批正常关。
    let mut detected = false;
    let mut b = 1u8;
    while (b as usize) <= BATCH_COUNT {
        match book.close_batch(b) {
            Ok(_) => {}
            Err(e) if e == FillCode::SkipDetected.val() => {
                if b == skip.0 {
                    detected = true;
                } else {
                    return Ok(false);
                }
            }
            Err(_) => return Ok(false),
        }
        b += 1;
    }
    Ok(detected)
}

/// 漏项注入 10 轮（LCG 驱动抽取位）。
pub fn skip_inject_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut tries = 0u32;
    let mut caught = 0u32;
    let mut r = 0;
    while r < rounds {
        let pick = rng.below(GAP_TOTAL as u64) as usize;
        if skip_inject_once(pick)? {
            caught += 1;
        }
        tries += 1;
        r += 1;
    }
    Ok((tries, caught))
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = fill_run()?;
    let b = fill_run()?;
    if a != b {
        return Err(FillCode::NotReproducible.val());
    }
    let r1 = skip_inject_rounds(JUDGE_ROUNDS)?;
    let r2 = skip_inject_rounds(JUDGE_ROUNDS)?;
    if r1 != r2 {
        return Err(FillCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FillCode::ALL.len() {
        if let Some(c) = FillCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：补全全链 26/26/10/0。
pub fn probe_seq_a() -> Result<(), u32> {
    match fill_run() {
        Ok((filled, ledger, batches, shrink))
            if filled == GAP_TOTAL as usize && ledger == GAP_TOTAL as usize
                && batches == BATCH_COUNT as u32 && shrink == 0 =>
        {
            Ok(())
        }
        _ => Err(FillCode::NotReproducible.val()),
    }
}

/// 序 B：抹账拒+未填即验拒+重复补拒+表外项拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut book = FillBook::new();
    // 未填即验。
    if book.verify(1, 0) != Err(FillCode::NotFilled.val()) {
        return Err(FillCode::NotReproducible.val());
    }
    book.fill(1, 0)?;
    // 重复补。
    if book.fill(1, 0) != Err(FillCode::DuplicateFill.val()) {
        return Err(FillCode::NotReproducible.val());
    }
    book.verify(1, 0)?;
    // 已验再验（倒退）。
    if book.verify(1, 0) != Err(FillCode::StageRegress.val()) {
        return Err(FillCode::NotReproducible.val());
    }
    // 抹账拒。
    if book.erase_line(0) != Err(FillCode::LedgerShrink.val()) || book.shrink_blocked != 1 {
        return Err(FillCode::NotReproducible.val());
    }
    // 表外项：批 11 与槽越界。
    if book.fill(11, 0) != Err(FillCode::BatchUnknown.val()) {
        return Err(FillCode::NotReproducible.val());
    }
    if book.fill(1, 99) != Err(FillCode::UnknownItem.val()) {
        return Err(FillCode::NotReproducible.val());
    }
    // 批未齐关批检出。
    if book.close_batch(1) != Err(FillCode::SkipDetected.val()) {
        return Err(FillCode::NotReproducible.val());
    }
    Ok(())
}

/// 序 C：冻结常量与段位守卫。
pub fn probe_seq_c() -> Result<(), u32> {
    let zone_ok = SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x75000 && SEG_BASE <= 0x781C0;
    let sum = {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < GAP_PER_BATCH.len() {
            if let Some(n) = GAP_PER_BATCH.get(i) {
                s += *n;
            }
            i += 1;
        }
        s
    };
    match (BATCH_COUNT == 10, sum == GAP_TOTAL, GAP_TOTAL == 26, zone_ok, segment_guard()) {
        (true, true, true, true, true) => Ok(()),
        _ => Err(FillCode::NotReproducible.val()),
    }
}

// ── 判据面（私有谓词，cond 为违约条件） ─────────────────────────
fn gap_list_closed_ok() -> bool {
    let items = gap_list();
    if gap_list_ok(&items).is_err() {
        return false;
    }
    // 首项 B01 槽 0、末项 B10 槽 1 显性抽验。
    match (items.first(), items.last()) {
        (Some(f), Some(l)) => f.batch == 1 && f.slot == 0 && l.batch == 10 && l.slot == 1,
        _ => false,
    }
}

fn fill_verify_all_ok() -> bool {
    match fill_run() {
        Ok((filled, ledger, batches, shrink)) => {
            filled == GAP_TOTAL as usize
                && ledger == GAP_TOTAL as usize
                && batches == BATCH_COUNT as u32
                && shrink == 0
        }
        Err(_) => false,
    }
}

fn ledger_append_only_ok() -> bool {
    let mut book = FillBook::new();
    if book.fill(1, 0).is_err() {
        return false;
    }
    let n1 = book.ledger.len();
    if book.fill(1, 1).is_err() {
        return false;
    }
    let n2 = book.ledger.len();
    if n2 != n1 + 1 {
        return false;
    }
    // 断言号段内连续。
    match (book.ledger.first(), book.ledger.last()) {
        (Some(f), Some(l)) => {
            if f.aid != ASSERT_BASE || l.aid != ASSERT_BASE + 1 {
                return false;
            }
        }
        _ => return false,
    }
    if book.erase_line(0).is_ok() {
        return false;
    }
    if book.ledger.len() != n2 || book.shrink_blocked != 1 {
        return false;
    }
    true
}

fn stage_machine_ok() -> bool {
    let mut book = FillBook::new();
    // 未填即验拒。
    if book.verify(2, 0) != Err(FillCode::NotFilled.val()) {
        return false;
    }
    if book.fill(2, 0).is_err() {
        return false;
    }
    // 重复补拒。
    if book.fill(2, 0) != Err(FillCode::DuplicateFill.val()) {
        return false;
    }
    // 补后验绿。
    if book.verify(2, 0).is_err() {
        return false;
    }
    // 已验再验拒。
    if book.verify(2, 0) != Err(FillCode::StageRegress.val()) {
        return false;
    }
    true
}

fn skip_inject_10_10_ok() -> bool {
    match skip_inject_rounds(JUDGE_ROUNDS) {
        Ok((t, c)) => t == JUDGE_ROUNDS && c == t,
        Err(_) => false,
    }
}

fn skip_batch_located_ok() -> bool {
    // 抽 B03（全局第 3 批首项）跳过：B03 关批必检出，前面批正常关。
    let items = gap_list();
    let mut idx_b03 = usize::MAX;
    let mut i = 0usize;
    while i < items.len() {
        if let Some(it) = items.get(i) {
            if it.batch == 3 && it.slot == 0 {
                idx_b03 = i;
            }
        }
        i += 1;
    }
    if idx_b03 == usize::MAX {
        return false;
    }
    match skip_inject_once(idx_b03) {
        Ok(true) => true,
        _ => false,
    }
}

fn item_bounds_ok() -> bool {
    let items = gap_list();
    // 批 2 恒 2 项：槽 2 越界显性拒。
    if item_index(&items, 2, 2) != Err(FillCode::UnknownItem.val()) {
        return false;
    }
    // 批 0 / 批 11 显性拒。
    if item_index(&items, 0, 0) != Err(FillCode::BatchUnknown.val()) {
        return false;
    }
    item_index(&items, 11, 0) == Err(FillCode::BatchUnknown.val())
}

fn aid_contiguous_ok() -> bool {
    // 全链总账 26 行断言号段内连续（第 i 行恰为 ASSERT_BASE+i）。
    let mut book = FillBook::new();
    let items = gap_list();
    let mut i = 0usize;
    while i < items.len() {
        if let Some(it) = items.get(i) {
            if book.fill(it.batch, it.slot).is_err() {
                return false;
            }
        }
        i += 1;
    }
    if book.ledger.len() != GAP_TOTAL as usize {
        return false;
    }
    let mut k = 0usize;
    while k < book.ledger.len() {
        match book.ledger.get(k) {
            Some(l) => {
                if l.aid != ASSERT_BASE + k as u32 {
                    return false;
                }
            }
            None => return false,
        }
        k += 1;
    }
    // 尾号仍在段窗口内。
    let last = ASSERT_BASE + GAP_TOTAL - 1;
    last >= SEG_BASE && last < SEG_BASE + 0x40
}

fn detail_closed_ok() -> bool {
    let mut i = 0;
    while i < FillCode::ALL.len() {
        if let Some(c) = FillCode::ALL.get(i) {
            if detail(c.val()).is_empty() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).contains("未知")
}

// ── 检查节 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-缺口清单闭集",
        gap_list_closed_ok(),
        String::from("B01~B10 十批 26 项 冻结表展开"),
    ));
    v.push((
        "J1-逐项补齐复验",
        fill_verify_all_ok(),
        String::from("清单→补全→复验 三段 26/26"),
    ));
    v.push((
        "J1-总账只增不减",
        ledger_append_only_ok(),
        String::from("总账追加+断言号连续+抹账拒"),
    ));
    v.push((
        "J2-三段状态机",
        stage_machine_ok(),
        String::from("未填即验拒/重复补拒/已验再验拒"),
    ));
    let inj = skip_inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J2-漏项注入10/10",
        skip_inject_10_10_ok(),
        format!(
            "t/c {}/{}",
            inj.as_ref().map(|(t, _)| *t).unwrap_or(0),
            inj.as_ref().map(|(_, c)| *c).unwrap_or(0)
        ),
    ));
    v.push((
        "J2-漏项批定位",
        skip_batch_located_ok(),
        String::from("抽 B03 跳过→B03 关批显性检出"),
    ));
    v.push(("J3-双跑一致", repro_check().is_ok(), String::from("fill_run+注入轮 两跑同值")));
    v.push((
        "J1-清单表外项拒",
        item_bounds_ok(),
        String::from("槽越界 UnknownItem / 批外 BatchUnknown"),
    ));
    v.push((
        "J1-断言号连续",
        aid_contiguous_ok(),
        String::from("总账 26 行段内连续编号"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x75A40 窗口")));
    v.push(("J3-诊断码闭集", detail_closed_ok(), String::from("查表闭集返回")));
    v.push((
        "J3-探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        String::from("A/B/C 三序全绿"),
    ));
    v
}

pub fn run_unx1483_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1483 补全执行一");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_list_frozen_shape() {
        let items = gap_list();
        assert_eq!(items.len(), 26);
        assert!(gap_list_ok(&items).is_ok());
        // 每批项数与冻结表一致。
        let mut b = 1u8;
        while (b as usize) <= BATCH_COUNT {
            let want = GAP_PER_BATCH[(b - 1) as usize] as usize;
            let got = items.iter().filter(|it| it.batch == b).count();
            assert_eq!(got, want, "批 {} 缺口项数", b);
            b += 1;
        }
    }

    #[test]
    fn item_index_bounds() {
        let items = gap_list();
        assert!(item_index(&items, 1, 0).is_ok());
        assert!(item_index(&items, 10, 1).is_ok());
        assert_eq!(item_index(&items, 11, 0), Err(FillCode::BatchUnknown.val()));
        assert_eq!(item_index(&items, 0, 0), Err(FillCode::BatchUnknown.val()));
        assert_eq!(item_index(&items, 2, 2), Err(FillCode::UnknownItem.val()));
        // 批 2 恒 2 项：槽 2 越界。
        assert_eq!(GAP_PER_BATCH[1], 2);
    }

    #[test]
    fn fill_verify_close_all() {
        let (filled, ledger, batches, shrink) = fill_run().expect("补全全链");
        assert_eq!((filled, ledger, batches, shrink), (26, 26, 10, 0));
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn stage_machine_gates() {
        let mut book = FillBook::new();
        assert_eq!(book.verify(1, 0), Err(FillCode::NotFilled.val()));
        let aid = book.fill(1, 0).expect("补全");
        assert_eq!(aid, ASSERT_BASE);
        assert_eq!(book.fill(1, 0), Err(FillCode::DuplicateFill.val()));
        assert!(book.verify(1, 0).is_ok());
        assert_eq!(book.verify(1, 0), Err(FillCode::StageRegress.val()));
    }

    #[test]
    fn ledger_only_grows() {
        let mut book = FillBook::new();
        book.fill(1, 0).expect("补");
        book.fill(1, 1).expect("补");
        let n = book.ledger.len();
        assert_eq!(book.erase_line(0), Err(FillCode::LedgerShrink.val()));
        assert_eq!(book.ledger.len(), n);
        assert_eq!(book.shrink_blocked, 1);
        // 断言号连续。
        let mut i = 0usize;
        while i < book.ledger.len() {
            if let Some(l) = book.ledger.get(i) {
                assert_eq!(l.aid, ASSERT_BASE + i as u32);
            }
            i += 1;
        }
    }

    #[test]
    fn close_batch_detects_skip() {
        let mut book = FillBook::new();
        // B01 3 项只补 2 项（跳槽位 1）。
        book.fill(1, 0).expect("补");
        book.fill(1, 2).expect("补");
        book.verify(1, 0).expect("验");
        book.verify(1, 2).expect("验");
        assert_eq!(book.close_batch(1), Err(FillCode::SkipDetected.val()));
        // 补齐后关批绿。
        book.fill(1, 1).expect("补");
        book.verify(1, 1).expect("验");
        assert_eq!(book.close_batch(1).expect("关批"), 3);
    }

    #[test]
    fn skip_injection_all_rounds() {
        // 26 项逐项抽取注入全抓。
        let mut i = 0usize;
        while i < 26 {
            assert!(skip_inject_once(i).expect("注入核"), "抽第 {} 项必须被抓", i);
            i += 1;
        }
        let (t, c) = skip_inject_rounds(JUDGE_ROUNDS).expect("注入轮");
        assert_eq!(t, JUDGE_ROUNDS);
        assert_eq!(c, t);
    }

    #[test]
    fn repro_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(detail_closed_ok());
    }
}
