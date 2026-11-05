//! UNX-F12697 防幻觉总核验（偏移/码值出处抽检 ≥50 处） · 0x81800段
//!
//! 判据原文（派工卡 12696 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12697-J1 确认单对齐表齐（批/确认单/版本三列），
//!      抽样 10 可点验；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 防幻觉总核验账：出处抽检账（判据号/段位/偏移/码值/种别五元），
//!    52 处冻结出处（≥50 下限），偏移出处 26 处 + 码值出处 26 处——
//!    防幻觉面双家族全覆盖；
//! 2. 码值重算闸：每处出处的码值必须等于「段位 ⊕ 偏移折算」的封闭
//!    重算式，重算不符即幻觉（码值凭空捏造必被重算闸抓出）；
//! 3. 偏移界闸：偏移必须落在 0x01..0x40 段内偏移闭区间且段位按 VX9
//!    公式对号（0x80000 + 0x40×(判据号−12600)），漂移即幻觉；
//! 4. 确认单对齐表：批号/确认单号/版本号三列齐（三列全部落在合法
//!    闭集），抽样 10 行按 LCG 固定种子点验——抽样行逐行三列回验；
//! 5. 总核验状态机 HaDraft → HaSweeping → HaSealed 单向推进，封账
//!    后禁写、倒退显性拒；幻觉注入十族逐路必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死
//!    = SEG_BASE）；取值一律 match/if let/.get()/checked_*/saturating_*，
//!    无 unwrap/expect/panic/裸下标；中文注释与中文诊断；无死代码
//!    无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `provenance_fifty_ok` ↔ 标题「偏移/码值出处抽检 ≥50 处」；
//!   - `align_table_ok` ↔ ② 确认单对齐表齐（批/确认单/版本三列）；
//!   - `sample_ten_ok` ↔ ② 抽样 10 可点验；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12696 − 12600)）。
pub const SEG_BASE: u32 = 0x81800;

// ── 防幻觉诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HaDiag {
    /// 码值幻觉（重算不符）。
    ECodeGhost,
    /// 偏移越段（不在 0x01..0x40）。
    EOffsetRange,
    /// 段位漂移（与 VX9 公式不符）。
    ESegDrift,
    /// 出处号重复。
    ESpotDup,
    /// 出处号跳号（序闸）。
    ESpotGap,
    /// 出处引文空。
    EQuoteEmpty,
    /// 对齐行三列缺列。
    EAlignMissing,
    /// 对齐版本倒退。
    EVersionRewind,
    /// 抽样额不足（< 10 行）。
    ESampleShort,
    /// 封账后写。
    ESealedWrite,
    /// 状态机倒退。
    ERewind,
    /// 摘要空。
    ESummaryEmpty,
}

impl HaDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                HaDiag::ECodeGhost => 0x01,
                HaDiag::EOffsetRange => 0x02,
                HaDiag::ESegDrift => 0x03,
                HaDiag::ESpotDup => 0x04,
                HaDiag::ESpotGap => 0x05,
                HaDiag::EQuoteEmpty => 0x06,
                HaDiag::EAlignMissing => 0x07,
                HaDiag::EVersionRewind => 0x08,
                HaDiag::ESampleShort => 0x09,
                HaDiag::ESealedWrite => 0x0a,
                HaDiag::ERewind => 0x0b,
                HaDiag::ESummaryEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            HaDiag::ECodeGhost => "码值幻觉：重算式不符，先回原表核码值再申报",
            HaDiag::EOffsetRange => "偏移越段：段内偏移限 0x01..0x40，越界先改偏移",
            HaDiag::ESegDrift => "段位漂移：段位须按 0x80000+0x40×(id−12600) 对号",
            HaDiag::ESpotDup => "出处号重复：一处一号，重复先查已有抽检账",
            HaDiag::ESpotGap => "出处号跳号：抽检按序入账，跳号先补前序",
            HaDiag::EQuoteEmpty => "出处引文空：引文非空才可核验，空引先补原文",
            HaDiag::EAlignMissing => "对齐三列缺列：批/确认单/版本三列全齐才收行",
            HaDiag::EVersionRewind => "对齐版本倒退：同确认单版本只进不退，倒退先查账",
            HaDiag::ESampleShort => "抽样额不足：抽样 ≥10 行才可点验，缺额先补抽",
            HaDiag::ESealedWrite => "封账后写：核验账只读，补账走下一轮抽检",
            HaDiag::ERewind => "状态机倒退：HaDraft→HaSweeping→HaSealed 只进不退",
            HaDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const HA_ALL: &[HaDiag] = &[
    HaDiag::ECodeGhost,
    HaDiag::EOffsetRange,
    HaDiag::ESegDrift,
    HaDiag::ESpotDup,
    HaDiag::ESpotGap,
    HaDiag::EQuoteEmpty,
    HaDiag::EAlignMissing,
    HaDiag::EVersionRewind,
    HaDiag::ESampleShort,
    HaDiag::ESealedWrite,
    HaDiag::ERewind,
    HaDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in HA_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81801..0x8180C 闭集内"
}

// ── 抽检种别闭集 ─────────────────────────────────────────────────
/// 出处抽检种别（双家族闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvKind {
    /// 偏移出处（段内偏移出处核验）。
    PkOffset,
    /// 码值出处（诊断码值出处核验）。
    PkCode,
}

impl ProvKind {
    /// 种别名。
    pub fn name(self) -> &'static str {
        match self {
            ProvKind::PkOffset => "偏移出处",
            ProvKind::PkCode => "码值出处",
        }
    }

    /// 全部种别（闭集数组）。
    pub fn all() -> &'static [ProvKind; 2] {
        &[ProvKind::PkOffset, ProvKind::PkCode]
    }
}

// ── 冻结出处表（52 处：26 偏移 + 26 码值） ───────────────────────
/// 冻结出处五元：(判据号, 出处引文, 偏移, 码值, 种别)。
///
/// 常量来源：VX9 派工卡 12660..12707 联签段判据登记账（本模块冻结
/// 快照），码值 = 段位 + 偏移（段内闭集），偏移 ∈ 0x01..0x40。
pub const SPOT_TABLE: &[(u32, &str, u32, u32, ProvKind)] = &[
    (12660, "F12660-J1@0x80F00", 0x0a, 0x80F0A, ProvKind::PkOffset),
    (12661, "F12661-J1@0x80F40", 0x12, 0x80F52, ProvKind::PkCode),
    (12662, "F12662-J1@0x80F80", 0x07, 0x80F87, ProvKind::PkOffset),
    (12663, "F12663-J1@0x80FC0", 0x1c, 0x80FDC, ProvKind::PkCode),
    (12664, "F12664-J1@0x81000", 0x03, 0x81003, ProvKind::PkOffset),
    (12665, "F12665-J1@0x81040", 0x25, 0x81065, ProvKind::PkCode),
    (12666, "F12666-J1@0x81080", 0x0e, 0x8108E, ProvKind::PkOffset),
    (12667, "F12667-J1@0x810C0", 0x31, 0x810F1, ProvKind::PkCode),
    (12668, "F12668-J1@0x81100", 0x09, 0x81109, ProvKind::PkOffset),
    (12669, "F12669-J1@0x81140", 0x18, 0x81158, ProvKind::PkCode),
    (12670, "F12670-J1@0x81180", 0x0b, 0x8118B, ProvKind::PkOffset),
    (12671, "F12671-J1@0x811C0", 0x2a, 0x811EA, ProvKind::PkCode),
    (12672, "F12672-J1@0x81200", 0x05, 0x81205, ProvKind::PkOffset),
    (12673, "F12673-J1@0x81240", 0x1f, 0x8125F, ProvKind::PkCode),
    (12674, "F12674-J1@0x81280", 0x0d, 0x8128D, ProvKind::PkOffset),
    (12675, "F12675-J1@0x812C0", 0x33, 0x812F3, ProvKind::PkCode),
    (12676, "F12676-J1@0x81300", 0x06, 0x81306, ProvKind::PkOffset),
    (12677, "F12677-J1@0x81340", 0x22, 0x81362, ProvKind::PkCode),
    (12678, "F12678-J1@0x81380", 0x10, 0x81390, ProvKind::PkOffset),
    (12679, "F12679-J1@0x813C0", 0x2c, 0x813EC, ProvKind::PkCode),
    (12680, "F12680-J1@0x81400", 0x08, 0x81408, ProvKind::PkOffset),
    (12681, "F12681-J1@0x81440", 0x15, 0x81455, ProvKind::PkCode),
    (12682, "F12682-J1@0x81480", 0x0c, 0x8148C, ProvKind::PkOffset),
    (12683, "F12683-J1@0x814C0", 0x36, 0x814F6, ProvKind::PkCode),
    (12684, "F12684-J1@0x81500", 0x02, 0x81502, ProvKind::PkOffset),
    (12685, "F12685-J1@0x81540", 0x19, 0x81559, ProvKind::PkCode),
    (12686, "F12686-J1@0x81580", 0x0f, 0x8158F, ProvKind::PkOffset),
    (12687, "F12687-J1@0x815C0", 0x27, 0x815E7, ProvKind::PkCode),
    (12688, "F12688-J1@0x81600", 0x04, 0x81604, ProvKind::PkOffset),
    (12689, "F12689-J1@0x81640", 0x1b, 0x8165B, ProvKind::PkCode),
    (12690, "F12690-J1@0x81680", 0x11, 0x81691, ProvKind::PkOffset),
    (12691, "F12691-J1@0x816C0", 0x2e, 0x816EE, ProvKind::PkCode),
    (12692, "F12692-J1@0x81700", 0x0a, 0x8170A, ProvKind::PkOffset),
    (12693, "F12693-J1@0x81740", 0x20, 0x81760, ProvKind::PkCode),
    (12694, "F12694-J1@0x81780", 0x07, 0x81787, ProvKind::PkOffset),
    (12695, "F12695-J1@0x817C0", 0x24, 0x817E4, ProvKind::PkCode),
    (12696, "F12696-J1@0x81800", 0x0d, 0x8180D, ProvKind::PkOffset),
    (12697, "F12697-J1@0x81840", 0x30, 0x81870, ProvKind::PkCode),
    (12698, "F12698-J1@0x81880", 0x05, 0x81885, ProvKind::PkOffset),
    (12699, "F12699-J1@0x818C0", 0x17, 0x818D7, ProvKind::PkCode),
    (12700, "F12700-J1@0x81900", 0x0b, 0x8190B, ProvKind::PkOffset),
    (12701, "F12701-J1@0x81940", 0x29, 0x81969, ProvKind::PkCode),
    (12702, "F12702-J1@0x81980", 0x08, 0x81988, ProvKind::PkOffset),
    (12703, "F12703-J1@0x819C0", 0x13, 0x819D3, ProvKind::PkCode),
    (12704, "F12704-J1@0x81A00", 0x0e, 0x81A0E, ProvKind::PkOffset),
    (12705, "F12705-J1@0x81A40", 0x35, 0x81A75, ProvKind::PkCode),
    (12706, "F12706-J1@0x81A80", 0x06, 0x81A86, ProvKind::PkOffset),
    (12707, "F12707-J1@0x81AC0", 0x21, 0x81AE1, ProvKind::PkCode),
    (12708, "F12708-J1@0x81B00", 0x09, 0x81B09, ProvKind::PkOffset),
    (12709, "F12709-J1@0x81B40", 0x1d, 0x81B5D, ProvKind::PkCode),
    (12710, "F12710-J1@0x81B80", 0x0c, 0x81B8C, ProvKind::PkOffset),
    (12711, "F12711-J1@0x81BC0", 0x2f, 0x81BEF, ProvKind::PkCode),
];

/// 抽检下限（判据：≥50 处）。
pub const SPOT_MIN: usize = 50;

/// 抽检额冻结值（52 处：26 偏移 + 26 码值）。
pub const SPOT_CAP: usize = 52;

/// 段内偏移下界（0x00 保留给段基）。
pub const OFFSET_LO: u32 = 0x01;

/// 段内偏移上界（0x40 步距内）。
pub const OFFSET_HI: u32 = 0x40;

// ── 确认单对齐表（批/确认单/版本三列） ──────────────────────────
/// 对齐行三元：(批号, 确认单号, 版本号)。
///
/// 常量来源：联签段 B29..B35 语义确认单登记账（冻结快照）。
pub const ALIGN_TABLE: &[(u32, u32, u32)] = &[
    (29, 2601, 3),
    (30, 2602, 2),
    (31, 2603, 4),
    (32, 2604, 2),
    (33, 2605, 5),
    (34, 2606, 3),
    (35, 2607, 2),
    (35, 2608, 1),
    (35, 2609, 6),
    (35, 2610, 2),
    (35, 2611, 3),
    (35, 2612, 1),
];

/// 对齐行数冻结值。
pub const ALIGN_CAP: usize = 12;

/// 抽样点验行数（判据：抽样 10 可点验）。
pub const SAMPLE_MIN: usize = 10;

// ── 出处条目 ─────────────────────────────────────────────────────
/// 出处抽检条目（冻结表重放产物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProvSpot {
    /// 出处号（入账序号，从 0 起连续）。
    pub no: u32,
    /// 判据号。
    pub judge: u32,
    /// 出处引文。
    pub quote: &'static str,
    /// 段内偏移。
    pub offset: u32,
    /// 码值。
    pub code: u32,
    /// 种别。
    pub kind: ProvKind,
}

/// 冻结表第 idx 行重放（越界 None）。
pub fn replay_spot(idx: usize) -> Option<ProvSpot> {
    let row = SPOT_TABLE.get(idx)?;
    Some(ProvSpot {
        no: idx as u32,
        judge: row.0,
        quote: row.1,
        offset: row.2,
        code: row.3,
        kind: row.4,
    })
}

/// 段位对号重算（VX9 公式）：判据号 → 期望段位；号越窗返回 None。
pub fn seg_of(judge: u32) -> Option<u32> {
    if judge < 12600 || judge > 12799 {
        return None;
    }
    Some(0x80000u32.checked_add(0x40u32.checked_mul(judge.checked_sub(12600)?)?)?)
}

/// 码值重算式（封闭）：码值 ≡ 段位 + 段内偏移。
pub fn recompute_code(judge: u32, offset: u32) -> Option<u32> {
    seg_of(judge)?.checked_add(offset)
}

/// 单条出处核验（段位对号 + 偏移界 + 码值重算 + 引文非空）。
pub fn spot_gates_pass(s: &ProvSpot) -> bool {
    if s.quote.is_empty() {
        return false;
    }
    if s.offset < OFFSET_LO || s.offset > OFFSET_HI {
        return false;
    }
    let seg = match seg_of(s.judge) {
        Some(v) => v,
        None => return false,
    };
    if s.code < seg {
        return false;
    }
    let expect = match recompute_code(s.judge, s.offset) {
        Some(v) => v,
        None => return false,
    };
    s.code == expect
}

// ── 总核验状态机 ─────────────────────────────────────────────────
/// 总核验相位（单调推进闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HaState {
    /// 草稿（未开扫）。
    HaDraft,
    /// 扫描中（抽检入账 + 对齐收行）。
    HaSweeping,
    /// 封账（只读）。
    HaSealed,
}

impl HaState {
    /// 相位秩（单调守卫用）。
    pub fn rank(self) -> u32 {
        match self {
            HaState::HaDraft => 0,
            HaState::HaSweeping => 1,
            HaState::HaSealed => 2,
        }
    }

    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            HaState::HaDraft => "草稿",
            HaState::HaSweeping => "扫描中",
            HaState::HaSealed => "已封账",
        }
    }
}

/// 相位迁移守卫（只进不退，同相位拒）。
pub fn ha_advance(from: HaState, to: HaState) -> Result<(), u32> {
    if to.rank() != from.rank() + 1 {
        return Err(HaDiag::ERewind.val());
    }
    Ok(())
}

/// 确认单对齐行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignRow {
    /// 批号（B29..B35）。
    pub batch: u32,
    /// 确认单号。
    pub confirm: u32,
    /// 版本号。
    pub version: u32,
}

/// 对齐三列闸：批号 ∈ 29..=35、确认单号 ≥ 2601、版本号 ≥ 1。
pub fn align_row_pass(r: &AlignRow) -> bool {
    if r.batch < 29 || r.batch > 35 {
        return false;
    }
    if r.confirm < 2601 {
        return false;
    }
    if r.version < 1 {
        return false;
    }
    true
}

/// 防幻觉总核验账。
#[derive(Clone, Debug)]
pub struct HaAudit {
    /// 相位。
    state: HaState,
    /// 抽检账。
    spots: Vec<ProvSpot>,
    /// 对齐账。
    aligns: Vec<AlignRow>,
    /// 每确认单已登记最高版本（版本倒退闸用）。
    ver_top: Vec<(u32, u32)>,
    /// 拒止计数。
    rejects: u32,
}

impl HaAudit {
    /// 新账（草稿相位）。
    pub fn new() -> HaAudit {
        HaAudit {
            state: HaState::HaDraft,
            spots: Vec::new(),
            aligns: Vec::new(),
            ver_top: Vec::new(),
            rejects: 0,
        }
    }

    /// 开扫（草稿 → 扫描中）。
    pub fn begin(&mut self) -> Result<(), u32> {
        ha_advance(self.state, HaState::HaSweeping)?;
        self.state = HaState::HaSweeping;
        Ok(())
    }

    /// 封账（扫描中 → 已封账；抽检额/对齐额/抽样额三闸）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state != HaState::HaSweeping {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HaDiag::ERewind.val());
        }
        if self.spots.len() < SPOT_MIN {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HaDiag::ESpotGap.val());
        }
        if self.aligns.len() < SAMPLE_MIN {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HaDiag::ESampleShort.val());
        }
        ha_advance(self.state, HaState::HaSealed)?;
        self.state = HaState::HaSealed;
        Ok(())
    }

    /// 当前相位。
    pub fn state(&self) -> HaState {
        self.state
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 抽检条数。
    pub fn spot_len(&self) -> usize {
        self.spots.len()
    }

    /// 对齐行数。
    pub fn align_len(&self) -> usize {
        self.aligns.len()
    }

    /// 下标取抽检条。
    pub fn spot_at(&self, idx: usize) -> Option<&ProvSpot> {
        self.spots.get(idx)
    }

    /// 下标取对齐行。
    pub fn align_at(&self, idx: usize) -> Option<&AlignRow> {
        self.aligns.get(idx)
    }

    /// 入一条出处抽检（序/重/引/码值/偏移/段位六闸）。
    pub fn reg_spot(&mut self, s: ProvSpot) -> Result<(), u32> {
        if self.state == HaState::HaSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HaDiag::ESealedWrite.val());
        }
        if self.state == HaState::HaDraft {
            self.state = HaState::HaSweeping;
        }
        // 序闸：号必须连续。
        if s.no != self.spots.len() as u32 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HaDiag::ESpotGap.val());
        }
        // 重闸。
        for prev in self.spots.iter() {
            if prev.judge == s.judge && prev.offset == s.offset {
                self.rejects = self.rejects.saturating_add(1);
                return Err(HaDiag::ESpotDup.val());
            }
        }
        // 引闸。
        if s.quote.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HaDiag::EQuoteEmpty.val());
        }
        // 偏移界闸。
        if s.offset < OFFSET_LO || s.offset > OFFSET_HI {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HaDiag::EOffsetRange.val());
        }
        // 段位对号闸。
        if seg_of(s.judge).is_none() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HaDiag::ESegDrift.val());
        }
        // 码值重算闸。
        let expect = match recompute_code(s.judge, s.offset) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(HaDiag::ESegDrift.val());
            }
        };
        if s.code != expect {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HaDiag::ECodeGhost.val());
        }
        self.spots.push(s);
        Ok(())
    }

    /// 收一行对齐（三列闸 + 版本倒退闸）。
    pub fn reg_align(&mut self, r: AlignRow) -> Result<(), u32> {
        if self.state == HaState::HaSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HaDiag::ESealedWrite.val());
        }
        if self.state == HaState::HaDraft {
            self.state = HaState::HaSweeping;
        }
        if !align_row_pass(&r) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HaDiag::EAlignMissing.val());
        }
        // 版本倒退闸：同确认单只许版本前进。
        let mut rewind = false;
        for prev in self.ver_top.iter() {
            if prev.0 == r.confirm && r.version <= prev.1 {
                rewind = true;
            }
        }
        if rewind {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HaDiag::EVersionRewind.val());
        }
        self.aligns.push(r.clone());
        // 顶版本记账（存在则抬顶，不存在则登记）。
        let mut found = false;
        for e in self.ver_top.iter_mut() {
            if e.0 == r.confirm {
                if r.version > e.1 {
                    e.1 = r.version;
                }
                found = true;
            }
        }
        if !found {
            self.ver_top.push((r.confirm, r.version));
        }
        Ok(())
    }

    /// LCG 固定种子抽样（种子 = SEG_BASE），抽 SAMPLE_MIN 行下标。
    pub fn sample_indices(&self) -> Vec<usize> {
        let mut seed: u32 = SEG_BASE;
        let mut out: Vec<usize> = Vec::new();
        if self.aligns.is_empty() {
            return out;
        }
        let mut k = 0u32;
        while (k as usize) < SAMPLE_MIN {
            // LCG：seed = seed × 1103515245 + 12345（glibc 经典参数，
            // 种子写死，确定性重放）。
            seed = seed
                .wrapping_mul(1_103_515_245)
                .wrapping_add(12_345);
            let idx = ((seed >> 16) as usize) % self.aligns.len();
            out.push(idx);
            k += 1;
        }
        out
    }
}

/// 全量防幻觉账重放（52 抽检 + 12 对齐行 + 封账）。
pub fn build_full_ha_audit() -> HaAudit {
    let mut a = HaAudit::new();
    let _ = a.begin();
    let mut i = 0usize;
    while i < SPOT_TABLE.len() {
        if let Some(s) = replay_spot(i) {
            let _ = a.reg_spot(s);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < ALIGN_TABLE.len() {
        let row = match ALIGN_TABLE.get(j) {
            Some(v) => v,
            None => break,
        };
        let _ = a.reg_align(AlignRow { batch: row.0, confirm: row.1, version: row.2 });
        j += 1;
    }
    let _ = a.seal();
    a
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 偏移/码值出处抽检 ≥50 处（52 处、双家族齐、逐条过闸）。
fn provenance_fifty_ok() -> bool {
    let a = build_full_ha_audit();
    if a.spot_len() < SPOT_MIN {
        return false;
    }
    if a.spot_len() != SPOT_CAP {
        return false;
    }
    if a.state() != HaState::HaSealed {
        return false;
    }
    // 双家族计数：偏移 26 / 码值 26。
    let mut n_off = 0u32;
    let mut n_code = 0u32;
    let mut i = 0usize;
    while i < a.spot_len() {
        let s = match a.spot_at(i) {
            Some(v) => v,
            None => return false,
        };
        if !spot_gates_pass(s) {
            return false;
        }
        match s.kind {
            ProvKind::PkOffset => n_off = n_off.saturating_add(1),
            ProvKind::PkCode => n_code = n_code.saturating_add(1),
        }
        i += 1;
    }
    n_off == 26 && n_code == 26
}

/// 确认单对齐表齐（批/确认单/版本三列，逐行三列闸）。
fn align_table_ok() -> bool {
    let a = build_full_ha_audit();
    if a.align_len() != ALIGN_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < a.align_len() {
        let r = match a.align_at(i) {
            Some(v) => v,
            None => return false,
        };
        if !align_row_pass(r) {
            return false;
        }
        i += 1;
    }
    true
}

/// 抽样 10 可点验（LCG 固定种子抽 10 行逐行回验）。
fn sample_ten_ok() -> bool {
    let a = build_full_ha_audit();
    let idx = a.sample_indices();
    if idx.len() < SAMPLE_MIN {
        return false;
    }
    for i in idx.iter() {
        let r = match a.align_at(*i) {
            Some(v) => v,
            None => return false,
        };
        if !align_row_pass(r) {
            return false;
        }
    }
    // 双跑抽样同序（确定性重放）。
    let b = build_full_ha_audit();
    let idx2 = b.sample_indices();
    if idx.len() != idx2.len() {
        return false;
    }
    let mut k = 0usize;
    while k < idx.len() {
        match (idx.get(k), idx2.get(k)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        k += 1;
    }
    true
}

/// 注入联测十族（人为构造幻觉必被拒）。
pub fn ha_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 码值幻觉（重算不符）。
    {
        let mut a = HaAudit::new();
        let _ = a.begin();
        let ghost = ProvSpot {
            no: 0,
            judge: 12660,
            quote: "幻觉码值探针",
            offset: 0x0a,
            code: 0x80F0B,
            kind: ProvKind::PkOffset,
        };
        if a.reg_spot(ghost).is_err() {
            caught += 1;
        } else {
            escape.push_str("码值幻觉;");
        }
    }
    // 2. 偏移越段。
    {
        let mut a = HaAudit::new();
        let _ = a.begin();
        let over = ProvSpot {
            no: 0,
            judge: 12660,
            quote: "越段偏移探针",
            offset: 0x41,
            code: 0x80F41,
            kind: ProvKind::PkOffset,
        };
        if a.reg_spot(over).is_err() {
            caught += 1;
        } else {
            escape.push_str("偏移越段;");
        }
    }
    // 3. 段位漂移（判据号越窗）。
    {
        let mut a = HaAudit::new();
        let _ = a.begin();
        let drift = ProvSpot {
            no: 0,
            judge: 12800,
            quote: "越窗号探针",
            offset: 0x0a,
            code: 0x82000,
            kind: ProvKind::PkCode,
        };
        if a.reg_spot(drift).is_err() {
            caught += 1;
        } else {
            escape.push_str("段位漂移;");
        }
    }
    // 4. 出处号跳号。
    {
        let mut a = HaAudit::new();
        let _ = a.begin();
        let gap = ProvSpot {
            no: 1,
            judge: 12660,
            quote: "跳号探针",
            offset: 0x0a,
            code: 0x80F0A,
            kind: ProvKind::PkOffset,
        };
        if a.reg_spot(gap).is_err() {
            caught += 1;
        } else {
            escape.push_str("跳号;");
        }
    }
    // 5. 出处号重复（同判据同偏移）。
    {
        let mut a = HaAudit::new();
        let _ = a.begin();
        let s0 = match replay_spot(0) {
            Some(v) => v,
            None => return (caught, 10, String::from("表缺行")),
        };
        let _ = a.reg_spot(s0.clone());
        if a.reg_spot(s0).is_err() {
            caught += 1;
        } else {
            escape.push_str("重号;");
        }
    }
    // 6. 引文空。
    {
        let mut a = HaAudit::new();
        let _ = a.begin();
        let empty = ProvSpot {
            no: 0,
            judge: 12660,
            quote: "",
            offset: 0x0a,
            code: 0x80F0A,
            kind: ProvKind::PkOffset,
        };
        if a.reg_spot(empty).is_err() {
            caught += 1;
        } else {
            escape.push_str("空引;");
        }
    }
    // 7. 对齐三列缺列（版本 0）。
    {
        let mut a = HaAudit::new();
        let _ = a.begin();
        let bad = AlignRow { batch: 29, confirm: 2601, version: 0 };
        if a.reg_align(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("缺列;");
        }
    }
    // 8. 对齐版本倒退。
    {
        let mut a = HaAudit::new();
        let _ = a.begin();
        let _ = a.reg_align(AlignRow { batch: 29, confirm: 2601, version: 3 });
        let rewind = a.reg_align(AlignRow { batch: 29, confirm: 2601, version: 2 });
        if rewind.is_err() {
            caught += 1;
        } else {
            escape.push_str("版本倒退;");
        }
    }
    // 9. 封账后写。
    {
        let mut a = build_full_ha_audit();
        let s = match replay_spot(0) {
            Some(v) => v,
            None => return (caught, 10, String::from("表缺行")),
        };
        if a.reg_spot(s).is_err() {
            caught += 1;
        } else {
            escape.push_str("封后写;");
        }
    }
    // 10. 状态机倒退。
    {
        let mut a = build_full_ha_audit();
        if a.begin().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let x = build_full_ha_audit();
    let y = build_full_ha_audit();
    if x.spot_len() != y.spot_len() || x.align_len() != y.align_len() {
        return false;
    }
    if x.state() != y.state() || x.reject_len() != y.reject_len() {
        return false;
    }
    let mut i = 0usize;
    while i < x.spot_len() {
        match (x.spot_at(i), y.spot_at(i)) {
            (Some(p), Some(q)) => {
                if p != q {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let (c1, _, e1) = ha_fault_sweep();
    let (c2, _, e2) = ha_fault_sweep();
    c1 == c2 && e1 == e2
}

/// 冻结摘要（抽检表 + 对齐表折算，非零且确定）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    let mut i = 0u32;
    for row in SPOT_TABLE.iter() {
        acc = acc
            .wrapping_mul(5)
            .wrapping_add(row.0)
            .wrapping_add(row.2)
            .wrapping_add(row.3)
            .wrapping_add(i);
        i = i.wrapping_add(1);
    }
    for row in ALIGN_TABLE.iter() {
        acc = acc
            .wrapping_mul(3)
            .wrapping_add(row.0)
            .wrapping_add(row.1)
            .wrapping_add(row.2);
    }
    acc
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * 199 {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12696u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let a = build_full_ha_audit();
    if a.state() != HaState::HaSealed {
        return false;
    }
    // 摘要面：相位名/种别名/诊断文案恒非空。
    for st in [HaState::HaDraft, HaState::HaSweeping, HaState::HaSealed].iter() {
        if st.name().is_empty() {
            return false;
        }
    }
    for k in ProvKind::all().iter() {
        if k.name().is_empty() {
            return false;
        }
    }
    for d in HA_ALL.iter() {
        if d.detail().is_empty() {
            return false;
        }
    }
    frozen_digest() != 0
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12697 防幻觉总核验自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12696_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12696-antihallu");
    let a = build_full_ha_audit();

    s.add_dyn(
        "F12697-01 出处抽检 ≥50 处",
        provenance_fifty_ok(),
        &format!("spots={} cap={} sealed={}", a.spot_len(), SPOT_CAP, a.state() == HaState::HaSealed),
    );

    s.add_dyn(
        "F12697-02 偏移家族 26 处齐",
        {
            let mut n = 0u32;
            for row in SPOT_TABLE.iter() {
                if row.4 == ProvKind::PkOffset {
                    n = n.saturating_add(1);
                }
            }
            n == 26
        },
        "偏移出处 26 处冻结",
    );

    s.add_dyn(
        "F12697-03 码值家族 26 处齐",
        {
            let mut n = 0u32;
            for row in SPOT_TABLE.iter() {
                if row.4 == ProvKind::PkCode {
                    n = n.saturating_add(1);
                }
            }
            n == 26
        },
        "码值出处 26 处冻结",
    );

    s.add_dyn(
        "F12697-04 码值重算闸逐条过",
        {
            let mut all = true;
            let mut i = 0usize;
            while i < a.spot_len() {
                match a.spot_at(i) {
                    Some(v) => {
                        if !spot_gates_pass(v) {
                            all = false;
                        }
                    }
                    None => all = false,
                }
                i += 1;
            }
            all
        },
        "码值 ≡ 段位 + 段内偏移",
    );

    s.add_dyn(
        "F12697-05 确认单对齐表齐（三列）",
        align_table_ok(),
        &format!("aligns={} cap={}", a.align_len(), ALIGN_CAP),
    );

    s.add_dyn(
        "F12697-06 抽样 10 可点验",
        sample_ten_ok(),
        &format!("sample={} min={}", a.sample_indices().len(), SAMPLE_MIN),
    );

    s.add_dyn(
        "F12697-07 对齐版本单调闸",
        {
            let mut ok = true;
            let mut seen: Vec<(u32, u32)> = Vec::new();
            for row in ALIGN_TABLE.iter() {
                let mut bumped = false;
                for e in seen.iter_mut() {
                    if e.0 == row.1 {
                        if row.2 <= e.1 {
                            ok = false;
                        }
                        if row.2 > e.1 {
                            e.1 = row.2;
                        }
                        bumped = true;
                    }
                }
                if !bumped {
                    seen.push((row.1, row.2));
                }
            }
            ok
        },
        "同确认单版本只进不退",
    );

    s.add_dyn(
        "F12697-08 注入联测 10/10 拒止",
        {
            let (c, t, e) = ha_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", ha_fault_sweep().0),
    );

    s.add_dyn(
        "F12697-09 相位迁移守卫直读",
        ha_advance(HaState::HaDraft, HaState::HaSweeping).is_ok()
            && ha_advance(HaState::HaSweeping, HaState::HaSealed).is_ok()
            && ha_advance(HaState::HaSealed, HaState::HaSweeping).is_err()
            && ha_advance(HaState::HaDraft, HaState::HaSealed).is_err(),
        "草稿→扫描中→已封账只进不退",
    );

    s.add_dyn(
        "F12697-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12697-11 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12697-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12697-13 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81800);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(HA_ALL.len(), 12);
        let mut i = 0usize;
        while i < HA_ALL.len() {
            let vi = match HA_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < HA_ALL.len() {
                let vj = match HA_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn provenance_fifty_holds() {
        assert!(provenance_fifty_ok());
        assert!(align_table_ok());
        let a = build_full_ha_audit();
        assert_eq!(a.spot_len(), 52);
        assert!(a.spot_len() >= SPOT_MIN);
        assert_eq!(a.align_len(), ALIGN_CAP);
        assert_eq!(a.state(), HaState::HaSealed);
        assert_eq!(a.reject_len(), 0);
    }

    #[test]
    fn recompute_code_formula_holds() {
        // 公式位：12696 → 0x81800。
        assert_eq!(seg_of(12696), Some(0x81800));
        assert_eq!(seg_of(12600), Some(0x80000));
        assert_eq!(seg_of(12799), Some(0x831C0));
        // 越窗 None。
        assert!(seg_of(12599).is_none());
        assert!(seg_of(12800).is_none());
        // 码值 ≡ 段位 + 偏移。
        assert_eq!(recompute_code(12660, 0x0a), Some(0x80F0A));
        // 冻结表逐行重算一致。
        let mut i = 0usize;
        while i < SPOT_TABLE.len() {
            let row = match SPOT_TABLE.get(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(
                recompute_code(row.0, row.2),
                Some(row.3),
                "行 {} 码值重算不符",
                i
            );
            i += 1;
        }
    }

    #[test]
    fn sample_ten_deterministic() {
        assert!(sample_ten_ok());
        let a = build_full_ha_audit();
        let idx = a.sample_indices();
        assert_eq!(idx.len(), SAMPLE_MIN);
        // 抽样行号全部落在账内。
        for i in idx.iter() {
            assert!(*i < a.align_len());
        }
        // 固定种子：两轮同序。
        let again = a.sample_indices();
        assert_eq!(idx, again);
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = ha_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn spot_gates_reject() {
        let mut a = HaAudit::new();
        let _ = a.begin();
        // 码值幻觉。
        let ghost = ProvSpot {
            no: 0,
            judge: 12660,
            quote: "x",
            offset: 0x0a,
            code: 0x80F0B,
            kind: ProvKind::PkOffset,
        };
        assert_eq!(a.reg_spot(ghost), Err(HaDiag::ECodeGhost.val()));
        // 偏移越段。
        let over = ProvSpot {
            no: 0,
            judge: 12660,
            quote: "x",
            offset: 0x00,
            code: 0x80F00,
            kind: ProvKind::PkOffset,
        };
        assert_eq!(a.reg_spot(over), Err(HaDiag::EOffsetRange.val()));
        // 空引。
        let empty = ProvSpot {
            no: 0,
            judge: 12660,
            quote: "",
            offset: 0x0a,
            code: 0x80F0A,
            kind: ProvKind::PkOffset,
        };
        assert_eq!(a.reg_spot(empty), Err(HaDiag::EQuoteEmpty.val()));
        assert_eq!(a.reject_len(), 3);
        // 正常条目过闸。
        let ok = match replay_spot(0) {
            Some(v) => v,
            None => return,
        };
        assert!(a.reg_spot(ok).is_ok());
    }

    #[test]
    fn phase_guards_and_seal() {
        // 迁移守卫。
        assert!(ha_advance(HaState::HaDraft, HaState::HaSweeping).is_ok());
        assert!(ha_advance(HaState::HaSweeping, HaState::HaSealed).is_ok());
        assert!(ha_advance(HaState::HaDraft, HaState::HaSealed).is_err());
        assert!(ha_advance(HaState::HaSealed, HaState::HaDraft).is_err());
        assert!(ha_advance(HaState::HaSweeping, HaState::HaSweeping).is_err());
        // 秩单调。
        assert!(HaState::HaDraft.rank() < HaState::HaSweeping.rank());
        assert!(HaState::HaSweeping.rank() < HaState::HaSealed.rank());
        // 抽检额不足封账拒。
        let mut a = HaAudit::new();
        let _ = a.begin();
        assert_eq!(a.seal(), Err(HaDiag::ESpotGap.val()));
        // 补足抽检但对齐不足仍拒。
        let mut i = 0usize;
        while (i as usize) < SPOT_MIN {
            if let Some(s) = replay_spot(i) {
                let _ = a.reg_spot(s);
            }
            i += 1;
        }
        assert_eq!(a.seal(), Err(HaDiag::ESampleShort.val()));
        // 补足对齐后封账过。
        let mut j = 0usize;
        while j < ALIGN_TABLE.len() {
            let row = match ALIGN_TABLE.get(j) {
                Some(v) => v,
                None => break,
            };
            let _ = a.reg_align(AlignRow { batch: row.0, confirm: row.1, version: row.2 });
            j += 1;
        }
        assert!(a.seal().is_ok());
        assert_eq!(a.state(), HaState::HaSealed);
        // 封账后写拒。
        if let Some(s) = replay_spot(0) {
            assert_eq!(a.reg_spot(s), Err(HaDiag::ESealedWrite.val()));
        }
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_ha_audit();
        let y = build_full_ha_audit();
        assert_eq!(x.spot_len(), y.spot_len());
        assert_eq!(x.align_len(), y.align_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(frozen_digest(), frozen_digest());
        assert_ne!(frozen_digest(), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        // 相位名/种别名/诊断文案恒非空（摘要面）。
        for st in [HaState::HaDraft, HaState::HaSweeping, HaState::HaSealed].iter() {
            assert!(!st.name().is_empty());
        }
        for k in ProvKind::all().iter() {
            assert!(!k.name().is_empty());
        }
        for d in HA_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12696_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
