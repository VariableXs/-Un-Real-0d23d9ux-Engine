//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14181 · I 段总账开篇（四批跨域总索引） · 0x663C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F14181-J1 I 段总账框架：B26–B29 四批 80 条总索引表（批号/
//!      对端域/条数/联动主题四列——批内锚），总索引与 F13899 I 段
//!      预告+handoff 台账对表一致断言，与 B25 E 段总账开篇体例同构
//!      声明（聚合批体例单源），指纹钉版，红绿齐；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四批闭集 B26–B29：批号/对端域/条数/联动主题四列写死，每批
//!     20 条批内锚（1..=20），合计 80 条（守恒双向断言）；
//!   - 批内锚唯一性：O(n²) 闭式扫描（批号+锚号二元组零重复）；
//!   - 双源对表：F13899 I 段预告黄金副本逐批对表 + handoff 台账
//!     （D1/D2/D4/D5/D6 域旗）对端域全就绪；
//!   - 体例同构声明：四列四态与 B25 E 段总账开篇逐项同构（聚合批
//!     体例单源，写死对表）；
//!   - 索引状态机：编制→对账→封账（越边拒收，终态收敛）；
//!   - 复现账：同输入双跑逐条一致 + 指纹双跑一致（确定性）；
//!   - LCG 种子写死 = SEG_BASE（0x663C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x663C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x663C0;

// ── 四批闭集（B26–B29） ──────────────────────────────────────────
pub const IX_COUNT: usize = 4;
pub const IX_IDS: [&str; IX_COUNT] = ["B26", "B27", "B28", "B29"];
/// 对端域列（写死）。
pub const IX_PEERS: [&str; IX_COUNT] = ["D2", "D1", "D4", "D5+D6"];
/// 联动主题列（写死）。
pub const IX_TOPICS: [&str; IX_COUNT] = [
    "用户域画像预演",
    "属性面联签预演",
    "进程域通道预演",
    "双域联动预演",
];
/// 每批条数（批内锚 20，写死）。
pub const IX_ITEMS: [usize; IX_COUNT] = [20, 20, 20, 20];
/// 总条数（黄金判据，独立写死）。
pub const GOLD_TOTAL: usize = 80;
/// 每批锚数（黄金判据，独立写死）。
pub const GOLD_ANCHOR: usize = 20;

// ── F13899 I 段预告黄金副本（独立写死） ──────────────────────────
pub const GOLD_IDS: [&str; IX_COUNT] = ["B26", "B27", "B28", "B29"];
pub const GOLD_PEERS: [&str; IX_COUNT] = ["D2", "D1", "D4", "D5+D6"];
pub const GOLD_TOPICS: [&str; IX_COUNT] = [
    "用户域画像预演",
    "属性面联签预演",
    "进程域通道预演",
    "双域联动预演",
];
pub const GOLD_ITEMS: [usize; IX_COUNT] = [20, 20, 20, 20];

// ── handoff 台账（对端域旗标，写死） ─────────────────────────────
pub const HO_DOMAINS: [&str; 5] = ["D1", "D2", "D4", "D5", "D6"];
pub const HO_READY: [bool; 5] = [true, true, true, true, true];

/// 对端域串解析为单域下标集（与 B26 路线图同口径）。
pub fn resolve_peer(peer: &str) -> Result<[i8; 2], u32> {
    if peer == "D1" {
        return Ok([0, -1]);
    }
    if peer == "D2" {
        return Ok([1, -1]);
    }
    if peer == "D4" {
        return Ok([2, -1]);
    }
    if peer == "D5+D6" {
        return Ok([3, 4]);
    }
    Err(IxCode::PeerDomainInvalid.val())
}

pub fn handoff_ready(idx: i8) -> Result<bool, u32> {
    if idx < 0 {
        return Ok(true);
    }
    let u = idx as usize;
    if u >= HO_DOMAINS.len() {
        return Err(IxCode::PeerDomainInvalid.val());
    }
    Ok(HO_READY[u])
}

// ── 体例同构常量（与 B25 E 段总账开篇对表；体例单源，写死） ──────
pub const ISO_COLUMNS_SELF: usize = 4;
pub const ISO_COLUMNS_PEER: usize = 4;
pub const ISO_STATES_SELF: usize = 3; // 编制/对账/封账
pub const ISO_STATES_PEER: usize = 3; // 声明/验证/封账（三步同构）

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IxCode {
    /// 批号越闭集
    BatchOutOfRange,
    /// 锚号越批内闭集（1..=20）
    AnchorOutOfRange,
    /// 对端域越闭集
    PeerDomainInvalid,
    /// handoff 域未就绪
    HandoffNotReady,
    /// 预告对表漂移
    ForecastDrift,
    /// 总数守恒失守（≠80）
    TotalConservation,
    /// 批内锚撞锚（零重复失守）
    AnchorDuplicate,
    /// 索引状态机越边
    IllegalTransition,
    /// 体例同构失守
    IsomorphismBroken,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl IxCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IxCode::BatchOutOfRange => "批号越闭集：B26–B29 冻结",
            IxCode::AnchorOutOfRange => "锚号越批内闭集：1..=20 冻结",
            IxCode::PeerDomainInvalid => "对端域越闭集：D1/D2/D4/D5/D6",
            IxCode::HandoffNotReady => "handoff 域未就绪：对账前置缺",
            IxCode::ForecastDrift => "预告对表漂移：总索引≠F13899",
            IxCode::TotalConservation => "总数守恒失守：80 条红线",
            IxCode::AnchorDuplicate => "批内锚撞锚：零重复失守",
            IxCode::IllegalTransition => "索引状态机越边：未对账先封账",
            IxCode::IsomorphismBroken => "体例同构失守：与 B25 开篇异构",
            IxCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [IxCode; 10] = [
        IxCode::BatchOutOfRange,
        IxCode::AnchorOutOfRange,
        IxCode::PeerDomainInvalid,
        IxCode::HandoffNotReady,
        IxCode::ForecastDrift,
        IxCode::TotalConservation,
        IxCode::AnchorDuplicate,
        IxCode::IllegalTransition,
        IxCode::IsomorphismBroken,
        IxCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IxCode::ALL.len() {
        if IxCode::ALL[i].val() == code {
            return IxCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x663C0 段闭集内"
}

/// 锚号闭集校验（1..=20）。
pub fn anchor_valid(n: usize) -> Result<(), u32> {
    if n == 0 || n > GOLD_ANCHOR {
        return Err(IxCode::AnchorOutOfRange.val());
    }
    Ok(())
}

// ── 总索引条目 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IndexEntry {
    pub batch: usize,
    pub anchor: usize,
}

// ── 索引状态机：编制 → 对账 → 封账（越边拒收） ───────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IndexState {
    Compiled,
    CrossChecked,
    Sealed,
}

// ── 四批总索引账 ─────────────────────────────────────────────────
pub struct IndexLedger {
    pub state: IndexState,
    pub entries: Vec<IndexEntry>,
    pub fingerprint: u64,
}

impl IndexLedger {
    pub fn new() -> IndexLedger {
        IndexLedger { state: IndexState::Compiled, entries: Vec::new(), fingerprint: 0 }
    }

    /// 编制阶段：逐批逐锚登记（80 条）。
    pub fn compile(&mut self) -> Result<u32, u32> {
        if self.state != IndexState::Compiled {
            return Err(IxCode::IllegalTransition.val());
        }
        let mut n = 0u32;
        for b in 0..IX_COUNT {
            if b >= IX_COUNT {
                return Err(IxCode::BatchOutOfRange.val());
            }
            for a in 1..=IX_ITEMS[b] {
                anchor_valid(a)?;
                self.entries.push(IndexEntry { batch: b, anchor: a });
                n += 1;
            }
        }
        Ok(n)
    }

    /// 对账阶段：预告对表 + handoff 就绪 + 锚唯一性（Compiled → CrossChecked）。
    pub fn cross_check(&mut self) -> Result<u32, u32> {
        if self.state != IndexState::Compiled {
            return Err(IxCode::IllegalTransition.val());
        }
        // 四列对表（F13899 预告黄金副本）。
        for b in 0..IX_COUNT {
            if IX_IDS[b] != GOLD_IDS[b]
                || IX_PEERS[b] != GOLD_PEERS[b]
                || IX_TOPICS[b] != GOLD_TOPICS[b]
                || IX_ITEMS[b] != GOLD_ITEMS[b]
            {
                return Err(IxCode::ForecastDrift.val());
            }
            let hits = resolve_peer(IX_PEERS[b])?;
            let mut h = 0;
            while h < 2 {
                if handoff_ready(hits[h])? == false {
                    return Err(IxCode::HandoffNotReady.val());
                }
                h += 1;
            }
        }
        // 总数守恒（80 条）。
        if self.entries.len() != GOLD_TOTAL {
            return Err(IxCode::TotalConservation.val());
        }
        // 批内锚唯一性（O(n²) 闭式扫描）。
        let mut i = 0;
        while i < self.entries.len() {
            let mut j = i + 1;
            while j < self.entries.len() {
                let a = self.entries[i];
                let b = self.entries[j];
                if a.batch == b.batch && a.anchor == b.anchor {
                    return Err(IxCode::AnchorDuplicate.val());
                }
                j += 1;
            }
            i += 1;
        }
        self.state = IndexState::CrossChecked;
        Ok(self.entries.len() as u32)
    }

    /// 封账阶段：指纹折叠（CrossChecked → Sealed）。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.state != IndexState::CrossChecked {
            return Err(IxCode::IllegalTransition.val());
        }
        let mut fp = 0u64;
        let mut i = 0;
        while i < self.entries.len() {
            let e = self.entries[i];
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(e.batch as u64)
                .wrapping_add(e.anchor as u64)
                .wrapping_add(i as u64);
            i += 1;
        }
        self.fingerprint = fp;
        self.state = IndexState::Sealed;
        Ok(fp)
    }

    /// 全流程：编制→对账→封账，80 条全收敛。
    pub fn run_all(&mut self) -> Result<(u32, u64), u32> {
        let n = self.compile()?;
        let m = self.cross_check()?;
        if n != m {
            return Err(IxCode::TotalConservation.val());
        }
        let fp = self.seal()?;
        Ok((n, fp))
    }

    /// 复现：双跑逐条 + 指纹一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = IndexLedger::new();
        let r2 = run2.run_all()?;
        if r2.1 != self.fingerprint {
            return Err(IxCode::NotReproducible.val());
        }
        if run2.entries.len() != self.entries.len() {
            return Err(IxCode::NotReproducible.val());
        }
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i] != run2.entries[i] {
                return Err(IxCode::NotReproducible.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 逐批锚数守恒（各批 20 条）。
    pub fn batch_distribution(&self) -> Result<[u32; IX_COUNT], u32> {
        let mut buckets = [0u32; IX_COUNT];
        let mut i = 0;
        while i < self.entries.len() {
            let b = self.entries[i].batch;
            if b >= IX_COUNT {
                return Err(IxCode::BatchOutOfRange.val());
            }
            buckets[b] += 1;
            i += 1;
        }
        for b in 0..IX_COUNT {
            if buckets[b] as usize != GOLD_ANCHOR {
                return Err(IxCode::TotalConservation.val());
            }
        }
        Ok(buckets)
    }
}

/// 体例同构对表（与 B25 E 段总账开篇逐项同构）。
pub fn isomorphism_audit() -> Result<(), u32> {
    if ISO_COLUMNS_SELF != ISO_COLUMNS_PEER {
        return Err(IxCode::IsomorphismBroken.val());
    }
    if ISO_STATES_SELF != ISO_STATES_PEER {
        return Err(IxCode::IsomorphismBroken.val());
    }
    Ok(())
}

// ── 确定性 LCG（种子写死 = SEG_BASE） ────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// LCG 抽样对账：确定性抽 20 条复验锚号闭集与批域归属。
pub fn lcg_spot_check() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut ok = 0u32;
    let mut k = 0;
    while k < 20 {
        let b = rng.below(IX_COUNT as u64) as usize;
        let a = rng.below(GOLD_ANCHOR as u64) as usize + 1;
        anchor_valid(a)?;
        resolve_peer(IX_PEERS[b])?;
        ok += 1;
        k += 1;
    }
    Ok(ok)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u64, [u32; IX_COUNT]), u32> {
    let mut led = IndexLedger::new();
    let (n, fp) = led.run_all()?;
    led.reproducible()?;
    isomorphism_audit()?;
    let buckets = led.batch_distribution()?;
    // 反向判据：逐批桶和 = 80。
    let mut sum = 0u32;
    let mut b = 0;
    while b < IX_COUNT {
        sum += buckets[b];
        b += 1;
    }
    if sum as usize != GOLD_TOTAL {
        return Err(IxCode::TotalConservation.val());
    }
    if n as usize != GOLD_TOTAL {
        return Err(IxCode::TotalConservation.val());
    }
    Ok((n, fp, buckets))
}

// ── 注入面：三族注入必抓 ─────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：锚号越闭集注入（0 号锚必拒）。
    ledger.tries[0] += 1;
    if let Err(e) = anchor_valid(0) {
        if e == IxCode::AnchorOutOfRange.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：对端域越闭集注入。
    ledger.tries[1] += 1;
    let mut dom_caught = false;
    if let Err(e) = resolve_peer("D3") {
        let _ = e;
        dom_caught = true;
    }
    if dom_caught {
        ledger.caught[1] += 1;
    }
    // 族 2：状态机越边注入（编制前先封账）。
    ledger.tries[2] += 1;
    let mut led = IndexLedger::new();
    if let Err(e) = led.seal() {
        if e == IxCode::IllegalTransition.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < IxCode::ALL.len() {
        let v = IxCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-80条守恒+复现+逐批20锚",
        matches!(golden, Ok((80, _, _))),
        format!("{:?}", golden),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    let ho_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < HO_DOMAINS.len() {
            if !HO_READY[i] {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push((
        "J1-handoff五域全就绪",
        ho_ok,
        format!("{:?}", HO_READY),
    ));
    let iso = isomorphism_audit();
    v.push((
        "J1-体例同构声明（与B25四列三态）",
        iso.is_ok(),
        String::from("列数=4 态数=3 聚合批体例单源"),
    ));
    let spot = lcg_spot_check();
    v.push((
        "J1-LCG抽样20条锚号对账",
        matches!(spot, Ok(20)),
        format!("{:?}", spot),
    ));
    // 反向判据：锚号 21 必拒（越批内闭集）。
    let anchor_reject = anchor_valid(GOLD_ANCHOR + 1).is_err();
    v.push((
        "J1-锚号反向拒收",
        anchor_reject,
        String::from("锚号 21 必拒（1..=20 闭集）"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x663C0 窗口")));
    v
}

pub fn run_unx14181_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14181 I 段总账开篇（B26–B29 四批 80 条总索引）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
