//! UNX-F0845 · spurious 中断五态处置账 · 诊断段 0x28B0（独占 0x28B0~0x28BF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md` L2484 · UNX-A2 中断与计时基座 → UNX-A2-B03 ·
//! EOI 全生命周期与 spurious 处置（中断面第 3 批）（F841–F860 · 20 条）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! | UNX-F0845 | spurious 中断五态处置账 | 260 | 已深化 |
//!   UNX-F0845-J1 五态判定正确率 100%（注入各态 10/10 次），处置动作与态匹配零错位 |
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 五态判定正确率 100%：五态（真中断/使能期伪/未使能期伪/风暴期伪/观察）
//!        各注入 10 次共 50 次，每次「期望态（const 规则表纯走查）vs 实判态」
//!        逐条对拍，50/50（= 10000‱）才过；不足一票即红。
//!   J1-b 处置动作与态匹配零错位：处置 const 表是唯一真源（每态↔每处置动作
//!        一一对应），运行时查表分发执行 + 双层错位检测器——
//!        查表层：执行用处置参数 vs 态查表结果；
//!        执行层：处置计数增量指纹 vs 处置参数。
//!        演练全程错位数必须恰为 0。
//!   J1-c 风暴滞回：率窗（256 tick 滑窗）内伪中断密度 ≥64 入风暴（限流处置）、
//!        ≤16 退风暴、17~63 带内保持原态——进出单向，带内零抖动。
//!   J1-d 观察升级单向：观察态逐向量计数达阈 8 → 升级立案（立案账），
//!        立案位只置不清——任何越级降档（撤销立案/回退计数）一律拦截入红账。
//!   J1-e 确定性：同种子双跑（同操作序列两实例）账指纹+状态指纹全等；
//!        事件账 tick 内置自增（调用点不带时间参数）。
//!
//! **建模口径（诚实声明）**：本模块是 spurious 中断处置语义的程序内模型
//! （基于本域 F0805 spurious 语义扩展为五态闭集），不访问真实 LAPIC MMIO
//! （真机路径由 A2 域寄存器面接线）。两条时间轴全部内置：
//!   · 账 tick：事件账 `record` 每入账一条自增 1（审计序）；
//!   · 窗 tick：每次 `arrive()`/`idle()` 推进（到达即节拍——风暴检测关心
//!     到达密度，窗长 256 tick 为任务书规定值）。
//! 不用墙钟，全程确定性。
//!
//! **段位复核记录**：0x2880~0x28BF=unx3401_uktestdesign（SEG_BASE=0x2880、
//! SEG_MASK=0x3F 整段声明，实际占用至 0x28A1）、0x2890~0x289F=unx0826_isrbitmap
//! （本域 F0826 独占）、0x28A0~0x28A3=svstar2/ves510_s28gfuzz（SEG_BASE=0x28A0）、
//! 0x28C0~0x28FF=unx3421_udualbackend；0x28B0~0x28BF 全树 grep 零命中后选定
//! （与 F0826 选段同口径——unx3401 整段声明为掩码解码面，实际未占用本子段）。
//!
//! **自包含纪律**：仅 `use core::…`/`use alloc::…`，禁 `use crate::…`；
//! 内核路径零 unwrap/expect/panic!/运行期 assert!（编译期 `const _: () =
//! assert!` 允许）；可失败全走 `Result<T, SpurFiveError>`，15 变体一码一义，
//! `detail()` 全中文可操作；溢出显式 saturating/wrapping；>64KB 结构体禁上栈
//! （本模块最大栈对象为 768B 快照与 ~1.6KB 引擎本体，均远低于上限）；
//! const fn 只用 while+索引（无 slice::get/Vec 索引/Ord::min/max/derive ==）。
//!
//! **借用纪律（本域实测教训固化，同 F0826）**：账 tick 内置于事件账 `record`
//! 自增，调用点不传时间参数——杜绝 `self.ledger.record(self.tick_next(), ..)`
//! 式双重可变借用（E0499，本域 F0806 残件同类病灶）。红账入账前先取
//! `let tk = self.ledger.tick();` 局部快照，再 `self.reds.push(code, v, tk)`。
//!
//! **误判/错位检出后账面策略（诚实声明）**：注入演练扭曲的是分类/查表环节，
//! 检出即立红项 + 检出计数，**不篡改实况账**——账面如实记录实际执行的处置，
//! 检出另立红账供审计（事后矫正重处置会造成双执行，反而不守恒）。

#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（0x28B0~0x28BF 十六码闭集，一码一义）
// ===========================================================================

/// 段基址（保留码：段标识，不作错误码）。
pub const SEG_BASE: u16 = 0x28B0;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x28BF;

/// 向量号越界（>255）。
pub const X_SF_VEC_RANGE: u16 = 0x28B1;
/// 使能态设置非法（重复设置同态，无变化也应显式上报）。
pub const X_SF_LINE_STATE_BAD: u16 = 0x28B2;
/// 向量槽已占用（重复 claim）。
pub const X_SF_SLOT_BUSY: u16 = 0x28B3;
/// 向量槽未占用（释放/查询空槽）。
pub const X_SF_SLOT_FREE: u16 = 0x28B4;
/// spurious 向量 0xFF 槽禁止占用（保留给仲裁失败报告路径）。
pub const X_SF_SPUR_SLOT_LOCKED: u16 = 0x28B5;
/// 事件账已满（定容拒绝；溢出计入 dropped 降级计数，不静默丢）。
pub const X_SF_LEDGER_FULL: u16 = 0x28B6;
/// 红账已满（定容拒绝）。
pub const X_SF_RED_FULL: u16 = 0x28B7;
/// 立案账已满（定容拒绝）。
pub const X_SF_CASE_FULL: u16 = 0x28B8;
/// 观察计数表已满（32 向量定容）。
pub const X_SF_OBS_TABLE_FULL: u16 = 0x28B9;
/// 态误判检出（注入面扭曲分类，自洽复检器抓获）。
pub const X_SF_MISJUDGE_DETECTED: u16 = 0x28BA;
/// 处置错位检出（处置动作与态不匹配，双层检测器抓获）。
pub const X_SF_DISPO_MISMATCH: u16 = 0x28BB;
/// 越级降档拦截（对已立案向量发起降档，单向闸拒绝）。
pub const X_SF_DOWNGRADE_BLOCKED: u16 = 0x28BC;
/// 快照损坏（魔数/长度/CRC 三闸任一失守）。
pub const X_SF_SNAPSHOT_CORRUPT: u16 = 0x28BD;
/// 快照往返失配（恢复核指纹与源核指纹不一致）。
pub const X_SF_ROUNDTRIP_MISMATCH: u16 = 0x28BE;
/// 双跑对拍失配（同种子两实例指纹不等——确定性破缺）。
pub const X_SF_DUALRUN_MISMATCH: u16 = 0x28BF;

/// 诊断码→中文名（段内 16 码全表；段外归「未知」）。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        0x28B0 => "段基址(保留)",
        0x28B1 => "向量号越界",
        0x28B2 => "使能态设置非法",
        0x28B3 => "向量槽已占用",
        0x28B4 => "向量槽未占用",
        0x28B5 => "spurious槽禁占",
        0x28B6 => "事件账满",
        0x28B7 => "红账满",
        0x28B8 => "立案账满",
        0x28B9 => "观察表满",
        0x28BA => "态误判检出",
        0x28BB => "处置错位检出",
        0x28BC => "越级降档拦截",
        0x28BD => "快照损坏",
        0x28BE => "往返失配",
        0x28BF => "双跑失配",
        _ => "未知诊断码",
    }
}

/// 编译期段闭集自检：基址/顶界命名一致；错误码段 15 码连续覆盖 0x28B1..=0x28BF。
const _: () = assert!(SEG_BASE == 0x28B0 && SEG_TOP == 0x28BF);
const _: () = assert!(X_SF_VEC_RANGE == SEG_BASE + 1);
const _: () = assert!(X_SF_DUALRUN_MISMATCH == SEG_TOP);
const _: () = assert!(
    X_SF_LINE_STATE_BAD == 0x28B2
        && X_SF_SLOT_BUSY == 0x28B3
        && X_SF_SLOT_FREE == 0x28B4
        && X_SF_SPUR_SLOT_LOCKED == 0x28B5
        && X_SF_LEDGER_FULL == 0x28B6
        && X_SF_RED_FULL == 0x28B7
        && X_SF_CASE_FULL == 0x28B8
        && X_SF_OBS_TABLE_FULL == 0x28B9
        && X_SF_MISJUDGE_DETECTED == 0x28BA
        && X_SF_DISPO_MISMATCH == 0x28BB
        && X_SF_DOWNGRADE_BLOCKED == 0x28BC
        && X_SF_SNAPSHOT_CORRUPT == 0x28BD
        && X_SF_ROUNDTRIP_MISMATCH == 0x28BE
);

// ===========================================================================
// 二、错误类型（15 变体，全带中文可操作诊断 + 段内诊断码）
// ===========================================================================

/// 本模块唯一错误类型（一码一义，closed set）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpurFiveError {
    /// 向量号越界（>255）。载荷：违规向量号。
    VecRange(u16),
    /// 使能态设置非法（重复设置同值）。载荷：被拒设置值。
    LineStateBad(bool),
    /// 向量槽已被占用。载荷：向量号。
    SlotBusy(u8),
    /// 向量槽未被占用（释放/降档无对象）。载荷：向量号。
    SlotFree(u8),
    /// spurious 保留槽（0xFF）禁止占用。载荷：向量号。
    SpurSlotLocked(u8),
    /// 事件账满（主入账路径拒绝；溢出降级计数已在账内）。
    LedgerFull,
    /// 红账满。
    RedFull,
    /// 立案账满。
    CaseFull,
    /// 观察计数表满。载荷：被拒向量号。
    ObsTableFull(u8),
    /// 态误判检出。载荷：向量号 / 期望态 tag / 实判态 tag。
    MisjudgeDetected { vec: u8, expect_tag: u8, got_tag: u8 },
    /// 处置错位检出。载荷：态 tag / 期望处置 tag / 实际处置 tag。
    DispoMismatch { state_tag: u8, expect_tag: u8, got_tag: u8 },
    /// 越级降档被单向闸拦截。载荷：向量号。
    DowngradeBlocked(u8),
    /// 快照损坏（魔数/版本/CRC/结构四闸）。载荷：CRC 现值。
    SnapshotCorrupt(u16),
    /// 快照往返失配（核指纹不等）。
    RoundtripMismatch,
    /// 双跑失配。载荷：两实例指纹。
    DualRunMismatch { fp_a: u64, fp_b: u64 },
}

impl SpurFiveError {
    /// 段内诊断码（0x28B1..=0x28BF，一变体一码）。
    pub const fn code(&self) -> u16 {
        match self {
            SpurFiveError::VecRange(_) => X_SF_VEC_RANGE,
            SpurFiveError::LineStateBad(_) => X_SF_LINE_STATE_BAD,
            SpurFiveError::SlotBusy(_) => X_SF_SLOT_BUSY,
            SpurFiveError::SlotFree(_) => X_SF_SLOT_FREE,
            SpurFiveError::SpurSlotLocked(_) => X_SF_SPUR_SLOT_LOCKED,
            SpurFiveError::LedgerFull => X_SF_LEDGER_FULL,
            SpurFiveError::RedFull => X_SF_RED_FULL,
            SpurFiveError::CaseFull => X_SF_CASE_FULL,
            SpurFiveError::ObsTableFull(_) => X_SF_OBS_TABLE_FULL,
            SpurFiveError::MisjudgeDetected { .. } => X_SF_MISJUDGE_DETECTED,
            SpurFiveError::DispoMismatch { .. } => X_SF_DISPO_MISMATCH,
            SpurFiveError::DowngradeBlocked(_) => X_SF_DOWNGRADE_BLOCKED,
            SpurFiveError::SnapshotCorrupt(_) => X_SF_SNAPSHOT_CORRUPT,
            SpurFiveError::RoundtripMismatch => X_SF_ROUNDTRIP_MISMATCH,
            SpurFiveError::DualRunMismatch { .. } => X_SF_DUALRUN_MISMATCH,
        }
    }

    /// 中文可操作诊断（每变体一句：现象 + 排查动作）。
    pub const fn detail(&self) -> &'static str {
        match self {
            SpurFiveError::VecRange(_) => {
                "向量号越界：合法域 0x00..=0xFF；核对投递源（IDT/MSI/LVT）配置后重发"
            }
            SpurFiveError::LineStateBad(_) => {
                "使能态重复设置：SVR 使能位写入与现值相同被拒；先读现态再决定是否写"
            }
            SpurFiveError::SlotBusy(_) => {
                "向量槽已占用：先 release_vector 或换向量；重复 claim 属配置漂移应上报"
            }
            SpurFiveError::SlotFree(_) => {
                "向量槽未占用：对空槽 release/降档无意义；核对槽表与 ISR 残留扫描账"
            }
            SpurFiveError::SpurSlotLocked(_) => {
                "0xFF 为 spurious 保留向量（SDM 10.9.1 惯例）：不可被设备占用；改用 MSI 区向量"
            }
            SpurFiveError::LedgerFull => {
                "事件账满（8192 条定容）：溢出已计 dropped 降级不丢失守恒；消费旧账或扩容重启"
            }
            SpurFiveError::RedFull => {
                "红账满（256 条定容）：违例速率异常，先消费红账定位病灶再继续演练"
            }
            SpurFiveError::CaseFull => {
                "立案账满（128 条定容）：未结案堆积超限；审计结案或调大 CASE_CAP 重建"
            }
            SpurFiveError::ObsTableFull(_) => {
                "观察计数表满（32 向量定容）：可疑向量源过多，先立案收敛在观向量再扩表"
            }
            SpurFiveError::MisjudgeDetected { .. } => {
                "态误判检出：实判态与 const 规则表期望态不符；查注入钩子是否残留或规则表被改"
            }
            SpurFiveError::DispoMismatch { .. } => {
                "处置错位检出：处置动作与态不匹配（查表层或执行层）；核对 DISPOSITION_TABLE 完整性"
            }
            SpurFiveError::DowngradeBlocked(_) => {
                "越级降档被拦截：立案单向（只升不降）；如需翻案走全量重建，不得原地降档"
            }
            SpurFiveError::SnapshotCorrupt(_) => {
                "快照损坏：魔数/版本/头位交叉校验/CRC 四闸失守；弃用该快照，从活账重取"
            }
            SpurFiveError::RoundtripMismatch => {
                "快照往返失配：恢复核指纹与源不等；检查偏移布局版本是否与写入端一致"
            }
            SpurFiveError::DualRunMismatch { .. } => {
                "双跑失配：同种子两实例指纹不等——确定性破缺；核对是否有墙钟/迭代序泄漏"
            }
        }
    }
}

// ===========================================================================
// 三、输入特征（向量 / 使能态 / 率窗 / 签名 → 特征位）
// ===========================================================================

/// spurious 保留向量号。
/// 来源：Intel SDM Vol.3 10.9.1「spurious vector 惯例取 0xFF」+
/// 本域 F0805 `VECTOR_SPURIOUS` 同口径。
pub const SPURIOUS_VECTOR: u8 = 0xFF;

/// 特征位：未使能期（SVR 使能位清零——AP 初始化窗语义）。
pub const FEAT_LINE_DISABLED: u8 = 1 << 0;
/// 特征位：spurious 签名（向量 0xFF 或仲裁失败签名）。
pub const FEAT_SPUR_SIG: u8 = 1 << 1;
/// 特征位：风暴滞回已进入（率窗内伪密度在风暴带）。
pub const FEAT_STORM_ENGAGED: u8 = 1 << 2;
/// 特征位：签名可疑（向量未被占用 或 入口 ISR 位为空——未达立案阈的观察候选）。
pub const FEAT_SUSPICIOUS: u8 = 1 << 3;

/// 到达签名（一次中断到达的硬件可观测特征，纯数据）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrivalSig {
    /// 到达向量号。
    pub vector: u8,
    /// 入口时刻 ISR 位是否已置位（真服务的硬签名：服务中位图必须有位）。
    pub isr_set_on_entry: bool,
    /// 仲裁失败签名（本地 APIC 使能但仲裁窗内输给更高优先级——SDM spurious 语义）。
    pub arb_loss: bool,
}

impl ArrivalSig {
    /// 构造一个到达签名。
    pub const fn new(vector: u8, isr_set_on_entry: bool, arb_loss: bool) -> Self {
        ArrivalSig { vector, isr_set_on_entry, arb_loss }
    }

    /// 是否携带 spurious 签名（0xFF 向量 或 仲裁失败）。
    pub const fn has_spur_sig(&self) -> bool {
        self.vector == SPURIOUS_VECTOR || self.arb_loss
    }

    /// 渲染签名（审计用）。
    pub fn render(&self) -> String {
        format!(
            "sig{{vec=0x{:02X}, isr={}, arb={}}}",
            self.vector, self.isr_set_on_entry, self.arb_loss
        )
    }
}

/// 分类输入特征（引擎在分类瞬间可观测的全部特征快照）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Features {
    /// 向量号。
    pub vector: u8,
    /// 本地 APIC 使能态（false=未使能期/AP 初始化窗）。
    pub line_enabled: bool,
    /// 向量槽是否已被占用（claim 过）。
    pub vector_claimed: bool,
    /// 入口 ISR 位置位签名。
    pub isr_set_on_entry: bool,
    /// 仲裁失败签名。
    pub arb_loss: bool,
    /// 风暴滞回当前是否处于风暴带。
    pub storm_engaged: bool,
}

impl Features {
    /// 由特征算判定特征位（const——规则表与运行时共用同一折叠函数）。
    pub const fn feature_bits(&self) -> u8 {
        let mut bits: u8 = 0;
        if !self.line_enabled {
            bits |= FEAT_LINE_DISABLED;
        }
        if self.vector == SPURIOUS_VECTOR || self.arb_loss {
            bits |= FEAT_SPUR_SIG;
        }
        if self.storm_engaged {
            bits |= FEAT_STORM_ENGAGED;
        }
        if !self.vector_claimed || !self.isr_set_on_entry {
            bits |= FEAT_SUSPICIOUS;
        }
        bits
    }
}

// ===========================================================================
// 四、五态闭集 + const 判定规则表
// ===========================================================================

/// 五态闭集（spurious 处置域的全部可达态；tag 0..=4 恒定，作数组索引）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpurState {
    /// 真中断：签名完备（已占用向量 + 入口 ISR 位置位 + 非 spurious 签名 +
    /// 使能期）——正常服务 + EOI 放行。
    Real,
    /// 使能期伪中断：spurious 签名但使能期非风暴——仲裁失败只入账拒 EOI。
    SpurEnabled,
    /// 未使能期伪中断：AP 初始化窗（SVR 未使能）到达——配置级异常直接立案。
    SpurDisabled,
    /// 风暴期伪中断：率窗密度超阈滞回带内——计数限流（采样审计）。
    SpurStorm,
    /// 观察态：签名可疑但未达立案阈——观察留痕（达阈升格立案）。
    Observing,
}

impl SpurState {
    /// 态 tag（数组索引用，0..=4）。
    pub const fn tag(&self) -> u8 {
        match self {
            SpurState::Real => 0,
            SpurState::SpurEnabled => 1,
            SpurState::SpurDisabled => 2,
            SpurState::SpurStorm => 3,
            SpurState::Observing => 4,
        }
    }

    /// 中文名。
    pub const fn name(&self) -> &'static str {
        match self {
            SpurState::Real => "真中断",
            SpurState::SpurEnabled => "使能期伪中断",
            SpurState::SpurDisabled => "未使能期伪中断",
            SpurState::SpurStorm => "风暴期伪中断",
            SpurState::Observing => "观察态",
        }
    }

    /// 按 tag 还原（tag 越界归 None——闭集外不存在第六态）。
    pub fn from_tag(tag: u8) -> Option<SpurState> {
        match tag {
            0 => Option::Some(SpurState::Real),
            1 => Option::Some(SpurState::SpurEnabled),
            2 => Option::Some(SpurState::SpurDisabled),
            3 => Option::Some(SpurState::SpurStorm),
            4 => Option::Some(SpurState::Observing),
            _ => Option::None,
        }
    }

    /// 该态是否计入率窗（「非真即入窗」：一切未按真中断服务的到达都喂给
    /// 风暴密度估计——Real 排除，其余四态计入）。
    pub const fn feeds_window(&self) -> bool {
        !matches!(*self, SpurState::Real)
    }
}

/// 单条判定规则（req_mask 全命中的特征位组 → 目标态）。
#[derive(Debug, Clone, Copy)]
pub struct DecideRule {
    /// 优先级（表内从 0 升序，首中即返回）。
    pub priority: u8,
    /// 规则名（审计渲染用）。
    pub name: &'static str,
    /// 要求命中的特征位掩码（与折叠特征位按位与后等于自身即命中）。
    pub req_mask: u8,
    /// 命中后的目标态。
    pub target: SpurState,
}

/// 五态 const 判定规则表（唯一真源：优先级 P0 最高，首中即判）。
///
/// | 优先级 | 规则 | 条件（特征位全命中） | 目标态 |
/// |---|---|---|---|
/// | P0 | 未使能期 | LINE_DISABLED | SpurDisabled |
/// | P1 | 风暴期伪 | STORM_ENGAGED ∧ SPUR_SIG | SpurStorm |
/// | P2 | 使能期伪 | SPUR_SIG | SpurEnabled |
/// | P3 | 观察 | SUSPICIOUS | Observing |
/// | P4 | 真中断（默认） | （无条件） | Real |
///
/// 语义要点：未使能期最高优先（初始化窗独立诊断，不看签名）；风暴只
/// 作用于 spurious 签名流（真签名完备的到达即使在风暴带也正常服务）；
/// 签名可疑（向量未占用/入口 ISR 空）但其余正常 → 观察留痕；全完备 → 真。
pub const DECIDE_TABLE: [DecideRule; 5] = [
    DecideRule {
        priority: 0,
        name: "P0 未使能期(AP初始化窗)",
        req_mask: FEAT_LINE_DISABLED,
        target: SpurState::SpurDisabled,
    },
    DecideRule {
        priority: 1,
        name: "P1 风暴期伪(密度超阈滞回带)",
        req_mask: FEAT_STORM_ENGAGED | FEAT_SPUR_SIG,
        target: SpurState::SpurStorm,
    },
    DecideRule {
        priority: 2,
        name: "P2 使能期伪(仲裁失败签名)",
        req_mask: FEAT_SPUR_SIG,
        target: SpurState::SpurEnabled,
    },
    DecideRule {
        priority: 3,
        name: "P3 观察(签名可疑未达立案阈)",
        req_mask: FEAT_SUSPICIOUS,
        target: SpurState::Observing,
    },
    DecideRule {
        priority: 4,
        name: "P4 真中断(默认/签名完备)",
        req_mask: 0,
        target: SpurState::Real,
    },
];

/// 规则表走查（const fn；while+数组索引，首中即返回）。
/// P4 的 req_mask=0 恒命中——保证闭集完备（任意特征位必有态）。
pub const fn decide_from_bits(bits: u8) -> SpurState {
    let mut i: usize = 0;
    while i < 5 {
        let rule = DECIDE_TABLE[i];
        if (bits & rule.req_mask) == rule.req_mask {
            return rule.target;
        }
        i += 1;
    }
    // 不可达（P4 无条件命中）；返回 Real 作编译期安全网。
    SpurState::Real
}

/// 按特征直接判态（特征 → 位折叠 → 表走查）。
pub const fn decide(f: Features) -> SpurState {
    decide_from_bits(f.feature_bits())
}

/// 命中规则名（审计渲染：返回首中规则的名称）。
pub const fn rule_name_of(bits: u8) -> &'static str {
    let mut i: usize = 0;
    while i < 5 {
        let rule = DECIDE_TABLE[i];
        if (bits & rule.req_mask) == rule.req_mask {
            return rule.name;
        }
        i += 1;
    }
    "P4 真中断(默认/签名完备)"
}

/// 编译期规则表自检：优先级 0..=4 升序；五目标态互异；P4 无条件兜底。
const _: () = assert!(DECIDE_TABLE.len() == 5);
const _: () = assert!(DECIDE_TABLE[0].priority == 0 && DECIDE_TABLE[1].priority == 1);
const _: () = assert!(DECIDE_TABLE[2].priority == 2 && DECIDE_TABLE[3].priority == 3);
const _: () = assert!(DECIDE_TABLE[4].priority == 4);
const _: () = assert!(DECIDE_TABLE[0].target.tag() == SpurState::SpurDisabled.tag());
const _: () = assert!(DECIDE_TABLE[1].target.tag() == SpurState::SpurStorm.tag());
const _: () = assert!(DECIDE_TABLE[2].target.tag() == SpurState::SpurEnabled.tag());
const _: () = assert!(DECIDE_TABLE[3].target.tag() == SpurState::Observing.tag());
const _: () = assert!(DECIDE_TABLE[4].target.tag() == SpurState::Real.tag());
const _: () = assert!(DECIDE_TABLE[4].req_mask == 0);
// 编译期五态闭集完备性抽查：五个代表性特征位组各得一态且互异。
const _: () = assert!(decide_from_bits(FEAT_LINE_DISABLED).tag() == 2);
const _: () = assert!(decide_from_bits(FEAT_STORM_ENGAGED | FEAT_SPUR_SIG).tag() == 3);
const _: () = assert!(decide_from_bits(FEAT_SPUR_SIG).tag() == 1);
const _: () = assert!(decide_from_bits(FEAT_SUSPICIOUS).tag() == 4);
const _: () = assert!(decide_from_bits(0).tag() == 0);

// ===========================================================================
// 五、处置闭集 + const 处置映射表 + 双层错位检测器
// ===========================================================================

/// 处置动作闭集（每态恰好一个动作——零错位的一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// EOI 放行（正常服务收尾——真中断专属）。
    EoiPass,
    /// 入账拒 EOI（SDM spurious 语义：不发 EOI，只入账）。
    RecordNoEoi,
    /// 计数限流（风暴期：全量计数 + 1/4 采样审计）。
    RateLimit,
    /// 观察留痕（签名可疑未立案：逐向量计数，达阈升格）。
    ObserveTrail,
    /// 升级立案（未使能期异常直接立案；观察升格也走本动作语义）。
    EscalateFile,
}

impl Disposition {
    /// 处置 tag（0..=4，作计数数组索引）。
    pub const fn tag(&self) -> u8 {
        match self {
            Disposition::EoiPass => 0,
            Disposition::RecordNoEoi => 1,
            Disposition::RateLimit => 2,
            Disposition::ObserveTrail => 3,
            Disposition::EscalateFile => 4,
        }
    }

    /// 中文名。
    pub const fn name(&self) -> &'static str {
        match self {
            Disposition::EoiPass => "EOI放行",
            Disposition::RecordNoEoi => "入账拒EOI",
            Disposition::RateLimit => "计数限流",
            Disposition::ObserveTrail => "观察留痕",
            Disposition::EscalateFile => "升级立案",
        }
    }

    /// 是否发 EOI（处置语义核心：只有 EoiPass 放行 EOI——其余四类一律拒发）。
    pub const fn issues_eoi(&self) -> bool {
        matches!(*self, Disposition::EoiPass)
    }

    /// 按 tag 还原（闭集外 None）。
    pub fn from_tag(tag: u8) -> Option<Disposition> {
        match tag {
            0 => Option::Some(Disposition::EoiPass),
            1 => Option::Some(Disposition::RecordNoEoi),
            2 => Option::Some(Disposition::RateLimit),
            3 => Option::Some(Disposition::ObserveTrail),
            4 => Option::Some(Disposition::EscalateFile),
            _ => Option::None,
        }
    }
}

/// const 处置映射表（唯一真源：按态 tag 索引，一态一动作）。
///
/// | 态 | 处置 |
/// |---|---|
/// | Real | EoiPass（EOI 放行 + 正常服务） |
/// | SpurEnabled | RecordNoEoi（入账拒 EOI——SDM spurious 不发 EOI） |
/// | SpurDisabled | EscalateFile（AP 初始化窗到达=配置级异常，直接立案） |
/// | SpurStorm | RateLimit（计数限流——风暴带采样审计） |
/// | Observing | ObserveTrail（观察留痕——达阈由升格器单向升格） |
pub const DISPOSITION_TABLE: [Disposition; 5] = [
    Disposition::EoiPass,
    Disposition::RecordNoEoi,
    Disposition::EscalateFile,
    Disposition::RateLimit,
    Disposition::ObserveTrail,
];

/// 处置映射的任务书命名面（DispositionMap = DISPOSITION_TABLE 的别名——
/// 「每态→处置动作」const 映射的正式类型名）。
pub type DispositionMap = [Disposition; 5];

/// 处置映射的规范实例（= DISPOSITION_TABLE；审计代码引用本名以对齐任务书）。
pub const DISPOSITION_MAP: DispositionMap = DISPOSITION_TABLE;

/// 运行时查表（闭集内恒有值；tag 由 SpurState::tag() 保证 0..=4）。
pub const fn disposition_of(state: SpurState) -> Disposition {
    DISPOSITION_TABLE[state.tag() as usize]
}

/// 处置名表（计数渲染用，按处置 tag 索引）。
pub const DISP_NAMES: [&str; 5] = [
    Disposition::EoiPass.name(),
    Disposition::RecordNoEoi.name(),
    Disposition::RateLimit.name(),
    Disposition::ObserveTrail.name(),
    Disposition::EscalateFile.name(),
];

/// 态名表（按态 tag 索引）。
pub const STATE_NAMES: [&str; 5] = [
    SpurState::Real.name(),
    SpurState::SpurEnabled.name(),
    SpurState::SpurDisabled.name(),
    SpurState::SpurStorm.name(),
    SpurState::Observing.name(),
];

/// 处置计数器（按处置 tag 的定长计数组——执行层错位检测的增量指纹基底）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispCounters {
    /// 按处置 tag 索引的执行计数（[EOI放行, 入账拒EOI, 计数限流, 观察留痕, 升级立案]）。
    pub v: [u64; 5],
}

impl DispCounters {
    /// 全零。
    pub const fn zero() -> Self {
        DispCounters { v: [0; 5] }
    }

    /// 单处置计数。
    pub const fn get(&self, d: Disposition) -> u64 {
        self.v[d.tag() as usize]
    }

    /// 单处置计数自增（saturating）。
    pub fn inc(&mut self, d: Disposition) {
        let i = d.tag() as usize;
        self.v[i] = self.v[i].saturating_add(1);
    }

    /// 两计数组差分（逐槽 saturating_sub——正增量指纹；错位检测用）。
    pub const fn delta_from(&self, before: &DispCounters) -> DispCounters {
        let mut out = DispCounters { v: [0; 5] };
        let mut i: usize = 0;
        while i < 5 {
            out.v[i] = self.v[i].saturating_sub(before.v[i]);
            i += 1;
        }
        out
    }

    /// 增量指纹是否符合处置 d 的唯一触点（执行层错位检测）：
    /// 正确执行当且仅当：d 槽增量恰 1，其余槽增量恰 0。
    pub const fn delta_matches(&self, d: Disposition) -> bool {
        let mut i: usize = 0;
        while i < 5 {
            let want: u64 = if i == d.tag() as usize { 1 } else { 0 };
            if self.v[i] != want {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 总执行数（守恒式对账：应等于到达总数）。
    pub const fn total(&self) -> u64 {
        self.v[0]
            .saturating_add(self.v[1])
            .saturating_add(self.v[2])
            .saturating_add(self.v[3])
            .saturating_add(self.v[4])
    }

    /// 折叠指纹（FNV）。
    pub const fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < 5 {
            h = (h ^ self.v[i]).wrapping_mul(0x0100_0000_01b3);
            i += 1;
        }
        h
    }
}

/// 编译期处置表自检：一态一动作（零错位的不变式写死在编译期）。
const _: () = assert!(DISPOSITION_TABLE.len() == 5);
const _: () = assert!(DISPOSITION_TABLE[0].tag() == Disposition::EoiPass.tag());
const _: () = assert!(DISPOSITION_TABLE[1].tag() == Disposition::RecordNoEoi.tag());
const _: () = assert!(DISPOSITION_TABLE[2].tag() == Disposition::EscalateFile.tag());
const _: () = assert!(DISPOSITION_TABLE[3].tag() == Disposition::RateLimit.tag());
const _: () = assert!(DISPOSITION_TABLE[4].tag() == Disposition::ObserveTrail.tag());
// EOI 唯一性：五处置中只有 EoiPass 放行 EOI（拒 EOI 红线的静态证明）。
const _: () = assert!(Disposition::EoiPass.issues_eoi());
const _: () = assert!(!Disposition::RecordNoEoi.issues_eoi());
const _: () = assert!(!Disposition::RateLimit.issues_eoi());
const _: () = assert!(!Disposition::ObserveTrail.issues_eoi());
const _: () = assert!(!Disposition::EscalateFile.issues_eoi());

// ===========================================================================
// 六、风暴窗（256 tick 滑窗 + 阈值 + 进出滞回）
// ===========================================================================

/// 率窗长 256 tick。
/// 来源：任务书 UNX-F0845「率窗 256 tick 滑窗」规定值（2 的幂——槽掩码直切）。
pub const STORM_WINDOW_TICKS: usize = 256;
/// 窗槽掩码（256 tick → 0xFF）。
pub const STORM_SLOT_MASK: usize = 0xFF;

/// 入风暴阈值（窗内伪中断密度 ≥64 立案风暴）。
/// 来源：任务书「率超阈值」+ 本域 F0849 风暴口径对齐，取窗长 1/4。
pub const STORM_ENTER: u32 = 64;
/// 退风暴阈值（窗内密度 ≤16 退风暴——滞回带 17~63 保持原态）。
/// 来源：滞回防抖惯例（进出比 4:1），同批 F0841 去抖口径对齐。
pub const STORM_EXIT: u32 = 16;

/// 入风暴判定（const：密度 ≥ 入阈）。
pub const fn enter_would(rate: u32) -> bool {
    rate >= STORM_ENTER
}
/// 退风暴判定（const：密度 ≤ 出阈）。
pub const fn exit_would(rate: u32) -> bool {
    rate <= STORM_EXIT
}

/// 风暴滑窗（环形槽：每 tick 一槽的伪中断计数；滞回锁存）。
///
/// 时间轴纪律：窗 tick 由引擎每次 `arrive()`/`idle()` 推进（到达即节拍），
/// 与事件账 tick 相互独立（账 tick=审计序，窗 tick=密度轴）——两者均内置，
/// 调用点不传任何时间参数。
#[derive(Debug, Clone)]
pub struct StormWindow {
    /// 环形槽（每槽=该窗 tick 内计入的伪中断到达数）。
    slots: [u16; STORM_WINDOW_TICKS],
    /// 窗推进总数（head = ticks & 0xFF）。
    ticks: u64,
    /// 窗内现存总量（= 率）。
    total: u32,
    /// 风暴滞回锁存（true=风暴带内）。
    latched: bool,
    /// 入风暴次数。
    enters: u32,
    /// 退风暴次数。
    exits: u32,
}

impl StormWindow {
    /// 全零窗（const 可构造——[0; 256] 字面量初始化）。
    pub const fn new() -> Self {
        StormWindow {
            slots: [0; STORM_WINDOW_TICKS],
            ticks: 0,
            total: 0,
            latched: false,
            enters: 0,
            exits: 0,
        }
    }

    /// 当前槽索引（head）。
    pub const fn head(&self) -> usize {
        (self.ticks as usize) & STORM_SLOT_MASK
    }

    /// 窗推进总数（只读）。
    pub const fn ticks(&self) -> u64 {
        self.ticks
    }

    /// 窗内密度（= 率，只读）。
    pub const fn rate(&self) -> u32 {
        self.total
    }

    /// 滞回锁存态（只读）。
    pub const fn latched(&self) -> bool {
        self.latched
    }

    /// 入/退风暴累计（只读，(enters, exits)）。
    pub const fn transitions(&self) -> (u32, u32) {
        (self.enters, self.exits)
    }

    /// 推进一 tick：淘汰最旧槽（新 head 槽清零并从总量扣减）。
    pub fn advance(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
        let idx = self.head();
        let evicted = self.slots[idx] as u32;
        self.total = self.total.saturating_sub(evicted);
        self.slots[idx] = 0;
    }

    /// 当前 tick 计入一次（advance 之后调用；槽计数 u16 封顶防溢出）。
    pub fn add_current(&mut self) {
        let idx = self.head();
        if self.slots[idx] < 0xFFFF {
            self.slots[idx] += 1;
        }
        self.total = self.total.saturating_add(1);
    }

    /// 滞回评估（每次推进后调用；进出互斥，带内不动）：
    /// 返回 Some(true)=本次入风暴，Some(false)=本次退风暴，None=保持。
    pub fn evaluate(&mut self) -> Option<bool> {
        if !self.latched && enter_would(self.total) {
            self.latched = true;
            self.enters = self.enters.saturating_add(1);
            Option::Some(true)
        } else if self.latched && exit_would(self.total) {
            self.latched = false;
            self.exits = self.exits.saturating_add(1);
            Option::Some(false)
        } else {
            Option::None
        }
    }

    /// 闲置推进 n tick（无到达时间流逝——淘汰最旧槽使密度自然衰减）。
    /// 返回推进后的窗率。
    pub fn idle(&mut self, n: u32) -> u32 {
        let mut i: u32 = 0;
        while i < n {
            self.advance();
            i += 1;
        }
        self.total
    }

    /// 窗指纹（槽平面 + 全字段 FNV 折叠；确定性）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = (h ^ self.ticks).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ (self.total as u64)).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ (self.latched as u64)).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ (self.enters as u64)).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ (self.exits as u64)).wrapping_mul(0x0100_0000_01b3);
        let mut i: usize = 0;
        while i < STORM_WINDOW_TICKS {
            h = (h ^ (self.slots[i] as u64)).wrapping_mul(0x0100_0000_01b3);
            i += 1;
        }
        h
    }
}

// 编译期滞回自检：入阈边界恰触发、阈下不触发；出阈边界恰触发、带上不触发；
// 滞回带非空（17..63 存在——进出阈之间必须有带，否则滞回无意义）。
const _: () = assert!(enter_would(STORM_ENTER) && !enter_would(STORM_ENTER - 1));
const _: () = assert!(exit_would(STORM_EXIT) && !exit_would(STORM_EXIT + 1));
const _: () = assert!(STORM_EXIT < STORM_ENTER);

// ===========================================================================
// 七、向量位平面 + 观察升格器 + 立案账（单向闸）
// ===========================================================================

/// 256 向量位平面（占用表/立案表共用结构；[u64; 4] 字平面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitPlane256 {
    words: [u64; 4],
}

impl BitPlane256 {
    /// 全零平面。
    pub const fn zero() -> Self {
        BitPlane256 { words: [0; 4] }
    }

    /// 置位（已置则 false——幂等拒绝）。
    pub fn set(&mut self, vec: u8) -> bool {
        let (w, b) = Self::split(vec);
        if (self.words[w] & b) != 0 {
            return false;
        }
        self.words[w] |= b;
        true
    }

    /// 清位（已清则 false）。
    pub fn clear(&mut self, vec: u8) -> bool {
        let (w, b) = Self::split(vec);
        if (self.words[w] & b) == 0 {
            return false;
        }
        self.words[w] &= !b;
        true
    }

    /// 查位。
    pub const fn test(&self, vec: u8) -> bool {
        let (w, b) = Self::split(vec);
        (self.words[w] & b) != 0
    }

    /// 置位总数。
    pub const fn count(&self) -> u32 {
        let mut total: u32 = 0;
        let mut w: usize = 0;
        while w < 4 {
            let mut x = self.words[w];
            while x != 0 {
                x &= x.wrapping_sub(1);
                total += 1;
            }
            w += 1;
        }
        total
    }

    /// 字平面拆位（const：字索引与位掩码）。
    pub const fn split(vec: u8) -> (usize, u64) {
        let idx = (vec >> 6) as usize;
        let bit = 1u64 << (vec & 0x3F);
        (idx, bit)
    }

    /// 序列化（32 字节定长）。
    pub const fn to_bytes(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        let mut w: usize = 0;
        while w < 4 {
            let x = self.words[w];
            out[w * 8] = (x & 0xFF) as u8;
            out[w * 8 + 1] = ((x >> 8) & 0xFF) as u8;
            out[w * 8 + 2] = ((x >> 16) & 0xFF) as u8;
            out[w * 8 + 3] = ((x >> 24) & 0xFF) as u8;
            out[w * 8 + 4] = ((x >> 32) & 0xFF) as u8;
            out[w * 8 + 5] = ((x >> 40) & 0xFF) as u8;
            out[w * 8 + 6] = ((x >> 48) & 0xFF) as u8;
            out[w * 8 + 7] = ((x >> 56) & 0xFF) as u8;
            w += 1;
        }
        out
    }

    /// 反序列化（逐字节拼装，无失败路径——定长 32 字节）。
    pub const fn from_bytes(bytes: &[u8; 32]) -> Self {
        let mut words = [0u64; 4];
        let mut w: usize = 0;
        while w < 4 {
            let mut x: u64 = 0;
            x |= bytes[w * 8] as u64;
            x |= (bytes[w * 8 + 1] as u64) << 8;
            x |= (bytes[w * 8 + 2] as u64) << 16;
            x |= (bytes[w * 8 + 3] as u64) << 24;
            x |= (bytes[w * 8 + 4] as u64) << 32;
            x |= (bytes[w * 8 + 5] as u64) << 40;
            x |= (bytes[w * 8 + 6] as u64) << 48;
            x |= (bytes[w * 8 + 7] as u64) << 56;
            words[w] = x;
            w += 1;
        }
        BitPlane256 { words }
    }

    /// 平面指纹（字序 FNV）。
    pub const fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut w: usize = 0;
        while w < 4 {
            h = (h ^ self.words[w]).wrapping_mul(0x0100_0000_01b3);
            w += 1;
        }
        h
    }
}

/// 观察立案阈（逐向量观察计数达 8 → 升格立案）。
/// 来源：任务书「观察计数达阈→升级立案」；取 8 使 10 次注入内可跨阈演示。
pub const OBS_FILE_THRESHOLD: u16 = 8;
/// 观察计数表容量（定容 32 向量——内核定容纪律，防观察面无界生长）。
pub const OBS_CAP: usize = 32;
/// 立案账容量（定容 128 案）。
pub const CASE_CAP: usize = 128;
/// 立案理由：未使能期异常直接立案。
pub const CASE_REASON_DISABLED: u8 = 1;
/// 立案理由：观察升格立案。
pub const CASE_REASON_OBSERVE: u8 = 2;

/// 立案条目（立案账元素——只增不减，单向账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaseEntry {
    /// 立案号（单调，从 1 起）。
    pub id: u64,
    /// 向量号。
    pub vec: u8,
    /// 立案理由（CASE_REASON_DISABLED / CASE_REASON_OBSERVE）。
    pub reason: u8,
    /// 立案时的账 tick（账时间轴——入账方传入）。
    pub tick: u64,
}

/// 观察升格器 + 立案账 + 单向闸（立案位只置不清）。
#[derive(Debug, Clone)]
pub struct ObsEscalator {
    /// 在观向量逐条计数（vec, count；定容 OBS_CAP，插入序稳定——确定性）。
    counts: Vec<(u8, u16)>,
    /// 立案位平面（一旦置位永不清——单向闸的物质基础）。
    filed: BitPlane256,
    /// 立案账（只追加）。
    cases: Vec<CaseEntry>,
    /// 下一立案号。
    next_id: u64,
    /// 观察升格触发次数（新立案计数）。
    escalations: u64,
    /// 同向量重复立案拦截次数（dedup——已立案向量再触阈不重复立）。
    dedup_blocks: u64,
    /// 降档拦截次数。
    downgrade_blocks: u64,
}

impl ObsEscalator {
    /// 空升格器。
    pub fn new() -> Self {
        ObsEscalator {
            counts: Vec::new(),
            filed: BitPlane256::zero(),
            cases: Vec::new(),
            next_id: 1,
            escalations: 0,
            dedup_blocks: 0,
            downgrade_blocks: 0,
        }
    }

    /// 查在观计数（不在观=0）。
    pub fn obs_count(&self, vec: u8) -> u16 {
        let mut i: usize = 0;
        while i < self.counts.len() {
            if self.counts[i].0 == vec {
                return self.counts[i].1;
            }
            i += 1;
        }
        0
    }

    /// 是否已立案。
    pub const fn is_filed(&self, vec: u8) -> bool {
        self.filed.test(vec)
    }

    /// 立案总数。
    pub const fn case_count(&self) -> u64 {
        self.cases.len() as u64
    }

    /// 观察升格次数（只读）。
    pub const fn escalations(&self) -> u64 {
        self.escalations
    }

    /// 降档拦截计数（只读）。
    pub const fn downgrade_blocks(&self) -> u64 {
        self.downgrade_blocks
    }

    /// 在观向量快照（插入序——确定性渲染/对拍用）。
    pub fn obs_snapshot(&self) -> Vec<(u8, u16)> {
        let mut out = Vec::new();
        let mut i: usize = 0;
        while i < self.counts.len() {
            out.push(self.counts[i]);
            i += 1;
        }
        out
    }

    /// 立案账快照（裁到最近 n 条；n=0 取全账）。
    pub fn cases_snapshot(&self, n: usize) -> Vec<CaseEntry> {
        let mut out = Vec::new();
        if n == 0 || self.cases.len() <= n {
            let mut i: usize = 0;
            while i < self.cases.len() {
                out.push(self.cases[i]);
                i += 1;
            }
        } else {
            let start = self.cases.len() - n;
            let mut i: usize = start;
            while i < self.cases.len() {
                out.push(self.cases[i]);
                i += 1;
            }
        }
        out
    }

    /// 内部立案（单向：置位+追加账；容量满报 CaseFull）。
    fn file(&mut self, vec: u8, reason: u8, tick: u64) -> Result<CaseEntry, SpurFiveError> {
        if self.cases.len() >= CASE_CAP {
            return Err(SpurFiveError::CaseFull);
        }
        if !self.filed.set(vec) {
            // 位平面已置——dedup 闸（逻辑上外层已拦，这里是第二道保险）。
            self.dedup_blocks = self.dedup_blocks.saturating_add(1);
        }
        let entry = CaseEntry { id: self.next_id, vec, reason, tick };
        self.next_id = self.next_id.wrapping_add(1);
        self.cases.push(entry);
        self.escalations = self.escalations.saturating_add(1);
        Ok(entry)
    }

    /// 观察一步（Observing 处置的执行体）：
    /// 计数 +1；达阈且未立案 → 升格立案（返回 Some(立案条目)）；
    /// 已立案再观察 → dedup（计数继续，不重复立）；表满 → Err。
    pub fn observe(&mut self, vec: u8, tick: u64) -> Result<Option<CaseEntry>, SpurFiveError> {
        // 找现有计数槽。
        let mut idx: isize = -1;
        let mut i: usize = 0;
        while i < self.counts.len() {
            if self.counts[i].0 == vec {
                idx = i as isize;
                break;
            }
            i += 1;
        }
        let new_count: u16;
        if idx < 0 {
            // 新在观向量：表满即报（fail-fast，不静默丢观察）。
            if self.counts.len() >= OBS_CAP {
                return Err(SpurFiveError::ObsTableFull(vec));
            }
            self.counts.push((vec, 1));
            new_count = 1;
        } else {
            let slot = self.counts[idx as usize];
            new_count = slot.1.saturating_add(1);
            self.counts[idx as usize] = (vec, new_count);
        }
        // 达阈升格（单向：只升不降）。
        if new_count >= OBS_FILE_THRESHOLD && !self.filed.test(vec) {
            let entry = self.file(vec, CASE_REASON_OBSERVE, tick)?;
            return Ok(Option::Some(entry));
        }
        Ok(Option::None)
    }

    /// 未使能期直接立案（SpurDisabled 处置执行体）：
    /// 已立案 → dedup（Ok(None)）；新立案 → Ok(Some(条目))。
    pub fn file_disabled(&mut self, vec: u8, tick: u64) -> Result<Option<CaseEntry>, SpurFiveError> {
        if self.filed.test(vec) {
            self.dedup_blocks = self.dedup_blocks.saturating_add(1);
            return Ok(Option::None);
        }
        Ok(Option::Some(self.file(vec, CASE_REASON_DISABLED, tick)?))
    }

    /// 越级降档尝试（单向闸演示面 + 注入面）：
    /// 已立案向量 → 拦截（Err(DowngradeBlocked) + 拦截计数）；
    /// 未立案向量 → 无降档对象（Err(SlotFree)）。
    /// 本函数永不 Ok——降档在语义上不存在合法路径（单向账红线的可执行证明）。
    pub fn try_downgrade(&mut self, vec: u8) -> Result<(), SpurFiveError> {
        if self.filed.test(vec) {
            self.downgrade_blocks = self.downgrade_blocks.saturating_add(1);
            Err(SpurFiveError::DowngradeBlocked(vec))
        } else {
            Err(SpurFiveError::SlotFree(vec))
        }
    }

    /// 升格器指纹（计数表 + 立案平面 + 立案账 + 全计数字段 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = (h ^ self.filed.fingerprint()).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.next_id).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.escalations).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.dedup_blocks).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.downgrade_blocks).wrapping_mul(0x0100_0000_01b3);
        let mut i: usize = 0;
        while i < self.counts.len() {
            h = (h ^ (self.counts[i].0 as u64)).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ (self.counts[i].1 as u64)).wrapping_mul(0x0100_0000_01b3);
            i += 1;
        }
        let mut j: usize = 0;
        while j < self.cases.len() {
            let c = self.cases[j];
            h = (h ^ c.id).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ (c.vec as u64)).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ (c.reason as u64)).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ c.tick).wrapping_mul(0x0100_0000_01b3);
            j += 1;
        }
        h
    }
}

// ===========================================================================
// 八、事件账（tick 内置）+ 红账
// ===========================================================================

/// 事件种类闭集（tag 1..=16；0 保留作「空」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvKind {
    /// 中断到达（每次 arrive 一条；flags=0）。
    Arrive,
    /// 判态落账（flags=态 tag | 误判位 0x80）。
    Classify,
    /// EOI 放行执行。
    EoiPass,
    /// 入账拒 EOI 执行。
    RecordNoEoi,
    /// 限流采样（第 N 个全审计）。
    RateLimitSample,
    /// 限流跳过（仅计数，不展开审计）。
    RateLimitSkip,
    /// 观察留痕执行。
    Observe,
    /// 观察升格立案。
    Escalate,
    /// 未使能期直接立案。
    CaseFile,
    /// 入风暴（滞回跨越）。
    StormEnter,
    /// 退风暴（滞回跨越）。
    StormExit,
    /// 闲置推进（flags=min(n,255)）。
    Idle,
    /// 误判检出红标。
    MisjudgeHit,
    /// 错位检出红标。
    MismatchHit,
    /// 降档拦截红标。
    DowngradeHit,
    /// 注入钩子装定。
    InjectArm,
}

impl EvKind {
    /// 事件 tag。
    pub const fn tag(&self) -> u8 {
        match self {
            EvKind::Arrive => 1,
            EvKind::Classify => 2,
            EvKind::EoiPass => 3,
            EvKind::RecordNoEoi => 4,
            EvKind::RateLimitSample => 5,
            EvKind::RateLimitSkip => 6,
            EvKind::Observe => 7,
            EvKind::Escalate => 8,
            EvKind::CaseFile => 9,
            EvKind::StormEnter => 10,
            EvKind::StormExit => 11,
            EvKind::Idle => 12,
            EvKind::MisjudgeHit => 13,
            EvKind::MismatchHit => 14,
            EvKind::DowngradeHit => 15,
            EvKind::InjectArm => 16,
        }
    }

    /// 中文名。
    pub const fn name(&self) -> &'static str {
        match self {
            EvKind::Arrive => "到达",
            EvKind::Classify => "判态",
            EvKind::EoiPass => "EOI放行",
            EvKind::RecordNoEoi => "入账拒EOI",
            EvKind::RateLimitSample => "限流采样",
            EvKind::RateLimitSkip => "限流跳过",
            EvKind::Observe => "观察留痕",
            EvKind::Escalate => "观察升格",
            EvKind::CaseFile => "直接立案",
            EvKind::StormEnter => "入风暴",
            EvKind::StormExit => "退风暴",
            EvKind::Idle => "闲置推进",
            EvKind::MisjudgeHit => "误判检出",
            EvKind::MismatchHit => "错位检出",
            EvKind::DowngradeHit => "降档拦截",
            EvKind::InjectArm => "钩子装定",
        }
    }
}

/// 事件账容量。
/// 来源：同域 F0826 LEDGER_CAP=8192 同口径（轨迹深度与内存折中；账满走
/// dropped 降级计数——主入账路径报错，标记路径溢出计入 dropped 不静默丢）。
pub const LEDGER_CAP: usize = 8192;
/// 红账容量。
pub const RED_CAP: usize = 256;

/// 定容事件账（时间轴内置：record 自增 tick——杜绝调用方双重借用）。
#[derive(Debug, Clone)]
pub struct EventLedger {
    /// 事件五元组（seq, tick, kind_tag, vec, flags）。
    events: Vec<(u64, u64, u8, u8, u8)>,
    /// 下一序号。
    next_seq: u64,
    /// 账 tick（内置自增）。
    tick: u64,
    /// 账满降级计数（标记路径溢出——不静默丢的面证）。
    dropped: u64,
}

impl EventLedger {
    /// 空账。
    pub fn new() -> Self {
        EventLedger { events: Vec::new(), next_seq: 1, tick: 0, dropped: 0 }
    }

    /// 入账（tick 内部自增；满则计 dropped 并报 LedgerFull）。
    pub fn record(&mut self, kind: EvKind, vec: u8, flags: u8) -> Result<u64, SpurFiveError> {
        if self.events.len() >= LEDGER_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(SpurFiveError::LedgerFull);
        }
        self.tick = self.tick.wrapping_add(1);
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.events.push((seq, self.tick, kind.tag(), vec, flags));
        Ok(seq)
    }

    /// 事件条数。
    pub const fn len(&self) -> usize {
        self.events.len()
    }

    /// 是否空账。
    pub const fn is_empty(&self) -> bool {
        self.events.len() == 0
    }

    /// 降级丢弃计数。
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 当前账 tick（只读）。
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// 取第 i 条（越界 None）。
    pub fn event(&self, i: usize) -> Option<(u64, u64, u8, u8, u8)> {
        if i < self.events.len() {
            Option::Some(self.events[i])
        } else {
            Option::None
        }
    }

    /// 逐类计数（tag ≤16 入槽；tag 槽数 17）。
    pub fn kind_counts(&self) -> [u32; 17] {
        let mut out = [0u32; 17];
        let mut i: usize = 0;
        while i < self.events.len() {
            let k = self.events[i].2 as usize;
            if k < 17 {
                out[k] = out[k].saturating_add(1);
            }
            i += 1;
        }
        out
    }

    /// 账指纹（全字段顺序 FNV；确定性）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = (h ^ self.next_seq).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.tick).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.dropped).wrapping_mul(0x0100_0000_01b3);
        let mut i: usize = 0;
        while i < self.events.len() {
            let (s, t, k, v, f) = self.events[i];
            h = (h ^ s).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ t).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ (k as u64)).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ (v as u64)).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ (f as u64)).wrapping_mul(0x0100_0000_01b3);
            i += 1;
        }
        h
    }
}

/// 红账（误判/错位/降档拦截/容量故障的违例记录；定容）。
#[derive(Debug, Clone)]
pub struct RedLedger {
    entries: Vec<(u16, u8, u64)>,
}

impl RedLedger {
    /// 空红账。
    pub fn new() -> Self {
        RedLedger { entries: Vec::new() }
    }

    /// 入红账（满则报 RedFull——不静默丢）。
    pub fn push(&mut self, code: u16, vec: u8, tick: u64) -> Result<(), SpurFiveError> {
        if self.entries.len() >= RED_CAP {
            return Err(SpurFiveError::RedFull);
        }
        self.entries.push((code, vec, tick));
        Ok(())
    }

    /// 红项数。
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否无红。
    pub const fn is_empty(&self) -> bool {
        self.entries.len() == 0
    }

    /// 首红项。
    pub fn first(&self) -> Option<(u16, u8, u64)> {
        if self.entries.is_empty() {
            Option::None
        } else {
            Option::Some(self.entries[0])
        }
    }

    /// 末红项。
    pub fn last(&self) -> Option<(u16, u8, u64)> {
        if self.entries.is_empty() {
            Option::None
        } else {
            Option::Some(self.entries[self.entries.len() - 1])
        }
    }

    /// 指定诊断码的红项计数（注入面断言用）。
    pub fn count_code(&self, code: u16) -> u32 {
        let mut n: u32 = 0;
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].0 == code {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 红账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < self.entries.len() {
            let (c, v, t) = self.entries[i];
            h = (h ^ (c as u64)).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ (v as u64)).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ t).wrapping_mul(0x0100_0000_01b3);
            i += 1;
        }
        h
    }
}

// ===========================================================================
// 九、五态处置引擎（分类 → 查表 → 执行 → 双层审计 → 入窗 → 升格）
// ===========================================================================

/// 限流采样比（风暴期每第 4 个事件展开全审计，其余仅计数）。
/// 来源：任务书「计数限流」处置语义的定比实现（1/4 采样——审计深度与
/// 风暴带吞吐的折中，采样位由限流计数低位决定，确定性可重放）。
pub const RL_SAMPLE_NTH: u64 = 4;

/// 故障注入钩子（演练机械：None=无故障；其余三值分别对应三注入面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultHook {
    /// 无故障（产线路径）。
    None,
    /// 态误判注入：分类结果被确定性扭曲（tag+1 mod 5）。
    Misjudge,
    /// 处置错位注入：查表分发结果被确定性扭曲（tag+1 mod 5）。
    Mismatch,
    /// 越级降档注入：升格立案后钩子立即对同向量发起降档（必被单向闸拦）。
    Downgrade,
}

impl FaultHook {
    /// 钩子名。
    pub const fn name(&self) -> &'static str {
        match self {
            FaultHook::None => "无故障",
            FaultHook::Misjudge => "态误判注入",
            FaultHook::Mismatch => "处置错位注入",
            FaultHook::Downgrade => "越级降档注入",
        }
    }
}

/// 确定性扭曲（tag+1 mod 5——注入面专用；两处共用同一扭曲规则）。
const fn twist_tag(tag: u8) -> u8 {
    (tag + 1) % 5
}

/// 单次到达的完整回报（对拍面：期望态/实判态/处置/执行效果/检出）。
#[derive(Debug, Clone, Copy)]
pub struct ArrivalReport {
    /// 到达序号（账 seq）。
    pub seq: u64,
    /// 期望态（const 规则表纯走查——对拍基准）。
    pub expected: SpurState,
    /// 实判态（引擎实际采用的态；无钩子时恒等于期望态）。
    pub actual: SpurState,
    /// 实际执行处置（查表结果；无钩子时与态零错位）。
    pub disposition: Disposition,
    /// 是否放行了 EOI（非 EoiPass 恒 false——拒 EOI 红线）。
    pub eoi_issued: bool,
    /// 本次是否新立一案。
    pub case_filed: bool,
    /// 本次是否发生观察升格立案。
    pub escalated: bool,
    /// 判定瞬间的窗率。
    pub window_rate: u32,
    /// 判定瞬间的滞回锁存态。
    pub storm_latched: bool,
    /// 检出（Some(诊断码)=本到达命中注入检出；None=干净）。
    pub fault_detected: Option<u16>,
}

/// 五态处置引擎主体。
pub struct SpurFiveEngine {
    /// 本地 APIC 使能态（false=未使能期/AP 初始化窗）。
    line_enabled: bool,
    /// 使能翻转次数（审计）。
    line_changes: u32,
    /// 向量占用平面。
    slots: BitPlane256,
    /// 风暴滑窗（独立窗 tick 轴）。
    window: StormWindow,
    /// 观察升格器 + 立案账 + 单向闸。
    obs: ObsEscalator,
    /// 事件账（tick 内置）。
    ledger: EventLedger,
    /// 红账。
    reds: RedLedger,
    /// 到达总数（守恒式分母）。
    arrivals: u64,
    /// 逐态计数（按态 tag 索引）。
    state_counts: [u64; 5],
    /// 逐处置执行计数（按处置 tag 索引——执行层审计基底）。
    disp_counters: DispCounters,
    /// EOI 放行总数（= disp_counters[EoiPass]——双记对拍）。
    eoi_issued: u64,
    /// 误判检出总数。
    misjudge_hits: u64,
    /// 错位检出总数。
    mismatch_hits: u64,
    /// 降档拦截总数（含钩子路径）。
    downgrade_blocks: u64,
    /// 注入钩子（演练机械；不计入账指纹——不属账面状态）。
    hook: FaultHook,
}

impl SpurFiveEngine {
    /// 新引擎（产线初值：AP 初始化窗——line 未使能，与真实 bring-up 序一致）。
    pub fn new() -> Self {
        SpurFiveEngine {
            line_enabled: false,
            line_changes: 0,
            slots: BitPlane256::zero(),
            window: StormWindow::new(),
            obs: ObsEscalator::new(),
            ledger: EventLedger::new(),
            reds: RedLedger::new(),
            arrivals: 0,
            state_counts: [0; 5],
            disp_counters: DispCounters::zero(),
            eoi_issued: 0,
            misjudge_hits: 0,
            mismatch_hits: 0,
            downgrade_blocks: 0,
            hook: FaultHook::None,
        }
    }

    // ----- 环境面（AP 使能/向量占用——到达签名的可观测环境） -----

    /// 设置本地 APIC 使能态（重复同值被拒——SVR 写入语义）。
    pub fn set_line_enabled(&mut self, enabled: bool) -> Result<(), SpurFiveError> {
        if self.line_enabled == enabled {
            return Err(SpurFiveError::LineStateBad(enabled));
        }
        self.line_enabled = enabled;
        self.line_changes = self.line_changes.saturating_add(1);
        Ok(())
    }

    /// 当前使能态。
    pub const fn line_enabled(&self) -> bool {
        self.line_enabled
    }

    /// 占用一向量槽（>255 越界 / 0xFF 保留 / 重复占用 全拒）。
    pub fn claim_vector(&mut self, v: u16) -> Result<u8, SpurFiveError> {
        if v > 0xFF {
            return Err(SpurFiveError::VecRange(v));
        }
        let vec = v as u8;
        if vec == SPURIOUS_VECTOR {
            return Err(SpurFiveError::SpurSlotLocked(vec));
        }
        if !self.slots.set(vec) {
            return Err(SpurFiveError::SlotBusy(vec));
        }
        Ok(vec)
    }

    /// 释放一向量槽（空槽释放被拒）。
    pub fn release_vector(&mut self, v: u16) -> Result<u8, SpurFiveError> {
        if v > 0xFF {
            return Err(SpurFiveError::VecRange(v));
        }
        let vec = v as u8;
        if !self.slots.clear(vec) {
            return Err(SpurFiveError::SlotFree(vec));
        }
        Ok(vec)
    }

    /// 向量占用查询。
    pub const fn is_claimed(&self, vec: u8) -> bool {
        self.slots.test(vec)
    }

    /// 占用数。
    pub const fn claimed_count(&self) -> u32 {
        self.slots.count()
    }

    // ----- 只读观测面 -----

    /// 窗率。
    pub const fn window_rate(&self) -> u32 {
        self.window.rate()
    }

    /// 风暴锁存态。
    pub const fn storm_latched(&self) -> bool {
        self.window.latched()
    }

    /// 风暴进出计数 (enters, exits)。
    pub const fn storm_transitions(&self) -> (u32, u32) {
        self.window.transitions()
    }

    /// 立案总数。
    pub const fn case_count(&self) -> u64 {
        self.obs.case_count()
    }

    /// 向量是否已立案。
    pub const fn is_filed(&self, vec: u8) -> bool {
        self.obs.is_filed(vec)
    }

    /// 向量在观计数。
    pub fn obs_count(&self, vec: u8) -> u16 {
        self.obs.obs_count(vec)
    }

    /// 逐态计数（拷贝）。
    pub const fn state_counts(&self) -> [u64; 5] {
        self.state_counts
    }

    /// 逐处置计数（拷贝）。
    pub const fn disp_counters(&self) -> DispCounters {
        self.disp_counters
    }

    /// 到达总数。
    pub const fn arrivals(&self) -> u64 {
        self.arrivals
    }

    /// EOI 放行总数。
    pub const fn eoi_issued(&self) -> u64 {
        self.eoi_issued
    }

    /// 误判检出数。
    pub const fn misjudge_hits(&self) -> u64 {
        self.misjudge_hits
    }

    /// 错位检出数。
    pub const fn mismatch_hits(&self) -> u64 {
        self.mismatch_hits
    }

    /// 降档拦截数。
    pub const fn downgrade_blocks(&self) -> u64 {
        self.downgrade_blocks
    }

    /// 红账长度。
    pub const fn red_len(&self) -> usize {
        self.reds.len()
    }

    /// 事件账长度。
    pub const fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 事件账降级丢弃数。
    pub const fn ledger_dropped(&self) -> u64 {
        self.ledger.dropped()
    }

    /// 事件账逐类计数（审计面便捷口；tag 槽 17）。
    pub fn ledger_kind_counts(&self) -> [u32; 17] {
        self.ledger.kind_counts()
    }

    /// 账 tick（只读观测）。
    pub const fn ledger_tick(&self) -> u64 {
        self.ledger.tick()
    }

    // ----- 分类面（期望态/实判态——对拍基准双轨） -----

    /// 期望态：const 规则表纯走查（无钩子、无副作用——对拍基准的唯一真源）。
    pub fn expected_state(&self, sig: ArrivalSig, storm_engaged: bool) -> SpurState {
        let f = Features {
            vector: sig.vector,
            line_enabled: self.line_enabled,
            vector_claimed: self.slots.test(sig.vector),
            isr_set_on_entry: sig.isr_set_on_entry,
            arb_loss: sig.arb_loss,
            storm_engaged,
        };
        decide(f)
    }

    /// 实判态：产线=期望态；Misjudge 钩子下被确定性扭曲（tag+1 mod 5）。
    fn classify_hooked(&self, sig: ArrivalSig, storm_engaged: bool) -> SpurState {
        let honest = self.expected_state(sig, storm_engaged);
        match self.hook {
            FaultHook::Misjudge => {
                let twisted = twist_tag(honest.tag());
                match SpurState::from_tag(twisted) {
                    Option::Some(s) => s,
                    Option::None => honest,
                }
            }
            _ => honest,
        }
    }

    /// 处置分发：const 表查表；Mismatch 钩子下被确定性扭曲（tag+1 mod 5）。
    fn dispatch_hooked(&self, state: SpurState) -> Disposition {
        let honest = disposition_of(state);
        match self.hook {
            FaultHook::Mismatch => {
                let twisted = twist_tag(honest.tag());
                match Disposition::from_tag(twisted) {
                    Option::Some(d) => d,
                    Option::None => honest,
                }
            }
            _ => honest,
        }
    }

    // ----- 注入面（钩子装定/解除——演练机械） -----

    /// 装定注入钩子（入账 InjectArm 标记；返回账 seq）。
    pub fn arm_fault(&mut self, hook: FaultHook) -> Result<u64, SpurFiveError> {
        self.hook = hook;
        self.ledger.record(EvKind::InjectArm, 0, hook.name().len() as u8)
    }

    /// 解除注入钩子（同样入账标记）。
    pub fn clear_fault(&mut self) -> Result<u64, SpurFiveError> {
        self.hook = FaultHook::None;
        self.ledger.record(EvKind::InjectArm, 0, 0)
    }

    /// 当前钩子。
    pub const fn hook(&self) -> FaultHook {
        self.hook
    }

    // ----- 红账入账（tick 先取局部——借用纪律） -----

    fn red_push(&mut self, code: u16, vec: u8) {
        let tk = self.ledger.tick();
        if self.reds.push(code, vec, tk).is_err() {
            // 红账满：升格为账面计数故障（不静默——health 必红）。
            self.mismatch_hits = self.mismatch_hits.saturating_add(1);
        }
    }

    // ----- 处置执行（每处置唯一触点——执行层审计的物质基础） -----

    /// 执行处置：只碰本处置自己的计数/账/对象，别的一律不动。
    /// 返回 (EOI放行?, 新立案?, 升格立案?)。
    fn execute(
        &mut self,
        disp: Disposition,
        sig: ArrivalSig,
    ) -> Result<(bool, bool, bool), SpurFiveError> {
        match disp {
            Disposition::EoiPass => {
                self.disp_counters.inc(disp);
                self.eoi_issued = self.eoi_issued.saturating_add(1);
                let _ = self.ledger.record(EvKind::EoiPass, sig.vector, 0);
                Ok((true, false, false))
            }
            Disposition::RecordNoEoi => {
                self.disp_counters.inc(disp);
                let _ = self.ledger.record(EvKind::RecordNoEoi, sig.vector, 0);
                Ok((false, false, false))
            }
            Disposition::RateLimit => {
                self.disp_counters.inc(disp);
                // 限流语义：全量计数已入；审计面 1/4 采样（确定性：按限流计数低位）。
                let n = self.disp_counters.get(disp);
                if n % RL_SAMPLE_NTH == 0 {
                    let _ = self.ledger.record(EvKind::RateLimitSample, sig.vector, (n % 255) as u8);
                } else {
                    let _ = self.ledger.record(EvKind::RateLimitSkip, sig.vector, 0);
                }
                Ok((false, false, false))
            }
            Disposition::ObserveTrail => {
                self.disp_counters.inc(disp);
                let _ = self.ledger.record(EvKind::Observe, sig.vector, 0);
                Ok((false, false, false))
            }
            Disposition::EscalateFile => {
                self.disp_counters.inc(disp);
                // SpurDisabled 路径：立即立案（已立案 dedup——单向闸防重复立）。
                let tk = self.ledger.tick();
                match self.obs.file_disabled(sig.vector, tk)? {
                    Option::Some(_entry) => {
                        let _ = self.ledger.record(EvKind::CaseFile, sig.vector, 1);
                        Ok((false, true, false))
                    }
                    Option::None => {
                        let _ = self.ledger.record(EvKind::CaseFile, sig.vector, 0);
                        Ok((false, false, false))
                    }
                }
            }
        }
    }

    // ----- 主路径：到达 → 判态 → 查表 → 执行 → 双层审计 → 入窗 → 升格 -----

    /// 中断到达主入口（两条时间轴全部内置推进；调用点不带任何时间参数）。
    ///
    /// 步序（确定性固定）：
    ///   1 到达入账（账 tick+1）；
    ///   2 窗推一格（窗 tick+1，淘汰最旧槽）；
    ///   3 滞回评估 A（入窗前——衰减驱动的退风暴在此发生）；
    ///   4 判态（期望态=表走查；实判态=可被钩子扭曲）；
    ///   5 查表分发处置（可被钩子扭曲）；
    ///   6 执行处置 + 双层错位审计（查表层+执行层）；
    ///   7 误判复检（期望 vs 实判——不一致立红）；
    ///   8 入窗计数（非真即入窗）；
    ///   9 滞回评估 B（入窗后——跨越驱动的入风暴在此发生，
    ///     跨越当次按跨越前态处置，风暴态自下一次到达生效）；
    ///   10 观察升格（Observing 态时逐向量计数达阈立案——单向）。
    pub fn arrive(&mut self, sig: ArrivalSig) -> Result<ArrivalReport, SpurFiveError> {
        // (1) 到达入账（主路径——账满即 Err，无降级）。
        let seq = self.ledger.record(EvKind::Arrive, sig.vector, 0)?;

        // (2)(3) 窗推 + 滞回评估 A。
        self.window.advance();
        if let Some(entered) = self.window.evaluate() {
            let kind = if entered { EvKind::StormEnter } else { EvKind::StormExit };
            let _ = self.ledger.record(kind, sig.vector, 0);
        }

        // (4) 判态双轨。
        let engaged = self.window.latched();
        let expected = self.expected_state(sig, engaged);
        let actual = self.classify_hooked(sig, engaged);
        let misjudged = expected.tag() != actual.tag();
        let flags = actual.tag() | if misjudged { 0x80 } else { 0 };
        let _ = self.ledger.record(EvKind::Classify, sig.vector, flags);

        // 守恒分母先记（即使后续可失败路径，到达已被账面计入一次）。
        self.arrivals = self.arrivals.saturating_add(1);
        let st_i = actual.tag() as usize;
        self.state_counts[st_i] = self.state_counts[st_i].saturating_add(1);

        // (5)(6) 查表分发 + 执行 + 双层审计。
        let disp = self.dispatch_hooked(actual);
        let before = self.disp_counters;
        let (eoi_issued, case_filed, escalated) = self.execute(disp, sig)?;
        let delta = self.disp_counters.delta_from(&before);

        // 双层错位检测器：
        //   查表层——执行用处置 vs 态查表（const 表唯一真源）；
        //   执行层——计数增量指纹 vs 执行用处置参数。
        let table_disp = disposition_of(actual);
        let mut mismatched = false;
        if disp.tag() != table_disp.tag() {
            mismatched = true;
            self.mismatch_hits = self.mismatch_hits.saturating_add(1);
            self.red_push(X_SF_DISPO_MISMATCH, sig.vector);
            let _ = self.ledger.record(EvKind::MismatchHit, sig.vector, disp.tag());
        }
        if !delta.delta_matches(disp) {
            mismatched = true;
            self.mismatch_hits = self.mismatch_hits.saturating_add(1);
            self.red_push(X_SF_DISPO_MISMATCH, sig.vector);
            let _ = self.ledger.record(EvKind::MismatchHit, sig.vector, 0x40 | disp.tag());
        }

        // (7) 误判复检（账记实况，检出另立红项——不篡改实况账）。
        if misjudged {
            self.misjudge_hits = self.misjudge_hits.saturating_add(1);
            self.red_push(X_SF_MISJUDGE_DETECTED, sig.vector);
            let _ = self.ledger.record(EvKind::MisjudgeHit, sig.vector, expected.tag());
        }

        // (8) 入窗计数（非真即入窗）。
        if actual.feeds_window() {
            self.window.add_current();
        }

        // (9) 滞回评估 B。
        if let Some(entered) = self.window.evaluate() {
            let kind = if entered { EvKind::StormEnter } else { EvKind::StormExit };
            let _ = self.ledger.record(kind, sig.vector, 0);
        }

        // (10) 观察升格（Observing 专属路径）。
        let mut escalated_obs = escalated;
        if matches!(actual, SpurState::Observing) {
            let tk = self.ledger.tick();
            if let Option::Some(_entry) = self.obs.observe(sig.vector, tk)? {
                escalated_obs = true;
                let _ = self.ledger.record(EvKind::Escalate, sig.vector, 1);
            }
            // 降档注入钩子：升格后立即尝试对同向量降档（必被单向闸拦——检出）。
            if escalated_obs && matches!(self.hook, FaultHook::Downgrade) {
                if self.obs.try_downgrade(sig.vector).is_err() {
                    self.downgrade_blocks = self.downgrade_blocks.saturating_add(1);
                    self.red_push(X_SF_DOWNGRADE_BLOCKED, sig.vector);
                    let _ = self.ledger.record(EvKind::DowngradeHit, sig.vector, 1);
                }
            }
        }

        // 检出汇总（一到达至多记一码：误判优先，其次错位，其次降档）。
        let fault_detected = if misjudged {
            Option::Some(X_SF_MISJUDGE_DETECTED)
        } else if mismatched {
            Option::Some(X_SF_DISPO_MISMATCH)
        } else if escalated_obs && matches!(self.hook, FaultHook::Downgrade) {
            // 钩子路径的降档拦截已在上面立红；此处对外标记（仅当真的发生了升格）。
            if self.obs.downgrade_blocks() > 0 {
                Option::Some(X_SF_DOWNGRADE_BLOCKED)
            } else {
                Option::None
            }
        } else {
            Option::None
        };

        Ok(ArrivalReport {
            seq,
            expected,
            actual,
            disposition: disp,
            eoi_issued,
            case_filed,
            escalated: escalated_obs,
            window_rate: self.window.rate(),
            storm_latched: self.window.latched(),
            fault_detected,
        })
    }

    /// 闲置推进（无到达时间流逝：窗衰减 + 滞回评估；账面记 Idle 标记）。
    /// 返回推进后窗率。
    pub fn idle(&mut self, n: u32) -> Result<u32, SpurFiveError> {
        let rate = self.window.idle(n);
        if let Some(entered) = self.window.evaluate() {
            let kind = if entered { EvKind::StormEnter } else { EvKind::StormExit };
            let _ = self.ledger.record(kind, 0, 0);
        }
        let flags = if n > 255 { 255 } else { n as u8 };
        let _ = self.ledger.record(EvKind::Idle, 0, flags);
        Ok(rate)
    }

    /// 越级降档尝试（公开单向闸证明面：恒 Err——降档无合法路径）。
    /// 已立案 → DowngradeBlocked（红账立红 + 拦截计数）；
    /// 未立案 → SlotFree。
    pub fn try_downgrade(&mut self, vec: u8) -> Result<(), SpurFiveError> {
        let r = self.obs.try_downgrade(vec);
        if matches!(r, Err(SpurFiveError::DowngradeBlocked(_))) {
            self.downgrade_blocks = self.downgrade_blocks.saturating_add(1);
            self.red_push(X_SF_DOWNGRADE_BLOCKED, vec);
            let _ = self.ledger.record(EvKind::DowngradeHit, vec, 0);
        }
        r
    }

    // ----- 对账面 -----

    /// 守恒对账：到达总数 vs 逐态计数和 vs 逐处置执行和（三向相等才平）。
    pub fn conservation(&self) -> ConservationReport {
        let mut state_sum: u64 = 0;
        let mut i: usize = 0;
        while i < 5 {
            state_sum = state_sum.saturating_add(self.state_counts[i]);
            i += 1;
        }
        let disp_sum = self.disp_counters.total();
        ConservationReport {
            arrivals: self.arrivals,
            state_sum,
            disp_sum,
            balanced: self.arrivals == state_sum && self.arrivals == disp_sum,
        }
    }

    /// 体检（红账空 + 守恒平 + EOI 双记平 + 账零丢弃 = 绿）。
    pub fn health(&self) -> HealthReport {
        let con = self.conservation();
        let green = self.reds.is_empty()
            && con.balanced
            && self.eoi_issued == self.disp_counters.get(Disposition::EoiPass)
            && self.ledger.dropped() == 0;
        HealthReport {
            green,
            red_entries: self.reds.len() as u64,
            arrivals: self.arrivals,
            state_counts: self.state_counts,
            disp_counters: self.disp_counters,
            misjudge_hits: self.misjudge_hits,
            mismatch_hits: self.mismatch_hits,
            downgrade_blocks: self.downgrade_blocks,
            window_rate: self.window.rate(),
            storm_latched: self.window.latched(),
            cases_filed: self.obs.case_count(),
            ledger_dropped: self.ledger.dropped(),
            conservation_balanced: con.balanced,
        }
    }

    /// 状态指纹（全账面状态 FNV 折叠；不含事件账明细与注入钩子——
    /// 钩子是演练机械不属账面，账明细由 ledger_fingerprint 单独对拍）。
    pub fn state_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = (h ^ (self.line_enabled as u64)).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ (self.line_changes as u64)).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.slots.fingerprint()).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.window.fingerprint()).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.obs.fingerprint()).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.reds.fingerprint()).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.arrivals).wrapping_mul(0x0100_0000_01b3);
        let mut i: usize = 0;
        while i < 5 {
            h = (h ^ self.state_counts[i]).wrapping_mul(0x0100_0000_01b3);
            i += 1;
        }
        h = (h ^ self.disp_counters.fingerprint()).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.eoi_issued).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.misjudge_hits).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.mismatch_hits).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ self.downgrade_blocks).wrapping_mul(0x0100_0000_01b3);
        h
    }

    /// 事件账指纹（明细轴对拍——双跑确定性第二指纹）。
    pub fn ledger_fingerprint(&self) -> u64 {
        self.ledger.fingerprint()
    }

    /// 人体可读体检单行（alloc::format——审计台渲染）。
    pub fn render_health(&self) -> String {
        let h = self.health();
        format!(
            "[spurfive] green={} red={} arr={} states=[R{}/E{}/D{}/S{}/O{}] disp=[EOI{}/NOE{}/RL{}/OBS{}/ESC{}] mis={} mm={} dg={} rate={} storm={} cases={} drop={}",
            h.green,
            h.red_entries,
            h.arrivals,
            h.state_counts[0],
            h.state_counts[1],
            h.state_counts[2],
            h.state_counts[3],
            h.state_counts[4],
            h.disp_counters.v[0],
            h.disp_counters.v[1],
            h.disp_counters.v[2],
            h.disp_counters.v[3],
            h.disp_counters.v[4],
            h.misjudge_hits,
            h.mismatch_hits,
            h.downgrade_blocks,
            h.window_rate,
            h.storm_latched,
            h.cases_filed,
            h.ledger_dropped
        )
    }
}

/// 守恒对账报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConservationReport {
    /// 到达总数。
    pub arrivals: u64,
    /// 逐态计数和。
    pub state_sum: u64,
    /// 逐处置执行和。
    pub disp_sum: u64,
    /// 三向相等才平。
    pub balanced: bool,
}

/// 体检报告。
#[derive(Debug, Clone, Copy)]
pub struct HealthReport {
    /// 绿=红账空+守恒平+EOI 双记平+零丢弃。
    pub green: bool,
    /// 红项数。
    pub red_entries: u64,
    /// 到达总数。
    pub arrivals: u64,
    /// 逐态计数。
    pub state_counts: [u64; 5],
    /// 逐处置计数。
    pub disp_counters: DispCounters,
    /// 误判检出数。
    pub misjudge_hits: u64,
    /// 错位检出数。
    pub mismatch_hits: u64,
    /// 降档拦截数。
    pub downgrade_blocks: u64,
    /// 当前窗率。
    pub window_rate: u32,
    /// 风暴锁存态。
    pub storm_latched: bool,
    /// 立案总数。
    pub cases_filed: u64,
    /// 事件账降级丢弃数。
    pub ledger_dropped: u64,
    /// 守恒是否平衡。
    pub conservation_balanced: bool,
}

// ===========================================================================
// 十、快照子系统（账核序列化 + CRC16 + 往返对拍）
// ===========================================================================

/// 快照魔数（0x5F08_45B0：5F=snap 记号、0845=任务号、B0=段基——写死可溯）。
pub const SNAP_MAGIC: u32 = 0x5F08_45B0;
/// 快照布局版本（布局改动必须 +1 并同步 from 偏移）。
pub const SNAP_VERSION: u8 = 1;
/// 快照定长（768 字节；CRC 在尾 2 字节）。
pub const SNAP_LEN: usize = 768;

/// CRC 偏移。
const SNAP_CRC_AT: usize = SNAP_LEN - 2;

/// CRC16-CCITT（位运算实现——无查表依赖，no_std 定容纪律）。
/// 多项式 0x1021、初值 0xFFFF（来源：CCITT-FALSE 标准口径，同域 F0826 同法）。
pub fn crc16(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    let mut i: usize = 0;
    while i < bytes.len() {
        crc ^= (bytes[i] as u16) << 8;
        let mut b: u8 = 0;
        while b < 8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
            b += 1;
        }
        i += 1;
    }
    crc
}

// 字节布局（小端手工拼装——内核序列化不依赖 serde）：
//   0..4   magic u32
//   4      version
//   5      line_enabled
//   6      window.latched
//   7      保留 0
//   8..12  window.total u32
//   12..20 window.ticks u64
//   20..24 window.enters u32
//   24..28 window.exits u32
//   28..30 window.head u16（冗余位——与 ticks&0xFF 交叉校验）
//   30..32 obs_len u16（≤OBS_CAP）
//   32..64 slots 平面 32B
//   64..96 filed 平面 32B
//   96..136 disp 计数 [u64;5] 40B
//   136..232 obs 表 32×(vec u8 + count u16) 96B
//   232..744 window.slots 256×u16 512B
//   744..752 arrivals u64
//   752..766 保留 0
//   766..768 crc16（覆盖 0..766）
const O_MAGIC: usize = 0;
const O_VERSION: usize = 4;
const O_LINE: usize = 5;
const O_LATCH: usize = 6;
const O_TOTAL: usize = 8;
const O_TICKS: usize = 12;
const O_ENTERS: usize = 20;
const O_EXITS: usize = 24;
const O_HEAD: usize = 28;
const O_OBSLEN: usize = 30;
const O_SLOTS: usize = 32;
const O_FILED: usize = 64;
const O_DISP: usize = 96;
const O_OBS: usize = 136;
const O_WSLOTS: usize = 232;
const O_ARRIVALS: usize = 744;

// 编译期布局互斥自检（无重叠、均在界内）。
const _: () = assert!(O_SLOTS + 32 == O_FILED);
const _: () = assert!(O_FILED + 32 == O_DISP);
const _: () = assert!(O_DISP + 40 == O_OBS);
const _: () = assert!(O_OBS + 96 == O_WSLOTS);
const _: () = assert!(O_WSLOTS + 512 == O_ARRIVALS);
const _: () = assert!(O_ARRIVALS + 8 <= SNAP_CRC_AT);

/// 读/写小端原语（定容安全：偏移+宽在编译期已核，运行期再闸一道）。
fn rd_u32(b: &[u8; SNAP_LEN], at: usize) -> u32 {
    (b[at] as u32)
        | ((b[at + 1] as u32) << 8)
        | ((b[at + 2] as u32) << 16)
        | ((b[at + 3] as u32) << 24)
}
fn rd_u64(b: &[u8; SNAP_LEN], at: usize) -> u64 {
    let mut x: u64 = 0;
    let mut i: usize = 0;
    while i < 8 {
        x |= (b[at + i] as u64) << (8 * i);
        i += 1;
    }
    x
}
fn wr_u32(b: &mut [u8; SNAP_LEN], at: usize, v: u32) {
    b[at] = (v & 0xFF) as u8;
    b[at + 1] = ((v >> 8) & 0xFF) as u8;
    b[at + 2] = ((v >> 16) & 0xFF) as u8;
    b[at + 3] = ((v >> 24) & 0xFF) as u8;
}
fn wr_u64(b: &mut [u8; SNAP_LEN], at: usize, v: u64) {
    let mut i: usize = 0;
    while i < 8 {
        b[at + i] = ((v >> (8 * i)) & 0xFF) as u8;
        i += 1;
    }
}
fn wr_u16(b: &mut [u8; SNAP_LEN], at: usize, v: u16) {
    b[at] = (v & 0xFF) as u8;
    b[at + 1] = ((v >> 8) & 0xFF) as u8;
}
fn rd_u16(b: &[u8; SNAP_LEN], at: usize) -> u16 {
    (b[at] as u16) | ((b[at + 1] as u16) << 8)
}

/// 引擎账核快照（定长 768B 栈对象；账明细不入快照——快照只保「核」：
/// 环境/窗/平面/计数/观察表，事件账与红账属审计轨迹另行对拍）。
#[derive(Debug, Clone, Copy)]
pub struct EngineSnapshot {
    /// 原始字节（含尾 CRC）。
    pub raw: [u8; SNAP_LEN],
}

/// 从快照恢复的账核（与引擎核字段一一对应）。
#[derive(Debug, Clone)]
pub struct RestoredCore {
    /// 使能态。
    pub line_enabled: bool,
    /// 向量占用平面。
    pub slots: BitPlane256,
    /// 立案平面。
    pub filed: BitPlane256,
    /// 风暴窗（含槽平面）。
    pub window: StormWindow,
    /// 观察计数表（插入序）。
    pub obs_counts: Vec<(u8, u16)>,
    /// 逐处置计数。
    pub disp_counters: DispCounters,
    /// 到达总数。
    pub arrivals: u64,
}

impl EngineSnapshot {
    /// 从引擎抓快照（无失败路径——全部字段定容写死）。
    pub fn capture(eng: &SpurFiveEngine) -> Self {
        let mut raw = [0u8; SNAP_LEN];
        wr_u32(&mut raw, O_MAGIC, SNAP_MAGIC);
        raw[O_VERSION] = SNAP_VERSION;
        raw[O_LINE] = eng.line_enabled as u8;
        raw[O_LATCH] = eng.window.latched() as u8;
        wr_u32(&mut raw, O_TOTAL, eng.window.rate());
        wr_u64(&mut raw, O_TICKS, eng.window.ticks());
        let (enters, exits) = eng.window.transitions();
        wr_u32(&mut raw, O_ENTERS, enters);
        wr_u32(&mut raw, O_EXITS, exits);
        wr_u16(&mut raw, O_HEAD, eng.window.head() as u16);
        let obs_snap = eng.obs.obs_snapshot();
        let obs_len = if obs_snap.len() > OBS_CAP { OBS_CAP } else { obs_snap.len() };
        wr_u16(&mut raw, O_OBSLEN, obs_len as u16);
        let slot_bytes = eng.slots.to_bytes();
        let mut i: usize = 0;
        while i < 32 {
            raw[O_SLOTS + i] = slot_bytes[i];
            i += 1;
        }
        // 立案平面：由逐向量 is_filed 重建字节（ObsEscalator 不外泄内部平面，
        // 走公开谓词逐向量折叠——256 次谓词调用，O(1) 内存）。
        let mut filed = BitPlane256::zero();
        let mut v: u16 = 0;
        while v <= 0xFF {
            if eng.obs.is_filed(v as u8) {
                filed.set(v as u8);
            }
            v += 1;
        }
        let filed_bytes = filed.to_bytes();
        let mut i: usize = 0;
        while i < 32 {
            raw[O_FILED + i] = filed_bytes[i];
            i += 1;
        }
        let mut di: usize = 0;
        while di < 5 {
            wr_u64(&mut raw, O_DISP + di * 8, eng.disp_counters.v[di]);
            di += 1;
        }
        let mut oi: usize = 0;
        while oi < obs_len {
            let (vec, cnt) = obs_snap[oi];
            raw[O_OBS + oi * 3] = vec;
            wr_u16(&mut raw, O_OBS + oi * 3 + 1, cnt);
            oi += 1;
        }
        // 窗槽平面：由指纹轴外取真实槽——通过 window.fingerprint 不可逆，
        // 改走逐率校验不可行；因此引擎暴露窗槽只读拷贝（见 window_slots()）。
        let ws = eng.window_slots();
        let mut wi: usize = 0;
        while wi < STORM_WINDOW_TICKS {
            wr_u16(&mut raw, O_WSLOTS + wi * 2, ws[wi]);
            wi += 1;
        }
        wr_u64(&mut raw, O_ARRIVALS, eng.arrivals);
        let crc = crc16(&raw[0..SNAP_CRC_AT]);
        wr_u16(&mut raw, SNAP_CRC_AT, crc);
        EngineSnapshot { raw }
    }

    /// 四闸校验（魔数/版本/头位交叉/CRC）。
    pub fn validate(&self) -> Result<(), SpurFiveError> {
        if rd_u32(&self.raw, O_MAGIC) != SNAP_MAGIC {
            return Err(SpurFiveError::SnapshotCorrupt(0));
        }
        if self.raw[O_VERSION] != SNAP_VERSION {
            return Err(SpurFiveError::SnapshotCorrupt(1));
        }
        let head = rd_u16(&self.raw, O_HEAD) as u64;
        let ticks = rd_u64(&self.raw, O_TICKS);
        if head != (ticks & 0xFF) {
            return Err(SpurFiveError::SnapshotCorrupt(2));
        }
        let crc_now = crc16(&self.raw[0..SNAP_CRC_AT]);
        let crc_want = rd_u16(&self.raw, SNAP_CRC_AT);
        if crc_now != crc_want {
            return Err(SpurFiveError::SnapshotCorrupt(crc_want));
        }
        Ok(())
    }

    /// 恢复账核（先四闸校验，再逐字段重建）。
    pub fn restore(&self) -> Result<RestoredCore, SpurFiveError> {
        self.validate()?;
        let mut window = StormWindow::new();
        window.latched = self.raw[O_LATCH] != 0;
        window.total = rd_u32(&self.raw, O_TOTAL);
        window.ticks = rd_u64(&self.raw, O_TICKS);
        window.enters = rd_u32(&self.raw, O_ENTERS);
        window.exits = rd_u32(&self.raw, O_EXITS);
        let mut wi: usize = 0;
        while wi < STORM_WINDOW_TICKS {
            window.slots[wi] = rd_u16(&self.raw, O_WSLOTS + wi * 2);
            wi += 1;
        }
        // 槽总量交叉校验（重建槽和 ≠ 记录 total → 布局损坏）。
        let mut slots_sum: u32 = 0;
        let mut si: usize = 0;
        while si < STORM_WINDOW_TICKS {
            slots_sum = slots_sum.saturating_add(window.slots[si] as u32);
            si += 1;
        }
        if slots_sum != window.total {
            return Err(SpurFiveError::SnapshotCorrupt(3));
        }
        let mut slot_bytes = [0u8; 32];
        let mut i: usize = 0;
        while i < 32 {
            slot_bytes[i] = self.raw[O_SLOTS + i];
            i += 1;
        }
        let mut filed_bytes = [0u8; 32];
        let mut i: usize = 0;
        while i < 32 {
            filed_bytes[i] = self.raw[O_FILED + i];
            i += 1;
        }
        let mut disp = DispCounters::zero();
        let mut di: usize = 0;
        while di < 5 {
            disp.v[di] = rd_u64(&self.raw, O_DISP + di * 8);
            di += 1;
        }
        let obs_len = rd_u16(&self.raw, O_OBSLEN) as usize;
        if obs_len > OBS_CAP {
            return Err(SpurFiveError::SnapshotCorrupt(4));
        }
        let mut obs_counts = Vec::new();
        let mut oi: usize = 0;
        while oi < obs_len {
            let vec = self.raw[O_OBS + oi * 3];
            let cnt = rd_u16(&self.raw, O_OBS + oi * 3 + 1);
            obs_counts.push((vec, cnt));
            oi += 1;
        }
        Ok(RestoredCore {
            line_enabled: self.raw[O_LINE] != 0,
            slots: BitPlane256::from_bytes(&slot_bytes),
            filed: BitPlane256::from_bytes(&filed_bytes),
            window,
            obs_counts,
            disp_counters: disp,
            arrivals: rd_u64(&self.raw, O_ARRIVALS),
        })
    }

    /// 从外部字节重建快照对象（先校验——损坏即 Err）。
    pub fn from_bytes(raw: &[u8; SNAP_LEN]) -> Result<Self, SpurFiveError> {
        let mut snap = EngineSnapshot { raw: [0u8; SNAP_LEN] };
        let mut i: usize = 0;
        while i < SNAP_LEN {
            snap.raw[i] = raw[i];
            i += 1;
        }
        snap.validate()?;
        Ok(snap)
    }

    /// 快照指纹（原始字节 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < SNAP_LEN {
            h = (h ^ (self.raw[i] as u64)).wrapping_mul(0x0100_0000_01b3);
            i += 1;
        }
        h
    }
}

impl SpurFiveEngine {
    /// 窗槽平面只读拷贝（快照捕获用——不改窗状态）。
    pub fn window_slots(&self) -> [u16; STORM_WINDOW_TICKS] {
        let mut out = [0u16; STORM_WINDOW_TICKS];
        let mut i: usize = 0;
        while i < STORM_WINDOW_TICKS {
            out[i] = self.window.slots[i];
            i += 1;
        }
        out
    }

    /// 引擎核视图（与 RestoredCore 同构——往返对拍基准）。
    pub fn core_view(&self) -> RestoredCore {
        let mut filed = BitPlane256::zero();
        let mut v: u16 = 0;
        while v <= 0xFF {
            if self.obs.is_filed(v as u8) {
                filed.set(v as u8);
            }
            v += 1;
        }
        RestoredCore {
            line_enabled: self.line_enabled,
            slots: self.slots,
            filed,
            window: self.window.clone(),
            obs_counts: self.obs.obs_snapshot(),
            disp_counters: self.disp_counters,
            arrivals: self.arrivals,
        }
    }

    /// 核指纹（RestoredCore 折叠——core_view 与快照恢复结果对拍用）。
    pub fn core_fingerprint(core: &RestoredCore) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = (h ^ (core.line_enabled as u64)).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ core.slots.fingerprint()).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ core.filed.fingerprint()).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ core.window.fingerprint()).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ core.disp_counters.fingerprint()).wrapping_mul(0x0100_0000_01b3);
        h = (h ^ core.arrivals).wrapping_mul(0x0100_0000_01b3);
        let mut i: usize = 0;
        while i < core.obs_counts.len() {
            h = (h ^ (core.obs_counts[i].0 as u64)).wrapping_mul(0x0100_0000_01b3);
            h = (h ^ (core.obs_counts[i].1 as u64)).wrapping_mul(0x0100_0000_01b3);
            i += 1;
        }
        h
    }

    /// 快照往返自证（capture → validate → restore → 核指纹对拍；
    /// 不等即 Err(RoundtripMismatch)——调用方免记忆布局细节）。
    pub fn snapshot_roundtrip(&self) -> Result<u64, SpurFiveError> {
        let snap = EngineSnapshot::capture(self);
        let restored = snap.restore()?;
        let fp_live = SpurFiveEngine::core_fingerprint(&self.core_view());
        let fp_restored = SpurFiveEngine::core_fingerprint(&restored);
        if fp_live != fp_restored {
            return Err(SpurFiveError::RoundtripMismatch);
        }
        Ok(fp_live)
    }
}

// ===========================================================================
// 十一、注入演练（五态各 10 次——J1-a 100% 判定正确率自证）
// ===========================================================================

/// 每态注入次数（任务书「注入各态 10/10 次」）。
pub const DRILL_PER_STATE: u32 = 10;
/// 风暴态演练的前置铺窗次数（64 次 spurious 到达恰好跨入阈）。
pub const DRILL_STORM_PRIME: u32 = STORM_ENTER;

/// 演练报告（J1-a 自证面）。
#[derive(Debug, Clone, Copy)]
pub struct DrillReport {
    /// 逐态期望计数（恒 [10;5]——对拍基准写入）。
    pub expected: [u32; 5],
    /// 逐态实判正确数（实判态 == 期望态的次数）。
    pub correct: [u32; 5],
    /// 注入总数（50）。
    pub injections: u32,
    /// 判定正确总数。
    pub correct_total: u32,
    /// 正确率（万分比：10000 = 100.00%）。
    pub rate_bp: u32,
    /// 演练全程处置错位数（判据要求恰 0）。
    pub mismatch_events: u32,
    /// 演练全程误判数（判据要求恰 0——演练不打钩子）。
    pub misjudge_events: u32,
    /// J1-a 与 J1-b 联合达成（正确率 100% 且错位 0）。
    pub ok: bool,
}

impl DrillReport {
    /// 渲染单行（审计台）。
    pub fn render(&self) -> String {
        format!(
            "[drill] ok={} rate={}bp({}/{}) correct=R{} E{} D{} S{} O{} mm={} mis={}",
            self.ok,
            self.rate_bp,
            self.correct_total,
            self.injections,
            self.correct[0],
            self.correct[1],
            self.correct[2],
            self.correct[3],
            self.correct[4],
            self.mismatch_events,
            self.misjudge_events
        )
    }
}

/// 五态注入演练主入口。
///
/// 每态独立引擎（态间隔离——风暴态的窗高密度不泄漏进其他态的判定）；
/// 每次注入「期望态（规则表纯走查，先算）vs 实判态（arrive 回报）」对拍；
/// 额外断言回报处置 == 处置表查表结果（零错位的逐次自证）。
pub fn run_drill() -> Result<DrillReport, SpurFiveError> {
    let mut expected = [0u32; 5];
    let mut correct = [0u32; 5];
    let mut mismatch_events: u32 = 0;
    let mut misjudge_events: u32 = 0;

    // --- 态 0：真中断（claim 0x41 → 签名完备 → EoiPass）---
    {
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true)?;
        eng.claim_vector(0x41)?;
        let sig = ArrivalSig::new(0x41, true, false);
        let mut n: u32 = 0;
        while n < DRILL_PER_STATE {
            let want = eng.expected_state(sig, eng.storm_latched());
            let rep = eng.arrive(sig)?;
            expected[0] += 1;
            if rep.actual.tag() == SpurState::Real.tag() && rep.expected.tag() == want.tag() {
                correct[0] += 1;
            }
            if rep.disposition.tag() != disposition_of(rep.actual).tag() {
                mismatch_events += 1;
            }
            if rep.fault_detected.is_some() {
                misjudge_events += 1;
            }
            n += 1;
        }
    }

    // --- 态 1：使能期伪中断（0xFF + 仲裁失败签名 → 入账拒 EOI）---
    {
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true)?;
        let sig = ArrivalSig::new(SPURIOUS_VECTOR, false, true);
        let mut n: u32 = 0;
        while n < DRILL_PER_STATE {
            let want = eng.expected_state(sig, eng.storm_latched());
            let rep = eng.arrive(sig)?;
            expected[1] += 1;
            if rep.actual.tag() == SpurState::SpurEnabled.tag() && rep.expected.tag() == want.tag() {
                correct[1] += 1;
            }
            if rep.disposition.tag() != disposition_of(rep.actual).tag() {
                mismatch_events += 1;
            }
            if rep.fault_detected.is_some() {
                misjudge_events += 1;
            }
            n += 1;
        }
    }

    // --- 态 2：未使能期伪中断（AP 初始化窗到达 → 直接立案）---
    {
        let mut eng = SpurFiveEngine::new();
        // 引擎初值即 line 未使能（bring-up 序）——无需 set。
        let sig = ArrivalSig::new(0x42, false, false);
        let mut n: u32 = 0;
        while n < DRILL_PER_STATE {
            let want = eng.expected_state(sig, eng.storm_latched());
            let rep = eng.arrive(sig)?;
            expected[2] += 1;
            if rep.actual.tag() == SpurState::SpurDisabled.tag() && rep.expected.tag() == want.tag() {
                correct[2] += 1;
            }
            if rep.disposition.tag() != disposition_of(rep.actual).tag() {
                mismatch_events += 1;
            }
            if rep.fault_detected.is_some() {
                misjudge_events += 1;
            }
            n += 1;
        }
    }

    // --- 态 3：风暴期伪中断（64 次铺窗跨阈滞回 → 限流处置）---
    {
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true)?;
        let prime = ArrivalSig::new(SPURIOUS_VECTOR, false, true);
        let mut p: u32 = 0;
        while p < DRILL_STORM_PRIME {
            eng.arrive(prime)?;
            p += 1;
        }
        // 铺窗后必已入风暴（64 ≥ STORM_ENTER；演练前置断言走确定性构造）。
        let sig = ArrivalSig::new(SPURIOUS_VECTOR, false, true);
        let mut n: u32 = 0;
        while n < DRILL_PER_STATE {
            let want = eng.expected_state(sig, eng.storm_latched());
            let rep = eng.arrive(sig)?;
            expected[3] += 1;
            if rep.actual.tag() == SpurState::SpurStorm.tag() && rep.expected.tag() == want.tag() {
                correct[3] += 1;
            }
            if rep.disposition.tag() != disposition_of(rep.actual).tag() {
                mismatch_events += 1;
            }
            if rep.fault_detected.is_some() {
                misjudge_events += 1;
            }
            n += 1;
        }
    }

    // --- 态 4：观察态（未占用向量可疑签名 → 观察留痕，达阈升格）---
    {
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true)?;
        let sig = ArrivalSig::new(0x50, true, false); // 未 claim → 签名可疑
        let mut n: u32 = 0;
        while n < DRILL_PER_STATE {
            let want = eng.expected_state(sig, eng.storm_latched());
            let rep = eng.arrive(sig)?;
            expected[4] += 1;
            if rep.actual.tag() == SpurState::Observing.tag() && rep.expected.tag() == want.tag() {
                correct[4] += 1;
            }
            if rep.disposition.tag() != disposition_of(rep.actual).tag() {
                mismatch_events += 1;
            }
            if rep.fault_detected.is_some() {
                misjudge_events += 1;
            }
            n += 1;
        }
    }

    let injections = DRILL_PER_STATE * 5;
    let correct_total =
        correct[0] + correct[1] + correct[2] + correct[3] + correct[4];
    let rate_bp = if injections == 0 {
        0
    } else {
        (correct_total * 10000) / injections
    };
    let ok = correct_total == injections && mismatch_events == 0 && misjudge_events == 0;
    Ok(DrillReport {
        expected,
        correct,
        injections,
        correct_total,
        rate_bp,
        mismatch_events,
        misjudge_events,
        ok,
    })
}

// ===========================================================================
// 十二、双跑确定性对拍（同种子同操作序列 → 两实例指纹全等）
// ===========================================================================

/// 确定性线性同余发生器（演练语料源；与同域 F0826 Lcg 同法：
/// x = x·6364136223846793005 + 1442695040888963407 (wrapping)）。
#[derive(Debug, Clone, Copy)]
pub struct Lcg(u64);

impl Lcg {
    /// 新种子。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    /// 下一原始值。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    /// 有界值 [0, bound)（bound=0 归 0——除零零路径）。
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
}

/// 演练操作（双跑语料的最小指令集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrillOp {
    /// 设置使能态。
    SetLine(bool),
    /// 占用向量。
    Claim(u8),
    /// 释放向量。
    Release(u8),
    /// 一次到达签名。
    Arrive(ArrivalSig),
    /// 闲置 n tick。
    Idle(u8),
}

/// 双跑报告。
#[derive(Debug, Clone, Copy)]
pub struct DualRunReport {
    /// 操作数。
    pub ops: u32,
    /// 实例 A 状态指纹。
    pub fp_a: u64,
    /// 实例 B 状态指纹。
    pub fp_b: u64,
    /// 实例 A 事件账指纹。
    pub ledger_a: u64,
    /// 实例 B 事件账指纹。
    pub ledger_b: u64,
    /// 双指纹全等。
    pub equal: bool,
}

/// 生成确定性操作序列（seed 相同 → 序列逐条相同）。
///
/// 语料纪律：到达/占用/释放共用 16 向量池（0x30..=0x3F）+ 偶发 0xFF——
/// 观察表（OBS_CAP=32）在任意种子下必然装得下（distinct 在观向量 ≤17），
/// 容量契约不被随机语料击穿；使能翻转与闲置推进穿插制造滞回/初始化窗路径。
pub fn generate_ops(seed: u64, n: u32) -> Vec<DrillOp> {
    let mut rng = Lcg::new(seed);
    let mut out = Vec::new();
    let mut i: u32 = 0;
    while i < n {
        let pick = rng.below(100);
        if pick < 25 {
            // 到达（池内向量 / 签名随机——覆盖多态路径）。
            let vec = (rng.below(16) + 0x30) as u8;
            let isr = rng.below(2) == 0;
            let arb = rng.below(4) == 0;
            out.push(DrillOp::Arrive(ArrivalSig::new(vec, isr, arb)));
        } else if pick < 40 {
            out.push(DrillOp::Idle((rng.below(32) as u8) + 1));
        } else if pick < 55 {
            let b = rng.below(2) == 0;
            out.push(DrillOp::SetLine(b));
        } else if pick < 75 {
            let v = (rng.below(16) + 0x30) as u8;
            out.push(DrillOp::Claim(v));
        } else if pick < 90 {
            let v = (rng.below(16) + 0x30) as u8;
            out.push(DrillOp::Release(v));
        } else {
            // 偶发 0xFF spurious 向量直达。
            out.push(DrillOp::Arrive(ArrivalSig::new(SPURIOUS_VECTOR, false, true)));
        }
        i += 1;
    }
    out
}

/// 在新引擎上重放操作序列（可失败操作吞计数不中断——重放语义：环境操作
/// 的失败（重复占用/空释放）是语料噪声，到达失败（账满）向上传播）。
pub fn run_ops(ops: &[DrillOp]) -> Result<SpurFiveEngine, SpurFiveError> {
    let mut eng = SpurFiveEngine::new();
    let mut i: usize = 0;
    while i < ops.len() {
        match ops[i] {
            DrillOp::SetLine(b) => {
                let _ = eng.set_line_enabled(b);
            }
            DrillOp::Claim(v) => {
                let _ = eng.claim_vector(v as u16);
            }
            DrillOp::Release(v) => {
                let _ = eng.release_vector(v as u16);
            }
            DrillOp::Arrive(sig) => {
                eng.arrive(sig)?;
            }
            DrillOp::Idle(n) => {
                eng.idle(n as u32)?;
            }
        }
        i += 1;
    }
    Ok(eng)
}

/// 双跑对拍（同种子两独立实例；指纹不等 = 确定性破缺 → Err）。
pub fn dual_run(n: u32, seed: u64) -> Result<DualRunReport, SpurFiveError> {
    let ops = generate_ops(seed, n);
    let a = run_ops(&ops)?;
    let b = run_ops(&ops)?;
    let fp_a = a.state_fingerprint();
    let fp_b = b.state_fingerprint();
    let ledger_a = a.ledger_fingerprint();
    let ledger_b = b.ledger_fingerprint();
    let equal = fp_a == fp_b && ledger_a == ledger_b;
    if !equal {
        return Err(SpurFiveError::DualRunMismatch { fp_a, fp_b });
    }
    Ok(DualRunReport { ops: n, fp_a, fp_b, ledger_a, ledger_b, equal })
}

// ===========================================================================
// 十三、注入面（态误判 / 处置错位 / 越级降档——各 10 次检出）
// ===========================================================================

/// 注入报告（三面各 10/10 检出自证）。
#[derive(Debug, Clone, Copy)]
pub struct InjectionReport {
    /// 态误判注入次数。
    pub misjudge_armed: u32,
    /// 态误判检出次数。
    pub misjudge_detected: u32,
    /// 处置错位注入次数。
    pub mismatch_armed: u32,
    /// 处置错位检出次数。
    pub mismatch_detected: u32,
    /// 越级降档尝试次数。
    pub downgrade_attempts: u32,
    /// 越级降档拦截次数。
    pub downgrade_blocked: u32,
    /// 三面全 10/10。
    pub ok: bool,
}

impl InjectionReport {
    /// 渲染单行。
    pub fn render(&self) -> String {
        format!(
            "[inject] ok={} misjudge {}/{} mismatch {}/{} downgrade {}/{}",
            self.ok,
            self.misjudge_detected,
            self.misjudge_armed,
            self.mismatch_detected,
            self.mismatch_armed,
            self.downgrade_blocked,
            self.downgrade_attempts
        )
    }
}

/// 三面注入主入口（每面独立引擎——故障不跨面污染）。
pub fn run_injections() -> Result<InjectionReport, SpurFiveError> {
    // --- 面 1：态误判注入 ×10（钩子扭曲分类 → 误判复检器必抓）---
    let mut misjudge_detected: u32 = 0;
    {
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true)?;
        eng.claim_vector(0x41)?;
        eng.arm_fault(FaultHook::Misjudge)?;
        let sig = ArrivalSig::new(0x41, true, false); // 期望真中断
        let mut n: u32 = 0;
        while n < DRILL_PER_STATE {
            let rep = eng.arrive(sig)?;
            // 检出面 1：回报 fault_detected 命中误判码；
            // 检出面 2：红账该码 +1；
            // 双面任一成立即计检出（判据口径：检出=有任一信号——实测双面恒同时成立）。
            let by_report = rep.fault_detected == Option::Some(X_SF_MISJUDGE_DETECTED);
            let by_red = eng.reds_count(X_SF_MISJUDGE_DETECTED) > n;
            if by_report || by_red {
                misjudge_detected += 1;
            }
            n += 1;
        }
        eng.clear_fault()?;
    }

    // --- 面 2：处置错位注入 ×10（钩子扭曲查表分发 → 双层检测器必抓）---
    let mut mismatch_detected: u32 = 0;
    {
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true)?;
        eng.claim_vector(0x41)?;
        eng.arm_fault(FaultHook::Mismatch)?;
        let sig = ArrivalSig::new(0x41, true, false); // 查表层错位：EoiPass 被扭为 RecordNoEoi
        let mut n: u32 = 0;
        while n < DRILL_PER_STATE {
            let rep = eng.arrive(sig)?;
            let by_report = rep.fault_detected == Option::Some(X_SF_DISPO_MISMATCH);
            if by_report {
                mismatch_detected += 1;
            }
            n += 1;
        }
        eng.clear_fault()?;
    }

    // --- 面 3：越级降档注入 ×10（对已立案向量降档 → 单向闸必拦）---
    let mut downgrade_blocked: u32 = 0;
    {
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true)?;
        // 观察 0x60 至升格（8 次 → 立案）。
        let obs_sig = ArrivalSig::new(0x60, true, false);
        let mut p: u32 = 0;
        while p < OBS_FILE_THRESHOLD as u32 {
            eng.arrive(obs_sig)?;
            p += 1;
        }
        // 立案前置断言（确定性构造——未立案则注入无对象，直接报错码路径自证）。
        if !eng.is_filed(0x60) {
            return Err(SpurFiveError::SlotFree(0x60));
        }
        let mut n: u32 = 0;
        while n < DRILL_PER_STATE {
            match eng.try_downgrade(0x60) {
                Err(SpurFiveError::DowngradeBlocked(0x60)) => {
                    downgrade_blocked += 1;
                }
                _ => {
                    // 降档竟然成功/错码——单向闸破缺，视为未检出。
                }
            }
            n += 1;
        }
    }

    let ok = misjudge_detected == DRILL_PER_STATE
        && mismatch_detected == DRILL_PER_STATE
        && downgrade_blocked == DRILL_PER_STATE;
    Ok(InjectionReport {
        misjudge_armed: DRILL_PER_STATE,
        misjudge_detected,
        mismatch_armed: DRILL_PER_STATE,
        mismatch_detected,
        downgrade_attempts: DRILL_PER_STATE,
        downgrade_blocked,
        ok,
    })
}

impl SpurFiveEngine {
    /// 红账指定码计数（注入面/审计面便捷谓词）。
    pub fn reds_count(&self, code: u16) -> u32 {
        self.reds.count_code(code)
    }
}

// ===========================================================================
// 十四、测试（隔离探针冒烟——探针 crate 内以 std 测试框架运行）
// ===========================================================================

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    // --- 基础面 ---

    #[test]
    fn t_codes_closed_set() {
        // 16 码全表：段内每码必有中文名，段外归「未知」。
        let mut c: u16 = SEG_BASE;
        while c <= SEG_TOP {
            let name = code_name(c);
            assert!(name != "未知诊断码", "0x{:X} 应在段内有名", c);
            c += 1;
        }
        assert_eq!(code_name(SEG_BASE - 1), "未知诊断码");
        assert_eq!(code_name(SEG_TOP + 1), "未知诊断码");
    }

    #[test]
    fn t_error_details_all_variants() {
        // 每变体：码在段内、detail 非空且含可操作语义（长度 ≥ 10）。
        let samples = [
            SpurFiveError::VecRange(0x100),
            SpurFiveError::LineStateBad(true),
            SpurFiveError::SlotBusy(0x41),
            SpurFiveError::SlotFree(0x41),
            SpurFiveError::SpurSlotLocked(0xFF),
            SpurFiveError::LedgerFull,
            SpurFiveError::RedFull,
            SpurFiveError::CaseFull,
            SpurFiveError::ObsTableFull(0x50),
            SpurFiveError::MisjudgeDetected { vec: 1, expect_tag: 0, got_tag: 1 },
            SpurFiveError::DispoMismatch { state_tag: 0, expect_tag: 0, got_tag: 1 },
            SpurFiveError::DowngradeBlocked(0x60),
            SpurFiveError::SnapshotCorrupt(0xBEEF),
            SpurFiveError::RoundtripMismatch,
            SpurFiveError::DualRunMismatch { fp_a: 1, fp_b: 2 },
        ];
        let mut i: usize = 0;
        while i < samples.len() {
            let e = samples[i];
            assert!(e.code() > SEG_BASE && e.code() <= SEG_TOP);
            let d = e.detail();
            assert!(d.len() >= 10, "变体 {:?} 诊断过短", e);
            // 一码一义：两两码互异。
            let mut j = i + 1;
            while j < samples.len() {
                assert!(samples[i].code() != samples[j].code(), "错误码重复");
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t_decide_table_five_states() {
        // 五特征组 → 五态（与编译期 const 断言一致的运行期复证）。
        assert_eq!(decide_from_bits(FEAT_LINE_DISABLED), SpurState::SpurDisabled);
        assert_eq!(
            decide_from_bits(FEAT_STORM_ENGAGED | FEAT_SPUR_SIG),
            SpurState::SpurStorm
        );
        assert_eq!(decide_from_bits(FEAT_SPUR_SIG), SpurState::SpurEnabled);
        assert_eq!(decide_from_bits(FEAT_SUSPICIOUS), SpurState::Observing);
        assert_eq!(decide_from_bits(0), SpurState::Real);
        // 优先级：未使能期压倒一切（即使风暴+spurious 签名）。
        let all = FEAT_LINE_DISABLED | FEAT_STORM_ENGAGED | FEAT_SPUR_SIG | FEAT_SUSPICIOUS;
        assert_eq!(decide_from_bits(all), SpurState::SpurDisabled);
        // 风暴不带 spurious 签名 → 不入风暴态（真签名完备正常服务）。
        assert_eq!(decide_from_bits(FEAT_STORM_ENGAGED), SpurState::Real);
        // 规则名可查。
        assert_eq!(rule_name_of(all), "P0 未使能期(AP初始化窗)");
    }

    #[test]
    fn t_disposition_table_zero_mismatch() {
        // 每态查表处置正确 + EOI 唯一性 + 计数指纹基底。
        let states = [
            SpurState::Real,
            SpurState::SpurEnabled,
            SpurState::SpurDisabled,
            SpurState::SpurStorm,
            SpurState::Observing,
        ];
        let wants = [
            Disposition::EoiPass,
            Disposition::RecordNoEoi,
            Disposition::EscalateFile,
            Disposition::RateLimit,
            Disposition::ObserveTrail,
        ];
        let mut i: usize = 0;
        while i < 5 {
            assert_eq!(disposition_of(states[i]), wants[i]);
            assert_eq!(DISPOSITION_TABLE[states[i].tag() as usize].tag(), wants[i].tag());
            i += 1;
        }
        assert!(Disposition::EoiPass.issues_eoi());
        // 计数增量指纹：正确执行唯一触点。
        let mut dc = DispCounters::zero();
        let before = dc;
        dc.inc(Disposition::RateLimit);
        let delta = dc.delta_from(&before);
        assert!(delta.delta_matches(Disposition::RateLimit));
        assert!(!delta.delta_matches(Disposition::EoiPass));
        assert_eq!(dc.total(), 1);
    }

    #[test]
    fn t_slot_table_discipline() {
        let mut eng = SpurFiveEngine::new();
        assert!(matches!(eng.claim_vector(0x100), Err(SpurFiveError::VecRange(0x100))));
        assert!(matches!(eng.claim_vector(0xFF), Err(SpurFiveError::SpurSlotLocked(0xFF))));
        assert_eq!(eng.claim_vector(0x41), Ok(0x41));
        assert!(matches!(eng.claim_vector(0x41), Err(SpurFiveError::SlotBusy(0x41))));
        assert!(eng.is_claimed(0x41));
        assert_eq!(eng.claimed_count(), 1);
        assert!(matches!(eng.release_vector(0x42), Err(SpurFiveError::SlotFree(0x42))));
        assert_eq!(eng.release_vector(0x41), Ok(0x41));
        assert!(!eng.is_claimed(0x41));
        // 使能态翻转纪律。
        assert!(matches!(eng.set_line_enabled(false), Err(SpurFiveError::LineStateBad(false))));
        assert!(eng.set_line_enabled(true).is_ok());
        assert!(matches!(eng.set_line_enabled(true), Err(SpurFiveError::LineStateBad(true))));
    }

    #[test]
    fn t_bitplane_basics() {
        let mut p = BitPlane256::zero();
        assert_eq!(p.count(), 0);
        assert!(p.set(0) && p.set(255) && p.set(64));
        assert!(!p.set(0));
        assert_eq!(p.count(), 3);
        assert!(p.test(0) && p.test(64) && p.test(255));
        assert!(p.clear(64) && !p.clear(64));
        assert_eq!(p.count(), 2);
        // 序列化往返。
        let q = BitPlane256::from_bytes(&p.to_bytes());
        assert_eq!(p, q);
    }

    // --- 五态主路径 ---

    #[test]
    fn t_real_path_eoi_pass() {
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true).unwrap();
        eng.claim_vector(0x41).unwrap();
        let sig = ArrivalSig::new(0x41, true, false);
        let mut n = 0;
        while n < 10 {
            let rep = eng.arrive(sig).unwrap();
            assert_eq!(rep.actual, SpurState::Real);
            assert_eq!(rep.expected, SpurState::Real);
            assert_eq!(rep.disposition, Disposition::EoiPass);
            assert!(rep.eoi_issued);
            n += 1;
        }
        assert_eq!(eng.eoi_issued(), 10);
        assert_eq!(eng.state_counts()[0], 10);
        assert!(eng.health().green, "干净跑体检应绿：{}", eng.render_health());
        assert!(eng.conservation().balanced);
        // 真中断不入率窗。
        assert_eq!(eng.window_rate(), 0);
    }

    #[test]
    fn t_spur_enabled_no_eoi() {
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true).unwrap();
        let sig = ArrivalSig::new(SPURIOUS_VECTOR, false, true);
        let mut n = 0;
        while n < 10 {
            let rep = eng.arrive(sig).unwrap();
            assert_eq!(rep.actual, SpurState::SpurEnabled);
            assert_eq!(rep.disposition, Disposition::RecordNoEoi);
            assert!(!rep.eoi_issued, "使能期伪中断不得发 EOI");
            assert!(rep.fault_detected.is_none());
            n += 1;
        }
        assert_eq!(eng.eoi_issued(), 0);
        assert_eq!(eng.state_counts()[1], 10);
        // 非真入窗：率 = 10。
        assert_eq!(eng.window_rate(), 10);
        assert!(eng.health().green);
    }

    #[test]
    fn t_spur_disabled_files_case() {
        let mut eng = SpurFiveEngine::new();
        // 引擎初值 = AP 初始化窗（line 未使能）。
        assert!(!eng.line_enabled());
        let sig = ArrivalSig::new(0x42, false, false);
        let rep1 = eng.arrive(sig).unwrap();
        assert_eq!(rep1.actual, SpurState::SpurDisabled);
        assert_eq!(rep1.disposition, Disposition::EscalateFile);
        assert!(rep1.case_filed, "首次未使能期到达必须立案");
        assert!(!rep1.eoi_issued);
        assert!(eng.is_filed(0x42));
        assert_eq!(eng.case_count(), 1);
        // 后续 9 次：dedup（不重复立）但处置仍升级立案、计数仍增。
        let mut n = 0;
        while n < 9 {
            let rep = eng.arrive(sig).unwrap();
            assert_eq!(rep.actual, SpurState::SpurDisabled);
            assert_eq!(rep.disposition, Disposition::EscalateFile);
            assert!(!rep.case_filed, "已立案向量不得重复立案");
            n += 1;
        }
        assert_eq!(eng.case_count(), 1);
        assert_eq!(eng.state_counts()[2], 10);
        assert_eq!(eng.disp_counters().get(Disposition::EscalateFile), 10);
        assert!(eng.conservation().balanced);
        assert!(eng.health().green);
    }

    #[test]
    fn t_storm_hysteresis() {
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true).unwrap();
        let sig = ArrivalSig::new(SPURIOUS_VECTOR, false, true);
        // (a) 63 次：未入风暴（阈下 1）。
        let mut n = 0;
        while n < STORM_ENTER - 1 {
            let rep = eng.arrive(sig).unwrap();
            assert_eq!(rep.actual, SpurState::SpurEnabled, "阈下应为使能期伪");
            n += 1;
        }
        assert!(!eng.storm_latched());
        // (b) 第 64 次：跨越事件按跨越前态处置（仍 SpurEnabled），入窗后锁存。
        let rep64 = eng.arrive(sig).unwrap();
        assert_eq!(rep64.actual, SpurState::SpurEnabled);
        assert!(eng.storm_latched(), "率 64 ≥ 64 必入风暴");
        assert_eq!(eng.storm_transitions().0, 1);
        // (c) 风暴期 10 次：限流处置。
        let mut n = 0;
        while n < 10 {
            let rep = eng.arrive(sig).unwrap();
            assert_eq!(rep.actual, SpurState::SpurStorm);
            assert_eq!(rep.disposition, Disposition::RateLimit);
            assert!(!rep.eoi_issued);
            n += 1;
        }
        // (d) 滞回带内保持：环形淘汰要到 head 绕回最旧事件槽（tick 257 起）才
        //     开始扣减——idle(206)（tick 74→280，淘汰事件槽 1..24）→ 率 74-24=50
        //     （17~63 带内），不得退风暴。
        eng.idle(206).unwrap();
        assert_eq!(eng.window_rate(), 50);
        assert!(eng.storm_latched(), "带内（50>16）必须保持风暴");
        // (e) 继续衰减 34（淘汰事件槽 25..58）→ 率 16 → 退风暴（≤16）。
        eng.idle(34).unwrap();
        assert_eq!(eng.window_rate(), 16);
        assert!(!eng.storm_latched(), "率 16 ≤ 16 必退风暴");
        assert_eq!(eng.storm_transitions().1, 1);
        // (f) 退后再爬到带内（40）：不得再入（40 < 64）。
        eng.idle(64).unwrap(); // 窗清空（率 0）。
        let mut n = 0;
        while n < 40 {
            eng.arrive(sig).unwrap();
            n += 1;
        }
        assert_eq!(eng.window_rate(), 40);
        assert!(!eng.storm_latched(), "带内（40<64）不得再入风暴");
        assert_eq!(eng.storm_transitions(), (1, 1), "进出各恰一次");
        assert!(eng.health().green);
        assert!(eng.conservation().balanced);
    }

    #[test]
    fn t_observation_escalation_one_way() {
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true).unwrap();
        let sig = ArrivalSig::new(0x50, true, false); // 未 claim → 可疑
        // 7 次：纯观察。
        let mut n = 0;
        while n < OBS_FILE_THRESHOLD - 1 {
            let rep = eng.arrive(sig).unwrap();
            assert_eq!(rep.actual, SpurState::Observing);
            assert_eq!(rep.disposition, Disposition::ObserveTrail);
            assert!(!rep.escalated);
            n += 1;
        }
        assert!(!eng.is_filed(0x50));
        // 第 8 次：升格立案。
        let rep8 = eng.arrive(sig).unwrap();
        assert_eq!(rep8.actual, SpurState::Observing, "升格不改态——处置仍是观察留痕");
        assert!(rep8.escalated, "计数达阈 8 必升格");
        assert!(eng.is_filed(0x50));
        assert_eq!(eng.case_count(), 1);
        // 第 9/10 次：已立案仍观察（不重复立、不降档）。
        let mut n = 0;
        while n < 2 {
            let rep = eng.arrive(sig).unwrap();
            assert_eq!(rep.actual, SpurState::Observing);
            assert!(!rep.escalated, "已立案不重复升格");
            n += 1;
        }
        assert_eq!(eng.case_count(), 1);
        assert_eq!(eng.obs_count(0x50), 10);
        // 单向闸：降档必拦 ×10。
        let mut blocked = 0;
        let mut n = 0;
        while n < 10 {
            if matches!(eng.try_downgrade(0x50), Err(SpurFiveError::DowngradeBlocked(0x50))) {
                blocked += 1;
            }
            n += 1;
        }
        assert_eq!(blocked, 10, "降档 10 次必须 10 次拦截");
        assert!(eng.is_filed(0x50), "拦截后立案位不得被清");
        assert_eq!(eng.downgrade_blocks(), 10);
        assert_eq!(eng.reds_count(X_SF_DOWNGRADE_BLOCKED), 10);
        // 降档对未立案向量：SlotFree 错码。
        assert!(matches!(eng.try_downgrade(0x51), Err(SpurFiveError::SlotFree(0x51))));
        assert!(eng.conservation().balanced);
        // 有红（降档拦截入红账）→ 体检红——符合预期语义。
        assert!(!eng.health().green);
    }

    // --- J1 主判据 ---

    #[test]
    fn t_drill_100_percent() {
        let rep = run_drill().unwrap();
        assert_eq!(rep.injections, 50);
        assert_eq!(rep.correct_total, 50, "五态判定必须 50/50：{}", rep.render());
        let mut i = 0;
        while i < 5 {
            assert_eq!(rep.correct[i], 10, "态 {} 必须 10/10", STATE_NAMES[i]);
            assert_eq!(rep.expected[i], 10);
            i += 1;
        }
        assert_eq!(rep.rate_bp, 10000, "正确率必须恰 10000bp（100%）");
        assert_eq!(rep.mismatch_events, 0, "演练全程处置零错位");
        assert_eq!(rep.misjudge_events, 0, "演练不打钩子零误判");
        assert!(rep.ok);
    }

    #[test]
    fn t_injection_surface_three_faces() {
        let rep = run_injections().unwrap();
        assert_eq!(rep.misjudge_armed, 10);
        assert_eq!(rep.misjudge_detected, 10, "态误判注入必须 10/10 检出：{}", rep.render());
        assert_eq!(rep.mismatch_armed, 10);
        assert_eq!(rep.mismatch_detected, 10, "处置错位注入必须 10/10 检出");
        assert_eq!(rep.downgrade_attempts, 10);
        assert_eq!(rep.downgrade_blocked, 10, "越级降档注入必须 10/10 拦截");
        assert!(rep.ok);
    }

    #[test]
    fn t_inject_misjudge_red_trace() {
        // 误判注入的红账轨迹与守恒（检出后账记实况）。
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true).unwrap();
        eng.claim_vector(0x41).unwrap();
        eng.arm_fault(FaultHook::Misjudge).unwrap();
        let sig = ArrivalSig::new(0x41, true, false);
        let mut hits = 0;
        let mut n = 0;
        while n < 10 {
            let rep = eng.arrive(sig).unwrap();
            assert_eq!(rep.fault_detected, Option::Some(X_SF_MISJUDGE_DETECTED));
            // 期望真中断被扭为使能期伪（tag 0+1=1）。
            assert_eq!(rep.expected, SpurState::Real);
            assert_eq!(rep.actual, SpurState::SpurEnabled);
            assert!(!rep.eoi_issued, "被扭态走了拒 EOI 处置——实况账");
            hits += 1;
            n += 1;
        }
        assert_eq!(hits, 10);
        assert_eq!(eng.misjudge_hits(), 10);
        assert_eq!(eng.reds_count(X_SF_MISJUDGE_DETECTED), 10);
        // 守恒仍平（账记实况：实判态计数与实际执行处置一致）。
        assert!(eng.conservation().balanced, "{}", eng.render_health());
        assert!(!eng.health().green, "有红项体检必红");
        eng.clear_fault().unwrap();
        // 解钩后恢复零误判。
        let rep = eng.arrive(sig).unwrap();
        assert_eq!(rep.actual, SpurState::Real);
        assert!(rep.fault_detected.is_none());
    }

    #[test]
    fn t_inject_mismatch_detector_layers() {
        // 处置错位注入：查表层检出 + 实况账 + 守恒。
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true).unwrap();
        eng.claim_vector(0x41).unwrap();
        eng.arm_fault(FaultHook::Mismatch).unwrap();
        let sig = ArrivalSig::new(0x41, true, false);
        let mut n = 0;
        while n < 10 {
            let rep = eng.arrive(sig).unwrap();
            assert_eq!(rep.fault_detected, Option::Some(X_SF_DISPO_MISMATCH));
            // 态判真中断，处置被扭为 RecordNoEoi（tag 0+1=1）——实况：拒 EOI。
            assert_eq!(rep.actual, SpurState::Real);
            assert_eq!(rep.disposition, Disposition::RecordNoEoi);
            assert!(!rep.eoi_issued);
            n += 1;
        }
        assert_eq!(eng.mismatch_hits(), 10);
        // 红账错位码：查表层 10 条（执行层无破缺——执行忠实于被扭参数）。
        assert_eq!(eng.reds_count(X_SF_DISPO_MISMATCH), 10);
        assert!(eng.conservation().balanced);
        assert!(!eng.health().green);
        eng.clear_fault().unwrap();
        let rep = eng.arrive(sig).unwrap();
        assert_eq!(rep.disposition, Disposition::EoiPass);
        assert!(rep.eoi_issued);
        assert!(rep.fault_detected.is_none());
    }

    #[test]
    fn t_inject_downgrade_hook_path() {
        // 降档钩子路径：升格瞬间钩子发起降档——必被拦且入红账。
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true).unwrap();
        eng.arm_fault(FaultHook::Downgrade).unwrap();
        let sig = ArrivalSig::new(0x60, true, false);
        let mut escalated_at: u32 = 0;
        let mut n = 0;
        while n < 10 {
            let rep = eng.arrive(sig).unwrap();
            if rep.escalated {
                escalated_at = n + 1;
                // 升格当次钩子降档被拦——回报检出码。
                assert_eq!(rep.fault_detected, Option::Some(X_SF_DOWNGRADE_BLOCKED));
                break;
            }
            n += 1;
        }
        assert_eq!(escalated_at, OBS_FILE_THRESHOLD as u32, "第 8 次必升格");
        assert!(eng.is_filed(0x60), "拦截后立案位保持");
        assert_eq!(eng.downgrade_blocks(), 1);
        assert_eq!(eng.reds_count(X_SF_DOWNGRADE_BLOCKED), 1);
        eng.clear_fault().unwrap();
    }

    // --- 确定性 ---

    #[test]
    fn t_dual_run_determinism() {
        let rep = dual_run(600, 0x0845_0001_BEEF).unwrap();
        assert!(rep.equal);
        assert_eq!(rep.fp_a, rep.fp_b);
        assert_eq!(rep.ledger_a, rep.ledger_b);
        assert_eq!(rep.ops, 600);
        // 换种子也能跑通（语料覆盖 SetLine/Claim/Release/Arrive/Idle 全指令）。
        let rep2 = dual_run(600, 0x9999_1111_CAFE).unwrap();
        assert!(rep2.equal);
        // 双跑引擎守恒必平（run_ops 不吞到达错误）。
        let ops = generate_ops(0x0845_0001_BEEF, 600);
        let eng = run_ops(&ops).unwrap();
        assert!(eng.conservation().balanced, "{}", eng.render_health());
        // 无钩子语料跑完：零误判零错位零降档（红账干净）。
        assert_eq!(eng.misjudge_hits(), 0);
        assert_eq!(eng.mismatch_hits(), 0);
        assert_eq!(eng.red_len(), 0);
    }

    #[test]
    fn t_ledger_tick_builtin_and_fingerprint() {
        // 账 tick 内置：不暴露任何 tick 写入口；事件 seq/tick 单调。
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true).unwrap();
        let sig = ArrivalSig::new(0x41, true, false);
        eng.claim_vector(0x41).unwrap();
        let t0 = eng.ledger_tick();
        eng.arrive(sig).unwrap();
        assert!(eng.ledger_tick() > t0, "到达必推进账 tick");
        // 同操作序列两引擎：账指纹全等。
        let mut eng2 = SpurFiveEngine::new();
        eng2.set_line_enabled(true).unwrap();
        eng2.claim_vector(0x41).unwrap();
        eng2.arrive(sig).unwrap();
        assert_eq!(eng.ledger_fingerprint(), eng2.ledger_fingerprint());
        assert_eq!(eng.state_fingerprint(), eng2.state_fingerprint());
    }

    #[test]
    fn t_ledger_capacity_and_drop_accounting() {
        // 打满事件账（8192）：主入账路径报 LedgerFull，drops 计数不静默。
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true).unwrap();
        let sig = ArrivalSig::new(0x41, true, false);
        eng.claim_vector(0x41).unwrap();
        let mut first_err: Option<SpurFiveError> = Option::None;
        let mut n: u64 = 0;
        while n < 12000 {
            match eng.arrive(sig) {
                Ok(_) => {}
                Err(SpurFiveError::LedgerFull) => {
                    first_err = Option::Some(SpurFiveError::LedgerFull);
                    break;
                }
                Err(e) => panic!("意外错误: {:?}", e),
            }
            n += 1;
        }
        assert!(matches!(first_err, Option::Some(SpurFiveError::LedgerFull)));
        assert!(eng.ledger_len() <= LEDGER_CAP);
        // 守恒仍平：被拒到达未入账也未计数。
        assert!(eng.conservation().balanced, "{}", eng.render_health());
        assert!(!eng.health().green, "账满丢弃>0 体检必红");
    }

    // --- 快照子系统 ---

    #[test]
    fn t_snapshot_roundtrip_clean() {
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true).unwrap();
        eng.claim_vector(0x41).unwrap();
        eng.claim_vector(0x80).unwrap();
        let real = ArrivalSig::new(0x41, true, false);
        let spur = ArrivalSig::new(SPURIOUS_VECTOR, false, true);
        let obs = ArrivalSig::new(0x60, true, false);
        let mut n = 0;
        while n < 9 {
            eng.arrive(real).unwrap();
            eng.arrive(spur).unwrap();
            eng.arrive(obs).unwrap();
            n += 1;
        }
        eng.idle(5).unwrap();
        let fp = eng.snapshot_roundtrip().unwrap();
        assert_ne!(fp, 0);
        // 快照字节级自反：capture→to_bytes→from_bytes 等价。
        let snap = EngineSnapshot::capture(&eng);
        let raw = snap.raw;
        let snap2 = EngineSnapshot::from_bytes(&raw).unwrap();
        assert_eq!(snap.fingerprint(), snap2.fingerprint());
        assert_eq!(snap2.restore().unwrap().arrivals, eng.arrivals());
        // 核字段抽查。
        let core = snap.restore().unwrap();
        assert!(core.line_enabled);
        assert!(core.slots.test(0x41) && core.slots.test(0x80));
        assert!(core.filed.test(0x60), "0x60 观察 9 次应已升格立案");
        assert_eq!(core.obs_counts.len(), 1);
        assert_eq!(core.obs_counts[0], (0x60, 9));
        assert_eq!(core.window.rate(), eng.window_rate());
    }

    #[test]
    fn t_snapshot_corruption_detected() {
        let eng = SpurFiveEngine::new();
        let snap = EngineSnapshot::capture(&eng);
        // (a) 魔数破坏。
        let mut bad = snap;
        bad.raw[O_MAGIC] = bad.raw[O_MAGIC].wrapping_add(1);
        assert!(matches!(bad.validate(), Err(SpurFiveError::SnapshotCorrupt(_))));
        // (b) 版本破坏。
        let mut bad = snap;
        bad.raw[O_VERSION] = 0xFF;
        assert!(matches!(bad.validate(), Err(SpurFiveError::SnapshotCorrupt(_))));
        // (c) CRC 破坏（改一个数据字节，CRC 不再吻合）。
        let mut bad = snap;
        bad.raw[100] = bad.raw[100].wrapping_add(1);
        assert!(matches!(bad.validate(), Err(SpurFiveError::SnapshotCorrupt(_))));
        // (d) 头位交叉校验破坏（head 与 ticks 脱钩）。
        let mut bad = snap;
        bad.raw[O_HEAD] = bad.raw[O_HEAD].wrapping_add(1);
        bad.raw[O_HEAD + 1] = bad.raw[O_HEAD + 1].wrapping_add(1);
        // 重算 CRC 绕过 CRC 闸——头位交叉闸必须独立抓出。
        let crc = crc16(&bad.raw[0..SNAP_CRC_AT]);
        let hi = (crc >> 8) as u8;
        let lo = (crc & 0xFF) as u8;
        bad.raw[SNAP_CRC_AT] = lo;
        bad.raw[SNAP_CRC_AT + 1] = hi;
        assert!(
            matches!(bad.validate(), Err(SpurFiveError::SnapshotCorrupt(_))),
            "头位交叉闸必须独立于 CRC 闸"
        );
        // (e) from_bytes 拒损坏输入。
        let mut garbage = [0u8; SNAP_LEN];
        garbage.copy_from_slice(&snap.raw);
        garbage[200] ^= 0x5A;
        assert!(EngineSnapshot::from_bytes(&garbage).is_err());
        // (f) 完好快照 from_bytes 通过。
        assert!(EngineSnapshot::from_bytes(&snap.raw).is_ok());
    }

    // --- 窗与限流细节 ---

    #[test]
    fn t_storm_window_eviction() {
        let mut w = StormWindow::new();
        // 64 事件 → 率 64 入风暴。
        let mut i = 0;
        while i < 64 {
            w.advance();
            w.add_current();
            i += 1;
        }
        assert_eq!(w.rate(), 64);
        assert_eq!(w.evaluate(), Option::Some(true));
        assert!(w.latched());
        // 满 256 tick 闲置后：窗清空。
        let rate = w.idle(256);
        assert_eq!(rate, 0);
        assert_eq!(w.evaluate(), Option::Some(false));
        assert!(!w.latched());
        // 未入风暴的窗：evaluate 无跃迁。
        assert_eq!(w.evaluate(), Option::None);
    }

    #[test]
    fn t_rate_limit_sampling() {
        // 风暴期限流：全量计数 + 1/4 采样审计（账面采样/跳过标记可查）。
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true).unwrap();
        let sig = ArrivalSig::new(SPURIOUS_VECTOR, false, true);
        let mut n = 0;
        while n < 74 {
            eng.arrive(sig).unwrap(); // 64 次铺窗 + 10 次风暴态
            n += 1;
        }
        assert_eq!(eng.state_counts()[3], 10, "风暴态恰 10 次");
        assert_eq!(eng.disp_counters().get(Disposition::RateLimit), 10);
        // 采样审计：限流计数 1..10 → 采样位在 4/8（%4==0）→ 2 采样 8 跳过。
        let counts = eng.ledger_kind_counts();
        assert_eq!(counts[EvKind::RateLimitSample.tag() as usize], 2);
        assert_eq!(counts[EvKind::RateLimitSkip.tag() as usize], 8);
        // 误判/错位为零。
        assert_eq!(eng.misjudge_hits(), 0);
        assert_eq!(eng.mismatch_hits(), 0);
        assert!(eng.health().green);
    }

    #[test]
    fn t_window_slots_snapshot_consistency() {
        // 窗槽只读拷贝与率一致（求和=率）。
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true).unwrap();
        let sig = ArrivalSig::new(SPURIOUS_VECTOR, false, true);
        let mut n = 0;
        while n < 30 {
            eng.arrive(sig).unwrap();
            n += 1;
        }
        let ws = eng.window_slots();
        let mut sum: u32 = 0;
        let mut i = 0;
        while i < ws.len() {
            sum += ws[i] as u32;
            i += 1;
        }
        assert_eq!(sum, eng.window_rate());
    }

    // --- 渲染与语料面 ---

    #[test]
    fn t_render_and_ops_surface() {
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true).unwrap();
        let _ = eng.claim_vector(0x41);
        let _ = eng.arrive(ArrivalSig::new(0x41, true, false));
        let line = eng.render_health();
        assert!(line.contains("[spurfive]"), "体检单行前缀：{}", line);
        assert!(line.contains("green=true"), "干净跑渲染绿：{}", line);
        let sig = ArrivalSig::new(0xFF, false, true);
        assert!(sig.render().contains("vec=0xFF"));
        // 语料生成确定性：同种子逐条相等。
        let a = generate_ops(42, 64);
        let b = generate_ops(42, 64);
        assert_eq!(a.len(), b.len());
        let mut i = 0;
        while i < a.len() {
            assert_eq!(a[i], b[i]);
            i += 1;
        }
        // Lcg 有界性。
        let mut rng = Lcg::new(7);
        let mut i = 0;
        while i < 100 {
            let v = rng.below(16);
            assert!(v < 16);
            i += 1;
        }
        assert_eq!(rng.below(0), 0);
        // 演练报告渲染。
        let rep = run_drill().unwrap();
        assert!(rep.render().contains("rate=10000bp"));
        let inj = run_injections().unwrap();
        assert!(inj.render().contains("ok=true"));
    }

    #[test]
    fn t_ledger_kind_counts_surface() {
        // 各事件类计数面可用（审计口径自证）。
        let mut eng = SpurFiveEngine::new();
        eng.set_line_enabled(true).unwrap();
        let _ = eng.claim_vector(0x41);
        let _ = eng.arrive(ArrivalSig::new(0x41, true, false));
        let _ = eng.arrive(ArrivalSig::new(SPURIOUS_VECTOR, false, true));
        let counts = eng.ledger_kind_counts();
        assert_eq!(counts[EvKind::Arrive.tag() as usize], 2);
        assert_eq!(counts[EvKind::EoiPass.tag() as usize], 1);
        assert_eq!(counts[EvKind::RecordNoEoi.tag() as usize], 1);
        assert_eq!(counts[EvKind::Classify.tag() as usize], 2);
    }

    #[test]
    fn t_obs_table_capacity_fail_fast() {
        // 观察表满：第 33 个可疑向量报 ObsTableFull（fail-fast 不静默丢）。
        let mut esc = ObsEscalator::new();
        let mut v: u8 = 0x10;
        while v < 0x10 + OBS_CAP as u8 {
            let _ = esc.observe(v, 1);
            v += 1;
        }
        assert!(matches!(esc.observe(0xF0, 1), Err(SpurFiveError::ObsTableFull(0xF0))));
        // 表内向量继续观察不受影响。
        assert!(esc.observe(0x10, 2).is_ok());
        assert_eq!(esc.obs_count(0x10), 2);
        // 立案账满：file 到 128 案报 CaseFull。
        let mut esc2 = ObsEscalator::new();
        let mut v: u8 = 0;
        while v < CASE_CAP as u8 {
            let r = esc2.file_disabled(v, 1);
            assert!(r.is_ok());
            v += 1;
        }
        assert!(matches!(esc2.file_disabled(0xFE, 1), Err(SpurFiveError::CaseFull)));
        // dedup：对已立案向量再直接立案 → Ok(None)。
        assert_eq!(esc2.file_disabled(0x00, 9).unwrap(), Option::None);
    }
}
