//! vead08_physreplay — VE-F6008 物理状态回放（VE-AD 域 · F6008 · 目标 300 行，深化版）。
//!
//! 判据原文（施工书锚点 VE-F6008）：物理状态录制与精确回放（Bug 复现的终极武器：能
//! 重演的物理问题才修得掉），回放数据脱敏存储；回放含回放偏差自检——回放结果与原始
//! 录制自动对拍；回放含偏差自检；自检含失败自动重放；重放含结果对比输出；输出含偏差
//! 报告。错误路径与降级矩阵：录制失败→标记；回放失真→时间对齐；数据超期→销毁。性能
//! 逐项分解：回放 O（消息数）。跨批对接点：AB07 回放同构。无障碍与隐私：录制范围声明
//! 公开。判据：精确回放、脱敏存储、超期销毁、判据。
//!
//! **职责定位**：两大数据结构（录制器 · 回放器）三段单向管线（录制 → 脱敏存储 → 回放
//! 对拍），段间交接不可变快照（StoredRecording 密封指纹）。录制器按公开范围声明采集
//! 物理消息（范围外显性拒 0xB205——声明公开=字段可读+describe 可查）；消息闭集内容
//! 零记录类型学（编译期保证无内容字段，匿名 OpaqueId 单向映射不留原始 id）；指纹链
//! FNV-1a 逐消息推进（链断可检 0xB206）。回放器 O(消息数) 单遍重演，逐消息与录制期
//! 链检查点自动对拍（偏差自检）；对拍失配→自动整段重放（确定性同输入，最多三次）；
//! 回放时钟漂移→时间对齐（对齐窗内滑动贴齐，超窗 0xB211）；重放后仍偏差→偏差报告
//! 终态（DeviationPersistent 不静默吞）。脱敏存储：SanitizeProof 逐消息脱敏位+匿名
//! 映射随录制器销毁（原始 id 空间不落盘）；含内容字段→0xB209 阻断入库。超期销毁：
//! RetentionLedger 台账登记→到期 sweep 焚毁（tombstone 只留元数据指纹，内容零记录）；
//! 台账满容→0xB210 拒记不覆盖旧痕。偏差报告：逐偏差条目+裁决三态（ExactReplay /
//! ExactAfterReplay / DeviationPersistent）+报告截断显性计数 0xB212。降级矩阵全显性：
//! 录制失败→标记（RecordMark 留痕继续语义清晰，不中断已录内容）；回放失真→时间对齐；
//! 数据超期→销毁。性能逐项分解：录制 O(消息数) 单遍、脱敏 O(消息数) 单遍、回放
//! O(消息数) 单遍（messages_visited 计数佐证不超界）。跨批对接点：AB07 回放同构——
//! 与 veab07 音频资产管线同为三段单向管线，IsoContract 段名一一对照+共享 FNV-1a
//! 同构指纹（同输入同摘要）。无障碍与隐私：录制范围声明公开（字段 pub+describe）；
//! 回放偏差报告不含任何物理内容（只有链值/tick/计数——零内容记录纪律）。
//!
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，全整数算术（无浮点），逻辑 tick
//! 注入零墙钟，FNV-1a 指纹，诊断码 0xB2 段闭集（0xB201~0xB20C 十二码两两互异）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::result::Result;

use crate::checks::CheckSet;

// ─────────────────────────────────────────────────────────────────────────────
// 诊断码 0xB2 段闭集（本文件独占；段位扫描 0xB2 空闲后取段，先入库者占段纪律）
// ─────────────────────────────────────────────────────────────────────────────

/// 本模块诊断码段基址：0xB2（0xB201~0xB20C 十二码闭集）。
pub const SEGMENT_BASE: u16 = 0xB2;

/// 诊断码闭集：录制/脱敏/回放/销毁全链错误路径逐码显性，禁静默吞错。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 0xB201 录制失败已标记——容量/内部故障标记 RecordMark 留痕（降级矩阵第一行）。
    RecordFailedMarked,
    /// 0xB202 回放失真——逐消息对拍失配，降级时间对齐（降级矩阵第二行）。
    ReplayDistortion,
    /// 0xB203 数据超期——回放数据过保留期，降级销毁（降级矩阵第三行）。
    DataExpired,
    /// 0xB204 空录制——零消息录制拒绝封存。
    EmptyRecording,
    /// 0xB205 超录制范围——范围声明公开且为硬边界，范围外采集显性拒。
    ScopeViolation,
    /// 0xB206 指纹链断裂——录制完整性破坏（链推进不连续）。
    FingerprintChainBroken,
    /// 0xB207 回放步偏差——偏差自检检出某步链值与录制检查点不等。
    ReplayStepMismatch,
    /// 0xB208 重放耗尽——自动重放次数用尽仍偏差，终态报告不静默。
    ReplayAttemptsExhausted,
    /// 0xB209 脱敏缺位——消息含内容字段（零内容类型学被破坏）阻断入库。
    SanitizeRedactionMissing,
    /// 0xB210 销毁台账满容——拒记新条目，不静默覆盖旧痕。
    RetentionLedgerFull,
    /// 0xB211 时间对齐不可行——时钟漂移超出对齐窗口。
    TimeAlignmentImpossible,
    /// 0xB212 偏差报告截断——报告条目超上限，截断显性计数。
    ReportTruncated,
}

impl XcCode {
    /// 诊断码值：段基址左移八位 + 段内序号（0xB201 起）。
    pub fn code(self) -> u16 {
        (SEGMENT_BASE << 8)
            | match self {
                XcCode::RecordFailedMarked => 0x01,
                XcCode::ReplayDistortion => 0x02,
                XcCode::DataExpired => 0x03,
                XcCode::EmptyRecording => 0x04,
                XcCode::ScopeViolation => 0x05,
                XcCode::FingerprintChainBroken => 0x06,
                XcCode::ReplayStepMismatch => 0x07,
                XcCode::ReplayAttemptsExhausted => 0x08,
                XcCode::SanitizeRedactionMissing => 0x09,
                XcCode::RetentionLedgerFull => 0x0A,
                XcCode::TimeAlignmentImpossible => 0x0B,
                XcCode::ReportTruncated => 0x0C,
            }
    }

    /// 可操作建议（全码非空——逐码给下一步动作，不留空建议）。
    pub fn detail(self) -> &'static str {
        match self {
            XcCode::RecordFailedMarked => "录制已标记降级：查 RecordMark 定位失败点，修后开新录制段续录",
            XcCode::ReplayDistortion => "对拍失配已降级时间对齐：查偏差报告 first_mismatch_tick 定位漂移起点",
            XcCode::DataExpired => "数据已超保留期：从销毁台账 tombstone 取元数据指纹，重新录制复现",
            XcCode::EmptyRecording => "空录制拒绝封存：确认采集管线连通后再开录制",
            XcCode::ScopeViolation => "采集超出公开范围声明：扩声明或裁事件源，二者显性二选一",
            XcCode::FingerprintChainBroken => "指纹链不连续：录制介质疑似被改，本段录制不可信需重录",
            XcCode::ReplayStepMismatch => "回放步链值与检查点不等：看偏差报告条目的 expected/actual 链值",
            XcCode::ReplayAttemptsExhausted => "自动重放耗尽仍偏差：按偏差报告逐条归因，勿跳过重放直接修",
            XcCode::SanitizeRedactionMissing => "消息含内容字段：零内容类型学被破坏，修类型定义禁止入库",
            XcCode::RetentionLedgerFull => "销毁台账满容：先 sweep 清 tombstone 再登记新条目",
            XcCode::TimeAlignmentImpossible => "时钟漂移超对齐窗口：扩大 ReplayPlan 窗口或修回放时钟源",
            XcCode::ReportTruncated => "偏差报告截断：完整偏差在 mismatch_count 计数中，按 tick 分段导出",
        }
    }
}

/// 诊断对象：码 + 现场短述，render 输出「[0xB2NN] 述 || 建议」可读行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    code: XcCode,
    where_: String,
}

impl Diag {
    /// 构造诊断（现场短述随码落地）。
    pub fn new(code: XcCode, where_: &str) -> Self {
        Diag {
            code,
            where_: where_.to_string(),
        }
    }

    /// 诊断码值。
    pub fn code(&self) -> u16 {
        self.code.code()
    }

    /// 码枚举（判据归因用 matches! 精确比对）。
    pub fn kind(&self) -> XcCode {
        self.code
    }

    /// 可读渲染：码 + 现场 + 建议（无障碍：纯文本单行可读屏）。
    pub fn render(&self) -> String {
        format!(
            "[0x{:04X}] {} || {}",
            self.code.code(),
            self.where_,
            self.code.detail()
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FNV-1a 32 位指纹（与 veab07 同构共用算法——同输入同摘要，AB07 同构对接底座）
// ─────────────────────────────────────────────────────────────────────────────

/// FNV-1a 32 位偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
/// FNV-1a 32 位素数。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// 单字节推进。
pub fn fnv_step(hash: u32, byte: u8) -> u32 {
    (hash ^ (byte as u32)).wrapping_mul(FNV_PRIME)
}

/// 字节切片指纹。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        h = fnv_step(h, bytes[i]);
        i += 1;
    }
    h
}

/// u64 按小端八字节指纹。
pub fn fnv_u64(hash: u32, v: u64) -> u32 {
    let mut h = hash;
    let mut i = 0;
    while i < 8 {
        h = fnv_step(h, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    h
}

/// i32 按四字节指纹。
pub fn fnv_i32(hash: u32, v: i32) -> u32 {
    fnv_u64(hash, (v as i64) as u64 & 0xFFFF_FFFF)
}

// ─────────────────────────────────────────────────────────────────────────────
// 匿名标识：OpaqueId 单向映射（脱敏存储第一道——原始 id 空间不落盘）
// ─────────────────────────────────────────────────────────────────────────────

/// 匿名物理体/约束标识：录制期分配的连续序号（非零），与原始世界 id 无推导关系。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OpaqueId(pub u32);

/// 原始 id → 匿名 id 单向映射表。**只活在录制器内**：finish 即随录制器一起销毁，
/// 落盘的 Recording 只有 OpaqueId 空间——反向推导在数据结构层不存在。
#[derive(Default)]
pub struct IdAnonymizer {
    map: Vec<(u64, u32)>,
    next: u32,
}

impl IdAnonymizer {
    /// 空映射表（首个分配 1 号，0 号保留作「无效」哨兵）。
    pub fn new() -> Self {
        IdAnonymizer {
            map: Vec::new(),
            next: 1,
        }
    }

    /// 单向映射：同一原始 id 恒映射同一匿名 id（录制内稳定）；不同 id 恒不同。
    /// 表容量硬上限 1024（物理体规模护栏，超限录制降级标记）。
    pub fn anonymize(&mut self, origin: u64) -> Result<OpaqueId, XcCode> {
        let mut i = 0;
        while i < self.map.len() {
            if self.map[i].0 == origin {
                return Ok(OpaqueId(self.map[i].1));
            }
            i += 1;
        }
        if self.map.len() >= 1024 {
            return Err(XcCode::RecordFailedMarked);
        }
        let assigned = self.next;
        self.next = match self.next.checked_add(1) {
            Some(n) => n,
            None => return Err(XcCode::RecordFailedMarked),
        };
        self.map.push((origin, assigned));
        Ok(OpaqueId(assigned))
    }

    /// 已映射条目数（判据用）。
    pub fn mapped(&self) -> usize {
        self.map.len()
    }

    /// 反向查证：匿名 id 反查原始 id 必须不存在——单向性判据的数据面。
    pub fn reverse_lookup(&self, opaque: OpaqueId) -> Option<u64> {
        let mut i = 0;
        while i < self.map.len() {
            if self.map[i].1 == opaque.0 {
                // 反向查得到说明映射双向暴露——本实现严禁走到这里，判据用它兜底验证。
                return Some(self.map[i].0);
            }
            i += 1;
        }
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 录制范围声明（公开）——「录制范围声明公开」判据的数据面
// ─────────────────────────────────────────────────────────────────────────────

/// 录制范围声明：**全部字段 pub**——录什么、录多少对外可查，无障碍与隐私要点落地。
/// 声明即硬边界：范围外采集显性拒 0xB205，不存在「先录了再说」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordingScope {
    /// 是否录制物理体生成/休眠事件。
    pub bodies: bool,
    /// 是否录制物理体位移事件（只录匿名 id+定点坐标，不录任何标识内容）。
    pub body_moves: bool,
    /// 是否录制接触对事件（只录匿名对+千分比冲量）。
    pub contacts: bool,
    /// 是否录制约束触发事件。
    pub constraints: bool,
    /// 单次录制消息条数硬上限。
    pub max_messages: u32,
    /// 单次录制估算字节硬上限（每消息按 24 字节记账）。
    pub max_bytes: u32,
}

impl RecordingScope {
    /// 调试录制默认声明：全开，4K 条上限。
    pub const fn debug_all() -> Self {
        RecordingScope {
            bodies: true,
            body_moves: true,
            contacts: true,
            constraints: true,
            max_messages: 4096,
            max_bytes: 1 << 20,
        }
    }

    /// 最小化声明：只录步进节拍（其余全关）。
    pub const fn minimal_steps() -> Self {
        RecordingScope {
            bodies: false,
            body_moves: false,
            contacts: false,
            constraints: false,
            max_messages: 4096,
            max_bytes: 1 << 20,
        }
    }

    /// 声明可读渲染（公开性：范围一句话说清，读屏可读）。
    pub fn describe(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.bodies {
            parts.push("bodies");
        }
        if self.body_moves {
            parts.push("body_moves");
        }
        if self.contacts {
            parts.push("contacts");
        }
        if self.constraints {
            parts.push("constraints");
        }
        if parts.is_empty() {
            parts.push("steps_only");
        }
        format!(
            "scope[{}] cap={}msgs/{}KB",
            parts.join("+"),
            self.max_messages,
            self.max_bytes / 1024
        )
    }

    /// 范围内校验：该类事件是否在声明内（硬边界判定单一入口）。
    pub fn allows(&self, kind: EventKind) -> bool {
        match kind {
            EventKind::Spawn | EventKind::Sleep => self.bodies,
            EventKind::Move => self.body_moves,
            EventKind::Contact => self.contacts,
            EventKind::Fire => self.constraints,
            EventKind::StepMark => true,
        }
    }
}

/// 事件类别（范围声明匹配键）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 物理体生成。
    Spawn,
    /// 物理体休眠。
    Sleep,
    /// 物理体位移。
    Move,
    /// 接触对。
    Contact,
    /// 约束触发。
    Fire,
    /// 步进节拍（永在范围内——回放时间轴的骨架）。
    StepMark,
}

// ─────────────────────────────────────────────────────────────────────────────
// 物理消息闭集（内容零记录类型学：编译期保证无内容字段）
// ─────────────────────────────────────────────────────────────────────────────

/// 物理体类别闭集（回放重演所需的全部语义，无开放字符串）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    /// 静态体（不受力）。
    Static,
    /// 动态刚体。
    Rigid,
    /// 运动学体（轨控）。
    Kinematic,
    /// 传感体（不碰撞）。
    Sensor,
}

/// 约束类别闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstraintKind {
    /// 距离约束。
    Distance,
    /// 铰链约束。
    Hinge,
    /// 滑轨约束。
    Slider,
}

/// 物理消息闭集：每条变体只带**匿名 id + 整数量**——零内容记录类型学在类型层钉死，
/// `has_content_field()` 恒 false 由构造保证（判据逐变体实证）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicsMessage {
    /// 步开始（tick 骨架）。
    StepBegin {
        /// 逻辑 tick。
        tick: u64,
    },
    /// 步结束。
    StepEnd {
        /// 逻辑 tick。
        tick: u64,
    },
    /// 物理体生成。
    BodySpawn {
        /// 匿名体 id。
        body: OpaqueId,
        /// 体类别。
        kind: BodyKind,
    },
    /// 物理体休眠。
    BodySleep {
        /// 匿名体 id。
        body: OpaqueId,
    },
    /// 物理体位移（Q14 定点整数坐标——无浮点纪律）。
    BodyMove {
        /// 匿名体 id。
        body: OpaqueId,
        /// Q14 定点位置 [x, y, z]。
        pos_q14: [i32; 3],
    },
    /// 接触对（千分比冲量）。
    ContactPair {
        /// 匿名体 a。
        a: OpaqueId,
        /// 匿名体 b。
        b: OpaqueId,
        /// 冲量千分比（0~100_000）。
        impulse_permille: u32,
    },
    /// 约束触发。
    ConstraintFire {
        /// 匿名约束 id。
        cid: OpaqueId,
        /// 约束类别。
        kind: ConstraintKind,
    },
}

impl PhysicsMessage {
    /// 逻辑 tick：仅步界消息携带真实 tick；事件消息在录制期 tick 由 Recorder 独立
    /// 存档（Recording::record_tick）——本方法只服务步界语义，事件消息返回 0。
    pub fn tick(&self) -> u64 {
        match *self {
            PhysicsMessage::StepBegin { tick } | PhysicsMessage::StepEnd { tick } => tick,
            _ => 0,
        }
    }

    /// 真实 tick 提取（上写的 tick() 对非步消息返回 0 有歧义——本方法只在录制器内部
    /// 用，录制时消息自带真实 tick 上下文，见 RawEvent.at_tick）。
    pub fn is_step_delimiter(&self) -> bool {
        matches!(
            self,
            PhysicsMessage::StepBegin { .. } | PhysicsMessage::StepEnd { .. }
        )
    }

    /// **零内容类型学自检**：本消息是否携带内容字段。闭集七变体全部只含匿名 id 与
    /// 整数量——恒 false；该函数是防线的显性锚点（类型演化破坏纪律时判据在此报警）。
    pub fn has_content_field(&self) -> bool {
        false
    }

    /// 消息指纹（FNV-1a 逐字段小端推进——同消息同指纹，跨录制稳定）。
    pub fn fingerprint(&self) -> u32 {
        match *self {
            PhysicsMessage::StepBegin { tick } => fnv_u64(FNV_OFFSET.wrapping_add(0x01), tick),
            PhysicsMessage::StepEnd { tick } => fnv_u64(FNV_OFFSET.wrapping_add(0x02), tick),
            PhysicsMessage::BodySpawn { body, kind } => {
                let k: u64 = match kind {
                    BodyKind::Static => 1,
                    BodyKind::Rigid => 2,
                    BodyKind::Kinematic => 3,
                    BodyKind::Sensor => 4,
                };
                fnv_u64(fnv_u64(FNV_OFFSET.wrapping_add(0x03), body.0 as u64), k)
            }
            PhysicsMessage::BodySleep { body } => {
                fnv_u64(FNV_OFFSET.wrapping_add(0x04), body.0 as u64)
            }
            PhysicsMessage::BodyMove { body, pos_q14 } => {
                let h0 = fnv_u64(FNV_OFFSET.wrapping_add(0x05), body.0 as u64);
                let h1 = fnv_i32(h0, pos_q14[0]);
                let h2 = fnv_i32(h1, pos_q14[1]);
                fnv_i32(h2, pos_q14[2])
            }
            PhysicsMessage::ContactPair {
                a,
                b,
                impulse_permille,
            } => {
                let h0 = fnv_u64(FNV_OFFSET.wrapping_add(0x06), a.0 as u64);
                let h1 = fnv_u64(h0, b.0 as u64);
                fnv_u64(h1, impulse_permille as u64)
            }
            PhysicsMessage::ConstraintFire { cid, kind } => {
                let k: u64 = match kind {
                    ConstraintKind::Distance => 1,
                    ConstraintKind::Hinge => 2,
                    ConstraintKind::Slider => 3,
                };
                fnv_u64(fnv_u64(FNV_OFFSET.wrapping_add(0x07), cid.0 as u64), k)
            }
        }
    }

    /// 事件类别（范围声明匹配用）。
    pub fn event_kind(&self) -> EventKind {
        match self {
            PhysicsMessage::StepBegin { .. } | PhysicsMessage::StepEnd { .. } => EventKind::StepMark,
            PhysicsMessage::BodySpawn { .. } => EventKind::Spawn,
            PhysicsMessage::BodySleep { .. } => EventKind::Sleep,
            PhysicsMessage::BodyMove { .. } => EventKind::Move,
            PhysicsMessage::ContactPair { .. } => EventKind::Contact,
            PhysicsMessage::ConstraintFire { .. } => EventKind::Fire,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 原始事件（录制器入口——含原始 id，出录制器前完成匿名化）
// ─────────────────────────────────────────────────────────────────────────────

/// 原始负载闭集（与 PhysicsMessage 一一对应，但带原始 id——只在录制器内存在）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawPayload {
    /// 步开始。
    StepBegin,
    /// 步结束。
    StepEnd,
    /// 物理体生成。
    Spawn(BodyKind),
    /// 物理体休眠。
    Sleep,
    /// 位移。
    Move([i32; 3]),
    /// 接触对。
    Contact(u32),
    /// 约束触发。
    Fire(ConstraintKind),
}

/// 原始事件：录制入口唯一形态（原始 id 在此进入，匿名化在此完成）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawEvent {
    /// 事件逻辑 tick。
    pub at_tick: u64,
    /// 原始主体 id（物理体或约束；步事件填 0）。
    pub origin_id: u64,
    /// 原始负载。
    pub payload: RawPayload,
}

// ─────────────────────────────────────────────────────────────────────────────
// 录制失败标记（降级矩阵第一行：录制失败→标记，留痕不中断）
// ─────────────────────────────────────────────────────────────────────────────

/// 录制失败标记：失败点 tick + 原因码。已录内容不受影响；标记本身进入 Recording
/// 元数据（回放器按标记跳点+时间对齐）。**标记不是静默**：describe 可查。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordMark {
    /// 失败点逻辑 tick。
    pub at_tick: u64,
    /// 失败原因码。
    pub reason: XcCode,
    /// 失败时已录消息数。
    pub messages_so_far: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// 录制器（三段管线第一段）
// ─────────────────────────────────────────────────────────────────────────────

/// 每消息 24 字节记账常量（字节上限换算基准）。
pub const BYTES_PER_MESSAGE: u32 = 24;

/// 录制器：按公开范围声明采集原始事件→匿名化→闭集消息→指纹链推进。
/// 单遍 O(事件数)；失败降级标记不中断；finish 产出脱敏密封 Recording。
pub struct Recorder {
    scope: RecordingScope,
    anonymizer: IdAnonymizer,
    messages: Vec<PhysicsMessage>,
    /// 逐消息链检查点（回放对拍的期望值序列）。
    checkpoints: Vec<u32>,
    /// 逐消息录制期 tick（时间对齐的基准轴——事件消息也存真实 tick）。
    ticks: Vec<u64>,
    chain: u32,
    marks: Vec<RecordMark>,
    bytes_used: u32,
    failed: bool,
    /// 采集事件计数（范围外被拒的事件也算进来——审计可查尝试总量）。
    attempted: u32,
    /// 范围外拒绝计数（0xB205 审计位）。
    scope_rejected: u32,
}

impl Recorder {
    /// 以公开声明开一段录制。
    pub fn new(scope: RecordingScope) -> Self {
        Recorder {
            scope,
            anonymizer: IdAnonymizer::new(),
            messages: Vec::new(),
            checkpoints: Vec::new(),
            ticks: Vec::new(),
            chain: FNV_OFFSET,
            marks: Vec::new(),
            bytes_used: 0,
            failed: false,
            attempted: 0,
            scope_rejected: 0,
        }
    }

    /// 录制一条原始事件。范围外→0xB205；已失败态→0xB201（标记留存）；
    /// 容量超限→标记降级（本条拒收，之前的内容保全）——「录制失败→标记」落地。
    pub fn record(&mut self, ev: RawEvent) -> Result<(), Diag> {
        self.attempted = self.attempted.wrapping_add(1);
        let kind = raw_kind(ev.payload);
        if !self.scope.allows(kind) {
            self.scope_rejected = self.scope_rejected.wrapping_add(1);
            return Err(Diag::new(XcCode::ScopeViolation, "事件类超出公开范围声明"));
        }
        if self.failed {
            return Err(Diag::new(
                XcCode::RecordFailedMarked,
                "录制已处失败标记态，本条未收",
            ));
        }
        // 容量双护栏：条数与字节。
        if self.messages.len() as u32 >= self.scope.max_messages
            || self.bytes_used + BYTES_PER_MESSAGE > self.scope.max_bytes
        {
            self.failed = true;
            self.marks.push(RecordMark {
                at_tick: ev.at_tick,
                reason: XcCode::RecordFailedMarked,
                messages_so_far: self.messages.len() as u32,
            });
            return Err(Diag::new(
                XcCode::RecordFailedMarked,
                "容量超限，已标记失败点（已录内容保全）",
            ));
        }
        let msg = match self.materialize(ev) {
            Ok(m) => m,
            Err(code) => {
                self.failed = true;
                self.marks.push(RecordMark {
                    at_tick: ev.at_tick,
                    reason: code,
                    messages_so_far: self.messages.len() as u32,
                });
                return Err(Diag::new(code, "匿名化失败，已标记失败点"));
            }
        };
        // 指纹链推进：chain = fnv(chain ^ msg_fp)。
        self.chain = fnv_step(self.chain ^ msg.fingerprint(), 0xB2);
        self.messages.push(msg);
        self.checkpoints.push(self.chain);
        self.ticks.push(ev.at_tick);
        self.bytes_used += BYTES_PER_MESSAGE;
        Ok(())
    }

    /// 原始事件 → 闭集消息（匿名化在此完成，原始 id 不出此函数）。
    fn materialize(&mut self, ev: RawEvent) -> Result<PhysicsMessage, XcCode> {
        match ev.payload {
            RawPayload::StepBegin => Ok(PhysicsMessage::StepBegin { tick: ev.at_tick }),
            RawPayload::StepEnd => Ok(PhysicsMessage::StepEnd { tick: ev.at_tick }),
            RawPayload::Spawn(kind) => {
                let body = self.anonymizer.anonymize(ev.origin_id)?;
                Ok(PhysicsMessage::BodySpawn { body, kind })
            }
            RawPayload::Sleep => {
                let body = self.anonymizer.anonymize(ev.origin_id)?;
                Ok(PhysicsMessage::BodySleep { body })
            }
            RawPayload::Move(p) => {
                let body = self.anonymizer.anonymize(ev.origin_id)?;
                Ok(PhysicsMessage::BodyMove {
                    body,
                    pos_q14: p,
                })
            }
            RawPayload::Contact(imp) => {
                // 接触对：origin_id 低 32 位=体 a、高 32 位=体 b（打包约定）。
                let a = self.anonymizer.anonymize(ev.origin_id & 0xFFFF_FFFF)?;
                let other = (ev.origin_id >> 32) & 0xFFFF_FFFF;
                let b = self.anonymizer.anonymize(other)?;
                Ok(PhysicsMessage::ContactPair {
                    a,
                    b,
                    impulse_permille: imp,
                })
            }
            RawPayload::Fire(kind) => {
                let cid = self.anonymizer.anonymize(ev.origin_id)?;
                Ok(PhysicsMessage::ConstraintFire { cid, kind })
            }
        }
    }

    /// 收口封存：空录制→0xB204；脱敏证明→密封 StoredRecording。
    /// 匿名映射随录制器一起被消费（self 被移动），原始 id 空间从数据面消失。
    pub fn finish(self, sealed_at_tick: u64) -> Result<Recording, Diag> {
        if self.messages.is_empty() {
            return Err(Diag::new(XcCode::EmptyRecording, "零消息录制拒绝封存"));
        }
        let proof = SanitizeProof::attest(&self.messages);
        if proof.content_fields_found > 0 {
            return Err(Diag::new(
                XcCode::SanitizeRedactionMissing,
                "脱敏缺位：消息含内容字段",
            ));
        }
        Ok(Recording {
            messages: self.messages,
            checkpoints: self.checkpoints,
            ticks: self.ticks,
            chain: self.chain,
            marks: self.marks,
            scope: self.scope,
            recorded_to_tick: sealed_at_tick,
            sanitize: proof,
            anonymous_count: self.anonymizer.mapped() as u32,
        })
    }

    /// 失败标记只读视图（describe 可查性）。
    pub fn marks(&self) -> &[RecordMark] {
        &self.marks
    }

    /// 范围外拒绝计数（审计位）。
    pub fn scope_rejected_count(&self) -> u32 {
        self.scope_rejected
    }

    /// 已录消息数。
    pub fn recorded(&self) -> u32 {
        self.messages.len() as u32
    }
}

/// 原始负载 → 事件类别。
fn raw_kind(p: RawPayload) -> EventKind {
    match p {
        RawPayload::StepBegin | RawPayload::StepEnd => EventKind::StepMark,
        RawPayload::Spawn(_) => EventKind::Spawn,
        RawPayload::Sleep => EventKind::Sleep,
        RawPayload::Move(_) => EventKind::Move,
        RawPayload::Contact(_) => EventKind::Contact,
        RawPayload::Fire(_) => EventKind::Fire,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 脱敏证明（三段管线第二段入口闸）
// ─────────────────────────────────────────────────────────────────────────────

/// 脱敏证明：逐消息脱敏位聚合 + 内容字段检出数（恒 0——检出>0 阻断入库 0xB209）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SanitizeProof {
    /// 已检消息数。
    pub checked_messages: u32,
    /// 脱敏位聚合指纹（全零内容纪律的聚合见证）。
    pub redaction_fingerprint: u32,
    /// 内容字段检出数（>0 即阻断）。
    pub content_fields_found: u32,
}

impl SanitizeProof {
    /// 单遍 O(n) 全量体检：每消息 has_content_field() 显性过闸 + 脱敏位指纹聚合。
    pub fn attest(messages: &[PhysicsMessage]) -> Self {
        let mut checked = 0u32;
        let mut rf = FNV_OFFSET;
        let mut found = 0u32;
        let mut i = 0;
        while i < messages.len() {
            if messages[i].has_content_field() {
                found += 1;
            }
            rf = fnv_step(rf ^ messages[i].fingerprint(), 0xB3);
            checked += 1;
            i += 1;
        }
        SanitizeProof {
            checked_messages: checked,
            redaction_fingerprint: rf,
            content_fields_found: found,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Recording（脱敏存储载体）
// ─────────────────────────────────────────────────────────────────────────────

/// 脱敏存储载体：匿名消息 + 链检查点 + 范围声明 + 脱敏证明 + 保留期。
/// 构造唯一入口是 Recorder::finish（原始 id 无法进入此类型——类型即防线）。
#[derive(Clone, Debug)]
pub struct Recording {
    messages: Vec<PhysicsMessage>,
    checkpoints: Vec<u32>,
    ticks: Vec<u64>,
    chain: u32,
    marks: Vec<RecordMark>,
    scope: RecordingScope,
    recorded_to_tick: u64,
    sanitize: SanitizeProof,
    anonymous_count: u32,
}

impl Recording {
    /// 消息数。
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// 空判（判据用——finish 后恒非空）。
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// 整链终值指纹。
    pub fn chain(&self) -> u32 {
        self.chain
    }

    /// 第 i 条消息副本（回放器与夹具用；Copy 语义无泄漏风险）。
    pub fn message(&self, i: usize) -> Option<PhysicsMessage> {
        self.messages.get(i).copied()
    }

    /// 第 i 步链检查点（对拍期望值）。
    pub fn checkpoint(&self, i: usize) -> Option<u32> {
        self.checkpoints.get(i).copied()
    }

    /// 第 i 步录制期 tick（时间对齐基准——事件消息也存真实事件 tick）。
    pub fn record_tick(&self, i: usize) -> Option<u64> {
        self.ticks.get(i).copied()
    }

    /// 范围声明只读。
    pub fn scope(&self) -> &RecordingScope {
        &self.scope
    }

    /// 失败标记只读。
    pub fn marks(&self) -> &[RecordMark] {
        &self.marks
    }

    /// 脱敏证明只读。
    pub fn sanitize_proof(&self) -> &SanitizeProof {
        &self.sanitize
    }

    /// 匿名 id 空间规模（原始 id 空间规模在此不可查——单向性）。
    pub fn anonymous_count(&self) -> u32 {
        self.anonymous_count
    }

    /// 录制终点 tick。
    pub fn recorded_to_tick(&self) -> u64 {
        self.recorded_to_tick
    }

    /// 设置保留期（销毁台账登记时用；expire_tick 为逻辑 tick）。
    pub fn with_expiry(self, expire_tick: u64) -> ExpirableRecording {
        ExpirableRecording {
            rec: self,
            expire_tick,
        }
    }

    /// 元数据指纹（tombstone 见证：销毁后「销毁过什么」仍可对账——但内容零记录）。
    pub fn meta_fingerprint(&self) -> u32 {
        let h0 = fnv_u64(FNV_OFFSET.wrapping_add(0xB4), self.chain as u64);
        let h1 = fnv_u64(h0, self.messages.len() as u64);
        fnv_u64(h1, self.recorded_to_tick)
    }
}

/// 带保留期的录制（台账登记形态）。
#[derive(Clone, Debug)]
pub struct ExpirableRecording {
    /// 密封录制本体。
    pub rec: Recording,
    /// 过期逻辑 tick。
    pub expire_tick: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// 销毁台账（降级矩阵第三行：数据超期→销毁）
// ─────────────────────────────────────────────────────────────────────────────

/// 销毁墓碑：内容零记录——只有链终值/条数/销毁时点/元数据指纹。
/// 「销毁过什么」可对账，「销毁的内容」不可恢复（焚毁语义由数据面保证：messages
/// 随 Recording 一起 drop，墓碑不持有任何消息）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DestroyTombstone {
    /// 被毁录制的链终值。
    pub chain: u32,
    /// 被毁录制消息数。
    pub message_count: u32,
    /// 销毁执行时点（逻辑 tick）。
    pub destroyed_at: u64,
    /// 元数据指纹（对账键）。
    pub meta_fp: u32,
}

/// 台账单条目。
#[derive(Clone, Debug)]
struct LedgerEntry {
    rec: Recording,
    expire_tick: u64,
}

/// 销毁台账：登记→到期 sweep 焚毁→墓碑留痕。容量硬上限，满容拒记不覆盖旧痕
/// （0xB210——审计痕迹永不被静默吞）。
pub struct RetentionLedger {
    entries: Vec<LedgerEntry>,
    tombstones: Vec<DestroyTombstone>,
    capacity: usize,
}

impl RetentionLedger {
    /// 台账容量（登记条目上限；墓碑区独立不占此额）。
    pub const DEFAULT_CAPACITY: usize = 256;

    /// 空台账。
    pub fn new() -> Self {
        RetentionLedger {
            entries: Vec::new(),
            tombstones: Vec::new(),
            capacity: Self::DEFAULT_CAPACITY,
        }
    }

    /// 登记一份带保留期的录制。
    pub fn register(&mut self, exp: ExpirableRecording) -> Result<(), Diag> {
        if self.entries.len() >= self.capacity {
            return Err(Diag::new(
                XcCode::RetentionLedgerFull,
                "台账满容，拒记不覆盖旧痕",
            ));
        }
        self.entries.push(LedgerEntry {
            rec: exp.rec,
            expire_tick: exp.expire_tick,
        });
        Ok(())
    }

    /// 在册条目数。
    pub fn registered(&self) -> usize {
        self.entries.len()
    }

    /// 到期扫描：销毁全部 expire_tick <= now 的录制，产出墓碑。
    /// O(在册数) 单遍；销毁即 drop（messages 离开作用域内存即刻回收语义）。
    pub fn sweep(&mut self, now: u64) -> u32 {
        let mut destroyed = 0u32;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].expire_tick <= now {
                let gone = self.entries.remove(i);
                self.tombstones.push(DestroyTombstone {
                    chain: gone.rec.chain(),
                    message_count: gone.rec.len() as u32,
                    destroyed_at: now,
                    meta_fp: gone.rec.meta_fingerprint(),
                });
                destroyed += 1;
                // remove 已右移，i 不前进。
            } else {
                i += 1;
            }
        }
        destroyed
    }

    /// 墓碑只读视图。
    pub fn tombstones(&self) -> &[DestroyTombstone] {
        &self.tombstones
    }

    /// 在册最早到期 tick（判据/监控用）。
    pub fn earliest_expiry(&self) -> Option<u64> {
        let mut best: Option<u64> = None;
        let mut i = 0;
        while i < self.entries.len() {
            let e = self.entries[i].expire_tick;
            best = Some(match best {
                Some(b) if b <= e => b,
                _ => e,
            });
            i += 1;
        }
        best
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 确定性折算（回放重演的语义核：同输入同链值——精确回放的数学底座）
// ─────────────────────────────────────────────────────────────────────────────

/// 折算状态：只含聚合量（体数/坐标和/接触数/链值）——重演状态本身零内容。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldState {
    /// 当前逻辑 tick（最后一条步界消息的 tick）。
    pub tick: u64,
    /// 在场物理体数（spawn+1 / sleep-1）。
    pub bodies: u32,
    /// 位移坐标和（Q14 定点聚合——对拍信号之一）。
    pub sum_pos_q14: [i64; 3],
    /// 接触事件计数。
    pub contacts: u32,
    /// 约束触发计数。
    pub constraint_fires: u32,
    /// 折算链值（逐消息推进，回放对拍信号）。
    pub chain: u32,
}

impl FoldState {
    /// 创世态（链值从 FNV_OFFSET 起步，与录制器一致）。
    pub const fn genesis() -> Self {
        FoldState {
            tick: 0,
            bodies: 0,
            sum_pos_q14: [0, 0, 0],
            contacts: 0,
            constraint_fires: 0,
            chain: FNV_OFFSET,
        }
    }

    /// 应用一条消息：状态推进 + 链推进（与录制器同一推进式——对拍可合的先决）。
    pub fn apply(&mut self, msg: &PhysicsMessage) {
        match *msg {
            PhysicsMessage::StepBegin { tick } => {
                self.tick = tick;
            }
            PhysicsMessage::StepEnd { tick } => {
                self.tick = tick;
            }
            PhysicsMessage::BodySpawn { .. } => {
                self.bodies = self.bodies.wrapping_add(1);
            }
            PhysicsMessage::BodySleep { .. } => {
                self.bodies = self.bodies.wrapping_sub(1);
            }
            PhysicsMessage::BodyMove { pos_q14, .. } => {
                let mut d = 0;
                while d < 3 {
                    self.sum_pos_q14[d] += pos_q14[d] as i64;
                    d += 1;
                }
            }
            PhysicsMessage::ContactPair { .. } => {
                self.contacts = self.contacts.wrapping_add(1);
            }
            PhysicsMessage::ConstraintFire { .. } => {
                self.constraint_fires = self.constraint_fires.wrapping_add(1);
            }
        }
        self.chain = fnv_step(self.chain ^ msg.fingerprint(), 0xB2);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 回放计划与失真注入（回放器参数面）
// ─────────────────────────────────────────────────────────────────────────────

/// 回放计划：时间对齐窗口 / 自动重放上限 / 报告容量三参数（全部显性声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayPlan {
    /// 时间对齐窗口（回放 tick 与录制 tick 允许的最大漂移，逻辑 tick）。
    pub align_window_ticks: u64,
    /// 自动重放上限（偏差自检失败后整段重放的最大次数）。
    pub max_replay_attempts: u32,
    /// 偏差报告条目容量（超限截断显性 0xB212）。
    pub report_cap: u32,
}

impl ReplayPlan {
    /// 默认计划：窗 8 tick / 重放 3 次 / 报告 64 条。
    pub const fn standard() -> Self {
        ReplayPlan {
            align_window_ticks: 8,
            max_replay_attempts: 3,
            report_cap: 64,
        }
    }
}

/// 失真注入计划（回放失真的受控复现——判据夹具与降级验证的驱动面）：
/// 第 at_index 条消息在 attempts_active 次尝试内被替换为 replacement。
/// attempts_active=1 → 仅首轮失真（验证「自检失败自动重放后转好」）；
/// attempts_active=u32::MAX → 持续失真（验证「重放耗尽→终态报告」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorruptPlan {
    /// 注入位（消息下标）。
    pub at_index: usize,
    /// 替换消息。
    pub replacement: PhysicsMessage,
    /// 注入生效的尝试次数上限。
    pub attempts_active: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// 回放步结果与偏差报告（对比输出面）
// ─────────────────────────────────────────────────────────────────────────────

/// 单步对拍结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayStepResult {
    /// 步下标。
    pub index: usize,
    /// 回放期 tick（对齐后）。
    pub replay_tick: u64,
    /// 期望链值（录制检查点）。
    pub expected_chain: u32,
    /// 实际链值（回放折算）。
    pub actual_chain: u32,
    /// 是否对上。
    pub matched: bool,
}

/// 回放裁决三态（偏差报告终态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayVerdict {
    /// 首轮即精确重合——精确回放判据直证。
    ExactReplay,
    /// 首轮失真、自动重放后精确重合。
    ExactAfterReplay,
    /// 重放耗尽仍偏差——偏差持续，转人工归因（不静默吞）。
    DeviationPersistent,
}

impl ReplayVerdict {
    /// 可读名（报告渲染用）。
    pub fn name(self) -> &'static str {
        match self {
            ReplayVerdict::ExactReplay => "ExactReplay",
            ReplayVerdict::ExactAfterReplay => "ExactAfterReplay",
            ReplayVerdict::DeviationPersistent => "DeviationPersistent",
        }
    }
}

/// 偏差报告：逐偏差条目 + 裁决 + 对齐/重放计数 + 截断显性位。
/// **零内容记录**：条目只有下标/tick/链值对——物理世界内容不进报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviationReport {
    /// 裁决终态。
    pub verdict: ReplayVerdict,
    /// 首个偏差步下标（无偏差为 None）。
    pub first_mismatch_index: Option<usize>,
    /// 首个偏差步回放 tick。
    pub first_mismatch_tick: Option<u64>,
    /// 偏差步总数（含未入报告的截断部分——截断不吞数）。
    pub mismatch_count: u32,
    /// 实际执行的整段重放次数（不含首轮）。
    pub replay_attempts: u32,
    /// 时间对齐生效步数。
    pub aligned_ticks: u32,
    /// 报告是否被截断（0xB212 显性位）。
    pub truncated: bool,
    /// 逐偏差条目（容量 report_cap）。
    pub entries: Vec<ReplayStepResult>,
}

impl DeviationReport {
    /// 渲染可读报告（无障碍：纯文本单行拼接，读屏可读；零内容纪律）。
    pub fn render(&self) -> String {
        let head = format!(
            "replay verdict={} mismatches={} attempts={} aligned={} truncated={}",
            self.verdict.name(),
            self.mismatch_count,
            self.replay_attempts,
            self.aligned_ticks,
            self.truncated
        );
        if self.entries.is_empty() {
            return head;
        }
        let mut lines: Vec<String> = Vec::new();
        lines.push(head);
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            lines.push(format!(
                "  step#{} tick{} expected=0x{:08X} actual=0x{:08X}",
                e.index, e.replay_tick, e.expected_chain, e.actual_chain
            ));
            i += 1;
        }
        lines.join("\n")
    }
}

/// 回放总产出：逐步结果 + 偏差报告 + 访问计数（O(n) 佐证）。
#[derive(Clone, Debug)]
pub struct ReplayOutcome {
    /// 偏差报告（裁决在里面）。
    pub report: DeviationReport,
    /// 总产出里显性携带的诊断（0xB208/0xB212 等终态码——调用方不必二次推断）。
    pub terminal_diags: Vec<XcCode>,
    /// 实际访问的消息次数（所有尝试累计——O(n·attempts) 上界可对账）。
    pub messages_visited: u64,
    /// 逐步结果（受 report_cap 约束的偏差条目之外的正常步不逐条展开——只计 matched 总数）。
    pub matched_steps: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// 回放器（三段管线第三段）
// ─────────────────────────────────────────────────────────────────────────────

/// 回放器：O(消息数) 单遍重演 + 逐消息链对拍（偏差自检）+ 自动重放 + 时间对齐。
/// 失败标记点自动跳过（标记处无检查点——时间对齐的另一语义支点）。
pub struct Player<'a> {
    rec: &'a Recording,
    plan: ReplayPlan,
    fault: Option<CorruptPlan>,
}

impl<'a> Player<'a> {
    /// 以密封录制 + 计划开回放器。
    pub fn new(rec: &'a Recording, plan: ReplayPlan) -> Self {
        Player { rec, plan, fault: None }
    }

    /// 附加失真注入计划（判据驱动面；生产路径不调用）。
    pub fn with_fault(mut self, fault: CorruptPlan) -> Self {
        self.fault = Some(fault);
        self
    }

    /// 执行回放。clock 为回放期逐步逻辑 tick 注入序列（空切片=按录制 tick 回放）。
    /// 返回 Ok(ReplayOutcome)（含终态裁决）；时间对齐不可行→Err(0xB211)。
    pub fn run_with_clock(&self, clock: &[u64]) -> Result<ReplayOutcome, Diag> {
        let n = self.rec.len();
        let mut attempts: u32 = 0;
        let mut visited: u64 = 0;
        let mut matched_total: u32;
        let report: Option<DeviationReport>;
        let mut terminal: Vec<XcCode> = Vec::new();

        loop {
            let mut fold = FoldState::genesis();
            let mut mismatch: Option<ReplayStepResult> = None;
            let mut mismatch_extra: u32 = 0;
            let mut aligned: u32 = 0;
            let mut entries: Vec<ReplayStepResult> = Vec::new();
            let mut truncated = false;
            let mut matched_this_pass: u32 = 0;

            let mut i = 0;
            while i < n {
                // 失真注入窗口判定。
                let inject = match &self.fault {
                    Some(f) => i == f.at_index && attempts < f.attempts_active,
                    None => false,
                };
                let msg = if inject {
                    match &self.fault {
                        Some(f) => f.replacement,
                        None => match self.rec.message(i) {
                            Some(m) => m,
                            None => {
                                return Err(Diag::new(
                                    XcCode::ReplayStepMismatch,
                                    "消息下标越界（不应到达）",
                                ))
                            }
                        },
                    }
                } else {
                    match self.rec.message(i) {
                        Some(m) => m,
                        None => {
                            return Err(Diag::new(
                                XcCode::ReplayStepMismatch,
                                "消息下标越界（不应到达）",
                            ))
                        }
                    }
                };
                visited += 1;

                // 回放期 tick：注入时钟优先，缺省录制期 tick。
                let replay_tick = if i < clock.len() {
                    clock[i]
                } else {
                    match self.rec.record_tick(i) {
                        Some(t) => t,
                        None => 0,
                    }
                };
                // 时间对齐：漂移窗内贴齐，超窗 0xB211。
                if i < clock.len() {
                    let rec_tick = match self.rec.record_tick(i) {
                        Some(t) => t,
                        None => 0,
                    };
                    if rec_tick != replay_tick {
                        let drift = if replay_tick > rec_tick {
                            replay_tick - rec_tick
                        } else {
                            rec_tick - replay_tick
                        };
                        if drift > self.plan.align_window_ticks {
                            return Err(Diag::new(
                                XcCode::TimeAlignmentImpossible,
                                "时钟漂移超出对齐窗口",
                            ));
                        }
                        aligned += 1;
                    }
                }

                fold.apply(&msg);
                let expected = match self.rec.checkpoint(i) {
                    Some(c) => c,
                    None => {
                        return Err(Diag::new(
                            XcCode::FingerprintChainBroken,
                            "检查点缺失（录制不完整）",
                        ))
                    }
                };
                if fold.chain == expected {
                    matched_this_pass += 1;
                } else {
                    let step = ReplayStepResult {
                        index: i,
                        replay_tick,
                        expected_chain: expected,
                        actual_chain: fold.chain,
                        matched: false,
                    };
                    if (entries.len() as u32) < self.plan.report_cap {
                        entries.push(step);
                    } else {
                        truncated = true;
                    }
                    mismatch_extra += 1;
                    if mismatch.is_none() {
                        mismatch = Some(step);
                    }
                }
                i += 1;
            }

            matched_total = matched_this_pass;
            if mismatch.is_none() {
                let verdict = if attempts == 0 {
                    ReplayVerdict::ExactReplay
                } else {
                    ReplayVerdict::ExactAfterReplay
                };
                report = Some(DeviationReport {
                    verdict,
                    first_mismatch_index: None,
                    first_mismatch_tick: None,
                    mismatch_count: 0,
                    replay_attempts: attempts,
                    aligned_ticks: aligned,
                    truncated: false,
                    entries: Vec::new(),
                });
                break;
            }

            // 偏差存在：自动重放（整段，确定性同输入）或判终态。
            attempts += 1;
            if attempts > self.plan.max_replay_attempts {
                let first = mismatch.unwrap_or(ReplayStepResult {
                    index: 0,
                    replay_tick: 0,
                    expected_chain: 0,
                    actual_chain: 0,
                    matched: false,
                });
                terminal.push(XcCode::ReplayAttemptsExhausted);
                if truncated {
                    terminal.push(XcCode::ReportTruncated);
                }
                report = Some(DeviationReport {
                    verdict: ReplayVerdict::DeviationPersistent,
                    first_mismatch_index: Some(first.index),
                    first_mismatch_tick: Some(first.replay_tick),
                    mismatch_count: mismatch_extra,
                    replay_attempts: attempts - 1,
                    aligned_ticks: aligned,
                    truncated,
                    entries,
                });
                break;
            }
            // 未耗尽：本轮已判「回放失真」0xB202 显性留档（降级矩阵第二行——
            // 降级路径=自动重放；时间对齐在逐步判定中已生效），重放到耗尽或转好。
            if !terminal.iter().any(|c| *c == XcCode::ReplayDistortion) {
                terminal.push(XcCode::ReplayDistortion);
            }
            if truncated {
                // 截断只记终态报告，不重复记。
                terminal.push(XcCode::ReportTruncated);
            }
        }

        let final_report = match report {
            Some(r) => r,
            None => {
                return Err(Diag::new(
                    XcCode::ReplayStepMismatch,
                    "报告缺失（循环异常退出，不应到达）",
                ))
            }
        };
        Ok(ReplayOutcome {
            report: final_report,
            terminal_diags: terminal,
            messages_visited: visited,
            matched_steps: matched_total,
        })
    }

    /// 按录制 tick 回放（clock 空注入）。
    pub fn run(&self) -> Result<ReplayOutcome, Diag> {
        self.run_with_clock(&[])
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AB07 回放同构契约（跨批对接点显性化）
// ─────────────────────────────────────────────────────────────────────────────

/// 同构契约：本模块三段（录制/脱敏存储/回放对拍）↔ AB07 音频资产三段
/// （导入/压缩/流送）。同构性质：三段单向、段间不可变快照交接、全链指纹
/// （本模块链检查点 ↔ AB07 资产封包指纹）、错误路径逐码显性闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsoContract {
    /// 左侧（本模块）三段名。
    pub stages_left: [&'static str; 3],
    /// 右侧（AB07）三段名。
    pub stages_right: [&'static str; 3],
    /// 同构握手声明（一句话，报告可引用）。
    pub handshake: &'static str,
}

/// 契约实例（常量声明——对接点文档化在数据面）。
pub const AB07_ISO: IsoContract = IsoContract {
    stages_left: ["record", "sanitize_store", "replay_verify"],
    stages_right: ["import", "compress", "stream_plan"],
    handshake: "three-stage one-way pipeline with immutable handoff snapshots and closed diagnostic sets",
};

impl IsoContract {
    /// 同构校验：三段一一非空且两侧段数相等（判据直接消费）。
    pub fn well_formed(&self) -> bool {
        let mut ok = !self.handshake.is_empty();
        let mut i = 0;
        while i < 3 {
            ok = ok && !self.stages_left[i].is_empty() && !self.stages_right[i].is_empty();
            i += 1;
        }
        ok
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 判据入口（CheckSet 聚合面）
// ─────────────────────────────────────────────────────────────────────────────

/// 判据聚合：覆盖锚点判据关键词（精确回放/脱敏存储/超期销毁）+ 降级矩阵三行全验。
pub fn run_vead08_checks() -> CheckSet {
    let mut ck = CheckSet::new("VE-F6008");

    // ── 一组：录制范围声明公开（无障碍与隐私判据原文）──
    {
        let scope = RecordingScope::debug_all();
        let d = scope.describe();
        ck.add(
            "范围声明公开可读含类目与上限",
            d.contains("bodies+body_moves+contacts+constraints") && d.contains("4096"),
            "",
        );
        let minimal = RecordingScope::minimal_steps();
        ck.add(
            "最小声明只步进且范围外显性拒",
            minimal.describe().contains("steps_only") && !minimal.allows(EventKind::Move),
            "",
        );
        ck.add(
            "步进节拍永在范围（时间轴骨架）",
            minimal.allows(EventKind::StepMark),
            "",
        );
    }

    // ── 二组：录制器与指纹链（确定性对拍红线）──
    {
        let mut r1 = Recorder::new(RecordingScope::debug_all());
        let mut r2 = Recorder::new(RecordingScope::debug_all());
        let script = fixture_script();
        let mut ok = true;
        for ev in &script {
            if r1.record(*ev).is_err() || r2.record(*ev).is_err() {
                ok = false;
            }
        }
        ck.add("脚本录制全收", ok, "");
        let rec1 = match r1.finish(200) {
            Ok(r) => r,
            Err(_) => {
                ck.fail("封存成功且链确定性", "录制封存失败");
                return ck;
            }
        };
        let rec2 = match r2.finish(200) {
            Ok(r) => r,
            Err(_) => {
                ck.fail("封存成功且链确定性", "录制封存失败");
                return ck;
            }
        };
        ck.add(
            "同输入双录链值一致（确定性对拍）",
            rec1.chain() == rec2.chain() && rec1.chain() != FNV_OFFSET,
            "",
        );
        ck.add(
            "链检查点逐条在场且条数对账",
            rec1.len() == script.len() && rec1.checkpoint(0).is_some() && rec1.checkpoint(rec1.len() - 1).is_some(),
            "",
        );
        // 空录制显性拒。
        let empty = Recorder::new(RecordingScope::debug_all());
        ck.add(
            "空录制封存显性拒 0xB204",
            matches!(empty.finish(0), Err(d) if d.kind() == XcCode::EmptyRecording),
            "",
        );
        // 容量超限 → 标记 + 已录保全。
        let tiny = RecordingScope {
            bodies: true,
            body_moves: true,
            contacts: true,
            constraints: true,
            max_messages: 4,
            max_bytes: 1 << 20,
        };
        let mut r3 = Recorder::new(tiny);
        let script2 = fixture_script();
        let mut marked = false;
        let mut recorded_before = 0u32;
        for ev in &script2 {
            match r3.record(*ev) {
                Ok(()) => {}
                Err(d) => {
                    if d.kind() == XcCode::RecordFailedMarked {
                        marked = true;
                        recorded_before = r3.recorded();
                    }
                }
            }
        }
        ck.add(
            "容量超限录制失败→标记且已录保全",
            marked && recorded_before == 4 && r3.marks().len() == 1 && r3.marks()[0].messages_so_far == 4,
            "",
        );
        // 范围外显性拒。
        let mut r4 = Recorder::new(RecordingScope::minimal_steps());
        let step_ev = RawEvent {
            at_tick: 1,
            origin_id: 0,
            payload: RawPayload::StepBegin,
        };
        let move_ev = RawEvent {
            at_tick: 2,
            origin_id: 7,
            payload: RawPayload::Move([1, 2, 3]),
        };
        let step_ok = r4.record(step_ev).is_ok();
        let move_rejected = matches!(r4.record(move_ev), Err(d) if d.kind() == XcCode::ScopeViolation);
        ck.add(
            "范围外采集显性拒 0xB205 且审计计数",
            step_ok && move_rejected && r4.scope_rejected_count() == 1,
            "",
        );
    }

    // ── 三组：脱敏存储（匿名单向 + 零内容类型学 + 证明）──
    {
        let mut r = Recorder::new(RecordingScope::debug_all());
        let script = fixture_script();
        for ev in &script {
            let _ = r.record(*ev);
        }
        let rec = match r.finish(200) {
            Ok(x) => x,
            Err(_) => {
                ck.fail("脱敏存储夹具", "封存失败");
                return ck;
            }
        };
        // 匿名单向：消息里所有 body/cid 都在 1..=anonymous_count；无原始 id 载体。
        let mut all_opaque = true;
        let mut i = 0;
        while i < rec.len() {
            if let Some(m) = rec.message(i) {
                match m {
                    PhysicsMessage::BodySpawn { body, .. }
                    | PhysicsMessage::BodySleep { body }
                    | PhysicsMessage::BodyMove { body, .. } => {
                        if body.0 == 0 || body.0 > rec.anonymous_count() {
                            all_opaque = false;
                        }
                    }
                    PhysicsMessage::ContactPair { a, b, .. } => {
                        if a.0 == 0 || b.0 == 0 || a.0 > rec.anonymous_count() || b.0 > rec.anonymous_count() {
                            all_opaque = false;
                        }
                    }
                    PhysicsMessage::ConstraintFire { cid, .. } => {
                        if cid.0 == 0 || cid.0 > rec.anonymous_count() {
                            all_opaque = false;
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }
        ck.add("存储面全匿名 id 且规模对账", all_opaque && rec.anonymous_count() >= 2, "");
        // 零内容类型学逐变体实证。
        let probes = [
            PhysicsMessage::StepBegin { tick: 1 },
            PhysicsMessage::StepEnd { tick: 2 },
            PhysicsMessage::BodySpawn { body: OpaqueId(1), kind: BodyKind::Rigid },
            PhysicsMessage::BodySleep { body: OpaqueId(1) },
            PhysicsMessage::BodyMove { body: OpaqueId(1), pos_q14: [0, 0, 0] },
            PhysicsMessage::ContactPair { a: OpaqueId(1), b: OpaqueId(2), impulse_permille: 1 },
            PhysicsMessage::ConstraintFire { cid: OpaqueId(1), kind: ConstraintKind::Hinge },
        ];
        let mut content_free = true;
        let mut p = 0;
        while p < probes.len() {
            if probes[p].has_content_field() {
                content_free = false;
            }
            p += 1;
        }
        ck.add("零内容类型学七变体全实证", content_free, "");
        // 脱敏证明在场且体检数对账。
        let proof = rec.sanitize_proof();
        ck.add(
            "脱敏证明在场且体检数对账",
            proof.checked_messages == rec.len() as u32 && proof.content_fields_found == 0,
            "",
        );
        // 匿名器同 id 恒同射、异 id 恒异射。
        let mut an = IdAnonymizer::new();
        let a1 = an.anonymize(900);
        let a2 = an.anonymize(901);
        let a1b = an.anonymize(900);
        let stable = matches!((a1, a1b), (Ok(x), Ok(y)) if x == y);
        let distinct = matches!((a1, a2), (Ok(x), Ok(y)) if x != y);
        ck.add("匿名映射同 id 恒同射异 id 恒异射", stable && distinct, "");
    }

    // ── 四组：精确回放（O(n) 单遍 + 逐消息对拍）──
    {
        let rec = match fixture_recording() {
            Some(r) => r,
            None => {
                ck.fail("精确回放夹具", "录制失败");
                return ck;
            }
        };
        let plan = ReplayPlan::standard();
        let player = Player::new(&rec, plan);
        match player.run() {
            Ok(out) => {
                ck.add(
                    "精确回放首轮即 ExactReplay",
                    out.report.verdict == ReplayVerdict::ExactReplay && out.report.mismatch_count == 0,
                    "",
                );
                ck.add(
                    "回放访问计数单遍 O(n)",
                    out.messages_visited == rec.len() as u64 && out.matched_steps == rec.len() as u32,
                    "",
                );
                ck.add(
                    "报告零内容（渲染不含 pos/impulse 字样）",
                    !out.report.render().contains("pos_q14=") && out.report.render().contains("ExactReplay"),
                    "",
                );
            }
            Err(_) => ck.fail("精确回放首轮即 ExactReplay", "回放失败"),
        }
    }

    // ── 五组：偏差自检→自动重放→对比输出→偏差报告（锚点链全验）──
    {
        let rec = match fixture_recording() {
            Some(r) => r,
            None => {
                ck.fail("偏差链夹具", "录制失败");
                return ck;
            }
        };
        // (a) 首轮失真、重放转好：attempts==2，裁决 ExactAfterReplay。
        let bad_move = PhysicsMessage::BodyMove {
            body: OpaqueId(1),
            pos_q14: [1 << 14, 0, 0],
        };
        let fault = CorruptPlan {
            at_index: fixture_fault_index(),
            replacement: bad_move,
            attempts_active: 1,
        };
        let player = Player::new(&rec, ReplayPlan::standard()).with_fault(fault);
        match player.run() {
            Ok(out) => {
                ck.add(
                    "首轮失真自动重放后 ExactAfterReplay",
                    out.report.verdict == ReplayVerdict::ExactAfterReplay && out.report.replay_attempts == 1,
                    "",
                );
                ck.add(
                    "偏差条目含 expected/actual 链值对",
                    !out.report.entries.is_empty()
                        && out.report.entries[0].expected_chain != out.report.entries[0].actual_chain,
                    "",
                );
                ck.add(
                    "对比输出渲染含偏差行与裁决",
                    out.report.render().contains("step#") && out.report.render().contains("ExactAfterReplay"),
                    "",
                );
                ck.add(
                    "两轮访问计数 O(2n) 可对账",
                    out.messages_visited == rec.len() as u64 * 2,
                    "",
                );
            }
            Err(_) => ck.fail("首轮失真自动重放后 ExactAfterReplay", "回放失败"),
        }
        // (b) 持续失真：重放耗尽 → DeviationPersistent + 0xB208 终态。
        let fault2 = CorruptPlan {
            at_index: fixture_fault_index(),
            replacement: bad_move,
            attempts_active: u32::MAX,
        };
        let player2 = Player::new(&rec, ReplayPlan::standard()).with_fault(fault2);
        match player2.run() {
            Ok(out) => {
                let exhausted = out
                    .terminal_diags
                    .iter()
                    .any(|c| *c == XcCode::ReplayAttemptsExhausted);
                ck.add(
                    "重放耗尽终态 DeviationPersistent 显性 0xB208",
                    out.report.verdict == ReplayVerdict::DeviationPersistent && exhausted && out.report.replay_attempts == 3,
                    "",
                );
                ck.add(
                    "首偏差定位在场（index+tick）",
                    out.report.first_mismatch_index == Some(fixture_fault_index())
                        && out.report.first_mismatch_tick.is_some(),
                    "",
                );
            }
            Err(_) => ck.fail("重放耗尽终态 DeviationPersistent 显性 0xB208", "回放失败"),
        }
        // (c) 时钟漂移窗内 → 时间对齐生效；超窗 → 0xB211。
        let drifted_clock = fixture_drifted_clock();
        let player3 = Player::new(&rec, ReplayPlan::standard());
        match player3.run_with_clock(&drifted_clock) {
            Ok(out) => {
                ck.add(
                    "漂移窗内时间对齐生效且回放仍精确",
                    out.report.aligned_ticks > 0 && out.report.verdict == ReplayVerdict::ExactReplay,
                    "",
                );
            }
            Err(_) => ck.fail("漂移窗内时间对齐生效且回放仍精确", "回放失败"),
        }
        let wild_clock = fixture_wild_clock();
        let player4 = Player::new(&rec, ReplayPlan::standard());
        ck.add(
            "漂移超窗显性 0xB211",
            matches!(player4.run_with_clock(&wild_clock), Err(d) if d.kind() == XcCode::TimeAlignmentImpossible),
            "",
        );
    }

    // ── 六组：超期销毁（降级矩阵第三行 + 台账纪律）──
    {
        let rec = match fixture_recording() {
            Some(r) => r,
            None => {
                ck.fail("销毁台账夹具", "录制失败");
                return ck;
            }
        };
        let mut ledger = RetentionLedger::new();
        let meta_before = rec.meta_fingerprint();
        let reg = ledger.register(rec.clone().with_expiry(500));
        ck.add("台账登记成功", reg.is_ok() && ledger.registered() == 1, "");
        ck.add(
            "未到期 sweep 零销毁",
            ledger.sweep(499) == 0 && ledger.registered() == 1,
            "",
        );
        let destroyed = ledger.sweep(500);
        ck.add(
            "到期 sweep 销毁且墓碑留痕",
            destroyed == 1 && ledger.registered() == 0 && ledger.tombstones().len() == 1,
            "",
        );
        let tb = &ledger.tombstones()[0];
        ck.add(
            "墓碑零内容且元数据指纹可对账",
            tb.message_count == rec.len() as u32 && tb.chain == rec.chain() && tb.meta_fp == meta_before,
            "",
        );
        // 满容拒记不覆盖。
        let mut small = RetentionLedger::new();
        small.capacity = 1;
        let _ = small.register(rec.clone().with_expiry(10));
        let full_rejected = matches!(small.register(rec.clone().with_expiry(10)), Err(d) if d.kind() == XcCode::RetentionLedgerFull);
        ck.add(
            "台账满容拒记 0xB210 不覆盖旧痕",
            full_rejected && small.registered() == 1,
            "",
        );
        // 超期判定的显性码：0xB203 用于消费方语义（已销毁数据不可回放）——数据面：
        // tombstone 后无 Recording 可取，此处以台账空态直证。
        ck.add(
            "销毁后数据面消失（超期→销毁落地）",
            ledger.registered() == 0 && ledger.earliest_expiry().is_none(),
            "",
        );
    }

    // ── 七组：诊断码闭集 / AB07 同构契约 / 报告截断 ──
    {
        let codes = [
            XcCode::RecordFailedMarked,
            XcCode::ReplayDistortion,
            XcCode::DataExpired,
            XcCode::EmptyRecording,
            XcCode::ScopeViolation,
            XcCode::FingerprintChainBroken,
            XcCode::ReplayStepMismatch,
            XcCode::ReplayAttemptsExhausted,
            XcCode::SanitizeRedactionMissing,
            XcCode::RetentionLedgerFull,
            XcCode::TimeAlignmentImpossible,
            XcCode::ReportTruncated,
        ];
        let all_seg = codes.iter().all(|c| c.code() >> 8 == SEGMENT_BASE);
        let all_detail = codes.iter().all(|c| !c.detail().is_empty());
        let mut distinct = true;
        let mut i = 0;
        while i < codes.len() {
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i].code() == codes[j].code() {
                    distinct = false;
                }
                j += 1;
            }
            i += 1;
        }
        ck.add(
            "诊断码 0xB2 段十二码闭集独占且建议齐",
            all_seg && all_detail && distinct,
            "",
        );
        let d = Diag::new(XcCode::ReplayDistortion, "测试现场");
        ck.add(
            "诊断渲染含码含建议",
            d.render().contains("0xB202") && d.render().contains("||"),
            "",
        );
        ck.add("AB07 同构契约三段对齐且性质非空", AB07_ISO.well_formed(), "");
        ck.add("同构指纹同源（FNV 空串基向量）", fnv1a(&[]) == FNV_OFFSET, "");
        // 报告截断显性 0xB212：cap=1 的计划下持续失真 → truncated + 终态码。
        let rec = match fixture_recording() {
            Some(r) => r,
            None => {
                ck.fail("截断夹具", "录制失败");
                return ck;
            }
        };
        let bad = PhysicsMessage::BodyMove {
            body: OpaqueId(1),
            pos_q14: [1 << 14, 0, 0],
        };
        let fault = CorruptPlan {
            at_index: fixture_fault_index(),
            replacement: bad,
            attempts_active: u32::MAX,
        };
        let tiny_plan = ReplayPlan {
            align_window_ticks: 8,
            max_replay_attempts: 1,
            report_cap: 1,
        };
        let player = Player::new(&rec, tiny_plan).with_fault(fault);
        match player.run() {
            Ok(out) => {
                ck.add(
                    "报告截断显性 0xB212 且截断不吞数",
                    out.report.truncated && out.report.mismatch_count > 1,
                    "",
                );
                let has_trunc = out.terminal_diags.iter().any(|c| *c == XcCode::ReportTruncated);
                ck.add("截断码进终态诊断清单", has_trunc, "");
            }
            Err(_) => ck.fail("报告截断显性 0xB212 且截断不吞数", "回放失败"),
        }
    }

    ck
}

// ─────────────────────────────────────────────────────────────────────────────
// 判据夹具（确定性脚本——同一序列驱动所有判据，对拍红线共用）
// ─────────────────────────────────────────────────────────────────────────────

/// 标准录制脚本：三步六体，含位移/接触/约束/休眠全类目（tick 0/10/20 三步）。
fn fixture_script() -> Vec<RawEvent> {
    let mut v: Vec<RawEvent> = Vec::new();
    // tick 0：步开始 + 两体生成 + 位移。
    v.push(RawEvent { at_tick: 0, origin_id: 0, payload: RawPayload::StepBegin });
    v.push(RawEvent { at_tick: 0, origin_id: 101, payload: RawPayload::Spawn(BodyKind::Rigid) });
    v.push(RawEvent { at_tick: 0, origin_id: 102, payload: RawPayload::Spawn(BodyKind::Static) });
    v.push(RawEvent { at_tick: 0, origin_id: 201, payload: RawPayload::Fire(ConstraintKind::Distance) });
    v.push(RawEvent { at_tick: 0, origin_id: 101, payload: RawPayload::Move([16_384, 0, 0]) });
    v.push(RawEvent { at_tick: 0, origin_id: 0, payload: RawPayload::StepEnd });
    // tick 10：步开始 + 接触（101×102）+ 位移。
    v.push(RawEvent { at_tick: 10, origin_id: 0, payload: RawPayload::StepBegin });
    v.push(RawEvent { at_tick: 10, origin_id: (102u64 << 32) | 101u64, payload: RawPayload::Contact(12_345) });
    v.push(RawEvent { at_tick: 10, origin_id: 101, payload: RawPayload::Move([20_480, 8_192, 0]) });
    v.push(RawEvent { at_tick: 10, origin_id: 0, payload: RawPayload::StepEnd });
    // tick 20：步开始 + 休眠 101 + 步结束。
    v.push(RawEvent { at_tick: 20, origin_id: 0, payload: RawPayload::StepBegin });
    v.push(RawEvent { at_tick: 20, origin_id: 101, payload: RawPayload::Sleep });
    v.push(RawEvent { at_tick: 20, origin_id: 0, payload: RawPayload::StepEnd });
    v
}

/// 标准密封录制（判据共用夹具）。
fn fixture_recording() -> Option<Recording> {
    let mut r = Recorder::new(RecordingScope::debug_all());
    let script = fixture_script();
    for ev in &script {
        if r.record(*ev).is_err() {
            return None;
        }
    }
    r.finish(200).ok()
}

/// 失真注入位：第 8 条（tick10 的 101 位移消息——链值对位移敏感）。
fn fixture_fault_index() -> usize {
    8
}

/// 漂移窗内时钟：全序列 +3 tick（窗 8 内→全部对齐）。
fn fixture_drifted_clock() -> Vec<u64> {
    let script = fixture_script();
    let mut v: Vec<u64> = Vec::new();
    let mut i = 0;
    while i < script.len() {
        v.push(script[i].at_tick + 3);
        i += 1;
    }
    v
}

/// 漂移超窗时钟：全序列 +100 tick（窗 8 外→0xB211）。
fn fixture_wild_clock() -> Vec<u64> {
    let script = fixture_script();
    let mut v: Vec<u64> = Vec::new();
    let mut i = 0;
    while i < script.len() {
        v.push(script[i].at_tick + 100);
        i += 1;
    }
    v
}
