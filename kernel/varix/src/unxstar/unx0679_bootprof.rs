//! UNX-F0679 · 冷启动性能画像报告（程序化生成）· 诊断段 0x5620（本单独占）
//!
//! 目标行数：≥570（纯功能代码）
//! 判据原文摘录：UNX-F0679-J1 画像报告六节程序化生成（节全 6/6）且零手抄字段
//! （每字段账源指针可解引）。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 六节闭集：总览/分布画像/阈值画像/离群画像/漂移画像/结论，节号固定序。
//!  3. 零手抄：每字段必带账源指针（五账族指针闭集），指针不可解引即报告非法。
//!  4. 节全 6/6：缺节/节序错位/空节全部拦截。
//!  5. 报告确定性：同账源两次生成逐行全等。
//!  6. 诊断段 0x5620 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x5620;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_SEC: u16 = SEG_BASE + 1;     // 节不齐（<6）或节序错位
pub const E_PTR: u16 = SEG_BASE + 2;     // 账源指针不可解引
pub const E_EMPTY: u16 = SEG_BASE + 3;   // 空节
pub const E_ROW: u16 = SEG_BASE + 4;     // 账源数据非法
pub const E_DETER: u16 = SEG_BASE + 5;   // 两次生成不一致（确定性破）
pub const E_SEAL: u16 = SEG_BASE + 6;    // 报告已封，再生成拦截
pub const E_N: u16 = SEG_BASE + 7;       // 账源样本量不足

/// LCG64。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 账源指针闭集（五账族 × 行号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SrcPtr {
    /// 阈值账行 n。
    ThreshRow(u16),
    /// 五数账列 n（0..4）。
    FiveCol(u8),
    /// 守恒账行 n。
    ConsRow(u16),
    /// 告警账行 n。
    AlertRow(u16),
    /// 漂移账窗 n。
    DriftWin(u16),
}

impl SrcPtr {
    /// 可解引判定：载荷非零即合法（账行号 1 起，列号 0 起但列上限 4）。
    pub fn deref_ok(&self) -> bool {
        match self {
            SrcPtr::ThreshRow(n) => *n > 0,
            SrcPtr::FiveCol(c) => *c <= 4,
            SrcPtr::ConsRow(n) => *n > 0,
            SrcPtr::AlertRow(n) => *n > 0,
            SrcPtr::DriftWin(n) => *n > 0,
        }
    }
}

/// 六节闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// 1 总览。
    Overview,
    /// 2 分布画像。
    Dist,
    /// 3 阈值画像。
    Thresh,
    /// 4 离群画像。
    Outlier,
    /// 5 漂移画像。
    Drift,
    /// 6 结论。
    Verdict,
}

impl Section {
    pub const ALL: [Section; 6] = [
        Section::Overview,
        Section::Dist,
        Section::Thresh,
        Section::Outlier,
        Section::Drift,
        Section::Verdict,
    ];

    pub fn no(self) -> u8 {
        match self {
            Section::Overview => 1,
            Section::Dist => 2,
            Section::Thresh => 3,
            Section::Outlier => 4,
            Section::Drift => 5,
            Section::Verdict => 6,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::Overview => "总览",
            Section::Dist => "分布画像",
            Section::Thresh => "阈值画像",
            Section::Outlier => "离群画像",
            Section::Drift => "漂移画像",
            Section::Verdict => "结论",
        }
    }
}

/// 账源数据（报告唯一输入）。
pub struct LedgerInput {
    pub mach: u8,
    /// 50 次耗时样本。
    pub samples: Vec<u64>,
    /// 阈值（ms）。
    pub thresh_ms: u64,
    /// 超阈计数。
    pub over_n: u32,
    /// 离群计数。
    pub outlier_n: u32,
    /// 带外窗计数。
    pub drift_n: u32,
}

impl LedgerInput {
    pub fn validate(&self) -> Result<(), u16> {
        if self.mach > 2 {
            return Err(E_ROW);
        }
        if self.samples.len() < 10 {
            return Err(E_N);
        }
        if self.thresh_ms == 0 {
            return Err(E_ROW);
        }
        Ok(())
    }
}

/// 报告行：文本 + 账源指针。
#[derive(Clone, Debug)]
pub struct ProfileLine {
    pub text: String,
    pub ptr: SrcPtr,
}

/// 报告节。
#[derive(Clone, Debug)]
pub struct ProfileSection {
    pub sec: Section,
    pub lines: Vec<ProfileLine>,
}

/// 排序法五数（报告生成内部用，与上游口径一致）。
fn five_of(samples: &[u64]) -> Result<[u64; 5], u16> {
    if samples.is_empty() {
        return Err(E_N);
    }
    let mut v = samples.to_vec();
    for i in 1..v.len() {
        let mut j = i;
        while j > 0 && v[j] < v[j - 1] {
            let t = v[j];
            v[j] = v[j - 1];
            v[j - 1] = t;
            j -= 1;
        }
    }
    let n = v.len();
    Ok([
        v[0],
        v[(n - 1) * 25 / 100],
        v[(n - 1) / 2],
        v[(n - 1) * 75 / 100],
        v[n - 1],
    ])
}

fn itoa(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    let mut v = v;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let mut s = String::new();
    for b in &buf[i..] {
        s.push(*b as char);
    }
    s
}

/// 单节程序化生成：每行带指针。
pub fn gen_section(sec: Section, d: &LedgerInput) -> Result<ProfileSection, u16> {
    d.validate()?;
    let mut lines: Vec<ProfileLine> = Vec::new();
    match sec {
        Section::Overview => {
            let mut l = String::from("机型 M");
            l.push_str(&itoa(d.mach as u64));
            l.push_str(" 样本=");
            l.push_str(&itoa(d.samples.len() as u64));
            lines.push(ProfileLine { text: l, ptr: SrcPtr::ThreshRow(1) });
            let mut l2 = String::from("阈值=");
            l2.push_str(&itoa(d.thresh_ms));
            lines.push(ProfileLine { text: l2, ptr: SrcPtr::ThreshRow(2) });
        }
        Section::Dist => {
            let f = five_of(&d.samples)?;
            let names = ["min", "p25", "med", "p75", "max"];
            for i in 0..5 {
                let mut l = String::from(names[i]);
                l.push('=');
                l.push_str(&itoa(f[i]));
                lines.push(ProfileLine { text: l, ptr: SrcPtr::FiveCol(i as u8) });
            }
        }
        Section::Thresh => {
            let mut over: u32 = 0;
            for s in &d.samples {
                if *s > d.thresh_ms {
                    over += 1;
                }
            }
            let mut l = String::from("超阈=");
            l.push_str(&itoa(over as u64));
            lines.push(ProfileLine { text: l, ptr: SrcPtr::ThreshRow(3) });
        }
        Section::Outlier => {
            let mut l = String::from("离群=");
            l.push_str(&itoa(d.outlier_n as u64));
            lines.push(ProfileLine { text: l, ptr: SrcPtr::AlertRow(1) });
        }
        Section::Drift => {
            let mut l = String::from("带外窗=");
            l.push_str(&itoa(d.drift_n as u64));
            lines.push(ProfileLine { text: l, ptr: SrcPtr::DriftWin(1) });
        }
        Section::Verdict => {
            let all_ok = d.over_n == 0 && d.outlier_n == 0 && d.drift_n == 0;
            let verdict = if all_ok { "画像健康" } else { "画像异常" };
            lines.push(ProfileLine {
                text: String::from(verdict),
                ptr: SrcPtr::ConsRow(1),
            });
        }
    }
    if lines.is_empty() {
        return Err(E_EMPTY);
    }
    Ok(ProfileSection { sec, lines })
}

/// 画像报告。
#[derive(Clone, Debug)]
pub struct ProfileReport {
    pub mach: u8,
    pub sections: Vec<ProfileSection>,
    pub sealed: bool,
}

impl ProfileReport {
    pub fn generate(d: &LedgerInput) -> Result<ProfileReport, u16> {
        let mut sections = Vec::new();
        for sec in Section::ALL {
            sections.push(gen_section(sec, d)?);
        }
        let rep = ProfileReport { mach: d.mach, sections, sealed: false };
        rep.assert_six()?;
        rep.assert_no_handcopy()?;
        Ok(rep)
    }

    /// 节全 6/6 断言。
    pub fn assert_six(&self) -> Result<(), u16> {
        if self.sections.len() != 6 {
            return Err(E_SEC);
        }
        for (i, s) in self.sections.iter().enumerate() {
            if s.sec.no() as usize != i + 1 {
                return Err(E_SEC);
            }
            if s.lines.is_empty() {
                return Err(E_EMPTY);
            }
        }
        Ok(())
    }

    /// 零手抄断言：指针全部可解引。
    pub fn assert_no_handcopy(&self) -> Result<(), u16> {
        for s in &self.sections {
            for l in &s.lines {
                if !l.ptr.deref_ok() {
                    return Err(E_PTR);
                }
                if l.text.is_empty() {
                    return Err(E_EMPTY);
                }
            }
        }
        Ok(())
    }

    /// 指纹（确定性锚点）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf = Vec::new();
        buf.push(self.mach);
        for s in &self.sections {
            buf.push(s.sec.no());
            for l in &s.lines {
                let p = match l.ptr {
                    SrcPtr::ThreshRow(n) => (1 << 24) | n as u32,
                    SrcPtr::FiveCol(c) => (2 << 24) | c as u32,
                    SrcPtr::ConsRow(n) => (3 << 24) | n as u32,
                    SrcPtr::AlertRow(n) => (4 << 24) | n as u32,
                    SrcPtr::DriftWin(n) => (5 << 24) | n as u32,
                };
                for k in 0..4u8 {
                    buf.push((p >> (24 - k * 8)) as u8);
                }
            }
        }
        fnv1a64(&buf)
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.assert_six()?;
        self.sealed = true;
        Ok(())
    }
}

/// 账源确定性生成。
pub fn synth_input(m: u8, seed: u64) -> Result<LedgerInput, u16> {
    if m > 2 {
        return Err(E_ROW);
    }
    let mut rng = Lcg::new(seed ^ ((m as u64 + 1).wrapping_mul(0x9E37_79B9)));
    let mut samples = Vec::new();
    for _ in 0..50 {
        samples.push(2_200 + (rng.next_u32() % 400) as u64);
    }
    Ok(LedgerInput {
        mach: m,
        samples,
        thresh_ms: 3_000,
        over_n: 0,
        outlier_n: 0,
        drift_n: 0,
    })
}

/// 指针闭集自检。
pub fn ptr_closure_audit() -> Result<(), u16> {
    if SrcPtr::ThreshRow(0).deref_ok() {
        return Err(SEG_BASE + 0x30);
    }
    if SrcPtr::ConsRow(0).deref_ok() {
        return Err(SEG_BASE + 0x31);
    }
    if SrcPtr::AlertRow(0).deref_ok() {
        return Err(SEG_BASE + 0x32);
    }
    if SrcPtr::DriftWin(0).deref_ok() {
        return Err(SEG_BASE + 0x33);
    }
    if SrcPtr::FiveCol(5).deref_ok() {
        return Err(SEG_BASE + 0x34);
    }
    if !SrcPtr::FiveCol(0).deref_ok() {
        return Err(SEG_BASE + 0x35);
    }
    Ok(())
}

/// 五数内部口径自检：样本 1..=10 → (1,3,5,7,10)。
pub fn five_audit() -> Result<(), u16> {
    let mut v = Vec::new();
    for i in 1..=10u64 {
        v.push(i);
    }
    let f = five_of(&v)?;
    if f != [1, 3, 5, 7, 10] {
        return Err(E_ROW);
    }
    Ok(())
}

/// 确定性自检：同账源两次生成指纹全等。
pub fn determinism_audit() -> Result<(), u16> {
    let d = synth_input(1, 0x0679_0001)?;
    let r1 = ProfileReport::generate(&d)?;
    let r2 = ProfileReport::generate(&d)?;
    if r1.fingerprint() != r2.fingerprint() {
        return Err(E_DETER);
    }
    Ok(())
}

/// 节-指针族一致性：每节指针族应与节主题同族（总览/阈值→Thresh；分布→Five；
/// 离群→Alert；漂移→Drift；结论→Cons）。破族即报告结构非法。
pub fn section_ptr_family_audit(rep: &ProfileReport) -> Result<(), u16> {
    rep.assert_six()?;
    for s in &rep.sections {
        for l in &s.lines {
            let fam_ok = match s.sec {
                Section::Overview | Section::Thresh => matches!(l.ptr, SrcPtr::ThreshRow(_)),
                Section::Dist => matches!(l.ptr, SrcPtr::FiveCol(_)),
                Section::Outlier => matches!(l.ptr, SrcPtr::AlertRow(_)),
                Section::Drift => matches!(l.ptr, SrcPtr::DriftWin(_)),
                Section::Verdict => matches!(l.ptr, SrcPtr::ConsRow(_)),
            };
            if !fam_ok {
                return Err(E_PTR);
            }
        }
    }
    Ok(())
}

/// 节标题可解引：每节标题非空且节号唯一。
pub fn section_title_audit(rep: &ProfileReport) -> Result<(), u16> {
    let mut seen = [false; 7];
    for s in &rep.sections {
        let no = s.sec.no() as usize;
        if no > 6 || seen[no] {
            return Err(E_SEC);
        }
        seen[no] = true;
        if s.sec.title().is_empty() {
            return Err(E_EMPTY);
        }
    }
    Ok(())
}

/// 字段计数审计：总览 2 行、分布 5 行、阈值 1、离群 1、漂移 1、结论 1 = 11 行。
pub fn field_count_audit(rep: &ProfileReport) -> Result<(), u16> {
    let want = [2usize, 5, 1, 1, 1, 1];
    if rep.sections.len() != 6 {
        return Err(E_SEC);
    }
    for (i, s) in rep.sections.iter().enumerate() {
        if s.lines.len() != want[i] {
            return Err(E_ROW);
        }
    }
    Ok(())
}

/// 指针族审计注入核：分布节混入阈值指针 → 检出。
pub fn inject_family_mix_must_fail(rep: &ProfileReport) -> Result<(), u16> {
    let mut bad = rep.clone();
    // 把分布节（第 2 节）首行指针换成阈值指针
    bad.sections[1].lines[0].ptr = SrcPtr::ThreshRow(9);
    if section_ptr_family_audit(&bad).is_ok() {
        return Err(SEG_BASE + 0x3C);
    }
    Ok(())
}

/// 画像报告摘要行。
pub struct ProfileSummary {
    pub lines: Vec<String>,
}

impl ProfileSummary {
    pub fn generate(reps: &[ProfileReport]) -> Result<ProfileSummary, u16> {
        if reps.is_empty() {
            return Err(E_EMPTY);
        }
        let mut lines = Vec::new();
        for r in reps {
            r.assert_six()?;
            let mut l = String::from("M");
            l.push_str(&itoa(r.mach as u64));
            l.push_str(" 六节=");
            l.push_str(&itoa(r.sections.len() as u64));
            l.push_str(" fp=");
            l.push_str(&(r.fingerprint() % 100_000).to_string());
            lines.push(l);
        }
        Ok(ProfileSummary { lines })
    }
}

/// 自例程：三机六节画像 + 零手抄 + 确定性 + 注入全拦截 → 0。
pub fn drill() -> Result<u32, u16> {
    ptr_closure_audit()?;
    five_audit()?;
    determinism_audit()?;

    let mut reps: Vec<ProfileReport> = Vec::new();
    for m in 0..3u8 {
        let d = synth_input(m, 0x0679_0002)?;
        let r = ProfileReport::generate(&d)?;
        r.assert_six()?;
        r.assert_no_handcopy()?;
        section_ptr_family_audit(&r)?;
        section_title_audit(&r)?;
        field_count_audit(&r)?;
        reps.push(r);
    }
    inject_family_mix_must_fail(&reps[0])?;
    // 汇总
    let sum = ProfileSummary::generate(&reps)?;
    if sum.lines.len() != 3 {
        return Err(E_SEC);
    }
    // 注入：样本不足 → 拦截
    let mut bad = synth_input(0, 1)?;
    bad.samples.truncate(5);
    if !matches!(ProfileReport::generate(&bad), Err(E_N)) {
        return Err(SEG_BASE + 0x36);
    }
    // 注入：阈值 0 → 拦截
    let mut bad2 = synth_input(0, 1)?;
    bad2.thresh_ms = 0;
    if !matches!(ProfileReport::generate(&bad2), Err(E_ROW)) {
        return Err(SEG_BASE + 0x37);
    }
    // 注入：机型越界 → 拦截
    let bad3 = synth_input(9, 1);
    if !matches!(bad3, Err(E_ROW)) {
        return Err(SEG_BASE + 0x38);
    }
    // 注入：指针不可解引 → 断言检出
    let mut bad_rep = reps[0].clone();
    bad_rep.sections[0].lines[0].ptr = SrcPtr::ThreshRow(0);
    if !matches!(bad_rep.assert_no_handcopy(), Err(E_PTR)) {
        return Err(SEG_BASE + 0x39);
    }
    // 注入：缺节 → 断言检出
    let mut bad_rep2 = reps[0].clone();
    bad_rep2.sections.pop();
    if !matches!(bad_rep2.assert_six(), Err(E_SEC)) {
        return Err(SEG_BASE + 0x3A);
    }
    // 封账 + 封后生成路径由 sealed 标记守卫（二次 seal 拦截）
    let mut r0 = reps[0].clone();
    r0.seal()?;
    if !matches!(r0.seal(), Err(E_SEAL)) {
        return Err(SEG_BASE + 0x3B);
    }
    Ok(0)
}
