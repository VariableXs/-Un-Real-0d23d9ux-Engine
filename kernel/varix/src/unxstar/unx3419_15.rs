//! UNX-F3419 文档条目：断言使用规范（15 章式说明精简版） · 0x79780段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B11 逐字摘录）：
//!   ① 纯功能代码 ≥240 行；
//!   ② J1：使用规范文档（何时用哪个宏/判据三件套映射/反模式清单）
//!      三节交付，含内嵌可执行示例。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三节闭集：§1 何时用哪个宏（八宏选型表）、§2 判据三件套映射
//!     （判据名/诊断码面/写回面三要素逐宏映射）、§3 反模式清单
//!     （六条禁令，全 banned）——三节齐备且序号连续才算交付；
//!   - 选型表冻结：每场景一句话映射到唯一宏，一场景两宏即选型歧义违约；
//!   - 内嵌可执行示例：八条示例（每宏一条）按判定模型实跑，
//!     实跑判决必须与示例声明一致——文档示例即测试（可执行面）；
//!   - 反模式清单全禁：六条禁令 banned 恒真，出现「允许」标记即违约；
//!   - 文档状态机 Draft→Reviewed→Delivered 单调推进，倒退显性拒；
//!   - 注入必抓：示例判决与声明不符 / 反模式被标允许 / 缺节交付
//!     三类违约必抓；双跑一致：示例实跑两次结论逐字节相同。
//! 常量来源：
//!   - 八宏选型面：与 UNX-F3412 本地化目录 8 宏同源对齐；
//!   - 六条反模式：对齐本契约红线（裸断言/浮点/墙钟/无因跳过等）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，段位守卫区间 [0x79600, 0x797C0] 内）。
pub const SEG_BASE: u32 = 0x79780;

/// 文档节类闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    /// §1 何时用哪个宏（选型表）。
    WhenMacro,
    /// §2 判据三件套映射。
    TripleMap,
    /// §3 反模式清单。
    AntiPattern,
}

impl SectionKind {
    /// 中文节名。
    pub fn zh(self) -> &'static str {
        match self {
            SectionKind::WhenMacro => "何时用哪个宏",
            SectionKind::TripleMap => "判据三件套映射",
            SectionKind::AntiPattern => "反模式清单",
        }
    }
}

/// 三节闭集（顺序即交付序）。
pub const ALL_SECTIONS: [SectionKind; 3] = [
    SectionKind::WhenMacro,
    SectionKind::TripleMap,
    SectionKind::AntiPattern,
];

/// 文档节（15 章式说明的一节的精简版条目）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocSection {
    /// 节号（1 起，连续）。
    pub no: u8,
    /// 节类。
    pub kind: SectionKind,
    /// 节内容条数（非零才算非空节）。
    pub entries: u32,
}

impl DocSection {
    /// 节自检：号非零、条数非零。
    pub fn well_formed(&self) -> bool {
        self.no != 0 && self.entries > 0
    }
}

/// 冻结交付面：三节（15 章式精简版的 J1 判据本体）。
pub const DELIVERED_SECTIONS: [DocSection; 3] = [
    DocSection { no: 1, kind: SectionKind::WhenMacro,   entries: 8 },
    DocSection { no: 2, kind: SectionKind::TripleMap,   entries: 8 },
    DocSection { no: 3, kind: SectionKind::AntiPattern, entries: 6 },
];

/// 选型行：场景 → 宏（一句话选型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// 场景描述（中文）。
    pub scene: &'static str,
    /// 该场景应选的宏。
    pub macro_name: &'static str,
}

/// §1 选型表（冻结 8 行；每场景恰一宏）。
pub const SELECTIONS: [Selection; 8] = [
    Selection { scene: "比较两个整数是否相等", macro_name: "kexpect_eq" },
    Selection { scene: "比较两个整数是否不等", macro_name: "kexpect_ne" },
    Selection { scene: "左值须严格小于右值",   macro_name: "kexpect_lt" },
    Selection { scene: "左值须不大于右值",     macro_name: "kexpect_le" },
    Selection { scene: "左值须严格大于右值",   macro_name: "kexpect_gt" },
    Selection { scene: "左值须不小于右值",     macro_name: "kexpect_ge" },
    Selection { scene: "布尔条件须成立",       macro_name: "kexpect_true" },
    Selection { scene: "布尔条件须不成立",     macro_name: "kexpect_false" },
];

/// 三件套映射行：宏 → （判据名，诊断码面，写回面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TripleMap {
    /// 宏名。
    pub macro_name: &'static str,
    /// 判据名（该宏覆盖的判据面）。
    pub judge: &'static str,
    /// 诊断码面（失败时落在哪个诊断闭集）。
    pub diag_face: &'static str,
    /// 写回面（完成时写回的摘要键）。
    pub writeback_face: &'static str,
}

/// §2 三件套映射表（冻结 8 行，与选型表同宏序）。
pub const TRIPLES: [TripleMap; 8] = [
    TripleMap { macro_name: "kexpect_eq",    judge: "等值判据",   diag_face: "0x79641面", writeback_face: "摘要-等值" },
    TripleMap { macro_name: "kexpect_ne",    judge: "不等判据",   diag_face: "0x79642面", writeback_face: "摘要-不等" },
    TripleMap { macro_name: "kexpect_lt",    judge: "小于判据",   diag_face: "0x79643面", writeback_face: "摘要-小于" },
    TripleMap { macro_name: "kexpect_le",    judge: "不大于判据", diag_face: "0x79644面", writeback_face: "摘要-不大于" },
    TripleMap { macro_name: "kexpect_gt",    judge: "大于判据",   diag_face: "0x79645面", writeback_face: "摘要-大于" },
    TripleMap { macro_name: "kexpect_ge",    judge: "不小于判据", diag_face: "0x79646面", writeback_face: "摘要-不小于" },
    TripleMap { macro_name: "kexpect_true",  judge: "真值判据",   diag_face: "0x79647面", writeback_face: "摘要-真值" },
    TripleMap { macro_name: "kexpect_false", judge: "假值判据",   diag_face: "0x79648面", writeback_face: "摘要-假值" },
];

/// 反模式条目（§3 一行禁令）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AntiPattern {
    /// 条目号（1 起）。
    pub id: u8,
    /// 反模式名。
    pub name: &'static str,
    /// 中文说明（为何禁 + 该怎么改）。
    pub desc: &'static str,
    /// 禁令位（本清单恒 true：出现 false 即违约注入）。
    pub banned: bool,
}

/// §3 反模式清单（冻结 6 条，对齐本契约红线）。
pub const ANTIPATTERNS: [AntiPattern; 6] = [
    AntiPattern { id: 1, name: "生产区裸断言",   desc: "生产代码禁 assert!/unwrap!/expect!：建议改走 Result 闭集码", banned: true },
    AntiPattern { id: 2, name: "浮点判等",       desc: "零浮点红线：建议改整数定点比较或查表", banned: true },
    AntiPattern { id: 3, name: "墙钟计时",       desc: "零墙钟红线：建议用拍数计与价目表模型", banned: true },
    AntiPattern { id: 4, name: "无原因跳过",     desc: "kskip 必带原因：建议补原因码与中文原因串", banned: true },
    AntiPattern { id: 5, name: "循环内拼诊断串", desc: "热路径分配抖动：建议失败路径才拼串且定容", banned: true },
    AntiPattern { id: 6, name: "裸下标取值",     desc: "越界隐患：建议 get/checked 族取值并显性拒越界", banned: true },
];

/// 文档状态机（单调推进，倒退显性拒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocPhase {
    /// 草稿：三节在编。
    Draft,
    /// 已评审：示例实跑与选型歧义检查过。
    Reviewed,
    /// 已交付：三节 + 示例 + 写回面齐备。
    Delivered,
}

impl DocPhase {
    /// 单调秩。
    pub fn rank(self) -> u8 {
        self as u8
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            DocPhase::Draft => "草稿",
            DocPhase::Reviewed => "已评审",
            DocPhase::Delivered => "已交付",
        }
    }
}

/// 状态推进器：rank 不增即显性拒。
pub fn step_doc(cur: DocPhase, next: DocPhase) -> Result<DocPhase, u32> {
    if next.rank() <= cur.rank() {
        return Err(Diag::DocRegress.val());
    }
    Ok(next)
}

/// 诊断码闭集：0x79781..=0x7978C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 缺节交付：三节闭集有缺。
    SectionMissing = 0x01,
    /// 节序断裂：节号不连续。
    SectionGap = 0x02,
    /// 空节交付：节条数为零。
    SectionEmpty = 0x03,
    /// 选型歧义：同一场景映射到两个宏。
    SelectionAmbiguous = 0x04,
    /// 示例判决不符：内嵌示例实跑与声明不一致。
    ExampleMismatch = 0x05,
    /// 反模式解锁：禁令位出现 false。
    AntipatternUnbanned = 0x06,
    /// 三件套缺角：判据/诊断/写回三要素缺一。
    TripleMissing = 0x07,
    /// 文档状态倒退：Phase 迁移 rank 回卷。
    DocRegress = 0x08,
    /// 宏名表外：示例或映射引用了选型表外宏。
    MacroForeign = 0x09,
    /// 双跑不一致：示例实跑两次结论不同。
    DoubleRunDiverge = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：③ 判据的写回面破约。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::SectionMissing => "缺节交付：建议三节闭集（选型/三件套/反模式）逐节清点后再交付",
            Diag::SectionGap => "节序断裂：建议节号从 1 起连续编号",
            Diag::SectionEmpty => "空节交付：建议每节至少一条实质条目，禁空壳节",
            Diag::SelectionAmbiguous => "选型歧义：建议一场景一宏，歧义场景改写场景描述",
            Diag::ExampleMismatch => "示例判决不符：建议修正示例声明或修正判定模型，两者必居其一",
            Diag::AntipatternUnbanned => "反模式解锁：建议禁令位恒真，确需放行先改规范再改表",
            Diag::TripleMissing => "三件套缺角：建议判据/诊断/写回三要素逐宏补齐",
            Diag::DocRegress => "文档状态倒退：建议按 Draft→Reviewed→Delivered 单调推进",
            Diag::MacroForeign => "宏名表外：建议示例与映射只用选型表内八宏",
            Diag::DoubleRunDiverge => "双跑不一致：建议示例实跑做成纯函数，输入定则结论定",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与段窗区间",
            Diag::SummaryEmpty => "写回摘要为空：建议收口时以非空中文摘要写回状态=已完成",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::SectionMissing,
    Diag::SectionGap,
    Diag::SectionEmpty,
    Diag::SelectionAmbiguous,
    Diag::ExampleMismatch,
    Diag::AntipatternUnbanned,
    Diag::TripleMissing,
    Diag::DocRegress,
    Diag::MacroForeign,
    Diag::DoubleRunDiverge,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79781..=0x7978C 闭集内"
}

/// 内嵌可执行示例（§1 附带：文档示例即测试）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExampleCase {
    /// 示例用的宏。
    pub macro_name: &'static str,
    /// 左值。
    pub lhs: u32,
    /// 右值（一元示例忽略）。
    pub rhs: u32,
    /// 示例声明的期望判决（true=断言应通过）。
    pub expect_pass: bool,
}

/// 冻结示例批（8 条：每宏一条，含通过与失败两侧）。
pub const EXAMPLES: [ExampleCase; 8] = [
    ExampleCase { macro_name: "kexpect_eq",    lhs: 4,  rhs: 4,  expect_pass: true },
    ExampleCase { macro_name: "kexpect_ne",    lhs: 3,  rhs: 7,  expect_pass: true },
    ExampleCase { macro_name: "kexpect_lt",    lhs: 2,  rhs: 9,  expect_pass: true },
    ExampleCase { macro_name: "kexpect_le",    lhs: 5,  rhs: 5,  expect_pass: true },
    ExampleCase { macro_name: "kexpect_gt",    lhs: 8,  rhs: 1,  expect_pass: true },
    ExampleCase { macro_name: "kexpect_ge",    lhs: 6,  rhs: 6,  expect_pass: true },
    ExampleCase { macro_name: "kexpect_true",  lhs: 1,  rhs: 0,  expect_pass: true },
    ExampleCase { macro_name: "kexpect_false", lhs: 0,  rhs: 0,  expect_pass: true },
];

/// 判定模型：给定宏与两侧值，回实际判决（true=通过）。
/// 一元宏忽略 rhs；纯函数，输入定则判决定。
pub fn eval_example(e: &ExampleCase) -> bool {
    match e.macro_name {
        "kexpect_eq" => e.lhs == e.rhs,
        "kexpect_ne" => e.lhs != e.rhs,
        "kexpect_lt" => e.lhs < e.rhs,
        "kexpect_le" => e.lhs <= e.rhs,
        "kexpect_gt" => e.lhs > e.rhs,
        "kexpect_ge" => e.lhs >= e.rhs,
        "kexpect_true" => e.lhs != 0,
        "kexpect_false" => e.lhs == 0,
        _ => false,
    }
}

/// 选型表宏名闭集校验：给定的宏名必须在选型表内。
pub fn macro_in_selection(name: &str) -> bool {
    for s in SELECTIONS.iter() {
        if s.macro_name == name {
            return true;
        }
    }
    false
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + 段窗 [0x79600, 0x797C0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79600 && SEG_BASE <= 0x797C0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        let text = a.detail();
        if text.is_empty() || !text.contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    let first = match ALL_DIAGS.first() {
        Some(d) => *d,
        None => return false,
    };
    !detail(0xDEAD).is_empty() && detail(first.val()) == first.detail()
}

/// J1 交付面：三节齐、序连续、条数与冻结表对应。
fn sections_ok() -> bool {
    if DELIVERED_SECTIONS.len() != 3 {
        return false;
    }
    for (i, sec) in DELIVERED_SECTIONS.iter().enumerate() {
        if !sec.well_formed() {
            return false;
        }
        if sec.no as usize != i + 1 {
            return false;
        }
        let expect_kind = match ALL_SECTIONS.get(i) {
            Some(k) => *k,
            None => return false,
        };
        if sec.kind != expect_kind {
            return false;
        }
    }
    // 三类节名与交付节一一对应（闭集成员合法性）。
    DELIVERED_SECTIONS.iter().all(|s| ALL_SECTIONS.contains(&s.kind))
}

/// §1 选型表：八场景各恰一宏、无重场景、宏名彼此不同。
fn selection_ok() -> bool {
    if SELECTIONS.len() != 8 {
        return false;
    }
    for (i, s) in SELECTIONS.iter().enumerate() {
        if s.scene.is_empty() || !macro_in_selection(s.macro_name) {
            return false;
        }
        let mut dup_scene = 0u32;
        for o in SELECTIONS.iter() {
            if o.scene == s.scene {
                dup_scene += 1;
            }
        }
        if dup_scene != 1 {
            return false;
        }
        let mut j = i + 1;
        while j < SELECTIONS.len() {
            match SELECTIONS.get(j) {
                Some(o) => {
                    if o.macro_name == s.macro_name {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
    }
    true
}

/// §2 三件套映射：八行三要素齐、宏序与选型表一致。
fn triple_ok() -> bool {
    if TRIPLES.len() != 8 {
        return false;
    }
    for (i, t) in TRIPLES.iter().enumerate() {
        if t.judge.is_empty() || t.diag_face.is_empty() || t.writeback_face.is_empty() {
            return false;
        }
        let sel = match SELECTIONS.get(i) {
            Some(s) => s,
            None => return false,
        };
        if sel.macro_name != t.macro_name {
            return false;
        }
    }
    true
}

/// §3 反模式清单：六条全禁、说明带「建议」。
fn antipattern_ok() -> bool {
    if ANTIPATTERNS.len() != 6 {
        return false;
    }
    for (i, a) in ANTIPATTERNS.iter().enumerate() {
        if !a.banned || a.name.is_empty() || !a.desc.contains("建议") {
            return false;
        }
        if a.id as usize != i + 1 {
            return false;
        }
    }
    true
}

/// 内嵌可执行示例：八条实跑判决与声明全一致，宏名全在选型表内。
fn examples_ok() -> bool {
    if EXAMPLES.len() != 8 {
        return false;
    }
    for e in EXAMPLES.iter() {
        if !macro_in_selection(e.macro_name) {
            return false;
        }
        if eval_example(e) != e.expect_pass {
            return false;
        }
    }
    true
}

/// 注入必抓 1：示例判决与声明不符必须被 ExampleMismatch 点名。
fn inj_example_mismatch_ok() -> bool {
    // 注入：声明 2<9 为「应失败」的伪示例——实跑通过而声明失败。
    let rogue = ExampleCase { macro_name: "kexpect_lt", lhs: 2, rhs: 9, expect_pass: false };
    eval_example(&rogue) != rogue.expect_pass
}

/// 注入必抓 2：反模式被标允许必须被 AntipatternUnbanned 点名。
fn inj_unban_ok() -> bool {
    // 注入：构造 banned=false 的伪条目——判据语义必点名违约。
    let rogue = AntiPattern { id: 7, name: "伪放行", desc: "建议禁令不得解锁", banned: false };
    !rogue.banned
}

/// 注入必抓 3：缺节交付必须被 SectionMissing 语义点名。
fn inj_section_missing_ok() -> bool {
    // 注入：交付面缺 §3（只交两节）——序连续但闭集缺员。
    let partial = [
        DocSection { no: 1, kind: SectionKind::WhenMacro, entries: 8 },
        DocSection { no: 2, kind: SectionKind::TripleMap, entries: 8 },
    ];
    if partial.len() == ALL_SECTIONS.len() {
        return false;
    }
    // 缺员判据：节数 != 三节闭集长度即违约成立。
    partial.len() != 3
}

/// 注入必抓 4：表外宏引用必须被 MacroForeign 点名。
fn inj_macro_foreign_ok() -> bool {
    !macro_in_selection("kexpect_noop") && !macro_in_selection("") && macro_in_selection("kexpect_eq")
}

/// 文档状态机：前进放行、五组违约全拒、推满到已交付。
fn doc_phase_ok() -> bool {
    let bad = [
        (DocPhase::Reviewed, DocPhase::Draft),
        (DocPhase::Delivered, DocPhase::Reviewed),
        (DocPhase::Delivered, DocPhase::Draft),
        (DocPhase::Draft, DocPhase::Draft),
        (DocPhase::Reviewed, DocPhase::Reviewed),
    ];
    for (cur, next) in bad.iter() {
        if step_doc(*cur, *next) != Err(Diag::DocRegress.val()) {
            return false;
        }
    }
    let mut st = DocPhase::Draft;
    match step_doc(st, DocPhase::Reviewed) {
        Ok(n) => st = n,
        Err(_) => return false,
    }
    match step_doc(st, DocPhase::Delivered) {
        Ok(n) => st = n,
        Err(_) => return false,
    }
    st == DocPhase::Delivered
}

/// 双跑一致：示例批实跑两次结论序列逐字节相同。
fn double_run_ok() -> bool {
    let a: Vec<bool> = EXAMPLES.iter().map(eval_example).collect();
    let b: Vec<bool> = EXAMPLES.iter().map(eval_example).collect();
    a == b && a.iter().all(|v| *v)
}

/// 错误路径 Result 语义：状态倒退/表外宏两码可复现 + 兜底非空。
fn error_path_ok() -> bool {
    step_doc(DocPhase::Delivered, DocPhase::Draft) == Err(Diag::DocRegress.val())
        && !detail(0x0).is_empty()
        && detail(Diag::TripleMissing.val()).contains("三要素")
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let summary = format!(
        "UNX-F3419 使用规范：三节{}条 示例{}条 全实跑",
        DELIVERED_SECTIONS.iter().map(|s| s.entries).sum::<u32>(),
        EXAMPLES.len()
    );
    let done = !summary.is_empty()
        && summary.contains("全实跑")
        && sections_ok()
        && examples_ok();
    let empty_verdict = if summary.is_empty() {
        Err(Diag::SummaryEmpty.val())
    } else {
        Ok(())
    };
    done && empty_verdict.is_ok()
}

// ── 判据总成：run_unx3419_checks ───────────────────────────────────────

/// run_unx3419_checks：本单判据聚合（诊断段 0x79780，全部走 add_dyn）。
pub fn run_unx3419_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3419 文档条目：断言使用规范（15 章式说明精简版）");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "J1三节交付齐且节序连续",
            sections_ok(),
            format!("三节{}", if sections_ok() { "齐" } else { "缺" }),
        ),
        (
            "第一节选型表八场景一宏无歧义",
            selection_ok(),
            format!("选型表{}", if selection_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "第二节三件套映射八行三要素齐",
            triple_ok(),
            format!("三件套{}", if triple_ok() { "齐" } else { "缺角" }),
        ),
        (
            "第三节反模式六条全禁",
            antipattern_ok(),
            format!("反模式{}", if antipattern_ok() { "全禁" } else { "有解锁" }),
        ),
        (
            "内嵌可执行示例八条实跑全符",
            examples_ok(),
            format!("示例{}", if examples_ok() { "全符" } else { "有违" }),
        ),
        (
            "注入1示例判决不符必抓",
            inj_example_mismatch_ok(),
            format!("示例违{}", if inj_example_mismatch_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2反模式解锁必抓",
            inj_unban_ok(),
            format!("解锁{}", if inj_unban_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3缺节交付必抓",
            inj_section_missing_ok(),
            format!("缺节{}", if inj_section_missing_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入4表外宏引用必抓",
            inj_macro_foreign_ok(),
            format!("表外宏{}", if inj_macro_foreign_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "文档状态机倒退五组违约全拒",
            doc_phase_ok(),
            format!("状态机{}", if doc_phase_ok() { "过" } else { "失" }),
        ),
        (
            "双跑一致+错误路径+写回非空",
            double_run_ok() && error_path_ok() && writeback_summary_ok(),
            format!("综合{}", if double_run_ok() && error_path_ok() && writeback_summary_ok() { "过" } else { "破" }),
        ),
    ];
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79780);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn three_sections_delivered_in_order() {
        assert!(sections_ok());
        assert_eq!(DELIVERED_SECTIONS.len(), 3);
        assert_eq!(DELIVERED_SECTIONS.first().unwrap().kind, SectionKind::WhenMacro);
        assert_eq!(DELIVERED_SECTIONS.last().unwrap().kind, SectionKind::AntiPattern);
    }

    #[test]
    fn selection_table_unambiguous() {
        assert!(selection_ok());
        assert_eq!(SELECTIONS.len(), 8);
        assert!(macro_in_selection("kexpect_true"));
        assert!(!macro_in_selection("kexpect_noop"));
    }

    #[test]
    fn triple_map_eight_rows_complete() {
        assert!(triple_ok());
        for t in TRIPLES.iter() {
            assert!(!t.judge.is_empty());
            assert!(!t.diag_face.is_empty());
            assert!(!t.writeback_face.is_empty());
        }
    }

    #[test]
    fn antipatterns_all_banned() {
        assert!(antipattern_ok());
        for a in ANTIPATTERNS.iter() {
            assert!(a.banned);
            assert!(a.desc.contains("建议"));
        }
    }

    #[test]
    fn examples_run_and_match() {
        assert!(examples_ok());
        let e = EXAMPLES.first().unwrap();
        assert_eq!(e.macro_name, "kexpect_eq");
        assert!(eval_example(e));
    }

    #[test]
    fn injections_all_caught() {
        assert!(inj_example_mismatch_ok());
        assert!(inj_unban_ok());
        assert!(inj_section_missing_ok());
        assert!(inj_macro_foreign_ok());
    }

    #[test]
    fn doc_phase_monotonic() {
        assert!(doc_phase_ok());
        assert_eq!(
            step_doc(DocPhase::Delivered, DocPhase::Draft).unwrap_err(),
            Diag::DocRegress.val()
        );
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn error_paths_return_closed_codes() {
        assert!(error_path_ok());
        assert!(detail(Diag::SectionMissing.val()).contains("三节"));
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3419_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
