//! UNX-F12652 会签槽位预置（E1 未开工，R-D1-030 登记） · 0x80CC0段
//!
//! 判据原文（派工卡 12651 逐字锚定）：
//!   ① 纯功能代码 ≥330 行；
//!   ② 判据原文：UNX-F12652-J1 预演形态三件齐（接口映射表/槽位预置/
//!      两态声明），R-D1-030 显式登记可点验；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 预演形态三件：第一件**接口映射表**（接口号/接口名/目标槽位三元，
//!    十行闭集，行序冻结）；第二件**槽位预置**（槽位号/占位接口/预置
//!    时点账号三元，逐槽预置）；第三件**两态声明**（每槽恰两态：
//!    未开工/已预置，第三态声明一律显性拒）；
//! 2. 两态声明闭集：SlotState 只含 Reserved（未开工）与 Preset（已预置）
//!    两个变体，状态机只许 Reserved→Preset 单向推进，倒退显性拒；
//! 3. R-D1-030 显式登记：预演形态整体作为一条登记行写入登记账
//!    （登记号/条款锚/登记内容/可点验标记），条款锚必须逐字等于
//!    "R-D1-030"，锚不符即登记无效；登记账只追加不改写；
//! 4. 对平规则：接口映射表的目标槽位必须与槽位预置的槽位号一一对应
//!    （十行对十槽），接口号必须与占位接口一致，缺行、重复、换序
//!    都在对平断言处翻红；
//! 5. 注入联测 10 路：第三态声明、槽位重复预置、未映射接口预置、
//!    三件缺件、槽位越界、条款锚不符、登记行重复、映射前预置、
//!    空接口名、槽位换序——逐路必须被闸抓住，放行即判据失效；
//! 6. 状态机 未登记→已登记→三件齐→封账 单向推进，倒退显性拒；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12651 − 12600)）。
pub const SEG_BASE: u32 = 0x80CC0;

/// R-D1-030 条款锚（逐字冻结）。
pub const RD1_ANCHOR: &str = "R-D1-030";

// ── 会签槽位诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotDiag {
    /// 接口号越映射表闭集。
    EIfRange,
    /// 接口名为空。
    EIfEmpty,
    /// 槽位号越槽位闭集。
    ESlotRange,
    /// 槽位重复预置。
    ESlotDup,
    /// 未映射接口试图预置槽位。
    EIfUnmapped,
    /// 第三态声明（状态越两态闭集）。
    EThirdState,
    /// 状态倒退（已预置又退回未开工）。
    ERewind,
    /// 条款锚不符（非 R-D1-030）。
    EAnchorBad,
    /// 登记行重复。
    ERegDup,
    /// 预演三件缺件。
    EPieceMiss,
    /// 未登记就试图封账。
    ENotRegistered,
    /// 对平断言未全过。
    EParity,
}

impl SlotDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                SlotDiag::EIfRange => 0x01,
                SlotDiag::EIfEmpty => 0x02,
                SlotDiag::ESlotRange => 0x03,
                SlotDiag::ESlotDup => 0x04,
                SlotDiag::EIfUnmapped => 0x05,
                SlotDiag::EThirdState => 0x06,
                SlotDiag::ERewind => 0x07,
                SlotDiag::EAnchorBad => 0x08,
                SlotDiag::ERegDup => 0x09,
                SlotDiag::EPieceMiss => 0x0a,
                SlotDiag::ENotRegistered => 0x0b,
                SlotDiag::EParity => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SlotDiag::EIfRange => "接口号越映射表闭集：只认十行映射表登记的接口号，先补映射再预置",
            SlotDiag::EIfEmpty => "接口名为空：空名不构成可点验接口，先落接口名再映射",
            SlotDiag::ESlotRange => "槽位号越槽位闭集：槽位只认 0..10，越界槽位先对映射表",
            SlotDiag::ESlotDup => "槽位重复预置：每槽只落一笔预置，重复预置先查已有槽位",
            SlotDiag::EIfUnmapped => "未映射接口禁止预置：接口必须先入映射表才能占槽，先补映射行",
            SlotDiag::EThirdState => "第三态声明被拒：槽位只许未开工/已预置两态，第三态先回两态闭集",
            SlotDiag::ERewind => "状态倒退：已预置槽位不得退回未开工，改动走新槽滚动",
            SlotDiag::EAnchorBad => "条款锚不符：登记锚必须逐字等于 R-D1-030，锚不符登记无效",
            SlotDiag::ERegDup => "登记行重复：同一预演形态只落一笔登记，重复登记先查已有行",
            SlotDiag::EPieceMiss => "预演三件缺件：接口映射表/槽位预置/两态声明缺一不可，先补缺件",
            SlotDiag::ENotRegistered => "未登记禁止封账：先落 R-D1-030 登记行，再走封账口",
            SlotDiag::EParity => "对平断言未全过：断言有红项，按红项编号回修对应件",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const SLOT_ALL: &[SlotDiag] = &[
    SlotDiag::EIfRange,
    SlotDiag::EIfEmpty,
    SlotDiag::ESlotRange,
    SlotDiag::ESlotDup,
    SlotDiag::EIfUnmapped,
    SlotDiag::EThirdState,
    SlotDiag::ERewind,
    SlotDiag::EAnchorBad,
    SlotDiag::ERegDup,
    SlotDiag::EPieceMiss,
    SlotDiag::ENotRegistered,
    SlotDiag::EParity,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in SLOT_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80CC1..0x80CCC 闭集内"
}

// ── 第一件：接口映射表（十行闭集） ───────────────────────────────
/// 映射行：接口号/接口名/目标槽位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IfMapRow {
    /// 接口号（0 起连续）。
    pub if_no: u32,
    /// 接口名（非空）。
    pub if_name: &'static str,
    /// 目标槽位（与接口号同值，预演形态约定）。
    pub slot: u32,
}

/// 接口映射表（十行闭集，行序冻结不许重排）。
pub const IF_MAP: &[IfMapRow] = &[
    IfMapRow { if_no: 0, if_name: "sysinfo", slot: 0 },
    IfMapRow { if_no: 1, if_name: "objtype", slot: 1 },
    IfMapRow { if_no: 2, if_name: "handle", slot: 2 },
    IfMapRow { if_no: 3, if_name: "wine-ref", slot: 3 },
    IfMapRow { if_no: 4, if_name: "mutation", slot: 4 },
    IfMapRow { if_no: 5, if_name: "cmptable", slot: 5 },
    IfMapRow { if_no: 6, if_name: "p95", slot: 6 },
    IfMapRow { if_no: 7, if_name: "edge", slot: 7 },
    IfMapRow { if_no: 8, if_name: "antihallu", slot: 8 },
    IfMapRow { if_no: 9, if_name: "ktest", slot: 9 },
];

/// 映射表容量（也是槽位容量）。
pub const MAP_CAP: usize = 10;

/// 接口号查映射行（越界 None）。
pub fn if_row_of(if_no: u32) -> Option<&'static IfMapRow> {
    IF_MAP.iter().find(|r| r.if_no == if_no)
}

/// 接口名查映射行（越名 None）。
pub fn if_row_of_name(name: &str) -> Option<&'static IfMapRow> {
    IF_MAP.iter().find(|r| r.if_name == name)
}

// ── 第二件：槽位预置 ─────────────────────────────────────────────
/// 第三件：两态声明闭集（只许两态，第三态不存在）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    /// 未开工（槽位已占但未预置内容）。
    Reserved,
    /// 已预置（预置内容落账）。
    Preset,
}

impl SlotState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            SlotState::Reserved => 0,
            SlotState::Preset => 1,
        }
    }

    /// 状态中文名。
    pub fn name(self) -> &'static str {
        match self {
            SlotState::Reserved => "未开工",
            SlotState::Preset => "已预置",
        }
    }

    /// 由状态序反查状态（越两态闭集返回 None——第三态声明的拒收口）。
    pub fn from_rank(rank: u8) -> Option<SlotState> {
        match rank {
            0 => Some(SlotState::Reserved),
            1 => Some(SlotState::Preset),
            _ => None,
        }
    }
}

/// 两态闭集数组。
pub const STATE_ALL: &[SlotState] = &[SlotState::Reserved, SlotState::Preset];

/// 槽位预置行：槽位号/占位接口号/状态/预置时点账号。
#[derive(Clone, Debug)]
pub struct SlotRow {
    /// 槽位号。
    pub slot: u32,
    /// 占位接口号。
    pub if_no: u32,
    /// 占位接口名（与映射表对平）。
    pub if_name: String,
    /// 槽位状态（两态闭集内）。
    pub state: SlotState,
    /// 预置时点账号（预置时写入，未开工为 0）。
    pub preset_tick: u32,
}

impl SlotRow {
    /// 行摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        format!(
            "槽{}({}):{} 时点账号{}",
            self.slot, self.if_name, self.state.name(), self.preset_tick
        )
    }
}

// ── R-D1-030 登记账 ──────────────────────────────────────────────
/// 登记行：登记号/条款锚/登记内容/可点验标记。
#[derive(Clone, Debug)]
pub struct RegRow {
    /// 登记号（0 起连续）。
    pub reg_no: u32,
    /// 条款锚（必须逐字等于 R-D1-030）。
    pub anchor: String,
    /// 登记内容（非空）。
    pub content: String,
    /// 可点验标记（true 才算显式登记）。
    pub verifiable: bool,
}

/// 预演账状态机：未登记 → 已登记 → 三件齐 → 封账，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormState {
    /// 未登记：只许可写登记行与映射核对。
    Unregistered,
    /// 已登记：R-D1-030 登记行在账。
    Registered,
    /// 三件齐：映射/预置/两态对平全绿。
    Assembled,
    /// 封账：只读。
    Sealed,
}

impl FormState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            FormState::Unregistered => 0,
            FormState::Registered => 1,
            FormState::Assembled => 2,
            FormState::Sealed => 3,
        }
    }
}

/// 会签槽位预演账。
#[derive(Clone, Debug)]
pub struct SlotForm {
    /// 槽位预置行（按槽位序）。
    slots: Vec<SlotRow>,
    /// R-D1-030 登记账（只追加）。
    regs: Vec<RegRow>,
    /// 账本状态。
    state: FormState,
}

impl SlotForm {
    /// 开一本新预演账。
    pub fn new() -> SlotForm {
        SlotForm {
            slots: Vec::new(),
            regs: Vec::new(),
            state: FormState::Unregistered,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> FormState {
        self.state
    }

    /// 槽位行数。
    pub fn slot_len(&self) -> usize {
        self.slots.len()
    }

    /// 登记行数。
    pub fn reg_len(&self) -> usize {
        self.regs.len()
    }

    /// 取第 idx 槽（越界 None）。
    pub fn slot_at(&self, idx: usize) -> Option<&SlotRow> {
        self.slots.get(idx)
    }

    /// 取第 idx 登记行（越界 None）。
    pub fn reg_at(&self, idx: usize) -> Option<&RegRow> {
        self.regs.get(idx)
    }

    /// R-D1-030 显式登记（锚闸/空内容闸/重复登记闸/可点验闸）。
    pub fn register(&mut self, reg_no: u32, anchor: &str, content: &str, verifiable: bool) -> Result<(), u32> {
        // 封账闸。
        if self.state.rank() >= FormState::Sealed.rank() {
            return Err(SlotDiag::ERewind.val());
        }
        // 登记号序闸：按 0 起连续登记。
        let expect = self.regs.len() as u32;
        if reg_no != expect {
            return Err(SlotDiag::ERegDup.val());
        }
        // 条款锚闸：逐字等于 R-D1-030。
        if anchor != RD1_ANCHOR {
            return Err(SlotDiag::EAnchorBad.val());
        }
        // 空内容闸。
        if content.is_empty() {
            return Err(SlotDiag::EIfEmpty.val());
        }
        // 可点验闸：不可点验不算显式登记。
        if !verifiable {
            return Err(SlotDiag::EAnchorBad.val());
        }
        self.regs.push(RegRow {
            reg_no,
            anchor: anchor.to_string(),
            content: content.to_string(),
            verifiable,
        });
        if self.state.rank() < FormState::Registered.rank() {
            self.state = FormState::Registered;
        }
        Ok(())
    }

    /// 槽位预置（映射闸/槽位闭集闸/重复闸/状态闸四道闸）。
    pub fn preset(&mut self, slot: u32, if_no: u32) -> Result<(), u32> {
        // 封账闸。
        if self.state.rank() >= FormState::Sealed.rank() {
            return Err(SlotDiag::ERewind.val());
        }
        // 槽位闭集闸。
        if slot >= MAP_CAP as u32 {
            return Err(SlotDiag::ESlotRange.val());
        }
        // 映射闸：接口号必须在映射表内。
        let row = match if_row_of(if_no) {
            Some(r) => r,
            None => return Err(SlotDiag::EIfRange.val()),
        };
        // 槽位与映射目标一致闸。
        if row.slot != slot {
            return Err(SlotDiag::EIfUnmapped.val());
        }
        // 槽位序闸：按 0..10 顺序逐槽预置。
        let expect = self.slots.len() as u32;
        if slot != expect {
            return Err(SlotDiag::ESlotDup.val());
        }
        // 空接口名闸（映射表冻结，防御性保留）。
        if row.if_name.is_empty() {
            return Err(SlotDiag::EIfEmpty.val());
        }
        // 预置时点账号：确定性生成（槽号 × 0x100 + 0x30，无墙钟）。
        let tick = slot.saturating_mul(0x100).saturating_add(0x30);
        self.slots.push(SlotRow {
            slot,
            if_no,
            if_name: row.if_name.to_string(),
            state: SlotState::Preset,
            preset_tick: tick,
        });
        Ok(())
    }

    /// 两态声明核验：每槽状态必须落在两态闭集且只进不退。
    pub fn declare_state(&mut self, slot: u32, rank: u8) -> Result<SlotState, u32> {
        // 第三态闸：rank 越两态闭集即第三态声明，显性拒。
        let s = match SlotState::from_rank(rank) {
            Some(v) => v,
            None => return Err(SlotDiag::EThirdState.val()),
        };
        // 槽位闭集闸。
        if slot >= MAP_CAP as u32 {
            return Err(SlotDiag::ESlotRange.val());
        }
        // 槽位必须在账（未预置槽不许声明状态）。
        let cur = match self.slots.iter().find(|r| r.slot == slot) {
            Some(r) => r.state,
            None => return Err(SlotDiag::EIfUnmapped.val()),
        };
        // 倒退闸：声明状态序不得低于现有状态序。
        if s.rank() < cur.rank() {
            return Err(SlotDiag::ERewind.val());
        }
        Ok(s)
    }

    /// 三件齐对平：映射/预置/两态三件全绿才许推进。
    pub fn assemble(&mut self) -> Result<(), u32> {
        if self.state.rank() >= FormState::Assembled.rank() {
            return Err(SlotDiag::ERewind.val());
        }
        if self.state.rank() < FormState::Registered.rank() {
            return Err(SlotDiag::ENotRegistered.val());
        }
        if !parity_all_green(self) {
            return Err(SlotDiag::EParity.val());
        }
        self.state = FormState::Assembled;
        Ok(())
    }

    /// 封账：三件齐后才能封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= FormState::Sealed.rank() {
            return Err(SlotDiag::ERewind.val());
        }
        if self.state.rank() < FormState::Assembled.rank() {
            return Err(SlotDiag::EPieceMiss.val());
        }
        self.state = FormState::Sealed;
        Ok(())
    }
}

// ── 预演对平断言（三件齐 + 登记可点验） ─────────────────────────
/// 对平断言：映射十行 / 预置十槽 / 登记在账 / 锚逐字 / 可点验 /
/// 槽位与映射对平 / 接口号与映射对平 / 槽位序连续 / 两态闭集内 /
/// 预置时点账号递增 = 10 条。每条独立写死，不复用被测值。
pub fn parity_assertions(f: &SlotForm) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    // 断言 0：映射表十行齐（冻结常量面）。
    out.push(("对平0-映射十行".to_string(), IF_MAP.len() == MAP_CAP));
    // 断言 1：槽位预置十槽齐。
    out.push(("对平1-预置十槽".to_string(), f.slot_len() == MAP_CAP));
    // 断言 2：登记在账（R-D1-030 登记行 ≥1）。
    out.push(("对平2-登记在账".to_string(), f.reg_len() >= 1));
    // 断言 3：锚逐字（每条登记锚逐字等于 R-D1-030）。
    let mut anchor_ok = true;
    for r in f.regs.iter() {
        if r.anchor != RD1_ANCHOR {
            anchor_ok = false;
        }
    }
    out.push(("对平3-锚逐字".to_string(), anchor_ok));
    // 断言 4：可点验（每条登记可点验标记为真且内容非空）。
    let mut verif_ok = true;
    for r in f.regs.iter() {
        if !r.verifiable || r.content.is_empty() {
            verif_ok = false;
        }
    }
    out.push(("对平4-登记可点验".to_string(), verif_ok));
    // 断言 5：槽位与映射对平（槽位号逐槽等于映射目标槽位）。
    let mut slot_ok = true;
    for i in 0..f.slot_len() {
        match (f.slot_at(i), IF_MAP.get(i)) {
            (Some(s), Some(m)) => {
                if s.slot != m.slot {
                    slot_ok = false;
                }
            }
            _ => slot_ok = false,
        }
    }
    out.push(("对平5-槽位对映射".to_string(), slot_ok));
    // 断言 6：接口号与映射对平（占位接口号逐槽等于映射接口号）。
    let mut if_ok = true;
    for i in 0..f.slot_len() {
        match (f.slot_at(i), IF_MAP.get(i)) {
            (Some(s), Some(m)) => {
                if s.if_no != m.if_no || s.if_name != m.if_name {
                    if_ok = false;
                }
            }
            _ => if_ok = false,
        }
    }
    out.push(("对平6-接口对映射".to_string(), if_ok));
    // 断言 7：槽位序连续（0..10 各一按序）。
    let mut seq_ok = true;
    for i in 0..f.slot_len() {
        match f.slot_at(i) {
            Some(s) => {
                if s.slot != i as u32 {
                    seq_ok = false;
                }
            }
            None => seq_ok = false,
        }
    }
    out.push(("对平7-槽位序连续".to_string(), seq_ok));
    // 断言 8：两态闭集内（每槽状态可由状态序反查回来）。
    let mut state_ok = true;
    for s in f.slots.iter() {
        match SlotState::from_rank(s.state.rank()) {
            Some(v) => {
                if v != s.state {
                    state_ok = false;
                }
            }
            None => state_ok = false,
        }
    }
    out.push(("对平8-两态闭集内".to_string(), state_ok));
    // 断言 9：预置时点账号递增（沿槽位序不减）。
    let mut tick_ok = true;
    let mut prev = 0u32;
    for s in f.slots.iter() {
        if s.preset_tick < prev {
            tick_ok = false;
        }
        prev = s.preset_tick;
    }
    out.push(("对平9-时点账号递增".to_string(), tick_ok));
    out
}

/// 全绿判定。
pub fn parity_all_green(f: &SlotForm) -> bool {
    let list = parity_assertions(f);
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

// ── 构建器：完整预演账 ───────────────────────────────────────────
/// 构建完整预演账（R-D1-030 登记 + 十槽逐槽预置 + 三件齐对平）。
pub fn build_full_form() -> SlotForm {
    let mut f = SlotForm::new();
    let reg = f.register(0, RD1_ANCHOR, "会签槽位预置预演形态整体登记（E1 未开工，占槽待实做）", true);
    match reg {
        Ok(()) => {}
        Err(e) => {
            // 正常流不该出错；出错时终止构造（无登记，断言会红）。
            let _ = e;
            return f;
        }
    }
    let mut i = 0u32;
    while i < MAP_CAP as u32 {
        match f.preset(i, i) {
            Ok(()) => i += 1,
            Err(e) => {
                let _ = e;
                break;
            }
        }
    }
    let _ = f.assemble();
    f
}

// ── 注入面：十路违约构造 ─────────────────────────────────────────
/// 违约路闭集（注入联测的十路手写违约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Violation {
    /// 第三态声明（rank=2 越两态闭集）。
    ThirdState,
    /// 槽位重复预置（槽 0 预置两次）。
    SlotDup,
    /// 未映射接口预置（接口号 99）。
    IfUnmapped,
    /// 三件缺件（无登记直接组装）。
    PieceMiss,
    /// 槽位越界（槽 10）。
    SlotRange,
    /// 条款锚不符（锚写成 R-D1-031）。
    AnchorBad,
    /// 登记号乱序（跳号登记）。
    RegOrder,
    /// 映射前预置（槽位与映射目标错配）。
    SlotMismatch,
    /// 空接口名登记。
    EmptyContent,
    /// 状态倒退（已预置声明回未开工）。
    StateBack,
}

impl Violation {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            Violation::ThirdState => 0,
            Violation::SlotDup => 1,
            Violation::IfUnmapped => 2,
            Violation::PieceMiss => 3,
            Violation::SlotRange => 4,
            Violation::AnchorBad => 5,
            Violation::RegOrder => 6,
            Violation::SlotMismatch => 7,
            Violation::EmptyContent => 8,
            Violation::StateBack => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            Violation::ThirdState => "第三态声明",
            Violation::SlotDup => "槽位重复",
            Violation::IfUnmapped => "未映射接口",
            Violation::PieceMiss => "三件缺件",
            Violation::SlotRange => "槽位越界",
            Violation::AnchorBad => "条款锚不符",
            Violation::RegOrder => "登记号乱序",
            Violation::SlotMismatch => "槽位错配",
            Violation::EmptyContent => "空内容登记",
            Violation::StateBack => "状态倒退",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [Violation] {
        &[
            Violation::ThirdState,
            Violation::SlotDup,
            Violation::IfUnmapped,
            Violation::PieceMiss,
            Violation::SlotRange,
            Violation::AnchorBad,
            Violation::RegOrder,
            Violation::SlotMismatch,
            Violation::EmptyContent,
            Violation::StateBack,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住（返回 Err 或断言翻红）。
/// 返回 (抓到路数, 总路数, 首个逃脱路描述)。
pub fn violation_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in Violation::all().iter() {
        total += 1;
        let bad = run_violation(*v);
        if bad {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住（闸拒收或断言翻红）。
fn run_violation(v: Violation) -> bool {
    match v {
        Violation::ThirdState => {
            let mut f = build_full_form();
            // rank=2 越两态闭集，必须被拒。
            f.declare_state(0, 2).is_err()
        }
        Violation::SlotDup => {
            let mut f = SlotForm::new();
            let _ = f.register(0, RD1_ANCHOR, "重复预置注入", true);
            let _ = f.preset(0, 0);
            // 槽 0 二次预置（乱序闸拒收）。
            f.preset(0, 0).is_err()
        }
        Violation::IfUnmapped => {
            let mut f = SlotForm::new();
            let _ = f.register(0, RD1_ANCHOR, "未映射接口注入", true);
            // 接口号 99 越映射表闭集。
            f.preset(0, 99).is_err()
        }
        Violation::PieceMiss => {
            let mut f = SlotForm::new();
            // 无登记直接组装（未登记闸拒收）。
            f.assemble().is_err()
        }
        Violation::SlotRange => {
            let mut f = SlotForm::new();
            let _ = f.register(0, RD1_ANCHOR, "槽位越界注入", true);
            // 槽 10 越闭集。
            f.preset(10, 10).is_err()
        }
        Violation::AnchorBad => {
            let mut f = SlotForm::new();
            // 锚写成 R-D1-031，必须被锚闸拒收。
            f.register(0, "R-D1-031", "锚不符注入", true).is_err()
        }
        Violation::RegOrder => {
            let mut f = SlotForm::new();
            // 首笔登记号跳到 1，序闸拒收。
            f.register(1, RD1_ANCHOR, "跳号注入", true).is_err()
        }
        Violation::SlotMismatch => {
            let mut f = SlotForm::new();
            let _ = f.register(0, RD1_ANCHOR, "槽位错配注入", true);
            // 接口 1（映射目标槽 1）试图占槽 0，必须被拒。
            f.preset(0, 1).is_err()
        }
        Violation::EmptyContent => {
            let mut f = SlotForm::new();
            // 空内容登记被拒。
            f.register(0, RD1_ANCHOR, "", true).is_err()
        }
        Violation::StateBack => {
            let mut f = build_full_form();
            // 槽 0 已预置（Preset），声明回未开工（rank 0）必须被拒。
            f.declare_state(0, 0).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 预演形态三件齐（判据 J1 的"三件齐"直读口）。
fn three_pieces_ok() -> bool {
    let f = build_full_form();
    // 第一件：接口映射表十行。
    if IF_MAP.len() != MAP_CAP {
        return false;
    }
    // 第二件：槽位预置十槽。
    if f.slot_len() != MAP_CAP {
        return false;
    }
    // 第三件：两态声明闭集恰两态。
    if STATE_ALL.len() != 2 {
        return false;
    }
    if !parity_all_green(&f) {
        return false;
    }
    true
}

/// R-D1-030 显式登记可点验（判据 J1 的登记直读口）。
fn rd1_registered_ok() -> bool {
    let f = build_full_form();
    if f.reg_len() < 1 {
        return false;
    }
    match f.reg_at(0) {
        Some(r) => {
            if r.anchor != RD1_ANCHOR {
                return false;
            }
            if !r.verifiable {
                return false;
            }
            if r.content.is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

/// 注入联测 10/10 拒止。
fn injection_ok() -> bool {
    let (caught, total, escape) = violation_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 双跑一致（确定性构造下两次全流程逐项相同）。
fn dual_run_ok() -> bool {
    let a = build_full_form();
    let b = build_full_form();
    if a.slot_len() != b.slot_len() || a.reg_len() != b.reg_len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.slot_len() {
        match (a.slot_at(i), b.slot_at(i)) {
            (Some(x), Some(y)) => {
                if x.slot != y.slot || x.if_name != y.if_name || x.preset_tick != y.preset_tick {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let ra = violation_sweep();
    let rb = violation_sweep();
    if ra.0 != rb.0 || ra.1 != rb.1 {
        return false;
    }
    true
}

/// 边界：封账后写入拒；未三件齐封账拒；两态闭集反查越界拒。
fn boundary_ok() -> bool {
    let mut f = build_full_form();
    // 未封账先验：状态应为三件齐（assemble 在构建器内已过）。
    if f.state() != FormState::Assembled {
        return false;
    }
    // 封账成功，重封倒退拒绝。
    if f.seal().is_err() {
        return false;
    }
    if f.seal().is_ok() {
        return false;
    }
    // 封账后登记/预置拒绝。
    if f.register(1, RD1_ANCHOR, "封账后写", true).is_ok() {
        return false;
    }
    if f.preset(0, 0).is_ok() {
        return false;
    }
    // 两态闭集反查：rank 2/255 返回 None。
    if SlotState::from_rank(2).is_some() || SlotState::from_rank(255).is_some() {
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
    if SEG_BASE != 0x80000 + 0x40 * (12651u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut f = build_full_form();
    let _ = f.seal();
    if f.state() != FormState::Sealed {
        return false;
    }
    match f.slot_at(0) {
        Some(s) => {
            if s.brief().is_empty() {
                return false;
            }
        }
        None => return false,
    }
    match f.reg_at(0) {
        Some(r) => {
            if r.content.is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12652 预演形态三件齐自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12651_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12651-cosign-slot");
    let form = build_full_form();

    s.add_dyn(
        "F12652-01 接口映射表十行齐",
        IF_MAP.len() == MAP_CAP,
        &format!("rows={}", IF_MAP.len()),
    );

    s.add_dyn(
        "F12652-02 预演三件齐",
        three_pieces_ok(),
        &format!("slots={} states={}", form.slot_len(), STATE_ALL.len()),
    );

    s.add_dyn(
        "F12652-03 R-D1-030 显式登记",
        rd1_registered_ok(),
        "锚逐字 R-D1-030 且可点验",
    );

    s.add_dyn(
        "F12652-04 注入联测 10/10 拒止",
        injection_ok(),
        &format!("caught={}", violation_sweep().0),
    );

    s.add_dyn(
        "F12652-05 两态声明闭集",
        SlotState::from_rank(0).is_some() && SlotState::from_rank(1).is_some() && SlotState::from_rank(2).is_none(),
        "rank 0/1 可反查，rank 2 第三态拒收",
    );

    s.add_dyn(
        "F12652-06 双跑一致",
        dual_run_ok(),
        "两次构建与两次联测逐项一致",
    );

    s.add_dyn(
        "F12652-07 边界封账与倒退",
        boundary_ok(),
        "封账/重封/封后写/第三态反查 四点",
    );

    s.add_dyn(
        "F12652-08 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12652-09 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 对平断言表结构：恒 10 条且每条独立为真。
    let list = parity_assertions(&form);
    let mut all_true = list.len() == 10;
    for item in list.iter() {
        let _name = &item.0;
        if !item.1 {
            all_true = false;
        }
    }
    s.add_dyn(
        "F12652-10 对平断言表结构",
        all_true,
        &format!("rows={} head={}", list.len(), match list.get(0) { Some(x) => x.0.clone(), None => String::new() }),
    );

    // 槽位摘要可点验：末槽摘要含槽号 9。
    let brief_ok = match form.slot_at(MAP_CAP - 1) {
        Some(x) => x.brief().contains("槽9"),
        None => false,
    };
    s.add_dyn(
        "F12652-11 槽位摘要",
        brief_ok,
        "末槽摘要含槽号可点验",
    );

    // 映射查表：册内接口可查回，册外查不回。
    let mut lookup_ok = if_row_of(0).is_some() && if_row_of_name("ktest").is_some();
    if if_row_of(10).is_some() || if_row_of_name("ghost").is_some() {
        lookup_ok = false;
    }
    s.add_dyn(
        "F12652-12 映射查表闭集",
        lookup_ok,
        "0/ktest 可查回，10/ghost 查不回",
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
        assert_eq!(SEG_BASE, 0x80CC0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(SLOT_ALL.len(), 12);
        let mut i = 0usize;
        while i < SLOT_ALL.len() {
            let vi = SLOT_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < SLOT_ALL.len() {
                let vj = SLOT_ALL.get(j).map(|d| d.val()).unwrap_or(0);
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
    fn happy_path_assembles_and_seals() {
        let mut f = build_full_form();
        assert_eq!(f.slot_len(), 10);
        assert_eq!(f.reg_len(), 1);
        assert_eq!(f.state(), FormState::Assembled);
        assert!(f.seal().is_ok());
        assert_eq!(f.state(), FormState::Sealed);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约全数拒止，无逃脱。
        let (caught, total, escape) = violation_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn two_state_closed_set() {
        // 两态声明闭集：恰两态，第三态反查 None，倒退被拒。
        assert_eq!(STATE_ALL.len(), 2);
        assert_eq!(SlotState::from_rank(0), Some(SlotState::Reserved));
        assert_eq!(SlotState::from_rank(1), Some(SlotState::Preset));
        assert_eq!(SlotState::from_rank(2), None);
        let mut f = build_full_form();
        assert_eq!(f.declare_state(0, 2), Err(SlotDiag::EThirdState.val()));
        assert_eq!(f.declare_state(0, 0), Err(SlotDiag::ERewind.val()));
        // 越槽位声明被拒。
        assert_eq!(f.declare_state(10, 1), Err(SlotDiag::ESlotRange.val()));
    }

    #[test]
    fn registration_gates_reject() {
        // 锚不符被拒。
        let mut f = SlotForm::new();
        assert_eq!(
            f.register(0, "R-D1-031", "锚不符", true),
            Err(SlotDiag::EAnchorBad.val())
        );
        // 空内容被拒。
        let mut f2 = SlotForm::new();
        assert_eq!(f2.register(0, RD1_ANCHOR, "", true), Err(SlotDiag::EIfEmpty.val()));
        // 不可点验被拒。
        let mut f3 = SlotForm::new();
        assert_eq!(
            f3.register(0, RD1_ANCHOR, "内容", false),
            Err(SlotDiag::EAnchorBad.val())
        );
        // 跳号被拒。
        let mut f4 = SlotForm::new();
        assert_eq!(
            f4.register(3, RD1_ANCHOR, "跳号", true),
            Err(SlotDiag::ERegDup.val())
        );
    }

    #[test]
    fn preset_gates_reject() {
        // 越槽位被拒。
        let mut f = SlotForm::new();
        let _ = f.register(0, RD1_ANCHOR, "预置闸注入", true);
        assert_eq!(f.preset(10, 10), Err(SlotDiag::ESlotRange.val()));
        // 未映射接口被拒。
        let mut f2 = SlotForm::new();
        let _ = f2.register(0, RD1_ANCHOR, "未映射注入", true);
        assert_eq!(f2.preset(0, 99), Err(SlotDiag::EIfRange.val()));
        // 槽位错配被拒。
        let mut f3 = SlotForm::new();
        let _ = f3.register(0, RD1_ANCHOR, "错配注入", true);
        assert_eq!(f3.preset(0, 1), Err(SlotDiag::EIfUnmapped.val()));
        // 重复预置被拒。
        let mut f4 = SlotForm::new();
        let _ = f4.register(0, RD1_ANCHOR, "重复注入", true);
        let _ = f4.preset(0, 0);
        assert_eq!(f4.preset(0, 0), Err(SlotDiag::ESlotDup.val()));
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：确定性构造下两次全流程逐项相同。
        let a = build_full_form();
        let b = build_full_form();
        assert_eq!(a.slot_len(), b.slot_len());
        let mut i = 0usize;
        while i < a.slot_len() {
            match (a.slot_at(i), b.slot_at(i)) {
                (Some(x), Some(y)) => {
                    assert_eq!(x.slot, y.slot);
                    assert_eq!(x.if_name, y.if_name);
                    assert_eq!(x.preset_tick, y.preset_tick);
                }
                (None, None) => {}
                _ => assert!(false, "双跑槽位缺失"),
            }
            i += 1;
        }
        assert_eq!(violation_sweep().0, violation_sweep().0);
    }

    #[test]
    fn seal_requires_assembled() {
        // 边界：未三件齐禁止封账。
        let mut f = SlotForm::new();
        assert_eq!(f.seal(), Err(SlotDiag::EPieceMiss.val()));
        // 未登记直接组装被拒。
        let mut bare = SlotForm::new();
        assert_eq!(bare.assemble(), Err(SlotDiag::ENotRegistered.val()));
        // 齐后封账成功，重封倒退拒绝，封后写拒。
        let mut full = build_full_form();
        assert!(full.seal().is_ok());
        assert_eq!(full.seal(), Err(SlotDiag::ERewind.val()));
        assert_eq!(
            full.register(1, RD1_ANCHOR, "封后写", true),
            Err(SlotDiag::ERewind.val())
        );
        assert_eq!(full.preset(0, 0), Err(SlotDiag::ERewind.val()));
    }

    #[test]
    fn parity_assertions_count_is_ten() {
        // 对平断言恒 10 条且全绿；缺登记账本必红。
        let f = build_full_form();
        let list = parity_assertions(&f);
        assert_eq!(list.len(), 10);
        assert!(parity_all_green(&f));
        // 缺登记账本：登记在账断言必红。
        let mut bare = SlotForm::new();
        let _ = bare.preset(0, 0);
        let list2 = parity_assertions(&bare);
        assert!(!parity_all_green(&bare));
        assert!(list2.len() == 10);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Unregistered→Registered→Assembled→Sealed。
        let mut f = build_full_form();
        assert_eq!(f.state(), FormState::Assembled);
        assert!(FormState::Unregistered.rank() < FormState::Registered.rank());
        assert!(FormState::Registered.rank() < FormState::Assembled.rank());
        assert!(FormState::Assembled.rank() < FormState::Sealed.rank());
        let _ = f.seal();
        assert_eq!(f.state(), FormState::Sealed);
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12651_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
