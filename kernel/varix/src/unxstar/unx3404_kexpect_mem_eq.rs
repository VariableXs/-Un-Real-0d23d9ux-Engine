//! UNX-F3404 kexpect_mem_eq（内存块比较断言） · 0x79440段
//!
//! 判据原文（UNX-F3404 派工卡逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② J1：内存块等值断言（首差异偏移报告）；M-163 差异定位验证。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 等值断言核：kexpect_mem_eq(a, b) 逐字节比对，等值返回 Pass，不等返回
//!    首差异报告（偏移 + 期望字节 + 实际字节），不 panic 不越界（取值一律
//!    .get()，长度不齐按「长度先异」报告）；
//! 2. 首差异偏移报告：DiffReport 带偏移/两侧字节/块长四元信息，偏移即定位；
//!    换位比对（b,a）报告的偏移必须与原序一致（对称性判据）；
//! 3. M-163 差异定位验证：语料按「差在首字节/中段/末字节/长度不齐/多差异/
//!    全等/双空」七档构造，逐档首差异偏移必须与钦定值一致；
//! 4. 差异计数面：窗口内差异总数扫描（偏移越窗即停），计数与逐档语料钦定
//!    值对账；零差异块对账必为零；
//! 5. 注入矩阵 8 项：改首字节/改中段/改末字节/长度截短/长度伸长/双空互报/
//!    对称翻转/报告偏移漂移逐种构造，逐种必须被口点出；
//! 6. 诊断码闭集 0x79441..0x7944C，Err 用 Result<T,u32>，中文诊断文案。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B10 给定值；本批窗口 0x79400..=0x795C0）。
pub const SEG_BASE: u32 = 0x79440;

/// M-163 语料档数。
pub const M163_CASES: usize = 7;

/// 差异计数扫描窗口（首差异之后仍允许计数的最远跨度）。
pub const DIFF_WINDOW: usize = 64;

/// 内存块比较诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 首差异：两块在报告偏移处字节不等。
    EFirstDiff,
    /// 长度不齐：两块长度不等，长度先异按首差异报告。
    ELenMismatch,
    /// 空块：两块皆空不构成比较对象（显性拒，不静默过）。
    EBothEmpty,
    /// 偏移越界：报告偏移超出任一块长（报告自身违约）。
    EOffsetRange,
    /// 偏移漂移：重跑同语料报告偏移漂移（非确定）。
    EOffsetDrift,
    /// 对称破缺：换位比对偏移与原序不一致。
    EAsymmetry,
    /// 差异计数对账破：扫描计数与钦定值不符。
    ECountLedger,
    /// 等值误报：不等块被判等。
    EFalseEqual,
    /// 不等误报：等值块被判不等。
    EFalseDiff,
    /// 字节档缺失：M-163 语料档未齐。
    ELackCase,
    /// 报告字段空：报告缺偏移或两侧字节。
    EEmptyReport,
    /// 相位倒退：比较器被复用后重置倒退被拒。
    EPhase,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EFirstDiff => 0x01,
                Diag::ELenMismatch => 0x02,
                Diag::EBothEmpty => 0x03,
                Diag::EOffsetRange => 0x04,
                Diag::EOffsetDrift => 0x05,
                Diag::EAsymmetry => 0x06,
                Diag::ECountLedger => 0x07,
                Diag::EFalseEqual => 0x08,
                Diag::EFalseDiff => 0x09,
                Diag::ELackCase => 0x0a,
                Diag::EEmptyReport => 0x0b,
                Diag::EPhase => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EFirstDiff => "首差异：按报告偏移核对两侧字节，先修期望块再复跑",
            Diag::ELenMismatch => "长度不齐：两块长度不等，先对齐块长再做等值断言",
            Diag::EBothEmpty => "双空块：无内容可比，先备齐比较块再断言",
            Diag::EOffsetRange => "偏移越界：报告偏移超出块长，比较器内部违约，回报修者",
            Diag::EOffsetDrift => "偏移漂移：同语料两次报告偏移不一致，比较器非确定，回报修者",
            Diag::EAsymmetry => "对称破缺：换位比对偏移不一致，比较器有方向性缺陷，回报修者",
            Diag::ECountLedger => "计数对账破：窗口内差异计数与钦定值不符，逐档重扫定位",
            Diag::EFalseEqual => "等值误报：块内容不等却被判等，比较核违约，回报修者",
            Diag::EFalseDiff => "不等误报：块内容相等却被判不等，比较核违约，回报修者",
            Diag::ELackCase => "语料档缺：M-163 七档未齐，补档后再验证",
            Diag::EEmptyReport => "报告字段空：差异报告缺偏移或字节，补齐报告再上账",
            Diag::EPhase => "相位倒退：比较器复位后不许沿用旧账，重置即清账",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EFirstDiff,
    Diag::ELenMismatch,
    Diag::EBothEmpty,
    Diag::EOffsetRange,
    Diag::EOffsetDrift,
    Diag::EAsymmetry,
    Diag::ECountLedger,
    Diag::EFalseEqual,
    Diag::EFalseDiff,
    Diag::ELackCase,
    Diag::EEmptyReport,
    Diag::EPhase,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79441..0x7944C 闭集内"
}

/// 比较结论相位（单向：待比→已比）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 待比：比较器就绪未用。
    Ready,
    /// 已比：结论已落定。
    Compared,
}

impl Phase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Ready => 0,
            Phase::Compared => 1,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Phase::Ready => "待比",
            Phase::Compared => "已比",
        }
    }
}

/// 等值断言结论：Pass 或首差异报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemEqRes {
    /// 等值：逐字节全同且长度齐。
    Pass,
    /// 不等：首差异报告（偏移 + 期望字节 + 实际字节 + 两侧块长）。
    Diff {
        /// 首差异偏移（0 起）。
        offset: usize,
        /// 期望块（a 块）在该偏移的字节。
        want: u8,
        /// 实际块（b 块）在该偏移的字节（长度不齐侧缺字节记 0）。
        got: u8,
        /// a 块长度。
        len_a: usize,
        /// b 块长度。
        len_b: usize,
    },
}

impl MemEqRes {
    /// 是否等值。
    pub fn ok(&self) -> bool {
        matches!(self, MemEqRes::Pass)
    }

    /// 首差异偏移（Pass 返回 None）。
    pub fn offset(&self) -> Option<usize> {
        match self {
            MemEqRes::Pass => None,
            MemEqRes::Diff { offset, .. } => Some(*offset),
        }
    }

    /// 报告字段完整性（Diff 必带偏移与两侧块长，且偏移在两块长并集内）。
    pub fn wellformed(&self) -> bool {
        match self {
            MemEqRes::Pass => true,
            MemEqRes::Diff {
                offset,
                len_a,
                len_b,
                ..
            } => {
                if *len_a == 0 && *len_b == 0 {
                    return false;
                }
                let max_len = if *len_a >= *len_b { *len_a } else { *len_b };
                *offset < max_len
            }
        }
    }

    /// 单行报告文本（中文，含偏移与两侧字节——写回面非空）。
    pub fn report_line(&self) -> String {
        match self {
            MemEqRes::Pass => String::from("内存块等值：逐字节全同"),
            MemEqRes::Diff {
                offset,
                want,
                got,
                len_a,
                len_b,
            } => format!(
                "首差异偏移{} 期望{:02x} 实际{:02x} 块长{}/{}",
                offset, want, got, len_a, len_b
            ),
        }
    }
}

/// 内存块等值断言核：逐字节 .get() 比对，首差异即报。
///
/// 空块对空块显性拒（无内容可比）；长度不齐时差异偏移取
/// min(len_a, len_b)（长度先异按首差异报告）。
pub fn kexpect_mem_eq(a: &[u8], b: &[u8]) -> Result<MemEqRes, u32> {
    if a.is_empty() && b.is_empty() {
        return Err(Diag::EBothEmpty.val());
    }
    let mut i = 0usize;
    let shorter = if a.len() <= b.len() { a.len() } else { b.len() };
    while i < shorter {
        let (wa, wb) = match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => (*x, *y),
            _ => return Err(Diag::EOffsetRange.val()),
        };
        if wa != wb {
            return Ok(MemEqRes::Diff {
                offset: i,
                want: wa,
                got: wb,
                len_a: a.len(),
                len_b: b.len(),
            });
        }
        i += 1;
    }
    if a.len() != b.len() {
        // 前缀全同但长度不齐：长度先异位次即 min 长度处。
        let wa = a.get(shorter).copied().unwrap_or(0);
        let wb = b.get(shorter).copied().unwrap_or(0);
        return Ok(MemEqRes::Diff {
            offset: shorter,
            want: wa,
            got: wb,
            len_a: a.len(),
            len_b: b.len(),
        });
    }
    Ok(MemEqRes::Pass)
}

/// 差异计数：窗口内（0..min(len, DIFF_WINDOW)）逐字节差异总数。
pub fn diff_count(a: &[u8], b: &[u8]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < DIFF_WINDOW {
        let (wa, wb) = match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => (Some(*x), Some(*y)),
            (Some(x), None) => (Some(*x), None),
            (None, Some(y)) => (None, Some(*y)),
            (None, None) => break,
        };
        if wa != wb {
            n += 1;
        }
        i += 1;
    }
    n
}

/// M-163 语料档：七档（全等/首字节异/中段异/末字节异/前缀长度不齐/多差异/双空）。
#[derive(Clone, Debug)]
pub struct M163Case {
    /// 档名。
    pub name: &'static str,
    /// a 块。
    pub a: Vec<u8>,
    /// b 块。
    pub b: Vec<u8>,
    /// 钦定首差异偏移（None=全等；双空档为 Err 面）。
    pub want_offset: Option<usize>,
    /// 钦定窗口差异计数（双空档记 0）。
    pub want_count: usize,
}

/// 构造 M-163 七档语料（块长 8，中段=4，末字节=7）。
pub fn m163_corpus() -> Vec<M163Case> {
    let mut out = Vec::new();
    // 档1：全等。
    out.push(M163Case {
        name: "全等",
        a: vec![0, 1, 2, 3, 4, 5, 6, 7],
        b: vec![0, 1, 2, 3, 4, 5, 6, 7],
        want_offset: None,
        want_count: 0,
    });
    // 档2：首字节异。
    out.push(M163Case {
        name: "首字节异",
        a: vec![0, 1, 2, 3, 4, 5, 6, 7],
        b: vec![9, 1, 2, 3, 4, 5, 6, 7],
        want_offset: Some(0),
        want_count: 1,
    });
    // 档3：中段异。
    out.push(M163Case {
        name: "中段异",
        a: vec![0, 1, 2, 3, 4, 5, 6, 7],
        b: vec![0, 1, 2, 3, 9, 5, 6, 7],
        want_offset: Some(4),
        want_count: 1,
    });
    // 档4：末字节异。
    out.push(M163Case {
        name: "末字节异",
        a: vec![0, 1, 2, 3, 4, 5, 6, 7],
        b: vec![0, 1, 2, 3, 4, 5, 6, 9],
        want_offset: Some(7),
        want_count: 1,
    });
    // 档5：前缀长度不齐（b 截短 3 字节，前缀全同）。
    out.push(M163Case {
        name: "长度不齐",
        a: vec![0, 1, 2, 3, 4, 5, 6, 7],
        b: vec![0, 1, 2],
        want_offset: Some(3),
        want_count: 5,
    });
    // 档6：多差异（三处）。
    out.push(M163Case {
        name: "多差异",
        a: vec![0, 1, 2, 3, 4, 5, 6, 7],
        b: vec![0, 8, 2, 9, 4, 5, 6, 7],
        want_offset: Some(1),
        want_count: 2,
    });
    // 档7：单侧空（长度不齐极端档）。
    out.push(M163Case {
        name: "单侧空",
        a: vec![0],
        b: vec![],
        want_offset: Some(0),
        want_count: 1,
    });
    out
}

/// M-163 差异定位验证：逐档首差异偏移与钦定值一致。
pub fn m163_verify() -> (bool, Vec<bool>) {
    let corpus = m163_corpus();
    let mut per: Vec<bool> = Vec::new();
    if corpus.len() != M163_CASES {
        return (false, per);
    }
    let mut all = true;
    for c in &corpus {
        let got = kexpect_mem_eq(&c.a, &c.b);
        let ok = match (&c.want_offset, got) {
            (None, Ok(MemEqRes::Pass)) => true,
            (Some(w), Ok(MemEqRes::Diff { offset, .. })) => *w == offset,
            _ => false,
        };
        per.push(ok);
        if !ok {
            all = false;
        }
    }
    (all && per.len() == M163_CASES, per)
}

/// 比较器（相位单向 + 同语料双跑确定性）。
#[derive(Clone, Debug)]
pub struct MemComparator {
    phase: Phase,
    last_offset: usize,
    runs: u32,
}

impl MemComparator {
    /// 新比较器（待比）。
    pub fn new() -> MemComparator {
        MemComparator {
            phase: Phase::Ready,
            last_offset: usize::MAX,
            runs: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 已比轮数。
    pub fn runs(&self) -> u32 {
        self.runs
    }

    /// 比一轮：结论落定，相位进「已比」；同语料偏移漂移即报。
    pub fn compare(&mut self, a: &[u8], b: &[u8]) -> Result<MemEqRes, u32> {
        let res = kexpect_mem_eq(a, b)?;
        let off = res.offset().unwrap_or(0);
        if self.phase == Phase::Compared && self.last_offset != usize::MAX && self.last_offset != off
        {
            // 同语料重比偏移漂移（确定性判据）。
            return Err(Diag::EOffsetDrift.val());
        }
        self.last_offset = off;
        self.runs += 1;
        if self.phase == Phase::Ready {
            self.phase = Phase::Compared;
        }
        Ok(res)
    }

    /// 报告落账（Diff 报告字段不齐被拒）。
    pub fn account(&self, res: &MemEqRes) -> Result<(), u32> {
        if !res.wellformed() {
            return Err(Diag::EEmptyReport.val());
        }
        if self.phase != Phase::Compared {
            return Err(Diag::EPhase.val());
        }
        Ok(())
    }
}

/// 注入矩阵置位（越界静默忽略——注入位恒在 0..INJECTS 内）。
pub const INJECTS: usize = 8;

fn mark(out: &mut [bool; INJECTS], idx: usize, detected: bool) {
    if let Some(x) = out.get_mut(idx) {
        *x = detected;
    }
}

/// 注入矩阵 8 项：逐种构造违约，逐种必须被口点出。
pub fn inject_matrix() -> [bool; INJECTS] {
    let mut out = [false; INJECTS];
    // 注入1：改首字节——首差异偏移必须落在 0。
    let r = kexpect_mem_eq(&[1, 2, 3], &[9, 2, 3]);
    mark(&mut out, 0, r.map(|x| x.offset()) == Ok(Some(0)));
    // 注入2：改中段——首差异偏移必须落在 1（首同中异）。
    let r = kexpect_mem_eq(&[1, 2, 3], &[1, 9, 3]);
    mark(&mut out, 1, r.map(|x| x.offset()) == Ok(Some(1)));
    // 注入3：改末字节——首差异偏移必须落在末位 2。
    let r = kexpect_mem_eq(&[1, 2, 3], &[1, 2, 9]);
    mark(&mut out, 2, r.map(|x| x.offset()) == Ok(Some(2)));
    // 注入4：长度截短——前缀全同，长度不齐必须被报告而非误判等值。
    let r = kexpect_mem_eq(&[1, 2, 3], &[1, 2]);
    mark(
        &mut out,
        3,
        matches!(r, Ok(MemEqRes::Diff { offset: 2, .. })),
    );
    // 注入5：长度伸长——长块多出的尾巴必须被报告。
    let r = kexpect_mem_eq(&[1, 2], &[1, 2, 3]);
    mark(
        &mut out,
        4,
        matches!(r, Ok(MemEqRes::Diff { offset: 2, .. })),
    );
    // 注入6：等值误报注入——不等块绝不许被判 Pass。
    let r = kexpect_mem_eq(&[1, 2, 3], &[1, 2, 4]);
    mark(&mut out, 5, !r.map(|x| x.ok()).unwrap_or(true));
    // 注入7：不等误报注入——等值块绝不许被判 Diff。
    let r = kexpect_mem_eq(&[7, 7, 7], &[7, 7, 7]);
    mark(&mut out, 6, r.map(|x| x.ok()).unwrap_or(false));
    // 注入8：双空显性拒——双空块必须 Err 而非 Pass。
    let r = kexpect_mem_eq(&[], &[]);
    mark(&mut out, 7, r == Err(Diag::EBothEmpty.val()));
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B10 派工卡 0x79400..=0x795C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79400 || SEG_BASE > 0x795C0 {
        return false;
    }
    SEG_BASE == 0x79440
}

/// M-163 语料档齐备（七档、档名互异、钦定值与计数对账）。
fn m163_corpus_ok() -> bool {
    let corpus = m163_corpus();
    if corpus.len() != M163_CASES {
        return false;
    }
    let mut i = 0usize;
    while i < corpus.len() {
        let c = match corpus.get(i) {
            Some(x) => x,
            None => return false,
        };
        if c.name.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < corpus.len() {
            let d = match corpus.get(j) {
                Some(x) => x,
                None => return false,
            };
            if c.name == d.name {
                return false;
            }
            j += 1;
        }
        // 计数对账：窗口差异计数与钦定值一致。
        if diff_count(&c.a, &c.b) != c.want_count {
            return false;
        }
        i += 1;
    }
    true
}

/// J1 主判据：内存块等值断言 + 首差异偏移报告。
fn j1_main_ok() -> bool {
    // 等值面。
    let eq = kexpect_mem_eq(&[0xDE, 0xAD, 0xBE, 0xEF], &[0xDE, 0xAD, 0xBE, 0xEF]);
    let eq_ok = matches!(eq, Ok(MemEqRes::Pass));
    // 首差异面：偏移+两侧字节+块长齐备。
    let df = kexpect_mem_eq(&[0x11, 0x22], &[0x11, 0x33]);
    let df_ok = match df {
        Ok(MemEqRes::Diff {
            offset,
            want,
            got,
            len_a,
            len_b,
        }) => offset == 1 && want == 0x22 && got == 0x33 && len_a == 2 && len_b == 2,
        _ => false,
    };
    // 报告行非空（写回面）。
    let line_ok = match kexpect_mem_eq(&[1], &[2]) {
        Ok(r) => !r.report_line().is_empty() && r.report_line().contains("首差异偏移0"),
        Err(_) => false,
    };
    eq_ok && df_ok && line_ok
}

/// M-163 差异定位验证全绿。
fn m163_ok() -> bool {
    let (all, per) = m163_verify();
    all && per.len() == M163_CASES
}

/// 对称性：换位比对偏移与原序一致。
fn symmetry_ok() -> bool {
    let corpus = m163_corpus();
    let mut i = 0usize;
    while i < corpus.len() {
        let c = match corpus.get(i) {
            Some(x) => x,
            None => return false,
        };
        if c.want_offset.is_none() {
            i += 1;
            continue;
        }
        let fwd = kexpect_mem_eq(&c.a, &c.b);
        let rev = kexpect_mem_eq(&c.b, &c.a);
        match (fwd, rev) {
            (Ok(f), Ok(r)) => {
                if f.offset() != r.offset() {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 确定性：同语料双跑结论一致且偏移不漂移。
fn determinism_ok() -> bool {
    let corpus = m163_corpus();
    let mut i = 0usize;
    while i < corpus.len() {
        let c = match corpus.get(i) {
            Some(x) => x,
            None => return false,
        };
        let r1 = kexpect_mem_eq(&c.a, &c.b);
        let r2 = kexpect_mem_eq(&c.a, &c.b);
        if r1 != r2 {
            return false;
        }
        i += 1;
    }
    true
}

/// 比较器相位与漂移门（同语料重比不漂移；换语料显性重置口不在此测）。
fn comparator_ok() -> bool {
    let mut cmp = MemComparator::new();
    if cmp.phase() != Phase::Ready {
        return false;
    }
    let r1 = cmp.compare(&[1, 2, 3], &[1, 2, 9]);
    if cmp.phase() != Phase::Compared || cmp.runs() != 1 {
        return false;
    }
    // 同语料重比：结论一致、不漂移。
    let r2 = cmp.compare(&[1, 2, 3], &[1, 2, 9]);
    match (r1, r2) {
        (Ok(a), Ok(b)) => a == b && cmp.runs() == 2,
        _ => false,
    }
}

/// 双跑一致（语料全档两次验证同判）。
fn dual_run_ok() -> bool {
    let (a, pa) = m163_verify();
    let (b, pb) = m163_verify();
    a && b && pa == pb
}

/// 注入 8/8 检出。
fn inject_all_ok() -> bool {
    inject_matrix() == [true; INJECTS]
}

/// 判据总成：UNX-F3404 kexpect_mem_eq。
pub fn run_unx3404_checks() -> CheckSet {
    let mut s = CheckSet::new("unx3404");

    s.add_dyn(
        "F3404-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    // 正常流：等值断言核（等值面+首差异面+报告行）（J1 主判据）。
    s.add_dyn(
        "F3404-02 内存块等值断言首差异报告",
        j1_main_ok(),
        &format!("j1={}", j1_main_ok()),
    );

    // M-163 七档语料齐备与计数对账。
    s.add_dyn(
        "F3404-03 M163语料七档齐备计数对账",
        m163_corpus_ok(),
        &format!("cases={}", M163_CASES),
    );

    // M-163 差异定位验证全绿。
    let (m_ok, m_per) = m163_verify();
    s.add_dyn(
        "F3404-04 M163差异定位验证全绿",
        m_ok,
        &format!("per={:?}", m_per),
    );

    // 对称性：换位比对偏移一致。
    s.add_dyn(
        "F3404-05 换位比对偏移对称",
        symmetry_ok(),
        &format!("sym={}", symmetry_ok()),
    );

    // 确定性：同语料双跑同判。
    s.add_dyn(
        "F3404-06 同语料双跑同判",
        determinism_ok(),
        &format!("det={}", determinism_ok()),
    );

    // 比较器相位门与漂移门。
    s.add_dyn(
        "F3404-07 比较器相位与漂移门",
        comparator_ok(),
        &format!("phase={}", MemComparator::new().phase().name()),
    );

    // 注入必抓：8 项注入全检出。
    s.add_dyn(
        "F3404-08 注入矩阵八项全检出",
        inject_all_ok(),
        &format!("matrix={:?}", inject_matrix()),
    );

    // 边界：双空块显性拒。
    let both_empty = kexpect_mem_eq(&[], &[]) == Err(Diag::EBothEmpty.val());
    s.add_dyn(
        "F3404-09 双空块显性拒",
        both_empty,
        &format!("code=0x{:05x}", Diag::EBothEmpty.val()),
    );

    // 边界：单侧空按长度不齐报告（偏移 0）。
    let one_empty = matches!(
        kexpect_mem_eq(&[0xAB], &[]),
        Ok(MemEqRes::Diff {
            offset: 0,
            want: 0xAB,
            got: 0,
            ..
        })
    );
    s.add_dyn(
        "F3404-10 单侧空按长度不齐报告",
        one_empty,
        &format!("one_empty={}", one_empty),
    );

    // 报告字段完整性：Diff 报告 wellformed 且 Pass 恒真。
    let wf = MemEqRes::Pass.wellformed()
        && matches!(
            kexpect_mem_eq(&[1, 2], &[1, 3]),
            Ok(r) if r.wellformed()
        );
    s.add_dyn(
        "F3404-11 差异报告字段完整",
        wf,
        &format!("wf={}", wf),
    );

    // 计数对账：全等块计数为零，多差异档计数与钦定值齐。
    let cnt_zero = diff_count(&[9, 9, 9], &[9, 9, 9]) == 0;
    let corpus = m163_corpus();
    let multi = corpus
        .get(5)
        .map(|c| diff_count(&c.a, &c.b) == c.want_count)
        .unwrap_or(false);
    s.add_dyn(
        "F3404-12 差异计数对账",
        cnt_zero && multi,
        &format!("zero={} multi={}", cnt_zero, multi),
    );

    // 双跑一致与诊断码闭集。
    s.add_dyn(
        "F3404-13 双跑一致",
        dual_run_ok(),
        &format!("dual={}", dual_run_ok()),
    );

    s.add_dyn(
        "F3404-14 诊断码闭集",
        ALL.len() == 12 && detail(SEG_BASE + 0x0C) == Diag::EPhase.detail(),
        &format!("all={} seg=0x{:05x}", ALL.len(), SEG_BASE),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EFirstDiff.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x79440);
    }

    #[test]
    fn equal_blocks_pass() {
        let r = kexpect_mem_eq(&[1, 2, 3, 4], &[1, 2, 3, 4]);
        assert_eq!(r, Ok(MemEqRes::Pass));
        assert_eq!(r.as_ref().map(|x| x.offset()), Ok(None));
    }

    #[test]
    fn first_diff_offset_exact() {
        let r = kexpect_mem_eq(&[10, 20, 30], &[10, 20, 99]);
        match r {
            Ok(MemEqRes::Diff {
                offset, want, got, ..
            }) => {
                assert_eq!(offset, 2);
                assert_eq!(want, 30);
                assert_eq!(got, 99);
            }
            other => assert!(
                matches!(other, Ok(MemEqRes::Diff { .. })),
                "期望首差异报告"
            ),
        }
    }

    #[test]
    fn length_mismatch_reported() {
        // 前缀全同、长度不齐：不得误判等值。
        let r = kexpect_mem_eq(&[1, 2, 3], &[1, 2]);
        assert!(matches!(r, Ok(MemEqRes::Diff { offset: 2, .. })));
        let r2 = kexpect_mem_eq(&[1, 2], &[1, 2, 3]);
        assert!(matches!(r2, Ok(MemEqRes::Diff { offset: 2, .. })));
    }

    #[test]
    fn both_empty_rejected() {
        assert_eq!(kexpect_mem_eq(&[], &[]), Err(Diag::EBothEmpty.val()));
    }

    #[test]
    fn m163_seven_cases_green() {
        assert!(m163_corpus_ok());
        let (all, per) = m163_verify();
        assert!(all);
        assert_eq!(per.len(), M163_CASES);
        assert!(per.iter().all(|x| *x));
    }

    #[test]
    fn symmetry_holds() {
        assert!(symmetry_ok());
        let r1 = kexpect_mem_eq(&[1, 2, 3], &[1, 5, 3]);
        let r2 = kexpect_mem_eq(&[1, 5, 3], &[1, 2, 3]);
        assert_eq!(r1.map(|x| x.offset()), r2.map(|x| x.offset()));
    }

    #[test]
    fn inject_eight_of_eight() {
        assert_eq!(inject_matrix(), [true; INJECTS]);
    }

    #[test]
    fn comparator_drift_gate() {
        let mut cmp = MemComparator::new();
        let _ = cmp.compare(&[1, 2], &[1, 9]);
        // 同语料重比不漂移。
        assert!(cmp.compare(&[1, 2], &[1, 9]).is_ok());
        // 换语料偏移漂移被点出。
        assert_eq!(cmp.compare(&[1, 2], &[9, 2]), Err(Diag::EOffsetDrift.val()));
    }

    #[test]
    fn diff_count_ledger() {
        assert_eq!(diff_count(&[1, 1, 1], &[1, 1, 1]), 0);
        assert_eq!(diff_count(&[1, 2, 3], &[1, 9, 3]), 1);
        assert_eq!(diff_count(&[1, 2], &[1, 2, 3]), 1);
        assert_eq!(diff_count(&[], &[]), 0);
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
        let (a, _) = m163_verify();
        let (b, _) = m163_verify();
        assert_eq!(a, b);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx3404_checks();
        assert_eq!(s.len(), 14);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
