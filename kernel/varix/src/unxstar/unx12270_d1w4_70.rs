//! UNX-F12270 IO_STATUS_BLOCK 与异步 IO 预告（0x648c0 段）
//!
//! 卡面：IOSB 两字段（status/information）语义与异步 IO 槽位模型——
//! 槽位 Empty→Pending→Filled 单向转移；Filled 一次性写入（重写必拒）；
//! 未决槽查询返回 PENDING；information 按操作码口径记账（读写字节数/
//! 打开共享位）。
//! 域：CoRun·UNX-D1-B14。
//!
//! 判据原文：
//! - J1 槽位单向转移：Empty→Pending→Filled，无回退。
//! - J2 Filled 一次性：已填 IOSB 再填必拒。
//! - J3 未决查询：Pending 槽取 IOSB 必报 PENDING。
//! - J4 information 口径：读写字节数 ≤ 请求长度；打开返回共享位。
//! - J5 空槽操作：空槽完成/查询必拒。
//! - J6 重号槽必拒。
//! - J7 印账冻结：seal 后提交/填写必拒。
//! - J8 指纹幂等。
//! - J9 注入五况全抓。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x648c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x648c0;
pub const IB_BAD_TAG: u32 = SEG_BASE + 0x01; // 槽号越域
pub const IB_PENDING: u32 = SEG_BASE + 0x02; // 未决槽查询
pub const IB_REWRITE: u32 = SEG_BASE + 0x03; // Filled 重写
pub const IB_DUP: u32 = SEG_BASE + 0x04; // 重号槽
pub const IB_FROZEN: u32 = SEG_BASE + 0x05; // 印账后操作

/// 槽位状态（单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum SlotState {
    Empty = 0,
    Pending = 1,
    Filled = 2,
}

/// IOSB 档。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Iosb {
    /// 模型内 0 = 成功。
    pub status: u32,
    /// 口径：读写字节数 / 打开共享位。
    pub information: u32,
}

/// 异步 IO 槽位。
#[derive(Clone)]
pub struct IoSlot {
    pub tag: u32,
    pub opcode: u8, // 0=read 1=write 2=open
    pub requested: u32,
    pub state: SlotState,
    pub iosb: Option<Iosb>,
}

/// 异步槽位账。
pub struct IoBook {
    pub slots: Vec<IoSlot>,
    pub sealed: bool,
}

impl IoBook {
    pub fn new() -> IoBook {
        IoBook { slots: Vec::new(), sealed: false }
    }

    fn find(&self, tag: u32) -> Option<usize> {
        self.slots.iter().position(|s| s.tag == tag)
    }

    /// 提交：Empty 落槽 → Pending。
    pub fn submit(&mut self, tag: u32, opcode: u8, requested: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IB_FROZEN);
        }
        if tag == 0 || opcode > 2 {
            return Err(IB_BAD_TAG);
        }
        if self.find(tag).is_some() {
            return Err(IB_DUP);
        }
        self.slots.push(IoSlot {
            tag,
            opcode,
            requested,
            state: SlotState::Pending,
            iosb: None,
        });
        Ok(())
    }

    /// 完成：Pending → Filled 一次性写入。
    pub fn complete(&mut self, tag: u32, status: u32, information: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IB_FROZEN);
        }
        match self.find(tag) {
            None => Err(IB_BAD_TAG),
            Some(i) => {
                if self.slots[i].state == SlotState::Filled {
                    return Err(IB_REWRITE);
                }
                // information 口径：读写字节数不得超过请求长度。
                if self.slots[i].opcode <= 1 && information > self.slots[i].requested {
                    return Err(IB_BAD_TAG);
                }
                self.slots[i].state = SlotState::Filled;
                self.slots[i].iosb = Some(Iosb { status, information });
                Ok(())
            }
        }
    }

    /// 查询 IOSB：Pending 槽报 PENDING，空槽报 BAD_TAG。
    pub fn query(&self, tag: u32) -> Result<Iosb, u32> {
        match self.find(tag) {
            None => Err(IB_BAD_TAG),
            Some(i) => match self.slots[i].iosb {
                Some(x) => Ok(x),
                None => Err(IB_PENDING),
            },
        }
    }

    pub fn count_state(&self, st: SlotState) -> usize {
        self.slots.iter().filter(|s| s.state == st).count()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for s in &self.slots {
            h = h.wrapping_mul(211).wrapping_add(s.tag);
            h = h.wrapping_mul(211).wrapping_add(s.opcode as u32);
            h = h.wrapping_mul(211).wrapping_add(s.requested);
            h = h.wrapping_mul(211).wrapping_add(s.state as u32);
            if let Some(x) = s.iosb {
                h = h.wrapping_mul(211).wrapping_add(x.status);
                h = h.wrapping_mul(211).wrapping_add(x.information);
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
        format!(
            "n={} p={} f={}",
            self.slots.len(),
            self.count_state(SlotState::Pending),
            self.count_state(SlotState::Filled)
        )
    }

    pub fn line_of(&self, tag: u32) -> String {
        match self.find(tag) {
            Some(i) => match self.slots[i].iosb {
                Some(x) => format!("t{}|st{}|info{}", tag, x.status, x.information),
                None => format!("t{}|pending", tag),
            },
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = IoBook::new();
    // 1) 零槽号必拒。
    got[0] = b.submit(0, 0, 16).err() == Some(IB_BAD_TAG);
    // 2) 空槽查询必拒。
    got[1] = b.query(7).err() == Some(IB_BAD_TAG);
    // 3) 未决槽查询必报 PENDING。
    let _ = b.submit(7, 0, 16);
    got[2] = b.query(7).err() == Some(IB_PENDING);
    // 4) information 超请求长度必拒（读）。
    got[3] = b.complete(7, 0, 32).err() == Some(IB_BAD_TAG);
    // 5) 印账后提交必抓。
    b.seal();
    got[4] = b.submit(8, 0, 16).err() == Some(IB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12270_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12270");

    // J1 槽位单向转移。
    let j1 = {
        let mut b = IoBook::new();
        let _ = b.submit(1, 0, 100);
        let p = b.query(1).err() == Some(IB_PENDING);
        let _ = b.complete(1, 0, 100);
        p && b.query(1).is_ok() && b.count_state(SlotState::Filled) == 1
    };
    s.add("J1-单向转移", j1, "Empty→Pending→Filled");

    // J2 Filled 一次性。
    let j2 = {
        let mut b = IoBook::new();
        let _ = b.submit(1, 0, 100);
        let _ = b.complete(1, 0, 100);
        b.complete(1, 0, 100).err() == Some(IB_REWRITE)
    };
    s.add("J2-一次性写入", j2, "重填必拒");

    // J3 未决查询。
    let j3 = {
        let mut b = IoBook::new();
        let _ = b.submit(2, 1, 50);
        b.query(2).err() == Some(IB_PENDING)
    };
    s.add("J3-未决查询", j3, "Pending 报 PENDING");

    // J4 information 口径。
    let j4 = {
        let mut b = IoBook::new();
        let _ = b.submit(1, 0, 64);
        let _ = b.complete(1, 0, 40);
        let _ = b.submit(2, 2, 0);
        let _ = b.complete(2, 0, SH_FLAGS);
        b.query(1).map(|x| x.information) == Ok(40)
            && b.query(2).map(|x| x.information) == Ok(SH_FLAGS)
    };
    s.add("J4-information口径", j4, "字节/共享位口径");

    // J5 information 超长拒。
    let j5 = {
        let mut b = IoBook::new();
        let _ = b.submit(1, 1, 16);
        b.complete(1, 0, 17).err() == Some(IB_BAD_TAG)
    };
    s.add("J5-超长拒", j5, "字节超请求必拒");

    // J6 空槽完成拒。
    let j6 = {
        let mut b = IoBook::new();
        b.complete(9, 0, 1).err() == Some(IB_BAD_TAG)
    };
    s.add("J6-空槽完成拒", j6, "不在册必拒");

    // J7 重号槽拒。
    let j7 = {
        let mut b = IoBook::new();
        let _ = b.submit(3, 0, 8);
        b.submit(3, 0, 8).err() == Some(IB_DUP)
    };
    s.add("J7-重号槽拒", j7, "tag 唯一");

    // J8 零槽号/坏操作码拒。
    let j8 = {
        let mut b = IoBook::new();
        b.submit(0, 0, 8).err() == Some(IB_BAD_TAG)
            && b.submit(4, 9, 8).err() == Some(IB_BAD_TAG)
    };
    s.add("J8-越域拒", j8, "tag0/opcode9 拒");

    // J9 印账冻结。
    let j9 = {
        let mut b = IoBook::new();
        b.seal();
        b.submit(1, 0, 8).err() == Some(IB_FROZEN)
            && b.complete(1, 0, 8).err() == Some(IB_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后双操作拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = IoBook::new();
        let _ = b.submit(1, 0, 8);
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 回读渲染。
    let j11 = {
        let mut b = IoBook::new();
        let _ = b.submit(5, 0, 32);
        let _ = b.complete(5, 0, 32);
        b.line_of(5).contains("info32") && b.line_of(6) == "missing"
    };
    s.add("J11-回读渲染", j11, "IOSB 行可渲染");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = IoBook::new();
    let _ = b.submit(1, 0, 64);
    let _ = b.complete(1, 0, 64);
    let _ = b.submit(2, 1, 32);
    s.add("摘要回读", b.summary().contains("n=2"), "双槽在册");
    s.add("未决记账", b.count_state(SlotState::Pending) == 1, "一槽未决");
    s
}

/// 打开操作的 information 口径（共享位样本）。
pub const SH_FLAGS: u32 = 0x3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12270_iosb_flow() {
        let mut b = IoBook::new();
        assert!(b.submit(1, 0, 100).is_ok());
        assert_eq!(b.query(1), Err(IB_PENDING));
        assert!(b.complete(1, 0, 70).is_ok());
        let iosb = b.query(1).map(|x| (x.status, x.information));
        assert_eq!(iosb, Ok((0, 70)));
        assert_eq!(b.complete(1, 0, 70), Err(IB_REWRITE));
    }

    #[test]
    fn unx12270_checks_green() {
        let s = run_unx12270_checks();
        assert!(s.all_passed(), "UNX-F12270 判据未全绿");
    }

    #[test]
    fn unx12270_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
