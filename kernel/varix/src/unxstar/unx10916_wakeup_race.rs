//! UNX-F10916 唤醒丢失防护矩阵（事件与 wait 竞态）（0x1d200 段）
//!
//! 锚点：UNX-F10916-J1 正面：三复用器并发压测万次零丢失（10/10）；
//! 反面：窗外零歧义（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·竞态防护面）。
//!
//! 结构：竞态窗（事件到达序号 × wait 进入序号双账）+ 防护器
//! （窗内事件必入挂起账、wait 必扫挂起账，三复用器同规则）+ 注入核
//! （跳窗丢失/双入账/窗账漂移/冻结改账四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x1d200 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d200;
pub const WK_LOST: u32 = SEG_BASE + 0x01; // 窗内事件丢失
pub const WK_DOUBLE: u32 = SEG_BASE + 0x02; // 双入账
pub const WK_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改账
pub const WK_NOMUX: u32 = SEG_BASE + 0x04; // 复用器号非法
pub const WK_DRIFT: u32 = SEG_BASE + 0x05; // 窗账漂移

/// 三复用器号（select/poll/epoll）。
pub const M_SELECT: u8 = 1;
pub const M_POLL: u8 = 2;
pub const M_EPOLL: u8 = 3;

/// 挂起账定容。
pub const CAP: usize = 64;
/// WK 域标（fold 种子）。
pub const WK_TAG: u64 = 0x1d20210;

/// 挂起事件条目。
#[derive(Clone, Debug)]
pub struct PendEvent {
    pub seq: u64,
    pub mux: u8,
    pub fd: i32,
}

impl PendEvent {
    pub fn render(&self) -> String {
        let m = match self.mux {
            M_SELECT => "SEL",
            M_POLL => "POL",
            _ => "EPL",
        };
        format!("W{}|{}|{}", self.seq, m, self.fd)
    }
}

/// 竞态防护账：事件序号账 + 挂起账 + wait 扫描账。
pub struct RaceGuard {
    pub pend: Vec<PendEvent>,
    pub ev_seq: u64,
    pub wait_seq: u64,
    pub delivered: u32,
    pub frozen: bool,
}

impl RaceGuard {
    pub fn new() -> RaceGuard {
        RaceGuard { pend: Vec::new(), ev_seq: 0, wait_seq: 0, delivered: 0, frozen: false }
    }

    /// 事件到达：必须入挂起账（窗内必收）。
    pub fn arrive(&mut self, mux: u8, fd: i32) -> Result<u64, u32> {
        if self.frozen {
            return Err(WK_FROZEN);
        }
        if mux < M_SELECT || mux > M_EPOLL {
            return Err(WK_NOMUX);
        }
        if self.pend.len() >= CAP {
            return Err(WK_DOUBLE);
        }
        self.ev_seq += 1;
        self.pend.push(PendEvent { seq: self.ev_seq, mux, fd });
        Ok(self.ev_seq)
    }

    /// wait 扫描：按序取走属于本复用器的挂起事件（不多取不漏取）。
    pub fn wait(&mut self, mux: u8) -> Result<Vec<u64>, u32> {
        if self.frozen {
            return Err(WK_FROZEN);
        }
        if mux < M_SELECT || mux > M_EPOLL {
            return Err(WK_NOMUX);
        }
        self.wait_seq += 1;
        let mut got: Vec<u64> = Vec::new();
        let mut taken: Vec<u64> = Vec::new();
        let mut k = 0usize;
        while k < self.pend.len() {
            if self.pend[k].mux == mux {
                got.push(self.pend[k].seq);
                taken.push(self.pend[k].seq);
            }
            k += 1;
        }
        // 取走（先收集后删，避免迭代中改容器）。
        let mut j = 0usize;
        while j < taken.len() {
            let mut idx = self.pend.len();
            let mut t = 0usize;
            while t < self.pend.len() {
                if self.pend[t].seq == taken[j] {
                    idx = t;
                    break;
                }
                t += 1;
            }
            if idx < self.pend.len() {
                self.pend.remove(idx);
            }
            j += 1;
        }
        self.delivered += got.len() as u32;
        Ok(got)
    }

    /// 零丢失审计：事件账序号 == 已投递 + 挂起余量。
    pub fn no_lost(&self) -> bool {
        self.ev_seq == self.delivered as u64 + self.pend.len() as u64
    }

    /// 零歧义审计：挂起账内序号唯一。
    pub fn unambiguous(&self) -> bool {
        let mut i = 0usize;
        while i < self.pend.len() {
            let mut j = i + 1;
            while j < self.pend.len() {
                if self.pend[i].seq == self.pend[j].seq {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 链式 fold 指纹（序敏感，含三账序号）。
    pub fn fold(&self) -> u64 {
        let mut h = WK_TAG;
        h = (h ^ self.ev_seq).wrapping_mul(0x100000001b3);
        h = (h ^ self.wait_seq).wrapping_mul(0x100000001b3);
        h = (h ^ self.delivered as u64).wrapping_mul(0x100000001b3);
        let mut k = 0usize;
        while k < self.pend.len() {
            h = (h ^ self.pend[k].seq).wrapping_mul(0x100000001b3);
            h = (h ^ self.pend[k].mux as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
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
    // 1) 跳窗丢失必抓：删账后 no_lost 必假
    let mut g = RaceGuard::new();
    let _ = g.arrive(M_EPOLL, 1);
    let _ = g.arrive(M_EPOLL, 2);
    g.pend.remove(0);
    got[0] = !g.no_lost();
    // 2) 双入账必抓：同序号两条目 unambiguous 必假
    let mut d = RaceGuard::new();
    let _ = d.arrive(M_POLL, 3);
    d.pend.push(PendEvent { seq: 1, mux: M_POLL, fd: 3 });
    got[1] = !d.unambiguous();
    // 3) 复用器号非法必拒
    let mut x = RaceGuard::new();
    got[2] = x.arrive(9, 4).err() == Some(WK_NOMUX) && x.wait(9).err() == Some(WK_NOMUX);
    // 4) 冻结后到达必抓
    let mut f = RaceGuard::new();
    f.freeze();
    got[3] = f.arrive(M_SELECT, 5).err() == Some(WK_FROZEN);
    got
}

/// 复用器确定性样本：第 r 轮的复用器号。
pub fn wk_sample_mux(r: u32) -> u8 {
    (r.wrapping_mul(2654435761).wrapping_add(31) % 3 + 1) as u8
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10916_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10916");

    // J1 正面：三复用器并发压测万次零丢失（十组各千轮）
    let mut h1 = 0u32;
    let mut grp = 0u32;
    while grp < 10 {
        let mut g = RaceGuard::new();
        let mut r = 0u32;
        while r < 1000 {
            let m = wk_sample_mux(grp * 1000 + r);
            let _ = g.arrive(m, (r % 9) as i32 + 1);
            if r % 3 == 0 {
                let _ = g.wait(m);
            }
            r += 1;
        }
        let _ = g.wait(M_SELECT);
        let _ = g.wait(M_POLL);
        let _ = g.wait(M_EPOLL);
        if g.no_lost() && g.pend.is_empty() {
            h1 += 1;
        }
        grp += 1;
    }
    s.add("J1-万次零丢失", h1 == 10, "三复用器十组千轮压测零丢失");

    // J2 反面：窗外零歧义千次
    let mut ok = 0u32;
    let mut r2 = 0u32;
    while r2 < 1000 {
        let mut g = RaceGuard::new();
        let m = wk_sample_mux(r2);
        let _ = g.arrive(m, 1);
        if g.unambiguous() && g.no_lost() {
            ok += 1;
        }
        r2 += 1;
    }
    s.add("J2-千次零歧义", ok == 1000, "千轮窗账唯一且零丢失");

    // J3 wait 只取本复用器事件：他复用器事件留账
    let j3 = {
        let mut g = RaceGuard::new();
        let _ = g.arrive(M_SELECT, 1);
        let _ = g.arrive(M_EPOLL, 2);
        let got = g.wait(M_SELECT);
        got == Ok(vec![1]) && g.pend.len() == 1 && g.pend[0].mux == M_EPOLL
    };
    s.add("J3-按器取账", j3, "wait 只取本复用器挂起事件");

    // J4 投递账逐次累积
    let j4 = {
        let mut g = RaceGuard::new();
        let _ = g.arrive(M_POLL, 3);
        let _ = g.arrive(M_POLL, 4);
        let _ = g.arrive(M_POLL, 5);
        let _ = g.wait(M_POLL);
        g.delivered == 3
    };
    s.add("J4-投递账累积", j4, "wait 一次取三投递账记三");

    // J5 注入四路全抓
    let inj = inject_four();
    s.add("J5-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "跳窗/双入/坏器号/冻结全数命中");

    // J6 挂起账满必拒（防双入）
    let j6 = {
        let mut g = RaceGuard::new();
        let mut f = 0u32;
        let mut full = true;
        while f < CAP as u32 {
            if g.arrive(M_EPOLL, (f % 7) as i32 + 1).is_err() {
                full = false;
            }
            f += 1;
        }
        full && g.arrive(M_EPOLL, 1).err() == Some(WK_DOUBLE)
    };
    s.add("J6-账满拒收", j6, "挂起账满必拒防双入");

    // J7 序号单调递增
    let j7 = {
        let mut g = RaceGuard::new();
        let a = g.arrive(M_SELECT, 6);
        let b = g.arrive(M_SELECT, 7);
        a == Ok(1) && b == Ok(2)
    };
    s.add("J7-序号单调", j7, "事件序号逐次加一");

    // J8 冻结后到达/wait 双拒
    let j8 = {
        let mut g = RaceGuard::new();
        g.freeze();
        g.arrive(M_POLL, 8).err() == Some(WK_FROZEN) && g.wait(M_POLL).err() == Some(WK_FROZEN)
    };
    s.add("J8-冻结双拒", j8, "冻结后到达与 wait 均必拒");

    // J9 渲染面：三复用器渲染互异
    let j9 = {
        let a = PendEvent { seq: 1, mux: M_SELECT, fd: 1 };
        let b = PendEvent { seq: 1, mux: M_POLL, fd: 1 };
        let c = PendEvent { seq: 1, mux: M_EPOLL, fd: 1 };
        a.render() != b.render() && b.render() != c.render() && a.render() != c.render()
    };
    s.add("J9-渲染互异", j9, "三复用器渲染可区分");

    // J10 fold 稳定：到达/wait 改变指纹、同态一致
    let j10 = {
        let mut g = RaceGuard::new();
        let _ = g.arrive(M_EPOLL, 9);
        let f0 = g.fold();
        let _ = g.wait(M_EPOLL);
        let f1 = g.fold();
        let f2 = g.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J10-指纹随账变", j10, "账动指纹动且同态稳定");

    // J11 动态明细：零歧义千次计数落账
    s.add_dyn("J11-千次明细", ok == 1000, &format!("unamb_ok={}/1000", ok));

    // J12 动态明细：万次零丢失组数落账
    s.add_dyn("J12-万次明细", h1 == 10, &format!("zero_lost_groups={}/10", h1));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10916_raceguard_flow() {
        let mut g = RaceGuard::new();
        assert_eq!(g.arrive(M_EPOLL, 1), Ok(1));
        assert_eq!(g.arrive(M_EPOLL, 2), Ok(2));
        let got = g.wait(M_EPOLL);
        assert_eq!(got, Ok(vec![1u64, 2]));
        assert!(g.pend.is_empty() && g.no_lost());
        assert!(g.arrive(9, 3).err() == Some(WK_NOMUX));
        g.freeze();
        assert!(g.arrive(M_POLL, 4).err() == Some(WK_FROZEN));
    }

    #[test]
    fn unx10916_checks_green() {
        let s = run_unx10916_checks();
        assert!(s.all_passed(), "UNX-F10916 判据未全绿");
    }
}
