//! UNX-F12207 展开器与 D2 装载器联签：函数表来源统一（SEG 0x640e0..0x64100 段窗）
//!
//! 卡面：展开器消费的函数表统一取自 D2 装载器登记面——条目入表、
//! 排序与镜像界拒止、联合指纹（镜像 id × 表目卷积）、重签一致性核账。
//! 域：CoRun·UNX-D1-B11。
//!
//! 判据原文：
//! - J1 条目入表：镜像界内 (begin,end) 升序不重叠入表。
//! - J2 越镜像界拒止：条目越出 [image_lo,image_hi) 必拒 JS_OUT_OF_IMAGE。
//! - J3 乱序拒止：begin 小于前条 end（半重叠/回退）必拒 JS_UNSORTED。
//! - J4 空表拒止：空表联签必拒 JS_EMPTY。
//! - J5 联合指纹：镜像 id 与表目共同参与 211 卷积。
//! - J6 重签一致：同态重签指纹同值。
//! - J7 表变签变：增条后重签指纹必变。
//! - J8 签差拒止：verify 对不上期望必拒 JS_SIG_DIFF。
//! - J9 表目回读：按 begin 可查条目。
//! - J10 印账冻结：seal 后入表/联签必拒 JS_FROZEN。
//! - J11 指纹幂等：同态同指纹。
//! - J12 注入五况全抓。
//!
//! 结构：FuncTableLedger 函数表联签账（镜像界/表目/签态/操作账）+
//! 入表核（add_entry）+ 联签核（co_sign/verify）+ 折叠指纹 + 注入核
//! （越界/乱序/空表/签差/印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x640e0 独占 0x20 窗）。
pub const SEG_BASE: u32 = 0x640e0;
pub const JS_OUT_OF_IMAGE: u32 = SEG_BASE + 0x01; // 越镜像界
pub const JS_UNSORTED: u32 = SEG_BASE + 0x02; // 条目乱序/半重叠
pub const JS_EMPTY: u32 = SEG_BASE + 0x03; // 空表联签
pub const JS_SIG_DIFF: u32 = SEG_BASE + 0x04; // 签差
pub const JS_FROZEN: u32 = SEG_BASE + 0x05; // 印账后操作

/// 函数表条目。
#[derive(Clone, Copy)]
pub struct FuncEntry {
    pub begin: u32,
    pub end: u32,
    pub info: u32, // unwind info 偏移
}

/// 函数表联签账。
pub struct FuncTableLedger {
    pub image_lo: u32,
    pub image_hi: u32,
    pub image_id: u32,
    pub entries: Vec<FuncEntry>,
    pub sig: Option<u32>,
    pub ops: Vec<String>,
    pub sealed: bool,
}

impl FuncTableLedger {
    pub fn new(image_lo: u32, image_hi: u32, image_id: u32) -> FuncTableLedger {
        FuncTableLedger {
            image_lo,
            image_hi,
            image_id,
            entries: Vec::new(),
            sig: None,
            ops: Vec::new(),
            sealed: false,
        }
    }

    /// 入表核：镜像界内、升序不重叠。
    pub fn add_entry(&mut self, e: FuncEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(JS_FROZEN);
        }
        if e.begin >= e.end {
            return Err(JS_UNSORTED);
        }
        if e.begin < self.image_lo || e.end > self.image_hi {
            return Err(JS_OUT_OF_IMAGE);
        }
        if let Some(last) = self.entries.last() {
            if e.begin < last.end {
                return Err(JS_UNSORTED);
            }
        }
        self.entries.push(e);
        self.ops.push(format!("ent{:x}", e.begin));
        Ok(())
    }

    /// 联合指纹：镜像 id × 表目 × 段基 211 卷积。
    pub fn joint_sig(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.image_id);
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.begin);
            h = h.wrapping_mul(211).wrapping_add(e.end);
            h = h.wrapping_mul(211).wrapping_add(e.info);
        }
        h = h.wrapping_mul(211).wrapping_add(self.entries.len() as u32);
        h
    }

    /// 联签核：空表拒止，签态落账。
    pub fn co_sign(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(JS_FROZEN);
        }
        if self.entries.is_empty() {
            return Err(JS_EMPTY);
        }
        let g = self.joint_sig();
        self.sig = Some(g);
        self.ops.push(format!("sig{:x}", g));
        Ok(g)
    }

    /// 验签：与期望签比对，签差必拒。
    pub fn verify(&self, expected: u32) -> Result<(), u32> {
        match self.sig {
            Some(g) if g == expected => Ok(()),
            Some(_) => Err(JS_SIG_DIFF),
            None => Err(JS_SIG_DIFF),
        }
    }

    /// 按 begin 查条目。
    pub fn entry_at(&self, begin: u32) -> Option<FuncEntry> {
        self.entries.iter().find(|e| e.begin == begin).copied()
    }

    /// 覆盖判定：ip 是否落某条目。
    pub fn covers(&self, ip: u32) -> bool {
        self.entries.iter().any(|e| ip >= e.begin && ip < e.end)
    }

    /// 折叠指纹（211 进制卷积，含签态）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE.wrapping_add(self.image_id);
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.begin);
            h = h.wrapping_mul(211).wrapping_add(e.end);
        }
        h = h.wrapping_mul(211).wrapping_add(self.sig.unwrap_or(0));
        for o in &self.ops {
            for by in o.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("ents={} sig={:x}", self.entries.len(), self.sig.unwrap_or(0))
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 越镜像界必拒。
    let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
    got[0] = l.add_entry(FuncEntry { begin: 0x0F00, end: 0x1100, info: 0 })
        .err()
        == Some(JS_OUT_OF_IMAGE);
    // 2) 乱序必拒。
    let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
    let _ = l.add_entry(FuncEntry { begin: 0x1100, end: 0x1200, info: 0 });
    got[1] = l.add_entry(FuncEntry { begin: 0x1150, end: 0x1300, info: 0 })
        .err()
        == Some(JS_UNSORTED);
    // 3) 空表联签必拒。
    got[2] = FuncTableLedger::new(0x1000, 0x2000, 0xD2).co_sign().err() == Some(JS_EMPTY);
    // 4) 签差必拒。
    let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
    let _ = l.add_entry(FuncEntry { begin: 0x1100, end: 0x1200, info: 4 });
    let g = l.co_sign().unwrap_or(0);
    got[3] = l.verify(g.wrapping_add(1)) == Err(JS_SIG_DIFF);
    // 5) 印账后操作必抓。
    let mut f = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
    f.seal();
    got[4] = f.add_entry(FuncEntry { begin: 0x1100, end: 0x1200, info: 0 })
        .err()
        == Some(JS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12207_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12207");

    // J1 条目入表。
    let j1 = {
        let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
        l.add_entry(FuncEntry { begin: 0x1000, end: 0x1100, info: 8 }).is_ok()
            && l.add_entry(FuncEntry { begin: 0x1100, end: 0x1180, info: 12 }).is_ok()
    };
    s.add("J1-条目入表", j1, "升序不重叠入表");

    // J2 越镜像界拒止。
    let j2 = {
        let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
        l.add_entry(FuncEntry { begin: 0x1F00, end: 0x2100, info: 0 }).err()
            == Some(JS_OUT_OF_IMAGE)
    };
    s.add("J2-越镜像界拒止", j2, "end 越界必拒");

    // J3 乱序拒止。
    let j3 = {
        let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
        let _ = l.add_entry(FuncEntry { begin: 0x1200, end: 0x1300, info: 0 });
        l.add_entry(FuncEntry { begin: 0x1000, end: 0x1100, info: 0 }).err() == Some(JS_UNSORTED)
    };
    s.add("J3-乱序拒止", j3, "回退条目必拒");

    // J4 空表拒止。
    let j4 = {
        let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
        l.co_sign().err() == Some(JS_EMPTY)
    };
    s.add("J4-空表拒止", j4, "无条目不得联签");

    // J5 联合指纹参与面。
    let j5 = {
        let mut a = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
        let _ = a.add_entry(FuncEntry { begin: 0x1100, end: 0x1200, info: 4 });
        let mut b = FuncTableLedger::new(0x1000, 0x2000, 0xD3);
        let _ = b.add_entry(FuncEntry { begin: 0x1100, end: 0x1200, info: 4 });
        a.joint_sig() != b.joint_sig()
    };
    s.add("J5-联合指纹", j5, "镜像 id 入签");

    // J6 重签一致。
    let j6 = {
        let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
        let _ = l.add_entry(FuncEntry { begin: 0x1100, end: 0x1200, info: 4 });
        let g1 = l.joint_sig();
        let g2 = l.joint_sig();
        g1 == g2
    };
    s.add("J6-重签一致", j6, "同态同签");

    // J7 表变签变。
    let j7 = {
        let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
        let _ = l.add_entry(FuncEntry { begin: 0x1100, end: 0x1200, info: 4 });
        let g1 = l.joint_sig();
        let _ = l.add_entry(FuncEntry { begin: 0x1200, end: 0x1300, info: 8 });
        l.joint_sig() != g1
    };
    s.add("J7-表变签变", j7, "增条后签必变");

    // J8 验签。
    let j8 = {
        let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
        let _ = l.add_entry(FuncEntry { begin: 0x1100, end: 0x1200, info: 4 });
        let g = l.co_sign().unwrap_or(0);
        l.verify(g).is_ok()
    };
    s.add("J8-验签", j8, "期望签一致通过");

    // J9 表目回读。
    let j9 = {
        let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
        let _ = l.add_entry(FuncEntry { begin: 0x1100, end: 0x1200, info: 4 });
        l.entry_at(0x1100).map(|e| e.info) == Some(4)
            && l.entry_at(0x1999).is_none()
            && l.covers(0x1150)
            && !l.covers(0x1250)
    };
    s.add("J9-表目回读", j9, "按 begin 可查可判覆盖");

    // J10 印账冻结。
    let j10 = {
        let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
        let _ = l.add_entry(FuncEntry { begin: 0x1100, end: 0x1200, info: 4 });
        l.seal();
        l.add_entry(FuncEntry { begin: 0x1200, end: 0x1300, info: 0 }).err() == Some(JS_FROZEN)
            && l.co_sign().err() == Some(JS_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后入表/联签皆拒");

    // J11 指纹幂等。
    let j11 = {
        let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
        let _ = l.add_entry(FuncEntry { begin: 0x1100, end: 0x1200, info: 4 });
        l.fold() == l.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 摘要口径。
    let j12 = {
        let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
        let _ = l.add_entry(FuncEntry { begin: 0x1100, end: 0x1200, info: 4 });
        let _ = l.co_sign();
        l.summary().contains("ents=1")
    };
    s.add("J12-摘要口径", j12, "表目与签可回读");

    let got = inject_five();
    s.add_dyn("J13-注入全抓", got.iter().all(|x| *x), &format!("inj5={:?}", got));
    s.add("J14-指纹非零", {
        let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
        let _ = l.add_entry(FuncEntry { begin: 0x1100, end: 0x1200, info: 4 });
        let _ = l.co_sign();
        l.fold() != 0
    }, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12207_cosign_flow() {
        let mut l = FuncTableLedger::new(0x1000, 0x2000, 0xD2);
        assert!(l.add_entry(FuncEntry { begin: 0x1100, end: 0x1200, info: 4 }).is_ok());
        assert_eq!(l.add_entry(FuncEntry { begin: 0x1150, end: 0x1300, info: 0 }), Err(JS_UNSORTED));
        let g = l.co_sign().unwrap_or(0);
        assert!(l.verify(g).is_ok());
        assert_eq!(l.verify(g.wrapping_add(1)), Err(JS_SIG_DIFF));
    }

    #[test]
    fn unx12207_checks_green() {
        let s = run_unx12207_checks();
        assert!(s.all_passed(), "UNX-F12207 判据未全绿");
    }

    #[test]
    fn unx12207_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
