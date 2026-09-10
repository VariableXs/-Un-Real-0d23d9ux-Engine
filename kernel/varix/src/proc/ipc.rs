//! AI-12 用户态 IPC 消息传递（F293）。
//!
//! 定长端口表 + 定长消息队列，零分配。能力位决定谁能发、谁能收：
//! 没有 `RIGHT_SEND` 的进程往端口写一律 `Eacces`，没有 `RIGHT_RECV` 读一律
//! `Eacces`，队列满返回 `Eagain` 而不是无限扩张。

use crate::entry::ErrNo;

pub const MAX_PORTS: usize = 16;
pub const MAX_MSG: usize = 32;
pub const MAX_PAYLOAD: usize = 96;

pub const RIGHT_SEND: u8 = 1 << 0;
pub const RIGHT_RECV: u8 = 1 << 1;
pub const RIGHT_OWNER: u8 = 1 << 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Port {
    pub id: u32,
    pub owner: u32,
    pub rights: u8,
    pub closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    pub from: u32,
    pub to: u32,
    pub kind: u8,
    pub len: usize,
    pub payload: [u8; MAX_PAYLOAD],
}

#[derive(Clone, Copy, Debug)]
pub struct IpcBus {
    pub ports: [Option<Port>; MAX_PORTS],
    pub port_count: usize,
    pub queue: [Option<Message>; MAX_MSG],
    pub head: usize,
    pub count: usize,
    pub dropped: u32,
}

impl IpcBus {
    pub const fn new() -> IpcBus {
        IpcBus {
            ports: [None; MAX_PORTS],
            port_count: 0,
            queue: [None; MAX_MSG],
            head: 0,
            count: 0,
            dropped: 0,
        }
    }

    pub fn create_port(&mut self, id: u32, owner: u32, rights: u8) -> Result<u32, ErrNo> {
        if self.find_port(id).is_some() {
            return Err(ErrNo::Einval);
        }
        if self.port_count >= MAX_PORTS {
            return Err(ErrNo::Enospc);
        }
        self.ports[self.port_count] = Some(Port { id, owner, rights, closed: false });
        self.port_count += 1;
        Ok(id)
    }

    pub fn find_port(&self, id: u32) -> Option<usize> {
        (0..self.port_count).find(|&i| self.ports[i].map(|p| p.id) == Some(id))
    }

    pub fn close_port(&mut self, id: u32) -> Result<(), ErrNo> {
        match self.find_port(id) {
            Some(i) => {
                if let Some(p) = self.ports[i].as_mut() {
                    p.closed = true;
                }
                // 端口关闭后残留消息立即作废，避免新主收到旧消息。
                let mut kept = 0usize;
                let mut survivors = [None; MAX_MSG];
                for slot in self.queue.iter() {
                    if let Some(m) = *slot {
                        if m.to != id && kept < MAX_MSG {
                            survivors[kept] = Some(m);
                            kept += 1;
                        }
                    }
                }
                self.queue = survivors;
                self.head = kept % MAX_MSG;
                self.count = kept;
                Ok(())
            }
            None => Err(ErrNo::Ebadf),
        }
    }

    /// F293 发送：能力位 + 端口状态 + 长度 + 队列容量 四道检查。
    pub fn send(&mut self, from: u32, to: u32, kind: u8, payload: &[u8]) -> Result<usize, ErrNo> {
        if payload.len() > MAX_PAYLOAD {
            return Err(ErrNo::Einval);
        }
        let idx = match self.find_port(to) {
            Some(i) => i,
            None => return Err(ErrNo::Ebadf),
        };
        let port = match self.ports[idx] {
            Some(p) => p,
            None => return Err(ErrNo::Ebadf),
        };
        if port.closed {
            return Err(ErrNo::Ebadf);
        }
        if port.rights & RIGHT_SEND == 0 {
            return Err(ErrNo::Eacces);
        }
        if self.count >= MAX_MSG {
            self.dropped += 1;
            return Err(ErrNo::Eagain);
        }
        let mut msg = Message { from, to, kind, len: payload.len(), payload: [0u8; MAX_PAYLOAD] };
        for (i, b) in payload.iter().enumerate() {
            msg.payload[i] = *b;
        }
        self.queue[self.head] = Some(msg);
        self.head = (self.head + 1) % MAX_MSG;
        self.count += 1;
        Ok(payload.len())
    }

    /// F293 接收：取最早一条（FIFO），无消息返回 `Eagain`。
    pub fn recv(&mut self, port_id: u32, out: &mut [u8]) -> Result<(u32, u8, usize), ErrNo> {
        let idx = match self.find_port(port_id) {
            Some(i) => i,
            None => return Err(ErrNo::Ebadf),
        };
        let port = match self.ports[idx] {
            Some(p) => p,
            None => return Err(ErrNo::Ebadf),
        };
        if port.closed {
            return Err(ErrNo::Ebadf);
        }
        if port.rights & RIGHT_RECV == 0 {
            return Err(ErrNo::Eacces);
        }
        // 队列是环形：最旧的一条在 (head - count + MAX_MSG) % MAX_MSG。
        let oldest = (self.head + MAX_MSG - self.count) % MAX_MSG;
        match self.queue[oldest] {
            Some(m) => {
                let n = m.len.min(out.len());
                for i in 0..n {
                    out[i] = m.payload[i];
                }
                self.queue[oldest] = None;
                self.count -= 1;
                Ok((m.from, m.kind, n))
            }
            None => Err(ErrNo::Eagain),
        }
    }

    /// F293 窥视：不取出，仅查看队首。
    pub fn peek(&self, port_id: u32) -> Result<Message, ErrNo> {
        match self.find_port(port_id) {
            Some(i) => match self.ports[i] {
                Some(p) if !p.closed && p.rights & RIGHT_RECV != 0 => {
                    let oldest = (self.head + MAX_MSG - self.count) % MAX_MSG;
                    self.queue[oldest].ok_or(ErrNo::Eagain)
                }
                _ => Err(ErrNo::Eacces),
            },
            None => Err(ErrNo::Ebadf),
        }
    }

    pub fn pending(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f293_send_recv_roundtrip() {
        let mut bus = IpcBus::new();
        assert_eq!(bus.create_port(1, 100, RIGHT_SEND | RIGHT_RECV), Ok(1));
        assert_eq!(bus.send(100, 1, 7, b"hello"), Ok(5));
        let mut out = [0u8; 16];
        assert_eq!(bus.recv(1, &mut out), Ok((100, 7, 5)));
        assert_eq!(&out[..5], b"hello");
        assert_eq!(bus.pending(), 0);
    }

    #[test]
    fn f293_rights_are_enforced() {
        let mut bus = IpcBus::new();
        bus.create_port(1, 100, RIGHT_SEND).unwrap();
        bus.create_port(2, 100, RIGHT_RECV).unwrap();
        assert_eq!(bus.send(100, 2, 0, b"x"), Err(ErrNo::Eacces));
        let mut out = [0u8; 4];
        assert_eq!(bus.recv(1, &mut out), Err(ErrNo::Eacces));
    }

    #[test]
    fn f293_backpressure_and_oversize() {
        let mut bus = IpcBus::new();
        bus.create_port(1, 1, RIGHT_SEND | RIGHT_RECV).unwrap();
        for _ in 0..MAX_MSG {
            assert_eq!(bus.send(1, 1, 0, b"m"), Ok(1));
        }
        assert_eq!(bus.send(1, 1, 0, b"m"), Err(ErrNo::Eagain));
        assert_eq!(bus.dropped, 1);
        let big = [0u8; MAX_PAYLOAD + 1];
        assert_eq!(bus.send(1, 1, 0, &big), Err(ErrNo::Einval));
    }

    #[test]
    fn f293_close_invalidates_pending() {
        let mut bus = IpcBus::new();
        bus.create_port(1, 1, RIGHT_SEND | RIGHT_RECV).unwrap();
        bus.create_port(2, 2, RIGHT_SEND | RIGHT_RECV).unwrap();
        bus.send(1, 1, 0, b"a").unwrap();
        bus.send(1, 2, 0, b"b").unwrap();
        assert_eq!(bus.pending(), 2);
        assert_eq!(bus.close_port(1), Ok(()));
        assert_eq!(bus.pending(), 1);
        let mut out = [0u8; 4];
        assert_eq!(bus.recv(2, &mut out), Ok((1, 0, 1)));
        assert_eq!(bus.close_port(999), Err(ErrNo::Ebadf));
    }

    #[test]
    fn f293_fifo_order_and_peek() {
        let mut bus = IpcBus::new();
        bus.create_port(1, 1, RIGHT_SEND | RIGHT_RECV).unwrap();
        bus.send(1, 1, 0, b"first").unwrap();
        bus.send(1, 1, 0, b"second").unwrap();
        let p = bus.peek(1).unwrap();
        assert_eq!(&p.payload[..p.len], b"first");
        let mut out = [0u8; 16];
        assert_eq!(bus.recv(1, &mut out), Ok((1, 0, 5)));
        assert_eq!(&out[..5], b"first");
    }
}
