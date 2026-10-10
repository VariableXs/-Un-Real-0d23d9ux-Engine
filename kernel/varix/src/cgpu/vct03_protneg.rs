//! CGPU-F3043 · 协议版本协商（CGPU-T 域 · VE 对接域 · T03 组 · 目标 300 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3043`
//!
//! **判据（锚点原文）**：能力交换、一组、判据。
//!
//! **职责定位（锚点原文）**：协商：版本协商（能力交换/降级约定——协商
//! 实现）。判据：能力交换、一组、判据。
//!
//! ## 一、能力交换（判据一：双向闭集求交）
//!
//! 能力闭集（[`Capability`]）：命令桥（T02 命令段）/状态桥（T03 状态段）
//! /资产桥（T04 资产段）/事件桥（T05 事件段）/压缩传输/批提交——六项
//! 闭集；本地能力 × 对端能力 → 交集（双方都支持才进协议面，单方支持
//! 不进）。交换是显性的：交集、单方缺失清单都回给调用方（哪些能力
//! 没谈成必须可见）。
//!
//! ## 二、版本协商与降级约定（判据一的版本面）
//!
//! 版本三元组（major.minor.patch，[`ProtoVersion`]）：major 不同不兼容
//! （显性拒绝——major 是语义断层不是小改）；minor 不同取 min（向后
//! 兼容口径——低者定协议）；patch 不参与协商（实现细节）。降级约定：
//! 无精确匹配版本时按 minor 取 min 即降级路径（降级显性标注
//! [`Negotiation::Downgraded`]，不静默装同版本）。
//!
//! ## 三、协商状态机
//!
//! [`Negotiator`] 三态（Hello→Exchange→Agreed/Failed）单向推进：
//! 跳步/回退显性拒（[`XcCode::PhaseViolation`]）；协商确定性：同输入
//! 同结论（两侧各自跑协商器结论必须一致——对拍前提）。
//!
//! **对接**：F3041/F3042（T 域开工与协议桥梁——本模块是版本面）；
//! F3044/F3045（握手鉴权/安全边界——协商通过后的下一步）。
//! 零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const PROTO_NEG_VERSION: &str = "T03-protneg-v1";

/// 本侧支持的最高协议 major（桥路 v1；变更走 ADR）。
pub const LOCAL_MAJOR: u32 = 1;

/// 本侧支持的 minor（与 LOCAL_MAJOR 配对）。
pub const LOCAL_MINOR: u32 = 4;

// ---------------------------------------------------------------------------
// 二、诊断码（0x6B 段独占：T03 协商面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x6B 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// major 不兼容（语义断层）。
    MajorMismatch,
    /// 协商相位违规（跳步/回退）。
    PhaseViolation,
    /// 能力位图越界。
    BadCapability,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::MajorMismatch => "major 不兼容：语义断层不可降级，建议对端升级或本侧双版本并存",
            XcCode::PhaseViolation => "协商相位违规：Hello→Exchange→Agreed 单向推进，跳步/回退显性拒",
            XcCode::BadCapability => "能力位图越界：六项闭集之外无位，建议按 Capability 闭集取用",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、能力交换（判据一）
// ---------------------------------------------------------------------------

/// 能力闭集六项（bit 位与 [`Capability`] 一一对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    /// 命令桥（T02 命令段）。
    CmdBridge,
    /// 状态桥（T03 状态段）。
    StateBridge,
    /// 资产桥（T04 资产段）。
    AssetBridge,
    /// 事件桥（T05 事件段）。
    EventBridge,
    /// 压缩传输。
    Compress,
    /// 批提交。
    BatchSubmit,
}

impl Capability {
    /// 位号（0-5）。
    pub const fn bit(self) -> u8 {
        match self {
            Capability::CmdBridge => 0,
            Capability::StateBridge => 1,
            Capability::AssetBridge => 2,
            Capability::EventBridge => 3,
            Capability::Compress => 4,
            Capability::BatchSubmit => 5,
        }
    }
}

/// 能力交换结论（显性：交集与缺失清单都可见）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exchange {
    /// 交集位图（双方都支持）。
    pub agreed_mask: u16,
    /// 单方缺失清单（人读；空串=无缺失）。
    pub missing: String,
}

impl Exchange {
    /// 交集能力判定。
    pub const fn has(&self, c: Capability) -> bool {
        (self.agreed_mask & (1u16 << c.bit())) != 0
    }
}

/// 能力交换（本地位图 × 对端位图 → 交集 + 缺失清单；确定性）。
pub fn exchange_capabilities(local: u16, remote: u16) -> Exchange {
    let agreed = local & remote;
    let mut missing = String::new();
    let mut i = 0u8;
    while i < 6 {
        let bit = 1u16 << i;
        // 任一侧缺即列缺失（协商可见性：谁缺、缺什么）。
        if (local & bit) == 0 || (remote & bit) == 0 {
            let name = match i {
                0 => "命令桥",
                1 => "状态桥",
                2 => "资产桥",
                3 => "事件桥",
                4 => "压缩传输",
                _ => "批提交",
            };
            if !missing.is_empty() {
                missing.push_str("、");
            }
            missing.push_str(name);
        }
        i += 1;
    }
    Exchange {
        agreed_mask: agreed,
        missing,
    }
}

// ---------------------------------------------------------------------------
// 四、版本协商与降级约定
// ---------------------------------------------------------------------------

/// 协议版本三元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtoVersion {
    /// major（语义断层；不同即不兼容）。
    pub major: u32,
    /// minor（向后兼容；取 min）。
    pub minor: u32,
    /// patch（实现细节；不参与协商）。
    pub patch: u32,
}

/// 协商结论三态（降级显性标注——不静默装同版本）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Negotiation {
    /// 精确匹配（major/minor 全同）。
    Exact,
    /// 降级达成（major 同、minor 取 min）。
    Downgraded { agreed_minor: u32 },
    /// 不可达（major 断层）。
    Incompatible,
}

/// 版本协商（判据一的版本面；确定性：双方各自跑结论一致）。
pub fn negotiate_version(local: ProtoVersion, remote: ProtoVersion) -> Negotiation {
    if local.major != remote.major {
        return Negotiation::Incompatible;
    }
    if local.minor == remote.minor {
        return Negotiation::Exact;
    }
    // minor 取 min（向后兼容口径：低者定协议——降级路径）。
    Negotiation::Downgraded {
        agreed_minor: if local.minor < remote.minor {
            local.minor
        } else {
            remote.minor
        },
    }
}

// ---------------------------------------------------------------------------
// 五、协商状态机
// ---------------------------------------------------------------------------

/// 协商相位三态（单向推进）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 问候（交换版本与能力位图前）。
    Hello,
    /// 交换中。
    Exchange,
    /// 已达成（终态）。
    Agreed,
    /// 已失败（终态）。
    Failed,
}

/// 协商器（本侧视角；对端各跑一份，结论必须一致——对拍前提）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Negotiator {
    phase: Phase,
    /// 本侧能力位图。
    pub local_mask: u16,
    /// 对端能力位图（Exchange 后有效）。
    pub remote_mask: u16,
    /// 协商结论（Agreed 后有效）。
    pub verdict: Option<Negotiation>,
}

impl Negotiator {
    /// 新协商器（Hello 相；本侧版本与能力注入）。
    pub const fn new(local_mask: u16) -> Negotiator {
        Negotiator {
            phase: Phase::Hello,
            local_mask,
            remote_mask: 0,
            verdict: None,
        }
    }

    /// 推进到交换相（对端版本+位图注入；跳步/回退显性拒）。
    pub fn exchange(
        &mut self,
        local_ver: ProtoVersion,
        remote_ver: ProtoVersion,
        remote_mask: u16,
    ) -> Result<Exchange, XcCode> {
        if self.phase != Phase::Hello {
            return Err(XcCode::PhaseViolation);
        }
        self.remote_mask = remote_mask;
        let ex = exchange_capabilities(self.local_mask, remote_mask);
        self.verdict = Some(negotiate_version(local_ver, remote_ver));
        self.phase = Phase::Exchange;
        Ok(ex)
    }

    /// 收口（Agreed / Failed 终态；跳步显性拒）。
    pub fn conclude(&mut self) -> Result<Negotiation, XcCode> {
        match self.phase {
            Phase::Exchange => {
                let v = self.verdict.unwrap_or(Negotiation::Incompatible);
                match v {
                    Negotiation::Incompatible => {
                        self.phase = Phase::Failed;
                        Err(XcCode::MajorMismatch)
                    }
                    _ => {
                        self.phase = Phase::Agreed;
                        Ok(v)
                    }
                }
            }
            _ => Err(XcCode::PhaseViolation),
        }
    }

    /// 当前相位（审计消费面）。
    pub const fn phase(&self) -> Phase {
        self.phase
    }
}
