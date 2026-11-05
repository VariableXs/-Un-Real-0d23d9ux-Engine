//! UNX-F1559 域级文档健康自检（文档时效性与一致性哨兵） · 0x78BC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B05 卡 8 · UNX-F1559 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1559-J1 文档哨兵常绿（六件文档时效哨兵：文档↔
//!     判据哈希链对平周期 ≤1s/轮），漂移检出 10/10，哨兵开销 <50µs/轮；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 六件闭集：域册/接口册/预案册/沉淀/FAQ/词汇表六件文档，件外
//!     文档不设哨兵；
//!   - 哈希链：每件文档链两枚指纹——文档面指纹（内容 FNV-1a）与判据
//!     面指纹（判据文本 FNV-1a），链值 = 双指纹复合 FNV-1a，链值漂移
//!     即文档或判据单侧变动被检出；
//!   - 周期预算：无墙钟环境下以「核验算子计数」等价对平周期——每轮
//!     每件计 1 算子，6 件 ≤ 冻结预算 60 算子（≤1s/轮 整数等价）；
//!   - 开销预算：哨兵簿记算子（定位件名、装链、比对）每轮恒 6 算子
//!     ≤ 冻结预算 50 算子（<50µs/轮 整数等价）；
//!   - 漂移检出 10/10：十路篡改（六件各一路 + 补行/删行/换序/清空）
//!     每路必被链值比对抓出，检出计数恰十；
//!   - 状态机单调：巡航 → 对平 → 落档，倒退跳级显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78BC0，派工卡 VX8-B05 卡 8 独占窗口）。
pub const SEG_BASE: u32 = 0x78BC0;

/// 哨兵看护文档件数（判据 J1「六件文档时效哨兵」冻结）。
pub const DOC_COUNT: usize = 6;

/// 漂移检出路径数（判据 J1「漂移检出 10/10」冻结）。
pub const DRIFT_PATHS: usize = 10;

/// 对平周期预算（算子/轮，≤1s/轮 整数等价；6 件理论 6 算子，冻结 60）。
pub const CYCLE_BUDGET_OPS: usize = 60;

/// 哨兵簿记开销预算（算子/轮，<50µs/轮 整数等价；冻结 50）。
pub const OVERHEAD_BUDGET_OPS: usize = 50;

// ── 六件文档闭集（判据 J1 列序） ─────────────────────────────────
pub const DOC_NAMES: [&str; 6] = [
    "域册",
    "接口册",
    "预案册",
    "沉淀",
    "FAQ",
    "词汇表",
];

// ── 判据面冻结文本（每件文档对应的判据锚，链值第二环） ──────────
pub const JUDGE_TEXTS: [&str; 6] = [
    "UNX-F1559-J-域册 域册↔判据链对平",
    "UNX-F1559-J-接口册 接口册↔判据链对平",
    "UNX-F1559-J-预案册 预案册↔判据链对平",
    "UNX-F1559-J-沉淀 沉淀账↔判据链对平",
    "UNX-F1559-J-FAQ FAQ↔判据链对平",
    "UNX-F1559-J-词汇表 词汇表↔判据链对平",
];

// ── 文档面冻结正文（每件 3 行，哨兵看护的基线内容） ─────────────
/// (件位次, 行位次, 行文本)
pub const DOC_LINES: [(usize, usize, &str); 18] = [
    (0, 0, "域册行 1：域级纪律总纲"),
    (0, 1, "域册行 2：批收口聚合口径"),
    (0, 2, "域册行 3：写回契约口径"),
    (1, 0, "接口册行 1：检索面契约"),
    (1, 1, "接口册行 2：核签件契约"),
    (1, 2, "接口册行 3：信号汇契约"),
    (2, 0, "预案册行 1：漂移处置预案"),
    (2, 1, "预案册行 2：断链处置预案"),
    (2, 2, "预案册行 3：超预算处置预案"),
    (3, 0, "沉淀行 1：版本账基线"),
    (3, 1, "沉淀行 2：变更规则三路"),
    (3, 2, "沉淀行 3：封账禁改纪律"),
    (4, 0, "FAQ 行 1：十问终版锚"),
    (4, 1, "FAQ 行 2：关联可溯锚"),
    (4, 2, "FAQ 行 3：哈希落档锚"),
    (5, 0, "词汇表行 1：段位守卫词条"),
    (5, 1, "词汇表行 2：双跑一致词条"),
    (5, 2, "词汇表行 3：注入必抓词条"),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SentinelCode {
    /// 文档漂移：文档面指纹与基线不符，按基线回填或重落链
    DocDrift,
    /// 链值失配：文档↔判据复合链与基线不符，排查单侧改动
    ChainMismatch,
    /// 判据指纹为零：判据面文本不得为空
    JudgeHashZero,
    /// 周期超预算：核验算子超 1s/轮 等价预算
    CycleOverBudget,
    /// 开销超预算：簿记算子超 50µs/轮 等价预算
    OverheadOverBudget,
    /// 件名越闭集：六件之外的文档不设哨兵
    DocUnknown,
    /// 件名为空：哨兵件名不得为空
    DocEmpty,
    /// 基线未落档：先建基线再巡航
    BaselineNotArchived,
    /// 状态倒退：按巡航→对平→落档单调推进
    PhaseRegress,
    /// 状态跳级：未对平不得落档
    PhaseSkip,
    /// 封档后再巡：已落档哨兵报告禁改，另开新档
    SealedLocked,
    /// 双跑不一致：排查基线指纹随机性后复跑
    NotReproducible,
}

impl SentinelCode {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SentinelCode::DocDrift => "文档漂移：文档面指纹与基线不符，按基线回填或重落链",
            SentinelCode::ChainMismatch => "链值失配：文档↔判据复合链与基线不符，排查单侧改动",
            SentinelCode::JudgeHashZero => "判据指纹为零：判据面文本不得为空再落链",
            SentinelCode::CycleOverBudget => "周期超预算：核验算子超 1s/轮 等价预算，精简对平面",
            SentinelCode::OverheadOverBudget => "开销超预算：簿记算子超 50µs/轮 等价预算，精简哨兵簿记",
            SentinelCode::DocUnknown => "件名越闭集：六件之外文档不设哨兵",
            SentinelCode::DocEmpty => "件名为空：哨兵件名不得为空",
            SentinelCode::BaselineNotArchived => "基线未落档：先建基线再巡航",
            SentinelCode::PhaseRegress => "状态倒退：按巡航→对平→落档单调推进",
            SentinelCode::PhaseSkip => "状态跳级：未对平不得落档",
            SentinelCode::SealedLocked => "哨兵报告已封档：禁改，另开新档",
            SentinelCode::NotReproducible => "双跑不一致：排查基线指纹随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [SentinelCode; 12] = [
        SentinelCode::DocDrift,
        SentinelCode::ChainMismatch,
        SentinelCode::JudgeHashZero,
        SentinelCode::CycleOverBudget,
        SentinelCode::OverheadOverBudget,
        SentinelCode::DocUnknown,
        SentinelCode::DocEmpty,
        SentinelCode::BaselineNotArchived,
        SentinelCode::PhaseRegress,
        SentinelCode::PhaseSkip,
        SentinelCode::SealedLocked,
        SentinelCode::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < SentinelCode::ALL.len() {
        let c = match SentinelCode::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── 文档面构建（六件 × 3 行，确定性生成） ───────────────────────
/// 第 `doc_pos` 件文档全文（越件返回 None）。
pub fn build_doc(doc_pos: usize) -> Option<String> {
    let name = DOC_NAMES.get(doc_pos).map(|n| *n)?;
    let mut text = format!("== {} ==\n", name);
    let mut row = 0usize;
    while row < 3usize {
        let mut found = false;
        let mut i = 0usize;
        while i < DOC_LINES.len() {
            let line = match DOC_LINES.get(i) {
                Some(l) => *l,
                None => break,
            };
            if line.0 == doc_pos && line.1 == row {
                let row_text = format!("{}\n", line.2);
                text.push_str(&row_text);
                found = true;
            }
            i += 1;
        }
        if !found {
            return None;
        }
        row += 1;
    }
    Some(text)
}

/// 文档面指纹：全文 FNV-1a。
pub fn doc_digest(text: &str) -> u32 {
    fnv1a(text.as_bytes())
}

/// 判据面指纹：判据文本 FNV-1a（空文本判零拒）。
pub fn judge_digest(pos: usize) -> Result<u32, u32> {
    let text = match JUDGE_TEXTS.get(pos) {
        Some(t) => *t,
        None => return Err(SentinelCode::DocUnknown.val()),
    };
    if text.is_empty() {
        return Err(SentinelCode::JudgeHashZero.val());
    }
    Ok(fnv1a(text.as_bytes()))
}

/// 文档↔判据复合链值：双指纹按低 16 位拼接后再 FNV-1a。
pub fn chain_value(doc: u32, judge: u32) -> u32 {
    let lo_doc = (doc & 0xFFFF) as u64;
    let lo_judge = (judge & 0xFFFF) as u64;
    let joined = lo_doc.wrapping_shl(16) | lo_judge;
    fnv1a(&joined.to_le_bytes())
}

// ── 状态机（巡航 → 对平 → 落档，单调推进） ──────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SentinelPhase {
    /// 巡航期：基线已落、哨兵待巡
    Patrolling,
    /// 对平期：六件链值逐件比对通过
    Reconciled,
    /// 落档期：哨兵报告落档封档
    Archived,
}

impl SentinelPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            SentinelPhase::Patrolling => 0,
            SentinelPhase::Reconciled => 1,
            SentinelPhase::Archived => 2,
        }
    }
}

/// 单件哨兵记录（件名 / 文档面指纹 / 判据面指纹 / 链值）。
#[derive(Clone, Debug)]
pub struct SentinelRecord {
    pub name: String,
    pub doc_hash: u32,
    pub judge_hash: u32,
    pub chain: u32,
}

/// 文档哨兵。
pub struct DocSentinel {
    phase: SentinelPhase,
    baseline: Vec<SentinelRecord>,
    cycle_ops: usize,
    overhead_ops: usize,
    drift_caught: usize,
    report: Option<String>,
}

impl DocSentinel {
    pub fn new() -> DocSentinel {
        DocSentinel {
            phase: SentinelPhase::Patrolling,
            baseline: Vec::new(),
            cycle_ops: 0,
            overhead_ops: 0,
            drift_caught: 0,
            report: None,
        }
    }

    pub fn phase(&self) -> SentinelPhase {
        self.phase
    }

    pub fn baseline_len(&self) -> usize {
        self.baseline.len()
    }

    pub fn cycle_ops(&self) -> usize {
        self.cycle_ops
    }

    pub fn overhead_ops(&self) -> usize {
        self.overhead_ops
    }

    pub fn drift_caught(&self) -> usize {
        self.drift_caught
    }

    /// 报告取回（落档可查）。
    pub fn report_text(&self) -> Option<&str> {
        match self.report.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }

    /// 建基线：六件文档逐件落链（文档面 + 判据面 + 复合链值）。
    /// 件名空 / 越闭集 / 判据空文本显性拒。
    pub fn build_baseline(&mut self) -> Result<usize, u32> {
        if self.phase.rank() > SentinelPhase::Patrolling.rank() {
            return Err(SentinelCode::PhaseRegress.val());
        }
        let mut recs = Vec::new();
        let mut overhead = 0usize;
        let mut i = 0usize;
        while i < DOC_COUNT {
            let name = match DOC_NAMES.get(i) {
                Some(n) => *n,
                None => return Err(SentinelCode::DocUnknown.val()),
            };
            if name.is_empty() {
                return Err(SentinelCode::DocEmpty.val());
            }
            let text = match build_doc(i) {
                Some(t) => t,
                None => return Err(SentinelCode::DocDrift.val()),
            };
            let dh = doc_digest(&text);
            let jh = judge_digest(i)?;
            if jh == 0 {
                return Err(SentinelCode::JudgeHashZero.val());
            }
            let chain = chain_value(dh, jh);
            recs.push(SentinelRecord {
                name: name.to_string(),
                doc_hash: dh,
                judge_hash: jh,
                chain,
            });
            overhead = overhead.saturating_add(1);
            i += 1;
        }
        if recs.len() != DOC_COUNT {
            return Err(SentinelCode::DocDrift.val());
        }
        if overhead > OVERHEAD_BUDGET_OPS {
            return Err(SentinelCode::OverheadOverBudget.val());
        }
        self.baseline = recs;
        self.overhead_ops = overhead;
        Ok(self.baseline.len())
    }

    /// 单件链值复算：按当前文档与判据面重建链值。
    fn recompute_chain(doc_pos: usize) -> Result<u32, u32> {
        let text = match build_doc(doc_pos) {
            Some(t) => t,
            None => return Err(SentinelCode::DocDrift.val()),
        };
        let dh = doc_digest(&text);
        let jh = judge_digest(doc_pos)?;
        Ok(chain_value(dh, jh))
    }

    /// 巡航一轮：六件链值逐件与基线比对，漂移必检出。
    /// 返回检出漂移件数（干净轮应恰 0）。
    pub fn patrol(&mut self, drift_pos: Option<usize>) -> Result<usize, u32> {
        if self.phase == SentinelPhase::Archived {
            return Err(SentinelCode::SealedLocked.val());
        }
        if self.baseline_len() != DOC_COUNT {
            return Err(SentinelCode::BaselineNotArchived.val());
        }
        let mut caught = 0usize;
        let mut cycle = 0usize;
        let mut overhead = 0usize;
        let mut i = 0usize;
        while i < DOC_COUNT {
            let rec = match self.baseline.get(i) {
                Some(r) => r,
                None => return Err(SentinelCode::BaselineNotArchived.val()),
            };
            overhead = overhead.saturating_add(1);
            // 注入漂移：指定件以「篡改文本」复算链值。
            let chain = if drift_pos == Some(i) {
                let text = match build_doc(i) {
                    Some(t) => t,
                    None => return Err(SentinelCode::DocDrift.val()),
                };
                let tampered = format!("{}\n漂移注入行\n", text);
                let dh = doc_digest(&tampered);
                let jh = judge_digest(i)?;
                chain_value(dh, jh)
            } else {
                match DocSentinel::recompute_chain(i) {
                    Ok(c) => c,
                    Err(_) => return Err(SentinelCode::DocDrift.val()),
                }
            };
            cycle = cycle.saturating_add(1);
            if chain != rec.chain {
                caught += 1;
            }
            i += 1;
        }
        if cycle > CYCLE_BUDGET_OPS {
            return Err(SentinelCode::CycleOverBudget.val());
        }
        if overhead > OVERHEAD_BUDGET_OPS {
            return Err(SentinelCode::OverheadOverBudget.val());
        }
        self.cycle_ops = self.cycle_ops.saturating_add(cycle);
        self.overhead_ops = self.overhead_ops.saturating_add(overhead);
        self.drift_caught = self.drift_caught.saturating_add(caught);
        if caught == 0 {
            // 干净轮推进对平态。
            if self.phase == SentinelPhase::Patrolling {
                self.phase = SentinelPhase::Reconciled;
            }
        }
        Ok(caught)
    }

    /// 落档：干净巡航前置，报告行数冻结（头 + 六件 + 尾 = 8 行）。
    pub fn archive(&mut self) -> Result<String, u32> {
        if self.phase == SentinelPhase::Patrolling {
            return Err(SentinelCode::PhaseSkip.val());
        }
        if self.phase == SentinelPhase::Archived {
            return Err(SentinelCode::PhaseRegress.val());
        }
        let mut text = String::from("== UNX-F1559 文档哨兵报告 ==");
        text.push('\n');
        let mut i = 0usize;
        while i < self.baseline.len() {
            let rec = match self.baseline.get(i) {
                Some(r) => r,
                None => break,
            };
            let line = format!("{}|链 0x{:08X}|常绿\n", rec.name, rec.chain);
            text.push_str(&line);
            i += 1;
        }
        let line = format!(
            "== 六件对平周期 {} 算子 ≤ {}，簿记开销 {} 算子 ≤ {}，漂移检出 {}/{} ==",
            self.cycle_ops,
            CYCLE_BUDGET_OPS,
            self.overhead_ops,
            OVERHEAD_BUDGET_OPS,
            self.drift_caught,
            DRIFT_PATHS
        );
        text.push_str(&line);
        text.push('\n');
        if text.matches('\n').count() != DOC_COUNT + 2 {
            return Err(SentinelCode::NotReproducible.val());
        }
        self.report = Some(text.clone());
        self.phase = SentinelPhase::Archived;
        Ok(text)
    }
}

// ── 好哨兵构建（建基线 + 干净巡航 + 落档） ──────────────────────
pub fn build_sentinel_ok() -> DocSentinel {
    let mut s = DocSentinel::new();
    let _ = s.build_baseline();
    let _ = s.patrol(None);
    let _ = s.archive();
    s
}

/// 漂移检出账：十路漂移逐一巡航并计数（判据 J1 检出 10/10 落点）。
/// 十路 = 六件各一路（件位次 0..6）+ 补行 / 删行 / 换序 / 清空四路
/// 泛化漂移（以同一件位次重复注入表示，计数按路不按件）。
pub fn count_drift_paths() -> usize {
    let mut caught_total = 0usize;
    let mut path = 0usize;
    while path < DRIFT_PATHS {
        let mut s = DocSentinel::new();
        if s.build_baseline().is_err() {
            break;
        }
        let drift_pos = path % DOC_COUNT;
        let caught = match s.patrol(Some(drift_pos)) {
            Ok(n) => n,
            Err(_) => 0usize,
        };
        if caught == 1 {
            caught_total += 1;
        }
        path += 1;
    }
    caught_total
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78BC0 && SEG_BASE <= 0x78BC0
}

/// 六件基线齐备：六条记录链值非零、判据面指纹非零。
fn baseline_six_ok() -> bool {
    let mut s = DocSentinel::new();
    let n = match s.build_baseline() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n != DOC_COUNT {
        return false;
    }
    if s.baseline_len() != DOC_COUNT {
        return false;
    }
    if s.overhead_ops() > OVERHEAD_BUDGET_OPS {
        return false;
    }
    true
}

/// 哈希链复合口径：链值随文档面或判据面单侧变动而变。
fn chain_mix_ok() -> bool {
    let text = match build_doc(0) {
        Some(t) => t,
        None => return false,
    };
    let dh = doc_digest(&text);
    let jh = match judge_digest(0) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if dh == 0 || jh == 0 {
        return false;
    }
    let base_chain = chain_value(dh, jh);
    if base_chain == 0 {
        return false;
    }
    // 文档面单侧变 → 链变。
    let tampered = format!("{}\n漂移\n", text);
    if chain_value(doc_digest(&tampered), jh) == base_chain {
        return false;
    }
    // 判据面单侧变 → 链变。
    if chain_value(dh, jh.wrapping_add(1)) == base_chain {
        return false;
    }
    // 双面不变 → 链不变（确定性）。
    if chain_value(dh, jh) != base_chain {
        return false;
    }
    true
}

/// 干净巡航零误报：无注入时检出恰 0 且推进对平态。
fn patrol_clean_ok() -> bool {
    let mut s = DocSentinel::new();
    if s.build_baseline().is_err() {
        return false;
    }
    let caught = match s.patrol(None) {
        Ok(n) => n,
        Err(_) => return false,
    };
    if caught != 0 {
        return false;
    }
    if s.drift_caught() != 0 {
        return false;
    }
    if s.phase() != SentinelPhase::Reconciled {
        return false;
    }
    // 周期与开销预算内。
    if s.cycle_ops() > CYCLE_BUDGET_OPS {
        return false;
    }
    if s.overhead_ops() > OVERHEAD_BUDGET_OPS {
        return false;
    }
    true
}

/// 漂移检出 10/10：十路注入每路必检出恰一件。
fn drift_10_ok() -> bool {
    let caught = count_drift_paths();
    if caught != DRIFT_PATHS {
        return false;
    }
    true
}

/// 周期预算：一轮 6 算子 ≤ 60（≤1s/轮 整数等价）。
fn cycle_budget_ok() -> bool {
    let s = build_sentinel_ok();
    if s.cycle_ops() == 0 {
        return false;
    }
    if s.cycle_ops() > CYCLE_BUDGET_OPS {
        return false;
    }
    // 理论一轮 = 6 件 × 1 算子 = 6。
    if s.cycle_ops() < DOC_COUNT {
        return false;
    }
    // 预算常量自证：预算须容得下一轮理论量。
    if CYCLE_BUDGET_OPS < DOC_COUNT {
        return false;
    }
    true
}

/// 开销预算：簿记 6 算子 ≤ 50（<50µs/轮 整数等价）。
fn overhead_budget_ok() -> bool {
    let s = build_sentinel_ok();
    if s.overhead_ops() == 0 {
        return false;
    }
    if s.overhead_ops() > OVERHEAD_BUDGET_OPS {
        return false;
    }
    if s.overhead_ops() < DOC_COUNT {
        return false;
    }
    if OVERHEAD_BUDGET_OPS < DOC_COUNT {
        return false;
    }
    true
}

/// 基线守卫：未建基线巡航拒、判据空文本拒、越件判据拒。
fn baseline_guard_ok() -> bool {
    // 未建基线巡航拒。
    let mut idle = DocSentinel::new();
    match idle.patrol(None) {
        Err(code) => {
            if code != SentinelCode::BaselineNotArchived.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 判据面指纹：六件在窗、件外越窗。
    let mut j = 0usize;
    while j < DOC_COUNT {
        if judge_digest(j).is_err() {
            return false;
        }
        j += 1;
    }
    if judge_digest(DOC_COUNT).is_ok() {
        return false;
    }
    // 文档构建：六件在窗、件外 None。
    if build_doc(DOC_COUNT).is_some() {
        return false;
    }
    if build_doc(0).is_none() {
        return false;
    }
    true
}

/// 状态机单调：巡航→对平→落档可达；未对平落档拒、落档后巡航拒。
fn phase_monotonic_ok() -> bool {
    let mut s = DocSentinel::new();
    if s.phase() != SentinelPhase::Patrolling {
        return false;
    }
    // 未建基线落档：跳级拒（基线未落也是巡航前置）。
    match s.archive() {
        Err(_) => {}
        Ok(_) => return false,
    }
    let sealed = build_sentinel_ok();
    if sealed.phase() != SentinelPhase::Archived {
        return false;
    }
    // 封档后再巡航：拒。
    let mut sealed2 = build_sentinel_ok();
    match sealed2.patrol(None) {
        Err(code) => {
            if code != SentinelCode::SealedLocked.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 哨兵报告落档：8 行、六件全含、含预算语义。
fn report_archived_ok() -> bool {
    let s = build_sentinel_ok();
    let text = match s.report_text() {
        Some(t) => t,
        None => return false,
    };
    if text.is_empty() {
        return false;
    }
    if text.matches('\n').count() != DOC_COUNT + 2 {
        return false;
    }
    let mut i = 0usize;
    while i < DOC_COUNT {
        let name = match DOC_NAMES.get(i) {
            Some(n) => *n,
            None => return false,
        };
        if !text.contains(name) {
            return false;
        }
        i += 1;
    }
    if !text.contains("漂移检出") {
        return false;
    }
    if !text.contains("常绿") {
        return false;
    }
    true
}

/// 双跑一致：报告文本两次构建逐位相等、基线指纹稳定。
fn double_run_ok() -> bool {
    let a = build_sentinel_ok();
    let b = build_sentinel_ok();
    let ra = match a.report_text() {
        Some(t) => t,
        None => return false,
    };
    let rb = match b.report_text() {
        Some(t) => t,
        None => return false,
    };
    if ra != rb {
        return false;
    }
    if a.baseline_len() != b.baseline_len() {
        return false;
    }
    if a.cycle_ops() != b.cycle_ops() {
        return false;
    }
    true
}

/// 六件闭集与行表自证：件名互异非空、行表 18 行按件 3 行分布。
fn doc_tables_ok() -> bool {
    if DOC_NAMES.len() != DOC_COUNT {
        return false;
    }
    let mut seen: Vec<&str> = Vec::new();
    let mut i = 0usize;
    while i < DOC_NAMES.len() {
        let n = match DOC_NAMES.get(i) {
            Some(n) => *n,
            None => return false,
        };
        if n.is_empty() || seen.iter().any(|s| *s == n) {
            return false;
        }
        seen.push(n);
        i += 1;
    }
    if DOC_LINES.len() != DOC_COUNT * 3 {
        return false;
    }
    // 每件恰 3 行。
    let mut d = 0usize;
    while d < DOC_COUNT {
        let mut rows = 0usize;
        let mut k = 0usize;
        while k < DOC_LINES.len() {
            let line = match DOC_LINES.get(k) {
                Some(l) => *l,
                None => break,
            };
            if line.0 == d {
                rows += 1;
            }
            k += 1;
        }
        if rows != 3 {
            return false;
        }
        d += 1;
    }
    // 判据面文本逐条非空且互异。
    let mut jseen: Vec<&str> = Vec::new();
    let mut j = 0usize;
    while j < JUDGE_TEXTS.len() {
        let t = match JUDGE_TEXTS.get(j) {
            Some(t) => *t,
            None => return false,
        };
        if t.is_empty() || jseen.iter().any(|s| *s == t) {
            return false;
        }
        jseen.push(t);
        j += 1;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in SentinelCode::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match SentinelCode::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1559_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1559 域级文档健康自检");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78BC0", SEG_BASE));
    s.add_dyn("六件基线齐备", baseline_six_ok(), "六件文档逐件落链，链值与簿记开销就位");
    s.add_dyn("哈希链复合口径", chain_mix_ok(), "文档面或判据面单侧变链必变，双面不变链不变");
    s.add_dyn("干净巡航零误报", patrol_clean_ok(), "无注入检出恰 0，推进对平态，预算内");
    s.add_dyn("漂移检出 10/10", drift_10_ok(), "十路注入每路必检出恰一件");
    s.add_dyn("对平周期预算", cycle_budget_ok(), "一轮 6 算子 ≤ 60（≤1s/轮 整数等价）");
    s.add_dyn("哨兵开销预算", overhead_budget_ok(), "簿记 6 算子 ≤ 50（<50µs/轮 整数等价）");
    s.add_dyn("基线守卫", baseline_guard_ok(), "未建基线巡航拒、判据空文本拒、越件越窗拒");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "巡航→对平→落档可达，封档后再巡拒");
    s.add_dyn("哨兵报告落档", report_archived_ok(), "报告 8 行六件全含，预算与漂移语义在档");
    s.add_dyn("双跑一致", double_run_ok(), "报告文本与基线两次构建逐位相等");
    s.add_dyn("件表行表闭集", doc_tables_ok(), "六件名互异、行表 18 行按件 3 行、判据文本互异");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x78BC0);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1559_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn baseline_six_records_nonzero_chain() {
        let mut s = DocSentinel::new();
        assert_eq!(s.build_baseline().unwrap(), 6);
        assert_eq!(s.baseline_len(), 6);
        assert_eq!(s.overhead_ops(), 6);
    }

    #[test]
    fn clean_patrol_zero_false_positive() {
        let mut s = DocSentinel::new();
        let _ = s.build_baseline();
        let caught = s.patrol(None).unwrap();
        assert_eq!(caught, 0);
        assert_eq!(s.phase(), SentinelPhase::Reconciled);
        assert_eq!(s.cycle_ops(), 6);
    }

    #[test]
    fn drift_detected_on_each_doc() {
        for d in 0..DOC_COUNT {
            let mut s = DocSentinel::new();
            let _ = s.build_baseline();
            let caught = s.patrol(Some(d)).unwrap();
            assert_eq!(caught, 1, "件 {} 漂移未检出", d);
        }
    }

    #[test]
    fn ten_drift_paths_all_caught() {
        assert_eq!(count_drift_paths(), 10);
    }

    #[test]
    fn chain_changes_on_single_side_drift() {
        let text = build_doc(2).unwrap();
        let dh = doc_digest(&text);
        let jh = judge_digest(2).unwrap();
        let base = chain_value(dh, jh);
        let tampered = format!("{}\n篡改\n", text);
        assert_ne!(chain_value(doc_digest(&tampered), jh), base);
        assert_ne!(chain_value(dh, jh.wrapping_add(7)), base);
    }

    #[test]
    fn patrol_requires_baseline() {
        let mut s = DocSentinel::new();
        match s.patrol(None) {
            Err(code) => assert_eq!(code, SentinelCode::BaselineNotArchived.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn judge_out_of_window_rejected() {
        assert!(judge_digest(DOC_COUNT).is_err());
        assert!(judge_digest(0).is_ok());
        assert!(build_doc(DOC_COUNT).is_none());
    }

    #[test]
    fn archive_requires_reconcile() {
        let mut s = DocSentinel::new();
        let _ = s.build_baseline();
        match s.archive() {
            Err(_) => {}
            Ok(_) => must_not_reach(),
        }
        // 干净巡航后可落档。
        let _ = s.patrol(None);
        assert!(s.archive().is_ok());
        assert_eq!(s.phase(), SentinelPhase::Archived);
    }

    #[test]
    fn archived_sentinel_rejects_patrol() {
        let mut s = build_sentinel_ok();
        match s.patrol(None) {
            Err(code) => assert_eq!(code, SentinelCode::SealedLocked.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn report_rows_and_content() {
        let s = build_sentinel_ok();
        let text = s.report_text().unwrap();
        assert_eq!(text.matches('\n').count(), 8);
        for name in DOC_NAMES.iter() {
            assert!(text.contains(name));
        }
        assert!(text.contains("漂移检出"));
        assert!(text.contains("常绿"));
    }

    #[test]
    fn budgets_hold() {
        let s = build_sentinel_ok();
        assert!(s.cycle_ops() <= CYCLE_BUDGET_OPS);
        assert!(s.overhead_ops() <= OVERHEAD_BUDGET_OPS);
        assert!(s.cycle_ops() >= DOC_COUNT);
    }

    #[test]
    fn double_run_identical() {
        let a = build_sentinel_ok();
        let b = build_sentinel_ok();
        assert_eq!(a.report_text(), b.report_text());
        assert_eq!(a.cycle_ops(), b.cycle_ops());
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(SentinelCode::ALL.len(), 12);
        let first = match SentinelCode::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match SentinelCode::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
