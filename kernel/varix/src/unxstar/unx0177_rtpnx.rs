//! unx0177_rtpnx — UNX-F0177 Runtime 区属性保护（NX/RO）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0177-J1）：运行时代码区 RO+NX 落位
//! 后写执行注入触发 #PF，属性断言过。
//!
//! 职责定位：EFI 运行时代码区落位后必须 RO+NX（对数据页）——不设保护
//! 就是把内核最信任的代码区裸奔给攻击面。本模块是属性账：①区间属性
//! 账（每段落的属性位：RO/W/RX/NX——与页表属性位同语义）；②落位断言
//! （运行时代码段必须 RO+RX、运行时数据段必须 RW+NX——错配即断言红）；
//! ③注入验证账（写执行注入的预期 #PF 路径显性入账——属性不是"设了就
//! 信"而是"注入验证过"）。no_std + alloc；零 unwrap 零 expect。
//! 诊断码段 0x6A10（本会话独占，grep 全库零占用后领取，2026-10-22）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const PX_VERSION: &str = "B177-rtpnx-v1";

// ── 诊断码（0x6A1 段，本会话独占）──
/// 属性错配（代码区可写/数据区可执行——断言红）。
pub const XC_PX_MISMATCH: u16 = 0x6A10;
/// 区间重叠（两段声明交叠——页属性二义）。
pub const XC_PX_OVERLAP: u16 = 0x6A11;
/// 注入验证未做（属性声明了但没验证——不许放行）。
pub const XC_PX_NOT_VERIFIED: u16 = 0x6A12;
/// 注入结果与预期不符（写入没触发 #PF / 执行没触发 #PF——保护失效）。
pub const XC_PX_INJECT_UNEXPECTED: u16 = 0x6A13;
/// 属性账不平（段数 ≡ 验证数）。
pub const XC_PX_LEDGER_UNBALANCED: u16 = 0x6A14;

/// 页属性位（与页表位语义同构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageAttrs {
    /// 可读。
    pub read: bool,
    /// 可写。
    pub write: bool,
    /// 可执行（NX 位的反面）。
    pub exec: bool,
}

/// 一段运行时区声明。
#[derive(Clone, Debug)]
pub struct RtRegion {
    /// 段名。
    pub name: &'static str,
    /// 起始页帧号。
    pub base_pfn: u64,
    /// 页数。
    pub pages: u64,
    /// 声明属性。
    pub attrs: PageAttrs,
    /// 段性质：代码 / 数据。
    pub is_code: bool,
    /// 注入验证已完成。
    pub verified: bool,
}

/// 运行时区属性账。
#[derive(Debug)]
pub struct RtPxLedger {
    pub regions: Vec<RtRegion>,
    /// 注入验证账（段名 → 预期与实际一致）。
    pub inject_log: Vec<(String, bool)>,
}

impl RtPxLedger {
    pub fn boot() -> Self {
        RtPxLedger { regions: Vec::new(), inject_log: Vec::new() }
    }

    /// 声明一段（区间重叠即拒——页属性不许二义）。
    pub fn declare(&mut self, name: &'static str, base_pfn: u64, pages: u64, attrs: PageAttrs, is_code: bool) -> Result<(), u16> {
        if pages == 0 {
            return Err(XC_PX_MISMATCH);
        }
        let lo = base_pfn;
        let hi = base_pfn.saturating_add(pages - 1);
        for r in &self.regions {
            let r_hi = r.base_pfn.saturating_add(r.pages - 1);
            if lo <= r_hi && r.base_pfn <= hi {
                return Err(XC_PX_OVERLAP);
            }
        }
        self.regions.push(RtRegion { name, base_pfn, pages, attrs, is_code, verified: false });
        Ok(())
    }

    /// 属性断言（判据核心）：代码区必须 RO+RX（read=T write=F exec=T），
    /// 数据区必须 RW+NX（read=T write=T exec=F）。
    pub fn attrs_audit(&self) -> Result<(), u16> {
        for r in &self.regions {
            let ok = if r.is_code {
                r.attrs.read && !r.attrs.write && r.attrs.exec
            } else {
                r.attrs.read && r.attrs.write && !r.attrs.exec
            };
            if !ok {
                return Err(XC_PX_MISMATCH);
            }
        }
        Ok(())
    }

    /// 注入验证（判据"注入触发 #PF"）：模型化注入——给定期望结果
    /// （写入/执行是否触发 #PF），与属性推导的预期比对。
    /// 代码区：写注入应触发 #PF（RO），执行注入不触发（RX）。
    /// 数据区：写注入不触发（RW），执行注入应触发 #PF（NX）。
    pub fn inject_verify(&mut self, name: &'static str, write_faults: bool, exec_faults: bool) -> Result<(), u16> {
        let r = match self.regions.iter().find(|r| r.name == name) {
            Some(r) => r,
            None => return Err(XC_PX_NOT_VERIFIED),
        };
        let exp_write_fault = !r.attrs.write;
        let exp_exec_fault = !r.attrs.exec;
        if write_faults != exp_write_fault || exec_faults != exp_exec_fault {
            return Err(XC_PX_INJECT_UNEXPECTED);
        }
        if let Some(slot) = self.regions.iter_mut().find(|r| r.name == name) {
            slot.verified = true;
        }
        self.inject_log.push((String::from(name), true));
        Ok(())
    }

    /// 全量放行审计：属性断言过 ∧ 每段注入验证过。
    pub fn release_audit(&self) -> Result<(), u16> {
        self.attrs_audit()?;
        for r in &self.regions {
            if !r.verified {
                return Err(XC_PX_NOT_VERIFIED);
            }
        }
        if self.inject_log.len() != self.regions.len() {
            return Err(XC_PX_LEDGER_UNBALANCED);
        }
        Ok(())
    }

    /// 诊断说明。
    pub fn detail(code: u16) -> String {
        match code {
            XC_PX_MISMATCH => "属性错配：代码区须 RO+RX、数据区须 RW+NX——错配即攻击面裸奔。".to_string(),
            XC_PX_OVERLAP => "区间重叠：页属性不许二义，先重划区间。".to_string(),
            XC_PX_NOT_VERIFIED => "注入验证未做：属性设了不算数，注入验证过才放行。".to_string(),
            XC_PX_INJECT_UNEXPECTED => "注入结果与预期不符：保护失效（写入没拦/执行没拦），先查页表实态。".to_string(),
            XC_PX_LEDGER_UNBALANCED => "属性账不平：段数 ≡ 验证数。".to_string(),
            _ => "未知诊断码：查 0x6A1 段分配表。".to_string(),
        }
    }
}

/// 自证：断言 + 注入验证正反面对拍。
pub fn self_audit() -> Result<(), u16> {
    let mut lg = RtPxLedger::boot();
    // 一、正确布局：代码区 RO+RX，数据区 RW+NX。
    lg.declare("rt_code", 0x1000, 16, PageAttrs { read: true, write: false, exec: true }, true)?;
    lg.declare("rt_data", 0x2000, 8, PageAttrs { read: true, write: true, exec: false }, false)?;
    lg.attrs_audit()?;
    // 二、注入验证：写代码页 #PF、执行代码页不 #PF；写数据页不 #PF、执行数据页 #PF。
    lg.inject_verify("rt_code", true, false)?;
    lg.inject_verify("rt_data", false, true)?;
    lg.release_audit()?;
    // 三、错配三面：代码可写、数据可执行、代码不可执行。
    let mut lg2 = RtPxLedger::boot();
    lg2.declare("bad_code", 0x1000, 4, PageAttrs { read: true, write: true, exec: true }, true)?;
    if lg2.attrs_audit().is_ok() {
        return Err(XC_PX_MISMATCH);
    }
    let mut lg3 = RtPxLedger::boot();
    lg3.declare("bad_data", 0x1000, 4, PageAttrs { read: true, write: true, exec: true }, false)?;
    if lg3.attrs_audit().is_ok() {
        return Err(XC_PX_MISMATCH);
    }
    // 四、注入与预期不符 → 拒（保护失效显性）。
    let mut lg4 = RtPxLedger::boot();
    lg4.declare("rt_code", 0x1000, 4, PageAttrs { read: true, write: false, exec: true }, true)?;
    if lg4.inject_verify("rt_code", false, false).is_ok() {
        return Err(XC_PX_INJECT_UNEXPECTED);
    }
    // 未验证不放行。
    let mut lg5 = RtPxLedger::boot();
    lg5.declare("rt_code", 0x1000, 4, PageAttrs { read: true, write: false, exec: true }, true)?;
    if lg5.release_audit().is_ok() {
        return Err(XC_PX_NOT_VERIFIED);
    }
    // 五、区间重叠拒。
    let mut lg6 = RtPxLedger::boot();
    lg6.declare("a", 0x1000, 8, PageAttrs { read: true, write: false, exec: true }, true)?;
    if lg6.declare("b", 0x1004, 4, PageAttrs { read: true, write: true, exec: false }, false).is_ok() {
        return Err(XC_PX_OVERLAP);
    }
    Ok(())
}
