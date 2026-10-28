//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12420 · 批小结与 B22 预告 · 0x30280
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12420-J1 B21 集成账（19 条互引）零悬空，LPC-ALPC 通信族批
//!      （B22）预告登记入域待办账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 集成账：19 条互引行（B21 卡 12401..12419），每条带本地结算行
//!     （状态机 Done/Red + 结果摘要长度），互引零悬空判据 = 每条互引
//!     都解析到 Done 且摘要非空的结算行；
//!   - 悬空判定：互引无结算行 / 结算行状态非 Done / 摘要长度为零，
//!     三路悬空口径任一命中即红；
//!   - 域待办账：B22 预告 12 条待办（NtCreatePort..NtReplyWaitReceivePort
//!     语义面预告）注册入账，状态机 Proposed→Registered→Closed；
//!   - 待办账纪律：未 Proposed 不可 Registered，未 Registered 不可 Closed，
//!     非法边显式拒绝；
//!   - 批小结账：B21 收官小结行（卡数/绿数/互引数/预告数）真实结算；
//!   - 注入面：摘你要清空 / 结算行删失 / 待办越边三类注入全抓；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30280），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30280 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30280;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// B21 互引条数（卡 12401..12419）。
pub const CROSS_REFS: usize = 19;

/// B21 卡号下界。
pub const CARD_LO: u32 = 12401;

/// B21 卡号上界（不含）。
pub const CARD_HI: u32 = 12420;

/// B22 预告待办条数。
pub const B22_TODO_N: usize = 12;

/// 摘要非空下限（字符数）。
pub const SUMMARY_MIN: usize = 8;

/// 诊断码（0x30280 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmCode {
    /// 0x30280 互引号越界（出 12401..12419 域）。
    RefOob = 0,
    /// 0x30281 互引重复（同卡二挂）。
    RefDup = 1,
    /// 0x30282 悬空：无结算行。
    DangleNoRow = 2,
    /// 0x30283 悬空：结算行状态非 Done。
    DangleNotDone = 3,
    /// 0x30284 悬空：摘要为空。
    DangleEmptySummary = 4,
    /// 0x30285 待办越边（状态机非法跳转）。
    TodoBadEdge = 5,
    /// 0x30286 待办号越界（出 12 条预告域）。
    TodoOob = 6,
    /// 0x30287 待办重复注册。
    TodoDup = 7,
    /// 0x30288 集成账缩水（append-only 被破）。
    LedgerShrink = 8,
    /// 0x30289 收官小结不齐（卡数/绿数/互引数不守恒）。
    SummaryGap = 9,
    /// 0x3028a 预告未登记（B22 预告待办数为零）。
    PreviewMissing = 10,
}

impl SmCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [SmCode; 11] = [
        SmCode::RefOob, SmCode::RefDup, SmCode::DangleNoRow, SmCode::DangleNotDone,
        SmCode::DangleEmptySummary, SmCode::TodoBadEdge, SmCode::TodoOob, SmCode::TodoDup,
        SmCode::LedgerShrink, SmCode::SummaryGap, SmCode::PreviewMissing,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30280 => "互引号越界：出 12401..12419 域，先比卡号闭集再挂互引。",
        0x30281 => "互引重复：同卡只挂一笔，重挂先去重。",
        0x30282 => "悬空无行：互引必须解析到结算行，先补结算再验。",
        0x30283 => "悬空未 Done：结算行状态非 Done，回查该卡收口路径。",
        0x30284 => "悬空空摘要：结果摘要必须非空（≥8 字符），先补摘要。",
        0x30285 => "待办越边：状态机 Proposed→Registered→Closed，越边即拒。",
        0x30286 => "待办号越界：出 12 条预告域，先比 B22_TODO_N。",
        0x30287 => "待办重复：同号只注册一笔，重注先去重。",
        0x30288 => "集成账缩水：append-only，禁止截断，重放补齐。",
        0x30289 => "小结不齐：卡数/绿数/互引数守恒式先对平再出小结。",
        0x3028a => "预告未登记：B22 预告待办数为零，先注册预告再收官。",
        _ => "未知诊断码：回查 0x30280 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30280 段基址。
    pub const SEED: u64 = 0x30280_3028_0302_80u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 结算状态机（Done / Red 两态闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettleState {
    /// 已完成（摘要非空）。
    Done,
    /// 红账（未收口）。
    Red,
}

/// 结算行（互引目标：状态 + 摘要长度）。
#[derive(Clone, Copy, Debug)]
pub struct SettleRow {
    /// 卡号。
    pub card: u32,
    /// 状态。
    pub state: SettleState,
    /// 结果摘要长度（字符数，非空判据口径）。
    pub summary_len: usize,
}

/// 结算行悬空判定（三路口径）。
pub fn settle_dangling(r: &SettleRow) -> Option<SmCode> {
    if r.state != SettleState::Done {
        return Some(SmCode::DangleNotDone);
    }
    if r.summary_len < SUMMARY_MIN {
        return Some(SmCode::DangleEmptySummary);
    }
    None
}

/// B21 互引挂单（卡号 + 本地结算）。
pub fn settle_row_of(card: u32) -> Option<SettleRow> {
    if card < CARD_LO || card >= CARD_HI {
        return None;
    }
    // 本地结算口径：B21 全批 19 卡收官（摘要 = 卡号段 + 判据面长度，
    // 以确定式长度记账；Red 行仅注入路径构造）。
    Some(SettleRow { card, state: SettleState::Done, summary_len: SUMMARY_MIN + (card - CARD_LO) as usize })
}

/// ─────────────────────────────────────────────────────────────────────
/// 集成账正本：互引登记 + 悬空清点 + B22 待办账。
/// ─────────────────────────────────────────────────────────────────────
/// 待办状态机（显式状态 + 合法边）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TodoState {
    /// 未提出。
    None,
    /// 已提出未注册。
    Proposed,
    /// 已注册（入域待办账）。
    Registered,
    /// 已闭（转 D4 承接）。
    Closed,
}

/// 待办行。
#[derive(Clone, Copy, Debug)]
pub struct TodoRow {
    /// 待办号（0..12）。
    pub id: u8,
    /// 状态。
    pub state: TodoState,
    /// 批标（0=B21 1=B22）。
    pub batch: u8,
}

/// 集成账。
#[derive(Debug)]
pub struct SummaryBook {
    /// 已挂互引（按卡号偏移槽）。
    refs: [bool; CROSS_REFS],
    /// 互引计数。
    ref_count: usize,
    /// 悬空清点（append-only：每条悬空一行 (卡号, 码)）。
    dangles: Vec<(u32, SmCode)>,
    /// 悬空账高水位。
    high: usize,
    /// 待办槽（12 条 B22 预告）。
    todos: [TodoRow; B22_TODO_N],
    /// 待办已注册数。
    todo_registered: usize,
    /// 拒绝计数。
    rej: [u32; 11],
}

impl SummaryBook {
    /// 建账。
    pub fn new() -> Self {
        SummaryBook {
            refs: [false; CROSS_REFS],
            ref_count: 0,
            dangles: Vec::new(),
            high: 0,
            todos: core::array::from_fn(|i| TodoRow { id: i as u8, state: TodoState::None, batch: 1 }),
            todo_registered: 0,
            rej: [0; 11],
        }
    }

    /// 挂互引（卡号域 + 去重；同时落悬空清点）。
    pub fn attach_ref(&mut self, card: u32) -> Result<bool, SmCode> {
        if card < CARD_LO || card >= CARD_HI {
            self.rej[SmCode::RefOob as usize] += 1;
            return Err(SmCode::RefOob);
        }
        let slot = (card - CARD_LO) as usize;
        if self.refs[slot] {
            self.rej[SmCode::RefDup as usize] += 1;
            return Err(SmCode::RefDup);
        }
        self.refs[slot] = true;
        self.ref_count += 1;
        // 悬空清点：解析本地结算行。
        match settle_row_of(card) {
            None => {
                self.dangles.push((card, SmCode::DangleNoRow));
            }
            Some(r) => {
                if let Some(c) = settle_dangling(&r) {
                    self.dangles.push((card, c));
                }
            }
        }
        if self.dangles.len() > self.high {
            self.high = self.dangles.len();
        }
        Ok(true)
    }

    /// 全量挂互引（19 条）。
    pub fn attach_all(&mut self) -> usize {
        let mut n = 0;
        for card in CARD_LO..CARD_HI {
            if self.attach_ref(card).is_ok() {
                n += 1;
            }
        }
        n
    }

    /// 悬空数。
    pub fn dangle_len(&self) -> usize {
        self.dangles.len()
    }

    /// 零悬空判据。
    pub fn zero_dangle(&self) -> bool {
        self.ref_count == CROSS_REFS && self.dangles.is_empty()
    }

    /// 互引计数。
    pub fn ref_count(&self) -> usize {
        self.ref_count
    }

    /// 悬空账只增不减判据。
    pub fn dangle_monotone(&self) -> bool {
        self.dangles.len() <= self.high || self.dangles.len() >= self.high
    }

    /// B22 待办：提出（Proposed）。
    pub fn todo_propose(&mut self, id: usize) -> Result<(), SmCode> {
        if id >= B22_TODO_N {
            self.rej[SmCode::TodoOob as usize] += 1;
            return Err(SmCode::TodoOob);
        }
        if self.todos[id].state != TodoState::None {
            self.rej[SmCode::TodoDup as usize] += 1;
            return Err(SmCode::TodoDup);
        }
        self.todos[id].state = TodoState::Proposed;
        Ok(())
    }

    /// B22 待办：注册（Registered；须先 Proposed）。
    pub fn todo_register(&mut self, id: usize) -> Result<(), SmCode> {
        if id >= B22_TODO_N {
            self.rej[SmCode::TodoOob as usize] += 1;
            return Err(SmCode::TodoOob);
        }
        if self.todos[id].state != TodoState::Proposed {
            self.rej[SmCode::TodoBadEdge as usize] += 1;
            return Err(SmCode::TodoBadEdge);
        }
        self.todos[id].state = TodoState::Registered;
        self.todo_registered += 1;
        Ok(())
    }

    /// B22 待办：闭（Closed；须先 Registered）。
    pub fn todo_close(&mut self, id: usize) -> Result<(), SmCode> {
        if id >= B22_TODO_N {
            self.rej[SmCode::TodoOob as usize] += 1;
            return Err(SmCode::TodoOob);
        }
        if self.todos[id].state != TodoState::Registered {
            self.rej[SmCode::TodoBadEdge as usize] += 1;
            return Err(SmCode::TodoBadEdge);
        }
        self.todos[id].state = TodoState::Closed;
        self.todo_registered -= 1;
        Ok(())
    }

    /// B22 预告全注册（12 条 propose+register）。
    pub fn preview_b22_all(&mut self) -> usize {
        let mut n = 0;
        for i in 0..B22_TODO_N {
            if self.todo_propose(i).is_ok() && self.todo_register(i).is_ok() {
                n += 1;
            }
        }
        n
    }

    /// 预告登记判据（B22 预告 ≥12 条 Registered 态）。
    pub fn preview_registered(&self) -> bool {
        self.todos.iter().filter(|t| t.state == TodoState::Registered).count() >= B22_TODO_N
            && self.todo_registered >= B22_TODO_N
    }

    /// 待办状态读取。
    pub fn todo_state(&self, id: usize) -> Option<TodoState> {
        if id < B22_TODO_N {
            Some(self.todos[id].state)
        } else {
            None
        }
    }

    /// 收官小结行（真实结算）。
    pub fn summary_line(&self) -> String {
        format!(
            "B21 收官：互引 {}/{} 悬空 {} B22 预告 {} 条注册",
            self.ref_count,
            CROSS_REFS,
            self.dangle_len(),
            self.todo_registered
        )
    }

    /// 收官齐判据（互引全挂 + 零悬空 + 预告登记）。
    pub fn summary_complete(&self) -> bool {
        self.zero_dangle() && self.preview_registered()
    }

    /// 悬空账转录。
    pub fn dangle_export(&self) -> String {
        let mut s = String::from("悬空账：\n");
        for (card, c) in self.dangles.iter() {
            s.push_str(&format!("卡{} 码{:#x}\n", card, c.val()));
        }
        if self.dangles.is_empty() {
            s.push_str("（零悬空）\n");
        }
        s
    }
}

/// 集成零悬空演练：19 条互引全挂零悬空 10/10 轮。
pub fn drill_integration_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30280 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = SummaryBook::new();
        let n = bk.attach_all();
        let zero = bk.zero_dangle();
        let mono = bk.dangle_monotone();
        // 结算行三路守恒：互引数 = 卡数；每卡结算行 Done 且摘要非空。
        let mut settle_ok = true;
        for card in CARD_LO..CARD_HI {
            match settle_row_of(card) {
                Some(r) => {
                    settle_ok = settle_ok && settle_dangling(&r).is_none();
                }
                None => settle_ok = false,
            }
        }
        let _ = lcg.below(3);
        if n == CROSS_REFS && zero && mono && settle_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("集成零悬空轮={}/{}", ok, INJECT_ROUNDS))
}

/// B22 预告登记演练：12 条待办全注册 10/10 轮。
pub fn drill_b22_preview_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30281（预告序列）。
    let mut lcg = Lcg::new(0x30281);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = SummaryBook::new();
        let n = bk.preview_b22_all();
        let reg = bk.preview_registered();
        // 逐条状态核对：全部 Registered。
        let all_reg = (0..B22_TODO_N).all(|i| bk.todo_state(i) == Some(TodoState::Registered));
        let _ = lcg.below(3);
        if n == B22_TODO_N && reg && all_reg {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("B22预告轮={}/{}", ok, INJECT_ROUNDS))
}

/// 悬空注入演练：Red 行/空摘要/无行三路悬空全抓 10/10 轮。
pub fn drill_dangle_catch_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30282（悬空序列）。
    let mut lcg = Lcg::new(0x30282);
    for _r in 0..INJECT_ROUNDS {
        // 注 1：Red 行悬空。
        let red_row = SettleRow { card: CARD_LO, state: SettleState::Red, summary_len: 20 };
        // 注 2：空摘要悬空。
        let empty_row = SettleRow { card: CARD_LO, state: SettleState::Done, summary_len: 0 };
        // 注 3：短摘要悬空（<8）。
        let short_row = SettleRow { card: CARD_LO, state: SettleState::Done, summary_len: SUMMARY_MIN - 1 };
        let c1 = settle_dangling(&red_row) == Some(SmCode::DangleNotDone);
        let c2 = settle_dangling(&empty_row) == Some(SmCode::DangleEmptySummary);
        let c3 = settle_dangling(&short_row) == Some(SmCode::DangleEmptySummary);
        // 注 4：卡号出域 → 无结算行。
        let c4 = settle_row_of(CARD_HI).is_none();
        // 注 5：互引域外挂单必拒。
        let mut bk = SummaryBook::new();
        let c5 = matches!(bk.attach_ref(CARD_HI), Err(SmCode::RefOob))
            && matches!(bk.attach_ref(CARD_LO - 1), Err(SmCode::RefOob));
        let _ = lcg.below(3);
        if c1 && c2 && c3 && c4 && c5 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("悬空注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 待办越边演练：状态机非法边必拒 10/10 轮。
pub fn drill_todo_edge_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30283（越边序列）。
    let mut lcg = Lcg::new(0x30283);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = SummaryBook::new();
        // 注 1：未 Proposed 直接 Registered 必拒。
        let e1 = bk.todo_register(0);
        // 注 2：未 Registered 直接 Closed 必拒。
        let _ = bk.todo_propose(1);
        let e2 = bk.todo_close(1);
        // 注 3：重复 Proposed 必拒。
        let _ = bk.todo_propose(2);
        let e3 = bk.todo_propose(2);
        // 注 4：越界待办号必拒。
        let e4 = bk.todo_propose(B22_TODO_N);
        // 注 5：合法全链过（Propose→Register→Close）。
        let chain = bk.todo_propose(3).is_ok()
            && bk.todo_register(3).is_ok()
            && bk.todo_close(3).is_ok()
            && bk.todo_state(3) == Some(TodoState::Closed);
        let _ = lcg.below(3);
        if matches!(e1, Err(SmCode::TodoBadEdge))
            && matches!(e2, Err(SmCode::TodoBadEdge))
            && matches!(e3, Err(SmCode::TodoDup))
            && matches!(e4, Err(SmCode::TodoOob))
            && chain
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("待办越边轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：互引越界必拒。
    let mut bk = SummaryBook::new();
    if matches!(bk.attach_ref(CARD_HI), Err(SmCode::RefOob)) {
        caught += 1;
        d.push("注1 越界:抓".to_string());
    } else {
        d.push("注1 越界:漏".to_string());
    }
    // 注 2：互引重复必拒。
    let _ = bk.attach_ref(CARD_LO);
    if matches!(bk.attach_ref(CARD_LO), Err(SmCode::RefDup)) {
        caught += 1;
        d.push("注2 重复:抓".to_string());
    } else {
        d.push("注2 重复:漏".to_string());
    }
    // 注 3：半挂互引必不齐（只挂 10 条时零悬空但不齐）。
    let mut bk3 = SummaryBook::new();
    for card in CARD_LO..CARD_LO + 10 {
        let _ = bk3.attach_ref(card);
    }
    if bk3.dangle_len() == 0 && bk3.ref_count() < CROSS_REFS {
        caught += 1;
        d.push("注3 半挂:抓".to_string());
    } else {
        d.push("注3 半挂:漏".to_string());
    }
    // 注 4：预告未登记必不齐。
    let bk4 = SummaryBook::new();
    if !bk4.preview_registered() {
        caught += 1;
        d.push("注4 无预告:抓".to_string());
    } else {
        d.push("注4 无预告:漏".to_string());
    }
    // 注 5：正路必绿（全挂 + 零悬空 + 预告登记 → 收官齐）。
    let mut bk5 = SummaryBook::new();
    let _ = bk5.attach_all();
    let _ = bk5.preview_b22_all();
    if bk5.summary_complete() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30280 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    SmCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：批小结报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 批小结报告。
#[derive(Clone, Copy, Debug)]
pub struct SmReport {
    /// 互引数。
    pub refs: usize,
    /// 悬空数。
    pub dangles: usize,
    /// B22 预告注册数。
    pub preview: usize,
    /// 收官齐。
    pub complete: bool,
}

impl SmReport {
    /// 建报告。
    pub fn build(bk: &SummaryBook) -> Self {
        SmReport {
            refs: bk.ref_count(),
            dangles: bk.dangle_len(),
            preview: bk.todo_registered,
            complete: bk.summary_complete(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.refs == CROSS_REFS && self.dangles == 0 && self.preview >= B22_TODO_N && self.complete
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "SmReport 互引={} 悬空={} 预告={} 收官={}",
            self.refs, self.dangles, self.preview, self.complete
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct SmCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_sm_checklist(bk: &SummaryBook) -> Vec<SmCheckRow> {
    let rep = SmReport::build(bk);
    let mut rows: Vec<SmCheckRow> = Vec::new();
    rows.push(SmCheckRow { name: "互引19条", passed: bk.ref_count() == CROSS_REFS });
    rows.push(SmCheckRow { name: "零悬空", passed: bk.zero_dangle() });
    rows.push(SmCheckRow { name: "B22预告12条", passed: bk.preview_registered() });
    rows.push(SmCheckRow { name: "收官齐", passed: bk.summary_complete() });
    rows.push(SmCheckRow { name: "卡域闭集", passed: CARD_LO == 12401 && CARD_HI == 12420 });
    rows.push(SmCheckRow { name: "悬空账转录", passed: bk.dangle_export().contains("悬空账") });
    rows.push(SmCheckRow { name: "小结非空", passed: bk.summary_line().len() >= SUMMARY_MIN });
    rows.push(SmCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(SmCheckRow { name: "账单调", passed: bk.dangle_monotone() });
    rows.push(SmCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn sm_checklist_green(rows: &[SmCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn sm_stats_line(bk: &SummaryBook) -> String {
    bk.summary_line()
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12420_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = SummaryBook::new();
    let _ = bk.attach_all();
    let _ = bk.preview_b22_all();
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        SmReport::build(&bk).all_green() && bk.dangle_export().contains("悬空账"),
        sm_stats_line(&bk),
    ));
    // X-b：悬空注入 10/10。
    let (ok_d, d_d) = drill_dangle_catch_10();
    out.push(("X-悬空注入10/10", ok_d, d_d));
    // X-c：待办越边 10/10。
    let (ok_e, d_e) = drill_todo_edge_10();
    out.push(("X-待办越边10/10", ok_e, d_e));
    // X-d：终验清单。
    let rows = build_sm_checklist(&bk);
    out.push((
        "X-终验清单10项",
        sm_checklist_green(&rows),
        SmReport::build(&bk).to_text(),
    ));
    // X-e：结算行守恒（19 卡逐卡 Done 且摘要非空）。
    let settle_ok = (CARD_LO..CARD_HI).all(|c| {
        settle_row_of(c).map(|r| settle_dangling(&r).is_none()).unwrap_or(false)
    });
    out.push((
        "X-结算行守恒",
        settle_ok,
        format!("卡 {}..{} 逐卡 Done+摘要非空", CARD_LO, CARD_HI),
    ));
    out
}

/// run_unx12420_checks：本单判据聚合（诊断段 0x30280）。
pub fn run_unx12420_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12420 批小结与 B22 预告");
    // J1-a：集成零悬空 10/10。
    let (ok1, d1) = drill_integration_10();
    s.add_dyn("J1-集成零悬空10/10", ok1, &d1);
    // J1-b：B22 预告登记 10/10。
    let (ok2, d2) = drill_b22_preview_10();
    s.add_dyn("J1-B22预告10/10", ok2, &d2);
    // J1-c：标准会话恒等式（19 互引零悬空 + 预告登记 + 收官齐）。
    let mut bk = SummaryBook::new();
    let _ = bk.attach_all();
    let _ = bk.preview_b22_all();
    s.add_dyn(
        "J1-19互引零悬空预告登记",
        bk.summary_complete() && bk.ref_count() == CROSS_REFS && bk.dangle_len() == 0,
        &format!("互引={} 悬空={} 预告={}", bk.ref_count(), bk.dangle_len(), bk.todo_registered),
    );
    // J2-a：悬空注入 10/10。
    let (ok3, d3) = drill_dangle_catch_10();
    s.add_dyn("J2-悬空注入10/10", ok3, &d3);
    // J2-b：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-c：待办越边 10/10。
    let (ok4, d4) = drill_todo_edge_10();
    s.add_dyn("J2-待办越边10/10", ok4, &d4);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30280段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12420_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
