//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18460 · B03 批守恒闭合断言 · 0x7EEC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18460-J1 本批 20 条行数逐条求和=6,000 三方一致，红绿齐
//! 本模块结构清单：
//!   1. 冻结常量层：B03 批 20 卡（F18441–F18460）行账（卡号/SEG/配额/
//!      目标行数四元组，取自派工卡 A05/A06 工单，逐行推导注记写死）；
//!   2. 诊断码闭集 ClCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构一（批账）：BatchRow + B03_LEDGER——20 行冻结账本
//!      （只读，注入全部走工作副本，账本不可变红线）；
//!   4. 三方一致核：①逐行求和（20 行累加）②分组求和（A05 组 2960
//!      + A06 组 3040）③批预算冻结值 6000——三路独立推导同值；
//!   5. 派生账：目标行数和 6160+6240=12400（两组派生注记）；
//!      段位连续性：0x7EA00 起步进 0x40 共 20 卡至 0x7EEC0；
//!      闭合链：本卡 SEG_BASE == 行账末行 seg（闭合卡自指闭环）；
//!   6. 数据结构二（红绿账）：RedGreenPair——红账=注入检出事件、
//!      绿账=干净审计事件，成对律可复算（红多必绿少，账不生灭）；
//!   7. 注入核 4 类×10 轮：配额篡改 / 目标篡改 / 段位断链 / 重复
//!      卡号，逐类必检出且守恒账不受注入污染；
//!   8. 黄金全链 + run_unx18460_checks：15 条判据（断言性质）。
//! 为什么闭合卡最后做：批守恒是把整批 20 行的账一次收口——任何
//! 中途卡面的配额/段位/卡号漂移都会让三方求和对不齐；本卡同时
//! 把自身（闭合卡 F18460）写进行账末行，SEG_BASE 自指闭环，批账
//! 才真正闭合。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EEC0，AI-VX11-1105-G1·A06 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EEC0;

// ── 批账冻结常量（推导注记） ─────────────────────────────────────
/// 批首卡号（A05 组首）。
pub const CARD_FIRST: u32 = 18441;
/// 批末卡号（A06 组末，即本闭合卡）。
pub const CARD_LAST: u32 = 18460;
/// 批行数（冻结：20 卡）。
pub const ROWS: usize = 20;
/// A05 组行数（冻结）。
pub const GROUP_A05: usize = 10;
/// 批预算（冻结：判据原文 6,000）。
pub const BATCH_BUDGET: u32 = 6_000;
/// A05 组配额和（逐行累加：300+320+280+340+260+300+320+280+300+260）。
pub const GROUP_A05_SUM: u32 = 2_960;
/// A06 组配额和（逐行累加：320+280+300+260+300+320+280+300+380+300）。
pub const GROUP_A06_SUM: u32 = 3_040;
/// 目标行数派生和（A05 组 6160 + A06 组 6240）。
pub const TARGET_TOTAL: u32 = 12_400;
/// 段位起点（批首卡）。
pub const SEG_START: u32 = 0x7EA00;
/// 段位步进（冻结：0x40 独占窗口）。
pub const SEG_STEP: u32 = 0x40;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 行四元组 ────────────────────────────────────────────────────
/// 批账行：卡号 / 段基址 / 配额 / 目标行数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchRow {
    pub card: u32,
    pub seg: u32,
    pub quota: u32,
    pub target: u32,
}

/// B03 批 20 行冻结账本（只读；配额/目标取自派工卡 A05/A06 工单；
/// 末行即本闭合卡 F18460）。
pub const B03_LEDGER: [BatchRow; ROWS] = [
    // A05 组（F18441–F18450）。
    BatchRow { card: 18441, seg: 0x7EA00, quota: 300, target: 600 },
    BatchRow { card: 18442, seg: 0x7EA40, quota: 320, target: 640 },
    BatchRow { card: 18443, seg: 0x7EA80, quota: 280, target: 600 },
    BatchRow { card: 18444, seg: 0x7EAC0, quota: 340, target: 680 },
    BatchRow { card: 18445, seg: 0x7EB00, quota: 260, target: 600 },
    BatchRow { card: 18446, seg: 0x7EB40, quota: 300, target: 600 },
    BatchRow { card: 18447, seg: 0x7EB80, quota: 320, target: 640 },
    BatchRow { card: 18448, seg: 0x7EBC0, quota: 280, target: 600 },
    BatchRow { card: 18449, seg: 0x7EC00, quota: 300, target: 600 },
    BatchRow { card: 18450, seg: 0x7EC40, quota: 260, target: 600 },
    // A06 组（F18451–F18460，末行=闭合卡本体）。
    BatchRow { card: 18451, seg: 0x7EC80, quota: 320, target: 640 },
    BatchRow { card: 18452, seg: 0x7ECC0, quota: 280, target: 600 },
    BatchRow { card: 18453, seg: 0x7ED00, quota: 300, target: 600 },
    BatchRow { card: 18454, seg: 0x7ED40, quota: 260, target: 600 },
    BatchRow { card: 18455, seg: 0x7ED80, quota: 300, target: 600 },
    BatchRow { card: 18456, seg: 0x7EDC0, quota: 320, target: 640 },
    BatchRow { card: 18457, seg: 0x7EE00, quota: 280, target: 600 },
    BatchRow { card: 18458, seg: 0x7EE40, quota: 300, target: 600 },
    BatchRow { card: 18459, seg: 0x7EE80, quota: 380, target: 760 },
    BatchRow { card: 18460, seg: 0x7EEC0, quota: 300, target: 600 },
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    /// 三方求和不一致
    SumMismatch,
    /// 逐行与冻结派工账不符
    RowMismatch,
    /// 段位断链（步进/对齐破）
    SegBroken,
    /// 卡号重复
    CardDup,
    /// 配额篡改（注入检出码）
    QuotaTamper,
    /// 目标行数篡改（注入检出码）
    TargetTamper,
    /// 段位篡改（注入检出码）
    SegTamper,
    /// 账本被写回（只读红线）
    LedgerMutated,
    /// 红绿账不齐
    RedGreenUnpaired,
    /// 闭合链破（本卡 SEG_BASE 与末行 seg 失配）
    ClosureBroken,
    /// 双跑不一致
    NotReproducible,
}

impl ClCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 诊断码别名：code() = SEG_BASE + n（与 val() 同值，契约骨架口径）。
    pub fn code(self) -> u32 {
        self.val()
    }
    pub fn detail(self) -> &'static str {
        match self {
            ClCode::SumMismatch => "三方求和不一致：逐行/分组/预算三路不同值",
            ClCode::RowMismatch => "逐行不符：账本行与冻结派工账不一致",
            ClCode::SegBroken => "段位断链：0x40 步进或 0x40 对齐破",
            ClCode::CardDup => "卡号重复：批账键不唯一",
            ClCode::QuotaTamper => "配额篡改：与冻结账逐行对不上",
            ClCode::TargetTamper => "目标行数篡改：与冻结账逐行对不上",
            ClCode::SegTamper => "段位篡改：断链注入被检出",
            ClCode::LedgerMutated => "账本被写回：只读红线",
            ClCode::RedGreenUnpaired => "红绿账不齐：红账事件与注入数不成对",
            ClCode::ClosureBroken => "闭合链破：闭合卡 SEG_BASE 与末行失配",
            ClCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [ClCode; 11] = [
        ClCode::SumMismatch,
        ClCode::RowMismatch,
        ClCode::SegBroken,
        ClCode::CardDup,
        ClCode::QuotaTamper,
        ClCode::TargetTamper,
        ClCode::SegTamper,
        ClCode::LedgerMutated,
        ClCode::RedGreenUnpaired,
        ClCode::ClosureBroken,
        ClCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ClCode::ALL.len() {
        if ClCode::ALL[i].val() == code {
            return ClCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EEC0 段闭集内"
}

// ── 三路求和（三方一致） ────────────────────────────────────────
/// 路 1：逐行求和（20 行累加）。
pub fn sum_rows(ledger: &[BatchRow; ROWS]) -> u32 {
    let mut acc = 0u32;
    let mut i = 0;
    while i < ROWS {
        acc = acc.saturating_add(ledger[i].quota);
        i += 1;
    }
    acc
}

/// 路 2：分组求和（前 10 行 A05 组 + 后 10 行 A06 组）。
pub fn sum_groups(ledger: &[BatchRow; ROWS]) -> (u32, u32) {
    let mut g1 = 0u32;
    let mut g2 = 0u32;
    let mut i = 0;
    while i < ROWS {
        if i < GROUP_A05 {
            g1 = g1.saturating_add(ledger[i].quota);
        } else {
            g2 = g2.saturating_add(ledger[i].quota);
        }
        i += 1;
    }
    (g1, g2)
}

/// 路 3：批预算冻结值（派工卡口径）。
pub fn budget_frozen() -> u32 {
    BATCH_BUDGET
}

/// 三方一致判定：三路同值 == 6000。
pub fn three_way(ledger: &[BatchRow; ROWS]) -> Result<u32, u32> {
    let w1 = sum_rows(ledger);
    let (g1, g2) = sum_groups(ledger);
    let w2 = g1.saturating_add(g2);
    let w3 = budget_frozen();
    if w1 != BATCH_BUDGET || w2 != BATCH_BUDGET || w3 != BATCH_BUDGET {
        return Err(ClCode::SumMismatch.val());
    }
    if w1 != w2 || w2 != w3 {
        return Err(ClCode::SumMismatch.val());
    }
    Ok(w1)
}

/// 目标行数派生账：两组 6160+6240=12400。
pub fn target_total(ledger: &[BatchRow; ROWS]) -> Result<(u32, u32, u32), u32> {
    let mut t1 = 0u32;
    let mut t2 = 0u32;
    let mut i = 0;
    while i < ROWS {
        if i < GROUP_A05 {
            t1 = t1.saturating_add(ledger[i].target);
        } else {
            t2 = t2.saturating_add(ledger[i].target);
        }
        i += 1;
    }
    if t1.saturating_add(t2) != TARGET_TOTAL {
        return Err(ClCode::SumMismatch.val());
    }
    Ok((t1, t2, t1.saturating_add(t2)))
}

/// 段位连续性：步进 0x40 × 20 卡，0x40 对齐，卡号连续。
pub fn seg_contiguity(ledger: &[BatchRow; ROWS]) -> Result<(), u32> {
    let mut i = 0;
    while i < ROWS {
        let r = match ledger.get(i) {
            Some(x) => *x,
            None => return Err(ClCode::SegBroken.val()),
        };
        if r.seg != SEG_START + SEG_STEP * (i as u32) {
            return Err(ClCode::SegBroken.val());
        }
        if r.seg % 0x40 != 0 {
            return Err(ClCode::SegBroken.val());
        }
        if r.card != CARD_FIRST + (i as u32) {
            return Err(ClCode::CardDup.val());
        }
        i += 1;
    }
    Ok(())
}

/// 闭合链：本卡 SEG_BASE == 账本末行 seg（闭合卡自指闭环），
/// 且末行卡号 == CARD_LAST。
pub fn closure_chain(ledger: &[BatchRow; ROWS]) -> Result<(), u32> {
    let last = match ledger.get(ROWS - 1) {
        Some(x) => *x,
        None => return Err(ClCode::ClosureBroken.val()),
    };
    if SEG_BASE != last.seg || last.card != CARD_LAST {
        return Err(ClCode::ClosureBroken.val());
    }
    Ok(())
}

// ── 审计面（逐行核对冻结账） ────────────────────────────────────
/// 逐行审计：账本行与 B03_LEDGER 冻结行全字段一致。
/// 返回红行下标集（干净账本必为空）。
pub fn audit_all(ledger: &[BatchRow; ROWS]) -> Vec<usize> {
    let mut red: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < ROWS {
        let f = &B03_LEDGER[i];
        let r = match ledger.get(i) {
            Some(x) => x,
            None => break,
        };
        if r.card != f.card || r.seg != f.seg || r.quota != f.quota || r.target != f.target {
            red.push(i);
        }
        i += 1;
    }
    red
}

/// 全账审计：逐行 + 三方 + 段位连续 + 卡号唯一。返回红码集（空=全绿）。
pub fn audit_full(ledger: &[BatchRow; ROWS]) -> Vec<u32> {
    let mut red: Vec<u32> = Vec::new();
    if three_way(ledger).is_err() {
        red.push(ClCode::SumMismatch.val());
    }
    if seg_contiguity(ledger).is_err() {
        red.push(ClCode::SegBroken.val());
    }
    if audit_all(ledger).len() > 0 {
        red.push(ClCode::RowMismatch.val());
    }
    // 卡号唯一（成对比较）。
    let mut i = 0;
    while i < ROWS {
        let mut j = i + 1;
        while j < ROWS {
            if ledger[i].card == ledger[j].card {
                red.push(ClCode::CardDup.val());
            }
            j += 1;
        }
        i += 1;
    }
    red
}

// ── 数据结构二：红绿账（红绿齐） ────────────────────────────────
/// 红绿账对：红账=注入检出事件，绿账=干净审计事件。
/// 成对律：红账计数 == 注入轮数×类数 ∧ 绿账计数 == 0（干净跑零红）。
#[derive(Debug)]
pub struct RedGreenPair {
    pub red_events: u32,
    pub green_events: u32,
}

impl RedGreenPair {
    /// 红绿齐判定。
    pub fn paired(&self, expect_red: u32) -> bool {
        self.red_events == expect_red && self.green_events == 0
    }
}

// ── 注入核（4 类 ×10 轮） ───────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 4 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入：四类篡改逐类构造工作副本（冻结账本本体永不写回），
/// 审计面必须逐类检出。
/// 槽 0 配额篡改（行 5 配额 +1）/ 槽 1 目标篡改（行 13 目标 −1）/
/// 槽 2 段位断链（行 17 段 +0x80）/ 槽 3 重复卡号（行 15 卡号改成
/// 行 14 的）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：配额篡改——三方求和必破 + 逐行核对必红。
        let mut l0 = B03_LEDGER;
        l0[5].quota = l0[5].quota.saturating_add(1);
        let red0 = audit_full(&l0);
        let hit0 = red0.contains(&ClCode::RowMismatch.val())
            && red0.contains(&ClCode::SumMismatch.val());
        led.record(0, hit0);
        // 槽 1：目标篡改——派生账必破。
        let mut l1 = B03_LEDGER;
        l1[13].target = l1[13].target.saturating_sub(1);
        let red1 = audit_full(&l1);
        let hit1 = red1.contains(&ClCode::RowMismatch.val())
            && target_total(&l1).is_err();
        led.record(1, hit1);
        // 槽 2：段位断链——行 17 段 +0x80 → 步进破。
        let mut l2 = B03_LEDGER;
        l2[17].seg = l2[17].seg.saturating_add(0x80);
        let red2 = audit_full(&l2);
        let hit2 = red2.contains(&ClCode::SegBroken.val());
        led.record(2, hit2);
        // 槽 3：重复卡号——行 15 卡号改成行 14 的。
        let mut l3 = B03_LEDGER;
        l3[15].card = l3[14].card;
        let red3 = audit_full(&l3);
        let hit3 = red3.contains(&ClCode::CardDup.val());
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：干净账本三方一致 + 段位连续 + 闭合链 + 审计零红。
pub fn probe_seq_a() -> Result<(), u32> {
    three_way(&B03_LEDGER)?;
    seg_contiguity(&B03_LEDGER)?;
    closure_chain(&B03_LEDGER)?;
    if audit_all(&B03_LEDGER).len() != 0 {
        return Err(ClCode::RowMismatch.val());
    }
    Ok(())
}

/// 负探针 1：配额篡改副本审计必红（冻结账本本体不受扰）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = B03_LEDGER;
    l[0].quota = l[0].quota.saturating_add(1);
    let red = audit_all(&l);
    if red.len() != 1 || red.get(0).copied() != Some(0) {
        return Err(ClCode::QuotaTamper.val());
    }
    if B03_LEDGER[0].quota != 300 {
        return Err(ClCode::LedgerMutated.val());
    }
    Ok(())
}

/// 负探针 2：闭合链自指（预算冻结值与账本无关性）。
pub fn probe_seq_c() -> Result<(), u32> {
    if budget_frozen() != 6_000 || SEG_BASE != B03_LEDGER[ROWS - 1].seg {
        return Err(ClCode::ClosureBroken.val());
    }
    Ok(())
}

/// 黄金全链：三方一致 + 段位连续 + 闭合链 + 派生账 + 注入全抓 +
/// 红绿齐 + 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let sum = three_way(&B03_LEDGER)?;
    seg_contiguity(&B03_LEDGER)?;
    closure_chain(&B03_LEDGER)?;
    let (_, _, tt) = target_total(&B03_LEDGER)?;
    if audit_all(&B03_LEDGER).len() != 0 {
        return Err(ClCode::RowMismatch.val());
    }
    if audit_full(&B03_LEDGER).len() != 0 {
        return Err(ClCode::RowMismatch.val());
    }
    // 注入 10 轮全抓。
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() {
        return Err(ClCode::NotReproducible.val());
    }
    // 红绿齐：红账 4 类 ×10 轮 = 40 事件，绿账 0。
    let red_total = inj.caught[0] + inj.caught[1] + inj.caught[2] + inj.caught[3];
    let pair = RedGreenPair { red_events: red_total, green_events: 0 };
    if !pair.paired(40) {
        return Err(ClCode::RedGreenUnpaired.val());
    }
    // 注入不许污染守恒账：三方求和仍 6000。
    let sum_after = three_way(&B03_LEDGER)?;
    if sum_after != sum {
        return Err(ClCode::LedgerMutated.val());
    }
    Ok((sum, tt, red_total))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ClCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ClCode::ALL.len() {
        let v = ClCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18460_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18460 B03 批守恒闭合");
    // ① 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!(
            "和{:?} 目标{:?} 红账{:?}",
            g.as_ref().map(|r| r.0).unwrap_or(0),
            g.as_ref().map(|r| r.1).unwrap_or(0),
            g.as_ref().map(|r| r.2).unwrap_or(0)
        ),
    );
    // ② 三方一致：逐行 == 分组 == 预算 == 6000（J1 前半句）。
    let w = three_way(&B03_LEDGER);
    let tw_ok = matches!(w, Ok(x) if x == BATCH_BUDGET);
    s.add_dyn(
        "三方求和=6000",
        tw_ok,
        &format!(
            "逐行{} 分组{:?} 预算{}",
            sum_rows(&B03_LEDGER),
            sum_groups(&B03_LEDGER),
            budget_frozen()
        ),
    );
    // ③ 分组账：A05 2960 / A06 3040。
    let (g1, g2) = sum_groups(&B03_LEDGER);
    s.add("分组2960+3040", g1 == GROUP_A05_SUM && g2 == GROUP_A06_SUM, "两组逐行累加复算");
    // ④ 逐行核对冻结派工账 20/20。
    s.add("逐行核对20/20", audit_all(&B03_LEDGER).len() == 0, "卡号/SEG/配额/目标四元组");
    // ⑤ 段位连续：0x7EA00 起步进 0x40 至 0x7EEC0。
    s.add("段位连续20卡", seg_contiguity(&B03_LEDGER).is_ok(), "0x40 步进+0x40 对齐");
    // ⑥ 闭合链：本卡 SEG_BASE == 末行 seg，卡号 18460 自指闭环。
    s.add("闭合链自指闭环", closure_chain(&B03_LEDGER).is_ok(),
        "闭合卡 F18460 收批账末行");
    // ⑦ 目标行数派生账 12400（6160+6240）。
    let tt = target_total(&B03_LEDGER);
    s.add_dyn(
        "目标行数账12400",
        matches!(tt, Ok((t1, t2, _)) if t1 + t2 == TARGET_TOTAL),
        &format!("{:?}", tt),
    );
    // ⑧ 注入四类 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "注入四类10/10检出",
        inj_ok,
        &format!(
            "{}/{}/{}/{}",
            inj.as_ref().map(|l| l.caught[0]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[1]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[2]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[3]).unwrap_or(0)
        ),
    );
    // ⑨ 注入漏检为零（计数守恒）。
    s.add("注入漏检为零", match &inj {
        Ok(l) => l.missed() == 0,
        Err(_) => false,
    }, "检出+漏检=注入，漏检=0");
    // ⑩ 红绿齐：红账 40 事件成对，绿账 0（J1 后半句）。
    s.add("红绿齐", g.is_ok(), "红账=4×10 事件 绿账=0");
    // ⑪ 守恒账不被注入污染（账本只读红线）。
    s.add("守恒账不被注入污染",
        B03_LEDGER[0].quota == 300 && B03_LEDGER[19].seg == 0x7EEC0
            && B03_LEDGER[13].target == 600,
        "冻结账本本体不可变");
    // ⑫ 探针三序。
    s.add("探针三序", probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        "正链/篡改副本/闭合自指");
    // ⑬ 双跑一致。
    s.add("双跑一致", g == golden_run(), "纯常量账无随机");
    // ⑭ 卡号连续复算：18441..18460 逐行 +1。
    let mut cards_ok = true;
    let mut ci = 0;
    while ci < ROWS {
        if B03_LEDGER[ci].card != CARD_FIRST + ci as u32 {
            cards_ok = false;
        }
        ci += 1;
    }
    s.add("卡号连续复算", cards_ok, "20 行卡号逐行 +1");
    // ⑮ 文案闭集 + 段位守卫。
    let mut closed = true;
    let mut di = 0;
    while di < ClCode::ALL.len() {
        if ClCode::ALL[di].detail().is_empty() {
            closed = false;
        }
        di += 1;
    }
    s.add("诊断文案闭集非空", closed, "11 码全有中文建议");
    s.add("段位守卫 0x7EEC0", segment_guard(), "SEG_BASE 对齐 0x40 窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_way_sum_is_six_thousand() {
        let w = three_way(&B03_LEDGER).unwrap_or(0);
        assert_eq!(w, 6_000, "逐行==分组==预算==6000");
        assert_eq!(sum_rows(&B03_LEDGER), 6_000);
        assert_eq!(sum_groups(&B03_LEDGER), (2_960, 3_040));
    }

    #[test]
    fn ledger_rows_match_dispatch() {
        assert_eq!(audit_all(&B03_LEDGER).len(), 0, "20 行逐条核对零红");
        assert_eq!(audit_full(&B03_LEDGER).len(), 0, "全账审计零红");
    }

    #[test]
    fn segment_contiguity_frozen() {
        assert!(seg_contiguity(&B03_LEDGER).is_ok());
        assert_eq!(B03_LEDGER[0].seg, 0x7EA00);
        assert_eq!(B03_LEDGER[ROWS - 1].seg, 0x7EEC0);
        assert_eq!(B03_LEDGER[ROWS - 1].seg - B03_LEDGER[0].seg, SEG_STEP * (ROWS as u32 - 1));
    }

    #[test]
    fn closure_chain_self_referential() {
        assert!(closure_chain(&B03_LEDGER).is_ok());
        assert_eq!(SEG_BASE, B03_LEDGER[ROWS - 1].seg, "闭合卡自指");
        assert_eq!(B03_LEDGER[ROWS - 1].card, 18460);
        assert_eq!(B03_LEDGER[ROWS - 1].quota, 300);
    }

    #[test]
    fn injections_four_classes_all_caught() {
        let inj = inject_rounds(10).unwrap_or_else(|_| InjectLedger::new());
        assert!(inj.all_caught(), "四类篡改必须 10/10 检出");
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn frozen_ledger_immutable_under_injection() {
        let _ = inject_rounds(10);
        // 注入全部走工作副本——冻结账本本体逐字段不变。
        // 行 17 = 卡 18458，冻结段位 = 0x7EA00 + 0x40×17 = 0x7EE40
        // （0x7EE00 是行 16 卡 18457 的段，勿混）。
        assert_eq!(B03_LEDGER[5].quota, 300);
        assert_eq!(B03_LEDGER[13].target, 600);
        assert_eq!(B03_LEDGER[17].seg, 0x7EE40);
        assert_eq!(B03_LEDGER[15].card, 18456);
    }

    #[test]
    fn target_total_derived() {
        let tt = target_total(&B03_LEDGER).unwrap_or((0, 0, 0));
        assert_eq!(tt, (6_160, 6_240, 12_400), "两组目标行数派生账");
    }

    #[test]
    fn red_green_pair_accounting() {
        let inj = inject_rounds(10).unwrap_or_else(|_| InjectLedger::new());
        let red_total = inj.caught[0] + inj.caught[1] + inj.caught[2] + inj.caught[3];
        let pair = RedGreenPair { red_events: red_total, green_events: 0 };
        assert!(pair.paired(40), "红账 40 事件绿账 0 成对");
        let clean_pair = RedGreenPair { red_events: 0, green_events: 0 };
        assert!(clean_pair.paired(0), "干净跑红绿两账皆空");
        assert!(!pair.paired(39), "红账 39 不成对（红多必绿少，账不生灭）");
    }

    #[test]
    fn probes_and_double_run() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert_eq!(golden_run(), golden_run(), "双跑一致");
    }

    #[test]
    fn cards_consecutive() {
        let mut i = 0;
        while i < ROWS {
            assert_eq!(B03_LEDGER[i].card, 18441 + i as u32, "行 {} 卡号连续", i);
            i += 1;
        }
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18460_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20, "判据条数 {} 在窗内", s.len());
    }
}
