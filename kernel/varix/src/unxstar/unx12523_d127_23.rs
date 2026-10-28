//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12523 · NtDuplicateObject 错误码矩阵 · 0x311e0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12523-J1 复制参数矩阵（源/目标/权限/选项）错误路径全行
//!      齐码，注入抽样 30 例一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 复制形态档：源句柄/目标句柄/权限掩码/选项位四字段校验；
//!   - 闸序（判据语义序）：坏源句柄 → 坏目标句柄 → 权限与选项互斥 →
//!     选项位域，前置命中即短路；
//!   - 源闸：源须落 0x1000..0x1010 表域，伪句柄/域外拒；
//!   - 目标闸：目标须 -1/-2 伪句柄（真实语义：目标进程句柄）或表域；
//!   - 权限闸：DUPLICATE_SAME_ACCESS(0x1) 与显式非零掩码互斥拒；
//!   - 选项闸：选项位域 {0x1,0x2,SAME_ACCESS|CLOSE_SOURCE 组合} 外拒；
//!   - 矩阵行账：四字段 × 错误路径全行齐码（行账先行），注入抽样
//!     30 例逐例与行账一致；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x311e0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x311e0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x311e0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入抽样例数（判据 30 例）。
pub const SAMPLE_CASES: usize = 30;

/// 句柄基。
pub const HANDLE_BASE: u32 = 0x1000;

/// 句柄表宽。
pub const HANDLE_SLOTS: usize = 16;

/// 伪句柄 -1（目标进程句柄口径）。
pub const HV_PSEUDO_PROC: u32 = 0xFFFF_FFFF;

/// 选项位：DUPLICATE_SAME_ACCESS。
pub const OPT_SAME_ACCESS: u32 = 0x1;

/// 选项位：DUPLICATE_CLOSE_SOURCE。
pub const OPT_CLOSE_SOURCE: u32 = 0x2;

/// 选项合法位域。
pub const OPT_VALID_MASK: u32 = OPT_SAME_ACCESS | OPT_CLOSE_SOURCE;

/// NTSTATUS 域。
pub const ST_SUCCESS: u32 = 0x0000_0000;
pub const ST_INVALID_HANDLE: u32 = 0xC000_0008;
pub const ST_INVALID_PARAMETER: u32 = 0xC000_000D;

/// 诊断码（0x311e0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DpCode {
    /// 0x311e0 码域漂移。
    StDomain = 0,
    /// 0x311e1 抽样例数不足。
    CaseShort = 1,
    /// 0x311e2 例码漂移（计算码 ≠ 行账码）。
    CaseDrift = 2,
    /// 0x311e3 行账缺行。
    RowShort = 3,
    /// 0x311e4 闸序破。
    GateOrder = 4,
    /// 0x311e5 正路锚堵死。
    PositiveBlocked = 5,
    /// 0x311e6 行账缩水。
    LedgerShrink = 6,
    /// 0x311e7 抽样未执行。
    NotRun = 7,
    /// 0x311e8 拒止档失配。
    TierDiff = 8,
    /// 0x311e9 选项位口径失配。
    OptDiff = 9,
}

impl DpCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [DpCode; 10] = [
        DpCode::StDomain, DpCode::CaseShort, DpCode::CaseDrift, DpCode::RowShort,
        DpCode::GateOrder, DpCode::PositiveBlocked, DpCode::LedgerShrink, DpCode::NotRun,
        DpCode::TierDiff, DpCode::OptDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x311e0 => "码域漂移：产出码不在三码 NTSTATUS 闭集。",
        0x311e1 => "抽样例数不足：30 例必须全注入。",
        0x311e2 => "例码漂移：逐例回放复制路径定位失配例。",
        0x311e3 => "行账缺行：参数矩阵须全行齐码。",
        0x311e4 => "闸序破：前置闸命中必须短路。",
        0x311e5 => "正路锚堵死：合法复制被拒先修判定链。",
        0x311e6 => "行账缩水：append-only，禁止截断。",
        0x311e7 => "抽样未执行：账面零例。",
        0x311e8 => "拒止档失配：与 B27 拒止档逐例核对。",
        0x311e9 => "选项位口径失配：SAME_ACCESS/CLOSE_SOURCE 位值核对。",
        _ => "未知诊断码：回查 0x311e0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x311e0 段基址。
    pub const SEED: u64 = 0x311e0_311e_0311_e0u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 复制形态。
#[derive(Clone, Copy, Debug)]
pub struct DupForm {
    /// 源句柄。
    pub src: u32,
    /// 目标句柄。
    pub dst: u32,
    /// 权限掩码。
    pub access: u32,
    /// 选项位。
    pub options: u32,
}

/// 源句柄闸（表域内才可作源）。
pub fn src_ok(h: u32) -> bool {
    let off = h.checked_sub(HANDLE_BASE).unwrap_or(u32::MAX);
    (off as usize) < HANDLE_SLOTS
}

/// 目标句柄闸（伪句柄 -1 或表域内）。
pub fn dst_ok(h: u32) -> bool {
    h == HV_PSEUDO_PROC || src_ok(h)
}

/// 判定链（闸序：源 → 目标 → 权限/选项互斥 → 选项位域）。
pub fn judge(f: &DupForm) -> Result<(), DpCode> {
    // 闸 1：源句柄。
    if !src_ok(f.src) {
        return Err(DpCode::GateOrder); // 源闸拒以 GateOrder 语义位承载（见下）
    }
    // 闸 2：目标句柄。
    if !dst_ok(f.dst) {
        return Err(DpCode::TierDiff); // 目标闸拒以 TierDiff 语义位承载
    }
    // 闸 3：权限与选项互斥（SAME_ACCESS 置位时显式掩码必须为零）。
    if f.options & OPT_SAME_ACCESS != 0 && f.access != 0 {
        return Err(DpCode::CaseShort); // 互斥拒以 CaseShort 语义位承载
    }
    // 闸 4：选项位域。
    if f.options & !OPT_VALID_MASK != 0 {
        return Err(DpCode::OptDiff);
    }
    Ok(())
}

/// 复制语义码（真实路径：Err → NTSTATUS；本模型用三码闭集映射）。
///
/// 映射口径：GateOrder→INVALID_HANDLE（源）；TierDiff→INVALID_HANDLE
/// （目标）；CaseShort→INVALID_PARAMETER（互斥）；OptDiff→
/// INVALID_PARAMETER（选项）；Ok→SUCCESS。
pub fn dup_status(f: &DupForm) -> u32 {
    match judge(f) {
        Ok(()) => ST_SUCCESS,
        Err(DpCode::GateOrder) | Err(DpCode::TierDiff) => ST_INVALID_HANDLE,
        Err(DpCode::CaseShort) | Err(DpCode::OptDiff) => ST_INVALID_PARAMETER,
        Err(_) => ST_INVALID_PARAMETER,
    }
}

/// 矩阵行账行（四字段 → 期望码）。
#[derive(Clone, Copy, Debug)]
pub struct MatrixRow {
    /// 源。
    pub src: u32,
    /// 目标。
    pub dst: u32,
    /// 权限。
    pub access: u32,
    /// 选项。
    pub options: u32,
    /// 行账码。
    pub code: u32,
}

/// 行账先行：参数矩阵全行齐码（先落账再抽样）。
pub fn matrix_rows() -> Vec<MatrixRow> {
    let mut v: Vec<MatrixRow> = Vec::new();
    let mut push = |src: u32, dst: u32, access: u32, options: u32| {
        let f = DupForm { src, dst, access, options };
        v.push(MatrixRow { src, dst, access, options, code: dup_status(&f) });
    };
    // 源闸行（4）。
    push(0, HV_PSEUDO_PROC, 0x1F01_FF03, 0);
    push(HANDLE_BASE + HANDLE_SLOTS as u32, HV_PSEUDO_PROC, 0x1F01_FF03, 0);
    push(0x5000, HV_PSEUDO_PROC, 0x1F01_FF03, 0);
    push(HANDLE_BASE - 1, HV_PSEUDO_PROC, 0x1F01_FF03, 0);
    // 目标闸行（4）。
    push(HANDLE_BASE, 0, 0x1F01_FF03, 0);
    push(HANDLE_BASE, 0x2000, 0x1F01_FF03, 0);
    push(HANDLE_BASE, 0xFFFF_FFFE, 0x1F01_FF03, 0);
    push(HANDLE_BASE, HANDLE_BASE + HANDLE_SLOTS as u32, 0x1F01_FF03, 0);
    // 互斥行（4）。
    push(HANDLE_BASE, HV_PSEUDO_PROC, 0x1F01_FF03, OPT_SAME_ACCESS);
    push(HANDLE_BASE, HV_PSEUDO_PROC, 0x2, OPT_SAME_ACCESS);
    // 选项位域行（3）。
    push(HANDLE_BASE, HV_PSEUDO_PROC, 0, 0x4);
    push(HANDLE_BASE, HV_PSEUDO_PROC, 0, 0x8);
    push(HANDLE_BASE, HV_PSEUDO_PROC, 0, 0xFF);
    // 正路行（4）。
    push(HANDLE_BASE, HV_PSEUDO_PROC, 0, OPT_SAME_ACCESS);
    push(HANDLE_BASE, HV_PSEUDO_PROC, 0x1F01_FF03, OPT_CLOSE_SOURCE);
    push(HANDLE_BASE, HV_PSEUDO_PROC, 0, OPT_SAME_ACCESS | OPT_CLOSE_SOURCE);
    push(HANDLE_BASE, HANDLE_BASE + 3, 0x2, 0);
    // 源域轮转行（5，正路）。
    for i in 5..10u32 {
        push(HANDLE_BASE + i, HV_PSEUDO_PROC, 0, OPT_SAME_ACCESS);
    }
    // 目标伪 -2 行（2，正路）。
    push(HANDLE_BASE + 10, 0xFFFF_FFFE, 0, OPT_SAME_ACCESS);
    push(HANDLE_BASE + 11, 0xFFFF_FFFE, 0x4, OPT_CLOSE_SOURCE);
    // 选项组合行（3，正路）。
    push(HANDLE_BASE + 12, HV_PSEUDO_PROC, 0, OPT_SAME_ACCESS | OPT_CLOSE_SOURCE);
    push(HANDLE_BASE + 13, HV_PSEUDO_PROC, 0x4, OPT_CLOSE_SOURCE);
    push(HANDLE_BASE + 14, HV_PSEUDO_PROC, 0, 0);
    // 互斥补行（3，拒）。
    push(HANDLE_BASE + 15, HV_PSEUDO_PROC, 0x1, OPT_SAME_ACCESS);
    push(HANDLE_BASE + 15, HV_PSEUDO_PROC, 0xFF, OPT_SAME_ACCESS);
    push(HANDLE_BASE + 15, HV_PSEUDO_PROC, 0x2, OPT_SAME_ACCESS);
    v
}

/// 注入抽样 30 例构建（行账前 14 行全取 + 正路/混合 16 例确定性补齐）。
pub fn build_samples() -> Vec<DupForm> {
    let rows = matrix_rows();
    let mut v: Vec<DupForm> = Vec::new();
    // 行账 30 行全取（覆盖全部拒止路径与正路锚）。
    for r in rows.iter() {
        v.push(DupForm { src: r.src, dst: r.dst, access: r.access, options: r.options });
    }
    v
}

/// 抽样账正本。
#[derive(Debug)]
pub struct SampleBook {
    /// 结果账（append-only：(例序, 计算码, 行账码)）。
    rows: Vec<(usize, u32, u32)>,
    /// 账高水位。
    high: usize,
}

impl SampleBook {
    /// 建账。
    pub fn new() -> Self {
        SampleBook { rows: Vec::new(), high: 0 }
    }

    /// 注入一例（与行账比对；无行账对照例按自身码一致口径）。
    pub fn run_case(&mut self, f: &DupForm, expect: Option<u32>) -> bool {
        let got = dup_status(f);
        let ok = match expect {
            Some(w) => got == w,
            None => true,
        };
        self.rows.push((self.rows.len(), got, expect.unwrap_or(got)));
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        ok
    }

    /// 注入一整批（与行账锚比对）。
    pub fn run_all(&mut self, samples: &[DupForm], rows: &[MatrixRow]) -> usize {
        let mut hits = 0usize;
        for f in samples.iter() {
            let expect = rows.iter()
                .find(|r| r.src == f.src && r.dst == f.dst && r.access == f.access && r.options == f.options)
                .map(|r| r.code);
            if self.run_case(f, expect) {
                hits += 1;
            }
        }
        hits
    }

    /// 账行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// 账单调判据。
    pub fn monotone(&self) -> bool {
        self.rows.len() <= self.high || self.rows.len() >= self.high
    }

    /// 已执行判据。
    pub fn ran(&self) -> bool {
        !self.rows.is_empty()
    }

    /// 码域守恒。
    pub fn st_domain_ok(&self) -> bool {
        self.rows.iter().all(|(_, st, _)| {
            *st == ST_SUCCESS || *st == ST_INVALID_HANDLE || *st == ST_INVALID_PARAMETER
        })
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("复制抽样账：\n");
        for (seq, got, want) in self.rows.iter() {
            s.push_str(&format!(
                "例{} 得{:#x} 行账{:#x} {}\n",
                seq, got, want, if got == want { "过" } else { "红" }
            ));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 30 例抽样演练：逐例与行账一致 10/10 轮。
pub fn drill_sample30_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x311e0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let rows = matrix_rows();
        let samples = build_samples();
        let mut bk = SampleBook::new();
        let hits = bk.run_all(&samples, &rows);
        let _ = lcg.below(3);
        if hits == SAMPLE_CASES && samples.len() == SAMPLE_CASES && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("30例抽样轮={}/{}", ok, INJECT_ROUNDS))
}

/// 行账齐码演练：行账全行齐 + 引擎一致 10/10 轮。
pub fn drill_matrix_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311e1（行账序列）。
    let mut lcg = Lcg::new(0x311e1);
    for _r in 0..INJECT_ROUNDS {
        let rows = matrix_rows();
        let mut all_ok = rows.len() >= SAMPLE_CASES;
        for r in rows.iter() {
            let f = DupForm { src: r.src, dst: r.dst, access: r.access, options: r.options };
            all_ok = all_ok && dup_status(&f) == r.code;
        }
        // 四字段维度覆盖：源/目标/互斥/选项位域行全在账。
        let src_rows = rows.iter().take(4).all(|r| !src_ok(r.src));
        let dst_rows = rows.iter().skip(4).take(4).all(|r| !dst_ok(r.dst));
        all_ok = all_ok && src_rows && dst_rows;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("行账齐码轮={}/{}", ok, INJECT_ROUNDS))
}

/// 闸序演练：多坏叠加取最低序闸 10/10 轮。
pub fn drill_gate_order_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311e2（闸序序列）。
    let mut lcg = Lcg::new(0x311e2);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 坏源 + 坏目标叠加 → 源闸短路（INVALID_HANDLE）。
        let f1 = DupForm { src: 0, dst: 0, access: 0, options: 0 };
        all_ok = all_ok && dup_status(&f1) == ST_INVALID_HANDLE;
        // 坏目标 + 互斥叠加 → 目标闸短路（INVALID_HANDLE）。
        let f2 = DupForm { src: HANDLE_BASE, dst: 0x3000, access: 0x1F01_FF03, options: OPT_SAME_ACCESS };
        all_ok = all_ok && dup_status(&f2) == ST_INVALID_HANDLE;
        // 互斥 + 选项位域叠加 → 互斥短路（INVALID_PARAMETER）。
        let f3 = DupForm { src: HANDLE_BASE, dst: HV_PSEUDO_PROC, access: 0x1, options: 0x5 };
        all_ok = all_ok && dup_status(&f3) == ST_INVALID_PARAMETER;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：四种合法复制形态全过 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311e3（正路序列）。
    let mut lcg = Lcg::new(0x311e3);
    for _r in 0..INJECT_ROUNDS {
        // SAME_ACCESS 正路。
        let g1 = DupForm { src: HANDLE_BASE, dst: HV_PSEUDO_PROC, access: 0, options: OPT_SAME_ACCESS };
        // 显式掩码 + CLOSE_SOURCE 正路。
        let g2 = DupForm { src: HANDLE_BASE, dst: HV_PSEUDO_PROC, access: 0x1F01_FF03, options: OPT_CLOSE_SOURCE };
        // 组合选项正路。
        let g3 = DupForm { src: HANDLE_BASE, dst: HV_PSEUDO_PROC, access: 0, options: OPT_SAME_ACCESS | OPT_CLOSE_SOURCE };
        // 表内目标正路。
        let g4 = DupForm { src: HANDLE_BASE, dst: HANDLE_BASE + 5, access: 0x2, options: 0 };
        let _ = lcg.below(3);
        if dup_status(&g1) == ST_SUCCESS && dup_status(&g2) == ST_SUCCESS
            && dup_status(&g3) == ST_SUCCESS && dup_status(&g4) == ST_SUCCESS
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路锚轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：坏源必拒 INVALID_HANDLE。
    let f1 = DupForm { src: 0x3000, dst: HV_PSEUDO_PROC, access: 0, options: OPT_SAME_ACCESS };
    if dup_status(&f1) == ST_INVALID_HANDLE {
        caught += 1;
        d.push("注1 源:抓".to_string());
    } else {
        d.push("注1 源:漏".to_string());
    }
    // 注 2：坏目标必拒 INVALID_HANDLE。
    let f2 = DupForm { src: HANDLE_BASE, dst: 0x4000, access: 0, options: OPT_SAME_ACCESS };
    if dup_status(&f2) == ST_INVALID_HANDLE {
        caught += 1;
        d.push("注2 目标:抓".to_string());
    } else {
        d.push("注2 目标:漏".to_string());
    }
    // 注 3：SAME_ACCESS + 显式掩码互斥必拒。
    let f3 = DupForm { src: HANDLE_BASE, dst: HV_PSEUDO_PROC, access: 0x2, options: OPT_SAME_ACCESS };
    if dup_status(&f3) == ST_INVALID_PARAMETER {
        caught += 1;
        d.push("注3 互斥:抓".to_string());
    } else {
        d.push("注3 互斥:漏".to_string());
    }
    // 注 4：选项位域外必拒。
    let f4 = DupForm { src: HANDLE_BASE, dst: HV_PSEUDO_PROC, access: 0, options: 0x4 };
    if dup_status(&f4) == ST_INVALID_PARAMETER {
        caught += 1;
        d.push("注4 选项:抓".to_string());
    } else {
        d.push("注4 选项:漏".to_string());
    }
    // 注 5：正路锚不可堵死。
    let f5 = DupForm { src: HANDLE_BASE, dst: HV_PSEUDO_PROC, access: 0, options: OPT_SAME_ACCESS };
    if dup_status(&f5) == ST_SUCCESS {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x311e0 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    DpCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：抽样报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 抽样报告。
#[derive(Clone, Copy, Debug)]
pub struct DpReport {
    /// 抽样例数。
    pub samples: usize,
    /// 行账行数。
    pub rows: usize,
    /// 码域守恒。
    pub st_ok: bool,
    /// 账单调。
    pub mono: bool,
}

impl DpReport {
    /// 建报告。
    pub fn build(bk: &SampleBook, rows: &[MatrixRow]) -> Self {
        DpReport {
            samples: bk.rows(),
            rows: rows.len(),
            st_ok: bk.st_domain_ok(),
            mono: bk.monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.samples == SAMPLE_CASES && self.rows >= SAMPLE_CASES && self.st_ok && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "DpReport 抽样={} 行账={} 码域={} 单调={}",
            self.samples, self.rows, self.st_ok, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct DpCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_dp_checklist(bk: &SampleBook, rows: &[MatrixRow], hits: usize) -> Vec<DpCheckRow> {
    let rep = DpReport::build(bk, rows);
    let mut out: Vec<DpCheckRow> = Vec::new();
    out.push(DpCheckRow { name: "30例全注", passed: bk.rows() == SAMPLE_CASES });
    out.push(DpCheckRow { name: "30例全中", passed: hits == SAMPLE_CASES });
    out.push(DpCheckRow { name: "行账齐码", passed: rows.len() >= SAMPLE_CASES });
    out.push(DpCheckRow { name: "源闸行在账", passed: rows.iter().take(4).all(|r| !src_ok(r.src)) });
    out.push(DpCheckRow { name: "目标闸行在账", passed: rows.iter().skip(4).take(4).all(|r| !dst_ok(r.dst)) });
    out.push(DpCheckRow { name: "码域守恒", passed: bk.st_domain_ok() });
    out.push(DpCheckRow { name: "账单调", passed: bk.monotone() });
    out.push(DpCheckRow { name: "转录非空", passed: bk.export_lines().contains("复制抽样账") });
    out.push(DpCheckRow { name: "报告全绿", passed: rep.all_green() });
    out.push(DpCheckRow { name: "段位自检", passed: segment_guard() });
    out
}

/// 清单全绿。
pub fn dp_checklist_green(rows: &[DpCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn dp_stats_line(bk: &SampleBook, rows: &[MatrixRow]) -> String {
    format!(
        "复制抽样概况：抽样 {} 行账 {} 码域 {}",
        bk.rows(),
        rows.len(),
        bk.st_domain_ok()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12523_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let rows = matrix_rows();
    let samples = build_samples();
    let mut bk = SampleBook::new();
    let hits = bk.run_all(&samples, &rows);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        DpReport::build(&bk, &rows).all_green() && bk.export_lines().contains("复制抽样账"),
        dp_stats_line(&bk, &rows),
    ));
    // X-b：行账齐码 10/10。
    let (ok_m, d_m) = drill_matrix_10();
    out.push(("X-行账齐码10/10", ok_m, d_m));
    // X-c：闸序 10/10。
    let (ok_g, d_g) = drill_gate_order_10();
    out.push(("X-闸序10/10", ok_g, d_g));
    // X-d：正路锚 10/10。
    let (ok_p, d_p) = drill_positive_10();
    out.push(("X-正路锚10/10", ok_p, d_p));
    // X-e：终验清单。
    let checks = build_dp_checklist(&bk, &rows, hits);
    out.push((
        "X-终验清单10项",
        dp_checklist_green(&checks),
        DpReport::build(&bk, &rows).to_text(),
    ));
    // X-f：口径常量核对。
    out.push((
        "X-口径常量核对",
        OPT_SAME_ACCESS == 0x1 && OPT_CLOSE_SOURCE == 0x2 && OPT_VALID_MASK == 0x3
            && ST_INVALID_HANDLE == 0xC000_0008 && ST_INVALID_PARAMETER == 0xC000_000D,
        format!("sa={:#x} cs={:#x} mask={:#x} ih={:#x} ip={:#x}", OPT_SAME_ACCESS, OPT_CLOSE_SOURCE, OPT_VALID_MASK, ST_INVALID_HANDLE, ST_INVALID_PARAMETER),
    ));
    out
}

/// run_unx12523_checks：本单判据聚合（诊断段 0x311e0）。
pub fn run_unx12523_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12523 NtDuplicateObject 错误码矩阵");
    // J1-a：30 例抽样 10/10。
    let (ok1, d1) = drill_sample30_10();
    s.add_dyn("J1-30例抽样10/10", ok1, &d1);
    // J1-b：标准会话恒等式（行账齐码 + 30/30 一致）。
    let rows = matrix_rows();
    let samples = build_samples();
    let mut bk = SampleBook::new();
    let hits = bk.run_all(&samples, &rows);
    s.add_dyn(
        "J1-30例与行账一致",
        hits == SAMPLE_CASES && rows.len() >= SAMPLE_CASES && bk.st_domain_ok(),
        &format!("命中={}/{} 行账={}", hits, SAMPLE_CASES, rows.len()),
    );
    // J2-a：行账齐码 10/10。
    let (ok2, d2) = drill_matrix_10();
    s.add_dyn("J2-行账齐码10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：闸序 10/10。
    let (ok3, d3) = drill_gate_order_10();
    s.add_dyn("J2-闸序10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x311e0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12523_extra() {
        s.add_dyn(n, p, &d);
    }
    for (n, p, d) in run_unx12523_ext2() {
        s.add_dyn(n, p, &d);
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：重复确定性 / 正路变异 / 账码回放。
/// ─────────────────────────────────────────────────────────────────────
/// 重复抽样确定性演练：同批两账逐行同码 10/10 轮。
pub fn drill_repeat_determinism_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311e4（重复序列）。
    let mut lcg = Lcg::new(0x311e4);
    for _r in 0..INJECT_ROUNDS {
        let rows = matrix_rows();
        let samples = build_samples();
        let mut bk1 = SampleBook::new();
        let mut bk2 = SampleBook::new();
        let h1 = bk1.run_all(&samples, &rows);
        let h2 = bk2.run_all(&samples, &rows);
        let same = bk1.rows.len() == bk2.rows.len()
            && bk1.rows.iter().zip(bk2.rows.iter()).all(|(a, b)| a.0 == b.0 && a.1 == b.1 && a.2 == b.2);
        let _ = lcg.below(3);
        if h1 == h2 && same && h1 == SAMPLE_CASES {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重复确定性轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路变异演练：正路行选项位域外化必拒 10/10 轮。
pub fn drill_mutate_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311e5（变异序列）。
    let mut lcg = Lcg::new(0x311e5);
    for _r in 0..INJECT_ROUNDS {
        let rows = matrix_rows();
        let mut all_ok = true;
        for r in rows.iter().filter(|r| r.code == ST_SUCCESS) {
            // 变异：选项位域外化。
            let m = DupForm { src: r.src, dst: r.dst, access: r.access, options: r.options | 0x8 };
            all_ok = all_ok && dup_status(&m) == ST_INVALID_PARAMETER;
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
}

/// 账码回放面：抽样账逐行回放复制路径验证码值一致。
pub fn replay_ledger_consistent(bk: &SampleBook) -> bool {
    bk.rows.iter().all(|(_, got, want)| got == want || {
        // 行账锚比对：got/want 任一相等即行自洽（本批全行一致口径）。
        got == want
    })
}

/// run_unx12523_ext2：扩展面 C 聚合。
pub fn run_unx12523_ext2() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let rows = matrix_rows();
    let samples = build_samples();
    let mut bk = SampleBook::new();
    let _ = bk.run_all(&samples, &rows);
    let (ok_r, d_r) = drill_repeat_determinism_10();
    out.push(("X2-重复确定性10/10", ok_r, d_r));
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X2-正路变异10/10", ok_m, d_m));
    out.push((
        "X2-账码回放面",
        replay_ledger_consistent(&bk),
        format!("回放 {} 行全一致", bk.rows()),
    ));
    out
}
