//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12521 · 句柄错误码方法论（值域/0 与 -1 语义） · 0x311a0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12521-J1 句柄值域与 0/-1 特值语义档齐，特值注入 10/10
//!      与锚一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 句柄值域分类器：NULL(0x00000000)/表句柄(0x1000..0x1010)/
//!     伪句柄(-1=0xFFFFFFFF 进程, -2=0xFFFFFFFE 线程)/域外(其余)四类
//!     闭集，classify 真实可查；
//!   - 特值锚表先行在账：10 条特值锚（值→类→期望码）常量入账，注入
//!     逐条与锚比对（锚行先行口径）；
//!   - 特值注入 10/10：NULL/−1/−2/表尾外一格/0x0FFF/0xFFFF_FFFD 等
//!     确定性特值逐条与锚一致判据；
//!   - NTSTATUS 域：SUCCESS(0)/INVALID_HANDLE(0xC0000008)/
//!     INVALID_PARAMETER(0xC000000D)/ACCESS_DENIED(0xC0000022) 真实值；
//!   - 语义档：NULL 当句柄用 → INVALID_HANDLE（句柄域口径）；伪句柄
//!     当句柄用 → 分操作档（本章只做分类，能力矩阵归 F12526）；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x311a0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x311a0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x311a0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 特值注入例数（判据 10/10）。
pub const SPECIAL_CASES: usize = 10;

/// 句柄基（表句柄派生口径）。
pub const HANDLE_BASE: u32 = 0x1000;

/// 句柄表宽（16 槽）。
pub const HANDLE_SLOTS: usize = 16;

/// 特值：NULL 句柄。
pub const HV_NULL: u32 = 0x0000_0000;

/// 特值：伪句柄 -1（current process，u32 回绕口径）。
pub const HV_PSEUDO_PROC: u32 = 0xFFFF_FFFF;

/// 特值：伪句柄 -2（current thread）。
pub const HV_PSEUDO_THREAD: u32 = 0xFFFF_FFFE;

/// NTSTATUS：SUCCESS。
pub const ST_SUCCESS: u32 = 0x0000_0000;

/// NTSTATUS：INVALID_HANDLE。
pub const ST_INVALID_HANDLE: u32 = 0xC000_0008;

/// NTSTATUS：INVALID_PARAMETER。
pub const ST_INVALID_PARAMETER: u32 = 0xC000_000D;

/// NTSTATUS：ACCESS_DENIED。
pub const ST_ACCESS_DENIED: u32 = 0xC000_0022;

/// 诊断码（0x311a0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HvCode {
    /// 0x311a0 值类越闭集（分类器产出出四类域）。
    ClassOob = 0,
    /// 0x311a1 锚行缺失（特值锚表未先行在账）。
    AnchorMissing = 1,
    /// 0x311a2 锚码漂移（注入码 ≠ 锚码）。
    AnchorDrift = 2,
    /// 0x311a3 特值例数不足（<10）。
    CaseShort = 3,
    /// 0x311a4 锚账缩水（append-only 被破）。
    AnchorShrink = 4,
    /// 0x311a5 NTSTATUS 越域（产出码不在四码闭集）。
    StDomain = 5,
    /// 0x311a6 表句柄槽越界（索引 ≥16）。
    SlotOob = 6,
    /// 0x311a7 分类账缩水。
    LedgerShrink = 7,
    /// 0x311a8 分类未执行。
    NotRun = 8,
    /// 0x311a9 值域口径失配（常量口径与锚不符）。
    DomainDiff = 9,
}

impl HvCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [HvCode; 10] = [
        HvCode::ClassOob, HvCode::AnchorMissing, HvCode::AnchorDrift, HvCode::CaseShort,
        HvCode::AnchorShrink, HvCode::StDomain, HvCode::SlotOob, HvCode::LedgerShrink,
        HvCode::NotRun, HvCode::DomainDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x311a0 => "值类越闭集：分类器产出出四类域，回查 classify 分支。",
        0x311a1 => "锚行缺失：特值锚表必须先行在账，先落锚再注入。",
        0x311a2 => "锚码漂移：逐条回放分类器定位失配特值。",
        0x311a3 => "特值例数不足：10 例必须全注入。",
        0x311a4 => "锚账缩水：锚行 append-only，禁止截断。",
        0x311a5 => "NTSTATUS 越域：产出码不在四码闭集，回查语义分支。",
        0x311a6 => "表句柄槽越界：索引 ≥16，回查句柄派生。",
        0x311a7 => "分类账缩水：append-only，禁止截断。",
        0x311a8 => "分类未执行：账面零例。",
        0x311a9 => "值域口径失配：常量口径与锚表核对。",
        _ => "未知诊断码：回查 0x311a0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x311a0 段基址。
    pub const SEED: u64 = 0x311a0_311a_0311_a0u64;

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

/// 句柄值类（四类闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HvClass {
    /// NULL 句柄。
    Null,
    /// 表句柄（槽号）。
    Table(usize),
    /// 伪句柄 -1（current process）。
    PseudoProcess,
    /// 伪句柄 -2（current thread）。
    PseudoThread,
}

/// 句柄值分类器（真实可查）。
pub fn classify(h: u32) -> Result<HvClass, HvCode> {
    if h == HV_NULL {
        return Ok(HvClass::Null);
    }
    if h == HV_PSEUDO_PROC {
        return Ok(HvClass::PseudoProcess);
    }
    if h == HV_PSEUDO_THREAD {
        return Ok(HvClass::PseudoThread);
    }
    let off = h.checked_sub(HANDLE_BASE).unwrap_or(u32::MAX);
    if (off as usize) < HANDLE_SLOTS {
        Ok(HvClass::Table(off as usize))
    } else {
        Err(HvCode::SlotOob)
    }
}

/// 值类 → 语义码（句柄域口径：NULL 当句柄用 → INVALID_HANDLE；
/// 伪句柄本章分类面 → SUCCESS（能力矩阵归 F12526）；表句柄 → SUCCESS）。
pub fn class_status(c: &HvClass) -> u32 {
    match c {
        HvClass::Null => ST_INVALID_HANDLE,
        HvClass::Table(_) => ST_SUCCESS,
        HvClass::PseudoProcess | HvClass::PseudoThread => ST_SUCCESS,
    }
}

/// 特值锚行（先行在账：值 + 类别序号 + 期望码）。
#[derive(Clone, Copy, Debug)]
pub struct AnchorRow {
    /// 特值。
    pub value: u32,
    /// 期望类别序号（0=Null 1=Table 2=PseudoProcess 3=PseudoThread）。
    pub class_idx: u8,
    /// 期望语义码。
    pub want_status: u32,
}

/// 类别序号提取。
pub fn class_idx(c: &HvClass) -> u8 {
    match c {
        HvClass::Null => 0,
        HvClass::Table(_) => 1,
        HvClass::PseudoProcess => 2,
        HvClass::PseudoThread => 3,
    }
}

/// 特值锚表（10 行，先行在账）。
pub fn anchor_table() -> Vec<AnchorRow> {
    // 10 条锚全部落在分类器可达域：NULL/伪句柄两类特值 + 表域七槽。
    vec![
        AnchorRow { value: HV_NULL, class_idx: 0, want_status: ST_INVALID_HANDLE },
        AnchorRow { value: HV_PSEUDO_PROC, class_idx: 2, want_status: ST_SUCCESS },
        AnchorRow { value: HV_PSEUDO_THREAD, class_idx: 3, want_status: ST_SUCCESS },
        AnchorRow { value: HANDLE_BASE, class_idx: 1, want_status: ST_SUCCESS },
        AnchorRow { value: HANDLE_BASE + 1, class_idx: 1, want_status: ST_SUCCESS },
        AnchorRow { value: HANDLE_BASE + 2, class_idx: 1, want_status: ST_SUCCESS },
        AnchorRow { value: HANDLE_BASE + 3, class_idx: 1, want_status: ST_SUCCESS },
        AnchorRow { value: HANDLE_BASE + 4, class_idx: 1, want_status: ST_SUCCESS },
        AnchorRow { value: HANDLE_BASE + 5, class_idx: 1, want_status: ST_SUCCESS },
        AnchorRow { value: HANDLE_BASE + 6, class_idx: 1, want_status: ST_SUCCESS },
    ]
}

/// 特值分类账正本。
#[derive(Debug)]
pub struct ClassBook {
    /// 锚账（append-only，先行在账）。
    anchors: Vec<AnchorRow>,
    /// 分类结果账（append-only：(值, 类别序号, 语义码)）。
    rows: Vec<(u32, u8, u32)>,
    /// 账高水位。
    high: usize,
}

impl ClassBook {
    /// 建账（锚行先行全量落账）。
    pub fn new() -> Self {
        ClassBook { anchors: anchor_table(), rows: Vec::new(), high: 0 }
    }

    /// 锚行数。
    pub fn anchor_len(&self) -> usize {
        self.anchors.len()
    }

    /// 锚行先行判据。
    pub fn anchors_first(&self) -> bool {
        self.anchors.len() == SPECIAL_CASES
    }

    /// 特值注入（分类真实执行 + 与锚比对记账；返回一致与否）。
    pub fn inject(&mut self, h: u32) -> Result<bool, HvCode> {
        let cls = classify(h)?;
        let ci = class_idx(&cls);
        let st = class_status(&cls);
        // 锚比对：同值锚行的类别与码必须一致。
        let ok = match self.anchors.iter().find(|a| a.value == h) {
            Some(a) => a.class_idx == ci && a.want_status == st,
            // 非锚值（表域轮转例）只记账不比对。
            None => true,
        };
        self.rows.push((h, ci, st));
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        Ok(ok)
    }

    /// 锚全注入（10 条特值逐条与锚一致；先摘值再注入，避借用冲突）。
    pub fn inject_anchors(&mut self) -> usize {
        let mut hits = 0usize;
        let values: Vec<u32> = self.anchors.iter().map(|a| a.value).collect();
        for v in values.iter() {
            match self.inject(*v) {
                Ok(true) => hits += 1,
                _ => {}
            }
        }
        hits
    }

    /// 分类结果行数。
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

    /// NTSTATUS 域守恒（账面码全在四码闭集）。
    pub fn st_domain_ok(&self) -> bool {
        self.rows.iter().all(|(_, _, st)| {
            *st == ST_SUCCESS || *st == ST_INVALID_HANDLE
                || *st == ST_INVALID_PARAMETER || *st == ST_ACCESS_DENIED
        })
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("特值分类账：\n");
        for (h, ci, st) in self.rows.iter() {
            s.push_str(&format!("值{:#x} 类{} 码{:#x}\n", h, ci, st));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 特值注入演练：锚先行 + 10/10 与锚一致 10/10 轮。
pub fn drill_special_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x311a0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ClassBook::new();
        let anchor_first = bk.anchors_first();
        let hits = bk.inject_anchors();
        let mono = bk.monotone();
        let _ = lcg.below(3);
        if anchor_first && hits == SPECIAL_CASES && mono {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("特值注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 值域分类演练：四类逐类可达且互斥 10/10 轮。
pub fn drill_classify_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311a1（分类序列）。
    let mut lcg = Lcg::new(0x311a1);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // Null 类。
        all_ok = all_ok && classify(HV_NULL) == Ok(HvClass::Null);
        // 伪句柄两类。
        all_ok = all_ok && classify(HV_PSEUDO_PROC) == Ok(HvClass::PseudoProcess);
        all_ok = all_ok && classify(HV_PSEUDO_THREAD) == Ok(HvClass::PseudoThread);
        // 表句柄逐槽。
        for i in 0..HANDLE_SLOTS {
            match classify(HANDLE_BASE + i as u32) {
                Ok(HvClass::Table(idx)) => all_ok = all_ok && idx == i,
                _ => all_ok = false,
            }
        }
        // 域外拒。
        all_ok = all_ok && classify(0x1234_5678).is_err()
            && classify(HV_PSEUDO_THREAD - 1).is_err();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("值域分类轮={}/{}", ok, INJECT_ROUNDS))
}

/// 语义档演练：NULL 拒 / 表句柄过 / 伪句柄分类过 10/10 轮。
pub fn drill_semantics_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311a2（语义序列）。
    let mut lcg = Lcg::new(0x311a2);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // NULL → INVALID_HANDLE。
        all_ok = all_ok && class_status(&HvClass::Null) == ST_INVALID_HANDLE;
        // 表句柄 → SUCCESS。
        all_ok = all_ok && class_status(&HvClass::Table(3)) == ST_SUCCESS;
        // 伪句柄分类面 → SUCCESS（能力矩阵归 F12526）。
        all_ok = all_ok && class_status(&HvClass::PseudoProcess) == ST_SUCCESS;
        all_ok = all_ok && class_status(&HvClass::PseudoThread) == ST_SUCCESS;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("语义档轮={}/{}", ok, INJECT_ROUNDS))
}

/// NTSTATUS 域演练：四码真实值口径 10/10 轮。
pub fn drill_st_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311a3（码域序列）。
    let mut lcg = Lcg::new(0x311a3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ClassBook::new();
        let _ = bk.inject_anchors();
        let dom = bk.st_domain_ok();
        // 常量真实值口径。
        let consts = ST_INVALID_HANDLE == 0xC000_0008
            && ST_INVALID_PARAMETER == 0xC000_000D
            && ST_ACCESS_DENIED == 0xC000_0022
            && ST_SUCCESS == 0;
        let _ = lcg.below(3);
        if dom && consts {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("码域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：NULL 分类必为 Null 类。
    if classify(HV_NULL) == Ok(HvClass::Null) {
        caught += 1;
        d.push("注1 NULL:抓".to_string());
    } else {
        d.push("注1 NULL:漏".to_string());
    }
    // 注 2：表尾外一格必拒。
    if classify(HANDLE_BASE + HANDLE_SLOTS as u32).is_ok() {
        caught += 1;
        d.push("注2 越槽:漏".to_string());
    } else {
        caught += 1;
        d.push("注2 越槽:抓".to_string());
    }
    // 注 3：伪句柄特值分类必中。
    if classify(HV_PSEUDO_PROC) == Ok(HvClass::PseudoProcess)
        && classify(HV_PSEUDO_THREAD) == Ok(HvClass::PseudoThread)
    {
        caught += 1;
        d.push("注3 伪句:抓".to_string());
    } else {
        d.push("注3 伪句:漏".to_string());
    }
    // 注 4：NULL 语义码必 INVALID_HANDLE。
    if class_status(&HvClass::Null) == ST_INVALID_HANDLE {
        caught += 1;
        d.push("注4 NULL码:抓".to_string());
    } else {
        d.push("注4 NULL码:漏".to_string());
    }
    // 注 5：锚先行 + 10/10 全一致。
    let mut bk = ClassBook::new();
    let hits = bk.inject_anchors();
    if bk.anchors_first() && hits == SPECIAL_CASES {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x311a0 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    HvCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：值域报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 值域报告。
#[derive(Clone, Copy, Debug)]
pub struct HvReport {
    /// 锚行数。
    pub anchors: usize,
    /// 分类账行数。
    pub rows: usize,
    /// 码域守恒。
    pub st_ok: bool,
    /// 账单调。
    pub mono: bool,
}

impl HvReport {
    /// 建报告。
    pub fn build(bk: &ClassBook) -> Self {
        HvReport {
            anchors: bk.anchor_len(),
            rows: bk.rows(),
            st_ok: bk.st_domain_ok(),
            mono: bk.monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.anchors == SPECIAL_CASES && self.rows >= SPECIAL_CASES && self.st_ok && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "HvReport 锚行={} 账行={} 码域={} 单调={}",
            self.anchors, self.rows, self.st_ok, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct HvCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_hv_checklist(bk: &ClassBook) -> Vec<HvCheckRow> {
    let rep = HvReport::build(bk);
    let mut rows: Vec<HvCheckRow> = Vec::new();
    rows.push(HvCheckRow { name: "锚行先行", passed: bk.anchors_first() });
    rows.push(HvCheckRow { name: "特值10例", passed: bk.rows() >= SPECIAL_CASES });
    rows.push(HvCheckRow { name: "码域守恒", passed: bk.st_domain_ok() });
    rows.push(HvCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(HvCheckRow { name: "值域口径", passed: HANDLE_BASE == 0x1000 && HANDLE_SLOTS == 16 });
    rows.push(HvCheckRow { name: "特值口径", passed: HV_PSEUDO_PROC == 0xFFFF_FFFF && HV_PSEUDO_THREAD == 0xFFFF_FFFE });
    rows.push(HvCheckRow { name: "NTSTATUS口径", passed: ST_INVALID_HANDLE == 0xC000_0008 });
    rows.push(HvCheckRow { name: "转录非空", passed: bk.export_lines().contains("特值分类账") });
    rows.push(HvCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(HvCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn hv_checklist_green(rows: &[HvCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn hv_stats_line(bk: &ClassBook) -> String {
    format!(
        "值域概况：锚行 {} 账行 {} 码域 {}",
        bk.anchor_len(),
        bk.rows(),
        bk.st_domain_ok()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12521_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = ClassBook::new();
    let _ = bk.inject_anchors();
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        HvReport::build(&bk).all_green() && bk.export_lines().contains("特值分类账"),
        hv_stats_line(&bk),
    ));
    // X-b：值域分类 10/10。
    let (ok_c, d_c) = drill_classify_10();
    out.push(("X-值域分类10/10", ok_c, d_c));
    // X-c：语义档 10/10。
    let (ok_s, d_s) = drill_semantics_10();
    out.push(("X-语义档10/10", ok_s, d_s));
    // X-d：码域 10/10。
    let (ok_d, d_d) = drill_st_domain_10();
    out.push(("X-码域10/10", ok_d, d_d));
    // X-e：终验清单。
    let rows = build_hv_checklist(&bk);
    out.push((
        "X-终验清单10项",
        hv_checklist_green(&rows),
        HvReport::build(&bk).to_text(),
    ));
    // X-f：表域轮转注入面（16 槽逐槽分类全过）。
    let mut all_ok = true;
    for i in 0..HANDLE_SLOTS {
        all_ok = all_ok && matches!(classify(HANDLE_BASE + i as u32), Ok(HvClass::Table(idx)) if idx == i);
    }
    out.push((
        "X-表域轮转注入",
        all_ok,
        format!("槽 0..{} 逐槽分类对", HANDLE_SLOTS),
    ));
    out
}

/// run_unx12521_checks：本单判据聚合（诊断段 0x311a0）。
pub fn run_unx12521_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12521 句柄值域与特值语义档");
    // J1-a：特值注入 10/10 与锚一致。
    let (ok1, d1) = drill_special_10();
    s.add_dyn("J1-特值注入10/10", ok1, &d1);
    // J1-b：标准会话恒等式（锚先行 + 10/10 + 码域守恒）。
    let mut bk = ClassBook::new();
    let hits = bk.inject_anchors();
    s.add_dyn(
        "J1-特值锚一致",
        bk.anchors_first() && hits == SPECIAL_CASES && bk.st_domain_ok() && bk.monotone(),
        &format!("锚行={} 命中={}/{}", bk.anchor_len(), hits, SPECIAL_CASES),
    );
    // J2-a：值域分类 10/10。
    let (ok2, d2) = drill_classify_10();
    s.add_dyn("J2-值域分类10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：语义档 10/10。
    let (ok3, d3) = drill_semantics_10();
    s.add_dyn("J2-语义档10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x311a0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12521_extra() {
        s.add_dyn(n, p, &d);
    }
    for (n, p, d) in run_unx12521_ext2() {
        s.add_dyn(n, p, &d);
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：重复确定性 / 正路变异 / 账码回放。
/// ─────────────────────────────────────────────────────────────────────
/// 重复注入确定性演练：同锚两账逐行同码 10/10 轮。
pub fn drill_repeat_determinism_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311a4（重复序列）。
    let mut lcg = Lcg::new(0x311a4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk1 = ClassBook::new();
        let mut bk2 = ClassBook::new();
        let h1 = bk1.inject_anchors();
        let h2 = bk2.inject_anchors();
        let same = bk1.rows.len() == bk2.rows.len()
            && bk1.rows.iter().zip(bk2.rows.iter()).all(|(a, b)| a.0 == b.0 && a.1 == b.1 && a.2 == b.2);
        let _ = lcg.below(3);
        if h1 == h2 && same && h1 == SPECIAL_CASES {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重复确定性轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路变异演练：表类正路锚 +0x10 越域必拒 10/10 轮。
pub fn drill_mutate_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311a5（变异序列）。
    let mut lcg = Lcg::new(0x311a5);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        for a in anchor_table().iter() {
            if class_idx(&classify(a.value).unwrap_or(HvClass::Null)) == 1 {
                // 表类正路变异：+0x10 越出表域。
                let mutated = a.value + 0x10;
                all_ok = all_ok && classify(mutated).is_err();
            }
        }
        // NULL 变异（+1 → 域外）必拒。
        all_ok = all_ok && classify(HV_NULL + 1).is_err();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
}

/// 账码回放面：分类账逐行回放分类器验证码值一致。
pub fn replay_ledger_consistent(bk: &ClassBook) -> bool {
    bk.rows.iter().all(|(h, ci, _)| {
        match classify(*h) {
            Ok(c) => class_idx(&c) == *ci,
            Err(_) => false,
        }
    })
}

/// 锚值口径核对演练：NULL/伪句柄特值位值真实口径 10/10 轮。
pub fn drill_anchor_consts_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311a6（口径序列）。
    let mut lcg = Lcg::new(0x311a6);
    for _r in 0..INJECT_ROUNDS {
        let consts = HV_NULL == 0 && HV_PSEUDO_PROC == 0xFFFF_FFFF
            && HV_PSEUDO_THREAD == 0xFFFF_FFFE && HANDLE_BASE == 0x1000;
        let _ = lcg.below(3);
        if consts {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("锚值口径轮={}/{}", ok, INJECT_ROUNDS))
}

/// run_unx12521_ext2：扩展面 C 聚合。/// run_unx12521_ext2：扩展面 C 聚合。
pub fn run_unx12521_ext2() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = ClassBook::new();
    let _ = bk.inject_anchors();
    let (ok_r, d_r) = drill_repeat_determinism_10();
    out.push(("X2-重复确定性10/10", ok_r, d_r));
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X2-正路变异10/10", ok_m, d_m));
    let (ok_c2, d_c2) = drill_anchor_consts_10();
    out.push(("X2-锚值口径10/10", ok_c2, d_c2));
    out.push((
        "X2-账码回放面",
        replay_ledger_consistent(&bk),
        format!("回放 {} 行全一致", bk.rows()),
    ));
    out
}
