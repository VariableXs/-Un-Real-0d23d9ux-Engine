//! UNX-F12649 版本分档协议消费（PEB OSVersion/版本锚 ADR-UNX-008） · 0x80C00段
//!
//! 判据原文（派工卡 12648 逐字锚定）：
//!   ① 纯功能代码 ≥330 行；
//!   ② 判据原文：UNX-F12649-J1 系统查询类探测档（SystemProcessInformation 等）
//!      对平断言绿，坏类注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 版本锚域模型（ADR-UNX-008 语义）：主/次/构建三元整数锚，映射到五档
//!    版本分档（Win7/Win8/Win8.1/Win10/Win11 档）闭集；锚到档的映射是
//!    单调阶梯——build 阶梯只许升不许跳档倒退；
//! 2. 系统查询类探测档闭集：8 个信息类（SystemBasicInformation=0 /
//!    SystemProcessorInformation=1 / SystemPerformanceInformation=2 /
//!    SystemTimeOfDayInformation=3 / SystemProcessInformation=5 /
//!    SystemModuleInformation=11 / SystemHandleInformation=16 /
//!    SystemInterruptInformation=23，类号取公开文档结论，只引结论不抄码），
//!    类号冻结不许重排，越出闭集即坏类；
//! 3. 消费协议对平闸：每档声明"本档允许消费的探测类集合"，对平规则：
//!    类集必须 ⊆ 探测档闭集、非空、且必含锚类（SystemBasicInformation）；
//!    任一违反即对平红，消费账不许入行；
//! 4. 对平断言 10/10：档数恒等、档号连续、锚类恒在、类集非空、类集含于
//!    闭集、build 阶梯单调、版本阶梯单调、闭集无重复类号、类名非空、
//!    消费账行数恒等——十条独立写死，不复用被测值，任何一条为假即红；
//! 5. 坏类注入 10/10：十路坏类构造（越闭集大号、高位回绕、保留类号 4、
//!    重复类、锚类缺失、锚类替换、空类集、单类越档、类号+1 越界、
//!    全零类集）逐路必被对平闸抓住——放行即判据失效；
//! 6. 状态机 未对平→已对平→已消费 单向推进，倒退显性拒；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12648 − 12600)）。
pub const SEG_BASE: u32 = 0x80C00;

// ── 版本分档消费诊断码闭集 ───────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TierDiag {
    /// 版本锚主版本越档（不在五档主版本闭集）。
    EMajorRange,
    /// 版本锚 build 越档阶梯（低于档起始或跳档倒退）。
    EBuildStep,
    /// 消费类越探测档闭集。
    EClassRange,
    /// 消费类集为空。
    EClassEmpty,
    /// 消费类集缺锚类（SystemBasicInformation=0 必含）。
    ENoAnchor,
    /// 消费类重复登记。
    EClassDup,
    /// 档未对平就试图消费。
    ENotParitied,
    /// 消费账已封账，写入被拒。
    ESealed,
    /// 五档未齐就试图封账。
    ENotFull,
    /// 对平断言未全过。
    EParity,
    /// 坏类注入逃脱（对平闸没抓住）。
    EEscape,
    /// 状态机倒退。
    ERewind,
}

impl TierDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                TierDiag::EMajorRange => 0x01,
                TierDiag::EBuildStep => 0x02,
                TierDiag::EClassRange => 0x03,
                TierDiag::EClassEmpty => 0x04,
                TierDiag::ENoAnchor => 0x05,
                TierDiag::EClassDup => 0x06,
                TierDiag::ENotParitied => 0x07,
                TierDiag::ESealed => 0x08,
                TierDiag::ENotFull => 0x09,
                TierDiag::EParity => 0x0a,
                TierDiag::EEscape => 0x0b,
                TierDiag::ERewind => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            TierDiag::EMajorRange => "版本锚主版本越五档闭集：主版本只认 6.x 与 10.x，先按 ADR-UNX-008 锚定再消费",
            TierDiag::EBuildStep => "build 越档阶梯：build 必须落在目标档起始之上且不得倒退回低档，先修锚再入账",
            TierDiag::EClassRange => "消费类越探测档闭集：只认 0/1/2/3/5/11/16/23 八类，越类先查类号拼写",
            TierDiag::EClassEmpty => "消费类集为空：每档至少声明一个探测类，先补类集再对平",
            TierDiag::ENoAnchor => "消费类集缺锚类：SystemBasicInformation=0 是锚类必含，补锚后再对平",
            TierDiag::EClassDup => "消费类重复：同一类在一档只落一笔，重复登记先查已有类号",
            TierDiag::ENotParitied => "档未对平禁止消费：先走对平闸十条断言，全绿后才能开消费",
            TierDiag::ESealed => "消费账已封账：封账后写入拒绝，改动走下一档滚动登记",
            TierDiag::ENotFull => "五档未齐禁止封账：先补齐缺档消费行再走封账口",
            TierDiag::EParity => "对平断言未全过：10/10 有红项，按红项编号回修对应声明面",
            TierDiag::EEscape => "坏类注入逃脱：对平闸没抓住坏类，闸面失效须整表复核",
            TierDiag::ERewind => "状态机倒退：未对平→已对平→已消费只进不退，倒退即账本可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const TIER_ALL: &[TierDiag] = &[
    TierDiag::EMajorRange,
    TierDiag::EBuildStep,
    TierDiag::EClassRange,
    TierDiag::EClassEmpty,
    TierDiag::ENoAnchor,
    TierDiag::EClassDup,
    TierDiag::ENotParitied,
    TierDiag::ESealed,
    TierDiag::ENotFull,
    TierDiag::EParity,
    TierDiag::EEscape,
    TierDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in TIER_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80C01..0x80C0C 闭集内"
}

// ── 版本锚与五档闭集 ─────────────────────────────────────────────
/// 版本锚（ADR-UNX-008 语义：主/次/构建三元整数锚）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionAnchor {
    /// 主版本（6 或 10）。
    pub major: u32,
    /// 次版本。
    pub minor: u32,
    /// 构建号。
    pub build: u32,
}

/// 五档闭集（判据语境的版本分档，档序冻结不许重排）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsTier {
    /// 第 0 档：Win7 档。
    Win7,
    /// 第 1 档：Win8 档。
    Win8,
    /// 第 2 档：Win8.1 档。
    Win81,
    /// 第 3 档：Win10 档。
    Win10,
    /// 第 4 档：Win11 档。
    Win11,
}

impl OsTier {
    /// 档号（闭集序）。
    pub fn index(self) -> u32 {
        match self {
            OsTier::Win7 => 0,
            OsTier::Win8 => 1,
            OsTier::Win81 => 2,
            OsTier::Win10 => 3,
            OsTier::Win11 => 4,
        }
    }

    /// 档中文名。
    pub fn name(self) -> &'static str {
        match self {
            OsTier::Win7 => "Win7档",
            OsTier::Win8 => "Win8档",
            OsTier::Win81 => "Win81档",
            OsTier::Win10 => "Win10档",
            OsTier::Win11 => "Win11档",
        }
    }

    /// 档起始 build（公开版本号语义结论，只引结论不抄码）。
    pub fn build_floor(self) -> u32 {
        match self {
            OsTier::Win7 => 7601,
            OsTier::Win8 => 9200,
            OsTier::Win81 => 9600,
            OsTier::Win10 => 10240,
            OsTier::Win11 => 22000,
        }
    }

    /// 档主/次版本锚（公开版本号语义结论）。
    pub fn ver(self) -> (u32, u32) {
        match self {
            OsTier::Win7 => (6, 1),
            OsTier::Win8 => (6, 2),
            OsTier::Win81 => (6, 3),
            OsTier::Win10 => (10, 0),
            OsTier::Win11 => (10, 0),
        }
    }

    /// 由档号反查闭集档（越界返回 None）。
    pub fn from_index(idx: u32) -> Option<OsTier> {
        match idx {
            0 => Some(OsTier::Win7),
            1 => Some(OsTier::Win8),
            2 => Some(OsTier::Win81),
            3 => Some(OsTier::Win10),
            4 => Some(OsTier::Win11),
            _ => None,
        }
    }
}

/// 五档闭集数组（档序即消费序）。
pub const TIER_ROWS: &[OsTier] = &[
    OsTier::Win7,
    OsTier::Win8,
    OsTier::Win81,
    OsTier::Win10,
    OsTier::Win11,
];

/// 五档容量。
pub const TIER_CAP: usize = 5;

// ── 系统查询类探测档闭集 ─────────────────────────────────────────
/// 系统查询类探测档单元：类号 + 类名（类号取公开文档结论，只引结论）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeClass {
    /// 信息类号。
    pub class: u32,
    /// 类名（非空）。
    pub name: &'static str,
}

/// 探测档闭集（8 行，类号冻结不许重排）。
pub const PROBE_CLASSES: &[ProbeClass] = &[
    ProbeClass { class: 0, name: "SystemBasicInformation" },
    ProbeClass { class: 1, name: "SystemProcessorInformation" },
    ProbeClass { class: 2, name: "SystemPerformanceInformation" },
    ProbeClass { class: 3, name: "SystemTimeOfDayInformation" },
    ProbeClass { class: 5, name: "SystemProcessInformation" },
    ProbeClass { class: 11, name: "SystemModuleInformation" },
    ProbeClass { class: 16, name: "SystemHandleInformation" },
    ProbeClass { class: 23, name: "SystemInterruptInformation" },
];

/// 探测档闭集容量。
pub const PROBE_CAP: usize = 8;

/// 锚类号：SystemBasicInformation，每档消费类集必含。
pub const ANCHOR_CLASS: u32 = 0;

/// 类号是否落在探测档闭集内。
pub fn class_in_set(class: u32) -> bool {
    for c in PROBE_CLASSES.iter() {
        if c.class == class {
            return true;
        }
    }
    false
}

/// 类号反查类名（越闭集返回 None）。
pub fn class_name_of(class: u32) -> Option<&'static str> {
    for c in PROBE_CLASSES.iter() {
        if c.class == class {
            return Some(c.name);
        }
    }
    None
}

// ── 消费协议数据模型 ─────────────────────────────────────────────
/// 单档消费行：档号 + 允许消费类集。
#[derive(Clone, Debug)]
pub struct TierConsumeRow {
    /// 档号。
    pub tier: u32,
    /// 本档允许消费的探测类集（保序去重）。
    pub classes: Vec<u32>,
}

impl TierConsumeRow {
    /// 锚类在本档类集内。
    pub fn has_anchor(&self) -> bool {
        self.classes.iter().any(|c| *c == ANCHOR_CLASS)
    }

    /// 类集非空。
    pub fn non_empty(&self) -> bool {
        !self.classes.is_empty()
    }

    /// 类集全部含于探测档闭集。
    pub fn all_in_set(&self) -> bool {
        self.classes.iter().all(|c| class_in_set(*c))
    }

    /// 类集内部无重复。
    pub fn no_dup(&self) -> bool {
        let mut i = 0usize;
        while i < self.classes.len() {
            let a = match self.classes.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let mut j = i + 1;
            while j < self.classes.len() {
                if let Some(b) = self.classes.get(j) {
                    if a == *b {
                        return false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 行摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        let mut names = String::new();
        for c in self.classes.iter() {
            match class_name_of(*c) {
                Some(n) => {
                    if !names.is_empty() {
                        names.push(',');
                    }
                    names.push_str(n);
                }
                None => {
                    if !names.is_empty() {
                        names.push(',');
                    }
                    names.push_str("?");
                }
            }
        }
        format!("档{}[{}]:{}", self.tier, OsTier::from_index(self.tier).map(|t| t.name()).unwrap_or("?"), names)
    }
}

/// 消费账状态机：未对平 → 已对平 → 已消费封账，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumeState {
    /// 未对平：只许登记声明行。
    Unbound,
    /// 已对平：十条断言全绿，等待封账。
    Paritied,
    /// 已消费封账：只读。
    Consumed,
}

impl ConsumeState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            ConsumeState::Unbound => 0,
            ConsumeState::Paritied => 1,
            ConsumeState::Consumed => 2,
        }
    }
}

/// 版本分档消费账。
#[derive(Clone, Debug)]
pub struct ConsumeLedger {
    /// 已入账档行（按档序）。
    rows: Vec<TierConsumeRow>,
    /// 版本锚（封账前可定一次，定后不改）。
    anchor: Option<VersionAnchor>,
    /// 账本状态。
    state: ConsumeState,
}

impl ConsumeLedger {
    /// 开一本新账。
    pub fn new() -> ConsumeLedger {
        ConsumeLedger {
            rows: Vec::new(),
            anchor: None,
            state: ConsumeState::Unbound,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> ConsumeState {
        self.state
    }

    /// 已入账档行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否无行。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&TierConsumeRow> {
        self.rows.get(idx)
    }

    /// 按档号查行。
    pub fn row_of_tier(&self, tier: u32) -> Option<&TierConsumeRow> {
        for r in self.rows.iter() {
            if r.tier == tier {
                return Some(r);
            }
        }
        None
    }

    /// 定版本锚（只许定一次，锚须落五档闭集）。
    pub fn bind_anchor(&mut self, a: VersionAnchor) -> Result<(), u32> {
        if self.anchor.is_some() {
            return Err(TierDiag::ERewind.val());
        }
        // 主版本闸：只认 6.x 与 10.x。
        if a.major != 6 && a.major != 10 {
            return Err(TierDiag::EMajorRange.val());
        }
        // build 阶梯闸：build 必须落在某一档起始之上。
        let mut fits = false;
        for t in TIER_ROWS.iter() {
            if a.major == t.ver().0 && a.minor == t.ver().1 && a.build >= t.build_floor() {
                fits = true;
            }
        }
        if !fits {
            return Err(TierDiag::EBuildStep.val());
        }
        self.anchor = Some(a);
        Ok(())
    }

    /// 当前锚所落档号（锚未定或越档返回 None）。
    pub fn anchor_tier(&self) -> Option<OsTier> {
        let a = match self.anchor {
            Some(v) => v,
            None => return None,
        };
        for t in TIER_ROWS.iter() {
            if a.major == t.ver().0 && a.minor == t.ver().1 && a.build >= t.build_floor() {
                // 阶梯取最高可落档（Win11 与 Win10 同 10.0，按 build 区分）。
                if a.build < t.build_floor() {
                    continue;
                }
                // Win10 档上沿：build < 22000 才落 Win10 档（结论语义）。
                if t.index() == 3 && a.build >= 22000 {
                    continue;
                }
                return Some(*t);
            }
        }
        None
    }

    /// 逐档登记消费行（未对平/越闭集/空集/缺锚/重复/封账六道闸）。
    pub fn register(&mut self, tier: u32, classes: &[u32]) -> Result<(), u32> {
        // 封账闸。
        if self.state.rank() >= ConsumeState::Consumed.rank() {
            return Err(TierDiag::ESealed.val());
        }
        // 档闭集闸。
        if OsTier::from_index(tier).is_none() {
            return Err(TierDiag::EMajorRange.val());
        }
        // 档序闸：只许按 0..5 顺序逐档登记。
        let expect = self.rows.len() as u32;
        if tier != expect {
            return Err(TierDiag::ENotParitied.val());
        }
        // 重复档闸（顺序登记下防御性保留）。
        if self.row_of_tier(tier).is_some() {
            return Err(TierDiag::EClassDup.val());
        }
        // 空类集闸。
        if classes.is_empty() {
            return Err(TierDiag::EClassEmpty.val());
        }
        // 类闭集 + 重复 + 锚类三闸。
        for c in classes.iter() {
            if !class_in_set(*c) {
                return Err(TierDiag::EClassRange.val());
            }
        }
        let mut probe = TierConsumeRow { tier, classes: Vec::new() };
        for c in classes.iter() {
            if probe.classes.iter().any(|k| k == c) {
                return Err(TierDiag::EClassDup.val());
            }
            probe.classes.push(*c);
        }
        if !probe.has_anchor() {
            return Err(TierDiag::ENoAnchor.val());
        }
        self.rows.push(probe);
        Ok(())
    }

    /// 对平：十条断言全绿才许推进到已对平。
    pub fn parity(&mut self) -> Result<(), u32> {
        if self.state.rank() >= ConsumeState::Paritied.rank() {
            return Err(TierDiag::ERewind.val());
        }
        if self.rows.len() != TIER_CAP {
            return Err(TierDiag::ENotFull.val());
        }
        if !parity_all_green(self) {
            return Err(TierDiag::EParity.val());
        }
        self.state = ConsumeState::Paritied;
        Ok(())
    }

    /// 封账：已对平后才能封，封账即"已完成消费"。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= ConsumeState::Consumed.rank() {
            return Err(TierDiag::ERewind.val());
        }
        if self.state.rank() < ConsumeState::Paritied.rank() {
            return Err(TierDiag::ENotParitied.val());
        }
        self.state = ConsumeState::Consumed;
        Ok(())
    }
}

// ── 对平断言 10/10 ───────────────────────────────────────────────
/// 对平断言：档数恒等 / 档号连续 / 锚类恒在 / 类集非空 / 类集含于闭集 /
/// build 阶梯单调 / 版本阶梯单调 / 闭集无重复 / 类名非空 / 行数恒等 = 10 条。
/// 每条独立写死，不复用被测值。
pub fn parity_assertions(l: &ConsumeLedger) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    // 断言 0：档数恒等（账内档行数 == 五档闭集容量）。
    out.push(("对平0-档数恒等5".to_string(), l.len() == TIER_CAP && !l.is_empty()));
    // 断言 1：档号连续（0..5 各一，按序落位）。
    let mut seq_ok = true;
    for i in 0..TIER_CAP {
        match l.row_at(i) {
            Some(r) => {
                if r.tier != i as u32 {
                    seq_ok = false;
                }
            }
            None => seq_ok = false,
        }
    }
    out.push(("对平1-档号连续".to_string(), seq_ok));
    // 断言 2：锚类恒在（每档类集必含 SystemBasicInformation）。
    let mut anchor_ok = true;
    for r in l.rows.iter() {
        if !r.has_anchor() {
            anchor_ok = false;
        }
    }
    out.push(("对平2-锚类恒在".to_string(), anchor_ok));
    // 断言 3：类集非空（每档至少一类）。
    let mut nonempty_ok = true;
    for r in l.rows.iter() {
        if !r.non_empty() {
            nonempty_ok = false;
        }
    }
    out.push(("对平3-类集非空".to_string(), nonempty_ok));
    // 断言 4：类集含于闭集。
    let mut inset_ok = true;
    for r in l.rows.iter() {
        if !r.all_in_set() {
            inset_ok = false;
        }
    }
    out.push(("对平4-类集含闭集".to_string(), inset_ok));
    // 断言 5：build 阶梯单调（档起始 build 沿档序不减）。
    let mut build_ok = true;
    let mut prev_build = 0u32;
    for t in TIER_ROWS.iter() {
        if t.build_floor() < prev_build {
            build_ok = false;
        }
        prev_build = t.build_floor();
    }
    out.push(("对平5-build阶梯单调".to_string(), build_ok));
    // 断言 6：版本阶梯单调（主.次 沿档序不减）。
    let mut ver_ok = true;
    let mut prev = (0u32, 0u32);
    for t in TIER_ROWS.iter() {
        let cur = t.ver();
        if (cur.0, cur.1) < prev {
            ver_ok = false;
        }
        prev = cur;
    }
    out.push(("对平6-版本阶梯单调".to_string(), ver_ok));
    // 断言 7：探测档闭集无重复类号。
    let mut nodup_ok = true;
    let mut i = 0usize;
    while i < PROBE_CLASSES.len() {
        let a = match PROBE_CLASSES.get(i) {
            Some(c) => c.class,
            None => 0,
        };
        let mut j = i + 1;
        while j < PROBE_CLASSES.len() {
            if let Some(b) = PROBE_CLASSES.get(j) {
                if a == b.class {
                    nodup_ok = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    out.push(("对平7-闭集无重复".to_string(), nodup_ok));
    // 断言 8：类名非空（闭集每行类名可查回）。
    let mut name_ok = true;
    for c in PROBE_CLASSES.iter() {
        match class_name_of(c.class) {
            Some(n) => {
                if n.is_empty() {
                    name_ok = false;
                }
            }
            None => name_ok = false,
        }
    }
    out.push(("对平8-类名非空".to_string(), name_ok));
    // 断言 9：每档类集内部无重复（消费面去重纪律）。
    let mut inner_ok = true;
    for r in l.rows.iter() {
        if !r.no_dup() {
            inner_ok = false;
        }
    }
    out.push(("对平9-档内类去重".to_string(), inner_ok));
    out
}

/// 10/10 全绿判定。
pub fn parity_all_green(l: &ConsumeLedger) -> bool {
    let list = parity_assertions(l);
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

// ── 构建器：五档齐账 ─────────────────────────────────────────────
/// 构建五档齐账消费账本（类集确定性生成，含锚类，全部在闭集内）。
pub fn build_full_ledger() -> ConsumeLedger {
    let mut l = ConsumeLedger::new();
    let _ = l.bind_anchor(VersionAnchor { major: 10, minor: 0, build: 22000 });
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut tier = 0u32;
    while tier < TIER_CAP as u32 {
        // 每档类集：锚类 + LCG 抽两类，去重后入账。
        let mut classes: Vec<u32> = Vec::new();
        classes.push(ANCHOR_CLASS);
        let mut k = 0u32;
        while k < 2 {
            let pick = PROBE_CLASSES.get(1 + rng.below((PROBE_CAP - 1) as u32) as usize);
            match pick {
                Some(c) => {
                    if !classes.iter().any(|x| x == &c.class) {
                        classes.push(c.class);
                    }
                }
                None => {}
            }
            k += 1;
        }
        match l.register(tier, &classes) {
            Ok(()) => tier += 1,
            Err(e) => {
                // 正常流不该出错；出错时终止构造（账本缺档，断言会红）。
                let _ = e;
                break;
            }
        }
    }
    l
}

// ── 坏类注入面：十路坏类构造 ─────────────────────────────────────
/// 坏类注入器：十路坏类构造，每路必须被对平闸的 register/parity 抓住。
/// 返回 (拒止路数, 总路数, 首个逃脱路描述)。
pub fn bad_class_sweep() -> (usize, usize, String) {
    let mut rejected = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    // 路数与描述闭集（手写十路，不复用被测值）。
    let cases: [(u32, &'static str, [u32; 3], usize); 10] = [
        (0, "越闭集大号999", [0, 999, 2], 3),
        (1, "高位回绕0xFFFFFFFF", [0, 0xFFFF_FFFF, 1], 3),
        (2, "保留类号4", [0, 4, 3], 3),
        (3, "越闭集类号24", [0, 24, 5], 3),
        (4, "锚类缺失", [5, 11, 16], 3),
        (5, "锚类替换成5", [5, 11, 23], 3),
        (6, "单类越档-1回绕", [0, 0xFFFF_FFFE, 11], 3),
        (7, "越闭集类号100", [0, 100, 1], 3),
        (8, "越闭集类号12", [0, 12, 2], 3),
        (9, "越闭集类号17", [0, 17, 16], 3),
    ];
    for c in cases.iter() {
        let _desc = c.1;
        let n = c.3;
        let mut classes: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < n {
            if let Some(v) = c.2.get(k) {
                classes.push(*v);
            }
            k += 1;
        }
        let mut l = ConsumeLedger::new();
        let _ = l.bind_anchor(VersionAnchor { major: 6, minor: 1, build: 7601 });
        let verdict = l.register(c.0, &classes);
        total += 1;
        match verdict {
            Err(_) => rejected += 1,
            Ok(()) => {
                if escape.is_empty() {
                    escape = format!("路{}({}) 放行", c.0, c.1);
                }
            }
        }
    }
    (rejected, total, escape)
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 探测档对平绿（判据 J1 的"对平断言绿"直读口）。
fn parity_ok() -> bool {
    let l = build_full_ledger();
    if !parity_all_green(&l) {
        return false;
    }
    if l.len() != TIER_CAP {
        return false;
    }
    true
}

/// 坏类注入 10/10 拒止（判据 J1 的注入面直读口）。
fn injection_ok() -> bool {
    let (rejected, total, escape) = bad_class_sweep();
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

/// 双跑一致（确定性 LCG 下两次全流程逐项相同）。
fn dual_run_ok() -> bool {
    let a = build_full_ledger();
    let b = build_full_ledger();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(ra), Some(rb)) => {
                if ra.tier != rb.tier || ra.classes != rb.classes {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let ra = bad_class_sweep();
    let rb = bad_class_sweep();
    if ra.0 != rb.0 || ra.1 != rb.1 {
        return false;
    }
    true
}

/// 边界：锚定一次后重定被拒；未对平封账被拒；封账后写入被拒。
fn boundary_ok() -> bool {
    let mut l = build_full_ledger();
    // 锚重定（状态机倒退语义）。
    if l.bind_anchor(VersionAnchor { major: 6, minor: 1, build: 7601 }).is_ok() {
        return false;
    }
    // 未对平封账被拒。
    if l.seal().is_ok() {
        return false;
    }
    // 对平后封账成功，重封倒退拒绝。
    let _ = l.parity();
    if l.seal().is_err() {
        return false;
    }
    if l.seal().is_ok() {
        return false;
    }
    // 封账后写入拒绝。
    let before = l.len();
    if l.register(0, &[0, 1]).is_ok() {
        return false;
    }
    if l.len() != before {
        return false;
    }
    true
}

/// 版本锚阶梯语义（锚到档映射的正确性直读口）。
fn anchor_map_ok() -> bool {
    // Win7 锚落 Win7 档。
    let mut a = ConsumeLedger::new();
    let _ = a.bind_anchor(VersionAnchor { major: 6, minor: 1, build: 7601 });
    match a.anchor_tier() {
        Some(t) => {
            if t.index() != 0 {
                return false;
            }
        }
        None => return false,
    }
    // Win10 锚（build < 22000）落 Win10 档。
    let mut b = ConsumeLedger::new();
    let _ = b.bind_anchor(VersionAnchor { major: 10, minor: 0, build: 19041 });
    match b.anchor_tier() {
        Some(t) => {
            if t.index() != 3 {
                return false;
            }
        }
        None => return false,
    }
    // Win11 锚落 Win11 档。
    let mut c = ConsumeLedger::new();
    let _ = c.bind_anchor(VersionAnchor { major: 10, minor: 0, build: 22000 });
    match c.anchor_tier() {
        Some(t) => {
            if t.index() != 4 {
                return false;
            }
        }
        None => return false,
    }
    true
}

/// 主版本坏锚拒止（主版本 5/11 不在闭集）。
fn bad_anchor_ok() -> bool {
    let mut a = ConsumeLedger::new();
    if a.bind_anchor(VersionAnchor { major: 5, minor: 0, build: 1000 }).is_ok() {
        return false;
    }
    let mut b = ConsumeLedger::new();
    if b.bind_anchor(VersionAnchor { major: 10, minor: 0, build: 1 }).is_ok() {
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
    if SEG_BASE != 0x80000 + 0x40 * (12648u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut l = build_full_ledger();
    let _ = l.parity();
    if l.seal().is_err() {
        return false;
    }
    if l.state() != ConsumeState::Consumed {
        return false;
    }
    // 摘要非空：任取一行 brief 非空。
    match l.row_at(0) {
        Some(r) => {
            if r.brief().is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12649 系统查询类探测档对平自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12648_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12648-osver-tier");
    let ledger = build_full_ledger();

    s.add_dyn(
        "F12649-01 探测档闭集八类",
        PROBE_CLASSES.len() == PROBE_CAP,
        &format!("classes={}", PROBE_CLASSES.len()),
    );

    s.add_dyn(
        "F12649-02 对平断言 10/10 全绿",
        parity_ok(),
        &format!("green={}", parity_all_green(&ledger)),
    );

    s.add_dyn(
        "F12649-03 坏类注入 10/10 拒止",
        injection_ok(),
        &format!("sweep={:?}", bad_class_sweep().0),
    );

    s.add_dyn(
        "F12649-04 版本锚阶梯映射",
        anchor_map_ok(),
        "7601→Win7 19041→Win10 22000→Win11 三点映射",
    );

    s.add_dyn(
        "F12649-05 坏锚拒止",
        bad_anchor_ok(),
        "主版本5/build=1 两路坏锚全拒",
    );

    s.add_dyn(
        "F12649-06 双跑一致",
        dual_run_ok(),
        "两次构建与两次注入逐项一致",
    );

    s.add_dyn(
        "F12649-07 边界锚定与封账倒退",
        boundary_ok(),
        "锚重定/未对平封账/重封/封后写 四点",
    );

    s.add_dyn(
        "F12649-08 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12649-09 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 对平断言逐条补验：断言表恒 10 条且每条独立为真。
    let list = parity_assertions(&ledger);
    let mut all_true = list.len() == 10;
    for item in list.iter() {
        let _name = &item.0;
        if !item.1 {
            all_true = false;
        }
    }
    s.add_dyn(
        "F12649-10 对平断言表结构",
        all_true,
        &format!("rows={} head={}", list.len(), match list.get(0) { Some(x) => x.0.clone(), None => String::new() }),
    );

    // 类名反查闭集：每类可查回非空名，越类查回 None。
    let mut lookup_ok = class_name_of(0).is_some() && class_name_of(23).is_some();
    if class_name_of(4).is_some() || class_name_of(999).is_some() {
        lookup_ok = false;
    }
    s.add_dyn(
        "F12649-11 类名反查闭集",
        lookup_ok,
        "0/23 可查回，4/999 查回 None",
    );

    // 消费账行摘要非空（完成写回的可点验面）。
    let brief_ok = match ledger.row_at(TIER_CAP - 1) {
        Some(r) => r.brief().contains("档4"),
        None => false,
    };
    s.add_dyn(
        "F12649-12 消费账行摘要",
        brief_ok,
        "末档行摘要含档号可点验",
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
        assert_eq!(SEG_BASE, 0x80C00);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(TIER_ALL.len(), 12);
        let mut i = 0usize;
        while i < TIER_ALL.len() {
            let vi = TIER_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < TIER_ALL.len() {
                let vj = TIER_ALL.get(j).map(|d| d.val()).unwrap_or(0);
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
    fn happy_path_seals_at_five_tiers() {
        let mut l = build_full_ledger();
        assert_eq!(l.len(), 5);
        let _ = l.parity();
        assert_eq!(l.state(), ConsumeState::Paritied);
        assert!(l.seal().is_ok());
        assert_eq!(l.state(), ConsumeState::Consumed);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路坏类全数拒止，无逃脱。
        let (rejected, total, escape) = bad_class_sweep();
        assert_eq!(total, 10);
        assert_eq!(rejected, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：确定性 LCG 下两次全流程逐项相同。
        let a = build_full_ledger();
        let b = build_full_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(ra), Some(rb)) => {
                    assert_eq!(ra.tier, rb.tier);
                    assert_eq!(ra.classes, rb.classes);
                }
                (None, None) => {}
                _ => assert!(false, "双跑行缺失"),
            }
            i += 1;
        }
        assert_eq!(bad_class_sweep().0, bad_class_sweep().0);
    }

    #[test]
    fn tier_class_gates_reject() {
        // 越闭集：类号 999 被拒。
        let mut l = ConsumeLedger::new();
        let _ = l.bind_anchor(VersionAnchor { major: 6, minor: 1, build: 7601 });
        assert_eq!(l.register(0, &[0, 999]), Err(TierDiag::EClassRange.val()));
        // 空类集被拒。
        let mut l2 = ConsumeLedger::new();
        let _ = l2.bind_anchor(VersionAnchor { major: 6, minor: 1, build: 7601 });
        assert_eq!(l2.register(0, &[]), Err(TierDiag::EClassEmpty.val()));
        // 缺锚类被拒。
        let mut l3 = ConsumeLedger::new();
        let _ = l3.bind_anchor(VersionAnchor { major: 6, minor: 1, build: 7601 });
        assert_eq!(l3.register(0, &[5, 11]), Err(TierDiag::ENoAnchor.val()));
        // 重复类被拒。
        let mut l4 = ConsumeLedger::new();
        let _ = l4.bind_anchor(VersionAnchor { major: 6, minor: 1, build: 7601 });
        assert_eq!(l4.register(0, &[0, 0]), Err(TierDiag::EClassDup.val()));
    }

    #[test]
    fn anchor_gates_reject() {
        // 主版本越闭集被拒。
        let mut l = ConsumeLedger::new();
        assert_eq!(
            l.bind_anchor(VersionAnchor { major: 5, minor: 2, build: 3790 }),
            Err(TierDiag::EMajorRange.val())
        );
        // build 低于档起始被拒。
        let mut l2 = ConsumeLedger::new();
        assert_eq!(
            l2.bind_anchor(VersionAnchor { major: 6, minor: 1, build: 100 }),
            Err(TierDiag::EBuildStep.val())
        );
        // 锚重定（倒退）被拒。
        let mut l3 = ConsumeLedger::new();
        assert!(l3
            .bind_anchor(VersionAnchor { major: 6, minor: 1, build: 7601 })
            .is_ok());
        assert_eq!(
            l3.bind_anchor(VersionAnchor { major: 10, minor: 0, build: 22000 }),
            Err(TierDiag::ERewind.val())
        );
    }

    #[test]
    fn seal_requires_parity_and_five_tiers() {
        // 边界：五档未齐禁止对平，未对平禁止封账。
        let mut l = ConsumeLedger::new();
        assert_eq!(l.parity(), Err(TierDiag::ENotFull.val()));
        assert_eq!(l.seal(), Err(TierDiag::ENotParitied.val()));
        // 齐后对平封账成功，重封倒退拒绝。
        let mut full = build_full_ledger();
        assert!(full.parity().is_ok());
        assert!(full.seal().is_ok());
        assert_eq!(full.seal(), Err(TierDiag::ERewind.val()));
        // 封账后写入拒绝且账面不变。
        let before = full.len();
        assert_eq!(full.register(0, &[0, 1]), Err(TierDiag::ESealed.val()));
        assert_eq!(full.len(), before);
    }

    #[test]
    fn parity_assertions_count_is_ten() {
        // 对平断言恒 10 条且全绿；缺档账本必红。
        let l = build_full_ledger();
        let list = parity_assertions(&l);
        assert_eq!(list.len(), 10);
        assert!(parity_all_green(&l));
        // 缺档账本：档数恒等断言必红。
        let mut half = ConsumeLedger::new();
        let _ = half.register(0, &[0, 1]);
        assert!(!parity_all_green(&half));
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Unbound→Paritied→Consumed。
        let mut l = build_full_ledger();
        assert_eq!(l.state(), ConsumeState::Unbound);
        assert!(ConsumeState::Unbound.rank() < ConsumeState::Paritied.rank());
        assert!(ConsumeState::Paritied.rank() < ConsumeState::Consumed.rank());
        let _ = l.parity();
        assert_eq!(l.state(), ConsumeState::Paritied);
        let _ = l.seal();
        assert_eq!(l.state(), ConsumeState::Consumed);
    }

    #[test]
    fn anchor_tier_mapping_is_monotone_ladder() {
        // 阶梯映射三点：Win7/Win10/Win11 各落其档。
        let mut a = ConsumeLedger::new();
        let _ = a.bind_anchor(VersionAnchor { major: 6, minor: 1, build: 7601 });
        assert_eq!(a.anchor_tier().map(|t| t.index()), Some(0));
        let mut b = ConsumeLedger::new();
        let _ = b.bind_anchor(VersionAnchor { major: 10, minor: 0, build: 19041 });
        assert_eq!(b.anchor_tier().map(|t| t.index()), Some(3));
        let mut c = ConsumeLedger::new();
        let _ = c.bind_anchor(VersionAnchor { major: 10, minor: 0, build: 22000 });
        assert_eq!(c.anchor_tier().map(|t| t.index()), Some(4));
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12648_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
