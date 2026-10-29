//! UNX-F10317 联签降级预案（对方缺席时挂账）（0x1b220 段）
//!
//! 锚点：UNX-F10317-J1 缺席挂账——对方缺席时逐项挂账不许硬裁，复勤
//! 后逐项销账，挂账未销即预案未清。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：挂账册（联签项：号+判据+理由，仅缺席时可挂）+ 在勤位（对方
//! 在勤/缺席独立可翻）+ 销账账（复勤后逐项销账，重销即拒）+ 清账裁断
//! （在勤+挂账全销）+ 注入核（在勤挂账/缺席销账/重销/重号四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b220 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b220;
pub const FB_DUP: u32 = SEG_BASE + 0x01; // 挂账号重号
pub const FB_EMPTY: u32 = SEG_BASE + 0x02; // 挂账理由空
pub const FB_ZERO: u32 = SEG_BASE + 0x03; // 判据号为零
pub const FB_CAP: u32 = SEG_BASE + 0x04; // 册超容
pub const FB_PRESENT: u32 = SEG_BASE + 0x05; // 对方在勤不许挂账
pub const FB_ABSENT: u32 = SEG_BASE + 0x06; // 对方缺席不许销账
pub const FB_NOHANG: u32 = SEG_BASE + 0x07; // 未知挂账
pub const FB_RESETTLE: u32 = SEG_BASE + 0x08; // 已销再销

/// 挂账册定容。
pub const CAP: usize = 12;
/// 挂账数锚（预案面允许的最大挂账）。
pub const HANG_LIMIT: usize = 4;

/// 单条挂账。
#[derive(Clone)]
pub struct HangItem {
    pub hid: u32,
    pub cid: u32,
    pub reason: String,
}

/// 联签降级预案册。
pub struct FallbackPlan {
    pub peer_present: bool,
    pub hung: Vec<HangItem>,
    pub settled: Vec<u32>,
}

impl FallbackPlan {
    pub fn new() -> FallbackPlan {
        FallbackPlan { peer_present: true, hung: Vec::new(), settled: Vec::new() }
    }

    /// 翻在勤位。
    pub fn set_presence(&mut self, p: bool) {
        self.peer_present = p;
    }

    /// 挂账：仅在勤位为缺席时许挂；重号、空理由、零判据、超容均拒。
    pub fn hang(&mut self, hid: u32, cid: u32, reason: &str) -> Result<(), u32> {
        if self.peer_present {
            return Err(FB_PRESENT);
        }
        if hid == 0 || self.hung.iter().any(|h| h.hid == hid) {
            return Err(FB_DUP);
        }
        if reason.is_empty() {
            return Err(FB_EMPTY);
        }
        if cid == 0 {
            return Err(FB_ZERO);
        }
        if self.hung.len() >= CAP {
            return Err(FB_CAP);
        }
        self.hung.push(HangItem { hid, cid, reason: reason.to_string() });
        Ok(())
    }

    fn hung_pos(&self, hid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.hung.len() {
            if self.hung[k].hid == hid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 销账：仅在勤位为在勤时许销；未知/重销均拒。
    pub fn settle(&mut self, hid: u32) -> Result<(), u32> {
        if !self.peer_present {
            return Err(FB_ABSENT);
        }
        if self.hung_pos(hid).is_none() {
            return Err(FB_NOHANG);
        }
        if self.settled.contains(&hid) {
            return Err(FB_RESETTLE);
        }
        self.settled.push(hid);
        Ok(())
    }

    /// 未销挂账清单。
    pub fn unsettled(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.hung.len() {
            if !self.settled.contains(&self.hung[k].hid) {
                v.push(self.hung[k].hid);
            }
            k += 1;
        }
        v
    }

    /// 已销清单（销账序）。
    pub fn settled_ids(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.settled.len() {
            v.push(self.settled[k]);
            k += 1;
        }
        v
    }

    /// 挂账号 → 判据号（未知挂账即拒）。
    pub fn cid_of(&self, hid: u32) -> Result<u32, u32> {
        match self.hung_pos(hid) {
            None => Err(FB_NOHANG),
            Some(k) => Ok(self.hung[k].cid),
        }
    }

    /// 挂账号 → 理由（未知挂账即拒）。
    pub fn reason_of(&self, hid: u32) -> Result<String, u32> {
        match self.hung_pos(hid) {
            None => Err(FB_NOHANG),
            Some(k) => Ok(self.hung[k].reason.clone()),
        }
    }

    /// 预案清账：对方在勤、挂账不超限且全数销账。
    pub fn clean(&self) -> bool {
        self.peer_present
            && self.hung.len() <= HANG_LIMIT
            && self.settled.len() == self.hung.len()
            && self.unsettled().is_empty()
    }

    /// 挂账理由清单（按登记序）。
    pub fn reasons(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.hung.len() {
            v.push(self.hung[k].reason.to_string());
            k += 1;
        }
        v
    }

    /// 预案指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b2200;
        h = h.wrapping_mul(31).wrapping_add(self.hung.len() as u32);
        let mut k = 0usize;
        while k < self.hung.len() {
            h = h.wrapping_mul(31).wrapping_add(self.hung[k].hid);
            h = h.wrapping_mul(31).wrapping_add(self.hung[k].cid);
            let bytes = self.hung[k].reason.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = h.wrapping_mul(31).wrapping_add(bytes[b] as u32);
                b += 1;
            }
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.settled.len() as u32);
        h = h.wrapping_mul(31).wrapping_add(self.peer_present as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut p = FallbackPlan::new();
    // 1) 对方在勤时挂账必拒
    got[0] = p.hang(1, 10, "在勤挂账").err() == Some(FB_PRESENT);
    // 2) 缺席销账必拒
    p.set_presence(false);
    let _ = p.hang(1, 10, "缺席挂账");
    got[1] = p.settle(1).err() == Some(FB_ABSENT);
    // 3) 重号挂账必拒
    got[2] = p.hang(1, 11, "重号").err() == Some(FB_DUP);
    // 4) 未销挂账堵清账
    p.set_presence(true);
    got[3] = !p.clean() && p.unsettled() == [1u32];
    got
}

/// 批内判据。
pub fn run_unx10317_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10317");

    // J1 正路径：缺席挂账两条→复勤→逐条销账→清账
    let mut p = FallbackPlan::new();
    p.set_presence(false);
    let ok1 = p.hang(1, 10, "乙缺席挂一").is_ok() && p.hang(2, 20, "乙缺席挂二").is_ok();
    p.set_presence(true);
    let ok2 = p.settle(1).is_ok() && p.settle(2).is_ok();
    s.add("J1-挂销闭环", ok1 && ok2 && p.clean(), "缺席挂账复勤销账全链成立");

    // J2 挂账理由清单齐
    let j2 = p.reasons() == ["乙缺席挂一".to_string(), "乙缺席挂二".to_string()];
    s.add("J2-理由清单", j2, "挂账理由按登记序齐备");

    // J3 注入检出：在勤挂账 + 缺席销账
    let inj = inject_four();
    s.add("J3-在勤缺席检出", inj[0] && inj[1], "在勤挂账必拒且缺席销账必拒");

    // J4 注入检出：重号 + 未销堵清账
    s.add("J4-重号未销检出", inj[2] && inj[3], "重号必拒且未销堵清账");

    // J5 销账后重销必拒
    let j5 = p.settle(1).err() == Some(FB_RESETTLE);
    s.add("J5-重销拒", j5, "已销再销必拒");

    // J6 未知挂账销账拒
    let j6 = p.settle(99).err() == Some(FB_NOHANG);
    s.add("J6-未知销拒", j6, "销不存在的挂账必拒");

    // J7 空理由拒
    let j7 = {
        let mut c = FallbackPlan::new();
        c.set_presence(false);
        c.hang(1, 10, "").err() == Some(FB_EMPTY)
    };
    s.add("J7-空理由拒", j7, "挂账理由空必拒");

    // J8 零判据拒
    let j8 = {
        let mut c = FallbackPlan::new();
        c.set_presence(false);
        c.hang(1, 0, "零").err() == Some(FB_ZERO)
    };
    s.add("J8-零判据拒", j8, "挂账判据号零必拒");

    // J9 挂账限额语义
    let j9 = {
        let mut c = FallbackPlan::new();
        c.set_presence(false);
        let mut n = 1u32;
        while n <= HANG_LIMIT as u32 {
            let r = format!("限{}", n);
            let _ = c.hang(n, n * 10, &r);
            n += 1;
        }
        c.hung.len() == HANG_LIMIT && c.unsettled().len() == HANG_LIMIT
    };
    s.add("J9-挂账限额", j9, "挂账达限即预案满载可枚举");

    // J10 容量边界
    let j10 = {
        let mut c = FallbackPlan::new();
        c.set_presence(false);
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            let r = format!("c{}", n);
            if c.hang(n, n, &r).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.hang(999, 999, "over").err() == Some(FB_CAP)
    };
    s.add("J10-容量边界", j10, "册超容必拒且原册不损");

    // J11 在勤初值即清账态（零挂账）
    let j11 = FallbackPlan::new().clean();
    s.add("J11-零挂即清", j11, "无挂账且在勤即预案清账");

    // J12 动态明细：未销清单落账
    let u = p.unsettled().len();
    s.add_dyn("J12-未销零清单", u == 0, &format!("unsettled={}", u));

    // J13 动态明细：预案指纹落账
    let fp = p.fold();
    s.add_dyn("J13-预案指纹", fp != 0, &format!("fold={:#x}", fp));

    // J14 动态明细：挂账定向取值对账
    let cid = p.cid_of(1);
    let rs = p.reason_of(2);
    let j14 = cid == Ok(10)
        && rs == Ok("乙缺席挂二".to_string())
        && p.cid_of(99).err() == Some(FB_NOHANG);
    s.add_dyn(
        "J14-定向取值",
        j14,
        &format!("挂一判据 {:?} 挂二理由 {:?}", cid, rs),
    );

    // J15 动态明细：已销清单对账
    let sd = p.settled_ids();
    s.add_dyn("J15-已销清单", sd == [1u32, 2], &format!("{:?}", sd));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10317_fallback_flow() {
        let mut p = FallbackPlan::new();
        assert!(p.hang(1, 10, "r").err() == Some(FB_PRESENT));
        p.set_presence(false);
        assert!(p.hang(1, 10, "r").is_ok());
        assert!(p.hang(1, 11, "dup").err() == Some(FB_DUP));
        assert!(p.hang(2, 0, "zero").err() == Some(FB_ZERO));
        assert!(p.hang(2, 20, "").err() == Some(FB_EMPTY));
        assert!(p.settle(1).err() == Some(FB_ABSENT));
        assert!(p.unsettled() == [1u32]);
        p.set_presence(true);
        assert!(p.settle(1).is_ok());
        assert!(p.settle(1).err() == Some(FB_RESETTLE));
        assert!(p.settle(9).err() == Some(FB_NOHANG));
        assert!(!p.clean());
        assert!(p.settle(2).is_ok());
        assert!(p.clean());
    }

    #[test]
    fn unx10317_checks_green() {
        let s = run_unx10317_checks();
        assert!(s.all_passed(), "UNX-F10317 判据未全绿");
    }
}
