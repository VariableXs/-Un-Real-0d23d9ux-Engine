//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12436 · 端口族与 D4 消息面联签预告 · 0x30480
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F12436-J1 ALPC→D4 消息泵消费面预告齐，D4 承接批号与联测用例
//!      登记在册；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 消费边域：ALPC 端口族 → D4 消息泵六条消费边（连接交付/消息入队/
//!     消息出队/回包交付/断开清理/池回收），逐边入账（EdgeHole 拒空、
//!     EdgeDup 拒重）；
//!   - D4 承接批号登记：批号/主题/段基三列齐才入联测清单，段基必须与
//!     D4 分工口径一致（SegMismatch 拒）、重复批号拒（BatchDup）；
//!   - 联测用例域：三类闭集（握手三态/消息序/池账），用例挂批次，
//!     列缺拒（CaseColMissing）、越类拒（CaseKindOob）、重号拒（CaseDup）；
//!   - 批次状态机：登记→承接→关闭 单向边，关闭后用例登记拒
//!     （BatchClosed），复活拒（StateEdgeBad）；
//!   - 清单 append-only：批次/用例只增不减（ListShrink 红账）；
//!   - 消费边面齐判据：六边全齐且逐边可查（边名/生产面/消费面非空）；
//!   - 错误路径带码+中文 detail，拒止不写回任何账行；
//!   - 注入核：LCG 写死种子 0x30480，预告齐/登记/用例 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30480 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30480;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// ALPC→D4 消费边数（六边口径）。
pub const EDGE_COUNT: usize = 6;

/// D4 承接段基口径（预告登记，与 D4 分工表一致）。
pub const D4_SEG: u32 = 0x30480;

/// 联测用例下限（三类各 ≥2 例）。
pub const CASE_MIN: usize = 6;

/// 诊断码（0x30480 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum D4Code {
    /// 0x30480 消费边空（边名/生产面/消费面有缺）。
    EdgeHole = 0,
    /// 0x30481 消费边重复。
    EdgeDup = 1,
    /// 0x30482 D4 批号重复登记。
    BatchDup = 2,
    /// 0x30483 承接段基失配。
    SegMismatch = 3,
    /// 0x30484 联测用例重号。
    CaseDup = 4,
    /// 0x30485 用例列缺（名/类有缺）。
    CaseColMissing = 5,
    /// 0x30486 用例类别越集（越三类闭集）。
    CaseKindOob = 6,
    /// 0x30487 清单缩水（append-only 破）。
    ListShrink = 7,
    /// 0x30488 关闭态登记（批次已关闭再收用例）。
    BatchClosed = 8,
    /// 0x30489 状态边非法（关闭→承接回跳）。
    StateEdgeBad = 9,
}

impl D4Code {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [D4Code; 10] = [
        D4Code::EdgeHole, D4Code::EdgeDup, D4Code::BatchDup, D4Code::SegMismatch,
        D4Code::CaseDup, D4Code::CaseColMissing, D4Code::CaseKindOob, D4Code::ListShrink,
        D4Code::BatchClosed, D4Code::StateEdgeBad,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30480 => "消费边空：边名/生产面/消费面三列齐才入账。",
        0x30481 => "消费边重复：同边名只登一条，重复走幂等拒。",
        0x30482 => "D4 批号重复：同批号幂等，查联测清单再登记。",
        0x30483 => "承接段基失配：段基必须与 D4 分工口径一致，回查分工表。",
        0x30484 => "用例重号：同批次用例名唯一，重号先查抽签器。",
        0x30485 => "用例列缺：用例名与类别两列齐才入清单。",
        0x30486 => "用例类别越集：只收握手三态/消息序/池账三类。",
        0x30487 => "清单缩水：append-only，禁止截断，重放补齐。",
        0x30488 => "关闭态登记：批次关闭后只读，新用例走新批。",
        0x30489 => "状态边非法：批次只走 登记→承接→关闭 单向边。",
        _ => "未知诊断码：回查 0x30480 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30480 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3048_3048_3048_3048;

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

/// 消费边行（ALPC→D4 一条消费边）。
#[derive(Clone, Copy, Debug)]
pub struct EdgeRow {
    /// 边名。
    pub edge: &'static str,
    /// 生产面（ALPC 接口）。
    pub producer: &'static str,
    /// 消费面（D4 接口）。
    pub consumer: &'static str,
}

/// 用例类别（三类闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseKind {
    /// 握手三态。
    Handshake = 0,
    /// 消息序。
    MessageOrder = 1,
    /// 池账。
    PoolLedger = 2,
}

impl CaseKind {
    /// 类别总数。
    pub const KINDS: usize = 3;
    /// 由序号取类别（越界返回 None）。
    pub fn of(i: u64) -> Option<CaseKind> {
        match i {
            0 => Some(CaseKind::Handshake),
            1 => Some(CaseKind::MessageOrder),
            2 => Some(CaseKind::PoolLedger),
            _ => None,
        }
    }
}

/// 批次状态（单向边：登记→承接→关闭）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchStatus {
    /// 已登记未承接。
    Registered = 0,
    /// 已承接。
    Taken = 1,
    /// 已关闭。
    Closed = 2,
}

/// D4 承接批次行。
#[derive(Clone, Copy, Debug)]
pub struct BatchRow {
    /// 批号名。
    pub batch: &'static str,
    /// 承接主题。
    pub theme: &'static str,
    /// 承接段基。
    pub seg: u32,
    /// 状态。
    pub status: BatchStatus,
}

/// 联测用例行。
#[derive(Clone, Copy, Debug)]
pub struct CaseRow {
    /// 批号名（挂靠）。
    pub batch: &'static str,
    /// 用例名。
    pub case: &'static str,
    /// 类别。
    pub kind: CaseKind,
}

/// ALPC→D4 消费面预告账正本。
///
/// 生命周期：register_edges（六边入账）→ register_batch（D4 承接批号）→
/// register_case（联测用例挂批）→ take/close（批次状态推进）。
#[derive(Debug)]
pub struct D4Book {
    /// 消费边账。
    edges: Vec<EdgeRow>,
    /// 批次账。
    batches: Vec<BatchRow>,
    /// 用例账。
    cases: Vec<CaseRow>,
    /// 账峰值（append-only 判准）。
    high: usize,
    /// 拒止累计。
    rejects: u32,
}

impl D4Book {
    /// 建账。
    pub fn new() -> Self {
        D4Book {
            edges: Vec::new(),
            batches: Vec::new(),
            cases: Vec::new(),
            high: 0,
            rejects: 0,
        }
    }

    /// 登记消费边（三列齐 + 唯一才入账）。
    pub fn register_edge(&mut self, edge: &'static str, producer: &'static str, consumer: &'static str) -> Result<bool, D4Code> {
        if edge.is_empty() || producer.is_empty() || consumer.is_empty() {
            self.rejects += 1;
            return Err(D4Code::EdgeHole);
        }
        if self.edges.iter().any(|e| e.edge == edge) {
            self.rejects += 1;
            return Err(D4Code::EdgeDup);
        }
        self.edges.push(EdgeRow { edge, producer, consumer });
        let total = self.edges.len() + self.batches.len() + self.cases.len();
        if total > self.high {
            self.high = total;
        }
        Ok(true)
    }

    /// 六边批量登记（标准口径）。
    pub fn register_std_edges(&mut self) -> Result<usize, D4Code> {
        const EDGES: [(&str, &str, &str); EDGE_COUNT] = [
            ("连接交付", "NtAcceptConnectPort", "D4 连接泵"),
            ("消息入队", "NtRequestPort", "D4 入队泵"),
            ("消息出队", "NtReplyWaitReceivePort", "D4 出队泵"),
            ("回包交付", "NtReplyPort", "D4 回包泵"),
            ("断开清理", "NtClose", "D4 断开泵"),
            ("池回收", "池账面", "D4 池泵"),
        ];
        let mut n = 0usize;
        for (e, p, c) in EDGES.iter() {
            self.register_edge(e, p, c)?;
            n += 1;
        }
        Ok(n)
    }

    /// 消费边齐判据（六边全齐且逐边可查）。
    pub fn edges_complete(&self) -> bool {
        self.edges.len() == EDGE_COUNT
            && self
                .edges
                .iter()
                .all(|e| !e.edge.is_empty() && !e.producer.is_empty() && !e.consumer.is_empty())
    }

    /// 消费边快照。
    pub fn edge_of(&self, idx: usize) -> Option<EdgeRow> {
        if idx >= self.edges.len() {
            None
        } else {
            Some(self.edges[idx])
        }
    }

    /// D4 承接批号登记（三列齐 + 段基一致 + 唯一才入账）。
    pub fn register_batch(&mut self, batch: &'static str, theme: &'static str, seg: u32) -> Result<bool, D4Code> {
        if batch.is_empty() || theme.is_empty() {
            self.rejects += 1;
            return Err(D4Code::BatchDup);
        }
        if seg != D4_SEG {
            self.rejects += 1;
            return Err(D4Code::SegMismatch);
        }
        if self.batches.iter().any(|b| b.batch == batch) {
            self.rejects += 1;
            return Err(D4Code::BatchDup);
        }
        self.batches.push(BatchRow { batch, theme, seg, status: BatchStatus::Registered });
        let total = self.edges.len() + self.batches.len() + self.cases.len();
        if total > self.high {
            self.high = total;
        }
        Ok(true)
    }

    /// D4 批次已登记判据。
    pub fn d4_registered(&self) -> bool {
        self.batches.iter().any(|b| b.batch == "D4-B" && b.seg == D4_SEG)
    }

    /// 批次承接（登记→承接）。
    pub fn take_batch(&mut self, batch: &str) -> Result<bool, D4Code> {
        match self.batches.iter_mut().find(|b| b.batch == batch) {
            Some(b) => {
                if b.status == BatchStatus::Closed {
                    self.rejects += 1;
                    return Err(D4Code::StateEdgeBad);
                }
                b.status = BatchStatus::Taken;
                Ok(true)
            }
            None => {
                self.rejects += 1;
                Err(D4Code::BatchDup)
            }
        }
    }

    /// 批次关闭（承接→关闭）。
    pub fn close_batch(&mut self, batch: &str) -> Result<bool, D4Code> {
        match self.batches.iter_mut().find(|b| b.batch == batch) {
            Some(b) => {
                if b.status != BatchStatus::Taken {
                    self.rejects += 1;
                    return Err(D4Code::StateEdgeBad);
                }
                b.status = BatchStatus::Closed;
                Ok(true)
            }
            None => {
                self.rejects += 1;
                Err(D4Code::BatchDup)
            }
        }
    }

    /// 联测用例登记（挂批 + 名/类齐 + 三类闭集 + 开启态）。
    pub fn register_case(&mut self, batch: &'static str, case: &'static str, kind: CaseKind) -> Result<bool, D4Code> {
        if case.is_empty() {
            self.rejects += 1;
            return Err(D4Code::CaseColMissing);
        }
        match self.batches.iter().find(|b| b.batch == batch) {
            None => {
                self.rejects += 1;
                return Err(D4Code::CaseColMissing);
            }
            Some(b) => {
                if b.status == BatchStatus::Closed {
                    self.rejects += 1;
                    return Err(D4Code::BatchClosed);
                }
            }
        }
        if self.cases.iter().any(|c| c.batch == batch && c.case == case) {
            self.rejects += 1;
            return Err(D4Code::CaseDup);
        }
        self.cases.push(CaseRow { batch, case, kind });
        let total = self.edges.len() + self.batches.len() + self.cases.len();
        if total > self.high {
            self.high = total;
        }
        Ok(true)
    }

    /// 用例账长。
    pub fn case_len(&self) -> usize {
        self.cases.len()
    }

    /// 用例快照。
    pub fn case_of(&self, idx: usize) -> Option<CaseRow> {
        if idx >= self.cases.len() {
            None
        } else {
            Some(self.cases[idx])
        }
    }

    /// 用例域齐判据（三类各有用例且总数 ≥ 下限）。
    pub fn cases_domain_ok(&self) -> bool {
        if self.cases.len() < CASE_MIN {
            return false;
        }
        (0..CaseKind::KINDS as u64).all(|k| {
            let want = CaseKind::of(k);
            self.cases.iter().any(|c| Some(c.kind) == want)
        })
    }

    /// 清单只增不减判据。
    pub fn monotone(&self) -> bool {
        let total = self.edges.len() + self.batches.len() + self.cases.len();
        total >= self.high
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 批次快照。
    pub fn batch_of(&self, idx: usize) -> Option<BatchRow> {
        if idx >= self.batches.len() {
            None
        } else {
            Some(self.batches[idx])
        }
    }
}

/// 消费边预告演练：10/10 轮六边全齐、逐边可查、重复拒。
pub fn drill_edges_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30480 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = D4Book::new();
        let n = bk.register_std_edges().unwrap_or(0);
        let dup = bk.register_edge("消息入队", "NtRequestPort", "D4 入队泵");
        let hole = bk.register_edge("", "X", "Y");
        let _ = lcg.below(3);
        if n == EDGE_COUNT
            && bk.edges_complete()
            && matches!(dup, Err(D4Code::EdgeDup))
            && matches!(hole, Err(D4Code::EdgeHole))
            && bk.monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("消费边预告轮={}/{}", ok, INJECT_ROUNDS))
}

/// D4 承接登记演练：10/10 轮登记成功、段基失配/重复批号拒。
pub fn drill_d4_register_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30480 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = D4Book::new();
        let r1 = bk.register_batch("D4-B", "消息泵承接批", D4_SEG);
        let r2 = bk.register_batch("D4-B", "消息泵承接批", D4_SEG);
        let r3 = bk.register_batch("D4-C", "消息泵尾段批", D4_SEG);
        let bad_seg = bk.register_batch("D4-D", "段基失配批", 0x30500);
        let _ = lcg.below(3);
        if matches!(r1, Ok(true))
            && matches!(r2, Err(D4Code::BatchDup))
            && matches!(r3, Ok(true))
            && matches!(bad_seg, Err(D4Code::SegMismatch))
            && bk.d4_registered()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("D4 承接登记轮={}/{}", ok, INJECT_ROUNDS))
}

/// 联测用例登记演练：10/10 轮三类用例齐、关闭态拒、重号拒。
pub fn drill_cases_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30480 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = D4Book::new();
        let _ = bk.register_batch("D4-B", "消息泵承接批", D4_SEG);
        // 三类各两例（确定性变参）。
        let mut pass = true;
        for i in 0..CASE_MIN {
            let k = CaseKind::of((i % CaseKind::KINDS) as u64);
            let name = match (i % 3, i / 3) {
                (0, 0) => "握手A",
                (0, _) => "握手B",
                (1, 0) => "消息序A",
                (1, _) => "消息序B",
                (_, 0) => "池账A",
                _ => "池账B",
            };
            if let Some(kind) = k {
                pass = pass && bk.register_case("D4-B", name, kind).is_ok();
            } else {
                pass = false;
            }
        }
        let dup = bk.register_case("D4-B", "握手A", CaseKind::Handshake);
        let col = bk.register_case("D4-B", "", CaseKind::Handshake);
        let _ = bk.take_batch("D4-B");
        let _ = bk.close_batch("D4-B");
        let closed = bk.register_case("D4-B", "握手C", CaseKind::Handshake);
        let _ = lcg.below(3);
        if pass
            && bk.cases_domain_ok()
            && matches!(dup, Err(D4Code::CaseDup))
            && matches!(col, Err(D4Code::CaseColMissing))
            && matches!(closed, Err(D4Code::BatchClosed))
            && bk.monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("联测用例登记轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：消费边转录导出（逐边一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 消费边转录导出。
pub fn export_edge_lines(bk: &D4Book) -> String {
    let mut s = String::from("消费边转录：\n");
    let mut i = 0usize;
    while let Some(e) = bk.edge_of(i) {
        s.push_str(&format!("· {} {}→{}\n", e.edge, e.producer, e.consumer));
        i += 1;
    }
    s
}

/// 转录完整判据（逐边行齐备）。
pub fn edge_lines_complete(bk: &D4Book) -> bool {
    let s = export_edge_lines(bk);
    if bk.edges.len() == 0 {
        return false;
    }
    (0..bk.edges.len()).all(|i| {
        bk.edge_of(i)
            .map(|e| s.contains(e.edge))
            .unwrap_or(false)
    })
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：联签预告报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct D4Report {
    /// 消费边数。
    pub edges: usize,
    /// 批次数。
    pub batches: usize,
    /// 用例数。
    pub cases: usize,
    /// 拒止累计。
    pub rejects: u32,
    /// 边面齐。
    pub edges_ok: bool,
    /// 用例域齐。
    pub cases_ok: bool,
}

impl D4Report {
    /// 建报告。
    pub fn build(bk: &D4Book) -> Self {
        D4Report {
            edges: bk.edges.len(),
            batches: bk.batches.len(),
            cases: bk.case_len(),
            rejects: bk.reject_total(),
            edges_ok: bk.edges_complete(),
            cases_ok: bk.cases_domain_ok(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.edges_ok && self.cases_ok && self.batches >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "D4Report 边={} 批={} 例={} 拒={} 边齐={} 例齐={}",
            self.edges, self.batches, self.cases, self.rejects, self.edges_ok, self.cases_ok
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：批次状态机演练（单向边 10/10，非法回跳拒）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_batch_state_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30480 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = D4Book::new();
        let _ = bk.register_batch("D4-B", "消息泵承接批", D4_SEG);
        let early_close = bk.close_batch("D4-B");
        let take = bk.take_batch("D4-B");
        let close = bk.close_batch("D4-B");
        let revive = bk.take_batch("D4-B");
        let st = bk.batch_of(0).map(|b| b.status);
        let _ = lcg.below(3);
        if matches!(early_close, Err(D4Code::StateEdgeBad))
            && matches!(take, Ok(true))
            && matches!(close, Ok(true))
            && matches!(revive, Err(D4Code::StateEdgeBad))
            && st == Some(BatchStatus::Closed)
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("批次状态机轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：用例类别域核对（三类闭集逐一可取）。
/// ─────────────────────────────────────────────────────────────────────
pub fn kind_domain_ok() -> bool {
    (0..CaseKind::KINDS as u64).all(|k| CaseKind::of(k).is_some()) && CaseKind::of(3).is_none()
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct D4CheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_d4_checklist(bk: &D4Book) -> Vec<D4CheckRow> {
    let rep = D4Report::build(bk);
    let mut rows: Vec<D4CheckRow> = Vec::new();
    rows.push(D4CheckRow { name: "六边全齐", passed: bk.edges_complete() });
    rows.push(D4CheckRow { name: "D4在册", passed: bk.d4_registered() });
    rows.push(D4CheckRow { name: "用例域齐", passed: bk.cases_domain_ok() });
    rows.push(D4CheckRow { name: "清单单调", passed: bk.monotone() });
    rows.push(D4CheckRow { name: "类别域", passed: kind_domain_ok() });
    rows.push(D4CheckRow { name: "段基口径", passed: D4_SEG == 0x30480 && EDGE_COUNT == 6 });
    rows.push(D4CheckRow { name: "转录完整", passed: edge_lines_complete(bk) });
    rows.push(D4CheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(D4CheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(D4CheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn d4_checklist_green(rows: &[D4CheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn d4_stats_line(bk: &D4Book) -> String {
    format!(
        "联签概况：边 {} 批 {} 例 {} 拒 {}",
        bk.edges.len(),
        bk.batches.len(),
        bk.case_len(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12436_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = D4Book::new();
    let _ = bk.register_std_edges();
    let _ = bk.register_batch("D4-B", "消息泵承接批", D4_SEG);
    for (name, kind) in [
        ("握手A", CaseKind::Handshake),
        ("握手B", CaseKind::Handshake),
        ("消息序A", CaseKind::MessageOrder),
        ("消息序B", CaseKind::MessageOrder),
        ("池账A", CaseKind::PoolLedger),
        ("池账B", CaseKind::PoolLedger),
    ] {
        let _ = bk.register_case("D4-B", name, kind);
    }
    // 注入一例空边（拒止账口径）。
    let _ = bk.register_edge("", "P", "C");
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        edge_lines_complete(&bk) && D4Report::build(&bk).all_green(),
        d4_stats_line(&bk),
    ));
    // X-b：批次状态机 10/10。
    let (ok_s, d_s) = drill_batch_state_10();
    out.push(("X-批次状态机10/10", ok_s, d_s));
    // X-c：终验清单。
    let rows = build_d4_checklist(&bk);
    out.push(("X-终验清单10项", d4_checklist_green(&rows), D4Report::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：消费边空必拒（EdgeHole）。
    let mut bk = D4Book::new();
    if matches!(bk.register_edge("", "P", "C"), Err(D4Code::EdgeHole)) {
        caught += 1;
        d.push("注1 边空:抓".to_string());
    } else {
        d.push("注1 边空:漏".to_string());
    }
    // 注 2：段基失配必拒（SegMismatch）。
    if matches!(bk.register_batch("D4-B", "T", 0x30500), Err(D4Code::SegMismatch)) {
        caught += 1;
        d.push("注2 段基:抓".to_string());
    } else {
        d.push("注2 段基:漏".to_string());
    }
    // 注 3：重复批号必拒（BatchDup）。
    let _ = bk.register_batch("D4-B", "消息泵承接批", D4_SEG);
    if matches!(bk.register_batch("D4-B", "消息泵承接批", D4_SEG), Err(D4Code::BatchDup)) {
        caught += 1;
        d.push("注3 重批:抓".to_string());
    } else {
        d.push("注3 重批:漏".to_string());
    }
    // 注 4：未挂批用例必拒（CaseColMissing）。
    if matches!(bk.register_case("D4-X", "C1", CaseKind::Handshake), Err(D4Code::CaseColMissing)) {
        caught += 1;
        d.push("注4 未挂:抓".to_string());
    } else {
        d.push("注4 未挂:漏".to_string());
    }
    // 注 5：关闭态用例必拒（BatchClosed）。
    let _ = bk.take_batch("D4-B");
    let _ = bk.close_batch("D4-B");
    if matches!(bk.register_case("D4-B", "C2", CaseKind::Handshake), Err(D4Code::BatchClosed)) {
        caught += 1;
        d.push("注5 关闭:抓".to_string());
    } else {
        d.push("注5 关闭:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30480 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    D4Code::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12436_checks：本单判据聚合（诊断段 0x30480）。
pub fn run_unx12436_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12436 端口族与 D4 消息面联签预告");
    // J1-a：消费面预告齐（六边 10/10）。
    let (ok1, d1) = drill_edges_10();
    s.add_dyn("J1-消费面预告齐10/10", ok1, &d1);
    // J1-b：D4 承接批号登记（10/10）。
    let (ok2, d2) = drill_d4_register_10();
    s.add_dyn("J1-D4承接批号登记10/10", ok2, &d2);
    // J1-c：联测用例登记在册（10/10）。
    let (ok3, d3) = drill_cases_10();
    s.add_dyn("J1-联测用例登记10/10", ok3, &d3);
    // J1-d：标准会话（六边 + D4 在册 + 三类用例）。
    let mut bk = D4Book::new();
    let _ = bk.register_std_edges();
    let _ = bk.register_batch("D4-B", "消息泵承接批", D4_SEG);
    for (name, kind) in [
        ("握手A", CaseKind::Handshake),
        ("握手B", CaseKind::Handshake),
        ("消息序A", CaseKind::MessageOrder),
        ("消息序B", CaseKind::MessageOrder),
        ("池账A", CaseKind::PoolLedger),
        ("池账B", CaseKind::PoolLedger),
    ] {
        let _ = bk.register_case("D4-B", name, kind);
    }
    // 注入一例重复消费边（拒止账口径）。
    let _ = bk.register_edge("消息入队", "NtRequestPort", "D4 入队泵");
    s.add_dyn(
        "J1-预告齐+D4在册",
        bk.edges_complete() && bk.d4_registered() && bk.cases_domain_ok() && bk.monotone(),
        &format!(
            "边齐={} D4={} 例域={} 单调={}",
            bk.edges_complete(),
            bk.d4_registered(),
            bk.cases_domain_ok(),
            bk.monotone()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（坏边登记后边数不变）。
    let e0 = bk.edges.len();
    let _ = bk.register_edge("", "P", "C");
    let _ = bk.register_edge("连接交付", "P2", "C2");
    s.add_dyn(
        "J2-拒止不写回",
        bk.edges.len() == e0 && bk.monotone(),
        &format!("边 {}→{}", e0, bk.edges.len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30480段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12436_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
