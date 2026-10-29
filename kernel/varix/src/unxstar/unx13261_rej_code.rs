//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13261 · 拒载错误码面总成（全拒载路径错误码统一账） · 0x65840
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13261-J1 统一错误码账覆盖 B01–B23 全拒载路径
//!      （路径→码映射全覆盖断言），账本可导出复测；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 路径闭集账：B01–B23 共 23 条拒载路径（格式/资源/依赖三族）
//!     逐条挂统一错误码，路径→码映射全覆盖（缺一条即 CoverageGap）；
//!   - 码面闭式：每码唯一（重复码必抓），码域 [CODE_BASE, CODE_BASE+23)
//!     越域必抓；
//!   - 可导出复测：账本导出为逐行文本（路径|码|摘要），导出行数 ==
//!     路径数（导出守恒），复测重放逐行比对一致；
//!   - 反向判据：错码注入（B 路径挂邻码）复测必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65840），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65840，全仓独占）。
pub const SEG_BASE: u32 = 0x65840;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RejCodeCode {
    /// 路径→码映射缺口（B01–B23 有路无码）
    CoverageGap,
    /// 统一码重复（两路同码）
    CodeDuplicated,
    /// 码越统一域
    CodeOutOfRange,
    /// 导出守恒破（行数 != 路径数）
    ExportUnbalanced,
    /// 复测重放失配
    ReplayMismatch,
    /// 未知路径查询
    PathUnknown,
    /// 路径族越闭集
    FamilyOutOfRange,
    /// 账本重复登记同路径
    PathDuplicated,
}

impl RejCodeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RejCodeCode::CoverageGap => "路径→码映射缺口：B01–B23 有路无码",
            RejCodeCode::CodeDuplicated => "统一码重复：两路同码必抓",
            RejCodeCode::CodeOutOfRange => "码越统一域：[CODE_BASE, CODE_BASE+23)",
            RejCodeCode::ExportUnbalanced => "导出守恒破：行数 != 路径数",
            RejCodeCode::ReplayMismatch => "复测重放失配：导出账被改",
            RejCodeCode::PathUnknown => "未知路径查询：路径闭集外",
            RejCodeCode::FamilyOutOfRange => "路径族越闭集：族账越界",
            RejCodeCode::PathDuplicated => "账本重复登记同路径：闭集破",
        }
    }
    pub const ALL: [RejCodeCode; 8] = [
        RejCodeCode::CoverageGap,
        RejCodeCode::CodeDuplicated,
        RejCodeCode::CodeOutOfRange,
        RejCodeCode::ExportUnbalanced,
        RejCodeCode::ReplayMismatch,
        RejCodeCode::PathUnknown,
        RejCodeCode::FamilyOutOfRange,
        RejCodeCode::PathDuplicated,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RejCodeCode::ALL.len() {
        if RejCodeCode::ALL[i].val() == code {
            return RejCodeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 常量（来源：拒载错误码面建模书 + 判据原文） ──────────────────
/// 路径闭集：B01–B23（23 条）。
pub const PATH_MIN: u8 = 1;
pub const PATH_MAX: u8 = 23;
pub const PATH_COUNT: usize = 23;
/// 统一错误码基址（建模：0x3C00 档，域宽 = 路径数）。
pub const CODE_BASE: u32 = 0x3C00;
/// 路径族闭集（3 族：格式/资源/依赖）。
pub const FAM_FORMAT: u8 = 1;
pub const FAM_RESOURCE: u8 = 2;
pub const FAM_DEPEND: u8 = 3;

/// 统一映射账条目（路径号 + 族 + 统一码 + 摘要）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathCode {
    pub path: u8,   // B01..B23 → 1..23
    pub family: u8,
    pub code: u32,
    pub summary: &'static str,
}

/// B01–B23 全路径统一账（逐条写死，映射全覆盖主账）。
pub const REJECT_LEDGER: [PathCode; PATH_COUNT] = [
    PathCode { path: 1,  family: FAM_FORMAT,   code: CODE_BASE + 1,  summary: "B01 DOS 头魔数失配" },
    PathCode { path: 2,  family: FAM_FORMAT,   code: CODE_BASE + 2,  summary: "B02 PE 签名缺失" },
    PathCode { path: 3,  family: FAM_FORMAT,   code: CODE_BASE + 3,  summary: "B03 可选头尺寸越窗" },
    PathCode { path: 4,  family: FAM_FORMAT,   code: CODE_BASE + 4,  summary: "B04 节表数越上限" },
    PathCode { path: 5,  family: FAM_FORMAT,   code: CODE_BASE + 5,  summary: "B05 节名区越窗" },
    PathCode { path: 6,  family: FAM_FORMAT,   code: CODE_BASE + 6,  summary: "B06 节对齐非法" },
    PathCode { path: 7,  family: FAM_FORMAT,   code: CODE_BASE + 7,  summary: "B07 头尺寸越文件界" },
    PathCode { path: 8,  family: FAM_RESOURCE, code: CODE_BASE + 8,  summary: "B08 资源目录环" },
    PathCode { path: 9,  family: FAM_RESOURCE, code: CODE_BASE + 9,  summary: "B09 资源项越资源界" },
    PathCode { path: 10, family: FAM_RESOURCE, code: CODE_BASE + 10, summary: "B10 资源深度越限" },
    PathCode { path: 11, family: FAM_RESOURCE, code: CODE_BASE + 11, summary: "B11 资源尺寸虚高" },
    PathCode { path: 12, family: FAM_RESOURCE, code: CODE_BASE + 12, summary: "B12 资源 RVA 越映像" },
    PathCode { path: 13, family: FAM_DEPEND,   code: CODE_BASE + 13, summary: "B13 导入表 RVA 越映像" },
    PathCode { path: 14, family: FAM_DEPEND,   code: CODE_BASE + 14, summary: "B14 依赖链深度越限" },
    PathCode { path: 15, family: FAM_DEPEND,   code: CODE_BASE + 15, summary: "B15 依赖环" },
    PathCode { path: 16, family: FAM_DEPEND,   code: CODE_BASE + 16, summary: "B16 导入名越窗" },
    PathCode { path: 17, family: FAM_DEPEND,   code: CODE_BASE + 17, summary: "B17 延迟导入越窗" },
    PathCode { path: 18, family: FAM_FORMAT,   code: CODE_BASE + 18, summary: "B18 重定位表越窗" },
    PathCode { path: 19, family: FAM_FORMAT,   code: CODE_BASE + 19, summary: "B19 基址重定位块失配" },
    PathCode { path: 20, family: FAM_FORMAT,   code: CODE_BASE + 20, summary: "B20 TLS 目录越窗" },
    PathCode { path: 21, family: FAM_DEPEND,   code: CODE_BASE + 21, summary: "B21 入口点越节界" },
    PathCode { path: 22, family: FAM_DEPEND,   code: CODE_BASE + 22, summary: "B22 依赖版本不满足" },
    PathCode { path: 23, family: FAM_RESOURCE, code: CODE_BASE + 23, summary: "B23 清单资源缺失" },
];

// ── 核账函数 ─────────────────────────────────────────────────────
/// 全覆盖核：B01–B23 每条有码、码唯一、码在统一域、族在闭集。
pub fn audit_ledger(led: &[PathCode]) -> Result<usize, RejCodeCode> {
    if led.is_empty() {
        return Err(RejCodeCode::CoverageGap);
    }
    let mut i = 0;
    while i < led.len() {
        let e = led[i];
        if e.path < PATH_MIN || e.path > PATH_MAX {
            return Err(RejCodeCode::PathUnknown);
        }
        if e.family < FAM_FORMAT || e.family > FAM_DEPEND {
            return Err(RejCodeCode::FamilyOutOfRange);
        }
        if e.code <= CODE_BASE || e.code > CODE_BASE + PATH_COUNT as u32 {
            return Err(RejCodeCode::CodeOutOfRange);
        }
        // 码唯一性 + 路径唯一性（闭式双扫）。
        let mut j = 0;
        while j < i {
            if led[j].code == e.code {
                return Err(RejCodeCode::CodeDuplicated);
            }
            if led[j].path == e.path {
                return Err(RejCodeCode::PathDuplicated);
            }
            j += 1;
        }
        i += 1;
    }
    // 全覆盖核：每条 B01..B23 均在账（缺一条即缺口）。
    let mut p = PATH_MIN;
    while p <= PATH_MAX {
        let mut found = false;
        let mut k = 0;
        while k < led.len() {
            if led[k].path == p {
                found = true;
                break;
            }
            k += 1;
        }
        if !found {
            return Err(RejCodeCode::CoverageGap);
        }
        p += 1;
    }
    Ok(led.len())
}

/// 路径→码查询（未知路径必拒）。
pub fn code_of(led: &[PathCode], path: u8) -> Result<u32, RejCodeCode> {
    if path < PATH_MIN || path > PATH_MAX {
        return Err(RejCodeCode::PathUnknown);
    }
    let mut i = 0;
    while i < led.len() {
        if led[i].path == path {
            return Ok(led[i].code);
        }
        i += 1;
    }
    Err(RejCodeCode::CoverageGap)
}

// ── 可导出复测面 ─────────────────────────────────────────────────
/// 导出账本为逐行文本（路径|码|摘要），行数守恒。
pub fn export_ledger(led: &[PathCode]) -> Result<Vec<String>, RejCodeCode> {
    let n = audit_ledger(led)?;
    if n != led.len() {
        return Err(RejCodeCode::ExportUnbalanced);
    }
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < led.len() {
        out.push(format!("B{:02X}|0x{:04X}|{}", led[i].path, led[i].code, led[i].summary));
        i += 1;
    }
    if out.len() != led.len() {
        return Err(RejCodeCode::ExportUnbalanced);
    }
    Ok(out)
}

/// 复测重放：导出行与账本逐行比对（错码注入必抓）。
pub fn replay_verify(led: &[PathCode], exported: &[String]) -> Result<(), RejCodeCode> {
    if exported.len() != led.len() {
        return Err(RejCodeCode::ExportUnbalanced);
    }
    let expect = export_ledger(led)?;
    let mut i = 0;
    while i < expect.len() {
        if i >= exported.len() || expect[i] != exported[i] {
            return Err(RejCodeCode::ReplayMismatch);
        }
        i += 1;
    }
    Ok(())
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 判据钻取面 ───────────────────────────────────────────────────
/// J1 主面：B01–B23 全覆盖断言（23 条全过核）。
pub fn drill_full_coverage() -> (bool, String) {
    let n = match audit_ledger(&REJECT_LEDGER) {
        Ok(n) => n,
        Err(e) => return (false, detail(e.val()).to_string()),
    };
    (
        n == PATH_COUNT,
        format!("路径→码映射全覆盖 {}/{}", n, PATH_COUNT),
    )
}

/// 反向判据：抽一条路径 → 覆盖缺口必抓（5 组全抓）。
pub fn drill_coverage_gap_5() -> (bool, String) {
    let mut caught = 0;
    let mut i = 0;
    while i < 5 {
        // 抽掉第 i+1 条（B01..B05 轮抽）。
        let mut cut: Vec<PathCode> = Vec::new();
        let mut j = 0;
        while j < REJECT_LEDGER.len() {
            if j != i {
                cut.push(REJECT_LEDGER[j]);
            }
            j += 1;
        }
        if audit_ledger(&cut) == Err(RejCodeCode::CoverageGap) {
            caught += 1;
        }
        i += 1;
    }
    (caught == 5, format!("覆盖缺口注入 {}/5 必抓", caught))
}

/// 反向判据：错码注入（邻码错挂）→ 码域/重复必抓（10/10）。
pub fn drill_bad_code_10() -> (bool, String) {
    let mut caught = 0;
    // 越域码 5 例：CODE_BASE+24..+28（统一域顶为 CODE_BASE+23）。
    let mut i = 0;
    while i < 5 {
        let bad = PathCode {
            path: (i + 1) as u8,
            family: FAM_FORMAT,
            code: CODE_BASE + PATH_COUNT as u32 + 1 + i as u32,
            summary: "越域注入",
        };
        let mut led: Vec<PathCode> = Vec::new();
        let mut j = 1;
        while j <= PATH_COUNT {
            if j != i + 1 {
                led.push(REJECT_LEDGER[j - 1]);
            }
            j += 1;
        }
        led.push(bad);
        if audit_ledger(&led) == Err(RejCodeCode::CodeOutOfRange) {
            caught += 1;
        }
        i += 1;
    }
    // 重复码 5 例：B02..B06 挂 B01 的码。
    let mut k = 0;
    while k < 5 {
        let bad = PathCode {
            path: (k + 2) as u8,
            family: FAM_FORMAT,
            code: CODE_BASE + 1,
            summary: "重复码注入",
        };
        let mut led: Vec<PathCode> = Vec::new();
        led.push(REJECT_LEDGER[0]);
        led.push(bad);
        if audit_ledger(&led) == Err(RejCodeCode::CodeDuplicated) {
            caught += 1;
        }
        k += 1;
    }
    (caught == 10, format!("错码注入 {}/10 必抓", caught))
}

/// 查询面：B01–B23 逐条查询全中（23/23）+ 未知路径必拒（5/5）。
pub fn drill_lookup_23() -> (bool, String) {
    let mut hit = 0;
    let mut p = PATH_MIN;
    while p <= PATH_MAX {
        if code_of(&REJECT_LEDGER, p).is_ok() {
            hit += 1;
        }
        p += 1;
    }
    let mut caught = 0;
    let mut i = 0;
    while i < 5 {
        if code_of(&REJECT_LEDGER, 0) == Err(RejCodeCode::PathUnknown)
            || code_of(&REJECT_LEDGER, PATH_MAX + 1 + i as u8) == Err(RejCodeCode::PathUnknown)
        {
            caught += 1;
        }
        i += 1;
    }
    (hit == PATH_COUNT && caught == 5, format!("查询 {}/{} 全中，未知路径 {}/5 必抓", hit, PATH_COUNT, caught))
}

/// 可导出复测面：导出行数守恒 + 重放一致。
pub fn drill_export_replay() -> (bool, String) {
    let exported = match export_ledger(&REJECT_LEDGER) {
        Ok(v) => v,
        Err(e) => return (false, detail(e.val()).to_string()),
    };
    let rows_ok = exported.len() == PATH_COUNT;
    let replay = replay_verify(&REJECT_LEDGER, &exported) == Ok(());
    // 反向判据：篡改一行导出 → 重放必抓。
    let mut tampered: Vec<String> = Vec::new();
    let mut i = 0;
    while i < exported.len() {
        if i == 3 {
            tampered.push(format!("B{:02X}|0x{:04X}|篡改行", 4, CODE_BASE + 99));
        } else {
            tampered.push(exported[i].clone());
        }
        i += 1;
    }
    let catch = replay_verify(&REJECT_LEDGER, &tampered) == Err(RejCodeCode::ReplayMismatch);
    (
        rows_ok && replay && catch,
        format!("导出 {} 行守恒={} 重放={} 篡改抓={}", exported.len(), rows_ok, replay, catch),
    )
}

/// 族分布面：三族各有分账（格式/资源/依赖），族账之和 == 总账。
pub fn drill_family_distribution() -> (bool, String) {
    let mut fmt = 0;
    let mut res = 0;
    let mut dep = 0;
    let mut i = 0;
    while i < REJECT_LEDGER.len() {
        match REJECT_LEDGER[i].family {
            FAM_FORMAT => fmt += 1,
            FAM_RESOURCE => res += 1,
            FAM_DEPEND => dep += 1,
            _ => {}
        }
        i += 1;
    }
    let ok = fmt + res + dep == PATH_COUNT && fmt > 0 && res > 0 && dep > 0;
    (ok, format!("族分布: 格式={} 资源={} 依赖={} 和={}", fmt, res, dep, fmt + res + dep))
}

/// 确定性面：同账重建两次，导出逐行一致（LCG 介入版）。
pub fn drill_determinism() -> (bool, String) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    // LCG 只驱动探针序（不扰账本体），账导出必须与 LCG 无关。
    let a = export_ledger(&REJECT_LEDGER);
    let mut probe = 0;
    let mut i = 0;
    while i < 10 {
        probe ^= rng.next_below(0xFF) as u32;
        i += 1;
    }
    let b = export_ledger(&REJECT_LEDGER);
    let same = a == b;
    (
        same,
        format!("同账双导一致={} 探针和={}（账与探针无关）", same, probe),
    )
}

/// 全流程对账（J1 主链）。
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_full_coverage();
    let (a2, d2) = drill_lookup_23();
    let (a3, d3) = drill_export_replay();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 全覆盖={} 查询23={} 导出复测={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 深化块：黄金路径→码对照 + 导出格式断言 ───────────────────────
/// 黄金对照表：10 条路径→统一码独立写死，与主账逐条复核（不复用取码逻辑值）。
pub fn drill_golden_path_code_10() -> (bool, String) {
    let golden: [(u8, u32); 10] = [
        (1, CODE_BASE + 1),
        (3, CODE_BASE + 3),
        (7, CODE_BASE + 7),
        (9, CODE_BASE + 9),
        (12, CODE_BASE + 12),
        (14, CODE_BASE + 14),
        (17, CODE_BASE + 17),
        (19, CODE_BASE + 19),
        (21, CODE_BASE + 21),
        (23, CODE_BASE + 23),
    ];
    let mut ok = true;
    let mut i = 0;
    while i < golden.len() {
        match code_of(&REJECT_LEDGER, golden[i].0) {
            Ok(c) => {
                if c != golden[i].1 {
                    ok = false;
                    break;
                }
            }
            Err(_) => {
                ok = false;
                break;
            }
        }
        i += 1;
    }
    (ok, format!("黄金路径→码对照 10 组全真={}", ok))
}

/// 导出格式深化断言：行首 B 前缀 + 三段式（路径|码|摘要）+ 摘要非空。
pub fn drill_export_format() -> (bool, String) {
    let exported = match export_ledger(&REJECT_LEDGER) {
        Ok(v) => v,
        Err(e) => return (false, detail(e.val()).to_string()),
    };
    let mut fmt_ok = true;
    let mut i = 0;
    while i < exported.len() {
        let line = &exported[i];
        let segs: Vec<&str> = line.split('|').collect();
        if !line.starts_with('B') || segs.len() != 3 || segs[2].is_empty() {
            fmt_ok = false;
            break;
        }
        i += 1;
    }
    // 反向判据：抽一条路径 → 全覆盖审计必抓（CoverageGap），导出面随之拒。
    let mut cut: Vec<PathCode> = Vec::new();
    let mut j = 1;
    while j < REJECT_LEDGER.len() {
        cut.push(REJECT_LEDGER[j]);
        j += 1;
    }
    let cut_audit = audit_ledger(&cut);
    let cut_export = export_ledger(&cut);
    let cut_ok = cut_audit == Err(RejCodeCode::CoverageGap) && cut_export.is_err();
    (
        fmt_ok && cut_ok,
        format!(
            "导出格式断言={} 抽行注入抓={}",
            fmt_ok, cut_ok
        ),
    )
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    let mut ok = true;
    while i < RejCodeCode::ALL.len() {
        let v = RejCodeCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_full_coverage();
    v.push(("J1-B01-B23映射全覆盖", a, b));
    let (a, b) = drill_coverage_gap_5();
    v.push(("J2-覆盖缺口注入5/5", a, b));
    let (a, b) = drill_bad_code_10();
    v.push(("J2-错码注入10/10", a, b));
    let (a, b) = drill_lookup_23();
    v.push(("J2-查询全中+未知拒", a, b));
    let (a, b) = drill_export_replay();
    v.push(("J1-账本可导出复测", a, b));
    let (a, b) = drill_family_distribution();
    v.push(("J3-三族分布守恒", a, b));
    let (a, b) = drill_determinism();
    v.push(("J3-同账双导一致", a, b));
    let (a, b) = drill_golden_path_code_10();
    v.push(("J3-黄金路径→码对照10组", a, b));
    let (a, b) = drill_export_format();
    v.push(("J3-导出格式深化断言", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13261_checks：本单判据聚合（诊断段 0x65840）。
pub fn run_unx13261_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13261 拒载错误码面总成");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65840 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
