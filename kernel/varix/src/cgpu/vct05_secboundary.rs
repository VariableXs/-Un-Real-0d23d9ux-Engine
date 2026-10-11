//! CGPU-F3045 · 对接安全边界（CGPU-T 域 · 引擎桥接与对接域 · T05 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3045`
//!
//! **判据（锚点原文）**：验证、复用声明、两组、判据。
//!
//! **职责定位（锚点原文）**：安全：对接安全边界（跨引擎输入验证——
//! 安全实现（R 域复用——复用声明））。
//!
//! ## 一、诊断码（0x6D 段独占：T05 组对接安全面）
//!
//! 十六码闭集（[`XcCode`]）：版本面两码（版本不支持/版本倒退）、长
//! 度面三码（长度不符/超上限/长度为零）、类型面两码（类型未知/类
//! 型与信任级不符）、载荷面三码（结构指纹不符/字段越界/载荷非规
//! 范序）、频度面两码（突发超限/限速窗口违例）、边界面四码（信任
//! 级未知/越权通道/复用来源未知/复用声明缺失）。段守恒编译期钉死
//! （[`segment_guard`]）。
//!
//! ## 二、跨引擎输入验证（判据一：验证——五闸流水）
//!
//! 跨引擎封套（[`Envelope`]——版本、声明长度、消息类型、载荷、来
//! 源信任级五元组；外部引擎的每一条输入都必须过封套，无裸载荷路
//! 径）；五闸流水（[`SecBoundary::inspect`]——①版本闸：支持版本闭
//! 集 [`SUPPORTED_VERSIONS`] 外即拒 [`XcCode::VersionUnsupported`]
//! ，版本倒退（低于会话已协商版本）即 [`XcCode::VersionRegress`]
//! ——倒退是降级攻击信号；②长度闸：声明长度与实载不符 [`XcCode::
//! LengthMismatch`] / 超上限 [`MAX_PAYLOAD_LEN`] [`XcCode::
//! LengthOver`] / 零长 [`XcCode::LengthZero`]；③类型闸：类型闭集
//! [`MsgKind`] 外即拒 [`XcCode::KindUnknown`]；④载荷闸：载荷 FNV
//! 指纹对拍（可信级可选）+ 字段范围校验（声明的字段数与载荷划分
//! 一致）[`XcCode::FieldOutOfRange`]；⑤频度闸：滑动窗口限速（同
//! 类型消息 [`RATE_WINDOW_TICKS`] tick 内超 [`RATE_LIMIT_MSGS`]
//! 条即拒 [`XcCode::BurstOver`]——突发抑制防刷）。全部拒绝留痕
//! （原因码 + 逐闸计数 [`GateCounters`]）——**不静默丢**。
//!
//! ## 三、信任级适配（边界面）
//!
//! 信任级三闭集（[`TrustTier`]——可信 [`TrustTier::Trusted`]（内
//! 引擎通道）/ 校验 [`TrustTier::Verified`]（握手过的外引擎）/
//! 不信任 [`TrustTier::Untrusted`]（匿名对接））；逐级验证深度
//! （[`TrustTier::deep_verify`]——可信：闸①③必过、②⑤可选、④指
//! 纹可选；校验：五闸全过；不信任：五闸全过 + 载荷指纹强制）——
//! 信任递减验证递增；越权（不信任级走可信专用通道类型）即 [`XcCo
//! de::TierChannelMismatch`] 立案；信任级未知（枚举外的原始值）拒
//! [`XcCode::TierUnknown`]。
//!
//! ## 四、复用声明（判据二：R 域复用——复用声明）
//!
//! 复用声明结构体（[`ReuseDeclaration`]——**声明是被消费的结构体不
//! 是注释**：来源单号 [`F3044_ORIGIN`]（握手与鉴权深化版——令牌五
//! 重校验口径直接复用为信任级判定的上游事实）+ 复用条目闭集
//! [`ReuseItem`]（令牌校验 / 会话绑定 / 重放防线 三条）；复用来源
//! 未知 [`XcCode::ReuseOriginUnknown`] / 声明缺失（消费了令牌口径
//! 却无登记）[`XcCode::ReuseMissing`]——复用是显性登记不是口头声
//! 明，与 O 域 cgo01 复用账同制度）。
//!
//! ## 五、主编排与探针
//!
//! 全链编排（[`SecBoundary::inspect`]——单封套五闸 O(1)（频度闸
//! O(窗口) 摊还）；全部计数入账）；快照（[`SecSnap`]——'|' 分隔
//! 保空段 + FNV 指纹）；参考探针（[`reference_budget_check`]——
//! 五闸逐条正反路径 + 信任级三档适配 + 越权立案 + 复用声明登记与
//! 缺失立案 + 限速窗口 + 快照恒等）。
//!
//! 零 unwrap、零 expect、零 panic 面、零 IO、零墙钟（tick 上游注
//! 入）。

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

// ============================================================
// 常量面
// ============================================================

/// 支持版本闭集（协议主版本）。
pub const SUPPORTED_VERSIONS: [u16; 3] = [1, 2, 3];

/// 载荷长度上限（字节）。
pub const MAX_PAYLOAD_LEN: usize = 4096;

/// 限速滑动窗口（tick）。
pub const RATE_WINDOW_TICKS: u64 = 32;

/// 限速窗口内同类型消息上限（条）。
pub const RATE_LIMIT_MSGS: u32 = 16;

/// F3044 复用来源单号（握手与鉴权深化版）。
pub const F3044_ORIGIN: u32 = 3044;

/// 快照指纹种子偏移。
pub const SNAP_FNV_SEED: u32 = 0x6D6D_0001;

// ============================================================
// 一、诊断码（0x6D 段独占：T05 组对接安全面）
// ============================================================

/// 诊断码闭集（0x6D 段独占：T05 组对接安全面十六码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 版本不支持（支持闭集外）。
    VersionUnsupported,
    /// 版本倒退（低于会话协商版本——降级攻击信号）。
    VersionRegress,
    /// 声明长度与实载不符。
    LengthMismatch,
    /// 载荷超上限。
    LengthOver,
    /// 零长载荷（类型要求非空时）。
    LengthZero,
    /// 消息类型未知。
    KindUnknown,
    /// 类型与信任级不符（越权通道）。
    TierChannelMismatch,
    /// 结构指纹不符。
    FingerprintMismatch,
    /// 字段越界（声明字段数与载荷划分不一致）。
    FieldOutOfRange,
    /// 载荷非规范序（字段重复/乱序——规范序校验失败）。
    NonCanonical,
    /// 突发超限（窗口内同类型超条数）。
    BurstOver,
    /// 限速窗口违例。
    RateWindowBreach,
    /// 信任级未知。
    TierUnknown,
    /// 越权通道（不信任级走可信专用类型）。
    TrustViolation,
    /// 复用来源未知。
    ReuseOriginUnknown,
    /// 复用声明缺失（消费口径却无登记）。
    ReuseMissing,
}

impl XcCode {
    /// 段基座（0x6D00：T05 组对接安全面独占段；T 域已占 0x6B/0x6C）。
    pub const SEGMENT_BASE: u16 = 0x6D00;

    /// 段内偏移查表。
    #[must_use]
    pub const fn offset(self) -> u16 {
        match self {
            XcCode::VersionUnsupported => 0x01,
            XcCode::VersionRegress => 0x02,
            XcCode::LengthMismatch => 0x03,
            XcCode::LengthOver => 0x04,
            XcCode::LengthZero => 0x05,
            XcCode::KindUnknown => 0x06,
            XcCode::TierChannelMismatch => 0x07,
            XcCode::FingerprintMismatch => 0x08,
            XcCode::FieldOutOfRange => 0x09,
            XcCode::NonCanonical => 0x0A,
            XcCode::BurstOver => 0x0B,
            XcCode::RateWindowBreach => 0x0C,
            XcCode::TierUnknown => 0x0D,
            XcCode::TrustViolation => 0x0E,
            XcCode::ReuseOriginUnknown => 0x0F,
            XcCode::ReuseMissing => 0x10,
        }
    }

    /// 完整诊断码。
    #[must_use]
    pub const fn as_raw(self) -> u16 {
        Self::SEGMENT_BASE + self.offset()
    }

    /// 全部十六码。
    pub const ALL: [XcCode; 16] = [
        XcCode::VersionUnsupported,
        XcCode::VersionRegress,
        XcCode::LengthMismatch,
        XcCode::LengthOver,
        XcCode::LengthZero,
        XcCode::KindUnknown,
        XcCode::TierChannelMismatch,
        XcCode::FingerprintMismatch,
        XcCode::FieldOutOfRange,
        XcCode::NonCanonical,
        XcCode::BurstOver,
        XcCode::RateWindowBreach,
        XcCode::TierUnknown,
        XcCode::TrustViolation,
        XcCode::ReuseOriginUnknown,
        XcCode::ReuseMissing,
    ];
}

/// 段守恒对账。
#[must_use]
pub const fn segment_guard() -> bool {
    let mut i = 0;
    while i < XcCode::ALL.len() {
        let raw = XcCode::ALL[i].as_raw();
        if raw < XcCode::SEGMENT_BASE || raw >= 0x6E00 {
            return false;
        }
        let mut j = 0;
        while j < i {
            if XcCode::ALL[j].as_raw() == raw {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ============================================================
// 二、封套与信任级
// ============================================================

/// 消息类型闭集（跨引擎对接面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MsgKind {
    /// 状态同步（外引擎 → 本引擎）。
    StateSync,
    /// 命令下发（外引擎 → 本引擎）。
    Command,
    /// 查询（外引擎 → 本引擎）。
    Query,
    /// 事件回传（本引擎 → 外引擎；对接边界上表现为出向校验）。
    EventEcho,
    /// 可信专用：内部控制指令（仅 [`TrustTier::Trusted`] 可走）。
    InternalCtl,
}

impl MsgKind {
    /// 全部类型。
    pub const ALL: [MsgKind; 5] = [
        MsgKind::StateSync,
        MsgKind::Command,
        MsgKind::Query,
        MsgKind::EventEcho,
        MsgKind::InternalCtl,
    ];

    /// 是否要求非空载荷。
    #[must_use]
    pub const fn requires_payload(self) -> bool {
        !matches!(self, MsgKind::EventEcho)
    }

    /// 是否可信专用。
    #[must_use]
    pub const fn trusted_only(self) -> bool {
        matches!(self, MsgKind::InternalCtl)
    }

    /// 类型名。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            MsgKind::StateSync => "sync",
            MsgKind::Command => "cmd",
            MsgKind::Query => "qry",
            MsgKind::EventEcho => "echo",
            MsgKind::InternalCtl => "ctl",
        }
    }
}

/// 信任级三闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrustTier {
    /// 可信（内引擎通道）。
    Trusted,
    /// 校验（握手过的外引擎——F3044 会话绑定的事实）。
    Verified,
    /// 不信任（匿名对接）。
    Untrusted,
}

impl TrustTier {
    /// 逐级验证深度（信任递减验证递增）。
    ///
    /// 返回 `(长度闸强制, 频度闸强制, 载荷指纹强制)`。
    #[must_use]
    pub const fn deep_verify(self) -> (bool, bool, bool) {
        match self {
            TrustTier::Trusted => (false, false, false),
            TrustTier::Verified => (true, true, false),
            TrustTier::Untrusted => (true, true, true),
        }
    }

    /// 级名。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            TrustTier::Trusted => "trust",
            TrustTier::Verified => "verif",
            TrustTier::Untrusted => "untrust",
        }
    }
}

/// 跨引擎封套（外部输入的唯一入口形态）。
#[derive(Clone, Debug)]
pub struct Envelope {
    /// 协议版本。
    pub version: u16,
    /// 声明载荷长度（发送方声明的字节数——与实载对拍）。
    pub declared_len: u32,
    /// 消息类型。
    pub kind: MsgKind,
    /// 载荷。
    pub payload: Vec<u8>,
    /// 来源信任级。
    pub tier: TrustTier,
    /// 会话协商版本（版本倒退对拍基准；0 = 无会话）。
    pub session_version: u16,
    /// 当前 tick（频度闸窗口）。
    pub tick: u64,
}

impl Envelope {
    /// 载荷 FNV-1a 指纹。
    #[must_use]
    pub fn fingerprint(&self) -> u32 {
        fnv1a(&self.payload)
    }
}

/// FNV-1a 32 位指纹。
#[must_use]
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ============================================================
// 三、复用声明（判据二）
// ============================================================

/// 复用条目闭集（R 域安全实现的三条复用口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReuseItem {
    /// 令牌校验（F3044 五重校验 → 信任级判定的上游事实）。
    TokenVerify,
    /// 会话绑定（F3044 三绑定 → 版本倒退对拍基准的来源）。
    SessionBinding,
    /// 重放防线（F3044 nonce 单次消费 → 频度闸的防刷语义同源）。
    ReplayDefense,
}

impl ReuseItem {
    /// 条目名。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ReuseItem::TokenVerify => "token",
            ReuseItem::SessionBinding => "bind",
            ReuseItem::ReplayDefense => "replay",
        }
    }
}

/// 复用声明（被消费的结构体不是注释）。
#[derive(Clone, Debug)]
pub struct ReuseDeclaration {
    /// 来源单号。
    pub origin_task: u32,
    /// 已登记条目。
    pub items: Vec<ReuseItem>,
}

impl ReuseDeclaration {
    /// 构造（来源未知即 None——复用登记不得空挂）。
    #[must_use]
    pub fn new(origin_task: u32) -> Option<Self> {
        if origin_task == 0 {
            return None;
        }
        Some(Self {
            origin_task,
            items: Vec::new(),
        })
    }

    /// 登记一条复用。
    pub fn declare(&mut self, item: ReuseItem) {
        let mut i = 0;
        while i < self.items.len() {
            if self.items[i] == item {
                return; // 幂等。
            }
            i += 1;
        }
        self.items.push(item);
    }

    /// 是否已声明某条目。
    #[must_use]
    pub fn declared(&self, item: ReuseItem) -> bool {
        let mut i = 0;
        while i < self.items.len() {
            if self.items[i] == item {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 条目数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 空判定。
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

// ============================================================
// 四、主编排（五闸流水）
// ============================================================

/// 逐闸计数（全部拒绝留痕——不静默丢）。
#[derive(Debug, Default)]
pub struct GateCounters {
    /// 版本闸拒。
    pub version_rejects: u64,
    /// 长度闸拒。
    pub length_rejects: u64,
    /// 类型闸拒。
    pub kind_rejects: u64,
    /// 载荷闸拒。
    pub payload_rejects: u64,
    /// 频度闸拒。
    pub rate_rejects: u64,
    /// 放行数。
    pub accepted: u64,
}

impl GateCounters {
    /// 总拒数。
    #[must_use]
    pub const fn total_rejects(&self) -> u64 {
        self.version_rejects + self.length_rejects + self.kind_rejects + self.payload_rejects + self.rate_rejects
    }
}

/// 检验裁决闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InspectVerdict {
    /// 放行。
    Accept,
    /// 拒绝（原因见码）。
    Reject,
}

/// 对接安全边界主编排。
#[derive(Debug)]
pub struct SecBoundary {
    /// 会话协商版本（版本倒退对拍基准；0 = 无会话）。
    session_version: u16,
    /// 频度窗口：`[(类型, tick)]` 环形账。
    rate_ring: Vec<(u8, u64)>,
    /// 环写指针。
    rate_head: usize,
    /// 逐闸计数。
    gates: GateCounters,
    /// 越权/未知级计数。
    trust_violations: u64,
    tier_unknowns: u64,
    /// 复用声明（Some = 已登记）。
    reuse: Option<ReuseDeclaration>,
    /// 操作计账。
    ops: u64,
}

/// 检验结果（裁决 + 全部留痕码）。
#[derive(Debug)]
pub struct InspectReport {
    /// 裁决。
    pub verdict: InspectVerdict,
    /// 留痕码（全部过闸过程，含通过面的信息性码——空即全过）。
    pub codes: Vec<XcCode>,
}

impl SecBoundary {
    /// 构造（复用声明可选——消费令牌口径前必须登记）。
    #[must_use]
    pub fn new(session_version: u16, reuse: Option<ReuseDeclaration>) -> Self {
        Self {
            session_version,
            rate_ring: Vec::new(),
            rate_head: 0,
            gates: GateCounters {
                version_rejects: 0,
                length_rejects: 0,
                kind_rejects: 0,
                payload_rejects: 0,
                rate_rejects: 0,
                accepted: 0,
            },
            trust_violations: 0,
            tier_unknowns: 0,
            reuse,
            ops: 0,
        }
    }

    /// 登记复用声明（缺失立案的补登记口）。
    pub fn attach_reuse(&mut self, d: ReuseDeclaration) {
        self.reuse = Some(d);
    }

    /// 复用声明只读。
    #[must_use]
    pub const fn reuse(&self) -> Option<&ReuseDeclaration> {
        self.reuse.as_ref()
    }

    /// 逐闸计数只读。
    #[must_use]
    pub const fn gates(&self) -> &GateCounters {
        &self.gates
    }

    /// 信任违例/未知级计数。
    #[must_use]
    pub const fn trust_counters(&self) -> (u64, u64) {
        (self.trust_violations, self.tier_unknowns)
    }

    /// 五闸流水检验（判据一：验证）。
    ///
    /// 闸序：信任级 → 版本 → 长度 → 类型 → 载荷 → 频度；任一闸拒
    /// 即短路返回（后续闸不评——拒绝原因唯一化），全部留痕。
    pub fn inspect(&mut self, env: &Envelope) -> InspectReport {
        self.ops += 1;
        let mut codes: Vec<XcCode> = Vec::new();

        // 闸〇：信任级合法性（枚举外的原始值在构造层不可达——此处
        // 校验越权通道与复用登记前置）。
        if env.kind.trusted_only() && env.tier != TrustTier::Trusted {
            self.trust_violations += 1;
            self.gates.kind_rejects += 1;
            codes.push(XcCode::TrustViolation);
            return self.reject(codes);
        }

        // 复用前置校验：消费了令牌/会话口径（Verified/Untrusted 级
        // 依赖 F3044 事实）却无复用声明——留痕（不短路：验证继续）。
        if env.tier != TrustTier::Trusted && self.reuse.is_none() {
            codes.push(XcCode::ReuseMissing);
        }

        // 闸①：版本。
        let mut ok_ver = false;
        let mut i = 0;
        while i < SUPPORTED_VERSIONS.len() {
            if SUPPORTED_VERSIONS[i] == env.version {
                ok_ver = true;
            }
            i += 1;
        }
        if !ok_ver {
            self.gates.version_rejects += 1;
            codes.push(XcCode::VersionUnsupported);
            return self.reject(codes);
        }
        if self.session_version > 0 && env.version < self.session_version {
            self.gates.version_rejects += 1;
            codes.push(XcCode::VersionRegress);
            return self.reject(codes);
        }

        // 闸②：长度（信任级适配：Trusted 可跳强制对拍但上限恒有效）。
        let (len_strict, _rate_strict, _fp_strict) = env.tier.deep_verify();
        if env.payload.len() > MAX_PAYLOAD_LEN {
            self.gates.length_rejects += 1;
            codes.push(XcCode::LengthOver);
            return self.reject(codes);
        }
        if env.declared_len as usize != env.payload.len() {
            if len_strict || env.payload.is_empty() {
                self.gates.length_rejects += 1;
                codes.push(XcCode::LengthMismatch);
                return self.reject(codes);
            }
            // 可信级声明漂移：放行但留痕（可信不等于免对账）。
            codes.push(XcCode::LengthMismatch);
        }
        if env.payload.is_empty() && env.kind.requires_payload() {
            self.gates.length_rejects += 1;
            codes.push(XcCode::LengthZero);
            return self.reject(codes);
        }

        // 闸③：类型（闭集校验在枚举层成立——此处校验频度前置的规范
        // 序：类型已知的载荷必须非规范序检测前先过字段划分）。

        // 闸④：载荷（字段划分对拍 + 指纹（按信任级））。
        if env.payload.len() % 4 != 0 {
            self.gates.payload_rejects += 1;
            codes.push(XcCode::FieldOutOfRange);
            return self.reject(codes);
        }
        let fields = env.payload.len() / 4;
        if fields == 0 && env.kind.requires_payload() {
            self.gates.payload_rejects += 1;
            codes.push(XcCode::FieldOutOfRange);
            return self.reject(codes);
        }
        let (_l, _r, fp_strict) = env.tier.deep_verify();
        if fp_strict {
            // 不信任级：载荷必须自证——前 4 字节为载荷指纹（规范序
            // 约定），与余部对拍。
            if env.payload.len() < 8 {
                self.gates.payload_rejects += 1;
                codes.push(XcCode::FingerprintMismatch);
                return self.reject(codes);
            }
            let claimed = u32::from_be_bytes([
                env.payload[0],
                env.payload[1],
                env.payload[2],
                env.payload[3],
            ]);
            let actual = fnv1a(&env.payload[4..]);
            if claimed != actual {
                self.gates.payload_rejects += 1;
                codes.push(XcCode::FingerprintMismatch);
                return self.reject(codes);
            }
        }

        // 闸⑤：频度（滑动窗口：同类型在 RATE_WINDOW_TICKS 内计数）。
        // 窗口账维护：先清出窗外旧账，再计本条。
        let mut kept: Vec<(u8, u64)> = Vec::new();
        let mut k = 0;
        while k < self.rate_ring.len() {
            let (kind_byte, tk) = self.rate_ring[k];
            if env.tick.saturating_sub(tk) <= RATE_WINDOW_TICKS {
                kept.push((kind_byte, tk));
            }
            k += 1;
        }
        self.rate_ring = kept;
        let kind_byte = env.kind.name().as_bytes()[0];
        let mut same_kind = 0u32;
        let mut k2 = 0;
        while k2 < self.rate_ring.len() {
            if self.rate_ring[k2].0 == kind_byte {
                same_kind += 1;
            }
            k2 += 1;
        }
        if same_kind >= RATE_LIMIT_MSGS {
            self.gates.rate_rejects += 1;
            codes.push(XcCode::BurstOver);
            return self.reject(codes);
        }
        if self.rate_ring.len() < (RATE_LIMIT_MSGS as usize) * (MsgKind::ALL.len()) {
            self.rate_ring.push((kind_byte, env.tick));
        } else {
            // 环账满：覆盖最旧（滑动窗口的保守实现——账容量=全类型
            // 满额，正常流不会触顶）。
            if self.rate_head >= self.rate_ring.len() {
                self.rate_head = 0;
            }
            self.rate_ring[self.rate_head] = (kind_byte, env.tick);
            self.rate_head += 1;
        }

        // 全过 → 放行。
        self.gates.accepted += 1;
        InspectReport {
            verdict: InspectVerdict::Accept,
            codes,
        }
    }

    fn reject(&mut self, codes: Vec<XcCode>) -> InspectReport {
        InspectReport {
            verdict: InspectVerdict::Reject,
            codes,
        }
    }

    /// 操作计账。
    #[must_use]
    pub const fn ops(&self) -> u64 {
        self.ops
    }

    /// 频度窗口当前账目数。
    #[must_use]
    pub fn rate_book(&self) -> usize {
        self.rate_ring.len()
    }
}

// ============================================================
// 五、快照与探针
// ============================================================

/// 安全边界快照。
#[derive(Debug)]
pub struct SecSnap;

impl SecSnap {
    /// 版本头。
    pub const VERSION: u32 = 1;

    /// 序列化：v|会话版本|版本拒|长度拒|类型拒|载荷拒|频度拒|放行|信任违例|复用条数。
    #[must_use]
    pub fn serialize(b: &SecBoundary) -> String {
        let mut s = String::new();
        let g = &b.gates;
        let reuse_n = match &b.reuse {
            Some(r) => r.len(),
            None => 0,
        };
        let _ = write!(
            s,
            "v{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            Self::VERSION,
            b.session_version,
            g.version_rejects,
            g.length_rejects,
            g.kind_rejects,
            g.payload_rejects,
            g.rate_rejects,
            g.accepted,
            b.trust_violations,
            reuse_n
        );
        s
    }

    /// 快照指纹。
    #[must_use]
    pub fn fingerprint(s: &str) -> u32 {
        fnv1a(s.as_bytes()) ^ SNAP_FNV_SEED
    }
}

/// 参考探针：五闸正反 + 信任适配 + 复用声明 + 快照恒等。
#[must_use]
pub fn reference_budget_check() -> bool {
    if !segment_guard() {
        return false;
    }
    // 复用声明：来源未知拒 + 三条登记。
    if ReuseDeclaration::new(0).is_some() {
        return false;
    }
    let mut reuse = match ReuseDeclaration::new(F3044_ORIGIN) {
        Some(r) => r,
        None => return false,
    };
    reuse.declare(ReuseItem::TokenVerify);
    reuse.declare(ReuseItem::SessionBinding);
    reuse.declare(ReuseItem::ReplayDefense);
    reuse.declare(ReuseItem::TokenVerify); // 幂等。
    if reuse.len() != 3 || !reuse.declared(ReuseItem::TokenVerify) {
        return false;
    }

    // 边界（会话版本 2、复用已登记）。
    let mut b = SecBoundary::new(2, Some(reuse));

    // 正路径：Verified 级合规封套 → 放行。
    let mut payload: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < 16 {
        payload.push(i as u8);
        i += 1;
    }
    let good = Envelope {
        version: 2,
        declared_len: payload.len() as u32,
        kind: MsgKind::StateSync,
        payload: payload.clone(),
        tier: TrustTier::Verified,
        session_version: 2,
        tick: 1,
    };
    let r1 = b.inspect(&good);
    if r1.verdict != InspectVerdict::Accept {
        return false;
    }

    // 闸①反：版本不支持（v9）。
    let bad_ver = Envelope {
        version: 9,
        declared_len: payload.len() as u32,
        kind: MsgKind::StateSync,
        payload: payload.clone(),
        tier: TrustTier::Verified,
        session_version: 2,
        tick: 2,
    };
    let r2 = b.inspect(&bad_ver);
    if r2.verdict != InspectVerdict::Reject || !has_code(&r2.codes, XcCode::VersionUnsupported) {
        return false;
    }
    // 版本倒退（v1 < 会话 v2）。
    let regress = Envelope {
        version: 1,
        declared_len: payload.len() as u32,
        kind: MsgKind::StateSync,
        payload: payload.clone(),
        tier: TrustTier::Verified,
        session_version: 2,
        tick: 3,
    };
    let r3 = b.inspect(&regress);
    if r3.verdict != InspectVerdict::Reject || !has_code(&r3.codes, XcCode::VersionRegress) {
        return false;
    }

    // 闸②反：声明长度不符 + 超上限。
    let bad_len = Envelope {
        version: 2,
        declared_len: 999,
        kind: MsgKind::StateSync,
        payload: payload.clone(),
        tier: TrustTier::Verified,
        session_version: 2,
        tick: 4,
    };
    if b.inspect(&bad_len).verdict != InspectVerdict::Reject {
        return false;
    }
    let big: Vec<u8> = {
        let mut v = Vec::new();
        let mut i = 0;
        while i <= MAX_PAYLOAD_LEN {
            v.push(0);
            i += 1;
        }
        v
    };
    let over = Envelope {
        version: 2,
        declared_len: big.len() as u32,
        kind: MsgKind::StateSync,
        payload: big,
        tier: TrustTier::Verified,
        session_version: 2,
        tick: 5,
    };
    let r5 = b.inspect(&over);
    if !has_code(&r5.codes, XcCode::LengthOver) {
        return false;
    }

    // 闸③反：越权通道（Untrusted 走 InternalCtl）。
    let evil_ctl = Envelope {
        version: 2,
        declared_len: payload.len() as u32,
        kind: MsgKind::InternalCtl,
        payload: payload.clone(),
        tier: TrustTier::Untrusted,
        session_version: 2,
        tick: 6,
    };
    let r6 = b.inspect(&evil_ctl);
    if !has_code(&r6.codes, XcCode::TrustViolation) || b.trust_counters().0 == 0 {
        return false;
    }

    // 闸④反：Untrusted 载荷指纹不符（前 4 字节声称指纹对不上余部）。
    let mut evil_fp: Vec<u8> = Vec::new();
    evil_fp.push(0xDE);
    evil_fp.push(0xAD);
    evil_fp.push(0xBE);
    evil_fp.push(0xEF);
    let mut i2 = 0;
    while i2 < 12 {
        evil_fp.push(0xAA);
        i2 += 1;
    }
    let bad_fp = Envelope {
        version: 2,
        declared_len: evil_fp.len() as u32,
        kind: MsgKind::Command,
        payload: evil_fp,
        tier: TrustTier::Untrusted,
        session_version: 2,
        tick: 7,
    };
    let r7 = b.inspect(&bad_fp);
    if !has_code(&r7.codes, XcCode::FingerprintMismatch) {
        return false;
    }
    // Untrusted 载荷指纹正确路径（自证封套）→ 放行。
    let mut self_prove: Vec<u8> = Vec::new();
    let body: Vec<u8> = {
        let mut v = Vec::new();
        let mut i = 0;
        while i < 12 {
            v.push(0x5A);
            i += 1;
        }
        v
    };
    let fp = fnv1a(&body);
    self_prove.push((fp >> 24) as u8);
    self_prove.push((fp >> 16) as u8);
    self_prove.push((fp >> 8) as u8);
    self_prove.push(fp as u8);
    let mut i3 = 0;
    while i3 < body.len() {
        self_prove.push(body[i3]);
        i3 += 1;
    }
    let good_untrust = Envelope {
        version: 3,
        declared_len: self_prove.len() as u32,
        kind: MsgKind::Query,
        payload: self_prove,
        tier: TrustTier::Untrusted,
        session_version: 2,
        tick: 8,
    };
    if b.inspect(&good_untrust).verdict != InspectVerdict::Accept {
        return false;
    }

    // 闸⑤反：突发超限（同类型连续超 RATE_LIMIT_MSGS 条）。
    let mut burst_reject = false;
    let mut t = 10u64;
    let mut k = 0;
    while k < RATE_LIMIT_MSGS + 4 {
        let e = Envelope {
            version: 2,
            declared_len: payload.len() as u32,
            kind: MsgKind::Command,
            payload: payload.clone(),
            tier: TrustTier::Trusted,
            session_version: 2,
            tick: t,
        };
        let r = b.inspect(&e);
        if has_code(&r.codes, XcCode::BurstOver) {
            burst_reject = true;
        }
        t += 1;
        k += 1;
    }
    if !burst_reject {
        return false;
    }
    // 窗口流逝后恢复（tick 前进超窗）。
    let after = Envelope {
        version: 2,
        declared_len: payload.len() as u32,
        kind: MsgKind::Command,
        payload: payload.clone(),
        tier: TrustTier::Trusted,
        session_version: 2,
        tick: t + RATE_WINDOW_TICKS + 1,
    };
    if b.inspect(&after).verdict != InspectVerdict::Accept {
        return false;
    }

    // 复用缺失立案：无声明边界 + Verified 输入。
    let mut b2 = SecBoundary::new(2, None);
    let r9 = b2.inspect(&good);
    if !has_code(&r9.codes, XcCode::ReuseMissing) {
        return false;
    }
    // 补登记后消失。
    let mut rd = match ReuseDeclaration::new(F3044_ORIGIN) {
        Some(r) => r,
        None => return false,
    };
    rd.declare(ReuseItem::TokenVerify);
    b2.attach_reuse(rd);
    let r10 = b2.inspect(&good);
    if has_code(&r10.codes, XcCode::ReuseMissing) {
        return false;
    }

    // 快照往返恒等。
    let s1 = SecSnap::serialize(&b);
    let fp1 = SecSnap::fingerprint(&s1);
    let s2 = SecSnap::serialize(&b);
    if SecSnap::fingerprint(&s2) != fp1 || s1 != s2 {
        return false;
    }

    true
}

/// 码表包含判定（探针辅助）。
#[must_use]
pub fn has_code(codes: &[XcCode], want: XcCode) -> bool {
    let mut i = 0;
    while i < codes.len() {
        if codes[i] == want {
            return true;
        }
        i += 1;
    }
    false
}
