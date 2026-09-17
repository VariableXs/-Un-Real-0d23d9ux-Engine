//! 任务33 · 消息通道原语——订阅/广播/单发（双域总案阶段4·步骤5）。
//!
//! 语义与验收对齐：
//! - **三语义**：`subscribe/unsubscribe`（按进程订阅通道）、
//!   `broadcast`（一次投递到全部订阅者）、`send`（单发给单个订阅者）。
//! - **通道容量 + 慢消费者策略**：每订阅者独立定容队列
//!   （[CHAN_CAP]），满后**丢最旧 + 计数**（dropped/missed 双计数，
//!   与 inputsvc 同策略——复用思路不重写其队列，本模块自持定容环）。
//! - **同通道 FIFO**：广播顺序 = 投递顺序，×1000 用例断言。
//! - **通道名注册表**：名字→通道号注册（开放性验收点），重名拒绝。
//! - 与 Windows 侧 emit/on 语义对照表见
//!   `docs/双域-任务33-消息通道语义对照-2026-09-17.md`（完善性验收）。
//!
//! 授权联动：订阅须过 vfswhitelist（任务37 热更新规则接管后可动态收敛），
//! 本层保留 `allowed` 位由调用方（垫片/任务26）按白名单裁决结果置位。

use alloc::vec::Vec;

/// 通道数上限（与内核 16 通道等待队列同量级——复用思路的定容口径）。
pub const CHAN_MAX: usize = 16;
/// 通道名上限。
pub const CHAN_NAME_MAX: usize = 32;
/// 每订阅者队列容量。
pub const CHAN_CAP: usize = 64;
/// 单通道订阅者上限。
pub const SUBS_MAX: usize = 8;
/// 消息载荷上限。
pub const MSG_MAX: usize = 56;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChanError {
    /// 通道/订阅者表满。
    Full,
    /// 通道名未注册。
    UnknownChannel,
    /// 通道名已注册。
    DuplicateName,
    /// 未订阅即收发。
    NotSubscribed,
    /// 载荷超限。
    PayloadTooBig,
    /// 白名单未放行。
    NotAllowed,
}

/// 一条消息（FIFO 环元素）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Msg {
    pub seq: u64,
    pub from: u32,
    pub len: u8,
    pub payload: [u8; MSG_MAX],
}

#[derive(Clone, Copy)]
struct Sub {
    pid: u32,
    ring: [Option<Msg>; CHAN_CAP],
    head: usize,
    len: usize,
    /// 慢消费者：满后丢最旧计数。
    dropped: u64,
}

impl Sub {
    fn new(pid: u32) -> Self {
        Sub { pid, ring: [None; CHAN_CAP], head: 0, len: 0, dropped: 0 }
    }
    fn push(&mut self, m: Msg) {
        if self.len == CHAN_CAP {
            // 慢消费者：丢最旧 + 计数（绝不阻塞发布者）。
            self.ring[self.head] = Some(m);
            self.head = (self.head + 1) % CHAN_CAP;
            self.dropped += 1;
        } else {
            self.ring[(self.head + self.len) % CHAN_CAP] = Some(m);
            self.len += 1;
        }
    }
    fn pop(&mut self) -> Option<Msg> {
        if self.len == 0 {
            return None;
        }
        let m = self.ring[self.head].take();
        self.head = (self.head + 1) % CHAN_CAP;
        self.len -= 1;
        m
    }
}

#[derive(Clone, Copy)]
struct Chan {
    used: bool,
    name: [u8; CHAN_NAME_MAX],
    name_len: u8,
    subs: [Option<Sub>; SUBS_MAX],
}

impl Chan {
    fn new() -> Self {
        Chan { used: false, name: [0; CHAN_NAME_MAX], name_len: 0, subs: [None; SUBS_MAX] }
    }
}

/// 消息总线（定容零分配）。
pub struct MsgBus {
    chans: [Chan; CHAN_MAX],
    /// 全总线单调消息序（FIFO 断言锚点）。
    next_seq: u64,
    /// 总投递条数（度量）。
    pub delivered: u64,
}

impl MsgBus {
    pub fn new() -> Self {
        MsgBus { chans: [Chan::new(); CHAN_MAX], next_seq: 1, delivered: 0 }
    }

    /// 注册通道名（开放性：注册表）。重名拒绝。
    pub fn register(&mut self, name: &[u8]) -> Result<usize, ChanError> {
        if name.is_empty() || name.len() > CHAN_NAME_MAX {
            return Err(ChanError::PayloadTooBig);
        }
        for c in &self.chans {
            if c.used && &c.name[..c.name_len as usize] == name {
                return Err(ChanError::DuplicateName);
            }
        }
        let slot = (0..CHAN_MAX).find(|&i| !self.chans[i].used).ok_or(ChanError::Full)?;
        let c = &mut self.chans[slot];
        c.used = true;
        c.name[..name.len()].copy_from_slice(name);
        c.name_len = name.len() as u8;
        Ok(slot)
    }

    fn chan_by_name(&self, name: &[u8]) -> Result<usize, ChanError> {
        (0..CHAN_MAX)
            .find(|&i| self.chans[i].used && &self.chans[i].name[..self.chans[i].name_len as usize] == name)
            .ok_or(ChanError::UnknownChannel)
    }

    /// 订阅。`allowed` 由调用方按白名单裁决置位（false 即拒绝订阅）。
    pub fn subscribe(&mut self, name: &[u8], pid: u32, allowed: bool) -> Result<(), ChanError> {
        if !allowed {
            return Err(ChanError::NotAllowed);
        }
        let ci = self.chan_by_name(name)?;
        let c = &mut self.chans[ci];
        if c.subs.iter().any(|s| matches!(s, Some(s) if s.pid == pid)) {
            return Ok(()); // 幂等重订阅
        }
        let slot = (0..SUBS_MAX).find(|&i| c.subs[i].is_none()).ok_or(ChanError::Full)?;
        c.subs[slot] = Some(Sub::new(pid));
        Ok(())
    }

    pub fn unsubscribe(&mut self, name: &[u8], pid: u32) -> Result<(), ChanError> {
        let ci = self.chan_by_name(name)?;
        let c = &mut self.chans[ci];
        match c.subs.iter_mut().find(|s| matches!(s, Some(s) if s.pid == pid)) {
            Some(s) => {
                *s = None;
                Ok(())
            }
            None => Err(ChanError::NotSubscribed),
        }
    }

    fn make_msg(&mut self, from: u32, payload: &[u8]) -> Result<Msg, ChanError> {
        if payload.len() > MSG_MAX {
            return Err(ChanError::PayloadTooBig);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        let mut m = Msg { seq, from, len: payload.len() as u8, payload: [0; MSG_MAX] };
        m.payload[..payload.len()].copy_from_slice(payload);
        Ok(m)
    }

    /// 广播：投递给通道全部订阅者（满者丢最旧）。
    pub fn broadcast(&mut self, name: &[u8], from: u32, payload: &[u8]) -> Result<usize, ChanError> {
        let ci = self.chan_by_name(name)?;
        let m = self.make_msg(from, payload)?;
        let mut n = 0;
        for s in self.chans[ci].subs.iter_mut().flatten() {
            s.push(m);
            n += 1;
        }
        self.delivered += n as u64;
        Ok(n)
    }

    /// 单发：仅投递给指定订阅者。
    pub fn send(&mut self, name: &[u8], from: u32, to: u32, payload: &[u8]) -> Result<(), ChanError> {
        let ci = self.chan_by_name(name)?;
        let m = self.make_msg(from, payload)?;
        let c = &mut self.chans[ci];
        let sub = c
            .subs
            .iter_mut()
            .flatten()
            .find(|s| s.pid == to)
            .ok_or(ChanError::NotSubscribed)?;
        sub.push(m);
        self.delivered += 1;
        Ok(())
    }

    /// 收（FIFO）。`missed` 语义：与 inputsvc 相同，本层以 dropped 丢最旧计。
    pub fn poll(&mut self, name: &[u8], pid: u32) -> Option<Msg> {
        let ci = self.chan_by_name(name).ok()?;
        let c = &mut self.chans[ci];
        let sub = c.subs.iter_mut().flatten().find(|s| s.pid == pid)?;
        sub.pop()
    }

    /// 某订阅者慢消费者计数（审计/诊断）。
    pub fn dropped_of(&self, name: &[u8], pid: u32) -> Option<u64> {
        let ci = self.chan_by_name(name).ok()?;
        self.chans[ci].subs.iter().flatten().find(|s| s.pid == pid).map(|s| s.dropped)
    }

    pub fn registered(&self) -> Vec<&[u8]> {
        self.chans
            .iter()
            .filter(|c| c.used)
            .map(|c| &c.name[..c.name_len as usize])
            .collect()
    }
}

impl Default for MsgBus {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_subscribe_broadcast_poll_fifo() {
        let mut bus = MsgBus::new();
        bus.register(b"dom-events").expect("注册");
        assert_eq!(bus.register(b"dom-events"), Err(ChanError::DuplicateName));
        bus.subscribe(b"dom-events", 1, true).expect("订阅");
        bus.subscribe(b"dom-events", 2, true).expect("订阅");
        assert_eq!(bus.subscribe(b"dom-events", 3, false), Err(ChanError::NotAllowed), "白名单未放行");
        for i in 0..10u8 {
            bus.broadcast(b"dom-events", 9, &[i]).expect("广播");
        }
        for i in 0..10u8 {
            let m = bus.poll(b"dom-events", 1).expect("FIFO");
            assert_eq!(m.payload[0], i, "同通道 FIFO 顺序");
            assert_eq!(m.from, 9);
            assert_eq!(bus.poll(b"dom-events", 2).unwrap().payload[0], i);
        }
        assert!(bus.poll(b"dom-events", 1).is_none());
        assert_eq!(bus.delivered, 20);
    }

    #[test]
    fn broadcast_fifo_order_x1000() {
        // 总案验收：广播顺序保证（同通道 FIFO）×1000。
        let mut bus = MsgBus::new();
        bus.register(b"bench").expect("注册");
        bus.subscribe(b"bench", 1, true).expect("订阅");
        for i in 0..1000u64 {
            let bytes = (i as u64).to_le_bytes();
            bus.broadcast(b"bench", 7, &bytes).expect("广播");
            // 边发边收：消费端看到的相对顺序必须严格递增。
            let m = bus.poll(b"bench", 1).expect("应有消息");
            assert_eq!(u64::from_le_bytes(m.payload[..8].try_into().expect("8B")), i);
        }
        assert_eq!(bus.delivered, 1000);
    }

    #[test]
    fn slow_consumer_drops_oldest_with_counter() {
        let mut bus = MsgBus::new();
        bus.register(b"hot").expect("注册");
        bus.subscribe(b"hot", 1, true).expect("订阅");
        // 不消费连发 CHAN_CAP+7 条。
        for i in 0..(CHAN_CAP as u64 + 7) {
            bus.broadcast(b"hot", 9, &(i as u64).to_le_bytes()).expect("广播");
        }
        assert_eq!(bus.dropped_of(b"hot", 1), Some(7), "慢消费者丢最旧计数");
        // 最旧 7 条被丢，先收到的应是 seq=7。
        let first = bus.poll(b"hot", 1).expect("消息");
        assert_eq!(u64::from_le_bytes(first.payload[..8].try_into().expect("8B")), 7);
        assert_eq!(bus.delivered, CHAN_CAP as u64 + 7, "发布者不被阻塞");
    }

    #[test]
    fn send_single_target_and_capacity_edges() {
        let mut bus = MsgBus::new();
        bus.register(b"dm").expect("注册");
        bus.subscribe(b"dm", 1, true).expect("订阅");
        bus.subscribe(b"dm", 2, true).expect("订阅");
        bus.send(b"dm", 9, 2, b"just-for-2").expect("单发");
        assert!(bus.poll(b"dm", 1).is_none(), "订阅者1不该收到");
        assert_eq!(bus.poll(b"dm", 2).unwrap().payload[..10], *b"just-for-2");
        // 载荷超限。
        assert_eq!(bus.broadcast(b"dm", 9, &[0u8; MSG_MAX + 1]), Err(ChanError::PayloadTooBig));
        // 未注册通道。
        assert_eq!(bus.broadcast(b"ghost", 9, b"x"), Err(ChanError::UnknownChannel));
        // 未订阅即单发。
        assert_eq!(bus.send(b"dm", 9, 3, b"x"), Err(ChanError::NotSubscribed));
        // 通道表满。
        for i in 0..(CHAN_MAX - 1) {
            let name = [b'c', b'0' + i as u8];
            bus.register(&name).expect("注册");
        }
        assert_eq!(bus.register(b"c-last"), Err(ChanError::Full));
    }
}
