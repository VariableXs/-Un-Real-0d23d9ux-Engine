//! UNX-F10398 移交下游确认（C4 域 F10401 接棒确认）（0x1bc40 段）
//! 锚点：UNX-F10398-J1 移交下游确认（C4 接棒四要素+回执闭环）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1bc40;
pub const EV_NO_ANCHOR: u32 = SEG_BASE + 1;
pub const EV_NO_SCOPE: u32 = SEG_BASE + 2;
pub const EV_NO_ACK: u32 = SEG_BASE + 3;
pub const EV_SEALED: u32 = SEG_BASE + 4;
pub const EV_TWICE: u32 = SEG_BASE + 5;
pub const EV_RANGE_OUT: u32 = SEG_BASE + 6;

pub const DOWN_ANCHOR: u16 = 10401;

/// 移交包四要素：锚点卡 / 范围清单 / 状态面 / 回执
#[derive(Clone, Debug)]
pub struct HandoffPack {
    pub anchor_card: u16,
    pub scope: Vec<u16>,
    pub acked: bool,
}

impl HandoffPack {
    pub fn new(anchor_card: u16) -> Self {
        HandoffPack { anchor_card, scope: Vec::new(), acked: false }
    }
    pub fn add_scope(&mut self, card: u16) -> Result<(), u32> {
        if card < DOWN_ANCHOR {
            return Err(EV_RANGE_OUT);
        }
        for c in self.scope.iter() {
            if *c == card {
                return Err(EV_TWICE);
            }
        }
        self.scope.push(card);
        Ok(())
    }
    pub fn ack(&mut self) {
        self.acked = true;
    }
    pub fn ready(&self) -> bool {
        self.anchor_card == DOWN_ANCHOR && !self.scope.is_empty() && self.acked
    }
}

/// 下游确认器：移交包登记 + 锚点核 + 回执闭环
pub struct DownstreamAck {
    pack: Option<HandoffPack>,
    confirmed: bool,
    sealed: bool,
}

impl DownstreamAck {
    pub fn new() -> Self {
        DownstreamAck { pack: None, confirmed: false, sealed: false }
    }

    pub fn submit(&mut self, pack: HandoffPack) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.pack.is_some() {
            return Err(EV_TWICE);
        }
        if pack.anchor_card != DOWN_ANCHOR {
            self.sealed = true;
            return Err(EV_NO_ANCHOR);
        }
        if pack.scope.is_empty() {
            self.sealed = true;
            return Err(EV_NO_SCOPE);
        }
        self.pack = Some(pack);
        Ok(())
    }

    pub fn ack(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        match &mut self.pack {
            None => {
                self.sealed = true;
                Err(EV_NO_ANCHOR)
            }
            Some(p) => {
                p.ack();
                Ok(())
            }
        }
    }

    /// 确认闭环：包齐备且回执已开
    pub fn confirm(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.confirmed {
            return Err(EV_TWICE);
        }
        let p = match &self.pack {
            None => {
                self.sealed = true;
                return Err(EV_NO_ANCHOR);
            }
            Some(p) => p,
        };
        if !p.acked {
            self.sealed = true;
            return Err(EV_NO_ACK);
        }
        if !p.ready() {
            self.sealed = true;
            return Err(EV_NO_SCOPE);
        }
        let n = p.scope.len();
        self.confirmed = true;
        self.sealed = true;
        Ok(n)
    }

    pub fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    pub fn scope_len(&self) -> Option<usize> {
        self.pack.as_ref().map(|p| p.scope.len())
    }
}

pub fn run_unx10398_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10398");
    s.add("J1-段锚", SEG_BASE == 0x1bc40, "下游确认段基址 0x1bc40");
    s.add("J2-锚卡", DOWN_ANCHOR == 10401, "C4 接棒锚卡 F10401");
    s.add("J3-新器空态", {
        let d = DownstreamAck::new();
        d.scope_len().is_none() && !d.is_confirmed()
    }, "新器无包未确认");
    s.add("J4-锚错拒收", {
        let mut d = DownstreamAck::new();
        d.submit(HandoffPack::new(10402)).unwrap_err() == EV_NO_ANCHOR
    }, "锚 10402 EV_NO_ANCHOR");
    s.add("J5-空范围拒收", {
        let mut d = DownstreamAck::new();
        d.submit(HandoffPack::new(DOWN_ANCHOR)).unwrap_err() == EV_NO_SCOPE
    }, "空 scope EV_NO_SCOPE");
    s.add("J6-范围入包", {
        let mut p = HandoffPack::new(DOWN_ANCHOR);
        p.add_scope(10401).unwrap();
        p.add_scope(10402).unwrap();
        p.scope.len() == 2
    }, "两卡入 scope");
    s.add("J7-范围越界", {
        let mut p = HandoffPack::new(DOWN_ANCHOR);
        p.add_scope(10400).unwrap_err() == EV_RANGE_OUT
    }, "10400 之下 EV_RANGE_OUT");
    s.add("J8-范围重卡", {
        let mut p = HandoffPack::new(DOWN_ANCHOR);
        p.add_scope(10401).unwrap();
        p.add_scope(10401).unwrap_err() == EV_TWICE
    }, "重卡 EV_TWICE");
    s.add("J9-未回执拒确认", {
        let mut d = DownstreamAck::new();
        let mut p = HandoffPack::new(DOWN_ANCHOR);
        p.add_scope(10401).unwrap();
        d.submit(p).unwrap();
        d.confirm().unwrap_err() == EV_NO_ACK
    }, "无回执确认 EV_NO_ACK");
    s.add("J10-回执确认闭环", {
        let mut d = DownstreamAck::new();
        let mut p = HandoffPack::new(DOWN_ANCHOR);
        p.add_scope(10401).unwrap();
        p.add_scope(10402).unwrap();
        p.add_scope(10403).unwrap();
        d.submit(p).unwrap();
        d.ack().unwrap();
        d.confirm() == Ok(3)
    }, "回执后确认 Ok(3)");
    s.add("J11-确认态可查", {
        let mut d = DownstreamAck::new();
        let mut p = HandoffPack::new(DOWN_ANCHOR);
        p.add_scope(10401).unwrap();
        d.submit(p).unwrap();
        d.ack().unwrap();
        d.confirm().unwrap();
        d.is_confirmed()
    }, "确认后 is_confirmed 真");
    s.add("J12-二次确认拒", {
        let mut d = DownstreamAck::new();
        let mut p = HandoffPack::new(DOWN_ANCHOR);
        p.add_scope(10401).unwrap();
        d.submit(p).unwrap();
        d.ack().unwrap();
        d.confirm().unwrap();
        d.confirm().unwrap_err() == EV_TWICE
    }, "二次确认 EV_TWICE");
    s.add("J13-重复提交拒", {
        let mut d = DownstreamAck::new();
        let mut p1 = HandoffPack::new(DOWN_ANCHOR);
        p1.add_scope(10401).unwrap();
        d.submit(p1).unwrap();
        let mut p2 = HandoffPack::new(DOWN_ANCHOR);
        p2.add_scope(10401).unwrap();
        d.submit(p2).unwrap_err() == EV_TWICE
    }, "二次提交 EV_TWICE");
    s.add("J14-确认封账", {
        let mut d = DownstreamAck::new();
        let mut p = HandoffPack::new(DOWN_ANCHOR);
        p.add_scope(10401).unwrap();
        d.submit(p).unwrap();
        d.ack().unwrap();
        d.confirm().unwrap();
        d.ack().unwrap_err() == EV_SEALED
    }, "确认后回执 EV_SEALED");
    s.add("J15-无包回执拒", {
        let mut d = DownstreamAck::new();
        d.ack().unwrap_err() == EV_NO_ANCHOR
    }, "无包回执 EV_NO_ANCHOR");
    s.add("J16-包 ready 判定", {
        let mut p = HandoffPack::new(DOWN_ANCHOR);
        let a = !p.ready();
        p.add_scope(10401).unwrap();
        let b = !p.ready();
        p.ack();
        p.ready()
            && a && b
    }, "缺范围缺回执皆未 ready，齐备 ready");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_downstream_ack() {
        let mut d = DownstreamAck::new();
        let mut p = HandoffPack::new(DOWN_ANCHOR);
        p.add_scope(10401).unwrap();
        p.add_scope(10402).unwrap();
        d.submit(p).unwrap();
        d.ack().unwrap();
        assert_eq!(d.confirm().unwrap(), 2);
    }

    #[test]
    fn checks_green() {
        let s = run_unx10398_checks();
        assert!(s.all_passed(), "UNX-F10398 判据全绿");
    }
}
