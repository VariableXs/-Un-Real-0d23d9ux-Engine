//! UNX-F11066 unix socket 流式回归段（0x1e840 段）
//!
//! B09/B16/B17/B18 四批判据在此重放：监听端以固定 backlog 记账，
//! 每条连接登记 (link, client fd, server fd, 字节序号, 半闭方向)。
//! 连接语义要求 server 端恒等于监听 fd、收发序号守恒、半闭后不再
//! 允许写方向发送，且 link 号段内唯一。
//!
//! 不变式：bytes_tx == bytes_rx；pending + accepted == 已连接数；
//! verify() 只在十席齐全且逐链合法时返回 10；冻结后写路径全拒。
//!
//! 结构：StreamLink（连接行）/ StreamListener（监听账）/ StreamLedger
//! （四批总账 + 重放 + 冻结）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11066 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e840;

/// 空账：fd 为 0、零字节发送、或空队列接收
pub const US_EMPTY: u32 = SEG_BASE + 0x01;
/// 越界：backlog 超上限或单次发送超窗口
pub const US_RANGE: u32 = SEG_BASE + 0x02;
/// 重号：link 号重复登记
pub const US_DUP: u32 = SEG_BASE + 0x03;
/// 失配：连接语义冲突（server 错端/半闭后写）
pub const US_MISMATCH: u32 = SEG_BASE + 0x04;
/// 短缺：backlog 满或重放席位不足
pub const US_SHORT: u32 = SEG_BASE + 0x05;
/// 已封：段账冻结后的连接/收发
pub const US_FROZEN: u32 = SEG_BASE + 0x06;

/// FNV 折前缀：低两字节 0x66 标记 11066 号卡
pub const US_TAG: u64 = 0x1e80_66;

/// 本段固定十席
const SEATS: usize = 10;
/// backlog 上限
const MAX_BACKLOG: u8 = 128;
/// 单次发送窗口
const MAX_SEND: u32 = 65_536;

fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01b3)
}

fn fnv_u16(h: u64, v: u16) -> u64 {
    fnv_byte(fnv_byte(h, (v & 0xff) as u8), ((v >> 8) & 0xff) as u8)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut k = 0u32;
    while k < 4 {
        h = fnv_byte(h, ((v >> k.wrapping_mul(8)) & 0xff) as u8);
        k = k.wrapping_add(1);
    }
    h
}

/// 连接状态（流式语义闭集）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// 握手前
    Connecting,
    /// 双向可收发
    Established,
    /// 至少一个方向已关闭
    HalfClosed,
    /// 对端消失
    Broken,
}

impl LinkState {
    /// 落账判别码
    pub fn code(self) -> u8 {
        match self {
            LinkState::Connecting => 1,
            LinkState::Established => 2,
            LinkState::HalfClosed => 3,
            LinkState::Broken => 4,
        }
    }

    /// 是否允许继续写方向发送
    pub fn writable(self) -> bool {
        match self {
            LinkState::Established => true,
            _ => false,
        }
    }
}

/// 一条流式连接
#[derive(Clone, Debug)]
pub struct StreamLink {
    /// 连接号（1..=10，段内唯一）
    pub link: u16,
    /// 客户端 fd
    pub client: u32,
    /// 服务端 fd（必须等于监听 fd）
    pub server: u32,
    /// 连接状态
    pub state: LinkState,
    /// 已发送累计字节
    pub tx_seq: u32,
    /// 已接收累计字节
    pub rx_seq: u32,
    /// 写方向已关闭
    pub shutdown_wr: bool,
    /// 读方向已关闭
    pub shutdown_rd: bool,
}

impl StreamLink {
    /// 新建 Connecting 行
    pub fn new(link: u16, client: u32, server: u32) -> StreamLink {
        StreamLink {
            link,
            client,
            server,
            state: LinkState::Connecting,
            tx_seq: 0,
            rx_seq: 0,
            shutdown_wr: false,
            shutdown_rd: false,
        }
    }

    /// 握手完成
    pub fn establish(&mut self, slot: u8) -> Result<(), u32> {
        if self.client == 0 || self.server == 0 {
            return Err(US_EMPTY);
        }
        if slot == 0 {
            return Err(US_RANGE);
        }
        if self.state != LinkState::Connecting {
            return Err(US_MISMATCH);
        }
        self.state = LinkState::Established;
        Ok(())
    }

    /// 写方向发送：半闭/断裂/零长/超窗四路拒
    pub fn send(&mut self, bytes: u32) -> Result<u32, u32> {
        if self.shutdown_wr || !self.state.writable() {
            return Err(US_MISMATCH);
        }
        if bytes == 0 {
            return Err(US_EMPTY);
        }
        if bytes > MAX_SEND {
            return Err(US_RANGE);
        }
        self.tx_seq = self.tx_seq.wrapping_add(bytes);
        Ok(self.tx_seq)
    }

    /// 读方向接收：不得超过对端已发
    pub fn recv(&mut self, bytes: u32) -> Result<u32, u32> {
        if self.shutdown_rd {
            return Err(US_MISMATCH);
        }
        if bytes == 0 {
            return Err(US_EMPTY);
        }
        if bytes > self.tx_seq.saturating_sub(self.rx_seq) {
            return Err(US_SHORT);
        }
        self.rx_seq = self.rx_seq.wrapping_add(bytes);
        Ok(self.rx_seq)
    }

    /// 关闭写方向（半闭）
    pub fn close_write(&mut self) -> Result<(), u32> {
        if self.shutdown_wr {
            return Err(US_DUP);
        }
        if self.state == LinkState::Broken {
            return Err(US_MISMATCH);
        }
        self.shutdown_wr = true;
        self.state = LinkState::HalfClosed;
        Ok(())
    }

    /// 关闭读方向
    pub fn close_read(&mut self) -> Result<(), u32> {
        if self.shutdown_rd {
            return Err(US_DUP);
        }
        self.shutdown_rd = true;
        if self.shutdown_wr {
            self.state = LinkState::Broken;
        }
        Ok(())
    }

    /// 双向序号守恒
    pub fn balanced(&self) -> bool {
        self.tx_seq == self.rx_seq
    }

    /// 单链语义合法性
    pub fn semantic_ok(&self, listener_fd: u32) -> bool {
        self.server == listener_fd
            && self.client != 0
            && self.balanced()
            && !(self.shutdown_wr && self.state == LinkState::Established)
    }

    /// 连接行指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_u16(h, self.link);
        h = fnv_u32(h, self.client);
        h = fnv_u32(h, self.server);
        h = fnv_byte(h, self.state.code());
        h = fnv_u32(h, self.tx_seq);
        h = fnv_u32(h, self.rx_seq);
        h = fnv_byte(h, if self.shutdown_wr { 1 } else { 0 });
        fnv_byte(h, if self.shutdown_rd { 1 } else { 0 })
    }
}

/// 监听账：backlog 水位 + accept 计数
#[derive(Clone, Debug)]
pub struct StreamListener {
    pub fd: u32,
    pub backlog_max: u8,
    pub pending: u8,
    pub accepted: u32,
    pub refusals: u32,
    frozen: Option<u64>,
}

impl StreamListener {
    /// 新建监听端（未 listen）
    pub fn new(fd: u32) -> StreamListener {
        StreamListener { fd, backlog_max: 0, pending: 0, accepted: 0, refusals: 0, frozen: None }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(US_FROZEN);
        }
        Ok(())
    }

    /// listen(2)：0 与超上限拒
    pub fn listen(&mut self, backlog: u8) -> Result<(), u32> {
        self.guard()?;
        if self.fd == 0 || backlog == 0 {
            return Err(US_EMPTY);
        }
        if backlog > MAX_BACKLOG {
            return Err(US_RANGE);
        }
        self.backlog_max = backlog;
        Ok(())
    }

    /// 新连接入 backlog；满则显式记 refusal
    pub fn admit(&mut self) -> Result<u8, u32> {
        self.guard()?;
        if self.pending >= self.backlog_max {
            self.refusals = self.refusals.wrapping_add(1);
            return Err(US_SHORT);
        }
        self.pending = self.pending.wrapping_add(1);
        Ok(self.pending)
    }

    /// accept：空 backlog 拒
    pub fn accept(&mut self) -> Result<u8, u32> {
        self.guard()?;
        if self.pending == 0 {
            return Err(US_EMPTY);
        }
        self.pending = self.pending.wrapping_sub(1);
        self.accepted = self.accepted.wrapping_add(1);
        Ok(self.pending)
    }

    /// 监听账指纹
    pub fn fold(&self) -> u64 {
        let mut h = US_TAG;
        h = fnv_u32(h, self.fd);
        h = fnv_byte(h, self.backlog_max);
        h = fnv_byte(h, self.pending);
        h = fnv_u32(h, self.accepted);
        fnv_u32(h, self.refusals)
    }

    /// 冻结监听端
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }
}

/// B09/B16/B17/B18 流式总账
#[derive(Clone, Debug)]
pub struct StreamLedger {
    /// 监听端
    pub listener: StreamListener,
    /// 连接行
    pub links: Vec<StreamLink>,
    /// 累计发送
    pub bytes_tx: u32,
    /// 累计接收
    pub bytes_rx: u32,
    /// 最近一次重放合法链数
    pub verified: u16,
    frozen: Option<u64>,
}

impl StreamLedger {
    /// 以监听 fd 建空账
    pub fn new(fd: u32) -> StreamLedger {
        let mut listener = StreamListener::new(fd);
        if listener.listen(MAX_BACKLOG).is_err() {
            listener.backlog_max = 0;
        }
        StreamLedger { listener, links: Vec::new(), bytes_tx: 0, bytes_rx: 0, verified: 0, frozen: None }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(US_FROZEN);
        }
        Ok(())
    }

    fn index(&self, link: u16) -> Option<usize> {
        let mut k = 0usize;
        while k < self.links.len() {
            if self.links[k].link == link {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 建连：重号 / 错端 / backlog 满三路拒
    pub fn connect(&mut self, mut link: StreamLink) -> Result<(), u32> {
        self.guard()?;
        if self.index(link.link).is_some() {
            return Err(US_DUP);
        }
        if link.server != self.listener.fd {
            return Err(US_MISMATCH);
        }
        let slot = self.listener.admit()?;
        link.establish(slot)?;
        self.links.push(link);
        Ok(())
    }

    /// 向指定连接发送
    pub fn send(&mut self, link: u16, bytes: u32) -> Result<u32, u32> {
        self.guard()?;
        let idx = match self.index(link) {
            Some(i) => i,
            None => return Err(US_EMPTY),
        };
        let seq = self.links[idx].send(bytes)?;
        self.bytes_tx = self.bytes_tx.wrapping_add(bytes);
        Ok(seq)
    }

    /// 从指定连接接收
    pub fn recv(&mut self, link: u16, bytes: u32) -> Result<u32, u32> {
        self.guard()?;
        let idx = match self.index(link) {
            Some(i) => i,
            None => return Err(US_EMPTY),
        };
        let seq = self.links[idx].recv(bytes)?;
        self.bytes_rx = self.bytes_rx.wrapping_add(bytes);
        Ok(seq)
    }

    /// 半闭指定连接的写方向
    pub fn shutdown_write(&mut self, link: u16) -> Result<(), u32> {
        self.guard()?;
        let idx = match self.index(link) {
            Some(i) => i,
            None => return Err(US_EMPTY),
        };
        self.links[idx].close_write()
    }

    /// 全部连接 FD 相等且序号守恒
    pub fn connection_semantics(&self) -> bool {
        if self.links.is_empty() {
            return false;
        }
        let mut k = 0usize;
        while k < self.links.len() {
            if !self.links[k].semantic_ok(self.listener.fd) {
                return false;
            }
            k += 1;
        }
        self.bytes_tx == self.bytes_rx
    }

    /// link 号唯一且递增
    pub fn ids_unique(&self) -> bool {
        let mut k = 1usize;
        while k < self.links.len() {
            if self.links[k].link <= self.links[k - 1].link {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 重放：不足十席 US_SHORT，语义不合 US_MISMATCH
    pub fn verify(&mut self) -> Result<u16, u32> {
        self.guard()?;
        if self.links.len() < SEATS {
            return Err(US_SHORT);
        }
        let mut ok = 0u16;
        let mut k = 0usize;
        while k < self.links.len() {
            if self.links[k].semantic_ok(self.listener.fd) {
                ok = ok.wrapping_add(1);
            }
            k += 1;
        }
        self.verified = ok;
        if ok as usize != self.links.len() {
            return Err(US_MISMATCH);
        }
        Ok(ok)
    }

    /// 段账指纹
    pub fn fold(&self) -> u64 {
        let mut h = self.listener.fold();
        h = fnv_u32(h, self.bytes_tx);
        h = fnv_u32(h, self.bytes_rx);
        h = fnv_u16(h, self.verified);
        let mut k = 0usize;
        while k < self.links.len() {
            h = self.links[k].digest(h);
            k += 1;
        }
        h
    }

    /// 冻结段账
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }
}

/// 四条负路径：零 backlog / 超限 backlog / 重号 / 错端
pub fn inject_four() -> [bool; 4] {
    let mut l0 = StreamListener::new(7);
    let empty = matches!(l0.listen(0), Err(US_EMPTY));
    let mut l1 = StreamListener::new(7);
    let range = matches!(l1.listen(200), Err(US_RANGE));
    let mut led = StreamLedger::new(7);
    let mut one = StreamLink::new(1, 11, 7);
    let first = led.connect(one.clone()).is_ok();
    one.client = 12;
    let dup = first && matches!(led.connect(one), Err(US_DUP));
    let mut l2 = StreamLedger::new(7);
    let wrong = matches!(l2.connect(StreamLink::new(2, 13, 9)), Err(US_MISMATCH));
    [empty, range, dup, wrong]
}

/// 构造流式样本（十链、先发后收、两链半闭）
fn sample_ledger() -> Result<StreamLedger, u32> {
    let mut led = StreamLedger::new(7);
    let mut i = 1u16;
    while i <= 10 {
        let link = StreamLink::new(i, 100 + i as u32, 7);
        led.connect(link)?;
        let bytes = 64 + (i as u32) * 37;
        led.send(i, bytes)?;
        led.recv(i, bytes)?;
        if i >= 9 {
            led.shutdown_write(i)?;
        }
        i = i.wrapping_add(1);
    }
    Ok(led)
}

/// UNX-F11066 判据入口
pub fn run_unx11066_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11066");
    let mut led = match sample_ledger() {
        Ok(v) => v,
        Err(_) => StreamLedger::new(7),
    };
    let verified = led.verify().unwrap_or(0);
    let mut acc = 0u8;
    while led.listener.pending > 0 {
        match led.listener.accept() {
            Ok(p) => acc = p,
            Err(_) => break,
        }
    }
    s.add(
        "J1a-流式判据全量重放",
        verified == 10 && led.links.len() == SEATS,
        "B09/B16/B17/B18 十链必须逐链合法",
    );
    s.add(
        "J1b-连接语义一致",
        led.connection_semantics() && led.listener.accepted == 10,
        "服务端 fd 恒等、序号守恒、十次 accept 落账",
    );
    s.add(
        "J2-字节守恒",
        led.bytes_tx == led.bytes_rx && led.bytes_tx > 0,
        "累计发送必须等于累计接收",
    );
    let inj = inject_four();
    s.add("J3a-零 backlog 拒", inj[0], "listen(0) 必须 US_EMPTY");
    s.add("J3b-超限 backlog 拒", inj[1], "listen(200) 必须 US_RANGE");
    s.add("J3c-重号连接拒", inj[2], "同 link 号二次建连必须 US_DUP");
    s.add("J3d-错端连接拒", inj[3], "server 非监听 fd 必须 US_MISMATCH");
    let mut fz = led.clone();
    let fp = fz.freeze();
    let frozen_ok = matches!(fz.send(1, 8), Err(US_FROZEN))
        && matches!(fz.connect(StreamLink::new(20, 120, 7)), Err(US_FROZEN));
    s.add(
        "J4-冻结不可写",
        frozen_ok && fz.fold() == fp,
        "冻结后发送/建连全拒且指纹不变",
    );
    let mut a = led.clone();
    let fp_a = a.fold();
    let mut b = led.clone();
    let sens = b.send(2, 16).is_ok() && b.fold() != fp_a;
    s.add("J5-指纹敏感性", sens, "单链多送 16 字节必须改变 fold");
    let zero_send = {
        let mut g = match sample_ledger() {
            Ok(v) => v,
            Err(_) => StreamLedger::new(7),
        };
        matches!(g.send(1, 0), Err(US_EMPTY))
    };
    let over_send = {
        let mut g = match sample_ledger() {
            Ok(v) => v,
            Err(_) => StreamLedger::new(7),
        };
        matches!(g.send(1, 70_000), Err(US_RANGE))
    };
    let over_recv = {
        let mut g = match sample_ledger() {
            Ok(v) => v,
            Err(_) => StreamLedger::new(7),
        };
        matches!(g.recv(1, 1_000_000), Err(US_SHORT))
    };
    s.add(
        "J6-范围与空账拒止",
        zero_send && over_send && over_recv,
        "零字节/超窗发送/超额接收三路都必须拒",
    );
    let unique = led.ids_unique();
    let mut dled = StreamLedger::new(7);
    let dup_reject = dled.connect(StreamLink::new(1, 31, 7)).is_ok()
        && matches!(dled.connect(StreamLink::new(1, 32, 7)), Err(US_DUP));
    s.add(
        "J7-唯一与排序不变量",
        unique && dup_reject,
        "link 号必须唯一递增且重号当场拒",
    );
    let mut partial = match sample_ledger() {
        Ok(v) => v,
        Err(_) => StreamLedger::new(7),
    };
    while partial.links.len() > 5 {
        partial.links.pop();
    }
    let negative = partial.links.len() == 5
        && matches!(partial.verify(), Err(US_SHORT))
        && partial.verified == 0;
    s.add(
        "J8-半成品不得全绿",
        negative,
        "五链半账必须 US_SHORT 且语义判据为假",
    );
    let half_closed_ok = matches!(led.shutdown_write(9), Err(US_DUP));
    s.add(
        "J10-半闭幂等拒",
        half_closed_ok && matches!(led.send(9, 1), Err(US_MISMATCH)),
        "重复半闭 US_DUP、半闭后发送 US_MISMATCH",
    );
    s.add_dyn(
        "J9-动态账",
        verified == 10 && acc == 0,
        &format!(
            "verified={} tx={} rx={} 待accept={}",
            verified, led.bytes_tx, led.bytes_rx, acc
        ),
    );
    s.add_dyn(
        "J9b-指纹账",
        led.fold() != 0,
        &format!("fold={:016x} tag={:x}", led.fold(), US_TAG),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11066_flow() {
        let mut led = sample_ledger().unwrap_or(StreamLedger::new(7));
        assert_eq!(led.verify().unwrap_or(0), 10);
        assert!(led.connection_semantics());
        assert!(led.ids_unique());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11066_checks_green() {
        assert!(run_unx11066_checks().all_passed(), "UNX-F11066 判据未全绿");
    }
}
