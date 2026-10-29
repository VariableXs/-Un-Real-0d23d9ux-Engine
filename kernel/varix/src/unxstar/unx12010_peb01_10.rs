//! UNX-F12010 PEB 只读快照页映射与内核唯一写者协议（0x19360 段）
//!
//! 判据原文：PEB 对用户态是只读快照页——快照在取页时冻结值面并记页
//! 纪元；底层 PEB 再写则快照纪元落后即判陈旧（须重取）；快照页自校验
//! 和（折叠指纹）不复即判篡改；用户态写快照必拒；快照只在持有唯一写
//! 者令牌时可取。
//!
//! 结构：PEB 底账（底值槽 + 写纪元）+ 快照页（SnapPage：值面 + 纪元 +
//! 自校验和）+ 取页核/读核/写核（五码守卫）+ 折叠指纹 + 印账冻结 +
//! 注入核 + 判据聚合（run_unx12010_checks）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19360 独占 0x20）。
pub const SEG_BASE: u32 = 0x19360;
pub const SN_NO_WRITER: u32 = SEG_BASE + 0x01; // 无写者令牌取页
pub const SN_STALE: u32 = SEG_BASE + 0x02; // 快照纪元落后
pub const SN_TAMPER: u32 = SEG_BASE + 0x03; // 快照页自校验不符
pub const SN_USER_WRITE: u32 = SEG_BASE + 0x04; // 用户态写快照
pub const SN_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 内核唯一写者令牌。
pub const PEB_WRITER: u32 = 0x5045_4257; // "PEBW"

/// PEB 底账模型（键值对 + 单调纪元）。
pub struct PebLedger {
    pub vals: Vec<(u32, u64)>, // (键, 值)
    pub epoch: u32,
    pub writer: u32,
    pub sealed: bool,
}

impl PebLedger {
    pub fn new() -> PebLedger {
        PebLedger { vals: Vec::new(), epoch: 1, writer: 0, sealed: false }
    }

    pub fn attach_writer(&mut self, token: u32) {
        self.writer = token;
    }

    /// 内核写：唯一写者令牌校验后落值推进纪元。
    pub fn kern_write(&mut self, k: u32, v: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(SN_FROZEN);
        }
        if self.writer != PEB_WRITER {
            return Err(SN_NO_WRITER);
        }
        if let Some(slot) = self.vals.iter_mut().find(|s| s.0 == k) {
            slot.1 = v;
        } else {
            self.vals.push((k, v));
        }
        self.epoch += 1;
        Ok(())
    }

    pub fn read(&self, k: u32) -> Option<u64> {
        self.vals.iter().find(|s| s.0 == k).map(|s| s.1)
    }
}

/// 快照页：取页时冻结的只读值面。
#[derive(Clone)]
pub struct SnapPage {
    pub vals: Vec<(u32, u64)>,
    /// 取页时底账纪元。
    pub epoch: u32,
    /// 取页时算出的自校验和。
    pub checksum: u32,
    pub sealed: bool,
}

/// 快照自校验和：211 进制卷积，种子=段位+纪元。
pub fn snap_checksum(vals: &[(u32, u64)], epoch: u32) -> u32 {
    let mut h: u32 = SEG_BASE.wrapping_add(epoch);
    for (k, v) in vals {
        h = h.wrapping_mul(211).wrapping_add(*k);
        h = h.wrapping_mul(211).wrapping_add((v & 0xFFFF) as u32);
        h = h.wrapping_mul(211).wrapping_add((v >> 16) as u32);
    }
    h
}

impl SnapPage {
    /// 取页核：须持唯一写者令牌（协议上取页在内核路径内完成）。
    pub fn take(base: &PebLedger, writer: u32) -> Result<SnapPage, u32> {
        if writer != PEB_WRITER {
            return Err(SN_NO_WRITER);
        }
        let page = SnapPage {
            vals: base.vals.clone(),
            epoch: base.epoch,
            checksum: snap_checksum(&base.vals, base.epoch),
            sealed: false,
        };
        Ok(page)
    }

    /// 读核：先验自校验和（篡改检出），再验纪元（陈旧检出）。
    pub fn verify(&self, base: &PebLedger) -> Result<(), u32> {
        if self.sealed {
            return Err(SN_FROZEN);
        }
        if snap_checksum(&self.vals, self.epoch) != self.checksum {
            return Err(SN_TAMPER);
        }
        if self.epoch < base.epoch {
            return Err(SN_STALE);
        }
        Ok(())
    }

    /// 快照内读值。
    pub fn read(&self, k: u32) -> Option<u64> {
        self.vals.iter().find(|s| s.0 == k).map(|s| s.1)
    }

    /// 用户态写快照：永不合法——恒拒 SN_USER_WRITE。
    pub fn user_write(&mut self, _k: u32, _v: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(SN_FROZEN);
        }
        Err(SN_USER_WRITE)
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.checksum
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut base = PebLedger::new();
    base.attach_writer(PEB_WRITER);
    let _ = base.kern_write(0x10, 0x400000);
    // 1) 无令牌取页 → SN_NO_WRITER。
    got[0] = SnapPage::take(&base, 0).is_err();
    // 2) 陈旧快照：取页后底账再写 → verify 判 SN_STALE。
    let mut page = SnapPage::take(&base, PEB_WRITER).unwrap_or(SnapPage {
        vals: Vec::new(),
        epoch: 0,
        checksum: 0,
        sealed: false,
    });
    let _ = base.kern_write(0x30, 0x260000);
    got[1] = page.verify(&base).err() == Some(SN_STALE);
    // 3) 篡改快照：改页值不改校验和 → SN_TAMPER。
    if let Some(slot) = page.vals.iter_mut().find(|s| s.0 == 0x10) {
        slot.1 = 0xDEAD;
    }
    got[2] = page.verify(&base).err() == Some(SN_TAMPER);
    // 4) 用户态写快照 → SN_USER_WRITE。
    let mut fresh = PebLedger::new();
    fresh.attach_writer(PEB_WRITER);
    let mut p2 = SnapPage::take(&fresh, PEB_WRITER)
        .unwrap_or(SnapPage { vals: Vec::new(), epoch: 0, checksum: 0, sealed: false });
    got[3] = p2.user_write(0x10, 1).err() == Some(SN_USER_WRITE);
    // 5) 印账后动作 → SN_FROZEN。
    p2.seal();
    got[4] = p2.user_write(0x10, 2).err() == Some(SN_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12010_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12010");

    // J1 正路径：取页→读值一致→verify 过（无陈旧无篡改）。
    let j1 = {
        let mut base = PebLedger::new();
        base.attach_writer(PEB_WRITER);
        let _ = base.kern_write(0x10, 0x400000);
        let _ = base.kern_write(0x30, 0x260000);
        let page = SnapPage::take(&base, PEB_WRITER).ok();
        match page {
            Some(p) => {
                p.verify(&base).is_ok()
                    && p.read(0x10) == Some(0x400000)
                    && p.read(0x30) == Some(0x260000)
            }
            None => false,
        }
    };
    s.add("J1-取页读值一致", j1, "快照面与底账对平");

    // J2 无令牌取页拒：伪令牌同拒。
    let j2 = {
        let mut base = PebLedger::new();
        base.attach_writer(PEB_WRITER);
        SnapPage::take(&base, PEB_WRITER + 1).err() == Some(SN_NO_WRITER)
    };
    s.add("J2-无令牌取页拒", j2, "非 PEB_WRITER 必拒");

    // J3 底账写须令牌：无令牌写拒。
    let j3 = {
        let mut base = PebLedger::new();
        base.kern_write(0x10, 1).err() == Some(SN_NO_WRITER)
    };
    s.add("J3-无令牌写拒", j3, "底账写同样守令牌");

    // J4 陈旧检出：底账推进后旧页判陈旧，重取后过。
    let j4 = {
        let mut base = PebLedger::new();
        base.attach_writer(PEB_WRITER);
        let _ = base.kern_write(0x10, 1);
        let old = SnapPage::take(&base, PEB_WRITER).ok();
        let _ = base.kern_write(0x10, 2);
        let new = SnapPage::take(&base, PEB_WRITER).ok();
        match (old, new) {
            (Some(o), Some(n)) => o.verify(&base).err() == Some(SN_STALE) && n.verify(&base).is_ok(),
            _ => false,
        }
    };
    s.add("J4-陈旧检出重取", j4, "旧页陈旧新页过");

    // J5 篡改检出：改页值不改校验和必红。
    let j5 = {
        let mut base = PebLedger::new();
        base.attach_writer(PEB_WRITER);
        let _ = base.kern_write(0x10, 7);
        let mut page = SnapPage::take(&base, PEB_WRITER)
            .unwrap_or(SnapPage { vals: Vec::new(), epoch: 0, checksum: 0, sealed: false });
        if let Some(slot) = page.vals.iter_mut().find(|s| s.0 == 0x10) {
            slot.1 = 8;
        }
        page.verify(&base).err() == Some(SN_TAMPER)
    };
    s.add("J5-篡改检出", j5, "自校验和不复必红");

    // J6 用户态写快照拒。
    let j6 = {
        let mut base = PebLedger::new();
        base.attach_writer(PEB_WRITER);
        let mut page = SnapPage::take(&base, PEB_WRITER)
            .unwrap_or(SnapPage { vals: Vec::new(), epoch: 0, checksum: 0, sealed: false });
        page.user_write(0x10, 9).err() == Some(SN_USER_WRITE)
    };
    s.add("J6-用户写快照拒", j6, "快照只读");

    // J7 纪元单调：底账每次写纪元 +1。
    let j7 = {
        let mut base = PebLedger::new();
        base.attach_writer(PEB_WRITER);
        let e0 = base.epoch;
        let _ = base.kern_write(0x10, 1);
        let e1 = base.epoch;
        let _ = base.kern_write(0x10, 2);
        e0 + 2 == base.epoch && e1 == e0 + 1
    };
    s.add("J7-纪元单调", j7, "逐写 +1 对账");

    // J8 快照值面冻结语义：底账后续写不回流旧页（取页即冻结）。
    let j8 = {
        let mut base = PebLedger::new();
        base.attach_writer(PEB_WRITER);
        let _ = base.kern_write(0x10, 1);
        let page = SnapPage::take(&base, PEB_WRITER).ok();
        let _ = base.kern_write(0x10, 2);
        match page {
            Some(p) => p.read(0x10) == Some(1),
            None => false,
        }
    };
    s.add("J8-取页即冻结", j8, "旧页值不回流");

    // J9 指纹幂等与区分度。
    let j9 = {
        let mut base = PebLedger::new();
        base.attach_writer(PEB_WRITER);
        let _ = base.kern_write(0x10, 1);
        let c1 = snap_checksum(&base.vals, base.epoch);
        let c2 = snap_checksum(&base.vals, base.epoch);
        let mut other = PebLedger::new();
        other.attach_writer(PEB_WRITER);
        let _ = other.kern_write(0x10, 2);
        c1 == c2 && c1 != snap_checksum(&other.vals, other.epoch)
    };
    s.add("J9-校验和性质", j9, "幂等且异面异纹");

    // J10 印账冻结。
    let j10 = {
        let mut base = PebLedger::new();
        base.attach_writer(PEB_WRITER);
        let mut page = SnapPage::take(&base, PEB_WRITER)
            .unwrap_or(SnapPage { vals: Vec::new(), epoch: 0, checksum: 0, sealed: false });
        page.seal();
        page.user_write(0x10, 1).err() == Some(SN_FROZEN)
    };
    s.add("J10-印账冻结", j10, "印后动作必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：双键底账。
    let mut base = PebLedger::new();
    base.attach_writer(PEB_WRITER);
    let _ = base.kern_write(0x10, 0x400000);
    let _ = base.kern_write(0x118, 10);
    let page = SnapPage::take(&base, PEB_WRITER);
    s.add("样例快照校验", page.as_ref().map(|p| p.verify(&base).is_ok()).unwrap_or(false), "快照过核");
    s.add("样例双键在页", page.map(|p| p.vals.len() == 2).unwrap_or(false), "两键冻结");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12010_snapshot_flow() {
        let mut base = PebLedger::new();
        base.attach_writer(PEB_WRITER);
        assert!(base.kern_write(0x10, 5).is_ok());
        let page = SnapPage::take(&base, PEB_WRITER);
        assert!(page.is_ok());
        let mut page = page.unwrap_or(SnapPage { vals: Vec::new(), epoch: 0, checksum: 0, sealed: false });
        assert!(page.verify(&base).is_ok());
        assert_eq!(page.user_write(0x10, 6), Err(SN_USER_WRITE));
        assert!(base.kern_write(0x10, 7).is_ok());
        assert_eq!(page.verify(&base), Err(SN_STALE));
    }

    #[test]
    fn unx12010_checks_green() {
        let s = run_unx12010_checks();
        assert!(s.all_passed(), "UNX-F12010 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12010_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
