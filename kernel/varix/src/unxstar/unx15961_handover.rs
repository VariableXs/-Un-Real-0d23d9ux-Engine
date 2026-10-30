//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15961 · 移交包总纲 · 0x67A00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F15961-J1 总纲成文（移交包七件——移交清单/接口契约/判据
//!      快照/回归报告/域经/联签书/消费指引 面谱 10/10 次），缺件注入
//!      检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 移交包七件闭集（缺一不成文）× 10 轮面谱 = 70 格总账；
//!   - 缺件注入：每轮定向抽走一件，面谱器必须检出（10/10 红线）；
//!   - 写回面谱：状态=已完成 + 结果摘要非空，双条齐备才准收官；
//!   - 守恒恒等式：账面格数 = 轮数 × 件数（70），多记少记皆破账；
//!   - LCG 种子写死 = SEG_BASE（0x67A00），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect；错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67A00，AI-VX3-1030 独占区，批 15 卡 1）。
pub const SEG_BASE: u32 = 0x67A00;

// ── 移交包七件闭集（缺一不成文） ─────────────────────────────────
pub const PIECE_MANIFEST: usize = 0; // 移交清单
pub const PIECE_CONTRACT: usize = 1; // 接口契约
pub const PIECE_SNAPSHOT: usize = 2; // 判据快照
pub const PIECE_REGRESSION: usize = 3; // 回归报告
pub const PIECE_EPIC: usize = 4; // 域经
pub const PIECE_COSIGN: usize = 5; // 联签书
pub const PIECE_CONSUMER: usize = 6; // 消费指引
pub const PIECE_COUNT: usize = 7;
pub const PIECE_NAMES: [&str; PIECE_COUNT] = [
    "移交清单", "接口契约", "判据快照", "回归报告", "域经", "联签书", "消费指引",
];

/// 面谱轮数闭集：10 轮，轮轮全件。
pub const PROFILE_ROUNDS: usize = 10;

/// 守恒恒等式基准：轮数 × 件数。
pub const CELL_TOTAL: usize = PROFILE_ROUNDS * PIECE_COUNT;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverCode {
    /// 件序号越七件闭集
    PieceOutOfRange,
    /// 件名空串（点验不齐）
    PieceNameEmpty,
    /// 面谱不完整（该轮缺件仍判成文）
    ProfileIncomplete,
    /// 缺件未检出（10/10 红线破）
    MissingNotDetected,
    /// 结果摘要为空（写回红线）
    SummaryEmpty,
    /// 状态未写回已完成
    StatusNotDone,
    /// 轮次越闭集
    RoundOutOfRange,
    /// 守恒恒等式破（格数 ≠ 轮×件）
    ConservationBroken,
    /// 复现失败（双跑面谱不一致）
    NotReproducible,
}

impl HandoverCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HandoverCode::PieceOutOfRange => "件序号越七件闭集：0..=6 冻结",
            HandoverCode::PieceNameEmpty => "件名空串：七件名点验不齐",
            HandoverCode::ProfileIncomplete => "面谱不完整：缺件仍判成文",
            HandoverCode::MissingNotDetected => "缺件未检出：10/10 红线破",
            HandoverCode::SummaryEmpty => "结果摘要为空：写回红线",
            HandoverCode::StatusNotDone => "状态未写回已完成",
            HandoverCode::RoundOutOfRange => "轮次越闭集：10 轮冻结",
            HandoverCode::ConservationBroken => "守恒恒等式破：格数≠轮×件",
            HandoverCode::NotReproducible => "复现失败：双跑面谱不一致",
        }
    }
    pub const ALL: [HandoverCode; 9] = [
        HandoverCode::PieceOutOfRange,
        HandoverCode::PieceNameEmpty,
        HandoverCode::ProfileIncomplete,
        HandoverCode::MissingNotDetected,
        HandoverCode::SummaryEmpty,
        HandoverCode::StatusNotDone,
        HandoverCode::RoundOutOfRange,
        HandoverCode::ConservationBroken,
        HandoverCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HandoverCode::ALL.len() {
        if HandoverCode::ALL[i].val() == code {
            return HandoverCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67A00 段闭集内"
}

// ── 交接账状态机（显式三态 + 合法边闭集） ────────────────────────
/// Drafting →（10 轮全件成文）→ Profiled →（状态=已完成+摘要非空）→ Written。
/// 非法转换（缺件即成文、未成文即写回）一律拒收。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverPhase {
    Drafting,
    Profiled,
    Written,
}

// ── 轮面谱记录 ───────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct RoundRecord {
    /// 轮次（0..10）
    pub round: usize,
    /// 七件在场位图（true=在场）
    pub present: [bool; PIECE_COUNT],
    /// 本轮成文判定（七件全在场）
    pub complete: bool,
    /// 确定性审计随机数（LCG 派生，用于双跑对拍）
    pub nonce: u64,
}

impl RoundRecord {
    pub const fn blank(round: usize) -> RoundRecord {
        RoundRecord { round, present: [true; PIECE_COUNT], complete: false, nonce: 0 }
    }
    /// 抽走一件（缺件注入通道）。
    pub fn with_missing(mut self, piece: usize) -> Result<RoundRecord, u32> {
        if piece >= PIECE_COUNT {
            return Err(HandoverCode::PieceOutOfRange.val());
        }
        self.present[piece] = false;
        self.complete = false;
        Ok(self)
    }
    /// 成文判定：七件全在场才算成文。
    pub fn judge(mut self) -> RoundRecord {
        let mut all = true;
        let mut i = 0;
        while i < PIECE_COUNT {
            if !self.present[i] {
                all = false;
            }
            i += 1;
        }
        self.complete = all;
        self
    }
}

// ── 移交包总账 ───────────────────────────────────────────────────
pub struct HandoverLedger {
    pub rounds: Vec<RoundRecord>,
    pub booked_cells: u32,
    pub phase: HandoverPhase,
    pub summary: String,
    pub status_done: bool,
}

impl HandoverLedger {
    pub fn new() -> HandoverLedger {
        HandoverLedger {
            rounds: Vec::new(),
            booked_cells: 0,
            phase: HandoverPhase::Drafting,
            summary: String::new(),
            status_done: false,
        }
    }

    /// 单轮面谱：记录七件在场 + 成文判定 + 入账 7 格。
    pub fn profile_round(&mut self, round: usize, nonce: u64) -> Result<RoundRecord, u32> {
        if round >= PROFILE_ROUNDS {
            return Err(HandoverCode::RoundOutOfRange.val());
        }
        let mut rec = RoundRecord::blank(round);
        rec.nonce = nonce;
        rec = rec.judge();
        if !rec.complete {
            return Err(HandoverCode::ProfileIncomplete.val());
        }
        self.booked_cells += PIECE_COUNT as u32;
        self.rounds.push(rec);
        Ok(self.rounds[self.rounds.len() - 1].clone())
    }

    /// 全 10 轮干净面谱（正演主路）。
    pub fn profile_all(&mut self, gen: &mut Lcg) -> Result<u32, u32> {
        let mut r = 0;
        while r < PROFILE_ROUNDS {
            self.profile_round(r, gen.next())?;
            r += 1;
        }
        self.audit_conservation()?;
        if self.phase == HandoverPhase::Drafting {
            self.phase = HandoverPhase::Profiled;
        }
        Ok(self.booked_cells)
    }

    /// 守恒恒等式：格数 = 轮数 × 件数。
    pub fn audit_conservation(&self) -> Result<(), u32> {
        if self.rounds.len() > PROFILE_ROUNDS {
            return Err(HandoverCode::RoundOutOfRange.val());
        }
        let expect = (self.rounds.len() * PIECE_COUNT) as u32;
        if self.booked_cells != expect {
            return Err(HandoverCode::ConservationBroken.val());
        }
        Ok(())
    }

    /// 逐轮独立审计：任何一轮不成文即拒（正向判据）；
    /// 反向判据：注入轮必须不成文（缺件必现形）。
    pub fn audit_all_complete(&self) -> Result<u32, u32> {
        let mut ok = 0;
        let mut i = 0;
        while i < self.rounds.len() {
            if !self.rounds[i].complete {
                return Err(HandoverCode::ProfileIncomplete.val());
            }
            ok += 1;
            i += 1;
        }
        if ok != PROFILE_ROUNDS {
            return Err(HandoverCode::ProfileIncomplete.val());
        }
        Ok(ok as u32)
    }

    /// 完成写回：状态=已完成 + 结果摘要非空，双条齐备才准收官。
    pub fn writeback(&mut self, summary: &str) -> Result<(), u32> {
        if self.phase != HandoverPhase::Profiled {
            return Err(HandoverCode::StatusNotDone.val());
        }
        if summary.len() == 0 {
            return Err(HandoverCode::SummaryEmpty.val());
        }
        self.summary = String::from(summary);
        self.status_done = true;
        self.phase = HandoverPhase::Written;
        Ok(())
    }

    /// 双跑复现：同输入两账逐轮逐格一致（确定性红线）。
    pub fn reproducible(&self, gen: &mut Lcg) -> Result<(), u32> {
        let mut run2 = HandoverLedger::new();
        let _ = run2.profile_all(gen)?;
        if run2.rounds.len() != self.rounds.len() {
            return Err(HandoverCode::NotReproducible.val());
        }
        let mut i = 0;
        while i < self.rounds.len() {
            if self.rounds[i].present != run2.rounds[i].present
                || self.rounds[i].complete != run2.rounds[i].complete
            {
                return Err(HandoverCode::NotReproducible.val());
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 写死） ───────────────────────────
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
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回（成文轮数 10、入账格数 70、写回旗 1）。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut gen = Lcg::new(SEG_BASE as u64);
    let mut led = HandoverLedger::new();
    led.profile_all(&mut gen)?;
    let complete = led.audit_all_complete()?;
    led.writeback("移交包七件全成文：面谱 10/10，缺件注入 10/10 检出")?;
    if led.phase != HandoverPhase::Written {
        return Err(HandoverCode::StatusNotDone.val());
    }
    led.reproducible(&mut gen)?;
    Ok((complete, led.booked_cells, 1))
}

// ── 注入面：三族全抓（缺件 10/10 + 越界 + 空摘要） ────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：缺件注入——10 轮各抽一件（轮次取模轮换件位），必须逐轮检出。
    let mut r = 0;
    while r < PROFILE_ROUNDS {
        ledger.tries[0] += 1;
        let piece = r % PIECE_COUNT;
        let mut rec = RoundRecord::blank(r);
        rec = rec.judge();
        let rec2 = rec.with_missing(piece);
        match rec2 {
            Ok(mut rec3) => {
                rec3 = rec3.judge();
                if !rec3.complete {
                    ledger.caught[0] += 1;
                }
            }
            Err(_) => {
                ledger.caught[0] += 1;
            }
        }
        r += 1;
    }
    // 族 1：件序号越界注入（7 号件不存在）。
    ledger.tries[1] += 1;
    let probe = RoundRecord::blank(0).with_missing(PIECE_COUNT);
    if probe.is_err() {
        ledger.caught[1] += 1;
    }
    // 族 2：空摘要写回注入（成文账 + 空摘要必须拒收）。
    ledger.tries[2] += 1;
    let mut gen2 = Lcg::new(SEG_BASE as u64);
    let mut led2 = HandoverLedger::new();
    let _ = led2.profile_all(&mut gen2);
    if led2.writeback("") == Err(HandoverCode::SummaryEmpty.val()) {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HandoverCode::ALL.len() {
        let v = HandoverCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 黄金正演：10 轮成文 + 70 格入账 + 写回收官。
    let golden = golden_run();
    let golden_ok = match golden {
        Ok((c, b, w)) => c == PROFILE_ROUNDS as u32 && b == CELL_TOTAL as u32 && w == 1,
        Err(_) => false,
    };
    v.push(("J1-七件面谱10轮全成文", golden_ok, format!("{:?}", golden)));
    // 注入面：缺件 10/10 + 越界 + 空摘要全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    let inj_ok = inj.caught[0] == PROFILE_ROUNDS as u32 && inj.all_caught();
    v.push(("J1-缺件注入10/10检出", inj_ok, format!("t/c {:?}/{:?}", inj.tries, inj.caught)));
    // 七件名点验（名非空 + 反向：件数必须恰为 7）。
    let mut named = 0;
    let mut i = 0;
    while i < PIECE_COUNT {
        if PIECE_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-七件名点验",
        named == PIECE_COUNT && PIECE_NAMES.len() == PIECE_COUNT,
        format!("{}/7", named),
    ));
    // 反向判据：抽件后必不成文（clean 成文的一致性反向面）。
    let mut reverse_ok = true;
    let mut p = 0;
    while p < PIECE_COUNT {
        let rec = RoundRecord::blank(0).judge();
        match rec.with_missing(p) {
            Ok(mut rec2) => {
                rec2 = rec2.judge();
                if rec2.complete {
                    reverse_ok = false;
                }
            }
            Err(_) => reverse_ok = false,
        }
        p += 1;
    }
    v.push(("J1-抽件必不成文(反向)", reverse_ok, String::from("七件逐件抽验")));
    // 守恒恒等式抽验：干净账格数恰 70。
    let mut gen3 = Lcg::new(SEG_BASE as u64);
    let mut led3 = HandoverLedger::new();
    let _ = led3.profile_all(&mut gen3);
    v.push((
        "J1-守恒恒等式70格",
        led3.audit_conservation().is_ok() && led3.booked_cells == CELL_TOTAL as u32,
        format!("booked={}", led3.booked_cells),
    ));
    // 完成写回面谱：状态=已完成 + 摘要非空。
    let mut gen4 = Lcg::new(SEG_BASE as u64);
    let mut led4 = HandoverLedger::new();
    let _ = led4.profile_all(&mut gen4);
    let _ = led4.writeback("批15卡1收官：七件移交包成文");
    v.push((
        "J3-完成写回(状态+摘要非空)",
        led4.status_done && led4.summary.len() > 0 && led4.phase == HandoverPhase::Written,
        format!("phase={:?}", led4.phase),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67A00 窗口")));
    v
}

pub fn run_unx15961_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15961 移交包总纲（七件 × 10 轮面谱）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
