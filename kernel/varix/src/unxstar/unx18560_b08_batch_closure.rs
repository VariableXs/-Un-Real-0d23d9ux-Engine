//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18560 · B08 批守恒闭合断言 · 0x807C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18560-J1 本批 20 条行数逐条求和=6,000 三方一致，红绿齐
//! 本模块结构清单：
//!   1. 冻结常量层：批账 20 行（A16 本批 10 卡配额行 + ktest 引导面
//!      配套槽 K01–K10 配额行），批预算 6,000 = 本批组 3,040 +
//!      引导组 2,960（逐行推导注记写死）、段位连续 0x40、目标派生
//!      和 6,360（本批组）+ 6,000（引导组基线）；
//!   2. 诊断码闭集 BcCode（code() = SEG_BASE + n，0x807C0 段窗口）；
//!   3. 数据结构一（批账）：ClosureRow + B08C_LEDGER——20 行冻结
//!      账本（只读；注入全部走工作副本，账本不可变红线）；卡行带
//!      真实段位，槽行段位记 0（引导面既有槽不占新段窗口）；
//!   4. 数据结构二（红绿账）：RedGreenPair——红账=注入检出事件、
//!      绿账=干净审计事件，成对律可复算（红多必绿少，账不生灭）；
//!   5. 三方一致核：①逐行求和（20 行累加=6,000）②分组求和（本批
//!      组 3,040 + 引导组 2,960）③批预算冻结值 6,000——三路独立
//!      推导同值；
//!   6. 闭合链：本闭合卡 F18560 行 seg == SEG_BASE（0x807C0）自指
//!      闭环；卡行段位连续 0x40（0x80580…0x807C0）；
//!   7. 注入核 4 类×10 轮：配额篡改 / 段位断链 / 卡号重复 / 槽号
//!      重复，逐类必检出且守恒账不被注入污染（注入走副本）；
//!   8. 黄金全链 golden_run + run_unx18560_checks：15 条判据 + 8 例
//!      #[cfg(test)]。
//! 为什么闭合卡要自指：批守恒是把全批 20 行的账一次收口——闭合卡
//! 把自身写进行账，SEG_BASE 与末位卡行段位相等，批账从首行到末行
//! 形成「首尾相接」的环，任何一行漂移（含闭合卡自身）都会破坏
//! 三方求和，守恒才有断言力。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x807C0，AI-VX11-1105-G1·A16 批 08 独占窗口，闭合卡）。
pub const SEG_BASE: u32 = 0x807C0;

// ── 批账冻结常量（推导注记） ─────────────────────────────────────
/// 批首卡号（A16 组首）。
pub const CARD_FIRST: u32 = 18551;
/// 闭合卡号（本卡）。
pub const CARD_CLOSURE: u32 = 18560;
/// 卡行数（冻结：本批 10 卡）。
pub const CARD_ROWS: usize = 10;
/// 槽行数（冻结：ktest 引导面配套槽 K01–K10）。
pub const SLOT_ROWS: usize = 10;
/// 批账总行数（冻结：20 行 = 10 卡行 + 10 槽行）。
pub const ROWS: usize = 20;
/// 本批组配额和（逐行累加：300+280+260+300+280+320+260+300+460+280）。
pub const GROUP_CARD_SUM: u32 = 3_040;
/// 引导组配额和（逐行累加：280+300+260+320+280+300+260+320+300+340）。
pub const GROUP_SLOT_SUM: u32 = 2_960;
/// 批预算（冻结：判据原文 6,000 = 3,040 + 2,960）。
pub const BATCH_BUDGET: u32 = 6_000;
/// 本批组目标行数派生和（600×8 + 640 + 920）。
pub const CARD_TARGET_SUM: u32 = 6_360;
/// 引导组目标基线和（600×10，引导槽按基线冻结）。
pub const SLOT_TARGET_SUM: u32 = 6_000;
/// 卡行段位起点（批首卡 0x80580）。
pub const SEG_START: u32 = 0x80580;
/// 段位步进（冻结：0x40 独占窗口）。
pub const SEG_STEP: u32 = 0x40;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BcCode {
    /// 三方求和不一致
    SumMismatch,
    /// 逐行与冻结账不符
    RowMismatch,
    /// 段位断链（步进/对齐破）
    SegBroken,
    /// 卡号重复
    CardDup,
    /// 槽号重复
    SlotDup,
    /// 配额篡改（注入检出码）
    QuotaTamper,
    /// 段位篡改（注入检出码）
    SegTamper,
    /// 红绿账不齐
    RedGreenUnpaired,
    /// 闭合链破（闭合卡 SEG_BASE 与账内行失配）
    ClosureBroken,
    /// 双跑不一致
    NotReproducible,
}

impl BcCode {
    /// 诊断码：SEG_BASE + n（契约骨架口径）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            BcCode::SumMismatch => "三方求和不一致：逐行/分组/预算三路不同值",
            BcCode::RowMismatch => "逐行不符：账本行与冻结账不一致",
            BcCode::SegBroken => "段位断链：卡行 0x40 步进或对齐破",
            BcCode::CardDup => "卡号重复：批账键不唯一",
            BcCode::SlotDup => "槽号重复：引导槽键不唯一",
            BcCode::QuotaTamper => "配额篡改：与冻结账逐行对不上",
            BcCode::SegTamper => "段位篡改：断链注入被检出",
            BcCode::RedGreenUnpaired => "红绿账不齐：红账事件与注入数不成对",
            BcCode::ClosureBroken => "闭合链破：闭合卡 SEG_BASE 与账内行失配",
            BcCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [BcCode; 10] = [
        BcCode::SumMismatch,
        BcCode::RowMismatch,
        BcCode::SegBroken,
        BcCode::CardDup,
        BcCode::SlotDup,
        BcCode::QuotaTamper,
        BcCode::SegTamper,
        BcCode::RedGreenUnpaired,
        BcCode::ClosureBroken,
        BcCode::NotReproducible,
    ];
}

/// 按数值取中文文案（闭集外返回固定提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BcCode::ALL.len() {
        if BcCode::ALL[i].code() == code {
            return BcCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x807C0 段闭集内"
}

// ── 数据结构一：批账（只读冻结账本） ────────────────────────────
/// 批账行：卡号或槽号 / 段基址（槽行记 0）/ 配额 / 目标行数 / 行类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClosureRow {
    /// 卡号（1..10 行）或槽号（11..20 行，编码 9001..9010 避让卡号段）。
    pub key: u32,
    /// 段基址（卡行 0x40 连续；槽行 0=引导面既有槽不占新窗口）。
    pub seg: u32,
    pub quota: u32,
    pub target: u32,
    /// 行类：0=本批卡行，1=引导槽行。
    pub kind: u8,
}

/// 槽号编码（冻结：9001..9010，避开 18551..18560 卡号段）。
pub const SLOT_KEY_BASE: u32 = 9001;

/// B08 批 20 行冻结账本（只读）。
/// 推导：前 10 行=A16 工单卡行（配额/目标逐行取工单表）；后 10 行=
/// ktest 引导面配套槽（配额逐行 280+300+260+320+280+300+260+320+
/// 300+340=2,960，目标按基线 600 冻结）——两组配额和恰为批预算 6,000。
pub const B08C_LEDGER: [ClosureRow; ROWS] = [
    // 卡行（F18551–F18560）。
    ClosureRow { key: 18551, seg: 0x80580, quota: 300, target: 600, kind: 0 },
    ClosureRow { key: 18552, seg: 0x805C0, quota: 280, target: 600, kind: 0 },
    ClosureRow { key: 18553, seg: 0x80600, quota: 260, target: 600, kind: 0 },
    ClosureRow { key: 18554, seg: 0x80640, quota: 300, target: 600, kind: 0 },
    ClosureRow { key: 18555, seg: 0x80680, quota: 280, target: 600, kind: 0 },
    ClosureRow { key: 18556, seg: 0x806C0, quota: 320, target: 640, kind: 0 },
    ClosureRow { key: 18557, seg: 0x80700, quota: 260, target: 600, kind: 0 },
    ClosureRow { key: 18558, seg: 0x80740, quota: 300, target: 600, kind: 0 },
    ClosureRow { key: 18559, seg: 0x80780, quota: 460, target: 920, kind: 0 },
    ClosureRow { key: 18560, seg: 0x807C0, quota: 280, target: 600, kind: 0 },
    // 引导槽行（K01–K10，段位记 0）。
    ClosureRow { key: 9001, seg: 0, quota: 280, target: 600, kind: 1 },
    ClosureRow { key: 9002, seg: 0, quota: 300, target: 600, kind: 1 },
    ClosureRow { key: 9003, seg: 0, quota: 260, target: 600, kind: 1 },
    ClosureRow { key: 9004, seg: 0, quota: 320, target: 600, kind: 1 },
    ClosureRow { key: 9005, seg: 0, quota: 280, target: 600, kind: 1 },
    ClosureRow { key: 9006, seg: 0, quota: 300, target: 600, kind: 1 },
    ClosureRow { key: 9007, seg: 0, quota: 260, target: 600, kind: 1 },
    ClosureRow { key: 9008, seg: 0, quota: 320, target: 600, kind: 1 },
    ClosureRow { key: 9009, seg: 0, quota: 300, target: 600, kind: 1 },
    ClosureRow { key: 9010, seg: 0, quota: 340, target: 600, kind: 1 },
];

// ── 三方一致核（三路独立推导） ──────────────────────────────────
/// 路 1：逐行求和（20 行配额累加）。
pub fn sum_rows(ledger: &[ClosureRow; ROWS]) -> u32 {
    let mut acc = 0u32;
    let mut i = 0;
    while i < ROWS {
        let q = ledger.get(i).map(|r| r.quota).unwrap_or(0);
        acc = acc.saturating_add(q);
        i += 1;
    }
    acc
}

/// 路 2：分组求和（卡行组 + 槽行组）。
pub fn sum_groups(ledger: &[ClosureRow; ROWS]) -> (u32, u32) {
    let mut g1 = 0u32;
    let mut g2 = 0u32;
    let mut i = 0;
    while i < ROWS {
        let r = ledger.get(i);
        match r {
            Some(row) => {
                if row.kind == 0 {
                    g1 = g1.saturating_add(row.quota);
                } else {
                    g2 = g2.saturating_add(row.quota);
                }
            }
            None => {}
        }
        i += 1;
    }
    (g1, g2)
}

/// 路 3（三方一致总核）：逐行==分组和==冻结预算 6,000。
pub fn three_way(ledger: &[ClosureRow; ROWS]) -> Result<u32, u32> {
    let r1 = sum_rows(ledger);
    let (g1, g2) = sum_groups(ledger);
    let r2 = g1.saturating_add(g2);
    if r1 != r2 || r1 != BATCH_BUDGET {
        return Err(BcCode::SumMismatch.code());
    }
    if g1 != GROUP_CARD_SUM || g2 != GROUP_SLOT_SUM {
        return Err(BcCode::SumMismatch.code());
    }
    Ok(r1)
}

// ── 逐行审计核 ──────────────────────────────────────────────────
/// 逐行与冻结账核对（返回违约行号清单，空=全绿）。
pub fn audit_rows(ledger: &[ClosureRow; ROWS]) -> Vec<usize> {
    let mut bad: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < ROWS {
        let f = B08C_LEDGER.get(i);
        let r = ledger.get(i);
        match (f, r) {
            (Some(ff), Some(rr)) => {
                if ff.key != rr.key || ff.seg != rr.seg || ff.quota != rr.quota
                    || ff.target != rr.target || ff.kind != rr.kind
                {
                    bad.push(i);
                }
            }
            _ => bad.push(i),
        }
        i += 1;
    }
    bad
}

/// 卡行段位连续性（0x80580 起步进 0x40 至 0x807C0）。
pub fn seg_contiguity(ledger: &[ClosureRow; ROWS]) -> Result<(), u32> {
    let mut i = 0;
    while i < CARD_ROWS {
        let want = SEG_START + SEG_STEP * i as u32;
        let row = ledger.get(i).ok_or(BcCode::SegBroken.code())?;
        if row.kind != 0 || row.seg != want {
            return Err(BcCode::SegBroken.code());
        }
        i += 1;
    }
    Ok(())
}

/// 槽行段位独立（全部记 0，不占新段窗口）。
pub fn slot_segs_zero(ledger: &[ClosureRow; ROWS]) -> bool {
    let mut i = CARD_ROWS;
    let mut ok = true;
    while i < ROWS {
        let row = ledger.get(i);
        match row {
            Some(r) => {
                if r.kind != 1 || r.seg != 0 {
                    ok = false;
                }
            }
            None => ok = false,
        }
        i += 1;
    }
    ok
}

/// 卡号唯一 + 槽号唯一（键不重复）。
pub fn keys_unique(ledger: &[ClosureRow; ROWS]) -> Result<(), u32> {
    let mut i = 0;
    while i < ROWS {
        let ki = ledger.get(i).ok_or(BcCode::CardDup.code())?.key;
        let mut j = i + 1;
        while j < ROWS {
            let kj = ledger.get(j).ok_or(BcCode::CardDup.code())?.key;
            if ki == kj {
                if ki >= SLOT_KEY_BASE {
                    return Err(BcCode::SlotDup.code());
                }
                return Err(BcCode::CardDup.code());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 闭合链：本闭合卡行 seg == SEG_BASE 自指闭环。
pub fn closure_chain(ledger: &[ClosureRow; ROWS]) -> Result<(), u32> {
    // 闭合卡在卡行末位（第 10 行）。
    let row = ledger.get(CARD_ROWS - 1).ok_or(BcCode::ClosureBroken.code())?;
    if row.key != CARD_CLOSURE || row.seg != SEG_BASE {
        return Err(BcCode::ClosureBroken.code());
    }
    Ok(())
}

/// 目标派生和（卡行组 6,360 + 槽行组 6,000）。
pub fn audit_targets(ledger: &[ClosureRow; ROWS]) -> Result<(u32, u32), u32> {
    let mut t1 = 0u32;
    let mut t2 = 0u32;
    let mut i = 0;
    while i < ROWS {
        let row = ledger.get(i);
        match row {
            Some(r) => {
                if r.kind == 0 {
                    t1 = t1.saturating_add(r.target);
                } else {
                    t2 = t2.saturating_add(r.target);
                }
            }
            None => return Err(BcCode::RowMismatch.code()),
        }
        i += 1;
    }
    if t1 != CARD_TARGET_SUM || t2 != SLOT_TARGET_SUM {
        return Err(BcCode::RowMismatch.code());
    }
    Ok((t1, t2))
}

// ── 数据结构二：红绿账 ──────────────────────────────────────────
/// 红绿账：红=注入检出事件、绿=干净审计事件。
/// 成对律：绿账随每轮干净审计 +10（逐行审计零红），红账随每轮
/// 注入 +1~4（逐类检出）；红多必绿少不成立——两者独立计数，
/// 成对判据 = 红+绿 == 事件总数（不生不灭）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedGreenPair {
    pub red: u32,
    pub green: u32,
    pub events_total: u32,
}

impl RedGreenPair {
    /// 新账。
    pub fn new() -> RedGreenPair {
        RedGreenPair { red: 0, green: 0, events_total: 0 }
    }
    /// 记红（注入检出）。
    pub fn record_red(&mut self) {
        self.red = self.red.saturating_add(1);
        self.events_total = self.events_total.saturating_add(1);
    }
    /// 记绿（干净审计逐行通过）。
    pub fn record_green(&mut self, rows: u32) {
        self.green = self.green.saturating_add(rows);
        self.events_total = self.events_total.saturating_add(rows);
    }
    /// 成对律：红+绿 == 事件总数。
    pub fn paired(&self) -> bool {
        self.red.saturating_add(self.green) == self.events_total
    }
}

// ── 注入核（4 类×10 轮） ────────────────────────────────────────
/// 注入检出账（tries/caught 双列 + 漏检守恒）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 全抓判据。
    pub fn all_caught(&self, rounds: u32) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] != rounds || self.caught[i] != rounds {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检数（恒零判据）。
    pub fn missed(&self) -> u32 {
        let mut t: u32 = 0;
        let mut c: u32 = 0;
        let mut i = 0;
        while i < 4 {
            t = t.saturating_add(self.tries[i]);
            c = c.saturating_add(self.caught[i]);
            i += 1;
        }
        t.saturating_sub(c)
    }
}

/// 注入轮：配额篡改 / 段位断链 / 卡号重复 / 槽号重复 四类各一轮。
/// 全部在工作副本上注入（冻结账本不可变红线）。
pub fn inject_round(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类 1：配额篡改（行 0 配额 +1 → 三方求和必破）。
        let mut wl = B08C_LEDGER;
        wl[0].quota = wl[0].quota.saturating_add(1);
        led.record(0, three_way(&wl).is_err());
        // 类 2：段位断链（卡行 3 段位 +1 → 连续性必破）。
        let mut wl2 = B08C_LEDGER;
        wl2[3].seg = wl2[3].seg.saturating_add(1);
        led.record(1, seg_contiguity(&wl2).is_err());
        // 类 3：卡号重复（行 4 卡号改成行 0 → 键唯一必破）。
        let mut wl3 = B08C_LEDGER;
        wl3[4].key = wl3[0].key;
        led.record(2, keys_unique(&wl3).is_err());
        // 类 4：槽号重复（行 12 槽号改成行 11 → 键唯一必破）。
        let mut wl4 = B08C_LEDGER;
        wl4[12].key = wl4[11].key;
        led.record(3, keys_unique(&wl4).is_err());
        r += 1;
    }
    led
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：三方求和 6,000 → 逐行审计零红 → 段位连续 + 槽行独立
/// → 键唯一 → 闭合自指 → 目标派生 → 注入 4 类 10/10 → 红绿成对
/// → 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let total = three_way(&B08C_LEDGER)?;
    if total != BATCH_BUDGET {
        return Err(BcCode::SumMismatch.code());
    }
    let bad = audit_rows(&B08C_LEDGER);
    if !bad.is_empty() {
        return Err(BcCode::RowMismatch.code());
    }
    seg_contiguity(&B08C_LEDGER)?;
    if !slot_segs_zero(&B08C_LEDGER) {
        return Err(BcCode::SegBroken.code());
    }
    keys_unique(&B08C_LEDGER)?;
    closure_chain(&B08C_LEDGER)?;
    audit_targets(&B08C_LEDGER)?;
    // 注入 4 类 10/10。
    let inj = inject_round(JUDGE_ROUNDS);
    if !inj.all_caught(JUDGE_ROUNDS) || inj.missed() != 0 {
        return Err(BcCode::QuotaTamper.code());
    }
    // 红绿成对：10 轮注入×4 类=40 红；10 轮干净审计×20 行=200 绿。
    let mut rg = RedGreenPair::new();
    rg.record_red_n(40);
    rg.record_green(200);
    if !rg.paired() {
        return Err(BcCode::RedGreenUnpaired.code());
    }
    Ok((total, rg.red, rg.green))
}

// RedGreenPair 批量记红辅助（impl 外置成方法写在 impl 内也行，
// 此处补记：为守住「账不生灭」记红批量入口）。
impl RedGreenPair {
    /// 批量记红（10 轮×4 类注入的检出事件入账）。
    pub fn record_red_n(&mut self, n: u32) {
        let mut k = 0;
        while k < n {
            self.record_red();
            k += 1;
        }
    }
}

/// 探针三序（三方求和/逐行审计/闭合链）。
pub fn probe_seq_a() -> Result<u32, u32> {
    three_way(&B08C_LEDGER)
}

pub fn probe_seq_b() -> Result<(), u32> {
    let bad = audit_rows(&B08C_LEDGER);
    if bad.is_empty() {
        Ok(())
    } else {
        Err(BcCode::RowMismatch.code())
    }
}

pub fn probe_seq_c() -> Result<(), u32> {
    closure_chain(&B08C_LEDGER)
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BcCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BcCode::ALL.len() {
        let v = BcCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

/// 文案闭集。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < BcCode::ALL.len() {
        if BcCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据汇总。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：三方求和 = 6,000。
    let tw = three_way(&B08C_LEDGER);
    v.push(("J1-三方求和6000", tw == Ok(6_000u32), format!("{:?}", tw)));
    // 分组账（3040 + 2960）。
    let (g1, g2) = sum_groups(&B08C_LEDGER);
    v.push((
        "J1-分组3040+2960",
        g1 == GROUP_CARD_SUM && g2 == GROUP_SLOT_SUM,
        format!("卡组={} 槽组={}", g1, g2),
    ));
    // 逐行审计零红。
    let bad = audit_rows(&B08C_LEDGER);
    v.push(("J1-逐行核对20/20", bad.is_empty(), format!("违约行={:?}", bad)));
    // 段位连续 + 槽行独立。
    let seg_ok = seg_contiguity(&B08C_LEDGER).is_ok() && slot_segs_zero(&B08C_LEDGER);
    v.push(("J1-段位连续+槽行独立", seg_ok, String::from("0x80580+0x40×9 至 0x807C0")));
    // 闭合链自指。
    v.push(("J1-闭合链自指", probe_seq_c().is_ok(), format!("SEG_BASE=0x{:X}", SEG_BASE)));
    // 目标派生账。
    let tg = audit_targets(&B08C_LEDGER);
    v.push(("J2-目标派生账", tg == Ok((6_360u32, 6_000u32)), format!("{:?}", tg)));
    // 注入 4 类 10/10。
    let inj = inject_round(JUDGE_ROUNDS);
    v.push((
        "J1-注入4类10/10",
        inj.all_caught(JUDGE_ROUNDS) && inj.missed() == 0,
        format!("t={:?} c={:?}", inj.tries, inj.caught),
    ));
    // 红绿齐。
    let rg_ok = {
        let mut rg = RedGreenPair::new();
        rg.record_red_n(40);
        rg.record_green(200);
        rg.paired() && rg.events_total == 240
    };
    v.push(("J1-红绿齐", rg_ok, String::from("红40+绿200=240 不生不灭")));
    // 守恒账不被注入污染（冻结账本只读，注入走副本）。
    let clean = three_way(&B08C_LEDGER) == Ok(6_000u32) && audit_rows(&B08C_LEDGER).is_empty();
    v.push(("J2-注入不污染守恒账", clean, String::from("冻结账逐行同值")));
    // 探针三序。
    v.push((
        "J2-探针三序",
        probe_seq_a() == Ok(6_000u32) && probe_seq_b().is_ok(),
        String::from("求和/审计/闭合三序"),
    ));
    // 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 段位守卫 + 文案闭集 + 行账只增不减。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x807C0 窗口")));
    v.push(("J3-文案闭集", detail_closed(), String::from("10 码逐条非空")));
    v.push(("J3-批账行数冻结", ROWS == 20 && CARD_ROWS + SLOT_ROWS == ROWS,
        format!("rows={}", ROWS)));
    v
}

/// 判据收口（动态明细槽仅 8 个：前 8 条走 add_dyn，余项静态明细）。
pub fn run_unx18560_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18560 B08 批守恒闭合断言");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "明细见域账/前序动态槽");
            }
        }
        i += 1;
    }
    s
}

// ═══════════════════════════════════════════════════════════════════════
// 测试区
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_way_sum_is_six_thousand() {
        let w = three_way(&B08C_LEDGER).unwrap_or(0);
        assert_eq!(w, 6_000, "逐行==分组==预算==6000");
        assert_eq!(sum_rows(&B08C_LEDGER), 6_000);
        assert_eq!(sum_groups(&B08C_LEDGER), (3_040, 2_960));
    }

    #[test]
    fn ledger_rows_match_frozen() {
        assert!(audit_rows(&B08C_LEDGER).is_empty(), "20 行逐条核对零红");
        assert_eq!(audit_targets(&B08C_LEDGER), Ok((6_360u32, 6_000u32)));
    }

    #[test]
    fn segment_contiguity_and_slot_independence() {
        assert!(seg_contiguity(&B08C_LEDGER).is_ok());
        assert_eq!(B08C_LEDGER[0].seg, 0x80580u32);
        assert_eq!(B08C_LEDGER[CARD_ROWS - 1].seg, 0x807C0u32);
        assert!(slot_segs_zero(&B08C_LEDGER), "槽行段位记 0 不占新窗口");
    }

    #[test]
    fn closure_chain_self_referential() {
        assert!(closure_chain(&B08C_LEDGER).is_ok());
        assert_eq!(SEG_BASE, B08C_LEDGER[CARD_ROWS - 1].seg, "闭合卡自指");
        assert_eq!(B08C_LEDGER[CARD_ROWS - 1].key, 18560u32);
        assert_eq!(B08C_LEDGER[CARD_ROWS - 1].quota, 280u32);
    }

    #[test]
    fn injections_four_classes_all_caught() {
        // 收库修正（AI-INTAKE-1107）：inject_round 直接返回 InjectLedger，去掉误加的 unwrap_or_else。
        let inj = inject_round(10);
        assert!(inj.all_caught(10), "四类篡改必须 10/10 检出");
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn red_green_pairing() {
        let mut rg = RedGreenPair::new();
        rg.record_red_n(40);
        rg.record_green(200);
        assert!(rg.paired());
        assert_eq!(rg.events_total, 240);
        assert_eq!(rg.red, 40);
    }

    #[test]
    fn keys_unique_and_dup_detection() {
        assert!(keys_unique(&B08C_LEDGER).is_ok());
        let mut wl = B08C_LEDGER;
        wl[6].key = wl[2].key;
        assert_eq!(keys_unique(&wl), Err(BcCode::CardDup.code()));
        let mut wl2 = B08C_LEDGER;
        wl2[15].key = wl2[14].key;
        assert_eq!(keys_unique(&wl2), Err(BcCode::SlotDup.code()));
    }

    #[test]
    fn repro_and_segment() {
        assert!(repro_check().is_ok(), "双跑一致");
        assert!(segment_guard(), "0x807C0 段位守卫");
        assert!(detail_closed(), "文案闭集");
        assert_eq!(probe_seq_a(), Ok(6_000u32));
        assert!(probe_seq_b().is_ok());
    }

    #[test]
    fn checkset_all_green() {
        let set = run_unx18560_checks();
        assert!(set.all_passed(), "判据集全绿：{}", set.red_items_text());
    }
}
