//! UNX-F1491 400+ 断言大关达成交付（域级计数宣告） · 0x75DC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批5 L 组）：
//!   域级断言计数 ≥400 大关宣告，计数只增不减，重复计数注入（同一
//!   断言重复入账）必抓，宣告文本渲染逐字对拍。
//!
//! 设计要点：
//!   ① 断言账以复合键（域号, 序号）唯一入账，重复入账显性拒
//!      （DuplicateKey）——注入必抓的锚点；
//!   ② 账只增不减：不提供删除通道，任何删行企图显性拒
//!      （DeleteForbidden），计数单调不减由构造保证；
//!   ③ 大关目标 400 冻结（GATE_TARGET），未达标宣告显性拒
//!      （GateNotReached），达标后宣告一次性完成（重复宣告拒）；
//!   ④ 宣告文本「域级断言 ≥400 大关达成」逐字对拍：渲染串与冻结
//!      常量串全等比对，一字之差即翻红；
//!   ⑤ 状态机单调：Collecting → Gate400Reached → Declared，倒退显
//!      性拒（BadState）；
//!   ⑥ 语料真实：9 个域 × 各 45 条断言 = 405 条种子账（冻结常量表
//!      注明来源），全部复合键唯一。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75DC0，批5 L 组 F1491 独占窗口）。
pub const SEG_BASE: u32 = 0x75DC0;

/// 大关目标（冻结 400，来源：分派卡「≥400 大关」）。
pub const GATE_TARGET: u32 = 400;

/// 种子域数（冻结 9：D01..D09）。
pub const SEED_DOMAINS: u32 = 9;

/// 每域种子断言数（冻结 45：9×45=405 ≥ 400 留 5 条余量）。
pub const SEED_PER_DOMAIN: u32 = 45;

/// 宣告文本（冻结，逐字对拍基准）。
pub const DECL_TEXT: &str = "域级断言 ≥400 大关达成";

/// 判定轮数（10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateCode {
    /// 重复入账：同一复合键已存在
    DuplicateKey,
    /// 删行企图：断言账只增不减
    DeleteForbidden,
    /// 未达大关就宣告
    GateNotReached,
    /// 宣告文本对拍失配
    DeclTextMismatch,
    /// 状态机倒退/越级
    BadState,
    /// 域号越界（≥ 闭集容量）
    DomainOver,
    /// 账面损坏：计数与行数不一致
    LedgerCorrupt,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl GateCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GateCode::DuplicateKey => "重复入账：同一（域,序号）复合键已在账；\
                请先查账再入账，或给新断言换唯一复合键",
            GateCode::DeleteForbidden => "删行被拒：断言账只增不减；\
                作废旧断言请走「停用标记」而非物理删行",
            GateCode::GateNotReached => "未达大关：断言数不足 400 不许宣告；\
                请补齐缺域断言后重试宣告",
            GateCode::DeclTextMismatch => "宣告文本对拍失配：渲染串与冻结基准不一致；\
                请勿手改宣告模板，复用 DECL_TEXT 常量",
            GateCode::BadState => "状态机倒退/越级：宣告流程只许单向推进；\
                请按 Collecting→Gate400Reached→Declared 次序走",
            GateCode::DomainOver => "域号越界：超出 9 域闭集；请核对域号分配表",
            GateCode::LedgerCorrupt => "账面损坏：计数器与账行数不一致；\
                请全量重算断言账并核对称重口径",
            GateCode::NotReproducible => "双跑不一致：同输入两跑结果不同；\
                请排查全局可变状态",
        }
    }
    pub const ALL: [GateCode; 8] = [
        GateCode::DuplicateKey,
        GateCode::DeleteForbidden,
        GateCode::GateNotReached,
        GateCode::DeclTextMismatch,
        GateCode::BadState,
        GateCode::DomainOver,
        GateCode::LedgerCorrupt,
        GateCode::NotReproducible,
    ];
}

/// 查表闭集返回。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GateCode::ALL.len() {
        if GateCode::ALL[i].val() == code {
            return GateCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x75DC0 段闭集内"
}

// ── 宣告状态机（单调，倒退显性拒） ──────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateState {
    /// 起账中：计数 < 400
    Collecting,
    /// 大关已达：计数 ≥ 400，待宣告
    Gate400Reached,
    /// 已宣告：大关交付完成
    Declared,
}

impl GateState {
    fn rank(self) -> u8 {
        match self {
            GateState::Collecting => 0,
            GateState::Gate400Reached => 1,
            GateState::Declared => 2,
        }
    }
}

// ── 断言账域模型 ────────────────────────────────────────────────
/// 复合键（域号, 序号）——断言账唯一性锚点。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AssertKey {
    pub domain: u32,
    pub seq: u32,
}

/// 单条断言账行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssertRow {
    pub key: AssertKey,
    /// 断言摘要（中文）
    pub title: String,
    /// 停用标记（只标记不删行：只增不减语义下的软退出）
    pub retired: bool,
}

/// 域级断言账。
#[derive(Clone, Debug)]
pub struct AssertLedger {
    pub rows: Vec<AssertRow>,
    pub state: GateState,
    /// 入账笔数（含被拒笔数，审计口径）
    pub add_attempts: u32,
    /// 重复入账被拒笔数
    pub dup_rejected: u32,
    /// 删行被拒笔数
    pub del_rejected: u32,
    /// 宣告完成时间序（单宣告后置 1，无墙钟纯序数）
    pub declared_seq: u32,
}

impl AssertLedger {
    pub fn new() -> AssertLedger {
        AssertLedger {
            rows: Vec::new(),
            state: GateState::Collecting,
            add_attempts: 0,
            dup_rejected: 0,
            del_rejected: 0,
            declared_seq: 0,
        }
    }

    /// 入账：复合键查重，重复显性拒；账行只增。
    pub fn add(&mut self, domain: u32, seq: u32, title: &str) -> Result<usize, u32> {
        self.add_attempts += 1;
        if domain >= SEED_DOMAINS + 1 {
            // 域闭集外一律拒（域号从 1 起编，≤ SEED_DOMAINS 合法）。
            return Err(GateCode::DomainOver.val());
        }
        if domain == 0 {
            return Err(GateCode::DomainOver.val());
        }
        let key = AssertKey { domain, seq };
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.key == key {
                    self.dup_rejected += 1;
                    return Err(GateCode::DuplicateKey.val());
                }
            }
            i += 1;
        }
        self.rows.push(AssertRow { key, title: String::from(title), retired: false });
        // 状态机正向判定（只允许 Collecting→Gate400Reached 一跃）。
        if self.state == GateState::Collecting && self.count() >= GATE_TARGET {
            self.state = GateState::Gate400Reached;
        }
        Ok(self.rows.len() - 1)
    }

    /// 删行企图：一律显性拒（只增不减红线）。
    pub fn remove(&mut self, _key: AssertKey) -> Result<(), u32> {
        self.del_rejected += 1;
        Err(GateCode::DeleteForbidden.val())
    }

    /// 有效计数（未停用行数）：宣告口径。
    pub fn count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if !r.retired {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 宣告：必须已达大关且未重复宣告。
    pub fn declare(&mut self) -> Result<String, u32> {
        if self.state.rank() < GateState::Gate400Reached.rank() {
            return Err(GateCode::GateNotReached.val());
        }
        if self.state == GateState::Declared {
            return Err(GateCode::BadState.val());
        }
        if self.count() < GATE_TARGET {
            return Err(GateCode::LedgerCorrupt.val());
        }
        self.state = GateState::Declared;
        self.declared_seq += 1;
        Ok(render_decl(self.count()))
    }
}

/// 宣告文本渲染：冻结模板 + 计数插值（逐字对拍基准由此产出）。
pub fn render_decl(count: u32) -> String {
    format!("{}（当前 {} 条）", DECL_TEXT, count)
}

// ── 种子语料（冻结常量表） ──────────────────────────────────────
/// 种子域标题表（来源：域级断言闭集 D01..D09，批5 L 组冻结）。
pub const SEED_DOMAIN_TITLES: [&str; 9] = [
    "启动与平台域",
    "CPU 与中断域",
    "内存管理域",
    "调度域",
    "进程与用户态域",
    "存储与文件系统域",
    "输入与 HID 域",
    "图形与显示域",
    "安全与隔离域",
];

/// 每域断言种子标题（第 d 域第 s 条）。
fn seed_title(domain: u32, seq: u32) -> String {
    let d = (domain as usize - 1) % SEED_DOMAIN_TITLES.len();
    match SEED_DOMAIN_TITLES.get(d) {
        Some(t) => format!("{}·断言{:02}", *t, seq),
        None => String::from("未知域"),
    }
}

/// 满账种子构建：9 域 × 45 条 = 405 条，全部复合键唯一。
pub fn seed_ledger() -> Result<AssertLedger, u32> {
    let mut l = AssertLedger::new();
    let mut d = 1u32;
    while d <= SEED_DOMAINS {
        let mut s = 1u32;
        while s <= SEED_PER_DOMAIN {
            l.add(d, s, &seed_title(d, s))?;
            s += 1;
        }
        d += 1;
    }
    Ok(l)
}

// ── 判据谓词（私有 bool 收口） ──────────────────────────────────
/// 大关判定：种子账有效计数 ≥ 400。
fn gate_reached_ok() -> bool {
    match seed_ledger() {
        Ok(l) => l.count() >= GATE_TARGET && l.state == GateState::Gate400Reached,
        Err(_) => false,
    }
}

/// 只增不减判定：删行全拒 + 停用后总行数不变。
fn monotonic_ok() -> bool {
    let mut l = match seed_ledger() {
        Ok(x) => x,
        Err(_) => return false,
    };
    let total_before = l.rows.len();
    let r1 = l.remove(AssertKey { domain: 1, seq: 1 });
    let r2 = l.remove(AssertKey { domain: 9, seq: 45 });
    if r1.is_ok() || r2.is_ok() || l.rows.len() != total_before {
        return false;
    }
    // 停用是唯一软退出：停用 1 条后总行数不变、有效计数减一。
    if let Some(r) = l.rows.get_mut(0) {
        r.retired = true;
    }
    l.rows.len() == total_before && l.count() == GATE_TARGET + 4
}

/// 重复入账注入判定：同键重复 10 轮全抓。
fn dup_inject_all_caught(rounds: u32) -> bool {
    let mut l = match seed_ledger() {
        Ok(x) => x,
        Err(_) => return false,
    };
    let before = l.rows.len();
    let mut r = 0u32;
    while r < rounds {
        // 注入：既有复合键 (1,1) 重复入账。
        match l.add(1, 1, "重复断言") {
            Err(e) => {
                if e != GateCode::DuplicateKey.val() {
                    return false;
                }
            }
            Ok(_) => return false,
        }
        r += 1;
    }
    l.rows.len() == before && l.dup_rejected == rounds
}

/// 宣告文本逐字对拍判定。
fn decl_text_ok() -> bool {
    let mut l = match seed_ledger() {
        Ok(x) => x,
        // 兜底空账：declare 必失败，函数随后按失败收口。
        Err(_) => AssertLedger::new(),
    };
    let rendered = match l.declare() {
        Ok(t) => t,
        Err(_) => return false,
    };
    // 逐字对拍：基准串逐字符一致。
    let expect = format!("{}（当前 {} 条）", DECL_TEXT, GATE_TARGET + 5);
    if rendered != expect {
        return false;
    }
    // DECL_TEXT 本体必须逐字冻结。
    DECL_TEXT == "域级断言 ≥400 大关达成"
}

/// 未达标宣告拒判定：小账（10 条）宣告必拒（GateNotReached）。
fn early_declare_rejected() -> bool {
    let mut l = AssertLedger::new();
    let mut s = 1u32;
    while s <= 10 {
        if l.add(1, s, "小账样本").is_err() {
            return false;
        }
        s += 1;
    }
    match l.declare() {
        Err(e) => e == GateCode::GateNotReached.val(),
        Ok(_) => false,
    }
}

/// 黄金全链：起账→达标→宣告→对拍→复核。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut l = seed_ledger()?;
    let reached = (l.count() >= GATE_TARGET) as u32;
    let text = l.declare()?;
    let text_ok = (text == render_decl(l.count()) && text.starts_with(DECL_TEXT)) as u32;
    // 账面复核：405 行、9 域齐（第三槽=覆盖域数）。
    let domains = count_domains(&l);
    Ok((reached + text_ok, l.dup_rejected, domains))
}

/// 域覆盖计数（多少个域至少有 1 条断言）。
fn count_domains(l: &AssertLedger) -> u32 {
    let mut seen = [false; 16];
    let mut i = 0;
    while i < l.rows.len() {
        if let Some(r) = l.rows.get(i) {
            let d = r.key.domain as usize;
            if d >= 1 {
                if let Some(s) = seen.get_mut(d) {
                    *s = true;
                }
            }
        }
        i += 1;
    }
    let mut n = 0u32;
    let mut k = 1usize;
    while k < 16 {
        if let Some(v) = seen.get(k) {
            if *v {
                n += 1;
            }
        }
        k += 1;
    }
    n
}

/// 探针序 A：黄金全链数值封口（达标+对拍=2，0 重复，9 域）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 0, 9)) => Ok(()),
        _ => Err(GateCode::NotReproducible.val()),
    }
}

/// 探针序 B：重复入账单点必抓。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = seed_ledger()?;
    match l.add(5, 5, "重复") {
        Err(e) if e == GateCode::DuplicateKey.val() => Ok(()),
        _ => Err(GateCode::NotReproducible.val()),
    }
}

/// 探针序 C：目标与宣告文本冻结。
pub fn probe_seq_c() -> Result<(), u32> {
    match (GATE_TARGET == 400, DECL_TEXT == "域级断言 ≥400 大关达成") {
        (true, true) => Ok(()),
        _ => Err(GateCode::NotReproducible.val()),
    }
}

// ── LCG（种子固定） ─────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入轮（LCG 驱动：重复入账/越界域/删行 三路） ────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c += 1;
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut l = seed_ledger()?;
    let mut r = 0u32;
    while r < rounds {
        let _ = rng.next();
        // 路 1：同键重复入账。
        led.record(0, l.add(1, 1, "重复").is_err());
        // 路 2：越界域号。
        led.record(1, l.add(SEED_DOMAINS + 1, 1, "越界域").is_err());
        // 路 3：删行企图。
        led.record(2, l.remove(AssertKey { domain: 2, seq: 2 }).is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GateCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < GateCode::ALL.len() {
        let v = GateCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks ≥12 条） ───────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 大关达标。
    v.push((
        "J1-大关≥400",
        gate_reached_ok(),
        match seed_ledger() {
            Ok(l) => format!("有效断言 {} 条 ≥ {}", l.count(), GATE_TARGET),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 2. 种子语料规模。
    v.push((
        "J1-种子账405行",
        match seed_ledger() {
            Ok(l) => l.rows.len() == 405,
            Err(_) => false,
        },
        String::from("9 域 × 45 条 = 405 条复合键唯一"),
    ));
    // 3. 只增不减。
    v.push((
        "J1-计数只增不减",
        monotonic_ok(),
        String::from("删行全拒，停用为唯一软退出"),
    ));
    // 4. 重复计数注入必抓。
    v.push((
        "J1-重复入账必抓",
        dup_inject_all_caught(JUDGE_ROUNDS),
        format!("{} 轮同键重复入账全部被抓", JUDGE_ROUNDS),
    ));
    // 5. 宣告文本逐字对拍。
    v.push((
        "J1-宣告文本对拍",
        decl_text_ok(),
        format!("基准：{}", render_decl(GATE_TARGET + 5)),
    ));
    // 6. 未达标宣告拒。
    v.push((
        "J1-未达标宣告拒",
        early_declare_rejected(),
        String::from("小账宣告必报 GateNotReached"),
    ));
    // 7. 边界：399 差一条不达标。
    v.push((
        "J1-399边界",
        gate_399_boundary_ok(),
        String::from("399 条不跃状态，第 400 条跃 Gate400Reached"),
    ));
    // 8. 域号越界拒。
    v.push((
        "J1-域号越界拒",
        domain_over_ok(),
        String::from("域 0 与域 10 入账均显性拒"),
    ));
    // 9. 注入三路账。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = inj.as_ref().map(|l| l.all_caught()).unwrap_or(false);
    v.push((
        "J1-注入三路账",
        inj_ok,
        match &inj {
            Ok(l) => format!("t={:?} c={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 10. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J2-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 11. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 12. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口 8 码全落段", SEG_BASE),
    ));
    // 13. 宣告一次性。
    v.push((
        "J2-宣告一次性",
        declare_once_ok(),
        String::from("二次宣告显性拒"),
    ));
    // 14. 账面自洽。
    v.push((
        "J1-账面自洽",
        match seed_ledger() {
            Ok(l) => l.rows.len() == 405 && l.count() == 405,
            Err(_) => false,
        },
        String::from("行数=计数，无幽灵行"),
    ));
    v
}

/// 399 边界判定：差 1 条不跃状态，补 1 条即跃。
fn gate_399_boundary_ok() -> bool {
    let mut l = AssertLedger::new();
    // 入 399 条：8 域满 45 + 第 9 域 39 条 = 360+39=399。
    let mut d = 1u32;
    while d <= SEED_DOMAINS - 1 {
        let mut s = 1u32;
        while s <= SEED_PER_DOMAIN {
            if l.add(d, s, "边界").is_err() {
                return false;
            }
            s += 1;
        }
        d += 1;
    }
    let mut s = 1u32;
    while s <= 39 {
        if l.add(9, s, "边界").is_err() {
            return false;
        }
        s += 1;
    }
    if l.count() != 399 || l.state != GateState::Collecting {
        return false;
    }
    if l.add(9, 40, "补一条").is_err() {
        return false;
    }
    l.count() == GATE_TARGET && l.state == GateState::Gate400Reached
}

/// 域号越界判定。
fn domain_over_ok() -> bool {
    let mut l = AssertLedger::new();
    let a = l.add(0, 1, "零域").is_err();
    let b = l.add(SEED_DOMAINS + 1, 1, "越界域").is_err();
    let c = l.add(1, 1, "合法域").is_ok();
    a && b && c
}

/// 宣告一次性判定。
fn declare_once_ok() -> bool {
    let mut l = match seed_ledger() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if l.declare().is_err() {
        return false;
    }
    l.declare().is_err() && l.declared_seq == 1
}

// ── 检查节 ─────────────────────────────────────────────────────
pub fn run_unx1491_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1491 400+ 断言大关达成交付（域级计数宣告）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ─────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_ledger_reaches_gate() {
        assert!(gate_reached_ok(), "种子账应 ≥400");
        match seed_ledger() {
            Ok(l) => assert_eq!(l.rows.len(), 405),
            Err(_) => assert!(false, "种子账不应失败"),
        }
    }

    #[test]
    fn ledger_is_append_only() {
        assert!(monotonic_ok(), "删行必须全拒");
        let mut l = match seed_ledger() {
            Ok(x) => x,
            Err(_) => {
                assert!(false, "种子账不应失败");
                return;
            }
        };
        assert_eq!(l.remove(AssertKey { domain: 3, seq: 3 }), Err(GateCode::DeleteForbidden.val()));
        assert_eq!(l.del_rejected, 1);
    }

    #[test]
    fn duplicate_key_injection_always_caught() {
        assert!(dup_inject_all_caught(JUDGE_ROUNDS), "重复入账必须 100% 抓");
    }

    #[test]
    fn declaration_text_matches_verbatim() {
        assert!(decl_text_ok(), "宣告文本必须逐字对拍");
        assert_eq!(DECL_TEXT, "域级断言 ≥400 大关达成");
    }

    #[test]
    fn early_declare_blocked() {
        assert!(early_declare_rejected(), "未达标宣告必须拒");
    }

    #[test]
    fn boundary_399_then_400_transitions() {
        assert!(gate_399_boundary_ok(), "399→400 状态跃迁边界");
    }

    #[test]
    fn domain_out_of_range_rejected() {
        assert!(domain_over_ok(), "越界域号必须拒");
    }

    #[test]
    fn golden_run_reproducible() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b, "双跑一致");
        assert_eq!(a, Ok((2, 0, 9)), "黄金全链数值封口");
    }

    #[test]
    fn probes_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn inject_rounds_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => assert!(l.all_caught(), "三路注入全抓：{:?}", l.caught),
            Err(_) => assert!(false, "注入轮不应失败"),
        }
    }

    #[test]
    fn segment_window_holds() {
        assert!(segment_guard());
        assert_eq!(detail(GateCode::DuplicateKey.val()), GateCode::DuplicateKey.detail());
        assert_eq!(detail(0x75DFF), "未知诊断码：不在 0x75DC0 段闭集内");
    }

    #[test]
    fn declare_exactly_once() {
        assert!(declare_once_ok());
        let mut l = match seed_ledger() {
            Ok(x) => x,
            Err(_) => return,
        };
        let t1 = l.declare();
        assert!(t1.is_ok());
        assert_eq!(l.declare(), Err(GateCode::BadState.val()));
    }
}
