//! UNX-F11108 冻结件清单总核（域冻结件全量在册）（0x1f588 段）
//!
//! 锚点：UNX-F11108-J1 冻结件全量终核对平（10/10），版本戳零漂移（10/10）。
//! 域：CoRun·UNX-C4-B36（冻结件清单）。
//!
//! 结构：冻结件册（FreezeRoll：逐件登记名/摘要，版本戳按册基值顺排 →
//! 漂移判定 → 复核 → 终核）+ 单件（FreezeArtifact：名 + 戳 + 摘要）+ 冻结根
//! + 注入核（空名/重名/零摘要/戳漂移四路必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1f588 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f588;
pub const FZ_NAME: u32 = SEG_BASE + 0x01; // 件名为空
pub const FZ_DUP: u32 = SEG_BASE + 0x02; // 件名重号
pub const FZ_DIGEST: u32 = SEG_BASE + 0x03; // 摘要为零
pub const FZ_DRIFT: u32 = SEG_BASE + 0x04; // 版本戳漂移
pub const FZ_VERIFY: u32 = SEG_BASE + 0x05; // 复核摘要不符
pub const FZ_SHORT: u32 = SEG_BASE + 0x06; // 件数不足
pub const FZ_FROZEN: u32 = SEG_BASE + 0x07; // 冻结后写账
pub const FZ_OVER: u32 = SEG_BASE + 0x08; // 超定容

/// 域冻结件数（12）。
pub const NEEDED: usize = 12;
/// 冻结件册定容。
pub const CAP: usize = 16;
/// 版本戳册基值。
pub const STAMP_BASE: u32 = 0x1f58_0000;
/// 冻结册标（fold 种子）。
pub const FZ_TAG: u64 = 0x1f5_8888;
/// 指纹步进。
pub const FP_STEP: u64 = 0x100000001b3;

/// 单件：名 + 版本戳 + 摘要。
#[derive(Clone, Debug, PartialEq)]
pub struct FreezeArtifact {
    pub name: String,
    pub stamp: u32,
    pub digest: u64,
}

impl FreezeArtifact {
    pub fn render(&self) -> String {
        format!("{}@{:#x}|{:#x}", self.name, self.stamp, self.digest)
    }
}

/// 冻结件册：按登记序发戳，漂移即戳不等于册基 + 序位。
#[derive(Clone, Debug)]
pub struct FreezeRoll {
    pub items: Vec<FreezeArtifact>,
    pub base: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl FreezeRoll {
    pub fn new(base: u32) -> FreezeRoll {
        FreezeRoll { items: Vec::new(), base, frozen: false, root: None }
    }

    /// 登记：冻结/空名/重名/零摘要/超容皆拒；戳 = 基值 + 序位。
    pub fn register(&mut self, name: &str, digest: u64) -> Result<u32, u32> {
        if self.frozen {
            return Err(FZ_FROZEN);
        }
        if name.is_empty() {
            return Err(FZ_NAME);
        }
        if self.items.iter().any(|a| a.name == name) {
            return Err(FZ_DUP);
        }
        if digest == 0 {
            return Err(FZ_DIGEST);
        }
        if self.items.len() >= CAP {
            return Err(FZ_OVER);
        }
        let seq = (self.items.len() as u32).wrapping_add(1);
        let stamp = self.base.wrapping_add(seq);
        self.items.push(FreezeArtifact { name: name.to_string(), stamp, digest });
        Ok(stamp)
    }

    /// 期望戳：册基值 + 序位（1 起）。
    pub fn expect_stamp(&self, idx: usize) -> Option<u32> {
        if idx >= self.items.len() {
            return None;
        }
        Some(self.base.wrapping_add((idx as u32).wrapping_add(1)))
    }

    /// 漂移件数：戳偏离册基顺排的件。
    pub fn drift(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.items.len() {
            match self.expect_stamp(k) {
                Some(s) => {
                    if self.items[k].stamp != s {
                        n += 1;
                    }
                }
                None => n += 1,
            }
            k += 1;
        }
        n
    }

    /// 漂移点名。
    pub fn drifted_names(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            let bad = match self.expect_stamp(k) {
                Some(s) => self.items[k].stamp != s,
                None => true,
            };
            if bad {
                v.push(self.items[k].name.clone());
            }
            k += 1;
        }
        v
    }

    /// 戳唯一性。
    pub fn stamps_unique(&self) -> bool {
        let mut k = 0usize;
        while k < self.items.len() {
            let mut j = k + 1;
            while j < self.items.len() {
                if self.items[j].stamp == self.items[k].stamp {
                    return false;
                }
                j += 1;
            }
            k += 1;
        }
        true
    }

    /// 复核：按名查件并比对摘要；查无此件或摘要不符皆拒。
    pub fn verify(&self, name: &str, digest: u64) -> Result<u32, u32> {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].name == name {
                if self.items[k].digest != digest {
                    return Err(FZ_VERIFY);
                }
                return Ok(self.items[k].stamp);
            }
            k += 1;
        }
        Err(FZ_NAME)
    }

    /// 全量终核：12 件、零漂移、戳唯一、无不符。
    pub fn closed(&self) -> bool {
        self.items.len() == NEEDED && self.drift() == 0 && self.stamps_unique()
    }

    /// 冻结并落根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 册级指纹：名 + 戳 + 摘要链式入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = FZ_TAG;
        h = (h ^ self.base as u64).wrapping_mul(FP_STEP);
        let mut k = 0usize;
        while k < self.items.len() {
            let a = &self.items[k];
            h = (h ^ a.stamp as u64).wrapping_mul(FP_STEP);
            h = (h ^ a.digest).wrapping_mul(FP_STEP);
            let bytes = a.name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            k += 1;
        }
        h
    }
}

/// 件名样本（确定性）。
pub fn artifact_name(seq: u32) -> String {
    format!("冻结件 {:02}", seq)
}

/// 摘要样本：非零且序号敏感。
pub fn artifact_digest(seq: u32) -> u64 {
    0x9e37_79b9u64.wrapping_mul(seq as u64).wrapping_add(1)
}

/// 标准册：12 件按序发戳。
pub fn build_roll() -> (FreezeRoll, bool) {
    let mut r = FreezeRoll::new(STAMP_BASE);
    let mut ok = true;
    let mut seq = 1u32;
    while seq <= NEEDED as u32 {
        if r.register(&artifact_name(seq), artifact_digest(seq)).is_err() {
            ok = false;
        }
        seq += 1;
    }
    (r, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 空名必拒
    let mut a = FreezeRoll::new(STAMP_BASE);
    got[0] = a.register("", 7).err() == Some(FZ_NAME);
    // 2) 重名必拒
    let mut b = FreezeRoll::new(STAMP_BASE);
    let _ = b.register("甲", 7);
    got[1] = b.register("甲", 8).err() == Some(FZ_DUP);
    // 3) 零摘要必拒
    let mut c = FreezeRoll::new(STAMP_BASE);
    got[2] = c.register("零摘要", 0).err() == Some(FZ_DIGEST);
    // 4) 戳漂移必检出（手改首件戳 +1）
    let (mut d, _) = build_roll();
    if let Some(first) = d.items.first_mut() {
        first.stamp = first.stamp.wrapping_add(1);
    }
    got[3] = d.drift() == 1 && !d.closed() && d.drifted_names().len() == 1;
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11108_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11108");

    let (mut roll, built) = build_roll();
    // J1 冻结件全量终核对平
    s.add(
        "J1-冻结件全量对平",
        built && roll.items.len() == NEEDED && roll.closed(),
        "12 件冻结件逐件在册且终核闭合",
    );

    // J2 版本戳零漂移
    s.add(
        "J2-版本戳零漂移",
        roll.drift() == 0 && roll.stamps_unique() && roll.drifted_names().is_empty(),
        "戳按册基顺排且无重复",
    );

    // J3 首件戳定值
    let j3 = roll.items.first().map(|a| a.stamp) == Some(STAMP_BASE + 1)
        && roll.expect_stamp(0) == Some(STAMP_BASE + 1);
    s.add("J3-首件戳定值", j3, "首件戳 = 册基 + 1");

    // J4 空名拒
    let j4 = {
        let mut c = FreezeRoll::new(STAMP_BASE);
        c.register("", 1).err() == Some(FZ_NAME) && c.items.is_empty()
    };
    s.add("J4-空名拒", j4, "空件名不得入册");

    // J5 重名拒
    let j5 = {
        let mut c = FreezeRoll::new(STAMP_BASE);
        let _ = c.register("甲", 1);
        c.register("甲", 2).err() == Some(FZ_DUP)
    };
    s.add("J5-重名拒", j5, "同名二次登记必拒");

    // J6 零摘要拒
    let j6 = {
        let mut c = FreezeRoll::new(STAMP_BASE);
        c.register("乙", 0).err() == Some(FZ_DIGEST)
    };
    s.add("J6-零摘要拒", j6, "摘要为零不得入册");

    // J7 复核摘要不符拒
    let j7 = {
        let (c, _) = build_roll();
        c.verify(&artifact_name(1), artifact_digest(1)).is_ok()
            && c.verify(&artifact_name(1), artifact_digest(2)).err() == Some(FZ_VERIFY)
            && c.verify("无件", 1).err() == Some(FZ_NAME)
    };
    s.add("J7-复核三态", j7, "复核命中/不符/无件三路可判");

    // J8 戳漂移检出
    let j8 = {
        let (mut c, _) = build_roll();
        if let Some(last) = c.items.last_mut() {
            last.stamp = last.stamp.wrapping_add(9);
        }
        c.drift() == 1 && !c.closed() && c.drifted_names().len() == 1
    };
    s.add("J8-漂移检出", j8, "单件戳偏移即漂移计数为 1");

    // J9 注入检出：空名 + 重名
    let inj = inject_four();
    s.add("J9-空名重名检出", inj[0] && inj[1], "空名与重名两路必抓");

    // J10 注入检出：零摘要 + 漂移
    s.add("J10-零摘要漂移检出", inj[2] && inj[3], "零摘要与戳漂移两路必抓");

    // J11 件数不足不闭合
    let j11 = {
        let mut c = FreezeRoll::new(STAMP_BASE);
        let mut seq = 1u32;
        while seq < NEEDED as u32 {
            let _ = c.register(&artifact_name(seq), artifact_digest(seq));
            seq += 1;
        }
        !c.closed() && c.drift() == 0 && c.items.len() == NEEDED - 1
    };
    s.add("J11-缺件不闭合", j11, "少一件即终核为假");

    // J12 冻结拒写
    let root = roll.seal();
    let j12 = roll.register("追件", 5).err() == Some(FZ_FROZEN);
    s.add("J12-冻结拒写", j12, "封存后登记必拒");

    // J13 冻结根稳定
    s.add(
        "J13-冻结根稳定",
        roll.root == Some(root) && root == roll.fold() && root != 0,
        "冻结册根可重算零漂移",
    );

    // J14 动态明细
    s.add_dyn(
        "J14-冻结明细",
        roll.closed() && roll.fold() == root,
        &format!(
            "items={} drift={} base={:#x} root={:#x}",
            roll.items.len(),
            roll.drift(),
            roll.base,
            root
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11108_freeze_flow() {
        let (mut r, built) = build_roll();
        assert!(built);
        assert!(r.closed());
        assert!(r.drift() == 0);
        assert!(r.verify(&artifact_name(3), artifact_digest(3)).is_ok());
        assert!(r.seal() != 0);
        assert!(r.register("追件", 1).err() == Some(FZ_FROZEN));
        assert!(FreezeRoll::new(STAMP_BASE).register("x", 0).err() == Some(FZ_DIGEST));
    }

    #[test]
    fn unx11108_checks_green() {
        let s = run_unx11108_checks();
        assert!(s.all_passed(), "UNX-F11108 判据未全绿");
    }
}
