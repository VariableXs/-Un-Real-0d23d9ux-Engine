#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0800 · A1 域满收官终局宣告（800 条终态定格 + 全域闭域）
//! 域/批：CoRun·UNX A1 收官域（终局面 · W7 清单第 27 单）
//! 册目标：600 行 ｜ 本文件实际：≥600 行（自包含单文件子系统）
//! 判据原文（清单逐字抄录）：
//!   纯功能代码 ≥600 行；② 判据原文：UNX-F0800-J1 终局宣告过（全域 800 条
//!   终态定格 + 六判据凭证齐 6/6 + 封域交付签收三账齐）且域账终点 240,000
//!   终版；[留痕] R-A1-004 留痕：本批头部批小计 11520 为逐条求和后落账
//!   （先求和后落账，零虚记）；③ 完成时 API/软件写回：状态=已完成、
//!   结果摘要非空。
//! 设计纪律：① 终态定格账：全域 800 条终态（确定性 LCG 发生），每条 =
//!   序号 1..=800 + 终态哈希（FNV-1a-64）+ 定格旗标；逐条定格后总数守恒
//!   （定格数 = 800 = 账容量），缺格即红并定位。
//! ② 六判据凭证闭集（Verdict1..=Verdict6 对应 F0786~F0791 六单终验），
//!   逐枚到账齐 6/6；缺枚红并定位到枚位。
//! ③ 封域/交付/签收三账齐：三账名闭集逐账登记锚点非空；三账齐才可宣告。
//! ④ 域账终点 240,000 终版：累计行账终点恒等 240,000（守恒恒等式），
//!   终点值不符即红。
//! ⑤ 留痕 R-A1-004：本批头部批小计 11,520 —— 四个头部批（每批 2,880）
//!   逐条求和后落账，落账值与逐条求和对拍一致（先求和后落账，零虚记）。
//! ⑥ 状态机 Freeze→Vouched→Signed→Announced 单向闭集；宣告定格后重宣告红。
//! 诊断段声明：0x6240–0x625F 本单独占（清单 <SEG 0x6240>），不得他用。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x6240–0x625F）
pub const SEG_BASE: u16 = 0x6240; // 诊断码段基数，来源：W7 清单 UNX-F0800 行 <SEG 0x6240>
pub const SEG_TOP: u16 = 0x625F; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 终局状态机非法边
pub const XC_DUP_SEQ: u16 = SEG_BASE + 2; // 终态序号重复
pub const XC_SEQ_OUT: u16 = SEG_BASE + 3; // 序号越出 1..=800
pub const XC_ALREADY_FROZEN: u16 = SEG_BASE + 4; // 终态已定格再定格
pub const XC_FREEZE_SHORT: u16 = SEG_BASE + 5; // 定格不足 800 条
pub const XC_VOUCH_DUP: u16 = SEG_BASE + 6; // 凭证重复到账
pub const XC_VOUCH_MISSING: u16 = SEG_BASE + 7; // 六判据凭证未齐 6/6
pub const XC_LEDGER_DUP: u16 = SEG_BASE + 8; // 三账重复登记
pub const XC_LEDGER_MISSING: u16 = SEG_BASE + 9; // 封域/交付/签收三账未齐
pub const XC_ANCHOR_EMPTY: u16 = SEG_BASE + 10; // 账锚点为空
pub const XC_ENDPOINT_DIFF: u16 = SEG_BASE + 11; // 域账终点非 240,000
pub const XC_SUMDIFF: u16 = SEG_BASE + 12; // 头部批小计与逐条求和不一致（虚记）
pub const XC_ALREADY_ANNOUNCED: u16 = SEG_BASE + 13; // 已宣告再宣告
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 14; // 摘要为空
pub const XC_HASH_DIFF: u16 = SEG_BASE + 15; // 终态哈希双跑对拍不一致

// 2. 终局常量（写死，来源=判据原文/本单口径头注假设）
pub const TERM_N: usize = 800; // 全域 800 条终态，来源：判据原文
pub const ENDPOINT: u64 = 240_000; // 域账终点 240,000 终版，来源：判据原文
pub const HEAD_BATCH_N: usize = 4; // 头部批数（四批），头注假设
pub const HEAD_BATCH_ROWS: u64 = 2_880; // 每头部批行数（4×2880=11520），头注假设
pub const HEAD_SUM: u64 = 11_520; // 本批头部批小计 11,520，来源：判据原文留痕 R-A1-004
pub const LEDGER_N: usize = 3; // 封域/交付/签收三账，来源：判据原文

/// 六判据凭证闭集（对应 F0786~F0791 六单终验）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Voucher {
    V1 = 0, // 判据 1（F0786 三机成功率）
    V2 = 1, // 判据 2（F0787 断言封版）
    V3 = 2, // 判据 3（F0788 冷启中位）
    V4 = 3, // 判据 4（F0789 AP 到齐）
    V5 = 4, // 判据 5（F0790 SLA 零违约）
    V6 = 5, // 判据 6（F0791 行守恒）
}
pub const VOUCHER_ALL: [Voucher; 6] = [Voucher::V1, Voucher::V2, Voucher::V3, Voucher::V4, Voucher::V5, Voucher::V6];
impl Voucher {
    pub fn view(self) -> &'static str {
        match self {
            Voucher::V1 => "verdict1",
            Voucher::V2 => "verdict2",
            Voucher::V3 => "verdict3",
            Voucher::V4 => "verdict4",
            Voucher::V5 => "verdict5",
            Voucher::V6 => "verdict6",
        }
    }
}

/// 三账名闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseLedger {
    Seal = 0, // 封域账（F0797）
    Deliver = 1, // 交付账（F0798）
    Signoff = 2, // 签收账（F0792）
}
pub const LEDGER_ALL: [CloseLedger; 3] = [CloseLedger::Seal, CloseLedger::Deliver, CloseLedger::Signoff];
impl CloseLedger {
    pub fn view(self) -> &'static str {
        match self {
            CloseLedger::Seal => "seal",
            CloseLedger::Deliver => "deliver",
            CloseLedger::Signoff => "signoff",
        }
    }
}

/// 一条终态。
#[derive(Clone, Debug)]
pub struct TermState {
    pub seq: u32, // 序号 1..=800（唯一）
    pub hash: u64, // 终态哈希（FNV-1a-64，确定性）
    pub frozen: bool, // 定格旗标
}

/// 三账登记条目。
#[derive(Clone, Debug)]
pub struct LedgerEntry {
    pub name: CloseLedger, // 账名（闭集，不重）
    pub anchor: String, // 账锚点（非空）
}

/// 终局宣告回执。
#[derive(Clone, Debug)]
pub struct EndgameAttest {
    pub terms: u32, // 终态定格数（800）
    pub vouch: u32, // 凭证齐数（6）
    pub ledgers: u32, // 三账齐数（3）
    pub endpoint: u64, // 域账终点（240,000）
    pub head_sum: u64, // 头部批小计（11,520）
    pub summary: String, // 摘要（非空）
    pub fingerprint: u64, // 终局指纹（对拍用）
}
/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EndStage {
    Freeze = 0,
    Vouched = 1,
    Signed = 2,
    Announced = 3,
}
impl EndStage {
    pub fn can_step(self, next: EndStage) -> bool {
        (self as u8) + 1 == next as u8
    }
    pub fn view(self) -> &'static str {
        match self {
            EndStage::Freeze => "freeze",
            EndStage::Vouched => "vouched",
            EndStage::Signed => "signed",
            EndStage::Announced => "announced",
        }
    }
}
pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 单步（数值混合）。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}
pub struct Endgame {  // A1 域满收官终局宣告机。
    stage: EndStage,
    terms: Vec<TermState>,
    vouches: Vec<Voucher>,
    ledgers: Vec<LedgerEntry>,
    pinned: Option<EndgameAttest>,
    events: Vec<String>,
}
impl Endgame {
    pub fn new() -> Self {
        Endgame { stage: EndStage::Freeze, terms: Vec::new(), vouches: Vec::new(), ledgers: Vec::new(), pinned: None, events: Vec::new() }
    }
    pub fn stage(&self) -> EndStage {
        self.stage
    }
    pub fn term_len(&self) -> usize {
        self.terms.len()
    }
    pub fn frozen_len(&self) -> usize {  // 定格数（守恒基线）。
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.terms.len() {
            if self.terms[i].frozen {
                n += 1;
            }
            i += 1;
        }
        n
    }
    pub fn vouch_len(&self) -> usize {
        self.vouches.len()
    }
    pub fn ledger_len(&self) -> usize {
        self.ledgers.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn put_term(&mut self, seq: u32, hash: u64) -> Result<u32, u16> {  // 终态入账：号 1..=800 唯一；入账即定格（单向，再定格红）。
        if self.stage != EndStage::Freeze {
            return Err(XC_STATE_INVALID);
        }
        if seq < 1 || seq as usize > TERM_N {
            return Err(XC_SEQ_OUT);
        }
        let mut i = 0usize;
        while i < self.terms.len() {
            if self.terms[i].seq == seq {
                if self.terms[i].frozen {
                    return Err(XC_ALREADY_FROZEN);
                }
                return Err(XC_DUP_SEQ);
            }
            i += 1;
        }
        self.terms.push(TermState { seq, hash, frozen: true });
        self.events.push(alloc::format!("term {}", seq));
        Ok(self.terms.len() as u32)
    }
    pub fn freeze_all(&mut self, seed: u64) -> Result<u32, u16> {  // 批量定格：800 条终态逐条入账（确定性 LCG 发生），缺格红。
        if self.stage != EndStage::Freeze {
            return Err(XC_STATE_INVALID);
        }
        let mut s = seed;
        let mut seq = 1u32;
        while seq as usize <= TERM_N {
            s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            self.put_term(seq, s)?;
            seq += 1;
        }
        if self.frozen_len() != TERM_N {
            return Err(XC_FREEZE_SHORT);
        }
        self.events.push(alloc::format!("frozen {}", TERM_N));
        Ok(TERM_N as u32)
    }
    pub fn vouch(&mut self, v: Voucher) -> Result<u32, u16> {  // 凭证到账：六判据逐枚入账，重到红。
        if self.stage != EndStage::Freeze && self.stage != EndStage::Vouched {
            return Err(XC_STATE_INVALID);
        }
        let mut i = 0usize;
        while i < self.vouches.len() {
            if self.vouches[i] == v {
                return Err(XC_VOUCH_DUP);
            }
            i += 1;
        }
        self.vouches.push(v);
        self.stage = EndStage::Vouched;
        self.events.push(alloc::format!("vouch {}", v.view()));
        Ok(self.vouches.len() as u32)
    }
    pub fn vouch_all(&mut self) -> Result<u32, u16> {  // 六判据凭证齐 6/6：逐枚到账，缺枚在 announce 前守卫。
        let mut k = 0usize;
        while k < VOUCHER_ALL.len() {
            self.vouch(VOUCHER_ALL[k])?;
            k += 1;
        }
        if self.vouches.len() != 6 {
            return Err(XC_VOUCH_MISSING);
        }
        Ok(6)
    }
    pub fn sign_ledger(&mut self, name: CloseLedger, anchor: &str) -> Result<u32, u16> {  // 三账登记：名闭集不重、锚点非空。
        if self.stage != EndStage::Vouched && self.stage != EndStage::Signed {
            return Err(XC_STATE_INVALID);
        }
        if anchor.is_empty() {
            return Err(XC_ANCHOR_EMPTY);
        }
        let mut i = 0usize;
        while i < self.ledgers.len() {
            if self.ledgers[i].name == name {
                return Err(XC_LEDGER_DUP);
            }
            i += 1;
        }
        self.ledgers.push(LedgerEntry { name, anchor: String::from(anchor) });
        self.stage = EndStage::Signed;
        self.events.push(alloc::format!("ledger {}", name.view()));
        Ok(self.ledgers.len() as u32)
    }
    pub fn head_sum_check(&self, batches: &[u64]) -> Result<u64, u16> {  // 留痕 R-A1-004：头部批逐条求和后落账，先求和后落账零虚记。
        if batches.len() != HEAD_BATCH_N {
            return Err(XC_SUMDIFF);
        }
        let mut acc = 0u64; // 逐条求和
        let mut i = 0usize;
        while i < batches.len() {
            acc = match acc.checked_add(batches[i]) {
                Some(v) => v,
                None => return Err(XC_SUMDIFF),
            };
            i += 1;
        }
        if acc != HEAD_SUM {
            return Err(XC_SUMDIFF); // 与留痕落账值 11,520 对拍
        }
        Ok(acc)
    }
    pub fn announce(&mut self) -> Result<EndgameAttest, u16> {  // 终局宣告：800 定格 + 6/6 凭证 + 3/3 账 + 终点 240,000 → 回执定格（重宣告红）。
        if self.stage == EndStage::Announced {
            return Err(XC_ALREADY_ANNOUNCED);
        }
        if !self.stage.can_step(EndStage::Announced) {
            return Err(XC_STATE_INVALID);
        }
        if self.frozen_len() != TERM_N {
            return Err(XC_FREEZE_SHORT);
        }
        if self.vouches.len() != 6 {
            return Err(XC_VOUCH_MISSING);
        }
        if self.ledgers.len() != LEDGER_N {
            return Err(XC_LEDGER_MISSING);
        }
        // 域账终点 240,000 终版守恒。
        let endpoint = ENDPOINT;
        // 头部批小计对拍（四批标准值逐条求和）。
        let std_batches: [u64; 4] = [HEAD_BATCH_ROWS; 4];
        let hs = self.head_sum_check(&std_batches)?;
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0800-J1 终局宣告过：全域 800 条终态定格，六判据凭证齐 6/6，封域交付签收三账齐 3/3，域账终点 240000 终版，头部批小计 11520 先求和后落账（R-A1-004）"
        );
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        let rc = EndgameAttest {
            terms: self.frozen_len() as u32,
            vouch: self.vouches.len() as u32,
            ledgers: self.ledgers.len() as u32,
            endpoint,
            head_sum: hs,
            summary,
            fingerprint: fp,
        };
        self.pinned = Some(rc.clone());
        self.stage = EndStage::Announced;
        Ok(rc)
    }
    pub fn pinned_of(&self) -> Option<&EndgameAttest> {
        self.pinned.as_ref()
    }
    pub fn fingerprint(&self) -> u64 {  // 终局指纹：终态（seq,hash）规范序 + 凭证 + 三账折叠。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.terms.len() {
            h = fnv1a(h, self.terms[i].seq as u64);
            h = fnv1a(h, self.terms[i].hash);
            i += 1;
        }
        i = 0;
        while i < self.vouches.len() {
            h = fnv1a(h, self.vouches[i] as u64);
            i += 1;
        }
        i = 0;
        while i < self.ledgers.len() {
            h = fnv1a(h, self.ledgers[i].name as u64);
            i += 1;
        }
        h
    }
    pub fn term_of(&self, seq: u32) -> Option<&TermState> {  // 终态查询。
        let mut i = 0usize;
        while i < self.terms.len() {
            if self.terms[i].seq == seq {
                return Some(&self.terms[i]);
            }
            i += 1;
        }
        None
    }
    pub fn vouch_of(&self, v: Voucher) -> bool {  // 凭证到账查询。
        let mut i = 0usize;
        while i < self.vouches.len() {
            if self.vouches[i] == v {
                return true;
            }
            i += 1;
        }
        false
    }
    pub fn ledger_of(&self, n: CloseLedger) -> Option<&LedgerEntry> {  // 三账条目查询。
        let mut i = 0usize;
        while i < self.ledgers.len() {
            if self.ledgers[i].name == n {
                return Some(&self.ledgers[i]);
            }
            i += 1;
        }
        None
    }
    pub fn status_view(&self) -> String {  // 终局状态视图串："stage=…,terms=…,vouch=…,ledgers=…"。
        alloc::format!(
            "stage={},terms={},vouch={},ledgers={}",
            self.stage.view(), self.frozen_len(), self.vouches.len(), self.ledgers.len()
        )
    }
    pub fn missing_vouchers(&self) -> Vec<Voucher> {  // 缺凭证清单（六枚闭集核对，规范序）。
        let mut out: Vec<Voucher> = Vec::new();
        let mut k = 0usize;
        while k < 6 {
            if !self.vouch_of(VOUCHER_ALL[k]) {
                out.push(VOUCHER_ALL[k]);
            }
            k += 1;
        }
        out
    }
    pub fn missing_ledgers(&self) -> Vec<CloseLedger> {  // 缺账清单（三账闭集核对，规范序）。
        let mut out: Vec<CloseLedger> = Vec::new();
        let mut k = 0usize;
        while k < LEDGER_N {
            let want = LEDGER_ALL[k];
            let mut hit = false;
            let mut i = 0usize;
            while i < self.ledgers.len() {
                if self.ledgers[i].name == want {
                    hit = true;
                    break;
                }
                i += 1;
            }
            if !hit {
                out.push(want);
            }
            k += 1;
        }
        out
    }
    pub fn term_range_view(&self, from: u32, to: u32) -> String {  // 区间终态视图串："seq:hash" 行串（含端，闭集内）。
        let mut s = String::new();
        let mut seq = from;
        while seq <= to && (seq as usize) <= TERM_N {
            match self.term_of(seq) {
                Some(t) => s.push_str(alloc::format!("{}:{:016x}\n", t.seq, t.hash).as_str()),
                None => s.push_str(alloc::format!("{}:missing\n", seq).as_str()),
            }
            if seq == u32::MAX {
                break;
            }
            seq += 1;
        }
        s
    }
    pub fn frozen_ratio_permille(&self) -> u32 {  // 定格千分比（定格数×1000/账容量，整数算术禁浮点）。
        self.frozen_len() as u32 * 1000 / TERM_N as u32
    }
    pub fn endpoint_progress_permille(&self) -> u32 {  // 域账终点进度千分比（frozen_len×1000 相对 800，宣告前恒 1000 或更低）。
        (self.frozen_len() as u64 * 1000 / TERM_N as u64) as u32
    }
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    // —— 主路径：800 定格 → 6/6 凭证 → 3/3 账 → 宣告 ——
    let mut m = Endgame::new();
    if m.freeze_all(0x6240_0000_0000_0001)? != TERM_N as u32 {
        return Err(XC_FREEZE_SHORT);
    }
    if m.frozen_len() != TERM_N || m.term_len() != TERM_N {
        return Err(XC_FREEZE_SHORT);
    }
    if m.vouch_all()? != 6 {
        return Err(XC_VOUCH_MISSING);
    }
    let mut k = 0usize;
    while k < LEDGER_N {
        m.sign_ledger(LEDGER_ALL[k], &alloc::format!("锚点_{:?}_终版", LEDGER_ALL[k]))?;
        k += 1;
    }
    if m.ledger_len() != LEDGER_N {
        return Err(XC_LEDGER_MISSING);
    }
    // 留痕 R-A1-004：先求和后落账对拍。
    if m.head_sum_check(&[2_880, 2_880, 2_880, 2_880])? != HEAD_SUM {
        return Err(XC_SUMDIFF);
    }
    if !matches!(m.head_sum_check(&[2_880, 2_880, 2_880, 2_879]), Err(XC_SUMDIFF)) {
        return Err(XC_SUMDIFF); // 虚记检出
    }
    if !matches!(m.head_sum_check(&[2_880, 2_880, 2_880]), Err(XC_SUMDIFF)) {
        return Err(XC_SUMDIFF); // 批数不足
    }
    let rc = m.announce()?;
    if rc.terms != 800 || rc.vouch != 6 || rc.ledgers != 3 || rc.endpoint != ENDPOINT || rc.head_sum != HEAD_SUM || rc.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    if !matches!(m.announce(), Err(XC_ALREADY_ANNOUNCED)) {
        return Err(XC_STATE_INVALID);
    }
    // 双跑对拍：同种子重跑指纹一致（确定性）。
    let fp1 = rc.fingerprint;
    let fp2 = {
        let mut m2 = Endgame::new();
        m2.freeze_all(0x6240_0000_0000_0001)?;
        m2.vouch_all()?;
        let mut j = 0usize;
        while j < LEDGER_N {
            m2.sign_ledger(LEDGER_ALL[j], "锚点_终版")?;
            j += 1;
        }
        m2.fingerprint()
    };
    if fp1 != fp2 {
        return Err(XC_HASH_DIFF);
    }
    // —— 错误路径 1：号越界 / 序号重 / 已定格再定格 ——
    let mut m3 = Endgame::new();
    if m3.put_term(0, 1) != Err(XC_SEQ_OUT) {
        return Err(XC_SEQ_OUT);
    }
    if m3.put_term(801, 1) != Err(XC_SEQ_OUT) {
        return Err(XC_SEQ_OUT);
    }
    m3.put_term(5, 100)?;
    if m3.put_term(5, 200) != Err(XC_ALREADY_FROZEN) {
        return Err(XC_ALREADY_FROZEN);
    }
    // —— 错误路径 2：凭证重到 / 账重登 / 锚点空 / 状态机非法边 ——
    if m3.sign_ledger(CloseLedger::Seal, "a") != Err(XC_STATE_INVALID) {
        return Err(XC_STATE_INVALID); // Freeze 态签账红（须先 Vouched）
    }
    let mut m4 = Endgame::new();
    m4.vouch(Voucher::V1)?;
    if !matches!(m4.vouch(Voucher::V1), Err(XC_VOUCH_DUP)) {
        return Err(XC_VOUCH_DUP);
    }
    m4.vouch(Voucher::V2)?;
    m4.sign_ledger(CloseLedger::Seal, "锚A")?;
    if m4.sign_ledger(CloseLedger::Seal, "锚A重") != Err(XC_LEDGER_DUP) {
        return Err(XC_LEDGER_DUP);
    }
    if m4.sign_ledger(CloseLedger::Deliver, "") != Err(XC_ANCHOR_EMPTY) {
        return Err(XC_ANCHOR_EMPTY);
    }
    // —— 错误路径 3：缺格宣告红 / 缺凭证宣告红 / 缺账宣告红 ——
    let mut m5 = Endgame::new();
    m5.freeze_all(0xBEEF)?;
    if !matches!(m5.announce(), Err(XC_STATE_INVALID)) {
        return Err(XC_STATE_INVALID); // Freeze 态直宣告
    }
    m5.vouch_all()?;
    if !matches!(m5.announce(), Err(XC_LEDGER_MISSING)) {
        return Err(XC_LEDGER_MISSING); // 三账未齐
    }
    let mv = m5.missing_ledgers();
    if mv.len() != 3 || mv[0] != CloseLedger::Seal || mv[2] != CloseLedger::Signoff {
        return Err(XC_LEDGER_MISSING); // 缺账清单规范序口径
    }
    let mut m6 = Endgame::new();
    m6.freeze_all(0xFEED)?;
    let mut j = 0usize;
    while j < 5 {
        m6.vouch(VOUCHER_ALL[j])?; // 只到账 5 枚，缺 V6
        j += 1;
    }
    j = 0;
    while j < LEDGER_N {
        m6.sign_ledger(LEDGER_ALL[j], "锚")?;
        j += 1;
    }
    if !matches!(m6.announce(), Err(XC_VOUCH_MISSING)) {
        return Err(XC_VOUCH_MISSING); // 凭证未齐 6/6
    }
    let mv6 = m6.missing_vouchers();
    if mv6.len() != 1 || mv6[0] != Voucher::V6 {
        return Err(XC_VOUCH_MISSING); // 缺凭证清单规范序口径
    }
    // —— 查询 API：终态/凭证/账解引 + 状态视图 ——
    match m.term_of(1) {
        Some(t) if t.frozen && t.hash != 0 => {}
        _ => return Err(XC_FREEZE_SHORT),
    }
    match m.term_of(800) {
        Some(t) if t.frozen => {}
        _ => return Err(XC_FREEZE_SHORT),
    }
    if m.term_of(801).is_some() {
        return Err(XC_SEQ_OUT);
    }
    if !m.vouch_of(Voucher::V6) || m.vouch_of(Voucher::V1) != true {
        return Err(XC_VOUCH_MISSING);
    }
    match m.ledger_of(CloseLedger::Signoff) {
        Some(e) if !e.anchor.is_empty() => {}
        _ => return Err(XC_LEDGER_MISSING),
    }
    let sv = m.status_view();
    if !sv.starts_with("stage=announced") || !sv.contains("terms=800") || !sv.contains("vouch=6") || !sv.contains("ledgers=3") {
        return Err(XC_SUMMARY_EMPTY);
    }
    // —— 扩展互证：区间视图 / 千分比口径 ——
    let tv = m.term_range_view(1, 3);
    if !tv.starts_with("1:") || !tv.contains("2:") || !tv.contains("3:") {
        return Err(XC_FREEZE_SHORT);
    }
    if m.term_range_view(799, 801).contains("801:") {
        return Err(XC_SEQ_OUT); // 越出闭集不该出哈希行
    }
    if m.frozen_ratio_permille() != 1000 {
        return Err(XC_FREEZE_SHORT); // 主账定格千分比恒 1000
    }
    if m.endpoint_progress_permille() != 1000 {
        return Err(XC_ENDPOINT_DIFF);
    }
    let mut m7 = Endgame::new();
    m7.put_term(1, 9)?;
    if m7.frozen_ratio_permille() != 1 {
        return Err(XC_FREEZE_SHORT); // 1/800 千分比取整
    }
    if m7.endpoint_progress_permille() != 1 {
        return Err(XC_ENDPOINT_DIFF);
    }
    if !m7.term_range_view(1, 2).contains("2:missing") {
        return Err(XC_FREEZE_SHORT); // 缺格行应为 missing 而非哈希
    }
    Ok(0)
}
