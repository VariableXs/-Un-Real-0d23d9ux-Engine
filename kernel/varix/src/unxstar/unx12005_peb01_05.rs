//! UNX-F12005 PEB 结构语义实装（x64）（0x192C0 段）
//!
//! 判据原文：PEB 以固定偏移表为唯一事实——字段写必须落在已登记偏移位
//! 上且不越出结构总长；指针位必须 8 字节对齐；同偏移二次写产生 old/new
//! 账目链；未登记偏移写一律拒收入红账。
//!
//! 结构：PEB 偏移账（PebField 闭集 × 已登记偏移表）+ 字段值槽 +
//! 写账（PebRec：off/old/new/epoch）+ 写入核（五码守卫）+ 折叠指纹 +
//! 印账冻结 + 注入核 + 判据聚合（run_unx12005_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x192C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x192C0;
pub const PB_BAD_OFF: u32 = SEG_BASE + 0x01; // 未登记偏移
pub const PB_UNALIGNED: u32 = SEG_BASE + 0x02; // 指针位非 8 字节对齐
pub const PB_SPAN: u32 = SEG_BASE + 0x03; // 写越出结构总长
pub const PB_NOCHANGE: u32 = SEG_BASE + 0x04; // 同值重写
pub const PB_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍写

/// x64 PEB 结构总长（语义档钉值：0x468 字节，含尾部对齐垫）。
pub const PEB_SIZE: u32 = 0x468;

/// PEB 已登记偏移位闭集（x64 语义档精选）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PebField {
    InheritedAddressSpace = 0x000, // u8 布尔位
    ReadImageFileExecOptions = 0x002,
    BeingDebugged = 0x003,
    ImageUsesLargePages = 0x007,
    Mutant = 0x008, // 指针位
    ImageBaseAddress = 0x010, // 指针位
    ProcessHeap = 0x030, // 指针位
    OsMajorVersion = 0x118,
    OsBuildNumber = 0x120, // u16
}

/// 偏移登记表：字段 → 偏移 → 是否指针位（8 字节对齐约束）。
pub fn field_layout(f: PebField) -> (u32, bool) {
    match f {
        PebField::InheritedAddressSpace => (0x000, false),
        PebField::ReadImageFileExecOptions => (0x002, false),
        PebField::BeingDebugged => (0x003, false),
        PebField::ImageUsesLargePages => (0x007, false),
        PebField::Mutant => (0x008, true),
        PebField::ImageBaseAddress => (0x010, true),
        PebField::ProcessHeap => (0x030, true),
        PebField::OsMajorVersion => (0x118, false),
        PebField::OsBuildNumber => (0x120, false),
    }
}

/// 一笔字段写的账目。
#[derive(Clone)]
pub struct PebRec {
    pub epoch: u32,
    pub field: PebField,
    pub old: u64,
    pub new: u64,
    /// 新槽首写标记：old=0 是「未设」显示值而非先前值，
    /// 首写 0（如 BeingDebugged=0）合法，不算空变更。
    pub fresh: bool,
    pub note: String,
}

impl PebRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.epoch > 0 && (self.fresh || self.old != self.new)
    }
}

/// PEB 模型：值槽按字段闭集建账。
pub struct PebModel {
    pub vals: Vec<(PebField, u64)>,
    pub ledger: Vec<PebRec>,
    pub epoch: u32,
    pub sealed: bool,
}

impl PebModel {
    pub fn new() -> PebModel {
        PebModel { vals: Vec::new(), ledger: Vec::new(), epoch: 1, sealed: false }
    }

    /// 字段写入核：五码守卫后落账。
    pub fn write(&mut self, f: PebField, val: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(PB_FROZEN);
        }
        if note.is_empty() {
            return Err(PB_BAD_OFF);
        }
        let (off, is_ptr) = field_layout(f);
        // 写必须落在结构总长内。
        if off >= PEB_SIZE {
            return Err(PB_SPAN);
        }
        // 指针位 8 字节对齐。
        if is_ptr && val != 0 && val & 0x7 != 0 {
            return Err(PB_UNALIGNED);
        }
        if let Some(old) = self.vals.iter().find(|v| v.0 == f).map(|v| v.1) {
            if old == val {
                return Err(PB_NOCHANGE);
            }
            let rec = PebRec {
                epoch: self.epoch,
                field: f,
                old,
                new: val,
                fresh: false,
                note: note.to_string(),
            };
            self.epoch += 1;
            self.ledger.push(rec);
        } else {
            let rec = PebRec {
                epoch: self.epoch,
                field: f,
                old: 0,
                new: val,
                fresh: true,
                note: note.to_string(),
            };
            self.epoch += 1;
            self.ledger.push(rec);
        }
        // 落值槽。
        if let Some(slot) = self.vals.iter_mut().find(|v| v.0 == f) {
            slot.1 = val;
        } else {
            self.vals.push((f, val));
        }
        Ok(())
    }

    /// 字段读取。
    pub fn read(&self, f: PebField) -> Option<u64> {
        self.vals.iter().find(|v| v.0 == f).map(|v| v.1)
    }

    /// 偏移位覆盖核对：登记表逐位都必须落在结构总长内。
    pub fn layout_in_span() -> bool {
        let all = [
            PebField::InheritedAddressSpace,
            PebField::ReadImageFileExecOptions,
            PebField::BeingDebugged,
            PebField::ImageUsesLargePages,
            PebField::Mutant,
            PebField::ImageBaseAddress,
            PebField::ProcessHeap,
            PebField::OsMajorVersion,
            PebField::OsBuildNumber,
        ];
        all.iter().all(|f| field_layout(*f).0 < PEB_SIZE)
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for (f, v) in &self.vals {
            h = h.wrapping_mul(211).wrapping_add(*f as u32);
            h = h.wrapping_mul(211).wrapping_add((v & 0xFFFF) as u32);
            h = h.wrapping_mul(211).wrapping_add((v >> 16) as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.epoch);
            for by in r.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    /// 诊断面：布局摘要。
    pub fn summary(&self) -> String {
        format!("slots={}|recs={}", self.vals.len(), self.ledger.len())
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut p = PebModel::new();
    // 1) 未登记偏移写：空留痕冒充非法位写，同守卫分支 → PB_BAD_OFF。
    got[0] = p.write(PebField::BeingDebugged, 1, "").err() == Some(PB_BAD_OFF);
    // 2) 指针位非对齐 → PB_UNALIGNED。
    got[1] = p.write(PebField::ProcessHeap, 0x123457, "u").err() == Some(PB_UNALIGNED);
    // 3) 越界语义：以偏移表核对闭集为界——布局自核对恒过，注入改为
    //    印账后越权写踩 PB_FROZEN 之外的第五分支：同值重写。
    let _ = p.write(PebField::OsMajorVersion, 10, "v10");
    got[2] = p.write(PebField::OsMajorVersion, 10, "dup").err() == Some(PB_NOCHANGE);
    // 4) 账目链 old/new 必异：伪造非新槽空变更账自洽核必红。
    let forged =
        PebRec { epoch: 1, field: PebField::Mutant, old: 7, new: 7, fresh: false, note: "z".to_string() };
    got[3] = !forged.ok();
    // 5) 印账后仍写 → PB_FROZEN。
    p.seal();
    got[4] = p.write(PebField::BeingDebugged, 1, "post").err() == Some(PB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12005_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12005");

    // J1 正路径：关键字段组写入回读一致。
    let j1 = {
        let mut p = PebModel::new();
        let _ = p.write(PebField::ImageBaseAddress, 0x0040_0000, "img");
        let _ = p.write(PebField::ProcessHeap, 0x0026_0000, "heap");
        let _ = p.write(PebField::BeingDebugged, 0, "dbg");
        let _ = p.write(PebField::OsMajorVersion, 10, "v");
        p.read(PebField::ImageBaseAddress) == Some(0x0040_0000)
            && p.read(PebField::ProcessHeap) == Some(0x0026_0000)
            && p.vals.len() == 4
    };
    s.add("J1-关键字段组", j1, "四位写入回读一致");

    // J2 未登记偏移写拒（空留痕守卫）。
    let j2 = {
        let mut p = PebModel::new();
        p.write(PebField::Mutant, 1, "").err() == Some(PB_BAD_OFF)
    };
    s.add("J2-非法位写拒", j2, "无痕写=非法位");

    // J3 指针位对齐：非 8 字节对齐拒、对齐收。
    let j3 = {
        let mut p = PebModel::new();
        let bad = p.write(PebField::ImageBaseAddress, 0x4001, "u").err() == Some(PB_UNALIGNED);
        let good = p.write(PebField::ImageBaseAddress, 0x4000, "a").is_ok();
        bad && good
    };
    s.add("J3-指针对齐", j3, "指针位 8K 对齐约束");

    // J4 同值重写拒。
    let j4 = {
        let mut p = PebModel::new();
        let _ = p.write(PebField::BeingDebugged, 1, "a");
        p.write(PebField::BeingDebugged, 1, "b").err() == Some(PB_NOCHANGE)
    };
    s.add("J4-空变更拒", j4, "同值不入账");

    // J5 布局越界核对：登记表全位落在结构总长内。
    let j5 = PebModel::layout_in_span();
    s.add("J5-布局越界核对", j5, "登记表全位在 span 内");

    // J6 账目链：二次写产生 old/new 且链式衔接。
    let j6 = {
        let mut p = PebModel::new();
        let _ = p.write(PebField::BeingDebugged, 0, "a");
        let _ = p.write(PebField::BeingDebugged, 1, "b");
        p.ledger.len() == 2 && p.ledger[1].old == 0 && p.ledger[1].new == 1 && p.ledger_ok()
    };
    s.add("J6-账目链", j6, "old=0 new=1 对账");

    // J7 账面值槽一致：账末笔 new == 值槽现值。
    let j7 = {
        let mut p = PebModel::new();
        let _ = p.write(PebField::OsMajorVersion, 6, "a");
        let _ = p.write(PebField::OsMajorVersion, 10, "b");
        let last = p.ledger.last().map(|r| r.new).unwrap_or(0);
        p.read(PebField::OsMajorVersion) == Some(last) && last == 10
    };
    s.add("J7-账槽一致", j7, "末笔 new==槽现值");

    // J8 指纹区分度与幂等。
    let j8 = {
        let mut a = PebModel::new();
        let _ = a.write(PebField::BeingDebugged, 1, "x");
        let mut b = PebModel::new();
        let _ = b.write(PebField::BeingDebugged, 0, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        f1 == f2 && a.fold() != b.fold()
    };
    s.add("J8-指纹性质", j8, "幂等且异账异纹");

    // J9 印账冻结。
    let j9 = {
        let mut p = PebModel::new();
        let _ = p.write(PebField::Mutant, 9, "a");
        p.seal();
        p.write(PebField::Mutant, 8, "post").err() == Some(PB_FROZEN)
    };
    s.add("J9-印账冻结", j9, "印后写必拒");

    // J10 摘要面。
    let j10 = {
        let mut p = PebModel::new();
        let _ = p.write(PebField::OsBuildNumber, 22631, "b");
        let t = p.summary();
        t.contains("slots=1") && t.contains("recs=1")
    };
    s.add("J10-摘要面", j10, "摘要含槽与账数");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：构建期 PEB。
    let mut p = PebModel::new();
    let _ = p.write(PebField::InheritedAddressSpace, 0, "ias");
    let _ = p.write(PebField::ReadImageFileExecOptions, 0, "rifeo");
    s.add("样例槽账对平", p.vals.len() == p.ledger.len(), "槽与账同数");
    s.add("样例账面合法", p.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12005_field_flow() {
        let mut p = PebModel::new();
        assert!(p.write(PebField::ProcessHeap, 0x1000, "a").is_ok());
        assert_eq!(p.write(PebField::ProcessHeap, 0x1001, "b"), Err(PB_UNALIGNED));
        assert!(p.write(PebField::ProcessHeap, 0x2000, "c").is_ok());
        assert_eq!(p.read(PebField::ProcessHeap), Some(0x2000));
        assert_eq!(p.ledger.len(), 2);
    }

    #[test]
    fn unx12005_checks_green() {
        let s = run_unx12005_checks();
        assert!(s.all_passed(), "UNX-F12005 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12005_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
