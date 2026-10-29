#![allow(dead_code)]
// =====================================================================================
// 任务单：UNX-F0778 · 收官报告草拟（六节初稿+账源指针）
// 域/批：CoRun·UNX A1 收官域（跨域回执/风险登记/A1 域满收官主题 · W7 清单第 9 单）
// 册目标：580 行 ｜ 本文件实际：≥580 行（自包含单文件子系统）
// 判据原文（清单逐字抄录）：
//   纯功能代码 ≥580 行；② 判据原文：UNX-F0778-J1 初稿六节齐（域概览/判据面/
//   域经面/移交面/风险面/遗留面——节全 6/6）且字段账源指针可解引 ≥90%；③ 完
//   成时 API/软件写回：状态=已完成、结果摘要非空。
// 设计纪律：① 六节闭集建模（域概览/判据面/域经面/移交面/风险面/遗留面），
//   每节定容字段账，节缺一即「节全 6/6」断言红。② 每个字段携带账源指针
//   SourcePtr{ledger, idx}，解引规则：账册已登记且 idx < 账长 → 可解引；
//   可解引率 = 解引数/指针总数，整数判据 ≥90%（×10 防浮点）。
// ③ 账源账册六本与六节一一对应（概览/判据/域经/移交/风险/遗留），账册定容
//   64，登记守恒：Σ账长 == Σ指针可解引候选。④ 状态机 Drafting→SectionsDone→
//   PointersBound→Validated→Finalized 单向闭集。⑤ 诊断码定位到（节, 字段）。
// 诊断段声明：0x6000–0x601F 本单独占（清单 <SEG 0x6000>），不得他用。
// 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
// =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x6000–0x601F）
pub const SEG_BASE: u16 = 0x6000; // 诊断码段基数，来源：W7 清单 UNX-F0778 行 <SEG 0x6000>
pub const SEG_TOP: u16 = 0x601F; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 草拟状态机非法边
pub const XC_SECTION_SHORT: u16 = SEG_BASE + 2; // 节全 6/6 断言红
pub const XC_DUP_SECTION: u16 = SEG_BASE + 3; // 节号重复
pub const XC_DUP_FIELD: u16 = SEG_BASE + 4; // 节内字段号重复
pub const XC_PTR_BAD_LEDGER: u16 = SEG_BASE + 5; // 指针账册代号越出六本闭集
pub const XC_PTR_UNRESOLVED: u16 = SEG_BASE + 6; // 指针解引失败（idx 越账长）
pub const XC_RESOLVE_LOW: u16 = SEG_BASE + 7; // 可解引率 <90%（×10 整数判据）
pub const XC_LEDGER_BAD: u16 = SEG_BASE + 8; // 账册登记越闭集/超容
pub const XC_EMPTY_FIELD: u16 = SEG_BASE + 9; // 空字段批
pub const XC_NOT_BOUND: u16 = SEG_BASE + 10; // 指针未绑完即请求校验
pub const XC_ALREADY_FINAL: u16 = SEG_BASE + 11; // 已定稿再操作
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 12; // 定稿摘要为空
pub const XC_FIELD_CAP: u16 = SEG_BASE + 13; // 节字段账超定容
pub const XC_VALUE_ZERO: u16 = SEG_BASE + 14; // 字段值为 0（非法占位）
pub const XC_COUNT_MISMATCH: u16 = SEG_BASE + 15; // 指针计数守恒断裂

// 2. 报告常量（写死，来源=判据原文/本单口径头注假设）
pub const SECTION_COUNT: usize = 6; // 六节，来源：判据原文「六节齐」
pub const FIELDS_PER_SECTION: usize = 6; // 每节字段定容，头注假设
pub const FIELD_TOTAL: usize = SECTION_COUNT * FIELDS_PER_SECTION; // 36 字段口径
pub const RESOLVE_PROMILLE: u64 = 900; // 可解引率 ≥90%（千分比整数判据）
pub const LEDGER_CAP: usize = 64; // 账册定容，头注假设

/// 六节闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReportSection {
    DomainOverview = 0, // 域概览
    CritFace = 1,       // 判据面
    ScrollFace = 2,     // 域经面
    HandoverFace = 3,   // 移交面
    RiskFace = 4,       // 风险面
    LegacyFace = 5,     // 遗留面
}
impl ReportSection {
    pub fn zh(self) -> &'static str {
        match self {
            ReportSection::DomainOverview => "域概览",
            ReportSection::CritFace => "判据面",
            ReportSection::ScrollFace => "域经面",
            ReportSection::HandoverFace => "移交面",
            ReportSection::RiskFace => "风险面",
            ReportSection::LegacyFace => "遗留面",
        }
    }
}

/// 账源账册代号闭集（与六节一一对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceLedger {
    Overview = 0,
    Crit = 1,
    Scroll = 2,
    Handover = 3,
    Risk = 4,
    Legacy = 5,
}

/// 账源指针：字段 → (账册, 账内下标)。
#[derive(Clone, Copy, Debug)]
pub struct SourcePtr {
    pub ledger: u8, // 账册代号 0..=5（闭集外拒收）
    pub idx: u32,   // 账内下标（< 账长才可解引）
}

/// 报告字段：值 + 账源指针。
#[derive(Clone, Copy, Debug)]
pub struct ReportField {
    pub seq: u8,      // 节内字段序 0..=5
    pub value: u64,   // 字段值（非 0）
    pub ptr: SourcePtr, // 账源指针
}

/// 节内容：节号 + 字段账（≤6）。
#[derive(Clone, Debug)]
pub struct SectionDraft {
    pub section: u8, // 节号 0..=5（闭集外拒收）
    pub fields: Vec<ReportField>,
}

/// 指针解引报告。
#[derive(Clone, Debug)]
pub struct ResolveReport {
    pub total_ptrs: u32,     // 指针总数
    pub resolved: u32,       // 可解引数
    pub unresolved: u32,     // 不可解引数（守恒：和 == 总数）
    pub promille: u64,       // 可解引率（千分比整数）
    pub bad_fields: Vec<(u8, u8)>, // 不可解引定位（节, 字段）
}

/// 定稿回执。
#[derive(Clone, Debug, PartialEq)]
pub struct FinalReceipt {
    pub sections_done: u32,  // 节全 6/6 回执
    pub fields_done: u32,    // 字段总数（36 口径）
    pub resolve_promille: u64, // 可解引率回执
    pub summary: String,     // 摘要（非空）
    pub fingerprint: u64,    // 稿面指纹
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DraftStage {
    Drafting = 0,
    SectionsDone = 1,
    PointersBound = 2,
    Validated = 3,
    Finalized = 4,
}
impl DraftStage {
    pub fn can_step(self, next: DraftStage) -> bool {
        (self as u8) + 1 == next as u8
    }
}
pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 单步。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}
pub struct ReportDrafter {  // 收官报告草拟机。
    stage: DraftStage,
    sections: Vec<SectionDraft>, // 六节稿面
    ledgers: Vec<Vec<u64>>,      // 六本账源账册（代号 = 下标）
    seen_sections: Vec<u8>,
    resolve: Option<ResolveReport>,
    events: Vec<String>,
}
impl ReportDrafter {
    pub fn new() -> Self {
        ReportDrafter {
            stage: DraftStage::Drafting,
            sections: Vec::new(),
            ledgers: Vec::new(),
            seen_sections: Vec::new(),
            resolve: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> DraftStage {
        self.stage
    }
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn register_ledger(&mut self, ledger: u8, entries: &[u64]) -> Result<u32, u16> {  // 登记账源账册：代号闭集 0..=5，每本 ≤64 条，非空。
        if self.stage != DraftStage::Drafting {
            return Err(XC_STATE_INVALID);
        }
        if ledger as usize >= SECTION_COUNT {
            return Err(XC_LEDGER_BAD);
        }
        if entries.is_empty() {
            return Err(XC_EMPTY_FIELD);
        }
        if entries.len() > LEDGER_CAP {
            return Err(XC_LEDGER_BAD);
        }
        // 重复登记覆盖即拒（账册只可登记一次，禁静默改账）。
        if self.ledgers.len() > ledger as usize && !self.ledgers[ledger as usize].is_empty() {
            return Err(XC_LEDGER_BAD);
        }
        while self.ledgers.len() <= ledger as usize {
            self.ledgers.push(Vec::new());
        }
        let mut i = 0usize;
        while i < entries.len() {
            self.ledgers[ledger as usize].push(entries[i]);
            i += 1;
        }
        self.events.push(alloc::format!("ledger {} +{}", ledger, entries.len()));
        Ok(entries.len() as u32)
    }
    pub fn ledger_len(&self, ledger: u8) -> Result<usize, u16> {
        if ledger as usize >= SECTION_COUNT {
            return Err(XC_LEDGER_BAD);
        }
        if ledger as usize >= self.ledgers.len() {
            return Ok(0);
        }
        Ok(self.ledgers[ledger as usize].len())
    }
    pub fn resolve_ptr(&self, p: &SourcePtr) -> Result<bool, u16> {  // 指针解引单点：账册在册且 idx < 账长。
        if p.ledger as usize >= SECTION_COUNT {
            return Err(XC_PTR_BAD_LEDGER);
        }
        if p.ledger as usize >= self.ledgers.len() {
            return Ok(false);
        }
        Ok((p.idx as usize) < self.ledgers[p.ledger as usize].len())
    }
    pub fn submit_section(&mut self, sec: &SectionDraft) -> Result<u32, u16> {  // 提交一节稿面：节号闭集唯一、字段号节内唯一、指针结构合法。
        if !self.stage.can_step(DraftStage::SectionsDone) {
            return Err(XC_STATE_INVALID);
        }
        if sec.section as usize >= SECTION_COUNT {
            return Err(XC_SECTION_SHORT);
        }
        if sec.fields.is_empty() {
            return Err(XC_EMPTY_FIELD);
        }
        if sec.fields.len() > FIELDS_PER_SECTION {
            return Err(XC_FIELD_CAP);
        }
        let mut i = 0usize;
        while i < self.seen_sections.len() {
            if self.seen_sections[i] == sec.section {
                return Err(XC_DUP_SECTION);
            }
            i += 1;
        }
        let mut a = 0usize;
        while a < sec.fields.len() {
            if sec.fields[a].value == 0 {
                return Err(XC_VALUE_ZERO);
            }
            if sec.fields[a].ptr.ledger as usize >= SECTION_COUNT {
                return Err(XC_PTR_BAD_LEDGER);
            }
            let mut b = a + 1;
            while b < sec.fields.len() {
                if sec.fields[b].seq == sec.fields[a].seq {
                    return Err(XC_DUP_FIELD);
                }
                b += 1;
            }
            a += 1;
        }
        self.seen_sections.push(sec.section);
        let n = sec.fields.len() as u32;
        self.sections.push(sec.clone());
        self.events.push(alloc::format!("section {} +{}", sec.section, n));
        Ok(n)
    }
    pub fn sections_assert(&mut self) -> Result<(u32, u32), u16> {  // 节全断言：6/6 节齐且每节 6 字段（36 字段口径）。
        if !self.stage.can_step(DraftStage::PointersBound) {
            return Err(XC_STATE_INVALID);
        }
        if self.sections.len() != SECTION_COUNT {
            return Err(XC_SECTION_SHORT);
        }
        let mut total_fields: u32 = 0;
        let mut i = 0usize;
        while i < self.sections.len() {
            total_fields += self.sections[i].fields.len() as u32;
            i += 1;
        }
        if total_fields as usize != FIELD_TOTAL {
            return Err(XC_FIELD_CAP);
        }
        self.stage = DraftStage::PointersBound;
        self.events.push(String::from("sections 6/6"));
        Ok((SECTION_COUNT as u32, total_fields))
    }
    pub fn bind_pointers(&mut self) -> Result<ResolveReport, u16> {  // 指针全量解引：36 指针逐个解引，产出报告并执行 ≥90% 整数判据。
        if !self.stage.can_step(DraftStage::Validated) {
            return Err(XC_NOT_BOUND);
        }
        let mut rep = ResolveReport {
            total_ptrs: 0,
            resolved: 0,
            unresolved: 0,
            promille: 0,
            bad_fields: Vec::new(),
        };
        let mut s = 0usize;
        while s < self.sections.len() {
            let sec = &self.sections[s];
            let mut f = 0usize;
            while f < sec.fields.len() {
                let fld = &sec.fields[f];
                rep.total_ptrs += 1;
                match self.resolve_ptr(&fld.ptr) {
                    Ok(true) => rep.resolved += 1,
                    Ok(false) => {
                        rep.unresolved += 1;
                        rep.bad_fields.push((sec.section, fld.seq));
                    }
                    Err(c) => return Err(c),
                }
                f += 1;
            }
            s += 1;
        }
        if rep.resolved + rep.unresolved != rep.total_ptrs {
            return Err(XC_COUNT_MISMATCH);
        }
        // 千分比整数判据：resolved*1000 / total ≥ 900。
        if rep.total_ptrs == 0 {
            return Err(XC_EMPTY_FIELD);
        }
        rep.promille = (rep.resolved as u64 * 1000) / rep.total_ptrs as u64;
        self.resolve = Some(rep.clone());
        self.events.push(alloc::format!("resolve {}/{}", rep.resolved, rep.total_ptrs));
        Ok(rep)
    }
    pub fn last_resolve(&self) -> Option<&ResolveReport> {
        self.resolve.as_ref()
    }
    pub fn finalize(&mut self) -> Result<FinalReceipt, u16> {  // 定稿：节全 6/6 + 指针率 ≥90% → 摘要定格。
        if !self.stage.can_step(DraftStage::Finalized) {
            return Err(if self.stage == DraftStage::Finalized {
                XC_ALREADY_FINAL
            } else {
                XC_NOT_BOUND
            });
        }
        let rep = match &self.resolve {
            Some(r) => r.clone(),
            None => return Err(XC_NOT_BOUND),
        };
        if rep.promille < RESOLVE_PROMILLE {
            return Err(XC_RESOLVE_LOW);
        }
        if self.sections.len() != SECTION_COUNT {
            return Err(XC_SECTION_SHORT);
        }
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0778-J1 初稿六节齐（6/6），字段 {} 个账源指针可解引率 {}‰（≥90%），定稿完成",
            rep.total_ptrs, rep.promille
        );
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        self.stage = DraftStage::Finalized;
        Ok(FinalReceipt {
            sections_done: SECTION_COUNT as u32,
            fields_done: rep.total_ptrs,
            resolve_promille: rep.promille,
            summary,
            fingerprint: fp,
        })
    }
    pub fn fingerprint(&self) -> u64 {  // 稿面指纹：全部（节, 字段, 值, 账册, 下标）规范序。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut s = 0usize;
        while s < self.sections.len() {
            let sec = &self.sections[s];
            h = fnv1a(h, sec.section as u64);
            let mut f = 0usize;
            while f < sec.fields.len() {
                let fl = &sec.fields[f];
                h = fnv1a(h, fl.seq as u64);
                h = fnv1a(h, fl.value);
                h = fnv1a(h, fl.ptr.ledger as u64);
                h = fnv1a(h, fl.ptr.idx as u64);
                f += 1;
            }
            s += 1;
        }
        h
    }
    pub fn coverage_by_section(&self) -> Result<[(u32, u32); SECTION_COUNT], u16> {  // 分节解引覆盖：逐节（指针数, 可解引数）六桶报告（定位粒度）。
        let mut cov = [(0u32, 0u32); SECTION_COUNT];
        let mut s = 0usize;
        while s < self.sections.len() {
            let sec = &self.sections[s];
            if sec.section as usize >= SECTION_COUNT {
                return Err(XC_SECTION_SHORT);
            }
            let mut f = 0usize;
            while f < sec.fields.len() {
                cov[sec.section as usize].0 += 1;
                if self.resolve_ptr(&sec.fields[f].ptr)? {
                    cov[sec.section as usize].1 += 1;
                }
                f += 1;
            }
            s += 1;
        }
        Ok(cov)
    }

    pub fn resolve_field(&self, section: u8, seq: u8) -> Result<bool, u16> {  // 单字段指针解引查询（节未提交/字段越节容 → 定位错误码）。
        if section as usize >= SECTION_COUNT {
            return Err(XC_SECTION_SHORT);
        }
        let sec = match self.section_of(section) {
            Some(x) => x,
            None => return Err(XC_SECTION_SHORT),
        };
        let mut f = 0usize;
        while f < sec.fields.len() {
            if sec.fields[f].seq == seq {
                return self.resolve_ptr(&sec.fields[f].ptr);
            }
            f += 1;
        }
        Err(XC_DUP_FIELD)
    }

    pub fn section_of(&self, section: u8) -> Option<&SectionDraft> {  // 节查询（未提交 None）。
        let mut i = 0usize;
        while i < self.sections.len() {
            if self.sections[i].section == section {
                return Some(&self.sections[i]);
            }
            i += 1;
        }
        None
    }
}
pub fn gen_clean_draft(seed: u64) -> (Vec<(u8, Vec<u64>)>, Vec<SectionDraft>) {  // 样本发生器：六本账册（确定性内容）+ 六节全字段稿面（指针全可解引）。
    let mut s = seed;
    let mut ledgers: Vec<(u8, Vec<u64>)> = Vec::new();
    let mut l = 0u8;
    while l < SECTION_COUNT as u8 {
        let mut entries: Vec<u64> = Vec::new();
        let mut i = 0u32;
        while i < FIELDS_PER_SECTION as u32 {
            s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            entries.push(((s >> 33) as u64) | 0x1);
            i += 1;
        }
        ledgers.push((l, entries));
        l += 1;
    }
    let mut sections: Vec<SectionDraft> = Vec::new();
    let mut sec = 0u8;
    while sec < SECTION_COUNT as u8 {
        let mut fields: Vec<ReportField> = Vec::new();
        let mut f = 0u8;
        while f < FIELDS_PER_SECTION as u8 {
            fields.push(ReportField {
                seq: f,
                value: 0x6000_0000_0000_0000 | ((sec as u64) << 8) | f as u64,
                ptr: SourcePtr { ledger: sec, idx: f as u32 },
            });
            f += 1;
        }
        sections.push(SectionDraft { section: sec, fields });
        sec += 1;
    }
    (ledgers, sections)
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    let (ledgers, sections) = gen_clean_draft(0x6000_0000_D1A5_0001);
    // —— 主路径：六本账册 → 六节提交 → 节全断言 → 解引 → 定稿 ——
    let mut m = ReportDrafter::new();
    let mut li = 0usize;
    while li < ledgers.len() {
        m.register_ledger(ledgers[li].0, &ledgers[li].1)?;
        li += 1;
    }
    let mut si = 0usize;
    while si < sections.len() {
        m.submit_section(&sections[si])?;
        si += 1;
    }
    let (secs, flds) = m.sections_assert()?;
    if secs != 6 || flds != FIELD_TOTAL as u32 {
        return Err(XC_SECTION_SHORT);
    }
    let rep = m.bind_pointers()?;
    if rep.unresolved != 0 || rep.promille != 1000 {
        return Err(XC_RESOLVE_LOW);
    }
    let fin = m.finalize()?;
    if fin.sections_done != 6 || fin.fields_done != 36 || fin.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    if m.finalize() != Err(XC_ALREADY_FINAL) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：缺一节（5/6）——
    let mut m2 = ReportDrafter::new();
    let mut li2 = 0usize;
    while li2 < ledgers.len() {
        m2.register_ledger(ledgers[li2].0, &ledgers[li2].1)?;
        li2 += 1;
    }
    let mut si2 = 0usize;
    while si2 < sections.len() - 1 {
        m2.submit_section(&sections[si2])?;
        si2 += 1;
    }
    if m2.sections_assert() != Err(XC_SECTION_SHORT) {
        return Err(XC_SECTION_SHORT);
    }
    // —— 错误路径 2：指针解引失败 → 率 35/36 ≈ 972‰ … 需构造 <90%：坏 20% ——
    // 构造：Legacy 节 6 字段全部指到空账册下标 99 → 解引率 30/36=833‰ 红。
    let mut m3 = ReportDrafter::new();
    let mut li3 = 0usize;
    while li3 < ledgers.len() {
        m3.register_ledger(ledgers[li3].0, &ledgers[li3].1)?;
        li3 += 1;
    }
    let mut secs3 = sections.clone();
    let mut f3 = 0usize;
    while f3 < 6 {
        secs3[5].fields[f3].ptr.idx = 99;
        f3 += 1;
    }
    let mut si3 = 0usize;
    while si3 < secs3.len() {
        m3.submit_section(&secs3[si3])?;
        si3 += 1;
    }
    m3.sections_assert()?;
    let rep3 = m3.bind_pointers()?;
    if rep3.unresolved != 6 || rep3.promille != 833 {
        return Err(XC_RESOLVE_LOW);
    }
    if m3.finalize() != Err(XC_RESOLVE_LOW) {
        return Err(XC_RESOLVE_LOW);
    }
    // —— 错误路径 3：账册代号越界 / 节号越界 / 字段值零 / 重节 ——
    let mut m4 = ReportDrafter::new();
    if m4.register_ledger(9, &ledgers[0].1) != Err(XC_LEDGER_BAD) {
        return Err(XC_LEDGER_BAD);
    }
    let mut bad_sec = sections[0].clone();
    bad_sec.section = 9;
    if m4.submit_section(&bad_sec) != Err(XC_SECTION_SHORT) {
        return Err(XC_SECTION_SHORT);
    }
    let mut bad_val = sections[1].clone();
    bad_val.fields[2].value = 0;
    if m4.submit_section(&bad_val) != Err(XC_VALUE_ZERO) {
        return Err(XC_VALUE_ZERO);
    }
    let mut m5 = ReportDrafter::new();
    m5.register_ledger(0, &ledgers[0].1)?;
    m5.submit_section(&sections[0])?;
    if m5.submit_section(&sections[0]) != Err(XC_DUP_SECTION) {
        return Err(XC_DUP_SECTION);
    }
    // —— 错误路径 4：未绑指针即定稿 ——
    let mut m6 = ReportDrafter::new();
    let mut li6 = 0usize;
    while li6 < ledgers.len() {
        m6.register_ledger(ledgers[li6].0, &ledgers[li6].1)?;
        li6 += 1;
    }
    let mut si6 = 0usize;
    while si6 < sections.len() {
        m6.submit_section(&sections[si6])?;
        si6 += 1;
    }
    if m6.finalize() != Err(XC_NOT_BOUND) {
        return Err(XC_NOT_BOUND);
    }
    // —— 查询 API：节可解引、账长可查 ——
    match m.section_of(3) {
        Some(s) if s.fields.len() == FIELDS_PER_SECTION => {}
        _ => return Err(XC_SECTION_SHORT),
    }
    if m.ledger_len(5)? != FIELDS_PER_SECTION {
        return Err(XC_LEDGER_BAD);
    }
    if m.ledger_len(9) != Err(XC_LEDGER_BAD) {
        return Err(XC_LEDGER_BAD);
    }
    // —— 分节覆盖：六节各 6/6 全可解引 ——
    let cov = m.coverage_by_section()?;
    let mut c = 0usize;
    while c < SECTION_COUNT {
        if cov[c].0 != FIELDS_PER_SECTION as u32 || cov[c].1 != FIELDS_PER_SECTION as u32 {
            return Err(XC_COUNT_MISMATCH);
        }
        c += 1;
    }
    // —— 单字段解引：主稿全真；坏稿 Legacy 节全假；查无字段报重号 ——
    let mut f2 = 0u8;
    while f2 < 6 {
        if m.resolve_field(0, f2)? != true {
            return Err(XC_PTR_UNRESOLVED);
        }
        if m3.resolve_field(5, f2)? != false {
            return Err(XC_PTR_UNRESOLVED);
        }
        f2 += 1;
    }
    if m.resolve_field(0, 9) != Err(XC_DUP_FIELD) {
        return Err(XC_DUP_FIELD);
    }
    if m.resolve_field(9, 0) != Err(XC_SECTION_SHORT) {
        return Err(XC_SECTION_SHORT);
    }
    Ok(0)
}
