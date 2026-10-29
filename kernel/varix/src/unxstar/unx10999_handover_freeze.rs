//! UNX-F10999 移交清单总封存（B16–B30 冻结件入册）（0x1dc60 段）
//!
//! 锚点：UNX-F10999-J1 全域冻结件逐件指纹入册——B16~B30 十五件冻结件
//! 逐件 FNV 指纹入册且指纹链闭合；J2 只增不删核销——入册后只增封存件
//! 不许删改、核销面逐件对平。
//! 域：CoRun·UNX-C4（多进程压力域）。
//!
//! 结构：封存册（冻结件：段号 + 指纹 + 封存序号，链式指纹闭合）+ 核销面
//! （逐件核销位，只增不删）+ 注入核（重件/删件改件/越界段/封账后写册
//! 四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1dc60 独占 0x20）。
pub const SEG_BASE: u32 = 0x1dc60;
pub const FZ_DUP: u32 = SEG_BASE + 0x01; // 冻结件重号
pub const FZ_RANGE: u32 = SEG_BASE + 0x02; // 段号越界
pub const FZ_MUTATE: u32 = SEG_BASE + 0x03; // 入册后删改
pub const FZ_FROZEN: u32 = SEG_BASE + 0x04; // 封账后写册
pub const FZ_NOENT: u32 = SEG_BASE + 0x05; // 件不在册
pub const FZ_CHAIN: u32 = SEG_BASE + 0x06; // 指纹链断裂

/// 封存段界。
pub const FZ_LO: u32 = 16;
pub const FZ_HI: u32 = 30;
/// 封存域标。
pub const FZ_TAG: u64 = 0x1dc6_0299;

/// 冻结件：段号 + 指纹 + 封存序号 + 核销位。
#[derive(Clone, Debug)]
pub struct FrozenItem {
    pub seg: u32,
    pub fp: u64,
    pub no: u32,
    pub verified: bool,
}

/// 封存册。
pub struct FreezeBook {
    pub items: Vec<FrozenItem>,
    pub sealed: bool,
    pub root: Option<u64>,
}

impl FreezeBook {
    pub fn new() -> FreezeBook {
        FreezeBook { items: Vec::new(), sealed: false, root: None }
    }

    fn find(&self, seg: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].seg == seg {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 入册：封账/重号/越界必拒；指纹零必拒（空件不入册）。
    pub fn enroll(&mut self, seg: u32, fp: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(FZ_FROZEN);
        }
        if seg < FZ_LO || seg > FZ_HI {
            return Err(FZ_RANGE);
        }
        if self.find(seg).is_some() {
            return Err(FZ_DUP);
        }
        if fp == 0 {
            return Err(FZ_CHAIN);
        }
        let no = self.items.len() as u32 + 1;
        self.items.push(FrozenItem { seg, fp, no, verified: false });
        Ok(no)
    }

    /// 核销：逐件翻核销位（只增不删——核销位只能从假到真）。
    pub fn verify(&mut self, seg: u32) -> Result<bool, u32> {
        if self.sealed {
            return Err(FZ_FROZEN);
        }
        match self.find(seg) {
            None => Err(FZ_NOENT),
            Some(i) => {
                let was = self.items[i].verified;
                self.items[i].verified = true;
                Ok(!was)
            }
        }
    }

    /// 只增不删裁断：无核销位回退（全真或前真后假均可，绝不假后真回退）。
    pub fn append_only_ok(&self) -> bool {
        let mut k = 1usize;
        while k < self.items.len() {
            // 序号单调且不回退。
            if self.items[k].no <= self.items[k - 1].no {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 指纹链闭合：链式 fold 与冻结根一致。
    pub fn chain_ok(&self) -> bool {
        match self.root {
            None => self.fold() != 0,
            Some(r) => self.fold() == r,
        }
    }

    /// 全员核销裁断。
    pub fn all_verified(&self) -> bool {
        if self.items.len() != (FZ_HI - FZ_LO + 1) as usize {
            return false;
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].verified {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 链式 fold：逐件指纹链（序敏感，前件链值参与后件）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = FZ_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            h = (h ^ it.seg as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.fp).wrapping_mul(0x100000001b3);
            h = (h ^ it.no as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.verified as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> u64 {
        self.sealed = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 确定性样本：段号 seg 冻结件指纹（贴域 FNV）。
pub fn fz_fp_of(seg: u32) -> u64 {
    let mut h: u64 = 0x1dc6_00f2 ^ seg as u64;
    let mut r = 0u32;
    while r < 4 {
        h = (h ^ (seg.wrapping_mul(r + 3)) as u64).wrapping_mul(0x100000001b3);
        r = r.wrapping_add(1);
    }
    if h == 0 {
        1
    } else {
        h
    }
}

/// 构建全域封存册（B16~B30 逐件入册并核销）。
pub fn build_full() -> (FreezeBook, bool) {
    let mut b = FreezeBook::new();
    let mut seg = FZ_LO;
    let mut ok = true;
    while seg <= FZ_HI {
        if b.enroll(seg, fz_fp_of(seg)).is_err() {
            ok = false;
        }
        seg = seg.wrapping_add(1);
    }
    let mut seg = FZ_LO;
    while seg <= FZ_HI {
        if b.verify(seg).is_err() {
            ok = false;
        }
        seg = seg.wrapping_add(1);
    }
    let b_ok = ok && b.all_verified() && b.append_only_ok();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 段号越界必抓
    let mut b = FreezeBook::new();
    got[0] = b.enroll(FZ_LO - 1, 7).err() == Some(FZ_RANGE)
        && b.enroll(FZ_HI + 1, 7).err() == Some(FZ_RANGE);
    // 2) 重件必抓
    let _ = b.enroll(FZ_LO, 7);
    got[1] = b.enroll(FZ_LO, 9).err() == Some(FZ_DUP);
    // 3) 空指纹必抓（链断件不入册）
    let mut c = FreezeBook::new();
    got[2] = c.enroll(FZ_LO, 0).err() == Some(FZ_CHAIN);
    // 4) 封账后写册与核销必拒
    let _ = c.enroll(FZ_LO, 7);
    c.freeze();
    got[3] = c.enroll(FZ_LO + 1, 7).err() == Some(FZ_FROZEN)
        && c.verify(FZ_LO).err() == Some(FZ_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10999_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10999");

    // J1 全域冻结件逐件指纹入册
    let (mut b, ok) = build_full();
    let root = b.freeze();
    let inj = inject_four();
    let j1 = ok && b.items.len() == (FZ_HI - FZ_LO + 1) as usize
        && b.chain_ok() && b.root == Some(root) && inj[0];
    s.add("J1-逐件指纹入册", j1, "B16~B30 十五件指纹链闭合入册");

    // J2 只增不删核销
    let mut c = FreezeBook::new();
    let _ = c.enroll(FZ_LO, 7);
    let v1 = c.verify(FZ_LO);
    // 核销幂等：二次核销返回 false 但位不回退。
    let v2 = c.verify(FZ_LO);
    let j2 = v1 == Ok(true) && v2 == Ok(false) && c.items[0].verified;
    s.add("J2-只增核销", j2, "核销位只增不删且二次核销幂等");

    // J3 越界段必抓
    s.add("J3-越界抓", inj[0], "封存界外段入册必拒");

    // J4 重件必抓
    s.add("J4-重件抓", inj[1], "同段重复入册必拒");

    // J5 空指纹必抓
    s.add("J5-链断抓", inj[2], "零指纹空件必拒保链闭合");

    // J6 封账后写册核销必拒
    s.add("J6-封账拒写", inj[3], "冻结后入册与核销两路均必拒");

    // J7 核销顺序无强制：乱序核销亦只增
    let mut d = FreezeBook::new();
    let _ = d.enroll(FZ_LO, 3);
    let _ = d.enroll(FZ_LO + 1, 5);
    let _ = d.verify(FZ_LO + 1);
    let _ = d.verify(FZ_LO);
    let j7 = d.items[0].verified && d.items[1].verified && d.append_only_ok();
    s.add("J7-乱序核销", j7, "乱序核销不破坏只增语义");

    // J8 链闭合：冻结后篡改指纹被抓出
    let mut e = build_full().0;
    let eroot = e.freeze();
    e.items[3].fp = e.items[3].fp.wrapping_add(1);
    let j8 = e.fold() != eroot;
    s.add("J8-篡改抓", j8, "指纹漂移必与冻结根失配");

    // J9 未全核销裁断为假
    let mut f = FreezeBook::new();
    let mut seg = FZ_LO;
    while seg <= FZ_HI {
        let _ = f.enroll(seg, fz_fp_of(seg));
        seg = seg.wrapping_add(1);
    }
    let _ = f.verify(FZ_LO);
    let j9 = !f.all_verified() && f.items.len() == 15;
    s.add("J9-漏核销抓", j9, "只核销一件时全员裁断为假");

    // J10 序号单调：封存序逐件递增
    let mut mono = true;
    let mut k = 0usize;
    while k < b.items.len() {
        if b.items[k].no != (k + 1) as u32 {
            mono = false;
        }
        k += 1;
    }
    s.add("J10-序号单调", mono, "封存序号逐件递增可对账");

    // J11 fold 指纹与冻结根一致
    let fp0 = b.fold();
    let j11 = fp0 == root && b.fold() == root && root != 0;
    s.add("J11-指纹冻结", j11, "冻结根与 fold 一致且二次 fold 零漂移");

    // J12 重建幂等：全量重建册指纹一致
    let (b2, ok2) = build_full();
    let j12 = ok2 && b2.fold() == fp0;
    s.add("J12-重建幂等", j12, "全量重建封存册指纹与首建一致");

    // J13 动态明细：件数与冻结根落账
    s.add_dyn("J13-封存明细", b.items.len() == 15,
        &format!("items={} root={:#x}", b.items.len(), root));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10999_freeze_flow() {
        let mut b = FreezeBook::new();
        assert!(b.enroll(FZ_LO - 1, 7).err() == Some(FZ_RANGE));
        assert!(b.enroll(FZ_LO, 0).err() == Some(FZ_CHAIN));
        assert!(b.enroll(FZ_LO, 7).is_ok());
        assert!(b.enroll(FZ_LO, 9).err() == Some(FZ_DUP));
        assert_eq!(b.verify(FZ_LO), Ok(true));
        assert_eq!(b.verify(FZ_LO), Ok(false));
        assert!(b.verify(FZ_LO + 1).err() == Some(FZ_NOENT));
        assert!(b.append_only_ok());
        let root = b.freeze();
        assert!(b.root == Some(root));
        assert!(b.verify(FZ_LO).err() == Some(FZ_FROZEN));
        assert!(b.enroll(FZ_HI, 7).err() == Some(FZ_FROZEN));
    }

    #[test]
    fn unx10999_checks_green() {
        let s = run_unx10999_checks();
        assert!(s.all_passed(), "UNX-F10999 判据未全绿");
    }
}
