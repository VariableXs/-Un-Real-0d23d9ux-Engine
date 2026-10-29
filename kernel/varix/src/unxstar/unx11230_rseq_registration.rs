//! UNX-F11230 rseq 登记面消费账（0x1f831 段）
//!
//! 锚点：UNX-F11230-J1 rseq 注册接受/拒绝两路径判据绿，拒绝时 glibc 回退
//! 非 rseq 路径有账。
//! 域：CoRun·UNX-C5-B02——接受路径校验长度、签名、标志三件；拒绝路径不只
//! 返回错码，还要把「已回退到非 rseq」这一事实记进账，避免静默降级。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f830;
pub const RS_REG: u32 = SEG_BASE + 0x01; // 地址为零
pub const RS_SIZE: u32 = SEG_BASE + 0x02; // 长度不足三十二
pub const RS_FLAG: u32 = SEG_BASE + 0x03; // 标志非零
pub const RS_OFF: u32 = SEG_BASE + 0x04; // 签名/偏移非法
pub const RS_FALLBACK: u32 = SEG_BASE + 0x05; // rseq 不可用
pub const RS_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const RS_OVER: u32 = SEG_BASE + 0x07; // 超容

/// rseq 结构最小长度与登记上限。
pub const MIN_RSEQ: u32 = 32;
pub const WANT: usize = 2;
pub const CAP: usize = 8;
pub const RS_TAG: u64 = 0x1f830b3;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一次 rseq 登记。
#[derive(Clone, Debug)]
pub struct Reg {
    pub id: u32,
    pub addr: u64,
    pub len: u32,
    pub sig: u32,
    pub flags: u32,
}

impl Reg {
    pub fn shaped(&self) -> bool {
        self.addr % 32 == 0 && self.len >= MIN_RSEQ && self.sig == 0 && self.flags == 0
    }
}

/// rseq 登记账。
#[derive(Clone, Debug)]
pub struct RseqLedger {
    pub available: bool,
    pub regs: Vec<Reg>,
    pub rejects: u32,
    pub fallback: bool,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl RseqLedger {
    pub fn new(available: bool) -> RseqLedger {
        RseqLedger { available, regs: Vec::new(), rejects: 0, fallback: false, frozen: false, root: None }
    }

    /// 注册：不可用/空址/短长/坏标志/坏签名五路拒。
    pub fn register(&mut self, id: u32, addr: u64, len: u32, sig: u32, flags: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(RS_FROZEN);
        }
        if !self.available {
            self.rejects = self.rejects.saturating_add(1);
            self.fallback = true;
            return Err(RS_FALLBACK);
        }
        if addr == 0 {
            return Err(RS_REG);
        }
        if len < MIN_RSEQ {
            return Err(RS_SIZE);
        }
        if flags != 0 {
            return Err(RS_FLAG);
        }
        if sig != 0 || addr % 32 != 0 {
            return Err(RS_OFF);
        }
        if self.regs.iter().any(|r| r.id == id || r.addr == addr) {
            return Err(RS_REG);
        }
        if self.regs.len() >= CAP {
            return Err(RS_OVER);
        }
        self.regs.push(Reg { id, addr, len, sig, flags });
        Ok(())
    }

    /// 拒绝路径是否已回退（有拒必有回退记录）。
    pub fn fallback_accounted(&self) -> bool {
        self.rejects == 0 || self.fallback
    }

    pub fn all_shaped(&self) -> bool {
        !self.regs.is_empty() && self.regs.iter().all(|r| r.shaped())
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(RS_FROZEN);
        }
        if !self.available {
            return Err(RS_FALLBACK);
        }
        if self.regs.len() != WANT {
            return Err(RS_REG);
        }
        if !self.all_shaped() {
            return Err(RS_SIZE);
        }
        if !self.fallback_accounted() {
            return Err(RS_FALLBACK);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("rseq regs {} rejects {} avail {} fb {}", self.regs.len(), self.rejects, self.available, self.fallback)
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = RS_TAG;
        h = (h ^ self.available as u64).wrapping_mul(FP_STEP);
        let mut k = 0usize;
        while k < self.regs.len() {
            h = (h ^ self.regs[k].addr).wrapping_mul(FP_STEP);
            h = (h ^ self.regs[k].len as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.regs[k].id as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h = (h ^ self.rejects as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.fallback as u64).wrapping_mul(FP_STEP);
        h
    }
}

/// 两个 rseq 区地址（三十二字节对齐）。
pub const ADDRS: [u64; 2] = [0x7f40_0000_0000, 0x7f40_0000_0040];

/// 组一张两登记、可用态的账。
pub fn build() -> RseqLedger {
    let mut l = RseqLedger::new(true);
    let mut k = 0usize;
    while k < WANT {
        let _ = l.register(k as u32 + 1, ADDRS[k], 32, 0, 0);
        k += 1;
    }
    l
}

/// 四路对抗：空址、短长、坏标志、不可用登记。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = RseqLedger::new(true);
    out[0] = a.register(1, 0, 32, 0, 0) == Err(RS_REG);
    out[1] = a.register(1, ADDRS[0], 16, 0, 0) == Err(RS_SIZE);
    out[2] = a.register(1, ADDRS[0], 32, 0, 2) == Err(RS_FLAG);
    let mut b = RseqLedger::new(false);
    out[3] = b.register(1, ADDRS[0], 32, 0, 0) == Err(RS_FALLBACK) && b.fallback;
    out
}

/// 判据入口。
pub fn run_unx11230_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11230");
    let mut l = build();
    let root = l.freeze().unwrap_or(0);

    s.add("J1-两登记", l.regs.len() == WANT, "可用态下恰两条 rseq 登记");
    s.add("J2-长度足", l.regs.iter().all(|r| r.len >= MIN_RSEQ), "每条长度不小于三十二字节");
    s.add("J3-签名零", l.regs.iter().all(|r| r.sig == 0), "签名列全部为零");
    s.add("J4-标志零", l.regs.iter().all(|r| r.flags == 0), "标志列全部为零");
    s.add("J5-地址对齐", l.regs.iter().all(|r| r.addr % 32 == 0), "地址按三十二字节对齐");
    s.add("J6-冻存根", root != 0 && l.root == Some(root), "登记账冻存产出非零根");

    s.add("J7-冻后登记拒", l.register(3, 0x7f40_0000_0080, 32, 0, 0) == Err(RS_FROZEN), "冻存后登记拒为 RS_FROZEN");
    s.add("J8-短长拒", {
        let mut x = RseqLedger::new(true);
        x.register(1, ADDRS[0], 31, 0, 0) == Err(RS_SIZE)
    }, "长度三十一拒为 RS_SIZE");
    s.add("J9-坏标志拒", {
        let mut x = RseqLedger::new(true);
        x.register(1, ADDRS[0], 32, 0, 1) == Err(RS_FLAG)
    }, "标志非零拒为 RS_FLAG");
    s.add("J10-坏签名拒", {
        let mut x = RseqLedger::new(true);
        x.register(1, ADDRS[0], 32, 7, 0) == Err(RS_OFF)
    }, "签名非零拒为 RS_OFF");

    let mut f = RseqLedger::new(false);
    let rejected = f.register(1, ADDRS[0], 32, 0, 0);
    s.add("J11-拒绝回退有账", rejected == Err(RS_FALLBACK) && f.fallback && f.fallback_accounted() && f.rejects == 1, "不可用路径拒后即记回退账");
    s.add("J12-不可用禁冻", f.freeze() == Err(RS_FALLBACK), "不可用态冻存拒为 RS_FALLBACK");

    let four = inject_four();
    s.add("J13-四路注入", four.iter().all(|x| *x), "空址/短长/坏标志/不可用四路全拒");
    s.add_dyn(
        "J14-双路径明细",
        l.regs.len() == 2 && f.fallback,
        &format!("接受 {} 条，拒绝 {} 次，回退账 {}", l.regs.len(), f.rejects, f.fallback),
    );
    s.add_dyn(
        "J15-账根",
        root == l.fold(),
        &format!("根 0x{:x}，首址 0x{:x}", root, l.regs[0].addr),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11230_rseq_registration_flow() {
        let mut l = build();
        assert!(l.all_shaped());
        let r = l.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(l.register(3, 0x7f40_0000_0080, 32, 0, 0), Err(RS_FROZEN));
        let mut f = RseqLedger::new(false);
        assert_eq!(f.register(1, ADDRS[0], 32, 0, 0), Err(RS_FALLBACK));
        assert!(f.fallback);
    }

    #[test]
    fn unx11230_checks_green() {
        let s = run_unx11230_checks();
        assert!(s.all_passed(), "UNX-F11230 判据未全绿");
    }
}
