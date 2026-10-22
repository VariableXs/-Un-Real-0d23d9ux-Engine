//! UNX-F21585 · 总纲 §7.3-F2 与批册一致性核
//!
//! 诊断段 0x91021~0x91028（F2-B40 域收官段 5/100）。
//!
//! 判据原文（施工书 UNX-F21585-J1）：40 行表与批册逐批一致（双检脚本）。
//!
//! 设计要点（为什么不是「两个 Vec 相等就绿」）：
//! 逐下标比对的谎报率极高。批册把 B03、B04 两行写反时，逐下标比对会把差异报成
//! 「B03 的起止条目漂移」，运维照此去改区间，改完仍红。所以比对拆成三道互不替代
//! 的闸，每道只回答一类问题：存在性（缺/多/重）、值相等（某列漂移）、顺序（换
//! 位）。前闸不放行后闸不启动——缺批的批号根本没有值可比，先比只会把真缺陷埋在
//! 几十条伪漂移里。
//!
//! 第四类「结构断裂」刻意不走对拍而走两侧自查：两侧同时抄错同一区间时对拍全绿，
//! 但上批末条 +1 ≠ 下批首条。这类缺陷只有结构 lint 看得见，故单独一码、排在最后。
//!
//! 空表对齐是真空真：两侧都空时差异集为空，但「一致」毫无内容。故判决三分（一致
//! / 真空 / 分歧），真空不许混进绿。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 域内批数（B01~B40，来源：施工书 F2 域 §7.3）。
pub const BATCH_COUNT: u16 = 40;
/// 单批条目数（每批 20 条）。
pub const ITEMS_PER_BATCH: u32 = 20;
/// 单批配额行数（每批 6,000 行）。
pub const ROWS_PER_BATCH: u32 = 6_000;
/// 域首条编号（F20801）。
pub const FIRST_ITEM_ID: u32 = 20_801;
/// 域末条编号（F21600）。
pub const LAST_ITEM_ID: u32 = 21_600;
/// 值列数：起条 / 止条 / 配额（批号只在顺序闸里比，不计入值列）。
pub const ROW_COLUMNS: u32 = 3;

/// 一致性核诊断码闭集（0x91021~0x91028），同时充当差异种类。
///
/// 显式判别式而非隐式递增：码值是与外部日志对账的键，插入变体会让后续码整体漂
/// 移，历史归档无法归因。种类与码合成一个枚举，是因为它们本来就一一对应——分成
/// 两套再写一处映射，等于给自己留一个「映射漏改」的缺口。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignCode {
    /// 0x91021 两表逐批逐列一致。
    Aligned = 0x91021,
    /// 0x91022 批册缺批（总纲有、批册无）。
    MissingBatch = 0x91022,
    /// 0x91023 批册多批（批册有、总纲无）。
    ExtraBatch = 0x91023,
    /// 0x91024 某批某列值漂移。
    ColumnDrift = 0x91024,
    /// 0x91025 批序错位（集合相同、次序不同）。
    OrderSkew = 0x91025,
    /// 0x91026 列值越界（配额为 0、起止倒置、批号越域）。
    ColumnOutOfRange = 0x91026,
    /// 0x91027 区间断裂（上批末条 +1 ≠ 下批首条）。
    SpanBreak = 0x91027,
    /// 0x91028 同批重复登记。
    DuplicateBatch = 0x91028,
}

impl AlignCode {
    /// 三要素之一：现象。
    pub const fn what(self) -> &'static str {
        match self {
            AlignCode::Aligned => "总纲与批册逐批逐列一致",
            AlignCode::MissingBatch => "批册缺少总纲列出的批",
            AlignCode::ExtraBatch => "批册列有总纲未定义的批",
            AlignCode::ColumnDrift => "同批某列两侧取值不同",
            AlignCode::OrderSkew => "批号集合相同但排列次序不同",
            AlignCode::ColumnOutOfRange => "列值落在合法域之外",
            AlignCode::SpanBreak => "相邻两批条目区间不接续",
            AlignCode::DuplicateBatch => "同一批号被登记两次",
        }
    }
    /// 三要素之二：原因。
    pub const fn why(self) -> &'static str {
        match self {
            AlignCode::Aligned => "两侧投影同源同序，三道闸全过",
            AlignCode::MissingBatch => "登记时漏抄整行，或该批在总纲侧被删除",
            AlignCode::ExtraBatch => "批册残留了总纲已裁掉的批，或批号超域",
            AlignCode::ColumnDrift => "两侧各自手抄，其中一侧的数字被改过",
            AlignCode::OrderSkew => "插入批槽时按批号排序失败或中途换位",
            AlignCode::ColumnOutOfRange => "配额未填、起止写反、批号越出 1..40",
            AlignCode::SpanBreak => "区间首尾各自独立推导，未按上批末条续写",
            AlignCode::DuplicateBatch => "同一批被两条插入路径各写了一次",
        }
    }
    /// 三要素之三：下一步。
    pub const fn advice(self) -> &'static str {
        match self {
            AlignCode::Aligned => "转入汇编册域节一致性核",
            AlignCode::MissingBatch => "按批号补登该行，补齐后重跑双检",
            AlignCode::ExtraBatch => "确认该批是否已裁撤，裁撤则从批册删除",
            AlignCode::ColumnDrift => "以总纲为权威值回填批册，勿反向改总纲",
            AlignCode::OrderSkew => "按批号重排批册，勿改列值",
            AlignCode::ColumnOutOfRange => "补齐配额并令 first<=last<=F21600",
            AlignCode::SpanBreak => "以上批末条 +1 为下批首条重算区间",
            AlignCode::DuplicateBatch => "保留首条登记，删除重复行后重跑",
        }
    }
    /// 摘要文本里的列名（七项计数按此顺序输出）。
    pub const fn label(self) -> &'static str {
        match self {
            AlignCode::Aligned => "ok",
            AlignCode::MissingBatch => "missing",
            AlignCode::ExtraBatch => "extra",
            AlignCode::ColumnDrift => "drift",
            AlignCode::OrderSkew => "skew",
            AlignCode::ColumnOutOfRange => "oor",
            AlignCode::SpanBreak => "break",
            AlignCode::DuplicateBatch => "dup",
        }
    }
    /// 数值形式。返回 u32：五位数码（0x91021）超 u16 值域，`as u16` 会静默截断成
    /// 0x1021 与低段码撞车——截断后的码在日志里长得像「另一个模块的故障」。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 表里的一行：单批的结构化投影。两侧共用行结构是刻意的——列集不同就无法逐列
/// 对拍，只能退回「字符串相等」这种既报不出列名、也定位不到批的粗判。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Row {
    pub batch: u16,
    pub first: u32,
    pub last: u32,
    pub quota: u32,
}

impl Row {
    /// 构造一行（校验集中在 `is_well_formed`）。
    pub const fn new(batch: u16, first: u32, last: u32, quota: u32) -> Row {
        Row { batch, first, last, quota }
    }
    /// 区间宽度。起止倒置返回 0 而不绕回：u32 绕回得巨大正数，恰好能在段数守恒
    /// 求和里补平别处缺额，把倒置行伪装成正常行。
    pub const fn width(&self) -> u32 {
        if self.last < self.first { 0 } else { self.last - self.first + 1 }
    }
    /// 行自洽：批号在域内、区间正序、配额非零、编号不越域。
    pub const fn is_well_formed(&self) -> bool {
        self.batch >= 1 && self.batch <= BATCH_COUNT && self.quota > 0
            && self.first >= FIRST_ITEM_ID && self.first <= self.last && self.last <= LAST_ITEM_ID
    }
}

/// 差异落在哪一侧。定位不到侧的差异对运维没有价值（不知道该改哪本账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Outline,
    Register,
    Both,
}

/// 差异列名。「批号序」也算一列：顺序错位若不落到列上，运维只看到「顺序不对」，
/// 不知道是拿哪一列比出来的结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Column {
    Batch,
    SpanStart,
    SpanEnd,
    Quota,
}

impl Column {
    /// 列名（差异文本与判据断言用的稳定标签）。
    pub const fn label(self) -> &'static str {
        match self {
            Column::Batch => "batch",
            Column::SpanStart => "first",
            Column::SpanEnd => "last",
            Column::Quota => "quota",
        }
    }
}

/// 一条差异：种类 + 定位（批号、列、侧）+ 两侧取值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Diff {
    /// 差异种类（码即种类）。
    pub kind: AlignCode,
    /// 相关批号（0 表示与具体批无关）。
    pub batch: u16,
    pub column: Column,
    pub side: Side,
    /// 左侧（总纲）取值。
    pub left: u32,
    /// 右侧（批册）取值。
    pub right: u32,
}

impl Diff {
    /// 定位串，形如 `register/B09/quota`。批号补前导零是刻意的：字符串序下 B9
    /// 排在 B10 之后，补零后批序与字符串序一致，归档检索不必写正则。
    pub fn locate(&self) -> String {
        let mut s = String::with_capacity(24);
        s.push_str(match self.side {
            Side::Outline => "outline",
            Side::Register => "register",
            Side::Both => "both",
        });
        s.push_str("/B");
        if self.batch < 10 {
            s.push('0');
        }
        s.push_str(&self.batch.to_string());
        s.push('/');
        s.push_str(self.column.label());
        s
    }
    /// 差异对应的诊断码（u32）。
    pub const fn code(&self) -> u32 {
        self.kind.code()
    }
}

/// 三分判决。不设 `Divergent(Diff)` 载荷：报告已持有差异表，判决枚举再持一份就
/// 会出现「判决说 B09、差异表首条是 B17」的双源不一致。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignVerdict {
    Aligned,
    Vacuous,
    Divergent,
}

impl AlignVerdict {
    /// 判决名（报告与判据断言用）。
    pub const fn label(self) -> &'static str {
        match self {
            AlignVerdict::Aligned => "aligned",
            AlignVerdict::Vacuous => "vacuous",
            AlignVerdict::Divergent => "divergent",
        }
    }
    /// 是否可放行下游。真空一致不可放行：空表通过只说明「没有东西可对」。
    pub const fn is_pass(self) -> bool {
        matches!(self, AlignVerdict::Aligned)
    }
}

/// 一致性核报告。
pub struct AlignReport {
    /// 差异表（按闸门顺序排列，首条即首要诊断）。
    pub diffs: Vec<Diff>,
    /// 完成逐批比对的配对数。
    pub pairs: u16,
    /// 完成逐列比对的列次数（配对数 × 值列数）。
    pub columns: u32,
    /// 两侧投影的联合指纹。
    pub fingerprint: u64,
}

impl AlignReport {
    /// 判决。
    pub fn verdict(&self) -> AlignVerdict {
        if !self.diffs.is_empty() {
            return AlignVerdict::Divergent;
        }
        if self.pairs == 0 {
            return AlignVerdict::Vacuous;
        }
        AlignVerdict::Aligned
    }
    /// 首要诊断码（无差异时返回 0x91021）。
    pub fn primary_code(&self) -> u32 {
        match self.diffs.first() {
            Some(d) => d.code(),
            None => AlignCode::Aligned.code(),
        }
    }
    /// 首要差异。
    pub fn primary(&self) -> Option<Diff> {
        self.diffs.first().copied()
    }
    /// 某码的差异条数。
    pub fn count_of(&self, code: u32) -> usize {
        let mut n = 0usize;
        for d in &self.diffs {
            if d.code() == code {
                n += 1;
            }
        }
        n
    }
    /// 摘要文本（字段顺序固定，便于 diff）：判决 + 计数 + 七类分项 + 指纹。
    pub fn summary(&self) -> String {
        let mut s = String::with_capacity(96);
        s.push_str("align verdict=");
        s.push_str(self.verdict().label());
        s.push_str(" pairs=");
        s.push_str(&self.pairs.to_string());
        s.push_str(" cols=");
        s.push_str(&self.columns.to_string());
        s.push_str(" diffs=");
        s.push_str(&self.diffs.len().to_string());
        for k in [AlignCode::MissingBatch, AlignCode::ExtraBatch, AlignCode::ColumnDrift, AlignCode::OrderSkew, AlignCode::ColumnOutOfRange, AlignCode::SpanBreak, AlignCode::DuplicateBatch] {
            s.push(' ');
            s.push_str(k.label());
            s.push('=');
            s.push_str(&self.count_of(k.code()).to_string());
        }
        s.push_str(" fp=");
        s.push_str(&hex64(self.fingerprint));
        s
    }
}

/// 构造期故障（夹具用：越域行直接拒收，不塞进表里再靠判据兜）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AlignFault {
    /// 诊断码（u32：五位数码段）。
    pub code: u32,
    /// 相关批号。
    pub batch: u16,
}

impl AlignFault {
    /// 以种类 + 批号构造故障。
    pub const fn new(kind: AlignCode, batch: u16) -> AlignFault {
        AlignFault { code: kind.code(), batch }
    }
}

/// 表构造器。
///
/// 为什么用构造器而不在判据里手搓 `Vec<Row>`：违规样本有七种变形，每种都要保证
/// 「除目标变形外其余 39 行与标准表逐位相同」——手搓必然带出第二个缺陷，判据会因
/// 「首要码是另一个」而红，且极难自查。
pub struct TableBuilder {
    rows: Vec<Row>,
}

impl TableBuilder {
    /// 标准 40 行表。
    pub fn full() -> Result<TableBuilder, AlignFault> {
        let mut rows = Vec::with_capacity(BATCH_COUNT as usize);
        let mut b: u16 = 1;
        while b <= BATCH_COUNT {
            rows.push(standard_row(b)?);
            b += 1;
        }
        Ok(TableBuilder { rows })
    }
    /// 当前行数。
    pub fn len(&self) -> usize { self.rows.len() }
    /// 删除某批（造「缺失」）。
    pub fn drop_batch(&mut self, batch: u16) -> bool {
        match self.rows.iter().position(|r| r.batch == batch) {
            Some(i) => {
                self.rows.remove(i);
                true
            }
            None => false,
        }
    }
    /// 追加一行（造「多出」/越域批）。
    pub fn push_row(&mut self, row: Row) -> bool {
        if self.rows.len() > BATCH_COUNT as usize {
            return false;
        }
        self.rows.push(row);
        true
    }
    /// 改配额（造「配额漂移」或「配额为零」）。
    pub fn set_quota(&mut self, batch: u16, quota: u32) -> bool {
        for r in &mut self.rows {
            if r.batch == batch {
                r.quota = quota;
                return true;
            }
        }
        false
    }
    /// 整段平移（首末同移、宽度不变；造「漂移」与「两侧同错的断口」）。
    pub fn shift_span(&mut self, batch: u16, delta: i32) -> bool {
        for r in &mut self.rows {
            if r.batch == batch {
                let f = r.first as i64 + delta as i64;
                let l = r.last as i64 + delta as i64;
                if f < 0 || l < 0 {
                    return false;
                }
                r.first = f as u32;
                r.last = l as u32;
                return true;
            }
        }
        false
    }
    /// 交换两行位置（造「顺序错位」；列值一个不动）。
    pub fn swap(&mut self, a: u16, b: u16) -> bool {
        match (self.rows.iter().position(|r| r.batch == a), self.rows.iter().position(|r| r.batch == b)) {
            (Some(x), Some(y)) => {
                self.rows.swap(x, y);
                true
            }
            _ => false,
        }
    }
    /// 复制某行（造「重复登记」）。
    pub fn duplicate(&mut self, batch: u16) -> bool {
        for i in 0..self.rows.len() {
            if self.rows[i].batch == batch {
                let row = self.rows[i];
                self.rows.insert(i + 1, row);
                return true;
            }
        }
        false
    }
    /// 取出表。
    pub fn build(self) -> Vec<Row> {
        self.rows
    }
}

/// 第 `batch` 批的标准行（条目按每批 20 条、批间连续推导）。
pub fn standard_row(batch: u16) -> Result<Row, AlignFault> {
    if batch < 1 || batch > BATCH_COUNT {
        return Err(AlignFault::new(AlignCode::ColumnOutOfRange, batch));
    }
    let first = FIRST_ITEM_ID + (batch as u32 - 1) * ITEMS_PER_BATCH;
    let last = first + ITEMS_PER_BATCH - 1;
    if last > LAST_ITEM_ID {
        return Err(AlignFault::new(AlignCode::ColumnOutOfRange, batch));
    }
    Ok(Row::new(batch, first, last, ROWS_PER_BATCH))
}

/// 标准总纲侧（40 行）。
pub fn standard_outline() -> Result<Vec<Row>, AlignFault> {
    Ok(TableBuilder::full()?.build())
}

/// 标准批册侧（40 行）。
pub fn standard_register() -> Result<Vec<Row>, AlignFault> {
    Ok(TableBuilder::full()?.build())
}

/// 在表里找某批的下标（首个命中）。
fn find_batch(rows: &[Row], batch: u16) -> Option<usize> {
    rows.iter().position(|r| r.batch == batch)
}

/// 统计某批在表里出现几次（重复登记检测用）。
fn occurrences(rows: &[Row], batch: u16) -> u16 {
    let mut n = 0u16;
    for r in rows {
        if r.batch == batch {
            n += 1;
        }
    }
    n
}

/// 抽取「两侧都存在的批号」序列（保持该侧原始次序，批号去重）。
fn common_sequence(rows: &[Row], peer: &[Row], out: &mut Vec<u16>) {
    out.clear();
    for r in rows {
        if find_batch(peer, r.batch).is_some() && !out.contains(&r.batch) {
            out.push(r.batch);
        }
    }
}

/// 三闸对拍器。持有差异表而非逐条返回 `Result`：一次核要报全部差异（运维一轮改
/// 完），而 `Result` 只能带回第一个错——那会把七类缺陷拆成七轮排查。
pub struct Aligner<'a> {
    outline: &'a [Row],
    register: &'a [Row],
    diffs: Vec<Diff>,
    pairs: u16,
    columns: u32,
    /// 已登记过重复的批号（避免同一重复被两侧各报一次）。
    dup_reported: Vec<u16>,
}

impl<'a> Aligner<'a> {
    /// 绑定两侧投影。
    pub fn new(outline: &'a [Row], register: &'a [Row]) -> Aligner<'a> {
        Aligner { outline, register, diffs: Vec::new(), pairs: 0, columns: 0, dup_reported: Vec::new() }
    }
    /// 追加一条差异。
    fn emit(&mut self, kind: AlignCode, batch: u16, column: Column, side: Side, left: u32, right: u32) {
        self.diffs.push(Diff { kind, batch, column, side, left, right });
    }
    /// 登记重复（同批只报一次：两侧各报一条会把「一行被复制」放大成两条）。
    fn note_duplicate(&mut self, rows: &[Row], side: Side) {
        for r in rows {
            let n = occurrences(rows, r.batch);
            if n > 1 && !self.dup_reported.contains(&r.batch) {
                self.dup_reported.push(r.batch);
                self.emit(AlignCode::DuplicateBatch, r.batch, Column::Batch, side, n as u32, 1);
            }
        }
    }
    /// 第一闸：存在性（集合比对）。
    fn scan_existence(&mut self) {
        let left = self.outline;
        let right = self.register;
        for r in left {
            if find_batch(right, r.batch).is_none() {
                self.emit(AlignCode::MissingBatch, r.batch, Column::Batch, Side::Outline, r.first, 0);
            }
        }
        for r in right {
            if find_batch(left, r.batch).is_none() {
                self.emit(AlignCode::ExtraBatch, r.batch, Column::Batch, Side::Register, 0, r.first);
            }
        }
        self.note_duplicate(left, Side::Outline);
        self.note_duplicate(right, Side::Register);
    }
    /// 第二闸：值相等（三列逐列比）。
    fn scan_values(&mut self) {
        let left = self.outline;
        let right = self.register;
        for r in left {
            if let Some(j) = find_batch(right, r.batch) {
                let p = right[j];
                self.pairs += 1;
                self.columns += ROW_COLUMNS;
                if r.first != p.first {
                    self.emit(AlignCode::ColumnDrift, r.batch, Column::SpanStart, Side::Both, r.first, p.first);
                }
                if r.last != p.last {
                    self.emit(AlignCode::ColumnDrift, r.batch, Column::SpanEnd, Side::Both, r.last, p.last);
                }
                if r.quota != p.quota {
                    self.emit(AlignCode::ColumnDrift, r.batch, Column::Quota, Side::Both, r.quota, p.quota);
                }
            }
        }
    }
    /// 第三闸：顺序一致。只报第一处错位：一次换位产生两个逆序点，逐点都报会让
    /// 「B03↔B04 互换」变成两条差异，运维按条数判断损坏规模时会高估一倍。
    fn scan_order(&mut self) {
        let mut left: Vec<u16> = Vec::new();
        let mut right: Vec<u16> = Vec::new();
        common_sequence(self.outline, self.register, &mut left);
        common_sequence(self.register, self.outline, &mut right);
        for i in 0..left.len().min(right.len()) {
            if left[i] != right[i] {
                let (a, b) = (left[i], right[i]);
                self.emit(AlignCode::OrderSkew, a, Column::Batch, Side::Both, a as u32, b as u32);
                return;
            }
        }
    }
    /// 结构 lint（两侧自查，不是对拍）。断裂只在「批号本身连续」时判：批号断了
    /// 说明缺批，此时再报断裂就是把同一个根因报两遍。
    fn scan_structure(&mut self, rows: &[Row], side: Side) {
        for i in 0..rows.len() {
            let row = rows[i];
            if !row.is_well_formed() {
                self.emit(AlignCode::ColumnOutOfRange, row.batch, Column::SpanEnd, side, row.first, row.last);
            }
            if i > 0 {
                let prev = rows[i - 1];
                let expect = prev.last.saturating_add(1);
                if row.batch == prev.batch + 1 && row.first != expect {
                    self.emit(AlignCode::SpanBreak, row.batch, Column::SpanStart, side, expect, row.first);
                }
            }
        }
    }
    /// 跑完三道对拍闸 + 结构 lint，产出报告。
    pub fn run(mut self) -> AlignReport {
        self.scan_existence();
        self.scan_values();
        self.scan_order();
        let left = self.outline;
        let right = self.register;
        self.scan_structure(left, Side::Outline);
        self.scan_structure(right, Side::Register);
        let fp = table_fingerprint(left, right);
        AlignReport { diffs: self.diffs, pairs: self.pairs, columns: self.columns, fingerprint: fp }
    }
}

/// 对拍入口：给定两侧投影，产出报告。
pub fn compare_tables(outline: &[Row], register: &[Row]) -> AlignReport {
    Aligner::new(outline, register).run()
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

/// 两侧联合指纹。逐字段喂而不是喂结构体内存：内存含 padding，未初始化字节会让
/// 指纹跨编译器版本漂移；两侧之间插哨兵，避免「总纲末几行搬去批册头部」重排后
/// 算出相同指纹。
fn table_fingerprint(outline: &[Row], register: &[Row]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    h = fnv_u64(h, outline.len() as u64);
    for r in outline {
        h = fnv_u64(h, r.batch as u64);
        h = fnv_u64(h, r.first as u64);
        h = fnv_u64(h, r.last as u64);
        h = fnv_u64(h, r.quota as u64);
    }
    h = fnv_u64(h, 0x5a5a_u64);
    h = fnv_u64(h, register.len() as u64);
    for r in register {
        h = fnv_u64(h, r.batch as u64);
        h = fnv_u64(h, r.first as u64);
        h = fnv_u64(h, r.last as u64);
        h = fnv_u64(h, r.quota as u64);
    }
    h
}

/// 64 位十六进制（no_std 手写，避免格式化分配）。
fn hex64(v: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    let mut shift = 60i32;
    while shift >= 0 {
        s.push(HEX[((v >> shift) & 0xF) as usize] as char);
        shift -= 4;
    }
    s
}

/// 取标准表构造器；构造失败时给空构造器（后续断言自然全红，不吞故障）。
fn builder_or_empty() -> TableBuilder {
    match TableBuilder::full() {
        Ok(b) => b,
        Err(_) => TableBuilder { rows: Vec::new() },
    }
}

/// 以标准总纲侧为基准核一份批册（夹具不可用时退化为空总纲，判据自会红）。
fn report_against(register: &[Row]) -> AlignReport {
    match standard_outline() {
        Ok(l) => compare_tables(&l, register),
        Err(_) => compare_tables(&[], register),
    }
}

/// 判据入口（UNX-F21585-J1 及其配套）。判据常量独立写死：40 / 120 / 0x91022 等在
/// 断言里直接以字面量出现，不复用被测常量，避免「被测值改成 41，判据跟着变」的
/// 自证循环。
pub fn run_unx21585_outlinealign_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21585");
    let outline = standard_outline();
    let register = standard_register();

    // 1) 夹具可构造：两侧 40 行且逐位相同。夹具自带矛盾会让后续全部判据失去意义。
    let fixture_ok = match (&outline, &register) {
        (Ok(l), Ok(r)) => l.len() == 40 && r.len() == 40 && l == r,
        _ => false,
    };
    cs.add("f21585.fixture_buildable", fixture_ok, "标准 40 行表构造失败或两侧不等");

    // 2) 正向：配对数 40、值列 120、零差异、判决 aligned、首要码 0x91021。
    if let (Ok(l), Ok(r)) = (&outline, &register) {
        let rep = compare_tables(l, r);
        cs.add("f21585.aligned_clean", rep.verdict() == AlignVerdict::Aligned && rep.pairs == 40 && rep.columns == 120 && rep.diffs.is_empty() && rep.primary_code() == 0x91021, "标准两侧未判一致，或配对数/列数不是 40/120");
    } else {
        cs.add("f21585.aligned_clean", false, "夹具不可用");
    }

    // 3) 边界·单行：只有 B01 时两侧仍有内容，判决必须是 aligned 而非 vacuous。
    {
        let one = match standard_row(1) {
            Ok(r) => r,
            Err(_) => Row::default(),
        };
        let rep = compare_tables(&[one], &[one]);
        cs.add("f21585.single_row_aligned", rep.verdict() == AlignVerdict::Aligned && rep.pairs == 1 && rep.columns == 3 && one.width() == 20, "单行对齐未被判有效一致（配对数应为 1、宽度应为 20）");
    }

    // 4) 边界·空表：两侧都空 → 真空一致，不许混进绿。
    {
        let empty: [Row; 0] = [];
        let rep = compare_tables(&empty, &empty);
        cs.add("f21585.empty_sides_vacuous", rep.verdict() == AlignVerdict::Vacuous && !rep.verdict().is_pass() && rep.diffs.is_empty() && rep.pairs == 0, "空表被当成有效一致（真空真必须判 vacuous）");
    }

    // 5) 反向·缺失：批册删掉 B17 → 0x91022，定位到 B17、侧为 outline。
    {
        let mut b = builder_or_empty();
        let ok = b.drop_batch(17);
        let rep = report_against(&b.build());
        let hit = rep.primary().map_or(false, |d| d.code() == 0x91022 && d.batch == 17 && d.side == Side::Outline && d.locate() == "outline/B17/batch");
        cs.add("f21585.missing_batch", ok && hit && rep.count_of(0x91022) == 1, "缺 1 批未被判 0x91022，或批号/侧定位错");
    }

    // 6) 反向·多出（上限 +1）：批册 41 行，末行 B41 → 0x91023。
    {
        let mut b = builder_or_empty();
        let ok = b.push_row(Row::new(41, 21_601, 21_620, 6_000));
        let rows = b.len();
        let rep = report_against(&b.build());
        let hit = rep.primary().map_or(false, |d| d.code() == 0x91023 && d.batch == 41 && d.side == Side::Register);
        cs.add("f21585.extra_batch", ok && hit && rows == 41, "41 行表未被判 0x91023，或多出批号定位错");
    }

    // 7) 反向·配额漂移：批册 B09 配额 6001 → 0x91024 + 列 quota + 批 9。
    {
        let mut b = builder_or_empty();
        let ok = b.set_quota(9, 6_001);
        let rep = report_against(&b.build());
        let hit = rep.primary().map_or(false, |d| d.code() == 0x91024 && d.batch == 9 && d.column == Column::Quota && d.right == 6_001);
        cs.add("f21585.quota_drift", ok && hit && rep.count_of(0x91024) == 1, "配额漂移未被判 0x91024，或列名/批号定位错");
    }

    // 8) 反向·区间漂移：批册 B22 整段后移一位 → 首末两列各报一条 0x91024。
    {
        let mut b = builder_or_empty();
        let ok = b.shift_span(22, 1);
        let rep = report_against(&b.build());
        let hit = rep.primary().map_or(false, |d| d.code() == 0x91024 && d.batch == 22 && d.column == Column::SpanStart && d.left == 21_221);
        cs.add("f21585.span_drift", ok && hit && rep.count_of(0x91024) == 2, "整段平移应令首末两列各报一次漂移，实际列名或条数不符");
    }

    // 9) 反向·顺序错位（双检的关键样本）：只交换 B03/B04 位置，列值一个不动。
    //    单检（逐下标取值）会误报成两条值漂移；双检必须先比集合、再比顺序。
    {
        let mut b = builder_or_empty();
        let ok = b.swap(3, 4);
        let rep = report_against(&b.build());
        let hit = rep.primary().map_or(false, |d| d.code() == 0x91025 && d.column == Column::Batch);
        cs.add("f21585.order_skew", ok && hit && rep.count_of(0x91024) == 0 && rep.count_of(0x91025) == 1, "换位被误判成值漂移（应零 0x91024、一条 0x91025）");
    }

    // 10) 反向·重复登记：批册 B05 出现两次 → 0x91028，且只报一条。
    {
        let mut b = builder_or_empty();
        let ok = b.duplicate(5);
        let rep = report_against(&b.build());
        let hit = rep.primary().map_or(false, |d| d.code() == 0x91028 && d.batch == 5);
        cs.add("f21585.duplicate_batch", ok && hit && rep.count_of(0x91028) == 1, "重复登记未报 0x91028，或同批被重复上报");
    }

    // 11) 反向·列值越界：配额清零 → 0x91026（结构 lint 层，不属于值漂移）。
    {
        let mut b = builder_or_empty();
        let ok = b.set_quota(2, 0);
        let rep = report_against(&b.build());
        let mut oor = 0u16;
        for d in &rep.diffs {
            if d.code() == 0x91026 {
                oor = d.batch;
                break;
            }
        }
        cs.add("f21585.column_out_of_range", ok && oor == 2, "配额为零未报 0x91026，或越界行定位不到 B02");
    }

    // 12) 反向·区间断裂（两侧同错）：两侧都把 B12 整段后移一位，对拍零漂移，但
    //     B11 末条 +1 ≠ B12 首条 → 只有结构 lint 看得见（每侧进入/离开各一处）。
    {
        let mut lo = builder_or_empty();
        let mut ro = builder_or_empty();
        let ok = lo.shift_span(12, 1) && ro.shift_span(12, 1);
        let rep = compare_tables(&lo.build(), &ro.build());
        let mut brk = 0u16;
        for d in &rep.diffs {
            if d.code() == 0x91027 {
                brk = d.batch;
                break;
            }
        }
        cs.add("f21585.span_break", ok && rep.count_of(0x91027) == 4 && brk == 12 && rep.count_of(0x91024) == 0, "两侧同错的断口未被结构 lint 抓到（应零漂移、两侧各两处 0x91027，首条在 B12）");
    }

    // 13) 幂等·确定性：同输入两次结果逐位相同（条数、首要码、指纹、摘要）。
    if let (Ok(l), Ok(r)) = (&outline, &register) {
        let a = compare_tables(l, r);
        let b = compare_tables(l, r);
        cs.add("f21585.idempotent", a.fingerprint == b.fingerprint && a.diffs.len() == b.diffs.len() && a.primary_code() == b.primary_code() && a.summary() == b.summary(), "两次对拍结果不一致（或摘要不稳定）");
    } else {
        cs.add("f21585.idempotent", false, "夹具不可用");
    }

    // 14) 指纹对篡改敏感：改一个配额，联合指纹必变（两侧同错也逃不掉）。
    {
        let base = match (&outline, &register) {
            (Ok(l), Ok(r)) => compare_tables(l, r).fingerprint,
            _ => 0,
        };
        let mut b = builder_or_empty();
        let ok = b.set_quota(1, 5_999);
        let fp = report_against(&b.build()).fingerprint;
        cs.add("f21585.fingerprint_sensitive", ok && base != 0 && base != fp, "改一列后指纹未变");
    }

    cs
}
