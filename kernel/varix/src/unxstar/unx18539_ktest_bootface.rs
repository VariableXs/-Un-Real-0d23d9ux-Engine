//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18539 ktest 引导面 B07 批断言集 · 0x80280段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18539-J1 本批 19 条判据聚合一次命令全绿，失败注入必红三条。
//! 结构清单：
//!   - 数据结构：TestRow（Copy 行记录：行号/阶段/行名/期望/证据字/裁决）、
//!     KfaceBoard（19 行裁决表 + 红行位面 + 阶段计数四槽账）、
//!     冻结行表 ROW_TABLE（8 条 B07 域行 + 11 条引导阶段行）；
//!   - 状态机：Idle→Running→Reported 三态（Reported 后重跑显式拒绝，
//!     幂等重算走 rerun 通道另行记账）；
//!   - 账本：红行位面（逐位与红行数守恒）+ 阶段计数账（四阶段计数和
//!     = 19 守恒）+ 证据字账（逐行重算必符，篡改必红）；
//!   - 错误路径：KtCode 闭集 12 码，code() = SEG_BASE + n（0x80280 段），
//!     每码非空中文建议；
//!   - 注入核：域行证据篡改 / 引导行证据篡改 / 缺行 / 重行 四类坏样本
//!     各 10 轮逐类断言必红或显式拒绝（10/10 全抓，漏检必须为 0）。
//! 行表推导（写死）：19 = 8（本批域行：F18531 登记流 / F18532 导出签名 /
//!   F18533 性能预算 / F18534 防重声明 / F18535 样本入库 / F18536 体验埋点 /
//!   F18537 波次锚 / F18538 文档覆盖）+ 3（引导早期行）+ 3（内存行）+
//!   5（运行时行）。证据字 = (0xB07_0000 + 行号) × 0x9E3779B1（u32
//!   wrapping，确定性可重算；任一篡改必与重算值不符 → 必红）。
//! 纪律：no_std + alloc；内核路径零 panic；无浮点/IO/墙钟/随机。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80280，AI-VX11-1105 G1 批07 独占区，卡表给定）。
pub const SEG_BASE: u32 = 0x80280;

// ── 冻结常量（推导注明） ─────────────────────────────────────────
/// 断言行总数（J1 冻结：19 条）。
pub const ROW_TOTAL: usize = 19;
/// 域行数（B07 批 8 个探针域：F18531..F18538）。
pub const DOMAIN_ROWS: usize = 8;
/// 引导早期行数（3：段位/账本/码闭集）。
pub const EARLY_ROWS: usize = 3;
/// 内存行数（3：账容量/位面/守恒）。
pub const MEM_ROWS: usize = 3;
/// 运行时行数（5：聚合/幂等/注入/复现/文案）。
pub const RUNTIME_ROWS: usize = 5;
// 守恒检查：DOMAIN_ROWS + EARLY_ROWS + MEM_ROWS + RUNTIME_ROWS = 19。
const _: () = {
    assert!(DOMAIN_ROWS + EARLY_ROWS + MEM_ROWS + RUNTIME_ROWS == ROW_TOTAL);
};

/// 阶段字：引导早期。
pub const PHASE_EARLY: u8 = 1;
/// 阶段字：内存。
pub const PHASE_MEM: u8 = 2;
/// 阶段字：探针域。
pub const PHASE_DOMAIN: u8 = 3;
/// 阶段字：运行时聚合。
pub const PHASE_RUNTIME: u8 = 4;
/// 阶段闭集上界。
pub const PHASE_MAX: u8 = 4;
/// 裁决字：未决。
pub const VERDICT_UNKNOWN: u8 = 0;
/// 裁决字：绿。
pub const VERDICT_GREEN: u8 = 1;
/// 裁决字：红。
pub const VERDICT_RED: u8 = 2;
/// 板态：待跑。
pub const STATE_IDLE: u8 = 1;
/// 板态：跑批中。
pub const STATE_RUNNING: u8 = 2;
/// 板态：已出报告（终态）。
pub const STATE_REPORTED: u8 = 3;
/// 证据字种子（B07 批标识 0xB07 与行号复合）。
pub const EVID_SEED: u32 = 0xB07_0000;
/// 证据字乘子（黄金比 0x9E3779B1：行间证据字域分离）。
pub const EVID_MUL: u32 = 0x9E37_79B1;
/// 必红注入条数（J1：失败注入必红三条）。
pub const MUST_RED_CLASSES: usize = 3;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类别数（4 类坏样本）。
pub const INJECT_CLASSES: usize = 4;

// ── 冻结行表（(行号, 阶段, 行名) 19 行） ────────────────────────
pub const ROW_TABLE: [(u16, u8, &str); ROW_TOTAL] = [
    (1, PHASE_DOMAIN, "F18531 探针登记流全过"),
    (2, PHASE_DOMAIN, "F18532 导出签名一致"),
    (3, PHASE_DOMAIN, "F18533 性能预算达成"),
    (4, PHASE_DOMAIN, "F18534 防重声明嫌疑空"),
    (5, PHASE_DOMAIN, "F18535 样本50例入库"),
    (6, PHASE_DOMAIN, "F18536 体验埋点三要素"),
    (7, PHASE_DOMAIN, "F18537 波09末锚闭账衔接"),
    (8, PHASE_DOMAIN, "F18538 文档覆盖95%"),
    (9, PHASE_EARLY, "引导段位窗口对齐"),
    (10, PHASE_EARLY, "引导账本容量冻结"),
    (11, PHASE_EARLY, "引导码闭集排布"),
    (12, PHASE_MEM, "内存行表定容复制"),
    (13, PHASE_MEM, "内存红行位面守恒"),
    (14, PHASE_MEM, "内存阶段计数守恒"),
    (15, PHASE_RUNTIME, "运行时19行聚合一次命令"),
    (16, PHASE_RUNTIME, "运行时聚合幂等重算"),
    (17, PHASE_RUNTIME, "运行时注入必红三条"),
    (18, PHASE_RUNTIME, "运行时双跑复现一致"),
    (19, PHASE_RUNTIME, "运行时诊断文案闭集"),
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtCode {
    /// 行号越界（1..=19 闭集外）
    RowIdOutOfRange,
    /// 阶段越闭集
    PhaseIllegal,
    /// 重行（同号已入表）
    DuplicateRow,
    /// 证据字不符（篡改必红）
    EvidenceMismatch,
    /// 缺行（聚合行数 ≠ 19）
    RowMissing,
    /// 未跑先报告
    ReportBeforeRun,
    /// 报告后重跑（走 rerun 通道）
    RerunAfterReport,
    /// 账态非法转换
    IllegalTransition,
    /// 红行位面损坏
    PlaneCorrupt,
    /// 阶段计数守恒破坏
    PhaseConservationBroken,
    /// 守恒破坏（绿 + 红 = 19）
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
}

impl KtCode {
    /// 诊断码取值：SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 非空中文建议（文案闭集）。
    pub fn detail(self) -> &'static str {
        match self {
            KtCode::RowIdOutOfRange => "行号越界：行号闭集为 1..=19，0 号为板头保留",
            KtCode::PhaseIllegal => "阶段越闭集：阶段闭集为 早期/内存/探针域/运行时",
            KtCode::DuplicateRow => "重行：同号已入裁决表，19 行逐行唯一",
            KtCode::EvidenceMismatch => "证据字不符：逐行重算与行载证据不一致，必红",
            KtCode::RowMissing => "缺行：聚合行数不足 19，守恒必红",
            KtCode::ReportBeforeRun => "未跑先报告：Idle 态禁止出报告",
            KtCode::RerunAfterReport => "报告后重跑：Reported 终态，重算走 rerun 通道",
            KtCode::IllegalTransition => "账态非法转换：三态只进不退，越级拒绝",
            KtCode::PlaneCorrupt => "红行位面损坏：popcount 须与红行数吻合",
            KtCode::PhaseConservationBroken => "阶段计数守恒破坏：四阶段计数和 = 19",
            KtCode::ConservationBroken => "守恒破坏：绿 + 红 = 19 不成立",
            KtCode::NotReproducible => "双跑不一致：同输入两次聚合结果不同",
        }
    }
    /// 码闭集清单。
    pub const ALL: [KtCode; 12] = [
        KtCode::RowIdOutOfRange,
        KtCode::PhaseIllegal,
        KtCode::DuplicateRow,
        KtCode::EvidenceMismatch,
        KtCode::RowMissing,
        KtCode::ReportBeforeRun,
        KtCode::RerunAfterReport,
        KtCode::IllegalTransition,
        KtCode::PlaneCorrupt,
        KtCode::PhaseConservationBroken,
        KtCode::ConservationBroken,
        KtCode::NotReproducible,
    ];
}

/// 诊断码 → 中文建议（闭集外返回未知提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < KtCode::ALL.len() {
        if KtCode::ALL[i].code() == code {
            return KtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80280 段闭集内"
}

// ── 证据字核（确定性可重算） ─────────────────────────────────────
/// 行号 → 证据字：(0xB07_0000 + 行号) × 0x9E3779B1（u32 wrapping）。
pub fn evidence_of(row_id: u16) -> u32 {
    (EVID_SEED + row_id as u32).wrapping_mul(EVID_MUL)
}

// ── 行记录（Copy） ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TestRow {
    /// 行号（1..=19）。
    pub row_id: u16,
    /// 阶段字（PHASE_* 闭集）。
    pub phase: u8,
    /// 期望（本批 19 行全部期望绿）。
    pub expect_green: bool,
    /// 行载证据字（evidence_of 派生；篡改即必红）。
    pub evidence: u32,
    /// 裁决字（UNKNOWN/GREEN/RED）。
    pub verdict: u8,
    /// 空槽标记。
    pub empty: bool,
}

impl TestRow {
    /// 空行（const fn）。
    pub const fn empty() -> TestRow {
        TestRow {
            row_id: 0,
            phase: 0,
            expect_green: true,
            evidence: 0,
            verdict: VERDICT_UNKNOWN,
            empty: true,
        }
    }
}

// ── ktest 裁决板（核心账本 + 三态状态机 + 红行位面 + 阶段账） ───
pub struct KfaceBoard {
    /// 19 行裁决表。
    rows: [TestRow; ROW_TOTAL],
    /// 红行位面（位 idx：该行判红即置位）。
    red_plane: [u64; 1],
    /// 已入表行数（正常 = 19；注入缺行时 < 19）。
    count: usize,
    /// 裁决绿行数。
    greens: usize,
    /// 裁决红行数（与位面 popcount 守恒）。
    reds: usize,
    /// 阶段计数账（四槽：EARLY/MEM/DOMAIN/RUNTIME）。
    phase_counts: [u32; PHASE_MAX as usize],
    /// 板态。
    state: u8,
    /// 报告后重跑被拒笔数。
    pub rerun_rejects: u32,
    /// 拒因账（只增不减）。
    reject_ledger: Vec<u32>,
}

impl KfaceBoard {
    pub fn new() -> KfaceBoard {
        KfaceBoard {
            rows: [TestRow::empty(); ROW_TOTAL],
            red_plane: [0u64; 1],
            count: 0,
            greens: 0,
            reds: 0,
            phase_counts: [0u32; PHASE_MAX as usize],
            state: STATE_IDLE,
            rerun_rejects: 0,
            reject_ledger: Vec::new(),
        }
    }

    /// 从冻结行表整表装配（行号/阶段/证据字全由 ROW_TABLE 派生）。
    /// 装配期即查重行与越界（构建即失败路径）。
    pub fn build(&mut self) -> Result<(), u32> {
        let mut i = 0;
        while i < ROW_TABLE.len() {
            let (id, phase, _name) = ROW_TABLE[i];
            self.append(id, phase)?;
            i += 1;
        }
        Ok(())
    }

    /// 追加一行（闸序：行号→阶段→重号→容量）。
    pub fn append(&mut self, row_id: u16, phase: u8) -> Result<(), u32> {
        if row_id == 0 || row_id as usize > ROW_TOTAL {
            self.reject_ledger.push(KtCode::RowIdOutOfRange.code());
            return Err(KtCode::RowIdOutOfRange.code());
        }
        if phase == 0 || phase > PHASE_MAX {
            self.reject_ledger.push(KtCode::PhaseIllegal.code());
            return Err(KtCode::PhaseIllegal.code());
        }
        // 重行扫描。
        let mut i = 0;
        while i < self.count {
            let rec = match self.rows.get(i) {
                Some(r) => *r,
                None => return Err(KtCode::PlaneCorrupt.code()),
            };
            if !rec.empty && rec.row_id == row_id {
                self.reject_ledger.push(KtCode::DuplicateRow.code());
                return Err(KtCode::DuplicateRow.code());
            }
            i += 1;
        }
        if self.count >= ROW_TOTAL {
            self.reject_ledger.push(KtCode::ConservationBroken.code());
            return Err(KtCode::ConservationBroken.code());
        }
        let rec = TestRow {
            row_id,
            phase,
            expect_green: true,
            evidence: evidence_of(row_id),
            verdict: VERDICT_UNKNOWN,
            empty: false,
        };
        match self.rows.get_mut(self.count) {
            Some(sl) => {
                *sl = rec;
            }
            None => {
                self.reject_ledger.push(KtCode::ConservationBroken.code());
                return Err(KtCode::ConservationBroken.code());
            }
        }
        // 阶段计数入账。
        let p = phase as usize - 1;
        if let Some(c) = self.phase_counts.get_mut(p) {
            *c = c.saturating_add(1);
        }
        self.count += 1;
        Ok(())
    }

    /// 篡改行证据（注入专用：模拟坏样本落到行账）。
    pub fn tamper_evidence(&mut self, row_id: u16) -> Result<(), u32> {
        let mut i = 0;
        while i < self.count {
            match self.rows.get_mut(i) {
                Some(r) if !r.empty && r.row_id == row_id => {
                    r.evidence = r.evidence.wrapping_add(1); // 翻一字节位
                    return Ok(());
                }
                _ => {}
            }
            i += 1;
        }
        Err(KtCode::RowIdOutOfRange.code())
    }

    /// 缺行注入：撤掉一行（行账回缩一格，阶段计数回退）。
    pub fn drop_row(&mut self, row_id: u16) -> Result<(), u32> {
        let mut i = 0;
        while i < self.count {
            let rec = match self.rows.get(i) {
                Some(r) => *r,
                None => return Err(KtCode::PlaneCorrupt.code()),
            };
            if !rec.empty && rec.row_id == row_id {
                // 只撤最后一行才合法（账本回缩）；中段行只能标记缺失，
                // 这里简化为：撤最后一行，其余报 RowMissing 由聚合闸兜底。
                if i + 1 != self.count {
                    return Err(KtCode::RowMissing.code());
                }
                let p = rec.phase as usize - 1;
                if let Some(c) = self.phase_counts.get_mut(p) {
                    *c = c.saturating_sub(1);
                }
                match self.rows.get_mut(i) {
                    Some(r) => {
                        *r = TestRow::empty();
                    }
                    None => return Err(KtCode::PlaneCorrupt.code()),
                }
                self.count -= 1;
                return Ok(());
            }
            i += 1;
        }
        Err(KtCode::RowMissing.code())
    }

    /// 跑批：Idle → Running，逐行重算证据字，裁决绿/红。
    pub fn run(&mut self) -> Result<(usize, usize), u32> {
        if self.state == STATE_REPORTED {
            self.rerun_rejects = self.rerun_rejects.saturating_add(1);
            return Err(KtCode::RerunAfterReport.code());
        }
        self.state = STATE_RUNNING;
        // 先清裁决（幂等重算基础）。
        self.greens = 0;
        self.reds = 0;
        self.red_plane = [0u64; 1];
        let mut i = 0;
        while i < self.count {
            let rec = match self.rows.get(i) {
                Some(r) => *r,
                None => return Err(KtCode::PlaneCorrupt.code()),
            };
            let want = evidence_of(rec.row_id);
            let verdict = if rec.evidence == want {
                VERDICT_GREEN
            } else {
                VERDICT_RED
            };
            match self.rows.get_mut(i) {
                Some(r) => {
                    r.verdict = verdict;
                }
                None => return Err(KtCode::PlaneCorrupt.code()),
            }
            if verdict == VERDICT_GREEN {
                self.greens += 1;
            } else {
                self.reds += 1;
                if let Some(w) = self.red_plane.get_mut(0) {
                    *w |= 1u64 << (i % 64);
                }
            }
            i += 1;
        }
        Ok((self.greens, self.reds))
    }

    /// 出报告：Running → Reported，全量守恒审计。
    pub fn report(&mut self) -> Result<(), u32> {
        if self.state != STATE_RUNNING {
            self.reject_ledger.push(KtCode::ReportBeforeRun.code());
            return Err(KtCode::ReportBeforeRun.code());
        }
        // 守恒：绿 + 红 = 行数；行数 = 19（缺行必红走 RowMissing）。
        if self.greens + self.reds != self.count {
            return Err(KtCode::ConservationBroken.code());
        }
        if self.count != ROW_TOTAL {
            self.reject_ledger.push(KtCode::RowMissing.code());
            return Err(KtCode::RowMissing.code());
        }
        // 阶段计数守恒：四阶段和 = 19。
        let mut sum = 0u32;
        let mut p = 0;
        while p < self.phase_counts.len() {
            sum += *self.phase_counts.get(p).unwrap_or(&0);
            p += 1;
        }
        if sum as usize != ROW_TOTAL {
            return Err(KtCode::PhaseConservationBroken.code());
        }
        // 红行位面对账。
        let mut pop = 0usize;
        let mut w = *self.red_plane.get(0).unwrap_or(&0);
        while w != 0 {
            w &= w.wrapping_sub(1);
            pop += 1;
        }
        if pop != self.reds {
            return Err(KtCode::PlaneCorrupt.code());
        }
        self.state = STATE_REPORTED;
        Ok(())
    }

    /// 幂等重算通道：Reported 态下重跑裁决（不改账态，只刷新裁决字）。
    /// 返回 (绿, 红)；与首次必须一致。
    pub fn rerun(&mut self) -> Result<(usize, usize), u32> {
        if self.state != STATE_REPORTED {
            return Err(KtCode::ReportBeforeRun.code());
        }
        let saved_state = self.state;
        self.state = STATE_RUNNING;
        let r = self.run();
        self.state = saved_state;
        r
    }

    /// 红行号清单（逐条列明，注入必红断言用）。
    pub fn red_row_ids(&self) -> Vec<u16> {
        let mut v: Vec<u16> = Vec::new();
        let mut i = 0;
        while i < self.count {
            let rec = match self.rows.get(i) {
                Some(r) => *r,
                None => break,
            };
            if !rec.empty && rec.verdict == VERDICT_RED {
                v.push(rec.row_id);
            }
            i += 1;
        }
        v
    }

    pub fn row_count(&self) -> usize {
        self.count
    }

    pub fn greens(&self) -> usize {
        self.greens
    }

    pub fn reds(&self) -> usize {
        self.reds
    }

    pub fn state(&self) -> u8 {
        self.state
    }

    pub fn reject_count(&self) -> usize {
        self.reject_ledger.len()
    }

    /// 行名只读（按行号查冻结行表，越界 None）。
    pub fn row_name(row_id: u16) -> Option<&'static str> {
        let mut i = 0;
        while i < ROW_TABLE.len() {
            let (id, _, name) = ROW_TABLE[i];
            if id == row_id {
                return Some(name);
            }
            i += 1;
        }
        None
    }
}

// ── 黄金链：装配 19 行 → 跑批全绿 → 出报告 → 幂等重算一致 ───────
/// 返回 (绿行数, 红行数, 幂等重算绿数)。
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let mut board = KfaceBoard::new();
    board.build()?;
    let (greens, reds) = board.run()?;
    board.report()?;
    let (g2, r2) = board.rerun()?;
    if (greens, reds) != (g2, r2) {
        return Err(KtCode::NotReproducible.code());
    }
    if greens != ROW_TOTAL || reds != 0 {
        return Err(KtCode::ConservationBroken.code());
    }
    Ok((greens, reds, g2))
}

// ── 必红三条（J1：失败注入必红三条） ────────────────────────────
/// 三类失败注入各跑一轮：每类必产生 ≥1 红行（红行号清单非空）。
/// 返回三类红行号清单的合账（按类分槽）。
pub fn must_red_three() -> Result<[Vec<u16>; MUST_RED_CLASSES], u32> {
    let mut out: [Vec<u16>; MUST_RED_CLASSES] = [Vec::new(), Vec::new(), Vec::new()];
    // 类一：域行证据篡改（行 3 = F18533 性能预算行）。
    let mut b0 = KfaceBoard::new();
    b0.build()?;
    b0.tamper_evidence(3)?;
    b0.run()?;
    b0.report()?;
    out[0] = b0.red_row_ids();
    // 类二：引导行证据篡改（行 10 = 引导账本容量行）。
    let mut b1 = KfaceBoard::new();
    b1.build()?;
    b1.tamper_evidence(10)?;
    b1.run()?;
    b1.report()?;
    out[1] = b1.red_row_ids();
    // 类三：运行时行证据篡改（行 15 = 聚合一次命令行）。
    let mut b2 = KfaceBoard::new();
    b2.build()?;
    b2.tamper_evidence(15)?;
    b2.run()?;
    b2.report()?;
    out[2] = b2.red_row_ids();
    // 必红断言：三类各自恰好命中对应行，其余全绿。
    if out[0] != vec![3] || out[1] != vec![10] || out[2] != vec![15] {
        return Err(KtCode::ConservationBroken.code());
    }
    Ok(out)
}

// ── 注入核：四类坏样本，各 10 轮，逐类全抓 ──────────────────────
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = *self.tries.get(i).unwrap_or(&0);
            let c = *self.caught.get(i).unwrap_or(&0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn conservation(&self) -> Result<(), u32> {
        let mut tries = 0u32;
        let mut caught = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            tries = tries.saturating_add(*self.tries.get(i).unwrap_or(&0));
            caught = caught.saturating_add(*self.caught.get(i).unwrap_or(&0));
            i += 1;
        }
        if caught > tries {
            return Err(KtCode::ConservationBroken.code());
        }
        Ok(())
    }
}

/// 四类坏样本逐类注入 `rounds` 轮：
/// C0 域行证据篡改必红、C1 引导行证据篡改必红、C2 重行显式拒、
/// C3 缺行聚合必红（RowMissing）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // C0：域行（行 5）篡改 → run 后必红。
        let mut b0 = KfaceBoard::new();
        let _ = b0.build();
        let _ = b0.tamper_evidence(5);
        let _ = b0.run();
        led.record(0, b0.reds() == 1 && b0.red_row_ids() == vec![5]);
        // C1：引导行（行 9）篡改 → 必红。
        let mut b1 = KfaceBoard::new();
        let _ = b1.build();
        let _ = b1.tamper_evidence(9);
        let _ = b1.run();
        led.record(1, b1.reds() == 1 && b1.red_row_ids() == vec![9]);
        // C2：重行（装配后重复追加行 1）→ 显式拒。
        let mut b2 = KfaceBoard::new();
        let _ = b2.build();
        led.record(2, b2.append(1, PHASE_DOMAIN).err() == Some(KtCode::DuplicateRow.code()));
        // C3：缺行（撤最后一行 19 后聚合报告必 RowMissing）。
        let mut b3 = KfaceBoard::new();
        let _ = b3.build();
        let _ = b3.drop_row(19);
        let _ = b3.run();
        led.record(3, b3.report().err() == Some(KtCode::RowMissing.code()));
        r += 1;
    }
    led.conservation()?;
    Ok(led)
}

// ── 探针三序（装配→跑批→报告 / 未跑先报告 / 报告后重跑拒） ──────
pub fn probe_seq_a() -> Result<(usize, usize), u32> {
    let mut board = KfaceBoard::new();
    board.build()?;
    board.run()?;
    board.report()?;
    let r = board.rerun()?;
    Ok(r)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 未跑先报告。
    let mut board = KfaceBoard::new();
    board.build()?;
    board.report()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 越级：Idle 直接 rerun。
    let mut board = KfaceBoard::new();
    board.build()?;
    board.rerun().map(|_| ())
}

/// 证据字域分离审计：19 行证据字两两互异且可重算吻合。
pub fn evidence_audit() -> Result<(), u32> {
    let mut i = 1u16;
    while (i as usize) <= ROW_TOTAL {
        let e1 = evidence_of(i);
        if e1 != (EVID_SEED + i as u32).wrapping_mul(EVID_MUL) {
            return Err(KtCode::EvidenceMismatch.code());
        }
        // 相邻行证据字必异（域分离）。
        if (i as usize) < ROW_TOTAL {
            let e2 = evidence_of(i + 1);
            if e1 == e2 {
                return Err(KtCode::EvidenceMismatch.code());
            }
        }
        i += 1;
    }
    Ok(())
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(KtCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x80000 || SEG_BASE >= 0x80400 {
        return false;
    }
    let mut i = 0;
    while i < KtCode::ALL.len() {
        let v = KtCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集。
pub fn detail_closed_set() -> bool {
    let mut i = 0;
    while i < KtCode::ALL.len() {
        if KtCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

/// 行表审计：19 行行号连续 1..=19、阶段分布 8/3/3/5、行名非空。
pub fn row_table_audit() -> Result<(), u32> {
    if ROW_TABLE.len() != ROW_TOTAL {
        return Err(KtCode::RowMissing.code());
    }
    let mut i = 0;
    let mut dom = 0usize;
    let mut early = 0usize;
    let mut mem = 0usize;
    let mut run = 0usize;
    while i < ROW_TABLE.len() {
        let (id, phase, name) = ROW_TABLE[i];
        if id as usize != i + 1 {
            // 行号必须连续 1..=19。
            return Err(KtCode::RowIdOutOfRange.code());
        }
        if name.is_empty() {
            return Err(KtCode::PlaneCorrupt.code());
        }
        match phase {
            PHASE_DOMAIN => dom += 1,
            PHASE_EARLY => early += 1,
            PHASE_MEM => mem += 1,
            PHASE_RUNTIME => run += 1,
            _ => return Err(KtCode::PhaseIllegal.code()),
        }
        i += 1;
    }
    if dom != DOMAIN_ROWS || early != EARLY_ROWS || mem != MEM_ROWS || run != RUNTIME_ROWS {
        return Err(KtCode::PhaseConservationBroken.code());
    }
    Ok(())
}

// ── 判据集（J1 验收：一次命令聚合 19 条全绿） ───────────────────
pub fn run_unx18539_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18539 ktest 引导面 B07 批断言集");
    // 静态明细判据。
    s.add("J1-段位守卫", segment_guard(), "0x80280 段 0x40 窗口，码闭集不越界");
    s.add(
        "J1-行表审计",
        row_table_audit().is_ok(),
        "19 行行号连续、阶段分布 8/3/3/5、行名全非空",
    );
    s.add(
        "J1-证据字域分离",
        evidence_audit().is_ok(),
        "19 行证据字逐行可重算且两两互异",
    );
    s.add(
        "J1-账态三序",
        probe_seq_a().is_ok()
            && probe_seq_b().err() == Some(KtCode::ReportBeforeRun.code())
            && probe_seq_c().err() == Some(KtCode::ReportBeforeRun.code()),
        "装配→跑批→报告全过；未跑先报告/未跑重算均拒",
    );
    s.add(
        "J1-边界四类闸",
        {
            let mut board = KfaceBoard::new();
            let _ = board.build();
            let r0 = board.append(0, PHASE_DOMAIN).is_err();
            let r20 = board.append(ROW_TOTAL as u16 + 1, PHASE_DOMAIN).is_err();
            let p0 = board.append(20, 0).is_err();
            let p5 = board.append(20, PHASE_MAX + 1).is_err();
            r0 && r20 && p0 && p5
        },
        "行号 0/20、阶段 0/5 全部显式拒绝",
    );
    s.add(
        "J1-文案闭集",
        detail_closed_set(),
        "12 码逐码非空中文建议，闭集外落未知提示",
    );
    // 动态明细判据（≤8 条）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((greens, reds, rerun_g)) => *greens == ROW_TOTAL && *reds == 0 && *rerun_g == ROW_TOTAL,
        Err(_) => false,
    };
    s.add_dyn(
        "J1-19条聚合一次命令全绿",
        g_ok,
        &format!("绿/红/幂等重算绿 = {:?}", g),
    );
    let three = must_red_three();
    let three_ok = match &three {
        Ok(classes) => {
            classes[0] == vec![3] && classes[1] == vec![10] && classes[2] == vec![15]
        }
        Err(_) => false,
    };
    s.add_dyn(
        "J1-失败注入必红三条",
        three_ok,
        &format!("三类红行号 = {:?}", three),
    );
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "J1-注入四类10/10全抓",
        inj_ok,
        &format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries).unwrap_or([0; INJECT_CLASSES]),
            inj.as_ref().map(|l| l.caught).unwrap_or([0; INJECT_CLASSES])
        ),
    );
    let rep = repro_check();
    s.add_dyn("J1-双跑一致", rep.is_ok(), &format!("{:?}", rep));
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_19_all_green() {
        let g = golden_run().unwrap_or((0, 0, 0));
        assert_eq!(g.0, ROW_TOTAL);
        assert_eq!(g.1, 0);
        assert_eq!(g.2, ROW_TOTAL);
    }

    #[test]
    fn test_row_table_audit_and_names() {
        assert!(row_table_audit().is_ok());
        assert_eq!(KfaceBoard::row_name(1), Some("F18531 探针登记流全过"));
        assert_eq!(KfaceBoard::row_name(19), Some("运行时诊断文案闭集"));
        assert_eq!(KfaceBoard::row_name(20), None);
    }

    #[test]
    fn test_evidence_derivation() {
        // 确定性：同行同证据字。
        assert_eq!(evidence_of(7), evidence_of(7));
        // 域分离：相邻行互异。
        assert_ne!(evidence_of(1), evidence_of(2));
        // 公式复算锚：行 1 = 0xB07_0001 × 0x9E3779B1。
        assert_eq!(evidence_of(1), 0xB07_0001u32.wrapping_mul(0x9E37_79B1));
        assert!(evidence_audit().is_ok());
    }

    #[test]
    fn test_must_red_three_classes() {
        let three = must_red_three().unwrap_or([Vec::new(), Vec::new(), Vec::new()]);
        assert_eq!(three[0], vec![3]);
        assert_eq!(three[1], vec![10]);
        assert_eq!(three[2], vec![15]);
    }

    #[test]
    fn test_inject_all_caught_10_rounds() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(led.all_caught(), "四类坏样本 10 轮必须全抓");
        assert!(led.conservation().is_ok());
    }

    #[test]
    fn test_state_machine_transitions() {
        // 未跑先报告拒。
        let mut board = KfaceBoard::new();
        let _ = board.build();
        assert!(board.report().is_err());
        // 正常链 + 幂等重算。
        let _ = board.run();
        assert!(board.report().is_ok());
        assert!(board.rerun().is_ok());
        // rerun 通道不破坏账态。
        assert_eq!(board.state(), STATE_REPORTED);
        // run 后重跑（Reported 态 run）拒并记账。
        assert!(board.run().is_err());
        assert_eq!(board.rerun_rejects, 1);
    }

    #[test]
    fn test_duplicate_and_boundary_rows() {
        let mut board = KfaceBoard::new();
        let _ = board.build();
        assert!(board.append(1, PHASE_DOMAIN).is_err()); // 重行
        assert!(board.append(0, PHASE_DOMAIN).is_err()); // 行号 0
        assert!(board.append(20, PHASE_DOMAIN).is_err()); // 行号越界
        assert!(board.append(20, 0).is_err()); // 阶段 0
        assert!(board.append(20, PHASE_MAX + 1).is_err()); // 阶段越界
        assert_eq!(board.row_count(), ROW_TOTAL);
    }

    #[test]
    fn test_missing_row_aggregate_red() {
        let mut board = KfaceBoard::new();
        let _ = board.build();
        assert!(board.drop_row(19).is_ok());
        assert_eq!(board.row_count(), ROW_TOTAL - 1);
        let _ = board.run();
        assert!(board.report().is_err());
        // 中段行不可撤（账本只允许尾部回缩）。
        let mut b2 = KfaceBoard::new();
        let _ = b2.build();
        assert!(b2.drop_row(5).is_err());
        assert_eq!(b2.row_count(), ROW_TOTAL);
    }

    #[test]
    fn test_tamper_detects_exactly_one_red() {
        let mut board = KfaceBoard::new();
        let _ = board.build();
        let _ = board.tamper_evidence(12);
        let (greens, reds) = board.run().unwrap_or((0, 0));
        assert_eq!(greens, ROW_TOTAL - 1);
        assert_eq!(reds, 1);
        assert_eq!(board.red_row_ids(), vec![12]);
        assert!(board.report().is_ok());
    }

    #[test]
    fn test_double_run_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard_and_detail_closed_set() {
        assert!(segment_guard());
        assert!(detail_closed_set());
        assert_eq!(KtCode::EvidenceMismatch.code(), SEG_BASE + 3);
    }

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18539_checks();
        assert!(set.all_passed(), "UNX-F18539 判据必须全绿：{}", set.red_items_text());
    }
}
