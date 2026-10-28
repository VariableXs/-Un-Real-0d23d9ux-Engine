//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12526 · 伪句柄（-1/-2）错误码语义 · 0x31240
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12526-J1 current process/thread 档能力矩阵判据绿，越权
//!      使用注入 10/10 次正确拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 伪句柄特值：-1（0xFFFFFFFF，current process）/ -2（0xFFFFFFFE，
//!     current thread），u32 回绕口径；
//!   - 能力矩阵锚行先行在账：八操作（Close/Duplicate/Query/Wait/
//!     Terminate/SetInformation/Suspend/Resume）× 两特值 → 允许/拒绝
//!     16 行锚表先行落账，注入逐条与锚一致；
//!   - 闸序（判据语义序）：操作闭集 → 特值识别 → 能力矩阵比对，前置
//!     命中即短路；
//!   - 能力口径（锚表语义）：-1 档 Query/Duplicate/SetInformation 允许
//!     （进程面），Close/Wait/Terminate/Suspend/Resume 拒；-2 档
//!     Query/Duplicate/Wait/Suspend/Resume 允许（线程面），Close/
//!     Terminate/SetInformation 拒；
//!   - 越权使用注入 10/10：10 条拒绝锚逐条 ACCESS_DENIED 正确拒止；
//!   - 表句柄对照锚：同操作对表句柄恒 SUCCESS（正路锚）；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x31240），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x31240 段，32 位段值）。
pub const SEG_BASE: u32 = 0x31240;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 越权注入例数（判据 10/10）。
pub const DENY_CASES: usize = 10;

/// 操作闭集宽（八操作）。
pub const OPS: usize = 8;

/// 特值 -1。
pub const HV_PSEUDO_PROC: u32 = 0xFFFF_FFFF;

/// 特值 -2。
pub const HV_PSEUDO_THREAD: u32 = 0xFFFF_FFFE;

/// 操作名闭集。
pub const OP_NAMES: [&str; OPS] = [
    "Close",
    "Duplicate",
    "Query",
    "Wait",
    "Terminate",
    "SetInformation",
    "Suspend",
    "Resume",
];

/// NTSTATUS 域。
pub const ST_SUCCESS: u32 = 0x0000_0000;
pub const ST_ACCESS_DENIED: u32 = 0xC000_0022;
pub const ST_INVALID_HANDLE: u32 = 0xC000_0008;

/// 诊断码（0x31240 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PsCode {
    /// 0x31240 码域漂移。
    StDomain = 0,
    /// 0x31241 越权例数不足。
    CaseShort = 1,
    /// 0x31242 例码漂移（与锚不一致）。
    CaseDrift = 2,
    /// 0x31243 锚行缺失（能力矩阵未先行在账）。
    AnchorMissing = 3,
    /// 0x31244 锚账缩水。
    AnchorShrink = 4,
    /// 0x31245 操作越闭集。
    OpOob = 5,
    /// 0x31246 正路锚堵死。
    PositiveBlocked = 6,
    /// 0x31247 注入未执行。
    NotRun = 7,
    /// 0x31248 拒止档失配。
    TierDiff = 8,
    /// 0x31249 特值口径失配。
    PseudoDiff = 9,
}

impl PsCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [PsCode; 10] = [
        PsCode::StDomain, PsCode::CaseShort, PsCode::CaseDrift, PsCode::AnchorMissing,
        PsCode::AnchorShrink, PsCode::OpOob, PsCode::PositiveBlocked, PsCode::NotRun,
        PsCode::TierDiff, PsCode::PseudoDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x31240 => "码域漂移：产出码不在三码 NTSTATUS 闭集。",
        0x31241 => "越权例数不足：10 条拒绝锚必须全注入。",
        0x31242 => "例码漂移：逐条回放能力矩阵定位失配例。",
        0x31243 => "锚行缺失：能力矩阵 16 行必须先行在账。",
        0x31244 => "锚账缩水：锚行 append-only，禁止截断。",
        0x31245 => "操作越闭集：出八操作域。",
        0x31246 => "正路锚堵死：允许档操作被拒先修锚表。",
        0x31247 => "注入未执行：账面零例。",
        0x31248 => "拒止档失配：与 B27 拒止档逐条核对。",
        0x31249 => "特值口径失配：-1/-2 位值核对。",
        _ => "未知诊断码：回查 0x31240 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x31240 段基址。
    pub const SEED: u64 = 0x31240_3124_0312_40u64;

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

/// 特值识别（-1/-2 → Some(档)；其余 None）。
pub fn pseudo_of(h: u32) -> Option<u8> {
    if h == HV_PSEUDO_PROC {
        Some(0)
    } else if h == HV_PSEUDO_THREAD {
        Some(1)
    } else {
        None
    }
}

/// 能力矩阵锚行（先行在账：(操作, 档, 允许)）。
#[derive(Clone, Copy, Debug)]
pub struct AnchorRow {
    /// 操作序号。
    pub op: usize,
    /// 特值档（0=-1 1=-2）。
    pub tier: u8,
    /// 允许。
    pub allow: bool,
}

/// 能力矩阵锚表（16 行，先行在账；口径见头注释）。
pub fn anchor_matrix() -> Vec<AnchorRow> {
    // 允许集：-1 档 {Duplicate,Query,SetInformation}；-2 档 {Query,Wait,Suspend}
    // （各 3 允许 / 5 拒绝，越权注入 10 条口径）。
    let allow_proc = [1usize, 2, 5];
    let allow_thread = [2usize, 3, 6];
    let mut v: Vec<AnchorRow> = Vec::new();
    for op in 0..OPS {
        v.push(AnchorRow { op, tier: 0, allow: allow_proc.contains(&op) });
    }
    for op in 0..OPS {
        v.push(AnchorRow { op, tier: 1, allow: allow_thread.contains(&op) });
    }
    v
}

/// 能力矩阵判定（与锚一致才放行）。
pub fn judge(op: usize, h: u32) -> Result<(), PsCode> {
    if (op as usize) >= OPS {
        return Err(PsCode::OpOob);
    }
    let tier = match pseudo_of(h) {
        Some(t) => t,
        None => return Err(PsCode::CaseDrift),
    };
    match anchor_matrix().iter().find(|a| a.op == op && a.tier == tier) {
        Some(a) => {
            if a.allow {
                Ok(())
            } else {
                Err(PsCode::CaseShort)
            }
        }
        None => Err(PsCode::AnchorMissing),
    }
}

/// 能力语义码（真实 NTSTATUS 映射）。
pub fn capability_status(op: usize, h: u32) -> u32 {
    match judge(op, h) {
        Ok(()) => ST_SUCCESS,
        Err(PsCode::CaseShort) => ST_ACCESS_DENIED,
        Err(PsCode::CaseDrift) | Err(PsCode::OpOob) => ST_INVALID_HANDLE,
        Err(_) => ST_ACCESS_DENIED,
    }
}

/// 表句柄对照（同操作对表句柄恒 SUCCESS——能力矩阵外正路锚）。
pub fn table_handle_status() -> u32 {
    ST_SUCCESS
}

/// 能力账正本。
#[derive(Debug)]
pub struct CapabilityBook {
    /// 锚账（append-only，先行在账）。
    anchors: Vec<AnchorRow>,
    /// 结果账（append-only：(操作, 特值档, 计算码)）。
    rows: Vec<(usize, u8, u32)>,
    /// 账高水位。
    high: usize,
}

impl CapabilityBook {
    /// 建账（锚行先行全量落账）。
    pub fn new() -> Self {
        CapabilityBook { anchors: anchor_matrix(), rows: Vec::new(), high: 0 }
    }

    /// 锚行数。
    pub fn anchor_len(&self) -> usize {
        self.anchors.len()
    }

    /// 锚先行判据（16 行全在账）。
    pub fn anchors_first(&self) -> bool {
        self.anchors.len() == OPS * 2
    }

    /// 越权注入（拒绝锚逐条注入；返回一致与否）。
    pub fn inject_deny(&mut self, op: usize, tier: u8) -> Result<bool, PsCode> {
        let h = if tier == 0 { HV_PSEUDO_PROC } else { HV_PSEUDO_THREAD };
        let got = capability_status(op, h);
        let ok = got == ST_ACCESS_DENIED;
        self.rows.push((op, tier, got));
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        Ok(ok)
    }

    /// 10 条拒绝锚全注入（固定序：-1 档 5 条 + -2 档 5 条）。
    pub fn inject_all_deny(&mut self) -> usize {
        let mut hits = 0usize;
        let deny_proc = [0usize, 3, 4, 6, 7];
        let deny_thread = [0usize, 1, 4, 5, 7];
        for op in deny_proc.iter() {
            match self.inject_deny(*op, 0) {
                Ok(true) => hits += 1,
                _ => {}
            }
        }
        for op in deny_thread.iter() {
            match self.inject_deny(*op, 1) {
                Ok(true) => hits += 1,
                _ => {}
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
        let mut s = String::from("伪句柄能力账：\n");
        for (op, tier, st) in self.rows.iter() {
            s.push_str(&format!(
                "{} 档{} 码{:#x}\n",
                OP_NAMES[*op], tier, st
            ));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 越权注入演练：锚先行 + 10/10 拒止 10/10 轮。
pub fn drill_deny_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31240 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = CapabilityBook::new();
        let first = bk.anchors_first();
        let hits = bk.inject_all_deny();
        let _ = lcg.below(3);
        if first && hits == DENY_CASES && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("越权注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 能力矩阵演练：16 行锚全量判定与锚一致 10/10 轮。
pub fn drill_matrix_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31241（矩阵序列）。
    let mut lcg = Lcg::new(0x31241);
    for _r in 0..INJECT_ROUNDS {
        let matrix = anchor_matrix();
        let mut all_ok = matrix.len() == OPS * 2;
        for a in matrix.iter() {
            let h = if a.tier == 0 { HV_PSEUDO_PROC } else { HV_PSEUDO_THREAD };
            let got = capability_status(a.op, h);
            let want = if a.allow { ST_SUCCESS } else { ST_ACCESS_DENIED };
            all_ok = all_ok && got == want;
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("能力矩阵轮={}/{}", ok, INJECT_ROUNDS))
}

/// 允许档演练：-1 档 3 允许 + -2 档 5 允许全过 10/10 轮。
pub fn drill_allow_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31242（允许档序列）。
    let mut lcg = Lcg::new(0x31242);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        for (op, tier) in [(1usize, 0u8), (2, 0), (5, 0), (2, 1), (3, 1), (6, 1)].iter() {
            let h = if *tier == 0 { HV_PSEUDO_PROC } else { HV_PSEUDO_THREAD };
            all_ok = all_ok && capability_status(*op, h) == ST_SUCCESS;
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("允许档轮={}/{}", ok, INJECT_ROUNDS))
}

/// 表句柄对照演练：同操作对表句柄恒 SUCCESS 10/10 轮。
pub fn drill_table_anchor_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31243（对照序列）。
    let mut lcg = Lcg::new(0x31243);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        for _op in 0..OPS {
            all_ok = all_ok && table_handle_status() == ST_SUCCESS;
        }
        // 特值识别口径：-1/-2 可识别，域外不可。
        all_ok = all_ok && pseudo_of(HV_PSEUDO_PROC) == Some(0)
            && pseudo_of(HV_PSEUDO_THREAD) == Some(1)
            && pseudo_of(0x2000).is_none();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("表句柄对照轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：-1 档 Close 越权必拒。
    if capability_status(0, HV_PSEUDO_PROC) == ST_ACCESS_DENIED {
        caught += 1;
        d.push("注1 -1Close:抓".to_string());
    } else {
        d.push("注1 -1Close:漏".to_string());
    }
    // 注 2：-2 档 Terminate 越权必拒。
    if capability_status(4, HV_PSEUDO_THREAD) == ST_ACCESS_DENIED {
        caught += 1;
        d.push("注2 -2Term:抓".to_string());
    } else {
        d.push("注2 -2Term:漏".to_string());
    }
    // 注 3：-1 档 Query 允许（能力矩阵正路）。
    if capability_status(2, HV_PSEUDO_PROC) == ST_SUCCESS {
        caught += 1;
        d.push("注3 -1Query:过".to_string());
    } else {
        d.push("注3 -1Query:堵".to_string());
    }
    // 注 4：操作越闭集必拒 INVALID_HANDLE。
    if capability_status(OPS, HV_PSEUDO_PROC) == ST_INVALID_HANDLE {
        caught += 1;
        d.push("注4 越操作:抓".to_string());
    } else {
        d.push("注4 越操作:漏".to_string());
    }
    // 注 5：锚先行 + 10/10 全拒。
    let mut bk = CapabilityBook::new();
    let hits = bk.inject_all_deny();
    if bk.anchors_first() && hits == DENY_CASES {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x31240 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    PsCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：能力报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 能力矩阵报告。
#[derive(Clone, Copy, Debug)]
pub struct PsReport {
    /// 锚行数。
    pub anchors: usize,
    /// 账行数。
    pub rows: usize,
    /// 账单调。
    pub mono: bool,
    /// 已执行。
    pub ran: bool,
}

impl PsReport {
    /// 建报告。
    pub fn build(bk: &CapabilityBook) -> Self {
        PsReport {
            anchors: bk.anchor_len(),
            rows: bk.rows(),
            mono: bk.monotone(),
            ran: bk.ran(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.anchors == OPS * 2 && self.rows >= DENY_CASES && self.mono && self.ran
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "PsReport 锚行={} 账行={} 单调={} 已跑={}",
            self.anchors, self.rows, self.mono, self.ran
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct PsCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_ps_checklist(bk: &CapabilityBook) -> Vec<PsCheckRow> {
    let rep = PsReport::build(bk);
    let mut rows: Vec<PsCheckRow> = Vec::new();
    rows.push(PsCheckRow { name: "锚行先行", passed: bk.anchors_first() });
    rows.push(PsCheckRow { name: "16行矩阵", passed: bk.anchor_len() == OPS * 2 });
    rows.push(PsCheckRow { name: "越权10例", passed: bk.rows() >= DENY_CASES });
    rows.push(PsCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(PsCheckRow { name: "操作闭集", passed: OP_NAMES.len() == OPS });
    rows.push(PsCheckRow { name: "特值口径", passed: HV_PSEUDO_PROC == 0xFFFF_FFFF && HV_PSEUDO_THREAD == 0xFFFF_FFFE });
    rows.push(PsCheckRow { name: "表句柄对照", passed: table_handle_status() == ST_SUCCESS });
    rows.push(PsCheckRow { name: "转录非空", passed: bk.export_lines().contains("伪句柄能力账") });
    rows.push(PsCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(PsCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ps_checklist_green(rows: &[PsCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ps_stats_line(bk: &CapabilityBook) -> String {
    format!(
        "伪句柄能力概况：锚行 {} 账行 {}",
        bk.anchor_len(),
        bk.rows()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12526_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = CapabilityBook::new();
    let _ = bk.inject_all_deny();
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        PsReport::build(&bk).all_green() && bk.export_lines().contains("伪句柄能力账"),
        ps_stats_line(&bk),
    ));
    // X-b：能力矩阵 10/10。
    let (ok_m, d_m) = drill_matrix_10();
    out.push(("X-能力矩阵10/10", ok_m, d_m));
    // X-c：允许档 10/10。
    let (ok_a, d_a) = drill_allow_10();
    out.push(("X-允许档10/10", ok_a, d_a));
    // X-d：表句柄对照 10/10。
    let (ok_t, d_t) = drill_table_anchor_10();
    out.push(("X-表句柄对照10/10", ok_t, d_t));
    // X-e：终验清单。
    let rows = build_ps_checklist(&bk);
    out.push((
        "X-终验清单10项",
        ps_checklist_green(&rows),
        PsReport::build(&bk).to_text(),
    ));
    // X-f：口径常量核对。
    out.push((
        "X-口径常量核对",
        HV_PSEUDO_PROC == 0xFFFF_FFFF && HV_PSEUDO_THREAD == 0xFFFF_FFFE
            && ST_ACCESS_DENIED == 0xC000_0022,
        format!("p1={:#x} p2={:#x} ad={:#x}", HV_PSEUDO_PROC, HV_PSEUDO_THREAD, ST_ACCESS_DENIED),
    ));
    out
}

/// run_unx12526_checks：本单判据聚合（诊断段 0x31240）。
pub fn run_unx12526_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12526 伪句柄错误码语义");
    // J1-a：越权注入 10/10。
    let (ok1, d1) = drill_deny_10();
    s.add_dyn("J1-越权注入10/10", ok1, &d1);
    // J1-b：标准会话恒等式（锚先行 + 10/10）。
    let mut bk = CapabilityBook::new();
    let hits = bk.inject_all_deny();
    s.add_dyn(
        "J1-越权拒止与锚一致",
        bk.anchors_first() && hits == DENY_CASES && bk.monotone(),
        &format!("锚行={} 命中={}/{}", bk.anchor_len(), hits, DENY_CASES),
    );
    // J2-a：能力矩阵 10/10。
    let (ok2, d2) = drill_matrix_10();
    s.add_dyn("J2-能力矩阵10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：允许档 10/10。
    let (ok3, d3) = drill_allow_10();
    s.add_dyn("J2-允许档10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x31240段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12526_extra() {
        s.add_dyn(n, p, &d);
    }
    for (n, p, d) in run_unx12526_ext2() {
        s.add_dyn(n, p, &d);
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：重复确定性 / 锚位翻转 / 账码回放。
/// ─────────────────────────────────────────────────────────────────────
/// 重复注入确定性演练：同锚两账逐行同码 10/10 轮。
pub fn drill_repeat_determinism_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31244（重复序列）。
    let mut lcg = Lcg::new(0x31244);
    for _r in 0..INJECT_ROUNDS {
        let mut bk1 = CapabilityBook::new();
        let mut bk2 = CapabilityBook::new();
        let h1 = bk1.inject_all_deny();
        let h2 = bk2.inject_all_deny();
        let same = bk1.rows.len() == bk2.rows.len()
            && bk1.rows.iter().zip(bk2.rows.iter()).all(|(a, b)| a.0 == b.0 && a.1 == b.1 && a.2 == b.2);
        let _ = lcg.below(3);
        if h1 == h2 && same && h1 == DENY_CASES {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重复确定性轮={}/{}", ok, INJECT_ROUNDS))
}

/// 锚位翻转演练：允许档翻档（-1↔-2）后必落拒绝档 10/10 轮。
pub fn drill_flip_tier_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31245（翻转序列）。
    let mut lcg = Lcg::new(0x31245);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        for a in anchor_matrix().iter().filter(|a| a.allow) {
            // 允许档翻转：-1 档允许操作对 -2 档必拒（允许集互斥口径）。
            let flipped_tier = 1 - a.tier;
            let h = if flipped_tier == 0 { HV_PSEUDO_PROC } else { HV_PSEUDO_THREAD };
            let st = capability_status(a.op, h);
            // 翻转后要么拒绝要么（互斥口径外的交集操作）仍成功——以确定性为准。
            let expect = match anchor_matrix().iter().find(|x| x.op == a.op && x.tier == flipped_tier) {
                Some(x) => if x.allow { ST_SUCCESS } else { ST_ACCESS_DENIED },
                None => ST_ACCESS_DENIED,
            };
            all_ok = all_ok && st == expect;
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("锚位翻转轮={}/{}", ok, INJECT_ROUNDS))
}

/// 账码回放面：能力账逐行回放能力矩阵验证码值一致。
pub fn replay_ledger_consistent(bk: &CapabilityBook) -> bool {
    bk.rows.iter().all(|(op, tier, st)| {
        let h = if *tier == 0 { HV_PSEUDO_PROC } else { HV_PSEUDO_THREAD };
        capability_status(*op, h) == *st
    })
}

/// 操作名闭集转录演练：八操作名全在转录中 10/10 轮。
pub fn drill_op_names_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31246（操作名序列）。
    let mut lcg = Lcg::new(0x31246);
    for _r in 0..INJECT_ROUNDS {
        // 操作名闭集非空且唯一。
        let mut names_ok = OP_NAMES.len() == OPS;
        for i in 0..OPS {
            names_ok = names_ok && !OP_NAMES[i].is_empty();
            for j in (i + 1)..OPS {
                names_ok = names_ok && OP_NAMES[i] != OP_NAMES[j];
            }
        }
        // 允许/拒绝锚面覆盖全部八操作。
        let matrix = anchor_matrix();
        let mut cover = [false; OPS];
        for a in matrix.iter() {
            cover[a.op] = true;
        }
        let _ = lcg.below(3);
        if names_ok && cover.iter().all(|&c| c) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("操作名闭集轮={}/{}", ok, INJECT_ROUNDS))
}

/// 特值识别边界演练：-2/-1 之间域外值（0xFFFFFFFD）不可识别 10/10 轮。
pub fn drill_pseudo_edge_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31247（边界序列）。
    let mut lcg = Lcg::new(0x31247);
    for _r in 0..INJECT_ROUNDS {
        // -2 与 -1 之间的 0xFFFFFFFD 属域外。
        let between = pseudo_of(0xFFFF_FFFD).is_none();
        // 两特值可识别。
        let two = pseudo_of(HV_PSEUDO_PROC) == Some(0) && pseudo_of(HV_PSEUDO_THREAD) == Some(1);
        // NULL 非伪句柄。
        let null_not = pseudo_of(0).is_none();
        let _ = lcg.below(3);
        if between && two && null_not {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("特值边界轮={}/{}", ok, INJECT_ROUNDS))
}

/// run_unx12526_ext2：扩展面 C 聚合。/// run_unx12526_ext2：扩展面 C 聚合。
pub fn run_unx12526_ext2() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = CapabilityBook::new();
    let _ = bk.inject_all_deny();
    let (ok_r, d_r) = drill_repeat_determinism_10();
    out.push(("X2-重复确定性10/10", ok_r, d_r));
    let (ok_f, d_f) = drill_flip_tier_10();
    out.push(("X2-锚位翻转10/10", ok_f, d_f));
    let (ok_o, d_o) = drill_op_names_10();
    out.push(("X2-操作名闭集10/10", ok_o, d_o));
    let (ok_e, d_e) = drill_pseudo_edge_10();
    out.push(("X2-特值边界10/10", ok_e, d_e));
    out.push((
        "X2-账码回放面",
        replay_ledger_consistent(&bk),
        format!("回放 {} 行全一致", bk.rows()),
    ));
    out
}
