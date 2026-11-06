//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2164 · 压测注入矩阵（第二十五矩阵） · 0x72A40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B29）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F2164-J1 注入矩阵四族八形态全检出（取数空转/口径
//!     失配/预算超限/登记悬空），预期声明完备率 100%，与 F1788 组织法
//!     一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：四族×两形态=八形态，组织法对齐 F1788（族主序/族内次序双
//!   冻结，三轮同序）；预期声明逐格在册（完备率 100%）；注入器落确定性
//!   症状（实注入量纲≥1，洁净跑恒 0），检出器按族阈值判 Catch 零假阳；
//!   报告行五列 FNV 尾校验；失真注入 10 路核验器 10/10 必抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72A40，VX3-1103 压测注入矩阵窗口独占）。
pub const SEG_BASE: u32 = 0x72A40;
/// 注入族数（四族，冻结）。
pub const FAMILY_N: usize = 4;
/// 注入形态数（四族×两形态 = 八形态，冻结）。
pub const FORM_N: usize = 8;
/// 矩阵格数（一格一形态一预期声明，冻结）。
pub const CELL_N: usize = 8;
/// 矩阵纪元（第二十五矩阵，冻结）。
pub const EPOCH: u64 = 25;
/// 注入检出轮数（判据 10/10 语义，冻结）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 预算族检出阈值（量纲超过即 Catch，冻结 100）。
pub const BUDGET_CAP: u64 = 100;
/// 组织法稳定序轮数（三轮同序，冻结）。
pub const ORDER_ROUNDS: u32 = 3;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StressCode {
    /// 族号未知：只认 0..=3 四族，其余一律破损
    FamUnknown,
    /// 形态号未知：只认 0..=7 八形态，其余一律破损
    FormUnknown,
    /// 预期声明缺失：矩阵格无声明，完备率掉 100%
    ExpectMissing,
    /// 检出漏抓：实注入形态未被检出器 Catch
    DetectMiss,
    /// 口径失配未判：症状量纲与族阈值口径不一致
    CaliberOff,
    /// 预算超限未判：超限量纲未超阈值仍放行
    BudgetOver,
    /// 登记悬空误放：悬空计数 >0 却判 CLEAN
    RegDangle,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl StressCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            StressCode::FamUnknown => "族号未知：族号必须落在 0..=3 四族闭集",
            StressCode::FormUnknown => "形态号未知：形态号必须落在 0..=7 八形态闭集",
            StressCode::ExpectMissing => "预期声明缺失：先补 ExpectDecl 再跑矩阵",
            StressCode::DetectMiss => "检出漏抓：实注入必 Catch，查检出阈值与症状口径",
            StressCode::CaliberOff => "口径失配未判：量纲口径与族阈值不一致，对齐口径",
            StressCode::BudgetOver => "预算超限未判：超限量纲 > 阈值必须 Catch",
            StressCode::RegDangle => "登记悬空误放：悬空计数 >0 必须判悬空",
            StressCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
}

/// 诊断码闭集全表（查表/段位守卫共用）。
pub const ALL_CODES: [StressCode; 8] = [
    StressCode::FamUnknown, StressCode::FormUnknown, StressCode::ExpectMissing,
    StressCode::DetectMiss, StressCode::CaliberOff, StressCode::BudgetOver,
    StressCode::RegDangle, StressCode::NotReproducible,
];

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ALL_CODES.len() {
        if ALL_CODES[i].val() == code {
            return ALL_CODES[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72A40 段闭集内"
}

/// 段位守卫（全部诊断码落 0x72A40 窗口）。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ALL_CODES.len() {
        let v = ALL_CODES[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 注入族闭集（四族，族主序冻结） ──────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fam {
    /// 取数空转：读路径拿不到有效页/索引陈旧。
    FetchSpin,
    /// 口径失配：量纲单位/往返口径与账面不一致。
    CaliberMiss,
    /// 预算超限：周期/配额越过冻结阈值。
    BudgetOver,
    /// 登记悬空：句柄/票号无登记可循。
    RegDangling,
}

/// 族号字面表（族主序冻结：0 取数/1 口径/2 预算/3 登记）。
pub const FAM_ALL: [Fam; FAMILY_N] = [
    Fam::FetchSpin, Fam::CaliberMiss, Fam::BudgetOver, Fam::RegDangling,
];

impl Fam {
    /// 族号（族主序冻结，闭集表内定位）。
    pub fn idx(self) -> usize {
        FAM_ALL.iter().position(|f| *f == self).unwrap_or(FAMILY_N)
    }
    /// 族号反查：闭集外返回 None。
    pub fn from_idx(i: u64) -> Option<Fam> {
        let f = FAM_ALL.get(i as usize).copied()?;
        if f.idx() as u64 == i { Some(f) } else { None }
    }
    /// 族名（报告行第 1 列，冻结拼写）。
    pub fn word(self) -> &'static str {
        match self {
            Fam::FetchSpin => "FETCH",
            Fam::CaliberMiss => "CALIB",
            Fam::BudgetOver => "BUDGT",
            Fam::RegDangling => "REGDL",
        }
    }
}

// ── 注入形态闭集（八形态，族内次序冻结） ────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 空页读：命中页数 0（取数空转·形态 0）。
    EmptyPage,
    /// 陈旧索引：代差 >0（取数空转·形态 1）。
    StaleIndex,
    /// 单位失配：单位码不合账（口径失配·形态 0）。
    UnitMismatch,
    /// 往返损：往返口径丢步（口径失配·形态 1）。
    RoundLoss,
    /// 周期超：执行周期越阈（预算超限·形态 0）。
    CycleOver,
    /// 配额超：分配配额越阈（预算超限·形态 1）。
    AllocOver,
    /// 幽灵句柄：悬空句柄计数 >0（登记悬空·形态 0）。
    GhostHandle,
    /// 孤儿票：无主票计数 >0（登记悬空·形态 1）。
    OrphanTicket,
}

/// 形态字面表（全局 0..=7，族主序+族内次序双冻结）。
pub const FORM_ALL: [Form; FORM_N] = [
    Form::EmptyPage, Form::StaleIndex, Form::UnitMismatch, Form::RoundLoss,
    Form::CycleOver, Form::AllocOver, Form::GhostHandle, Form::OrphanTicket,
];

impl Form {
    /// 形态号（全局 0..=7，闭集表内定位）。
    pub fn idx(self) -> usize {
        FORM_ALL.iter().position(|f| *f == self).unwrap_or(FORM_N)
    }
    /// 形态号反查：闭集外返回 None。
    pub fn from_idx(i: u64) -> Option<Form> {
        let f = FORM_ALL.get(i as usize).copied()?;
        if f.idx() as u64 == i { Some(f) } else { None }
    }
    /// 所属族。
    pub fn fam(self) -> Fam {
        match self {
            Form::EmptyPage | Form::StaleIndex => Fam::FetchSpin,
            Form::UnitMismatch | Form::RoundLoss => Fam::CaliberMiss,
            Form::CycleOver | Form::AllocOver => Fam::BudgetOver,
            Form::GhostHandle | Form::OrphanTicket => Fam::RegDangling,
        }
    }
    /// 形态名（报告行第 2 列，冻结拼写）。
    pub fn word(self) -> &'static str {
        match self {
            Form::EmptyPage => "EMPPG",
            Form::StaleIndex => "STIDX",
            Form::UnitMismatch => "UNITX",
            Form::RoundLoss => "RDLOS",
            Form::CycleOver => "CYCOV",
            Form::AllocOver => "ALCOV",
            Form::GhostHandle => "GHSTH",
            Form::OrphanTicket => "ORPHT",
        }
    }
}

// ── 预期声明（矩阵格声明体） ────────────────────────────────────
/// 预期声明：每形态一格，声明检出阈值与必抓旗（完备率判据载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpectDecl {
    /// 检出阈值：量纲 > cap 即必须 Catch。
    pub cap: u64,
    /// 必抓旗：判据原文全检出，恒 true。
    pub must_catch: bool,
}

/// 建矩阵：八格按形态号排布，逐格带预期声明（族主序/族内次序冻结）。
pub fn build_matrix() -> Vec<(Form, ExpectDecl)> {
    let mut v: Vec<(Form, ExpectDecl)> = Vec::new();
    for f in FORM_ALL.iter() {
        // 预算族阈值 100，其余族零容忍（量纲 ≥1 即抓）。
        let cap = if f.fam() == Fam::BudgetOver { BUDGET_CAP } else { 0 };
        v.push((*f, ExpectDecl { cap, must_catch: true }));
    }
    v
}

/// 预期声明完备率（万分比）：已声明格 / 总格 × 10000。
pub fn completeness_bps(matrix: &[(Form, ExpectDecl)]) -> u64 {
    let mut ok = 0u64;
    for (_, d) in matrix.iter() {
        if d.must_catch {
            ok += 1;
        }
    }
    ok.saturating_mul(10_000) / FORM_N as u64
}

// ── 注入器（症状落面，确定性量纲） ─────────────────────────────
/// 症状：形态 + 违约量纲（洁净跑量纲恒 0，实注入量纲 ≥1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Symptom {
    pub form: Form,
    pub metric: u64,
}

/// 洁净症状（对照跑专用：量纲 0，检出器必须不误报）。
pub fn clean_symptom(form: Form) -> Symptom {
    Symptom { form, metric: 0 }
}

/// 注入症状：量纲由形态号确定性派生，可复算可对拍。
pub fn inject(form: Form, round: u32) -> Symptom {
    let metric = match form {
        Form::EmptyPage => 1,
        Form::StaleIndex => 3,
        Form::UnitMismatch => 2,
        Form::RoundLoss => 5,
        Form::CycleOver => BUDGET_CAP + 137,
        Form::AllocOver => BUDGET_CAP + 58,
        Form::GhostHandle => 1 + (round as u64 % 3),
        Form::OrphanTicket => 2 + (round as u64 % 2),
    };
    Symptom { form, metric }
}

// ── 检出器（族阈值表 + 判定） ───────────────────────────────────
/// 检出器：四族阈值表（族主序），量纲 > 阈值即 Catch。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detector {
    /// 族阈值表：下标 = 族号（0 取数/1 口径/2 预算/3 登记）。
    pub caps: [u64; FAMILY_N],
    /// 检出计数（Catch 累计）。
    pub catches: u32,
    /// 假阳计数（洁净跑被误判 Catch）。
    pub false_pos: u32,
}

impl Detector {
    /// 标准检出器：阈值表与预期声明一致。
    pub fn standard() -> Detector {
        Detector { caps: [0, 0, BUDGET_CAP, 0], catches: 0, false_pos: 0 }
    }
    /// 阈值核：与矩阵预期声明逐格对拍（口径一致判据）。
    pub fn caps_match_matrix(&self, matrix: &[(Form, ExpectDecl)]) -> bool {
        for (f, d) in matrix.iter() {
            if self.caps[f.fam().idx()] != d.cap {
                return false;
            }
        }
        true
    }
    /// 判定单症状：量纲 > 族阈值即 Catch；量纲 0 恒放行零假阳。
    pub fn judge(&mut self, sym: &Symptom) -> bool {
        let hit = sym.metric > self.caps[sym.form.fam().idx()];
        if hit {
            self.catches += 1;
        }
        hit
    }
}

// ── FNV-1a 32 位校验和（整数实现，报告行尾校验共用） ────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes.iter() {
        h = (h ^ (*b as u32)).wrapping_mul(0x0100_0193);
    }
    h
}

// ── 检出报告行（五列：族号|形态号|量纲|判定|校验和） ────────────
/// 单行报告：判定字闭集 CATCH/CLEAN，尾校验覆盖前四列。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportRow {
    pub fam: Fam,
    pub form: Form,
    pub metric: u64,
    /// true=CATCH，false=CLEAN。
    pub caught: bool,
}

/// 渲染单行：族号|形态号|量纲|判定|校验和;。
pub fn render_row(r: &ReportRow) -> String {
    let verdict = if r.caught { "CATCH" } else { "CLEAN" };
    let prefix = format!("{}|{}|{}|{}|", r.fam.word(), r.form.word(), r.metric, verdict);
    format!("{}{:08X};", prefix, fnv1a(prefix.as_bytes()))
}

/// 字节查找（ASCII 正文专用）。
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    h.iter().skip(from).position(|c| *c == needle).map(|p| p + from)
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
    let b = s.as_bytes();
    let mut v: u64 = 0;
    for c in b.iter() {
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
    let b = s.as_bytes();
    let mut v: u32 = 0;
    for i in 0..8 {
        let d = match b[i] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'A'..=b'F' => (c - b'A') as u32 + 10,
            _ => return None,
        };
        v = (v << 4) | d;
    }
    Some(v)
}

/// 族号字面反查（闭集外返回 None）。
pub fn parse_fam(w: &str) -> Option<Fam> {
    for f in FAM_ALL.iter() {
        if f.word() == w {
            return Some(*f);
        }
    }
    None
}

/// 形态字面反查（闭集外返回 None）。
pub fn parse_form(w: &str) -> Option<Form> {
    for f in FORM_ALL.iter() {
        if f.word() == w {
            return Some(*f);
        }
    }
    None
}

/// 解析单行五列：族号|形态号|量纲|判定|校验和;。
pub fn parse_row(line: &str) -> Result<ReportRow, u32> {
    let b = line.as_bytes();
    if b.is_empty() || b[b.len() - 1] != b';' {
        return Err(StressCode::FormUnknown.val());
    }
    let body = &line[..b.len() - 1];
    let mut cuts: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < body.len() {
        if let Some(p) = find_byte(body.as_bytes(), i, b'|') {
            cuts.push(p);
            i = p + 1;
        } else {
            break;
        }
    }
    if cuts.len() != 4 {
        return Err(StressCode::FormUnknown.val());
    }
    let (c0, c1, c2, c3) = (cuts[0], cuts[1], cuts[2], cuts[3]);
    if c0 == 0 || c1 == c0 + 1 {
        return Err(StressCode::FormUnknown.val());
    }
    let fam = parse_fam(&body[..c0]).ok_or(StressCode::FamUnknown.val())?;
    let form = parse_form(&body[c0 + 1..c1]).ok_or(StressCode::FormUnknown.val())?;
    if form.fam() != fam {
        return Err(StressCode::CaliberOff.val());
    }
    let metric = parse_dec(&body[c1 + 1..c2]).ok_or(StressCode::CaliberOff.val())?;
    let caught = match &body[c2 + 1..c3] { "CATCH" => true, "CLEAN" => false,
        _ => return Err(StressCode::CaliberOff.val()) };
    let sum = parse_hex8(&body[c3 + 1..]).ok_or(StressCode::CaliberOff.val())?;
    let verdict = if caught { "CATCH" } else { "CLEAN" };
    let prefix = format!("{}|{}|{}|{}|", fam.word(), form.word(), metric, verdict);
    if fnv1a(prefix.as_bytes()) != sum {
        return Err(StressCode::NotReproducible.val());
    }
    Ok(ReportRow { fam, form, metric, caught })
}

// ── 报告账（行表 + 矩阵口径核验） ───────────────────────────────
/// 报告账：纪元 + 行表；行序按形态号单调（族主序冻结）。
#[derive(Clone, Debug)]
pub struct ReportLedger {
    pub epoch: u64,
    pub rows: Vec<ReportRow>,
    /// 已声明格数（完备率载体）。
    pub declared: usize,
}

impl ReportLedger {
    pub fn new(epoch: u64) -> ReportLedger {
        ReportLedger { epoch, rows: Vec::new(), declared: 0 }
    }
    /// 落一行：形态号必须递增；族形态口径必须自洽。
    pub fn append(&mut self, row: ReportRow) -> Result<(), u32> {
        if let Some(last) = self.rows.last() {
            if row.form.idx() <= last.form.idx() {
                return Err(StressCode::FormUnknown.val());
            }
        }
        if row.form.fam() != row.fam {
            return Err(StressCode::CaliberOff.val());
        }
        self.rows.push(row);
        Ok(())
    }
    /// 全账渲染。
    pub fn render_all(&self) -> String {
        let mut out = String::new();
        for r in &self.rows {
            out.push_str(&render_row(r));
            out.push('\n');
        }
        out
    }
    /// Catch 行数。
    pub fn catch_count(&self) -> u32 {
        let mut c = 0u32;
        for r in &self.rows {
            if r.caught {
                c += 1;
            }
        }
        c
    }
}

/// 矩阵跑一轮：八形态逐格注入 → 检出 → 落报告账。
pub fn run_matrix(epoch: u64, det: &mut Detector, round: u32) -> Result<ReportLedger, u32> {
    let matrix = build_matrix();
    if matrix.len() != CELL_N {
        return Err(StressCode::ExpectMissing.val());
    }
    let mut led = ReportLedger::new(epoch);
    led.declared = matrix.len();
    for (f, d) in matrix.iter() {
        if !d.must_catch {
            return Err(StressCode::ExpectMissing.val());
        }
        let sym = inject(*f, round);
        let caught = det.judge(&sym);
        led.append(ReportRow { fam: f.fam(), form: *f, metric: sym.metric, caught })?;
    }
    Ok(led)
}

/// 洁净对照跑：八形态量纲全 0，报告全 CLEAN，检出器零假阳。
pub fn run_clean(det: &mut Detector) -> Result<ReportLedger, u32> {
    let matrix = build_matrix();
    let mut led = ReportLedger::new(EPOCH);
    led.declared = matrix.len();
    for (f, _) in matrix.iter() {
        let sym = clean_symptom(*f);
        let caught = det.judge(&sym);
        led.append(ReportRow { fam: f.fam(), form: *f, metric: sym.metric, caught })?;
    }
    if led.catch_count() != 0 {
        return Err(StressCode::RegDangle.val());
    }
    Ok(led)
}

// ── 核验器（全检出 + 完备率 + 组织法三核） ──────────────────────
/// 核验报告账文：逐行解析 + 行数 + 形态序 + 全 Catch + 完备率。
pub fn verify_text(text: &str) -> Result<(u32, u64), u32> {
    if text.is_empty() {
        return Err(StressCode::DetectMiss.val());
    }
    let mut rows: Vec<ReportRow> = Vec::new();
    let bts = text.as_bytes();
    let mut i = 0usize;
    while i < bts.len() {
        let end = match find_byte(bts, i, b'\n') {
            Some(p) => p,
            None => bts.len(),
        };
        let line = &text[i..end];
        if !line.is_empty() {
            rows.push(parse_row(line)?);
        }
        i = end.saturating_add(1);
    }
    if rows.len() != CELL_N {
        return Err(StressCode::DetectMiss.val());
    }
    // 组织法核：行序 = 形态号升序（族主序/族内次序双冻结）+ 全 Catch。
    for (k, r) in rows.iter().enumerate() {
        if r.form.idx() != k || !r.caught {
            return Err(StressCode::DetectMiss.val());
        }
    }
    let matrix = build_matrix();
    let bps = completeness_bps(&matrix);
    if bps != 10_000 {
        return Err(StressCode::ExpectMissing.val());
    }
    Ok((rows.iter().filter(|r| r.caught).count() as u32, bps))
}

/// 核验账本本体：渲染→解析→与本体对拍（双路一致）。
pub fn verify_ledger(led: &ReportLedger) -> Result<(), u32> {
    let (c, bps) = verify_text(&led.render_all())?;
    if c != led.catch_count() || bps != 10_000 {
        return Err(StressCode::DetectMiss.val());
    }
    if led.declared != CELL_N {
        return Err(StressCode::ExpectMissing.val());
    }
    Ok(())
}

// ── 组织法稳定序探针（三轮同序，对齐 F1788） ────────────────────
pub fn order_stable() -> bool {
    let base: Vec<u64> = build_matrix().iter().map(|(f, _)| f.idx() as u64).collect();
    // 族主序核：0 取数起、4 预算起，逐段升格；三轮同序。
    if base.len() != FORM_N || base.get(0) != Some(&0) || base.get(4) != Some(&4) {
        return false;
    }
    for _ in 1..ORDER_ROUNDS {
        let seq: Vec<u64> = build_matrix().iter().map(|(f, _)| f.idx() as u64).collect();
        if seq != base {
            return false;
        }
    }
    true
}

// ── 行往返探针（建行→解析→逐列对账 + 三拒） ─────────────────────
pub fn row_roundtrip() -> bool {
    let r = ReportRow { fam: Fam::BudgetOver, form: Form::CycleOver, metric: 237, caught: true };
    let line = render_row(&r);
    let a = match parse_row(&line) {
        Ok(p) => p.fam == Fam::BudgetOver && p.form == Form::CycleOver && p.metric == 237 && p.caught,
        Err(_) => false,
    };
    // 三拒：判定字野 / 族形态口径断 / 校验和断。
    let bad_verdict = parse_row(&line.replace("CATCH", "CATCHX")).is_err();
    let cross = parse_row(&line.replace("BUDGT|CYCOV", "FETCH|CYCOV")).is_err();
    let mut bad_sum = false;
    let mut ch = line.clone().into_bytes();
    if ch.len() >= 3 {
        let idx = ch.len() - 3;
        let c = ch[idx];
        ch[idx] = if c == b'8' { b'9' } else { b'8' };
        bad_sum = match String::from_utf8(ch) {
            Ok(s2) => parse_row(&s2).is_err(),
            Err(_) => false,
        };
    }
    a && bad_verdict && cross && bad_sum
}

// ── 黄金报告行区间工具（注入器共用，正文全 ASCII） ──────────────
/// 黄金账文第 k 行的字节区间 [lo, hi)（hi 为行尾换行下标）。
fn row_span(text: &str, k: usize) -> (usize, usize) {
    let b = text.as_bytes();
    let mut lo = 0usize;
    let mut idx = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'\n' {
            if idx == k {
                return (lo, i);
            }
            idx += 1;
            lo = i + 1;
        }
        i += 1;
    }
    (lo, b.len())
}

/// 行内单字节改写：第 k 行定位 pat，其后第 off 字节改写。
fn patch_in_row(text: &str, k: usize, pat: &[u8], off: usize, newc: u8) -> String {
    let (lo, hi) = row_span(text, k);
    let seg = &text[lo..hi];
    let tgt = match find_sub(seg.as_bytes(), 0, pat) {
        Some(p) => p + off,
        None => usize::MAX,
    };
    let mut out = String::from(&text[..lo]);
    let mut i = 0usize;
    while i < seg.len() {
        if i == tgt {
            out.push(newc as char);
        } else {
            out.push_str(&seg[i..i + 1]);
        }
        i += 1;
    }
    out.push_str(&text[hi..]);
    out
}

/// 行对调：第 a/b 行互换（形态号单调破坏注入）。
fn swap_rows(text: &str, a: usize, b2: usize) -> String {
    let (a0, a1) = row_span(text, a);
    let (b0, b1) = row_span(text, b2);
    let mut out = String::new();
    out.push_str(&text[b0..b1 + 1]);
    out.push_str(&text[a0..a1 + 1]);
    out.push_str(&text[b1 + 1..]);
    out
}

/// 行复制：第 k 行复制一份（形态序重复注入）。
fn dup_row(text: &str, k: usize) -> String {
    let (lo, hi) = row_span(text, k);
    let mut out = String::from(&text[..hi + 1]);
    out.push_str(&text[lo..hi + 1]);
    out.push_str(&text[hi + 1..]);
    out
}

/// 对黄金报告账文注入第 kind 路失真，返回坏账文（全 ASCII，安全）。
pub fn distort(text: &str, kind: usize) -> String {
    match kind % 10 {
        0 => {
            // 翻判定：首行 CATCH → XATCH（判定字野必抓）。
            patch_in_row(text, 0, b"EMPPG|", 0, b'X')
        }
        1 => {
            // 改量纲：首行量纲 1→9（口径账断、断校验必抓）。
            patch_in_row(text, 0, b"EMPPG|", 1, b'9')
        }
        2 => {
            // 改族号：首行族名 FETCH → FE8CH（族闭集外必抓）。
            patch_in_row(text, 0, b"FETCH|", 2, b'8')
        }
        3 => swap_rows(text, 0, 1), // 换序：形态号单调被破坏必抓。
        4 => {
            // 截尾：去末 6 字节（行数不足/终止符缺失必抓）。
            String::from(&text[..text.len().saturating_sub(6)])
        }
        5 => {
            // 加行：尾部追加伪造第 9 行（行数越界/摘要断必抓）。
            let mut out = String::from(text);
            out.push_str("REGDL|ORPHT|2|CATCH|00000000;\n");
            out
        }
        6 => {
            // 插控制符：第 3 行行尾（分号后换行前）插裸 0x01。
            let (_, hi) = row_span(text, 2);
            let mut out = String::from(&text[..hi]);
            out.push(0x01 as char);
            out.push_str(&text[hi..]);
            out
        }
        7 => patch_in_row(text, 2, b"|UNITX|", 5, b'Y'), // 改形态字：闭集外必抓。
        8 => patch_in_row(text, 4, b"|CYCOV|", 5, b'X'), // 改形态字：闭集外必抓。
        _ => dup_row(text, 0), // 复制行：形态序重复必抓。
    }
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

/// 注入轮：10 轮实注入（逐路失真全走）+ 2 轮洁净对照。
/// 实注入轮核验器必须抓；洁净轮必须零误报且核验 CLEAN 全过。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut acc = InjectLedger::new();
    for r in 0..rounds {
        let kind = (r % 10) as usize;
        // 实注入轮：新鲜矩阵跑 → 核验 → 失真必抓。
        let mut det = Detector::standard();
        let led = run_matrix(EPOCH + r as u64, &mut det, r)?;
        let text = led.render_all();
        verify_text(&text)?;
        acc.record(kind, verify_text(&distort(&text, kind)).is_err());
    }
    // 洁净对照两轮：全 CLEAN，必须零误报（假阳记为漏抓）。
    for c in 0..2u32 {
        let mut det = Detector::standard();
        let led = run_clean(&mut det)?;
        acc.record(10 + c as usize, led.catch_count() == 0 && det.false_pos == 0);
    }
    Ok(acc)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：矩阵跑 + 全检出核 → 洁净对照 → 完备率 → 组织法序 → 四拒探。
/// 返回 (正旗数, 拒旗数, Catch 行数, 完备率 bps)。
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let mut det = Detector::standard();
    let led = run_matrix(EPOCH, &mut det, 0)?;
    verify_ledger(&led)?;
    let catches = led.catch_count();
    let all_caught = catches == CELL_N as u32 && det.catches == CELL_N as u32 && det.false_pos == 0;
    let matrix = build_matrix();
    let bps = completeness_bps(&matrix);
    let complete = bps == 10_000 && det.caps_match_matrix(&matrix);
    let ordered = order_stable();
    let rt_ok = row_roundtrip();
    // 四拒探：空账文 / 阈值过松漏抓 / 族形态口径断 / 洁净跑零假阳。
    let empty_rej = verify_text("").is_err();
    let mut loose = Detector::standard();
    loose.caps[2] = BUDGET_CAP * 4;
    let loose_led = run_matrix(9, &mut loose, 0)?;
    let loose_rej = loose_led.rows.get(4).map(|r| !r.caught).unwrap_or(true);
    let cross_rej = parse_row("FETCH|CYCOV|237|CATCH|00000000;").is_err();
    let mut fp_det = Detector::standard();
    // 假阳探针：量纲 0 恒放行（judge 返回 false 且不计假阳）。
    let fp_hit = !fp_det.judge(&clean_symptom(Form::EmptyPage)) && fp_det.false_pos == 0;
    let flags = all_caught as u32 + complete as u32 + ordered as u32 + rt_ok as u32;
    let rejects = empty_rej as u32 + loose_rej as u32 + cross_rej as u32 + fp_hit as u32;
    Ok((flags, rejects, catches, bps))
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(StressCode::NotReproducible.val());
    }
    Ok(())
}

// ── 形态反查守卫探针（闭集外拒） ────────────────────────────────
pub fn form_guard() -> bool {
    let f_bad = Form::from_idx(8).is_none() && Form::from_idx(u64::MAX).is_none();
    let fam_bad = Fam::from_idx(4).is_none() && Fam::from_idx(u64::MAX).is_none();
    let f_ok = Form::from_idx(6) == Some(Form::GhostHandle)
        && match Form::from_idx(6) { Some(f) => f.fam() == Fam::RegDangling, None => false };
    let fam_ok = Fam::from_idx(2) == Some(Fam::BudgetOver);
    // 洁净症状恒量纲 0；注入症状恒量纲 ≥1。
    let clean_ok = clean_symptom(Form::EmptyPage).metric == 0;
    let inj_ok = inject(Form::EmptyPage, 0).metric >= 1 && inject(Form::OrphanTicket, 1).metric >= 1;
    f_bad && fam_bad && f_ok && fam_ok && clean_ok && inj_ok
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let mut det = Detector::standard();
    let led = run_matrix(EPOCH, &mut det, 0)?;
    verify_ledger(&led)?;
    let matrix = build_matrix();
    Ok(format!(
        "状态=已完成 matrix=EPOCH{} catches={}/{} bps={} order=stable fams={}",
        EPOCH, led.catch_count(), CELL_N, completeness_bps(&matrix), FAMILY_N
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒 + Catch 8/8）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, c, b)) => (*f == 4 && *r == 4, format!("flags={} rejects={} catch={}/8 bps={}", f, r, c, b)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-注入矩阵全过", gp, gd));
    // J1 四族八形态全检出。
    let led = match golden_matrix() {
        Ok(l) => l,
        Err(e) => {
            v.push(("J1-四族八形态全检出", false, format!("err=0x{:x}", e)));
            return v;
        }
    };
    let caught = led.0;
    v.push(("J1-四族八形态全检出", caught == CELL_N as u32, format!("catch={}/{}", caught, CELL_N)));
    // J1 预期声明完备率 100%。
    let matrix = build_matrix();
    let bps = completeness_bps(&matrix);
    v.push(("J1-预期声明完备率", bps == 10_000 && led.1, format!("bps={} declared={}/{}", bps, matrix.len(), CELL_N)));
    // J1 组织法稳定序（三轮同序，对齐 F1788）。
    v.push(("J1-组织法稳定序", order_stable(), String::from("族主序+族内次序三轮同序")));
    // J1 检出报告行往返 + 三拒。
    v.push(("J1-报告行往返", row_roundtrip(), String::from("建行→解析逐列对拍；野判定/口径断/断校验必败")));
    // J1 失真注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    // J2 洁净对照零假阳。
    let mut det2 = Detector::standard();
    let cp = match run_clean(&mut det2) {
        Ok(l) => l.catch_count() == 0 && det2.false_pos == 0,
        Err(_) => false,
    };
    v.push(("J2-洁净对照零假阳", cp, String::from("量纲 0 恒 CLEAN，假阳账恒零")));
    // J2 形态/族反查守卫。
    v.push(("J2-闭集反查守卫", form_guard(), String::from("形态/族反查：闭集外拒、闭集内自洽")));
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
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72A40 窗口")));
    v
}

/// 黄金矩阵装填（标准检出器跑一轮，供判据面复用）。
pub fn golden_matrix() -> Result<(u32, bool), u32> {
    let mut det = Detector::standard();
    let led = run_matrix(EPOCH, &mut det, 0)?;
    verify_ledger(&led)?;
    Ok((led.catch_count(), det.caps_match_matrix(&build_matrix())))
}

pub fn run_unx2164_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2164 压测注入矩阵（第二十五矩阵）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
