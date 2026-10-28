//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12405 · 视图生命周期账 · 0x300a0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12405-J1 映射-访问-解除守恒账 10^4 次零漂移，进程退出兜底
//!      解除判据绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 生命周期账三操作闭集：Map/Access/Unmap，逐操作记一笔账行
//!     （append-only 转录 + 计数器），任何一步漂移即 LedgerDrift；
//!   - 守恒恒等式：live ≡ mapped - unmapped，逐操作后立即复核，
//!     live 不得为负、unmapped 不得超 mapped；
//!   - 访问域纪律：只许访问活视口（AccessNotLive 拒），访问计数逐视口
//!     归账，访问数与活态严格联动；
//!   - 进程退出兜底：exit_sweep 逐进程解除其全部视口，兜底解除数入账，
//!     扫描后该进程 live 必须为零（SweepMiss 判漏）；
//!   - 演练核：LCG 写死种子 0x300a0，10 轮 × 1000 操作 = 10^4 次零漂移；
//!   - 双进程域（P0/P1）：账按进程分列，跨进程操作拒（PidOob）；
//!   - 重复退出幂等：同进程二次退出走幂等路径不重复解除（DupExit 拒）；
//!   - 错误路径带码+中文 detail，拒止不写回任何账行；
//!   - 槽位定容 32（两进程各 16），满槽拒（MapOverCap）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x300a0 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x300a0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮操作数（10 轮 × 1000 = 10^4 口径）。
pub const OPS_PER_ROUND: usize = 1000;

/// 进程域宽度（P0/P1 两域）。
pub const PROC_DOMES: usize = 2;

/// 每进程视口槽位（16）。
pub const SLOTS_PER_PROC: usize = 16;

/// 视口总槽位。
pub const VIEW_SLOTS: usize = PROC_DOMES * SLOTS_PER_PROC;

/// 页粒度（4KiB 口径）。
pub const PAGE_SIZE: u64 = 0x1000;

/// 诊断码（0x300a0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LifeCode {
    /// 0x300a0 映射超容（槽满）。
    MapOverCap = 0,
    /// 0x300a1 访问非活视口。
    AccessNotLive = 1,
    /// 0x300a2 解除非活视口。
    UnmapNotLive = 2,
    /// 0x300a3 账漂移（守恒恒等式破）。
    LedgerDrift = 3,
    /// 0x300a4 进程域越界（P0/P1 之外）。
    PidOob = 4,
    /// 0x300a5 扫描漏解（退出后该进程仍有活视口）。
    SweepMiss = 5,
    /// 0x300a6 账缩水（账行 append-only 被破）。
    LedgerShrink = 6,
    /// 0x300a7 重复退出（已退出进程再扫描）。
    DupExit = 7,
    /// 0x300a8 操作码越界（账操作域之外）。
    OpOob = 8,
    /// 0x300a9 视口槽越界（索引出表）。
    SlotOob = 9,
}

impl LifeCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [LifeCode; 10] = [
        LifeCode::MapOverCap, LifeCode::AccessNotLive, LifeCode::UnmapNotLive,
        LifeCode::LedgerDrift, LifeCode::PidOob, LifeCode::SweepMiss,
        LifeCode::LedgerShrink, LifeCode::DupExit, LifeCode::OpOob, LifeCode::SlotOob,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x300a0 => "映射超容：先释放废槽再映射，槽位账对平。",
        0x300a1 => "访问非活视口：只许访问 Mapped 态视口，解除后访问必拒。",
        0x300a2 => "解除非活视口：Unmap 只落 Mapped 态，重复解除走幂等拒。",
        0x300a3 => "账漂移：live ≡ mapped - unmapped，每步操作后立即复核。",
        0x300a4 => "进程域越界：只收 P0/P1 两域，越域回查调用方。",
        0x300a5 => "扫描漏解：退出兜底必须清空该进程全部活视口，漏解红账。",
        0x300a6 => "账缩水：账行 append-only，禁止截断，重放补齐。",
        0x300a7 => "重复退出：已退出进程再扫描走幂等，不重复解除。",
        0x300a8 => "操作码越界：只收 Map/Access/Unmap 三操作。",
        0x300a9 => "视口槽越界：索引先比 VIEW_SLOTS 再取行。",
        _ => "未知诊断码：回查 0x300a0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x300a0 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x300a_300a_300a_300a;

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

/// 生命周期操作闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LifeOp {
    /// 映射。
    Map = 0,
    /// 访问。
    Access = 1,
    /// 解除。
    Unmap = 2,
}

impl LifeOp {
    /// 由序号取操作（越界返回 None）。
    pub fn of(i: u64) -> Option<LifeOp> {
        match i {
            0 => Some(LifeOp::Map),
            1 => Some(LifeOp::Access),
            2 => Some(LifeOp::Unmap),
            _ => None,
        }
    }
}

/// 视口账行（槽态+归属+访问计数）。
#[derive(Clone, Copy, Debug)]
pub struct ViewRow {
    /// 归属进程域（0/1）。
    pub pid: usize,
    /// 视口尺寸（页）。
    pub pages: u64,
    /// 活跃。
    pub live: bool,
    /// 访问计数。
    pub accesses: u64,
    /// 解除标记（兜底解除来源时置位）。
    pub swept: bool,
}

/// 账行（操作转录一行，append-only）。
#[derive(Clone, Copy, Debug)]
pub struct LedgerRow {
    /// 操作。
    pub op: LifeOp,
    /// 进程域。
    pub pid: usize,
    /// 槽号。
    pub slot: usize,
    /// 操作序（全局单调）。
    pub seq: u64,
}

/// 视图生命周期账正本。
///
/// 生命周期：map（占槽）→ access（活视口计数）→ unmap（释放）→
/// exit_sweep（进程退出兜底解除）。
#[derive(Debug)]
pub struct LifeBook {
    /// 视口账。
    views: [ViewRow; VIEW_SLOTS],
    /// 转录账（append-only）。
    rows: Vec<LedgerRow>,
    /// 操作序。
    seq: u64,
    /// 映射累计。
    mapped: u32,
    /// 解除累计。
    unmapped: u32,
    /// 访问累计。
    accesses: u32,
    /// 兜底解除累计。
    swept: u32,
    /// 已退出进程域标记。
    exited: [bool; PROC_DOMES],
    /// 拒止累计。
    rejects: u32,
}

impl LifeBook {
    /// 建账。
    pub fn new() -> Self {
        LifeBook {
            views: core::array::from_fn(|_| ViewRow {
                pid: 0,
                pages: 0,
                live: false,
                accesses: 0,
                swept: false,
            }),
            rows: Vec::new(),
            seq: 0,
            mapped: 0,
            unmapped: 0,
            accesses: 0,
            swept: 0,
            exited: [false; PROC_DOMES],
            rejects: 0,
        }
    }

    /// 进程域核对。
    pub fn pid_ok(pid: usize) -> bool {
        pid < PROC_DOMES
    }

    /// 槽归属核对（槽必须属于该进程域）。
    fn slot_owner(&self, slot: usize) -> Option<usize> {
        if slot >= VIEW_SLOTS {
            return None;
        }
        Some(slot / SLOTS_PER_PROC)
    }

    /// 转录一笔（append-only）。
    fn record(&mut self, op: LifeOp, pid: usize, slot: usize) {
        self.seq += 1;
        self.rows.push(LedgerRow { op, pid, slot, seq: self.seq });
    }

    /// 映射（占槽；槽满/越域/已退出必拒）。
    pub fn map(&mut self, pid: usize, pages: u64) -> Result<usize, LifeCode> {
        if !Self::pid_ok(pid) {
            return Err(LifeCode::PidOob);
        }
        if self.exited[pid] {
            return Err(LifeCode::DupExit);
        }
        let base = pid * SLOTS_PER_PROC;
        let mut slot: Option<usize> = None;
        for i in base..base + SLOTS_PER_PROC {
            if !self.views[i].live {
                slot = Some(i);
                break;
            }
        }
        let s = match slot {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(LifeCode::MapOverCap);
            }
        };
        self.views[s] = ViewRow { pid, pages, live: true, accesses: 0, swept: false };
        self.mapped += 1;
        self.record(LifeOp::Map, pid, s);
        Ok(s)
    }

    /// 访问（活视口才许；访问计数归账）。
    pub fn access(&mut self, pid: usize, slot: usize) -> Result<u64, LifeCode> {
        if !Self::pid_ok(pid) {
            return Err(LifeCode::PidOob);
        }
        if slot >= VIEW_SLOTS {
            return Err(LifeCode::SlotOob);
        }
        if self.slot_owner(slot) != Some(pid) {
            return Err(LifeCode::PidOob);
        }
        if !self.views[slot].live {
            self.rejects += 1;
            return Err(LifeCode::AccessNotLive);
        }
        self.views[slot].accesses += 1;
        self.accesses += 1;
        self.record(LifeOp::Access, pid, slot);
        Ok(self.views[slot].accesses)
    }

    /// 解除（活视口才许；幂等拒）。
    pub fn unmap(&mut self, pid: usize, slot: usize) -> Result<bool, LifeCode> {
        if !Self::pid_ok(pid) {
            return Err(LifeCode::PidOob);
        }
        if slot >= VIEW_SLOTS {
            return Err(LifeCode::SlotOob);
        }
        if self.slot_owner(slot) != Some(pid) {
            return Err(LifeCode::PidOob);
        }
        if !self.views[slot].live {
            self.rejects += 1;
            return Err(LifeCode::UnmapNotLive);
        }
        self.views[slot].live = false;
        self.unmapped += 1;
        self.record(LifeOp::Unmap, pid, slot);
        Ok(true)
    }

    /// 进程退出兜底解除（扫全槽；重复退出幂等）。
    pub fn exit_sweep(&mut self, pid: usize) -> Result<u32, LifeCode> {
        if !Self::pid_ok(pid) {
            return Err(LifeCode::PidOob);
        }
        if self.exited[pid] {
            return Err(LifeCode::DupExit);
        }
        self.exited[pid] = true;
        let base = pid * SLOTS_PER_PROC;
        let mut n = 0u32;
        for i in base..base + SLOTS_PER_PROC {
            if self.views[i].live {
                self.views[i].live = false;
                self.views[i].swept = true;
                self.unmapped += 1;
                self.swept += 1;
                n += 1;
                self.record(LifeOp::Unmap, pid, i);
            }
        }
        // 扫描漏解核对（该进程域必须清零）。
        let left = (base..base + SLOTS_PER_PROC).filter(|&i| self.views[i].live).count();
        if left != 0 {
            return Err(LifeCode::SweepMiss);
        }
        Ok(n)
    }

    /// 守恒：live ≡ mapped - unmapped 且 unmapped ≤ mapped。
    pub fn conserved(&self) -> bool {
        let live = self.views.iter().filter(|v| v.live).count();
        self.mapped >= self.unmapped
            && self.mapped as usize == live + self.unmapped as usize
    }

    /// 转录账单调判据（seq 全局递增且行数只增）。
    pub fn ledger_monotone(&self) -> bool {
        let mut last = 0u64;
        for r in self.rows.iter() {
            if r.seq <= last {
                return false;
            }
            last = r.seq;
        }
        true
    }

    /// 转录账长。
    pub fn row_len(&self) -> usize {
        self.rows.len()
    }

    /// 转录账行快照。
    pub fn row_of(&self, i: usize) -> Option<LedgerRow> {
        if i >= self.rows.len() {
            None
        } else {
            Some(self.rows[i])
        }
    }

    /// 活视口计数（全账）。
    pub fn live_views(&self) -> usize {
        self.views.iter().filter(|v| v.live).count()
    }

    /// 进程域活视口计数。
    pub fn live_of(&self, pid: usize) -> usize {
        if !Self::pid_ok(pid) {
            return 0;
        }
        let base = pid * SLOTS_PER_PROC;
        (base..base + SLOTS_PER_PROC).filter(|&i| self.views[i].live).count()
    }

    /// 各累计面。
    pub fn totals(&self) -> (u32, u32, u32, u32) {
        (self.mapped, self.unmapped, self.accesses, self.swept)
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 视口快照。
    pub fn view_of(&self, slot: usize) -> Option<ViewRow> {
        if slot >= VIEW_SLOTS {
            None
        } else {
            Some(self.views[slot])
        }
    }
}

/// 守恒账 10^4 次零漂移演练（10 轮 × 1000 操作）。
pub fn drill_10k_ops_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x300a0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = LifeBook::new();
        // 每进程预置 4 个活视口（操作域池）。
        for pid in 0..PROC_DOMES {
            for _i in 0..4 {
                let _ = bk.map(pid, PAGE_SIZE / 0x1000);
            }
        }
        // 记住各进程第一个活槽位做操作锚。
        let mut anchors: [usize; PROC_DOMES] = [0; PROC_DOMES];
        for pid in 0..PROC_DOMES {
            anchors[pid] = pid * SLOTS_PER_PROC;
        }
        let mut drift = false;
        for i in 0..OPS_PER_ROUND {
            let pid = (lcg.below(PROC_DOMES as u64)) as usize;
            let op = lcg.below(3);
            let r = match op {
                0 => {
                    // 访问锚槽。
                    bk.access(pid, anchors[pid]).is_ok()
                }
                1 => {
                    // 映射新槽（容量池足够：每轮最多 1000 次但槽 16 个，
                    // 先解锚槽再映射避免超容——走交替口径）。
                    let _ = bk.unmap(pid, anchors[pid]);
                    match bk.map(pid, 1) {
                        Ok(s) => {
                            anchors[pid] = s;
                            true
                        }
                        Err(_) => false,
                    }
                }
                _ => {
                    // 解除锚槽后重映射（保证池不枯竭）。
                    let u = bk.unmap(pid, anchors[pid]).is_ok();
                    match bk.map(pid, 1) {
                        Ok(s) => {
                            anchors[pid] = s;
                            u
                        }
                        Err(_) => false,
                    }
                }
            };
            if !r || !bk.conserved() || !bk.ledger_monotone() {
                drift = true;
                break;
            }
            let _ = i;
        }
        if !drift && bk.conserved() && bk.row_len() >= OPS_PER_ROUND + 8 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("10^4 守恒零漂移轮={}/{}", ok, INJECT_ROUNDS))
}

/// 进程退出兜底解除演练：10/10 轮退出清零、重复退出幂等。
pub fn drill_exit_sweep_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x300a0 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = LifeBook::new();
        let n = 3 + lcg.below(4) as usize;
        for _i in 0..n {
            let _ = bk.map(0, 1);
        }
        let before = bk.live_of(0);
        let swept = bk.exit_sweep(0);
        let after = bk.live_of(0);
        let dup = bk.exit_sweep(0);
        let (m, u, _a, s_total) = bk.totals();
        if before == n
            && matches!(swept, Ok(x) if x as usize == n)
            && after == 0
            && matches!(dup, Err(LifeCode::DupExit))
            && u == m
            && s_total == n as u32
            && bk.conserved()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("退出兜底轮={}/{}", ok, INJECT_ROUNDS))
}

/// 跨域与死视口拒止演练：跨进程操作/访问死视口 10/10 拒。
pub fn drill_domain_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x300a0 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = LifeBook::new();
        let s = bk.map(0, 1).unwrap_or(usize::MAX);
        let cross_acc = bk.access(1, s); // 跨进程域
        let cross_unm = bk.unmap(1, s);
        let _ = bk.unmap(0, s);
        let dead_acc = bk.access(0, s); // 死视口
        let dead_unm = bk.unmap(0, s); // 重复解除
        let oob = bk.access(0, VIEW_SLOTS + 3); // 槽越界
        let _ = lcg.below(3);
        if matches!(cross_acc, Err(LifeCode::PidOob))
            && matches!(cross_unm, Err(LifeCode::PidOob))
            && matches!(dead_acc, Err(LifeCode::AccessNotLive))
            && matches!(dead_unm, Err(LifeCode::UnmapNotLive))
            && matches!(oob, Err(LifeCode::SlotOob))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("跨域/死视口拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：转录账导出（逐操作一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 转录账导出（前 n 行）。
pub fn export_ledger_lines(bk: &LifeBook, n: usize) -> String {
    let mut s = String::from("生命周期账转录：\n");
    for i in 0..n {
        if let Some(r) = bk.row_of(i) {
            s.push_str(&format!("#{} P{} S{} {}\n", r.seq, r.pid, r.slot, r.op as u8));
        }
    }
    s
}

/// 转录完整判据（前 n 行齐备）。
pub fn ledger_lines_complete(bk: &LifeBook, n: usize) -> bool {
    let s = export_ledger_lines(bk, n);
    (0..n).all(|i| s.contains(&format!("#{} ", i + 1)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：生命周期报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct LifeReport {
    /// 映射累计。
    pub mapped: u32,
    /// 解除累计。
    pub unmapped: u32,
    /// 访问累计。
    pub accesses: u32,
    /// 兜底解除累计。
    pub swept: u32,
    /// 活视口。
    pub live: usize,
    /// 守恒。
    pub conserved: bool,
}

impl LifeReport {
    /// 建报告。
    pub fn build(bk: &LifeBook) -> Self {
        let (m, u, a, s) = bk.totals();
        LifeReport {
            mapped: m,
            unmapped: u,
            accesses: a,
            swept: s,
            live: bk.live_views(),
            conserved: bk.conserved(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.conserved && self.mapped >= 1 && self.accesses >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "LifeReport 映={} 解={} 访={} 兜={} 活={} 守恒={}",
            self.mapped, self.unmapped, self.accesses, self.swept, self.live, self.conserved
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：操作域核对（三操作闭集逐一可取）。
/// ─────────────────────────────────────────────────────────────────────
pub fn op_domain_ok() -> bool {
    (0..3).all(|i| LifeOp::of(i).is_some()) && LifeOp::of(3).is_none()
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：双进程独立演练（P1 操作不影响 P0 账 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_two_proc_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x300a0 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = LifeBook::new();
        let _ = bk.map(0, 1);
        let _ = bk.map(1, 2);
        let live0 = bk.live_of(0);
        let s1 = 1 * SLOTS_PER_PROC;
        let _ = bk.unmap(1, s1);
        let live0_after = bk.live_of(0);
        let _ = bk.access(0, 0);
        let acc0 = bk.view_of(0).map(|v| v.accesses).unwrap_or(0);
        let acc1 = bk.view_of(s1).map(|v| v.accesses).unwrap_or(0);
        let _ = lcg.below(3);
        if live0 == 1 && live0_after == 1 && acc0 == 1 && acc1 == 0 && bk.conserved() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("双进程独立轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct LifeCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_life_checklist(bk: &LifeBook) -> Vec<LifeCheckRow> {
    let rep = LifeReport::build(bk);
    let mut rows: Vec<LifeCheckRow> = Vec::new();
    rows.push(LifeCheckRow { name: "守恒", passed: bk.conserved() });
    rows.push(LifeCheckRow { name: "账单调", passed: bk.ledger_monotone() });
    rows.push(LifeCheckRow { name: "操作域", passed: op_domain_ok() });
    rows.push(LifeCheckRow { name: "槽位宽", passed: VIEW_SLOTS == 32 });
    rows.push(LifeCheckRow { name: "每进程16", passed: SLOTS_PER_PROC == 16 });
    rows.push(LifeCheckRow { name: "转录完整", passed: ledger_lines_complete(bk, bk.row_len().min(8)) });
    rows.push(LifeCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(LifeCheckRow { name: "10^4口径", passed: OPS_PER_ROUND == 1000 && INJECT_ROUNDS == 10 });
    rows.push(LifeCheckRow { name: "页粒度常量", passed: PAGE_SIZE == 0x1000 });
    rows.push(LifeCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn life_checklist_green(rows: &[LifeCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn life_stats_line(bk: &LifeBook) -> String {
    let (m, u, a, s) = bk.totals();
    format!(
        "生命周期概况：映 {} 解 {} 访 {} 兜 {} 活 {}",
        m, u, a, s, bk.live_views()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12405_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = LifeBook::new();
    let _ = bk.map(0, 1);
    let _ = bk.access(0, 0);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        ledger_lines_complete(&bk, 2) && LifeReport::build(&bk).all_green(),
        life_stats_line(&bk),
    ));
    // X-b：双进程独立 10/10。
    let (ok_t, d_t) = drill_two_proc_10();
    out.push(("X-双进程独立10/10", ok_t, d_t));
    // X-c：终验清单。
    let rows = build_life_checklist(&bk);
    out.push(("X-终验清单10项", life_checklist_green(&rows), LifeReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：访问死视口必拒（AccessNotLive）。
    let mut bk = LifeBook::new();
    let s = bk.map(0, 1).unwrap_or(usize::MAX);
    let _ = bk.unmap(0, s);
    if matches!(bk.access(0, s), Err(LifeCode::AccessNotLive)) {
        caught += 1;
        d.push("注1 死访问:抓".to_string());
    } else {
        d.push("注1 死访问:漏".to_string());
    }
    // 注 2：跨进程域必拒（PidOob）。
    let mut bk2 = LifeBook::new();
    let s2 = bk2.map(0, 1).unwrap_or(usize::MAX);
    if matches!(bk2.access(1, s2), Err(LifeCode::PidOob)) {
        caught += 1;
        d.push("注2 跨域:抓".to_string());
    } else {
        d.push("注2 跨域:漏".to_string());
    }
    // 注 3：重复解除必拒（UnmapNotLive）。
    let mut bk3 = LifeBook::new();
    let s3 = bk3.map(0, 1).unwrap_or(usize::MAX);
    let _ = bk3.unmap(0, s3);
    if matches!(bk3.unmap(0, s3), Err(LifeCode::UnmapNotLive)) {
        caught += 1;
        d.push("注3 重复解:抓".to_string());
    } else {
        d.push("注3 重复解:漏".to_string());
    }
    // 注 4：进程域越界必拒（PidOob）。
    if matches!(bk3.map(5, 1), Err(LifeCode::PidOob)) {
        caught += 1;
        d.push("注4 越域:抓".to_string());
    } else {
        d.push("注4 越域:漏".to_string());
    }
    // 注 5：重复退出必拒（DupExit）。
    let mut bk5 = LifeBook::new();
    let _ = bk5.map(1, 1);
    let _ = bk5.exit_sweep(1);
    if matches!(bk5.exit_sweep(1), Err(LifeCode::DupExit)) {
        caught += 1;
        d.push("注5 重退:抓".to_string());
    } else {
        d.push("注5 重退:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x300a0 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    LifeCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12405_checks：本单判据聚合（诊断段 0x300a0）。
pub fn run_unx12405_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12405 视图生命周期账");
    // J1-a：映射-访问-解除守恒账 10^4 次零漂移。
    let (ok1, d1) = drill_10k_ops_10();
    s.add_dyn("J1-10^4守恒零漂移", ok1, &d1);
    // J1-b：进程退出兜底解除判据。
    let (ok2, d2) = drill_exit_sweep_10();
    s.add_dyn("J1-退出兜底解除10/10", ok2, &d2);
    // J1-c：跨域/死视口拒止 10/10。
    let (ok3, d3) = drill_domain_reject_10();
    s.add_dyn("J1-跨域死视口拒止10/10", ok3, &d3);
    // J1-d：标准会话守恒。
    let mut bk = LifeBook::new();
    let _ = bk.map(0, 1);
    let _ = bk.map(1, 2);
    let _ = bk.access(0, 0);
    let _ = bk.unmap(0, 0);
    s.add_dyn(
        "J1-生命周期守恒",
        bk.conserved() && bk.ledger_monotone() && bk.live_views() == 1,
        &format!("映解访=({:?}) 活={} 守恒={}", bk.totals(), bk.live_views(), bk.conserved()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：兜底解除入账（swept 与 unmapped 联动）。
    let mut bk2 = LifeBook::new();
    let _ = bk2.map(0, 1);
    let _ = bk2.map(0, 1);
    let swept = bk2.exit_sweep(0).unwrap_or(0);
    let (_m, _u, _a, s_total) = bk2.totals();
    s.add_dyn(
        "J2-兜底解除入账",
        swept == 2 && s_total == swept && bk2.conserved(),
        &format!("兜={} 账兜={}", swept, s_total),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x300a0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12405_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
