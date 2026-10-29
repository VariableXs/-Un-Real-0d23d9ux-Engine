//! UNX-F10894 pending 错误队列出队序矩阵（0x1c840 段）
//! 锚点：UNX-F10894-J1 pending 错误队列（FIFO 出队+错误先行数据面）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c840;
pub const E_EAGAIN: u32 = SEG_BASE + 1;
pub const E_EBADF: u32 = SEG_BASE + 2;
pub const E_EINVAL: u32 = SEG_BASE + 3;
pub const E_OK: u32 = SEG_BASE + 4;
pub const E_SEALED: u32 = SEG_BASE + 5;

/// 队列元素：错误码或数据
#[derive(Clone, Debug, PartialEq)]
pub enum PendingItem {
    Err(u32),
    Data(Vec<u8>),
}

impl PendingItem {
    pub fn is_err(&self) -> bool {
        matches!(self, PendingItem::Err(_))
    }
}

/// pending 队列：错误与数据混排 FIFO（SO_ERROR 读首错不清队）
pub struct PendingQueue {
    items: Vec<PendingItem>,
    open: bool,
    sealed: bool,
}

impl PendingQueue {
    pub fn new() -> Self {
        PendingQueue { items: Vec::new(), open: true, sealed: false }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn push(&mut self, it: PendingItem) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !self.open {
            return Err(E_EBADF);
        }
        self.items.push(it);
        Ok(())
    }

    pub fn depth(&self) -> usize {
        self.items.len()
    }

    /// 出队：空 → EAGAIN；首元素原样出
    pub fn pop(&mut self) -> Result<PendingItem, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if self.items.is_empty() {
            return Err(E_EAGAIN);
        }
        Ok(self.items.remove(0))
    }

    /// SO_ERROR：窥首错不移除；首为数据 → Ok(None)
    pub fn so_error(&self) -> Result<Option<u32>, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if self.items.is_empty() {
            return Ok(None);
        }
        match &self.items[0] {
            PendingItem::Err(e) => Ok(Some(*e)),
            PendingItem::Data(_) => Ok(None),
        }
    }

    /// 错误先行判定：队首错先于其后数据出
    pub fn err_first_probe(&self) -> bool {
        if self.items.len() < 2 {
            return false;
        }
        self.items[0].is_err() && !self.items[1].is_err()
    }

    pub fn drain_count(&mut self) -> usize {
        let mut n = 0;
        while self.pop().is_ok() {
            n += 1;
        }
        n
    }
}

pub fn run_unx10894_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10894");
    s.add("J1-段锚", SEG_BASE == 0x1c840, "pending 队列段基址 0x1c840");
    s.add("J2-空队 pop EAGAIN", {
        let mut q = PendingQueue::new();
        q.pop() == Err(E_EAGAIN)
    }, "空队 E_EAGAIN");
    s.add("J3-FIFO 出序", {
        let mut q = PendingQueue::new();
        q.push(PendingItem::Data(b"a".to_vec())).unwrap();
        q.push(PendingItem::Data(b"b".to_vec())).unwrap();
        q.pop() == Ok(PendingItem::Data(b"a".to_vec()))
            && q.pop() == Ok(PendingItem::Data(b"b".to_vec()))
    }, "a 先 b 后");
    s.add("J4-错误先行", {
        let mut q = PendingQueue::new();
        q.push(PendingItem::Err(7)).unwrap();
        q.push(PendingItem::Data(b"x".to_vec())).unwrap();
        q.err_first_probe() && q.pop() == Ok(PendingItem::Err(7))
    }, "首错先出");
    s.add("J5-数据先行 SO_ERROR 空", {
        let mut q = PendingQueue::new();
        q.push(PendingItem::Data(b"x".to_vec())).unwrap();
        q.push(PendingItem::Err(9)).unwrap();
        q.so_error() == Ok(None)
    }, "首数据窥错 None");
    s.add("J6-SO_ERROR 不移除", {
        let mut q = PendingQueue::new();
        q.push(PendingItem::Err(5)).unwrap();
        let a = q.so_error();
        let b = q.so_error();
        a == Ok(Some(5)) && b == Ok(Some(5)) && q.depth() == 1
    }, "双窥同错深度 1");
    s.add("J7-pop 清队首", {
        let mut q = PendingQueue::new();
        q.push(PendingItem::Err(5)).unwrap();
        q.pop().unwrap();
        q.so_error() == Ok(None) && q.depth() == 0
    }, "pop 后窥错 None");
    s.add("J8-空队 SO_ERROR None", {
        let q = PendingQueue::new();
        q.so_error() == Ok(None)
    }, "空队窥错 None");
    s.add("J9-混排出序", {
        let mut q = PendingQueue::new();
        q.push(PendingItem::Data(b"1".to_vec())).unwrap();
        q.push(PendingItem::Err(3)).unwrap();
        q.push(PendingItem::Data(b"2".to_vec())).unwrap();
        let a = q.pop().unwrap();
        let b = q.pop().unwrap();
        let c = q.pop().unwrap();
        a == PendingItem::Data(b"1".to_vec())
            && b == PendingItem::Err(3)
            && c == PendingItem::Data(b"2".to_vec())
    }, "数-错-数全序");
    s.add("J10-关闭后拒", {
        let mut q = PendingQueue::new();
        q.push(PendingItem::Data(b"x".to_vec())).unwrap();
        q.close();
        q.pop() == Err(E_EBADF) && q.so_error() == Err(E_EBADF)
    }, "关后 pop/窥皆 E_EBADF");
    s.add("J11-关后 push 拒", {
        let mut q = PendingQueue::new();
        q.close();
        q.push(PendingItem::Err(1)) == Err(E_EBADF)
    }, "关后推 E_EBADF");
    s.add("J12-封账拒推", {
        let mut q = PendingQueue::new();
        q.seal();
        q.push(PendingItem::Err(1)) == Err(E_SEALED)
    }, "封账 E_SEALED");
    s.add("J13-排空计数", {
        let mut q = PendingQueue::new();
        q.push(PendingItem::Err(1)).unwrap();
        q.push(PendingItem::Data(b"2".to_vec())).unwrap();
        q.push(PendingItem::Err(3)).unwrap();
        q.drain_count() == 3 && q.depth() == 0
    }, "三元素排空计 3");
    s.add("J14-排空后 EAGAIN", {
        let mut q = PendingQueue::new();
        q.push(PendingItem::Err(1)).unwrap();
        q.drain_count();
        q.pop() == Err(E_EAGAIN)
    }, "排空后 E_EAGAIN");
    s.add("J15-错误码透传", {
        let mut q = PendingQueue::new();
        q.push(PendingItem::Err(E_EINVAL)).unwrap();
        match q.pop().unwrap() {
            PendingItem::Err(e) => e == E_EINVAL,
            _ => false,
        }
    }, "E_EINVAL 码透传");
    s.add_dyn("J16-窥错出清账", {
        let mut q = PendingQueue::new();
        q.push(PendingItem::Err(11)).unwrap();
        q.push(PendingItem::Data(b"d".to_vec())).unwrap();
        let e1 = q.so_error();
        let p = q.pop();
        let e2 = q.so_error();
        e1 == Ok(Some(11)) && p.is_ok() && e2 == Ok(None)
    }, "Some(11)-出清-None 账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_pending_queue() {
        let mut q = PendingQueue::new();
        q.push(PendingItem::Err(7)).unwrap();
        q.push(PendingItem::Data(b"x".to_vec())).unwrap();
        assert_eq!(q.so_error(), Ok(Some(7)));
        assert_eq!(q.pop(), Ok(PendingItem::Err(7)));
        assert_eq!(q.so_error(), Ok(None));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10894_checks();
        assert!(s.all_passed(), "UNX-F10894 判据全绿");
    }
}
