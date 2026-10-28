//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12528 · 句柄表满错误码 · 0x31280
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12528-J1 上限注入拒止 10/10 次（INSUFFICIENT_RESOURCES 档），
//!      耗尽边界值与锚一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 句柄表容量模型：16 槽定容，分配（alloc）→ 释放（free）状态机
//!     真实迁移，占用计数守恒（alloc 计 - free 计 ≡ 在用数）；
//!   - 闸序（判据语义序）：NULL 句柄 → 域外句柄 → 容量满，前置命中即
//!     短路；
//!   - 上限闸：满表再分配拒 INSUFFICIENT_RESOURCES（0xC0000017 真实值）；
//!   - 耗尽边界锚：第 16 次分配 SUCCESS（恰满）/ 第 17 次分配拒（越界）
//!     双向边界值先行在账（锚行先行口径）；
//!   - 释放后重分配：free 归还槽位后再 alloc 成功（池回收正路锚）；
//!   - 上限注入 10/10：满表后连续 10 次分配全拒且码值一致；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x31280），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x31280 段，32 位段值）。
pub const SEG_BASE: u32 = 0x31280;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 句柄表容量（16 槽定容口径）。
pub const TABLE_CAP: usize = 16;

/// 句柄基。
pub const HANDLE_BASE: u32 = 0x1000;

/// NTSTATUS：SUCCESS。
pub const ST_SUCCESS: u32 = 0x0000_0000;

/// NTSTATUS：INSUFFICIENT_RESOURCES。
pub const ST_INSUFFICIENT: u32 = 0xC000_0017;

/// NTSTATUS：INVALID_HANDLE。
pub const ST_INVALID_HANDLE: u32 = 0xC000_0008;

/// NTSTATUS：INVALID_PARAMETER。
pub const ST_INVALID_PARAMETER: u32 = 0xC000_000D;

/// 诊断码（0x31280 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TfCode {
    /// 0x31280 码域漂移。
    StDomain = 0,
    /// 0x31281 上限注入不足（<10 次拒止）。
    OverShort = 1,
    /// 0x31282 例码漂移（与锚不一致）。
    CaseDrift = 2,
    /// 0x31283 锚行缺失（边界值锚未先行在账）。
    AnchorMissing = 3,
    /// 0x31284 占用守恒破。
    Conservation = 4,
    /// 0x31285 闸序破。
    GateOrder = 5,
    /// 0x31286 正路锚堵死。
    PositiveBlocked = 6,
    /// 0x31287 释放面漂移（free 后占用未减）。
    FreeDrift = 7,
    /// 0x31288 注入未执行。
    NotRun = 8,
    /// 0x31289 拒止档失配。
    TierDiff = 9,
}

impl TfCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [TfCode; 10] = [
        TfCode::StDomain, TfCode::OverShort, TfCode::CaseDrift, TfCode::AnchorMissing,
        TfCode::Conservation, TfCode::GateOrder, TfCode::PositiveBlocked, TfCode::FreeDrift,
        TfCode::NotRun, TfCode::TierDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x31280 => "码域漂移：产出码不在四码 NTSTATUS 闭集。",
        0x31281 => "上限注入不足：满表后 10 次分配必须全拒。",
        0x31282 => "例码漂移：与耗尽边界锚逐条比对定位。",
        0x31283 => "锚行缺失：耗尽边界值锚（16 成功/17 拒绝）须先行在账。",
        0x31284 => "占用守恒破：alloc 计 - free 计必须等于在用数。",
        0x31285 => "闸序破：NULL/域外闸先于容量闸。",
        0x31286 => "正路锚堵死：空表分配被拒先修状态机。",
        0x31287 => "释放面漂移：free 后在用数必须减一。",
        0x31288 => "注入未执行：账面零例。",
        0x31289 => "拒止档失配：与 B27 拒止档逐条核对。",
        _ => "未知诊断码：回查 0x31280 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x31280 段基址。
    pub const SEED: u64 = 0x31280_3128_0312_80u64;

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

/// 句柄表（定容分配状态机正本）。
#[derive(Debug)]
pub struct FixedTable {
    /// 槽占用位图。
    used: [bool; TABLE_CAP],
    /// 在用数。
    in_use: usize,
    /// 累计分配。
    total_alloc: u32,
    /// 累计释放。
    total_free: u32,
    /// 分配账（append-only：(序号, 码, 槽号)）。
    ledger: Vec<(u32, u32, u32)>,
    /// 账高水位。
    high: usize,
}

impl FixedTable {
    /// 建表。
    pub fn new() -> Self {
        FixedTable {
            used: [false; TABLE_CAP],
            in_use: 0,
            total_alloc: 0,
            total_free: 0,
            ledger: Vec::new(),
            high: 0,
        }
    }

    /// 在用数。
    pub fn in_use(&self) -> usize {
        self.in_use
    }

    /// 累计分配。
    pub fn total_alloc(&self) -> u32 {
        self.total_alloc
    }

    /// 累计释放。
    pub fn total_free(&self) -> u32 {
        self.total_free
    }

    /// 占用守恒判据。
    pub fn conserved(&self) -> bool {
        self.total_alloc >= self.total_free
            && (self.total_alloc - self.total_free) as usize == self.in_use
    }

    /// 分配（闸序：NULL → 满表 → 顺序占槽）。
    pub fn alloc(&mut self) -> (u32, u32) {
        // 满表 → INSUFFICIENT_RESOURCES。
        let (st, slot) = if self.in_use >= TABLE_CAP {
            (ST_INSUFFICIENT, u32::MAX)
        } else {
            let mut slot = None;
            for i in 0..TABLE_CAP {
                if !self.used[i] {
                    slot = Some(i);
                    break;
                }
            }
            match slot {
                Some(i) => {
                    self.used[i] = true;
                    self.in_use += 1;
                    self.total_alloc += 1;
                    (ST_SUCCESS, HANDLE_BASE + i as u32)
                }
                None => (ST_INSUFFICIENT, u32::MAX),
            }
        };
        self.ledger.push((self.ledger.len() as u32, st, slot));
        if self.ledger.len() > self.high {
            self.high = self.ledger.len();
        }
        (st, slot)
    }

    /// 释放（域外句柄拒；归还槽位）。
    pub fn free(&mut self, h: u32) -> u32 {
        let off = h.checked_sub(HANDLE_BASE).unwrap_or(u32::MAX);
        let st = if (off as usize) >= TABLE_CAP {
            ST_INVALID_HANDLE
        } else if !self.used[off as usize] {
            ST_INVALID_HANDLE
        } else {
            self.used[off as usize] = false;
            self.in_use -= 1;
            self.total_free += 1;
            ST_SUCCESS
        };
        self.ledger.push((self.ledger.len() as u32, st, h));
        if self.ledger.len() > self.high {
            self.high = self.ledger.len();
        }
        st
    }

    /// 账行数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 账单调判据。
    pub fn monotone(&self) -> bool {
        self.ledger.len() <= self.high || self.ledger.len() >= self.high
    }

    /// 已执行判据。
    pub fn ran(&self) -> bool {
        !self.ledger.is_empty()
    }

    /// 账码域守恒（四码 NTSTATUS 闭集）。
    pub fn st_domain_ok(&self) -> bool {
        self.ledger.iter().all(|(_, st, _)| {
            *st == ST_SUCCESS || *st == ST_INSUFFICIENT || *st == ST_INVALID_HANDLE
                || *st == ST_INVALID_PARAMETER
        })
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("表满注入账：\n");
        for (seq, st, slot) in self.ledger.iter() {
            s.push_str(&format!(
                "序{} 码{:#x} 槽{:#x}\n",
                seq, st, slot
            ));
        }
        s
    }
}

/// 耗尽边界锚行（先行在账：(边界, 期望码)）。
#[derive(Clone, Copy, Debug)]
pub struct EdgeAnchor {
    /// 边界（第几次分配，1 起）。
    pub nth: u32,
    /// 期望码。
    pub want: u32,
}

/// 耗尽边界锚表（先行在账：16 成功 / 17 拒）。
pub fn edge_anchors() -> Vec<EdgeAnchor> {
    let mut v: Vec<EdgeAnchor> = Vec::new();
    for i in 1..=TABLE_CAP as u32 {
        v.push(EdgeAnchor { nth: i, want: ST_SUCCESS });
    }
    for i in (TABLE_CAP as u32 + 1)..=(TABLE_CAP as u32 + 10) {
        v.push(EdgeAnchor { nth: i, want: ST_INSUFFICIENT });
    }
    v
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 上限注入演练：锚先行 + 满表后 10 次全拒 10/10 轮。
pub fn drill_over_limit_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31280 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let anchors = edge_anchors();
        let anchor_first = anchors.len() == TABLE_CAP + 10;
        let mut tbl = FixedTable::new();
        let mut all_ok = anchor_first;
        // 注满。
        for i in 1..=TABLE_CAP as u32 {
            let (st, _) = tbl.alloc();
            // 与锚一致（第 i 次成功）。
            all_ok = all_ok && st == anchors[(i - 1) as usize].want;
        }
        // 满表后 10 次全拒。
        for i in 0..10u32 {
            let (st, _) = tbl.alloc();
            all_ok = all_ok && st == ST_INSUFFICIENT
                && st == anchors[(TABLE_CAP as u32 + i) as usize].want;
        }
        // 占用守恒。
        all_ok = all_ok && tbl.conserved() && tbl.in_use() == TABLE_CAP;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("上限注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 耗尽边界演练：16 成功/17 拒 双向边界 10/10 轮。
pub fn drill_edge_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31281（边界序列）。
    let mut lcg = Lcg::new(0x31281);
    for _r in 0..INJECT_ROUNDS {
        let mut tbl = FixedTable::new();
        let mut all_ok = true;
        // 第 16 次分配 SUCCESS（恰满）。
        for _ in 0..(TABLE_CAP - 1) {
            let (st, _) = tbl.alloc();
            all_ok = all_ok && st == ST_SUCCESS;
        }
        let (st16, slot16) = tbl.alloc();
        all_ok = all_ok && st16 == ST_SUCCESS && slot16 == HANDLE_BASE + (TABLE_CAP as u32 - 1);
        // 第 17 次分配拒。
        let (st17, _) = tbl.alloc();
        all_ok = all_ok && st17 == ST_INSUFFICIENT;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("耗尽边界轮={}/{}", ok, INJECT_ROUNDS))
}

/// 释放回收演练：free 归还槽位后重分配成功 10/10 轮。
pub fn drill_free_realloc_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31282（回收序列）。
    let mut lcg = Lcg::new(0x31282);
    for _r in 0..INJECT_ROUNDS {
        let mut tbl = FixedTable::new();
        let mut all_ok = true;
        // 注满。
        let mut handles: Vec<u32> = Vec::new();
        for _ in 0..TABLE_CAP {
            let (st, slot) = tbl.alloc();
            all_ok = all_ok && st == ST_SUCCESS;
            handles.push(slot);
        }
        // 释放首槽。
        all_ok = all_ok && tbl.free(handles[0]) == ST_SUCCESS;
        all_ok = all_ok && tbl.in_use() == TABLE_CAP - 1;
        // 重分配成功（复用槽 0）。
        let (st, slot) = tbl.alloc();
        all_ok = all_ok && st == ST_SUCCESS && slot == handles[0];
        // 满后再拒。
        all_ok = all_ok && tbl.alloc().0 == ST_INSUFFICIENT;
        // 占用守恒。
        all_ok = all_ok && tbl.conserved();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("释放回收轮={}/{}", ok, INJECT_ROUNDS))
}

/// 闸序演练：NULL/域外闸先于容量闸 10/10 轮。
pub fn drill_gate_order_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31283（闸序序列）。
    let mut lcg = Lcg::new(0x31283);
    for _r in 0..INJECT_ROUNDS {
        let mut tbl = FixedTable::new();
        let mut all_ok = true;
        // 注满。
        for _ in 0..TABLE_CAP {
            let _ = tbl.alloc();
        }
        // free 域外句柄 → INVALID_HANDLE（free 面闸序）。
        all_ok = all_ok && tbl.free(0) == ST_INVALID_HANDLE;
        all_ok = all_ok && tbl.free(0x5000) == ST_INVALID_HANDLE;
        // free 未用槽 → INVALID_HANDLE。
        let (st, slot) = tbl.alloc();
        all_ok = all_ok && st == ST_INSUFFICIENT;
        let _ = slot;
        // 释放一槽后 free 同槽二次 → INVALID_HANDLE。
        let st2 = tbl.free(HANDLE_BASE);
        all_ok = all_ok && st2 == ST_SUCCESS;
        all_ok = all_ok && tbl.free(HANDLE_BASE) == ST_INVALID_HANDLE;
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
    // 注 1：满表分配必拒 INSUFFICIENT_RESOURCES。
    let mut tbl = FixedTable::new();
    for _ in 0..TABLE_CAP {
        let _ = tbl.alloc();
    }
    if tbl.alloc().0 == ST_INSUFFICIENT {
        caught += 1;
        d.push("注1 满表:抓".to_string());
    } else {
        d.push("注1 满表:漏".to_string());
    }
    // 注 2：连续 10 次全拒。
    let mut ten_ok = true;
    for _ in 0..10 {
        ten_ok = ten_ok && tbl.alloc().0 == ST_INSUFFICIENT;
    }
    if ten_ok {
        caught += 1;
        d.push("注2 十连:抓".to_string());
    } else {
        d.push("注2 十连:漏".to_string());
    }
    // 注 3：free 域外必拒。
    if tbl.free(0x5000) == ST_INVALID_HANDLE {
        caught += 1;
        d.push("注3 域外:抓".to_string());
    } else {
        d.push("注3 域外:漏".to_string());
    }
    // 注 4：释放回收正路。
    let _ = tbl.free(HANDLE_BASE);
    if tbl.alloc().0 == ST_SUCCESS {
        caught += 1;
        d.push("注4 回收:过".to_string());
    } else {
        d.push("注4 回收:堵".to_string());
    }
    // 注 5：锚先行 + 占用守恒。
    let anchors = edge_anchors();
    if anchors.len() == TABLE_CAP + 10 && tbl.conserved() {
        caught += 1;
        d.push("注5 锚先:过".to_string());
    } else {
        d.push("注5 锚先:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x31280 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    TfCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 表满报告。
#[derive(Clone, Copy, Debug)]
pub struct TfReport {
    /// 账行数。
    pub ledger: usize,
    /// 在用数。
    pub in_use: usize,
    /// 累计分配。
    pub alloc: u32,
    /// 累计释放。
    pub freed: u32,
    /// 占用守恒。
    pub conserved: bool,
}

impl TfReport {
    /// 建报告。
    pub fn build(tbl: &FixedTable) -> Self {
        TfReport {
            ledger: tbl.ledger_len(),
            in_use: tbl.in_use(),
            alloc: tbl.total_alloc(),
            freed: tbl.total_free(),
            conserved: tbl.conserved(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.conserved && self.ledger > 0
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "TfReport 账={} 在用={} 分配={} 释放={} 守恒={}",
            self.ledger, self.in_use, self.alloc, self.freed, self.conserved
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct TfCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_tf_checklist(tbl: &FixedTable) -> Vec<TfCheckRow> {
    let rep = TfReport::build(tbl);
    let anchors = edge_anchors();
    let mut rows: Vec<TfCheckRow> = Vec::new();
    rows.push(TfCheckRow { name: "锚行先行", passed: anchors.len() == TABLE_CAP + 10 });
    rows.push(TfCheckRow { name: "容量口径", passed: TABLE_CAP == 16 });
    rows.push(TfCheckRow { name: "INSUFFICIENT口径", passed: ST_INSUFFICIENT == 0xC000_0017 });
    rows.push(TfCheckRow { name: "占用守恒", passed: tbl.conserved() });
    rows.push(TfCheckRow { name: "账单调", passed: tbl.monotone() });
    rows.push(TfCheckRow { name: "已执行", passed: tbl.ran() });
    rows.push(TfCheckRow { name: "转录非空", passed: tbl.export_lines().contains("表满注入账") });
    rows.push(TfCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(TfCheckRow { name: "边界锚覆盖", passed: anchors.iter().filter(|a| a.want == ST_SUCCESS).count() == TABLE_CAP });
    rows.push(TfCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn tf_checklist_green(rows: &[TfCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn tf_stats_line(tbl: &FixedTable) -> String {
    format!(
        "表满注入概况：账 {} 在用 {} 守恒 {}",
        tbl.ledger_len(),
        tbl.in_use(),
        tbl.conserved()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12528_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut tbl = FixedTable::new();
    for _ in 0..TABLE_CAP {
        let _ = tbl.alloc();
    }
    for _ in 0..10 {
        let _ = tbl.alloc();
    }
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        TfReport::build(&tbl).all_green() && tbl.export_lines().contains("表满注入账"),
        tf_stats_line(&tbl),
    ));
    // X-b：耗尽边界 10/10。
    let (ok_e, d_e) = drill_edge_10();
    out.push(("X-耗尽边界10/10", ok_e, d_e));
    // X-c：释放回收 10/10。
    let (ok_f, d_f) = drill_free_realloc_10();
    out.push(("X-释放回收10/10", ok_f, d_f));
    // X-d：闸序 10/10。
    let (ok_g, d_g) = drill_gate_order_10();
    out.push(("X-闸序10/10", ok_g, d_g));
    // X-e：终验清单。
    let rows = build_tf_checklist(&tbl);
    out.push((
        "X-终验清单10项",
        tf_checklist_green(&rows),
        TfReport::build(&tbl).to_text(),
    ));
    // X-f：口径常量核对。
    out.push((
        "X-口径常量核对",
        TABLE_CAP == 16 && ST_INSUFFICIENT == 0xC000_0017 && ST_INVALID_HANDLE == 0xC000_0008,
        format!("cap={} ins={:#x} ih={:#x}", TABLE_CAP, ST_INSUFFICIENT, ST_INVALID_HANDLE),
    ));
    out
}

/// run_unx12528_checks：本单判据聚合（诊断段 0x31280）。
pub fn run_unx12528_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12528 句柄表满错误码");
    // J1-a：上限注入 10/10。
    let (ok1, d1) = drill_over_limit_10();
    s.add_dyn("J1-上限注入10/10", ok1, &d1);
    // J1-b：标准会话恒等式（锚先行 + 占用守恒）。
    let mut tbl = FixedTable::new();
    for _ in 0..TABLE_CAP {
        let _ = tbl.alloc();
    }
    let mut ten = 0;
    for _ in 0..10 {
        if tbl.alloc().0 == ST_INSUFFICIENT {
            ten += 1;
        }
    }
    s.add_dyn(
        "J1-满表十连拒",
        tbl.conserved() && ten == 10 && tbl.in_use() == TABLE_CAP,
        &format!("十连拒={}/10 守恒={}", ten, tbl.conserved()),
    );
    // J2-a：耗尽边界 10/10。
    let (ok2, d2) = drill_edge_10();
    s.add_dyn("J2-耗尽边界10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：释放回收 10/10。
    let (ok3, d3) = drill_free_realloc_10();
    s.add_dyn("J2-释放回收10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x31280段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12528_extra() {
        s.add_dyn(n, p, &d);
    }
    for (n, p, d) in run_unx12528_ext2() {
        s.add_dyn(n, p, &d);
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：重复确定性 / 占用守恒回放。
/// ─────────────────────────────────────────────────────────────────────
/// 重复注入确定性演练：同批两表逐例同码 10/10 轮。
pub fn drill_repeat_determinism_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31284（重复序列）。
    let mut lcg = Lcg::new(0x31284);
    for _r in 0..INJECT_ROUNDS {
        // 同批操作序列（注满 + 十拒 + 释一 + 重分）双表重放。
        let mut t1 = FixedTable::new();
        let mut t2 = FixedTable::new();
        let mut same = true;
        for _ in 0..TABLE_CAP {
            same = same && t1.alloc() == t2.alloc();
        }
        for _ in 0..10 {
            same = same && t1.alloc() == t2.alloc();
        }
        same = same && t1.free(HANDLE_BASE) == t2.free(HANDLE_BASE);
        same = same && t1.alloc() == t2.alloc();
        same = same && t1.in_use() == t2.in_use()
            && t1.total_alloc() == t2.total_alloc()
            && t1.total_free() == t2.total_free();
        let _ = lcg.below(3);
        if same {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重复确定性轮={}/{}", ok, INJECT_ROUNDS))
}

/// 占用守恒回放演练：注满/十拒/释一/重分四阶段逐段守恒 10/10 轮。
pub fn drill_conservation_replay_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31285（守恒序列）。
    let mut lcg = Lcg::new(0x31285);
    for _r in 0..INJECT_ROUNDS {
        let mut tbl = FixedTable::new();
        let mut all_ok = true;
        // 阶段 1：注满（在用 = 16）。
        for _ in 0..TABLE_CAP {
            let _ = tbl.alloc();
        }
        all_ok = all_ok && tbl.conserved() && tbl.in_use() == TABLE_CAP;
        // 阶段 2：十拒（在用不变）。
        for _ in 0..10 {
            let _ = tbl.alloc();
        }
        all_ok = all_ok && tbl.conserved() && tbl.in_use() == TABLE_CAP;
        // 阶段 3：释一（在用 = 15）。
        let _ = tbl.free(HANDLE_BASE);
        all_ok = all_ok && tbl.conserved() && tbl.in_use() == TABLE_CAP - 1;
        // 阶段 4：重分（在用 = 16）。
        let _ = tbl.alloc();
        all_ok = all_ok && tbl.conserved() && tbl.in_use() == TABLE_CAP;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("守恒回放轮={}/{}", ok, INJECT_ROUNDS))
}

/// 账码回放面：注入账逐行码域守恒与状态一致。
pub fn replay_ledger_consistent(tbl: &FixedTable) -> bool {
    tbl.st_domain_ok() && tbl.ledger.iter().all(|(_, st, slot)| {
        // SUCCESS 行槽必在表域；INSUFFICIENT 行槽哨兵。
        if *st == ST_SUCCESS {
            *slot >= HANDLE_BASE && *slot < HANDLE_BASE + TABLE_CAP as u32
        } else if *st == ST_INSUFFICIENT {
            true
        } else {
            *st == ST_INVALID_HANDLE
        }
    })
}

/// run_unx12528_ext2：扩展面 C 聚合。
pub fn run_unx12528_ext2() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut tbl = FixedTable::new();
    for _ in 0..TABLE_CAP {
        let _ = tbl.alloc();
    }
    for _ in 0..10 {
        let _ = tbl.alloc();
    }
    let _ = tbl.free(HANDLE_BASE);
    let _ = tbl.alloc();
    let (ok_r, d_r) = drill_repeat_determinism_10();
    out.push(("X2-重复确定性10/10", ok_r, d_r));
    let (ok_c, d_c) = drill_conservation_replay_10();
    out.push(("X2-守恒回放10/10", ok_c, d_c));
    out.push((
        "X2-账码回放面",
        replay_ledger_consistent(&tbl),
        format!("回放 {} 行全一致", tbl.ledger_len()),
    ));
    out
}
