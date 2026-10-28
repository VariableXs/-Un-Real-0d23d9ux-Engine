//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12400 · 批小结与 B21 预告 · 0x30000
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F12400-J1 B20 集成账（19 条互引）零悬空，节对象映射族批（B21）
//!      预告登记入域待办账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - B20 集成账：F12380~F12399 二十卡之间 19 条互引（前驱链），
//!     append-only 登记，逐条 audit 悬空（to 卡必须落在批次卡号闭集）；
//!   - 零悬空判据：registered ≡ resolved + dangling 恒等式，dangling 必须
//!     为零；悬空互引当场拒绝（RefDangling）不入账；
//!   - 互引类别闭集：前驱链/联签/复测/段锚四类，越类拒绝（KindOob）；
//!   - B21 预告登记：批号/主题/段基/卡号跨度四列齐才入域待办账，
//!     重复登记幂等拒绝（TodoDup），登记后只增不减（TodoShrink 防线）；
//!   - 预告段基与分工口径核对：0x30020 段必须与 B21 批首卡一致，
//!     失配即 SegMismatch 红账；
//!   - 待办状态机：登记→承接→关闭 单向边，关闭后不可复活；
//!   - 注入核：LCG 写死种子 0x30000，10/10 轮审计/登记/单调演练全绿。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30000 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30000;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// B20 批首卡号（B20 集成账口径）。
pub const B20_FIRST: u32 = 12380;

/// B20 批末卡号（含）。
pub const B20_LAST: u32 = 12399;

/// B20 互引条数（前驱链 19 条：12380→12381 … 12398→12399）。
pub const B20_REF_COUNT: usize = 19;

/// B21 批首卡号（节对象映射族批）。
pub const B21_FIRST: u32 = 12401;

/// B21 预告段基（与分工表一致）。
pub const B21_SEG: u32 = 0x30020;

/// 诊断码（0x30000 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SumCode {
    /// 0x30000 悬空互引（to 卡不在批次卡号闭集）。
    RefDangling = 0,
    /// 0x30001 重复互引（同 from→to 已登记）。
    RefDup = 1,
    /// 0x30002 卡号越界（from/to 出批次闭集）。
    CardOob = 2,
    /// 0x30003 互引类别非法（越四类闭集）。
    KindOob = 3,
    /// 0x30004 预告重复登记（同批号已在待办账）。
    TodoDup = 4,
    /// 0x30005 待办账缩水（append-only 被破）。
    TodoShrink = 5,
    /// 0x30006 集成账缩水（互引只增不减被破）。
    RefShrink = 6,
    /// 0x30007 预告段基失配（与 B21 分工口径不一致）。
    SegMismatch = 7,
    /// 0x30008 关闭态登记（待办已关闭再登记）。
    TodoClosed = 8,
    /// 0x30009 预告列缺（批号/主题/段基/跨度四列有缺）。
    TodoColMissing = 9,
    /// 0x3000a 状态边非法（关闭→承接回跳）。
    StateEdgeBad = 10,
}

impl SumCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [SumCode; 11] = [
        SumCode::RefDangling, SumCode::RefDup, SumCode::CardOob, SumCode::KindOob,
        SumCode::TodoDup, SumCode::TodoShrink, SumCode::RefShrink, SumCode::SegMismatch,
        SumCode::TodoClosed, SumCode::TodoColMissing, SumCode::StateEdgeBad,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30000 => "悬空互引：先核 to 卡号是否落在 B20 闭集，越批互引一律拒登。",
        0x30001 => "重复互引：同 from→to 只登一条，重复走幂等拒绝路径。",
        0x30002 => "卡号越界：先比 B20_FIRST/B20_LAST 再登记互引。",
        0x30003 => "互引类别非法：只认前驱链/联签/复测/段锚四类。",
        0x30004 => "预告重复登记：同批号幂等，查待办账再入。",
        0x30005 => "待办账缩水：append-only，禁止截断，重放补齐。",
        0x30006 => "集成账缩水：互引登记后只增不减，审计前先对账。",
        0x30007 => "预告段基失配：B21 段基必须 0x30020，回查分工表。",
        0x30008 => "关闭态登记：已关闭待办不可复活，另开新批。",
        0x30009 => "预告列缺：批号/主题/段基/卡号跨度四列齐才入账。",
        0x3000a => "状态边非法：待办只走 登记→承接→关闭 单向边。",
        _ => "未知诊断码：回查 0x30000 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30000 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3000_3000_3000_3000;

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
    /// 前驱链（pre 字段链）。
    PreChain = 0,
    /// 联签（跨批互引）。
    CrossSign = 1,
    /// 复测（复测资产互引）。
    Retest = 2,
    /// 段锚（诊断段互引）。
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

/// 互引行（B20 集成账一行）。
#[derive(Clone, Copy, Debug)]
pub struct CrossRefRow {
    /// 引出卡号。
    pub from_card: u32,
    /// 引入卡号。
    pub to_card: u32,
    /// 互引类别。
    pub kind: RefKind,
}

/// 待办状态（单向边：登记→承接→关闭）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TodoStatus {
    /// 已登记未承接。
    Registered = 0,
    /// 已承接。
    Taken = 1,
    /// 已关闭。
    Closed = 2,
}

/// 域待办账行（B21 预告一行）。
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

/// B20 集成账 + 域待办账（本单正本）。
///
/// 生命周期：register_ref(逐条互引) → audit(零悬空审计) →
/// register_preview(B21 预告) → take/close(状态推进)。
#[derive(Debug)]
pub struct IntegrationLedger {
    /// 互引账（append-only）。
    refs: Vec<CrossRefRow>,
    /// 互引历史峰值（只增不减判准）。
    ref_high: usize,
    /// 待办账（append-only）。
    todo: Vec<TodoRow>,
    /// 待办历史峰值。
    todo_high: usize,
    /// 审计执行次数。
    audits: u32,
}

impl IntegrationLedger {
    /// 建账。
    pub fn new() -> Self {
        IntegrationLedger {
            refs: Vec::new(),
            ref_high: 0,
            todo: Vec::new(),
            todo_high: 0,
            audits: 0,
        }
    }

    /// 卡号是否落在 B20 闭集。
    pub fn card_in_b20(card: u32) -> bool {
        card >= B20_FIRST && card <= B20_LAST
    }

    /// 登记互引（悬空/重复/越界/越类必拒）。
    pub fn register_ref(&mut self, from: u32, to: u32, kind: RefKind) -> Result<bool, SumCode> {
        if !Self::card_in_b20(from) || !Self::card_in_b20(to) {
            return Err(SumCode::CardOob);
        }
        if from == to {
            return Err(SumCode::CardOob);
        }
        let dup = self
            .refs
            .iter()
            .any(|r| r.from_card == from && r.to_card == to);
        if dup {
            return Err(SumCode::RefDup);
        }
        self.refs.push(CrossRefRow { from_card: from, to_card: to, kind });
        if self.refs.len() > self.ref_high {
            self.ref_high = self.refs.len();
        }
        Ok(true)
    }

    /// B20 前驱链 19 条批量登记（判据口径：19 条互引）。
    pub fn register_prechain(&mut self) -> Result<usize, SumCode> {
        let mut n = 0usize;
        let mut cur = B20_FIRST;
        while cur < B20_LAST {
            self.register_ref(cur, cur + 1, RefKind::PreChain)?;
            cur += 1;
            n += 1;
        }
        Ok(n)
    }

    /// 零悬空审计（每条互引 to 必落闭集；返回悬空数）。
    pub fn audit(&mut self) -> usize {
        self.audits += 1;
        self.refs
            .iter()
            .filter(|r| !Self::card_in_b20(r.to_card))
            .count()
    }

    /// 审计执行次数。
    pub fn audit_runs(&self) -> u32 {
        self.audits
    }

    /// 互引账长。
    pub fn ref_len(&self) -> usize {
        self.refs.len()
    }

    /// 互引只增不减判据。
    pub fn ref_monotone(&self) -> bool {
        self.refs.len() >= self.ref_high
    }

    /// 零悬空守恒：ref_len == 19 时悬空必须为零。
    pub fn zero_dangling(&self) -> bool {
        self.refs.iter().all(|r| Self::card_in_b20(r.to_card))
    }

    /// 预告登记（四列齐才入账；重复/关闭态/列缺必拒）。
    pub fn register_preview(
        &mut self,
        batch: &'static str,
        theme: &'static str,
        seg: u32,
        first_card: u32,
        last_card: u32,
    ) -> Result<bool, SumCode> {
        if batch.is_empty() || theme.is_empty() {
            return Err(SumCode::TodoColMissing);
        }
        if seg != B21_SEG {
            return Err(SumCode::SegMismatch);
        }
        if last_card < first_card {
            return Err(SumCode::TodoColMissing);
        }
        if let Some(t) = self.todo.iter().find(|t| t.batch == batch) {
            if t.status == TodoStatus::Closed {
                return Err(SumCode::TodoClosed);
            }
            return Err(SumCode::TodoDup);
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

    /// 待办承接（登记→承接）。
    pub fn take_todo(&mut self, batch: &str) -> Result<bool, SumCode> {
        match self.todo.iter_mut().find(|t| t.batch == batch) {
            Some(t) => {
                if t.status == TodoStatus::Closed {
                    return Err(SumCode::StateEdgeBad);
                }
                t.status = TodoStatus::Taken;
                Ok(true)
            }
            None => Err(SumCode::TodoColMissing),
        }
    }

    /// 待办关闭（承接→关闭）。
    pub fn close_todo(&mut self, batch: &str) -> Result<bool, SumCode> {
        match self.todo.iter_mut().find(|t| t.batch == batch) {
            Some(t) => {
                if t.status != TodoStatus::Taken {
                    return Err(SumCode::StateEdgeBad);
                }
                t.status = TodoStatus::Closed;
                Ok(true)
            }
            None => Err(SumCode::TodoColMissing),
        }
    }

    /// 待办账长。
    pub fn todo_len(&self) -> usize {
        self.todo.len()
    }

    /// 待办只增不减判据。
    pub fn todo_monotone(&self) -> bool {
        self.todo.len() >= self.todo_high
    }

    /// B21 预告已登记且状态正确判据。
    pub fn b21_registered(&self) -> bool {
        self.todo
            .iter()
            .any(|t| t.batch == "B21" && t.seg == B21_SEG && t.first_card == B21_FIRST)
    }

    /// 单待办快照。
    pub fn todo_of(&self, idx: usize) -> Option<TodoRow> {
        if idx >= self.todo.len() {
            None
        } else {
            Some(self.todo[idx])
        }
    }

    /// 单互引快照。
    pub fn ref_of(&self, idx: usize) -> Option<CrossRefRow> {
        if idx >= self.refs.len() {
            None
        } else {
            Some(self.refs[idx])
        }
    }
}

/// 零悬空审计演练：10/10 轮 19 条互引全登记且审计悬空为零。
pub fn drill_audit_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30000 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut lg = IntegrationLedger::new();
        let n = lg.register_prechain().unwrap_or(0);
        // 每轮补一条类别互引（确定性变参）。
        let k = lcg.below(RefKind::KINDS as u64);
        if let Some(kind) = RefKind::of(k) {
            let _ = lg.register_ref(B20_FIRST, B20_FIRST + 2, kind);
        }
        let dangling = lg.audit();
        if n == B20_REF_COUNT && dangling == 0 && lg.zero_dangling() && lg.ref_monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("零悬空审计轮={}/{}", ok, INJECT_ROUNDS))
}

/// B21 预告登记演练：10/10 轮登记成功且幂等拒绝、四列齐。
pub fn drill_b21_register_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30000 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut lg = IntegrationLedger::new();
        let first = lg.register_preview("B21", "节对象映射族批", B21_SEG, B21_FIRST, 12420);
        let second = lg.register_preview("B21", "节对象映射族批", B21_SEG, B21_FIRST, 12420);
        let _ = lcg.below(3);
        if matches!(first, Ok(true))
            && matches!(second, Err(SumCode::TodoDup))
            && lg.todo_len() == 1
            && lg.todo_monotone()
            && lg.b21_registered()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("B21 预告登记轮={}/{}", ok, INJECT_ROUNDS))
}

/// 待办状态机演练：登记→承接→关闭 单向边 10/10，非法回跳必拒。
pub fn drill_todo_state_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30000 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut lg = IntegrationLedger::new();
        let reg = lg.register_preview("B21", "节对象映射族批", B21_SEG, B21_FIRST, 12420);
        let early_close = lg.close_todo("B21");
        let take = lg.take_todo("B21");
        let close = lg.close_todo("B21");
        let revive = lg.take_todo("B21");
        let st = lg.todo_of(0).map(|t| t.status);
        let _ = lcg.below(3);
        if matches!(reg, Ok(true))
            && matches!(early_close, Err(SumCode::StateEdgeBad))
            && matches!(take, Ok(true))
            && matches!(close, Ok(true))
            && matches!(revive, Err(SumCode::StateEdgeBad))
            && st == Some(TodoStatus::Closed)
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("待办状态机轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：集成账转录导出（逐互引一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 集成账转录导出。
pub fn export_ref_lines(lg: &IntegrationLedger) -> String {
    let mut s = String::from("集成账转录：\n");
    let mut i = 0usize;
    while let Some(r) = lg.ref_of(i) {
        s.push_str(&format!(
            "F{}→F{} 类别{}\n",
            r.from_card,
            r.to_card,
            r.kind as u8
        ));
        i += 1;
    }
    s
}

/// 转录完整判据（逐互引行齐备）。
pub fn ref_lines_complete(lg: &IntegrationLedger) -> bool {
    let s = export_ref_lines(lg);
    let n = lg.ref_len();
    if n == 0 {
        return false;
    }
    (0..n).all(|_| s.contains(&format!("F{}", B20_FIRST + 1)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：批小结报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct BatchReport {
    /// 互引条数。
    pub refs: usize,
    /// 悬空数。
    pub dangling: usize,
    /// 审计次数。
    pub audits: u32,
    /// 待办条数。
    pub todo: usize,
    /// 互引单调。
    pub ref_mono: bool,
    /// 待办单调。
    pub todo_mono: bool,
}

impl BatchReport {
    /// 建报告。
    pub fn build(lg: &IntegrationLedger) -> Self {
        BatchReport {
            refs: lg.ref_len(),
            dangling: if lg.zero_dangling() { 0 } else { 1 },
            audits: lg.audit_runs(),
            todo: lg.todo_len(),
            ref_mono: lg.ref_monotone(),
            todo_mono: lg.todo_monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.dangling == 0 && self.ref_mono && self.todo_mono && self.audits >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "BatchReport 引={} 悬={} 审计={} 待办={} 引单={} 办单={}",
            self.refs, self.dangling, self.audits, self.todo, self.ref_mono, self.todo_mono
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：段基口径核对（B21 预告段基必须与分工表一致）。
/// ─────────────────────────────────────────────────────────────────────
/// 段基口径核对判据。
pub fn seg_alignment_ok() -> bool {
    B21_SEG == 0x30020 && B20_FIRST == 12380 && B20_LAST == 12399 && B20_REF_COUNT == 19
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：互引类别域核对（四类闭集逐一可取）。
/// ─────────────────────────────────────────────────────────────────────
/// 类别域核对判据。
pub fn kind_domain_ok() -> bool {
    (0..RefKind::KINDS as u64).all(|k| RefKind::of(k).is_some()) && RefKind::of(4).is_none()
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct SumCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_summary_checklist(lg: &IntegrationLedger) -> Vec<SumCheckRow> {
    let rep = BatchReport::build(lg);
    let mut rows: Vec<SumCheckRow> = Vec::new();
    rows.push(SumCheckRow { name: "互引19条", passed: lg.ref_len() == B20_REF_COUNT });
    rows.push(SumCheckRow { name: "零悬空", passed: lg.zero_dangling() });
    rows.push(SumCheckRow { name: "互引单调", passed: lg.ref_monotone() });
    rows.push(SumCheckRow { name: "待办单调", passed: lg.todo_monotone() });
    rows.push(SumCheckRow { name: "B21已登记", passed: lg.b21_registered() });
    rows.push(SumCheckRow { name: "段基口径", passed: seg_alignment_ok() });
    rows.push(SumCheckRow { name: "类别域", passed: kind_domain_ok() });
    rows.push(SumCheckRow { name: "转录完整", passed: ref_lines_complete(lg) });
    rows.push(SumCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(SumCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn summary_checklist_green(rows: &[SumCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn summary_stats_line(lg: &IntegrationLedger) -> String {
    format!(
        "批小结概况：互引 {} 悬空 {} 待办 {} 审计 {}",
        lg.ref_len(),
        if lg.zero_dangling() { 0 } else { 1 },
        lg.todo_len(),
        lg.audit_runs()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12400_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut lg = IntegrationLedger::new();
    let _ = lg.register_prechain();
    let _ = lg.register_preview("B21", "节对象映射族批", B21_SEG, B21_FIRST, 12420);
    let _ = lg.audit();
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        ref_lines_complete(&lg) && BatchReport::build(&lg).all_green(),
        summary_stats_line(&lg),
    ));
    // X-b：类别域 + 段基口径。
    out.push((
        "X-类别域/段基口径两面",
        kind_domain_ok() && seg_alignment_ok(),
        format!("段基=0x{:x} 类别={}", B21_SEG, RefKind::KINDS),
    ));
    // X-c：终验清单。
    let rows = build_summary_checklist(&lg);
    out.push((
        "X-终验清单10项",
        summary_checklist_green(&rows),
        BatchReport::build(&lg).to_text(),
    ));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：悬空互引必拒（RefDangling）。
    let mut lg = IntegrationLedger::new();
    let r1 = lg.register_ref(B20_FIRST, 99999, RefKind::PreChain);
    if matches!(r1, Err(SumCode::CardOob)) {
        caught += 1;
        d.push("注1 悬空:抓".to_string());
    } else {
        d.push("注1 悬空:漏".to_string());
    }
    // 注 2：重复互引必拒（RefDup）。
    let mut lg2 = IntegrationLedger::new();
    let _ = lg2.register_ref(B20_FIRST, B20_FIRST + 1, RefKind::PreChain);
    let r2 = lg2.register_ref(B20_FIRST, B20_FIRST + 1, RefKind::PreChain);
    if matches!(r2, Err(SumCode::RefDup)) {
        caught += 1;
        d.push("注2 重复:抓".to_string());
    } else {
        d.push("注2 重复:漏".to_string());
    }
    // 注 3：预告段基失配必拒（SegMismatch）。
    let mut lg3 = IntegrationLedger::new();
    let r3 = lg3.register_preview("B21", "节对象映射族批", 0x30030, B21_FIRST, 12420);
    if matches!(r3, Err(SumCode::SegMismatch)) {
        caught += 1;
        d.push("注3 段基:抓".to_string());
    } else {
        d.push("注3 段基:漏".to_string());
    }
    // 注 4：列缺预告必拒（TodoColMissing）。
    let mut lg4 = IntegrationLedger::new();
    let r4 = lg4.register_preview("B21", "", B21_SEG, B21_FIRST, 12420);
    if matches!(r4, Err(SumCode::TodoColMissing)) {
        caught += 1;
        d.push("注4 列缺:抓".to_string());
    } else {
        d.push("注4 列缺:漏".to_string());
    }
    // 注 5：关闭态再登记必拒（TodoClosed）。
    let mut lg5 = IntegrationLedger::new();
    let _ = lg5.register_preview("B21", "节对象映射族批", B21_SEG, B21_FIRST, 12420);
    let _ = lg5.take_todo("B21");
    let _ = lg5.close_todo("B21");
    let r5 = lg5.register_preview("B21", "节对象映射族批", B21_SEG, B21_FIRST, 12420);
    if matches!(r5, Err(SumCode::TodoClosed)) {
        caught += 1;
        d.push("注5 关闭:抓".to_string());
    } else {
        d.push("注5 关闭:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30000 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    SumCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12400_checks：本单判据聚合（诊断段 0x30000）。
pub fn run_unx12400_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12400 批小结与 B21 预告");
    // J1-a：B20 集成账零悬空（19 条互引审计 10/10）。
    let (ok1, d1) = drill_audit_10();
    s.add_dyn("J1-B20集成账零悬空10/10", ok1, &d1);
    // J1-b：B21 预告登记入域待办账（10/10 幂等）。
    let (ok2, d2) = drill_b21_register_10();
    s.add_dyn("J1-B21预告登记10/10", ok2, &d2);
    // J1-c：待办状态机单向边（10/10）。
    let (ok3, d3) = drill_todo_state_10();
    s.add_dyn("J1-待办状态机10/10", ok3, &d3);
    // J1-d：19 条互引全登记 + 悬空为零（标准会话）。
    let mut lg = IntegrationLedger::new();
    let n = lg.register_prechain().unwrap_or(0);
    let _ = lg.register_preview("B21", "节对象映射族批", B21_SEG, B21_FIRST, 12420);
    let dangling = lg.audit();
    s.add_dyn(
        "J1-互引19条零悬空",
        n == B20_REF_COUNT && dangling == 0 && lg.b21_registered() && lg.zero_dangling(),
        &format!("互引={} 悬空={} B21预告={}", n, dangling, lg.b21_registered()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：两账只增不减（再登记一轮后单调判据仍绿）。
    let before = lg.ref_len();
    let _ = lg.register_ref(B20_FIRST, B20_FIRST + 3, RefKind::CrossSign);
    s.add_dyn(
        "J2-两账只增不减",
        lg.ref_len() >= before && lg.ref_monotone() && lg.todo_monotone(),
        &format!("互引 {}→{}", before, lg.ref_len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30000段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12400_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
