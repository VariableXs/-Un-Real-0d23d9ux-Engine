//! UNX-F24188 vblank/HPD 内核事件面与事件队列·子功能08（订阅广播）（0x41960 段）
//!
//! 锚点：UNX-F24188-J1 广播账（扇出逐订阅者落账）；J2 广播语义（全
//! 扇出 + 独立队列 + 野订阅拒三性质）；J3 广播指纹（账折叠指纹）三
//! 判据全绿。
//! 域：CoRun·UNX-F5-B15（订阅广播面）。
//!
//! 结构：广播器 + 订阅表 + 扇出核 + 账本 + 折叠指纹 + 注入核（无订
//! 阅/野类型/重订/印后/订阅者满必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41960 独占 0x20）。
pub const SEG_BASE: u32 = 0x41960;
pub const BC_NO_SUB: u32 = SEG_BASE + 0x01; // 无订阅者
pub const BC_BAD_KIND: u32 = SEG_BASE + 0x02; // 野类型
pub const BC_DUP_SUB: u32 = SEG_BASE + 0x03; // 重复订阅
pub const BC_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const BC_FULL_SUB: u32 = SEG_BASE + 0x05; // 订阅者满

/// 订阅者独立队列。
pub struct BcSub {
    pub id: u64,
    pub cap: usize,
    buf: Vec<(u8, u64)>,
    pub dropped: u64,
}

impl BcSub {
    pub fn new(id: u64, cap: usize) -> BcSub {
        BcSub {
            id,
            cap,
            buf: Vec::new(),
            dropped: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }
}

/// 广播器：事件扇出到全部订阅者。
pub struct BcHub {
    subs: Vec<BcSub>,
    pub broadcast: u64,
    sealed: bool,
}

impl BcHub {
    pub fn new() -> BcHub {
        BcHub {
            subs: Vec::new(),
            broadcast: 0,
            sealed: false,
        }
    }

    /// 订阅：重订拒。
    pub fn subscribe(&mut self, id: u64, cap: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(BC_FROZEN);
        }
        if self.subs.iter().any(|s| s.id == id) {
            return Err(BC_DUP_SUB);
        }
        self.subs.push(BcSub::new(id, cap));
        Ok(())
    }

    pub fn sub_count(&self) -> usize {
        self.subs.len()
    }

    /// 广播：逐订阅者投递；某订阅者满则该订阅者丢计，不影响他者。
    pub fn broadcast_event(&mut self, kind: u8, ts: u64) -> Result<usize, u32> {
        if self.sealed {
            return Err(BC_FROZEN);
        }
        if kind > 7 {
            return Err(BC_BAD_KIND);
        }
        if self.subs.is_empty() {
            return Err(BC_NO_SUB);
        }
        let mut delivered = 0;
        for s in self.subs.iter_mut() {
            if s.buf.len() < s.cap {
                s.buf.push((kind, ts));
                delivered += 1;
            } else {
                s.dropped += 1;
            }
        }
        self.broadcast += 1;
        Ok(delivered)
    }

    /// 订阅者快照（id, len, dropped）。
    pub fn snapshot(&self) -> Vec<(u64, usize, u64)> {
        self.subs.iter().map(|s| (s.id, s.len(), s.dropped)).collect()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41960;
        h = h.wrapping_mul(31).wrapping_add(self.broadcast as u32);
        h = h.wrapping_mul(31).wrapping_add(self.subs.len() as u32);
        for s in &self.subs {
            h = h.wrapping_mul(31).wrapping_add(s.len() as u32);
            h = h.wrapping_mul(31).wrapping_add(s.dropped as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：两订阅者（容 2/1），广播 3 次。
pub fn mk_hub() -> BcHub {
    let mut h = BcHub::new();
    let _ = h.subscribe(1, 2);
    let _ = h.subscribe(2, 1);
    let _ = h.broadcast_event(1, 10);
    let _ = h.broadcast_event(2, 20);
    let _ = h.broadcast_event(3, 30);
    h
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 无订阅者
    got[0] = BcHub::new().broadcast_event(1, 1).err() == Some(BC_NO_SUB);
    // 2) 野类型
    let mut h = BcHub::new();
    let _ = h.subscribe(1, 4);
    got[1] = h.broadcast_event(8, 1).err() == Some(BC_BAD_KIND);
    // 3) 重复订阅
    let mut h2 = BcHub::new();
    let _ = h2.subscribe(1, 4);
    got[2] = h2.subscribe(1, 4).err() == Some(BC_DUP_SUB);
    // 4) 订阅者满：容 1 广播 2 次，第二次该订阅者丢
    let mut h3 = BcHub::new();
    let _ = h3.subscribe(7, 1);
    let _ = h3.broadcast_event(1, 1);
    let _ = h3.broadcast_event(1, 2);
    got[3] = h3.snapshot() == alloc::vec![(7, 1, 1)];
    // 5) 印后操作
    let mut h4 = mk_hub();
    h4.seal();
    got[4] = h4.broadcast_event(1, 99).err() == Some(BC_FROZEN)
        && h4.subscribe(9, 9).err() == Some(BC_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24188_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24188");
    let h = mk_hub();

    // J1 全扇出：前 2 次各收 2 事件
    let j1 = h.snapshot()[0].1 == 2 && h.snapshot()[1].1 == 1;
    s.add("J1-扇出", j1, "订阅者各自收满");

    // J2 独立队列：容异丢异
    let j2 = h.snapshot()[0].2 == 0 && h.snapshot()[1].2 == 2;
    s.add("J2-独立丢计", j2, "容1者丢2");

    // J3 订阅者数
    let j3 = h.sub_count() == 2;
    s.add("J3-订阅数", j3, "2 订阅者");

    // J4 广播计数
    let j4 = h.broadcast == 3;
    s.add("J4-广播计数", j4, "广播 3 次");

    // J5 无订阅者拒
    let j5 = BcHub::new().broadcast_event(1, 1).err() == Some(BC_NO_SUB);
    s.add("J5-无订阅拒", j5, "零订阅必拒");

    // J6 野类型拒
    let j6 = {
        let mut h2 = BcHub::new();
        let _ = h2.subscribe(1, 4);
        h2.broadcast_event(8, 1).err() == Some(BC_BAD_KIND)
    };
    s.add("J6-野类型拒", j6, "kind>7 必拒");

    // J7 重订拒
    let j7 = {
        let mut h2 = BcHub::new();
        let _ = h2.subscribe(3, 4);
        h2.subscribe(3, 4).err() == Some(BC_DUP_SUB)
    };
    s.add("J7-重订拒", j7, "同 id 重订必拒");

    // J8 部分满不影响他者
    let j8 = {
        let mut h2 = BcHub::new();
        let _ = h2.subscribe(1, 1);
        let _ = h2.subscribe(2, 8);
        let _ = h2.broadcast_event(1, 1);
        let _ = h2.broadcast_event(1, 2);
        let snap = h2.snapshot();
        snap[0].2 == 1 && snap[1].1 == 2 && snap[1].2 == 0
    };
    s.add("J8-满不株连", j8, "满者丢计他者照收");

    // J9 满返值=实达数
    let j9 = {
        let mut h2 = BcHub::new();
        let _ = h2.subscribe(1, 1);
        let _ = h2.subscribe(2, 1);
        let _ = h2.broadcast_event(1, 1);
        h2.broadcast_event(1, 2) == Ok(0)
    };
    s.add("J9-实达数", j9, "全满时实达 0");

    // J10 快照结构
    let j10 = {
        let snap = h.snapshot();
        snap.len() == 2 && snap[0].0 == 1 && snap[1].0 == 2
    };
    s.add("J10-快照结构", j10, "id 序=订阅序");

    // J11 印后操作拒
    let j11 = {
        let mut h2 = mk_hub();
        h2.seal();
        h2.broadcast_event(1, 99).err() == Some(BC_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut h2 = mk_hub();
        let a = h2.fold();
        h2.seal();
        h2.fold() == a
    };
    s.add("J12-指纹稳定", j12, "印不改指纹");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24188_broadcast_flow() {
        let mut h = BcHub::new();
        assert_eq!(h.broadcast_event(1, 1), Err(BC_NO_SUB));
        assert_eq!(h.subscribe(1, 1), Ok(()));
        assert_eq!(h.subscribe(1, 1), Err(BC_DUP_SUB));
        assert_eq!(h.broadcast_event(9, 1), Err(BC_BAD_KIND));
        assert_eq!(h.broadcast_event(1, 1), Ok(1));
        assert_eq!(h.broadcast_event(1, 2), Ok(0));
        assert_eq!(h.snapshot(), alloc::vec![(1, 1, 1)]);
        h.seal();
        assert_eq!(h.broadcast_event(1, 3), Err(BC_FROZEN));
        assert_eq!(h.subscribe(2, 1), Err(BC_FROZEN));
    }

    #[test]
    fn unx24188_checks_green() {
        let s = run_unx24188_checks();
        assert!(s.all_passed(), "UNX-F24188 判据未全绿");
    }
}
