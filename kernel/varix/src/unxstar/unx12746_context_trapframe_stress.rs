//! UNX-F12746 CONTEXT 陷阱帧一致性压测 · 0x82480段
//!
//! 判据原文（派工卡 12746 逐字锚定）：
//!   UNX-F12747-J1 双源漂移压测维持 10/10（B10 档回指），漂移即告警零静默。
//!
//! 设计要点：
//! 1. CONTEXT 保存/恢复一致性域模型：寄存器组 18 槽闭集——卡列 RAX/RBX/RCX/
//!    RDX/RBP/RDI/RSI/R8..R15/RIP/RSP 共 17 槽，补 CONTEXT 结构 RFlags 位域
//!    合成 18 槽闭集（x64 CONTEXT 陷阱帧必存 RFlags），槽序冻结，越槽显性拒；
//! 2. 保存/恢复状态机 Idle→Saved→Restored 单调推进，倒退与越级显性拒：
//!    未保存先恢复拒（EState）、已保存再保存拒（EState）、重复恢复拒（EAlready）；
//! 3. 一致性判定器 judge_context_roundtrip：对平记录 [恢复计数 1 字 +
//!    保存前 18 槽 + 恢复后 18 槽] 共 37 字；判定次序冻结为
//!    长度(EINVAL)→未恢复(EState)→重复恢复(EAlready)→逐槽对平(EDrift)；
//!    任一槽漂移即报 EDRIFT 语义码并指明槽号（槽号编码进错误码高 8 位）；
//! 4. 漂移注入器三类各 10 轮全抓：双源单槽（保存侧源A/恢复侧源B 逐轮轮转，
//!    即判据"双源漂移压测"）、多槽批量（2~3 槽）、恢复越权（重复恢复）——
//!    每轮漂移必须被判定器检出并留痕告警，漂移即告警零静默；
//! 5. 告警留痕日志定容显性：容量耗尽按条记 dropped 不静默吞；双跑一致指纹
//!    覆盖三类注入caught数 + 告警全文逐字节折叠，确定性 LCG（种子 = SEG_BASE）
//!    保证两次全流程逐字节复现；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死）；
//!    取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!    无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12746 − 12600) = 0x82480）。
pub const SEG_BASE: u32 = 0x82480;

// ── CONTEXT 诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtxDiag {
    /// 对平记录长度越 37 字闭集（EINVAL 语义）。
    ELen,
    /// 状态机越级/倒退（未保存先恢复、保存后再保存、计数为零却提交）。
    EState,
    /// 重复恢复（EALREADY 语义：已落 Restored 再次恢复越权）。
    EAlready,
    /// 槽漂移（EDRIFT 语义：逐槽对平失败，错误码高 8 位指明槽号）。
    EDrift,
    /// 注入槽号越 18 槽闭集。
    ESlotRange,
    /// 注入逃脱（漂移注入未被判定器抓住，零静默被破）。
    ETamper,
    /// 双跑指纹不一致（确定性被隐式状态破坏）。
    EDual,
    /// 告警日志容量耗尽未显性记账。
    ECap,
    /// 段位守卫失败（SEG_BASE 对齐/窗口违规）。
    ESeg,
    /// 诊断码/槽位闭集自检失败。
    EClosed,
}

impl CtxDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CtxDiag::ELen => 0x01,
                CtxDiag::EState => 0x02,
                CtxDiag::EAlready => 0x03,
                CtxDiag::EDrift => 0x04,
                CtxDiag::ESlotRange => 0x05,
                CtxDiag::ETamper => 0x06,
                CtxDiag::EDual => 0x07,
                CtxDiag::ECap => 0x08,
                CtxDiag::ESeg => 0x09,
                CtxDiag::EClosed => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            CtxDiag::ELen => "对平记录长度越37字闭集：格式为恢复计数1字+保存前18槽+恢复后18槽，先对长度再对槽",
            CtxDiag::EState => "状态机越级：未保存先恢复或已保存再保存，按 Idle→Saved→Restored 单调重走",
            CtxDiag::EAlready => "重复恢复：状态机已落 Restored 再恢复即越权，一轮对平只许恢复一次",
            CtxDiag::EDrift => "槽漂移：保存前/恢复后逐槽对平失败，错误码高8位即漂移槽号，按槽回查注入源",
            CtxDiag::ESlotRange => "注入槽号越18槽闭集：只认 RAX..RFLAGS 槽序0..17，先对槽表再注入",
            CtxDiag::ETamper => "注入逃脱：漂移注入未被判定器抓住，判定面失效须整组复核",
            CtxDiag::EDual => "双跑指纹不一致：种子固定下两次压测必须逐字节一致，先查隐式状态泄漏",
            CtxDiag::ECap => "告警日志溢出未记账：容量耗尽必须显性记 dropped，不许静默吞告警",
            CtxDiag::ESeg => "段位守卫失败：SEG_BASE 须对齐 0x40 且落在 0x80000..=0x831C0 窗口",
            CtxDiag::EClosed => "闭集自检失败：诊断码须互异且全落段窗口，槽位须 18 槽序完整",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const CTX_ALL: &[CtxDiag] = &[
    CtxDiag::ELen,
    CtxDiag::EState,
    CtxDiag::EAlready,
    CtxDiag::EDrift,
    CtxDiag::ESlotRange,
    CtxDiag::ETamper,
    CtxDiag::EDual,
    CtxDiag::ECap,
    CtxDiag::ESeg,
    CtxDiag::EClosed,
];

/// 语义码掩码（低 24 位；漂移码的槽号占高 8 位，不参与查表）。
pub const SEM_MASK: u32 = 0x00FF_FFFF;

/// 构造带槽号的漂移错误码（语义码在低 24 位，槽号在高 8 位）。
pub fn drift_code(slot: usize) -> u32 {
    (((slot as u32) & 0xFF) << 24) | (CtxDiag::EDrift.val() & SEM_MASK)
}

/// 提取错误码的语义部分（剥掉槽号位）。
pub fn code_semantic(code: u32) -> u32 {
    code & SEM_MASK
}

/// 提取漂移错误码中的槽号（非漂移码返回值无意义，调用方先验语义）。
pub fn code_slot(code: u32) -> usize {
    (code >> 24) as usize
}

/// 诊断码查表（闭集返回；带槽号的漂移码按语义部分命中，未知码回落提示）。
pub fn detail(code: u32) -> &'static str {
    let sem = code_semantic(code);
    for d in CTX_ALL.iter() {
        if d.val() == sem {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82481..0x8248A 闭集内"
}

// ── 寄存器组 18 槽闭集 ───────────────────────────────────────────
/// 18 槽闭集容量（判据冻结，不许扩缩）。
pub const SLOT_N: usize = 18;

/// 对平记录布局：恢复计数 1 字 + 保存前 18 槽 + 恢复后 18 槽 = 37 字。
pub const REC_HDR: usize = 1;
pub const REC_LEN: usize = REC_HDR + SLOT_N * 2;

/// CONTEXT 寄存器组 18 槽闭集（槽序冻结，对应 x64 陷阱帧保存序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtxSlot {
    /// 槽 0：RAX。
    Rax,
    /// 槽 1：RBX。
    Rbx,
    /// 槽 2：RCX。
    Rcx,
    /// 槽 3：RDX。
    Rdx,
    /// 槽 4：RBP。
    Rbp,
    /// 槽 5：RDI。
    Rdi,
    /// 槽 6：RSI。
    Rsi,
    /// 槽 7：R8。
    R8,
    /// 槽 8：R9。
    R9,
    /// 槽 9：R10。
    R10,
    /// 槽 10：R11。
    R11,
    /// 槽 11：R12。
    R12,
    /// 槽 12：R13。
    R13,
    /// 槽 13：R14。
    R14,
    /// 槽 14：R15。
    R15,
    /// 槽 15：RIP。
    Rip,
    /// 槽 16：RSP。
    Rsp,
    /// 槽 17：RFLAGS（x64 CONTEXT 陷阱帧必存位域，与卡列 17 槽合成 18 槽闭集）。
    RFlags,
}

impl CtxSlot {
    /// 槽号（闭集序，冻结）。
    pub fn index(self) -> usize {
        match self {
            CtxSlot::Rax => 0,
            CtxSlot::Rbx => 1,
            CtxSlot::Rcx => 2,
            CtxSlot::Rdx => 3,
            CtxSlot::Rbp => 4,
            CtxSlot::Rdi => 5,
            CtxSlot::Rsi => 6,
            CtxSlot::R8 => 7,
            CtxSlot::R9 => 8,
            CtxSlot::R10 => 9,
            CtxSlot::R11 => 10,
            CtxSlot::R12 => 11,
            CtxSlot::R13 => 12,
            CtxSlot::R14 => 13,
            CtxSlot::R15 => 14,
            CtxSlot::Rip => 15,
            CtxSlot::Rsp => 16,
            CtxSlot::RFlags => 17,
        }
    }

    /// 槽中文名（诊断文本用）。
    pub fn name(self) -> &'static str {
        match self {
            CtxSlot::Rax => "RAX",
            CtxSlot::Rbx => "RBX",
            CtxSlot::Rcx => "RCX",
            CtxSlot::Rdx => "RDX",
            CtxSlot::Rbp => "RBP",
            CtxSlot::Rdi => "RDI",
            CtxSlot::Rsi => "RSI",
            CtxSlot::R8 => "R8",
            CtxSlot::R9 => "R9",
            CtxSlot::R10 => "R10",
            CtxSlot::R11 => "R11",
            CtxSlot::R12 => "R12",
            CtxSlot::R13 => "R13",
            CtxSlot::R14 => "R14",
            CtxSlot::R15 => "R15",
            CtxSlot::Rip => "RIP",
            CtxSlot::Rsp => "RSP",
            CtxSlot::RFlags => "RFLAGS",
        }
    }

    /// 由槽号反查闭集槽（越界返回 None）。
    pub fn from_index(idx: u32) -> Option<CtxSlot> {
        match idx {
            0 => Some(CtxSlot::Rax),
            1 => Some(CtxSlot::Rbx),
            2 => Some(CtxSlot::Rcx),
            3 => Some(CtxSlot::Rdx),
            4 => Some(CtxSlot::Rbp),
            5 => Some(CtxSlot::Rdi),
            6 => Some(CtxSlot::Rsi),
            7 => Some(CtxSlot::R8),
            8 => Some(CtxSlot::R9),
            9 => Some(CtxSlot::R10),
            10 => Some(CtxSlot::R11),
            11 => Some(CtxSlot::R12),
            12 => Some(CtxSlot::R13),
            13 => Some(CtxSlot::R14),
            14 => Some(CtxSlot::R15),
            15 => Some(CtxSlot::Rip),
            16 => Some(CtxSlot::Rsp),
            17 => Some(CtxSlot::RFlags),
            _ => None,
        }
    }
}

/// 18 槽闭集数组（槽序即保存/恢复对平序）。
pub const SLOT_ALL: &[CtxSlot] = &[
    CtxSlot::Rax,
    CtxSlot::Rbx,
    CtxSlot::Rcx,
    CtxSlot::Rdx,
    CtxSlot::Rbp,
    CtxSlot::Rdi,
    CtxSlot::Rsi,
    CtxSlot::R8,
    CtxSlot::R9,
    CtxSlot::R10,
    CtxSlot::R11,
    CtxSlot::R12,
    CtxSlot::R13,
    CtxSlot::R14,
    CtxSlot::R15,
    CtxSlot::Rip,
    CtxSlot::Rsp,
    CtxSlot::RFlags,
];

// ── 保存/恢复状态机 ──────────────────────────────────────────────
/// 陷阱帧上下文状态机：Idle→Saved→Restored 单调，只进不退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtxState {
    /// 空闲：尚未保存。
    Idle,
    /// 已保存：允许恢复一次。
    Saved,
    /// 已恢复：本轮对平终结，再恢复即越权。
    Restored,
}

impl CtxState {
    /// 状态序（只许前进，倒退即违约）。
    pub fn rank(self) -> u8 {
        match self {
            CtxState::Idle => 0,
            CtxState::Saved => 1,
            CtxState::Restored => 2,
        }
    }
}

/// CONTEXT 陷阱帧保存/恢复组：18 槽闭集 + 单调状态机。
///
/// 保存侧源A = `live`（保存前原始快照，保存后不可变），存储帧 = `saved`
///（可被注入篡改的暂存帧），恢复侧源B = `restored`（恢复回写值）。
/// 双源逐槽对平（live vs restored）即判据"双源漂移压测"的对平面：
/// 存储帧被篡改会在恢复回写时传导为源B漂移，被判定器按槽抓出。
#[derive(Clone, Debug)]
pub struct ContextBank {
    /// 保存前原始快照（源A，保存后冻结不可变）。
    live: [u64; SLOT_N],
    /// 存储帧（可被注入篡改的暂存帧）。
    saved: [u64; SLOT_N],
    /// 恢复后回写值（源B）。
    restored: [u64; SLOT_N],
    /// 状态机落点。
    state: CtxState,
    /// 恢复执行次数（0=未恢复，1=恰一次，≥2=越权）。
    restore_count: u32,
}

impl ContextBank {
    /// 开一个空闲组。
    pub fn new() -> ContextBank {
        ContextBank {
            live: [0u64; SLOT_N],
            saved: [0u64; SLOT_N],
            restored: [0u64; SLOT_N],
            state: CtxState::Idle,
            restore_count: 0,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> CtxState {
        self.state
    }

    /// 恢复执行次数。
    pub fn restore_count(&self) -> u32 {
        self.restore_count
    }

    /// 保存：18 槽快照入组（长度越闭集拒；Idle 之外拒——单调不倒退）。
    ///
    /// 原始快照落 `live`（源A，此后冻结），同时落存储帧 `saved`。
    pub fn save(&mut self, live: &[u64]) -> Result<(), u32> {
        if live.len() != SLOT_N {
            return Err(CtxDiag::ELen.val());
        }
        if self.state.rank() >= CtxState::Saved.rank() {
            return Err(CtxDiag::EState.val());
        }
        for (dst, src) in self.live.iter_mut().zip(live.iter()) {
            *dst = *src;
        }
        self.saved = self.live;
        self.state = CtxState::Saved;
        Ok(())
    }

    /// 恢复：已保存帧回写（重复恢复拒；未保存先恢复拒）。
    pub fn restore(&mut self) -> Result<[u64; SLOT_N], u32> {
        if self.state == CtxState::Restored {
            return Err(CtxDiag::EAlready.val());
        }
        if self.state != CtxState::Saved {
            return Err(CtxDiag::EState.val());
        }
        self.restored = self.saved;
        self.restore_count = self.restore_count.saturating_add(1);
        self.state = CtxState::Restored;
        Ok(self.restored)
    }

    /// 注入：篡改存储帧单槽（模拟暂存帧损坏；仅 Saved 态可注入，槽号越闭集拒）。
    /// 存储帧损坏会在恢复回写时传导为源B漂移，被逐槽对平按槽抓出。
    pub fn tamper_saved(&mut self, slot: usize, val: u64) -> Result<(), u32> {
        if slot >= SLOT_N {
            return Err(CtxDiag::ESlotRange.val());
        }
        if self.state != CtxState::Saved {
            return Err(CtxDiag::EState.val());
        }
        match self.saved.get_mut(slot) {
            Some(p) => {
                *p = val;
                Ok(())
            }
            None => Err(CtxDiag::ESlotRange.val()),
        }
    }

    /// 注入：篡改恢复侧源B单槽（仅 Restored 态可注入，模拟回写损坏）。
    pub fn tamper_restored(&mut self, slot: usize, val: u64) -> Result<(), u32> {
        if slot >= SLOT_N {
            return Err(CtxDiag::ESlotRange.val());
        }
        if self.state != CtxState::Restored {
            return Err(CtxDiag::EState.val());
        }
        match self.restored.get_mut(slot) {
            Some(p) => {
                *p = val;
                Ok(())
            }
            None => Err(CtxDiag::ESlotRange.val()),
        }
    }

    /// 生成对平记录：[恢复计数, 源A(live) 18 槽, 源B(restored) 18 槽] 共 37 字。
    pub fn record(&self) -> Vec<u64> {
        let mut out: Vec<u64> = Vec::new();
        out.push(self.restore_count as u64);
        out.extend_from_slice(&self.live);
        out.extend_from_slice(&self.restored);
        out
    }
}

// ── 一致性判定器 ─────────────────────────────────────────────────
/// 对平判定成功产物：恢复后逐槽值 + 对平槽数 + 状态落点。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextRow {
    /// 恢复后 18 槽值（按槽序）。
    pub regs: [u64; SLOT_N],
    /// 对平通过的槽数（满对平 = 18）。
    pub matched: usize,
    /// 判定通过时的状态落点（恒 Restored）。
    pub state: CtxState,
}

/// CONTEXT 保存/恢复一致性判定器。
///
/// 对平记录布局：[恢复计数 1 字][保存前 18 槽][恢复后 18 槽] = 37 字。
/// 判定次序冻结：长度(EINVAL)→未恢复(EState)→重复恢复(EAlready)→逐槽对平
/// (EDrift+槽号)。任一槽漂移即报 EDRIFT 语义码并以错误码高 8 位指明槽号。
pub fn judge_context_roundtrip(rec: &[u64]) -> Result<ContextRow, u32> {
    // ① 长度闸：越 37 字闭集即 EINVAL，先于一切语义判定。
    if rec.len() != REC_LEN {
        return Err(CtxDiag::ELen.val());
    }
    // ② 恢复计数闸：0=未恢复却提交对平记录；>1=重复恢复越权。
    let rc = match rec.first() {
        Some(v) => *v,
        None => return Err(CtxDiag::ELen.val()),
    };
    if rc == 0 {
        return Err(CtxDiag::EState.val());
    }
    if rc > 1 {
        return Err(CtxDiag::EAlready.val());
    }
    // ③ 逐槽对平：保存前/恢复后两源逐槽对平，任一槽漂移即报槽号。
    let saved_part = rec.iter().skip(REC_HDR).take(SLOT_N);
    let rest_part = rec.iter().skip(REC_HDR + SLOT_N).take(SLOT_N);
    for (i, (a, b)) in saved_part.zip(rest_part).enumerate() {
        if a != b {
            return Err(drift_code(i));
        }
    }
    // ④ 满对平：恢复后 18 槽逐槽落行。
    let mut regs = [0u64; SLOT_N];
    let rest_iter = rec.iter().skip(REC_HDR + SLOT_N).take(SLOT_N);
    for (dst, src) in regs.iter_mut().zip(rest_iter) {
        *dst = *src;
    }
    Ok(ContextRow {
        regs,
        matched: SLOT_N,
        state: CtxState::Restored,
    })
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg64 {
    state: u64,
}

impl Lcg64 {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg64 {
        Lcg64 { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

/// 构造确定性 live 上下文：18 槽全非零（零槽值保留给哨兵语义）。
fn make_live(rng: &mut Lcg64) -> [u64; SLOT_N] {
    let mut v = [0u64; SLOT_N];
    for slot in v.iter_mut() {
        *slot = 1u64 + rng.below(0xFFFE) as u64;
    }
    v
}

// ── 告警留痕日志（定容显性记账） ─────────────────────────────────
/// 告警日志容量：三类注入 10+10+10=30 条留痕，留 10 条余量。
pub const ALERT_CAP: usize = 40;

/// 定容告警留痕日志：容量耗尽按条记 dropped，显性不静默。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlertLog {
    entries: Vec<String>,
    dropped: usize,
}

impl AlertLog {
    /// 开一个空日志。
    pub fn new() -> AlertLog {
        AlertLog {
            entries: Vec::new(),
            dropped: 0,
        }
    }

    /// 落一条告警（超容显性记 dropped，不静默吞）。
    pub fn push(&mut self, msg: String) {
        if self.entries.len() >= ALERT_CAP {
            self.dropped = self.dropped.saturating_add(1);
        } else {
            self.entries.push(msg);
        }
    }

    /// 已留痕条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否无留痕。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 因容量耗尽被拒的条数。
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// 全文快照（双跑比对用）。
    pub fn snapshot(&self) -> Vec<String> {
        self.entries.clone()
    }
}

// ── 注入面：三类漂移注入器（各 10 轮全抓） ───────────────────────
/// 注入轮次（三类各 10 轮，判据"维持 10/10"）。
pub const INJ_ROUNDS: usize = 10;

/// 对平判定：漂移必须被检出且槽号精确指认（单槽用）。
fn drift_slot_matches(rec: &[u64], slot: usize) -> bool {
    match judge_context_roundtrip(rec) {
        Err(code) => {
            code_semantic(code) == CtxDiag::EDrift.val() && code_slot(code) == slot
        }
        Ok(_) => false,
    }
}

/// 对平判定：漂移必须被检出且首漂移槽落在注入集合内（批量用）。
fn drift_slot_in_set(rec: &[u64], picks: &[usize]) -> bool {
    match judge_context_roundtrip(rec) {
        Err(code) => {
            code_semantic(code) == CtxDiag::EDrift.val() && picks.contains(&code_slot(code))
        }
        Ok(_) => false,
    }
}

/// 注入扫描统计：轮数 / 抓住数 / 首个逃脱路描述（抓住即留痕告警）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SweepStat {
    /// 实际轮数。
    pub rounds: usize,
    /// 漂移被判定器检出的轮数。
    pub caught: usize,
    /// 首个逃脱轮的描述（全抓时为空串）。
    pub first_escape: String,
}

/// 注入一：双源单槽漂移压测（判据"双源"锚定）。
///
/// 10 轮逐轮轮转注入源：偶数轮篡改保存侧源A（保存帧被改），
/// 奇数轮篡改恢复侧源B（回写值被改）——两源都必须被逐槽对平抓住，
/// 且错误码高 8 位必须指认被注入的槽号。抓住即落告警留痕。
pub fn inject_two_source_sweep(alerts: &mut AlertLog) -> SweepStat {
    let mut rng = Lcg64::new((SEG_BASE as u64).wrapping_add(0x5A5A));
    let mut st = SweepStat {
        rounds: 0,
        caught: 0,
        first_escape: String::new(),
    };
    let mut round = 0usize;
    while round < INJ_ROUNDS {
        st.rounds = st.rounds.saturating_add(1);
        let slot = rng.below(SLOT_N as u32) as usize;
        let poison = 0xDEAD_0000u64.wrapping_add(round as u64);
        let mut bank = ContextBank::new();
        let live = make_live(&mut rng);
        let mut why = String::new();
        let mut caught = false;
        match bank.save(&live) {
            Ok(()) => {
                // 双源轮转：偶数轮注源A（保存态改保存帧，再恢复），
                // 奇数轮注源B（恢复态改回写值）。
                let flow = if round % 2 == 0 {
                    bank.tamper_saved(slot, poison)
                        .and_then(|_| bank.restore().map(|_| ()))
                } else {
                    bank.restore().and_then(|_| bank.tamper_restored(slot, poison))
                };
                match flow {
                    Ok(_) => {
                        let rec = bank.record();
                        caught = drift_slot_matches(&rec, slot);
                        if !caught {
                            why = format!("轮{}槽{}漂移未被判定器检出", round, slot);
                        }
                    }
                    Err(e) => why = format!("轮{}注入流程被拒0x{:x}", round, e),
                }
            }
            Err(e) => why = format!("轮{}保存被拒0x{:x}", round, e),
        }
        if caught {
            st.caught = st.caught.saturating_add(1);
            alerts.push(format!("双源轮{}槽{}漂移已检出留痕", round, slot));
        } else if st.first_escape.is_empty() {
            st.first_escape = why;
        }
        round = round.saturating_add(1);
    }
    st
}

/// 注入二：多槽批量漂移（每轮 2~3 槽批量篡改保存侧源A）。
///
/// 10 轮全部必须被抓住，且首个漂移槽号必须落在注入集合内。
pub fn inject_multi_slot_sweep(alerts: &mut AlertLog) -> SweepStat {
    let mut rng = Lcg64::new((SEG_BASE as u64).wrapping_add(0xA5A5));
    let mut st = SweepStat {
        rounds: 0,
        caught: 0,
        first_escape: String::new(),
    };
    let mut round = 0usize;
    while round < INJ_ROUNDS {
        st.rounds = st.rounds.saturating_add(1);
        // 每轮批量 2 或 3 槽，槽号互异（LCG 抖动 + 重试上限）。
        let want = 2usize + (round % 2);
        let mut picks: Vec<usize> = Vec::new();
        let mut guard = 0u32;
        while picks.len() < want && guard < 64 {
            guard = guard.saturating_add(1);
            let cand = rng.below(SLOT_N as u32) as usize;
            if !picks.contains(&cand) {
                picks.push(cand);
            }
        }
        let mut why = String::new();
        let mut caught = false;
        if picks.len() != want {
            why = format!("轮{}批量槽抽样不足{}槽", round, want);
        } else {
            let mut bank = ContextBank::new();
            let live = make_live(&mut rng);
            match bank.save(&live) {
                Ok(()) => {
                    // 批量注入走保存态（源A），注完再恢复进入对平面。
                    let mut inj_ok = true;
                    for p in picks.iter() {
                        let poison = 0xFA11_0000u64.wrapping_add(*p as u64);
                        if bank.tamper_saved(*p, poison).is_err() {
                            inj_ok = false;
                        }
                    }
                    if !inj_ok {
                        why = format!("轮{}批量注入被拒", round);
                    } else {
                        match bank.restore() {
                            Ok(_) => {
                                let rec = bank.record();
                                caught = drift_slot_in_set(&rec, &picks);
                                if !caught {
                                    why = format!("轮{}批量漂移未被判定器检出", round);
                                }
                            }
                            Err(e) => why = format!("轮{}恢复被拒0x{:x}", round, e),
                        }
                    }
                }
                Err(e) => why = format!("轮{}保存被拒0x{:x}", round, e),
            }
        }
        if caught {
            st.caught = st.caught.saturating_add(1);
            alerts.push(format!("批量轮{}槽{:?}漂移已检出留痕", round, picks));
        } else if st.first_escape.is_empty() {
            st.first_escape = why;
        }
        round = round.saturating_add(1);
    }
    st
}

/// 注入三：恢复越权（每轮先正常恢复一次，再越权重复恢复一次）。
///
/// 10 轮全部必须被 EALREADY 拒绝：重复恢复即漂移面的另一源，零静默。
pub fn inject_overreach_sweep(alerts: &mut AlertLog) -> SweepStat {
    let mut rng = Lcg64::new((SEG_BASE as u64).wrapping_add(0x0F0F));
    let mut st = SweepStat {
        rounds: 0,
        caught: 0,
        first_escape: String::new(),
    };
    let mut round = 0usize;
    while round < INJ_ROUNDS {
        st.rounds = st.rounds.saturating_add(1);
        let live = make_live(&mut rng);
        let mut bank = ContextBank::new();
        let mut why = String::new();
        let mut caught = false;
        match bank.save(&live) {
            Ok(()) => {
                let first = bank.restore();
                let second = bank.restore();
                caught = first.is_ok() && second == Err(CtxDiag::EAlready.val());
                if !caught {
                    why = format!("轮{}重复恢复未被EALREADY拒绝", round);
                }
            }
            Err(e) => why = format!("轮{}保存被拒0x{:x}", round, e),
        }
        if caught {
            st.caught = st.caught.saturating_add(1);
            alerts.push(format!("越权轮{}重复恢复已拒留痕", round));
        } else if st.first_escape.is_empty() {
            st.first_escape = why;
        }
        round = round.saturating_add(1);
    }
    st
}

// ── 记录改写工具（判定次序/边界构作用，无裸下标） ────────────────
/// 把记录中第 offset 字替换为 val（越界 offset 返回原样副本，不越权写）。
fn with_slot_replaced(rec: &[u64], offset: usize, val: u64) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    for (i, v) in rec.iter().enumerate() {
        if i == offset {
            out.push(val);
        } else {
            out.push(*v);
        }
    }
    out
}

/// 把记录的恢复计数（首字）改写为 hdr（构造 0/2 计数样本）。
fn record_with_hdr(rec: &[u64], hdr: u64) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    out.push(hdr);
    for v in rec.iter().skip(REC_HDR) {
        out.push(*v);
    }
    out
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在派工窗口 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：互异、落窗、查表命中、槽号编码往返。
fn diag_closed_ok() -> bool {
    for d in CTX_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        // 互异：同值码只许出现一次。
        let mut cnt = 0usize;
        for e in CTX_ALL.iter() {
            if e.val() == v {
                cnt = cnt.saturating_add(1);
            }
        }
        if cnt != 1 {
            return false;
        }
        if d.detail().is_empty() {
            return false;
        }
    }
    // 查表命中与未知回落。
    if detail(CtxDiag::ELen.val()) != CtxDiag::ELen.detail() {
        return false;
    }
    if detail(SEG_BASE + 0x3F) == CtxDiag::EDrift.detail() {
        return false;
    }
    // 槽号编码往返：语义可查、槽号可提、查表按语义命中。
    let dc = drift_code(7);
    if code_semantic(dc) != CtxDiag::EDrift.val() {
        return false;
    }
    if code_slot(dc) != 7 {
        return false;
    }
    if detail(dc) != CtxDiag::EDrift.detail() {
        return false;
    }
    true
}

/// 槽位闭集守卫：18 槽、槽序完整、反查往返、名称非空。
fn slot_closed_ok() -> bool {
    if SLOT_ALL.len() != SLOT_N {
        return false;
    }
    for (i, s) in SLOT_ALL.iter().enumerate() {
        if s.index() != i {
            return false;
        }
        if s.name().is_empty() {
            return false;
        }
        match CtxSlot::from_index(i as u32) {
            Some(r) => {
                if r.index() != i {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
}

/// 状态机单调：Idle→Saved→Restored 只进不退，倒退/越级显性拒。
fn state_monotonic_ok() -> bool {
    if !(CtxState::Idle.rank() < CtxState::Saved.rank()
        && CtxState::Saved.rank() < CtxState::Restored.rank())
    {
        return false;
    }
    let mut rng = Lcg64::new((SEG_BASE as u64).wrapping_add(0x11));
    let live = make_live(&mut rng);
    let mut bank = ContextBank::new();
    // 未保存先恢复 → EState。
    if bank.restore() != Err(CtxDiag::EState.val()) {
        return false;
    }
    // 保存成功后再保存 → EState（单调倒退拒）。
    if bank.save(&live).is_err() {
        return false;
    }
    if bank.save(&live) != Err(CtxDiag::EState.val()) {
        return false;
    }
    // 恢复一次成功；再恢复 → EAlready。
    if bank.restore().is_err() {
        return false;
    }
    if bank.restore() != Err(CtxDiag::EAlready.val()) {
        return false;
    }
    // Restored 后再保存 → EState。
    bank.save(&live) == Err(CtxDiag::EState.val())
}

/// 正常流：保存→恢复→对平全绿，逐槽值与 live 恒等。
fn happy_path_ok() -> bool {
    let mut rng = Lcg64::new((SEG_BASE as u64).wrapping_add(0x22));
    let live = make_live(&mut rng);
    let mut bank = ContextBank::new();
    if bank.state() != CtxState::Idle {
        return false;
    }
    if bank.save(&live).is_err() {
        return false;
    }
    if bank.state() != CtxState::Saved {
        return false;
    }
    if bank.restore().is_err() {
        return false;
    }
    if bank.restore_count() != 1 {
        return false;
    }
    let rec = bank.record();
    if rec.len() != REC_LEN {
        return false;
    }
    match judge_context_roundtrip(&rec) {
        Ok(row) => {
            if row.matched != SLOT_N {
                return false;
            }
            if row.state != CtxState::Restored {
                return false;
            }
            for (a, b) in row.regs.iter().zip(live.iter()) {
                if a != b {
                    return false;
                }
            }
            true
        }
        Err(_) => false,
    }
}

/// 判定次序：长度 → 未恢复 → 重复恢复 → 逐槽对平，先到先报。
fn judge_order_ok() -> bool {
    let mut rng = Lcg64::new((SEG_BASE as u64).wrapping_add(0x33));
    let live = make_live(&mut rng);
    let mut bank = ContextBank::new();
    if bank.save(&live).is_err() {
        return false;
    }
    if bank.restore().is_err() {
        return false;
    }
    let clean = bank.record();
    // ① 长度闸最前：38 字即便含漂移也先报 EINVAL。
    let mut long38 = clean.clone();
    long38.push(0xBAD0);
    let drifted_long = with_slot_replaced(&long38, REC_HDR + SLOT_N + 5, 0xBAD1);
    if judge_context_roundtrip(&drifted_long) != Err(CtxDiag::ELen.val()) {
        return false;
    }
    // ② 未恢复（计数 0）先于逐槽漂移。
    let drifted = with_slot_replaced(&clean, REC_HDR + SLOT_N + 3, 0xBAD2);
    let zero_hdr = record_with_hdr(&drifted, 0);
    if judge_context_roundtrip(&zero_hdr) != Err(CtxDiag::EState.val()) {
        return false;
    }
    // ③ 重复恢复（计数 2）先于逐槽漂移。
    let two_hdr = record_with_hdr(&drifted, 2);
    if judge_context_roundtrip(&two_hdr) != Err(CtxDiag::EAlready.val()) {
        return false;
    }
    // ④ 同一记录无违约才放行。
    judge_context_roundtrip(&clean).is_ok()
}

/// 注入一全抓：双源单槽 10/10，且留痕恰 10 条。
fn two_source_caught_ok() -> bool {
    let mut log = AlertLog::new();
    let st = inject_two_source_sweep(&mut log);
    st.rounds == INJ_ROUNDS
        && st.caught == INJ_ROUNDS
        && st.first_escape.is_empty()
        && log.len() == INJ_ROUNDS
        && log.dropped() == 0
}

/// 注入二全抓：多槽批量 10/10，且留痕恰 10 条。
fn multi_slot_caught_ok() -> bool {
    let mut log = AlertLog::new();
    let st = inject_multi_slot_sweep(&mut log);
    st.rounds == INJ_ROUNDS
        && st.caught == INJ_ROUNDS
        && st.first_escape.is_empty()
        && log.len() == INJ_ROUNDS
        && log.dropped() == 0
}

/// 注入三全抓：恢复越权 10/10 全被 EALREADY 拒，且留痕恰 10 条。
fn overreach_caught_ok() -> bool {
    let mut log = AlertLog::new();
    let st = inject_overreach_sweep(&mut log);
    st.rounds == INJ_ROUNDS
        && st.caught == INJ_ROUNDS
        && st.first_escape.is_empty()
        && log.len() == INJ_ROUNDS
        && log.dropped() == 0
}

/// 漂移即告警零静默：三类注入抓住总数 == 留痕总数 == 轮数总数。
fn zero_silence_ok(a: &SweepStat, b: &SweepStat, c: &SweepStat, log: &AlertLog) -> bool {
    let caught = a.caught + b.caught + c.caught;
    let rounds = a.rounds + b.rounds + c.rounds;
    if rounds != INJ_ROUNDS * 3 {
        return false;
    }
    if caught != rounds {
        return false;
    }
    if !a.first_escape.is_empty() || !b.first_escape.is_empty() || !c.first_escape.is_empty() {
        return false;
    }
    log.len() == caught && log.dropped() == 0
}

/// 双跑一致：指纹 + 三类统计 + 告警全文两次全流程逐项相同。
fn dual_run_ok() -> bool {
    stress_bundle() == stress_bundle()
}

/// 压测全家桶：三类注入统计 + 告警全文 + 折叠指纹（确定性，双跑比对用）。
fn stress_bundle() -> (u64, Vec<SweepStat>, Vec<String>) {
    let mut log = AlertLog::new();
    let mut stats: Vec<SweepStat> = Vec::new();
    stats.push(inject_two_source_sweep(&mut log));
    stats.push(inject_multi_slot_sweep(&mut log));
    stats.push(inject_overreach_sweep(&mut log));
    // 指纹：caught/rounds/留痕数 + 告警全文逐字节折叠。
    let mut h = 0x2446_7821_0000_0001u64;
    for v in [stats.len(), log.len(), log.dropped()] {
        h = h.wrapping_mul(0x1_0000_0001_B3).wrapping_add(v as u64);
    }
    for st in stats.iter() {
        h = h
            .wrapping_mul(0x1_0000_0001_B3)
            .wrapping_add(st.caught as u64);
        h = h
            .wrapping_mul(0x1_0000_0001_B3)
            .wrapping_add(st.rounds as u64);
    }
    for t in log.snapshot().iter() {
        for byte in t.as_bytes().iter() {
            h = h
                .wrapping_mul(0x1_0000_0001_B3)
                .wrapping_add(*byte as u64);
        }
    }
    (h, stats, log.snapshot())
}

/// 边界四类：空记录 / 短长记录 / 首尾槽漂移指认 / 计数 0 与 2 语义。
fn boundary_ok() -> bool {
    let mut rng = Lcg64::new((SEG_BASE as u64).wrapping_add(0x44));
    let live = make_live(&mut rng);
    let mut bank = ContextBank::new();
    if bank.save(&live).is_err() {
        return false;
    }
    if bank.restore().is_err() {
        return false;
    }
    let clean = bank.record();
    // 边界一：空记录 → EINVAL。
    if judge_context_roundtrip(&[]) != Err(CtxDiag::ELen.val()) {
        return false;
    }
    // 边界二：短 36 字 / 长 38 字均 → EINVAL。
    let mut short36: Vec<u64> = Vec::new();
    for v in clean.iter().take(REC_LEN - 1) {
        short36.push(*v);
    }
    if judge_context_roundtrip(&short36) != Err(CtxDiag::ELen.val()) {
        return false;
    }
    let mut long38 = clean.clone();
    long38.push(7);
    if judge_context_roundtrip(&long38) != Err(CtxDiag::ELen.val()) {
        return false;
    }
    // 边界三：首槽（槽0）与尾槽（槽17）漂移，槽号指认必须精确。
    let head = with_slot_replaced(&clean, REC_HDR + SLOT_N, 0xBAD3);
    match judge_context_roundtrip(&head) {
        Err(code) => {
            if code_semantic(code) != CtxDiag::EDrift.val() || code_slot(code) != 0 {
                return false;
            }
        }
        Ok(_) => return false,
    }
    let tail_off = REC_HDR + SLOT_N + (SLOT_N - 1);
    let tail = with_slot_replaced(&clean, tail_off, 0xBAD4);
    match judge_context_roundtrip(&tail) {
        Err(code) => {
            if code_semantic(code) != CtxDiag::EDrift.val() || code_slot(code) != SLOT_N - 1 {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 边界四：保存侧源A漂移同样被抓（对平面覆盖两源）。
    let saved_side = with_slot_replaced(&clean, REC_HDR, 0xBAD5);
    match judge_context_roundtrip(&saved_side) {
        Err(code) => {
            if code_semantic(code) != CtxDiag::EDrift.val() || code_slot(code) != 0 {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 容量耗尽显性记账：超容按条记 dropped，正常压测留痕不溢出。
fn capacity_ok() -> bool {
    // 超容 5 条：留痕恒 ALERT_CAP，dropped 恰 5。
    let mut log = AlertLog::new();
    let total = ALERT_CAP + 5;
    let mut k = 0usize;
    while k < total {
        log.push(format!("容量压测条目{}", k));
        k = k.saturating_add(1);
    }
    if log.len() != ALERT_CAP || log.dropped() != 5 {
        return false;
    }
    // 正常三类注入留痕 30 条 < 容量 40：零溢出。
    let mut log2 = AlertLog::new();
    let _a = inject_two_source_sweep(&mut log2);
    let _b = inject_multi_slot_sweep(&mut log2);
    let _c = inject_overreach_sweep(&mut log2);
    log2.dropped() == 0 && log2.len() == INJ_ROUNDS * 3
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// 本节判据检查总条数（13 条主检 + 1 条收口）。
pub const CHECK_N: usize = 14;

/// UNX-F12746 自检（判据 12746 卡：CONTEXT 陷阱帧一致性压测，双源漂移 10/10）。
pub fn run_unx12746_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12746");
    let mut added = 0usize;

    s.add_dyn(
        "F12746-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} 对齐40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );
    added += 1;

    s.add_dyn(
        "F12746-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x}",
            CTX_ALL.len(),
            CTX_ALL.first().map(|d| d.val()).unwrap_or(0)
        ),
    );
    added += 1;

    s.add_dyn(
        "F12746-03 槽位闭集 18 槽",
        slot_closed_ok(),
        &format!(
            "槽数={} 首={} 尾={}",
            SLOT_ALL.len(),
            SLOT_ALL.first().map(|r| r.name()).unwrap_or("?"),
            SLOT_ALL.last().map(|r| r.name()).unwrap_or("?")
        ),
    );
    added += 1;

    s.add_dyn(
        "F12746-04 状态机单调",
        state_monotonic_ok(),
        "Idle→Saved→Restored 倒退/越级全数拒",
    );
    added += 1;

    s.add_dyn("F12746-05 正常流对平", happy_path_ok(), "保存→恢复→逐槽全平 18/18");
    added += 1;

    s.add_dyn(
        "F12746-06 判定次序",
        judge_order_ok(),
        "长度→未恢复→重复恢复→逐槽 先到先报",
    );
    added += 1;

    let mut log = AlertLog::new();
    let single = inject_two_source_sweep(&mut log);
    let multi = inject_multi_slot_sweep(&mut log);
    let over = inject_overreach_sweep(&mut log);

    s.add_dyn(
        "F12746-07 双源单槽注入 10/10",
        single.caught == INJ_ROUNDS && single.rounds == INJ_ROUNDS && single.first_escape.is_empty(),
        &format!("抓{}/{} {}", single.caught, single.rounds, single.first_escape),
    );
    added += 1;

    s.add_dyn(
        "F12746-08 多槽批量注入 10/10",
        multi.caught == INJ_ROUNDS && multi.rounds == INJ_ROUNDS && multi.first_escape.is_empty(),
        &format!("抓{}/{} {}", multi.caught, multi.rounds, multi.first_escape),
    );
    added += 1;

    s.add_dyn(
        "F12746-09 恢复越权注入 10/10",
        over.caught == INJ_ROUNDS && over.rounds == INJ_ROUNDS && over.first_escape.is_empty(),
        &format!("抓{}/{} EALREADY全拒", over.caught, over.rounds),
    );
    added += 1;

    s.add_dyn(
        "F12746-10 漂移告警零静默",
        zero_silence_ok(&single, &multi, &over, &log),
        &format!(
            "留痕={} 抓住={} 溢出={}",
            log.len(),
            single.caught + multi.caught + over.caught,
            log.dropped()
        ),
    );
    added += 1;

    s.add_dyn(
        "F12746-11 双跑一致",
        dual_run_ok(),
        "指纹+三类统计+告警全文 两次全流程逐项一致",
    );
    added += 1;

    s.add_dyn("F12746-12 边界四类", boundary_ok(), "空/36/38/首尾槽/源A侧 五点");
    added += 1;

    s.add_dyn(
        "F12746-13 容量耗尽",
        capacity_ok(),
        &format!("cap={} 超5条dropped恰5 正常留痕零溢出", ALERT_CAP),
    );
    added += 1;

    // 收口：判据条数声明（13 条主检 + 本条收口 = CHECK_N）。
    s.add_dyn(
        "F12746-14 收口·判据条数",
        added + 1 == CHECK_N,
        &format!("判据检查共{}条=主检{}条+收口1条", CHECK_N, added),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在派工窗口 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：10 码互异、全落窗口、查表命中、未知回落、槽号编码往返。
        assert_eq!(CTX_ALL.len(), 10);
        let mut i = 0usize;
        while i < CTX_ALL.len() {
            let vi = CTX_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < CTX_ALL.len() {
                let vj = CTX_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        // 未知码回落。
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
        // 漂移槽号编码往返。
        let dc = drift_code(17);
        assert_eq!(code_semantic(dc), CtxDiag::EDrift.val());
        assert_eq!(code_slot(dc), 17);
        assert_eq!(detail(dc), CtxDiag::EDrift.detail());
    }

    #[test]
    fn slot_closed_set_is_18() {
        // 18 槽闭集：槽序 0..18 完整、反查往返、名称非空。
        assert_eq!(SLOT_N, 18);
        assert_eq!(SLOT_ALL.len(), 18);
        assert_eq!(REC_LEN, 37);
        for (i, s) in SLOT_ALL.iter().enumerate() {
            assert_eq!(s.index(), i);
            assert!(!s.name().is_empty());
            assert_eq!(CtxSlot::from_index(i as u32).map(|r| r.index()), Some(i));
        }
        // 越界反查返回 None。
        assert!(CtxSlot::from_index(18).is_none());
        // 首尾槽指认。
        assert_eq!(SLOT_ALL.first().map(|r| r.name()), Some("RAX"));
        assert_eq!(SLOT_ALL.last().map(|r| r.name()), Some("RFLAGS"));
    }

    #[test]
    fn happy_path_roundtrip_green() {
        // 正常流：保存→恢复→对平全绿，恢复值与 live 逐槽恒等。
        let mut rng = Lcg64::new((SEG_BASE as u64).wrapping_add(0x22));
        let live = make_live(&mut rng);
        let mut bank = ContextBank::new();
        assert_eq!(bank.state(), CtxState::Idle);
        assert!(bank.save(&live).is_ok());
        assert_eq!(bank.state(), CtxState::Saved);
        assert!(bank.restore().is_ok());
        assert_eq!(bank.state(), CtxState::Restored);
        assert_eq!(bank.restore_count(), 1);
        let rec = bank.record();
        assert_eq!(rec.len(), REC_LEN);
        let row = match judge_context_roundtrip(&rec) {
            Ok(r) => r,
            Err(e) => panic!("正常流被判红 0x{:x}", e),
        };
        assert_eq!(row.matched, 18);
        assert_eq!(row.state, CtxState::Restored);
        for (a, b) in row.regs.iter().zip(live.iter()) {
            assert_eq!(a, b);
        }
        assert!(happy_path_ok());
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机单调：越级/倒退/重复恢复全数显性拒。
        assert!(CtxState::Idle.rank() < CtxState::Saved.rank());
        assert!(CtxState::Saved.rank() < CtxState::Restored.rank());
        let mut rng = Lcg64::new((SEG_BASE as u64).wrapping_add(0x11));
        let live = make_live(&mut rng);
        let mut bank = ContextBank::new();
        assert_eq!(bank.restore(), Err(CtxDiag::EState.val()));
        assert!(bank.save(&live).is_ok());
        assert_eq!(bank.save(&live), Err(CtxDiag::EState.val()));
        assert!(bank.restore().is_ok());
        assert_eq!(bank.restore(), Err(CtxDiag::EAlready.val()));
        assert_eq!(bank.save(&live), Err(CtxDiag::EState.val()));
        // 注入状态闸：Idle 态禁注恢复侧。
        let mut idle = ContextBank::new();
        assert_eq!(idle.tamper_restored(0, 1), Err(CtxDiag::EState.val()));
        assert!(state_monotonic_ok());
    }

    #[test]
    fn judge_order_is_frozen() {
        // 判定次序：长度 → 未恢复 → 重复恢复 → 逐槽对平，先到先报。
        let mut rng = Lcg64::new((SEG_BASE as u64).wrapping_add(0x33));
        let live = make_live(&mut rng);
        let mut bank = ContextBank::new();
        assert!(bank.save(&live).is_ok());
        assert!(bank.restore().is_ok());
        let clean = bank.record();
        // ① 长度闸最前：38 字含漂移也先报 EINVAL。
        let mut long38 = clean.clone();
        long38.push(0xBAD0);
        let drifted_long = with_slot_replaced(&long38, REC_HDR + SLOT_N + 5, 0xBAD1);
        assert_eq!(
            judge_context_roundtrip(&drifted_long),
            Err(CtxDiag::ELen.val())
        );
        // ② 未恢复（计数 0）先于漂移。
        let drifted = with_slot_replaced(&clean, REC_HDR + SLOT_N + 3, 0xBAD2);
        assert_eq!(
            judge_context_roundtrip(&record_with_hdr(&drifted, 0)),
            Err(CtxDiag::EState.val())
        );
        // ③ 重复恢复（计数 2）先于漂移。
        assert_eq!(
            judge_context_roundtrip(&record_with_hdr(&drifted, 2)),
            Err(CtxDiag::EAlready.val())
        );
        // ④ 无违约才放行。
        assert!(judge_context_roundtrip(&clean).is_ok());
        assert!(judge_order_ok());
    }

    #[test]
    fn two_source_injection_ten_of_ten() {
        // 双源单槽：10/10 全抓，偶轮源A/奇轮源B，槽号指认精确。
        let mut log = AlertLog::new();
        let st = inject_two_source_sweep(&mut log);
        assert_eq!(st.rounds, 10);
        assert_eq!(st.caught, 10, "逃脱路：{}", st.first_escape);
        assert!(st.first_escape.is_empty());
        assert_eq!(log.len(), 10);
        assert_eq!(log.dropped(), 0);
        // 定点复核：槽 5 注源A、槽 9 注源B，槽号必须精确指认。
        let mut rng = Lcg64::new((SEG_BASE as u64).wrapping_add(0x77));
        let live = make_live(&mut rng);
        let mut b1 = ContextBank::new();
        let _ = b1.save(&live);
        let _ = b1.tamper_saved(5, 0xDEAD_BEEF);
        let _ = b1.restore();
        match judge_context_roundtrip(&b1.record()) {
            Err(code) => {
                assert_eq!(code_semantic(code), CtxDiag::EDrift.val());
                assert_eq!(code_slot(code), 5);
            }
            Ok(_) => panic!("源A单槽漂移未被抓"),
        }
        let mut b2 = ContextBank::new();
        let _ = b2.save(&live);
        let _ = b2.restore();
        let _ = b2.tamper_restored(9, 0xDEAD_F00D);
        match judge_context_roundtrip(&b2.record()) {
            Err(code) => {
                assert_eq!(code_semantic(code), CtxDiag::EDrift.val());
                assert_eq!(code_slot(code), 9);
            }
            Ok(_) => panic!("源B单槽漂移未被抓"),
        }
    }

    #[test]
    fn multi_slot_injection_ten_of_ten() {
        // 多槽批量：10/10 全抓，首个漂移槽号必落注入集合。
        let mut log = AlertLog::new();
        let st = inject_multi_slot_sweep(&mut log);
        assert_eq!(st.rounds, 10);
        assert_eq!(st.caught, 10, "逃脱路：{}", st.first_escape);
        assert!(st.first_escape.is_empty());
        assert_eq!(log.len(), 10);
        // 定点复核：批量 3 槽 {2, 11, 16}，首漂移槽必在其中。
        let mut rng = Lcg64::new((SEG_BASE as u64).wrapping_add(0x88));
        let live = make_live(&mut rng);
        let mut bank = ContextBank::new();
        let _ = bank.save(&live);
        let picks = [2usize, 11, 16];
        for p in picks.iter() {
            let _ = bank.tamper_saved(*p, 0xFA11_0000 + *p as u64);
        }
        let _ = bank.restore();
        match judge_context_roundtrip(&bank.record()) {
            Err(code) => {
                assert_eq!(code_semantic(code), CtxDiag::EDrift.val());
                assert!(picks.contains(&code_slot(code)));
            }
            Ok(_) => panic!("批量漂移未被抓"),
        }
    }

    #[test]
    fn overreach_injection_ten_of_ten() {
        // 恢复越权：10/10 全被 EALREADY 拒。
        let mut log = AlertLog::new();
        let st = inject_overreach_sweep(&mut log);
        assert_eq!(st.rounds, 10);
        assert_eq!(st.caught, 10, "逃脱路：{}", st.first_escape);
        assert!(st.first_escape.is_empty());
        assert_eq!(log.len(), 10);
        // 定点复核：首恢复 Ok，再恢复必 EALREADY。
        let mut rng = Lcg64::new((SEG_BASE as u64).wrapping_add(0x99));
        let live = make_live(&mut rng);
        let mut bank = ContextBank::new();
        let _ = bank.save(&live);
        assert!(bank.restore().is_ok());
        assert_eq!(bank.restore(), Err(CtxDiag::EAlready.val()));
        assert_eq!(bank.restore_count(), 1);
    }

    #[test]
    fn zero_silence_alert_ledger() {
        // 漂移即告警零静默：抓住数 == 留痕数 == 轮数，无溢出无逃脱。
        let mut log = AlertLog::new();
        let a = inject_two_source_sweep(&mut log);
        let b = inject_multi_slot_sweep(&mut log);
        let c = inject_overreach_sweep(&mut log);
        assert!(zero_silence_ok(&a, &b, &c, &log));
        assert_eq!(log.len(), 30);
        assert_eq!(log.dropped(), 0);
        // 留痕全文非空且每条含"留痕"落款。
        for t in log.snapshot().iter() {
            assert!(t.contains("留痕"));
        }
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：指纹 + 三类统计 + 告警全文两次全流程逐项相同。
        assert!(dual_run_ok());
        let (h1, s1, t1) = stress_bundle();
        let (h2, s2, t2) = stress_bundle();
        assert_eq!(h1, h2);
        assert_eq!(s1, s2);
        assert_eq!(t1, t2);
        assert_eq!(s1.len(), 3);
    }

    #[test]
    fn boundary_four_classes() {
        // 边界四类：空记录 / 短36长38 / 首尾槽指认 / 源A侧漂移。
        let mut rng = Lcg64::new((SEG_BASE as u64).wrapping_add(0x44));
        let live = make_live(&mut rng);
        let mut bank = ContextBank::new();
        let _ = bank.save(&live);
        let _ = bank.restore();
        let clean = bank.record();
        // 空记录。
        assert_eq!(judge_context_roundtrip(&[]), Err(CtxDiag::ELen.val()));
        // 短 36。
        let mut short36: Vec<u64> = Vec::new();
        for v in clean.iter().take(REC_LEN - 1) {
            short36.push(*v);
        }
        assert_eq!(short36.len(), 36);
        assert_eq!(
            judge_context_roundtrip(&short36),
            Err(CtxDiag::ELen.val())
        );
        // 长 38。
        let mut long38 = clean.clone();
        long38.push(7);
        assert_eq!(judge_context_roundtrip(&long38), Err(CtxDiag::ELen.val()));
        // 首槽与尾槽漂移指认。
        let head = with_slot_replaced(&clean, REC_HDR + SLOT_N, 0xBAD3);
        match judge_context_roundtrip(&head) {
            Err(code) => assert_eq!(code_slot(code), 0),
            Ok(_) => panic!("首槽漂移未被抓"),
        }
        let tail = with_slot_replaced(&clean, REC_HDR + SLOT_N + 17, 0xBAD4);
        match judge_context_roundtrip(&tail) {
            Err(code) => assert_eq!(code_slot(code), 17),
            Ok(_) => panic!("尾槽漂移未被抓"),
        }
        // 保存侧源A漂移同样被抓。
        let saved_side = with_slot_replaced(&clean, REC_HDR, 0xBAD5);
        match judge_context_roundtrip(&saved_side) {
            Err(code) => assert_eq!(code_slot(code), 0),
            Ok(_) => panic!("源A侧漂移未被抓"),
        }
        assert!(boundary_ok());
    }

    #[test]
    fn capacity_exhaustion_accounted() {
        // 容量耗尽：超容按条记 dropped，不静默吞。
        let mut log = AlertLog::new();
        let total = ALERT_CAP + 5;
        let mut k = 0usize;
        while k < total {
            log.push(format!("容量压测条目{}", k));
            k += 1;
        }
        assert_eq!(log.len(), ALERT_CAP);
        assert_eq!(log.dropped(), 5);
        // 正常压测留痕 30 条 < 40：零溢出。
        assert!(capacity_ok());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条（实落 14 条含收口）。
        let s = run_unx12746_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), CHECK_N);
        assert_eq!(s.tally().1, 0);
        assert_eq!(CHECK_N, 14);
    }
}
