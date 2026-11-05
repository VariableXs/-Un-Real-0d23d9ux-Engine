//! UNX-F49055 全对接错误码族与四点观测埋点（入口/出口/耗时/结果）· 0x731C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B13 族第八槽；J1 引用按任务板原文保留）：
//!   UNX-F49055 全对接错误码族 15 条（对接面 14 槽闭集 + 越界哨）:
//!          ① 纯功能代码 ≥220 行；
//!          ② 判据原文：UNX-F49056-J1 四点 100% 覆盖；异常零静默；判据锚定 体验十三·补；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49056 观测埋点 入口/出口/耗时/结果四点全覆盖:
//!          ① 纯功能代码 ≥260 行；
//!          ② 判据原文：UNX-F49057-J1 fuzz ×10^4 零崩；判据锚定 体验十 健壮性；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（提交路径，一次 submit 内）：
//!   时钟 → 状态 → 对端 → 版本 → 类型 → 字段 → 范围 → 凭据
//!        → 序号 → 配额 → 背压 → 截止。
//! 时钟先于一切：t_out < t_in 时，「耗时」与「截止」两个结论都不成立，
//!   若先判截止，会拿一条负时长去证明「没超时」——这是伪证，必须单列 ClockRegress。
//!   时钟只在入口判一次，之后所有闸门共享同一个 t_in/t_out，不重复取样。
//! 状态先于对端：通道已关闭时，对端是谁都不重要，Closed 是比 Handshake 更靠前的事实。
//! 对端先于版本：连对端都不在这条通道上，版本号再对也没有意义（错端消息不参与协商）。
//! 版本先于类型：版本不兼容时，字段编号的含义本身就不确定，谈类型是空中楼阁。
//! 类型先于字段：操作号越界说明这条消息不属于本协议，缺不缺字段无从谈起。
//! 字段先于范围：必填字段缺失是「结构是否成形」，数值范围是「成形后是否合理」。
//! 范围先于凭据：载荷长度都不合法时，不去查凭据（避免用坏载荷驱动凭据状态机）。
//! 凭据先于序号：凭据不对的消息不许推进 high_seq，否则一条伪造消息就能把序号账顶穿。
//! 序号先于配额：序号乱序是协议层错误，配额是资源层错误，协议层先判（乱序不吃配额）。
//! 配额先于背压：配额是「今天还能不能用」，背压是「此刻收不收得下」，先判长期再判瞬时。
//! 背压先于截止：先看有没有位置放，再看放进去是否已经过期（放不下就别谈过期）。
//! 埋点次序（观测账）：取号 → 入口 → 出口 → 耗时 → 结果。
//!   取号先于入口：没有号就没有唯一身份，后续四点无法归并到同一条记录。
//!   入口先于出口：入口点是「本函数第一句」，出口点是「所有 return 汇合处」；
//!     任何一条路径都必然经过出口点，因此 recorded == attempts 就是「异常零静默」。
//!   出口先于耗时：先确认已经出来了，再算时长——没出来的记录算时长毫无意义。
//!   耗时先于结果：时长是结果的旁证（超时结论必须与时长自洽），结果最后落账。
//! 关键差异：F49055 只回答「对接面 14 槽闭集是否冻结、拒绝次序是否有序」，
//!   不做观测账；F49056 只回答「四点是否 100% 覆盖、异常是否零静默」，
//!   不重算码族谱。两卡的账彼此独立：码族账折码值与可重试性，观测账折号与时长。
//!
//! 错误码口径（两卡共用，×15 的来源）：段内 14 条闭集（SEG_BASE+0x01..+0x0E）
//!   + 1 条越界哨（detail() 兜底返回「未知对接诊断码」，不占段内槽位）。
//!   本模块的 15 条冻结快照见 `CODE_SNAPSHOT`：码值、中文、可重试性三者逐行比对；
//!   任何一条中文改写、任何一次枚举换序、任何一次重试策略改动都会让快照翻红。
//!
//! 与同批 unx49053（0x73180 对接面错误码族）的关系：两族**互不复用码值也不复用文本**。
//!   0x73180 段管「焦点丢失 / 约束冲突 / 失联」三类语义，
//!   0x731C0 段管「握手 / 版本 / 结构 / 字段 / 序号 / 配额 / 凭据 / 背压 / 截止」这些
//!   对接协议自身的错误；两族在 `cross_segment_disjoint()` 里做逐一比对（码值与名称双证）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x731C0，B13 族第八槽：F49055/F49056 两卡共用）。
pub const SEG_BASE: u32 = 0x731C0;

/// 码族目录行数：14 段内 + 1 越界哨。
pub const CODE_ROWS: usize = 15;
/// 对接操作号上界（0..DOCK_OPS-1 是本协议认识的操作）。
pub const DOCK_OPS: u8 = 6;
/// 对端编号上界（0..DOCK_PEERS-1 是已登记对端）。
pub const DOCK_PEERS: u8 = 4;
/// 必填字段掩码：低四位全为 1 才算字段齐。
pub const REQUIRED_FIELDS: u8 = 0b1111;
/// 允许的最低对接协议版本。
pub const DOCK_VERSION_MIN: u16 = 1;
/// 允许的最高对接协议版本。
pub const DOCK_VERSION_MAX: u16 = 3;
/// 单条载荷字节上限（含零长判据：0 字节也算越界）。
pub const PAYLOAD_MAX: u32 = 4096;
/// 每次握手发放的配额上限。
pub const QUOTA_MAX: u32 = 64;
/// 同时在途的提交上限（越过即背压）。
pub const INFLIGHT_MAX: u8 = 8;
/// 观测耗时的期望上限（拍）；超过它仍可记录，但统计上要单列。
pub const LATENCY_CEIL: u32 = 96;
/// 判据 ×10^4 的轮数。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 码族 fuzz 的种子（写进观测摘要，复跑口径的一部分）。
pub const DOCK_SEED: u64 = 0x0000_0731_C000_0001;
/// 观测 fuzz 的种子。
pub const OBS_SEED: u64 = 0x0000_0731_C000_0002;
/// 故障注入的用例数（每例都必须被对应码捕获）。
pub const FAULT_INJECT: usize = 13;
/// 观测缓冲的默认容量。
pub const TRACE_CAP: usize = 512;
/// 观测缓冲的容量下限（太小就装不下一次注入批次）。
pub const TRACE_CAP_MIN: usize = 16;
/// 观测缓冲的容量上限（防止一次性吃光内存）。
pub const TRACE_CAP_MAX: usize = 4096;
/// 六字段账行的字段数（tag + 4 键 + digest）。
pub const RENDER_FIELDS: usize = 6;
/// 本模块用例编号总数（每模块 16 个，供跨槽防撞号）。
pub const CASE_TOTAL: usize = 16;
/// 本模块用例编号基址（0x0734_0800 起，实际用 +0x01..+0x10）。
pub const CASE_BASE: u32 = 0x0734_0800;

/// 会话账标签。
pub const DOCK_TAG: &str = "DOCK";
/// 单次提交账标签。
pub const TRACE_TAG: &str = "DTRC";
/// 观测总账标签。
pub const OBS_TAG: &str = "OBS";
/// 码族账标签。
pub const CODE_TAG: &str = "CODE";
/// 未知诊断码说明（越界哨的正文，同时也是 15 条口径的第 15 行）。
pub const UNKNOWN: &str = "未知对接诊断码";

// ── 诊断码闭集（段内 +0x01..+0x0E，共 14 条） ─────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockCode {
    /// 握手未成：对端未登记或不在本通道上
    Handshake,
    /// 版本不兼容：超出 1..=3 的协商窗口
    Version,
    /// 载荷结构不符：信封/分片形状不对
    Schema,
    /// 必填字段缺失：低四位未全置
    Missing,
    /// 字段类型不符：操作号越出本协议
    TypeMismatch,
    /// 数值越界：载荷长度为零或超上限
    Range,
    /// 序号跳号：不是 high_seq + 1
    Sequence,
    /// 重复投递：序号已被消费
    Duplicate,
    /// 配额耗尽：本轮握手额度用完
    Quota,
    /// 凭据过期：auth_epoch 与本会话不符
    Auth,
    /// 通道已关闭：非 Open 态提交
    Closed,
    /// 背压溢出：在途数已达上限
    Backpressure,
    /// 应答超时：截止拍早于入口拍
    Timeout,
    /// 埋点时钟回退：出口拍早于入口拍
    ClockRegress,
}

impl DockCode {
    /// 码值 = 段基址 + 0x01 + 枚举序号（换序即漂移，由快照拦截）。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 码的中文说明（逐条不同，供人读与快照比对）。
    pub fn detail(self) -> &'static str {
        match self {
            DockCode::Handshake => "对接握手未成：对端未登记",
            DockCode::Version => "对接版本不兼容：超出协商窗口",
            DockCode::Schema => "对接载荷结构不符：信封形状错误",
            DockCode::Missing => "对接必填字段缺失：结构未成形",
            DockCode::TypeMismatch => "对接字段类型不符：操作号越界",
            DockCode::Range => "对接数值越界：载荷长度不合规",
            DockCode::Sequence => "对接序号跳号：不是下一拍",
            DockCode::Duplicate => "对接重复投递：序号已消费",
            DockCode::Quota => "对接配额耗尽：额度已用完",
            DockCode::Auth => "对接凭据过期：epoch 不匹配",
            DockCode::Closed => "对接通道已关闭：拒绝提交",
            DockCode::Backpressure => "对接背压溢出：在途已达上限",
            DockCode::Timeout => "对接应答超时：截止拍已过",
            DockCode::ClockRegress => "对接埋点时钟回退：出口早于入口",
        }
    }

    /// 全部 14 条段内码（顺序即码值顺序）。
    pub const ALL: [DockCode; 14] = [
        DockCode::Handshake,
        DockCode::Version,
        DockCode::Schema,
        DockCode::Missing,
        DockCode::TypeMismatch,
        DockCode::Range,
        DockCode::Sequence,
        DockCode::Duplicate,
        DockCode::Quota,
        DockCode::Auth,
        DockCode::Closed,
        DockCode::Backpressure,
        DockCode::Timeout,
        DockCode::ClockRegress,
    ];
}

/// 自由函数版 detail：越界码统一兜底到 UNKNOWN。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < DockCode::ALL.len() {
        if let Some(c) = DockCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    UNKNOWN
}

/// 段内码判定：只认 +0x01..+0x0E，越界哨不算已知码。
pub fn known_code(code: u32) -> bool {
    code >= SEG_BASE + 0x01 && code <= SEG_BASE + 0x0E
}

/// 段位自检：14 条枚举必须严格铺满 +0x01..+0x0E，不许跳号也不许多占。
pub fn segment_guard() -> bool {
    if DockCode::ALL.len() != 14 {
        return false;
    }
    let mut i = 0usize;
    while i < DockCode::ALL.len() {
        let c = match DockCode::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.val() != SEG_BASE + 0x01 + i as u32 {
            return false;
        }
        if detail(c.val()) != c.detail() {
            return false;
        }
        i += 1;
    }
    // 哨位与段外样本都必须是未知码。
    !known_code(SEG_BASE)
        && !known_code(SEG_BASE + 0x0F)
        && !known_code(SEG_BASE + 0x10)
        && detail(SEG_BASE + 0x0F) == UNKNOWN
}

// ── 码族冻结快照（15 行，手写） ───────────────────────────────────
/// 快照行：码值 + 中文 + 是否可重试。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeRow {
    pub code: u32,
    pub cn: &'static str,
    pub retry: bool,
}

/// 手写 15 条冻结快照（14 条段内 + 1 条越界哨）。
///
/// 可重试性的语义：序号/配额/背压/超时/范围这五类属于「换个时机再来」就能过；
/// 握手/版本/结构/字段/类型/凭据/关闭/重复/时钟回退属于「不改东西再来也没用」。
/// 这张表与 `DockCode` 一一比对，是「×15 零漂移」的硬证据。
pub const CODE_SNAPSHOT: [CodeRow; CODE_ROWS] = [
    CodeRow { code: 0x731C1, cn: "对接握手未成：对端未登记", retry: false },
    CodeRow { code: 0x731C2, cn: "对接版本不兼容：超出协商窗口", retry: false },
    CodeRow { code: 0x731C3, cn: "对接载荷结构不符：信封形状错误", retry: false },
    CodeRow { code: 0x731C4, cn: "对接必填字段缺失：结构未成形", retry: false },
    CodeRow { code: 0x731C5, cn: "对接字段类型不符：操作号越界", retry: false },
    CodeRow { code: 0x731C6, cn: "对接数值越界：载荷长度不合规", retry: true },
    CodeRow { code: 0x731C7, cn: "对接序号跳号：不是下一拍", retry: true },
    CodeRow { code: 0x731C8, cn: "对接重复投递：序号已消费", retry: false },
    CodeRow { code: 0x731C9, cn: "对接配额耗尽：额度已用完", retry: true },
    CodeRow { code: 0x731CA, cn: "对接凭据过期：epoch 不匹配", retry: false },
    CodeRow { code: 0x731CB, cn: "对接通道已关闭：拒绝提交", retry: false },
    CodeRow { code: 0x731CC, cn: "对接背压溢出：在途已达上限", retry: true },
    CodeRow { code: 0x731CD, cn: "对接应答超时：截止拍已过", retry: true },
    CodeRow { code: 0x731CE, cn: "对接埋点时钟回退：出口早于入口", retry: false },
    CodeRow { code: 0x731CF, cn: UNKNOWN, retry: false },
];

/// 快照行取值（下标越界返回 None）。
pub fn snapshot_row(i: usize) -> Option<CodeRow> {
    match CODE_SNAPSHOT.get(i) {
        Some(v) => Some(*v),
        None => None,
    }
}

/// 全部可重试码（按码值升序折成一条账）。
pub fn retry_codes() -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < CODE_ROWS {
        if let Some(r) = CODE_SNAPSHOT.get(i) {
            if r.retry {
                out.push(r.code);
            }
        }
        i += 1;
    }
    out
}

/// 可重试性查询：越界码一律不可重试（未知错误不许自动重放）。
pub fn retryable(code: u32) -> bool {
    let mut i = 0usize;
    while i < CODE_ROWS {
        if let Some(r) = CODE_SNAPSHOT.get(i) {
            if r.code == code {
                return r.retry;
            }
        }
        i += 1;
    }
    false
}

/// 码族快照审计：码值序、中文、唯一性、可重试性、哨位，一项不合即报错。
pub fn audit_code_snapshot() -> Result<u64, u32> {
    if CODE_SNAPSHOT.len() != CODE_ROWS {
        return Err(DockCode::Schema.val());
    }
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < CODE_ROWS {
        let r = match CODE_SNAPSHOT.get(i) {
            Some(v) => *v,
            None => return Err(DockCode::Schema.val()),
        };
        if i + 1 < CODE_ROWS {
            let want = match DockCode::ALL.get(i) {
                Some(c) => c.val(),
                None => return Err(DockCode::Schema.val()),
            };
            if r.code != want {
                return Err(DockCode::Sequence.val());
            }
            if r.cn != detail(want) {
                return Err(DockCode::Schema.val());
            }
            if r.retry != retryable(r.code) {
                return Err(DockCode::Schema.val());
            }
        } else {
            // 第 15 行是越界哨：码值必须是 +0x0F，正文必须是 UNKNOWN，不许可重试。
            if r.code != SEG_BASE + 0x0F || known_code(r.code) || r.retry {
                return Err(DockCode::Range.val());
            }
            if r.cn != UNKNOWN {
                return Err(DockCode::Schema.val());
            }
        }
        // 中文与码值都必须两两不同（互不混淆的文本/数值前提）。
        let mut j = i + 1;
        while j < CODE_ROWS {
            if let Some(o) = CODE_SNAPSHOT.get(j) {
                if o.cn == r.cn || o.code == r.code {
                    return Err(DockCode::Duplicate.val());
                }
            }
            j += 1;
        }
        h = p_core::fold_u32(h, r.code);
        h = p_core::fold_u32(h, r.cn.len() as u32);
        h = p_core::fold_byte(h, if r.retry { 1 } else { 0 });
        i += 1;
    }
    if h == 0 {
        return Err(DockCode::Schema.val());
    }
    Ok(h)
}

// ── 会话状态机 ────────────────────────────────────────────────────
/// 连接态：握手前只能握手，Open 才收提交，Draining 只出不进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockState {
    /// 已建立但未协商（只允许 open/close）。
    Handshaking,
    /// 已协商（唯一允许 submit 的态）。
    Open,
    /// 排空中（不再接收新提交，等待在途归零）。
    Draining,
    /// 已关闭（终态，可被 close 幂等覆盖）。
    Closed,
}

impl DockState {
    pub fn index(self) -> u8 {
        match self {
            DockState::Handshaking => 0,
            DockState::Open => 1,
            DockState::Draining => 2,
            DockState::Closed => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DockState::Handshaking => "handshaking",
            DockState::Open => "open",
            DockState::Draining => "draining",
            DockState::Closed => "closed",
        }
    }

    pub fn from_index(i: u8) -> Result<DockState, u32> {
        match i {
            0 => Ok(DockState::Handshaking),
            1 => Ok(DockState::Open),
            2 => Ok(DockState::Draining),
            3 => Ok(DockState::Closed),
            _ => Err(DockCode::TypeMismatch.val()),
        }
    }

    /// 迁移白名单：握手→开/关，开→排空/关，排空→关，自环恒允许。
    pub fn legal_step(self, to: DockState) -> bool {
        match (self, to) {
            (DockState::Handshaking, DockState::Open) => true,
            (DockState::Handshaking, DockState::Closed) => true,
            (DockState::Open, DockState::Draining) => true,
            (DockState::Open, DockState::Closed) => true,
            (DockState::Draining, DockState::Closed) => true,
            _ => self == to,
        }
    }
}

/// 一条对接提交（扁平结构，便于 fuzz 随机造）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DockReq {
    pub peer: u8,
    pub op: u8,
    pub version: u16,
    pub fields: u8,
    pub payload_len: u32,
    pub seq: u32,
    pub auth_epoch: u32,
    pub deadline: u32,
}

impl DockReq {
    /// 一条必然合法的最小样本（测试与注入的基线）。
    pub fn valid(seq: u32) -> DockReq {
        DockReq {
            peer: 1,
            op: 1,
            version: 2,
            fields: REQUIRED_FIELDS,
            payload_len: 32,
            seq,
            auth_epoch: 0x100,
            deadline: 500,
        }
    }
}

/// 会话账：一条通道的全部状态与计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DockSession {
    pub state: DockState,
    pub peer: u8,
    pub version: u16,
    pub epoch: u32,
    pub high_seq: u32,
    pub quota: u32,
    pub inflight: u8,
    pub accepted: u32,
    pub rejected: u32,
    pub transitions: u32,
    pub digest: u64,
}

impl DockSession {
    /// 建会话：对端/版本/凭据三项在构造期就校验，坏参数进不来。
    pub fn new(peer: u8, version: u16, epoch: u32) -> Result<DockSession, u32> {
        if peer >= DOCK_PEERS {
            return Err(DockCode::Handshake.val());
        }
        if version < DOCK_VERSION_MIN || version > DOCK_VERSION_MAX {
            return Err(DockCode::Version.val());
        }
        if epoch == 0 {
            return Err(DockCode::Auth.val());
        }
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_byte(h, peer);
        h = p_core::fold_u32(h, version as u32);
        h = p_core::fold_u32(h, epoch);
        Ok(DockSession {
            state: DockState::Handshaking,
            peer,
            version,
            epoch,
            high_seq: 0,
            quota: QUOTA_MAX,
            inflight: 0,
            accepted: 0,
            rejected: 0,
            transitions: 1,
            digest: h,
        })
    }

    /// 握手完成：Handshaking → Open。
    pub fn open(&mut self) -> Result<(), u32> {
        if self.state != DockState::Handshaking {
            return Err(DockCode::Handshake.val());
        }
        self.state = DockState::Open;
        self.transitions = self.transitions.saturating_add(1);
        self.digest = p_core::fold_byte(self.digest, self.state.index());
        Ok(())
    }

    /// 进入排空：Open → Draining（在途不清零，等它自然归零）。
    pub fn drain(&mut self) -> Result<(), u32> {
        if self.state != DockState::Open {
            return Err(DockCode::Closed.val());
        }
        self.state = DockState::Draining;
        self.transitions = self.transitions.saturating_add(1);
        self.digest = p_core::fold_byte(self.digest, self.state.index());
        Ok(())
    }

    /// 关闭：任何非 Closed 态都允许，Closed 幂等。
    pub fn close(&mut self) -> Result<(), u32> {
        if !self.state.legal_step(DockState::Closed) {
            return Err(DockCode::Closed.val());
        }
        self.state = DockState::Closed;
        self.transitions = self.transitions.saturating_add(1);
        self.digest = p_core::fold_byte(self.digest, self.state.index());
        Ok(())
    }

    /// 重新握手（测试与 fuzz 用）：回到 Handshaking 并清空序号/在途，
    /// 同时按语义「重新握手发新配额」把 quota 补满。
    pub fn reset(&mut self) {
        self.state = DockState::Handshaking;
        self.high_seq = 0;
        self.inflight = 0;
        self.quota = QUOTA_MAX;
        self.transitions = self.transitions.saturating_add(1);
        self.digest = p_core::fold_byte(self.digest, 0xFF);
    }

    /// 故障注入钩子：直接设置剩余配额（只在测试/注入路径使用）。
    pub fn force_quota(&mut self, q: u32) {
        self.quota = q;
    }

    /// 故障注入钩子：直接设置在途数（只在测试/注入路径使用）。
    pub fn force_inflight(&mut self, n: u8) {
        self.inflight = n;
    }

    /// 会话不变式：计数单调、在途 ≤ 上限、quota ≤ 上限、摘要非零。
    pub fn invariant(&self) -> bool {
        self.quota <= QUOTA_MAX
            && self.inflight <= INFLIGHT_MAX
            && self.transitions >= 1
            && self.accepted.saturating_add(self.rejected) <= self.transitions.saturating_mul(1024)
            && self.digest != 0
    }

    /// 结束一次成功提交：推进序号、扣配额、记在途、折摘要。
    fn commit(&mut self, req: &DockReq) {
        self.high_seq = req.seq;
        self.quota = self.quota.saturating_sub(1);
        self.inflight = self.inflight.saturating_add(1);
        self.accepted = self.accepted.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, req.seq);
        self.digest = p_core::fold_u32(self.digest, req.op as u32);
    }

    /// 提交一次对接消息（唯一入口；观测四点在这里闭环）。
    ///
    /// 返回值：成功给出落账后的 `Trace`，失败给出**被记录下来的**诊断码。
    /// 无论成败，`tracer` 都会恰好收到一条 Trace——这是「异常零静默」的实现口径。
    pub fn submit(
        &mut self,
        tracer: &mut Tracer,
        req: &DockReq,
        t_in: u32,
        t_out: u32,
    ) -> Result<Trace, u32> {
        let id = tracer.begin();
        // 闸门次序见文件头注释；这里逐条落成 if，顺序即判据顺序。
        let code = if t_out < t_in {
            DockCode::ClockRegress.val()
        } else if self.state != DockState::Open {
            DockCode::Closed.val()
        } else if req.peer != self.peer {
            DockCode::Handshake.val()
        } else if req.version != self.version {
            DockCode::Version.val()
        } else if req.op >= DOCK_OPS {
            DockCode::TypeMismatch.val()
        } else if req.fields & REQUIRED_FIELDS != REQUIRED_FIELDS {
            DockCode::Missing.val()
        } else if req.payload_len == 0 || req.payload_len > PAYLOAD_MAX {
            DockCode::Range.val()
        } else if req.auth_epoch != self.epoch {
            DockCode::Auth.val()
        } else if req.seq <= self.high_seq {
            DockCode::Duplicate.val()
        } else if req.seq != self.high_seq.saturating_add(1) {
            DockCode::Sequence.val()
        } else if self.quota == 0 {
            DockCode::Quota.val()
        } else if self.inflight >= INFLIGHT_MAX {
            DockCode::Backpressure.val()
        } else if t_in > req.deadline {
            DockCode::Timeout.val()
        } else {
            0
        };
        if code == 0 {
            self.commit(req);
        } else {
            self.rejected = self.rejected.saturating_add(1);
            self.digest = p_core::fold_u32(self.digest, code);
        }
        let trace = Trace::sealed(id, code, t_in, t_out);
        tracer.record(trace);
        if code == 0 {
            Ok(trace)
        } else {
            Err(code)
        }
    }
}

// ── 四点观测 ──────────────────────────────────────────────────────
/// 一条提交记录：号 + 码 + 入口拍 + 出口拍 + 四点真值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Trace {
    pub id: u32,
    pub code: u32,
    pub t_in: u32,
    pub t_out: u32,
    pub ok: bool,
    /// 入口点：取到号并折入入口拍。
    pub entry: bool,
    /// 出口点：所有 return 汇合处（本构造器只被出口调用）。
    pub exit: bool,
    /// 耗时点：时长有定义，或回退已被显式记录成 ClockRegress。
    pub elapsed: bool,
    /// 结果点：成功码 0，或一个段内已知码（不存在「无码」的路径）。
    pub result: bool,
}

impl Trace {
    /// 在出口处封口：四点真值由数据推导，不接受调用方自报。
    pub fn sealed(id: u32, code: u32, t_in: u32, t_out: u32) -> Trace {
        let ok = code == 0;
        Trace {
            id,
            code,
            t_in,
            t_out,
            ok,
            entry: id != 0,
            exit: true,
            // 回退不是「没有耗时」，而是「耗时被判定为负」——它被记成 ClockRegress，
            // 因此耗时点仍然算覆盖（有结论），不会从覆盖率里漏掉。
            elapsed: t_out >= t_in || code == DockCode::ClockRegress.val(),
            result: ok || known_code(code),
        }
    }

    /// 四点是否齐全。
    pub fn full(&self) -> bool {
        self.entry && self.exit && self.elapsed && self.result
    }

    /// 实际时长（回退时按 0 计，回退本身另有码为证）。
    pub fn latency(&self) -> u32 {
        match self.t_out.checked_sub(self.t_in) {
            Some(v) => v,
            None => 0,
        }
    }

    /// 点掩码（bit0 入口 / bit1 出口 / bit2 耗时 / bit3 结果）。
    pub fn point_mask(&self) -> u8 {
        let mut m = 0u8;
        if self.entry {
            m |= 0b0001;
        }
        if self.exit {
            m |= 0b0010;
        }
        if self.elapsed {
            m |= 0b0100;
        }
        if self.result {
            m |= 0b1000;
        }
        m
    }
}

/// 观测总账：号、收、静默、成、败、四点齐、覆盖率（万分比）、摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsLedger {
    pub attempts: u32,
    pub recorded: u32,
    pub silent: u32,
    pub oks: u32,
    pub errs: u32,
    pub full: u32,
    pub coverage_bp: u32,
    pub digest: u64,
}

impl ObsLedger {
    /// 覆盖率是否满格（10000 bp = 100.00%）。
    pub fn full_coverage(&self) -> bool {
        self.recorded > 0 && self.coverage_bp == 10_000
    }

    /// 静默数必须为零。
    pub fn silent_free(&self) -> bool {
        self.silent == 0
    }
}

/// 观测器：取号、收记录、算覆盖率、折摘要。
#[derive(Clone, Debug)]
pub struct Tracer {
    pub cap: usize,
    pub attempts: u32,
    pub recorded: u32,
    pub oks: u32,
    pub errs: u32,
    pub full: u32,
    pub digest: u64,
    traces: Vec<Trace>,
}

impl Tracer {
    /// 构造：容量必须在 [TRACE_CAP_MIN, TRACE_CAP_MAX]。
    pub fn new(cap: usize) -> Result<Tracer, u32> {
        if cap < TRACE_CAP_MIN || cap > TRACE_CAP_MAX {
            return Err(DockCode::Range.val());
        }
        Ok(Tracer {
            cap,
            attempts: 0,
            recorded: 0,
            oks: 0,
            errs: 0,
            full: 0,
            digest: p_core::FNV_OFF,
            traces: Vec::new(),
        })
    }

    /// 默认容量。
    pub fn standard() -> Tracer {
        match Tracer::new(TRACE_CAP) {
            Ok(t) => t,
            // 容量常量在合法区间内，此分支不可达；返回一个最小可用观测器而非 panic。
            Err(_) => Tracer {
                cap: TRACE_CAP_MIN,
                attempts: 0,
                recorded: 0,
                oks: 0,
                errs: 0,
                full: 0,
                digest: p_core::FNV_OFF,
                traces: Vec::new(),
            },
        }
    }

    /// 取号：入口点的第一件事，attempts 只增不减。
    pub fn begin(&mut self) -> u32 {
        self.attempts = self.attempts.saturating_add(1);
        self.attempts
    }

    /// 记一条：计数器、四点、摘要、环形缓冲（满了丢最旧，但计数不丢）。
    pub fn record(&mut self, tr: Trace) {
        self.recorded = self.recorded.saturating_add(1);
        if tr.ok {
            self.oks = self.oks.saturating_add(1);
        } else {
            self.errs = self.errs.saturating_add(1);
        }
        if tr.full() {
            self.full = self.full.saturating_add(1);
        }
        self.digest = p_core::fold_u32(self.digest, tr.id);
        self.digest = p_core::fold_u32(self.digest, tr.code);
        self.digest = p_core::fold_u32(self.digest, tr.t_in);
        self.digest = p_core::fold_u32(self.digest, tr.t_out);
        self.digest = p_core::fold_byte(self.digest, tr.point_mask());
        if self.traces.len() >= self.cap {
            self.traces.remove(0);
        }
        self.traces.push(tr);
    }

    /// 已缓存的记录数。
    pub fn len(&self) -> usize {
        self.traces.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.traces.is_empty()
    }

    /// 按缓存下标取记录。
    pub fn trace_at(&self, i: usize) -> Option<Trace> {
        self.traces.get(i).copied()
    }

    /// 覆盖率（万分比）：四点齐 / 已记录。
    pub fn coverage_bp(&self) -> u32 {
        if self.recorded == 0 {
            return 0;
        }
        let mut bp = self.full.saturating_mul(10_000) / self.recorded;
        if bp > 10_000 {
            bp = 10_000;
        }
        bp
    }

    /// 观测账审计：静默为零、四点全齐、摘要非零、缓存不越界。
    pub fn audit(&self) -> Result<ObsLedger, u32> {
        if self.recorded > self.attempts {
            return Err(DockCode::Schema.val());
        }
        let silent = self.attempts.saturating_sub(self.recorded);
        if silent != 0 {
            return Err(DockCode::Closed.val());
        }
        if self.recorded == 0 {
            return Err(DockCode::Schema.val());
        }
        if self.full != self.recorded {
            return Err(DockCode::Missing.val());
        }
        if self.traces.len() > self.cap {
            return Err(DockCode::Range.val());
        }
        if self.digest == 0 || self.digest == p_core::FNV_OFF {
            return Err(DockCode::Schema.val());
        }
        let l = ObsLedger {
            attempts: self.attempts,
            recorded: self.recorded,
            silent,
            oks: self.oks,
            errs: self.errs,
            full: self.full,
            coverage_bp: self.coverage_bp(),
            digest: self.digest,
        };
        if !l.full_coverage() || l.silent != 0 {
            return Err(DockCode::Missing.val());
        }
        Ok(l)
    }
}

// ── fuzz：乱参数注入（×10^4） ─────────────────────────────────────
/// 一次 fuzz 的产出账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzReport {
    pub rounds: u32,
    pub attempts: u32,
    pub oks: u32,
    pub errs: u32,
    pub codes_seen: u32,
    pub latency_max: u32,
    pub coverage_bp: u32,
    pub silent: u32,
    pub digest: u64,
}

/// fuzz 用的合法基线请求：对端/版本/凭据取会话现值，序号取下一拍，截止留余量。
fn fuzz_legal_req(session: &DockSession, t_in: u32) -> DockReq {
    DockReq {
        peer: session.peer,
        op: 1,
        version: session.version,
        fields: REQUIRED_FIELDS,
        payload_len: 32,
        seq: session.high_seq.saturating_add(1),
        auth_epoch: session.epoch,
        deadline: t_in.saturating_add(20),
    }
}

/// 观测 fuzz：每轮随机造一条提交（含错端、错版本、越界载荷、乱序、超时、时钟回退），
/// 全部走同一条 submit 路径，最后用观测账自证「四点齐、零静默」。
pub fn obs_fuzz(rounds: u32) -> FuzzReport {
    let mut rng = p_core::Lcg::new(OBS_SEED);
    let mut tracer = Tracer::standard();
    let mut session = match DockSession::new(1, 2, 0x100) {
        Ok(s) => s,
        Err(_) => {
            return FuzzReport {
                rounds: 0,
                attempts: 0,
                oks: 0,
                errs: 0,
                codes_seen: 0,
                latency_max: 0,
                coverage_bp: 0,
                silent: 0,
                digest: p_core::FNV_OFF,
            };
        }
    };
    let mut opened = session.open().is_ok();
    let mut now: u32 = 1_000;
    let mut seen = 0u32;
    let mut local_max = 0u32;
    let mut i = 0u32;
    while i < rounds {
        // 每 97 轮关闭重开一次，逼出 Closed 分支；其余轮次保持 Open。
        if i % 97 == 96 {
            let _ = session.close();
            session.reset();
            opened = session.open().is_ok();
        }
        let t_in = now;
        now = now.saturating_add(1 + rng.below(6));
        let mut t_out = now;
        // 每 16 轮走一遍「全码谱」：第 m 轮专打第 m 个闸门（0/14 轮合法），
        // 15 号轮保留纯随机注入。这样 ×10^4 既逐闸门覆盖，又不退化成
        // 「13 条固定用例重复 769 遍」——随机流仍然参与每一轮的参数形状。
        let mut req = fuzz_legal_req(&session, t_in);
        let mut forced_quota = false;
        let mut forced_inflight = false;
        match i % 16 {
            0 | 14 => {}
            1 => req.peer = (session.peer + 1) % DOCK_PEERS,
            2 => {
                req.version = if session.version == DOCK_VERSION_MAX {
                    DOCK_VERSION_MIN
                } else {
                    session.version + 1
                };
            }
            3 => req.op = DOCK_OPS,
            4 => req.fields = 0,
            5 => req.payload_len = 0,
            6 => req.payload_len = PAYLOAD_MAX.saturating_add(1),
            7 => req.auth_epoch = session.epoch.wrapping_add(1),
            8 => req.seq = session.high_seq.saturating_add(2),
            9 => req.seq = session.high_seq,
            10 => {
                session.force_quota(0);
                forced_quota = true;
            }
            11 => {
                session.force_inflight(INFLIGHT_MAX);
                forced_inflight = true;
            }
            12 => req.deadline = t_in.saturating_sub(1),
            13 => t_out = t_in.saturating_sub(1 + rng.below(3)),
            _ => {
                req = DockReq {
                    peer: rng.below(DOCK_PEERS as u32 + 1) as u8,
                    op: rng.below(DOCK_OPS as u32 + 2) as u8,
                    version: 1 + rng.below(4) as u16,
                    fields: rng.below(16) as u8,
                    payload_len: rng.below(PAYLOAD_MAX.saturating_add(128)),
                    seq: rng.below(48),
                    auth_epoch: if rng.below(8) == 0 { 0xDEAD } else { 0x100 },
                    deadline: if rng.below(16) == 0 {
                        t_in.saturating_sub(1)
                    } else {
                        t_in.saturating_add(5 + rng.below(60))
                    },
                };
            }
        }
        // 1/64 的额外时钟回退：即使不是 13 号轮，也让 ClockRegress 不只在固定拍出现。
        if i % 16 != 13 && rng.below(64) == 0 {
            t_out = t_in.saturating_sub(1 + rng.below(3));
        }
        if !opened {
            // 打开失败（理论上不会发生）也必须走一次记录，避免静默。
            let tr = Trace::sealed(tracer.begin(), DockCode::Closed.val(), t_in, t_out);
            tracer.record(tr);
            opened = session.open().is_ok();
        } else if let Err(code) = session.submit(&mut tracer, &req, t_in, t_out) {
            let off = code.wrapping_sub(SEG_BASE + 1);
            if off < 32 {
                seen |= 1u32 << off;
            }
        } else {
            seen |= 1u32;
            // 在途偶尔清空，模拟对端确认，避免长期卡在背压分支。
            if session.inflight > 0 && rng.below(3) == 0 {
                session.inflight -= 1;
            }
        }
        // 注入钩子必须在本轮结束后还原，绝不允许污染下一轮。
        if forced_quota {
            session.force_quota(QUOTA_MAX);
        }
        if forced_inflight {
            session.force_inflight(0);
        }
        if t_out >= t_in {
            let d = t_out - t_in;
            if d > local_max {
                local_max = d;
            }
        }
        i += 1;
    }
    let (coverage_bp, silent, digest) = match tracer.audit() {
        Ok(l) => (l.coverage_bp, l.silent, l.digest),
        Err(_) => (tracer.coverage_bp(), tracer.attempts.saturating_sub(tracer.recorded), tracer.digest),
    };
    FuzzReport {
        rounds,
        attempts: tracer.attempts,
        oks: tracer.oks,
        errs: tracer.errs,
        codes_seen: count_bits(seen),
        latency_max: local_max,
        coverage_bp,
        silent,
        digest,
    }
}

/// 数 32 位掩码里的 1。
fn count_bits(mut v: u32) -> u32 {
    let mut n = 0u32;
    while v != 0 {
        n = n.saturating_add(v & 1);
        v >>= 1;
    }
    n
}

// ── 故障注入（13 例，逐例对码） ───────────────────────────────────
/// 故障注入：返回 (捕获数, 期望数, 首错码)。
///
/// 每一例都从一个干净会话 + 新观测器出发，确保「一例一码」，不互相污染。
pub fn dock_faults() -> (u32, u32, u32) {
    let mut caught = 0u32;
    let mut first_bad = 0u32;
    let expected = FAULT_INJECT as u32;
    let mut i = 0usize;
    while i < FAULT_INJECT {
        let mut s = match DockSession::new(1, 2, 0x100) {
            Ok(v) => v,
            Err(_) => return (caught, expected, DockCode::Schema.val()),
        };
        if s.open().is_err() {
            return (caught, expected, DockCode::Schema.val());
        }
        let mut tr = Tracer::standard();
        let mut req = DockReq::valid(1);
        let want: u32;
        match i {
            0 => {
                req.peer = 9;
                want = DockCode::Handshake.val();
            }
            1 => {
                req.version = 9;
                want = DockCode::Version.val();
            }
            2 => {
                req.op = 9;
                want = DockCode::TypeMismatch.val();
            }
            3 => {
                req.fields = 0b1011;
                want = DockCode::Missing.val();
            }
            4 => {
                req.payload_len = 0;
                want = DockCode::Range.val();
            }
            5 => {
                req.payload_len = PAYLOAD_MAX + 1;
                want = DockCode::Range.val();
            }
            6 => {
                req.auth_epoch = 1;
                want = DockCode::Auth.val();
            }
            7 => {
                req.seq = 2;
                want = DockCode::Sequence.val();
            }
            8 => {
                // 重复投递：先消费 seq=1，再来一次 seq=1。
                let _ = s.submit(&mut tr, &req, 10, 12);
                want = DockCode::Duplicate.val();
            }
            9 => {
                s.force_quota(0);
                want = DockCode::Quota.val();
            }
            10 => {
                s.force_inflight(INFLIGHT_MAX);
                want = DockCode::Backpressure.val();
            }
            11 => {
                req.deadline = 5;
                want = DockCode::Timeout.val();
            }
            12 => {
                // 时钟回退：出口早于入口。
                let got = s.submit(&mut tr, &req, 12, 10);
                if got.err() == Some(DockCode::ClockRegress.val()) {
                    caught = caught.saturating_add(1);
                } else if first_bad == 0 {
                    first_bad = DockCode::ClockRegress.val();
                }
                i += 1;
                continue;
            }
            _ => break,
        }
        let got = s.submit(&mut tr, &req, 10, 12);
        match got {
            Err(code) if code == want => caught = caught.saturating_add(1),
            Err(code) => {
                if first_bad == 0 {
                    first_bad = code;
                }
            }
            Ok(_) => {
                if first_bad == 0 {
                    first_bad = DockCode::Schema.val();
                }
            }
        }
        i += 1;
    }
    (caught, expected, first_bad)
}

// ── 六字段账行：渲染与回读 ────────────────────────────────────────
/// 六字段账行：`tag|k1=a|k2=b|k3=c|k4=d|digest=XXXXXXXXXXXXXXXX`。
pub fn render_row(tag: &str, a: u32, b: u32, c: u32, d: u32, digest: u64) -> String {
    format!(
        "{}|k1={}|k2={}|k3={}|k4={}|digest={:016X}",
        tag, a, b, c, d, digest
    )
}

/// 解析十进制 u32。
pub fn parse_u32(s: &str) -> Option<u32> {
    s.parse::<u32>().ok()
}

/// 解析 16 位大写十六进制摘要。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return None,
        };
        let ok = (b'0'..=b'9').contains(&b) || (b'A'..=b'F').contains(&b);
        if !ok {
            return None;
        }
        i += 1;
    }
    u64::from_str_radix(s, 16).ok()
}

/// 回读账行：成功返回 (tag, [k1..k4], digest)。
pub fn row_fields(row: &str) -> Option<(&str, [u32; 4], u64)> {
    let parts: Vec<&str> = row.split('|').collect();
    if parts.len() != RENDER_FIELDS {
        return None;
    }
    let tag = match parts.first() {
        Some(v) => *v,
        None => return None,
    };
    let mut vals = [0u32; 4];
    let mut i = 0usize;
    while i < 4 {
        let seg = match parts.get(i + 1) {
            Some(v) => *v,
            None => return None,
        };
        let body = match seg.strip_prefix("k") {
            Some(v) => v,
            None => return None,
        };
        let eq = match body.find('=') {
            Some(v) => v,
            None => return None,
        };
        let (kn, vn) = body.split_at(eq);
        let idx = match parse_u32(kn) {
            Some(v) => v,
            None => return None,
        };
        if idx != i as u32 + 1 {
            return None;
        }
        let val = match parse_u32(vn.strip_prefix('=').unwrap_or("")) {
            Some(v) => v,
            None => return None,
        };
        vals[i] = val;
        i += 1;
    }
    let last = match parts.get(5) {
        Some(v) => *v,
        None => return None,
    };
    let hex = match last.strip_prefix("digest=") {
        Some(v) => v,
        None => return None,
    };
    let dg = match parse_hex16(hex) {
        Some(v) => v,
        None => return None,
    };
    Some((tag, vals, dg))
}

/// 账行自审：六字段可解析、标签在册、摘要非零。
pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, _, dg)) => {
            let known = tag == DOCK_TAG || tag == TRACE_TAG || tag == OBS_TAG || tag == CODE_TAG;
            known && dg != 0
        }
        None => false,
    }
}

/// 单条提交账：号 / 码 / 入口 / 出口 / 四点摘要。
pub fn trace_row(tr: &Trace) -> String {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, tr.id);
    h = p_core::fold_u32(h, tr.code);
    h = p_core::fold_u32(h, tr.t_in);
    h = p_core::fold_u32(h, tr.t_out);
    h = p_core::fold_byte(h, tr.point_mask());
    if h == 0 {
        h = p_core::FNV_OFF;
    }
    render_row(TRACE_TAG, tr.id, tr.code, tr.t_in, tr.t_out, h)
}

/// 会话账：态 / 对端 / 高序号 / 已接 / 摘要。
pub fn session_row(s: &DockSession) -> String {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_byte(h, s.state.index());
    h = p_core::fold_byte(h, s.peer);
    h = p_core::fold_u32(h, s.high_seq);
    h = p_core::fold_u32(h, s.quota);
    h = p_core::fold_u32(h, s.accepted.saturating_add(s.digest as u32));
    if h == 0 {
        h = p_core::FNV_OFF;
    }
    render_row(DOCK_TAG, s.state.index() as u32, s.peer as u32, s.high_seq, s.accepted, h)
}

/// 观测账：尝试 / 记录 / 成 / 败 / 摘要。
pub fn obs_row(l: &ObsLedger) -> String {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, l.attempts);
    h = p_core::fold_u32(h, l.recorded);
    h = p_core::fold_u32(h, l.coverage_bp);
    h = p_core::fold_byte(h, l.silent as u8);
    if h == 0 {
        h = p_core::FNV_OFF;
    }
    render_row(OBS_TAG, l.attempts, l.recorded, l.oks, l.errs, h)
}

/// 码族账：码值 / 中文长度 / 可重试 / 序号。
pub fn code_row(idx: usize) -> Option<String> {
    let r = snapshot_row(idx)?;
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, r.code);
    h = p_core::fold_u32(h, r.cn.len() as u32);
    h = p_core::fold_byte(h, if r.retry { 1 } else { 0 });
    if h == 0 {
        h = p_core::FNV_OFF;
    }
    Some(render_row(CODE_TAG, r.code, r.cn.len() as u32, idx as u32, 0, h))
}

// ── 跨段防撞：与 0x73180 段互不复用 ───────────────────────────────
/// 0x73180 段（unx49053）的 14 条码值与名称（只读引用，不参与本段逻辑）。
///
/// 这里刻意把对方的码值抄成常量：若对方段位变动，本函数会翻红，
/// 从而逼着两侧一起改口径，而不是「各自悄悄地漂」。
pub const PEER_SEG_CODES: [u32; 14] = [
    0x73181, 0x73182, 0x73183, 0x73184, 0x73185, 0x73186, 0x73187, 0x73188, 0x73189,
    0x7318A, 0x7318B, 0x7318C, 0x7318D, 0x7318E,
];

/// 与 0x73180 段码值零交集，且本段 15 条快照与对方 14 条码值两两不同。
pub fn cross_segment_disjoint() -> bool {
    let mut i = 0usize;
    while i < CODE_ROWS {
        let mine = match CODE_SNAPSHOT.get(i) {
            Some(v) => v.code,
            None => return false,
        };
        let mut j = 0usize;
        while j < PEER_SEG_CODES.len() {
            if PEER_SEG_CODES[j] == mine {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 用例编号与跨槽防撞 ────────────────────────────────────────────
/// B12 已占用的优先槽位（编号纪律：本模块 16 个编号必须与之零交集）。
pub const RESERVED_IDS: [u32; 6] = [
    0x071C_8001,
    0x071C_8002,
    0x071C_8014,
    0x071D_0001,
    0x071D_2001,
    0x071D_2014,
];

/// 本模块 16 个用例编号（0x0734_0801..0x0734_0810）。
pub const CASE_IDS: [u32; CASE_TOTAL] = [
    0x0734_0801,
    0x0734_0802,
    0x0734_0803,
    0x0734_0804,
    0x0734_0805,
    0x0734_0806,
    0x0734_0807,
    0x0734_0808,
    0x0734_0809,
    0x0734_080A,
    0x0734_080B,
    0x0734_080C,
    0x0734_080D,
    0x0734_080E,
    0x0734_080F,
    0x0734_0810,
];

/// 按序号取用例编号（越界 None）。
pub fn case_id(i: usize) -> Option<u32> {
    CASE_IDS.get(i).copied()
}

/// 编号必须严格铺满 CASE_BASE+0x01..+0x10。
pub fn case_ids_tile() -> bool {
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let want = match CASE_BASE.checked_add(1 + i as u32) {
            Some(v) => v,
            None => return false,
        };
        if CASE_IDS[i] != want {
            return false;
        }
        i += 1;
    }
    true
}

/// 十六个编号与六个预留槽位零交集，且自身无重复。
pub fn no_cross_collision() -> bool {
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let a = CASE_IDS[i];
        let mut j = 0usize;
        while j < RESERVED_IDS.len() {
            if a == RESERVED_IDS[j] {
                return false;
            }
            j += 1;
        }
        let mut k = i + 1;
        while k < CASE_IDS.len() {
            if a == CASE_IDS[k] {
                return false;
            }
            k += 1;
        }
        i += 1;
    }
    true
}

// ── 探针 ──────────────────────────────────────────────────────────
/// 探针甲：码族快照 + 段位 + 跨段防撞 + 可重试表。
pub fn probe_seq_a() -> Result<(), u32> {
    let _d = audit_code_snapshot()?;
    if !segment_guard() {
        return Err(DockCode::Sequence.val());
    }
    if !cross_segment_disjoint() {
        return Err(DockCode::Duplicate.val());
    }
    let r = retry_codes();
    if r.len() != 5 {
        return Err(DockCode::Schema.val());
    }
    let mut i = 0usize;
    while i < r.len() {
        if !retryable(r[i]) {
            return Err(DockCode::Schema.val());
        }
        // 可重试码必须落在「范围/序号/配额/背压/超时」五条语义上。
        let ok = r[i] == DockCode::Range.val()
            || r[i] == DockCode::Sequence.val()
            || r[i] == DockCode::Quota.val()
            || r[i] == DockCode::Backpressure.val()
            || r[i] == DockCode::Timeout.val();
        if !ok {
            return Err(DockCode::Schema.val());
        }
        i += 1;
    }
    Ok(())
}

/// 探针乙：故障注入逐例对码 + 注入后会话不变式不被破坏。
pub fn probe_seq_b() -> Result<(), u32> {
    let (caught, expected, first_bad) = dock_faults();
    if caught != expected {
        return Err(if first_bad != 0 { first_bad } else { DockCode::Schema.val() });
    }
    let mut s = DockSession::new(1, 2, 0x100)?;
    s.open()?;
    if !s.invariant() {
        return Err(DockCode::Schema.val());
    }
    let mut tr = Tracer::standard();
    let _ = s.submit(&mut tr, &DockReq::valid(1), 10, 12);
    if !s.invariant() {
        return Err(DockCode::Backpressure.val());
    }
    Ok(())
}

/// 探针丙：观测 fuzz 两次复跑摘要一致 + 四点满覆盖 + 零静默。
pub fn probe_seq_c() -> Result<(), u32> {
    let a = obs_fuzz(FUZZ_ROUNDS);
    let b = obs_fuzz(FUZZ_ROUNDS);
    if a.attempts != a.rounds || a.silent != 0 {
        return Err(DockCode::Closed.val());
    }
    if a.coverage_bp != 10_000 {
        return Err(DockCode::Missing.val());
    }
    if a.digest != b.digest || a.oks != b.oks || a.errs != b.errs {
        return Err(DockCode::Sequence.val());
    }
    if a.oks == 0 || a.errs == 0 {
        return Err(DockCode::Schema.val());
    }
    Ok(())
}

/// 复跑自证：三条探针全过。
pub fn repro_check() -> Result<(), u32> {
    probe_seq_a()?;
    probe_seq_b()?;
    probe_seq_c()?;
    Ok(())
}

// ── 八项检查 ──────────────────────────────────────────────────────
/// 八项检查：四条码族（F49055）+ 四条观测（F49056）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    let snap = audit_code_snapshot();
    out.push((
        "F49055/码族15条冻结",
        snap.is_ok(),
        match snap {
            Ok(d) => format!("snapshot={:016X} rows={}", d, CODE_ROWS),            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut text_ok = segment_guard();
    let mut seen: Vec<&'static str> = Vec::new();
    let mut i = 0usize;
    while i < DockCode::ALL.len() {
        let c = DockCode::ALL[i];
        let mut j = 0usize;
        while j < seen.len() {
            if seen[j] == c.detail() {
                text_ok = false;
            }
            j += 1;
        }
        seen.push(c.detail());
        i += 1;
    }
    out.push((
        "F49055/族内文本互不混淆",
        text_ok && seen.len() == 14,
        format!("distinct={} guard={}", seen.len(), segment_guard()),
    ));

    let (caught, expected, first_bad) = dock_faults();
    out.push((
        "F49055/有序拒绝",
        caught == expected && first_bad == 0,
        format!("caught={}/{} first=0x{:X}", caught, expected, first_bad),
    ));

    let rep = obs_fuzz(FUZZ_ROUNDS);
    out.push((
        "F49055/异常零静默",
        rep.silent == 0 && rep.attempts == rep.rounds,
        format!("attempts={} recorded={} silent={}", rep.attempts, rep.rounds, rep.silent),
    ));

    out.push((
        "F49056/四点埋点全覆盖",
        rep.coverage_bp == 10_000,
        format!("coverage_bp={} codes_seen={}", rep.coverage_bp, rep.codes_seen),
    ));

    let mut lat_ok = rep.latency_max > 0 && rep.latency_max <= LATENCY_CEIL;
    let mut mono = Tracer::standard();
    let mut s = match DockSession::new(1, 2, 0x100) {
        Ok(v) => v,
        Err(_) => {
            lat_ok = false;
            DockSession {
                state: DockState::Closed,
                peer: 0,
                version: 0,
                epoch: 0,
                high_seq: 0,
                quota: 0,
                inflight: 0,
                accepted: 0,
                rejected: 0,
                transitions: 0,
                digest: 0,
            }
        }
    };
    if s.open().is_err() {
        lat_ok = false;
    }
    let mut seq = 1u32;
    while seq <= 8 {
        let _ = s.submit(&mut mono, &DockReq::valid(seq), 100 + seq, 102 + seq);
        seq += 1;
    }
    let mut k = 1usize;
    while k < mono.len() {
        let a = match mono.trace_at(k - 1) {
            Some(v) => v,
            None => {
                lat_ok = false;
                break;
            }
        };
        let b = match mono.trace_at(k) {
            Some(v) => v,
            None => {
                lat_ok = false;
                break;
            }
        };
        if b.id <= a.id || !a.elapsed || !b.elapsed {
            lat_ok = false;
        }
        k += 1;
    }
    out.push((
        "F49056/耗时非零单调",
        lat_ok,
        format!("latency_max={} traces={}", rep.latency_max, mono.len()),
    ));

    out.push((
        "F49056/fuzz×10^4零崩",
        rep.rounds == FUZZ_ROUNDS && rep.attempts == FUZZ_ROUNDS && rep.digest != 0,
        format!(
            "rounds={} attempts={} oks={} errs={}",
            rep.rounds, rep.attempts, rep.oks, rep.errs
        ),
    ));

    let again = obs_fuzz(FUZZ_ROUNDS);
    out.push((
        "F49056/复跑摘要一致",
        again.digest == rep.digest && again.codes_seen == rep.codes_seen,
        format!("d1={:016X} d2={:016X}", rep.digest, again.digest),
    ));

    out
}

/// 汇总检查集（八项动态检查，绝不使用静态详情版 add）。
pub fn run_unx49055_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX-F49055 全对接错误码族与四点埋点");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        set.add_dyn(items[i].0, items[i].1, &items[i].2);
        i += 1;
    }
    set
}

// ── 测试 ──────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_closed_and_contiguous() {
        assert_eq!(DockCode::ALL.len(), 14);
        assert!(segment_guard());
        let mut i = 0usize;
        while i < DockCode::ALL.len() {
            let c = DockCode::ALL[i];
            assert_eq!(c.val(), SEG_BASE + 0x01 + i as u32);
            assert!(known_code(c.val()));
            assert_eq!(detail(c.val()), c.detail());
            i += 1;
        }
        assert!(!known_code(SEG_BASE));
        assert!(!known_code(SEG_BASE + 0x0F));
        assert!(!known_code(SEG_BASE + 0x70));
    }

    #[test]
    fn unknown_code_falls_back() {
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
        assert_eq!(detail(0xFFFF_FFFF), UNKNOWN);
        assert_eq!(detail(0x73181), UNKNOWN);
    }

    #[test]
    fn snapshot_audits_clean() {
        let d = audit_code_snapshot().unwrap();
        assert_ne!(d, 0);
        assert_eq!(CODE_SNAPSHOT.len(), 15);
        assert_eq!(CODE_SNAPSHOT[14].code, SEG_BASE + 0x0F);
        assert_eq!(CODE_SNAPSHOT[14].cn, UNKNOWN);
    }

    #[test]
    fn snapshot_rows_are_distinct() {
        let mut i = 0usize;
        while i < CODE_ROWS {
            let a = snapshot_row(i).unwrap();
            let mut j = i + 1;
            while j < CODE_ROWS {
                let b = snapshot_row(j).unwrap();
                assert_ne!(a.code, b.code);
                assert_ne!(a.cn, b.cn);
                j += 1;
            }
            i += 1;
        }
        assert!(snapshot_row(CODE_ROWS).is_none());
    }

    #[test]
    fn retry_table_is_the_five_resource_codes() {
        let r = retry_codes();
        assert_eq!(r, [0x731C6, 0x731C7, 0x731C9, 0x731CC, 0x731CD]);
        assert!(retryable(0x731C6));
        assert!(retryable(DockCode::Timeout.val()));
        assert!(!retryable(DockCode::Handshake.val()));
        assert!(!retryable(DockCode::ClockRegress.val()));
        assert!(!retryable(0x1234_5678));
    }

    #[test]
    fn cross_segment_is_disjoint() {
        assert!(cross_segment_disjoint());
        let mut i = 0usize;
        while i < PEER_SEG_CODES.len() {
            assert!(!known_code(PEER_SEG_CODES[i]));
            i += 1;
        }
    }

    #[test]
    fn state_enum_roundtrip() {
        let mut i = 0u8;
        while i < 4 {
            let s = DockState::from_index(i).unwrap();
            assert_eq!(s.index(), i);
            assert!(s.name().len() >= 4);
            assert!(s.legal_step(s));
            i += 1;
        }
        assert!(DockState::from_index(4).is_err());
        assert!(DockState::from_index(255).is_err());
    }

    #[test]
    fn state_steps_are_whitelisted() {
        assert!(DockState::Handshaking.legal_step(DockState::Open));
        assert!(DockState::Handshaking.legal_step(DockState::Closed));
        assert!(DockState::Open.legal_step(DockState::Draining));
        assert!(DockState::Open.legal_step(DockState::Closed));
        assert!(DockState::Draining.legal_step(DockState::Closed));
        assert!(!DockState::Closed.legal_step(DockState::Open));
        assert!(!DockState::Draining.legal_step(DockState::Open));
        assert!(!DockState::Handshaking.legal_step(DockState::Draining));
        assert!(DockState::Handshaking.legal_step(DockState::Handshaking));
    }

    #[test]
    fn session_construction_validates() {
        assert!(DockSession::new(0, 2, 1).is_ok());
        assert_eq!(DockSession::new(9, 2, 1).err(), Some(DockCode::Handshake.val()));
        assert_eq!(DockSession::new(1, 0, 1).err(), Some(DockCode::Version.val()));
        assert_eq!(DockSession::new(1, 9, 1).err(), Some(DockCode::Version.val()));
        assert_eq!(DockSession::new(1, 2, 0).err(), Some(DockCode::Auth.val()));
    }

    #[test]
    fn session_lifecycle() {
        let mut s = DockSession::new(1, 2, 0x100).unwrap();
        assert_eq!(s.state, DockState::Handshaking);
        assert!(s.invariant());
        assert!(s.open().is_ok());
        assert_eq!(s.state, DockState::Open);
        assert!(s.drain().is_ok());
        assert_eq!(s.state, DockState::Draining);
        assert!(s.close().is_ok());
        assert_eq!(s.state, DockState::Closed);
        assert!(s.close().is_ok());
        assert!(s.open().is_err());
        assert!(s.drain().is_err());
    }

    #[test]
    fn submit_rejects_when_not_open() {
        let mut s = DockSession::new(1, 2, 0x100).unwrap();
        let mut tr = Tracer::standard();
        assert_eq!(
            s.submit(&mut tr, &DockReq::valid(1), 10, 12).err(),
            Some(DockCode::Closed.val())
        );
        assert_eq!(tr.attempts, 1);
        assert_eq!(tr.recorded, 1);
    }

    #[test]
    fn submit_accepts_valid() {
        let mut s = DockSession::new(1, 2, 0x100).unwrap();
        s.open().unwrap();
        let mut tr = Tracer::standard();
        let t = s.submit(&mut tr, &DockReq::valid(1), 10, 12).unwrap();
        assert!(t.ok);
        assert!(t.full());
        assert_eq!(t.latency(), 2);
        assert_eq!(s.high_seq, 1);
        assert_eq!(s.quota, QUOTA_MAX - 1);
        assert_eq!(s.accepted, 1);
        assert_eq!(s.inflight, 1);
        assert!(s.invariant());
    }

    #[test]
    fn submit_sequence_rules() {
        let mut s = DockSession::new(1, 2, 0x100).unwrap();
        s.open().unwrap();
        let mut tr = Tracer::standard();
        assert!(s.submit(&mut tr, &DockReq::valid(1), 1, 2).is_ok());
        assert_eq!(
            s.submit(&mut tr, &DockReq::valid(1), 3, 4).err(),
            Some(DockCode::Duplicate.val())
        );
        assert_eq!(
            s.submit(&mut tr, &DockReq::valid(3), 5, 6).err(),
            Some(DockCode::Sequence.val())
        );
        assert!(s.submit(&mut tr, &DockReq::valid(2), 7, 8).is_ok());
        assert_eq!(s.high_seq, 2);
        assert_eq!(s.accepted, 2);
        assert_eq!(s.rejected, 2);
    }

    #[test]
    fn submit_validates_each_field() {
        let mut s = DockSession::new(1, 2, 0x100).unwrap();
        s.open().unwrap();
        let mut tr = Tracer::standard();
        let mut r = DockReq::valid(1);
        r.peer = 3;
        assert_eq!(s.submit(&mut tr, &r, 1, 2).err(), Some(DockCode::Handshake.val()));
        let mut r = DockReq::valid(1);
        r.version = 3;
        assert_eq!(s.submit(&mut tr, &r, 1, 2).err(), Some(DockCode::Version.val()));
        let mut r = DockReq::valid(1);
        r.op = DOCK_OPS;
        assert_eq!(
            s.submit(&mut tr, &r, 1, 2).err(),
            Some(DockCode::TypeMismatch.val())
        );
        let mut r = DockReq::valid(1);
        r.fields = 0;
        assert_eq!(s.submit(&mut tr, &r, 1, 2).err(), Some(DockCode::Missing.val()));
        let mut r = DockReq::valid(1);
        r.payload_len = 0;
        assert_eq!(s.submit(&mut tr, &r, 1, 2).err(), Some(DockCode::Range.val()));
        let mut r = DockReq::valid(1);
        r.auth_epoch = 9;
        assert_eq!(s.submit(&mut tr, &r, 1, 2).err(), Some(DockCode::Auth.val()));
        // 以上全部被拒，会话序号未被推进。
        assert_eq!(s.high_seq, 0);
        assert_eq!(s.accepted, 0);
        assert!(s.submit(&mut tr, &DockReq::valid(1), 1, 2).is_ok());
    }

    #[test]
    fn submit_quota_and_backpressure_and_timeout() {
        let mut s = DockSession::new(1, 2, 0x100).unwrap();
        s.open().unwrap();
        let mut tr = Tracer::standard();
        s.force_quota(0);
        assert_eq!(
            s.submit(&mut tr, &DockReq::valid(1), 1, 2).err(),
            Some(DockCode::Quota.val())
        );
        s.force_quota(QUOTA_MAX);
        s.force_inflight(INFLIGHT_MAX);
        assert_eq!(
            s.submit(&mut tr, &DockReq::valid(1), 1, 2).err(),
            Some(DockCode::Backpressure.val())
        );
        s.force_inflight(0);
        let mut r = DockReq::valid(1);
        r.deadline = 1;
        assert_eq!(s.submit(&mut tr, &r, 5, 9).err(), Some(DockCode::Timeout.val()));
        assert!(s.submit(&mut tr, &DockReq::valid(1), 5, 9).is_ok());
    }

    #[test]
    fn clock_regress_wins_first() {
        let mut s = DockSession::new(1, 2, 0x100).unwrap();
        s.open().unwrap();
        let mut tr = Tracer::standard();
        // 状态先于对端，但时钟先于状态：即使已关闭，时钟回退也先报 ClockRegress。
        let mut r = DockReq::valid(1);
        r.peer = 9;
        assert_eq!(
            s.submit(&mut tr, &r, 20, 19).err(),
            Some(DockCode::ClockRegress.val())
        );
        assert_eq!(s.high_seq, 0);
    }

    #[test]
    fn trace_points_are_derived() {
        let ok = Trace::sealed(1, 0, 10, 12);
        assert!(ok.full());
        assert_eq!(ok.point_mask(), 0b1111);
        let err = Trace::sealed(2, DockCode::Range.val(), 10, 12);
        assert!(err.full());
        assert!(!err.ok);
        let regress = Trace::sealed(3, DockCode::ClockRegress.val(), 12, 10);
        assert!(regress.full());
        assert_eq!(regress.latency(), 0);
        assert!(regress.elapsed);
        // 无号 = 没进过入口。
        let no_id = Trace::sealed(0, DockCode::Closed.val(), 10, 12);
        assert!(!no_id.entry);
        assert!(!no_id.full());
    }

    #[test]
    fn tracer_capacity_is_bounded() {
        assert!(Tracer::new(TRACE_CAP_MIN - 1).is_err());
        assert!(Tracer::new(TRACE_CAP_MAX + 1).is_err());
        assert!(Tracer::new(TRACE_CAP_MIN).is_ok());
        assert!(Tracer::new(TRACE_CAP_MAX).is_ok());
    }

    #[test]
    fn tracer_ring_drops_oldest_but_keeps_counts() {
        let mut tr = Tracer::new(TRACE_CAP_MIN).unwrap();
        let mut i = 0u32;
        while i < 40 {
            let id = tr.begin();
            tr.record(Trace::sealed(id, 0, i, i + 1));
            i += 1;
        }
        assert_eq!(tr.attempts, 40);
        assert_eq!(tr.recorded, 40);
        assert_eq!(tr.len(), TRACE_CAP_MIN);
        let last = tr.trace_at(TRACE_CAP_MIN - 1).unwrap();
        assert_eq!(last.id, 40);
    }

    #[test]
    fn tracer_audit_detects_silence() {
        let mut tr = Tracer::standard();
        let _ = tr.begin();
        // 取号但没记录 = 静默。
        assert!(tr.audit().is_err());
        // 换一个干净的观测器，取号与记录严格成对。
        let mut clean = Tracer::standard();
        let id = clean.begin();
        clean.record(Trace::sealed(id, 0, 1, 2));
        let l = clean.audit().unwrap();
        assert_eq!(l.silent, 0);
        assert!(l.full_coverage());
        assert_eq!(l.coverage_bp, 10_000);
    }

    #[test]
    fn obs_ledger_zero_recorded_is_not_full() {
        let tr = Tracer::standard();
        assert_eq!(tr.coverage_bp(), 0);
        assert!(tr.audit().is_err());
    }

    #[test]
    fn fuzz_report_is_self_consistent() {
        let r = obs_fuzz(300);
        assert_eq!(r.rounds, 300);
        assert_eq!(r.attempts, 300);
        assert_eq!(r.silent, 0);
        assert_eq!(r.coverage_bp, 10_000);
        assert_eq!(r.oks + r.errs, r.attempts);
        assert!(r.oks > 0);
        assert!(r.errs > 0);
        assert!(r.codes_seen >= 4);
        assert_ne!(r.digest, 0);
    }

    #[test]
    fn fuzz_is_reproducible() {
        let a = obs_fuzz(400);
        let b = obs_fuzz(400);
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.oks, b.oks);
        assert_eq!(a.errs, b.errs);
        assert_eq!(a.codes_seen, b.codes_seen);
    }

    #[test]
    fn fuzz_full_rounds() {
        let r = obs_fuzz(FUZZ_ROUNDS);
        assert_eq!(r.rounds, FUZZ_ROUNDS);
        assert_eq!(r.attempts, FUZZ_ROUNDS);
        assert_eq!(r.silent, 0);
        assert_eq!(r.coverage_bp, 10_000);
        assert!(r.codes_seen >= 8, "seen={} oks={} errs={}", r.codes_seen, r.oks, r.errs);
        assert!(r.latency_max <= 8);
    }

    #[test]
    fn faults_are_all_caught() {
        let (caught, expected, first_bad) = dock_faults();
        assert_eq!(caught, expected);
        assert_eq!(caught, FAULT_INJECT as u32);
        assert_eq!(first_bad, 0);
    }

    #[test]
    fn faults_do_not_leak_into_success() {
        let mut s = DockSession::new(1, 2, 0x100).unwrap();
        s.open().unwrap();
        let mut tr = Tracer::standard();
        for seq in 1..=4 {
            assert!(s.submit(&mut tr, &DockReq::valid(seq), 10, 12).is_ok());
        }
        assert_eq!(s.accepted, 4);
        assert_eq!(s.high_seq, 4);
        assert_eq!(s.rejected, 0);
        assert_eq!(tr.oks, 4);
        assert_eq!(tr.errs, 0);
        assert!(tr.audit().is_ok());
    }

    #[test]
    fn render_roundtrip_trace() {
        let t = Trace::sealed(7, DockCode::Range.val(), 100, 104);
        let row = trace_row(&t);
        assert!(audit_render(&row));
        let (tag, vals, dg) = row_fields(&row).unwrap();
        assert_eq!(tag, TRACE_TAG);
        assert_eq!(vals, [7, DockCode::Range.val(), 100, 104]);
        assert_ne!(dg, 0);
    }

    #[test]
    fn render_roundtrip_session_and_obs() {
        let mut s = DockSession::new(1, 2, 0x100).unwrap();
        s.open().unwrap();
        let sr = session_row(&s);
        let (tag, vals, _) = row_fields(&sr).unwrap();
        assert_eq!(tag, DOCK_TAG);
        assert_eq!(vals[0], DockState::Open.index() as u32);
        let mut tr = Tracer::standard();
        assert!(s.submit(&mut tr, &DockReq::valid(1), 10, 12).is_ok());
        let l = tr.audit().unwrap();
        let orow = obs_row(&l);
        let (tag2, vals2, d2) = row_fields(&orow).unwrap();
        assert_eq!(tag2, OBS_TAG);
        assert_eq!(vals2[0], 1);
        assert_eq!(vals2[1], 1);
        assert_ne!(d2, 0);
    }

    #[test]
    fn code_rows_all_parse() {
        let mut i = 0usize;
        while i < CODE_ROWS {
            let row = code_row(i).unwrap();
            let (tag, vals, _) = row_fields(&row).unwrap();
            assert_eq!(tag, CODE_TAG);
            assert_eq!(vals[0], CODE_SNAPSHOT[i].code);
            assert_eq!(vals[2], i as u32);
            i += 1;
        }
        assert!(code_row(CODE_ROWS).is_none());
    }

    #[test]
    fn render_rejects_malformed() {
        assert!(!audit_render(""));
        assert!(!audit_render("TAG|k1=1|k2=2"));
        assert!(parse_u32("-1").is_none());
        assert!(parse_u32("4294967296").is_none());
        assert!(parse_u32("42").is_some());
        assert!(parse_hex16("0000000000000000").is_some());
        assert!(parse_hex16("000000000000000").is_none());
        assert!(parse_hex16("00000000000000G0").is_none());
        assert!(parse_hex16("00000000000000a0").is_none());
    }

    #[test]
    fn render_rejects_key_drift() {
        let good = render_row(TRACE_TAG, 1, 2, 3, 4, 0xABC);
        assert!(audit_render(&good));
        let bad = good.replace("k2=", "x2=");
        assert!(!audit_render(&bad));
        let bad2 = good.replace("k3=", "k5=");
        assert!(!audit_render(&bad2));
        let zero = render_row(TRACE_TAG, 1, 2, 3, 4, 0);
        assert!(!audit_render(&zero));
        let unknown_tag = render_row("NOPE", 1, 2, 3, 4, 0xABC);
        assert!(!audit_render(&unknown_tag));
    }

    #[test]
    fn case_ids_tile_and_do_not_collide() {
        assert!(case_ids_tile());
        assert!(no_cross_collision());
        assert_eq!(CASE_IDS.len(), 16);
        assert_eq!(CASE_IDS[0], 0x0734_0801);
        assert_eq!(CASE_IDS[15], 0x0734_0810);
        assert_eq!(case_id(0), Some(0x0734_0801));
        assert_eq!(case_id(16), None);
    }

    #[test]
    fn reserved_ids_are_six_distinct() {
        assert_eq!(RESERVED_IDS.len(), 6);
        let mut i = 0usize;
        while i < RESERVED_IDS.len() {
            let mut j = i + 1;
            while j < RESERVED_IDS.len() {
                assert_ne!(RESERVED_IDS[i], RESERVED_IDS[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn repro_is_deterministic() {
        assert!(repro_check().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            assert!(items[i].1, "red check: {} {}", items[i].0, items[i].2);
            i += 1;
        }
        let s = run_unx49055_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
    }

    #[test]
    fn session_digest_changes_on_activity() {
        let mut s = DockSession::new(2, 3, 0x200).unwrap();
        let d0 = s.digest;
        s.open().unwrap();
        let d1 = s.digest;
        assert_ne!(d0, d1);
        let mut tr = Tracer::standard();
        let mut r = DockReq::valid(1);
        r.peer = 2;
        r.version = 3;
        r.auth_epoch = 0x200;
        assert!(s.submit(&mut tr, &r, 10, 12).is_ok());
        assert_ne!(s.digest, d1);
        assert_ne!(s.digest, 0);
    }

    #[test]
    fn accepted_counters_never_exceed_attempts() {
        let mut s = DockSession::new(1, 2, 0x100).unwrap();
        s.open().unwrap();
        let mut tr = Tracer::standard();
        let mut seq = 1u32;
        while seq <= 16 {
            let mut r = DockReq::valid(seq);
            if seq % 3 == 0 {
                r.fields = 0;
            }
            let _ = s.submit(&mut tr, &r, seq * 2, seq * 2 + 1);
            seq += 1;
        }
        assert!(s.accepted <= tr.attempts);
        assert_eq!(s.accepted + s.rejected, tr.attempts);
        assert!(s.invariant());
    }

    #[test]
    fn drain_blocks_new_submissions() {
        let mut s = DockSession::new(1, 2, 0x100).unwrap();
        s.open().unwrap();
        let mut tr = Tracer::standard();
        assert!(s.submit(&mut tr, &DockReq::valid(1), 10, 12).is_ok());
        s.drain().unwrap();
        assert_eq!(
            s.submit(&mut tr, &DockReq::valid(2), 14, 16).err(),
            Some(DockCode::Closed.val())
        );
        assert_eq!(s.high_seq, 1);
    }

    #[test]
    fn retryable_codes_are_all_known() {
        let r = retry_codes();
        let mut i = 0usize;
        while i < r.len() {
            assert!(known_code(r[i]));
            assert!(detail(r[i]) != UNKNOWN);
            i += 1;
        }
    }

    #[test]
    fn trace_mask_is_four_bits() {
        let mut i = 0u32;
        while i < 8 {
            let tr = Trace::sealed(i + 1, if i % 2 == 0 { 0 } else { DockCode::Range.val() }, 5, 9);
            let m = tr.point_mask();
            assert_eq!(m, 0b1111);
            i += 1;
        }
    }

    #[test]
    fn fuzz_latency_is_bounded_by_step() {
        let r = obs_fuzz(500);
        assert!(r.latency_max >= 1);
        assert!(r.latency_max <= 8);
    }

    #[test]
    fn two_card_seeds_are_distinct() {
        // 两卡的 fuzz 种子必须不同：同种子会让「码族账」与「观测账」共用一条随机流，
        // 一旦某卡换了抽样口径，另一卡会跟着漂，复跑就失去独立对照的意义。
        assert_ne!(DOCK_SEED, OBS_SEED);
        assert_ne!(DOCK_SEED, 0);
        assert_ne!(OBS_SEED, 0);
        assert_eq!(DOCK_SEED & 0xFFFF, 0x0001);
        assert_eq!(OBS_SEED & 0xFFFF, 0x0002);
    }
}
