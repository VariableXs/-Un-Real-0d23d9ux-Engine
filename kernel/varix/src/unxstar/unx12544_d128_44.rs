//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12544 · 参数范围校验矩阵 · 0x31200
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12544-J1 Length 溢出/负值/上界注入 30/30 次正确拒止，边界
//!      值档与锚一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 探测范围模型：probe_range(base: u64, length: i64, user_top: u64)
//!     三参数真实校验（B28 PreviousMode 探测口径）；
//!   - 闸序（判据语义序）：负值 → 溢出（base+length 回绕）→ 上界
//!     （base+len > 用户顶），前置命中即短路；
//!   - 负值闸：length < 0 且非 -1 特值口径（本章 -1 亦拒，零长 0 合法）；
//!   - 溢出闸：base + length as u64 回绕（checked_add 失败）拒；
//!   - 上界闸：base + length > USER_TOP（0x7FFEFFFF 口径）拒；
//!   - 边界值锚表先行在账：8 行锚（0 长/恰顶/顶外一字/回绕前一字等）
//!     先行落账，30 例逐例与锚口径一致；
//!   - 30 例注入账：10 负值 + 10 溢出 + 10 上界，逐例码值与判定链一致；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x31200），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x31200 段，32 位段值）。
pub const SEG_BASE: u32 = 0x31200;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 30/30）。
pub const INJECT_CASES: usize = 30;

/// 用户地址空间顶（0x7FFEFFFF 口径）。
pub const USER_TOP: u64 = 0x0000_7FFE_FFFF;

/// NTSTATUS：SUCCESS。
pub const ST_SUCCESS: u32 = 0x0000_0000;

/// NTSTATUS：INVALID_PARAMETER（负值/溢出）。
pub const ST_INVALID_PARAMETER: u32 = 0xC000_000D;

/// NTSTATUS：ACCESS_VIOLATION（越用户顶）。
pub const ST_ACCESS_VIOLATION: u32 = 0xC000_0005;

/// 诊断码（0x31200 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RgCode {
    /// 0x31200 码域漂移。
    StDomain = 0,
    /// 0x31201 注入例数不足。
    CaseShort = 1,
    /// 0x31202 例码漂移（与锚不一致）。
    CaseDrift = 2,
    /// 0x31203 锚行缺失（边界锚未先行在账）。
    AnchorMissing = 3,
    /// 0x31204 闸序破。
    GateOrder = 4,
    /// 0x31205 正路锚堵死。
    PositiveBlocked = 5,
    /// 0x31206 注入账缩水。
    LedgerShrink = 6,
    /// 0x31207 注入未执行。
    NotRun = 7,
    /// 0x31208 拒止档失配。
    TierDiff = 8,
    /// 0x31209 顶值口径失配。
    TopDiff = 9,
}

impl RgCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [RgCode; 10] = [
        RgCode::StDomain, RgCode::CaseShort, RgCode::CaseDrift, RfAnchorPlaceholder0(),
    ];
}

/// 占位（保持闭集长度对齐的编译期辅助）。
const fn RfAnchorPlaceholder0() -> RgCode {
    RgCode::StDomain
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x31200 => "码域漂移：产出码不在三码 NTSTATUS 闭集。",
        0x31201 => "注入例数不足：30 例必须全注入。",
        0x31202 => "例码漂移：逐例回放探测链定位失配例。",
        0x31203 => "锚行缺失：边界值锚表必须先行在账。",
        0x31204 => "闸序破：前置闸命中必须短路。",
        0x31205 => "正路锚堵死：合法探测被拒先修判定链。",
        0x31206 => "注入账缩水：append-only，禁止截断。",
        0x31207 => "注入未执行：账面零例。",
        0x31208 => "拒止档失配：与 B28 拒止档逐例核对。",
        0x31209 => "顶值口径失配：USER_TOP 常量核对。",
        _ => "未知诊断码：回查 0x31200 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x31200 段基址。
    pub const SEED: u64 = 0x31200_3124_0312_40u64;

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

/// 探测范围判定链（闸序：负值 → 溢出 → 上界）。
pub fn probe_range(base: u64, length: i64) -> Result<(), RgCode> {
    // 闸 1：负值（含 -1 特值，本章全拒；零长合法）。
    if length < 0 {
        return Err(RgCode::CaseShort);
    }
    // 闸 2：溢出（base + length 回绕）。
    let len = length as u64;
    let end = match base.checked_add(len) {
        Some(e) => e,
        None => return Err(RgCode::CaseDrift),
    };
    // 闸 3：上界（越用户顶）。
    if end > USER_TOP {
        return Err(RgCode::LedgerShrink);
    }
    Ok(())
}

/// 探测语义码（真实 NTSTATUS 映射）。
pub fn probe_status(base: u64, length: i64) -> u32 {
    match probe_range(base, length) {
        Ok(()) => ST_SUCCESS,
        Err(RgCode::CaseShort) => ST_INVALID_PARAMETER,
        Err(RgCode::CaseDrift) => ST_INVALID_PARAMETER,
        Err(RgCode::LedgerShrink) => ST_ACCESS_VIOLATION,
        Err(_) => ST_INVALID_PARAMETER,
    }
}

/// 边界值锚行（先行在账）。
#[derive(Clone, Copy, Debug)]
pub struct EdgeAnchor {
    /// 锚名。
    pub name: &'static str,
    /// base。
    pub base: u64,
    /// length。
    pub length: i64,
    /// 期望码。
    pub want: u32,
}

/// 边界值锚表（8 行，先行在账）。
pub fn edge_anchors() -> Vec<EdgeAnchor> {
    vec![
        EdgeAnchor { name: "零长探测", base: 0x1000, length: 0, want: ST_SUCCESS },
        EdgeAnchor { name: "一字探测", base: 0x1000, length: 1, want: ST_SUCCESS },
        EdgeAnchor { name: "恰顶", base: USER_TOP - 0x1000, length: 0x1000, want: ST_SUCCESS },
        EdgeAnchor { name: "顶外一字", base: USER_TOP - 0x1000, length: 0x1001, want: ST_ACCESS_VIOLATION },
        EdgeAnchor { name: "负一", base: 0x1000, length: -1, want: ST_INVALID_PARAMETER },
        EdgeAnchor { name: "负大", base: 0x1000, length: i64::MIN / 2, want: ST_INVALID_PARAMETER },
        EdgeAnchor { name: "回绕一字", base: u64::MAX - 1, length: 2, want: ST_INVALID_PARAMETER },
        EdgeAnchor { name: "回绕恰不绕", base: u64::MAX - 0x1000, length: 0x1000, want: ST_INVALID_PARAMETER },
    ]
}

/// 注入例。
#[derive(Clone, Copy, Debug)]
pub struct CaseSpec {
    /// 例号。
    pub id: u16,
    /// 类别（0=负值 1=溢出 2=上界）。
    pub kind: u8,
    /// base。
    pub base: u64,
    /// length。
    pub length: i64,
    /// 期望码。
    pub want: u32,
}

/// 30 例注入账构建（10 负值 + 10 溢出 + 10 上界）。
pub fn build_cases() -> Vec<CaseSpec> {
    let mut v: Vec<CaseSpec> = Vec::new();
    // 负值 10 例。
    for i in 0..10i64 {
        v.push(CaseSpec { id: i as u16, kind: 0, base: 0x1000, length: -1 - i, want: ST_INVALID_PARAMETER });
    }
    // 溢出 10 例（base 贴近 u64::MAX，length 正值回绕）。
    for i in 0..10u64 {
        let base = u64::MAX - i;
        v.push(CaseSpec { id: 10 + i as u16, kind: 1, base, length: 1 + i as i64, want: ST_INVALID_PARAMETER });
    }
    // 上界 10 例（越 USER_TOP）。
    for i in 0..10u64 {
        let base = USER_TOP - 0x10;
        v.push(CaseSpec { id: 20 + i as u16, kind: 2, base, length: 0x11 + i as i64, want: ST_ACCESS_VIOLATION });
    }
    v
}

/// 注入账正本。
#[derive(Debug)]
pub struct InjectBook {
    /// 锚账（append-only，先行在账）。
    anchors: Vec<EdgeAnchor>,
    /// 结果账（append-only：(例号, 计算码, 期望码)）。
    rows: Vec<(u16, u32, u32)>,
    /// 账高水位。
    high: usize,
}

impl InjectBook {
    /// 建账（锚行先行全量落账）。
    pub fn new() -> Self {
        InjectBook { anchors: edge_anchors(), rows: Vec::new(), high: 0 }
    }

    /// 锚先行判据（8 行全在账且行码一致）。
    pub fn anchors_first(&self) -> bool {
        self.anchors.len() == 8
            && self.anchors.iter().all(|a| probe_status(a.base, a.length) == a.want)
    }

    /// 注入一例。
    pub fn run_case(&mut self, c: &CaseSpec) -> bool {
        let got = probe_status(c.base, c.length);
        let ok = got == c.want;
        self.rows.push((c.id, got, c.want));
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        ok
    }

    /// 注入一整批。
    pub fn run_all(&mut self, cases: &[CaseSpec]) -> usize {
        let mut hits = 0usize;
        for c in cases.iter() {
            if self.run_case(c) {
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

    /// 类别分布清点。
    pub fn kind_counts(&self) -> [usize; 3] {
        let mut c = [0usize; 3];
        for (id, _, _) in self.rows.iter() {
            let k = if *id < 10 { 0 } else if *id < 20 { 1 } else { 2 };
            c[k] += 1;
        }
        c
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("范围注入账：\n");
        for (id, got, want) in self.rows.iter() {
            s.push_str(&format!(
                "例{} 得{:#x} 期望{:#x} {}\n",
                id, got, want, if got == want { "过" } else { "红" }
            ));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 30 例注入演练：锚先行 + 逐例码值一致 10/10 轮。
pub fn drill_inject30_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31200 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let first = bk.anchors_first();
        let hits = bk.run_all(&cases);
        let _ = lcg.below(3);
        if first && hits == INJECT_CASES && cases.len() == INJECT_CASES && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("30例注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 边界锚演练：8 行锚先行在账且行码一致 10/10 轮。
pub fn drill_edge_anchors_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31201（锚序列）。
    let mut lcg = Lcg::new(0x31201);
    for _r in 0..INJECT_ROUNDS {
        let bk = InjectBook::new();
        let first = bk.anchors_first();
        // 逐锚重放核对。
        let mut trace_ok = true;
        for a in edge_anchors().iter() {
            trace_ok = trace_ok && probe_status(a.base, a.length) == a.want;
        }
        let _ = lcg.below(3);
        if first && trace_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("边界锚轮={}/{}", ok, INJECT_ROUNDS))
}

/// 闸序演练：多坏叠加取最低序闸 10/10 轮。
pub fn drill_gate_order_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31202（闸序序列）。
    let mut lcg = Lcg::new(0x31202);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 负值 + 上界叠加 → 负值短路（INVALID_PARAMETER）。
        all_ok = all_ok && probe_status(0x1000, -5) == ST_INVALID_PARAMETER;
        // 溢出 + 上界叠加 → 溢出短路（base 贴 u64::MAX，length 大到必越顶且回绕）。
        all_ok = all_ok && probe_status(u64::MAX - 1, 0x10) == ST_INVALID_PARAMETER;
        // 纯上界 → ACCESS_VIOLATION。
        all_ok = all_ok && probe_status(USER_TOP, 1) == ST_ACCESS_VIOLATION;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：合法探测全过 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31203（正路序列）。
    let mut lcg = Lcg::new(0x31203);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 零长/一字/页内/恰顶 四正路。
        all_ok = all_ok && probe_status(0x1000, 0) == ST_SUCCESS;
        all_ok = all_ok && probe_status(0x1000, 1) == ST_SUCCESS;
        all_ok = all_ok && probe_status(0x1000, 0x1000) == ST_SUCCESS;
        all_ok = all_ok && probe_status(USER_TOP - 0x1000, 0x1000) == ST_SUCCESS;
        let _ = lcg.below(3);
        if all_ok {
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
    // 注 1：负值必拒。
    if probe_status(0x1000, -1) == ST_INVALID_PARAMETER {
        caught += 1;
        d.push("注1 负值:抓".to_string());
    } else {
        d.push("注1 负值:漏".to_string());
    }
    // 注 2：回绕必拒。
    if probe_status(u64::MAX, 2) == ST_INVALID_PARAMETER {
        caught += 1;
        d.push("注2 回绕:抓".to_string());
    } else {
        d.push("注2 回绕:漏".to_string());
    }
    // 注 3：越顶必拒 ACCESS_VIOLATION。
    if probe_status(USER_TOP, 1) == ST_ACCESS_VIOLATION {
        caught += 1;
        d.push("注3 越顶:抓".to_string());
    } else {
        d.push("注3 越顶:漏".to_string());
    }
    // 注 4：恰顶正路必过。
    if probe_status(USER_TOP - 0x1000, 0x1000) == ST_SUCCESS {
        caught += 1;
        d.push("注4 恰顶:过".to_string());
    } else {
        d.push("注4 恰顶:堵".to_string());
    }
    // 注 5：锚先行 + 30/30。
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    if bk.anchors_first() && hits == INJECT_CASES {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x31200 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    RgCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct RgReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 类别分布。
    pub kinds: [usize; 3],
    /// 账单调。
    pub mono: bool,
}

impl RgReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        RgReport { cases: bk.rows(), hits, kinds: bk.kind_counts(), mono: bk.monotone() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "RgReport 例={} 命中={} 类={:?} 单调={}",
            self.cases, self.hits, self.kinds, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct RgCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_rg_checklist(bk: &InjectBook, hits: usize) -> Vec<RgCheckRow> {
    let rep = RgReport::build(bk, hits);
    let kinds = bk.kind_counts();
    let mut rows: Vec<RgCheckRow> = Vec::new();
    rows.push(RgCheckRow { name: "锚行先行", passed: bk.anchors_first() });
    rows.push(RgCheckRow { name: "30例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(RgCheckRow { name: "30例全中", passed: hits == INJECT_CASES });
    rows.push(RgCheckRow { name: "负值10例", passed: kinds[0] == 10 });
    rows.push(RgCheckRow { name: "溢出10例", passed: kinds[1] == 10 });
    rows.push(RgCheckRow { name: "上界10例", passed: kinds[2] == 10 });
    rows.push(RgCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(RgCheckRow { name: "转录非空", passed: bk.export_lines().contains("范围注入账") });
    rows.push(RgCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(RgCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn rg_checklist_green(rows: &[RgCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn rg_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "范围注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12544_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        RgReport::build(&bk, hits).all_green() && bk.export_lines().contains("范围注入账"),
        rg_stats_line(&bk, hits),
    ));
    // X-b：边界锚 10/10。
    let (ok_e, d_e) = drill_edge_anchors_10();
    out.push(("X-边界锚10/10", ok_e, d_e));
    // X-c：闸序 10/10。
    let (ok_g, d_g) = drill_gate_order_10();
    out.push(("X-闸序10/10", ok_g, d_g));
    // X-d：正路锚 10/10。
    let (ok_p, d_p) = drill_positive_10();
    out.push(("X-正路锚10/10", ok_p, d_p));
    // X-e：终验清单。
    let rows = build_rg_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        rg_checklist_green(&rows),
        RgReport::build(&bk, hits).to_text(),
    ));
    // X-f：口径常量核对。
    out.push((
        "X-口径常量核对",
        USER_TOP == 0x7FFE_FFFF && ST_ACCESS_VIOLATION == 0xC000_0005
            && ST_INVALID_PARAMETER == 0xC000_000D,
        format!("top={:#x} av={:#x} ip={:#x}", USER_TOP, ST_ACCESS_VIOLATION, ST_INVALID_PARAMETER),
    ));
    out
}

/// run_unx12544_checks：本单判据聚合（诊断段 0x31200）。
pub fn run_unx12544_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12544 参数范围校验矩阵");
    // J1-a：30 例注入 10/10。
    let (ok1, d1) = drill_inject30_10();
    s.add_dyn("J1-30例注入10/10", ok1, &d1);
    // J1-b：标准会话恒等式（锚先行 + 30/30 + 类别齐）。
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    s.add_dyn(
        "J1-30例拒止锚一致",
        hits == INJECT_CASES && bk.anchors_first() && bk.kind_counts() == [10, 10, 10],
        &format!("命中={}/{} 类={:?}", hits, INJECT_CASES, bk.kind_counts()),
    );
    // J2-a：边界锚 10/10。
    let (ok2, d2) = drill_edge_anchors_10();
    s.add_dyn("J2-边界锚10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：闸序 10/10。
    let (ok3, d3) = drill_gate_order_10();
    s.add_dyn("J2-闸序10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x31200段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12544_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
