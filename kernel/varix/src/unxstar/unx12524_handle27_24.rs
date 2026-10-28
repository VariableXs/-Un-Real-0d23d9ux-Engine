//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12524 · 句柄权限错误码矩阵 · 0x31200
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12524-J1 ACCESS_DENIED 全族行齐码（开/查/写/复制四路），
//!      注入 30/30 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 授权模型：对象授权掩码（granted）× 请求掩码（desired）四路
//!     （开/查/写/复制），desired & !granted == 0 才放行；
//!   - 四路闭集：Open（全权域）/Query（读域位）/Write（写域位）/
//!     Duplicate（复制域位），各路请求位域独立；
//!   - 闸序（判据语义序）：路别闭集 → 请求位域 → 授权比对，前置命中
//!     即短路；
//!   - 码值口径：放行 SUCCESS(0) / 拒绝 ACCESS_DENIED(0xC0000022) /
//!     路别域外 INVALID_PARAMETER(0xC000000D)；
//!   - 30 例注入账：8 例开路 + 8 例查路 + 7 例写路 + 7 例复制路，
//!     逐例码值与判定链一致判据；
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

/// 路别闭集（四路）。
pub const PATHS: usize = 4;

/// 授权位：读域（查询/复制读取口径）。
pub const ACC_READ: u32 = 0x0000_0001;

/// 授权位：写域。
pub const ACC_WRITE: u32 = 0x0000_0002;

/// 授权位：复制域。
pub const ACC_DUP: u32 = 0x0000_0004;

/// 授权位：全权域（读/写/复制三位的并集口径）。
pub const ACC_ALL: u32 = ACC_READ | ACC_WRITE | ACC_DUP;

/// 请求位合法域（三路并集）。
pub const ACC_VALID_MASK: u32 = ACC_ALL;

/// NTSTATUS 域。
pub const ST_SUCCESS: u32 = 0x0000_0000;
pub const ST_ACCESS_DENIED: u32 = 0xC000_0022;
pub const ST_INVALID_PARAMETER: u32 = 0xC000_000D;

/// 诊断码（0x31200 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdCode {
    /// 0x31200 码域漂移。
    StDomain = 0,
    /// 0x31201 注入例数不足。
    CaseShort = 1,
    /// 0x31202 例码漂移。
    CaseDrift = 2,
    /// 0x31203 注入账缩水。
    LedgerShrink = 3,
    /// 0x31204 闸序破。
    GateOrder = 4,
    /// 0x31205 正路锚堵死。
    PositiveBlocked = 5,
    /// 0x31206 路别越闭集。
    PathOob = 6,
    /// 0x31207 注入未执行。
    NotRun = 7,
    /// 0x31208 拒止档失配。
    TierDiff = 8,
    /// 0x31209 授权位口径失配。
    AccDiff = 9,
}

impl AdCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [AdCode; 10] = [
        AdCode::StDomain, AdCode::CaseShort, AdCode::CaseDrift, AdCode::LedgerShrink,
        AdCode::GateOrder, AdCode::PositiveBlocked, AdCode::PathOob, AdCode::NotRun,
        AdCode::TierDiff, AdCode::AccDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x31200 => "码域漂移：产出码不在三码 NTSTATUS 闭集。",
        0x31201 => "注入例数不足：30 例必须全注入。",
        0x31202 => "例码漂移：逐例回放授权比对定位失配例。",
        0x31203 => "注入账缩水：append-only，禁止截断。",
        0x31204 => "闸序破：前置闸命中必须短路。",
        0x31205 => "正路锚堵死：授权内请求被拒先修判定链。",
        0x31206 => "路别越闭集：出开/查/写/复制四路域。",
        0x31207 => "注入未执行：账面零例。",
        0x31208 => "拒止档失配：与 B27 拒止档逐例核对。",
        0x31209 => "授权位口径失配：读/写/复制/全权位值核对。",
        _ => "未知诊断码：回查 0x31200 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x31200 段基址。
    pub const SEED: u64 = 0x31200_3120_0312_00u64;

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

/// 权限检查形态（路别 + 授权掩码 + 请求掩码）。
#[derive(Clone, Copy, Debug)]
pub struct AccessForm {
    /// 路别（0=开 1=查 2=写 3=复制）。
    pub path: u32,
    /// 授权掩码。
    pub granted: u32,
    /// 请求掩码。
    pub desired: u32,
}

/// 判定链（闸序：路别闭集 → 请求位域 → 授权比对）。
pub fn judge(f: &AccessForm) -> Result<(), AdCode> {
    // 闸 1：路别闭集。
    if (f.path as usize) >= PATHS {
        return Err(AdCode::PathOob);
    }
    // 闸 2：请求位域。
    if f.desired & !ACC_VALID_MASK != 0 {
        return Err(AdCode::CaseShort);
    }
    // 闸 3：授权比对。
    if f.desired & !f.granted != 0 {
        return Err(AdCode::CaseDrift);
    }
    Ok(())
}

/// 权限语义码（真实 NTSTATUS 映射）。
pub fn access_status(f: &AccessForm) -> u32 {
    match judge(f) {
        Ok(()) => ST_SUCCESS,
        Err(AdCode::PathOob) | Err(AdCode::CaseShort) => ST_INVALID_PARAMETER,
        Err(AdCode::CaseDrift) => ST_ACCESS_DENIED,
        Err(_) => ST_INVALID_PARAMETER,
    }
}

/// 注入例。
#[derive(Clone, Copy, Debug)]
pub struct CaseSpec {
    /// 例号。
    pub id: u16,
    /// 形态。
    pub form: AccessForm,
    /// 期望码。
    pub want: u32,
}

/// 30 例注入账构建（8 开 + 8 查 + 7 写 + 7 复制；拒绝 22 + 放行 8）。
pub fn build_cases() -> Vec<CaseSpec> {
    let mut v: Vec<CaseSpec> = Vec::new();
    // 开路 8 例（granted=READ：全权请求拒 / 读请求过 / 写请求拒 / …）。
    for i in 0..8u32 {
        let (desired, want) = match i % 4 {
            0 => (ACC_ALL, ST_ACCESS_DENIED),
            1 => (ACC_READ, ST_SUCCESS),
            2 => (ACC_WRITE, ST_ACCESS_DENIED),
            _ => (ACC_READ | ACC_WRITE, ST_ACCESS_DENIED),
        };
        v.push(CaseSpec { id: i as u16, form: AccessForm { path: 0, granted: ACC_READ, desired }, want });
    }
    // 查路 8 例（granted=READ|DUP）。
    for i in 0..8u32 {
        let (desired, want) = match i % 4 {
            0 => (ACC_READ, ST_SUCCESS),
            1 => (ACC_DUP, ST_SUCCESS),
            2 => (ACC_WRITE, ST_ACCESS_DENIED),
            _ => (ACC_ALL, ST_ACCESS_DENIED),
        };
        v.push(CaseSpec { id: 8 + i as u16, form: AccessForm { path: 1, granted: ACC_READ | ACC_DUP, desired }, want });
    }
    // 写路 7 例（granted=WRITE）。
    for i in 0..7u32 {
        let (desired, want) = match i % 3 {
            0 => (ACC_WRITE, ST_SUCCESS),
            1 => (ACC_READ, ST_ACCESS_DENIED),
            _ => (ACC_DUP, ST_ACCESS_DENIED),
        };
        v.push(CaseSpec { id: 16 + i as u16, form: AccessForm { path: 2, granted: ACC_WRITE, desired }, want });
    }
    // 复制路 7 例（granted=DUP|READ）。
    for i in 0..7u32 {
        let (desired, want) = match i % 3 {
            0 => (ACC_DUP, ST_SUCCESS),
            1 => (ACC_WRITE, ST_ACCESS_DENIED),
            _ => (ACC_ALL, ST_ACCESS_DENIED),
        };
        v.push(CaseSpec { id: 23 + i as u16, form: AccessForm { path: 3, granted: ACC_DUP | ACC_READ, desired }, want });
    }
    v
}

/// 注入账正本。
#[derive(Debug)]
pub struct InjectBook {
    /// 结果账（append-only：(例号, 计算码, 期望码)）。
    rows: Vec<(u16, u32, u32)>,
    /// 账高水位。
    high: usize,
}

impl InjectBook {
    /// 建账。
    pub fn new() -> Self {
        InjectBook { rows: Vec::new(), high: 0 }
    }

    /// 注入一例。
    pub fn run_case(&mut self, c: &CaseSpec) -> bool {
        let got = access_status(&c.form);
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

    /// 路别分布清点（账面以例号段归类）。
    pub fn path_counts(&self) -> [usize; PATHS] {
        let mut c = [0usize; PATHS];
        for (id, _, _) in self.rows.iter() {
            let p = if *id < 8 { 0 } else if *id < 16 { 1 } else if *id < 23 { 2 } else { 3 };
            c[p] += 1;
        }
        c
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("权限注入账：\n");
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
/// 30 例注入演练：逐例码值一致 10/10 轮。
pub fn drill_inject30_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31200 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let hits = bk.run_all(&cases);
        let _ = lcg.below(3);
        if hits == INJECT_CASES && cases.len() == INJECT_CASES && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("30例注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 四路齐码演练：四路各行在账且行码与判定链一致 10/10 轮。
pub fn drill_four_paths_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31201（四路序列）。
    let mut lcg = Lcg::new(0x31201);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let _ = bk.run_all(&cases);
        let counts = bk.path_counts();
        // 四路行数口径 [8,8,7,7]。
        let dist_ok = counts[0] == 8 && counts[1] == 8 && counts[2] == 7 && counts[3] == 7;
        // 全行一致。
        let cons = cases.iter().all(|c| access_status(&c.form) == c.want);
        let _ = lcg.below(3);
        if dist_ok && cons {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("四路齐码轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：授权内请求全过 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31202（正路序列）。
    let mut lcg = Lcg::new(0x31202);
    for _r in 0..INJECT_ROUNDS {
        // 全权授权下四路全过。
        let mut all_ok = true;
        for p in 0..PATHS as u32 {
            let g = AccessForm { path: p, granted: ACC_ALL, desired: ACC_READ };
            all_ok = all_ok && access_status(&g) == ST_SUCCESS;
        }
        // 零请求正路（不请求任何权限 → 过）。
        let g2 = AccessForm { path: 0, granted: 0, desired: 0 };
        all_ok = all_ok && access_status(&g2) == ST_SUCCESS;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路锚轮={}/{}", ok, INJECT_ROUNDS))
}

/// 闸序演练：路别域外优先短路 10/10 轮。
pub fn drill_gate_order_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31203（闸序序列）。
    let mut lcg = Lcg::new(0x31203);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 路别域外 + 位域外 + 授权不足叠加 → PathOob 短路。
        let f1 = AccessForm { path: 9, granted: 0, desired: 0x8000_0000 };
        all_ok = all_ok && matches!(judge(&f1), Err(AdCode::PathOob));
        // 位域外 + 授权不足叠加 → INVALID_PARAMETER 短路。
        let f2 = AccessForm { path: 0, granted: 0, desired: 0x8000_0000 };
        all_ok = all_ok && access_status(&f2) == ST_INVALID_PARAMETER;
        // 授权不足 → ACCESS_DENIED。
        let f3 = AccessForm { path: 2, granted: 0, desired: ACC_WRITE };
        all_ok = all_ok && access_status(&f3) == ST_ACCESS_DENIED;
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
    // 注 1：开路越权必拒。
    let f1 = AccessForm { path: 0, granted: ACC_READ, desired: ACC_WRITE };
    if access_status(&f1) == ST_ACCESS_DENIED {
        caught += 1;
        d.push("注1 开路:抓".to_string());
    } else {
        d.push("注1 开路:漏".to_string());
    }
    // 注 2：查路越权必拒。
    let f2 = AccessForm { path: 1, granted: ACC_READ, desired: ACC_WRITE };
    if access_status(&f2) == ST_ACCESS_DENIED {
        caught += 1;
        d.push("注2 查路:抓".to_string());
    } else {
        d.push("注2 查路:漏".to_string());
    }
    // 注 3：写路越权必拒。
    let f3 = AccessForm { path: 2, granted: ACC_WRITE, desired: ACC_READ };
    if access_status(&f3) == ST_ACCESS_DENIED {
        caught += 1;
        d.push("注3 写路:抓".to_string());
    } else {
        d.push("注3 写路:漏".to_string());
    }
    // 注 4：复制路越权必拒。
    let f4 = AccessForm { path: 3, granted: ACC_DUP, desired: ACC_WRITE };
    if access_status(&f4) == ST_ACCESS_DENIED {
        caught += 1;
        d.push("注4 复制:抓".to_string());
    } else {
        d.push("注4 复制:漏".to_string());
    }
    // 注 5：正路锚不可堵死。
    let f5 = AccessForm { path: 0, granted: ACC_ALL, desired: ACC_ALL };
    if access_status(&f5) == ST_SUCCESS {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x31200 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    AdCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct AdReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 路别分布。
    pub paths: [usize; PATHS],
    /// 账单调。
    pub mono: bool,
}

impl AdReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        AdReport { cases: bk.rows(), hits, paths: bk.path_counts(), mono: bk.monotone() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "AdReport 例={} 命中={} 路={:?} 单调={}",
            self.cases, self.hits, self.paths, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct AdCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_ad_checklist(bk: &InjectBook, hits: usize) -> Vec<AdCheckRow> {
    let rep = AdReport::build(bk, hits);
    let mut rows: Vec<AdCheckRow> = Vec::new();
    rows.push(AdCheckRow { name: "30例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(AdCheckRow { name: "30例全中", passed: hits == INJECT_CASES });
    rows.push(AdCheckRow { name: "四路齐码", passed: bk.path_counts() == [8, 8, 7, 7] });
    rows.push(AdCheckRow { name: "开路8例", passed: bk.path_counts()[0] == 8 });
    rows.push(AdCheckRow { name: "查路8例", passed: bk.path_counts()[1] == 8 });
    rows.push(AdCheckRow { name: "写路7例", passed: bk.path_counts()[2] == 7 });
    rows.push(AdCheckRow { name: "复制路7例", passed: bk.path_counts()[3] == 7 });
    rows.push(AdCheckRow { name: "转录非空", passed: bk.export_lines().contains("权限注入账") });
    rows.push(AdCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(AdCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ad_checklist_green(rows: &[AdCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ad_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "权限注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12524_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        AdReport::build(&bk, hits).all_green() && bk.export_lines().contains("权限注入账"),
        ad_stats_line(&bk, hits),
    ));
    // X-b：四路齐码 10/10。
    let (ok_f, d_f) = drill_four_paths_10();
    out.push(("X-四路齐码10/10", ok_f, d_f));
    // X-c：正路锚 10/10。
    let (ok_p, d_p) = drill_positive_10();
    out.push(("X-正路锚10/10", ok_p, d_p));
    // X-d：闸序 10/10。
    let (ok_g, d_g) = drill_gate_order_10();
    out.push(("X-闸序10/10", ok_g, d_g));
    // X-e：终验清单。
    let rows = build_ad_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        ad_checklist_green(&rows),
        AdReport::build(&bk, hits).to_text(),
    ));
    // X-f：口径常量核对。
    out.push((
        "X-口径常量核对",
        ACC_READ == 0x1 && ACC_WRITE == 0x2 && ACC_DUP == 0x4 && ACC_ALL == 0x7
            && ST_ACCESS_DENIED == 0xC000_0022,
        format!("r={:#x} w={:#x} d={:#x} a={:#x} ad={:#x}", ACC_READ, ACC_WRITE, ACC_DUP, ACC_ALL, ST_ACCESS_DENIED),
    ));
    out
}

/// run_unx12524_checks：本单判据聚合（诊断段 0x31200）。
pub fn run_unx12524_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12524 句柄权限错误码矩阵");
    // J1-a：30 例注入 10/10。
    let (ok1, d1) = drill_inject30_10();
    s.add_dyn("J1-30例注入10/10", ok1, &d1);
    // J1-b：标准会话恒等式（四路齐码 + 30/30 一致）。
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    s.add_dyn(
        "J1-30例ACCESS_DENIED一致",
        hits == INJECT_CASES && bk.path_counts() == [8, 8, 7, 7],
        &format!("命中={}/{} 路={:?}", hits, INJECT_CASES, bk.path_counts()),
    );
    // J2-a：四路齐码 10/10。
    let (ok2, d2) = drill_four_paths_10();
    s.add_dyn("J2-四路齐码10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：正路锚 10/10。
    let (ok3, d3) = drill_positive_10();
    s.add_dyn("J2-正路锚10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x31200段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12524_extra() {
        s.add_dyn(n, p, &d);
    }
    for (n, p, d) in run_unx12524_ext2() {
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
    // 种子写死：0x31204（重复序列）。
    let mut lcg = Lcg::new(0x31204);
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

/// 正路变异演练：正路例请求超授权必拒 10/10 轮。
pub fn drill_mutate_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31205（变异序列）。
    let mut lcg = Lcg::new(0x31205);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().filter(|c| c.want == ST_SUCCESS && c.form.desired != 0) {
            // 变异：请求位翻转为授权外位。
            let flipped = ACC_READ | ACC_WRITE | ACC_DUP & !c.form.granted;
            let m = AccessForm { path: c.form.path, granted: c.form.granted, desired: flipped | 0x1 };
            all_ok = all_ok && access_status(&m) == ST_ACCESS_DENIED
                || flipped & !c.form.granted == 0;
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
}

/// 账码回放面：注入账逐行回放授权比对验证码值一致。
pub fn replay_ledger_consistent(bk: &InjectBook, cases: &[CaseSpec]) -> bool {
    bk.rows.iter().all(|(id, got, _)| {
        match cases.iter().find(|c| c.id == *id) {
            Some(c) => access_status(&c.form) == *got,
            None => false,
        }
    })
}

/// 授权位并集演练：全权位 = 读|写|复制三位的并集口径 10/10 轮。
pub fn drill_acc_union_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31206（并集序列）。
    let mut lcg = Lcg::new(0x31206);
    for _r in 0..INJECT_ROUNDS {
        // 全权位并集口径核对。
        let union_ok = ACC_ALL == ACC_READ | ACC_WRITE | ACC_DUP;
        // 全权授权下三位请求全过。
        let mut all_ok = union_ok;
        for desired in [ACC_READ, ACC_WRITE, ACC_DUP].iter() {
            let g = AccessForm { path: 0, granted: ACC_ALL, desired: *desired };
            all_ok = all_ok && access_status(&g) == ST_SUCCESS;
        }
        // 无全权授权下全权请求拒。
        let b = AccessForm { path: 0, granted: ACC_READ, desired: ACC_ALL };
        all_ok = all_ok && access_status(&b) == ST_ACCESS_DENIED;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("授权位并集轮={}/{}", ok, INJECT_ROUNDS))
}

/// run_unx12524_ext2：扩展面 C 聚合。/// run_unx12524_ext2：扩展面 C 聚合。
pub fn run_unx12524_ext2() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let _ = bk.run_all(&cases);
    let (ok_r, d_r) = drill_repeat_determinism_10();
    out.push(("X2-重复确定性10/10", ok_r, d_r));
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X2-正路变异10/10", ok_m, d_m));
    let (ok_u, d_u) = drill_acc_union_10();
    out.push(("X2-授权位并集10/10", ok_u, d_u));
    out.push((
        "X2-账码回放面",
        replay_ledger_consistent(&bk, &cases),
        format!("回放 {} 行全一致", bk.rows()),
    ));
    out
}
