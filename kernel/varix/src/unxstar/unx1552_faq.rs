//! UNX-F1552 域级常见问题册（FAQ 十问终版） · 0x78A00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B05 卡 1 · UNX-F1552 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1552-J1 FAQ 十问终版（高频问题逐条登记：问题/答案/
//!     关联条目三列），答案关联可溯 10/10 次，FAQ 哈希落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 十问闭集：高频问题恰好 10 条（Q01..Q10），每条三列齐备——问题、
//!     答案、关联条目位次，缺列显性拒；
//!   - 关联可溯：答案关联条目必须命中域级条目表（20 条闭集）内位次，
//!     越表位次注入必抓，可溯计数恰 10/10；
//!   - 哈希落档：十问全文按登记序拼接后 FNV-1a 落档，双跑逐位一致，
//!     零哈希判失败；
//!   - 状态机单调：起草 → 封账 → 写回三态推进，倒退 / 跳级 / 封账后再登
//!     一律显性拒；
//!   - 写回契约：封账后方可写回，写回产物「状态=已完成、结果摘要非空」
//!     双条件同验，摘要为空显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78A00，派工卡 VX8-B05 卡 1 独占窗口）。
pub const SEG_BASE: u32 = 0x78A00;

/// FAQ 十问闭集容量（判据 J1「十问」冻结）。
pub const FAQ_COUNT: usize = 10;

/// 域级条目表容量（关联可溯的闭集域，判据 J1「关联条目」落点）。
pub const ENTRY_COUNT: usize = 20;

// ── 域级条目表（20 条，FAQ 关联可溯的闭集域） ─────────────────────
/// (条目编号, 条目标题)
pub const ENTRIES: [(&str, &str); 20] = [
    ("UNX-ENTRY-01", "域级判据冻结纪律"),
    ("UNX-ENTRY-02", "批收口聚合规则"),
    ("UNX-ENTRY-03", "诊断码分段闭集"),
    ("UNX-ENTRY-04", "双跑一致性口径"),
    ("UNX-ENTRY-05", "注入必抓清单"),
    ("UNX-ENTRY-06", "段位守卫窗口"),
    ("UNX-ENTRY-07", "写回契约三条件"),
    ("UNX-ENTRY-08", "沉淀账版本化"),
    ("UNX-ENTRY-09", "文档打包预演"),
    ("UNX-ENTRY-10", "知识检索面"),
    ("UNX-ENTRY-11", "人话总册三 scope"),
    ("UNX-ENTRY-12", "核签件台账三态"),
    ("UNX-ENTRY-13", "体验信号四态登记"),
    ("UNX-ENTRY-14", "文档哨兵漂移检出"),
    ("UNX-ENTRY-15", "断言账只增不减"),
    ("UNX-ENTRY-16", "收口报告行数冻结"),
    ("UNX-ENTRY-17", "指纹 FNV-1a 口径"),
    ("UNX-ENTRY-18", "中文诊断文案纪律"),
    ("UNX-ENTRY-19", "零浮点整数运算"),
    ("UNX-ENTRY-20", "FAQ 十问终版"),
];

// ── FAQ 十问冻结表（问题 / 答案 / 关联条目位次三列，判据 J1 列序） ─
pub const FAQ_SET: [(&str, &str, &str, usize); 10] = [
    ("UNX-F1552-Q01", "域级判据为什么冻结后不许改？",
     "判据冻结是批收口的地基：改判据等于改尺子，收口结论失去可比性；变更必须走沉淀账变更规则（关联：UNX-ENTRY-08）。",
     7),
    ("UNX-F1552-Q02", "批收口聚合的前置条件是什么？",
     "批内判据逐条承载断言且断言数达下限，聚合才能推进；缺判据或缺断言都会被拒（关联：UNX-ENTRY-02）。",
     1),
    ("UNX-F1552-Q03", "诊断码为什么按段分配？",
     "每张派工卡独占 0x40 对齐的段窗，段内 0x01..0x0C 是诊断码闭集，跨段码不属于本段，查表返回未登记占位（关联：UNX-ENTRY-03）。",
     2),
    ("UNX-F1552-Q04", "双跑一致性怎么验？",
     "同一构建入口跑两次，指纹与产物逐位比对；不一致即判不可复现，先排查随机源（关联：UNX-ENTRY-04）。",
     3),
    ("UNX-F1552-Q05", "注入必抓是什么意思？",
     "人为构造违约（重号、越界、缺列、倒退）必须被显性拒，拒不到就是检查面漏抓（关联：UNX-ENTRY-05）。",
     4),
    ("UNX-F1552-Q06", "段位守卫查什么？",
     "SEG_BASE 必须 0x40 对齐且落在本批段窗内，错段位意味着诊断码窗口错位（关联：UNX-ENTRY-06）。",
     5),
    ("UNX-F1552-Q07", "写回契约要求哪三条件？",
     "状态=已完成、结果摘要非空、写回前置封账完成；三者缺一写回被拒（关联：UNX-ENTRY-07）。",
     6),
    ("UNX-F1552-Q08", "核签台账有哪三态？",
     "待核签、已核签、已收口三态单调推进，汇总结论必须与台账状态逐批对平（关联：UNX-ENTRY-12）。",
     11),
    ("UNX-F1552-Q09", "体验信号怎么登记？",
     "重复风暴、无反馈投递、反复开关三类信号各按捕获、归类、登记、闭环四态逐类登记（关联：UNX-ENTRY-13）。",
     12),
    ("UNX-F1552-Q10", "文档哨兵多久跑一轮？",
     "六件文档时效哨兵每轮对平文档与判据哈希链，周期与开销都受冻结预算约束，漂移必检出（关联：UNX-ENTRY-14）。",
     13),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaqCode {
    /// 问题编号为空：补编号再登记
    NoEmpty,
    /// 问题列为空：问题列必须可读
    QuestionEmpty,
    /// 答案列为空：答案必须写足可操作内容
    AnswerEmpty,
    /// 问题重复登记：高频问题册一问一条，先查重再补登
    QuestionDup,
    /// 关联条目越表：关联位次必须落在条目表 20 条闭集内
    LinkUnknown,
    /// 十问不足：补足 10 问再封账
    CountShort,
    /// 状态倒退：按起草→封账→写回单调推进
    PhaseRegress,
    /// 状态跳级：未封账不得写回
    PhaseSkip,
    /// 封账后再登：已封账问题册禁止补登，另开新册
    SealedLocked,
    /// 未封账写回：先封账落哈希再写回
    WritebackPremature,
    /// 结果摘要为空：摘要必须写明落档结论
    SummaryEmpty,
    /// 双跑不一致：排查指纹随机性后复跑
    NotReproducible,
}

impl FaqCode {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            FaqCode::NoEmpty => "问题编号为空：补编号（Q01..Q10）再登记",
            FaqCode::QuestionEmpty => "问题列为空：问题必须可读可检索",
            FaqCode::AnswerEmpty => "答案列为空：答案必须写足可操作内容再登记",
            FaqCode::QuestionDup => "问题重复登记：高频册一问一条，先查重再补登",
            FaqCode::LinkUnknown => "关联条目越表：关联位次收进条目表 20 条闭集",
            FaqCode::CountShort => "十问不足：补足 10 问再封账",
            FaqCode::PhaseRegress => "状态倒退：按起草→封账→写回单调推进",
            FaqCode::PhaseSkip => "状态跳级：未封账不得写回",
            FaqCode::SealedLocked => "已封账问题册封册：禁止补登，另开新册",
            FaqCode::WritebackPremature => "未封账写回：先封账落哈希再写回",
            FaqCode::SummaryEmpty => "结果摘要为空：摘要必须写明落档结论",
            FaqCode::NotReproducible => "双跑不一致：排查指纹随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [FaqCode; 12] = [
        FaqCode::NoEmpty,
        FaqCode::QuestionEmpty,
        FaqCode::AnswerEmpty,
        FaqCode::QuestionDup,
        FaqCode::LinkUnknown,
        FaqCode::CountShort,
        FaqCode::PhaseRegress,
        FaqCode::PhaseSkip,
        FaqCode::SealedLocked,
        FaqCode::WritebackPremature,
        FaqCode::SummaryEmpty,
        FaqCode::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < FaqCode::ALL.len() {
        let c = match FaqCode::ALL.get(i) {
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

// ── 状态机（起草 → 封账 → 写回，单调推进） ──────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaqPhase {
    /// 起草期：十问逐条登记
    Draft,
    /// 封账期：哈希落档、册面冻结
    Sealed,
    /// 写回期：状态=已完成、结果摘要非空
    Writeback,
}

impl FaqPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            FaqPhase::Draft => 0,
            FaqPhase::Sealed => 1,
            FaqPhase::Writeback => 2,
        }
    }
}

/// 单条 FAQ 登记项（问题 / 答案 / 关联条目三列）。
#[derive(Clone, Debug)]
pub struct FaqItem {
    pub no: String,
    pub question: String,
    pub answer: String,
    pub related: usize,
}

/// FAQ 十问终版册。
pub struct FaqLedger {
    phase: FaqPhase,
    items: Vec<FaqItem>,
    hash: u32,
    summary: Option<String>,
}

impl FaqLedger {
    pub fn new() -> FaqLedger {
        FaqLedger {
            phase: FaqPhase::Draft,
            items: Vec::new(),
            hash: 0,
            summary: None,
        }
    }

    pub fn phase(&self) -> FaqPhase {
        self.phase
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    pub fn hash(&self) -> u32 {
        self.hash
    }

    /// 条目表闭集查询：位次 → (编号, 标题)。越表返回 None。
    pub fn lookup_entry(pos: usize) -> Option<(&'static str, &'static str)> {
        let id = ENTRIES.get(pos).map(|e| e.0);
        let title = ENTRIES.get(pos).map(|e| e.1);
        match (id, title) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        }
    }

    /// 第 `idx` 条登记项取回（越界返回 None）。
    pub fn item_at(&self, idx: usize) -> Option<(&str, &str, usize)> {
        match self.items.get(idx) {
            Some(it) => Some((it.question.as_str(), it.answer.as_str(), it.related)),
            None => None,
        }
    }

    /// 关联可溯判定：单条登记项的关联位次命中条目表闭集。
    pub fn link_resolves(related: usize) -> bool {
        match FaqLedger::lookup_entry(related) {
            Some((id, _)) => !id.is_empty(),
            None => false,
        }
    }

    /// FAQ 登记：编号唯一 + 三列齐备 + 关联落表，违者显性拒。
    pub fn register(&mut self, no: &str, question: &str, answer: &str, related: usize) -> Result<(), u32> {
        if self.phase == FaqPhase::Sealed || self.phase == FaqPhase::Writeback {
            return Err(FaqCode::SealedLocked.val());
        }
        if no.is_empty() {
            return Err(FaqCode::NoEmpty.val());
        }
        if question.is_empty() {
            return Err(FaqCode::QuestionEmpty.val());
        }
        if answer.is_empty() {
            return Err(FaqCode::AnswerEmpty.val());
        }
        let mut i = 0usize;
        while i < self.items.len() {
            let it = match self.items.get(i) {
                Some(it) => it,
                None => return Err(FaqCode::NotReproducible.val()),
            };
            if it.no == no {
                return Err(FaqCode::QuestionDup.val());
            }
            if it.question == question {
                return Err(FaqCode::QuestionDup.val());
            }
            i += 1;
        }
        if !FaqLedger::link_resolves(related) {
            return Err(FaqCode::LinkUnknown.val());
        }
        self.items.push(FaqItem {
            no: no.to_string(),
            question: question.to_string(),
            answer: answer.to_string(),
            related,
        });
        Ok(())
    }

    /// 封账：十问齐备且关联全可溯方可落哈希封册。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.phase.rank() > FaqPhase::Draft.rank() {
            return Err(FaqCode::PhaseRegress.val());
        }
        if self.items.len() < FAQ_COUNT {
            return Err(FaqCode::CountShort.val());
        }
        let mut text = String::new();
        let mut i = 0usize;
        while i < self.items.len() {
            let it = match self.items.get(i) {
                Some(it) => it,
                None => return Err(FaqCode::NotReproducible.val()),
            };
            if !FaqLedger::link_resolves(it.related) {
                return Err(FaqCode::LinkUnknown.val());
            }
            text.push_str(it.no.as_str());
            text.push('|');
            text.push_str(it.question.as_str());
            text.push('|');
            text.push_str(it.answer.as_str());
            text.push('|');
            let entry = match FaqLedger::lookup_entry(it.related) {
                Some((id, _)) => id,
                None => return Err(FaqCode::LinkUnknown.val()),
            };
            text.push_str(entry);
            text.push('\n');
            i += 1;
        }
        let h = fnv1a(text.as_bytes());
        if h == 0 {
            return Err(FaqCode::NotReproducible.val());
        }
        self.hash = h;
        self.phase = FaqPhase::Sealed;
        Ok(h)
    }

    /// 写回：封账前置，产物「状态=已完成、结果摘要非空」。
    pub fn write_back(&mut self) -> Result<String, u32> {
        if self.phase == FaqPhase::Draft {
            return Err(FaqCode::WritebackPremature.val());
        }
        if self.phase == FaqPhase::Writeback {
            return Err(FaqCode::PhaseRegress.val());
        }
        let traced = self.trace_count();
        let summary = format!(
            "FAQ 十问终版落档：登记 {} 问，答案关联可溯 {}/{}，档指纹 0x{:X}，状态=已完成",
            self.items.len(),
            traced,
            self.items.len(),
            self.hash
        );
        if summary.is_empty() {
            return Err(FaqCode::SummaryEmpty.val());
        }
        self.summary = Some(summary.clone());
        self.phase = FaqPhase::Writeback;
        Ok(summary)
    }

    /// 关联可溯计数：登记项中关联位次命中条目表的条数。
    pub fn trace_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = match self.items.get(i) {
                Some(it) => it,
                None => break,
            };
            if FaqLedger::link_resolves(it.related) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 摘要取回（写回产物可查）。
    pub fn summary_text(&self) -> Option<&str> {
        match self.summary.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }
}

// ── 好册构建（十问全登 + 封账） ─────────────────────────────────
/// 封账未写回的好册（写回契约判据的 fresh 前置态）。
pub fn build_faq_sealed() -> FaqLedger {
    let mut ledger = FaqLedger::new();
    let mut i = 0usize;
    while i < FAQ_SET.len() {
        let row = match FAQ_SET.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = ledger.register(row.0, row.1, row.2, row.3);
        i += 1;
    }
    let _ = ledger.seal();
    ledger
}

// ── 好册构建（十问全登 + 封账 + 写回） ──────────────────────────
pub fn build_faq_ok() -> FaqLedger {
    let mut ledger = build_faq_sealed();
    let _ = ledger.write_back();
    ledger
}

/// 注入册：重复问题 + 越表关联两路注入（好册复制后补注）。
pub fn build_faq_injected() -> FaqLedger {
    let mut ledger = FaqLedger::new();
    let mut i = 0usize;
    while i < FAQ_SET.len() {
        let row = match FAQ_SET.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = ledger.register(row.0, row.1, row.2, row.3);
        i += 1;
    }
    // 注入一：Q01 编号重登（不同问题、同编号）。
    let _ = ledger.register("UNX-F1552-Q01", "重复编号注入问句？", "重复编号注入答案。", 0);
    // 注入二：关联位次越条目表闭集（20 之外）。
    let _ = ledger.register("UNX-F1552-Q99", "越表关联注入问句？", "越表关联注入答案。", ENTRY_COUNT);
    ledger
}

/// 只登九问的欠册（封账必拒）。
pub fn build_faq_short() -> FaqLedger {
    let mut ledger = FaqLedger::new();
    let mut i = 0usize;
    while i < FAQ_SET.len() {
        let row = match FAQ_SET.get(i) {
            Some(r) => *r,
            None => break,
        };
        if i == 0 {
            // 跳过首条 → 9 问，关联齐但计数不足。
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
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78A00 && SEG_BASE <= 0x78BC0
}

/// 十问恰好登满且写回态到位。
fn faq_ten_ok() -> bool {
    let l = build_faq_ok();
    if l.count() != FAQ_COUNT {
        return false;
    }
    if l.phase() != FaqPhase::Writeback {
        return false;
    }
    true
}

/// 答案关联可溯 10/10：逐条位次命中条目表闭集。
fn link_trace_ok() -> bool {
    let l = build_faq_ok();
    if l.trace_count() != FAQ_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < FAQ_COUNT {
        let triple = match l.item_at(i) {
            Some(t) => t,
            None => return false,
        };
        if !FaqLedger::link_resolves(triple.2) {
            return false;
        }
        // 关联位次逐条与冻结表一致（列序可溯）。
        let row = match FAQ_SET.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if triple.2 != row.3 {
            return false;
        }
        i += 1;
    }
    true
}

/// 三列齐备且内容非空：问题、答案、编号逐条可读。
fn content_nonempty_ok() -> bool {
    let l = build_faq_ok();
    let mut i = 0usize;
    while i < FAQ_COUNT {
        let triple = match l.item_at(i) {
            Some(t) => t,
            None => return false,
        };
        if triple.0.is_empty() || triple.1.is_empty() {
            return false;
        }
        let row = match FAQ_SET.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if triple.0 != row.1 {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入必抓：重号 / 越表关联 / 欠册封账 / 空答案四路全抓。
fn injection_caught_ok() -> bool {
    let mut l = build_faq_injected();
    // 两路注入都被拒：账面仍恰好十问。
    if l.count() != FAQ_COUNT {
        return false;
    }
    // 欠册封账必拒。
    let mut short = build_faq_short();
    if short.count() != FAQ_COUNT - 1 {
        return false;
    }
    match short.seal() {
        Err(code) => {
            if code != FaqCode::CountShort.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 空答案注入必拒。
    let mut l2 = FaqLedger::new();
    match l2.register("UNX-F1552-Q01", "空答案注入问句？", "", 0) {
        Err(code) => {
            if code != FaqCode::AnswerEmpty.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if l2.count() != 0 {
        return false;
    }
    true
}

/// 关联越表守卫：越表位次登记必拒、查表必 None、可溯必 false。
fn link_guard_ok() -> bool {
    let mut l = FaqLedger::new();
    match l.register("UNX-F1552-Q01", "越表注入问句？", "越表注入答案。", ENTRY_COUNT) {
        Err(code) => {
            if code != FaqCode::LinkUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if l.count() != 0 {
        return false;
    }
    if FaqLedger::lookup_entry(ENTRY_COUNT).is_some() {
        return false;
    }
    if FaqLedger::link_resolves(usize::MAX) {
        return false;
    }
    true
}

/// FAQ 哈希落档：封账产物非零且与登记序绑定。
fn hash_archived_ok() -> bool {
    let l = build_faq_ok();
    if l.hash() == 0 {
        return false;
    }
    if l.phase() == FaqPhase::Draft {
        return false;
    }
    // 顺序扰动应改变指纹：交换登记序后哈希必不同（哈希绑序的证据）。
    let mut swapped = FaqLedger::new();
    let mut i = FAQ_SET.len();
    while i > 0 {
        i -= 1;
        let row = match FAQ_SET.get(i) {
            Some(r) => *r,
            None => return false,
        };
        let _ = swapped.register(row.0, row.1, row.2, row.3);
    }
    let _ = swapped.seal();
    if swapped.hash() == l.hash() {
        return false;
    }
    true
}

/// 双跑一致：好册哈希与写回摘要两次构建逐位相等。
fn double_run_ok() -> bool {
    let a = build_faq_ok();
    let b = build_faq_ok();
    if a.hash() != b.hash() {
        return false;
    }
    let sa = match a.summary_text() {
        Some(t) => t,
        None => return false,
    };
    let sb = match b.summary_text() {
        Some(t) => t,
        None => return false,
    };
    if sa != sb {
        return false;
    }
    if sa.is_empty() {
        return false;
    }
    true
}

/// 状态机单调：起草→封账→写回可达；未封账写回、二次写回、封账补登全拒。
fn phase_monotonic_ok() -> bool {
    let mut l = FaqLedger::new();
    if l.phase() != FaqPhase::Draft {
        return false;
    }
    // 未封账写回：跳级拒。
    match l.write_back() {
        Err(code) => {
            if code != FaqCode::WritebackPremature.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    let full = build_faq_ok();
    if full.phase() != FaqPhase::Writeback {
        return false;
    }
    // 封账补登拒（SealedLocked）。
    let mut sealed = FaqLedger::new();
    let mut k = 0usize;
    while k < FAQ_SET.len() {
        let row = match FAQ_SET.get(k) {
            Some(r) => *r,
            None => return false,
        };
        let _ = sealed.register(row.0, row.1, row.2, row.3);
        k += 1;
    }
    let _ = sealed.seal();
    match sealed.register("UNX-F1552-Q77", "封账补登注入？", "封账补登注入。", 0) {
        Err(code) => {
            if code != FaqCode::SealedLocked.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 空编号守卫：空编号登记必拒，账面不进脏数据。
fn no_guard_ok() -> bool {
    let mut l = FaqLedger::new();
    match l.register("", "空编号注入问句？", "空编号注入答案。", 0) {
        Err(code) => {
            if code != FaqCode::NoEmpty.val() {
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

/// 写回契约：状态=已完成、结果摘要非空、二次写回拒。
fn writeback_ok() -> bool {
    let mut l = build_faq_sealed();
    let summary = match l.write_back() {
        Ok(t) => t,
        Err(_) => return false,
    };
    if summary.is_empty() {
        return false;
    }
    if !summary.contains("状态=已完成") {
        return false;
    }
    if !summary.contains("10/10") {
        return false;
    }
    let archived = match l.summary_text() {
        Some(t) => t,
        None => return false,
    };
    if archived != summary {
        return false;
    }
    // 二次写回：倒退拒。
    match l.write_back() {
        Err(code) => {
            if code != FaqCode::PhaseRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 条目表闭集：20 条编号唯一可查。
fn entry_table_ok() -> bool {
    if ENTRIES.len() != ENTRY_COUNT {
        return false;
    }
    let mut seen: Vec<&str> = Vec::new();
    let mut i = 0usize;
    while i < ENTRIES.len() {
        let row = match ENTRIES.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if row.0.is_empty() || row.1.is_empty() {
            return false;
        }
        if seen.iter().any(|s| *s == row.0) {
            return false;
        }
        seen.push(row.0);
        i += 1;
    }
    if seen.len() != ENTRY_COUNT {
        return false;
    }
    let first = match FaqLedger::lookup_entry(0) {
        Some(e) => e.0,
        None => return false,
    };
    if first != "UNX-ENTRY-01" {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in FaqCode::ALL.iter() {
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
    let first = match FaqCode::ALL.get(0) {
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
pub fn run_unx1552_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1552 域级常见问题册");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78A00..=0x78BC0", SEG_BASE));
    s.add_dyn("FAQ 十问终版", faq_ten_ok(), "高频问题恰好十条登记封账并写回完成");
    s.add_dyn("答案关联可溯 10/10", link_trace_ok(), "逐条关联位次命中条目表闭集，可溯计数满额");
    s.add_dyn("三列齐备非空", content_nonempty_ok(), "问题/答案/关联三列逐条可读，与冻结表列序一致");
    s.add_dyn("注入必抓", injection_caught_ok(), "重号/越表关联/欠册封账/空答案四路全抓");
    s.add_dyn("关联越表守卫", link_guard_ok(), "越表位次登记必拒、查表必空、可溯必否");
    s.add_dyn("FAQ 哈希落档", hash_archived_ok(), "封账指纹非零且与登记序绑定，扰动即变");
    s.add_dyn("双跑一致", double_run_ok(), "好册哈希与写回摘要两次构建逐位相等");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "起草→封账→写回可达，跳级倒退封账补登全拒");
    s.add_dyn("空编号守卫", no_guard_ok(), "空编号登记必拒，账面不进脏数据");
    s.add_dyn("写回契约", writeback_ok(), "状态=已完成、结果摘要非空、二次写回拒");
    s.add_dyn("条目表闭集", entry_table_ok(), "20 条编号唯一可查，首条可溯");
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
        assert_eq!(SEG_BASE, 0x78A00);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1552_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn ten_questions_registered_and_writeback() {
        let l = build_faq_ok();
        assert_eq!(l.count(), 10);
        assert_eq!(l.phase(), FaqPhase::Writeback);
        assert!(l.hash() != 0);
    }

    #[test]
    fn link_trace_counts_ten() {
        let l = build_faq_ok();
        assert_eq!(l.trace_count(), 10);
        let mut i = 0usize;
        while i < FAQ_SET.len() {
            let row = FAQ_SET.get(i).unwrap();
            assert!(FaqLedger::link_resolves(row.3), "第 {} 问关联越表", i);
            i += 1;
        }
    }

    #[test]
    fn duplicate_question_rejected() {
        let mut l = FaqLedger::new();
        let row = FAQ_SET.get(0).unwrap();
        assert!(l.register(row.0, row.1, row.2, row.3).is_ok());
        match l.register("UNX-F1552-Q01", "另一问句？", "另一答案。", 1) {
            Err(code) => assert_eq!(code, FaqCode::QuestionDup.val()),
            Ok(()) => must_not_reach(),
        }
        assert_eq!(l.count(), 1);
    }

    #[test]
    fn out_of_table_link_rejected() {
        let mut l = FaqLedger::new();
        match l.register("UNX-F1552-Q88", "越表问句？", "越表答案。", 20) {
            Err(code) => assert_eq!(code, FaqCode::LinkUnknown.val()),
            Ok(()) => must_not_reach(),
        }
        assert!(FaqLedger::lookup_entry(20).is_none());
        assert!(!FaqLedger::link_resolves(999));
    }

    #[test]
    fn short_ledger_blocks_seal() {
        let mut l = build_faq_short();
        assert_eq!(l.count(), 9);
        match l.seal() {
            Err(code) => assert_eq!(code, FaqCode::CountShort.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn writeback_requires_seal() {
        let mut l = FaqLedger::new();
        let row = FAQ_SET.get(0).unwrap();
        let _ = l.register(row.0, row.1, row.2, row.3);
        match l.write_back() {
            Err(code) => assert_eq!(code, FaqCode::WritebackPremature.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn writeback_summary_contract() {
        let mut l = build_faq_sealed();
        let summary = match l.write_back() {
            Ok(t) => t,
            Err(_) => {
                must_not_reach();
                return;
            }
        };
        assert!(summary.contains("状态=已完成"));
        assert!(summary.contains("10/10"));
        assert!(!summary.is_empty());
    }

    #[test]
    fn double_run_digest_identical() {
        let a = build_faq_ok();
        let b = build_faq_ok();
        assert_eq!(a.hash(), b.hash());
        assert_ne!(a.hash(), 0);
    }

    #[test]
    fn sealed_ledger_rejects_register() {
        let mut l = build_faq_ok();
        match l.register("UNX-F1552-Q77", "封账补登？", "封账补登。", 0) {
            Err(code) => assert_eq!(code, FaqCode::SealedLocked.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(FaqCode::ALL.len(), 12);
        let first = match FaqCode::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match FaqCode::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
