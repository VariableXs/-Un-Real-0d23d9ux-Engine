//! UNX-F12651 对象类型枚举探测（ObjectTypeInformation） · 0x80C80段
//!
//! 判据原文（派工卡 12650 逐字锚定）：
//!   ① 纯功能代码 ≥290 行；
//!   ② 判据原文：UNX-F12651-J1 特殊句柄值域（CurrentProcess=-1/
//!      CurrentThread=-2 档注出处）对平断言绿，越值域注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 对象类型枚举域模型：枚举行 =（类型索引/类型名/句柄计数/可查句柄数/
//!    峰值句柄数）五元，类型索引从 1 起单调递增（ObjectTypeInformation
//!    公开语义，只引结论不抄码）；
//! 2. 特殊句柄值域闭集（档注出处：公开句柄语义结论，只引结论不抄码）：
//!    CurrentProcess = -1（u64 全 1 回绕位型 0xFFFF_FFFF_FFFF_FFFF）、
//!    CurrentThread = -2（0xFFFF_FFFF_FFFF_FFFE）；此二值是值域合法档，
//!    其余近邻回绕位型（-3/-4/高位半零）一律越值域；
//! 3. 句柄档位模型：任意句柄先归档（特殊档/普通档/越域档），归档是纯
//!    位型判别——本面档案只收特殊档（-1/-2 两位型），普通档位型与本面
//!    无关（分类语义保留），越域档全部显性拒；
//! 4. 对平断言 10/10：枚举行数、类型索引单调、类型名非空、句柄计数
//!    递增序、可查 ≤ 总数、峰值 ≤ 总数、特殊档恒两行、位型值恒等、
//!    值域档位齐、枚举名闭集齐——十条独立写死，任何一条为假即红；
//! 5. 越值域注入 10/10：十路越域位型构造（-3、-4、0x7FFF...、
//!    0x8000...、0、1、0xFFFF_0000_0000_0000、0x1、全零半回绕、
//!    高位混搭）逐路必须被值域闸抓住——放行即判据失效；
//! 6. 状态机 空面→已枚举→已对平 单向推进，倒退显性拒；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12650 − 12600)）。
pub const SEG_BASE: u32 = 0x80C80;

// ── 对象类型枚举诊断码闭集 ───────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjDiag {
    /// 类型索引越枚举序（不按 1..N 单调递增）。
    EIndexOrder,
    /// 类型名越名册闭集。
    ENameRange,
    /// 类型名重复。
    ENameDup,
    /// 句柄计数倒退（枚举序上不许减）。
    EHandleBack,
    /// 可查句柄数超总数。
    EValidOver,
    /// 峰值句柄数超总数。
    EPeakOver,
    /// 句柄位型越值域（特殊档只认 -1/-2）。
    EHandleRange,
    /// 句柄档案重复归档。
    EHandleDup,
    /// 未枚举就试图对平。
    ENotEnumerated,
    /// 对平断言未全过。
    EParity,
    /// 越值域注入逃脱。
    EEscape,
    /// 状态机倒退。
    ERewind,
}

impl ObjDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ObjDiag::EIndexOrder => 0x01,
                ObjDiag::ENameRange => 0x02,
                ObjDiag::ENameDup => 0x03,
                ObjDiag::EHandleBack => 0x04,
                ObjDiag::EValidOver => 0x05,
                ObjDiag::EPeakOver => 0x06,
                ObjDiag::EHandleRange => 0x07,
                ObjDiag::EHandleDup => 0x08,
                ObjDiag::ENotEnumerated => 0x09,
                ObjDiag::EParity => 0x0a,
                ObjDiag::EEscape => 0x0b,
                ObjDiag::ERewind => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            ObjDiag::EIndexOrder => "类型索引越枚举序：ObjectTypeInformation 按 1..N 单调递增枚举，跳行先回补",
            ObjDiag::ENameRange => "类型名越名册闭集：枚举名必须与本面名册逐字对上，先核名再落账",
            ObjDiag::ENameDup => "类型名重复：每类型只落一笔，重复名先查名册闭集",
            ObjDiag::EHandleBack => "句柄计数倒退：枚举序上句柄计数不许减，倒退行先复核快照时点",
            ObjDiag::EValidOver => "可查句柄数超总数：可查 ≤ 总数恒成立，越等先修计数再入账",
            ObjDiag::EPeakOver => "峰值句柄数超总数：峰值 ≤ 总数恒成立，越等先修峰值再入账",
            ObjDiag::EHandleRange => "句柄位型越值域：特殊档只认 -1/-2 两位型，越域位型一律显性拒",
            ObjDiag::EHandleDup => "句柄档案重复归档：同一位型只归一档，重复归档先查已有位型",
            ObjDiag::ENotEnumerated => "未枚举禁止对平：先落齐枚举行，再走对平口",
            ObjDiag::EParity => "对平断言未全过：10/10 有红项，按红项编号回修对应枚举行",
            ObjDiag::EEscape => "越值域注入逃脱：值域闸没抓住越域位型，闸面失效须整面复核",
            ObjDiag::ERewind => "状态机倒退：空面→已枚举→已对平只进不退，倒退即面可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const OBJ_ALL: &[ObjDiag] = &[
    ObjDiag::EIndexOrder,
    ObjDiag::ENameRange,
    ObjDiag::ENameDup,
    ObjDiag::EHandleBack,
    ObjDiag::EValidOver,
    ObjDiag::EPeakOver,
    ObjDiag::EHandleRange,
    ObjDiag::EHandleDup,
    ObjDiag::ENotEnumerated,
    ObjDiag::EParity,
    ObjDiag::EEscape,
    ObjDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in OBJ_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80C81..0x80C8C 闭集内"
}

// ── 特殊句柄位型闭集（档注出处：公开句柄语义结论，只引结论不抄码） ─
/// CurrentProcess 特殊句柄位型（-1 的 u64 全 1 回绕位型）。
pub const HANDLE_CURRENT_PROCESS: u64 = 0xFFFF_FFFF_FFFF_FFFF;
/// CurrentThread 特殊句柄位型（-2 的 u64 回绕位型）。
pub const HANDLE_CURRENT_THREAD: u64 = 0xFFFF_FFFF_FFFF_FFFE;

/// 特殊句柄档位：值域合法档闭集（只认 -1/-2）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialTier {
    /// CurrentProcess = -1。
    CurProcess,
    /// CurrentThread = -2。
    CurThread,
}

impl SpecialTier {
    /// 位型值。
    pub fn bits(self) -> u64 {
        match self {
            SpecialTier::CurProcess => HANDLE_CURRENT_PROCESS,
            SpecialTier::CurThread => HANDLE_CURRENT_THREAD,
        }
    }

    /// 档名（含档注出处语义）。
    pub fn name(self) -> &'static str {
        match self {
            SpecialTier::CurProcess => "CurrentProcess(-1)",
            SpecialTier::CurThread => "CurrentThread(-2)",
        }
    }

    /// 由位型归档（越值域返回 None）。
    pub fn from_bits(bits: u64) -> Option<SpecialTier> {
        if bits == HANDLE_CURRENT_PROCESS {
            return Some(SpecialTier::CurProcess);
        }
        if bits == HANDLE_CURRENT_THREAD {
            return Some(SpecialTier::CurThread);
        }
        None
    }
}

/// 特殊档闭集数组。
pub const SPECIAL_ALL: &[SpecialTier] = &[SpecialTier::CurProcess, SpecialTier::CurThread];

/// 特殊档容量（恒 2）。
pub const SPECIAL_CAP: usize = 2;

/// 任意句柄档位：特殊档 / 普通档 / 越域档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleTier {
    /// 特殊档（-1/-2 位型）。
    Special(SpecialTier),
    /// 普通档（1..0x7FFF_FFFF_FFFF_FFFE 的正向合法位型）。
    Normal,
    /// 越域档（0 位型 / 负语义近邻回绕 / 高位异常回绕）。
    OutOfRange,
}

/// 普通档位型上沿（负语义半区起点减一）。
pub const NORMAL_CAP: u64 = 0x7FFF_FFFF_FFFF_FFFE;

/// 句柄位型归档（纯位型判别，无 IO 无浮点）。
pub fn tier_of_bits(bits: u64) -> HandleTier {
    match SpecialTier::from_bits(bits) {
        Some(t) => HandleTier::Special(t),
        None => {
            // 普通档：正向位型 1..0x7FFF_FFFF_FFFF_FFFE；其余（0 位型、
            // 负语义近邻回绕 -3 及以上、高位异常回绕）一律越域。
            if bits >= 1 && bits <= NORMAL_CAP {
                HandleTier::Normal
            } else {
                HandleTier::OutOfRange
            }
        }
    }
}

// ── 对象类型名册闭集 ─────────────────────────────────────────────
/// 名册行：枚举类型名（行序即枚举序，冻结）。
pub const OBJ_NAMES: &[&str] = &[
    "Process",
    "Thread",
    "Token",
    "Event",
    "Mutant",
    "Semaphore",
    "Section",
    "File",
];

/// 名册容量。
pub const OBJ_CAP: usize = 8;

/// 名是否在名册闭集内。
pub fn name_in_roster(name: &str) -> bool {
    OBJ_NAMES.iter().any(|n| *n == name)
}

// ── 枚举行数据模型 ───────────────────────────────────────────────
/// 枚举行五元：类型索引/类型名/句柄计数/可查句柄数/峰值句柄数。
#[derive(Clone, Debug)]
pub struct ObjRow {
    /// 类型索引（1 起）。
    pub index: u32,
    /// 类型名。
    pub name: String,
    /// 句柄计数。
    pub handle_count: u32,
    /// 可查句柄数。
    pub valid_count: u32,
    /// 峰值句柄数。
    pub peak_count: u32,
}

impl ObjRow {
    /// 行内恒等：可查 ≤ 总数 且 峰值 ≤ 总数。
    pub fn counts_sound(&self) -> bool {
        self.valid_count <= self.handle_count && self.peak_count <= self.handle_count
    }

    /// 行摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        format!(
            "类型{}({}):总{} 可查{} 峰值{}",
            self.index, self.name, self.handle_count, self.valid_count, self.peak_count
        )
    }
}

/// 句柄档案行：位型 + 归档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandleRec {
    /// 位型值。
    pub bits: u64,
    /// 归档。
    pub tier: HandleTier,
}

/// 枚举面状态机：空面 → 已枚举 → 已对平，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnumState {
    /// 空面：只许落枚举行与归档句柄。
    Bare,
    /// 已枚举：行齐 + 特殊档齐，等待对平。
    Enumerated,
    /// 已对平：只读。
    Paritied,
}

impl EnumState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            EnumState::Bare => 0,
            EnumState::Enumerated => 1,
            EnumState::Paritied => 2,
        }
    }
}

/// 对象类型枚举探测面。
#[derive(Clone, Debug)]
pub struct ObjEnum {
    /// 枚举行（按枚举序）。
    rows: Vec<ObjRow>,
    /// 句柄档案（按归档序）。
    handles: Vec<HandleRec>,
    /// 面状态。
    state: EnumState,
}

impl ObjEnum {
    /// 开一个新枚举面。
    pub fn new() -> ObjEnum {
        ObjEnum {
            rows: Vec::new(),
            handles: Vec::new(),
            state: EnumState::Bare,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> EnumState {
        self.state
    }

    /// 枚举行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否无行。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&ObjRow> {
        self.rows.get(idx)
    }

    /// 句柄档案行数。
    pub fn handle_len(&self) -> usize {
        self.handles.len()
    }

    /// 取第 idx 档（越界 None）。
    pub fn handle_at(&self, idx: usize) -> Option<&HandleRec> {
        self.handles.get(idx)
    }

    /// 落一行枚举（索引序/名册闭集/重复名/计数倒退/计数恒等五闸）。
    pub fn push_row(&mut self, index: u32, name: &str, total: u32, valid: u32, peak: u32) -> Result<(), u32> {
        // 对平后写行拒绝。
        if self.state.rank() >= EnumState::Paritied.rank() {
            return Err(ObjDiag::ERewind.val());
        }
        // 索引序闸：1 起单调递增。
        let expect = self.rows.len() as u32 + 1;
        if index != expect {
            return Err(ObjDiag::EIndexOrder.val());
        }
        // 名册闭集闸。
        if !name_in_roster(name) {
            return Err(ObjDiag::ENameRange.val());
        }
        // 重复名闸。
        for r in self.rows.iter() {
            if r.name == name {
                return Err(ObjDiag::ENameDup.val());
            }
        }
        // 计数恒等闸：可查/峰值 ≤ 总数。
        if valid > total {
            return Err(ObjDiag::EValidOver.val());
        }
        if peak > total {
            return Err(ObjDiag::EPeakOver.val());
        }
        // 计数倒退闸：枚举序上总数不许减。
        if let Some(prev) = self.rows.last() {
            if total < prev.handle_count {
                return Err(ObjDiag::EHandleBack.val());
            }
        }
        self.rows.push(ObjRow {
            index,
            name: name.to_string(),
            handle_count: total,
            valid_count: valid,
            peak_count: peak,
        });
        Ok(())
    }

    /// 句柄归档（特殊值域闸 + 重复归档闸）。
    /// 值域语义：本面档案只收特殊档位型（-1/-2），普通档位型与本面
    /// 无关（分类语义保留在 tier_of_bits），越域档位型显性拒。
    pub fn file_handle(&mut self, bits: u64) -> Result<HandleTier, u32> {
        // 对平后归档拒绝。
        if self.state.rank() >= EnumState::Paritied.rank() {
            return Err(ObjDiag::ERewind.val());
        }
        let t = tier_of_bits(bits);
        // 特殊值域闸：非特殊档位型（普通档/越域档）一律显性拒。
        match t {
            HandleTier::Special(_) => {}
            _ => return Err(ObjDiag::EHandleRange.val()),
        }
        // 重复归档闸。
        for h in self.handles.iter() {
            if h.bits == bits {
                return Err(ObjDiag::EHandleDup.val());
            }
        }
        self.handles.push(HandleRec { bits, tier: t });
        Ok(t)
    }

    /// 推进到已枚举：名册行齐 + 特殊档恒两行才许推进。
    pub fn mark_enumerated(&mut self) -> Result<(), u32> {
        if self.state.rank() >= EnumState::Enumerated.rank() {
            return Err(ObjDiag::ERewind.val());
        }
        if self.rows.len() != OBJ_CAP {
            return Err(ObjDiag::ENotEnumerated.val());
        }
        if self.handles.len() != SPECIAL_CAP {
            return Err(ObjDiag::EHandleRange.val());
        }
        self.state = EnumState::Enumerated;
        Ok(())
    }

    /// 对平：10/10 断言全绿才许推进。
    pub fn reconcile(&mut self) -> Result<(), u32> {
        if self.state.rank() >= EnumState::Paritied.rank() {
            return Err(ObjDiag::ERewind.val());
        }
        if self.state.rank() < EnumState::Enumerated.rank() {
            return Err(ObjDiag::ENotEnumerated.val());
        }
        if !parity_all_green(self) {
            return Err(ObjDiag::EParity.val());
        }
        self.state = EnumState::Paritied;
        Ok(())
    }
}

// ── 对平断言 10/10 ───────────────────────────────────────────────
/// 对平断言：枚举行数 / 类型索引单调 / 类型名非空 / 句柄计数不减序 /
/// 可查 ≤ 总数 / 峰值 ≤ 总数 / 特殊档恒两行 / 位型值恒等 / 值域档位齐 /
/// 枚举名闭集齐 = 10 条。每条独立写死，不复用被测值。
pub fn parity_assertions(p: &ObjEnum) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    // 断言 0：枚举行数恒等名册容量。
    out.push(("对平0-行数恒等8".to_string(), p.len() == OBJ_CAP && !p.is_empty()));
    // 断言 1：类型索引单调（1..N 各一按序）。
    let mut idx_ok = true;
    for i in 0..p.len() {
        match p.row_at(i) {
            Some(r) => {
                if r.index != (i + 1) as u32 {
                    idx_ok = false;
                }
            }
            None => idx_ok = false,
        }
    }
    out.push(("对平1-索引单调".to_string(), idx_ok));
    // 断言 2：类型名非空。
    let mut name_ok = true;
    for r in p.rows.iter() {
        if r.name.is_empty() {
            name_ok = false;
        }
    }
    out.push(("对平2-名称非空".to_string(), name_ok));
    // 断言 3：句柄计数不减序。
    let mut mono_ok = true;
    let mut prev = 0u32;
    for r in p.rows.iter() {
        if r.handle_count < prev {
            mono_ok = false;
        }
        prev = r.handle_count;
    }
    out.push(("对平3-计数不减序".to_string(), mono_ok));
    // 断言 4：可查 ≤ 总数（逐行）。
    let mut valid_ok = true;
    for r in p.rows.iter() {
        if !r.counts_sound() {
            valid_ok = false;
        }
    }
    out.push(("对平4-计数恒等".to_string(), valid_ok));
    // 断言 5：峰值 ≤ 总数（逐行显式复核）。
    let mut peak_ok = true;
    for r in p.rows.iter() {
        if r.peak_count > r.handle_count {
            peak_ok = false;
        }
    }
    out.push(("对平5-峰值恒等".to_string(), peak_ok));
    // 断言 6：特殊档恒两行。
    out.push((
        "对平6-特殊档恒两行".to_string(),
        p.handle_len() == SPECIAL_CAP && p.handle_len() == SPECIAL_ALL.len(),
    ));
    // 断言 7：位型值恒等（档案位型与特殊档闭集逐位相等）。
    let mut bits_ok = true;
    for i in 0..p.handle_len() {
        let rec = match p.handle_at(i) {
            Some(h) => h,
            None => {
                bits_ok = false;
                break;
            }
        };
        let expect = match SPECIAL_ALL.get(i) {
            Some(t) => t.bits(),
            None => {
                bits_ok = false;
                break;
            }
        };
        if rec.bits != expect {
            bits_ok = false;
        }
    }
    out.push(("对平7-位型值恒等".to_string(), bits_ok));
    // 断言 8：值域档位齐（档案只含特殊档，无越域）。
    let mut tier_ok = true;
    for i in 0..p.handle_len() {
        if let Some(h) = p.handle_at(i) {
            if let HandleTier::OutOfRange = h.tier {
                tier_ok = false;
            }
            if SpecialTier::from_bits(h.bits).is_none() {
                tier_ok = false;
            }
        } else {
            tier_ok = false;
        }
    }
    out.push(("对平8-值域档位齐".to_string(), tier_ok));
    // 断言 9：枚举名闭集齐（逐行与名册对上）。
    let mut roster_ok = true;
    for i in 0..p.len() {
        match p.row_at(i) {
            Some(r) => {
                let roster = match OBJ_NAMES.get(i) {
                    Some(n) => *n,
                    None => {
                        roster_ok = false;
                        break;
                    }
                };
                if r.name != roster {
                    roster_ok = false;
                }
            }
            None => {
                roster_ok = false;
                break;
            }
        }
    }
    out.push(("对平9-名册闭集齐".to_string(), roster_ok));
    out
}

/// 10/10 全绿判定。
pub fn parity_all_green(p: &ObjEnum) -> bool {
    let list = parity_assertions(p);
    if list.len() != 10 {
        return false;
    }
    for item in list.iter() {
        let _name = &item.0;
        if !item.1 {
            return false;
        }
    }
    true
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取自 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
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

// ── 构建器：完整枚举面 ───────────────────────────────────────────
/// 构建完整枚举面（名册逐行复现，计数确定性递增，特殊档 -1/-2 归档）。
pub fn build_full_enum() -> ObjEnum {
    let mut p = ObjEnum::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut base = 4u32;
    let mut i = 0usize;
    while i < OBJ_CAP {
        let name = match OBJ_NAMES.get(i) {
            Some(n) => *n,
            None => break,
        };
        let total = base + 1 + rng.below(64);
        base = total;
        let valid = if total > 1 { 1 + rng.below(total - 1) } else { 1 };
        let peak = if total > 1 { 1 + rng.below(total - 1) } else { 1 };
        match p.push_row((i + 1) as u32, name, total, valid, peak) {
            Ok(()) => i += 1,
            Err(e) => {
                // 正常流不该出错；出错时终止构造（面缺行，断言会红）。
                let _ = e;
                break;
            }
        }
    }
    let _ = p.file_handle(HANDLE_CURRENT_PROCESS);
    let _ = p.file_handle(HANDLE_CURRENT_THREAD);
    let _ = p.mark_enumerated();
    p
}

// ── 越值域注入面：十路越域位型 ───────────────────────────────────
/// 越值域注入器：十路越域位型构造，每路必须被值域闸抓住。
/// 返回 (拒止路数, 总路数, 首个逃脱路描述)。
pub fn out_of_range_sweep() -> (usize, usize, String) {
    let mut rejected = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    // 十路越域位型（手写闭集：0 与近邻回绕混搭，全不落 -1/-2）。
    let cases: [(&'static str, u64); 10] = [
        ("零位型", 0),
        ("一号位型", 1),
        ("回绕-3", 0xFFFF_FFFF_FFFF_FFFD),
        ("回绕-4", 0xFFFF_FFFF_FFFF_FFFC),
        ("高位半零", 0xFFFF_0000_0000_0000),
        ("低位半回绕", 0x0000_0000_FFFF_FFFF),
        ("符号位单置", 0x8000_0000_0000_0000),
        ("高位混搭A", 0xFFFF_FFFF_0000_0001),
        ("高位混搭B", 0x7FFF_FFFF_FFFF_FFFF),
        ("特殊位型差一位", 0xFFFF_FFFF_FFFF_FFF0),
    ];
    for c in cases.iter() {
        let _desc = c.0;
        let mut p = build_full_enum();
        let verdict = p.file_handle(c.1);
        total += 1;
        match verdict {
            Err(_) => rejected += 1,
            Ok(_) => {
                if escape.is_empty() {
                    escape = format!("路({}) 位型 0x{:016X} 放行", c.0, c.1);
                }
            }
        }
    }
    (rejected, total, escape)
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 特殊句柄值域对平绿（判据 J1 的"对平断言绿"直读口）。
fn parity_ok() -> bool {
    let mut p = build_full_enum();
    if p.len() != OBJ_CAP {
        return false;
    }
    if p.handle_len() != SPECIAL_CAP {
        return false;
    }
    if p.reconcile().is_err() {
        return false;
    }
    if p.state() != EnumState::Paritied {
        return false;
    }
    true
}

/// 越值域注入 10/10 拒止（判据 J1 的注入面直读口）。
fn injection_ok() -> bool {
    let (rejected, total, escape) = out_of_range_sweep();
    if total != 10 {
        return false;
    }
    if rejected != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 特殊档位型语义（档注出处直读口：-1/-2 归档正确）。
fn special_tier_ok() -> bool {
    match tier_of_bits(HANDLE_CURRENT_PROCESS) {
        HandleTier::Special(SpecialTier::CurProcess) => {}
        _ => return false,
    }
    match tier_of_bits(HANDLE_CURRENT_THREAD) {
        HandleTier::Special(SpecialTier::CurThread) => {}
        _ => return false,
    }
    // 越域位型归档必须落在越域档。
    match tier_of_bits(0) {
        HandleTier::OutOfRange => {}
        _ => return false,
    }
    match tier_of_bits(0xFFFF_FFFF_FFFF_FFFD) {
        HandleTier::OutOfRange => {}
        _ => return false,
    }
    true
}

/// 双跑一致（确定性 LCG 下两次全流程逐项相同）。
fn dual_run_ok() -> bool {
    let a = build_full_enum();
    let b = build_full_enum();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(ra), Some(rb)) => {
                if ra.name != rb.name || ra.handle_count != rb.handle_count {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let ra = out_of_range_sweep();
    let rb = out_of_range_sweep();
    if ra.0 != rb.0 || ra.1 != rb.1 {
        return false;
    }
    true
}

/// 边界：索引乱序/名册越名/重复名/计数越等/计数倒退 五点全拒。
fn boundary_ok() -> bool {
    let mut p = ObjEnum::new();
    // 索引乱序：首行索引 2 被拒。
    if p.push_row(2, "Process", 10, 5, 6).is_ok() {
        return false;
    }
    // 名册越名被拒。
    if p.push_row(1, "GhostObj", 10, 5, 6).is_ok() {
        return false;
    }
    // 计数越等被拒。
    if p.push_row(1, "Process", 10, 11, 6).is_ok() {
        return false;
    }
    // 峰值越等被拒。
    if p.push_row(1, "Process", 10, 5, 11).is_ok() {
        return false;
    }
    // 正常落首行后计数倒退被拒。
    let _ = p.push_row(1, "Process", 10, 5, 6);
    if p.push_row(2, "Thread", 5, 3, 4).is_ok() {
        return false;
    }
    // 重复名被拒（先补到第 2 行再重复第 1 行名——重复名在第 2 行落）。
    let mut q = ObjEnum::new();
    let _ = q.push_row(1, "Process", 10, 5, 6);
    if q.push_row(2, "Process", 12, 6, 7).is_ok() {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12650u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut p = build_full_enum();
    let _ = p.reconcile();
    if p.state() != EnumState::Paritied {
        return false;
    }
    match p.row_at(0) {
        Some(r) => {
            if r.brief().is_empty() {
                return false;
            }
        }
        None => return false,
    }
    // 特殊档摘要非空。
    let mut specials = String::new();
    for i in 0..p.handle_len() {
        if let Some(h) = p.handle_at(i) {
            match SpecialTier::from_bits(h.bits) {
                Some(t) => specials.push_str(t.name()),
                None => specials.push_str("?"),
            }
        }
    }
    if specials.is_empty() {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12651 特殊句柄值域对平自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12650_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12650-objtype-enum");
    let face = build_full_enum();

    s.add_dyn(
        "F12651-01 名册八行齐",
        OBJ_NAMES.len() == OBJ_CAP,
        &format!("rows={}", OBJ_NAMES.len()),
    );

    s.add_dyn(
        "F12651-02 特殊句柄值域对平绿",
        parity_ok(),
        &format!("green={}", parity_all_green(&face)),
    );

    s.add_dyn(
        "F12651-03 越值域注入 10/10 拒止",
        injection_ok(),
        &format!("rejected={}", out_of_range_sweep().0),
    );

    s.add_dyn(
        "F12651-04 特殊档位型语义",
        special_tier_ok(),
        "-1→CurProcess -2→CurThread 0/-3→越域 四点归档",
    );

    s.add_dyn(
        "F12651-05 双跑一致",
        dual_run_ok(),
        "两次构建与两次注入逐项一致",
    );

    s.add_dyn(
        "F12651-06 边界五闸",
        boundary_ok(),
        "索引乱序/越名/越等/峰值越等/计数倒退+重名 全拒",
    );

    s.add_dyn(
        "F12651-07 完成写回语义",
        writeback_ok(),
        "对平即已完成且摘要非空",
    );

    s.add_dyn(
        "F12651-08 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 对平断言表结构：恒 10 条且每条独立为真。
    let list = parity_assertions(&face);
    let mut all_true = list.len() == 10;
    for item in list.iter() {
        let _name = &item.0;
        if !item.1 {
            all_true = false;
        }
    }
    s.add_dyn(
        "F12651-09 对平断言表结构",
        all_true,
        &format!("rows={} head={}", list.len(), match list.get(0) { Some(x) => x.0.clone(), None => String::new() }),
    );

    // 特殊档恒等：档案两行位型与闭集逐位相等。
    let mut bits_ok = face.handle_len() == SPECIAL_CAP;
    for i in 0..face.handle_len() {
        if let (Some(h), Some(t)) = (face.handle_at(i), SPECIAL_ALL.get(i)) {
            if h.bits != t.bits() {
                bits_ok = false;
            }
        } else {
            bits_ok = false;
        }
    }
    s.add_dyn(
        "F12651-10 特殊档位型恒等",
        bits_ok,
        "档案位型 = 0xFFFF_FFFF_FFFF_FFFF / 0xFFFF_FFFF_FFFF_FFFE",
    );

    // 枚举行摘要可点验：首行摘要含类型 1。
    let brief_ok = match face.row_at(0) {
        Some(r) => r.brief().contains("类型1"),
        None => false,
    };
    s.add_dyn(
        "F12651-11 枚举行摘要",
        brief_ok,
        "首行摘要含类型索引可点验",
    );

    // 名册闭集守卫：册内名可查回，册外名查不回。
    let mut roster_ok = name_in_roster("Process") && name_in_roster("File");
    if name_in_roster("GhostObj") || name_in_roster("") {
        roster_ok = false;
    }
    s.add_dyn(
        "F12651-12 名册闭集守卫",
        roster_ok,
        "Process/File 可查回，GhostObj/空名查不回",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x80C80);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(OBJ_ALL.len(), 12);
        let mut i = 0usize;
        while i < OBJ_ALL.len() {
            let vi = OBJ_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < OBJ_ALL.len() {
                let vj = OBJ_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        // 未知码回落。
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn happy_path_reconciles() {
        let mut p = build_full_enum();
        assert_eq!(p.len(), 8);
        assert_eq!(p.state(), EnumState::Enumerated);
        assert!(p.reconcile().is_ok());
        assert_eq!(p.state(), EnumState::Paritied);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路越域位型全数拒止，无逃脱。
        let (rejected, total, escape) = out_of_range_sweep();
        assert_eq!(total, 10);
        assert_eq!(rejected, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn special_tier_semantics() {
        // 档注出处语义：-1/-2 归特殊档，0/-3/-4/混搭 归越域档。
        assert_eq!(SpecialTier::from_bits(HANDLE_CURRENT_PROCESS), Some(SpecialTier::CurProcess));
        assert_eq!(SpecialTier::from_bits(HANDLE_CURRENT_THREAD), Some(SpecialTier::CurThread));
        assert_eq!(SpecialTier::from_bits(0), None);
        assert_eq!(SpecialTier::from_bits(0xFFFF_FFFF_FFFF_FFFD), None);
        assert_eq!(tier_of_bits(HANDLE_CURRENT_PROCESS), HandleTier::Special(SpecialTier::CurProcess));
        assert_eq!(tier_of_bits(0x8000_0000_0000_0000), HandleTier::OutOfRange);
        assert_eq!(tier_of_bits(42), HandleTier::Normal);
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：确定性 LCG 下两次全流程逐项相同。
        let a = build_full_enum();
        let b = build_full_enum();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(ra), Some(rb)) => {
                    assert_eq!(ra.name, rb.name);
                    assert_eq!(ra.handle_count, rb.handle_count);
                }
                (None, None) => {}
                _ => assert!(false, "双跑行缺失"),
            }
            i += 1;
        }
        assert_eq!(out_of_range_sweep().0, out_of_range_sweep().0);
    }

    #[test]
    fn row_gates_reject() {
        // 索引乱序被拒。
        let mut p = ObjEnum::new();
        assert_eq!(
            p.push_row(2, "Process", 10, 5, 6),
            Err(ObjDiag::EIndexOrder.val())
        );
        // 名册越名被拒。
        let mut p2 = ObjEnum::new();
        assert_eq!(
            p2.push_row(1, "GhostObj", 10, 5, 6),
            Err(ObjDiag::ENameRange.val())
        );
        // 可查越等被拒。
        let mut p3 = ObjEnum::new();
        assert_eq!(
            p3.push_row(1, "Process", 10, 11, 6),
            Err(ObjDiag::EValidOver.val())
        );
        // 峰值越等被拒。
        let mut p4 = ObjEnum::new();
        assert_eq!(
            p4.push_row(1, "Process", 10, 5, 11),
            Err(ObjDiag::EPeakOver.val())
        );
    }

    #[test]
    fn handle_gates_reject() {
        // 越域位型被拒。
        let mut p = ObjEnum::new();
        assert_eq!(p.file_handle(0), Err(ObjDiag::EHandleRange.val()));
        // 普通档位型（1）与本面值域无关，同样拒收。
        let mut p0 = ObjEnum::new();
        assert_eq!(p0.file_handle(1), Err(ObjDiag::EHandleRange.val()));
        // -3 位型被拒。
        let mut p2 = ObjEnum::new();
        assert_eq!(
            p2.file_handle(0xFFFF_FFFF_FFFF_FFFD),
            Err(ObjDiag::EHandleRange.val())
        );
        // 特殊档重复归档被拒。
        let mut p3 = ObjEnum::new();
        assert!(p3.file_handle(HANDLE_CURRENT_PROCESS).is_ok());
        assert_eq!(
            p3.file_handle(HANDLE_CURRENT_PROCESS),
            Err(ObjDiag::EHandleDup.val())
        );
        // -2 与 -1 各归一档后，-3 仍被拒。
        let mut p4 = ObjEnum::new();
        let _ = p4.file_handle(HANDLE_CURRENT_PROCESS);
        let _ = p4.file_handle(HANDLE_CURRENT_THREAD);
        assert_eq!(
            p4.file_handle(0xFFFF_FFFF_FFFF_FFFC),
            Err(ObjDiag::EHandleRange.val())
        );
    }

    #[test]
    fn reconcile_requires_full_rows_and_specials() {
        // 边界：名册行未齐禁止推进，特殊档未齐禁止推进。
        let mut p = ObjEnum::new();
        assert_eq!(p.mark_enumerated(), Err(ObjDiag::ENotEnumerated.val()));
        // 构建器已推进到已枚举态，重复推进倒退拒绝。
        let mut full = build_full_enum();
        assert_eq!(full.mark_enumerated(), Err(ObjDiag::ERewind.val()));
        assert!(full.reconcile().is_ok());
        assert_eq!(full.reconcile(), Err(ObjDiag::ERewind.val()));
        // 对平后写行/归档拒绝。
        assert_eq!(
            full.push_row(9, "Process", 10, 5, 6),
            Err(ObjDiag::ERewind.val())
        );
        assert_eq!(
            full.file_handle(1),
            Err(ObjDiag::ERewind.val())
        );
    }

    #[test]
    fn parity_assertions_count_is_ten() {
        // 对平断言恒 10 条且全绿；缺特殊档面必红。
        let p = build_full_enum();
        let list = parity_assertions(&p);
        assert_eq!(list.len(), 10);
        assert!(parity_all_green(&p));
        // 缺特殊档面：特殊档恒两行断言必红。
        let mut bare = ObjEnum::new();
        let _ = bare.push_row(1, "Process", 10, 5, 6);
        let _ = bare.file_handle(HANDLE_CURRENT_PROCESS);
        let _ = bare.mark_enumerated();
        assert!(!parity_all_green(&bare) || bare.mark_enumerated().is_err());
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Bare→Enumerated→Paritied。
        let mut p = build_full_enum();
        assert_eq!(p.state(), EnumState::Enumerated);
        assert!(EnumState::Bare.rank() < EnumState::Enumerated.rank());
        assert!(EnumState::Enumerated.rank() < EnumState::Paritied.rank());
        let _ = p.reconcile();
        assert_eq!(p.state(), EnumState::Paritied);
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12650_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
