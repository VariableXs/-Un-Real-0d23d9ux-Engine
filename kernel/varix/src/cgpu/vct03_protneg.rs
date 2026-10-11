//! CGPU-F3043 · 协议版本协商（CGPU-T 域 · VE 对接域 · T03 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3043`
//!
//! **判据（锚点原文）**：能力交换、一组、判据。
//!
//! **职责定位（锚点原文）**：协商：版本协商（能力交换/降级约定——协商
//! 实现）。判据：能力交换、一组、判据。
//!
//! ## 一、能力交换（判据一：双向闭集求交 + 参数级协商）
//!
//! 能力闭集（[`Capability`]）：命令桥（T02 命令段）/状态桥（T03 状态段）
//! /资产桥（T04 资产段）/事件桥（T05 事件段）/压缩传输/批提交——六项
//! 闭集；本地能力 × 对端能力 → 交集（双方都支持才进协议面，单方支持
//! 不进）。交换是显性的：交集、单方缺失清单都回给调用方（哪些能力
//! 没谈成必须可见）。能力不止有无：带参能力有**参数级协商**
//! （[`CapParam`]：压缩算法号/批上限——参数取双方声明值的 min 或
//! 单值匹配，协商规则按参数类型钉死）。弃用面（[`DEPRECATED`]）：
//! 能力在哪个 minor 起弃用显性登记——弃用能力不进新剖面。
//!
//! ## 二、版本协商与降级约定（判据一的版本面）
//!
//! 版本三元组（major.minor.patch，[`ProtoVersion`]）：major 不同不兼容
//! （显性拒绝——major 是语义断层不是小改）；minor 不同取 min（向后
//! 兼容口径——低者定协议）；patch 不参与协商（实现细节）。多版本
//! 支持集（[`VersionSet`]）：本侧可声明支持多个 minor（双版本并存——
//! 对端挑双方交集最高 minor）；版本地板（[`VersionSet::floor`]）：
//! 低于地板的 minor 显性拒（地板以下是已知缺陷版——不协商回缺陷）。
//! 降级约定：无精确匹配版本时按 minor 取 min 即降级路径（降级显性
//! 标注 [`Negotiation::Downgraded`]，不静默装同版本）。
//!
//! ## 三、协商状态机与协商帧
//!
//! [`Negotiator`] 四相（Hello→Exchange→Agreed/Failed）单向推进：
//! 跳步/回退显性拒（[`XcCode::PhaseViolation`]）；交换相超时
//! （[`Negotiator::tick_exchange`]——[`EXCHANGE_TIMEOUT_TICKS`] 内未
//! 收口显性失败，不挂死）。协商帧（[`NegotiationFrame`]）三型
//! （Hello/Exchange/Conclude）带确定性编解码（[`NegotiationFrame::encode`]
//! / [`NegotiationFrame::decode`]——编解码往返逐位一致，线面无歧义）。
//! 会话剖面（[`SessionProfile`]）：Agreed 后产出的不可变协议剖面
//! （major/agreed minor/能力交集/参数表——桥路段装配的唯一输入）。
//!
//! ## 四、确定性对拍
//!
//! 协商确定性：同输入同结论（[`determinism_check`]：两侧各自跑协商器，
//! 结论必须一致——对拍前提；不一致即实现缺陷显性暴露）。
//!
//! **对接**：F3041/F3042（T 域开工与协议桥梁——本模块是版本面）；
//! F3044/F3045（握手鉴权/安全边界——协商通过后的下一步）。
//! 零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::String;
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

/// 本侧版本地板（低于此 minor 拒——已知缺陷版不协商回退）。
pub const LOCAL_MINOR_FLOOR: u32 = 2;

/// 能力闭集位宽（六项闭集之外无位——位图校验的唯一权威口径）。
pub const CAPABILITY_BITS: u8 = 6;

/// 交换相超时（tick 数；超时显性失败不挂死。当前策略值，可调）。
pub const EXCHANGE_TIMEOUT_TICKS: u64 = 64;

/// 压缩算法闭集（CapParam::CompressAlgo 的取值域）。
pub const COMPRESSION_ALGOS: [&str; 3] = ["none", "lz4-frame", "zstd-min"];

/// 批提交上限默认（CapParam::BatchMax 的默认协商值；当前策略值，可调）。
pub const BATCH_MAX_DEFAULT: u32 = 64;

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
    /// minor 低于地板（已知缺陷版——不协商回退）。
    BelowFloor,
    /// 交换相超时（对端停等超限——显性失败不挂死）。
    ExchangeTimeout,
    /// 帧解码失败（线面字节不符合帧格式——拒绝而非猜测）。
    FrameMalformed,
    /// 参数协商非法（取值域外——如压缩算法号越界）。
    BadParam,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::MajorMismatch => "major 不兼容：语义断层不可降级，建议对端升级或本侧双版本并存",
            XcCode::PhaseViolation => "协商相位违规：Hello→Exchange→Agreed 单向推进，跳步/回退显性拒",
            XcCode::BadCapability => "能力位图越界：六项闭集之外无位，建议按 Capability 闭集取用",
            XcCode::BelowFloor => "minor 低于地板：地板以下是已知缺陷版，建议对端升级过地板再协商",
            XcCode::ExchangeTimeout => "交换相超时：对端超限未收口，显性失败；建议查对端活性后重协商",
            XcCode::FrameMalformed => "帧解码失败：线面字节不符合帧格式；建议核对编码端版本与帧布局",
            XcCode::BadParam => "参数协商非法：取值域外（如压缩算法号越界）；建议按参数闭集取值",
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

    /// 人读名（缺失清单用）。
    pub const fn zh(self) -> &'static str {
        match self {
            Capability::CmdBridge => "命令桥",
            Capability::StateBridge => "状态桥",
            Capability::AssetBridge => "资产桥",
            Capability::EventBridge => "事件桥",
            Capability::Compress => "压缩传输",
            Capability::BatchSubmit => "批提交",
        }
    }

    /// 是否带参能力（压缩传输带算法号、批提交带上限——参数级协商面）。
    pub const fn has_params(self) -> bool {
        matches!(self, Capability::Compress | Capability::BatchSubmit)
    }
}

/// 弃用登记（能力 → 起始弃用 minor——弃用能力不进新剖面；
/// 空表示未弃用）。
pub const DEPRECATED: [Option<(u8, u32)>; 6] = [
    None,                       // 命令桥：未弃用
    None,                       // 状态桥：未弃用
    None,                       // 资产桥：未弃用
    None,                       // 事件桥：未弃用
    Some((Capability::Compress.bit(), 3)), // 压缩传输：minor 3 起弃用（改带参协商）
    None,                       // 批提交：未弃用
];

/// 能力弃用查询（给定运行 minor，能力是否已弃用）。
pub fn deprecated_at(c: Capability, running_minor: u32) -> bool {
    let entry = DEPRECATED[c.bit() as usize];
    match entry {
        Some((_, since)) => running_minor >= since,
        None => false,
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

/// 能力位图校验（闭集之外无位：bit 6..16 置位即越界——`BadCapability`
/// 的唯一产生点；协商入口两侧都校验，坏位图进不了协议面）。
pub fn validate_mask(mask: u16) -> Result<(), XcCode> {
    // 高位掩码：合法位 = (1<<CAPABILITY_BITS)-1；超集非空即越界。
    let legal = if CAPABILITY_BITS >= 16 {
        u16::MAX
    } else {
        (1u16 << CAPABILITY_BITS) - 1
    };
    if (mask & !legal) != 0 {
        return Err(XcCode::BadCapability);
    }
    Ok(())
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
// 四、参数级协商（带参能力的参数面）
// ---------------------------------------------------------------------------

/// 能力参数闭集（带参能力的参数键；无参能力无参数行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapParam {
    /// 压缩算法号（0=none/1=lz4-frame/2=zstd-min——COMPRESSION_ALGOS 下标）。
    CompressAlgo,
    /// 批提交上限（u32；取双方 min——低者定，防高位图对端溢出）。
    BatchMax,
}

impl CapParam {
    /// 归属能力。
    pub const fn owner(self) -> Capability {
        match self {
            CapParam::CompressAlgo => Capability::Compress,
            CapParam::BatchMax => Capability::BatchSubmit,
        }
    }

    /// 参数名（剖面装配与审计面）。
    pub const fn zh(self) -> &'static str {
        match self {
            CapParam::CompressAlgo => "压缩算法",
            CapParam::BatchMax => "批提交上限",
        }
    }
}

/// 单参数协商规则（确定性）：
/// - CompressAlgo：双方同值才成（算法是语义约定不是量级）；不同给 None。
/// - BatchMax：取 min（低者定——与 minor 同构的保守口径）。
pub fn negotiate_param(p: CapParam, local: u32, remote: u32) -> Option<u32> {
    match p {
        CapParam::CompressAlgo => {
            if local == remote && (local as usize) < COMPRESSION_ALGOS.len() {
                Some(local)
            } else {
                None
            }
        }
        CapParam::BatchMax => {
            // 上限必须非零；取 min 饱和。
            if local == 0 || remote == 0 {
                None
            } else {
                Some(if local < remote { local } else { remote })
            }
        }
    }
}

/// 参数协商批（能力交集内带参能力的全部参数结论——剖面装配的参数表）。
pub fn negotiate_params(
    agreed_mask: u16,
    local: &[(CapParam, u32)],
    remote: &[(CapParam, u32)],
) -> Result<Vec<(CapParam, u32)>, XcCode> {
    let mut out: Vec<(CapParam, u32)> = Vec::new();
    let mut i = 0usize;
    while i < local.len() {
        let (p, lv) = match local.get(i) {
            Some((p, v)) => (*p, *v),
            None => break,
        };
        // 能力未进交集 → 参数不协商（跳过不是失败）。
        if (agreed_mask & (1u16 << p.owner().bit())) == 0 {
            i += 1;
            continue;
        }
        // 对端找同键参数（缺失=BadParam——带参能力进交集就必须带参数声明）。
        let mut rv: Option<u32> = None;
        let mut j = 0usize;
        while j < remote.len() {
            if let Some((rp, rvalue)) = remote.get(j) {
                if *rp == p {
                    rv = Some(*rvalue);
                    break;
                }
            }
            j += 1;
        }
        let r = match rv {
            Some(r) => r,
            None => return Err(XcCode::BadParam),
        };
        match negotiate_param(p, lv, r) {
            Some(v) => out.push((p, v)),
            None => return Err(XcCode::BadParam),
        }
        i += 1;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 五、版本协商与降级约定
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

/// 多版本支持集（本侧声明：支持 [floor, top] 区间全部 minor——
/// 双版本并存的声明面；区间逆序显性拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionSet {
    /// 支持的 major（区间内全部 minor 都在此 major 下）。
    pub major: u32,
    /// 地板 minor（含）。
    pub floor: u32,
    /// 顶 minor（含）。
    pub top: u32,
}

impl VersionSet {
    /// 构造并校验（floor > top 显性拒）。
    pub fn build(major: u32, floor: u32, top: u32) -> Result<VersionSet, XcCode> {
        if floor > top {
            return Err(XcCode::BelowFloor);
        }
        Ok(VersionSet { major, floor, top })
    }

    /// 某 minor 是否在支持集内。
    pub const fn supports(&self, minor: u32) -> bool {
        minor >= self.floor && minor <= self.top
    }

    /// 与对端支持集求交的最高公共 minor（无交集 None——major 不同由
    /// 上层按断层处理）。
    pub const fn best_common(&self, other: &VersionSet) -> Option<u32> {
        if self.major != other.major {
            return None;
        }
        let lo = if self.floor > other.floor { self.floor } else { other.floor };
        let hi = if self.top < other.top { self.top } else { other.top };
        if lo > hi {
            None
        } else {
            Some(hi)
        }
    }
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

/// 支持集协商（多版本面的结论：取最高公共 minor 并按地板复核；
/// 断层/无交集/低于地板各有显性结论）。
pub fn negotiate_version_set(local: &VersionSet, remote: &VersionSet) -> Result<Negotiation, XcCode> {
    if local.major != remote.major {
        return Err(XcCode::MajorMismatch);
    }
    match local.best_common(remote) {
        None => Err(XcCode::BelowFloor),
        Some(minor) => {
            if minor == local.top && minor == remote.top {
                Ok(Negotiation::Exact)
            } else {
                Ok(Negotiation::Downgraded { agreed_minor: minor })
            }
        }
    }
}

/// 确定性对拍（判据「判据」的实现面：两侧各自以自己为 local 跑协商，
/// 结论的 agreed minor 必须一致——不一致即实现缺陷显性暴露）。
pub fn determinism_check(a: ProtoVersion, b: ProtoVersion) -> bool {
    let ab = negotiate_version(a, b);
    let ba = negotiate_version(b, a);
    match (ab, ba) {
        (Negotiation::Exact, Negotiation::Exact) => true,
        (
            Negotiation::Downgraded { agreed_minor: x },
            Negotiation::Downgraded { agreed_minor: y },
        ) => x == y,
        (Negotiation::Incompatible, Negotiation::Incompatible) => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// 六、协商帧（线面：确定性编解码——往返逐位一致）
// ---------------------------------------------------------------------------

/// 帧魔数（帧首两字节；0x56 0x58 = "VX"——错魔数即坏帧）。
pub const FRAME_MAGIC: [u8; 2] = [0x56, 0x58];

/// 协商帧三型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    /// 问候帧（major + 能力位图）。
    Hello,
    /// 交换帧（major.minor + 能力位图 + 两参数）。
    Exchange,
    /// 收口帧（结论码 + agreed minor）。
    Conclude,
}

/// 协商帧（三型统一布局：魔数 2B + 型号 1B + major 1B + minor 1B +
/// 位图 2B（LE）+ 参数 8B（两 u32 LE）——定长 15B，线面无歧义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NegotiationFrame {
    /// 帧型。
    pub kind: FrameKind,
    /// major。
    pub major: u32,
    /// minor（Hello 帧恒 0——问候不谈版本细节）。
    pub minor: u32,
    /// 能力位图。
    pub mask: u16,
    /// 参数一（CompressAlgo 号；无参帧恒 0）。
    pub param_a: u32,
    /// 参数二（BatchMax；无参帧恒 0）。
    pub param_b: u32,
}

/// 帧定长（编码布局字节数——decode 的长度门）。
pub const FRAME_LEN: usize = 15;

impl NegotiationFrame {
    /// 编码（定长 15B；确定性——同帧同字节）。
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(FRAME_LEN);
        out.push(FRAME_MAGIC[0]);
        out.push(FRAME_MAGIC[1]);
        out.push(match self.kind {
            FrameKind::Hello => 1,
            FrameKind::Exchange => 2,
            FrameKind::Conclude => 3,
        });
        out.push((self.major & 0xFF) as u8);
        out.push((self.minor & 0xFF) as u8);
        out.push((self.mask & 0xFF) as u8);
        out.push((self.mask >> 8) as u8);
        let a = self.param_a.to_le_bytes();
        let mut i = 0usize;
        while i < 4 {
            if let Some(b) = a.get(i) {
                out.push(*b);
            }
            i += 1;
        }
        let b = self.param_b.to_le_bytes();
        let mut i = 0usize;
        while i < 4 {
            if let Some(x) = b.get(i) {
                out.push(*x);
            }
            i += 1;
        }
        out
    }

    /// 解码（魔数/型号/长度三门校验——坏帧显性拒，不猜）。
    pub fn decode(bytes: &[u8]) -> Result<NegotiationFrame, XcCode> {
        if bytes.len() != FRAME_LEN {
            return Err(XcCode::FrameMalformed);
        }
        if bytes.first() != Some(&FRAME_MAGIC[0]) || bytes.get(1) != Some(&FRAME_MAGIC[1]) {
            return Err(XcCode::FrameMalformed);
        }
        let kind = match bytes.get(2) {
            Some(1) => FrameKind::Hello,
            Some(2) => FrameKind::Exchange,
            Some(3) => FrameKind::Conclude,
            _ => return Err(XcCode::FrameMalformed),
        };
        let get = |i: usize| -> Result<u8, XcCode> {
            bytes.get(i).copied().ok_or(XcCode::FrameMalformed)
        };
        let major = get(3)? as u32;
        let minor = get(4)? as u32;
        let lo = get(5)? as u16;
        let hi = get(6)? as u16;
        let mask = lo | (hi << 8);
        let mut pa = [0u8; 4];
        let mut i = 0usize;
        while i < 4 {
            pa[i] = get(7 + i)?;
            i += 1;
        }
        let mut pb = [0u8; 4];
        let mut i = 0usize;
        while i < 4 {
            pb[i] = get(11 + i)?;
            i += 1;
        }
        Ok(NegotiationFrame {
            kind,
            major,
            minor,
            mask,
            param_a: u32::from_le_bytes(pa),
            param_b: u32::from_le_bytes(pb),
        })
    }

    /// 从本侧状态构 Hello 帧。
    pub fn hello(major: u32, mask: u16) -> NegotiationFrame {
        NegotiationFrame {
            kind: FrameKind::Hello,
            major,
            minor: 0,
            mask,
            param_a: 0,
            param_b: 0,
        }
    }

    /// 从本侧状态构 Exchange 帧。
    pub fn exchange(ver: ProtoVersion, mask: u16, algo: u32, batch: u32) -> NegotiationFrame {
        NegotiationFrame {
            kind: FrameKind::Exchange,
            major: ver.major,
            minor: ver.minor,
            mask,
            param_a: algo,
            param_b: batch,
        }
    }
}

// ---------------------------------------------------------------------------
// 七、协商状态机与会话剖面
// ---------------------------------------------------------------------------

/// 协商相位四态（单向推进）。
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

/// 会话剖面（Agreed 后的不可变协议快照——桥路段装配唯一输入）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionProfile {
    /// 协议 major。
    pub major: u32,
    /// 达成 minor（Exact 与 Downgraded 的运行值同源此字段）。
    pub minor: u32,
    /// 能力交集位图。
    pub capability_mask: u16,
    /// 参数表（交集内带参能力的参数结论）。
    pub params: Vec<(CapParam, u32)>,
    /// 达成 tick（零墙钟记账）。
    pub agreed_at_tick: u64,
}

impl SessionProfile {
    /// 剖面内能力判定。
    pub const fn has(&self, c: Capability) -> bool {
        (self.capability_mask & (1u16 << c.bit())) != 0
    }

    /// 参数取值（无此参数 None）。
    pub fn param(&self, p: CapParam) -> Option<u32> {
        let mut i = 0usize;
        while i < self.params.len() {
            if let Some((k, v)) = self.params.get(i) {
                if *k == p {
                    return Some(*v);
                }
            }
            i += 1;
        }
        None
    }

    /// 弃用复核：剖面内不得含已弃用能力（按运行 minor——弃用面闭环）。
    pub fn no_deprecated(&self) -> bool {
        let caps = [
            Capability::CmdBridge,
            Capability::StateBridge,
            Capability::AssetBridge,
            Capability::EventBridge,
            Capability::Compress,
            Capability::BatchSubmit,
        ];
        let mut i = 0usize;
        while i < caps.len() {
            if let Some(c) = caps.get(i) {
                if self.has(*c) && deprecated_at(*c, self.minor) {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
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
    /// 交换相已等待 tick（超时面）。
    exchange_ticks: u64,
    /// 达成剖面（Agreed 后有效）。
    profile: Option<SessionProfile>,
}

impl Negotiator {
    /// 新协商器（Hello 相；本侧版本与能力注入）。
    pub const fn new(local_mask: u16) -> Negotiator {
        Negotiator {
            phase: Phase::Hello,
            local_mask,
            remote_mask: 0,
            verdict: None,
            exchange_ticks: 0,
            profile: None,
        }
    }

    /// 推进到交换相（对端版本+位图注入；跳步/回退显性拒；两侧位图
    /// 先过闭集校验——坏位图显性拒 [`XcCode::BadCapability`]）。
    pub fn exchange(
        &mut self,
        local_ver: ProtoVersion,
        remote_ver: ProtoVersion,
        remote_mask: u16,
    ) -> Result<Exchange, XcCode> {
        if self.phase != Phase::Hello {
            return Err(XcCode::PhaseViolation);
        }
        validate_mask(self.local_mask)?;
        validate_mask(remote_mask)?;
        // 地板复核（降级不得跌进缺陷版）。
        if remote_ver.major == local_ver.major && remote_ver.minor < LOCAL_MINOR_FLOOR {
            return Err(XcCode::BelowFloor);
        }
        self.remote_mask = remote_mask;
        let ex = exchange_capabilities(self.local_mask, remote_mask);
        self.verdict = Some(negotiate_version(local_ver, remote_ver));
        self.phase = Phase::Exchange;
        self.exchange_ticks = 0;
        Ok(ex)
    }

    /// 交换相计时（对端每 tick 未收口则累计；超限显性转 Failed）。
    pub fn tick_exchange(&mut self) -> Result<(), XcCode> {
        if self.phase != Phase::Exchange {
            return Err(XcCode::PhaseViolation);
        }
        self.exchange_ticks = self.exchange_ticks.saturating_add(1);
        if self.exchange_ticks >= EXCHANGE_TIMEOUT_TICKS {
            self.phase = Phase::Failed;
            return Err(XcCode::ExchangeTimeout);
        }
        Ok(())
    }

    /// 收口（Agreed / Failed 终态；跳步显性拒；达成同时产出会话剖面）。
    pub fn conclude(
        &mut self,
        local_ver: ProtoVersion,
        local_params: &[(CapParam, u32)],
        remote_params: &[(CapParam, u32)],
        at_tick: u64,
    ) -> Result<SessionProfile, XcCode> {
        match self.phase {
            Phase::Exchange => {
                let v = self.verdict.unwrap_or(Negotiation::Incompatible);
                match v {
                    Negotiation::Incompatible => {
                        self.phase = Phase::Failed;
                        Err(XcCode::MajorMismatch)
                    }
                    _ => {
                        // 参数协商（交集内带参能力——失败即整体失败）。
                        let mask = self.local_mask & self.remote_mask;
                        let params = negotiate_params(mask, local_params, remote_params)?;
                        let minor = match v {
                            Negotiation::Exact => local_ver.minor,
                            Negotiation::Downgraded { agreed_minor } => agreed_minor,
                            Negotiation::Incompatible => 0,
                        };
                        let profile = SessionProfile {
                            major: local_ver.major,
                            minor,
                            capability_mask: mask,
                            params,
                            agreed_at_tick: at_tick,
                        };
                        // 弃用复核（剖面不得携带已弃用能力）。
                        if !profile.no_deprecated() {
                            self.phase = Phase::Failed;
                            return Err(XcCode::BadCapability);
                        }
                        self.profile = Some(profile.clone());
                        self.phase = Phase::Agreed;
                        Ok(profile)
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

    /// 达成后的交集位图（Agreed 后有效；审计对账用——协议面只含交集）。
    pub const fn agreed_mask(&self) -> u16 {
        self.local_mask & self.remote_mask
    }

    /// 达成剖面（Agreed 后有效）。
    pub const fn profile(&self) -> Option<&SessionProfile> {
        self.profile.as_ref()
    }

    /// 对端版本与本侧版本的 minor 取 min（Agreed 后的实际运行 minor；
    /// Exact 时两侧同值，Downgraded 时为低者——消费方以此装配桥路段）。
    pub fn agreed_minor(&self, local_ver: ProtoVersion, remote_ver: ProtoVersion) -> u32 {
        if local_ver.minor < remote_ver.minor {
            local_ver.minor
        } else {
            remote_ver.minor
        }
    }
}
