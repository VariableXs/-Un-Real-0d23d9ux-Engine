//! 任务61（AI-S）· Wine 进程默认能力收敛（无网络/无宿主盘）＋申请式授权。
//!
//! 语义（对齐总案「默认拒绝 + 申请式授权」与任务42 隔离链）：
//! - **默认收敛**：Wine 进程（持有 prefix 的进程）默认能力位 =
//!   GFX|INPUT|AUDIO——渲染/输入/音频是桌面软件的运行必需；**NET 与
//!   HOST_DISK 默认拒绝**（零出站网络 + 零宿主盘暴露的总案红线）。
//! - **申请式授权**：进程 `cap_request`（带定长原因）→ 宿主 UI 审批
//!   （前端 capabilityRequest.ts 同源模型）→ `cap_decide` 允许才置位。
//!   拒绝不置位且计 denied 审计数——"拒绝多少次"本身是安全信号。
//! - **位口径**与 compatapi 能力位同源（GFX=1/FS=2/NET=4/INPUT=8/
//!   AUDIO=16），HOST_DISK 复用 FS 位（prefix 内文件不算宿主盘——
//!   prefix_resolve 的逃逸防护管边界，本模块管"越出 prefix 的宿主盘
//!   访问"这类越域能力）。
//! - 栈纪律：全表 static SpinProtected；原因定长字节缓冲，零堆。

/// 能力位（与 compatapi 同源口径）。
pub const WINE_CAP_GFX: u32 = 1;
pub const WINE_CAP_HOST_DISK: u32 = 2;
pub const WINE_CAP_NET: u32 = 4;
pub const WINE_CAP_INPUT: u32 = 8;
pub const WINE_CAP_AUDIO: u32 = 16;

/// 默认能力：无网络、无宿主盘（任务61 收敛口径）。
pub const WINE_CAP_DEFAULT: u32 = WINE_CAP_GFX | WINE_CAP_INPUT | WINE_CAP_AUDIO;

/// 能力表容量（与 PREFIX_MAX 同刻度：一软件一槽）。
pub const WINECAP_MAX: usize = 16;
/// 申请原因字节上限。
pub const REASON_MAX: usize = 48;
/// 每进程待审申请上限。
pub const PENDING_MAX: usize = 4;

/// 一条待审申请。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PendingReq {
    pub cap: u32,
    pub reason_len: usize,
    pub reason: [u8; REASON_MAX],
}

impl PendingReq {
    const EMPTY: PendingReq = PendingReq { cap: 0, reason_len: 0, reason: [0; REASON_MAX] };

    fn set(&mut self, cap: u32, reason: &[u8]) {
        let n = reason.len().min(REASON_MAX);
        self.reason[..n].copy_from_slice(&reason[..n]);
        self.reason_len = n;
        self.cap = cap;
    }
}

#[derive(Clone, Copy)]
struct WineCapEntry {
    used: bool,
    pid: u32,
    /// 已授能力位（默认收敛位起步）。
    granted: u32,
    pending: [PendingReq; PENDING_MAX],
    pending_n: usize,
    /// 拒绝审计计数（审批拒绝 + 无能力访问被拒均计入）。
    denied: u64,
}

impl WineCapEntry {
    const fn new() -> WineCapEntry {
        WineCapEntry {
            used: false,
            pid: 0,
            granted: 0,
            pending: [PendingReq::EMPTY; PENDING_MAX],
            pending_n: 0,
            denied: 0,
        }
    }
}

static WINECAPS: crate::cpu::sync::SpinProtected<[WineCapEntry; WINECAP_MAX]> =
    crate::cpu::sync::SpinProtected::new([const { WineCapEntry::new() }; WINECAP_MAX]);

/// 申请结果。
#[derive(Debug, PartialEq, Eq)]
pub enum ReqResult {
    Granted,
    Queued,
    AlreadyGranted,
    QueueFull,
    NoEntry,
}

fn slot_of(px: &mut [WineCapEntry; WINECAP_MAX], pid: u32) -> Option<usize> {
    px.iter().position(|e| e.used && e.pid == pid)
}

/// Wine 进程能力登记（prefix_create 挂点）：默认收敛位起步，幂等。
pub fn wine_caps_grant(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let mut px = WINECAPS.lock();
    if let Some(i) = slot_of(&mut px, pid) {
        px[i].granted = WINE_CAP_DEFAULT; // 幂等：重复登记回到默认收敛
        return true;
    }
    let Some(slot) = px.iter_mut().enumerate().find(|(_, e)| !e.used).map(|(i, _)| i) else {
        return false; // 表满
    };
    px[slot] = WineCapEntry {
        used: true,
        pid,
        granted: WINE_CAP_DEFAULT,
        pending: [PendingReq::EMPTY; PENDING_MAX],
        pending_n: 0,
        denied: 0,
    };
    true
}

/// 进程退出（prefix_destroy 挂点）：槽位归还，零泄漏。
pub fn wine_caps_revoke_all(pid: u32) -> bool {
    let mut px = WINECAPS.lock();
    match slot_of(&mut px, pid) {
        Some(i) => {
            px[i] = WineCapEntry::new();
            true
        }
        None => false,
    }
}

/// 查询已授能力位。
pub fn caps_of(pid: u32) -> Option<u32> {
    let px = WINECAPS.lock();
    px.iter().find(|e| e.used && e.pid == pid).map(|e| e.granted)
}

/// 拒绝审计计数。
pub fn denied_of(pid: u32) -> Option<u64> {
    let px = WINECAPS.lock();
    px.iter().find(|e| e.used && e.pid == pid).map(|e| e.denied)
}

/// 能力检查（网络/宿主盘路径挂点）：默认无 NET/HOST_DISK → 恒 false。
pub fn cap_check(pid: u32, cap: u32) -> bool {
    let mut px = WINECAPS.lock();
    let Some(i) = slot_of(&mut px, pid) else {
        return false; // 未登记 = 无任何能力（默认拒绝）
    };
    if px[i].granted & cap != 0 {
        true
    } else {
        px[i].denied += 1; // 无能力访问被拒，计入审计
        false
    }
}

/// 申请式授权：进程侧请求。
pub fn cap_request(pid: u32, cap: u32, reason: &[u8]) -> ReqResult {
    let mut px = WINECAPS.lock();
    let Some(i) = slot_of(&mut px, pid) else {
        return ReqResult::NoEntry;
    };
    if px[i].granted & cap != 0 {
        return ReqResult::AlreadyGranted;
    }
    if px[i].pending[..px[i].pending_n].iter().any(|p| p.cap == cap) {
        return ReqResult::Queued; // 幂等：重复申请不重复入队
    }
    if px[i].pending_n >= PENDING_MAX {
        return ReqResult::QueueFull;
    }
    let n = px[i].pending_n;
    px[i].pending[n].set(cap, reason);
    px[i].pending_n += 1;
    ReqResult::Queued
}

/// 取待审申请（宿主 UI 消费：capRequest.ts 同源展示）。
pub fn cap_pending(pid: u32) -> Option<(u32, usize, [u8; REASON_MAX])> {
    let px = WINECAPS.lock();
    let e = px.iter().find(|e| e.used && e.pid == pid)?;
    let p = e.pending[..e.pending_n].first()?;
    Some((p.cap, p.reason_len, p.reason))
}

/// 审批裁决（宿主 UI → 内核）：允许置位并摘除申请；拒绝保持并计审计。
pub fn cap_decide(pid: u32, cap: u32, allow: bool) -> bool {
    let mut px = WINECAPS.lock();
    let Some(i) = slot_of(&mut px, pid) else {
        return false;
    };
    let had_pending = px[i].pending[..px[i].pending_n].iter().any(|p| p.cap == cap);
    // 摘除该申请（保持队列紧凑）。
    let mut w = 0usize;
    for r in 0..px[i].pending_n {
        if px[i].pending[r].cap != cap {
            px[i].pending[w] = px[i].pending[r];
            w += 1;
        }
    }
    while w < px[i].pending_n {
        px[i].pending[w] = PendingReq::EMPTY;
        w += 1;
    }
    px[i].pending_n = px[i].pending_n.saturating_sub(if had_pending { 1 } else { 0 });
    if allow {
        px[i].granted |= cap;
        true
    } else {
        px[i].denied += 1;
        false
    }
}

// ---------------------------------------------------------------------------
// 实机探针（ring3 演示链挂接）
// ---------------------------------------------------------------------------

/// 实机探针：默认收敛 → 检查拒（审计1）→ 申请 → 审批拒（审计2）→
/// 复查仍拒（审计3）→ 审批允 → 检查过 → 宿主盘恒拒 → 退槽零残留。
pub fn winecaps_probe() {
    let pid = 990u32;
    let _ = wine_caps_grant(pid);
    let d = caps_of(pid).unwrap_or(0);
    let default_ok = d & WINE_CAP_NET == 0 && d & WINE_CAP_HOST_DISK == 0;
    let net_denied = !cap_check(pid, WINE_CAP_NET);
    let req = cap_request(pid, WINE_CAP_NET, b"engine update check");
    let pending_ok = matches!(cap_pending(pid), Some((c, l, _)) if c == WINE_CAP_NET && l == 19);
    let denied_once = !cap_decide(pid, WINE_CAP_NET, false);
    let still_denied = !cap_check(pid, WINE_CAP_NET);
    let audit = denied_of(pid).unwrap_or(0);
    let _ = cap_decide(pid, WINE_CAP_NET, true);
    let net_allowed = cap_check(pid, WINE_CAP_NET);
    let disk_always_denied = !cap_check(pid, WINE_CAP_HOST_DISK);
    let revoked = wine_caps_revoke_all(pid);
    let gone = caps_of(pid).is_none();
    let ok = default_ok
        && net_denied
        && req == ReqResult::Queued
        && pending_ok
        && denied_once
        && still_denied
        && audit == 3
        && net_allowed
        && disk_always_denied
        && revoked
        && gone;
    crate::kinfo!(
        "winecaps-probe: default={:#x} net_denied={} req={:?} deny_then_audit={} allow_then_check={} disk_denied={} revoked={} verdict={}",
        d,
        net_denied,
        req,
        still_denied && audit == 3,
        net_allowed,
        disk_always_denied,
        revoked && gone,
        if ok { "ok" } else { "FAIL" }
    );
}

// ---------------------------------------------------------------------------
// 宿主测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_caps_converged_no_net_no_host_disk() {
        assert!(wine_caps_grant(11));
        let d = caps_of(11).unwrap();
        assert_eq!(d & WINE_CAP_NET, 0, "网络默认拒绝");
        assert_eq!(d & WINE_CAP_HOST_DISK, 0, "宿主盘默认拒绝");
        assert_ne!(d & WINE_CAP_GFX, 0);
        assert_ne!(d & WINE_CAP_INPUT, 0);
        assert_ne!(d & WINE_CAP_AUDIO, 0);
        assert!(cap_check(11, WINE_CAP_GFX));
        assert!(!cap_check(11, WINE_CAP_NET));
    }

    #[test]
    fn unknown_pid_has_nothing() {
        assert_eq!(caps_of(424242), None);
        assert!(!cap_check(424242, WINE_CAP_GFX), "未登记 = 零能力");
        assert_eq!(denied_of(424242), None);
    }

    #[test]
    fn request_allow_grants_and_clears_pending() {
        assert!(wine_caps_grant(12));
        assert_eq!(cap_request(12, WINE_CAP_NET, b"download driver"), ReqResult::Queued);
        let (cap, len, reason) = cap_pending(12).expect("待审申请可见");
        assert_eq!(cap, WINE_CAP_NET);
        assert_eq!(&reason[..len], b"download driver");
        assert!(cap_decide(12, WINE_CAP_NET, true));
        assert_eq!(cap_pending(12), None, "审批后申请摘除");
        assert!(cap_check(12, WINE_CAP_NET));
    }

    #[test]
    fn request_deny_keeps_denied_and_counts() {
        assert!(wine_caps_grant(13));
        let _ = cap_request(13, WINE_CAP_HOST_DISK, b"read C:");
        assert!(!cap_decide(13, WINE_CAP_HOST_DISK, false));
        assert_eq!(cap_pending(13), None);
        assert!(!cap_check(13, WINE_CAP_HOST_DISK));
        // 一次审批拒绝 + 一次无能力访问 = 审计 2。
        assert_eq!(denied_of(13), Some(2));
    }

    #[test]
    fn duplicate_request_is_idempotent() {
        assert!(wine_caps_grant(14));
        assert_eq!(cap_request(14, WINE_CAP_AUDIO, b"r"), ReqResult::AlreadyGranted, "默认已含 AUDIO");
        let _ = cap_request(14, WINE_CAP_NET, b"a");
        assert_eq!(cap_request(14, WINE_CAP_NET, b"a"), ReqResult::Queued, "重复申请不重复入队");
        let px = WINECAPS.lock();
        let e = px.iter().find(|e| e.used && e.pid == 14).unwrap();
        assert_eq!(e.pending_n, 1);
    }

    #[test]
    fn pending_queue_full_is_explicit() {
        assert!(wine_caps_grant(15));
        for cap in [WINE_CAP_NET, WINE_CAP_HOST_DISK, 32, 64] {
            assert_eq!(cap_request(15, cap, b"x"), ReqResult::Queued);
        }
        assert_eq!(cap_request(15, 128, b"y"), ReqResult::QueueFull);
    }

    #[test]
    fn revoke_all_clears_slot() {
        assert!(wine_caps_grant(16));
        assert!(wine_caps_revoke_all(16));
        assert_eq!(caps_of(16), None);
        assert!(!wine_caps_revoke_all(16), "重复退槽如实 false");
    }

    #[test]
    fn grant_is_idempotent_back_to_default() {
        assert!(wine_caps_grant(17));
        let _ = cap_request(17, WINE_CAP_NET, b"r");
        let _ = cap_decide(17, WINE_CAP_NET, true);
        assert!(cap_check(17, WINE_CAP_NET));
        // 重新登记（重启/prefix 重建）→ 回到默认收敛，提升位不跨会话。
        assert!(wine_caps_grant(17));
        assert_eq!(caps_of(17).unwrap() & WINE_CAP_NET, 0);
        assert_eq!(cap_pending(17), None);
    }

    #[test]
    fn reason_truncates_to_cap() {
        assert!(wine_caps_grant(18));
        let long = [b'a'; REASON_MAX + 10];
        assert_eq!(cap_request(18, WINE_CAP_NET, &long), ReqResult::Queued);
        let (_, len, _) = cap_pending(18).unwrap();
        assert_eq!(len, REASON_MAX);
    }
}
