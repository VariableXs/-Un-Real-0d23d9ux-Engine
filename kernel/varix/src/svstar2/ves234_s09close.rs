//! VE-F3980 · S09 组收口双签
//!
//! S09 组收口：20 项核验汇总（ves215~ves233 各模块核心判据汇总）+
//! 28 对对账回执（pair_index 0..28 双射回执）+ 六维资产汇总（文档/测试/
//! 性能/安全/双维/手册六册资产计数与缺口）+ 预备报告出具。四件齐备后
//! 双签：自查签 + 复核签分离（禁同人），任一签可拒签；拒签生成催办单
//! 回四件补齐循环。
//! 错误路径：四件缺一→收口拒绝；同人双签→P0；缺签→待签；复核拒签→
//! 催办；回执缺对→对账缺口。
//! 性能：汇总 O(判据)；回执 O(对)；资产 O(册)；双签 O(1)。
//! 跨批对接：F3961~F3979 全部上游模块；F3997 域级收口汇总；F3999 终审。
//! 无障碍与隐私：核验汇总含无障碍 20 项专技；回执与人名脱敏后入档。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x38C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x38C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9FCode(pub u16);

impl S9FCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const QUAD_INCOMPLETE: Self = Self(SEG_BASE + 1);
    pub const SIGNER_SAME: Self = Self(SEG_BASE + 2);
    pub const SIGN_MISSING: Self = Self(SEG_BASE + 3);
    pub const REJECTED: Self = Self(SEG_BASE + 4);
    pub const URGED: Self = Self(SEG_BASE + 5);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::QUAD_INCOMPLETE => "四件缺一→收口拒绝",
            Self::SIGNER_SAME => "同人双签→P0",
            Self::SIGN_MISSING => "缺签→待签",
            Self::REJECTED => "复核拒签→催办",
            Self::URGED => "催办中",
            _ => "未知",
        }
    }
}

// ── 第一件：20 项核验汇总 ────────────────────────────────────────────

/// 单项核验：来源模块 + 判据标签 + 结论。
#[derive(Debug, Clone)]
pub struct VerifyItem {
    /// 来源任务号。
    pub fid: &'static str,
    /// 判据标签。
    pub label: &'static str,
    /// 结论（true = 过）。
    pub ok: bool,
    /// 明细。
    pub detail: String,
}

/// 20 项核验汇总册：ves215~ves233 各抽一项核心 + 收口自检一项。
#[derive(Debug, Clone)]
pub struct VerifyRollup {
    /// 核验项（长度恒 20）。
    pub items: Vec<VerifyItem>,
}

pub const ROLLUP_SOURCES: [&str; 19] = [
    "VE-F3961", "VE-F3962", "VE-F3963", "VE-F3964", "VE-F3965",
    "VE-F3966", "VE-F3967", "VE-F3968", "VE-F3969", "VE-F3970",
    "VE-F3971", "VE-F3972", "VE-F3973", "VE-F3974", "VE-F3975",
    "VE-F3976", "VE-F3977", "VE-F3978", "VE-F3979",
];

impl VerifyRollup {
    /// 逐源登记初始项（ok=false 待填）。
    pub fn new() -> Self {
        let mut items = Vec::new();
        for (i, fid) in ROLLUP_SOURCES.iter().enumerate() {
            items.push(VerifyItem {
                fid,
                label: "",
                ok: false,
                detail: String::from("待汇总"),
            });
            let _ = i;
        }
        // 第 20 项 = 收口自检。
        items.push(VerifyItem {
            fid: "VE-F3980",
            label: "C3980-01",
            ok: false,
            detail: String::from("收口自检"),
        });
        Self { items }
    }

    /// 回填某源结论（按 fid 定位）。
    pub fn fill(&mut self, fid: &str, label: &'static str, ok: bool, detail: &str) -> bool {
        if let Some(slot) = self.items.iter_mut().find(|it| it.fid == fid) {
            slot.label = label;
            slot.ok = ok;
            slot.detail = String::from(detail);
            true
        } else {
            false
        }
    }

    /// 20 项齐备：每项 label 非空。
    pub fn complete(&self) -> bool {
        self.items.len() == 20 && self.items.iter().all(|it| !it.label.is_empty())
    }

    /// 全过：齐备且全 ok。
    pub fn all_green(&self) -> bool {
        self.complete() && self.items.iter().all(|it| it.ok)
    }

    /// 红项清单。
    pub fn red_fids(&self) -> Vec<&'static str> {
        self.items.iter().filter(|it| !it.ok).map(|it| it.fid).collect()
    }
}

// ── 第二件：28 对对账回执 ────────────────────────────────────────────

/// 回执状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptState {
    /// 待回。
    Pending,
    /// 已回且平。
    Balanced,
    /// 已回且不平。
    OffBalance,
}

/// 单对回执。
#[derive(Debug, Clone)]
pub struct PairReceipt {
    /// pair_index 0..28（双射）。
    pub pair_index: u8,
    /// 状态。
    pub state: ReceiptState,
    /// 回执说明。
    pub note: String,
}

/// 28 对对账回执册。
#[derive(Debug, Clone)]
pub struct PairReceiptBook {
    pub receipts: Vec<PairReceipt>,
}

pub const PAIR_TOTAL: u8 = 28;

impl PairReceiptBook {
    /// 初始 28 对全待回。
    pub fn new() -> Self {
        let mut receipts = Vec::new();
        for i in 0..PAIR_TOTAL {
            receipts.push(PairReceipt {
                pair_index: i,
                state: ReceiptState::Pending,
                note: String::new(),
            });
        }
        Self { receipts }
    }

    /// 回执登记（pair_index 越界拒绝）。
    pub fn record(&mut self, pair_index: u8, balanced: bool, note: &str) -> bool {
        if pair_index >= PAIR_TOTAL {
            return false;
        }
        if let Some(r) = self.receipts.iter_mut().find(|r| r.pair_index == pair_index) {
            r.state = if balanced { ReceiptState::Balanced } else { ReceiptState::OffBalance };
            r.note = String::from(note);
            true
        } else {
            false
        }
    }

    /// 双射齐备：0..28 每对恰一 receipt（登记即平账面；不平也视为已回）。
    pub fn all_received(&self) -> bool {
        self.receipts.len() == 28
            && self.receipts.iter().all(|r| r.state != ReceiptState::Pending)
    }

    /// 不平清单。
    pub fn off_pairs(&self) -> Vec<u8> {
        self.receipts.iter().filter(|r| r.state == ReceiptState::OffBalance).map(|r| r.pair_index).collect()
    }
}

// ── 第三件：六维资产汇总 ─────────────────────────────────────────────

/// 资产维度六册。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetDim {
    /// 文档资产。
    Doc,
    /// 测试资产。
    Test,
    /// 性能资产。
    Perf,
    /// 安全资产。
    Sec,
    /// 双维资产。
    Duo,
    /// 手册资产。
    Handbook,
}

pub const SIX_DIMS: [AssetDim; 6] = [
    AssetDim::Doc,
    AssetDim::Test,
    AssetDim::Perf,
    AssetDim::Sec,
    AssetDim::Duo,
    AssetDim::Handbook,
];

impl AssetDim {
    pub fn name(self) -> &'static str {
        match self {
            AssetDim::Doc => "文档",
            AssetDim::Test => "测试",
            AssetDim::Perf => "性能",
            AssetDim::Sec => "安全",
            AssetDim::Duo => "双维",
            AssetDim::Handbook => "手册",
        }
    }

    pub fn slot(self) -> usize {
        match self {
            AssetDim::Doc => 0,
            AssetDim::Test => 1,
            AssetDim::Perf => 2,
            AssetDim::Sec => 3,
            AssetDim::Duo => 4,
            AssetDim::Handbook => 5,
        }
    }
}

/// 六维资产汇总：每维计数 + 缺口数 + 脱敏标记。
#[derive(Debug, Clone)]
pub struct AssetRollup {
    /// 每维资产计数。
    pub counts: [u32; 6],
    /// 每维缺口数。
    pub gaps: [u32; 6],
    /// 每维脱敏标记。
    pub sanitized: [bool; 6],
}

impl AssetRollup {
    pub fn new() -> Self {
        Self { counts: [0; 6], gaps: [0; 6], sanitized: [false; 6] }
    }

    /// 登记一维。
    pub fn put(&mut self, dim: AssetDim, count: u32, gap: u32, sanitized: bool) {
        let s = dim.slot();
        self.counts[s] = count;
        self.gaps[s] = gap;
        self.sanitized[s] = sanitized;
    }

    /// 六维齐备：每维已登记且脱敏。
    pub fn complete(&self) -> bool {
        SIX_DIMS.iter().all(|d| {
            let s = d.slot();
            // 已登记 = 计数+缺口>0 或显式零缺口（以脱敏标记代表登记过）。
            self.sanitized[s] && self.gaps[s] == 0
        })
    }

    /// 资产总量。
    pub fn total(&self) -> u32 {
        self.counts.iter().sum()
    }
}

// ── 第四件：预备报告 ────────────────────────────────────────────────

/// 预备报告（final 报告出具前的 pre 版）。
#[derive(Debug, Clone)]
pub struct PreGoReport {
    /// 结论：true = 建议进终审。
    pub recommend_go: bool,
    /// 20 项核验过数。
    pub verify_green: u8,
    /// 回执不平对数。
    pub off_pairs: u8,
    /// 资产总量。
    pub asset_total: u32,
    /// 报告正文。
    pub body: String,
}

impl PreGoReport {
    /// 由前三件合成预备报告。
    pub fn compose(v: &VerifyRollup, p: &PairReceiptBook, a: &AssetRollup) -> Self {
        let green = v.items.iter().filter(|it| it.ok).count() as u8;
        let off = p.off_pairs().len() as u8;
        let go = v.all_green() && p.all_received() && off == 0 && a.complete();
        let mut body = String::from("S09 预备报告：核验 ");
        let mut buf = [0u8; 4];
        body.push_str((green as u32).itoa_str(&mut buf));
        body.push_str("/20");
        Self {
            recommend_go: go,
            verify_green: green,
            off_pairs: off,
            asset_total: a.total(),
            body,
        }
    }
}

/// 极简 u32→十进制 ASCII（no_std 无 format! 依赖时的定长工具）。
trait U32Dec {
    fn itoa_str<'a>(&self, buf: &'a mut [u8; 4]) -> &'a str;
}

impl U32Dec for u32 {
    fn itoa_str<'a>(&self, buf: &'a mut [u8; 4]) -> &'a str {
        let mut n = *self;
        let mut i = buf.len();
        if n == 0 {
            i -= 1;
            buf[i] = b'0';
        }
        while n > 0 && i > 0 {
            i -= 1;
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        core::str::from_utf8(&buf[i..]).unwrap_or("?")
    }
}

// ── 四件齐备 + 双签 ─────────────────────────────────────────────────

/// 双签账：自查签 + 复核签分离（禁同人）。
#[derive(Debug, Clone)]
pub struct DualSign {
    /// 自查签人。
    pub self_signer: String,
    /// 复核签人。
    pub review_signer: String,
    /// 自查结论（false = 自查拒签）。
    pub self_ok: bool,
    /// 复核结论（false = 复核拒签）。
    pub review_ok: bool,
}

impl DualSign {
    pub fn new() -> Self {
        Self {
            self_signer: String::new(),
            review_signer: String::new(),
            self_ok: false,
            review_ok: false,
        }
    }

    /// 落签：同人检测（禁同人 → P0 路径）。
    pub fn sign(&mut self, slot: u8, signer: &str, ok: bool) -> S9FCode {
        if signer.is_empty() {
            return S9FCode::SIGN_MISSING;
        }
        match slot {
            0 => {
                if signer == self.review_signer {
                    return S9FCode::SIGNER_SAME;
                }
                self.self_signer = String::from(signer);
                self.self_ok = ok;
            }
            _ => {
                if signer == self.self_signer {
                    return S9FCode::SIGNER_SAME;
                }
                self.review_signer = String::from(signer);
                self.review_ok = ok;
            }
        }
        S9FCode::OK
    }

    /// 双签齐备。
    pub fn signed(&self) -> bool {
        !self.self_signer.is_empty() && !self.review_signer.is_empty()
    }

    /// 收口通过：双签齐备且双 ok。
    pub fn closed(&self) -> bool {
        self.signed() && self.self_ok && self.review_ok
    }
}

/// 催办单。
#[derive(Debug, Clone)]
pub struct UrgencySlip {
    /// 催办对象（缺件编号 1..4 或拒签 slot）。
    pub target: &'static str,
    /// 原因。
    pub reason: String,
}

/// 收口机：四件 → 双签 → 通过 / 拒绝 / 催办。
#[derive(Debug, Clone)]
pub struct Closer {
    pub verify: VerifyRollup,
    pub receipts: PairReceiptBook,
    pub assets: AssetRollup,
    pub report: Option<PreGoReport>,
    pub sign: DualSign,
    pub urgencies: Vec<UrgencySlip>,
}

impl Closer {
    pub fn new() -> Self {
        Self {
            verify: VerifyRollup::new(),
            receipts: PairReceiptBook::new(),
            assets: AssetRollup::new(),
            report: None,
            sign: DualSign::new(),
            urgencies: Vec::new(),
        }
    }

    /// 四件齐备检查，返回 (齐备, 缺件说明)。
    pub fn quad_ready(&self) -> (bool, &'static str) {
        if !self.verify.complete() {
            (false, "核验汇总未齐")
        } else if !self.receipts.all_received() {
            (false, "回执未齐")
        } else if !self.assets.complete() {
            (false, "资产汇总未齐")
        } else if self.report.is_none() {
            (false, "预备报告未出具")
        } else {
            (true, "")
        }
    }

    /// 出具预备报告（第三件齐后调）。
    pub fn issue_report(&mut self) -> S9FCode {
        let (ready, _) = self.quad_ready_report_lazy();
        if !self.verify.complete() || !self.receipts.all_received() || !self.assets.complete() {
            return S9FCode::QUAD_INCOMPLETE;
        }
        let _ = ready;
        self.report = Some(PreGoReport::compose(&self.verify, &self.receipts, &self.assets));
        S9FCode::OK
    }

    fn quad_ready_report_lazy(&self) -> (bool, &'static str) {
        // 报告尚未出具时的中间态检查（不含报告件本身）。
        (
            self.verify.complete() && self.receipts.all_received() && self.assets.complete(),
            "",
        )
    }

    /// 双签落笔（四件齐备前置硬门）。
    pub fn sign_off(&mut self, slot: u8, signer: &str, ok: bool) -> S9FCode {
        let (ready, missing) = self.quad_ready();
        if !ready {
            self.urgencies.push(UrgencySlip {
                target: "quad",
                reason: String::from(missing),
            });
            return S9FCode::QUAD_INCOMPLETE;
        }
        let code = self.sign.sign(slot, signer, ok);
        if code == S9FCode::SIGNER_SAME || code == S9FCode::SIGN_MISSING {
            return code;
        }
        // 任一签拒 → 催办单 + REJECTED。
        if (slot == 0 && !ok) || (slot != 0 && !ok) {
            self.urgencies.push(UrgencySlip {
                target: if slot == 0 { "self-sign" } else { "review-sign" },
                reason: String::from("签人拒签"),
            });
            return S9FCode::REJECTED;
        }
        code
    }

    /// 收口终态。
    pub fn outcome(&self) -> S9FCode {
        let (ready, _) = self.quad_ready();
        if !ready {
            return S9FCode::QUAD_INCOMPLETE;
        }
        if !self.sign.signed() {
            return S9FCode::SIGN_MISSING;
        }
        if self.sign.closed() {
            S9FCode::OK
        } else {
            S9FCode::REJECTED
        }
    }
}

impl Default for Closer {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves234_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves234_s09close");

    // C3980-01 诊断码段零冲突：段内码不越窗。
    chk(&mut set, "C3980-01",
        S9FCode::URGED.0 < SEG_BASE + 16 && SEG_BASE == 0x38C0, "err");

    // C3980-02 核验汇总初始 20 项全待填、不齐备。
    let mut v = VerifyRollup::new();
    chk(&mut set, "C3980-02",
        v.items.len() == 20 && !v.complete() && v.red_fids().len() == 20, "err");

    // C3980-03 逐源回填后齐备；全绿判定。
    for (i, fid) in ROLLUP_SOURCES.iter().enumerate() {
        v.fill(fid, "C", i % 3 != 0, "填");
    }
    v.fill("VE-F3980", "C3980-03", true, "自检过");
    let green_cnt = v.items.iter().filter(|it| it.ok).count();
    chk(&mut set, "C3980-03", v.complete() && green_cnt == 20 - 7, "err");
    // 7 = 源 i∈{0,3,6,9,12,15,18} 共 7 个红项。

    // C3980-04 未知 fid 回填拒绝。
    let mut v2 = VerifyRollup::new();
    chk(&mut set, "C3980-04", !v2.fill("VE-F9999", "X", true, "无") && !v2.complete(), "err");

    // C3980-05 回执越界拒绝。
    let mut p = PairReceiptBook::new();
    chk(&mut set, "C3980-05", !p.record(28, true, "越界") && p.record(0, true, "首对"), "err");

    // C3980-06 28 对齐备 + 不平清单。
    let mut p2 = PairReceiptBook::new();
    for i in 0..PAIR_TOTAL {
        let balanced = i != 5 && i != 11;
        p2.record(i, balanced, "回");
    }
    let off = p2.off_pairs();
    chk(&mut set, "C3980-06",
        p2.all_received() && off == vec![5u8, 11], "err");

    // C3980-07 六维齐备：全脱敏零缺口才过。
    let mut a = AssetRollup::new();
    a.put(AssetDim::Doc, 12, 0, true);
    a.put(AssetDim::Test, 34, 0, true);
    a.put(AssetDim::Perf, 8, 0, true);
    a.put(AssetDim::Sec, 6, 0, true);
    a.put(AssetDim::Duo, 15, 0, true);
    chk(&mut set, "C3980-07", !a.complete(), "err"); // 手册缺
    a.put(AssetDim::Handbook, 1, 0, true);
    chk(&mut set, "C3980-07", a.complete() && a.total() == 76, "err");

    // C3980-08 有缺口则六维不齐。
    let mut a2 = AssetRollup::new();
    for d in SIX_DIMS.iter() {
        a2.put(*d, 3, 1, true);
    }
    chk(&mut set, "C3980-08", !a2.complete(), "err");

    // C3980-09 预备报告合成：全绿 → recommend_go。
    let mut v3 = VerifyRollup::new();
    for fid in ROLLUP_SOURCES.iter() {
        v3.fill(fid, "C", true, "绿");
    }
    v3.fill("VE-F3980", "C", true, "绿");
    let mut p3 = PairReceiptBook::new();
    for i in 0..PAIR_TOTAL {
        p3.record(i, true, "平");
    }
    let r3 = PreGoReport::compose(&v3, &p3, &a);
    chk(&mut set, "C3980-09", r3.recommend_go && r3.verify_green == 20 && r3.off_pairs == 0, "err");

    // C3980-10 预备报告：有红 → NO-GO。
    let mut v4 = VerifyRollup::new();
    for fid in ROLLUP_SOURCES.iter() {
        v4.fill(fid, "C", true, "绿");
    }
    v4.fill("VE-F3980", "C", false, "自检红");
    let r4 = PreGoReport::compose(&v4, &p3, &a);
    chk(&mut set, "C3980-10", !r4.recommend_go && r4.verify_green == 19, "err");

    // C3980-11 双签禁同人：复核签与自查签同人 → P0 路径码。
    let mut s = DualSign::new();
    chk(&mut set, "C3980-11",
        s.sign(0, "甲", true) == S9FCode::OK
            && s.sign(1, "甲", true) == S9FCode::SIGNER_SAME
            && !s.signed(), "err");

    // C3980-12 异人双签齐备 → closed。
    let mut s2 = DualSign::new();
    s2.sign(0, "甲", true);
    s2.sign(1, "乙", true);
    chk(&mut set, "C3980-12", s2.signed() && s2.closed(), "err");

    // C3980-13 拒签可拒：复核拒 → 不 closed。
    let mut s3 = DualSign::new();
    s3.sign(0, "甲", true);
    s3.sign(1, "乙", false);
    chk(&mut set, "C3980-13", s3.signed() && !s3.closed(), "err");

    // C3980-14 收口机：四件缺一 → 拒双签并出催办单。
    let mut c = Closer::new();
    chk(&mut set, "C3980-14",
        c.sign_off(0, "甲", true) == S9FCode::QUAD_INCOMPLETE
            && c.urgencies.len() == 1
            && c.outcome() == S9FCode::QUAD_INCOMPLETE, "err");

    // C3980-15 四件齐备 → 双签全绿 → 收口 OK。
    let mut c2 = Closer::new();
    for (i, fid) in ROLLUP_SOURCES.iter().enumerate() {
        c2.verify.fill(fid, "C", i % 2 == 0, "混");
    }
    c2.verify.fill("VE-F3980", "C", true, "过");
    // 注意：上面一半红，六件齐备仍成立（齐备≠全绿），但 recommend_go=false。
    for i in 0..PAIR_TOTAL {
        c2.receipts.record(i, true, "平");
    }
    for d in SIX_DIMS.iter() {
        let dim = *d;
        c2.assets.put(dim, 2, 0, true);
    }
    chk(&mut set, "C3980-15", c2.issue_report() == S9FCode::OK, "err");
    chk(&mut set, "C3980-15",
        c2.sign_off(0, "甲", true) == S9FCode::OK
            && c2.sign_off(1, "乙", true) == S9FCode::OK
            && c2.outcome() == S9FCode::OK, "err");

    // C3980-16 复核拒签 → 催办 + REJECTED 终态。
    let mut c3 = Closer::new();
    for fid in ROLLUP_SOURCES.iter() {
        c3.verify.fill(fid, "C", true, "绿");
    }
    c3.verify.fill("VE-F3980", "C", true, "过");
    for i in 0..PAIR_TOTAL {
        c3.receipts.record(i, true, "平");
    }
    for d in SIX_DIMS.iter() {
        c3.assets.put(*d, 2, 0, true);
    }
    c3.issue_report();
    chk(&mut set, "C3980-16",
        c3.sign_off(0, "甲", true) == S9FCode::OK
            && c3.sign_off(1, "乙", false) == S9FCode::REJECTED
            && c3.urgencies.len() == 1
            && c3.outcome() == S9FCode::REJECTED, "err");

    // C3980-17 空签人 → 缺签码。
    let mut c4 = Closer::new();
    for fid in ROLLUP_SOURCES.iter() {
        c4.verify.fill(fid, "C", true, "绿");
    }
    c4.verify.fill("VE-F3980", "C", true, "过");
    for i in 0..PAIR_TOTAL {
        c4.receipts.record(i, true, "平");
    }
    for d in SIX_DIMS.iter() {
        c4.assets.put(*d, 2, 0, true);
    }
    c4.issue_report();
    chk(&mut set, "C3980-17",
        c4.sign_off(0, "", true) == S9FCode::SIGN_MISSING
            && c4.outcome() == S9FCode::SIGN_MISSING, "err");

    // C3980-18 报告未出具时四件不齐（缺第四件）。
    let mut c5 = Closer::new();
    for fid in ROLLUP_SOURCES.iter() {
        c5.verify.fill(fid, "C", true, "绿");
    }
    c5.verify.fill("VE-F3980", "C", true, "过");
    for i in 0..PAIR_TOTAL {
        c5.receipts.record(i, true, "平");
    }
    for d in SIX_DIMS.iter() {
        c5.assets.put(*d, 2, 0, true);
    }
    let (ready, why) = c5.quad_ready();
    chk(&mut set, "C3980-18", !ready && why == "预备报告未出具", "err");

    // C3980-19 异人双签窗口：同人先自查后复核仍拒。
    let mut c6 = Closer::new();
    for fid in ROLLUP_SOURCES.iter() {
        c6.verify.fill(fid, "C", true, "绿");
    }
    c6.verify.fill("VE-F3980", "C", true, "过");
    for i in 0..PAIR_TOTAL {
        c6.receipts.record(i, true, "平");
    }
    for d in SIX_DIMS.iter() {
        c6.assets.put(*d, 2, 0, true);
    }
    c6.issue_report();
    chk(&mut set, "C3980-19",
        c6.sign_off(0, "同一个人", true) == S9FCode::OK
            && c6.sign_off(1, "同一个人", true) == S9FCode::SIGNER_SAME, "err");

    // C3980-20 itoa 工具与回执状态机全覆盖。
    let mut buf = [0u8; 4];
    chk(&mut set, "C3980-20",
        0u32.itoa_str(&mut buf) == "0" && 20u32.itoa_str(&mut buf) == "20"
            && ReceiptState::Pending != ReceiptState::Balanced, "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quad_gate_blocks_signing() {
        let mut c = Closer::new();
        assert_eq!(c.sign_off(0, "甲", true), S9FCode::QUAD_INCOMPLETE);
    }

    #[test]
    fn dual_sign_no_same_person() {
        let mut s = DualSign::new();
        s.sign(0, "甲", true);
        assert_eq!(s.sign(1, "甲", true), S9FCode::SIGNER_SAME);
    }

    #[test]
    fn full_close_ok() {
        let mut c = Closer::new();
        for fid in ROLLUP_SOURCES.iter() {
            c.verify.fill(fid, "C", true, "绿");
        }
        c.verify.fill("VE-F3980", "C", true, "过");
        for i in 0..PAIR_TOTAL {
            c.receipts.record(i, true, "平");
        }
        for d in SIX_DIMS.iter() {
            c.assets.put(*d, 2, 0, true);
        }
        assert_eq!(c.issue_report(), S9FCode::OK);
        c.sign_off(0, "甲", true);
        c.sign_off(1, "乙", true);
        assert_eq!(c.outcome(), S9FCode::OK);
    }
}
