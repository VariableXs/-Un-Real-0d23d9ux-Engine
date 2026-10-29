//! UNX-F12049 PEB CSDVersion 与 NTDDI 版本对照
//! 分段: 0x19840 批次: B03 领域: PEB
//! 验收: 折叠指纹 + 封存 + 注入五样 + 12~20 条判据全绿
//!
//! 语义模型：CSDVersion（Service Pack 版本串）在 Vista 之后已从
//! 环境块中的 UNICODE_STRING 退化为仅由注册表承载的历史字段。
//! 本卡维护一张 "NTDDI 平台 ↔ 允许的 CSD 后缀" 对照表：
//!   NTDDI_WIN7   → 允许 SP0/SP1
//!   NTDDI_WIN8   → 仅 SP0
//!   NTDDI_WINBLUE→ 仅 SP0
//!   NTDDI_WIN10  → 仅 SP0（无 SP 体系）
//! CSD 版本串形式为 "Service Pack N"（N ≤ 3），SP0 允许空串。
//! 任何在 Win10 平台写入 SP>0 的操作都将被拒绝（W49_CSD_FORBID）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x19840;
pub const SEG_CSD_FORMAT: u32 = SEG_BASE + 0x01;
pub const SEG_CSD_LEVEL: u32 = SEG_BASE + 0x02;
pub const SEG_CSD_PLATFORM: u32 = SEG_BASE + 0x03;
pub const SEG_CSD_FORBID: u32 = SEG_BASE + 0x04;
pub const SEG_CSD_SEALED: u32 = SEG_BASE + 0x05;

/// 支持的 NTDDI 平台层级
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NtdPlatform {
    Win7 = 1,
    Win8 = 2,
    WinBlue = 3,
    Win10 = 4,
}

impl NtdPlatform {
    /// 该平台允许的最高 SP 号
    pub fn max_sp(self) -> u32 {
        match self {
            NtdPlatform::Win7 => 1,
            NtdPlatform::Win8 => 0,
            NtdPlatform::WinBlue => 0,
            NtdPlatform::Win10 => 0,
        }
    }

    /// 平台显示名（用于摘要）
    pub fn label(self) -> &'static str {
        match self {
            NtdPlatform::Win7 => "WIN7",
            NtdPlatform::Win8 => "WIN8",
            NtdPlatform::WinBlue => "WINBLUE",
            NtdPlatform::Win10 => "WIN10",
        }
    }
}

/// CSD 版本记录：平台 + SP 级别 + 原始串
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsdRec {
    pub platform: NtdPlatform,
    pub sp_level: u32,
    pub raw: String,
    pub sealed: bool,
}

impl CsdRec {
    /// 新建记录；校验 SP 级别与平台约束
    /// 错误：W49_LEVEL（SP 超上限）/ W49_FORMAT（原始串不匹配级别）
    pub fn new(platform: NtdPlatform, sp_level: u32) -> Result<CsdRec, u32> {
        if sp_level > 3 {
            return Err(SEG_CSD_LEVEL);
        }
        if sp_level > platform.max_sp() {
            return Err(SEG_CSD_FORBID);
        }
        let raw = if sp_level == 0 {
            String::new()
        } else {
            format!("Service Pack {}", sp_level)
        };
        Ok(CsdRec { platform, sp_level, raw, sealed: false })
    }

    /// 用已有原始串构造并校验串与级别一致
    pub fn from_raw(platform: NtdPlatform, raw: &str) -> Result<CsdRec, u32> {
        let sp_level = if raw.is_empty() {
            0
        } else if raw == "Service Pack 1" {
            1
        } else if raw == "Service Pack 2" {
            2
        } else if raw == "Service Pack 3" {
            3
        } else {
            return Err(SEG_CSD_FORMAT);
        };
        if sp_level > platform.max_sp() {
            return Err(SEG_CSD_FORBID);
        }
        Ok(CsdRec { platform, sp_level, raw: raw.to_string(), sealed: false })
    }

    /// 升级 SP（单步）；封存后不可改
    pub fn bump(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(SEG_CSD_SEALED);
        }
        if self.sp_level + 1 > 3 {
            return Err(SEG_CSD_LEVEL);
        }
        if self.sp_level + 1 > self.platform.max_sp() {
            return Err(SEG_CSD_FORBID);
        }
        self.sp_level += 1;
        self.raw = if self.sp_level == 0 {
            String::new()
        } else {
            format!("Service Pack {}", self.sp_level)
        };
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 语义一致性：原始串与级别互相印证
    pub fn consistent(&self) -> bool {
        let expect = if self.sp_level == 0 {
            String::new()
        } else {
            format!("Service Pack {}", self.sp_level)
        };
        self.raw == expect
    }

    /// 摘要行
    pub fn summary(&self) -> String {
        format!("[{}|SP{}|{}]", self.platform.label(), self.sp_level,
            if self.raw.is_empty() { "-" } else { &self.raw })
    }
}

/// 对照台账：同一平台只允许一条记录
#[derive(Debug, Clone)]
pub struct CsdBook {
    pub ledger: Vec<CsdRec>,
    pub sealed: bool,
}

impl CsdBook {
    pub fn new() -> CsdBook {
        CsdBook { ledger: Vec::new(), sealed: false }
    }

    /// 登记一条；平台重复 → W49_PLATFORM；封存 → W49_SEALED
    pub fn enroll(&mut self, rec: CsdRec) -> Result<(), u32> {
        if self.sealed {
            return Err(SEG_CSD_SEALED);
        }
        if self.ledger.iter().any(|r| r.platform == rec.platform) {
            return Err(SEG_CSD_PLATFORM);
        }
        self.ledger.push(rec);
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 全台账一致性（串-级互证 + 未封存记录可检查）
    pub fn audit(&self) -> usize {
        self.ledger.iter().filter(|r| r.consistent()).count()
    }
}

/// 折叠指纹：211 进制卷积，种子 SEG_BASE
pub fn fold(recs: &[CsdRec]) -> u32 {
    let mut h: u64 = SEG_BASE as u64;
    for r in recs {
        h = (h.wrapping_mul(211).wrapping_add(r.platform as u64)) & 0xFFFF_FFFF;
        h = (h.wrapping_mul(211).wrapping_add(r.sp_level as u64)) & 0xFFFF_FFFF;
        for b in r.raw.bytes() {
            h = (h.wrapping_mul(211).wrapping_add(b as u64)) & 0xFFFF_FFFF;
        }
        h = (h.wrapping_mul(211).wrapping_add(r.sealed as u64)) & 0xFFFF_FFFF;
    }
    h as u32
}

/// 注入五样：每个坏样命中一个独立错误分支
pub fn inject_five() -> [bool; 5] {
    let mut ok = [false; 5];
    // 样1: SP 号 4 超绝对上限 → W49_LEVEL
    if CsdRec::new(NtdPlatform::Win10, 4) == Err(SEG_CSD_LEVEL) {
        ok[0] = true;
    }
    // 样2: Win10 平台写 SP1 → W49_FORBID（平台禁 SP）
    if CsdRec::new(NtdPlatform::Win10, 1) == Err(SEG_CSD_FORBID) {
        ok[1] = true;
    }
    // 样3: 原始串伪造 → W49_FORMAT
    if CsdRec::from_raw(NtdPlatform::Win7, "Service Pack 9") == Err(SEG_CSD_FORMAT) {
        ok[2] = true;
    }
    // 样4: 同平台重复登记 → W49_PLATFORM
    let mut book = CsdBook::new();
    let r1 = CsdRec::new(NtdPlatform::Win7, 1).unwrap_or_else(|_| CsdRec {
        platform: NtdPlatform::Win7, sp_level: 1, raw: "Service Pack 1".to_string(), sealed: false,
    });
    let r2 = CsdRec::new(NtdPlatform::Win7, 0).unwrap_or_else(|_| CsdRec {
        platform: NtdPlatform::Win7, sp_level: 0, raw: String::new(), sealed: false,
    });
    let _ = book.enroll(r1);
    if book.enroll(r2) == Err(SEG_CSD_PLATFORM) {
        ok[3] = true;
    }
    // 样5: 封存后再 bump → W49_SEALED
    let mut rec = CsdRec::new(NtdPlatform::Win7, 0).unwrap_or_else(|_| CsdRec {
        platform: NtdPlatform::Win7, sp_level: 0, raw: String::new(), sealed: false,
    });
    rec.seal();
    if rec.bump() == Err(SEG_CSD_SEALED) {
        ok[4] = true;
    }
    ok
}

/// 自检：12 条以上判据
pub fn run_unx12049_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F12049");
    // J1 Win7 SP0
    cs.add("J1", CsdRec::new(NtdPlatform::Win7, 0).is_ok(), "win7 sp0 ok");
    // J2 Win7 SP1
    cs.add("J2", CsdRec::new(NtdPlatform::Win7, 1).is_ok(), "win7 sp1 ok");
    // J3 Win7 SP2 → FORBID
    cs.add("J3", CsdRec::new(NtdPlatform::Win7, 2).err() == Some(SEG_CSD_FORBID), "win7 sp2 forbid");
    // J4 Win8 SP1 → FORBID
    cs.add("J4", CsdRec::new(NtdPlatform::Win8, 1).err() == Some(SEG_CSD_FORBID), "win8 sp1 forbid");
    // J5 SP4 → LEVEL
    cs.add("J5", CsdRec::new(NtdPlatform::Win7, 4).err() == Some(SEG_CSD_LEVEL), "sp4 level");
    // J6 bump Win7 SP0→SP1
    let mut r = CsdRec::new(NtdPlatform::Win7, 0).unwrap_or_else(|_| CsdRec {
        platform: NtdPlatform::Win7, sp_level: 0, raw: String::new(), sealed: false,
    });
    let b_ok = r.bump().is_ok() && r.sp_level == 1 && r.raw == "Service Pack 1";
    cs.add("J6", b_ok, "bump to sp1");
    // J7 Win10 bump 到 SP1 → FORBID
    let mut r2 = CsdRec::new(NtdPlatform::Win10, 0).unwrap_or_else(|_| CsdRec {
        platform: NtdPlatform::Win10, sp_level: 0, raw: String::new(), sealed: false,
    });
    cs.add("J7", r2.bump() == Err(SEG_CSD_FORBID), "win10 bump forbid");
    // J8 from_raw 合法串
    cs.add("J8", CsdRec::from_raw(NtdPlatform::Win7, "Service Pack 1").is_ok(), "from raw sp1");
    // J9 from_raw 空串 = SP0
    let e = CsdRec::from_raw(NtdPlatform::Win10, "");
    cs.add("J9", e.is_ok() && e.unwrap_or_else(|_| CsdRec {
        platform: NtdPlatform::Win10, sp_level: 9, raw: String::new(), sealed: false,
    }).sp_level == 0, "empty sp0");
    // J10 consistent 语义互证
    let r3 = CsdRec::from_raw(NtdPlatform::Win7, "Service Pack 1").unwrap_or_else(|_| CsdRec {
        platform: NtdPlatform::Win7, sp_level: 1, raw: "Service Pack 1".to_string(), sealed: false,
    });
    cs.add("J10", r3.consistent(), "consistent");
    // J11 台账四平台可全录
    let mut book = CsdBook::new();
    let _ = book.enroll(CsdRec::new(NtdPlatform::Win7, 1).unwrap_or_else(|_| CsdRec {
        platform: NtdPlatform::Win7, sp_level: 1, raw: "Service Pack 1".to_string(), sealed: false,
    }));
    let _ = book.enroll(CsdRec::new(NtdPlatform::Win8, 0).unwrap_or_else(|_| CsdRec {
        platform: NtdPlatform::Win8, sp_level: 0, raw: String::new(), sealed: false,
    }));
    let _ = book.enroll(CsdRec::new(NtdPlatform::WinBlue, 0).unwrap_or_else(|_| CsdRec {
        platform: NtdPlatform::WinBlue, sp_level: 0, raw: String::new(), sealed: false,
    }));
    let _ = book.enroll(CsdRec::new(NtdPlatform::Win10, 0).unwrap_or_else(|_| CsdRec {
        platform: NtdPlatform::Win10, sp_level: 0, raw: String::new(), sealed: false,
    }));
    cs.add("J11", book.ledger.len() == 4 && book.audit() == 4, "book 4 platforms");
    // J12 摘要包含平台名
    cs.add("J12", r3.summary().contains("WIN7"), "summary label");
    // J13 注入五样
    let inj = inject_five();
    cs.add("J13", inj.iter().all(|&b| b), "inject five");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sp_limits() {
        assert!(CsdRec::new(NtdPlatform::Win7, 1).is_ok());
        assert_eq!(CsdRec::new(NtdPlatform::Win8, 1).err(), Some(SEG_CSD_FORBID));
        assert_eq!(CsdRec::new(NtdPlatform::Win7, 5).err(), Some(SEG_CSD_LEVEL));
    }

    #[test]
    fn test_bump_chain() {
        let mut r = CsdRec::new(NtdPlatform::Win7, 0).ok().unwrap_or_else(|| CsdRec {
            platform: NtdPlatform::Win7, sp_level: 0, raw: String::new(), sealed: false,
        });
        assert!(r.bump().is_ok());
        assert!(r.bump().is_err()); // Win7 上限 SP1
        assert!(r.consistent());
    }

    #[test]
    fn test_fold_stable() {
        let r = CsdRec::new(NtdPlatform::Win7, 1).ok().unwrap_or_else(|| CsdRec {
            platform: NtdPlatform::Win7, sp_level: 1, raw: "Service Pack 1".to_string(), sealed: false,
        });
        let a = fold(&[r.clone()]);
        let b = fold(&[r]);
        assert_eq!(a, b);
        assert_ne!(a, SEG_BASE);
    }
}
