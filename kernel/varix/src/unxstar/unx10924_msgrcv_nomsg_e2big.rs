//! UNX-F10924 msgrcv ENOMSG/E2BIG 矩阵（0x1d300 段）
//!
//! 锚点：UNX-F10924-J1 正面：无匹配 NOWAIT→ENOMSG（10/10）、超长
//! E2BIG（10/10）；反面：匹配消息零误报（千次 10/10）。
//! 域：CoRun·UNX-B27（SysV 错误矩阵域·msgrcv 面）。
//!
//! 结构：收取账（mtype 路由 + msgmax 缓冲账 + NOWAIT 位）+ 裁断器
//! （无匹配→ENOMSG、缓冲不足→E2BIG、匹配全通）+ 注入核（空队/
//! 型号错配/缓冲不足/冻结改账四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d300 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d300;
pub const RC_ENOMSG: u32 = SEG_BASE + 0x01; // 无匹配
pub const RC_E2BIG: u32 = SEG_BASE + 0x02; // 超长
pub const RC_EINVAL: u32 = SEG_BASE + 0x03; // 型号零非法
pub const RC_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改账
pub const RC_EAGAIN: u32 = SEG_BASE + 0x05; // 空队

/// IPC_NOWAIT 位。
pub const F_NOWAIT: u32 = 0x1;
/// 缓冲上限。
pub const BUFMAX: u32 = 48;
/// 账容量。
pub const CAP: usize = 8;
/// RC 域标（fold 种子）。
pub const RC_TAG: u64 = 0x1d30218;

/// 收取账条目。
#[derive(Clone, Debug)]
pub struct RcvMsg {
    pub mtype: u64,
    pub size: u32,
}

impl RcvMsg {
    pub fn render(&self) -> String {
        format!("R{}|{}", self.mtype, self.size)
    }
}

/// msgrcv 裁断账。
pub struct RcvBoard {
    pub msgs: Vec<RcvMsg>,
    pub frozen: bool,
    pub got: u32,
}

impl RcvBoard {
    pub fn new() -> RcvBoard {
        RcvBoard { msgs: Vec::new(), frozen: false, got: 0 }
    }

    /// 发入。
    pub fn put(&mut self, mtype: u64, size: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(RC_FROZEN);
        }
        if mtype == 0 || size == 0 {
            return Err(RC_EINVAL);
        }
        if self.msgs.len() >= CAP {
            return Err(RC_EINVAL);
        }
        self.msgs.push(RcvMsg { mtype, size });
        Ok(())
    }

    /// 首个 mtype 匹配条目下标。
    fn find(&self, mtype: u64) -> Option<usize> {
        let mut k = 0usize;
        while k < self.msgs.len() {
            if self.msgs[k].mtype == mtype {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// msgrcv 主裁断：无匹配→ENOMSG（NOWAIT）、缓冲不足→E2BIG、
    /// 匹配收取。
    pub fn get(&mut self, mtype: u64, bufmax: u32, flags: u32) -> Result<(u64, u32), u32> {
        if self.frozen {
            return Err(RC_FROZEN);
        }
        if mtype == 0 {
            return Err(RC_EINVAL);
        }
        match self.find(mtype) {
            None => {
                if flags & F_NOWAIT != 0 {
                    return Err(RC_ENOMSG);
                }
                Err(RC_EAGAIN)
            }
            Some(idx) => {
                let sz = self.msgs[idx].size;
                if sz > bufmax {
                    return Err(RC_E2BIG);
                }
                let mt = self.msgs[idx].mtype;
                self.msgs.remove(idx);
                self.got += 1;
                Ok((mt, sz))
            }
        }
    }

    /// 类型错配审计：目标型无匹配而旁型存在。
    pub fn mismatch_exists(&self, mtype: u64) -> bool {
        self.find(mtype).is_none() && !self.msgs.is_empty()
    }

    /// 链式 fold 指纹（序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h = RC_TAG;
        let mut k = 0usize;
        while k < self.msgs.len() {
            let m = &self.msgs[k];
            h = (h ^ m.mtype).wrapping_mul(0x100000001b3);
            h = (h ^ m.size as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.got as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 空队 NOWAIT 必报 ENOMSG
    let mut b = RcvBoard::new();
    got[0] = b.get(5, BUFMAX, F_NOWAIT).err() == Some(RC_ENOMSG);
    // 2) 型号错配必报 ENOMSG 且旁型在账
    let mut m = RcvBoard::new();
    let _ = m.put(7, 8);
    got[1] = m.get(9, BUFMAX, F_NOWAIT).err() == Some(RC_ENOMSG) && m.mismatch_exists(9);
    // 3) 缓冲不足必报 E2BIG 且消息不消费
    let mut e = RcvBoard::new();
    let _ = e.put(11, 32);
    let before = e.msgs.len();
    got[2] = e.get(11, 8, F_NOWAIT).err() == Some(RC_E2BIG) && e.msgs.len() == before;
    // 4) 冻结后收取必抓
    let mut f = RcvBoard::new();
    f.freeze();
    got[3] = f.get(1, BUFMAX, F_NOWAIT).err() == Some(RC_FROZEN);
    got
}

/// 型号确定性样本。
pub fn rc_sample_type(r: u32) -> u64 {
    r.wrapping_mul(2654435761).wrapping_add(47) as u64 % 50 + 1
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10924_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10924");

    // J1 正面·ENOMSG 十连
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut b = RcvBoard::new();
        let _ = b.put(rc_sample_type(r), 8);
        if b.get(rc_sample_type(r) + 100, BUFMAX, F_NOWAIT).err() == Some(RC_ENOMSG) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-ENOMSG十连", h1 == 10, "型号错配十轮全数 ENOMSG");

    // J2 正面·E2BIG 十连
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut b = RcvBoard::new();
        let _ = b.put(rc_sample_type(r2), BUFMAX + r2 + 1);
        if b.get(rc_sample_type(r2), BUFMAX, F_NOWAIT).err() == Some(RC_E2BIG) {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-E2BIG十连", h2 == 10, "缓冲不足十轮全数 E2BIG");

    // J3 反面：匹配消息千次零误报
    let mut ok = 0u32;
    let mut r3 = 0u32;
    while r3 < 1000 {
        let mut b = RcvBoard::new();
        let t = rc_sample_type(r3);
        let _ = b.put(t, 8);
        let got = b.get(t, BUFMAX, F_NOWAIT);
        let pass = match got {
            Ok((mt, sz)) => mt == t && sz == 8,
            Err(_) => false,
        };
        if pass {
            ok += 1;
        }
        r3 += 1;
    }
    s.add("J3-匹配千次零误报", ok == 1000, "匹配收取千轮型号尺寸双对");

    // J4 E2BIG 后消息留存可小缓冲再收
    let j4 = {
        let mut b = RcvBoard::new();
        let _ = b.put(21, 32);
        let _ = b.get(21, 8, F_NOWAIT);
        b.get(21, 32, F_NOWAIT).is_ok()
    };
    s.add("J4-E2BIG留存", j4, "E2BIG 后消息留存可再收");

    // J5 收取账逐次累积
    let j5 = {
        let mut b = RcvBoard::new();
        let _ = b.put(22, 8);
        let _ = b.put(22, 8);
        let _ = b.get(22, BUFMAX, F_NOWAIT);
        let _ = b.get(22, BUFMAX, F_NOWAIT);
        b.got == 2 && b.msgs.is_empty()
    };
    s.add("J5-收取账累积", j5, "两次收取账记二且清空");

    // J6 注入四路全抓
    let inj = inject_four();
    s.add("J6-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "空队/错配/不足/冻结全数命中");

    // J7 型号零拒收
    let j7 = {
        let mut b = RcvBoard::new();
        b.put(0, 8).err() == Some(RC_EINVAL) && b.get(0, BUFMAX, F_NOWAIT).err() == Some(RC_EINVAL)
    };
    s.add("J7-型号零拒", j7, "型号零登记与收取均必拒");

    // J8 空队无 NOWAIT 返 EAGAIN
    let j8 = RcvBoard::new().get(30, BUFMAX, 0).err() == Some(RC_EAGAIN);
    s.add("J8-空队EAGAIN", j8, "空队阻塞路径显性 EAGAIN");

    // J9 账容量边界
    let j9 = {
        let mut b = RcvBoard::new();
        let mut i = 0u32;
        let mut full = true;
        while i < CAP as u32 {
            if b.put(i as u64 + 1, 4).is_err() {
                full = false;
            }
            i += 1;
        }
        full && b.put(999, 4).err() == Some(RC_EINVAL)
    };
    s.add("J9-容量边界", j9, "超容登记必拒且原账不损");

    // J10 冻结后收取/发入双拒
    let j10 = {
        let mut b = RcvBoard::new();
        b.freeze();
        b.put(31, 8).err() == Some(RC_FROZEN) && b.get(31, BUFMAX, F_NOWAIT).err() == Some(RC_FROZEN)
    };
    s.add("J10-冻结双拒", j10, "冻结后发入与收取均必拒");

    // J11 渲染面：型号尺寸渲染可区分
    let j11 = {
        let a = RcvMsg { mtype: 1, size: 8 };
        let b = RcvMsg { mtype: 2, size: 8 };
        a.render() != b.render()
    };
    s.add("J11-渲染分旗", j11, "mtype 渲染可区分");

    // J12 fold 稳定：收取改变指纹、同态一致
    let j12 = {
        let mut b = RcvBoard::new();
        let _ = b.put(32, 8);
        let f0 = b.fold();
        let _ = b.get(32, BUFMAX, F_NOWAIT);
        let f1 = b.fold();
        let f2 = b.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J12-指纹稳定", j12, "收取代价入指纹且同态稳定");

    // J13 动态明细：千次匹配计数落账
    s.add_dyn("J13-千次明细", ok == 1000, &format!("match_ok={}/1000", ok));

    // J14 动态明细：双十连计数落账
    s.add_dyn("J14-十连明细", h1 == 10 && h2 == 10, &format!("enomsg={} e2big={}", h1, h2));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10924_rcvboard_flow() {
        let mut b = RcvBoard::new();
        assert!(b.put(7, 8).is_ok());
        assert!(b.put(0, 8).err() == Some(RC_EINVAL));
        assert!(b.get(9, BUFMAX, F_NOWAIT).err() == Some(RC_ENOMSG));
        assert!(b.get(7, 4, F_NOWAIT).err() == Some(RC_E2BIG));
        assert_eq!(b.get(7, BUFMAX, F_NOWAIT), Ok((7u64, 8u32)));
        assert!(b.get(7, BUFMAX, F_NOWAIT).err() == Some(RC_ENOMSG));
        b.freeze();
        assert!(b.get(7, BUFMAX, F_NOWAIT).err() == Some(RC_FROZEN));
    }

    #[test]
    fn unx10924_checks_green() {
        let s = run_unx10924_checks();
        assert!(s.all_passed(), "UNX-F10924 判据未全绿");
    }
}
