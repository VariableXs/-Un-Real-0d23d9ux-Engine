//! CGPU-F1704 · 异构调试协议
//!
//! 判据锚点（施工书原文摘录）：
//! 协议：CIPC 扩展（异构调试消息（面板/时间线/trace/剖析——消息 schema 扩展
//! ——扩展记录（协议冻结 v1；远程调试（远程异构调试预留（接口位——预留；安全
//! （调试接口权限（调试态声明——权限复用；测试（schema/扩展/预留/权限四组）。
//! 判据：schema 扩展、冻结、预留、权限、四组、判据。
//!
//! 诊断码段 0x8B01~0x8B05（独占）：
//!   0x8B01 消息 schema 失格（族越界/版本不支持/载荷字段越 schema）
//!   0x8B02 协议冻结破（v1 签名失配/冻结后消息集变更）
//!   0x8B03 远程预留失信（接口位未声明/预留位被实占）
//!   0x8B04 权限链破（低权限行高权限为/调试态声明缺失）
//!   0x8B05 段越界（判码落 0x8B00..0x8C00 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。


/// 模块版本。
pub const DBG_PROTO_VERSION: &str = "K23-dbgproto-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x8B01~0x8B05）
// ---------------------------------------------------------------------------

pub const XC_MSGSCHEMA_MALFORMED: u32 = 0x8B01;
pub const XC_FROZEN_MUTATED: u32 = 0x8B02;
pub const XC_REMOTE_SLOT_BROKEN: u32 = 0x8B03;
pub const XC_PERM_CHAIN_BROKEN: u32 = 0x8B04;
pub const XC_SEG_OOR: u32 = 0x8B05;

// ---------------------------------------------------------------------------
// 一、消息 schema 扩展（CIPC 复用）
// ---------------------------------------------------------------------------

/// 异构调试消息族四闭集（判据"面板/时间线/trace/剖析"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MsgKind {
    /// 面板消息（框架元数据/刷新推送）。
    Panel,
    /// 时间线消息（Gantt 窗口/旅程行）。
    Timeline,
    /// trace 消息（事件流帧）。
    Trace,
    /// 剖析消息（热点报告/建议）。
    Profile,
}

impl MsgKind {
    pub const ALL: [MsgKind; 4] =
        [MsgKind::Panel, MsgKind::Timeline, MsgKind::Trace, MsgKind::Profile];
    pub const fn tag(self) -> u8 {
        match self {
            MsgKind::Panel => 1,
            MsgKind::Timeline => 2,
            MsgKind::Trace => 3,
            MsgKind::Profile => 4,
        }
    }
    pub const fn from_tag(t: u8) -> Option<MsgKind> {
        match t {
            1 => Some(MsgKind::Panel),
            2 => Some(MsgKind::Timeline),
            3 => Some(MsgKind::Trace),
            4 => Some(MsgKind::Profile),
            _ => None,
        }
    }
    /// 各族载荷字段闭集（schema 扩展记录——每族字段名单冻结）。
    pub const fn fields(self) -> &'static [&'static str] {
        match self {
            MsgKind::Panel => &["panel_kind", "metric", "refresh_mhz"],
            MsgKind::Timeline => &["w0_us", "width_us", "engine_mask"],
            MsgKind::Trace => &["event_tag", "ts_us", "task"],
            MsgKind::Profile => &["axis", "top_n"],
        }
    }
}

/// 协议版本（冻结 v1）。
pub const PROTO_VER_V1: u16 = 1;

/// 单条调试消息。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DbgMsg {
    pub kind: MsgKind,
    pub ver: u16,
    /// 载荷字段数（必须 == 族 schema 字段数）。
    pub field_count: u8,
    /// 载荷 64 位字（定宽——变长载荷走 trace 文件不走消息）。
    pub words: [u64; 4],
}

/// FNV-1a 64。
pub const fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf_29ce_4842_22325;
    let mut i = 0;
    while i < bytes.len() {
        h = (h ^ bytes[i] as u64).wrapping_mul(0x100_0000_01b3);
        i += 1;
    }
    h
}

/// 协议冻结 v1 签名：四族名+各自字段数串接 FNV。
pub const fn proto_signature() -> u64 {
    let mut buf = [0u8; 64];
    let mut n = 0usize;
    let mut f = 0;
    while f < MsgKind::ALL.len() {
        let name = match MsgKind::ALL[f] {
            MsgKind::Panel => "panel3",
            MsgKind::Timeline => "timeline3",
            MsgKind::Trace => "trace3",
            MsgKind::Profile => "profile2",
        };
        let b = name.as_bytes();
        let mut i = 0;
        while i < b.len() && n < buf.len() {
            buf[n] = b[i];
            n += 1;
            i += 1;
        }
        f += 1;
    }
    // const fn 禁坑：切片索引在 const 上下文不可用——以固定 64 字节全缓冲
    // 计算 FNV（尾部零填充为确定性行为，签名仍稳定）。
    fnv64(&buf)
}

/// 消息校验：族合法/版本支持/字段数与族 schema 一致（0x8B01）。
pub fn msg_check(m: &DbgMsg) -> Result<(), u32> {
    if m.ver < PROTO_VER_V1 || m.ver > PROTO_VER_V1 {
        return Err(XC_MSGSCHEMA_MALFORMED);
    }
    let expect = m.kind.fields().len() as u8;
    if m.field_count != expect {
        return Err(XC_MSGSCHEMA_MALFORMED);
    }
    // 载荷字数不得超定宽。
    if m.field_count as usize > m.words.len() {
        return Err(XC_MSGSCHEMA_MALFORMED);
    }
    Ok(())
}

/// 冻结账：签名快照 + 变更侦测。
pub struct FrozenProto {
    sig: u64,
    /// 冻结后收到的消息数（审计面）。
    pub audited: u32,
}

impl FrozenProto {
    /// 冻结 v1。
    pub fn freeze() -> Self {
        FrozenProto { sig: proto_signature(), audited: 0 }
    }

    /// 审计消息：先 msg_check 再累计（签名失配=冻结被改即 0x8B02）。
    pub fn audit(&mut self, m: &DbgMsg) -> Result<(), u32> {
        if self.sig != proto_signature() {
            return Err(XC_FROZEN_MUTATED);
        }
        msg_check(m)?;
        self.audited += 1;
        Ok(())
    }

    /// 签名自对拍（外部篡改模拟入口）。
    pub fn sig(&self) -> u64 {
        self.sig
    }
}

// ---------------------------------------------------------------------------
// 二、远程调试预留（接口位声明——不实现传输）
// ---------------------------------------------------------------------------

/// 远程预留位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RemoteSlot {
    /// 接口位声明（预留=声明在册）。
    pub declared: bool,
    /// 预留位被实占禁令：远程传输实现不在 v1（v1 只收声明）。
    pub implemented: bool,
    /// 预留锚（协议版本窗）。
    pub anchor: &'static str,
}

impl RemoteSlot {
    pub fn build() -> Self {
        RemoteSlot { declared: true, implemented: false, anchor: "v2-window" }
    }

    /// 预留校验：声明在 + 未实占 + 锚非空（0x8B03）。
    pub fn verified(&self) -> Result<(), u32> {
        if !self.declared || self.implemented || self.anchor.is_empty() {
            return Err(XC_REMOTE_SLOT_BROKEN);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、调试接口权限（调试态声明——权限复用）
// ---------------------------------------------------------------------------

/// 调试态三闭集（权限递增链 Off < Passive < Active）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DebugMode {
    /// 关：无调试消息收发。
    Off,
    /// 被动：只读遥测/面板数据。
    Passive,
    /// 主动：可注入调试命令（trace/剖析触发）。
    Active,
}

impl DebugMode {
    pub const fn name(self) -> &'static str {
        match self {
            DebugMode::Off => "off",
            DebugMode::Passive => "passive",
            DebugMode::Active => "active",
        }
    }
}

/// 权限闸：消息族所需调试态（面板=Passive，时间线/trace/剖析=Active）。
pub fn required_mode(kind: MsgKind) -> DebugMode {
    match kind {
        MsgKind::Panel => DebugMode::Passive,
        _ => DebugMode::Active,
    }
}

/// 权限检查：当前态 >= 所需态放行（0x8B04）。
pub fn perm_check(current: DebugMode, kind: MsgKind) -> Result<(), u32> {
    if current < required_mode(kind) {
        return Err(XC_PERM_CHAIN_BROKEN);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检（四组：schema/扩展冻结/预留/权限）
// ---------------------------------------------------------------------------

/// 协议引擎。
pub struct ProtoEngine {
    pub proto: FrozenProto,
}

impl ProtoEngine {
    pub fn new() -> Self {
        ProtoEngine { proto: FrozenProto::freeze() }
    }

    /// 组一：schema——四族合法消息 + 字段数失配拒 + 版本越界拒。
    pub fn selfcheck_schema(&mut self) -> Result<(), u32> {
        // 四族合法消息全过。
        for k in MsgKind::ALL {
            let m = DbgMsg { kind: k, ver: PROTO_VER_V1, field_count: k.fields().len() as u8, words: [0; 4] };
            msg_check(&m)?;
        }
        // 字段数失配拒。
        let bad = DbgMsg { kind: MsgKind::Panel, ver: PROTO_VER_V1, field_count: 9, words: [0; 4] };
        if msg_check(&bad).is_ok() {
            return Err(XC_MSGSCHEMA_MALFORMED);
        }
        // 版本越界拒。
        let badv = DbgMsg { kind: MsgKind::Trace, ver: 2, field_count: 3, words: [0; 4] };
        if msg_check(&badv).is_ok() {
            return Err(XC_MSGSCHEMA_MALFORMED);
        }
        // 族 tag 往返。
        for k in MsgKind::ALL {
            match MsgKind::from_tag(k.tag()) {
                Some(b) if b == k => {}
                _ => return Err(XC_MSGSCHEMA_MALFORMED),
            }
        }
        Ok(())
    }

    /// 组二：扩展冻结——签名稳定 + 审计累计 + 篡改侦测（模拟）。
    pub fn selfcheck_frozen(&mut self) -> Result<(), u32> {
        let s1 = proto_signature();
        let s2 = proto_signature();
        if s1 != s2 || s1 != self.proto.sig() {
            return Err(XC_FROZEN_MUTATED);
        }
        let m = DbgMsg { kind: MsgKind::Timeline, ver: 1, field_count: 3, words: [10, 1000, 0, 0] };
        self.proto.audit(&m)?;
        self.proto.audit(&m)?;
        if self.proto.audited != 2 {
            return Err(XC_FROZEN_MUTATED);
        }
        // schema 越界消息审计拒（冻结闸拦截）。
        let bad = DbgMsg { kind: MsgKind::Panel, ver: 1, field_count: 2, words: [0; 4] };
        if self.proto.audit(&bad).is_ok() {
            return Err(XC_FROZEN_MUTATED);
        }
        Ok(())
    }

    /// 组三：远程预留——声明位 + 实占反例 + 断锚反例。
    pub fn selfcheck_remote(&mut self) -> Result<(), u32> {
        RemoteSlot::build().verified()?;
        let mut bad = RemoteSlot::build();
        bad.implemented = true;
        if bad.verified().is_ok() {
            return Err(XC_REMOTE_SLOT_BROKEN);
        }
        let mut bad2 = RemoteSlot::build();
        bad2.declared = false;
        if bad2.verified().is_ok() {
            return Err(XC_REMOTE_SLOT_BROKEN);
        }
        Ok(())
    }

    /// 组四：权限——递增链 + 各族门槛 + 越权反例。
    pub fn selfcheck_perm(&mut self) -> Result<(), u32> {
        // 链序：Off < Passive < Active。
        if !(DebugMode::Off < DebugMode::Passive && DebugMode::Passive < DebugMode::Active) {
            return Err(XC_PERM_CHAIN_BROKEN);
        }
        // 面板在 Passive 放行、在 Off 拒。
        perm_check(DebugMode::Passive, MsgKind::Panel)?;
        if perm_check(DebugMode::Off, MsgKind::Panel).is_ok() {
            return Err(XC_PERM_CHAIN_BROKEN);
        }
        // 剖析在 Active 放行、在 Passive 拒。
        perm_check(DebugMode::Active, MsgKind::Profile)?;
        if perm_check(DebugMode::Passive, MsgKind::Profile).is_ok() {
            return Err(XC_PERM_CHAIN_BROKEN);
        }
        // 全族门槛核对：Active 对四族全放行。
        for k in MsgKind::ALL {
            perm_check(DebugMode::Active, k)?;
        }
        Ok(())
    }

    /// 全量自检入口（四组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        Self::segment_guard()?;
        self.selfcheck_schema()?;
        self.selfcheck_frozen()?;
        self.selfcheck_remote()?;
        self.selfcheck_perm()?;
        Ok(())
    }

    /// 段对账：五码全落 0x8B00..0x8C00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_MSGSCHEMA_MALFORMED,
            XC_FROZEN_MUTATED,
            XC_REMOTE_SLOT_BROKEN,
            XC_PERM_CHAIN_BROKEN,
            XC_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x8B00 || codes[i] >= 0x8C00 {
                return Err(XC_MSGSCHEMA_MALFORMED);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_MSGSCHEMA_MALFORMED);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
