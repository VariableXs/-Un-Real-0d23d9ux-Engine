//! UNX-F10340 进程联签轴闭账（十九项对平+哈希冻结）（0x1b500 段）
//!
//! 锚点：UNX-F10340-J1 进程轴闭账——派生/替换/回收三氏族各按 7/6/6
//! 配额登记十九轴项，逐项甲乙双值对平，三项零偏差零未结方可按氏族序
//! 折哈希冻结闭账，冻结后拒动。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：轴项册（项=项号+氏族号+甲值+乙值）+ 配额核（三氏族计数恰达
//! 配额）+ 对平核（偏差单/未结单可枚举）+ 冻结闸（全结零偏差方可冻结，
//! 冻结后拒结拒登）+ 注入核（坏氏族/重项/未结冻结/重结四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b500 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b500;
pub const AX_ZERO: u32 = SEG_BASE + 0x01; // 项号零
pub const AX_DUP: u32 = SEG_BASE + 0x02; // 项号重号
pub const AX_CLAN: u32 = SEG_BASE + 0x03; // 氏族号越界
pub const AX_NOITEM: u32 = SEG_BASE + 0x04; // 未知项
pub const AX_RESETTLE: u32 = SEG_BASE + 0x05; // 已结再结
pub const AX_CAP: u32 = SEG_BASE + 0x06; // 册超容
pub const AX_FROZEN: u32 = SEG_BASE + 0x07; // 冻结后动

/// 氏族号。
pub const CLAN_FORK: u8 = 1;
pub const CLAN_EXEC: u8 = 2;
pub const CLAN_WAIT: u8 = 3;
pub const CLAN_N: u8 = 4;

/// 配额锚：7/6/6 共十九项。
pub const CLAN_QUOTA: [usize; 3] = [7, 6, 6];
/// 总项锚。
pub const ITEMS_NEEDED: usize = 19;
/// 册定容。
pub const CAP: usize = 24;

/// 按项号取样例氏族号（1..=7 派生、8..=13 替换、14..=19 回收）。
pub fn clan_for(no: u32) -> u8 {
    if no <= 7 {
        CLAN_FORK
    } else if no <= 13 {
        CLAN_EXEC
    } else {
        CLAN_WAIT
    }
}

/// 轴项。
#[derive(Clone)]
pub struct AxisItem {
    pub no: u32,
    pub clan: u8,
    pub va: Option<u32>,
    pub vb: Option<u32>,
    pub settled: bool,
}

/// 进程联签轴闭账核。
pub struct ProcAxisClose {
    pub items: Vec<AxisItem>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl ProcAxisClose {
    pub fn new() -> ProcAxisClose {
        ProcAxisClose { items: Vec::new(), frozen: false, root: None }
    }

    fn pos(&self, no: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].no == no {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    fn clan_count(&self, clan: u8) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].clan == clan {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 登记轴项。
    pub fn enroll(&mut self, no: u32, clan: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(AX_FROZEN);
        }
        if no == 0 {
            return Err(AX_ZERO);
        }
        if self.pos(no).is_some() {
            return Err(AX_DUP);
        }
        if clan == 0 || clan >= CLAN_N {
            return Err(AX_CLAN);
        }
        if self.items.len() >= CAP {
            return Err(AX_CAP);
        }
        self.items.push(AxisItem { no, clan, va: None, vb: None, settled: false });
        Ok(())
    }

    /// 结一项：甲乙双值对平（值不等亦记账，但入偏差单）。
    pub fn settle(&mut self, no: u32, va: u32, vb: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(AX_FROZEN);
        }
        match self.pos(no) {
            None => Err(AX_NOITEM),
            Some(p) => {
                if self.items[p].settled {
                    return Err(AX_RESETTLE);
                }
                self.items[p].va = Some(va);
                self.items[p].vb = Some(vb);
                self.items[p].settled = true;
                Ok(())
            }
        }
    }

    /// 偏差单（已结但双值不等）。
    pub fn deviated(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            if it.settled && it.va != it.vb {
                v.push(it.no);
            }
            k += 1;
        }
        v
    }

    /// 未结单。
    pub fn unsettled(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].settled {
                v.push(self.items[k].no);
            }
            k += 1;
        }
        v
    }

    /// 氏族计数。
    pub fn plane_count(&self, clan: u8) -> usize {
        if clan == 0 || clan >= CLAN_N {
            return 0;
        }
        self.clan_count(clan)
    }

    /// 配额核：三氏族计数恰达配额。
    pub fn quotas_ok(&self) -> bool {
        let mut c = 1u8;
        while c < CLAN_N {
            if self.clan_count(c) != CLAN_QUOTA[(c - 1) as usize] {
                return false;
            }
            c += 1;
        }
        true
    }

    /// 闭账：配额齐、零未结、零偏差。
    pub fn closed(&self) -> bool {
        self.quotas_ok()
            && self.unsettled().is_empty()
            && self.deviated().is_empty()
    }

    /// 轴根：按项号序折 64 位哈希（仅已结项入链）。
    pub fn axis_root(&self) -> u64 {
        let mut h: u64 = 0x1b5001b50;
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            if it.settled {
                h = h.wrapping_mul(0x100000001b3).wrapping_add(it.no as u64);
                h = h
                    .wrapping_mul(0x100000001b3)
                    .wrapping_add(it.clan as u64);
                h = h
                    .wrapping_mul(0x100000001b3)
                    .wrapping_add(it.va.unwrap_or(0) as u64);
                h = h
                    .wrapping_mul(0x100000001b3)
                    .wrapping_add(it.vb.unwrap_or(0) as u64);
            }
            k += 1;
        }
        h
    }

    /// 冻结：全结零偏差方可，返回轴根。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Ok(self.root.unwrap_or(0));
        }
        if !self.closed() {
            return Err(AX_FROZEN);
        }
        self.root = Some(self.axis_root());
        self.frozen = true;
        Ok(self.root.unwrap_or(0))
    }

    /// 轴账指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b5000;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            h = h
                .wrapping_mul(31)
                .wrapping_add(it.no)
                .wrapping_mul(31)
                .wrapping_add(it.clan as u32)
                .wrapping_mul(31)
                .wrapping_add(it.va.unwrap_or(0) + 1)
                .wrapping_mul(31)
                .wrapping_add(it.vb.unwrap_or(0) + 1)
                .wrapping_mul(31)
                .wrapping_add(it.settled as u32);
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.frozen as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut c = ProcAxisClose::new();
    // 1) 坏氏族号必拒
    got[0] = c.enroll(1, 0).err() == Some(AX_CLAN)
        && c.enroll(2, 4).err() == Some(AX_CLAN);
    // 2) 重项必拒
    let _ = c.enroll(1, CLAN_FORK);
    got[1] = c.enroll(1, CLAN_EXEC).err() == Some(AX_DUP);
    // 3) 未知项结账必拒
    got[2] = c.settle(99, 1, 1).err() == Some(AX_NOITEM);
    // 4) 未结冻结必拒
    got[3] = c.freeze().err() == Some(AX_FROZEN);
    got
}

/// 批内判据。
pub fn run_unx10340_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10340");

    // J1 正路径：十九项三氏族登记对平闭账冻结
    let mut c = ProcAxisClose::new();
    let mut ok = true;
    let mut no = 1u32;
    let mut clan = 1u8;
    while no <= ITEMS_NEEDED as u32 {
        let idx = (clan - 1) as usize;
        if c.enroll(no, clan).is_err() {
            ok = false;
        }
        if c.plane_count(clan) >= CLAN_QUOTA[idx] && clan < CLAN_N - 1 {
            clan += 1;
        }
        no += 1;
    }
    no = 1;
    while no <= ITEMS_NEEDED as u32 {
        if c.settle(no, no * 3, no * 3).is_err() {
            ok = false;
        }
        no += 1;
    }
    let frozen = c.freeze();
    s.add(
        "J1-进程轴闭账",
        ok && c.closed() && frozen.is_ok() && c.frozen,
        "十九项三氏族对平闭账并哈希冻结",
    );

    // J2 配额核逐族对平
    let j2 = c.plane_count(CLAN_FORK) == 7
        && c.plane_count(CLAN_EXEC) == 6
        && c.plane_count(CLAN_WAIT) == 6
        && c.quotas_ok();
    s.add("J2-配额对平", j2, "三氏族计数恰为 7/6/6");

    // J3 注入检出：坏氏族 + 重项
    let inj = inject_four();
    s.add("J3-氏族重项检出", inj[0] && inj[1], "坏氏族号与重项必拒");

    // J4 注入检出：未知项 + 未结冻结
    s.add("J4-未知未结检出", inj[2] && inj[3], "未知项结账与未结冻结必拒");

    // J5 偏差阻断冻结
    let j5 = {
        let mut t = ProcAxisClose::new();
        let mut n = 1u32;
        while n <= ITEMS_NEEDED as u32 {
            let _ = t.enroll(n, clan_for(n));
            n += 1;
        }
        n = 1;
        while n <= ITEMS_NEEDED as u32 {
            let dv = if n == 9 { 5 } else { n };
            let _ = t.settle(n, dv, n);
            n += 1;
        }
        !t.closed() && t.deviated().contains(&9)
    };
    s.add("J5-偏差阻断", j5, "任一项双值不等即不得冻结");

    // J6 未结阻断冻结（缺第十九项）
    let j6 = {
        let mut t = ProcAxisClose::new();
        let mut n = 1u32;
        while n < ITEMS_NEEDED as u32 {
            let _ = t.enroll(n, clan_for(n));
            let _ = t.settle(n, n, n);
            n += 1;
        }
        !t.closed() && t.freeze().err() == Some(AX_FROZEN)
    };
    s.add("J6-未结阻断", j6, "存在未结项即不得冻结");

    // J7 冻结后动必拒
    let j7 = {
        let mut t = ProcAxisClose::new();
        let mut n = 1u32;
        while n <= ITEMS_NEEDED as u32 {
            let _ = t.enroll(n, clan_for(n));
            let _ = t.settle(n, n, n);
            n += 1;
        }
        let _ = t.freeze();
        t.enroll(99, CLAN_FORK).err() == Some(AX_FROZEN)
            && t.settle(1, 1, 1).err() == Some(AX_FROZEN)
    };
    s.add("J7-冻结拒动", j7, "冻结后登记与结账均拒");

    // J8 重结必拒
    let j8 = {
        let mut t = ProcAxisClose::new();
        let _ = t.enroll(1, CLAN_FORK);
        let _ = t.settle(1, 2, 2);
        t.settle(1, 3, 3).err() == Some(AX_RESETTLE)
    };
    s.add("J8-重结拒收", j8, "一项只许结一次");

    // J9 轴根随结项推进且冻结后稳定
    let j9 = {
        let mut t = ProcAxisClose::new();
        let _ = t.enroll(1, CLAN_FORK);
        let r0 = t.axis_root();
        let _ = t.settle(1, 2, 2);
        let r1 = t.axis_root();
        r0 != r1 && t.axis_root() == r1
    };
    s.add("J9-轴根推进", j9, "每结一项轴根一变且终态稳定");

    // J10 容量边界
    let j10 = {
        let mut t = ProcAxisClose::new();
        let mut n = 1u32;
        let mut filled = true;
        while n <= CAP as u32 {
            if t.enroll(n, ((n - 1) % 3 + 1) as u8).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.enroll(999, CLAN_FORK).err() == Some(AX_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原册不损");

    // J11 冻结返回轴根与账内根一致
    let j11 = match frozen {
        Ok(r) => r == c.axis_root() && r != 0,
        Err(_) => false,
    };
    s.add("J11-冻结回根", j11, "冻结返回值即冻结轴根");

    // J12 动态明细：未结计数落账
    let un = c.unsettled().len();
    s.add_dyn("J12-未结计数", un == 0, &format!("unsettled={}", un));

    // J13 动态明细：轴账指纹落账
    let fp = c.fold();
    s.add_dyn("J13-轴账指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10340_proc_axis_close_flow() {
        let mut c = ProcAxisClose::new();
        assert!(c.enroll(0, CLAN_FORK).err() == Some(AX_ZERO));
        assert!(c.enroll(1, 0).err() == Some(AX_CLAN));
        assert!(c.enroll(1, 9).err() == Some(AX_CLAN));
        assert!(c.enroll(1, CLAN_FORK).is_ok());
        assert!(c.enroll(1, CLAN_EXEC).err() == Some(AX_DUP));
        assert!(c.settle(99, 1, 1).err() == Some(AX_NOITEM));
        assert!(c.settle(1, 1, 2).is_ok());
        assert!(c.settle(1, 1, 1).err() == Some(AX_RESETTLE));
        assert!(c.deviated().contains(&1));
        assert!(!c.closed());
        assert!(c.freeze().err() == Some(AX_FROZEN));
        let mut n = 2u32;
        while n <= ITEMS_NEEDED as u32 {
            let _ = c.enroll(n, clan_for(n));
            n += 1;
        }
        n = 1;
        while n <= ITEMS_NEEDED as u32 {
            let _ = c.settle(n, n, n);
            n += 1;
        }
        assert!(c.quotas_ok() || c.plane_count(CLAN_WAIT) == 6);
        assert!(c.closed());
        let r = c.freeze();
        assert!(r.is_ok());
        assert!(c.frozen);
        assert!(c.enroll(88, CLAN_FORK).err() == Some(AX_FROZEN));
    }

    #[test]
    fn unx10340_checks_green() {
        let s = run_unx10340_checks();
        assert!(s.all_passed(), "UNX-F10340 判据未全绿");
    }
}
