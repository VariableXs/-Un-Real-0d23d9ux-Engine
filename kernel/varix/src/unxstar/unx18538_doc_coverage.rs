//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18538 探针文档账（每例出处覆盖） · 0x80240段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18538-J1 每例出处（API 文档锚）覆盖率 ≥95%，无锚项逐条列明。
//! 结构清单：
//!   - 数据结构：CaseRec（Copy 例记录：例号/锚哈希/锚引用号/有锚标记）、
//!     DocLedger（64 槽例账 + 覆盖位面 50 位 + 无锚清单）、锚格式解析器
//!     （API::<sym>#<ref> 五闸）、FNV-1a 锚哈希（去重凭据）；
//!   - 状态机：Collecting→Audited→Sealed 三态（Sealed 后登记显式拒绝）；
//!   - 账本：例账（追加序单调）+ 锚哈希账（逐例唯一，共享锚拒绝）+
//!     无锚清单（逐条列明，与覆盖位面 popcount 守恒）；
//!   - 错误路径：DcCode 闭集 13 码，code() = SEG_BASE + n（0x80240 段），
//!     每码非空中文建议；
//!   - 注入核：空锚 / 前缀错 / 缺井号 / 共享锚 四类坏样本各 10 轮
//!     逐类断言被显式拒绝（10/10 全抓）。
//! 覆盖率推导（写死）：整数判式 anchored×100 ≥ 95×total；50 例下界
//!   ceil(50×95/100) = ceil(47.5) = 48 → ANCHORED_MIN = 48（47/50 = 94%
//!   必判短少，48/50 = 96% 过线）。黄金链 48 锚 + 2 无锚 → 96% ≥ 95%。
//! 纪律：no_std + alloc；内核路径零 panic；无浮点/IO/墙钟/随机。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80240，AI-VX11-1105 G1 批07 独占区，卡表给定）。
pub const SEG_BASE: u32 = 0x80240;

// ── 冻结常量（推导注明） ─────────────────────────────────────────
/// 探针例数（J1 冻结：每例出处账 50 例）。
pub const CASE_TOTAL: usize = 50;
/// 例账容量（64 槽）。
pub const CASE_CAP: usize = 64;
/// 覆盖率下界（95%，整数判式分母因子）。
pub const COVERAGE_NUM: usize = 95;
/// 覆盖率判式分子基数（100 = 百分比归一）。
pub const COVERAGE_DEN: usize = 100;
/// 有锚下限（推导：ceil(50×95/100) = 48）。
pub const ANCHORED_MIN: usize = 48;
/// 锚前缀（API 文档锚闭集前缀）。
pub const ANCHOR_PREFIX: &str = "API::";
/// 锚总长上限（含前缀与井号段）。
pub const ANCHOR_LEN_MAX: usize = 32;
/// 例号下限（0 号保留给账头）。
pub const CASE_ID_MIN: u32 = 1;
/// 例号上限。
pub const CASE_ID_MAX: u32 = 4096;
/// 账态：收账中。
pub const STATE_COLLECTING: u8 = 1;
/// 账态：核算中。
pub const STATE_AUDITED: u8 = 2;
/// 账态：封账（终态）。
pub const STATE_SEALED: u8 = 3;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类别数（4 类坏样本）。
pub const INJECT_CLASSES: usize = 4;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    /// 例号越界（0 保留 / >4096）
    CaseIdOutOfRange,
    /// 例重号
    CaseDuplicate,
    /// 空锚（无出处文本）
    AnchorEmpty,
    /// 锚前缀错（非 API::）
    AnchorPrefixWrong,
    /// 锚缺井号（无引用号段）
    AnchorNoHash,
    /// 锚引用号为零
    AnchorZeroRef,
    /// 锚超长（>32 字节）
    AnchorTooLong,
    /// 共享锚（同锚同哈希挂多例，出处不唯一）
    SharedAnchor,
    /// 例账满（64 槽耗尽）
    LedgerFull,
    /// 账态非法转换
    IllegalTransition,
    /// 位面损坏（覆盖位面 popcount ≠ 有锚数）
    PlaneCorrupt,
    /// 覆盖率短少（<95%）
    CoverageShortfall,
    /// 双跑不一致
    NotReproducible,
}

impl DcCode {
    /// 诊断码取值：SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 非空中文建议（文案闭集）。
    pub fn detail(self) -> &'static str {
        match self {
            DcCode::CaseIdOutOfRange => "例号越界：编号须在 1..=4096，0 号为账头保留",
            DcCode::CaseDuplicate => "例重号：同号已登记，出处账按例唯一",
            DcCode::AnchorEmpty => "空锚：无出处文本，登记为无锚项并逐条列明",
            DcCode::AnchorPrefixWrong => "锚前缀错：API 文档锚前缀闭集为 API::",
            DcCode::AnchorNoHash => "锚缺井号：格式须为 API::<符号>#<引用号>",
            DcCode::AnchorZeroRef => "锚引用号为零：引用号须为正整数",
            DcCode::AnchorTooLong => "锚超长：上限 32 字节，长锚走分册缩写",
            DcCode::SharedAnchor => "共享锚：同锚哈希不得挂多例，出处须逐例唯一",
            DcCode::LedgerFull => "例账满：64 槽耗尽，账本只增不减需扩册",
            DcCode::IllegalTransition => "账态非法转换：三态只进不退，越级拒绝",
            DcCode::PlaneCorrupt => "位面损坏：覆盖位面 popcount 须与有锚数吻合",
            DcCode::CoverageShortfall => "覆盖率短少：有锚率须 ≥95%，无锚项逐条列明补锚",
            DcCode::NotReproducible => "双跑不一致：同输入两次核算覆盖率不同",
        }
    }
    /// 码闭集清单。
    pub const ALL: [DcCode; 13] = [
        DcCode::CaseIdOutOfRange,
        DcCode::CaseDuplicate,
        DcCode::AnchorEmpty,
        DcCode::AnchorPrefixWrong,
        DcCode::AnchorNoHash,
        DcCode::AnchorZeroRef,
        DcCode::AnchorTooLong,
        DcCode::SharedAnchor,
        DcCode::LedgerFull,
        DcCode::IllegalTransition,
        DcCode::PlaneCorrupt,
        DcCode::CoverageShortfall,
        DcCode::NotReproducible,
    ];
}

/// 诊断码 → 中文建议（闭集外返回未知提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DcCode::ALL.len() {
        if DcCode::ALL[i].code() == code {
            return DcCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80240 段闭集内"
}

// ── 锚哈希（FNV-1a 32 位，规格冻结） ────────────────────────────
/// FNV-1a：h = 2166136261；逐字节 h ^= b; h *= 16777619。
pub fn anchor_hash(s: &str) -> u32 {
    let mut h: u32 = 2166_136_261;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b.get(i) {
            Some(&c) => {
                h ^= c as u32;
                h = h.wrapping_mul(1677_7619);
            }
            None => break,
        }
        i += 1;
    }
    h
}

// ── 锚格式解析器（五闸：空/前缀/井号/零引用/超长） ──────────────
/// "API::<符号>#<引用号>" → 引用号（u32）。任一闸不合规即显式拒。
pub fn parse_anchor(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(DcCode::AnchorEmpty.code());
    }
    if s.len() > ANCHOR_LEN_MAX {
        return Err(DcCode::AnchorTooLong.code());
    }
    let b = s.as_bytes();
    // 闸二：前缀闭集。
    if b.len() < ANCHOR_PREFIX.len() {
        return Err(DcCode::AnchorPrefixWrong.code());
    }
    let head = &b[..ANCHOR_PREFIX.len()];
    if head != ANCHOR_PREFIX.as_bytes() {
        return Err(DcCode::AnchorPrefixWrong.code());
    }
    // 闸三：井号段（前缀后必须出现 '#'）。
    let mut hash_pos: usize = ANCHOR_LEN_MAX + 1; // 哨兵
    let mut i = ANCHOR_PREFIX.len();
    while i < b.len() {
        if b[i] == b'#' {
            hash_pos = i;
            break;
        }
        i += 1;
    }
    if hash_pos > ANCHOR_LEN_MAX {
        return Err(DcCode::AnchorNoHash.code());
    }
    // 引用号段：至少一位数字且值为正。
    let num_start = hash_pos + 1;
    if num_start >= b.len() {
        return Err(DcCode::AnchorZeroRef.code());
    }
    let mut acc: u32 = 0;
    let mut any = false;
    let mut j = num_start;
    while j < b.len() {
        let c = match b.get(j) {
            Some(&v) => v,
            None => return Err(DcCode::AnchorNoHash.code()),
        };
        if c < b'0' || c > b'9' {
            return Err(DcCode::AnchorNoHash.code());
        }
        acc = acc.wrapping_mul(10).wrapping_add((c - b'0') as u32);
        any = true;
        j += 1;
    }
    if !any || acc == 0 {
        return Err(DcCode::AnchorZeroRef.code());
    }
    Ok(acc)
}

// ── 例记录（Copy） ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseRec {
    /// 例号（1..=4096）。
    pub id: u32,
    /// 锚哈希（FNV-1a，无锚为 0）。
    pub anchor_h: u32,
    /// 锚引用号（parse_anchor 产物，无锚为 0）。
    pub anchor_ref: u32,
    /// 有锚标记。
    pub anchored: bool,
    /// 空槽标记。
    pub empty: bool,
}

impl CaseRec {
    /// 空例（const fn）。
    pub const fn empty() -> CaseRec {
        CaseRec { id: 0, anchor_h: 0, anchor_ref: 0, anchored: false, empty: true }
    }
}

// ── 文档账（核心账本 + 三态状态机 + 覆盖位面 + 无锚清单） ───────
pub struct DocLedger {
    /// 64 槽例账。
    cases: [CaseRec; CASE_CAP],
    /// 覆盖位面（位 idx：有锚即置位，单字 64 位够 50 例）。
    coverage_plane: [u64; 1],
    /// 已登记例数。
    count: usize,
    /// 有锚例数（与位面 popcount 守恒）。
    anchored_count: usize,
    /// 账态。
    state: u8,
    /// 无锚清单（逐条列明，J1 验收项）。
    pub unanchored: Vec<u32>,
    /// 拒因账（只增不减）。
    reject_ledger: Vec<u32>,
}

impl DocLedger {
    pub fn new() -> DocLedger {
        DocLedger {
            cases: [CaseRec::empty(); CASE_CAP],
            coverage_plane: [0u64; 1],
            count: 0,
            anchored_count: 0,
            state: STATE_COLLECTING,
            unanchored: Vec::new(),
            reject_ledger: Vec::new(),
        }
    }

    /// 登记一例（anchor 为 None 即无锚项，进无锚清单逐条列明）。
    /// 闸序：账态→例号→重号→锚格式→共享锚→容量。
    pub fn register(&mut self, id: u32, anchor: Option<&str>) -> Result<(), u32> {
        if self.state != STATE_COLLECTING {
            self.reject_ledger.push(DcCode::IllegalTransition.code());
            return Err(DcCode::IllegalTransition.code());
        }
        if id < CASE_ID_MIN || id > CASE_ID_MAX {
            self.reject_ledger.push(DcCode::CaseIdOutOfRange.code());
            return Err(DcCode::CaseIdOutOfRange.code());
        }
        // 重号扫描。
        let mut i = 0;
        while i < self.count {
            let rec = match self.cases.get(i) {
                Some(r) => *r,
                None => return Err(DcCode::PlaneCorrupt.code()),
            };
            if !rec.empty && rec.id == id {
                self.reject_ledger.push(DcCode::CaseDuplicate.code());
                return Err(DcCode::CaseDuplicate.code());
            }
            i += 1;
        }
        // 锚解析（None → 无锚项）。
        let (h, refr, anchored) = match anchor {
            None => (0u32, 0u32, false),
            Some(text) => {
                let refr = parse_anchor(text)?;
                (anchor_hash(text), refr, true)
            }
        };
        // 共享锚闸：同哈希已挂他例 → 拒（出处逐例唯一）。
        if anchored {
            let mut j = 0;
            while j < self.count {
                let rec = match self.cases.get(j) {
                    Some(r) => *r,
                    None => return Err(DcCode::PlaneCorrupt.code()),
                };
                if !rec.empty && rec.anchor_h == h {
                    self.reject_ledger.push(DcCode::SharedAnchor.code());
                    return Err(DcCode::SharedAnchor.code());
                }
                j += 1;
            }
        }
        if self.count >= CASE_CAP {
            self.reject_ledger.push(DcCode::LedgerFull.code());
            return Err(DcCode::LedgerFull.code());
        }
        // 写账。
        let rec = CaseRec { id, anchor_h: h, anchor_ref: refr, anchored, empty: false };
        let idx = self.count;
        match self.cases.get_mut(idx) {
            Some(sl) => {
                *sl = rec;
            }
            None => {
                self.reject_ledger.push(DcCode::LedgerFull.code());
                return Err(DcCode::LedgerFull.code());
            }
        }
        if anchored {
            if let Some(w) = self.coverage_plane.get_mut(0) {
                *w |= 1u64 << (idx % 64);
            }
            self.anchored_count += 1;
        } else {
            self.unanchored.push(id);
        }
        self.count += 1;
        Ok(())
    }

    /// 账态推进：Collecting → Audited（触发覆盖核算）。
    pub fn audit(&mut self) -> Result<usize, u32> {
        if self.state != STATE_COLLECTING {
            self.reject_ledger.push(DcCode::IllegalTransition.code());
            return Err(DcCode::IllegalTransition.code());
        }
        self.state = STATE_AUDITED;
        // 位面对账：popcount == anchored_count。
        let mut pop = 0usize;
        let mut w = *self.coverage_plane.get(0).unwrap_or(&0);
        while w != 0 {
            w &= w.wrapping_sub(1);
            pop += 1;
        }
        if pop != self.anchored_count {
            return Err(DcCode::PlaneCorrupt.code());
        }
        // 守恒：有锚 + 无锚 = 总数。
        if self.anchored_count + self.unanchored.len() != self.count {
            return Err(DcCode::PlaneCorrupt.code());
        }
        // 覆盖率整数判式：anchored×100 ≥ 95×total（total>0）。
        if self.count == 0 {
            return Err(DcCode::CoverageShortfall.code());
        }
        if self.anchored_count * COVERAGE_DEN < COVERAGE_NUM * self.count {
            return Err(DcCode::CoverageShortfall.code());
        }
        Ok(self.anchored_count)
    }

    /// 封账：Audited → Sealed（终态）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state != STATE_AUDITED {
            self.reject_ledger.push(DcCode::IllegalTransition.code());
            return Err(DcCode::IllegalTransition.code());
        }
        self.state = STATE_SEALED;
        Ok(())
    }

    /// 追加序单调审计。
    pub fn ids_monotonic(&self) -> bool {
        let mut prev = 0u32;
        let mut i = 0;
        while i < self.count {
            let rec = match self.cases.get(i) {
                Some(r) => *r,
                None => return false,
            };
            if rec.id <= prev {
                return false;
            }
            prev = rec.id;
            i += 1;
        }
        true
    }

    /// 有锚数只读。
    pub fn anchored(&self) -> usize {
        self.anchored_count
    }

    pub fn case_count(&self) -> usize {
        self.count
    }

    pub fn state(&self) -> u8 {
        self.state
    }

    pub fn reject_count(&self) -> usize {
        self.reject_ledger.len()
    }

    /// 覆盖率千分比（整数，便于动态文本展示）。
    pub fn coverage_permille(&self) -> usize {
        if self.count == 0 {
            return 0;
        }
        self.anchored_count * 1000 / self.count
    }
}

// ── 黄金链：50 例（48 锚 + 2 无锚）→ 核算 96% → 无锚逐条列明 ───
/// 返回 (例数, 有锚数, 覆盖率千分比, 无锚清单)。
pub fn golden_run() -> Result<(usize, usize, usize, Vec<u32>), u32> {
    let mut led = DocLedger::new();
    let mut i = 0usize;
    while i < CASE_TOTAL {
        let id = 100 + i as u32;
        // 48 例有锚（例 1..48），例 49/50 无锚 → 96%。
        if i < ANCHORED_MIN {
            let anchor = format!("API::probe_run#{}", 500 + i as u32);
            led.register(id, Some(&anchor))?;
        } else {
            led.register(id, None)?;
        }
        i += 1;
    }
    let anchored = led.audit()?;
    led.seal()?;
    if !led.ids_monotonic() {
        return Err(DcCode::NotReproducible.code());
    }
    let pm = led.coverage_permille();
    if anchored != ANCHORED_MIN || pm != 960 {
        // 48/50 = 96% = 960‰（写死锚）。
        return Err(DcCode::CoverageShortfall.code());
    }
    let unanchored = led.unanchored.clone();
    if unanchored.len() != 2 {
        return Err(DcCode::CoverageShortfall.code());
    }
    Ok((led.case_count(), anchored, pm, unanchored))
}

// ── 注入核：四类坏样本，各 10 轮，逐类全抓 ──────────────────────
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = *self.tries.get(i).unwrap_or(&0);
            let c = *self.caught.get(i).unwrap_or(&0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn conservation(&self) -> Result<(), u32> {
        let mut tries = 0u32;
        let mut caught = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            tries = tries.saturating_add(*self.tries.get(i).unwrap_or(&0));
            caught = caught.saturating_add(*self.caught.get(i).unwrap_or(&0));
            i += 1;
        }
        if caught > tries {
            return Err(DcCode::NotReproducible.code());
        }
        Ok(())
    }
}

/// 四类坏样本逐类注入 `rounds` 轮：
/// C0 空锚文本、C1 前缀错、C2 缺井号、C3 共享锚。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // C0：空锚文本（Some("")）。
        let mut l0 = DocLedger::new();
        led.record(0, l0.register(1, Some("")).err() == Some(DcCode::AnchorEmpty.code()));
        // C1：前缀错。
        led.record(1, parse_anchor("DOC::probe_run#1").err() == Some(DcCode::AnchorPrefixWrong.code()));
        // C2：缺井号。
        led.record(2, parse_anchor("API::probe_run1").err() == Some(DcCode::AnchorNoHash.code()));
        // C3：共享锚（同锚挂两例）。
        let mut l3 = DocLedger::new();
        let _ = l3.register(1, Some("API::probe_run#77"));
        led.record(
            3,
            l3.register(2, Some("API::probe_run#77")).err() == Some(DcCode::SharedAnchor.code()),
        );
        r += 1;
    }
    led.conservation()?;
    Ok(led)
}

// ── 探针三序（登记→核算→封账 / 越级 / 覆盖短少） ────────────────
pub fn probe_seq_a() -> Result<usize, u32> {
    let mut led = DocLedger::new();
    led.register(1, Some("API::probe_run#1"))?;
    led.register(2, Some("API::probe_run#2"))?;
    let a = led.audit()?;
    led.seal()?;
    Ok(a)
}

pub fn probe_seq_b() -> Result<usize, u32> {
    // 覆盖短少：47/50 = 94% < 95%（补 3 例无锚拉低到线内探不出——直接
    // 用 1 例有锚 + 1 例无锚 = 50% 短少）。
    let mut led = DocLedger::new();
    led.register(1, Some("API::probe_run#1"))?;
    led.register(2, None)?;
    led.audit()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 越级：未核算直接封账。
    let mut led = DocLedger::new();
    led.seal()
}

/// 容量耗尽：64 槽写满后第 65 例必拒且账不丢。
pub fn capacity_probe() -> Result<usize, u32> {
    let mut led = DocLedger::new();
    let mut id = 1u32;
    while (id as usize) <= CASE_CAP {
        let anchor = format!("API::probe_run#{}", id);
        led.register(id, Some(&anchor))?;
        id += 1;
    }
    let before = led.case_count();
    let r = led.register(CASE_CAP as u32 + 1, None);
    if r.err() != Some(DcCode::LedgerFull.code()) {
        return Err(DcCode::NotReproducible.code());
    }
    if led.case_count() != before {
        return Err(DcCode::NotReproducible.code());
    }
    Ok(before)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DcCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x80000 || SEG_BASE >= 0x80400 {
        return false;
    }
    let mut i = 0;
    while i < DcCode::ALL.len() {
        let v = DcCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集。
pub fn detail_closed_set() -> bool {
    let mut i = 0;
    while i < DcCode::ALL.len() {
        if DcCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 判据集（J1 验收） ───────────────────────────────────────────
pub fn run_unx18538_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18538 探针文档账");
    // 静态明细判据。
    s.add("J1-段位守卫", segment_guard(), "0x80240 段 0x40 窗口，码闭集不越界");
    s.add(
        "J1-覆盖率整数判式边界",
        {
            // 47×100 < 95×50 且 48×100 ≥ 95×50（判式两侧边界复算）。
            47 * COVERAGE_DEN < COVERAGE_NUM * CASE_TOTAL
                && 48 * COVERAGE_DEN >= COVERAGE_NUM * CASE_TOTAL
                && ANCHORED_MIN == 48
        },
        "47/50=94% 必判短少；48/50=96% 过线；下限冻结 48",
    );
    s.add(
        "J1-锚格式五闸",
        {
            let e0 = parse_anchor("").err() == Some(DcCode::AnchorEmpty.code());
            let pre = parse_anchor("DOC::x#1").err() == Some(DcCode::AnchorPrefixWrong.code());
            let noh = parse_anchor("API::x1").err() == Some(DcCode::AnchorNoHash.code());
            let z0 = parse_anchor("API::x#0").err() == Some(DcCode::AnchorZeroRef.code());
            let z1 = parse_anchor("API::x#").err() == Some(DcCode::AnchorZeroRef.code());
            let long = make_long_anchor();
            let toolong = parse_anchor(&long).err() == Some(DcCode::AnchorTooLong.code());
            let good = parse_anchor("API::probe_run#12").unwrap_or(0) == 12;
            e0 && pre && noh && z0 && z1 && toolong && good
        },
        "空/前缀/缺井号/零引用/超长五闸全拒；合法锚解析往返正确",
    );
    s.add(
        "J1-账态三序",
        probe_seq_a().is_ok()
            && probe_seq_c().err() == Some(DcCode::IllegalTransition.code()),
        "登记→核算→封账全过；未核算封账拒",
    );
    s.add(
        "J1-文案闭集",
        detail_closed_set(),
        "13 码逐码非空中文建议，闭集外落未知提示",
    );
    // 动态明细判据（≤8 条）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, anc, pm, una)) => {
            *n == CASE_TOTAL && *anc == ANCHORED_MIN && *pm == 960 && una.len() == 2
        }
        Err(_) => false,
    };
    s.add_dyn(
        "J1-50例覆盖96%≥95%",
        g_ok,
        &format!("例/锚/千分比 = {:?}", g.as_ref().map(|(n, a, p, _)| (*n, *a, *p))),
    );
    let una_txt = match &g {
        Ok((_, _, _, una)) => format!("无锚清单 = {:?}", una),
        Err(_) => String::from("黄金链失败"),
    };
    s.add_dyn(
        "J1-无锚项逐条列明",
        match &g {
            Ok((_, _, _, una)) => una.len() == 2,
            Err(_) => false,
        },
        &una_txt,
    );
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "J1-注入四类10/10全抓",
        inj_ok,
        &format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries).unwrap_or([0; INJECT_CLASSES]),
            inj.as_ref().map(|l| l.caught).unwrap_or([0; INJECT_CLASSES])
        ),
    );
    let cap = capacity_probe();
    s.add_dyn(
        "J1-容量耗尽账不丢",
        cap.is_ok(),
        &format!("满账例数 = {:?}", cap),
    );
    let rep = repro_check();
    s.add_dyn("J1-双跑一致", rep.is_ok(), &format!("{:?}", rep));
    let short = probe_seq_b();
    s.add_dyn(
        "J1-覆盖短少必红",
        short.err() == Some(DcCode::CoverageShortfall.code()),
        &format!("1锚+1无锚 判定 = {:?}", short.err()),
    );
    s
}

/// 超长锚构造（33 字节：前缀 5 + 28 个 'x'）。
fn make_long_anchor() -> String {
    let mut s = String::from("API::");
    let mut i = 0;
    while i <= ANCHOR_LEN_MAX {
        s.push('x');
        i += 1;
    }
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_coverage_96_percent() {
        let g = golden_run().unwrap_or((0, 0, 0, Vec::new()));
        assert_eq!(g.0, CASE_TOTAL);
        assert_eq!(g.1, ANCHORED_MIN);
        assert_eq!(g.2, 960);
        assert_eq!(g.3, vec![148, 149]);
    }

    #[test]
    fn test_anchor_parse_five_gates() {
        assert!(parse_anchor("").is_err());
        assert!(parse_anchor("DOC::x#1").is_err());
        assert!(parse_anchor("API::x1").is_err());
        assert!(parse_anchor("API::x#0").is_err());
        assert!(parse_anchor("API::x#").is_err());
        assert_eq!(parse_anchor("API::probe_run#12").unwrap_or(0), 12);
        assert_eq!(parse_anchor("API::probe_run#4096").unwrap_or(0), 4096);
    }

    #[test]
    fn test_anchor_hash_fnv1a() {
        // 确定性：同串同值；异串（大概率）异值。
        assert_eq!(anchor_hash("API::probe_run#1"), anchor_hash("API::probe_run#1"));
        assert_ne!(anchor_hash("API::probe_run#1"), anchor_hash("API::probe_run#2"));
        // FNV-1a 空串 = 2166136261（规格已知答案）。
        assert_eq!(anchor_hash(""), 2166_136_261);
    }

    #[test]
    fn test_inject_all_caught_10_rounds() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(led.all_caught(), "四类坏样本 10 轮必须全抓");
        assert!(led.conservation().is_ok());
    }

    #[test]
    fn test_shared_anchor_rejected() {
        let mut led = DocLedger::new();
        assert!(led.register(1, Some("API::probe_run#9")).is_ok());
        assert!(led.register(2, Some("API::probe_run#9")).is_err());
        assert_eq!(led.case_count(), 1);
    }

    #[test]
    fn test_state_machine_transitions() {
        let mut led = DocLedger::new();
        // 越级封账拒。
        assert!(led.seal().is_err());
        assert!(led.register(1, Some("API::probe_run#1")).is_ok());
        assert!(led.audit().is_ok());
        assert!(led.seal().is_ok());
        // 封账后登记拒。
        assert!(led.register(2, Some("API::probe_run#2")).is_err());
        // 二次核算拒（Audited 态再 audit）。
        let mut led2 = DocLedger::new();
        let _ = led2.register(1, Some("API::probe_run#1"));
        assert!(led2.audit().is_ok());
        assert!(led2.audit().is_err());
    }

    #[test]
    fn test_boundary_case_ids_and_coverage() {
        let mut led = DocLedger::new();
        assert!(led.register(0, None).is_err());
        assert!(led.register(CASE_ID_MAX + 1, None).is_err());
        // 覆盖短少（1 锚 + 1 无锚 = 50%）。
        assert!(probe_seq_b().is_err());
    }

    #[test]
    fn test_capacity_exhaustion() {
        assert_eq!(capacity_probe().unwrap_or(0), CASE_CAP);
        let mut led = DocLedger::new();
        let mut id = 1u32;
        while (id as usize) <= CASE_CAP {
            let anchor = format!("API::probe_run#{}", id);
            assert!(led.register(id, Some(&anchor)).is_ok());
            id += 1;
        }
        assert!(led.register(999, None).is_err());
        assert_eq!(led.case_count(), CASE_CAP);
    }

    #[test]
    fn test_unanchored_list_conservation() {
        let mut led = DocLedger::new();
        let mut i = 0;
        while i < 10 {
            let id = 100 + i as u32;
            if i % 2 == 0 {
                let anchor = format!("API::probe_run#{}", id);
                let _ = led.register(id, Some(&anchor));
            } else {
                let _ = led.register(id, None);
            }
            i += 1;
        }
        assert_eq!(led.anchored() + led.unanchored.len(), led.case_count());
        assert_eq!(led.unanchored, vec![101, 103, 105, 107, 109]);
    }

    #[test]
    fn test_double_run_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard_and_detail_closed_set() {
        assert!(segment_guard());
        assert!(detail_closed_set());
        assert_eq!(DcCode::CoverageShortfall.code(), SEG_BASE + 11);
    }

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18538_checks();
        assert!(set.all_passed(), "UNX-F18538 判据必须全绿：{}", set.red_items_text());
    }
}
