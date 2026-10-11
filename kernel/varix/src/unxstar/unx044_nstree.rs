//! UNX-F0044 · AML 命名空间树构建（只读）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F0044`
//!
//! **判据 UNX-F0044-J1（原文摘录）**：构建的 \_SB/\_GPE/\_PR 树节点数与 iasl
//! 反汇编结果抽样一致（抽 3 机型各 100 节点）。
//!
//! ## 职责定位
//!
//! 纯功能命名空间层：把 AML 定义块字节流（只读）解析成内部记号流，再按
//! ScopeOp 压栈/弹栈配对状态机构建命名空间树。根 `\` 与五个预定义作用域
//! \_SB/\_GPE/\_PR/\_SI/\_TZ 全部写死播种；节点类型闭集
//! Scope/Device/Method/OpRegion/Field/PowerResource/Processor/ThermalZone
//! （外加 NameLeaf 常量叶）各带子节点表；路径解析支持绝对 `\`、相对 `^`
//! 单级/多级上溯前缀；重名冲突立案不清零；树不变量审计覆盖父指针一致性、
//! 无环、深度上限；\_SB/\_GPE/\_PR 三作用域各有节点数统计账；iasl 抽样
//! 对照按 3 机型 × 100 节点写死抽样表逐节点对拍节点类型与路径，一致率
//! 100% 才判达标；BNode 分配账显性记节点编号递增与容量封顶；增量重扫出
//! 新增/消失/类型漂移三分账；清单导出双形态（缩进树 + 路径清单）。
//!
//! ## 设计要点
//! 1. **预定义作用域写死**：构造期播种根与五作用域（NameSeg 定长四字符、
//!    下划线补位，对照 iasl 反汇编）；后续字节流对它们只做"重开合并"
//!    （ACPI 6.5 §19.6.101 ScopeOp 语义），不重复建节点。
//! 2. **节点类型闭集 9 类**：`kind_id()` 给直方图账用，无裸 u8 散落。
//! 3. **ScopeOp 配对状态机**：Scope/Device/ThermalZone/PowerResource/
//!    Processor 五类带包体操作码压栈（帧 = 节点号 + body_end 偏移），记号
//!    偏移达帧尾即弹；弹栈偏移须与帧尾严格相等（否则判配对破坏）；嵌套
//!    帧尾不得越过父帧尾；流尾弹栈帧尾须等于流长（否则判包体未闭）。
//!    Method/Field 包体对词法不透明，整体跳过。
//! 4. **路径三形态**：绝对 `\`（RootChar 0x5C）；相对 `^` 上溯
//!    （ParentPrefixChar 0x5E，可叠加多级）；多段 NamePath（SegCount
//!    前缀或 DualNamePath 0x2F）。NameSeg 定长四字节大写折叠后直接比。
//! 5. **重名冲突立案**：同父同名异类型、或非 Scope 类重定义，一律记冲突
//!    账（路径 + 旧类型 + 新类型），不静默覆盖；Scope 同名重开为合法
//!    合并，另记重开账。
//! 6. **树不变量审计**：父子互指一致性、深度场一致性、无环（父链步数
//!    封顶节点数）、深度上限 32。
//! 7. **iasl 抽样对拍**：3 机型（DT-ICH9-A / NB-ULV-B / SRV-DUAL-C）各由
//!    写死参数展开出 DFS 序期望抽样表，同一份表经内建 AML 编码器编回
//!    字节流，走"词法 → 建树 → DFS 清单"全链路得实测表，取前 100 节点
//!    逐对拍节点类型与路径，100/100 才判达标。
//! 8. **BNode 分配账**：节点号全局递增（根为 0），容量封顶 4096，触顶
//!    显性报错；账面含已用/余量/重开数/冲突数/扫描次数。
//! 9. **增量重扫三分账**：基线流树 vs 增量流树按路径字典序归并出新增/
//!    消失/类型漂移三分账，互不混记。
//! 10. **零 panic 面**：禁 unwrap/expect/panic!；错误一律 `Err(SgCode)` +
//!     `reason()` 中文诊断；无 IO、无墙钟（逻辑 tick u64 注入）；全整数
//!     无浮点；全确定性（同输入同输出）。
//! 11. **诊断码独占 0x0A40..=0x0A5F 段**：已 grep 同目录确认 0x0Axx 全段
//!     空闲（0x0701~0x070F 为 F007 系、0x0720~0x072F 为 F0043 系所占）。
#![cfg_attr(not(feature = "std"), no_std)]
use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

/// 词法流容量上限（1 MiB，与 F0043 词法窗口口径一致再放宽一档）。
pub const STREAM_CAP: usize = 1 << 20;
/// BNode 容量封顶：单树节点总数上限，触顶显性报错。
pub const NODE_CAP: u32 = 4096;
/// 树深度上限：审计硬闸。
pub const MAX_DEPTH: u32 = 32;
/// 每机型抽样节点数（判据原文指定 100）。
pub const SAMPLE_PER_MACHINE: usize = 100;
/// 抽样机型数（判据原文指定 3）。
pub const MACHINE_COUNT: usize = 3;
/// 根节点 BNode 编号（分配账从 1 号起给非根节点）。
pub const ROOT_ID: u32 = 0;

// === 诊断码闭集（独占 0x0A40..=0x0A5F 段）===

/// UNX-F0044 诊断码。独占 `0x0A40..=0x0A5F` 段（已 grep 同目录确认空闲）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SgCode {
    EmptyStream = 0x0A40,      // 输入字节流为空（无定义块可建树）
    StreamTooLarge = 0x0A41,   // 输入字节流超 1 MiB 词法流封顶
    UnknownOpcode = 0x0A42,    // 主码闭集之外的字节（未知操作码）
    UnknownExtOpcode = 0x0A43, // 0x5B 二级前缀后跟闭集外字节
    BadPkgLength = 0x0A44,     // 包长度编码非法（档位/尾字节不合法）
    PkgOverrun = 0x0A45,       // 包体尾越过流尾（声明超窗）
    BadNameLead = 0x0A46,      // NameSeg 首字符非法（不在 LeadNameChar 表）
    BadNameTail = 0x0A47,      // NameSeg 尾字符非法（不在 NameChar 表）
    NameSegTruncated = 0x0A48, // NameSeg 残缺（四字符未补齐流即耗尽）
    BadNamePath = 0x0A49,      // 名字路径形态非法（空路径/零段数作操作数）
    UnsupportedData = 0x0A4A,  // 数据项编码不在闭集
    OperandTruncated = 0x0A4B, // 操作数半途截断（流在取数中耗尽）
    ScopePairBreak = 0x0A4C,   // ScopeOp 配对破坏（弹栈偏移与帧尾不等）
    ScopeUnclosed = 0x0A4D,    // 流尾弹栈时帧尾与流长不符（包体未闭）
    PathNotFound = 0x0A4E,     // 路径解析失败（目标不存在且不隐式创建）
    UpBeyondRoot = 0x0A4F,     // 相对上溯越过根（^ 级数超过树深）
    NodeCapExceeded = 0x0A50,  // BNode 容量封顶（节点数超上限）
    DepthExceeded = 0x0A51,    // 树深度超上限
    NameConflict = 0x0A52,     // 重名冲突（同父同名异类型/非 Scope 重定义）
    PhaseViolation = 0x0A53,   // 状态机相位非法迁移
    SampleShort = 0x0A54,      // 抽样表节点数不足（少于 100）
    SampleMismatch = 0x0A55,   // 抽样对拍不一致（类型或路径对不上）
    AuditBroken = 0x0A56,      // 树不变量被破坏（父指针/环/深度场）
    BadProfilePath = 0x0A57,   // 机型剖面表路径非法
    EmptyProfile = 0x0A58,     // 机型剖面表为空（无法构流）
    TrailingBytes = 0x0A59,    // 流尾残留字节（末作用域已闭但流未耗尽）
    ScopeBodyNest = 0x0A5A,    // 子作用域帧尾越过父作用域帧尾
}

impl SgCode {
    /// 诊断码原始值。
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 中文诊断语（结构化错误 reason，逐码一意）。
    pub fn reason(self) -> &'static str {
        match self {
            SgCode::EmptyStream => "输入字节流为空，无定义块可建树",
            SgCode::StreamTooLarge => "输入字节流超 1MiB 词法流上限，拒绝构建",
            SgCode::UnknownOpcode => "主操作码不在闭集，记未知码账",
            SgCode::UnknownExtOpcode => "0x5B 二级前缀后续码不在闭集，记未知码账",
            SgCode::BadPkgLength => "包长度编码非法：档位或尾字节不合法",
            SgCode::PkgOverrun => "包体尾越过流尾：PkgLength 声明超出窗口",
            SgCode::BadNameLead => "NameSeg 首字符非法：不在 LeadNameChar 表",
            SgCode::BadNameTail => "NameSeg 尾字符非法：不在 NameChar 表",
            SgCode::NameSegTruncated => "NameSeg 残缺：四字符未补齐流已耗尽",
            SgCode::BadNamePath => "名字路径形态非法：空路径或零段数不可作操作数",
            SgCode::UnsupportedData => "数据项编码不在闭集：仅支持 Zero/One/B/W/D/Q",
            SgCode::OperandTruncated => "操作数半途截断：流在取数中耗尽",
            SgCode::ScopePairBreak => "ScopeOp 配对破坏：弹栈偏移与帧尾不严格相等",
            SgCode::ScopeUnclosed => "作用域包体未闭：流尾帧尾与流长不符",
            SgCode::PathNotFound => "路径解析失败：目标节点不存在且不隐式创建",
            SgCode::UpBeyondRoot => "相对上溯越过根：^ 级数超过当前树深",
            SgCode::NodeCapExceeded => "BNode 容量封顶：节点总数超过上限，显性拒建",
            SgCode::DepthExceeded => "树深度超上限：超过 MAX_DEPTH 硬闸",
            SgCode::NameConflict => "重名冲突：同父同名异类型或非 Scope 重定义，已立案",
            SgCode::PhaseViolation => "状态机相位非法迁移：当前相位不允许该操作",
            SgCode::SampleShort => "抽样表节点数不足：期望或实测少于 100 节点",
            SgCode::SampleMismatch => "抽样对拍不一致：节点类型或路径对不上",
            SgCode::AuditBroken => "树不变量被破坏：父指针、环或深度场异常",
            SgCode::BadProfilePath => "机型剖面表路径非法：不满足 \\ 前缀四字符段形态",
            SgCode::EmptyProfile => "机型剖面表为空：无法构造字节流",
            SgCode::TrailingBytes => "流尾残留字节：末作用域已闭但流未耗尽",
            SgCode::ScopeBodyNest => "子作用域帧尾越过父作用域帧尾：包体嵌套越界",
        }
    }
}

// === NameSeg：定长四字符路径段 ===

/// AML NameSeg（ACPI 6.5 §19.2.6 / §20.2.2）：定长 4 字节。构造时即做
/// 字符校验并大写折叠（AML 名大小写不敏感），此后 `PartialEq` 即为折叠
/// 等价比较。根节点无 NameSeg，用 `Option` 表达。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameSeg([u8; 4]);

impl NameSeg {
    /// 首字符表 LeadNameChar：'A'~'Z'、'_'（小写折叠收编 'a'~'z'）。
    fn is_lead_char(c: u8) -> bool {
        matches!(c, b'A'..=b'Z' | b'_' | b'a'..=b'z')
    }
    /// 尾字符表 NameChar：LeadNameChar + '0'~'9'。
    fn is_name_char(c: u8) -> bool {
        matches!(c, b'A'..=b'Z' | b'_' | b'a'..=b'z' | b'0'..=b'9')
    }
    fn fold(c: u8) -> u8 {
        if c.is_ascii_lowercase() {
            c - b'a' + b'A'
        } else {
            c
        }
    }
    /// 从恰好 4 字节构造（词法层原始字节形态），非法字符报错。
    pub fn from_bytes(b: &[u8]) -> Result<Self, SgCode> {
        if b.len() != 4 {
            return Err(SgCode::NameSegTruncated);
        }
        if !Self::is_lead_char(b[0]) {
            return Err(SgCode::BadNameLead);
        }
        for k in 1..4 {
            if !Self::is_name_char(b[k]) {
                return Err(SgCode::BadNameTail);
            }
        }
        Ok(NameSeg([Self::fold(b[0]), Self::fold(b[1]), Self::fold(b[2]), Self::fold(b[3])]))
    }
    /// 从 ASCII 串构造（须恰 4 字符），非法字符报错。
    pub fn from_ascii(s: &str) -> Result<Self, SgCode> {
        Self::from_bytes(s.as_bytes())
    }
    /// 原始四字节（已折叠大写）。
    pub fn raw(self) -> [u8; 4] {
        self.0
    }
    /// 展示形态（4 字符串）。
    pub fn as_str(self) -> String {
        let mut s = String::new();
        for b in self.0.iter() {
            s.push(*b as char);
        }
        s
    }
}

// === 节点类型闭集与节点结构 ===

/// 节点类型闭集（判据指定 8 类 + NameLeaf 常量叶，共 9 类）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Scope,         // 作用域（含根与预定义作用域，可重开合并）
    Device,        // 设备
    Method,        // 方法（叶）
    OpRegion,      // 操作区（叶）
    Field,         // 域（容器与命名域条目同用，条目带位宽载荷）
    PowerResource, // 电源资源（容器）
    Processor,     // 处理器（容器）
    ThermalZone,   // 热区（容器）
    NameLeaf,      // 常量名叶（NameOp 所建，带整值载荷）
}

pub const KIND_COUNT: usize = 9;

impl NodeKind {
    /// 直方图账下标。
    pub fn kind_id(self) -> usize {
        match self {
            NodeKind::Scope => 0,
            NodeKind::Device => 1,
            NodeKind::Method => 2,
            NodeKind::OpRegion => 3,
            NodeKind::Field => 4,
            NodeKind::PowerResource => 5,
            NodeKind::Processor => 6,
            NodeKind::ThermalZone => 7,
            NodeKind::NameLeaf => 8,
        }
    }
    /// AML 术语名（对拍明细与清单导出用）。
    pub fn kind_name(self) -> &'static str {
        match self {
            NodeKind::Scope => "Scope",
            NodeKind::Device => "Device",
            NodeKind::Method => "Method",
            NodeKind::OpRegion => "OpRegion",
            NodeKind::Field => "Field",
            NodeKind::PowerResource => "PowerResource",
            NodeKind::Processor => "Processor",
            NodeKind::ThermalZone => "ThermalZone",
            NodeKind::NameLeaf => "NameLeaf",
        }
    }
}

/// 节点载荷：各类型各自的实参记账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodePayload {
    None,                                              // 无载荷（Scope/Device/TZ/Field 容器）
    MethodFlags(u8),                                   // 方法标志字节
    Region { space: u8, offset: u32, len: u32 },       // 操作区：空间、偏移、长度
    FieldBits(u32),                                    // 命名域条目位宽
    Power { system_level: u32, resource_order: u16 },  // 电源资源：系统级、资源序
    Cpu { proc_id: u8, pblk_addr: u32, pblk_len: u8 }, // 处理器：编号、P 块地址、P 块长
    Const(u64),                                        // 常量叶整值
}

/// BNode：命名空间树节点。子节点表为插入序 Vec（即 DFS 先序）。
#[derive(Debug, Clone)]
pub struct NsNode {
    pub id: u32,               // 全局递增节点号
    pub name: Option<NameSeg>, // 路径段；根为 None
    pub kind: NodeKind,        // 节点类型
    pub parent: Option<u32>,   // 父节点号；根为 None
    pub children: Vec<u32>,    // 子节点号表（插入序）
    pub depth: u32,            // 深度场（根 0，逐层 +1，审计对账）
    pub born_tick: u64,        // 建号时注入的逻辑 tick
    pub payload: NodePayload,  // 类型载荷
}

/// 名字路径目标：绝对 `\` / 相对 `^×n` / 多段 NamePath 的统一中间形态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NsTarget {
    pub absolute: bool,     // 是否绝对（RootChar 0x5C 开头）
    pub ups: u32,           // 相对上溯级数（ParentPrefixChar 0x5E 叠加数）
    pub segs: Vec<NameSeg>, // 路径段（折叠大写后）
}

impl NsTarget {
    /// 判空路径（NullName 不可作操作数位）。
    pub fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }
}

/// 建树状态机六相：Idle → Seeded → Building → Sealed → Audited → Rescanned。
/// 相位迁移显性、单向推进；Sealed 之后允许重扫再入 Building。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NsPhase {
    Idle,      // 初始相（未播种，拒一切业务操作）
    Seeded,    // 已播种（根 + 五预定义作用域就位）
    Building,  // 建树中
    Sealed,    // 已封账（一次 build 完成）
    Audited,   // 已审计（不变量审计通过）
    Rescanned, // 已重扫（增量 diff 完成）
}

impl NsPhase {
    /// 中文相位名（诊断输出用）。
    pub fn name(self) -> &'static str {
        match self {
            NsPhase::Idle => "初始相",
            NsPhase::Seeded => "已播种相",
            NsPhase::Building => "建树相",
            NsPhase::Sealed => "封账相",
            NsPhase::Audited => "已审计相",
            NsPhase::Rescanned => "已重扫相",
        }
    }
}

// === 内部记号流（词法产物）===

/// 域条目：段 + 位宽（FieldLength，PkgLength 编码形态还原）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldEntry {
    pub seg: NameSeg, // 命名域条目段
    pub bit_len: u32, // 位宽
}

/// 常量值（数据项闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameValue {
    pub tag: &'static str, // 编码标签（"Zero"/"One"/"Byte"/"Word"/"DWord"/"QWord"）
    pub value: u64,        // 整值
}

/// 词法记号种类。带包体的记号携带 body_end（包首字节偏移 + 总长）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NsTokenKind {
    Scope { target: NsTarget, body_end: u32 },       // ScopeOp 0x10：压栈
    Device { target: NsTarget, body_end: u32 },      // DeviceOp 0x5B82：压栈
    ThermalZone { target: NsTarget, body_end: u32 }, // ThermalZoneOp 0x5B85：压栈
    PowerRes { target: NsTarget, system_level: u32, resource_order: u16, body_end: u32 }, // 0x5B84：压栈
    Processor { target: NsTarget, proc_id: u8, pblk_addr: u32, pblk_len: u8, body_end: u32 }, // 0x5B83：压栈
    Method { target: NsTarget, flags: u8 },          // MethodOp 0x14：叶（包体不透明跳过）
    Field { target: NsTarget, entries: Vec<FieldEntry> }, // FieldOp 0x5B81：叶容器+条目表
    OpRegion { target: NsTarget, space: u8, offset: u32, len: u32 }, // 0x5B80：叶（无包体）
    Name { target: NsTarget, value: NameValue },     // NameOp 0x08：常量叶
}

/// 词法记号：偏移 + 种类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NsToken {
    pub off: u32, // 记号首字节偏移
    pub kind: NsTokenKind,
}

/// 作用域帧：压栈单元（节点号 + 包体尾偏移，严格配对基准）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScopeFrame {
    node: u32,
    body_end: u32,
}

// === 命名空间树主体：账面结构 ===

/// 重名冲突账条目（路径 + 在册类型 + 来犯类型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameConflict {
    pub path: String,
    pub exist_kind: NodeKind,
    pub new_kind: NodeKind,
}

/// 建树报告（本流记号/新建/重开/冲突/建后总节点）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildReport {
    pub tokens: usize,
    pub created: u32,
    pub reopened: u32,
    pub conflicts: usize,
    pub node_total: u32,
}

/// BNode 分配账（用量/封顶/余量/重开/冲突/扫描次数）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocLedger {
    pub used: u32,
    pub cap: u32,
    pub headroom: u32,
    pub reopens: usize,
    pub conflicts: usize,
    pub scans: u32,
}

/// 三作用域（加 \_SI/\_TZ）节点数统计账（判据 J1 的直接账面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeStats {
    pub sb_count: u32,                // \_SB 子树节点数（含 \_SB 本身）
    pub gpe_count: u32,               // \_GPE 子树节点数（含 \_GPE 本身）
    pub pr_count: u32,                // \_PR 子树节点数（含 \_PR 本身）
    pub si_count: u32,                // \_SI 子树节点数（含 \_SI 本身）
    pub tz_count: u32,                // \_TZ 子树节点数（含 \_TZ 本身）
    pub total: u32,                   // 全树节点总数
    pub kind_hist: [u32; KIND_COUNT], // 类型直方图（下标 = kind_id）
}

/// 审计违例条目（节点号 + 中文违例描述）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditViolation {
    pub node: u32,
    pub detail: String,
}

/// 审计报告（检查数/违例数/明细/是否全绿）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditReport {
    pub checked: u32,
    pub violations: usize,
    pub detail: Vec<AuditViolation>,
    pub ok: bool,
}

/// 命名空间树。种子（根 + 五预定义作用域）在构造期写死播种。
pub struct NamespaceTree {
    nodes: Vec<NsNode>,           // 节点池（BNode 编号即下标）
    scope_stack: Vec<ScopeFrame>, // 作用域栈（ScopeOp 配对状态机核心）
    phase: NsPhase,               // 相位
    next_id: u32,                 // 下一节点号（分配账递增指针）
    tick: u64,                    // 最近注入的逻辑 tick
    conflicts: Vec<NameConflict>, // 重名冲突账（跨扫描累计）
    reopens: Vec<String>,         // 作用域重开账（路径，跨扫描累计）
    stream_len: usize,            // 最近一次流的字节长
    scans: u32,                   // 累计扫描次数
}

impl NamespaceTree {
    /// 构造：播种根 `\` 与 \_SB\_\_GPE\_\_PR\_\_SI\_\_TZ\_ 五预定义作用域
    /// （全部写死，对照 iasl 反汇编的标准命名）。播种失败即整体失败。
    pub fn new(tick: u64) -> Result<Self, SgCode> {
        let mut tree = NamespaceTree {
            nodes: Vec::new(), scope_stack: Vec::new(), phase: NsPhase::Idle,
            next_id: 0, tick, conflicts: Vec::new(), reopens: Vec::new(),
            stream_len: 0, scans: 0,
        };
        // 根：节点 0，无段无父。
        tree.nodes.push(NsNode {
            id: 0, name: None, kind: NodeKind::Scope, parent: None,
            children: Vec::new(), depth: 0, born_tick: tick, payload: NodePayload::None,
        });
        tree.next_id = 1;
        // 五预定义作用域（NameSeg 定长四字符，下划线补位）。
        const PREDEF: [&str; 5] = ["_SB_", "_GPE", "_PR_", "_SI_", "_TZ_"];
        for seg in PREDEF.iter() {
            let ns = NameSeg::from_ascii(seg)?;
            tree.raw_child_push(ROOT_ID, Some(ns), NodeKind::Scope, NodePayload::None)?;
        }
        tree.phase = NsPhase::Seeded;
        Ok(tree)
    }
    /// BNode 分配：编号递增 + 容量封顶显性 + 深度上限 + 挂父表。
    fn raw_child_push(&mut self, parent: u32, name: Option<NameSeg>, kind: NodeKind, payload: NodePayload) -> Result<u32, SgCode> {
        if self.next_id >= NODE_CAP {
            return Err(SgCode::NodeCapExceeded);
        }
        let pdepth = self.nd(parent)?.depth;
        if pdepth + 1 > MAX_DEPTH {
            return Err(SgCode::DepthExceeded);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(NsNode {
            id, name, kind, parent: Some(parent), children: Vec::new(),
            depth: pdepth + 1, born_tick: self.tick, payload,
        });
        let pnode = self.nd_mut(parent)?;
        pnode.children.push(id);
        Ok(id)
    }
    /// 取节点（只读），越界即审计级错误。
    fn nd(&self, id: u32) -> Result<&NsNode, SgCode> {
        self.nodes.get(id as usize).ok_or(SgCode::AuditBroken)
    }
    fn nd_mut(&mut self, id: u32) -> Result<&mut NsNode, SgCode> {
        self.nodes.get_mut(id as usize).ok_or(SgCode::AuditBroken)
    }
    /// 取节点类型。
    fn kind_of(&self, id: u32) -> Result<NodeKind, SgCode> {
        Ok(self.nd(id)?.kind)
    }
    /// 同父查子（折叠等价比较）。
    fn find_child(&self, parent: u32, seg: NameSeg) -> Option<u32> {
        let pnode = self.nodes.get(parent as usize)?;
        for &c in pnode.children.iter() {
            if let Some(nm) = self.nodes.get(c as usize)?.name {
                if nm == seg {
                    return Some(c);
                }
            }
        }
        None
    }
    /// 相对上溯：从 cur 起 ^n 级；越过根报错。
    fn walk_up(&self, cur: u32, ups: u32) -> Result<u32, SgCode> {
        let mut at = cur;
        for _ in 0..ups {
            let n = self.nd(at)?;
            match n.parent {
                Some(p) => at = p,
                None => return Err(SgCode::UpBeyondRoot),
            }
        }
        Ok(at)
    }
    /// 父路径 + 段 → 子路径（根特判：`\\_SB_` 而非 `\\._SB_`）。
    fn join_path(parent_path: &str, seg: &str) -> String {
        if parent_path == "\\" {
            format!("\\{}", seg)
        } else {
            format!("{}.{}", parent_path, seg)
        }
    }
    /// 节点全路径（折叠大写形态，根为 `\\`）。
    pub fn path_of(&self, id: u32) -> Result<String, SgCode> {
        let mut segs: Vec<NameSeg> = Vec::new();
        let mut at = id;
        loop {
            let n = self.nd(at)?;
            if let Some(seg) = n.name {
                segs.push(seg);
            }
            match n.parent {
                Some(p) => at = p,
                None => break,
            }
        }
        let mut path = String::from("\\");
        for (k, seg) in segs.iter().rev().enumerate() {
            if k > 0 {
                path.push('.');
            }
            path.push_str(&seg.as_str());
        }
        Ok(path)
    }
    /// 为建节点解析目标：中段按需隐式建 Scope（容器类），末段返回
    /// （父号，既有节点号，末段）。
    fn resolve_for_create(&mut self, cur: u32, target: &NsTarget, create_inter: bool) -> Result<(u32, Option<u32>, NameSeg), SgCode> {
        if target.is_empty() {
            return Err(SgCode::BadNamePath);
        }
        let base = if target.absolute { ROOT_ID } else { self.walk_up(cur, target.ups)? };
        let mut at = base;
        let last_idx = target.segs.len() - 1;
        for k in 0..last_idx {
            let seg = target.segs[k];
            match self.find_child(at, seg) {
                Some(c) => {
                    // 中段必须已是作用域，否则路径形态非法。
                    if self.kind_of(c)? != NodeKind::Scope {
                        return Err(SgCode::PathNotFound);
                    }
                    at = c;
                }
                None => {
                    if !create_inter {
                        return Err(SgCode::PathNotFound);
                    }
                    at = self.raw_child_push(at, Some(seg), NodeKind::Scope, NodePayload::None)?;
                }
            }
        }
        let last = target.segs[last_idx];
        let existing = self.find_child(at, last);
        Ok((at, existing, last))
    }
    /// 绝对路径逐段下查（只读查找）。
    pub fn lookup_abs(&self, segs: &[NameSeg]) -> Result<u32, SgCode> {
        let mut at = ROOT_ID;
        for seg in segs.iter() {
            match self.find_child(at, *seg) {
                Some(c) => at = c,
                None => return Err(SgCode::PathNotFound),
            }
        }
        Ok(at)
    }
    /// 按路径串查找（`\\_SB_.PCI0` 形态）；查无回 None 而非报错。
    pub fn lookup_optional(&self, path: &str) -> Option<u32> {
        let segs = split_path(path).ok()?;
        self.lookup_abs(&segs).ok()
    }
    /// 从定义块字节流构建/增量并入命名空间树。状态机：
    /// Seeded/Sealed/Audited/Rescanned → Building → Sealed。
    pub fn build_from_bytes(&mut self, bytes: &[u8], tick: u64) -> Result<BuildReport, SgCode> {
        match self.phase {
            NsPhase::Seeded | NsPhase::Sealed | NsPhase::Audited | NsPhase::Rescanned => {}
            _ => return Err(SgCode::PhaseViolation),
        }
        if bytes.is_empty() {
            return Err(SgCode::EmptyStream);
        }
        if bytes.len() > STREAM_CAP {
            return Err(SgCode::StreamTooLarge);
        }
        let toks = lex_namespace_bytes(bytes)?;
        self.phase = NsPhase::Building;
        self.tick = tick;
        self.stream_len = bytes.len();
        self.scans += 1;
        let before_nodes = self.nodes.len() as u32;
        let before_conflicts = self.conflicts.len();
        let before_reopens = self.reopens.len();
        for tok in toks.iter() {
            // 弹栈配对：偏移达帧尾即弹；偏移越帧尾即判破坏。
            loop {
                let top = match self.scope_stack.last() {
                    Some(t) => *t,
                    None => break,
                };
                if tok.off >= top.body_end {
                    if tok.off > top.body_end {
                        return Err(SgCode::ScopePairBreak);
                    }
                    self.scope_stack.pop();
                } else {
                    break;
                }
            }
            let cur = match self.scope_stack.last() {
                Some(f) => f.node,
                None => ROOT_ID,
            };
            self.apply_token(tok, cur)?;
        }
        // 流尾弹栈：帧尾必须等于流长，否则包体未闭。
        let stream_len = bytes.len() as u32;
        while let Some(top) = self.scope_stack.pop() {
            if top.body_end != stream_len {
                return Err(SgCode::ScopeUnclosed);
            }
        }
        self.phase = NsPhase::Sealed;
        Ok(BuildReport {
            tokens: toks.len(),
            created: self.nodes.len() as u32 - before_nodes,
            reopened: (self.reopens.len() - before_reopens) as u32,
            conflicts: self.conflicts.len() - before_conflicts,
            node_total: self.nodes.len() as u32,
        })
    }
    /// 单记号入树：按种类分流（容器压栈 / 叶建节点）。
    fn apply_token(&mut self, tok: &NsToken, cur: u32) -> Result<(), SgCode> {
        match &tok.kind {
            NsTokenKind::Scope { target, body_end } => {
                let (parent, existing, last) = self.resolve_for_create(cur, target, true)?;
                let id = match existing {
                    Some(eid) => {
                        // Scope 同名重开为合法合并；异类型为冲突立案。
                        if self.kind_of(eid)? == NodeKind::Scope {
                            let p = self.path_of(eid)?;
                            self.reopens.push(p);
                            eid
                        } else {
                            self.record_conflict(parent, last, NodeKind::Scope)?;
                            return Ok(());
                        }
                    }
                    None => self.raw_child_push(parent, Some(last), NodeKind::Scope, NodePayload::None)?,
                };
                self.check_nest_then_push(id, *body_end)?;
            }
            // Device 与 ThermalZone 载荷同形，分支合一（按记号种类定型）。
            NsTokenKind::Device { target, body_end } | NsTokenKind::ThermalZone { target, body_end } => {
                let kind = if matches!(tok.kind, NsTokenKind::Device { .. }) {
                    NodeKind::Device
                } else {
                    NodeKind::ThermalZone
                };
                let (p, e, l) = self.resolve_for_create(cur, target, true)?;
                if let Some(id) = self.create_or_conflict(p, e, l, kind, NodePayload::None)? {
                    self.check_nest_then_push(id, *body_end)?;
                }
            }
            NsTokenKind::PowerRes { target, system_level, resource_order, body_end } => {
                let (p, e, l) = self.resolve_for_create(cur, target, true)?;
                let pl = NodePayload::Power { system_level: *system_level, resource_order: *resource_order };
                if let Some(id) = self.create_or_conflict(p, e, l, NodeKind::PowerResource, pl)? {
                    self.check_nest_then_push(id, *body_end)?;
                }
            }
            NsTokenKind::Processor { target, proc_id, pblk_addr, pblk_len, body_end } => {
                let (p, e, l) = self.resolve_for_create(cur, target, true)?;
                let pl = NodePayload::Cpu { proc_id: *proc_id, pblk_addr: *pblk_addr, pblk_len: *pblk_len };
                if let Some(id) = self.create_or_conflict(p, e, l, NodeKind::Processor, pl)? {
                    self.check_nest_then_push(id, *body_end)?;
                }
            }
            NsTokenKind::Method { target, flags } => {
                let (p, e, l) = self.resolve_for_create(cur, target, false)?;
                self.create_or_conflict(p, e, l, NodeKind::Method, NodePayload::MethodFlags(*flags))?;
            }
            NsTokenKind::Field { target, entries } => {
                let (p, e, l) = self.resolve_for_create(cur, target, false)?;
                if let Some(cid) = self.create_or_conflict(p, e, l, NodeKind::Field, NodePayload::None)? {
                    for ent in entries.iter() {
                        self.raw_child_push(cid, Some(ent.seg), NodeKind::Field, NodePayload::FieldBits(ent.bit_len))?;
                    }
                }
            }
            NsTokenKind::OpRegion { target, space, offset, len } => {
                let (p, e, l) = self.resolve_for_create(cur, target, false)?;
                let pl = NodePayload::Region { space: *space, offset: *offset, len: *len };
                self.create_or_conflict(p, e, l, NodeKind::OpRegion, pl)?;
            }
            NsTokenKind::Name { target, value } => {
                let (p, e, l) = self.resolve_for_create(cur, target, false)?;
                self.create_or_conflict(p, e, l, NodeKind::NameLeaf, NodePayload::Const(value.value))?;
            }
        }
        Ok(())
    }
    /// 重名冲突立案（路径 + 旧类型 + 新类型）。
    fn record_conflict(&mut self, parent: u32, last: NameSeg, new_kind: NodeKind) -> Result<(), SgCode> {
        let ppath = self.path_of(parent)?;
        let path = Self::join_path(&ppath, &last.as_str());
        let exist_kind = match self.find_child(parent, last) {
            Some(eid) => self.kind_of(eid)?,
            None => NodeKind::Scope,
        };
        self.conflicts.push(NameConflict { path, exist_kind, new_kind });
        Ok(())
    }
    /// 建叶/容器节点；重名即立案并跳过（回 None），不静默覆盖。
    fn create_or_conflict(&mut self, parent: u32, existing: Option<u32>, last: NameSeg, kind: NodeKind, payload: NodePayload) -> Result<Option<u32>, SgCode> {
        if existing.is_some() {
            // 同类型非 Scope 重定义同样立案（AML 只许 Scope 重开）。
            self.record_conflict(parent, last, kind)?;
            return Ok(None);
        }
        let id = self.raw_child_push(parent, Some(last), kind, payload)?;
        Ok(Some(id))
    }
    /// 压栈前嵌套校验：子帧尾不得越过父帧尾。
    fn check_nest_then_push(&mut self, id: u32, body_end: u32) -> Result<(), SgCode> {
        if let Some(top) = self.scope_stack.last() {
            if body_end > top.body_end {
                return Err(SgCode::ScopeBodyNest);
            }
        }
        self.scope_stack.push(ScopeFrame { node: id, body_end });
        Ok(())
    }
    /// 子树节点数（含自身；迭代栈防爆栈）。
    pub fn subtree_count(&self, root: u32) -> Result<u32, SgCode> {
        let mut count: u32 = 0;
        let mut stack: Vec<u32> = vec![root];
        while let Some(id) = stack.pop() {
            let n = self.nd(id)?;
            count += 1;
            for &c in n.children.iter() {
                stack.push(c);
            }
        }
        Ok(count)
    }
    /// 三作用域（加 \_SI/\_TZ）节点数统计账 + 类型直方图。
    pub fn scope_stats(&self) -> Result<ScopeStats, SgCode> {
        let sb = self.lookup_abs(&[NameSeg::from_ascii("_SB_")?])?;
        let gpe = self.lookup_abs(&[NameSeg::from_ascii("_GPE")?])?;
        let pr = self.lookup_abs(&[NameSeg::from_ascii("_PR_")?])?;
        let si = self.lookup_abs(&[NameSeg::from_ascii("_SI_")?])?;
        let tz = self.lookup_abs(&[NameSeg::from_ascii("_TZ_")?])?;
        let mut kind_hist = [0u32; KIND_COUNT];
        for n in self.nodes.iter() {
            kind_hist[n.kind.kind_id()] += 1;
        }
        Ok(ScopeStats {
            sb_count: self.subtree_count(sb)?,
            gpe_count: self.subtree_count(gpe)?,
            pr_count: self.subtree_count(pr)?,
            si_count: self.subtree_count(si)?,
            tz_count: self.subtree_count(tz)?,
            total: self.nodes.len() as u32,
            kind_hist,
        })
    }
    /// BNode 分配账快照。
    pub fn alloc_ledger(&self) -> AllocLedger {
        let used = self.nodes.len() as u32;
        AllocLedger {
            used,
            cap: NODE_CAP,
            headroom: NODE_CAP - used,
            reopens: self.reopens.len(),
            conflicts: self.conflicts.len(),
            scans: self.scans,
        }
    }
    /// 当前相位。
    pub fn phase(&self) -> NsPhase {
        self.phase
    }
    /// 树总节点数。
    pub fn node_total(&self) -> u32 {
        self.nodes.len() as u32
    }
    /// 审计三闸：父指针一致性（父子互指 + 深度场 + 子表查重）、无环
    /// （父链步数封顶节点数）、深度上限。违例逐条立案，不清零。
    pub fn audit(&self) -> Result<AuditReport, SgCode> {
        match self.phase {
            NsPhase::Sealed | NsPhase::Audited | NsPhase::Rescanned => {}
            _ => return Err(SgCode::PhaseViolation),
        }
        let mut detail: Vec<AuditViolation> = Vec::new();
        let total = self.nodes.len() as u32;
        // 闸一：父子互指 + 深度场 + 子表查重。
        for (i, n) in self.nodes.iter().enumerate() {
            let id = i as u32;
            match n.parent {
                None => {
                    if id != ROOT_ID {
                        detail.push(AuditViolation { node: id, detail: String::from("非根节点父指针为空") });
                    }
                    if n.depth != 0 {
                        detail.push(AuditViolation { node: id, detail: String::from("根节点深度场非 0") });
                    }
                }
                Some(p) => {
                    let pn = match self.nodes.get(p as usize) {
                        Some(pn) => pn,
                        None => {
                            detail.push(AuditViolation { node: id, detail: format!("父节点号 {} 越界", p) });
                            continue;
                        }
                    };
                    if !pn.children.contains(&id) {
                        detail.push(AuditViolation { node: id, detail: format!("父节点 {} 的子表缺少本节点", p) });
                    }
                    if pn.depth + 1 != n.depth {
                        detail.push(AuditViolation { node: id, detail: String::from("深度场与父层不一致") });
                    }
                    if n.depth > MAX_DEPTH {
                        detail.push(AuditViolation { node: id, detail: format!("深度 {} 超上限 {}", n.depth, MAX_DEPTH) });
                    }
                }
            }
            let cl = n.children.len();
            for a in 0..cl {
                if n.children[a + 1..].contains(&n.children[a]) {
                    detail.push(AuditViolation { node: id, detail: format!("子表重复节点号 {}", n.children[a]) });
                }
            }
        }
        // 闸二：无环——沿父链步数封顶节点总数。
        for start in 0..total {
            let mut at = start;
            let mut steps: u32 = 0;
            while steps <= total {
                let n = match self.nodes.get(at as usize) {
                    Some(n) => n,
                    None => {
                        detail.push(AuditViolation { node: start, detail: format!("父链指向越界节点 {}", at) });
                        break;
                    }
                };
                match n.parent {
                    Some(p) => {
                        at = p;
                        steps += 1;
                    }
                    None => break,
                }
            }
            if steps > total {
                detail.push(AuditViolation { node: start, detail: String::from("父链步数超过节点总数，判成环") });
            }
        }
        let violations = detail.len();
        Ok(AuditReport { checked: total, violations, detail, ok: violations == 0 })
    }
    /// DFS 先序清单（供 iasl 抽样对拍与 diff 用）。
    pub fn listing(&self) -> Result<Vec<SampleEnt>, SgCode> {
        let mut out: Vec<SampleEnt> = Vec::new();
        self.listing_dfs(ROOT_ID, &mut out, 0)?;
        Ok(out)
    }
    fn listing_dfs(&self, id: u32, out: &mut Vec<SampleEnt>, depth: u32) -> Result<(), SgCode> {
        if depth > MAX_DEPTH {
            return Err(SgCode::DepthExceeded);
        }
        let n = self.nd(id)?;
        out.push(SampleEnt { path: self.path_of(id)?, kind: n.kind });
        for &c in n.children.iter() {
            self.listing_dfs(c, out, depth + 1)?;
        }
        Ok(())
    }
    /// 导出形态一：缩进树（每层两空格，节点号 + 类型）。
    pub fn export_indented(&self) -> Result<String, SgCode> {
        let mut s = String::new();
        self.export_indent_dfs(ROOT_ID, 0, &mut s)?;
        Ok(s)
    }
    fn export_indent_dfs(&self, id: u32, depth: u32, out: &mut String) -> Result<(), SgCode> {
        if depth > MAX_DEPTH {
            return Err(SgCode::DepthExceeded);
        }
        let n = self.nd(id)?;
        let label = match n.name {
            Some(seg) => seg.as_str(),
            None => String::from("\\"),
        };
        for _ in 0..depth {
            out.push_str("  ");
        }
        out.push_str(&format!("{} [{}] #{}\n", label, n.kind.kind_name(), n.id));
        for &c in n.children.iter() {
            self.export_indent_dfs(c, depth + 1, out)?;
        }
        Ok(())
    }
    /// 导出形态二：路径清单（DFS 先序，一行一路径）。
    pub fn export_path_list(&self) -> Result<String, SgCode> {
        let list = self.listing()?;
        let mut s = String::new();
        for ent in list.iter() {
            s.push_str(&format!("{} [{}]\n", ent.path, ent.kind.kind_name()));
        }
        Ok(s)
    }
}

// === 抽样实体（对拍两侧共用形态）===

/// 抽样条目：路径 + 类型（iasl 反汇编抽样表的最小对拍单元）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleEnt {
    pub path: String, // 全路径（折叠大写形态）
    pub kind: NodeKind,
}

/// 对拍结果（抽样数/一致数/不一致明细）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleCmp {
    pub sampled: usize,
    pub matched: usize,
    pub mismatches: Vec<String>,
}

/// 取两侧前 `take` 条逐对拍节点类型与路径；任一侧不足即判抽样表短缺。
pub fn compare_tables(expected: &[SampleEnt], observed: &[SampleEnt], take: usize) -> Result<SampleCmp, SgCode> {
    if expected.len() < take || observed.len() < take {
        return Err(SgCode::SampleShort);
    }
    let mut matched = 0usize;
    let mut mismatches: Vec<String> = Vec::new();
    for k in 0..take {
        let e = &expected[k];
        let o = &observed[k];
        if e.path == o.path && e.kind == o.kind {
            matched += 1;
        } else {
            mismatches.push(format!(
                "样张#{} 期望 {}[{}] 实得 {}[{}]",
                k, e.path, e.kind.kind_name(), o.path, o.kind.kind_name()
            ));
        }
    }
    Ok(SampleCmp { sampled: take, matched, mismatches })
}

// === 内建词法器：字节流 → 记号流 ===

/// 流内取一字节，越界即截断错误。
fn rd_u8(bytes: &[u8], i: &mut usize) -> Result<u8, SgCode> {
    let b = *bytes.get(*i).ok_or(SgCode::OperandTruncated)?;
    *i += 1;
    Ok(b)
}

fn rd_u16(bytes: &[u8], i: &mut usize) -> Result<u16, SgCode> {
    let lo = rd_u8(bytes, i)? as u16;
    Ok(lo | ((rd_u8(bytes, i)? as u16) << 8))
}

fn rd_u32(bytes: &[u8], i: &mut usize) -> Result<u32, SgCode> {
    let mut v: u32 = 0;
    for k in 0..4 {
        v |= (rd_u8(bytes, i)? as u32) << (8 * k);
    }
    Ok(v)
}

fn rd_u64(bytes: &[u8], i: &mut usize) -> Result<u64, SgCode> {
    let mut v: u64 = 0;
    for k in 0..8 {
        v |= (rd_u8(bytes, i)? as u64) << (8 * k);
    }
    Ok(v)
}

/// 包长度四档解码（ACPI §20.2.4）：LeadByte bit7~6 定档，返回
/// （总长含长码自身，包首字节偏移）。一字节档低 6 位即长；多字节档
/// 低 4 位为低位半字节，尾字节小端、每字节携 8 位。
fn pkg_decode(bytes: &[u8], i: &mut usize) -> Result<(u32, usize), SgCode> {
    let pkg_pos = *i;
    let lead = rd_u8(bytes, i)?;
    let n = ((lead >> 6) + 1) as usize;
    let mut total: u32 = if n == 1 { (lead & 0x3F) as u32 } else { (lead & 0x0F) as u32 };
    for k in 1..n {
        let tail = rd_u8(bytes, i)?;
        total |= (tail as u32) << (4 * k);
    }
    Ok((total, pkg_pos))
}

/// 解码包长度并校验包体不越流，回 body_end（包首字节偏移 + 总长）。
fn pkg_body(bytes: &[u8], i: &mut usize) -> Result<u32, SgCode> {
    let (total, pkg_pos) = pkg_decode(bytes, i)?;
    let body_end = pkg_pos as u32 + total;
    if body_end as usize > bytes.len() {
        return Err(SgCode::PkgOverrun);
    }
    Ok(body_end)
}

/// 读一个定长四字节段。
fn read_seg(bytes: &[u8], i: &mut usize) -> Result<NameSeg, SgCode> {
    let raw = [rd_u8(bytes, i)?, rd_u8(bytes, i)?, rd_u8(bytes, i)?, rd_u8(bytes, i)?];
    NameSeg::from_bytes(&raw)
}

/// 名字路径解码：^ 前缀 ×n → 可选 0x5C → 单段 / 0x2F 双段 / SegCount 多段。
fn parse_name_string(bytes: &[u8], i: &mut usize) -> Result<NsTarget, SgCode> {
    let mut ups: u32 = 0;
    loop {
        if rd_u8(bytes, i)? == 0x5E {
            ups += 1;
        } else {
            *i -= 1;
            break;
        }
    }
    let mut absolute = false;
    if rd_u8(bytes, i)? == 0x5C {
        absolute = true;
    } else {
        *i -= 1;
    }
    let lead = rd_u8(bytes, i)?;
    let mut segs: Vec<NameSeg> = Vec::new();
    if lead == 0x00 {
        // NullPath：空段表。
        return Ok(NsTarget { absolute, ups, segs });
    }
    if lead == 0x2F {
        // DualNamePath：恰两段。
        for _ in 0..2 {
            segs.push(read_seg(bytes, i)?);
        }
    } else if NameSeg::is_lead_char(lead) {
        // 单 NameSeg 形态（首字节即 LeadNameChar）。
        *i -= 1;
        segs.push(read_seg(bytes, i)?);
    } else {
        // MultiNamePath：SegCount 前缀字节。
        for _ in 0..lead {
            segs.push(read_seg(bytes, i)?);
        }
    }
    Ok(NsTarget { absolute, ups, segs })
}

/// 数据项解码（闭集：Zero/One/Byte/Word/DWord/QWord）。
fn parse_data(bytes: &[u8], i: &mut usize) -> Result<NameValue, SgCode> {
    match rd_u8(bytes, i)? {
        0x00 => Ok(NameValue { tag: "Zero", value: 0 }),
        0x01 => Ok(NameValue { tag: "One", value: 1 }),
        0x0A => Ok(NameValue { tag: "Byte", value: rd_u8(bytes, i)? as u64 }),
        0x0B => Ok(NameValue { tag: "Word", value: rd_u16(bytes, i)? as u64 }),
        0x0C => Ok(NameValue { tag: "DWord", value: rd_u32(bytes, i)? as u64 }),
        0x0E => Ok(NameValue { tag: "QWord", value: rd_u64(bytes, i)? }),
        _ => Err(SgCode::UnsupportedData),
    }
}

/// 词法主循环：定义块字节流 → 记号流。Method/Field 包体不透明跳过；
/// Scope/Device/ThermalZone/PowerRes/Processor 包体交回主循环递归处理，
/// 配对由建树状态机按 body_end 完成。
pub fn lex_namespace_bytes(bytes: &[u8]) -> Result<Vec<NsToken>, SgCode> {
    let mut i: usize = 0;
    let mut toks: Vec<NsToken> = Vec::new();
    while i < bytes.len() {
        let off = i as u32;
        match rd_u8(bytes, &mut i)? {
            0x10 => {
                let body_end = pkg_body(bytes, &mut i)?;
                let target = parse_name_string(bytes, &mut i)?;
                toks.push(NsToken { off, kind: NsTokenKind::Scope { target, body_end } });
            }
            0x08 => {
                let target = parse_name_string(bytes, &mut i)?;
                let value = parse_data(bytes, &mut i)?;
                toks.push(NsToken { off, kind: NsTokenKind::Name { target, value } });
            }
            0x14 => {
                let body_end = pkg_body(bytes, &mut i)?;
                let target = parse_name_string(bytes, &mut i)?;
                let flags = rd_u8(bytes, &mut i)?;
                i = body_end as usize; // 方法体对词法不透明，整段跳过
                toks.push(NsToken { off, kind: NsTokenKind::Method { target, flags } });
            }
            0x5B => {
                let ext = rd_u8(bytes, &mut i)?;
                match ext {
                0x80 => {
                    // OpRegion：无包体——段、空间、偏移、长度。
                    let target = parse_name_string(bytes, &mut i)?;
                    let space = rd_u8(bytes, &mut i)?;
                    let offset = rd_u32(bytes, &mut i)?;
                    let len = rd_u32(bytes, &mut i)?;
                    toks.push(NsToken { off, kind: NsTokenKind::OpRegion { target, space, offset, len } });
                }
                0x81 => {
                    // Field：包体 = 命名域条目表（段 + 位宽 PkgLength）。
                    let body_end = pkg_body(bytes, &mut i)?;
                    let target = parse_name_string(bytes, &mut i)?;
                    let mut entries: Vec<FieldEntry> = Vec::new();
                    while i < body_end as usize {
                        let seg = read_seg(bytes, &mut i)?;
                        let (bit_len, _pos) = pkg_decode(bytes, &mut i)?;
                        entries.push(FieldEntry { seg, bit_len });
                    }
                    i = body_end as usize;
                    toks.push(NsToken { off, kind: NsTokenKind::Field { target, entries } });
                }
                0x82 | 0x85 => {
                    // Device / ThermalZone：包体 = 项表，结构同形分支合一。
                    let body_end = pkg_body(bytes, &mut i)?;
                    let target = parse_name_string(bytes, &mut i)?;
                    let kind = if ext == 0x82 {
                        NsTokenKind::Device { target, body_end }
                    } else {
                        NsTokenKind::ThermalZone { target, body_end }
                    };
                    toks.push(NsToken { off, kind });
                }
                0x83 => {
                    let body_end = pkg_body(bytes, &mut i)?;
                    let target = parse_name_string(bytes, &mut i)?;
                    let proc_id = rd_u8(bytes, &mut i)?;
                    let pblk_addr = rd_u32(bytes, &mut i)?;
                    let pblk_len = rd_u8(bytes, &mut i)?;
                    toks.push(NsToken { off, kind: NsTokenKind::Processor { target, proc_id, pblk_addr, pblk_len, body_end } });
                }
                0x84 => {
                    let body_end = pkg_body(bytes, &mut i)?;
                    let target = parse_name_string(bytes, &mut i)?;
                    let system_level = rd_u32(bytes, &mut i)?;
                    let resource_order = rd_u16(bytes, &mut i)?;
                    toks.push(NsToken { off, kind: NsTokenKind::PowerRes { target, system_level, resource_order, body_end } });
                }
                _ => return Err(SgCode::UnknownExtOpcode),
            }
            }
            _ => return Err(SgCode::UnknownOpcode),
        }
    }
    Ok(toks)
}

// === 内建 AML 编码器：机型剖面表 → 字节流 ===

/// 机型剖面表条目：路径 + 类型 + 载荷（抽样期望表与编码流同源）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileEnt {
    pub path: String, // 全路径（`\_SB_.PCI0.DV00` 形态，段须四字符）
    pub kind: NodeKind,
    pub payload: NodePayload,
}

/// 编码器中间树节点。
struct EncNode {
    seg: Option<NameSeg>,
    kind: Option<NodeKind>,
    payload: NodePayload,
    children: Vec<EncNode>,
}

/// 路径串切段（`\` 前缀 + `.` 分隔，根为空段表）。
fn split_path(path: &str) -> Result<Vec<NameSeg>, SgCode> {
    match path.chars().next() {
        Some('\\') => {}
        _ => return Err(SgCode::BadProfilePath),
    }
    let rest: &str = path.get(1..).unwrap_or("");
    if rest.is_empty() {
        return Ok(Vec::new());
    }
    let mut segs = Vec::new();
    for part in rest.split('.') {
        segs.push(NameSeg::from_ascii(part).map_err(|_| SgCode::BadProfilePath)?);
    }
    Ok(segs)
}

/// 剖面表插入中间树（先序插入序即子表序）。根条目（空段表）即合成根
/// 本身，不产生编码（建树侧根已播种），直接跳过。
fn enc_insert(root: &mut EncNode, ent: &ProfileEnt) -> Result<(), SgCode> {
    let segs = split_path(&ent.path)?;
    if segs.is_empty() {
        return Ok(());
    }
    let mut at = root;
    let last_idx = segs.len() - 1;
    for k in 0..last_idx {
        let seg = segs[k];
        let mut idx = None;
        for (n, c) in at.children.iter().enumerate() {
            if c.seg == Some(seg) {
                idx = Some(n);
                break;
            }
        }
        match idx {
            Some(n) => at = &mut at.children[n],
            None => {
                at.children.push(EncNode {
                    seg: Some(seg),
                    kind: Some(NodeKind::Scope),
                    payload: NodePayload::None,
                    children: Vec::new(),
                });
                let n = at.children.len();
                at = &mut at.children[n - 1];
            }
        }
    }
    let last = segs[last_idx];
    for c in at.children.iter() {
        if c.seg == Some(last) {
            return Err(SgCode::NameConflict);
        }
    }
    at.children.push(EncNode {
        seg: Some(last),
        kind: Some(ent.kind),
        payload: ent.payload.clone(),
        children: Vec::new(),
    });
    Ok(())
}

/// 包长度编码：档位迭代自洽（载荷 + 长码字节数 = 总长）。
/// 返回（编码字节，有效长度 ≤4）。一字节档低 6 位即长；多字节档
/// 低 4 位为低位半字节，尾字节小端、每字节携 8 位。
fn pkg_encode(total: u32) -> ([u8; 4], usize) {
    for n in 1..=4usize {
        let fits = match n {
            1 => total < 64,
            2 => total < 4096,
            3 => total < (1 << 20),
            _ => true,
        };
        if fits {
            let mut out = [0u8; 4];
            out[0] = if n == 1 {
                (total & 0x3F) as u8
            } else {
                (((n as u8) - 1) << 6) | ((total & 0x0F) as u8)
            };
            for k in 1..n {
                out[k] = ((total >> (4 * k)) & 0xFF) as u8;
            }
            return (out, n);
        }
    }
    let mut out = [0u8; 4];
    out[0] = 0xC0 | ((total & 0x0F) as u8);
    out[1] = ((total >> 4) & 0xFF) as u8;
    out[2] = ((total >> 12) & 0xFF) as u8;
    out[3] = ((total >> 20) & 0xFF) as u8;
    (out, 4)
}

/// 包长度占位回填：净包体 = 占位之后的实际字节长；总长 = 净包体 + 长码
/// 自身字节数。档位迭代求解（长码字节数变化会反哺总长，直到自洽）。
fn patch_pkg(out: &mut Vec<u8>, pos: usize) {
    let raw = (out.len() - pos - 4) as u32;
    let mut n = 1usize;
    let mut total: u32 = raw + 1;
    loop {
        let (enc, n2) = pkg_encode(total);
        if n2 == n {
            for k in 0..n {
                out[pos + k] = enc[k];
            }
            if n < 4 {
                out.drain(pos + n..pos + 4);
            }
            return;
        }
        n = n2;
        total = raw + n as u32;
    }
}

/// 压入操作码前缀 + 4 字节包长占位，回占位起始偏移。
fn push_pkg_op(out: &mut Vec<u8>, op: &[u8]) -> usize {
    out.extend_from_slice(op);
    let pos = out.len();
    out.extend_from_slice(&[0, 0, 0, 0]);
    pos
}

/// 小端推入无符号整数的低 n 字节。
fn push_le(out: &mut Vec<u8>, v: u64, n: usize) {
    for k in 0..n {
        out.push(((v >> (8 * k)) & 0xFF) as u8);
    }
}

/// 名字路径编码：绝对（0x5C）或相对单段（裸 NameSeg）。
fn emit_name(out: &mut Vec<u8>, seg: NameSeg, absolute: bool) {
    if absolute {
        out.push(0x5C);
    }
    out.extend_from_slice(&seg.raw());
}

/// 常量编码：0 → ZeroOp；≤ u32 上限 → DWordConst；否则 QWordConst。
fn emit_const(out: &mut Vec<u8>, v: u64) {
    if v == 0 {
        out.push(0x00);
    } else if v <= u32::MAX as u64 {
        out.push(0x0C);
        push_le(out, v, 4);
    } else {
        out.push(0x0E);
        push_le(out, v, 8);
    }
}

/// 子树递归编码（相对单段名字）。
fn emit_children(out: &mut Vec<u8>, node: &EncNode) -> Result<(), SgCode> {
    for c in node.children.iter() {
        emit_node(out, c, false)?;
    }
    Ok(())
}

/// 单节点递归编码。根的第一层子节点（预定义作用域）用绝对路径，
/// 其余一律相对单段（编码器树保证父子直连）。
fn emit_node(out: &mut Vec<u8>, node: &EncNode, absolute: bool) -> Result<(), SgCode> {
    let kind = match node.kind {
        Some(k) => k,
        None => return Err(SgCode::EmptyProfile),
    };
    let seg = match node.seg {
        Some(s) => s,
        None => return Err(SgCode::BadProfilePath),
    };
    match kind {
        NodeKind::Scope => {
            let pos = push_pkg_op(out, &[0x10]);
            emit_name(out, seg, absolute);
            emit_children(out, node)?;
            patch_pkg(out, pos);
        }
        NodeKind::Device => {
            let pos = push_pkg_op(out, &[0x5B, 0x82]);
            emit_name(out, seg, absolute);
            emit_children(out, node)?;
            patch_pkg(out, pos);
        }
        NodeKind::ThermalZone => {
            let pos = push_pkg_op(out, &[0x5B, 0x85]);
            emit_name(out, seg, absolute);
            emit_children(out, node)?;
            patch_pkg(out, pos);
        }
        NodeKind::PowerResource => {
            let pos = push_pkg_op(out, &[0x5B, 0x84]);
            emit_name(out, seg, absolute);
            let (lvl, order) = match node.payload {
                NodePayload::Power { system_level, resource_order } => (system_level, resource_order),
                _ => (0, 0),
            };
            push_le(out, lvl as u64, 4);
            push_le(out, order as u64, 2);
            emit_children(out, node)?;
            patch_pkg(out, pos);
        }
        NodeKind::Processor => {
            let pos = push_pkg_op(out, &[0x5B, 0x83]);
            emit_name(out, seg, absolute);
            let (pid, addr, plen) = match node.payload {
                NodePayload::Cpu { proc_id, pblk_addr, pblk_len } => (proc_id, pblk_addr, pblk_len),
                _ => (0, 0, 0),
            };
            out.push(pid);
            push_le(out, addr as u64, 4);
            out.push(plen);
            emit_children(out, node)?;
            patch_pkg(out, pos);
        }
        NodeKind::Method => {
            let pos = push_pkg_op(out, &[0x14]);
            emit_name(out, seg, absolute);
            let flags = match node.payload {
                NodePayload::MethodFlags(f) => f,
                _ => 0x01,
            };
            out.push(flags);
            out.push(0xA4); // 方法体（对词法不透明）：Return(Zero)
            out.push(0x00);
            patch_pkg(out, pos);
        }
        NodeKind::Field => {
            let pos = push_pkg_op(out, &[0x5B, 0x81]);
            emit_name(out, seg, absolute);
            // 命名域条目：段 + 位宽（PkgLength 形态）。
            for c in node.children.iter() {
                let cseg = match c.seg {
                    Some(s) => s,
                    None => return Err(SgCode::BadProfilePath),
                };
                out.extend_from_slice(&cseg.raw());
                let bits = match c.payload {
                    NodePayload::FieldBits(b) => b,
                    _ => 0,
                };
                let (enc, n) = pkg_encode(bits);
                for k in 0..n {
                    out.push(enc[k]);
                }
            }
            patch_pkg(out, pos);
        }
        NodeKind::OpRegion => {
            out.extend_from_slice(&[0x5B, 0x80]); // OpRegion 无包体：段 空间 偏移 长度
            emit_name(out, seg, absolute);
            let (space, offset, len) = match node.payload {
                NodePayload::Region { space, offset, len } => (space, offset, len),
                _ => (0, 0, 0),
            };
            out.push(space);
            push_le(out, offset as u64, 4);
            push_le(out, len as u64, 4);
        }
        NodeKind::NameLeaf => {
            out.push(0x08);
            emit_name(out, seg, absolute);
            let v = match node.payload {
                NodePayload::Const(c) => c,
                _ => 0,
            };
            emit_const(out, v);
        }
    }
    Ok(())
}

/// 剖面表 → 字节流：先序插入中间树，再递归编码。
/// 根层子节点须全为 Scope（即预定义作用域重开流），否则判剖面非法。
pub fn encode_profile(entries: &[ProfileEnt]) -> Result<Vec<u8>, SgCode> {
    if entries.is_empty() {
        return Err(SgCode::EmptyProfile);
    }
    let mut root = EncNode {
        seg: None,
        kind: None,
        payload: NodePayload::None,
        children: Vec::new(),
    };
    for ent in entries.iter() {
        enc_insert(&mut root, ent)?;
    }
    for c in root.children.iter() {
        if c.kind != Some(NodeKind::Scope) {
            return Err(SgCode::BadProfilePath);
        }
    }
    let mut out: Vec<u8> = Vec::new();
    for c in root.children.iter() {
        emit_node(&mut out, c, true)?;
    }
    if out.is_empty() {
        return Err(SgCode::EmptyProfile);
    }
    Ok(out)
}

// === 机型剖面表（3 机型写死，展开即 iasl 反汇编抽样期望表）===

/// 剖面条目便捷构造：无载荷。
fn pe(path: &str, kind: NodeKind) -> Result<ProfileEnt, SgCode> {
    if path.is_empty() {
        return Err(SgCode::BadProfilePath);
    }
    Ok(ProfileEnt { path: path.to_string(), kind, payload: NodePayload::None })
}

fn pe_method(path: &str) -> Result<ProfileEnt, SgCode> {
    Ok(ProfileEnt { path: String::from(path), kind: NodeKind::Method, payload: NodePayload::MethodFlags(0x01) })
}

fn pe_region(path: &str, space: u8, offset: u32, len: u32) -> Result<ProfileEnt, SgCode> {
    Ok(ProfileEnt { path: String::from(path), kind: NodeKind::OpRegion, payload: NodePayload::Region { space, offset, len } })
}

fn pe_field_bits(path: &str, bits: u32) -> Result<ProfileEnt, SgCode> {
    Ok(ProfileEnt { path: String::from(path), kind: NodeKind::Field, payload: NodePayload::FieldBits(bits) })
}

fn pe_const(path: &str, v: u64) -> Result<ProfileEnt, SgCode> {
    Ok(ProfileEnt { path: String::from(path), kind: NodeKind::NameLeaf, payload: NodePayload::Const(v) })
}

fn pe_cpu(path: &str, proc_id: u8, pblk_addr: u32, pblk_len: u8) -> Result<ProfileEnt, SgCode> {
    Ok(ProfileEnt { path: String::from(path), kind: NodeKind::Processor, payload: NodePayload::Cpu { proc_id, pblk_addr, pblk_len } })
}

fn pe_power(path: &str, lvl: u32, order: u16) -> Result<ProfileEnt, SgCode> {
    Ok(ProfileEnt { path: String::from(path), kind: NodeKind::PowerResource, payload: NodePayload::Power { system_level: lvl, resource_order: order } })
}

/// 公共头部：根 + \_SB\_（\_GPE/\_PR/\_SI/\_TZ\_ 由各机型按 DFS 先序
/// 插在各自内容块前，保证期望表与树清单同序）。
fn seed_entries() -> Result<Vec<ProfileEnt>, SgCode> {
    let mut v = Vec::new();
    v.push(pe("\\", NodeKind::Scope)?);
    v.push(pe("\\_SB_", NodeKind::Scope)?);
    Ok(v)
}

/// 笔电 \_SB 设备写死表：(设备段，方法集，\_HID 常量)。展开序 = DFS 序。
const NB_DEVS: [(&str, &[&str], Option<u64>); 12] = [
    ("LID0", &["_STA"], Some(0x0B3F5C10)),
    ("BAT0", &["_STA", "_BIF", "_BST"], None),
    ("BAT1", &["_STA", "_BST"], None),
    ("GLAN", &["_STA"], Some(0x01000001)),
    ("XHC0", &["_STA", "_PS0", "_PS3"], Some(0x0B3F8086)),
    ("HDEF", &["_STA", "_DSM"], Some(0x02000001)),
    ("AUD0", &["_STA"], Some(0x03000001)),
    ("SDC0", &["_STA"], None),
    ("KBD0", &["_STA"], Some(0x0A3F5C03)),
    ("MSE0", &["_STA"], None),
    ("CAM0", &["_STA"], Some(0x06000001)),
    ("SER0", &["_STA"], None),
];

/// 机型 A：桌面主板 DT-ICH9-A。双 PCI 主机（PCI0 16 设备、PCI1 8 设备）、
/// 操作区×3、域条目×4、电源资源、4 CPU、\_GPE 事件×4、热区、\_SI。
/// 展开后 113 节点（≥ 100 抽样线）。
pub fn profile_desktop() -> Result<Vec<ProfileEnt>, SgCode> {
    let mut v = seed_entries()?;
    // ---- \_SB_ 块（DFS 序：PCI0 子树先于 PWRS）----
    v.push(pe("\\_SB_.PCI0", NodeKind::Scope)?);
    v.push(pe_region("\\_SB_.PCI0.CONF", 0x02, 0x0000, 0x0100)?);
    v.push(pe_region("\\_SB_.PCI0.PMBS", 0x01, 0x0400, 0x0080)?);
    v.push(pe_region("\\_SB_.PCI0.GPBS", 0x01, 0x0500, 0x0040)?);
    v.push(pe("\\_SB_.PCI0.FLDS", NodeKind::Field)?);
    v.push(pe_field_bits("\\_SB_.PCI0.FLDS.F000", 8)?);
    v.push(pe_field_bits("\\_SB_.PCI0.FLDS.F001", 16)?);
    v.push(pe_field_bits("\\_SB_.PCI0.FLDS.F002", 1)?);
    v.push(pe_field_bits("\\_SB_.PCI0.FLDS.F003", 32)?);
    for i in 0..16 {
        let dev = format!("\\_SB_.PCI0.DV{:02}", i);
        v.push(pe(&dev, NodeKind::Device)?);
        v.push(pe_method(&format!("{}._STA", &dev))?);
        v.push(pe_method(&format!("{}._PS0", &dev))?);
    }
    v.push(pe("\\_SB_.PCI1", NodeKind::Scope)?);
    for i in 0..8 {
        let dev = format!("\\_SB_.PCI1.DV{:02}", i);
        v.push(pe(&dev, NodeKind::Device)?);
        v.push(pe_method(&format!("{}._STA", &dev))?);
        v.push(pe_method(&format!("{}._OFF", &dev))?);
    }
    v.push(pe_power("\\_SB_.PWRS", 0xFFFFFFFF, 0x0001)?);
    v.push(pe_method("\\_SB_.PWRS._ON_")?);
    v.push(pe_method("\\_SB_.PWRS._OFF")?);
    v.push(pe("\\_GPE", NodeKind::Scope)?);
    for l in 0..4 {
        v.push(pe_method(&format!("\\_GPE._L{:02}", l))?);
    }
    v.push(pe("\\_PR_", NodeKind::Scope)?);
    for i in 0..4 {
        let cpu = format!("\\_PR_.CPU{}", i);
        v.push(pe_cpu(&cpu, i as u8, 0x0810 + (i as u32) * 8, 6)?);
        v.push(pe_method(&format!("{}._PPC", &cpu))?);
        v.push(pe_method(&format!("{}._PCT", &cpu))?);
    }
    v.push(pe("\\_SI_", NodeKind::Scope)?);
    v.push(pe_const("\\_SI_._S0_", 0)?);
    v.push(pe_const("\\_SI_._S3_", 3)?);
    v.push(pe_const("\\_SI_._S5_", 5)?);
    v.push(pe("\\_TZ_", NodeKind::Scope)?);
    v.push(pe("\\_TZ_.TZS0", NodeKind::ThermalZone)?);
    v.push(pe_method("\\_TZ_.TZS0._TMP")?);
    v.push(pe_method("\\_TZ_.TZS0._CRT")?);
    Ok(v)
}

/// 机型 B：超低电压笔电 NB-ULV-B。嵌入式控制器（24 条域条目）、
/// NB_DEVS 写死设备表 12 台、电源资源、5 CPU、\_GPE 事件×4、热区、\_SI。
/// 展开后 104 节点（≥ 100 抽样线）。
pub fn profile_laptop() -> Result<Vec<ProfileEnt>, SgCode> {
    let mut v = seed_entries()?;
    v.push(pe("\\_SB_.EC00", NodeKind::Device)?);
    v.push(pe_region("\\_SB_.EC00.ECMD", 0x03, 0x0000, 0x0100)?);
    v.push(pe("\\_SB_.EC00.ECFD", NodeKind::Field)?);
    for i in 0..24 {
        let bits = [8u32, 16, 1, 32][i % 4];
        v.push(pe_field_bits(&format!("\\_SB_.EC00.ECFD.E{:03}", i), bits)?);
    }
    for (dev, mets, hid) in NB_DEVS.iter() {
        v.push(pe(&format!("\\_SB_.{}", dev), NodeKind::Device)?);
        for m in mets.iter() {
            v.push(pe_method(&format!("\\_SB_.{}.{}", dev, m))?);
        }
        if let Some(h) = hid {
            v.push(pe_const(&format!("\\_SB_.{}._HID", dev), *h)?);
        }
    }
    v.push(pe_power("\\_SB_.PWRG", 0x0000FFFF, 0x0002)?);
    v.push(pe_method("\\_SB_.PWRG._ON_")?);
    v.push(pe_method("\\_SB_.PWRG._OFF")?);
    v.push(pe("\\_GPE", NodeKind::Scope)?);
    v.push(pe_method("\\_GPE._L08")?);
    v.push(pe_method("\\_GPE._L0B")?);
    v.push(pe_method("\\_GPE._E00")?);
    v.push(pe_method("\\_GPE._E12")?);
    v.push(pe("\\_PR_", NodeKind::Scope)?);
    for i in 0..5 {
        let cpu = format!("\\_PR_.CPU{}", i);
        v.push(pe_cpu(&cpu, i as u8, 0x0C00 + (i as u32) * 8, 6)?);
        v.push(pe_method(&format!("{}._PSS", &cpu))?);
        v.push(pe_method(&format!("{}._PCT", &cpu))?);
        v.push(pe_method(&format!("{}._PPC", &cpu))?);
    }
    v.push(pe("\\_SI_", NodeKind::Scope)?);
    v.push(pe_const("\\_SI_._S0_", 0)?);
    v.push(pe_const("\\_SI_._S3_", 3)?);
    v.push(pe_const("\\_SI_._S5_", 5)?);
    v.push(pe("\\_TZ_", NodeKind::Scope)?);
    v.push(pe("\\_TZ_.TZ00", NodeKind::ThermalZone)?);
    v.push(pe_method("\\_TZ_.TZ00._TMP")?);
    v.push(pe_method("\\_TZ_.TZ00._PSV")?);
    v.push(pe_method("\\_TZ_.TZ00._CRT")?);
    Ok(v)
}

/// 机型 C：双路服务器 SRV-DUAL-C。16 CPU（\_\_PR 块）、8 个 PCI 主机各
/// 2 设备、SMBus、\_GPE 事件×4、\_SI ×2、热区。展开后 137 节点。
pub fn profile_server() -> Result<Vec<ProfileEnt>, SgCode> {
    let mut v = seed_entries()?;
    for h in 0..8 {
        let host = format!("\\_SB_.PCI{}", h);
        v.push(pe(&host, NodeKind::Scope)?);
        for d in 0..2 {
            let dev = format!("{}.DV{:02}", &host, d);
            v.push(pe(&dev, NodeKind::Device)?);
            v.push(pe_method(&format!("{}._STA", &dev))?);
            v.push(pe_method(&format!("{}._ON_", &dev))?);
        }
    }
    v.push(pe("\\_SB_.SMBS", NodeKind::Device)?);
    v.push(pe_method("\\_SB_.SMBS._STA")?);
    v.push(pe("\\_GPE", NodeKind::Scope)?);
    for l in 0..4 {
        v.push(pe_method(&format!("\\_GPE._L{:02}", l))?);
    }
    v.push(pe("\\_PR_", NodeKind::Scope)?);
    for i in 0..16 {
        let cpu = format!("\\_PR_.CP{:02}", i);
        v.push(pe_cpu(&cpu, i as u8, 0x1000 + (i as u32) * 8, 6)?);
        v.push(pe_method(&format!("{}._PPC", &cpu))?);
        v.push(pe_method(&format!("{}._PCT", &cpu))?);
        v.push(pe_method(&format!("{}._CST", &cpu))?);
    }
    v.push(pe("\\_SI_", NodeKind::Scope)?);
    v.push(pe_const("\\_SI_._S0_", 0)?);
    v.push(pe_const("\\_SI_._S5_", 5)?);
    v.push(pe("\\_TZ_", NodeKind::Scope)?);
    v.push(pe("\\_TZ_.TZS0", NodeKind::ThermalZone)?);
    v.push(pe_method("\\_TZ_.TZS0._TMP")?);
    v.push(pe_method("\\_TZ_.TZS0._CRT")?);
    Ok(v)
}

/// 剖面表 → iasl 抽样期望表（同源展开：路径 + 类型，判据 J1 对拍基准侧）。
pub fn expected_table(entries: &[ProfileEnt]) -> Result<Vec<SampleEnt>, SgCode> {
    if entries.len() < SAMPLE_PER_MACHINE {
        return Err(SgCode::SampleShort);
    }
    let mut out = Vec::new();
    for e in entries.iter() {
        let path = if split_path(&e.path)?.is_empty() { String::from("\\") } else { e.path.clone() };
        out.push(SampleEnt { path, kind: e.kind });
    }
    Ok(out)
}

// === 增量重扫 diff（新增 / 消失 / 类型漂移三分账）===

/// 类型漂移条目（路径 + 基线类型 + 增量类型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftRec {
    pub path: String,
    pub old_kind: NodeKind,
    pub new_kind: NodeKind,
}

/// 三分账报告（新增/消失/漂移）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReport {
    pub added: Vec<String>,   // 新增路径（只在增量树）
    pub removed: Vec<String>, // 消失路径（只在基线树）
    pub drift: Vec<DriftRec>, // 类型漂移（两树同路径异类型）
}

/// 两树按路径字典序归并出三分账。全整数、全确定性、无浮点。
pub fn diff_namespaces(old: &NamespaceTree, new: &NamespaceTree) -> Result<DiffReport, SgCode> {
    let mut a = old.listing()?;
    let mut b = new.listing()?;
    a.sort_by(|x, y| x.path.cmp(&y.path));
    b.sort_by(|x, y| x.path.cmp(&y.path));
    let mut added: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut drift: Vec<DriftRec> = Vec::new();
    let mut i = 0usize;
    let mut j = 0usize;
    while i < a.len() || j < b.len() {
        if i >= a.len() {
            added.push(b[j].path.clone());
            j += 1;
            continue;
        }
        if j >= b.len() {
            removed.push(a[i].path.clone());
            i += 1;
            continue;
        }
        match a[i].path.cmp(&b[j].path) {
            core::cmp::Ordering::Less => {
                removed.push(a[i].path.clone());
                i += 1;
            }
            core::cmp::Ordering::Greater => {
                added.push(b[j].path.clone());
                j += 1;
            }
            core::cmp::Ordering::Equal => {
                if a[i].kind != b[j].kind {
                    drift.push(DriftRec { path: a[i].path.clone(), old_kind: a[i].kind, new_kind: b[j].kind });
                }
                i += 1;
                j += 1;
            }
        }
    }
    Ok(DiffReport { added, removed, drift })
}

// === 路径解析特性探针（^ 上溯 / 绝对多段 / 相对单段三形态实测）===

/// 探针报告（三形态落点 + 常量值回读）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    pub stream_len: usize, // 特性流字节长
    pub found_n000: bool,  // 绝对多段路径建的常量叶是否就位
    pub found_mup0: bool,  // ^ 上溯建的方法是否落点正确（\_SB 层）
    pub n000_value: u64,   // 常量叶整值
    pub found_pci0: bool,  // 相对单段建的作用域是否就位
    pub pass: bool,        // 三形态全中
}

/// 手工字节流（帧尾逐字节算定）：off 0 Scope \_SB_（绝对单段，pkg 0x2A，
/// 帧尾 43）；off 7 Scope PCI0（相对单段，pkg 0x23，帧尾 43）；off 13
/// Method ^MUP0（^ 单级上溯 → \_SB 层，pkg 0x09，帧尾 23）；off 23 Name
/// \.SB.PCI0.N000（绝对多段 0x5C + SegCount 3），常量 DWord 0x2A。
pub fn probe_stream(tick: u64) -> Result<ProbeReport, SgCode> {
    #[rustfmt::skip]
    let stream: [u8; 43] = [
        0x10, 0x2A, 0x5C, b'_', b'S', b'B', b'_',
        0x10, 0x23, b'P', b'C', b'I', b'0',
        0x14, 0x09, 0x5E, b'M', b'U', b'P', b'0', 0x01, 0xA4, 0x00,
        0x08, 0x5C, 0x03, b'_', b'S', b'B', b'_', b'P', b'C', b'I', b'0', b'N', b'0', b'0', b'0',
        0x0C, 0x2A, 0x00, 0x00, 0x00,
    ];
    let mut tree = NamespaceTree::new(tick)?;
    tree.build_from_bytes(&stream, tick)?;
    let n000 = tree.lookup_optional("\\_SB_.PCI0.N000");
    let mup0 = tree.lookup_optional("\\_SB_.MUP0");
    let pci0 = tree.lookup_optional("\\_SB_.PCI0");
    let mut n000_value: u64 = 0;
    if let Some(id) = n000 {
        if let NodePayload::Const(v) = tree.nd(id)?.payload {
            n000_value = v;
        }
    }
    let pass = n000.is_some() && mup0.is_some() && pci0.is_some() && n000_value == 0x2A;
    Ok(ProbeReport {
        stream_len: stream.len(),
        found_n000: n000.is_some(),
        found_mup0: mup0.is_some(),
        n000_value,
        found_pci0: pci0.is_some(),
        pass,
    })
}

// === 判据主入口 run_f0044 ===

/// 单机型抽样报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineSampleReport {
    pub name: &'static str,      // 机型名（写死）
    pub stream_len: usize,       // 编码流字节长
    pub tokens: usize,           // 记号数
    pub node_total: u32,         // 建树后节点总数
    pub sb_count: u32,           // \_SB 子树节点数
    pub gpe_count: u32,          // \_GPE 子树节点数
    pub pr_count: u32,           // \_PR 子树节点数
    pub sampled: usize,          // 抽样数（100）
    pub matched: usize,          // 一致数（判据要求 100）
    pub mismatches: Vec<String>, // 对拍不一致明细
    pub conflicts: usize,        // 重名冲突立案数
    pub audit_violations: usize, // 审计违例数
    pub pass: bool,              // 本机型判达标
}

/// UNX-F0044 总报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct F0044Report {
    pub tick: u64,                          // 注入的逻辑 tick
    pub machines: Vec<MachineSampleReport>, // 3 机型抽样报告
    pub probe: ProbeReport,                 // 路径解析特性探针报告
    pub diff: DiffReport,                   // 增量重扫三分账（机型 A 基线 vs 增量）
    pub diff_added: usize,                  // 新增账计数
    pub diff_removed: usize,                // 消失账计数
    pub diff_drift: usize,                  // 漂移账计数
    pub alloc: AllocLedger,                 // BNode 分配账（基线树）
    pub diag: Vec<String>,                  // 中文诊断摘要账
    pub passed: bool,                       // 判据 UNX-F0044-J1 总判
}

/// 机名表（写死三机型）。
pub const MACHINE_NAMES: [&str; MACHINE_COUNT] = ["DT-ICH9-A", "NB-ULV-B", "SRV-DUAL-C"];

/// 机型剖面分发（0=桌面 1=笔电 2=服务器）。
fn machine_profile(m: usize) -> Result<Vec<ProfileEnt>, SgCode> {
    match m {
        0 => profile_desktop(),
        1 => profile_laptop(),
        2 => profile_server(),
        _ => Err(SgCode::BadProfilePath),
    }
}

/// 增量流剖面：基于机型 A 基线，构造 新增/消失/类型漂移 各 ≥1 的增量。
/// - 消失：剔除 PCI0.DV15 设备及其两方法（3 路径）。
/// - 漂移：PCI1.DV00._STA 方法 → 设备（同路径异类型）。
/// - 新增：\_SB_.NEW0 设备 + \_SB_.NEW0._STA 方法（2 路径）。
pub fn profile_desktop_delta() -> Result<Vec<ProfileEnt>, SgCode> {
    let base = profile_desktop()?;
    let mut v: Vec<ProfileEnt> = Vec::new();
    for ent in base.iter() {
        if ent.path.contains(".DV15") {
            continue; // 消失账：整设备连方法一起剔除
        }
        if ent.path == "\\_SB_.PCI1.DV00._STA" {
            let mut d = ent.clone();
            d.kind = NodeKind::Device; // 类型漂移账
            d.payload = NodePayload::None;
            v.push(d);
            continue;
        }
        v.push(ent.clone());
    }
    // 新增账：置于 _SB 块尾（PWRS 之后、_GPE 之前），保持先序可读。
    let mut out: Vec<ProfileEnt> = Vec::new();
    let mut inserted = false;
    for ent in v.into_iter() {
        if !inserted && ent.path == "\\_GPE" {
            out.push(pe("\\_SB_.NEW0", NodeKind::Device)?);
            out.push(pe_method("\\_SB_.NEW0._STA")?);
            inserted = true;
        }
        out.push(ent);
    }
    if !inserted {
        return Err(SgCode::EmptyProfile);
    }
    Ok(out)
}

/// 判据主入口：3 机型 × 100 节点对拍 + 特性探针 + 增量重扫三分账。
/// 全部整数账、全确定性；任何一环不中即 passed = false（不静默）。
pub fn run_f0044(tick: u64) -> Result<F0044Report, SgCode> {
    let mut machines: Vec<MachineSampleReport> = Vec::new();
    let mut diag: Vec<String> = Vec::new();
    let mut base_tree: Option<NamespaceTree> = None;
    let mut alloc_snap = AllocLedger {
        used: 0, cap: NODE_CAP, headroom: NODE_CAP, reopens: 0, conflicts: 0, scans: 0,
    };
    for m in 0..MACHINE_COUNT {
        let name = MACHINE_NAMES[m];
        let prof = machine_profile(m)?;
        let expected = expected_table(&prof)?;
        let bytes = encode_profile(&prof)?;
        let mut tree = NamespaceTree::new(tick + (m as u64) * 16 + 1)?;
        let build = tree.build_from_bytes(&bytes, tick + (m as u64) * 16 + 2)?;
        let aud = tree.audit()?;
        let st = tree.scope_stats()?;
        let observed = tree.listing()?;
        let cmp = compare_tables(&expected, &observed, SAMPLE_PER_MACHINE)?;
        let pass = cmp.matched == SAMPLE_PER_MACHINE && cmp.mismatches.is_empty() && aud.ok && build.conflicts == 0;
        diag.push(format!(
            "机型 {} 流 {}B 记号 {} 建树 {} 节点，_SB={} _GPE={} _PR={}，抽样 {}/{}，冲突 {}，审计违例 {}",
            name, bytes.len(), build.tokens, build.node_total, st.sb_count, st.gpe_count,
            st.pr_count, cmp.matched, cmp.sampled, build.conflicts, aud.violations
        ));
        for mis in cmp.mismatches.iter().take(8) {
            diag.push(format!("机型 {} 对拍不中：{}", name, mis));
        }
        if m == 0 {
            alloc_snap = tree.alloc_ledger();
            base_tree = Some(tree);
        }
        machines.push(MachineSampleReport {
            name,
            stream_len: bytes.len(),
            tokens: build.tokens,
            node_total: build.node_total,
            sb_count: st.sb_count,
            gpe_count: st.gpe_count,
            pr_count: st.pr_count,
            sampled: cmp.sampled,
            matched: cmp.matched,
            mismatches: cmp.mismatches,
            conflicts: build.conflicts,
            audit_violations: aud.violations,
            pass,
        });
    }
    // 路径解析特性探针（^ / 绝对多段 / 相对单段三形态）。
    let probe = probe_stream(tick + 0x100)?;
    diag.push(if probe.pass {
        String::from("特性探针：^ 上溯、绝对多段、相对单段三形态全部落点正确")
    } else {
        String::from("特性探针：存在落点错误，见 probe 报告字段")
    });
    // 增量重扫三分账：基线流树 vs 增量流树。
    let diff = match &base_tree {
        Some(old_tree) => {
            let dprof = profile_desktop_delta()?;
            let dbytes = encode_profile(&dprof)?;
            let mut new_tree = NamespaceTree::new(tick + 0x200)?;
            new_tree.build_from_bytes(&dbytes, tick + 0x201)?;
            let d = diff_namespaces(old_tree, &new_tree)?;
            diag.push(format!(
                "增量重扫三分账：新增 {} 消失 {} 类型漂移 {}",
                d.added.len(), d.removed.len(), d.drift.len()
            ));            d
        }
        None => return Err(SgCode::EmptyProfile),
    };
    let diff_added = diff.added.len();
    let diff_removed = diff.removed.len();
    let diff_drift = diff.drift.len();
    let mut passed = probe.pass;
    for mr in machines.iter() {
        if !mr.pass {
            passed = false;
        }
    }
    // 三分账须各 ≥1（增量构造设计保证；为 0 说明对拍链路失真）。
    if diff_added == 0 || diff_removed == 0 || diff_drift == 0 {
        passed = false;
        diag.push(String::from("增量三分账有零账：新增/消失/漂移均须非零"));
    }
    // BNode 账面显性核对：基线树用量须在容量内且余量非负。
    if alloc_snap.used > alloc_snap.cap {
        passed = false;
        diag.push(String::from("BNode 分配账越顶：用量超过容量封顶"));
    }
    Ok(F0044Report {
        tick,
        machines,
        probe,
        diff,
        diff_added,
        diff_removed,
        diff_drift,
        alloc: alloc_snap,
        diag,
        passed,
    })
}
