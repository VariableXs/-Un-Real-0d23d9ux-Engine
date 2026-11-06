//! UNX-F12744 UWOP 全码族压测回归 · 0x82400段
//!
//! 判据原文（派工卡 12744 逐字锚定）：
//!   UNX-F12745-J1 坏表拒止压测 20/20（截断/环/越界三态），fail-closed 语义维持。
//!
//! 设计要点：
//! 1. UWOP 操作码冻结闭集（模拟 winnt.h 展开码族语义，取值按卡冻结）：
//!    UWOP_PUSH_NONVOL=0、UWOP_ALLOC_SMALL=2、UWOP_ALLOC_LARGE=3、
//!    UWOP_SET_FPREG=4、UWOP_SAVE_NONVOL=5、UWOP_EPILOGUE=6、
//!    UWOP_SAVE_XMM128=9、UWOP_SAVE_XMM128_FAR=10、UWOP_PUSH_MACHFRAME=11，
//!    共 9 值；1/7/8 为闭集外码，from_code 显性拒。
//! 2. 语料库（冻结闭集 32 行，行可复算）：每行四元组 op/槽位/偏移/合法位型
//!    （packed = 操作码低半字节 | 槽位左移 4），由纯函数 corpus_row(i) 确定性
//!    复算，语料表第 i 行与复算结果恒等；槽位受各 op 的合法位型掩码约束，
//!    偏移落在各 op 的步长栅格上且不越上界。
//! 3. 语料状态机 Raw→Validated→Sealed 单调只进不退，倒退显性拒（EREWIND）；
//!    判定器 judge_uwop_corpus(row) 判定次序冻结：未登记 ENOENT → 位型越
//!    闭集 EINVAL → 状态倒退 EREWIND → 容量耗尽 ECapFull，先到先判。
//! 4. 坏表拒止注入器三类（截断表/环表/越界表）各 10 轮全抓：截断表 = 节点
//!    追随槽载荷不足声明所需；环表 = 后继回边/自环；越界表 = 后继越出表长。
//!    fail-closed 语义：坏表走查必回错误码、绝不 Ok。判据 20/20 口径 =
//!    截断 10 + 越界 10 主对全抓，环表 10 轮为三态补压、亦须全抓（合计
//!    30 轮全抓，与卡内"三类各 10 轮全抓"一致）。
//! 5. 双跑一致指纹：语料复算 + 32 行判定 + 封账 + 坏表注入压测 + 好表对照
//!    走查全程混入 FNV 指纹，两次独立全流程逐位一致；指纹全零视为异常。
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE 派生，
//!    序列确定）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!    无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12744 − 12600)）。
pub const SEG_BASE: u32 = 0x82400;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UwDiag {
    /// 未登记语料行（操作码越冻结闭集或行号越语料库 32 行）。
    ENoEnt,
    /// 位型越闭集（槽位越合法掩码/偏移失步/偏移越上界/打包位型不自洽）。
    EBits,
    /// 状态倒退（封账账本再判定/重封账，只进不退被显性拒）。
    ERewind,
    /// 截断表（节点声明的操作码需要追随槽，实际载荷不足）。
    ETrunc,
    /// 环表（后继回边/自环，单调链被打破）。
    ERing,
    /// 越界表（后继节点号越出表长）。
    ERange,
    /// 容量耗尽（账本槽位判满后新行拒入）。
    ECapFull,
    /// 未知操作码（表节点低半字节不在冻结闭集）。
    EOpBad,
    /// 未齐封账（语料 32 行未判齐即试图封账）。
    ESeal,
    /// 双跑指纹不一致（两次全流程指纹漂移，确定性被破坏）。
    EFp,
    /// 走查步数预算耗尽（疑似无限链，fail-closed 剪断）。
    EBudget,
}

impl UwDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                UwDiag::ENoEnt => 0x01,
                UwDiag::EBits => 0x02,
                UwDiag::ERewind => 0x03,
                UwDiag::ETrunc => 0x04,
                UwDiag::ERing => 0x05,
                UwDiag::ERange => 0x06,
                UwDiag::ECapFull => 0x07,
                UwDiag::EOpBad => 0x08,
                UwDiag::ESeal => 0x09,
                UwDiag::EFp => 0x0a,
                UwDiag::EBudget => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            UwDiag::ENoEnt => "未登记语料行：操作码不在 UWOP 冻结闭集或行号越 32 行语料库，先对语料表登记后再判",
            UwDiag::EBits => "位型越闭集：槽位越合法掩码/偏移失步/偏移越上界/打包不自洽，按该 op 的位型栅格重排行",
            UwDiag::ERewind => "状态倒退：Raw→Validated→Sealed 只进不退，封账后再判先解封走新账本",
            UwDiag::ETrunc => "截断表：节点追随槽载荷不足声明所需，补齐载荷或改用无追随槽的 op",
            UwDiag::ERing => "环表：后继回边/自环打破单调链，重排节点号使链严格递增",
            UwDiag::ERange => "越界表：后继节点号越出表长，核对表长与链尾 TERM 终点",
            UwDiag::ECapFull => "容量耗尽：账本槽位判满，先封账或换新账本再入新行",
            UwDiag::EOpBad => "未知操作码：表节点低半字节不在冻结闭集，核对 UWOP 全码族闭集表",
            UwDiag::ESeal => "未齐封账：语料 32 行未判齐即封，先补齐缺行再走封账口",
            UwDiag::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源（墙钟/随机/未初始化）",
            UwDiag::EBudget => "走查步数预算耗尽：链长超预算疑似无限环，fail-closed 剪断并复核表结构",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const UW_ALL: &[UwDiag] = &[
    UwDiag::ENoEnt,
    UwDiag::EBits,
    UwDiag::ERewind,
    UwDiag::ETrunc,
    UwDiag::ERing,
    UwDiag::ERange,
    UwDiag::ECapFull,
    UwDiag::EOpBad,
    UwDiag::ESeal,
    UwDiag::EFp,
    UwDiag::EBudget,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in UW_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82401..0x8240B 闭集内"
}

// ── UWOP 操作码冻结闭集 ─────────────────────────────────────────
/// UWOP 操作码冻结闭集（模拟 winnt.h 展开码族语义，取值按卡冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UwopOp {
    /// UWOP_PUSH_NONVOL = 0：非易失寄存器压栈。
    PushNonvol,
    /// UWOP_ALLOC_SMALL = 2：小栈分配（槽位即 (size/8)-1）。
    AllocSmall,
    /// UWOP_ALLOC_LARGE = 3：大栈分配（槽位为扩展标志，偏移域承尺寸）。
    AllocLarge,
    /// UWOP_SET_FPREG = 4：帧指针设定（偏移域承帧偏移）。
    SetFpreg,
    /// UWOP_SAVE_NONVOL = 5：非易失寄存器入槽保存（1 追随槽）。
    SaveNonvol,
    /// UWOP_EPILOGUE = 6：尾声标记（槽位为标志组）。
    Epilogue,
    /// UWOP_SAVE_XMM128 = 9：xmm128 就地保存（1 追随槽）。
    SaveXmm128,
    /// UWOP_SAVE_XMM128_FAR = 10：xmm128 远保存（2 追随槽）。
    SaveXmm128Far,
    /// UWOP_PUSH_MACHFRAME = 11：机器帧压栈（槽位为标志）。
    PushMachframe,
}

impl UwopOp {
    /// 操作码值（冻结闭集，0..=0xF 半字节域）。
    pub fn code(self) -> u32 {
        match self {
            UwopOp::PushNonvol => 0,
            UwopOp::AllocSmall => 2,
            UwopOp::AllocLarge => 3,
            UwopOp::SetFpreg => 4,
            UwopOp::SaveNonvol => 5,
            UwopOp::Epilogue => 6,
            UwopOp::SaveXmm128 => 9,
            UwopOp::SaveXmm128Far => 10,
            UwopOp::PushMachframe => 11,
        }
    }

    /// 由码反查闭集 op（越闭集返回 None）。
    pub fn from_code(code: u32) -> Option<UwopOp> {
        match code {
            0 => Some(UwopOp::PushNonvol),
            2 => Some(UwopOp::AllocSmall),
            3 => Some(UwopOp::AllocLarge),
            4 => Some(UwopOp::SetFpreg),
            5 => Some(UwopOp::SaveNonvol),
            6 => Some(UwopOp::Epilogue),
            9 => Some(UwopOp::SaveXmm128),
            10 => Some(UwopOp::SaveXmm128Far),
            11 => Some(UwopOp::PushMachframe),
            _ => None,
        }
    }

    /// op 中文名（诊断文本用）。
    pub fn name(self) -> &'static str {
        match self {
            UwopOp::PushNonvol => "非易失压栈",
            UwopOp::AllocSmall => "小栈分配",
            UwopOp::AllocLarge => "大栈分配",
            UwopOp::SetFpreg => "帧指针设定",
            UwopOp::SaveNonvol => "非易失保存",
            UwopOp::Epilogue => "尾声标记",
            UwopOp::SaveXmm128 => "xmm128保存",
            UwopOp::SaveXmm128Far => "xmm128远保存",
            UwopOp::PushMachframe => "机器帧压栈",
        }
    }

    /// op 在闭集序中的位号（0..9，走查覆盖位图用）。
    pub fn rank(self) -> u8 {
        match self {
            UwopOp::PushNonvol => 0,
            UwopOp::AllocSmall => 1,
            UwopOp::AllocLarge => 2,
            UwopOp::SetFpreg => 3,
            UwopOp::SaveNonvol => 4,
            UwopOp::Epilogue => 5,
            UwopOp::SaveXmm128 => 6,
            UwopOp::SaveXmm128Far => 7,
            UwopOp::PushMachframe => 8,
        }
    }

    /// 该 op 的合法位型规格（冻结，来源：按 winnt.h 展开码族语义自定边界）。
    pub fn spec(self) -> OpSpec {
        match self {
            // 压栈族：槽位即寄存器号 0..15，无偏移域、无追随槽。
            UwopOp::PushNonvol => OpSpec { slot_mask: 0xF, off_step: 0, off_max: 0, extra_slots: 0 },
            // 小栈分配：槽位即 (size/8)-1，0..15，无偏移域。
            UwopOp::AllocSmall => OpSpec { slot_mask: 0xF, off_step: 0, off_max: 0, extra_slots: 0 },
            // 大栈分配：槽位为扩展标志 0/1；尺寸建模进偏移域（8 栅格，≤240）。
            UwopOp::AllocLarge => OpSpec { slot_mask: 0x1, off_step: 8, off_max: 240, extra_slots: 0 },
            // 帧指针设定：槽位即寄存器号；帧偏移 16 栅格，≤240。
            UwopOp::SetFpreg => OpSpec { slot_mask: 0xF, off_step: 16, off_max: 240, extra_slots: 0 },
            // 非易失保存：槽位即寄存器号；槽偏移 8 栅格，≤504；1 追随槽。
            UwopOp::SaveNonvol => OpSpec { slot_mask: 0xF, off_step: 8, off_max: 504, extra_slots: 1 },
            // 尾声标记：槽位为标志组 0..7；尾声尺寸 16 栅格，≤112。
            UwopOp::Epilogue => OpSpec { slot_mask: 0x7, off_step: 16, off_max: 112, extra_slots: 0 },
            // xmm128 就地保存：槽位即 xmm 号；偏移 16 栅格，≤240；1 追随槽。
            UwopOp::SaveXmm128 => OpSpec { slot_mask: 0xF, off_step: 16, off_max: 240, extra_slots: 1 },
            // xmm128 远保存：槽位即 xmm 号；偏移 16 栅格，≤2032；2 追随槽。
            UwopOp::SaveXmm128Far => OpSpec { slot_mask: 0xF, off_step: 16, off_max: 2032, extra_slots: 2 },
            // 机器帧压栈：槽位为标志 0/1，无偏移域。
            UwopOp::PushMachframe => OpSpec { slot_mask: 0x1, off_step: 0, off_max: 0, extra_slots: 0 },
        }
    }
}

/// UWOP 操作码闭集数组（序即语料轮转序与走查覆盖位号序）。
pub const OP_ALL: &[UwopOp] = &[
    UwopOp::PushNonvol,
    UwopOp::AllocSmall,
    UwopOp::AllocLarge,
    UwopOp::SetFpreg,
    UwopOp::SaveNonvol,
    UwopOp::Epilogue,
    UwopOp::SaveXmm128,
    UwopOp::SaveXmm128Far,
    UwopOp::PushMachframe,
];

/// 单 op 合法位型规格：槽位掩码 + 偏移栅格 + 追随槽数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpSpec {
    /// 槽位半字节合法位型掩码（槽位与掩码取反按位与为零才算合法位型）。
    pub slot_mask: u32,
    /// 偏移步长栅格（0 = 该 op 无偏移域，偏移必须为 0）。
    pub off_step: u32,
    /// 偏移上界。
    pub off_max: u32,
    /// 声明所需追随槽数（AllocLarge 另由槽位标志推 1/2）。
    pub extra_slots: u32,
}

// ── 语料库（冻结闭集 32 行，行可复算） ──────────────────────────
/// 语料库行容量（冻结闭集行数下限 32）。
pub const CORPUS_CAP: usize = 32;

/// 语料行四元组：op / 槽位 / 偏移 / 合法位型（packed 打包字）。
///
/// packed 位型约定：低半字节 = 操作码，第 4..12 位 = 槽位域（8 位视窗）。
/// 合法位型 = 低半字节与 op_code 自洽 且 槽位落在该 op 的 slot_mask 内
/// 且 偏移落在 off_step 栅格上、不越 off_max。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorpusRow {
    /// 行号（0..32，语料库登记位）。
    pub idx: u32,
    /// 操作码值（冻结闭集内）。
    pub op_code: u32,
    /// 槽位（受 op 的 slot_mask 约束）。
    pub slot: u32,
    /// 偏移（受 op 的 off_step 栅格与 off_max 约束）。
    pub off: u32,
    /// 打包位型：op_code | (slot << 4)。
    pub packed: u32,
}

/// 语料行纯函数复算（确定性公式，第 i 行冻结可复算）。
///
/// 公式（整数、无随机）：
/// - op = OP_ALL[i % 9]（九 op 轮转，32 行覆盖全部九族）；
/// - slot = (7i + 3) % (slot_max + 1)，slot_max 由 slot_mask 推；
/// - off = off_step × ((5i + 2) % (off_max/step + 1))，无偏移域 op 恒 0。
pub fn corpus_row(i: u32) -> CorpusRow {
    let op = match OP_ALL.get((i % OP_ALL.len() as u32) as usize) {
        Some(o) => *o,
        None => UwopOp::PushNonvol,
    };
    let spec = op.spec();
    let slot_bound = match spec.slot_mask {
        0x1 => 2u32,
        0x7 => 8u32,
        _ => 16u32,
    };
    let slot = i.wrapping_mul(7).wrapping_add(3) % slot_bound;
    let off = if spec.off_step == 0 {
        0u32
    } else {
        let cells = spec.off_max / spec.off_step + 1;
        spec.off_step.wrapping_mul(i.wrapping_mul(5).wrapping_add(2) % cells)
    };
    CorpusRow {
        idx: i,
        op_code: op.code(),
        slot,
        off,
        packed: op.code() | (slot << 4),
    }
}

/// 语料库冻结闭集表（32 行，逐行等于 corpus_row(i) 复算结果）。
pub fn corpus_table() -> Vec<CorpusRow> {
    let mut out: Vec<CorpusRow> = Vec::new();
    let mut i = 0u32;
    while i < CORPUS_CAP as u32 {
        out.push(corpus_row(i));
        i += 1;
    }
    out
}

// ── 语料状态机 Raw→Validated→Sealed（单调） ─────────────────────
/// 语料状态机：Raw（未判）→ Validated（判定通过）→ Sealed（封账），只进不退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusState {
    /// Raw：语料行未判定。
    Raw,
    /// Validated：判定通过入账。
    Validated,
    /// Sealed：封账，只读。
    Sealed,
}

impl CorpusState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            CorpusState::Raw => 0,
            CorpusState::Validated => 1,
            CorpusState::Sealed => 2,
        }
    }
}

/// 已判入账的压测行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PressRow {
    /// 语料行号。
    pub idx: u32,
    /// 操作码值。
    pub op_code: u32,
    /// 槽位。
    pub slot: u32,
    /// 偏移。
    pub off: u32,
    /// 行状态（入账即 Validated，封账后置 Sealed）。
    pub state: CorpusState,
}

/// 语料压测账本：判定器承载面 + 单调状态机 + 容量闸。
#[derive(Clone, Debug)]
pub struct PressLedger {
    rows: Vec<PressRow>,
    state: CorpusState,
    cap: usize,
}

impl PressLedger {
    /// 开满容账本（容量 = 语料库 32 行）。
    pub fn new() -> PressLedger {
        PressLedger {
            rows: Vec::new(),
            state: CorpusState::Raw,
            cap: CORPUS_CAP,
        }
    }

    /// 开小容账本（压测容量闸用；上界饱和到语料库容量）。
    pub fn new_capped(cap: usize) -> PressLedger {
        let c = if cap > CORPUS_CAP { CORPUS_CAP } else { cap };
        PressLedger {
            rows: Vec::new(),
            state: CorpusState::Raw,
            cap: c,
        }
    }

    /// 当前账本状态。
    pub fn state(&self) -> CorpusState {
        self.state
    }

    /// 已入账行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 按行号查行状态。
    pub fn row_state(&self, idx: u32) -> Option<CorpusState> {
        let mut k = 0usize;
        while k < self.rows.len() {
            if let Some(r) = self.rows.get(k) {
                if r.idx == idx {
                    return Some(r.state);
                }
            }
            k += 1;
        }
        None
    }

    /// 按存储位取行（越界 None）。
    pub fn row_at(&self, pos: usize) -> Option<PressRow> {
        self.rows.get(pos).copied()
    }

    /// 压测判定器：语料行 → 入账压测行。
    ///
    /// 判定次序冻结（先到先判）：
    /// 1. 未登记语料行 ENOENT（操作码越冻结闭集 / 行号越语料库）；
    /// 2. 位型越闭集 EINVAL（打包不自洽 / 槽位越掩码 / 偏移失步 / 偏移越上界）；
    /// 3. 状态倒退 EREWIND（封账账本再判即倒退）；
    /// 4. 容量耗尽 ECapFull（新行占槽时账满）。
    pub fn judge_uwop_corpus(&mut self, row: &CorpusRow) -> Result<PressRow, u32> {
        // 闸一：未登记语料行。
        let op = match UwopOp::from_code(row.op_code) {
            Some(o) => o,
            None => return Err(UwDiag::ENoEnt.val()),
        };
        if row.idx >= CORPUS_CAP as u32 {
            return Err(UwDiag::ENoEnt.val());
        }
        // 闸二：位型越闭集。
        let spec = op.spec();
        let pop = row.packed & 0xF;
        let pslot = (row.packed >> 4) & 0xFF;
        if pop != row.op_code {
            return Err(UwDiag::EBits.val());
        }
        if (pslot & !spec.slot_mask) != 0 {
            return Err(UwDiag::EBits.val());
        }
        if spec.off_step == 0 {
            if row.off != 0 {
                return Err(UwDiag::EBits.val());
            }
        } else {
            if (row.off % spec.off_step) != 0 || row.off > spec.off_max {
                return Err(UwDiag::EBits.val());
            }
        }
        // 闸三：状态倒退（封账账本再判定 = 倒退）。
        if self.state.rank() >= CorpusState::Sealed.rank() {
            return Err(UwDiag::ERewind.val());
        }
        // 闸四：容量耗尽（仅新行占槽时查）。
        let mut pos: Option<usize> = None;
        let mut k = 0usize;
        while k < self.rows.len() {
            if let Some(r) = self.rows.get(k) {
                if r.idx == row.idx {
                    pos = Some(k);
                    break;
                }
            }
            k += 1;
        }
        if pos.is_none() && self.rows.len() >= self.cap {
            return Err(UwDiag::ECapFull.val());
        }
        // 入账：行状态 Raw→Validated，账本状态只进不退。
        let press = PressRow {
            idx: row.idx,
            op_code: row.op_code,
            slot: pslot,
            off: row.off,
            state: CorpusState::Validated,
        };
        match pos {
            Some(p) => {
                if let Some(r) = self.rows.get_mut(p) {
                    *r = press;
                } else {
                    self.rows.push(press);
                }
            }
            None => self.rows.push(press),
        }
        if self.state.rank() < CorpusState::Validated.rank() {
            self.state = CorpusState::Validated;
        }
        Ok(press)
    }

    /// 封账：语料 32 行判齐才许封；封后只读，重封即倒退。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= CorpusState::Sealed.rank() {
            return Err(UwDiag::ERewind.val());
        }
        if self.rows.len() != CORPUS_CAP {
            return Err(UwDiag::ESeal.val());
        }
        let mut k = 0usize;
        while k < self.rows.len() {
            if let Some(r) = self.rows.get(k) {
                if r.state != CorpusState::Validated {
                    return Err(UwDiag::ESeal.val());
                }
            }
            k += 1;
        }
        let mut k2 = 0usize;
        while k2 < self.rows.len() {
            if let Some(r) = self.rows.get_mut(k2) {
                r.state = CorpusState::Sealed;
            }
            k2 += 1;
        }
        self.state = CorpusState::Sealed;
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 派生，零随机红线合规） ───────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性；常数取 Numerical Recipes 32 位推荐值）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        (self.state >> 16) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 展开码表与走查器（坏表拒止承载面） ───────────────────────────
/// 表走查终点标记。
pub const TERM: u32 = 0xFFFF_FFFF;

/// 表走查步数预算（超预算视为疑似无限环，fail-closed 剪断）。
pub const WALK_BUDGET: usize = 64;

/// 展开码表节点：打包位型 + 单调后继 + 追随槽载荷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableNode {
    /// 打包位型：低半字节 op，第 4..12 位槽位域。
    pub packed: u32,
    /// 单调后继节点号（严格递增链；TERM 表终点）。
    pub next: u32,
    /// 追随槽载荷 0。
    pub ex0: u32,
    /// 追随槽载荷 1。
    pub ex1: u32,
    /// 实载追随槽数（须 ≥ 该 op 声明所需，否则判截断）。
    pub extra_n: u32,
}

/// 展开码表（节点向量 + 单调后继链）。
#[derive(Clone, Debug)]
pub struct UnwindTable {
    /// 节点集（按节点号存取，走查经 .get() 越界即拒）。
    pub nodes: Vec<TableNode>,
}

/// 表走查报告：步数 + 覆盖 op 位图（bit r = 第 r 号 op 命中）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkReport {
    /// 走查步数（含终点节点）。
    pub steps: usize,
    /// op 覆盖位图（按 UwopOp::rank 置位）。
    pub seen: u32,
}

/// 坏表走查判定器（fail-closed：任何异常必回错误码，绝不 Ok）。
///
/// 走查次序（先到先判）：越界表 ERange → 未知操作码 EOpBad → 位型越闭集
/// EBits → 截断表 ETrunc → 越界表 ERange（后继）→ 环表 ERing → 预算 EBudget。
pub fn walk_table(tbl: &UnwindTable) -> Result<WalkReport, u32> {
    let mut cur = 0usize;
    let mut steps = 0usize;
    let mut seen = 0u32;
    loop {
        let node = match tbl.nodes.get(cur) {
            Some(n) => *n,
            None => return Err(UwDiag::ERange.val()),
        };
        let opn = node.packed & 0xF;
        let op = match UwopOp::from_code(opn) {
            Some(o) => o,
            None => return Err(UwDiag::EOpBad.val()),
        };
        let spec = op.spec();
        let slot = (node.packed >> 4) & 0xFF;
        if (slot & !spec.slot_mask) != 0 {
            return Err(UwDiag::EBits.val());
        }
        // 追随槽需求：AllocLarge 由槽位标志推 1/2，其余按规格声明。
        let need = if op == UwopOp::AllocLarge {
            slot + 1
        } else {
            spec.extra_slots
        };
        if node.extra_n < need {
            return Err(UwDiag::ETrunc.val());
        }
        seen |= 1u32 << op.rank();
        if node.next == TERM {
            steps += 1;
            return Ok(WalkReport { steps, seen });
        }
        if node.next as usize >= tbl.nodes.len() {
            return Err(UwDiag::ERange.val());
        }
        let nxt = node.next as usize;
        if nxt <= cur {
            return Err(UwDiag::ERing.val());
        }
        cur = nxt;
        steps += 1;
        if steps > WALK_BUDGET {
            return Err(UwDiag::EBudget.val());
        }
    }
}

/// 好表构造：由语料 32 行派生（打包位型合法、载荷齐、链严格递增、链尾 TERM）。
pub fn build_good_table() -> UnwindTable {
    let mut nodes: Vec<TableNode> = Vec::new();
    let mut i = 0u32;
    while i < CORPUS_CAP as u32 {
        let row = corpus_row(i);
        let op = match UwopOp::from_code(row.op_code) {
            Some(o) => o,
            None => UwopOp::PushNonvol,
        };
        let spec = op.spec();
        let need = if op == UwopOp::AllocLarge {
            row.slot + 1
        } else {
            spec.extra_slots
        };
        let next = if i + 1 >= CORPUS_CAP as u32 {
            TERM
        } else {
            i + 1
        };
        nodes.push(TableNode {
            packed: row.packed,
            next,
            ex0: row.off,
            ex1: row.slot,
            extra_n: need,
        });
        i += 1;
    }
    UnwindTable { nodes }
}

// ── 坏表拒止注入器三类（各 10 轮） ──────────────────────────────
/// 截断表注入：链中第 p 节点用 SAVE_XMM128_FAR（需 2 追随槽），
/// 实载 extra_n 只给 0 或 1 → 走查必回截断错误码。
pub fn make_trunc_table(round: u32) -> UnwindTable {
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(round as u64).wrapping_add(1));
    let k = (3 + rng.below(4)) as usize;
    let p = rng.below(k as u32) as usize;
    let load = rng.below(2);
    let mut nodes: Vec<TableNode> = Vec::new();
    let mut i = 0u32;
    while i < k as u32 {
        let next = if i + 1 >= k as u32 {
            TERM
        } else {
            i + 1
        };
        if i as usize == p {
            nodes.push(TableNode {
                packed: UwopOp::SaveXmm128Far.code() | (5u32 << 4),
                next,
                ex0: 16,
                ex1: 32,
                extra_n: load,
            });
        } else {
            nodes.push(TableNode {
                packed: UwopOp::PushNonvol.code() | (3u32 << 4),
                next,
                ex0: 0,
                ex1: 0,
                extra_n: 0,
            });
        }
        i += 1;
    }
    UnwindTable { nodes }
}

/// 环表注入：节点 p 的后继回指自身（自环）或前一节点（回边），
/// 打破单调链 → 走查必回环表错误码。
pub fn make_ring_table(round: u32) -> UnwindTable {
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(0x1000).wrapping_add(round as u64));
    let k = (4 + rng.below(3)) as usize;
    let p = (1 + rng.below((k - 1) as u32)) as usize;
    let self_loop = rng.below(2) == 0;
    let mut nodes: Vec<TableNode> = Vec::new();
    let mut i = 0u32;
    while i < k as u32 {
        let next = if i as usize == p {
            if self_loop {
                i
            } else {
                i - 1
            }
        } else if i + 1 >= k as u32 {
            TERM
        } else {
            i + 1
        };
        nodes.push(TableNode {
            packed: UwopOp::AllocSmall.code() | (9u32 << 4),
            next,
            ex0: 0,
            ex1: 0,
            extra_n: 0,
        });
        i += 1;
    }
    UnwindTable { nodes }
}

/// 越界表注入：节点 p 的后继指向表长之外 → 走查必回越界错误码。
pub fn make_range_table(round: u32) -> UnwindTable {
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(0x2000).wrapping_add(round as u64));
    let k = (3 + rng.below(4)) as usize;
    let p = rng.below(k as u32) as usize;
    let overshoot = 1 + rng.below(5);
    let mut nodes: Vec<TableNode> = Vec::new();
    let mut i = 0u32;
    while i < k as u32 {
        let next = if i as usize == p {
            (k as u32).wrapping_add(overshoot)
        } else if i + 1 >= k as u32 {
            TERM
        } else {
            i + 1
        };
        nodes.push(TableNode {
            packed: UwopOp::PushNonvol.code() | (2u32 << 4),
            next,
            ex0: 0,
            ex1: 0,
            extra_n: 0,
        });
        i += 1;
    }
    UnwindTable { nodes }
}

/// 坏表注入压测汇总（三类各 10 轮）。
#[derive(Clone, Debug)]
pub struct PressSweep {
    /// 总轮数（恒 30）。
    pub total: usize,
    /// 全抓轮数（错误码与注入类一一命中）。
    pub caught: usize,
    /// 截断类命中轮数（目标 10）。
    pub trunc_caught: usize,
    /// 环类命中轮数（目标 10）。
    pub ring_caught: usize,
    /// 越界类命中轮数（目标 10）。
    pub range_caught: usize,
    /// Ok 泄漏轮数（坏表判 Ok 即 fail-closed 失效，目标 0）。
    pub ok_leak: usize,
    /// 错误码漂移轮数（回了错但不是本类预期码，目标 0）。
    pub misc_err: usize,
    /// 首个逃脱/漂移轮描述（空 = 无逃脱）。
    pub escape: String,
}

/// 坏表拒止压测：三类注入器各 10 轮，逐轮走查，全数须按类命中。
pub fn bad_table_sweep() -> PressSweep {
    let mut sw = PressSweep {
        total: 0,
        caught: 0,
        trunc_caught: 0,
        ring_caught: 0,
        range_caught: 0,
        ok_leak: 0,
        misc_err: 0,
        escape: String::new(),
    };
    // 截断表 10 轮。
    let mut r = 0u32;
    while r < 10 {
        let got = walk_table(&make_trunc_table(r));
        sw.total += 1;
        match got {
            Err(e) if e == UwDiag::ETrunc.val() => {
                sw.trunc_caught += 1;
                sw.caught += 1;
            }
            Err(_) => {
                sw.misc_err += 1;
                if sw.escape.is_empty() {
                    sw.escape = format!("截断轮{} 错码漂移", r);
                }
            }
            Ok(_) => {
                sw.ok_leak += 1;
                if sw.escape.is_empty() {
                    sw.escape = format!("截断轮{} Ok 泄漏", r);
                }
            }
        }
        r += 1;
    }
    // 环表 10 轮。
    let mut r2 = 0u32;
    while r2 < 10 {
        let got = walk_table(&make_ring_table(r2));
        sw.total += 1;
        match got {
            Err(e) if e == UwDiag::ERing.val() => {
                sw.ring_caught += 1;
                sw.caught += 1;
            }
            Err(_) => {
                sw.misc_err += 1;
                if sw.escape.is_empty() {
                    sw.escape = format!("环轮{} 错码漂移", r2);
                }
            }
            Ok(_) => {
                sw.ok_leak += 1;
                if sw.escape.is_empty() {
                    sw.escape = format!("环轮{} Ok 泄漏", r2);
                }
            }
        }
        r2 += 1;
    }
    // 越界表 10 轮。
    let mut r3 = 0u32;
    while r3 < 10 {
        let got = walk_table(&make_range_table(r3));
        sw.total += 1;
        match got {
            Err(e) if e == UwDiag::ERange.val() => {
                sw.range_caught += 1;
                sw.caught += 1;
            }
            Err(_) => {
                sw.misc_err += 1;
                if sw.escape.is_empty() {
                    sw.escape = format!("越界轮{} 错码漂移", r3);
                }
            }
            Ok(_) => {
                sw.ok_leak += 1;
                if sw.escape.is_empty() {
                    sw.escape = format!("越界轮{} Ok 泄漏", r3);
                }
            }
        }
        r3 += 1;
    }
    sw
}

// ── 双跑一致指纹（FNV 混入全程） ────────────────────────────────
/// 全流程指纹：语料复算 + 32 行判定 + 封账 + 坏表压测 + 好表走查。
/// 确定性混入（FNV 素数 0x100000001B3），两次独立全流程逐位一致。
pub fn press_fingerprint() -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    // 语料 32 行复算混入。
    let mut i = 0u32;
    while i < CORPUS_CAP as u32 {
        let r = corpus_row(i);
        h = h
            .wrapping_mul(0x100_0000_01B3)
            .wrapping_add(r.packed as u64);
        h = h
            .wrapping_mul(0x100_0000_01B3)
            .wrapping_add(r.off as u64);
        i += 1;
    }
    // 判定全流程混入（独立账本）。
    let mut led = PressLedger::new();
    let mut j = 0u32;
    while j < CORPUS_CAP as u32 {
        let row = corpus_row(j);
        let mix = match led.judge_uwop_corpus(&row) {
            Ok(p) => (p.state.rank() as u64).wrapping_add(p.slot as u64),
            Err(e) => e as u64,
        };
        h = h.wrapping_mul(0x100_0000_01B3).wrapping_add(mix);
        j += 1;
    }
    match led.seal() {
        Ok(()) => {
            h = h.wrapping_add(0x5EED);
        }
        Err(e) => {
            h = h.wrapping_add(e as u64);
        }
    }
    // 坏表压测计数混入。
    let sw = bad_table_sweep();
    h = h
        .wrapping_mul(0x100_0000_01B3)
        .wrapping_add(sw.caught as u64);
    h = h
        .wrapping_mul(0x100_0000_01B3)
        .wrapping_add(sw.ok_leak as u64);
    h = h
        .wrapping_mul(0x100_0000_01B3)
        .wrapping_add(sw.misc_err as u64);
    // 好表走查混入。
    let walk = walk_table(&build_good_table());
    h = h.wrapping_add(match walk {
        Ok(rep) => (rep.steps as u64) | ((rep.seen as u64) << 8),
        Err(e) => e as u64,
    });
    h
}

/// 双跑一致判定：两次全流程指纹逐位一致且非零。
pub fn dual_run_verdict() -> Result<u64, u32> {
    let a = press_fingerprint();
    let b = press_fingerprint();
    if a == b && a != 0 {
        Ok(a)
    } else {
        Err(UwDiag::EFp.val())
    }
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 [0x80000, 0x831C0] 窗口（卡给定窗口）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落窗口、互异、查表可命中、未知码回落。
fn diag_closed_ok() -> bool {
    if UW_ALL.len() != 11 {
        return false;
    }
    let mut i = 0usize;
    while i < UW_ALL.len() {
        let vi = match UW_ALL.get(i) {
            Some(d) => d.val(),
            None => return false,
        };
        if vi < SEG_BASE || vi >= SEG_BASE + 0x40 {
            return false;
        }
        let mut j = i + 1;
        while j < UW_ALL.len() {
            let vj = match UW_ALL.get(j) {
                Some(d) => d.val(),
                None => return false,
            };
            if vi == vj {
                return false;
            }
            j += 1;
        }
        if detail(vi).is_empty() {
            return false;
        }
        i += 1;
    }
    // 已知码必须命中文案，未知码必须回落。
    if detail(UwDiag::ENoEnt.val()) != UwDiag::ENoEnt.detail() {
        return false;
    }
    if detail(SEG_BASE + 0x3F) == UwDiag::ENoEnt.detail() {
        return false;
    }
    true
}

/// 操作码冻结闭集守卫：9 值互异、全在半字节域、from_code 往返恒等、
/// 闭集外码（1/7/8）显性拒。
fn opcode_closed_ok() -> bool {
    if OP_ALL.len() != 9 {
        return false;
    }
    let mut i = 0usize;
    while i < OP_ALL.len() {
        let a = match OP_ALL.get(i) {
            Some(o) => o.code(),
            None => return false,
        };
        if a > 0xF {
            return false;
        }
        let mut j = i + 1;
        while j < OP_ALL.len() {
            let b = match OP_ALL.get(j) {
                Some(o) => o.code(),
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        match UwopOp::from_code(a) {
            Some(o) => {
                if o.code() != a {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    if UwopOp::from_code(1).is_some() {
        return false;
    }
    if UwopOp::from_code(7).is_some() {
        return false;
    }
    if UwopOp::from_code(8).is_some() {
        return false;
    }
    true
}

/// 语料库守卫：32 行齐、逐行可复算恒等、行操作码落闭集、打包位型自洽。
fn corpus_frozen_ok() -> bool {
    let t = corpus_table();
    if t.len() != CORPUS_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS_CAP {
        match t.get(i) {
            Some(r) => {
                // 行可复算：表内行与纯函数复算恒等。
                if *r != corpus_row(i as u32) {
                    return false;
                }
                // 行操作码必须落在冻结闭集内且打包自洽。
                if UwopOp::from_code(r.op_code).is_none() {
                    return false;
                }
                if (r.packed & 0xF) != r.op_code {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    // 九 op 全覆盖（32 行轮转须命中全部九族）。
    let mut cover = 0u32;
    let mut k = 0usize;
    while k < t.len() {
        if let Some(r) = t.get(k) {
            if let Some(op) = UwopOp::from_code(r.op_code) {
                cover |= 1u32 << op.rank();
            }
        }
        k += 1;
    }
    cover.count_ones() == 9
}

/// 正常流：32 行逐行判定入账 Raw→Validated，判齐封账 Sealed，行状态随封推进。
fn happy_path_ok() -> bool {
    let mut led = PressLedger::new();
    if led.state() != CorpusState::Raw {
        return false;
    }
    let mut i = 0u32;
    while i < CORPUS_CAP as u32 {
        let row = corpus_row(i);
        match led.judge_uwop_corpus(&row) {
            Ok(p) => {
                if p.state != CorpusState::Validated || p.idx != i {
                    return false;
                }
            }
            Err(_) => return false,
        }
        i += 1;
    }
    if led.len() != CORPUS_CAP || led.state() != CorpusState::Validated {
        return false;
    }
    match led.seal() {
        Ok(()) => {}
        Err(_) => return false,
    }
    led.state() == CorpusState::Sealed && led.row_state(0) == Some(CorpusState::Sealed)
}

/// 位型越闭集拒止：槽位越掩码 / 打包不自洽 / 偏移失步 / 偏移越上界 /
/// 零步 op 带偏移，五路全数 EINVAL；对照合法行须通过。
fn einval_gates_ok() -> bool {
    let mut led = PressLedger::new();
    // (a) 槽位越合法掩码：AllocLarge（掩码 0x1）行槽位置 3。
    let r2 = corpus_row(2);
    let bad_a = CorpusRow {
        idx: r2.idx,
        op_code: r2.op_code,
        slot: 3,
        off: r2.off,
        packed: (r2.op_code & 0xF) | (3 << 4),
    };
    if led.judge_uwop_corpus(&bad_a) != Err(UwDiag::EBits.val()) {
        return false;
    }
    // (b) 打包位型与操作码域不自洽：op_code=0 而 packed 低半字节=1。
    let r0 = corpus_row(0);
    let bad_b = CorpusRow {
        packed: 1 | (r0.slot << 4),
        ..r0
    };
    if led.judge_uwop_corpus(&bad_b) != Err(UwDiag::EBits.val()) {
        return false;
    }
    // (c) 偏移失步：SetFpreg（16 栅格）行偏移 +1。
    let r3 = corpus_row(3);
    let bad_c = CorpusRow {
        off: r3.off.wrapping_add(1),
        ..r3
    };
    if led.judge_uwop_corpus(&bad_c) != Err(UwDiag::EBits.val()) {
        return false;
    }
    // (d) 偏移越上界：SetFpreg 行偏移置 999。
    let bad_d = CorpusRow {
        off: 999,
        ..r3
    };
    if led.judge_uwop_corpus(&bad_d) != Err(UwDiag::EBits.val()) {
        return false;
    }
    // (e) 零步 op 带偏移：PushNonvol 行偏移置 8。
    let bad_e = CorpusRow {
        off: 8,
        ..r0
    };
    if led.judge_uwop_corpus(&bad_e) != Err(UwDiag::EBits.val()) {
        return false;
    }
    // 对照：位型合法行须通过（同账本判 PushNonvol 语料行）。
    led.judge_uwop_corpus(&corpus_row(0)).is_ok()
}

/// 未登记语料行拒止：闭集外操作码（1/7/8）与越库行号（32 / u32::MAX）
/// 全数 ENOENT。
fn enoent_gates_ok() -> bool {
    let mut led = PressLedger::new();
    // (a) 操作码 1/7/8 不在冻结闭集。
    let rogue = [1u32, 7, 8];
    let mut k = 0usize;
    while k < rogue.len() {
        let code = match rogue.get(k) {
            Some(c) => *c,
            None => return false,
        };
        let row = CorpusRow {
            idx: 0,
            op_code: code,
            slot: 5,
            off: 0,
            packed: code | (5 << 4),
        };
        if led.judge_uwop_corpus(&row) != Err(UwDiag::ENoEnt.val()) {
            return false;
        }
        k += 1;
    }
    // (b) 行号越语料库（内容照抄语料第 0 行，仅行号越界）。
    let r0 = corpus_row(0);
    let far = CorpusRow {
        idx: CORPUS_CAP as u32,
        ..r0
    };
    if led.judge_uwop_corpus(&far) != Err(UwDiag::ENoEnt.val()) {
        return false;
    }
    let far2 = CorpusRow {
        idx: u32::MAX,
        ..r0
    };
    led.judge_uwop_corpus(&far2) == Err(UwDiag::ENoEnt.val())
}

/// 状态倒退拒止：未齐封账 ESeal、判齐封账后重封 EREWIND、封账后判定 EREWIND。
fn rewind_gates_ok() -> bool {
    let mut led = PressLedger::new();
    // 未齐封账。
    if led.seal() != Err(UwDiag::ESeal.val()) {
        return false;
    }
    // 判 1 行后再封仍 ESeal。
    if led.judge_uwop_corpus(&corpus_row(0)).is_err() {
        return false;
    }
    if led.seal() != Err(UwDiag::ESeal.val()) {
        return false;
    }
    // 判齐其余行后封账成功。
    let mut i = 1u32;
    while i < CORPUS_CAP as u32 {
        if led.judge_uwop_corpus(&corpus_row(i)).is_err() {
            return false;
        }
        i += 1;
    }
    if led.seal().is_err() {
        return false;
    }
    // 封账后重封 = 倒退；封账后判定 = 倒退。
    if led.seal() != Err(UwDiag::ERewind.val()) {
        return false;
    }
    led.judge_uwop_corpus(&corpus_row(9)) == Err(UwDiag::ERewind.val())
}

/// 判定次序冻结：ENOENT → EINVAL → EREWIND → ECapFull，先到先判。
fn judge_order_ok() -> bool {
    // ① ENOENT 先于 EINVAL：未知 op + 越掩码槽位 → ENoEnt。
    let mut led = PressLedger::new();
    let bad_op = CorpusRow {
        idx: 0,
        op_code: 1,
        slot: 20,
        off: 0,
        packed: 1 | (20 << 4),
    };
    if led.judge_uwop_corpus(&bad_op) != Err(UwDiag::ENoEnt.val()) {
        return false;
    }
    // ② ENOENT 先于容量：零容账本 + 未知 op 行 → ENoEnt（而非 ECapFull）。
    let mut zero = PressLedger::new_capped(0);
    if zero.judge_uwop_corpus(&bad_op) != Err(UwDiag::ENoEnt.val()) {
        return false;
    }
    // 备好满账封账账本。
    let mut full = PressLedger::new();
    let mut i = 0u32;
    while i < CORPUS_CAP as u32 {
        match full.judge_uwop_corpus(&corpus_row(i)) {
            Ok(_) => {}
            Err(_) => return false,
        }
        i += 1;
    }
    if full.seal().is_err() {
        return false;
    }
    // ③ EINVAL 先于 EREWIND：封账账本 + 位型越掩码行 → EBits。
    let r2 = corpus_row(2);
    let bad_bits = CorpusRow {
        idx: r2.idx,
        op_code: r2.op_code,
        slot: 3,
        off: r2.off,
        packed: (r2.op_code & 0xF) | (3 << 4),
    };
    if full.judge_uwop_corpus(&bad_bits) != Err(UwDiag::EBits.val()) {
        return false;
    }
    // ④ EREWIND 收尾：封账账本 + 合法行 → ERewind。
    full.judge_uwop_corpus(&corpus_row(5)) == Err(UwDiag::ERewind.val())
}

/// 容量耗尽边界：小容账本判满后新行拒入、已登记行幂等重判不占新槽、
/// 零容账本首行即拒、满容账本判齐后重判账面不涨。
fn capacity_ok() -> bool {
    let mut led = PressLedger::new_capped(8);
    let mut i = 0u32;
    while i < 8 {
        if led.judge_uwop_corpus(&corpus_row(i)).is_err() {
            return false;
        }
        i += 1;
    }
    // 第 9 行 → 容量耗尽。
    if led.judge_uwop_corpus(&corpus_row(8)) != Err(UwDiag::ECapFull.val()) {
        return false;
    }
    // 已登记行幂等重判仍 Ok，不占新槽。
    if led.judge_uwop_corpus(&corpus_row(3)).is_err() {
        return false;
    }
    if led.len() != 8 {
        return false;
    }
    // 零容账本：第一行即容量耗尽。
    let mut zero = PressLedger::new_capped(0);
    if zero.judge_uwop_corpus(&corpus_row(0)) != Err(UwDiag::ECapFull.val()) {
        return false;
    }
    // 满容账本判齐 32 行：重判已登记行仍 Ok，账面不涨。
    let mut full = PressLedger::new();
    let mut j = 0u32;
    while j < CORPUS_CAP as u32 {
        if full.judge_uwop_corpus(&corpus_row(j)).is_err() {
            return false;
        }
        j += 1;
    }
    if full.judge_uwop_corpus(&corpus_row(31)).is_err() {
        return false;
    }
    full.len() == CORPUS_CAP
}

/// 判据主口径：坏表拒止压测 20/20（截断 10 + 越界 10 主对全抓），
/// 环表 10 轮为三态补压、亦须全抓。
fn press20_ok() -> bool {
    let sw = bad_table_sweep();
    sw.trunc_caught == 10 && sw.range_caught == 10 && sw.ring_caught == 10
}

/// fail-closed 语义：30 轮无 Ok 泄漏、无错码漂移；空表、闭集外 op 节点、
/// 长链超预算三类边角同样必回错误码。
fn fail_closed_ok() -> bool {
    let sw = bad_table_sweep();
    if sw.ok_leak != 0 || sw.misc_err != 0 {
        return false;
    }
    if sw.caught != 30 || sw.total != 30 {
        return false;
    }
    // 空表：走查即越界拒。
    let empty = UnwindTable {
        nodes: Vec::new(),
    };
    if walk_table(&empty).is_ok() {
        return false;
    }
    // 闭集外 op 节点：走查即未知操作码拒。
    let mut badop = UnwindTable {
        nodes: Vec::new(),
    };
    badop.nodes.push(TableNode {
        packed: 7 | (1 << 4),
        next: TERM,
        ex0: 0,
        ex1: 0,
        extra_n: 0,
    });
    if walk_table(&badop) != Err(UwDiag::EOpBad.val()) {
        return false;
    }
    // 长链超预算：100 节点单调链超步数预算 → EBudget（不死循环）。
    let mut long = UnwindTable {
        nodes: Vec::new(),
    };
    let mut i = 0u32;
    while i < 100 {
        let next = if i + 1 >= 100 {
            TERM
        } else {
            i + 1
        };
        long.nodes.push(TableNode {
            packed: UwopOp::PushNonvol.code() | (1u32 << 4),
            next,
            ex0: 0,
            ex1: 0,
            extra_n: 0,
        });
        i += 1;
    }
    walk_table(&long) == Err(UwDiag::EBudget.val())
}

/// 好表对照走查：32 步走通、九 op 全覆盖位图。
fn good_walk_ok() -> bool {
    match walk_table(&build_good_table()) {
        Ok(rep) => rep.steps == CORPUS_CAP && rep.seen.count_ones() == 9,
        Err(_) => false,
    }
}

/// 双跑一致：两次全流程指纹逐位一致且非零。
fn dual_run_ok() -> bool {
    match dual_run_verdict() {
        Ok(fp) => fp != 0,
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12744 自检（判据 12744 卡：UNX-F12745-J1 坏表拒止压测 20/20）。
pub fn run_unx12744_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12744");

    // 探针账本：正常流统计数据（供明细文本引用，不跨谓词复用判定值）。
    let mut probe = PressLedger::new();
    let mut judged = 0usize;
    let mut pi = 0u32;
    while pi < CORPUS_CAP as u32 {
        if probe.judge_uwop_corpus(&corpus_row(pi)).is_ok() {
            judged += 1;
        }
        pi += 1;
    }

    s.add_dyn(
        "F12745-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12745-02 诊断码闭集查表",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x}",
            UW_ALL.len(),
            UW_ALL.first().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12745-03 UWOP 操作码冻结闭集",
        opcode_closed_ok(),
        &format!(
            "ops={} 首={} 尾={}",
            OP_ALL.len(),
            OP_ALL.first().map(|o| o.name()).unwrap_or("?"),
            OP_ALL.last().map(|o| o.name()).unwrap_or("?")
        ),
    );

    s.add_dyn(
        "F12745-04 语料库 32 行齐·行可复算",
        corpus_frozen_ok(),
        &format!(
            "rows={} 复算恒等={}",
            corpus_table().len(),
            corpus_frozen_ok()
        ),
    );

    s.add_dyn(
        "F12745-05 正常流 Raw→Validated→Sealed",
        happy_path_ok(),
        &format!("判入={}/{} probe_state={:?}", judged, CORPUS_CAP, probe.state()),
    );

    s.add_dyn(
        "F12745-06 位型越闭集拒止 EINVAL",
        einval_gates_ok(),
        "槽位越掩码/打包不自洽/偏移失步/偏移越上界/零步带偏移 五路全拒",
    );

    s.add_dyn(
        "F12745-07 未登记语料行拒止 ENOENT",
        enoent_gates_ok(),
        "闭集外 op(1/7/8) 与越库行号(32/u32::MAX) 全拒",
    );

    s.add_dyn(
        "F12745-08 状态倒退拒止 EREWIND",
        rewind_gates_ok(),
        "未齐封账 ESeal/重封倒退/封账后判定 三点全拒",
    );

    s.add_dyn(
        "F12745-09 判定次序 ENOENT→EINVAL→EREWIND→ECapFull",
        judge_order_ok(),
        "先到先判：未知op压过位型/位型压过倒退/倒退压过容量",
    );

    let sw = bad_table_sweep();
    s.add_dyn(
        "F12745-10 坏表拒止压测 20/20（截断/环/越界三态）",
        press20_ok(),
        &format!(
            "主对截断{}/10+越界{}/10=20/20 环补{}/10",
            sw.trunc_caught, sw.range_caught, sw.ring_caught
        ),
    );

    s.add_dyn(
        "F12745-11 fail-closed 语义（绝不 Ok）",
        fail_closed_ok(),
        &format!(
            "caught={}/{} leak={} 漂移={} 逃生={}",
            sw.caught,
            sw.total,
            sw.ok_leak,
            sw.misc_err,
            sw.escape
        ),
    );

    s.add_dyn(
        "F12745-12 容量耗尽边界",
        capacity_ok(),
        "小容判满拒新行/零容首行即拒/已登记行幂等重判不占槽",
    );

    s.add_dyn(
        "F12745-13 好表对照走查",
        good_walk_ok(),
        &format!(
            "步数={} 覆盖op={} 族数=9",
            match walk_table(&build_good_table()) {
                Ok(rep) => rep.steps,
                Err(_) => 0,
            },
            match walk_table(&build_good_table()) {
                Ok(rep) => rep.seen.count_ones(),
                Err(_) => 0,
            }
        ),
    );

    s.add_dyn(
        "F12745-14 双跑一致指纹",
        dual_run_ok(),
        "语料+判定+注入+好表 全程 FNV 指纹两次逐位一致",
    );

    // 收口：判据条数声明。
    let before = s.len();
    s.add_dyn(
        "F12745-15 收口·判据条数声明",
        before == 14,
        &format!(
            "判据实判 {} 条 + 收口 1 条 = {} 条（≥12 达标）",
            before,
            before + 1
        ),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 [0x80000, 0x831C0] 窗口。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x82400);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：11 个码互异、全部落窗口、查表可命中、未知码回落。
        assert_eq!(UW_ALL.len(), 11);
        let mut i = 0usize;
        while i < UW_ALL.len() {
            let vi = UW_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < UW_ALL.len() {
                let vj = UW_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn opcode_closed_set_frozen() {
        // 冻结闭集：9 值互异、全在半字节域、闭集外码显性拒。
        assert_eq!(OP_ALL.len(), 9);
        assert!(opcode_closed_ok());
        assert!(UwopOp::from_code(0).is_some());
        assert!(UwopOp::from_code(11).is_some());
        assert!(UwopOp::from_code(1).is_none());
        assert!(UwopOp::from_code(7).is_none());
        assert!(UwopOp::from_code(8).is_none());
        // 状态机位号严格递增（0..9）。
        assert!(UwopOp::PushNonvol.rank() < UwopOp::SaveXmm128Far.rank());
        assert!(UwopOp::SaveXmm128Far.rank() < UwopOp::PushMachframe.rank());
    }

    #[test]
    fn corpus_recomputable_and_frozen() {
        // 语料库 32 行：逐行与纯函数复算恒等；九 op 全覆盖。
        let t = corpus_table();
        assert_eq!(t.len(), CORPUS_CAP);
        let mut i = 0usize;
        while i < CORPUS_CAP {
            assert_eq!(t.get(i).copied(), Some(corpus_row(i as u32)));
            i += 1;
        }
        let mut cover = 0u32;
        let mut k = 0usize;
        while k < t.len() {
            if let Some(r) = t.get(k) {
                assert!(UwopOp::from_code(r.op_code).is_some());
                assert_eq!(r.packed & 0xF, r.op_code);
                if let Some(op) = UwopOp::from_code(r.op_code) {
                    cover |= 1u32 << op.rank();
                }
            }
            k += 1;
        }
        assert_eq!(cover.count_ones(), 9);
        // 状态机 Raw→Validated→Sealed 位号单调。
        assert!(CorpusState::Raw.rank() < CorpusState::Validated.rank());
        assert!(CorpusState::Validated.rank() < CorpusState::Sealed.rank());
    }

    #[test]
    fn happy_path_raw_validated_sealed() {
        // 正常流：32 行判入 → 判齐封账 → 行状态随封推进。
        let mut led = PressLedger::new();
        assert_eq!(led.state(), CorpusState::Raw);
        let mut i = 0u32;
        while i < CORPUS_CAP as u32 {
            let row = corpus_row(i);
            match led.judge_uwop_corpus(&row) {
                Ok(p) => {
                    assert_eq!(p.state, CorpusState::Validated);
                    assert_eq!(p.idx, i);
                }
                Err(e) => assert!(false, "正常行判定报错 0x{:x}", e),
            }
            i += 1;
        }
        assert_eq!(led.len(), CORPUS_CAP);
        assert_eq!(led.state(), CorpusState::Validated);
        assert!(led.seal().is_ok());
        assert_eq!(led.state(), CorpusState::Sealed);
        assert_eq!(led.row_state(0), Some(CorpusState::Sealed));
        assert_eq!(led.row_state(31), Some(CorpusState::Sealed));
        assert!(happy_path_ok());
    }

    #[test]
    fn injections_all_caught_30_rounds() {
        // 三类注入各 10 轮全抓：错码按类命中、无 Ok 泄漏、无漂移。
        let sw = bad_table_sweep();
        assert_eq!(sw.total, 30);
        assert_eq!(sw.trunc_caught, 10);
        assert_eq!(sw.ring_caught, 10);
        assert_eq!(sw.range_caught, 10);
        assert_eq!(sw.caught, 30);
        assert_eq!(sw.ok_leak, 0);
        assert_eq!(sw.misc_err, 0);
        assert!(sw.escape.is_empty());
        // 判据 20/20 主对口径成立。
        assert!(press20_ok());
    }

    #[test]
    fn fail_closed_never_ok_on_bad_tables() {
        // fail-closed：逐轮独立复走，坏表绝不 Ok；空表/坏 op/长链边角同拒。
        let mut r = 0u32;
        while r < 10 {
            assert!(walk_table(&make_trunc_table(r)).is_err());
            assert!(walk_table(&make_ring_table(r)).is_err());
            assert!(walk_table(&make_range_table(r)).is_err());
            r += 1;
        }
        let empty = UnwindTable {
            nodes: Vec::new(),
        };
        assert!(walk_table(&empty).is_err());
        let mut badop = UnwindTable {
            nodes: Vec::new(),
        };
        badop.nodes.push(TableNode {
            packed: 7 | (1 << 4),
            next: TERM,
            ex0: 0,
            ex1: 0,
            extra_n: 0,
        });
        assert!(walk_table(&badop).is_err());
        let mut long = UnwindTable {
            nodes: Vec::new(),
        };
        let mut i = 0u32;
        while i < 100 {
            let next = if i + 1 >= 100 { TERM } else { i + 1 };
            long.nodes.push(TableNode {
                packed: UwopOp::PushNonvol.code() | (1u32 << 4),
                next,
                ex0: 0,
                ex1: 0,
                extra_n: 0,
            });
            i += 1;
        }
        assert_eq!(walk_table(&long), Err(UwDiag::EBudget.val()));
        assert!(fail_closed_ok());
    }

    #[test]
    fn judge_order_pinned_enoent_einval_erewind() {
        // 判定次序：ENOENT 压过 EINVAL；EINVAL 压过 EREWIND；EREWIND 压过容量。
        let mut led = PressLedger::new();
        let bad_op = CorpusRow {
            idx: 0,
            op_code: 1,
            slot: 20,
            off: 0,
            packed: 1 | (20 << 4),
        };
        assert_eq!(led.judge_uwop_corpus(&bad_op), Err(UwDiag::ENoEnt.val()));
        let mut zero = PressLedger::new_capped(0);
        assert_eq!(zero.judge_uwop_corpus(&bad_op), Err(UwDiag::ENoEnt.val()));
        let mut full = PressLedger::new();
        let mut i = 0u32;
        while i < CORPUS_CAP as u32 {
            assert!(full.judge_uwop_corpus(&corpus_row(i)).is_ok());
            i += 1;
        }
        assert!(full.seal().is_ok());
        let r2 = corpus_row(2);
        let bad_bits = CorpusRow {
            idx: r2.idx,
            op_code: r2.op_code,
            slot: 3,
            off: r2.off,
            packed: (r2.op_code & 0xF) | (3 << 4),
        };
        assert_eq!(full.judge_uwop_corpus(&bad_bits), Err(UwDiag::EBits.val()));
        assert_eq!(
            full.judge_uwop_corpus(&corpus_row(5)),
            Err(UwDiag::ERewind.val())
        );
        assert!(judge_order_ok());
    }

    #[test]
    fn bit_pattern_gates_reject() {
        // 位型五路：槽位越掩码 / 打包不自洽 / 偏移失步 / 偏移越上界 / 零步带偏移。
        let mut led = PressLedger::new();
        let r2 = corpus_row(2);
        let bad_a = CorpusRow {
            idx: r2.idx,
            op_code: r2.op_code,
            slot: 3,
            off: r2.off,
            packed: (r2.op_code & 0xF) | (3 << 4),
        };
        assert_eq!(led.judge_uwop_corpus(&bad_a), Err(UwDiag::EBits.val()));
        let r0 = corpus_row(0);
        let bad_b = CorpusRow {
            packed: 1 | (r0.slot << 4),
            ..r0
        };
        assert_eq!(led.judge_uwop_corpus(&bad_b), Err(UwDiag::EBits.val()));
        let r3 = corpus_row(3);
        let bad_c = CorpusRow {
            off: r3.off.wrapping_add(1),
            ..r3
        };
        assert_eq!(led.judge_uwop_corpus(&bad_c), Err(UwDiag::EBits.val()));
        let bad_d = CorpusRow {
            off: 999,
            ..r3
        };
        assert_eq!(led.judge_uwop_corpus(&bad_d), Err(UwDiag::EBits.val()));
        let bad_e = CorpusRow {
            off: 8,
            ..r0
        };
        assert_eq!(led.judge_uwop_corpus(&bad_e), Err(UwDiag::EBits.val()));
        // 对照：合法行须通过。
        assert!(led.judge_uwop_corpus(&corpus_row(0)).is_ok());
        assert!(einval_gates_ok());
    }

    #[test]
    fn enoent_unregistered_rows_rejected() {
        // 未登记：闭集外 op 与越库行号全数 ENOENT。
        let mut led = PressLedger::new();
        let rogue = [1u32, 7, 8];
        let mut k = 0usize;
        while k < rogue.len() {
            let code = rogue.get(k).copied().unwrap_or(0);
            let row = CorpusRow {
                idx: 0,
                op_code: code,
                slot: 5,
                off: 0,
                packed: code | (5 << 4),
            };
            assert_eq!(led.judge_uwop_corpus(&row), Err(UwDiag::ENoEnt.val()));
            k += 1;
        }
        let r0 = corpus_row(0);
        let far = CorpusRow {
            idx: CORPUS_CAP as u32,
            ..r0
        };
        assert_eq!(led.judge_uwop_corpus(&far), Err(UwDiag::ENoEnt.val()));
        let far2 = CorpusRow {
            idx: u32::MAX,
            ..r0
        };
        assert_eq!(led.judge_uwop_corpus(&far2), Err(UwDiag::ENoEnt.val()));
        assert!(enoent_gates_ok());
    }

    #[test]
    fn capacity_exhaustion_boundary() {
        // 容量耗尽：小容判满拒新行；已登记行幂等重判；零容首行即拒。
        let mut led = PressLedger::new_capped(8);
        let mut i = 0u32;
        while i < 8 {
            assert!(led.judge_uwop_corpus(&corpus_row(i)).is_ok());
            i += 1;
        }
        assert_eq!(
            led.judge_uwop_corpus(&corpus_row(8)),
            Err(UwDiag::ECapFull.val())
        );
        assert!(led.judge_uwop_corpus(&corpus_row(3)).is_ok());
        assert_eq!(led.len(), 8);
        let mut zero = PressLedger::new_capped(0);
        assert_eq!(
            zero.judge_uwop_corpus(&corpus_row(0)),
            Err(UwDiag::ECapFull.val())
        );
        let mut full = PressLedger::new();
        let mut j = 0u32;
        while j < CORPUS_CAP as u32 {
            assert!(full.judge_uwop_corpus(&corpus_row(j)).is_ok());
            j += 1;
        }
        assert!(full.judge_uwop_corpus(&corpus_row(31)).is_ok());
        assert_eq!(full.len(), CORPUS_CAP);
        assert!(capacity_ok());
    }

    #[test]
    fn seal_gates_and_rewind() {
        // 封账闸：未齐封账 ESeal；判齐封账 Ok；重封/封后判定 EREWIND。
        let mut led = PressLedger::new();
        assert_eq!(led.seal(), Err(UwDiag::ESeal.val()));
        assert!(led.judge_uwop_corpus(&corpus_row(0)).is_ok());
        assert_eq!(led.seal(), Err(UwDiag::ESeal.val()));
        let mut i = 1u32;
        while i < CORPUS_CAP as u32 {
            assert!(led.judge_uwop_corpus(&corpus_row(i)).is_ok());
            i += 1;
        }
        assert!(led.seal().is_ok());
        assert_eq!(led.seal(), Err(UwDiag::ERewind.val()));
        assert_eq!(
            led.judge_uwop_corpus(&corpus_row(9)),
            Err(UwDiag::ERewind.val())
        );
        assert!(rewind_gates_ok());
    }

    #[test]
    fn good_table_walk_covers_all_ops() {
        // 好表对照：32 步走通，九 op 全覆盖。
        let rep = walk_table(&build_good_table());
        assert!(rep.is_ok());
        if let Ok(r) = rep {
            assert_eq!(r.steps, CORPUS_CAP);
            assert_eq!(r.seen.count_ones(), 9);
        }
        assert!(good_walk_ok());
    }

    #[test]
    fn dual_run_fingerprint_identical() {
        // 双跑一致：两次全流程指纹逐位一致且非零。
        let a = press_fingerprint();
        let b = press_fingerprint();
        assert_eq!(a, b);
        assert_ne!(a, 0);
        assert!(dual_run_ok());
        assert!(dual_run_verdict().is_ok());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12744_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
