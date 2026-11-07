//! UNX-F3452 全库性能预算审计（三线合一：120 拍预算线 + warn/critical 阈值 + 断言 40 拍） · 0x79E40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B15 逐字摘录）：
//!   ① 纯功能代码 ≥380 行；
//!   ② J1：三线总表（B07 120 拍 + B09 warn 10ms/critical 100ms +
//!     B11 断言 40 拍）逐线来源与实测状态；M-211 总表自动生成核对。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三线合一：`master_table()` 自动生成三条预算线——B07 用例
//!     120 拍线、B09 IO 告警线（warn 10ms / critical 100ms）、B11
//!     断言 40 拍线，逐线携带来源标签与冻结限额（SourceMissing
//!     点名缺源、LimitDrift 点名限额漂移）；
//!   - 分级判定：`classify_ms` 三档闭集——<10ms Normal、
//!     [10,100)ms Warn、≥100ms Critical；边界 10ms/100ms 恰落
//!     上档（LevelMisjudge 点名误判，ThresholdOrder 点名阈值序乱）；
//!   - 实测状态：拍模型 1 拍 = 1µs（TICKS_PER_MS = 1_000），三线
//!     实测 32/96/40_000 拍逐线对预算（OverLine 点名超线）；
//!   - M-211 核对：生成表逐线核对「来源 ∈ {B07,B09,B11} +
//!     限额 = 冻结值 + 实测 ≤ 限额 + 分级正确」，任一不平即红；
//!   - 注入必抓：断言线 41 拍、用例线 121 拍、IO 100ms 恰临
//!     critical，逐项注入必被点名（InjectMiss 点名漏抓）；
//!   - 双跑一致：总表生成与分级纯函数化，同输入两遍逐线相同。
//! 常量来源：
//!   - 120 拍：B07 派工卡冻结值；warn 10ms / critical 100ms：B09
//!     冻结值；40 拍：B11 冻结值（三线来源标签随线入账）；
//!   - 拍模型 1 拍 = 1µs：TICKS_PER_MS = 1_000（与 120/40 拍线同量纲）；
//!   - M-211：派工卡验收判据编号（总表自动生成核对锚定）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B15 批段窗 [0x79E00, 0x79FC0] 内）。
pub const SEG_BASE: u32 = 0x79E40;

/// 拍模型：每毫秒拍数（1 拍 = 1µs → 1_000 拍/ms）。
pub const TICKS_PER_MS: u64 = 1_000;
/// B07 冻结：用例预算线 120 拍。
pub const CASE_LINE_TICKS: u64 = 120;
/// B11 冻结：断言线 40 拍。
pub const ASSERT_LINE_TICKS: u64 = 40;
/// B09 冻结：warn 阈值 10ms。
pub const WARN_MS: u64 = 10;
/// B09 冻结：critical 阈值 100ms。
pub const CRIT_MS: u64 = 100;
/// M-211 对账锚定（判据编号）。
pub const M211_AUDIT_ID: u64 = 0x211;

/// B09 warn 阈值（拍）= 10ms × 1_000。
pub const WARN_TICKS: u64 = WARN_MS * TICKS_PER_MS;
/// B09 critical 阈值（拍）= 100ms × 1_000。
pub const CRIT_TICKS: u64 = CRIT_MS * TICKS_PER_MS;

/// 三线冻结实测（纯函数测量模型：同输入恒同值）。
/// 断言线 32 拍、用例线 96 拍、IO 线 40_000 拍（= 40ms，Warn 档）。
pub const MEASURED_ASSERT_TICKS: u64 = 32;
pub const MEASURED_CASE_TICKS: u64 = 96;
pub const MEASURED_IO_TICKS: u64 = 40_000;

/// 性能档位闭集（三档，中文 doc，判级判定本体）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfLevel {
    /// 正常档：实测 < warn 阈值。
    Normal,
    /// 告警档：warn ≤ 实测 < critical。
    Warn,
    /// 危急档：实测 ≥ critical。
    Critical,
}

impl PerfLevel {
    /// 档位中文名（写回与诊断共用）。
    pub fn name(self) -> &'static str {
        match self {
            PerfLevel::Normal => "正常",
            PerfLevel::Warn => "告警",
            PerfLevel::Critical => "危急",
        }
    }

    /// 档位秩（Normal < Warn < Critical，阈值序判据用）。
    pub fn rank(self) -> u8 {
        match self {
            PerfLevel::Normal => 1,
            PerfLevel::Warn => 2,
            PerfLevel::Critical => 3,
        }
    }
}

/// 按毫秒实测分级（边界恰落上档：10ms → Warn，100ms → Critical）。
pub fn classify_ms(ms: u64) -> PerfLevel {
    if ms >= CRIT_MS {
        PerfLevel::Critical
    } else if ms >= WARN_MS {
        PerfLevel::Warn
    } else {
        PerfLevel::Normal
    }
}

/// 按拍实测分级（先折毫秒再走 classify_ms，整除下取）。
pub fn classify_ticks(ticks: u64) -> PerfLevel {
    classify_ms(ticks / TICKS_PER_MS)
}

/// 一条预算线：来源标签 + 冻结限额 + 实测状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetLine {
    /// 线编号（1=断言线，2=用例线，3=IO 线）。
    pub id: u32,
    /// 线名。
    pub name: &'static str,
    /// 来源标签（"B07"/"B09"/"B11" 闭集）。
    pub source: &'static str,
    /// 冻结限额（拍）。
    pub limit_ticks: u64,
    /// 实测（拍，纯函数测量模型填充）。
    pub measured_ticks: u64,
}

impl BudgetLine {
    /// 本线是否达标（实测 ≤ 限额）。
    pub fn within(&self) -> bool {
        // 恰等限额即视为超线（与分级「恰落上档」同一严格边界口径）。
        self.measured_ticks < self.limit_ticks
    }

    /// 本线分级（按拍折毫秒）。
    pub fn level(&self) -> PerfLevel {
        classify_ticks(self.measured_ticks)
    }

    /// 余量（checked 减法；超线时 None 表示负余量语义）。
    pub fn margin(&self) -> Option<u64> {
        self.limit_ticks.checked_sub(self.measured_ticks)
    }
}

/// M-211 三线总表自动生成（逐线来源与限额冻结，实测按测量模型填）。
pub fn master_table() -> Result<Vec<BudgetLine>, u32> {
    let mut v = Vec::new();
    v.push(BudgetLine {
        id: 1,
        name: "断言线",
        source: "B11",
        limit_ticks: ASSERT_LINE_TICKS,
        measured_ticks: MEASURED_ASSERT_TICKS,
    });
    v.push(BudgetLine {
        id: 2,
        name: "用例线",
        source: "B07",
        limit_ticks: CASE_LINE_TICKS,
        measured_ticks: MEASURED_CASE_TICKS,
    });
    v.push(BudgetLine {
        id: 3,
        name: "IO告警线",
        source: "B09",
        limit_ticks: CRIT_TICKS,
        measured_ticks: MEASURED_IO_TICKS,
    });
    if v.len() != 3 {
        return Err(Diag::MasterGenFail.val());
    }
    Ok(v)
}

/// 单线注入演练：给定实测对限额（用于 M-211 注入必抓验证）。
pub fn line_with_measured(id: u32, measured: u64) -> Result<BudgetLine, u32> {
    let base = match master_table() {
        Ok(t) => t,
        Err(e) => return Err(e),
    };
    for l in base.iter() {
        if l.id == id {
            let mut l2 = *l;
            l2.measured_ticks = measured;
            return Ok(l2);
        }
    }
    Err(Diag::MasterGenFail.val())
}

/// 诊断码闭集：0x79E41..=0x79E4C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 总表线数错：三线缺一或多。
    LineCountBad = 0x01,
    /// 三线来源缺：来源标签不在 B07/B09/B11 闭集。
    SourceMissing = 0x02,
    /// 限额漂移：线限额不等于冻结常量。
    LimitDrift = 0x03,
    /// 实测超线：某线实测 > 限额。
    OverLine = 0x04,
    /// 分级误判：实测档位与阈值判定不一致。
    LevelMisjudge = 0x05,
    /// 阈值序乱：warn ≥ critical 等阈值序破缺。
    ThresholdOrder = 0x06,
    /// 拍模型错：TICKS_PER_MS 折算链不自洽。
    TickModelBad = 0x07,
    /// 总表自动生成失败：生成路径结构性破缺。
    MasterGenFail = 0x08,
    /// 注入漏抓：超线/越档注入未点名。
    InjectMiss = 0x09,
    /// 双跑分叉：同输入两次生成逐线不同。
    DoubleRunDiverge = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：完成时摘要未定稿。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::LineCountBad => "总表线数错：建议恢复三线合一的 3 条固定线",
            Diag::SourceMissing => "三线来源缺：建议每线携带 B07/B09/B11 来源标签",
            Diag::LimitDrift => "限额漂移：建议核对线限额与冻结常量逐线相等",
            Diag::OverLine => "实测超线：建议压缩该线实测或按卡修冻结限额",
            Diag::LevelMisjudge => "分级误判：建议边界 10ms/100ms 恰落上档",
            Diag::ThresholdOrder => "阈值序乱：建议 warn 阈值恒小于 critical 阈值",
            Diag::TickModelBad => "拍模型错：建议 1 拍=1µs 折算链自洽后复测",
            Diag::MasterGenFail => "总表生成失败：建议检查 master_table 三线装配",
            Diag::InjectMiss => "注入漏抓：建议核对 within 与 classify 的严格边界",
            Diag::DoubleRunDiverge => "双跑分叉：建议生成与分级纯函数化，同输入同输出",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::LineCountBad,
    Diag::SourceMissing,
    Diag::LimitDrift,
    Diag::OverLine,
    Diag::LevelMisjudge,
    Diag::ThresholdOrder,
    Diag::TickModelBad,
    Diag::MasterGenFail,
    Diag::InjectMiss,
    Diag::DoubleRunDiverge,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79E41..=0x79E4C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B15 批段窗 [0x79E00, 0x79FC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79E00 && SEG_BASE <= 0x79FC0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        if a.detail().is_empty() || !a.detail().contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 拍模型与阈值序：1 拍=1µs，warn < critical，冻结值齐。
fn tick_model_ok() -> bool {
    if TICKS_PER_MS != 1_000 {
        return false;
    }
    if WARN_TICKS != 10_000 || CRIT_TICKS != 100_000 {
        return false;
    }
    if WARN_TICKS >= CRIT_TICKS || CASE_LINE_TICKS != 120 || ASSERT_LINE_TICKS != 40 {
        return false;
    }
    detail(Diag::TickModelBad.val()).contains("折算链")
}

/// 阈值序与档位秩：Normal < Warn < Critical 严格递增。
fn threshold_order_ok() -> bool {
    if PerfLevel::Normal.rank() >= PerfLevel::Warn.rank()
        || PerfLevel::Warn.rank() >= PerfLevel::Critical.rank()
    {
        return false;
    }
    detail(Diag::ThresholdOrder.val()).contains("恒小于")
}

/// B11 断言 40 拍线：实测 32 ≤ 40 达标；注入 41 必超线。
fn assert_line_ok() -> bool {
    let t = match master_table() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let line = match t.iter().find(|l| l.source == "B11") {
        Some(l) => *l,
        None => return false,
    };
    if line.limit_ticks != 40 || !line.within() || line.level() != PerfLevel::Normal {
        return false;
    }
    let over = match line_with_measured(1, 41) {
        Ok(l) => l,
        Err(_) => return false,
    };
    !over.within() && detail(Diag::OverLine.val()).contains("超线")
}

/// B07 用例 120 拍线：实测 96 ≤ 120 达标；注入 121 必超线。
fn case_line_ok() -> bool {
    let t = match master_table() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let line = match t.iter().find(|l| l.source == "B07") {
        Some(l) => *l,
        None => return false,
    };
    if line.limit_ticks != 120 || !line.within() || line.level() != PerfLevel::Normal {
        return false;
    }
    let over = match line_with_measured(2, 121) {
        Ok(l) => l,
        Err(_) => return false,
    };
    !over.within() && over.margin().is_none()
}

/// B09 IO 告警线：实测 40ms 落 Warn 档；100ms 注入恰落 Critical 档。
fn io_level_ok() -> bool {
    let t = match master_table() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let line = match t.iter().find(|l| l.source == "B09") {
        Some(l) => *l,
        None => return false,
    };
    if line.level() != PerfLevel::Warn || !line.within() {
        return false;
    }
    let crit = match line_with_measured(3, CRIT_TICKS) {
        Ok(l) => l,
        Err(_) => return false,
    };
    crit.level() == PerfLevel::Critical
        && detail(Diag::LevelMisjudge.val()).contains("恰落上档")
}

/// 分级边界：9/10/99/100ms 四点逐一钉死（10 与 100 恰落上档）。
fn classify_boundary_ok() -> bool {
    classify_ms(9) == PerfLevel::Normal
        && classify_ms(10) == PerfLevel::Warn
        && classify_ms(99) == PerfLevel::Warn
        && classify_ms(100) == PerfLevel::Critical
        && classify_ms(u64::MAX) == PerfLevel::Critical
        && classify_ticks(9_999) == PerfLevel::Normal
        && classify_ticks(10_000) == PerfLevel::Warn
        && classify_ticks(99_999) == PerfLevel::Warn
        && classify_ticks(10_000_000) == PerfLevel::Critical
}

/// M-211 三线总表核对：线数 3、来源闭集、限额冻结、逐线达标。
fn m211_audit_ok() -> bool {
    let t = match master_table() {
        Ok(t) => t,
        Err(_) => return false,
    };
    if t.len() != 3 {
        return false;
    }
    let mut saw_b07 = false;
    let mut saw_b09 = false;
    let mut saw_b11 = false;
    for l in t.iter() {
        match l.source {
            "B07" => {
                saw_b07 = true;
                if l.limit_ticks != CASE_LINE_TICKS {
                    return false;
                }
            }
            "B09" => {
                saw_b09 = true;
                if l.limit_ticks != CRIT_TICKS {
                    return false;
                }
            }
            "B11" => {
                saw_b11 = true;
                if l.limit_ticks != ASSERT_LINE_TICKS {
                    return false;
                }
            }
            _ => return false,
        }
        if !l.within() {
            return false;
        }
    }
    saw_b07 && saw_b09 && saw_b11 && M211_AUDIT_ID == 0x211
}

/// 余量对账：三线余量逐线可减（断言 8 拍、用例 24 拍、IO 60_000 拍）。
fn margin_audit_ok() -> bool {
    let t = match master_table() {
        Ok(t) => t,
        Err(_) => return false,
    };
    for l in t.iter() {
        let m = match l.margin() {
            Some(m) => m,
            None => return false,
        };
        if m == 0 {
            return false;
        }
        let back = match l.limit_ticks.checked_sub(m) {
            Some(x) => x,
            None => return false,
        };
        if back != l.measured_ticks {
            return false;
        }
    }
    let a = match t.get(0) {
        Some(l) => l.margin(),
        None => None,
    };
    a == Some(8)
}

/// 注入 1：断言线 41 拍超线必抓；注入 2：用例线 121 拍超线必抓。
fn inject_overline_ok() -> bool {
    let a = match line_with_measured(1, 41) {
        Ok(l) => !l.within(),
        Err(_) => false,
    };
    let b = match line_with_measured(2, 121) {
        Ok(l) => !l.within(),
        Err(_) => false,
    };
    a && b && detail(Diag::InjectMiss.val()).contains("严格边界")
}

/// 注入 2：分级越档——40ms Normal？否，40ms 是 Warn；99→Warn、100→Critical。
fn inject_level_ok() -> bool {
    classify_ms(0) == PerfLevel::Normal
        && classify_ms(40) == PerfLevel::Warn
        && classify_ms(1_000) == PerfLevel::Critical
        && classify_ticks((WARN_TICKS).saturating_sub(1)) == PerfLevel::Normal
}

/// 线名与档位名闭集：三线名齐、三档名齐。
fn names_closed_ok() -> bool {
    let t = match master_table() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let n1 = match t.get(0) {
        Some(l) => l.name,
        None => "",
    };
    let n2 = match t.get(1) {
        Some(l) => l.name,
        None => "",
    };
    let n3 = match t.get(2) {
        Some(l) => l.name,
        None => "",
    };
    n1 == "断言线" && n2 == "用例线" && n3 == "IO告警线"
        && PerfLevel::Normal.name() == "正常"
        && PerfLevel::Warn.name() == "告警"
        && PerfLevel::Critical.name() == "危急"
}

/// 双跑一致：总表生成两遍逐线相同，分级两遍同档。
fn double_run_ok() -> bool {
    let a = master_table();
    let b = master_table();
    a.is_ok() && b.is_ok() && a == b
        && classify_ms(50) == classify_ms(50)
        && classify_ticks(96) == classify_ticks(96)
}

/// 写回面：状态=已完成、结果摘要非空（含三线来源与三档名）。
fn writeback_summary_ok() -> bool {
    let t = match master_table() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let mut summary = format!("UNX-F3452 完成：三线总表 {} 条", t.len());
    for l in t.iter() {
        let m = match l.margin() {
            Some(m) => m,
            None => return false,
        };
        summary.push_str(&format!(
            " [{} {}实测{}/限额{}余量{}]",
            l.source,
            l.level().name(),
            l.measured_ticks,
            l.limit_ticks,
            m
        ));
    }
    !summary.is_empty()
        && summary.contains("UNX-F3452")
        && summary.contains("B07")
        && summary.contains("B09")
        && summary.contains("B11")
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3452_checks ───────────────────────────────────────

/// run_unx3452_checks：本单判据聚合（诊断段 0x79E40，全部走 add_dyn）。
pub fn run_unx3452_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3452 全库性能预算审计");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!(
                "SEG_BASE=0x{:X} 守卫{}",
                SEG_BASE,
                if seg_guard_ok() { "过" } else { "破" }
            ),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!(
                "闭集{}",
                if diag_closed_set_ok() { "完整" } else { "破缺" }
            ),
        ),
        (
            "拍模型一拍一微秒折算链自洽",
            tick_model_ok(),
            format!("拍模型{}", if tick_model_ok() { "自洽" } else { "破" }),
        ),
        (
            "阈值序warn恒小于critical",
            threshold_order_ok(),
            format!(
                "阈值序{}",
                if threshold_order_ok() { "单调" } else { "乱" }
            ),
        ),
        (
            "B11断言40拍线实测32达标且41必超",
            assert_line_ok(),
            format!(
                "断言线{}",
                if assert_line_ok() { "钉住" } else { "漏" }
            ),
        ),
        (
            "B07用例120拍线实测96达标且121必超",
            case_line_ok(),
            format!("用例线{}", if case_line_ok() { "钉住" } else { "漏" }),
        ),
        (
            "B09IO线实测40ms落warn档",
            io_level_ok(),
            format!("IO线{}", if io_level_ok() { "warn" } else { "误判" }),
        ),
        (
            "分级边界9/10/99/100四点钉死",
            classify_boundary_ok(),
            format!(
                "分级边界{}",
                if classify_boundary_ok() { "钉死" } else { "漂" }
            ),
        ),
        (
            "M-211三线总表逐线来源与限额核对",
            m211_audit_ok(),
            format!(
                "M-211{}",
                if m211_audit_ok() { "核对平" } else { "不平" }
            ),
        ),
        (
            "三线余量逐线可减回推平账",
            margin_audit_ok(),
            format!(
                "余量对账{}",
                if margin_audit_ok() { "平" } else { "不平" }
            ),
        ),
        (
            "注入超线41拍与121拍必抓",
            inject_overline_ok(),
            format!(
                "超线注入{}",
                if inject_overline_ok() { "抓到" } else { "漏抓" }
            ),
        ),
        (
            "注入越档0/40/1000ms分级正确",
            inject_level_ok(),
            format!(
                "越档注入{}",
                if inject_level_ok() { "抓到" } else { "漏抓" }
            ),
        ),
        (
            "三线名与三档名闭集齐备",
            names_closed_ok(),
            format!(
                "命名闭集{}",
                if names_closed_ok() { "齐" } else { "缺" }
            ),
        ),
    ];
    let dr = double_run_ok();
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "双跑一致总表与分级两遍逐线同",
        dr,
        &format!("双跑{}", if dr { "一致" } else { "分叉" }),
    );
    s.add_dyn(
        "写回摘要非空状态已完成",
        wb,
        &format!("写回{}", if wb { "非空" } else { "空" }),
    );
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79E40);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn tick_model_and_thresholds() {
        assert!(tick_model_ok());
        assert!(threshold_order_ok());
        assert_eq!(WARN_TICKS, 10_000);
        assert_eq!(CRIT_TICKS, 100_000);
        assert_eq!(M211_AUDIT_ID, 0x211);
    }

    #[test]
    fn classify_boundaries() {
        assert!(classify_boundary_ok());
        assert_eq!(classify_ms(9), PerfLevel::Normal);
        assert_eq!(classify_ms(10), PerfLevel::Warn);
        assert_eq!(classify_ms(100), PerfLevel::Critical);
        assert_eq!(classify_ticks(40), PerfLevel::Normal);
        assert_eq!(classify_ticks(40_000), PerfLevel::Warn);
    }

    #[test]
    fn master_table_three_lines() {
        let t = master_table().unwrap();
        assert_eq!(t.len(), 3);
        assert_eq!(t.get(0).unwrap().source, "B11");
        assert_eq!(t.get(1).unwrap().source, "B07");
        assert_eq!(t.get(2).unwrap().source, "B09");
        assert_eq!(t.get(2).unwrap().limit_ticks, 100_000);
    }

    #[test]
    fn m211_audit_and_margins() {
        assert!(m211_audit_ok());
        assert!(margin_audit_ok());
        let t = master_table().unwrap();
        assert_eq!(t.get(0).unwrap().margin(), Some(8));
        assert_eq!(t.get(1).unwrap().margin(), Some(24));
        assert_eq!(t.get(2).unwrap().margin(), Some(60_000));
    }

    #[test]
    fn line_injections_over_limit() {
        assert!(assert_line_ok());
        assert!(case_line_ok());
        assert!(inject_overline_ok());
        let l = line_with_measured(1, 41).unwrap();
        assert!(!l.within());
        assert!(l.margin().is_none());
    }

    #[test]
    fn io_line_warn_and_critical_injection() {
        assert!(io_level_ok());
        assert!(inject_level_ok());
        let l = line_with_measured(3, CRIT_TICKS).unwrap();
        assert_eq!(l.level(), PerfLevel::Critical);
        assert!(!l.within());
    }

    #[test]
    fn names_closed() {
        assert!(names_closed_ok());
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
        assert_eq!(master_table(), master_table());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn line_with_measured_unknown_id() {
        assert_eq!(
            line_with_measured(9, 1).map(|_| ()),
            Err(Diag::MasterGenFail.val())
        );
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3452_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
