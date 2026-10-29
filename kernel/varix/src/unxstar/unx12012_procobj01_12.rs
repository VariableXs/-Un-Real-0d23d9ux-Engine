//! UNX-F12012 进程对象安全描述符挂点（与 J1 联签预告）（0x193A0 段）
//!
//! 判据原文：安全描述符（SD）挂到进程对象上——SD 头版本号必须为 1
//! （本档钉值）；控制位声明 owner/group 在位时对应 RID 必须非零；挂接
//! 单次性（重复挂即冲突）；卸挂要求 owner 在位（无主 SD 不可卸）；
//! 挂接后 SD 不可就地改版本。
//!
//! 结构：SD 册（SecDesc：revision/control/owner_rid/group_rid）+ 挂点
//! （ProcSdMount：单槽 + 账）+ 挂/卸双核（五码守卫）+ 折叠指纹 +
//! 印账冻结 + 注入核 + 判据聚合（run_unx12012_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x193A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x193A0;
pub const SD_BAD_REVISION: u32 = SEG_BASE + 0x01; // 版本号非 1
pub const SD_NO_OWNER: u32 = SEG_BASE + 0x02; // owner 位声明却零 RID
pub const SD_ALREADY: u32 = SEG_BASE + 0x03; // 重复挂接
pub const SD_DETACH_OWNERLESS: u32 = SEG_BASE + 0x04; // 无主 SD 卸挂
pub const SD_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 控制位：owner/group 在位。
pub const SE_OWNER_PRESENT: u16 = 0x0001;
pub const SE_GROUP_PRESENT: u16 = 0x0002;
/// 本档钉值 SD 版本号。
pub const SD_REVISION: u8 = 1;

/// 安全描述符。
#[derive(Clone)]
pub struct SecDesc {
    pub revision: u8,
    pub control: u16,
    pub owner_rid: u32,
    pub group_rid: u32,
}

impl SecDesc {
    /// 头自洽：版本为 1；声明在位的 RID 必须非零。
    pub fn ok(&self) -> bool {
        self.revision == SD_REVISION
            && (self.control & SE_OWNER_PRESENT == 0 || self.owner_rid != 0)
            && (self.control & SE_GROUP_PRESENT == 0 || self.group_rid != 0)
    }
}

/// 一笔挂/卸的账目。
#[derive(Clone)]
pub struct SdRec {
    pub seq: u32,
    /// 操作码：1=挂 2=卸。
    pub op: u8,
    pub owner_rid: u32,
    pub note: String,
}

impl SdRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 进程对象 SD 挂点模型。
pub struct ProcSdMount {
    pub mounted: Option<SecDesc>,
    pub ledger: Vec<SdRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ProcSdMount {
    pub fn new() -> ProcSdMount {
        ProcSdMount { mounted: None, ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 挂接核：头自洽 + 单次性。
    pub fn mount(&mut self, sd: SecDesc, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(SD_FROZEN);
        }
        if note.is_empty() {
            return Err(SD_BAD_REVISION);
        }
        if sd.revision != SD_REVISION {
            return Err(SD_BAD_REVISION);
        }
        if sd.control & SE_OWNER_PRESENT != 0 && sd.owner_rid == 0 {
            return Err(SD_NO_OWNER);
        }
        if sd.control & SE_GROUP_PRESENT != 0 && sd.group_rid == 0 {
            return Err(SD_NO_OWNER);
        }
        if self.mounted.is_some() {
            return Err(SD_ALREADY);
        }
        self.mounted = Some(sd.clone());
        self.ledger.push(SdRec {
            seq: self.seq_next,
            op: 1,
            owner_rid: sd.owner_rid,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(())
    }

    /// 卸挂核：无主 SD（owner 位缺）不可卸。
    pub fn unmount(&mut self, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(SD_FROZEN);
        }
        if note.is_empty() {
            return Err(SD_BAD_REVISION);
        }
        let cur = match self.mounted {
            Some(ref sd) => sd.clone(),
            None => return Err(SD_DETACH_OWNERLESS),
        };
        if cur.control & SE_OWNER_PRESENT == 0 {
            return Err(SD_DETACH_OWNERLESS);
        }
        self.mounted = None;
        self.ledger.push(SdRec {
            seq: self.seq_next,
            op: 2,
            owner_rid: cur.owner_rid,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(())
    }

    /// 挂接态查询。
    pub fn is_mounted(&self) -> bool {
        self.mounted.is_some()
    }

    /// owner RID 回读。
    pub fn owner_of(&self) -> Option<u32> {
        self.mounted.as_ref().map(|sd| sd.owner_rid)
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
        if let Some(ref sd) = self.mounted {
            h = h.wrapping_mul(211).wrapping_add(sd.revision as u32);
            h = h.wrapping_mul(211).wrapping_add(sd.control as u32);
            h = h.wrapping_mul(211).wrapping_add(sd.owner_rid);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
            h = h.wrapping_mul(211).wrapping_add(r.op as u32);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut m = ProcSdMount::new();
    // 1) 版本号非 1 → SD_BAD_REVISION。
    got[0] = m
        .mount(
            SecDesc { revision: 2, control: SE_OWNER_PRESENT, owner_rid: 500, group_rid: 513 },
            "rev",
        )
        .err() == Some(SD_BAD_REVISION);
    // 2) owner 位声明却零 RID → SD_NO_OWNER。
    got[1] = m
        .mount(
            SecDesc { revision: 1, control: SE_OWNER_PRESENT, owner_rid: 0, group_rid: 513 },
            "own",
        )
        .err() == Some(SD_NO_OWNER);
    // 3) 合法挂接后重复挂 → SD_ALREADY。
    let _ = m.mount(
        SecDesc { revision: 1, control: SE_OWNER_PRESENT, owner_rid: 500, group_rid: 513 },
        "ok",
    );
    got[2] = m
        .mount(
            SecDesc { revision: 1, control: SE_OWNER_PRESENT, owner_rid: 501, group_rid: 513 },
            "dup",
        )
        .err() == Some(SD_ALREADY);
    // 4) 无主 SD 挂上后不可卸 → 先换无主 SD 卸挂路径（须先卸现有）——
    //    直接对空挂点卸挂亦踩同码；此处以「已挂无主 SD」构造：先卸，
    //    挂无主 SD（owner 位缺），再卸 → SD_DETACH_OWNERLESS。
    let _ = m.unmount("u");
    let _ = m.mount(
        SecDesc { revision: 1, control: SE_GROUP_PRESENT, owner_rid: 0, group_rid: 513 },
        "noown",
    );
    got[3] = m.unmount("d").err() == Some(SD_DETACH_OWNERLESS);
    // 5) 印账后卸挂 → SD_FROZEN。
    m.seal();
    got[4] = m.unmount("post").err() == Some(SD_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12012_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12012");

    // J1 正路径：挂→查 owner→卸→查空。
    let j1 = {
        let mut m = ProcSdMount::new();
        let mo = m.mount(
            SecDesc { revision: 1, control: SE_OWNER_PRESENT, owner_rid: 1001, group_rid: 513 },
            "a",
        );
        let own = m.owner_of() == Some(1001);
        let mu = m.unmount("b");
        mo.is_ok() && own && mu.is_ok() && !m.is_mounted()
    };
    s.add("J1-挂卸正路径", j1, "挂查卸全通");

    // J2 版本号非 1 拒（0 与 2 双面）。
    let j2 = {
        let mut m = ProcSdMount::new();
        let r0 = m
            .mount(SecDesc { revision: 0, control: 0, owner_rid: 1, group_rid: 1 }, "v0")
            .err() == Some(SD_BAD_REVISION);
        let r2 = m
            .mount(SecDesc { revision: 2, control: 0, owner_rid: 1, group_rid: 1 }, "v2")
            .err() == Some(SD_BAD_REVISION);
        r0 && r2
    };
    s.add("J2-版本拒", j2, "版本钉 1");

    // J3 owner 位声明零 RID 拒。
    let j3 = {
        let mut m = ProcSdMount::new();
        m.mount(
            SecDesc { revision: 1, control: SE_OWNER_PRESENT, owner_rid: 0, group_rid: 0 },
            "z",
        )
        .err() == Some(SD_NO_OWNER)
    };
    s.add("J3-零owner拒", j3, "声明在位必非零");

    // J4 group 位声明零 RID 同拒。
    let j4 = {
        let mut m = ProcSdMount::new();
        m.mount(
            SecDesc { revision: 1, control: SE_GROUP_PRESENT, owner_rid: 1, group_rid: 0 },
            "zg",
        )
        .err() == Some(SD_NO_OWNER)
    };
    s.add("J4-零group拒", j4, "group 同纪律");

    // J5 头自洽核：合法 SD 过自洽。
    let j5 = {
        let sd = SecDesc { revision: 1, control: SE_OWNER_PRESENT | SE_GROUP_PRESENT, owner_rid: 9, group_rid: 10 };
        sd.ok()
    };
    s.add("J5-头自洽", j5, "合法 SD 过核");

    // J6 重复挂接拒。
    let j6 = {
        let mut m = ProcSdMount::new();
        let _ = m.mount(SecDesc { revision: 1, control: 0, owner_rid: 0, group_rid: 0 }, "a");
        m.mount(SecDesc { revision: 1, control: 0, owner_rid: 0, group_rid: 0 }, "b")
            .err() == Some(SD_ALREADY)
    };
    s.add("J6-重复挂拒", j6, "单槽单 SD");

    // J7 空挂点卸挂拒。
    let j7 = {
        let mut m = ProcSdMount::new();
        m.unmount("e").err() == Some(SD_DETACH_OWNERLESS)
    };
    s.add("J7-空点卸挂拒", j7, "无 SD 不可卸");

    // J8 无主 SD 挂上后不可卸。
    let j8 = {
        let mut m = ProcSdMount::new();
        let _ = m.mount(
            SecDesc { revision: 1, control: SE_GROUP_PRESENT, owner_rid: 0, group_rid: 7 },
            "g",
        );
        m.unmount("d").err() == Some(SD_DETACH_OWNERLESS)
    };
    s.add("J8-无主不可卸", j8, "无主 SD 冻结在点");

    // J9 账目留痕：挂/卸 op 对账。
    let j9 = {
        let mut m = ProcSdMount::new();
        let _ = m.mount(
            SecDesc { revision: 1, control: SE_OWNER_PRESENT, owner_rid: 77, group_rid: 0 },
            "a",
        );
        let _ = m.unmount("b");
        m.ledger.len() == 2 && m.ledger[0].op == 1 && m.ledger[1].op == 2 && m.ledger_ok()
    };
    s.add("J9-挂卸留痕", j9, "op=1/2 对账");

    // J10 印账冻结。
    let j10 = {
        let mut m = ProcSdMount::new();
        let _ = m.mount(SecDesc { revision: 1, control: SE_OWNER_PRESENT, owner_rid: 5, group_rid: 0 }, "a");
        let f1 = m.fold();
        let f2 = m.fold();
        m.seal();
        f1 == f2 && m.unmount("post").err() == Some(SD_FROZEN)
    };
    s.add("J10-指纹封账", j10, "指纹幂等且印后必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：系统进程 SD。
    let mut m = ProcSdMount::new();
    let _ = m.mount(
        SecDesc { revision: 1, control: SE_OWNER_PRESENT | SE_GROUP_PRESENT, owner_rid: 18, group_rid: 18 },
        "system",
    );
    s.add("样例挂接在点", m.is_mounted(), "SYSTEM SD 在点");
    s.add("样例账面合法", m.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12012_mount_flow() {
        let mut m = ProcSdMount::new();
        let sd = SecDesc { revision: 1, control: SE_OWNER_PRESENT, owner_rid: 500, group_rid: 0 };
        assert!(m.mount(sd.clone(), "a").is_ok());
        assert_eq!(m.mount(sd, "b"), Err(SD_ALREADY));
        assert!(m.unmount("c").is_ok());
        assert!(!m.is_mounted());
    }

    #[test]
    fn unx12012_checks_green() {
        let s = run_unx12012_checks();
        assert!(s.all_passed(), "UNX-F12012 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12012_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
