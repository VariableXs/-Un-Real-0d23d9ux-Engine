//! UNX-F12011 TEB Self 指针与 gs 段基址挂点初始化（0x19380 段）
//!
//! 判据原文：gs 段基址挂点双态轮转——用户态基址必须指向本线程 TEB
//! （Self 自指锚），内核态基址指向内核每处理器槽；双基址不得相同；
//! 向当前已处状态的切换是空操作冲突（须先反向）；双态零基址皆非法。
//!
//! 结构：GS 槽（GsSlot：user_base/kernel_base/active 态）+ 切换核
//! （五码守卫）+ 切换账（GsRec 逐次留痕）+ 折叠指纹 + 印账冻结 +
//! 注入核 + 判据聚合（run_unx12011_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19380 独占 0x20）。
pub const SEG_BASE: u32 = 0x19380;
pub const GS_ZERO_BASE: u32 = SEG_BASE + 0x01; // 零基址
pub const GS_SAME_BASE: u32 = SEG_BASE + 0x02; // 双基址相同
pub const GS_DOUBLE_SWAP: u32 = SEG_BASE + 0x03; // 向当前态切换
pub const GS_BAD_SELF: u32 = SEG_BASE + 0x04; // 用户基址 ≠ TEB 自指锚
pub const GS_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍切换

/// gs 段活动态。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GsSide {
    User = 1,
    Kernel = 2,
}

/// 一次切换的账目。
#[derive(Clone)]
pub struct GsRec {
    pub seq: u32,
    pub from: GsSide,
    pub to: GsSide,
    pub note: String,
}

impl GsRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// GS 基址槽模型。
pub struct GsSlot {
    pub user_base: u64,
    pub kernel_base: u64,
    pub teb_addr: u64,
    pub active: GsSide,
    pub ledger: Vec<GsRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl GsSlot {
    /// 建槽：初始内核态（内核初始化先挂内核基址）。
    pub fn new(kernel_base: u64, teb_addr: u64) -> GsSlot {
        GsSlot {
            user_base: 0,
            kernel_base,
            teb_addr,
            active: GsSide::Kernel,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 挂用户基址：必须等于 TEB 自指锚且非零且异于内核基址。
    pub fn attach_user_base(&mut self, base: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(GS_FROZEN);
        }
        if base == 0 {
            return Err(GS_ZERO_BASE);
        }
        if base != self.teb_addr {
            return Err(GS_BAD_SELF);
        }
        if base == self.kernel_base {
            return Err(GS_SAME_BASE);
        }
        self.user_base = base;
        Ok(())
    }

    /// 切换核：向当前态切换即空操作冲突。
    pub fn swap_to(&mut self, to: GsSide, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(GS_FROZEN);
        }
        if note.is_empty() {
            return Err(GS_ZERO_BASE);
        }
        if to == self.active {
            return Err(GS_DOUBLE_SWAP);
        }
        // 切用户态前用户基址必须已挂。
        if to == GsSide::User && self.user_base == 0 {
            return Err(GS_ZERO_BASE);
        }
        let from = self.active;
        self.active = to;
        let rec = GsRec { seq: self.seq_next, from, to, note: note.to_string() };
        if !rec.ok() {
            return Err(GS_ZERO_BASE);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        Ok(())
    }

    /// 当前生效基址。
    pub fn current_base(&self) -> u64 {
        match self.active {
            GsSide::User => self.user_base,
            GsSide::Kernel => self.kernel_base,
        }
    }

    /// 自指锚核对：用户基址精确等于 TEB 地址。
    pub fn self_anchor_ok(&self) -> bool {
        self.user_base == self.teb_addr && self.user_base != 0
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add((self.user_base & 0xFFFF) as u32);
        h = h.wrapping_mul(211).wrapping_add((self.kernel_base & 0xFFFF) as u32);
        h = h.wrapping_mul(211).wrapping_add(self.active as u32);
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let teb = 0x7EF_50000u64;
    let kgs = 0xFFFF_F800_0000_0000u64;
    let mut got = [false; 5];
    // 1) 零基址挂用户 → GS_ZERO_BASE。
    let mut a = GsSlot::new(kgs, teb);
    got[0] = a.attach_user_base(0).err() == Some(GS_ZERO_BASE);
    // 2) 用户基址错挂（≠ TEB）→ GS_BAD_SELF。
    let mut b = GsSlot::new(kgs, teb);
    got[1] = b.attach_user_base(teb + 0x10).err() == Some(GS_BAD_SELF);
    // 3) 双基址相同 → GS_SAME_BASE。
    let mut c = GsSlot::new(teb, teb);
    got[2] = c.attach_user_base(teb).err() == Some(GS_SAME_BASE);
    // 4) 向当前态切换 → GS_DOUBLE_SWAP。
    let mut d = GsSlot::new(kgs, teb);
    got[3] = d.swap_to(GsSide::Kernel, "same").err() == Some(GS_DOUBLE_SWAP);
    // 5) 未挂用户基址先切用户 → GS_ZERO_BASE（切换面零基址守卫）。
    let mut e = GsSlot::new(kgs, teb);
    got[4] = e.swap_to(GsSide::User, "no-attach").err() == Some(GS_ZERO_BASE);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12011_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12011");

    // J1 正路径：挂用户基址→切用户→切内核→切用户。
    let j1 = {
        let teb = 0x7EF_60000u64;
        let kgs = 0xFFFF_F800_0000_1000u64;
        let mut g = GsSlot::new(kgs, teb);
        let attach = g.attach_user_base(teb).is_ok();
        let to_u = g.swap_to(GsSide::User, "syscall-ret").is_ok();
        let bu = g.current_base() == teb;
        let to_k = g.swap_to(GsSide::Kernel, "syscall").is_ok();
        let bk = g.current_base() == kgs;
        attach && to_u && bu && to_k && bk && g.self_anchor_ok()
    };
    s.add("J1-双态轮转", j1, "基址随态切换");

    // J2 零基址挂用户拒。
    let j2 = {
        let mut g = GsSlot::new(0xFFFF_F800_0000_2000, 0x7EF_61000);
        g.attach_user_base(0).err() == Some(GS_ZERO_BASE)
    };
    s.add("J2-零基址拒", j2, "零基址永不合法");

    // J3 自指锚精确性：差一字节即拒。
    let j3 = {
        let teb = 0x7EF_62000u64;
        let mut g = GsSlot::new(0xFFFF_F800_0000_3000, teb);
        g.attach_user_base(teb - 1).err() == Some(GS_BAD_SELF)
    };
    s.add("J3-自指锚差一拒", j3, "Self 必须精确");

    // J4 双基址相同拒。
    let j4 = {
        let teb = 0x7EF_63000u64;
        let mut g = GsSlot::new(teb, teb);
        g.attach_user_base(teb).err() == Some(GS_SAME_BASE)
    };
    s.add("J4-双基相同拒", j4, "用户内核基址互斥");

    // J5 向当前态切换拒（双面：Kernel→Kernel 已验，User→User 补验）。
    let j5 = {
        let teb = 0x7EF_64000u64;
        let mut g = GsSlot::new(0xFFFF_F800_0000_4000, teb);
        let _ = g.attach_user_base(teb);
        let _ = g.swap_to(GsSide::User, "u");
        g.swap_to(GsSide::User, "u2").err() == Some(GS_DOUBLE_SWAP)
    };
    s.add("J5-重复切换拒", j5, "空操作切换必拒");

    // J6 未挂用户基址切用户拒。
    let j6 = {
        let mut g = GsSlot::new(0xFFFF_F800_0000_5000, 0x7EF_65000);
        g.swap_to(GsSide::User, "e").err() == Some(GS_ZERO_BASE)
    };
    s.add("J6-未挂先切拒", j6, "切用户前必挂基址");

    // J7 切换账：seq 递增且逐笔合法。
    let j7 = {
        let teb = 0x7EF_66000u64;
        let mut g = GsSlot::new(0xFFFF_F800_0000_6000, teb);
        let _ = g.attach_user_base(teb);
        let _ = g.swap_to(GsSide::User, "a");
        let _ = g.swap_to(GsSide::Kernel, "b");
        let _ = g.swap_to(GsSide::User, "c");
        g.ledger.len() == 3 && g.ledger_ok() && g.ledger[2].seq == 3
    };
    s.add("J7-切换账三笔", j7, "seq=3 对账");

    // J8 建槽初始态：内核态起、用户基址未挂。
    let j8 = {
        let g = GsSlot::new(0xFFFF_F800_0000_7000, 0x7EF_67000);
        g.active == GsSide::Kernel && g.user_base == 0 && !g.self_anchor_ok()
    };
    s.add("J8-初始内核态", j8, "建槽先内核");

    // J9 印账冻结。
    let j9 = {
        let teb = 0x7EF_68000u64;
        let mut g = GsSlot::new(0xFFFF_F800_0000_8000, teb);
        let f1 = g.fold();
        let f2 = g.fold();
        g.seal();
        f1 == f2
            && g.attach_user_base(teb).err() == Some(GS_FROZEN)
            && g.swap_to(GsSide::User, "post").err() == Some(GS_FROZEN)
    };
    s.add("J9-指纹封账", j9, "双面操作印后全拒");

    // J10 摘要面指纹区分度。
    let j10 = {
        let a = GsSlot::new(0xFFFF_F800_0000_9000, 0x7EF_69000);
        let b = GsSlot::new(0xFFFF_F800_0000_A000, 0x7EF_6A000);
        a.fold() != b.fold()
    };
    s.add("J10-异槽异纹", j10, "不同槽指纹必异");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：一次完整系统调用往返。
    let teb = 0x7EF_6B000u64;
    let mut g = GsSlot::new(0xFFFF_F800_0000_B000, teb);
    let _ = g.attach_user_base(teb);
    let _ = g.swap_to(GsSide::User, "ret");
    let _ = g.swap_to(GsSide::Kernel, "ent");
    s.add("样例往返账", g.ledger.len() == 2, "两笔切换");
    s.add("样例账面合法", g.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12011_swap_flow() {
        let teb = 0x7EF_70000u64;
        let mut g = GsSlot::new(0xFFFF_F800_0000_C000, teb);
        assert!(g.attach_user_base(teb).is_ok());
        assert_eq!(g.swap_to(GsSide::Kernel, "same"), Err(GS_DOUBLE_SWAP));
        assert!(g.swap_to(GsSide::User, "u").is_ok());
        assert_eq!(g.current_base(), teb);
        assert!(g.swap_to(GsSide::Kernel, "k").is_ok());
        assert_eq!(g.current_base(), 0xFFFF_F800_0000_C000);
    }

    #[test]
    fn unx12011_checks_green() {
        let s = run_unx12011_checks();
        assert!(s.all_passed(), "UNX-F12011 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12011_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
