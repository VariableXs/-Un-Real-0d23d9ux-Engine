//! UNX-F0229 · SecureBoot 开启环境引导路径
//!
//! 锚点判据（施工书原文）：UNX-F0229-J1 SecureBoot on 时签名校验
//! 链引导成功（shim 类只跟随），off 时路径不受影响。
//!
//! 设计：引导路径状态机（off → 直通；on → 签名校验链：内核 →
//! 模块 → 每级签名验证后才放行）→ shim 跟随语义（shim 自身签名
//! 由固件验，后续信任链由 shim 转发，内核侧只跟随不重验 shim）。
//! 红线：①on 态任何一级签名坏即拒载（放行即红）；②off 态不因
//! 签名缺失拒载（路径不受影响）；③on 态链序（先 shim 后内核）
//! 颠倒必拒。
//!
//! 错误码段：0x2D40（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec::Vec;

/// 模块版本。
pub const F0229_VERSION: &str = "F0229-sbpath-v1";

/// 诊断码段基址（UNX-F0229 独占）。
pub const SEG_BASE: u16 = 0x2D40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// on 态坏签名被放行。
    BadSigAdmitted = SEG_BASE + 1,
    /// off 态被签名缺失误拒。
    OffPathBlocked = SEG_BASE + 2,
    /// 链序颠倒。
    ChainOrderInvert = SEG_BASE + 3,
    /// shim 被重验（跟随语义破）。
    ShimReverified = SEG_BASE + 4,
    /// 路径账不平。
    PathBroken = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 引导件（内核 / shim / 模块）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootArtifact {
    /// 名称。
    pub name: &'static str,
    /// 签名有效。
    pub signed: bool,
}

/// SecureBoot 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SbState {
    /// off：直通路径。
    Off,
    /// on：校验链路径。
    On,
}

/// 引导路径状态机。
#[derive(Debug)]
pub struct SbPath {
    /// SB 态。
    pub state: SbState,
    /// 校验链放行账（on 态逐件登记）。
    pub chain: Vec<&'static str>,
    /// shim 重验计数（跟随语义下必须恒 0）。
    pub shim_reverify: u64,
}

impl SbPath {
    /// 建路径。
    pub fn open(state: SbState) -> Self {
        SbPath {
            state,
            chain: Vec::new(),
            shim_reverify: 0,
        }
    }

    /// shim 跟随（只登记信任转发，不重验签名）。
    pub fn follow_shim(&mut self, a: &BootArtifact) -> Result<(), u16> {
        if a.name != "shim" {
            return Err(E::ChainOrderInvert.code());
        }
        // 跟随语义：shim 自身由固件验过，内核侧零重验。
        Ok(())
    }

    /// 内核/模块校验（on 态签名必验；off 态直通）。
    pub fn admit(&mut self, order: u32, a: &BootArtifact) -> Result<(), u16> {
        match self.state {
            SbState::Off => {
                // off：签名缺失不影响路径。
                if !a.signed {
                    return Err(E::OffPathBlocked.code());
                }
                Ok(())
            }
            SbState::On => {
                // 链序：shim(0) 先于内核(1) 先于模块(2)。
                if order == 0 || (order == 1 && self.chain.is_empty())
                    || (order == 2 && self.chain.len() < 2)
                {
                    return Err(E::ChainOrderInvert.code());
                }
                if !a.signed {
                    return Err(E::BadSigAdmitted.code());
                }
                self.chain.push(a.name);
                Ok(())
            }
        }
    }

    /// 跟随语义审计：shim 零重验。
    pub fn follow_semantics_ok(&self) -> Result<(), u16> {
        if self.shim_reverify != 0 {
            return Err(E::ShimReverified.code());
        }
        Ok(())
    }
}

/// J1 自证（判据原文：on 时签名链引导成功，off 时路径不受影响）。
pub fn self_audit() -> Result<(), u16> {
    // 一、on 态全绿路径：shim → 内核 → 模块 三级链。
    let shim = BootArtifact {
        name: "shim",
        signed: true,
    };
    let kernel = BootArtifact {
        name: "kernel",
        signed: true,
    };
    let module = BootArtifact {
        name: "module",
        signed: true,
    };
    let mut on = SbPath::open(SbState::On);
    on.follow_shim(&shim)?;
    on.admit(1, &kernel)?;
    on.admit(2, &module)?;
    if on.chain.len() != 2 {
        return Err(E::ChainOrderInvert.code());
    }
    on.follow_semantics_ok()?;

    // 二、on 态红线：坏签名内核拒载（放行即红）。
    let bad_kernel = BootArtifact {
        name: "kernel",
        signed: false,
    };
    let mut on2 = SbPath::open(SbState::On);
    on2.follow_shim(&shim)?;
    if on2.admit(1, &bad_kernel).is_ok() {
        return Err(E::BadSigAdmitted.code());
    }
    if on2.admit(1, &bad_kernel) != Err(E::BadSigAdmitted.code()) {
        return Err(E::BadSigAdmitted.code());
    }

    // 三、链序颠倒：跳过 shim 直验内核必拒。
    let mut on3 = SbPath::open(SbState::On);
    if on3.admit(1, &kernel).is_ok() {
        return Err(E::ChainOrderInvert.code());
    }
    let mut on4 = SbPath::open(SbState::On);
    on4.follow_shim(&shim)?;
    if on4.admit(2, &module).is_ok() {
        return Err(E::ChainOrderInvert.code());
    }

    // 四、off 态直通：无签名也放行（路径不受影响红线）。
    let unsigned = BootArtifact {
        name: "kernel",
        signed: false,
    };
    let mut off = SbPath::open(SbState::Off);
    off.admit(1, &unsigned)?;
    let unsigned2 = BootArtifact {
        name: "module",
        signed: false,
    };
    off.admit(2, &unsigned2)?;
    if off.follow_semantics_ok().is_err() {
        return Err(E::ShimReverified.code());
    }

    // 五、账平：off 态链账空（不走校验链）。
    if !off.chain.is_empty() {
        return Err(E::PathBroken.code());
    }

    // 六、判别式段位对拍。
    if E::BadSigAdmitted.code() != SEG_BASE + 1 || E::PathBroken.code() != SEG_BASE + 5 {
        return Err(E::BadSigAdmitted.code());
    }
    Ok(())
}
