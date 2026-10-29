//! UNX-F10910 poll revents 契约矩阵（0x1d140 段）
//!
//! 锚点：UNX-F10910-J1 正面：events 用户域/revents 内核域隔离（10/10）；
//! 反面：内核零写 events 域（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·revents 契约面）。
//!
//! 结构：双域条目（events 用户域 / revents 内核域各持一份）+ 契约器
//! （内核只写 revents、revents ⊆ events∪异常三旗）+ 注入核（内核写
//! events 域/越旗回填/双写漂移/冻结改册四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d140 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d140;
pub const RC_EVTWRITE: u32 = SEG_BASE + 0x01; // 内核写 events 域
pub const RC_OUTFLAG: u32 = SEG_BASE + 0x02; // revents 越旗
pub const RC_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改册
pub const RC_DUP: u32 = SEG_BASE + 0x04; // 重号登记
pub const RC_NOEVENT: u32 = SEG_BASE + 0x05; // 全无事件

/// 事件旗位。
pub const F_IN: u16 = 0x0001;
pub const F_OUT: u16 = 0x0004;
pub const F_ERR: u16 = 0x0008;
pub const F_HUP: u16 = 0x0010;
pub const F_NVAL: u16 = 0x0020;
/// 异常三旗（revents 可含而 events 不需置）。
pub const EXC_MASK: u16 = F_ERR | F_HUP | F_NVAL;

/// 契约册定容。
pub const CAP: usize = 32;
/// RC 域标（fold 种子）。
pub const RC_TAG: u64 = 0x1d1420a;

/// 双域条目。
#[derive(Clone, Debug)]
pub struct DualSlot {
    pub fd: i32,
    pub live: bool,
    pub events: u16,
    pub revents: u16,
    pub evt_writes: u32,
}

impl DualSlot {
    /// 契约核验：revents 必 ⊆ events ∪ 异常三旗。
    pub fn contract_ok(&self) -> bool {
        let allow = self.events | EXC_MASK;
        self.revents & !allow == 0
    }

    /// 用户域快照（内核回填前后 events 不变即隔离成立）。
    pub fn evt_snapshot(&self) -> u16 {
        self.events
    }

    pub fn render(&self) -> String {
        format!("D{}|e{:04x}|r{:04x}", self.fd, self.events, self.revents)
    }
}

/// revents 契约册。
pub struct ContractBoard {
    pub slots: Vec<DualSlot>,
    pub frozen: bool,
    pub kernel_writes: u32,
}

impl ContractBoard {
    pub fn new() -> ContractBoard {
        ContractBoard { slots: Vec::new(), frozen: false, kernel_writes: 0 }
    }

    /// 登记：只写 events 域。
    pub fn enroll(&mut self, fd: i32, events: u16) -> Result<(), u32> {
        if self.frozen {
            return Err(RC_FROZEN);
        }
        if self.slots.iter().any(|e| e.fd == fd) {
            return Err(RC_DUP);
        }
        if self.slots.len() >= CAP {
            return Err(RC_FROZEN);
        }
        self.slots.push(DualSlot { fd, live: true, events, revents: 0, evt_writes: 0 });
        Ok(())
    }

    /// 杀 fd：NVAL 语义由回填体现。
    pub fn kill(&mut self, fd: i32) {
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].fd == fd {
                self.slots[k].live = false;
            }
            k += 1;
        }
    }

    /// 内核回填：只写 revents 域，记账写次数；events 域零写。
    pub fn fill(&mut self, ready_map: &[(i32, u16)]) -> Result<usize, u32> {
        if self.frozen {
            return Err(RC_FROZEN);
        }
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            let fd = self.slots[i].fd;
            let live = self.slots[i].live;
            let events = self.slots[i].events;
            if !live {
                self.slots[i].revents = F_NVAL;
                self.kernel_writes += 1;
                n += 1;
            } else {
                // 查就绪表（副本读，不越界）。
                let mut got = 0u16;
                let mut j = 0usize;
                while j < ready_map.len() {
                    if ready_map[j].0 == fd {
                        got = ready_map[j].1;
                    }
                    j += 1;
                }
                let rv = got & (events | EXC_MASK);
                self.slots[i].revents = rv;
                if rv != 0 {
                    self.kernel_writes += 1;
                    n += 1;
                }
            }
            i += 1;
        }
        if n == 0 {
            return Err(RC_NOEVENT);
        }
        Ok(n)
    }

    /// 契约审计：全册 contract_ok 且 events 域零写。
    pub fn audit(&self) -> bool {
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            if !e.contract_ok() || e.evt_writes != 0 {
                return false;
            }
            k += 1;
        }
        true
    }

    /// revents 置位条目数。
    pub fn set_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].revents != 0 {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 链式 fold 指纹（序敏感，含双域值）。
    pub fn fold(&self) -> u64 {
        let mut h = RC_TAG;
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.events as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.revents as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.live as u64).wrapping_mul(0x100000001b3);
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
    // 1) 内核写 events 域即契约破裂（模拟坏内核）
    let mut bad = DualSlot { fd: 1, live: true, events: F_IN, revents: 0, evt_writes: 1 };
    got[0] = !bad.contract_ok() || bad.evt_writes != 0;
    // 2) 越旗回填：revents 含 events 外旗必被 contract_ok 抓
    let mut oob = DualSlot { fd: 2, live: true, events: F_IN, revents: F_OUT, evt_writes: 0 };
    got[1] = !oob.contract_ok();
    // 3) 异常三旗豁免：HUP 即使未监听也合法
    let mut hup = DualSlot { fd: 3, live: true, events: 0, revents: F_HUP, evt_writes: 0 };
    let _ = &mut bad;
    let _ = &mut oob;
    got[2] = hup.contract_ok();
    // 4) 冻结后回填必抓
    let mut f = ContractBoard::new();
    f.freeze();
    let mut rm: Vec<(i32, u16)> = Vec::new();
    rm.push((1, F_IN));
    got[3] = f.fill(&rm).err() == Some(RC_FROZEN);
    got
}

/// 就绪图确定性样本：第 r 轮 fd 的就绪旗。
pub fn rc_sample_ready(fd: i32, r: u32) -> u16 {
    let m = (fd as u32).wrapping_add(r).wrapping_mul(2654435761) % 4;
    match m {
        0 => 0,
        1 => F_IN,
        2 => F_OUT,
        _ => F_HUP,
    }
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10910_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10910");

    // J1 正面：双域隔离十轮全守约
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut b = ContractBoard::new();
        let _ = b.enroll(r as i32 + 1, F_IN | F_OUT);
        let mut rm: Vec<(i32, u16)> = Vec::new();
        rm.push((r as i32 + 1, rc_sample_ready(r as i32 + 1, r)));
        let _ = b.fill(&rm);
        let e0 = b.slots[0].evt_snapshot();
        if b.audit() && e0 == F_IN | F_OUT && b.kernel_writes >= 1 {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-双域隔离十连", h1 == 10, "内核只写 revents 十轮全守约");

    // J2 反面：内核零写 events 域（千次）
    let mut ok = 0u32;
    let mut r2 = 0u32;
    while r2 < 1000 {
        let mut b = ContractBoard::new();
        let fd = (r2 % 8) as i32 + 1;
        let _ = b.enroll(fd, F_IN);
        let mut rm: Vec<(i32, u16)> = Vec::new();
        rm.push((fd, rc_sample_ready(fd, r2)));
        let _ = b.fill(&rm);
        if b.slots[0].evt_writes == 0 && b.slots[0].evt_snapshot() == F_IN {
            ok += 1;
        }
        r2 += 1;
    }
    s.add("J2-零写events千次", ok == 1000, "内核千轮零写 events 域");

    // J3 revents ⊆ events∪异常三旗审计
    let j3 = {
        let mut b = ContractBoard::new();
        let _ = b.enroll(11, F_IN);
        let mut rm: Vec<(i32, u16)> = Vec::new();
        rm.push((11, F_OUT | F_HUP)); // OUT 未监听 → 越旗裁去；HUP 豁免保留
        let _ = b.fill(&rm);
        b.slots[0].revents == F_HUP && b.audit()
    };
    s.add("J3-越旗裁剪", j3, "未监听 OUT 裁去、HUP 豁免保留");

    // J4 注入四路全抓
    let inj = inject_four();
    s.add("J4-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "写events/越旗/豁免/冻结全数命中");

    // J5 杀 fd 回填 NVAL
    let j5 = {
        let mut b = ContractBoard::new();
        let _ = b.enroll(12, F_IN);
        b.kill(12);
        let mut rm: Vec<(i32, u16)> = Vec::new();
        let _ = b.fill(&rm);
        b.slots[0].revents == F_NVAL && b.audit()
    };
    s.add("J5-死fd回NVAL", j5, "杀 fd 回填 NVAL 且守约");

    // J6 重号登记拒
    let j6 = {
        let mut b = ContractBoard::new();
        let _ = b.enroll(13, F_IN);
        b.enroll(13, F_OUT).err() == Some(RC_DUP)
    };
    s.add("J6-重号拒收", j6, "同 fd 重复登记必拒");

    // J7 冻结后回填与登记均拒
    let j7 = {
        let mut b = ContractBoard::new();
        b.freeze();
        let mut rm: Vec<(i32, u16)> = Vec::new();
        b.enroll(14, F_IN).err() == Some(RC_FROZEN) && b.fill(&rm).err() == Some(RC_FROZEN)
    };
    s.add("J7-冻结双拒", j7, "冻结后登记与回填均必拒");

    // J8 零事件显性报
    let j8 = {
        let mut b = ContractBoard::new();
        let _ = b.enroll(15, F_IN);
        let mut rm: Vec<(i32, u16)> = Vec::new();
        rm.push((15, 0));
        b.fill(&rm).err() == Some(RC_NOEVENT)
    };
    s.add("J8-零事件显性", j8, "全零回填报 NOEVENT 而非成功");

    // J9 返回数与置位数一致
    let j9 = {
        let mut b = ContractBoard::new();
        let _ = b.enroll(16, F_IN);
        let _ = b.enroll(17, F_OUT);
        let _ = b.enroll(18, 0);
        let mut rm: Vec<(i32, u16)> = Vec::new();
        rm.push((16, F_IN));
        rm.push((17, F_OUT));
        rm.push((18, F_IN));
        let v = b.fill(&rm);
        v == Ok(2) && b.set_count() == 2
    };
    s.add("J9-计数一致", j9, "返回数与 revents 置位数一致");

    // J10 渲染面：双域十六进制渲染可区分
    let j10 = {
        let a = DualSlot { fd: 1, live: true, events: F_IN, revents: F_IN, evt_writes: 0 };
        let b = DualSlot { fd: 1, live: true, events: F_IN, revents: F_OUT, evt_writes: 0 };
        a.render() != b.render()
    };
    s.add("J10-渲染分域", j10, "双域值渲染可区分");

    // J11 fold 稳定：回填前后指纹必变
    let j11 = {
        let mut b = ContractBoard::new();
        let _ = b.enroll(19, F_IN);
        let f0 = b.fold();
        let mut rm: Vec<(i32, u16)> = Vec::new();
        rm.push((19, F_IN));
        let _ = b.fill(&rm);
        let f1 = b.fold();
        let f2 = b.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J11-指纹随回填变", j11, "回填改变指纹且同态稳定");

    // J12 动态明细：零写千次计数落账
    s.add_dyn("J12-千次明细", ok == 1000, &format!("nowrite_ok={}/1000", ok));

    // J13 动态明细：守约十连计数落账
    s.add_dyn("J13-十连明细", h1 == 10, &format!("isolate_hits={}/10", h1));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10910_contractboard_flow() {
        let mut b = ContractBoard::new();
        assert!(b.enroll(3, F_IN | F_OUT).is_ok());
        assert!(b.enroll(3, F_IN).err() == Some(RC_DUP));
        let mut rm: Vec<(i32, u16)> = Vec::new();
        rm.push((3, F_IN | F_OUT));
        assert_eq!(b.fill(&rm), Ok(1));
        assert_eq!(b.slots[0].revents, F_IN | F_OUT);
        assert!(b.audit());
        b.kill(3);
        let mut rm2: Vec<(i32, u16)> = Vec::new();
        let _ = b.fill(&rm2);
        assert_eq!(b.slots[0].revents, F_NVAL);
        b.freeze();
        assert!(b.fill(&rm).err() == Some(RC_FROZEN));
    }

    #[test]
    fn unx10910_checks_green() {
        let s = run_unx10910_checks();
        assert!(s.all_passed(), "UNX-F10910 判据未全绿");
    }
}
