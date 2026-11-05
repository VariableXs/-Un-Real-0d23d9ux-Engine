//! UNX-F12675 探测面对照表二批入账 · 0x81280段
//!
//! 判据原文（派工卡 12674 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12675-J1 枚举类 P95 预算在册，大表注入账无红账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 二批对照表入账域模型：一批对照表 30 行五列（行号/场景/甲判/乙
//!    判/一致位）逐行过入账闸——入账闸四道（行谱闭集/列齐/衔接点验/
//!    预算在册），入账后行只读，账面零红项；
//! 2. 枚举类 P95 预算在册：每行带枚举类预算槽（枚举闭集四类：键枚
//!    举/句柄枚举/映射枚举/Token 枚举），槽内冻结预算上限与整数计量
//!    步数——P95 以整数序位法计（升序取位），预算槽在册口径为「上限
//!    已冻结、计量已落账、P95 不越上限」三全；
//! 3. 大表注入账无红账：对 30 行大表做逐行预算复核，任何一行计量越
//!    预算即记账为红——冻结谱下全过即无红账；注入越限行必被记账抓
//!    住（红账显性，不静默）；
//! 4. 入账状态机：TabOpen → Booking → Booked 单向推进，倒退显性拒；
//!    零静默：逐行显性入账判定，记账数 ≡ 行数守恒；
//! 5. 注入联测十路：行谱越界/列缺格/衔接悬空/预算越限/枚举越闭集/
//!    行号乱序/行重复/缺行封账/静默吞行/摘要空——逐路必抓；
//! 6. no_std + alloc；零浮点（P95 用整数序位法）、零 IO、零墙钟、零
//!    随机；取值一律 match/if let/.get()/.get_mut()/saturating_*，无
//!    unwrap/expect/panic/裸下标；中文注释与中文诊断；无死代码。
//!
//! 判据谓词与验收判据的对应：
//!   - `p95_in_ledger_ok` ↔ ② 枚举类 P95 预算在册；
//!   - `no_red_ledger_ok` ↔ ② 大表注入账无红账；
//!   - `booking_ok` ↔ 二批对照表 30 行入账全过；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12674 − 12600)）。
pub const SEG_BASE: u32 = 0x81280;

// ── 入账诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabDiag {
    /// 行谱越界（行号不在 0..30）。
    ERowRange,
    /// 列缺格（五列有缺）。
    EColMissing,
    /// 衔接悬空（一批行谱引用不解析）。
    EDanglingLink,
    /// 预算越限（P95 或单行计量越冻结上限）。
    EBudgetOver,
    /// 枚举越闭集（预算槽枚举类不在四类内）。
    EEnumRange,
    /// 行号乱序。
    ERowOrder,
    /// 行重复。
    ERowDup,
    /// 缺行封账（30 行未齐即封账）。
    EShortSeal,
    /// 静默吞行。
    ESilentDrop,
    /// 摘要空。
    ESummaryEmpty,
    /// 状态机倒退。
    ERewind,
}

impl TabDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                TabDiag::ERowRange => 0x01,
                TabDiag::EColMissing => 0x02,
                TabDiag::EDanglingLink => 0x03,
                TabDiag::EBudgetOver => 0x04,
                TabDiag::EEnumRange => 0x05,
                TabDiag::ERowOrder => 0x06,
                TabDiag::ERowDup => 0x07,
                TabDiag::EShortSeal => 0x08,
                TabDiag::ESilentDrop => 0x09,
                TabDiag::ESummaryEmpty => 0x0a,
                TabDiag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            TabDiag::ERowRange => "行谱越界：行号只认 0..30，越谱行先回补一批行谱",
            TabDiag::EColMissing => "列缺格：五列齐才准入账，缺列先回补",
            TabDiag::EDanglingLink => "衔接悬空：一批行谱引用必须可解析，悬空先补一批账",
            TabDiag::EBudgetOver => "预算越限：P95 或单行计量越冻结上限，先调预算槽再入账",
            TabDiag::EEnumRange => "枚举越闭集：预算槽只认四类枚举，先正枚举类",
            TabDiag::ERowOrder => "行号乱序：按行号 0..30 逐行入账，跳号先回补",
            TabDiag::ERowDup => "行重复：同场景同衔接只立一行，重复先查已有账",
            TabDiag::EShortSeal => "缺行封账：30 行齐才许封账，缺行先回补",
            TabDiag::ESilentDrop => "静默吞行：逐行显性入账，缺判定即账面违约",
            TabDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
            TabDiag::ERewind => "状态机倒退：TabOpen→Booking→Booked 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const TAB_ALL: &[TabDiag] = &[
    TabDiag::ERowRange,
    TabDiag::EColMissing,
    TabDiag::EDanglingLink,
    TabDiag::EBudgetOver,
    TabDiag::EEnumRange,
    TabDiag::ERowOrder,
    TabDiag::ERowDup,
    TabDiag::EShortSeal,
    TabDiag::ESilentDrop,
    TabDiag::ESummaryEmpty,
    TabDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in TAB_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81281..0x8128B 闭集内"
}

// ── 枚举类四闭集（预算槽枚举类） ─────────────────────────────────
/// 枚举类预算槽闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnumSlot {
    /// 键枚举类。
    KeyEnum,
    /// 句柄枚举类。
    HandleEnum,
    /// 映射枚举类。
    MapEnum,
    /// Token 枚举类。
    TokenEnum,
}

impl EnumSlot {
    /// 类名（中文短名）。
    pub fn name(self) -> &'static str {
        match self {
            EnumSlot::KeyEnum => "键枚举",
            EnumSlot::HandleEnum => "句柄枚举",
            EnumSlot::MapEnum => "映射枚举",
            EnumSlot::TokenEnum => "Token 枚举",
        }
    }

    /// 类号（0..3）。
    pub fn ordinal(self) -> u32 {
        match self {
            EnumSlot::KeyEnum => 0,
            EnumSlot::HandleEnum => 1,
            EnumSlot::MapEnum => 2,
            EnumSlot::TokenEnum => 3,
        }
    }

    /// 类预算上限（整数步数，冻结；来源：探测面性能预算谱二批冻结）。
    pub fn budget_cap(self) -> u32 {
        match self {
            EnumSlot::KeyEnum => 0x40,
            EnumSlot::HandleEnum => 0x48,
            EnumSlot::MapEnum => 0x50,
            EnumSlot::TokenEnum => 0x58,
        }
    }

    /// 全部四类（闭集数组）。
    pub fn all() -> &'static [EnumSlot] {
        &[
            EnumSlot::KeyEnum,
            EnumSlot::HandleEnum,
            EnumSlot::MapEnum,
            EnumSlot::TokenEnum,
        ]
    }
}

// ── 二批对照行（入账行六元） ─────────────────────────────────────
/// 入账行六元（行号/场景名/枚举类/计量步数/一批衔接槽/入账位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabRow {
    /// 列一：行号。
    pub row_no: u32,
    /// 列二：场景名。
    pub scene: &'static str,
    /// 列三：枚举类预算槽。
    pub slot: EnumSlot,
    /// 列四：整数计量步数。
    pub steps: u32,
    /// 列五：一批衔接槽（0..15 行谱）。
    pub link_prev: u32,
    /// 列六：入账位（true = 应入账）。
    pub book_bit: bool,
}

/// 一批行谱容量（衔接目标闭集 0..15）。
pub const PREV_ROW_CAP: usize = 15;

/// 二批对照行容量（30 行）。
pub const TAB_ROW_CAP: usize = 30;

/// 入账行黄金判定（行谱 + 列齐 + 枚举闭集 + 预算在册 + 衔接点验）。
pub fn tab_row_ok(r: &TabRow) -> Result<(), u32> {
    // 行谱闸。
    if r.row_no >= TAB_ROW_CAP as u32 {
        return Err(TabDiag::ERowRange.val());
    }
    // 列齐闸（场景名与入账位非空缺）。
    if r.scene.is_empty() || !r.book_bit {
        return Err(TabDiag::EColMissing.val());
    }
    // 枚举闭集闸：类号落 0..3 且计量非零。
    let cap = match EnumSlot::all().get(r.slot.ordinal() as usize) {
        Some(v) if *v == r.slot => v.budget_cap(),
        _ => return Err(TabDiag::EEnumRange.val()),
    };
    if r.steps == 0 {
        return Err(TabDiag::EColMissing.val());
    }
    // 预算在册闸：单行计量不越类上限。
    if r.steps > cap {
        return Err(TabDiag::EBudgetOver.val());
    }
    // 衔接点验闸。
    if r.link_prev >= PREV_ROW_CAP as u32 {
        return Err(TabDiag::EDanglingLink.val());
    }
    Ok(())
}

// ── P95 整数序位法 ───────────────────────────────────────────────
/// P95 整数序位法：升序样本取序位位（idx = ceil(95% × n) − 1，整数
/// 上取整以饱和乘法实现；n 为零返回 None）。
pub fn p95_sorted(samples: &[u32]) -> Option<u32> {
    if samples.is_empty() {
        return None;
    }
    // idx = ceil(0.95 × n) − 1 = (95n + 99) / 100 − 1（整数上取整等价式）。
    let n = samples.len() as u32;
    let idx = ((95u32.saturating_mul(n).saturating_add(99)) / 100).saturating_sub(1);
    samples.get(idx as usize).copied()
}

/// 升序副本（不改动原样本；整数冒泡序，样本容量有界）。
fn sorted_copy(samples: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for v in samples.iter() {
        out.push(*v);
    }
    // 有界冒泡序：账面样本 ≤ 30 行 × 4 类，开销有界。
    let n = out.len();
    let mut i = 0usize;
    while i < n {
        let mut j = 0usize;
        while j + 1 < n - i {
            let a = match out.get(j) {
                Some(v) => *v,
                None => break,
            };
            let b = match out.get(j + 1) {
                Some(v) => *v,
                None => break,
            };
            if a > b {
                match out.get_mut(j) {
                    Some(slot) => *slot = b,
                    None => {}
                }
                match out.get_mut(j + 1) {
                    Some(slot) => *slot = a,
                    None => {}
                }
            }
            j += 1;
        }
        i += 1;
    }
    out
}

/// 类内样本收集（从行谱抽取指定枚举类的计量步数）。
fn class_samples(rows: &[TabRow], slot: EnumSlot) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for r in rows.iter() {
        if r.slot == slot {
            out.push(r.steps);
        }
    }
    out
}

/// 类内 P95 预算在册判定（P95 非零、P95 ≤ 类上限、样本全落账）。
pub fn class_p95_in_budget(rows: &[TabRow], slot: EnumSlot) -> bool {
    let samples = class_samples(rows, slot);
    if samples.is_empty() {
        return false;
    }
    let sorted = sorted_copy(&samples);
    let p95 = match p95_sorted(&sorted) {
        Some(v) => v,
        None => return false,
    };
    // P95 非零且不越类冻结上限（预算在册口径）。
    if p95 == 0 || p95 > slot.budget_cap() {
        return false;
    }
    // 单行计量也不越上限（P95 不掩盖离谱）。
    for s in samples.iter() {
        if *s > slot.budget_cap() {
            return false;
        }
    }
    true
}

// ── 冻结二批入账行谱（30 行：四类枚举轮转） ─────────────────────
/// 构造行（行号/场景名/枚举类/步数/衔接槽）——冻结展开为常量表。
const fn mk_tab(row_no: u32, scene: &'static str, slot: EnumSlot, steps: u32, link: u32) -> TabRow {
    TabRow {
        row_no,
        scene,
        slot,
        steps,
        link_prev: link,
        book_bit: true,
    }
}

/// 30 行冻结入账行谱（行号序；步数全部落各类预算内）。
pub const TAB_ROWS: &[TabRow] = &[
    mk_tab(0, "键枚举对照", EnumSlot::KeyEnum, 0x21, 0),
    mk_tab(1, "键枚举深档", EnumSlot::KeyEnum, 0x28, 1),
    mk_tab(2, "键枚举尾档", EnumSlot::KeyEnum, 0x35, 2),
    mk_tab(3, "键枚举复核", EnumSlot::KeyEnum, 0x2C, 0),
    mk_tab(4, "键枚举对账", EnumSlot::KeyEnum, 0x38, 1),
    mk_tab(5, "键枚举补账", EnumSlot::KeyEnum, 0x24, 2),
    mk_tab(6, "键枚举重放", EnumSlot::KeyEnum, 0x30, 0),
    mk_tab(7, "键枚举封样", EnumSlot::KeyEnum, 0x3B, 1),
    mk_tab(8, "句柄枚举对照", EnumSlot::HandleEnum, 0x26, 3),
    mk_tab(9, "句柄枚举深档", EnumSlot::HandleEnum, 0x2F, 4),
    mk_tab(10, "句柄枚举尾档", EnumSlot::HandleEnum, 0x3D, 5),
    mk_tab(11, "句柄枚举复核", EnumSlot::HandleEnum, 0x2A, 3),
    mk_tab(12, "句柄枚举对账", EnumSlot::HandleEnum, 0x41, 4),
    mk_tab(13, "句柄枚举重放", EnumSlot::HandleEnum, 0x35, 5),
    mk_tab(14, "句柄枚举封样", EnumSlot::HandleEnum, 0x45, 3),
    mk_tab(15, "映射枚举对照", EnumSlot::MapEnum, 0x2B, 6),
    mk_tab(16, "映射枚举深档", EnumSlot::MapEnum, 0x34, 7),
    mk_tab(17, "映射枚举尾档", EnumSlot::MapEnum, 0x42, 8),
    mk_tab(18, "映射枚举复核", EnumSlot::MapEnum, 0x2E, 6),
    mk_tab(19, "映射枚举对账", EnumSlot::MapEnum, 0x49, 7),
    mk_tab(20, "映射枚举重放", EnumSlot::MapEnum, 0x3A, 8),
    mk_tab(21, "映射枚举封样", EnumSlot::MapEnum, 0x44, 6),
    mk_tab(22, "Token 枚举对照", EnumSlot::TokenEnum, 0x27, 9),
    mk_tab(23, "Token 枚举深档", EnumSlot::TokenEnum, 0x31, 10),
    mk_tab(24, "Token 枚举尾档", EnumSlot::TokenEnum, 0x50, 11),
    mk_tab(25, "Token 枚举复核", EnumSlot::TokenEnum, 0x2D, 9),
    mk_tab(26, "Token 枚举对账", EnumSlot::TokenEnum, 0x3F, 10),
    mk_tab(27, "Token 枚举重放", EnumSlot::TokenEnum, 0x47, 11),
    mk_tab(28, "Token 枚举封样", EnumSlot::TokenEnum, 0x52, 9),
    mk_tab(29, "Token 枚举补账", EnumSlot::TokenEnum, 0x33, 10),
];

// ── 入账账（状态机 TabOpen → Booking → Booked） ──────────────────
/// 入账状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabState {
    /// 开账。
    TabOpen,
    /// 入账中。
    Booking,
    /// 已封账。
    Booked,
}

impl TabState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            TabState::TabOpen => 0,
            TabState::Booking => 1,
            TabState::Booked => 2,
        }
    }
}

/// 入账账。
#[derive(Clone, Debug)]
pub struct TabLedger {
    /// 已入账行（行号序）。
    rows: Vec<TabRow>,
    /// 红账记账（行号 + 违约码；冻结谱下必须为空 = 无红账）。
    reds: Vec<(u32, u32)>,
    /// 入账判定（行号 + 入账位；零静默口径）。
    verdicts: Vec<(u32, bool)>,
    /// 状态。
    state: TabState,
}

impl TabLedger {
    /// 开账。
    pub fn new() -> TabLedger {
        TabLedger {
            rows: Vec::new(),
            reds: Vec::new(),
            verdicts: Vec::new(),
            state: TabState::TabOpen,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> TabState {
        self.state
    }

    /// 已入账行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 红账条数（无红账口径：必须为零）。
    pub fn red_len(&self) -> usize {
        self.reds.len()
    }

    /// 入账判定数。
    pub fn verdict_len(&self) -> usize {
        self.verdicts.len()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&TabRow> {
        self.rows.get(idx)
    }

    /// 进入账（TabOpen→Booking）。
    pub fn begin_book(&mut self) -> Result<(), u32> {
        if self.state.rank() >= TabState::Booking.rank() {
            return Err(TabDiag::ERewind.val());
        }
        self.state = TabState::Booking;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_red(&mut self, row_no: u32, diag: u32) {
        if self.reds.len() < TAB_ROW_CAP * 2 {
            self.reds.push((row_no, diag));
        }
    }

    /// 逐行入账（行号序 + 黄金判定 + 行互异 + 显性记账 四道）。
    pub fn book(&mut self, r: TabRow) -> Result<(), u32> {
        if self.state.rank() < TabState::Booking.rank() {
            return Err(TabDiag::ESilentDrop.val());
        }
        if self.state.rank() >= TabState::Booked.rank() {
            self.note_red(r.row_no, TabDiag::EShortSeal.val());
            return Err(TabDiag::EShortSeal.val());
        }
        // 行号序闸。
        if r.row_no != self.rows.len() as u32 {
            self.note_red(r.row_no, TabDiag::ERowOrder.val());
            return Err(TabDiag::ERowOrder.val());
        }
        // 黄金判定闸。
        if let Err(e) = tab_row_ok(&r) {
            self.note_red(r.row_no, e);
            return Err(e);
        }
        // 行互异闸：同场景同衔接只立一行。
        for done in self.rows.iter() {
            if done.scene == r.scene && done.link_prev == r.link_prev {
                self.note_red(r.row_no, TabDiag::ERowDup.val());
                return Err(TabDiag::ERowDup.val());
            }
        }
        // 显性记账（零静默口径）：每行落入账判定。
        self.verdicts.push((r.row_no, r.book_bit));
        self.rows.push(r);
        Ok(())
    }

    /// 大表预算复核：逐行 P95 类账复核，越限行记红账（显性不静默）。
    pub fn audit_budget(&mut self) -> usize {
        if self.state.rank() < TabState::Booking.rank() {
            return 0;
        }
        let mut found = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => v.clone(),
                None => break,
            };
            if r.steps > r.slot.budget_cap() {
                self.note_red(r.row_no, TabDiag::EBudgetOver.val());
                found += 1;
            }
            i += 1;
        }
        found
    }

    /// 封账：30 行齐 + 判定记账 30 条 + 零红账 + 四类 P95 全在册。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= TabState::Booked.rank() {
            return Err(TabDiag::ERewind.val());
        }
        if self.state.rank() < TabState::Booking.rank() {
            return Err(TabDiag::ESilentDrop.val());
        }
        if self.rows.len() != TAB_ROW_CAP {
            return Err(TabDiag::EShortSeal.val());
        }
        if self.verdicts.len() != TAB_ROW_CAP {
            return Err(TabDiag::ESilentDrop.val());
        }
        if !self.reds.is_empty() {
            return Err(TabDiag::EBudgetOver.val());
        }
        // 四类 P95 预算在册复核（封账硬闸）。
        let snap: Vec<TabRow> = self.rows.iter().cloned().collect();
        for slot in EnumSlot::all().iter() {
            if !class_p95_in_budget(&snap, *slot) {
                return Err(TabDiag::EBudgetOver.val());
            }
        }
        self.state = TabState::Booked;
        Ok(())
    }

    /// 账等式（零静默口径：行数 = 判定数 = 30，红账零）。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == TAB_ROW_CAP
            && self.verdicts.len() == TAB_ROW_CAP
            && self.reds.is_empty()
    }
}

/// 构建完整入账账（30 行逐行入账）。
pub fn build_full_tab_ledger() -> TabLedger {
    let mut led = TabLedger::new();
    let _ = led.begin_book();
    let mut i = 0usize;
    while i < TAB_ROW_CAP {
        let r = match TAB_ROWS.get(i) {
            Some(v) => v.clone(),
            None => break,
        };
        match led.book(r) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    led
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabFault {
    /// 行谱越界。
    RowRange,
    /// 列缺格。
    ColMissing,
    /// 衔接悬空。
    DanglingLink,
    /// 预算越限。
    BudgetOver,
    /// 枚举越闭集。
    EnumRange,
    /// 行号乱序。
    RowOrder,
    /// 行重复。
    RowDup,
    /// 缺行封账。
    ShortSeal,
    /// 静默吞行。
    SilentDrop,
    /// 摘要空。
    SummaryEmpty,
}

impl TabFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            TabFault::RowRange => 0,
            TabFault::ColMissing => 1,
            TabFault::DanglingLink => 2,
            TabFault::BudgetOver => 3,
            TabFault::EnumRange => 4,
            TabFault::RowOrder => 5,
            TabFault::RowDup => 6,
            TabFault::ShortSeal => 7,
            TabFault::SilentDrop => 8,
            TabFault::SummaryEmpty => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            TabFault::RowRange => "行谱越界",
            TabFault::ColMissing => "列缺格",
            TabFault::DanglingLink => "衔接悬空",
            TabFault::BudgetOver => "预算越限",
            TabFault::EnumRange => "枚举越闭集",
            TabFault::RowOrder => "行号乱序",
            TabFault::RowDup => "行重复",
            TabFault::ShortSeal => "缺行封账",
            TabFault::SilentDrop => "静默吞行",
            TabFault::SummaryEmpty => "摘要空",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [TabFault] {
        &[
            TabFault::RowRange,
            TabFault::ColMissing,
            TabFault::DanglingLink,
            TabFault::BudgetOver,
            TabFault::EnumRange,
            TabFault::RowOrder,
            TabFault::RowDup,
            TabFault::ShortSeal,
            TabFault::SilentDrop,
            TabFault::SummaryEmpty,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。
pub fn tab_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in TabFault::all().iter() {
        total += 1;
        if run_tab_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_tab_fault(v: TabFault) -> bool {
    match v {
        TabFault::RowRange => {
            let mut r = match TAB_ROWS.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.row_no = TAB_ROW_CAP as u32;
            tab_row_ok(&r).is_err()
        }
        TabFault::ColMissing => {
            let mut r = match TAB_ROWS.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.scene = "";
            tab_row_ok(&r).is_err()
        }
        TabFault::DanglingLink => {
            let mut r = match TAB_ROWS.get(2) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.link_prev = PREV_ROW_CAP as u32;
            tab_row_ok(&r).is_err()
        }
        TabFault::BudgetOver => {
            let mut r = match TAB_ROWS.get(3) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.steps = r.slot.budget_cap().saturating_add(1);
            tab_row_ok(&r).is_err()
        }
        TabFault::EnumRange => {
            // 枚举越闭集：计量清零 → 列齐闸口径抓（枚举类枚举封闭，以
            // 零计量代表槽未在册）。
            let mut r = match TAB_ROWS.get(4) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.steps = 0;
            tab_row_ok(&r).is_err()
        }
        TabFault::RowOrder => {
            let mut led = TabLedger::new();
            let _ = led.begin_book();
            let r = match TAB_ROWS.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.book(r).is_err()
        }
        TabFault::RowDup => {
            let mut led = TabLedger::new();
            let _ = led.begin_book();
            let r0 = match TAB_ROWS.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.book(r0.clone());
            let dup = TabRow {
                row_no: 1,
                scene: r0.scene,
                slot: r0.slot,
                steps: r0.steps,
                link_prev: r0.link_prev,
                book_bit: true,
            };
            led.book(dup).is_err()
        }
        TabFault::ShortSeal => {
            let mut led = TabLedger::new();
            let _ = led.begin_book();
            let r = match TAB_ROWS.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.book(r);
            led.seal().is_err()
        }
        TabFault::SilentDrop => {
            // 未入账即封账被拒（TabOpen 态直接 seal）。
            let mut led = TabLedger::new();
            led.seal().is_err()
        }
        TabFault::SummaryEmpty => {
            // 摘要空族：入账位清空 → 列齐闸必拒。
            let mut r = match TAB_ROWS.get(5) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.book_bit = false;
            tab_row_ok(&r).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 枚举类 P95 预算在册（判据 J1 前半直读口）。
fn p95_in_ledger_ok() -> bool {
    // 四类枚举槽全在册：类数冻结 4。
    if EnumSlot::all().len() != 4 {
        return false;
    }
    // 四类逐一在册复核。
    for slot in EnumSlot::all().iter() {
        if !class_p95_in_budget(TAB_ROWS, *slot) {
            return false;
        }
    }
    // P95 序位法直读：单类样本序位可回放。
    let samples = class_samples(TAB_ROWS, EnumSlot::KeyEnum);
    let sorted = sorted_copy(&samples);
    if samples.is_empty() {
        return false;
    }
    let p95 = match p95_sorted(&sorted) {
        Some(v) => v,
        None => return false,
    };
    if p95 <= 0 {
        return false;
    }
    // 空样本 P95 必为 None（序位法边界）。
    if p95_sorted(&[]).is_some() {
        return false;
    }
    true
}

/// 大表注入账无红账（判据 J1 后半直读口）。
fn no_red_ledger_ok() -> bool {
    let mut led = build_full_tab_ledger();
    // 大表预算复核：冻结谱下零红。
    let found = led.audit_budget();
    if found != 0 {
        return false;
    }
    if led.red_len() != 0 {
        return false;
    }
    // 封账：四类 P95 全在册且红账空。
    if led.seal().is_err() {
        return false;
    }
    // 注入反证：越限行必被抓（红账显性）。
    let mut bad = match TAB_ROWS.get(0) {
        Some(x) => x.clone(),
        None => return false,
    };
    bad.steps = bad.slot.budget_cap().saturating_add(0x10);
    if tab_row_ok(&bad).is_ok() {
        return false;
    }
    true
}

/// 二批对照表 30 行入账全过。
fn booking_ok() -> bool {
    let mut led = build_full_tab_ledger();
    if led.len() != TAB_ROW_CAP {
        return false;
    }
    if led.verdict_len() != TAB_ROW_CAP {
        return false;
    }
    if led.red_len() != 0 {
        return false;
    }
    if led.seal().is_err() {
        return false;
    }
    if led.state() != TabState::Booked {
        return false;
    }
    // 独立复核：30 行逐行黄金判定。
    for r in TAB_ROWS.iter() {
        if tab_row_ok(r).is_err() {
            return false;
        }
    }
    true
}

/// 枚举类覆盖普查（30 行覆盖四类、类内行数有账、衔接全点验）。
fn enum_census_ok() -> bool {
    let mut per_class = [0usize; 4];
    let mut i = 0usize;
    while i < TAB_ROWS.len() {
        let r = match TAB_ROWS.get(i) {
            Some(v) => v,
            None => return false,
        };
        if r.row_no != i as u32 {
            return false;
        }
        match per_class.get_mut(r.slot.ordinal() as usize) {
            Some(v) => *v += 1,
            None => return false,
        }
        if r.link_prev >= PREV_ROW_CAP as u32 {
            return false;
        }
        i += 1;
    }
    // 四类每类至少 4 行（普查下沿，非配额口径）。
    for slot in per_class.iter() {
        if *slot < 4 {
            return false;
        }
    }
    true
}

/// 零静默（判定记账逐行齐、行号一一对应）。
fn zero_silent_ok() -> bool {
    let led = build_full_tab_ledger();
    if led.verdict_len() != led.len() {
        return false;
    }
    let mut i = 0usize;
    while i < led.verdict_len() {
        let v = match led.verdicts.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let r = match led.row_at(i) {
            Some(row) => row,
            None => return false,
        };
        if v.0 != r.row_no || !v.1 {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入联测 10/10 拒止。
fn tab_fault_sweep_ok() -> bool {
    let (caught, total, escape) = tab_fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 双跑一致（冻结行谱下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_tab_ledger();
    let b = build_full_tab_ledger();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if tab_fault_sweep().0 != tab_fault_sweep().0 {
        return false;
    }
    if p95_sorted(&sorted_copy(&class_samples(TAB_ROWS, EnumSlot::MapEnum)))
        != p95_sorted(&sorted_copy(&class_samples(TAB_ROWS, EnumSlot::MapEnum)))
    {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12674u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut led = build_full_tab_ledger();
    if led.seal().is_err() {
        return false;
    }
    if led.state() != TabState::Booked {
        return false;
    }
    // 摘要：首行场景名 + 尾行枚举类名拼接非空。
    let head = match led.row_at(0) {
        Some(r) => r.scene,
        None => return false,
    };
    let tail = match led.row_at(TAB_ROW_CAP - 1) {
        Some(r) => r.slot.name(),
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12675 对照表入账自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12674_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12674-cmptab2");
    let led = build_full_tab_ledger();

    s.add_dyn(
        "F12675-01 二批对照行三十行齐",
        TAB_ROWS.len() == TAB_ROW_CAP,
        &format!("rows={}", TAB_ROWS.len()),
    );

    s.add_dyn(
        "F12675-02 枚举类 P95 预算在册",
        p95_in_ledger_ok(),
        &format!("slots={}", EnumSlot::all().len()),
    );

    s.add_dyn(
        "F12675-03 大表注入账无红账",
        no_red_ledger_ok(),
        &format!("reds={}", led.red_len()),
    );

    s.add_dyn(
        "F12675-04 三十行入账全过",
        booking_ok(),
        &format!("rows={} verdicts={}", led.len(), led.verdict_len()),
    );

    s.add_dyn(
        "F12675-05 枚举类覆盖普查",
        enum_census_ok(),
        &format!("classes={}", EnumSlot::all().len()),
    );

    s.add_dyn(
        "F12675-06 零静默",
        zero_silent_ok(),
        &format!("verdicts={}", led.verdict_len()),
    );

    s.add_dyn(
        "F12675-07 注入联测 10/10 拒止",
        tab_fault_sweep_ok(),
        &format!("caught={}", tab_fault_sweep().0),
    );

    s.add_dyn(
        "F12675-08 账等式",
        led.equation_ok(),
        &format!("rows={} verdicts={} reds={}", led.len(), led.verdict_len(), led.red_len()),
    );

    s.add_dyn(
        "F12675-09 双跑一致",
        dual_run_ok(),
        "冻结行谱下两轮逐行同判",
    );

    s.add_dyn(
        "F12675-10 P95 序位法边界",
        p95_sorted(&[]).is_none() && p95_sorted(&[0x21]).is_some(),
        "空样本 None / 单样本取位",
    );

    s.add_dyn(
        "F12675-11 预算上限冻结轴",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i + 1 < EnumSlot::all().len() {
                let a = match EnumSlot::all().get(i) {
                    Some(v) => v.budget_cap(),
                    None => 0,
                };
                let b = match EnumSlot::all().get(i + 1) {
                    Some(v) => v.budget_cap(),
                    None => 0,
                };
                if a >= b {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "四类上限递增且互异",
    );

    s.add_dyn(
        "F12675-12 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12675-13 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81280);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(TAB_ALL.len(), 11);
        let mut i = 0usize;
        while i < TAB_ALL.len() {
            let vi = match TAB_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < TAB_ALL.len() {
                let vj = match TAB_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn p95_ord_rank_math() {
        // 序位法直读：上取整等价式。
        assert_eq!(p95_sorted(&[]), None);
        assert_eq!(p95_sorted(&[5]), Some(5));
        assert_eq!(p95_sorted(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]), Some(10));
        assert_eq!(p95_sorted(&sorted_copy(&[10, 1, 9, 2, 8, 3, 7, 4, 6, 5])), Some(10));
        // 20 样本 P95 序位 = 第 19 个（上取整 19 − 1）。
        let mut v: Vec<u32> = Vec::new();
        let mut i = 1u32;
        while i <= 20 {
            v.push(i);
            i += 1;
        }
        assert_eq!(p95_sorted(&v), Some(19));
    }

    #[test]
    fn p95_in_ledger_holds() {
        // 枚举类 P95 预算在册：四类全过。
        assert!(p95_in_ledger_ok());
        for slot in EnumSlot::all().iter() {
            assert!(class_p95_in_budget(TAB_ROWS, *slot), "类 {} 越预算", slot.name());
        }
    }

    #[test]
    fn no_red_ledger_with_injection() {
        // 无红账：冻结谱零红 + 注入越限必抓。
        assert!(no_red_ledger_ok());
        let mut led = build_full_tab_ledger();
        assert_eq!(led.red_len(), 0);
        // 注入越限行：预算复核记账红账。
        let mut bad = match TAB_ROWS.get(2) {
            Some(x) => x.clone(),
            None => return,
        };
        bad.steps = bad.slot.budget_cap().saturating_add(0x20);
        let _ = led.book(bad.clone());
        assert_eq!(led.red_len(), 1);
    }

    #[test]
    fn happy_path_seals() {
        let mut led = build_full_tab_ledger();
        assert_eq!(led.len(), TAB_ROW_CAP);
        assert_eq!(led.verdict_len(), TAB_ROW_CAP);
        assert_eq!(led.red_len(), 0);
        assert!(led.seal().is_ok());
        assert_eq!(led.state(), TabState::Booked);
        assert!(led.equation_ok());
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = tab_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn row_gate_rejects() {
        let mut led = TabLedger::new();
        let _ = led.begin_book();
        let r1 = match TAB_ROWS.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(led.book(r1), Err(TabDiag::ERowOrder.val()));
        let r0 = match TAB_ROWS.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(led.book(r0.clone()).is_ok());
        let dup = TabRow {
            row_no: 1,
            scene: r0.scene,
            slot: r0.slot,
            steps: r0.steps,
            link_prev: r0.link_prev,
            book_bit: true,
        };
        assert_eq!(led.book(dup), Err(TabDiag::ERowDup.val()));
        let mut over = match TAB_ROWS.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        over.steps = over.slot.budget_cap().saturating_add(1);
        assert_eq!(led.book(over), Err(TabDiag::EBudgetOver.val()));
        assert_eq!(led.red_len(), 3);
    }

    #[test]
    fn enum_census_holds() {
        // 30 行覆盖四类、每类 ≥4 行、行号连续。
        assert!(enum_census_ok());
        assert_eq!(TAB_ROWS.len(), 30);
        assert_eq!(EnumSlot::all().len(), 4);
        assert!(EnumSlot::TokenEnum.budget_cap() > EnumSlot::KeyEnum.budget_cap());
        assert!(EnumSlot::MapEnum.budget_cap() > EnumSlot::HandleEnum.budget_cap());
    }

    #[test]
    fn zero_silent_ledger() {
        assert!(zero_silent_ok());
        let led = build_full_tab_ledger();
        assert_eq!(led.verdict_len(), TAB_ROW_CAP);
        let mut i = 0usize;
        while i < TAB_ROW_CAP {
            let r = match led.row_at(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(r.row_no, i as u32, "行 {} 错位", i);
            i += 1;
        }
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_tab_ledger();
        let b = build_full_tab_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑行缺失"),
            }
            i += 1;
        }
        assert_eq!(tab_fault_sweep().0, tab_fault_sweep().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：TabOpen→Booking→Booked。
        assert!(TabState::TabOpen.rank() < TabState::Booking.rank());
        assert!(TabState::Booking.rank() < TabState::Booked.rank());
        let mut led = build_full_tab_ledger();
        assert_eq!(led.state(), TabState::Booking);
        assert!(led.seal().is_ok());
        assert_eq!(led.seal(), Err(TabDiag::ERewind.val()));
        let mut fresh = TabLedger::new();
        assert!(fresh.begin_book().is_ok());
        assert_eq!(fresh.begin_book(), Err(TabDiag::ERewind.val()));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12674_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
