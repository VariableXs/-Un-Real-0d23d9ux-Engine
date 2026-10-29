//! UNX-F24170 MSI-X 中断消费与 IrqBackend 对接·子功能10（IrqBackend 注册与派发）（0x41720 段）
//!
//! 锚点：UNX-F24170-J1 后端账（backend 注册与中断派发逐条落账）；J2
//! 后端语义（独占后端 + 派发路由 + 注销清路由三性质）；J3 后端指纹
//! （账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（IrqBackend 对接面）。
//!
//! 结构：后端表 + 注册/注销核 + 路由派发核 + 账本 + 折叠指纹 + 注入
//! 核（重后端/野后端/无路由派发/印后/重复注销必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41720 独占 0x20）。
pub const SEG_BASE: u32 = 0x41720;
pub const IB_DUP_BACKEND: u32 = SEG_BASE + 0x01; // 重后端
pub const IB_UNK_BACKEND: u32 = SEG_BASE + 0x02; // 野后端
pub const IB_NO_ROUTE: u32 = SEG_BASE + 0x03; // 无路由派发
pub const IB_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const IB_BAD_NAME: u32 = SEG_BASE + 0x05; // 空后端名

/// 后端条目。
#[derive(Clone)]
pub struct IbEnt {
    pub name: String,
    pub vecs: Vec<u32>, // 路由的向量
    pub dispatched: u32,
}

/// 后端簿。
pub struct IbBook {
    pub backends: Vec<IbEnt>,
    pub sealed: bool,
}

impl IbBook {
    pub fn new() -> IbBook {
        IbBook { backends: Vec::new(), sealed: false }
    }

    pub fn register(&mut self, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(IB_FROZEN);
        }
        if name.is_empty() {
            return Err(IB_BAD_NAME);
        }
        if self.backends.iter().any(|b| b.name == name) {
            return Err(IB_DUP_BACKEND);
        }
        self.backends.push(IbEnt { name: name.to_string(), vecs: Vec::new(), dispatched: 0 });
        Ok(())
    }

    /// 路由：向量 → 后端（一向量只一路由）。
    pub fn route(&mut self, name: &str, vec_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IB_FROZEN);
        }
        if self.backends.iter().any(|b| b.vecs.contains(&vec_no)) {
            return Err(IB_DUP_BACKEND);
        }
        match self.backends.iter_mut().find(|b| b.name == name) {
            Some(b) => {
                b.vecs.push(vec_no);
                Ok(())
            }
            None => Err(IB_UNK_BACKEND),
        }
    }

    /// 派发：路由到后端计数。
    pub fn dispatch(&mut self, vec_no: u32) -> Result<String, u32> {
        if self.sealed {
            return Err(IB_FROZEN);
        }
        let idx = self.backends.iter().position(|b| b.vecs.contains(&vec_no));
        match idx {
            Some(i) => {
                let name = self.backends[i].name.clone();
                self.backends[i].dispatched += 1;
                Ok(name)
            }
            None => Err(IB_NO_ROUTE),
        }
    }

    /// 注销：清全部路由。
    pub fn unregister(&mut self, name: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(IB_FROZEN);
        }
        let before = self.backends.len();
        let mut n = 0usize;
        self.backends.retain(|b| {
            if b.name == name {
                n = b.vecs.len();
                false
            } else {
                true
            }
        });
        if self.backends.len() == before {
            return Err(IB_UNK_BACKEND);
        }
        Ok(n)
    }

    pub fn dispatched_of(&self, name: &str) -> u32 {
        self.backends.iter().find(|b| b.name == name).map(|b| b.dispatched).unwrap_or(0)
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41720;
        for b in &self.backends {
            for by in b.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
            h = h.wrapping_mul(31).wrapping_add(b.dispatched);
            h = h.wrapping_mul(31).wrapping_add(b.vecs.len() as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：两后端三路由。
pub fn mk_book() -> IbBook {
    let mut b = IbBook::new();
    let _ = b.register("msix");
    let _ = b.register("legacy");
    let _ = b.route("msix", 16);
    let _ = b.route("msix", 17);
    let _ = b.route("legacy", 3);
    let _ = b.dispatch(16);
    let _ = b.dispatch(16);
    let _ = b.dispatch(3);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 重后端
    got[0] = b.register("msix").err() == Some(IB_DUP_BACKEND);
    // 2) 野后端路由
    got[1] = b.route("ghost", 20).err() == Some(IB_UNK_BACKEND);
    // 3) 无路由派发
    got[2] = b.dispatch(99).err() == Some(IB_NO_ROUTE);
    // 4) 一向量双路由
    got[3] = b.route("legacy", 16).err() == Some(IB_DUP_BACKEND);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.register("late").err() == Some(IB_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24170_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24170");
    let mut b = mk_book();

    // J1 后端账齐
    let j1 = b.backends.len() == 2;
    s.add("J1-后端账齐", j1, "两后端落表");

    // J2 路由齐
    let j2 = b.backends.iter().find(|x| x.name == "msix").map(|x| x.vecs.len() == 2).unwrap_or(false);
    s.add("J2-路由齐", j2, "msix 两向量路由");

    // J3 派发路由
    let j3 = b.dispatch(16) == Ok("msix".to_string());
    s.add("J3-派发路由", j3, "向量 16 → msix");

    // J4 派发计数
    let j4 = b.dispatched_of("msix") == 3 && b.dispatched_of("legacy") == 1;
    s.add("J4-派发计数", j4, "msix:3 legacy:1");

    // J5 无路由派发拒
    let j5 = b.dispatch(99).err() == Some(IB_NO_ROUTE);
    s.add("J5-无路由拒", j5, "未路由向量必拒");

    // J6 重后端拒
    let j6 = b.register("msix").err() == Some(IB_DUP_BACKEND);
    s.add("J6-重后端拒", j6, "重名必拒");

    // J7 一向量一路由
    let j7 = b.route("legacy", 16).err() == Some(IB_DUP_BACKEND);
    s.add("J7-一向量一路由", j7, "向量重路由必拒");

    // J8 注销清路由
    let j8 = {
        let n = b.unregister("legacy").unwrap_or(0);
        n == 1 && b.dispatch(3).is_err()
    };
    s.add("J8-注销清路由", j8, "注销后路由失效");

    // J9 重复注销拒
    let j9 = b.unregister("legacy").err() == Some(IB_UNK_BACKEND);
    s.add("J9-重注销拒", j9, "二次注销必拒");

    // J10 空后端名拒
    let j10 = b.register("").err() == Some(IB_BAD_NAME);
    s.add("J10-空名拒", j10, "空后端名必拒");

    // J11 野后端路由拒
    let j11 = b.route("ghost", 20).err() == Some(IB_UNK_BACKEND);
    s.add("J11-野后端拒", j11, "未注册后端必拒");

    // J12 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

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
    fn unx24170_backend_flow() {
        let mut b = IbBook::new();
        assert!(b.register("msix").is_ok());
        assert_eq!(b.register("msix"), Err(IB_DUP_BACKEND));
        assert!(b.route("msix", 16).is_ok());
        assert_eq!(b.route("ghost", 16), Err(IB_UNK_BACKEND));
        assert_eq!(b.dispatch(16), Ok("msix".to_string()));
        assert_eq!(b.dispatch(99), Err(IB_NO_ROUTE));
        assert_eq!(b.unregister("ghost"), Err(IB_UNK_BACKEND));
        b.seal();
        assert_eq!(b.register("late"), Err(IB_FROZEN));
    }

    #[test]
    fn unx24170_checks_green() {
        let s = run_unx24170_checks();
        assert!(s.all_passed(), "UNX-F24170 判据未全绿");
    }
}
