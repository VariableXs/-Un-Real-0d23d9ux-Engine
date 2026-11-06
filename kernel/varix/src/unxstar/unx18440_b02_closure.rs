//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18440 B02 批守恒闭合断言 · 0x7E9C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18440-J1 本批 20 条行数逐条求和=6,000 三方一致，红绿齐
//! 设计要点：
//!   - 批账本冻结：B02 批 20 卡（F18421–F18440）逐行 (卡号, SEG, 配额,
//!     目标行数) 四元组账，配额取自派工卡 A03/A04 工单——账本只读，
//!     任何注入不许写回（账本不可变红线）；
//!   - 三方一致：①逐行求和（20 行累加）②分组求和（A03 组 3040 +
//!     A04 组 2960）③批预算冻结值 6000——三路独立推导同值；
//!   - 派生账：目标行数和 6240+6240=12480（两组恰等，派生注记）；
//!     段位连续性：0x7E500 起步进 0x40 共 20 卡至 0x7E9C0；
//!   - 红绿齐：红账=注入检出事件账，绿账=干净审计事件账——干净跑
//!     零红、注入跑逐类必红且红账计数==注入轮数（10/10），红绿两账
//!     成对可复算（红多必绿少，账不凭空生灭）；
//!   - 注入核四类×10 轮：配额篡改（±1）/目标篡改/段位断链/重复卡号
//!     ——四类全检出，且守恒账（6000）不受注入污染；
//!   - 确定性：账本全部写死常量，无随机；no_std+alloc；零 unwrap/
//!     expect/panic；禁浮点/IO/墙钟/rand。
//! 结构清单：
//!   BatchRow(行四元组) / B02_LEDGER(20 行冻结账本) / ClosureCode
//!   / sum_rows/sum_groups(三路求和) / seg_contiguity(段位连续)
//!   / audit_row+audit_all(审计面) / red_green_ledgers(红绿齐账)
//!   / InjectLedger / golden_run / run_unx18440_checks / tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E9C0，AI-VX11-1105-G1·A04 独占区）。
pub const SEG_BASE: u32 = 0x7E9C0;

// ── 批账冻结常量（推导注记） ─────────────────────────────────────
/// 批首卡号（A03 组首）。
pub const CARD_FIRST: u32 = 18421;
/// 批末卡号（A04 组末）。
pub const CARD_LAST: u32 = 18440;
/// 批行数（冻结：20 卡）。
pub const ROWS: usize = 20;
/// A03 组行数（冻结）。
pub const GROUP_A03: usize = 10;
/// 批预算（冻结：判据原文 6,000）。
pub const BATCH_BUDGET: u32 = 6_000;
/// A03 组配额和（逐行累加：320+300+340+280+300+260+340+320+280+300）。
pub const GROUP_A03_SUM: u32 = 3_040;
/// A04 组配额和（逐行累加：340+260+300+320+280+300+260+300+360+240）。
pub const GROUP_A04_SUM: u32 = 2_960;
/// 目标行数派生和（A03 组 6240 + A04 组 6240，两组恰等——派生注记）。
pub const TARGET_TOTAL: u32 = 12_480;
/// 段位起点（A03 组首卡）。
pub const SEG_START: u32 = 0x7E500;
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

/// B02 批 20 行冻结账本（只读；配额/目标取自派工卡 A03/A04 工单）。
pub const B02_LEDGER: [BatchRow; ROWS] = [
    // A03 组（F18421–F18430）。
    BatchRow { card: 18421, seg: 0x7E500, quota: 320, target: 640 },
    BatchRow { card: 18422, seg: 0x7E540, quota: 300, target: 600 },
    BatchRow { card: 18423, seg: 0x7E580, quota: 340, target: 680 },
    BatchRow { card: 18424, seg: 0x7E5C0, quota: 280, target: 600 },
    BatchRow { card: 18425, seg: 0x7E600, quota: 300, target: 600 },
    BatchRow { card: 18426, seg: 0x7E640, quota: 260, target: 600 },
    BatchRow { card: 18427, seg: 0x7E680, quota: 340, target: 680 },
    BatchRow { card: 18428, seg: 0x7E6C0, quota: 320, target: 640 },
    BatchRow { card: 18429, seg: 0x7E700, quota: 280, target: 600 },
    BatchRow { card: 18430, seg: 0x7E740, quota: 300, target: 600 },
    // A04 组（F18431–F18440）。
    BatchRow { card: 18431, seg: 0x7E780, quota: 340, target: 680 },
    BatchRow { card: 18432, seg: 0x7E7C0, quota: 260, target: 600 },
    BatchRow { card: 18433, seg: 0x7E800, quota: 300, target: 600 },
    BatchRow { card: 18434, seg: 0x7E840, quota: 320, target: 640 },
    BatchRow { card: 18435, seg: 0x7E880, quota: 280, target: 600 },
    BatchRow { card: 18436, seg: 0x7E8C0, quota: 300, target: 600 },
    BatchRow { card: 18437, seg: 0x7E900, quota: 260, target: 600 },
    BatchRow { card: 18438, seg: 0x7E940, quota: 300, target: 600 },
    BatchRow { card: 18439, seg: 0x7E980, quota: 360, target: 720 },
    BatchRow { card: 18440, seg: 0x7E9C0, quota: 240, target: 600 },
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosureCode {
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
    /// 双跑不一致
    NotReproducible,
}

impl ClosureCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ClosureCode::SumMismatch => "三方求和不一致：逐行/分组/预算三路不同值",
            ClosureCode::RowMismatch => "逐行不符：账本行与冻结派工账不一致",
            ClosureCode::SegBroken => "段位断链：0x40 步进或 0x40 对齐破",
            ClosureCode::CardDup => "卡号重复：批账键不唯一",
            ClosureCode::QuotaTamper => "配额篡改：与冻结账逐行对不上",
            ClosureCode::TargetTamper => "目标行数篡改：与冻结账逐行对不上",
            ClosureCode::SegTamper => "段位篡改：断链注入被检出",
            ClosureCode::LedgerMutated => "账本被写回：只读红线",
            ClosureCode::RedGreenUnpaired => "红绿账不齐：红账事件与注入数不成对",
            ClosureCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [ClosureCode; 10] = [
        ClosureCode::SumMismatch,
        ClosureCode::RowMismatch,
        ClosureCode::SegBroken,
        ClosureCode::CardDup,
        ClosureCode::QuotaTamper,
        ClosureCode::TargetTamper,
        ClosureCode::SegTamper,
        ClosureCode::LedgerMutated,
        ClosureCode::RedGreenUnpaired,
        ClosureCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ClosureCode::ALL.len() {
        if ClosureCode::ALL[i].val() == code {
            return ClosureCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E9C0 段闭集内"
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

/// 路 2：分组求和（前 10 行 A03 组 + 后 10 行 A04 组）。
pub fn sum_groups(ledger: &[BatchRow; ROWS]) -> (u32, u32) {
    let mut g1 = 0u32;
    let mut g2 = 0u32;
    let mut i = 0;
    while i < ROWS {
        if i < GROUP_A03 {
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
        return Err(ClosureCode::SumMismatch.val());
    }
    if w1 != w2 || w2 != w3 {
        return Err(ClosureCode::SumMismatch.val());
    }
    Ok(w1)
}

/// 目标行数派生账：两组各 6240，合计 12480。
pub fn target_total(ledger: &[BatchRow; ROWS]) -> Result<(u32, u32, u32), u32> {
    let mut t1 = 0u32;
    let mut t2 = 0u32;
    let mut i = 0;
    while i < ROWS {
        if i < GROUP_A03 {
            t1 = t1.saturating_add(ledger[i].target);
        } else {
            t2 = t2.saturating_add(ledger[i].target);
        }
        i += 1;
    }
    if t1.saturating_add(t2) != TARGET_TOTAL {
        return Err(ClosureCode::SumMismatch.val());
    }
    Ok((t1, t2, t1.saturating_add(t2)))
}

/// 段位连续性：步进 0x40 × 20 卡，0x40 对齐，卡号连续。
pub fn seg_contiguity(ledger: &[BatchRow; ROWS]) -> Result<(), u32> {
    let mut i = 0;
    while i < ROWS {
        let r = &ledger[i];
        // 段位 = 起点 + 步进×i。
        if r.seg != SEG_START + SEG_STEP * (i as u32) {
            return Err(ClosureCode::SegBroken.val());
        }
        // 0x40 对齐。
        if r.seg % 0x40 != 0 {
            return Err(ClosureCode::SegBroken.val());
        }
        // 卡号连续。
        if r.card != CARD_FIRST + (i as u32) {
            return Err(ClosureCode::CardDup.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 审计面（逐行核对冻结账） ────────────────────────────────────
/// 逐行审计：账本行与 B02_LEDGER 冻结行全字段一致。
/// 返回红行下标集（干净账本必为空）。
pub fn audit_all(ledger: &[BatchRow; ROWS]) -> Vec<usize> {
    let mut red: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < ROWS {
        let f = &B02_LEDGER[i];
        let r = &ledger[i];
        if r.card != f.card || r.seg != f.seg || r.quota != f.quota || r.target != f.target {
            red.push(i);
        }
        i += 1;
    }
    red
}

/// 全账审计：逐行 + 三方 + 段位连续。返回红码集（空=全绿）。
pub fn audit_full(ledger: &[BatchRow; ROWS]) -> Vec<u32> {
    let mut red: Vec<u32> = Vec::new();
    if three_way(ledger).is_err() {
        red.push(ClosureCode::SumMismatch.val());
    }
    if seg_contiguity(ledger).is_err() {
        red.push(ClosureCode::SegBroken.val());
    }
    if audit_all(ledger).len() > 0 {
        red.push(ClosureCode::RowMismatch.val());
    }
    // 卡号唯一（成对比较）。
    let mut i = 0;
    while i < ROWS {
        let mut j = i + 1;
        while j < ROWS {
            if ledger[i].card == ledger[j].card {
                red.push(ClosureCode::CardDup.val());
            }
            j += 1;
        }
        i += 1;
    }
    red
}

// ── 红绿账（红绿齐） ────────────────────────────────────────────
/// 红绿账对：红账=注入检出事件，绿账=干净审计事件。
/// 成对律：红账计数 == 注入轮数 ∧ 绿账计数 == 0（干净跑零红）。
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

// ── 注入核（四类 ×10 轮） ───────────────────────────────────────
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
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：配额篡改（行 5 配额 +1）。
        let mut l0 = B02_LEDGER;
        l0[5].quota = l0[5].quota.saturating_add(1);
        let red0 = audit_full(&l0);
        let hit0 = red0.contains(&ClosureCode::RowMismatch.val())
            && red0.contains(&ClosureCode::SumMismatch.val());
        led.record(0, hit0);
        // 槽 1：目标行数篡改（行 11 目标 −1）。
        let mut l1 = B02_LEDGER;
        l1[11].target = l1[11].target.saturating_sub(1);
        let red1 = audit_full(&l1);
        let hit1 = red1.contains(&ClosureCode::RowMismatch.val());
        led.record(1, hit1);
        // 槽 2：段位断链（行 19 段基址 +0x80 → 步进破）。
        let mut l2 = B02_LEDGER;
        l2[19].seg = l2[19].seg.saturating_add(0x80);
        let red2 = audit_full(&l2);
        let hit2 = red2.contains(&ClosureCode::SegBroken.val());
        led.record(2, hit2);
        // 槽 3：重复卡号（行 3 卡号改成行 2 的）。
        let mut l3 = B02_LEDGER;
        l3[3].card = l3[2].card;
        let red3 = audit_full(&l3);
        let hit3 = red3.contains(&ClosureCode::CardDup.val());
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：干净账本三方一致 + 段位连续 + 审计零红。
pub fn probe_seq_a() -> Result<(), u32> {
    three_way(&B02_LEDGER)?;
    seg_contiguity(&B02_LEDGER)?;
    if audit_all(&B02_LEDGER).len() != 0 {
        return Err(ClosureCode::RowMismatch.val());
    }
    Ok(())
}

/// 负探针 1：配额篡改副本审计必红（冻结账本本体不受扰）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = B02_LEDGER;
    l[0].quota = l[0].quota.saturating_add(1);
    let red = audit_all(&l);
    if red.len() != 1 || red[0] != 0 {
        return Err(ClosureCode::QuotaTamper.val());
    }
    // 本体未被写回。
    if B02_LEDGER[0].quota != 320 {
        return Err(ClosureCode::LedgerMutated.val());
    }
    Ok(())
}

/// 负探针 2：预算冻结值与账本无关性（改不了）。
pub fn probe_seq_c() -> Result<(), u32> {
    if budget_frozen() != 6_000 {
        return Err(ClosureCode::SumMismatch.val());
    }
    Ok(())
}

/// 黄金全链：三方一致 + 段位连续 + 派生账 + 注入全抓 + 红绿齐 + 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let sum = three_way(&B02_LEDGER)?;
    seg_contiguity(&B02_LEDGER)?;
    let (_, _, tt) = target_total(&B02_LEDGER)?;
    if audit_all(&B02_LEDGER).len() != 0 {
        return Err(ClosureCode::RowMismatch.val());
    }
    // 干净全审计零红。
    if audit_full(&B02_LEDGER).len() != 0 {
        return Err(ClosureCode::RowMismatch.val());
    }
    // 注入 10 轮全抓。
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() {
        return Err(ClosureCode::NotReproducible.val());
    }
    // 红绿齐：红账 4 类 ×10 轮 = 40 事件，绿账 0。
    let red_total = inj.caught[0] + inj.caught[1] + inj.caught[2] + inj.caught[3];
    let pair = RedGreenPair { red_events: red_total, green_events: 0 };
    if !pair.paired(40) {
        return Err(ClosureCode::RedGreenUnpaired.val());
    }
    // 注入不许污染守恒账：三方求和仍 6000。
    let sum_after = three_way(&B02_LEDGER)?;
    if sum_after != sum {
        return Err(ClosureCode::LedgerMutated.val());
    }
    Ok((sum, tt, red_total))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ClosureCode::ALL.len() {
        let v = ClosureCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18440_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18440 B02 批守恒闭合");
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
    // ② 三方一致：逐行 == 分组 == 预算 == 6000。
    let w = three_way(&B02_LEDGER);
    let tw_ok = matches!(w, Ok(x) if x == BATCH_BUDGET);
    s.add_dyn(
        "三方求和=6000",
        tw_ok,
        &format!(
            "逐行{} 分组{:?} 预算{}",
            sum_rows(&B02_LEDGER),
            sum_groups(&B02_LEDGER),
            budget_frozen()
        ),
    );
    // ③ 分组账：A03 3040 / A04 2960。
    let (g1, g2) = sum_groups(&B02_LEDGER);
    s.add("分组3040+2960", g1 == GROUP_A03_SUM && g2 == GROUP_A04_SUM, "两组逐行累加复算");
    // ④ 逐行核对冻结派工账 20/20。
    s.add("逐行核对20/20", audit_all(&B02_LEDGER).len() == 0, "卡号/SEG/配额/目标四元组");
    // ⑤ 段位连续：0x7E500 起步进 0x40 至 0x7E9C0。
    s.add("段位连续20卡", seg_contiguity(&B02_LEDGER).is_ok(), "0x40 步进+0x40 对齐");
    // ⑥ 目标行数派生账 12480（两组恰等 6240）。
    let tt = target_total(&B02_LEDGER);
    s.add_dyn(
        "目标行数账12480",
        matches!(tt, Ok((t1, t2, _)) if t1 == t2 && t1 + t2 == TARGET_TOTAL),
        &format!("{:?}", tt),
    );
    // ⑦ 注入四类 10/10。
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
    s.add("注入漏检为零", match &inj {
        Ok(l) => l.missed() == 0,
        Err(_) => false,
    }, "检出+漏检=注入");
    // ⑧ 红绿齐：红账 40 事件成对，绿账 0。
    s.add("红绿齐", g.is_ok(), "红账=4×10 事件 绿账=0");
    // ⑨ 守恒账不被注入污染（账本只读红线）。
    s.add("守恒账不被注入污染", B02_LEDGER[0].quota == 320 && B02_LEDGER[19].seg == 0x7E9C0, "冻结账本本体不可变");
    // ⑩ 探针三序。
    s.add("探针三序", probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(), "正/篡改副本/预算冻结");
    // ⑪ 双跑一致。
    s.add("双跑一致", g == golden_run(), "纯常量账无随机");
    // ⑫ 文案闭集 + 段位守卫。
    let mut closed = true;
    let mut ci = 0;
    while ci < ClosureCode::ALL.len() {
        if ClosureCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "10 码全有中文建议");
    s.add("段位守卫 0x7E9C0", segment_guard(), "SEG_BASE 对齐 0x40 窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_way_sum_is_six_thousand() {
        let w = three_way(&B02_LEDGER).unwrap_or(0);
        assert_eq!(w, 6_000, "逐行==分组==预算==6000");
        assert_eq!(sum_rows(&B02_LEDGER), 6_000);
        assert_eq!(sum_groups(&B02_LEDGER), (3_040, 2_960));
    }

    #[test]
    fn ledger_rows_match_dispatch() {
        assert_eq!(audit_all(&B02_LEDGER).len(), 0, "20 行逐条核对零红");
        assert_eq!(audit_full(&B02_LEDGER).len(), 0, "全账审计零红");
    }

    #[test]
    fn segment_contiguity_frozen() {
        assert!(seg_contiguity(&B02_LEDGER).is_ok());
        assert_eq!(B02_LEDGER[0].seg, 0x7E500);
        assert_eq!(B02_LEDGER[ROWS - 1].seg, 0x7E9C0);
        assert_eq!(B02_LEDGER[ROWS - 1].seg - B02_LEDGER[0].seg, SEG_STEP * (ROWS as u32 - 1));
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
        // 注入全部走工作副本——冻结账本本体逐字段不变（行 5=F18426 配额 260）。
        assert_eq!(B02_LEDGER[5].quota, 260);
        assert_eq!(B02_LEDGER[11].target, 600);
        assert_eq!(B02_LEDGER[19].seg, 0x7E9C0);
        assert_eq!(B02_LEDGER[3].card, 18424);
    }

    #[test]
    fn target_total_derived() {
        let tt = target_total(&B02_LEDGER).unwrap_or((0, 0, 0));
        assert_eq!(tt, (6_240, 6_240, 12_480), "两组目标行数恰等");
    }

    #[test]
    fn red_green_pair_accounting() {
        let inj = inject_rounds(10).unwrap_or_else(|_| InjectLedger::new());
        let red_total = inj.caught[0] + inj.caught[1] + inj.caught[2] + inj.caught[3];
        let pair = RedGreenPair { red_events: red_total, green_events: 0 };
        assert!(pair.paired(40), "红账 40 事件绿账 0 成对");
        let clean_pair = RedGreenPair { red_events: 0, green_events: 0 };
        assert!(clean_pair.paired(0), "干净跑红绿两账皆空");
    }

    #[test]
    fn probes_and_double_run() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert_eq!(golden_run(), golden_run(), "双跑一致");
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18440_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20);
    }
}
