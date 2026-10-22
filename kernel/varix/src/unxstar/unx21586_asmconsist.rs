//! UNX-F21586 · 汇编册域节与批册一致性核
//!
//! 诊断段 0x91029~0x91030（F2-B40 域收官段 6/100）。
//!
//! 判据原文（施工书 UNX-F21586-J1）：域节 40 批段与批册一致（双检脚本）。
//!
//! 设计要点（为什么只有两个码，还偏要再设一个 DefectKind）：
//! 本单的码段只给两码：一致 / 不一致。这一位是给外层聚合器用的——它只需要知道
//! 「这一册能不能过」。但运维拿到一个「不一致」无从下手，所以粒度下沉到
//! `DefectKind`（八种病灶），码只表达判决、种类表达病灶。反过来做（八码）会逼
//! 聚合层理解本册内部语义，是分层倒挂。
//!
//! 解析层「坏行不清仓」是本册最关键的取舍：域节是手写汇编册，一行标题打错就让
//! 整册 40 段全废，会把「1 行瑕疵」放大成「整册不可用」，运维只能靠肉眼重找。
//! 故解析器把坏行单独清点（bad_header / bad_entry / unknown），好段照常入册，由
//! 检查层把「残渣计数」转成一条册级缺陷——既不拒收，也不隐瞒。
//!
//! 交接核（上段末条 +1 == 下段首条）刻意独立于「与批册对拍」：两侧同时抄错同一
//! 处时对拍全绿，但段间已断开。段序、交接、段数守恒三条都必须各自成立。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 域节批段数（B01~B40）。
pub const SECTION_BATCHES: u16 = 40;
/// 单段条目数（每段 20 条）。
pub const SECTION_WIDTH: u32 = 20;
/// 单段配额行数。
pub const SECTION_ROWS: u32 = 6_000;
/// 全册条目数（40 × 20）。
pub const SECTION_ITEMS: u16 = 800;
/// 域首条编号（F20801）。
pub const SECTION_FIRST_ID: u32 = 20_801;

/// 域节诊断码闭集（0x91029~0x91030）。
///
/// 只有两码是派工契约规定的：判决位给聚合器，病灶位给 `DefectKind`。码值写死
/// 判别式，插入变体会让码漂移，历史日志无法归因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsmCode {
    /// 0x91029 域节与批册一致（40 段 / 800 条 / 零残渣）。
    SectionOk = 0x91029,
    /// 0x91030 域节与批册存在不一致（病灶见 DefectKind）。
    SectionDrift = 0x91030,
}

impl AsmCode {
    /// 三要素之一：现象。
    pub const fn what(self) -> &'static str {
        match self {
            AsmCode::SectionOk => "汇编册域节与批册逐段一致",
            AsmCode::SectionDrift => "汇编册域节与批册存在不一致",
        }
    }
    /// 三要素之二：原因。
    pub const fn why(self) -> &'static str {
        match self {
            AsmCode::SectionOk => "四十段齐、条目守恒、段间交接连续",
            AsmCode::SectionDrift => "标题坏行、区间漂移、条目数不符或段序倒置",
        }
    }
    /// 三要素之三：下一步。
    pub const fn advice(self) -> &'static str {
        match self {
            AsmCode::SectionOk => "转入域守恒终声明",
            AsmCode::SectionDrift => "按 DefectKind 与批号逐条修，修完重跑双检",
        }
    }
    /// 数值形式。返回 u32：五位数码（0x91029）超 u16 值域，`as u16` 会静默截断成
    /// 0x1029 与低段码撞车——截断后的码在日志里长得像「另一个模块的故障」。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 病灶种类（判决只有一位，病灶必须细分，否则码表里全是同一个 0x91030）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefectKind {
    /// 批册登记了某批，域节里没有对应段。
    MissingSegment,
    /// 域节有某段，批册没有登记该批。
    ExtraSegment,
    /// 标题行解析失败（该段未能入册）。
    HeaderMalformed,
    /// 条目行解析失败（该条未能计数）。
    EntryMalformed,
    /// 段区间（首条/末条/配额）与批册不符。
    SpanDrift,
    /// 段内条目数与区间宽度不符。
    CountDrift,
    /// 上段末条 +1 ≠ 下段首条。
    HandoverBreak,
    /// 段序非按批号递增。
    OrderInverted,
}

impl DefectKind {
    /// 病灶名（报告与判据断言用的稳定标签）。
    pub const fn label(self) -> &'static str {
        match self {
            DefectKind::MissingSegment => "missing",
            DefectKind::ExtraSegment => "extra",
            DefectKind::HeaderMalformed => "bad-header",
            DefectKind::EntryMalformed => "bad-entry",
            DefectKind::SpanDrift => "span",
            DefectKind::CountDrift => "count",
            DefectKind::HandoverBreak => "handover",
            DefectKind::OrderInverted => "order",
        }
    }
}

/// 一条缺陷：病灶 + 批号 + 一句话证据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Defect {
    pub kind: DefectKind,
    /// 相关批号（0 表示册级，如标题残渣）。
    pub batch: u16,
    /// 证据（静态串，避免为了诊断文本做分配）。
    pub note: &'static str,
}

impl Defect {
    /// 归属的诊断码：病灶一律落在 0x91030 之下，判决位只有一位。
    pub const fn code(&self) -> u32 {
        AsmCode::SectionDrift.code()
    }
}

/// 解析出的一个批段。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Segment {
    pub batch: u16,
    pub first: u32,
    pub last: u32,
    pub quota: u32,
    /// 本段成功入册的条目行数（不含坏行）。
    pub entries: u16,
}

impl Segment {
    /// 区间宽度。起止倒置返回 0：绕回的正数会在守恒求和里补平别处缺额。
    pub const fn width(&self) -> u32 {
        if self.last < self.first { 0 } else { self.last - self.first + 1 }
    }
    /// 段内自洽：条目数与宽度相符。
    pub const fn count_holds(&self) -> bool {
        self.entries as u32 == self.width()
    }
}

/// 解析残渣计数。
///
/// 单独清点是「坏行不清仓」的记账面：坏行不阻断解析，但也绝不能悄悄消失——每
/// 一类各一个计数，报告里如实列出，检查层再把非零计数转成册级缺陷。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ParseScrap {
    pub blank: u16,
    pub comment: u16,
    pub bad_header: u16,
    pub bad_entry: u16,
    pub unknown: u16,
}

impl ParseScrap {
    /// 真残渣（不含空行与注释：它们是合法排版）。
    pub const fn faults(&self) -> u16 {
        self.bad_header + self.bad_entry + self.unknown
    }
    /// 摘要（报告一行）。顺序固定便于 diff。
    pub fn summary(&self) -> String {
        let mut s = String::with_capacity(64);
        s.push_str("scrap blank=");
        s.push_str(&self.blank.to_string());
        s.push_str(" comment=");
        s.push_str(&self.comment.to_string());
        s.push_str(" bad_header=");
        s.push_str(&self.bad_header.to_string());
        s.push_str(" bad_entry=");
        s.push_str(&self.bad_entry.to_string());
        s.push_str(" unknown=");
        s.push_str(&self.unknown.to_string());
        s
    }
}

/// 整册解析结果。
pub struct SectionBook {
    pub segments: Vec<Segment>,
    pub scrap: ParseScrap,
    /// 全册入册条目数。
    pub entries: u16,
}

impl SectionBook {
    /// 段数。
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }
    /// 段数守恒 + 条目守恒（40 段 × 20 条 = 800）。
    pub fn conserved(&self) -> bool {
        self.segments.len() == SECTION_BATCHES as usize
            && self.entries == SECTION_ITEMS
            && self.scrap.faults() == 0
    }
    /// 批号序是否严格递增。
    pub fn order_holds(&self) -> bool {
        for i in 1..self.segments.len() {
            if self.segments[i].batch <= self.segments[i - 1].batch {
                return false;
            }
        }
        true
    }
    /// 解析指纹：逐段逐字段 + 残渣计数，用来判「同一册解析两次是否同构」。
    pub fn fingerprint(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        h = fnv_u64(h, self.segments.len() as u64);
        for s in &self.segments {
            h = fnv_u64(h, s.batch as u64);
            h = fnv_u64(h, s.first as u64);
            h = fnv_u64(h, s.last as u64);
            h = fnv_u64(h, s.quota as u64);
            h = fnv_u64(h, s.entries as u64);
        }
        h = fnv_u64(h, self.entries as u64);
        h = fnv_u64(h, self.scrap.bad_header as u64);
        h = fnv_u64(h, self.scrap.bad_entry as u64);
        h = fnv_u64(h, self.scrap.unknown as u64);
        h
    }
}

/// 批册的一行（批号即数组下标，不再重复存）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RegRow {
    pub first: u32,
    pub last: u32,
    pub quota: u32,
}

/// 批册：定长 40 槽的稠密数组。
///
/// 为什么不用 `Vec` 顺序表：批册以批号为键，顺序表定位要 O(n) 扫，且「缺登记」
/// 只能靠长度差间接发现；稠密数组一个 `None` 就是一处缺登，缺陷直接被数据结构
/// 表达出来，判定代码不必再推。
pub struct Registry {
    slots: [Option<RegRow>; SECTION_BATCHES as usize],
}

impl Registry {
    /// 空册。
    pub const fn new() -> Registry {
        Registry { slots: [None; SECTION_BATCHES as usize] }
    }
    /// 登记一批（批号越域返回 false，不静默丢弃）。
    pub fn put(&mut self, batch: u16, row: RegRow) -> bool {
        if batch < 1 || batch > SECTION_BATCHES {
            return false;
        }
        self.slots[(batch - 1) as usize] = Some(row);
        true
    }
    /// 取某批登记。
    pub fn get(&self, batch: u16) -> Option<RegRow> {
        if batch < 1 || batch > SECTION_BATCHES {
            return None;
        }
        self.slots[(batch - 1) as usize]
    }
    /// 已登记批数。
    pub fn filled(&self) -> u16 {
        let mut n = 0u16;
        for s in &self.slots {
            if s.is_some() {
                n += 1;
            }
        }
        n
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// 规格行：生成正文与批册的共同源头。
///
/// 正文与批册同源看着像「自己验自己」，其实是必需的：本册要核的是「文本解析结
/// 果 vs 登记表」两条**路径**是否一致，同源只保证起点相同，渲染（排版/条目行）
/// 与解析（逐行识别）仍是两条独立实现。若要造两侧同错的样本，也必须能同时改
/// 两侧——这正是同源规格的用处。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SpanRow {
    pub batch: u16,
    pub first: u32,
    pub last: u32,
    pub quota: u32,
}

/// 域节规格。
pub struct BookSpec {
    pub rows: Vec<SpanRow>,
}

/// 标准规格（40 批，每批 20 条，批间连续）。
pub fn standard_spec() -> Result<BookSpec, AsmFault> {
    let mut rows = Vec::with_capacity(SECTION_BATCHES as usize);
    let mut b: u16 = 1;
    while b <= SECTION_BATCHES {
        let first = SECTION_FIRST_ID + (b as u32 - 1) * SECTION_WIDTH;
        let last = first + SECTION_WIDTH - 1;
        if last > 21_600 {
            return Err(AsmFault { code: AsmCode::SectionDrift.code(), batch: b });
        }
        rows.push(SpanRow { batch: b, first, last, quota: SECTION_ROWS });
        b += 1;
    }
    Ok(BookSpec { rows })
}

/// 构造期故障。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AsmFault {
    pub code: u32,
    pub batch: u16,
}

/// 把规格渲染成域节正文（标题行 + 逐条条目行）。
pub fn render_spec(spec: &BookSpec) -> String {
    let mut s = String::with_capacity(16 * 1024);
    s.push_str("# F2 域节（40 批段，每段 20 条）\n");
    for r in &spec.rows {
        s.push_str("## B");
        if r.batch < 10 {
            s.push('0');
        }
        s.push_str(&r.batch.to_string());
        s.push_str(" F");
        s.push_str(&r.first.to_string());
        s.push_str("-F");
        s.push_str(&r.last.to_string());
        s.push_str(" rows=");
        s.push_str(&r.quota.to_string());
        s.push('\n');
        let mut id = r.first;
        while id <= r.last {
            s.push_str("- F");
            s.push_str(&id.to_string());
            s.push_str(" 条目\n");
            id += 1;
        }
    }
    s
}

/// 从规格生成批册。
pub fn registry_from_spec(spec: &BookSpec) -> Registry {
    let mut reg = Registry::new();
    for r in &spec.rows {
        let _ = reg.put(r.batch, RegRow { first: r.first, last: r.last, quota: r.quota });
    }
    reg
}

/// 解析十进制无符号数（拒绝空串、非数字、溢出；不 panic 不绕回）。
fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 10 {
        return None;
    }
    let mut v: u32 = 0;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(v)
}

/// 解析条目编号（必须 `F` + 五位数字，短号视为坏行：F2081 与 F20801 混用会让
/// 区间判定在两种口径之间漂移，宁可直接拒收）。
fn parse_item_id(s: &str) -> Option<u32> {
    if s.len() != 6 {
        return None;
    }
    parse_u32(s.strip_prefix('F')?)
}

/// 解析标题行：`## B<nn> F<id>-F<id> rows=<quota>`。
fn parse_header(line: &str) -> Option<Segment> {
    let tk: Vec<&str> = line.split_ascii_whitespace().collect();
    if tk.len() != 4 || tk[0] != "##" {
        return None;
    }
    let batch = parse_u32(tk[1].strip_prefix('B')?)?;
    // 只挡三位以上批号（那必然是手误），1..=40 之外的批号保留入册——否则「批册
    // 多出未登记的段」这一类缺陷永远不可达，ExtraSegment 成了死码。
    if batch < 1 || batch > 99 {
        return None;
    }
    let (a, b) = tk[2].split_once('-')?;
    let first = parse_item_id(a)?;
    let last = parse_item_id(b)?;
    let quota = parse_u32(tk[3].strip_prefix("rows=")?)?;
    if first > last || quota == 0 {
        return None;
    }
    Some(Segment { batch: batch as u16, first, last, quota, entries: 0 })
}

/// 解析条目行：`- F<id> <说明>`。
fn parse_entry(line: &str) -> Option<u32> {
    let mut it = line.split_ascii_whitespace();
    if it.next()? != "-" {
        return None;
    }
    parse_item_id(it.next()?)
}

/// 逐行解析域节。
///
/// 判定顺序刻意按「行首前缀」分派而不是先试解析：`##` 开头的行哪怕内容全错也
/// 必须记成 bad_header，否则它会被当成未知行计入另一类，残渣分类就对不上排版
/// 事实（这两类要分别指导两种修法）。
pub fn parse_section(text: &str) -> SectionBook {
    let mut segments: Vec<Segment> = Vec::new();
    let mut scrap = ParseScrap::default();
    let mut entries: u16 = 0;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            scrap.blank += 1;
            continue;
        }
        if line.starts_with("##") {
            match parse_header(line) {
                Some(seg) => segments.push(seg),
                None => scrap.bad_header += 1,
            }
            continue;
        }
        if line.starts_with('#') || line.starts_with(';') {
            scrap.comment += 1;
            continue;
        }
        if line.starts_with('-') {
            match parse_entry(line) {
                Some(_id) => match segments.last_mut() {
                    // 条目归属上一段：域节没有「跨段条目」，条目行必须挂在段下。
                    Some(seg) => {
                        seg.entries = seg.entries.saturating_add(1);
                        entries = entries.saturating_add(1);
                    }
                    None => scrap.bad_entry += 1,
                },
                None => scrap.bad_entry += 1,
            }
            continue;
        }
        scrap.unknown += 1;
    }
    SectionBook { segments, scrap, entries }
}

/// 一致性结果。
pub struct Consistency {
    pub defects: Vec<Defect>,
    pub segments: u16,
    pub entries: u16,
    /// 与批册对上号的段数。
    pub compared: u16,
    /// 检查过的段间交接数。
    pub handovers: u16,
}

impl Consistency {
    /// 判决码（0x91029 / 0x91030）。
    pub fn code(&self) -> u32 {
        if self.is_ok() { AsmCode::SectionOk.code() } else { AsmCode::SectionDrift.code() }
    }
    /// 是否可放行：零缺陷、40 段、800 条、40 段全对上批册。
    pub fn is_ok(&self) -> bool {
        self.defects.is_empty()
            && self.segments == SECTION_BATCHES
            && self.entries == SECTION_ITEMS
            && self.compared == SECTION_BATCHES
    }
    /// 首要病灶（无缺陷返回 None）。
    pub fn first(&self) -> Option<Defect> {
        self.defects.first().copied()
    }
    /// 某类病灶的条数。
    pub fn count_of(&self, kind: DefectKind) -> usize {
        let mut n = 0usize;
        for d in &self.defects {
            if d.kind == kind {
                n += 1;
            }
        }
        n
    }
    /// 首个指定病灶的批号（用于断言「差异定位到批号」）。
    pub fn batch_of(&self, kind: DefectKind) -> u16 {
        for d in &self.defects {
            if d.kind == kind {
                return d.batch;
            }
        }
        0
    }
    /// 报告一行：判决 + 段数/条目数 + 各类病灶计数。
    pub fn summary(&self) -> String {
        let mut s = String::with_capacity(128);
        s.push_str("section code=0x");
        s.push_str(&hex32(self.code()));
        s.push_str(" seg=");
        s.push_str(&self.segments.to_string());
        s.push_str(" entries=");
        s.push_str(&self.entries.to_string());
        s.push_str(" compared=");
        s.push_str(&self.compared.to_string());
        s.push_str(" handovers=");
        s.push_str(&self.handovers.to_string());
        for k in [
            DefectKind::MissingSegment,
            DefectKind::ExtraSegment,
            DefectKind::HeaderMalformed,
            DefectKind::EntryMalformed,
            DefectKind::SpanDrift,
            DefectKind::CountDrift,
            DefectKind::HandoverBreak,
            DefectKind::OrderInverted,
        ] {
            s.push(' ');
            s.push_str(k.label());
            s.push('=');
            s.push_str(&self.count_of(k).to_string());
        }
        s
    }
}

/// 段与批册逐段对拍 + 段间交接核。
///
/// 检查顺序固定且不可交换：段序 → 段内 → 段间 → 缺登 → 残渣。段序错时段的「上
/// 一段」本身就是错的，先查交接会报出一串伪断裂；残渣放最后，因为它不指向具体
/// 段，先报会把首要码钉在册级，掩盖真正能定位的段级缺陷。
pub fn check_book(book: &SectionBook, reg: &Registry) -> Consistency {
    let mut defects: Vec<Defect> = Vec::new();
    let mut compared: u16 = 0;
    let mut handovers: u16 = 0;

    for i in 0..book.segments.len() {
        let seg = book.segments[i];
        if i > 0 && seg.batch <= book.segments[i - 1].batch {
            defects.push(Defect { kind: DefectKind::OrderInverted, batch: seg.batch, note: "段序未递增" });
        }
        match reg.get(seg.batch) {
            Some(row) => {
                compared += 1;
                if row.first != seg.first || row.last != seg.last || row.quota != seg.quota {
                    defects.push(Defect { kind: DefectKind::SpanDrift, batch: seg.batch, note: "区间或配额与批册不符" });
                }
            }
            None => defects.push(Defect {
                kind: DefectKind::ExtraSegment,
                batch: seg.batch,
                note: "批册未登记该批",
            }),
        }
        if !seg.count_holds() {
            defects.push(Defect { kind: DefectKind::CountDrift, batch: seg.batch, note: "条目数与区间宽度不符" });
        }
        if i > 0 {
            let prev = book.segments[i - 1];
            // 只在批号本身连续时核交接：批号断了是「缺段」，再报断裂是同一根因两报。
            if seg.batch == prev.batch + 1 {
                handovers += 1;
                if seg.first != prev.last.saturating_add(1) {
                    defects.push(Defect { kind: DefectKind::HandoverBreak, batch: seg.batch, note: "上段末条+1≠本段首条" });
                }
            }
        }
    }

    // 缺登：批册有、域节无。按批号升序报，保证报告可重放。
    let mut b: u16 = 1;
    while b <= SECTION_BATCHES {
        if reg.get(b).is_some() {
            let mut seen = false;
            for seg in &book.segments {
                if seg.batch == b {
                    seen = true;
                    break;
                }
            }
            if !seen {
                defects.push(Defect { kind: DefectKind::MissingSegment, batch: b, note: "域节缺该批段" });
            }
        }
        b += 1;
    }

    if book.scrap.bad_header > 0 {
        defects.push(Defect { kind: DefectKind::HeaderMalformed, batch: 0, note: "有标题行未能解析" });
    }
    if book.scrap.bad_entry > 0 {
        defects.push(Defect { kind: DefectKind::EntryMalformed, batch: 0, note: "有条目行未能解析" });
    }
    if book.scrap.unknown > 0 {
        defects.push(Defect { kind: DefectKind::EntryMalformed, batch: 0, note: "有无法归类的行" });
    }

    Consistency { defects, segments: book.segments.len() as u16, entries: book.entries, compared, handovers }
}

/// FNV-1a 64 单步（写死质数与偏移基，来源：FNV 规范 1a 变体）。
fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        h ^= ((v >> (i * 8)) & 0xFF) as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 24 位小写十六进制（no_std 手写，避免格式化分配）。
fn hex32(v: u32) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(6);
    let mut shift = 20i32;
    while shift >= 0 {
        s.push(HEX[((v >> shift) & 0xF) as usize] as char);
        shift -= 4;
    }
    s
}

/// 判据入口（UNX-F21586-J1 及其配套）。判据常量独立写死：40 / 800 / 0x91030 等
/// 直接以字面量出现，不复用被测常量。
pub fn run_unx21586_asmconsist_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21586");
    let spec = standard_spec();
    let text = match &spec {
        Ok(s) => render_spec(s),
        Err(_) => String::new(),
    };
    let book = parse_section(&text);

    // 1) 夹具可构造：40 段、800 条、零残渣，批册 40 行。
    let fixture_ok = match &spec {
        Ok(s) => {
            let reg = registry_from_spec(s);
            book.segment_count() == 40
                && book.entries == 800
                && book.scrap.faults() == 0
                && reg.filled() == 40
                && text.len() > 8_000
        }
        Err(_) => false,
    };
    cs.add("f21586.fixture_buildable", fixture_ok, "域节正文或批册构造失败（应 40 段/800 条/零残渣）");

    // 2) 正向：判决 0x91029、零缺陷、40 段全对上、39 处交接、守恒成立。
    if let Ok(s) = &spec {
        let reg = registry_from_spec(s);
        let c = check_book(&book, &reg);
        cs.add("f21586.clean_consistent", c.code() == 0x91029 && c.defects.is_empty() && c.compared == 40 && c.handovers == 39 && book.conserved(), "干净册未判一致，或段数/交接数/守恒不成立");
    } else {
        cs.add("f21586.clean_consistent", false, "夹具不可用");
    }

    // 3) 容错·空行与注释：合法排版不得计入残渣，也不得影响判决。
    {
        let padded = format!("\n# 头注\n\n{}\n; 尾注\n\n", text);
        let b = parse_section(&padded);
        let reg = match &spec { Ok(s) => registry_from_spec(s), Err(_) => Registry::new() };
        let c = check_book(&b, &reg);
        cs.add("f21586.blank_and_comment_tolerated", b.scrap.blank >= 3 && b.scrap.comment == 3 && b.scrap.faults() == 0 && c.code() == 0x91029 && b.segment_count() == 40 && b.scrap.summary().contains("bad_header=0"), "空行/注释被误计残渣，或影响了判决");
    }

    // 4) 反序·标题坏行：坏 1 行只丢 1 段，其余 39 段照常入册（不整册拒收）。
    {
        let broken = text.replace("## B05 F20881-F20900 rows=6000", "## B05 F20881..F20900 rows=6000");
        let b = parse_section(&broken);
        let reg = match &spec { Ok(s) => registry_from_spec(s), Err(_) => Registry::new() };
        let c = check_book(&b, &reg);
        cs.add("f21586.header_malformed", b.scrap.bad_header == 1 && b.segments.len() == 39 && c.code() == 0x91030 && c.count_of(DefectKind::HeaderMalformed) == 1, "标题坏行未被单独清点，或整册被拒收");
    }

    // 5) 反序·条目坏行：条目号少一位 → bad_entry=1，该段条目差 1（CountDrift）。
    {
        let broken = text.replace("- F20901 ", "- 20901 ");
        let b = parse_section(&broken);
        let reg = match &spec { Ok(s) => registry_from_spec(s), Err(_) => Registry::new() };
        let c = check_book(&b, &reg);
        let scrap_code = c.first().map_or(0, |d| d.code());
        cs.add("f21586.entry_malformed", b.scrap.bad_entry == 1 && b.entries == 799 && c.code() == 0x91030 && c.count_of(DefectKind::EntryMalformed) == 1 && scrap_code == 0x91030, "条目坏行未被清点，或残渣未转成册级缺陷");
    }

    // 6) 反序·区间漂移：正文把 B12 末条改成 21241 → 与批册不符（0x91030/span/12）。
    {
        let broken = text.replace("## B12 F21021-F21040", "## B12 F21021-F21041");
        let b = parse_section(&broken);
        let reg = match &spec { Ok(s) => registry_from_spec(s), Err(_) => Registry::new() };
        let c = check_book(&b, &reg);
        cs.add("f21586.span_drift", c.code() == 0x91030 && c.batch_of(DefectKind::SpanDrift) == 12 && c.count_of(DefectKind::MissingSegment) == 0, "区间漂移未被判 0x91030，或批号定位错");
    }

    // 7) 反序·条目数漂移：删掉 B07 一条条目行 → 19 ≠ 宽度 20。
    {
        let broken = text.replace("- F20921 条目\n", "");
        let b = parse_section(&broken);
        let reg = match &spec { Ok(s) => registry_from_spec(s), Err(_) => Registry::new() };
        let c = check_book(&b, &reg);
        cs.add("f21586.count_drift", b.entries == 799 && c.batch_of(DefectKind::CountDrift) == 7 && c.count_of(DefectKind::SpanDrift) == 0, "段内条目数与宽度不符未报 CountDrift，或批号定位错");
    }

    // 8) 反序·缺段：规格删掉 B33 → 正文无该段，批册仍登记 → MissingSegment@33。
    {
        let mut s = match standard_spec() { Ok(s) => s, Err(_) => BookSpec { rows: Vec::new() } };
        let full_reg = match &spec { Ok(x) => registry_from_spec(x), Err(_) => Registry::new() };
        let ok = s.rows.len() == 40;
        s.rows.remove(32);
        let b = parse_section(&render_spec(&s));
        let c = check_book(&b, &full_reg);
        cs.add("f21586.missing_segment", ok && b.segments.len() == 39 && c.batch_of(DefectKind::MissingSegment) == 33 && c.code() == 0x91030, "缺段未被判 MissingSegment@33，或段数不是 39");
    }

    // 9) 反序·多段（上限 +1）：规格追加 B41 → ExtraSegment@41。
    {
        let mut s = match standard_spec() { Ok(s) => s, Err(_) => BookSpec { rows: Vec::new() } };
        let full_reg = match &spec { Ok(x) => registry_from_spec(x), Err(_) => Registry::new() };
        s.rows.push(SpanRow { batch: 41, first: 21_601, last: 21_620, quota: 6_000 });
        let b = parse_section(&render_spec(&s));
        let c = check_book(&b, &full_reg);
        cs.add("f21586.extra_segment", b.segments.len() == 41 && c.batch_of(DefectKind::ExtraSegment) == 41 && c.code() == 0x91030, "41 段未被判 ExtraSegment@41");
    }

    // 10) 反序·段序倒置：规格里交换 B03/B04 两行（区间随行走，故无区间漂移）。
    {
        let mut s = match standard_spec() { Ok(s) => s, Err(_) => BookSpec { rows: Vec::new() } };
        let full_reg = match &spec { Ok(x) => registry_from_spec(x), Err(_) => Registry::new() };
        s.rows.swap(2, 3);
        let b = parse_section(&render_spec(&s));
        let c = check_book(&b, &full_reg);
        cs.add("f21586.order_inverted", !b.order_holds() && c.count_of(DefectKind::OrderInverted) == 1 && c.count_of(DefectKind::SpanDrift) == 0 && c.code() == 0x91030, "换段未被判 OrderInverted，或误报区间漂移");
    }

    // 11) 反序·交接断裂（两侧同错）：B13 整段后移一位，正文与批册同源，区间逐段
    //     都对得上，唯 B12 末条 +1 ≠ B13 首条 → 只有交接核看得见。
    {
        let mut s = match standard_spec() { Ok(s) => s, Err(_) => BookSpec { rows: Vec::new() } };
        s.rows[12].first += 1;
        s.rows[12].last += 1;
        let reg = registry_from_spec(&s);
        let b = parse_section(&render_spec(&s));
        let c = check_book(&b, &reg);
        // 平移一段必然产生两处断口（进、出各一），故断言 2 处而首处落在批 13。
        cs.add("f21586.handover_break", c.count_of(DefectKind::HandoverBreak) == 2 && c.batch_of(DefectKind::HandoverBreak) == 13 && c.count_of(DefectKind::SpanDrift) == 0, "两侧同错的交接断裂未被抓到（应 2 处、首处批 13、零区间漂移）");
    }

    // 12) 边界·空册：0 段 0 条，不 panic；批册 40 行全成缺段。
    {
        let b = parse_section("");
        let reg = match &spec { Ok(s) => registry_from_spec(s), Err(_) => Registry::new() };
        let c = check_book(&b, &reg);
        cs.add("f21586.empty_book", b.segments.is_empty() && b.entries == 0 && c.count_of(DefectKind::MissingSegment) == 40 && c.code() == 0x91030 && !b.conserved(), "空册未被判 40 处缺段，或被误判为一致");
    }

    // 13) 边界·单段：只保留 B01 一段 20 条 → 1 段入册、39 处缺段、交接 0。
    {
        let mut s = match standard_spec() { Ok(s) => s, Err(_) => BookSpec { rows: Vec::new() } };
        let full_reg = match &spec { Ok(x) => registry_from_spec(x), Err(_) => Registry::new() };
        s.rows.truncate(1);
        let b = parse_section(&render_spec(&s));
        let c = check_book(&b, &full_reg);
        cs.add("f21586.single_segment", b.segments.len() == 1 && b.entries == 20 && c.handovers == 0 && c.count_of(DefectKind::MissingSegment) == 39, "单段册的段数/交接数/缺段数不符");
    }

    // 14) 幂等·确定性：同一正文解析两次，指纹、段数、缺陷数、判决逐位相同。
    if let Ok(s) = &spec {
        let reg = registry_from_spec(s);
        let a = parse_section(&text);
        let b = parse_section(&text);
        let ca = check_book(&a, &reg);
        let cb = check_book(&b, &reg);
        cs.add("f21586.parse_idempotent", a.fingerprint() == b.fingerprint() && ca.defects.len() == cb.defects.len() && ca.code() == cb.code() && ca.summary() == cb.summary(), "两次解析结果不一致（指纹/缺陷/摘要漂移）");
    } else {
        cs.add("f21586.parse_idempotent", false, "夹具不可用");
    }

    cs
}
