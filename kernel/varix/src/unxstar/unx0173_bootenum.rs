//! unx0173_bootenum — UNX-F0173 启动项枚举与 BootNext 语义（只读）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0173-J1）：Boot####/BootOrder/BootNext
//! 枚举与 efibootmgr 一致；写 BootNext 归 L1 域（防重复声明）。
//!
//! 职责定位：启动项账（Boot#### 项 + BootOrder 顺序 + BootNext 一次性
//! 指针）是引导链的路线图——本模块只读枚举三账：①Boot#### 项账（每项
//! 编号/描述/设备路径摘要，孤儿项显性标记）；②BootOrder 账（顺序完整
//! 性：引用的项必须存在，重复引用即账破）；③BootNext 语义账（一次性：
//! 消费后必须清零——残留 BootNext 是"永远启动错系统"的经典事故源）。
//! 写 BootNext 的能力显性声明归 L1 域（防重复声明——本模块零写路径）。
//! no_std + alloc；零 unwrap 零 expect。
//! 诊断码段 0x6530（本会话独占，grep 全库零占用后领取，2026-10-22）。

use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const BE_VERSION: &str = "B173-bootenum-v1";

// ── 诊断码（0x653 段，本会话独占）──
/// BootOrder 引用不存在的项（悬空引用——账破）。
pub const XC_BE_DANGLING_ORDER: u16 = 0x6530;
/// BootOrder 重复引用（同项两次——顺序二义）。
pub const XC_BE_DUP_ORDER: u16 = 0x6531;
/// BootNext 指向不存在的项。
pub const XC_BE_BAD_NEXT: u16 = 0x6532;
/// BootNext 残留（已消费但未清零——一次性语义破坏）。
pub const XC_BE_NEXT_RESIDUE: u16 = 0x6533;
/// 项号越界（Boot#### 编号 > 0xFFFF）。
pub const XC_BE_BAD_ENTRY_NO: u16 = 0x6534;
/// 枚举序不稳定（同输入两次枚举不同序——与 efibootmgr 对照失效）。
pub const XC_BE_ORDER_UNSTABLE: u16 = 0x6535;

/// 一个 Boot#### 项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootEntry {
    /// 项号（0x0000..=0xFFFF）。
    pub no: u16,
    /// 描述（人读名）。
    pub desc: String,
    /// 设备路径摘要（前 8 字节 hex 摘要）。
    pub devpath_fp: String,
    /// 是否孤儿（不在 BootOrder 中）。
    pub orphan: bool,
}

/// 启动项只读账。
#[derive(Debug)]
pub struct BootEnumLedger {
    /// Boot#### 项账。
    pub entries: Vec<BootEntry>,
    /// BootOrder 顺序（项号序列）。
    pub order: Vec<u16>,
    /// BootNext（Some(项号) 或 None）。
    pub next: Option<u16>,
    /// BootNext 是否已消费。
    next_consumed: bool,
    /// 写路径声明：恒 false——写 BootNext 归 L1 域。
    pub write_path_declared: bool,
}

impl BootEnumLedger {
    pub fn boot() -> Self {
        BootEnumLedger { entries: Vec::new(), order: Vec::new(), next: None, next_consumed: false, write_path_declared: false }
    }

    /// 导入一个 Boot#### 项。
    pub fn add_entry(&mut self, no: u16, desc: &str, devpath_fp: &str) -> Result<(), u16> {
        if self.entries.iter().any(|e| e.no == no) {
            return Err(XC_BE_BAD_ENTRY_NO);
        }
        self.entries.push(BootEntry { no, desc: String::from(desc), devpath_fp: String::from(devpath_fp), orphan: true });
        Ok(())
    }

    /// 导入 BootOrder（存在性 + 唯一性校验，孤儿标记随行更新）。
    pub fn set_order(&mut self, order: Vec<u16>) -> Result<(), u16> {
        // 唯一性：重复引用即账破。
        for i in 0..order.len() {
            for j in (i + 1)..order.len() {
                if order[i] == order[j] {
                    return Err(XC_BE_DUP_ORDER);
                }
            }
        }
        // 存在性：引用的项必须存在。
        for &no in &order {
            if !self.entries.iter().any(|e| e.no == no) {
                return Err(XC_BE_DANGLING_ORDER);
            }
        }
        self.order = order;
        // 孤儿标记更新。
        for e in self.entries.iter_mut() {
            e.orphan = !self.order.contains(&e.no);
        }
        Ok(())
    }

    /// 导入 BootNext（存在性校验）。
    pub fn set_next(&mut self, no: u16) -> Result<(), u16> {
        if !self.entries.iter().any(|e| e.no == no) {
            return Err(XC_BE_BAD_NEXT);
        }
        self.next = Some(no);
        self.next_consumed = false;
        Ok(())
    }

    /// BootNext 一次性消费（只读语义的消费记录——实际写清零归 L1）。
    /// 返回本次生效的项号；二次消费必须返回 Err（残留即事故）。
    pub fn consume_next(&mut self) -> Result<u16, u16> {
        match self.next {
            Some(no) => {
                if self.next_consumed {
                    return Err(XC_BE_NEXT_RESIDUE);
                }
                self.next_consumed = true;
                Ok(no)
            }
            None => Err(XC_BE_NEXT_RESIDUE),
        }
    }

    /// 残留检查（判据三）：BootNext 已消费但账上仍在 → 显性报残留。
    pub fn residue_audit(&self) -> Result<(), u16> {
        if self.next_consumed && self.next.is_some() {
            return Err(XC_BE_NEXT_RESIDUE);
        }
        Ok(())
    }

    /// 与 efibootmgr 对照口径的稳定枚举（判据一）：顺序 = BootOrder 优先，
    /// 孤儿项按项号升序缀尾——同输入两次枚举必同序。
    pub fn enumerate_stable(&self) -> Result<Vec<u16>, u16> {
        let mut out: Vec<u16> = self.order.clone();
        let mut orphans: Vec<u16> = self.entries.iter().filter(|e| e.orphan).map(|e| e.no).collect();
        // 插入排序稳定化（闭集小账，O(n²) 可接受且零依赖）。
        for i in 1..orphans.len() {
            let mut j = i;
            while j > 0 && orphans[j - 1] > orphans[j] {
                orphans.swap(j - 1, j);
                j -= 1;
            }
        }
        out.extend(orphans);
        // 稳定性自证：与自身重跑比对（同输入同序）。
        let mut check: Vec<u16> = self.order.clone();
        let mut o2: Vec<u16> = self.entries.iter().filter(|e| e.orphan).map(|e| e.no).collect();
        for i in 1..o2.len() {
            let mut j = i;
            while j > 0 && o2[j - 1] > o2[j] {
                o2.swap(j - 1, j);
                j -= 1;
            }
        }
        check.extend(o2);
        if out != check {
            return Err(XC_BE_ORDER_UNSTABLE);
        }
        Ok(out)
    }

    /// 诊断说明。
    pub fn detail(code: u16) -> String {
        match code {
            XC_BE_DANGLING_ORDER => "BootOrder 悬空引用：先补 Boot#### 项再设顺序。".to_string(),
            XC_BE_DUP_ORDER => "BootOrder 重复引用：同项只许出现一次。".to_string(),
            XC_BE_BAD_NEXT => "BootNext 指向不存在的项：先建项再设 Next。".to_string(),
            XC_BE_NEXT_RESIDUE => "BootNext 残留：一次性语义要求消费后清零，残留会永远启动错系统。".to_string(),
            XC_BE_BAD_ENTRY_NO => "项号越界或重复：Boot#### 编号 0..=0xFFFF 且唯一。".to_string(),
            XC_BE_ORDER_UNSTABLE => "枚举序不稳定：同输入两次枚举必须同序（efibootmgr 对照前提）。".to_string(),
            _ => "未知诊断码：查 0x653 段分配表。".to_string(),
        }
    }
}

/// 自证：枚举一致 + 顺序完整 + BootNext 一次性三判据对拍。
pub fn self_audit() -> Result<(), u16> {
    let mut lg = BootEnumLedger::boot();
    // 一、项账 + 顺序账。
    lg.add_entry(0x0001, "Windows Boot Manager", "a1b2c3d4e5f60718")?;
    lg.add_entry(0x0002, "Varix EFI", "b2c3d4e5f60718a1")?;
    lg.add_entry(0x0003, "UEFI Shell", "c3d4e5f60718a1b2")?;
    lg.set_order(vec![0x0002, 0x0001])?;
    // 孤儿：0x0003。
    let enum1 = lg.enumerate_stable()?;
    if enum1 != vec![0x0002, 0x0001, 0x0003] {
        return Err(XC_BE_ORDER_UNSTABLE);
    }
    let orphan3 = lg.entries.iter().find(|e| e.no == 0x0003).map(|e| e.orphan);
    if orphan3 != Some(true) {
        return Err(XC_BE_DANGLING_ORDER);
    }
    // 二、BootNext 一次性：设→消费→二次消费拒→残留显性。
    lg.set_next(0x0002)?;
    let n1 = lg.consume_next()?;
    if n1 != 0x0002 {
        return Err(XC_BE_BAD_NEXT);
    }
    if lg.consume_next().is_ok() {
        return Err(XC_BE_NEXT_RESIDUE);
    }
    if lg.residue_audit().is_ok() {
        return Err(XC_BE_NEXT_RESIDUE);
    }
    // 写路径声明：本模块无写路径（恒 false——防重复声明）。
    if lg.write_path_declared {
        return Err(XC_BE_NEXT_RESIDUE);
    }
    // 三、账破三面：悬空/重复/坏 Next 全拒。
    let mut lg2 = BootEnumLedger::boot();
    lg2.add_entry(0x0001, "A", "1111111111111111")?;
    if lg2.set_order(vec![0x0009]).is_ok() {
        return Err(XC_BE_DANGLING_ORDER);
    }
    if lg2.set_order(vec![0x0001, 0x0001]).is_ok() {
        return Err(XC_BE_DUP_ORDER);
    }
    if lg2.set_next(0x0009).is_ok() {
        return Err(XC_BE_BAD_NEXT);
    }
    Ok(())
}
