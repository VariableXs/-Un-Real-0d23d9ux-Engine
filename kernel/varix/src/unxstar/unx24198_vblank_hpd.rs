//! UNX-F24198 vblank/HPD 内核事件面与事件队列·子功能18（多显示器路由）（0x41AA0 段）
//!
//! 锚点：UNX-F24198-J1 路由账（管道投递逐条落账）；J2 路由语义（按
//! 管道独立序 + 野管道拒 + 零管道拒三性质）；J3 路由指纹（账折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（多显路由面）。
//!
//! 结构：路由器 + 管道表 + 投递核 + 账本 + 折叠指纹 + 注入核（野管
//! 道/零管道/野类型/印后/管道满必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41AA0 独占 0x20）。
pub const SEG_BASE: u32 = 0x41AA0;
pub const RT_BAD_PIPE: u32 = SEG_BASE + 0x01; // 野管道
pub const RT_NO_PIPE: u32 = SEG_BASE + 0x02; // 零管道
pub const RT_BAD_KIND: u32 = SEG_BASE + 0x03; // 野类型
pub const RT_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const RT_FULL_PIPE: u32 = SEG_BASE + 0x05; // 管道满

/// 单管道事件队列。
pub struct RtPipe {
    pub id: u64,
    pub cap: usize,
    buf: Vec<(u8, u64)>,
    pub routed: u64,
}

impl RtPipe {
    pub fn new(id: u64, cap: usize) -> RtPipe {
        RtPipe {
            id,
            cap,
            buf: Vec::new(),
            routed: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }
}

/// 路由器：事件按管道 id 投递。
pub struct RtRouter {
    pipes: Vec<RtPipe>,
    pub dispatched: u64,
    sealed: bool,
}

impl RtRouter {
    pub fn new() -> RtRouter {
        RtRouter {
            pipes: Vec::new(),
            dispatched: 0,
            sealed: false,
        }
    }

    /// 挂管道：id 重复拒。
    pub fn attach(&mut self, id: u64, cap: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(RT_FROZEN);
        }
        if self.pipes.iter().any(|p| p.id == id) {
            return Err(RT_BAD_PIPE);
        }
        self.pipes.push(RtPipe::new(id, cap));
        Ok(())
    }

    pub fn pipe_count(&self) -> usize {
        self.pipes.len()
    }

    /// 投递：野管道/零管道/野类型拒；管道满拒。
    pub fn route(&mut self, pipe_id: u64, kind: u8, ts: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(RT_FROZEN);
        }
        if pipe_id == 0 {
            return Err(RT_NO_PIPE);
        }
        if kind > 7 {
            return Err(RT_BAD_KIND);
        }
        let p = match self.pipes.iter_mut().find(|p| p.id == pipe_id) {
            Some(p) => p,
            None => return Err(RT_BAD_PIPE),
        };
        if p.len() >= p.cap {
            return Err(RT_FULL_PIPE);
        }
        p.buf.push((kind, ts));
        p.routed += 1;
        self.dispatched += 1;
        Ok(())
    }

    /// 管道快照 (id, len, routed)。
    pub fn snapshot(&self) -> Vec<(u64, usize, u64)> {
        self.pipes.iter().map(|p| (p.id, p.len(), p.routed)).collect()
    }

    /// 全管道清空（垂直消隐同步用）。
    pub fn flush_all(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(RT_FROZEN);
        }
        let mut n = 0;
        for p in self.pipes.iter_mut() {
            n += p.buf.len();
            p.buf.clear();
        }
        Ok(n)
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41AA0;
        h = h.wrapping_mul(31).wrapping_add(self.dispatched as u32);
        for p in &self.pipes {
            h = h.wrapping_mul(31).wrapping_add(p.id as u32);
            h = h.wrapping_mul(31).wrapping_add(p.len() as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：三管道（1/2/3 容 2/2/1），路由 5 次清 1 次。
pub fn mk_router() -> RtRouter {
    let mut r = RtRouter::new();
    let _ = r.attach(1, 2);
    let _ = r.attach(2, 2);
    let _ = r.attach(3, 1);
    let _ = r.route(1, 1, 10);
    let _ = r.route(1, 2, 20);
    let _ = r.route(2, 1, 30);
    let _ = r.route(2, 1, 40);
    let _ = r.route(3, 3, 50);
    let _ = r.flush_all();
    let _ = r.route(1, 1, 60);
    r
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 零管道
    got[0] = RtRouter::new().route(0, 1, 1).err() == Some(RT_NO_PIPE);
    // 2) 野管道
    let mut r = RtRouter::new();
    let _ = r.attach(5, 4);
    got[1] = r.route(9, 1, 1).err() == Some(RT_BAD_PIPE);
    // 3) 重复挂管道
    let mut r2 = RtRouter::new();
    let _ = r2.attach(5, 4);
    got[2] = r2.attach(5, 4).err() == Some(RT_BAD_PIPE);
    // 4) 管道满
    let mut r3 = RtRouter::new();
    let _ = r3.attach(1, 1);
    let _ = r3.route(1, 1, 1);
    got[3] = r3.route(1, 1, 2).err() == Some(RT_FULL_PIPE);
    // 5) 印后操作
    let mut r4 = mk_router();
    r4.seal();
    got[4] = r4.route(1, 1, 99).err() == Some(RT_FROZEN)
        && r4.attach(9, 9).err() == Some(RT_FROZEN)
        && r4.flush_all().err() == Some(RT_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24198_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24198");
    let mut r = mk_router();

    // J1 管道数
    let j1 = r.pipe_count() == 3;
    s.add("J1-管道数", j1, "3 管道");

    // J2 独立序：管道 1 余 1（flush 后又入 1）
    let j2 = r.snapshot()[0].1 == 1;
    s.add("J2-独立序", j2, "管道 1 余 1");

    // J3 全清：flush 计 5
    let j3 = r.dispatched == 6; // 5 投 + flush 后 1
    s.add("J3-投递计数", j3, "累计 6 投");

    // J4 零管道拒
    let j4 = r.route(0, 1, 1).err() == Some(RT_NO_PIPE);
    s.add("J4-零管道拒", j4, "id=0 必拒");

    // J5 野管道拒
    let j5 = r.route(9, 1, 1).err() == Some(RT_BAD_PIPE);
    s.add("J5-野管道拒", j5, "未挂必拒");

    // J6 野类型拒
    let j6 = r.route(1, 8, 1).err() == Some(RT_BAD_KIND);
    s.add("J6-野类型拒", j6, "kind>7 必拒");

    // J7 重复挂拒
    let j7 = r.attach(1, 4).err() == Some(RT_BAD_PIPE);
    s.add("J7-重挂拒", j7, "同 id 重挂必拒");

    // J8 管道满拒
    let j8 = {
        let mut r2 = RtRouter::new();
        let _ = r2.attach(1, 1);
        let _ = r2.route(1, 1, 1);
        r2.route(1, 1, 2).err() == Some(RT_FULL_PIPE)
    };
    s.add("J8-满管拒", j8, "超容必拒");

    // J9 清空
    let j9 = r.flush_all() == Ok(1);
    s.add("J9-清空", j9, "flush 归零");

    // J10 管道路由计数
    let j10 = {
        let mut r2 = RtRouter::new();
        let _ = r2.attach(1, 8);
        let _ = r2.route(1, 1, 1);
        let _ = r2.route(1, 1, 2);
        r2.snapshot()[0].2 == 2
    };
    s.add("J10-路数", j10, "管道路由计数 2");

    // J11 印后操作拒
    let j11 = {
        let mut r2 = mk_router();
        r2.seal();
        r2.route(1, 1, 99).err() == Some(RT_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut r2 = mk_router();
        let a = r2.fold();
        r2.seal();
        r2.fold() == a
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
    fn unx24198_route_flow() {
        let mut r = RtRouter::new();
        assert_eq!(r.route(1, 1, 1), Err(RT_NO_PIPE));
        assert_eq!(r.attach(1, 1), Ok(()));
        assert_eq!(r.attach(1, 1), Err(RT_BAD_PIPE));
        assert_eq!(r.route(1, 8, 1), Err(RT_BAD_KIND));
        assert_eq!(r.route(1, 1, 1), Ok(()));
        assert_eq!(r.route(1, 1, 2), Err(RT_FULL_PIPE));
        assert_eq!(r.flush_all(), Ok(1));
        assert_eq!(r.route(2, 1, 1), Err(RT_BAD_PIPE));
        r.seal();
        assert_eq!(r.route(1, 1, 9), Err(RT_FROZEN));
        assert_eq!(r.flush_all(), Err(RT_FROZEN));
        assert_eq!(r.attach(3, 1), Err(RT_FROZEN));
    }

    #[test]
    fn unx24198_checks_green() {
        let s = run_unx24198_checks();
        assert!(s.all_passed(), "UNX-F24198 判据未全绿");
    }
}
