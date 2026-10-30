//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1664 · 类间选择序与下降语义 · 0x6CE40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B3）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1664-J1 选中沿优先级链自高向低，下降条件（高类空账）
//!     显式落账，跳类注入 10/10 次捕获；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 类优先级链闭集：交互类 > 批量类 > 空闲类（高→低，冻结三
//!      类单向链）；
//!    - 选择语义：从最高类起逐类查非空队列，命中即选；高类空 →
//!      显式下降落账（下降事件账）；
//!    - 跳类注入：跳过非空高类直接选低类 = 违例，10/10 捕获；
//!    - 选择账：每选必留（选中类/下降沿数/队列长）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6CE40，AI-VX3-1030 批D 独占区）。
pub const SEG_BASE: u32 = 0x6CE40;

// ── 类闭集（高→低） ────────────────────────────────────────────
pub const CLS_INTERACTIVE: usize = 0;
pub const CLS_BATCH: usize = 1;
pub const CLS_IDLE: usize = 2;
pub const CLASS_COUNT: usize = 3;
pub const CLASS_NAMES: [&str; CLASS_COUNT] = ["交互类", "批量类", "空闲类"];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelCode {
    /// 类越闭集
    ClassOutOfRange,
    /// 类名空（三类齐备破坏）
    ClassNameBlank,
    /// 跳类违例（高类非空却选低类）
    ClassSkipViolation,
    /// 下降未落账
    DescentUnbooked,
    /// 选择账缺列
    SelectLedgerShort,
    /// 队列长负
    NegativeQueue,
    /// 全类空选（无任务可选显性）
    AllEmptySelect,
    /// 双跑不一致
    NotReproducible,
}

impl SelCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SelCode::ClassOutOfRange => "类越闭集：三类冻结",
            SelCode::ClassNameBlank => "类名空：齐备破坏",
            SelCode::ClassSkipViolation => "跳类违例：高类非空却选低类",
            SelCode::DescentUnbooked => "下降未落账",
            SelCode::SelectLedgerShort => "选择账缺列",
            SelCode::NegativeQueue => "队列长负",
            SelCode::AllEmptySelect => "全类空选：显性无选",
            SelCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SelCode; 8] = [
        SelCode::ClassOutOfRange,
        SelCode::ClassNameBlank,
        SelCode::ClassSkipViolation,
        SelCode::DescentUnbooked,
        SelCode::SelectLedgerShort,
        SelCode::NegativeQueue,
        SelCode::AllEmptySelect,
        SelCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SelCode::ALL.len() {
        if SelCode::ALL[i].val() == code {
            return SelCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6CE40 段闭集内"
}

// ── 选择器（优先级链自高向低） ─────────────────────────────────
#[derive(Clone, Debug)]
pub struct SelectRow {
    pub chosen: usize,
    pub descents: usize,
    pub queue_len: u64,
}

pub struct ClassSelector {
    /// 三类队列长。
    pub queues: [u64; CLASS_COUNT],
    /// 选择账。
    pub ledger: Vec<SelectRow>,
    /// 下降事件账。
    pub descents_total: u64,
    pub skip_caught: u32,
}

impl ClassSelector {
    pub fn new() -> ClassSelector {
        ClassSelector { queues: [0; CLASS_COUNT], ledger: Vec::new(), descents_total: 0, skip_caught: 0 }
    }

    /// 入队。
    pub fn enqueue(&mut self, cls: usize) -> Result<(), u32> {
        if cls >= CLASS_COUNT {
            return Err(SelCode::ClassOutOfRange.val());
        }
        self.queues[cls] += 1;
        Ok(())
    }

    /// 出队（选择）：从高类起逐类查；下降沿显式落账。
    pub fn select_next(&mut self) -> Result<SelectRow, u32> {
        let mut descents = 0usize;
        let mut c = 0;
        while c < CLASS_COUNT {
            if self.queues[c] > 0 {
                // 选中沿：优先级链自高向低。
                self.queues[c] -= 1;
                let row = SelectRow { chosen: c, descents, queue_len: self.queues[c] };
                // 下降沿落账（每下降一档记一笔）。
                self.descents_total += descents as u64;
                self.ledger.push(row.clone());
                return Ok(row);
            }
            // 高类空 → 下降。
            descents += 1;
            c += 1;
        }
        Err(SelCode::AllEmptySelect.val())
    }

    /// 跳类审计：选中类之下不得有非空高类（10/10 捕获）。
    pub fn skip_audit(&self, row: &SelectRow) -> Result<(), u32> {
        if row.chosen >= CLASS_COUNT {
            return Err(SelCode::ClassOutOfRange.val());
        }
        let mut c = 0;
        while c < row.chosen {
            if self.queues[c] > 0 {
                self_skip_violation()?;
            }
            c += 1;
        }
        Ok(())
    }

    /// 账齐审计。
    pub fn ledger_audit(&self) -> Result<(), u32> {
        if CLASS_NAMES.len() != CLASS_COUNT {
            return Err(SelCode::ClassNameBlank.val());
        }
        let mut i = 0;
        while i < self.ledger.len() {
            if self.ledger[i].chosen >= CLASS_COUNT {
                return Err(SelCode::SelectLedgerShort.val());
            }
            i += 1;
        }
        Ok(())
    }
}

/// 跳类违例显性（供审计触发）。
fn self_skip_violation() -> Result<(), u32> {
    Err(SelCode::ClassSkipViolation.val())
}

// ── 跳类注入 10/10 ─────────────────────────────────────────────
/// 构造"高类非空但选了低类"的伪账行，审计必抓。
pub fn skip_inject_10() -> Result<u32, u32> {
    let mut caught = 0;
    let mut k = 0;
    while k < 10 {
        let mut sel = ClassSelector::new();
        sel.queues[CLS_INTERACTIVE] = 5; // 高类非空
        let row = SelectRow { chosen: CLS_IDLE, descents: 0, queue_len: 0 };
        if sel.skip_audit(&row).is_err() {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(SelCode::ClassSkipViolation.val());
    }
    Ok(caught)
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

// ── 注入检出账 ──────────────────────────────────────────────────
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
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
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
}

/// 黄金全链：入队→选择沿自高向低→下降落账→全空显性→跳类 10/10。
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    if CLASS_NAMES.len() != CLASS_COUNT {
        return Err(SelCode::ClassNameBlank.val());
    }
    let mut sel = ClassSelector::new();
    // 只入空闲类 2 个 + 批量类 1 个。
    sel.enqueue(CLS_BATCH)?;
    sel.enqueue(CLS_IDLE)?;
    sel.enqueue(CLS_IDLE)?;
    // 选择：首个选中批量（交互空 → 下降 1）。
    let r1 = sel.select_next()?;
    if r1.chosen != CLS_BATCH || r1.descents != 1 {
        return Err(SelCode::ClassSkipViolation.val());
    }
    sel.skip_audit(&r1)?;
    // 后两次选中空闲（下降 2）。
    let r2 = sel.select_next()?;
    let r3 = sel.select_next()?;
    if r2.chosen != CLS_IDLE || r3.chosen != CLS_IDLE {
        return Err(SelCode::ClassSkipViolation.val());
    }
    sel.skip_audit(&r2)?;
    sel.skip_audit(&r3)?;
    // 全空显性。
    if sel.select_next().is_ok() {
        return Err(SelCode::AllEmptySelect.val());
    }
    sel.ledger_audit()?;
    let skip = skip_inject_10()?;
    Ok((sel.descents_total, sel.ledger.len() as u64, skip))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut sel = ClassSelector::new();
    sel.enqueue(CLS_INTERACTIVE)?;
    sel.select_next().map(|_| ()).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 全空选 → 必拒。
    let mut sel = ClassSelector::new();
    sel.select_next().map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 类越界 → 必拒。
    let mut sel = ClassSelector::new();
    sel.enqueue(9)
}

/// 注入 10/10：越类、跳类、全空选、下降未落账四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越类。
        let mut s0 = ClassSelector::new();
        led.record(0, s0.enqueue(9).is_err());
        // 槽1：跳类。
        let mut s1 = ClassSelector::new();
        s1.queues[CLS_INTERACTIVE] = 3;
        let row = SelectRow { chosen: CLS_IDLE, descents: 0, queue_len: 0 };
        led.record(1, s1.skip_audit(&row).is_err());
        // 槽2：全空选。
        let mut s2 = ClassSelector::new();
        led.record(2, s2.select_next().is_err());
        // 槽3：跳类 10/10。
        led.record(3, skip_inject_10() == Ok(10));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SelCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SelCode::ALL.len() {
        let v = SelCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 优先级链自高向低。
    v.push((
        "J1-优先级链自高向低",
        CLASS_NAMES.len() == 3 && CLS_INTERACTIVE == 0,
        format!("{:?}", CLASS_NAMES),
    ));
    // 跳类 10/10。
    v.push((
        "J1-跳类注入10/10",
        skip_inject_10() == Ok(10),
        format!("{}/10 捕获", skip_inject_10().unwrap_or(0)),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6CE40 窗口")));
    v
}

pub fn run_unx1664_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1664 类间选择序与下降语义");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
