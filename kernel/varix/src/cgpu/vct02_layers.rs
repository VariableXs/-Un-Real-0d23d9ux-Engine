//! CGPU-F3042 · 对接协议分层模型（CGPU-T 域 · T02 命令通道协议组首单 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3042`
//!
//! **锚点原文**：协议分层模型（传输层/会话层/语义层三层——分层实现——
//! 分层表；测试（三层一组）。判据：分层表、一组、判据。
//!
//! # 一、三层闭集：传输层/会话层/语义层
//!
//! 对接协议的骨架是三层闭集（[LAYER_TABLE] 在册）：
//!
//! * **传输层**——帧封装与校验：字节在链路上的可靠搬运（帧头 + 校验位，
//!   翻转必拒——[transport_frame]/[transport_unframe]）。
//! * **会话层**——会话上下文与序号：S10 衔接包「多用户会话上下文传递
//!   约定」的协议承载面（[SessionChannel]，session_id 不匹配即拒——
//!   串户在协议层显性失败，F3041 SessionCtxTag 纪律的分层延续）。
//! * **语义层**——消息类型与负载：四段语义（命令/状态/资产/事件）的
//!   承载（[semantic_encode]/[semantic_decode]，未知类型必拒）。
//!
//! # 二、分层实现：自上而下封装，自下而上剥净
//!
//! 发送方向逐层加封套（语义头 → 会话头 → 传输帧），接收方向逐层剥净
//! 还原（[ProtocolStack::stack_send]/[stack_recv]）。任何一层校验不过
//! 即整链拒绝——分层不是为了把错误埋进下一层，而是为了让错误**停在
//! 它发生的那一层**（诊断码 0x5D01-0x5D05 逐层独占）。
//!
//! # 三、判据自查清单
//!
//! 分层表（三行齐、层号递增、名称互异、开销合计=栈总开销）；三层各
//! 功能往返+显性拒绝路径；全链路还原；域自检见 vct02_layers_checks。
//!
//! ## 错误契约：独占 0x5D 细分段（0x5A=vct01、0x5B=vcv01、0x5C=vcw01
//! 均已占用——段位经远端登记表逐一排查后取首个空段）
//!
//! 零 panic 面：表驱动 + `Option`/`Result`，无 `unwrap`/`expect`；
//! 长度先检后索引；序号递增走 `checked_add`。

use alloc::vec::Vec;

// ===========================================================================
// 一、版本与诊断码（vct02 独占 0x5Dxx 段）
// ===========================================================================

/// 版本标识（家族格式）。
pub const LAYERS_VERSION: &str = "T02-layers-v1";

/// 传输帧头字节数：长度 1 + 校验 2。
pub const TRANSPORT_OVERHEAD: u16 = 3;
/// 会话头字节数：session_id 4 + seq 2。
pub const SESSION_OVERHEAD: u16 = 6;
/// 语义头字节数：消息类型 1。
pub const SEMANTIC_OVERHEAD: u16 = 1;
/// 三层封套总开销（分层表合计口径）。
pub const STACK_OVERHEAD_TOTAL: u16 = TRANSPORT_OVERHEAD + SESSION_OVERHEAD + SEMANTIC_OVERHEAD;

/// 传输帧负载上限（u8 长度字段减帧头）。
pub const MAX_TRANSPORT_PAYLOAD: usize = 255 - TRANSPORT_OVERHEAD as usize;
/// 语义负载上限（传输负载减会话头与语义头）。
pub const MAX_SEMANTIC_PAYLOAD: usize = MAX_TRANSPORT_PAYLOAD - (SESSION_OVERHEAD + SEMANTIC_OVERHEAD) as usize;

/// 分层域诊断码（独占段；逐层独占——错误停在发生它的那一层）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LvCode(pub u16);

impl LvCode {
    /// 传输层：帧校验不匹配（字节翻转/损坏）。
    pub const TX_CHECKSUM: LvCode = LvCode(0x5D01);
    /// 传输层：帧长度非法（头不齐/长度字段不符/空帧/负载超限）。
    pub const TX_LENGTH: LvCode = LvCode(0x5D02);
    /// 会话层：会话不匹配（未建立/串户——session_id 不一致）。
    pub const SES_UNKNOWN: LvCode = LvCode(0x5D03);
    /// 会话层：序号非法（非严格递增——重放/乱序/溢出）。
    pub const SES_SEQ: LvCode = LvCode(0x5D04);
    /// 语义层：消息类型未知（四段闭集之外/缺类型字节）。
    pub const SEM_KIND: LvCode = LvCode(0x5D05);

    /// 短码。
    pub const fn code(self) -> u16 {
        self.0
    }
}

// ===========================================================================
// 二、分层表（判据一：分层表——三层闭集逐行在册）
// ===========================================================================

/// 分层表行：层号 + 层名 + 职责句 + 本层封套开销。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayerRow {
    /// 层号（1 起，自下而上：传输=1、会话=2、语义=3）。
    pub no: u8,
    /// 层名（判据逐字 grep 面）。
    pub name: &'static str,
    /// 职责一句话。
    pub duty: &'static str,
    /// 本层封套开销（字节）。
    pub overhead: u16,
}

/// 协议分层表（锚点原文「传输层/会话层/语义层三层」逐行落位；
/// 自下而上排列——接收方向剥净的顺序）。
pub const LAYER_TABLE: [LayerRow; 3] = [
    LayerRow {
        no: 1,
        name: "传输层",
        duty: "帧封装与校验——字节在链路上的可靠搬运",
        overhead: TRANSPORT_OVERHEAD,
    },
    LayerRow {
        no: 2,
        name: "会话层",
        duty: "会话上下文与序号——上下文传递不串户",
        overhead: SESSION_OVERHEAD,
    },
    LayerRow {
        no: 3,
        name: "语义层",
        duty: "消息类型与负载——四段语义的承载",
        overhead: SEMANTIC_OVERHEAD,
    },
];

/// 三层标识（闭集枚举；闭集外不猜测）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerId {
    /// 传输层（层号 1）。
    Transport,
    /// 会话层（层号 2）。
    Session,
    /// 语义层（层号 3）。
    Semantic,
}

impl LayerId {
    /// 层号（0 基索引）→ 层；闭集外 `None`。
    pub fn from_index(i: u8) -> Option<LayerId> {
        match i {
            0 => Some(LayerId::Transport),
            1 => Some(LayerId::Session),
            2 => Some(LayerId::Semantic),
            _ => None,
        }
    }

    /// 层名（与 [LAYER_TABLE] 同源口径）。
    pub const fn name(self) -> &'static str {
        match self {
            LayerId::Transport => "传输层",
            LayerId::Session => "会话层",
            LayerId::Semantic => "语义层",
        }
    }
}

/// 分层表审计：层号 1/2/3 严格递增 + 名称两两互异 + 职责句逐条
/// 非空 + 开销合计=栈总开销——分层表不是装饰，是对账面。
pub fn layer_audit() -> bool {
    // 层号递增 1..=3。
    let mut i = 0usize;
    while i < LAYER_TABLE.len() {
        if LAYER_TABLE[i].no != (i + 1) as u8 {
            return false;
        }
        i += 1;
    }
    // 名称两两互异。
    let mut i = 0usize;
    while i < LAYER_TABLE.len() {
        let mut j = i + 1;
        while j < LAYER_TABLE.len() {
            if LAYER_TABLE[i].name == LAYER_TABLE[j].name {
                return false;
            }
            j += 1;
        }
        // 职责句非空。
        if LAYER_TABLE[i].duty.is_empty() {
            return false;
        }
        i += 1;
    }
    // 开销合计=栈总开销。
    let mut total = 0u16;
    let mut k = 0usize;
    while k < LAYER_TABLE.len() {
        total += LAYER_TABLE[k].overhead;
        k += 1;
    }
    total == STACK_OVERHEAD_TOTAL
}

// ===========================================================================
// 三、语义层（层号 3：消息类型与负载——四段语义的承载）
// ===========================================================================

/// 语义消息类型闭集：1..=4 对应 T01 四段（命令/状态/资产/事件各 +1）。
pub const SEM_KIND_COMMAND: u8 = 1;
pub const SEM_KIND_STATE: u8 = 2;
pub const SEM_KIND_ASSET: u8 = 3;
pub const SEM_KIND_EVENT: u8 = 4;

/// 语义消息（解码产物）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SemanticMsg {
    /// 消息类型（闭集 1..=4）。
    pub kind: u8,
    /// 语义负载（原样承载，分层模型不解释内容）。
    pub payload: Vec<u8>,
}

/// 语义类型 → 四段索引（0..=3；与 F3041 Segment::from_index 对拍面）。
pub fn kind_to_segment_index(kind: u8) -> Option<u8> {
    match kind {
        SEM_KIND_COMMAND => Some(0),
        SEM_KIND_STATE => Some(1),
        SEM_KIND_ASSET => Some(2),
        SEM_KIND_EVENT => Some(3),
        _ => None,
    }
}

/// 语义层编码：`[kind][payload...]`。类型必须在闭集内——未知类型
/// 在编码端就显性失败，不等链路对端兜圈子。
pub fn semantic_encode(kind: u8, payload: &[u8]) -> Result<Vec<u8>, LvCode> {
    if kind_to_segment_index(kind).is_none() {
        return Err(LvCode::SEM_KIND);
    }
    let mut out = Vec::with_capacity(payload.len() + 1);
    out.push(kind);
    let mut i = 0usize;
    while i < payload.len() {
        out.push(payload[i]);
        i += 1;
    }
    Ok(out)
}

/// 语义层解码：缺类型字节或类型闭集外即拒（0x5D05）。
pub fn semantic_decode(bytes: &[u8]) -> Result<SemanticMsg, LvCode> {
    if bytes.is_empty() {
        return Err(LvCode::SEM_KIND);
    }
    let kind = bytes[0];
    if kind_to_segment_index(kind).is_none() {
        return Err(LvCode::SEM_KIND);
    }
    let mut payload = Vec::with_capacity(bytes.len() - 1);
    let mut i = 1usize;
    while i < bytes.len() {
        payload.push(bytes[i]);
        i += 1;
    }
    Ok(SemanticMsg { kind, payload })
}

// ===========================================================================
// 四、会话层（层号 2：会话上下文与序号——上下文传递不串户）
// ===========================================================================

/// 会话通道：一条会话的发收状态（S10 衔接包约定的协议承载面）。
/// session_id 与 F3041 SessionCtxTag 同源口径；发/收序号**两条独立
/// 流**各自严格递增——重放与乱序在会话层显性失败，且不会把本端
/// 自己的发送序号误判为对端重放。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SessionChannel {
    /// 会话标识（非零；零值=未初始化哨兵）。
    pub session_id: u32,
    /// 发送方向最大序号（wrap 严格递增）。
    pub tx_seq: u16,
    /// 接收方向最大序号（unwrap 严格递增校验）。
    pub rx_seq: u16,
    /// 通道是否开启。
    pub open: bool,
}

impl SessionChannel {
    /// 开启会话通道（session_id 零值=未初始化，显性拒绝）。
    pub fn new(session_id: u32) -> Result<SessionChannel, LvCode> {
        if session_id == 0 {
            return Err(LvCode::SES_UNKNOWN);
        }
        Ok(SessionChannel {
            session_id,
            tx_seq: 0,
            rx_seq: 0,
            open: true,
        })
    }

    /// 会话层封装：`[session_id:4 大端][seq:2 大端][payload...]`。
    /// seq 取发送流严格递增（溢出即拒绝——回绕会被当重放，显性失败）。
    pub fn wrap(&mut self, payload: &[u8]) -> Result<Vec<u8>, LvCode> {
        if !self.open || self.session_id == 0 {
            return Err(LvCode::SES_UNKNOWN);
        }
        let seq = match self.tx_seq.checked_add(1) {
            Some(s) => s,
            None => return Err(LvCode::SES_SEQ),
        };
        self.tx_seq = seq;
        let mut out = Vec::with_capacity(payload.len() + SESSION_OVERHEAD as usize);
        out.push((self.session_id >> 24) as u8);
        out.push((self.session_id >> 16) as u8);
        out.push((self.session_id >> 8) as u8);
        out.push((self.session_id & 0xFF) as u8);
        out.push((seq >> 8) as u8);
        out.push((seq & 0xFF) as u8);
        let mut i = 0usize;
        while i < payload.len() {
            out.push(payload[i]);
            i += 1;
        }
        Ok(out)
    }

    /// 会话层解封：会话不匹配（串户）拒 0x5D03；序号对**接收流**
    /// 非严格递增（重放/乱序）拒 0x5D04；通过则推进 rx_seq 并返回负载。
    pub fn unwrap(&mut self, bytes: &[u8]) -> Result<Vec<u8>, LvCode> {
        if !self.open || bytes.len() < SESSION_OVERHEAD as usize {
            return Err(LvCode::SES_UNKNOWN);
        }
        let sid = ((bytes[0] as u32) << 24)
            | ((bytes[1] as u32) << 16)
            | ((bytes[2] as u32) << 8)
            | (bytes[3] as u32);
        if sid != self.session_id {
            return Err(LvCode::SES_UNKNOWN);
        }
        let seq = ((bytes[4] as u16) << 8) | (bytes[5] as u16);
        if seq <= self.rx_seq {
            return Err(LvCode::SES_SEQ);
        }
        self.rx_seq = seq;
        let mut out = Vec::with_capacity(bytes.len() - SESSION_OVERHEAD as usize);
        let mut i = SESSION_OVERHEAD as usize;
        while i < bytes.len() {
            out.push(bytes[i]);
            i += 1;
        }
        Ok(out)
    }
}

// ===========================================================================
// 五、传输层（层号 1：帧封装与校验——字节在链路上的可靠搬运）
// ===========================================================================

/// 校验基（翻转敏感：payload 任一字节变化必改校验位）。
pub const TX_CHECK_BASE: u16 = 0x5D5B;

/// 传输帧校验：`base ^ (len*257) ^ sum(payload)`（确定性、整数算术、
/// 对单字节翻转必敏感——判据在 checks 逐位验证）。
pub fn tx_checksum(len: usize, payload: &[u8]) -> u16 {
    let mut sum = 0u16;
    let mut i = 0usize;
    while i < payload.len() {
        sum = sum.wrapping_add(payload[i] as u16);
        i += 1;
    }
    TX_CHECK_BASE ^ ((len as u16).wrapping_mul(257)) ^ sum
}

/// 传输层封装：`[len:1][ck_hi:1][ck_lo:1][payload...]`。
/// 空负载与超限负载都在发送端显性拒绝——空帧没有语义，超限帧
/// 出了发送端就没人能收。
pub fn transport_frame(payload: &[u8]) -> Result<Vec<u8>, LvCode> {
    if payload.is_empty() || payload.len() > MAX_TRANSPORT_PAYLOAD {
        return Err(LvCode::TX_LENGTH);
    }
    let ck = tx_checksum(payload.len(), payload);
    let mut out = Vec::with_capacity(payload.len() + TRANSPORT_OVERHEAD as usize);
    out.push(payload.len() as u8);
    out.push((ck >> 8) as u8);
    out.push((ck & 0xFF) as u8);
    let mut i = 0usize;
    while i < payload.len() {
        out.push(payload[i]);
        i += 1;
    }
    Ok(out)
}

/// 传输层解封：头不齐/长度字段不符/空帧拒 0x5D02；校验不匹配
/// 拒 0x5D01（字节在链路上被翻转=帧已损坏，不猜内容）。
pub fn transport_unframe(frame: &[u8]) -> Result<Vec<u8>, LvCode> {
    if frame.len() < TRANSPORT_OVERHEAD as usize {
        return Err(LvCode::TX_LENGTH);
    }
    let len = frame[0] as usize;
    if len == 0 || len != frame.len() - TRANSPORT_OVERHEAD as usize {
        return Err(LvCode::TX_LENGTH);
    }
    let ck = ((frame[1] as u16) << 8) | (frame[2] as u16);
    if ck != tx_checksum(len, &frame[TRANSPORT_OVERHEAD as usize..]) {
        return Err(LvCode::TX_CHECKSUM);
    }
    let mut out = Vec::with_capacity(len);
    let mut i = TRANSPORT_OVERHEAD as usize;
    while i < frame.len() {
        out.push(frame[i]);
        i += 1;
    }
    Ok(out)
}

// ===========================================================================
// 六、协议栈（三层贯通：自上而下封装、自下而上剥净）
// ===========================================================================

/// 对接协议栈：一条会话通道上的三层收发（分层实现的组装面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProtocolStack {
    /// 会话层通道（session_id + 序号状态）。
    pub session: SessionChannel,
}

impl ProtocolStack {
    /// 开栈（会话零值显性失败——未初始化不成栈）。
    pub fn new(session_id: u32) -> Result<ProtocolStack, LvCode> {
        Ok(ProtocolStack {
            session: SessionChannel::new(session_id)?,
        })
    }

    /// 全链路发送：语义编码 → 会话封装 → 传输成帧。
    /// 语义负载上限先检——超限在入口显性拒绝（0x5D02）。
    pub fn stack_send(&mut self, kind: u8, payload: &[u8]) -> Result<Vec<u8>, LvCode> {
        if payload.len() > MAX_SEMANTIC_PAYLOAD {
            return Err(LvCode::TX_LENGTH);
        }
        let sem = semantic_encode(kind, payload)?;
        let ses = self.session.wrap(&sem)?;
        transport_frame(&ses)
    }

    /// 全链路接收：传输解封 → 会话解封 → 语义解码。
    /// 每层校验不过即整链拒绝——错误停在发生它的那一层。
    pub fn stack_recv(&mut self, frame: &[u8]) -> Result<SemanticMsg, LvCode> {
        let ses = transport_unframe(frame)?;
        let sem = self.session.unwrap(&ses)?;
        semantic_decode(&sem)
    }
}

// ===========================================================================
// 七、栈级审计（判据三：判据——分层模型的整体对账）
// ===========================================================================

/// 栈级审计：分层表对账 + 语义闭集与四段对拍 + 全链路一个往返
/// （真实消息流经三层封装与三层剥净）——分层不是三段孤立代码。
pub fn stack_audit() -> bool {
    if !layer_audit() {
        return false;
    }
    // 语义闭集 1..=4 与四段索引 0..=3 逐一对拍。
    let mut kind = SEM_KIND_COMMAND;
    while kind <= SEM_KIND_EVENT {
        match kind_to_segment_index(kind) {
            Some(seg) => {
                if seg != kind - 1 {
                    return false;
                }
            }
            None => return false,
        }
        kind += 1;
    }
    if kind_to_segment_index(0).is_some() || kind_to_segment_index(5).is_some() {
        return false;
    }
    // 全链路往返：三层加封 → 三层剥净 → 原样还原。
    let mut st = match ProtocolStack::new(0xA11CE) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let payload: [u8; 5] = [0x11, 0x22, 0x33, 0x44, 0x55];
    let frame = match st.stack_send(SEM_KIND_COMMAND, &payload) {
        Ok(f) => f,
        Err(_) => return false,
    };
    if frame.len() != payload.len() + STACK_OVERHEAD_TOTAL as usize {
        return false;
    }
    match st.stack_recv(&frame) {
        Ok(msg) => msg.kind == SEM_KIND_COMMAND && msg.payload == payload,
        Err(_) => false,
    }
}
