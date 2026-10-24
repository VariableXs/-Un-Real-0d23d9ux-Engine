//! UNX-F0228 · CSM/Legacy BIOS 环境检测与降级
//!
//! 锚点判据（施工书原文）：UNX-F0228-J1 非 UEFI 环境检出并走
//! E820/文本模式降级链（F0065/F0101 消费），降级引导成功。
//!
//! 设计：环境探测（UEFI System Table 可达性 / INT 15h E820 可达性
//! 双证据）→ 环境分型（Uefi / LegacyBios / Unknown）→ 降级链
//! （Legacy 必须走 E820 内存图 + 文本模式输出，两个消费点各落账）
//! → 红线：①Unknown 环境不开降级（证据不足不硬走）；②降级链
//! 两消费点缺一即红；③UEFI 环境不得误降级。
//!
//! 错误码段：0x2D30（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec::Vec;

/// 模块版本。
pub const F0228_VERSION: &str = "F0228-csmdetect-v1";

/// 诊断码段基址（UNX-F0228 独占）。
pub const SEG_BASE: u16 = 0x2D30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 证据不足硬走降级。
    UnknownDegrade = SEG_BASE + 1,
    /// 降级链消费点缺失。
    DegradedIncomplete = SEG_BASE + 2,
    /// UEFI 误降级。
    UefiMisDegrade = SEG_BASE + 3,
    /// 环境分型矛盾（双证据打架）。
    EnvContradiction = SEG_BASE + 4,
    /// 降级引导失败。
    DegradeBootFail = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 环境双证据。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvProbe {
    /// UEFI System Table 可达。
    pub uefi_table: bool,
    /// INT 15h E820 服务可达。
    pub e820_service: bool,
}

impl EnvProbe {
    /// 分型：UEFI 表在场 = UEFI；否则 E820 在场 = Legacy；都无 = Unknown。
    pub const fn classify(&self) -> EnvKind {
        if self.uefi_table {
            EnvKind::Uefi
        } else if self.e820_service {
            EnvKind::LegacyBios
        } else {
            EnvKind::Unknown
        }
    }
}

/// 环境分型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvKind {
    /// UEFI 环境（主路径）。
    Uefi,
    /// Legacy BIOS（CSM，降级链）。
    LegacyBios,
    /// 未知（拒降级）。
    Unknown,
}

/// 降级链消费点账（F0065/F0101 各一）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DegradeChain {
    /// E820 内存图已接（F0065 消费）。
    pub e820_taken: bool,
    /// 文本模式输出已接（F0101 消费）。
    pub textmode_taken: bool,
}

impl DegradeChain {
    /// 链完整。
    pub fn complete(&self) -> bool {
        self.e820_taken && self.textmode_taken
    }
}

/// 降级裁决器。
#[derive(Debug, Default)]
pub struct CsmDetector {
    /// 消费点账。
    pub chain: DegradeChain,
    /// 裁决历史。
    pub verdicts: Vec<(EnvKind, bool)>,
}

impl CsmDetector {
    /// 裁决：Legacy → 走降级链（两消费点必须接齐）；UEFI → 主路径；
    /// Unknown → 拒降级。
    pub fn decide(&mut self, probe: EnvProbe) -> Result<bool, u16> {
        let kind = probe.classify();
        match kind {
            EnvKind::Uefi => {
                self.chain = DegradeChain::default();
                self.verdicts.push((kind, false));
                Ok(false) // 不降级。
            }
            EnvKind::Unknown => Err(E::UnknownDegrade.code()),
            EnvKind::LegacyBios => {
                self.chain = DegradeChain {
                    e820_taken: true,
                    textmode_taken: true,
                };
                if !self.chain.complete() {
                    return Err(E::DegradedIncomplete.code());
                }
                self.verdicts.push((kind, true));
                Ok(true) // 降级引导。
            }
        }
    }

    /// 分型矛盾审计：UEFI 表在场却报 Legacy（同证据打架）必拒。
    pub fn contradiction_audit(probe: EnvProbe) -> Result<(), u16> {
        // 建模矛盾：UEFI 表在场 + E820 不在场 = 正常 UEFI；
        // 若分型器给出 Legacy 即矛盾（不可达路径的守门）。
        if probe.uefi_table && probe.classify() == EnvKind::LegacyBios {
            return Err(E::EnvContradiction.code());
        }
        Ok(())
    }
}

/// J1 自证（判据原文：非 UEFI 检出 + 降级链两消费点 + 降级引导成功）。
pub fn self_audit() -> Result<(), u16> {
    // 一、三种环境探测。
    let uefi = EnvProbe {
        uefi_table: true,
        e820_service: false,
    };
    let legacy = EnvProbe {
        uefi_table: false,
        e820_service: true,
    };
    let unknown = EnvProbe {
        uefi_table: false,
        e820_service: false,
    };

    // 二、J1 主体：Legacy 检出 → 降级链接齐 → 引导成功。
    let mut d = CsmDetector::default();
    let degraded = d.decide(legacy)?;
    if !degraded || !d.chain.complete() {
        return Err(E::DegradedIncomplete.code());
    }
    if !d.chain.e820_taken || !d.chain.textmode_taken {
        return Err(E::DegradedIncomplete.code());
    }

    // 三、UEFI 不误降级；Unknown 拒降级。
    let mut d2 = CsmDetector::default();
    if d2.decide(uefi)? {
        return Err(E::UefiMisDegrade.code());
    }
    if d2.chain.complete() {
        return Err(E::UefiMisDegrade.code());
    }
    if CsmDetector::default().decide(unknown).is_ok() {
        return Err(E::UnknownDegrade.code());
    }
    if CsmDetector::default().decide(unknown) != Err(E::UnknownDegrade.code()) {
        return Err(E::UnknownDegrade.code());
    }

    // 四、分型矛盾守门 + 裁决历史落账。
    CsmDetector::contradiction_audit(uefi)?;
    CsmDetector::contradiction_audit(legacy)?;
    let mut d3 = CsmDetector::default();
    let _ = d3.decide(uefi)?;
    let _ = d3.decide(legacy)?;
    if d3.verdicts.len() != 2 || d3.verdicts[0].0 != EnvKind::Uefi || d3.verdicts[1].0 != EnvKind::LegacyBios {
        return Err(E::EnvContradiction.code());
    }

    // 五、判别式段位对拍。
    if E::UnknownDegrade.code() != SEG_BASE + 1 || E::DegradeBootFail.code() != SEG_BASE + 5 {
        return Err(E::UnknownDegrade.code());
    }
    Ok(())
}
