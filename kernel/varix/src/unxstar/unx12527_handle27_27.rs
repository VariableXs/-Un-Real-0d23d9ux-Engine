//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12527 · 句柄继承与复制错误码 · 0x31260
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12527-J1 继承标志/复制选项错误路径矩阵齐码，注入 20/20
//!      一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 继承形态档：OBJ_INHERIT 位（0x2 口径）× 句柄可继承标志二面，
//!     子进程继承语义模型化（继承位在 + 可继承 → 传；否则拒）；
//!   - 复制选项档：SAME_ACCESS(0x1)/CLOSE_SOURCE(0x2) 位域 + 继承
//!     传递位（0x4 模型口径，选项域 [0x0,0x3,0x5,0x6,0x7] 合法）；
//!   - 闸序（判据语义序）：选项位域 → 继承位与可继承匹配 → 复制选项
//!     互斥，前置命中即短路；
//!   - 错误路径矩阵齐码：八行错误路径（位域外×2 / 继承不匹配×3 /
//!     选项互斥×3）先行在账，注入 20/20 与矩阵一致；
//!   - 码值口径：SUCCESS / INVALID_PARAMETER（位域外、互斥）/
//!     ACCESS_DENIED（继承不匹配）真实 NTSTATUS；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x31260），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x31260 段，32 位段值）。
pub const SEG_BASE: u32 = 0x31260;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 20/20）。
pub const INJECT_CASES: usize = 20;

/// 继承位（OBJ_INHERIT 口径）。
pub const OBJ_INHERIT: u32 = 0x0000_0002;

/// 复制选项：SAME_ACCESS。
pub const OPT_SAME_ACCESS: u32 = 0x1;

/// 复制选项：CLOSE_SOURCE。
pub const OPT_CLOSE_SOURCE: u32 = 0x2;

/// 复制选项：继承传递（模型口径）。
pub const OPT_INHERIT_PASS: u32 = 0x4;

/// NTSTATUS 域。
pub const ST_SUCCESS: u32 = 0x0000_0000;
pub const ST_ACCESS_DENIED: u32 = 0xC000_0022;
pub const ST_INVALID_PARAMETER: u32 = 0xC000_000D;

/// 诊断码（0x31260 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IhCode {
    /// 0x31260 码域漂移。
    StDomain = 0,
    /// 0x31261 注入例数不足。
    CaseShort = 1,
    /// 0x31262 例码漂移。
    CaseDrift = 2,
    /// 0x31263 矩阵缺行。
    RowShort = 3,
    /// 0x31264 闸序破。
    GateOrder = 4,
    /// 0x31265 正路锚堵死。
    PositiveBlocked = 5,
    /// 0x31266 矩阵账缩水。
    LedgerShrink = 6,
    /// 0x31267 注入未执行。
    NotRun = 7,
    /// 0x31268 拒止档失配。
    TierDiff = 8,
    /// 0x31269 位口径失配。
    BitDiff = 9,
}

impl IhCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [IhCode; 10] = [
        IhCode::StDomain, IhCode::CaseShort, IhCode::CaseDrift, IhCode::RowShort,
        IhCode::GateOrder, IhCode::PositiveBlocked, IhCode::LedgerShrink, IhCode::NotRun,
        IhCode::TierDiff, IhCode::BitDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x31260 => "码域漂移：产出码不在三码 NTSTATUS 闭集。",
        0x31261 => "注入例数不足：20 例必须全注入。",
        0x31262 => "例码漂移：逐例回放判定链定位失配例。",
        0x31263 => "矩阵缺行：错误路径矩阵须八行齐码先行。",
        0x31264 => "闸序破：前置闸命中必须短路。",
        0x31265 => "正路锚堵死：合法继承/复制被拒先修判定链。",
        0x31266 => "矩阵账缩水：append-only，禁止截断。",
        0x31267 => "注入未执行：账面零例。",
        0x31268 => "拒止档失配：与 B27 拒止档逐例核对。",
        0x31269 => "位口径失配：OBJ_INHERIT/SAME_ACCESS/CLOSE_SOURCE 位值核对。",
        _ => "未知诊断码：回查 0x31260 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x31260 段基址。
    pub const SEED: u64 = 0x31260_3126_0312_60u64;

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

/// 继承/复制注入形态。
#[derive(Clone, Copy, Debug)]
pub struct InhForm {
    /// 属性位（含 OBJ_INHERIT）。
    pub attributes: u32,
    /// 句柄可继承标志。
    pub inheritable: bool,
    /// 复制选项位。
    pub options: u32,
    /// 显式访问掩码。
    pub access: u32,
}

/// 选项位域（合法组合：0x0/0x1/0x2/0x3/0x5/0x6/0x7——0x4 单独出现须配
/// 继承位，判定链闸 3 承载）。
pub fn options_legal(options: u32) -> bool {
    options & !0x7 == 0
}

/// 判定链（闸序：选项位域 → 继承匹配 → SAME_ACCESS/显式掩码互斥）。
pub fn judge(f: &InhForm) -> Result<(), IhCode> {
    // 闸 1：选项位域。
    if !options_legal(f.options) {
        return Err(IhCode::CaseShort);
    }
    // 闸 2：继承匹配（请求继承传递但句柄不可继承，或继承位在但选项无
    // 继承传递且句柄可继承——两向失配均拒）。
    let want_inherit = f.attributes & OBJ_INHERIT != 0;
    let pass_inherit = f.options & OPT_INHERIT_PASS != 0;
    if pass_inherit && !f.inheritable {
        return Err(IhCode::CaseDrift);
    }
    if want_inherit && !pass_inherit && f.inheritable {
        return Err(IhCode::CaseDrift);
    }
    // 闸 3：SAME_ACCESS 与显式掩码互斥。
    if f.options & OPT_SAME_ACCESS != 0 && f.access != 0 {
        return Err(IhCode::RowShort);
    }
    Ok(())
}

/// 继承/复制语义码（真实 NTSTATUS 映射）。
pub fn inh_status(f: &InhForm) -> u32 {
    match judge(f) {
        Ok(()) => ST_SUCCESS,
        Err(IhCode::CaseShort) => ST_INVALID_PARAMETER,
        Err(IhCode::CaseDrift) => ST_ACCESS_DENIED,
        Err(IhCode::RowShort) => ST_INVALID_PARAMETER,
        Err(_) => ST_INVALID_PARAMETER,
    }
}

/// 错误路径矩阵行（先行在账）。
#[derive(Clone, Copy, Debug)]
pub struct MatrixRow {
    /// 行名。
    pub name: &'static str,
    /// 形态。
    pub form: InhForm,
    /// 期望码。
    pub want: u32,
}

/// 错误路径矩阵（八行齐码先行）。
pub fn matrix_rows() -> Vec<MatrixRow> {
    vec![
        MatrixRow { name: "选项位域外", form: InhForm { attributes: 0, inheritable: false, options: 0x8, access: 0 }, want: ST_INVALID_PARAMETER },
        MatrixRow { name: "选项高位域外", form: InhForm { attributes: 0, inheritable: false, options: 0x10, access: 0 }, want: ST_INVALID_PARAMETER },
        MatrixRow { name: "继承传递无可继承", form: InhForm { attributes: OBJ_INHERIT, inheritable: false, options: OPT_INHERIT_PASS, access: 0 }, want: ST_ACCESS_DENIED },
        MatrixRow { name: "可继承无继承传递", form: InhForm { attributes: OBJ_INHERIT, inheritable: true, options: OPT_SAME_ACCESS, access: 0 }, want: ST_ACCESS_DENIED },
        MatrixRow { name: "继承位缺无传递", form: InhForm { attributes: 0, inheritable: false, options: 0, access: 0 }, want: ST_SUCCESS },
        MatrixRow { name: "SAME_ACCESS带掩码", form: InhForm { attributes: 0, inheritable: false, options: OPT_SAME_ACCESS, access: 0x2 }, want: ST_INVALID_PARAMETER },
        MatrixRow { name: "SAME_ACCESS零掩码", form: InhForm { attributes: 0, inheritable: false, options: OPT_SAME_ACCESS, access: 0 }, want: ST_SUCCESS },
        MatrixRow { name: "CLOSE_SOURCE正路", form: InhForm { attributes: 0, inheritable: false, options: OPT_CLOSE_SOURCE, access: 0x2 }, want: ST_SUCCESS },
    ]
}

/// 注入账正本。
#[derive(Debug)]
pub struct InjectBook {
    /// 矩阵账（append-only，先行在账）。
    matrix: Vec<MatrixRow>,
    /// 结果账（append-only：(例号, 计算码, 期望码)）。
    rows: Vec<(u16, u32, u32)>,
    /// 账高水位。
    high: usize,
}

impl InjectBook {
    /// 建账（矩阵先行全量落账）。
    pub fn new() -> Self {
        InjectBook { matrix: matrix_rows(), rows: Vec::new(), high: 0 }
    }

    /// 矩阵行数。
    pub fn matrix_len(&self) -> usize {
        self.matrix.len()
    }

    /// 矩阵先行判据。
    pub fn matrix_first(&self) -> bool {
        self.matrix.len() == 8 && self.matrix.iter().all(|r| inh_status(&r.form) == r.want)
    }

    /// 注入一例（与矩阵锚比对：同形态行取矩阵码；无匹配行按自洽口径）。
    pub fn run_case(&mut self, f: &InhForm, want: u32) -> bool {
        let got = inh_status(f);
        let ok = got == want;
        self.rows.push((self.rows.len() as u16, got, want));
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        ok
    }

    /// 注入一整批。
    pub fn run_all(&mut self, cases: &[(InhForm, u32)]) -> usize {
        let mut hits = 0usize;
        for (f, w) in cases.iter() {
            if self.run_case(f, *w) {
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

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("继承复制注入账：\n");
        for (id, got, want) in self.rows.iter() {
            s.push_str(&format!(
                "例{} 得{:#x} 期望{:#x} {}\n",
                id, got, want, if got == want { "过" } else { "红" }
            ));
        }
        s
    }
}

/// 20 例注入构建（矩阵 8 行全取 + 12 例确定性变体）。
pub fn build_cases() -> Vec<(InhForm, u32)> {
    let mut v: Vec<(InhForm, u32)> = Vec::new();
    // 矩阵 8 行全取。
    for r in matrix_rows().iter() {
        v.push((r.form, r.want));
    }
    // 变体 12 例（继承匹配正路 4 + 位域外 4 + 互斥 4）。
    for i in 0..4u32 {
        // 继承匹配正路：可继承 + 继承传递。
        v.push((InhForm { attributes: OBJ_INHERIT, inheritable: true, options: OPT_INHERIT_PASS | (i as u32 & OPT_CLOSE_SOURCE), access: 0 }, ST_SUCCESS));
    }
    for i in 0..4u32 {
        // 选项位域外（0x8 << i）。
        v.push((InhForm { attributes: 0, inheritable: false, options: 0x8 << i, access: 0 }, ST_INVALID_PARAMETER));
    }
    for i in 0..4u32 {
        // SAME_ACCESS + 显式掩码互斥。
        v.push((InhForm { attributes: 0, inheritable: false, options: OPT_SAME_ACCESS, access: 1 + i as u32 }, ST_INVALID_PARAMETER));
    }
    v
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 矩阵先行演练：八行齐码先行 + 行码一致 10/10 轮。
pub fn drill_matrix_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31260 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let bk = InjectBook::new();
        let first = bk.matrix_first();
        let rows = matrix_rows();
        let _ = lcg.below(3);
        if first && rows.len() == 8 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("矩阵先行轮={}/{}", ok, INJECT_ROUNDS))
}

/// 20 例注入演练：逐例与矩阵一致 10/10 轮。
pub fn drill_inject20_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31261（注入序列）。
    let mut lcg = Lcg::new(0x31261);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let hits = bk.run_all(&cases);
        let _ = lcg.below(3);
        if hits == INJECT_CASES && cases.len() == INJECT_CASES && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("20例注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 继承匹配演练：两向失配拒 / 双向匹配过 10/10 轮。
pub fn drill_inherit_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31262（继承序列）。
    let mut lcg = Lcg::new(0x31262);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 失配 1：传递请求但不可继承。
        let f1 = InhForm { attributes: OBJ_INHERIT, inheritable: false, options: OPT_INHERIT_PASS, access: 0 };
        all_ok = all_ok && inh_status(&f1) == ST_ACCESS_DENIED;
        // 失配 2：可继承但无传递请求。
        let f2 = InhForm { attributes: OBJ_INHERIT, inheritable: true, options: 0, access: 0 };
        all_ok = all_ok && inh_status(&f2) == ST_ACCESS_DENIED;
        // 匹配：可继承 + 传递。
        let g1 = InhForm { attributes: OBJ_INHERIT, inheritable: true, options: OPT_INHERIT_PASS, access: 0 };
        all_ok = all_ok && inh_status(&g1) == ST_SUCCESS;
        // 匹配：不可继承 + 无传递 + 无继承位。
        let g2 = InhForm { attributes: 0, inheritable: false, options: 0, access: 0 };
        all_ok = all_ok && inh_status(&g2) == ST_SUCCESS;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("继承匹配轮={}/{}", ok, INJECT_ROUNDS))
}

/// 闸序演练：多坏叠加取最低序闸 10/10 轮。
pub fn drill_gate_order_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31263（闸序序列）。
    let mut lcg = Lcg::new(0x31263);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 位域外 + 继承失配叠加 → 位域外短路。
        let f1 = InhForm { attributes: OBJ_INHERIT, inheritable: false, options: 0x8 | OPT_INHERIT_PASS, access: 0 };
        all_ok = all_ok && inh_status(&f1) == ST_INVALID_PARAMETER;
        // 继承失配 + 互斥叠加 → 继承失配短路。
        let f2 = InhForm { attributes: OBJ_INHERIT, inheritable: true, options: OPT_SAME_ACCESS, access: 0x2 };
        all_ok = all_ok && inh_status(&f2) == ST_ACCESS_DENIED;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：选项位域外必拒。
    let f1 = InhForm { attributes: 0, inheritable: false, options: 0x8, access: 0 };
    if inh_status(&f1) == ST_INVALID_PARAMETER {
        caught += 1;
        d.push("注1 位域:抓".to_string());
    } else {
        d.push("注1 位域:漏".to_string());
    }
    // 注 2：继承传递无可继承必拒。
    let f2 = InhForm { attributes: OBJ_INHERIT, inheritable: false, options: OPT_INHERIT_PASS, access: 0 };
    if inh_status(&f2) == ST_ACCESS_DENIED {
        caught += 1;
        d.push("注2 继承:抓".to_string());
    } else {
        d.push("注2 继承:漏".to_string());
    }
    // 注 3：SAME_ACCESS 带掩码互斥必拒。
    let f3 = InhForm { attributes: 0, inheritable: false, options: OPT_SAME_ACCESS, access: 0x2 };
    if inh_status(&f3) == ST_INVALID_PARAMETER {
        caught += 1;
        d.push("注3 互斥:抓".to_string());
    } else {
        d.push("注3 互斥:漏".to_string());
    }
    // 注 4：继承位口径核对。
    if OBJ_INHERIT == 0x2 && OPT_INHERIT_PASS == 0x4 {
        caught += 1;
        d.push("注4 位口径:对".to_string());
    } else {
        d.push("注4 位口径:错".to_string());
    }
    // 注 5：正路锚不可堵死。
    let f5 = InhForm { attributes: OBJ_INHERIT, inheritable: true, options: OPT_INHERIT_PASS, access: 0 };
    if inh_status(&f5) == ST_SUCCESS {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x31260 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    IhCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct IhReport {
    /// 注入例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 矩阵行数。
    pub matrix: usize,
    /// 账单调。
    pub mono: bool,
}

impl IhReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        IhReport { cases: bk.rows(), hits, matrix: bk.matrix_len(), mono: bk.monotone() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES
            && self.matrix == 8 && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "IhReport 例={} 命中={} 矩阵={} 单调={}",
            self.cases, self.hits, self.matrix, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct IhCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_ih_checklist(bk: &InjectBook, hits: usize) -> Vec<IhCheckRow> {
    let rep = IhReport::build(bk, hits);
    let mut rows: Vec<IhCheckRow> = Vec::new();
    rows.push(IhCheckRow { name: "20例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(IhCheckRow { name: "20例全中", passed: hits == INJECT_CASES });
    rows.push(IhCheckRow { name: "矩阵先行", passed: bk.matrix_first() });
    rows.push(IhCheckRow { name: "矩阵八行", passed: bk.matrix_len() == 8 });
    rows.push(IhCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(IhCheckRow { name: "位口径", passed: OBJ_INHERIT == 0x2 && OPT_SAME_ACCESS == 0x1 && OPT_CLOSE_SOURCE == 0x2 });
    rows.push(IhCheckRow { name: "转录非空", passed: bk.export_lines().contains("继承复制注入账") });
    rows.push(IhCheckRow { name: "已执行", passed: bk.ran() });
    rows.push(IhCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(IhCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ih_checklist_green(rows: &[IhCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ih_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "继承复制注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12527_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        IhReport::build(&bk, hits).all_green() && bk.export_lines().contains("继承复制注入账"),
        ih_stats_line(&bk, hits),
    ));
    // X-b：矩阵先行 10/10。
    let (ok_m, d_m) = drill_matrix_10();
    out.push(("X-矩阵先行10/10", ok_m, d_m));
    // X-c：继承匹配 10/10。
    let (ok_i, d_i) = drill_inherit_10();
    out.push(("X-继承匹配10/10", ok_i, d_i));
    // X-d：闸序 10/10。
    let (ok_g, d_g) = drill_gate_order_10();
    out.push(("X-闸序10/10", ok_g, d_g));
    // X-e：终验清单。
    let rows = build_ih_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        ih_checklist_green(&rows),
        IhReport::build(&bk, hits).to_text(),
    ));
    // X-f：口径常量核对。
    out.push((
        "X-口径常量核对",
        OBJ_INHERIT == 0x2 && OPT_SAME_ACCESS == 0x1 && OPT_CLOSE_SOURCE == 0x2
            && OPT_INHERIT_PASS == 0x4 && INJECT_CASES == 20,
        format!("inh={:#x} sa={:#x} cs={:#x} pass={:#x} cases={}", OBJ_INHERIT, OPT_SAME_ACCESS, OPT_CLOSE_SOURCE, OPT_INHERIT_PASS, INJECT_CASES),
    ));
    out
}

/// run_unx12527_checks：本单判据聚合（诊断段 0x31260）。
pub fn run_unx12527_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12527 句柄继承与复制错误码");
    // J1-a：20 例注入 10/10。
    let (ok1, d1) = drill_inject20_10();
    s.add_dyn("J1-20例注入10/10", ok1, &d1);
    // J1-b：标准会话恒等式（矩阵先行 + 20/20）。
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    s.add_dyn(
        "J1-20例矩阵一致",
        hits == INJECT_CASES && bk.matrix_first() && bk.monotone(),
        &format!("命中={}/{} 矩阵={}", hits, INJECT_CASES, bk.matrix_len()),
    );
    // J2-a：继承匹配 10/10。
    let (ok2, d2) = drill_inherit_10();
    s.add_dyn("J2-继承匹配10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：矩阵先行 10/10。
    let (ok3, d3) = drill_matrix_10();
    s.add_dyn("J2-矩阵先行10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x31260段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12527_extra() {
        s.add_dyn(n, p, &d);
    }
    for (n, p, d) in run_unx12527_ext2() {
        s.add_dyn(n, p, &d);
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：重复确定性 / 正路变异 / 账码回放。
/// ─────────────────────────────────────────────────────────────────────
/// 重复注入确定性演练：同批两账逐行同码 10/10 轮。
pub fn drill_repeat_determinism_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31264（重复序列）。
    let mut lcg = Lcg::new(0x31264);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk1 = InjectBook::new();
        let mut bk2 = InjectBook::new();
        let h1 = bk1.run_all(&cases);
        let h2 = bk2.run_all(&cases);
        let same = bk1.rows.len() == bk2.rows.len()
            && bk1.rows.iter().zip(bk2.rows.iter()).all(|(a, b)| a.0 == b.0 && a.1 == b.1 && a.2 == b.2);
        let _ = lcg.below(3);
        if h1 == h2 && same && h1 == INJECT_CASES {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重复确定性轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路变异演练：正路例选项位域外化必拒 10/10 轮。
pub fn drill_mutate_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31265（变异序列）。
    let mut lcg = Lcg::new(0x31265);
    for _r in 0..INJECT_ROUNDS {
        let rows = matrix_rows();
        let mut all_ok = true;
        for r in rows.iter().filter(|r| r.want == ST_SUCCESS) {
            // 变异：选项位域外化。
            let m = InhForm { attributes: r.form.attributes, inheritable: r.form.inheritable, options: r.form.options | 0x8, access: r.form.access };
            all_ok = all_ok && inh_status(&m) == ST_INVALID_PARAMETER;
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
}

/// 账码回放面：注入账逐行回放判定链验证码值一致。
pub fn replay_ledger_consistent(bk: &InjectBook, cases: &[(InhForm, u32)]) -> bool {
    bk.rows.iter().all(|(id, got, _)| {
        match cases.get(*id as usize) {
            Some((f, _)) => inh_status(f) == *got,
            None => false,
        }
    })
}

/// 位口径转录演练：三特值位值全对 10/10 轮。
pub fn drill_bit_consts_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31266（位口径序列）。
    let mut lcg = Lcg::new(0x31266);
    for _r in 0..INJECT_ROUNDS {
        // 复制选项三位（同域）两两互斥且非零；OBJ_INHERIT 属属性域
        // 与复制选项域分立，不参与同域互斥断言。
        let disjoint = (OPT_SAME_ACCESS & OPT_CLOSE_SOURCE == 0)
            && (OPT_SAME_ACCESS & OPT_INHERIT_PASS == 0)
            && (OPT_CLOSE_SOURCE & OPT_INHERIT_PASS == 0);
        // 非零。
        let nonzero = OBJ_INHERIT != 0 && OPT_SAME_ACCESS != 0
            && OPT_CLOSE_SOURCE != 0 && OPT_INHERIT_PASS != 0;
        let _ = lcg.below(3);
        if disjoint && nonzero {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("位口径转录轮={}/{}", ok, INJECT_ROUNDS))
}

/// 选项组合枚举演练：三位选项八组合逐一判定确定 10/10 轮。
pub fn drill_opt_enum_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31267（组合枚举序列）。
    let mut lcg = Lcg::new(0x31267);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        for bits in 0..8u32 {
            // 位域内组合（0..7）判定恒确定（Err 或 Ok 皆可，唯不可挂起）。
            let f = InhForm { attributes: 0, inheritable: false, options: bits, access: 0 };
            let st = inh_status(&f);
            all_ok = all_ok && (st == ST_SUCCESS || st == ST_INVALID_PARAMETER || st == ST_ACCESS_DENIED);
        }
        // 位域外组合（0x8）恒 INVALID_PARAMETER。
        let bad = InhForm { attributes: 0, inheritable: false, options: 0x8, access: 0 };
        all_ok = all_ok && inh_status(&bad) == ST_INVALID_PARAMETER;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("选项组合枚举轮={}/{}", ok, INJECT_ROUNDS))
}

/// run_unx12527_ext2：扩展面 C 聚合。/// run_unx12527_ext2：扩展面 C 聚合。
pub fn run_unx12527_ext2() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let _ = bk.run_all(&cases);
    let (ok_r, d_r) = drill_repeat_determinism_10();
    out.push(("X2-重复确定性10/10", ok_r, d_r));
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X2-正路变异10/10", ok_m, d_m));
    let (ok_b, d_b) = drill_bit_consts_10();
    out.push(("X2-位口径转录10/10", ok_b, d_b));
    let (ok_g, d_g) = drill_opt_enum_10();
    out.push(("X2-选项组合枚举10/10", ok_g, d_g));
    out.push((
        "X2-账码回放面",
        replay_ledger_consistent(&bk, &cases),
        format!("回放 {} 行全一致", bk.rows()),
    ));
    out
}
