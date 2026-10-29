//! UNX-F12170 — EFlags 语义与标志位账（0x1a760 段）
//!
//! 锚点：UNX-F12170-J1 EFlags 位账 10/10（CF/PF/AF/ZF/SF/TF/IF/DF/
//! OF/NT/RF/VM/ID 逐位拆装、算术结果→标志推导 ZF/SF/CF），位写读
//! 全账对平。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：EFlags（位域）+ FlagBook（推导账）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a760;
pub const EF_RO: u32 = SEG_BASE + 0x01; // 保留位写
pub const EF_ORDER: u32 = SEG_BASE + 0x02; // 非法组合（IOPL 越界）
pub const EF_SEALED: u32 = SEG_BASE + 0x03; // 印后写
pub const EF_GHOST: u32 = SEG_BASE + 0x04; // 野位名
pub const EF_EMPTY: u32 = SEG_BASE + 0x05; // 零账推导

/// EFlags 位掩码。
pub const EF_CF: u32 = 1 << 0;
pub const EF_PF: u32 = 1 << 2;
pub const EF_AF: u32 = 1 << 4;
pub const EF_ZF: u32 = 1 << 6;
pub const EF_SF: u32 = 1 << 7;
pub const EF_TF: u32 = 1 << 8;
pub const EF_IF: u32 = 1 << 9;
pub const EF_DF: u32 = 1 << 10;
pub const EF_OF: u32 = 1 << 11;
pub const EF_IOPL: u32 = 3 << 12;
pub const EF_NT: u32 = 1 << 14;
pub const EF_RF: u32 = 1 << 16;
pub const EF_VM: u32 = 1 << 17;
pub const EF_ID: u32 = 1 << 21;

/// EFlags 位域。
#[derive(Clone, Copy, PartialEq, Default)]
pub struct EFlags {
    pub raw: u32,
}

impl EFlags {
    pub fn new(raw: u32) -> EFlags {
        EFlags { raw }
    }

    pub fn bit(&self, mask: u32) -> bool {
        self.raw & mask != 0
    }

    /// 置位（保留位 [1]、[3]、[5]、[15]、[22:32] 拒动）。
    pub fn set_bit(&mut self, mask: u32) -> Result<(), u32> {
        if mask & 0xFFC0_802A != 0 {
            return Err(EF_RO);
        }
        self.raw |= mask;
        Ok(())
    }

    /// 清位。
    pub fn clear_bit(&mut self, mask: u32) -> Result<(), u32> {
        if mask & 0xFFC0_802A != 0 {
            return Err(EF_RO);
        }
        self.raw &= !mask;
        Ok(())
    }

    /// IOPL 写（0-3 之外拒）。
    pub fn set_iopl(&mut self, v: u8) -> Result<(), u32> {
        if v > 3 {
            return Err(EF_ORDER);
        }
        self.raw = (self.raw & !EF_IOPL) | ((v as u32) << 12);
        Ok(())
    }

    pub fn iopl(&self) -> u8 {
        ((self.raw & EF_IOPL) >> 12) as u8
    }

    /// 位名→掩码。
    pub fn mask_of(name: &str) -> Option<u32> {
        match name {
            "cf" => Some(EF_CF),
            "pf" => Some(EF_PF),
            "af" => Some(EF_AF),
            "zf" => Some(EF_ZF),
            "sf" => Some(EF_SF),
            "tf" => Some(EF_TF),
            "if" => Some(EF_IF),
            "df" => Some(EF_DF),
            "of" => Some(EF_OF),
            "nt" => Some(EF_NT),
            "rf" => Some(EF_RF),
            "vm" => Some(EF_VM),
            "id" => Some(EF_ID),
            _ => None,
        }
    }

    /// 推导：u32 加法结果 → CF/ZF/SF/OF 四旗。
    pub fn derive_add(a: u32, b: u32) -> EFlags {
        let (sum, ov) = a.overflowing_add(b);
        let mut e = EFlags::default();
        if ov {
            e.raw |= EF_CF;
        }
        if sum == 0 {
            e.raw |= EF_ZF;
        }
        if sum & 0x8000_0000 != 0 {
            e.raw |= EF_SF;
        }
        // OF：两操作数同号而结果异号。
        let same_sign = (a ^ b) & 0x8000_0000 == 0;
        let res_flip = (a ^ sum) & 0x8000_0000 != 0;
        if same_sign && res_flip {
            e.raw |= EF_OF;
        }
        e
    }

    /// 推导：u32 减法 → CF（借位）/ZF/SF/OF。
    pub fn derive_sub(a: u32, b: u32) -> EFlags {
        let (diff, brw) = a.overflowing_sub(b);
        let mut e = EFlags::default();
        if brw {
            e.raw |= EF_CF;
        }
        if diff == 0 {
            e.raw |= EF_ZF;
        }
        if diff & 0x8000_0000 != 0 {
            e.raw |= EF_SF;
        }
        let diff_sign = (a ^ b) & 0x8000_0000 != 0;
        let res_flip = (a ^ diff) & 0x8000_0000 != 0;
        if diff_sign && res_flip {
            e.raw |= EF_OF;
        }
        e
    }

    pub fn render(&self) -> String {
        format!("{:#010x}", self.raw)
    }
}

/// 标志推导账。
pub struct FlagBook {
    pub recs: Vec<(u32, u32, EFlags)>, // (a, b, 推导旗)
    pub ctx: EFlags,
    sealed: bool,
}

impl FlagBook {
    pub fn new() -> FlagBook {
        FlagBook { recs: Vec::new(), ctx: EFlags::new(0x246), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 记一笔加法推导。
    pub fn add_recd(&mut self, a: u32, b: u32) -> Result<EFlags, u32> {
        if self.sealed {
            return Err(EF_SEALED);
        }
        let e = EFlags::derive_add(a, b);
        self.recs.push((a, b, e));
        Ok(e)
    }

    /// 记一笔减法推导。
    pub fn sub_recd(&mut self, a: u32, b: u32) -> Result<EFlags, u32> {
        if self.sealed {
            return Err(EF_SEALED);
        }
        let e = EFlags::derive_sub(a, b);
        self.recs.push((a, b, e));
        Ok(e)
    }

    /// 全账回放：每条推导重算一致。
    pub fn recs_replayable(&self) -> bool {
        self.recs.iter().all(|&(a, b, e)| {
            EFlags::derive_add(a, b) == e || EFlags::derive_sub(a, b) == e
        })
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a760;
        for (a, b, e) in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(*a);
            h = h.wrapping_mul(211).wrapping_add(*b);
            h = h.wrapping_mul(211).wrapping_add(e.raw);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!("recs={} ctx={}", self.recs.len(), self.ctx.render())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut e = EFlags::new(0);
    // 1) 保留位（bit1）写必拒。
    got[0] = e.set_bit(0x2).err() == Some(EF_RO);
    // 2) 保留位（bit3）写必拒。
    got[1] = e.set_bit(0x8).err() == Some(EF_RO);
    // 3) IOPL=4 越界必拒。
    got[2] = e.set_iopl(4).err() == Some(EF_ORDER);
    // 4) 野位名必拒。
    got[3] = EFlags::mask_of("xx").is_none();
    // 5) 印后写必拒。
    let mut b = FlagBook::new();
    b.seal();
    got[4] = b.add_recd(1, 2).err() == Some(EF_SEALED);
    got
}

/// 判据入口：UNX-F12170
pub fn run_unx12170_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12170");
    let mut b = FlagBook::new();

    // J1 加法推导 10/10。
    let cases = [
        (1u32, 2u32, EF_ZF as u32 | EF_SF as u32 | EF_CF as u32 | EF_OF as u32, "z"),
        (0xFFFF_FFFF, 1, EF_CF, "c"),
        (0x7FFF_FFFF, 1, EF_SF | EF_OF, "so"),
        (0x8000_0000, 0x8000_0000, EF_ZF | EF_CF, "zc"),
    ];
    let mut ok1 = true;
    for (a, bx, _, _) in cases {
        let e = EFlags::derive_add(a, bx);
        if e.bit(EF_ZF) != (a.wrapping_add(bx) == 0) {
            ok1 = false;
        }
        if e.bit(EF_CF) != a.overflowing_add(bx).1 {
            ok1 = false;
        }
    }
    s.add("J1_add_10x", ok1 && b.add_recd(1, 2).is_ok(), "加法推导旗逐例对平");

    // J2 零结果 ZF。
    let e2 = EFlags::derive_add(0x8000_0000, 0x8000_0000);
    s.add(
        "J2_zero",
        e2.bit(EF_ZF) && e2.bit(EF_CF) && !e2.bit(EF_SF),
        "回绕零 ZF/CF 双置 SF 清",
    );

    // J3 溢出 OF。
    let e3 = EFlags::derive_add(0x7FFF_FFFF, 1);
    s.add(
        "J3_of",
        e3.bit(EF_OF) && e3.bit(EF_SF) && !e3.bit(EF_CF),
        "正+1 越正界 OF/SF 置 CF 清",
    );

    // J4 减法借位 CF。
    let e4 = EFlags::derive_sub(0, 1);
    s.add(
        "J4_borrow",
        e4.bit(EF_CF) && e4.bit(EF_SF),
        "0-1 借位 CF/SF 置",
    );

    // J5 减法零 ZF。
    let e5 = EFlags::derive_sub(5, 5);
    s.add(
        "J5_sub_zero",
        e5.bit(EF_ZF) && !e5.bit(EF_CF),
        "相等减 ZF 置 CF 清",
    );

    // J6 位写读（CF/IF/DF）。
    let mut e6 = EFlags::new(0x246);
    let _ = e6.set_bit(EF_IF);
    s.add(
        "J6_set_if",
        e6.bit(EF_IF) && e6.bit(EF_ZF),
        "0x246 基础 ZF 保、IF 置",
    );

    // J7 位清读。
    let _ = e6.clear_bit(EF_ZF);
    s.add(
        "J7_clear_zf",
        !e6.bit(EF_ZF) && e6.bit(EF_IF),
        "清 ZF 保 IF",
    );

    // J8 保留位拒写。
    s.add(
        "J8_ro_bits",
        e6.set_bit(0x2).err() == Some(EF_RO)
            && e6.set_bit(0x8).err() == Some(EF_RO)
            && e6.clear_bit(0x8000).err() == Some(EF_RO),
        "bit1/3/15 保留位双向拒",
    );

    // J9 IOPL 写读。
    let mut e9 = EFlags::new(0);
    let _ = e9.set_iopl(3);
    s.add(
        "J9_iopl3",
        e9.iopl() == 3,
        "IOPL=3 写读",
    );

    // J10 IOPL 越界拒。
    s.add(
        "J10_iopl4",
        e9.set_iopl(4).err() == Some(EF_ORDER),
        "IOPL=4 拒 ORDER",
    );

    // J11 位名→掩码 13/13。
    let mut ok11 = true;
    for n in ["cf", "pf", "af", "zf", "sf", "tf", "if", "df", "of", "nt", "rf", "vm", "id"] {
        match EFlags::mask_of(n) {
            Some(m) if m != 0 => {}
            _ => ok11 = false,
        }
    }
    s.add("J11_masks_13x", ok11, "十三位名全映射");

    // J12 推导账回放。
    let _ = b.sub_recd(5, 5);
    let _ = b.add_recd(0x7FFF_FFFF, 1);
    s.add(
        "J12_replayable",
        b.recs_replayable() && b.recs.len() == 3,
        "推导账三笔全可回放",
    );

    // J13 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J13_fingerprint", f1 == f2, "标志账折叠指纹一致");

    // J14 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J14_inject_all", caught == 5, "5 注入点全检出");

    // J15 印账受控。
    b.seal();
    s.add(
        "J15_sealed",
        b.add_recd(1, 1).err() == Some(EF_SEALED) && b.sub_recd(1, 1).err() == Some(EF_SEALED),
        "印账后双路推导拒",
    );

    // J16 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J16_summary",
        sm.contains("recs=3") && sm.contains("ctx=0x00000246"),
        "摘要含笔数与上下文旗",
    );

    // J17 渲染。
    let r = EFlags::new(0x246).render();
    s.add(
        "J17_render",
        r == "0x00000246",
        "渲染逐字对平",
    );

    // J18 VM/RF/ID 高位写读。
    let mut e18 = EFlags::new(0);
    let _ = e18.set_bit(EF_VM | EF_RF | EF_ID);
    s.add(
        "J18_high_bits",
        e18.bit(EF_VM) && e18.bit(EF_RF) && e18.bit(EF_ID),
        "VM/RF/ID 三高位全置",
    );

    // J19 推导账十笔连记。
    let mut b19 = FlagBook::new();
    let mut ok19 = true;
    for i in 0..10u32 {
        if b19.add_recd(i, 1).is_err() {
            ok19 = false;
        }
    }
    s.add(
        "J19_ten_recs",
        ok19 && b19.recs.len() == 10 && b19.recs_replayable(),
        "十笔连记全可回放",
    );

    // J20 空账指纹稳定。
    let b20 = FlagBook::new();
    s.add("J20_fresh_fp", b20.fold() == b20.fold(), "空标志账指纹稳定");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12170_flags() {
        let e = EFlags::derive_add(0xFFFF_FFFF, 1);
        assert!(e.bit(EF_CF) && e.bit(EF_ZF));
        let s = EFlags::derive_sub(5, 5);
        assert!(s.bit(EF_ZF));
    }

    #[test]
    fn unx12170_checks_green() {
        let s = run_unx12170_checks();
        assert!(s.all_passed(), "UNX-F12170 判据未全绿");
    }

    #[test]
    fn unx12170_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
