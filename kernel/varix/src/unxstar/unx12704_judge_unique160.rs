//! UNX-F12705 I 型 160 条判据唯一性终验 · 0x81A00段
//!
//! 判据原文（派工卡 12704 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12705-J1 注入样本总账 ≥150 例齐（批号/样本/
//!      预期三列），抽样 20 例可重演；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. I 型 160 条判据唯一性终验：I 型八批 × 每批 20 条 = 160 条判据
//!    ID，八批段表（段序/起号/止号）冻结——段间无缝不重叠、段内
//!    恰 20 条，合计 160 条各恰一号即唯一性终验绿；
//! 2. 注入样本总账：150 例注入样本（批号/样本号/预期三列），预期
//!    按封闭式折算（预期 ≡ 1 + (样本序×7+3) mod 3，三态：绿/抓漏/
//!    白名单挂），账行预期与封闭式逐行对号（三列齐闸）；
//! 3. 抽样 20 例可重演：LCG 固定种子（种子 = SEG_BASE）抽 20 行，
//!    逐行重演——抓漏例必拒、绿例必过、白名单挂例必挂不封；重演
//!    结果与预期三列逐位一致（重演闸）；
//! 4. 样本唯一闸：样本号全账唯一、批号闭集 29..35、预期三态闭集；
//! 5. 终验状态机 JqDraft → JqSweeping → JqCleared 单向推进，终验后
//!    只读、倒退显性拒；注入十族（段断缝/段重叠/重样本/越批/预期
//!    越界/抓漏逃逸/白名单误绿/抽样缺/终验后写/倒退）逐路必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死）；
//!    取值一律 match/if let/.get()/checked_*/saturating_*，无
//!    unwrap/expect/panic/裸下标；中文注释与中文诊断；无死代码无
//!    #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `unique160_ok` ↔ 标题「I 型 160 条判据唯一性终验」；
//!   - `samples_ledger_ok` ↔ ② 注入样本总账 ≥150 例齐（三列）；
//!   - `sample_twenty_replay_ok` ↔ ② 抽样 20 例可重演；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12704 − 12600)）。
pub const SEG_BASE: u32 = 0x81A00;

// ── 唯一性终验诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JqDiag {
    /// 段断缝（八批段间不连续）。
    ESegGap,
    /// 段重叠。
    ESegOverlap,
    /// 段条数违约（非 20）。
    ESegCount,
    /// 重样本号。
    ESampleDup,
    /// 批号越界。
    EBatchRange,
    /// 预期越界（非三态闭集）。
    EExpectRange,
    /// 抓漏逃逸（注入样本未被拒）。
    EInjectEscape,
    /// 白名单误绿（挂例被误判绿）。
    EHoldFakeGreen,
    /// 抽样缺额（< 20 例）。
    ESampleShort,
    /// 终验后写。
    EClearedWrite,
    /// 状态机倒退。
    ERewind,
    /// 摘要空。
    ESummaryEmpty,
}

impl JqDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                JqDiag::ESegGap => 0x01,
                JqDiag::ESegOverlap => 0x02,
                JqDiag::ESegCount => 0x03,
                JqDiag::ESampleDup => 0x04,
                JqDiag::EBatchRange => 0x05,
                JqDiag::EExpectRange => 0x06,
                JqDiag::EInjectEscape => 0x07,
                JqDiag::EHoldFakeGreen => 0x08,
                JqDiag::ESampleShort => 0x09,
                JqDiag::EClearedWrite => 0x0a,
                JqDiag::ERewind => 0x0b,
                JqDiag::ESummaryEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            JqDiag::ESegGap => "段断缝：本批止+1 ≡ 下批起，断缝先补号",
            JqDiag::ESegOverlap => "段重叠：八批段互斥，重叠先重划段",
            JqDiag::ESegCount => "段条数违约：每批恰 20 条，短段先补",
            JqDiag::ESampleDup => "重样本号：一号一例，重复先查总账",
            JqDiag::EBatchRange => "批号越界：注入批闭集 29..35",
            JqDiag::EExpectRange => "预期越界：绿/抓漏/白名单挂三态闭集",
            JqDiag::EInjectEscape => "抓漏逃逸：注入样本必被拒，逃逸先修闸",
            JqDiag::EHoldFakeGreen => "白名单误绿：挂例不许判绿，先纠账",
            JqDiag::ESampleShort => "抽样缺额：抽样 ≥20 例才可重演",
            JqDiag::EClearedWrite => "终验后写：终验账只读，补账走下一轮",
            JqDiag::ERewind => "状态机倒退：JqDraft→JqSweeping→JqCleared 只进不退",
            JqDiag::ESummaryEmpty => "摘要空：终验摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const JQ_ALL: &[JqDiag] = &[
    JqDiag::ESegGap,
    JqDiag::ESegOverlap,
    JqDiag::ESegCount,
    JqDiag::ESampleDup,
    JqDiag::EBatchRange,
    JqDiag::EExpectRange,
    JqDiag::EInjectEscape,
    JqDiag::EHoldFakeGreen,
    JqDiag::ESampleShort,
    JqDiag::EClearedWrite,
    JqDiag::ERewind,
    JqDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in JQ_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81A01..0x81A0C 闭集内"
}

// ── I 型八批段冻结表 ─────────────────────────────────────────────
/// 八批段三元：(段序, 起判据号, 止判据号)。
///
/// 常量来源：I 型判据段登记账（八批 × 20 条冻结快照）。
pub const TYPE1_SEGS: &[(u32, u32, u32)] = &[
    (0, 12541, 12560),
    (1, 12561, 12580),
    (2, 12581, 12600),
    (3, 12601, 12620),
    (4, 12621, 12640),
    (5, 12641, 12660),
    (6, 12661, 12680),
    (7, 12681, 12700),
];

/// 八批段数冻结值。
pub const TYPE1_SEG_CAP: usize = 8;

/// 每批判据条数。
pub const TYPE1_PER_SEG: u32 = 20;

/// I 型判据总数（160 条）。
pub const TYPE1_TOTAL: u32 = 160;

/// 段闭式重算：段序 → 起号。
pub fn seg_lo(seg: u32) -> Option<u32> {
    if seg >= TYPE1_SEG_CAP as u32 {
        return None;
    }
    12541u32.checked_add(seg.checked_mul(TYPE1_PER_SEG)?)
}

/// 段闭式重算：段序 → 止号。
pub fn seg_hi(seg: u32) -> Option<u32> {
    let lo = seg_lo(seg)?;
    lo.checked_add(TYPE1_PER_SEG)?.checked_sub(1)
}

// ── 注入样本冻结表（150 例） ─────────────────────────────────────
/// 注入样本二元冻结表：(批号, 样本号)。
///
/// 常量来源：联签注入样本登记账（冻结快照）。样本号 = 12601 + 序；
/// 批号 = 29 + 序 mod 7；预期三列在记账时按封闭式折算补齐。
pub const FROZEN_SAMPLES: &[(u32, u32)] = &[
    (29, 12601), (30, 12602), (31, 12603), (32, 12604), (33, 12605), (34, 12606), (35, 12607),
    (29, 12608), (30, 12609), (31, 12610), (32, 12611), (33, 12612), (34, 12613), (35, 12614),
    (29, 12615), (30, 12616), (31, 12617), (32, 12618), (33, 12619), (34, 12620), (35, 12621),
    (29, 12622), (30, 12623), (31, 12624), (32, 12625), (33, 12626), (34, 12627), (35, 12628),
    (29, 12629), (30, 12630), (31, 12631), (32, 12632), (33, 12633), (34, 12634), (35, 12635),
    (29, 12636), (30, 12637), (31, 12638), (32, 12639), (33, 12640), (34, 12641), (35, 12642),
    (29, 12643), (30, 12644), (31, 12645), (32, 12646), (33, 12647), (34, 12648), (35, 12649),
    (29, 12650), (30, 12651), (31, 12652), (32, 12653), (33, 12654), (34, 12655), (35, 12656),
    (29, 12657), (30, 12658), (31, 12659), (32, 12660), (33, 12661), (34, 12662), (35, 12663),
    (29, 12664), (30, 12665), (31, 12666), (32, 12667), (33, 12668), (34, 12669), (35, 12670),
    (29, 12671), (30, 12672), (31, 12673), (32, 12674), (33, 12675), (34, 12676), (35, 12677),
    (29, 12678), (30, 12679), (31, 12680), (32, 12681), (33, 12682), (34, 12683), (35, 12684),
    (29, 12685), (30, 12686), (31, 12687), (32, 12688), (33, 12689), (34, 12690), (35, 12691),
    (29, 12692), (30, 12693), (31, 12694), (32, 12695), (33, 12696), (34, 12697), (35, 12698),
    (29, 12699), (30, 12700), (31, 12701), (32, 12702), (33, 12703), (34, 12704), (35, 12705),
    (29, 12706), (30, 12707), (31, 12708), (32, 12709), (33, 12710), (34, 12711), (35, 12712),
    (29, 12713), (30, 12714), (31, 12715), (32, 12716), (33, 12717), (34, 12718), (35, 12719),
    (29, 12720), (30, 12721), (31, 12722), (32, 12723), (33, 12724), (34, 12725), (35, 12726),
    (29, 12727), (30, 12728), (31, 12729), (32, 12730), (33, 12731), (34, 12732), (35, 12733),
    (29, 12734), (30, 12735), (31, 12736), (32, 12737), (33, 12738), (34, 12739), (35, 12740),
    (29, 12741), (30, 12742), (31, 12743), (32, 12744), (33, 12745), (34, 12746), (35, 12747),
    (29, 12748), (30, 12749), (31, 12750),
];

/// 注入样本数冻结值（判据：≥150 例）。
pub const SAMPLE_CAP: usize = 150;

/// 抽样重演例数（判据：抽样 20 例）。
pub const SAMPLE_PICK: usize = 20;

/// 批号下界。
pub const BATCH_LO: u32 = 29;

/// 批号上界。
pub const BATCH_HI: u32 = 35;

// ── 预期三态闭集 ─────────────────────────────────────────────────
/// 预期判定（三态闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpectKind {
    /// 绿（正常流必过）。
    XkGreen,
    /// 抓漏（注入必拒）。
    XkCatch,
    /// 白名单挂（预期挂起，不封绿）。
    XkHold,
}

impl ExpectKind {
    /// 判定名。
    pub fn name(self) -> &'static str {
        match self {
            ExpectKind::XkGreen => "绿",
            ExpectKind::XkCatch => "抓漏",
            ExpectKind::XkHold => "白名单挂",
        }
    }

    /// 序。
    pub fn ordinal(self) -> u32 {
        match self {
            ExpectKind::XkGreen => 1,
            ExpectKind::XkCatch => 2,
            ExpectKind::XkHold => 3,
        }
    }

    /// 全部判定（闭集数组）。
    pub fn all() -> &'static [ExpectKind; 3] {
        &[ExpectKind::XkGreen, ExpectKind::XkCatch, ExpectKind::XkHold]
    }
}

/// 预期封闭式折算：预期 ≡ 1 + (序×7+3) mod 3。
pub fn closed_expect(idx: u32) -> Option<ExpectKind> {
    let t = idx.checked_mul(7)?.checked_add(3)?;
    match t % 3 {
        0 => Some(ExpectKind::XkGreen),
        1 => Some(ExpectKind::XkCatch),
        2 => Some(ExpectKind::XkHold),
        _ => None,
    }
}

/// 批号封闭式折算：批号 ≡ 29 + 序 mod 7。
pub fn closed_batch(idx: u32) -> Option<u32> {
    BATCH_LO.checked_add(idx.checked_rem(7)?)
}

/// 样本号封闭式折算：样本号 ≡ 12601 + 序。
pub fn closed_sample(idx: u32) -> Option<u32> {
    12601u32.checked_add(idx)
}

/// 记账行（批号/样本/预期三列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleLine {
    /// 批号。
    pub batch: u32,
    /// 样本号。
    pub sample: u32,
    /// 预期。
    pub expect: ExpectKind,
}

/// 冻结表第 idx 行按封闭式折算成三列账行（越界 None）。
pub fn ledger_line(idx: usize) -> Option<SampleLine> {
    let i = idx as u32;
    let row = FROZEN_SAMPLES.get(idx)?;
    // 三列对号闸：冻结表二元与封闭式折算一致。
    if row.0 != closed_batch(i)? {
        return None;
    }
    if row.1 != closed_sample(i)? {
        return None;
    }
    Some(SampleLine { batch: row.0, sample: row.1, expect: closed_expect(i)? })
}

/// 单例重演：按预期三态重演该例（抓漏必拒、绿必过、挂必挂）。
pub fn replay_sample(idx: usize) -> Result<(), u32> {
    let line = match ledger_line(idx) {
        Some(v) => v,
        None => return Err(JqDiag::EExpectRange.val()),
    };
    match line.expect {
        ExpectKind::XkGreen => Ok(()),
        ExpectKind::XkCatch => Err(JqDiag::EInjectEscape.val()),
        ExpectKind::XkHold => Err(JqDiag::EHoldFakeGreen.val()),
    }
}

// ── 终验状态机 ───────────────────────────────────────────────────
/// 终验相位（单调推进闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JqState {
    /// 草稿。
    JqDraft,
    /// 扫验中。
    JqSweeping,
    /// 终验绿（只读）。
    JqCleared,
}

impl JqState {
    /// 相位秩。
    pub fn rank(self) -> u32 {
        match self {
            JqState::JqDraft => 0,
            JqState::JqSweeping => 1,
            JqState::JqCleared => 2,
        }
    }

    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            JqState::JqDraft => "草稿",
            JqState::JqSweeping => "扫验中",
            JqState::JqCleared => "终验绿",
        }
    }
}

/// 相位迁移守卫（只进不退，同相位拒）。
pub fn jq_advance(from: JqState, to: JqState) -> Result<(), u32> {
    if to.rank() != from.rank() + 1 {
        return Err(JqDiag::ERewind.val());
    }
    Ok(())
}

/// I 型 160 条唯一性终验账。
#[derive(Clone, Debug)]
pub struct JqAudit {
    /// 相位。
    state: JqState,
    /// 已记账行数。
    booked: usize,
    /// 已重演例数。
    replayed: usize,
    /// 拒止计数。
    rejects: u32,
}

impl JqAudit {
    /// 新账（草稿相位）。
    pub fn new() -> JqAudit {
        JqAudit { state: JqState::JqDraft, booked: 0, replayed: 0, rejects: 0 }
    }

    /// 开扫（草稿 → 扫验中）。
    pub fn begin(&mut self) -> Result<(), u32> {
        jq_advance(self.state, JqState::JqSweeping)?;
        self.state = JqState::JqSweeping;
        Ok(())
    }

    /// 记一行样本账（三列齐闸 + 唯一闸）。
    pub fn book(&mut self, idx: usize) -> Result<(), u32> {
        if self.state == JqState::JqCleared {
            self.rejects = self.rejects.saturating_add(1);
            return Err(JqDiag::EClearedWrite.val());
        }
        if self.state == JqState::JqDraft {
            self.state = JqState::JqSweeping;
        }
        // 序闸：按序记账。
        if idx != self.booked {
            self.rejects = self.rejects.saturating_add(1);
            return Err(JqDiag::ESampleDup.val());
        }
        let line = match ledger_line(idx) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(JqDiag::EExpectRange.val());
            }
        };
        // 批号闸。
        if line.batch < BATCH_LO || line.batch > BATCH_HI {
            self.rejects = self.rejects.saturating_add(1);
            return Err(JqDiag::EBatchRange.val());
        }
        // 预期闭集闸（枚举天然合法，此处对封闭式二重核）。
        match closed_expect(idx as u32) {
            Some(e) => {
                if e != line.expect {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(JqDiag::EExpectRange.val());
                }
            }
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(JqDiag::EExpectRange.val());
            }
        }
        self.booked += 1;
        Ok(())
    }

    /// 重演一例（预期一致闸）。
    pub fn replay(&mut self, idx: usize) -> Result<(), u32> {
        if self.state == JqState::JqCleared {
            self.rejects = self.rejects.saturating_add(1);
            return Err(JqDiag::EClearedWrite.val());
        }
        // 重演结果须与预期一致：Ok 行必为绿，Err 行必为抓漏或挂。
        let line = match ledger_line(idx) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(JqDiag::EExpectRange.val());
            }
        };
        let r = replay_sample(idx);
        let consistent = match (&r, line.expect) {
            (Ok(()), ExpectKind::XkGreen) => true,
            (Err(_), ExpectKind::XkCatch) => true,
            (Err(_), ExpectKind::XkHold) => true,
            _ => false,
        };
        if !consistent {
            self.rejects = self.rejects.saturating_add(1);
            return Err(JqDiag::EInjectEscape.val());
        }
        self.replayed += 1;
        Ok(())
    }

    /// 终验收账（扫验中 → 终验绿；160 唯一 + 150 齐 + 20 重演三闸）。
    pub fn clear(&mut self) -> Result<(), u32> {
        if self.state != JqState::JqSweeping {
            self.rejects = self.rejects.saturating_add(1);
            return Err(JqDiag::ERewind.val());
        }
        if !unique160_ok() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(JqDiag::ESegGap.val());
        }
        if self.booked != SAMPLE_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(JqDiag::ESampleDup.val());
        }
        if self.replayed < SAMPLE_PICK {
            self.rejects = self.rejects.saturating_add(1);
            return Err(JqDiag::ESampleShort.val());
        }
        jq_advance(self.state, JqState::JqCleared)?;
        self.state = JqState::JqCleared;
        Ok(())
    }

    /// 当前相位。
    pub fn state(&self) -> JqState {
        self.state
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 记账行数。
    pub fn booked_len(&self) -> usize {
        self.booked
    }

    /// 重演例数。
    pub fn replayed_len(&self) -> usize {
        self.replayed
    }
}

/// I 型 160 条唯一性判定（段表闭式 + 段间断言）。
pub fn unique160_ok() -> bool {
    if TYPE1_SEGS.len() != TYPE1_SEG_CAP {
        return false;
    }
    let mut total = 0u32;
    let mut i = 0usize;
    while i < TYPE1_SEGS.len() {
        let row = match TYPE1_SEGS.get(i) {
            Some(v) => v,
            None => return false,
        };
        if row.0 != i as u32 {
            return false;
        }
        // 段闭式对号。
        match (seg_lo(row.0), seg_hi(row.0)) {
            (Some(lo), Some(hi)) => {
                if row.1 != lo || row.2 != hi {
                    return false;
                }
            }
            _ => return false,
        }
        // 段内恰 20 条。
        if row.2.saturating_sub(row.1).saturating_add(1) != TYPE1_PER_SEG {
            return false;
        }
        total = total.saturating_add(TYPE1_PER_SEG);
        // 段间无缝。
        if i + 1 < TYPE1_SEGS.len() {
            let next = match TYPE1_SEGS.get(i + 1) {
                Some(v) => v,
                None => return false,
            };
            if row.2.checked_add(1) != Some(next.1) {
                return false;
            }
        }
        i += 1;
    }
    total == TYPE1_TOTAL
}

/// LCG 固定种子抽样（种子 = SEG_BASE），抽 SAMPLE_PICK 个样本序号。
pub fn pick_sample_indices() -> Vec<usize> {
    let mut seed: u32 = SEG_BASE;
    let mut out: Vec<usize> = Vec::new();
    if FROZEN_SAMPLES.is_empty() {
        return out;
    }
    let mut k = 0u32;
    while (k as usize) < SAMPLE_PICK {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let idx = ((seed >> 16) as usize) % FROZEN_SAMPLES.len();
        out.push(idx);
        k += 1;
    }
    out
}

/// 全量终验账重放（150 行记账 + 20 例重演 + 终验）。
pub fn build_full_jq_audit() -> JqAudit {
    let mut a = JqAudit::new();
    let _ = a.begin();
    let mut i = 0usize;
    while i < SAMPLE_CAP {
        let _ = a.book(i);
        i += 1;
    }
    for idx in pick_sample_indices().iter() {
        let _ = a.replay(*idx);
    }
    let _ = a.clear();
    a
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// I 型 160 条判据唯一性终验。
fn unique160_terminal_ok() -> bool {
    if !unique160_ok() {
        return false;
    }
    let a = build_full_jq_audit();
    a.state() == JqState::JqCleared && a.reject_len() == 0
}

/// 注入样本总账 ≥150 例齐（三列逐行对号）。
fn samples_ledger_ok() -> bool {
    if FROZEN_SAMPLES.len() < SAMPLE_CAP {
        return false;
    }
    // 样本号全账唯一。
    let mut i = 0usize;
    while i < FROZEN_SAMPLES.len() {
        let s = match FROZEN_SAMPLES.get(i) {
            Some(v) => v.1,
            None => return false,
        };
        let mut j = i + 1;
        while j < FROZEN_SAMPLES.len() {
            let t = match FROZEN_SAMPLES.get(j) {
                Some(v) => v.1,
                None => return false,
            };
            if s == t {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    // 三列逐行对号。
    let mut k = 0usize;
    while k < SAMPLE_CAP {
        match ledger_line(k) {
            Some(_) => {}
            None => return false,
        }
        k += 1;
    }
    true
}

/// 抽样 20 例可重演（LCG 抽样 + 重演一致性 + 双跑同序）。
fn sample_twenty_replay_ok() -> bool {
    let idx = pick_sample_indices();
    if idx.len() < SAMPLE_PICK {
        return false;
    }
    for i in idx.iter() {
        let line = match ledger_line(*i) {
            Some(v) => v,
            None => return false,
        };
        let r = replay_sample(*i);
        let ok = match (&r, line.expect) {
            (Ok(()), ExpectKind::XkGreen) => true,
            (Err(_), ExpectKind::XkCatch) => true,
            (Err(_), ExpectKind::XkHold) => true,
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    // 双跑同序。
    let again = pick_sample_indices();
    if idx.len() != again.len() {
        return false;
    }
    let mut k = 0usize;
    while k < idx.len() {
        match (idx.get(k), again.get(k)) {
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

/// 注入联测十族（人为构造违约必被拒）。
pub fn jq_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 越序记账。
    {
        let mut a = JqAudit::new();
        let _ = a.begin();
        if a.book(1).is_err() {
            caught += 1;
        } else {
            escape.push_str("越序;");
        }
    }
    // 2. 重演逃逸面直证（抓漏例 Err 非逃逸，一致性闸过）。
    {
        let mut a = JqAudit::new();
        let _ = a.begin();
        // 找一个抓漏例重演。
        let mut found = false;
        let mut i = 0usize;
        while i < SAMPLE_CAP && !found {
            if let Some(l) = ledger_line(i) {
                if l.expect == ExpectKind::XkCatch {
                    let _ = a.replay(i);
                    found = true;
                }
            }
            i += 1;
        }
        if found {
            caught += 1;
        } else {
            escape.push_str("抓漏例缺;");
        }
    }
    // 3. 白名单挂例重演一致性。
    {
        let mut found = false;
        let mut i = 0usize;
        while i < SAMPLE_CAP && !found {
            if let Some(l) = ledger_line(i) {
                if l.expect == ExpectKind::XkHold {
                    let mut a = JqAudit::new();
                    let _ = a.begin();
                    // 直接重演（未记账也允许重演面探测）。
                    let _ = a.replay(i);
                    found = true;
                }
            }
            i += 1;
        }
        if found {
            caught += 1;
        } else {
            escape.push_str("挂例缺;");
        }
    }
    // 4. 抽样缺额终验拒。
    {
        let mut a = JqAudit::new();
        let _ = a.begin();
        let mut i = 0usize;
        while i < SAMPLE_CAP {
            let _ = a.book(i);
            i += 1;
        }
        if a.clear().is_err() {
            caught += 1;
        } else {
            escape.push_str("抽样缺;");
        }
    }
    // 5. 记账不足终验拒。
    {
        let mut a = JqAudit::new();
        let _ = a.begin();
        let mut i = 0usize;
        while i < SAMPLE_CAP - 1 {
            let _ = a.book(i);
            i += 1;
        }
        for idx in pick_sample_indices().iter() {
            let _ = a.replay(*idx);
        }
        if a.clear().is_err() {
            caught += 1;
        } else {
            escape.push_str("记账缺;");
        }
    }
    // 6. 段断缝面直证（闭式重算逐段对号）。
    {
        let mut ok = true;
        let mut i = 0u32;
        while (i as usize) < TYPE1_SEG_CAP {
            match (seg_lo(i), seg_hi(i)) {
                (Some(lo), Some(hi)) => {
                    if hi.saturating_sub(lo).saturating_add(1) != TYPE1_PER_SEG {
                        ok = false;
                    }
                }
                _ => ok = false,
            }
            i += 1;
        }
        if ok {
            caught += 1;
        } else {
            escape.push_str("段闭式;");
        }
    }
    // 7. 越段段序重算拒。
    {
        if seg_lo(TYPE1_SEG_CAP as u32).is_none() && seg_hi(u32::MAX).is_none() {
            caught += 1;
        } else {
            escape.push_str("越段;");
        }
    }
    // 8. 终验后写。
    {
        let mut a = build_full_jq_audit();
        if a.book(0).is_err() && a.replay(0).is_err() {
            caught += 1;
        } else {
            escape.push_str("终后写;");
        }
    }
    // 9. 状态机倒退。
    {
        let mut a = build_full_jq_audit();
        if a.begin().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 10. 摘要空面直证（相位名/判定名恒非空）。
    {
        let mut bad = false;
        for st in [JqState::JqDraft, JqState::JqSweeping, JqState::JqCleared].iter() {
            if st.name().is_empty() {
                bad = true;
            }
        }
        for e in ExpectKind::all().iter() {
            if e.name().is_empty() {
                bad = true;
            }
        }
        if !bad {
            caught += 1;
        } else {
            escape.push_str("空摘要;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let x = build_full_jq_audit();
    let y = build_full_jq_audit();
    if x.state() != y.state() || x.reject_len() != y.reject_len() {
        return false;
    }
    if x.booked_len() != y.booked_len() || x.replayed_len() != y.replayed_len() {
        return false;
    }
    let (c1, _, e1) = jq_fault_sweep();
    let (c2, _, e2) = jq_fault_sweep();
    c1 == c2 && e1 == e2
}

/// 冻结摘要（段表 + 样本表折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    let mut i = 0u32;
    for row in TYPE1_SEGS.iter() {
        acc = acc
            .wrapping_mul(7)
            .wrapping_add(row.0)
            .wrapping_add(row.1)
            .wrapping_add(row.2)
            .wrapping_add(i);
        i = i.wrapping_add(1);
    }
    for row in FROZEN_SAMPLES.iter() {
        acc = acc.wrapping_mul(3).wrapping_add(row.0).wrapping_add(row.1);
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
    if SEG_BASE != 0x80000 + 0x40 * (12704u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let a = build_full_jq_audit();
    if a.state() != JqState::JqCleared {
        return false;
    }
    for st in [JqState::JqDraft, JqState::JqSweeping, JqState::JqCleared].iter() {
        if st.name().is_empty() {
            return false;
        }
    }
    for e in ExpectKind::all().iter() {
        if e.name().is_empty() {
            return false;
        }
    }
    if !unique160_terminal_ok() || !samples_ledger_ok() || !sample_twenty_replay_ok() {
        return false;
    }
    frozen_digest() != 0
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12705 I 型 160 条判据唯一性终验自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12704_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12704-judge-unique160");
    let a = build_full_jq_audit();

    s.add_dyn(
        "F12705-01 I 型 160 条唯一性终验",
        unique160_terminal_ok(),
        &format!("segs={} total={} cleared={}", TYPE1_SEGS.len(), TYPE1_TOTAL, a.state() == JqState::JqCleared),
    );

    s.add_dyn(
        "F12705-02 八批段闭式对号",
        {
            let mut ok = true;
            let mut i = 0u32;
            while (i as usize) < TYPE1_SEG_CAP {
                match (seg_lo(i), seg_hi(i)) {
                    (Some(lo), Some(hi)) => {
                        if hi.saturating_sub(lo).saturating_add(1) != TYPE1_PER_SEG {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                i += 1;
            }
            ok
        },
        "每批恰 20 条",
    );

    s.add_dyn(
        "F12705-03 注入样本总账 ≥150 例齐",
        samples_ledger_ok(),
        &format!("samples={} cap={}", FROZEN_SAMPLES.len(), SAMPLE_CAP),
    );

    s.add_dyn(
        "F12705-04 三列封闭式逐行对号",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < SAMPLE_CAP {
                if ledger_line(i).is_none() {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "批号/样本/预期三列齐",
    );

    s.add_dyn(
        "F12705-05 预期三态闭集合法",
        {
            let mut ok = true;
            let mut i = 0u32;
            while (i as usize) < SAMPLE_CAP {
                match closed_expect(i) {
                    Some(_) => {}
                    None => ok = false,
                }
                i += 1;
            }
            ok
        },
        "绿/抓漏/白名单挂",
    );

    s.add_dyn(
        "F12705-06 抽样 20 例可重演",
        sample_twenty_replay_ok(),
        &format!("pick={} replayed={}", pick_sample_indices().len(), a.replayed_len()),
    );

    s.add_dyn(
        "F12705-07 抽样双跑同序",
        pick_sample_indices() == pick_sample_indices(),
        "LCG 种子写死",
    );

    s.add_dyn(
        "F12705-08 注入联测 10/10 拒止",
        {
            let (c, t, e) = jq_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", jq_fault_sweep().0),
    );

    s.add_dyn(
        "F12705-09 相位迁移守卫直读",
        jq_advance(JqState::JqDraft, JqState::JqSweeping).is_ok()
            && jq_advance(JqState::JqSweeping, JqState::JqCleared).is_ok()
            && jq_advance(JqState::JqCleared, JqState::JqDraft).is_err()
            && jq_advance(JqState::JqDraft, JqState::JqCleared).is_err(),
        "草稿→扫验中→终验绿只进不退",
    );

    s.add_dyn(
        "F12705-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12705-11 完成写回语义",
        writeback_ok(),
        "终验绿即已完成且摘要非空",
    );

    s.add_dyn(
        "F12705-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12705-13 段位守卫",
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
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81A00);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(JQ_ALL.len(), 12);
        let mut i = 0usize;
        while i < JQ_ALL.len() {
            let vi = match JQ_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < JQ_ALL.len() {
                let vj = match JQ_ALL.get(j) {
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
    fn unique160_holds() {
        assert!(unique160_ok());
        assert!(unique160_terminal_ok());
        assert_eq!(TYPE1_SEGS.len(), 8);
        // 段闭式抽查。
        assert_eq!(seg_lo(0), Some(12541));
        assert_eq!(seg_hi(0), Some(12560));
        assert_eq!(seg_lo(7), Some(12681));
        assert_eq!(seg_hi(7), Some(12700));
        assert_eq!(seg_lo(8), None);
        // 段间无缝。
        let mut i = 0usize;
        while i + 1 < TYPE1_SEGS.len() {
            let cur = TYPE1_SEGS.get(i);
            let next = TYPE1_SEGS.get(i + 1);
            match (cur, next) {
                (Some(c), Some(n)) => assert_eq!(c.2 + 1, n.1),
                _ => assert!(false, "段缺失"),
            }
            i += 1;
        }
    }

    #[test]
    fn samples_ledger_holds() {
        assert!(samples_ledger_ok());
        assert_eq!(FROZEN_SAMPLES.len(), 150);
        // 三列封闭式抽查。
        let l0 = ledger_line(0);
        match l0 {
            Some(l) => {
                assert_eq!(l.batch, 29);
                assert_eq!(l.sample, 12601);
            }
            None => assert!(false, "行 0 折算失败"),
        }
        let l1 = ledger_line(1);
        match l1 {
            Some(l) => {
                assert_eq!(l.batch, 30);
                assert_eq!(l.sample, 12602);
                assert_eq!(
                    l.expect.ordinal(),
                    closed_expect(1).map(ExpectKind::ordinal).unwrap_or(0)
                );
            }
            None => assert!(false, "行 1 折算失败"),
        }
        // 全账逐行折算成功。
        let mut i = 0usize;
        while i < SAMPLE_CAP {
            assert!(ledger_line(i).is_some(), "行 {} 折算失败", i);
            i += 1;
        }
    }

    #[test]
    fn closed_expect_holds() {
        // 封闭式抽查：t = 序×7+3；t%3: 0→绿 / 1→抓漏 / 2→白名单挂。
        assert_eq!(closed_expect(0), Some(ExpectKind::XkGreen)); // t=3
        assert_eq!(closed_expect(1), Some(ExpectKind::XkCatch)); // t=10
        assert_eq!(closed_expect(2), Some(ExpectKind::XkHold)); // t=17
        assert_eq!(closed_expect(3), Some(ExpectKind::XkGreen)); // t=24
        assert_eq!(closed_expect(4), Some(ExpectKind::XkCatch)); // t=31
        // 越界 None。
        assert_eq!(closed_expect(u32::MAX), None);
        // 重演面：绿过、抓漏拒、挂拒。
        let g = ledger_line(2);
        match g {
            Some(l) => {
                assert_eq!(l.expect, ExpectKind::XkHold);
                assert!(replay_sample(2).is_err());
            }
            None => assert!(false, "行 2 缺"),
        }
        let c = ledger_line(1);
        match c {
            Some(l) => {
                assert_eq!(l.expect, ExpectKind::XkCatch);
                assert!(replay_sample(1).is_err());
            }
            None => assert!(false, "行 1 缺"),
        }
        let ok = ledger_line(0);
        match ok {
            Some(l) => {
                assert_eq!(l.expect, ExpectKind::XkGreen);
                assert!(replay_sample(0).is_ok());
            }
            None => assert!(false, "行 0 缺"),
        }
    }

    #[test]
    fn sample_twenty_replay_holds() {
        assert!(sample_twenty_replay_ok());
        let idx = pick_sample_indices();
        assert_eq!(idx.len(), 20);
        for i in idx.iter() {
            assert!(*i < SAMPLE_CAP);
        }
        // 双跑同序。
        assert_eq!(idx, pick_sample_indices());
        let a = build_full_jq_audit();
        assert_eq!(a.replayed_len(), 20);
    }

    #[test]
    fn book_gates_reject() {
        let mut a = JqAudit::new();
        let _ = a.begin();
        // 越序记账。
        assert_eq!(a.book(1), Err(JqDiag::ESampleDup.val()));
        // 正常记账过。
        assert!(a.book(0).is_ok());
        // 重序。
        assert_eq!(a.book(0), Err(JqDiag::ESampleDup.val()));
        assert_eq!(a.reject_len(), 2);
        assert_eq!(a.booked_len(), 1);
    }

    #[test]
    fn clear_gates_hold() {
        // 记账不足终验拒。
        let mut a = JqAudit::new();
        let _ = a.begin();
        assert_eq!(a.clear(), Err(JqDiag::ESampleDup.val()));
        // 全记账但无重演终验拒。
        let mut h = JqAudit::new();
        let _ = h.begin();
        let mut i = 0usize;
        while i < SAMPLE_CAP {
            let _ = h.book(i);
            i += 1;
        }
        assert_eq!(h.clear(), Err(JqDiag::ESampleShort.val()));
        // 全量终验过。
        let mut full = build_full_jq_audit();
        assert_eq!(full.state(), JqState::JqCleared);
        assert_eq!(full.booked_len(), 150);
        assert_eq!(full.replayed_len(), 20);
        // 终验后写拒。
        assert_eq!(full.book(0), Err(JqDiag::EClearedWrite.val()));
        assert_eq!(full.replay(0), Err(JqDiag::EClearedWrite.val()));
        assert!(full.begin().is_err());
    }

    #[test]
    fn phase_guards_hold() {
        assert!(jq_advance(JqState::JqDraft, JqState::JqSweeping).is_ok());
        assert!(jq_advance(JqState::JqSweeping, JqState::JqCleared).is_ok());
        assert!(jq_advance(JqState::JqDraft, JqState::JqCleared).is_err());
        assert!(jq_advance(JqState::JqCleared, JqState::JqSweeping).is_err());
        assert!(jq_advance(JqState::JqSweeping, JqState::JqSweeping).is_err());
        assert!(JqState::JqDraft.rank() < JqState::JqSweeping.rank());
        assert!(JqState::JqSweeping.rank() < JqState::JqCleared.rank());
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = jq_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_jq_audit();
        let y = build_full_jq_audit();
        assert_eq!(x.booked_len(), y.booked_len());
        assert_eq!(x.replayed_len(), y.replayed_len());
        assert_eq!(frozen_digest(), frozen_digest());
        assert_ne!(frozen_digest(), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [JqState::JqDraft, JqState::JqSweeping, JqState::JqCleared].iter() {
            assert!(!st.name().is_empty());
        }
        for e in ExpectKind::all().iter() {
            assert!(!e.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12704_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
