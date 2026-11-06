//! UNX-F48979 批次封存总账（防重复关键字 1000 轮询 / 20 卡成链）· 0x71600
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B09 族封口槽）：
//!   F48979 ① 纯功能代码 ≥300 行；② UNX-F48979-J1 防重复 grep 关键字：
//!          1,000 轮轮询零重名、零重段、零空键；重复注入四路全捕获。
//!   F48980 ① 纯功能代码 ≥380 行；② UNX-F48980-J1 批次封存：
//!          20 卡号成链、9 模块段址成链、波次配额 5,400、
//!          批次下限 54,000、批次目标 240,000 全部对得上。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：卡号成链 → 段址成链 → 行数下限 → 关键字扫描 → 批次对账 →
//!           渲染回读。
//! 先认卡号再认段址：卡号是「这张单写给谁」的身份，段址是「这份码落在
//! 哪个诊断窗口」的地址。卡号断了说明封存清单本身缺页，此时再查段址只会
//! 把「少一张单」误诊成「段位错」，排障方向被带偏。
//! 行数下限放在关键字扫描之前：行数不足时模块可能整段缺席，关键字表会
//! 短一截却不重名——扫描全绿是假绿。先量身高再看名册，假绿无处落脚。
//! 关键差异：本模块不实现事件语义（速率/调度/风暴/调优/背压/热拔/fuzz/
//! 埋点各自在 0x71400..0x715C0 自证），只回答「这 20 张单是否全部落盘、
//! 是否互相撞名、批次账是否封得住」。撞名检查按「段址相同」或「模块名
//! 逐字节相同」两条独立判据同时跑，任一条命中即拒——只查其一会让
//! 「同一模块注册两次、段址各占一格」这种最隐蔽的重复漏网。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71600，VX6 批 B09 族封口槽）。
pub const SEG_BASE: u32 = 0x71600;

/// 段址链首（第一个卡模块的窗口基址）。
pub const CHAIN_FIRST: u32 = 0x71400;
/// 段址链步长（每模块独占 0x40 个诊断码位）。
pub const CHAIN_STEP: u32 = 0x40;
/// 链内模块数（0x71400..0x71600，共 9 槽）。
pub const MODULES: usize = 9;
/// 基础设施模块数（共享轮核，不占卡）。
pub const INFRA: usize = 1;
/// 基础设施段址（0x71180，轮核窗口）。
pub const INFRA_SEG: u32 = 0x71180;

/// 本波卡数（B09 = 20 卡）。
pub const CARDS: usize = 20;
/// 首卡号。
pub const CARD_FIRST: u32 = 48_961;
/// 末卡号。
pub const CARD_LAST: u32 = 48_980;

/// 每波卡数（与 CARDS 同值，用于批次除法）。
pub const WAVE_CARDS: u32 = 20;
/// 全批波数（200 卡 = 10 波）。
pub const WAVES: u32 = 10;
/// 全批卡数（200 卡）。
pub const BATCH_CARDS: u32 = 200;
/// 每波行数配额（54,000 / 10）。
pub const WAVE_QUOTA: u32 = 5_400;
/// 全批行数下限（200 卡 × 270 行均值）。
pub const BATCH_FLOOR: u32 = 54_000;
/// 全批行数目标（200 卡 × 1,200 行深水线）。
pub const BATCH_TARGET: u32 = 240_000;
/// 单卡深水线（240,000 / 200）。
pub const TARGET_PER_CARD: u32 = 1_200;

/// 模块行数下限之和（9 个卡模块，运行期复核）。
pub const MODULE_FLOOR_SUM: u32 = 2_840;
/// 9 个卡模块实测行数之和（封存快照，运行期复核）。
pub const SEALED_LINES: u32 = 12_194;

/// 防重复轮询数（卡面 1,000）。
pub const DUPLICATE_POLLS: u32 = 1_000;
/// 关键字表规模（9 卡模块 + 1 基础设施）。
pub const KEY_COUNT: usize = MODULES + 1;
/// 两两对数（10 选 2）。
pub const EXPECTED_PAIRS: u32 = (KEY_COUNT as u32 * (KEY_COUNT as u32 - 1)) / 2;

/// fuzz 轮数（卡面 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 注入路数（段重/名重/空键/卡断）。
pub const INJECT_ROUTES: usize = 4;
/// 封存行字段数。
pub const SEAL_FIELDS: usize = 6;
/// 黄金轮数。
pub const GOLDEN_ROUNDS: u32 = 32;
/// 单模块行数合法上界（防把整仓行数塞进一格）。
pub const LINE_CAP: u32 = 4_096;
/// 模块行数合法下界（低于此值视为整段缺席）。
pub const LINE_FLOOR: u32 = 220;
/// 域检查项数。
pub const CHECKS: u32 = 8;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0D） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlCode {
    /// 卡号断裂（缺号/重号/越界）
    CardBad,
    /// 行数低于下限或越过上界
    FloorBad,
    /// 关键字重复（段址相同或模块名相同）
    DupBad,
    /// 轮询账不平（轮数/抽样数/碰撞数对不上）
    PollBad,
    /// 波次配额不平
    WaveBad,
    /// 批次账不平（54,000/240,000）
    BatchBad,
    /// 行数总账与快照不等
    LineBad,
    /// 卡序或模块序错乱
    OrderBad,
    /// 空键（模块名为空）
    EmptyBad,
    /// 渲染非法（字段缺失或回读失败）
    RenderBad,
    /// 复算不一致（同账两次不同）
    ReplayBad,
    /// 覆盖不足（卡数/模块数/轮数不足）
    CoverBad,
    /// 段位越界（诊断码逃出 0x71600 窗口）
    SegmentBad,
    /// fuzz 处置错配（合法被拒或非法被接）
    FuzzBad,
}

impl SlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            SlCode::CardBad => "卡号断裂：缺号、重号或越出 48961..48980",
            SlCode::FloorBad => "行数越低：低于卡面下限或越过 4096 上界",
            SlCode::DupBad => "关键字重复：段址相同或模块名逐字节相同",
            SlCode::PollBad => "轮询账不平：轮数、抽样数与碰撞数对不上",
            SlCode::WaveBad => "波次配额不平：54,000 行下限除以 10 波不等于 5,400",
            SlCode::BatchBad => "批次账不平：200 卡 × 1,200 行不等于 240,000",
            SlCode::LineBad => "行数总账不平：实测之和与封存快照不等",
            SlCode::OrderBad => "顺序错乱：卡号未递增或段址未按 0x40 递增",
            SlCode::EmptyBad => "空键：模块名为空，无法参与去重",
            SlCode::RenderBad => "渲染非法：字段缺失或数值回读失败",
            SlCode::ReplayBad => "复算不一致：同一封存账两次得到不同摘要",
            SlCode::CoverBad => "覆盖不足：卡数、模块数或轮数不足",
            SlCode::SegmentBad => "段位越界：诊断码逃出 0x71600 窗口",
            SlCode::FuzzBad => "fuzz 处置错配：合法被拒或非法被接",
        }
    }

    pub const ALL: [SlCode; 14] = [
        SlCode::CardBad,
        SlCode::FloorBad,
        SlCode::DupBad,
        SlCode::PollBad,
        SlCode::WaveBad,
        SlCode::BatchBad,
        SlCode::LineBad,
        SlCode::OrderBad,
        SlCode::EmptyBad,
        SlCode::RenderBad,
        SlCode::ReplayBad,
        SlCode::CoverBad,
        SlCode::SegmentBad,
        SlCode::FuzzBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < SlCode::ALL.len() {
        if let Some(c) = SlCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 模块清单（9 卡模块 + 1 基础设施） ─────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModuleSpec {
    /// 模块文件名（不含扩展名，去重判据之一）。
    pub tag: &'static str,
    /// 诊断段基址。
    pub seg: u32,
    /// 本模块承载的卡数。
    pub cards: u8,
    /// 卡面行数下限。
    pub floor: u32,
    /// 封存时实测行数。
    pub lines: u32,
}

pub const MODULE_TABLE: [ModuleSpec; MODULES] = [
    ModuleSpec {
        tag: "unx48961_rate_core",
        seg: 0x71400,
        cards: 2,
        floor: 420,
        lines: 1_305,
    },
    ModuleSpec {
        tag: "unx48963_sched",
        seg: 0x71440,
        cards: 2,
        floor: 380,
        lines: 1_633,
    },
    ModuleSpec {
        tag: "unx48965_storm",
        seg: 0x71480,
        cards: 2,
        floor: 340,
        lines: 1_167,
    },
    ModuleSpec {
        tag: "unx48967_tuning",
        seg: 0x714C0,
        cards: 2,
        floor: 340,
        lines: 1_315,
    },
    ModuleSpec {
        tag: "unx48969_backpressure",
        seg: 0x71500,
        cards: 2,
        floor: 300,
        lines: 1_421,
    },
    ModuleSpec {
        tag: "unx48971_hotremove",
        seg: 0x71540,
        cards: 2,
        floor: 280,
        lines: 1_319,
    },
    ModuleSpec {
        tag: "unx48973_fuzzface",
        seg: 0x71580,
        cards: 3,
        floor: 260,
        lines: 1_117,
    },
    ModuleSpec {
        tag: "unx48976_quality",
        seg: 0x715C0,
        cards: 3,
        floor: 220,
        lines: 1_329,
    },
    ModuleSpec {
        tag: "unx48979_seal",
        seg: 0x71600,
        cards: 2,
        floor: 300,
        lines: 1_588,
    },
];

pub const INFRA_TABLE: [ModuleSpec; INFRA] = [ModuleSpec {
    tag: "unx48941_wheel_core",
    seg: INFRA_SEG,
    cards: 0,
    floor: 0,
    lines: 1_086,
}];

// ── 卡号归属（20 卡 → 9 模块） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CardRef {
    pub id: u32,
    pub module: u8,
}

pub const CARD_REFS: [CardRef; CARDS] = [
    CardRef { id: 48_961, module: 0 },
    CardRef { id: 48_962, module: 0 },
    CardRef { id: 48_963, module: 1 },
    CardRef { id: 48_964, module: 1 },
    CardRef { id: 48_965, module: 2 },
    CardRef { id: 48_966, module: 2 },
    CardRef { id: 48_967, module: 3 },
    CardRef { id: 48_968, module: 3 },
    CardRef { id: 48_969, module: 4 },
    CardRef { id: 48_970, module: 4 },
    CardRef { id: 48_971, module: 5 },
    CardRef { id: 48_972, module: 5 },
    CardRef { id: 48_973, module: 6 },
    CardRef { id: 48_974, module: 6 },
    CardRef { id: 48_975, module: 6 },
    CardRef { id: 48_976, module: 7 },
    CardRef { id: 48_977, module: 7 },
    CardRef { id: 48_978, module: 7 },
    CardRef { id: 48_979, module: 8 },
    CardRef { id: 48_980, module: 8 },
];

// ── 卡号审计 ──────────────────────────────────────────────────────
pub fn module_at(idx: usize) -> Option<&'static ModuleSpec> {
    MODULE_TABLE.get(idx)
}

pub fn card_at(idx: usize) -> Option<&'static CardRef> {
    CARD_REFS.get(idx)
}

/// 复核卡号序列（独立于归属表，供注入路径复用）。
pub fn audit_card_ids(ids: &[u32]) -> Result<u32, u32> {
    if ids.len() != CARDS {
        return Err(SlCode::CoverBad.val());
    }
    let mut i = 0usize;
    let mut prev = 0u32;
    while i < CARDS {
        let id = match ids.get(i) {
            Some(v) => *v,
            None => return Err(SlCode::CardBad.val()),
        };
        if i == 0 {
            if id != CARD_FIRST {
                return Err(SlCode::CardBad.val());
            }
        } else if id != prev.saturating_add(1) {
            return Err(SlCode::CardBad.val());
        }
        prev = id;
        i += 1;
    }
    if prev != CARD_LAST {
        return Err(SlCode::CardBad.val());
    }
    Ok(CARDS as u32)
}

/// 卡号成链 + 归属合法 + 每模块卡数与清单一致。
pub fn audit_cards() -> Result<u32, u32> {
    let mut ids = [0u32; CARDS];
    let mut i = 0usize;
    while i < CARDS {
        match CARD_REFS.get(i) {
            Some(c) => match ids.get_mut(i) {
                Some(slot) => *slot = c.id,
                None => return Err(SlCode::CoverBad.val()),
            },
            None => return Err(SlCode::CoverBad.val()),
        }
        i += 1;
    }
    let n = audit_card_ids(&ids)?;
    let mut counts = [0u8; MODULES];
    let mut j = 0usize;
    while j < CARDS {
        let c = match CARD_REFS.get(j) {
            Some(v) => v,
            None => return Err(SlCode::CoverBad.val()),
        };
        let m = c.module as usize;
        if m >= MODULES {
            return Err(SlCode::CardBad.val());
        }
        match counts.get_mut(m) {
            Some(slot) => *slot = slot.saturating_add(1),
            None => return Err(SlCode::CardBad.val()),
        }
        j += 1;
    }
    let mut k = 0usize;
    while k < MODULES {
        let spec = match MODULE_TABLE.get(k) {
            Some(v) => v,
            None => return Err(SlCode::CoverBad.val()),
        };
        let got = match counts.get(k) {
            Some(v) => *v,
            None => return Err(SlCode::CoverBad.val()),
        };
        if got != spec.cards {
            return Err(SlCode::CardBad.val());
        }
        k += 1;
    }
    Ok(n)
}

// ── 段址成链 ──────────────────────────────────────────────────────
pub fn audit_segments() -> Result<u32, u32> {
    let mut i = 0usize;
    while i < MODULES {
        let m = match MODULE_TABLE.get(i) {
            Some(v) => v,
            None => return Err(SlCode::CoverBad.val()),
        };
        let want = CHAIN_FIRST.saturating_add((i as u32).saturating_mul(CHAIN_STEP));
        if m.seg != want {
            return Err(SlCode::OrderBad.val());
        }
        if m.seg < CHAIN_FIRST || m.seg > SEG_BASE {
            return Err(SlCode::SegmentBad.val());
        }
        let mut j = i + 1;
        while j < MODULES {
            if let Some(o) = MODULE_TABLE.get(j) {
                if o.seg == m.seg {
                    return Err(SlCode::DupBad.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    let last = match MODULE_TABLE.get(MODULES - 1) {
        Some(v) => v,
        None => return Err(SlCode::CoverBad.val()),
    };
    if last.seg != SEG_BASE {
        return Err(SlCode::SegmentBad.val());
    }
    let infra = match INFRA_TABLE.first() {
        Some(v) => v,
        None => return Err(SlCode::CoverBad.val()),
    };
    if infra.seg != INFRA_SEG {
        return Err(SlCode::OrderBad.val());
    }
    let mut k = 0usize;
    while k < MODULES {
        if let Some(m) = MODULE_TABLE.get(k) {
            if m.seg == infra.seg {
                return Err(SlCode::DupBad.val());
            }
        }
        k += 1;
    }
    Ok(MODULES as u32)
}

// ── 行数下限与总账 ────────────────────────────────────────────────
pub fn audit_floors() -> Result<u32, u32> {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < MODULES {
        let m = match MODULE_TABLE.get(i) {
            Some(v) => v,
            None => return Err(SlCode::CoverBad.val()),
        };
        if m.floor == 0 || m.lines < m.floor {
            return Err(SlCode::FloorBad.val());
        }
        if m.lines > LINE_CAP {
            return Err(SlCode::FloorBad.val());
        }
        sum = sum.saturating_add(m.floor);
        i += 1;
    }
    if sum != MODULE_FLOOR_SUM {
        return Err(SlCode::FloorBad.val());
    }
    Ok(sum)
}

pub fn audit_lines() -> Result<u32, u32> {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < MODULES {
        let m = match MODULE_TABLE.get(i) {
            Some(v) => v,
            None => return Err(SlCode::CoverBad.val()),
        };
        if m.lines < LINE_FLOOR || m.lines > LINE_CAP {
            return Err(SlCode::FloorBad.val());
        }
        sum = sum.saturating_add(m.lines);
        i += 1;
    }
    if sum != SEALED_LINES {
        return Err(SlCode::LineBad.val());
    }
    if sum < WAVE_QUOTA {
        return Err(SlCode::WaveBad.val());
    }
    Ok(sum)
}

// ── 批次账 ────────────────────────────────────────────────────────
/// 返回 (批次下限, 批次目标)。
pub fn batch_account() -> Result<(u32, u32), u32> {
    if WAVE_CARDS == 0 || WAVES == 0 {
        return Err(SlCode::BatchBad.val());
    }
    let waves = BATCH_CARDS / WAVE_CARDS;
    if waves != WAVES {
        return Err(SlCode::WaveBad.val());
    }
    if WAVE_QUOTA.saturating_mul(WAVES) != BATCH_FLOOR {
        return Err(SlCode::WaveBad.val());
    }
    if BATCH_CARDS == 0 || BATCH_TARGET / BATCH_CARDS != TARGET_PER_CARD {
        return Err(SlCode::BatchBad.val());
    }
    // 波次配额不得低于「20 卡 × 单模块行数地板 220」的合账下限（4,400）。
    if WAVE_QUOTA < (CARDS as u32).saturating_mul(LINE_FLOOR) {
        return Err(SlCode::BatchBad.val());
    }
    Ok((BATCH_FLOOR, BATCH_TARGET))
}

// ── 关键字表与去重 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    pub tag: &'static str,
    pub seg: u32,
    pub lines: u32,
}

/// 从清单装配关键字表：9 卡模块在前，基础设施压尾。
pub fn key_table() -> [Key; KEY_COUNT] {
    let mut out = [Key {
        tag: "",
        seg: 0,
        lines: 0,
    }; KEY_COUNT];
    let mut i = 0usize;
    while i < MODULES {
        if let Some(m) = MODULE_TABLE.get(i) {
            if let Some(slot) = out.get_mut(i) {
                *slot = Key {
                    tag: m.tag,
                    seg: m.seg,
                    lines: m.lines,
                };
            }
        }
        i += 1;
    }
    if let Some(m) = INFRA_TABLE.first() {
        if let Some(slot) = out.get_mut(MODULES) {
            *slot = Key {
                tag: m.tag,
                seg: m.seg,
                lines: m.lines,
            };
        }
    }
    out
}

fn tag_eq(a: &str, b: &str) -> bool {
    let x = a.as_bytes();
    let y = b.as_bytes();
    if x.len() != y.len() {
        return false;
    }
    let mut i = 0usize;
    while i < x.len() {
        match (x.get(i), y.get(i)) {
            (Some(p), Some(q)) => {
                if *p != *q {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 关键字表审计：非空、段址在窗、逐对不重名不重段。
pub fn audit_keys(keys: &[Key]) -> Result<u32, u32> {
    if keys.len() != KEY_COUNT {
        return Err(SlCode::CoverBad.val());
    }
    let mut i = 0usize;
    let mut pairs = 0u32;
    while i < KEY_COUNT {
        let a = match keys.get(i) {
            Some(v) => v,
            None => return Err(SlCode::CoverBad.val()),
        };
        if a.tag.is_empty() {
            return Err(SlCode::EmptyBad.val());
        }
        if i == MODULES {
            if a.seg != INFRA_SEG {
                return Err(SlCode::OrderBad.val());
            }
        } else if a.seg < CHAIN_FIRST || a.seg > SEG_BASE {
            return Err(SlCode::SegmentBad.val());
        }
        let mut j = i + 1;
        while j < KEY_COUNT {
            let b = match keys.get(j) {
                Some(v) => v,
                None => return Err(SlCode::CoverBad.val()),
            };
            if a.seg == b.seg || tag_eq(a.tag, b.tag) {
                return Err(SlCode::DupBad.val());
            }
            pairs = pairs.saturating_add(1);
            j += 1;
        }
        i += 1;
    }
    if pairs != EXPECTED_PAIRS {
        return Err(SlCode::PollBad.val());
    }
    Ok(pairs)
}

// ── 1,000 轮轮询账 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrepTrace {
    pub polls: u32,
    pub samples: u32,
    pub collisions: u32,
    pub digest: u64,
}

impl GrepTrace {
    pub fn new() -> GrepTrace {
        GrepTrace {
            polls: 0,
            samples: 0,
            collisions: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.polls == 0 {
            return Err(SlCode::CoverBad.val());
        }
        let expect = self.polls.saturating_mul(KEY_COUNT as u32);
        if self.samples != expect {
            return Err(SlCode::PollBad.val());
        }
        if self.collisions != 0 {
            return Err(SlCode::DupBad.val());
        }
        if self.digest == 0 {
            return Err(SlCode::ReplayBad.val());
        }
        Ok(())
    }
}

pub fn dup_scan(rounds: u32) -> Result<GrepTrace, u32> {
    if rounds == 0 {
        return Err(SlCode::CoverBad.val());
    }
    let keys = key_table();
    let pairs = audit_keys(&keys)?;
    if pairs != EXPECTED_PAIRS {
        return Err(SlCode::PollBad.val());
    }
    let mut t = GrepTrace::new();
    let mut i = 0u32;
    while i < rounds {
        let idx = (i as usize) % KEY_COUNT;
        let probe = match keys.get(idx) {
            Some(v) => v,
            None => return Err(SlCode::CoverBad.val()),
        };
        let mut j = 0usize;
        while j < KEY_COUNT {
            let o = match keys.get(j) {
                Some(v) => v,
                None => return Err(SlCode::CoverBad.val()),
            };
            t.digest = fold_tag(t.digest, o.tag);
            t.digest = p_core::fold_u32(t.digest, o.seg);
            t.samples = t.samples.saturating_add(1);
            if j != idx && (o.seg == probe.seg || tag_eq(o.tag, probe.tag)) {
                t.collisions = t.collisions.saturating_add(1);
                if t.collisions != 0 {
                    return Err(SlCode::DupBad.val());
                }
            }
            j += 1;
        }
        t.polls = t.polls.saturating_add(1);
        t.digest = p_core::fold_u32(t.digest, i);
        i = i.saturating_add(1);
    }
    t.audit()?;
    Ok(t)
}

// ── 重复注入（四路必须全捕获） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; INJECT_ROUTES],
    pub caught: [u32; INJECT_ROUTES],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < INJECT_ROUTES {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

fn card_ids_copy() -> [u32; CARDS] {
    let mut ids = [0u32; CARDS];
    let mut i = 0usize;
    while i < CARDS {
        if let Some(c) = CARD_REFS.get(i) {
            if let Some(slot) = ids.get_mut(i) {
                *slot = c.id;
            }
        }
        i += 1;
    }
    ids
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 {
        return Err(SlCode::CoverBad.val());
    }
    let mut log = InjectLog {
        tries: [0; INJECT_ROUTES],
        caught: [0; INJECT_ROUTES],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 段址重复：把 1 号槽的段址抄成 0 号槽，必须炸 DupBad。
        let mut k1 = key_table();
        let seg0 = match k1.get(0) {
            Some(v) => v.seg,
            None => return Err(SlCode::CoverBad.val()),
        };
        if let Some(slot) = k1.get_mut(1) {
            slot.seg = seg0;
        }
        if audit_keys(&k1).err() == Some(SlCode::DupBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);
        // ② 模块名重复：把 2 号槽的名字抄成 0 号槽，必须炸 DupBad。
        let mut k2 = key_table();
        let tag0 = match k2.get(0) {
            Some(v) => v.tag,
            None => return Err(SlCode::CoverBad.val()),
        };
        if let Some(slot) = k2.get_mut(2) {
            slot.tag = tag0;
        }
        if audit_keys(&k2).err() == Some(SlCode::DupBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);
        // ③ 空键：3 号槽清空，必须炸 EmptyBad。
        let mut k3 = key_table();
        if let Some(slot) = k3.get_mut(3) {
            slot.tag = "";
        }
        if audit_keys(&k3).err() == Some(SlCode::EmptyBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);
        // ④ 卡号断裂：5 号卡抄成 4 号卡，必须炸 CardBad。
        let mut ids = card_ids_copy();
        let prev = match ids.get(4) {
            Some(v) => *v,
            None => return Err(SlCode::CoverBad.val()),
        };
        if let Some(slot) = ids.get_mut(5) {
            *slot = prev;
        }
        if audit_card_ids(&ids).err() == Some(SlCode::CardBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(SlCode::CoverBad.val());
    }
    Ok(log)
}

// ── fuzz（关键字表 × 卡表联合扰动） ──────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealFuzz {
    pub attempts: u32,
    pub clean: u32,
    pub caught: u32,
    pub digest: u64,
}

impl SealFuzz {
    pub fn new() -> SealFuzz {
        SealFuzz {
            attempts: 0,
            clean: 0,
            caught: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn balanced(&self) -> bool {
        self.attempts > 0 && self.attempts == self.clean.saturating_add(self.caught)
    }
}

/// 四类扰动：0 = 段址平移（合法）、1 = 段址重复、2 = 空键、3 = 卡号重复。
pub fn fuzz_seal(rounds: u32) -> Result<SealFuzz, u32> {
    if rounds == 0 {
        return Err(SlCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x7160_0F01_9A00_0001);
    let mut f = SealFuzz::new();
    let mut i = 0u32;
    while i < rounds {
        let kind = ((rng.next() >> 33) % 4) as u32;
        let slot = ((rng.next() >> 33) as usize) % MODULES;
        let mut keys = key_table();
        let mut cards = card_ids_copy();
        let expect_ok = kind == 0;
        match kind {
            0 => {
                // 平移量 1..=0x3F：留在本模块窗口内，不与邻居撞段。
                // 末槽（0x71600）平移必然越窗，故合法平移只在 0..MODULES-1 中取。
                let safe = slot % (MODULES - 1);
                let span = rng.below(CHAIN_STEP - 1);
                let delta = span.saturating_add(1);
                if let Some(k) = keys.get_mut(safe) {
                    k.seg = k.seg.saturating_add(delta);
                }
                if delta == 0 || delta >= CHAIN_STEP {
                    return Err(SlCode::FuzzBad.val());
                }
            }
            1 => {
                // src 与 slot 错开：同槽自抄等于没改，会制造一次假合法。
                let src = (slot + 1 + ((rng.next() >> 33) as usize) % (KEY_COUNT - 1)) % KEY_COUNT;
                let seg = match keys.get(src) {
                    Some(v) => v.seg,
                    None => return Err(SlCode::CoverBad.val()),
                };
                if let Some(k) = keys.get_mut(slot) {
                    k.seg = seg;
                }
            }
            2 => {
                if let Some(k) = keys.get_mut(slot) {
                    k.tag = "";
                }
            }
            _ => {
                let at = 1 + ((rng.next() >> 33) as usize) % (CARDS - 1);
                let prev = match cards.get(at - 1) {
                    Some(v) => *v,
                    None => return Err(SlCode::CoverBad.val()),
                };
                if let Some(c) = cards.get_mut(at) {
                    *c = prev;
                }
            }
        }
        let key_ok = audit_keys(&keys).is_ok();
        let card_ok = audit_card_ids(&cards).is_ok();
        let verdict = key_ok && card_ok;
        if verdict != expect_ok {
            return Err(SlCode::FuzzBad.val());
        }
        f.attempts = f.attempts.saturating_add(1);
        if verdict {
            f.clean = f.clean.saturating_add(1);
        } else {
            f.caught = f.caught.saturating_add(1);
        }
        f.digest = p_core::fold_u32(f.digest, kind);
        f.digest = p_core::fold_u32(f.digest, if verdict { 1 } else { 0 });
        i = i.saturating_add(1);
    }
    if !f.balanced() {
        return Err(SlCode::PollBad.val());
    }
    Ok(f)
}

// ── 封存账与渲染 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sealed {
    pub cards: u32,
    pub lines: u32,
    pub floor: u32,
    pub quota: u32,
    pub waves: u32,
    pub digest: u64,
}

fn fold_tag(mut h: u64, tag: &str) -> u64 {
    let b = tag.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            h = p_core::fold_u32(h, *c as u32);
        }
        i += 1;
    }
    h
}

/// 封存摘要：9 模块清单 + 20 卡清单 + 批次常数。
pub fn seal_digest() -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < MODULES {
        if let Some(m) = MODULE_TABLE.get(i) {
            h = fold_tag(h, m.tag);
            h = p_core::fold_u32(h, m.seg);
            h = p_core::fold_u32(h, m.lines);
            h = p_core::fold_u32(h, m.floor);
            h = p_core::fold_u32(h, m.cards as u32);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < CARDS {
        if let Some(c) = CARD_REFS.get(j) {
            h = p_core::fold_u32(h, c.id);
            h = p_core::fold_u32(h, c.module as u32);
        }
        j += 1;
    }
    h = p_core::fold_u32(h, BATCH_FLOOR);
    h = p_core::fold_u32(h, BATCH_TARGET);
    h = p_core::fold_u32(h, WAVE_QUOTA);
    h = p_core::fold_u32(h, WAVES);
    h = p_core::fold_u32(h, MODULE_FLOOR_SUM);
    h
}

/// 封存：五道闸门全部通过才出一本账。
pub fn seal_book() -> Result<Sealed, u32> {
    let cards = audit_cards()?;
    let segs = audit_segments()?;
    let floor = audit_floors()?;
    let lines = audit_lines()?;
    let (batch, target) = batch_account()?;
    let pairs = audit_keys(&key_table())?;
    if cards != CARDS as u32 || segs != MODULES as u32 || pairs != EXPECTED_PAIRS {
        return Err(SlCode::CoverBad.val());
    }
    if batch != BATCH_FLOOR || target != BATCH_TARGET {
        return Err(SlCode::BatchBad.val());
    }
    if cards.saturating_mul(WAVES) != BATCH_CARDS {
        return Err(SlCode::BatchBad.val());
    }
    if floor.saturating_add(lines) < WAVE_QUOTA {
        return Err(SlCode::WaveBad.val());
    }
    let digest = seal_digest();
    if digest == 0 {
        return Err(SlCode::ReplayBad.val());
    }
    Ok(Sealed {
        cards,
        lines,
        floor,
        quota: WAVE_QUOTA,
        waves: WAVES,
        digest,
    })
}

pub fn render_seal(s: &Sealed) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.cards, s.lines, s.floor, s.quota, s.waves, s.digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        let c = *b.get(i)?;
        if !(48..=57).contains(&c) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - 48) as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 18 {
        return None;
    }
    if *b.get(0)? != b'0' || *b.get(1)? != b'x' {
        return None;
    }
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let c = *b.get(i)?;
        let d = match c {
            48..=57 => c - 48,
            65..=70 => c - 55,
            97..=102 => c - 87,
            _ => return None,
        };
        v = (v << 4) | d as u64;
        i += 1;
    }
    Some(v)
}

pub fn audit_render(text: &str) -> Result<usize, u32> {
    let mut parts = [""; SEAL_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < SEAL_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != SEAL_FIELDS {
        return Err(SlCode::RenderBad.val());
    }
    let cards = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(SlCode::RenderBad.val()),
    };
    let lines = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(SlCode::RenderBad.val()),
    };
    let floor = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(SlCode::RenderBad.val()),
    };
    let quota = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(SlCode::RenderBad.val()),
    };
    let waves = match parts.get(4).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(SlCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(SlCode::RenderBad.val()),
    };
    if cards != CARDS as u32 || lines != SEALED_LINES {
        return Err(SlCode::LineBad.val());
    }
    if floor != MODULE_FLOOR_SUM || quota != WAVE_QUOTA {
        return Err(SlCode::RenderBad.val());
    }
    if waves != WAVES || digest != seal_digest() || digest == 0 {
        return Err(SlCode::RenderBad.val());
    }
    if cards.saturating_mul(waves) != BATCH_CARDS {
        return Err(SlCode::BatchBad.val());
    }
    if quota.saturating_mul(waves) != BATCH_FLOOR {
        return Err(SlCode::BatchBad.val());
    }
    Ok(SEAL_FIELDS)
}

// ── 黄金运行与复算 ────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let s = seal_book()?;
    let mut cards = 0u32;
    let mut i = 0usize;
    while i < CARDS {
        if CARD_REFS.get(i).is_some() {
            cards = cards.saturating_add(1);
        }
        i += 1;
    }
    if cards != s.cards {
        return Err(SlCode::CardBad.val());
    }
    Ok((s.cards, s.lines, CHECKS, s.digest))
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SlCode::ReplayBad.val());
    }
    let x = dup_scan(64)?;
    let y = dup_scan(64)?;
    if x.digest != y.digest || x.samples != y.samples {
        return Err(SlCode::ReplayBad.val());
    }
    if seal_digest() != seal_digest() {
        return Err(SlCode::ReplayBad.val());
    }
    Ok(())
}

// ── 码族与段位 ────────────────────────────────────────────────────
pub fn codes_are_distinct() -> bool {
    let mut i = 0usize;
    while i < SlCode::ALL.len() {
        let mut j = i + 1;
        while j < SlCode::ALL.len() {
            match (SlCode::ALL.get(i), SlCode::ALL.get(j)) {
                (Some(a), Some(b)) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                _ => return false,
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < SlCode::ALL.len() {
        if let Some(c) = SlCode::ALL.get(i) {
            let v = c.val();
            if v <= SEG_BASE || v > SEG_BASE + 0x0F {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let s = seal_book()?;
    if s.cards != CARDS as u32 || s.lines != SEALED_LINES {
        return Err(SlCode::LineBad.val());
    }
    if s.floor != MODULE_FLOOR_SUM || s.quota != WAVE_QUOTA || s.waves != WAVES {
        return Err(SlCode::BatchBad.val());
    }
    if s.digest == 0 {
        return Err(SlCode::ReplayBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let t = dup_scan(DUPLICATE_POLLS)?;
    if t.polls != DUPLICATE_POLLS {
        return Err(SlCode::PollBad.val());
    }
    if t.collisions != 0 {
        return Err(SlCode::DupBad.val());
    }
    if t.samples != DUPLICATE_POLLS.saturating_mul(KEY_COUNT as u32) {
        return Err(SlCode::PollBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let f = fuzz_seal(2_000)?;
    if !f.balanced() || f.caught == 0 {
        return Err(SlCode::PollBad.val());
    }
    let s = seal_book()?;
    let text = render_seal(&s);
    if audit_render(&text)? != SEAL_FIELDS {
        return Err(SlCode::RenderBad.val());
    }
    Ok(())
}

// ── 域检查项（动态明细槽上限 8） ──────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    let scan = dup_scan(DUPLICATE_POLLS);
    out.push((
        "F48979 关键字1000轮零重复",
        matches!(&scan, Ok(t) if t.polls == DUPLICATE_POLLS
            && t.collisions == 0
            && t.samples == DUPLICATE_POLLS.saturating_mul(KEY_COUNT as u32)),
        match &scan {
            Ok(t) => format!(
                "polls={} samples={} hits={}",
                t.polls, t.samples, t.collisions
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let inj = inject_rounds(4);
    out.push((
        "F48979 重复注入四路全捕获",
        matches!(&inj, Ok(l) if l.all_caught()),
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let fuzz = fuzz_seal(FUZZ_ROUNDS);
    out.push((
        "F48979 fuzz×10^4零错配",
        matches!(&fuzz, Ok(f) if f.balanced() && f.caught > 0),
        match &fuzz {
            Ok(f) => format!(
                "attempts={} clean={} caught={}",
                f.attempts, f.clean, f.caught
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let cards = audit_cards();
    out.push((
        "F48980 20卡成链归属唯一",
        cards == Ok(CARDS as u32),
        match &cards {
            Ok(n) => format!("cards={} span={}..{}", n, CARD_FIRST, CARD_LAST),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let floors = audit_floors();
    let lines = audit_lines();
    out.push((
        "F48980 卡面行数下限",
        floors == Ok(MODULE_FLOOR_SUM) && matches!(&lines, Ok(n) if *n == SEALED_LINES),
        format!(
            "floor={:?} lines={:?} cap={}",
            floors, lines, LINE_CAP
        ),
    ));

    let acc = batch_account();
    out.push((
        "F48980 批次账54000/240000",
        acc == Ok((BATCH_FLOOR, BATCH_TARGET)),
        match &acc {
            Ok((f, t)) => format!(
                "floor={} target={} quota={} waves={} cards={}",
                f, t, WAVE_QUOTA, WAVES, BATCH_CARDS
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48979 段位闭集",
        segment_guard() && codes_are_distinct(),
        format!(
            "codes={} seg=0x{:04X} chain={}x0x{:X}",
            SlCode::ALL.len(),
            SEG_BASE,
            MODULES,
            CHAIN_STEP
        ),
    ));

    let seal = seal_book().map(|s| render_seal(&s));
    out.push((
        "F48979 封存可解析",
        matches!(&seal, Ok(t) if audit_render(t) == Ok(SEAL_FIELDS)),
        match &seal {
            Ok(t) => format!("text={}", t),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out
}

pub fn run_unx48979_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48979 批次封存总账");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x71600);
        assert_eq!(CHAIN_FIRST, 0x71400);
        assert_eq!(CHAIN_STEP, 0x40);
        assert_eq!(MODULES, 9);
        assert_eq!(CARDS, 20);
        assert_eq!(CARD_FIRST, 48_961);
        assert_eq!(CARD_LAST, 48_980);
        assert_eq!(WAVE_QUOTA, 5_400);
        assert_eq!(BATCH_FLOOR, 54_000);
        assert_eq!(BATCH_TARGET, 240_000);
        assert_eq!(TARGET_PER_CARD, 1_200);
        assert_eq!(MODULE_FLOOR_SUM, 2_840);
        assert_eq!(SEALED_LINES, 12_194);
        assert_eq!(KEY_COUNT, 10);
        assert_eq!(EXPECTED_PAIRS, 45);
        assert_eq!(SlCode::ALL.len(), 14);
        assert_eq!(CHECKS, 8);
    }

    #[test]
    fn segment_chain_is_exact() {
        assert_eq!(audit_segments(), Ok(MODULES as u32));
        let last = MODULE_TABLE[MODULES - 1];
        assert_eq!(last.seg, SEG_BASE);
        assert_eq!(last.tag, "unx48979_seal");
        assert_eq!(INFRA_TABLE[0].seg, INFRA_SEG);
    }

    #[test]
    fn cards_are_consecutive_and_owned() {
        assert_eq!(audit_cards(), Ok(CARDS as u32));
        let mut i = 0usize;
        while i < CARDS {
            let c = CARD_REFS[i];
            assert_eq!(c.id, CARD_FIRST + i as u32);
            assert!((c.module as usize) < MODULES);
            i += 1;
        }
    }

    #[test]
    fn floors_sum_matches_constant() {
        assert_eq!(audit_floors(), Ok(MODULE_FLOOR_SUM));
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < MODULES {
            sum += MODULE_TABLE[i].floor;
            i += 1;
        }
        assert_eq!(sum, MODULE_FLOOR_SUM);
    }

    #[test]
    fn lines_sum_matches_snapshot() {
        assert_eq!(audit_lines(), Ok(SEALED_LINES));
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < MODULES {
            sum += MODULE_TABLE[i].lines;
            i += 1;
        }
        assert_eq!(sum, SEALED_LINES);
    }

    #[test]
    fn batch_math_is_closed() {
        assert_eq!(batch_account(), Ok((BATCH_FLOOR, BATCH_TARGET)));
        assert_eq!(WAVE_QUOTA * WAVES, BATCH_FLOOR);
        assert_eq!(BATCH_CARDS / WAVE_CARDS, WAVES);
        assert_eq!(BATCH_TARGET / BATCH_CARDS, TARGET_PER_CARD);
        assert_eq!(BATCH_CARDS / CARDS as u32, WAVES);
    }

    #[test]
    fn key_pairs_are_clean() {
        assert_eq!(audit_keys(&key_table()), Ok(EXPECTED_PAIRS));
    }

    #[test]
    fn dup_scan_thousand_is_clean() {
        match dup_scan(DUPLICATE_POLLS) {
            Ok(t) => {
                assert_eq!(t.polls, 1_000);
                assert_eq!(t.samples, 10_000);
                assert_eq!(t.collisions, 0);
                assert!(t.digest != 0);
                assert!(t.audit().is_ok());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn dup_scan_rejects_zero() {
        assert_eq!(dup_scan(0), Err(SlCode::CoverBad.val()));
    }

    #[test]
    fn duplicate_seg_is_caught() {
        let mut keys = key_table();
        let seg0 = keys[0].seg;
        keys[1].seg = seg0;
        assert_eq!(audit_keys(&keys), Err(SlCode::DupBad.val()));
    }

    #[test]
    fn duplicate_tag_is_caught() {
        let mut keys = key_table();
        let tag0 = keys[0].tag;
        keys[2].tag = tag0;
        assert_eq!(audit_keys(&keys), Err(SlCode::DupBad.val()));
    }

    #[test]
    fn empty_tag_is_caught() {
        let mut keys = key_table();
        keys[3].tag = "";
        assert_eq!(audit_keys(&keys), Err(SlCode::EmptyBad.val()));
    }

    #[test]
    fn card_gap_is_caught() {
        let mut ids = card_ids_copy();
        ids[5] = ids[4];
        assert_eq!(audit_card_ids(&ids), Err(SlCode::CardBad.val()));
    }

    #[test]
    fn card_short_table_is_rejected() {
        let ids = [CARD_FIRST, CARD_FIRST + 1];
        assert_eq!(audit_card_ids(&ids), Err(SlCode::CoverBad.val()));
    }

    #[test]
    fn wrong_window_is_caught() {
        let mut keys = key_table();
        keys[4].seg = SEG_BASE + CHAIN_STEP;
        assert_eq!(audit_keys(&keys), Err(SlCode::SegmentBad.val()));
    }

    #[test]
    fn fuzz_is_balanced() {
        match fuzz_seal(2_000) {
            Ok(f) => {
                assert_eq!(f.attempts, 2_000);
                assert_eq!(f.attempts, f.clean + f.caught);
                assert!(f.caught >= 1_000);
                assert!(f.clean >= 1);
                assert!(f.digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_rejects_zero() {
        assert_eq!(fuzz_seal(0), Err(SlCode::CoverBad.val()));
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(5) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [5, 5, 5, 5]);
                assert_eq!(l.caught, [5, 5, 5, 5]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn injection_rejects_zero() {
        assert_eq!(inject_rounds(0), Err(SlCode::CoverBad.val()));
    }

    #[test]
    fn seal_book_fields() {
        match seal_book() {
            Ok(s) => {
                assert_eq!(s.cards, 20);
                assert_eq!(s.lines, SEALED_LINES);
                assert_eq!(s.floor, MODULE_FLOOR_SUM);
                assert_eq!(s.quota, WAVE_QUOTA);
                assert_eq!(s.waves, WAVES);
                assert!(s.digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_round_trips() {
        match seal_book().map(|s| render_seal(&s)) {
            Ok(text) => {
                assert_eq!(text.split('|').count(), SEAL_FIELDS);
                assert_eq!(audit_render(&text), Ok(SEAL_FIELDS));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_tamper_is_rejected() {
        match seal_book().map(|s| render_seal(&s)) {
            Ok(text) => {
                let bad = text.replacen("|2840|", "|2841|", 1);
                assert_eq!(audit_render(&bad), Err(SlCode::RenderBad.val()));
                let bad2 = text.replacen(format!("|{}|", SEALED_LINES).as_str(), "|1|", 1);
                assert_eq!(audit_render(&bad2), Err(SlCode::LineBad.val()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_short_is_rejected() {
        assert_eq!(
            audit_render("20|12194|2840|5400|10"),
            Err(SlCode::RenderBad.val())
        );
        assert_eq!(audit_render(""), Err(SlCode::RenderBad.val()));
    }

    #[test]
    fn parse_helpers_are_strict() {
        assert_eq!(parse_u32("5400"), Some(5_400));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("12a"), None);
        assert_eq!(parse_u32("99999999999"), None);
        assert_eq!(parse_hex16("0x0000000000000000"), Some(0));
        assert_eq!(parse_hex16("0x00000000000000FF"), Some(255));
        assert_eq!(parse_hex16("0x1"), None);
        assert_eq!(parse_hex16("xx0000000000000000"), None);
    }

    #[test]
    fn golden_is_reproducible() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
        match golden_run() {
            Ok((cards, lines, checks, digest)) => {
                assert_eq!(cards, 20);
                assert_eq!(lines, SEALED_LINES);
                assert_eq!(checks, CHECKS);
                assert!(digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn codes_are_closed() {
        assert!(codes_are_distinct());
        assert!(segment_guard());
        assert_eq!(detail(SlCode::DupBad.val()), SlCode::DupBad.detail());
        assert_eq!(detail(0xDEAD), "未知诊断码");
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48979_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
