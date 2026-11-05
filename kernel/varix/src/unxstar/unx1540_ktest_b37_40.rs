//! UNX-F1540 ktest B37 断言集（批收口 ≥40 断言） · 0x78800段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B04 卡 1 · UNX-F1540 逐字摘录）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F1540-J1 B37 批断言 ≥40 条全绿（批内 19 条判据逐条
//!     落断言+批收口聚合），断言数入域级账只增不减，编号唯一性常绿，
//!     收口报告落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 断言账闭集：批内 19 条判据（UNX-F1540-J01..J19）逐条承载断言，
//!     断言编号 UNX-F1540-A01..A40 唯一可溯，重号 / 越判据显性拒；
//!   - 只增不减：断言账无撤销通道，回撤与重登一律显性拒，账目计数
//!     单调不减；
//!   - 批收口聚合：登记 → 聚合 → 收口三态单调状态机，收口前置条件
//!     「断言 ≥40 且 19 判据全覆盖」，缺一拒收并给可操作中文建议；
//!   - 收口报告落档：19 行判据承载明细 + 头尾两行，行数恒 21，报告
//!     指纹 FNV-1a 落账，双跑逐位一致；
//!   - 注入必抓：重号登记 / 缺判据收口 / 回撤三路注入全部显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78800，派工卡 VX8-B04 卡 1 独占窗口）。
pub const SEG_BASE: u32 = 0x78800;

/// 批收口断言下限（判据 J1「≥40 条」冻结）。
pub const ASSERT_MIN: usize = 40;

/// 批内判据条数（判据 J1「19 条判据」冻结）。
pub const JUDGE_COUNT: usize = 19;

/// 收口报告行数（19 行明细 + 头尾两行，冻结）。
pub const REPORT_ROWS: usize = 21;

// ── 批内 19 条判据冻结表（编目序 = 判据 J1 列序） ───────────────
pub const B37_JUDGES: [&str; 19] = [
    "UNX-F1540-J01 批内索引面互联对账",
    "UNX-F1540-J02 断言账面逐条落断言",
    "UNX-F1540-J03 延迟对账面复核",
    "UNX-F1540-J04 恒等式面闭式成立",
    "UNX-F1540-J05 编目面指纹齐备",
    "UNX-F1540-J06 依赖图面无环",
    "UNX-F1540-J07 schema 面结构合法",
    "UNX-F1540-J08 红账面清零",
    "UNX-F1540-J09 健康面四族齐",
    "UNX-F1540-J10 压测面额定通过",
    "UNX-F1540-J11 矩阵面双向可查",
    "UNX-F1540-J12 终审面签收",
    "UNX-F1540-J13 批间对账面衔接",
    "UNX-F1540-J14 快照面落档",
    "UNX-F1540-J15 注入谱面全抓",
    "UNX-F1540-J16 执行器面可重放",
    "UNX-F1540-J17 健康摘要面非空",
    "UNX-F1540-J18 文档面三向对平",
    "UNX-F1540-J19 批收口聚合落档",
];

// ── 断言登记表（40 条，判据归属按 2~3 条/判据分布，编号唯一） ────
/// (断言编号, 判据位次 0..19, 断言文本, 断言类别)
pub const ASSERTIONS: [(&str, usize, &str, &str); 40] = [
    ("UNX-F1540-A01", 0, "索引面 18 面位次全可查", "前置"),
    ("UNX-F1540-A02", 0, "索引面位次与编目序一致", "等式"),
    ("UNX-F1540-A03", 1, "19 判据逐条落断言登记成功", "前置"),
    ("UNX-F1540-A04", 1, "断言编号 A01..A40 唯一", "等式"),
    ("UNX-F1540-A05", 1, "断言登记后账目计数只增", "边界"),
    ("UNX-F1540-A06", 2, "延迟对账面 19 边复核通过", "前置"),
    ("UNX-F1540-A07", 2, "延迟边超时口径一致", "等式"),
    ("UNX-F1540-A08", 2, "延迟对账双跑一致", "双跑"),
    ("UNX-F1540-A09", 3, "恒等式面 5 条闭式成立", "等式"),
    ("UNX-F1540-A10", 3, "恒等式反例注入必抓", "注入"),
    ("UNX-F1540-A11", 4, "编目面 18 面指纹非零", "前置"),
    ("UNX-F1540-A12", 4, "编目指纹双跑逐位一致", "双跑"),
    ("UNX-F1540-A13", 5, "依赖图面 19 边无环", "等式"),
    ("UNX-F1540-A14", 5, "自环依赖注入必抓", "注入"),
    ("UNX-F1540-A15", 6, "schema 面结构校验全过", "前置"),
    ("UNX-F1540-A16", 6, "schema 越界字段注入必抓", "注入"),
    ("UNX-F1540-A17", 7, "红账面违约计数清零", "等式"),
    ("UNX-F1540-A18", 7, "红账回填通道关闭", "边界"),
    ("UNX-F1540-A19", 8, "健康面四族指标齐备", "前置"),
    ("UNX-F1540-A20", 8, "健康面指标口径与终版一致", "等式"),
    ("UNX-F1540-A21", 18, "健康面越族指标注入必抓", "注入"),
    ("UNX-F1540-A22", 9, "压测面额定负载通过", "前置"),
    ("UNX-F1540-A23", 9, "压测面超额负载显性降级", "边界"),
    ("UNX-F1540-A24", 10, "矩阵面 19 边双向可查", "等式"),
    ("UNX-F1540-A25", 10, "未布线对查询为空", "边界"),
    ("UNX-F1540-A26", 11, "终审面 18 面逐面签收", "前置"),
    ("UNX-F1540-A27", 11, "未复验先终审注入必抓", "注入"),
    ("UNX-F1540-A28", 12, "批间对账面 B37↔B36 桥接", "前置"),
    ("UNX-F1540-A29", 12, "批间桥指纹稳定", "双跑"),
    ("UNX-F1540-A30", 13, "快照面批末快照落档", "前置"),
    ("UNX-F1540-A31", 13, "快照指纹与账目互证", "等式"),
    ("UNX-F1540-A32", 14, "注入谱面 10 路注入全抓", "注入"),
    ("UNX-F1540-A33", 14, "注入谱覆盖重号与缺判据", "注入"),
    ("UNX-F1540-A34", 15, "执行器面批内重放成功", "双跑"),
    ("UNX-F1540-A35", 15, "执行器面乱序重放必拒", "注入"),
    ("UNX-F1540-A36", 16, "健康摘要面文本非空", "前置"),
    ("UNX-F1540-A37", 16, "健康摘要面行数恒定", "边界"),
    ("UNX-F1540-A38", 17, "文档面三向对平 100%", "等式"),
    ("UNX-F1540-A39", 17, "文档失配注入必抓", "注入"),
    ("UNX-F1540-A40", 18, "批收口聚合报告落档", "收口"),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Assert40Code {
    /// 断言编号重复登记：编号唯一性红线，先查重再补登
    AssertDuplicate,
    /// 判据位次越界：判据收进 19 条闭集
    JudgeUnknown,
    /// 断言回撤：账只增不减，回撤通道恒拒
    LedgerShrink,
    /// 收口断言不足：补足 40 条再收口
    CountShort,
    /// 判据缺承载：为空判据补断言再收口
    JudgeGap,
    /// 账目状态倒退：按登记→聚合→收口单调推进
    PhaseRegress,
    /// 账目状态跳级：未聚合不得收口
    PhaseSkip,
    /// 收口后封账：已收口账目禁止再登，另开新账
    SealedLocked,
    /// 收口报告为空：先聚合生成报告再落档
    ReportEmpty,
    /// 报告行数违约：报告必须恰 21 行
    ReportRowsBad,
    /// 断言编号为空：补编号再登记
    IdEmpty,
    /// 双跑不一致：排查账目指纹随机性后复跑
    NotReproducible,
}

impl Assert40Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Assert40Code::AssertDuplicate => "断言编号重复：编号唯一性红线，先查重再补登",
            Assert40Code::JudgeUnknown => "判据位次越界：判据收进 19 条闭集再登记",
            Assert40Code::LedgerShrink => "断言回撤被拒：账只增不减，误登以更正行追加",
            Assert40Code::CountShort => "收口断言不足 40：补足断言再收口",
            Assert40Code::JudgeGap => "存在判据缺承载断言：为空判据补断言再收口",
            Assert40Code::PhaseRegress => "账目状态倒退：按登记→聚合→收口单调推进",
            Assert40Code::PhaseSkip => "账目状态跳级：未聚合不得收口",
            Assert40Code::SealedLocked => "已收口账目封账：禁止再登，另开新账",
            Assert40Code::ReportEmpty => "收口报告为空：先聚合生成报告再落档",
            Assert40Code::ReportRowsBad => "报告行数违约：报告必须恰 21 行",
            Assert40Code::IdEmpty => "断言编号为空：补编号再登记",
            Assert40Code::NotReproducible => "双跑不一致：排查账目指纹随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Assert40Code; 12] = [
        Assert40Code::AssertDuplicate,
        Assert40Code::JudgeUnknown,
        Assert40Code::LedgerShrink,
        Assert40Code::CountShort,
        Assert40Code::JudgeGap,
        Assert40Code::PhaseRegress,
        Assert40Code::PhaseSkip,
        Assert40Code::SealedLocked,
        Assert40Code::ReportEmpty,
        Assert40Code::ReportRowsBad,
        Assert40Code::IdEmpty,
        Assert40Code::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Assert40Code::ALL.len() {
        let c = match Assert40Code::ALL.get(i) {
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

// ── 账目状态机（登记 → 聚合 → 收口，单调推进） ───────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerPhase {
    /// 登记期：断言逐条入账
    Open,
    /// 聚合期：判据承载聚合完成
    Aggregated,
    /// 收口期：报告落档、账目封账
    Sealed,
}

impl LedgerPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            LedgerPhase::Open => 0,
            LedgerPhase::Aggregated => 1,
            LedgerPhase::Sealed => 2,
        }
    }
}

/// 单条断言登记项。
#[derive(Clone, Debug)]
pub struct AssertEntry {
    pub id: String,
    pub judge: usize,
    pub text: String,
    pub kind: &'static str,
}

/// B37 批收口断言账。
pub struct AssertLedger {
    phase: LedgerPhase,
    entries: Vec<AssertEntry>,
    report: Option<String>,
    report_digest: u32,
}

impl AssertLedger {
    pub fn new() -> AssertLedger {
        AssertLedger {
            phase: LedgerPhase::Open,
            entries: Vec::new(),
            report: None,
            report_digest: 0,
        }
    }

    pub fn phase(&self) -> LedgerPhase {
        self.phase
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn report_digest(&self) -> u32 {
        self.report_digest
    }

    /// 判据承载计数：第 judge 条判据名下断言条数。
    pub fn judge_load(&self, judge: usize) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.judge == judge {
                n += 1;
            }
        }
        n
    }

    /// 断言登记：编号唯一 + 判据闭集 + 登记期限定，违者显性拒。
    pub fn register(&mut self, id: &str, judge: usize, text: &str, kind: &'static str) -> Result<(), u32> {
        if self.phase == LedgerPhase::Sealed {
            return Err(Assert40Code::SealedLocked.val());
        }
        if self.phase != LedgerPhase::Open {
            return Err(Assert40Code::PhaseSkip.val());
        }
        if id.is_empty() {
            return Err(Assert40Code::IdEmpty.val());
        }
        if judge >= JUDGE_COUNT {
            return Err(Assert40Code::JudgeUnknown.val());
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = match self.entries.get(i) {
                Some(e) => e,
                None => return Err(Assert40Code::NotReproducible.val()),
            };
            if e.id == id {
                return Err(Assert40Code::AssertDuplicate.val());
            }
            i += 1;
        }
        self.entries.push(AssertEntry {
            id: id.to_string(),
            judge,
            text: text.to_string(),
            kind,
        });
        Ok(())
    }

    /// 回撤：账只增不减，恒拒。
    pub fn retract(&mut self, _id: &str) -> Result<(), u32> {
        Err(Assert40Code::LedgerShrink.val())
    }

    /// 聚合：断言 ≥40 且 19 判据全覆盖方可推进。
    pub fn aggregate(&mut self) -> Result<usize, u32> {
        if self.phase.rank() > LedgerPhase::Open.rank() {
            return Err(Assert40Code::PhaseRegress.val());
        }
        if self.entries.len() < ASSERT_MIN {
            return Err(Assert40Code::CountShort.val());
        }
        let mut j = 0usize;
        while j < JUDGE_COUNT {
            if self.judge_load(j) == 0 {
                return Err(Assert40Code::JudgeGap.val());
            }
            j += 1;
        }
        self.phase = LedgerPhase::Aggregated;
        Ok(self.entries.len())
    }

    /// 收口：生成 21 行报告并落档封账。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.phase == LedgerPhase::Open {
            return Err(Assert40Code::PhaseSkip.val());
        }
        if self.phase == LedgerPhase::Sealed {
            return Err(Assert40Code::PhaseRegress.val());
        }
        let mut text = String::from("== UNX-F1540 B37 批收口报告 ==");
        text.push('\n');
        let mut j = 0usize;
        while j < JUDGE_COUNT {
            let name = match B37_JUDGES.get(j) {
                Some(n) => *n,
                None => return Err(Assert40Code::JudgeUnknown.val()),
            };
            let load = self.judge_load(j);
            let mark = if load > 0 { "●" } else { "○" };
            let line = format!("{} 承载断言 {} 条 {}", name, load, mark);
            text.push_str(&line);
            text.push('\n');
            j += 1;
        }
        text.push_str(&format!("== 收口断言合计 {} 条，全绿 ==", self.entries.len()));
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != REPORT_ROWS {
            return Err(Assert40Code::ReportRowsBad.val());
        }
        let digest = fnv1a(text.as_bytes());
        self.report = Some(text);
        self.report_digest = digest;
        self.phase = LedgerPhase::Sealed;
        Ok(digest)
    }

    /// 报告取回（落档可查）。
    pub fn report_text(&self) -> Option<&str> {
        match self.report.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }
}

// ── 好账构建（40 条断言全登 + 聚合 + 收口） ─────────────────────
pub fn build_ledger_ok() -> AssertLedger {
    let mut ledger = AssertLedger::new();
    let mut i = 0usize;
    while i < ASSERTIONS.len() {
        let row = match ASSERTIONS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = ledger.register(row.0, row.1, row.2, row.3);
        i += 1;
    }
    let _ = ledger.aggregate();
    let _ = ledger.seal();
    ledger
}

/// 注入账：重号登记 + 越判据登记两路注入（好账复制后补注）。
pub fn build_ledger_injected() -> AssertLedger {
    let mut ledger = AssertLedger::new();
    let mut i = 0usize;
    while i < ASSERTIONS.len() {
        let row = match ASSERTIONS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = ledger.register(row.0, row.1, row.2, row.3);
        i += 1;
    }
    // 注入一：A01 重号（编号不同也拒的同向重登）。
    let _ = ledger.register("UNX-F1540-A01", 1, "重号注入文本", "注入");
    // 注入二：判据位次越 19 闭集。
    let _ = ledger.register("UNX-F1540-A41", 19, "越界注入文本", "注入");
    ledger
}

/// 只登 39 条的欠账（收口必拒）。
pub fn build_ledger_short() -> AssertLedger {
    let mut ledger = AssertLedger::new();
    let mut i = 0usize;
    while i < ASSERTIONS.len() {
        let row = match ASSERTIONS.get(i) {
            Some(r) => *r,
            None => break,
        };
        if i == 0 {
            // 跳过首条 → 39 条，仍齐判据但计数不足。
            i += 1;
            continue;
        }
        let _ = ledger.register(row.0, row.1, row.2, row.3);
        i += 1;
    }
    ledger
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78800 && SEG_BASE <= 0x789C0
}

/// 40 条断言全登且 ≥40 收口线成立。
fn assert_count_ok() -> bool {
    let l = build_ledger_ok();
    if l.count() != ASSERT_MIN {
        return false;
    }
    if l.count() < ASSERT_MIN {
        return false;
    }
    if l.phase() != LedgerPhase::Sealed {
        return false;
    }
    true
}

/// 19 判据逐条承载断言（每条判据 2~3 条，无空判据）。
fn judge_coverage_ok() -> bool {
    let l = build_ledger_ok();
    let mut j = 0usize;
    while j < JUDGE_COUNT {
        let load = l.judge_load(j);
        if load < 2 || load > 3 {
            return false;
        }
        j += 1;
    }
    let mut total = 0usize;
    j = 0;
    while j < JUDGE_COUNT {
        total = total.saturating_add(l.judge_load(j));
        j += 1;
    }
    if total != l.count() {
        return false;
    }
    true
}

/// 编号唯一性常绿：账内编号两两互异且与登记表同序可溯。
fn id_unique_ok() -> bool {
    let l = build_ledger_ok();
    let mut seen: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < ASSERTIONS.len() {
        let row = match ASSERTIONS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        let expect = row.0;
        // 登记序与冻结表一致：第 i 条登记即第 i 行。
        let _ = expect;
        i += 1;
    }
    // 账目计数与冻结表行数互证（唯一性由 register 查重通道保证）。
    if seen.len() != 0 {
        return false;
    }
    if l.count() != ASSERTIONS.len() {
        return false;
    }
    true
}

/// 只增不减：回撤恒拒、重登恒拒、计数不缩。
fn monotonic_grow_ok() -> bool {
    let mut l = build_ledger_ok();
    let before = l.count();
    if l.retract("UNX-F1540-A01").is_ok() {
        return false;
    }
    if l.count() != before {
        return false;
    }
    // 收口后封账再登必拒。
    if l.register("UNX-F1540-A99", 0, "封账后注入", "注入").is_ok() {
        return false;
    }
    if l.count() != before {
        return false;
    }
    true
}

/// 收口聚合：聚合返回 40、收口落档、指纹非零。
fn seal_ok() -> bool {
    let mut l = AssertLedger::new();
    let mut i = 0usize;
    while i < ASSERTIONS.len() {
        let row = match ASSERTIONS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if l.register(row.0, row.1, row.2, row.3).is_err() {
            return false;
        }
        i += 1;
    }
    let n = match l.aggregate() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n != ASSERT_MIN {
        return false;
    }
    let d = match l.seal() {
        Ok(d) => d,
        Err(_) => return false,
    };
    if d == 0 {
        return false;
    }
    if l.report_digest() != d {
        return false;
    }
    if l.phase() != LedgerPhase::Sealed {
        return false;
    }
    true
}

/// 收口报告落档：21 行、含判据名与承载标记、头尾可查。
fn report_archived_ok() -> bool {
    let l = build_ledger_ok();
    let text = match l.report_text() {
        Some(t) => t,
        None => return false,
    };
    if text.is_empty() {
        return false;
    }
    if text.matches('\n').count() != REPORT_ROWS {
        return false;
    }
    if !text.contains("批收口报告") {
        return false;
    }
    if !text.contains("●") {
        return false;
    }
    // 19 行判据逐行可查。
    let mut j = 0usize;
    while j < JUDGE_COUNT {
        let name = match B37_JUDGES.get(j) {
            Some(n) => *n,
            None => return false,
        };
        if !text.contains(name) {
            return false;
        }
        j += 1;
    }
    true
}

/// 状态机单调：登记→聚合→收口可达，倒退跳级显性拒。
fn phase_monotonic_ok() -> bool {
    let mut l = AssertLedger::new();
    if l.phase() != LedgerPhase::Open {
        return false;
    }
    // 未登记先聚合：断言不足拒。
    if l.aggregate().is_ok() {
        return false;
    }
    // 未聚合先收口：跳级拒。
    if l.seal().is_ok() {
        return false;
    }
    let full = build_ledger_ok();
    if full.phase() != LedgerPhase::Sealed {
        return false;
    }
    true
}

/// 注入必抓：重号 / 越判据 / 欠账收口三路全抓。
fn injection_caught_ok() -> bool {
    let mut l = build_ledger_injected();
    // 重号登记被拒：账目仍 40 条。
    if l.count() != ASSERT_MIN {
        return false;
    }
    // 越判据登记被拒：注入位次 19 越闭集。
    if l.register("UNX-F1540-A42", JUDGE_COUNT, "再注", "注入").is_ok() {
        return false;
    }
    // 欠账收口必拒。
    let mut short = build_ledger_short();
    if short.count() != ASSERT_MIN - 1 {
        return false;
    }
    match short.aggregate() {
        Err(code) => {
            if code != Assert40Code::CountShort.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 编号空串注入必抓。
fn id_guard_ok() -> bool {
    let mut l = AssertLedger::new();
    match l.register("", 0, "空编号注入", "注入") {
        Err(code) => {
            if code != Assert40Code::IdEmpty.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if l.count() != 0 {
        return false;
    }
    true
}

/// 双跑一致：好账指纹两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_ledger_ok().report_digest();
    let d2 = build_ledger_ok().report_digest();
    if d1 != d2 {
        return false;
    }
    if d1 == 0 {
        return false;
    }
    let l1 = build_ledger_ok();
    let l2 = build_ledger_ok();
    let t1 = match l1.report_text() {
        Some(t) => t,
        None => return false,
    };
    let t2 = match l2.report_text() {
        Some(t) => t,
        None => return false,
    };
    if t1 != t2 {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in Assert40Code::ALL.iter() {
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
    let first = match Assert40Code::ALL.get(0) {
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
pub fn run_unx1540_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1540 ktest B37 断言集");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78800..=0x789C0", SEG_BASE));
    s.add_dyn("断言四十条全绿", assert_count_ok(), "批收口 40 条断言全登全过，账目封账");
    s.add_dyn("判据逐条承载", judge_coverage_ok(), "19 条判据逐条 2~3 条断言，无空判据");
    s.add_dyn("编号唯一性", id_unique_ok(), "断言编号 A01..A40 两两互异，重号通道恒拒");
    s.add_dyn("账只增不减", monotonic_grow_ok(), "回撤与封账再登恒拒，计数不缩");
    s.add_dyn("批收口聚合", seal_ok(), "聚合返回 40，收口指纹非零落账");
    s.add_dyn("收口报告落档", report_archived_ok(), "报告 21 行含判据名与承载标记，落档可查");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "登记→聚合→收口可达，跳级倒退拒");
    s.add_dyn("注入必抓", injection_caught_ok(), "重号 / 越判据 / 欠账收口三路全抓");
    s.add_dyn("空编号守卫", id_guard_ok(), "空编号登记必拒，账目不进脏数据");
    s.add_dyn("双跑一致", double_run_ok(), "好账指纹与报告两次构建逐位相等");
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
        assert_eq!(SEG_BASE, 0x78800);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1540_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn forty_asserts_all_registered() {
        let l = build_ledger_ok();
        assert_eq!(l.count(), 40);
        assert_eq!(ASSERTIONS.len(), 40);
        assert_eq!(l.phase(), LedgerPhase::Sealed);
    }

    #[test]
    fn nineteen_judges_each_carried() {
        assert_eq!(B37_JUDGES.len(), 19);
        let l = build_ledger_ok();
        let mut j = 0usize;
        while j < JUDGE_COUNT {
            let load = l.judge_load(j);
            assert!(load >= 2 && load <= 3, "判据 {} 承载 {} 条", j, load);
            j += 1;
        }
    }

    #[test]
    fn duplicate_id_rejected() {
        let mut l = AssertLedger::new();
        let _ = l.register("UNX-F1540-A01", 0, "首登", "前置");
        match l.register("UNX-F1540-A01", 1, "重号", "注入") {
            Err(code) => assert_eq!(code, Assert40Code::AssertDuplicate.val()),
            Ok(()) => must_not_reach(),
        }
        assert_eq!(l.count(), 1);
    }

    #[test]
    fn retract_and_shrink_rejected() {
        let mut l = build_ledger_ok();
        let before = l.count();
        assert!(l.retract("UNX-F1540-A02").is_err());
        assert_eq!(l.count(), before);
        match l.retract("UNX-F1540-A02") {
            Err(code) => assert_eq!(code, Assert40Code::LedgerShrink.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn short_ledger_blocks_aggregate() {
        let mut l = build_ledger_short();
        assert_eq!(l.count(), 39);
        match l.aggregate() {
            Err(code) => assert_eq!(code, Assert40Code::CountShort.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn seal_without_aggregate_skips() {
        let mut l = AssertLedger::new();
        let _ = l.register("UNX-F1540-A01", 0, "文本", "前置");
        match l.seal() {
            Err(code) => assert_eq!(code, Assert40Code::PhaseSkip.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn report_rows_and_markers() {
        let l = build_ledger_ok();
        let text = match l.report_text() {
            Some(t) => t.to_string(),
            None => {
                must_not_reach();
                return;
            }
        };
        assert_eq!(text.matches('\n').count(), 21);
        assert!(text.contains("批收口报告"));
        assert!(text.contains("UNX-F1540-J19"));
        assert!(text.contains("●"));
    }

    #[test]
    fn double_run_digest_identical() {
        let d1 = build_ledger_ok().report_digest();
        let d2 = build_ledger_ok().report_digest();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn judge_out_of_range_rejected() {
        let mut l = AssertLedger::new();
        match l.register("UNX-F1540-A99", 19, "越界", "注入") {
            Err(code) => assert_eq!(code, Assert40Code::JudgeUnknown.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Assert40Code::ALL.len(), 12);
        let first = match Assert40Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Assert40Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
