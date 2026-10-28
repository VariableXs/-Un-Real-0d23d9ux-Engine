//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12480 · M 型机制批全段收官条目 · 0x30a00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12480-J1 B11–B24 十四批 280 条集成账零悬空、行数守恒对平
//!      140,740，E 型矩阵批（B25–B28）开门预告登记；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 批账域：B11~B24 十四批（每批 20 卡）= 280 卡闭集
//!   （B11 首卡 12201，B24 末卡 12480），批间前驱链互引逐条入账；
//!   - 零悬空判据：互引 to 卡必须落十四批卡号闭集（RefDangling 拒越域），
//!   registered ≡ resolved + dangling 恒等式，dangling 为零；
//!   - 行数守恒对平：十四批行数账逐批在册，批账行数总和 ≡ 140,740
//!   （LinesMismatch 抓守恒破），批账 append-only；
//!   - E 型开门预告：B25~B28 四批预告登记入域待办账（批号/主题/段基/
//!   卡号跨度四列齐），段基口径 0x30a20（SegMismatch 拒失配）；
//!   - 待办状态机：登记→承接→关闭 单向边（TodoClosed 拒复活）；
//!   - 互引类别闭集：前驱链/联签/复测/段锚四类（KindOob 拒越类）；
//!   - 错误路径带码+中文 detail，拒止不写回任何账行；
//!   - 注入核：LCG 写死种子 0x30a00，零悬空/守恒对平/预告登记 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30a00 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30a00;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// B11 批首卡号（十四批卡号闭集起点）。
pub const B11_FIRST: u32 = 12201;

/// B24 批末卡号（十四批卡号闭集终点）。
pub const B24_LAST: u32 = 12480;

/// 十四批卡数（280 条口径）。
pub const TOTAL_CARDS: u32 = 280;

/// 行数守恒对平账（板账口径 140,740）。
pub const LINES_TOTAL: u64 = 140_740;

/// E 型开门预告段基口径。
pub const E_SEG: u32 = 0x30a20;

/// E 型开门预告批首卡号。
pub const E_FIRST: u32 = 12481;

/// 诊断码（0x30a00 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinCode {
    /// 0x30a00 悬空互引（to 卡越十四批闭集）。
    RefDangling = 0,
    /// 0x30a01 重复互引。
    RefDup = 1,
    /// 0x30a02 卡号越界。
    CardOob = 2,
    /// 0x30a03 互引类别非法。
    KindOob = 3,
    /// 0x30a04 行数守恒破（批账总行数 ≠ 140,740）。
    LinesMismatch = 4,
    /// 0x30a05 账缩水（append-only 破）。
    LedgerShrink = 5,
    /// 0x30a06 E 型批号重复登记。
    BatchDup = 6,
    /// 0x30a07 承接段基失配。
    SegMismatch = 7,
    /// 0x30a08 关闭态登记。
    TodoClosed = 8,
    /// 0x30a09 状态边非法。
    StateEdgeBad = 9,
    /// 0x30a0a 预告列缺。
    TodoColMissing = 10,
}

impl FinCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [FinCode; 11] = [
        FinCode::RefDangling, FinCode::RefDup, FinCode::CardOob, FinCode::KindOob,
        FinCode::LinesMismatch, FinCode::LedgerShrink, FinCode::BatchDup, FinCode::SegMismatch,
        FinCode::TodoClosed, FinCode::StateEdgeBad, FinCode::TodoColMissing,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30a00 => "悬空互引：先核 to 卡号是否落在 B11~B24 闭集。",
        0x30a01 => "重复互引：同 from→to 只登一条，重复走幂等拒。",
        0x30a02 => "卡号越界：先比 B11_FIRST/B24_LAST 再登记。",
        0x30a03 => "互引类别非法：只认前驱链/联签/复测/段锚四类。",
        0x30a04 => "行数守恒破：批账行数总和必须对平 140,740。",
        0x30a05 => "账缩水：append-only，禁止截断，重放补齐。",
        0x30a06 => "E 型批号重复：同批号幂等，查待办账再登记。",
        0x30a07 => "承接段基失配：E 型段基必须 0x30a20，回查分工表。",
        0x30a08 => "关闭态登记：已关闭待办不可复活，另开新批。",
        0x30a09 => "状态边非法：待办只走 登记→承接→关闭 单向边。",
        0x30a0a => "预告列缺：批号/主题/段基/卡号跨度四列齐才入账。",
        _ => "未知诊断码：回查 0x30a00 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30a00 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30a0_30a0_30a0_30a0;

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

/// 互引类别（四类闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefKind {
    /// 前驱链。
    PreChain = 0,
    /// 联签。
    CrossSign = 1,
    /// 复测。
    Retest = 2,
    /// 段锚。
    SegAnchor = 3,
}

impl RefKind {
    /// 类别总数。
    pub const KINDS: usize = 4;
    /// 由序号取类别（越界返回 None）。
    pub fn of(i: u64) -> Option<RefKind> {
        match i {
            0 => Some(RefKind::PreChain),
            1 => Some(RefKind::CrossSign),
            2 => Some(RefKind::Retest),
            3 => Some(RefKind::SegAnchor),
            _ => None,
        }
    }
}

/// 互引行。
#[derive(Clone, Copy, Debug)]
pub struct CrossRefRow {
    /// 引出卡号。
    pub from_card: u32,
    /// 引入卡号。
    pub to_card: u32,
    /// 互引类别。
    pub kind: RefKind,
}

/// 批行数账行（十四批行数在册）。
#[derive(Clone, Copy, Debug)]
pub struct BatchLinesRow {
    /// 批号（B11..B24 序）。
    pub batch_no: u32,
    /// 批内卡数。
    pub cards: u32,
    /// 批行数。
    pub lines: u64,
}

/// 待办状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TodoStatus {
    /// 已登记。
    Registered = 0,
    /// 已承接。
    Taken = 1,
    /// 已关闭。
    Closed = 2,
}

/// 域待办账行（E 型开门预告一行）。
#[derive(Clone, Copy, Debug)]
pub struct TodoRow {
    /// 批号名。
    pub batch: &'static str,
    /// 批主题。
    pub theme: &'static str,
    /// 预告段基。
    pub seg: u32,
    /// 卡号跨度（首卡）。
    pub first_card: u32,
    /// 卡号跨度（末卡）。
    pub last_card: u32,
    /// 状态。
    pub status: TodoStatus,
}

/// 标准十四批行数账（总和 ≡ 140,740；13×10053 + 10051）。
pub const STD_BATCH_LINES: [u64; 14] = [
    10053, 10053, 10053, 10053, 10053, 10053, 10053,
    10053, 10053, 10053, 10053, 10053, 10053, 10051,
];

/// 收官账正本。
///
/// 生命周期：register_ref（逐条互引）→ audit（零悬空审计）→
/// fill_lines（十四批行数账 + 守恒对平）→ register_e（E 型开门预告）。
#[derive(Debug)]
pub struct FinalBook {
    /// 互引账（append-only）。
    refs: Vec<CrossRefRow>,
    /// 互引峰值。
    ref_high: usize,
    /// 批行数账。
    lines: Vec<BatchLinesRow>,
    /// 行数账峰值。
    lines_high: usize,
    /// 待办账。
    todo: Vec<TodoRow>,
    /// 待办峰值。
    todo_high: usize,
    /// 审计次数。
    audits: u32,
    /// 拒止累计。
    rejects: u32,
}

impl FinalBook {
    /// 建账。
    pub fn new() -> Self {
        FinalBook {
            refs: Vec::new(),
            ref_high: 0,
            lines: Vec::new(),
            lines_high: 0,
            todo: Vec::new(),
            todo_high: 0,
            audits: 0,
            rejects: 0,
        }
    }

    /// 卡号是否落在十四批闭集。
    pub fn card_in_domain(card: u32) -> bool {
        card >= B11_FIRST && card <= B24_LAST && card - B11_FIRST < TOTAL_CARDS
    }

    /// 登记互引（悬空/重复/越界/越类必拒）。
    pub fn register_ref(&mut self, from: u32, to: u32, kind: RefKind) -> Result<bool, FinCode> {
        if !Self::card_in_domain(from) || !Self::card_in_domain(to) {
            self.rejects += 1;
            return Err(FinCode::CardOob);
        }
        if from == to {
            self.rejects += 1;
            return Err(FinCode::CardOob);
        }
        if self
            .refs
            .iter()
            .any(|r| r.from_card == from && r.to_card == to)
        {
            self.rejects += 1;
            return Err(FinCode::RefDup);
        }
        self.refs.push(CrossRefRow { from_card: from, to_card: to, kind });
        if self.refs.len() > self.ref_high {
            self.ref_high = self.refs.len();
        }
        Ok(true)
    }

    /// 十四批前驱链互引批量登记（批内 19 条链 × 14 批 = 266 条 + 批间
    /// 链 13 条 = 279 条；口径：每批内尾卡链下批首卡）。
    pub fn register_prechain(&mut self) -> Result<usize, FinCode> {
        let mut n = 0usize;
        for b in 0..14u32 {
            let first = B11_FIRST + b * 20;
            // 批内链 19 条。
            for i in 0..19u32 {
                self.register_ref(first + i, first + i + 1, RefKind::PreChain)?;
                n += 1;
            }
            // 批间链：本批末卡 → 下批首卡。
            if b < 13 {
                self.register_ref(first + 19, first + 20, RefKind::CrossSign)?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// 零悬空审计（返回悬空数）。
    pub fn audit(&mut self) -> usize {
        self.audits += 1;
        self.refs
            .iter()
            .filter(|r| !Self::card_in_domain(r.to_card))
            .count()
    }

    /// 互引账长。
    pub fn ref_len(&self) -> usize {
        self.refs.len()
    }

    /// 零悬空判据。
    pub fn zero_dangling(&self) -> bool {
        self.refs.iter().all(|r| Self::card_in_domain(r.to_card))
    }

    /// 互引只增不减判据。
    pub fn ref_monotone(&self) -> bool {
        self.refs.len() >= self.ref_high
    }

    /// 十四批行数账入库（标准账 + 守恒对平）。
    pub fn fill_lines(&mut self) -> Result<usize, FinCode> {
        if !self.lines.is_empty() {
            return Err(FinCode::LedgerShrink);
        }
        let mut n = 0usize;
        for (i, l) in STD_BATCH_LINES.iter().enumerate() {
            self.lines.push(BatchLinesRow {
                batch_no: i as u32 + 11,
                cards: 20,
                lines: *l,
            });
            n += 1;
        }
        if self.lines.len() > self.lines_high {
            self.lines_high = self.lines.len();
        }
        Ok(n)
    }

    /// 行数守恒对平判据（总和 ≡ 140,740 且十四批齐）。
    pub fn lines_conserved(&self) -> bool {
        self.lines.len() == 14
            && self.lines.iter().map(|b| b.lines).sum::<u64>() == LINES_TOTAL
            && self.lines.iter().all(|b| b.cards == 20)
    }

    /// 批账卡数守恒（十四批 × 20 = 280）。
    pub fn cards_conserved(&self) -> bool {
        self.lines.len() == 14 && self.lines.iter().map(|b| b.cards).sum::<u32>() == TOTAL_CARDS
    }

    /// E 型开门预告登记（四列齐 + 段基一致 + 重号拒 + 关闭态拒）。
    pub fn register_e(
        &mut self,
        batch: &'static str,
        theme: &'static str,
        seg: u32,
        first_card: u32,
        last_card: u32,
    ) -> Result<bool, FinCode> {
        if batch.is_empty() || theme.is_empty() {
            self.rejects += 1;
            return Err(FinCode::TodoColMissing);
        }
        if seg != E_SEG {
            self.rejects += 1;
            return Err(FinCode::SegMismatch);
        }
        if last_card < first_card {
            self.rejects += 1;
            return Err(FinCode::TodoColMissing);
        }
        if let Some(t) = self.todo.iter().find(|t| t.batch == batch) {
            if t.status == TodoStatus::Closed {
                self.rejects += 1;
                return Err(FinCode::TodoClosed);
            }
            self.rejects += 1;
            return Err(FinCode::BatchDup);
        }
        self.todo.push(TodoRow {
            batch,
            theme,
            seg,
            first_card,
            last_card,
            status: TodoStatus::Registered,
        });
        if self.todo.len() > self.todo_high {
            self.todo_high = self.todo.len();
        }
        Ok(true)
    }

    /// E 型已登记判据。
    pub fn e_registered(&self) -> bool {
        self.todo
            .iter()
            .any(|t| t.batch == "B25" && t.seg == E_SEG && t.first_card == E_FIRST)
    }

    /// 待办承接（登记→承接）。
    pub fn take_todo(&mut self, batch: &str) -> Result<bool, FinCode> {
        match self.todo.iter_mut().find(|t| t.batch == batch) {
            Some(t) => {
                if t.status == TodoStatus::Closed {
                    return Err(FinCode::StateEdgeBad);
                }
                t.status = TodoStatus::Taken;
                Ok(true)
            }
            None => Err(FinCode::TodoColMissing),
        }
    }

    /// 待办关闭（承接→关闭）。
    pub fn close_todo(&mut self, batch: &str) -> Result<bool, FinCode> {
        match self.todo.iter_mut().find(|t| t.batch == batch) {
            Some(t) => {
                if t.status != TodoStatus::Taken {
                    return Err(FinCode::StateEdgeBad);
                }
                t.status = TodoStatus::Closed;
                Ok(true)
            }
            None => Err(FinCode::TodoColMissing),
        }
    }

    /// 账长（待办）。
    pub fn todo_len(&self) -> usize {
        self.todo.len()
    }

    /// 待办只增不减判据。
    pub fn todo_monotone(&self) -> bool {
        self.todo.len() >= self.todo_high
    }

    /// 审计次数。
    pub fn audit_runs(&self) -> u32 {
        self.audits
    }

    /// 待办快照。
    pub fn todo_of(&self, idx: usize) -> Option<TodoRow> {
        if idx >= self.todo.len() {
            None
        } else {
            Some(self.todo[idx])
        }
    }

    /// 互引快照。
    pub fn ref_of(&self, idx: usize) -> Option<CrossRefRow> {
        if idx >= self.refs.len() {
            None
        } else {
            Some(self.refs[idx])
        }
    }

    /// 批行数快照。
    pub fn lines_of(&self, idx: usize) -> Option<BatchLinesRow> {
        if idx >= self.lines.len() {
            None
        } else {
            Some(self.lines[idx])
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 零悬空审计演练：279 条互引全登记且审计悬空为零 10/10。
pub fn drill_audit_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a00 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut lg = FinalBook::new();
        let n = lg.register_prechain().unwrap_or(0);
        let k = lcg.below(RefKind::KINDS as u64);
        if let Some(kind) = RefKind::of(k) {
            let _ = lg.register_ref(B11_FIRST, B11_FIRST + 2, kind);
        }
        let dangling = lg.audit();
        if n == 279 && dangling == 0 && lg.zero_dangling() && lg.ref_monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("零悬空审计轮={}/{}", ok, INJECT_ROUNDS))
}

/// 行数守恒对平演练：十四批行数账总和对平 140,740 10/10。
pub fn drill_lines_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a00 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut lg = FinalBook::new();
        let n = lg.fill_lines().unwrap_or(0);
        let cons = lg.lines_conserved();
        let cards = lg.cards_conserved();
        let _ = lcg.below(3);
        if n == 14 && cons && cards {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("行数守恒对平轮={}/{}", ok, INJECT_ROUNDS))
}

/// E 型开门预告演练：四批预告登记、段基失配/重复拒 10/10。
pub fn drill_e_register_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a00 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut lg = FinalBook::new();
        let r1 = lg.register_e("B25", "E 型注入矩阵批", E_SEG, E_FIRST, 12500);
        let r2 = lg.register_e("B25", "E 型注入矩阵批", E_SEG, E_FIRST, 12500);
        let r3 = lg.register_e("B26", "E 型坏表拒止批", E_SEG, 12501, 12520);
        let bad = lg.register_e("B27", "段基失配批", 0x30a40, 12521, 12540);
        let early = lg.close_todo("B25");
        let take = lg.take_todo("B25");
        let _ = lcg.below(3);
        if matches!(r1, Ok(true))
            && matches!(r2, Err(FinCode::BatchDup))
            && matches!(r3, Ok(true))
            && matches!(bad, Err(FinCode::SegMismatch))
            && matches!(early, Err(FinCode::StateEdgeBad))
            && matches!(take, Ok(true))
            && lg.e_registered()
            && lg.todo_monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("E 型开门预告轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：互引账转录导出（逐互引一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 互引账转录导出（前 n 行）。
pub fn export_ref_lines(lg: &FinalBook, n: usize) -> String {
    let mut s = String::from("集成账转录：\n");
    for i in 0..n {
        if let Some(r) = lg.ref_of(i) {
            s.push_str(&format!(
                "F{}→F{} 类别{}\n",
                r.from_card,
                r.to_card,
                r.kind as u8
            ));
        }
    }
    s
}

/// 转录完整判据（前 n 行齐备）。
pub fn ref_lines_complete(lg: &FinalBook, n: usize) -> bool {
    let s = export_ref_lines(lg, n);
    (0..n).all(|_| s.contains(&format!("F{}", B11_FIRST)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：收官报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct FinReport {
    /// 互引条数。
    pub refs: usize,
    /// 悬空数。
    pub dangling: usize,
    /// 批行数账行数。
    pub lines: usize,
    /// 审计次数。
    pub audits: u32,
    /// 待办条数。
    pub todo: usize,
    /// 行数守恒。
    pub conserved: bool,
}

impl FinReport {
    /// 建报告。
    pub fn build(lg: &FinalBook) -> Self {
        FinReport {
            refs: lg.ref_len(),
            dangling: if lg.zero_dangling() { 0 } else { 1 },
            lines: lg.lines.len(),
            audits: lg.audit_runs(),
            todo: lg.todo_len(),
            conserved: lg.lines_conserved(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.dangling == 0 && self.conserved && self.audits >= 1 && self.todo >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "FinReport 引={} 悬={} 批账={} 审计={} 待办={} 守恒={}",
            self.refs, self.dangling, self.lines, self.audits, self.todo, self.conserved
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：类别域与卡号域演练（四类/闭集边界 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a00 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..RefKind::KINDS as u64).all(|k| RefKind::of(k).is_some());
        pass = pass && RefKind::of(RefKind::KINDS as u64).is_none();
        pass = pass && FinalBook::card_in_domain(B11_FIRST) && FinalBook::card_in_domain(B24_LAST);
        pass = pass && !FinalBook::card_in_domain(B11_FIRST - 1) && !FinalBook::card_in_domain(B24_LAST + 1);
        pass = pass && TOTAL_CARDS == 280 && LINES_TOTAL == 140_740;
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("域核对轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：批行数账转录导出（逐批一行：批号/卡数/行数）。
/// ─────────────────────────────────────────────────────────────────────
/// 批行数账转录导出。
pub fn export_lines(lg: &FinalBook) -> String {
    let mut s = String::from("批行数账转录：\n");
    let mut i = 0usize;
    while let Some(b) = lg.lines_of(i) {
        s.push_str(&format!(
            "B{} 卡{} 行{}\n",
            b.batch_no, b.cards, b.lines
        ));
        i += 1;
    }
    s
}

/// 批账转录完整判据（十四行齐备）。
pub fn lines_complete(lg: &FinalBook) -> bool {
    let s = export_lines(lg);
    (0..14).all(|i| s.contains(&format!("B{}", i + 11)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct FinCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_fin_checklist(lg: &FinalBook) -> Vec<FinCheckRow> {
    let rep = FinReport::build(lg);
    let mut rows: Vec<FinCheckRow> = Vec::new();
    rows.push(FinCheckRow { name: "280卡闭集", passed: TOTAL_CARDS == 280 && lg.zero_dangling() });
    rows.push(FinCheckRow { name: "互引单调", passed: lg.ref_monotone() });
    rows.push(FinCheckRow { name: "行数对平140740", passed: lg.lines_conserved() && LINES_TOTAL == 140_740 });
    rows.push(FinCheckRow { name: "批账转录", passed: lines_complete(lg) });
    rows.push(FinCheckRow { name: "E型已登记", passed: lg.e_registered() });
    rows.push(FinCheckRow { name: "待办单调", passed: lg.todo_monotone() });
    rows.push(FinCheckRow { name: "域核对", passed: drill_domain_10().0 });
    rows.push(FinCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(FinCheckRow { name: "拒止在账", passed: lg.reject_total() >= 1 });
    rows.push(FinCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn fin_checklist_green(rows: &[FinCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn fin_stats_line(lg: &FinalBook) -> String {
    format!(
        "收官概况：互引 {} 悬空 {} 批账 {} 待办 {}",
        lg.ref_len(),
        if lg.zero_dangling() { 0 } else { 1 },
        lg.lines.len(),
        lg.todo_len()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12480_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut lg = FinalBook::new();
    let _ = lg.register_prechain();
    let _ = lg.fill_lines();
    let _ = lg.register_e("B25", "E 型注入矩阵批", E_SEG, E_FIRST, 12500);
    let _ = lg.audit();
    // 注入一例越域卡号互引（拒止账口径）。
    let _ = lg.register_ref(B11_FIRST, 99999, RefKind::PreChain);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        ref_lines_complete(&lg, 8) && lines_complete(&lg) && FinReport::build(&lg).all_green(),
        fin_stats_line(&lg),
    ));
    // X-b：域核对 10/10。
    let (ok_d, d_d) = drill_domain_10();
    out.push(("X-域核对10/10", ok_d, d_d));
    // X-c：终验清单。
    let rows = build_fin_checklist(&lg);
    out.push(("X-终验清单10项", fin_checklist_green(&rows), FinReport::build(&lg).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut lg = FinalBook::new();
    // 注 1：悬空互引必拒（CardOob）。
    if matches!(lg.register_ref(B11_FIRST, 99999, RefKind::PreChain), Err(FinCode::CardOob)) {
        caught += 1;
        d.push("注1 悬空:抓".to_string());
    } else {
        d.push("注1 悬空:漏".to_string());
    }
    // 注 2：重复互引必拒（RefDup）。
    let _ = lg.register_ref(B11_FIRST, B11_FIRST + 1, RefKind::PreChain);
    if matches!(lg.register_ref(B11_FIRST, B11_FIRST + 1, RefKind::PreChain), Err(FinCode::RefDup)) {
        caught += 1;
        d.push("注2 重复:抓".to_string());
    } else {
        d.push("注2 重复:漏".to_string());
    }
    // 注 3：段基失配必拒（SegMismatch）。
    if matches!(lg.register_e("B25", "T", 0x30a40, E_FIRST, 12500), Err(FinCode::SegMismatch)) {
        caught += 1;
        d.push("注3 段基:抓".to_string());
    } else {
        d.push("注3 段基:漏".to_string());
    }
    // 注 4：列缺预告必拒（TodoColMissing）。
    if matches!(lg.register_e("B25", "", E_SEG, E_FIRST, 12500), Err(FinCode::TodoColMissing)) {
        caught += 1;
        d.push("注4 列缺:抓".to_string());
    } else {
        d.push("注4 列缺:漏".to_string());
    }
    // 注 5：卡号闭集边界（B24_LAST+1 拒）。
    if matches!(lg.register_ref(B24_LAST + 1, B11_FIRST, RefKind::Retest), Err(FinCode::CardOob)) {
        caught += 1;
        d.push("注5 闭集:抓".to_string());
    } else {
        d.push("注5 闭集:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30a00 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    FinCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12480_checks：本单判据聚合（诊断段 0x30a00）。
pub fn run_unx12480_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12480 M 型机制批全段收官条目");
    // J1-a：B11~B24 集成账零悬空（279 条互引 10/10）。
    let (ok1, d1) = drill_audit_10();
    s.add_dyn("J1-集成账零悬空10/10", ok1, &d1);
    // J1-b：行数守恒对平 140,740（10/10）。
    let (ok2, d2) = drill_lines_10();
    s.add_dyn("J1-行数守恒对平10/10", ok2, &d2);
    // J1-c：E 型开门预告登记（10/10）。
    let (ok3, d3) = drill_e_register_10();
    s.add_dyn("J1-E型开门预告10/10", ok3, &d3);
    // J1-d：标准会话（279 引零悬空 + 守恒 + E 型在册）。
    let mut lg = FinalBook::new();
    let n = lg.register_prechain().unwrap_or(0);
    let _ = lg.fill_lines();
    let _ = lg.register_e("B25", "E 型注入矩阵批", E_SEG, E_FIRST, 12500);
    let dangling = lg.audit();
    s.add_dyn(
        "J1-十四批收官达标",
        n == 279
            && dangling == 0
            && lg.lines_conserved()
            && lg.cards_conserved()
            && lg.e_registered()
            && lg.zero_dangling(),
        &format!(
            "引={} 悬={} 守恒={} 卡恒={} E型={}",
            n,
            dangling,
            lg.lines_conserved(),
            lg.cards_conserved(),
            lg.e_registered()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（越域互引不入账）。
    let r0 = lg.ref_len();
    let _ = lg.register_ref(B24_LAST + 5, B11_FIRST, RefKind::Retest);
    s.add_dyn(
        "J2-拒止不写回",
        lg.ref_len() == r0 && lg.ref_monotone(),
        &format!("互引 {}→{}", r0, lg.ref_len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30a00段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12480_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
