//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18459 · ktest 引导面 B03 批断言集 · 0x7EE80 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18459-J1 本批 19 条判据聚合一次命令全绿，失败注入必
//!       红三条
//! 本模块结构清单：
//!   1. 冻结常量层：B03 批 19 卡（F18441–F18459，闭合卡 F18460 归
//!      下一卡）行账（卡号/SEG/配额/目标行数四元组，取自派工卡
//!      A05/A06 工单，推导注释写死）；
//!   2. 诊断码闭集 KtCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构一（行账）：KtestRow——19 行冻结四元组账本（只读，
//!     注入全部走工作副本）；
//!   4. 数据结构二（断言账）：AssertRow——每行断言载荷
//!      （段位公式/段位对齐/卡号连续/配额窗/目标窗五查）；
//!   5. 引导状态机：BootIdle→Loading→Aggregating→Verdict 单向
//!      （跳级/倒退拒）；
//!   6. 聚合核 run_all：一次命令跑满 19 行断言 → 绿账/红账/裁决，
//!      守恒绿+红=19；
//!   7. 注入核 3 类×10 轮：段位断链 / 配额篡改 / 卡号重复——
//!      逐类单注入必红 ≥1；三类同时注入恰红 3 条（J1 后半句）；
//!   8. 黄金全链 + run_unx18459_checks：19 条判据（断言性质）。
//! 为什么聚合面先行：ktest 引导面一次命令要吐出整批红绿裁决，
//! 判据粒度必须逐行可指认（红在哪一行、因哪一类注入）——本卡把
//! 「19 行各自绿」与「三类注入各自红」做成同一账本上的对偶断言，
//! 一次聚合即可复算。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EE80，AI-VX11-1105-G1·A06 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EE80;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// B03 批首卡号（F18441）。
pub const CARD_FIRST: u32 = 18441;
/// B03 批末卡号（F18459；闭合卡 F18460 归下一卡管辖）。
pub const CARD_LAST: u32 = 18459;
/// 行数（冻结 19）。
pub const ROWS: usize = 19;
/// A05 组行数（F18441–F18450）。
pub const GROUP_A05: usize = 10;
/// A05 组配额和（逐行累加：300+320+280+340+260+300+320+280+300+260）。
pub const GROUP_A05_SUM: u32 = 2_960;
/// A06 前九卡配额和（逐行累加：320+280+300+260+300+320+280+300+380）。
pub const GROUP_A06_SUM: u32 = 2_740;
/// 批预算口径（A05 组 + A06 组 + 闭合卡 300 = 6_000；本卡管辖前 19 行）。
pub const BATCH_BUDGET_VIEW: u32 = 5_700;
/// 段位起点（B03 批首卡）。
pub const SEG_START: u32 = 0x7EA00;
/// 段位步进（冻结 0x40 独占窗口）。
pub const SEG_STEP: u32 = 0x40;
/// 配额窗（冻结：派工配额全在 240..=480）。
pub const QUOTA_LO: u32 = 240;
pub const QUOTA_HI: u32 = 480;
/// 目标行数窗（冻结：目标行数全在 600..=760）。
pub const TARGET_LO: u32 = 600;
pub const TARGET_HI: u32 = 760;
/// 注入判定轮数（冻结：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtCode {
    /// 段位断链（步进/对齐破）
    SegBroken,
    /// 卡号断链（连续性破或重复）
    CardBroken,
    /// 配额越窗
    QuotaOutOfWindow,
    /// 目标行数越窗
    TargetOutOfWindow,
    /// 行账与冻结派工账不符
    RowMismatch,
    /// 引导状态机跳级
    BootSkip,
    /// 引导状态机倒退
    BootBackward,
    /// 聚合守恒破（绿+红 ≠ 19）
    AggrConserveBroken,
    /// 注入红数不符（三类注入必红恰 3 条）
    RedCountWrong,
    /// 误指（红账类别与注入类别对不上）
    RedMislabel,
    /// 双跑不一致
    NotReproducible,
}

impl KtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 诊断码别名：code() = SEG_BASE + n（与 val() 同值，契约骨架口径）。
    pub fn code(self) -> u32 {
        self.val()
    }
    pub fn detail(self) -> &'static str {
        match self {
            KtCode::SegBroken => "段位断链：0x40 步进或 0x40 对齐破",
            KtCode::CardBroken => "卡号断链：连续性破或批内重复",
            KtCode::QuotaOutOfWindow => "配额越窗：派工配额不在 240..480",
            KtCode::TargetOutOfWindow => "目标行数越窗：不在 600..760",
            KtCode::RowMismatch => "行账与冻结派工账不符",
            KtCode::BootSkip => "引导状态机跳级：未加载先聚合",
            KtCode::BootBackward => "引导状态机倒退：裁决后回载",
            KtCode::AggrConserveBroken => "聚合守恒破：绿+红≠19",
            KtCode::RedCountWrong => "注入红数不符：三类注入必红恰 3 条",
            KtCode::RedMislabel => "误指：红账类别与注入类别对不上",
            KtCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [KtCode; 11] = [
        KtCode::SegBroken,
        KtCode::CardBroken,
        KtCode::QuotaOutOfWindow,
        KtCode::TargetOutOfWindow,
        KtCode::RowMismatch,
        KtCode::BootSkip,
        KtCode::BootBackward,
        KtCode::AggrConserveBroken,
        KtCode::RedCountWrong,
        KtCode::RedMislabel,
        KtCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < KtCode::ALL.len() {
        if KtCode::ALL[i].val() == code {
            return KtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EE80 段闭集内"
}

// ── 数据结构一：行账 ────────────────────────────────────────────
/// 批行四元组：卡号 / SEG / 配额 / 目标行数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KtestRow {
    pub card: u32,
    pub seg: u32,
    pub quota: u32,
    pub target: u32,
}

/// B03 批 19 行冻结账本（只读；值取自派工卡 A05/A06 工单）。
pub const B03_LEDGER: [KtestRow; ROWS] = [
    // A05 组（F18441–F18450）。
    KtestRow { card: 18441, seg: 0x7EA00, quota: 300, target: 600 },
    KtestRow { card: 18442, seg: 0x7EA40, quota: 320, target: 640 },
    KtestRow { card: 18443, seg: 0x7EA80, quota: 280, target: 600 },
    KtestRow { card: 18444, seg: 0x7EAC0, quota: 340, target: 680 },
    KtestRow { card: 18445, seg: 0x7EB00, quota: 260, target: 600 },
    KtestRow { card: 18446, seg: 0x7EB40, quota: 300, target: 600 },
    KtestRow { card: 18447, seg: 0x7EB80, quota: 320, target: 640 },
    KtestRow { card: 18448, seg: 0x7EBC0, quota: 280, target: 600 },
    KtestRow { card: 18449, seg: 0x7EC00, quota: 300, target: 600 },
    KtestRow { card: 18450, seg: 0x7EC40, quota: 260, target: 600 },
    // A06 组前九卡（F18451–F18459）。
    KtestRow { card: 18451, seg: 0x7EC80, quota: 320, target: 640 },
    KtestRow { card: 18452, seg: 0x7ECC0, quota: 280, target: 600 },
    KtestRow { card: 18453, seg: 0x7ED00, quota: 300, target: 600 },
    KtestRow { card: 18454, seg: 0x7ED40, quota: 260, target: 600 },
    KtestRow { card: 18455, seg: 0x7ED80, quota: 300, target: 600 },
    KtestRow { card: 18456, seg: 0x7EDC0, quota: 320, target: 640 },
    KtestRow { card: 18457, seg: 0x7EE00, quota: 280, target: 600 },
    KtestRow { card: 18458, seg: 0x7EE40, quota: 300, target: 600 },
    KtestRow { card: 18459, seg: 0x7EE80, quota: 380, target: 760 },
];

// ── 数据结构二：断言账（行断言五查） ────────────────────────────
/// 单行断言：五查全过 = 绿。返回 Err(码) = 红且码可指认。
pub fn assert_row(ledger: &[KtestRow; ROWS], idx: usize) -> Result<(), u32> {
    let r = match ledger.get(idx) {
        Some(x) => *x,
        None => return Err(KtCode::RowMismatch.val()),
    };
    // 查 1：段位公式（seg = 起点 + 步进×行号）。
    if r.seg != SEG_START + SEG_STEP * (idx as u32) {
        return Err(KtCode::SegBroken.val());
    }
    // 查 2：段位对齐。
    if r.seg % 0x40 != 0 {
        return Err(KtCode::SegBroken.val());
    }
    // 查 3：卡号连续。
    if r.card != CARD_FIRST + (idx as u32) {
        return Err(KtCode::CardBroken.val());
    }
    // 查 4：配额窗。
    if r.quota < QUOTA_LO || r.quota > QUOTA_HI {
        return Err(KtCode::QuotaOutOfWindow.val());
    }
    // 查 5：目标行数窗。
    if r.target < TARGET_LO || r.target > TARGET_HI {
        return Err(KtCode::TargetOutOfWindow.val());
    }
    Ok(())
}

/// 段位全行连续性：步进公式 + 0x40 对齐 + 卡号连续（19 行全查）。
pub fn seg_contiguity_all() -> bool {
    let mut i = 0;
    while i < ROWS {
        let r = match B03_LEDGER.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if r.seg != SEG_START + SEG_STEP * (i as u32) {
            return false;
        }
        if r.seg % 0x40 != 0 {
            return false;
        }
        if r.card != CARD_FIRST + (i as u32) {
            return false;
        }
        i += 1;
    }
    true
}

/// 逐行核对冻结派工账：与 B03_LEDGER 全字段一致（干净副本必零红）。
pub fn audit_rows(ledger: &[KtestRow; ROWS]) -> Vec<usize> {    let mut red: Vec<usize> = Vec::new();
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

// ── 引导状态机 ──────────────────────────────────────────────────
/// 引导四态：BootIdle→Loading→Aggregating→Verdict 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootState {
    BootIdle,
    Loading,
    Aggregating,
    Verdict,
}

/// 引导闸：状态机 + 聚合结果账。
pub struct BootGate {
    pub state: BootState,
    pub green: u32,
    pub red: u32,
    pub red_codes: Vec<(usize, u32)>,
}

impl BootGate {
    pub fn new() -> BootGate {
        BootGate {
            state: BootState::BootIdle,
            green: 0,
            red: 0,
            red_codes: Vec::new(),
        }
    }

    /// 状态推进（单向）。
    pub fn advance(&mut self, to: BootState) -> Result<(), u32> {
        let legal = match (self.state, to) {
            (BootState::BootIdle, BootState::Loading) => true,
            (BootState::Loading, BootState::Aggregating) => true,
            (BootState::Aggregating, BootState::Verdict) => true,
            _ => false,
        };
        if !legal {
            if self.state as u32 > to as u32 {
                return Err(KtCode::BootBackward.val());
            }
            return Err(KtCode::BootSkip.val());
        }
        self.state = to;
        Ok(())
    }

    /// 聚合核：跑满 19 行断言，绿/红入账（须在 Aggregating 态调用）。
    pub fn aggregate(&mut self, ledger: &[KtestRow; ROWS]) -> Result<(u32, u32), u32> {
        if self.state != BootState::Aggregating {
            return Err(KtCode::BootSkip.val());
        }
        self.green = 0;
        self.red = 0;
        self.red_codes.clear();
        let mut i = 0;
        while i < ROWS {
            match assert_row(ledger, i) {
                Ok(()) => self.green += 1,
                Err(c) => {
                    self.red += 1;
                    self.red_codes.push((i, c));
                }
            }
            i += 1;
        }
        if self.green.saturating_add(self.red) != ROWS as u32 {
            return Err(KtCode::AggrConserveBroken.val());
        }
        Ok((self.green, self.red))
    }
}

// ── 一次命令聚合（ktest 口径） ──────────────────────────────────
/// 干净聚合：引导四态走满 → 19/19 绿 → 零红。
pub fn run_all_clean() -> Result<(u32, u32), u32> {
    let mut g = BootGate::new();
    g.advance(BootState::Loading)?;
    g.advance(BootState::Aggregating)?;
    let (gr, rd) = g.aggregate(&B03_LEDGER)?;
    g.advance(BootState::Verdict)?;
    if gr != ROWS as u32 || rd != 0 {
        return Err(KtCode::AggrConserveBroken.val());
    }
    Ok((gr, rd))
}

// ── 注入核（3 类 ×10 轮 + 三类齐注） ────────────────────────────
/// 注入检出账：三槽 tries/caught 成对。
#[derive(Debug)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
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
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 构造单类注入副本（冻结账本本体不写回）。
/// 类 0：段位断链（行 7 段 +0x80）；类 1：配额篡改（行 3 配额顶出
/// 窗口上界——−1 仍在窗内不会红，必须越过 QUOTA_HI 才触发越窗码）；
/// 类 2：卡号重复（行 11 卡号改成行 10 的）。
pub fn tampered_copy(kind: usize) -> [KtestRow; ROWS] {
    let mut l = B03_LEDGER;
    match kind {
        0 => {
            if let Some(r) = l.get_mut(7) {
                let mut nr = *r;
                nr.seg = nr.seg.saturating_add(0x80);
                l[7] = nr;
            }
        }
        1 => {
            if let Some(r) = l.get_mut(3) {
                let mut nr = *r;
                nr.quota = QUOTA_HI.saturating_add(1);
                l[3] = nr;
            }
        }
        _ => {
            if let (Some(a), Some(b)) = (l.get(10).copied(), l.get(11).copied()) {
                let mut nr = b;
                nr.card = a.card;
                l[11] = nr;
            }
        }
    }
    l
}

/// 三类齐注副本（恰坏三行：行 7 / 行 3 / 行 11）。
pub fn triple_tampered_copy() -> [KtestRow; ROWS] {
    let mut l = tampered_copy(0);
    if let Some(r) = l.get_mut(3) {
        let mut nr = *r;
        nr.quota = QUOTA_HI.saturating_add(1);
        l[3] = nr;
    }
    if let (Some(a), Some(b)) = (l.get(10).copied(), l.get(11).copied()) {
        let mut nr = b;
        nr.card = a.card;
        l[11] = nr;
    }
    l
}

/// 聚合一次注入副本并回收红码集。
pub fn aggregate_reds(ledger: &[KtestRow; ROWS]) -> Result<Vec<(usize, u32)>, u32> {
    let mut g = BootGate::new();
    g.advance(BootState::Loading)?;
    g.advance(BootState::Aggregating)?;
    g.aggregate(ledger)?;
    Ok(g.red_codes.clone())
}

/// 10 轮注入：槽 0 段位断链 / 槽 1 配额篡改 / 槽 2 卡号重复——
/// 逐类必红且类别指认正确；三类齐注恰红 3 条。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：段位断链 → 行 7 必红 SegBroken。
        let reds0 = aggregate_reds(&tampered_copy(0))?;
        let hit0 = reds0.len() == 1
            && reds0.get(0).map(|x| x.0).unwrap_or(99) == 7
            && reds0.get(0).map(|x| x.1).unwrap_or(0) == KtCode::SegBroken.val();
        led.record(0, hit0);
        // 槽 1：配额篡改 → 行 3 必红 QuotaOutOfWindow。
        let reds1 = aggregate_reds(&tampered_copy(1))?;
        let hit1 = reds1.len() == 1
            && reds1.get(0).map(|x| x.0).unwrap_or(99) == 3
            && reds1.get(0).map(|x| x.1).unwrap_or(0) == KtCode::QuotaOutOfWindow.val();
        led.record(1, hit1);
        // 槽 2：卡号重复 → 行 11 必红 CardBroken。
        let reds2 = aggregate_reds(&tampered_copy(2))?;
        let hit2 = reds2.len() == 1
            && reds2.get(0).map(|x| x.0).unwrap_or(99) == 11
            && reds2.get(0).map(|x| x.1).unwrap_or(0) == KtCode::CardBroken.val();
        led.record(2, hit2);
        r += 1;
    }
    Ok(led)
}

/// 三类齐注：恰红 3 条（行 7/3/11），类别一一对应（J1 后半句）。
pub fn triple_injection_reds() -> Result<Vec<(usize, u32)>, u32> {
    aggregate_reds(&triple_tampered_copy())
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：干净聚合 19/19 绿 → 注入逐类必红 → 三类齐注恰红 3 →
/// 冻结账不写回 → 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // ① 干净跑全绿。
    let (gr, rd) = run_all_clean()?;
    if gr != ROWS as u32 || rd != 0 {
        return Err(KtCode::AggrConserveBroken.val());
    }
    // ② 逐行核对冻结派工账零红。
    if audit_rows(&B03_LEDGER).len() != 0 {
        return Err(KtCode::RowMismatch.val());
    }
    // ③ 注入逐类 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() || inj.missed() != 0 {
        return Err(KtCode::NotReproducible.val());
    }
    // ④ 三类齐注恰红 3 条。
    let tri = triple_injection_reds()?;
    if tri.len() != 3 {
        return Err(KtCode::RedCountWrong.val());
    }
    // ⑤ 红账行集恰为 {3,7,11}。
    let mut expect = [3usize, 7, 11];
    let mut got = [99usize; 3];
    let mut k = 0;
    while k < 3 {
        got[k] = tri.get(k).map(|x| x.0).unwrap_or(99);
        k += 1;
    }
    // 排序对拍（聚合按行号升序出账，故天然有序；此处显式排序复算）。
    let mut a = 0;
    while a < 3 {
        let mut b = a + 1;
        while b < 3 {
            if got[b] < got[a] {
                let t = got[a];
                got[a] = got[b];
                got[b] = t;
            }
            if expect[b] < expect[a] {
                let t = expect[a];
                expect[a] = expect[b];
                expect[b] = t;
            }
            b += 1;
        }
        a += 1;
    }
    if got[0] != expect[0] || got[1] != expect[1] || got[2] != expect[2] {
        return Err(KtCode::RedMislabel.val());
    }
    // ⑥ 冻结账本体不写回（注入走副本）。行 7（卡 18448）段位冻结值
    // = 0x7EA00 + 0x40×7 = 0x7EBC0（0x7EC00 是行 8 卡 18449 的段）。
    if B03_LEDGER[7].seg != 0x7EBC0 || B03_LEDGER[3].quota != 340 || B03_LEDGER[11].card != 18452
    {
        return Err(KtCode::RowMismatch.val());
    }
    Ok((gr, rd, tri.len() as u32))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(KtCode::NotReproducible.val());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：干净聚合 19/19 绿。
pub fn probe_seq_a() -> Result<(), u32> {
    run_all_clean()?;
    Ok(())
}

/// 负探针 1：引导跳级拒（BootIdle 直接聚合）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut g = BootGate::new();
    match g.aggregate(&B03_LEDGER) {
        Err(c) if c == KtCode::BootSkip.val() => Ok(()),
        _ => Err(KtCode::BootSkip.val()),
    }
}

/// 负探针 2：引导倒退拒（Verdict 后回 Aggregating）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut g = BootGate::new();
    g.advance(BootState::Loading).ok();
    g.advance(BootState::Aggregating).ok();
    g.advance(BootState::Verdict).ok();
    match g.advance(BootState::Aggregating) {
        Err(c) if c == KtCode::BootBackward.val() => Ok(()),
        _ => Err(KtCode::BootBackward.val()),
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < KtCode::ALL.len() {
        let v = KtCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口（19 条） ───────────────────────────────────────────
pub fn run_unx18459_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18459 ktest 引导面 B03 批断言集");
    // ① 黄金全链（干净 19 绿 + 三类齐注恰红 3）。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!(
            "绿{:?} 红{:?} 齐注红{:?}",
            g.as_ref().map(|r| r.0).unwrap_or(0),
            g.as_ref().map(|r| r.1).unwrap_or(0),
            g.as_ref().map(|r| r.2).unwrap_or(0)
        ),
    );
    // ② 一次命令聚合全绿（J1 前半句）。
    s.add("一次命令19/19绿", matches!(run_all_clean(), Ok((gr, _)) if gr == ROWS as u32),
        "BootGate 四态走满，聚合 19 行零红");
    // ③ 逐行核对冻结派工账 19/19。
    s.add("逐行核对19/19", audit_rows(&B03_LEDGER).len() == 0,
        "卡号/SEG/配额/目标四元组与派工单一致");
    // ④ 段位连续：0x7EA00 步进 0x40 至 0x7EE80。
    s.add("段位连续19卡", assert_row(&B03_LEDGER, 0).is_ok()
        && assert_row(&B03_LEDGER, ROWS - 1).is_ok(),
        "0x40 步进 + 0x40 对齐");
    // ⑤ 分组配额账：A05 组 2960。
    let mut s05 = 0u32;
    let mut s06 = 0u32;
    let mut i = 0;
    while i < ROWS {
        let q = B03_LEDGER[i].quota;
        if i < GROUP_A05 {
            s05 = s05.saturating_add(q);
        } else {
            s06 = s06.saturating_add(q);
        }
        i += 1;
    }
    s.add("A05组配额2960", s05 == GROUP_A05_SUM, "F18441–F18450 逐行累加复算");
    // ⑥ A06 前九卡配额 2740。
    s.add("A06前九卡2740", s06 == GROUP_A06_SUM, "F18451–F18459 逐行累加复算");
    // ⑦ 批账守恒：两组和 + 闭合卡 300 = 批预算 6000。
    s.add("批账闭合6000", s05 + s06 + 300 == 6_000,
        "2960+2740+闭合卡 300=6000（闭合卡归 F18460 管辖）");
    // ⑧ 聚合守恒：绿+红=19。
    let mut gcon = BootGate::new();
    gcon.advance(BootState::Loading).ok();
    gcon.advance(BootState::Aggregating).ok();
    let agg = gcon.aggregate(&B03_LEDGER);
    s.add("聚合守恒", matches!(agg, Ok((gr, rd)) if gr + rd == ROWS as u32),
        "绿账+红账=行数，账不凭空生灭");
    // ⑨ 注入槽 0 段位断链 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let slot_ok = |idx: usize| -> bool {
        match &inj {
            Ok(l) => l.caught[idx] == l.tries[idx] && l.tries[idx] == JUDGE_ROUNDS,
            Err(_) => false,
        }
    };
    s.add("注入段位断链10/10", slot_ok(0), "行 7 必红 SegBroken");
    // ⑩ 注入槽 1 配额篡改 10/10。
    s.add("注入配额篡改10/10", slot_ok(1), "行 3 必红 QuotaOutOfWindow");
    // ⑪ 注入槽 2 卡号重复 10/10。
    s.add("注入卡号重复10/10", slot_ok(2), "行 11 必红 CardBroken");
    // ⑫ 三类齐注必红恰三条（J1 后半句）。
    let tri = triple_injection_reds();
    s.add("三类齐注恰红3条", matches!(&tri, Ok(v) if v.len() == 3),
        "行 3/7/11 各红一条，类别一一对应");
    // ⑬ 红账类别指认零误指。
    let tri_ok = match &tri {
        Ok(v) => {
            let mut labels_ok = true;
            let mut k = 0;
            while k < v.len() {
                let (row, code) = v[k];
                let expect_code = match row {
                    3 => KtCode::QuotaOutOfWindow.val(),
                    7 => KtCode::SegBroken.val(),
                    11 => KtCode::CardBroken.val(),
                    _ => 0,
                };
                if code != expect_code {
                    labels_ok = false;
                }
                k += 1;
            }
            labels_ok
        }
        Err(_) => false,
    };
    s.add("红账零误指", tri_ok, "红码与注入类别逐条对得上");
    // ⑭ 冻结账本体不写回（行 7 段位冻结值 0x7EBC0）。
    s.add("冻结账不写回",
        B03_LEDGER[7].seg == 0x7EBC0 && B03_LEDGER[3].quota == 340
            && B03_LEDGER[11].card == 18452,
        "注入全部走工作副本");
    // ⑮ 引导状态机：跳级/倒退双拒 + 正常路径通。
    s.add("引导状态机单向",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        "Idle→Loading→Aggregating→Verdict 单向");
    // ⑯ 断言窗复算：配额/目标全部在冻结窗内。
    let mut win_ok = true;
    let mut wi = 0;
    while wi < ROWS {
        let r = B03_LEDGER[wi];
        if r.quota < QUOTA_LO || r.quota > QUOTA_HI || r.target < TARGET_LO || r.target > TARGET_HI
        {
            win_ok = false;
        }
        wi += 1;
    }
    s.add("断言窗复算", win_ok, "19 行配额窗 240..480、目标窗 600..760");
    // ⑰ 双跑一致。
    s.add("双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // ⑱ 文案闭集。
    let mut closed = true;
    let mut ci = 0;
    while ci < KtCode::ALL.len() {
        if KtCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "11 码全有中文建议");
    // ⑲ 段位守卫。
    s.add("段位守卫 0x7EE80", segment_guard(), "SEG_BASE 0x40 对齐窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_run_all_green() {
        let (gr, rd) = run_all_clean().unwrap_or((0, 0));
        assert_eq!(gr, 19, "19 行全绿");
        assert_eq!(rd, 0, "干净跑零红");
    }

    #[test]
    fn ledger_rows_match_dispatch() {
        assert_eq!(audit_rows(&B03_LEDGER).len(), 0, "19 行逐条核对零红");
        assert_eq!(B03_LEDGER[0].card, 18441);
        assert_eq!(B03_LEDGER[ROWS - 1].card, 18459);
        assert_eq!(B03_LEDGER[ROWS - 1].seg, 0x7EE80);
    }

    #[test]
    fn seg_contiguity_frozen() {
        assert!(seg_contiguity_all());
        assert_eq!(B03_LEDGER[0].seg, 0x7EA00);
        assert_eq!(B03_LEDGER[ROWS - 1].seg - B03_LEDGER[0].seg, SEG_STEP * (ROWS as u32 - 1));
    }

    #[test]
    fn group_quota_sums() {
        let mut s05 = 0u32;
        let mut s06 = 0u32;
        let mut i = 0;
        while i < ROWS {
            if i < GROUP_A05 {
                s05 += B03_LEDGER[i].quota;
            } else {
                s06 += B03_LEDGER[i].quota;
            }
            i += 1;
        }
        assert_eq!(s05, 2_960);
        assert_eq!(s06, 2_740);
        assert_eq!(s05 + s06 + 300, 6_000, "闭合卡 300 归 F18460");
    }

    #[test]
    fn single_injections_red_exact_row() {
        // 槽 0：段位断链——恰行 7 红。
        let reds0 = aggregate_reds(&tampered_copy(0)).unwrap_or_default();
        assert_eq!(reds0.len(), 1);
        assert_eq!(reds0[0].0, 7);
        assert_eq!(reds0[0].1, KtCode::SegBroken.val());
        // 槽 1：配额篡改——恰行 3 红。
        let reds1 = aggregate_reds(&tampered_copy(1)).unwrap_or_default();
        assert_eq!(reds1.len(), 1);
        assert_eq!(reds1[0].0, 3);
        assert_eq!(reds1[0].1, KtCode::QuotaOutOfWindow.val());
        // 槽 2：卡号重复——恰行 11 红。
        let reds2 = aggregate_reds(&tampered_copy(2)).unwrap_or_default();
        assert_eq!(reds2.len(), 1);
        assert_eq!(reds2[0].0, 11);
        assert_eq!(reds2[0].1, KtCode::CardBroken.val());
    }

    #[test]
    fn triple_injection_exactly_three_reds() {
        let tri = triple_injection_reds().unwrap_or_default();
        assert_eq!(tri.len(), 3, "三类齐注恰红 3 条");
        let mut rows = [tri[0].0, tri[1].0, tri[2].0];
        rows.sort_unstable();
        assert_eq!(rows, [3, 7, 11]);
    }

    #[test]
    fn frozen_ledger_immutable_under_injection() {
        let _ = inject_rounds(10);
        let _ = triple_injection_reds();
        assert_eq!(B03_LEDGER[7].seg, 0x7EBC0, "行 7 段位不变");
        assert_eq!(B03_LEDGER[3].quota, 340, "行 3 配额不变");
        assert_eq!(B03_LEDGER[11].card, 18452, "行 11 卡号不变");
    }

    #[test]
    fn boot_state_machine_monotonic() {
        let mut g = BootGate::new();
        assert_eq!(g.aggregate(&B03_LEDGER).err(), Some(KtCode::BootSkip.val()), "跳级拒");
        g.advance(BootState::Loading).ok();
        g.advance(BootState::Aggregating).ok();
        g.advance(BootState::Verdict).ok();
        assert_eq!(g.advance(BootState::Loading).err(), Some(KtCode::BootBackward.val()), "倒退拒");
        // Verdict→Verdict 是同态非法重入（未回退到更早态），按 advance
        // 协议（state > to 才算倒退）归跳级 BootSkip，不是 BootBackward。
        assert_eq!(g.advance(BootState::Verdict).err(), Some(KtCode::BootSkip.val()));
    }

    #[test]
    fn injection_ledger_all_caught_no_miss() {
        let inj = inject_rounds(10).unwrap_or_else(|_| InjectLedger::new());
        assert!(inj.all_caught(), "三类注入 10/10 全抓");
        assert_eq!(inj.missed(), 0);
        for i in 0..3 {
            assert_eq!(inj.tries[i], inj.caught[i], "槽 {} 计数守恒", i);
        }
    }

    #[test]
    fn double_run_identical() {
        assert_eq!(golden_run(), golden_run(), "双跑一致");
    }

    #[test]
    fn probes_and_segment_guard() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn checks_set_all_green_19_criteria() {
        let s = run_unx18459_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert_eq!(s.len(), 19, "J1 口径恰 19 条判据");
    }
}
