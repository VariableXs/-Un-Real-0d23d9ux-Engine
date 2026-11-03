//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2364 · 体验收官账（挫败信号全闭环终证） · 0x72CC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B39）：
//!   ① 纯功能代码 ≥290 行；
//!   ② 判据原文：UNX-F2364-J1 体验收官账通过（域内体验信号全闭环复验：
//!     闭环率 100%），新信号注入 10/10 入流，终账归档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 体验信号五类（卡住/迟滞/困惑/噪音/死路）× 各 5 条基线信号 = 25 条，
//!     五级状态机 Opened→Triaged→Fixed→Verified→Archived 单调推进，
//!     倒退/平移显性拒，未到 Archived 记归档一律拒；
//!   · 闭环率 = 已归档 / 总数（bps 口），收官闸要求 100%；
//!   · 新信号注入 10 路（五类各 2 条）：入流（登记获号）→ 全管线走通
//!     → 归档，10/10 必须入流且闭环，任一条卡管即 InjectMiss；
//!   · 终账归档：头行（epoch/total/closed/rate/sum）+ 逐信号行，sum=FNV-1a；
//!   · 失真注入 10 路，核验器 10/10 必抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72CC0，VX3-1103 体验收官窗口独占）。
pub const SEG_BASE: u32 = 0x72CC0;
/// 基线信号数（冻结 25：五类 × 5）。
pub const BASE_SIGNAL_N: usize = 25;
/// 收官纪元（冻结 3）。
pub const EPOCH: u64 = 3;
/// 新信号注入路数（判据原文 10/10，冻结）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 闭环率满值（100% = 10000 bps，冻结）。
pub const RATE_FULL_BPS: u64 = 10_000;
/// 状态机级数（Opened→Triaged→Fixed→Verified→Archived，冻结 5）。
pub const STATE_RANKS: u8 = 5;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UxCode {
    /// 信号类未知：只认卡住/迟滞/困惑/噪音/死路五类，其余一律拒
    SignalUnknown,
    /// 状态倒退：五级状态机单调推进，倒退/平移一律拒
    StateBack,
    /// 未闭环归档拒：信号未到 Archived 不许进终账
    NotClosed,
    /// 闭环未闭：收官闸要求闭环率 100%，缺一即拒
    LoopOpen,
    /// 闭环率失真：归档件闭环率与实计对不上
    RateLow,
    /// 注入未入流：注入信号必须登记获号并走通全管线
    InjectMiss,
    /// 终账破损：头行缺失/字段不合法/正文有裸控制符/截尾
    ArchiveBroken,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl UxCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            UxCode::SignalUnknown => "信号类未知：只认五类（卡住/迟滞/困惑/噪音/死路），先归类再入流",
            UxCode::StateBack => "状态倒退：五级状态机单调，倒退单即拒，按 opened 号回查",
            UxCode::NotClosed => "未闭环归档拒：信号必须走满五级到 Archived 才许入终账",
            UxCode::LoopOpen => "闭环未闭：收官闸闭环率须 100%，逐信号定位未闭环项",
            UxCode::RateLow => "闭环率失真：归档件率值与实计不符，重出终账头行",
            UxCode::InjectMiss => "注入未入流：新信号未获号或卡管，查入流登记与管线推进",
            UxCode::ArchiveBroken => "终账破损：头行/控制符/长度不对，重出终账再验",
            UxCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [UxCode; 8] = [
        UxCode::SignalUnknown, UxCode::StateBack, UxCode::NotClosed, UxCode::LoopOpen,
        UxCode::RateLow, UxCode::InjectMiss, UxCode::ArchiveBroken, UxCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    for c in UxCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码：不在 0x72CC0 段闭集内"
}

// ── 体验信号五类（冻结） ────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UxKind {
    /// 卡住：界面/流程无响应。
    Stuck,
    /// 迟滞：响应时长超出预算。
    Lag,
    /// 困惑：文案/指引让用户无法理解下一步。
    Confuse,
    /// 噪音：无意义的提示/弹窗打断。
    Noise,
    /// 死路：流程走到无出口的终态。
    DeadEnd,
}

impl UxKind {
    pub fn ord(self) -> u8 {
        match self {
            UxKind::Stuck => 0,
            UxKind::Lag => 1,
            UxKind::Confuse => 2,
            UxKind::Noise => 3,
            UxKind::DeadEnd => 4,
        }
    }
    pub fn tag(self) -> &'static str {
        match self {
            UxKind::Stuck => "STUCK",
            UxKind::Lag => "LAG",
            UxKind::Confuse => "CONFUSE",
            UxKind::Noise => "NOISE",
            UxKind::DeadEnd => "DEADEND",
        }
    }
    /// 类标签直取（越界回退 GHOST，渲染双路共用）。
    pub fn tag_of(o: u8) -> &'static str {
        from_rank_kind(o).map(|k| k.tag()).unwrap_or("GHOST")
    }
    /// 按标签还原类（未知标签 None）。
    pub fn from_tag(t: &str) -> Option<UxKind> {
        match t {
            "STUCK" => Some(UxKind::Stuck),
            "LAG" => Some(UxKind::Lag),
            "CONFUSE" => Some(UxKind::Confuse),
            "NOISE" => Some(UxKind::Noise),
            "DEADEND" => Some(UxKind::DeadEnd),
            _ => None,
        }
    }
    /// 全类表（五类 × 各 2 条注入面用）。
    pub const ALL: [UxKind; 5] =
        [UxKind::Stuck, UxKind::Lag, UxKind::Confuse, UxKind::Noise, UxKind::DeadEnd];
}

/// UxKind 数值还原（ord → kind；越界 None）。
pub fn from_rank_kind(o: u8) -> Option<UxKind> {
    UxKind::ALL.iter().find(|k| k.ord() == o).copied()
}

// ── 闭环状态机（Opened→Triaged→Fixed→Verified→Archived，单调） ──
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoopState {
    /// 开单：信号入流登记。
    Opened,
    /// 已分诊：归入五类之一并定责。
    Triaged,
    /// 已修复：修复面落地。
    Fixed,
    /// 已验证：复验确认挫败消失。
    Verified,
    /// 已归档：入终账收官。
    Archived,
}

impl LoopState {
    /// 单调序值：Opened=0 < Triaged=1 < Fixed=2 < Verified=3 < Archived=4。
    pub fn rank(self) -> u8 {
        match self {
            LoopState::Opened => 0,
            LoopState::Triaged => 1,
            LoopState::Fixed => 2,
            LoopState::Verified => 3,
            LoopState::Archived => 4,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: LoopState) -> bool {
        to.rank() > self.rank()
    }
    /// 按序值还原状态（越界 None）。
    pub fn from_rank(r: u8) -> Option<LoopState> {
        match r {
            0 => Some(LoopState::Opened),
            1 => Some(LoopState::Triaged),
            2 => Some(LoopState::Fixed),
            3 => Some(LoopState::Verified),
            4 => Some(LoopState::Archived),
            _ => None,
        }
    }
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 体验信号：id 全账单调，kind 五类之一，opened/closed 纪元对。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UxSignal {
    pub id: u64,
    pub kind: u8,
    pub opened: u64,
    pub closed: u64,
    pub state: u8,
}

/// 收官账：纪元 + 信号表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UxLedger {
    pub epoch: u64,
    pub signals: Vec<UxSignal>,
}

impl UxLedger {
    /// 已归档数（闭环计）。
    pub fn closed_n(&self) -> usize {
        self.signals.iter().filter(|s| s.state == LoopState::Archived.rank()).count()
    }

    /// 闭环率（bps 口）：已归档 / 总数；空账按 0。
    pub fn closure_bps(&self) -> u64 {
        if self.signals.is_empty() {
            return 0;
        }
        (self.closed_n() as u64).saturating_mul(RATE_FULL_BPS) / self.signals.len() as u64
    }

    /// 收官闸：闭环率必须 100%。
    pub fn gate(&self) -> Result<u64, u32> {
        let bps = self.closure_bps();
        if self.signals.is_empty() || bps < RATE_FULL_BPS {
            return Err(UxCode::LoopOpen.val());
        }
        Ok(bps)
    }
}

// ── 校验和（FNV-1a 32 位，整数实现） ────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes.iter() {
        h = (h ^ (*b as u32)).wrapping_mul(0x0100_0193);
    }
    h
}

// ── 收官簿记员（入流 + 管线推进 + 归档） ────────────────────────
#[derive(Clone, Debug)]
pub struct Closer {
    pub ledger: UxLedger,
    /// 簿记开销：管线推进次数 + 载荷记账（诊断用）。
    pub admin_ops: u64,
    pub payload_ops: u64,
}

impl Closer {
    pub fn new(epoch: u64) -> Closer {
        Closer { ledger: UxLedger { epoch, signals: Vec::new() }, admin_ops: 0, payload_ops: 0 }
    }

    /// 入流：新信号登记获号（id 全账连续），kind 必须在五类闭集内。
    pub fn admit(&mut self, kind: UxKind) -> Result<u64, u32> {
        if kind.ord() as usize >= 5 {
            return Err(UxCode::SignalUnknown.val());
        }
        let id = (self.ledger.signals.len() + 1) as u64;
        self.ledger.signals.push(UxSignal {
            id,
            kind: kind.ord(),
            opened: self.ledger.epoch,
            closed: 0,
            state: LoopState::Opened.rank(),
        });
        self.payload_ops += 8;
        Ok(id)
    }

    /// 入流查重：id 是否已登记。
    pub fn admitted(&self, id: u64) -> bool {
        self.ledger.signals.iter().any(|s| s.id == id)
    }

    /// 管线推进：指定信号状态前进一步（Opened→…→Archived），满级/未知 id 拒。
    pub fn step(&mut self, id: u64) -> Result<u8, u32> {
        let idx = match self.ledger.signals.iter().position(|s| s.id == id) {
            Some(i) => i,
            None => return Err(UxCode::InjectMiss.val()),
        };
        let cur = LoopState::from_rank(self.ledger.signals[idx].state).ok_or(UxCode::StateBack.val())?;
        let next = LoopState::from_rank(cur.rank() + 1).ok_or(UxCode::StateBack.val())?;
        if !cur.can_advance(next) {
            return Err(UxCode::StateBack.val());
        }
        self.admin_ops += 1;
        self.ledger.signals[idx].state = next.rank();
        if next == LoopState::Archived {
            self.ledger.signals[idx].closed = self.ledger.epoch;
        }
        Ok(next.rank())
    }

    /// 直达归档：全管线四步走通（Opened→Triaged→Fixed→Verified→Archived）。
    pub fn close_full(&mut self, id: u64) -> Result<(), u32> {
        for expect in 1..STATE_RANKS {
            let r = self.step(id)?;
            if r != expect {
                return Err(UxCode::StateBack.val());
            }
        }
        Ok(())
    }
}

// ── 两条独立渲染路径（对拍双路） ────────────────────────────────
/// 归档路径渲染：format 一次性成型（字段序冻结：id→kind→opened→closed→st）。
pub fn render_signal(s: &UxSignal) -> String {
    format!("{{\"id\":{},\"kind\":{},\"op\":{},\"cl\":{},\"st\":{}}}\n",
        s.id, UxKind::tag_of(s.kind), s.opened, s.closed, s.state)
}

/// 直读路径渲染：逐字段拼接（与归档路径结构独立，供对拍基准）。
pub fn direct_read_signal(s: &UxSignal) -> String {
    let mut out = format!("{{\"id\":{}", s.id);
    out.push_str(",\"kind\":"); out.push_str(UxKind::tag_of(s.kind));
    out.push_str(",\"op\":"); out.push_str(&s.opened.to_string());
    out.push_str(",\"cl\":"); out.push_str(&s.closed.to_string());
    out.push_str(",\"st\":"); out.push_str(&s.state.to_string());
    out.push_str("}\n");
    out
}

/// 全账规范正文（归档路径）。
pub fn canonical_body(l: &UxLedger) -> String {
    let mut s = String::new();
    for sg in l.signals.iter() {
        s.push_str(&render_signal(sg));
    }
    s
}

/// 全账直读正文（对拍基准路径，逐信号独立拼接）。
pub fn direct_body(l: &UxLedger) -> String {
    let mut s = String::new();
    for sg in l.signals.iter() {
        s.push_str(&direct_read_signal(sg));
    }
    s
}



// ── JSON 字符串转义（归档件的转义闭集） ─────────────────────────
/// 键串转义：引号/反斜杠/换行/回车/制表 + 0x00..0x1F 控制符走 \u00XX。
pub fn esc_json(s: &str) -> String {
    let mut out = String::new();
    out.push('"');
    for c in s.as_bytes().iter() {
        match *c {
            0x22 => out.push_str("\\\""),
            0x5C => out.push_str("\\\\"),
            0x0A => out.push_str("\\n"),
            0x0D => out.push_str("\\r"),
            0x09 => out.push_str("\\t"),
            0x00..=0x1F => out.push_str(&format!("\\u{:04x}", c)),
            _ => out.push(*c as char),
        }
    }

    out.push('"');
    out
}

// ── 终账归档件 ──────────────────────────────────────────────────
/// 终账头行：@VXU9|epoch=|total=|closed=|rate=|sum=XXXXXXXX;
pub fn archive_header(epoch: u64, total: u64, closed: u64, rate: u64, sum: u32) -> String {
    format!("@VXU9|epoch={}|total={}|closed={}|rate={}|sum={:08X};\n", epoch, total, closed, rate, sum)
}

/// 终账归档件 = 头行 + 全账规范正文。
pub fn archive_text(l: &UxLedger) -> String {
    let body = canonical_body(l);
    let mut out = archive_header(
        l.epoch, l.signals.len() as u64, l.closed_n() as u64, l.closure_bps(),
        fnv1a(body.as_bytes()),
    );
    out.push_str(&body);
    out
}

/// 解析后的终账头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchHeader {
    pub epoch: u64,
    pub total: u64,
    pub closed: u64,
    pub rate: u64,
    pub sum: u32,
}

/// 字节查找（ASCII 正文专用）。
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    h.iter().skip(from).position(|&c| c == needle).map(|p| p + from)
}

/// 子串查找（ASCII 专用）。
pub fn find_sub(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || h.len() < pat.len() {
        return None;
    }
    h.windows(pat.len()).skip(from).position(|w| w == pat).map(|p| p + from)
}

/// 十进制无符号解析（checked，防溢出）。
pub fn parse_dec(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for c in s.as_bytes().iter() {
        if *c < b'0' || *c > b'9' {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((*c - b'0') as u64)?;
    }
    Some(v)
}

/// 8 位十六进制解析（校验和域专用）。
pub fn parse_hex8(s: &str) -> Option<u32> {
    if s.len() != 8 {
        return None;
    }
    let mut v: u32 = 0;
    for c in s.as_bytes().iter() {
        let d = match *c {
            x @ b'0'..=b'9' => (x - b'0') as u32,
            x @ b'A'..=b'F' => (x - b'A') as u32 + 10,
            _ => return None,
        };
        v = (v << 4) | d;
    }
    Some(v)
}

/// 终账头解析：逐 token 扫描，未知字段即破损。
pub fn parse_header(hdr: &str) -> Option<ArchHeader> {
    let mut epoch: Option<u64> = None;
    let mut total: Option<u64> = None;
    let mut closed: Option<u64> = None;
    let mut rate: Option<u64> = None;
    let mut sum: Option<u32> = None;
    for tok in hdr.split('|') {
        if tok.is_empty() {
            continue;
        }
        let eq = tok.find('=')?;
        let (k, v) = (&tok[..eq], &tok[eq + 1..]);
        match k {
            "epoch" => epoch = Some(parse_dec(v)?),
            "total" => total = Some(parse_dec(v)?),
            "closed" => closed = Some(parse_dec(v)?),
            "rate" => rate = Some(parse_dec(v)?),
            "sum" => sum = Some(parse_hex8(v)?),
            _ => return None,
        }
    }
    Some(ArchHeader { epoch: epoch?, total: total?, closed: closed?, rate: rate?, sum: sum? })
}

/// 闭环序复核：id 连续、state 全 Archived、closed 纪元对齐、类闭集。
fn closure_order_scan(l: &UxLedger) -> Result<(), u32> {
    for (i, s) in l.signals.iter().enumerate() {
        if s.id != (i + 1) as u64 {
            return Err(UxCode::InjectMiss.val());
        }
        if s.state != LoopState::Archived.rank() || s.closed != l.epoch {
            return Err(UxCode::NotClosed.val());
        }
        if from_rank_kind(s.kind).is_none() {
            return Err(UxCode::SignalUnknown.val());
        }
    }
    Ok(())
}

/// 终账核验：头行解析 + 计数/率对账 + 正文逐字节对拍 + 校验和 + 闭环序。
pub fn verify_archive(l: &UxLedger, out: &str) -> Result<(), u32> {
    let ob = out.as_bytes();
    let broken = || UxCode::ArchiveBroken.val();
    if out.len() < 7 || !out.starts_with("@VXU9|") {
        return Err(broken());
    }
    let semi = match find_byte(ob, 0, b';') {
        Some(p) => p,
        None => return Err(broken()),
    };
    let h = match parse_header(&out[6..semi]) {
        Some(h) => h,
        None => return Err(broken()),
    };
    if h.epoch != l.epoch {
        return Err(broken());
    }
    if h.total != l.signals.len() as u64 || h.closed != l.closed_n() as u64 || h.rate != l.closure_bps() {
        return Err(UxCode::RateLow.val());
    }
    // 正文从头行换行之后起算（头行行尾 '\n' 不属于正文）。
    let mut start = semi + 1;
    if start < out.len() && ob[start] == b'\n' {
        start += 1;
    }
    let body = &out[start..];
    // 裸控制符扫描（<0x20 且非换行 → 破损）。
    for c in body.as_bytes().iter() {
        if *c < 0x20 && *c != b'\n' {
            return Err(broken());
        }
    }
    if body != canonical_body(l) || h.sum != fnv1a(body.as_bytes()) {
        return Err(broken());
    }
    closure_order_scan(l)?;
    l.gate()?;
    Ok(())
}

// ── 黄金建账（25 条基线信号全管线闭环） ─────────────────────────
/// 建账：五类各 5 条基线信号，逐条入流 → 全管线走通 → 归档。
pub fn build_golden_ledger(epoch: u64) -> Result<UxLedger, u32> {
    let mut cs = Closer::new(epoch);
    for k in 0..BASE_SIGNAL_N {
        let id = cs.admit(UxKind::ALL[k % UxKind::ALL.len()])?;
        cs.close_full(id)?;
    }
    if cs.ledger.closed_n() != BASE_SIGNAL_N {
        return Err(UxCode::LoopOpen.val());
    }
    Ok(cs.ledger)
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 未入流的注入路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit { self.caught += 1; } else { self.miss_kinds.push(kind); }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

// ── 新信号注入（10 路入流） ─────────────────────────────────────
/// 注入轮：五类各 2 条新信号入流 → 全管线走通 → 归档；
/// 全部入流且闭环才算入流成功（10/10）。
pub fn inject_new_signals(base: &mut Closer) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    for r in 0..JUDGE_ROUNDS {
        let kind = UxKind::ALL[(r % 5) as usize];
        let before = base.ledger.signals.len() as u64;
        let id = base.admit(kind)?;
        // 入流验证：获号连续 + 确在账内；全管线走通：四步到 Archived。
        let inflow = id == before + 1 && base.admitted(id);
        let closed = base.close_full(id).is_ok();
        led.record(r as usize, inflow && closed);
    }
    Ok(led)
}

// ── 失真注入（10 路逐字失真） ───────────────────────────────────
/// 复制 [lo, hi] 闭区间字节（越界自动夹到流长）。
fn push_range(b: &[u8], lo: usize, hi: usize, v: &mut Vec<u8>) {
    let mut k = lo;
    while k <= hi && k < b.len() {
        v.push(b[k]);
        k += 1;
    }
}

/// 头域单字节改写：定位 pat，把其后首个字节改写（保证与原值不同）。
fn patch_after(b: &[u8], pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let tgt = find_sub(b, 0, pat).map(|p| p + pat.len()).unwrap_or(usize::MAX);
    for (i, c) in b.iter().enumerate() {
        v.push(if i == tgt { if *c == newc { alt } else { newc } } else { *c });
    }
}

/// 头行行尾下标（头行 '\n' 位置；找不到取全长）。
fn header_line_end(b: &[u8]) -> usize {
    find_byte(b, 0, b';').and_then(|sp| find_byte(b, sp, b'\n')).unwrap_or(b.len())
}

/// 对黄金终账件注入第 kind 路失真，返回坏件（内容全 ASCII，安全）。
pub fn distort(out: &str, kind: usize) -> String {
    let b = out.as_bytes();
    let mut v: Vec<u8> = Vec::new();
    let he = header_line_end(b);
    match kind % 10 {
        0 => patch_after(b, b"\"st\":", b'4', b'0', &mut v), // 状态清零（未闭环）
        1 => patch_after(b, b"rate=", b'9', b'8', &mut v),   // 率补丁
        2 => patch_after(b, b"total=", b'9', b'1', &mut v),  // 总数补丁
        3 => {
            // 删行：删正文第一行（计数漂移）。
            let le = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, le + 1, b.len(), &mut v);
        }
        4 => {
            // 加行：尾部追加伪造信号行（st=0 未闭环）。
            push_range(b, 0, b.len(), &mut v);
            let tail: &[u8] = b"{\"id\":999,\"kind\":\"GHOST\",\"op\":0,\"cl\":0,\"st\":0}\n";
            for t in tail.iter() { v.push(*t); }
        }
        5 => {
            // 换序：正文前两行对调（id 序冻结被破坏）。
            let l1 = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            let l2 = find_byte(b, l1 + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, l1 + 1, l2, &mut v);
            push_range(b, he + 1, l1, &mut v);
            push_range(b, l2 + 1, b.len(), &mut v);
        }
        6 => push_range(b, 0, b.len().saturating_sub(7), &mut v), // 截尾
        7 => {
            // 注控制符：头行后插裸 0x01。
            push_range(b, 0, he, &mut v);
            v.push(0x01);
            push_range(b, he + 1, b.len(), &mut v);
        }
        8 => patch_after(b, b"\"kind\":", b'S', b'X', &mut v), // 类改写（STUCK→XTUCK）
        _ => {
            // 行重复：正文第一行复制一份（id 重号）。
            let le = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, b.len(), &mut v);
            push_range(b, he + 1, le, &mut v);
        }
    }
    match String::from_utf8(v) {
        Ok(s) => s,
        Err(_) => String::new(),
    }
}

/// 注入检出轮：每轮对新鲜黄金终账注入一路失真，核验器必须抓到
///（黄金件先自证核验通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    for r in 0..rounds {
        let l = build_golden_ledger(EPOCH + r as u64)?;
        let out = archive_text(&l);
        verify_archive(&l, &out)?;
        let kind = (r % 10) as usize;
        led.record(kind, verify_archive(&l, &distort(&out, kind)).is_err());
    }
    Ok(led)
}

// ── 对拍抽验（归档路径 vs 直读路径，100 组逐字节） ──────────────
pub const FIDELITY_SAMPLES: usize = 100;

pub fn fidelity_100(l: &UxLedger) -> Result<u32, u32> {
    if l.signals.is_empty() {
        return Err(UxCode::LoopOpen.val());
    }
    if canonical_body(l) != direct_body(l) {
        return Err(UxCode::ArchiveBroken.val());
    }
    let n = l.signals.len();
    for k in 0..FIDELITY_SAMPLES {
        let idx = k % n;
        let s = match l.signals.get(idx) {
            Some(s) => s,
            None => return Err(UxCode::ArchiveBroken.val()),
        };
        if render_signal(s) != direct_read_signal(s) || s.id != (idx + 1) as u64 {
            return Err(UxCode::ArchiveBroken.val());
        }
    }
    Ok(FIDELITY_SAMPLES as u32)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：基线闭环 + 新信号注入 10/10 入流 + 归档核验 + 收官闸 + 对拍
/// + 五拒探。返回 (正旗数, 拒旗数, 总信号数, 闭环率 bps)。
pub fn golden_run() -> Result<(u32, u32, usize, u64), u32> {
    // 基线 25 条已闭环；新信号注入 10/10 入流并闭环。
    let mut cs = Closer::new(EPOCH);
    cs.ledger = build_golden_ledger(EPOCH)?;
    let inflow = inject_new_signals(&mut cs)?;
    if !inflow.all_caught() {
        return Err(UxCode::InjectMiss.val());
    }
    let l = cs.ledger;
    let out = archive_text(&l);
    verify_archive(&l, &out)?;
    let rate = l.gate()?;
    let fid = fidelity_100(&l)?;
    // 拒探 1：未知类拒（标签闭集外一律 None）；拒探 2：状态倒退拒。
    let unknown_rej = UxKind::from_tag("GHOST").is_none();
    let back_rej = !LoopState::Fixed.can_advance(LoopState::Triaged)
        && !LoopState::Triaged.can_advance(LoopState::Triaged)
        && LoopState::Fixed.can_advance(LoopState::Verified);
    // 拒探 3：未闭环归档拒（只走一步的账，核验器必抓）。
    let mut nc = Closer::new(EPOCH);
    let id2 = nc.admit(UxKind::Lag)?;
    let _ = nc.step(id2)?;
    let nc_rej = verify_archive(&nc.ledger, &archive_text(&nc.ledger)).is_err();
    // 拒探 4：闭环率不足（只归档一半）；拒探 5：终账篡改（率路失真）。
    let mut half = Closer::new(EPOCH);
    for (h, k) in (0..BASE_SIGNAL_N).enumerate() {
        let kind = UxKind::ALL[k % UxKind::ALL.len()];
        let id = half.admit(kind)?;
        if h % 2 == 0 {
            let _ = half.close_full(id);
        }
    }
    let half_rej = half.ledger.gate().is_err();
    let tamper_rej = verify_archive(&l, &distort(&out, 1)).is_err();
    let flags = (fid == FIDELITY_SAMPLES as u32) as u32 + inflow.all_caught() as u32
        + (rate == RATE_FULL_BPS) as u32 + probe_escape() as u32;
    let rejects = unknown_rej as u32 + back_rej as u32 + nc_rej as u32 + half_rej as u32 + tamper_rej as u32;
    Ok((flags, rejects, l.signals.len(), rate))
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(UxCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    UxCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40)
}

// ── 终账头往返探针（建头→解析→逐域对账） ────────────────────────
pub fn header_roundtrip() -> bool {
    let sum = fnv1a(b"probe");
    let h = archive_header(7, 25, 25, 10_000, sum);
    let a = match parse_header(&h[6..h.len() - 2]) {
        Some(p) => p.epoch == 7 && p.total == 25 && p.closed == 25 && p.rate == 10_000 && p.sum == sum,
        None => false,
    };
    // 破损头三拒：未知字段 / 坏数字 / 非 8 位校验和。
    let bad = parse_header("epoch=7|ghost=1|rate=10000|sum=00000000").is_none()
        && parse_header("epoch=x|rate=10000|sum=00000000").is_none()
        && parse_header("epoch=7|rate=10000|sum=00").is_none();
    a && bad
}

// ── 转义探针 ────────────────────────────────────────────────────
/// 转义闭集探针：引号/反斜杠/换行/制表/控制符逐字验证。
pub fn probe_escape() -> bool {
    let q = esc_json("a\"b\\c\nd\te");
    let q2 = esc_json("\u{01}");
    q.starts_with('"') && q.ends_with('"')
        && q.contains("\\\"") && q.contains("\\\\") && q.contains("\\n") && q.contains("\\t")
        && !q.contains('\n') && !q.contains('\t')
        && q2 == String::from("\"\\u0001\"")
        && esc_json("DEADEND") == String::from("\"DEADEND\"")
}

// ── 状态机单调守卫探针 ──────────────────────────────────────────
/// 五级平移/倒退全拒 + 类标签往返 + 满级 step 拒 + 未知 id 拒。
pub fn probe_monotonic() -> bool {
    let m1 = !LoopState::Opened.can_advance(LoopState::Opened);
    let m2 = !LoopState::Archived.can_advance(LoopState::Opened);
    let m3 = LoopState::Verified.can_advance(LoopState::Archived);
    let m4 = LoopState::from_rank(STATE_RANKS).is_none();
    let t1 = UxKind::from_tag("STUCK") == Some(UxKind::Stuck)
        && UxKind::from_tag("DEADEND") == Some(UxKind::DeadEnd)
        && UxKind::from_tag("GHOST").is_none();
    // 满级信号再 step 拒；未知 id step 拒。
    let mut cs = Closer::new(EPOCH);
    let probe = cs.admit(UxKind::Noise).map(|id| cs.close_full(id).is_ok() && cs.step(id).is_err() && cs.step(999).is_err());
    let m56 = matches!(probe, Ok(true));
    m1 && m2 && m3 && m4 && t1 && m56
}

// ── 收官闸边界 + 五类覆盖探针 ───────────────────────────────────
/// 24/25 拒 / 25/25 过 + 基线账五类各恰 5 条（逐类计数对账）。
pub fn probe_gate_edge() -> bool {
    let l = match build_golden_ledger(EPOCH) {
        Ok(l) => l,
        Err(_) => return false,
    };
    // 24/25：末条状态降级（闸门探针模型，仅本函数内生效）。
    let mut m = l.clone();
    let flipped = m.signals.last_mut().map(|s| {
        s.state = LoopState::Fixed.rank();
        true
    }).unwrap_or(false);
    let gate_ok = flipped && m.gate().is_err() && l.gate() == Ok(RATE_FULL_BPS);
    // 五类覆盖：逐类计数恰 BASE_SIGNAL_N/5。
    let mut counts = [0usize; 5];
    for s in l.signals.iter() {
        match from_rank_kind(s.kind) {
            Some(k) => counts[k.ord() as usize] += 1,
            None => return false,
        }
    }
    gate_ok && counts.iter().all(|&c| c == BASE_SIGNAL_N / 5)
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let l = build_golden_ledger(EPOCH)?;
    verify_archive(&l, &archive_text(&l))?;
    let rate = l.gate()?;
    Ok(format!(
        "状态=已完成 signals={} closed={} rate={}bps sum={:08X}",
        l.signals.len(), l.closed_n(), rate,
        fnv1a(canonical_body(&l).as_bytes())
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正五拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, n, rate)) => (*f == 4 && *r == 5, format!("flags={} rejects={} signals={} rate={}bps", f, r, n, rate)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-体验收官全链", gp, gd));
    // J1 闭环率 100%。
    let cl = build_golden_ledger(EPOCH);
    let cp = match &cl {
        Ok(l) => l.gate() == Ok(RATE_FULL_BPS) && l.closed_n() == BASE_SIGNAL_N,
        Err(_) => false,
    };
    v.push(("J1-闭环率100%", cp, format!("{}/{} 全归档", BASE_SIGNAL_N, BASE_SIGNAL_N)));
    // J1 新信号注入 10/10 入流（基线账先建满，再注入十路）。
    let mut cs = Closer::new(EPOCH);
    cs.ledger = match &cl {
        Ok(l) => l.clone(),
        Err(_) => return v,
    };
    let ip = match inject_new_signals(&mut cs) {
        Ok(x) => x.all_caught(),
        Err(_) => false,
    };
    v.push(("J1-新信号注入10/10入流", ip, String::from("五类各2条全入流全闭环")));
    // J1 失真注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (dp, dd) = match &inj {
        Ok(x) => (x.all_caught(), format!("t={} c={} miss={:?}", x.tries, x.caught, x.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", dp, dd));
    // J1 终账归档核验。
    let arc = match &cl {
        Ok(l) => verify_archive(l, &archive_text(l)).is_ok(),
        Err(_) => false,
    };
    v.push(("J1-终账归档", arc, String::from("头行+闭环序+sum 复核全过")));
    // J2 状态机单调守卫。
    v.push(("J2-状态机单调", probe_monotonic(), String::from("五级平移/倒退/满级/未知 id 四拒")));
    // J2 收官闸边界（24/25 拒 / 25/25 过）+ 五类覆盖（五类各恰 5 条）。
    v.push(("J2-收官闸边界+五类覆盖", probe_gate_edge(), format!("闸门边界+五类×{} 条", BASE_SIGNAL_N / 5)));
    // J1 终账头往返 + 破损头拒。
    v.push(("J1-终账头校验", header_roundtrip(), String::from("建头→解析逐域对账；未知字段/坏数字/短校验和必败")));
    // J1 转义闭集。
    v.push(("J1-转义闭集", probe_escape(), String::from("引号/反斜杠/换行/制表/\\u00XX 全落位")));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72CC0 窗口")));
    v
}

pub fn run_unx2364_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2364 体验收官账（挫败信号全闭环终证）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
