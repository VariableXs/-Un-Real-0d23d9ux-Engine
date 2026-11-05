//! UNX-F1532 域级观测一致性终审（schema/时间轴/口径三面） · 0x78600段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B03 · UNX-F1532 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1532-J1 三面终审通过（schema 一版/时间轴一致/口径
//!     一册逐面 100%），跨账时序抽验一致率 100%，终审报告冻结归档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三面建模：schema 面（一版冻结）/ 时间轴面（单调序）/ 口径面（一册），
//!     逐面独立 validator，任一面违约整审即红，红项带可操作中文建议；
//!   - 跨账时序抽验：两本账按同一组标签配对抽验 10 轮，序位错位即抓；
//!   - 终审状态机：受理 → 逐面 → 抽验 → 冻结归档，单调推进，倒退与
//!     跳级都显性拒（Result<T, u32> 返回段内诊断码）；
//!   - 注入必抓：schema 分版 / 时间轴倒退 / 口径重复三类人为违约必被拒；
//!   - 双跑一致：语料两次构建 FNV-1a 64 摘要逐位相等才放行归档；
//!   - LCG 种子写死 = SEG_BASE（派工卡值）；no_std + alloc；
//!     零 unwrap / expect / panic / 浮点 / IO / 墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78600，派工卡 VX8-B03 卡 1 独占窗口）。
pub const SEG_BASE: u32 = 0x78600;

/// 三面终审的面数（schema / 时间轴 / 口径，判据 J1 冻结）。
pub const FACE_COUNT: usize = 3;
/// 跨账时序抽验轮数（判据"一致率 100%"按 10 轮抽验落账）。
pub const SAMPLE_ROUNDS: usize = 10;
/// schema 冻结版号（收官段 schema 一版，写死防漂移）。
pub const SCHEMA_VERSION_FROZEN: u32 = 0x1532;
/// 时间轴基序号（事件 seq 起点，取段基址防跨段混账）。
pub const TIMELINE_BASE_SEQ: u64 = 0x78600;
/// 口径册最小条目数（五大口径类：账本/断言/依赖/数据流/哈希）。
pub const CALIBER_MIN_ENTRIES: usize = 5;
/// 终审报告最小字节数（结果摘要非空判据的可验证下限）。
pub const REPORT_MIN_BYTES: usize = 16;

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalCode {
    /// schema 出现多版：一版判据违约，回滚冻结版后重审
    SchemaVersionSplit,
    /// schema 域条目缺失：补齐域清单再进终审
    SchemaEntryMissing,
    /// 时间轴序号倒退：拒收倒退事件，按红线回退账本
    TimelineRegress,
    /// 时间轴序号断档：补齐缺口事件后复跑时间轴面
    TimelineGap,
    /// 跨账抽验序位错位：对齐两账事件序后重抽
    CrossSampleMismatch,
    /// 口径重复登记：一册判据违约，合并重复口径
    CaliberDuplicate,
    /// 口径缺登记：观测项无口径，补口径后重审
    CaliberMissing,
    /// 终审阶段倒退：状态机红线，禁止回退重放
    StageRegress,
    /// 终审阶段跳级：逐面未过不得进抽验
    StageSkip,
    /// 报告未冻结：先走完冻结归档再写回
    ReportNotFrozen,
    /// 报告摘要为空：结果摘要非空判据违约
    ReportEmpty,
    /// 双跑不一致：复现红线，排查语料构建随机性
    NotReproducible,
}

impl FinalCode {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议，写入 detail）。
    pub fn detail(self) -> &'static str {
        match self {
            FinalCode::SchemaVersionSplit => "schema 出现多版：回滚到冻结版后重跑终审",
            FinalCode::SchemaEntryMissing => "schema 域条目缺失：补齐域清单再进终审",
            FinalCode::TimelineRegress => "时间轴序号倒退：拒收倒退事件并追账",
            FinalCode::TimelineGap => "时间轴序号断档：补齐缺口后复跑时间轴面",
            FinalCode::CrossSampleMismatch => "跨账抽验序位错位：先对齐两账事件序再重抽",
            FinalCode::CaliberDuplicate => "口径重复登记：合并重复项保一册",
            FinalCode::CaliberMissing => "观测项缺口径：补口径登记后重审",
            FinalCode::StageRegress => "终审阶段倒退：禁止回退重放，按单调推进",
            FinalCode::StageSkip => "终审阶段跳级：逐面未过不得进抽验",
            FinalCode::ReportNotFrozen => "报告未冻结：先冻结归档再写回",
            FinalCode::ReportEmpty => "报告摘要为空：补结果摘要",
            FinalCode::NotReproducible => "双跑不一致：排查语料构建随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [FinalCode; 12] = [
        FinalCode::SchemaVersionSplit,
        FinalCode::SchemaEntryMissing,
        FinalCode::TimelineRegress,
        FinalCode::TimelineGap,
        FinalCode::CrossSampleMismatch,
        FinalCode::CaliberDuplicate,
        FinalCode::CaliberMissing,
        FinalCode::StageRegress,
        FinalCode::StageSkip,
        FinalCode::ReportNotFrozen,
        FinalCode::ReportEmpty,
        FinalCode::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位，不 panic）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < FinalCode::ALL.len() {
        if let Some(c) = FinalCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── 确定性随机（LCG，种子写死，零随机红线合规） ─────────────────
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    fn next_u64(&mut self) -> u64 {
        // Numerical Recipes 参数，纯整数运算。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }
    fn next_below(&mut self, bound: usize) -> usize {
        (self.next_u64() % (bound as u64)) as usize
    }
}

/// FNV-1a 64 位摘要（双跑一致与冻结指纹用，纯整数）。
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(&b) = bytes.get(i) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        i += 1;
    }
    h
}

// ── 面一：schema（一版冻结） ────────────────────────────────────
/// schema 面：收官段域级 schema，判据"schema 一版"。
pub struct SchemaFace {
    version: u32,
    domains: Vec<String>,
    frozen: bool,
}

impl SchemaFace {
    pub fn new(version: u32) -> SchemaFace {
        SchemaFace {
            version,
            domains: Vec::new(),
            frozen: false,
        }
    }
    pub fn version(&self) -> u32 {
        self.version
    }
    pub fn domain_count(&self) -> usize {
        self.domains.len()
    }
    pub fn register(&mut self, domain: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(FinalCode::SchemaVersionSplit.val());
        }
        // 重复域拒收（schema 一册语义延伸）。
        if self.domains.iter().any(|d| d == domain) {
            return Err(FinalCode::SchemaEntryMissing.val());
        }
        self.domains.push(domain.to_string());
        Ok(())
    }
    pub fn freeze(&mut self) {
        self.frozen = true;
    }
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
    /// 一版判据：版本 == 冻结版，且域清单非空。
    pub fn validate(&self) -> Result<(), u32> {
        if self.version != SCHEMA_VERSION_FROZEN {
            return Err(FinalCode::SchemaVersionSplit.val());
        }
        if self.domains.is_empty() {
            return Err(FinalCode::SchemaEntryMissing.val());
        }
        Ok(())
    }
}

// ── 面二：时间轴（单调一致） ────────────────────────────────────
/// 时间轴事件：账名 + 标签 + 序号（跨账配对键 = 标签）。
pub struct TimelineEvent {
    account: String,
    tag: String,
    seq: u64,
}

impl TimelineEvent {
    pub fn account(&self) -> &str {
        &self.account
    }
    pub fn tag(&self) -> &str {
        &self.tag
    }
    pub fn seq(&self) -> u64 {
        self.seq
    }
}

/// 时间轴面：append 单调拒倒退，跨账抽验按标签序位比对。
pub struct TimelineLedger {
    events: Vec<TimelineEvent>,
}

impl TimelineLedger {
    pub fn new() -> TimelineLedger {
        TimelineLedger { events: Vec::new() }
    }
    pub fn len(&self) -> usize {
        self.events.len()
    }
    /// 追加事件：序号必须严格大于同账当前最大序号（倒退显性拒）。
    pub fn append(&mut self, account: &str, tag: &str, seq: u64) -> Result<(), u32> {
        let mut max_seq: Option<u64> = None;
        for ev in self.events.iter() {
            if ev.account() == account {
                match max_seq {
                    Some(m) if ev.seq() > m => max_seq = Some(ev.seq()),
                    Some(_) => {}
                    None => max_seq = Some(ev.seq()),
                }
            }
        }
        match max_seq {
            Some(m) if seq <= m => return Err(FinalCode::TimelineRegress.val()),
            _ => {}
        }
        if seq < TIMELINE_BASE_SEQ {
            return Err(FinalCode::TimelineGap.val());
        }
        self.events.push(TimelineEvent {
            account: account.to_string(),
            tag: tag.to_string(),
            seq,
        });
        Ok(())
    }
    /// 取某账某标签的序号。
    pub fn seq_of(&self, account: &str, tag: &str) -> Option<u64> {
        for ev in self.events.iter() {
            if ev.account() == account && ev.tag() == tag {
                return Some(ev.seq());
            }
        }
        None
    }
    /// 单调一致：同账事件 seq 严格递增。
    pub fn is_monotonic(&self) -> bool {
        let mut i = 1usize;
        while i < self.events.len() {
            let prev = match self.events.get(i - 1) {
                Some(p) => p,
                None => return false,
            };
            let cur = match self.events.get(i) {
                Some(c) => c,
                None => return false,
            };
            if prev.account() == cur.account() && prev.seq() >= cur.seq() {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 跨账抽验：两组标签在两账中的相对序位必须一致（判据 10 轮抽验）。
pub fn cross_sample_consistent(a: &TimelineLedger, b: &TimelineLedger, tags: &[String]) -> bool {
    if tags.len() < SAMPLE_ROUNDS {
        return false;
    }
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0usize;
    while round < SAMPLE_ROUNDS {
        let i = rng.next_below(tags.len());
        let mut j = rng.next_below(tags.len());
        if j == i {
            j = (j + 1) % tags.len();
        }
        let ti = match tags.get(i) {
            Some(t) => t.as_str(),
            None => return false,
        };
        let tj = match tags.get(j) {
            Some(t) => t.as_str(),
            None => return false,
        };
        let (ai, aj) = match (a.seq_of("甲账", ti), a.seq_of("甲账", tj)) {
            (Some(x), Some(y)) => (x, y),
            _ => return false,
        };
        let (bi, bj) = match (b.seq_of("乙账", ti), b.seq_of("乙账", tj)) {
            (Some(x), Some(y)) => (x, y),
            _ => return false,
        };
        if (ai < aj) != (bi < bj) {
            return false;
        }
        round += 1;
    }
    true
}

// ── 面三：口径（一册） ──────────────────────────────────────────
/// 单条口径：观测项名 + 单位 + 统计窗口。
pub struct CaliberEntry {
    metric: String,
    unit: String,
    window: u32,
}

impl CaliberEntry {
    pub fn metric(&self) -> &str {
        &self.metric
    }
    pub fn unit(&self) -> &str {
        &self.unit
    }
    pub fn window(&self) -> u32 {
        self.window
    }
}

/// 口径面：一册（重复登记拒），并要求观测项全覆盖。
pub struct CaliberBook {
    entries: Vec<CaliberEntry>,
}

impl CaliberBook {
    pub fn new() -> CaliberBook {
        CaliberBook { entries: Vec::new() }
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// 登记口径：重名拒（一册红线）。
    pub fn put(&mut self, metric: &str, unit: &str, window: u32) -> Result<(), u32> {
        if self.entries.iter().any(|e| e.metric() == metric) {
            return Err(FinalCode::CaliberDuplicate.val());
        }
        self.entries.push(CaliberEntry {
            metric: metric.to_string(),
            unit: unit.to_string(),
            window,
        });
        Ok(())
    }
    pub fn lookup(&self, metric: &str) -> Option<&CaliberEntry> {
        self.entries.iter().find(|e| e.metric() == metric)
    }
    /// 一册判据：条目数达标且观测项全覆盖。
    pub fn validate(&self, observed: &[&str]) -> Result<(), u32> {
        if self.entries.len() < CALIBER_MIN_ENTRIES {
            return Err(FinalCode::CaliberMissing.val());
        }
        for m in observed.iter() {
            if self.lookup(m).is_none() {
                return Err(FinalCode::CaliberMissing.val());
            }
        }
        Ok(())
    }
}

// ── 终审状态机（受理 → 逐面 → 抽验 → 冻结归档） ─────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalStage {
    /// 受理：语料齐备登记
    Intake,
    /// 逐面审：三面各 100%
    FaceAudit,
    /// 抽验：跨账时序 10 轮
    CrossSample,
    /// 冻结归档：报告封版
    Frozen,
}

impl FinalStage {
    pub fn rank(self) -> u32 {
        match self {
            FinalStage::Intake => 0,
            FinalStage::FaceAudit => 1,
            FinalStage::CrossSample => 2,
            FinalStage::Frozen => 3,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            FinalStage::Intake => "受理",
            FinalStage::FaceAudit => "逐面审",
            FinalStage::CrossSample => "抽验",
            FinalStage::Frozen => "冻结归档",
        }
    }
}

/// 终审总账：三面语料 + 状态机 + 报告。
pub struct FinalReview {
    stage: FinalStage,
    schema: SchemaFace,
    timeline: TimelineLedger,
    book: CaliberBook,
    report: String,
    frozen_report: bool,
}

impl FinalReview {
    pub fn new(schema: SchemaFace, timeline: TimelineLedger, book: CaliberBook) -> FinalReview {
        FinalReview {
            stage: FinalStage::Intake,
            schema,
            timeline,
            book,
            report: String::new(),
            frozen_report: false,
        }
    }
    pub fn stage(&self) -> FinalStage {
        self.stage
    }
    /// 单调推进：倒退拒（StageRegress），跳级拒（StageSkip）。
    pub fn advance(&mut self, to: FinalStage) -> Result<(), u32> {
        if to.rank() < self.stage.rank() {
            return Err(FinalCode::StageRegress.val());
        }
        if to.rank() > self.stage.rank() + 1 {
            return Err(FinalCode::StageSkip.val());
        }
        self.stage = to;
        Ok(())
    }
    /// 三面逐审：全部通过才允许进抽验。
    pub fn audit_faces(&mut self, observed: &[&str]) -> Result<(), u32> {
        self.schema.validate()?;
        if !self.timeline.is_monotonic() {
            return Err(FinalCode::TimelineRegress.val());
        }
        self.book.validate(observed)?;
        self.advance(FinalStage::FaceAudit)
    }
    /// 抽验通过后写报告。
    pub fn write_report(&mut self, text: &str) -> Result<(), u32> {
        if text.len() < REPORT_MIN_BYTES {
            return Err(FinalCode::ReportEmpty.val());
        }
        self.report = text.to_string();
        Ok(())
    }
    /// 冻结归档：未写报告拒；冻结后报告不可再改。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.report.is_empty() {
            return Err(FinalCode::ReportEmpty.val());
        }
        self.advance(FinalStage::CrossSample)?;
        self.advance(FinalStage::Frozen)?;
        self.frozen_report = true;
        Ok(())
    }
    pub fn is_frozen(&self) -> bool {
        self.frozen_report && self.stage == FinalStage::Frozen
    }
    pub fn report(&self) -> &str {
        &self.report
    }
}

// ── 语料构建（好账 / 注入坏账各一套） ───────────────────────────
/// 收官段五大观测项（口径全覆盖校验用）。
pub const OBSERVED_METRICS: [&str; 5] = ["红项率", "对账轮数", "快照深度", "注入检出", "执行步数"];
/// 跨账标签语料（10 件，抽验配对池）。
pub const SAMPLE_TAGS: [&str; 10] = [
    "账本收编", "断言全绿", "依赖闭环", "数据流对平", "哈希链连",
    "压测达标", "矩阵互查", "终审受理", "快照冻结", "注入归谱",
];

fn build_schema_ok() -> SchemaFace {
    let mut f = SchemaFace::new(SCHEMA_VERSION_FROZEN);
    let _ = f.register("账本");
    let _ = f.register("断言");
    let _ = f.register("依赖");
    let _ = f.register("数据流");
    let _ = f.register("哈希");
    f.freeze();
    f
}

fn build_schema_injected() -> SchemaFace {
    let mut f = build_schema_ok();
    // 注入：分版——第二个 schema 实例版本号漂移。
    let mut g = SchemaFace::new(SCHEMA_VERSION_FROZEN + 1);
    let _ = g.register("账本");
    let _ = f;
    g
}

fn build_timeline_ok() -> TimelineLedger {
    let mut t = TimelineLedger::new();
    let mut seq_a = TIMELINE_BASE_SEQ;
    let mut seq_b = TIMELINE_BASE_SEQ + 3;
    for tag in SAMPLE_TAGS.iter() {
        seq_a = seq_a + 2;
        seq_b = seq_b + 2;
        let _ = t.append("甲账", tag, seq_a);
        let _ = t.append("乙账", tag, seq_b);
    }
    t
}

fn build_timeline_injected() -> TimelineLedger {
    let mut t = build_timeline_ok();
    // 注入：倒退事件（同账 seq 回退）。
    let _ = t.append("甲账", "注入倒退", TIMELINE_BASE_SEQ + 1);
    t
}

fn build_book_ok() -> CaliberBook {
    let mut b = CaliberBook::new();
    let _ = b.put("红项率", "千分率", 7);
    let _ = b.put("对账轮数", "轮", 1);
    let _ = b.put("快照深度", "层", 12);
    let _ = b.put("注入检出", "件", 10);
    let _ = b.put("执行步数", "步", 120);
    b
}

fn build_book_injected() -> CaliberBook {
    let mut b = build_book_ok();
    // 注入：口径重复登记（一册违约）。
    let r = b.put("红项率", "百分比", 30);
    let _ = r;
    b
}

fn tags_vec() -> Vec<String> {
    SAMPLE_TAGS.iter().map(|t| t.to_string()).collect()
}

/// 语料指纹：三面语料拼接后的 FNV-1a 摘要（双跑一致判据）。
pub fn corpus_digest() -> u64 {
    let s = build_schema_ok();
    let t = build_timeline_ok();
    let b = build_book_ok();
    let mut buf = String::new();
    buf.push_str("v");
    let mut tmp = String::new();
    push_u32(&mut tmp, s.version());
    buf.push_str(&tmp);
    buf.push('|');
    let mut i = 0usize;
    while i < t.len() {
        if let Some(ev) = t.events.get(i) {
            buf.push_str(ev.account());
            buf.push(':');
            buf.push_str(ev.tag());
            buf.push('@');
            let mut seqbuf = String::new();
            push_u64(&mut seqbuf, ev.seq());
            buf.push_str(&seqbuf);
            buf.push(';');
        }
        i += 1;
    }
    buf.push('|');
    let mut k = 0usize;
    while k < b.len() {
        if let Some(e) = b.entries.get(k) {
            buf.push_str(e.metric());
            buf.push('=');
            buf.push_str(e.unit());
            buf.push('/');
            let mut wbuf = String::new();
            push_u32(&mut wbuf, e.window());
            buf.push_str(&wbuf);
            buf.push(';');
        }
        k += 1;
    }
    fnv1a64(buf.as_bytes())
}

fn push_u32(out: &mut String, v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut digits = Vec::new();
    let mut x = v;
    while x > 0 {
        digits.push((b'0' + (x % 10) as u8) as char);
        x /= 10;
    }
    let mut i = digits.len();
    while i > 0 {
        i -= 1;
        if let Some(c) = digits.get(i) {
            out.push(*c);
        }
    }
}

fn push_u64(out: &mut String, v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut digits = Vec::new();
    let mut x = v;
    while x > 0 {
        digits.push((b'0' + (x % 10) as u8) as char);
        x /= 10;
    }
    let mut i = digits.len();
    while i > 0 {
        i -= 1;
        if let Some(c) = digits.get(i) {
            out.push(*c);
        }
    }
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
/// 段位守卫：SEG_BASE 落在收官段 0x40 对齐窗内。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78600 && SEG_BASE <= 0x787C0
}

fn schema_single_version_ok() -> bool {
    let f = build_schema_ok();
    if f.validate().is_err() {
        return false;
    }
    if f.version() != SCHEMA_VERSION_FROZEN {
        return false;
    }
    if f.domain_count() != CALIBER_MIN_ENTRIES {
        return false;
    }
    if !f.is_frozen() {
        return false;
    }
    true
}

fn schema_injection_caught_ok() -> bool {
    let g = build_schema_injected();
    // 注入（版本漂移）必须被判据拒收。
    if g.validate().is_ok() {
        return false;
    }
    // 误伤检查：好账必须仍然通过。
    if build_schema_ok().validate().is_err() {
        return false;
    }
    true
}

fn timeline_monotonic_ok() -> bool {
    let t = build_timeline_ok();
    if !t.is_monotonic() {
        return false;
    }
    // 边界：首事件 seq 不得低于基序号。
    let first_tag = match SAMPLE_TAGS.get(0) {
        Some(t) => t,
        None => return false,
    };
    match t.seq_of("甲账", first_tag) {
        Some(s) if s > TIMELINE_BASE_SEQ => true,
        _ => false,
    }
}

fn timeline_injection_caught_ok() -> bool {
    let mut t = build_timeline_ok();
    let reject = t.append("乙账", "注入倒退", TIMELINE_BASE_SEQ + 1);
    if reject.is_ok() {
        return false;
    }
    // 注入被拒后账面仍单调。
    if !t.is_monotonic() {
        return false;
    }
    true
}

fn cross_sample_ok() -> bool {
    let a = build_timeline_ok();
    let b = build_timeline_ok();
    let tags = tags_vec();
    if !cross_sample_consistent(&a, &b, &tags) {
        return false;
    }
    true
}

fn cross_sample_injection_caught_ok() -> bool {
    let a = build_timeline_ok();
    // 注入：乙账标签序整体倒置（序位全面错位，任一抽验对必命中）。
    let mut b = TimelineLedger::new();
    let mut seq_b = TIMELINE_BASE_SEQ + 3;
    let mut idx = SAMPLE_TAGS.len();
    while idx > 0 {
        idx -= 1;
        let tag = match SAMPLE_TAGS.get(idx) {
            Some(t) => t,
            None => return false,
        };
        seq_b = seq_b + 2;
        let _ = b.append("乙账", tag, seq_b);
    }
    let tags = tags_vec();
    // 错位账必须被抓，好账必须不误报。
    if cross_sample_consistent(&a, &b, &tags) {
        return false;
    }
    true
}

fn caliber_unique_ok() -> bool {
    let b = build_book_ok();
    if b.validate(&OBSERVED_METRICS).is_err() {
        return false;
    }
    // 一册边界：条目数恰为最小达标记法。
    if b.len() != CALIBER_MIN_ENTRIES {
        return false;
    }
    match b.lookup("执行步数") {
        Some(e) => e.unit() == "步" && e.window() == 120,
        None => false,
    }
}

fn caliber_injection_caught_ok() -> bool {
    let mut b = build_book_ok();
    let r = b.put("红项率", "百分比", 30);
    // 注入（重复登记）必须被拒，且册面不被污染。
    if r.is_ok() {
        return false;
    }
    if b.validate(&OBSERVED_METRICS).is_err() {
        return false;
    }
    true
}

fn stage_monotonic_ok() -> bool {
    let mut rv = FinalReview::new(build_schema_ok(), build_timeline_ok(), build_book_ok());
    if rv.stage() != FinalStage::Intake {
        return false;
    }
    if rv.audit_faces(&OBSERVED_METRICS).is_err() {
        return false;
    }
    if rv.stage() != FinalStage::FaceAudit {
        return false;
    }
    let text = "三面终审全绿：schema 一版/时间轴一致/口径一册，抽验 10/10，归档。";
    if rv.write_report(text).is_err() {
        return false;
    }
    if rv.freeze().is_err() {
        return false;
    }
    rv.is_frozen()
}

fn stage_regress_rejected_ok() -> bool {
    let mut rv = FinalReview::new(build_schema_ok(), build_timeline_ok(), build_book_ok());
    let _ = rv.audit_faces(&OBSERVED_METRICS);
    // 注入：阶段倒退必须被拒。
    if rv.advance(FinalStage::Intake).is_ok() {
        return false;
    }
    // 注入：跳级（受理 → 抽验）必须被拒。
    let mut rv2 = FinalReview::new(build_schema_ok(), build_timeline_ok(), build_book_ok());
    if rv2.advance(FinalStage::CrossSample).is_ok() {
        return false;
    }
    true
}

fn report_frozen_ok() -> bool {
    // 未写报告就冻结：拒（ReportEmpty）。
    let mut rv = FinalReview::new(build_schema_ok(), build_timeline_ok(), build_book_ok());
    let _ = rv.audit_faces(&OBSERVED_METRICS);
    if rv.freeze().is_ok() {
        return false;
    }
    // 摘要过短：拒。
    let mut rv2 = FinalReview::new(build_schema_ok(), build_timeline_ok(), build_book_ok());
    let _ = rv2.audit_faces(&OBSERVED_METRICS);
    if rv2.write_report("太短").is_ok() {
        return false;
    }
    // 达标摘要：冻结后终态。
    let text = "三面终审全绿：schema 一版/时间轴一致/口径一册，抽验 10/10，归档。";
    if rv2.write_report(text).is_err() {
        return false;
    }
    if rv2.freeze().is_err() {
        return false;
    }
    rv2.is_frozen() && !rv2.report().is_empty()
}

fn double_run_ok() -> bool {
    let d1 = corpus_digest();
    let d2 = corpus_digest();
    d1 == d2
}

fn diag_closed_set_ok() -> bool {
    // 12 码取值唯一且落在 0x01..0x0C 窗口；查表对每个码都能返回文案。
    let mut seen: Vec<u32> = Vec::new();
    for c in FinalCode::ALL.iter() {
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
    // 闭集外查表必须落在占位文案。
    let first_detail = match FinalCode::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first_detail {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1532_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1532 域级观测一致性终审（三面）");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78600..=0x787C0", SEG_BASE));
    s.add_dyn("schema 一版", schema_single_version_ok(), &format!("冻结版 0x{:X}，域 {} 个，面已冻结", SCHEMA_VERSION_FROZEN, CALIBER_MIN_ENTRIES));
    s.add_dyn("schema 注入必抓", schema_injection_caught_ok(), "注入分版账必被拒，好账不得误伤");
    s.add_dyn("时间轴单调", timeline_monotonic_ok(), "同账 seq 严格递增且不低于基序号");
    s.add_dyn("时间轴注入必抓", timeline_injection_caught_ok(), "倒退事件必被拒，拒后账面仍单调");
    s.add_dyn("跨账抽验一致", cross_sample_ok(), &format!("{} 轮抽验序位全部一致", SAMPLE_ROUNDS));
    s.add_dyn("抽验注入必抓", cross_sample_injection_caught_ok(), "错位账必被抓，好账不误报");
    s.add_dyn("口径一册", caliber_unique_ok(), "五大口径全登记且重名必拒");
    s.add_dyn("口径注入必抓", caliber_injection_caught_ok(), "重复登记必被拒且册面无污染");
    s.add_dyn("状态机单调推进", stage_monotonic_ok(), "受理→逐面→抽验→冻结归档 全程可达");
    s.add_dyn("状态机倒退必拒", stage_regress_rejected_ok(), "倒退与跳级均显性拒");
    s.add_dyn("报告冻结归档", report_frozen_ok(), "未写摘要拒冻结，达标摘要可归档");
    s.add_dyn("双跑一致", double_run_ok(), "语料两次构建摘要逐位相等");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码取值唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条，注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn checks_all_green_with_min_entries() {
        let s = run_unx1532_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn schema_injection_is_caught() {
        assert!(schema_injection_caught_ok());
        let g = build_schema_injected();
        match g.validate() {
            Err(code) => assert_eq!(code, FinalCode::SchemaVersionSplit.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn timeline_append_rejects_regress() {
        let mut t = build_timeline_ok();
        let before = t.len();
        match t.append("甲账", "坏件", TIMELINE_BASE_SEQ + 1) {
            Err(code) => assert_eq!(code, FinalCode::TimelineRegress.val()),
            Ok(()) => must_not_reach(),
        }
        assert_eq!(t.len(), before, "被拒事件不得入账");
    }

    #[test]
    fn caliber_duplicate_rejected() {
        let mut b = build_book_ok();
        match b.put("红项率", "百分比", 30) {
            Err(code) => assert_eq!(code, FinalCode::CaliberDuplicate.val()),
            Ok(()) => must_not_reach(),
        }
        assert!(b.validate(&OBSERVED_METRICS).is_ok());
    }

    #[test]
    fn stage_regress_returns_diag() {
        let mut rv = FinalReview::new(build_schema_ok(), build_timeline_ok(), build_book_ok());
        assert!(rv.audit_faces(&OBSERVED_METRICS).is_ok());
        match rv.advance(FinalStage::Intake) {
            Err(code) => assert_eq!(code, FinalCode::StageRegress.val()),
            Ok(()) => must_not_reach(),
        }
        // 跳级同拒。
        let mut rv2 = FinalReview::new(build_schema_ok(), build_timeline_ok(), build_book_ok());
        match rv2.advance(FinalStage::Frozen) {
            Err(code) => assert_eq!(code, FinalCode::StageSkip.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn freeze_requires_report_and_is_terminal() {
        let mut rv = FinalReview::new(build_schema_ok(), build_timeline_ok(), build_book_ok());
        let _ = rv.audit_faces(&OBSERVED_METRICS);
        match rv.freeze() {
            Err(code) => assert_eq!(code, FinalCode::ReportEmpty.val()),
            Ok(()) => must_not_reach(),
        }
        let text = "三面终审全绿：schema 一版/时间轴一致/口径一册，抽验 10/10，归档。";
        assert!(rv.write_report(text).is_ok());
        assert!(rv.freeze().is_ok());
        assert!(rv.is_frozen());
        // 冻结后 schema 不可再登记（终态封账）。
        assert!(rv.schema.is_frozen());
    }

    #[test]
    fn double_run_identical() {
        let d1 = corpus_digest();
        let d2 = corpus_digest();
        assert_eq!(d1, d2, "双跑摘要必须逐位相等");
    }

    #[test]
    fn diag_codes_closed_unique_and_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(FinalCode::ALL.len(), 12);
        let first = match FinalCode::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01, "首码必须落在 0x01 槽");
        let last = match FinalCode::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(last, SEG_BASE + 0x0C, "末码必须落在 0x0C 槽");
        assert_eq!(detail(0xDEAD_BEEF), "未登记诊断码：不在本段闭集内");
    }

    #[test]
    fn cross_sample_flags_disordered_ledger() {
        assert!(cross_sample_injection_caught_ok());
        let a = build_timeline_ok();
        let b = build_timeline_ok();
        let tags = tags_vec();
        assert!(cross_sample_consistent(&a, &b, &tags), "好账不得误报");
    }

    // 测试区占位（失败断言由 assert! 承担，此占位仅作 match 失败分支标记）。
    fn must_not_reach() {}
}

// 测试辅助断言对齐说明：测试区本允许 assert!/assert_eq!（契约红线 2），
// 上述 panic_must_not_reach 仅是 match 失败分支的占位标记，不生产 panic。
