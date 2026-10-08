//! F060 · 事件端口包装。

use crate::abi::{EVENT_EXIT, EVENT_IO, EVENT_KEY, EVENT_SIGNAL, EVENT_TIMER, SYS_EVENT_CTL, SYS_EVENT_WAIT};
use crate::error::{invoke, Result};

/// F060 · 事件类别位。
pub const fn event_bit(kind: u32) -> u64 {
    let k = if kind > 63 { 63 } else { kind };
    1u64 << k
}

pub const KEY: u64 = event_bit(EVENT_KEY);
pub const TIMER: u64 = event_bit(EVENT_TIMER);
pub const SIGNAL: u64 = event_bit(EVENT_SIGNAL);
pub const IO: u64 = event_bit(EVENT_IO);
pub const EXIT: u64 = event_bit(EVENT_EXIT);

/// 一条内核事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    pub kind: u32,
    pub ident: u64,
    pub data: [u64; 2],
    pub tick: u64,
}

impl Event {
    pub const fn decode(raw: [u64; 5]) -> Event {
        Event {
            kind: raw[0] as u32,
            ident: raw[1],
            data: [raw[2], raw[3]],
            tick: raw[4],
        }
    }
}

/// 事件端口句柄。订阅是位掩码——没订阅的类别在内核侧连队都不排。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EventPort(pub u32);

impl EventPort {
    /// 开放一个端口并订阅 `mask` 里的类别。
    pub fn open(mask: u64) -> Result<EventPort> {
        let v = unsafe { invoke(SYS_EVENT_CTL, [u64::MAX, mask, 0, 0, 0, 0])? };
        Ok(EventPort(v as u32))
    }

    /// 收紧订阅集合。已入队的事件不会因此消失——必须先取空，否则会丢事件。
    pub fn subscribe(&self, mask: u64) -> Result<()> {
        unsafe {
            invoke(SYS_EVENT_CTL, [self.0 as u64, mask, 1, 0, 0, 0])?;
        }
        Ok(())
    }

    /// 取一条事件；队列空时返回 `Error::Empty`（不阻塞）。
    pub fn poll(&self) -> Result<Event> {
        let mut raw = [0u64; 5];
        unsafe {
            invoke(
                SYS_EVENT_WAIT,
                [self.0 as u64, raw.as_mut_ptr() as u64, 0, 0, 0, 0],
            )?;
        }
        Ok(Event::decode(raw))
    }
}
