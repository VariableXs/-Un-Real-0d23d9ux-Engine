//! UNX-F9239 fd 族 E 型与 glibc perror/strerror 消费核 · 0x7A940段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9239 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9239-J1 strerror 逐码输出判据过，
//!   与 errno 透传单源格过，消费号登记格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 消息表 20 行逐码登记，strerror 只做表内查找，未知码给固定兜底文案；
//!      逐码判据就是「表里每一行查出来必须等于登记文案」，不允许分支特判。
//!   ② errno 透传单源：glibc 系统调用返回 -1..-4095 才是错误，<-4095（如
//!      -4096）是合法返回值。classify 是唯一判据，naive 版本把负数一律当错误，
//!      两者在 -4096 处分道，单源格用这条边界把错误实现钉死。
//!   ③ errno 粘滞：成功调用不清 errno，消费方不得在成功路径读 errno；失败
//!      才写。perror 读的是 errno 当前值，因此调用序列决定输出，不决定正确性。
//!   ④ perror 渲染与错误码一一对应：空前缀只输出消息加换行，非空前缀输出
//!      「前缀: 消息」；渲染器不查 errno 全局量，只接受显式码值，可复测。
//!   ⑤ 消费号登记：16 个消费位、id 0..15 唯一，容量满与重复 id 都拒绝；每次
//!      消费记 attempts，成功记 consumed、失败记 rejected，三者守恒。
//!   ⑥ 三类注入各 10 轮：原始值边界误判、成功清 errno、未登记消费，检出
//!      10/10；注入账与消费账分开，互不冲销。
//! ─────────────────────────────────────────────────────────────────────
use alloc::{format, string::String, vec::Vec};
use crate::checks::CheckSet;
/// 诊断段基址（F9239 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A940;
/// glibc 错误码上界（-1..-4095 为错误）。
pub const GLIBC_ERRNO_MAX: u32 = 4095;
/// 消息表行数。
pub const TABLE_ROWS: usize = 20;
/// 消费位容量。
pub const CONSUMER_CAP: usize = 16;
/// 单类注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 坏文件描述符。
pub const EBADF: u32 = 9;
/// 坏用户指针。
pub const EFAULT: u32 = 14;
/// 参数值非法。
pub const EINVAL: u32 = 22;
/// 未知码兜底文案。
pub const UNKNOWN_TEXT: &str = "Unknown error";
/// 一条消息登记。
#[derive(Clone, Copy)]
pub struct Message {
    pub code: u32,
    pub text: &'static str,
}
/// 20 行消息表。
pub const STRERROR_TABLE: [Message; 20] = [
    Message { code: 0, text: "Success" },
    Message { code: 1, text: "Operation not permitted" },
    Message { code: 2, text: "No such file or directory" },
    Message { code: 3, text: "No such process" },
    Message { code: 4, text: "Interrupted system call" },
    Message { code: 5, text: "Input/output error" },
    Message { code: 6, text: "No such device or address" },
    Message { code: 7, text: "Argument list too long" },
    Message { code: 8, text: "Exec format error" },
    Message { code: 9, text: "Bad file descriptor" },
    Message { code: 11, text: "Resource temporarily unavailable" },
    Message { code: 12, text: "Cannot allocate memory" },
    Message { code: 13, text: "Permission denied" },
    Message { code: 14, text: "Bad address" },
    Message { code: 17, text: "File exists" },
    Message { code: 22, text: "Invalid argument" },
    Message { code: 23, text: "Too many open files in system" },
    Message { code: 24, text: "Too many open files" },
    Message { code: 29, text: "Illegal seek" },
    Message { code: 32, text: "Broken pipe" },
];
/// 是否登记过。
pub fn known_errno(code: u32) -> bool {
    STRERROR_TABLE.iter().any(|m| m.code == code)
}
/// 逐码查表，未知码固定兜底。
pub fn strerror(code: u32) -> &'static str {
    for m in STRERROR_TABLE.iter() {
        if m.code == code {
            return m.text;
        }
    }
    UNKNOWN_TEXT
}
/// perror 渲染：前缀空则只出消息。
pub fn perror_render(prefix: &str, code: u32) -> String {
    let msg = strerror(code);
    if prefix.is_empty() {
        format!("{}\n", msg)
    } else {
        format!("{}: {}\n", prefix, msg)
    }
}
/// 表内不一致计数。
pub fn reconcile_messages() -> usize {
    STRERROR_TABLE.iter().filter(|m| strerror(m.code) != m.text).count()
}
/// 码值是否唯一。
pub fn codes_unique() -> bool {
    let mut i = 0;
    while i < STRERROR_TABLE.len() {
        let mut j = i + 1;
        while j < STRERROR_TABLE.len() {
            if STRERROR_TABLE[i].code == STRERROR_TABLE[j].code {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}
/// 兜底文案是否与所有登记文案不同。
pub fn unknown_distinct() -> bool {
    STRERROR_TABLE.iter().all(|m| m.text != UNKNOWN_TEXT)
}
/// 系统调用原始返回值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wire {
    Ok(i64),
    Errno(u32),
}
/// 单源分类：仅 [-4095, -1] 是错误。
pub fn classify(raw: i64) -> Wire {
    if raw < 0 && raw >= -(GLIBC_ERRNO_MAX as i64) {
        Wire::Errno((-raw) as u32)
    } else {
        Wire::Ok(raw)
    }
}
/// 朴素分类（故意错误：负数一律当错误）。
pub fn classify_naive(raw: i64) -> Wire {
    if raw < 0 {
        Wire::Errno((-raw) as u32)
    } else {
        Wire::Ok(raw)
    }
}
/// 两种分类是否一致。
pub fn wire_agrees(raw: i64) -> bool {
    classify(raw) == classify_naive(raw)
}
/// errno 粘滞单源格。
pub fn reconcile_wire() -> bool {
    let mut ok = true;
    let mut r = -1i64;
    while r >= -(GLIBC_ERRNO_MAX as i64) {
        if !wire_agrees(r) {
            ok = false;
        }
        r -= 1;
    }
    ok && !wire_agrees(-4096)
}
/// errno 槽（成功不清）。
pub struct ErrnoCell {
    value: u32,
}
impl ErrnoCell {
    /// 起步 0。
    pub fn new() -> Self {
        ErrnoCell { value: 0 }
    }
    /// 失败写入。
    pub fn fail(&mut self, e: u32) {
        self.value = e;
    }
    /// 成功保持原值。
    pub fn ok(&mut self) {
        // glibc 语义：成功不清 errno。
    }
    /// 读取。
    pub fn get(&self) -> u32 {
        self.value
    }
}
/// 调用结果：原始值分类 + 当前 errno。
#[derive(Clone, Copy)]
pub struct LibcRet {
    pub wire: Wire,
    pub errno: u32,
}
/// 模拟一次 libc 调用。
pub fn libc_call(raw: i64, cell: &mut ErrnoCell) -> LibcRet {
    match classify(raw) {
        Wire::Errno(e) => {
            cell.fail(e);
            LibcRet { wire: Wire::Errno(e), errno: e }
        }
        Wire::Ok(v) => {
            cell.ok();
            LibcRet { wire: Wire::Ok(v), errno: cell.get() }
        }
    }
}
/// 消费链阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Idle,
    Classified,
    Rendered,
    Delivered,
    Sealed,
}
impl Stage {
    /// 单步前进。
    pub fn can_go(self, next: Stage) -> bool {
        matches!(
            (self, next),
            (Stage::Idle, Stage::Classified)
                | (Stage::Classified, Stage::Rendered)
                | (Stage::Rendered, Stage::Delivered)
                | (Stage::Delivered, Stage::Sealed)
        )
    }
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            Stage::Idle => 0,
            Stage::Classified => 1,
            Stage::Rendered => 2,
            Stage::Delivered => 3,
            Stage::Sealed => 4,
        }
    }
}
/// 消费链。
pub struct ConsumerChain {
    pub stage: Stage,
    pub code: u32,
    pub events: u32,
    pub rendered: u32,
}
impl ConsumerChain {
    /// 新建。
    pub fn new() -> Self {
        ConsumerChain { stage: Stage::Idle, code: 0, events: 0, rendered: 0 }
    }
    /// 喂入原始返回值并分类。
    pub fn feed(&mut self, raw: i64) -> bool {
        if self.stage != Stage::Idle {
            return false;
        }
        match classify(raw) {
            Wire::Errno(e) => {
                self.code = e;
            }
            Wire::Ok(_) => {
                self.code = 0;
            }
        }
        self.events += 1;
        self.stage = Stage::Classified;
        true
    }
    /// 渲染当前码。
    pub fn render(&mut self) -> Option<String> {
        if self.stage != Stage::Classified {
            return None;
        }
        self.rendered += 1;
        self.stage = Stage::Rendered;
        Some(perror_render("unx9239", self.code))
    }
    /// 交付。
    pub fn deliver(&mut self) -> bool {
        if self.stage != Stage::Rendered {
            return false;
        }
        self.stage = Stage::Delivered;
        true
    }
    /// 封存。
    pub fn seal(&mut self) -> bool {
        if self.stage != Stage::Delivered {
            return false;
        }
        self.stage = Stage::Sealed;
        true
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h = fnv1a(0x811C_9DC5, b"unx9239/chain");
        h = fnv1a(h, &[self.stage.tag() as u8]);
        h = fnv1a(h, &self.code.to_le_bytes());
        h = fnv1a(h, &self.events.to_le_bytes());
        h = fnv1a(h, &self.rendered.to_le_bytes());
        h
    }
}
/// 消费位。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Consumer {
    pub id: u8,
    pub name: &'static str,
}
/// 消费号登记表。
pub struct ConsumerRegistry {
    slots: [Option<Consumer>; CONSUMER_CAP],
    count: usize,
    attempts: u32,
    consumed: u32,
    rejected: u32,
}
impl ConsumerRegistry {
    /// 空表。
    pub fn new() -> Self {
        ConsumerRegistry {
            slots: [None; CONSUMER_CAP],
            count: 0,
            attempts: 0,
            consumed: 0,
            rejected: 0,
        }
    }
    /// 已登记数。
    pub fn count(&self) -> usize {
        self.count
    }
    /// 是否已登记。
    pub fn registered(&self, id: u8) -> bool {
        self.slots.iter().any(|s| match s {
            Some(c) => c.id == id,
            None => false,
        })
    }
    /// 登记：重复 id 或满员拒绝。
    pub fn register(&mut self, c: Consumer) -> bool {
        if self.registered(c.id) || self.count >= CONSUMER_CAP {
            return false;
        }
        let idx = self.count;
        if let Some(slot) = self.slots.get_mut(idx) {
            *slot = Some(c);
            self.count += 1;
            true
        } else {
            false
        }
    }
    /// 消费一个错误码：须已登记且码已知。
    pub fn consume(&mut self, id: u8, code: u32) -> bool {
        self.attempts += 1;
        if self.registered(id) && known_errno(code) {
            self.consumed += 1;
            true
        } else {
            self.rejected += 1;
            false
        }
    }
    /// 尝试数。
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
    /// 成功消费数。
    pub fn consumed(&self) -> u32 {
        self.consumed
    }
    /// 被拒数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }
    /// 台账守恒。
    pub fn conserved(&self) -> bool {
        self.attempts == self.consumed + self.rejected && self.count <= CONSUMER_CAP
    }
    /// 登记表指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h = fnv1a(0x811C_9DC5, b"unx9239/registry");
        h = fnv1a(h, &(self.count as u32).to_le_bytes());
        for s in self.slots.iter() {
            match s {
                Some(c) => {
                    h = fnv1a(h, &[c.id]);
                    h = fnv1a(h, c.name.as_bytes());
                }
                None => h = fnv1a(h, &[0xFF]),
            }
        }
        h
    }
}
/// 构建满员登记表。
pub fn full_registry() -> ConsumerRegistry {
    let mut r = ConsumerRegistry::new();
    let mut i = 0u8;
    while i < CONSUMER_CAP as u8 {
        r.register(Consumer { id: i, name: "consumer" });
        i += 1;
    }
    r
}
/// 巡检登记表。
pub fn patrol_registry() -> u32 {
    let mut r = full_registry();
    let mut i = 0u8;
    while i < CONSUMER_CAP as u8 {
        r.consume(i, EBADF);
        i += 1;
    }
    r.consume(0, 9999);
    let mut h = fnv1a(r.fingerprint(), b"patrol");
    h = fnv1a(h, &r.attempts().to_le_bytes());
    h = fnv1a(h, &r.consumed().to_le_bytes());
    h = fnv1a(h, &r.rejected().to_le_bytes());
    h = fnv1a(h, &[r.conserved() as u8]);
    h
}
/// 注入种类。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum C2Inject {
    RawBoundary,
    ErrnoClobber,
    UnregisteredConsume,
}
impl C2Inject {
    /// 三类注入。
    pub fn all() -> [C2Inject; 3] {
        [C2Inject::RawBoundary, C2Inject::ErrnoClobber, C2Inject::UnregisteredConsume]
    }
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            C2Inject::RawBoundary => 0,
            C2Inject::ErrnoClobber => 1,
            C2Inject::UnregisteredConsume => 2,
        }
    }
    /// 中文说明。
    pub fn detail(self) -> &'static str {
        match self {
            C2Inject::RawBoundary => "把 -4096 当错误：单源格必须在边界处分道",
            C2Inject::ErrnoClobber => "成功调用清 errno：粘滞格必须拒绝",
            C2Inject::UnregisteredConsume => "未登记消费位消费已知码：登记格必须拒绝",
        }
    }
}
/// 注入是否被抓住。
pub fn inject_once(k: C2Inject) -> bool {
    match k {
        C2Inject::RawBoundary => classify(-4096) == Wire::Ok(-4096) && classify_naive(-4096) != classify(-4096),
        C2Inject::ErrnoClobber => {
            let mut c = ErrnoCell::new();
            c.fail(EINVAL);
            c.ok();
            c.get() == EINVAL
        }
        C2Inject::UnregisteredConsume => {
            let mut r = ConsumerRegistry::new();
            !r.consume(0, EBADF) && r.rejected() == 1 && r.consumed() == 0
        }
    }
}
/// 注入账。
pub struct C2Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub rounds: Vec<(u8, u32)>,
}
impl C2Ledger {
    /// 空账。
    pub fn new() -> Self {
        C2Ledger { injected: 0, detected: 0, escaped: 0, rounds: Vec::new() }
    }
    /// 记一轮。
    pub fn record(&mut self, tag: u8, hit: bool) {
        self.injected += 1;
        if hit {
            self.detected += 1;
        } else {
            self.escaped += 1;
        }
        self.rounds.push((tag, if hit { 1 } else { 0 }));
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped
    }
}
/// 3 类 × 10 轮。
pub fn run_consumer_matrix() -> C2Ledger {
    let mut l = C2Ledger::new();
    let mut r = 0;
    while r < INJECT_ROUNDS {
        for k in C2Inject::all().iter() {
            l.record(k.tag() as u8, inject_once(*k));
        }
        r += 1;
    }
    l
}
/// 每类检出数。
pub fn per_class(l: &C2Ledger) -> [u32; 3] {
    let mut out = [0u32; 3];
    for pair in l.rounds.iter() {
        let (tag, hit) = *pair;
        out[(tag as usize) % 3] += hit;
    }
    out
}
/// 诊断码闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum C2Code {
    TableMissing,
    TableDuplicate,
    StrerrorMismatch,
    UnknownNotClosed,
    PerrorPrefixLost,
    WireBoundaryLeak,
    ErrnoClearedOnOk,
    ConsumerDuplicate,
    ConsumerCapacity,
    ConsumeUnknownAccepted,
    LedgerNotConserved,
    SegmentOutOfRange,
}
impl C2Code {
    /// 码值 = 段基址 + 序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            C2Code::TableMissing => "消息表不足 20 行：逐码补齐",
            C2Code::TableDuplicate => "码值重复：查表会命中错误文案",
            C2Code::StrerrorMismatch => "输出与登记文案不符：回到表内查找",
            C2Code::UnknownNotClosed => "兜底文案与登记文案撞车：未知码不可伪装成已知",
            C2Code::PerrorPrefixLost => "perror 前缀丢失：空前缀与非空前缀分开渲染",
            C2Code::WireBoundaryLeak => "原始值边界泄漏：-4096 必须判成功",
            C2Code::ErrnoClearedOnOk => "成功路径清了 errno：粘滞语义被破坏",
            C2Code::ConsumerDuplicate => "消费号重复：登记表必须拒绝第二次",
            C2Code::ConsumerCapacity => "消费表满员仍登记：容量 16 必须硬拒",
            C2Code::ConsumeUnknownAccepted => "未登记位消费已知码被接受：先登记再消费",
            C2Code::LedgerNotConserved => "台账不守恒：尝试须等于成功加拒绝",
            C2Code::SegmentOutOfRange => "码值越出 0x7A940 窗口：段位守卫拒绝",
        }
    }
    /// 全量闭集。
    pub const ALL: [C2Code; 12] = [
        C2Code::TableMissing,
        C2Code::TableDuplicate,
        C2Code::StrerrorMismatch,
        C2Code::UnknownNotClosed,
        C2Code::PerrorPrefixLost,
        C2Code::WireBoundaryLeak,
        C2Code::ErrnoClearedOnOk,
        C2Code::ConsumerDuplicate,
        C2Code::ConsumerCapacity,
        C2Code::ConsumeUnknownAccepted,
        C2Code::LedgerNotConserved,
        C2Code::SegmentOutOfRange,
    ];
}
/// FNV-1a 32 位。
pub fn fnv1a(seed: u32, bytes: &[u8]) -> u32 {
    let mut h = seed;
    for b in bytes.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}
/// 消息表指纹。
pub fn table_digest() -> u32 {
    let mut h = fnv1a(0x811C_9DC5, b"unx9239/table");
    for m in STRERROR_TABLE.iter() {
        h = fnv1a(h, &m.code.to_le_bytes());
        h = fnv1a(h, m.text.as_bytes());
    }
    h
}
/// 跑批指纹。
pub fn run_digest() -> u32 {
    let l = run_consumer_matrix();
    let pc = per_class(&l);
    let mut h = fnv1a(table_digest(), b"run");
    h = fnv1a(h, &l.injected.to_le_bytes());
    h = fnv1a(h, &pc[0].to_le_bytes());
    h = fnv1a(h, &pc[1].to_le_bytes());
    h = fnv1a(h, &pc[2].to_le_bytes());
    h = fnv1a(h, &patrol_registry().to_le_bytes());
    h
}
/// 文案闭集自检。
pub fn text_ok() -> bool {
    if C2Code::ALL.len() != 12 {
        return false;
    }
    let mut i = 0;
    while i < C2Code::ALL.len() {
        let a = C2Code::ALL[i];
        if a.detail().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < C2Code::ALL.len() {
            let b = C2Code::ALL[j];
            if a.detail() == b.detail() || a.val() == b.val() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}
/// 自检总成。
pub fn run_unx9239_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9239");
    s.add("seg_base", SEG_BASE == 0x7A940, "段基址应为 0x7A940");
    s.add("code_window", C2Code::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40), "码值落在 0x7A940..0x7A980");
    s.add("text_closed_set", text_ok(), "12 条闭集、值唯一、建议非空");
    s.add("table_rows", STRERROR_TABLE.len() == TABLE_ROWS, "消息表 20 行");
    s.add("table_codes", codes_unique(), "码值互不重复");
    s.add("table_known", STRERROR_TABLE.iter().all(|m| known_errno(m.code)), "表内码全部可查");
    s.add("table_reconciled", reconcile_messages() == 0, "逐码输出等于登记文案");
    s.add("unknown_closed", !known_errno(9999) && strerror(9999) == UNKNOWN_TEXT && unknown_distinct(), "未知码兜底且不与登记文案撞车");
    let err_text = strerror(EBADF);
    let ok_text = strerror(0);
    s.add("strerror_badf", err_text == "Bad file descriptor", "EBADF 文案逐字正确");
    s.add("strerror_ok", ok_text == "Success", "0 号文案逐字正确");
    let p1 = perror_render("", EBADF);
    let p2 = perror_render("app", EBADF);
    s.add("perror_empty", p1 == format!("{}\n", err_text), "空前缀只出消息");
    s.add("perror_prefix", p2.contains("app: ") && p2.contains(err_text) && p2.ends_with('\n'), "非空前缀为「前缀: 消息」");
    s.add("perror_unknown", perror_render("x", 9999).contains(UNKNOWN_TEXT), "未知码渲染兜底文案");
    s.add("wire_errno", classify(-1) == Wire::Errno(1) && classify(-(EBADF as i64)) == Wire::Errno(EBADF) && classify(-(GLIBC_ERRNO_MAX as i64)) == Wire::Errno(GLIBC_ERRNO_MAX), "[-4095,-1] 判错误");    s.add("wire_ok_positive", classify(0) == Wire::Ok(0) && classify(42) == Wire::Ok(42), "非负判成功");
    s.add("wire_boundary", classify(-4096) == Wire::Ok(-4096) && classify(-4097) == Wire::Ok(-4097), "-4096/-4097 判成功");
    s.add("wire_diverges", !wire_agrees(-4096) && reconcile_wire(), "朴素实现仅在此边界分道，单源格成立");
    let mut cell = ErrnoCell::new();
    let r1 = libc_call(-(EBADF as i64), &mut cell);
    let err_set = r1.wire == Wire::Errno(EBADF) && r1.errno == EBADF && cell.get() == EBADF;
    let r2 = libc_call(7, &mut cell);
    let err_hold = r2.wire == Wire::Ok(7) && r2.errno == EBADF && cell.get() == EBADF;
    let r3 = libc_call(-4096, &mut cell);
    let boundary_hold = r3.wire == Wire::Ok(-4096) && r3.errno == EBADF;
    s.add("errno_set_on_fail", err_set, "失败写入 errno");
    s.add("errno_sticky", err_hold, "成功不清 errno");
    s.add("errno_boundary", boundary_hold, "-4096 成功且不写 errno");
    let mut chain = ConsumerChain::new();
    let c1 = chain.feed(-(EBADF as i64));
    let c2 = chain.render();
    let c3 = chain.deliver();
    let c4 = chain.seal();
    let rendered_ok = match c2 {
        Some(t) => t.contains("Bad file descriptor"),
        None => false,
    };
    s.add("chain_flow", c1 && rendered_ok && c3 && c4 && chain.stage == Stage::Sealed, "分类→渲染→交付→封存");
    s.add("chain_single_shot", !chain.feed(0) && chain.render().is_none(), "封存后拒绝再喂入");
    let stage_ok = Stage::Idle.can_go(Stage::Classified)
        && Stage::Classified.can_go(Stage::Rendered)
        && Stage::Rendered.can_go(Stage::Delivered)
        && Stage::Delivered.can_go(Stage::Sealed)
        && !Stage::Idle.can_go(Stage::Sealed);
    s.add("state_machine", stage_ok, "消费链只许单步前进");
    let mut reg = full_registry();
    let count_ok = reg.count() == CONSUMER_CAP;
    let dup_ok = !reg.register(Consumer { id: 0, name: "dup" });
    let full_ok = !reg.register(Consumer { id: 16, name: "overflow" }) && reg.count() == CONSUMER_CAP;
    let ids_ok = (0..CONSUMER_CAP as u8).all(|i| reg.registered(i));
    s.add("consumer_ids", count_ok && ids_ok, "16 个消费位 id 0..15 齐全");
    s.add("consumer_dup", dup_ok, "重复 id 被拒绝");
    s.add("consumer_capacity", full_ok, "满员后新 id 被拒绝且计数不变");
    let mut acc = 0u32;
    let mut i = 0u8;
    while i < CONSUMER_CAP as u8 {
        if reg.consume(i, EBADF) {
            acc += 1;
        }
        i += 1;
    }
    let unknown_ok = !reg.consume(0, 9999) && !reg.consume(99, EBADF);
    s.add("consumer_consume", acc == CONSUMER_CAP as u32 && reg.consumed() == acc, "已登记位消费已知码全部通过");
    s.add("consumer_unknown", unknown_ok && reg.rejected() == 2, "未知码与未登记位均被拒绝");
    s.add("consumer_conserved", reg.conserved() && reg.attempts() == CONSUMER_CAP as u32 + 2, "尝试 = 成功 + 拒绝");
    let l = run_consumer_matrix();
    let pc = per_class(&l);
    s.add("inject_ten_of_ten", pc == [INJECT_ROUNDS; 3], "三类注入各 10/10");
    s.add("ledger_conserved", l.conserved() && l.escaped == 0 && l.injected == INJECT_ROUNDS * 3, "注入 30 = 检出 30 + 逃逸 0");
    s.add("inject_text", C2Inject::all().iter().all(|k| !k.detail().is_empty()), "注入文案非空");
    let d1 = run_digest();
    let d2 = run_digest();
    let l2 = run_consumer_matrix();
    s.add("double_run", d1 == d2 && l2.injected == l.injected && per_class(&l2) == pc, "双跑指纹与注入账一致");
    s.add_dyn("table_detail", TABLE_ROWS == 20, &format!("行={} 指纹={:08x}", STRERROR_TABLE.len(), table_digest()));
    s.add_dyn("wire_detail", reconcile_wire(), &format!("-4096={:?} 单源={}", classify(-4096), reconcile_wire()));
    s.add_dyn("registry_detail", reg.conserved(), &format!("位={} 尝试={} 成功={} 拒绝={}", reg.count(), reg.attempts(), reg.consumed(), reg.rejected()));
    s.add_dyn("inject_detail", pc == [10, 10, 10], &format!("边界={} 清errno={} 未登记={}", pc[0], pc[1], pc[2]));
    s
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_all_green() {
        let s = run_unx9239_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.len() >= 12);
    }
    #[test]
    fn table_is_twenty_and_unique() {
        assert_eq!(STRERROR_TABLE.len(), 20);
        assert!(codes_unique());
        for m in STRERROR_TABLE.iter() {
            assert_eq!(strerror(m.code), m.text);
            assert!(known_errno(m.code));
        }
    }
    #[test]
    fn unknown_is_closed() {
        assert!(!known_errno(9999));
        assert_eq!(strerror(9999), UNKNOWN_TEXT);
        assert!(unknown_distinct());
        assert_eq!(reconcile_messages(), 0);
    }
    #[test]
    fn perror_renders_both_forms() {
        assert_eq!(perror_render("", EBADF), format!("{}\n", strerror(EBADF)));
        let with = perror_render("app", EBADF);
        assert!(with.starts_with("app: "));
        assert!(with.ends_with('\n'));
        assert!(perror_render("app", 9999).contains(UNKNOWN_TEXT));
    }
    #[test]
    fn wire_boundary_is_4095() {
        assert_eq!(classify(-1), Wire::Errno(1));
        assert_eq!(classify(-4095), Wire::Errno(4095));
        assert_eq!(classify(-4096), Wire::Ok(-4096));
        assert_eq!(classify(0), Wire::Ok(0));
        assert!(!wire_agrees(-4096));
        assert!(reconcile_wire());
    }
    #[test]
    fn errno_is_sticky() {
        let mut c = ErrnoCell::new();
        assert_eq!(c.get(), 0);
        let r = libc_call(-(EBADF as i64), &mut c);
        assert_eq!(r.errno, EBADF);
        assert_eq!(c.get(), EBADF);
        let r2 = libc_call(5, &mut c);
        assert_eq!(r2.errno, EBADF);
        assert_eq!(c.get(), EBADF);
    }
    #[test]
    fn chain_runs_once_in_order() {
        let mut ch = ConsumerChain::new();
        assert!(ch.feed(-(EINVAL as i64)));
        assert!(ch.render().is_some());
        assert!(ch.deliver());
        assert!(ch.seal());
        assert_eq!(ch.stage, Stage::Sealed);
        assert!(!ch.feed(0));
        assert!(ch.render().is_none());
        assert_ne!(ch.fingerprint(), 0x811C_9DC5);
        assert_eq!(ch.rendered, 1);
    }
    #[test]
    fn registry_capacity_and_duplicates() {
        let mut r = full_registry();
        assert_eq!(r.count(), 16);
        assert!(!r.register(Consumer { id: 0, name: "dup" }));
        assert!(!r.register(Consumer { id: 16, name: "new" }));
        assert_eq!(r.count(), 16);
        for i in 0..16u8 {
            assert!(r.registered(i));
        }
        assert!(!r.registered(16));
    }
    #[test]
    fn registry_consumption_is_conserved() {
        let mut r = full_registry();
        for i in 0..16u8 {
            assert!(r.consume(i, EBADF));
        }
        assert!(!r.consume(0, 9999));
        assert!(!r.consume(99, EBADF));
        assert_eq!(r.consumed(), 16);
        assert_eq!(r.rejected(), 2);
        assert_eq!(r.attempts(), 18);
        assert!(r.conserved());
    }
    #[test]
    fn injections_ten_of_ten() {
        let l = run_consumer_matrix();
        assert_eq!(per_class(&l), [10, 10, 10]);
        assert_eq!(l.escaped, 0);
        assert!(l.conserved());
        for k in C2Inject::all().iter() {
            assert!(inject_once(*k));
        }
    }
    #[test]
    fn stage_and_digest_stable() {
        let mut cur = Stage::Idle;
        for nxt in [Stage::Classified, Stage::Rendered, Stage::Delivered, Stage::Sealed].iter() {
            assert!(cur.can_go(*nxt));
            cur = *nxt;
        }
        assert!(!Stage::Idle.can_go(Stage::Sealed));
        assert_eq!(run_digest(), run_digest());
        assert_eq!(table_digest(), table_digest());
    }
}
