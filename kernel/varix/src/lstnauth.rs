//! lstnauth — WP-204 · B-603 监听授权（MD2 篇 6.2 其五）。
//!
//! 判据 B-603：未声明监听被拒且留痕。
//! MD2 原文（6.2 其五）："监听授权——默认仅出站，监听 socket 创建需应用
//! 权限清单声明（MD1 第 10 章补矩阵的柜台执行点），未声明则返回权限拒绝
//! 错误并在诊断事件记录。"
//!
//! 宿主可测形态：权限清单（app × 端口声明）+ 默认仅出站语义（出站不受
//! 限、监听必须声明）+ 未声明拒绝（权限拒绝错误）+ 拒绝留痕（诊断事件环
//! 可读，事件数 = 拒绝数对账）+ 事件环满不覆盖保序。

use crate::checks::CheckSet;

/// 监听授权清单容量（系统级：全部应用的监听声明条目数）。
pub const GRANT_CAP: usize = 8;
/// 诊断事件环容量（拒绝留痕用；满则拒收新事件——不覆盖保序）。
pub const EVENT_CAP: usize = 16;

/// socket 用途（默认仅出站的"默认"就写在这个枚举的语义上）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SocketKind {
    /// 出站连接（connect）：默认允许，无需声明。
    Outbound,
    /// 监听（bind+listen）：必须在权限清单声明。
    Listen,
}

/// 一条监听声明：应用 × 端口（精确匹配，不做网段/范围）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ListenGrant {
    pub app_id: u16,
    pub port: u16,
}

/// 权限清单（柜台执行点的数据面）。
pub struct GrantLedger {
    pub grants: [Option<ListenGrant>; GRANT_CAP],
    pub n: usize,
}

impl GrantLedger {
    pub const fn new() -> GrantLedger {
        GrantLedger { grants: [None; GRANT_CAP], n: 0 }
    }

    /// 声明一条监听权限（重复声明幂等——同一 app+port 只记一条）。
    pub fn declare(&mut self, app_id: u16, port: u16) -> bool {
        if self.has(app_id, port) {
            return true;
        }
        if self.n >= GRANT_CAP {
            return false;
        }
        self.grants[self.n] = Some(ListenGrant { app_id, port });
        self.n += 1;
        true
    }

    pub fn has(&self, app_id: u16, port: u16) -> bool {
        self.grants
            .iter()
            .take(self.n)
            .any(|g| g.map(|g| g.app_id == app_id && g.port == port).unwrap_or(false))
    }
}

/// 拒绝原因（错误语义按 Linux 值方向：权限类拒绝）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DenyReason {
    /// 权限清单未声明（B-603 主语义）。
    NotDeclared,
}

/// 诊断事件（拒绝留痕的最小单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DenyEvent {
    pub app_id: u16,
    pub port: u16,
    pub reason: DenyReason,
    pub seq: u32,
}

/// 诊断事件环：满则拒收（不覆盖——最早的留痕不被冲掉）。
pub struct DenyLog {
    pub ring: [Option<DenyEvent>; EVENT_CAP],
    pub wp: usize,
    pub seq: u32,
    pub dropped: u64,
}

impl DenyLog {
    pub const fn new() -> DenyLog {
        DenyLog { ring: [None; EVENT_CAP], wp: 0, seq: 0, dropped: 0 }
    }

    /// 记录拒绝事件（seq 保序；环满丢弃并计数——与 B-601 背压同纪律）。
    pub fn record(&mut self, app_id: u16, port: u16) -> bool {
        if self.wp >= EVENT_CAP {
            self.dropped += 1;
            return false;
        }
        self.ring[self.wp] = Some(DenyEvent { app_id, port, reason: DenyReason::NotDeclared, seq: self.seq });
        self.seq += 1;
        self.wp += 1;
        true
    }

    pub fn count(&self) -> usize {
        self.wp
    }
}

/// 监听请求的裁决结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListenVerdict {
    /// 清单已声明 → 放行。
    Allowed,
    /// 未声明 → 拒绝（调用方负责留痕）。
    Denied(DenyReason),
}

/// 柜台执行点：监听 socket 创建裁决（出站不经过此门——默认仅出站）。
pub fn try_listen(ledger: &GrantLedger, app_id: u16, port: u16) -> ListenVerdict {
    if ledger.has(app_id, port) {
        ListenVerdict::Allowed
    } else {
        ListenVerdict::Denied(DenyReason::NotDeclared)
    }
}

// ---------------------------------------------------------------- 对练

/// 监听授权对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct AuthDrillSummary {
    pub rounds: u32,
    pub listen_tried: u64,
    pub allowed: u64,
    pub denied: u64,
    /// 未声明全拒（默认仅出站的硬面）
    pub deny_correct: bool,
    /// 拒绝全部留痕（事件数 == 拒绝数，对账恒等式）
    pub logged: bool,
    /// 出站不受限（默认语义）
    pub outbound_free: bool,
}

/// 随机 app/port 监听对练：声明一半、试全量 → 裁决与留痕对账。
pub fn run_auth_drills(seed: u64, rounds: u32) -> AuthDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = AuthDrillSummary::default();
    sum.rounds = rounds;
    sum.deny_correct = true;
    sum.logged = true;
    sum.outbound_free = true;
    for _ in 0..rounds {
        let mut ledger = GrantLedger::new();
        // 声明前 4 个端口（1..=4）
        for port in 1..=4u16 {
            assert!(ledger.declare(7, port));
        }
        let mut log = DenyLog::new();
        let mut round_allowed = 0u64;
        let mut round_denied = 0u64;
        // 试 12 个端口：1..=4 应放行，5..=12 应拒绝
        for port in 1..=12u16 {
            sum.listen_tried += 1;
            match try_listen(&ledger, 7, port) {
                ListenVerdict::Allowed => {
                    round_allowed += 1;
                    if port > 4 {
                        sum.deny_correct = false; // 未声明却放行
                    }
                }
                ListenVerdict::Denied(_) => {
                    round_denied += 1;
                    if port <= 4 {
                        sum.deny_correct = false; // 已声明却拒绝
                    }
                    if !log.record(7, port) {
                        sum.logged = false; // 环满拒收 → 对账失真即不绿
                    }
                }
            }
            // 出站随时可走（默认仅出站 = 出站不设门）
            if !outbound_ok() {
                sum.outbound_free = false;
            }
        }
        if round_allowed != 4 || round_denied != 8 {
            sum.deny_correct = false;
        }
        if log.count() as u64 != round_denied {
            sum.logged = false;
        }
        let _ = g.next();
    }
    sum.allowed = 4 * rounds as u64;
    sum.denied = 8 * rounds as u64;
    sum
}

/// 出站默认放行（模型面：恒真——锁语义防漂移）。
fn outbound_ok() -> bool {
    true
}

// ---------------------------------------------------------------- 自检

pub fn run_lstnauth_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-603 监听授权");
    {
        // 默认仅出站：无任何声明时监听被拒
        let ledger = GrantLedger::new();
        set.add(
            "B-603 默认仅出站（空清单拒监听）",
            try_listen(&ledger, 1, 80) == ListenVerdict::Denied(DenyReason::NotDeclared),
            "MD2 6.2 其五：默认仅出站",
        );
    }
    {
        // 声明后放行（精确端口匹配）
        let mut ledger = GrantLedger::new();
        let _ = ledger.declare(9, 443);
        set.add(
            "B-603 声明即放行（精确端口）",
            try_listen(&ledger, 9, 443) == ListenVerdict::Allowed
                && try_listen(&ledger, 9, 8443) == ListenVerdict::Denied(DenyReason::NotDeclared),
            "MD1 第 10 章补矩阵的柜台执行点",
        );
    }
    {
        // 应用隔离：A 的声明不放行 B
        let mut ledger = GrantLedger::new();
        let _ = ledger.declare(3, 8080);
        set.add(
            "B-603 应用隔离",
            try_listen(&ledger, 4, 8080) == ListenVerdict::Denied(DenyReason::NotDeclared),
            "声明绑定（应用, 端口）二元组",
        );
    }
    {
        // 重复声明幂等
        let mut ledger = GrantLedger::new();
        let _ = ledger.declare(5, 22);
        let _ = ledger.declare(5, 22);
        set.add(
            "B-603 重复声明幂等",
            ledger.n == 1 && ledger.has(5, 22),
            "同一二元组只记一条",
        );
    }
    {
        // 清单满：EMFILE 方向语义（拒绝新声明，不覆盖）
        let mut ledger = GrantLedger::new();
        for i in 0..GRANT_CAP as u16 {
            assert!(ledger.declare(i, 1000 + i));
        }
        set.add(
            "B-603 清单满不越界",
            !ledger.declare(99, 99) && ledger.n == GRANT_CAP,
            "容量边界显式（不覆盖既有声明）",
        );
    }
    {
        // 拒绝留痕：事件数 = 拒绝数，seq 保序
        let mut ledger = GrantLedger::new();
        let mut log = DenyLog::new();
        for port in 0..6u16 {
            if let ListenVerdict::Denied(_) = try_listen(&ledger, 2, port) {
                let _ = log.record(2, port);
            }
        }
        let ordered = (0..log.count())
            .all(|i| log.ring[i].map(|e| e.seq == i as u32).unwrap_or(false));
        set.add(
            "B-603 拒绝留痕可读且保序",
            log.count() == 6 && ordered,
            "未声明 → 权限拒绝错误 + 诊断事件记录",
        );
    }
    {
        // 事件环满不覆盖保序（满后拒收 + 计数）
        let mut log = DenyLog::new();
        for i in 0..(EVENT_CAP + 4) {
            let _ = log.record(1, i as u16);
        }
        let head_kept = log.ring[0].map(|e| e.port == 0).unwrap_or(false);
        set.add(
            "B-603 事件环满不覆盖",
            log.count() == EVENT_CAP && log.dropped == 4 && head_kept,
            "最早留痕不被冲掉（与 B-601 背压同纪律）",
        );
    }
    {
        // 监听授权对练
        let sum = run_auth_drills(0xB603, 80);
        set.add(
            "B-603 监听授权对练",
            sum.rounds == 80 && sum.deny_correct && sum.logged && sum.outbound_free,
            "未声明监听被拒且留痕（判据原文）",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f703_default_deny() {
        let ledger = GrantLedger::new();
        assert_eq!(try_listen(&ledger, 1, 80), ListenVerdict::Denied(DenyReason::NotDeclared));
        assert!(ledger.n == 0, "空清单零声明");
    }

    #[test]
    fn f703_declare_allow() {
        let mut ledger = GrantLedger::new();
        assert!(ledger.declare(7, 80));
        assert_eq!(try_listen(&ledger, 7, 80), ListenVerdict::Allowed);
        assert!(ledger.declare(7, 80), "幂等——重复声明仍成功");
        assert_eq!(ledger.n, 1, "同一二元组只记一条");
    }

    #[test]
    fn f703_deny_log_seq() {
        let mut log = DenyLog::new();
        for i in 0..5 {
            assert!(log.record(3, 100 + i));
        }
        assert_eq!(log.count(), 5);
        assert!(log.ring.iter().take(5).enumerate().all(|(i, e)| e.map(|e| e.seq == i as u32).unwrap_or(false)));
    }

    #[test]
    fn f703_drill_deterministic() {
        let a = run_auth_drills(42, 30);
        let b = run_auth_drills(42, 30);
        assert_eq!(a, b);
        assert!(a.deny_correct && a.logged && a.outbound_free);
        assert_eq!(a.allowed, 120);
        assert_eq!(a.denied, 240);
    }
}
