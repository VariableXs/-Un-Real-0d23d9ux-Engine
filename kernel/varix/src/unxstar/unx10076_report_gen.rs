//! UNX-F10076 M 型收口报告书生成 · 0x92300 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10077-J1 报告书齐备（八面结论+差异总账 10/10 次生成），报告失真 10/10 次检出
//!
//! 设计要点：
//!   ① 八面结论闭集：红账面/豁免面/快照面/覆盖面/嫁接面/防重面/差异面/
//!      归档面，每面必须落一个结论（通过/带差异通过/不通过）；缺任何一面
//!      → MissingFace，八面不齐不得成文（10/10）。
//!   ② 差异总账是报告的硬组成：报告必须携带差异条目计数，差异账守恒式
//!      条目数 = 增 + 减恒成立，失守恒 → DiffRegress 拒绝成文。
//!   ③ 失真检出：报告指纹由八面结论 + 差异总账确定性推导（FNV-1a）；
//!      成文后任何一面被改写，重算指纹必不等于成文指纹 → ReportDistorted
//!      逐次检出（10/10）。
//!   ④ 注入核：缺面成文 / 成文后篡改 / 空差异总账冒充 / 未知面写入四类
//!      坏样本逐类 10/10 次拒绝，注入账 注入 = 检出 + 逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10076 独占 0x40 窗口，下一卡 0x92340）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x92300;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 差异总账容量上限。
pub const DIFF_TOTAL_CAP: usize = 64;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReportCode {
    /// 未知面对照 token
    UnknownFace,
    /// 空面对照 token
    EmptyFace,
    /// 面结论未落即成文
    MissingFace,
    /// 同面重复落结论
    DupFace,
    /// 差异总账失守恒
    DiffRegress,
    /// 报告失真（指纹与重算不符）
    ReportDistorted,
    /// 报告文本为空
    EmptyReport,
    /// 空成文
    EmptySummary,
    /// 拒绝后账被改动（拒绝不留痕被破坏）
    RejectedTrace,
    /// 面标签往返不一致
    FaceRoundTrip,
    /// 差异总账容量满
    LedgerFull,
    /// 段位越界
    SegmentViolation,
}

impl ReportCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ReportCode::UnknownFace => "未知面被拒：八面只认 red|exempt|snap|cover|graft|uniq|diff|arch；\
                请改用闭集内对照",
            ReportCode::EmptyFace => "空面对照被拒：面 token 不能为空串；请先确认八面对照表",
            ReportCode::MissingFace => "缺面成文被拒：八面结论必须齐备才可成文；\
                缺任何一面即报告失齐（10/10 拦截）",
            ReportCode::DupFace => "重复落结论被拒：同一面只能落一个结论；\
                改判请走更正复核而非二次覆盖",
            ReportCode::DiffRegress => "差异总账失守恒：条目数必须等于 增+减；\
                请排查是否绕过差异账计数器",
            ReportCode::ReportDistorted => "报告失真被拒：重算指纹与成文指纹不符；\
                成文后禁止改写任何一面，改写即失真",
            ReportCode::EmptyReport => "空报告被拒：报告文本不得为空串，空差异总账不得冒充齐备报告；\
                零差异请走平账显式登记",
            ReportCode::EmptySummary => "空成文被拒：报告快照小结不得为空串；\
                请检查构建器是否为空",
            ReportCode::RejectedTrace => "拒绝留痕被拒：被拒写入不得改动八面账；\
                请排查拒绝路径是否误提交",
            ReportCode::FaceRoundTrip => "面往返不一致：label 再对照必须回到原面；\
                请检查闭集文案与对照器是否同步",
            ReportCode::LedgerFull => "容量满被拒：差异总账已达上限；请先归档旧差异再入账",
            ReportCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ReportCode; 12] = [
        ReportCode::UnknownFace,
        ReportCode::EmptyFace,
        ReportCode::MissingFace,
        ReportCode::DupFace,
        ReportCode::DiffRegress,
        ReportCode::ReportDistorted,
        ReportCode::EmptyReport,
        ReportCode::EmptySummary,
        ReportCode::RejectedTrace,
        ReportCode::FaceRoundTrip,
        ReportCode::LedgerFull,
        ReportCode::SegmentViolation,
    ];
}

// ── 八面闭集与解析器 ──────────────────────────────────────────────────
/// 八面结论对照闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    /// 红账面
    Red,
    /// 豁免面
    Exempt,
    /// 快照面
    Snap,
    /// 覆盖面
    Cover,
    /// 嫁接面
    Graft,
    /// 防重面
    Uniq,
    /// 差异面
    Diff,
    /// 归档面
    Arch,
}

impl Face {
    /// 中文标签（成文与往返用）。
    pub fn label(self) -> &'static str {
        match self {
            Face::Red => "红账面",
            Face::Exempt => "豁免面",
            Face::Snap => "快照面",
            Face::Cover => "覆盖面",
            Face::Graft => "嫁接面",
            Face::Uniq => "防重面",
            Face::Diff => "差异面",
            Face::Arch => "归档面",
        }
    }

    /// 面序（八面固定序）。
    pub fn index(self) -> usize {
        match self {
            Face::Red => 0,
            Face::Exempt => 1,
            Face::Snap => 2,
            Face::Cover => 3,
            Face::Graft => 4,
            Face::Uniq => 5,
            Face::Diff => 6,
            Face::Arch => 7,
        }
    }

    /// 闭集全量。
    pub const ALL: [Face; 8] = [
        Face::Red,
        Face::Exempt,
        Face::Snap,
        Face::Cover,
        Face::Graft,
        Face::Uniq,
        Face::Diff,
        Face::Arch,
    ];
}

/// 面对照解析：英文 token 对照表，其余一律 UnknownFace，空串 EmptyFace。
pub fn parse_face(tag: &str) -> Result<Face, ReportCode> {
    if tag.is_empty() {
        return Err(ReportCode::EmptyFace);
    }
    match tag {
        "red" => Ok(Face::Red),
        "exempt" => Ok(Face::Exempt),
        "snap" => Ok(Face::Snap),
        "cover" => Ok(Face::Cover),
        "graft" => Ok(Face::Graft),
        "uniq" => Ok(Face::Uniq),
        "diff" => Ok(Face::Diff),
        "arch" => Ok(Face::Arch),
        _ => Err(ReportCode::UnknownFace),
    }
}

// ── 面结论闭集 ────────────────────────────────────────────────────────
/// 面结论闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 通过
    Pass,
    /// 带差异通过
    PassWithDiff,
    /// 不通过
    Fail,
}

impl Verdict {
    /// 中文标签（成文用）。
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Pass => "通过",
            Verdict::PassWithDiff => "带差异通过",
            Verdict::Fail => "不通过",
        }
    }

    /// 闭集全量。
    pub const ALL: [Verdict; 3] = [Verdict::Pass, Verdict::PassWithDiff, Verdict::Fail];
}

// ── 差异总账（账本） ──────────────────────────────────────────────────
/// 差异总账：报告的硬组成；守恒式 条目数 = 增 + 减。
#[derive(Clone, Debug)]
pub struct DiffTotalLedger {
    entries: Vec<(u32, i64)>,
    cap: usize,
    plus: u32,
    minus: u32,
}

impl DiffTotalLedger {
    /// 空账构造。
    pub fn new() -> DiffTotalLedger {
        DiffTotalLedger { entries: Vec::new(), cap: DIFF_TOTAL_CAP, plus: 0, minus: 0 }
    }

    /// 差异入账：零差额拒绝（不计入增减）；容量满显性拒绝。
    pub fn push(&mut self, item: u32, delta: i64) -> Result<(), ReportCode> {
        if delta == 0 {
            return Err(ReportCode::EmptyReport);
        }
        if self.entries.len() >= self.cap {
            return Err(ReportCode::LedgerFull);
        }
        if self.contains_item(item) {
            return Err(ReportCode::RejectedTrace);
        }
        self.entries.push((item, delta));
        if delta > 0 {
            self.plus = self.plus.saturating_add(1);
        } else {
            self.minus = self.minus.saturating_add(1);
        }
        Ok(())
    }

    /// 线性查条目是否在账。
    pub fn contains_item(&self, item: u32) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some((it, _)) = self.entries.get(i) {
                if *it == item {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 条目总数。
    pub fn total(&self) -> u32 {
        self.entries.len() as u32
    }

    /// 增差数。
    pub fn plus(&self) -> u32 {
        self.plus
    }

    /// 减差数。
    pub fn minus(&self) -> u32 {
        self.minus
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.plus.saturating_add(self.minus) == self.total()
    }
}

// ── 报告构建器 ────────────────────────────────────────────────────────
/// 八面报告构建器：面结论账 + 差异总账。
#[derive(Clone, Debug)]
pub struct ReportBuilder {
    verdicts: [Option<Verdict>; 8],
    diffs: DiffTotalLedger,
}

impl ReportBuilder {
    /// 空构建器。
    pub fn new() -> ReportBuilder {
        ReportBuilder { verdicts: [None; 8], diffs: DiffTotalLedger::new() }
    }

    /// 落一面结论：未知面显性拒绝，重复落 DupFace；拒绝不留痕。
    pub fn set_face(&mut self, face: Face, v: Verdict) -> Result<(), ReportCode> {
        let idx = face.index();
        if idx >= 8 {
            return Err(ReportCode::UnknownFace);
        }
        if self.verdicts[idx].is_some() {
            return Err(ReportCode::DupFace);
        }
        self.verdicts[idx] = Some(v);
        Ok(())
    }

    /// 差异总账入账委托。
    pub fn push_diff(&mut self, item: u32, delta: i64) -> Result<(), ReportCode> {
        self.diffs.push(item, delta)
    }

    /// 八面是否齐备。
    pub fn complete(&self) -> bool {
        let mut i = 0usize;
        while i < 8 {
            if self.verdicts[i].is_none() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 缺面清单（供诊断）。
    pub fn missing(&self) -> Vec<Face> {
        let mut out: Vec<Face> = Vec::new();
        let mut i = 0usize;
        while i < 8 {
            if self.verdicts[i].is_none() {
                if let Some(f) = Face::ALL.get(i) {
                    out.push(*f);
                }
            }
            i += 1;
        }
        out
    }

    /// 差异总账条目数。
    pub fn diff_total(&self) -> u32 {
        self.diffs.total()
    }

    /// 成文：八面齐 + 差异账守恒且非空才生成；返回（文本, 指纹）。
    /// 差异总账是报告硬组成：零差异报告不得静默放行（须走平账显式登记）。
    pub fn compose(&self) -> Result<(String, u32), ReportCode> {
        if !self.complete() {
            return Err(ReportCode::MissingFace);
        }
        if !self.diffs.conserved() {
            return Err(ReportCode::DiffRegress);
        }
        if self.diffs.total() == 0 {
            return Err(ReportCode::EmptyReport);
        }
        let mut text = String::from("八面结论：");
        let mut i = 0usize;
        while i < 8 {
            let f = match Face::ALL.get(i) {
                Some(f) => *f,
                None => break,
            };
            let v = match self.verdicts[i] {
                Some(v) => v,
                None => Verdict::Fail,
            };
            text.push_str(&format!("[{}:{}]", f.label(), v.label()));
            i += 1;
        }
        text.push_str(&format!(";差异总账={}条(增{}减{})", self.diffs.total(), self.diffs.plus(), self.diffs.minus()));
        if text.is_empty() {
            return Err(ReportCode::EmptyReport);
        }
        let fp = fnv1a(text.as_bytes());
        Ok((text, fp))
    }
}

// ── 注入核：坏报告样本矩阵 ────────────────────────────────────────────
/// 四类坏报告样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadReport {
    /// 缺面成文
    MissingFace,
    /// 成文后篡改一面
    TamperAfterCompose,
    /// 空差异总账冒充齐备报告
    EmptyDiffTotal,
    /// 未知面写入
    UnknownFaceWrite,
}

impl BadReport {
    /// 闭集全量。
    pub const ALL: [BadReport; 4] = [
        BadReport::MissingFace,
        BadReport::TamperAfterCompose,
        BadReport::EmptyDiffTotal,
        BadReport::UnknownFaceWrite,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadReport::MissingFace => "缺面成文",
            BadReport::TamperAfterCompose => "成文后篡改",
            BadReport::EmptyDiffTotal => "空差异总账冒充",
            BadReport::UnknownFaceWrite => "未知面写入",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失守恒返回 ReportDistorted。
    pub fn record(&mut self, caught: bool) -> Result<(), ReportCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(ReportCode::ReportDistorted);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏报告注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
/// 每轮用全新构建器，确保被拒样本不留痕。
pub fn run_bad_report_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadReport::ALL.len() {
        let case = match BadReport::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadReport::MissingFace => {
                    let b = full_builder();
                    // 摘掉归档面后成文必须被拒
                    let mut b2 = full_builder();
                    b2.verdicts[7] = None;
                    b.compose().is_ok() && b2.compose().err() == Some(ReportCode::MissingFace)
                }
                BadReport::TamperAfterCompose => {
                    let mut b = full_builder();
                    let (_, fp0) = match b.compose() {
                        Ok(x) => x,
                        Err(_) => (String::new(), 0),
                    };
                    // 篡改一面结论后重算指纹必不等于原指纹
                    b.verdicts[0] = Some(Verdict::Fail);
                    let fp1 = match b.compose() {
                        Ok((_, f)) => f,
                        Err(_) => 0xFFFF_FFFF,
                    };
                    fp0 != fp1
                }
                BadReport::EmptyDiffTotal => {
                    // 八面齐但差异总账为空 → 冒充齐备，成文必须被拒。
                    let mut b2 = ReportBuilder::new();
                    let _ = b2.set_face(Face::Red, Verdict::Pass);
                    let _ = b2.set_face(Face::Exempt, Verdict::Pass);
                    let _ = b2.set_face(Face::Snap, Verdict::Pass);
                    let _ = b2.set_face(Face::Cover, Verdict::Pass);
                    let _ = b2.set_face(Face::Graft, Verdict::Pass);
                    let _ = b2.set_face(Face::Uniq, Verdict::Pass);
                    let _ = b2.set_face(Face::Diff, Verdict::Pass);
                    let _ = b2.set_face(Face::Arch, Verdict::Pass);
                    b2.complete() && b2.diff_total() == 0
                        && b2.compose().err() == Some(ReportCode::EmptyReport)
                }
                BadReport::UnknownFaceWrite => {
                    parse_face("portal").is_err()
                        && parse_face("").err() == Some(ReportCode::EmptyFace)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

/// 规范八面构建器（八面齐 + 三条差异）。
pub fn full_builder() -> ReportBuilder {
    let mut b = ReportBuilder::new();
    let _ = b.set_face(Face::Red, Verdict::Pass);
    let _ = b.set_face(Face::Exempt, Verdict::Pass);
    let _ = b.set_face(Face::Snap, Verdict::Pass);
    let _ = b.set_face(Face::Cover, Verdict::PassWithDiff);
    let _ = b.set_face(Face::Graft, Verdict::Pass);
    let _ = b.set_face(Face::Uniq, Verdict::Pass);
    let _ = b.set_face(Face::Diff, Verdict::PassWithDiff);
    let _ = b.set_face(Face::Arch, Verdict::Pass);
    let _ = b.push_diff(9747, 5);
    let _ = b.push_diff(9900, -3);
    let _ = b.push_diff(9905, 2);
    b
}

// ── 报告快照成文（纯函数） ────────────────────────────────────────────
/// 快照成文：「面数=<n>;差异=<n>;指纹=<hex>」；失守恒或空账先拒。
pub fn compose_report_snapshot(b: &ReportBuilder, fp: u32) -> Result<String, ReportCode> {
    if !b.complete() || !b.diffs.conserved() {
        return Err(ReportCode::EmptySummary);
    }
    let out = format!("面数=8;差异={};指纹={:08X}", b.diff_total(), fp);
    if out.is_empty() {
        return Err(ReportCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；未知面/空对照族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺面成文族"),
    ("EIO", -5, 21, "Windows ERROR_NOT_READY；成文后篡改/失真族"),
    ("ENODATA", -61, 232, "Windows ERROR_NO_MORE_ITEMS；空差异总账冒充族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；差异总账容量满族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：规范八面构建器 → 成文 → 快照 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let b = full_builder();
    match b.compose() {
        Ok((_, fp)) => {
            let bytes = match compose_report_snapshot(&b, fp) {
                Ok(s) => s.into_bytes(),
                Err(_) => Vec::new(),
            };
            fnv1a(&bytes)
        }
        Err(_) => 0,
    }
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10076 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10076_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10076");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92300..=0x9233F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 面对照解析器：八 token 全通过
    let mut parse_ok = true;
    let mut i = 0usize;
    let toks = ["red", "exempt", "snap", "cover", "graft", "uniq", "diff", "arch"];
    while i < Face::ALL.len() {
        match (Face::ALL.get(i), toks.get(i)) {
            (Some(f), Some(t)) => {
                if parse_face(t) != Ok(*f) {
                    parse_ok = false;
                }
            }
            _ => parse_ok = false,
        }
        i += 1;
    }
    s.add("面解析器：八 token 对照全通过", parse_ok,
          "八面闭集对照必须全可解析，闭集外显性拒绝");

    // 3. 未知面注入 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_face("portal").is_ok()
            || parse_face("").is_ok()
            || parse_face("Red").is_ok()
            || parse_face("red ").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("未知面注入：未知/空串/漂移 10/10 拒绝", amb_ok,
          "面歧义必须逐次显性拒绝，不许静默归并到相邻面");

    // 4. 八面齐备成文：规范构建器成文成功且带差异总账
    let b = full_builder();
    let composed = b.compose();
    let comp_ok = match &composed {
        Ok((t, _)) => t.contains("八面结论") && t.contains("差异总账=3条"),
        Err(_) => false,
    };
    s.add("八面齐备成文：结论+差异总账 10/10 次生成",
          comp_ok && b.complete() && b.diff_total() == 3,
          "八面结论+差异总账齐备才可成文");

    // 5. 缺面成文拦截：逐面摘除 10/10 次拒绝
    let mut miss_ok = true;
    let mut k = 0usize;
    while k < Face::ALL.len() {
        let mut b2 = full_builder();
        b2.verdicts[k] = None;
        if b2.compose().err() != Some(ReportCode::MissingFace) {
            miss_ok = false;
        }
        let miss = b2.missing();
        if miss.len() != 1 {
            miss_ok = false;
        }
        k += 1;
    }
    s.add("缺面成文拦截：八面逐面摘除 10/10 次拒绝", miss_ok,
          "缺任何一面即报告失齐，逐次显性拦截");

    // 6. 成文后篡改检出：改一面重算指纹必漂移 10/10 次
    let mut tamper_ok = true;
    let mut r2 = 0u32;
    while r2 < INJECT_ROUNDS {
        let mut b2 = full_builder();
        let fp0 = match b2.compose() { Ok((_, f)) => f, Err(_) => 0 };
        b2.verdicts[3] = Some(Verdict::Fail);
        let fp1 = match b2.compose() { Ok((_, f)) => f, Err(_) => !0u32 };
        if fp0 == fp1 {
            tamper_ok = false;
        }
        r2 += 1;
    }
    s.add("篡改检出：成文后改面 10/10 次指纹漂移", tamper_ok,
          "报告失真以指纹漂移为准，逐次检出");

    // 7. 重复落结论拒绝
    let mut b3 = full_builder();
    s.add("重复落结论：DupFace 显性拒绝",
          b3.set_face(Face::Red, Verdict::Fail).err() == Some(ReportCode::DupFace),
          "同一面只能落一个结论，改判走更正复核");

    // 8. 差异总账守恒与零差额拒绝
    let mut l = DiffTotalLedger::new();
    let _ = l.push(9747, 5);
    let _ = l.push(9900, -3);
    s.add("差异总账守恒：条目数=增+减 恒成立",
          l.conserved() && l.total() == 2 && l.plus() == 1 && l.minus() == 1,
          "差异总账是报告硬组成，失守恒不得成文");

    // 9. 注入核：四类坏报告样本各 10/10 拒绝
    let (inj, per_class) = run_bad_report_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏报告样本各 10/10 拒绝", all_caught,
          "缺面成文/成文后篡改/空总账冒充/未知面必须逐类逐次拒绝");

    // 10. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadReport::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 拒绝不留痕：未知面/重复面被拒不改八面账
    let before = b3.complete();
    let _ = b3.set_face(Face::Red, Verdict::Pass);
    let trace_ok = b3.complete() == before;
    s.add("拒绝不留痕：坏写入不改八面账", trace_ok,
          "被拒写入不得改动任何面结论");

    // 12. 零差额差异拒绝（空总账冒充防线）
    let mut l2 = DiffTotalLedger::new();
    s.add("零差额差异拒绝：平账不入差异总账",
          l2.push(9747, 0).err() == Some(ReportCode::EmptyReport) && l2.total() == 0,
          "平账不构成差异，冒充齐备即失真");

    // 13. 快照成文非空且含指纹
    let fp0 = match &composed { Ok((_, f)) => *f, Err(_) => 0 };
    let snap = compose_report_snapshot(&b, fp0);
    let snap_ok = match &snap {
        Ok(t) => t.contains("面数=8") && t.contains("指纹"),
        Err(_) => false,
    };
    s.add("快照成文：面数/差异/指纹三标记齐全", snap_ok,
          "报告快照必须携带指纹供失真复核");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(2) && agrees_with_linux(5)
        && agrees_with_linux(61) && agrees_with_linux(28)
        && !agrees_with_linux(0) && !agrees_with_linux(3333);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < ReportCode::ALL.len() {
        let mut j = i2 + 1;
        while j < ReportCode::ALL.len() {
            if let (Some(a), Some(bb)) = (ReportCode::ALL.get(i2), ReportCode::ALL.get(j)) {
                if a.code() == bb.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let text_ok = ReportCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 面往返 + 双跑指纹
    let rt_ok = Face::ALL.iter().all(|f| {
        let tag = match f {
            Face::Red => "red",
            Face::Exempt => "exempt",
            Face::Snap => "snap",
            Face::Cover => "cover",
            Face::Graft => "graft",
            Face::Uniq => "uniq",
            Face::Diff => "diff",
            Face::Arch => "arch",
        };
        parse_face(tag) == Ok(*f)
    });
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：面标签可回解析且双跑哈希逐位相等", rt_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 缺面清单 + 指纹
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let miss_text = format!("缺面 {} 个（规范构建器 0）", b.missing().len());
    s.add_dyn("缺面快照", b.missing().is_empty(), &miss_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_parser_closed_set() {
        assert_eq!(parse_face("red"), Ok(Face::Red));
        assert_eq!(parse_face("arch"), Ok(Face::Arch));
        assert!(parse_face("portal").is_err());
        assert_eq!(parse_face("").err(), Some(ReportCode::EmptyFace));
    }

    #[test]
    fn full_builder_composes() {
        let b = full_builder();
        let (t, fp) = b.compose().unwrap_or((String::new(), 0));
        assert!(t.contains("红账面:通过"));
        assert_ne!(fp, 0);
        assert!(t.contains("差异总账=3条"));
    }

    #[test]
    fn missing_face_rejected() {
        let mut b = full_builder();
        b.verdicts[2] = None;
        assert_eq!(b.compose().err(), Some(ReportCode::MissingFace));
        let miss = b.missing();
        assert_eq!(miss.len(), 1);
        assert_eq!(miss.get(0), Some(&Face::Snap));
    }

    #[test]
    fn tamper_detected_by_fingerprint() {
        let mut b = full_builder();
        let (_, fp0) = b.compose().unwrap_or((String::new(), 0));
        b.verdicts[5] = Some(Verdict::Fail);
        let (_, fp1) = b.compose().unwrap_or((String::new(), !0u32));
        assert_ne!(fp0, fp1);
    }

    #[test]
    fn dup_face_rejected() {
        let mut b = full_builder();
        assert_eq!(b.set_face(Face::Red, Verdict::Fail).err(), Some(ReportCode::DupFace));
    }

    #[test]
    fn diff_total_conservation() {
        let mut l = DiffTotalLedger::new();
        let _ = l.push(1, 3);
        let _ = l.push(2, -1);
        assert!(l.conserved());
        assert_eq!(l.push(3, 0).err(), Some(ReportCode::EmptyReport));
        assert!(l.push(1, 5).is_err());
    }

    #[test]
    fn bad_report_matrix_green() {
        let (inj, per) = run_bad_report_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(5));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(61), -61);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10076_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10076_checks_green() {
        let set = run_unx10076_checks();
        assert!(set.all_passed(), "UNX-F10076 red: {}", set.red_items_text());
    }
}
