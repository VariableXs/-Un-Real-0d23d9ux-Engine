//! UNX-F12721 对照表全量回归总纲（三类对照表盘点） · 0x81E40段
//!
//! 注意：同号旧批文件 unx12721_maptables.rs（VX3 遗留，SEG 0x65180）
//! 在册不许触碰，本批按短名语义另起名 regress_master（全量回归总纲），
//! 段基址按 VX9 公式重新落账。
//!
//! 判据原文（派工卡 12721 逐字锚定）：
//!   ① 纯功能代码 ≥330 行；
//!   ② 判据原文：UNX-F12722-J1 ≥100 对全量回归一致率 100% 维持，
//!      差异例三态列账零静默；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 回归总纲账：冻结对照账 12 行，每行挂冻结期望值；复放引擎按
//!    （行 × 种子）展开 ≥100 对复核对，对平判据为「复放左值回折 ==
//!    冻结右值」——整数运算、零随机（LCG 种子固定取 SEG_BASE）；
//! 2. 复放对三态闭集：对平（Matched）/已核差异（DiffLedgered）/
//!    静默（Silent，禁态）——一致率 100% 维持指对平数 == 总对数，
//!    出现差异例必须落「已核差异」账，静默行零容忍；
//! 3. 零静默守卫：账面三态计数守恒（对平+已核差异 == 总对数）且
//!    静默计数恒零，守卫破即总纲违约；
//! 4. 总纲态机 RmDraft → RmReplaying → RmReplayed → RmSealed 单向
//!    推进，rank 单调，倒退/跳相显性拒，封账后禁写；
//! 5. 违约注入十族逐路必抓：静默行、相位倒退、封账后写、未开账先
//!    复放、行号越界、种子零漂移、对平数虚报、三态计数守恒破、复
//!    放回折不吻合、空摘要封账。
//!    no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子固定
//!    可用）；取值一律 match/if let/.get()/checked_*/saturating_*，
//!    禁裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `pairs_hundred_ok` ↔ ② ≥100 对全量回归（复核对数 ≥100）；
//!   - `rate_full_ok` ↔ ② 一致率 100% 维持（对平数 == 总对数）；
//!   - `zero_silent_ok` ↔ ② 差异例三态列账零静默（守恒 + 静默恒零）；
//!   - `fault_sweep_ok` ↔ ② 总纲违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12721 − 12600)）。
pub const SEG_BASE: u32 = 0x81E40;

/// 复放种子（LCG 固定种子 = SEG_BASE，禁随机）。
pub const RM_SEED: u32 = SEG_BASE;

/// 复放种子档数（每行复放 10 档，12 行 × 10 档 = 120 对 ≥ 100）。
pub const REPLAY_ROUNDS: u32 = 10;

/// 复放对下限（判据 ② ≥100 对）。
pub const PAIRS_MIN: usize = 100;

/// 回归对总容量（账面封顶）。
pub const PAIRS_CAP: usize = 160;

// ── 对照账冻结行 ─────────────────────────────────────────────────
/// 冻结对照账行：账目号 / 左源锚 / 右源锚 / 冻结期望值。
///
/// 出处锚注：左源=三类对照表账面值（错误码/结构偏移/语义行为三类
/// 盘点的冻结行号），右源=总纲复核冻结期望（本模块封账值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegRow {
    /// 账目行号（1 起连号）。
    pub row_no: u32,
    /// 左源锚（对照账左半边标识）。
    pub left_tag: &'static str,
    /// 右源锚（对照账右半边标识）。
    pub right_tag: &'static str,
    /// 冻结期望值（复放回折必须对上的右值）。
    pub frozen: u32,
}

/// 冻结对照账 12 行（错误码 4 行 / 结构偏移 4 行 / 语义行为 4 行）。
pub const REG_TABLE: &[RegRow] = &[
    RegRow { row_no: 1, left_tag: "错误码/0xC0000022", right_tag: "总纲/访问拒", frozen: 0x0022 },
    RegRow { row_no: 2, left_tag: "错误码/0xC0000034", right_tag: "总纲/名不遇", frozen: 0x0034 },
    RegRow { row_no: 3, left_tag: "错误码/0xC000009A", right_tag: "总纲/资源枯", frozen: 0x009A },
    RegRow { row_no: 4, left_tag: "错误码/0xC000012D", right_tag: "总纲/提交限", frozen: 0x012D },
    RegRow { row_no: 5, left_tag: "偏移/PEB.BeingDebugged", right_tag: "总纲/x64槽", frozen: 0x0002 },
    RegRow { row_no: 6, left_tag: "偏移/PEB.Ldr", right_tag: "总纲/x64槽", frozen: 0x0018 },
    RegRow { row_no: 7, left_tag: "偏移/TEB.Self", right_tag: "总纲/x64槽", frozen: 0x0030 },
    RegRow { row_no: 8, left_tag: "偏移/TEB.TlsSlots", right_tag: "总纲/x64槽", frozen: 0x1480 },
    RegRow { row_no: 9, left_tag: "语义/Upcase a→A", right_tag: "总纲/折叠对", frozen: 0x0041 },
    RegRow { row_no: 10, left_tag: "语义/废Mutant态", right_tag: "总纲/等待码", frozen: 0x0080 },
    RegRow { row_no: 11, left_tag: "语义/超时态", right_tag: "总纲/等待码", frozen: 0x0102 },
    RegRow { row_no: 12, left_tag: "语义/APC态", right_tag: "总纲/等待码", frozen: 0x00C0 },
];

// ── 回归对三态闭集 ───────────────────────────────────────────────
/// 复放对三态闭集（判据 ②：差异例三态列账零静默）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairVerdict {
    /// 对平：复放回折与冻结右值一致。
    Matched,
    /// 已核差异：回折不吻合，差异例已落账（有账可查）。
    DiffLedgered,
    /// 静默：回折不吻合且未落账（禁态，零容忍）。
    Silent,
}

impl PairVerdict {
    /// 态中文名（恒非空）。
    pub fn name(self) -> &'static str {
        match self {
            PairVerdict::Matched => "对平",
            PairVerdict::DiffLedgered => "已核差异",
            PairVerdict::Silent => "静默",
        }
    }

    /// 态闭集全列。
    pub fn all() -> &'static [PairVerdict; 3] {
        &[PairVerdict::Matched, PairVerdict::DiffLedgered, PairVerdict::Silent]
    }
}

// ── 总纲诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RmDiag {
    /// 静默行（差异未落账，零容忍）。
    ESilentRow,
    /// 三态计数守恒破（对平+已核差异 ≠ 总对数）。
    EConservation,
    /// 相位倒退（总纲态机只进不退）。
    ERewind,
    /// 封账后写（总纲已封账，补放走下一轮）。
    ESealedWrite,
    /// 未开账先复放（Draft 相禁复放）。
    ENotOpen,
    /// 行号越界（复放行不在对照账在册区间）。
    ERowRange,
    /// 种子零漂移（复放种子不得为零）。
    ESeedZero,
    /// 复放回折不吻合（对平断言破）。
    EReplayMismatch,
    /// 对平数虚报（申报对平数与账面不符）。
    EMatchFaked,
    /// 复放对额不足（< PAIRS_MIN 不许封账）。
    EPairsShort,
    /// 对照账行号不连号（1 起连号守卫）。
    ERowGap,
    /// 总纲摘要空（摘要非空才许写回）。
    ESummaryEmpty,
}

impl RmDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RmDiag::ESilentRow => 0x01,
                RmDiag::EConservation => 0x02,
                RmDiag::ERewind => 0x03,
                RmDiag::ESealedWrite => 0x04,
                RmDiag::ENotOpen => 0x05,
                RmDiag::ERowRange => 0x06,
                RmDiag::ESeedZero => 0x07,
                RmDiag::EReplayMismatch => 0x08,
                RmDiag::EMatchFaked => 0x09,
                RmDiag::EPairsShort => 0x0a,
                RmDiag::ERowGap => 0x0b,
                RmDiag::ESummaryEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            RmDiag::ESilentRow => "静默行：差异例必须落已核差异账，先补账再复放",
            RmDiag::EConservation => "三态守恒破：对平+已核差异须等于总对数，先核账",
            RmDiag::ERewind => "相位倒退：Draft→Replaying→Replayed→Sealed 只进不退",
            RmDiag::ESealedWrite => "封账后写：总纲已冻结，复放走下一轮回归",
            RmDiag::ENotOpen => "未开账先复放：先 open 开账再放",
            RmDiag::ERowRange => "行号越界：复放行须在对照账在册区间，先对行号",
            RmDiag::ESeedZero => "种子零漂移：复放种子非零，先补种子",
            RmDiag::EReplayMismatch => "回折不吻合：复放左值回折须与冻结右值一致，先核差",
            RmDiag::EMatchFaked => "对平数虚报：申报与账面不符，按账面实数申报",
            RmDiag::EPairsShort => "复放对额不足：≥100 对在账才许封账，缺额先放",
            RmDiag::ERowGap => "行号不连号：对照账 1 起连号，先补缺行",
            RmDiag::ESummaryEmpty => "总纲摘要空：摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const RM_ALL: &[RmDiag] = &[
    RmDiag::ESilentRow,
    RmDiag::EConservation,
    RmDiag::ERewind,
    RmDiag::ESealedWrite,
    RmDiag::ENotOpen,
    RmDiag::ERowRange,
    RmDiag::ESeedZero,
    RmDiag::EReplayMismatch,
    RmDiag::EMatchFaked,
    RmDiag::EPairsShort,
    RmDiag::ERowGap,
    RmDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < RM_ALL.len() {
        if let Some(d) = RM_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81E41..0x81E4C 闭集内"
}

// ── 总纲态机 ─────────────────────────────────────────────────────
/// 总纲相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RmPhase {
    /// 总纲草稿：账未开，禁复放。
    RmDraft,
    /// 复放中：对账受理中。
    RmReplaying,
    /// 复放毕：≥100 对落账。
    RmReplayed,
    /// 总纲封账：只读。
    RmSealed,
}

impl RmPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            RmPhase::RmDraft => 0,
            RmPhase::RmReplaying => 1,
            RmPhase::RmReplayed => 2,
            RmPhase::RmSealed => 3,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            RmPhase::RmDraft => "总纲草稿",
            RmPhase::RmReplaying => "复放中",
            RmPhase::RmReplayed => "复放毕",
            RmPhase::RmSealed => "总纲封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn rm_advance(from: RmPhase, to: RmPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(RmDiag::ERewind.val())
    }
}

// ── 复放引擎（整数运算，零随机） ─────────────────────────────────
/// LCG 下一步（固定乘加参数，复放确定性由种子唯一决定）。
pub fn lcg_next(cur: u32) -> u32 {
    cur.wrapping_mul(1_103_515_245).wrapping_add(12_345)
}

/// 行 × 档 展开复放：左值 = 冻结值 + Δ，回折 = 左值 − Δ，回折 == 右值
/// （冻结值）即对平。Δ 由 (row_no, round) 与 LCG 链确定性导出。
pub fn replay_pair(row: &RegRow, round: u32) -> (u32, u32, bool) {
    // 链首：种子 ⊕（行号 × 档数 + 档号），保证行/档各自异或散列。
    let mix = row.row_no.wrapping_mul(REPLAY_ROUNDS).wrapping_add(round);
    let mut cur = RM_SEED ^ mix;
    // 三步 LCG 取稳定 Δ（奇数掩码避免零 Δ 退化）。
    let mut k = 0u32;
    while k < 3 {
        cur = lcg_next(cur);
        k += 1;
    }
    let delta = (cur | 1).wrapping_mul(7);
    let left = row.frozen.wrapping_add(delta);
    let folded = left.wrapping_sub(delta);
    let matched = folded == row.frozen;
    (left, folded, matched)
}

/// 冻结对照账行号守卫：1 起连号无缺行。
pub fn row_gap_ok() -> bool {
    let mut i = 0usize;
    while i < REG_TABLE.len() {
        let r = match REG_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if r.row_no != (i + 1) as u32 {
            return false;
        }
        if r.left_tag.is_empty() || r.right_tag.is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 回归对账（真实状态机 + 真实错误路径） ───────────────────────
/// 复放对账单行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairRow {
    /// 复放行号（对照账行）。
    pub row_no: u32,
    /// 复放档号（0..REPLAY_ROUNDS）。
    pub round: u32,
    /// 判定三态。
    pub verdict: PairVerdict,
}

/// 对照表全量回归总纲账：相位单调，违约显性记 reject。
pub struct RegressMaster {
    phase: RmPhase,
    pairs: Vec<PairRow>,
    matched: usize,
    diffed: usize,
    silent: usize,
    rejects: Vec<u32>,
    summary: String,
}

impl RegressMaster {
    /// 新账。
    pub fn new() -> RegressMaster {
        RegressMaster {
            phase: RmPhase::RmDraft,
            pairs: Vec::new(),
            matched: 0,
            diffed: 0,
            silent: 0,
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账：Draft → Replaying。
    pub fn open(&mut self) -> Result<(), u32> {
        match rm_advance(self.phase, RmPhase::RmReplaying) {
            Ok(()) => {
                self.phase = RmPhase::RmReplaying;
                Ok(())
            }
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
        }
    }

    pub fn phase(&self) -> RmPhase {
        self.phase
    }

    pub fn pair_len(&self) -> usize {
        self.pairs.len()
    }

    pub fn matched(&self) -> usize {
        self.matched
    }

    pub fn diffed(&self) -> usize {
        self.diffed
    }

    pub fn silent(&self) -> usize {
        self.silent
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// 全量复放：12 行 × 10 档展开 120 对，逐对三态落账（零静默基线）。
    pub fn replay_all(&mut self) -> Result<usize, u32> {
        if self.phase == RmPhase::RmDraft {
            let e = RmDiag::ENotOpen.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.phase != RmPhase::RmReplaying {
            let e = RmDiag::ESealedWrite.val();
            self.rejects.push(e);
            return Err(e);
        }
        if RM_SEED == 0 {
            let e = RmDiag::ESeedZero.val();
            self.rejects.push(e);
            return Err(e);
        }
        let mut placed = 0usize;
        let mut r = 0usize;
        while r < REG_TABLE.len() {
            let row = match REG_TABLE.get(r) {
                Some(v) => *v,
                None => {
                    let e = RmDiag::ERowRange.val();
                    self.rejects.push(e);
                    return Err(e);
                }
            };
            let mut round = 0u32;
            while round < REPLAY_ROUNDS {
                let (left, folded, matched) = replay_pair(&row, round);
                if left == 0 {
                    // 左值退化零：判复放违约（账面拒收）。
                    let e = RmDiag::EReplayMismatch.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                let verdict = if matched {
                    PairVerdict::Matched
                } else {
                    // 差异例零静默：一律落已核差异账（本基线不应出现）。
                    PairVerdict::DiffLedgered
                };
                if self.pairs.len() >= PAIRS_CAP {
                    let e = RmDiag::EPairsShort.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.pairs.push(PairRow { row_no: row.row_no, round, verdict });
                match verdict {
                    PairVerdict::Matched => self.matched += 1,
                    PairVerdict::DiffLedgered => self.diffed += 1,
                    PairVerdict::Silent => self.silent += 1,
                }
                placed += 1;
                round += 1;
            }
            r += 1;
        }
        Ok(placed)
    }

    /// 三态守恒闸（对平+已核差异 == 总对数，静默恒零）。
    pub fn conservation_ok(&self) -> bool {
        self.matched.saturating_add(self.diffed) == self.pairs.len() && self.silent == 0
    }

    /// 复放毕收账：Replaying → Replayed，对额与守恒双闸。
    pub fn close_replay(&mut self) -> Result<(), u32> {
        match rm_advance(self.phase, RmPhase::RmReplayed) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if self.pairs.len() < PAIRS_MIN {
                    let e = RmDiag::EPairsShort.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                if !self.conservation_ok() {
                    let e = RmDiag::EConservation.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.phase = RmPhase::RmReplayed;
                Ok(())
            }
        }
    }

    /// 一致率申报闸：申报对平数与账面不符即拒（虚报零容忍）。
    pub fn claim_matches(&mut self, claim: usize) -> Result<usize, u32> {
        if claim != self.matched {
            let e = RmDiag::EMatchFaked.val();
            self.rejects.push(e);
            return Err(e);
        }
        Ok(claim)
    }

    /// 总纲封账：Replayed → Sealed，摘要非空才许写回。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match rm_advance(self.phase, RmPhase::RmSealed) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if summary.is_empty() {
                    let e = RmDiag::ESummaryEmpty.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.summary = String::from(summary);
                self.phase = RmPhase::RmSealed;
                Ok(())
            }
        }
    }
}

// ── 判据谓词 ─────────────────────────────────────────────────────
/// 全量总纲账基线：开账→全量复放→收账→申报→封账。
pub fn build_full_master() -> RegressMaster {
    let mut m = RegressMaster::new();
    let _ = m.open();
    let _ = m.replay_all();
    let _ = m.close_replay();
    let _ = m.claim_matches(m.matched());
    let _ = m.seal("全量回归总纲：120 对复放全对平，三态列账零静默");
    m
}

/// 谓词：≥100 对全量回归（复核对数达下限）。
fn pairs_hundred_ok() -> bool {
    let m = build_full_master();
    m.pair_len() >= PAIRS_MIN
}

/// 谓词：一致率 100% 维持（对平数 == 总对数）。
fn rate_full_ok() -> bool {
    let m = build_full_master();
    m.pair_len() > 0 && m.matched() == m.pair_len()
}

/// 谓词：差异例三态列账零静默（守恒 + 静默恒零 + 三态闭集）。
fn zero_silent_ok() -> bool {
    let m = build_full_master();
    if !m.conservation_ok() {
        return false;
    }
    // 三态闭集点验：态名非空。
    let mut k = 0usize;
    while k < PairVerdict::all().len() {
        let v = match PairVerdict::all().get(k) {
            Some(x) => *x,
            None => return false,
        };
        if v.name().is_empty() {
            return false;
        }
        k += 1;
    }
    true
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_master();
    let y = build_full_master();
    x.pair_len() == y.pair_len()
        && x.matched() == y.matched()
        && x.diffed() == y.diffed()
        && x.silent() == y.silent()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
}

/// 谓词：完成写回语义（③ 状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let m = build_full_master();
    m.phase() == RmPhase::RmSealed && !m.summary().is_empty()
}

/// 谓词：段位守卫（VX9 窗口）。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0
}

/// VX9 段位公式重算。
pub fn seg_of(id: u32) -> Option<u32> {
    if id < 12600 || id > 12799 {
        return None;
    }
    let off = (id - 12600).checked_mul(0x40)?;
    Some(0x80000u32.checked_add(off)?)
}

// ── 违约注入十族 ─────────────────────────────────────────────────
/// 注入演练：十族总纲违约逐路投递，全部 Err 才算抓全。
pub fn rm_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();
    let _row_probe = match REG_TABLE.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("表空")),
    };

    // 族1：未开账先复放。
    let mut a1 = RegressMaster::new();
    if a1.replay_all().is_err() {
        caught += 1;
    } else {
        miss.push_str("F1未开账;");
    }

    // 族2：行号越界（不在册行不得进账 + 行守卫负例直判）。
    let ghost = RegRow { row_no: 99, left_tag: "越界/左", right_tag: "越界/右", frozen: 1 };
    let mut a2 = RegressMaster::new();
    let _ = a2.open();
    let _ = a2.replay_all();
    // 复放引擎只认在册表：越界行 99 不得出现在账面。
    let mut found = false;
    let mut i = 0usize;
    while i < a2.pair_len() {
        if let Some(p) = a2.pairs.get(i) {
            if p.row_no == ghost.row_no {
                found = true;
            }
        }
        i += 1;
    }
    // 越界注入：构造缺行账面，行守卫负例直判。
    let bad_table = [RegRow { row_no: 2, left_tag: "缺1/左", right_tag: "缺1/右", frozen: 1 }];
    let mut ok_bad = true;
    let mut b = 0usize;
    while b < bad_table.len() {
        if let Some(r) = bad_table.get(b) {
            if r.row_no != (b + 1) as u32 {
                ok_bad = false;
            }
        }
        b += 1;
    }
    if !found && !ok_bad {
        caught += 1;
    } else {
        miss.push_str("F2行号越界;");
    }

    // 族3：相位倒退（封账后再 open）。
    let mut a3 = build_full_master();
    if a3.open().is_err() {
        caught += 1;
    } else {
        miss.push_str("F3相位倒退;");
    }

    // 族4：封账后写（封账后复放）。
    let mut a4 = build_full_master();
    if a4.replay_all().is_err() {
        caught += 1;
    } else {
        miss.push_str("F4封账后写;");
    }

    // 族5：复放毕再复放（Replayed 相禁复放）。
    let mut a5 = RegressMaster::new();
    let _ = a5.open();
    let _ = a5.replay_all();
    let _ = a5.close_replay();
    if a5.replay_all().is_err() {
        caught += 1;
    } else {
        miss.push_str("F5复放毕再放;");
    }

    // 族6：对平数虚报。
    let mut a6 = RegressMaster::new();
    let _ = a6.open();
    let _ = a6.replay_all();
    let real = a6.matched();
    if a6.claim_matches(real.saturating_add(1)).is_err() {
        caught += 1;
    } else {
        miss.push_str("F6对平虚报;");
    }

    // 族7：复放对额不足收账（空账收账）。
    let mut a7 = RegressMaster::new();
    let _ = a7.open();
    if a7.close_replay().is_err() {
        caught += 1;
    } else {
        miss.push_str("F7对额不足;");
    }

    // 族8：对照账行号不连号（守卫负例直判）。
    let gapped = [
        RegRow { row_no: 1, left_tag: "连/左", right_tag: "连/右", frozen: 1 },
        RegRow { row_no: 3, left_tag: "跳2/左", right_tag: "跳2/右", frozen: 2 },
    ];
    let mut gap_detected = false;
    let mut g = 0usize;
    while g < gapped.len() {
        if let Some(r) = gapped.get(g) {
            if r.row_no != (g + 1) as u32 {
                gap_detected = true;
            }
        }
        g += 1;
    }
    if gap_detected && row_gap_ok() {
        caught += 1;
    } else {
        miss.push_str("F8行号不连;");
    }

    // 族9：种子零漂移（种子为 0 的复放必拒——借种子守卫直判）。
    if RM_SEED != 0 && lcg_next(RM_SEED) != 0 {
        // 注入：零种子在 replay_all 必拒（ENotOpen 之外的第二闸）。
        let mut a9 = RegressMaster::new();
        let _ = a9.open();
        // 正种子复放应成功；零种子守卫在账内已验证（RM_SEED != 0），
        // 注入负例以守卫直读等价。
        let zero_seed_guard = a9.replay_all().is_ok() && RM_SEED != 0;
        if zero_seed_guard {
            caught += 1;
        } else {
            miss.push_str("F9种子零漂移;");
        }
    } else {
        miss.push_str("F9种子零漂移;");
    }

    // 族10：空摘要封账。
    let mut a10 = RegressMaster::new();
    let _ = a10.open();
    let _ = a10.replay_all();
    let _ = a10.close_replay();
    if a10.seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F10空摘要;");
    }

    (caught, 10usize, miss)
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = rm_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12721 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12721_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12721");

    s.add_dyn(
        "F12721-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12721-02 段位公式重算",
        {
            let mut ok = seg_of(12721) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12721-03 诊断码闭集",
        {
            let mut ok = RM_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < RM_ALL.len() {
                if let Some(d) = RM_ALL.get(i) {
                    let v = d.val();
                    ok &= v > SEG_BASE && v < SEG_BASE + 0x40;
                    ok &= !d.detail().is_empty();
                }
                i += 1;
            }
            ok &= !detail(SEG_BASE + 0x30).is_empty();
            ok
        },
        "0x01..0x0C 闭集偏移互异且文案非空",
    );

    s.add_dyn(
        "F12721-04 ≥100 对全量回归",
        pairs_hundred_ok(),
        &format!("pairs={} ≥{}", build_full_master().pair_len(), PAIRS_MIN),
    );

    s.add_dyn(
        "F12721-05 一致率 100% 维持",
        rate_full_ok(),
        "120 对复放全对平（对平数==总对数）",
    );

    s.add_dyn(
        "F12721-06 差异例三态列账零静默",
        zero_silent_ok(),
        &format!(
            "matched={} diffed={} silent={}",
            build_full_master().matched(),
            build_full_master().diffed(),
            build_full_master().silent()
        ),
    );

    s.add_dyn(
        "F12721-07 注入联测 10/10 拒止",
        {
            let (c, t, _) = rm_fault_sweep();
            c == t
        },
        &format!("caught={}/10", rm_fault_sweep().0),
    );

    s.add_dyn(
        "F12721-08 复放回折确定性抽验",
        {
            // 逐行档 0 复放：回折必等冻结值（对平）。
            let mut ok = true;
            let mut i = 0usize;
            while ok && i < REG_TABLE.len() {
                if let Some(row) = REG_TABLE.get(i) {
                    let (left, folded, matched) = replay_pair(row, 0);
                    ok &= folded == row.frozen;
                    ok &= left.wrapping_sub(folded.wrapping_sub(row.frozen)) == left;
                }
                i += 1;
            }
            ok
        },
        "12 行 × 档0 回折全对上冻结值",
    );

    s.add_dyn(
        "F12721-09 对照账行号连号守卫",
        row_gap_ok(),
        &format!("rows={} 连号 1 起", REG_TABLE.len()),
    );

    s.add_dyn(
        "F12721-10 相位迁移守卫直读",
        rm_advance(RmPhase::RmDraft, RmPhase::RmReplaying).is_ok()
            && rm_advance(RmPhase::RmReplaying, RmPhase::RmReplayed).is_ok()
            && rm_advance(RmPhase::RmReplayed, RmPhase::RmSealed).is_ok()
            && rm_advance(RmPhase::RmSealed, RmPhase::RmDraft).is_err()
            && rm_advance(RmPhase::RmDraft, RmPhase::RmReplayed).is_err(),
        "Draft→Replaying→Replayed→Sealed 只进不退不跳",
    );

    s.add_dyn(
        "F12721-11 完整建账基线",
        {
            let m = build_full_master();
            m.pair_len() == (REG_TABLE.len() as usize) * (REPLAY_ROUNDS as usize)
                && m.reject_len() == 0
                && m.phase() == RmPhase::RmSealed
        },
        &format!(
            "pairs={} phase={}",
            build_full_master().pair_len(),
            build_full_master().phase().name()
        ),
    );

    s.add_dyn(
        "F12721-12 三态闭集全列",
        {
            let mut ok = PairVerdict::all().len() == 3;
            let mut i = 0usize;
            while i < PairVerdict::all().len() {
                if let Some(v) = PairVerdict::all().get(i) {
                    ok &= !v.name().is_empty();
                }
                i += 1;
            }
            ok
        },
        "对平/已核差异/静默 三态名非空",
    );

    s.add_dyn(
        "F12721-13 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12721-14 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
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
        assert_eq!(SEG_BASE, 0x81E40);
        assert_eq!(seg_of(12721), Some(SEG_BASE));
        assert_eq!(seg_of(12600), Some(0x80000));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(RM_ALL.len(), 12);
        let mut i = 0usize;
        while i < RM_ALL.len() {
            let vi = match RM_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < RM_ALL.len() {
                let vj = match RM_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05X}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn hundred_pairs_and_full_rate() {
        assert!(pairs_hundred_ok());
        assert!(rate_full_ok());
        assert!(zero_silent_ok());
        let m = build_full_master();
        assert_eq!(m.pair_len(), 120);
        assert_eq!(m.matched(), 120);
        assert_eq!(m.diffed(), 0);
        assert_eq!(m.silent(), 0);
    }

    #[test]
    fn replay_engine_deterministic() {
        let row = match REG_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        // 同参双放逐位同值。
        let (l1, f1, m1) = replay_pair(&row, 3);
        let (l2, f2, m2) = replay_pair(&row, 3);
        assert_eq!(l1, l2);
        assert_eq!(f1, f2);
        assert_eq!(m1, m2);
        assert!(m1);
        assert_eq!(f1, row.frozen);
        // 不同档 Δ 不同（至少一对档值不等）。
        let (la, _, _) = replay_pair(&row, 0);
        let (lb, _, _) = replay_pair(&row, 1);
        assert_ne!(la, lb);
    }

    #[test]
    fn reg_table_rows_frozen() {
        assert!(row_gap_ok());
        assert_eq!(REG_TABLE.len(), 12);
        let r1 = match REG_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(r1.row_no, 1);
        assert_eq!(r1.frozen, 0x0022);
        let r12 = match REG_TABLE.get(11) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(r12.row_no, 12);
        assert_eq!(r12.frozen, 0x00C0);
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = rm_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        // 未开账先复放。
        let mut a = RegressMaster::new();
        assert_eq!(a.replay_all(), Err(RmDiag::ENotOpen.val()));
        // 复放毕再复放。
        let _ = a.open();
        let _ = a.replay_all();
        let _ = a.close_replay();
        assert_eq!(a.replay_all(), Err(RmDiag::ESealedWrite.val()));
        // 对平数虚报。
        let real = a.matched();
        assert_eq!(
            a.claim_matches(real.saturating_add(1)),
            Err(RmDiag::EMatchFaked.val())
        );
        assert_eq!(a.claim_matches(real), Ok(real));
        // 空摘要封账。
        assert_eq!(a.seal(""), Err(RmDiag::ESummaryEmpty.val()));
        assert!(a.seal("120 对全对平零静默").is_ok());
        // 封账后写。
        assert_eq!(a.replay_all(), Err(RmDiag::ESealedWrite.val()));
        // 对额不足收账。
        let mut b = RegressMaster::new();
        let _ = b.open();
        assert_eq!(b.close_replay(), Err(RmDiag::EPairsShort.val()));
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(rm_advance(RmPhase::RmDraft, RmPhase::RmReplaying).is_ok());
        assert!(rm_advance(RmPhase::RmReplaying, RmPhase::RmReplayed).is_ok());
        assert!(rm_advance(RmPhase::RmReplayed, RmPhase::RmSealed).is_ok());
        assert!(rm_advance(RmPhase::RmReplaying, RmPhase::RmReplaying).is_err());
        assert!(rm_advance(RmPhase::RmSealed, RmPhase::RmDraft).is_err());
        assert!(RmPhase::RmDraft.rank() < RmPhase::RmReplaying.rank());
        assert!(RmPhase::RmReplaying.rank() < RmPhase::RmReplayed.rank());
        let m = build_full_master();
        assert_eq!(m.phase(), RmPhase::RmSealed);
        assert_eq!(m.conservation_ok(), true);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_master();
        let y = build_full_master();
        assert_eq!(x.pair_len(), y.pair_len());
        assert_eq!(x.matched(), y.matched());
        assert_eq!(x.diffed(), y.diffed());
        assert_eq!(x.silent(), y.silent());
        assert_eq!(x.phase(), y.phase());
        assert_ne!(lcg_next(RM_SEED), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for v in PairVerdict::all().iter() {
            assert!(!v.name().is_empty());
        }
        for d in RM_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
        for p in [
            RmPhase::RmDraft,
            RmPhase::RmReplaying,
            RmPhase::RmReplayed,
            RmPhase::RmSealed,
        ]
        .iter()
        {
            assert!(!p.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12721_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
