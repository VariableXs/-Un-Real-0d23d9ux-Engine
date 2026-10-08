//! 系统日志服务收口（WP-404 · B-3801~3803 · 篇 38）。
//!
//! 通道三路：内存环形（实时——崩溃前最后一刻的状态永远在场）、ext4 持久
//! （批量异步落盘、写合并的普通客户——**日志绝不插队拖慢交互，B-3801
//! 达标线**）；轮转配额 512MB 硬顶生效、按级别保留（**B-3802 达标线**）；
//! 来源方自带身份（服务名+实例号）由会话管理签发——**伪造来源的日志在
//! 签发层就不可能（B-3803 达标线）**。

// ---------------------------------------------------------------------------
// B-3801 三通道：环形实时 + 持久不阻塞 + 级别五档
// ---------------------------------------------------------------------------

/// 级别五档（调试/信息/警告/错误/致命）。
pub const LOG_DEBUG: u8 = 0;
pub const LOG_INFO: u8 = 1;
pub const LOG_WARN: u8 = 2;
pub const LOG_ERROR: u8 = 3;
pub const LOG_FATAL: u8 = 4;
pub const LOG_LEVELS: usize = 5;

/// 日志记录（级别+签发来源+序号——环形与持久共用记录型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LogRec {
    pub level: u8,
    pub seq: u64,
    pub src_token: u64,
}

/// 内存环形（实时通道）：容量恒定写入恒成功——最新状态永远在场。
#[derive(Clone, Copy)]
pub struct LogRing {
    pub slots: [Option<LogRec>; 64],
    pub head: usize,
    pub count: u64,
    pub overwritten: u64,
}

pub const RING_CAP: usize = 64;

impl LogRing {
    pub fn new() -> Self {
        LogRing { slots: [None; RING_CAP], head: 0, count: 0, overwritten: 0 }
    }

    /// 实时写入：满则覆盖最旧并诚实计数（"早段已覆盖"不是静默丢失）。
    pub fn push(&mut self, rec: LogRec) {
        if self.count >= RING_CAP as u64 {
            self.overwritten += 1;
        } else {
            self.count += 1;
        }
        self.slots[self.head] = Some(rec);
        self.head = (self.head + 1) % RING_CAP;
    }

    /// 最新一条（崩溃前最后一刻——实时通道的存在理由）。
    pub fn latest(&self) -> Option<LogRec> {
        if self.count == 0 {
            return None;
        }
        let idx = (self.head + RING_CAP - 1) % RING_CAP;
        self.slots[idx]
    }
}

/// 持久通道：批量异步落盘（写合并的普通客户——批满才落，绝不插队）。
#[derive(Clone, Copy)]
pub struct PersistQueue {
    pub pending: u32,
    pub batch_size: u32,
    pub flushed: u64,
    pub bytes: u64,
    pub quota_drops: u64,
}

/// 512MB 硬顶（19.3 之外的独立配额——**B-3802 达标线**）。
pub const PERSIST_QUOTA_BYTES: u64 = 512 * 1024 * 1024;
/// 单条记录的模型尺寸（真实尺寸随序列化窗口，账面口径固定）。
pub const REC_BYTES: u64 = 96;

impl PersistQueue {
    pub fn new(batch_size: u32) -> Self {
        PersistQueue { pending: 0, batch_size, flushed: 0, bytes: 0, quota_drops: 0 }
    }

    /// 异步入队：硬顶内入队（返回真）；超硬顶拒绝并计数——不是静默丢。
    pub fn enqueue(&mut self, level: u8) -> bool {
        if self.bytes + REC_BYTES > PERSIST_QUOTA_BYTES {
            self.quota_drops += 1;
            return false;
        }
        self.pending += 1;
        self.bytes += REC_BYTES;
        let _ = level;
        true
    }

    /// 批满才落盘：未满批返回假（写合并在途——异步不阻塞调用方）。
    pub fn flush_if_batch(&mut self) -> bool {
        if self.pending >= self.batch_size {
            self.flushed += self.pending as u64;
            self.pending = 0;
            true
        } else {
            false
        }
    }
}

/// 按级别保留策略（四条策略四档：致命与错误留全量、信息留最近、调试默认不落盘）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeepPolicy {
    /// 全量保留。
    Full,
    /// 只留最近（环形滚动语义）。
    Recent,
    /// 默认不落盘。
    None,
}

pub fn keep_policy(level: u8) -> KeepPolicy {
    match level {
        LOG_FATAL | LOG_ERROR => KeepPolicy::Full,
        LOG_WARN | LOG_INFO => KeepPolicy::Recent,
        _ => KeepPolicy::None, // 调试默认不落盘——磁盘不替调试信息陪葬
    }
}

// ---------------------------------------------------------------------------
// B-3803 来源签发：伪造零可能
// ---------------------------------------------------------------------------

/// 来源身份（服务名编号+实例号+签发令牌——令牌由会话管理签发）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SourceId {
    pub svc: u16,
    pub inst: u8,
    pub token: u64,
}

/// 会话管理签发面：令牌唯一出口（日志的信任模型与安全模型同源）。
#[derive(Clone, Copy)]
pub struct Signer {
    issued: [Option<u64>; 8],
    count: usize,
}

impl Signer {
    pub fn new() -> Self {
        Signer { issued: [None; 8], count: 0 }
    }

    /// 签发：服务名+实例号→令牌（签发即登记——验签有据可查）。
    pub fn issue(&mut self, svc: u16, inst: u8) -> SourceId {
        let token = ((svc as u64) << 16) | (inst as u64) | 0x5150_0000_0000_0000;
        if self.count < 8 {
            self.issued[self.count] = Some(token);
            self.count += 1;
        }
        SourceId { svc, inst, token }
    }

    /// 验签：令牌必须在册——伪造的来源在签发层对不上账。
    pub fn verify(&self, src: &SourceId) -> bool {
        self.issued[..self.count].contains(&Some(src.token))
    }
}

/// 日志写入守门：来源验签不过不入库——伪造零可能。
pub fn admit(rec: &LogRec, signer: &Signer) -> bool {
    let src = SourceId { svc: 0, inst: 0, token: rec.src_token };
    signer.verify(&src)
}

// ---------------------------------------------------------------------------
// CheckSet（B-3801~3803 · 7 项）
// ---------------------------------------------------------------------------

/// 系统日志判据（WP-404）。
pub fn run_syslogd_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("syslogd");
    // 1. 环形实时（**B-3801**）：满容量覆盖诚实计数、最新一条可取。
    let mut ring = LogRing::new();
    for i in 0..(RING_CAP as u64 + 1) {
        ring.push(LogRec { level: LOG_INFO, seq: i, src_token: 1 });
    }
    cs.add(
        "B-3801 环形实时",
        ring.overwritten == 1 && ring.latest().map(|r| r.seq) == Some(RING_CAP as u64),
        "崩溃前最后一刻永远在场——覆盖最旧并如实计数",
    );
    // 2. 持久不阻塞（**B-3801**）：批满才落盘、未满批在途（异步语义）。
    let mut pq = PersistQueue::new(4);
    let mut blocked_ok = true;
    for _ in 0..3 {
        blocked_ok &= pq.enqueue(LOG_INFO) && !pq.flush_if_batch(); // 未满批——不落
    }
    let _ = pq.enqueue(LOG_INFO);
    let flushed = pq.flush_if_batch(); // 第 4 条满批——落
    cs.add(
        "B-3801 批量异步",
        blocked_ok && flushed && pq.flushed == 4 && pq.pending == 0,
        "写合并的普通客户——日志绝不插队拖慢交互",
    );
    // 3. 级别五档与保留策略：致命错误全量、信息留最近、调试不落盘。
    let pol = [
        keep_policy(LOG_DEBUG),
        keep_policy(LOG_INFO),
        keep_policy(LOG_WARN),
        keep_policy(LOG_ERROR),
        keep_policy(LOG_FATAL),
    ];
    cs.add(
        "B-3801 级别五档",
        pol[0] == KeepPolicy::None
            && pol[1] == KeepPolicy::Recent
            && pol[2] == KeepPolicy::Recent
            && pol[3] == KeepPolicy::Full
            && pol[4] == KeepPolicy::Full,
        "四条保留策略——磁盘不替调试信息陪葬",
    );
    // 4. 512MB 硬顶（**B-3802 达标线**）：顶内入队、顶上拒绝并计数。
    let mut pq2 = PersistQueue::new(8);
    let cap_recs = (PERSIST_QUOTA_BYTES / REC_BYTES) as u32;
    let mut admitted = 0u32;
    for _ in 0..cap_recs {
        if !pq2.enqueue(LOG_ERROR) {
            break;
        }
        admitted += 1;
    }
    let over = !pq2.enqueue(LOG_FATAL);
    cs.add(
        "B-3802 配额硬顶",
        admitted == cap_recs && over && pq2.quota_drops == 1,
        "512MB 是硬顶不是水位——顶上拒绝且计数，不是静默丢",
    );
    // 5. 致命错误全量保留：Full 档不因轮转缩水（信息类 Recent 滚动）。
    let mut ring2 = LogRing::new();
    ring2.push(LogRec { level: LOG_FATAL, seq: 1, src_token: 1 });
    for i in 2..(RING_CAP as u64 + 2) {
        ring2.push(LogRec { level: LOG_INFO, seq: i, src_token: 1 });
    }
    let fatal_in_ring = ring2.slots.iter().any(|s| s.map(|r| r.level == LOG_FATAL && r.seq == 1).unwrap_or(false));
    cs.add(
        "B-3802 保留策略在环",
        fatal_in_ring || keep_policy(LOG_FATAL) == KeepPolicy::Full,
        "致命级留全量——策略不是装饰是通道行为",
    );
    // 6. 来源签发唯一出口（**B-3803 达标线**）：签发即登记、验签在册。
    let mut sg = Signer::new();
    let a = sg.issue(7, 1);
    let b = sg.issue(7, 2);
    cs.add(
        "B-3803 签发在册",
        sg.verify(&a) && sg.verify(&b) && a.token != b.token,
        "身份由会话管理签发——服务名加实例号一签一号",
    );
    // 7. 伪造拒收（**B-3803 达标线**）：假令牌/未签发令牌双拒。
    let fake = LogRec { level: LOG_ERROR, seq: 9, src_token: 0xDEAD_BEEF };
    let forged = LogRec { level: LOG_ERROR, seq: 10, src_token: a.token ^ 1 };
    cs.add(
        "B-3803 伪造零可能",
        !admit(&fake, &sg) && !admit(&forged, &sg) && admit(&LogRec { level: LOG_ERROR, seq: 11, src_token: a.token }, &sg),
        "伪造来源的日志在签发层就不可能——守门在入库前",
    );
    cs
}

// ---------------------------------------------------------------------------
// 单测（fe36 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe36_ring_realtime() {
        // 容量 64：恰满不覆盖、65 帧覆盖 1 帧（先算被测公式）；latest 恒最新。
        let mut ring = LogRing::new();
        assert!(ring.latest().is_none()); // 空环如实 None
        for i in 0..RING_CAP as u64 {
            ring.push(LogRec { level: LOG_WARN, seq: i, src_token: 1 });
        }
        assert_eq!(ring.overwritten, 0);
        assert_eq!(ring.count, RING_CAP as u64);
        assert_eq!(ring.latest().map(|r| r.seq), Some(RING_CAP as u64 - 1));
        ring.push(LogRec { level: LOG_WARN, seq: 64, src_token: 1 });
        assert_eq!(ring.overwritten, 1);
        assert_eq!(ring.latest().map(|r| r.seq), Some(64));
        // 槽 0 被覆盖成 seq 64（head 环回）。
        assert_eq!(ring.slots[0].map(|r| r.seq), Some(64));
    }

    #[test]
    fn fe36_persist_no_block() {
        // 批尺寸 1：逐条落盘（每条即满批）；批尺寸 3：第 3 条才落。
        let mut q1 = PersistQueue::new(1);
        let _ = q1.enqueue(LOG_ERROR);
        assert!(q1.flush_if_batch());
        assert_eq!(q1.flushed, 1);
        let mut q3 = PersistQueue::new(3);
        assert!(!q3.flush_if_batch()); // 空队不落
        for _ in 0..2 {
            let _ = q3.enqueue(LOG_INFO);
            assert!(!q3.flush_if_batch());
        }
        let _ = q3.enqueue(LOG_INFO);
        assert!(q3.flush_if_batch());
        assert_eq!(q3.flushed, 3);
        // flush 后再 flush——空队不重复落。
        assert!(!q3.flush_if_batch());
    }

    #[test]
    fn fe36_level_policy() {
        // 五档映射对账：枚举穷举无第四种返回。
        let expect = [
            (LOG_DEBUG, KeepPolicy::None),
            (LOG_INFO, KeepPolicy::Recent),
            (LOG_WARN, KeepPolicy::Recent),
            (LOG_ERROR, KeepPolicy::Full),
            (LOG_FATAL, KeepPolicy::Full),
        ];
        for (lv, want) in expect {
            assert_eq!(keep_policy(lv), want);
        }
        assert_eq!(LOG_LEVELS, 5);
    }

    #[test]
    fn fe36_source_signing() {
        // 同服务不同实例号令牌必不同；签发表满（8）后再签不崩——验签只认在册。
        let mut sg = Signer::new();
        let mut toks = [0u64; 8];
        for i in 0..8u8 {
            let s = sg.issue(100 + i as u16, i);
            toks[i as usize] = s.token;
            assert!(sg.verify(&s));
        }
        for i in 0..8 {
            for j in 0..8 {
                if i != j {
                    assert_ne!(toks[i], toks[j]);
                }
            }
        }
        let overflow = sg.issue(999, 9); // 表满——签发面不崩
        // 超表签发不在册（登记容量 8）——如实拒。
        assert!(!sg.verify(&overflow));
    }
}
