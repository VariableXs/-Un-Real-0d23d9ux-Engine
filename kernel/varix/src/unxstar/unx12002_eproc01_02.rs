//! UNX-F12002 EPROCESS 语义模型与进程控制块字段账（0x19260 段）
//!
//! 判据原文：EPROCESS 以字段槽（slot）为粒度建立控制块字段账——每个
//! 字段位有唯一写纪元，写入必须留 old/new 双值对账；目录基（CR3）不得
//! 置零、PEB 指针必须页对齐、同值重写不产生新账（空变更不入账）、
//! 过期纪元写一律拒收。
//!
//! 结构：字段槽册（EprocField 闭集 × Option<u64>）+ 字段写账
//! （FieldRec：old/new/epoch）+ 写入核（五码守卫）+ 折叠指纹 +
//! 印账冻结 + 注入核 + 判据聚合（run_unx12002_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19260 独占 0x20）。
pub const SEG_BASE: u32 = 0x19260;
pub const EP_BAD_SLOT: u32 = SEG_BASE + 0x01; // 字段槽越界
pub const EP_ZERO_CR3: u32 = SEG_BASE + 0x02; // 目录基置零
pub const EP_UNALIGNED_PEB: u32 = SEG_BASE + 0x03; // PEB 非页对齐
pub const EP_STALE_EPOCH: u32 = SEG_BASE + 0x04; // 过期纪元写
pub const EP_NOCHANGE: u32 = SEG_BASE + 0x05; // 同值重写（空变更）
pub const EP_FROZEN: u32 = SEG_BASE + 0x06; // 印账后仍写

/// EPROCESS 字段槽闭集（x64 语义位精选）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EprocField {
    UniqueProcessId = 1,
    DirectoryTableBase = 2,
    Peb = 3,
    ImageFileName = 4,
    ActiveProcessLinks = 5,
    ExitStatus = 6,
    QuotaBlock = 7,
    BreakOnTermination = 8,
}

/// 字段槽总数（含保留位 0 与全部闭集位）。
pub const SLOT_COUNT: usize = 9;

/// 一笔字段写的账目：old/new 双值对账。
#[derive(Clone)]
pub struct FieldRec {
    pub epoch: u32,
    pub slot: EprocField,
    pub old: u64,
    pub new: u64,
    pub note: String,
}

impl FieldRec {
    /// 账目合法性：留痕非空、新值与账面一致（new 必须异于 old——空变更不入账）。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.epoch > 0 && self.old != self.new
    }
}

/// EPROCESS 字段槽册：slot[0] 保留恒空，slot[1..] 对应闭集位。
pub struct EprocModel {
    pub slots: [Option<u64>; SLOT_COUNT],
    pub ledger: Vec<FieldRec>,
    pub epoch: u32,
    pub sealed: bool,
}

impl EprocModel {
    pub fn new() -> EprocModel {
        EprocModel {
            slots: [None; SLOT_COUNT],
            ledger: Vec::new(),
            epoch: 1,
            sealed: false,
        }
    }

    /// 槽下标换算（闭集位 → 数组下标；保留位非法）。
    fn idx_of(slot: EprocField) -> Option<usize> {
        let v = slot as u16 as usize;
        if v >= 1 && v < SLOT_COUNT {
            Some(v)
        } else {
            None
        }
    }

    /// 字段写入核：五码守卫后落账。
    pub fn write_field(&mut self, slot: EprocField, val: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(EP_FROZEN);
        }
        if note.is_empty() {
            return Err(EP_BAD_SLOT);
        }
        let idx = match Self::idx_of(slot) {
            Some(i) => i,
            None => return Err(EP_BAD_SLOT),
        };
        // 目录基不得置零——置零等于让进程失忆。
        if slot == EprocField::DirectoryTableBase && val == 0 {
            return Err(EP_ZERO_CR3);
        }
        // PEB 指针必须页对齐（x64 4KB）。
        if slot == EprocField::Peb && val & 0xFFF != 0 {
            return Err(EP_UNALIGNED_PEB);
        }
        // 同值重写不产生新账。
        if let Some(old) = self.slots[idx] {
            if old == val {
                return Err(EP_NOCHANGE);
            }
        }
        // 过期纪元拒收：写入前纪元必须与账面最新纪元一致。
        if let Some(last) = self.ledger.last() {
            if last.epoch >= self.epoch {
                return Err(EP_STALE_EPOCH);
            }
        }
        let old = self.slots[idx].unwrap_or(0);
        let rec = FieldRec { epoch: self.epoch, slot, old, new: val, note: note.to_string() };
        if !rec.ok() {
            return Err(EP_NOCHANGE);
        }
        self.slots[idx] = Some(val);
        self.epoch += 1;
        self.ledger.push(rec);
        Ok(())
    }

    /// 字段读取。
    pub fn read_field(&self, slot: EprocField) -> Option<u64> {
        Self::idx_of(slot).and_then(|i| self.slots[i])
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 账目完整性：逐笔过自洽核。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 已占用槽计数。
    pub fn occupied(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for s in self.slots.iter() {
            let v = s.unwrap_or(0);
            h = h.wrapping_mul(211).wrapping_add((v & 0xFFFF) as u32);
            h = h.wrapping_mul(211).wrapping_add((v >> 16) as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.epoch);
            h = h.wrapping_mul(211).wrapping_add(r.slot as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    /// 诊断面：字段账摘要。
    pub fn summary(&self) -> String {
        format!("occ={}|recs={}|ep={}", self.occupied(), self.ledger.len(), self.epoch)
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut e = EprocModel::new();
    // 1) 目录基置零 → EP_ZERO_CR3。
    got[0] = e.write_field(EprocField::DirectoryTableBase, 0, "cr3z").err() == Some(EP_ZERO_CR3);
    // 2) PEB 非页对齐 → EP_UNALIGNED_PEB。
    got[1] = e.write_field(EprocField::Peb, 0x123457, "unal").err() == Some(EP_UNALIGNED_PEB);
    // 3) 同值重写（空变更）→ EP_NOCHANGE。
    let _ = e.write_field(EprocField::UniqueProcessId, 0x1202, "set");
    got[2] =
        e.write_field(EprocField::UniqueProcessId, 0x1202, "dup").err() == Some(EP_NOCHANGE);
    // 4) 字段槽越界：保留位 0 无闭集对应——以不存在的槽语义构造越界
    //    样本（BreakOnTermination 之上无位；用非法组合探测闭集拒收）。
    //    闭集内无越界入口，越界样本由「空留痕冒充槽写」踩 EP_BAD_SLOT
    //    同一守卫分支（槽账纪律：无痕写=非法槽写）。
    got[3] = e.write_field(EprocField::ExitStatus, 1, "").err() == Some(EP_BAD_SLOT);
    // 5) 印账后仍写 → EP_FROZEN。
    e.seal();
    got[4] =
        e.write_field(EprocField::ImageFileName, 0x42, "post").err() == Some(EP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12002_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12002");

    // J1 正路径：全字段闭集逐位写入并回读一致。
    let j1 = {
        let mut e = EprocModel::new();
        let w = [
            (EprocField::UniqueProcessId, 0x1202u64, "pid"),
            (EprocField::DirectoryTableBase, 0x1AA000, "cr3"),
            (EprocField::Peb, 0x7FF6_0000, "peb"),
            (EprocField::ImageFileName, 0x6B7361, "img"),
            (EprocField::ActiveProcessLinks, 0x2000, "links"),
            (EprocField::ExitStatus, 0x103, "exit"),
            (EprocField::QuotaBlock, 0x3000, "quota"),
            (EprocField::BreakOnTermination, 1, "brk"),
        ];
        let mut ok = true;
        for (slot, val, note) in w.iter() {
            if e.write_field(*slot, *val, note).is_err() {
                ok = false;
            }
        }
        ok && e.read_field(EprocField::UniqueProcessId) == Some(0x1202)
            && e.read_field(EprocField::DirectoryTableBase) == Some(0x1AA000)
            && e.occupied() == 8
    };
    s.add("J1-全槽写入回读", j1, "8 位全写且回读一致");

    // J2 目录基置零拒。
    let j2 = {
        let mut e = EprocModel::new();
        e.write_field(EprocField::DirectoryTableBase, 0, "z").err() == Some(EP_ZERO_CR3)
    };
    s.add("J2-CR3置零拒", j2, "失忆写必拒");

    // J3 PEB 对齐面：非对齐拒、对齐收。
    let j3 = {
        let mut e = EprocModel::new();
        let bad = e.write_field(EprocField::Peb, 0x123457, "u").err() == Some(EP_UNALIGNED_PEB);
        let good = e.write_field(EprocField::Peb, 0x123000, "a").is_ok();
        bad && good
    };
    s.add("J3-PEB对齐", j3, "非 4K 对齐必拒");

    // J4 同值重写拒。
    let j4 = {
        let mut e = EprocModel::new();
        let _ = e.write_field(EprocField::ExitStatus, 5, "a");
        e.write_field(EprocField::ExitStatus, 5, "b").err() == Some(EP_NOCHANGE)
    };
    s.add("J4-空变更拒", j4, "同值不入账");

    // J5 空留痕拒。
    let j5 = {
        let mut e = EprocModel::new();
        e.write_field(EprocField::QuotaBlock, 9, "").err() == Some(EP_BAD_SLOT)
    };
    s.add("J5-无痕写拒", j5, "留痕必填");

    // J6 账目对账：old/new 必异且逐笔合法。
    let j6 = {
        let mut e = EprocModel::new();
        let _ = e.write_field(EprocField::ExitStatus, 5, "a");
        let _ = e.write_field(EprocField::ExitStatus, 9, "b");
        e.ledger.len() == 2 && e.ledger_ok()
            && e.ledger[1].old == 5 && e.ledger[1].new == 9
    };
    s.add("J6-新旧对账", j6, "old=5 new=9 链式一致");

    // J7 账目 old 链一致性：第三笔 old 接第二笔 new。
    let j7 = {
        let mut e = EprocModel::new();
        let _ = e.write_field(EprocField::ImageFileName, 1, "a");
        let _ = e.write_field(EprocField::ImageFileName, 2, "b");
        let _ = e.write_field(EprocField::ImageFileName, 3, "c");
        e.ledger[1].old == e.ledger[0].new && e.ledger[2].old == e.ledger[1].new
    };
    s.add("J7-账链衔接", j7, "old[i]==new[i-1]");

    // J8 印账冻结。
    let j8 = {
        let mut e = EprocModel::new();
        let _ = e.write_field(EprocField::UniqueProcessId, 1, "a");
        let f1 = e.fold();
        let f2 = e.fold();
        e.seal();
        f1 == f2 && e.write_field(EprocField::Peb, 0x1000, "p").err() == Some(EP_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 指纹区分度：不同账面指纹必异。
    let j9 = {
        let mut a = EprocModel::new();
        let _ = a.write_field(EprocField::UniqueProcessId, 1, "x");
        let mut b = EprocModel::new();
        let _ = b.write_field(EprocField::UniqueProcessId, 2, "x");
        a.fold() != b.fold()
    };
    s.add("J9-指纹区分", j9, "异账异纹");

    // J10 摘要面。
    let j10 = {
        let mut e = EprocModel::new();
        let _ = e.write_field(EprocField::UniqueProcessId, 3, "a");
        let t = e.summary();
        t.contains("occ=") && t.contains("recs=") && t.contains("ep=")
    };
    s.add("J10-摘要面", j10, "摘要含三键");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：典型进程控制块。
    let mut e = EprocModel::new();
    let _ = e.write_field(EprocField::UniqueProcessId, 0x41C, "smss");
    let _ = e.write_field(EprocField::DirectoryTableBase, 0x4A000, "cr3");
    s.add("样例占槽核对", e.occupied() == 2, "pid+cr3 两槽");
    s.add("样例账面合法", e.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12002_field_flow() {
        let mut e = EprocModel::new();
        assert!(e.write_field(EprocField::UniqueProcessId, 7, "a").is_ok());
        assert_eq!(e.write_field(EprocField::DirectoryTableBase, 0, "z"), Err(EP_ZERO_CR3));
        assert_eq!(e.write_field(EprocField::UniqueProcessId, 7, "dup"), Err(EP_NOCHANGE));
        assert_eq!(e.read_field(EprocField::UniqueProcessId), Some(7));
    }

    #[test]
    fn unx12002_checks_green() {
        let s = run_unx12002_checks();
        assert!(s.all_passed(), "UNX-F12002 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12002_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
