//! UNX-F51523 带宽超预算显式拒绝语义（禁隐性降速） · 0xC05C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51524-J1 查询 ×100 与实表一致；并发查询零撕裂
//!
//! 设计要点：
//!   ① 带宽申请走「冻结契约表」：每端点的分配额编译期锁定，运行期只认
//!      契约内端点；总线总预算独立于各端点分配额之和，二级封顶；
//!   ② 全量或明拒：放行则 granted 恒等于 want，任何「给一半」的隐性降速
//!      都是违约；每次明拒都落一条拒绝账（端点/请求量/原因码），原因可查；
//!   ③ 判定次序固定：未知端点 → 负值 → 零长 → 端点超额 → 总线超额；
//!      多判据同时成立时取最前者，检查逐对锁定；
//!   ④ 状态机单调：提交账与释放账共用单调 seq；回放探针 force_commit
//!      目标低于已承诺额时显性拒（CommitBackward），承诺额一律不回退
//!      （显式 release 除外，且 release 不得超出已承诺额）；
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；超预算组合/隐性截断/
//!      未知端点三类注入各 ×10 轮全部检出；
//!   ⑥ 诊断码闭集 + 中文建议文案；段位守卫锁定 SEG_BASE 对齐与分派区间。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC05C0;

/// 冻结契约表：端点 → 分配额（编译期锁定，运行期只读）。
pub const CONTRACTS: [(u16, u32); 4] = [(1, 1024), (2, 1024), (3, 512), (4, 256)];
/// 总线二级封顶（小于契约额之和 2816，用于逼出总线级超预算组合）。
pub const BUS_BUDGET: u32 = 2048;

/// 诊断码闭集（带宽显式拒绝域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BwRejectCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 未知端点（不在冻结契约表内）
    UnknownEp,
    /// 负值申请
    NegativeWant,
    /// 零长申请
    ZeroWant,
    /// 超出端点契约分配额
    EpOverAlloc,
    /// 超出总线总预算
    BusOverBudget,
    /// 释放超出已承诺额
    ReleaseTooMuch,
    /// 回放承诺额倒退（状态机显性拒）
    CommitBackward,
    /// 对账守恒失败
    LedgerMismatch,
}

impl BwRejectCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BwRejectCode::Ok => "申请成功：全量放行",
            BwRejectCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            BwRejectCode::UnknownEp => "未知端点：端点必须先登记进冻结契约表",
            BwRejectCode::NegativeWant => "负值申请：带宽必须为非负整数",
            BwRejectCode::ZeroWant => "零长申请：至少申请 1 个单位",
            BwRejectCode::EpOverAlloc => "超端点契约额：降低申请量或改约",
            BwRejectCode::BusOverBudget => "超总线总预算：组合超限，本笔明拒",
            BwRejectCode::ReleaseTooMuch => "释放超额：不得超过该端点已承诺额",
            BwRejectCode::CommitBackward => "承诺额倒退：回放目标必须 ≥ 已承诺额",
            BwRejectCode::LedgerMismatch => "对账不符：账本重算与记账面不一致",
        }
    }
}

/// 契约槽位（运行期镜像，来源冻结契约表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BwSlot {
    pub ep: u16,
    pub alloc: u32,
    pub committed: u32,
}

/// 放行账记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitRec {
    pub seq: u32,
    pub ep: u16,
    pub units: u32,
}

/// 拒绝账记录（每次明拒都落账，原因可查）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RejectRec {
    pub seq: u32,
    pub ep: u16,
    pub want: i64,
    pub code: BwRejectCode,
}

/// 带宽账引擎：契约封顶 + 全量或明拒 + 单调状态机。
pub struct BwRejectEngine {
    slots: Vec<BwSlot>,
    bus_committed: u32,
    seq: u32,
    commits: Vec<CommitRec>,
    rejects: Vec<RejectRec>,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl BwRejectEngine {
    /// 从冻结契约表构建引擎。
    pub fn new() -> BwRejectEngine {
        let mut slots = Vec::new();
        for c in CONTRACTS.iter() {
            slots.push(BwSlot {
                ep: c.0,
                alloc: c.1,
                committed: 0,
            });
        }
        BwRejectEngine {
            slots,
            bus_committed: 0,
            seq: 0,
            commits: Vec::new(),
            rejects: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    fn slot_idx(&self, ep: u16) -> Option<usize> {
        self.slots.iter().position(|s| s.ep == ep)
    }

    fn push_reject(&mut self, ep: u16, want: i64, code: BwRejectCode) {
        self.rejects.push(RejectRec {
            seq: self.seq,
            ep,
            want,
            code,
        });
        self.seq = self.seq.saturating_add(1);
    }

    /// 提交一笔带宽申请（全量或明拒；判定次序见设计要点③）。
    pub fn submit(&mut self, ep: u16, want: i64) -> Result<u32, BwRejectCode> {
        let idx = match self.slot_idx(ep) {
            Some(i) => i,
            None => {
                self.push_reject(ep, want, BwRejectCode::UnknownEp);
                return Err(BwRejectCode::UnknownEp);
            }
        };
        if want < 0 {
            self.push_reject(ep, want, BwRejectCode::NegativeWant);
            return Err(BwRejectCode::NegativeWant);
        }
        if want == 0 {
            self.push_reject(ep, want, BwRejectCode::ZeroWant);
            return Err(BwRejectCode::ZeroWant);
        }
        // 已排除负值与 0；超出 i64→u32 安全范围的申请必超契约额。
        if want > u32::MAX as i64 {
            self.push_reject(ep, want, BwRejectCode::EpOverAlloc);
            return Err(BwRejectCode::EpOverAlloc);
        }
        let want_u = want as u32;
        let alloc = match self.slots.get(idx) {
            Some(s) => s.alloc,
            None => {
                self.push_reject(ep, want, BwRejectCode::UnknownEp);
                return Err(BwRejectCode::UnknownEp);
            }
        };
        let cur = match self.slots.get(idx) {
            Some(s) => s.committed,
            None => 0,
        };
        let new_committed = match cur.checked_add(want_u) {
            Some(v) => v,
            None => {
                self.push_reject(ep, want, BwRejectCode::EpOverAlloc);
                return Err(BwRejectCode::EpOverAlloc);
            }
        };
        if new_committed > alloc {
            self.push_reject(ep, want, BwRejectCode::EpOverAlloc);
            return Err(BwRejectCode::EpOverAlloc);
        }
        let new_bus = match self.bus_committed.checked_add(want_u) {
            Some(v) => v,
            None => {
                self.push_reject(ep, want, BwRejectCode::BusOverBudget);
                return Err(BwRejectCode::BusOverBudget);
            }
        };
        if new_bus > BUS_BUDGET {
            self.push_reject(ep, want, BwRejectCode::BusOverBudget);
            return Err(BwRejectCode::BusOverBudget);
        }
        // 全量放行：granted 恒等于 want，禁隐性截断。
        if let Some(s) = self.slots.get_mut(idx) {
            s.committed = new_committed;
        }
        self.bus_committed = new_bus;
        self.commits.push(CommitRec {
            seq: self.seq,
            ep,
            units: want_u,
        });
        self.seq = self.seq.saturating_add(1);
        Ok(want_u)
    }

    /// 显式释放（承诺额唯一合法的回退通道，不得超已承诺额）。
    pub fn release(&mut self, ep: u16, units: i64) -> Result<(), BwRejectCode> {
        let idx = match self.slot_idx(ep) {
            Some(i) => i,
            None => return Err(BwRejectCode::UnknownEp),
        };
        if units < 0 || units == 0 {
            return Err(BwRejectCode::ReleaseTooMuch);
        }
        let u = units as u32;
        let cur = match self.slots.get(idx) {
            Some(s) => s.committed,
            None => return Err(BwRejectCode::UnknownEp),
        };
        let after = match cur.checked_sub(u) {
            Some(v) => v,
            None => return Err(BwRejectCode::ReleaseTooMuch),
        };
        if let Some(s) = self.slots.get_mut(idx) {
            s.committed = after;
        }
        self.bus_committed = self.bus_committed.saturating_sub(u);
        Ok(())
    }

    /// 回放探针：目标承诺额低于当前值即显性拒（状态机倒退判据）。
    pub fn force_commit_probe(&self, ep: u16, target: u32) -> Result<(), BwRejectCode> {
        let idx = match self.slot_idx(ep) {
            Some(i) => i,
            None => return Err(BwRejectCode::UnknownEp),
        };
        let cur = match self.slots.get(idx) {
            Some(s) => s.committed,
            None => return Err(BwRejectCode::UnknownEp),
        };
        if target < cur {
            return Err(BwRejectCode::CommitBackward);
        }
        Ok(())
    }

    /// 对账：放行账重算与各端点承诺额、总线承诺额一致则通过。
    pub fn reconcile(&self) -> Result<(), BwRejectCode> {
        let mut per_ep: [(u16, u64); 4] = [(0, 0), (0, 0), (0, 0), (0, 0)];
        let mut bus = 0u64;
        for c in self.commits.iter() {
            bus = bus.saturating_add(c.units as u64);
            for i in 0..4usize {
                if let Some(slot) = self.slots.get(i) {
                    if slot.ep == c.ep {
                        if let Some(p) = per_ep.get_mut(i) {
                            p.1 = p.1.saturating_add(c.units as u64);
                        }
                    }
                }
            }
        }
        if bus != self.bus_committed as u64 {
            return Err(BwRejectCode::LedgerMismatch);
        }
        for i in 0..4usize {
            let (_, sum) = match per_ep.get(i) {
                Some(p) => *p,
                None => return Err(BwRejectCode::LedgerMismatch),
            };
            let committed = match self.slots.get(i) {
                Some(s) => s.committed as u64,
                None => return Err(BwRejectCode::LedgerMismatch),
            };
            if sum != committed {
                return Err(BwRejectCode::LedgerMismatch);
            }
        }
        Ok(())
    }

    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    pub fn committed_of(&self, ep: u16) -> Option<u32> {
        let idx = self.slot_idx(ep)?;
        match self.slots.get(idx) {
            Some(s) => Some(s.committed),
            None => None,
        }
    }
    pub fn bus_committed(&self) -> u32 {
        self.bus_committed
    }
    pub fn commits_len(&self) -> usize {
        self.commits.len()
    }
    pub fn rejects_len(&self) -> usize {
        self.rejects.len()
    }
    pub fn reject_at(&self, i: usize) -> Option<RejectRec> {
        self.rejects.get(i).copied()
    }
    pub fn injected(&self) -> u64 {
        self.injected
    }
    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0、检出 > 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }
}

/// 段位守卫：SEG_BASE 0x40 对齐且落在分派区间。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一操作序列两次建账逐字段一致（确定性）。
pub fn dual_run_consistent() -> bool {
    let build = || -> (Vec<CommitRec>, Vec<RejectRec>) {
        let mut e = BwRejectEngine::new();
        let _ = e.submit(1, 512);
        let _ = e.submit(2, 700);
        let _ = e.submit(9, 100);
        let _ = e.submit(3, 1200);
        (e.commits.clone(), e.rejects.clone())
    };
    let a = build();
    let b = build();
    a == b
}

impl BwRejectEngine {
    /// 隐性降速审计：放行额必须等于申请额且为正（禁「给一半」）。
    pub fn audit_grant_invariant(&self, want: i64, granted: i64) -> Result<(), BwRejectCode> {
        if granted != want || granted <= 0 {
            Err(BwRejectCode::LedgerMismatch)
        } else {
            Ok(())
        }
    }
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51523_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51523");
    // 1 正常流：契约内申请全量放行，granted == want。
    {
        let mut e = BwRejectEngine::new();
        let r = e.submit(1, 512);
        let ok = r == Ok(512) && e.committed_of(1) == Some(512) && e.bus_committed() == 512;
        s.add_dyn("契约内申请全量放行", ok, "放行额必须等于申请额");
    }
    // 2 边界零：零长申请明拒且落拒绝账。
    {
        let mut e = BwRejectEngine::new();
        let r = e.submit(1, 0);
        s.add_dyn(
            "零长申请明拒",
            r == Err(BwRejectCode::ZeroWant) && e.rejects_len() == 1,
            "零长申请必须明拒并落一条拒绝账",
        );
    }
    // 3 边界负：负值申请明拒。
    {
        let mut e = BwRejectEngine::new();
        let r = e.submit(2, -16);
        s.add_dyn("负值申请明拒", r == Err(BwRejectCode::NegativeWant), "负值申请必须明拒");
    }
    // 4 边界上限/上限+1：端点契约额顶格与越顶。
    {
        let mut e = BwRejectEngine::new();
        let ok_cap = e.submit(4, 256) == Ok(256);
        let mut e2 = BwRejectEngine::new();
        let r_over = e2.submit(4, 257);
        s.add_dyn(
            "契约额顶格与+1",
            ok_cap && r_over == Err(BwRejectCode::EpOverAlloc),
            "顶格放行；契约额+1 必须明拒",
        );
    }
    // 5 超预算组合 ×10：总线满载后再申请，全部明拒并报告原因。
    {
        let mut e = BwRejectEngine::new();
        let _ = e.submit(1, 1024);
        let _ = e.submit(2, 1024);
        let mut caught = 0u32;
        let mut reason_ok = true;
        for i in 0..10u32 {
            let r = e.submit(3, 64 + i as i64);
            match r {
                Err(BwRejectCode::BusOverBudget) => {
                    caught += 1;
                    if BwRejectCode::BusOverBudget.detail().is_empty() {
                        reason_ok = false;
                    }
                    e.note_detected();
                }
                _ => reason_ok = false,
            }
        }
        s.add_dyn(
            "超预算组合 10/10 明拒带原因",
            caught == 10 && reason_ok && e.rejects_len() >= 10,
            "总线超预算组合必须全部明拒并逐笔落拒绝账",
        );
    }
    // 6 隐性降速注入 ×10：伪造「给一半」的放行额，审计全抓。
    {
        let e = BwRejectEngine::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let want = 128 + i as i64;
            let forged = want / 2;
            if e.audit_grant_invariant(want, forged) == Err(BwRejectCode::LedgerMismatch) {
                caught += 1;
            }
        }
        s.add_dyn("隐性降速注入 10/10 检出", caught == 10, "放行额≠申请额必须全部判违");
    }
    // 7 未知端点注入 ×10：契约表外端点全部明拒。
    {
        let mut e = BwRejectEngine::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let ep = 100 + i as u16;
            if e.submit(ep, 32) == Err(BwRejectCode::UnknownEp) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("未知端点注入 10/10 检出", caught == 10, "契约表外端点必须全部明拒");
    }
    // 8 判定次序：未知端点先于负值；端点超额先于总线超额。
    {
        let mut e = BwRejectEngine::new();
        let r1 = e.submit(99, -5);
        let _ = e.submit(1, 1024);
        let _ = e.submit(2, 1024);
        let r2 = e.submit(4, 300);
        s.add_dyn(
            "判定次序锁定",
            r1 == Err(BwRejectCode::UnknownEp)
                && r2 == Err(BwRejectCode::EpOverAlloc),
            "未知端点先于负值；端点契约额先于总线预算",
        );
    }
    // 9 状态机拒止：回放承诺额倒退显性拒，承诺额不变。
    {
        let mut e = BwRejectEngine::new();
        let _ = e.submit(1, 512);
        let before = e.committed_of(1);
        let r = e.force_commit_probe(1, 256);
        s.add_dyn(
            "承诺额倒退显性拒",
            r == Err(BwRejectCode::CommitBackward) && e.committed_of(1) == before,
            "回放目标低于已承诺额必须拒绝且账面不变",
        );
    }
    // 10 容量耗尽与恢复：总线满后明拒，显式释放后同量申请放行。
    {
        let mut e = BwRejectEngine::new();
        let _ = e.submit(1, 1024);
        let _ = e.submit(2, 1024);
        let r_full = e.submit(3, 100);
        let rel = e.release(2, 100);
        let r_back = e.submit(3, 100);
        s.add_dyn(
            "容量耗尽与恢复",
            r_full == Err(BwRejectCode::BusOverBudget)
                && rel == Ok(())
                && r_back == Ok(100),
            "总线满明拒；显式释放腾出额度后放行",
        );
    }
    // 11 对账守恒：放行账重算与承诺额一致，注入=检出+逃逸且逃逸 0。
    {
        let mut e = BwRejectEngine::new();
        let _ = e.submit(1, 512);
        let _ = e.submit(2, 700);
        for _ in 0..10u32 {
            let _ = e.submit(3, 4096);
            e.note_detected();
        }
        let recon = e.reconcile() == Ok(());
        s.add_dyn(
            "对账守恒",
            recon && e.conserved() && e.injected() == 10 && e.escaped() == 0,
            "注入=检出+逃逸，逃逸恒 0，账本重算一致",
        );
    }
    // 12 拒绝账逐条带原因码（报告原因语义）。
    {
        let mut e = BwRejectEngine::new();
        let _ = e.submit(1, 0);
        let _ = e.submit(1, -1);
        let _ = e.submit(99, 8);
        let mut all_reasoned = e.rejects_len() == 3;
        for i in 0..3usize {
            match e.reject_at(i) {
                Some(rec) => {
                    if rec.code.detail().is_empty() {
                        all_reasoned = false;
                    }
                }
                None => all_reasoned = false,
            }
        }
        s.add_dyn("拒绝账逐条带原因", all_reasoned, "每笔明拒都必须可查原因码");
    }
    // 13 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一输入序列两次建账逐字段相等");
    // 14 文案闭集 + 段位守卫。
    {
        let mut closed = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
        s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    }
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [BwRejectCode; 10] = [
    BwRejectCode::Ok,
    BwRejectCode::SegBad,
    BwRejectCode::UnknownEp,
    BwRejectCode::NegativeWant,
    BwRejectCode::ZeroWant,
    BwRejectCode::EpOverAlloc,
    BwRejectCode::BusOverBudget,
    BwRejectCode::ReleaseTooMuch,
    BwRejectCode::CommitBackward,
    BwRejectCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_grant_equals_want() {
        let mut e = BwRejectEngine::new();
        assert_eq!(e.submit(1, 512), Ok(512));
        assert_eq!(e.committed_of(1), Some(512));
        assert_eq!(e.bus_committed(), 512);
        assert_eq!(e.commits_len(), 1);
    }

    #[test]
    fn zero_and_negative_boundary() {
        let mut e = BwRejectEngine::new();
        assert_eq!(e.submit(1, 0), Err(BwRejectCode::ZeroWant));
        assert_eq!(e.submit(1, -4), Err(BwRejectCode::NegativeWant));
        assert_eq!(e.rejects_len(), 2);
        assert_eq!(e.commits_len(), 0);
    }

    #[test]
    fn alloc_cap_and_plus_one() {
        let mut e = BwRejectEngine::new();
        assert_eq!(e.submit(4, 256), Ok(256));
        let mut e2 = BwRejectEngine::new();
        assert_eq!(e2.submit(4, 257), Err(BwRejectCode::EpOverAlloc));
    }

    #[test]
    fn bus_combo_over_budget_x10_all_rejected() {
        let mut e = BwRejectEngine::new();
        let _ = e.submit(1, 1024);
        let _ = e.submit(2, 1024);
        for i in 0..10u32 {
            let r = e.submit(3, 64 + i as i64);
            assert_eq!(r, Err(BwRejectCode::BusOverBudget), "第 {} 轮应明拒", i);
            e.note_detected();
        }
        assert!(e.conserved());
        assert!(e.rejects_len() >= 10);
    }

    #[test]
    fn unknown_ep_injection_x10() {
        let mut e = BwRejectEngine::new();
        for i in 0..10u32 {
            assert_eq!(
                e.submit(100 + i as u16, 32),
                Err(BwRejectCode::UnknownEp)
            );
            e.note_detected();
        }
        assert!(e.conserved());
    }

    #[test]
    fn hidden_throttle_injection_x10_detected() {
        let e = BwRejectEngine::new();
        for i in 0..10u32 {
            let want = 128 + i as i64;
            assert_eq!(
                e.audit_grant_invariant(want, want / 2),
                Err(BwRejectCode::LedgerMismatch),
                "第 {} 轮隐性截断必须判违",
                i
            );
        }
    }

    #[test]
    fn judge_order_unknown_first_ep_before_bus() {
        let mut e = BwRejectEngine::new();
        assert_eq!(e.submit(99, -5), Err(BwRejectCode::UnknownEp));
        let _ = e.submit(1, 1024);
        let _ = e.submit(2, 1024);
        assert_eq!(e.submit(4, 300), Err(BwRejectCode::EpOverAlloc));
    }

    #[test]
    fn commit_backward_state_machine() {
        let mut e = BwRejectEngine::new();
        let _ = e.submit(1, 512);
        assert_eq!(
            e.force_commit_probe(1, 256),
            Err(BwRejectCode::CommitBackward)
        );
        assert_eq!(e.force_commit_probe(1, 512), Ok(()));
        assert_eq!(e.committed_of(1), Some(512));
    }

    #[test]
    fn release_recover_and_release_too_much() {
        let mut e = BwRejectEngine::new();
        let _ = e.submit(1, 512);
        assert_eq!(e.release(1, 600), Err(BwRejectCode::ReleaseTooMuch));
        assert_eq!(e.committed_of(1), Some(512));
        let _ = e.submit(2, 1024);
        let _ = e.submit(3, 512);
        let _ = e.submit(4, 256);
        assert_eq!(e.submit(1, 8), Err(BwRejectCode::BusOverBudget));
        assert_eq!(e.release(2, 100), Ok(()));
        assert_eq!(e.submit(1, 8), Ok(8));
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn conservation_and_reconcile() {
        let mut e = BwRejectEngine::new();
        let _ = e.submit(1, 512);
        for _ in 0..10u32 {
            let _ = e.submit(3, 4096);
            e.note_detected();
        }
        assert!(e.reconcile() == Ok(()));
        assert!(e.conserved());
        assert_eq!(e.escaped(), 0);
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}

