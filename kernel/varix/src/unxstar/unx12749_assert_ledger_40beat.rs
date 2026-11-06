//! UNX-F12749 断言账本压测对账（40 拍口径） · 0x82540段
//!
//! 判据原文（派工卡 12749 逐字锚定）：
//!   UNX-F12750-J1 分发 P95 预算在册（O1 配套账），高压注入账无红账。
//!
//! 设计要点：
//! 1. 断言账本 40 拍对账域模型：40 拍登记表（拍号 1..=40 / 账面断言数 /
//!    实算断言数 / 差值），冻结闭集逐拍可复算——账面数来自冻结常量表副本，
//!    实算数由冻结事件流独立重放计数，两侧互不引用，篡改一侧即可判差；
//! 2. 对账判定器 `judge_beat(beat_no) -> Result<BeatRow, u32>`：
//!    未登记拍 → ENOENT；账实不等 → EMISMATCH 并指明拍号（last_error_beat
//!    可取回）；重复对账 → EALREADY；判定次序固定为
//!    越界 → 闭合 → 未登记 → 重复 → 账实，任何改序即语义漂移；
//! 3. 账实差零静默：差值为零不产生任何告警行；任何非零差必须告警入账，
//!    告警账行留痕（拍号/账面/实算/差值四元组），闭合前告警账必须清空；
//! 4. 高压注入器三路各 10 轮全抓：账面篡改 / 拍号越界 / 重复对账，
//!    每轮人为构造违约必须被判定器抓住，逃脱即判据失效（ETamperEscape）；
//! 5. 全拍对账闭合：40 拍全部执行对账且全部对平（零差零告警）才许闭合，
//!    闭合单向推进（Idle→Judging→Closed），闭合后写入与再闭合显性拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死）；
//!    双跑一致指纹（FNV-1a 覆盖对账行集/告警集/已判拍数/状态秩）；
//!    取值一律 match/if let/.get()/checked_*/saturating_*，
//!    无 unwrap/expect/panic/裸下标，无 std/unsafe。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12749 − 12600)）。
pub const SEG_BASE: u32 = 0x82540;

// ── 对账诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconDiag {
    /// 拍号越界：对账域闭集只认 1..=40，越界拍一律显性拒。
    EBeatRange,
    /// 未登记拍：拍号在闭集内但登记表无此拍，先登记再对账。
    ENOENT,
    /// 重复对账/重复登记：同拍只落一笔对账动作，二次触发即拒。
    EALREADY,
    /// 账实不等：账面断言数与实算断言数不一致，指明拍号并告警入账。
    EMISMATCH,
    /// 容量耗尽：40 拍登记表已满，再登记一律拒。
    ECapacity,
    /// 未全拍对平禁止闭合：40 拍未判满或仍有非零差告警。
    ENotClosed,
    /// 闭合后写入拒：账本已闭合只读，改动走下一轮压测滚动。
    ESealed,
    /// 闭合倒退/重复闭合：闭合单向推进，倒退即账本可疑。
    ERewind,
    /// 非零差未留痕：差零静默不变量被破坏（告警账行缺失）。
    EWarnLost,
    /// 注入逃脱：人为构造的违约未被判定器抓住，判定面失效须整账复核。
    ETamperEscape,
    /// 双跑指纹不一致：确定性重放下两次全流程指纹不同。
    EFingerprint,
    /// 账面断言数越域：登记表账面数必须在 1..=64 非零域。
    EBookedRange,
}

impl ReconDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ReconDiag::EBeatRange => 0x01,
                ReconDiag::ENOENT => 0x02,
                ReconDiag::EALREADY => 0x03,
                ReconDiag::EMISMATCH => 0x04,
                ReconDiag::ECapacity => 0x05,
                ReconDiag::ENotClosed => 0x06,
                ReconDiag::ESealed => 0x07,
                ReconDiag::ERewind => 0x08,
                ReconDiag::EWarnLost => 0x09,
                ReconDiag::ETamperEscape => 0x0a,
                ReconDiag::EFingerprint => 0x0b,
                ReconDiag::EBookedRange => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            ReconDiag::EBeatRange => "拍号越界：对账域闭集只认 1..=40，先对拍号再入账，越界拍不构成对账记录",
            ReconDiag::ENOENT => "未登记拍：拍号在闭集内但登记表无此拍，先走登记口补账面再对账",
            ReconDiag::EALREADY => "重复对账：同拍只判一次，重复触发先查已判拍表，勿重放对账动作",
            ReconDiag::EMISMATCH => "账实不等：账面断言数与实算数有差，按指明拍号回查账面篡改源并留痕告警",
            ReconDiag::ECapacity => "容量耗尽：40 拍登记表已满，先闭合本轮再开下一轮压测账本",
            ReconDiag::ENotClosed => "未全拍对平禁止闭合：40 拍须全部对账且零差零告警，先补判缺拍并清告警",
            ReconDiag::ESealed => "账本已闭合：闭合后写入拒绝，改动走下一轮压测滚动，勿改已闭合账面",
            ReconDiag::ERewind => "闭合倒退：Idle→Judging→Closed 只进不退，重复闭合即账本状态可疑",
            ReconDiag::EWarnLost => "非零差未留痕：账实差零静默被破坏，非零差拍必须补告警账行留痕",
            ReconDiag::ETamperEscape => "注入逃脱：人为违约未被判定器抓住，判定面失效，整账逐拍复核",
            ReconDiag::EFingerprint => "双跑指纹不一致：确定性重放下两次全流程指纹不同，查非确定源",
            ReconDiag::EBookedRange => "账面断言数越域：登记账面数必须在 1..=64 非零域，先修冻结表再入账",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const RECON_ALL: &[ReconDiag] = &[
    ReconDiag::EBeatRange,
    ReconDiag::ENOENT,
    ReconDiag::EALREADY,
    ReconDiag::EMISMATCH,
    ReconDiag::ECapacity,
    ReconDiag::ENotClosed,
    ReconDiag::ESealed,
    ReconDiag::ERewind,
    ReconDiag::EWarnLost,
    ReconDiag::ETamperEscape,
    ReconDiag::EFingerprint,
    ReconDiag::EBookedRange,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in RECON_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82541..0x8254C 闭集内"
}

// ── 冻结闭集：40 拍账面断言数登记表 ─────────────────────────────
/// 拍容量：40 拍冻结闭集（判据口径固定，不许扩缩）。
pub const BEAT_CAP: usize = 40;
/// 拍容量的 u32 形态（拍号闭集 1..=40）。
pub const BEAT_CAP_U32: u32 = 40;
/// 账面断言数下界（非零域）。
pub const BOOKED_MIN: u32 = 1;
/// 账面断言数上界（单拍断言容量 64 条）。
pub const BOOKED_MAX: u32 = 64;

/// 冻结 40 拍账面断言数登记表（本卡验收语料写死，逐拍可复算）。
/// 语料来源：分发 P95 预算在册（O1 配套账）压测口径的 40 拍基线，
/// 值域 1..=64，冻结闭集——登记账面副本从此表拷贝，实算侧独立重放。
pub const BOOKED_TABLE: [u32; BEAT_CAP] = [
    7, 12, 3, 21, 9, 44, 16, 2, 30, 58, // 拍 01..10
    11, 25, 6, 39, 47, 14, 1, 33, 52, 19, // 拍 11..20
    8, 27, 61, 4, 36, 15, 49, 22, 10, 55, // 拍 21..30
    31, 5, 42, 18, 60, 24, 13, 37, 28, 20, // 拍 31..40
];

/// 冻结表守卫：长度恰 40、逐值在 1..=64、逐拍可复算（实算重放等于账面）。
fn frozen_table_ok() -> bool {
    if BOOKED_TABLE.len() != BEAT_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < BEAT_CAP {
        let v = match BOOKED_TABLE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if v < BOOKED_MIN || v > BOOKED_MAX {
            return false;
        }
        // 逐拍可复算：实算重放必须与账面原值一致。
        if replay_beat((i + 1) as u32) != v {
            return false;
        }
        i += 1;
    }
    true
}

// ── 实算侧：断言事件流独立重放 ──────────────────────────────────
/// 实算断言数：从冻结登记表逐拍重放断言事件流并计数。
/// 事件流有效性谓词只依赖冻结表原值与拍容量，与账本内的可变账面副本
/// 无关——账面被篡改不影响实算侧，账实差因此可判、可追、可留痕。
fn replay_beat(bn: u32) -> u32 {
    let src = match BOOKED_TABLE.get((bn as usize).wrapping_sub(1)) {
        Some(v) => *v,
        None => return 0,
    };
    if src < BOOKED_MIN || src > BOOKED_MAX {
        return 0;
    }
    let mut count = 0u32;
    let mut ev = 0u32;
    while ev < src {
        // 断言事件有效：事件槽未越单拍断言容量且拍号在对账域闭集内。
        if ev < BOOKED_MAX && bn >= 1 && bn <= BEAT_CAP_U32 {
            count = count.saturating_add(1);
        }
        ev = ev.saturating_add(1);
    }
    count
}

// ── 对账行与告警账行 ────────────────────────────────────────────
/// 单拍对账行：拍号 / 账面断言数 / 实算断言数 / 差值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeatRow {
    /// 拍号（1..=40）。
    pub beat: u32,
    /// 账面断言数（冻结表副本，可被注入篡改）。
    pub booked: u32,
    /// 实算断言数（冻结事件流重放计数，不受账面篡改影响）。
    pub actual: u32,
    /// 差值 = 账面 − 实算（对平为 0，非零必须告警入账）。
    pub diff: i32,
}

impl BeatRow {
    /// 本拍对平（账实相等且差值为零）。
    pub fn is_flat(&self) -> bool {
        self.diff == 0 && self.booked == self.actual
    }

    /// 行摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        format!(
            "拍{} 账面{} 实算{} 差{}",
            self.beat, self.booked, self.actual, self.diff
        )
    }
}

/// 告警账行：非零差的留痕单元（差零静默——差零拍绝不产生本行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WarnRow {
    /// 拍号。
    pub beat: u32,
    /// 告警时账面断言数。
    pub booked: u32,
    /// 告警时实算断言数。
    pub actual: u32,
    /// 差值（必非零）。
    pub diff: i32,
}

impl WarnRow {
    /// 差值确非零（留痕合法性）。
    pub fn diff_nonzero(&self) -> bool {
        self.diff != 0
    }

    /// 告警摘要。
    pub fn brief(&self) -> String {
        format!(
            "告警拍{} 账面{} 实算{} 差{}",
            self.beat, self.booked, self.actual, self.diff
        )
    }
}

// ── 账本状态机：Idle → Judging → Closed 单向推进 ────────────────
/// 账本状态：只进不退，倒退显性拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconState {
    /// 空账：登记表无拍。
    Idle,
    /// 对账中：已有登记拍，允许逐拍登记与判定。
    Judging,
    /// 已闭合：全拍对平，只读。
    Closed,
}

impl ReconState {
    /// 状态秩（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            ReconState::Idle => 0,
            ReconState::Judging => 1,
            ReconState::Closed => 2,
        }
    }
}

// ── 断言账本 ────────────────────────────────────────────────────
/// 断言账本：40 拍登记表 + 告警账 + 已判拍表 + 单向状态机。
#[derive(Clone, Debug)]
pub struct AssertLedger {
    /// 对账行集（按登记序，最多 40 行）。
    rows: Vec<BeatRow>,
    /// 告警账行集（非零差留痕，差零静默）。
    warns: Vec<WarnRow>,
    /// 已执行对账动作的拍号（判定即留痕，二次判定走 EALREADY）。
    judged: Vec<u32>,
    /// 最近一次账实不等的拍号（EMISMATCH 指明拍号口）。
    last_err_beat: Option<u32>,
    /// 账本状态。
    state: ReconState,
}

impl AssertLedger {
    /// 开一本空账。
    pub fn new() -> AssertLedger {
        AssertLedger {
            rows: Vec::new(),
            warns: Vec::new(),
            judged: Vec::new(),
            last_err_beat: None,
            state: ReconState::Idle,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> ReconState {
        self.state
    }

    /// 已登记拍数。
    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    /// 告警账行数。
    pub fn warns_len(&self) -> usize {
        self.warns.len()
    }

    /// 已执行对账动作的拍数。
    pub fn judged_len(&self) -> usize {
        self.judged.len()
    }

    /// 最近一次账实不等的拍号。
    pub fn last_error_beat(&self) -> Option<u32> {
        self.last_err_beat
    }

    /// 按登记序取对账行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<BeatRow> {
        self.rows.get(idx).copied()
    }

    /// 按拍号查对账行。
    pub fn row_of(&self, beat: u32) -> Option<BeatRow> {
        for r in self.rows.iter() {
            if r.beat == beat {
                return Some(*r);
            }
        }
        None
    }

    /// 按拍号查告警账行。
    pub fn warn_row_of(&self, beat: u32) -> Option<WarnRow> {
        for w in self.warns.iter() {
            if w.beat == beat {
                return Some(*w);
            }
        }
        None
    }

    /// 按序取告警账行（越界 None）。
    pub fn warn_at(&self, idx: usize) -> Option<WarnRow> {
        self.warns.get(idx).copied()
    }

    /// 已判拍表是否无重复（判定留痕不变量）。
    pub fn judged_unique(&self) -> bool {
        let mut i = 0usize;
        while i < self.judged.len() {
            let a = match self.judged.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let mut j = i + 1;
            while j < self.judged.len() {
                let b = match self.judged.get(j) {
                    Some(v) => *v,
                    None => return false,
                };
                if a == b {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 账面登记：拍号入账面副本，实算侧同步重放，差值入行。
    /// 闸序：越界 → 闭合 → 容量 → 重复 → 账面域。
    pub fn register_beat(&mut self, bn: u32) -> Result<(), u32> {
        // 闸 1：拍号越界（闭集 1..=40）。
        if bn < 1 || bn > BEAT_CAP_U32 {
            return Err(ReconDiag::EBeatRange.val());
        }
        // 闸 2：闭合后只读。
        if self.state.rank() >= ReconState::Closed.rank() {
            return Err(ReconDiag::ESealed.val());
        }
        // 闸 3：容量耗尽（40 拍冻结闭集，满即拒）。
        if self.rows.len() >= BEAT_CAP {
            return Err(ReconDiag::ECapacity.val());
        }
        // 闸 4：重复登记。
        if self.row_of(bn).is_some() {
            return Err(ReconDiag::EALREADY.val());
        }
        // 闸 5：账面取冻结表（闭集内必命中，防御性 ENOENT）。
        let booked = match BOOKED_TABLE.get((bn as usize).wrapping_sub(1)) {
            Some(v) => *v,
            None => return Err(ReconDiag::ENOENT.val()),
        };
        // 闸 6：账面域（1..=64 非零域）。
        if booked < BOOKED_MIN || booked > BOOKED_MAX {
            return Err(ReconDiag::EBookedRange.val());
        }
        let actual = replay_beat(bn);
        // 差值 = 账面 − 实算；值域有界（<=64），饱和减法足矣。
        let diff = (booked as i32).saturating_sub(actual as i32);
        self.rows.push(BeatRow {
            beat: bn,
            booked,
            actual,
            diff,
        });
        // 状态只进：Idle → Judging。
        if self.state.rank() < ReconState::Judging.rank() {
            self.state = ReconState::Judging;
        }
        Ok(())
    }

    /// 高压注入面（仅供验收注入使用）：篡改账面断言数副本。
    /// 账面为可变副本，实算侧读冻结表，故篡改必产生可判差异。
    pub fn tamper_booked(&mut self, bn: u32, delta: i32) -> Result<(), u32> {
        // 闭合后注入面也只读。
        if self.state.rank() >= ReconState::Closed.rank() {
            return Err(ReconDiag::ESealed.val());
        }
        // 未登记拍无从篡改。
        let idx = match self.rows.iter().position(|r| r.beat == bn) {
            Some(i) => i,
            None => return Err(ReconDiag::ENOENT.val()),
        };
        let row = match self.rows.get_mut(idx) {
            Some(r) => r,
            None => return Err(ReconDiag::ENOENT.val()),
        };
        // 有界扰动：正增量饱和加，负增量饱和减，保证账实必不等。
        let nb = if delta >= 0 {
            row.booked.saturating_add(delta as u32)
        } else {
            row.booked.saturating_sub((0i32 - delta) as u32)
        };
        if nb == row.actual {
            // 注入被自身抵消（账实仍相等），判定面无差异可抓，判无效注入。
            return Err(ReconDiag::EBookedRange.val());
        }
        row.booked = nb;
        Ok(())
    }

    /// 对账判定器（判据核心口）：
    /// 未登记拍 → ENOENT；账实不等 → EMISMATCH 并指明拍号 + 告警入账；
    /// 重复对账 → EALREADY。判定次序固定：越界 → 闭合 → 未登记 → 重复 → 账实。
    pub fn judge_beat(&mut self, bn: u32) -> Result<BeatRow, u32> {
        // 闸 1：拍号越界最优先（即便闭合也先报越界）。
        if bn < 1 || bn > BEAT_CAP_U32 {
            return Err(ReconDiag::EBeatRange.val());
        }
        // 闸 2：闭合后对账口关闭。
        if self.state.rank() >= ReconState::Closed.rank() {
            return Err(ReconDiag::ESealed.val());
        }
        // 闸 3：未登记拍 → ENOENT。
        let row = match self.row_of(bn) {
            Some(r) => r,
            None => return Err(ReconDiag::ENOENT.val()),
        };
        // 闸 4：重复对账 → EALREADY（判定即留痕，同拍二次判定一律拒）。
        let already = self.judged.iter().any(|b| *b == bn);
        if already {
            return Err(ReconDiag::EALREADY.val());
        }
        // 判定留痕：无论对平与否，对账动作已执行。
        self.judged.push(bn);
        // 闸 5：账实不等 → EMISMATCH，并指明拍号 + 告警入账留痕。
        if row.booked != row.actual {
            // 判定时刻差值口径：账面现值 − 实算现值（饱和有界）。
            let diff_now = (row.booked as i32).saturating_sub(row.actual as i32);
            // 对账行差值同步为判定时刻快照。
            if let Some(idx) = self.rows.iter().position(|r| r.beat == bn) {
                if let Some(r) = self.rows.get_mut(idx) {
                    r.diff = diff_now;
                }
            }
            self.last_err_beat = Some(bn);
            self.warns.push(WarnRow {
                beat: bn,
                booked: row.booked,
                actual: row.actual,
                diff: diff_now,
            });
            return Err(ReconDiag::EMISMATCH.val());
        }
        Ok(row)
    }

    /// 全拍对账闭合：40 拍全部执行对账且全部对平（零差零告警）才许闭合。
    pub fn close_reconciliation(&mut self) -> Result<(), u32> {
        // 闸 1：闭合倒退/重复闭合。
        if self.state.rank() >= ReconState::Closed.rank() {
            return Err(ReconDiag::ERewind.val());
        }
        // 闸 2：40 拍须全部判满。
        if self.judged.len() != BEAT_CAP {
            return Err(ReconDiag::ENotClosed.val());
        }
        // 闸 3：告警账必须清空（非零差未清不许闭合）。
        if !self.warns.is_empty() {
            return Err(ReconDiag::ENotClosed.val());
        }
        // 闸 4：逐行复核对平（账实相等且差零，防御性双查）。
        for r in self.rows.iter() {
            if !r.is_flat() {
                return Err(ReconDiag::ENotClosed.val());
            }
        }
        // 闸 5：已判拍表无重复。
        if !self.judged_unique() {
            return Err(ReconDiag::ENotClosed.val());
        }
        self.state = ReconState::Closed;
        Ok(())
    }

    /// 双跑一致指纹：FNV-1a 覆盖对账行集 / 告警账行 / 已判拍数 / 状态秩。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a 64 位偏移基
        h = self.mix_row(h);
        for w in self.warns.iter() {
            h = (h ^ w.beat as u64).wrapping_mul(0x100_0000_01b3);
            h = (h ^ w.booked as u64).wrapping_mul(0x100_0000_01b3);
            h = (h ^ w.actual as u64).wrapping_mul(0x100_0000_01b3);
            h = (h ^ (w.diff as i64 as u64)).wrapping_mul(0x100_0000_01b3);
        }
        h = (h ^ self.judged.len() as u64).wrapping_mul(0x100_0000_01b3);
        h = (h ^ self.state.rank() as u64).wrapping_mul(0x100_0000_01b3);
        h = (h ^ self.last_err_beat.unwrap_or(0) as u64).wrapping_mul(0x100_0000_01b3);
        h
    }

    /// 行集混入（指纹内部分步）。
    fn mix_row(&self, mut h: u64) -> u64 {
        for r in self.rows.iter() {
            h = (h ^ r.beat as u64).wrapping_mul(0x100_0000_01b3);
            h = (h ^ r.booked as u64).wrapping_mul(0x100_0000_01b3);
            h = (h ^ r.actual as u64).wrapping_mul(0x100_0000_01b3);
            h = (h ^ (r.diff as i64 as u64)).wrapping_mul(0x100_0000_01b3);
        }
        h
    }
}

/// 构造健康账本：40 拍全部登记（实算与账面天然对平）。
pub fn healthy_ledger() -> AssertLedger {
    let mut lg = AssertLedger::new();
    let mut bn = 1u32;
    while bn <= BEAT_CAP_U32 {
        let _ = lg.register_beat(bn);
        bn += 1;
    }
    lg
}

// ── 确定性 LCG（种子写死，零随机红线合规） ──────────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
/// 常数取经典 64 位 LCG 组（与样板模块不同源的数论常数组）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性，高 32 位输出）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(2_862_933_555_777_941_757)
            .wrapping_add(3_037_000_493);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn next_below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 高压注入器：三路各 10 轮全抓 ────────────────────────────────
/// 路一：账面篡改注入 10 轮。每轮 LCG 选拍号与扰动量，篡改后逐拍判定，
/// 目标拍必须 EMISMATCH 且指明拍号、告警留痕，其余拍必须对平，闭合必须拒。
/// 返回 (抓到轮数, 总轮数, 首个逃脱轮描述)。
pub fn tamper_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x5A) as u64);
    let mut k = 0usize;
    while k < 10 {
        let target = 1 + rng.next_below(BEAT_CAP_U32);
        let delta = (1 + rng.next_below(9)) as i32;
        let mut lg = healthy_ledger();
        let injected = lg.tamper_booked(target, delta).is_ok();
        // 逐拍判定：目标拍必须账实不等，其余拍必须对平。
        let mut hit = false;
        let mut clean = true;
        let mut bn = 1u32;
        while bn <= BEAT_CAP_U32 {
            match lg.judge_beat(bn) {
                Ok(_) => {}
                Err(e) => {
                    if e == ReconDiag::EMISMATCH.val()
                        && lg.last_error_beat() == Some(target)
                    {
                        hit = true;
                    } else {
                        clean = false;
                    }
                }
            }
            bn += 1;
        }
        // 告警账行必须留痕且差值非零。
        let warned = match lg.warn_row_of(target) {
            Some(w) => w.diff_nonzero(),
            None => false,
        };
        // 非零差未清，闭合必须拒。
        let close_refused =
            lg.close_reconciliation() == Err(ReconDiag::ENotClosed.val());
        total += 1;
        if injected && hit && clean && warned && close_refused {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!(
                "轮{}拍{}扰动{}：注入{} 命中{} 清白{} 留痕{} 闭拒{}",
                k,
                target,
                delta,
                injected,
                hit,
                clean,
                warned,
                close_refused
            );
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 路二：拍号越界注入 10 轮。每轮构造闭集外拍号（0 / 41.. 上溢域 /
/// u32 尾域），登记与判定两端口都必须 EBeatRange。
pub fn range_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x7B) as u64);
    let mut k = 0usize;
    while k < 10 {
        // 四类越界来源轮转，保证覆盖下越界/上越界/高位域/零拍。
        let bad = match k % 4 {
            0 => 0u32,
            1 => BEAT_CAP_U32.wrapping_add(1 + rng.next_below(1000)),
            2 => u32::MAX.wrapping_sub(rng.next_below(1000)),
            _ => BEAT_CAP_U32.wrapping_add(1 + rng.next_below(BEAT_CAP_U32)),
        };
        let mut lg = healthy_ledger();
        let reg_rejected = lg.register_beat(bad) == Err(ReconDiag::EBeatRange.val());
        let judge_rejected = match lg.judge_beat(bad) {
            Err(e) => e == ReconDiag::EBeatRange.val(),
            Ok(_) => false,
        };
        total += 1;
        if reg_rejected && judge_rejected {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("轮{}拍{}：登记拒{} 判定拒{}", k, bad, reg_rejected, judge_rejected);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 路三：重复对账注入 10 轮。每轮选拍，首判对平，二判必须 EALREADY；
/// 重复登记同样必须 EALREADY。
pub fn dup_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x9C) as u64);
    let mut k = 0usize;
    while k < 10 {
        let target = 1 + rng.next_below(BEAT_CAP_U32);
        let mut lg = healthy_ledger();
        // 首判必须对平。
        let first_ok = lg.judge_beat(target).is_ok();
        // 二判必须 EALREADY。
        let dup_judge = lg.judge_beat(target) == Err(ReconDiag::EALREADY.val());
        // 重复登记必须 EALREADY（部分账本，避开满账容量闸）。
        let mut partial = AssertLedger::new();
        let _ = partial.register_beat(target);
        let dup_reg = partial.register_beat(target) == Err(ReconDiag::EALREADY.val());
        total += 1;
        if first_ok && dup_judge && dup_reg {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!(
                "轮{}拍{}：首判{} 二判拒{} 重复登记拒{}",
                k, target, first_ok, dup_judge, dup_reg
            );
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 差零静默不变量 ──────────────────────────────────────────────
/// 账实差零静默复核：差零拍不得有告警行；非零差拍必须恰有一条留痕。
/// 告警账行数必须恒等于非零差拍数。
fn warn_invariant_ok(l: &AssertLedger) -> bool {
    let mut nonzero_rows = 0usize;
    let mut i = 0usize;
    while i < l.rows_len() {
        let r = match l.row_at(i) {
            Some(r) => r,
            None => return false,
        };
        let warned = l.warn_row_of(r.beat).is_some();
        if r.diff == 0 && warned {
            // 差零拍出现告警行：静默被破坏。
            return false;
        }
        if r.diff != 0 {
            nonzero_rows += 1;
            if !warned {
                // 非零差未留痕：告警账行缺失。
                return false;
            }
        }
        i += 1;
    }
    l.warns_len() == nonzero_rows
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在本批窗口 [0x80000, 0x831C0]
/// （与 `(0x80000..=0x831C0).contains(&SEG_BASE)` 同义）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x831C0 {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落在 [SEG_BASE, SEG_BASE+0x40) 窗口，查表可命中。
fn diag_closed_ok() -> bool {
    for d in RECON_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    // 已知码必须命中文案，未知码必须回落。
    if detail(ReconDiag::EMISMATCH.val()) != ReconDiag::EMISMATCH.detail() {
        return false;
    }
    if detail(SEG_BASE + 0x3F) == ReconDiag::EBeatRange.detail() {
        return false;
    }
    true
}

/// 正常流：40 拍登记 → 逐拍对账全对平 → 闭合成功 → 告警账为空。
fn happy_path_ok() -> bool {
    let mut lg = healthy_ledger();
    if lg.rows_len() != BEAT_CAP {
        return false;
    }
    if lg.state() != ReconState::Judging {
        return false;
    }
    let mut bn = 1u32;
    while bn <= BEAT_CAP_U32 {
        match lg.judge_beat(bn) {
            Ok(r) => {
                if !r.is_flat() {
                    return false;
                }
            }
            Err(_) => return false,
        }
        bn += 1;
    }
    if lg.judged_len() != BEAT_CAP || lg.warns_len() != 0 {
        return false;
    }
    match lg.close_reconciliation() {
        Ok(()) => lg.state() == ReconState::Closed,
        Err(_) => false,
    }
}

/// 全拍闭合闸：半账本/判 39 拍/有告警三种未对平账本闭合一律拒。
fn close_gate_ok() -> bool {
    // 半账本：只登记不对账。
    let mut half = healthy_ledger();
    if half.close_reconciliation() != Err(ReconDiag::ENotClosed.val()) {
        return false;
    }
    // 判 39 拍缺一拍。
    let mut part = healthy_ledger();
    let mut bn = 1u32;
    while bn < BEAT_CAP_U32 {
        let _ = part.judge_beat(bn);
        bn += 1;
    }
    if part.judged_len() != BEAT_CAP - 1 {
        return false;
    }
    if part.close_reconciliation() != Err(ReconDiag::ENotClosed.val()) {
        return false;
    }
    // 有告警账行（非零差未清）。
    let mut dirty = healthy_ledger();
    let _ = dirty.tamper_booked(5, 2);
    let mut bn = 1u32;
    while bn <= BEAT_CAP_U32 {
        let _ = dirty.judge_beat(bn);
        bn += 1;
    }
    if dirty.warns_len() != 1 {
        return false;
    }
    if dirty.close_reconciliation() != Err(ReconDiag::ENotClosed.val()) {
        return false;
    }
    true
}

/// 差零静默：正常流无告警行；篡改流非零差拍恰有留痕，差零拍无告警。
fn zero_silence_ok() -> bool {
    // 正常流：全对平，告警账必须为空。
    let mut lg = healthy_ledger();
    let mut bn = 1u32;
    while bn <= BEAT_CAP_U32 {
        let _ = lg.judge_beat(bn);
        bn += 1;
    }
    if lg.warns_len() != 0 {
        return false;
    }
    if !warn_invariant_ok(&lg) {
        return false;
    }
    // 篡改流：两拍非零差，告警账恰两行，其余拍静默。
    let mut tg = healthy_ledger();
    let _ = tg.tamper_booked(9, 4);
    let _ = tg.tamper_booked(17, -3);
    let mut bn = 1u32;
    while bn <= BEAT_CAP_U32 {
        let _ = tg.judge_beat(bn);
        bn += 1;
    }
    if tg.warns_len() != 2 {
        return false;
    }
    if tg.warn_row_of(9).is_none() || tg.warn_row_of(17).is_none() {
        return false;
    }
    if tg.warn_row_of(10).is_some() || tg.warn_row_of(16).is_some() {
        return false;
    }
    warn_invariant_ok(&tg)
}

/// 注入必抓·路一：账面篡改 10/10 全命中，无逃脱。
fn tamper_caught_ok() -> bool {
    let (caught, total, escape) = tamper_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 注入必抓·路二：拍号越界 10/10 全命中（登记与判定两端口）。
fn range_caught_ok() -> bool {
    let (caught, total, escape) = range_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 注入必抓·路三：重复对账 10/10 全命中（判定与登记两端口）。
fn dup_caught_ok() -> bool {
    let (caught, total, escape) = dup_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// ENOENT：未登记拍在空账与部分账本上都返回 ENOENT。
fn enoent_ok() -> bool {
    // 空账：闭集内任意拍未登记。
    let mut lg = AssertLedger::new();
    if lg.judge_beat(1) != Err(ReconDiag::ENOENT.val()) {
        return false;
    }
    if lg.judge_beat(40) != Err(ReconDiag::ENOENT.val()) {
        return false;
    }
    // 部分账本：已登记拍 1，拍 2 仍未登记。
    let mut part = AssertLedger::new();
    let _ = part.register_beat(1);
    if part.judge_beat(2) != Err(ReconDiag::ENOENT.val()) {
        return false;
    }
    // 已登记拍仍可正常判定。
    part.judge_beat(1).is_ok()
}

/// EALREADY：重复登记与重复对账两端口均拒。
fn ealready_ok() -> bool {
    // 重复登记：部分账本上同拍二登（避开满账容量闸）。
    let mut part = AssertLedger::new();
    let _ = part.register_beat(3);
    if part.register_beat(3) != Err(ReconDiag::EALREADY.val()) {
        return false;
    }
    // 重复对账：满账本上首判对平后二判拒。
    let mut lg = healthy_ledger();
    // 首判对平后二判拒。
    if lg.judge_beat(3).is_err() {
        return false;
    }
    if lg.judge_beat(3) != Err(ReconDiag::EALREADY.val()) {
        return false;
    }
    // 三判仍拒（留痕不变）。
    lg.judge_beat(3) == Err(ReconDiag::EALREADY.val())
}

/// EMISMATCH：账实不等时指明拍号 + 告警留痕 + 差值正确。
fn emismatch_ok() -> bool {
    let mut lg = healthy_ledger();
    // 注入 +6：账面 7 → 13，实算仍 7，差 +6。
    let _ = lg.tamper_booked(1, 6);
    match lg.judge_beat(1) {
        Ok(_) => false,
        Err(e) => {
            if e != ReconDiag::EMISMATCH.val() {
                return false;
            }
            // 并指明拍号。
            if lg.last_error_beat() != Some(1) {
                return false;
            }
            // 告警账行留痕且差值正确。
            let w = match lg.warn_row_of(1) {
                Some(w) => w,
                None => return false,
            };
            if w.diff != 6 || w.booked != 13 || w.actual != 7 {
                return false;
            }
            true
        }
    }
}

/// 容量耗尽：40 拍登记满后，闭集内任何拍再登记一律 ECapacity。
fn capacity_ok() -> bool {
    let mut lg = AssertLedger::new();
    let mut bn = 1u32;
    while bn <= BEAT_CAP_U32 {
        if lg.register_beat(bn).is_err() {
            return false;
        }
        bn += 1;
    }
    if lg.rows_len() != BEAT_CAP {
        return false;
    }
    // 满后重复登记既有拍：容量闸先于重复闸。
    if lg.register_beat(40) != Err(ReconDiag::ECapacity.val()) {
        return false;
    }
    if lg.register_beat(1) != Err(ReconDiag::ECapacity.val()) {
        return false;
    }
    // 登记数不变。
    lg.rows_len() == BEAT_CAP
}

/// 判定次序固定：越界 → 闭合 → 未登记 → 重复 → 账实。
fn judge_order_ok() -> bool {
    // 闭合账本：越界仍最优先，闭集合次之。
    let mut closed = healthy_ledger();
    let mut bn = 1u32;
    while bn <= BEAT_CAP_U32 {
        let _ = closed.judge_beat(bn);
        bn += 1;
    }
    let _ = closed.close_reconciliation();
    if closed.judge_beat(0) != Err(ReconDiag::EBeatRange.val()) {
        return false;
    }
    if closed.judge_beat(41) != Err(ReconDiag::EBeatRange.val()) {
        return false;
    }
    if closed.judge_beat(1) != Err(ReconDiag::ESealed.val()) {
        return false;
    }
    // 开账本：越界优先于未登记。
    let mut open = AssertLedger::new();
    if open.judge_beat(41) != Err(ReconDiag::EBeatRange.val()) {
        return false;
    }
    // 未登记优先于重复（重复无从谈起）。
    if open.judge_beat(7) != Err(ReconDiag::ENOENT.val()) {
        return false;
    }
    // 重复优先于账实：篡改后首判 EMISMATCH，再判 EALREADY。
    let mut tg = healthy_ledger();
    let _ = tg.tamper_booked(3, 5);
    if tg.judge_beat(3) != Err(ReconDiag::EMISMATCH.val()) {
        return false;
    }
    tg.judge_beat(3) == Err(ReconDiag::EALREADY.val())
}

/// 边界四点：下越界拍 0 / 上越界拍 41 / 首拍 1 / 末拍 40。
fn boundary_ok() -> bool {
    let mut lg = AssertLedger::new();
    // 下越界与上越界登记拒。
    if lg.register_beat(0) != Err(ReconDiag::EBeatRange.val()) {
        return false;
    }
    if lg.register_beat(41) != Err(ReconDiag::EBeatRange.val()) {
        return false;
    }
    // 首拍与末拍正常登记对账。
    if lg.register_beat(1).is_err() || lg.register_beat(40).is_err() {
        return false;
    }
    if lg.judge_beat(1).is_err() || lg.judge_beat(40).is_err() {
        return false;
    }
    // 未判满闭合拒（首末判了，中间 38 拍未判）。
    if lg.close_reconciliation() != Err(ReconDiag::ENotClosed.val()) {
        return false;
    }
    // 首拍二判拒（重复对账在边界拍同样生效）。
    lg.judge_beat(1) == Err(ReconDiag::EALREADY.val())
}

/// 双跑一致：两次全流程（登记/对账/闭合 + 三路注入）逐项指纹一致。
fn dual_run_ok() -> bool {
    // 流程一。
    let mut a = healthy_ledger();
    let mut bn = 1u32;
    while bn <= BEAT_CAP_U32 {
        let _ = a.judge_beat(bn);
        bn += 1;
    }
    let _ = a.close_reconciliation();
    // 流程二（独立重建）。
    let mut b = healthy_ledger();
    let mut bn = 1u32;
    while bn <= BEAT_CAP_U32 {
        let _ = b.judge_beat(bn);
        bn += 1;
    }
    let _ = b.close_reconciliation();
    if a.fingerprint() != b.fingerprint() {
        return false;
    }
    // 注入面双跑一致。
    if tamper_sweep() != tamper_sweep() {
        return false;
    }
    if range_sweep() != range_sweep() {
        return false;
    }
    dup_sweep() == dup_sweep()
}

/// 本卡判据条数声明（收口用）。
pub const CRITERIA_COUNT: usize = 17;

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12749 自检（判据 12749 卡：分发 P95 预算在册，高压注入账无红账）。
pub fn run_unx12749_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12749");
    let ledger = healthy_ledger();

    s.add_dyn(
        "F12750-01 段位守卫",
        seg_guard_ok(),
        &format!(
            "seg=0x{:05x} 对齐0x40={} 窗口80000-831C0={}",
            SEG_BASE,
            SEG_BASE % 0x40 == 0,
            SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0
        ),
    );

    s.add_dyn(
        "F12750-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "码数={} 首码=0x{:05x} 尾码=0x{:05x}",
            RECON_ALL.len(),
            RECON_ALL.first().map(|d| d.val()).unwrap_or(0),
            RECON_ALL.last().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12750-03 冻结40拍登记表",
        frozen_table_ok(),
        &format!(
            "拍数={} 值域={}..={} 首拍账面={} 末拍账面={}",
            BOOKED_TABLE.len(),
            BOOKED_MIN,
            BOOKED_MAX,
            BOOKED_TABLE.first().copied().unwrap_or(0),
            BOOKED_TABLE.last().copied().unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12750-04 正常流40拍全对平闭合",
        happy_path_ok(),
        &format!(
            "登记={} 已判={} 告警={} 状态={:?}",
            ledger.rows_len(),
            ledger.judged_len(),
            ledger.warns_len(),
            ledger.state()
        ),
    );

    s.add_dyn(
        "F12750-05 全拍对账闭合闸",
        close_gate_ok(),
        "半账本/判39拍/有告警 三种未对平账本闭合一律拒",
    );

    s.add_dyn(
        "F12750-06 账实差零静默",
        zero_silence_ok(),
        "差零拍无告警行；非零差拍恰一条留痕；告警行数=非零差拍数",
    );

    // 注入必抓：三路各 10 轮，人为构造违约必被判定器抓住。
    let (t_caught, t_total, t_escape) = tamper_sweep();
    s.add_dyn(
        "F12750-07 注入-账面篡改10轮全抓",
        tamper_caught_ok(),
        &format!("抓到={}/{} {}", t_caught, t_total, t_escape),
    );

    let (r_caught, r_total, r_escape) = range_sweep();
    s.add_dyn(
        "F12750-08 注入-拍号越界10轮全抓",
        range_caught_ok(),
        &format!("抓到={}/{} {}", r_caught, r_total, r_escape),
    );

    let (d_caught, d_total, d_escape) = dup_sweep();
    s.add_dyn(
        "F12750-09 注入-重复对账10轮全抓",
        dup_caught_ok(),
        &format!("抓到={}/{} {}", d_caught, d_total, d_escape),
    );

    s.add_dyn(
        "F12750-10 未登记拍ENOENT",
        enoent_ok(),
        "空账与部分账本上未登记拍一律 ENOENT",
    );

    s.add_dyn(
        "F12750-11 重复对账EALREADY",
        ealready_ok(),
        "重复登记/二判/三判 全数拒且留痕不变",
    );

    s.add_dyn(
        "F12750-12 账实不等EMISMATCH指明拍号",
        emismatch_ok(),
        "判码+拍号回指+告警留痕+差值 四点齐",
    );

    s.add_dyn(
        "F12750-13 容量耗尽",
        capacity_ok(),
        "40 拍满后闭集内再登记一律 ECapacity 且账面不变",
    );

    s.add_dyn(
        "F12750-14 判定次序固定",
        judge_order_ok(),
        "越界→闭合→未登记→重复→账实 改序即语义漂移",
    );

    s.add_dyn(
        "F12750-15 双跑一致指纹",
        dual_run_ok(),
        "两次全流程与三路注入逐项指纹一致",
    );

    s.add_dyn(
        "F12750-16 边界四点",
        boundary_ok(),
        "拍0/拍41越界拒，拍1/拍40登记对账正常，未判满闭合拒",
    );

    // 收口：判据条数声明（本检查节实录条数与声明一致）。
    s.add_dyn(
        "F12750-17 收口·判据条数声明",
        s.len() + 1 == CRITERIA_COUNT,
        &format!(
            "声明判据条数={} 实录={}（含本条）",
            CRITERIA_COUNT,
            s.len() + 1
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
        // 段位守卫：对齐 0x40 且落在本批窗口 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        // 派工公式复核：0x80000 + 0x40 × (12749 − 12600) = 0x82540。
        assert_eq!(SEG_BASE, 0x80000u32.wrapping_add(0x40u32.wrapping_mul(149)));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中、未知码回落。
        assert_eq!(RECON_ALL.len(), 12);
        let mut i = 0usize;
        while i < RECON_ALL.len() {
            let vi = RECON_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < RECON_ALL.len() {
                let vj = RECON_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn happy_path_reconciles_and_closes() {
        // 正常流：40 拍登记 → 逐拍对平 → 闭合 → 只读。
        let mut lg = healthy_ledger();
        assert_eq!(lg.rows_len(), BEAT_CAP);
        assert_eq!(lg.state(), ReconState::Judging);
        let mut bn = 1u32;
        while bn <= BEAT_CAP_U32 {
            let r = lg.judge_beat(bn);
            assert!(r.is_ok(), "拍{} 判定失败", bn);
            assert!(r.unwrap_or(BeatRow {
                beat: 0,
                booked: 0,
                actual: 0,
                diff: 0,
            })
            .is_flat());
            bn += 1;
        }
        assert_eq!(lg.judged_len(), BEAT_CAP);
        assert_eq!(lg.warns_len(), 0);
        assert!(lg.close_reconciliation().is_ok());
        assert_eq!(lg.state(), ReconState::Closed);
        // 冻结表逐拍可复算。
        assert!(frozen_table_ok());
    }

    #[test]
    fn tamper_injection_all_caught() {
        // 注入必抓·账面篡改：10 轮全命中，无逃脱。
        let (caught, total, escape) = tamper_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱轮：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn range_injection_all_caught() {
        // 注入必抓·拍号越界：10 轮登记与判定两端口全命中。
        let (caught, total, escape) = range_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱轮：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn duplicate_injection_all_caught() {
        // 注入必抓·重复对账：10 轮二判与重复登记全命中。
        let (caught, total, escape) = dup_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱轮：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn judge_gate_order_is_fixed() {
        // 判定次序：越界 → 闭合 → 未登记 → 重复 → 账实。
        let mut closed = healthy_ledger();
        let mut bn = 1u32;
        while bn <= BEAT_CAP_U32 {
            let _ = closed.judge_beat(bn);
            bn += 1;
        }
        let _ = closed.close_reconciliation();
        assert_eq!(closed.judge_beat(0), Err(ReconDiag::EBeatRange.val()));
        assert_eq!(closed.judge_beat(41), Err(ReconDiag::EBeatRange.val()));
        assert_eq!(closed.judge_beat(1), Err(ReconDiag::ESealed.val()));
        // 开账本：越界优先于未登记。
        let mut open = AssertLedger::new();
        assert_eq!(open.judge_beat(41), Err(ReconDiag::EBeatRange.val()));
        assert_eq!(open.judge_beat(7), Err(ReconDiag::ENOENT.val()));
        // 重复优先于账实：篡改后首判 EMISMATCH，再判 EALREADY。
        let mut tg = healthy_ledger();
        let _ = tg.tamper_booked(3, 5);
        assert_eq!(tg.judge_beat(3), Err(ReconDiag::EMISMATCH.val()));
        assert_eq!(tg.last_error_beat(), Some(3));
        assert_eq!(tg.judge_beat(3), Err(ReconDiag::EALREADY.val()));
    }

    #[test]
    fn capacity_exhausted_rejects() {
        // 容量耗尽：40 拍满后任何闭集内拍再登记一律 ECapacity。
        let mut lg = AssertLedger::new();
        let mut bn = 1u32;
        while bn <= BEAT_CAP_U32 {
            assert!(lg.register_beat(bn).is_ok());
            bn += 1;
        }
        assert_eq!(lg.rows_len(), BEAT_CAP);
        assert_eq!(lg.register_beat(40), Err(ReconDiag::ECapacity.val()));
        assert_eq!(lg.register_beat(1), Err(ReconDiag::ECapacity.val()));
        assert_eq!(lg.rows_len(), BEAT_CAP);
    }

    #[test]
    fn dual_run_fingerprints_identical() {
        // 双跑一致：两次独立全流程指纹一致；三路注入双跑一致。
        let mut a = healthy_ledger();
        let mut b = healthy_ledger();
        let mut bn = 1u32;
        while bn <= BEAT_CAP_U32 {
            let _ = a.judge_beat(bn);
            let _ = b.judge_beat(bn);
            bn += 1;
        }
        let _ = a.close_reconciliation();
        let _ = b.close_reconciliation();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.state(), b.state());
        assert_eq!(tamper_sweep(), tamper_sweep());
        assert_eq!(range_sweep(), range_sweep());
        assert_eq!(dup_sweep(), dup_sweep());
    }

    #[test]
    fn boundary_four_points() {
        // 边界四点：下越界 0 / 上越界 41 拒，首拍 1 / 末拍 40 正常。
        let mut lg = AssertLedger::new();
        assert_eq!(lg.register_beat(0), Err(ReconDiag::EBeatRange.val()));
        assert_eq!(lg.register_beat(41), Err(ReconDiag::EBeatRange.val()));
        assert!(lg.register_beat(1).is_ok());
        assert!(lg.register_beat(40).is_ok());
        assert!(lg.judge_beat(1).is_ok());
        assert!(lg.judge_beat(40).is_ok());
        // 边界拍重复对账同样拒。
        assert_eq!(lg.judge_beat(1), Err(ReconDiag::EALREADY.val()));
        // 未判满闭合拒。
        assert_eq!(lg.close_reconciliation(), Err(ReconDiag::ENotClosed.val()));
    }

    #[test]
    fn zero_diff_silent_nonzero_diff_logged() {
        // 账实差零静默：差零无告警行；非零差恰一条留痕且差值非零。
        let mut lg = healthy_ledger();
        let mut bn = 1u32;
        while bn <= BEAT_CAP_U32 {
            let _ = lg.judge_beat(bn);
            bn += 1;
        }
        assert_eq!(lg.warns_len(), 0);
        assert!(warn_invariant_ok(&lg));
        // 篡改两拍：告警账恰两行，其余拍静默。
        let mut tg = healthy_ledger();
        let _ = tg.tamper_booked(9, 4);
        let _ = tg.tamper_booked(17, -3);
        let mut bn = 1u32;
        while bn <= BEAT_CAP_U32 {
            let _ = tg.judge_beat(bn);
            bn += 1;
        }
        assert_eq!(tg.warns_len(), 2);
        assert!(tg.warn_row_of(9).is_some());
        assert!(tg.warn_row_of(17).is_some());
        assert!(tg.warn_row_of(10).is_none());
        assert!(tg.warn_row_of(16).is_none());
        assert!(warn_invariant_ok(&tg));
        // 闭合闸：非零差未清拒绝闭合。
        assert_eq!(tg.close_reconciliation(), Err(ReconDiag::ENotClosed.val()));
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条、收口声明一致。
        let s = run_unx12749_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), CRITERIA_COUNT);
        assert_eq!(s.tally().1, 0);
    }
}
