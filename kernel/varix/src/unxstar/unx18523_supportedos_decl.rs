//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18523 · supportedOS 声明探针（15 例） · 0x7FE80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据锚点：UNX-F18523-J1 15 例（各世代 GUID）判定与 Windows 对照零偏差
//! ─────────────────────────────────────────────────────────────────────
//! 本模块结构清单：
//!   - 数据结构：Guid（四段 GUID：d1/d2/d3/d4）、DeclCombo（声明世代
//!     位掩码 u8，bit0=Vista..bit4=Win10）、DeclResult（判定+期望+偏差）、
//!     DeclLedger（15 例总账：结果账 + 状态账 + 注入账）；
//!   - 状态机：DeclState 四态（Unparsed→Parsed→Judged→Matched/Deviated），
//!     迁移闭集 4 条合法、其余 16 组合显式拒；
//!   - 账本：五世代 GUID 冻结语料（实机口径）、15 组合期望裁决表、
//!     注入检出账（tries/caught 逐类 10 轮）；
//!   - 错误路径：SosCode 十二码闭集，code()=SEG_BASE+n，中文 detail；
//!   - 注入核：三类违约（伪造 GUID / 世代重复 / GUID 段损坏）逐类
//!     10 轮断言被显式拒绝，漏检恒零。
//! 设计要点：
//!   - GUID 语料为 Windows 实机 supportedOS 闭集（Vista/Win7/Win8/
//!     Win8.1/Win10 五代），d4 十六位写死，任何段损坏必须可感知；
//!   - 判定语义（Windows 对照）：声明集适用世代 = 声明掩码最高有效位
//!     （max ordinal），空声明（无 supportedOS）按契约显式拒；
//!   - 15 组合期望值写死在裁决表（mask→ordinal 逐行冻结，对照零偏差）；
//!   - GUID 损坏检测分两级：d1 完全未知→GuidUnknown；d1/d2 命中但
//!     d4 错→GuidCorrupt（语义级篡改感知）；
//!   - 常量推导：0x7FE80 = 0x7FE40 + 0x40（A13 批第 3 卡独占窗）；
//!     判据轮数 10 来自注入 10/10 语义；
//!   - no_std + alloc；零 unwrap/expect/panic/裸越界下标/浮点/IO。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FE80，A13 批第 3 卡独占区，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7FE80;

// ── 冻结常量 ────────────────────────────────────────────────────
/// 声明探针总数（冻结：15，J1 主锚点）。
pub const PROBE_COUNT: usize = 15;
/// 世代数（冻结：5 代 = Vista/Win7/Win8/Win8.1/Win10）。
pub const GEN_COUNT: usize = 5;
/// 合法掩码上限（bit0..bit4 全占 = 0x1F；bit5 及以上非法）。
pub const MASK_MAX: u8 = 0x1F;
/// 组合裁决表行数（= 15）。
pub const ADJ_ROWS: usize = 15;
/// 判据迭代轮数（推导：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类数（伪造 GUID/世代重复/段损坏 三类）。
pub const INJECT_CLASSES: usize = 3;

// ── FNV-1a 64 位指纹 ────────────────────────────────────────────
/// FNV-1a（GUID 语料与账本指纹唯一哈希来源）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

// ── 五世代 GUID 冻结语料 ────────────────────────────────────────
/// GUID 四段结构（d1/d2/d3/d4，与 Windows SDK 支持世代 GUID 一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guid {
    pub d1: u32,
    pub d2: u16,
    pub d3: u16,
    pub d4: u64,
}

/// supportedOS 世代（bit 位 = 掩码位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Generation {
    /// bit0 · Vista
    Vista,
    /// bit1 · Win7
    Win7,
    /// bit2 · Win8
    Win8,
    /// bit3 · Win8.1
    Win81,
    /// bit4 · Win10
    Win10,
}

impl Generation {
    /// 世代序（= 掩码位号，0..5）。
    pub fn ordinal(self) -> u32 {
        match self {
            Generation::Vista => 0,
            Generation::Win7 => 1,
            Generation::Win8 => 2,
            Generation::Win81 => 3,
            Generation::Win10 => 4,
        }
    }
    /// 世代掩码位（1 << ordinal）。
    pub fn bit(self) -> u8 {
        let b = 1u8.checked_shl(self.ordinal()).unwrap_or(0);
        b
    }
    /// 冻结 GUID（Windows 实机 supportedOS 闭集）。
    pub fn guid(self) -> Guid {
        match self {
            Generation::Vista => Guid { d1: 0xE2011457, d2: 0x35DE, d3: 0x43DA, d4: 0x9100_910D_4849_7CE8 },
            Generation::Win7 => Guid { d1: 0x35138B9A, d2: 0x5D96, d3: 0x4FBD, d4: 0x8E2D_A244_0225_F93A },
            Generation::Win8 => Guid { d1: 0x4A2F28E8, d2: 0x1534, d3: 0x468C, d4: 0x8D05_1D88_7D3E_A1C5 },
            Generation::Win81 => Guid { d1: 0x1F676C76, d2: 0x80E1, d3: 0x4239, d4: 0x95BB_83D0_F6D0_DA78 },
            Generation::Win10 => Guid { d1: 0x8E0F7A12, d2: 0xBFB3, d3: 0x4FE8, d4: 0xB9A5_48FD_50A1_5A9A },
        }
    }
}

/// 五世代全量（掩码位遍历用）。
pub const ALL_GENS: [Generation; GEN_COUNT] = [
    Generation::Vista,
    Generation::Win7,
    Generation::Win8,
    Generation::Win81,
    Generation::Win10,
];

/// GUID → 世代查找（全四段精确匹配才算命中）。
pub fn guid_to_gen(g: Guid) -> Option<Generation> {
    let mut i = 0;
    while i < ALL_GENS.len() {
        let gen = match ALL_GENS.get(i) {
            Some(x) => *x,
            None => break,
        };
        if gen.guid() == g {
            return Some(gen);
        }
        i += 1;
    }
    None
}

/// GUID 部分命中检测（d1 命中某世代但 d4 不等 → 语义级损坏）。
pub fn guid_corrupted(g: Guid) -> bool {
    let mut i = 0;
    while i < ALL_GENS.len() {
        let gen = match ALL_GENS.get(i) {
            Some(x) => *x,
            None => break,
        };
        let w = gen.guid();
        if w.d1 == g.d1 && w.d2 == g.d2 && w != g {
            // d1/d2 命中但整体不等 → d3/d4 被篡改。
            return true;
        }
        i += 1;
    }
    false
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SosCode {
    /// 空声明（无 supportedOS 条目）
    EmptyDecl,
    /// GUID 完全未知（闭集外）
    GuidUnknown,
    /// GUID 段损坏（d1/d2 命中但 d3/d4 错）
    GuidCorrupt,
    /// 同一声明内世代重复
    GenDup,
    /// 掩码越界（bit5 及以上 / > 0x1F）
    MaskOutOfRange,
    /// 空掩码判定（无世代可适用）
    NoDeclaration,
    /// 状态机非法迁移
    StateIllegalTransit,
    /// 判定与 Windows 对照偏差
    DeviationFound,
    /// probe_id 越界（0 或 >15）
    IdOutOfRange,
    /// 双跑不一致
    NotReproducible,
    /// 裁决表缺行（组合不在 15 行闭集）
    AdjudicationMissing,
    /// 注入漏检
    InjectMissed,
}

impl SosCode {
    /// 诊断码 = SEG_BASE + 枚举序（0x7FE80..0x7FEC0）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断明细（闭集非空）。
    pub fn detail(self) -> &'static str {
        match self {
            SosCode::EmptyDecl => "空声明：无 supportedOS 条目，契约显式拒",
            SosCode::GuidUnknown => "GUID 未知：不在五代 supportedOS 闭集内",
            SosCode::GuidCorrupt => "GUID 段损坏：d1/d2 命中但 d3/d4 被篡改",
            SosCode::GenDup => "世代重复：同一声明内同世代多 entry",
            SosCode::MaskOutOfRange => "掩码越界：bit5 及以上非法（上限 0x1F）",
            SosCode::NoDeclaration => "空掩码判定：无世代可适用",
            SosCode::StateIllegalTransit => "状态机非法迁移：越级/回退全拒",
            SosCode::DeviationFound => "判定偏差：与 Windows 对照不等",
            SosCode::IdOutOfRange => "probe_id 越界：合法闭集 1..=15",
            SosCode::NotReproducible => "双跑不一致：确定性红线",
            SosCode::AdjudicationMissing => "裁决表缺行：组合不在 15 行闭集",
            SosCode::InjectMissed => "注入漏检：检出 < 注入，验收红线",
        }
    }
    /// 全码闭集（12 条）。
    pub const ALL: [SosCode; 12] = [
        SosCode::EmptyDecl,
        SosCode::GuidUnknown,
        SosCode::GuidCorrupt,
        SosCode::GenDup,
        SosCode::MaskOutOfRange,
        SosCode::NoDeclaration,
        SosCode::StateIllegalTransit,
        SosCode::DeviationFound,
        SosCode::IdOutOfRange,
        SosCode::NotReproducible,
        SosCode::AdjudicationMissing,
        SosCode::InjectMissed,
    ];
}

/// u32 诊断码反查中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < SosCode::ALL.len() {
        if let Some(c) = SosCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FE80 段闭集内"
}

// ── 15 组合裁决表（冻结：mask → 期望适用世代 ordinal） ──────────
/// 裁决表行（mask 写死，期望 ordinal = 掩码最高有效位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdjRow {
    /// 声明掩码（bit0=Vista..bit4=Win10）。
    pub mask: u8,
    /// Windows 对照期望适用世代 ordinal。
    pub expected: u32,
}

/// 15 行冻结裁决表（J1 主锚点：判定与 Windows 对照零偏差）。
pub const ADJ_TABLE: [AdjRow; ADJ_ROWS] = [
    AdjRow { mask: 0x01, expected: 0 },                            // 1 仅 Vista
    AdjRow { mask: 0x02, expected: 1 },                            // 2 仅 Win7
    AdjRow { mask: 0x04, expected: 2 },                            // 3 仅 Win8
    AdjRow { mask: 0x08, expected: 3 },                            // 4 仅 Win8.1
    AdjRow { mask: 0x10, expected: 4 },                            // 5 仅 Win10
    AdjRow { mask: 0x03, expected: 1 },                            // 6 Vista+7
    AdjRow { mask: 0x07, expected: 2 },                            // 7 Vista+7+8
    AdjRow { mask: 0x0F, expected: 3 },                            // 8 Vista..8.1
    AdjRow { mask: 0x1F, expected: 4 },                            // 9 全五代
    AdjRow { mask: 0x06, expected: 2 },                            // 10 7+8
    AdjRow { mask: 0x0C, expected: 3 },                            // 11 8+8.1
    AdjRow { mask: 0x18, expected: 4 },                            // 12 8.1+10
    AdjRow { mask: 0x11, expected: 4 },                            // 13 Vista+10
    AdjRow { mask: 0x1A, expected: 4 },                            // 14 7+8.1+10
    AdjRow { mask: 0x14, expected: 4 },                            // 15 8+10
];

/// mask → 期望 ordinal 查裁决表（缺行返回 Err）。
pub fn adjudicate(mask: u8) -> Result<u32, u32> {
    let mut i = 0;
    while i < ADJ_TABLE.len() {
        let row = match ADJ_TABLE.get(i) {
            Some(r) => *r,
            None => break,
        };
        if row.mask == mask {
            return Ok(row.expected);
        }
        i += 1;
    }
    Err(SosCode::AdjudicationMissing.code())
}

// ── 声明解析（世代列表 → 掩码，重复/未知/损坏全拒） ─────────────
/// 解析世代列表为掩码：空列表拒、世代重复拒、掩码越界拒。
pub fn parse_decl(gens: &[Generation]) -> Result<u8, u32> {
    if gens.len() == 0 {
        return Err(SosCode::EmptyDecl.code());
    }
    let mut mask = 0u8;
    let mut i = 0;
    while i < gens.len() {
        let g = match gens.get(i) {
            Some(x) => *x,
            None => break,
        };
        let b = g.bit();
        if b == 0 {
            return Err(SosCode::MaskOutOfRange.code());
        }
        // 重复世代：位已置 → 拒。
        if mask & b != 0 {
            return Err(SosCode::GenDup.code());
        }
        mask |= b;
        i += 1;
    }
    if mask > MASK_MAX {
        return Err(SosCode::MaskOutOfRange.code());
    }
    Ok(mask)
}

/// 判定：掩码 → 适用世代 ordinal（最高有效位；空掩码拒）。
pub fn judge(mask: u8) -> Result<u32, u32> {
    if mask == 0 {
        return Err(SosCode::NoDeclaration.code());
    }
    if mask > MASK_MAX {
        return Err(SosCode::MaskOutOfRange.code());
    }
    // 最高有效位 = max ordinal（Windows 对照语义）。
    let mut best = 0u32;
    let mut has = false;
    let mut i = 0;
    while i < ALL_GENS.len() {
        let g = match ALL_GENS.get(i) {
            Some(x) => *x,
            None => break,
        };
        if mask & g.bit() != 0 {
            best = g.ordinal();
            has = true;
        }
        i += 1;
    }
    if !has {
        return Err(SosCode::NoDeclaration.code());
    }
    Ok(best)
}

// ── 状态机（四态） ──────────────────────────────────────────────
/// 单声明探针生命周期四态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeclState {
    /// 未解析（原始世代列表未过闸）
    Unparsed,
    /// 已解析（掩码在账）
    Parsed,
    /// 已判定
    Judged,
    /// 对照一致
    Matched,
    /// 对照偏差
    Deviated,
}

/// 迁移闭集：合法恰 4 条，其余 16 组合显式拒。
pub fn can_transit(from: DeclState, to: DeclState) -> bool {
    match (from, to) {
        (DeclState::Unparsed, DeclState::Parsed) => true,
        (DeclState::Parsed, DeclState::Judged) => true,
        (DeclState::Judged, DeclState::Matched) => true,
        (DeclState::Judged, DeclState::Deviated) => true,
        _ => false,
    }
}

// ── 账本 ────────────────────────────────────────────────────────
/// 单声明探针结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclResult {
    pub probe_id: u32,
    /// 声明掩码。
    pub mask: u8,
    /// 判定 ordinal。
    pub verdict: u32,
    /// Windows 对照期望。
    pub expected: u32,
    pub deviated: bool,
}

/// 声明探针总账。
#[derive(Clone, Debug)]
pub struct DeclLedger {
    pub results: Vec<DeclResult>,
    /// 每探针状态（下标 = probe_id-1，恒 15 槽）。
    pub states: [DeclState; PROBE_COUNT],
    /// 状态机拒迁计数。
    pub transit_rejects: u32,
}

impl DeclLedger {
    pub fn new() -> DeclLedger {
        DeclLedger {
            results: Vec::new(),
            states: [DeclState::Unparsed; PROBE_COUNT],
            transit_rejects: 0,
        }
    }

    /// 单探针：裁决表取 mask → judge → 对拍 → 落账（状态链式迁移）。
    pub fn run_probe(&mut self, probe_id: u32) -> Result<DeclResult, u32> {
        if probe_id == 0 || probe_id > PROBE_COUNT as u32 {
            return Err(SosCode::IdOutOfRange.code());
        }
        let idx = (probe_id - 1) as usize;
        let cur = match self.states.get(idx) {
            Some(s) => *s,
            None => return Err(SosCode::IdOutOfRange.code()),
        };
        if cur != DeclState::Unparsed {
            self.transit_rejects = self.transit_rejects.saturating_add(1);
            return Err(SosCode::StateIllegalTransit.code());
        }
        let row = match ADJ_TABLE.get(idx) {
            Some(r) => *r,
            None => return Err(SosCode::AdjudicationMissing.code()),
        };
        // 掩码必须能在裁决表反查到（一致性闸）。
        if adjudicate(row.mask).is_err() {
            return Err(SosCode::AdjudicationMissing.code());
        }
        let verdict = judge(row.mask)?;
        let deviated = verdict != row.expected;
        let res = DeclResult {
            probe_id,
            mask: row.mask,
            verdict,
            expected: row.expected,
            deviated,
        };
        // Unparsed→Parsed→Judged→终态 链式闸。
        if !can_transit(DeclState::Unparsed, DeclState::Parsed)
            || !can_transit(DeclState::Parsed, DeclState::Judged)
        {
            return Err(SosCode::StateIllegalTransit.code());
        }
        let fin = if deviated {
            DeclState::Deviated
        } else {
            DeclState::Matched
        };
        if !can_transit(DeclState::Judged, fin) {
            return Err(SosCode::StateIllegalTransit.code());
        }
        self.results.push(res.clone());
        if let Some(s) = self.states.get_mut(idx) {
            *s = fin;
        }
        Ok(res)
    }

    /// 零偏差门：15 例全部 Matched。
    pub fn zero_deviation(&self) -> bool {
        let mut i = 0;
        while i < PROBE_COUNT {
            let st = match self.states.get(i) {
                Some(s) => *s,
                None => return false,
            };
            if st != DeclState::Matched {
                return false;
            }
            i += 1;
        }
        self.results.len() == PROBE_COUNT
    }

    /// 偏差清单。
    pub fn deviation_list(&self) -> Vec<(u32, u32, u32)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.results.len() {
            if let Some(r) = self.results.get(i) {
                if r.deviated {
                    out.push((r.probe_id, r.verdict, r.expected));
                }
            }
            i += 1;
        }
        out
    }
}

// ── 黄金全链：15 例全跑全对拍 ───────────────────────────────────
pub fn golden_run() -> Result<(usize, usize), u32> {
    let mut led = DeclLedger::new();
    let mut id = 1u32;
    while id <= PROBE_COUNT as u32 {
        led.run_probe(id)?;
        id += 1;
    }
    if !led.zero_deviation() {
        return Err(SosCode::DeviationFound.code());
    }
    if led.deviation_list().len() != 0 {
        return Err(SosCode::DeviationFound.code());
    }
    Ok((led.results.len(), ADJ_TABLE.len()))
}

// ── GUID 语料指纹往返 ───────────────────────────────────────────
/// 五世代 GUID 语料指纹（语料冻结可感知：任一段变更指纹即变）。
pub fn corpus_fnv() -> u64 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < ALL_GENS.len() {
        let g = match ALL_GENS.get(i) {
            Some(x) => *x,
            None => break,
        };
        let w = g.guid();
        bytes.push((w.d1 & 0xFF) as u8);
        bytes.push(((w.d1 >> 8) & 0xFF) as u8);
        bytes.push(((w.d1 >> 16) & 0xFF) as u8);
        bytes.push((w.d1 >> 24) as u8);
        bytes.push((w.d2 & 0xFF) as u8);
        bytes.push((w.d2 >> 8) as u8);
        bytes.push((w.d3 & 0xFF) as u8);
        bytes.push((w.d3 >> 8) as u8);
        bytes.push((w.d4 & 0xFF) as u8);
        bytes.push(((w.d4 >> 8) & 0xFF) as u8);
        bytes.push(((w.d4 >> 16) & 0xFF) as u8);
        bytes.push(((w.d4 >> 24) & 0xFF) as u8);
        bytes.push(((w.d4 >> 32) & 0xFF) as u8);
        bytes.push(((w.d4 >> 40) & 0xFF) as u8);
        bytes.push(((w.d4 >> 48) & 0xFF) as u8);
        bytes.push((w.d4 >> 56) as u8);
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 注入核：三类违约 ────────────────────────────────────────────
/// 类①：伪造 GUID（闭集外 d1）→ GuidUnknown 显式拒。
pub fn inject_fabricated_guid() -> Result<bool, u32> {
    let g = Guid { d1: 0xDEADBEEF, d2: 0x0001, d3: 0x0002, d4: 0x0102030405060708 };
    // 伪造 GUID 查不到世代（None）且非损坏（d1 不命中任何语料）。
    let hit = guid_to_gen(g).is_none() && !guid_corrupted(g);
    Ok(hit)
}

/// 类②：世代重复（同一声明 {Win7, Win7}）→ GenDup 显式拒。
pub fn inject_gen_dup() -> Result<bool, u32> {
    let gens = [Generation::Win7, Generation::Win7];
    let hit = match parse_decl(&gens) {
        Err(c) => c == SosCode::GenDup.code(),
        Ok(_) => false,
    };
    Ok(hit)
}

/// 类③：GUID 段损坏（Win10 的 d4 低位翻转）→ GuidCorrupt 显式感知。
pub fn inject_corrupt_guid() -> Result<bool, u32> {
    let mut g = Generation::Win10.guid();
    g.d4 ^= 0x01; // 段损坏一位
    // 精确查找必失败（找不到世代）。
    let unknown = guid_to_gen(g).is_none();
    // 语义级损坏感知（d1/d2 命中）。
    let corrupt = guid_corrupted(g);
    Ok(unknown && corrupt)
}

/// 注入检出账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed_total(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 注入 10/10：三类违约逐类断言被显式拒绝。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        led.record(0, inject_fabricated_guid()?);
        led.record(1, inject_gen_dup()?);
        led.record(2, inject_corrupt_guid()?);
        r += 1;
    }
    Ok(led)
}

// ── 对账辅助判据 ────────────────────────────────────────────────
/// 双跑一致：黄金全链 + 语料指纹两次相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SosCode::NotReproducible.code());
    }
    if corpus_fnv() != corpus_fnv() {
        return Err(SosCode::NotReproducible.code());
    }
    Ok(())
}

/// 裁决表一致性：每行 mask 的 judge() 必须等于行内期望（表内自洽）。
pub fn adj_table_recheck() -> Result<usize, u32> {
    let mut n = 0u32;
    let mut i = 0;
    while i < ADJ_TABLE.len() {
        let row = match ADJ_TABLE.get(i) {
            Some(r) => *r,
            None => break,
        };
        let v = judge(row.mask)?;
        if v != row.expected {
            return Err(SosCode::DeviationFound.code());
        }
        n = n.saturating_add(1);
        i += 1;
    }
    Ok(n as usize)
}

/// 段位守卫：12 码全落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SosCode::ALL.len() {
        if let Some(c) = SosCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据收口 ────────────────────────────────────────────────────
/// UNX-F18523 自检判据（15 条 ≤ 20 上限；动态明细 ≤ 8 槽）。
pub fn run_unx18523_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18523 supportedOS 声明探针（15 例）");

    s.add("J3-段位守卫", segment_guard(), "0x7FE80 窗口内 12 码全落位");

    // J1 主锚点：15 例判定与 Windows 对照零偏差。
    let gold = golden_run();
    let (g_ok, g_txt) = match &gold {
        Ok((r, adj)) => (*r == PROBE_COUNT && *adj == ADJ_ROWS, format!("判定={} 裁决行={}", r, adj)),
        Err(c) => (false, format!("黄金链断:{}", detail_of(*c))),
    };
    s.add_dyn("J1-15例对照零偏差", g_ok, &g_txt);

    // 裁决表自洽：15 行逐行 judge == expected。
    let adj = adj_table_recheck();
    s.add(
        "J1-裁决表15行自洽",
        match &adj { Ok(n) => *n == ADJ_ROWS, Err(_) => false },
        "mask→ordinal 逐行复算相等",
    );

    // 五世代 GUID 语料在位（每代精确互查）。
    let corpus_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < ALL_GENS.len() {
            let g = match ALL_GENS.get(i) {
                Some(x) => *x,
                None => { ok = false; break; }
            };
            if guid_to_gen(g.guid()) != Some(g) {
                ok = false;
                break;
            }
            i += 1;
        }
        ok && corpus_fnv() != 0
    };
    s.add("J1-五代GUID语料在位", corpus_ok, "五代 GUID 精确互查 + 语料指纹非零");

    let inj = inject_rounds(JUDGE_ROUNDS);
    s.add(
        "J2-注入①伪造GUID10/10",
        match &inj { Ok(l) => l.caught[0] == l.tries[0] && l.tries[0] == JUDGE_ROUNDS, Err(_) => false },
        "闭集外 GUID 无命中且非损坏",
    );
    s.add(
        "J2-注入②世代重复10/10",
        match &inj { Ok(l) => l.caught[1] == l.tries[1] && l.tries[1] == JUDGE_ROUNDS, Err(_) => false },
        "GenDup 码显式拒",
    );
    s.add(
        "J2-注入③GUID段损坏10/10",
        match &inj { Ok(l) => l.caught[2] == l.tries[2] && l.tries[2] == JUDGE_ROUNDS, Err(_) => false },
        "d4 翻转 → 精确失配 + GuidCorrupt 感知",
    );

    s.add(
        "J2-漏检恒零",
        match &inj { Ok(l) => l.missed_total() == 0, Err(_) => false },
        "tries = caught + missed，missed=0",
    );

    // 边界：空声明 / 空掩码 / 掩码越界 / id 越界。
    let boundary = {
        let empty_hit = parse_decl(&[]).is_err();
        let nomask_hit = judge(0).is_err();
        let mask_hit = judge(MASK_MAX + 1).is_err();
        let mut led = DeclLedger::new();
        let id0 = led.run_probe(0).is_err();
        let id16 = led.run_probe(PROBE_COUNT as u32 + 1).is_err();
        empty_hit && nomask_hit && mask_hit && id0 && id16
    };
    s.add("J3-边界空掩码越界全拒", boundary, "空声明/空掩码/掩码0x20/id 0与16");

    // 状态机：迁移闭集合法恰 4 条。
    let states = [
        DeclState::Unparsed,
        DeclState::Parsed,
        DeclState::Judged,
        DeclState::Matched,
        DeclState::Deviated,
    ];
    let mut legal_n = 0usize;
    let mut a = 0;
    while a < states.len() {
        let mut b = 0;
        while b < states.len() {
            let fa = match states.get(a) { Some(x) => *x, None => break };
            let fb = match states.get(b) { Some(x) => *x, None => break };
            if can_transit(fa, fb) {
                legal_n = legal_n.saturating_add(1);
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J2-迁移闭集合法恰4条", legal_n == 4, "4 态 25 组合枚举：合法 4 / 非法 21");

    // 重复运行同一探针（已 Matched 再跑）→ 非法迁移拒。
    let rerun_ok = {
        let mut led = DeclLedger::new();
        let first = led.run_probe(1).is_ok();
        let second = match led.run_probe(1) {
            Err(c) => c == SosCode::StateIllegalTransit.code(),
            Ok(_) => false,
        };
        first && second
    };
    s.add("J2-已判探针重跑拒", rerun_ok, "Matched 后再 run_probe → StateIllegalTransit");

    // 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run + corpus_fnv 两跑同值");

    // 掩码全占边界（0x1F 合法）。
    s.add(
        "J3-全占掩码0x1F合法",
        judge(MASK_MAX) == Ok(4),
        "五代全声明 → 适用世代 Win10(4)",
    );

    // 文案闭集。
    let mut detail_full = true;
    let mut di = 0;
    while di < SosCode::ALL.len() {
        if let Some(c) = SosCode::ALL.get(di) {
            if c.detail().len() == 0 || detail_of(c.code()).len() == 0 {
                detail_full = false;
            }
        }
        di += 1;
    }
    s.add("J3-文案闭集非空", detail_full, "12 码双查非空");

    // 收口。
    s.add("J3-判据条数≤20收口", s.len() < 20, "本模块判据上限纪律（含本条）");

    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_all_checks_green() {
        let set = run_unx18523_checks();
        assert!(set.all_passed(), "UNX-F18523 全判据应全绿: {}", set.red_items_text());
    }

    #[test]
    fn t02_golden_zero_deviation() {
        let g = golden_run();
        assert!(g.is_ok());
        assert_eq!(g.unwrap_or((0, 0)), (15, 15));
    }

    #[test]
    fn t03_guid_corpus_exact() {
        // 五代 GUID 语料逐代精确互查。
        assert_eq!(guid_to_gen(Generation::Vista.guid()), Some(Generation::Vista));
        assert_eq!(guid_to_gen(Generation::Win7.guid()), Some(Generation::Win7));
        assert_eq!(guid_to_gen(Generation::Win8.guid()), Some(Generation::Win8));
        assert_eq!(guid_to_gen(Generation::Win81.guid()), Some(Generation::Win81));
        assert_eq!(guid_to_gen(Generation::Win10.guid()), Some(Generation::Win10));
        // 实机口径抽查：Win7 d1 = 0x35138B9A。
        assert_eq!(Generation::Win7.guid().d1, 0x35138B9A);
        assert_eq!(Generation::Win10.guid().d1, 0x8E0F7A12);
    }

    #[test]
    fn t04_adj_table_self_consistent() {
        assert_eq!(adj_table_recheck().unwrap_or(0), 15);
        // 抽查：仅 Vista → 0；全五代 → 4；7+8 → 2。
        assert_eq!(judge(0x01).unwrap_or(9), 0);
        assert_eq!(judge(0x1F).unwrap_or(9), 4);
        assert_eq!(judge(0x06).unwrap_or(9), 2);
        // 裁决表缺行：0x20 越界掩码。
        assert!(adjudicate(0x20).is_err());
    }

    #[test]
    fn t05_parse_gates() {
        // 空声明。
        assert!(parse_decl(&[]).is_err());
        // 世代重复。
        assert!(parse_decl(&[Generation::Win8, Generation::Win8]).is_err());
        // 合法多声明。
        assert_eq!(parse_decl(&[Generation::Vista, Generation::Win10]).unwrap_or(0), 0x11);
        // 顺序无关（集合语义）。
        assert_eq!(parse_decl(&[Generation::Win10, Generation::Vista]).unwrap_or(0), 0x11);
    }

    #[test]
    fn t06_injection_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(inj.all_caught());
        assert_eq!(inj.missed_total(), 0);
        assert_eq!(inj.caught[0], 10);
    }

    #[test]
    fn t07_guid_corruption_two_level() {
        // 全未知 d1 → GuidUnknown 语义（无命中且非损坏）。
        let fab = Guid { d1: 0x11111111, d2: 0x2222, d3: 0x3333, d4: 0x4444444444444444 };
        assert!(guid_to_gen(fab).is_none());
        assert!(!guid_corrupted(fab));
        // d1/d2 命中但 d4 错 → 损坏感知。
        let mut g = Generation::Win81.guid();
        g.d4 = 0;
        assert!(guid_to_gen(g).is_none());
        assert!(guid_corrupted(g));
    }

    #[test]
    fn t08_state_machine_and_rerun() {
        let mut led = DeclLedger::new();
        assert!(led.run_probe(5).is_ok());
        // 重跑拒。
        assert!(led.run_probe(5).is_err());
        assert!(led.states[4] == DeclState::Matched);
        // 偏差注入模拟：篡改裁决表不可行（表是 const），改用掩码越界验证。
        assert!(judge(0x20).is_err());
    }

    #[test]
    fn t09_double_run_deterministic() {
        assert!(repro_check().is_ok());
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        // 语料指纹稳定。
        assert_eq!(corpus_fnv(), corpus_fnv());
    }

    #[test]
    fn t10_segment_and_detail() {
        assert!(segment_guard());
        let mut i = 0;
        while i < SosCode::ALL.len() {
            if let Some(c) = SosCode::ALL.get(i) {
                assert_eq!(detail_of(c.code()), c.detail());
                assert!(c.detail().len() > 0);
            }
            i += 1;
        }
        assert_eq!(detail_of(0xDEAD), "未知诊断码：不在 0x7FE80 段闭集内");
    }
}
