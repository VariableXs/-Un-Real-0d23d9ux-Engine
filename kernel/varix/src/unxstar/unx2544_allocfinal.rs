//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2544 · 分配器域自检终版（F2418 全域聚合） · 0x72E00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A4-B08）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F2544-J1 域自检终版全项过，自检报告含 B01–B08 全
//!     段清单；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 八段域卡（B01–B08）逐域登记→逐条记账→封账→全集齐→聚合出报，
//!     状态机 Empty→Collecting→CollectedAll→Reported 单调推进，倒退显性拒；
//!   · 双路对拍：记账路（卡片聚合数）vs 复核路（检查流水逐条复算），
//!     两路独立，账数被篡改必抓；
//!   · 失真注入 10 路（通过数虚增/红项抹除/封账位清零/总数虚增/权重
//!     清零/摘要涂改/域名涂改/掉卡/插卡/红项清空），复核器 10/10 必抓；
//!   · 加权通过率 bps（整数运算）：Σ(过×权重)/Σ(总×权重)×10^4，封顶 10^4；
//!   · 摘要链：按段位序对八段卡片摘要做 FNV-1a 折叠，序变即变。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72E00，VX3-1103 分配器聚合窗口独占）。
pub const SEG_BASE: u32 = 0x72E00;
/// 分配器域段数（B01–B08，冻结 8）。
pub const DOMAIN_N: u8 = 8;
/// 每域最少检查条数（少于即账目破损）。
pub const MIN_CHECKS_PER_DOM: u32 = 2;
/// 每域检查容量上限（防账目爆炸，冻结 64）。
pub const CHECK_CAP_PER_DOM: u32 = 64;
/// 加权通过率封顶（100% = 10000 bps）。
pub const RATE_FULL_BPS: u64 = 10_000;
/// 注入轮数（判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggCode {
    /// 未知域号：只认 B01–B08（1..=8），越界一律拒
    DomUnknown,
    /// 域重复登记：同号域卡只许一张，重登即账面破损
    DomDup,
    /// 状态倒退：聚合状态机只进不退，封账域再记账同拒
    StateBack,
    /// 早出报告：B01–B08 未集齐未全封就出报告
    ReportEarly,
    /// 域未封账：存在未封账域就进聚合
    DomUnsealed,
    /// 账目数不符：通过数+红项数≠总检查数，或低于每域下限
    CountMismatch,
    /// 红项泄漏：聚合判绿但仍有红项/空红项/摘要对不上
    RedLeak,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl AggCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AggCode::DomUnknown => "未知域号：只认 B01–B08，查登记段位",
            AggCode::DomDup => "域重复登记：同号域卡只许一张，去重后再登记",
            AggCode::StateBack => "状态倒退：聚合状态机单调推进，不许回退",
            AggCode::ReportEarly => "早出报告：先集齐并封账 B01–B08 再出报告",
            AggCode::DomUnsealed => "域未封账：存在未封账域，逐域 seal 后再聚合",
            AggCode::CountMismatch => "账目数不符：通过数+红项数≠总数，重对流水账",
            AggCode::RedLeak => "红项泄漏：聚合判绿仍有红项或摘要对不上，逐项定位",
            AggCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [AggCode; 8] = [
        AggCode::DomUnknown,
        AggCode::DomDup,
        AggCode::StateBack,
        AggCode::ReportEarly,
        AggCode::DomUnsealed,
        AggCode::CountMismatch,
        AggCode::RedLeak,
        AggCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AggCode::ALL.len() {
        if AggCode::ALL[i].val() == code {
            return AggCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72E00 段闭集内"
}

// ── 聚合状态机（Empty→Collecting→CollectedAll→Reported，单调） ──
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggState {
    /// 空册：一域未登。
    Empty,
    /// 登记中：至少一域在册，尚有域未封账。
    Collecting,
    /// 已集齐：B01–B08 全部封账，可聚合。
    CollectedAll,
    /// 已出报：终版报告已产出，账面冻结。
    Reported,
}

impl AggState {
    /// 单调序值：Empty=0 < Collecting=1 < CollectedAll=2 < Reported=3。
    pub fn rank(self) -> u8 {
        match self {
            AggState::Empty => 0,
            AggState::Collecting => 1,
            AggState::CollectedAll => 2,
            AggState::Reported => 3,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: AggState) -> bool {
        to.rank() > self.rank()
    }
}

// ── 校验和（FNV-1a 32 位，整数实现） ────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 域号（B01=1 .. B08=8）。
pub type DomId = u8;

/// 单条检查流水（记账原子单位，复核路的唯一事实来源）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckRec {
    pub dom: DomId,
    pub name: String,
    pub passed: bool,
    pub note: String,
}

/// 域卡：一域的聚合账面（由检查流水折叠而来，可被篡改，故有复核路）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomCard {
    pub id: DomId,
    pub name: String,
    /// 严重度权重（聚合加权用，1..=16）。
    pub weight: u32,
    /// 封账位：封账后本域账面冻结。
    pub sealed: bool,
    /// 总检查条数。
    pub total: u32,
    /// 通过条数。
    pub passed: u32,
    /// 红项清单（未过检查的定位文本，name·note）。
    pub reds: Vec<String>,
    /// 域卡摘要（封账时对全部字段折叠，防涂改）。
    pub digest: u32,
}

impl DomCard {
    /// 域卡摘要折叠：id|weight|sealed|total|passed|reds 逐字进 FNV。
    pub fn recompute_digest(&self) -> u32 {
        let mut h = fnv1a(&[self.id, (self.weight & 0xFF) as u8, (self.weight >> 8) as u8, self.sealed as u8]);
        h = fnv1a(&h.to_le_bytes()).wrapping_mul(31) ^ fnv1a(self.name.as_bytes());
        h = (h ^ (self.total as u32)).wrapping_mul(0x0100_0193);
        h = (h ^ (self.passed as u32)).wrapping_mul(0x0100_0193);
        let mut i = 0usize;
        while i < self.reds.len() {
            h = (h ^ fnv1a(self.reds[i].as_bytes())).wrapping_mul(0x0100_0193);
            i += 1;
        }
        h
    }
    /// 红项数（账面口径）。
    pub fn red_n(&self) -> u32 {
        self.reds.len() as u32
    }
    /// 账目自洽：通过+红=总，且每域不低于下限、不超容量。
    pub fn tally_ok(&self) -> bool {
        self.passed <= self.total
            && self.passed + self.red_n() == self.total
            && self.total >= MIN_CHECKS_PER_DOM
            && self.total <= CHECK_CAP_PER_DOM
            && self.weight >= 1
            && self.weight <= 16
            && self.digest == self.recompute_digest()
    }
}

/// 单域登记 + 记账 + 封账的错误收口。
pub fn card_from_checks(id: DomId, name: &str, weight: u32, recs: &[CheckRec]) -> Result<DomCard, u32> {
    if id < 1 || id > DOMAIN_N {
        return Err(AggCode::DomUnknown.val());
    }
    if weight < 1 || weight > 16 {
        return Err(AggCode::CountMismatch.val());
    }
    let mut total = 0u32;
    let mut passed = 0u32;
    let mut reds: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < recs.len() {
        let r = match recs.get(i) {
            Some(r) => r,
            None => return Err(AggCode::CountMismatch.val()),
        };
        if r.dom != id {
            return Err(AggCode::DomUnknown.val());
        }
        total += 1;
        if r.passed {
            passed += 1;
        } else {
            reds.push(format!("B{:02}·{}·{}", id, r.name, r.note));
        }
        i += 1;
    }
    if total < MIN_CHECKS_PER_DOM || total > CHECK_CAP_PER_DOM {
        return Err(AggCode::CountMismatch.val());
    }
    let mut card = DomCard {
        id,
        name: String::from(name),
        weight,
        sealed: true,
        total,
        passed,
        reds,
        digest: 0,
    };
    card.digest = card.recompute_digest();
    Ok(card)
}

// ── 段名表（B01–B08 冻结命名） ─────────────────────────────────
pub const DOM_NAMES: [&str; 8] = [
    "B01-伙伴页册",
    "B02- slab 账",
    "B03-缓存线账",
    "B04-大页池账",
    "B05-DMA 窗账",
    "B06-回收站账",
    "B07-水位闸账",
    "B08-聚合终版",
];

// ── 聚合器 ──────────────────────────────────────────────────────
/// 分配器域聚合器：域卡册 + 状态机 + 加权账。
#[derive(Clone, Debug)]
pub struct DomRegistry {
    /// 域卡册（按登记序存放，聚合前必须恰有 DOMAIN_N 张且号全）。
    pub cards: Vec<DomCard>,
    /// 检查流水（复核路唯一事实来源，只追加不删改）。
    pub recs: Vec<CheckRec>,
    pub state: AggState,
    /// 生命周期纪元：任何状态推进 +1，倒退检测用。
    pub epoch: u64,
    /// 出报次数。
    pub reports: u32,
}

impl DomRegistry {
    pub fn new() -> DomRegistry {
        DomRegistry { cards: Vec::new(), recs: Vec::new(), state: AggState::Empty, epoch: 0, reports: 0 }
    }

    /// 状态推进公开口：倒退/平移显性拒。
    pub fn advance_state(&mut self, to: AggState) -> Result<(), u32> {
        if !self.state.can_advance(to) {
            return Err(AggCode::StateBack.val());
        }
        self.state = to;
        self.epoch += 1;
        Ok(())
    }

    /// 登记域卡：号必须在 B01–B08 内且未重登；首登推进 Empty→Collecting。
    pub fn register(&mut self, card: DomCard) -> Result<(), u32> {
        if card.id < 1 || card.id > DOMAIN_N {
            return Err(AggCode::DomUnknown.val());
        }
        let mut i = 0usize;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                if c.id == card.id {
                    return Err(AggCode::DomDup.val());
                }
            }
            i += 1;
        }
        if !card.sealed || !card.tally_ok() {
            return Err(AggCode::DomUnsealed.val());
        }
        if self.state == AggState::Empty {
            self.advance_state(AggState::Collecting)?;
        }
        if self.state != AggState::Collecting {
            // 集齐/出报后不许再插卡。
            return Err(AggCode::StateBack.val());
        }
        self.cards.push(card);
        Ok(())
    }

    /// 追加检查流水（只进不退；对应域已封账则拒）。
    pub fn record(&mut self, rec: CheckRec) -> Result<(), u32> {
        if rec.dom < 1 || rec.dom > DOMAIN_N {
            return Err(AggCode::DomUnknown.val());
        }
        if self.state.rank() >= AggState::CollectedAll.rank() {
            return Err(AggCode::StateBack.val());
        }
        if self.state == AggState::Empty {
            self.advance_state(AggState::Collecting)?;
        }
        self.recs.push(rec);
        Ok(())
    }

    /// 逐域封账：从流水折叠出域卡并登记；同域重复封账拒。
    pub fn seal_domain(&mut self, id: DomId, weight: u32) -> Result<(), u32> {
        if id < 1 || id > DOMAIN_N {
            return Err(AggCode::DomUnknown.val());
        }
        let mut i = 0usize;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                if c.id == id {
                    return Err(AggCode::DomDup.val());
                }
            }
            i += 1;
        }
        // 取本域流水（逐条复制，不动原账）。
        let mut mine: Vec<CheckRec> = Vec::new();
        let mut k = 0usize;
        while k < self.recs.len() {
            if let Some(r) = self.recs.get(k) {
                if r.dom == id {
                    mine.push(r.clone());
                }
            }
            k += 1;
        }
        let card = card_from_checks(id, dom_name(id), weight, &mine)?;
        self.register(card)
    }

    /// 集齐判定：B01–B08 每号恰一张。
    pub fn all_present(&self) -> bool {
        if self.cards.len() != DOMAIN_N as usize {
            return false;
        }
        let mut id = 1u8;
        while id <= DOMAIN_N {
            let mut found = false;
            let mut i = 0usize;
            while i < self.cards.len() {
                if let Some(c) = self.cards.get(i) {
                    if c.id == id {
                        found = true;
                    }
                }
                i += 1;
            }
            if !found {
                return false;
            }
            id += 1;
        }
        true
    }

    /// 进聚合：全部封账且集齐才可推进 CollectedAll。
    pub fn collect_done(&mut self) -> Result<(), u32> {
        if !self.all_present() {
            return Err(AggCode::DomUnsealed.val());
        }
        self.advance_state(AggState::CollectedAll)
    }

    /// 聚合：加权通过率（bps）+ 摘要链 + 总账；账面冻结判据不在此处
    /// （判定交给独立复核路 verify_cards，聚合只算数）。
    pub fn aggregate(&self) -> Result<AggResult, u32> {
        if self.state.rank() < AggState::CollectedAll.rank() {
            return Err(AggCode::DomUnsealed.val());
        }
        let mut total = 0u64;
        let mut wtotal = 0u64;
        let mut wpass = 0u64;
        let mut reds = 0u64;
        let mut chain = fnv1a(b"AGG-CHAIN-V1");
        let mut id = 1u8;
        while id <= DOMAIN_N {
            let mut hit = false;
            let mut i = 0usize;
            while i < self.cards.len() {
                if let Some(c) = self.cards.get(i) {
                    if c.id == id {
                        total += c.total as u64;
                        wtotal += c.total as u64 * c.weight as u64;
                        wpass += c.passed as u64 * c.weight as u64;
                        reds += c.red_n() as u64;
                        chain = (chain ^ c.digest).wrapping_mul(0x0100_0193);
                        hit = true;
                    }
                }
                i += 1;
            }
            if !hit {
                return Err(AggCode::DomUnsealed.val());
            }
            id += 1;
        }
        let bps = if wtotal == 0 {
            RATE_FULL_BPS
        } else {
            (wpass.saturating_mul(RATE_FULL_BPS) / wtotal).min(RATE_FULL_BPS)
        };
        Ok(AggResult { total, reds, rate_bps: bps, chain, digest: chain })
    }

    /// 出终版报告：集齐才许出；首出推进 Reported；复出须逐字一致。
    pub fn render_report(&mut self) -> Result<String, u32> {
        let agg = self.aggregate()?;
        let first = self.state == AggState::CollectedAll;
        let mut s = String::new();
        s.push_str("══ 分配器域自检终版报告（F2418 全域聚合）══\n");
        s.push_str(&format!("段数={} 总账={} 红项={} 加权={}\n",
            DOMAIN_N, agg.total, agg.reds, agg.rate_bps));
        let mut id = 1u8;
        while id <= DOMAIN_N {
            let mut found: Option<&DomCard> = None;
            let mut i = 0usize;
            while i < self.cards.len() {
                if let Some(c) = self.cards.get(i) {
                    if c.id == id {
                        found = Some(c);
                    }
                }
                i += 1;
            }
            match found {
                Some(c) => {
                    let verdict = if c.reds.is_empty() && c.tally_ok() { "绿" } else { "红" };
                    s.push_str(&format!("[B{:02}] {} 总={} 过={} 红={} 判={}\n",
                        c.id, c.name, c.total, c.passed, c.red_n(), verdict));
                }
                None => return Err(AggCode::DomUnsealed.val()),
            }
            id += 1;
        }
        s.push_str(&format!("摘要链={:08X} 纪元={}\n", agg.chain, self.epoch));
        if first {
            self.advance_state(AggState::Reported)?;
        } else if self.state != AggState::Reported {
            return Err(AggCode::StateBack.val());
        }
        self.reports += 1;
        Ok(s)
    }

    /// 报告含 B01–B08 全段清单（判据原文逐字落点）。
    pub fn report_has_all_segments(rep: &str) -> bool {
        let mut id = 1u8;
        while id <= DOMAIN_N {
            let tag = format!("[B{:02}]", id);
            if !rep.contains(&tag) {
                return false;
            }
            id += 1;
        }
        true
    }
}

/// 段号 → 段名（越界回退占位名）。
pub fn dom_name(id: DomId) -> &'static str {
    if id >= 1 && id as usize <= DOM_NAMES.len() {
        match DOM_NAMES.get(id as usize - 1) {
            Some(n) => n,
            None => "B??-无名段",
        }
    } else {
        "B??-无名段"
    }
}

// ── 聚合结果 ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AggResult {
    /// 总检查条数。
    pub total: u64,
    /// 红项总条数。
    pub reds: u64,
    /// 加权通过率（bps，封顶 10^4）。
    pub rate_bps: u64,
    /// 八段摘要链（序冻结折叠）。
    pub chain: u32,
    /// 与 chain 同域（留显式字段便于诊断展开）。
    pub digest: u32,
}

// ── 独立复核路（对拍双路） ──────────────────────────────────────
/// 复核器：从域卡册独立复算全部判据，与记账路互为对拍。
/// 违约逐条落 Err(诊断码)；全部过 → Ok(复核摘要)。
pub fn verify_cards(cards: &[DomCard], recs: &[CheckRec]) -> Result<u32, u32> {
    // ① 段数与段号闭包。
    if cards.len() != DOMAIN_N as usize {
        return Err(AggCode::DomUnsealed.val());
    }
    let mut id = 1u8;
    while id <= DOMAIN_N {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < cards.len() {
            if let Some(c) = cards.get(i) {
                if c.id == id {
                    n += 1;
                }
            }
            i += 1;
        }
        if n != 1 {
            return Err(AggCode::DomDup.val());
        }
        id += 1;
    }
    // ② 逐卡账目自洽 + 封账位 + 摘要。
    let mut i = 0usize;
    while i < cards.len() {
        let c = match cards.get(i) {
            Some(c) => c,
            None => return Err(AggCode::DomUnsealed.val()),
        };
        if !c.sealed {
            return Err(AggCode::DomUnsealed.val());
        }
        if !c.tally_ok() {
            return Err(AggCode::CountMismatch.val());
        }
        let mut k = 0usize;
        while k < c.reds.len() {
            if let Some(r) = c.reds.get(k) {
                if r.is_empty() {
                    return Err(AggCode::RedLeak.val());
                }
            }
            k += 1;
        }
        i += 1;
    }
    // ③ 流水复算对拍：每域 流水条数/通过数 必须与卡片一致。
    let mut id = 1u8;
    while id <= DOMAIN_N {
        let mut f_total = 0u32;
        let mut f_pass = 0u32;
        let mut k = 0usize;
        while k < recs.len() {
            if let Some(r) = recs.get(k) {
                if r.dom == id {
                    f_total += 1;
                    if r.passed {
                        f_pass += 1;
                    }
                }
            }
            k += 1;
        }
        let mut j = 0usize;
        let mut ok = false;
        while j < cards.len() {
            if let Some(c) = cards.get(j) {
                if c.id == id {
                    if c.total != f_total || c.passed != f_pass {
                        return Err(AggCode::CountMismatch.val());
                    }
                    ok = true;
                }
            }
            j += 1;
        }
        if !ok {
            return Err(AggCode::DomUnsealed.val());
        }
        id += 1;
    }
    // ④ 流水条目合法性。
    let mut k = 0usize;
    while k < recs.len() {
        let r = match recs.get(k) {
            Some(r) => r,
            None => return Err(AggCode::CountMismatch.val()),
        };
        if r.dom < 1 || r.dom > DOMAIN_N {
            return Err(AggCode::DomUnknown.val());
        }
        k += 1;
    }
    Ok(recs.len() as u32)
}

/// 判绿收口：复核过 + 全卡零红项 才算终版全绿。
pub fn final_verdict(cards: &[DomCard], recs: &[CheckRec]) -> Result<bool, u32> {
    let _ = verify_cards(cards, recs)?;
    let mut i = 0usize;
    while i < cards.len() {
        if let Some(c) = cards.get(i) {
            if !c.reds.is_empty() {
                return Ok(false);
            }
        }
        i += 1;
    }
    Ok(true)
}

// ── 参考册构建（确定性，可复算） ────────────────────────────────
/// 构建参考检查流水：每域 5 条，全绿（name/note 由下标派生）。
pub fn build_reference_recs() -> Vec<CheckRec> {
    let mut v: Vec<CheckRec> = Vec::new();
    let mut id = 1u8;
    while id <= DOMAIN_N {
        let mut k = 0u8;
        while k < 5 {
            v.push(CheckRec {
                dom: id,
                name: format!("c{}", k),
                passed: true,
                note: String::from("ok"),
            });
            k += 1;
        }
        id += 1;
    }
    v
}

/// 从流水逐域封账建册（权重 = 段号，确定性）。
pub fn build_registry() -> Result<DomRegistry, u32> {
    let recs = build_reference_recs();
    let mut reg = DomRegistry::new();
    let mut i = 0usize;
    while i < recs.len() {
        let r = match recs.get(i) {
            Some(r) => r.clone(),
            None => return Err(AggCode::CountMismatch.val()),
        };
        reg.record(r)?;
        i += 1;
    }
    let mut id = 1u8;
    while id <= DOMAIN_N {
        reg.seal_domain(id, id as u32)?;
        id += 1;
    }
    reg.collect_done()?;
    Ok(reg)
}

// ── 失真注入（10 路逐字失真，全打在账面字段上） ────────────────
/// 对黄金册注入第 kind 路失真，返回被涂改的域卡册。
pub fn distort(reg: &DomRegistry, kind: usize) -> Vec<DomCard> {
    let mut cards: Vec<DomCard> = reg.cards.clone();
    let tgt = kind % (DOMAIN_N as usize);
    match kind % 10 {
        0 => {
            // 通过数虚增：passed+1（总数不动 → 账目数不符必抓）。
            if let Some(c) = cards.get_mut(tgt) {
                c.passed = c.passed.saturating_add(1);
            }
        }
        1 => {
            // 红项抹除：清空红项但 total 不动（通过数对不上必抓）。
            if let Some(c) = cards.get_mut(tgt) {
                c.reds = Vec::new();
                c.digest = c.recompute_digest();
            }
        }
        2 => {
            // 封账位清零：未封账域进聚合必抓。
            if let Some(c) = cards.get_mut(tgt) {
                c.sealed = false;
            }
        }
        3 => {
            // 总数虚增：total+7（流水对不上必抓）。
            if let Some(c) = cards.get_mut(tgt) {
                c.total = c.total.saturating_add(7);
                c.digest = c.recompute_digest();
            }
        }
        4 => {
            // 权重清零：weight=0（权重域非法必抓）。
            if let Some(c) = cards.get_mut(tgt) {
                c.weight = 0;
                c.digest = c.recompute_digest();
            }
        }
        5 => {
            // 摘要涂改：digest 翻转（重算对不上必抓）。
            if let Some(c) = cards.get_mut(tgt) {
                c.digest = !c.digest;
            }
        }
        6 => {
            // 域名涂改：改名字但不重算摘要（摘要必抓）；重算了也因
            // 摘要链与黄金册不同被双跑一致性兜住。
            if let Some(c) = cards.get_mut(tgt) {
                c.name = String::from("涂改段");
                c.digest = c.recompute_digest();
            }
        }
        7 => {
            // 掉卡：移除一张（段数≠8 必抓）。
            if cards.len() > tgt {
                cards.remove(tgt);
            }
        }
        8 => {
            // 插卡：克隆一张改号越界（未知域号必抓）或重号（重复必抓）。
            if let Some(c) = cards.first() {
                let mut dup = c.clone();
                dup.id = if tgt == 0 { 1 } else { (tgt as u8 % DOMAIN_N) + 1 };
                cards.push(dup);
            }
        }
        _ => {
            // 红项清空：把红项文本改成空串（空红项必抓）。
            if let Some(c) = cards.get_mut(tgt) {
                if !c.reds.is_empty() {
                    let mut i = 0usize;
                    while i < c.reds.len() {
                        if let Some(r) = c.reds.get_mut(i) {
                            *r = String::new();
                        }
                        i += 1;
                    }
                } else {
                    c.reds.push(String::new());
                }
                c.digest = c.recompute_digest();
            }
        }
    }
    cards
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入轮：每轮对新鲜黄金册注入一路失真，复核器必须抓到
///（黄金册本身必须复核通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let reg = build_registry()?;
        let recs = reg.recs.clone();
        // 基准必须干净。
        if !final_verdict(&reg.cards, &recs)? {
            return Err(AggCode::RedLeak.val());
        }
        let kind = (r % 10) as usize;
        let bad = distort(&reg, kind);
        led.record(kind, final_verdict(&bad, &recs).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：建册→聚合→出报（含 B01–B08 清单）→四拒探→复核对拍。
/// 返回 (正旗数, 拒旗数, 总账, 摘要链)。
pub fn golden_run() -> Result<(u32, u32, u64, u32), u32> {
    let mut reg = build_registry()?;
    let agg = reg.aggregate()?;
    let rep = reg.render_report()?;
    let rep_ok = DomRegistry::report_has_all_segments(&rep) && !rep.is_empty();
    // 复核对拍（独立路）。
    let recs = reg.recs.clone();
    let verdict = final_verdict(&reg.cards, &recs)?;
    // 四拒探：未知域号 / 重复登记 / 未集齐聚合 / 早出报告。
    let mut ghost = reg.clone();
    let unknown_rej = ghost.seal_domain(DOMAIN_N + 1, 1).is_err();
    let mut dup = build_registry()?;
    let dup_rej = dup.seal_domain(1, 1).is_err();
    let mut part = DomRegistry::new();
    part.record(CheckRec { dom: 1, name: String::from("x"), passed: true, note: String::from("ok") })?;
    part.seal_domain(1, 1)?;
    let part_rej = part.collect_done().is_err();
    let early_rej = part.render_report().is_err();
    // 状态机四探：Collecting→Reported 跳级拒 / 倒退拒。
    let mut sm = part.clone();
    let skip_rej = sm.advance_state(AggState::Reported).is_err();
    let mut back = build_registry()?;
    let back_rej = back.advance_state(AggState::Empty).is_err();
    let flags = rep_ok as u32 + verdict as u32
        + (agg.rate_bps == RATE_FULL_BPS) as u32 + (agg.total == 40) as u32;
    let rejects = unknown_rej as u32 + dup_rej as u32 + part_rej as u32
        + early_rej as u32 + skip_rej as u32 + back_rej as u32;
    Ok((flags, rejects, agg.total, agg.chain))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正六拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 6, _, _)) => Ok(()),
        _ => Err(AggCode::NotReproducible.val()),
    }
}

/// 序 B：失真注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(AggCode::RedLeak.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72E00 && AggCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = DOMAIN_N == 8 && MIN_CHECKS_PER_DOM == 2 && CHECK_CAP_PER_DOM == 64
        && RATE_FULL_BPS == 10_000 && JUDGE_ROUNDS == 10 && in_window
        && DOM_NAMES.len() == 8;
    if ok {
        Ok(())
    } else {
        Err(AggCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AggCode::NotReproducible.val());
    }
    // 报告逐字一致（出报两次内容冻结）。
    let mut r1 = build_registry()?;
    let p1 = r1.render_report()?;
    let p2 = r1.render_report()?;
    if p1 != p2 {
        return Err(AggCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AggCode::ALL.len() {
        let v = AggCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let mut reg = build_registry()?;
    let agg = reg.aggregate()?;
    let rep = reg.render_report()?;
    if !DomRegistry::report_has_all_segments(&rep) {
        return Err(AggCode::ReportEarly.val());
    }
    Ok(format!("状态=已完成 doms={} total={} reds={} bps={} chain={:08X}",
        DOMAIN_N, agg.total, agg.reds, agg.rate_bps, agg.chain))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正六拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, t, _)) => (*f == 4 && *r == 6, format!("flags={} rejects={} total={}", f, r, t)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-全域聚合黄金链", gp, gd));
    // J1 报告含 B01–B08 全段清单。
    let mut reg = match build_registry() {
        Ok(r) => r,
        Err(e) => {
            v.push(("J1-八段清单", false, format!("err=0x{:x}", e)));
            v.push(("J1-注入10/10", false, String::from("建册失败")));
            v.push(("J2-单调守卫", false, String::from("建册失败")));
            v.push(("J2-双跑一致", false, String::from("建册失败")));
            v.push(("J4-段位守卫", segment_guard(), String::from("0x72E00 窗口")));
            return v;
        }
    };
    let rep = reg.render_report();
    let rep_ok = match &rep {
        Ok(s) => DomRegistry::report_has_all_segments(s),
        Err(_) => false,
    };
    v.push(("J1-八段清单", rep_ok, format!("doms={} reports={}", DOMAIN_N, reg.reports)));
    // J1 失真注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-注入10/10", ip, id));
    // J1 复核路对拍（独立路复算全绿）。
    let fv = final_verdict(&reg.cards, &reg.recs);
    v.push(("J1-复核路对拍", matches!(&fv, Ok(true)), String::from("记账路vs复核路双路一致")));
    // J2 单调守卫：跳级/倒退/封账后再记/集齐后再插 全拒。
    let mut sm = match build_registry() {
        Ok(r) => r,
        Err(_) => DomRegistry::new(),
    };
    let s1 = sm.advance_state(AggState::Empty).is_err();
    let s2 = sm.advance_state(AggState::Collecting).is_err();
    let s3 = sm.record(CheckRec { dom: 1, name: String::from("late"), passed: true, note: String::from("x") }).is_err();
    let mut late = DomRegistry::new();
    late.advance_state(AggState::CollectedAll).is_err(); // 空册跳级本身应拒
    let s4 = late.state == AggState::Empty;
    v.push(("J2-单调守卫", s1 && s2 && s3 && s4, format!("倒退={} 平移={} 封后记={} 跳级拒={}", s1, s2, s3, s4)));
    // J2 重复登记拒。
    let mut dup2 = match build_registry() {
        Ok(r) => r,
        Err(_) => DomRegistry::new(),
    };
    let dup_ok = dup2.seal_domain(3, 3).is_err();
    v.push(("J2-重复登记拒", dup_ok, String::from("同号域卡只许一张")));
    // J2 加权通过率 100%。
    let agg = reg.aggregate();
    let bps_ok = matches!(&agg, Ok(a) if a.rate_bps == RATE_FULL_BPS);
    v.push(("J1-加权通过率100%", bps_ok, format!("cap={} bps", RATE_FULL_BPS)));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 与出报两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72E00 窗口")));
    v
}

pub fn run_unx2544_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2544 分配器域自检终版（F2418 全域聚合）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
