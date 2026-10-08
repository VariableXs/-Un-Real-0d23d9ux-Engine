//! F023 深化批次四 · 错误注入矩阵面（compatstar2/deep3 · G-A-23）。
//!
//! 主层 winsock.rs 覆盖套接字表/getaddrinfo/异步消息/公平调度，批次三深化
//! 覆盖缓冲收发/就绪集/地址族边界；本批补齐主册【功能定义】「全语义对齐」
//! 的容错/账本面：操作×错误注入矩阵（connect/send/recv/close 四操作 ×
//! WSAENETDOWN/WSAETIMEDOUT/WSAECONNRESET/WSAEWOULDBLOCK 四错误 = 16 格
//! 判定函数，如实翻译语义并分类可重试/致命）、重试策略表（每错误码 → 重试
//! 上限/退避基数）、每 socket 统计账（收发字节/错误计数/重试计数，定长
//! 8 socket）、WSAEWOULDBLOCK 忙退避模型（退避步进上限封顶）。
//!
//! 判据对账：主册 G-A-23（FTP 全流程绿/并发 50 socket 无饥饿）+ MS Winsock
//! 文档错误码语义（WSAENETDOWN=10050/WSAETIMEDOUT=10060/WSAECONNRESET=
//! 10054/WSAEWOULDBLOCK=10035 为真实值）。与主层 winsock.rs、deep2/f023e.rs
//! 语义面互补不重叠。零堆纪律：定长数组 + &'static str，错误显性化。

use crate::checks::CheckSet;

/// WSA 错误码真实值（MS Winsock2.h / MSDN Windows Sockets Error Codes）。
pub const WSAENETDOWN: u16 = 10050;
pub const WSAETIMEDOUT: u16 = 10060;
pub const WSAECONNRESET: u16 = 10054;
pub const WSAEWOULDBLOCK: u16 = 10035;

/// 操作编号（矩阵行）。
pub const OP_CONNECT: u8 = 0;
pub const OP_SEND: u8 = 1;
pub const OP_RECV: u8 = 2;
pub const OP_CLOSE: u8 = 3;
/// 错误列序（矩阵列；ERR_CODES 下标）。
pub const ERR_COUNT: usize = 4;
pub const ERR_CODES: [u16; ERR_COUNT] = [WSAENETDOWN, WSAETIMEDOUT, WSAECONNRESET, WSAEWOULDBLOCK];
pub const ERR_NAMES: [&'static str; ERR_COUNT] =
    ["WSAENETDOWN", "WSAETIMEDOUT", "WSAECONNRESET", "WSAEWOULDBLOCK"];

/// 重试分类（矩阵每格二分：可重试/致命）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryClass {
    /// 可重试（重试策略表生效）。
    Retryable,
    /// 致命（连接/子系统已不可用，重试无意义——如实上抛）。
    Fatal,
}

/// 错误列下标；未知错误码显性 Err。
fn err_index(err: u16) -> Result<usize, &'static str> {
    ERR_CODES.iter().position(|&c| c == err).ok_or("unknown-err")
}

/// 16 格判定函数：返回（错误名，重试分类）。语义出处（MSDN 逐格对拍）：
/// - connect：NETDOWN 致命（子系统失效）；TIMEDOUT 可重试（本次尝试超时）；
///   CONNRESET 致命（对端拒绝/重置）；WOULDBLOCK 可重试（非阻塞连接进行中，
///   等可写）。
/// - send：NETDOWN 致命；TIMEDOUT 可重试；CONNRESET 致命（连接已坏，句柄
///   不可再用）；WOULDBLOCK 可重试（发送缓冲满）。
/// - recv：NETDOWN 致命；TIMEDOUT 可重试；CONNRESET 致命（对端重置，句柄
///   不可再用）；WOULDBLOCK 可重试（无数据可读）。
/// - close：NETDOWN 致命；TIMEDOUT 致命（SO_LINGER 超时——句柄已释放，
///   重试 close 必得无效句柄）；CONNRESET 致命（悬挂重置通知，句柄已释放）；
///   WOULDBLOCK 可重试（非阻塞 linger 关闭未完成，可再试）。
pub fn judge(op: u8, err: u16) -> Result<(&'static str, RetryClass), &'static str> {
    let e = err_index(err)?;
    let retryable = match (op, e) {
        (OP_CONNECT, 1) | (OP_CONNECT, 3) => true,
        (OP_CONNECT, _) => false,
        (OP_SEND, 1) | (OP_SEND, 3) => true,
        (OP_SEND, _) => false,
        (OP_RECV, 1) | (OP_RECV, 3) => true,
        (OP_RECV, _) => false,
        (OP_CLOSE, 3) => true,
        (OP_CLOSE, _) => false,
        _ => return Err("unknown-op"),
    };
    Ok((
        ERR_NAMES[e],
        if retryable { RetryClass::Retryable } else { RetryClass::Fatal },
    ))
}

// ---------------------------------------------------------------------------
// 重试策略表（每错误码 → 重试上限/退避基数）
// ---------------------------------------------------------------------------

/// 一条重试策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    /// 重试上限（0 = 不重试——致命类）。
    pub max_retries: u32,
    /// 退避基数（毫秒，忙退避模型按 2 的幂步进）。
    pub backoff_base_ms: u32,
}

/// 策略表（与 ERR_CODES 同序；致命类上限 0——账面即文档）。
pub const RETRY_TABLE: [RetryPolicy; ERR_COUNT] = [
    RetryPolicy { max_retries: 0, backoff_base_ms: 0 }, // NETDOWN：致命
    RetryPolicy { max_retries: 3, backoff_base_ms: 50 }, // TIMEDOUT：重试 3 次
    RetryPolicy { max_retries: 0, backoff_base_ms: 0 }, // CONNRESET：致命
    RetryPolicy { max_retries: 8, backoff_base_ms: 1 }, // WOULDBLOCK：忙退避 8 步
];

pub fn policy_for(err: u16) -> Result<RetryPolicy, &'static str> {
    let e = err_index(err)?;
    Ok(RETRY_TABLE[e])
}

/// WOULDBLOCK 忙退避模型：基数 1ms 按 2 的幂步进，64ms 封顶
/// （第 7 步起恒 64——上限封顶，防活锁自旋）。
pub const BACKOFF_CAP_MS: u32 = 64;

pub fn backoff_ms(attempt: u32) -> u32 {
    let shift = attempt.min(6);
    let v = 1u32.checked_shl(shift).unwrap_or(BACKOFF_CAP_MS);
    v.min(BACKOFF_CAP_MS)
}

// ---------------------------------------------------------------------------
// 每 socket 统计账（定长 8 socket 三账）
// ---------------------------------------------------------------------------

/// 账面 socket 容量。
pub const MAX_LEDGER_SOCKETS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketStat {
    pub sent_bytes: u64,
    pub recv_bytes: u64,
    pub err_count: u32,
    pub retry_count: u32,
}

/// 每 socket 三账：收发字节/错误计数/重试计数（句柄越界显性 Err——零静默）。
pub struct SocketLedgers {
    stats: [SocketStat; MAX_LEDGER_SOCKETS],
}

impl SocketLedgers {
    pub const fn new() -> Self {
        SocketLedgers {
            stats: [SocketStat { sent_bytes: 0, recv_bytes: 0, err_count: 0, retry_count: 0 };
                MAX_LEDGER_SOCKETS],
        }
    }

    fn slot(&mut self, handle: u32) -> Result<&mut SocketStat, &'static str> {
        if handle as usize >= MAX_LEDGER_SOCKETS {
            return Err("socket-out-of-range");
        }
        Ok(&mut self.stats[handle as usize])
    }

    pub fn note_send(&mut self, handle: u32, bytes: u64) -> Result<(), &'static str> {
        self.slot(handle)?.sent_bytes += bytes;
        Ok(())
    }

    pub fn note_recv(&mut self, handle: u32, bytes: u64) -> Result<(), &'static str> {
        self.slot(handle)?.recv_bytes += bytes;
        Ok(())
    }

    pub fn note_err(&mut self, handle: u32) -> Result<(), &'static str> {
        self.slot(handle)?.err_count += 1;
        Ok(())
    }

    pub fn note_retry(&mut self, handle: u32) -> Result<(), &'static str> {
        self.slot(handle)?.retry_count += 1;
        Ok(())
    }

    pub fn stat(&self, handle: u32) -> Result<SocketStat, &'static str> {
        if handle as usize >= MAX_LEDGER_SOCKETS {
            return Err("socket-out-of-range");
        }
        Ok(self.stats[handle as usize])
    }
}

/// 域自检（深化批次四）。
pub fn run_f023f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F023-errinject-d4");
    // 1) WSA 错误码真实值（MSDN 对拍）。
    cs.add("wsa_codes_real", ERR_CODES == [10050, 10060, 10054, 10035], "");
    // 2) connect 行：WOULDBLOCK 可重试（非阻塞连接进行中）、NETDOWN 致命。
    let (n1, c1) = judge(OP_CONNECT, WSAEWOULDBLOCK).expect("合法格");
    let (_, c2) = judge(OP_CONNECT, WSAENETDOWN).expect("合法格");
    cs.add("matrix_connect_cells", n1 == "WSAEWOULDBLOCK"
        && c1 == RetryClass::Retryable && c2 == RetryClass::Fatal, "");
    // 3) send/recv 行：CONNRESET 致命、TIMEDOUT 可重试。
    let (_, c3) = judge(OP_SEND, WSAECONNRESET).expect("合法格");
    let (_, c4) = judge(OP_RECV, WSAECONNRESET).expect("合法格");
    let (_, c5) = judge(OP_RECV, WSAETIMEDOUT).expect("合法格");
    cs.add("matrix_send_recv_cells", c3 == RetryClass::Fatal
        && c4 == RetryClass::Fatal && c5 == RetryClass::Retryable, "");
    // 4) close 行：仅 WOULDBLOCK 可重试（linger 未完），其余致命。
    let (_, c6) = judge(OP_CLOSE, WSAEWOULDBLOCK).expect("合法格");
    let (_, c7) = judge(OP_CLOSE, WSAETIMEDOUT).expect("合法格");
    let (_, c8) = judge(OP_CLOSE, WSAECONNRESET).expect("合法格");
    cs.add("matrix_close_cells", c6 == RetryClass::Retryable
        && c7 == RetryClass::Fatal && c8 == RetryClass::Fatal, "");
    // 5) 未知错误码/操作显性 Err（零静默）。
    cs.add("matrix_unknown_err", judge(OP_SEND, 10048) == Err("unknown-err")
        && judge(9, WSAENETDOWN) == Err("unknown-op"), "");
    // 6) 策略表：致命类 0 上限，TIMEDOUT 3×50ms，WOULDBLOCK 8×1ms。
    let p = policy_for(WSAETIMEDOUT).expect("合法码");
    let q = policy_for(WSAEWOULDBLOCK).expect("合法码");
    let r = policy_for(WSAENETDOWN).expect("合法码");
    cs.add("retry_policy_table", p == RETRY_TABLE[1] && q.max_retries == 8
        && q.backoff_base_ms == 1 && r.max_retries == 0, "");
    // 7) 忙退避序列 1,2,4,8,16,32,64,64（第 7 步封顶）。
    let seq = [backoff_ms(0), backoff_ms(1), backoff_ms(2), backoff_ms(3),
        backoff_ms(4), backoff_ms(5), backoff_ms(6), backoff_ms(7)];
    cs.add("backoff_capped", seq == [1, 2, 4, 8, 16, 32, 64, 64]
        && BACKOFF_CAP_MS == 64, "");
    // 8) 每 socket 三账：收发字节/错误/重试记账全数可读回。
    let mut led = SocketLedgers::new();
    let _ = led.note_send(0, 1024);
    let _ = led.note_recv(0, 2048);
    let _ = led.note_err(0);
    let _ = led.note_retry(0);
    let _ = led.note_retry(0);
    let s = led.stat(0).expect("合法句柄");
    cs.add("ledger_record_readback", s.sent_bytes == 1024 && s.recv_bytes == 2048
        && s.err_count == 1 && s.retry_count == 2, "");
    // 9) 句柄越界显性 Err（账面 8 socket 上限）。
    cs.add("ledger_out_of_range", led.note_send(8, 1) == Err("socket-out-of-range")
        && led.stat(8) == Err("socket-out-of-range"), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_16_cells_swept() {
        // 全 16 格扫掠：可重试 7 格（connect/send/recv 各 2 + close 1）、
        // 致命 9 格——账面与文档一致，无静默格。
        let mut retryable = 0;
        let mut fatal = 0;
        for op in [OP_CONNECT, OP_SEND, OP_RECV, OP_CLOSE] {
            for &err in ERR_CODES.iter() {
                match judge(op, err) {
                    Ok((_, RetryClass::Retryable)) => retryable += 1,
                    Ok((_, RetryClass::Fatal)) => fatal += 1,
                    Err(e) => panic!("格 ({op}, {err}) 不应显性 Err: {e}"),
                }
            }
        }
        assert_eq!(retryable, 7);
        assert_eq!(fatal, 9);
    }

    #[test]
    fn backoff_large_attempt_stays_capped() {
        assert_eq!(backoff_ms(20), BACKOFF_CAP_MS);
        assert_eq!(backoff_ms(u32::MAX), BACKOFF_CAP_MS);
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f023f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
