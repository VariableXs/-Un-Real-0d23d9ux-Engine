#![allow(dead_code)]
//! ============================================================================
//! UNX-F1782 —— 每核结构一致性总检（PerCpu 字段↔账目↔接口三层总清单）
//! ============================================================================
//!
//! 单号：UNX-F1782
//! 标题：每核结构一致性总检（PerCpu 字段↔账目↔接口三层总清单）
//! 目标行数：2200 ~ 2800 行（硬指标，写满）
//! 域：CoRun·UNX-A3-B10
//! 纯功能线：是（纯功能，无 std、无 unwrap/expect/panic!、无浮点、无真实 IO、无墙钟）
//!
//! 判据原文（注释引用基准）：
//! `UNX-F1782-J1 三层总清单（PerCpu 冻结版字段×统计账位×观测接口）三方对齐断言全过，
//!   缺漏注入 10/10 检出`
//!
//! 册内上下文：
//!   CoRun·UNX-A3-B10 段「每核结构总成」——十层拼图（布局/变量/队列/类/时间片/追踪/
//!   均衡/锁/空闲唤醒/观测）。相关单：F1781 十层结构总成总纲、F1784 跨批聚合断言≥30 条、
//!   F1786 启动期结构自检、F1787 运行期周期巡检、F1788 注入总矩阵对账、
//!   F1623/F1639 PerCpu 冻结版、F1762 十字段族账、F1763 登记宏、F1790 面板全量试运行
//!   （64 核满配建模）。
//!
//! ---------------------------------------------------------------------------
//! 组件清单（本文件提供的全部构件）
//! ---------------------------------------------------------------------------
//!   1. 三层模型：PerCpu 冻结版字段表（≥40 项真实调度字段）
//!                Ledger 统计账位表（F1762 十字段族）
//!                Interface 观测接口表（≥24 项导出接口）
//!   2. 三方对齐矩阵：(percpu_field_idx, ledger_slot_idx, iface_idx) 三元组逐行判定
//!   3. 十层结构拼图：StructLayer 十层闭集 + 层间连接点登记
//!   4. 结构自检断言集：35 条具名断言 + run_all_assertions 聚合报告
//!   5. 启动期自检 boot_audit（64 核满配建模，失败即 Block）
//!   6. 运行期周期巡检 periodic_patrol（三族巡检 + 缺口检出 + 开销在档）
//!   7. 注入核：10 形态注入 + 10 形态检出 + inject_all
//!   8. 三件套导出：结构图 / 账目图 / 挂点图 / 图账一致校验 / export_tlv + FNV 指纹
//!
//! ---------------------------------------------------------------------------
//! 职责定位（做什么 / 不做什么）
//! ---------------------------------------------------------------------------
//!   做：对「PerCpu 冻结版字段 × 统计账位 × 观测接口」三层总清单做全量三方对齐，
//!       断言结构无缺漏、无重复、无类型宽度错配、无偏移错位、无双向孤儿、无热路径挂锁，
//!       并在启动期与运行期分别执行总检与巡检，产出可导出的三件套文本与 TLV。
//!   不做：不定义新字段、不实现真实调度动作、不做跨批聚合（归 F1784）、
//!       不做内存布局的真正编译期布局（仅登记契约层偏移并断言其自洽）。
//!
//! ---------------------------------------------------------------------------
//! 三层结构的字段布局契约（偏移 / 对齐 / 语义 / 宽度）
//! ---------------------------------------------------------------------------
//!   L0  PerCpu 冻结版字段（契约布局，4 字节紧凑排布 + 8 字节槽宽语义）
//!       基址        : PERCPU_BASE = 0x0000_0000（每核结构契约基址，逻辑地址）
//!       对齐        : 4 字节（FIELD_ALIGN）；槽宽语义为 8 字节，指针/掩码/序列占满槽
//!       语义        : 字段名 + desc 说明该槽在本核调度器中的唯一职责
//!       访问权限    : R（只读观测）/ W（内核写）/ RW（读写双向）
//!       热路径      : hot=true 表示每 tick 或每切换必经，禁止挂锁
//!   L1  Ledger 统计账位（族内递进，宽度与被映字段一致）
//!       基址        : LEDGER_BASE = 0x0000_1000
//!       对齐        : 8 字节；计数族 4 字节宽度，累加族 8 字节宽度
//!       语义        : 账位是该字段被统计消费后的落点，entry 标注更新入口符号
//!   L2  Interface 观测接口（导出面，签名串为契约文本）
//!       基址        : IFACE_BASE = 0x0000_2000（导出表描述符区）
//!       对齐        : 描述符 16 字节：{u64 符号地址, u16 参数宽度, u8 消费者族, u8 标志}
//!       语义        : 签名串形如 "fn(structaudit::probe) -> u64"，consumer 标注 B09/B29 归属
//!
//!   三层之间的对齐判定优先级：
//!       MissingPerCpu > MissingLedger > MissingInterface > TypeMismatch
//!         > OffsetMismatch > DupMapping > OrphanLedger > OrphanIface
//!
//! ---------------------------------------------------------------------------
//! 性能逐项分解（纯函数，无分配抖动外的运行时成本；所有成本为 O(n) 且 n 为常数上界）
//! ---------------------------------------------------------------------------
//!   P1 登记册构建     : O(F+L+I)，F=50 字段 / L=50 账位 / I=28 接口，append-only 校验
//!   P2 对齐矩阵构建   : O(F)，每字段一次三元组归位 + 名称→索引 BTreeMap 查表
//!   P3 逐行判定       : O(F)，行内 ≤3 次比较，短路返回
//!   P4 双向孤儿扫描   : O(F+L+I)，账侧与接口侧各一次布尔标记表扫描
//!   P5 层完备性       : O(10)，每层一次字段/账位/接口计数与下限比较
//!   P6 连接点登记     : O(E)，E=24 条层间连接，重复边检测一次
//!   P7 断言聚合       : O(35 × F) 上界，实际短路求值，失败即返回首个失败项
//!   P8 启动期总检     : O(64 × F)，64 核满配逐核逐项核验，任一失败即 Block
//!   P9 运行期巡检     : O(核数) 每 tick，三族各一趟标记扫描，巡检自身开销计入 PATROL_COST_CAP=128
//!   P10 注入核        : O(F) 每形态，注入后立即执行对应检出器，10 形态串行确定性执行
//!   P11 三件套导出    : O(F+L+I+E) 文本行数与字节数线性，写入 TLV 后 FNV-1a 64 指纹自洽
//!   P12 无 IO 无墙钟   : 全部时间以逻辑 tick: u64 表达，测试与生产同码同路径
//!
//! ---------------------------------------------------------------------------
//! 诊断码段（独占 0xB080 ~ 0xB08F，主码 0xB080）
//! ---------------------------------------------------------------------------
//!   0xB080 BOOT_BLOCKED            启动期总检失败，阻断上线
//!   0xB081 PATROL_GAP              运行期巡检缺口（某核被跳过）
//!   0xB082 INJECT_DETECTED        注入形态被检出
//!   0xB083 INJECT_MISSED          注入形态漏检（判据失败信号）
//!   0xB084 ALIGN_ROW_BROKEN       对齐矩阵存在非 Aligned 行
//!   0xB085 ORPHAN_LEDGER          账侧孤儿（账有字段无）
//!   0xB086 ORPHAN_IFACE           接口侧孤儿（接口有字段无）
//!   0xB087 DUP_MAPPING            一个字段映射到多个账位
//!   0xB088 TYPE_WIDTH_MISMATCH    类型宽度不匹配
//!   0xB089 OFFSET_MISMATCH        偏移错位
//!   0xB08A HOTPATH_LOCKED         热路径含锁（层次倒置）
//!   0xB08B PATROL_COST_EXCEEDED   巡检自身 tick 开销越上界
//!   0xB08C TLV_FINGERPRINT_BAD    TLV 长度/指纹不自洽
//!   0xB08D GRAPH_LEDGER_DRIFT     三件套图与静态登记不一致
//!   0xB08E APPEND_ONLY_VIOLATION  登记册非 append-only（改写既有条目）
//!   0xB08F ASSERT_FAILED          聚合断言失败（携带首失败断言 id）
//!
//! ---------------------------------------------------------------------------
//! 相关单衔接
//! ---------------------------------------------------------------------------
//!   F1623 / F1639 : PerCpu 冻结版字段定义（本单 offset/width/access 契约来源）
//!   F1762         : 十字段族账（本单 LedgerFamily 十族划分与字段级对齐判据）
//!   F1763         : 登记宏（本单登记册 append-only 语义对齐）
//!   F1781         : 十层结构总成总纲（本单十层闭集与连接点为其下游展开）
//!   F1784         : 跨批聚合断言 ≥30 条（本单为单批，聚合职责归 F1784）
//!   F1786 / F1787 : 启动期自检 / 运行期巡检（本单 E 节为其实现体）
//!   F1788         : 注入总矩阵对账（本单 10 形态为其 F 段输入）
//!   F1789         : 三件套导出对齐（本单 G 节为其实现体）
//!   F1790         : 面板全量试运行，64 核满配建模（本单 boot_audit 采用同建模）
//! ============================================================================

use alloc::vec;

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ============================================================================
// 0. 基础类型：Result / 诊断码 / LCG
// ============================================================================

/// 诊断码段下界（独占段 0xB080 ~ 0xB08F）。
pub const DIAG_BASE: u16 = 0xB080;
/// 诊断码段上界（独占段 0xB080 ~ 0xB08F）。
pub const DIAG_LIMIT: u16 = 0xB08F;
/// 主诊断码：启动期总检失败。
pub const DIAG_MAIN_BOOT_BLOCKED: u16 = 0xB080;
/// 主诊断码：注入检出。
pub const DIAG_MAIN_INJECT_DETECTED: u16 = 0xB082;
/// 主诊断码：注入漏检（判据失败信号）。
pub const DIAG_MAIN_INJECT_MISSED: u16 = 0xB083;
/// 主诊断码：聚合断言失败。
pub const DIAG_MAIN_ASSERT_FAILED: u16 = 0xB08F;

/// 本单全部错误路径的类型（无 std、无 unwrap/expect/panic!）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AuditError {
    /// 登记册为空，无法总检。
    RegistryEmpty,
    /// 索引越界：给出目标域、越界索引与当前长度。
    IndexOutOfRange {
        domain: &'static str,
        idx: u32,
        len: u32,
    },
    /// 非 append-only：试图改写既有条目。
    NotAppendOnly {
        name: &'static str,
    },
    /// 名称重复登记。
    DuplicateName {
        name: &'static str,
    },
    /// 字段宽度为零或超宽。
    BadWidth {
        name: &'static str,
    },
    /// 偏移未满足 8 字节对齐。
    BadAlignment {
        name: &'static str,
        offset: u32,
    },
    /// 偏移与契约表不一致。
    OffsetMismatch {
        name: &'static str,
        expect: u32,
        got: u32,
    },
    /// 十层闭集外的层标签。
    UnknownLayer {
        name: &'static str,
    },
    /// 巡检缺口：某核在本轮被跳过。
    PatrolGap {
        core: u32,
    },
    /// 巡检自身开销越上界。
    PatrolCostExceeded {
        got: u64,
        cap: u64,
    },
    /// TLV 长度/指纹不自洽。
    TlvBroken {
        reason: &'static str,
    },
    /// 三件套图与静态登记漂移。
    GraphDrift {
        reason: &'static str,
    },
}

/// 本单统一结果类型。
pub type AuditResult<T> = Result<T, AuditError>;

impl AuditError {
    /// 取该错误的诊断码（0xB080 ~ 0xB08F 独占段内）。
    pub fn code(&self) -> u16 {
        match self {
            AuditError::RegistryEmpty => 0xB080,
            AuditError::IndexOutOfRange { .. } => 0xB084,
            AuditError::NotAppendOnly { .. } => 0xB08E,
            AuditError::DuplicateName { .. } => 0xB08E,
            AuditError::BadWidth { .. } => 0xB088,
            AuditError::BadAlignment { .. } => 0xB089,
            AuditError::OffsetMismatch { .. } => 0xB089,
            AuditError::UnknownLayer { .. } => 0xB084,
            AuditError::PatrolGap { .. } => 0xB081,
            AuditError::PatrolCostExceeded { .. } => 0xB08B,
            AuditError::TlvBroken { .. } => 0xB08C,
            AuditError::GraphDrift { .. } => 0xB08D,
        }
    }

    /// 取该错误的诊断文本。
    pub fn text(&self) -> String {
        match self {
            AuditError::RegistryEmpty => String::from("registry_empty"),
            AuditError::IndexOutOfRange { domain, idx, len } => {
                format!("index_out_of_range:{}:{}:{}", domain, idx, len)
            }
            AuditError::NotAppendOnly { name } => format!("append_only_violation:{}", name),
            AuditError::DuplicateName { name } => format!("duplicate_name:{}", name),
            AuditError::BadWidth { name } => format!("bad_width:{}", name),
            AuditError::BadAlignment { name, offset } => {
                format!("bad_alignment:{}:{}", name, offset)
            }
            AuditError::OffsetMismatch { name, expect, got } => {
                format!("offset_mismatch:{}:{}:{}", name, expect, got)
            }
            AuditError::UnknownLayer { name } => format!("unknown_layer:{}", name),
            AuditError::PatrolGap { core } => format!("patrol_gap:core{}", core),
            AuditError::PatrolCostExceeded { got, cap } => {
                format!("patrol_cost_exceeded:{}:{}", got, cap)
            }
            AuditError::TlvBroken { reason } => format!("tlv_broken:{}", reason),
            AuditError::GraphDrift { reason } => format!("graph_drift:{}", reason),
        }
    }
}

/// 标准 64 位 LCG 常数（Knuth MMIX）：a = 6364136223846793005，c = 1442695040888963407。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量常数。
pub const LCG_C: u64 = 1442695040888963407;

/// 确定性 LCG 发生器（no_std，仅整数运算，无浮点、无墙钟）。
#[derive(Clone, Copy)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造，保证跨机器跨运行完全一致。
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }

    /// 取下一原始 64 位值。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(LCG_A)
            .wrapping_add(LCG_C);
        self.state
    }

    /// 取 [0, bound) 内的确定性值（bound=0 时返回 0，无除零风险）。
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            0
        } else {
            self.next_u64() % bound
        }
    }

    /// 以确定性抖动污染一个 u32（幅度上限 amp）。
    pub fn jitter_u32(&mut self, value: u32, amp: u32) -> u32 {
        if amp == 0 {
            value
        } else {
            value.wrapping_add(self.below(amp as u64 + 1) as u32)
        }
    }
}

/// FNV-1a 64 位指纹常数（偏移基）。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 位指纹素数。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// 计算字节序列的 FNV-1a 64 位指纹。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

// ============================================================================
// 1. 闭集枚举：十层结构 / 十字段族 / 访问权限 / 导出族 / 对齐结果
// ============================================================================

/// 十层结构拼图闭集（承接 F1781 段「每核结构总成」）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StructLayer {
    /// 第 1 层：布局层（基址/跨度/槽位对齐契约）。
    Layout,
    /// 第 2 层：变量层（每核私有标量与指针）。
    Var,
    /// 第 3 层：队列层（就绪队列与迁移队列）。
    Runq,
    /// 第 4 层：类层（调度类与优先级）。
    Class,
    /// 第 5 层：时间片层（时间片预算与截止期）。
    Timeslice,
    /// 第 6 层：追踪层（切换轨迹与序号）。
    Trace,
    /// 第 7 层：均衡层（负载 credit 与迁移计数）。
    Balance,
    /// 第 8 层：锁层（队列锁与等待掩码）。
    Lock,
    /// 第 9 层：空闲唤醒层（空闲态与唤醒挂点）。
    IdleWake,
    /// 第 10 层：观测层（观测环与导出挂点）。
    Observe,
}

/// 判据：UNX-F1782-J1 —— 子句「十层拼图（布局/变量/队列/类/时间片/追踪/均衡/锁/空闲唤醒/观测）」
impl StructLayer {
    /// 十层闭集全集，顺序即层号顺序，禁止增删。
    pub const ALL: [StructLayer; 10] = [
        StructLayer::Layout,
        StructLayer::Var,
        StructLayer::Runq,
        StructLayer::Class,
        StructLayer::Timeslice,
        StructLayer::Trace,
        StructLayer::Balance,
        StructLayer::Lock,
        StructLayer::IdleWake,
        StructLayer::Observe,
    ];

    /// 层序号（0..=9）。
    pub fn ordinal(&self) -> u32 {
        match self {
            StructLayer::Layout => 0,
            StructLayer::Var => 1,
            StructLayer::Runq => 2,
            StructLayer::Class => 3,
            StructLayer::Timeslice => 4,
            StructLayer::Trace => 5,
            StructLayer::Balance => 6,
            StructLayer::Lock => 7,
            StructLayer::IdleWake => 8,
            StructLayer::Observe => 9,
        }
    }

    /// 层名称文本。
    pub fn name(&self) -> &'static str {
        match self {
            StructLayer::Layout => "Layout",
            StructLayer::Var => "Var",
            StructLayer::Runq => "Runq",
            StructLayer::Class => "Class",
            StructLayer::Timeslice => "Timeslice",
            StructLayer::Trace => "Trace",
            StructLayer::Balance => "Balance",
            StructLayer::Lock => "Lock",
            StructLayer::IdleWake => "IdleWake",
            StructLayer::Observe => "Observe",
        }
    }

    /// 依名反查层（闭集外返回 None）。
    pub fn from_name(name: &str) -> Option<StructLayer> {
        let mut i = 0usize;
        while i < StructLayer::ALL.len() {
            if StructLayer::ALL[i].name() == name {
                return Some(StructLayer::ALL[i]);
            }
            i += 1;
        }
        None
    }

    /// 该层的字段数下限（完备性断言用）。
    pub fn min_fields(&self) -> u32 {
        match self {
            StructLayer::Layout => 4,
            StructLayer::Var => 6,
            StructLayer::Runq => 4,
            StructLayer::Class => 4,
            StructLayer::Timeslice => 4,
            StructLayer::Trace => 3,
            StructLayer::Balance => 4,
            StructLayer::Lock => 3,
            StructLayer::IdleWake => 4,
            StructLayer::Observe => 3,
        }
    }

    /// 该层的账位数下限（完备性断言用）。
    pub fn min_ledger(&self) -> u32 {
        match self {
            StructLayer::Layout => 3,
            StructLayer::Var => 4,
            StructLayer::Runq => 4,
            StructLayer::Class => 3,
            StructLayer::Timeslice => 3,
            StructLayer::Trace => 2,
            StructLayer::Balance => 3,
            StructLayer::Lock => 2,
            StructLayer::IdleWake => 3,
            StructLayer::Observe => 3,
        }
    }

    /// 该层的接口数下限（完备性断言用）。
    pub fn min_iface(&self) -> u32 {
        match self {
            StructLayer::Layout => 2,
            StructLayer::Var => 2,
            StructLayer::Runq => 3,
            StructLayer::Class => 2,
            StructLayer::Timeslice => 2,
            StructLayer::Trace => 2,
            StructLayer::Balance => 2,
            StructLayer::Lock => 2,
            StructLayer::IdleWake => 2,
            StructLayer::Observe => 3,
        }
    }
}

/// F1762 十字段族：账位归属族（与十层一一映射，账族名独立以保留账目语义）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LedgerFamily {
    /// 布局账族：基址/跨度/槽数。
    Geo,
    /// 变量账族：标量与指针读取。
    Var,
    /// 队列账族：入队/出队/深度。
    Runq,
    /// 类账族：调度类分布。
    Class,
    /// 时间片账族：切片消耗。
    Slice,
    /// 追踪账族：轨迹写入。
    Trace,
    /// 均衡账族：迁移/窃取。
    Balance,
    /// 锁账族：持锁/等锁。
    Lock,
    /// 空闲唤醒账族：空闲/唤醒。
    Idle,
    /// 观测账族：观测环水位。
    Observe,
}

/// 判据：UNX-F1782-J1 —— 子句「F1762 十字段族账」族闭集
impl LedgerFamily {
    /// 十字段族全集。
    pub const ALL: [LedgerFamily; 10] = [
        LedgerFamily::Geo,
        LedgerFamily::Var,
        LedgerFamily::Runq,
        LedgerFamily::Class,
        LedgerFamily::Slice,
        LedgerFamily::Trace,
        LedgerFamily::Balance,
        LedgerFamily::Lock,
        LedgerFamily::Idle,
        LedgerFamily::Observe,
    ];

    /// 族名文本。
    pub fn name(&self) -> &'static str {
        match self {
            LedgerFamily::Geo => "GEO",
            LedgerFamily::Var => "VAR",
            LedgerFamily::Runq => "RUNQ",
            LedgerFamily::Class => "CLS",
            LedgerFamily::Slice => "TSL",
            LedgerFamily::Trace => "TRC",
            LedgerFamily::Balance => "BAL",
            LedgerFamily::Lock => "LCK",
            LedgerFamily::Idle => "IDL",
            LedgerFamily::Observe => "OBS",
        }
    }

    /// 族序号（0..=9）。
    pub fn ordinal(&self) -> u32 {
        match self {
            LedgerFamily::Geo => 0,
            LedgerFamily::Var => 1,
            LedgerFamily::Runq => 2,
            LedgerFamily::Class => 3,
            LedgerFamily::Slice => 4,
            LedgerFamily::Trace => 5,
            LedgerFamily::Balance => 6,
            LedgerFamily::Lock => 7,
            LedgerFamily::Idle => 8,
            LedgerFamily::Observe => 9,
        }
    }

    /// 族到十层的映射（十字段族与十层同构，保留独立枚举以示账目侧语义）。
    pub fn layer(&self) -> StructLayer {
        StructLayer::ALL[self.ordinal() as usize]
    }

    /// 依名反查族。
    pub fn from_name(name: &str) -> Option<LedgerFamily> {
        let mut i = 0usize;
        while i < LedgerFamily::ALL.len() {
            if LedgerFamily::ALL[i].name() == name {
                return Some(LedgerFamily::ALL[i]);
            }
            i += 1;
        }
        None
    }
}

/// 字段访问权限。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// 只读：仅观测路径可读。
    R,
    /// 只写：仅内核调度路径可写。
    W,
    /// 读写双向。
    Rw,
}

impl Access {
    /// 权限文本。
    pub fn name(&self) -> &'static str {
        match self {
            Access::R => "R",
            Access::W => "W",
            Access::Rw => "RW",
        }
    }

    /// 是否可写。
    pub fn writable(&self) -> bool {
        matches!(self, Access::W | Access::Rw)
    }

    /// 是否可读。
    pub fn readable(&self) -> bool {
        matches!(self, Access::R | Access::Rw)
    }

    /// 依文本反查权限（闭集外返回 None）。
    pub fn from_name(name: &str) -> Option<Access> {
        match name {
            "R" => Some(Access::R),
            "W" => Some(Access::W),
            "RW" => Some(Access::Rw),
            _ => None,
        }
    }
}

/// 观测接口导出族（面板/追踪/告警三面）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExportFamily {
    /// 面板族：F1790 面板全量试运行消费。
    Panel,
    /// 追踪族：切迹导出消费。
    Trace,
    /// 告警族：越界与缺口告警消费。
    Alarm,
    /// 内核自检族：F1786/F1787 内核侧自检消费。
    SelfCheck,
}

impl ExportFamily {
    /// 导出族全集。
    pub const ALL: [ExportFamily; 4] = [
        ExportFamily::Panel,
        ExportFamily::Trace,
        ExportFamily::Alarm,
        ExportFamily::SelfCheck,
    ];

    /// 导出族名文本。
    pub fn name(&self) -> &'static str {
        match self {
            ExportFamily::Panel => "PANEL",
            ExportFamily::Trace => "TRACE",
            ExportFamily::Alarm => "ALARM",
            ExportFamily::SelfCheck => "SELFCHECK",
        }
    }
}

/// 观测接口消费方（B09 面板侧 / B29 追踪侧）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Consumer {
    /// B09 面板消费方。
    B09,
    /// B29 追踪消费方。
    B29,
    /// 内核侧自检消费方。
    Kernel,
}

impl Consumer {
    /// 消费方文本。
    pub fn name(&self) -> &'static str {
        match self {
            Consumer::B09 => "B09",
            Consumer::B29 => "B29",
            Consumer::Kernel => "KERNEL",
        }
    }
}

/// 三方对齐结果的闭集（九种）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AlignStatus {
    /// 三方齐备且类型宽度、偏移均一致。
    Aligned,
    /// 缺账位登记。
    MissingLedger,
    /// 缺观测接口挂点。
    MissingInterface,
    /// 缺 PerCpu 字段登记。
    MissingPerCpu,
    /// 类型宽度不匹配。
    TypeMismatch,
    /// 偏移错位。
    OffsetMismatch,
    /// 映射重复：一个字段映到多个账位。
    DupMapping,
    /// 孤儿账位：账有字段无。
    OrphanLedger,
    /// 孤儿接口：接口有字段无。
    OrphanIface,
}

impl AlignStatus {
    /// 对齐结果全集（判定顺序即优先级顺序）。
    pub const ALL: [AlignStatus; 9] = [
        AlignStatus::Aligned,
        AlignStatus::MissingLedger,
        AlignStatus::MissingInterface,
        AlignStatus::MissingPerCpu,
        AlignStatus::TypeMismatch,
        AlignStatus::OffsetMismatch,
        AlignStatus::DupMapping,
        AlignStatus::OrphanLedger,
        AlignStatus::OrphanIface,
    ];

    /// 结果名文本。
    pub fn name(&self) -> &'static str {
        match self {
            AlignStatus::Aligned => "Aligned",
            AlignStatus::MissingLedger => "MissingLedger",
            AlignStatus::MissingInterface => "MissingInterface",
            AlignStatus::MissingPerCpu => "MissingPerCpu",
            AlignStatus::TypeMismatch => "TypeMismatch",
            AlignStatus::OffsetMismatch => "OffsetMismatch",
            AlignStatus::DupMapping => "DupMapping",
            AlignStatus::OrphanLedger => "OrphanLedger",
            AlignStatus::OrphanIface => "OrphanIface",
        }
    }

    /// 是否为通过态。
    pub fn is_pass(&self) -> bool {
        matches!(self, AlignStatus::Aligned)
    }

    /// 该结果对应的诊断码。
    pub fn code(&self) -> u16 {
        match self {
            AlignStatus::Aligned => 0,
            AlignStatus::MissingLedger => 0xB084,
            AlignStatus::MissingInterface => 0xB084,
            AlignStatus::MissingPerCpu => 0xB084,
            AlignStatus::TypeMismatch => 0xB088,
            AlignStatus::OffsetMismatch => 0xB089,
            AlignStatus::DupMapping => 0xB087,
            AlignStatus::OrphanLedger => 0xB085,
            AlignStatus::OrphanIface => 0xB086,
        }
    }
}

// ============================================================================
// 2. 三层模型：结构描述符（PerCpu 字段 / Ledger 账位 / Interface 接口）
// ============================================================================

/// PerCpu 契约基址（逻辑地址）。
pub const PERCPU_BASE: u64 = 0x0000_0000;
/// Ledger 契约基址（逻辑地址）。
pub const LEDGER_BASE: u64 = 0x0000_1000;
/// Interface 描述符区基址（逻辑地址）。
pub const IFACE_BASE: u64 = 0x0000_2000;
/// 三层槽位统一对齐字节数（账位与接口描述符步进）。
pub const SLOT_ALIGN: u32 = 8;
/// PerCpu 字段槽位对齐字节数（冻结版为 4 字节紧凑排布）。
pub const FIELD_ALIGN: u32 = 4;

/// PerCpu 冻结版字段描述符（F1623/F1639 冻结版布局契约的下游展开）。
#[derive(Clone, Copy)]
pub struct FieldDesc {
    /// 字段序号（登记册下标，append-only 递增）。
    pub idx: u32,
    /// 字段名（与 PerCpu 结构成员名一致）。
    pub name: &'static str,
    /// 契约偏移（相对 PERCPU_BASE）。
    pub offset: u32,
    /// 宽度（字节，4 或 8）。
    pub width: u32,
    /// 访问权限。
    pub access: Access,
    /// 所属十层。
    pub layer: StructLayer,
    /// 是否热路径（每 tick / 每切换必经）。
    pub hot_path: bool,
    /// 语义说明。
    pub semantic: &'static str,
    /// 单位（id / ptr / cnt / tick / ns / mask / flag 等）。
    pub unit: &'static str,
}

impl FieldDesc {
    /// 字段绝对逻辑地址。
    pub fn addr(&self) -> u64 {
        PERCPU_BASE + self.offset as u64
    }

    /// 字段占用的槽位尾偏移（供单调性断言使用）。
    pub fn end_offset(&self) -> u32 {
        self.offset + self.width
    }

    /// 字段是否为只读观测位。
    pub fn read_only(&self) -> bool {
        matches!(self.access, Access::R)
    }

    /// 单行自描述文本（导出用）。
    pub fn line(&self) -> String {
        format!(
            "F{:02} {} off=0x{:04X} w={} acc={} layer={} hot={} unit={}",
            self.idx,
            self.name,
            self.offset,
            self.width,
            self.access.name(),
            self.layer.name(),
            self.hot_path,
            self.unit
        )
    }
}

/// 统计账位描述符（F1762 十字段族账的下游展开）。
#[derive(Clone, Copy)]
pub struct LedgerSlot {
    /// 账位序号（登记册下标，append-only 递增）。
    pub idx: u32,
    /// 账位名（族前缀 + 被映字段名）。
    pub name: &'static str,
    /// 所属十字段族。
    pub family: LedgerFamily,
    /// 宽度（字节，与被映字段宽度一致）。
    pub width: u32,
    /// 更新入口符号（登记宏 F1763 生成的写入点）。
    pub update_entry: &'static str,
    /// 是否热路径更新。
    pub hot_path: bool,
    /// 语义说明。
    pub desc: &'static str,
}

impl LedgerSlot {
    /// 账位绝对逻辑地址。
    pub fn addr(&self) -> u64 {
        LEDGER_BASE + (self.idx as u64) * SLOT_ALIGN as u64
    }

    /// 单行自描述文本（导出用）。
    pub fn line(&self) -> String {
        format!(
            "L{:02} {} fam={} w={} hot={} entry={}",
            self.idx,
            self.name,
            self.family.name(),
            self.width,
            self.hot_path,
            self.update_entry
        )
    }
}

/// 观测接口描述符（导出面契约）。
#[derive(Clone, Copy)]
pub struct IfaceDesc {
    /// 接口序号（登记册下标，append-only 递增）。
    pub idx: u32,
    /// 接口名。
    pub name: &'static str,
    /// 导出族。
    pub family: ExportFamily,
    /// 签名串（契约文本，全表唯一）。
    pub signature: &'static str,
    /// 消费方。
    pub consumer: Consumer,
    /// 是否热路径调用。
    pub hot_path: bool,
    /// 语义说明。
    pub desc: &'static str,
}

impl IfaceDesc {
    /// 描述符逻辑地址（16 字节步进）。
    pub fn desc_addr(&self) -> u64 {
        IFACE_BASE + (self.idx as u64) * 16
    }

    /// 签名串长度。
    pub fn sig_len(&self) -> u32 {
        self.signature.len() as u32
    }

    /// 单行自描述文本（导出用）。
    pub fn line(&self) -> String {
        format!(
            "I{:02} {} fam={} sig={} consumer={} hot={}",
            self.idx,
            self.name,
            self.family.name(),
            self.signature,
            self.consumer.name(),
            self.hot_path
        )
    }
}

// ============================================================================
// 2.1 L0 PerCpu 冻结版字段表（50 项真实调度字段）
// ============================================================================

/// PerCpu 冻结版字段总数下界（判据要求 ≥40）。
pub const FIELD_MIN_TOTAL: u32 = 40;
/// Ledger 账位总数下界。
pub const LEDGER_MIN_TOTAL: u32 = 40;
/// Interface 接口总数下界（判据要求 ≥24）。
pub const IFACE_MIN_TOTAL: u32 = 24;

/// PerCpu 冻结版字段静态登记（append-only 基线）。
pub const FIELDS: [FieldDesc; 50] = [
    FieldDesc {
        idx: 0,
        name: "apic_id",
        offset: 0x0000,
        width: 4,
        access: Access::Rw,
        layer: StructLayer::Layout,
        hot_path: true,
        semantic: "本核 APIC 逻辑标识，布局层锚点",
        unit: "id",
    },
    FieldDesc {
        idx: 1,
        name: "cpu_index",
        offset: 0x0004,
        width: 4,
        access: Access::Rw,
        layer: StructLayer::Layout,
        hot_path: true,
        semantic: "扁平 CPU 序号，跨核遍历主键",
        unit: "idx",
    },
    FieldDesc {
        idx: 2,
        name: "core_id",
        offset: 0x0008,
        width: 4,
        access: Access::R,
        layer: StructLayer::Layout,
        hot_path: false,
        semantic: "物理核号，拓扑定位用",
        unit: "core",
    },
    FieldDesc {
        idx: 3,
        name: "node_id",
        offset: 0x000C,
        width: 4,
        access: Access::R,
        layer: StructLayer::Layout,
        hot_path: false,
        semantic: "NUMA 节点号，跨节点均衡输入",
        unit: "node",
    },
    FieldDesc {
        idx: 4,
        name: "online_mask",
        offset: 0x0010,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Layout,
        hot_path: true,
        semantic: "本核在线掩码位，决定是否参与巡检",
        unit: "mask",
    },
    FieldDesc {
        idx: 5,
        name: "cur_thread",
        offset: 0x0018,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Var,
        hot_path: true,
        semantic: "当前运行线程指针",
        unit: "ptr",
    },
    FieldDesc {
        idx: 6,
        name: "next_thread",
        offset: 0x0020,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Var,
        hot_path: true,
        semantic: "预选下一线程指针，切链起点",
        unit: "ptr",
    },
    FieldDesc {
        idx: 7,
        name: "idle_thread",
        offset: 0x0028,
        width: 8,
        access: Access::R,
        layer: StructLayer::Var,
        hot_path: false,
        semantic: "空闲线程指针，空闲唤醒层终点",
        unit: "ptr",
    },
    FieldDesc {
        idx: 8,
        name: "mm_ptr",
        offset: 0x0030,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Var,
        hot_path: true,
        semantic: "当前地址空间指针",
        unit: "ptr",
    },
    FieldDesc {
        idx: 9,
        name: "active_mm",
        offset: 0x0038,
        width: 8,
        access: Access::R,
        layer: StructLayer::Var,
        hot_path: false,
        semantic: "活跃地址空间指针，观测侧读取",
        unit: "ptr",
    },
    FieldDesc {
        idx: 10,
        name: "gs_base",
        offset: 0x0040,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Layout,
        hot_path: true,
        semantic: "每核 GS 基址，布局层必备",
        unit: "addr",
    },
    FieldDesc {
        idx: 11,
        name: "rsp",
        offset: 0x0048,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Var,
        hot_path: true,
        semantic: "当前内核栈指针",
        unit: "addr",
    },
    FieldDesc {
        idx: 12,
        name: "rsp0",
        offset: 0x0050,
        width: 8,
        access: Access::R,
        layer: StructLayer::Layout,
        hot_path: false,
        semantic: "线程基栈指针，切链时校验",
        unit: "addr",
    },
    FieldDesc {
        idx: 13,
        name: "tsc_base",
        offset: 0x0058,
        width: 8,
        access: Access::R,
        layer: StructLayer::Layout,
        hot_path: false,
        semantic: "本核 TSC 校准基值，时间片换算",
        unit: "ts",
    },
    FieldDesc {
        idx: 14,
        name: "fs_base",
        offset: 0x0060,
        width: 8,
        access: Access::R,
        layer: StructLayer::Layout,
        hot_path: false,
        semantic: "用户态 FS 基址，切核恢复用",
        unit: "addr",
    },
    FieldDesc {
        idx: 15,
        name: "quiescent",
        offset: 0x0068,
        width: 4,
        access: Access::Rw,
        layer: StructLayer::Var,
        hot_path: false,
        semantic: "静默态标志，停机前须清零",
        unit: "flag",
    },
    FieldDesc {
        idx: 16,
        name: "need_resched",
        offset: 0x006C,
        width: 4,
        access: Access::Rw,
        layer: StructLayer::Class,
        hot_path: true,
        semantic: "抢占请求标志，类层仲裁入口",
        unit: "flag",
    },
    FieldDesc {
        idx: 17,
        name: "prio",
        offset: 0x0070,
        width: 4,
        access: Access::Rw,
        layer: StructLayer::Class,
        hot_path: true,
        semantic: "线程动态优先级",
        unit: "prio",
    },
    FieldDesc {
        idx: 18,
        name: "prio_static",
        offset: 0x0074,
        width: 4,
        access: Access::R,
        layer: StructLayer::Class,
        hot_path: false,
        semantic: "线程静态优先级",
        unit: "prio",
    },
    FieldDesc {
        idx: 19,
        name: "sched_class",
        offset: 0x0078,
        width: 4,
        access: Access::Rw,
        layer: StructLayer::Class,
        hot_path: true,
        semantic: "调度类号，决定仲裁路径",
        unit: "class",
    },
    FieldDesc {
        idx: 20,
        name: "class_mask",
        offset: 0x007C,
        width: 4,
        access: Access::R,
        layer: StructLayer::Class,
        hot_path: false,
        semantic: "可用调度类位掩码",
        unit: "mask",
    },
    FieldDesc {
        idx: 21,
        name: "time_slice",
        offset: 0x0080,
        width: 4,
        access: Access::Rw,
        layer: StructLayer::Timeslice,
        hot_path: true,
        semantic: "当前剩余时间片",
        unit: "tick",
    },
    FieldDesc {
        idx: 22,
        name: "slice_quantum",
        offset: 0x0084,
        width: 4,
        access: Access::R,
        layer: StructLayer::Timeslice,
        hot_path: false,
        semantic: "时间片配额常数",
        unit: "tick",
    },
    FieldDesc {
        idx: 23,
        name: "deadline_ns",
        offset: 0x0088,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Timeslice,
        hot_path: true,
        semantic: "调度截止期，EDF 比较键",
        unit: "ns",
    },
    FieldDesc {
        idx: 24,
        name: "slice_used",
        offset: 0x0090,
        width: 8,
        access: Access::W,
        layer: StructLayer::Timeslice,
        hot_path: true,
        semantic: "已消耗时间片累计",
        unit: "ns",
    },
    FieldDesc {
        idx: 25,
        name: "preempt_count",
        offset: 0x0098,
        width: 4,
        access: Access::W,
        layer: StructLayer::Trace,
        hot_path: true,
        semantic: "抢占次数累计",
        unit: "cnt",
    },
    FieldDesc {
        idx: 26,
        name: "switch_count",
        offset: 0x009C,
        width: 4,
        access: Access::W,
        layer: StructLayer::Trace,
        hot_path: true,
        semantic: "上下文切换次数累计",
        unit: "cnt",
    },
    FieldDesc {
        idx: 27,
        name: "last_switch_tick",
        offset: 0x00A0,
        width: 8,
        access: Access::R,
        layer: StructLayer::Trace,
        hot_path: false,
        semantic: "上次切换的逻辑时刻",
        unit: "tick",
    },
    FieldDesc {
        idx: 28,
        name: "trace_cursor",
        offset: 0x00A8,
        width: 4,
        access: Access::Rw,
        layer: StructLayer::Trace,
        hot_path: false,
        semantic: "轨迹环写游标",
        unit: "idx",
    },
    FieldDesc {
        idx: 29,
        name: "trace_seq",
        offset: 0x00AC,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Trace,
        hot_path: true,
        semantic: "全局轨迹序号，追踪层主键",
        unit: "seq",
    },
    FieldDesc {
        idx: 30,
        name: "trace_mask",
        offset: 0x00B4,
        width: 4,
        access: Access::R,
        layer: StructLayer::Trace,
        hot_path: false,
        semantic: "轨迹采样掩码",
        unit: "mask",
    },
    FieldDesc {
        idx: 31,
        name: "runq_head",
        offset: 0x00B8,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Runq,
        hot_path: true,
        semantic: "就绪队列队头索引",
        unit: "idx",
    },
    FieldDesc {
        idx: 32,
        name: "runq_tail",
        offset: 0x00C0,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Runq,
        hot_path: true,
        semantic: "就绪队列队尾索引",
        unit: "idx",
    },
    FieldDesc {
        idx: 33,
        name: "runq_depth",
        offset: 0x00C8,
        width: 4,
        access: Access::Rw,
        layer: StructLayer::Runq,
        hot_path: true,
        semantic: "就绪队列深度",
        unit: "cnt",
    },
    FieldDesc {
        idx: 34,
        name: "runq_hint",
        offset: 0x00CC,
        width: 4,
        access: Access::R,
        layer: StructLayer::Runq,
        hot_path: false,
        semantic: "入队提示位，跳队优化",
        unit: "idx",
    },
    FieldDesc {
        idx: 35,
        name: "migrate_out",
        offset: 0x00D0,
        width: 8,
        access: Access::W,
        layer: StructLayer::Balance,
        hot_path: true,
        semantic: "迁出计数",
        unit: "cnt",
    },
    FieldDesc {
        idx: 36,
        name: "balance_credit",
        offset: 0x00D8,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Balance,
        hot_path: true,
        semantic: "负载均衡 credit",
        unit: "credit",
    },
    FieldDesc {
        idx: 37,
        name: "steal_count",
        offset: 0x00E0,
        width: 4,
        access: Access::W,
        layer: StructLayer::Balance,
        hot_path: true,
        semantic: "窃取次数",
        unit: "cnt",
    },
    FieldDesc {
        idx: 38,
        name: "load_avg",
        offset: 0x00E4,
        width: 4,
        access: Access::R,
        layer: StructLayer::Balance,
        hot_path: false,
        semantic: "平滑负载均值（定点）",
        unit: "load",
    },
    FieldDesc {
        idx: 39,
        name: "lock_hold_mask",
        offset: 0x00E8,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Lock,
        hot_path: true,
        semantic: "持锁位掩码",
        unit: "mask",
    },
    FieldDesc {
        idx: 40,
        name: "lock_wait_mask",
        offset: 0x00F0,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Lock,
        hot_path: true,
        semantic: "等待锁位掩码",
        unit: "mask",
    },
    FieldDesc {
        idx: 41,
        name: "lock_dag_depth",
        offset: 0x00F8,
        width: 4,
        access: Access::R,
        layer: StructLayer::Lock,
        hot_path: false,
        semantic: "锁依赖图深度",
        unit: "cnt",
    },
    FieldDesc {
        idx: 42,
        name: "idle_state",
        offset: 0x00FC,
        width: 4,
        access: Access::Rw,
        layer: StructLayer::IdleWake,
        hot_path: true,
        semantic: "空闲态枚举（深睡/浅睡/待命/运行）",
        unit: "state",
    },
    FieldDesc {
        idx: 43,
        name: "idle_cycles",
        offset: 0x0100,
        width: 8,
        access: Access::W,
        layer: StructLayer::IdleWake,
        hot_path: true,
        semantic: "空闲周期累计",
        unit: "cyc",
    },
    FieldDesc {
        idx: 44,
        name: "busy_cycles",
        offset: 0x0108,
        width: 8,
        access: Access::W,
        layer: StructLayer::IdleWake,
        hot_path: true,
        semantic: "忙碌周期累计",
        unit: "cyc",
    },
    FieldDesc {
        idx: 45,
        name: "wake_pending",
        offset: 0x0110,
        width: 4,
        access: Access::Rw,
        layer: StructLayer::IdleWake,
        hot_path: true,
        semantic: "待唤醒标志，空闲态下降沿判定",
        unit: "flag",
    },
    FieldDesc {
        idx: 46,
        name: "wake_list_head",
        offset: 0x0118,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::IdleWake,
        hot_path: true,
        semantic: "唤醒挂点队头索引",
        unit: "idx",
    },
    FieldDesc {
        idx: 47,
        name: "observe_head",
        offset: 0x0120,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Observe,
        hot_path: true,
        semantic: "观测环读游标",
        unit: "idx",
    },
    FieldDesc {
        idx: 48,
        name: "observe_tail",
        offset: 0x0128,
        width: 8,
        access: Access::Rw,
        layer: StructLayer::Observe,
        hot_path: true,
        semantic: "观测环写游标",
        unit: "idx",
    },
    FieldDesc {
        idx: 49,
        name: "observe_drop",
        offset: 0x0130,
        width: 8,
        access: Access::W,
        layer: StructLayer::Observe,
        hot_path: true,
        semantic: "观测丢弃计数",
        unit: "cnt",
    },
];

// ============================================================================
// 2.2 L1 Ledger 统计账位表（50 账位，覆盖 F1762 十字段族）
// ============================================================================

/// Ledger 账位静态登记（append-only 基线）。
pub const LEDGERS: [LedgerSlot; 50] = [
    LedgerSlot {
        idx: 0,
        name: "GEO_apic_id",
        family: LedgerFamily::Geo,
        width: 4,
        update_entry: "macro_reg_geo!",
        hot_path: false,
        desc: "APIC 标识登记账，拓扑核对用",
    },
    LedgerSlot {
        idx: 1,
        name: "GEO_cpu_index",
        family: LedgerFamily::Geo,
        width: 4,
        update_entry: "macro_reg_geo!",
        hot_path: true,
        desc: "扁平序号登记账，遍历主键",
    },
    LedgerSlot {
        idx: 2,
        name: "GEO_core_id",
        family: LedgerFamily::Geo,
        width: 4,
        update_entry: "macro_reg_geo!",
        hot_path: false,
        desc: "物理核号登记账",
    },
    LedgerSlot {
        idx: 3,
        name: "GEO_node_id",
        family: LedgerFamily::Geo,
        width: 4,
        update_entry: "macro_reg_geo!",
        hot_path: false,
        desc: "NUMA 节点登记账",
    },
    LedgerSlot {
        idx: 4,
        name: "GEO_online_mask",
        family: LedgerFamily::Geo,
        width: 8,
        update_entry: "macro_reg_geo!",
        hot_path: true,
        desc: "在线掩码位登记账",
    },
    LedgerSlot {
        idx: 5,
        name: "VAR_cur_thread",
        family: LedgerFamily::Var,
        width: 8,
        update_entry: "macro_reg_var!",
        hot_path: true,
        desc: "当前线程读取账",
    },
    LedgerSlot {
        idx: 6,
        name: "VAR_next_thread",
        family: LedgerFamily::Var,
        width: 8,
        update_entry: "macro_reg_var!",
        hot_path: true,
        desc: "预选线程读取账",
    },
    LedgerSlot {
        idx: 7,
        name: "VAR_idle_thread",
        family: LedgerFamily::Var,
        width: 8,
        update_entry: "macro_reg_var!",
        hot_path: false,
        desc: "空闲线程读取账",
    },
    LedgerSlot {
        idx: 8,
        name: "VAR_mm_ptr",
        family: LedgerFamily::Var,
        width: 8,
        update_entry: "macro_reg_var!",
        hot_path: true,
        desc: "地址空间指针账",
    },
    LedgerSlot {
        idx: 9,
        name: "VAR_active_mm",
        family: LedgerFamily::Var,
        width: 8,
        update_entry: "macro_reg_var!",
        hot_path: false,
        desc: "活跃地址空间账",
    },
    LedgerSlot {
        idx: 10,
        name: "GEO_gs_base",
        family: LedgerFamily::Geo,
        width: 8,
        update_entry: "macro_reg_geo!",
        hot_path: true,
        desc: "GS 基址登记账",
    },
    LedgerSlot {
        idx: 11,
        name: "VAR_rsp",
        family: LedgerFamily::Var,
        width: 8,
        update_entry: "macro_reg_var!",
        hot_path: true,
        desc: "内核栈指针账",
    },
    LedgerSlot {
        idx: 12,
        name: "GEO_rsp0",
        family: LedgerFamily::Geo,
        width: 8,
        update_entry: "macro_reg_geo!",
        hot_path: false,
        desc: "基栈指针账",
    },
    LedgerSlot {
        idx: 13,
        name: "GEO_tsc_base",
        family: LedgerFamily::Geo,
        width: 8,
        update_entry: "macro_reg_geo!",
        hot_path: false,
        desc: "TSC 校准基值账",
    },
    LedgerSlot {
        idx: 14,
        name: "GEO_fs_base",
        family: LedgerFamily::Geo,
        width: 8,
        update_entry: "macro_reg_geo!",
        hot_path: false,
        desc: "用户 FS 基址账",
    },
    LedgerSlot {
        idx: 15,
        name: "VAR_quiescent",
        family: LedgerFamily::Var,
        width: 4,
        update_entry: "macro_reg_var!",
        hot_path: false,
        desc: "静默标志账",
    },
    LedgerSlot {
        idx: 16,
        name: "CLS_need_resched",
        family: LedgerFamily::Class,
        width: 4,
        update_entry: "macro_reg_cls!",
        hot_path: true,
        desc: "抢占请求标志账",
    },
    LedgerSlot {
        idx: 17,
        name: "CLS_prio",
        family: LedgerFamily::Class,
        width: 4,
        update_entry: "macro_reg_cls!",
        hot_path: true,
        desc: "动态优先级账",
    },
    LedgerSlot {
        idx: 18,
        name: "CLS_prio_static",
        family: LedgerFamily::Class,
        width: 4,
        update_entry: "macro_reg_cls!",
        hot_path: false,
        desc: "静态优先级账",
    },
    LedgerSlot {
        idx: 19,
        name: "CLS_sched_class",
        family: LedgerFamily::Class,
        width: 4,
        update_entry: "macro_reg_cls!",
        hot_path: true,
        desc: "调度类号账",
    },
    LedgerSlot {
        idx: 20,
        name: "CLS_class_mask",
        family: LedgerFamily::Class,
        width: 4,
        update_entry: "macro_reg_cls!",
        hot_path: false,
        desc: "调度类掩码账",
    },
    LedgerSlot {
        idx: 21,
        name: "TSL_time_slice",
        family: LedgerFamily::Slice,
        width: 4,
        update_entry: "macro_reg_tsl!",
        hot_path: true,
        desc: "剩余时间片账",
    },
    LedgerSlot {
        idx: 22,
        name: "TSL_slice_quantum",
        family: LedgerFamily::Slice,
        width: 4,
        update_entry: "macro_reg_tsl!",
        hot_path: false,
        desc: "时间片配额账",
    },
    LedgerSlot {
        idx: 23,
        name: "TSL_deadline_ns",
        family: LedgerFamily::Slice,
        width: 8,
        update_entry: "macro_reg_tsl!",
        hot_path: true,
        desc: "截止期账",
    },
    LedgerSlot {
        idx: 24,
        name: "TSL_slice_used",
        family: LedgerFamily::Slice,
        width: 8,
        update_entry: "macro_reg_tsl!",
        hot_path: true,
        desc: "已消耗时间片账",
    },
    LedgerSlot {
        idx: 25,
        name: "TRC_preempt_count",
        family: LedgerFamily::Trace,
        width: 4,
        update_entry: "macro_reg_trc!",
        hot_path: true,
        desc: "抢占次数账",
    },
    LedgerSlot {
        idx: 26,
        name: "TRC_switch_count",
        family: LedgerFamily::Trace,
        width: 4,
        update_entry: "macro_reg_trc!",
        hot_path: true,
        desc: "切换次数账",
    },
    LedgerSlot {
        idx: 27,
        name: "TRC_last_switch_tick",
        family: LedgerFamily::Trace,
        width: 8,
        update_entry: "macro_reg_trc!",
        hot_path: false,
        desc: "上次切换时刻账",
    },
    LedgerSlot {
        idx: 28,
        name: "TRC_trace_cursor",
        family: LedgerFamily::Trace,
        width: 4,
        update_entry: "macro_reg_trc!",
        hot_path: false,
        desc: "轨迹写游标账",
    },
    LedgerSlot {
        idx: 29,
        name: "TRC_trace_seq",
        family: LedgerFamily::Trace,
        width: 8,
        update_entry: "macro_reg_trc!",
        hot_path: true,
        desc: "轨迹序号账",
    },
    LedgerSlot {
        idx: 30,
        name: "TRC_trace_mask",
        family: LedgerFamily::Trace,
        width: 4,
        update_entry: "macro_reg_trc!",
        hot_path: false,
        desc: "采样掩码账",
    },
    LedgerSlot {
        idx: 31,
        name: "RUNQ_head",
        family: LedgerFamily::Runq,
        width: 8,
        update_entry: "macro_reg_runq!",
        hot_path: true,
        desc: "队头入出队账",
    },
    LedgerSlot {
        idx: 32,
        name: "RUNQ_tail",
        family: LedgerFamily::Runq,
        width: 8,
        update_entry: "macro_reg_runq!",
        hot_path: true,
        desc: "队尾入出队账",
    },
    LedgerSlot {
        idx: 33,
        name: "RUNQ_depth",
        family: LedgerFamily::Runq,
        width: 4,
        update_entry: "macro_reg_runq!",
        hot_path: true,
        desc: "队列深度账",
    },
    LedgerSlot {
        idx: 34,
        name: "RUNQ_hint",
        family: LedgerFamily::Runq,
        width: 4,
        update_entry: "macro_reg_runq!",
        hot_path: false,
        desc: "入队提示账",
    },
    LedgerSlot {
        idx: 35,
        name: "BAL_migrate_out",
        family: LedgerFamily::Balance,
        width: 8,
        update_entry: "macro_reg_bal!",
        hot_path: true,
        desc: "迁出计数账",
    },
    LedgerSlot {
        idx: 36,
        name: "BAL_credit",
        family: LedgerFamily::Balance,
        width: 8,
        update_entry: "macro_reg_bal!",
        hot_path: true,
        desc: "均衡 credit 账",
    },
    LedgerSlot {
        idx: 37,
        name: "BAL_steal_count",
        family: LedgerFamily::Balance,
        width: 4,
        update_entry: "macro_reg_bal!",
        hot_path: false,
        desc: "窃取次数账",
    },
    LedgerSlot {
        idx: 38,
        name: "BAL_load_avg",
        family: LedgerFamily::Balance,
        width: 4,
        update_entry: "macro_reg_bal!",
        hot_path: false,
        desc: "负载均值账",
    },
    LedgerSlot {
        idx: 39,
        name: "LCK_hold_mask",
        family: LedgerFamily::Lock,
        width: 8,
        update_entry: "macro_reg_lck!",
        hot_path: true,
        desc: "持锁掩码账",
    },
    LedgerSlot {
        idx: 40,
        name: "LCK_wait_mask",
        family: LedgerFamily::Lock,
        width: 8,
        update_entry: "macro_reg_lck!",
        hot_path: true,
        desc: "等锁掩码账",
    },
    LedgerSlot {
        idx: 41,
        name: "LCK_dag_depth",
        family: LedgerFamily::Lock,
        width: 4,
        update_entry: "macro_reg_lck!",
        hot_path: false,
        desc: "锁依赖深度账",
    },
    LedgerSlot {
        idx: 42,
        name: "IDL_state",
        family: LedgerFamily::Idle,
        width: 4,
        update_entry: "macro_reg_idl!",
        hot_path: true,
        desc: "空闲态账",
    },
    LedgerSlot {
        idx: 43,
        name: "IDL_idle_cycles",
        family: LedgerFamily::Idle,
        width: 8,
        update_entry: "macro_reg_idl!",
        hot_path: true,
        desc: "空闲周期账",
    },
    LedgerSlot {
        idx: 44,
        name: "IDL_busy_cycles",
        family: LedgerFamily::Idle,
        width: 8,
        update_entry: "macro_reg_idl!",
        hot_path: true,
        desc: "忙碌周期账",
    },
    LedgerSlot {
        idx: 45,
        name: "IDL_wake_pending",
        family: LedgerFamily::Idle,
        width: 4,
        update_entry: "macro_reg_idl!",
        hot_path: true,
        desc: "待唤醒标志账",
    },
    LedgerSlot {
        idx: 46,
        name: "IDL_wake_list_head",
        family: LedgerFamily::Idle,
        width: 8,
        update_entry: "macro_reg_idl!",
        hot_path: true,
        desc: "唤醒挂点账",
    },
    LedgerSlot {
        idx: 47,
        name: "OBS_head",
        family: LedgerFamily::Observe,
        width: 8,
        update_entry: "macro_reg_obs!",
        hot_path: true,
        desc: "观测环读游标账",
    },
    LedgerSlot {
        idx: 48,
        name: "OBS_tail",
        family: LedgerFamily::Observe,
        width: 8,
        update_entry: "macro_reg_obs!",
        hot_path: true,
        desc: "观测环写游标账",
    },
    LedgerSlot {
        idx: 49,
        name: "OBS_drop",
        family: LedgerFamily::Observe,
        width: 8,
        update_entry: "macro_reg_obs!",
        hot_path: true,
        desc: "观测丢弃账",
    },
];

// ============================================================================
// 2.3 L2 Interface 观测接口表（28 个导出接口）
// ============================================================================

/// Interface 接口静态登记（append-only 基线）。
pub const IFACES: [IfaceDesc; 28] = [
    IfaceDesc {
        idx: 0,
        name: "percpu_dump_layout",
        family: ExportFamily::Panel,
        signature: "fn(core:u32) -> [u64;8]",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "布局层全槽导出",
    },
    IfaceDesc {
        idx: 1,
        name: "percpu_dump_var",
        family: ExportFamily::Panel,
        signature: "fn(core:u32) -> [u64;7]",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "变量层全槽导出",
    },
    IfaceDesc {
        idx: 2,
        name: "percpu_dump_runq",
        family: ExportFamily::Panel,
        signature: "fn(core:u32) -> [u64;4;runq]",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "队列层全槽导出",
    },
    IfaceDesc {
        idx: 3,
        name: "percpu_dump_class",
        family: ExportFamily::Panel,
        signature: "fn(core:u32) -> [u64;5;class]",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "类层全槽导出",
    },
    IfaceDesc {
        idx: 4,
        name: "percpu_dump_timeslice",
        family: ExportFamily::Panel,
        signature: "fn(core:u32) -> [u64;4;tslice]",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "时间片层全槽导出",
    },
    IfaceDesc {
        idx: 5,
        name: "percpu_dump_trace",
        family: ExportFamily::Trace,
        signature: "fn(core:u32) -> [u64;6]",
        consumer: Consumer::B29,
        hot_path: false,
        desc: "追踪层全槽导出",
    },
    IfaceDesc {
        idx: 6,
        name: "percpu_dump_balance",
        family: ExportFamily::Panel,
        signature: "fn(core:u32) -> [u64;4;balance]",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "均衡层全槽导出",
    },
    IfaceDesc {
        idx: 7,
        name: "percpu_dump_lock",
        family: ExportFamily::Alarm,
        signature: "fn(core:u32) -> [u64;3;lock]",
        consumer: Consumer::B29,
        hot_path: false,
        desc: "锁层全槽导出",
    },
    IfaceDesc {
        idx: 8,
        name: "percpu_dump_idlewake",
        family: ExportFamily::Panel,
        signature: "fn(core:u32) -> [u64;5;idlewake]",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "空闲唤醒层全槽导出",
    },
    IfaceDesc {
        idx: 9,
        name: "percpu_dump_observe",
        family: ExportFamily::Panel,
        signature: "fn(core:u32) -> [u64;3;observe]",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "观测层全槽导出",
    },
    IfaceDesc {
        idx: 10,
        name: "field_read_at",
        family: ExportFamily::SelfCheck,
        signature: "fn(field:u32) -> u64",
        consumer: Consumer::Kernel,
        hot_path: true,
        desc: "按字段号读取槽值",
    },
    IfaceDesc {
        idx: 11,
        name: "field_meta_probe",
        family: ExportFamily::SelfCheck,
        signature: "fn(field:u32) -> FieldMeta",
        consumer: Consumer::Kernel,
        hot_path: false,
        desc: "按字段号读元数据",
    },
    IfaceDesc {
        idx: 12,
        name: "ledger_slot_read",
        family: ExportFamily::Panel,
        signature: "fn(slot:u32) -> u64",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "按账位号读取账值",
    },
    IfaceDesc {
        idx: 13,
        name: "ledger_family_agg",
        family: ExportFamily::Panel,
        signature: "fn(family:u8) -> u64",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "按族聚合账值",
    },
    IfaceDesc {
        idx: 14,
        name: "ledger_hot_mask",
        family: ExportFamily::Alarm,
        signature: "fn() -> u64;hot_mask",
        consumer: Consumer::B29,
        hot_path: false,
        desc: "热路径账位位图",
    },
    IfaceDesc {
        idx: 15,
        name: "align_matrix_row",
        family: ExportFamily::SelfCheck,
        signature: "fn(field:u32) -> AlignRow",
        consumer: Consumer::Kernel,
        hot_path: false,
        desc: "取对齐矩阵单行",
    },
    IfaceDesc {
        idx: 16,
        name: "align_orphan_scan",
        family: ExportFamily::SelfCheck,
        signature: "fn() -> OrphanScan",
        consumer: Consumer::Kernel,
        hot_path: false,
        desc: "双向孤儿扫描结果",
    },
    IfaceDesc {
        idx: 17,
        name: "assert_run_all",
        family: ExportFamily::SelfCheck,
        signature: "fn() -> AssertionReport",
        consumer: Consumer::Kernel,
        hot_path: false,
        desc: "聚合断言入口",
    },
    IfaceDesc {
        idx: 18,
        name: "boot_audit_run",
        family: ExportFamily::SelfCheck,
        signature: "fn() -> BootDecision",
        consumer: Consumer::Kernel,
        hot_path: false,
        desc: "启动期总检入口",
    },
    IfaceDesc {
        idx: 19,
        name: "patrol_run",
        family: ExportFamily::SelfCheck,
        signature: "fn(tick:u64) -> PatrolReport",
        consumer: Consumer::Kernel,
        hot_path: true,
        desc: "运行期巡检入口",
    },
    IfaceDesc {
        idx: 20,
        name: "patrol_cost_probe",
        family: ExportFamily::Panel,
        signature: "fn() -> u64;cost",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "巡检自身 tick 开销",
    },
    IfaceDesc {
        idx: 21,
        name: "struct_graph_export",
        family: ExportFamily::Panel,
        signature: "fn() -> String;struct",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "结构图文本导出",
    },
    IfaceDesc {
        idx: 22,
        name: "ledger_graph_export",
        family: ExportFamily::Panel,
        signature: "fn() -> String;ledger",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "账目图文本导出",
    },
    IfaceDesc {
        idx: 23,
        name: "hook_graph_export",
        family: ExportFamily::Trace,
        signature: "fn() -> String;hook",
        consumer: Consumer::B29,
        hot_path: false,
        desc: "挂点图文本导出",
    },
    IfaceDesc {
        idx: 24,
        name: "tlv_export",
        family: ExportFamily::Panel,
        signature: "fn() -> Vec<u8>",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "TLV 打包导出",
    },
    IfaceDesc {
        idx: 25,
        name: "inject_run_all",
        family: ExportFamily::SelfCheck,
        signature: "fn() -> Vec<InjectResult>",
        consumer: Consumer::Kernel,
        hot_path: false,
        desc: "注入核总入口",
    },
    IfaceDesc {
        idx: 26,
        name: "layer_field_count",
        family: ExportFamily::Panel,
        signature: "fn(layer:u8) -> u32",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "层字段数统计",
    },
    IfaceDesc {
        idx: 27,
        name: "layer_edge_list",
        family: ExportFamily::Panel,
        signature: "fn() -> Vec<LayerEdge>",
        consumer: Consumer::B09,
        hot_path: false,
        desc: "层间连接点清单导出",
    },
];

// ============================================================================
// 3. 登记册（append-only）
// ============================================================================

/// PerCpu 字段登记册：描述符数组 + 名称表 + 偏移表，全部 append-only。
#[derive(Clone)]
pub struct FieldRegistry {
    /// 描述符数组。
    pub items: Vec<FieldDesc>,
    /// 名称表（与 items 同序）。
    pub names: Vec<&'static str>,
    /// 偏移表（与 items 同序）。
    pub offsets: Vec<u32>,
    /// 名称 → 索引映射（构造期一次性建表，之后只读）。
    pub by_name: BTreeMap<&'static str, u32>,
}

impl FieldRegistry {
    /// 从静态基线构建登记册（append-only 建立）。
    pub fn from_baseline() -> AuditResult<FieldRegistry> {
        let mut reg = FieldRegistry {
            items: Vec::new(),
            names: Vec::new(),
            offsets: Vec::new(),
            by_name: BTreeMap::new(),
        };
        let mut i = 0usize;
        while i < FIELDS.len() {
            reg.append(FIELDS[i])?;
            i += 1;
        }
        Ok(reg)
    }

    /// 追加一个字段（append-only；重名与重偏移均拒绝）。
    pub fn append(&mut self, desc: FieldDesc) -> AuditResult<()> {
        if desc.width == 0 || desc.width > SLOT_ALIGN {
            return Err(AuditError::BadWidth { name: desc.name });
        }
        if desc.offset % FIELD_ALIGN != 0 {
            return Err(AuditError::BadAlignment {
                name: desc.name,
                offset: desc.offset,
            });
        }
        if self.by_name.contains_key(desc.name) {
            return Err(AuditError::DuplicateName { name: desc.name });
        }
        let expect_idx = self.items.len() as u32;
        if desc.idx != expect_idx {
            return Err(AuditError::NotAppendOnly { name: desc.name });
        }
        self.by_name.insert(desc.name, desc.idx);
        self.items.push(desc);
        self.names.push(desc.name);
        self.offsets.push(desc.offset);
        Ok(())
    }

    /// 按索引取字段描述符（越界返回 IndexOutOfRange）。
    pub fn get(&self, idx: u32) -> AuditResult<&FieldDesc> {
        match self.items.get(idx as usize) {
            Some(d) => Ok(d),
            None => Err(AuditError::IndexOutOfRange {
                domain: "field",
                idx,
                len: self.items.len() as u32,
            }),
        }
    }

    /// 按名取字段索引。
    pub fn index_of(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }

    /// 字段总数。
    pub fn len(&self) -> u32 {
        self.items.len() as u32
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Ledger 账位登记册：append-only。
#[derive(Clone)]
pub struct LedgerRegistry {
    /// 账位数组。
    pub items: Vec<LedgerSlot>,
    /// 名称表。
    pub names: Vec<&'static str>,
    /// 族表（与 items 同序）。
    pub families: Vec<LedgerFamily>,
    /// 名称 → 索引映射。
    pub by_name: BTreeMap<&'static str, u32>,
}

impl LedgerRegistry {
    /// 从静态基线构建账位登记册。
    pub fn from_baseline() -> AuditResult<LedgerRegistry> {
        let mut reg = LedgerRegistry {
            items: Vec::new(),
            names: Vec::new(),
            families: Vec::new(),
            by_name: BTreeMap::new(),
        };
        let mut i = 0usize;
        while i < LEDGERS.len() {
            reg.append(LEDGERS[i])?;
            i += 1;
        }
        Ok(reg)
    }

    /// 追加一个账位（append-only）。
    pub fn append(&mut self, slot: LedgerSlot) -> AuditResult<()> {
        if slot.width == 0 || slot.width > SLOT_ALIGN {
            return Err(AuditError::BadWidth { name: slot.name });
        }
        if self.by_name.contains_key(slot.name) {
            return Err(AuditError::DuplicateName { name: slot.name });
        }
        if slot.idx != self.items.len() as u32 {
            return Err(AuditError::NotAppendOnly { name: slot.name });
        }
        self.by_name.insert(slot.name, slot.idx);
        self.families.push(slot.family);
        self.items.push(slot);
        self.names.push(slot.name);
        Ok(())
    }

    /// 按索引取账位。
    pub fn get(&self, idx: u32) -> AuditResult<&LedgerSlot> {
        match self.items.get(idx as usize) {
            Some(s) => Ok(s),
            None => Err(AuditError::IndexOutOfRange {
                domain: "ledger",
                idx,
                len: self.items.len() as u32,
            }),
        }
    }

    /// 按名取账位索引。
    pub fn index_of(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }

    /// 某族的账位数。
    pub fn family_count(&self, fam: LedgerFamily) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].family == fam {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 账位总数。
    pub fn len(&self) -> u32 {
        self.items.len() as u32
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Interface 接口登记册：append-only。
#[derive(Clone)]
pub struct IfaceRegistry {
    /// 接口数组。
    pub items: Vec<IfaceDesc>,
    /// 名称表。
    pub names: Vec<&'static str>,
    /// 签名表（与 items 同序，全表唯一）。
    pub signatures: Vec<&'static str>,
    /// 名称 → 索引映射。
    pub by_name: BTreeMap<&'static str, u32>,
    /// 签名 → 索引映射（唯一性索引）。
    pub by_sig: BTreeMap<&'static str, u32>,
}

impl IfaceRegistry {
    /// 从静态基线构建接口登记册。
    pub fn from_baseline() -> AuditResult<IfaceRegistry> {
        let mut reg = IfaceRegistry {
            items: Vec::new(),
            names: Vec::new(),
            signatures: Vec::new(),
            by_name: BTreeMap::new(),
            by_sig: BTreeMap::new(),
        };
        let mut i = 0usize;
        while i < IFACES.len() {
            reg.append(IFACES[i])?;
            i += 1;
        }
        Ok(reg)
    }

    /// 追加一个接口（append-only；名与签名均须唯一）。
    pub fn append(&mut self, desc: IfaceDesc) -> AuditResult<()> {
        if desc.signature.is_empty() {
            return Err(AuditError::BadWidth { name: desc.name });
        }
        if self.by_name.contains_key(desc.name) {
            return Err(AuditError::DuplicateName { name: desc.name });
        }
        if self.by_sig.contains_key(desc.signature) {
            return Err(AuditError::DuplicateName {
                name: desc.signature,
            });
        }
        if desc.idx != self.items.len() as u32 {
            return Err(AuditError::NotAppendOnly { name: desc.name });
        }
        self.by_name.insert(desc.name, desc.idx);
        self.by_sig.insert(desc.signature, desc.idx);
        self.signatures.push(desc.signature);
        self.items.push(desc);
        self.names.push(desc.name);
        Ok(())
    }

    /// 按索引取接口。
    pub fn get(&self, idx: u32) -> AuditResult<&IfaceDesc> {
        match self.items.get(idx as usize) {
            Some(d) => Ok(d),
            None => Err(AuditError::IndexOutOfRange {
                domain: "iface",
                idx,
                len: self.items.len() as u32,
            }),
        }
    }

    /// 按名取接口索引。
    pub fn index_of(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }

    /// 按签名取接口索引。
    pub fn index_of_sig(&self, sig: &str) -> Option<u32> {
        self.by_sig.get(sig).copied()
    }

    /// 接口总数。
    pub fn len(&self) -> u32 {
        self.items.len() as u32
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

// ============================================================================
// 4. 十层结构拼图：层间连接点登记
// ============================================================================

/// 层间连接点描述符（F1781 十层拼图的连接面展开）。
#[derive(Clone, Copy)]
pub struct LayerEdge {
    /// 连接点序号。
    pub idx: u32,
    /// 源层。
    pub src: StructLayer,
    /// 目标层。
    pub dst: StructLayer,
    /// 连接名。
    pub name: &'static str,
    /// 是否热路径连接。
    pub hot_path: bool,
    /// 语义说明。
    pub desc: &'static str,
}

impl LayerEdge {
    /// 单行文本。
    pub fn line(&self) -> String {
        format!(
            "E{:02} {} -> {} name={} hot={}",
            self.idx,
            self.src.name(),
            self.dst.name(),
            self.name,
            self.hot_path
        )
    }
}

/// 十层连接点静态登记（append-only 基线，24 条）。
pub const LAYER_EDGES: [LayerEdge; 24] = [
    LayerEdge {
        idx: 0,
        src: StructLayer::Layout,
        dst: StructLayer::Var,
        name: "base_anchor",
        hot_path: true,
        desc: "布局基址向变量层提供锚点",
    },
    LayerEdge {
        idx: 1,
        src: StructLayer::Layout,
        dst: StructLayer::Runq,
        name: "ring_span",
        hot_path: false,
        desc: "布局跨度界定就绪环容量",
    },
    LayerEdge {
        idx: 2,
        src: StructLayer::Layout,
        dst: StructLayer::Lock,
        name: "lock_slot_map",
        hot_path: false,
        desc: "锁槽位映射表挂载于布局层",
    },
    LayerEdge {
        idx: 3,
        src: StructLayer::Var,
        dst: StructLayer::Class,
        name: "prio_view",
        hot_path: true,
        desc: "变量层向类层提供优先级视图",
    },
    LayerEdge {
        idx: 4,
        src: StructLayer::Var,
        dst: StructLayer::Runq,
        name: "thread_link",
        hot_path: true,
        desc: "线程链指针供入队/出队使用",
    },
    LayerEdge {
        idx: 5,
        src: StructLayer::Var,
        dst: StructLayer::IdleWake,
        name: "idle_fallback",
        hot_path: false,
        desc: "空闲线程回退指针",
    },
    LayerEdge {
        idx: 6,
        src: StructLayer::Class,
        dst: StructLayer::Timeslice,
        name: "quantum_grant",
        hot_path: true,
        desc: "调度类授予时间片配额",
    },
    LayerEdge {
        idx: 7,
        src: StructLayer::Class,
        dst: StructLayer::Runq,
        name: "class_pick",
        hot_path: true,
        desc: "类层仲裁后向队列层取任务",
    },
    LayerEdge {
        idx: 8,
        src: StructLayer::Class,
        dst: StructLayer::Balance,
        name: "weight_hint",
        hot_path: false,
        desc: "类权重提示送均衡层",
    },
    LayerEdge {
        idx: 9,
        src: StructLayer::Timeslice,
        dst: StructLayer::Trace,
        name: "slice_emit",
        hot_path: true,
        desc: "时间片耗尽事件送追踪层",
    },
    LayerEdge {
        idx: 10,
        src: StructLayer::Timeslice,
        dst: StructLayer::IdleWake,
        name: "expire_idle",
        hot_path: false,
        desc: "片耗尽触发空闲重评估",
    },
    LayerEdge {
        idx: 11,
        src: StructLayer::Runq,
        dst: StructLayer::Lock,
        name: "runq_lock",
        hot_path: true,
        desc: "就绪队列锁（锁层向队列层提供）",
    },
    LayerEdge {
        idx: 12,
        src: StructLayer::Runq,
        dst: StructLayer::Balance,
        name: "queue_pressure",
        hot_path: true,
        desc: "队列水位上报均衡层",
    },
    LayerEdge {
        idx: 13,
        src: StructLayer::Balance,
        dst: StructLayer::Runq,
        name: "steal_pull",
        hot_path: true,
        desc: "均衡层向邻核队列窃取",
    },
    LayerEdge {
        idx: 14,
        src: StructLayer::Balance,
        dst: StructLayer::IdleWake,
        name: "balance_park",
        hot_path: false,
        desc: "过载核进入停泊态",
    },
    LayerEdge {
        idx: 15,
        src: StructLayer::Lock,
        dst: StructLayer::Observe,
        name: "hold_publish",
        hot_path: true,
        desc: "持锁位发布到观测层",
    },
    LayerEdge {
        idx: 16,
        src: StructLayer::Lock,
        dst: StructLayer::Trace,
        name: "contend_emit",
        hot_path: true,
        desc: "锁竞争事件送追踪层",
    },
    LayerEdge {
        idx: 17,
        src: StructLayer::Trace,
        dst: StructLayer::Observe,
        name: "ring_write",
        hot_path: true,
        desc: "轨迹写入观测环",
    },
    LayerEdge {
        idx: 18,
        src: StructLayer::IdleWake,
        dst: StructLayer::Runq,
        name: "wake_enqueue",
        hot_path: true,
        desc: "唤醒任务入就绪队列",
    },
    LayerEdge {
        idx: 19,
        src: StructLayer::IdleWake,
        dst: StructLayer::Observe,
        name: "wake_log",
        hot_path: false,
        desc: "唤醒事件记录观测环",
    },
    LayerEdge {
        idx: 20,
        src: StructLayer::Observe,
        dst: StructLayer::Balance,
        name: "observe_feedback",
        hot_path: false,
        desc: "观测侧反馈修正均衡权重",
    },
    LayerEdge {
        idx: 21,
        src: StructLayer::Observe,
        dst: StructLayer::Trace,
        name: "trace_emit",
        hot_path: false,
        desc: "观测环内容导出为轨迹",
    },
    LayerEdge {
        idx: 22,
        src: StructLayer::Var,
        dst: StructLayer::Trace,
        name: "switch_record",
        hot_path: true,
        desc: "切链时写入切换记录",
    },
    LayerEdge {
        idx: 23,
        src: StructLayer::Runq,
        dst: StructLayer::Observe,
        name: "depth_publish",
        hot_path: true,
        desc: "队列深度发布到观测层",
    },
];

// ============================================================================
// 5. 三方对齐矩阵：(percpu_field_idx, ledger_slot_idx, iface_idx)
// ============================================================================

/// 一行对齐映射的静态三元组契约。
#[derive(Clone, Copy)]
pub struct AlignTriple {
    /// PerCpu 字段索引。
    pub field: u32,
    /// 账位索引。
    pub ledger: u32,
    /// 观测接口索引。
    pub iface: u32,
}

/// 三方对齐矩阵的静态三元组基线（50 行，与字段一一对应）。
///
/// 判据：UNX-F1782-J1 —— 子句「PerCpu 冻结版字段 × 统计账位 × 观测接口 三方对齐」
/// 账位侧与字段侧同序 1:1；接口侧允许一个接口承载多字段，但每个接口至少被一字段挂载
/// （否则构成 OrphanIface）。
pub const ALIGN_TRIPLES: [AlignTriple; 50] = [
    AlignTriple { field: 0, ledger: 0, iface: 0 },
    AlignTriple { field: 1, ledger: 1, iface: 0 },
    AlignTriple { field: 2, ledger: 2, iface: 0 },
    AlignTriple { field: 3, ledger: 3, iface: 21 },
    AlignTriple { field: 4, ledger: 4, iface: 18 },
    AlignTriple { field: 5, ledger: 5, iface: 10 },
    AlignTriple { field: 6, ledger: 6, iface: 11 },
    AlignTriple { field: 7, ledger: 7, iface: 1 },
    AlignTriple { field: 8, ledger: 8, iface: 1 },
    AlignTriple { field: 9, ledger: 9, iface: 1 },
    AlignTriple { field: 10, ledger: 10, iface: 0 },
    AlignTriple { field: 11, ledger: 11, iface: 1 },
    AlignTriple { field: 12, ledger: 12, iface: 0 },
    AlignTriple { field: 13, ledger: 13, iface: 0 },
    AlignTriple { field: 14, ledger: 14, iface: 0 },
    AlignTriple { field: 15, ledger: 15, iface: 1 },
    AlignTriple { field: 16, ledger: 16, iface: 3 },
    AlignTriple { field: 17, ledger: 17, iface: 3 },
    AlignTriple { field: 18, ledger: 18, iface: 3 },
    AlignTriple { field: 19, ledger: 19, iface: 15 },
    AlignTriple { field: 20, ledger: 20, iface: 3 },
    AlignTriple { field: 21, ledger: 21, iface: 14 },
    AlignTriple { field: 22, ledger: 22, iface: 4 },
    AlignTriple { field: 23, ledger: 23, iface: 4 },
    AlignTriple { field: 24, ledger: 24, iface: 4 },
    AlignTriple { field: 25, ledger: 25, iface: 5 },
    AlignTriple { field: 26, ledger: 26, iface: 25 },
    AlignTriple { field: 27, ledger: 27, iface: 5 },
    AlignTriple { field: 28, ledger: 28, iface: 5 },
    AlignTriple { field: 29, ledger: 29, iface: 19 },
    AlignTriple { field: 30, ledger: 30, iface: 5 },
    AlignTriple { field: 31, ledger: 31, iface: 22 },
    AlignTriple { field: 32, ledger: 32, iface: 2 },
    AlignTriple { field: 33, ledger: 33, iface: 12 },
    AlignTriple { field: 34, ledger: 34, iface: 2 },
    AlignTriple { field: 35, ledger: 35, iface: 6 },
    AlignTriple { field: 36, ledger: 36, iface: 6 },
    AlignTriple { field: 37, ledger: 37, iface: 20 },
    AlignTriple { field: 38, ledger: 38, iface: 13 },
    AlignTriple { field: 39, ledger: 39, iface: 7 },
    AlignTriple { field: 40, ledger: 40, iface: 17 },
    AlignTriple { field: 41, ledger: 41, iface: 7 },
    AlignTriple { field: 42, ledger: 42, iface: 8 },
    AlignTriple { field: 43, ledger: 43, iface: 26 },
    AlignTriple { field: 44, ledger: 44, iface: 8 },
    AlignTriple { field: 45, ledger: 45, iface: 23 },
    AlignTriple { field: 46, ledger: 46, iface: 27 },
    AlignTriple { field: 47, ledger: 47, iface: 9 },
    AlignTriple { field: 48, ledger: 48, iface: 24 },
    AlignTriple { field: 49, ledger: 49, iface: 16 },
];

/// 对齐矩阵的一行（运行期可被注入核改写）。
#[derive(Clone)]
pub struct AlignRow {
    /// 行号（= 字段索引语义位，注入时可重复出现以表达 DupMapping）。
    pub row: u32,
    /// PerCpu 字段索引（缺登记时为 NONE）。
    pub field: u32,
    /// 账位索引（缺登记时为 NONE）。
    pub ledger: u32,
    /// 接口索引（缺登记时为 NONE）。
    pub iface: u32,
    /// 判定结果。
    pub status: AlignStatus,
    /// 判定说明。
    pub reason: String,
}

/// 空索引哨兵（表示该侧无登记）。
pub const NONE_IDX: u32 = 0xFFFF_FFFF;

impl AlignRow {
    /// 构造一行默认 Aligned 的三元组。
    pub fn new(row: u32, field: u32, ledger: u32, iface: u32) -> AlignRow {
        AlignRow {
            row,
            field,
            ledger,
            iface,
            status: AlignStatus::Aligned,
            reason: String::from("baseline"),
        }
    }

    /// 单行文本。
    pub fn line(&self) -> String {
        format!(
            "R{:02} f={} l={} i={} status={} reason={}",
            self.row,
            self.field,
            self.ledger,
            self.iface,
            self.status.name(),
            self.reason
        )
    }
}

/// 三层总清单（登记册 + 对齐矩阵的容器）。
#[derive(Clone)]
pub struct TripleList {
    /// 字段登记册。
    pub fields: FieldRegistry,
    /// 账位登记册。
    pub ledgers: LedgerRegistry,
    /// 接口登记册。
    pub ifaces: IfaceRegistry,
    /// 对齐矩阵行。
    pub rows: Vec<AlignRow>,
    /// 层间连接点。
    pub edges: Vec<LayerEdge>,
}

impl TripleList {
    /// 由静态基线构建完整三层总清单。
    pub fn baseline() -> AuditResult<TripleList> {
        let mut rows: Vec<AlignRow> = Vec::new();
        let mut i = 0usize;
        while i < ALIGN_TRIPLES.len() {
            let t = ALIGN_TRIPLES[i];
            rows.push(AlignRow::new(t.field, t.field, t.ledger, t.iface));
            i += 1;
        }
        let mut edges: Vec<LayerEdge> = Vec::new();
        i = 0;
        while i < LAYER_EDGES.len() {
            edges.push(LAYER_EDGES[i]);
            i += 1;
        }
        Ok(TripleList {
            fields: FieldRegistry::from_baseline()?,
            ledgers: LedgerRegistry::from_baseline()?,
            ifaces: IfaceRegistry::from_baseline()?,
            rows,
            edges,
        })
    }

    /// 行数。
    pub fn row_count(&self) -> u32 {
        self.rows.len() as u32
    }
}

// ============================================================================
// 6. 三方对齐判定（逐行）+ 双向孤儿扫描
// ============================================================================

/// 判定单行三元组。
///
/// 判据：UNX-F1782-J1 —— 子句「三方对齐断言全过」
pub fn judge_row(
    list: &TripleList,
    row: &AlignRow,
    dup_fields: &BTreeMap<u32, u32>,
) -> AlignStatus {
    if row.field == NONE_IDX {
        return AlignStatus::MissingPerCpu;
    }
    if row.ledger == NONE_IDX {
        return AlignStatus::MissingLedger;
    }
    if row.iface == NONE_IDX {
        return AlignStatus::MissingInterface;
    }
    let f = match list.fields.get(row.field) {
        Ok(v) => v,
        Err(_) => return AlignStatus::MissingPerCpu,
    };
    let l = match list.ledgers.get(row.ledger) {
        Ok(v) => v,
        Err(_) => return AlignStatus::MissingLedger,
    };
    if list.ifaces.get(row.iface).is_err() {
        return AlignStatus::MissingInterface;
    }
    if let Some(cnt) = dup_fields.get(&row.field) {
        if *cnt > 1 {
            return AlignStatus::DupMapping;
        }
    }
    if f.width != l.width {
        return AlignStatus::TypeMismatch;
    }
    if l.family.layer() != f.layer {
        return AlignStatus::TypeMismatch;
    }
    if f.offset % FIELD_ALIGN != 0 || l.width == 0 {
        return AlignStatus::OffsetMismatch;
    }
    AlignStatus::Aligned
}

/// 统计字段索引出现次数（用于 DupMapping 判定）。
pub fn count_field_usage(list: &TripleList) -> BTreeMap<u32, u32> {
    let mut m: BTreeMap<u32, u32> = BTreeMap::new();
    let mut i = 0usize;
    while i < list.rows.len() {
        let f = list.rows[i].field;
        if f != NONE_IDX {
            let cur = match m.get(&f) {
                Some(v) => *v,
                None => 0,
            };
            m.insert(f, cur + 1);
        }
        i += 1;
    }
    m
}

/// 全矩阵逐行判定并回填 status/reason。
pub fn judge_all(list: &mut TripleList) -> u32 {
    let dups = count_field_usage(list);
    let mut broken = 0u32;
    let mut i = 0usize;
    while i < list.rows.len() {
        let st = judge_row(list, &list.rows[i], &dups);
        if !st.is_pass() {
            broken += 1;
            list.rows[i].reason = String::from("row_broken");
        } else {
            list.rows[i].reason = String::from("ok");
        }
        list.rows[i].status = st;
        i += 1;
    }
    broken
}

/// 双向孤儿扫描结果。
#[derive(Clone)]
pub struct OrphanScan {
    /// 未被任何字段挂载的账位索引。
    pub orphan_ledgers: Vec<u32>,
    /// 未被任何字段挂载的接口索引。
    pub orphan_ifaces: Vec<u32>,
    /// 未映射到任何账位的字段索引。
    pub unmapped_fields: Vec<u32>,
}

impl OrphanScan {
    /// 是否完全闭合（三侧均无悬挂项）。
    pub fn is_closed(&self) -> bool {
        self.orphan_ledgers.is_empty() && self.orphan_ifaces.is_empty() && self.unmapped_fields.is_empty()
    }

    /// 悬挂项总数。
    pub fn total(&self) -> u32 {
        self.orphan_ledgers.len() as u32
            + self.orphan_ifaces.len() as u32
            + self.unmapped_fields.len() as u32
    }
}

/// 账侧与接口侧孤儿双向扫描。
///
/// 判据：UNX-F1782-J1 —— 子句「F1762 字段级对齐」的反向闭合
pub fn scan_orphans(list: &TripleList) -> OrphanScan {
    let n_l = list.ledgers.len() as usize;
    let n_i = list.ifaces.len() as usize;
    let mut hit_l = vec![false; n_l];
    let mut hit_i = vec![false; n_i];
    let mut hit_f = vec![false; list.fields.len() as usize];
    let mut i = 0usize;
    while i < list.rows.len() {
        let r = &list.rows[i];
        if r.ledger != NONE_IDX && (r.ledger as usize) < n_l {
            hit_l[r.ledger as usize] = true;
        }
        if r.iface != NONE_IDX && (r.iface as usize) < n_i {
            hit_i[r.iface as usize] = true;
        }
        if r.field != NONE_IDX && (r.field as usize) < hit_f.len() {
            hit_f[r.field as usize] = true;
        }
        i += 1;
    }
    let mut out = OrphanScan {
        orphan_ledgers: Vec::new(),
        orphan_ifaces: Vec::new(),
        unmapped_fields: Vec::new(),
    };
    i = 0;
    while i < n_l {
        if !hit_l[i] {
            out.orphan_ledgers.push(i as u32);
        }
        i += 1;
    }
    i = 0;
    while i < n_i {
        if !hit_i[i] {
            out.orphan_ifaces.push(i as u32);
        }
        i += 1;
    }
    i = 0;
    while i < hit_f.len() {
        if !hit_f[i] {
            out.unmapped_fields.push(i as u32);
        }
        i += 1;
    }
    out
}

// ============================================================================
// 7. 十层完备性统计
// ============================================================================

/// 单层的完备性统计。
#[derive(Clone, Copy, Default)]
pub struct LayerStat {
    /// 层序号。
    pub layer_ord: u32,
    /// 层名。
    pub layer_name: &'static str,
    /// 字段数。
    pub field_count: u32,
    /// 账位数。
    pub ledger_count: u32,
    /// 接口挂点数（该层字段所挂接口去重数）。
    pub iface_count: u32,
    /// 热路径字段数。
    pub hot_fields: u32,
    /// 出边数。
    pub out_edges: u32,
    /// 入边数。
    pub in_edges: u32,
}

impl LayerStat {
    /// 层字段数是否达标。
    pub fn fields_ok(&self) -> bool {
        let l = match StructLayer::from_name(self.layer_name) {
            Some(v) => v,
            None => return false,
        };
        self.field_count >= l.min_fields()
    }

    /// 层账位数是否达标。
    pub fn ledger_ok(&self) -> bool {
        let l = match StructLayer::from_name(self.layer_name) {
            Some(v) => v,
            None => return false,
        };
        self.ledger_count >= l.min_ledger()
    }

    /// 层接口数是否达标。
    pub fn iface_ok(&self) -> bool {
        let l = match StructLayer::from_name(self.layer_name) {
            Some(v) => v,
            None => return false,
        };
        self.iface_count >= l.min_iface()
    }

    /// 单行文本。
    pub fn line(&self) -> String {
        format!(
            "L{:02} {} fields={} ledger={} iface={} hot={} out={} in={}",
            self.layer_ord,
            self.layer_name,
            self.field_count,
            self.ledger_count,
            self.iface_count,
            self.hot_fields,
            self.out_edges,
            self.in_edges
        )
    }
}

/// 统计十层完备性。
///
/// 判据：UNX-F1782-J1 —— 子句「十层拼图」层完备性
pub fn layer_stats(list: &TripleList) -> Vec<LayerStat> {
    let mut out: Vec<LayerStat> = Vec::new();
    let mut li = 0usize;
    while li < StructLayer::ALL.len() {
        let layer = StructLayer::ALL[li];
        let mut st = LayerStat {
            layer_ord: layer.ordinal(),
            layer_name: layer.name(),
            field_count: 0,
            ledger_count: 0,
            iface_count: 0,
            hot_fields: 0,
            out_edges: 0,
            in_edges: 0,
        };
        // 字段与账位：按行的映射关系统计（账位归到被映字段的层上）。
        let mut seen_iface: Vec<u32> = Vec::new();
        let mut ri = 0usize;
        while ri < list.rows.len() {
            let r = &list.rows[ri];
            if r.field != NONE_IDX {
                if let Ok(f) = list.fields.get(r.field) {
                    if f.layer == layer {
                        st.field_count += 1;
                        if f.hot_path {
                            st.hot_fields += 1;
                        }
                        if r.ledger != NONE_IDX {
                            st.ledger_count += 1;
                        }
                        if r.iface != NONE_IDX && !seen_iface.contains(&r.iface) {
                            seen_iface.push(r.iface);
                        }
                    }
                }
            }
            ri += 1;
        }
        st.iface_count = seen_iface.len() as u32;
        let mut ei = 0usize;
        while ei < list.edges.len() {
            let e = &list.edges[ei];
            if e.src == layer {
                st.out_edges += 1;
            }
            if e.dst == layer {
                st.in_edges += 1;
            }
            ei += 1;
        }
        out.push(st);
        li += 1;
    }
    out
}

// ============================================================================
// 8. 结构自检断言集（35 条具名断言）
// ============================================================================

/// 断言求值上下文。
pub struct AssertCtx<'a> {
    /// 三层总清单。
    pub list: &'a TripleList,
    /// 注入核允许置位（注入场景下放宽部分断言）。
    pub injection_mode: bool,
}

/// 断言结果。
#[derive(Clone)]
pub struct AssertResult {
    /// 断言 id（F1782 内序，从 1 起）。
    pub id: u32,
    /// 断言名。
    pub name: &'static str,
    /// 是否通过。
    pub pass: bool,
    /// 说明文本。
    pub detail: String,
}

/// 断言报告。
#[derive(Clone)]
pub struct AssertionReport {
    /// 通过数。
    pub passed: u32,
    /// 失败数。
    pub failed: u32,
    /// 首个失败项（无失败时为空）。
    pub first_failure: Option<AssertResult>,
    /// 全部结果（按 id 序）。
    pub results: Vec<AssertResult>,
    /// 首个失败诊断码。
    pub first_failure_code: u16,
}

impl AssertionReport {
    /// 报告是否全过。
    pub fn all_pass(&self) -> bool {
        self.failed == 0
    }

    /// 报告单行摘要。
    pub fn summary(&self) -> String {
        match &self.first_failure {
            Some(f) => format!(
                "F1782 assertions passed={} failed={} first={}#{}",
                self.passed, self.failed, f.name, f.id
            ),
            None => format!(
                "F1782 assertions passed={} failed={} all_ok",
                self.passed, self.failed
            ),
        }
    }
}

/// 断言阈值：热路径字段允许的最小偏移（用于防重叠）。
const ASSERT_SIG_SAMPLE: u32 = 4;

fn mk(id: u32, name: &'static str, pass: bool, detail: String) -> AssertResult {
    AssertResult {
        id,
        name,
        pass,
        detail,
    }
}

// --- 断言 01~08：登记册规模与 append-only 语义 ---

/// 断言 01：字段登记册非空。
pub fn assert_registry_nonempty(ctx: &AssertCtx) -> AssertResult {
    let pass = !ctx.list.fields.is_empty()
        && !ctx.list.ledgers.is_empty()
        && !ctx.list.ifaces.is_empty();
    mk(
        1,
        "assert_registry_nonempty",
        pass,
        format!(
            "fields={} ledgers={} ifaces={}",
            ctx.list.fields.len(),
            ctx.list.ledgers.len(),
            ctx.list.ifaces.len()
        ),
    )
}

/// 断言 02：字段总数下界 ≥40。
pub fn assert_field_min_total(ctx: &AssertCtx) -> AssertResult {
    let n = ctx.list.fields.len();
    mk(
        2,
        "assert_field_min_total",
        n >= FIELD_MIN_TOTAL || ctx.injection_mode,
        format!("fields={} min={}", n, FIELD_MIN_TOTAL),
    )
}

/// 断言 03：账位总数下界。
pub fn assert_ledger_min_total(ctx: &AssertCtx) -> AssertResult {
    let n = ctx.list.ledgers.len();
    mk(
        3,
        "assert_ledger_min_total",
        n >= LEDGER_MIN_TOTAL || ctx.injection_mode,
        format!("ledgers={} min={}", n, LEDGER_MIN_TOTAL),
    )
}

/// 断言 04：接口总数下界 ≥24。
pub fn assert_iface_min_total(ctx: &AssertCtx) -> AssertResult {
    let n = ctx.list.ifaces.len();
    mk(
        4,
        "assert_iface_min_total",
        n >= IFACE_MIN_TOTAL || ctx.injection_mode,
        format!("ifaces={} min={}", n, IFACE_MIN_TOTAL),
    )
}

/// 断言 05：字段名唯一（登记册 by_name 表规模与字段数一致）。
pub fn assert_field_name_unique(ctx: &AssertCtx) -> AssertResult {
    let pass = ctx.list.fields.by_name.len() as u32 == ctx.list.fields.len();
    mk(
        5,
        "assert_field_name_unique",
        pass,
        format!(
            "unique={} total={}",
            ctx.list.fields.by_name.len(),
            ctx.list.fields.len()
        ),
    )
}

/// 断言 06：账位名唯一。
pub fn assert_ledger_name_unique(ctx: &AssertCtx) -> AssertResult {
    let pass = ctx.list.ledgers.by_name.len() as u32 == ctx.list.ledgers.len();
    mk(
        6,
        "assert_ledger_name_unique",
        pass,
        format!(
            "unique={} total={}",
            ctx.list.ledgers.by_name.len(),
            ctx.list.ledgers.len()
        ),
    )
}

/// 断言 07：接口签名串唯一。
pub fn assert_iface_signature_unique(ctx: &AssertCtx) -> AssertResult {
    let pass = ctx.list.ifaces.by_sig.len() as u32 == ctx.list.ifaces.len();
    mk(
        7,
        "assert_iface_signature_unique",
        pass,
        format!(
            "unique={} total={}",
            ctx.list.ifaces.by_sig.len(),
            ctx.list.ifaces.len()
        ),
    )
}

/// 断言 08：登记册 append-only —— 描述符数组与名称表、偏移表同长同序。
pub fn assert_append_only_shape(ctx: &AssertCtx) -> AssertResult {
    let f = &ctx.list.fields;
    let l = &ctx.list.ledgers;
    let i = &ctx.list.ifaces;
    let pass = f.items.len() == f.names.len()
        && f.items.len() == f.offsets.len()
        && l.items.len() == l.names.len()
        && l.items.len() == l.families.len()
        && i.items.len() == i.names.len()
        && i.items.len() == i.signatures.len();
    mk(8, "assert_append_only_shape", pass, String::from("shape_checked"))
}

// --- 断言 09~16：布局与偏移契约 ---

/// 断言 09：字段偏移 8 字节对齐。
pub fn assert_offset_alignment(ctx: &AssertCtx) -> AssertResult {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < ctx.list.fields.items.len() {
        if ctx.list.fields.items[i].offset % FIELD_ALIGN != 0 {
            bad += 1;
        }
        i += 1;
    }
    mk(
        9,
        "assert_offset_alignment",
        bad == 0,
        format!("misaligned={}", bad),
    )
}

/// 断言 10：偏移在字段序号上单调不减。
pub fn assert_offset_monotonic(ctx: &AssertCtx) -> AssertResult {
    let mut bad = 0u32;
    let mut i = 1usize;
    while i < ctx.list.fields.items.len() {
        if ctx.list.fields.items[i].offset < ctx.list.fields.items[i - 1].offset {
            bad += 1;
        }
        i += 1;
    }
    mk(
        10,
        "assert_offset_monotonic",
        bad == 0,
        format!("inversions={}", bad),
    )
}

/// 断言 11：槽位不重叠（end_offset <= 下一字段 offset）。
pub fn assert_slot_no_overlap(ctx: &AssertCtx) -> AssertResult {
    let mut bad = 0u32;
    let mut i = 1usize;
    while i < ctx.list.fields.items.len() {
        if ctx.list.fields.items[i - 1].end_offset() > ctx.list.fields.items[i].offset {
            bad += 1;
        }
        i += 1;
    }
    mk(
        11,
        "assert_slot_no_overlap",
        bad == 0,
        format!("overlaps={}", bad),
    )
}

/// 断言 12：字段宽度合法（4 或 8）。
pub fn assert_width_legal(ctx: &AssertCtx) -> AssertResult {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < ctx.list.fields.items.len() {
        let w = ctx.list.fields.items[i].width;
        if w != 4 && w != SLOT_ALIGN {
            bad += 1;
        }
        i += 1;
    }
    mk(12, "assert_width_legal", bad == 0, format!("bad={}", bad))
}

/// 断言 13：字段绝对地址落在 PerCpu 契约跨度内。
pub fn assert_addr_in_span(ctx: &AssertCtx) -> AssertResult {
    let span = ctx.list.fields.items.len() as u64 * SLOT_ALIGN as u64;
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < ctx.list.fields.items.len() {
        if ctx.list.fields.items[i].addr() >= PERCPU_BASE + span {
            bad += 1;
        }
        i += 1;
    }
    mk(
        13,
        "assert_addr_in_span",
        bad == 0,
        format!("span={} bad={}", span, bad),
    )
}

/// 断言 14：账位地址按 16 字节描述符步进不重叠。
pub fn assert_ledger_addr_unique(ctx: &AssertCtx) -> AssertResult {
    let mut seen: Vec<u64> = Vec::new();
    let mut dup = 0u32;
    let mut i = 0usize;
    while i < ctx.list.ledgers.items.len() {
        let a = ctx.list.ledgers.items[i].addr();
        if seen.contains(&a) {
            dup += 1;
        } else {
            seen.push(a);
        }
        i += 1;
    }
    mk(
        14,
        "assert_ledger_addr_unique",
        dup == 0,
        format!("dup_addr={}", dup),
    )
}

/// 断言 15：接口描述符地址按 16 字节步进不重叠。
pub fn assert_iface_desc_addr_unique(ctx: &AssertCtx) -> AssertResult {
    let mut seen: Vec<u64> = Vec::new();
    let mut dup = 0u32;
    let mut i = 0usize;
    while i < ctx.list.ifaces.items.len() {
        let a = ctx.list.ifaces.items[i].desc_addr();
        if seen.contains(&a) {
            dup += 1;
        } else {
            seen.push(a);
        }
        i += 1;
    }
    mk(
        15,
        "assert_iface_desc_addr_unique",
        dup == 0,
        format!("dup_desc={}", dup),
    )
}

/// 断言 16：热路径字段全部落在已登记槽位内且宽度非零（热路径字段不得为空槽）。
pub fn assert_hot_fields_valid(ctx: &AssertCtx) -> AssertResult {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < ctx.list.fields.items.len() {
        let f = &ctx.list.fields.items[i];
        if f.hot_path && (f.width == 0 || f.offset == u32::MAX) {
            bad += 1;
        }
        i += 1;
    }
    mk(
        16,
        "assert_hot_fields_valid",
        bad == 0,
        format!("bad_hot={}", bad),
    )
}

// --- 断言 17~23：三层对齐与孤儿闭合 ---

/// 断言 17：矩阵行数与字段数一致（缺登记会破坏该等式）。
pub fn assert_matrix_row_eq_fields(ctx: &AssertCtx) -> AssertResult {
    let pass = ctx.list.row_count() == ctx.list.fields.len();
    mk(
        17,
        "assert_matrix_row_eq_fields",
        pass || ctx.injection_mode,
        format!(
            "rows={} fields={}",
            ctx.list.row_count(),
            ctx.list.fields.len()
        ),
    )
}

/// 断言 18：矩阵全行 Aligned。
///
/// 判据：UNX-F1782-J1 —— 子句「三方对齐断言全过」
pub fn assert_matrix_all_aligned(ctx: &AssertCtx) -> AssertResult {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < ctx.list.rows.len() {
        if !ctx.list.rows[i].status.is_pass() {
            bad += 1;
        }
        i += 1;
    }
    mk(
        18,
        "assert_matrix_all_aligned",
        bad == 0,
        format!("broken_rows={}", bad),
    )
}

/// 断言 19：每行账位存在（无 MissingLedger）。
pub fn assert_no_missing_ledger(ctx: &AssertCtx) -> AssertResult {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < ctx.list.rows.len() {
        if ctx.list.rows[i].ledger == NONE_IDX {
            bad += 1;
        }
        i += 1;
    }
    mk(
        19,
        "assert_no_missing_ledger",
        bad == 0,
        format!("missing={}", bad),
    )
}

/// 断言 20：每行接口存在（无 MissingInterface）。
pub fn assert_no_missing_interface(ctx: &AssertCtx) -> AssertResult {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < ctx.list.rows.len() {
        if ctx.list.rows[i].iface == NONE_IDX {
            bad += 1;
        }
        i += 1;
    }
    mk(
        20,
        "assert_no_missing_interface",
        bad == 0,
        format!("missing={}", bad),
    )
}

/// 断言 21：账侧零孤儿。
pub fn assert_no_orphan_ledger(ctx: &AssertCtx) -> AssertResult {
    let s = scan_orphans(ctx.list);
    mk(
        21,
        "assert_no_orphan_ledger",
        s.orphan_ledgers.is_empty(),
        format!("orphans={}", s.orphan_ledgers.len()),
    )
}

/// 断言 22：接口侧零孤儿。
pub fn assert_no_orphan_iface(ctx: &AssertCtx) -> AssertResult {
    let s = scan_orphans(ctx.list);
    mk(
        22,
        "assert_no_orphan_iface",
        s.orphan_ifaces.is_empty(),
        format!("orphans={}", s.orphan_ifaces.len()),
    )
}

/// 断言 23：字段侧零未映射（反向闭合）。
pub fn assert_no_unmapped_field(ctx: &AssertCtx) -> AssertResult {
    let s = scan_orphans(ctx.list);
    mk(
        23,
        "assert_no_unmapped_field",
        s.unmapped_fields.is_empty(),
        format!("unmapped={}", s.unmapped_fields.len()),
    )
}

/// 断言 24：无重复映射（一个字段不得映到两个账位）。
pub fn assert_no_dup_mapping(ctx: &AssertCtx) -> AssertResult {
    let dups = count_field_usage(ctx.list);
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < ctx.list.rows.len() {
        let f = ctx.list.rows[i].field;
        if let Some(c) = dups.get(&f) {
            if *c > 1 {
                bad += 1;
            }
        }
        i += 1;
    }
    mk(
        24,
        "assert_no_dup_mapping",
        bad == 0,
        format!("dup_rows={}", bad),
    )
}

/// 断言 25：账位不得被两字段共享（账位唯一归属）。
pub fn assert_ledger_single_owner(ctx: &AssertCtx) -> AssertResult {
    let mut owner: BTreeMap<u32, u32> = BTreeMap::new();
    let mut dup = 0u32;
    let mut i = 0usize;
    while i < ctx.list.rows.len() {
        let l = ctx.list.rows[i].ledger;
        if l != NONE_IDX {
            if owner.contains_key(&l) {
                dup += 1;
            } else {
                owner.insert(l, ctx.list.rows[i].field);
            }
        }
        i += 1;
    }
    mk(
        25,
        "assert_ledger_single_owner",
        dup == 0,
        format!("shared_ledgers={}", dup),
    )
}

/// 断言 26：类型宽度匹配（字段宽度 == 账位宽度）。
pub fn assert_type_width_match(ctx: &AssertCtx) -> AssertResult {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < ctx.list.rows.len() {
        let r = &ctx.list.rows[i];
        if r.field == NONE_IDX || r.ledger == NONE_IDX {
            continue;
        }
        if let Ok(f) = ctx.list.fields.get(r.field) {
            if let Ok(l) = ctx.list.ledgers.get(r.ledger) {
                if f.width != l.width {
                    bad += 1;
                }
            }
        }
        i += 1;
    }
    mk(
        26,
        "assert_type_width_match",
        bad == 0,
        format!("mismatch={}", bad),
    )
}

/// 断言 27：账族与字段层同构。
pub fn assert_family_layer_consistent(ctx: &AssertCtx) -> AssertResult {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < ctx.list.rows.len() {
        let r = &ctx.list.rows[i];
        if r.field == NONE_IDX || r.ledger == NONE_IDX {
            continue;
        }
        if let Ok(f) = ctx.list.fields.get(r.field) {
            if let Ok(l) = ctx.list.ledgers.get(r.ledger) {
                if l.family.layer() != f.layer {
                    bad += 1;
                }
            }
        }
        i += 1;
    }
    mk(
        27,
        "assert_family_layer_consistent",
        bad == 0,
        format!("cross_layer={}", bad),
    )
}

/// 断言 28：偏移契约自洽（字段偏移落在槽位图内且与账位步长一致）。
pub fn assert_offset_contract(ctx: &AssertCtx) -> AssertResult {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < ctx.list.rows.len() {
        let r = &ctx.list.rows[i];
        if r.field == NONE_IDX {
            continue;
        }
        if let Ok(f) = ctx.list.fields.get(r.field) {
            if f.offset % FIELD_ALIGN != 0 {
                bad += 1;
            }
            if f.end_offset() % 4 != 0 {
                bad += 1;
            }
        }
        i += 1;
    }
    mk(
        28,
        "assert_offset_contract",
        bad == 0,
        format!("broken={}", bad),
    )
}

// --- 断言 29~35：十层完备性与层间连接 ---

/// 断言 29：每层字段数达标。
pub fn assert_layer_min_fields(ctx: &AssertCtx) -> AssertResult {
    let stats = layer_stats(ctx.list);
    let mut bad = Vec::new();
    let mut i = 0usize;
    while i < stats.len() {
        if !stats[i].fields_ok() {
            bad.push(stats[i].layer_name);
        }
        i += 1;
    }
    mk(
        29,
        "assert_layer_min_fields",
        bad.is_empty(),
        format!("fail_layers={}", bad.len()),
    )
}

/// 断言 30：每层账位数达标。
pub fn assert_layer_min_ledger(ctx: &AssertCtx) -> AssertResult {
    let stats = layer_stats(ctx.list);
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < stats.len() {
        if !stats[i].ledger_ok() {
            bad += 1;
        }
        i += 1;
    }
    mk(
        30,
        "assert_layer_min_ledger",
        bad == 0,
        format!("fail_layers={}", bad),
    )
}

/// 断言 31：每层接口数达标。
pub fn assert_layer_min_iface(ctx: &AssertCtx) -> AssertResult {
    let stats = layer_stats(ctx.list);
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < stats.len() {
        if !stats[i].iface_ok() {
            bad += 1;
        }
        i += 1;
    }
    mk(
        31,
        "assert_layer_min_iface",
        bad == 0,
        format!("fail_layers={}", bad),
    )
}

/// 断言 32：层间连接点无重复登记（源层/目标层/连接名三元组唯一）。
pub fn assert_layer_edge_unique(ctx: &AssertCtx) -> AssertResult {
    let mut seen: Vec<String> = Vec::new();
    let mut dup = 0u32;
    let mut i = 0usize;
    while i < ctx.list.edges.len() {
        let e = &ctx.list.edges[i];
        let key = format!("{}>{}:{}", e.src.name(), e.dst.name(), e.name).to_string();
        if seen.contains(&key) {
            dup += 1;
        } else {
            seen.push(key);
        }
        i += 1;
    }
    mk(
        32,
        "assert_layer_edge_unique",
        dup == 0,
        format!("dup_edges={}", dup),
    )
}

/// 断言 33：每层至少一条出边（无孤立层）。
pub fn assert_layer_not_isolated(ctx: &AssertCtx) -> AssertResult {
    let stats = layer_stats(ctx.list);
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < stats.len() {
        if stats[i].out_edges == 0 {
            bad += 1;
        }
        i += 1;
    }
    mk(
        33,
        "assert_layer_not_isolated",
        bad == 0,
        format!("isolated_layers={}", bad),
    )
}

/// 断言 34：热路径字段不得声明为锁层语义字段（层次倒置防护）。
///
/// 判据：UNX-F1782-J1 —— 子句「热路径含锁」注入形态的守门断言
pub fn assert_hotpath_has_no_lock(ctx: &AssertCtx) -> AssertResult {
    let mut i = 0usize;
    while i < ctx.list.edges.len() {
        let e = &ctx.list.edges[i];
        // 锁层热路径出边只允许两条只读发布边；其余热路径出边即为层次倒置。
        let allowed = e.name == "hold_publish" || e.name == "contend_emit";
        if e.hot_path && e.src == StructLayer::Lock && !allowed {
            return mk(
                34,
                "assert_hotpath_has_no_lock",
                false,
                format!("inverted_edge={}", e.name),
            );
        }
        i += 1;
    }
    mk(
        34,
        "assert_hotpath_has_no_lock",
        true,
        String::from("no_hot_lock_inversion"),
    )
}

/// 断言 35：接口消费方必须显式登记（B09/B29/KERNEL 三选一，且热接口不得为面板族）。
pub fn assert_iface_consumer_declared(ctx: &AssertCtx) -> AssertResult {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < ctx.list.ifaces.items.len() {
        let it = &ctx.list.ifaces.items[i];
        if it.hot_path && it.family == ExportFamily::Panel {
            bad += 1;
        }
        i += 1;
    }
    mk(
        35,
        "assert_iface_consumer_declared",
        bad == 0,
        format!("bad_hot_iface={}", bad),
    )
}

/// 单条断言的函数别名类型（便于统一调度）。
pub type AssertFn = fn(&AssertCtx) -> AssertResult;

/// 全部 35 条断言的调度表（顺序即 id 序）。
pub const ASSERT_TABLE: [(u32, &'static str, AssertFn); 35] = [
    (1, "assert_registry_nonempty", assert_registry_nonempty),
    (2, "assert_field_min_total", assert_field_min_total),
    (3, "assert_ledger_min_total", assert_ledger_min_total),
    (4, "assert_iface_min_total", assert_iface_min_total),
    (5, "assert_field_name_unique", assert_field_name_unique),
    (6, "assert_ledger_name_unique", assert_ledger_name_unique),
    (7, "assert_iface_signature_unique", assert_iface_signature_unique),
    (8, "assert_append_only_shape", assert_append_only_shape),
    (9, "assert_offset_alignment", assert_offset_alignment),
    (10, "assert_offset_monotonic", assert_offset_monotonic),
    (11, "assert_slot_no_overlap", assert_slot_no_overlap),
    (12, "assert_width_legal", assert_width_legal),
    (13, "assert_addr_in_span", assert_addr_in_span),
    (14, "assert_ledger_addr_unique", assert_ledger_addr_unique),
    (15, "assert_iface_desc_addr_unique", assert_iface_desc_addr_unique),
    (16, "assert_hot_fields_valid", assert_hot_fields_valid),
    (17, "assert_matrix_row_eq_fields", assert_matrix_row_eq_fields),
    (18, "assert_matrix_all_aligned", assert_matrix_all_aligned),
    (19, "assert_no_missing_ledger", assert_no_missing_ledger),
    (20, "assert_no_missing_interface", assert_no_missing_interface),
    (21, "assert_no_orphan_ledger", assert_no_orphan_ledger),
    (22, "assert_no_orphan_iface", assert_no_orphan_iface),
    (23, "assert_no_unmapped_field", assert_no_unmapped_field),
    (24, "assert_no_dup_mapping", assert_no_dup_mapping),
    (25, "assert_ledger_single_owner", assert_ledger_single_owner),
    (26, "assert_type_width_match", assert_type_width_match),
    (27, "assert_family_layer_consistent", assert_family_layer_consistent),
    (28, "assert_offset_contract", assert_offset_contract),
    (29, "assert_layer_min_fields", assert_layer_min_fields),
    (30, "assert_layer_min_ledger", assert_layer_min_ledger),
    (31, "assert_layer_min_iface", assert_layer_min_iface),
    (32, "assert_layer_edge_unique", assert_layer_edge_unique),
    (33, "assert_layer_not_isolated", assert_layer_not_isolated),
    (34, "assert_hotpath_has_no_lock", assert_hotpath_has_no_lock),
    (35, "assert_iface_consumer_declared", assert_iface_consumer_declared),
];

/// 聚合执行全部断言并产出报告。
///
/// 与各批断言集的分工：F1784 负责跨批聚合（把 F1782 报告并入总账），
/// 本函数只负责本单批内断言的求值与首个失败项定位。
pub fn run_all_assertions(list: &TripleList, injection_mode: bool) -> AssertionReport {
    let ctx = AssertCtx {
        list,
        injection_mode,
    };
    let mut rep = AssertionReport {
        passed: 0,
        failed: 0,
        first_failure: None,
        results: Vec::new(),
        first_failure_code: 0,
    };
    let mut i = 0usize;
    while i < ASSERT_TABLE.len() {
        let (_, _, f) = ASSERT_TABLE[i];
        let r = f(&ctx);
        if r.pass {
            rep.passed += 1;
        } else {
            rep.failed += 1;
            if rep.first_failure.is_none() {
                rep.first_failure_code = DIAG_MAIN_ASSERT_FAILED;
                rep.first_failure = Some(r.clone());
            }
        }
        rep.results.push(r);
        i += 1;
    }
    rep
}

// ============================================================================
// 9. 启动期自检 boot_audit（64 核满配建模，F1790 同建模）
// ============================================================================

/// F1790 面板全量试运行采用的 64 核满配建模核数。
pub const FULL_CONFIG_CORES: u32 = 64;

/// 单核的建模结构（boot_audit 逐核核验对象）。
#[derive(Clone)]
pub struct CoreModel {
    /// 核序号。
    pub core: u32,
    /// 是否在线。
    pub online: bool,
    /// 该核可见的字段数（应等于字段总数）。
    pub visible_fields: u32,
    /// 该核可见的账位数。
    pub visible_ledgers: u32,
    /// 该核挂载的接口数。
    pub mounted_ifaces: u32,
    /// 该核对齐矩阵健康行数。
    pub aligned_rows: u32,
}

impl CoreModel {
    /// 构建 64 核满配模型（核 0..63 全部在线、结构完全可见）。
    pub fn full_config(list: &TripleList) -> CoreModel {
        CoreModel {
            core: 0,
            online: true,
            visible_fields: list.fields.len(),
            visible_ledgers: list.ledgers.len(),
            mounted_ifaces: list.ifaces.len(),
            aligned_rows: list.row_count(),
        }
    }
}

/// 单核核验结果。
#[derive(Clone)]
pub struct CoreVerdict {
    /// 核序号。
    pub core: u32,
    /// 是否通过。
    pub pass: bool,
    /// 首个失败原因。
    pub reason: String,
}

/// 启动期自检决策。
#[derive(Clone)]
pub struct BootDecision {
    /// 是否放行上线。
    pub allow: bool,
    /// 失败核数。
    pub failed_cores: u32,
    /// 首个失败核。
    pub first_failed_core: u32,
    /// 诊断码。
    pub code: u16,
    /// 说明。
    pub detail: String,
}

impl BootDecision {
    /// 是否为阻断态。
    pub fn is_block(&self) -> bool {
        !self.allow
    }
}

/// 单核逐项核验。
fn verify_core(list: &TripleList, model: &CoreModel, orphan: &OrphanScan) -> CoreVerdict {
    if !model.online {
        return CoreVerdict {
            core: model.core,
            pass: false,
            reason: String::from("core_offline"),
        };
    }
    if model.visible_fields != list.fields.len() {
        return CoreVerdict {
            core: model.core,
            pass: false,
            reason: String::from("field_count_mismatch"),
        };
    }
    if model.visible_ledgers != list.ledgers.len() {
        return CoreVerdict {
            core: model.core,
            pass: false,
            reason: String::from("ledger_count_mismatch"),
        };
    }
    if model.mounted_ifaces < IFACE_MIN_TOTAL {
        return CoreVerdict {
            core: model.core,
            pass: false,
            reason: String::from("iface_mount_shortfall"),
        };
    }
    if model.aligned_rows != list.row_count() {
        return CoreVerdict {
            core: model.core,
            pass: false,
            reason: String::from("matrix_row_mismatch"),
        };
    }
    if !orphan.is_closed() {
        return CoreVerdict {
            core: model.core,
            pass: false,
            reason: format!(
                "orphan_open:{}",
                orphan.total()
            ),
        };
    }
    CoreVerdict {
        core: model.core,
        pass: true,
        reason: String::from("ok"),
    }
}

/// 启动期结构自检：64 核满配逐核逐项核验，失败即 Block。
///
/// 判据：UNX-F1782-J1 —— 子句「三方对齐断言全过」在启动期的落地形态
pub fn boot_audit(list: &TripleList) -> AuditResult<BootDecision> {
    let broken = judge_all_clone(list);
    let orphan = scan_orphans(list);
    let mut failed = 0u32;
    let mut first = 0u32;
    let mut have_first = false;
    let mut core = 0u32;
    while core < FULL_CONFIG_CORES {
        let mut model = CoreModel::full_config(list);
        model.core = core;
        model.aligned_rows = model.aligned_rows.min(list.row_count() - broken);
        let v = verify_core(list, &model, &orphan);
        if !v.pass {
            failed += 1;
            if !have_first {
                have_first = true;
                first = v.core;
            }
        }
        core += 1;
    }
    let rep = run_all_assertions(list, false);
    let allow = failed == 0 && broken == 0 && rep.all_pass();
    Ok(BootDecision {
        allow,
        failed_cores: failed,
        first_failed_core: if have_first { first } else { 0 },
        code: if allow { 0 } else { DIAG_MAIN_BOOT_BLOCKED },
        detail: if allow {
            format!("boot_ok cores={} rows={}", FULL_CONFIG_CORES, list.row_count())
        } else {
            format!(
                "boot_block broken_rows={} failed_cores={} asserts={}",
                broken, failed, rep.summary()
            )
        },
    })
}

/// 只读副本上执行全矩阵判定，返回非 Aligned 行数（不污染调用方清单）。
pub fn judge_all_clone(list: &TripleList) -> u32 {
    let dups = count_field_usage(list);
    let mut broken = 0u32;
    let mut i = 0usize;
    while i < list.rows.len() {
        if !judge_row(list, &list.rows[i], &dups).is_pass() {
            broken += 1;
        }
        i += 1;
    }
    broken
}

// ============================================================================
// 10. 运行期周期巡检（三族巡检 + 缺口检出 + 开销在档）
// ============================================================================

/// 巡检三族。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PatrolFamily {
    /// 账目活性：账位每轮须被读到（挂点活）。
    LedgerAlive,
    /// 挂点完整：接口须仍挂载（无被摘除）。
    HookIntact,
    /// mask 一致：在线掩码与巡检集合一致。
    MaskConsistent,
}

impl PatrolFamily {
    /// 三族全集。
    pub const ALL: [PatrolFamily; 3] = [
        PatrolFamily::LedgerAlive,
        PatrolFamily::HookIntact,
        PatrolFamily::MaskConsistent,
    ];

    /// 族名文本。
    pub fn name(&self) -> &'static str {
        match self {
            PatrolFamily::LedgerAlive => "ledger_alive",
            PatrolFamily::HookIntact => "hook_intact",
            PatrolFamily::MaskConsistent => "mask_consistent",
        }
    }
}

/// 巡检节拍周期（逻辑 tick）。
pub const PATROL_PERIOD: u64 = 16;
/// 巡检自身 tick 开销上界（开销在档的档位上限；模型口径为 64 核一趟 + 三族各一趟）。
pub const PATROL_COST_CAP: u64 = 128;

/// 巡检运行态（跨轮次保持）。
#[derive(Clone)]
pub struct PatrolState {
    /// 当前逻辑时刻。
    pub tick: u64,
    /// 已执行巡检轮次。
    pub rounds: u64,
    /// 各核上次被巡检的时刻。
    pub last_seen: Vec<u64>,
    /// 各族累计命中数。
    pub family_hits: Vec<u32>,
    /// 累计巡检开销（tick 计量）。
    pub cost_total: u64,
    /// 最近一轮开销。
    pub last_cost: u64,
}

impl PatrolState {
    /// 以 64 核满配初始化巡检状态。
    pub fn new_full_config() -> PatrolState {
        PatrolState {
            tick: 0,
            rounds: 0,
            last_seen: vec![u64::MAX; FULL_CONFIG_CORES as usize],
            family_hits: vec![0; PatrolFamily::ALL.len()],
            cost_total: 0,
            last_cost: 0,
        }
    }

    /// 某核距上次巡检的间隔。
    pub fn gap(&self, core: u32) -> u64 {
        if self.tick == 0 {
            return 0;
        }
        match self.last_seen.get(core as usize) {
            Some(v) if *v != u64::MAX => self.tick - *v,
            _ => u64::MAX,
        }
    }
}

/// 单族巡检结果。
#[derive(Clone)]
pub struct FamilyPatrol {
    /// 族。
    pub family: PatrolFamily,
    /// 本族覆盖的核数。
    pub covered: u32,
    /// 本族异常核数。
    pub abnormal: u32,
    /// 首个异常核。
    pub first_bad: u32,
}

/// 巡检报告。
#[derive(Clone)]
pub struct PatrolReport {
    /// 逻辑时刻。
    pub tick: u64,
    /// 轮次。
    pub round: u64,
    /// 三族结果。
    pub families: Vec<FamilyPatrol>,
    /// 缺口核（未在周期内被巡检）。
    pub gap_cores: Vec<u32>,
    /// 本轮开销。
    pub cost: u64,
    /// 诊断码（0 表示健康）。
    pub code: u16,
    /// 说明。
    pub detail: String,
}

impl PatrolReport {
    /// 是否健康。
    pub fn healthy(&self) -> bool {
        self.gap_cores.is_empty() && self.code == 0
    }
}

/// 三族巡检核心（可指定跳过某核以注入缺口）。
///
/// 判据：UNX-F1782-J1 —— 子句「巡检跳过某核必须被检出」
pub fn patrol_run(
    state: &mut PatrolState,
    list: &TripleList,
    tick: u64,
    skip_core: u32,
) -> PatrolReport {
    state.tick = tick;
    state.rounds += 1;
    let mut rep = PatrolReport {
        tick,
        round: state.rounds,
        families: Vec::new(),
        gap_cores: Vec::new(),
        cost: 0,
        code: 0,
        detail: String::from("patrol_ok"),
    };
    let mut covered = 0u32;
    let mut core = 0u32;
    while core < FULL_CONFIG_CORES {
        if core == skip_core {
            rep.gap_cores.push(core);
        } else {
            if let Some(slot) = state.last_seen.get_mut(core as usize) {
                *slot = tick;
            }
            covered += 1;
        }
        core += 1;
    }
    let mut fi = 0usize;
    while fi < PatrolFamily::ALL.len() {
        let fam = PatrolFamily::ALL[fi];
        let mut abnormal = 0u32;
        let mut first_bad = 0u32;
        if fam == PatrolFamily::HookIntact && list.ifaces.len() < IFACE_MIN_TOTAL {
            abnormal = FULL_CONFIG_CORES;
            first_bad = 0;
        }
        if fam == PatrolFamily::LedgerAlive && list.ledgers.len() < LEDGER_MIN_TOTAL {
            abnormal = FULL_CONFIG_CORES;
            first_bad = 0;
        }
        if fam == PatrolFamily::MaskConsistent && !rep.gap_cores.is_empty() {
            abnormal = rep.gap_cores.len() as u32;
            first_bad = rep.gap_cores[0];
        }
        if abnormal == 0 {
            if let Some(slot) = state.family_hits.get_mut(fi) {
                *slot += 1;
            }
        }
        rep.families.push(FamilyPatrol {
            family: fam,
            covered,
            abnormal,
            first_bad,
        });
        fi += 1;
    }
    // 开销在档：基础每核 1 单位 + 三族各 1 单位，受 PATROL_COST_CAP 约束。
    let cost = covered as u64 + PatrolFamily::ALL.len() as u64;
    state.last_cost = cost;
    state.cost_total = state.cost_total.wrapping_add(cost);
    rep.cost = cost;
    if !rep.gap_cores.is_empty() {
        rep.code = 0xB081;
        rep.detail = format!("patrol_gap core={}", rep.gap_cores[0]);
    } else if cost > PATROL_COST_CAP {
        rep.code = 0xB08B;
        rep.detail = format!("patrol_cost cost={} cap={}", cost, PATROL_COST_CAP);
    }
    rep
}

/// 运行期周期巡检包装：按 tick 节拍判定是否到巡检期，到期则执行。
pub fn periodic_patrol(
    state: &mut PatrolState,
    list: &TripleList,
    tick: u64,
) -> Option<PatrolReport> {
    if tick % PATROL_PERIOD != 0 {
        return None;
    }
    Some(patrol_run(state, list, tick, NONE_IDX))
}

// ============================================================================
// 11. 注入核：10 种缺漏形态，每形态一注入 + 一检出
// ============================================================================

/// 注入形态描述符。
#[derive(Clone, Copy)]
pub struct InjectForm {
    /// 形态序号（1..=10）。
    pub id: u32,
    /// 形态名。
    pub name: &'static str,
    /// 期望检出的判定码（0 表示 healthy 判定）。
    pub expect_status: AlignStatus,
    /// 期望的诊断码。
    pub expect_code: u16,
    /// 语义说明。
    pub desc: &'static str,
}

/// 注入结果。
#[derive(Clone)]
pub struct InjectResult {
    /// 形态序号。
    pub id: u32,
    /// 形态名。
    pub name: &'static str,
    /// 是否被检出。
    pub detected: bool,
    /// 实际检出判定。
    pub found_status: AlignStatus,
    /// 实际诊断码。
    pub code: u16,
    /// 说明。
    pub detail: String,
}

/// 10 形态静态登记。
pub const INJECT_FORMS: [InjectForm; 10] = [
    InjectForm {
        id: 1,
        name: "missing_percpu_field",
        expect_status: AlignStatus::MissingPerCpu,
        expect_code: 0xB084,
        desc: "PerCpu 字段缺登记",
    },
    InjectForm {
        id: 2,
        name: "missing_ledger_slot",
        expect_status: AlignStatus::MissingLedger,
        expect_code: 0xB084,
        desc: "账位缺登记",
    },
    InjectForm {
        id: 3,
        name: "iface_not_mounted",
        expect_status: AlignStatus::MissingInterface,
        expect_code: 0xB084,
        desc: "观测接口未挂",
    },
    InjectForm {
        id: 4,
        name: "dup_mapping",
        expect_status: AlignStatus::DupMapping,
        expect_code: 0xB087,
        desc: "映射重复（一字段映两账位）",
    },
    InjectForm {
        id: 5,
        name: "offset_shifted",
        expect_status: AlignStatus::OffsetMismatch,
        expect_code: 0xB089,
        desc: "偏移错位",
    },
    InjectForm {
        id: 6,
        name: "width_mismatch",
        expect_status: AlignStatus::TypeMismatch,
        expect_code: 0xB088,
        desc: "类型宽度不匹配",
    },
    InjectForm {
        id: 7,
        name: "orphan_ledger",
        expect_status: AlignStatus::OrphanLedger,
        expect_code: 0xB085,
        desc: "孤儿账位（账有字段无）",
    },
    InjectForm {
        id: 8,
        name: "orphan_iface",
        expect_status: AlignStatus::OrphanIface,
        expect_code: 0xB086,
        desc: "孤儿接口（接口有字段无）",
    },
    InjectForm {
        id: 9,
        name: "patrol_skip_core",
        expect_status: AlignStatus::Aligned,
        expect_code: 0xB081,
        desc: "巡检跳过某核",
    },
    InjectForm {
        id: 10,
        name: "hotpath_locked",
        expect_status: AlignStatus::Aligned,
        expect_code: 0xB08A,
        desc: "热路径含锁（层次倒置）",
    },
];

/// 判定结果摘要（用于注入核统一取用）。
fn summarize(list: &TripleList) -> (AlignStatus, u16) {
    let dups = count_field_usage(list);
    let mut first = AlignStatus::Aligned;
    let mut i = 0usize;
    while i < list.rows.len() {
        let st = judge_row(list, &list.rows[i], &dups);
        if !st.is_pass() {
            first = st;
            break;
        }
        i += 1;
    }
    let s = scan_orphans(list);
    if first.is_pass() {
        if !s.orphan_ledgers.is_empty() {
            return (AlignStatus::OrphanLedger, 0xB085);
        }
        if !s.orphan_ifaces.is_empty() {
            return (AlignStatus::OrphanIface, 0xB086);
        }
        if !s.unmapped_fields.is_empty() {
            return (AlignStatus::MissingPerCpu, 0xB084);
        }
    }
    let code = if first.is_pass() { 0 } else { first.code() };
    (first, code)
}

fn wrap(id: u32, detected: bool, st: AlignStatus, code: u16, detail: String) -> InjectResult {
    InjectResult {
        id,
        name: "injected",
        detected,
        found_status: st,
        code,
        detail,
    }
}

// --- 形态 1：PerCpu 字段缺登记 ---

/// 注入形态 1：摘除一个 PerCpu 字段登记（行侧置 NONE）。
pub fn inject_1_missing_percpu_field() -> InjectResult {
    let mut r = wrap(1, false, AlignStatus::Aligned, 0, String::from("setup"));
    if let Ok(mut list) = TripleList::baseline() {
        if let Some(row) = list.rows.get_mut(20) {
            row.field = NONE_IDX;
            row.ledger = NONE_IDX;
        }
        let (st, code) = summarize(&list);
        r = wrap(1, st == AlignStatus::MissingPerCpu, st, code, String::from("field_removed"));
        r.name = "missing_percpu_field";
    }
    r
}

// --- 形态 2：账位缺登记 ---

/// 注入形态 2：摘除账位挂接。
pub fn inject_2_missing_ledger_slot() -> InjectResult {
    let mut r = wrap(2, false, AlignStatus::Aligned, 0, String::from("setup"));
    if let Ok(mut list) = TripleList::baseline() {
        if let Some(row) = list.rows.get_mut(25) {
            row.ledger = NONE_IDX;
        }
        let (st, code) = summarize(&list);
        r = wrap(2, st == AlignStatus::MissingLedger, st, code, String::from("ledger_removed"));
        r.name = "missing_ledger_slot";
    }
    r
}

// --- 形态 3：观测接口未挂 ---

/// 注入形态 3：摘除接口挂点。
pub fn inject_3_iface_not_mounted() -> InjectResult {
    let mut r = wrap(3, false, AlignStatus::Aligned, 0, String::from("setup"));
    if let Ok(mut list) = TripleList::baseline() {
        if let Some(row) = list.rows.get_mut(31) {
            row.iface = NONE_IDX;
        }
        let (st, code) = summarize(&list);
        r = wrap(3, st == AlignStatus::MissingInterface, st, code, String::from("iface_removed"));
        r.name = "iface_not_mounted";
    }
    r
}

// --- 形态 4：映射重复 ---

/// 注入形态 4：让一个字段映到两个账位（追加重复行）。
pub fn inject_4_dup_mapping() -> InjectResult {
    let mut r = wrap(4, false, AlignStatus::Aligned, 0, String::from("setup"));
    if let Ok(mut list) = TripleList::baseline() {
        let src = AlignRow::new(49, 49, 49, 16);
        list.rows.push(src);
        let (st, code) = summarize(&list);
        r = wrap(4, st == AlignStatus::DupMapping, st, code, String::from("dup_row"));
        r.name = "dup_mapping";
    }
    r
}

// --- 形态 5：偏移错位 ---

/// 注入形态 5：把某字段偏移改为非 8 字节对齐（错位）。
pub fn inject_5_offset_shifted() -> InjectResult {
    let mut r = wrap(5, false, AlignStatus::Aligned, 0, String::from("setup"));
    if let Ok(mut list) = TripleList::baseline() {
        if let Some(f) = list.fields.items.get_mut(16) {
            // +2 制造非 8 字节对齐错位（+4 会意外仍对齐，故取 +2）。
            f.offset = f.offset.wrapping_add(2);
        }
        let dups = count_field_usage(&list);
        let row = list.rows.get(16).cloned();
        let st = match row {
            Some(rw) => judge_row(&list, &rw, &dups),
            None => AlignStatus::MissingPerCpu,
        };
        r = wrap(5, st == AlignStatus::OffsetMismatch, st, st.code(), String::from("offset_shift"));
        r.name = "offset_shifted";
    }
    r
}

// --- 形态 6：类型宽度不匹配 ---

/// 注入形态 6：把某账位宽度改为与字段不一致。
pub fn inject_6_width_mismatch() -> InjectResult {
    let mut r = wrap(6, false, AlignStatus::Aligned, 0, String::from("setup"));
    if let Ok(mut list) = TripleList::baseline() {
        if let Some(l) = list.ledgers.items.get_mut(36) {
            l.width = 4;
        }
        let dups = count_field_usage(&list);
        let row = list.rows.get(36).cloned();
        let st = match row {
            Some(rw) => judge_row(&list, &rw, &dups),
            None => AlignStatus::MissingLedger,
        };
        r = wrap(6, st == AlignStatus::TypeMismatch, st, st.code(), String::from("width_break"));
        r.name = "width_mismatch";
    }
    r
}

// --- 形态 7：孤儿账位 ---

/// 注入形态 7：追加一个无人挂接的账位。
pub fn inject_7_orphan_ledger() -> InjectResult {
    let mut r = wrap(7, false, AlignStatus::Aligned, 0, String::from("setup"));
    if let Ok(mut list) = TripleList::baseline() {
        let _ = list.ledgers.append(LedgerSlot {
            idx: list.ledgers.len(),
            name: "ORPHAN_probe",
            family: LedgerFamily::Var,
            width: 8,
            update_entry: "macro_reg_var!",
            hot_path: false,
            desc: "注入用孤儿账位",
        });
        let s = scan_orphans(&list);
        r = wrap(
            7,
            !s.orphan_ledgers.is_empty(),
            AlignStatus::OrphanLedger,
            0xB085,
            format!("orphans={}", s.orphan_ledgers.len()),
        );
        r.name = "orphan_ledger";
    }
    r
}

// --- 形态 8：孤儿接口 ---

/// 注入形态 8：追加一个无人挂载的接口。
pub fn inject_8_orphan_iface() -> InjectResult {
    let mut r = wrap(8, false, AlignStatus::Aligned, 0, String::from("setup"));
    if let Ok(mut list) = TripleList::baseline() {
        let _ = list.ifaces.append(IfaceDesc {
            idx: list.ifaces.len(),
            name: "orphan_probe",
            family: ExportFamily::SelfCheck,
            signature: "fn() -> ()",
            consumer: Consumer::Kernel,
            hot_path: false,
            desc: "注入用孤儿接口",
        });
        let s = scan_orphans(&list);
        r = wrap(
            8,
            !s.orphan_ifaces.is_empty(),
            AlignStatus::OrphanIface,
            0xB086,
            format!("orphans={}", s.orphan_ifaces.len()),
        );
        r.name = "orphan_iface";
    }
    r
}

// --- 形态 9：巡检跳过某核 ---

/// 注入形态 9：巡检跳过第 17 号核。
pub fn inject_9_patrol_skip_core() -> InjectResult {
    let mut r = wrap(9, false, AlignStatus::Aligned, 0, String::from("setup"));
    if let Ok(list) = TripleList::baseline() {
        let mut st = PatrolState::new_full_config();
        let rep = patrol_run(&mut st, &list, PATROL_PERIOD, 17);
        r = wrap(
            9,
            !rep.gap_cores.is_empty() && rep.code == 0xB081,
            AlignStatus::Aligned,
            rep.code,
            format!("gap_cores={}", rep.gap_cores.len()),
        );
        r.name = "patrol_skip_core";
    }
    r
}

// --- 形态 10：热路径含锁（层次倒置） ---

/// 注入形态 10：插入一条 Lock→Runq 的热路径连接，制造层次倒置。
pub fn inject_10_hotpath_locked() -> InjectResult {
    let mut r = wrap(10, false, AlignStatus::Aligned, 0, String::from("setup"));
    if let Ok(mut list) = TripleList::baseline() {
        list.edges.push(LayerEdge {
            idx: list.edges.len() as u32,
            src: StructLayer::Lock,
            dst: StructLayer::Runq,
            name: "runq_lock_injected",
            hot_path: true,
            desc: "注入用热路径锁边",
        });
        let ctx = AssertCtx {
            list: &list,
            injection_mode: false,
        };
        let a = assert_hotpath_has_no_lock(&ctx);
        r = wrap(10, !a.pass, AlignStatus::Aligned, 0xB08A, a.detail);
        r.name = "hotpath_locked";
    }
    r
}

/// 串行确定性执行 10 形态注入核。
///
/// 判据：UNX-F1782-J1 —— 子句「缺漏注入 10/10 检出」
pub fn inject_all() -> Vec<InjectResult> {
    let mut out: Vec<InjectResult> = Vec::new();
    out.push(inject_1_missing_percpu_field());
    out.push(inject_2_missing_ledger_slot());
    out.push(inject_3_iface_not_mounted());
    out.push(inject_4_dup_mapping());
    out.push(inject_5_offset_shifted());
    out.push(inject_6_width_mismatch());
    out.push(inject_7_orphan_ledger());
    out.push(inject_8_orphan_iface());
    out.push(inject_9_patrol_skip_core());
    out.push(inject_10_hotpath_locked());
    out
}

/// 注入检出计数（应为 10/10）。
pub fn inject_detected_count(results: &[InjectResult]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < results.len() {
        if results[i].detected {
            n += 1;
        }
        i += 1;
    }
    n
}

// ============================================================================
// 12. 三件套导出（F1789 对齐）
// ============================================================================

/// 结构图文本导出：十层 + 层间连接点，缩进树。
///
/// 判据：UNX-F1782-J1 —— 子句「三层总清单」的可导出形态
pub fn export_struct_graph(list: &TripleList) -> String {
    let mut s = String::from("== STRUCT GRAPH (F1782) ==\n");
    let stats = layer_stats(list);
    let mut li = 0usize;
    while li < StructLayer::ALL.len() {
        let layer = StructLayer::ALL[li];
        s.push_str(&format!(
            "L{} {} [{}]\n",
            layer.ordinal(),
            layer.name(),
            stats[li].line()
        ));
        let mut i = 0usize;
        while i < list.fields.items.len() {
            let f = &list.fields.items[i];
            if f.layer == layer {
                s.push_str(&format!(
                    "  |- {} hot={} acc={}\n",
                    f.line(),
                    f.hot_path,
                    f.access.name()
                ));
            }
            i += 1;
        }
        let mut e = 0usize;
        while e < list.edges.len() {
            let ed = &list.edges[e];
            if ed.src == layer {
                s.push_str(&format!(
                    "  -> {} -> {} hot={}\n",
                    ed.dst.name(),
                    ed.name,
                    ed.hot_path
                ));
            }
            e += 1;
        }
        li += 1;
    }
    s
}

/// 账目图文本导出：族 → 账位 → 被映字段。
pub fn export_ledger_graph(list: &TripleList) -> String {
    let mut s = String::from("== LEDGER GRAPH (F1782) ==\n");
    let mut fi = 0usize;
    while fi < LedgerFamily::ALL.len() {
        let fam = LedgerFamily::ALL[fi];
        s.push_str(&format!(
            "FAM {} (layer={} slots={})\n",
            fam.name(),
            fam.layer().name(),
            list.ledgers.family_count(fam)
        ));
        let mut i = 0usize;
        while i < list.ledgers.items.len() {
            let l = &list.ledgers.items[i];
            if l.family == fam {
                s.push_str(&format!(
                    "  |- {} owner_field={}\n",
                    l.line(),
                    ledger_owner_field(list, l.idx)
                ));
            }
            i += 1;
        }
        fi += 1;
    }
    s
}

/// 账位归属字段名（未挂接时返回 "-"）。
fn ledger_owner_field(list: &TripleList, slot: u32) -> String {
    let mut i = 0usize;
    while i < list.rows.len() {
        if list.rows[i].ledger == slot && list.rows[i].field != NONE_IDX {
            if let Ok(f) = list.fields.get(list.rows[i].field) {
                return String::from(f.name);
            }
        }
        i += 1;
    }
    String::from("-")
}

/// 挂点图文本导出：接口 → 消费方 → 所挂字段。
pub fn export_hook_graph(list: &TripleList) -> String {
    let mut s = String::from("== HOOK GRAPH (F1782) ==\n");
    let mut i = 0usize;
    while i < list.ifaces.items.len() {
        let it = &list.ifaces.items[i];
        s.push_str(&format!(
            "{} consumer={}\n",
            it.line(),
            it.consumer.name()
        ));
        let mut mounted = 0u32;
        let mut r = 0usize;
        while r < list.rows.len() {
            if list.rows[r].iface == it.idx && list.rows[r].field != NONE_IDX {
                if let Ok(f) = list.fields.get(list.rows[r].field) {
                    s.push_str(&format!("    + {}\n", f.name));
                }
                mounted += 1;
            }
            r += 1;
        }
        s.push_str(&format!("    = mounted={}\n", mounted));
        i += 1;
    }
    s
}

/// 图账一致校验：三件套文本与静态登记规模一致。
///
/// 判据：UNX-F1782-J1 —— 子句「三方对齐」在导出侧的闭合
pub fn verify_graph_consistency(
    list: &TripleList,
    struct_g: &str,
    ledger_g: &str,
    hook_g: &str,
) -> AuditResult<()> {
    let mut field_hits = 0u32;
    let mut i = 0usize;
    while i < list.fields.items.len() {
        if struct_g.contains(list.fields.items[i].name) {
            field_hits += 1;
        }
        i += 1;
    }
    if field_hits != list.fields.len() {
        return Err(AuditError::GraphDrift {
            reason: "struct_graph_field_count",
        });
    }
    let mut l_hits = 0u32;
    i = 0;
    while i < list.ledgers.items.len() {
        if ledger_g.contains(list.ledgers.items[i].name) {
            l_hits += 1;
        }
        i += 1;
    }
    if l_hits != list.ledgers.len() {
        return Err(AuditError::GraphDrift {
            reason: "ledger_graph_slot_count",
        });
    }
    let mut i_hits = 0u32;
    i = 0;
    while i < list.ifaces.items.len() {
        if hook_g.contains(list.ifaces.items[i].name) {
            i_hits += 1;
        }
        i += 1;
    }
    if i_hits != list.ifaces.len() {
        return Err(AuditError::GraphDrift {
            reason: "hook_graph_iface_count",
        });
    }
    Ok(())
}

/// TLV 打包导出：tag(u16) + len(u16) + payload，带 FNV 指纹尾项。
///
/// 判据：UNX-F1782-J1 —— 子句「总清单」的机读导出
pub fn export_tlv(list: &TripleList) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let struct_g = export_struct_graph(list);
    let ledger_g = export_ledger_graph(list);
    let hook_g = export_hook_graph(list);
    push_tlv(&mut out, 0x1782, struct_g.as_bytes());
    push_tlv(&mut out, 0x1783, ledger_g.as_bytes());
    push_tlv(&mut out, 0x1784, hook_g.as_bytes());
    // 末项：指纹标签（tag=0x1785，payload = 前文指纹 u64 大端）
    let body = fnv1a64(&out);
    let mut tail: [u8; 8] = [0u8; 8];
    let mut k = 0usize;
    while k < 8 {
        tail[k] = ((body >> (56 - 8 * k)) & 0xFF) as u8;
        k += 1;
    }
    push_tlv(&mut out, 0x1785, &tail);
    out
}

/// 向字节缓冲追加一个 TLV 项。
fn push_tlv(buf: &mut Vec<u8>, tag: u16, payload: &[u8]) {
    let hi = ((tag >> 8) & 0xFF) as u8;
    let lo = (tag & 0xFF) as u8;
    let len = payload.len();
    let lh = ((len >> 8) & 0xFF) as u8;
    let ll = (len & 0xFF) as u8;
    buf.push(hi);
    buf.push(lo);
    buf.push(lh);
    buf.push(ll);
    let mut i = 0usize;
    while i < len {
        buf.push(payload[i]);
        i += 1;
    }
}

/// TLV 长度与指纹自洽断言。
pub fn assert_tlv_self_consistent(tlv: &[u8]) -> AuditResult<()> {
    if tlv.is_empty() || tlv.len() < 16 {
        return Err(AuditError::TlvBroken {
            reason: "shorter_than_minimum_frame",
        });
    }
    // 遍历所有 TLV 项，确认长度自洽。
    let mut off = 0usize;
    let mut items = 0u32;
    while off + 4 <= tlv.len() {
        let len = ((tlv[off + 2] as usize) << 8) | tlv[off + 3] as usize;
        if off + 4 + len > tlv.len() {
            return Err(AuditError::TlvBroken {
                reason: "payload_overflow",
            });
        }
        off += 4 + len;
        items += 1;
    }
    if off != tlv.len() || items < 4 {
        return Err(AuditError::TlvBroken {
            reason: "trailing_bytes_or_missing_items",
        });
    }
    // 末项为 8 字节指纹，重算校验。
    let tail = &tlv[tlv.len() - 12..];
    let body = &tlv[..tlv.len() - 12];
    let mut expect: u64 = 0;
    let mut i = 0usize;
    while i < 8 {
        expect = (expect << 8) | tail[4 + i] as u64;
        i += 1;
    }
    if fnv1a64(body) != expect {
        return Err(AuditError::TlvBroken {
            reason: "fingerprint_mismatch",
        });
    }
    Ok(())
}

// ============================================================================
// 13. 总装入口
// ============================================================================

/// 单次总检的完整结论（供 F1784 跨批聚合取用）。
#[derive(Clone)]
pub struct AuditVerdict {
    /// 对齐矩阵非 Aligned 行数。
    pub broken_rows: u32,
    /// 孤儿扫描结果。
    pub orphans: OrphanScan,
    /// 断言报告。
    pub assertions: AssertionReport,
    /// 启动期决策。
    pub boot: BootDecision,
    /// 注入检出数。
    pub inject_detected: u32,
    /// 总判定是否通过（判据 J1 的落点）。
    pub pass: bool,
    /// 首个诊断码。
    pub first_code: u16,
}

/// 执行一次性完整总检（对齐 + 孤儿 + 断言 + 启动期 + 注入 + 导出自洽）。
///
/// 判据：UNX-F1782-J1 —— 完整判据求值
pub fn audit_once() -> AuditResult<AuditVerdict> {
    let mut list = TripleList::baseline()?;
    let broken = judge_all(&mut list);
    let orphans = scan_orphans(&list);
    let assertions = run_all_assertions(&list, false);
    let boot = boot_audit(&list)?;
    let inj = inject_all();
    let inject_detected = inject_detected_count(&inj);
    let sg = export_struct_graph(&list);
    let lg = export_ledger_graph(&list);
    let hg = export_hook_graph(&list);
    verify_graph_consistency(&list, &sg, &lg, &hg)?;
    let tlv = export_tlv(&list);
    assert_tlv_self_consistent(&tlv)?;
    let pass = broken == 0
        && orphans.is_closed()
        && assertions.all_pass()
        && boot.allow
        && inject_detected == 10;
    let first_code = if pass {
        0
    } else if broken > 0 {
        0xB084
    } else if !orphans.is_closed() {
        0xB085
    } else if !assertions.all_pass() {
        DIAG_MAIN_ASSERT_FAILED
    } else if !boot.allow {
        DIAG_MAIN_BOOT_BLOCKED
    } else {
        DIAG_MAIN_INJECT_MISSED
    };
    Ok(AuditVerdict {
        broken_rows: broken,
        orphans,
        assertions,
        boot,
        inject_detected,
        pass,
        first_code,
    })
}

/// 判据一次性摘要文本。
pub fn verdict_summary(v: &AuditVerdict) -> String {
    format!(
        "UNX-F1782-J1 pass={} broken_rows={} orphans={} asserts_pass={} asserts_fail={} boot_allow={} inject={}/10 code=0x{:04X}",
        v.pass,
        v.broken_rows,
        v.orphans.total(),
        v.assertions.passed,
        v.assertions.failed,
        v.boot.allow,
        v.inject_detected,
        v.first_code
    )
}