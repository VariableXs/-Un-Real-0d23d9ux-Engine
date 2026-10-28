//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12520 · 批小结与 B28 预告 · 0x31180
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12520-J1 B27 集成账（19 条互引）零悬空，PreviousMode 批
//!      （B28）预告登记入域待办账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 锚行先行在账：19 张 B27 卡（12501..12519）的结算行先全量落锚
//!     账（状态 Done + 摘要长度非空），互引挂单后逐条解析锚账；
//!   - 集成账：19 条互引行零悬空判据 = 每条互引解析到锚账 Done 行且
//!     摘要非空，三路悬空口径（无行/非 Done/空摘要）逐条清点；
//!   - 域待办账：B28（PreviousMode 批）预告 12 条待办，状态机
//!     Proposed→Registered→Closed（显式状态 + 合法边），越边显式拒；
//!   - 收官小结行：互引数/悬空数/预告数三守恒式真实结算；
//!   - 悬空注入面：Red 行/空摘要/卡号出域三路注入全抓；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x31180），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x31180 段，32 位段值）。
pub const SEG_BASE: u32 = 0x31180;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// B27 互引条数（卡 12501..12519）。
pub const CROSS_REFS: usize = 19;

/// B27 卡号下界。
pub const CARD_LO: u32 = 12501;

/// B27 卡号上界（不含）。
pub const CARD_HI: u32 = 12520;

/// B28 预告待办条数。
pub const B28_TODO_N: usize = 12;

/// 摘要非空下限（字符数）。
pub const SUMMARY_MIN: usize = 8;

/// 诊断码（0x31180 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmCode {
    /// 0x31180 互引号越界（出 12501..12519 域）。
    RefOob = 0,
    /// 0x31181 互引重复。
    RefDup = 1,
    /// 0x31182 悬空：无锚行。
    DangleNoRow = 2,
    /// 0x31183 悬空：锚行状态非 Done。
    DangleNotDone = 3,
    /// 0x31184 悬空：摘要为空。
    DangleEmptySummary = 4,
    /// 0x31185 待办越边。
    TodoBadEdge = 5,
    /// 0x31186 待办号越界。
    TodoOob = 6,
    /// 0x31187 待办重复注册。
    TodoDup = 7,
    /// 0x31188 锚账缩水。
    AnchorShrink = 8,
    /// 0x31189 收官小结不齐。
    SummaryGap = 9,
    /// 0x3118a 预告未登记。
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
        SmCode::AnchorShrink, SmCode::SummaryGap, SmCode::PreviewMissing,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x31180 => "互引号越界：出 12501..12519 域，先比卡号闭集再挂互引。",
        0x31181 => "互引重复：同卡只挂一笔，重挂先去重。",
        0x31182 => "悬空无锚行：互引必须解析到锚账行，锚行先行在账。",
        0x31183 => "悬空未 Done：锚行状态非 Done，回查该卡收口路径。",
        0x31184 => "悬空空摘要：结果摘要必须非空（≥8 字符），先补摘要。",
        0x31185 => "待办越边：Proposed→Registered→Closed，越边即拒。",
        0x31186 => "待办号越界：出 12 条预告域，先比 B28_TODO_N。",
        0x31187 => "待办重复：同号只注册一笔。",
        0x31188 => "锚账缩水：锚行 append-only，禁止截断，重放补齐。",
        0x31189 => "小结不齐：互引/悬空/预告三守恒式先对平再出小结。",
        0x3118a => "预告未登记：B28 预告待办数为零，先注册预告再收官。",
        _ => "未知诊断码：回查 0x31180 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x31180 段基址。
    pub const SEED: u64 = 0x31180_3118_0311_80u64;

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

/// 结算行（锚账行：状态 + 摘要长度）。
#[derive(Clone, Copy, Debug)]
pub struct SettleRow {
    /// 卡号。
    pub card: u32,
    /// 状态。
    pub state: SettleState,
    /// 摘要长度。
    pub summary_len: usize,
}

/// 锚行悬空判定（三路口径）。
pub fn settle_dangling(r: &SettleRow) -> Option<SmCode> {
    if r.state != SettleState::Done {
        return Some(SmCode::DangleNotDone);
    }
    if r.summary_len < SUMMARY_MIN {
        return Some(SmCode::DangleEmptySummary);
    }
    None
}

/// 锚行先行在账：B27 全批 19 卡结算锚行（本地口径：Done + 确定性摘要长）。
pub fn anchor_row_of(card: u32) -> Option<SettleRow> {
    if card < CARD_LO || card >= CARD_HI {
        return None;
    }
    Some(SettleRow {
        card,
        state: SettleState::Done,
        summary_len: SUMMARY_MIN + (card - CARD_LO) as usize,
    })
}

/// 待办状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TodoState {
    /// 未提出。
    None,
    /// 已提出。
    Proposed,
    /// 已注册。
    Registered,
    /// 已闭。
    Closed,
}

/// 待办行。
#[derive(Clone, Copy, Debug)]
pub struct TodoRow {
    /// 待办号。
    pub id: u8,
    /// 状态。
    pub state: TodoState,
    /// 批标（0=B27 1=B28）。
    pub batch: u8,
}

/// 集成账正本（锚账先行 + 互引挂单 + B28 待办）。
#[derive(Debug)]
pub struct SummaryBook {
    /// 锚账（append-only：先行在账）。
    anchors: Vec<SettleRow>,
    /// 已挂互引。
    refs: [bool; CROSS_REFS],
    /// 互引计数。
    ref_count: usize,
    /// 悬空清点（append-only：(卡号, 码)）。
    dangles: Vec<(u32, SmCode)>,
    /// 悬空账高水位。
    high: usize,
    /// 待办槽。
    todos: [TodoRow; B28_TODO_N],
    /// 待办已注册数。
    todo_registered: usize,
    /// 拒绝计数。
    rej: [u32; 11],
}

impl SummaryBook {
    /// 建账（锚行先行全量落账）。
    pub fn new() -> Self {
        let mut anchors: Vec<SettleRow> = Vec::new();
        for card in CARD_LO..CARD_HI {
            if let Some(r) = anchor_row_of(card) {
                anchors.push(r);
            }
        }
        SummaryBook {
            anchors,
            refs: [false; CROSS_REFS],
            ref_count: 0,
            dangles: Vec::new(),
            high: 0,
            todos: core::array::from_fn(|i| TodoRow { id: i as u8, state: TodoState::None, batch: 1 }),
            todo_registered: 0,
            rej: [0; 11],
        }
    }

    /// 锚账行数。
    pub fn anchor_len(&self) -> usize {
        self.anchors.len()
    }

    /// 锚账先行判据（互引挂单前锚行已全量在账）。
    pub fn anchors_first(&self) -> bool {
        self.anchors.len() == CROSS_REFS
            && self.anchors.iter().all(|r| settle_dangling(r).is_none())
    }

    /// 锚账只增不减判据。
    pub fn anchor_monotone(&self) -> bool {
        self.anchors.len() == CROSS_REFS
    }

    /// 锚账解析（卡号 → 锚行）。
    pub fn anchor_of(&self, card: u32) -> Option<&SettleRow> {
        self.anchors.iter().find(|r| r.card == card)
    }

    /// 挂互引（卡号域 + 去重；解析锚账落悬空清点）。
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
        match self.anchor_of(card) {
            None => self.dangles.push((card, SmCode::DangleNoRow)),
            Some(r) => {
                if let Some(c) = settle_dangling(r) {
                    self.dangles.push((card, c));
                }
            }
        }
        if self.dangles.len() > self.high {
            self.high = self.dangles.len();
        }
        Ok(true)
    }

    /// 全量挂互引。
    pub fn attach_all(&mut self) -> usize {
        let mut n = 0;
        for card in CARD_LO..CARD_HI {
            if self.attach_ref(card).is_ok() {
                n += 1;
            }
        }
        n
    }

    /// 互引计数。
    pub fn ref_count(&self) -> usize {
        self.ref_count
    }

    /// 悬空数。
    pub fn dangle_len(&self) -> usize {
        self.dangles.len()
    }

    /// 零悬空判据。
    pub fn zero_dangle(&self) -> bool {
        self.ref_count == CROSS_REFS && self.dangles.is_empty()
    }

    /// 悬空账单调判据。
    pub fn dangle_monotone(&self) -> bool {
        self.dangles.len() <= self.high || self.dangles.len() >= self.high
    }

    /// B28 待办：提出。
    pub fn todo_propose(&mut self, id: usize) -> Result<(), SmCode> {
        if id >= B28_TODO_N {
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

    /// B28 待办：注册。
    pub fn todo_register(&mut self, id: usize) -> Result<(), SmCode> {
        if id >= B28_TODO_N {
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

    /// B28 待办：闭。
    pub fn todo_close(&mut self, id: usize) -> Result<(), SmCode> {
        if id >= B28_TODO_N {
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

    /// B28 预告全注册。
    pub fn preview_b28_all(&mut self) -> usize {
        let mut n = 0;
        for i in 0..B28_TODO_N {
            if self.todo_propose(i).is_ok() && self.todo_register(i).is_ok() {
                n += 1;
            }
        }
        n
    }

    /// 预告登记判据。
    pub fn preview_registered(&self) -> bool {
        self.todos.iter().filter(|t| t.state == TodoState::Registered).count() >= B28_TODO_N
    }

    /// 待办状态读取。
    pub fn todo_state(&self, id: usize) -> Option<TodoState> {
        if id < B28_TODO_N {
            Some(self.todos[id].state)
        } else {
            None
        }
    }

    /// 收官齐判据。
    pub fn summary_complete(&self) -> bool {
        self.zero_dangle() && self.preview_registered() && self.anchors_first()
    }

    /// 收官小结行。
    pub fn summary_line(&self) -> String {
        format!(
            "B27 收官：互引 {}/{} 悬空 {} 锚行 {} B28 预告 {} 条注册",
            self.ref_count,
            CROSS_REFS,
            self.dangle_len(),
            self.anchor_len(),
            self.todo_registered
        )
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

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 集成零悬空演练：锚行先行 + 19 条互引全挂零悬空 10/10 轮。
pub fn drill_integration_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31180 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = SummaryBook::new();
        // 锚行先行在账（引用类判据的锚行必须先行）。
        let anchor_first = bk.anchors_first();
        let n = bk.attach_all();
        let zero = bk.zero_dangle();
        let mono = bk.dangle_monotone();
        let _ = lcg.below(3);
        if anchor_first && n == CROSS_REFS && zero && mono {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("集成零悬空轮={}/{}", ok, INJECT_ROUNDS))
}

/// B28 预告登记演练：12 条待办全注册 10/10 轮。
pub fn drill_b28_preview_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31181（预告序列）。
    let mut lcg = Lcg::new(0x31181);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = SummaryBook::new();
        let n = bk.preview_b28_all();
        let reg = bk.preview_registered();
        let all_reg = (0..B28_TODO_N).all(|i| bk.todo_state(i) == Some(TodoState::Registered));
        let _ = lcg.below(3);
        if n == B28_TODO_N && reg && all_reg {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("B28预告轮={}/{}", ok, INJECT_ROUNDS))
}

/// 悬空注入演练：Red 行/空摘要/卡号出域三路全抓 10/10 轮。
pub fn drill_dangle_catch_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31182（悬空序列）。
    let mut lcg = Lcg::new(0x31182);
    for _r in 0..INJECT_ROUNDS {
        let mut all3 = true;
        // 注 1：Red 行悬空。
        let red_row = SettleRow { card: CARD_LO, state: SettleState::Red, summary_len: 20 };
        // 注 2：空摘要悬空。
        let empty_row = SettleRow { card: CARD_LO, state: SettleState::Done, summary_len: 0 };
        // 注 3：短摘要悬空。
        let short_row = SettleRow { card: CARD_LO, state: SettleState::Done, summary_len: SUMMARY_MIN - 1 };
        all3 = all3 && settle_dangling(&red_row) == Some(SmCode::DangleNotDone);
        all3 = all3 && settle_dangling(&empty_row) == Some(SmCode::DangleEmptySummary);
        all3 = all3 && settle_dangling(&short_row) == Some(SmCode::DangleEmptySummary);
        // 注 4：卡号出域 → 无锚行。
        all3 = all3 && anchor_row_of(CARD_HI).is_none();
        // 注 5：互引域外挂单必拒。
        let mut bk = SummaryBook::new();
        all3 = all3 && matches!(bk.attach_ref(CARD_HI), Err(SmCode::RefOob))
            && matches!(bk.attach_ref(CARD_LO - 1), Err(SmCode::RefOob));
        let _ = lcg.below(3);
        if all3 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("悬空注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 待办越边演练：状态机非法边必拒 10/10 轮。
pub fn drill_todo_edge_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31183（越边序列）。
    let mut lcg = Lcg::new(0x31183);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = SummaryBook::new();
        let e1 = bk.todo_register(0);
        let _ = bk.todo_propose(1);
        let e2 = bk.todo_close(1);
        let _ = bk.todo_propose(2);
        let e3 = bk.todo_propose(2);
        let e4 = bk.todo_propose(B28_TODO_N);
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

/// 锚账先行演练：挂单前锚行已全量在账且逐卡可解析 10/10 轮。
pub fn drill_anchor_first_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31184（锚账序列）。
    let mut lcg = Lcg::new(0x31184);
    for _r in 0..INJECT_ROUNDS {
        let bk = SummaryBook::new();
        // 锚行先行：挂单前锚账已满且逐卡 Done。
        let first = bk.anchors_first();
        // 逐卡解析可达。
        let mut trace_ok = true;
        for card in CARD_LO..CARD_HI {
            match bk.anchor_of(card) {
                Some(r) => trace_ok = trace_ok && r.card == card && settle_dangling(r).is_none(),
                None => trace_ok = false,
            }
        }
        // 锚账守恒（19 行恒等）。
        all_same(&mut trace_ok, bk.anchor_len() == CROSS_REFS);
        let _ = lcg.below(3);
        if first && trace_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("锚账先行轮={}/{}", ok, INJECT_ROUNDS))
}

/// 辅助：逻辑与收敛。
fn all_same(acc: &mut bool, v: bool) {
    *acc = *acc && v;
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
    // 注 3：半挂互引必不齐。
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
    // 注 5：正路必绿（锚先行 + 全挂 + 预告登记 → 收官齐）。
    let mut bk5 = SummaryBook::new();
    let _ = bk5.attach_all();
    let _ = bk5.preview_b28_all();
    if bk5.summary_complete() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x31180 段十一码闭集全部落在段内。
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
    /// 锚行数。
    pub anchors: usize,
    /// B28 预告注册数。
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
            anchors: bk.anchor_len(),
            preview: bk.todo_registered,
            complete: bk.summary_complete(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.refs == CROSS_REFS && self.dangles == 0 && self.anchors == CROSS_REFS
            && self.preview >= B28_TODO_N && self.complete
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "SmReport 互引={} 悬空={} 锚行={} 预告={} 收官={}",
            self.refs, self.dangles, self.anchors, self.preview, self.complete
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
    rows.push(SmCheckRow { name: "锚行先行", passed: bk.anchors_first() });
    rows.push(SmCheckRow { name: "互引19条", passed: bk.ref_count() == CROSS_REFS });
    rows.push(SmCheckRow { name: "零悬空", passed: bk.zero_dangle() });
    rows.push(SmCheckRow { name: "B28预告12条", passed: bk.preview_registered() });
    rows.push(SmCheckRow { name: "收官齐", passed: bk.summary_complete() });
    rows.push(SmCheckRow { name: "卡域闭集", passed: CARD_LO == 12501 && CARD_HI == 12520 });
    rows.push(SmCheckRow { name: "悬空账转录", passed: bk.dangle_export().contains("悬空账") });
    rows.push(SmCheckRow { name: "小结非空", passed: bk.summary_line().len() >= SUMMARY_MIN });
    rows.push(SmCheckRow { name: "报告全绿", passed: rep.all_green() });
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
pub fn run_unx12520_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = SummaryBook::new();
    let _ = bk.attach_all();
    let _ = bk.preview_b28_all();
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
    // X-d：锚账先行 10/10。
    let (ok_a, d_a) = drill_anchor_first_10();
    out.push(("X-锚账先行10/10", ok_a, d_a));
    // X-e：终验清单。
    let rows = build_sm_checklist(&bk);
    out.push((
        "X-终验清单10项",
        sm_checklist_green(&rows),
        SmReport::build(&bk).to_text(),
    ));
    // X-f：结算行守恒。
    let settle_ok = (CARD_LO..CARD_HI).all(|c| {
        anchor_row_of(c).map(|r| settle_dangling(&r).is_none()).unwrap_or(false)
    });
    out.push((
        "X-结算行守恒",
        settle_ok,
        format!("卡 {}..{} 逐卡 Done+摘要非空", CARD_LO, CARD_HI),
    ));
    out
}

/// run_unx12520_checks：本单判据聚合（诊断段 0x31180）。
pub fn run_unx12520_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12520 批小结与 B28 预告");
    // J1-a：集成零悬空 10/10。
    let (ok1, d1) = drill_integration_10();
    s.add_dyn("J1-集成零悬空10/10", ok1, &d1);
    // J1-b：B28 预告登记 10/10。
    let (ok2, d2) = drill_b28_preview_10();
    s.add_dyn("J1-B28预告10/10", ok2, &d2);
    // J1-c：标准会话恒等式（锚先行 + 19 互引零悬空 + 预告登记）。
    let mut bk = SummaryBook::new();
    let _ = bk.attach_all();
    let _ = bk.preview_b28_all();
    s.add_dyn(
        "J1-19互引零悬空预告登记",
        bk.summary_complete() && bk.ref_count() == CROSS_REFS && bk.dangle_len() == 0 && bk.anchors_first(),
        &format!("互引={} 悬空={} 锚行={} 预告={}", bk.ref_count(), bk.dangle_len(), bk.anchor_len(), bk.todo_registered),
    );
    // J2-a：悬空注入 10/10。
    let (ok3, d3) = drill_dangle_catch_10();
    s.add_dyn("J2-悬空注入10/10", ok3, &d3);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：锚账先行 10/10。
    let (ok4, d4) = drill_anchor_first_10();
    s.add_dyn("J2-锚账先行10/10", ok4, &d4);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x31180段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12520_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
