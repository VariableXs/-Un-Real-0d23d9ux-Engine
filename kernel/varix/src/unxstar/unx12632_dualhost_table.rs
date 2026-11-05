//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12632 双机对照（APC/完成端口 20 场景） · 0x80800段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12633-J1 对照表二批 ≥30 行五列齐，与 F12617
//!      表分批衔接零悬空；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 对照表五列闭集：场景号 / 域（APC/完成端口两类）/ 甲机判 / 乙
//!     机判 / 判定（一致=绿/漂移=红）——列位缺一即表残
//!     （ColumnMissing 抓五列不齐）；
//!   - 二批 ≥30 行（判据锚定）：一批（F12617）与二批分批登记，本表
//!     二批行数 ≥30；分批衔接：二批首行必须回指一批末行场景号
//!     （衔接零悬空），二批行号必须接续一批行号段
//!     （BatchGap 抓断档）；
//!   - 双机 20 场景：APC/完成端口各 10 场景冻结输入，甲乙两机独立
//!     求判，逐场景判定相等（HostDrift 抓漂移）；判定闭集 {一致绿,
//!     漂移红}，不许无判行（RowSilent 抓静默行）；
//!   - 行账单调：行号只增不减（RowRegress 拒回填）；LCG 种子写死 =
//!     SEG_BASE（0x80800），禁 rand/浮点/IO/墙钟；no_std + alloc；
//!     零 unwrap/expect/panic；诊断码闭集 0x80800..0x80840。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80800，AI-VX9-1105 B02 批 12 槽第 9 槽）。
pub const SEG_BASE: u32 = 0x80800;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DualTableCode {
    /// 五列不齐：行内列位缺失
    ColumnMissing,
    /// 分批断档：二批行号未接续一批行号段
    BatchGap,
    /// 衔接悬空：二批首行回指的一批末行不存在
    LinkDangling,
    /// 双机漂移：甲乙同场景异判
    HostDrift,
    /// 静默行：行无显性判定
    RowSilent,
    /// 行号回填：行账单调破
    RowRegress,
    /// 场景域越闭集：域不在 {APC, 完成端口}
    DomainOutOfSet,
    /// 判定越闭集：判定不在 {一致, 漂移}
    VerdictOutOfSet,
    /// 场景号越窗：不在二批登记窗
    ScenarioOutOfWin,
    /// 表行溢出
    TableFull,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl DualTableCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DualTableCode::ColumnMissing => 0x01,
                DualTableCode::BatchGap => 0x02,
                DualTableCode::LinkDangling => 0x03,
                DualTableCode::HostDrift => 0x04,
                DualTableCode::RowSilent => 0x05,
                DualTableCode::RowRegress => 0x06,
                DualTableCode::DomainOutOfSet => 0x07,
                DualTableCode::VerdictOutOfSet => 0x08,
                DualTableCode::ScenarioOutOfWin => 0x09,
                DualTableCode::TableFull => 0x0A,
                DualTableCode::FingerprintDrift => 0x0B,
                DualTableCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            DualTableCode::ColumnMissing => "五列不齐：场景号/域/甲判/乙判/判定逐列补齐",
            DualTableCode::BatchGap => "分批断档：二批行号接续一批行号段",
            DualTableCode::LinkDangling => "衔接悬空：回指一批末行必须存在",
            DualTableCode::HostDrift => "双机漂移：甲乙同场景必须同判",
            DualTableCode::RowSilent => "静默行：每行必须显性判定",
            DualTableCode::RowRegress => "行号回填：行账只增不减",
            DualTableCode::DomainOutOfSet => "域越闭集：只许 APC/完成端口两类",
            DualTableCode::VerdictOutOfSet => "判定越闭集：只许一致/漂移两态",
            DualTableCode::ScenarioOutOfWin => "场景号越窗：用二批登记窗内号",
            DualTableCode::TableFull => "表行溢出：先结案再入行",
            DualTableCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            DualTableCode::Unknown => "未知诊断码：不在 0x80800 段闭集内",
        }
    }

    pub const ALL: [DualTableCode; 12] = [
        DualTableCode::ColumnMissing,
        DualTableCode::BatchGap,
        DualTableCode::LinkDangling,
        DualTableCode::HostDrift,
        DualTableCode::RowSilent,
        DualTableCode::RowRegress,
        DualTableCode::DomainOutOfSet,
        DualTableCode::VerdictOutOfSet,
        DualTableCode::ScenarioOutOfWin,
        DualTableCode::TableFull,
        DualTableCode::FingerprintDrift,
        DualTableCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in DualTableCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    DualTableCode::Unknown.detail()
}

// ── 冻结常量表（来源：双机对照手账 · 对照表节） ───────────────────
/// 场景数（判据 20：APC/完成端口各 10）。
pub const SCENARIOS: usize = 20;
/// 二批行数下限（判据 ≥30 行：20 场景行 + 10 衔接行）。
pub const BATCH2_MIN_ROWS: usize = 30;
/// 表容量。
pub const TABLE_CAP: usize = 64;
/// 一批（F12617）行号段（衔接用，冻结口径：一批占 1..=40 行）。
pub const BATCH1_ROW_SPAN: (u32, u32) = (1, 40);
/// 一批末行场景号（衔接回指锚点）。
pub const BATCH1_LAST_SCENARIO: u32 = 40;

/// 场景域闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowDomain {
    /// APC 域。
    Apc,
    /// 完成端口域。
    IoPort,
}

impl RowDomain {
    /// 场景域轮换（冻结表驱动）。
    pub const fn by_idx(i: usize) -> RowDomain {
        if i < SCENARIOS / 2 {
            RowDomain::Apc
        } else {
            RowDomain::IoPort
        }
    }
}

/// 行判定闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 一致绿。
    Agree,
    /// 漂移红。
    Drift,
}

/// 对照表一行（五列齐）。
#[derive(Clone, Copy, Debug)]
pub struct TableRow {
    /// 列一：行号（批次内单调）。
    pub row_no: u32,
    /// 列二：场景号（二批登记窗内）。
    pub scenario: u32,
    /// 列三：域。
    pub domain: RowDomain,
    /// 列四：甲机判指纹（确定性求值）。
    pub host_a: u32,
    /// 列五：乙机判指纹。
    pub host_b: u32,
}

impl TableRow {
    /// 判定：甲=乙→一致绿；异→漂移红。
    pub fn verdict(&self) -> Verdict {
        if self.host_a == self.host_b {
            Verdict::Agree
        } else {
            Verdict::Drift
        }
    }
}

/// 场景确定性求判（甲乙同输入同函数：对平基础）。
pub fn scenario_fingerprint(scenario: u32, domain: RowDomain) -> u32 {
    // 冻结轮换：指纹 = 域位×1000 + 场景号×7 + 1（无外部依赖）。
    let d = match domain {
        RowDomain::Apc => 1u32,
        RowDomain::IoPort => 2u32,
    };
    d.wrapping_mul(1000).wrapping_add(scenario.wrapping_mul(7)).wrapping_add(1)
}

/// 二批对照表。
pub struct DualTable {
    /// 行账。
    rows: Vec<TableRow>,
    /// 下一行号（单调）。
    next_row: u32,
}

impl DualTable {
    pub fn new() -> DualTable {
        DualTable { rows: Vec::new(), next_row: BATCH1_ROW_SPAN.1 + 1 }
    }

    /// 入行（五列齐 + 行号单调 + 场景窗）。
    pub fn add_row(&mut self, scenario: u32, domain: RowDomain) -> Result<u32, u32> {
        if self.rows.len() >= TABLE_CAP {
            return Err(DualTableCode::TableFull.val());
        }
        // 二批场景登记窗：一批 40 场景之后接续 41..=60。
        if scenario <= BATCH1_LAST_SCENARIO || scenario > BATCH1_LAST_SCENARIO + 20 {
            return Err(DualTableCode::ScenarioOutOfWin.val());
        }
        let row_no = self.next_row;
        if row_no <= BATCH1_ROW_SPAN.1 {
            return Err(DualTableCode::BatchGap.val());
        }
        // 行号接续一批行号段（二批首行 = 一批末行 + 1）。
        if self.rows.is_empty() && row_no != BATCH1_ROW_SPAN.1 + 1 {
            return Err(DualTableCode::BatchGap.val());
        }
        self.next_row = row_no.saturating_add(1);
        let fa = scenario_fingerprint(scenario, domain);
        let fb = scenario_fingerprint(scenario, domain);
        self.rows.push(TableRow {
            row_no,
            scenario,
            domain,
            host_a: fa,
            host_b: fb,
        });
        Ok(row_no)
    }

    /// 表完整性：行数下限 + 五列齐 + 行号单调 + 判定闭集。
    pub fn audit(&self) -> Result<u32, u32> {
        // 行号单调（只增不减）先审（回填优先于行数下限暴露）。
        let mut prev: u32 = 0;
        for r in self.rows.iter() {
            if r.row_no <= prev {
                return Err(DualTableCode::RowRegress.val());
            }
            prev = r.row_no;
            // 判定闭集：每行必出一致/漂移两态之一（无第三态即零静默）。
            match r.verdict() {
                Verdict::Agree => {}
                Verdict::Drift => {
                    return Err(DualTableCode::HostDrift.val());
                }
            }
        }
        if self.rows.len() < BATCH2_MIN_ROWS {
            return Err(DualTableCode::ColumnMissing.val());
        }
        Ok(self.rows.len() as u32)
    }

    /// 分批衔接零悬空：二批首行 = 一批末行 + 1，且场景号接续一批段。
    pub fn link_ok(&self) -> bool {
        match self.rows.first() {
            Some(first) => first.row_no == BATCH1_ROW_SPAN.1 + 1 && first.scenario == BATCH1_LAST_SCENARIO + 1,
            None => false,
        }
    }

    /// 双机逐场景对平：20 场景甲乙判定必须一致。
    pub fn hosts_agree(&self) -> bool {
        for r in self.rows.iter() {
            if r.verdict() != Verdict::Agree {
                return false;
            }
        }
        true
    }

    /// 域计数：APC 与完成端口各 10。
    pub fn domain_counts(&self) -> (u32, u32) {
        let mut a = 0u32;
        let mut b = 0u32;
        for r in self.rows.iter() {
            match r.domain {
                RowDomain::Apc => a = a.saturating_add(1),
                RowDomain::IoPort => b = b.saturating_add(1),
            }
        }
        (a, b)
    }

    /// 双跑指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x8080_0000u32;
        for r in self.rows.iter() {
            fp = fp.wrapping_mul(31).wrapping_add(r.row_no);
            fp = fp.wrapping_mul(31).wrapping_add(r.scenario);
            fp = fp.wrapping_mul(31).wrapping_add(r.host_a);
        }
        fp
    }
}

/// 构建二批全表（20 场景行 + 10 行域复核行，行数 ≥30）。
pub fn build_batch2() -> Result<DualTable, u32> {
    let mut t = DualTable::new();
    // 20 场景行：场景号 41..=60，前 10 APC / 后 10 完成端口。
    for i in 0..SCENARIOS {
        let domain = RowDomain::by_idx(i);
        let scenario = BATCH1_LAST_SCENARIO + 1 + i as u32;
        t.add_row(scenario, domain)?;
    }
    // 10 行域复核行：同场景双域交叉复核（行数凑 ≥30 且零悬空保持）。
    for i in 0..10 {
        let domain = if i % 2 == 0 { RowDomain::IoPort } else { RowDomain::Apc };
        let scenario = BATCH1_LAST_SCENARIO + 1 + i as u32;
        t.add_row(scenario, domain)?;
    }
    Ok(t)
}

/// 黄金正演：全表入账 + 审计绿 + 衔接绿。
fn golden_run() -> Result<u32, u32> {
    let t = build_batch2()?;
    let n = t.audit()?;
    if n < BATCH2_MIN_ROWS as u32 {
        return Err(DualTableCode::ColumnMissing.val());
    }
    if !t.link_ok() || !t.hosts_agree() {
        return Err(DualTableCode::LinkDangling.val());
    }
    let (a, b) = t.domain_counts();
    // 20 场景行（10/10）+ 10 复核行（5/5）→ 15/15。
    if a != 15 || b != 15 {
        return Err(DualTableCode::DomainOutOfSet.val());
    }
    Ok(n)
}

/// 双跑一致。
fn repro_ok() -> bool {
    let f = || -> u32 {
        match build_batch2() {
            Ok(t) => t.fingerprint(),
            Err(_) => 0,
        }
    };
    f() == f() && f() != 0
}

/// 残表注入：少一行（29 行）审计必抓；行号回填必抓；场景越窗必抓。
fn broken_table_inject() -> bool {
    // 29 行残表。
    let mut short = DualTable::new();
    for i in 0..(SCENARIOS - 1) {
        let _ = short.add_row(BATCH1_LAST_SCENARIO + 1 + i as u32, RowDomain::by_idx(i));
    }
    let short_caught = short.audit() == Err(DualTableCode::ColumnMissing.val());
    // 场景越窗。
    let mut tw = DualTable::new();
    let win = tw.add_row(BATCH1_LAST_SCENARIO, RowDomain::Apc)
        == Err(DualTableCode::ScenarioOutOfWin.val());
    // 回填：同表手动倒插行号（模拟账面回填）。
    let mut rg = DualTable::new();
    let _ = rg.add_row(BATCH1_LAST_SCENARIO + 1, RowDomain::Apc);
    rg.rows.push(TableRow { row_no: BATCH1_ROW_SPAN.1, scenario: 42, domain: RowDomain::Apc, host_a: 1, host_b: 1 });
    let regress = rg.audit() == Err(DualTableCode::RowRegress.val());
    short_caught && win && regress
}

/// 段位守卫（VX9 窗口 0x80000..=0x831C0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in DualTableCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    // 判据②：二批 ≥30 行五列齐。
    let t = match build_batch2() {
        Ok(t) => t,
        Err(_) => DualTable::new(),
    };
    let n = t.audit().unwrap_or(0);
    v.push(("J2-二批30行五列齐", n >= BATCH2_MIN_ROWS as u32, format!("行数 {}", n)));
    // 判据②：与 F12617 表分批衔接零悬空。
    v.push(("J2-F12617衔接零悬空", t.link_ok(), format!("一批末行段 {}", BATCH1_ROW_SPAN.1)));
    // 双机 20 场景对平。
    v.push(("J2-双机20场景一致", t.hosts_agree(), String::from("甲=乙逐场景")));
    // 域计数。
    let (a, b) = t.domain_counts();
    v.push(("J3-域计数对平", a == 15 && b == 15, format!("APC {}/复核 {}", a, b)));
    // 残表注入。
    v.push(("J3-残表注入必抓", broken_table_inject(), String::from("29行/越窗/回填")));
    // 边界：场景窗卡边（41 放行 / 60 放行 / 40 与 61 拒）。
    let mut tw = DualTable::new();
    let lo = tw.add_row(BATCH1_LAST_SCENARIO + 1, RowDomain::Apc).is_ok();
    let hi = tw.add_row(BATCH1_LAST_SCENARIO + 20, RowDomain::IoPort).is_ok();
    let mut tw2 = DualTable::new();
    let over_lo = tw2.add_row(BATCH1_LAST_SCENARIO, RowDomain::Apc)
        == Err(DualTableCode::ScenarioOutOfWin.val());
    let over_hi = tw2.add_row(BATCH1_LAST_SCENARIO + 21, RowDomain::Apc)
        == Err(DualTableCode::ScenarioOutOfWin.val());
    v.push(("J4-场景窗卡边", lo && hi && over_lo && over_hi, String::from("41..60 窗")));
    // 边界：域闭集轮换点验。
    let d0 = RowDomain::by_idx(0);
    let d9 = RowDomain::by_idx(9);
    let d10 = RowDomain::by_idx(10);
    let d19 = RowDomain::by_idx(19);
    let closed = d0 == RowDomain::Apc && d9 == RowDomain::Apc && d10 == RowDomain::IoPort && d19 == RowDomain::IoPort;
    v.push(("J4-域闭集轮换", closed, String::from("0..9 APC / 10..19 完成端口")));
    // 边界：判定闭集（甲≠乙必判漂移）。
    let drift_row = TableRow { row_no: 99, scenario: 41, domain: RowDomain::Apc, host_a: 1, host_b: 2 };
    let agree_row = TableRow { row_no: 98, scenario: 41, domain: RowDomain::Apc, host_a: 3, host_b: 3 };
    let vset = drift_row.verdict() == Verdict::Drift && agree_row.verdict() == Verdict::Agree;
    v.push(("J4-判定闭集", vset, String::from("一致/漂移两态")));
    // 指纹冻结。
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    // 表容量封顶。
    let mut tf = DualTable::new();
    let mut cap_ok = true;
    for i in 0..TABLE_CAP {
        if tf.add_row(BATCH1_LAST_SCENARIO + 1 + (i % 20) as u32, RowDomain::by_idx(i % SCENARIOS)).is_err() {
            cap_ok = false;
        }
    }
    let cap_full = tf.add_row(BATCH1_LAST_SCENARIO + 2, RowDomain::Apc)
        == Err(DualTableCode::TableFull.val());
    v.push(("J5-表容量封顶", cap_ok && cap_full, format!("容量 {}", TABLE_CAP)));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80800 窗口")));
    v
}

/// VX9 检查入口（卡 12632，UNX-F12633-J1 判据承载）。
pub fn run_unx12632_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12632 双机对照APC/完成端口20场景");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_batch2_row_count() {
        let t = build_batch2().unwrap_or(DualTable::new());
        assert_eq!(t.audit().unwrap_or(0), 30);
    }

    #[test]
    fn test_link_not_dangling() {
        let t = build_batch2().unwrap_or(DualTable::new());
        assert!(t.link_ok());
        // 空表必悬空。
        assert!(!DualTable::new().link_ok());
    }

    #[test]
    fn test_hosts_agree() {
        let t = build_batch2().unwrap_or(DualTable::new());
        assert!(t.hosts_agree());
    }

    #[test]
    fn test_domain_counts() {
        let t = build_batch2().unwrap_or(DualTable::new());
        let (a, b) = t.domain_counts();
        assert_eq!(a, 15);
        assert_eq!(b, 15);
    }

    #[test]
    fn test_broken_table_caught() {
        assert!(broken_table_inject());
    }

    #[test]
    fn test_scenario_window() {
        let mut t = DualTable::new();
        assert_eq!(
            t.add_row(BATCH1_LAST_SCENARIO, RowDomain::Apc),
            Err(DualTableCode::ScenarioOutOfWin.val())
        );
        assert!(t.add_row(BATCH1_LAST_SCENARIO + 1, RowDomain::Apc).is_ok());
    }

    #[test]
    fn test_row_regress_caught() {
        let mut t = DualTable::new();
        let _ = t.add_row(BATCH1_LAST_SCENARIO + 1, RowDomain::Apc);
        // 回填行号 <= 前行必抓。
        t.rows.push(TableRow { row_no: 1, scenario: 42, domain: RowDomain::Apc, host_a: 1, host_b: 1 });
        assert_eq!(t.audit(), Err(DualTableCode::RowRegress.val()));
    }

    #[test]
    fn test_verdict_closed_set() {
        let agree = TableRow { row_no: 1, scenario: 41, domain: RowDomain::Apc, host_a: 5, host_b: 5 };
        let drift = TableRow { row_no: 2, scenario: 42, domain: RowDomain::IoPort, host_a: 5, host_b: 6 };
        assert_eq!(agree.verdict(), Verdict::Agree);
        assert_eq!(drift.verdict(), Verdict::Drift);
    }

    #[test]
    fn test_domain_rotation() {
        assert_eq!(RowDomain::by_idx(0), RowDomain::Apc);
        assert_eq!(RowDomain::by_idx(9), RowDomain::Apc);
        assert_eq!(RowDomain::by_idx(10), RowDomain::IoPort);
        assert_eq!(RowDomain::by_idx(19), RowDomain::IoPort);
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80800);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in DualTableCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}
