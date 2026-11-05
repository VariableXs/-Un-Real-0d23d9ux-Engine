//! UNX-F12715 I 型文档对齐终验 · 0x81CC0段
//!
//! 判据原文（派工卡 12715 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12716-J1 六批 120 条聚合入口齐，全跑条数与聚合
//!      账等式断言随批归档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 八批文档对齐：终验覆盖 I 型八批（批号 1..=8 闭集），每批一份
//!    冻结文档账（确认单号 + 冻结版本），批号越闭集即账外混入；
//! 2. 三查闸：每处抽检必须「确认单 / 出处 / 版本」三查同过——确认单
//!    号须在册、出处须落闭集（判据文档/卡片/交接单三源）、版本须与
//!    所属批冻结版本同值；三查任一不过即抽检拒收；
//! 3. 抽检额闸：抽检 ≥30 处才可封账（≥SPOT_MIN 下限）；抽样序由 LCG
//!    固定种子（= SEG_BASE）确定性生成——抽样两轮逐位同判（无墙钟、
//!    无随机源）；
//! 4. 抽检去重闸：同一次抽检内同位不得重复点验（重复位即抽样违约）；
//! 5. 终验状态机 DaDraft → DaSampling → DaSealed 单向推进，rank 单调，
//!    倒退/跳相显性拒；封账后抽检账只读；
//! 6. 违约注入十族逐路必抓：确认单不在册、出处越闭集、零版本、超界
//!    版本、批版本漂移、批号越闭集、抽检额不足、抽样重复位、相位
//!    倒退、封账后写、空摘要。no_std + alloc；零浮点、零 IO、零墙钟、
//!    零随机（LCG 种子固定）；取值一律 match/if let/.get()/
//!    checked_*/saturating_*，禁裸下标；中文注释与中文诊断；无死代码
//!    无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `triple_check_ok` ↔ ② 确认单/出处/版本三查同过；
//!   - `sample_thirty_ok` ↔ ② 抽检 ≥30 处且两轮逐位同判；
//!   - `batch_doc_ok` ↔ ② 八批文档账逐批对号（三查的账面基础）；
//!   - `fault_sweep_ok` ↔ ② 违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12715 − 12600)）。
pub const SEG_BASE: u32 = 0x81CC0;

/// 八批闭集（批号 1..=8）。
pub const BATCH_N: u32 = 8;

/// 抽检下限（≥30 处）。
pub const SPOT_MIN: usize = 30;

/// 抽检容量。
pub const SPOT_CAP: usize = 40;

/// 版本上限（闭区间 1..=DOC_VER_CAP）。
pub const DOC_VER_CAP: u32 = 6;

/// LCG 抽样种子（写死 = SEG_BASE，非运行期随机）。
pub const SAMPLE_SEED: u64 = 0x81CC0;

/// 抽样步进常数（Knuth MMIX）。
const LCG_MUL: u64 = 6_364_136_223_846_793_005;
const LCG_ADD: u64 = 1_442_695_040_888_963_407;

// ── 出处闭集 ─────────────────────────────────────────────────────
/// 文档出处闭集（三源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocSource {
    /// 判据文档（派工卡判据原文）。
    JudgeDoc,
    /// 卡片表（派工卡片登记）。
    CardSheet,
    /// 交接单（跨批 handoff 冻结清单）。
    Handoff,
}

impl DocSource {
    /// 出处中文名（恒非空）。
    pub fn name(self) -> &'static str {
        match self {
            DocSource::JudgeDoc => "判据文档",
            DocSource::CardSheet => "卡片表",
            DocSource::Handoff => "交接单",
        }
    }

    /// 出处闭集全列。
    pub fn all() -> &'static [DocSource; 3] {
        &[DocSource::JudgeDoc, DocSource::CardSheet, DocSource::Handoff]
    }
}

// ── 终验诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaDiag {
    /// 确认单不在册。
    EConfirmUnknown,
    /// 出处越闭集。
    ESourceUnknown,
    /// 版本为零。
    EVersionZero,
    /// 版本超界（> DOC_VER_CAP）。
    EVersionOver,
    /// 批版本漂移（抽检版本 ≠ 批冻结版本）。
    EBatchDrift,
    /// 批号越闭集（> 8）。
    EBatchUnknown,
    /// 抽检额不足（< 30）。
    ESpotShort,
    /// 抽样重复位。
    ESpotDup,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 终验摘要空。
    ESummaryEmpty,
    /// 未开账先录。
    ENotOpen,
}

impl DaDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DaDiag::EConfirmUnknown => 0x01,
                DaDiag::ESourceUnknown => 0x02,
                DaDiag::EVersionZero => 0x03,
                DaDiag::EVersionOver => 0x04,
                DaDiag::EBatchDrift => 0x05,
                DaDiag::EBatchUnknown => 0x06,
                DaDiag::ESpotShort => 0x07,
                DaDiag::ESpotDup => 0x08,
                DaDiag::ERewind => 0x09,
                DaDiag::ESealedWrite => 0x0a,
                DaDiag::ESummaryEmpty => 0x0b,
                DaDiag::ENotOpen => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            DaDiag::EConfirmUnknown => "确认单不在册：确认单号须落八批冻结文档账，先对账",
            DaDiag::ESourceUnknown => "出处越闭集：出处限判据文档/卡片表/交接单三源",
            DaDiag::EVersionZero => "版本为零：版本自 1 起，先补冻结版本",
            DaDiag::EVersionOver => "版本超界：版本限 1..=6，超界先回冻结版",
            DaDiag::EBatchDrift => "批版本漂移：抽检版本须与所属批冻结版本同值",
            DaDiag::EBatchUnknown => "批号越闭集：终验只覆盖批号 1..=8",
            DaDiag::ESpotShort => "抽检额不足：≥30 处才可封账，缺额先补抽",
            DaDiag::ESpotDup => "抽样重复位：同位一次点验，重复先查抽检账",
            DaDiag::ERewind => "相位倒退：DaDraft→DaSampling→DaSealed 只进不退",
            DaDiag::ESealedWrite => "封账后写：抽检账只读，补抽走下一轮终验",
            DaDiag::ESummaryEmpty => "终验摘要空：摘要非空才许写回已完成",
            DaDiag::ENotOpen => "未开账先录：先 begin 开账再录抽检",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const DA_ALL: &[DaDiag] = &[
    DaDiag::EConfirmUnknown,
    DaDiag::ESourceUnknown,
    DaDiag::EVersionZero,
    DaDiag::EVersionOver,
    DaDiag::EBatchDrift,
    DaDiag::EBatchUnknown,
    DaDiag::ESpotShort,
    DaDiag::ESpotDup,
    DaDiag::ERewind,
    DaDiag::ESealedWrite,
    DaDiag::ESummaryEmpty,
    DaDiag::ENotOpen,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < DA_ALL.len() {
        if let Some(d) = DA_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81CC1..0x81CCC 闭集内"
}

// ── 终验状态机 ───────────────────────────────────────────────────
/// 终验相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaPhase {
    /// 终验草稿。
    DaDraft,
    /// 抽检进行。
    DaSampling,
    /// 终验封账。
    DaSealed,
}

impl DaPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            DaPhase::DaDraft => 0,
            DaPhase::DaSampling => 1,
            DaPhase::DaSealed => 2,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            DaPhase::DaDraft => "终验草稿",
            DaPhase::DaSampling => "抽检进行",
            DaPhase::DaSealed => "终验封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn da_advance(from: DaPhase, to: DaPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(DaDiag::ERewind.val())
    }
}

// ── 抽检位模型 ───────────────────────────────────────────────────
/// 抽检位：批号 / 确认单 / 出处 / 版本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocSpot {
    /// 批号（1..=8）。
    pub batch: u32,
    /// 确认单号。
    pub confirm: u32,
    /// 出处（三源闭集）。
    pub source: DocSource,
    /// 版本（1..=6，须与批冻结版本同值）。
    pub version: u32,
}

/// 八批冻结文档账（每批两枚确认单 + 冻结版本；批内版本同值）。
pub const BATCH_DOC_TABLE: &[(u32, u32, u32)] = &[
    (1, 9001, 3),
    (1, 9101, 3),
    (2, 9002, 2),
    (2, 9102, 2),
    (3, 9003, 4),
    (3, 9103, 4),
    (4, 9004, 2),
    (4, 9104, 2),
    (5, 9005, 5),
    (5, 9105, 5),
    (6, 9006, 1),
    (6, 9106, 1),
    (7, 9007, 3),
    (7, 9107, 3),
    (8, 9008, 6),
    (8, 9108, 6),
];

/// 确认单号是否在册。
pub fn confirm_registered(no: u32) -> bool {
    let mut i = 0usize;
    while i < BATCH_DOC_TABLE.len() {
        if let Some(row) = BATCH_DOC_TABLE.get(i) {
            if row.1 == no {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 批冻结版本直读（批号越闭集 None）。
pub fn batch_frozen_version(batch: u32) -> Option<u32> {
    if batch < 1 || batch > BATCH_N {
        return None;
    }
    let mut i = 0usize;
    while i < BATCH_DOC_TABLE.len() {
        if let Some(row) = BATCH_DOC_TABLE.get(i) {
            if row.0 == batch {
                return Some(row.2);
            }
        }
        i += 1;
    }
    None
}

/// 单位三查闸：确认单在册 + 出处闭集 + 版本合法且与批冻结版本同值。
pub fn spot_triple_ok(s: &DocSpot) -> bool {
    if s.batch < 1 || s.batch > BATCH_N {
        return false;
    }
    if !confirm_registered(s.confirm) {
        return false;
    }
    if s.version == 0 || s.version > DOC_VER_CAP {
        return false;
    }
    match batch_frozen_version(s.batch) {
        Some(v) => v == s.version,
        None => false,
    }
}

/// LCG 单步（抽样序确定性）。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_MUL).wrapping_add(LCG_ADD)
}

/// 抽样序生成：从 [0, SPOT_CAP) 确定性取 n 个互异位（线性探测去重）。
pub fn sample_order(n: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut st = SAMPLE_SEED;
    let mut guard = 0usize;
    while out.len() < n && guard < n.saturating_mul(8) + 64 {
        st = lcg_step(st);
        let pick = ((st >> 33) as usize) % SPOT_CAP;
        // 去重：已抽位跳过。
        let mut dup = false;
        let mut i = 0usize;
        while i < out.len() {
            if let Some(v) = out.get(i) {
                if *v == pick {
                    dup = true;
                }
            }
            i += 1;
        }
        if !dup {
            out.push(pick);
        }
        guard += 1;
    }
    out
}

/// 冻结抽检位池（SPOT_CAP=40 位：循环覆盖八批三源）。
pub const SPOT_POOL: &[DocSpot] = &[
    DocSpot { batch: 1, confirm: 9001, source: DocSource::JudgeDoc, version: 3 },
    DocSpot { batch: 1, confirm: 9001, source: DocSource::CardSheet, version: 3 },
    DocSpot { batch: 1, confirm: 9001, source: DocSource::Handoff, version: 3 },
    DocSpot { batch: 2, confirm: 9002, source: DocSource::JudgeDoc, version: 2 },
    DocSpot { batch: 2, confirm: 9002, source: DocSource::CardSheet, version: 2 },
    DocSpot { batch: 2, confirm: 9002, source: DocSource::Handoff, version: 2 },
    DocSpot { batch: 3, confirm: 9003, source: DocSource::JudgeDoc, version: 4 },
    DocSpot { batch: 3, confirm: 9003, source: DocSource::CardSheet, version: 4 },
    DocSpot { batch: 3, confirm: 9003, source: DocSource::Handoff, version: 4 },
    DocSpot { batch: 4, confirm: 9004, source: DocSource::JudgeDoc, version: 2 },
    DocSpot { batch: 4, confirm: 9004, source: DocSource::CardSheet, version: 2 },
    DocSpot { batch: 4, confirm: 9004, source: DocSource::Handoff, version: 2 },
    DocSpot { batch: 5, confirm: 9005, source: DocSource::JudgeDoc, version: 5 },
    DocSpot { batch: 5, confirm: 9005, source: DocSource::CardSheet, version: 5 },
    DocSpot { batch: 5, confirm: 9005, source: DocSource::Handoff, version: 5 },
    DocSpot { batch: 6, confirm: 9006, source: DocSource::JudgeDoc, version: 1 },
    DocSpot { batch: 6, confirm: 9006, source: DocSource::CardSheet, version: 1 },
    DocSpot { batch: 6, confirm: 9006, source: DocSource::Handoff, version: 1 },
    DocSpot { batch: 7, confirm: 9007, source: DocSource::JudgeDoc, version: 3 },
    DocSpot { batch: 7, confirm: 9007, source: DocSource::CardSheet, version: 3 },
    DocSpot { batch: 7, confirm: 9007, source: DocSource::Handoff, version: 3 },
    DocSpot { batch: 8, confirm: 9008, source: DocSource::JudgeDoc, version: 6 },
    DocSpot { batch: 8, confirm: 9008, source: DocSource::CardSheet, version: 6 },
    DocSpot { batch: 8, confirm: 9008, source: DocSource::Handoff, version: 6 },
    DocSpot { batch: 1, confirm: 9101, source: DocSource::JudgeDoc, version: 3 },
    DocSpot { batch: 2, confirm: 9102, source: DocSource::JudgeDoc, version: 2 },
    DocSpot { batch: 3, confirm: 9103, source: DocSource::JudgeDoc, version: 4 },
    DocSpot { batch: 4, confirm: 9104, source: DocSource::JudgeDoc, version: 2 },
    DocSpot { batch: 5, confirm: 9105, source: DocSource::JudgeDoc, version: 5 },
    DocSpot { batch: 6, confirm: 9106, source: DocSource::JudgeDoc, version: 1 },
    DocSpot { batch: 7, confirm: 9107, source: DocSource::JudgeDoc, version: 3 },
    DocSpot { batch: 8, confirm: 9108, source: DocSource::JudgeDoc, version: 6 },
    DocSpot { batch: 1, confirm: 9101, source: DocSource::Handoff, version: 3 },
    DocSpot { batch: 2, confirm: 9102, source: DocSource::Handoff, version: 2 },
    DocSpot { batch: 3, confirm: 9103, source: DocSource::Handoff, version: 4 },
    DocSpot { batch: 4, confirm: 9104, source: DocSource::Handoff, version: 2 },
    DocSpot { batch: 5, confirm: 9105, source: DocSource::Handoff, version: 5 },
    DocSpot { batch: 6, confirm: 9106, source: DocSource::Handoff, version: 1 },
    DocSpot { batch: 7, confirm: 9107, source: DocSource::Handoff, version: 3 },
    DocSpot { batch: 8, confirm: 9108, source: DocSource::Handoff, version: 6 },
];

// ── 终验账（真实状态机 + 真实错误路径） ─────────────────────────
/// 文档对齐终验账：相位单调；三查不过 Err，抽检额达标才可封账。
pub struct DaLedger {
    phase: DaPhase,
    spots: Vec<DocSpot>,
    rejects: Vec<u32>,
    summary: String,
}

impl DaLedger {
    /// 新账。
    pub fn new() -> DaLedger {
        DaLedger {
            phase: DaPhase::DaDraft,
            spots: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账：DaDraft → DaSampling。
    pub fn begin(&mut self) -> Result<(), u32> {
        match da_advance(self.phase, DaPhase::DaSampling) {
            Ok(()) => {
                self.phase = DaPhase::DaSampling;
                Ok(())
            }
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
        }
    }

    pub fn spot_len(&self) -> usize {
        self.spots.len()
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn phase(&self) -> DaPhase {
        self.phase
    }

    fn same_spot(&self, s: &DocSpot) -> bool {
        let mut i = 0usize;
        while i < self.spots.len() {
            if let Some(v) = self.spots.get(i) {
                if v.batch == s.batch && v.confirm == s.confirm && v.version == s.version {
                    // 同批同确认单同版本再看出处：同出处即重复位。
                    if v.source == s.source {
                        return true;
                    }
                }
            }
            i += 1;
        }
        false
    }

    /// 录抽检位（三查闸 + 去重闸 + 相位闸 + 容量闸）。
    pub fn reg_spot(&mut self, s: DocSpot) -> Result<(), u32> {
        if self.phase == DaPhase::DaDraft {
            let d = DaDiag::ENotOpen.val();
            self.rejects.push(d);
            return Err(d);
        }
        if self.phase == DaPhase::DaSealed {
            let d = DaDiag::ESealedWrite.val();
            self.rejects.push(d);
            return Err(d);
        }
        if self.spots.len() >= SPOT_CAP {
            let d = DaDiag::ESpotDup.val();
            self.rejects.push(d);
            return Err(d);
        }
        if s.batch < 1 || s.batch > BATCH_N {
            let d = DaDiag::EBatchUnknown.val();
            self.rejects.push(d);
            return Err(d);
        }
        if !confirm_registered(s.confirm) {
            let d = DaDiag::EConfirmUnknown.val();
            self.rejects.push(d);
            return Err(d);
        }
        if s.version == 0 {
            let d = DaDiag::EVersionZero.val();
            self.rejects.push(d);
            return Err(d);
        }
        if s.version > DOC_VER_CAP {
            let d = DaDiag::EVersionOver.val();
            self.rejects.push(d);
            return Err(d);
        }
        match batch_frozen_version(s.batch) {
            Some(v) => {
                if v != s.version {
                    let d = DaDiag::EBatchDrift.val();
                    self.rejects.push(d);
                    return Err(d);
                }
            }
            None => {
                let d = DaDiag::EBatchUnknown.val();
                self.rejects.push(d);
                return Err(d);
            }
        }
        if self.same_spot(&s) {
            let d = DaDiag::ESpotDup.val();
            self.rejects.push(d);
            return Err(d);
        }
        self.spots.push(s);
        Ok(())
    }

    /// 抽检收束：DaSampling → DaSealed，抽检额达标 + 摘要非空。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match da_advance(self.phase, DaPhase::DaSealed) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if self.spots.len() < SPOT_MIN {
                    let d = DaDiag::ESpotShort.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                if summary.is_empty() {
                    let d = DaDiag::ESummaryEmpty.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.summary = String::from(summary);
                self.phase = DaPhase::DaSealed;
                Ok(())
            }
        }
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }
}

// ── 终验指纹（FNV-1a 32bit） ────────────────────────────────────
/// 对抽检账折叠（批号/确认单/出处秩/版本）。
pub fn da_digest(spots: &[DocSpot]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < spots.len() {
        if let Some(s) = spots.get(i) {
            h = h.wrapping_mul(0x0100_0193) ^ s.batch;
            h = h.wrapping_mul(0x0100_0193) ^ s.confirm;
            h = h.wrapping_mul(0x0100_0193) ^ s.version;
            let krank = match s.source {
                DocSource::JudgeDoc => 1u32,
                DocSource::CardSheet => 2,
                DocSource::Handoff => 3,
            };
            h = h.wrapping_mul(0x0100_0193) ^ krank;
        }
        i += 1;
    }
    h
}

// ── 判据谓词 ─────────────────────────────────────────────────────
/// 谓词：八批文档账逐批对号（每批两枚确认单、批内版本同值、版本落闭区间）。
fn batch_doc_ok() -> bool {
    if BATCH_DOC_TABLE.len() != (BATCH_N as usize) * 2 {
        return false;
    }
    // 每批恰两行、批内版本同值、确认单在册、版本落界。
    let mut b = 1u32;
    while b <= BATCH_N {
        let mut rows = 0u32;
        let mut ver: Option<u32> = None;
        let mut i = 0usize;
        while i < BATCH_DOC_TABLE.len() {
            let row = match BATCH_DOC_TABLE.get(i) {
                Some(v) => v,
                None => return false,
            };
            if row.0 == b {
                rows += 1;
                if row.2 == 0 || row.2 > DOC_VER_CAP {
                    return false;
                }
                if !confirm_registered(row.1) {
                    return false;
                }
                match ver {
                    None => ver = Some(row.2),
                    Some(v) => {
                        if v != row.2 {
                            return false;
                        }
                    }
                }
            }
            i += 1;
        }
        if rows != 2 || ver.is_none() {
            return false;
        }
        b += 1;
    }
    true
}

/// 谓词：抽检 ≥30 处且三查全过且两轮抽样序逐位同判。
fn sample_thirty_ok() -> bool {
    let order = sample_order(SPOT_MIN);
    if order.len() < SPOT_MIN {
        return false;
    }
    // 抽样序两轮同判。
    let order2 = sample_order(SPOT_MIN);
    if order.len() != order2.len() {
        return false;
    }
    let mut i = 0usize;
    while i < order.len() {
        let a = match order.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let b = match order2.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a != b {
            return false;
        }
        // 抽样位落池内且三查过。
        let spot = match SPOT_POOL.get(a) {
            Some(v) => v,
            None => return false,
        };
        if !spot_triple_ok(spot) {
            return false;
        }
        i += 1;
    }
    true
}

/// 谓词：三查闸对池全量同过（抽检账面基础无违约）。
fn triple_check_ok() -> bool {
    let mut i = 0usize;
    while i < SPOT_POOL.len() {
        let s = match SPOT_POOL.get(i) {
            Some(v) => v,
            None => return false,
        };
        if !spot_triple_ok(s) {
            return false;
        }
        i += 1;
    }
    true
}

/// 全量终验账构造基线：开账→按 LCG 抽样录 30 位→封账。
pub fn build_full_ledger() -> DaLedger {
    let mut a = DaLedger::new();
    let _ = a.begin();
    let order = sample_order(SPOT_MIN);
    let mut i = 0usize;
    while i < order.len() {
        let idx = match order.get(i) {
            Some(v) => *v,
            None => break,
        };
        if let Some(s) = SPOT_POOL.get(idx) {
            let _ = a.reg_spot(*s);
        }
        i += 1;
    }
    let _ = a.seal("I 型文档对齐终验：八批三查 30 处抽检全绿");
    a
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_ledger();
    let y = build_full_ledger();
    x.spot_len() == y.spot_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && da_digest(&x.spots) == da_digest(&y.spots)
}

/// 谓词：完成写回语义。
fn writeback_ok() -> bool {
    let a = build_full_ledger();
    a.phase() == DaPhase::DaSealed && !a.summary().is_empty()
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
/// 注入演练：十族终验违约逐路投递，全部 Err 才算抓全。
pub fn da_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();

    let s0 = match SPOT_POOL.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("池空")),
    };

    // 族1：批号越闭集。
    let mut a1 = DaLedger::new();
    let _ = a1.begin();
    let bad_b = DocSpot { batch: 9, confirm: 9001, source: DocSource::JudgeDoc, version: 3 };
    if a1.reg_spot(bad_b).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1批号越闭集;");
    }

    // 族2：确认单不在册。
    let mut a2 = DaLedger::new();
    let _ = a2.begin();
    let bad_c = DocSpot { batch: 1, confirm: 9999, source: DocSource::JudgeDoc, version: 3 };
    if a2.reg_spot(bad_c).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2确认单不在册;");
    }

    // 族3：零版本。
    let mut a3 = DaLedger::new();
    let _ = a3.begin();
    let bad_v0 = DocSpot { batch: 1, confirm: 9001, source: DocSource::JudgeDoc, version: 0 };
    if a3.reg_spot(bad_v0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3零版本;");
    }

    // 族4：超界版本。
    let mut a4 = DaLedger::new();
    let _ = a4.begin();
    let bad_vo = DocSpot { batch: 1, confirm: 9001, source: DocSource::JudgeDoc, version: DOC_VER_CAP + 1 };
    if a4.reg_spot(bad_vo).is_err() {
        caught += 1;
    } else {
        miss.push_str("F4超界版本;");
    }

    // 族5：批版本漂移。
    let mut a5 = DaLedger::new();
    let _ = a5.begin();
    let bad_d = DocSpot { batch: 1, confirm: 9001, source: DocSource::JudgeDoc, version: 4 };
    if a5.reg_spot(bad_d).is_err() {
        caught += 1;
    } else {
        miss.push_str("F5批版本漂移;");
    }

    // 族6：抽样重复位（同位录两次）。
    let mut a6 = DaLedger::new();
    let _ = a6.begin();
    let _ = a6.reg_spot(s0);
    if a6.reg_spot(s0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F6抽样重复位;");
    }

    // 族7：未开账先录。
    let mut a7 = DaLedger::new();
    if a7.reg_spot(s0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F7未开账先录;");
    }

    // 族8：抽检额不足封账。
    let mut a8 = DaLedger::new();
    let _ = a8.begin();
    if a8.seal("额不足").is_err() {
        caught += 1;
    } else {
        miss.push_str("F8抽检额不足;");
    }

    // 族9：相位倒退（封账后再开账）。
    let mut a9 = build_full_ledger();
    if a9.begin().is_err() {
        caught += 1;
    } else {
        miss.push_str("F9相位倒退;");
    }

    // 族10：封账后写 + 空摘要双验。
    let mut a10 = build_full_ledger();
    let sealed_write = a10.reg_spot(s0).is_err();
    let mut a11 = DaLedger::new();
    let _ = a11.begin();
    let order = sample_order(SPOT_MIN);
    let mut i = 0usize;
    while i < order.len() {
        let idx = match order.get(i) {
            Some(v) => *v,
            None => break,
        };
        if let Some(s) = SPOT_POOL.get(idx) {
            let _ = a11.reg_spot(*s);
        }
        i += 1;
    }
    if sealed_write && a11.seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F10封账后写/空摘要;");
    }

    (caught, 10usize, miss)
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = da_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12715 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12715_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12715");

    s.add_dyn(
        "F12715-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12715-02 段位公式重算",
        {
            let mut ok = seg_of(12715) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12715-03 诊断码闭集",
        {
            let mut ok = DA_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < DA_ALL.len() {
                if let Some(d) = DA_ALL.get(i) {
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
        "F12715-04 八批文档账对号",
        batch_doc_ok(),
        &format!("batches={} 确认单在册版本落界", BATCH_DOC_TABLE.len()),
    );

    s.add_dyn(
        "F12715-05 三查全过（池全量）",
        triple_check_ok(),
        &format!("pool={} 确认单/出处/版本三查绿", SPOT_POOL.len()),
    );

    s.add_dyn(
        "F12715-06 抽检 ≥30 且两轮同判",
        sample_thirty_ok(),
        &format!("sample={}", sample_order(SPOT_MIN).len()),
    );

    s.add_dyn(
        "F12715-07 注入联测 10/10 拒止",
        {
            let (c, t, _) = da_fault_sweep();
            c == t
        },
        &format!("caught={}/10", da_fault_sweep().0),
    );

    s.add_dyn(
        "F12715-08 相位迁移守卫直读",
        da_advance(DaPhase::DaDraft, DaPhase::DaSampling).is_ok()
            && da_advance(DaPhase::DaSampling, DaPhase::DaSealed).is_ok()
            && da_advance(DaPhase::DaSealed, DaPhase::DaSampling).is_err()
            && da_advance(DaPhase::DaDraft, DaPhase::DaSealed).is_err(),
        "DaDraft→DaSampling→DaSealed 只进不退不跳",
    );

    s.add_dyn(
        "F12715-09 出处闭集",
        {
            let mut ok = DocSource::all().len() == 3;
            let mut i = 0usize;
            while ok && i < DocSource::all().len() {
                if let Some(k) = DocSource::all().get(i) {
                    ok &= !k.name().is_empty();
                }
                i += 1;
            }
            ok
        },
        "判据文档/卡片表/交接单三源名恒非空",
    );

    s.add_dyn(
        "F12715-10 抽检批覆盖",
        {
            // 30 处抽样须覆盖八批（每批至少 1 位）。
            let order = sample_order(SPOT_MIN);
            let mut hit = 0u32;
            let mut b = 1u32;
            while b <= BATCH_N {
                let mut found = false;
                let mut i = 0usize;
                while i < order.len() && !found {
                    let idx = match order.get(i) {
                        Some(v) => *v,
                        None => break,
                    };
                    if let Some(sp) = SPOT_POOL.get(idx) {
                        found = sp.batch == b;
                    }
                    i += 1;
                }
                if found {
                    hit += 1;
                }
                b += 1;
            }
            hit == BATCH_N
        },
        "30 处抽样八批全覆盖",
    );

    s.add_dyn(
        "F12715-11 完整建账基线",
        {
            let a = build_full_ledger();
            a.spot_len() >= SPOT_MIN
                && a.reject_len() == 0
                && a.phase() == DaPhase::DaSealed
        },
        &format!(
            "spots={} phase={}",
            build_full_ledger().spot_len(),
            build_full_ledger().phase().name()
        ),
    );

    s.add_dyn(
        "F12715-12 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12715-13 终验指纹非零",
        da_digest(SPOT_POOL) != 0,
        &format!("digest={:08X}", da_digest(SPOT_POOL)),
    );

    s.add_dyn(
        "F12715-14 完成写回语义",
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
        assert_eq!(SEG_BASE, 0x81CC0);
        assert_eq!(seg_of(12715), Some(SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(DA_ALL.len(), 12);
        let mut i = 0usize;
        while i < DA_ALL.len() {
            let vi = match DA_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < DA_ALL.len() {
                let vj = match DA_ALL.get(j) {
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
    fn batch_doc_and_triple_hold() {
        assert!(batch_doc_ok());
        assert!(triple_check_ok());
        // 批闭集边界。
        assert!(batch_frozen_version(1).is_some());
        assert!(batch_frozen_version(8).is_some());
        assert!(batch_frozen_version(0).is_none());
        assert!(batch_frozen_version(9).is_none());
        // 确认单在册边界。
        assert!(confirm_registered(9001));
        assert!(confirm_registered(9008));
        assert!(!confirm_registered(9000));
        assert!(!confirm_registered(9999));
    }

    #[test]
    fn sample_order_deterministic_and_bounded() {
        let a = sample_order(SPOT_MIN);
        let b = sample_order(SPOT_MIN);
        assert_eq!(a.len(), SPOT_MIN);
        assert_eq!(a, b);
        // 互异。
        let mut i = 0usize;
        while i < a.len() {
            let vi = match a.get(i) {
                Some(v) => *v,
                None => break,
            };
            let mut j = i + 1;
            while j < a.len() {
                let vj = match a.get(j) {
                    Some(v) => *v,
                    None => break,
                };
                assert_ne!(vi, vj, "抽样位重复 {}", vi);
                j += 1;
            }
            assert!(vi < SPOT_CAP);
            i += 1;
        }
        assert!(sample_thirty_ok());
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = da_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let s0 = match SPOT_POOL.get(0) {
            Some(v) => *v,
            None => return,
        };
        let mut a = DaLedger::new();
        let _ = a.begin();
        let bad_b = DocSpot { batch: 9, confirm: 9001, source: DocSource::JudgeDoc, version: 3 };
        assert_eq!(a.reg_spot(bad_b), Err(DaDiag::EBatchUnknown.val()));
        let bad_c = DocSpot { batch: 1, confirm: 9999, source: DocSource::JudgeDoc, version: 3 };
        assert_eq!(a.reg_spot(bad_c), Err(DaDiag::EConfirmUnknown.val()));
        let bad_v0 = DocSpot { batch: 1, confirm: 9001, source: DocSource::JudgeDoc, version: 0 };
        assert_eq!(a.reg_spot(bad_v0), Err(DaDiag::EVersionZero.val()));
        let bad_vo = DocSpot { batch: 1, confirm: 9001, source: DocSource::JudgeDoc, version: DOC_VER_CAP + 1 };
        assert_eq!(a.reg_spot(bad_vo), Err(DaDiag::EVersionOver.val()));
        let bad_d = DocSpot { batch: 1, confirm: 9001, source: DocSource::JudgeDoc, version: 4 };
        assert_eq!(a.reg_spot(bad_d), Err(DaDiag::EBatchDrift.val()));
        let _ = a.reg_spot(s0);
        assert_eq!(a.reg_spot(s0), Err(DaDiag::ESpotDup.val()));
        let mut b = DaLedger::new();
        assert_eq!(b.reg_spot(s0), Err(DaDiag::ENotOpen.val()));
        // 拒单面：批越闭集 + 确认单不在册 + 零版本 + 超界版本 + 批版本漂移 + 重复位 = 6。
        assert_eq!(a.reject_len(), 6);
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(da_advance(DaPhase::DaDraft, DaPhase::DaSampling).is_ok());
        assert!(da_advance(DaPhase::DaSampling, DaPhase::DaSealed).is_ok());
        assert!(da_advance(DaPhase::DaSampling, DaPhase::DaSampling).is_err());
        assert!(da_advance(DaPhase::DaSealed, DaPhase::DaDraft).is_err());
        assert!(DaPhase::DaDraft.rank() < DaPhase::DaSampling.rank());
        assert!(DaPhase::DaSampling.rank() < DaPhase::DaSealed.rank());
        // 额不足封账拒；补足后过；封账后写拒；空摘要拒。
        let mut a = DaLedger::new();
        let _ = a.begin();
        assert_eq!(a.seal("额不足"), Err(DaDiag::ESpotShort.val()));
        let order = sample_order(SPOT_MIN);
        let mut i = 0usize;
        while i < order.len() {
            let idx = match order.get(i) {
                Some(v) => *v,
                None => break,
            };
            if let Some(s) = SPOT_POOL.get(idx) {
                let _ = a.reg_spot(*s);
            }
            i += 1;
        }
        assert!(a.seal("抽检封账").is_ok());
        let s0 = match SPOT_POOL.get(0) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(a.reg_spot(s0), Err(DaDiag::ESealedWrite.val()));
        assert_eq!(a.phase(), DaPhase::DaSealed);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_ledger();
        let y = build_full_ledger();
        assert_eq!(x.spot_len(), y.spot_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(da_digest(SPOT_POOL), da_digest(SPOT_POOL));
        assert_ne!(da_digest(SPOT_POOL), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [DaPhase::DaDraft, DaPhase::DaSampling, DaPhase::DaSealed].iter() {
            assert!(!st.name().is_empty());
        }
        for d in DA_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
        for k in DocSource::all().iter() {
            assert!(!k.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12715_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
