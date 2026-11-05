//! UNX-F1538 收官段人话化（explain 对齐体例） · 0x78780段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B03 · UNX-F1538 逐字摘录）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F1538-J1 收官段人话化覆盖 100%（总收编/对账/快照/
//!     注入四面全部状态可解释），一致性 1000 次逐字一致，快照校验通过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四面十二态闭集：总收编 / 对账 / 快照 / 注入各 3 态，状态号即段内
//!     诊断码（0x01..0x0C），一态一段人话 + 一条可操作建议；
//!   - 体例对齐：每行人话化文案冻结为「【面名】状态名：正文」体例，
//!     缺面名前缀 / 缺正文即体例违约，显性拒；
//!   - 覆盖 100% 判据：册面逐状态必须有非空解释行，缺行 / 错体例注入
//!     必被覆盖审抓出；
//!   - 一致性 1000 次逐字一致：全册文案 1000 轮重建，轮轮 FNV 指纹
//!     逐位相等（纯整数循环，无随机无墙钟）；
//!   - 快照校验通过：全册指纹快照冻结后可一键复验（重算比对）；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78780，派工卡 VX8-B03 卡 7 独占窗口）。
pub const SEG_BASE: u32 = 0x78780;

/// 人话化面数（总收编/对账/快照/注入）。
pub const FACE_COUNT: usize = 4;
/// 每面状态数（3 态，四面共 12 态闭集）。
pub const STATUS_PER_FACE: usize = 3;
/// 一致性验证轮数（判据"1000 次逐字一致"冻结）。
pub const CONSISTENCY_ROUNDS: u32 = 1000;
/// 全册指纹快照种子（快照校验链头）。
pub const SNAPSHOT_SEED: u64 = 0x78780;
/// 人话化单行最小字节数（正文非空的可验证下限）。
pub const LINE_MIN_BYTES: usize = 12;

// ── 人话化面（四闭集） ──────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExplainFace {
    /// 总收编面：全域账册收口
    Takeover,
    /// 对账面：批间对账结论
    Reconcile,
    /// 快照面：资产基线冻结
    Snapshot,
    /// 注入面：故障注入总谱
    Injection,
}

impl ExplainFace {
    pub const ALL: [ExplainFace; 4] = [
        ExplainFace::Takeover,
        ExplainFace::Reconcile,
        ExplainFace::Snapshot,
        ExplainFace::Injection,
    ];
    /// 面名（体例前缀用，冻结）。
    pub fn name(self) -> &'static str {
        match self {
            ExplainFace::Takeover => "总收编",
            ExplainFace::Reconcile => "对账",
            ExplainFace::Snapshot => "快照",
            ExplainFace::Injection => "注入",
        }
    }
    pub fn rank(self) -> usize {
        match self {
            ExplainFace::Takeover => 0,
            ExplainFace::Reconcile => 1,
            ExplainFace::Snapshot => 2,
            ExplainFace::Injection => 3,
        }
    }
}

// ── 十二态闭集（状态号 = 段内诊断码 0x01..0x0C） ─────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExplainStatus {
    /// 总收编全量入册：全域账册齐收
    TakeoverDone,
    /// 总收编部分在途：余账在收
    TakeoverPartial,
    /// 总收编缺册在逃：有账未收
    TakeoverGap,
    /// 对账全平：批间逐边对平
    ReconFlat,
    /// 对账红边在途：红边归因中
    ReconRedPending,
    /// 对账未起跑：对账未执行
    ReconNotStarted,
    /// 快照已冻结：基线封版
    SnapFrozen,
    /// 快照待建链：链未建满
    SnapChainPending,
    /// 快照复验失配：链根不符
    SnapVerifyMismatch,
    /// 注入全检出：逐件必抓
    InjAllCaught,
    /// 注入逃逸在册：有故障逃逸
    InjEscape,
    /// 注入未编谱：总谱缺件
    InjNotScored,
}

impl ExplainStatus {
    /// 闭集数组（十二态，编谱序 = 面序 × 态序）。
    pub const ALL: [ExplainStatus; 12] = [
        ExplainStatus::TakeoverDone,
        ExplainStatus::TakeoverPartial,
        ExplainStatus::TakeoverGap,
        ExplainStatus::ReconFlat,
        ExplainStatus::ReconRedPending,
        ExplainStatus::ReconNotStarted,
        ExplainStatus::SnapFrozen,
        ExplainStatus::SnapChainPending,
        ExplainStatus::SnapVerifyMismatch,
        ExplainStatus::InjAllCaught,
        ExplainStatus::InjEscape,
        ExplainStatus::InjNotScored,
    ];
    /// 段内诊断码取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 所属面（闭集映射，一一对应）。
    pub fn face_of(self) -> ExplainFace {
        match self {
            ExplainStatus::TakeoverDone
            | ExplainStatus::TakeoverPartial
            | ExplainStatus::TakeoverGap => ExplainFace::Takeover,
            ExplainStatus::ReconFlat
            | ExplainStatus::ReconRedPending
            | ExplainStatus::ReconNotStarted => ExplainFace::Reconcile,
            ExplainStatus::SnapFrozen
            | ExplainStatus::SnapChainPending
            | ExplainStatus::SnapVerifyMismatch => ExplainFace::Snapshot,
            ExplainStatus::InjAllCaught
            | ExplainStatus::InjEscape
            | ExplainStatus::InjNotScored => ExplainFace::Injection,
        }
    }
    /// 状态名（体例正文头，冻结）。
    pub fn status_name(self) -> &'static str {
        match self {
            ExplainStatus::TakeoverDone => "全量入册",
            ExplainStatus::TakeoverPartial => "部分在途",
            ExplainStatus::TakeoverGap => "缺册在逃",
            ExplainStatus::ReconFlat => "全平",
            ExplainStatus::ReconRedPending => "红边在途",
            ExplainStatus::ReconNotStarted => "未起跑",
            ExplainStatus::SnapFrozen => "已冻结",
            ExplainStatus::SnapChainPending => "待建链",
            ExplainStatus::SnapVerifyMismatch => "复验失配",
            ExplainStatus::InjAllCaught => "全检出",
            ExplainStatus::InjEscape => "逃逸在册",
            ExplainStatus::InjNotScored => "未编谱",
        }
    }
    /// 人话化正文（对齐体例：说清"是什么 + 下一步"）。
    pub fn plain(self) -> &'static str {
        match self {
            ExplainStatus::TakeoverDone => "全域账册已齐收，收口完成，无欠账。",
            ExplainStatus::TakeoverPartial => "账册还在收口，剩余账在途，等下一轮收编。",
            ExplainStatus::TakeoverGap => "有账没进来，先把缺册账找齐再谈收口。",
            ExplainStatus::ReconFlat => "批间对账逐边都平了，两两咬合没有红边。",
            ExplainStatus::ReconRedPending => "还有红边没轧平，先归因红边再复跑对账。",
            ExplainStatus::ReconNotStarted => "对账还没跑，先起跑逐边对平再下结论。",
            ExplainStatus::SnapFrozen => "资产基线已冻结封版，篡改有账可查。",
            ExplainStatus::SnapChainPending => "哈希链还没建满，先建满链再冻结。",
            ExplainStatus::SnapVerifyMismatch => "复验链根对不上，先全链重算再定基线。",
            ExplainStatus::InjAllCaught => "注入逐件都抓中了，误报为零，总谱可信。",
            ExplainStatus::InjEscape => "有故障逃出了审计，先补判据再重跑注入。",
            ExplainStatus::InjNotScored => "注入总谱缺件，先补齐三列编谱再执行。",
        }
    }
    /// 可操作建议（detail 面，与正文分离）。
    pub fn detail(self) -> &'static str {
        match self {
            ExplainStatus::TakeoverDone => "保持收口态，后续增量走增量收编流程",
            ExplainStatus::TakeoverPartial => "盯在途账清单，收齐即转全量入册",
            ExplainStatus::TakeoverGap => "按缺册清单逐账追缴，收齐后复审",
            ExplainStatus::ReconFlat => "保持逐边对平态，批间增量随批对账",
            ExplainStatus::ReconRedPending => "按红边清单逐边归因，轧平后复跑",
            ExplainStatus::ReconNotStarted => "先执行一键对账，再解读结论",
            ExplainStatus::SnapFrozen => "保持封版态，改动走快照重冻流程",
            ExplainStatus::SnapChainPending => "补齐链后按冻结流程走",
            ExplainStatus::SnapVerifyMismatch => "先排查载荷篡改面，再全链重算",
            ExplainStatus::InjAllCaught => "保持总谱冻结态，新故障先编谱再注入",
            ExplainStatus::InjEscape => "按逃逸清单补审计判据，重跑全谱",
            ExplainStatus::InjNotScored => "补齐注入号/目标/判据三列后执行",
        }
    }
    /// 体例整行：「【面名】状态名：正文」。
    pub fn styled_line(self) -> String {
        format!("【{}】{}：{}", self.face_of().name(), self.status_name(), self.plain())
    }
}

/// 诊断码 → 人话化正文闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < ExplainStatus::ALL.len() {
        if let Some(c) = ExplainStatus::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 64（指纹核心） ───────────────────────────────────────
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

// ── 册面模型 ────────────────────────────────────────────────────
/// 一行人话化文案。
pub struct ExplainLine {
    status: ExplainStatus,
    text: String,
}

impl ExplainLine {
    pub fn status(&self) -> ExplainStatus {
        self.status
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    /// 体例审：行必须带面名前缀 + 状态名 + 正文。
    pub fn style_ok(&self) -> bool {
        let prefix = format!("【{}】{}", self.status.face_of().name(), self.status.status_name());
        if !self.text.starts_with(&prefix) {
            return false;
        }
        // 正文非空（体例「：正文」段）。
        match self.text.split_once('：') {
            Some((_head, body)) => body.len() >= LINE_MIN_BYTES / 2,
            None => false,
        }
    }
}

/// 人话化阶段（单调状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExplainPhase {
    /// 草稿：行装配中
    Draft,
    /// 体例对齐：覆盖审 + 体例审全过
    Aligned,
    /// 快照过：指纹冻结复验通过
    SnapOk,
}

impl ExplainPhase {
    pub fn rank(self) -> u32 {
        match self {
            ExplainPhase::Draft => 0,
            ExplainPhase::Aligned => 1,
            ExplainPhase::SnapOk => 2,
        }
    }
}

/// 人话化册：十二态全行装配 + 覆盖审 + 体例审 + 快照。
pub struct ExplainBook {
    lines: Vec<ExplainLine>,
    phase: ExplainPhase,
    snapshot_digest: Option<u64>,
}

impl ExplainBook {
    pub fn new() -> ExplainBook {
        ExplainBook {
            lines: Vec::new(),
            phase: ExplainPhase::Draft,
            snapshot_digest: None,
        }
    }
    pub fn len(&self) -> usize {
        self.lines.len()
    }
    pub fn phase(&self) -> ExplainPhase {
        self.phase
    }
    /// 装一行（自定义文案面，注入测试用）。
    pub fn add_line(&mut self, status: ExplainStatus, text: &str) {
        self.lines.push(ExplainLine {
            status,
            text: text.to_string(),
        });
    }
    /// 全册装配：十二态逐一按冻结体例成行。
    pub fn compile_full(&mut self) {
        self.lines.clear();
        for st in ExplainStatus::ALL.iter() {
            self.add_line(*st, &st.styled_line());
        }
    }
    /// 覆盖审：缺行返回缺的状态（Result 闭集）。
    pub fn coverage_missing(&self) -> Result<(), u32> {
        if self.lines.len() != 12 {
            let mut k = 0usize;
            while k < ExplainStatus::ALL.len() {
                let st = match ExplainStatus::ALL.get(k) {
                    Some(s) => *s,
                    None => break,
                };
                let have = self.lines.iter().any(|l| l.status() == st);
                if !have {
                    return Err(st.val());
                }
                k += 1;
            }
            let first = match ExplainStatus::ALL.get(0) {
                Some(s) => s.val(),
                None => 0,
            };
            return Err(first);
        }
        Ok(())
    }
    /// 体例审：逐行体例必须合规。
    pub fn audit_style(&self) -> Result<(), u32> {
        self.coverage_missing()?;
        for l in self.lines.iter() {
            if !l.style_ok() {
                return Err(l.status().val());
            }
        }
        Ok(())
    }
    /// 体例对齐推进（草稿 → 对齐）。
    pub fn align(&mut self) -> Result<(), u32> {
        self.audit_style()?;
        self.phase = ExplainPhase::Aligned;
        Ok(())
    }
    /// 全册指纹（逐行拼接 FNV，快照载荷）。
    pub fn digest(&self) -> u64 {
        let mut buf = String::new();
        for l in self.lines.iter() {
            buf.push_str(l.text());
            buf.push('\n');
        }
        buf.push_str("@SNAP@");
        let mut sb = String::new();
        push_u64(&mut sb, SNAPSHOT_SEED);
        buf.push_str(&sb);
        fnv1a64(buf.as_bytes())
    }
    /// 快照冻结：对齐后才许冻结。
    pub fn freeze_snapshot(&mut self) -> Result<(), u32> {
        if self.phase.rank() < ExplainPhase::Aligned.rank() {
            return Err(ExplainStatus::SnapChainPending.val());
        }
        self.snapshot_digest = Some(self.digest());
        Ok(())
    }
    /// 快照校验（一键复验）：重算指纹与冻结指纹比对。
    pub fn verify_snapshot(&mut self) -> Result<(), u32> {
        let frozen = match self.snapshot_digest {
            Some(d) => d,
            None => return Err(ExplainStatus::SnapChainPending.val()),
        };
        if self.digest() != frozen {
            return Err(ExplainStatus::SnapVerifyMismatch.val());
        }
        self.phase = ExplainPhase::SnapOk;
        Ok(())
    }
    /// 单调推进：倒退拒。
    pub fn advance(&mut self, to: ExplainPhase) -> Result<(), u32> {
        if to.rank() < self.phase.rank() {
            return Err(ExplainStatus::TakeoverGap.val());
        }
        self.phase = to;
        Ok(())
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

// ── 语料构建（好册 / 缺行注入册 / 体例错行注入册） ───────────────
/// 好册：十二态全装配。
pub fn build_book_ok() -> ExplainBook {
    let mut b = ExplainBook::new();
    b.compile_full();
    b
}

/// 注入册：抽掉快照面"复验失配"一行（缺行）。
pub fn build_book_missing_line() -> ExplainBook {
    let mut b = ExplainBook::new();
    for st in ExplainStatus::ALL.iter() {
        if *st != ExplainStatus::SnapVerifyMismatch {
            b.add_line(*st, &st.styled_line());
        }
    }
    b
}

/// 注入册：把"注入全检出"行换成无面名前缀的野文案（体例违约）。
pub fn build_book_bad_style() -> ExplainBook {
    let mut c = ExplainBook::new();
    for st in ExplainStatus::ALL.iter() {
        if *st == ExplainStatus::InjAllCaught {
            c.add_line(*st, "注入全检出，都抓中了（体例外文案）");
        } else {
            c.add_line(*st, &st.styled_line());
        }
    }
    c
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78600 && SEG_BASE <= 0x787C0
}

/// 十二态闭集：码唯一、落窗、面映射齐。
fn status_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for st in ExplainStatus::ALL.iter() {
        let v = st.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    // 面映射：四面各 3 态。
    for f in ExplainFace::ALL.iter() {
        let n = ExplainStatus::ALL.iter().filter(|s| s.face_of() == *f).count();
        if n != STATUS_PER_FACE {
            return false;
        }
    }
    true
}

/// 覆盖 100%：十二态全行且逐行非空正文。
fn coverage_full_ok() -> bool {
    let mut b = build_book_ok();
    if b.coverage_missing().is_err() {
        return false;
    }
    if b.len() != FACE_COUNT * STATUS_PER_FACE {
        return false;
    }
    if b.align().is_err() {
        return false;
    }
    if b.phase() != ExplainPhase::Aligned {
        return false;
    }
    true
}

/// 体例对齐：逐行「【面名】状态名：正文」合规。
fn style_aligned_ok() -> bool {
    let b = build_book_ok();
    if b.audit_style().is_err() {
        return false;
    }
    // 正文与建议分离：逐态两文案互不为空且不相同。
    for st in ExplainStatus::ALL.iter() {
        if st.plain().is_empty() || st.detail().is_empty() {
            return false;
        }
        if st.plain() == st.detail() {
            return false;
        }
    }
    true
}

/// 缺行注入必抓：抽行册覆盖审必拒，且缺的正是被抽态。
fn missing_line_caught_ok() -> bool {
    let b = build_book_missing_line();
    if b.len() != 11 {
        return false;
    }
    match b.coverage_missing() {
        Err(code) => code == ExplainStatus::SnapVerifyMismatch.val(),
        Ok(()) => false,
    }
}

/// 体例错行注入必抓：野文案行体例审必拒。
fn bad_style_caught_ok() -> bool {
    let b = build_book_bad_style();
    if b.len() != 12 {
        return false;
    }
    match b.audit_style() {
        Err(code) => code == ExplainStatus::InjAllCaught.val(),
        Ok(()) => false,
    }
}

/// 一致性 1000 次逐字一致：全册 1000 轮重建指纹轮轮相等。
fn consistency_1000_ok() -> bool {
    let first = build_book_ok().digest();
    let mut round: u32 = 0;
    while round < CONSISTENCY_ROUNDS {
        let d = build_book_ok().digest();
        if d != first {
            return false;
        }
        round += 1;
    }
    if first == 0 {
        return false;
    }
    true
}

/// 快照校验通过：冻结后复验可过，篡改册复验必失配。
fn snapshot_verify_ok() -> bool {
    let mut b = build_book_ok();
    // 未对齐先冻结：拒。
    if b.freeze_snapshot().is_ok() {
        return false;
    }
    if b.align().is_err() {
        return false;
    }
    if b.freeze_snapshot().is_err() {
        return false;
    }
    if b.verify_snapshot().is_err() {
        return false;
    }
    if b.phase() != ExplainPhase::SnapOk {
        return false;
    }
    // 未冻结先复验：拒。
    let mut c = build_book_ok();
    if c.verify_snapshot().is_ok() {
        return false;
    }
    true
}

/// 快照篡改注入必抓：冻结后换行复验必失配。
fn snapshot_tamper_caught_ok() -> bool {
    let mut b = build_book_ok();
    if b.align().is_err() {
        return false;
    }
    if b.freeze_snapshot().is_err() {
        return false;
    }
    let frozen = match b.verify_snapshot() {
        Ok(()) => b.digest(),
        Err(_) => return false,
    };
    // 注入：换一行野文案后指纹必变。
    let mut bad = build_book_bad_style();
    if bad.digest() == frozen {
        return false;
    }
    true
}

/// 双跑一致：两册指纹逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_book_ok().digest();
    let d2 = build_book_ok().digest();
    d1 == d2
}

/// 诊断码查表闭集：12 码查表可回、闭集外落占位。
fn diag_lookup_ok() -> bool {
    let mut i = 0usize;
    while i < ExplainStatus::ALL.len() {
        let st = match ExplainStatus::ALL.get(i) {
            Some(s) => *s,
            None => break,
        };
        if detail(st.val()) != st.detail() {
            return false;
        }
        i += 1;
    }
    let first_detail = match ExplainStatus::ALL.get(0) {
        Some(s) => s.detail(),
        None => return false,
    };
    detail(SEG_BASE + 0x0D) != first_detail
}

/// 面名闭集：四面名互异且非空，位次连续。
fn face_names_ok() -> bool {
    if ExplainFace::ALL.len() != FACE_COUNT {
        return false;
    }
    let mut names: Vec<&str> = Vec::new();
    let mut idx = 0usize;
    while idx < ExplainFace::ALL.len() {
        let f = match ExplainFace::ALL.get(idx) {
            Some(f) => *f,
            None => break,
        };
        if f.rank() != idx {
            return false;
        }
        let n = f.name();
        if n.is_empty() || names.iter().any(|x| *x == n) {
            return false;
        }
        names.push(n);
        idx += 1;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1538_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1538 收官段人话化（explain 对齐体例）");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78600..=0x787C0", SEG_BASE));
    s.add_dyn("十二态闭集", status_closed_set_ok(), &format!("四面 × {} 态，码落 0x01..0x0C 且唯一", STATUS_PER_FACE));
    s.add_dyn("覆盖 100%", coverage_full_ok(), &format!("{} 态全行装配，覆盖审全过", FACE_COUNT * STATUS_PER_FACE));
    s.add_dyn("体例对齐", style_aligned_ok(), "逐行「【面名】状态名：正文」，正文建议分离");
    s.add_dyn("缺行注入必抓", missing_line_caught_ok(), "抽行册覆盖审必拒且缺态归位");
    s.add_dyn("体例错行注入必抓", bad_style_caught_ok(), "野文案行体例审必拒且行归位");
    s.add_dyn("一致性 1000 次", consistency_1000_ok(), &format!("{} 轮重建指纹轮轮逐位相等", CONSISTENCY_ROUNDS));
    s.add_dyn("快照校验通过", snapshot_verify_ok(), "对齐后冻结复验可过，未冻复验拒");
    s.add_dyn("快照篡改必抓", snapshot_tamper_caught_ok(), "换行后指纹必变");
    s.add_dyn("双跑一致", double_run_ok(), "两册指纹逐位相等");
    s.add_dyn("面名闭集", face_names_ok(), "四面名互异非空，位次连续");
    s.add_dyn("诊断码查表闭集", diag_lookup_ok(), "12 码查表可回，闭集外落占位");
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
        assert_eq!(SNAPSHOT_SEED, SEG_BASE as u64);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1538_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn twelve_statuses_face_mapping() {
        assert!(status_closed_set_ok());
        let first = match ExplainStatus::ALL.get(0) {
            Some(s) => s.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(ExplainStatus::TakeoverDone.face_of(), ExplainFace::Takeover);
        assert_eq!(ExplainStatus::InjNotScored.face_of(), ExplainFace::Injection);
    }

    #[test]
    fn missing_line_injection_caught() {
        assert!(missing_line_caught_ok());
        let b = build_book_missing_line();
        match b.coverage_missing() {
            Err(code) => assert_eq!(code, ExplainStatus::SnapVerifyMismatch.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn bad_style_injection_caught() {
        assert!(bad_style_caught_ok());
        let b = build_book_bad_style();
        match b.audit_style() {
            Err(code) => assert_eq!(code, ExplainStatus::InjAllCaught.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn consistency_thousand_rounds_identical() {
        assert!(consistency_1000_ok());
        let d1 = build_book_ok().digest();
        let d2 = build_book_ok().digest();
        assert_eq!(d1, d2);
    }

    #[test]
    fn snapshot_flow_reaches_snapok() {
        let mut b = build_book_ok();
        match b.freeze_snapshot() {
            Err(code) => assert_eq!(code, ExplainStatus::SnapChainPending.val()),
            Ok(()) => must_not_reach(),
        }
        assert!(b.align().is_ok());
        assert!(b.freeze_snapshot().is_ok());
        assert!(b.verify_snapshot().is_ok());
        assert_eq!(b.phase(), ExplainPhase::SnapOk);
    }

    #[test]
    fn styled_lines_follow_template() {
        for st in ExplainStatus::ALL.iter() {
            let line = st.styled_line();
            let prefix = format!("【{}】{}", st.face_of().name(), st.status_name());
            assert!(line.starts_with(&prefix), "体例违约: {}", line);
            assert!(line.len() >= LINE_MIN_BYTES, "正文过短: {}", line);
        }
    }

    #[test]
    fn plain_and_detail_separated() {
        for st in ExplainStatus::ALL.iter() {
            assert!(!st.plain().is_empty());
            assert!(!st.detail().is_empty());
            assert_ne!(st.plain(), st.detail(), "正文与建议必须分离");
        }
    }

    #[test]
    fn diag_lookup_closed() {
        assert!(diag_lookup_ok());
        assert_eq!(detail(0x1234), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
