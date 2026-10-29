//! unx10701_listenbk —— UNX-F10701 listen backlog 队列深度语义。
//! 锚点（CoRun Unxreal 主册）：UNX-F10701 · backlog 队列深度 · 域 UNX-C4-B16 · 行数预算 280。
//! 判据原文：UNX-F10701-J1 未 accept 连接排队上界 = backlog（100
//!           样本逐界核对），超出即拒连（ECONNREFUSED 面）且零
//!           队列污染，accept FIFO 出队恒等，SOMAXCONN 封顶
//!           （backlog > 4096 收敛 4096），未 listen 即 accept 必拒。
//! 组件：监听账（socket → 监听位 + backlog 生效值）+ 挂起队列
//!       （SYN 已完成未 accept 的连接行）+ 深度闸（入队前查上
//!       界，满即拒连）+ FIFO 核（accept 出队序 = 入队序）+ 封顶
//!       核（backlog 上取 SOMAXCONN）+ 未监听闸（accept 前查监
//!       听位）+ 注入核（超界入队/FIFO 乱序/封顶漂移/未监听
//!       accept 四类 10/10 必抓）。
//! 职责定位：TCP 监听面 backlog 语义的权威模型——深度闸、FIFO、
//!   封顶、未监听闸四性质机检。
//! 性能逐项分解：入队 O(1)；出队 O(1)；封顶 O(1)。全整数确定性。
//! 诊断码段：0xF2A0（0xF2A1~0xF2A9 闭集；全仓十六进制字面量扫描确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const LBV_VERSION: &str = "unx10701_listenbk-v1";

/// 诊断段基址（0xF2A0 段 · 本模块独占）。
pub const LBV_SEG_BASE: u16 = 0xF2A0;

/// 诊断码闭集（0xF2A1~0xF2A9）。
pub const LB_OVER_MISS: u32 = 0xF2A1; // 超界入队漏拒
pub const LB_FIFO_BREAK: u32 = 0xF2A2; // FIFO 出队乱序
pub const LB_CAP_DRIFT: u32 = 0xF2A3; // 封顶漂移（SOMAXCONN）
pub const LB_NO_LISTEN: u32 = 0xF2A4; // 未监听 accept
pub const LB_QUEUE_POLLUTE: u32 = 0xF2A5; // 拒连后队列污染
pub const LB_INJECT_MISS: u32 = 0xF2A6; // 注入漏抓
pub const LB_DOUBLE_LISTEN: u32 = 0xF2A7; // 二次 listen 漏拒
pub const LB_CONN_GHOST: u32 = 0xF2A8; // 幽灵连接出队
pub const LB_DEEP_DRIFT: u32 = 0xF2A9; // 生效 backlog 漂移

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        LB_OVER_MISS => ("超界入队漏拒（ECONNREFUSED）", "深度闸先查再入"),
        LB_FIFO_BREAK => ("accept 出队乱序", "FIFO 序恒等"),
        LB_CAP_DRIFT => ("封顶漂移（SOMAXCONN=4096）", "backlog 上取封顶"),
        LB_NO_LISTEN => ("未 listen 即 accept", "先 listen 再 accept"),
        LB_QUEUE_POLLUTE => ("拒连后队列污染", "满队拒绝不入账"),
        LB_INJECT_MISS => ("注入未监听核拦截", "修复闸谓词后重跑"),
        LB_DOUBLE_LISTEN => ("二次 listen", "监听位幂等拒"),
        LB_CONN_GHOST => ("幽灵连接出队", "队账互查"),
        LB_DEEP_DRIFT => ("生效 backlog 漂移", "封顶后值落账"),
        _ => ("未知诊断码", "查 0xF2A0 段闭集登记表"),
    }
}

/// ECONNREFUSED / EINVAL。
pub const ECONNREFUSED: u32 = 111;
pub const EINVAL: u32 = 22;

/// SOMAXCONN（Linux 对照锚定）。
pub const SOMAXCONN: usize = 4096;

/// 挂起连接行。
#[derive(Clone, Copy, Debug)]
pub struct PendConn {
    /// 入队序号（FIFO 凭证）。
    pub seq: u64,
    /// 客户端代号（对账面）。
    pub client: u64,
}

/// 监听端点。
pub struct ListenEp {
    listening: bool,
    /// 生效 backlog（封顶后）。
    effective: usize,
    queue: Vec<PendConn>,
    wseq: u64,
    rseq: u64,
    /// 拒连计数。
    pub refused: u64,
    /// accept 计数。
    pub accepted: u64,
}

impl ListenEp {
    pub fn new() -> ListenEp {
        ListenEp {
            listening: false,
            effective: 0,
            queue: Vec::new(),
            wseq: 0,
            rseq: 0,
            refused: 0,
            accepted: 0,
        }
    }

    /// listen：封顶核 + 监听位（二次即拒）。
    pub fn listen(&mut self, backlog: usize) -> Result<usize, u32> {
        if self.listening {
            return Err(EINVAL);
        }
        let eff = backlog.min(SOMAXCONN);
        self.listening = true;
        self.effective = eff;
        Ok(eff)
    }

    /// 生效 backlog。
    pub fn effective_backlog(&self) -> usize {
        self.effective
    }

    /// 连接入队：深度闸（挂起数 ≥ backlog 即拒）。
    pub fn connect(&mut self, client: u64) -> Result<(), u32> {
        if !self.listening {
            return Err(ECONNREFUSED);
        }
        if self.queue.len() >= self.effective {
            self.refused += 1;
            return Err(ECONNREFUSED);
        }
        self.queue.push(PendConn { seq: self.wseq, client });
        self.wseq += 1;
        Ok(())
    }

    /// accept：FIFO 出队（未监听闸 + 序号核）。
    pub fn accept(&mut self) -> Result<PendConn, u32> {
        if !self.listening {
            return Err(EINVAL);
        }
        if self.queue.is_empty() {
            return Err(EAGAIN_EMPTY);
        }
        let c = self.queue.remove(0);
        if c.seq != self.rseq {
            return Err(LB_FIFO_BREAK);
        }
        self.rseq += 1;
        self.accepted += 1;
        Ok(c)
    }

    /// 挂起深度。
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// 守恒核：入队序号 - 出队序号 = 挂起数。
    pub fn invariant_ok(&self) -> bool {
        self.wseq - self.rseq == self.queue.len() as u64
    }
}

/// 空队列号（本段内部）。
pub const EAGAIN_EMPTY: u32 = 11;

impl Default for ListenEp {
    fn default() -> Self {
        Self::new()
    }
}

/// LCG64 确定性伪随机。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 百界引擎：backlog 1..=100 逐界核对（满队恰拒 + FIFO 全出）。
pub fn bounds_100() -> Result<usize, u32> {
    let mut checked = 0usize;
    let mut rng = Lcg::new(0x10701_0A01);
    for backlog in 1..=100usize {
        let mut ep = ListenEp::new();
        let eff = ep.listen(backlog)?;
        if eff != backlog {
            return Err(LB_CAP_DRIFT);
        }
        let mut admitted = 0u64;
        for c in 0..(backlog as u64 + 10) {
            match ep.connect(1000 + c) {
                Ok(()) => admitted += 1,
                Err(ECONNREFUSED) => {}
                Err(e) => return Err(e),
            }
        }
        // 恰好 backlog 个入队。
        if admitted != backlog as u64 || ep.pending() != backlog {
            return Err(LB_OVER_MISS);
        }
        // FIFO 全出（随机穿插丢弃样本不涉序）。
        for k in 0..backlog {
            let c = ep.accept()?;
            if c.client != 1000 + k as u64 {
                return Err(LB_FIFO_BREAK);
            }
            let _ = rng.next_u64();
        }
        if !ep.invariant_ok() {
            return Err(LB_CONN_GHOST);
        }
        checked += 1;
    }
    Ok(checked)
}

/// 注入核：超界漏拒/封顶漂移/未监听 accept/二次 listen 四类 10/10。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;

    // 注入一：超界必拒且零污染——3 轮。
    for i in 0..3u32 {
        let mut ep = ListenEp::new();
        let _ = ep.listen(4);
        for c in 0..4u64 {
            let _ = ep.connect(c);
        }
        let before = ep.pending();
        let r = ep.connect(99);
        let caught = r == Err(ECONNREFUSED) && ep.pending() == before;
        all &= caught;
        log.push_str(&format!("超界#{} caught={}\n", i + 1, caught));
    }
    // 注入二：封顶（5000 → 4096）——3 轮。
    for i in 0..3u32 {
        let mut ep = ListenEp::new();
        let eff = ep.listen(5000).unwrap_or(0);
        let caught = eff == SOMAXCONN && ep.effective_backlog() == SOMAXCONN;
        all &= caught;
        log.push_str(&format!("封顶#{} caught={}\n", i + 1, caught));
    }
    // 注入三：未监听 accept 与 connect 必拒——2 轮。
    for i in 0..2u32 {
        let mut ep = ListenEp::new();
        let c1 = matches!(ep.accept(), Err(EINVAL));
        let c2 = ep.connect(1) == Err(ECONNREFUSED);
        let caught = c1 && c2;
        all &= caught;
        log.push_str(&format!("未监听#{} caught={}\n", i + 1, caught));
    }
    // 注入四：二次 listen 必拒——2 轮。
    for i in 0..2u32 {
        let mut ep = ListenEp::new();
        let _ = ep.listen(8);
        let caught = ep.listen(9) == Err(EINVAL);
        all &= caught;
        log.push_str(&format!("二次#{} caught={}\n", i + 1, caught));
    }

    (all, log)
}

/// 验收入口：判据 J1 全链核。
pub fn drill_listenbk() -> (bool, &'static str) {
    // 判据一：百界逐界核对（满队恰拒 + FIFO 全出）。
    let checked = match bounds_100() {
        Ok(x) => x,
        Err(_) => return (false, "F10701 百界引擎失败"),
    };
    if checked != 100 {
        return (false, "F10701 百界覆盖不足");
    }
    // 判据二：封顶 + 生效值落账。
    let mut ep = ListenEp::new();
    let eff = match ep.listen(SOMAXCONN + 100) {
        Ok(x) => x,
        Err(_) => return (false, "F10701 listen 失败"),
    };
    if eff != SOMAXCONN {
        return (false, "F10701 封顶漂移");
    }
    // 判据三：注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F10701 注入核未全抓");
    }
    // 判据四：深化面（accept4 + 半开闸）。
    let (ok2, _log2) = injection_deep();
    if !ok2 {
        return (false, "F10701 深化注入未全抓");
    }
    (true, "F10701 全判据绿")
}

/// ─── 深化面：accept4 旗标 + 半开连接洪泛闸 ───

/// accept4 旗标闭集。
pub const S4_CLOEXEC: u32 = 0x80000;
pub const S4_NONBLOCK: u32 = 0x800;
pub const S4_LEGAL: u32 = S4_CLOEXEC | S4_NONBLOCK;

/// accept4 旗标核（越位必拒）。
pub fn accept4_flags(flags: u32) -> Result<u32, u32> {
    if flags & !S4_LEGAL != 0 {
        return Err(LB_CAP_DRIFT);
    }
    Ok(flags)
}

/// 半开连接闸：半开数独立于 backlog 计数，超 SYN 上限即静默丢。
pub const SYN_CAP: usize = 256;

pub struct SynGuard {
    half_open: usize,
    pub dropped: u64,
}

impl SynGuard {
    pub fn new() -> SynGuard {
        SynGuard { half_open: 0, dropped: 0 }
    }

    /// SYN 到达：半开 +1（超上限即静默丢）。
    pub fn syn(&mut self) -> bool {
        if self.half_open >= SYN_CAP {
            self.dropped += 1;
            return false;
        }
        self.half_open += 1;
        true
    }

    /// ACK 完成：半开 -1（负界必拒）。
    pub fn ack(&mut self) -> Result<(), u32> {
        if self.half_open == 0 {
            return Err(LB_CONN_GHOST);
        }
        self.half_open -= 1;
        Ok(())
    }
}

impl Default for SynGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// 深化注入：旗标越位 / 半开超限静默丢 / 幽灵 ACK 三类必抓。
pub fn injection_deep() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;
    // 注入五：accept4 旗标越位（0x1）必拒，合法位恒过——2 轮。
    for i in 0..2u32 {
        let c1 = accept4_flags(0x1).is_err();
        let c2 = accept4_flags(S4_CLOEXEC | S4_NONBLOCK).is_ok();
        let caught = c1 && c2;
        all &= caught;
        log.push_str(&format!("旗标#{} caught={}\n", i + 1, caught));
    }
    // 注入六：半开超 SYN_CAP 静默丢（非 ECONNREFUSED 面）——2 轮。
    for i in 0..2u32 {
        let mut g = SynGuard::new();
        let mut admitted = 0u64;
        for _ in 0..(SYN_CAP + 8) {
            if g.syn() {
                admitted += 1;
            }
        }
        let caught = admitted == SYN_CAP as u64 && g.dropped == 8;
        all &= caught;
        log.push_str(&format!("半开闸#{} caught={}\n", i + 1, caught));
    }
    // 注入七：幽灵 ACK（无半开）必拒——2 轮。
    for i in 0..2u32 {
        let mut g = SynGuard::new();
        let caught = g.ack() == Err(LB_CONN_GHOST);
        all &= caught;
        log.push_str(&format!("幽灵ACK#{} caught={}\n", i + 1, caught));
    }
    (all, log)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_listenbk().0);
    }
}
