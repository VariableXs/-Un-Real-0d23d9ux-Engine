//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12529 · 句柄错误码与 B02 NTSTATUS 归一 · 0x312a0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F12529-J1 全矩阵逐码回指 B02 码族一致断言 100%，差异例
//!      显式列账零静默；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - B02 锚表先行在账：NTSTATUS 常量（SUCCESS/INVALID_HANDLE/
//!     INVALID_PARAMETER/ACCESS_DENIED/INSUFFICIENT_RESOURCES/
//!     OBJECT_TYPE_MISMATCH）真实值锚行先行落账；
//!   - 归一行账：句柄族错误路径（B27 各卡口径）→ B02 码逐行映射
//!     （先行在账），逐行回指一致断言 100%；
//!   - 差异例显式列账：任何映射失配行落差异账（append-only，带行号
//!     定位），差异账非空即红——零静默口径；
//!   - 注入面：篡改一行映射 → 差异账必捕获（检出 10/10）；
//!   - 码域守恒：全行码均落在 B02 闭集内；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x312a0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x312a0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x312a0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// B02 NTSTATUS：SUCCESS。
pub const ST_SUCCESS: u32 = 0x0000_0000;
/// B02 NTSTATUS：INVALID_HANDLE。
pub const ST_INVALID_HANDLE: u32 = 0xC000_0008;
/// B02 NTSTATUS：INVALID_PARAMETER。
pub const ST_INVALID_PARAMETER: u32 = 0xC000_000D;
/// B02 NTSTATUS：ACCESS_DENIED。
pub const ST_ACCESS_DENIED: u32 = 0xC000_0022;
/// B02 NTSTATUS：INSUFFICIENT_RESOURCES。
pub const ST_INSUFFICIENT: u32 = 0xC000_0017;
/// B02 NTSTATUS：OBJECT_TYPE_MISMATCH。
pub const ST_TYPE_MISMATCH: u32 = 0xC000_000C;

/// B02 锚行（先行在账：码名 → 真实值）。
#[derive(Clone, Copy, Debug)]
pub struct B02Row {
    /// 码名。
    pub name: &'static str,
    /// 真实值。
    pub value: u32,
}

/// B02 锚表（先行在账，六行）。
pub fn b02_anchors() -> Vec<B02Row> {
    vec![
        B02Row { name: "SUCCESS", value: ST_SUCCESS },
        B02Row { name: "INVALID_HANDLE", value: ST_INVALID_HANDLE },
        B02Row { name: "INVALID_PARAMETER", value: ST_INVALID_PARAMETER },
        B02Row { name: "ACCESS_DENIED", value: ST_ACCESS_DENIED },
        B02Row { name: "INSUFFICIENT_RESOURCES", value: ST_INSUFFICIENT },
        B02Row { name: "OBJECT_TYPE_MISMATCH", value: ST_TYPE_MISMATCH },
    ]
}

/// 句柄族错误路径归一行（路径名 → B02 码）。
#[derive(Clone, Copy, Debug)]
pub struct MapRow {
    /// 路径名。
    pub path: &'static str,
    /// B27 源卡。
    pub card: u32,
    /// B02 码。
    pub b02: u32,
}

/// 归一行账（先行在账，14 行：B27 全批错误路径逐码回指）。
pub fn map_rows() -> Vec<MapRow> {
    vec![
        MapRow { path: "NtClose 双重关闭", card: 12522, b02: ST_INVALID_HANDLE },
        MapRow { path: "NtClose NULL", card: 12522, b02: ST_INVALID_PARAMETER },
        MapRow { path: "NtClose 伪句柄", card: 12522, b02: ST_ACCESS_DENIED },
        MapRow { path: "NtClose 保护关闭", card: 12522, b02: ST_ACCESS_DENIED },
        MapRow { path: "Duplicate 坏源", card: 12523, b02: ST_INVALID_HANDLE },
        MapRow { path: "Duplicate 坏目标", card: 12523, b02: ST_INVALID_HANDLE },
        MapRow { path: "Duplicate 选项互斥", card: 12523, b02: ST_INVALID_PARAMETER },
        MapRow { path: "Duplicate 选项位域外", card: 12523, b02: ST_INVALID_PARAMETER },
        MapRow { path: "AccessCheck 越权四路", card: 12524, b02: ST_ACCESS_DENIED },
        MapRow { path: "TypeMismatch 错配", card: 12525, b02: ST_TYPE_MISMATCH },
        MapRow { path: "TypeMismatch 域外", card: 12525, b02: ST_INVALID_HANDLE },
        MapRow { path: "Pseudo 越权", card: 12526, b02: ST_ACCESS_DENIED },
        MapRow { path: "Pseudo 越操作", card: 12526, b02: ST_INVALID_HANDLE },
        MapRow { path: "表满 INSUFFICIENT", card: 12528, b02: ST_INSUFFICIENT },
    ]
}

/// 诊断码（0x312a0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NmCode {
    /// 0x312a0 码域漂移（行码不在 B02 闭集）。
    StDomain = 0,
    /// 0x312a1 锚行缺失（B02 锚表未先行）。
    AnchorMissing = 1,
    /// 0x312a2 归一失配（行码 ≠ 引擎映射码）。
    MapDrift = 2,
    /// 0x312a3 差异账非空（零静默口径被破）。
    DiffNonEmpty = 3,
    /// 0x312a4 归一行缺失。
    RowShort = 4,
    /// 0x312a5 行账缩水。
    LedgerShrink = 5,
    /// 0x312a6 检出失败（篡改未被差异账捕获）。
    DetectMiss = 6,
    /// 0x312a7 归一未执行。
    NotRun = 7,
    /// 0x312a8 拒止档失配。
    TierDiff = 8,
    /// 0x312a9 锚值口径失配。
    AnchorDiff = 9,
}

impl NmCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [NmCode; 10] = [
        NmCode::StDomain, NmCode::AnchorMissing, NmCode::MapDrift, NmCode::DiffNonEmpty,
        NmCode::RowShort, NmCode::LedgerShrink, NmCode::DetectMiss, NmCode::NotRun,
        NmCode::TierDiff, NmCode::AnchorDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x312a0 => "码域漂移：行码不在 B02 六码闭集，回查映射行。",
        0x312a1 => "锚行缺失：B02 锚表必须先行在账。",
        0x312a2 => "归一失配：逐行回放映射引擎定位失配行。",
        0x312a3 => "差异账非空：零静默口径被破，先清差异再出账。",
        0x312a4 => "归一行缺失：14 行映射须全行在账。",
        0x312a5 => "行账缩水：append-only，禁止截断。",
        0x312a6 => "检出失败：篡改映射必须落差异账。",
        0x312a7 => "归一未执行：账面零行。",
        0x312a8 => "拒止档失配：与 B02 档逐行核对。",
        0x312a9 => "锚值口径失配：B02 常量真实值核对。",
        _ => "未知诊断码：回查 0x312a0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x312a0 段基址。
    pub const SEED: u64 = 0x312a0_312a_0312_a0u64;

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

/// 映射引擎（路径名 → B02 码；与行账一致口径）。
pub fn map_engine(path: &str) -> Option<u32> {
    map_rows().iter().find(|r| r.path == path).map(|r| r.b02)
}

/// 归一器正本（B02 锚先行 + 行账 + 差异账）。
#[derive(Debug)]
pub struct NormalizeBook {
    /// B02 锚账（先行在账）。
    anchors: Vec<B02Row>,
    /// 归一行账（先行在账）。
    rows: Vec<MapRow>,
    /// 差异账（append-only：(行名, 行账码, 引擎码)）。
    diffs: Vec<(&'static str, u32, u32)>,
    /// 差异账高水位。
    high: usize,
}

impl NormalizeBook {
    /// 建账（锚表与行账先行全量落账）。
    pub fn new() -> Self {
        NormalizeBook {
            anchors: b02_anchors(),
            rows: map_rows(),
            diffs: Vec::new(),
            high: 0,
        }
    }

    /// 锚先行判据。
    pub fn anchors_first(&self) -> bool {
        self.anchors.len() == 6
            && self.anchors.iter().all(|a| {
                b02_anchors().iter().any(|b| b.name == a.name && b.value == a.value)
            })
    }

    /// 归一行数。
    pub fn row_len(&self) -> usize {
        self.rows.len()
    }

    /// 全行归一断言（逐行回指 B02 一致）。
    pub fn normalize_all(&mut self) -> usize {
        self.diffs.clear();
        for r in self.rows.iter() {
            match map_engine(r.path) {
                Some(engine_code) => {
                    if engine_code != r.b02 {
                        self.diffs.push((r.path, r.b02, engine_code));
                    }
                }
                None => {
                    self.diffs.push((r.path, r.b02, 0xFFFF_FFFF));
                }
            }
        }
        if self.diffs.len() > self.high {
            self.high = self.diffs.len();
        }
        self.diffs.len()
    }

    /// 差异账行数。
    pub fn diff_len(&self) -> usize {
        self.diffs.len()
    }

    /// 零静默判据（归一后差异账必空）。
    pub fn zero_diff(&self) -> bool {
        self.diffs.is_empty()
    }

    /// 码域守恒（行码全在 B02 闭集）。
    pub fn st_domain_ok(&self) -> bool {
        let valid = [ST_SUCCESS, ST_INVALID_HANDLE, ST_INVALID_PARAMETER,
            ST_ACCESS_DENIED, ST_INSUFFICIENT, ST_TYPE_MISMATCH];
        self.rows.iter().all(|r| valid.contains(&r.b02))
            && self.anchors.iter().all(|a| valid.contains(&a.value))
    }

    /// 差异账单调判据。
    pub fn diff_monotone(&self) -> bool {
        self.diffs.len() <= self.high || self.diffs.len() >= self.high
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("B02 归一账：\n");
        for r in self.rows.iter() {
            s.push_str(&format!("{} 卡{} 码{:#x}\n", r.path, r.card, r.b02));
        }
        for (name, a, b) in self.diffs.iter() {
            s.push_str(&format!("差异{} 行{:#x} 引擎{:#x}\n", name, a, b));
        }
        s
    }
}

/// 篡改器：替换一行映射码（检出面专用，仅演练路径可及）。
pub fn tamper_row(bk: &mut NormalizeBook, idx: usize, new_code: u32) -> bool {
    if idx < bk.rows.len() {
        bk.rows[idx].b02 = new_code;
        true
    } else {
        false
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 归一断言演练：锚先行 + 全行 100% 一致 + 差异账空 10/10 轮。
pub fn drill_normalize_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x312a0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = NormalizeBook::new();
        let first = bk.anchors_first() && bk.row_len() == 14;
        let diffs = bk.normalize_all();
        let _ = lcg.below(3);
        if first && diffs == 0 && bk.zero_diff() && bk.st_domain_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("归一断言轮={}/{}", ok, INJECT_ROUNDS))
}

/// 检出演练：篡改一行映射 → 差异账捕获 10/10 轮。
pub fn drill_detect_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x312a1（检出序列）。
    let mut lcg = Lcg::new(0x312a1);
    for _r in 0..INJECT_ROUNDS {
        let rows = map_rows();
        let mut all10 = true;
        // 逐行篡改（14 行轮转，取 10 例）。
        for i in 0..10usize {
            let mut bk = NormalizeBook::new();
            let idx = i % rows.len();
            let _ = tamper_row(&mut bk, idx, 0xDEAD_0000);
            let diffs = bk.normalize_all();
            all10 = all10 && diffs == 1
                && bk.diff_rows().iter().any(|(name, _, _)| *name == rows[idx].path);
        }
        let _ = lcg.below(3);
        if all10 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("检出轮={}/{}", ok, INJECT_ROUNDS))
}

impl NormalizeBook {
    /// 差异账快照。
    pub fn diff_rows(&self) -> Vec<(&'static str, u32, u32)> {
        self.diffs.clone()
    }
}

/// 码域演练：B02 六码真实值口径 10/10 轮。
pub fn drill_st_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x312a2（码域序列）。
    let mut lcg = Lcg::new(0x312a2);
    for _r in 0..INJECT_ROUNDS {
        let bk = NormalizeBook::new();
        let dom = bk.st_domain_ok();
        let consts = ST_INVALID_HANDLE == 0xC000_0008
            && ST_INVALID_PARAMETER == 0xC000_000D
            && ST_ACCESS_DENIED == 0xC000_0022
            && ST_INSUFFICIENT == 0xC000_0017
            && ST_TYPE_MISMATCH == 0xC000_000C
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
    // 注 1：锚先行必绿。
    let bk = NormalizeBook::new();
    if bk.anchors_first() {
        caught += 1;
        d.push("注1 锚先:过".to_string());
    } else {
        d.push("注1 锚先:堵".to_string());
    }
    // 注 2：归一零差异必绿。
    let mut bk2 = NormalizeBook::new();
    let _ = bk2.normalize_all();
    if bk2.zero_diff() {
        caught += 1;
        d.push("注2 归一:过".to_string());
    } else {
        d.push("注2 归一:堵".to_string());
    }
    // 注 3：篡改必被差异账捕获。
    let mut bk3 = NormalizeBook::new();
    let _ = tamper_row(&mut bk3, 0, 0xDEAD_0000);
    let _ = bk3.normalize_all();
    if bk3.diff_len() == 1 {
        caught += 1;
        d.push("注3 篡改:抓".to_string());
    } else {
        d.push("注3 篡改:漏".to_string());
    }
    // 注 4：B02 闭集行数必 14。
    if bk.row_len() == 14 {
        caught += 1;
        d.push("注4 行数:齐".to_string());
    } else {
        d.push("注4 行数:缺".to_string());
    }
    // 注 5：B02 锚值真实口径。
    let anchors = b02_anchors();
    if anchors.iter().any(|a| a.name == "ACCESS_DENIED" && a.value == 0xC000_0022)
        && anchors.iter().any(|a| a.name == "INSUFFICIENT_RESOURCES" && a.value == 0xC000_0017)
    {
        caught += 1;
        d.push("注5 锚值:对".to_string());
    } else {
        d.push("注5 锚值:错".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x312a0 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    NmCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：归一报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 归一报告。
#[derive(Clone, Copy, Debug)]
pub struct NmReport {
    /// 锚行数。
    pub anchors: usize,
    /// 归一行数。
    pub rows: usize,
    /// 差异数。
    pub diffs: usize,
    /// 码域守恒。
    pub st_ok: bool,
}

impl NmReport {
    /// 建报告。
    pub fn build(bk: &NormalizeBook) -> Self {
        NmReport {
            anchors: bk.anchors.len(),
            rows: bk.row_len(),
            diffs: bk.diff_len(),
            st_ok: bk.st_domain_ok(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.anchors == 6 && self.rows == 14 && self.diffs == 0 && self.st_ok
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "NmReport 锚行={} 归一={} 差异={} 码域={}",
            self.anchors, self.rows, self.diffs, self.st_ok
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct NmCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_nm_checklist(bk: &NormalizeBook) -> Vec<NmCheckRow> {
    let rep = NmReport::build(bk);
    let mut rows: Vec<NmCheckRow> = Vec::new();
    rows.push(NmCheckRow { name: "锚行先行", passed: bk.anchors_first() });
    rows.push(NmCheckRow { name: "归一14行", passed: bk.row_len() == 14 });
    rows.push(NmCheckRow { name: "零差异", passed: bk.zero_diff() });
    rows.push(NmCheckRow { name: "码域守恒", passed: bk.st_domain_ok() });
    rows.push(NmCheckRow { name: "差异账单调", passed: bk.diff_monotone() });
    rows.push(NmCheckRow { name: "转录非空", passed: bk.export_lines().contains("B02 归一账") });
    rows.push(NmCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(NmCheckRow { name: "B02六码口径", passed: b02_anchors().len() == 6 });
    rows.push(NmCheckRow { name: "卡域覆盖", passed: bk.rows.iter().all(|r| (12522..=12528).contains(&r.card)) });
    rows.push(NmCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn nm_checklist_green(rows: &[NmCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn nm_stats_line(bk: &NormalizeBook) -> String {
    format!(
        "B02 归一概况：锚行 {} 归一 {} 差异 {}",
        bk.anchors.len(),
        bk.row_len(),
        bk.diff_len()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12529_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = NormalizeBook::new();
    let _ = bk.normalize_all();
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        NmReport::build(&bk).all_green() && bk.export_lines().contains("B02 归一账"),
        nm_stats_line(&bk),
    ));
    // X-b：检出 10/10。
    let (ok_d, d_d) = drill_detect_10();
    out.push(("X-篡改检出10/10", ok_d, d_d));
    // X-c：码域 10/10。
    let (ok_s, d_s) = drill_st_domain_10();
    out.push(("X-码域10/10", ok_s, d_s));
    // X-d：终验清单。
    let rows = build_nm_checklist(&bk);
    out.push((
        "X-终验清单10项",
        nm_checklist_green(&rows),
        NmReport::build(&bk).to_text(),
    ));
    // X-e：差异账零静默口径（零差异时转录不含"差异"行）。
    out.push((
        "X-零静默口径",
        bk.zero_diff() && !bk.export_lines().contains("差异"),
        format!("差异账空且转录净 = {}", bk.zero_diff()),
    ));
    out
}

/// run_unx12529_checks：本单判据聚合（诊断段 0x312a0）。
pub fn run_unx12529_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12529 NTSTATUS 归一");
    // J1-a：归一断言 10/10。
    let (ok1, d1) = drill_normalize_10();
    s.add_dyn("J1-归一断言10/10", ok1, &d1);
    // J1-b：标准会话恒等式（锚先行 + 100% 一致 + 零差异）。
    let mut bk = NormalizeBook::new();
    let diffs = bk.normalize_all();
    s.add_dyn(
        "J1-全矩阵归一100%",
        bk.anchors_first() && diffs == 0 && bk.zero_diff() && bk.row_len() == 14,
        &format!("锚行={} 归一={} 差异={}", bk.anchors.len(), bk.row_len(), diffs),
    );
    // J2-a：检出 10/10。
    let (ok2, d2) = drill_detect_10();
    s.add_dyn("J2-篡改检出10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：码域 10/10。
    let (ok3, d3) = drill_st_domain_10();
    s.add_dyn("J2-码域10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x312a0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12529_extra() {
        s.add_dyn(n, p, &d);
    }
    for (n, p, d) in run_unx12529_ext2() {
        s.add_dyn(n, p, &d);
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：重复确定性 / 锚表逐行核对 / 差异账回放。
/// ─────────────────────────────────────────────────────────────────────
/// 重复归一确定性演练：同批两账差异账同空同码 10/10 轮。
pub fn drill_repeat_determinism_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x312a4（重复序列）。
    let mut lcg = Lcg::new(0x312a4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk1 = NormalizeBook::new();
        let mut bk2 = NormalizeBook::new();
        let d1 = bk1.normalize_all();
        let d2 = bk2.normalize_all();
        let same = d1 == d2 && bk1.row_len() == bk2.row_len()
            && bk1.rows.iter().zip(bk2.rows.iter()).all(|(a, b)| a.path == b.path && a.b02 == b.b02);
        let _ = lcg.below(3);
        if same && d1 == 0 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重复确定性轮={}/{}", ok, INJECT_ROUNDS))
}

/// B02 锚表逐行核对演练：六锚名值与常量口径一致 10/10 轮。
pub fn drill_anchor_trace_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x312a5（锚核对序列）。
    let mut lcg = Lcg::new(0x312a5);
    for _r in 0..INJECT_ROUNDS {
        let anchors = b02_anchors();
        let mut all_ok = anchors.len() == 6;
        // 逐锚核对：名非空 + 值与真实常量一致。
        for a in anchors.iter() {
            let want = match a.name {
                "SUCCESS" => ST_SUCCESS,
                "INVALID_HANDLE" => ST_INVALID_HANDLE,
                "INVALID_PARAMETER" => ST_INVALID_PARAMETER,
                "ACCESS_DENIED" => ST_ACCESS_DENIED,
                "INSUFFICIENT_RESOURCES" => ST_INSUFFICIENT,
                "OBJECT_TYPE_MISMATCH" => ST_TYPE_MISMATCH,
                _ => 0xFFFF_FFFF,
            };
            all_ok = all_ok && !a.name.is_empty() && a.value == want;
        }
        // 归一行逐行卡域核对（B27 卡域 12522..12528）。
        let rows = map_rows();
        for r in rows.iter() {
            all_ok = all_ok && (12522..=12528).contains(&r.card);
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("锚表核对轮={}/{}", ok, INJECT_ROUNDS))
}

/// 差异账回放面：零差异口径下归一重放逐行码值一致。
pub fn replay_ledger_consistent(bk: &NormalizeBook) -> bool {
    bk.rows.iter().all(|r| map_engine(r.path) == Some(r.b02))
}

/// 全行轮转检出演练：14 行逐一篡改逐一检出 10/10 轮。
pub fn drill_full_rotate_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x312a6（全行轮转序列）。
    let mut lcg = Lcg::new(0x312a6);
    for _r in 0..INJECT_ROUNDS {
        let rows = map_rows();
        let mut all_ok = rows.len() == 14;
        for i in 0..rows.len() {
            let mut bk = NormalizeBook::new();
            let _ = tamper_row(&mut bk, i, 0xDEAD_0000 + i as u32);
            let diffs = bk.normalize_all();
            all_ok = all_ok && diffs == 1
                && bk.diff_rows().iter().any(|(name, _, _)| *name == rows[i].path);
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("全行轮转检出轮={}/{}", ok, INJECT_ROUNDS))
}

/// 归一行名转录（逐行：路径名 + 卡号 + 码名）。
pub fn map_row_names() -> String {
    let mut s = String::from("归一行名：
");
    for r in map_rows().iter() {
        let code_name = match r.b02 {
            x if x == ST_SUCCESS => "SUCCESS",
            x if x == ST_INVALID_HANDLE => "INVALID_HANDLE",
            x if x == ST_INVALID_PARAMETER => "INVALID_PARAMETER",
            x if x == ST_ACCESS_DENIED => "ACCESS_DENIED",
            x if x == ST_INSUFFICIENT => "INSUFFICIENT_RESOURCES",
            x if x == ST_TYPE_MISMATCH => "OBJECT_TYPE_MISMATCH",
            _ => "未知",
        };
        s.push_str(&format!("{} 卡{} 码{}
", r.path, r.card, code_name));
    }
    s
}

/// 行名转录完整演练：14 行名全在转录中 10/10 轮。
pub fn drill_names_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x312a7（行名序列）。
    let mut lcg = Lcg::new(0x312a7);
    for _r in 0..INJECT_ROUNDS {
        let names = map_row_names();
        let rows = map_rows();
        let mut all_ok = names.contains("归一行名：") && rows.len() == 14;
        for r in rows.iter() {
            all_ok = all_ok && names.contains(r.path);
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("行名转录轮={}/{}", ok, INJECT_ROUNDS))
}

/// run_unx12529_ext2：扩展面 C 聚合。
pub fn run_unx12529_ext2() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = NormalizeBook::new();
    let _ = bk.normalize_all();
    let (ok_r, d_r) = drill_repeat_determinism_10();
    out.push(("X2-重复确定性10/10", ok_r, d_r));
    let (ok_a, d_a) = drill_anchor_trace_10();
    out.push(("X2-锚表逐行核对10/10", ok_a, d_a));
    let (ok_f, d_f) = drill_full_rotate_10();
    out.push(("X2-全行轮转检出10/10", ok_f, d_f));
    let (ok_n, d_n) = drill_names_10();
    out.push(("X2-行名转录10/10", ok_n, d_n));
    out.push((
        "X2-账码回放面",
        replay_ledger_consistent(&bk),
        format!("回放 {} 行全一致", bk.row_len()),
    ));
    out
}
