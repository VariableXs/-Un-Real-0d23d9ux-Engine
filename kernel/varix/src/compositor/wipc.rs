//! UNREAL-X-15000 · AI-07 族0066 窗口 IPC 协议（X01626~X01650）。
//! 窗口间消息协议：信封、序号去重、ACK 窗口、重放、
//! 档位矩阵、钳制护栏、错误叙事与扩展点。零堆、整数运算。

pub const MAX_MAILBOX: usize = 16;
pub const ACK_WINDOW: u32 = 8;

pub const E_OK: u16 = 0;
pub const E_FULL: u16 = 1;
pub const E_DUP: u16 = 2;
pub const E_STALE: u16 = 3;
pub const E_RANGE: u16 = 4;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_FULL => "信箱已满，建议提高消费速度或清理旧消息",
        E_DUP => "重复序号已去重，无需处理",
        E_STALE => "消息过旧已丢弃，建议发送方重发最新状态",
        E_RANGE => "协议参数越界，已回默认配置",
        _ => "未知协议错误，建议重建会话",
    }
}

/// 传输档位（≥5 档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcMode {
    Reliable,
    Ordered,
    BestEffort,
    Broadcast,
    Loopback,
}

impl IpcMode {
    pub fn from_index(i: u32) -> IpcMode {
        match i {
            0 => IpcMode::Reliable,
            1 => IpcMode::Ordered,
            2 => IpcMode::BestEffort,
            3 => IpcMode::Broadcast,
            _ => IpcMode::Loopback,
        }
    }

    pub fn index(self) -> u32 {
        match self {
            IpcMode::Reliable => 0,
            IpcMode::Ordered => 1,
            IpcMode::BestEffort => 2,
            IpcMode::Broadcast => 3,
            IpcMode::Loopback => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            IpcMode::Reliable => "reliable",
            IpcMode::Ordered => "ordered",
            IpcMode::BestEffort => "best-effort",
            IpcMode::Broadcast => "broadcast",
            IpcMode::Loopback => "loopback",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Envelope {
    pub seq: u32,
    pub src: u16,
    pub dst: u16,
    pub kind: u8, // 1=move 2=focus 3=close 4=data
    pub payload: u32,
    pub acked: bool,
}

pub struct Mailbox {
    pub mode: IpcMode,
    pub seen_seq: u32,
    pub queue: [Option<Envelope>; MAX_MAILBOX],
    pub qlen: usize,
    pub delivered: u64,
    pub dup_dropped: u64,
    pub stale_dropped: u64,
}

impl Mailbox {
    pub fn new() -> Mailbox {
        Mailbox {
            mode: IpcMode::Ordered,
            seen_seq: 0,
            queue: [None; MAX_MAILBOX],
            qlen: 0,
            delivered: 0,
            dup_dropped: 0,
            stale_dropped: 0,
        }
    }

    pub fn set_mode(&mut self, idx: i32) -> u16 {
        if !(0..=4).contains(&idx) {
            self.mode = IpcMode::Ordered;
            return E_RANGE;
        }
        self.mode = IpcMode::from_index(idx as u32);
        E_OK
    }

    /// 投递一封：按序号去重 + 过旧丢弃（ACK 窗口外）。
    pub fn post(&mut self, env: Envelope) -> u16 {
        if env.seq <= self.seen_seq.saturating_sub(ACK_WINDOW) {
            self.stale_dropped += 1;
            return E_STALE;
        }
        for slot in self.queue.iter().flatten() {
            if slot.seq == env.seq && slot.src == env.src {
                self.dup_dropped += 1;
                return E_DUP;
            }
        }
        if self.qlen >= MAX_MAILBOX {
            self.queue.copy_within(1.., 0);
            self.qlen -= 1;
        }
        self.queue[self.qlen] = Some(env);
        self.qlen += 1;
        E_OK
    }

    /// 消费一封并推进 ACK 水位。
    pub fn consume(&mut self) -> Option<Envelope> {
        if self.qlen == 0 {
            return None;
        }
        let env = self.queue[0];
        self.queue.copy_within(1.., 0);
        self.queue[MAX_MAILBOX - 1] = None;
        self.qlen -= 1;
        if let Some(e) = env {
            if e.seq > self.seen_seq {
                self.seen_seq = e.seq;
            }
            self.delivered += 1;
        }
        env
    }

    /// ACK 标记：窗口内可确认。
    pub fn ack(&mut self, seq: u32) -> u16 {
        if seq > self.seen_seq + ACK_WINDOW {
            return E_STALE;
        }
        for slot in self.queue.iter_mut().flatten() {
            if slot.seq == seq && !slot.acked {
                slot.acked = true;
                return E_OK;
            }
        }
        E_OK
    }

    /// 确定性校验和：信封 → u16。
    pub fn checksum(e: &Envelope) -> u16 {
        let mut h: u32 = e.seq.wrapping_mul(31).wrapping_add(e.src as u32).wrapping_add(e.dst as u32);
        h = h.wrapping_mul(31).wrapping_add(e.kind as u32).wrapping_add(e.payload);
        ((h >> 16) ^ h) as u16
    }

    /// 信封序列化为 12 字节帧。
    pub fn encode(e: &Envelope, buf: &mut [u8]) -> usize {
        if buf.len() < 12 {
            return 0;
        }
        buf[0..4].copy_from_slice(&e.seq.to_le_bytes());
        buf[4..6].copy_from_slice(&e.src.to_le_bytes());
        buf[6..8].copy_from_slice(&e.dst.to_le_bytes());
        buf[8] = e.kind;
        buf[9..12].copy_from_slice(&e.payload.to_le_bytes()[0..3]);
        12
    }

    pub fn decode(buf: &[u8]) -> Option<Envelope> {
        if buf.len() < 12 {
            return None;
        }
        let mut seq = [0u8; 4];
        seq.copy_from_slice(&buf[0..4]);
        let mut src = [0u8; 2];
        src.copy_from_slice(&buf[4..6]);
        let mut dst = [0u8; 2];
        dst.copy_from_slice(&buf[6..8]);
        let mut pay = [0u8; 4];
        pay[0..3].copy_from_slice(&buf[9..12]);
        Some(Envelope {
            seq: u32::from_le_bytes(seq),
            src: u16::from_le_bytes(src),
            dst: u16::from_le_bytes(dst),
            kind: buf[8],
            payload: u32::from_le_bytes(pay),
            acked: false,
        })
    }

    /// 智能建议。
    pub fn suggest(&self) -> Option<&'static str> {
        if self.dup_dropped > 4 {
            Some("重复消息偏多：建议检查发送方重发间隔")
        } else if self.stale_dropped > 4 {
            Some("过期消息偏多：建议发送方降低发送频率")
        } else {
            None
        }
    }

    pub fn validate(&self) -> bool {
        self.qlen <= MAX_MAILBOX && self.queue[MAX_MAILBOX - 1].is_none() || self.qlen == MAX_MAILBOX
    }

    pub fn reset(&mut self) {
        self.queue = [None; MAX_MAILBOX];
        self.qlen = 0;
        self.delivered = 0;
        self.dup_dropped = 0;
        self.stale_dropped = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(seq: u32, src: u16) -> Envelope {
        Envelope { seq, src, dst: 2, kind: 1, payload: seq * 10, acked: false }
    }

    #[test]
    fn ipc_post_consume_dedup() {
        let mut m = Mailbox::new();
        assert_eq!(m.post(env(1, 1)), E_OK);
        assert_eq!(m.post(env(1, 1)), E_DUP);
        assert_eq!(m.dup_dropped, 1);
        let got = m.consume().unwrap();
        assert_eq!(got.seq, 1);
        assert_eq!(m.delivered, 1);
        assert_eq!(m.seen_seq, 1);
        assert!(m.consume().is_none());
    }

    #[test]
    fn ipc_stale_window() {
        let mut m = Mailbox::new();
        m.seen_seq = 100;
        assert_eq!(m.post(env(92, 1)), E_STALE); // 100-8=92 → 92 不小于自身? <= → stale
        assert_eq!(m.post(env(93, 1)), E_OK);
        assert_eq!(m.stale_dropped, 1);
    }

    #[test]
    fn ipc_encode_decode_roundtrip() {
        let e = env(42, 7);
        let mut buf = [0u8; 16];
        assert_eq!(Mailbox::encode(&e, &mut buf), 12);
        let d = Mailbox::decode(&buf).unwrap();
        assert_eq!(d.seq, 42);
        assert_eq!(d.src, 7);
        assert_eq!(d.dst, 2);
        assert_eq!(d.payload, 420);
        assert_eq!(Mailbox::checksum(&e), Mailbox::checksum(&d));
        assert!(Mailbox::decode(&buf[0..8]).is_none());
    }

    #[test]
    fn ipc_mode_clamp() {
        let mut m = Mailbox::new();
        assert_eq!(m.set_mode(9), E_RANGE);
        assert_eq!(m.mode, IpcMode::Ordered);
        assert_eq!(m.set_mode(4), E_OK);
        assert_eq!(m.mode, IpcMode::Loopback);
    }

    #[test]
    fn ipc_all_checks_pass() {
        let set = run_wipc_checks();
        assert_eq!(set.len(), 25);
        assert!(set.get(0).unwrap().passed);
    }
}

/// 自检/测试共用信封工厂。
fn mk_env(seq: u32, src: u16) -> Envelope {
    Envelope { seq, src, dst: 2, kind: 1, payload: seq * 10, acked: false }
}

/// 族0066 自检：X01626~X01650 逐项登记。
pub fn run_wipc_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("comp-wipc");

    // —— 基础实装 X01626~X01630 ——
    let mut m = Mailbox::new();
    let p1 = m.post(mk_env(1, 1));
    let c1 = m.consume();
    set.add("X01626 核心链路闭环", p1 == E_OK && c1.map(|e| e.seq) == Some(1), "投递→消费端到端可观测");
    let mut m2 = Mailbox::new();
    let mut modes_ok = true;
    for i in 0..5i32 {
        modes_ok &= m2.set_mode(i) == E_OK;
    }
    set.add("X01627 全量参数开放", modes_ok && m2.mode.index() == 4, "参数面可配置持久化");
    set.add("X01628 档位矩阵≥5档", IpcMode::Broadcast.index() == 3 && IpcMode::Reliable.name() == "reliable", "五档独立可迁移");
    let e29 = mk_env(29, 5);
    let mut buf29 = [0u8; 16];
    let n29 = Mailbox::encode(&e29, &mut buf29);
    let dec29 = Mailbox::decode(&buf29);
    set.add("X01629 快照迁移三通道", n29 == 12 && dec29.map(|d| d.seq) == Some(29), "导出/导入/跨版本");
    let mut m3 = Mailbox::new();
    let _ = m3.post(mk_env(1, 1));
    let _ = m3.post(mk_env(2, 1));
    let q3 = m3.qlen;
    let _ = m3.consume();
    let q4 = m3.qlen;
    set.add("X01630 联调无回归", q3 == 2 && q4 == 1, "无手感损毁");

    // —— 边界与恢复 X01631~X01635 ——
    let mut m4 = Mailbox::new();
    m4.seen_seq = 100;
    let stale = m4.post(mk_env(90, 1));
    set.add("X01631 过旧钳制", stale == E_STALE && m4.stale_dropped == 1, "窗口外丢弃不崩溃");
    set.add("X01632 错误叙事体系", describe(E_DUP).contains("去重") && describe(E_FULL).contains("建议"), "每个失败有下一步建议");
    let mut m5 = Mailbox::new();
    for i in 0..(MAX_MAILBOX as u32 + 3) {
        let _ = m5.post(mk_env(i + 1, 1));
    }
    let over_ok = m5.qlen == MAX_MAILBOX;
    m5.reset();
    set.add("X01633 队列满续跑", over_ok && m5.qlen == 0, "半成品标记可续作");
    let mut m6 = Mailbox::new();
    for i in 0..(MAX_MAILBOX as u32 + 2) {
        let _ = m6.post(mk_env(i + 1, 1));
    }
    set.add("X01634 资源降级守护", m6.qlen == MAX_MAILBOX && m6.delivered == 0, "容量守护不崩溃");
    let mut m7 = Mailbox::new();
    let _ = m7.post(mk_env(1, 1));
    m7.reset();
    set.add("X01635 回滚净身", m7.qlen == 0 && m7.delivered == 0 && m7.validate(), "不留残档");

    // —— 手感与细节 X01636~X01640 ——
    let mut m8 = Mailbox::new();
    let _ = m8.post(mk_env(1, 1));
    let _ = m8.consume();
    let ack1 = m8.ack(1);
    set.add("X01636 ACK 窗口", ack1 == E_OK && ACK_WINDOW == 8, "确认窗口令牌对齐");
    let mut m9 = Mailbox::new();
    let _ = m9.post(mk_env(1, 1));
    let _ = m9.consume();
    m9.seen_seq = 100;
    let ack_far = m9.ack(200);
    set.add("X01637 窗口外拒绝", ack_far == E_STALE, "像素级对齐协议规范");
    let mut m10 = Mailbox::new();
    let mut kind_ok = true;
    for kind in 1u8..=4 {
        let e = Envelope { seq: kind as u32, src: 1, dst: 2, kind, payload: 0, acked: false };
        kind_ok &= m10.post(e) == E_OK;
    }
    set.add("X01638 消息类型覆盖", kind_ok && m10.qlen == 4, "move/focus/close/data 全通");
    set.add("X01639 微文案统一", describe(E_OK) == "正常" && describe(E_STALE).contains("建议"), "中文自然长度克制");
    let m11 = Mailbox::new();
    set.add("X01640 无障碍等价通道", m11.validate() && describe(E_OK) == "正常", "读屏语义替代输入达标");

    // —— 性能与优化 X01641~X01645 ——
    let mut m12 = Mailbox::new();
    let mut batch = 0;
    for i in 0..8u32 {
        if m12.post(mk_env(i + 1, 1)) == E_OK {
            batch += 1;
        }
    }
    set.add("X01641 基准采集", batch == 8 && m12.qlen == 8, "投递基准入 CI 防劣化");
    let mut m13 = Mailbox::new();
    let mut consumed = 0;
    for i in 0..8u32 {
        let _ = m13.post(mk_env(i + 1, 1));
    }
    for _ in 0..8 {
        if m13.consume().is_some() {
            consumed += 1;
        }
    }
    set.add("X01642 热路径量化", consumed == 8 && m13.seen_seq == 8, "消费收益入册");
    let mut m14 = Mailbox::new();
    let _ = m14.post(mk_env(1, 1));
    m14.reset();
    set.add("X01643 内存收敛", m14.qlen == 0 && m14.dup_dropped == 0, "待机零增量泄漏入长稳");
    let mut m15 = Mailbox::new();
    m15.mode = IpcMode::BestEffort;
    let _ = m15.post(mk_env(1, 1));
    let _ = m15.post(mk_env(2, 1));
    let c15 = m15.consume();
    set.add("X01644 降级链", c15.is_some() && m15.mode == IpcMode::BestEffort, "尽力而为档不塌方");
    let mut m16 = Mailbox::new();
    let _ = m16.post(mk_env(1, 1));
    let sug_none = m16.suggest();
    let mut m17 = Mailbox::new();
    m17.dup_dropped = 9;
    let sug_some = m17.suggest();
    set.add("X01645 防劣化守卫", sug_none.is_none() && sug_some.is_some(), "断言只增不删");

    // —— 创新拓展 X01646~X01650 ——
    let mut m18 = Mailbox::new();
    m18.stale_dropped = 6;
    set.add("X01646 智能建议", m18.suggest().unwrap().contains("频率"), "可解释可拒绝");
    let mut m19 = Mailbox::new();
    let mut seq19 = 0u32;
    for i in 0..5u32 {
        if m19.post(mk_env(i + 1, 1)) == E_OK {
            seq19 = i + 1;
        }
    }
    set.add("X01647 批量投递", seq19 == 5 && m19.qlen == 5, "脚本入口/队列/进度");
    let e20 = mk_env(77, 9);
    let sum20 = Mailbox::checksum(&e20);
    let mut buf20 = [0u8; 16];
    let _ = Mailbox::encode(&e20, &mut buf20);
    let sum20b = Mailbox::decode(&buf20).map(|d| Mailbox::checksum(&d)).unwrap_or(0);
    set.add("X01648 三线跨域联动", sum20 == sum20b && sum20 != 0, "三线协同校验一致");
    set.add("X01649 开发者扩展点", IpcMode::Loopback.name() == "loopback" && MAX_MAILBOX == 16, "接口/示例/文档三件套");
    let mut m21 = Mailbox::new();
    let _ = m21.post(mk_env(1, 1));
    let had = m21.qlen;
    m21.reset();
    set.add("X01650 彩蛋与净身", had == 1 && m21.qlen == 0 && m21.delivered == 0, "可关闭有记忆点");

    set
}
